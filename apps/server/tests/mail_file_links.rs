//! Large files sent as links (spec §5.9, U14): release, the public download, revoke.

#![allow(dead_code)]
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_mail::{AuthOutcome, DmarcVerdict, Verdict};
use orbit_platform::{
    AttachmentMutationCoordinator, AuthenticatedUser, Id, LocalBlobStore, MailMode,
    PasswordService, TestDatabase, TimestampMillis, UploadLimits, UploadService,
};
use orbit_server::auth_routes::CookieMode;
use orbit_server::mail_intake::{InboundMessage, MailIntake, Recipient, Source};
use orbit_server::mail_routes::{MailSettings, MailState, mail_router};
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::repositories::instance_settings::InstanceSettingsRepository;
use orbit_server::repositories::mail::{IngestOutcome, MailRepository, MailboxKind, NewMailbox};
use serde_json::{Value, json};
use tower::ServiceExt;

const APP_KEY: [u8; 32] = [32; 32];
const CONTENT: &str = "https://content.orbit.test";
const APP: &str = "https://orbit.test";

#[derive(Clone)]
struct Member {
    cookie: String,
    membership: Id,
}

struct Fixture {
    _root: tempfile::TempDir,
    database: TestDatabase,
    identity: Arc<IdentityRepository>,
    mail: Arc<MailRepository>,
    intake: MailIntake,
    workspace: Id,
    owner: Member,
    ann: Member,
    bob: Member,
    domain: Id,
    team: Id,
    team_primary: Id,
    team_alias: Id,
    sales: Id,
    sales_primary: Id,
    ann_personal: Id,
    ann_primary: Id,
}

impl Fixture {
    async fn new() -> Self {
        Self::with_limits(UploadLimits::default()).await
    }

    async fn with_limits(limits: UploadLimits) -> Self {
        Self::build(limits, false).await
    }

    async fn build(limits: UploadLimits, cloud: bool) -> Self {
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
            limits,
        );
        let mail = Arc::new(
            MailRepository::new((*database).clone(), uploads, APP_KEY)
                .with_cloud(cloud)
                .with_link_origin(CONTENT),
        );
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
            cookie: format!("__Host-orbit_session={}", setup.session.token),
            membership: owner_membership.parse().unwrap(),
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
            domain,
            team: Id::new_v7(),
            team_primary: Id::new_v7(),
            team_alias: Id::new_v7(),
            sales: Id::new_v7(),
            sales_primary: Id::new_v7(),
            ann_personal: Id::new_v7(),
            ann_primary: Id::new_v7(),
        };
        fixture.ready().await;
        fixture.ann = fixture.member("Ann Lee").await;
        fixture.bob = fixture.member("Bob").await;
        let team = fixture
            .mailbox(
                MailboxKind::Shared,
                "Team",
                "team",
                None,
                vec![fixture.owner.membership, fixture.ann.membership],
            )
            .await;
        fixture.team = team.id;
        fixture.team_primary = team.addresses[0].id;
        let team = fixture
            .mail
            .add_alias(
                fixture.workspace,
                user_of(&fixture, &fixture.owner).await,
                fixture.team,
                domain,
                "support",
                "request",
            )
            .await
            .unwrap();
        fixture.team_alias = team
            .addresses
            .iter()
            .find(|address| address.local_part == "support")
            .unwrap()
            .id;
        let sales = fixture
            .mailbox(
                MailboxKind::Shared,
                "Sales",
                "sales",
                None,
                vec![fixture.owner.membership],
            )
            .await;
        fixture.sales = sales.id;
        fixture.sales_primary = sales.addresses[0].id;
        let personal = fixture
            .mailbox(
                MailboxKind::Personal,
                "Ann Lee",
                "ann",
                Some(fixture.ann.membership),
                vec![],
            )
            .await;
        fixture.ann_personal = personal.id;
        fixture.ann_primary = personal.addresses[0].id;
        fixture
    }

    async fn mailbox(
        &self,
        kind: MailboxKind,
        name: &str,
        local_part: &str,
        owner_membership_id: Option<Id>,
        member_ids: Vec<Id>,
    ) -> orbit_server::repositories::mail::Mailbox {
        self.mail
            .create_mailbox(
                self.workspace,
                user_of(self, &self.owner).await,
                NewMailbox {
                    kind,
                    name: name.to_owned(),
                    domain_id: self.domain,
                    local_part: local_part.to_owned(),
                    owner_membership_id,
                    member_ids,
                },
                "request",
            )
            .await
            .unwrap()
    }

    async fn member(&self, display_name: &str) -> Member {
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
        Member {
            cookie: format!("__Host-orbit_session={}", session.token),
            membership: membership_id,
        }
    }

    fn app(&self) -> axum::Router {
        mail_router(
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
            .with_origins(APP, Some(CONTENT.to_owned())),
        )
    }

    async fn call(
        &self,
        member: &Member,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
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
        let response = self.app().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    /// Delivers a message to the mailbox of `address_id`, sent to `rcpt`; returns its message id
    /// and thread id.
    async fn deliver(&self, rcpt: &str, address_id: Id) -> (Id, Id) {
        let raw = format!(
            "From: Carl <carl@sender.test>\r\nTo: {rcpt}\r\nSubject: Hello\r\n\
             Message-ID: <m1@sender.test>\r\n\r\nHi\r\n"
        );
        self.deliver_raw(raw, rcpt, address_id).await
    }

    async fn deliver_raw(&self, raw: String, rcpt: &str, address_id: Id) -> (Id, Id) {
        let ids = self
            .intake
            .accept(InboundMessage {
                source: Source::Smtp,
                trace_len: 0,
                raw: raw.into_bytes(),
                mail_from: Some("carl@sender.test".to_owned()),
                recipients: vec![Recipient::Mailbox {
                    workspace_id: self.workspace,
                    address_id,
                    rcpt: rcpt.to_owned(),
                }],
                peer_ip: IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)),
                helo: "mx.sender.test".to_owned(),
                tls: None,
                auth: AuthOutcome {
                    spf: Verdict::Pass,
                    dkim: Verdict::Pass,
                    dmarc: DmarcVerdict::Pass,
                    aligned_domain: Some("sender.test".to_owned()),
                    temp_error: false,
                    spf_domain: Some("sender.test".to_owned()),
                    dkim_domain: Some("sender.test".to_owned()),
                    from_domain: Some("sender.test".to_owned()),
                },
            })
            .await
            .unwrap();
        let IngestOutcome::Ingested { message_id, .. } = self
            .mail
            .ingest(ids[0], TimestampMillis::now())
            .await
            .unwrap()
        else {
            panic!("not ingested");
        };
        let thread: String =
            sqlx::query_scalar("SELECT thread_id FROM mail_thread_messages WHERE message_id = $1")
                .bind(message_id.to_string())
                .fetch_one(self.database.pool())
                .await
                .unwrap();
        (message_id, thread.parse().unwrap())
    }
}

/// The user id behind a member (the mailbox API takes users).
async fn user_of(fixture: &Fixture, member: &Member) -> Id {
    let user: String = sqlx::query_scalar("SELECT user_id FROM memberships WHERE id = $1")
        .bind(member.membership.to_string())
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    user.parse().unwrap()
}

impl Fixture {
    /// Makes the domain able to send: MX live, an active DKIM key and a route.
    async fn ready(&self) {
        let now = TimestampMillis::now().as_millis();
        let route = Id::new_v7();
        sqlx::query(
            "INSERT INTO mail_send_routes (id, workspace_id, kind, name, secrets, settings, \
             created_at, updated_at) VALUES ($1, NULL, 'smtp', 'Relay', $2, '{}', $3, $3)",
        )
        .bind(route.to_string())
        .bind(b"sealed".to_vec())
        .bind(now)
        .execute(self.database.pool())
        .await
        .unwrap();
        sqlx::query(
            "UPDATE mail_domains SET send_route_id = $1, dns_report = $2 WHERE id = $3",
        )
        .bind(route.to_string())
        .bind(
            json!({"verify": {"status": "ok"}, "mx": {"status": "ok"}, "dkim": {"status": "ok"},
                   "spf": {"status": "ok"}, "dmarc": {"status": "ok"}})
            .to_string(),
        )
        .bind(self.domain.to_string())
        .execute(self.database.pool())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO mail_dkim_keys (id, domain_id, selector, public_key, private_sealed, \
             status, created_at, updated_at) VALUES ($1, $2, 'orbit1', 'key', $3, 'active', $4, $4)",
        )
        .bind(Id::new_v7().to_string())
        .bind(self.domain.to_string())
        .bind(b"sealed".to_vec())
        .bind(now)
        .execute(self.database.pool())
        .await
        .unwrap();
    }

    async fn sql(&self, sql: &str) {
        sqlx::query(sqlx::AssertSqlSafe(sql.to_owned()))
            .execute(self.database.pool())
            .await
            .unwrap();
    }

    /// A new draft of Ann's with the given recipients.
    async fn draft(&self, to: &[&str]) -> String {
        let (status, draft) = self
            .call(
                &self.ann,
                "POST",
                "/drafts",
                Some(
                    json!({"mode": "new", "from_address_id": self.ann_primary.to_string(),
                            "to": to, "subject": "Hello", "body_html": "<p>Hi</p>"}),
                ),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{draft}");
        draft["id"].as_str().unwrap().to_owned()
    }

    async fn send(&self, draft: &str, body: Value) -> (StatusCode, Value) {
        self.call(
            &self.ann,
            "POST",
            &format!("/drafts/{draft}/send"),
            Some(body),
        )
        .await
    }

    async fn outbound_row(&self, id: &str) -> (String, i64, i64) {
        let row =
            sqlx::query("SELECT status, release_at, created_at FROM mail_outbound WHERE id = $1")
                .bind(id)
                .fetch_one(self.database.pool())
                .await
                .unwrap();
        use sqlx::Row;
        (
            row.get("status"),
            row.get("release_at"),
            row.get("created_at"),
        )
    }

    async fn jobs(&self) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM jobs WHERE kind = 'mail.send'")
            .fetch_one(self.database.pool())
            .await
            .unwrap()
    }
}

fn at(millis: i64) -> TimestampMillis {
    TimestampMillis::from_millis(millis)
}
fn multipart(name: &str, bytes: &[u8]) -> (String, Vec<u8>) {
    let boundary = "orbit-test-boundary";
    let mut body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\n\
         Content-Type: application/octet-stream\r\n\r\n"
    )
    .into_bytes();
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

impl Fixture {
    async fn upload(
        &self,
        member: &Member,
        draft: &str,
        name: &str,
        bytes: &[u8],
    ) -> (StatusCode, Value) {
        let (content_type, body) = multipart(name, bytes);
        let request = Request::builder()
            .method("POST")
            .uri(format!(
                "/api/v1/workspaces/{}/mail/drafts/{draft}/files",
                self.workspace
            ))
            .header(header::COOKIE, &member.cookie)
            .header(header::CONTENT_TYPE, content_type)
            .body(Body::from(body))
            .unwrap();
        let response = self.app().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn reply(&self, member: &Member, mode: &str, thread: Id) -> (StatusCode, Value) {
        self.call(
            member,
            "POST",
            "/drafts",
            Some(json!({"mode": mode, "thread_id": thread.to_string()})),
        )
        .await
    }
}

use orbit_server::repositories::mail::Released;
impl Fixture {
    /// Gives the domain a real active DKIM key (the fixture's is a placeholder); returns its
    /// selector and its TXT record.
    async fn signing(&self) -> (String, String) {
        let key = self
            .mail
            .rotate_dkim(
                self.workspace,
                user_of(self, &self.owner).await,
                self.domain,
                "request",
                TimestampMillis::now(),
            )
            .await
            .unwrap();
        self.sql(&format!(
            "DELETE FROM mail_dkim_keys WHERE domain_id = '{}' AND selector <> '{}'",
            self.domain, key.selector
        ))
        .await;
        self.sql(&format!(
            "UPDATE mail_dkim_keys SET status = 'active' WHERE domain_id = '{}'",
            self.domain
        ))
        .await;
        (
            key.selector,
            format!("v=DKIM1; k=rsa; p={}", key.public_key),
        )
    }

    async fn new_draft(&self, from: Id, to: &[&str], cc: &[&str], bcc: &[&str]) -> String {
        let (status, draft) = self
            .call(
                &self.ann,
                "POST",
                "/drafts",
                Some(json!({"mode": "new", "from_address_id": from.to_string(), "to": to,
                            "cc": cc, "bcc": bcc, "subject": "Hello",
                            "body_html": "<p onclick=\"x()\">Hi <img src=\"cid:logo@one.test\"></p>"})),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{draft}");
        draft["id"].as_str().unwrap().to_owned()
    }

    async fn thread_draft(&self, mode: &str, thread: Id) -> String {
        let (status, draft) = self
            .call(
                &self.ann,
                "POST",
                "/drafts",
                Some(json!({"mode": mode, "thread_id": thread.to_string(),
                            "body_html": "<p>Thanks</p>"})),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{draft}");
        draft["id"].as_str().unwrap().to_owned()
    }

    /// Sends Ann's draft and releases it at its time.
    async fn send_and_release(&self, draft: &str) -> (Id, Released, i64) {
        let (status, sent) = self.send(draft, json!({})).await;
        assert_eq!(status, StatusCode::OK, "{sent}");
        let id: Id = sent["id"].as_str().unwrap().parse().unwrap();
        let (_, release_at, _) = self.outbound_row(&id.to_string()).await;
        let released = self
            .mail
            .release(id, Some("mx.orbit.test"), at(release_at))
            .await
            .unwrap()
            .expect("released");
        (id, released, release_at)
    }

    async fn raw_of(&self, message: Id) -> Vec<u8> {
        self.mail
            .message_raw(self.workspace, user_of(self, &self.ann).await, message)
            .await
            .unwrap()
            .0
    }

    async fn text(&self, sql: &str, id: impl ToString) -> Option<String> {
        sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_owned()))
            .bind(id.to_string())
            .fetch_one(self.database.pool())
            .await
            .unwrap()
    }

    async fn number(&self, sql: &str, id: impl ToString) -> Option<i64> {
        sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_owned()))
            .bind(id.to_string())
            .fetch_one(self.database.pool())
            .await
            .unwrap()
    }

    async fn thread_ids(&self, member: &Member, query: &str) -> Vec<String> {
        let (status, page) = self
            .call(member, "GET", &format!("/threads?{query}"), None)
            .await;
        assert_eq!(status, StatusCode::OK, "{page}");
        page["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["id"].as_str().unwrap().to_owned())
            .collect()
    }

    async fn unread(&self, member: &Member, thread: Id) -> bool {
        let (status, detail) = self
            .call(member, "GET", &format!("/threads/{thread}"), None)
            .await;
        assert_eq!(status, StatusCode::OK, "{detail}");
        detail["thread"]["unread"].as_bool().unwrap()
    }

    /// Ann sends at once (no undo time), so the release happens now and mail delivered after it
    /// is newer.
    async fn no_undo(&self) {
        self.sql(&format!(
            "INSERT INTO mail_member_settings (membership_id, undo_send_seconds, updated_at) \
             VALUES ('{}', 0, 0)",
            self.ann.membership
        ))
        .await;
    }
}

// ---- Large files as links (Task 17) ----

struct Linked {
    thread: Id,
    message: Id,
    outbound: Id,
    token: String,
    link: String,
    file: Vec<u8>,
}

struct Reply {
    status: StatusCode,
    headers: axum::http::HeaderMap,
    body: Vec<u8>,
}

impl Fixture {
    /// Sends a message with `name` as a link file and releases it.
    async fn linked(&self, name: &str, file: &[u8]) -> Linked {
        self.linked_with(name, file, "<p>Hi</p>").await
    }

    async fn linked_with(&self, name: &str, file: &[u8], body_html: &str) -> Linked {
        self.signing().await;
        self.no_undo().await;
        let draft = self.draft(&["x@out.test"]).await;
        let (status, patched) = self
            .call(
                &self.ann,
                "PATCH",
                &format!("/drafts/{draft}"),
                Some(json!({"version": 1, "body_html": body_html})),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{patched}");
        let (status, uploaded) = self.upload(&self.ann, &draft, name, file).await;
        assert_eq!(status, StatusCode::CREATED, "{uploaded}");
        let (status, set) = self
            .call(
                &self.ann,
                "PATCH",
                &format!("/drafts/{draft}/files/{}", uploaded["id"].as_str().unwrap()),
                Some(json!({"as_link": true})),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{set}");
        let (outbound, released, _) = self.send_and_release(&draft).await;
        let html_part: Option<i64> =
            sqlx::query_scalar("SELECT html_part FROM mail_messages WHERE id = $1")
                .bind(released.message_id.to_string())
                .fetch_one(self.database.pool())
                .await
                .unwrap();
        let raw = self.raw_of(released.message_id).await;
        let html = String::from_utf8(
            orbit_mail::part_bytes(&raw, u32::try_from(html_part.unwrap()).unwrap()).unwrap(),
        )
        .unwrap();
        let start = html.find(&format!("{CONTENT}/mail/f/")).expect(&html);
        let link: String = html[start..]
            .chars()
            .take_while(|c| !c.is_whitespace() && *c != '"' && *c != '<')
            .collect();
        Linked {
            thread: released.thread_id,
            message: released.message_id,
            outbound,
            token: link.rsplit('/').next().unwrap().to_owned(),
            link,
            file: file.to_vec(),
        }
    }

    /// A GET on `host` and `path`, from `ip` (no address when `None`).
    async fn download(&self, host: &str, path: &str, ip: Option<&str>) -> Reply {
        let mut request = Request::builder()
            .method("GET")
            .uri(path)
            .header(header::HOST, host)
            .body(Body::empty())
            .unwrap();
        if let Some(ip) = ip {
            request
                .extensions_mut()
                .insert(orbit_platform::ClientIp(ip.parse().unwrap()));
        }
        let response = self.app().oneshot(request).await.unwrap();
        let (parts, body) = response.into_parts();
        Reply {
            status: parts.status,
            headers: parts.headers,
            body: to_bytes(body, usize::MAX).await.unwrap().to_vec(),
        }
    }

    async fn get_link(&self, linked: &Linked) -> Reply {
        self.download(
            "content.orbit.test",
            &format!("/mail/f/{}", linked.token),
            Some("192.0.2.9"),
        )
        .await
    }

    async fn link_id(&self, outbound: Id) -> String {
        sqlx::query_scalar("SELECT id FROM mail_file_links WHERE outbound_id = $1")
            .bind(outbound.to_string())
            .fetch_one(self.database.pool())
            .await
            .unwrap()
    }
}

#[tokio::test]
async fn link_downloads_with_attachment_disposition() {
    let f = Fixture::new().await;
    let linked = f.linked("big report.bin", &[7u8; 5000]).await;
    let reply = f.get_link(&linked).await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.body, linked.file);
    let h = |name: &str| reply.headers[name].to_str().unwrap().to_owned();
    let disposition = h("content-disposition");
    assert!(disposition.starts_with("attachment;"), "{disposition}");
    assert!(
        disposition.contains("filename*=UTF-8''big%20report.bin"),
        "{disposition}"
    );
    assert_eq!(h("content-type"), "application/octet-stream");
    assert_eq!(h("x-content-type-options"), "nosniff");
    assert_eq!(h("referrer-policy"), "no-referrer");
    assert_eq!(h("cache-control"), "private, no-store");
    // Counted.
    let id = f.link_id(linked.outbound).await;
    let downloads: i64 = sqlx::query_scalar("SELECT downloads FROM mail_file_links WHERE id = $1")
        .bind(&id)
        .fetch_one(f.database.pool())
        .await
        .unwrap();
    assert_eq!(downloads, 1);
}

#[tokio::test]
async fn a_hostile_file_name_stays_out_of_the_header() {
    let f = Fixture::new().await;
    let linked = f.linked("a.bin", b"data").await;
    f.sql("UPDATE mail_file_links SET filename = 'q\"; x=y\r\nX-Evil: 1\u{e9}.bin'")
        .await;
    let reply = f.get_link(&linked).await;
    assert_eq!(reply.status, StatusCode::OK);
    assert!(reply.headers.get("x-evil").is_none());
    let disposition = reply.headers["content-disposition"].to_str().unwrap();
    assert!(!disposition.contains('\r') && !disposition.contains('\n'));
    assert!(!disposition.contains("\"; x=y"), "{disposition}");
    assert!(
        disposition.contains("filename*=UTF-8''q%22%3B%20x%3Dy%0D%0AX-Evil%3A%201%C3%A9.bin"),
        "{disposition}"
    );
}

#[tokio::test]
async fn expired_link_404() {
    let f = Fixture::new().await;
    let linked = f.linked("a.bin", b"data").await;
    f.sql("UPDATE mail_file_links SET expires_at = 1").await;
    let reply = f.get_link(&linked).await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
    let unknown = f
        .download("content.orbit.test", "/mail/f/nope", Some("192.0.2.9"))
        .await;
    assert_eq!(unknown.status, StatusCode::NOT_FOUND);
    assert_eq!(
        unknown.body, reply.body,
        "Expired and unknown look the same"
    );
}

#[tokio::test]
async fn revoked_link_404() {
    let f = Fixture::new().await;
    let linked = f.linked("a.bin", b"data").await;
    assert_eq!(f.get_link(&linked).await.status, StatusCode::OK);
    let id = f.link_id(linked.outbound).await;
    let (status, view) = f
        .call(
            &f.ann,
            "POST",
            &format!(
                "/threads/{}/messages/{}/file-links/{id}/revoke",
                linked.thread, linked.message
            ),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["revoked"], true);
    assert_eq!(view["downloads"], 1);
    assert_eq!(f.get_link(&linked).await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn token_stored_hashed() {
    use sha2::{Digest, Sha256};
    let f = Fixture::new().await;
    let linked = f.linked("a.bin", b"data").await;
    assert_eq!(linked.token.len(), 43, "32 bytes, URL-safe base64");
    let stored: String = sqlx::query_scalar("SELECT token_sha256 FROM mail_file_links")
        .fetch_one(f.database.pool())
        .await
        .unwrap();
    assert_eq!(
        stored,
        format!("{:x}", Sha256::digest(linked.token.as_bytes()))
    );
    assert!(!stored.contains(&linked.token));
    // Nothing else in the database holds the token.
    let holders: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM mail_file_links WHERE filename = $1")
            .bind(&linked.token)
            .fetch_one(f.database.pool())
            .await
            .unwrap();
    assert_eq!(holders, 0);
}

#[tokio::test]
async fn app_origin_404_with_content_origin() {
    let f = Fixture::new().await;
    let linked = f.linked("a.bin", b"data").await;
    let reply = f
        .download(
            "orbit.test",
            &format!("/mail/f/{}", linked.token),
            Some("192.0.2.9"),
        )
        .await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
    assert_eq!(f.get_link(&linked).await.status, StatusCode::OK);
}

#[tokio::test]
async fn rate_limited() {
    let f = Fixture::new().await;
    let linked = f.linked("a.bin", b"data").await;
    let path = format!("/mail/f/{}", linked.token);
    for _ in 0..60 {
        let reply = f
            .download("content.orbit.test", &path, Some("198.51.100.1"))
            .await;
        assert_eq!(reply.status, StatusCode::OK);
    }
    let reply = f
        .download("content.orbit.test", &path, Some("198.51.100.1"))
        .await;
    assert_eq!(reply.status, StatusCode::TOO_MANY_REQUESTS);
    // Another address is not affected.
    let reply = f
        .download("content.orbit.test", &path, Some("198.51.100.2"))
        .await;
    assert_eq!(reply.status, StatusCode::OK);
    // A day's 1,000 downloads of one link, from many addresses.
    let day = TimestampMillis::now().as_millis().div_euclid(86_400_000);
    let id = f.link_id(linked.outbound).await;
    f.sql(&format!(
        "INSERT INTO rate_limit_windows (bucket, window_minute, hits) \
         VALUES ('mailfile:link:{id}:{day}', {}, 1000) \
         ON CONFLICT (bucket, window_minute) DO UPDATE SET hits = 1000",
        (day + 1) * 1440
    ))
    .await;
    let reply = f
        .download("content.orbit.test", &path, Some("198.51.100.3"))
        .await;
    assert_eq!(reply.status, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn link_block_in_sent_message() {
    let f = Fixture::new().await;
    let linked = f.linked("big report.bin", &[1u8; 2500]).await;
    let (text, html_part): (String, Option<i64>) = {
        use sqlx::Row;
        let row = sqlx::query("SELECT body_text, html_part FROM mail_messages WHERE id = $1")
            .bind(linked.message.to_string())
            .fetch_one(f.database.pool())
            .await
            .unwrap();
        (row.get("body_text"), row.get("html_part"))
    };
    assert!(text.contains("big report.bin"), "{text}");
    assert!(text.contains("2 KB") || text.contains("3 KB"), "{text}");
    // The stored text never holds the token (only the sealed raw message does).
    assert!(text.contains("[link]"), "{text}");
    assert!(!text.contains(&linked.token), "{text}");
    let snippet: String = sqlx::query_scalar("SELECT snippet FROM mail_messages WHERE id = $1")
        .bind(linked.message.to_string())
        .fetch_one(f.database.pool())
        .await
        .unwrap();
    assert!(!snippet.contains(&linked.token), "{snippet}");
    let search = |q: &str| {
        let q: String = url::form_urlencoded::byte_serialize(q.as_bytes()).collect();
        format!("/search?q={q}")
    };
    let (status, found) = f.call(&f.ann, "GET", &search("report"), None).await;
    assert_eq!(status, StatusCode::OK, "{found}");
    assert_eq!(found["items"].as_array().unwrap().len(), 1, "{found}");
    for query in [linked.token.as_str(), linked.link.as_str()] {
        let (_, found) = f.call(&f.ann, "GET", &search(query), None).await;
        assert!(found["items"].as_array().unwrap().is_empty(), "{found}");
    }
    assert!(text.contains("link works until 20"), "{text}");
    let raw = f.raw_of(linked.message).await;
    let html = String::from_utf8(
        orbit_mail::part_bytes(&raw, u32::try_from(html_part.unwrap()).unwrap()).unwrap(),
    )
    .unwrap();
    assert!(
        html.contains(&format!("<a href=\"{}\">", linked.link)),
        "{html}"
    );
    assert!(html.contains("big report.bin"), "{html}");
    // The file is not attached.
    assert!(!String::from_utf8_lossy(&raw).contains("filename=\"big report.bin\""));
    assert!(!String::from_utf8_lossy(&raw).contains("AQEBAQEB"));
    // The link lasts 30 days (the default), from the release.
    let (expires, created): (i64, i64) =
        sqlx::query_as("SELECT expires_at, created_at FROM mail_file_links")
            .fetch_one(f.database.pool())
            .await
            .unwrap();
    assert_eq!(expires - created, 30 * 86_400_000);
    // The sender sees the link on the message; nobody else does.
    let (status, detail) = f
        .call(&f.ann, "GET", &format!("/threads/{}", linked.thread), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    let links = &detail["messages"][0]["file_links"];
    assert_eq!(links.as_array().unwrap().len(), 1, "{detail}");
    assert_eq!(links[0]["filename"], "big report.bin");
    assert_eq!(links[0]["size"], 2500);
    assert_eq!(links[0]["revoked"], false);
    assert_eq!(links[0]["downloads"], 0);
    assert!(links[0].get("url").is_none());
}

#[tokio::test]
async fn link_days_follow_the_workspace_policy() {
    let f = Fixture::new().await;
    f.sql(&format!(
        "INSERT INTO mail_send_policies (workspace_id, tier, link_days, created_at, updated_at) \
         VALUES ('{}', 'standard', 7, 1, 1)",
        f.workspace
    ))
    .await;
    let _ = f.linked("a.bin", b"data").await;
    let (expires, created): (i64, i64) =
        sqlx::query_as("SELECT expires_at, created_at FROM mail_file_links")
            .fetch_one(f.database.pool())
            .await
            .unwrap();
    assert_eq!(expires - created, 7 * 86_400_000);
}

#[tokio::test]
async fn only_sender_revokes() {
    let f = Fixture::new().await;
    let linked = f.linked("a.bin", b"data").await;
    let id = f.link_id(linked.outbound).await;
    let path = format!(
        "/threads/{}/messages/{}/file-links/{id}/revoke",
        linked.thread, linked.message
    );
    let (status, _) = f.call(&f.bob, "POST", &path, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = f.call(&f.owner, "POST", &path, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // Still live; a wrong message id is not found for the sender either.
    assert_eq!(f.get_link(&linked).await.status, StatusCode::OK);
    let wrong = format!(
        "/threads/{}/messages/{}/file-links/{id}/revoke",
        linked.thread,
        Id::new_v7()
    );
    let (status, _) = f.call(&f.ann, "POST", &wrong, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = f.call(&f.ann, "POST", &path, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(f.get_link(&linked).await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn without_an_origin_the_message_is_held() {
    let f = Fixture::new().await;
    f.signing().await;
    f.no_undo().await;
    let bare = MailRepository::new((*f.database).clone(), f.mail.uploads().clone(), APP_KEY);
    let draft = f.draft(&["x@out.test"]).await;
    let (_, uploaded) = f.upload(&f.ann, &draft, "a.bin", b"data").await;
    f.call(
        &f.ann,
        "PATCH",
        &format!("/drafts/{draft}/files/{}", uploaded["id"].as_str().unwrap()),
        Some(json!({"as_link": true})),
    )
    .await;
    let (_, sent) = f.send(&draft, json!({})).await;
    let id: Id = sent["id"].as_str().unwrap().parse().unwrap();
    let (_, release_at, _) = f.outbound_row(&id.to_string()).await;
    let error = bare.release(id, None, at(release_at)).await.unwrap_err();
    // The row stays held with its reason (the job records it); nothing is sent unsigned.
    f.mail
        .hold_release(id, "link_origin_missing", at(release_at))
        .await
        .unwrap();
    let held: (String, Option<String>) =
        sqlx::query_as("SELECT status, hold_reason FROM mail_outbound WHERE id = $1")
            .bind(id.to_string())
            .fetch_one(f.database.pool())
            .await
            .unwrap();
    assert_eq!(
        held,
        ("held".to_owned(), Some("link_origin_missing".to_owned()))
    );
    let links: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM mail_file_links")
        .fetch_one(f.database.pool())
        .await
        .unwrap();
    assert_eq!(links, 0);
    assert!(
        matches!(
            error,
            orbit_server::repositories::mail::MailError::Unprocessable("link_origin_missing")
        ),
        "{error:?}"
    );
}

#[tokio::test]
async fn ipv6_clients_share_a_bucket_per_64() {
    let f = Fixture::new().await;
    let linked = f.linked("a.bin", b"data").await;
    let path = format!("/mail/f/{}", linked.token);
    for _ in 0..60 {
        let reply = f
            .download("content.orbit.test", &path, Some("2001:db8:0:5::1"))
            .await;
        assert_eq!(reply.status, StatusCode::OK);
    }
    // Another address of the same /64 shares the budget; another /64 does not.
    let same = f
        .download("content.orbit.test", &path, Some("2001:db8:0:5:ffff::9"))
        .await;
    assert_eq!(same.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(same.headers["retry-after"], "60");
    let other = f
        .download("content.orbit.test", &path, Some("2001:db8:0:6::1"))
        .await;
    assert_eq!(other.status, StatusCode::OK);
}

#[tokio::test]
async fn a_deleted_blob_is_not_found_and_not_counted() {
    let f = Fixture::new().await;
    let linked = f.linked("a.bin", b"data").await;
    f.sql("UPDATE attachment_blobs SET deleted_at = 1 WHERE id IN (SELECT blob_id FROM mail_file_links)")
        .await;
    assert_eq!(f.get_link(&linked).await.status, StatusCode::NOT_FOUND);
    let downloads: i64 = sqlx::query_scalar("SELECT downloads FROM mail_file_links")
        .fetch_one(f.database.pool())
        .await
        .unwrap();
    assert_eq!(downloads, 0);
}

#[tokio::test]
async fn inline_and_forwarded_files_cannot_be_links() {
    let f = Fixture::new().await;
    let draft = f.draft(&["x@out.test"]).await;
    let (_, uploaded) = f.upload(&f.ann, &draft, "logo.png", b"png").await;
    let file = uploaded["id"].as_str().unwrap();
    f.sql("UPDATE mail_draft_files SET content_id = 'logo@one.test'")
        .await;
    let (status, body) = f
        .call(
            &f.ann,
            "PATCH",
            &format!("/drafts/{draft}/files/{file}"),
            Some(json!({"as_link": true})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["code"], "link_inline");

    let raw = "From: Carl <carl@sender.test>\r\nTo: team@one.test\r\nSubject: Report\r\n\
         Message-ID: <m3@sender.test>\r\nMIME-Version: 1.0\r\n\
         Content-Type: multipart/mixed; boundary=\"b\"\r\n\r\n\
         --b\r\nContent-Type: text/plain\r\n\r\nSee attached\r\n\
         --b\r\nContent-Type: application/pdf; name=\"report.pdf\"\r\n\
         Content-Disposition: attachment; filename=\"report.pdf\"\r\n\
         Content-Transfer-Encoding: base64\r\n\r\nJVBERi0xLjQK\r\n--b--\r\n";
    let (_, thread) = f
        .deliver_raw(raw.to_owned(), "team@one.test", f.team_primary)
        .await;
    let (status, forward) = f
        .call(
            &f.ann,
            "POST",
            "/drafts",
            Some(json!({"mode": "forward", "thread_id": thread.to_string()})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{forward}");
    let forward_id = forward["id"].as_str().unwrap();
    let forwarded = forward["files"][0]["id"].as_str().unwrap();
    let (status, body) = f
        .call(
            &f.ann,
            "PATCH",
            &format!("/drafts/{forward_id}/files/{forwarded}"),
            Some(json!({"as_link": true})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["code"], "link_forwarded");
}

#[tokio::test]
async fn a_url_cut_by_the_snippet_limit_leaves_no_token_prefix() {
    // Where the URL starts in the text of a short message.
    let first = Fixture::new().await;
    let probe = first.linked("a.bin", b"data").await;
    let text: String = sqlx::query_scalar("SELECT body_text FROM mail_messages WHERE id = $1")
        .bind(probe.message.to_string())
        .fetch_one(first.database.pool())
        .await
        .unwrap();
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let at = collapsed.find("[link]").unwrap();
    // Pad the body so that the URL starts 50 characters before the snippet's 200th.
    let pad = "a".repeat(150 - at - 1);
    let f = Fixture::new().await;
    let linked = f
        .linked_with("a.bin", b"data", &format!("<p>Hi {pad}</p>"))
        .await;
    let snippet: String = sqlx::query_scalar("SELECT snippet FROM mail_messages WHERE id = $1")
        .bind(linked.message.to_string())
        .fetch_one(f.database.pool())
        .await
        .unwrap();
    assert!(
        snippet.contains("[link]"),
        "The cut falls after the mask: {snippet}"
    );
    assert!(!snippet.contains(&linked.token[..10]), "{snippet}");
    assert!(!snippet.contains("/mail/f/"), "{snippet}");
}

#[tokio::test]
async fn release_refuses_inline_and_forwarded_link_files() {
    use orbit_server::repositories::mail::MailError;
    let f = Fixture::new().await;
    f.signing().await;
    f.no_undo().await;
    let draft = f.draft(&["x@out.test"]).await;
    let (_, uploaded) = f.upload(&f.ann, &draft, "a.png", b"png").await;
    assert_eq!(uploaded["as_link"], false);
    f.sql("UPDATE mail_draft_files SET as_link = 1, content_id = 'logo@one.test'")
        .await;
    let (_, sent) = f.send(&draft, json!({})).await;
    let id: Id = sent["id"].as_str().unwrap().parse().unwrap();
    let (_, release_at, _) = f.outbound_row(&id.to_string()).await;
    let error = f.mail.release(id, None, at(release_at)).await.unwrap_err();
    assert!(
        matches!(error, MailError::Unprocessable("link_inline")),
        "{error:?}"
    );

    // A forwarded file, flagged in the database directly.
    let raw = "From: Carl <carl@sender.test>\r\nTo: team@one.test\r\nSubject: Report\r\n\
         Message-ID: <m3@sender.test>\r\nMIME-Version: 1.0\r\n\
         Content-Type: multipart/mixed; boundary=\"b\"\r\n\r\n\
         --b\r\nContent-Type: text/plain\r\n\r\nSee attached\r\n\
         --b\r\nContent-Type: application/pdf; name=\"report.pdf\"\r\n\
         Content-Disposition: attachment; filename=\"report.pdf\"\r\n\
         Content-Transfer-Encoding: base64\r\n\r\nJVBERi0xLjQK\r\n--b--\r\n";
    let (_, thread) = f
        .deliver_raw(raw.to_owned(), "team@one.test", f.team_primary)
        .await;
    let forward = f.thread_draft("forward", thread).await;
    let (status, body) = f
        .call(
            &f.ann,
            "PATCH",
            &format!("/drafts/{forward}"),
            Some(json!({"version": 1, "to": ["x@out.test"]})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    f.sql(&format!(
        "UPDATE mail_draft_files SET as_link = 1 WHERE draft_id = '{forward}'"
    ))
    .await;
    let (status, sent) = f.send(&forward, json!({})).await;
    assert_eq!(status, StatusCode::OK, "{sent}");
    let id: Id = sent["id"].as_str().unwrap().parse().unwrap();
    let (_, release_at, _) = f.outbound_row(&id.to_string()).await;
    let error = f.mail.release(id, None, at(release_at)).await.unwrap_err();
    assert!(
        matches!(error, MailError::Unprocessable("link_forwarded")),
        "{error:?}"
    );
}
