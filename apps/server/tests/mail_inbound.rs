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
    Id, LocalBlobStore, MailMode, StagedUpload, StoredObject, TestDatabase, TimestampMillis,
    UploadLimits, UploadService,
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
const RESEND_KEY: &str = "re_test_key";
const POSTSHIBA_KEY: &str = "ps_test_key";
const POSTSHIBA_SECRET: &str = "postshiba-inbox-secret";
const SVIX_KEY: &[u8] = b"resend-svix-key-0123456789abcdef";
const TOPIC: &str = "arn:aws:sns:us-east-1:123456789012:orbit-mail";
const SNS_HOST: &str = "sns.us-east-1.amazonaws.com";

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
        Self::with(|intake| intake).await
    }

    /// `customize` adds the test seams (local hosts for fetches, a local S3).
    async fn with(customize: impl FnOnce(ProviderIntake) -> ProviderIntake) -> Self {
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
            MAX,
            Arc::new(Authenticator::with_static_dns(StaticDns::new()).unwrap()),
            HOSTNAME.to_owned(),
        );
        let intake = customize(intake);
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
        self.provider_with(kind, domains, ProviderSettings::default())
            .await
    }

    async fn provider_with(
        &mut self,
        kind: ProviderKind,
        domains: &[Id],
        settings: ProviderSettings,
    ) -> Provider {
        let secrets = match kind {
            ProviderKind::Mailgun => Some(ProviderSecrets::Mailgun {
                signing_key: MAILGUN_KEY.to_owned(),
            }),
            ProviderKind::Postshiba => Some(ProviderSecrets::Postshiba {
                api_key: POSTSHIBA_KEY.to_owned(),
                webhook_secret: POSTSHIBA_SECRET.to_owned(),
            }),
            ProviderKind::Resend => Some(ProviderSecrets::Resend {
                api_key: RESEND_KEY.to_owned(),
                webhook_secret: format!(
                    "whsec_{}",
                    base64::engine::general_purpose::STANDARD.encode(SVIX_KEY)
                ),
            }),
            ProviderKind::Ses => Some(ProviderSecrets::Ses {
                access_key_id: "AKIDEXAMPLE".to_owned(),
                secret_access_key: "secret".to_owned(),
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
                    settings,
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
        let timestamp = (TimestampMillis::now().as_millis() / 1000).to_string();
        let from = "ann%40sender.example".to_owned();
        let to = to.replace('@', "%40");
        let digest: String = <Sha256 as sha2::Digest>::digest(raw.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let signature = hmac_hex(
            signing_secret.as_bytes(),
            format!("{timestamp}.{from}.{to}.{digest}").as_bytes(),
        );
        self.post(
            &format!("/api/v1/mail/inbound/cloudflare/{}", provider.id),
            "application/octet-stream",
            &[
                ("x-orbit-timestamp", timestamp),
                ("x-orbit-signature", signature),
                ("x-orbit-from", from),
                ("x-orbit-to", to),
            ],
            raw.as_bytes().to_vec(),
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
        self.mailgun_at(
            provider,
            token,
            to,
            raw,
            TimestampMillis::now().as_millis() / 1000,
        )
        .await
    }

    /// Mailgun's form, multipart as some integrations send it.
    async fn mailgun_multipart(
        &self,
        provider: &Provider,
        token: &str,
        to: &str,
        raw: &[u8],
    ) -> (StatusCode, Value) {
        let timestamp = (TimestampMillis::now().as_millis() / 1000).to_string();
        let signature = hmac_hex(
            MAILGUN_KEY.as_bytes(),
            format!("{timestamp}{token}").as_bytes(),
        );
        let mut body = Vec::new();
        for (name, value) in [
            ("timestamp", timestamp.as_bytes()),
            ("token", token.as_bytes()),
            ("signature", signature.as_bytes()),
            ("recipient", to.as_bytes()),
            ("sender", b"ann@sender.example".as_slice()),
            ("body-mime", raw),
        ] {
            body.extend_from_slice(
                format!("--XyZ\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n")
                    .as_bytes(),
            );
            body.extend_from_slice(value);
            body.extend_from_slice(b"\r\n");
        }
        body.extend_from_slice(b"--XyZ--\r\n");
        self.post(
            &format!("/api/v1/mail/inbound/mailgun/{}/mime", provider.id),
            "multipart/form-data; boundary=XyZ",
            &[],
            body,
        )
        .await
    }

    async fn mailgun_at(
        &self,
        provider: &Provider,
        token: &str,
        to: &str,
        raw: &str,
        timestamp: i64,
    ) -> (StatusCode, Value) {
        let timestamp = timestamp.to_string();
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
         Received: from mx.sender.example by mx.provider.example;\r\n\
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
    let from_cloudflare = failed
        .rejected
        .iter()
        .find(|entry| entry.recipient == "nobody@one.test")
        .unwrap();
    assert_eq!(
        from_cloudflare.mail_from.as_deref(),
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
    // a body far past the limit is cut off before it is read (the signature is checked after)
    let (status, _) = fixture
        .post(
            &format!("/api/v1/mail/inbound/cloudflare/{}", cloudflare.id),
            "application/json",
            &[
                ("x-orbit-timestamp", "1".to_owned()),
                ("x-orbit-signature", "00".to_owned()),
                ("x-orbit-from", String::new()),
                ("x-orbit-to", "box%40one.test".to_owned()),
            ],
            vec![b'x'; (MAX as usize) + 128 * 1024],
        )
        .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    // without the signature headers nothing is read at all
    let (status, _) = fixture
        .post(
            &format!("/api/v1/mail/inbound/cloudflare/{}", cloudflare.id),
            "application/json",
            &[],
            vec![b'x'; 1024],
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
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

// ---------------------------------------------------------------- Resend and SES (fetching)

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Mutex, OnceLock};

use axum::response::IntoResponse;
use orbit_server::mail_image_proxy::SafeFetch;
use rcgen::SigningKey as _;

/// A local server for every provider host; `deleted` collects S3 deletes, `confirmed` SNS confirmations.
#[derive(Clone, Default)]
struct Fake {
    deleted: Arc<Mutex<Vec<String>>>,
    confirmed: Arc<Mutex<u32>>,
}

async fn fake_hosts(fake: Fake) -> SocketAddr {
    use axum::extract::{Path, Query};
    use axum::routing::get;
    let deleted = Arc::clone(&fake.deleted);
    let confirmed = Arc::clone(&fake.confirmed);
    let router = axum::Router::new()
        .route(
            "/emails/receiving/{id}",
            get(|Path(id): Path<String>, headers: axum::http::HeaderMap| async move {
                if headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok())
                    != Some(&format!("Bearer {RESEND_KEY}"))
                {
                    return StatusCode::UNAUTHORIZED.into_response();
                }
                let url = match id.as_str() {
                    "private" => "https://127.0.0.1:9/raw".to_owned(),
                    "ten" => "https://10.0.0.1/raw".to_owned(),
                    "metadata" => "https://download.test/to-metadata".to_owned(),
                    "broken" => "https://download.test/broken".to_owned(),
                    _ => "https://download.test/raw/1".to_owned(),
                };
                axum::Json(json!({ "object": "email", "id": id, "raw": { "download_url": url, "expires_at": "2026-10-30T12:00:00Z" } })).into_response()
            }),
        )
        .route(
            "/api/v1/inboxes/{inbox}/inbound_messages/{id}",
            get(|Path((inbox, id)): Path<(String, String)>, headers: axum::http::HeaderMap| async move {
                if headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok())
                    != Some(&format!("Bearer {POSTSHIBA_KEY}"))
                    || inbox != "PqRzMn"
                {
                    return StatusCode::UNAUTHORIZED.into_response();
                }
                match id.as_str() {
                    "broken" => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
                    // what a key of a user who is not a team admin gets
                    "masked" => axum::Json(json!({ "id": id, "raw": null })).into_response(),
                    _ => axum::Json(json!({ "id": id, "inbox_id": inbox, "raw": mail("box@one.test", "postshiba") })).into_response(),
                }
            }),
        )
        .route("/raw/1", get(|| async { mail("box@one.test", "resend") }))
        .route("/broken", get(|| async { StatusCode::INTERNAL_SERVER_ERROR }))
        .route(
            "/to-metadata",
            get(|| async {
                (StatusCode::FOUND, [(header::LOCATION, "http://169.254.169.254/latest/")]).into_response()
            }),
        )
        .route("/SimpleNotificationService-test.pem", get(|| async { sns_signer().0.clone() }))
        .route(
            "/",
            get(move |Query(query): Query<HashMap<String, String>>| {
                let confirmed = Arc::clone(&confirmed);
                async move {
                    if query.get("Action").map(String::as_str) == Some("ConfirmSubscription") {
                        *confirmed.lock().unwrap() += 1;
                    }
                    StatusCode::OK
                }
            }),
        )
        .route(
            "/orbit-mail/{*key}",
            get(|Path(key): Path<String>| async move {
                if key == "inbound/o1" {
                    mail("box@one.test", "ses").into_response()
                } else {
                    StatusCode::NOT_FOUND.into_response()
                }
            })
            .delete(move |Path(key): Path<String>| {
                let deleted = Arc::clone(&deleted);
                async move {
                    deleted.lock().unwrap().push(key);
                    StatusCode::NO_CONTENT
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    address
}

async fn fetching_fixture(fake: Fake) -> Fixture {
    let address = fake_hosts(fake).await;
    Fixture::with(move |intake| {
        let hosts = [
            "api.resend.com",
            "app.postshiba.com",
            "download.test",
            SNS_HOST,
        ]
        .into_iter()
        .map(|host| (host.to_owned(), address))
        .collect();
        intake
            .with_fetch(SafeFetch::with_test_hosts(hosts))
            .with_s3_endpoint(format!("http://{address}"))
    })
    .await
}

fn svix_headers(body: &str) -> Vec<(&'static str, String)> {
    let timestamp = TimestampMillis::now().as_millis() / 1000;
    let mut mac = Hmac::<Sha256>::new_from_slice(SVIX_KEY).unwrap();
    mac.update(format!("msg_1.{timestamp}.{body}").as_bytes());
    vec![
        ("svix-id", "msg_1".to_owned()),
        ("svix-timestamp", timestamp.to_string()),
        (
            "svix-signature",
            format!(
                "v1,{}",
                base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes())
            ),
        ),
    ]
}

async fn resend(fixture: &Fixture, provider: &Provider, email_id: &str) -> (StatusCode, Value) {
    let body = json!({
        "type": "email.received",
        "data": { "email_id": email_id, "from": "ann@sender.example", "to": ["box@one.test"], "received_for": ["box@one.test"] }
    })
    .to_string();
    fixture
        .post(
            &format!("/api/v1/mail/inbound/resend/{}", provider.id),
            "application/json",
            &svix_headers(&body),
            body.into_bytes(),
        )
        .await
}

async fn postshiba(
    fixture: &Fixture,
    provider: &Provider,
    message_id: &str,
    secret: &str,
) -> (StatusCode, Value) {
    let body = json!({
        "id": message_id,
        "inbox_id": "PqRzMn",
        "to": "box@one.test",
        "from": "ann@sender.example",
        "envelope": { "mail_from": "ann@sender.example", "rcpt_to": "box@one.test", "plus_tag": null },
        "authentication": { "spf": { "result": "pass", "domain": "sender.example" } },
        "attachments": []
    })
    .to_string();
    let timestamp = (TimestampMillis::now().as_millis() / 1000).to_string();
    let signature = hmac_hex(secret.as_bytes(), format!("{timestamp}.{body}").as_bytes());
    fixture
        .post(
            &format!("/api/v1/mail/inbound/postshiba/{}", provider.id),
            "application/json",
            &[
                ("x-capsule-timestamp", timestamp),
                ("x-capsule-signature", format!("sha256={signature}")),
            ],
            body.into_bytes(),
        )
        .await
}

#[tokio::test]
async fn postshiba_webhook_fetches_and_stores_the_raw_message() {
    let mut fixture = fetching_fixture(Fake::default()).await;
    let provider = fixture
        .provider(ProviderKind::Postshiba, &[fixture.one])
        .await;
    let (status, body) = postshiba(&fixture, &provider, "GxTyVu", POSTSHIBA_SECRET).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["stored"], 1);
    let receipts = fixture.receipts().await;
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].1, "postshiba");
    assert!(
        receipts[0].2.contains(r#""spf":"pass""#),
        "{}",
        receipts[0].2
    );
}

#[tokio::test]
async fn postshiba_webhook_with_a_wrong_signature_is_refused() {
    let mut fixture = fetching_fixture(Fake::default()).await;
    let provider = fixture
        .provider(ProviderKind::Postshiba, &[fixture.one])
        .await;
    let (status, _) = postshiba(&fixture, &provider, "GxTyVu", "another-secret").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(fixture.receipts().await.is_empty());
}

#[tokio::test]
async fn postshiba_without_the_raw_message_is_503_so_postshiba_retries() {
    let mut fixture = fetching_fixture(Fake::default()).await;
    let provider = fixture
        .provider(ProviderKind::Postshiba, &[fixture.one])
        .await;
    for message_id in ["masked", "broken"] {
        let (status, _) = postshiba(&fixture, &provider, message_id, POSTSHIBA_SECRET).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{message_id}");
    }
    assert!(fixture.receipts().await.is_empty());
}

#[tokio::test]
async fn resend_webhook_fetches_and_stores_the_raw_message() {
    let mut fixture = fetching_fixture(Fake::default()).await;
    let provider = fixture.provider(ProviderKind::Resend, &[fixture.one]).await;
    let (status, body) = resend(&fixture, &provider, "56761188").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let receipts = fixture.receipts().await;
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].1, "resend");
}

#[tokio::test]
async fn resend_download_failure_is_503_and_stores_nothing() {
    let mut fixture = fetching_fixture(Fake::default()).await;
    let provider = fixture.provider(ProviderKind::Resend, &[fixture.one]).await;
    let (status, _) = resend(&fixture, &provider, "broken").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(fixture.receipts().await.is_empty());
}

/// An RSA certificate like SNS's and its key (made once: RSA key generation is slow in debug).
fn sns_signer() -> &'static (String, rcgen::KeyPair) {
    static SIGNER: OnceLock<(String, rcgen::KeyPair)> = OnceLock::new();
    SIGNER.get_or_init(|| {
        let key = orbit_mail::generate_dkim_key();
        let pair = rcgen::KeyPair::from_pkcs8_der_and_sign_algo(
            &rustls_pki_types::PrivatePkcs8KeyDer::from(key.private_pkcs8_der.clone()),
            &rcgen::PKCS_RSA_SHA256,
        )
        .unwrap();
        let cert = rcgen::CertificateParams::new(vec![SNS_HOST.to_owned()])
            .unwrap()
            .self_signed(&pair)
            .unwrap();
        (cert.pem(), pair)
    })
}

fn sns_body(fields: &[(&str, String)], cert_url: &str, topic: &str) -> String {
    let fields: Vec<(&str, String)> = fields
        .iter()
        .map(|(name, value)| {
            (
                *name,
                if *name == "TopicArn" {
                    topic.to_owned()
                } else {
                    value.clone()
                },
            )
        })
        .collect();
    let mut text = String::new();
    for (name, value) in &fields {
        text.push_str(&format!("{name}\n{value}\n"));
    }
    let signature = sns_signer().1.sign(text.as_bytes()).unwrap();
    let mut body = json!({
        "SignatureVersion": "2",
        "Signature": base64::engine::general_purpose::STANDARD.encode(signature),
        "SigningCertURL": cert_url,
    });
    for (name, value) in fields {
        body[name] = value.into();
    }
    body.to_string()
}

/// An SNS `Timestamp` of now.
fn sns_now() -> String {
    chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string()
}

const CERT_URL: &str = "https://sns.us-east-1.amazonaws.com/SimpleNotificationService-test.pem";

fn ses_notification_fields(key: &str) -> Vec<(&'static str, String)> {
    let message = json!({
        "notificationType": "Received",
        "mail": { "source": "ann@sender.example", "messageId": "o1" },
        "receipt": {
            "recipients": ["box@one.test"],
            "spfVerdict": { "status": "PASS" },
            "action": { "type": "S3", "bucketName": "orbit-mail", "objectKey": key }
        }
    })
    .to_string();
    vec![
        ("Message", message),
        ("MessageId", "m-1".to_owned()),
        ("Timestamp", sns_now()),
        ("TopicArn", TOPIC.to_owned()),
        ("Type", "Notification".to_owned()),
    ]
}

fn ses_settings() -> ProviderSettings {
    ProviderSettings {
        region: Some("us-east-1".to_owned()),
        bucket: Some("orbit-mail".to_owned()),
        topic_arn: Some(TOPIC.to_owned()),
        ..ProviderSettings::default()
    }
}

async fn ses(fixture: &Fixture, provider: &Provider, body: String) -> (StatusCode, Value) {
    fixture
        .post(
            &format!("/api/v1/mail/inbound/ses/{}", provider.id),
            "text/plain; charset=UTF-8",
            &[],
            body.into_bytes(),
        )
        .await
}

#[tokio::test]
async fn ses_subscription_is_confirmed() {
    let fake = Fake::default();
    let mut fixture = fetching_fixture(fake.clone()).await;
    let provider = fixture
        .provider_with(ProviderKind::Ses, &[fixture.one], ses_settings())
        .await;
    let fields = vec![
        ("Message", "You have chosen to subscribe".to_owned()),
        ("MessageId", "m-2".to_owned()),
        (
            "SubscribeURL",
            format!("https://{SNS_HOST}/?Action=ConfirmSubscription&TopicArn={TOPIC}&Token=t"),
        ),
        ("Timestamp", sns_now()),
        ("Token", "t".to_owned()),
        ("TopicArn", TOPIC.to_owned()),
        ("Type", "SubscriptionConfirmation".to_owned()),
    ];
    let (status, body) = ses(&fixture, &provider, sns_body(&fields, CERT_URL, TOPIC)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(*fake.confirmed.lock().unwrap(), 1);
}

#[tokio::test]
async fn ses_notification_reads_s3_and_deletes_the_object() {
    let fake = Fake::default();
    let mut fixture = fetching_fixture(fake.clone()).await;
    let provider = fixture
        .provider_with(ProviderKind::Ses, &[fixture.one], ses_settings())
        .await;
    let body = sns_body(&ses_notification_fields("inbound/o1"), CERT_URL, TOPIC);
    let (status, reply) = ses(&fixture, &provider, body).await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    let receipts = fixture.receipts().await;
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].1, "ses");
    let auth: Value = serde_json::from_str(&receipts[0].2).unwrap();
    assert_eq!(auth["spf"], "pass");
    assert_eq!(*fake.deleted.lock().unwrap(), vec!["inbound/o1".to_owned()]);
}

#[tokio::test]
async fn ses_bad_signature_or_another_topic_is_401() {
    let mut fixture = fetching_fixture(Fake::default()).await;
    let provider = fixture
        .provider_with(ProviderKind::Ses, &[fixture.one], ses_settings())
        .await;
    let forged = sns_body(&ses_notification_fields("inbound/o1"), CERT_URL, TOPIC)
        .replace("inbound/o1", "inbound/o2");
    assert_eq!(
        ses(&fixture, &provider, forged).await.0,
        StatusCode::UNAUTHORIZED
    );
    // a valid SNS message from a topic this provider was not set up with
    let other = sns_body(
        &ses_notification_fields("inbound/o1"),
        CERT_URL,
        "arn:aws:sns:us-east-1:999999999999:attacker",
    );
    assert_eq!(
        ses(&fixture, &provider, other).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert!(fixture.receipts().await.is_empty());
}

#[tokio::test]
async fn fetch_targets_must_be_public_provider_hosts() {
    let mut fixture = fetching_fixture(Fake::default()).await;
    let resend_provider = fixture.provider(ProviderKind::Resend, &[fixture.one]).await;
    for email_id in ["private", "ten", "metadata"] {
        let (status, _) = resend(&fixture, &resend_provider, email_id).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{email_id}");
    }
    let ses_provider = fixture
        .provider_with(ProviderKind::Ses, &[fixture.two], ses_settings())
        .await;
    let elsewhere = sns_body(
        &ses_notification_fields("inbound/o1"),
        "https://evil.example/SimpleNotificationService-test.pem",
        TOPIC,
    );
    assert_eq!(
        ses(&fixture, &ses_provider, elsewhere).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert!(fixture.receipts().await.is_empty());
}

// ---------------------------------------------------------------- review fixes

#[tokio::test]
async fn mailgun_retry_after_a_failure_is_stored() {
    let mut fixture = Fixture::new().await;
    let provider = fixture
        .provider(ProviderKind::Mailgun, &[fixture.one])
        .await;
    let raw = mail("box@one.test", "retry");
    let at = TimestampMillis::now().as_millis() / 1000;
    fixture.fail.store(true, Ordering::SeqCst);
    let (status, _) = fixture
        .mailgun_at(&provider, "token-r", "box@one.test", &raw, at)
        .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    // Mailgun sends the same signed request again later
    fixture.fail.store(false, Ordering::SeqCst);
    let (status, body) = fixture
        .mailgun_at(&provider, "token-r", "box@one.test", &raw, at)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(fixture.receipts().await.len(), 1);
}

#[tokio::test]
async fn mailgun_multipart_form_is_read() {
    let mut fixture = Fixture::new().await;
    let provider = fixture
        .provider(ProviderKind::Mailgun, &[fixture.one])
        .await;
    let mut raw = mail("box@one.test", "latin").into_bytes();
    raw.extend_from_slice(b"caf\xe9\r\n");
    let (status, body) = fixture
        .mailgun_multipart(&provider, "token-m", "box@one.test", &raw)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(fixture.receipts().await.len(), 1);
}

#[tokio::test]
async fn small_payload_providers_refuse_big_bodies() {
    let mut fixture = Fixture::new().await;
    let provider = fixture.provider(ProviderKind::Resend, &[fixture.one]).await;
    // a Resend webhook is a small JSON event: a big body is refused before it is read in full
    let (status, _) = fixture
        .post(
            &format!("/api/v1/mail/inbound/resend/{}", provider.id),
            "application/json",
            &[("svix-signature", "v1,AA==".to_owned())],
            vec![b'x'; 512 * 1024],
        )
        .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn a_workspace_provider_cannot_fill_another_workspaces_list() {
    let fixture = Fixture::new().await;
    let other = orbit_server::repositories::workspaces::WorkspaceRepository::new(
        (*fixture.database).clone(),
    )
    .create(
        fixture.owner,
        "Other".to_owned(),
        "request",
        TimestampMillis::now(),
    )
    .await
    .unwrap()
    .id;
    let created = fixture
        .mail
        .create_provider(
            ProviderScope::Workspace(other),
            fixture.owner,
            NewProvider {
                kind: ProviderKind::Cloudflare,
                name: "Other's".to_owned(),
                secrets: None,
                settings: ProviderSettings::default(),
            },
            "request",
        )
        .await
        .unwrap();
    let provider = Provider {
        id: created.provider.id,
        secrets: created.generated.unwrap(),
    };
    // a signed request of the other workspace's provider names a recipient of this workspace
    let (status, _) = fixture
        .cloudflare(&provider, "box@one.test", &mail("box@one.test", "spam"))
        .await;
    assert_eq!(status, StatusCode::OK);
    let ours = fixture
        .mail
        .failed_receipts(fixture.workspace, fixture.owner)
        .await
        .unwrap();
    assert!(ours.rejected.is_empty(), "{:?}", ours.rejected);
    let theirs = fixture
        .mail
        .failed_receipts(other, fixture.owner)
        .await
        .unwrap();
    assert_eq!(theirs.rejected.len(), 1);
    assert_eq!(theirs.rejected[0].reason, RejectReason::WrongRoute);
}

#[tokio::test]
async fn unverified_requests_leave_the_provider_status_alone() {
    let mut fixture = fetching_fixture(Fake::default()).await;
    let cloudflare = fixture
        .provider(ProviderKind::Cloudflare, &[fixture.one])
        .await;
    let ses_provider = fixture
        .provider_with(ProviderKind::Ses, &[fixture.two], ses_settings())
        .await;
    let status = |id| {
        let mail = Arc::clone(&fixture.mail);
        async move { mail.load_provider(id).await.unwrap().unwrap().0.last_error }
    };
    // anyone can post to the URL: a bad signature is refused and not noted
    let (code, _) = fixture
        .post(
            &format!("/api/v1/mail/inbound/cloudflare/{}", cloudflare.id),
            "application/octet-stream",
            &[
                ("x-orbit-timestamp", "1".to_owned()),
                ("x-orbit-signature", "00".to_owned()),
                ("x-orbit-from", String::new()),
                ("x-orbit-to", "box%40one.test".to_owned()),
            ],
            b"Subject: x\r\n\r\nx\r\n".to_vec(),
        )
        .await;
    assert_eq!(code, StatusCode::UNAUTHORIZED);
    assert_eq!(status(cloudflare.id).await, None);
    // an SNS message from before the window is a replay, also before its certificate is fetched
    let mut old = ses_notification_fields("inbound/o1");
    old[2].1 = "2020-01-01T00:00:00.000Z".to_owned();
    let (code, _) = ses(&fixture, &ses_provider, sns_body(&old, CERT_URL, TOPIC)).await;
    assert_eq!(code, StatusCode::UNAUTHORIZED);
    // a certificate that cannot be fetched is not noted either (nothing is verified yet)
    let (code, _) = ses(
        &fixture,
        &ses_provider,
        sns_body(
            &ses_notification_fields("inbound/o1"),
            "https://sns.us-east-1.amazonaws.com/SimpleNotificationService-missing.pem",
            TOPIC,
        ),
    )
    .await;
    assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(status(ses_provider.id).await, None);
    assert!(fixture.receipts().await.is_empty());
}

#[tokio::test]
async fn ses_retry_after_the_object_is_gone_is_ok() {
    let fake = Fake::default();
    let mut fixture = fetching_fixture(fake.clone()).await;
    let provider = fixture
        .provider_with(ProviderKind::Ses, &[fixture.one], ses_settings())
        .await;
    // SNS sends the notification again; the first one stored the message and deleted the object
    let body = sns_body(&ses_notification_fields("inbound/gone"), CERT_URL, TOPIC);
    let (status, reply) = ses(&fixture, &provider, body).await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert!(fixture.receipts().await.is_empty());
    let loaded = fixture
        .mail
        .load_provider(provider.id)
        .await
        .unwrap()
        .unwrap()
        .0;
    assert_eq!(loaded.last_error, None);
}
