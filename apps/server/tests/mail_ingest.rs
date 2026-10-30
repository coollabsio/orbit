//! Ingest (spec §4.4, §6.3, §6.4, §9): a receipt is parsed into a message, threaded into each
//! recipient mailbox once (Review Focus 1), safely re-run (Review Focus 2), filed (spam on DMARC
//! failure, an archived thread back to the inbox), indexed for search and announced on the live
//! socket to the mailbox members only. Failed ingests end as `failed` receipts that can be
//! retried, and `mail.purge` removes old trash and frees the blobs. Receipts are made through
//! `MailIntake::accept`, as the listener does.

use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use orbit_mail::{AuthOutcome, DmarcPolicy, DmarcVerdict, Verdict};
use orbit_platform::{
    AttachmentMutationCoordinator, BlobFuture, BlobObject, BlobReader, BlobStore, BlobStoreError,
    Id, IntegrityService, JobError, JobStore, LocalBlobStore, Row, StagedUpload, StoredObject,
    TestDatabase, TimestampMillis, UploadLimits, UploadService, testing_postgres,
};
use orbit_server::mail_intake::{InboundMessage, MailIntake, Recipient, Source};
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::repositories::mail::{
    IngestOutcome, MailError, MailRepository, TRASH_RETENTION_MS, run_ingest_job,
};
use serde_json::{Value, json};

const APP_KEY: [u8; 32] = [5; 32];
const DAY: i64 = 24 * 60 * 60 * 1000;

/// The local store; `open` fails while `fail_open` is set (the blob store is down).
#[derive(Clone)]
struct SwitchableStore {
    inner: LocalBlobStore,
    fail_open: Arc<AtomicBool>,
}

impl BlobStore for SwitchableStore {
    fn create_temporary(&self) -> Result<std::path::PathBuf, BlobStoreError> {
        BlobStore::create_temporary(&self.inner)
    }

    fn install<'a>(&'a self, upload: &'a StagedUpload) -> BlobFuture<'a, StoredObject> {
        BlobStore::install(&self.inner, upload)
    }

    fn open<'a>(&'a self, storage_key: &'a str) -> BlobFuture<'a, BlobReader> {
        if self.fail_open.load(Ordering::SeqCst) {
            return Box::pin(async {
                Err(BlobStoreError::Io {
                    path: "blobs".into(),
                    source: std::io::Error::other("store unavailable"),
                })
            });
        }
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
    jobs: JobStore,
    fail_open: Arc<AtomicBool>,
    workspace: Id,
    owner: Id,
    /// A workspace member who is not a member of the mailbox.
    outsider: Id,
    mailbox: Id,
    /// `box@one.test`, the primary address of `mailbox`.
    primary: Id,
    /// `team@one.test`, an alias of `mailbox`.
    alias: Id,
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
                    password_hash: "unused".to_owned(),
                    workspace_name: "Orbit".to_owned(),
                    project_name: "General".to_owned(),
                },
                now,
            )
            .await
            .unwrap();
        let root = tempfile::tempdir().unwrap();
        let fail_open = Arc::new(AtomicBool::new(false));
        let uploads = UploadService::new(
            (*database).clone(),
            Arc::new(SwitchableStore {
                inner: LocalBlobStore::new(root.path().join("attachments")),
                fail_open: Arc::clone(&fail_open),
            }),
            AttachmentMutationCoordinator::default(),
            UploadLimits::default(),
        );
        let mail = Arc::new(MailRepository::new((*database).clone(), uploads, APP_KEY));
        let jobs = JobStore::new((*database).clone());
        let intake = MailIntake::new(Arc::clone(&mail), jobs.clone(), 52_428_800);
        let mut fixture = Self {
            _root: root,
            database,
            mail,
            intake,
            jobs,
            fail_open,
            workspace: setup.workspace_id,
            owner: setup.user_id,
            outsider: Id::new_v7(),
            mailbox: Id::new_v7(),
            primary: Id::new_v7(),
            alias: Id::new_v7(),
        };
        fixture.outsider = fixture.member("Outsider").await.0;
        let owner_membership = fixture.membership_of(fixture.owner).await;
        let domain = Id::new_v7();
        let ws = fixture.workspace.to_string();
        let at = now.as_millis();
        fixture
            .execute(
                &format!(
                    "INSERT INTO mail_domains (id, workspace_id, name, status, verify_token, created_at, updated_at) \
                     VALUES ($1, $2, 'one.test', 'verified', 'token', {at}, {at})"
                ),
                &[&domain.to_string(), &ws],
            )
            .await;
        fixture
            .execute(
                &format!(
                    "INSERT INTO mailboxes (id, workspace_id, kind, name, owner_membership_id, status, created_at, updated_at) \
                     VALUES ($1, $2, 'shared', 'Box', NULL, 'active', {at}, {at})"
                ),
                &[&fixture.mailbox.to_string(), &ws],
            )
            .await;
        for (id, local, kind) in [
            (fixture.primary, "box", "primary"),
            (fixture.alias, "team", "alias"),
        ] {
            fixture
                .execute(
                    &format!(
                        "INSERT INTO mail_addresses (id, domain_id, mailbox_id, local_part, kind, created_at) \
                         VALUES ($1, $2, $3, $4, $5, {at})"
                    ),
                    &[
                        &id.to_string(),
                        &domain.to_string(),
                        &fixture.mailbox.to_string(),
                        local,
                        kind,
                    ],
                )
                .await;
        }
        fixture
            .execute(
                &format!(
                    "INSERT INTO mailbox_members (mailbox_id, membership_id, created_at) VALUES ($1, $2, {at})"
                ),
                &[&fixture.mailbox.to_string(), &owner_membership.to_string()],
            )
            .await;
        fixture
    }

    /// A new workspace member; returns (user id, membership id).
    async fn member(&self, name: &str) -> (Id, Id) {
        let user = Id::new_v7();
        let membership = Id::new_v7();
        let now = TimestampMillis::now().as_millis();
        let email = format!("{user}@example.com");
        self.execute(
            &format!(
                "INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at) \
                 VALUES ($1, $2, $2, $3, 'unused', {now}, {now})"
            ),
            &[&user.to_string(), &email, name],
        )
        .await;
        self.execute(
            &format!(
                "INSERT INTO memberships (id, workspace_id, user_id, role, version, created_at, updated_at) \
                 VALUES ($1, $2, $3, 'member', 0, {now}, {now})"
            ),
            &[
                &membership.to_string(),
                &self.workspace.to_string(),
                &user.to_string(),
            ],
        )
        .await;
        (user, membership)
    }

    async fn membership_of(&self, user: Id) -> Id {
        let id: String = sqlx::query_scalar(
            "SELECT id FROM memberships WHERE workspace_id = $1 AND user_id = $2",
        )
        .bind(self.workspace.to_string())
        .bind(user.to_string())
        .fetch_one(self.database.pool())
        .await
        .unwrap();
        id.parse().unwrap()
    }

    /// Runs SQL with text binds (times are written into the SQL).
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

    /// Accepts `raw` for `recipients` as the listener would; returns the receipt.
    async fn receive(&self, raw: &[u8], recipients: &[Id], dmarc: DmarcVerdict) -> Id {
        let recipients = recipients
            .iter()
            .map(|address| Recipient::Mailbox {
                workspace_id: self.workspace,
                address_id: *address,
                rcpt: if *address == self.alias {
                    "team@one.test".to_owned()
                } else {
                    "box@one.test".to_owned()
                },
            })
            .collect();
        let ids = self
            .intake
            .accept(InboundMessage {
                source: Source::Smtp,
                trace_len: 0,
                raw: raw.to_vec(),
                mail_from: Some("ann@sender.test".to_owned()),
                recipients,
                peer_ip: IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)),
                helo: "mx.sender.test".to_owned(),
                tls: None,
                auth: auth(dmarc),
            })
            .await
            .unwrap();
        assert_eq!(ids.len(), 1);
        ids[0]
    }

    async fn ingest(&self, receipt: Id) -> IngestOutcome {
        self.mail
            .ingest(receipt, TimestampMillis::now())
            .await
            .unwrap()
    }

    /// Receives and ingests `raw` for the primary address; returns the receipt.
    async fn deliver(&self, raw: &[u8]) -> Id {
        let receipt = self.receive(raw, &[self.primary], DmarcVerdict::Pass).await;
        self.ingest(receipt).await;
        receipt
    }

    async fn threads(&self) -> Vec<Thread> {
        let rows = sqlx::query(
            "SELECT id, row_id, subject, folder, message_count, snippet, spam_reason, has_attachments \
             FROM mail_threads WHERE mailbox_id = $1 ORDER BY row_id",
        )
        .bind(self.mailbox.to_string())
        .fetch_all(self.database.pool())
        .await
        .unwrap();
        rows.iter()
            .map(|row| Thread {
                id: row.try_get("id").unwrap(),
                row_id: row.try_get("row_id").unwrap(),
                subject: row.try_get("subject").unwrap(),
                folder: row.try_get("folder").unwrap(),
                count: row.try_get("message_count").unwrap(),
                snippet: row.try_get("snippet").unwrap(),
                spam_reason: row.try_get("spam_reason").unwrap(),
                has_attachments: row.try_get("has_attachments").unwrap(),
            })
            .collect()
    }

    async fn thread_messages(&self) -> i64 {
        self.count(
            "SELECT COUNT(*) FROM mail_thread_messages WHERE mailbox_id = $1",
            &self.mailbox.to_string(),
        )
        .await
    }

    async fn receipt_status(&self, receipt: Id) -> (String, i64, Option<String>) {
        sqlx::query_as("SELECT status, attempts, error FROM mail_receipts WHERE id = $1")
            .bind(receipt.to_string())
            .fetch_one(self.database.pool())
            .await
            .unwrap()
    }

    /// Search rows for a thread (by `row_id`).
    async fn search_rows(&self, row_id: i64) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM mail_search WHERE rowid = $1")
            .bind(row_id)
            .fetch_one(self.database.pool())
            .await
            .unwrap()
    }

    /// The `row_id`s of threads whose search row matches `word`.
    async fn search(&self, word: &str) -> Vec<i64> {
        let sql = if testing_postgres() {
            "SELECT rowid FROM mail_search WHERE search_vector @@ to_tsquery('orbit_search', $1) ORDER BY rowid"
        } else {
            "SELECT rowid FROM mail_search WHERE mail_search MATCH $1 ORDER BY rowid"
        };
        sqlx::query_scalar(sql)
            .bind(word)
            .fetch_all(self.database.pool())
            .await
            .unwrap()
    }

    /// The `mail` live events: (recipients JSON, event).
    async fn live_events(&self) -> Vec<(Value, Value)> {
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

#[derive(Debug)]
struct Thread {
    id: String,
    row_id: i64,
    subject: String,
    folder: String,
    count: i64,
    snippet: String,
    spam_reason: Option<String>,
    has_attachments: bool,
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

fn mail(from: &str, message_id: &str, headers: &str, subject: &str, body: &str) -> Vec<u8> {
    format!(
        "From: {from}\r\nTo: Box <box@one.test>\r\nCc: carl@other.test\r\nSubject: {subject}\r\n\
         Message-ID: <{message_id}>\r\nDate: Thu, 22 Oct 2026 10:00:00 +0000\r\n{headers}\r\n{body}\r\n"
    )
    .into_bytes()
}

fn first() -> Vec<u8> {
    mail(
        "Ann <ann@sender.test>",
        "first@sender.test",
        "",
        "Invoice for October",
        "Please find the invoice attached.",
    )
}

#[tokio::test]
async fn a_message_starts_a_thread_and_is_indexed() {
    let fixture = Fixture::new().await;
    let raw = b"From: Ann <ann@sender.test>\r\nTo: box@one.test\r\nSubject: Invoice for October\r\n\
Message-ID: <first@sender.test>\r\nMIME-Version: 1.0\r\n\
Content-Type: multipart/mixed; boundary=\"b\"\r\n\r\n\
--b\r\nContent-Type: text/plain\r\n\r\nPlease pay the invoice.\r\n\
--b\r\nContent-Type: application/pdf\r\nContent-Disposition: attachment; filename=\"invoice.pdf\"\r\n\
Content-Transfer-Encoding: base64\r\n\r\nJVBERi0xLjQK\r\n--b--\r\n";
    let receipt = fixture
        .receive(raw, &[fixture.primary], DmarcVerdict::Pass)
        .await;
    let outcome = fixture.ingest(receipt).await;
    let IngestOutcome::Ingested { deliveries, .. } = outcome else {
        panic!("not ingested: {outcome:?}");
    };
    assert_eq!(deliveries.len(), 1);
    assert_eq!(deliveries[0].mailbox_id, fixture.mailbox);
    assert!(deliveries[0].new_thread);

    let threads = fixture.threads().await;
    assert_eq!(threads.len(), 1);
    let thread = &threads[0];
    assert_eq!(thread.id, deliveries[0].thread_id.to_string());
    assert_eq!(thread.subject, "Invoice for October");
    assert_eq!(thread.folder, "inbox");
    assert_eq!(thread.count, 1);
    assert_eq!(thread.snippet, "Please pay the invoice.");
    assert!(thread.has_attachments);
    assert_eq!(fixture.receipt_status(receipt).await.0, "ingested");

    let (from_name, from_addr, to_addrs, message_id, has_attachments): (
        Option<String>,
        Option<String>,
        String,
        Option<String>,
        i64,
    ) = sqlx::query_as(
        "SELECT from_name, from_addr, to_addrs, message_id_header, has_attachments \
         FROM mail_messages WHERE receipt_id = $1",
    )
    .bind(receipt.to_string())
    .fetch_one(fixture.database.pool())
    .await
    .unwrap();
    assert_eq!(from_name.as_deref(), Some("Ann"));
    assert_eq!(from_addr.as_deref(), Some("ann@sender.test"));
    assert_eq!(message_id.as_deref(), Some("first@sender.test"));
    assert_eq!(has_attachments, 1);
    let to: Value = serde_json::from_str(&to_addrs).unwrap();
    assert_eq!(to, json!([{ "name": null, "address": "box@one.test" }]));

    let (filename, declared, disposition, size): (Option<String>, String, String, i64) =
        sqlx::query_as(
            "SELECT a.filename, a.declared_type, a.disposition, a.size FROM mail_attachments a \
             JOIN mail_messages m ON m.id = a.message_id WHERE m.receipt_id = $1",
        )
        .bind(receipt.to_string())
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
    assert_eq!(filename.as_deref(), Some("invoice.pdf"));
    assert_eq!(declared, "application/pdf");
    assert_eq!(disposition, "attachment");
    assert_eq!(size, 9);

    assert_eq!(fixture.search_rows(thread.row_id).await, 1);
    assert_eq!(fixture.search("invoice").await, vec![thread.row_id]);
    assert_eq!(fixture.search("ann").await, vec![thread.row_id], "people");
    assert_eq!(fixture.search("pay").await, vec![thread.row_id], "body");
}

#[tokio::test]
async fn a_reply_with_in_reply_to_joins_the_thread() {
    let fixture = Fixture::new().await;
    fixture.deliver(&first()).await;
    // Another subject would not matter: Ann is a participant.
    fixture
        .deliver(&mail(
            "Ann <ann@sender.test>",
            "second@sender.test",
            "In-Reply-To: <first@sender.test>\r\nReferences: <first@sender.test>\r\n",
            "Re: Invoice for October",
            "Paid yesterday.",
        ))
        .await;

    let threads = fixture.threads().await;
    assert_eq!(threads.len(), 1, "{threads:?}");
    assert_eq!(threads[0].count, 2);
    assert_eq!(threads[0].snippet, "Paid yesterday.");
    assert_eq!(threads[0].subject, "Invoice for October");
    assert_eq!(fixture.thread_messages().await, 2);

    // Carl was on Cc of the first message: a participant, so another subject joins as well.
    fixture
        .deliver(&mail(
            "Carl <carl@other.test>",
            "third@other.test",
            "In-Reply-To: <second@sender.test>\r\n",
            "Something else entirely",
            "Joining in.",
        ))
        .await;
    let threads = fixture.threads().await;
    assert_eq!(threads.len(), 1, "{threads:?}");
    assert_eq!(threads[0].count, 3);
    let external: Vec<i64> =
        sqlx::query_scalar("SELECT external FROM mail_thread_messages WHERE mailbox_id = $1")
            .bind(fixture.mailbox.to_string())
            .fetch_all(fixture.database.pool())
            .await
            .unwrap();
    assert_eq!(external, vec![0, 0, 0], "the thread started outside");
    // The thread's search row was replaced three times; the weekly check still passes.
    IntegrityService::new((*fixture.database).clone())
        .full()
        .await
        .unwrap();
}

#[tokio::test]
async fn forged_references_from_an_unknown_sender_start_a_new_thread() {
    let fixture = Fixture::new().await;
    fixture.deliver(&first()).await;
    fixture
        .deliver(&mail(
            "Mallory <mallory@evil.test>",
            "forged@evil.test",
            "In-Reply-To: <first@sender.test>\r\nReferences: <first@sender.test>\r\n",
            "Updated bank details",
            "Please pay to this account instead.",
        ))
        .await;

    let threads = fixture.threads().await;
    assert_eq!(threads.len(), 2, "{threads:?}");
    assert_eq!(threads[0].count, 1);
    assert_eq!(threads[1].count, 1);
    assert_eq!(threads[1].subject, "Updated bank details");
}

#[tokio::test]
async fn an_outsider_in_a_thread_that_started_inside_is_marked_external() {
    let fixture = Fixture::new().await;
    // A colleague writes from the workspace's own domain, cc'ing an outside address.
    fixture
        .deliver(&mail(
            "Bob <bob@one.test>",
            "inside@one.test",
            "",
            "Quarterly plan",
            "Draft attached.",
        ))
        .await;
    fixture
        .deliver(&mail(
            "Carl <carl@other.test>",
            "reply@other.test",
            "In-Reply-To: <inside@one.test>\r\n",
            "Re: Quarterly plan",
            "Looks good.",
        ))
        .await;
    assert_eq!(fixture.threads().await.len(), 1);
    let external: Vec<(String, i64)> = sqlx::query_as(
        "SELECT m.from_addr, tm.external FROM mail_thread_messages tm \
         JOIN mail_messages m ON m.id = tm.message_id WHERE tm.mailbox_id = $1 ORDER BY m.received_at, m.id",
    )
    .bind(fixture.mailbox.to_string())
    .fetch_all(fixture.database.pool())
    .await
    .unwrap();
    assert_eq!(
        external,
        vec![
            ("bob@one.test".to_owned(), 0),
            ("carl@other.test".to_owned(), 1)
        ]
    );
}

/// Review Focus 1: one mailbox addressed through two of its addresses gets the message once.
#[tokio::test]
async fn two_addresses_of_one_mailbox_get_the_message_once() {
    let fixture = Fixture::new().await;
    let receipt = fixture
        .receive(
            &first(),
            &[fixture.primary, fixture.alias],
            DmarcVerdict::Pass,
        )
        .await;
    let IngestOutcome::Ingested { deliveries, .. } = fixture.ingest(receipt).await else {
        panic!("not ingested");
    };
    assert_eq!(deliveries.len(), 1);
    assert_eq!(fixture.thread_messages().await, 1);
    let threads = fixture.threads().await;
    assert_eq!(threads.len(), 1);
    assert_eq!(threads[0].count, 1);

    // The sender retries the same bytes (a second receipt): the dedupe key drops it.
    let again = fixture
        .receive(&first(), &[fixture.alias], DmarcVerdict::Pass)
        .await;
    let IngestOutcome::Ingested { deliveries, .. } = fixture.ingest(again).await else {
        panic!("not ingested");
    };
    assert!(deliveries.is_empty());
    assert_eq!(fixture.thread_messages().await, 1);
    assert_eq!(fixture.threads().await[0].count, 1);
    assert_eq!(fixture.receipt_status(again).await.0, "ingested");
}

/// Review Focus 2: an ingest that runs again (a job retry after the commit, or a receipt that
/// was not marked) adds nothing and leaves the counts.
#[tokio::test]
async fn ingest_again_adds_nothing() {
    let fixture = Fixture::new().await;
    let receipt = fixture.deliver(&first()).await;
    let before = fixture.threads().await;
    assert_eq!(before.len(), 1);

    assert!(matches!(
        fixture.ingest(receipt).await,
        IngestOutcome::AlreadyIngested
    ));

    // As if the job ran again before the receipt was marked.
    sqlx::query("UPDATE mail_receipts SET status = 'pending' WHERE id = $1")
        .bind(receipt.to_string())
        .execute(fixture.database.pool())
        .await
        .unwrap();
    let IngestOutcome::Ingested { deliveries, .. } = fixture.ingest(receipt).await else {
        panic!("not ingested");
    };
    assert!(deliveries.is_empty(), "{deliveries:?}");
    assert_eq!(fixture.receipt_status(receipt).await.0, "ingested");
    let after = fixture.threads().await;
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].count, 1);
    assert_eq!(after[0].id, before[0].id);
    assert_eq!(fixture.thread_messages().await, 1);
    assert_eq!(
        fixture
            .count(
                "SELECT COUNT(*) FROM mail_messages WHERE receipt_id = $1",
                &receipt.to_string()
            )
            .await,
        1
    );
    assert_eq!(fixture.search_rows(after[0].row_id).await, 1);
    assert_eq!(fixture.live_events().await.len(), 1, "no second event");
}

#[tokio::test]
async fn dmarc_failure_files_the_message_as_spam() {
    let fixture = Fixture::new().await;
    fixture.deliver(&first()).await;
    let forged = fixture
        .receive(
            &mail(
                "Ann <ann@sender.test>",
                "spoof@sender.test",
                "In-Reply-To: <first@sender.test>\r\n",
                "Re: Invoice for October",
                "Wire it here.",
            ),
            &[fixture.primary],
            DmarcVerdict::Fail {
                policy: DmarcPolicy::Reject,
            },
        )
        .await;
    fixture.ingest(forged).await;

    let threads = fixture.threads().await;
    assert_eq!(
        threads.len(),
        2,
        "a failed message does not join an inbox thread"
    );
    assert_eq!(threads[0].folder, "inbox");
    assert_eq!(threads[0].count, 1);
    assert_eq!(threads[1].folder, "spam");
    assert_eq!(threads[1].spam_reason.as_deref(), Some("dmarc_fail"));
}

#[tokio::test]
async fn a_reply_moves_an_archived_thread_to_the_inbox_but_not_a_trashed_one() {
    let fixture = Fixture::new().await;
    fixture.deliver(&first()).await;
    let thread = fixture.threads().await.remove(0);
    sqlx::query("UPDATE mail_threads SET folder = 'archive' WHERE id = $1")
        .bind(&thread.id)
        .execute(fixture.database.pool())
        .await
        .unwrap();
    fixture
        .deliver(&mail(
            "Ann <ann@sender.test>",
            "second@sender.test",
            "In-Reply-To: <first@sender.test>\r\n",
            "Re: Invoice for October",
            "Any news?",
        ))
        .await;
    assert_eq!(fixture.threads().await[0].folder, "inbox");

    let now = TimestampMillis::now().as_millis();
    sqlx::query("UPDATE mail_threads SET folder = 'trash', trashed_at = $1 WHERE id = $2")
        .bind(now)
        .bind(&thread.id)
        .execute(fixture.database.pool())
        .await
        .unwrap();
    fixture
        .deliver(&mail(
            "Ann <ann@sender.test>",
            "third@sender.test",
            "In-Reply-To: <second@sender.test>\r\n",
            "Re: Invoice for October",
            "Hello?",
        ))
        .await;
    let threads = fixture.threads().await;
    assert_eq!(threads.len(), 1);
    assert_eq!(threads[0].folder, "trash");
    assert_eq!(threads[0].count, 3);
}

#[tokio::test]
async fn unparsable_mail_is_kept_as_plain_text_with_a_warning() {
    let fixture = Fixture::new().await;
    // Over the header count cap.
    let mut bomb = String::from("From: ann@sender.test\r\nSubject: Bomb\r\n");
    for index in 0..1_001 {
        bomb.push_str(&format!("X-Filler-{index}: value\r\n"));
    }
    bomb.push_str("\r\nbody text\0 with a NUL\r\n");
    let limited = fixture.deliver(bomb.as_bytes()).await;
    // Not a message at all.
    let empty = fixture.deliver(b"").await;

    for (receipt, warning) in [(limited, "limit"), (empty, "parse_failed")] {
        let (stored, body, subject): (Option<String>, String, String) = sqlx::query_as(
            "SELECT warning, body_text, subject FROM mail_messages WHERE receipt_id = $1",
        )
        .bind(receipt.to_string())
        .fetch_one(fixture.database.pool())
        .await
        .unwrap();
        assert_eq!(stored.as_deref(), Some(warning));
        assert_eq!(subject, "");
        if receipt == limited {
            assert!(
                body.contains("X-Filler-1000: value"),
                "raw text is the body"
            );
            assert!(body.contains("body text with a NUL"), "NUL is dropped");
        }
        assert_eq!(fixture.receipt_status(receipt).await.0, "ingested");
    }
    assert_eq!(fixture.threads().await.len(), 2);
}

#[tokio::test]
async fn exhausted_retries_fail_the_receipt_and_retry_ingests_it() {
    let fixture = Fixture::new().await;
    let receipt = fixture
        .receive(&first(), &[fixture.primary], DmarcVerdict::Pass)
        .await;
    let payload = json!({ "receipt_id": receipt.to_string() });
    fixture.fail_open.store(true, Ordering::SeqCst);

    let error = run_ingest_job(&fixture.mail, &payload, 1, 3)
        .await
        .unwrap_err();
    assert!(matches!(error, JobError::Retryable(_)), "{error:?}");
    let (status, attempts, message) = fixture.receipt_status(receipt).await;
    assert_eq!((status.as_str(), attempts), ("pending", 1));
    assert!(message.is_some());

    let error = run_ingest_job(&fixture.mail, &payload, 3, 3)
        .await
        .unwrap_err();
    assert!(matches!(error, JobError::Retryable(_)), "{error:?}");
    let (status, attempts, message) = fixture.receipt_status(receipt).await;
    assert_eq!((status.as_str(), attempts), ("failed", 2));
    assert!(message.unwrap().contains("blob"), "the error is kept");
    assert!(fixture.threads().await.is_empty());

    // A member may not retry; an admin may, once the store is back.
    let (member, _) = fixture.member("Member").await;
    let refused = fixture
        .mail
        .retry_receipt(fixture.workspace, member, receipt, &fixture.jobs)
        .await
        .unwrap_err();
    assert!(matches!(refused, MailError::Forbidden(_)), "{refused:?}");
    fixture.fail_open.store(false, Ordering::SeqCst);
    let queued_before = fixture
        .count(
            "SELECT COUNT(*) FROM jobs WHERE kind = $1 AND state = 'queued'",
            "mail.ingest",
        )
        .await;
    fixture
        .mail
        .retry_receipt(fixture.workspace, fixture.owner, receipt, &fixture.jobs)
        .await
        .unwrap();
    let (status, _, message) = fixture.receipt_status(receipt).await;
    assert_eq!(status, "pending");
    assert!(message.is_none());
    assert_eq!(
        fixture
            .count(
                "SELECT COUNT(*) FROM jobs WHERE kind = $1 AND state = 'queued'",
                "mail.ingest",
            )
            .await,
        queued_before + 1
    );
    // Only a failed receipt can be retried.
    run_ingest_job(&fixture.mail, &payload, 1, 3).await.unwrap();
    assert_eq!(fixture.receipt_status(receipt).await.0, "ingested");
    assert_eq!(fixture.threads().await.len(), 1);
    let again = fixture
        .mail
        .retry_receipt(fixture.workspace, fixture.owner, receipt, &fixture.jobs)
        .await
        .unwrap_err();
    assert!(matches!(again, MailError::Conflict(_)), "{again:?}");
}

#[tokio::test]
async fn purge_removes_old_trash_and_frees_the_blob() {
    let fixture = Fixture::new().await;
    let old = fixture.deliver(&first()).await;
    let kept = fixture
        .deliver(&mail(
            "Bob <bob@other.test>",
            "other@other.test",
            "",
            "Lunch",
            "Noon?",
        ))
        .await;
    let threads = fixture.threads().await;
    let now = TimestampMillis::now().as_millis();
    // The first thread was trashed 31 days ago, the second one a day ago.
    for (thread, trashed_at) in [
        (&threads[0], now - TRASH_RETENTION_MS - DAY),
        (&threads[1], now - DAY),
    ] {
        sqlx::query("UPDATE mail_threads SET folder = 'trash', trashed_at = $1 WHERE id = $2")
            .bind(trashed_at)
            .bind(&thread.id)
            .execute(fixture.database.pool())
            .await
            .unwrap();
    }

    let purged = fixture
        .mail
        .purge(TimestampMillis::from_millis(now))
        .await
        .unwrap();
    assert_eq!(
        (purged.threads, purged.messages, purged.receipts),
        (1, 1, 1)
    );
    let remaining = fixture.threads().await;
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, threads[1].id);
    assert_eq!(fixture.search_rows(threads[0].row_id).await, 0);
    for (receipt, expected) in [(old, 0), (kept, 1)] {
        assert_eq!(
            fixture
                .count(
                    "SELECT COUNT(*) FROM mail_receipts WHERE id = $1",
                    &receipt.to_string()
                )
                .await,
            expected
        );
    }

    // The blob of the purged receipt is now unreferenced: the reconcile removes it.
    let blobs_before = fixture
        .count(
            "SELECT COUNT(*) FROM attachment_blobs WHERE workspace_id = $1",
            &fixture.workspace.to_string(),
        )
        .await;
    let result = fixture
        .mail
        .uploads()
        .reconcile(now + 3 * DAY)
        .await
        .unwrap();
    assert_eq!(result.deleted_blobs, 1);
    assert_eq!(
        fixture
            .count(
                "SELECT COUNT(*) FROM attachment_blobs WHERE workspace_id = $1",
                &fixture.workspace.to_string(),
            )
            .await,
        blobs_before - 1
    );

    // A second run finds nothing.
    let purged = fixture
        .mail
        .purge(TimestampMillis::from_millis(now))
        .await
        .unwrap();
    assert_eq!(
        (purged.threads, purged.messages, purged.receipts),
        (0, 0, 0)
    );
}

#[tokio::test]
async fn only_mailbox_members_get_the_live_event() {
    let fixture = Fixture::new().await;
    fixture.deliver(&first()).await;
    let thread = fixture.threads().await.remove(0);

    let events = fixture.live_events().await;
    assert_eq!(events.len(), 1, "{events:?}");
    let (recipients, event) = &events[0];
    assert_eq!(
        recipients,
        &json!({ "to": "users", "users": [fixture.owner.to_string()] })
    );
    assert!(
        !recipients
            .to_string()
            .contains(&fixture.outsider.to_string()),
        "a workspace member outside the mailbox gets nothing"
    );
    assert_eq!(
        event,
        &json!({
            "type": "thread.updated",
            "mailbox_id": fixture.mailbox.to_string(),
            "thread_id": thread.id,
        })
    );
}
