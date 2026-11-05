//! Request limits for each endpoint class. A request counts against its client IP address, then
//! its credential (the session cookie or the `Authorization` header) and, when it has a credential,
//! the workspace in its path. The first of these over its limit refuses the request, so refused
//! requests do not use up the budget of the subjects after it. The workspace is charged only once
//! the response shows the credential was accepted (a status below 400, [`WorkspaceCharge`]):
//! anyone can send a made-up cookie or token, and it must not use up the budget of the
//! workspace's members.
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
    /// The APIs of integrations: the public REST API (`/api/public/…`) and MCP. Their own budget,
    /// with the general limit, so busy tokens cannot use up the web app's requests.
    TokenApi,
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
            Self::TokenApi => "token_api",
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

/// The workspace a request is charged to, once its response is known ([`RateLimiter::charge`]).
pub(super) struct WorkspaceCharge {
    subject: Subject,
    class: EndpointClass,
    minute: i64,
}

impl WorkspaceCharge {
    /// Whether a response with `status` was served to an accepted credential of the workspace:
    /// only a response below 400 shows that. A made-up credential gets error responses before
    /// it is checked: the router's 405, and the 400, 413, 415 and 422 of the extractors (the web
    /// handlers check the session after their extractors). So no error is charged, also not the
    /// errors of members.
    pub(super) fn applies_to(status: axum::http::StatusCode) -> bool {
        status.as_u16() < 400
    }
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
    ) -> Result<Option<WorkspaceCharge>, Refused> {
        let Some(class) = endpoint_class(method, path) else {
            return Ok(None);
        };
        let limit = self.limit(class);
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let minute = i64::try_from(seconds / 60).unwrap_or(i64::MAX);
        let refused = Refused {
            retry_after: Duration::from_secs(60 - seconds % 60),
        };
        let mut subjects = vec![Subject::Ip(rate_limit_ip(ip))];
        let credential = self.credential(headers);
        if let Some(credential) = credential {
            subjects.push(Subject::Credential(credential));
        }
        for subject in subjects {
            if !self.count(subject, class, limit, minute).await {
                return Err(refused);
            }
        }
        let Some(workspace) = workspace_in(path).filter(|_| credential.is_some()) else {
            return Ok(None);
        };
        let subject = Subject::Workspace(digest(workspace.as_bytes()));
        let workspace_limit = limit.saturating_mul(self.config.workspace_multiplier);
        if self.hits(subject, class, minute).await >= workspace_limit {
            return Err(refused);
        }
        Ok(Some(WorkspaceCharge {
            subject,
            class,
            minute,
        }))
    }

    /// Counts a request whose credential the workspace accepted.
    pub(super) async fn charge(&self, charge: WorkspaceCharge) {
        self.count(charge.subject, charge.class, u32::MAX, charge.minute)
            .await;
    }

    /// The requests counted for `subject` in `minute`, without counting one.
    async fn hits(&self, subject: Subject, class: EndpointClass, minute: i64) -> u32 {
        if class.shared()
            && let Some(database) = &self.shared
        {
            match sqlx::query_scalar::<_, i64>(
                "SELECT hits FROM rate_limit_windows WHERE bucket = $1 AND window_minute = $2",
            )
            .bind(subject.bucket(class))
            .bind(minute)
            .fetch_optional(database.pool())
            .await
            {
                Ok(hits) => return u32::try_from(hits.unwrap_or(0)).unwrap_or(u32::MAX),
                Err(error) => {
                    tracing::warn!(%error, "shared rate limit; counting on this server");
                }
            }
        }
        let entries = self.local.lock().unwrap_or_else(PoisonError::into_inner);
        entries
            .get(&(subject, class))
            .filter(|(window, _)| *window == minute)
            .map_or(0, |(_, hits)| *hits)
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
            EndpointClass::General | EndpointClass::TokenApi => self.config.general_per_minute,
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
            // Still at least half full: forget the quieter half at once, so the next new subjects
            // need no scan. A flood of new subjects then replaces itself, while the subjects near
            // their limit stay counted. A new subject is never refused because the table is full.
            if entries.len() >= MAX_LOCAL_ENTRIES / 2 {
                let mut quiet: Vec<(u32, Key)> = entries
                    .iter()
                    .map(|(key, (_, hits))| (*hits, *key))
                    .collect();
                let half = quiet.len() / 2;
                quiet.select_nth_unstable_by_key(half, |(hits, _)| *hits);
                for (_, key) in &quiet[..half] {
                    entries.remove(key);
                }
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

/// The workspace ID of a `/api/v1/workspaces/{id}/…` or `/api/public/v1/workspaces/{id}/…` path.
fn workspace_in(path: &str) -> Option<&str> {
    path.strip_prefix("/api/v1/workspaces/")
        .or_else(|| path.strip_prefix("/api/public/v1/workspaces/"))?
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
    // Before the rules below: their words (`/session`, `/attachments`, …) can be in any path that
    // a client makes up.
    if path == "/mcp" || path == "/api/public" || path.starts_with("/api/public/") {
        return Some(EndpointClass::TokenApi);
    }
    if path.starts_with("/.well-known/oauth-") {
        return Some(EndpointClass::General);
    }
    if path != "/api" && !path.starts_with("/api/") {
        return None;
    }
    // The provider callback is a GET that calls out to Google or GitHub; the generic rules below miss it.
    if path.starts_with("/api/v1/auth/providers/") {
        return Some(EndpointClass::Authentication);
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
    } else if path.ends_with("/login")
        || path.starts_with("/api/v1/auth/login/")
        || path.starts_with("/api/v1/auth/passkey/login/")
        || path == "/api/v1/auth/password"
        || path.contains("/session")
        || path.contains("/setup")
    {
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

    #[test]
    fn a_full_table_forgets_its_quieter_half_at_once() {
        const MINUTE: i64 = 1;
        let limiter = general(10);
        let key = |host: u32| (Subject::Ip(IpAddr::V4(host.into())), EndpointClass::General);
        for host in 0..MAX_LOCAL_ENTRIES as u32 {
            assert!(limiter.count_local(key(host), 10, MINUTE));
            // Every other subject is busier.
            if host % 2 == 0 {
                assert!(limiter.count_local(key(host), 10, MINUTE));
            }
        }
        assert!(limiter.count_local(key(u32::MAX), 10, MINUTE));
        let entries = limiter.local.lock().unwrap();
        assert_eq!(entries.len(), MAX_LOCAL_ENTRIES / 2 + 1);
        assert!(
            (0..MAX_LOCAL_ENTRIES as u32)
                .step_by(2)
                .all(|host| entries.contains_key(&key(host)))
        );
    }

    #[test]
    fn password_and_second_factor_sign_in_are_authentication_requests() {
        for path in [
            "/api/v1/auth/login",
            "/api/v1/auth/login/two-factor",
            "/api/v1/auth/passkey/login/start",
            "/api/v1/auth/passkey/login/finish",
            "/api/v1/auth/password",
        ] {
            assert_eq!(
                endpoint_class(&Method::POST, path),
                Some(EndpointClass::Authentication),
                "{path}"
            );
        }
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
            let charge = limiter.check(ip, &Method::GET, path, &headers).await;
            // The credentials were accepted.
            limiter.charge(charge.ok().flatten().unwrap()).await;
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
    async fn public_api_tokens_share_their_workspace_budget() {
        let limiter = RateLimiter::new(RateLimitConfig {
            general_per_minute: 2,
            workspace_multiplier: 2,
            ..RateLimitConfig::default()
        });
        let path = "/api/public/v1/workspaces/w1/tasks";
        for client in 0..4_u32 {
            let mut headers = HeaderMap::new();
            headers.insert(
                AUTHORIZATION,
                format!("Bearer orb_{client}").parse().unwrap(),
            );
            let charge = limiter
                .check(IpAddr::V4(client.into()), &Method::GET, path, &headers)
                .await;
            limiter.charge(charge.ok().flatten().unwrap()).await;
        }
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, "Bearer orb_9".parse().unwrap());
        assert!(
            limiter
                .check(IpAddr::V4(9_u32.into()), &Method::GET, path, &headers)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn refused_credentials_do_not_count_against_a_workspace() {
        let limiter = RateLimiter::new(RateLimitConfig {
            general_per_minute: 1,
            workspace_multiplier: 1,
            ..RateLimitConfig::default()
        })
        .with_session_cookie("session");
        let path = "/api/v1/workspaces/w1/tasks";
        // Made-up cookies from many hosts: each answer is a 401, so nothing is charged.
        for host in 0..10_u32 {
            let headers = with_cookie(&format!("made-up-{host}"));
            assert!(
                limiter
                    .check(IpAddr::V4(host.into()), &Method::GET, path, &headers)
                    .await
                    .is_ok()
            );
        }
        assert!(!WorkspaceCharge::applies_to(
            axum::http::StatusCode::UNAUTHORIZED
        ));
        assert!(WorkspaceCharge::applies_to(axum::http::StatusCode::OK));
        let headers = with_cookie("member");
        assert!(
            limiter
                .check(IpAddr::V4(99_u32.into()), &Method::GET, path, &headers)
                .await
                .is_ok()
        );
    }

    #[test]
    fn only_a_response_below_400_charges_the_workspace() {
        use axum::http::StatusCode;
        // A made-up credential gets these before (or without) its check: the router's 405, the
        // extractors' 400, 413, 415 and 422 (web handlers check the session in their body), a
        // socket's 426, the billing guard's 402 and 503. The errors of members are not charged
        // either: their status does not show where they came from.
        for status in [
            400, 401, 402, 403, 404, 405, 409, 413, 415, 422, 426, 500, 503,
        ] {
            let status = StatusCode::from_u16(status).unwrap();
            assert!(!WorkspaceCharge::applies_to(status), "{status}");
        }
        for status in [101, 200, 201, 204, 302, 304] {
            let status = StatusCode::from_u16(status).unwrap();
            assert!(WorkspaceCharge::applies_to(status), "{status}");
        }
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

    #[test]
    fn every_public_api_path_and_mcp_are_token_api_requests() {
        for (method, path) in [
            (Method::GET, "/api/public/v1/me"),
            (Method::GET, "/api/public/v1/workspaces/w/tasks"),
            (Method::POST, "/api/public/v1/workspaces/w/tasks/t/comments"),
            (Method::PATCH, "/api/public/v1/workspaces/w/docs/d"),
            (Method::POST, "/api/public/v1/integrations/discord/events"),
            (Method::GET, "/api/public/v2/anything"),
            (Method::POST, "/mcp"),
            // Made-up paths with the words of the stricter classes stay in the token API class.
            (Method::POST, "/api/public/v1/workspaces/w/docs/session"),
            (Method::POST, "/api/public/v1/workspaces/w/tasks/login"),
            (Method::POST, "/api/public/v1/workspaces/w/setup"),
            (Method::POST, "/api/public/v1/recovery"),
            (Method::POST, "/api/public/v1/auth/register"),
            (Method::POST, "/api/public/v1/workspaces/w/invitations"),
            (
                Method::POST,
                "/api/public/v1/workspaces/w/tasks/t/attachments",
            ),
            (Method::POST, "/api/public/v1/workspaces/w/chat/files"),
            (
                Method::GET,
                "/api/public/v1/workspaces/w/chat/conversations",
            ),
        ] {
            assert_eq!(
                endpoint_class(&method, path),
                Some(EndpointClass::TokenApi),
                "{method} {path}"
            );
        }
        // The web app keeps its classes.
        for (path, class) in [
            ("/api/v1/workspaces/w/tasks", EndpointClass::General),
            (
                "/api/v1/workspaces/w/tasks/t/comments",
                EndpointClass::General,
            ),
            ("/api/v1/auth/session", EndpointClass::Authentication),
            (
                "/.well-known/oauth-authorization-server",
                EndpointClass::General,
            ),
        ] {
            assert_eq!(endpoint_class(&Method::GET, path), Some(class), "{path}");
        }
    }

    #[tokio::test]
    async fn token_api_requests_do_not_use_up_the_web_app_budget() {
        let limiter = RateLimiter::new(RateLimitConfig {
            general_per_minute: 2,
            workspace_multiplier: 1,
            ..RateLimitConfig::default()
        })
        .with_session_cookie("session");
        let client = IpAddr::V4(1_u32.into());
        let mut bearer = HeaderMap::new();
        bearer.insert(AUTHORIZATION, "Bearer orb_busy".parse().unwrap());
        let public = "/api/public/v1/workspaces/w1/tasks";
        for _ in 0..2 {
            let charge = limiter.check(client, &Method::GET, public, &bearer).await;
            limiter.charge(charge.ok().flatten().unwrap()).await;
        }
        // The address, the token and the workspace are at their token API limit.
        assert!(
            limiter
                .check(client, &Method::GET, public, &bearer)
                .await
                .is_err()
        );
        assert!(
            limiter
                .check(client, &Method::POST, "/mcp", &bearer)
                .await
                .is_err()
        );
        // The web app of the same address and workspace has its own budget.
        let web = "/api/v1/workspaces/w1/tasks";
        assert!(
            limiter
                .check(client, &Method::GET, web, &with_cookie("member"))
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

    #[test]
    fn provider_sign_in_routes_are_authentication_requests() {
        for (method, path) in [
            (Method::POST, "/api/v1/auth/providers/google/start"),
            (Method::GET, "/api/v1/auth/providers/github/callback"),
            (Method::POST, "/api/v1/auth/providers/exchange"),
        ] {
            assert_eq!(
                endpoint_class(&method, path),
                Some(EndpointClass::Authentication),
                "{path}"
            );
        }
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
