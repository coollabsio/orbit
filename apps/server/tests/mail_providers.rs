//! Mail providers: instance and workspace scope, sealed secrets, the instance switch and the
//! domain route (provider intake design §3 P2, §4).

use std::sync::Arc;

use orbit_platform::{
    AttachmentMutationCoordinator, Id, LocalBlobStore, PasswordService, TestDatabase,
    TimestampMillis, UploadLimits, UploadService,
};
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::repositories::instance_settings::InstanceSettingsRepository;
use orbit_server::repositories::mail::{
    MailError, MailRepository, NewProvider, ProviderKind, ProviderScope, ProviderSecrets,
    ProviderSettings,
};
use orbit_server::repositories::workspaces::WorkspaceRepository;

const APP_KEY: [u8; 32] = [9; 32];

struct Fixture {
    _root: tempfile::TempDir,
    database: TestDatabase,
    mail: Arc<MailRepository>,
    settings: InstanceSettingsRepository,
    owner: Id,
    first: Id,
    second: Id,
}

impl Fixture {
    async fn new() -> Self {
        let database = TestDatabase::new().await.unwrap();
        let identity = IdentityRepository::new((*database).clone());
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
        let settings = InstanceSettingsRepository::new((*database).clone());
        Self {
            _root: root,
            database,
            mail,
            settings,
            owner: setup.user_id,
            first: setup.workspace_id,
            second: second.id,
        }
    }

    /// A user who is a plain member of `workspace`.
    async fn member(&self, workspace: Id) -> Id {
        let id = Id::new_v7();
        let now = TimestampMillis::now().as_millis();
        let email = format!("{id}@example.com");
        sqlx::query(
            "INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at) \
             VALUES ($1, $2, $3, 'Member', 'unused', $4, $4)",
        )
        .bind(id.to_string())
        .bind(&email)
        .bind(&email)
        .bind(now)
        .execute(self.database.pool())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at) \
             VALUES ($1, $2, $3, 'member', 0, $4, $4)",
        )
        .bind(Id::new_v7().to_string())
        .bind(workspace.to_string())
        .bind(id.to_string())
        .bind(now)
        .execute(self.database.pool())
        .await
        .unwrap();
        id
    }

    async fn provider(&self, scope: ProviderScope, name: &str) -> Id {
        self.mail
            .create_provider(
                scope,
                self.owner,
                NewProvider {
                    kind: ProviderKind::Mailgun,
                    name: name.to_owned(),
                    secrets: Some(ProviderSecrets::Mailgun {
                        signing_key: "key-signing-secret".to_owned(),
                    }),
                    settings: ProviderSettings::default(),
                },
                "request",
            )
            .await
            .unwrap()
            .provider
            .id
    }

    /// A verified domain of `workspace`, inserted directly (no DKIM key generation).
    async fn domain(&self, workspace: Id, name: &str) -> Id {
        let id = Id::new_v7();
        let now = TimestampMillis::now().as_millis();
        sqlx::query(
            "INSERT INTO mail_domains (id, workspace_id, name, status, verify_token, created_at, updated_at) \
             VALUES ($1, $2, $3, 'verified', 'token', $4, $4)",
        )
        .bind(id.to_string())
        .bind(workspace.to_string())
        .bind(name)
        .bind(now)
        .execute(self.database.pool())
        .await
        .unwrap();
        id
    }
}

#[tokio::test]
async fn instance_and_workspace_providers_are_separate() {
    let fixture = Fixture::new().await;
    let instance = fixture.provider(ProviderScope::Instance, "Shared").await;
    let first = fixture
        .provider(ProviderScope::Workspace(fixture.first), "First")
        .await;
    let second = fixture
        .provider(ProviderScope::Workspace(fixture.second), "Second")
        .await;

    let ids = |providers: Vec<orbit_server::repositories::mail::MailProvider>| {
        let mut ids: Vec<Id> = providers.into_iter().map(|provider| provider.id).collect();
        ids.sort();
        ids
    };
    let mut expected = vec![instance, first];
    expected.sort();
    assert_eq!(
        ids(fixture
            .mail
            .providers_for_workspace(fixture.first, fixture.owner)
            .await
            .unwrap()),
        expected
    );
    assert_eq!(
        ids(fixture
            .mail
            .list_providers(ProviderScope::Workspace(fixture.first), fixture.owner)
            .await
            .unwrap()),
        vec![first]
    );
    assert_eq!(
        ids(fixture
            .mail
            .list_providers(ProviderScope::Instance, fixture.owner)
            .await
            .unwrap()),
        vec![instance]
    );
    // a workspace cannot reach another workspace's provider
    assert!(matches!(
        fixture
            .mail
            .delete_provider(
                ProviderScope::Workspace(fixture.first),
                fixture.owner,
                second,
                "request"
            )
            .await,
        Err(MailError::NotFound)
    ));
}

#[tokio::test]
async fn workspace_provider_needs_the_switch() {
    let fixture = Fixture::new().await;
    let existing = fixture
        .provider(ProviderScope::Workspace(fixture.first), "Before")
        .await;
    fixture
        .settings
        .set_mail_workspace_providers_enabled(
            fixture.owner,
            false,
            "request",
            TimestampMillis::now(),
        )
        .await
        .unwrap();
    assert!(
        !fixture
            .settings
            .mail_workspace_providers_enabled()
            .await
            .unwrap()
    );

    let refused = fixture
        .mail
        .create_provider(
            ProviderScope::Workspace(fixture.first),
            fixture.owner,
            NewProvider {
                kind: ProviderKind::Cloudflare,
                name: "After".to_owned(),
                secrets: None,
                settings: ProviderSettings::default(),
            },
            "request",
        )
        .await;
    assert!(matches!(
        refused,
        Err(MailError::Forbidden("workspace_providers_disabled"))
    ));
    // the instance can still add one, and the existing workspace provider still receives
    fixture.provider(ProviderScope::Instance, "Shared").await;
    assert!(
        fixture
            .mail
            .load_provider(existing)
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn secrets_are_sealed_and_never_listed() {
    let fixture = Fixture::new().await;
    let created = fixture
        .mail
        .create_provider(
            ProviderScope::Instance,
            fixture.owner,
            NewProvider {
                kind: ProviderKind::Postmark,
                name: "Postmark".to_owned(),
                secrets: None,
                settings: ProviderSettings::default(),
            },
            "request",
        )
        .await
        .unwrap();
    // Orbit makes the Postmark credentials and shows them once
    let Some(ProviderSecrets::Postmark {
        basic_user,
        basic_password,
    }) = created.generated.clone()
    else {
        panic!(
            "postmark credentials were not generated: {:?}",
            created.generated
        );
    };
    assert!(basic_password.len() >= 40, "{basic_password}");
    assert_eq!(
        created.provider.secrets_set,
        vec!["basic_user", "basic_password"]
    );
    assert_eq!(
        created.provider.settings.mx_hosts,
        vec!["inbound.postmarkapp.com".to_owned()]
    );

    let stored: Vec<u8> = sqlx::query_scalar("SELECT secrets FROM mail_providers WHERE id = $1")
        .bind(created.provider.id.to_string())
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    let stored = String::from_utf8_lossy(&stored);
    assert!(!stored.contains(&basic_password));
    assert!(!stored.contains(&basic_user));

    let (provider, secrets) = fixture
        .mail
        .load_provider(created.provider.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(provider.kind, ProviderKind::Postmark);
    assert_eq!(Some(secrets), created.generated);
}

#[tokio::test]
async fn provider_in_use_cannot_be_deleted() {
    let fixture = Fixture::new().await;
    let provider = fixture.provider(ProviderScope::Instance, "Shared").await;
    let domain = fixture.domain(fixture.first, "example.com").await;
    fixture
        .mail
        .set_domain_route(
            fixture.first,
            fixture.owner,
            domain,
            Some(provider),
            "request",
        )
        .await
        .unwrap();
    let listed = fixture
        .mail
        .list_providers(ProviderScope::Instance, fixture.owner)
        .await
        .unwrap();
    assert_eq!(listed[0].domain_count, 1);

    assert!(matches!(
        fixture
            .mail
            .delete_provider(ProviderScope::Instance, fixture.owner, provider, "request")
            .await,
        Err(MailError::Conflict("provider_in_use"))
    ));
    fixture
        .mail
        .set_domain_route(fixture.first, fixture.owner, domain, None, "request")
        .await
        .unwrap();
    fixture
        .mail
        .delete_provider(ProviderScope::Instance, fixture.owner, provider, "request")
        .await
        .unwrap();
    assert!(
        fixture
            .mail
            .load_provider(provider)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn domain_cannot_use_another_workspaces_provider() {
    let fixture = Fixture::new().await;
    let foreign = fixture
        .provider(ProviderScope::Workspace(fixture.second), "Second")
        .await;
    let domain = fixture.domain(fixture.first, "example.com").await;

    assert!(matches!(
        fixture
            .mail
            .set_domain_route(
                fixture.first,
                fixture.owner,
                domain,
                Some(foreign),
                "request"
            )
            .await,
        Err(MailError::Invalid {
            field: "provider_id"
        })
    ));
    // the trigger is the backstop below the repository
    let direct = sqlx::query("UPDATE mail_domains SET provider_id = $1 WHERE id = $2")
        .bind(foreign.to_string())
        .bind(domain.to_string())
        .execute(fixture.database.pool())
        .await
        .unwrap_err();
    assert!(
        direct
            .to_string()
            .contains("mail domain must use a provider of its workspace"),
        "{direct}"
    );
}

#[tokio::test]
async fn member_cannot_manage_providers() {
    let fixture = Fixture::new().await;
    let member = fixture.member(fixture.first).await;
    let refused = fixture
        .mail
        .list_providers(ProviderScope::Workspace(fixture.first), member)
        .await;
    assert!(matches!(
        refused,
        Err(MailError::Forbidden("mail_forbidden"))
    ));
    let outsider = fixture.member(fixture.second).await;
    assert!(matches!(
        fixture
            .mail
            .providers_for_workspace(fixture.first, outsider)
            .await,
        Err(MailError::NotFound)
    ));
}

#[tokio::test]
async fn rejected_deliveries_are_kept_for_thirty_days() {
    let fixture = Fixture::new().await;
    let provider = fixture.provider(ProviderScope::Instance, "Shared").await;
    let now = TimestampMillis::now();
    let old = TimestampMillis::from_millis(now.as_millis() - 31 * 24 * 60 * 60 * 1000);
    for at in [old, now] {
        fixture
            .mail
            .record_rejected(
                orbit_server::repositories::mail::NewRejection {
                    workspace_id: Some(fixture.first),
                    provider_id: provider,
                    mail_from: Some("someone@sender.example".to_owned()),
                    recipient: "typo@example.com".to_owned(),
                    reason: orbit_server::repositories::mail::RejectReason::UnknownAddress,
                },
                at,
            )
            .await
            .unwrap();
    }
    assert_eq!(fixture.mail.purge_rejected(now).await.unwrap(), 1);
    let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM mail_rejected_deliveries")
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(left, 1);
}
