//! Reading mail (spec §5, §7, §8.3, §10): the thread list (one mailbox or all of the caller's),
//! a thread with its messages, organising (folder, read, star, labels; one or many threads),
//! unread counts, the client's mailbox list and search with operators. Only mailbox members
//! read a mailbox: everyone else gets `mail_not_found`, and search never returns their threads.

use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_mail::{AuthOutcome, DmarcPolicy, DmarcVerdict, Verdict};
use orbit_platform::{
    AttachmentMutationCoordinator, AuthenticatedUser, Id, LocalBlobStore, MailMode,
    PasswordService, TestDatabase, TimestampMillis, UploadLimits, UploadService,
};
use orbit_server::auth_routes::CookieMode;
use orbit_server::mail_intake::{InboundMessage, MailIntake, Recipient, Source};
use orbit_server::mail_routes::{MailSettings, MailState, mail_router};
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::repositories::mail::{
    IngestOutcome, MailFolder, MailRepository, MailboxKind, NewMailbox, parse_query,
};
use orbit_server::repositories::workspaces::WorkspaceRepository;
use serde_json::{Value, json};
use tower::ServiceExt;

const APP_KEY: [u8; 32] = [11; 32];

#[derive(Clone)]
struct Member {
    id: Id,
    membership_id: Id,
    cookie: String,
}

/// A mailbox with its primary address.
#[derive(Clone, Copy)]
struct Box {
    id: Id,
    address: Id,
}

struct Fixture {
    _root: tempfile::TempDir,
    database: TestDatabase,
    identity: Arc<IdentityRepository>,
    mail: Arc<MailRepository>,
    intake: MailIntake,
    app: axum::Router,
    workspace: Id,
    /// Another workspace of the owner.
    second: Id,
    owner: Member,
    /// "Ann Lee": a member of `shared` and owner of `personal`.
    ann: Member,
    /// A workspace member who reads no mailbox.
    bob: Member,
    domain: Id,
    /// `team@one.test`: the owner and Ann.
    shared: Box,
    /// `ann@one.test`: Ann's own.
    personal: Box,
    /// `owner@one.test`: only the owner.
    private: Box,
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
        let second = WorkspaceRepository::new((*database).clone())
            .create(setup.user_id, "Second".to_owned(), "request", now)
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
        let app = mail_router(MailState::new(
            Arc::clone(&identity),
            Arc::clone(&mail),
            CookieMode::secure(),
            MailSettings {
                enabled: true,
                hostname: Some("mx.orbit.test".to_owned()),
                mode: MailMode::Direct,
            },
        ));
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
            membership_id: owner_membership.parse().unwrap(),
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
        let placeholder = Box {
            id: Id::new_v7(),
            address: Id::new_v7(),
        };
        let mut fixture = Self {
            _root: root,
            database,
            identity,
            mail,
            intake,
            app,
            workspace: setup.workspace_id,
            second: second.id,
            ann: owner.clone(),
            bob: owner.clone(),
            owner,
            domain,
            shared: placeholder,
            personal: placeholder,
            private: placeholder,
        };
        fixture.ann = fixture.member("Ann Lee").await;
        fixture.bob = fixture.member("Bob").await;
        fixture.shared = fixture
            .mailbox(
                MailboxKind::Shared,
                "team",
                None,
                vec![fixture.owner.membership_id, fixture.ann.membership_id],
            )
            .await;
        fixture.personal = fixture
            .mailbox(
                MailboxKind::Personal,
                "ann",
                Some(fixture.ann.membership_id),
                Vec::new(),
            )
            .await;
        fixture.private = fixture
            .mailbox(
                MailboxKind::Shared,
                "owner",
                None,
                vec![fixture.owner.membership_id],
            )
            .await;
        fixture
    }

    /// A workspace member with a session.
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
            id,
            membership_id,
            cookie: format!("__Host-orbit_session={}", session.token),
        }
    }

    async fn mailbox(
        &self,
        kind: MailboxKind,
        local: &str,
        owner: Option<Id>,
        members: Vec<Id>,
    ) -> Box {
        let mailbox = self
            .mail
            .create_mailbox(
                self.workspace,
                self.owner.id,
                NewMailbox {
                    kind,
                    name: format!("Box {local}"),
                    domain_id: self.domain,
                    local_part: local.to_owned(),
                    owner_membership_id: owner,
                    member_ids: members,
                },
                "request",
            )
            .await
            .unwrap();
        Box {
            id: mailbox.id,
            address: mailbox.addresses[0].id,
        }
    }

    /// Receives and ingests `raw` for `mailbox`; returns its thread.
    async fn deliver_with(&self, mailbox: Box, raw: &[u8], dmarc: DmarcVerdict) -> Id {
        let ids = self
            .intake
            .accept(InboundMessage {
                source: Source::Smtp,
                trace_len: 0,
                raw: raw.to_vec(),
                mail_from: Some("ann@sender.test".to_owned()),
                recipients: vec![Recipient::Mailbox {
                    workspace_id: self.workspace,
                    address_id: mailbox.address,
                    rcpt: "someone@one.test".to_owned(),
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
        let IngestOutcome::Ingested { deliveries, .. } = outcome else {
            panic!("not ingested: {outcome:?}");
        };
        deliveries[0].thread_id
    }

    async fn deliver(&self, mailbox: Box, raw: &[u8]) -> Id {
        self.deliver_with(mailbox, raw, DmarcVerdict::Pass).await
    }

    async fn call_in(
        &self,
        member: &Member,
        workspace: Id,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let builder = Request::builder()
            .method(method)
            .uri(format!("/api/v1/workspaces/{workspace}/mail{path}"))
            .header(header::COOKIE, &member.cookie);
        let request = match body {
            Some(value) => builder
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(value.to_string())),
            None => builder.body(Body::empty()),
        }
        .unwrap();
        let response = self.app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn call(
        &self,
        member: &Member,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        self.call_in(member, self.workspace, method, path, body)
            .await
    }

    /// A call that must succeed with 200.
    async fn ok(&self, member: &Member, method: &str, path: &str, body: Option<Value>) -> Value {
        let (status, value) = self.call(member, method, path, body).await;
        assert_eq!(status, StatusCode::OK, "{method} {path}: {value}");
        value
    }

    /// The thread ids of a list or search page.
    async fn ids(&self, member: &Member, path: &str) -> Vec<String> {
        ids(&self.ok(member, "GET", path, None).await)
    }

    async fn mail_events(&self) -> Vec<(Value, Value)> {
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT recipients_json, frame FROM live_events WHERE workspace_id = $1 ORDER BY seq",
        )
        .bind(self.workspace.to_string())
        .fetch_all(self.database.pool())
        .await
        .unwrap();
        rows.into_iter()
            .map(|(recipients, frame)| {
                (
                    serde_json::from_str(&recipients).unwrap(),
                    serde_json::from_str::<Value>(&frame).unwrap(),
                )
            })
            .filter(|(_, frame)| frame["topic"] == "mail")
            .map(|(recipients, frame)| (recipients, frame["event"].clone()))
            .collect()
    }
}

fn ids(page: &Value) -> Vec<String> {
    page["items"]
        .as_array()
        .unwrap_or_else(|| panic!("not a page: {page}"))
        .iter()
        .map(|item| item["id"].as_str().unwrap().to_owned())
        .collect()
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

/// A plain message; `id` makes the bytes unique.
fn mail(from: &str, id: &str, subject: &str, body: &str) -> Vec<u8> {
    format!(
        "From: {from}\r\nTo: Team <team@one.test>\r\nSubject: {subject}\r\n\
         Message-ID: <{id}@sender.test>\r\nDate: Thu, 22 Oct 2026 10:00:00 +0000\r\n\r\n{body}\r\n"
    )
    .into_bytes()
}

/// A message with a PDF attachment.
fn with_attachment(from: &str, id: &str, subject: &str, body: &str) -> Vec<u8> {
    format!(
        "From: {from}\r\nTo: team@one.test\r\nSubject: {subject}\r\nMessage-ID: <{id}@sender.test>\r\n\
         MIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"b\"\r\n\r\n\
         --b\r\nContent-Type: text/plain\r\n\r\n{body}\r\n\
         --b\r\nContent-Type: application/pdf\r\nContent-Disposition: attachment; filename=\"doc.pdf\"\r\n\
         Content-Transfer-Encoding: base64\r\n\r\nJVBERi0xLjQK\r\n--b--\r\n"
    )
    .into_bytes()
}

#[test]
fn search_operators_parse() {
    let query = parse_query(
        "from:ann to:\"Team Box\" subject:invoice label:Work in:archive is:unread is:starred \
         has:attachment after:2026-10-01 before:2026-10-31 paid  October",
    );
    assert_eq!(query.text, "paid October");
    assert_eq!(query.from.as_deref(), Some("ann"));
    assert_eq!(query.to.as_deref(), Some("Team Box"));
    assert_eq!(query.subject.as_deref(), Some("invoice"));
    assert_eq!(query.label.as_deref(), Some("Work"));
    assert_eq!(query.folder, Some(MailFolder::Archive));
    assert!(query.unread && query.starred && query.has_attachment);
    // 2026-10-01T00:00:00Z and 2026-10-31T00:00:00Z.
    assert_eq!(query.after, Some(1_790_812_800_000));
    assert_eq!(query.before, Some(1_793_404_800_000));

    // Unknown operators and bad values stay words; operators are case-insensitive.
    let query = parse_query("FROM:Bob is:weird before:yesterday in:nowhere hello");
    assert_eq!(query.from.as_deref(), Some("Bob"));
    assert_eq!(query.text, "is:weird before:yesterday in:nowhere hello");
    assert_eq!(query.before, None);
    assert_eq!(query.folder, None);
    assert!(!query.unread);

    assert_eq!(parse_query("   ").text, "");
}

#[tokio::test]
async fn the_unified_list_has_only_the_callers_mailboxes() {
    let fixture = Fixture::new().await;
    let team = fixture
        .deliver(
            fixture.shared,
            &mail("Carl <carl@sender.test>", "t1", "Team news", "Hello team."),
        )
        .await;
    let own = fixture
        .deliver(
            fixture.personal,
            &mail("Carl <carl@sender.test>", "t2", "For Ann", "Hello Ann."),
        )
        .await;
    let private = fixture
        .deliver(
            fixture.private,
            &mail("Carl <carl@sender.test>", "t3", "Owner only", "Secret."),
        )
        .await;

    let mut ann = fixture.ids(&fixture.ann, "/threads").await;
    ann.sort();
    let mut expected = vec![team.to_string(), own.to_string()];
    expected.sort();
    assert_eq!(ann, expected);
    let mut owner = fixture.ids(&fixture.owner, "/threads").await;
    owner.sort();
    let mut expected = vec![team.to_string(), private.to_string()];
    expected.sort();
    assert_eq!(owner, expected, "the owner does not read Ann's own mailbox");
    assert!(fixture.ids(&fixture.bob, "/threads").await.is_empty());

    // One mailbox.
    assert_eq!(
        fixture
            .ids(
                &fixture.ann,
                &format!("/threads?mailbox_id={}", fixture.personal.id)
            )
            .await,
        vec![own.to_string()]
    );
    let (status, value) = fixture
        .call(
            &fixture.ann,
            "GET",
            &format!("/threads?mailbox_id={}", fixture.private.id),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{value}");
    assert_eq!(value["code"], "mail_not_found");

    // The thread itself.
    let thread = fixture
        .ok(&fixture.ann, "GET", &format!("/threads/{team}"), None)
        .await;
    assert_eq!(thread["thread"]["subject"], "Team news");
    assert_eq!(
        thread["thread"]["mailbox_id"],
        fixture.shared.id.to_string()
    );
    assert_eq!(thread["thread"]["folder"], "inbox");
    assert_eq!(thread["thread"]["unread"], true);
    assert_eq!(thread["thread"]["message_count"], 1);
    assert_eq!(
        thread["thread"]["from"],
        json!([{ "name": "Carl", "address": "carl@sender.test" }])
    );
    let message = &thread["messages"][0];
    assert_eq!(message["body_text"].as_str().unwrap().trim(), "Hello team.");
    assert_eq!(message["from"]["address"], "carl@sender.test");
    assert_eq!(
        message["to"],
        json!([{ "name": "Team", "address": "team@one.test" }])
    );
    assert_eq!(message["has_html"], false);
    assert_eq!(message["from_external"], true);
    assert_eq!(message["external"], false);
    assert_eq!(message["auth"]["dmarc"], "pass");
    assert_eq!(message["auth"]["spf"], "pass");
    assert_eq!(message["auth"]["aligned_domain"], "sender.test");
    assert_eq!(message["lookalike_member"], Value::Null);
    assert_eq!(message["load_images_default"], false);

    for (member, thread) in [
        (&fixture.ann, private),
        (&fixture.bob, team),
        (&fixture.owner, own),
    ] {
        for (method, path, body) in [
            ("GET", format!("/threads/{thread}"), None),
            (
                "PATCH",
                format!("/threads/{thread}"),
                Some(json!({ "starred": true })),
            ),
        ] {
            let (status, value) = fixture.call(member, method, &path, body).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{method} {path}: {value}");
            assert_eq!(value["code"], "mail_not_found");
        }
    }
    // Another workspace of the owner names nothing here.
    let (status, _) = fixture
        .call_in(
            &fixture.owner,
            fixture.second,
            "GET",
            &format!("/threads/{team}"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // A user outside the workspace.
    let stranger = fixture.member("Stranger").await;
    sqlx::query("DELETE FROM memberships WHERE id = $1")
        .bind(stranger.membership_id.to_string())
        .execute(fixture.database.pool())
        .await
        .unwrap();
    let (status, _) = fixture.call(&stranger, "GET", "/threads", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn search_matches_operators_and_never_other_mailboxes() {
    let fixture = Fixture::new().await;
    let wanted = fixture
        .deliver(
            fixture.shared,
            &with_attachment(
                "Ann Sender <ann@sender.test>",
                "s1",
                "Invoice 42",
                "Please pay.",
            ),
        )
        .await;
    // Not from Ann.
    fixture
        .deliver(
            fixture.shared,
            &with_attachment("Bob <bob@else.test>", "s2", "Invoice 43", "Please pay."),
        )
        .await;
    // No attachment.
    fixture
        .deliver(
            fixture.shared,
            &mail(
                "Ann Sender <ann@sender.test>",
                "s3",
                "Invoice 44",
                "Please pay.",
            ),
        )
        .await;
    // No invoice.
    fixture
        .deliver(
            fixture.shared,
            &with_attachment("Ann Sender <ann@sender.test>", "s4", "Lunch", "Pizza?"),
        )
        .await;
    // A mailbox Ann does not read.
    let hidden = fixture
        .deliver(
            fixture.private,
            &with_attachment("Ann Sender <ann@sender.test>", "s5", "Invoice 45", "Pay."),
        )
        .await;

    let search = |q: &str| format!("/search?q={}", urlencode(q));
    assert_eq!(
        fixture
            .ids(&fixture.ann, &search("from:ann has:attachment invoice"))
            .await,
        vec![wanted.to_string()]
    );
    let owner = fixture
        .ids(&fixture.owner, &search("from:ann has:attachment invoice"))
        .await;
    assert_eq!(owner.len(), 2, "{owner:?}");
    assert!(owner.contains(&hidden.to_string()));
    assert!(
        fixture
            .ids(&fixture.bob, &search("invoice"))
            .await
            .is_empty()
    );

    // Words alone, prefix on the last one, subject and body.
    assert_eq!(fixture.ids(&fixture.ann, &search("invo")).await.len(), 3);
    assert_eq!(fixture.ids(&fixture.ann, &search("pizza")).await.len(), 1);
    assert_eq!(
        fixture
            .ids(&fixture.ann, &search("subject:lunch"))
            .await
            .len(),
        1
    );
    assert_eq!(
        fixture
            .ids(&fixture.ann, &search("from:bob@else.test"))
            .await
            .len(),
        1
    );
    assert_eq!(
        fixture
            .ids(&fixture.ann, &search("to:team@one"))
            .await
            .len(),
        4
    );
    // FTS syntax is plain text.
    assert!(
        fixture
            .ids(&fixture.ann, &search("invoice NEAR(\"x\" OR"))
            .await
            .is_empty()
    );
    // Nothing to search for.
    assert!(fixture.ids(&fixture.ann, &search("")).await.is_empty());

    // Labels, stars, read state and folders.
    let (_, label) = fixture
        .call(
            &fixture.owner,
            "POST",
            "/labels",
            Some(json!({ "name": "Work", "color": "#336699" })),
        )
        .await;
    fixture
        .ok(
            &fixture.ann,
            "PATCH",
            &format!("/threads/{wanted}"),
            Some(json!({ "add_labels": [label["id"]], "starred": true, "read": true })),
        )
        .await;
    assert_eq!(
        fixture.ids(&fixture.ann, &search("label:work")).await,
        vec![wanted.to_string()]
    );
    assert_eq!(
        fixture.ids(&fixture.ann, &search("is:starred")).await,
        vec![wanted.to_string()]
    );
    assert!(
        !fixture
            .ids(&fixture.ann, &search("is:unread invoice"))
            .await
            .contains(&wanted.to_string())
    );
    assert!(
        fixture
            .ids(&fixture.owner, &search("is:starred"))
            .await
            .is_empty(),
        "stars are each member's own"
    );
    fixture
        .ok(
            &fixture.ann,
            "PATCH",
            &format!("/threads/{wanted}"),
            Some(json!({ "folder": "archive" })),
        )
        .await;
    assert_eq!(
        fixture.ids(&fixture.ann, &search("in:archive")).await,
        vec![wanted.to_string()]
    );
    assert_eq!(
        fixture.ids(&fixture.ann, &search("invoice 42")).await,
        vec![wanted.to_string()],
        "archived mail is found"
    );
    fixture
        .ok(
            &fixture.ann,
            "PATCH",
            &format!("/threads/{wanted}"),
            Some(json!({ "folder": "trash" })),
        )
        .await;
    assert!(
        fixture
            .ids(&fixture.ann, &search("invoice 42"))
            .await
            .is_empty(),
        "trash is left out without in:trash"
    );
    assert_eq!(
        fixture.ids(&fixture.ann, &search("in:trash invoice")).await,
        vec![wanted.to_string()]
    );
    // Dates (the mail arrived today).
    assert_eq!(
        fixture
            .ids(&fixture.ann, &search("after:2000-01-01 invoice"))
            .await
            .len(),
        2
    );
    assert!(
        fixture
            .ids(&fixture.ann, &search("before:2000-01-01 invoice"))
            .await
            .is_empty()
    );
}

#[tokio::test]
async fn trash_and_restore() {
    let fixture = Fixture::new().await;
    let thread = fixture
        .deliver(
            fixture.shared,
            &mail("Carl <carl@sender.test>", "r1", "Old news", "Archive me."),
        )
        .await;
    let path = format!("/threads/{thread}");
    let archived = fixture
        .ok(
            &fixture.ann,
            "PATCH",
            &path,
            Some(json!({ "folder": "archive" })),
        )
        .await;
    assert_eq!(archived["folder"], "archive");
    let trashed = fixture
        .ok(
            &fixture.ann,
            "PATCH",
            &path,
            Some(json!({ "folder": "trash" })),
        )
        .await;
    assert_eq!(trashed["folder"], "trash");
    let (trashed_at, restore): (Option<i64>, Option<String>) =
        sqlx::query_as("SELECT trashed_at, restore_folder FROM mail_threads WHERE id = $1")
            .bind(thread.to_string())
            .fetch_one(fixture.database.pool())
            .await
            .unwrap();
    assert!(trashed_at.is_some());
    assert_eq!(restore.as_deref(), Some("archive"));
    assert!(
        fixture
            .ids(&fixture.ann, "/threads?folder=archive")
            .await
            .is_empty()
    );
    assert_eq!(
        fixture.ids(&fixture.ann, "/threads?folder=trash").await,
        vec![thread.to_string()]
    );
    // Trash twice keeps the folder to restore.
    fixture
        .ok(
            &fixture.ann,
            "PATCH",
            &path,
            Some(json!({ "folder": "trash" })),
        )
        .await;

    let restored = fixture
        .ok(
            &fixture.ann,
            "PATCH",
            &path,
            Some(json!({ "restore": true })),
        )
        .await;
    assert_eq!(restored["folder"], "archive");
    let (trashed_at, restore): (Option<i64>, Option<String>) =
        sqlx::query_as("SELECT trashed_at, restore_folder FROM mail_threads WHERE id = $1")
            .bind(thread.to_string())
            .fetch_one(fixture.database.pool())
            .await
            .unwrap();
    assert_eq!((trashed_at, restore), (None, None));

    // Moving out of the trash by folder clears it too; restore without a folder → inbox.
    fixture
        .ok(
            &fixture.ann,
            "PATCH",
            &path,
            Some(json!({ "folder": "trash" })),
        )
        .await;
    let moved = fixture
        .ok(
            &fixture.ann,
            "PATCH",
            &path,
            Some(json!({ "folder": "inbox" })),
        )
        .await;
    assert_eq!(moved["folder"], "inbox");
    sqlx::query(
        "UPDATE mail_threads SET folder = 'trash', trashed_at = 1, restore_folder = NULL WHERE id = $1",
    )
    .bind(thread.to_string())
    .execute(fixture.database.pool())
    .await
    .unwrap();
    let restored = fixture
        .ok(
            &fixture.ann,
            "PATCH",
            &path,
            Some(json!({ "restore": true })),
        )
        .await;
    assert_eq!(restored["folder"], "inbox");

    // A bad folder and restore together with a folder are refused.
    let (status, _) = fixture
        .call(
            &fixture.ann,
            "PATCH",
            &path,
            Some(json!({ "folder": "sent" })),
        )
        .await;
    assert!(status.is_client_error(), "{status}");
    let (status, value) = fixture
        .call(
            &fixture.ann,
            "PATCH",
            &path,
            Some(json!({ "folder": "inbox", "restore": true })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{value}");
}

#[tokio::test]
async fn bulk_star_read_and_labels_with_live_events() {
    let fixture = Fixture::new().await;
    let first = fixture
        .deliver(
            fixture.shared,
            &mail("Carl <carl@sender.test>", "b1", "One", "First."),
        )
        .await;
    let second = fixture
        .deliver(
            fixture.shared,
            &mail("Carl <carl@sender.test>", "b2", "Two", "Second."),
        )
        .await;
    let hidden = fixture
        .deliver(
            fixture.private,
            &mail("Carl <carl@sender.test>", "b3", "Three", "Third."),
        )
        .await;
    let before = fixture.mail_events().await.len();

    let result = fixture
        .ok(
            &fixture.ann,
            "POST",
            "/threads/bulk",
            Some(json!({
                "ids": [first.to_string(), second.to_string(), hidden.to_string()],
                "starred": true,
            })),
        )
        .await;
    let mut updated = ids(&result);
    updated.sort();
    let mut expected = vec![first.to_string(), second.to_string()];
    expected.sort();
    assert_eq!(updated, expected, "a thread Ann cannot read is left alone");
    let mut starred = fixture.ids(&fixture.ann, "/threads?starred=true").await;
    starred.sort();
    assert_eq!(starred, expected);
    assert!(
        fixture
            .ids(&fixture.owner, "/threads?starred=true")
            .await
            .is_empty()
    );
    let starred_hidden: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM mail_thread_user_state WHERE thread_id = $1")
            .bind(hidden.to_string())
            .fetch_one(fixture.database.pool())
            .await
            .unwrap();
    assert_eq!(starred_hidden, 0);

    // Live events for the two threads, to the mailbox members (owner and Ann) only.
    let events = fixture.mail_events().await;
    let new_events = &events[before..];
    assert_eq!(new_events.len(), 2, "{new_events:?}");
    for (recipients, event) in new_events {
        assert_eq!(event["type"], "thread.updated");
        assert_eq!(event["mailbox_id"], fixture.shared.id.to_string());
        let text = recipients.to_string();
        assert!(text.contains(&fixture.ann.id.to_string()), "{text}");
        assert!(text.contains(&fixture.owner.id.to_string()), "{text}");
        assert!(!text.contains(&fixture.bob.id.to_string()), "{text}");
    }

    // Counts and the mailbox list follow the read state.
    let counts = fixture.ok(&fixture.ann, "GET", "/counts", None).await;
    let shared = counts["mailboxes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["mailbox_id"] == fixture.shared.id.to_string())
        .unwrap()
        .clone();
    assert_eq!(shared["inbox"], 2);
    assert_eq!(shared["archive"], 0);
    fixture
        .ok(
            &fixture.ann,
            "POST",
            "/threads/bulk",
            Some(json!({ "ids": [first.to_string()], "read": true, "folder": "archive" })),
        )
        .await;
    let counts = fixture.ok(&fixture.ann, "GET", "/counts", None).await;
    let shared = counts["mailboxes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["mailbox_id"] == fixture.shared.id.to_string())
        .unwrap()
        .clone();
    assert_eq!(shared["inbox"], 1);
    assert_eq!(shared["archive"], 0, "the archived thread was read");
    let owner_counts = fixture.ok(&fixture.owner, "GET", "/counts", None).await;
    let owner_shared = owner_counts["mailboxes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["mailbox_id"] == fixture.shared.id.to_string())
        .unwrap()
        .clone();
    assert_eq!(
        owner_shared["archive"], 1,
        "read state is each member's own"
    );

    let me = fixture.ok(&fixture.ann, "GET", "/me", None).await;
    assert_eq!(me["can_manage"], false);
    let mailboxes = me["mailboxes"].as_array().unwrap();
    assert_eq!(mailboxes.len(), 2, "{me}");
    let team = mailboxes
        .iter()
        .find(|mailbox| mailbox["id"] == fixture.shared.id.to_string())
        .unwrap();
    assert_eq!(team["primary_address"], "team@one.test");
    assert_eq!(team["kind"], "shared");
    assert_eq!(team["unread"], 1);
    let owner_me = fixture.ok(&fixture.owner, "GET", "/me", None).await;
    assert_eq!(owner_me["can_manage"], true);
    assert_eq!(owner_me["mailboxes"].as_array().unwrap().len(), 2);

    // Unread again.
    let unread = fixture
        .ok(
            &fixture.ann,
            "PATCH",
            &format!("/threads/{first}"),
            Some(json!({ "read": false })),
        )
        .await;
    assert_eq!(unread["unread"], true);

    // Labels: added and removed; a label of another workspace is refused.
    let (_, label) = fixture
        .call(
            &fixture.owner,
            "POST",
            "/labels",
            Some(json!({ "name": "Todo", "color": "#aa0000" })),
        )
        .await;
    let labelled = fixture
        .ok(
            &fixture.ann,
            "POST",
            "/threads/bulk",
            Some(json!({ "ids": [first.to_string(), second.to_string()], "add_labels": [label["id"]] })),
        )
        .await;
    for item in labelled["items"].as_array().unwrap() {
        assert_eq!(item["labels"], json!([label["id"]]));
    }
    assert_eq!(
        fixture
            .ids(
                &fixture.ann,
                &format!("/threads?label_id={}", label["id"].as_str().unwrap())
            )
            .await
            .len(),
        2,
        "a label lists every folder but spam and trash"
    );
    let cleared = fixture
        .ok(
            &fixture.ann,
            "PATCH",
            &format!("/threads/{first}"),
            Some(json!({ "remove_labels": [label["id"]] })),
        )
        .await;
    assert_eq!(cleared["labels"], json!([]));
    let (_, other) = fixture
        .call_in(
            &fixture.owner,
            fixture.second,
            "POST",
            "/labels",
            Some(json!({ "name": "Elsewhere", "color": "#00aa00" })),
        )
        .await;
    let (status, value) = fixture
        .call(
            &fixture.ann,
            "PATCH",
            &format!("/threads/{first}"),
            Some(json!({ "add_labels": [other["id"]] })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{value}");
}

#[tokio::test]
async fn a_member_name_from_outside_is_a_lookalike() {
    let fixture = Fixture::new().await;
    let forged = fixture
        .deliver(
            fixture.shared,
            &mail(
                "\" ann lee \" <ann.lee@evil.test>",
                "l1",
                "Urgent",
                "Wire money.",
            ),
        )
        .await;
    let real = fixture
        .deliver(
            fixture.shared,
            &mail("Ann Lee <ann@one.test>", "l2", "Lunch", "Pizza?"),
        )
        .await;
    let stranger = fixture
        .deliver(
            fixture.shared,
            b"From: Dora <dora@else.test>\r\nTo: team@one.test\r\nSubject: Hi\r\n\
Message-ID: <l3@sender.test>\r\nMIME-Version: 1.0\r\n\
Content-Type: multipart/alternative; boundary=\"a\"\r\n\r\n\
--a\r\nContent-Type: text/plain\r\n\r\nHello.\r\n\
--a\r\nContent-Type: text/html\r\n\r\n<p>Hello.</p>\r\n--a--\r\n",
        )
        .await;

    let message = |thread: Value| thread["messages"][0].clone();
    let forged = message(
        fixture
            .ok(&fixture.owner, "GET", &format!("/threads/{forged}"), None)
            .await,
    );
    assert_eq!(forged["lookalike_member"], "Ann Lee");
    assert_eq!(forged["from_external"], true);
    let real = message(
        fixture
            .ok(&fixture.owner, "GET", &format!("/threads/{real}"), None)
            .await,
    );
    assert_eq!(real["lookalike_member"], Value::Null, "Ann's own address");
    assert_eq!(real["from_external"], false);
    let stranger = message(
        fixture
            .ok(&fixture.owner, "GET", &format!("/threads/{stranger}"), None)
            .await,
    );
    assert_eq!(stranger["lookalike_member"], Value::Null);
    assert_eq!(stranger["has_html"], true);
    assert_eq!(real["has_html"], false);
}

#[tokio::test]
async fn images_load_by_default_only_for_a_chosen_sender_that_passed_dmarc() {
    let fixture = Fixture::new().await;
    let passed = fixture
        .deliver(
            fixture.shared,
            &mail("Shop <news@shop.test>", "i1", "Sale", "Buy."),
        )
        .await;
    let failed = fixture
        .deliver_with(
            fixture.shared,
            &mail("Shop <news@shop.test>", "i2", "Sale again", "Buy now."),
            DmarcVerdict::Fail {
                policy: DmarcPolicy::Reject,
            },
        )
        .await;
    sqlx::query(
        "INSERT INTO mail_sender_prefs (membership_id, address, load_images, updated_at) \
         VALUES ($1, 'news@shop.test', 1, 0)",
    )
    .bind(fixture.ann.membership_id.to_string())
    .execute(fixture.database.pool())
    .await
    .unwrap();

    let view = |member: Member, thread: Id| {
        let fixture = &fixture;
        async move {
            fixture
                .ok(&member, "GET", &format!("/threads/{thread}"), None)
                .await["messages"][0]
                .clone()
        }
    };
    assert_eq!(
        view(fixture.ann.clone(), passed).await["load_images_default"],
        true
    );
    assert_eq!(
        view(fixture.owner.clone(), passed).await["load_images_default"],
        false,
        "the choice is Ann's"
    );
    let failed = view(fixture.ann.clone(), failed).await;
    assert_eq!(failed["load_images_default"], false);
    assert_eq!(failed["auth"]["dmarc"], "fail");
    // The DMARC failure went to Spam.
    assert_eq!(
        fixture
            .ids(&fixture.ann, "/threads?folder=spam")
            .await
            .len(),
        1
    );
}

#[tokio::test]
async fn cursor_paging_is_stable_under_new_mail() {
    let fixture = Fixture::new().await;
    let mut delivered = Vec::new();
    for n in 0..5 {
        delivered.push(
            fixture
                .deliver(
                    fixture.shared,
                    &mail(
                        "Carl <carl@sender.test>",
                        &format!("p{n}"),
                        &format!("Page {n}"),
                        "Body.",
                    ),
                )
                .await
                .to_string(),
        );
        // Distinct arrival times.
        tokio::time::sleep(std::time::Duration::from_millis(3)).await;
    }
    let first = fixture
        .ok(&fixture.ann, "GET", "/threads?limit=2", None)
        .await;
    let mut seen = ids(&first);
    assert_eq!(seen, vec![delivered[4].clone(), delivered[3].clone()]);
    let mut cursor = first["cursor"].as_str().unwrap().to_owned();

    // New mail arrives at the top meanwhile; the next pages neither repeat nor skip.
    fixture
        .deliver(
            fixture.shared,
            &mail("Carl <carl@sender.test>", "p-new", "Newest", "Body."),
        )
        .await;
    loop {
        let page = fixture
            .ok(
                &fixture.ann,
                "GET",
                &format!("/threads?limit=2&cursor={}", urlencode(&cursor)),
                None,
            )
            .await;
        seen.extend(ids(&page));
        match page["cursor"].as_str() {
            Some(next) => cursor = next.to_owned(),
            None => break,
        }
    }
    let mut expected = delivered.clone();
    expected.reverse();
    assert_eq!(seen, expected);

    let (status, value) = fixture
        .call(&fixture.ann, "GET", "/threads?cursor=garbage", None)
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{value}");
    let (status, _) = fixture
        .call(&fixture.ann, "GET", "/threads?limit=0", None)
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

/// Percent-encodes a query value.
fn urlencode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}
