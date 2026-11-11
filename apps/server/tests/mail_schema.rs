//! Migration 0094 (mail): constraints, blob references and the workspace purge, on SQLite and
//! Postgres.

use std::sync::Arc;

use orbit_platform::{
    AttachmentMutationCoordinator, Id, IntegrityService, LocalBlobStore, PasswordService,
    TestDatabase, TimestampMillis, UploadLimits, UploadService,
};
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::repositories::instance_settings::InstanceSettingsRepository;
use orbit_server::repositories::mail::MailRepository;
use orbit_server::repositories::workspaces::WorkspaceRepository;

const DAY_MILLIS: i64 = 24 * 60 * 60 * 1000;

struct Fixture {
    _root: tempfile::TempDir,
    database: TestDatabase,
    store: Arc<LocalBlobStore>,
    uploads: UploadService,
    user_id: Id,
    workspace_id: String,
    membership_id: String,
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
        let root = tempfile::tempdir().unwrap();
        let store = Arc::new(LocalBlobStore::new(root.path().join("attachments")));
        let uploads = UploadService::new(
            (*database).clone(),
            store.clone(),
            AttachmentMutationCoordinator::default(),
            UploadLimits::default(),
        );
        let workspace_id = setup.workspace_id.to_string();
        let membership_id =
            sqlx::query_scalar("SELECT id FROM memberships WHERE workspace_id = $1")
                .bind(&workspace_id)
                .fetch_one(database.pool())
                .await
                .unwrap();
        Self {
            _root: root,
            database,
            store,
            uploads,
            user_id: setup.user_id,
            workspace_id,
            membership_id,
        }
    }

    async fn count(&self, sql: &str) -> i64 {
        sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_owned()))
            .fetch_one(self.database.pool())
            .await
            .unwrap()
    }

    async fn domain(&self, name: &str) -> Result<String, sqlx::Error> {
        let id = Id::new_v7().to_string();
        let now = TimestampMillis::now().as_millis();
        sqlx::query(
            "INSERT INTO mail_domains (id, workspace_id, name, status, verify_token, created_at, updated_at) \
             VALUES ($1, $2, $3, 'pending', 'token', $4, $5)",
        )
        .bind(&id)
        .bind(&self.workspace_id)
        .bind(name)
        .bind(now)
        .bind(now)
        .execute(self.database.pool())
        .await?;
        Ok(id)
    }

    async fn mailbox(&self, kind: &str, owner: Option<&str>) -> Result<String, sqlx::Error> {
        let id = Id::new_v7().to_string();
        let now = TimestampMillis::now().as_millis();
        sqlx::query(
            "INSERT INTO mailboxes (id, workspace_id, kind, name, owner_membership_id, status, created_at, updated_at) \
             VALUES ($1, $2, $3, 'Box', $4, 'active', $5, $6)",
        )
        .bind(&id)
        .bind(&self.workspace_id)
        .bind(kind)
        .bind(owner)
        .bind(now)
        .bind(now)
        .execute(self.database.pool())
        .await?;
        Ok(id)
    }

    async fn address(
        &self,
        domain_id: &str,
        mailbox_id: &str,
        local_part: &str,
        kind: &str,
    ) -> Result<String, sqlx::Error> {
        let id = Id::new_v7().to_string();
        sqlx::query(
            "INSERT INTO mail_addresses (id, domain_id, mailbox_id, local_part, kind, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(&id)
        .bind(domain_id)
        .bind(mailbox_id)
        .bind(local_part)
        .bind(kind)
        .bind(TimestampMillis::now().as_millis())
        .execute(self.database.pool())
        .await?;
        Ok(id)
    }

    /// A stored blob and a receipt that references it; returns (receipt id, blob id).
    async fn receipt(&self, raw: &[u8]) -> (String, String) {
        let finalization = self.uploads.begin_finalization().await;
        let now = TimestampMillis::now();
        let mut tx = self.database.immediate_transaction().await.unwrap();
        let workspace_id: Id = self.workspace_id.parse().unwrap();
        let blob = finalization
            .finalize_bytes_in_transaction(&mut tx, workspace_id, raw, now)
            .await
            .unwrap();
        let id = Id::new_v7().to_string();
        sqlx::query(
            "INSERT INTO mail_receipts (id, workspace_id, source, blob_id, key_version, raw_sha256, size, \
             mail_from, peer_ip, helo, tls, auth, status, received_at, updated_at) \
             VALUES ($1, $2, 'smtp', $3, 1, $4, $5, 'ann@sender.test', '192.0.2.1', 'mx.sender.test', NULL, \
             '{\"dmarc\":\"pass\"}', 'pending', $6, $7)",
        )
        .bind(&id)
        .bind(&self.workspace_id)
        .bind(blob.id.to_string())
        .bind("a".repeat(64))
        .bind(i64::try_from(raw.len()).unwrap())
        .bind(now.as_millis())
        .bind(now.as_millis())
        .execute(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
        (id, blob.id.to_string())
    }

    async fn message(&self, receipt_id: &str) -> String {
        let id = Id::new_v7().to_string();
        let now = TimestampMillis::now().as_millis();
        sqlx::query(
            "INSERT INTO mail_messages (id, workspace_id, receipt_id, raw_sha256, message_id_header, \
             from_name, from_addr, subject, received_at, size, created_at) \
             VALUES ($1, $2, $3, $4, '<one@sender.test>', 'Ann', 'ann@sender.test', 'Hello', $5, 10, $6)",
        )
        .bind(&id)
        .bind(&self.workspace_id)
        .bind(receipt_id)
        .bind("a".repeat(64))
        .bind(now)
        .bind(now)
        .execute(self.database.pool())
        .await
        .unwrap();
        id
    }

    /// Returns (thread id, search row id).
    async fn thread(&self, mailbox_id: &str) -> (String, i64) {
        let id = Id::new_v7().to_string();
        let now = TimestampMillis::now().as_millis();
        sqlx::query(
            "INSERT INTO mail_threads (id, workspace_id, mailbox_id, subject, normalized_subject, folder, \
             last_message_at, message_count, snippet, created_at, updated_at) \
             VALUES ($1, $2, $3, 'Hello', 'hello', 'inbox', $4, 1, 'Hi', $5, $6)",
        )
        .bind(&id)
        .bind(&self.workspace_id)
        .bind(mailbox_id)
        .bind(now)
        .bind(now)
        .bind(now)
        .execute(self.database.pool())
        .await
        .unwrap();
        let row_id = sqlx::query_scalar("SELECT row_id FROM mail_threads WHERE id = $1")
            .bind(&id)
            .fetch_one(self.database.pool())
            .await
            .unwrap();
        (id, row_id)
    }

    async fn thread_message(
        &self,
        thread_id: &str,
        message_id: &str,
        mailbox_id: &str,
        raw_sha256: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO mail_thread_messages (thread_id, message_id, mailbox_id, raw_sha256, external, created_at) \
             VALUES ($1, $2, $3, $4, 0, $5)",
        )
        .bind(thread_id)
        .bind(message_id)
        .bind(mailbox_id)
        .bind(raw_sha256)
        .bind(TimestampMillis::now().as_millis())
        .execute(self.database.pool())
        .await
        .map(|_| ())
    }

    async fn search_hits(&self, word: &str) -> Vec<i64> {
        let sql = if orbit_platform::testing_postgres() {
            "SELECT rowid FROM mail_search WHERE search_vector @@ plainto_tsquery('orbit_search', $1) ORDER BY rowid"
        } else {
            "SELECT rowid FROM mail_search WHERE mail_search MATCH $1 ORDER BY rowid"
        };
        sqlx::query_scalar(sql)
            .bind(word)
            .fetch_all(self.database.pool())
            .await
            .unwrap()
    }
}

#[tokio::test]
async fn schema_0094_applies_and_constraints_hold() {
    let fixture = Fixture::new().await;
    assert_eq!(
        fixture
            .count("SELECT MAX(version) FROM schema_migrations")
            .await,
        94
    );

    // A domain name has one owner on the instance, whatever its case.
    let domain = fixture.domain("example.com").await.unwrap();
    assert!(fixture.domain("Example.COM").await.is_err());

    // A shared mailbox has no owner.
    assert!(
        fixture
            .mailbox("shared", Some(&fixture.membership_id))
            .await
            .is_err()
    );
    let mailbox = fixture.mailbox("shared", None).await.unwrap();

    // One primary address for each mailbox; a local part is unique within its domain.
    fixture
        .address(&domain, &mailbox, "ann", "primary")
        .await
        .unwrap();
    assert!(
        fixture
            .address(&domain, &mailbox, "bob", "primary")
            .await
            .is_err()
    );
    fixture
        .address(&domain, &mailbox, "bob", "alias")
        .await
        .unwrap();
    let other = fixture.mailbox("shared", None).await.unwrap();
    assert!(
        fixture
            .address(&domain, &other, "ann", "primary")
            .await
            .is_err()
    );

    // One copy of a raw message in each mailbox.
    let (first_receipt, _) = fixture.receipt(b"first raw message").await;
    let (second_receipt, _) = fixture.receipt(b"second raw message").await;
    let first_message = fixture.message(&first_receipt).await;
    let second_message = fixture.message(&second_receipt).await;
    let (thread, _) = fixture.thread(&mailbox).await;
    let (other_thread, _) = fixture.thread(&other).await;
    let sha = "b".repeat(64);
    fixture
        .thread_message(&thread, &first_message, &mailbox, &sha)
        .await
        .unwrap();
    assert!(
        fixture
            .thread_message(&thread, &second_message, &mailbox, &sha)
            .await
            .is_err()
    );
    fixture
        .thread_message(&other_thread, &first_message, &other, &sha)
        .await
        .unwrap();

    // One message for each receipt, so a retried ingest finds the first one.
    let duplicate = sqlx::query(
        "INSERT INTO mail_messages (id, workspace_id, receipt_id, raw_sha256, received_at, size, created_at) \
         VALUES ($1, $2, $3, $4, 0, 0, 0)",
    )
    .bind(Id::new_v7().to_string())
    .bind(&fixture.workspace_id)
    .bind(&first_receipt)
    .bind("a".repeat(64))
    .execute(fixture.database.pool())
    .await;
    assert!(duplicate.is_err());

    // Label names are unique in a workspace, whatever their case.
    let label = |name: &'static str| {
        sqlx::query(
            "INSERT INTO mail_labels (id, workspace_id, name, color, created_at, updated_at) \
             VALUES ($1, $2, $3, 'red', 0, 0)",
        )
        .bind(Id::new_v7().to_string())
        .bind(&fixture.workspace_id)
        .bind(name)
        .execute(fixture.database.pool())
    };
    label("Invoices").await.unwrap();
    assert!(label("invoices").await.is_err());

    // Mail domains are off until the root user turns them on.
    let settings = InstanceSettingsRepository::new((*fixture.database).clone());
    assert!(!settings.mail_domains_enabled().await.unwrap());
    settings
        .set_mail_domains_enabled(fixture.user_id, true, "request", TimestampMillis::now())
        .await
        .unwrap();
    assert!(settings.mail_domains_enabled().await.unwrap());
}

#[tokio::test]
async fn mail_rows_stay_inside_their_workspace() {
    let fixture = Fixture::new().await;
    let workspaces = WorkspaceRepository::new((*fixture.database).clone());
    let second = workspaces
        .create(
            fixture.user_id,
            "Second".to_owned(),
            "request",
            TimestampMillis::now(),
        )
        .await
        .unwrap();
    let domain = fixture.domain("example.com").await.unwrap();
    let now = TimestampMillis::now().as_millis();
    let foreign_mailbox = Id::new_v7().to_string();
    sqlx::query(
        "INSERT INTO mailboxes (id, workspace_id, kind, name, status, created_at, updated_at) \
         VALUES ($1, $2, 'shared', 'Elsewhere', 'active', $3, $4)",
    )
    .bind(&foreign_mailbox)
    .bind(second.id.to_string())
    .bind(now)
    .bind(now)
    .execute(fixture.database.pool())
    .await
    .unwrap();

    // An address joins a domain and a mailbox of one workspace only.
    assert!(
        fixture
            .address(&domain, &foreign_mailbox, "ann", "primary")
            .await
            .is_err()
    );
    // A mailbox member is a member of the mailbox's workspace.
    let foreign_member = sqlx::query(
        "INSERT INTO mailbox_members (mailbox_id, membership_id, created_at) VALUES ($1, $2, $3)",
    )
    .bind(&foreign_mailbox)
    .bind(&fixture.membership_id)
    .bind(now)
    .execute(fixture.database.pool())
    .await;
    assert!(foreign_member.is_err());
}

#[tokio::test]
async fn an_owner_who_leaves_keeps_the_personal_mailbox_for_the_admins() {
    let fixture = Fixture::new().await;
    let now = TimestampMillis::now().as_millis();
    let user = Id::new_v7().to_string();
    sqlx::query(
        "INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at) \
         VALUES ($1, 'ann@example.com', 'ann@example.com', 'Ann', 'unused', $2, $3)",
    )
    .bind(&user)
    .bind(now)
    .bind(now)
    .execute(fixture.database.pool())
    .await
    .unwrap();
    let membership = Id::new_v7().to_string();
    sqlx::query(
        "INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at) \
         VALUES ($1, $2, $3, 'member', 0, $4, $5)",
    )
    .bind(&membership)
    .bind(&fixture.workspace_id)
    .bind(&user)
    .bind(now)
    .bind(now)
    .execute(fixture.database.pool())
    .await
    .unwrap();
    let mailbox = fixture
        .mailbox("personal", Some(&membership))
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO mailbox_members (mailbox_id, membership_id, created_at) VALUES ($1, $2, $3)",
    )
    .bind(&mailbox)
    .bind(&membership)
    .bind(now)
    .execute(fixture.database.pool())
    .await
    .unwrap();

    sqlx::query("DELETE FROM memberships WHERE id = $1")
        .bind(&membership)
        .execute(fixture.database.pool())
        .await
        .unwrap();

    let owner: Option<String> =
        sqlx::query_scalar("SELECT owner_membership_id FROM mailboxes WHERE id = $1")
            .bind(&mailbox)
            .fetch_one(fixture.database.pool())
            .await
            .unwrap();
    assert_eq!(owner, None);
    assert_eq!(
        fixture.count("SELECT COUNT(*) FROM mailbox_members").await,
        0
    );
}

#[tokio::test]
async fn search_rows_follow_their_thread() {
    let fixture = Fixture::new().await;
    let mailbox = fixture.mailbox("shared", None).await.unwrap();
    let (first, first_row) = fixture.thread(&mailbox).await;
    let (_, second_row) = fixture.thread(&mailbox).await;
    let index = |row_id: i64, body: &'static str| {
        sqlx::query(
            "INSERT INTO mail_search (rowid, subject, people, body) VALUES ($1, $2, $3, $4)",
        )
        .bind(row_id)
        .bind("Quarterly invoice")
        .bind("Ann Smith ann@sender.test")
        .bind(body)
        .execute(fixture.database.pool())
    };
    index(first_row, "Please find the café receipt attached")
        .await
        .unwrap();
    index(second_row, "Nothing to see").await.unwrap();

    assert_eq!(fixture.search_hits("cafe").await, vec![first_row]);
    assert_eq!(
        fixture.search_hits("ann").await,
        vec![first_row, second_row]
    );

    // A re-index replaces the row.
    sqlx::query("DELETE FROM mail_search WHERE rowid = $1")
        .bind(second_row)
        .execute(fixture.database.pool())
        .await
        .unwrap();
    index(second_row, "A new café").await.unwrap();
    assert_eq!(
        fixture.search_hits("cafe").await,
        vec![first_row, second_row]
    );

    // A deleted thread leaves the index.
    sqlx::query("DELETE FROM mail_threads WHERE id = $1")
        .bind(&first)
        .execute(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(fixture.search_hits("cafe").await, vec![second_row]);

    // The weekly integrity job (and App start) accepts the contentless index after these changes.
    IntegrityService::new((*fixture.database).clone())
        .full()
        .await
        .unwrap();
}

#[tokio::test]
async fn blob_referenced_by_receipt_is_not_reconciled() {
    let fixture = Fixture::new().await;
    let (receipt, kept) = fixture.receipt(b"sealed raw message").await;
    // A blob without a reference, to prove the reconcile runs.
    let finalization = fixture.uploads.begin_finalization().await;
    let mut tx = fixture.database.immediate_transaction().await.unwrap();
    finalization
        .finalize_bytes_in_transaction(
            &mut tx,
            fixture.workspace_id.parse().unwrap(),
            b"orphan",
            TimestampMillis::now(),
        )
        .await
        .unwrap();
    tx.commit().await.unwrap();
    drop(finalization);

    let later = TimestampMillis::now().as_millis() + 2 * DAY_MILLIS;
    let result = fixture.uploads.reconcile(later).await.unwrap();
    assert_eq!(result.deleted_blobs, 1);
    let remaining: Vec<String> = sqlx::query_scalar("SELECT id FROM attachment_blobs")
        .fetch_all(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(remaining, vec![kept.clone()]);

    // A receipt holds its blob: the blob cannot be deleted under it.
    let delete_blob = sqlx::query("DELETE FROM attachment_blobs WHERE id = $1")
        .bind(&kept)
        .execute(fixture.database.pool())
        .await;
    assert!(delete_blob.is_err());

    // Once the receipt is gone, the blob is quarantined and then reclaimed.
    sqlx::query("DELETE FROM mail_receipts WHERE id = $1")
        .bind(&receipt)
        .execute(fixture.database.pool())
        .await
        .unwrap();
    let result = fixture
        .uploads
        .reconcile(TimestampMillis::now().as_millis() + 3 * DAY_MILLIS)
        .await
        .unwrap();
    assert_eq!(result.deleted_blobs, 1);
    assert!(fixture.store.blobs().await.unwrap().is_empty());
}

#[tokio::test]
async fn workspace_purge_removes_every_mail_row_and_blob() {
    let fixture = Fixture::new().await;
    let now = TimestampMillis::now().as_millis();
    let domain = fixture.domain("example.com").await.unwrap();
    let mailbox = fixture
        .mailbox("personal", Some(&fixture.membership_id))
        .await
        .unwrap();
    sqlx::query("UPDATE mail_domains SET catch_all_mailbox_id = $1 WHERE id = $2")
        .bind(&mailbox)
        .bind(&domain)
        .execute(fixture.database.pool())
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO mailbox_members (mailbox_id, membership_id, created_at) VALUES ($1, $2, $3)",
    )
    .bind(&mailbox)
    .bind(&fixture.membership_id)
    .bind(now)
    .execute(fixture.database.pool())
    .await
    .unwrap();
    let address = fixture
        .address(&domain, &mailbox, "owner", "primary")
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO mail_dkim_keys (id, domain_id, selector, public_key, private_sealed, status, created_at, updated_at) \
         VALUES ($1, $2, 'orbit2026a', 'MIIB', $3, 'pending', $4, $5)",
    )
    .bind(Id::new_v7().to_string())
    .bind(&domain)
    .bind(vec![1_u8; 40])
    .bind(now)
    .bind(now)
    .execute(fixture.database.pool())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO mail_health_tests (id, domain_id, address_local, expires_at, created_at) \
         VALUES ($1, $2, 'check-abcdefgh', $3, $4)",
    )
    .bind(Id::new_v7().to_string())
    .bind(&domain)
    .bind(now + 3_600_000)
    .bind(now)
    .execute(fixture.database.pool())
    .await
    .unwrap();
    let repository = MailRepository::new(
        (*fixture.database).clone(),
        fixture.uploads.clone(),
        [7; 32],
    );
    let mut tx = fixture.database.immediate_transaction().await.unwrap();
    repository
        .keys()
        .current(&mut tx, fixture.workspace_id.parse().unwrap())
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let (receipt, _) = fixture.receipt(b"sealed raw message").await;
    sqlx::query(
        "INSERT INTO mail_receipt_recipients (receipt_id, address_id, rcpt) VALUES ($1, $2, 'owner+news@example.com')",
    )
    .bind(&receipt)
    .bind(&address)
    .execute(fixture.database.pool())
    .await
    .unwrap();
    let message = fixture.message(&receipt).await;
    sqlx::query(
        "INSERT INTO mail_attachments (id, message_id, filename, declared_type, sniffed_type, size, part_id, disposition) \
         VALUES ($1, $2, 'a.pdf', 'application/pdf', 'application/pdf', 3, 2, 'attachment')",
    )
    .bind(Id::new_v7().to_string())
    .bind(&message)
    .execute(fixture.database.pool())
    .await
    .unwrap();
    let (thread, row_id) = fixture.thread(&mailbox).await;
    fixture
        .thread_message(&thread, &message, &mailbox, &"c".repeat(64))
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO mail_thread_user_state (thread_id, membership_id, last_read_at, starred) VALUES ($1, $2, $3, 1)",
    )
    .bind(&thread)
    .bind(&fixture.membership_id)
    .bind(now)
    .execute(fixture.database.pool())
    .await
    .unwrap();
    let label = Id::new_v7().to_string();
    sqlx::query(
        "INSERT INTO mail_labels (id, workspace_id, name, color, created_at, updated_at) VALUES ($1, $2, 'Work', 'red', 0, 0)",
    )
    .bind(&label)
    .bind(&fixture.workspace_id)
    .execute(fixture.database.pool())
    .await
    .unwrap();
    sqlx::query("INSERT INTO mail_thread_labels (thread_id, label_id) VALUES ($1, $2)")
        .bind(&thread)
        .bind(&label)
        .execute(fixture.database.pool())
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO mail_sender_prefs (membership_id, address, load_images, updated_at) VALUES ($1, 'ann@sender.test', 1, $2)",
    )
    .bind(&fixture.membership_id)
    .bind(now)
    .execute(fixture.database.pool())
    .await
    .unwrap();
    sqlx::query("INSERT INTO mail_search (rowid, subject, people, body) VALUES ($1, 'Hello', 'Ann', 'Body')")
        .bind(row_id)
        .execute(fixture.database.pool())
        .await
        .unwrap();

    sqlx::query("UPDATE workspaces SET deleted_at = $1 WHERE id = $2")
        .bind(now - 31 * DAY_MILLIS)
        .bind(&fixture.workspace_id)
        .execute(fixture.database.pool())
        .await
        .unwrap();
    let summary =
        WorkspaceRepository::with_blob_store((*fixture.database).clone(), fixture.store.clone())
            .purge_retention(TimestampMillis::now())
            .await
            .unwrap();

    assert_eq!(summary.workspaces_purged, 1);
    assert_eq!(summary.attachment_blobs_purged, 1);
    for table in [
        "mail_domains",
        "mail_dkim_keys",
        "mailboxes",
        "mail_addresses",
        "mailbox_members",
        "mail_keys",
        "mail_receipts",
        "mail_receipt_recipients",
        "mail_messages",
        "mail_attachments",
        "mail_threads",
        "mail_thread_messages",
        "mail_thread_user_state",
        "mail_labels",
        "mail_thread_labels",
        "mail_health_tests",
        "mail_sender_prefs",
    ] {
        assert_eq!(
            fixture
                .count(&format!("SELECT COUNT(*) FROM {table}"))
                .await,
            0,
            "{table} is empty"
        );
    }
    assert!(fixture.search_hits("hello").await.is_empty());
}
