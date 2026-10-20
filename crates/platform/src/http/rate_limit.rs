//! Request limits for each endpoint class. A request counts against its client IP address, then
//! its credential (the session cookie or the `Authorization` header) and, when it has a credential,
//! the workspace in its path. The first of these over its limit refuses the request, so refused
//! requests do not use up the budget of the subjects after it.
//!
//! The sensitive classes (sign-in, recovery and registration, invitations) are counted in the
//! database (`rate_limit_windows`), so their limits hold for every server of the database. They are
//! rare, and one atomic upsert does not take the database-wide write lock. The other classes are
//! counted in this server's memory: they are most requests, and a database write for each would
//! put every request of the cluster through the one writer. With N servers their limit is N times
//! the setting.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv6Addr};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::http::header::{AUTHORIZATION, COOKIE};
use axum::http::{HeaderMap, Method};
use sha2::{Digest, Sha256};

use crate::{Database, RateLimitConfig};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) enum EndpointClass {
    Authentication,
    Recovery,
    Invitation,
    Upload,
    /// Chat has its own budget, so a busy conversation cannot use up the requests of the
    /// other apps (and they cannot use up chat's).
    Chat,
    General,
}

impl EndpointClass {
    fn name(self) -> &'static str {
        match self {
            Self::Authentication => "authentication",
            Self::Recovery => "recovery",
            Self::Invitation => "invitation",
            Self::Upload => "upload",
            Self::Chat => "chat",
            Self::General => "general",
        }
    }

    /// Whether the class is counted in the database, for every server.
    fn shared(self) -> bool {
        matches!(
            self,
            Self::Authentication | Self::Recovery | Self::Invitation
        )
    }
}

/// Chat makes more small requests than the other apps (a read receipt for each conversation
/// that is looked at), so its budget is this many times the general one.
const CHAT_RATE_FACTOR: u32 = 3;

/// Who a request is counted against. Credentials and workspaces are kept as digests, so the
/// memory of an entry is fixed and no token is kept.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum Subject {
    Ip(IpAddr),
    Credential([u8; 16]),
    Workspace([u8; 16]),
}

impl Subject {
    fn bucket(self, class: EndpointClass) -> String {
        let class = class.name();
        match self {
            Self::Ip(ip) => format!("{class}:ip:{ip}"),
            Self::Credential(digest) => format!("{class}:credential:{}", hex(&digest)),
            Self::Workspace(digest) => format!("{class}:workspace:{}", hex(&digest)),
        }
    }
}

type Key = (Subject, EndpointClass);
/// The minute and the requests counted in it.
type Window = (i64, u32);
pub(super) const MAX_LOCAL_ENTRIES: usize = 16_384;

#[derive(Clone, Debug)]
pub(super) struct RateLimiter {
    config: RateLimitConfig,
    session_cookie: Option<&'static str>,
    local: Arc<Mutex<HashMap<Key, Window>>>,
    shared: Option<Database>,
    /// The minute in which this server last deleted the past windows from the database.
    swept_minute: Arc<AtomicI64>,
}

/// A refused request may be retried after this long (the start of the next minute).
pub(super) struct Refused {
    pub retry_after: Duration,
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new(RateLimitConfig::default())
    }
}

impl RateLimiter {
    pub(super) fn new(config: RateLimitConfig) -> Self {
        Self {
            config,
            session_cookie: None,
            local: Arc::new(Mutex::new(HashMap::new())),
            shared: None,
            swept_minute: Arc::new(AtomicI64::new(0)),
        }
    }

    pub(super) fn with_config(mut self, config: RateLimitConfig) -> Self {
        self.config = config;
        self
    }

    pub(super) fn with_session_cookie(mut self, name: &'static str) -> Self {
        self.session_cookie = Some(name);
        self
    }

    pub(super) fn with_database(mut self, database: Database) -> Self {
        self.shared = Some(database);
        self
    }

    pub(super) async fn check(
        &self,
        ip: IpAddr,
        method: &Method,
        path: &str,
        headers: &HeaderMap,
    ) -> Result<(), Refused> {
        let Some(class) = endpoint_class(method, path) else {
            return Ok(());
        };
        let limit = self.limit(class);
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let minute = i64::try_from(seconds / 60).unwrap_or(i64::MAX);
        let mut subjects = vec![(Subject::Ip(rate_limit_ip(ip)), limit)];
        if let Some(credential) = self.credential(headers) {
            subjects.push((Subject::Credential(credential), limit));
            if let Some(workspace) = workspace_in(path) {
                subjects.push((
                    Subject::Workspace(digest(workspace.as_bytes())),
                    limit.saturating_mul(self.config.workspace_multiplier),
                ));
            }
        }
        for (subject, limit) in subjects {
            if !self.count(subject, class, limit, minute).await {
                return Err(Refused {
                    retry_after: Duration::from_secs(60 - seconds % 60),
                });
            }
        }
        Ok(())
    }

    fn limit(&self, class: EndpointClass) -> u32 {
        match class {
            EndpointClass::Authentication => self.config.authentication_per_minute,
            EndpointClass::Recovery => self.config.recovery_per_minute,
            EndpointClass::Invitation => self.config.invitation_per_minute,
            EndpointClass::Upload => self.config.upload_per_minute,
            EndpointClass::Chat => self
                .config
                .general_per_minute
                .saturating_mul(CHAT_RATE_FACTOR),
            EndpointClass::General => self.config.general_per_minute,
        }
    }

    fn credential(&self, headers: &HeaderMap) -> Option<[u8; 16]> {
        if let Some(authorization) = headers.get(AUTHORIZATION) {
            return Some(digest(authorization.as_bytes()));
        }
        let name = self.session_cookie?;
        headers
            .get_all(COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .flat_map(|value| value.split(';'))
            .find_map(|pair| pair.trim().strip_prefix(name)?.strip_prefix('='))
            .filter(|token| !token.is_empty())
            .map(|token| digest(token.as_bytes()))
    }

    /// Counts the request; `false` when it is over `limit`.
    async fn count(&self, subject: Subject, class: EndpointClass, limit: u32, minute: i64) -> bool {
        if class.shared()
            && let Some(database) = &self.shared
        {
            match self
                .count_shared(database, &subject.bucket(class), minute)
                .await
            {
                Ok(hits) => return hits <= i64::from(limit),
                Err(error) => {
                    tracing::warn!(%error, "shared rate limit; counting on this server");
                }
            }
        }
        self.count_local((subject, class), limit, minute)
    }

    async fn count_shared(
        &self,
        database: &Database,
        bucket: &str,
        minute: i64,
    ) -> Result<i64, sqlx::Error> {
        if self.swept_minute.swap(minute, Ordering::Relaxed) != minute {
            sqlx::query("DELETE FROM rate_limit_windows WHERE window_minute < $1")
                .bind(minute)
                .execute(database.pool())
                .await?;
        }
        sqlx::query_scalar(
            "INSERT INTO rate_limit_windows (bucket, window_minute, hits) VALUES ($1, $2, 1) \
             ON CONFLICT (bucket, window_minute) DO UPDATE SET hits = rate_limit_windows.hits + 1 \
             RETURNING hits",
        )
        .bind(bucket)
        .bind(minute)
        .fetch_one(database.pool())
        .await
    }

    fn count_local(&self, key: Key, limit: u32, minute: i64) -> bool {
        let mut entries = self.local.lock().unwrap_or_else(PoisonError::into_inner);
        if !entries.contains_key(&key) && entries.len() >= MAX_LOCAL_ENTRIES {
            entries.retain(|_, (window, _)| *window == minute);
            // Still full: forget the quietest subject. A flood of new subjects then replaces
            // itself, while the subjects near their limit stay counted. A new subject is never
            // refused because the table is full.
            if entries.len() >= MAX_LOCAL_ENTRIES
                && let Some(quietest) = entries
                    .iter()
                    .min_by_key(|(_, (_, hits))| *hits)
                    .map(|(key, _)| *key)
            {
                entries.remove(&quietest);
            }
        }
        let entry = entries.entry(key).or_insert((minute, 0));
        if entry.0 != minute {
            *entry = (minute, 0);
        }
        if entry.1 >= limit {
            return false;
        }
        entry.1 += 1;
        true
    }
}

fn digest(value: &[u8]) -> [u8; 16] {
    let mut digest = [0; 16];
    digest.copy_from_slice(&Sha256::digest(value)[..16]);
    digest
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn rate_limit_ip(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V4(_) => ip,
        IpAddr::V6(ip) => IpAddr::V6(Ipv6Addr::from(u128::from(ip) & (u128::MAX << 64))),
    }
}

/// The workspace ID of a `/api/v1/workspaces/{id}/…` path.
fn workspace_in(path: &str) -> Option<&str> {
    path.strip_prefix("/api/v1/workspaces/")?
        .split('/')
        .next()
        .filter(|id| !id.is_empty())
}

pub(super) fn endpoint_class(method: &Method, path: &str) -> Option<EndpointClass> {
    if matches!(
        path,
        "/oauth/register" | "/oauth/authorize" | "/oauth/token" | "/oauth/revoke"
    ) {
        return Some(EndpointClass::Authentication);
    }
    if path == "/mcp" || path.starts_with("/.well-known/oauth-") {
        return Some(EndpointClass::General);
    }
    if path != "/api" && !path.starts_with("/api/") {
        return None;
    }
    // Both send email to an address anyone can type, so they share the strict recovery limit.
    if path.contains("/recovery") || path.contains("/auth/register") {
        Some(EndpointClass::Recovery)
    } else if path.contains("/invitations") {
        Some(EndpointClass::Invitation)
    } else if path.contains("/attachments") {
        Some(EndpointClass::Upload)
    } else if method == Method::POST && path.ends_with("/chat/files") {
        // A chat file upload. The downloads (`GET .../chat/files/{id}`) are chat requests.
        Some(EndpointClass::Upload)
    } else if path.ends_with("/login") || path.contains("/session") || path.contains("/setup") {
        Some(EndpointClass::Authentication)
    } else if path.contains("/chat/") {
        Some(EndpointClass::Chat)
    } else {
        Some(EndpointClass::General)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TASKS: &str = "/api/v1/tasks";

    async fn permits(limiter: &RateLimiter, ip: IpAddr, method: &Method, path: &str) -> bool {
        limiter
            .check(ip, method, path, &HeaderMap::new())
            .await
            .is_ok()
    }

    fn with_cookie(token: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            COOKIE,
            format!("theme=dark; session={token}").parse().unwrap(),
        );
        headers
    }

    fn general(limit: u32) -> RateLimiter {
        RateLimiter::new(RateLimitConfig {
            general_per_minute: limit,
            ..RateLimitConfig::default()
        })
    }

    #[tokio::test]
    async fn a_full_table_never_refuses_a_new_client() {
        let limiter = RateLimiter::default();
        for host in 0..(MAX_LOCAL_ENTRIES as u32 + 5_000) {
            assert!(
                permits(&limiter, IpAddr::V4(host.into()), &Method::GET, TASKS).await,
                "client {host} was refused"
            );
        }
        assert!(limiter.local.lock().unwrap().len() <= MAX_LOCAL_ENTRIES);
    }

    #[test]
    fn a_flood_of_new_clients_does_not_reset_an_exhausted_one() {
        // One fixed minute: the flood must not cross into the next window.
        const MINUTE: i64 = 1;
        let limiter = general(2);
        let key = |host: u32| (Subject::Ip(IpAddr::V4(host.into())), EndpointClass::General);
        let exhausted = key(u32::MAX);
        assert!(limiter.count_local(exhausted, 2, MINUTE));
        assert!(limiter.count_local(exhausted, 2, MINUTE));
        assert!(!limiter.count_local(exhausted, 2, MINUTE));
        for host in 0..(MAX_LOCAL_ENTRIES as u32 + 1_000) {
            assert!(limiter.count_local(key(host), 2, MINUTE));
        }
        assert!(!limiter.count_local(exhausted, 2, MINUTE));
    }

    #[tokio::test]
    async fn rate_limit_aggregates_ipv6_clients_by_network_prefix() {
        let limiter = general(1);
        let first = "2001:db8:1:2::1".parse().unwrap();
        let second = "2001:db8:1:2::2".parse().unwrap();
        assert!(permits(&limiter, first, &Method::GET, TASKS).await);
        assert!(!permits(&limiter, second, &Method::GET, TASKS).await);
    }

    #[tokio::test]
    async fn a_credential_is_limited_across_client_addresses() {
        let limiter = general(2).with_session_cookie("session");
        let headers = with_cookie("token-a");
        for host in 0..2_u32 {
            let ip = IpAddr::V4(host.into());
            assert!(
                limiter
                    .check(ip, &Method::GET, TASKS, &headers)
                    .await
                    .is_ok()
            );
        }
        let fresh_ip = IpAddr::V4(9_u32.into());
        assert!(
            limiter
                .check(fresh_ip, &Method::GET, TASKS, &headers)
                .await
                .is_err()
        );
        // Another session from the same address has its own budget.
        assert!(
            limiter
                .check(fresh_ip, &Method::GET, TASKS, &with_cookie("token-b"))
                .await
                .is_ok()
        );
        // So does a bearer token.
        let mut bearer = HeaderMap::new();
        bearer.insert(AUTHORIZATION, "Bearer orbit-token".parse().unwrap());
        assert!(
            limiter
                .check(IpAddr::V4(10_u32.into()), &Method::POST, "/mcp", &bearer)
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn a_workspace_has_a_budget_for_all_its_credentials() {
        let limiter = RateLimiter::new(RateLimitConfig {
            general_per_minute: 2,
            workspace_multiplier: 2,
            ..RateLimitConfig::default()
        })
        .with_session_cookie("session");
        let path = "/api/v1/workspaces/w1/tasks";
        for client in 0..4_u32 {
            let headers = with_cookie(&format!("token-{client}"));
            let ip = IpAddr::V4(client.into());
            assert!(
                limiter
                    .check(ip, &Method::GET, path, &headers)
                    .await
                    .is_ok()
            );
        }
        let headers = with_cookie("token-9");
        let ip = IpAddr::V4(9_u32.into());
        assert!(
            limiter
                .check(ip, &Method::GET, path, &headers)
                .await
                .is_err()
        );
        // Other workspaces are not affected.
        let other = "/api/v1/workspaces/w2/tasks";
        assert!(
            limiter
                .check(ip, &Method::GET, other, &headers)
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn requests_without_a_credential_do_not_count_against_a_workspace() {
        let limiter = RateLimiter::new(RateLimitConfig {
            general_per_minute: 1,
            workspace_multiplier: 1,
            ..RateLimitConfig::default()
        })
        .with_session_cookie("session");
        let path = "/api/v1/workspaces/w1/tasks";
        for host in 0..10_u32 {
            assert!(permits(&limiter, IpAddr::V4(host.into()), &Method::GET, path).await);
        }
        let ip = IpAddr::V4(99_u32.into());
        let headers = with_cookie("member");
        assert!(
            limiter
                .check(ip, &Method::GET, path, &headers)
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn chat_has_its_own_budget_of_three_times_the_general_one() {
        let limiter = general(1);
        let client = IpAddr::V4(1_u32.into());
        let chat = "/api/v1/workspaces/w/chat/conversations/c/read";
        for _ in 0..3 {
            assert!(permits(&limiter, client, &Method::GET, chat).await);
        }
        assert!(!permits(&limiter, client, &Method::GET, chat).await);
        // Exhausted chat leaves the other apps alone.
        assert!(permits(&limiter, client, &Method::GET, "/api/v1/workspaces/w/tasks").await);
    }

    #[tokio::test]
    async fn a_chat_file_upload_is_an_upload_and_a_download_is_a_chat_request() {
        let files = "/api/v1/workspaces/w/chat/files";
        assert_eq!(
            endpoint_class(&Method::POST, files),
            Some(EndpointClass::Upload)
        );
        assert_eq!(
            endpoint_class(&Method::GET, "/api/v1/workspaces/w/chat/files/f"),
            Some(EndpointClass::Chat)
        );
        assert_eq!(
            endpoint_class(&Method::POST, "/api/v1/workspaces/w/chat/read-all"),
            Some(EndpointClass::Chat)
        );

        let limiter = RateLimiter::new(RateLimitConfig {
            upload_per_minute: 1,
            ..RateLimitConfig::default()
        });
        let client = IpAddr::V4(1_u32.into());
        assert!(permits(&limiter, client, &Method::POST, files).await);
        assert!(!permits(&limiter, client, &Method::POST, files).await);
    }

    #[tokio::test]
    async fn sensitive_classes_are_counted_for_every_server_of_the_database() {
        let database = crate::TestDatabase::new().await.unwrap();
        let config = RateLimitConfig {
            recovery_per_minute: 2,
            general_per_minute: 1,
            ..RateLimitConfig::default()
        };
        let one = RateLimiter::new(config).with_database((*database).clone());
        let two = RateLimiter::new(config).with_database((*database).clone());
        let client = IpAddr::V4(1_u32.into());
        let recovery = "/api/v1/auth/recovery/request";
        assert!(permits(&one, client, &Method::POST, recovery).await);
        assert!(permits(&two, client, &Method::POST, recovery).await);
        assert!(!permits(&one, client, &Method::POST, recovery).await);
        assert!(!permits(&two, client, &Method::POST, recovery).await);

        // The general class stays on each server.
        assert!(permits(&one, client, &Method::GET, TASKS).await);
        assert!(permits(&two, client, &Method::GET, TASKS).await);
    }
}
