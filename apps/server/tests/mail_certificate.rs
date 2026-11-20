//! The listener's certificate from Admin → Mail: the renewal rule, the public HTTP-01 answer, the files that win
//! over it, the DNS check before the CA is asked, and the reload that keeps the last good certificate.

use std::path::PathBuf;
use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_mail::TlsAcceptorHandle;
use orbit_platform::{
    AttachmentMutationCoordinator, LocalBlobStore, MailConfig, MailMode, PasswordService,
    TestDatabase, TimestampMillis, UploadLimits, UploadService,
};
use orbit_server::auth_routes::CookieMode;
use orbit_server::mail_certificate::{MailCertificates, renew_after};
use orbit_server::mail_dns::FakeDns;
use orbit_server::mail_listener::MailListenerStatus;
use orbit_server::mail_routes::{MailSettings, MailState, mail_router};
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::repositories::mail::{ChallengeMethod, MailRepository, NewCert, PendingOrder};
use rcgen::{CertificateParams, KeyPair};
use serde_json::{Value, json};
use tower::ServiceExt;

const APP_KEY: [u8; 32] = [7; 32];
const HOSTNAME: &str = "mx.orbit.test";
const DAY: i64 = 24 * 60 * 60;

struct Fixture {
    _root: tempfile::TempDir,
    _database: TestDatabase,
    mail: Arc<MailRepository>,
    dns: FakeDns,
    app: axum::Router,
    certificates: Arc<MailCertificates>,
    tls: Arc<TlsAcceptorHandle>,
    cookie: String,
}

impl Fixture {
    /// `files`: `ORBIT_MAIL_TLS_CERT` / `ORBIT_MAIL_TLS_KEY` are set.
    async fn new(files: bool) -> Self {
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
                    password_hash: PasswordService::default()
                        .hash("correct horse battery")
                        .unwrap(),
                    workspace_name: "Orbit".to_owned(),
                    project_name: "General".to_owned(),
                },
                now,
            )
            .await
            .unwrap();
        let root = tempfile::tempdir().unwrap();
        let uploads = UploadService::new(
            (*database).clone(),
            Arc::new(LocalBlobStore::new(root.path().join("attachments"))),
            AttachmentMutationCoordinator::default(),
            UploadLimits::default(),
        );
        let mail = Arc::new(MailRepository::new((*database).clone(), uploads, APP_KEY));
        let dns = FakeDns::new();
        let config = MailConfig {
            enabled: true,
            hostname: Some(HOSTNAME.to_owned()),
            mode: MailMode::Direct,
            tls_cert: files.then(|| PathBuf::from("/c.pem")),
            tls_key: files.then(|| PathBuf::from("/k.pem")),
            // Unreachable: no test here may reach the CA.
            acme_directory: "https://127.0.0.1:9/dir".to_owned(),
            ..MailConfig::default()
        };
        let tls = Arc::new(TlsAcceptorHandle::empty());
        let certificates = Arc::new(
            MailCertificates::new(
                Arc::clone(&mail),
                &config,
                Arc::new(dns.clone()),
                (!files).then(|| Arc::clone(&tls)),
            )
            .unwrap(),
        );
        let app = mail_router(
            MailState::new(
                Arc::clone(&identity),
                Arc::clone(&mail),
                CookieMode::secure(),
                MailSettings {
                    enabled: true,
                    hostname: Some(HOSTNAME.to_owned()),
                    mode: MailMode::Direct,
                },
            )
            .with_health(
                Arc::new(MailListenerStatus::new(Some(Arc::clone(&tls)))),
                Arc::new(dns.clone()),
            )
            .with_certificates(Arc::clone(&certificates)),
        );
        Self {
            _root: root,
            _database: database,
            mail,
            dns,
            app,
            certificates,
            tls,
            cookie: format!("__Host-orbit_session={}", setup.session.token),
        }
    }

    async fn send(&self, method: &str, path: &str, body: Option<Value>) -> (StatusCode, Value) {
        let request = Request::builder()
            .method(method)
            .uri(path)
            .header(header::COOKIE, &self.cookie)
            .header(header::CONTENT_TYPE, "application/json")
            .body(body.map_or_else(Body::empty, |body| Body::from(body.to_string())))
            .unwrap();
        let response = self.app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    /// The public challenge answer: status, content type and body.
    async fn challenge(&self, token: &str) -> (StatusCode, Option<String>, String) {
        let response = self
            .app
            .clone()
            .oneshot(
                Request::get(format!("/.well-known/acme-challenge/{token}"))
                    .header(header::HOST, HOSTNAME)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .map(|value| value.to_str().unwrap().to_owned());
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (
            status,
            content_type,
            String::from_utf8(bytes.to_vec()).unwrap(),
        )
    }

    async fn store(&self, cert_pem: &str, key_pem: &str) -> i64 {
        self.mail
            .store_certificate(
                &NewCert {
                    cert_pem,
                    key_pem,
                    hostname: HOSTNAME,
                    not_before: 1_000,
                    not_after: 1_000 + 90 * DAY,
                    method: ChallengeMethod::Http,
                },
                None,
                "test",
                TimestampMillis::now(),
            )
            .await
            .unwrap()
    }
}

fn http_order(token: &str) -> PendingOrder {
    PendingOrder::Http {
        url: "https://ca.test/order/1".to_owned(),
        hostname: HOSTNAME.to_owned(),
        token: token.to_owned(),
        key_authorization: format!("{token}.thumbprint"),
        started_at: TimestampMillis::now(),
    }
}

fn self_signed(names: &[&str]) -> (String, String) {
    let key = KeyPair::generate().unwrap();
    let cert = CertificateParams::new(
        names
            .iter()
            .map(|name| (*name).to_owned())
            .collect::<Vec<_>>(),
    )
    .unwrap()
    .self_signed(&key)
    .unwrap();
    (cert.pem(), key.serialize_pem())
}

#[test]
fn renewal_is_due_after_two_thirds_or_30_days_before_expiry_but_not_before_half() {
    assert_eq!(
        renew_after(1_000_000, 1_000_000 + 90 * DAY),
        1_000_000 + 60 * DAY
    );
    // 75 days: 30 days before expiry (day 45) comes before 2/3 (day 50).
    assert_eq!(
        renew_after(1_000_000, 1_000_000 + 75 * DAY),
        1_000_000 + 45 * DAY
    );
    // 45 days: 30 days before expiry would be day 15; half of the lifetime is the floor.
    assert_eq!(
        renew_after(1_000_000, 1_000_000 + 45 * DAY),
        1_000_000 + 45 * DAY / 2
    );
    assert_eq!(
        renew_after(1_000_000, 1_000_000 + 6 * DAY),
        1_000_000 + 3 * DAY
    );
}

#[tokio::test]
async fn challenge_route_answers_only_the_pending_http_token() {
    let fixture = Fixture::new(false).await;
    assert_eq!(fixture.challenge("token-a").await.0, StatusCode::NOT_FOUND);

    fixture
        .mail
        .set_pending_order(&http_order("token-a"))
        .await
        .unwrap();
    assert_eq!(
        fixture.challenge("token-a").await,
        (
            StatusCode::OK,
            Some("text/plain; charset=utf-8".to_owned()),
            "token-a.thumbprint".to_owned()
        )
    );
    assert_eq!(fixture.challenge("token-b").await.0, StatusCode::NOT_FOUND);

    // A newer order replaces the token; a DNS order has none.
    fixture
        .mail
        .set_pending_order(&http_order("token-b"))
        .await
        .unwrap();
    assert_eq!(fixture.challenge("token-a").await.0, StatusCode::NOT_FOUND);
    assert_eq!(fixture.challenge("token-b").await.0, StatusCode::OK);
    fixture
        .mail
        .set_pending_order(&PendingOrder::Dns {
            url: "https://ca.test/order/2".to_owned(),
            hostname: HOSTNAME.to_owned(),
            name: format!("_acme-challenge.{HOSTNAME}"),
            value: "token-b".to_owned(),
            started_at: TimestampMillis::now(),
        })
        .await
        .unwrap();
    assert_eq!(fixture.challenge("token-b").await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn certificate_files_win_over_admin_mail() {
    let fixture = Fixture::new(true).await;
    fixture
        .mail
        .set_pending_order(&http_order("token-a"))
        .await
        .unwrap();
    assert_eq!(fixture.challenge("token-a").await.0, StatusCode::NOT_FOUND);

    let (status, view) = fixture
        .send("GET", "/api/v1/admin/mail/certificate", None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(view["source"], "environment");
    assert_eq!(view["hostname"], HOSTNAME);

    let (status, problem) = fixture
        .send(
            "POST",
            "/api/v1/admin/mail/certificate",
            Some(json!({ "method": "http" })),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(problem["code"], "certificate_from_environment");
    let (status, problem) = fixture
        .send("POST", "/api/v1/admin/mail/certificate/verify", None)
        .await;
    assert_eq!(
        (status, problem["code"].as_str()),
        (StatusCode::CONFLICT, Some("certificate_from_environment"))
    );

    // A stored certificate is not loaded over the files.
    let (cert, key) = self_signed(&[HOSTNAME]);
    fixture.store(&cert, &key).await;
    fixture.certificates.reload_if_changed().await.unwrap();
    assert_eq!(fixture.tls.info(), None);
}

#[tokio::test]
async fn verify_refuses_a_missing_txt_record_before_asking_the_ca() {
    let fixture = Fixture::new(false).await;
    let (status, problem) = fixture
        .send("POST", "/api/v1/admin/mail/certificate/verify", None)
        .await;
    assert_eq!(
        (status, problem["code"].as_str()),
        (StatusCode::CONFLICT, Some("no_dns_order"))
    );

    let name = format!("_acme-challenge.{HOSTNAME}");
    fixture
        .mail
        .set_pending_order(&PendingOrder::Dns {
            url: "https://ca.test/order/1".to_owned(),
            hostname: HOSTNAME.to_owned(),
            name: name.clone(),
            value: "expected-value".to_owned(),
            started_at: TimestampMillis::now(),
        })
        .await
        .unwrap();
    fixture.dns.set_txt(&name, &["some-other-value"]);
    let (status, problem) = fixture
        .send("POST", "/api/v1/admin/mail/certificate/verify", None)
        .await;
    assert_eq!(
        (status, problem["code"].as_str()),
        (StatusCode::CONFLICT, Some("dns_record_not_found"))
    );

    // The order stays for the next try, without an error.
    let (status, view) = fixture
        .send("GET", "/api/v1/admin/mail/certificate", None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(view["source"], "none");
    assert_eq!(
        view["pending"],
        json!({ "method": "dns", "dns_name": name, "dns_value": "expected-value", "started_at": view["pending"]["started_at"] })
    );
    assert_eq!(view["last_error"], Value::Null);

    let (status, view) = fixture
        .send("DELETE", "/api/v1/admin/mail/certificate/order", None)
        .await;
    assert_eq!((status, &view["pending"]), (StatusCode::OK, &Value::Null));
}

#[tokio::test]
async fn reload_loads_a_new_version_and_keeps_the_last_good_certificate() {
    let fixture = Fixture::new(false).await;
    fixture.certificates.reload_if_changed().await.unwrap();
    assert_eq!(fixture.tls.info(), None);

    let (cert, key) = self_signed(&[HOSTNAME]);
    assert_eq!(fixture.store(&cert, &key).await, 1);
    fixture.certificates.reload_if_changed().await.unwrap();
    assert_eq!(fixture.tls.info().unwrap().names, vec![HOSTNAME.to_owned()]);

    let (view_status, view) = fixture
        .send("GET", "/api/v1/admin/mail/certificate", None)
        .await;
    assert_eq!(view_status, StatusCode::OK);
    assert_eq!(view["source"], "orbit");
    assert_eq!(view["active"]["names"], json!([HOSTNAME]));
    assert_eq!(view["active"]["covers_hostname"], true);
    assert_eq!(view["active"]["method"], "http");

    // Version 2 has a key that does not match its certificate: the listener keeps version 1.
    let (other_cert, _) = self_signed(&["mx2.orbit.test"]);
    assert_eq!(fixture.store(&other_cert, &key).await, 2);
    fixture.certificates.reload_if_changed().await.unwrap();
    assert_eq!(fixture.tls.info().unwrap().names, vec![HOSTNAME.to_owned()]);

    let (good_cert, good_key) = self_signed(&["mx3.orbit.test"]);
    assert_eq!(fixture.store(&good_cert, &good_key).await, 3);
    fixture.certificates.reload_if_changed().await.unwrap();
    assert_eq!(
        fixture.tls.info().unwrap().names,
        vec!["mx3.orbit.test".to_owned()]
    );
}
