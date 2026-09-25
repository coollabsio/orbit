use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use orbit_platform::{
    CatchUpMode, ClaimSelection, CronSchedule, Job, JobError, JobKind, JobKindRegistry,
    JobPriority, JobQueue, JobState, JobStore, RecurringSchedule, Scheduler, TestDatabase,
    TimestampMillis, Worker, WorkerConfig,
};
use serde_json::json;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

const MINUTE: i64 = 60_000;
const FIVE_MINUTES: i64 = 5 * MINUTE;
const DAY: i64 = 24 * 60 * MINUTE;

async fn store() -> (TestDatabase, JobStore) {
    let database = TestDatabase::new().await.unwrap();
    let store = JobStore::new((*database).clone());
    (database, store)
}

fn job(kind: &str, priority: JobPriority, now: i64) -> Job {
    Job::new(
        JobKind::new(kind),
        json!({"message": kind}),
        TimestampMillis::from_millis(now),
    )
    .with_priority(priority)
}

#[tokio::test]
async fn claims_priority_first_and_fifo_within_priority() {
    let (_database, store) = store().await;
    let queue = JobQueue::new(store.clone());
    let first_normal = queue
        .enqueue(job("first", JobPriority::Normal, 1_000))
        .await
        .unwrap();
    let high = queue
        .enqueue(job("high", JobPriority::High, 1_000))
        .await
        .unwrap();
    let second_normal = queue
        .enqueue(job("second", JobPriority::Normal, 1_000))
        .await
        .unwrap();

    let now = TimestampMillis::from_millis(1_000);
    let claims = [
        store
            .claim(now, ClaimSelection::Any)
            .await
            .unwrap()
            .unwrap(),
        store
            .claim(now, ClaimSelection::Any)
            .await
            .unwrap()
            .unwrap(),
        store
            .claim(now, ClaimSelection::Any)
            .await
            .unwrap()
            .unwrap(),
    ];

    assert_eq!(claims[0].job_id, high);
    assert_eq!(claims[1].job_id, first_normal);
    assert_eq!(claims[2].job_id, second_normal);
}

#[tokio::test]
async fn worker_keeps_a_slot_available_for_critical_work() {
    let (_database, store) = store().await;
    let queue = JobQueue::new(store.clone());
    queue
        .enqueue(job("normal", JobPriority::Normal, 0))
        .await
        .unwrap();

    let normal_started = Arc::new(Notify::new());
    let normal_started_in_handler = normal_started.clone();
    let critical_started = Arc::new(Notify::new());
    let critical_started_in_handler = critical_started.clone();
    let worker = Worker::new(
        store.clone(),
        WorkerConfig::new(2)
            .unwrap()
            .with_poll_interval(Duration::from_millis(5))
            .with_drain_timeout(Duration::from_millis(20)),
    )
    .with_handler(JobKind::new("normal"), move |context| {
        let started = normal_started_in_handler.clone();
        async move {
            started.notify_one();
            context.cancelled().await;
            Err(JobError::Retryable("cancelled".to_owned()))
        }
    })
    .unwrap()
    .with_handler(JobKind::new("critical"), move |_| {
        let started = critical_started_in_handler.clone();
        async move {
            started.notify_one();
            Ok(())
        }
    })
    .unwrap();
    let shutdown = CancellationToken::new();
    let worker_task = tokio::spawn(worker.run(shutdown.clone()));

    tokio::time::timeout(Duration::from_secs(1), normal_started.notified())
        .await
        .unwrap();
    queue
        .enqueue(job("critical", JobPriority::Critical, 0))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(1), critical_started.notified())
        .await
        .expect("critical work must start while normal work is active");

    shutdown.cancel();
    worker_task.await.unwrap().unwrap();
}

#[tokio::test]
async fn retryable_failures_use_all_eight_attempts_and_positive_bounded_jitter() {
    let (_database, store) = store().await;
    let id = JobQueue::new(store.clone())
        .enqueue(job("retry", JobPriority::Normal, 0))
        .await
        .unwrap();
    let base_delays = [10, 30, 120, 600, 3_600, 21_600, 86_400];
    let mut now = TimestampMillis::from_millis(0);

    for (index, base_seconds) in base_delays.into_iter().enumerate() {
        let claim = store
            .claim(now, ClaimSelection::Any)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(claim.attempt, index as u32 + 1);
        assert!(
            store
                .fail(&claim, JobError::Retryable("temporary".to_owned()), now,)
                .await
                .unwrap()
        );
        let queued = store.get(id).await.unwrap().unwrap();
        let delay = queued.available_at.as_millis() - now.as_millis();
        assert!(delay >= base_seconds * 1_000);
        assert!(delay <= base_seconds * 1_200);
        now = queued.available_at;
    }

    let final_claim = store
        .claim(now, ClaimSelection::Any)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(final_claim.attempt, 8);
    store
        .fail(
            &final_claim,
            JobError::Retryable("still broken".to_owned()),
            now,
        )
        .await
        .unwrap();
    assert_eq!(store.get(id).await.unwrap().unwrap().state, JobState::Dead);
}

#[tokio::test]
async fn permanent_failure_moves_directly_to_dead_state() {
    let (_database, store) = store().await;
    let id = JobQueue::new(store.clone())
        .enqueue(job("invalid", JobPriority::Normal, 0))
        .await
        .unwrap();
    let claim = store
        .claim(TimestampMillis::from_millis(0), ClaimSelection::Any)
        .await
        .unwrap()
        .unwrap();

    store
        .fail(
            &claim,
            JobError::Permanent("invalid configuration".to_owned()),
            TimestampMillis::from_millis(1),
        )
        .await
        .unwrap();

    assert_eq!(store.get(id).await.unwrap().unwrap().state, JobState::Dead);
}

#[tokio::test]
async fn expired_lease_is_recovered_as_a_new_attempt() {
    let (_database, store) = store().await;
    JobQueue::new(store.clone())
        .enqueue(job("recover", JobPriority::Normal, 0))
        .await
        .unwrap();
    let now = TimestampMillis::from_millis(0);
    let first = store
        .claim(now, ClaimSelection::Any)
        .await
        .unwrap()
        .unwrap();
    let second = store
        .claim(
            TimestampMillis::from_millis(now.as_millis() + FIVE_MINUTES),
            ClaimSelection::Any,
        )
        .await
        .unwrap()
        .unwrap();

    assert_eq!(first.attempt, 1);
    assert_eq!(second.attempt, 2);
    assert_ne!(first.claim_token, second.claim_token);
}

#[tokio::test]
async fn an_expired_final_attempt_is_dead_instead_of_claimed_a_ninth_time() {
    let (_database, store) = store().await;
    let limited = Job::new(
        JobKind::new("limited").with_retry_policy(1, Vec::new()),
        json!({}),
        TimestampMillis::from_millis(0),
    );
    let id = JobQueue::new(store.clone()).enqueue(limited).await.unwrap();
    store
        .claim(TimestampMillis::from_millis(0), ClaimSelection::Any)
        .await
        .unwrap()
        .unwrap();

    assert!(
        store
            .claim(
                TimestampMillis::from_millis(FIVE_MINUTES),
                ClaimSelection::Any,
            )
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(store.get(id).await.unwrap().unwrap().state, JobState::Dead);
}

#[tokio::test]
async fn stale_claim_cannot_heartbeat_or_complete_reclaimed_job() {
    let (_database, store) = store().await;
    JobQueue::new(store.clone())
        .enqueue(job("recover", JobPriority::Normal, 0))
        .await
        .unwrap();
    let first = store
        .claim(TimestampMillis::from_millis(0), ClaimSelection::Any)
        .await
        .unwrap()
        .unwrap();
    let second = store
        .claim(
            TimestampMillis::from_millis(FIVE_MINUTES),
            ClaimSelection::Any,
        )
        .await
        .unwrap()
        .unwrap();

    assert!(
        !store
            .heartbeat(&first, TimestampMillis::from_millis(FIVE_MINUTES + 1))
            .await
            .unwrap()
    );
    assert!(
        !store
            .complete(&first, TimestampMillis::from_millis(FIVE_MINUTES + 1))
            .await
            .unwrap()
    );
    assert!(
        store
            .complete(&second, TimestampMillis::from_millis(FIVE_MINUTES + 1))
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn manual_retry_links_a_new_job_without_changing_the_dead_job() {
    let (_database, store) = store().await;
    let original = JobQueue::new(store.clone())
        .enqueue(job("invalid", JobPriority::Normal, 0))
        .await
        .unwrap();
    let claim = store
        .claim(TimestampMillis::from_millis(0), ClaimSelection::Any)
        .await
        .unwrap()
        .unwrap();
    store
        .fail(
            &claim,
            JobError::Permanent("bad input".to_owned()),
            TimestampMillis::from_millis(1),
        )
        .await
        .unwrap();

    let retry = store
        .manual_retry(original, TimestampMillis::from_millis(2))
        .await
        .unwrap();

    let retried = store.get(retry).await.unwrap().unwrap();
    assert_eq!(retried.manual_retry_of, Some(original));
    assert_eq!(retried.state, JobState::Queued);
    assert_eq!(
        store.get(original).await.unwrap().unwrap().state,
        JobState::Dead
    );
}

#[tokio::test]
async fn purge_removes_only_expired_dead_jobs_without_incident_holds() {
    let (database, store) = store().await;
    let expired = JobQueue::new(store.clone())
        .enqueue(job("expired", JobPriority::Normal, 0))
        .await
        .unwrap();
    let held = JobQueue::new(store.clone())
        .enqueue(job("held", JobPriority::Normal, 0))
        .await
        .unwrap();
    for id in [expired, held] {
        let claim = store
            .claim(TimestampMillis::from_millis(0), ClaimSelection::Any)
            .await
            .unwrap()
            .unwrap();
        store
            .fail(
                &claim,
                JobError::Permanent("dead".to_owned()),
                TimestampMillis::from_millis(1),
            )
            .await
            .unwrap();
        assert_eq!(claim.job_id, id);
    }
    database
        .execute(&format!(
            "UPDATE jobs SET incident_hold = 1 WHERE id = '{held}'"
        ))
        .await
        .unwrap();

    let purged = store
        .purge_dead(TimestampMillis::from_millis(30 * DAY + 2))
        .await
        .unwrap();

    assert_eq!(purged, 1);
    assert!(store.get(expired).await.unwrap().is_none());
    assert!(store.get(held).await.unwrap().is_some());
}

#[tokio::test]
async fn schedule_materialization_is_deduplicated() {
    let (_database, store) = store().await;
    let scheduler = Scheduler::new(store.clone());
    let schedule = RecurringSchedule::interval(
        JobKind::new("cleanup"),
        json!({}),
        Duration::from_secs(60),
        TimestampMillis::from_millis(MINUTE),
    );
    scheduler.upsert(&schedule).await.unwrap();
    assert_eq!(
        scheduler
            .materialize_due(TimestampMillis::from_millis(MINUTE))
            .await
            .unwrap()
            .len(),
        1
    );
    scheduler.upsert(&schedule).await.unwrap();

    assert!(
        scheduler
            .materialize_due(TimestampMillis::from_millis(MINUTE))
            .await
            .unwrap()
            .is_empty()
    );
}

#[test]
fn five_field_cron_is_evaluated_in_utc() {
    let cron = CronSchedule::parse("0 0 * * *").unwrap();
    let midday_utc = TimestampMillis::from_millis(1_767_268_800_000);

    assert_eq!(
        cron.next_after(midday_utc).unwrap(),
        TimestampMillis::from_millis(1_767_312_000_000)
    );
    assert!(CronSchedule::parse("0 0 0 * * *").is_err());
}

#[tokio::test]
async fn latest_catch_up_coalesces_missed_runs() {
    let (_database, store) = store().await;
    let scheduler = Scheduler::new(store.clone());
    let schedule = RecurringSchedule::interval(
        JobKind::new("minute"),
        json!({}),
        Duration::from_secs(60),
        TimestampMillis::from_millis(MINUTE),
    )
    .with_catch_up(CatchUpMode::Latest);
    scheduler.upsert(&schedule).await.unwrap();

    let jobs = scheduler
        .materialize_due(TimestampMillis::from_millis(5 * MINUTE + 30_000))
        .await
        .unwrap();

    assert_eq!(jobs.len(), 1);
    assert_eq!(
        store.get(jobs[0]).await.unwrap().unwrap().scheduled_for,
        Some(TimestampMillis::from_millis(5 * MINUTE))
    );
}

#[test]
fn diagnostics_redact_declared_sensitive_payload_fields() {
    let kind = JobKind::new("email").with_sensitive_fields(["token", "password"]);
    let payload = json!({
        "recipient": "user@example.com",
        "token": "secret-token",
        "nested": {"password": "secret-password"}
    });
    let redacted = kind.redact_payload(&payload);

    assert_eq!(redacted["recipient"], "user@example.com");
    assert_eq!(redacted["token"], "[REDACTED]");
    assert_eq!(redacted["nested"]["password"], "[REDACTED]");
    assert!(!redacted.to_string().contains("secret-"));
    let diagnostic = format!(
        "{:?}",
        Job::new(kind, payload, TimestampMillis::from_millis(0))
    );
    assert!(!diagnostic.contains("secret-token"));
    assert!(!diagnostic.contains("secret-password"));
}

#[tokio::test]
async fn worker_context_uses_the_registered_kinds_redaction_policy() {
    let (_database, store) = store().await;
    let kind = JobKind::new("diagnostic").with_sensitive_fields(["token"]);
    JobQueue::new(store.clone())
        .enqueue(Job::new(
            kind.clone(),
            json!({"token": "must-not-leak"}),
            TimestampMillis::from_millis(0),
        ))
        .await
        .unwrap();
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let sender = Arc::new(std::sync::Mutex::new(Some(sender)));
    let handler_sender = sender.clone();
    let worker = Worker::new(
        store,
        WorkerConfig::new(1)
            .unwrap()
            .with_poll_interval(Duration::from_millis(5)),
    )
    .with_handler(kind, move |context| {
        let sender = handler_sender.clone();
        async move {
            if let Some(sender) = sender.lock().unwrap().take() {
                let _ = sender.send(context.job.diagnostic_payload());
            }
            Ok(())
        }
    })
    .unwrap();
    let shutdown = CancellationToken::new();
    let task = tokio::spawn(worker.run(shutdown.clone()));

    let diagnostic = tokio::time::timeout(Duration::from_secs(1), receiver)
        .await
        .unwrap()
        .unwrap();
    shutdown.cancel();
    task.await.unwrap().unwrap();

    assert_eq!(diagnostic["token"], "[REDACTED]");
}

#[tokio::test]
async fn stored_job_debug_output_never_exposes_payload_values() {
    let (_database, store) = store().await;
    let id = JobQueue::new(store.clone())
        .enqueue(Job::new(
            JobKind::new("diagnostic").with_sensitive_fields(["token"]),
            json!({"token": "must-not-leak"}),
            TimestampMillis::from_millis(0),
        ))
        .await
        .unwrap();

    let diagnostic = format!("{:?}", store.get(id).await.unwrap().unwrap());

    assert!(!diagnostic.contains("must-not-leak"));
}

#[tokio::test]
async fn stored_job_diagnostics_keep_the_registered_sensitive_field_policy() {
    let database = TestDatabase::new().await.unwrap();
    let kinds = JobKindRegistry::new();
    let writer = JobStore::with_registry((*database).clone(), kinds.clone());
    let id = JobQueue::new(writer)
        .enqueue(Job::new(
            JobKind::new("round-trip-secret").with_sensitive_fields(["token"]),
            json!({"token": "must-not-leak", "safe": "visible"}),
            TimestampMillis::from_millis(0),
        ))
        .await
        .unwrap();

    let reader = JobStore::with_registry((*database).clone(), kinds);
    let diagnostic = reader.get(id).await.unwrap().unwrap().diagnostic_payload();

    assert_eq!(diagnostic["token"], "[REDACTED]");
    assert_eq!(diagnostic["safe"], "visible");
}

#[tokio::test]
async fn direct_claim_and_failure_use_the_canonical_custom_kind_policy() {
    let database = TestDatabase::new().await.unwrap();
    let kinds = JobKindRegistry::new();
    let writer = JobStore::with_registry((*database).clone(), kinds.clone());
    let custom = JobKind::new("custom-direct")
        .with_retry_policy(3, vec![Duration::from_secs(7), Duration::from_secs(11)])
        .with_lease(Duration::from_secs(11 * 60));
    let id = JobQueue::new(writer)
        .enqueue(Job::new(custom, json!({}), TimestampMillis::from_millis(0)))
        .await
        .unwrap();

    let store = JobStore::with_registry((*database).clone(), kinds);
    let claim = store
        .claim(TimestampMillis::from_millis(0), ClaimSelection::Any)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(claim.lease_expires_at.as_millis(), 11 * MINUTE);
    store
        .fail(
            &claim,
            JobError::Retryable("temporary".to_owned()),
            TimestampMillis::from_millis(1_000),
        )
        .await
        .unwrap();
    let queued = store.get(id).await.unwrap().unwrap();
    let delay = queued.available_at.as_millis() - 1_000;
    assert!((7_000..=8_400).contains(&delay));
    assert_eq!(queued.max_attempts, 3);
}

#[tokio::test]
async fn materialized_schedule_keeps_the_canonical_custom_kind_policy() {
    let database = TestDatabase::new().await.unwrap();
    let kinds = JobKindRegistry::new();
    let writer = JobStore::with_registry((*database).clone(), kinds.clone());
    let custom = JobKind::new("custom-scheduled")
        .with_sensitive_fields(["token"])
        .with_retry_policy(3, vec![Duration::from_secs(7), Duration::from_secs(11)])
        .with_lease(Duration::from_secs(11 * 60));
    let scheduler = Scheduler::new(writer);
    let schedule = RecurringSchedule::interval(
        custom,
        json!({"token": "must-not-leak"}),
        Duration::from_secs(60),
        TimestampMillis::from_millis(MINUTE),
    );
    scheduler.upsert(&schedule).await.unwrap();
    let store = JobStore::with_registry((*database).clone(), kinds);
    let scheduler = Scheduler::new(store.clone());
    let id = scheduler
        .materialize_due(TimestampMillis::from_millis(MINUTE))
        .await
        .unwrap()[0];

    let materialized = store.get(id).await.unwrap().unwrap();
    assert_eq!(materialized.max_attempts, 3);
    assert_eq!(materialized.diagnostic_payload()["token"], "[REDACTED]");
    let claim = store
        .claim(TimestampMillis::from_millis(MINUTE), ClaimSelection::Any)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(claim.lease_expires_at.as_millis(), 12 * MINUTE);
    store
        .fail(
            &claim,
            JobError::Retryable("temporary".to_owned()),
            TimestampMillis::from_millis(MINUTE + 1_000),
        )
        .await
        .unwrap();
    let queued = store.get(id).await.unwrap().unwrap();
    let delay = queued.available_at.as_millis() - (MINUTE + 1_000);
    assert!((7_000..=8_400).contains(&delay));
}

#[tokio::test]
async fn database_clock_drives_claim_lease_deadlines() {
    let (_database, store) = store().await;
    JobQueue::new(store.clone())
        .enqueue(job("database-clock", JobPriority::Normal, 0))
        .await
        .unwrap();
    let before = store.database_now().await.unwrap();

    let claim = store
        .claim_at_database_time(ClaimSelection::Any)
        .await
        .unwrap()
        .unwrap();
    let after = store.database_now().await.unwrap();

    assert!(
        claim.lease_expires_at.as_millis()
            >= before.as_millis() + Duration::from_secs(5 * 60).as_millis() as i64
    );
    assert!(
        claim.lease_expires_at.as_millis()
            <= after.as_millis() + Duration::from_secs(5 * 60).as_millis() as i64
    );
}

#[tokio::test]
async fn shutdown_cancels_a_pending_claim_without_starting_its_handler() {
    let (database, store) = store().await;
    JobQueue::new(store.clone())
        .enqueue(job("blocked-claim", JobPriority::Normal, 0))
        .await
        .unwrap();
    let mut write_lock = database.transaction().await.unwrap();
    sqlx::query("UPDATE jobs SET updated_at = updated_at WHERE kind = 'blocked-claim'")
        .execute(&mut *write_lock)
        .await
        .unwrap();
    let started = Arc::new(AtomicBool::new(false));
    let handler_started = started.clone();
    let worker = Worker::new(
        store,
        WorkerConfig::new(1)
            .unwrap()
            .with_poll_interval(Duration::from_millis(5)),
    )
    .with_handler(JobKind::new("blocked-claim"), move |_| {
        let started = handler_started.clone();
        async move {
            started.store(true, Ordering::SeqCst);
            Ok(())
        }
    })
    .unwrap();
    let shutdown = CancellationToken::new();
    let task = tokio::spawn(worker.run(shutdown.clone()));
    tokio::time::sleep(Duration::from_millis(30)).await;

    shutdown.cancel();
    tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .expect("worker must cancel its pending claim")
        .unwrap()
        .unwrap();
    assert!(!started.load(Ordering::SeqCst));
    write_lock.rollback().await.unwrap();
}

#[tokio::test]
async fn panicking_handler_releases_kind_concurrency_for_the_next_job() {
    let (_database, store) = store().await;
    let queue = JobQueue::new(store.clone());
    let kind = JobKind::new("panic-once").with_concurrency_limit(1);
    for _ in 0..2 {
        queue
            .enqueue(
                Job::new(kind.clone(), json!({}), TimestampMillis::from_millis(0))
                    .with_priority(JobPriority::Normal),
            )
            .await
            .unwrap();
    }
    let calls = Arc::new(AtomicUsize::new(0));
    let handler_calls = calls.clone();
    let second_started = Arc::new(Notify::new());
    let handler_second_started = second_started.clone();
    let worker = Worker::new(
        store,
        WorkerConfig::new(2)
            .unwrap()
            .with_poll_interval(Duration::from_millis(5)),
    )
    .with_handler(kind, move |_| {
        let calls = handler_calls.clone();
        let second_started = handler_second_started.clone();
        async move {
            if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                panic!("controlled handler panic");
            }
            second_started.notify_one();
            Ok(())
        }
    })
    .unwrap();
    let shutdown = CancellationToken::new();
    let task = tokio::spawn(worker.run(shutdown.clone()));

    tokio::time::timeout(Duration::from_secs(1), second_started.notified())
        .await
        .expect("the second limit-one job must run after the first panics");
    shutdown.cancel();
    let result = task.await.unwrap();

    assert!(result.is_err(), "the worker must surface the handler panic");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn sub_millisecond_intervals_are_rejected_at_construction() {
    let result = std::panic::catch_unwind(|| {
        RecurringSchedule::interval(
            JobKind::new("too-fast"),
            json!({}),
            Duration::from_nanos(1),
            TimestampMillis::from_millis(0),
        )
    });

    assert!(result.is_err());
}

#[tokio::test]
async fn conflicting_enqueue_cannot_weaken_the_canonical_kind_policy() {
    let (_database, store) = store().await;
    let strong = JobKind::new("immutable-policy")
        .with_sensitive_fields(["token"])
        .with_retry_policy(3, vec![Duration::from_secs(7), Duration::from_secs(11)])
        .with_lease(Duration::from_secs(11 * 60));
    let original = JobQueue::new(store.clone())
        .enqueue(Job::new(
            strong,
            json!({"token": "must-not-leak"}),
            TimestampMillis::from_millis(0),
        ))
        .await
        .unwrap();

    let conflict = JobQueue::new(store.clone())
        .enqueue(Job::new(
            JobKind::new("immutable-policy"),
            json!({}),
            TimestampMillis::from_millis(0),
        ))
        .await;

    assert!(matches!(
        conflict,
        Err(orbit_platform::JobStoreError::KindPolicy(_))
    ));
    assert_eq!(
        store
            .get(original)
            .await
            .unwrap()
            .unwrap()
            .diagnostic_payload()["token"],
        "[REDACTED]"
    );
    let claim = store
        .claim(TimestampMillis::from_millis(0), ClaimSelection::Any)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(claim.lease_expires_at.as_millis(), 11 * MINUTE);
    store
        .fail(
            &claim,
            JobError::Retryable("temporary".to_owned()),
            TimestampMillis::from_millis(1_000),
        )
        .await
        .unwrap();
    let delay = store
        .get(original)
        .await
        .unwrap()
        .unwrap()
        .available_at
        .as_millis()
        - 1_000;
    assert!((7_000..=8_400).contains(&delay));
}

#[tokio::test]
async fn schedule_upsert_propagates_a_conflicting_kind_policy() {
    let (_database, store) = store().await;
    store
        .register_kind(
            JobKind::new("schedule-policy")
                .with_sensitive_fields(["token"])
                .with_lease(Duration::from_secs(11 * 60)),
        )
        .unwrap();
    let schedule = RecurringSchedule::interval(
        JobKind::new("schedule-policy"),
        json!({}),
        Duration::from_secs(60),
        TimestampMillis::from_millis(MINUTE),
    );

    assert!(matches!(
        Scheduler::new(store).upsert(&schedule).await,
        Err(orbit_platform::ScheduleError::KindPolicy(_))
    ));
}

#[tokio::test]
async fn handler_registration_propagates_a_conflicting_kind_policy() {
    let (_database, store) = store().await;
    store
        .register_kind(
            JobKind::new("handler-policy")
                .with_sensitive_fields(["token"])
                .with_lease(Duration::from_secs(11 * 60)),
        )
        .unwrap();

    let worker = Worker::new(store, WorkerConfig::default())
        .with_handler(JobKind::new("handler-policy"), |_| async { Ok(()) });

    assert!(matches!(
        worker,
        Err(orbit_platform::JobKindRegistrationError { name }) if name == "handler-policy"
    ));
}

#[tokio::test]
async fn identical_kind_policy_registration_is_idempotent() {
    let (_database, store) = store().await;
    let kind = JobKind::new("same-policy")
        .with_sensitive_fields(["token"])
        .with_lease(Duration::from_secs(11 * 60));

    store.register_kind(kind.clone()).unwrap();
    store.register_kind(kind).unwrap();
}

#[tokio::test]
async fn graceful_shutdown_requeues_without_spending_attempts() {
    let (database, store) = store().await;
    let kind = JobKind::new("interruptible").with_retry_policy(2, vec![Duration::ZERO]);
    let id = JobQueue::new(store.clone())
        .enqueue(Job::new(
            kind.clone(),
            json!({}),
            TimestampMillis::from_millis(0),
        ))
        .await
        .unwrap();
    // More shutdowns than the job has attempts: none of them may count.
    for _ in 0..4 {
        let started = Arc::new(Notify::new());
        let handler_started = started.clone();
        let worker = Worker::new(
            store.clone(),
            WorkerConfig::new(1)
                .unwrap()
                .with_poll_interval(Duration::from_millis(5)),
        )
        .with_handler(kind.clone(), move |context| {
            let started = handler_started.clone();
            async move {
                started.notify_one();
                context.cancelled().await;
                Err(JobError::Retryable("interrupted by shutdown".to_owned()))
            }
        })
        .unwrap();
        let shutdown = CancellationToken::new();
        let task = tokio::spawn(worker.run(shutdown.clone()));
        tokio::time::timeout(Duration::from_secs(2), started.notified())
            .await
            .expect("handler starts");
        shutdown.cancel();
        task.await.unwrap().unwrap();

        let stored = store.get(id).await.unwrap().unwrap();
        assert_eq!(stored.state, JobState::Queued);
        assert_eq!(stored.attempt_count, 0);
        let attempts: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM job_attempts WHERE job_id = ?")
                .bind(id.to_string())
                .fetch_one(database.pool())
                .await
                .unwrap();
        assert_eq!(attempts, 0);
    }
}
