//! Drafts (spec §7, D13): reply recipients, one draft per thread and member, versions, forwarded
//! files and uploads.

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

const APP_KEY: [u8; 32] = [31; 32];

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
        mail_router(MailState::new(
            Arc::clone(&self.identity),
            Arc::clone(&self.mail),
            CookieMode::secure(),
            MailSettings {
                enabled: true,
                hostname: Some("mx.orbit.test".to_owned()),
                mode: MailMode::Direct,
            },
        ))
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

fn reply_all_raw() -> String {
    "From: Carl <carl@sender.test>\r\nReply-To: Carl Work <carl.work@sender.test>\r\n\
     To: Team <team@one.test>, Dan <dan@sender.test>, SUPPORT+News@one.test\r\n\
     Cc: Eve <eve@sender.test>, DAN@sender.test, Support@ONE.test\r\n\
     Subject: Re: Plans\r\nMessage-ID: <m2@sender.test>\r\n\r\nHi\r\n"
        .to_owned()
}

fn with_attachment() -> String {
    "From: Carl <carl@sender.test>\r\nTo: team@one.test\r\nSubject: Report\r\n\
     Message-ID: <m3@sender.test>\r\nMIME-Version: 1.0\r\n\
     Content-Type: multipart/mixed; boundary=\"b\"\r\n\r\n\
     --b\r\nContent-Type: text/plain\r\n\r\nSee attached\r\n\
     --b\r\nContent-Type: application/pdf; name=\"report.pdf\"\r\n\
     Content-Disposition: attachment; filename=\"report.pdf\"\r\n\
     Content-Transfer-Encoding: base64\r\n\r\nJVBERi0xLjQK\r\n--b--\r\n"
        .to_owned()
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

fn strings(value: &Value) -> Vec<&str> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect()
}

#[tokio::test]
async fn reply_all_recipients_exclude_self() {
    let f = Fixture::new().await;
    let (_, thread) = f
        .deliver_raw(reply_all_raw(), "team@one.test", f.team_primary)
        .await;
    let (status, reply) = f.reply(&f.ann, "reply", thread).await;
    assert_eq!(status, StatusCode::CREATED, "{reply}");
    assert_eq!(strings(&reply["to"]), ["carl.work@sender.test"]);
    assert_eq!(reply["cc"], json!([]));
    assert_eq!(reply["subject"], "Re: Plans");
    assert_eq!(reply["mode"], "reply");
    assert_eq!(reply["version"], 1);
    // Reply all: Reply-To, then To; Cc; no address of the mailbox (its alias with a tag too),
    // each address once whatever its case.
    let (status, all) = f.reply(&f.ann, "reply_all", thread).await;
    assert_eq!(status, StatusCode::CREATED, "{all}");
    assert_eq!(all["id"], reply["id"]);
    assert_eq!(all["mode"], "reply_all");
    assert_eq!(
        strings(&all["to"]),
        ["carl.work@sender.test", "dan@sender.test"]
    );
    assert_eq!(
        strings(&all["cc"]),
        ["eve@sender.test"],
        "The mailbox addresses and the repeat of Dan are removed: {all}"
    );
    assert_eq!(all["subject"], "Re: Plans");
    assert_eq!(all["version"], 2);
}

#[tokio::test]
async fn one_reply_draft_per_thread_per_member() {
    let f = Fixture::new().await;
    let (_, thread) = f.deliver("team@one.test", f.team_primary).await;
    let (_, first) = f.reply(&f.ann, "reply", thread).await;
    let (status, again) = f.reply(&f.ann, "reply", thread).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(again["id"], first["id"]);
    // An edited draft keeps its recipients when the mode changes.
    let id = first["id"].as_str().unwrap();
    let (status, edited) = f
        .call(
            &f.ann,
            "PATCH",
            &format!("/drafts/{id}"),
            Some(json!({"version": 1, "to": ["x@y.test"], "body_html": "<p>Hi</p>"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{edited}");
    let (_, forward) = f.reply(&f.ann, "forward", thread).await;
    assert_eq!(forward["id"], first["id"]);
    assert_eq!(forward["mode"], "forward");
    assert_eq!(strings(&forward["to"]), ["x@y.test"]);
    assert_eq!(forward["subject"], "Re: Hello");
    // Another member of the mailbox has a draft of their own.
    let (_, owner) = f.reply(&f.owner, "reply", thread).await;
    assert_ne!(owner["id"], first["id"]);
    // A member who does not read the mailbox cannot start one.
    let (status, _) = f.reply(&f.bob, "reply", thread).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM mail_drafts WHERE thread_id = $1")
        .bind(thread.to_string())
        .fetch_one(f.database.pool())
        .await
        .unwrap();
    assert_eq!(count, 2);
}

#[tokio::test]
async fn stale_version_conflict() {
    let f = Fixture::new().await;
    let (status, draft) = f
        .call(
            &f.ann,
            "POST",
            "/drafts",
            Some(
                json!({"mode": "new", "from_address_id": f.ann_primary.to_string(),
                "subject": "Hi", "body_html": "<p>a</p><script>x()</script>"}),
            ),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{draft}");
    assert_eq!(draft["body_html"], "<p>a</p>");
    let id = draft["id"].as_str().unwrap();
    let (status, one) = f
        .call(
            &f.ann,
            "PATCH",
            &format!("/drafts/{id}"),
            Some(json!({"version": 1, "subject": "First"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{one}");
    assert_eq!(one["version"], 2);
    let (status, stale) = f
        .call(
            &f.ann,
            "PATCH",
            &format!("/drafts/{id}"),
            Some(json!({"version": 1, "subject": "Second"})),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{stale}");
    assert_eq!(stale["code"], "draft_conflict");
    assert_eq!(stale["conflict"]["current"]["subject"], "First");
    assert_eq!(stale["conflict"]["current"]["version"], 2);
    // The send-as check holds on change: Ann cannot send as Sales.
    let (status, _) = f
        .call(
            &f.ann,
            "PATCH",
            &format!("/drafts/{id}"),
            Some(json!({"version": 2, "from_address_id": f.sales_primary.to_string()})),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // A new message needs a From, and it must be one Ann can send as.
    let (status, _) = f
        .call(&f.ann, "POST", "/drafts", Some(json!({"mode": "new"})))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = f
        .call(
            &f.ann,
            "POST",
            "/drafts",
            Some(json!({"mode": "new", "from_address_id": f.sales_primary.to_string()})),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn forward_copies_attachments() {
    let f = Fixture::new().await;
    let (_, thread) = f
        .deliver_raw(with_attachment(), "team@one.test", f.team_primary)
        .await;
    let (status, forward) = f.reply(&f.ann, "forward", thread).await;
    assert_eq!(status, StatusCode::CREATED, "{forward}");
    assert_eq!(forward["subject"], "Fwd: Report");
    assert_eq!(forward["to"], json!([]));
    let files = forward["files"].as_array().unwrap();
    assert_eq!(files.len(), 1, "{forward}");
    assert_eq!(files[0]["filename"], "report.pdf");
    assert!(files[0]["source_attachment_id"].is_string());
    // Attaching the message itself copies no file; back to a forward copies them again.
    let id = forward["id"].as_str().unwrap();
    let (_, attached) = f
        .call(
            &f.ann,
            "PATCH",
            &format!("/drafts/{id}"),
            Some(json!({"version": 1, "mode": "forward_attached"})),
        )
        .await;
    assert_eq!(attached["files"], json!([]));
    let (_, again) = f
        .call(
            &f.ann,
            "PATCH",
            &format!("/drafts/{id}"),
            Some(json!({"version": 2, "mode": "forward"})),
        )
        .await;
    assert_eq!(again["files"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn subject_prefix_not_doubled() {
    let f = Fixture::new().await;
    let (_, thread) = f
        .deliver_raw(reply_all_raw(), "team@one.test", f.team_primary)
        .await;
    // The message is "Re: Plans" already.
    let (_, reply) = f.reply(&f.ann, "reply", thread).await;
    assert_eq!(reply["subject"], "Re: Plans");
    let (_, forward) = f.reply(&f.ann, "forward", thread).await;
    assert_eq!(forward["subject"], "Fwd: Re: Plans");
}

#[tokio::test]
async fn draft_private_to_author() {
    let f = Fixture::new().await;
    let (_, thread) = f.deliver("team@one.test", f.team_primary).await;
    let (_, draft) = f.reply(&f.ann, "reply", thread).await;
    let id = draft["id"].as_str().unwrap();
    // The owner reads the same mailbox, and still sees nothing of Ann's draft.
    for (method, path, body) in [
        ("GET", format!("/drafts/{id}"), None),
        (
            "PATCH",
            format!("/drafts/{id}"),
            Some(json!({"version": 1, "subject": "x"})),
        ),
        ("DELETE", format!("/drafts/{id}"), None),
        (
            "PATCH",
            format!("/drafts/{id}/files/{}", Id::new_v7()),
            Some(json!({"as_link": true})),
        ),
    ] {
        let (status, _) = f.call(&f.owner, method, &path, body).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {path}");
    }
    let (status, _) = f.upload(&f.owner, id, "a.txt", b"x").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, list) = f.call(&f.owner, "GET", "/drafts", None).await;
    assert_eq!(list, json!([]));
    let (status, _) = f
        .call(&f.ann, "DELETE", &format!("/drafts/{id}"), None)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = f.call(&f.ann, "GET", &format!("/drafts/{id}"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn file_upload_and_remove() {
    let f = Fixture::new().await;
    let (_, draft) = f
        .call(
            &f.ann,
            "POST",
            "/drafts",
            Some(json!({"mode": "new", "from_address_id": f.ann_primary.to_string()})),
        )
        .await;
    let id = draft["id"].as_str().unwrap();
    let (status, file) = f.upload(&f.ann, id, "notes.txt", b"hello notes").await;
    assert_eq!(status, StatusCode::CREATED, "{file}");
    assert_eq!(file["filename"], "notes.txt");
    assert_eq!(file["size"], 11);
    assert_eq!(file["as_link"], false);
    let file_id = file["id"].as_str().unwrap();
    let (status, linked) = f
        .call(
            &f.ann,
            "PATCH",
            &format!("/drafts/{id}/files/{file_id}"),
            Some(json!({"as_link": true})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{linked}");
    assert_eq!(linked["as_link"], true);
    let (_, got) = f.call(&f.ann, "GET", &format!("/drafts/{id}"), None).await;
    assert_eq!(got["files"].as_array().unwrap().len(), 1);
    assert_eq!(got["version"], 1, "a file change is not a new version");
    let (status, _) = f
        .call(
            &f.ann,
            "DELETE",
            &format!("/drafts/{id}/files/{file_id}"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, got) = f.call(&f.ann, "GET", &format!("/drafts/{id}"), None).await;
    assert_eq!(got["files"], json!([]));
    // A request that is not a single file is refused.
    let request = Request::builder()
        .method("POST")
        .uri(format!(
            "/api/v1/workspaces/{}/mail/drafts/{id}/files",
            f.workspace
        ))
        .header(header::COOKIE, &f.ann.cookie)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let response = f.app().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn drafts_listed_newest_first() {
    let f = Fixture::new().await;
    let mut ids = Vec::new();
    for subject in ["one", "two", "three"] {
        let (_, draft) = f
            .call(
                &f.ann,
                "POST",
                "/drafts",
                Some(
                    json!({"mode": "new", "from_address_id": f.ann_primary.to_string(),
                    "subject": subject}),
                ),
            )
            .await;
        ids.push(draft["id"].as_str().unwrap().to_owned());
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    let (_, list) = f.call(&f.ann, "GET", "/drafts", None).await;
    let subjects: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["subject"].as_str().unwrap())
        .collect();
    assert_eq!(subjects, ["three", "two", "one"]);
    // Changing the oldest moves it to the top; the mailbox filter narrows the list.
    let (status, _) = f
        .call(
            &f.ann,
            "PATCH",
            &format!("/drafts/{}", ids[0]),
            Some(json!({"version": 1, "body_html": "<p>x</p>"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (_, list) = f.call(&f.ann, "GET", "/drafts", None).await;
    assert_eq!(list[0]["id"], ids[0].as_str());
    let (_, other) = f
        .call(
            &f.ann,
            "GET",
            &format!("/drafts?mailbox_id={}", f.team),
            None,
        )
        .await;
    assert_eq!(other, json!([]));
    let (_, mine) = f
        .call(
            &f.ann,
            "GET",
            &format!("/drafts?mailbox_id={}", f.ann_personal),
            None,
        )
        .await;
    assert_eq!(mine.as_array().unwrap().len(), 3);
}
