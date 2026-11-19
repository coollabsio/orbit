//! Compose support (spec §7, D13, U6): the addresses a member can send as, the default address of
//! a reply, signatures, personal snippets and the undo-send delay.

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
use orbit_server::repositories::mail::{
    IngestOutcome, MailError, MailRepository, MailboxKind, NewMailbox,
};
use serde_json::{Value, json};
use tower::ServiceExt;

const APP_KEY: [u8; 32] = [29; 32];

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

    /// Delivers a message to the team mailbox sent to `rcpt` (an address of it); returns its
    /// message id and thread id.
    async fn deliver(&self, rcpt: &str, address_id: Id) -> (Id, Id) {
        let raw = format!(
            "From: Carl <carl@sender.test>\r\nTo: {rcpt}\r\nSubject: Hello\r\n\
             Message-ID: <m1@sender.test>\r\n\r\nHi\r\n"
        );
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

fn addresses(list: &[orbit_server::repositories::mail::FromAddress]) -> Vec<String> {
    list.iter().map(|a| a.address.clone()).collect()
}

#[tokio::test]
async fn lists_own_aliases_and_shared_mailboxes() {
    let f = Fixture::new().await;
    let ann = f
        .mail
        .from_addresses(f.ann.membership, f.workspace, None)
        .await
        .unwrap();
    // Mailboxes by name, the primary address first.
    assert_eq!(
        addresses(&ann),
        ["ann@one.test", "team@one.test", "support@one.test"]
    );
    assert_eq!(ann[0].display_name, "Ann Lee");
    assert_eq!(ann[1].display_name, "Team");
    assert_eq!(ann[2].display_name, "Team");
    assert_eq!(ann[0].mailbox_id, f.ann_personal);
    // The fixture domain has no DNS report, so it cannot send yet.
    assert!(matches!(
        ann[1].ready,
        orbit_server::repositories::mail::SendReadiness::NotReady(_)
    ));
    // The owner is no member of Ann's personal mailbox.
    let owner = f
        .mail
        .from_addresses(f.owner.membership, f.workspace, None)
        .await
        .unwrap();
    assert_eq!(
        addresses(&owner),
        ["sales@one.test", "team@one.test", "support@one.test"]
    );
    assert!(
        f.mail
            .from_addresses(f.bob.membership, f.workspace, None)
            .await
            .unwrap()
            .is_empty()
    );
    let (status, body) = f.call(&f.ann, "GET", "/from-addresses", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.as_array().unwrap().len(), 3);
    assert_eq!(body[0]["address"], "ann@one.test");
    assert_eq!(body[0]["ready"], "not_ready");
}

#[tokio::test]
async fn reply_limits_to_thread_mailbox() {
    let f = Fixture::new().await;
    let (_, thread) = f.deliver("team@one.test", f.team_primary).await;
    let reply = f
        .mail
        .from_addresses(f.owner.membership, f.workspace, Some(f.team))
        .await
        .unwrap();
    assert_eq!(addresses(&reply), ["team@one.test", "support@one.test"]);
    let (status, body) = f
        .call(
            &f.ann,
            "GET",
            &format!("/from-addresses?thread_id={thread}"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let list: Vec<&str> = body
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["address"].as_str().unwrap())
        .collect();
    assert_eq!(list, ["team@one.test", "support@one.test"]);
    // A member of other mailboxes does not read the thread.
    let (status, body) = f
        .call(
            &f.bob,
            "GET",
            &format!("/from-addresses?thread_id={thread}"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    // Sales is not Ann's mailbox: nothing to send from there.
    assert!(
        f.mail
            .from_addresses(f.ann.membership, f.workspace, Some(f.sales))
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn reply_from_alias_drops_plus_tag() {
    let f = Fixture::new().await;
    let (message, _) = f.deliver("Support+News@one.test", f.team_alias).await;
    let from = f
        .mail
        .reply_from(f.ann.membership, message, f.team)
        .await
        .unwrap();
    assert_eq!(from, Some(f.team_alias));
    // Even when the stored address was removed from the receipt, the written address decides.
    sqlx::query("UPDATE mail_receipt_recipients SET address_id = NULL")
        .execute(f.database.pool())
        .await
        .unwrap();
    assert_eq!(
        f.mail
            .reply_from(f.ann.membership, message, f.team)
            .await
            .unwrap(),
        Some(f.team_alias)
    );
    // Not a reader of the mailbox: not found.
    assert!(matches!(
        f.mail.reply_from(f.bob.membership, message, f.team).await,
        Err(MailError::NotFound)
    ));
    // A message that no address of this mailbox received: no default.
    assert_eq!(
        f.mail
            .reply_from(f.owner.membership, Id::new_v7(), f.team)
            .await
            .unwrap(),
        None
    );
}

#[tokio::test]
async fn signature_per_address() {
    let f = Fixture::new().await;
    let stored = f
        .mail
        .put_signature(
            f.ann.membership,
            f.workspace,
            f.team_primary,
            "<p>Team <b>Ann</b></p><script>alert(1)</script>",
        )
        .await
        .unwrap();
    assert!(
        stored.contains("<b>Ann</b>") && !stored.contains("script"),
        "{stored}"
    );
    f.mail
        .put_signature(f.ann.membership, f.workspace, f.ann_primary, "<p>Ann</p>")
        .await
        .unwrap();
    // A second save replaces the first.
    f.mail
        .put_signature(
            f.ann.membership,
            f.workspace,
            f.ann_primary,
            "<p>Ann L.</p>",
        )
        .await
        .unwrap();
    let list = f
        .mail
        .from_addresses(f.ann.membership, f.workspace, None)
        .await
        .unwrap();
    assert_eq!(list[0].signature_html.as_deref(), Some("<p>Ann L.</p>"));
    assert!(list[1].signature_html.as_deref().unwrap().contains("Team"));
    assert_eq!(list[2].signature_html, None);
    // Not an address the member can send as: not found, whatever it is.
    for address in [f.sales_primary, Id::new_v7()] {
        assert!(matches!(
            f.mail
                .put_signature(f.ann.membership, f.workspace, address, "x")
                .await,
            Err(MailError::NotFound)
        ));
    }
    assert!(matches!(
        f.mail
            .put_signature(f.owner.membership, f.workspace, f.ann_primary, "x")
            .await,
        Err(MailError::NotFound)
    ));
    // Another member's signature is not readable either way.
    let (status, _) = f
        .call(
            &f.owner,
            "GET",
            &format!("/signatures/{}", f.team_primary),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        f.mail
            .signature(f.owner.membership, f.workspace, f.team_primary)
            .await
            .unwrap(),
        None
    );
    let (status, _) = f
        .call(
            &f.ann,
            "DELETE",
            &format!("/signatures/{}", f.team_primary),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        f.mail
            .signature(f.ann.membership, f.workspace, f.team_primary)
            .await
            .unwrap(),
        None
    );
    let (status, _) = f
        .call(
            &f.ann,
            "PUT",
            &format!("/signatures/{}", f.sales_primary),
            Some(json!({"body_html": "x"})),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn undo_delay_validation() {
    let f = Fixture::new().await;
    let (status, body) = f.call(&f.ann, "GET", "/settings/me", None).await;
    assert_eq!(
        (status, body["undo_send_seconds"].as_i64()),
        (StatusCode::OK, Some(10))
    );
    let (status, _) = f
        .call(
            &f.ann,
            "PUT",
            "/settings/me",
            Some(json!({"undo_send_seconds": 7})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    for seconds in [0, 5, 10, 20, 30] {
        let (status, body) = f
            .call(
                &f.ann,
                "PUT",
                "/settings/me",
                Some(json!({"undo_send_seconds": seconds})),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }
    let (_, body) = f.call(&f.ann, "GET", "/settings/me", None).await;
    assert_eq!(body["undo_send_seconds"], 30);
    // Settings belong to the member.
    let (_, body) = f.call(&f.bob, "GET", "/settings/me", None).await;
    assert_eq!(body["undo_send_seconds"], 10);
}

#[tokio::test]
async fn snippets_are_personal() {
    let f = Fixture::new().await;
    let (status, body) = f
        .call(
            &f.ann,
            "POST",
            "/snippets",
            Some(json!({"name": "Thanks", "body_html": "<p>Thanks!</p><script>x</script>"})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["body_html"], "<p>Thanks!</p>");
    let id = body["id"].as_str().unwrap().to_owned();
    let (status, body) = f
        .call(
            &f.ann,
            "PATCH",
            &format!("/snippets/{id}"),
            Some(json!({"name": "Thank you"})),
        )
        .await;
    assert_eq!(
        (status, body["name"].as_str()),
        (StatusCode::OK, Some("Thank you"))
    );
    assert_eq!(body["body_html"], "<p>Thanks!</p>");
    // Another member sees none and cannot change or delete it.
    let (_, list) = f.call(&f.owner, "GET", "/snippets", None).await;
    assert_eq!(list.as_array().unwrap().len(), 0);
    let (status, _) = f
        .call(
            &f.owner,
            "PATCH",
            &format!("/snippets/{id}"),
            Some(json!({"name": "Mine"})),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = f
        .call(&f.owner, "DELETE", &format!("/snippets/{id}"), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = f
        .call(&f.ann, "POST", "/snippets", Some(json!({"name": ""})))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (_, list) = f.call(&f.ann, "GET", "/snippets", None).await;
    assert_eq!(list[0]["name"], "Thank you");
    let (status, _) = f
        .call(&f.ann, "DELETE", &format!("/snippets/{id}"), None)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}
