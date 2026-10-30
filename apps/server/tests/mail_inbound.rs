//! Mail through a provider's webhook (provider intake design §3 P5–P9, §6): authentication, the
//! domain route, unknown addresses, retries, size limits and storage failures.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use base64::Engine as _;
use hmac::{Hmac, Mac};
use orbit_mail::{Authenticator, StaticDns};
use orbit_platform::{
    AttachmentMutationCoordinator, BlobFuture, BlobObject, BlobReader, BlobStore, BlobStoreError,
    Id, JobStore, LocalBlobStore, MailMode, StagedUpload, StoredObject, TestDatabase,
    TimestampMillis, UploadLimits, UploadService,
};
use orbit_server::auth_routes::CookieMode;
use orbit_server::mail_delivery::ProviderIntake;
use orbit_server::mail_routes::{MailSettings, MailState, mail_router};
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::repositories::mail::{
    HealthTestStatus, MailRepository, NewProvider, ProviderKind, ProviderScope, ProviderSecrets,
    ProviderSettings, RejectReason,
};
use serde_json::{Value, json};
use sha2::Sha256;
use tower::ServiceExt;

const APP_KEY: [u8; 32] = [6; 32];
const MAX: u64 = 64 * 1024;
const HOSTNAME: &str = "mx.orbit.test";
const MAILGUN_KEY: &str = "mailgun-signing-key";

/// The local store; `install` fails while `fail` is set (the blob store is down).
#[derive(Clone)]
struct SwitchableStore {
    inner: LocalBlobStore,
    fail: Arc<AtomicBool>,
}

impl BlobStore for SwitchableStore {
    fn create_temporary(&self) -> Result<std::path::PathBuf, BlobStoreError> {
        BlobStore::create_temporary(&self.inner)
    }

    fn install<'a>(&'a self, upload: &'a StagedUpload) -> BlobFuture<'a, StoredObject> {
        if self.fail.load(Ordering::SeqCst) {
            return Box::pin(async {
                Err(BlobStoreError::Io {
                    path: std::path::PathBuf::from("blob"),
                    source: std::io::Error::other("blob store is down"),
                })
            });
        }
        BlobStore::install(&self.inner, upload)
    }

    fn open<'a>(&'a self, storage_key: &'a str) -> BlobFuture<'a, BlobReader> {
        BlobStore::open(&self.inner, storage_key)
    }

    fn delete<'a>(&'a self, storage_key: &'a str) -> BlobFuture<'a, ()> {
        BlobStore::delete(&self.inner, storage_key)
    }

    fn delete_temporary<'a>(&'a self, path: &'a std::path::Path) -> BlobFuture<'a, ()> {
        BlobStore::delete_temporary(&self.inner, path)
    }

    fn blob_modified_at<'a>(&'a self, storage_key: &'a str) -> BlobFuture<'a, Option<i64>> {
        BlobStore::blob_modified_at(&self.inner, storage_key)
    }

    fn blobs(&self) -> BlobFuture<'_, Vec<BlobObject>> {
        BlobStore::blobs(&self.inner)
    }

    fn temporary_files(&self) -> BlobFuture<'_, Vec<BlobObject>> {
        BlobStore::temporary_files(&self.inner)
    }
}

struct Provider {
    id: Id,
    secrets: ProviderSecrets,
}

struct Fixture {
    _root: tempfile::TempDir,
    database: TestDatabase,
    mail: Arc<MailRepository>,
    app: axum::Router,
    fail: Arc<AtomicBool>,
    workspace: Id,
    owner: Id,
    /// `one.test` and `two.test` (both start on the SMTP route).
    one: Id,
    two: Id,
    /// `box@one.test`.
    primary: Id,
}

impl Fixture {
    async fn new() -> Self {
        let database = TestDatabase::new().await.unwrap();
        let identity = Arc::new(IdentityRepository::new((*database).clone()));
        let now = TimestampMillis::now();
        identity
            .store_setup_token(
                "operator-secret",
                TimestampMillis::from_millis(now.as_millis() + 60_000),
            )
            .await
            .unwrap();
        let setup = identity
            .complete_setup(
                SetupRequest {
                    token: "operator-secret".to_owned(),
                    email: "owner@example.com".to_owned(),
                    display_name: "Owner".to_owned(),
                    password_hash: "unused".to_owned(),
                    workspace_name: "Orbit".to_owned(),
                    project_name: "General".to_owned(),
                },
                now,
            )
            .await
            .unwrap();
        let root = tempfile::tempdir().unwrap();
        let fail = Arc::new(AtomicBool::new(false));
        let uploads = UploadService::new(
            (*database).clone(),
            Arc::new(SwitchableStore {
                inner: LocalBlobStore::new(root.path().join("attachments")),
                fail: Arc::clone(&fail),
            }),
            AttachmentMutationCoordinator::default(),
            UploadLimits::default(),
        );
        let mail = Arc::new(MailRepository::new((*database).clone(), uploads, APP_KEY));
        let intake = ProviderIntake::new(
            Arc::clone(&mail),
            JobStore::new((*database).clone()),
            MAX,
            Arc::new(Authenticator::with_static_dns(StaticDns::new()).unwrap()),
            HOSTNAME.to_owned(),
        );
        let app = mail_router(
            MailState::new(
                Arc::clone(&identity),
                Arc::clone(&mail),
                CookieMode::secure(),
                MailSettings {
                    enabled: false,
                    hostname: Some(HOSTNAME.to_owned()),
                    mode: MailMode::Relay,
                },
            )
            .with_inbound(Arc::new(intake)),
        );
        let fixture = Self {
            _root: root,
            database,
            mail,
            app,
            fail,
            workspace: setup.workspace_id,
            owner: setup.user_id,
            one: Id::new_v7(),
            two: Id::new_v7(),
            primary: Id::new_v7(),
        };
        let membership: String =
            sqlx::query_scalar("SELECT id FROM memberships WHERE user_id = $1")
                .bind(fixture.owner.to_string())
                .fetch_one(fixture.database.pool())
                .await
                .unwrap();
        let mailbox = Id::new_v7();
        let at = now.as_millis();
        let ws = fixture.workspace.to_string();
        for (id, name) in [(fixture.one, "one.test"), (fixture.two, "two.test")] {
            fixture
                .execute(
                    &format!(
                        "INSERT INTO mail_domains (id, workspace_id, name, status, verify_token, created_at, updated_at) \
                         VALUES ($1, $2, $3, 'verified', 'token', {at}, {at})"
                    ),
                    &[&id.to_string(), &ws, name],
                )
                .await;
        }
        fixture
            .execute(
                &format!(
                    "INSERT INTO mailboxes (id, workspace_id, kind, name, owner_membership_id, status, created_at, updated_at) \
                     VALUES ($1, $2, 'shared', 'Box', NULL, 'active', {at}, {at})"
                ),
                &[&mailbox.to_string(), &ws],
            )
            .await;
        let alias = Id::new_v7();
        for (id, domain, kind) in [
            (fixture.primary, fixture.one, "primary"),
            (alias, fixture.two, "alias"),
        ] {
            fixture
                .execute(
                    &format!(
                        "INSERT INTO mail_addresses (id, domain_id, mailbox_id, local_part, kind, created_at) \
                         VALUES ($1, $2, $3, 'box', $4, {at})"
                    ),
                    &[&id.to_string(), &domain.to_string(), &mailbox.to_string(), kind],
                )
                .await;
        }
        fixture
            .execute(
                &format!(
                    "INSERT INTO mailbox_members (mailbox_id, membership_id, created_at) VALUES ($1, $2, {at})"
                ),
                &[&mailbox.to_string(), &membership],
            )
            .await;
        fixture.fail.store(false, Ordering::SeqCst);
        fixture
    }

    async fn execute(&self, sql: &str, binds: &[&str]) {
        let mut query = sqlx::query(sqlx::AssertSqlSafe(sql.to_owned()));
        for bind in binds {
            query = query.bind(*bind);
        }
        query.execute(self.database.pool()).await.unwrap();
    }

    async fn count(&self, sql: &str) -> i64 {
        sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_owned()))
            .fetch_one(self.database.pool())
            .await
            .unwrap()
    }

    /// An instance provider of `kind`, and `domains` routed to it.
    async fn provider(&mut self, kind: ProviderKind, domains: &[Id]) -> Provider {
        let secrets = match kind {
            ProviderKind::Mailgun => Some(ProviderSecrets::Mailgun {
                signing_key: MAILGUN_KEY.to_owned(),
            }),
            _ => None,
        };
        let created = self
            .mail
            .create_provider(
                ProviderScope::Instance,
                self.owner,
                NewProvider {
                    kind,
                    name: kind.as_str().to_owned(),
                    secrets: secrets.clone(),
                    settings: ProviderSettings::default(),
                },
                "request",
            )
            .await
            .unwrap();
        for domain in domains {
            self.mail
                .set_domain_route(
                    self.workspace,
                    self.owner,
                    *domain,
                    Some(created.provider.id),
                    "request",
                )
                .await
                .unwrap();
        }
        Provider {
            id: created.provider.id,
            secrets: created.generated.or(secrets).unwrap(),
        }
    }

    async fn post(
        &self,
        uri: &str,
        content_type: &str,
        headers: &[(&str, String)],
        body: Vec<u8>,
    ) -> (StatusCode, Value) {
        let mut request = Request::builder()
            .method("POST")
            .uri(uri)
            .header(header::CONTENT_TYPE, content_type);
        for (name, value) in headers {
            request = request.header(*name, value);
        }
        let response = self
            .app
            .clone()
            .oneshot(request.body(Body::from(body)).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn cloudflare(&self, provider: &Provider, to: &str, raw: &str) -> (StatusCode, Value) {
        let ProviderSecrets::Cloudflare { signing_secret } = &provider.secrets else {
            panic!("not cloudflare");
        };
        let body = json!({
            "from": "ann@sender.example",
            "to": to,
            "raw": base64::engine::general_purpose::STANDARD.encode(raw),
        })
        .to_string();
        let timestamp = (TimestampMillis::now().as_millis() / 1000).to_string();
        let signature = hmac_hex(
            signing_secret.as_bytes(),
            format!("{timestamp}.{body}").as_bytes(),
        );
        self.post(
            &format!("/api/v1/mail/inbound/cloudflare/{}", provider.id),
            "application/json",
            &[
                ("x-orbit-timestamp", timestamp),
                ("x-orbit-signature", signature),
            ],
            body.into_bytes(),
        )
        .await
    }

    async fn postmark(
        &self,
        provider: &Provider,
        auth: bool,
        to: &str,
        raw: &str,
    ) -> (StatusCode, Value) {
        let ProviderSecrets::Postmark {
            basic_user,
            basic_password,
        } = &provider.secrets
        else {
            panic!("not postmark");
        };
        let headers = if auth {
            vec![(
                "authorization",
                format!(
                    "Basic {}",
                    base64::engine::general_purpose::STANDARD
                        .encode(format!("{basic_user}:{basic_password}"))
                ),
            )]
        } else {
            Vec::new()
        };
        let body =
            json!({ "From": "ann@sender.example", "OriginalRecipient": to, "RawEmail": raw });
        self.post(
            &format!("/api/v1/mail/inbound/postmark/{}", provider.id),
            "application/json",
            &headers,
            body.to_string().into_bytes(),
        )
        .await
    }

    async fn mailgun(
        &self,
        provider: &Provider,
        token: &str,
        to: &str,
        raw: &str,
    ) -> (StatusCode, Value) {
        let timestamp = (TimestampMillis::now().as_millis() / 1000).to_string();
        let signature = hmac_hex(
            MAILGUN_KEY.as_bytes(),
            format!("{timestamp}{token}").as_bytes(),
        );
        let body: String = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("timestamp", &timestamp)
            .append_pair("token", token)
            .append_pair("signature", &signature)
            .append_pair("recipient", to)
            .append_pair("sender", "ann@sender.example")
            .append_pair("body-mime", raw)
            .finish();
        self.post(
            &format!("/api/v1/mail/inbound/mailgun/{}/mime", provider.id),
            "application/x-www-form-urlencoded",
            &[],
            body.into_bytes(),
        )
        .await
    }

    async fn receipts(&self) -> Vec<(Id, String, String)> {
        let rows: Vec<(String, String, String)> =
            sqlx::query_as("SELECT id, source, auth FROM mail_receipts ORDER BY id")
                .fetch_all(self.database.pool())
                .await
                .unwrap();
        rows.into_iter()
            .map(|(id, source, auth)| (id.parse().unwrap(), source, auth))
            .collect()
    }
}

fn hmac_hex(key: &[u8], message: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).unwrap();
    mac.update(message);
    mac.finalize()
        .into_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn mail(to: &str, body: &str) -> String {
    format!(
        "Received-SPF: pass (provider) envelope-from=\"ann@sender.example\";\r\n\
         From: Ann <ann@sender.example>\r\nTo: {to}\r\nSubject: Hello\r\n\
         Message-ID: <{body}@sender.example>\r\nDate: Thu, 29 Oct 2026 10:00:00 +0000\r\n\r\n{body}\r\n"
    )
}

#[tokio::test]
async fn cloudflare_delivery_is_stored_and_threaded() {
    let mut fixture = Fixture::new().await;
    let provider = fixture
        .provider(ProviderKind::Cloudflare, &[fixture.one])
        .await;
    let (status, body) = fixture
        .cloudflare(&provider, "box@one.test", &mail("box@one.test", "first"))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let receipts = fixture.receipts().await;
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].1, "cloudflare");
    fixture
        .mail
        .ingest(receipts[0].0, TimestampMillis::now())
        .await
        .unwrap();
    assert_eq!(
        fixture
            .count("SELECT COUNT(*) FROM mail_thread_messages")
            .await,
        1
    );
    let loaded = fixture
        .mail
        .load_provider(provider.id)
        .await
        .unwrap()
        .unwrap()
        .0;
    assert!(loaded.last_received_at.is_some());
}

#[tokio::test]
async fn test_message_arrives_through_a_provider() {
    let mut fixture = Fixture::new().await;
    let provider = fixture
        .provider(ProviderKind::Cloudflare, &[fixture.one])
        .await;
    let test = fixture.mail.create_health_test(fixture.one).await.unwrap();
    let (status, _) = fixture
        .cloudflare(&provider, &test.address, &mail(&test.address, "check"))
        .await;
    assert_eq!(status, StatusCode::OK);
    let test = fixture.mail.health_test(test.id).await.unwrap();
    assert_eq!(test.status, HealthTestStatus::Received);
    assert!(fixture.receipts().await.is_empty());
}

#[tokio::test]
async fn postmark_basic_auth_is_required() {
    let mut fixture = Fixture::new().await;
    let provider = fixture
        .provider(ProviderKind::Postmark, &[fixture.one])
        .await;
    let raw = mail("box@one.test", "pm");
    // Postmark stops retrying only on 403
    let (status, _) = fixture
        .postmark(&provider, false, "box@one.test", &raw)
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(fixture.receipts().await.is_empty());
    let (status, _) = fixture
        .postmark(&provider, true, "box@one.test", &raw)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(fixture.receipts().await[0].1, "postmark");
}

#[tokio::test]
async fn mailgun_reused_token_is_refused() {
    let mut fixture = Fixture::new().await;
    let provider = fixture
        .provider(ProviderKind::Mailgun, &[fixture.one])
        .await;
    let raw = mail("box@one.test", "mg");
    let (status, _) = fixture
        .mailgun(&provider, "token-1", "box@one.test", &raw)
        .await;
    assert_eq!(status, StatusCode::OK);
    // Mailgun does not retry a 406
    let (status, _) = fixture
        .mailgun(&provider, "token-1", "box@one.test", &raw)
        .await;
    assert_eq!(status, StatusCode::NOT_ACCEPTABLE);
    assert_eq!(fixture.receipts().await.len(), 1);
}

#[tokio::test]
async fn unknown_address_is_listed_as_rejected() {
    let mut fixture = Fixture::new().await;
    let cloudflare = fixture
        .provider(ProviderKind::Cloudflare, &[fixture.one])
        .await;
    let (status, body) = fixture
        .cloudflare(
            &cloudflare,
            "nobody@one.test",
            &mail("nobody@one.test", "x"),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body["reject"].as_str().unwrap().contains("nobody@one.test"),
        "{body}"
    );

    let postmark = fixture
        .provider(ProviderKind::Postmark, &[fixture.two])
        .await;
    let (status, _) = fixture
        .postmark(
            &postmark,
            true,
            "typo@two.test",
            &mail("typo@two.test", "y"),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(fixture.receipts().await.is_empty());

    let failed = fixture
        .mail
        .failed_receipts(fixture.workspace, fixture.owner)
        .await
        .unwrap();
    let mut rejected: Vec<(String, RejectReason)> = failed
        .rejected
        .iter()
        .map(|entry| (entry.recipient.clone(), entry.reason))
        .collect();
    rejected.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        rejected,
        vec![
            ("nobody@one.test".to_owned(), RejectReason::UnknownAddress),
            ("typo@two.test".to_owned(), RejectReason::UnknownAddress),
        ]
    );
    assert_eq!(
        failed.rejected[0].mail_from.as_deref(),
        Some("ann@sender.example")
    );
}

#[tokio::test]
async fn provider_retry_is_not_duplicated() {
    let mut fixture = Fixture::new().await;
    let provider = fixture
        .provider(ProviderKind::Cloudflare, &[fixture.one])
        .await;
    let raw = mail("box@one.test", "again");
    for _ in 0..2 {
        let (status, _) = fixture.cloudflare(&provider, "box@one.test", &raw).await;
        assert_eq!(status, StatusCode::OK);
    }
    for (receipt, _, _) in fixture.receipts().await {
        fixture
            .mail
            .ingest(receipt, TimestampMillis::now())
            .await
            .unwrap();
    }
    assert_eq!(
        fixture
            .count("SELECT COUNT(*) FROM mail_thread_messages")
            .await,
        1
    );
}

#[tokio::test]
async fn only_the_routed_domain_gets_the_mail() {
    let mut fixture = Fixture::new().await;
    // two.test stays on the SMTP route
    let provider = fixture
        .provider(ProviderKind::Mailgun, &[fixture.one])
        .await;
    let (status, _) = fixture
        .mailgun(
            &provider,
            "token-2",
            "box@one.test, box@two.test",
            &mail("box@one.test", "two"),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let addresses: Vec<String> =
        sqlx::query_scalar("SELECT address_id FROM mail_receipt_recipients")
            .fetch_all(fixture.database.pool())
            .await
            .unwrap();
    assert_eq!(addresses, vec![fixture.primary.to_string()]);
    let failed = fixture
        .mail
        .failed_receipts(fixture.workspace, fixture.owner)
        .await
        .unwrap();
    assert_eq!(failed.rejected.len(), 1);
    assert_eq!(failed.rejected[0].recipient, "box@two.test");
    assert_eq!(failed.rejected[0].reason, RejectReason::WrongRoute);
}

#[tokio::test]
async fn bodies_over_the_limit_are_refused() {
    let mut fixture = Fixture::new().await;
    let cloudflare = fixture
        .provider(ProviderKind::Cloudflare, &[fixture.one])
        .await;
    let postmark = fixture
        .provider(ProviderKind::Postmark, &[fixture.two])
        .await;
    let big = mail("box@one.test", &"x".repeat(MAX as usize));
    let (status, _) = fixture.cloudflare(&cloudflare, "box@one.test", &big).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    // Postmark: 403, its only "do not retry" answer
    let (status, _) = fixture
        .postmark(&postmark, true, "box@two.test", &big)
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    // a body far past the limit is cut off before it is read
    let (status, _) = fixture
        .post(
            &format!("/api/v1/mail/inbound/cloudflare/{}", cloudflare.id),
            "application/json",
            &[],
            vec![b'x'; (MAX as usize) * 3 + 2 * 1024 * 1024],
        )
        .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert!(fixture.receipts().await.is_empty());
}

#[tokio::test]
async fn deleted_or_wrong_kind_provider_is_404() {
    let mut fixture = Fixture::new().await;
    let provider = fixture.provider(ProviderKind::Cloudflare, &[]).await;
    for uri in [
        format!("/api/v1/mail/inbound/postmark/{}", provider.id),
        format!("/api/v1/mail/inbound/cloudflare/{}", Id::new_v7()),
        "/api/v1/mail/inbound/cloudflare/not-an-id".to_owned(),
        format!("/api/v1/mail/inbound/sendgrid/{}", provider.id),
    ] {
        let (status, _) = fixture
            .post(&uri, "application/json", &[], b"{}".to_vec())
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
    }
}

#[tokio::test]
async fn storage_failure_is_503() {
    let mut fixture = Fixture::new().await;
    let provider = fixture
        .provider(ProviderKind::Cloudflare, &[fixture.one])
        .await;
    fixture.fail.store(true, Ordering::SeqCst);
    let (status, _) = fixture
        .cloudflare(&provider, "box@one.test", &mail("box@one.test", "down"))
        .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(fixture.receipts().await.is_empty());
    let loaded = fixture
        .mail
        .load_provider(provider.id)
        .await
        .unwrap()
        .unwrap()
        .0;
    assert!(loaded.last_error.is_some());
}

#[tokio::test]
async fn provider_spf_reaches_the_receipt_auth() {
    let mut fixture = Fixture::new().await;
    let provider = fixture
        .provider(ProviderKind::Cloudflare, &[fixture.one])
        .await;
    fixture
        .cloudflare(&provider, "box@one.test", &mail("box@one.test", "spf"))
        .await;
    let auth: Value = serde_json::from_str(&fixture.receipts().await[0].2).unwrap();
    assert_eq!(auth["spf"], "pass", "{auth}");
    assert_eq!(auth["spf_domain"], "sender.example");
}
