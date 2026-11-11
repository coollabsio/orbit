//! Mail content (spec §6.5, §7 Content, §8.3, §10): the short-lived view URL bound to the session
//! and the message, the sandboxed HTML document with its exact CSP, inline `cid:` parts, the image
//! proxy (signed, bound to the session, refusing private addresses on every hop), attachment and
//! raw downloads, and "Always load images from this sender". Anyone who does not read the mailbox
//! gets `mail_not_found`.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Request, StatusCode, header};
use axum::response::IntoResponse;
use orbit_mail::{AuthOutcome, DmarcPolicy, DmarcVerdict, Verdict};
use orbit_platform::{
    AttachmentMutationCoordinator, AuthenticatedUser, Id, LocalBlobStore, MailMode,
    PasswordService, TestDatabase, TimestampMillis, UploadLimits, UploadService,
};
use orbit_server::auth_routes::CookieMode;
use orbit_server::mail_image_proxy::{ImageProxy, ImageProxyError};
use orbit_server::mail_intake::{InboundMessage, MailIntake, Recipient, Source};
use orbit_server::mail_routes::{
    MailSettings, MailState, ViewGrant, ViewKey, ViewPurpose, mail_router,
};
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::repositories::instance_settings::InstanceSettingsRepository;
use orbit_server::repositories::mail::{IngestOutcome, MailRepository, MailboxKind, NewMailbox};
use serde_json::{Value, json};
use tower::ServiceExt;

const APP_KEY: [u8; 32] = [23; 32];
const ORIGIN: &str = "https://orbit.test";
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDRpng-bytes";

#[derive(Clone)]
struct Member {
    id: Id,
    session_id: Id,
    cookie: String,
}

struct Fixture {
    _root: tempfile::TempDir,
    database: TestDatabase,
    identity: Arc<IdentityRepository>,
    mail: Arc<MailRepository>,
    intake: MailIntake,
    workspace: Id,
    owner: Member,
    /// A member of `team@one.test`.
    ann: Member,
    /// A workspace member who reads no mailbox.
    bob: Member,
    /// `team@one.test`: the owner and Ann; its primary address.
    mailbox_address: Id,
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
        InstanceSettingsRepository::new((*database).clone())
            .set_mail_enabled(setup.user_id, true, "request", now)
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
        let intake = MailIntake::new(Arc::clone(&mail), 52_428_800);
        let owner_membership: String = sqlx::query_scalar(
            "SELECT id FROM memberships WHERE workspace_id = $1 AND user_id = $2",
        )
        .bind(setup.workspace_id.to_string())
        .bind(setup.user_id.to_string())
        .fetch_one(database.pool())
        .await
        .unwrap();
        let owner = Member {
            id: setup.user_id,
            session_id: setup.session.id,
            cookie: format!("__Host-orbit_session={}", setup.session.token),
        };
        let domain = Id::new_v7();
        sqlx::query(
            "INSERT INTO mail_domains (id, workspace_id, name, status, verify_token, created_at, updated_at) \
             VALUES ($1, $2, 'one.test', 'verified', 'token', $3, $4)",
        )
        .bind(domain.to_string())
        .bind(setup.workspace_id.to_string())
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(database.pool())
        .await
        .unwrap();
        let mut fixture = Self {
            _root: root,
            database,
            identity,
            mail,
            intake,
            workspace: setup.workspace_id,
            ann: owner.clone(),
            bob: owner.clone(),
            owner,
            mailbox_address: Id::new_v7(),
        };
        let (ann, ann_membership) = fixture.member("Ann Lee").await;
        let (bob, _) = fixture.member("Bob").await;
        fixture.ann = ann;
        fixture.bob = bob;
        let mailbox = fixture
            .mail
            .create_mailbox(
                fixture.workspace,
                fixture.owner.id,
                NewMailbox {
                    kind: MailboxKind::Shared,
                    name: "Team".to_owned(),
                    domain_id: domain,
                    local_part: "team".to_owned(),
                    owner_membership_id: None,
                    member_ids: vec![owner_membership.parse().unwrap(), ann_membership],
                },
                "request",
            )
            .await
            .unwrap();
        fixture.mailbox_address = mailbox.addresses[0].id;
        fixture
    }

    /// A workspace member with a session, and the membership id.
    async fn member(&self, display_name: &str) -> (Member, Id) {
        let id = Id::new_v7();
        let membership_id = Id::new_v7();
        let now = TimestampMillis::now();
        let email = format!("{id}@example.com");
        sqlx::query(
            "INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, 'unused', $5, $6)",
        )
        .bind(id.to_string())
        .bind(&email)
        .bind(&email)
        .bind(display_name)
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(self.database.pool())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at) \
             VALUES ($1, $2, $3, 'member', 0, $4, $5)",
        )
        .bind(membership_id.to_string())
        .bind(self.workspace.to_string())
        .bind(id.to_string())
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(self.database.pool())
        .await
        .unwrap();
        let session = self
            .identity
            .create_session(
                &AuthenticatedUser {
                    id,
                    email,
                    display_name: display_name.to_owned(),
                },
                now,
            )
            .await
            .unwrap();
        (
            Member {
                id,
                session_id: session.id,
                cookie: format!("__Host-orbit_session={}", session.token),
            },
            membership_id,
        )
    }

    fn state(&self) -> MailState {
        MailState::new(
            Arc::clone(&self.identity),
            Arc::clone(&self.mail),
            CookieMode::secure(),
            MailSettings {
                enabled: true,
                hostname: Some("mx.orbit.test".to_owned()),
                mode: MailMode::Direct,
            },
        )
        .with_origins(ORIGIN, None)
    }

    fn app(&self) -> axum::Router {
        mail_router(self.state())
    }

    /// Receives and ingests `raw` for the team mailbox; returns the message id.
    async fn deliver_with(&self, raw: &[u8], dmarc: DmarcVerdict) -> Id {
        let ids = self
            .intake
            .accept(InboundMessage {
                source: Source::Smtp,
                trace_len: 0,
                raw: raw.to_vec(),
                mail_from: Some("carl@sender.test".to_owned()),
                recipients: vec![Recipient::Mailbox {
                    workspace_id: self.workspace,
                    address_id: self.mailbox_address,
                    rcpt: "team@one.test".to_owned(),
                }],
                peer_ip: IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)),
                helo: "mx.sender.test".to_owned(),
                tls: None,
                auth: auth(dmarc),
            })
            .await
            .unwrap();
        let outcome = self
            .mail
            .ingest(ids[0], TimestampMillis::now())
            .await
            .unwrap();
        let IngestOutcome::Ingested { message_id, .. } = outcome else {
            panic!("not ingested: {outcome:?}");
        };
        message_id
    }

    async fn deliver(&self, raw: &[u8]) -> Id {
        self.deliver_with(raw, DmarcVerdict::Pass).await
    }

    /// A workspace mail API call with the member's cookie.
    async fn call(
        &self,
        app: &axum::Router,
        member: &Member,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> Reply {
        let builder = Request::builder()
            .method(method)
            .uri(format!("/api/v1/workspaces/{}/mail{path}", self.workspace))
            .header(header::COOKIE, &member.cookie);
        let request = match body {
            Some(value) => builder
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(value.to_string())),
            None => builder.body(Body::empty()),
        }
        .unwrap();
        reply(app.clone().oneshot(request).await.unwrap()).await
    }

    /// The view URL of a message for the member.
    async fn view_url(
        &self,
        app: &axum::Router,
        member: &Member,
        message: Id,
        load: bool,
    ) -> String {
        let reply = self
            .call(
                app,
                member,
                "POST",
                &format!("/messages/{message}/view-url"),
                Some(json!({ "load_images": load })),
            )
            .await;
        assert_eq!(reply.status, StatusCode::OK, "{}", reply.text());
        let value = reply.json();
        assert!(value["expires_at"].is_string(), "{value}");
        value["url"].as_str().unwrap().to_owned()
    }
}

struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: Vec<u8>,
}

impl Reply {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }

    fn header(&self, name: &str) -> &str {
        self.headers
            .get(name)
            .unwrap_or_else(|| panic!("no {name} header"))
            .to_str()
            .unwrap()
    }
}

async fn reply(response: axum::response::Response) -> Reply {
    let status = response.status();
    let headers = response.headers().clone();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap()
        .to_vec();
    Reply {
        status,
        headers,
        body,
    }
}

/// A cookie-less GET of an absolute or root-relative URL (the sandboxed frame sends no cookies). An absolute URL
/// sends its host as `Host`, as a browser does.
async fn get(app: &axum::Router, url: &str) -> Reply {
    let (host, path) = if let Some(path) = url.strip_prefix(ORIGIN) {
        (Some("orbit.test"), path)
    } else if let Some(path) = url.strip_prefix("https://content.orbit.test") {
        (Some("content.orbit.test"), path)
    } else {
        (None, url)
    };
    let mut request = Request::builder().method("GET").uri(path);
    if let Some(host) = host {
        request = request.header("host", host);
    }
    reply(
        app.clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap(),
    )
    .await
}

fn auth(dmarc: DmarcVerdict) -> AuthOutcome {
    AuthOutcome {
        spf: Verdict::Pass,
        dkim: Verdict::Pass,
        dmarc,
        aligned_domain: Some("sender.test".to_owned()),
        temp_error: false,
        spf_domain: Some("sender.test".to_owned()),
        dkim_domain: Some("sender.test".to_owned()),
        from_domain: Some("sender.test".to_owned()),
    }
}

fn html_mail(id: &str, html: &str) -> Vec<u8> {
    format!(
        "From: Carl <carl@sender.test>\r\nTo: team@one.test\r\nSubject: Newsletter\r\n\
         Message-ID: <{id}@sender.test>\r\nMIME-Version: 1.0\r\n\
         Content-Type: multipart/alternative; boundary=\"alt\"\r\n\r\n\
         --alt\r\nContent-Type: text/plain\r\n\r\nPlain version.\r\n\
         --alt\r\nContent-Type: text/html; charset=utf-8\r\n\r\n{html}\r\n--alt--\r\n"
    )
    .into_bytes()
}

/// HTML with an inline PNG (`cid:logo`), a PNG attachment and an SVG attachment.
fn related_mail(id: &str) -> Vec<u8> {
    use base64::Engine as _;
    let png = base64::engine::general_purpose::STANDARD.encode(PNG);
    let svg = base64::engine::general_purpose::STANDARD
        .encode(b"<svg xmlns=\"http://www.w3.org/2000/svg\"><script>alert(1)</script></svg>");
    format!(
        "From: Carl <carl@sender.test>\r\nTo: team@one.test\r\nSubject: With parts\r\n\
         Message-ID: <{id}@sender.test>\r\nMIME-Version: 1.0\r\n\
         Content-Type: multipart/mixed; boundary=\"mix\"\r\n\r\n\
         --mix\r\nContent-Type: multipart/related; boundary=\"rel\"\r\n\r\n\
         --rel\r\nContent-Type: text/html\r\n\r\n<p>Logo <img src=\"cid:logo@x\"></p>\r\n\
         --rel\r\nContent-Type: image/png\r\nContent-ID: <logo@x>\r\nContent-Disposition: inline\r\n\
         Content-Transfer-Encoding: base64\r\n\r\n{png}\r\n--rel--\r\n\
         --mix\r\nContent-Type: image/png\r\nContent-Disposition: attachment; filename=\"photo.png\"\r\n\
         Content-Transfer-Encoding: base64\r\n\r\n{png}\r\n\
         --mix\r\nContent-Type: image/svg+xml\r\nContent-Disposition: attachment; filename=\"evil.svg\"\r\n\
         Content-Transfer-Encoding: base64\r\n\r\n{svg}\r\n--mix--\r\n"
    )
    .into_bytes()
}

fn nonce_of(document: &str) -> String {
    let start = document
        .find("<script nonce=\"")
        .expect("the height script")
        + 15;
    let end = start + document[start..].find('"').unwrap();
    document[start..end].to_owned()
}

/// The `src` of every `<img>` in the document.
fn image_sources(document: &str) -> Vec<String> {
    document
        .match_indices("<img")
        .filter_map(|(index, _)| {
            let rest = &document[index..];
            let start = rest.find("src=\"")? + 5;
            let end = start + rest[start..].find('"')?;
            Some(rest[start..end].replace("&amp;", "&"))
        })
        .collect()
}

fn attachment_ids(thread: &Value) -> HashMap<String, String> {
    thread["messages"][0]["attachments"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|attachment| {
            Some((
                attachment["filename"].as_str()?.to_owned(),
                attachment["id"].as_str().unwrap().to_owned(),
            ))
        })
        .collect()
}

async fn thread_of(fixture: &Fixture, app: &axum::Router, message: Id) -> Value {
    let thread_id: String =
        sqlx::query_scalar("SELECT thread_id FROM mail_thread_messages WHERE message_id = $1")
            .bind(message.to_string())
            .fetch_one(fixture.database.pool())
            .await
            .unwrap();
    let reply = fixture
        .call(
            app,
            &fixture.ann,
            "GET",
            &format!("/threads/{thread_id}"),
            None,
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text());
    reply.json()
}

#[tokio::test]
async fn the_view_document_has_the_exact_headers_and_no_mail_script() {
    let fixture = Fixture::new().await;
    let app = fixture.app();
    let message = fixture
        .deliver(&html_mail(
            "h1",
            "<p onclick=\"steal()\">Hi</p><script>alert('mail')</script>\
             <img src=\"x\" onerror=\"alert(2)\"><img src=\"https://tracker.example/p.gif\">",
        ))
        .await;

    let url = fixture.view_url(&app, &fixture.ann, message, false).await;
    assert!(
        url.starts_with("https://orbit.test/api/v1/mail/content/"),
        "{url}"
    );
    let document = get(&app, &url).await;
    assert_eq!(document.status, StatusCode::OK, "{}", document.text());
    let html = document.text();
    let nonce = nonce_of(&html);
    assert_eq!(
        document.header("content-security-policy"),
        format!(
            "sandbox allow-scripts allow-popups allow-popups-to-escape-sandbox; default-src 'none'; \
             script-src 'nonce-{nonce}'; img-src https://orbit.test data:; style-src 'unsafe-inline'; \
             font-src 'none'; form-action 'none'; base-uri 'none'; frame-ancestors https://orbit.test"
        )
    );
    assert_eq!(document.header("referrer-policy"), "no-referrer");
    assert_eq!(document.header("x-content-type-options"), "nosniff");
    assert_eq!(document.header("cache-control"), "private, no-store");
    assert!(document.header("content-type").starts_with("text/html"));

    // Only Orbit's height script, with the nonce of the header.
    assert_eq!(html.matches("<script").count(), 1, "{html}");
    assert!(html.contains(&format!("<script nonce=\"{nonce}\">")));
    assert!(!html.contains("alert"), "{html}");
    assert!(
        !html.contains("onclick") && !html.contains("onerror"),
        "{html}"
    );
    // Without Load images the tracker is gone.
    assert!(!html.contains("tracker.example"), "{html}");
    assert!(html.contains("Hi"));

    // A second view gets a fresh nonce.
    let again = get(
        &app,
        &fixture.view_url(&app, &fixture.ann, message, false).await,
    )
    .await;
    assert_ne!(nonce_of(&again.text()), nonce);
}

#[tokio::test]
async fn a_message_without_html_renders_its_text_escaped() {
    let fixture = Fixture::new().await;
    let app = fixture.app();
    let message = fixture
        .deliver(
            b"From: carl@sender.test\r\nTo: team@one.test\r\nSubject: Plain\r\n\
              Message-ID: <p1@sender.test>\r\n\r\nHello <b>there</b> & bye\r\n",
        )
        .await;
    let document = get(
        &app,
        &fixture.view_url(&app, &fixture.ann, message, false).await,
    )
    .await;
    assert_eq!(document.status, StatusCode::OK);
    let html = document.text();
    assert!(
        html.contains("<pre>Hello &lt;b&gt;there&lt;/b&gt; &amp; bye"),
        "{html}"
    );
    assert!(
        document
            .header("content-security-policy")
            .starts_with("sandbox allow-scripts")
    );
}

#[tokio::test]
async fn the_content_origin_serves_the_view_and_the_app_frames_it() {
    let fixture = Fixture::new().await;
    let app = mail_router(
        fixture
            .state()
            .with_origins(ORIGIN, Some("https://content.orbit.test".to_owned())),
    );
    let message = fixture.deliver(&related_mail("o1")).await;
    let url = fixture.view_url(&app, &fixture.ann, message, false).await;
    assert!(
        url.starts_with("https://content.orbit.test/api/v1/mail/content/"),
        "{url}"
    );
    let document = get(&app, &url).await;
    assert_eq!(document.status, StatusCode::OK);
    let csp = document.header("content-security-policy");
    assert!(
        csp.contains("img-src https://content.orbit.test data:"),
        "{csp}"
    );
    assert!(csp.ends_with("frame-ancestors https://orbit.test"), "{csp}");
    let sources = image_sources(&document.text());
    assert_eq!(sources.len(), 1, "{sources:?}");
    assert!(
        sources[0].starts_with("https://content.orbit.test/api/v1/mail/parts/"),
        "{sources:?}"
    );
    // with a content origin, the app origin does not serve mail content
    let on_app = |url: &str| url.replace("https://content.orbit.test", ORIGIN);
    assert_eq!(get(&app, &on_app(&url)).await.status, StatusCode::NOT_FOUND);
    assert_eq!(
        get(&app, &on_app(&sources[0])).await.status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn view_tokens_are_bound_to_a_live_session_and_a_reader() {
    let fixture = Fixture::new().await;
    let app = fixture.app();
    let message = fixture.deliver(&html_mail("b1", "<p>Bound</p>")).await;
    let key = ViewKey::new(&APP_KEY);
    let now = TimestampMillis::now();
    let grant = |session_id, expires_at| ViewGrant {
        session_id,
        workspace_id: fixture.workspace,
        message_id: message,
        load_images: false,
        expires_at: TimestampMillis::from_millis(expires_at),
    };
    let content = |token: String| format!("/api/v1/mail/content/{token}");

    // A token signed here works like the API's.
    let fresh = key.sign(
        ViewPurpose::View,
        &grant(fixture.ann.session_id, now.as_millis() + 60_000),
        "",
    );
    assert_eq!(
        get(&app, &content(fresh.clone())).await.status,
        StatusCode::OK
    );

    // Expired.
    let expired = key.sign(
        ViewPurpose::View,
        &grant(fixture.ann.session_id, now.as_millis() - 1),
        "",
    );
    let reply = get(&app, &content(expired)).await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
    assert_eq!(reply.json()["code"], "mail_not_found");

    // Another key, an altered token, another purpose.
    let other = ViewKey::new(&[1; 32]).sign(
        ViewPurpose::View,
        &grant(fixture.ann.session_id, now.as_millis() + 60_000),
        "",
    );
    assert_eq!(
        get(&app, &content(other)).await.status,
        StatusCode::NOT_FOUND
    );
    let mut altered = fresh.clone().into_bytes();
    let last = altered.len() - 1;
    altered[last] = if altered[last] == b'A' { b'B' } else { b'A' };
    assert_eq!(
        get(&app, &content(String::from_utf8(altered).unwrap()))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    let part = key.sign(
        ViewPurpose::Part,
        &grant(fixture.ann.session_id, now.as_millis() + 60_000),
        "",
    );
    assert_eq!(
        get(&app, &content(part)).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        get(&app, "/api/v1/mail/content/garbage").await.status,
        StatusCode::NOT_FOUND
    );

    // The session of a member who does not read the mailbox.
    let bobs = key.sign(
        ViewPurpose::View,
        &grant(fixture.bob.session_id, now.as_millis() + 60_000),
        "",
    );
    assert_eq!(
        get(&app, &content(bobs)).await.status,
        StatusCode::NOT_FOUND
    );

    // Signing out ends the token at once.
    let url = fixture.view_url(&app, &fixture.ann, message, false).await;
    assert_eq!(get(&app, &url).await.status, StatusCode::OK);
    assert!(
        fixture
            .identity
            .revoke_session(fixture.ann.session_id, fixture.ann.id, now)
            .await
            .unwrap()
    );
    assert_eq!(get(&app, &url).await.status, StatusCode::NOT_FOUND);
    assert_eq!(
        get(&app, &content(fresh)).await.status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn outsiders_get_not_found_from_every_content_route() {
    let fixture = Fixture::new().await;
    let app = fixture.app();
    let message = fixture.deliver(&related_mail("n1")).await;
    let thread = thread_of(&fixture, &app, message).await;
    let photo = attachment_ids(&thread)["photo.png"].clone();
    for (method, path, body) in [
        (
            "POST",
            format!("/messages/{message}/view-url"),
            Some(json!({ "load_images": false })),
        ),
        ("GET", format!("/attachments/{photo}"), None),
        ("GET", format!("/attachments/{photo}?inline=1"), None),
        ("GET", format!("/messages/{message}/raw"), None),
        ("GET", format!("/messages/{}/raw", Id::new_v7()), None),
        ("GET", "/attachments/not-an-id".to_owned(), None),
    ] {
        let reply = fixture.call(&app, &fixture.bob, method, &path, body).await;
        assert_eq!(reply.status, StatusCode::NOT_FOUND, "{method} {path}");
        assert_eq!(reply.json()["code"], "mail_not_found", "{method} {path}");
    }
    // A signed part URL of a reader does not work for an outsider's session.
    let document = get(
        &app,
        &fixture.view_url(&app, &fixture.ann, message, false).await,
    )
    .await;
    let part = image_sources(&document.text())[0].clone();
    assert_eq!(get(&app, &part).await.status, StatusCode::OK);
    let key = ViewKey::new(&APP_KEY);
    let bobs = key.sign(
        ViewPurpose::Part,
        &ViewGrant {
            session_id: fixture.bob.session_id,
            workspace_id: fixture.workspace,
            message_id: message,
            load_images: false,
            expires_at: TimestampMillis::from_millis(TimestampMillis::now().as_millis() + 60_000),
        },
        "",
    );
    let inline_id = part.rsplit('/').next().unwrap();
    assert_eq!(
        get(&app, &format!("/api/v1/mail/parts/{bobs}/{inline_id}"))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn attachments_download_safely_and_only_images_show_inline() {
    let fixture = Fixture::new().await;
    let app = fixture.app();
    let raw = related_mail("a1");
    let message = fixture.deliver(&raw).await;
    let thread = thread_of(&fixture, &app, message).await;
    let ids = attachment_ids(&thread);

    let photo = fixture
        .call(
            &app,
            &fixture.ann,
            "GET",
            &format!("/attachments/{}", ids["photo.png"]),
            None,
        )
        .await;
    assert_eq!(photo.status, StatusCode::OK);
    assert_eq!(photo.body, PNG);
    assert_eq!(photo.header("content-type"), "image/png");
    assert!(
        photo
            .header("content-disposition")
            .starts_with("attachment;")
    );
    assert!(photo.header("content-disposition").contains("photo.png"));
    assert_eq!(photo.header("x-content-type-options"), "nosniff");
    let inline = fixture
        .call(
            &app,
            &fixture.ann,
            "GET",
            &format!("/attachments/{}?inline=1", ids["photo.png"]),
            None,
        )
        .await;
    assert!(inline.header("content-disposition").starts_with("inline;"));

    // SVG is never an image here: a download, not sniffed as SVG, even when asked inline.
    let svg = fixture
        .call(
            &app,
            &fixture.ann,
            "GET",
            &format!("/attachments/{}?inline=1", ids["evil.svg"]),
            None,
        )
        .await;
    assert_eq!(svg.status, StatusCode::OK);
    assert!(
        svg.header("content-disposition").starts_with("attachment;"),
        "{}",
        svg.header("content-disposition")
    );
    assert_eq!(svg.header("content-type"), "application/octet-stream");
    assert_eq!(svg.header("x-content-type-options"), "nosniff");

    // The original message.
    let original = fixture
        .call(
            &app,
            &fixture.ann,
            "GET",
            &format!("/messages/{message}/raw"),
            None,
        )
        .await;
    assert_eq!(original.status, StatusCode::OK);
    assert_eq!(original.body, raw);
    assert_eq!(original.header("content-type"), "message/rfc822");
    let disposition = original.header("content-disposition");
    assert!(disposition.starts_with("attachment;"), "{disposition}");
    assert!(disposition.contains(".eml"), "{disposition}");
    assert_eq!(original.header("x-content-type-options"), "nosniff");
}

#[tokio::test]
async fn cid_images_load_through_signed_part_urls() {
    let fixture = Fixture::new().await;
    let app = fixture.app();
    let message = fixture.deliver(&related_mail("c1")).await;
    let document = get(
        &app,
        &fixture.view_url(&app, &fixture.ann, message, false).await,
    )
    .await;
    let sources = image_sources(&document.text());
    assert_eq!(sources.len(), 1, "{}", document.text());
    assert!(
        sources[0].starts_with("https://orbit.test/api/v1/mail/parts/"),
        "{sources:?}"
    );
    let part = get(&app, &sources[0]).await;
    assert_eq!(part.status, StatusCode::OK, "{}", part.text());
    assert_eq!(part.body, PNG);
    assert_eq!(part.header("content-type"), "image/png");
    assert_eq!(part.header("x-content-type-options"), "nosniff");

    // The SVG attachment of the same message is not served as a part.
    let thread = thread_of(&fixture, &app, message).await;
    let svg = attachment_ids(&thread)["evil.svg"].clone();
    let token = sources[0]
        .strip_prefix("https://orbit.test/api/v1/mail/parts/")
        .unwrap()
        .split('/')
        .next()
        .unwrap()
        .to_owned();
    assert_eq!(
        get(&app, &format!("/api/v1/mail/parts/{token}/{svg}"))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
}

/// A local image host the proxy reaches only through its test host table.
async fn image_host() -> SocketAddr {
    use axum::routing::get as route;
    let router = axum::Router::new()
        .route(
            "/a.png",
            route(|| async { ([(header::CONTENT_TYPE, "image/png")], PNG) }),
        )
        .route(
            "/page",
            route(|| async { ([(header::CONTENT_TYPE, "text/html")], "<html>hi</html>") }),
        )
        .route(
            "/lying.png",
            route(|| async { ([(header::CONTENT_TYPE, "image/png")], "<svg></svg>") }),
        )
        .route(
            "/big.png",
            route(|| async {
                let mut body = PNG.to_vec();
                body.resize(6 * 1024 * 1024, 0);
                ([(header::CONTENT_TYPE, "image/png")], body)
            }),
        )
        .route(
            "/to-metadata",
            route(|| async {
                (
                    StatusCode::FOUND,
                    [(header::LOCATION, "http://169.254.169.254/latest/meta-data/")],
                )
                    .into_response()
            }),
        )
        .route(
            "/to-localhost",
            route(|| async {
                (
                    StatusCode::FOUND,
                    [(header::LOCATION, "http://localhost:9/a.png")],
                )
                    .into_response()
            }),
        )
        .route(
            "/to-image",
            route(|| async {
                (
                    StatusCode::MOVED_PERMANENTLY,
                    [(header::LOCATION, "/a.png")],
                )
                    .into_response()
            }),
        )
        .route(
            "/hop/{n}",
            route(
                |axum::extract::Path(n): axum::extract::Path<u32>| async move {
                    (
                        StatusCode::FOUND,
                        [(header::LOCATION, format!("/hop/{}", n + 1))],
                    )
                        .into_response()
                },
            ),
        )
        .route(
            "/cookie",
            route(|headers: HeaderMap| async move {
                assert!(headers.get(header::COOKIE).is_none());
                assert!(headers.get(header::REFERER).is_none());
                ([(header::CONTENT_TYPE, "image/png")], PNG)
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    address
}

fn test_proxy(host: SocketAddr) -> ImageProxy {
    ImageProxy::with_test_hosts(HashMap::from([("images.test".to_owned(), host)]))
}

#[tokio::test]
async fn the_image_proxy_refuses_private_targets_before_connecting() {
    let proxy = ImageProxy::new();
    for url in [
        "http://127.0.0.1/a.png",
        "http://10.0.0.1/a.png",
        "http://192.168.1.1/a.png",
        "http://172.16.0.1/a.png",
        "http://100.64.0.1/a.png",
        "http://169.254.169.254/latest/meta-data/",
        "http://0.0.0.0/a.png",
        "http://224.0.0.1/a.png",
        "http://255.255.255.255/a.png",
        "http://192.0.2.10/a.png",
        "http://[::1]/a.png",
        "http://[::ffff:127.0.0.1]/a.png",
        "http://[::ffff:10.0.0.1]/a.png",
        "http://[fd00:ec2::254]/a.png",
        "http://[fe80::1]/a.png",
        "http://[fc00::1]/a.png",
        "http://[64:ff9b::a00:1]/a.png",
        // A name that resolves to loopback (resolved here; nothing is fetched).
        "http://localhost:9/a.png",
        "ftp://example.com/a.png",
        "file:///etc/passwd",
        "http://user:pass@example.com/a.png",
        "not a url",
    ] {
        let result = proxy.fetch(url).await;
        assert!(
            matches!(result, Err(ImageProxyError::Refused)),
            "{url}: {result:?}"
        );
    }
}

#[tokio::test]
async fn the_image_proxy_checks_every_redirect_and_the_body() {
    let host = image_host().await;
    let proxy = test_proxy(host);
    let port = host.port();
    let url = |path: &str| format!("http://images.test:{port}{path}");

    let image = proxy.fetch(&url("/a.png")).await.unwrap();
    assert_eq!(image.content_type, "image/png");
    assert_eq!(image.bytes, PNG);
    let image = proxy.fetch(&url("/to-image")).await.unwrap();
    assert_eq!(
        image.bytes, PNG,
        "a redirect to a public target is followed"
    );
    proxy.fetch(&url("/cookie")).await.unwrap();

    for (path, expected) in [
        ("/to-metadata", "refused"),
        ("/to-localhost", "refused"),
        ("/page", "not_image"),
        ("/lying.png", "not_image"),
        ("/big.png", "too_large"),
        ("/hop/1", "too_many_redirects"),
        ("/missing", "status"),
    ] {
        let result = proxy.fetch(&url(path)).await;
        let found = match &result {
            Err(ImageProxyError::Refused) => "refused",
            Err(ImageProxyError::NotImage) => "not_image",
            Err(ImageProxyError::TooLarge) => "too_large",
            Err(ImageProxyError::TooManyRedirects) => "too_many_redirects",
            Err(ImageProxyError::Status(_)) => "status",
            _ => "other",
        };
        assert_eq!(found, expected, "{path}: {result:?}");
    }
}

#[tokio::test]
async fn load_images_rewrites_remote_images_to_the_signed_proxy() {
    let fixture = Fixture::new().await;
    let host = image_host().await;
    let app = mail_router(fixture.state().with_image_proxy(Arc::new(test_proxy(host))));
    let remote = format!("http://images.test:{}/a.png", host.port());
    let message = fixture
        .deliver(&html_mail(
            "i1",
            &format!(
                "<p>Pic</p><img src=\"{remote}\"><img src=\"http://127.0.0.1:{}/a.png\">",
                host.port()
            ),
        ))
        .await;

    let blocked = get(
        &app,
        &fixture.view_url(&app, &fixture.ann, message, false).await,
    )
    .await;
    assert!(
        image_sources(&blocked.text()).is_empty(),
        "{}",
        blocked.text()
    );

    let loaded = get(
        &app,
        &fixture.view_url(&app, &fixture.ann, message, true).await,
    )
    .await;
    let sources = image_sources(&loaded.text());
    assert_eq!(sources.len(), 2, "{}", loaded.text());
    for source in &sources {
        assert!(
            source.starts_with("https://orbit.test/api/v1/mail/image?u="),
            "{source}"
        );
    }
    let image = get(&app, &sources[0]).await;
    assert_eq!(image.status, StatusCode::OK, "{}", image.text());
    assert_eq!(image.body, PNG);
    assert_eq!(image.header("content-type"), "image/png");
    assert_eq!(image.header("x-content-type-options"), "nosniff");
    // The loopback image is signed but refused by the proxy.
    let refused = get(&app, &sources[1]).await;
    assert_eq!(
        refused.status,
        StatusCode::BAD_GATEWAY,
        "{}",
        refused.text()
    );
    assert_eq!(refused.json()["code"], "mail_image_unavailable");

    // The signature covers the URL.
    let swapped = sources[0].replace("a.png", "page");
    assert_eq!(get(&app, &swapped).await.status, StatusCode::NOT_FOUND);
    assert_eq!(
        get(
            &app,
            "/api/v1/mail/image?u=http%3A%2F%2Fexample.com%2Fa.png&sig=x"
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    // And the session: signed out, the proxy answers nothing.
    fixture
        .identity
        .revoke_session(
            fixture.ann.session_id,
            fixture.ann.id,
            TimestampMillis::now(),
        )
        .await
        .unwrap();
    assert_eq!(get(&app, &sources[0]).await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn always_load_images_needs_dmarc_pass() {
    let fixture = Fixture::new().await;
    let app = fixture.app();
    let passing = fixture.deliver(&html_mail("s1", "<p>From Carl</p>")).await;
    let failing = fixture
        .deliver_with(
            &html_mail("s2", "<p>Forged Carl</p>"),
            DmarcVerdict::Fail {
                policy: DmarcPolicy::Reject,
            },
        )
        .await;
    let first = thread_of(&fixture, &app, passing).await;
    assert_eq!(first["messages"][0]["load_images_default"], false);

    let reply = fixture
        .call(
            &app,
            &fixture.ann,
            "PUT",
            "/sender-prefs",
            Some(json!({ "address": " Carl@Sender.TEST ", "load_images": true })),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text());
    assert_eq!(reply.json()["address"], "carl@sender.test");
    assert_eq!(reply.json()["load_images"], true);

    let first = thread_of(&fixture, &app, passing).await;
    assert_eq!(first["messages"][0]["load_images_default"], true);
    let forged = thread_of(&fixture, &app, failing).await;
    assert_eq!(
        forged["messages"][0]["load_images_default"], false,
        "DMARC failed"
    );

    // Only for the caller.
    let owner_view = {
        let thread_id: String =
            sqlx::query_scalar("SELECT thread_id FROM mail_thread_messages WHERE message_id = $1")
                .bind(passing.to_string())
                .fetch_one(fixture.database.pool())
                .await
                .unwrap();
        fixture
            .call(
                &app,
                &fixture.owner,
                "GET",
                &format!("/threads/{thread_id}"),
                None,
            )
            .await
            .json()
    };
    assert_eq!(owner_view["messages"][0]["load_images_default"], false);

    // Turned off again.
    let reply = fixture
        .call(
            &app,
            &fixture.ann,
            "PUT",
            "/sender-prefs",
            Some(json!({ "address": "carl@sender.test", "load_images": false })),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK);
    let first = thread_of(&fixture, &app, passing).await;
    assert_eq!(first["messages"][0]["load_images_default"], false);

    let invalid = fixture
        .call(
            &app,
            &fixture.ann,
            "PUT",
            "/sender-prefs",
            Some(json!({ "address": "nobody", "load_images": true })),
        )
        .await;
    assert_eq!(
        invalid.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        invalid.text()
    );
}
