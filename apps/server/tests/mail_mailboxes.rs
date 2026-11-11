//! Mailboxes, addresses, members and labels (spec §5, §6.2, §7): address rules, the recipient
//! lookup order, the bulk personal mailboxes, permissions, and a removed owner's mailbox
//! (Review Focus 3).

use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_platform::{
    AttachmentMutationCoordinator, AuthenticatedUser, Id, LocalBlobStore, MailMode,
    PasswordService, TestDatabase, TimestampMillis, UploadLimits, UploadService,
};
use orbit_server::auth_routes::CookieMode;
use orbit_server::mail_routes::{MailSettings, MailState, mail_router};
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::repositories::instance_settings::InstanceSettingsRepository;
use orbit_server::repositories::mail::{
    MailRepository, RecipientLookup, SUSPENDED_GRACE_MS, lookup_recipient,
};
use orbit_server::repositories::workspaces::WorkspaceRepository;
use serde_json::{Value, json};
use tower::ServiceExt;

const APP_KEY: [u8; 32] = [9; 32];
const DAY: i64 = 24 * 60 * 60 * 1000;

#[derive(Clone)]
struct Member {
    id: Id,
    membership_id: Option<Id>,
    cookie: String,
}

struct Fixture {
    _root: tempfile::TempDir,
    database: TestDatabase,
    identity: Arc<IdentityRepository>,
    mail: Arc<MailRepository>,
    app: axum::Router,
    first: Id,
    second: Id,
    owner: Member,
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
        Self {
            _root: root,
            database,
            identity,
            mail,
            app,
            first: setup.workspace_id,
            second: second.id,
            owner: Member {
                id: setup.user_id,
                membership_id: Some(owner_membership.parse().unwrap()),
                cookie: format!("__Host-orbit_session={}", setup.session.token),
            },
        }
    }

    /// A user with a session and `display_name`, a member of `workspace` with `role` when given.
    async fn user(&self, display_name: &str, membership: Option<(Id, &str)>) -> Member {
        let id = Id::new_v7();
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
        let mut membership_id = None;
        if let Some((workspace_id, role)) = membership {
            let member = Id::new_v7();
            sqlx::query(
                "INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at) \
                 VALUES ($1, $2, $3, $4, 0, $5, $6)",
            )
            .bind(member.to_string())
            .bind(workspace_id.to_string())
            .bind(id.to_string())
            .bind(role)
            .bind(now.as_millis())
            .bind(now.as_millis())
            .execute(self.database.pool())
            .await
            .unwrap();
            membership_id = Some(member);
        }
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

    async fn call(
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

    /// The owner's call; `expected` must match.
    async fn ok(
        &self,
        workspace: Id,
        method: &str,
        path: &str,
        body: Option<Value>,
        expected: StatusCode,
    ) -> Value {
        let (status, value) = self.call(&self.owner, workspace, method, path, body).await;
        assert_eq!(status, expected, "{method} {path}: {value}");
        value
    }

    /// The owner's call that fails with `status` and `code`.
    async fn refused(
        &self,
        workspace: Id,
        method: &str,
        path: &str,
        body: Value,
    ) -> (StatusCode, String) {
        let (status, value) = self
            .call(&self.owner, workspace, method, path, Some(body))
            .await;
        (
            status,
            value["code"].as_str().unwrap_or_default().to_owned(),
        )
    }

    /// A domain of `workspace` with `status` (`verified` by default in the tests).
    async fn domain(&self, workspace: Id, name: &str, status: &str) -> Id {
        let domain = self
            .mail
            .create_domain(workspace, self.owner.id, name, "request")
            .await
            .unwrap();
        self.set_domain(domain.id, status, None).await;
        domain.id
    }

    async fn set_domain(&self, domain_id: Id, status: &str, suspended_at: Option<i64>) {
        sqlx::query("UPDATE mail_domains SET status = $1, suspended_at = $2 WHERE id = $3")
            .bind(status)
            .bind(suspended_at)
            .bind(domain_id.to_string())
            .execute(self.database.pool())
            .await
            .unwrap();
    }

    async fn shared(&self, workspace: Id, domain_id: Id, local: &str, members: &[Id]) -> Value {
        self.ok(
            workspace,
            "POST",
            "/mailboxes",
            Some(json!({
                "kind": "shared",
                "name": format!("Box {local}"),
                "domain_id": domain_id.to_string(),
                "local_part": local,
                "member_ids": members.iter().map(ToString::to_string).collect::<Vec<_>>(),
            })),
            StatusCode::CREATED,
        )
        .await
    }

    async fn personal(&self, workspace: Id, domain_id: Id, local: &str, owner: Id) -> Value {
        self.ok(
            workspace,
            "POST",
            "/mailboxes",
            Some(json!({
                "kind": "personal",
                "name": format!("Box {local}"),
                "domain_id": domain_id.to_string(),
                "local_part": local,
                "owner_membership_id": owner.to_string(),
            })),
            StatusCode::CREATED,
        )
        .await
    }

    async fn lookup(&self, rcpt: &str) -> RecipientLookup {
        self.lookup_at(rcpt, TimestampMillis::now()).await
    }

    async fn lookup_at(&self, rcpt: &str, now: TimestampMillis) -> RecipientLookup {
        lookup_recipient(self.database.pool(), rcpt, now)
            .await
            .unwrap()
    }

    async fn count(&self, sql: &str, value: &str) -> i64 {
        sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_owned()))
            .bind(value.to_owned())
            .fetch_one(self.database.pool())
            .await
            .unwrap()
    }
}

fn id(value: &Value) -> Id {
    value["id"].as_str().unwrap().parse().unwrap()
}

fn primary_id(mailbox: &Value) -> Id {
    mailbox["addresses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|address| address["kind"] == "primary")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap()
}

fn accepted(lookup: &RecipientLookup) -> (Id, Id) {
    match lookup {
        RecipientLookup::Accept {
            address_id,
            mailbox_id,
            ..
        } => (*address_id, *mailbox_id),
        other => panic!("expected Accept, got {other:?}"),
    }
}

#[tokio::test]
async fn addresses_are_unique_per_domain_and_primary_stays() {
    let fixture = Fixture::new().await;
    let domain = fixture
        .domain(fixture.first, "example.com", "verified")
        .await;
    let other = fixture
        .domain(fixture.first, "other.test", "verified")
        .await;
    let support = fixture.shared(fixture.first, domain, "Support", &[]).await;
    assert_eq!(support["kind"], "shared");
    assert_eq!(support["status"], "active");
    assert_eq!(support["addresses"][0]["address"], "support@example.com");
    assert_eq!(support["addresses"][0]["kind"], "primary");
    let support_path = format!("/mailboxes/{}", id(&support));

    let body = |local: &str, domain_id: Id| {
        json!({
            "kind": "shared",
            "name": "Again",
            "domain_id": domain_id.to_string(),
            "local_part": local,
            "member_ids": [],
        })
    };
    assert_eq!(
        fixture
            .refused(fixture.first, "POST", "/mailboxes", body("support", domain))
            .await,
        (StatusCode::CONFLICT, "address_taken".to_owned())
    );
    for bad in ["ann+news", "a b", "", "@x", ".ann", "ann%x"] {
        assert_eq!(
            fixture
                .refused(fixture.first, "POST", "/mailboxes", body(bad, domain))
                .await,
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                "validation_failed".to_owned()
            ),
            "{bad:?}"
        );
    }
    // A domain of another workspace is not usable.
    let foreign = fixture
        .domain(fixture.second, "foreign.test", "verified")
        .await;
    assert_eq!(
        fixture
            .refused(fixture.first, "POST", "/mailboxes", body("x", foreign))
            .await,
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation_failed".to_owned()
        )
    );

    let sales = fixture.shared(fixture.first, domain, "sales", &[]).await;
    let sales_aliases = format!("/mailboxes/{}/addresses", id(&sales));
    assert_eq!(
        fixture
            .refused(
                fixture.first,
                "POST",
                &sales_aliases,
                json!({ "domain_id": domain.to_string(), "local_part": "SUPPORT" }),
            )
            .await,
        (StatusCode::CONFLICT, "address_taken".to_owned())
    );
    // The same local part on another domain is a different address.
    let updated = fixture
        .ok(
            fixture.first,
            "POST",
            &sales_aliases,
            Some(json!({ "domain_id": other.to_string(), "local_part": "Support" })),
            StatusCode::CREATED,
        )
        .await;
    let alias = updated["addresses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|address| address["kind"] == "alias")
        .unwrap()
        .clone();
    assert_eq!(alias["address"], "support@other.test");

    // The primary address is not removed; an alias is.
    let (status, value) = fixture
        .call(
            &fixture.owner,
            fixture.first,
            "DELETE",
            &format!("{support_path}/addresses/{}", primary_id(&support)),
            None,
        )
        .await;
    assert_eq!(
        (status, value["code"].as_str()),
        (StatusCode::CONFLICT, Some("address_primary"))
    );
    let after = fixture
        .ok(
            fixture.first,
            "DELETE",
            &format!("{sales_aliases}/{}", alias["id"].as_str().unwrap()),
            None,
            StatusCode::OK,
        )
        .await;
    assert_eq!(after["addresses"].as_array().unwrap().len(), 1);
    // An address of another mailbox is not removed through this one.
    let (status, _) = fixture
        .call(
            &fixture.owner,
            fixture.first,
            "DELETE",
            &format!("{sales_aliases}/{}", primary_id(&support)),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let renamed = fixture
        .ok(
            fixture.first,
            "PATCH",
            &support_path,
            Some(json!({ "name": "Customer support" })),
            StatusCode::OK,
        )
        .await;
    assert_eq!(renamed["name"], "Customer support");

    assert_eq!(
        fixture
            .count(
                "SELECT COUNT(*) FROM audit_events WHERE workspace_id = $1 AND action IN \
                 ('mail.mailbox_created', 'mail.address_added', 'mail.address_removed', 'mail.mailbox_updated')",
                &fixture.first.to_string(),
            )
            .await,
        5
    );
}

#[tokio::test]
async fn recipients_are_looked_up_in_the_spec_order() {
    let fixture = Fixture::new().await;
    let domain = fixture
        .domain(fixture.first, "example.com", "verified")
        .await;
    let ann = fixture.user("Ann", Some((fixture.first, "member"))).await;
    let ann_box = fixture
        .personal(fixture.first, domain, "ann", ann.membership_id.unwrap())
        .await;
    let ann_address = primary_id(&ann_box);

    // `+tag` and case.
    for rcpt in [
        "ann@example.com",
        "ann+news@example.com",
        "ANN@Example.COM.",
    ] {
        assert_eq!(
            accepted(&fixture.lookup(rcpt).await),
            (ann_address, id(&ann_box)),
            "{rcpt}"
        );
    }
    match fixture.lookup("ann@example.com").await {
        RecipientLookup::Accept { workspace_id, .. } => assert_eq!(workspace_id, fixture.first),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        fixture.lookup("bob@example.com").await,
        RecipientLookup::UnknownUser
    );
    assert_eq!(
        fixture.lookup("ann@elsewhere.test").await,
        RecipientLookup::NotLocal
    );
    for bad in [
        "",
        "ann",
        "a@b@example.com",
        "@relay:ann@example.com",
        "\"a\"@example.com",
    ] {
        assert_eq!(
            fixture.lookup(bad).await,
            RecipientLookup::Invalid,
            "{bad:?}"
        );
    }
    // postmaster@ is routed like any address: refused until an admin adds it.
    assert_eq!(
        fixture.lookup("postmaster@example.com").await,
        RecipientLookup::UnknownUser
    );

    // Catch-all takes unknown local parts; an alias is still exact.
    let catch = fixture.shared(fixture.first, domain, "inbox", &[]).await;
    fixture
        .ok(
            fixture.first,
            "PUT",
            &format!("/domains/{domain}/catch-all"),
            Some(json!({ "mailbox_id": id(&catch).to_string() })),
            StatusCode::OK,
        )
        .await;
    assert_eq!(
        accepted(&fixture.lookup("bob+x@example.com").await),
        (primary_id(&catch), id(&catch))
    );
    assert_eq!(
        accepted(&fixture.lookup("postmaster@example.com").await).1,
        id(&catch)
    );
    assert_eq!(
        accepted(&fixture.lookup("ann@example.com").await).1,
        id(&ann_box)
    );

    // An active test address comes first; an expired one is an ordinary address.
    let now = TimestampMillis::now().as_millis();
    let test_id = Id::new_v7();
    for (test, local, expires_at) in [
        (test_id, "check-abc", now + 3_600_000),
        (Id::new_v7(), "check-old", now - 1),
    ] {
        sqlx::query(
            "INSERT INTO mail_health_tests (id, domain_id, address_local, expires_at, created_at) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(test.to_string())
        .bind(domain.to_string())
        .bind(local)
        .bind(expires_at)
        .bind(now)
        .execute(fixture.database.pool())
        .await
        .unwrap();
    }
    assert_eq!(
        fixture.lookup("Check-ABC@example.com").await,
        RecipientLookup::HealthTest { test_id }
    );
    assert_eq!(
        accepted(&fixture.lookup("check-old@example.com").await).1,
        id(&catch)
    );

    // A suspended mailbox refuses, also through the catch-all.
    fixture
        .ok(
            fixture.first,
            "PUT",
            &format!("/mailboxes/{}/status", id(&catch)),
            Some(json!({ "status": "suspended" })),
            StatusCode::OK,
        )
        .await;
    assert_eq!(
        fixture.lookup("bob@example.com").await,
        RecipientLookup::MailboxSuspended
    );
    assert_eq!(
        fixture.lookup("check-abc@example.com").await,
        RecipientLookup::HealthTest { test_id }
    );

    // Domain status: pending is not local; suspended receives within the grace period only;
    // disabled refuses.
    fixture.set_domain(domain, "pending", None).await;
    assert_eq!(
        fixture.lookup("ann@example.com").await,
        RecipientLookup::NotLocal
    );
    let suspended_at = now - DAY;
    fixture
        .set_domain(domain, "suspended", Some(suspended_at))
        .await;
    accepted(&fixture.lookup("ann@example.com").await);
    let past_grace = TimestampMillis::from_millis(suspended_at + SUSPENDED_GRACE_MS + 1);
    assert_eq!(
        fixture.lookup_at("ann@example.com", past_grace).await,
        RecipientLookup::DomainUnavailable
    );
    assert_eq!(
        fixture.lookup_at("check-abc@example.com", past_grace).await,
        RecipientLookup::DomainUnavailable
    );
    fixture.set_domain(domain, "disabled", None).await;
    assert_eq!(
        fixture.lookup("ann@example.com").await,
        RecipientLookup::DomainUnavailable
    );

    // The repository wrapper gives the same answer.
    fixture.set_domain(domain, "verified", None).await;
    assert_eq!(
        accepted(
            &fixture
                .mail
                .lookup_recipient("ann@example.com", TimestampMillis::now())
                .await
                .unwrap()
        )
        .1,
        id(&ann_box)
    );
}

#[tokio::test]
async fn bulk_personal_mailboxes_fold_names_and_mark_conflicts() {
    let fixture = Fixture::new().await;
    let domain = fixture
        .domain(fixture.first, "example.com", "verified")
        .await;
    let jose = fixture
        .user("José Núñez", Some((fixture.first, "member")))
        .await;
    let ann = fixture
        .user("Ann Lee", Some((fixture.first, "member")))
        .await;
    let ann2 = fixture
        .user("Ann Lewis", Some((fixture.first, "admin")))
        .await;
    // Not in this workspace.
    fixture
        .user("Zed Other", Some((fixture.second, "member")))
        .await;
    // `owner` (display name "Owner") is taken already by a shared mailbox.
    fixture.shared(fixture.first, domain, "owner", &[]).await;

    let preview = |pattern: &'static str| {
        let fixture = &fixture;
        async move {
            let value = fixture
                .ok(
                    fixture.first,
                    "POST",
                    "/mailboxes/bulk-personal/preview",
                    Some(json!({ "domain_id": domain.to_string(), "pattern": pattern })),
                    StatusCode::OK,
                )
                .await;
            value
                .as_array()
                .unwrap()
                .iter()
                .map(|proposal| {
                    (
                        proposal["membership_id"].as_str().unwrap().to_owned(),
                        proposal["local_part"].as_str().unwrap().to_owned(),
                        proposal["conflict"].as_bool().unwrap(),
                    )
                })
                .collect::<Vec<_>>()
        }
    };
    let find = |proposals: &[(String, String, bool)], member: &Member| {
        proposals
            .iter()
            .find(|(membership, _, _)| *membership == member.membership_id.unwrap().to_string())
            .map(|(_, local, conflict)| (local.clone(), *conflict))
            .unwrap()
    };

    let dotted = preview("first_dot_last").await;
    assert_eq!(dotted.len(), 4, "{dotted:?}");
    assert_eq!(find(&dotted, &jose), ("jose.nunez".to_owned(), false));
    assert_eq!(find(&dotted, &ann), ("ann.lee".to_owned(), false));
    assert_eq!(find(&dotted, &ann2), ("ann.lewis".to_owned(), false));
    assert_eq!(find(&dotted, &fixture.owner), ("owner".to_owned(), true));

    let first = preview("first").await;
    assert_eq!(find(&first, &jose), ("jose".to_owned(), false));
    let anns = [find(&first, &ann), find(&first, &ann2)];
    assert!(anns.iter().all(|(local, _)| local == "ann"));
    // One of the two `ann` proposals is a conflict.
    assert_eq!(anns.iter().filter(|(_, conflict)| *conflict).count(), 1);

    let flast = preview("flast").await;
    assert_eq!(find(&flast, &jose), ("jnunez".to_owned(), false));

    // Create with `first`: the admin renames the second Ann and the owner.
    let later_ann = if anns[0].1 { &ann } else { &ann2 };
    let result = fixture
        .ok(
            fixture.first,
            "POST",
            "/mailboxes/bulk-personal",
            Some(json!({
                "domain_id": domain.to_string(),
                "pattern": "first",
                "overrides": [
                    { "membership_id": later_ann.membership_id.unwrap().to_string(), "local_part": "Ann.L" },
                    { "membership_id": fixture.owner.membership_id.unwrap().to_string(), "local_part": "sales" },
                    { "membership_id": jose.membership_id.unwrap().to_string(), "local_part": "bad local" },
                ],
            })),
            StatusCode::OK,
        )
        .await;
    let created = result["created"].as_array().unwrap();
    let mut addresses = created
        .iter()
        .map(|mailbox| {
            assert_eq!(mailbox["kind"], "personal");
            assert_eq!(mailbox["member_ids"].as_array().unwrap().len(), 1);
            mailbox["addresses"][0]["address"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect::<Vec<_>>();
    addresses.sort();
    assert_eq!(
        addresses,
        ["ann.l@example.com", "ann@example.com", "sales@example.com"]
    );
    let conflicts = result["conflicts"].as_array().unwrap();
    assert_eq!(conflicts.len(), 1, "{conflicts:?}");
    assert_eq!(
        conflicts[0]["membership_id"],
        jose.membership_id.unwrap().to_string()
    );
    assert_eq!(conflicts[0]["conflict"], true);
    let jose_box = created
        .iter()
        .find(|mailbox| mailbox["owner_membership_id"] == jose.membership_id.unwrap().to_string());
    assert!(jose_box.is_none());

    // Members with a personal mailbox are left out of the next preview.
    let again = preview("first_dot_last").await;
    assert_eq!(again.len(), 1);
    assert_eq!(find(&again, &jose), ("jose.nunez".to_owned(), false));
}

#[tokio::test]
async fn a_removed_owner_leaves_a_suspended_mailbox_for_the_admins() {
    let fixture = Fixture::new().await;
    let domain = fixture
        .domain(fixture.first, "example.com", "verified")
        .await;
    let ann = fixture.user("Ann", Some((fixture.first, "member"))).await;
    let bob = fixture.user("Bob", Some((fixture.first, "member"))).await;
    let admin = fixture.user("Admin", Some((fixture.first, "admin"))).await;
    let ann_membership = ann.membership_id.unwrap();
    let ann_box = fixture
        .personal(fixture.first, domain, "ann", ann_membership)
        .await;
    let ann_path = format!("/mailboxes/{}", id(&ann_box));
    let (status, mine) = fixture
        .call(&ann, fixture.first, "GET", "/mailboxes", None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(mine.as_array().unwrap().len(), 1);
    let (status, _) = fixture
        .call(&ann, fixture.first, "GET", &ann_path, None)
        .await;
    assert_eq!(status, StatusCode::OK);
    accepted(&fixture.lookup("ann@example.com").await);

    let version: i64 = sqlx::query_scalar("SELECT version FROM memberships WHERE id = $1")
        .bind(ann_membership.to_string())
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    WorkspaceRepository::new((*fixture.database).clone())
        .remove_member(
            fixture.first,
            ann_membership,
            admin.id,
            u64::try_from(version).unwrap(),
            "request",
            TimestampMillis::now(),
        )
        .await
        .unwrap();

    // Mail is refused (550 5.2.1) and nobody reads the mailbox.
    assert_eq!(
        fixture.lookup("ann@example.com").await,
        RecipientLookup::MailboxSuspended
    );
    assert_eq!(
        fixture
            .count(
                "SELECT COUNT(*) FROM mailbox_members WHERE mailbox_id = $1",
                &id(&ann_box).to_string(),
            )
            .await,
        0
    );
    for member in [&ann, &bob] {
        let (status, _) = fixture
            .call(member, fixture.first, "GET", &ann_path, None)
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
    let (status, listed) = fixture
        .call(&bob, fixture.first, "GET", "/mailboxes", None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(listed.as_array().unwrap().is_empty());

    // The admins' management view still has it, suspended and without an owner.
    let (status, managed) = fixture
        .call(&admin, fixture.first, "GET", &ann_path, None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(managed["status"], "suspended");
    assert_eq!(managed["owner_membership_id"], Value::Null);
    assert!(managed["member_ids"].as_array().unwrap().is_empty());
    // It cannot be resumed or handed to someone else.
    assert_eq!(
        fixture
            .refused(
                fixture.first,
                "PUT",
                &format!("{ann_path}/status"),
                json!({ "status": "active" }),
            )
            .await,
        (StatusCode::CONFLICT, "mailbox_owner_left".to_owned())
    );
    assert_eq!(
        fixture
            .refused(
                fixture.first,
                "POST",
                &format!("{ann_path}/members"),
                json!({ "membership_id": bob.membership_id.unwrap().to_string() }),
            )
            .await,
        (StatusCode::CONFLICT, "mailbox_personal".to_owned())
    );
    assert_eq!(
        fixture
            .count(
                "SELECT COUNT(*) FROM audit_events WHERE action = 'mail.mailbox_suspended' AND resource_id = $1",
                &id(&ann_box).to_string(),
            )
            .await,
        1
    );
    // The admins can delete it.
    fixture
        .ok(
            fixture.first,
            "DELETE",
            &ann_path,
            None,
            StatusCode::NO_CONTENT,
        )
        .await;
    assert_eq!(
        fixture.lookup("ann@example.com").await,
        RecipientLookup::UnknownUser
    );
}

#[tokio::test]
async fn personal_and_shared_mailbox_member_rules() {
    let fixture = Fixture::new().await;
    let domain = fixture
        .domain(fixture.first, "example.com", "pending")
        .await;
    let ann = fixture.user("Ann", Some((fixture.first, "member"))).await;
    let bob = fixture.user("Bob", Some((fixture.first, "member"))).await;
    let stranger = fixture
        .user("Stranger", Some((fixture.second, "member")))
        .await;
    let ann_m = ann.membership_id.unwrap().to_string();
    let bob_m = bob.membership_id.unwrap().to_string();

    let base = |kind: &str, local: &str| {
        json!({
            "kind": kind,
            "name": "Box",
            "domain_id": domain.to_string(),
            "local_part": local,
        })
    };
    let with = |mut body: Value, key: &str, value: Value| {
        body[key] = value;
        body
    };
    // Personal: an owner of the workspace, no other members.
    for body in [
        base("personal", "p1"),
        with(
            base("personal", "p1"),
            "owner_membership_id",
            json!(stranger.membership_id.unwrap().to_string()),
        ),
        with(
            with(base("personal", "p1"), "owner_membership_id", json!(ann_m)),
            "member_ids",
            json!([bob_m]),
        ),
        with(base("shared", "s1"), "owner_membership_id", json!(ann_m)),
        with(
            base("shared", "s1"),
            "member_ids",
            json!([stranger.membership_id.unwrap().to_string()]),
        ),
        with(base("shared", "s1"), "name", json!("  ")),
    ] {
        assert_eq!(
            fixture
                .refused(fixture.first, "POST", "/mailboxes", body.clone())
                .await,
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                "validation_failed".to_owned()
            ),
            "{body}"
        );
    }
    let personal = fixture
        .personal(fixture.first, domain, "ann", ann.membership_id.unwrap())
        .await;
    assert_eq!(personal["member_ids"], json!([ann_m]));
    assert_eq!(personal["owner_membership_id"], json!(ann_m));
    // One personal mailbox for each member.
    assert_eq!(
        fixture
            .refused(
                fixture.first,
                "POST",
                "/mailboxes",
                with(
                    base("personal", "ann2"),
                    "owner_membership_id",
                    json!(ann_m)
                ),
            )
            .await,
        (StatusCode::CONFLICT, "member_has_mailbox".to_owned())
    );
    let personal_path = format!("/mailboxes/{}/members", id(&personal));
    assert_eq!(
        fixture
            .refused(
                fixture.first,
                "POST",
                &personal_path,
                json!({ "membership_id": bob_m }),
            )
            .await,
        (StatusCode::CONFLICT, "mailbox_personal".to_owned())
    );
    let (status, value) = fixture
        .call(
            &fixture.owner,
            fixture.first,
            "DELETE",
            &format!("{personal_path}/{ann_m}"),
            None,
        )
        .await;
    assert_eq!(
        (status, value["code"].as_str()),
        (StatusCode::CONFLICT, Some("mailbox_personal"))
    );

    // Shared: members come and go; Bob reads it only while a member.
    let shared = fixture
        .shared(
            fixture.first,
            domain,
            "team",
            &[ann.membership_id.unwrap(), ann.membership_id.unwrap()],
        )
        .await;
    assert_eq!(shared["member_ids"], json!([ann_m]));
    let members = format!("/mailboxes/{}/members", id(&shared));
    let (status, _) = fixture
        .call(
            &bob,
            fixture.first,
            "GET",
            &format!("/mailboxes/{}", id(&shared)),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let added = fixture
        .ok(
            fixture.first,
            "POST",
            &members,
            Some(json!({ "membership_id": bob_m })),
            StatusCode::OK,
        )
        .await;
    assert_eq!(added["member_ids"].as_array().unwrap().len(), 2);
    let (status, listed) = fixture
        .call(&bob, fixture.first, "GET", "/mailboxes", None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed.as_array().unwrap().len(), 1);
    assert_eq!(
        fixture
            .refused(
                fixture.first,
                "POST",
                &members,
                json!({ "membership_id": stranger.membership_id.unwrap().to_string() }),
            )
            .await,
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation_failed".to_owned()
        )
    );
    let removed = fixture
        .ok(
            fixture.first,
            "DELETE",
            &format!("{members}/{bob_m}"),
            None,
            StatusCode::OK,
        )
        .await;
    assert_eq!(removed["member_ids"], json!([ann_m]));
    let (_, listed) = fixture
        .call(&bob, fixture.first, "GET", "/mailboxes", None)
        .await;
    assert!(listed.as_array().unwrap().is_empty());

    // The owner sees every mailbox of the workspace, none of another.
    let all = fixture
        .ok(fixture.first, "GET", "/mailboxes", None, StatusCode::OK)
        .await;
    assert_eq!(all.as_array().unwrap().len(), 2);
    let (status, _) = fixture
        .call(
            &fixture.owner,
            fixture.second,
            "GET",
            &format!("/mailboxes/{}", id(&shared)),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Deleting a mailbox frees its addresses and clears a catch-all.
    fixture
        .ok(
            fixture.first,
            "PUT",
            &format!("/domains/{domain}/catch-all"),
            Some(json!({ "mailbox_id": id(&shared).to_string() })),
            StatusCode::OK,
        )
        .await;
    fixture
        .ok(
            fixture.first,
            "DELETE",
            &format!("/mailboxes/{}", id(&shared)),
            None,
            StatusCode::NO_CONTENT,
        )
        .await;
    let domain_view = fixture
        .ok(
            fixture.first,
            "GET",
            &format!("/domains/{domain}"),
            None,
            StatusCode::OK,
        )
        .await;
    assert_eq!(domain_view["catch_all_mailbox_id"], Value::Null);
    fixture.shared(fixture.first, domain, "team", &[]).await;
}

#[tokio::test]
async fn only_owners_and_admins_manage_mailboxes_and_labels() {
    let fixture = Fixture::new().await;
    let domain = fixture
        .domain(fixture.first, "example.com", "verified")
        .await;
    let member = fixture
        .user("Member", Some((fixture.first, "member")))
        .await;
    let admin = fixture.user("Admin", Some((fixture.first, "admin"))).await;
    let outsider = fixture.user("Outsider", None).await;
    let shared = fixture
        .shared(
            fixture.first,
            domain,
            "team",
            &[member.membership_id.unwrap()],
        )
        .await;
    let label = fixture
        .ok(
            fixture.first,
            "POST",
            "/labels",
            Some(json!({ "name": "Work", "color": "#AA0011" })),
            StatusCode::CREATED,
        )
        .await;
    let path = format!("/mailboxes/{}", id(&shared));
    let label_path = format!("/labels/{}", id(&label));
    let member_m = member.membership_id.unwrap().to_string();

    let requests = [
        (
            "POST",
            "/mailboxes".to_owned(),
            Some(json!({
                "kind": "shared", "name": "X", "domain_id": domain.to_string(),
                "local_part": "x", "member_ids": [],
            })),
        ),
        ("PATCH", path.clone(), Some(json!({ "name": "Y" }))),
        ("DELETE", path.clone(), None),
        (
            "PUT",
            format!("{path}/status"),
            Some(json!({ "status": "suspended" })),
        ),
        (
            "POST",
            format!("{path}/members"),
            Some(json!({ "membership_id": member_m })),
        ),
        ("DELETE", format!("{path}/members/{member_m}"), None),
        (
            "POST",
            format!("{path}/addresses"),
            Some(json!({ "domain_id": domain.to_string(), "local_part": "y" })),
        ),
        (
            "DELETE",
            format!("{path}/addresses/{}", primary_id(&shared)),
            None,
        ),
        (
            "POST",
            "/mailboxes/bulk-personal/preview".to_owned(),
            Some(json!({ "domain_id": domain.to_string(), "pattern": "first" })),
        ),
        (
            "POST",
            "/mailboxes/bulk-personal".to_owned(),
            Some(json!({ "domain_id": domain.to_string(), "pattern": "first", "overrides": [] })),
        ),
        (
            "POST",
            "/labels".to_owned(),
            Some(json!({ "name": "Other", "color": "#000000" })),
        ),
        ("PATCH", label_path.clone(), Some(json!({ "name": "Z" }))),
        ("DELETE", label_path.clone(), None),
    ];
    for (method, path, body) in &requests {
        let (status, value) = fixture
            .call(&member, fixture.first, method, path, body.clone())
            .await;
        assert_eq!(
            (status, value["code"].as_str()),
            (StatusCode::FORBIDDEN, Some("mail_forbidden")),
            "{method} {path}"
        );
        let (status, value) = fixture
            .call(&outsider, fixture.first, method, path, body.clone())
            .await;
        assert_eq!(
            (status, value["code"].as_str()),
            (StatusCode::NOT_FOUND, Some("mail_not_found")),
            "{method} {path}"
        );
    }
    for get in ["/mailboxes", "/labels", path.as_str()] {
        let (status, _) = fixture
            .call(&outsider, fixture.first, "GET", get, None)
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{get}");
        let (status, _) = fixture.call(&member, fixture.first, "GET", get, None).await;
        assert_eq!(status, StatusCode::OK, "{get}");
    }
    // An admin manages.
    let (status, value) = fixture
        .call(
            &admin,
            fixture.first,
            "PATCH",
            &path,
            Some(json!({ "name": "Team" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{value}");
}

#[tokio::test]
async fn labels_have_unique_names_per_workspace() {
    let fixture = Fixture::new().await;
    let work = fixture
        .ok(
            fixture.first,
            "POST",
            "/labels",
            Some(json!({ "name": " Work ", "color": "#AA0011" })),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(work["name"], "Work");
    assert_eq!(work["color"], "#aa0011");
    assert_eq!(
        fixture
            .refused(
                fixture.first,
                "POST",
                "/labels",
                json!({ "name": "WORK", "color": "#000000" }),
            )
            .await,
        (StatusCode::CONFLICT, "label_exists".to_owned())
    );
    fixture
        .ok(
            fixture.second,
            "POST",
            "/labels",
            Some(json!({ "name": "work", "color": "#000000" })),
            StatusCode::CREATED,
        )
        .await;
    for body in [
        json!({ "name": "X", "color": "red" }),
        json!({ "name": "", "color": "#000000" }),
        json!({ "name": "x".repeat(81), "color": "#000000" }),
    ] {
        assert_eq!(
            fixture
                .refused(fixture.first, "POST", "/labels", body.clone())
                .await,
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                "validation_failed".to_owned()
            ),
            "{body}"
        );
    }
    let home = fixture
        .ok(
            fixture.first,
            "POST",
            "/labels",
            Some(json!({ "name": "Home", "color": "#00ff00" })),
            StatusCode::CREATED,
        )
        .await;
    let home_path = format!("/labels/{}", id(&home));
    assert_eq!(
        fixture
            .refused(
                fixture.first,
                "PATCH",
                &home_path,
                json!({ "name": "work" })
            )
            .await,
        (StatusCode::CONFLICT, "label_exists".to_owned())
    );
    // Changing only the case of its own name is fine.
    let renamed = fixture
        .ok(
            fixture.first,
            "PATCH",
            &home_path,
            Some(json!({ "name": "HOME", "color": "#0000FF" })),
            StatusCode::OK,
        )
        .await;
    assert_eq!(
        (renamed["name"].as_str(), renamed["color"].as_str()),
        (Some("HOME"), Some("#0000ff"))
    );
    let listed = fixture
        .ok(fixture.first, "GET", "/labels", None, StatusCode::OK)
        .await;
    let names = listed
        .as_array()
        .unwrap()
        .iter()
        .map(|label| label["name"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(names, ["HOME", "Work"]);
    fixture
        .ok(
            fixture.first,
            "DELETE",
            &home_path,
            None,
            StatusCode::NO_CONTENT,
        )
        .await;
    let (status, _) = fixture
        .call(&fixture.owner, fixture.first, "DELETE", &home_path, None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // Labels of another workspace are not found from this one.
    let (status, _) = fixture
        .call(
            &fixture.owner,
            fixture.second,
            "PATCH",
            &format!("/labels/{}", id(&work)),
            Some(json!({ "name": "Taken" })),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        fixture
            .count(
                "SELECT COUNT(*) FROM audit_events WHERE workspace_id = $1 AND action LIKE 'mail.label_%'",
                &fixture.first.to_string(),
            )
            .await,
        4
    );
}
