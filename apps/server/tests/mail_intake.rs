//! Intake (spec §4.3, §6, §9): a received message is sealed, stored as a blob and recorded as a
//! receipt with its ingest job in one transaction before the `250` reply; any failure stores
//! nothing for any workspace. Health test messages only record their result.

use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use orbit_mail::{AuthOutcome, DmarcPolicy, DmarcVerdict, TlsInfo, Verdict};
use orbit_platform::{
    AttachmentMutationCoordinator, BlobFuture, BlobObject, BlobReader, BlobStore, BlobStoreError,
    Id, LocalBlobStore, StagedUpload, StoredObject, TestDatabase, TimestampMillis, UploadLimits,
    UploadService,
};
use orbit_server::mail_intake::{
    InboundMessage, IntakeError, MailIntake, Recipient, Source, mail_ingest_kind,
};
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::repositories::mail::{MailRepository, open_blob, receipt_aad};
use orbit_server::repositories::workspaces::WorkspaceRepository;
use serde_json::Value;

const APP_KEY: [u8; 32] = [7; 32];
const RAW: &[u8] =
    b"From: Ann <ann@sender.test>\r\nTo: box@one.test\r\nSubject: Hello\r\n\r\nHi there\r\n";

/// The local store, or an error from `install` while `fail` is set.
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
                    path: "blobs".into(),
                    source: std::io::Error::other("disk full"),
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

struct Fixture {
    _root: tempfile::TempDir,
    database: TestDatabase,
    mail: Arc<MailRepository>,
    intake: MailIntake,
    fail: Arc<AtomicBool>,
    first: Id,
    second: Id,
}

impl Fixture {
    async fn new() -> Self {
        Self::with_limit(52_428_800).await
    }

    async fn with_limit(max_message_bytes: u64) -> Self {
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
                    password_hash: "unused".to_owned(),
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
        let intake = MailIntake::new(Arc::clone(&mail), max_message_bytes);
        Self {
            _root: root,
            database,
            mail,
            intake,
            fail,
            first: setup.workspace_id,
            second: second.id,
        }
    }

    async fn execute(&self, sql: &str, binds: &[&str]) {
        let mut query = sqlx::query(sqlx::AssertSqlSafe(sql.to_owned()));
        for bind in binds {
            query = query.bind(*bind);
        }
        query.execute(self.database.pool()).await.unwrap();
    }

    async fn count(&self, sql: &str, bind: &str) -> i64 {
        sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_owned()))
            .bind(bind)
            .fetch_one(self.database.pool())
            .await
            .unwrap()
    }

    /// A verified domain with a shared mailbox and its primary address `local@domain`; returns
    /// (domain id, address id).
    async fn address(&self, workspace: Id, domain: &str, local: &str) -> (Id, Id) {
        let now = TimestampMillis::now().as_millis().to_string();
        let domain_id = Id::new_v7();
        let mailbox_id = Id::new_v7();
        let address_id = Id::new_v7();
        self.execute(
            &format!(
                "INSERT INTO mail_domains (id, workspace_id, name, status, verify_token, created_at, updated_at) \
                 VALUES ($1, $2, $3, 'verified', 'token', {now}, {now})"
            ),
            &[&domain_id.to_string(), &workspace.to_string(), domain],
        )
        .await;
        self.execute(
            &format!(
                "INSERT INTO mailboxes (id, workspace_id, kind, name, owner_membership_id, status, created_at, updated_at) \
                 VALUES ($1, $2, 'shared', 'Box', NULL, 'active', {now}, {now})"
            ),
            &[&mailbox_id.to_string(), &workspace.to_string()],
        )
        .await;
        self.execute(
            &format!(
                "INSERT INTO mail_addresses (id, domain_id, mailbox_id, local_part, kind, created_at) \
                 VALUES ($1, $2, $3, $4, 'primary', {now})"
            ),
            &[
                &address_id.to_string(),
                &domain_id.to_string(),
                &mailbox_id.to_string(),
                local,
            ],
        )
        .await;
        (domain_id, address_id)
    }

    async fn receipts(&self, workspace: Id) -> i64 {
        self.count(
            "SELECT COUNT(*) FROM mail_receipts WHERE workspace_id = $1",
            &workspace.to_string(),
        )
        .await
    }

    async fn blobs(&self, workspace: Id) -> i64 {
        self.count(
            "SELECT COUNT(*) FROM attachment_blobs WHERE workspace_id = $1",
            &workspace.to_string(),
        )
        .await
    }

    async fn jobs(&self) -> i64 {
        self.count("SELECT COUNT(*) FROM jobs WHERE kind = $1", "mail.ingest")
            .await
    }
}

fn auth() -> AuthOutcome {
    AuthOutcome {
        spf: Verdict::Pass,
        dkim: Verdict::None,
        dmarc: DmarcVerdict::Fail {
            policy: DmarcPolicy::Reject,
        },
        aligned_domain: None,
        temp_error: false,
        spf_domain: Some("sender.test".to_owned()),
        dkim_domain: None,
        from_domain: Some("sender.test".to_owned()),
    }
}

fn message(recipients: Vec<Recipient>) -> InboundMessage {
    InboundMessage {
        source: Source::Smtp,
        trace_len: 0,
        raw: RAW.to_vec(),
        mail_from: Some("ann@sender.test".to_owned()),
        recipients,
        peer_ip: IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)),
        helo: "mx.sender.test".to_owned(),
        tls: Some(TlsInfo {
            version: "TLSv1.3".to_owned(),
            cipher: "TLS13_AES_128_GCM_SHA256".to_owned(),
        }),
        auth: auth(),
    }
}

fn to(workspace_id: Id, address_id: Id, rcpt: &str) -> Recipient {
    Recipient::Mailbox {
        workspace_id,
        address_id,
        rcpt: rcpt.to_owned(),
    }
}

#[test]
fn ingest_kind_uses_the_planned_limits() {
    let kind = mail_ingest_kind();
    assert_eq!(kind.name(), "mail.ingest");
    let limits = kind.default_limits();
    assert_eq!(limits.concurrency, Some(4));
    assert_eq!(limits.workspace_concurrency, Some(2));
}

#[tokio::test]
async fn accept_stores_a_sealed_receipt_and_enqueues_ingest() {
    let fixture = Fixture::new().await;
    let (_, address) = fixture.address(fixture.first, "one.test", "box").await;
    let (_, alias) = fixture.address(fixture.first, "two.test", "team").await;

    let ids = fixture
        .intake
        .accept(message(vec![
            to(fixture.first, address, "box+news@one.test"),
            to(fixture.first, alias, "team@two.test"),
        ]))
        .await
        .unwrap();
    assert_eq!(ids.len(), 1, "one receipt for one workspace");
    let receipt_id = ids[0];

    let row = sqlx::query_as::<_, (String, String, i64, String, i64, Option<String>, String, String, Option<String>, String, String)>(
        "SELECT workspace_id, blob_id, key_version, raw_sha256, size, mail_from, peer_ip, helo, tls, auth, status \
         FROM mail_receipts WHERE id = $1",
    )
    .bind(receipt_id.to_string())
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    let (
        workspace_id,
        blob_id,
        key_version,
        raw_sha256,
        size,
        mail_from,
        peer_ip,
        helo,
        tls,
        auth,
        status,
    ) = row;
    assert_eq!(workspace_id, fixture.first.to_string());
    assert_eq!(key_version, 1);
    use sha2::Digest;
    assert_eq!(raw_sha256, format!("{:x}", sha2::Sha256::digest(RAW)));
    assert_eq!(size, i64::try_from(RAW.len()).unwrap());
    assert_eq!(mail_from.as_deref(), Some("ann@sender.test"));
    assert_eq!(peer_ip, "192.0.2.1");
    assert_eq!(helo, "mx.sender.test");
    assert_eq!(status, "pending");
    let tls: Value = serde_json::from_str(&tls.unwrap()).unwrap();
    assert_eq!(tls["version"], "TLSv1.3");
    let auth: Value = serde_json::from_str(&auth).unwrap();
    assert_eq!(auth["spf"], "pass");
    assert_eq!(auth["dmarc"]["result"], "fail");
    assert_eq!(auth["dmarc"]["policy"], "reject");
    let decoded: AuthOutcome = serde_json::from_value(auth).unwrap();
    assert_eq!(decoded, self::auth());

    let recipients: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT rcpt, address_id FROM mail_receipt_recipients WHERE receipt_id = $1 ORDER BY rcpt",
    )
    .bind(receipt_id.to_string())
    .fetch_all(fixture.database.pool())
    .await
    .unwrap();
    assert_eq!(
        recipients,
        vec![
            ("box+news@one.test".to_owned(), Some(address.to_string())),
            ("team@two.test".to_owned(), Some(alias.to_string())),
        ]
    );

    // The blob is the sealed message: it opens with the workspace key and the receipt's AAD only.
    let storage_key: String =
        sqlx::query_scalar("SELECT storage_key FROM attachment_blobs WHERE id = $1")
            .bind(&blob_id)
            .fetch_one(fixture.database.pool())
            .await
            .unwrap();
    let sealed = fixture
        .mail
        .uploads()
        .read_blob(&storage_key)
        .await
        .unwrap();
    assert_ne!(sealed.as_slice(), RAW);
    let key = fixture.mail.keys().get(fixture.first, 1).await.unwrap();
    let opened = open_blob(&key, &receipt_aad(receipt_id), &sealed).unwrap();
    assert_eq!(opened.as_slice(), RAW);
    assert!(open_blob(&key, &receipt_aad(Id::new_v7()), &sealed).is_err());

    let (kind, workspace, payload, state): (String, Option<String>, String, String) =
        sqlx::query_as("SELECT kind, workspace_id, payload_json, state FROM jobs WHERE kind = $1")
            .bind("mail.ingest")
            .fetch_one(fixture.database.pool())
            .await
            .unwrap();
    assert_eq!(kind, "mail.ingest");
    assert_eq!(workspace, Some(fixture.first.to_string()));
    assert_eq!(state, "queued");
    let payload: Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(payload["receipt_id"], receipt_id.to_string());
}

#[tokio::test]
async fn a_failed_blob_install_stores_nothing() {
    let fixture = Fixture::new().await;
    let (_, address) = fixture.address(fixture.first, "one.test", "box").await;
    fixture.fail.store(true, Ordering::SeqCst);

    let error = fixture
        .intake
        .accept(message(vec![to(fixture.first, address, "box@one.test")]))
        .await
        .unwrap_err();
    assert!(matches!(error, IntakeError::Temporary(_)), "{error:?}");
    assert_eq!(fixture.receipts(fixture.first).await, 0);
    assert_eq!(fixture.blobs(fixture.first).await, 0);
    assert_eq!(fixture.jobs().await, 0);

    fixture.fail.store(false, Ordering::SeqCst);
    let ids = fixture
        .intake
        .accept(message(vec![to(fixture.first, address, "box@one.test")]))
        .await
        .unwrap();
    assert_eq!(ids.len(), 1, "the retry of the sender is stored");
}

#[tokio::test]
async fn over_quota_is_refused() {
    let fixture = Fixture::new().await;
    let (_, address) = fixture.address(fixture.first, "one.test", "box").await;
    fixture
        .execute(
            "UPDATE instance_settings SET workspace_storage_quota_bytes = 10 WHERE id = 1",
            &[],
        )
        .await;

    let error = fixture
        .intake
        .accept(message(vec![to(fixture.first, address, "box@one.test")]))
        .await
        .unwrap_err();
    assert!(matches!(error, IntakeError::QuotaExceeded), "{error:?}");
    assert_eq!(fixture.receipts(fixture.first).await, 0);
    assert_eq!(fixture.jobs().await, 0);
}

#[tokio::test]
async fn over_the_size_limit_is_refused() {
    let fixture = Fixture::with_limit(16).await;
    let (_, address) = fixture.address(fixture.first, "one.test", "box").await;

    let error = fixture
        .intake
        .accept(message(vec![to(fixture.first, address, "box@one.test")]))
        .await
        .unwrap_err();
    assert!(matches!(error, IntakeError::TooLarge), "{error:?}");
    assert_eq!(fixture.receipts(fixture.first).await, 0);
}

#[tokio::test]
async fn two_workspaces_get_their_own_receipts_and_fail_together() {
    let fixture = Fixture::new().await;
    let (_, first) = fixture.address(fixture.first, "one.test", "box").await;
    let (_, second) = fixture.address(fixture.second, "two.test", "box").await;

    let ids = fixture
        .intake
        .accept(message(vec![
            to(fixture.first, first, "box@one.test"),
            to(fixture.second, second, "box@two.test"),
        ]))
        .await
        .unwrap();
    assert_eq!(ids.len(), 2);
    let blobs: Vec<(String, String)> =
        sqlx::query_as("SELECT workspace_id, blob_id FROM mail_receipts ORDER BY workspace_id")
            .fetch_all(fixture.database.pool())
            .await
            .unwrap();
    assert_eq!(blobs.len(), 2);
    assert_ne!(blobs[0].1, blobs[1].1, "each workspace has its own blob");
    assert_ne!(blobs[0].0, blobs[1].0);
    assert_eq!(fixture.jobs().await, 2);

    // The second workspace is over the quota (it already stores 1 MB); the first is not, but
    // the reply is for the whole message, so nothing is stored for either.
    fixture
        .execute(
            "UPDATE instance_settings SET workspace_storage_quota_bytes = 1000000 WHERE id = 1",
            &[],
        )
        .await;
    let now = TimestampMillis::now().as_millis();
    fixture
        .execute(
            &format!(
                "INSERT INTO attachment_blobs (id, workspace_id, sha256, byte_size, storage_key, created_at, quarantine_until) \
                 VALUES ($1, $2, $3, 1000000, 'blobs/large', {now}, {now})"
            ),
            &[
                &Id::new_v7().to_string(),
                &fixture.second.to_string(),
                &"b".repeat(64),
            ],
        )
        .await;
    let first_blobs = fixture.blobs(fixture.first).await;
    let error = fixture
        .intake
        .accept(message(vec![
            to(fixture.first, first, "box@one.test"),
            to(fixture.second, second, "box@two.test"),
        ]))
        .await
        .unwrap_err();
    assert!(matches!(error, IntakeError::QuotaExceeded), "{error:?}");
    assert_eq!(
        fixture.receipts(fixture.first).await,
        1,
        "first rolled back"
    );
    assert_eq!(fixture.receipts(fixture.second).await, 1);
    assert_eq!(fixture.blobs(fixture.first).await, first_blobs);
    assert_eq!(fixture.jobs().await, 2);
}

#[tokio::test]
async fn a_health_test_records_its_result_without_a_receipt() {
    let fixture = Fixture::new().await;
    let (domain, _) = fixture.address(fixture.first, "one.test", "box").await;
    let test_id = Id::new_v7();
    let now = TimestampMillis::now().as_millis();
    fixture
        .execute(
            &format!(
                "INSERT INTO mail_health_tests (id, domain_id, address_local, expires_at, created_at) \
                 VALUES ($1, $2, 'check-abcdefgh', {}, {now})",
                now + 3_600_000
            ),
            &[&test_id.to_string(), &domain.to_string()],
        )
        .await;

    let ids = fixture
        .intake
        .accept(message(vec![Recipient::HealthTest {
            test_id,
            rcpt: "check-abcdefgh@one.test".to_owned(),
        }]))
        .await
        .unwrap();
    assert!(ids.is_empty());
    assert_eq!(fixture.receipts(fixture.first).await, 0);
    assert_eq!(fixture.blobs(fixture.first).await, 0);
    assert_eq!(fixture.jobs().await, 0);

    let (result, received_at): (Option<String>, Option<i64>) =
        sqlx::query_as("SELECT result, received_at FROM mail_health_tests WHERE id = $1")
            .bind(test_id.to_string())
            .fetch_one(fixture.database.pool())
            .await
            .unwrap();
    assert!(received_at.is_some());
    let result: Value = serde_json::from_str(&result.unwrap()).unwrap();
    assert_eq!(result["peer_ip"], "192.0.2.1");
    assert_eq!(result["tls"]["cipher"], "TLS13_AES_128_GCM_SHA256");
    assert_eq!(result["auth"]["spf"], "pass");
    assert_eq!(result["auth"]["dmarc"]["result"], "fail");
}
