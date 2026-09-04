use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use thiserror::Error;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use super::{
    Claim, ClaimSelection, DEFAULT_HEARTBEAT, Job, JobError, JobKind, JobPriority, JobStore,
    JobStoreError,
};
use crate::TimestampMillis;

type HandlerFuture = Pin<Box<dyn Future<Output = Result<(), JobError>> + Send>>;
type HandlerFn = Arc<dyn Fn(JobContext) -> HandlerFuture + Send + Sync>;

#[derive(Clone, Debug)]
pub struct JobContext {
    pub job: Job,
    pub claim: Claim,
    cancellation: CancellationToken,
}

impl JobContext {
    pub async fn cancelled(&self) {
        self.cancellation.cancelled().await;
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancellation.is_cancelled()
    }
}

#[derive(Clone, Debug)]
pub struct WorkerConfig {
    concurrency: usize,
    poll_interval: Duration,
    drain_timeout: Duration,
    heartbeat_interval: Duration,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Error)]
pub enum WorkerConfigError {
    #[error("worker concurrency must be positive")]
    ZeroConcurrency,
}

impl WorkerConfig {
    pub fn new(concurrency: usize) -> Result<Self, WorkerConfigError> {
        if concurrency == 0 {
            return Err(WorkerConfigError::ZeroConcurrency);
        }
        Ok(Self {
            concurrency,
            poll_interval: Duration::from_millis(100),
            drain_timeout: Duration::from_secs(30),
            heartbeat_interval: DEFAULT_HEARTBEAT,
        })
    }

    #[must_use]
    pub fn with_poll_interval(mut self, interval: Duration) -> Self {
        self.poll_interval = interval;
        self
    }

    #[must_use]
    pub fn with_drain_timeout(mut self, timeout: Duration) -> Self {
        self.drain_timeout = timeout;
        self
    }

    #[must_use]
    pub fn with_heartbeat_interval(mut self, interval: Duration) -> Self {
        self.heartbeat_interval = interval;
        self
    }
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self::new(4).expect("default worker concurrency is positive")
    }
}

#[derive(Clone)]
struct RegisteredHandler {
    kind: JobKind,
    run: HandlerFn,
}

pub struct Worker {
    store: JobStore,
    config: WorkerConfig,
    handlers: BTreeMap<String, RegisteredHandler>,
}

impl Worker {
    #[must_use]
    pub fn new(store: JobStore, config: WorkerConfig) -> Self {
        Self {
            store,
            config,
            handlers: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn with_handler<F, Fut>(mut self, kind: JobKind, handler: F) -> Self
    where
        F: Fn(JobContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), JobError>> + Send + 'static,
    {
        let run: HandlerFn = Arc::new(move |context| Box::pin(handler(context)));
        self.handlers
            .insert(kind.name().to_owned(), RegisteredHandler { kind, run });
        self
    }

    pub async fn run(self, shutdown: CancellationToken) -> Result<(), JobStoreError> {
        let mut tasks: JoinSet<(String, JobPriority, Result<(), JobStoreError>)> = JoinSet::new();
        let mut active_by_kind = BTreeMap::<String, usize>::new();
        let mut active_noncritical = 0usize;
        let mut first_error = None;

        loop {
            while let Some(result) = tasks.try_join_next() {
                if let Ok((kind, priority, result)) = result {
                    decrement(&mut active_by_kind, &kind);
                    if priority != JobPriority::Critical {
                        active_noncritical = active_noncritical.saturating_sub(1);
                    }
                    if let Err(error) = result {
                        first_error.get_or_insert(error);
                    }
                }
            }
            if shutdown.is_cancelled() {
                break;
            }

            let excluded = excluded_kinds(&self.handlers, &active_by_kind);
            let mut claimed = false;
            if tasks.len() < self.config.concurrency
                && let Some(claim) = self
                    .store
                    .claim_excluding(TimestampMillis::now(), ClaimSelection::Critical, &excluded)
                    .await?
            {
                self.spawn_claim(
                    &mut tasks,
                    &mut active_by_kind,
                    &mut active_noncritical,
                    claim,
                    shutdown.clone(),
                )
                .await?;
                claimed = true;
            }

            let noncritical_slots = if self.config.concurrency == 1 {
                1
            } else {
                self.config.concurrency - 1
            };
            if !claimed
                && tasks.len() < self.config.concurrency
                && active_noncritical < noncritical_slots
            {
                let excluded = excluded_kinds(&self.handlers, &active_by_kind);
                if let Some(claim) = self
                    .store
                    .claim_excluding(
                        TimestampMillis::now(),
                        ClaimSelection::NonCritical,
                        &excluded,
                    )
                    .await?
                {
                    self.spawn_claim(
                        &mut tasks,
                        &mut active_by_kind,
                        &mut active_noncritical,
                        claim,
                        shutdown.clone(),
                    )
                    .await?;
                    claimed = true;
                }
            }
            if claimed {
                continue;
            }

            tokio::select! {
                () = shutdown.cancelled() => break,
                () = tokio::time::sleep(self.config.poll_interval) => {},
            }
        }

        let drain = async {
            while let Some(result) = tasks.join_next().await {
                if let Ok((_kind, _priority, Err(error))) = result {
                    first_error.get_or_insert(error);
                }
            }
        };
        if tokio::time::timeout(self.config.drain_timeout, drain)
            .await
            .is_err()
        {
            tasks.abort_all();
            while tasks.join_next().await.is_some() {}
        }

        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    async fn spawn_claim(
        &self,
        tasks: &mut JoinSet<(String, JobPriority, Result<(), JobStoreError>)>,
        active_by_kind: &mut BTreeMap<String, usize>,
        active_noncritical: &mut usize,
        claim: Claim,
        shutdown: CancellationToken,
    ) -> Result<(), JobStoreError> {
        let job = self
            .store
            .get(claim.job_id)
            .await?
            .ok_or(JobStoreError::NotFound(claim.job_id))?;
        let kind_name = job.kind.name().to_owned();
        let priority = job.priority;
        *active_by_kind.entry(kind_name.clone()).or_default() += 1;
        if priority != JobPriority::Critical {
            *active_noncritical += 1;
        }
        let handler = self.handlers.get(&kind_name).cloned();
        let store = self.store.clone();
        let heartbeat_interval = self.config.heartbeat_interval;
        tasks.spawn(async move {
            let result =
                execute_claim(store, job, claim, handler, heartbeat_interval, shutdown).await;
            (kind_name, priority, result)
        });
        Ok(())
    }
}

fn excluded_kinds(
    handlers: &BTreeMap<String, RegisteredHandler>,
    active: &BTreeMap<String, usize>,
) -> Vec<String> {
    handlers
        .iter()
        .filter_map(|(name, handler)| {
            handler
                .kind
                .concurrency_limit()
                .filter(|limit| active.get(name).copied().unwrap_or(0) >= *limit)
                .map(|_| name.clone())
        })
        .collect()
}

fn decrement(counts: &mut BTreeMap<String, usize>, kind: &str) {
    if let Some(count) = counts.get_mut(kind) {
        *count = count.saturating_sub(1);
    }
}

async fn execute_claim(
    store: JobStore,
    mut job: Job,
    claim: Claim,
    handler: Option<RegisteredHandler>,
    heartbeat_interval: Duration,
    shutdown: CancellationToken,
) -> Result<(), JobStoreError> {
    let cancellation = CancellationToken::new();
    let (kind, run) = match handler {
        Some(handler) => (handler.kind, Some(handler.run)),
        None => (job.kind.clone(), None),
    };
    job.kind = kind.clone();
    let context = JobContext {
        job,
        claim: claim.clone(),
        cancellation: cancellation.clone(),
    };
    let mut future: HandlerFuture = match run {
        Some(run) => run(context),
        None => Box::pin(async {
            Err(JobError::Permanent(
                "no handler is registered for this job kind".to_owned(),
            ))
        }),
    };
    store
        .heartbeat_with_lease(&claim, TimestampMillis::now(), kind.lease())
        .await?;
    let mut heartbeat = tokio::time::interval(heartbeat_interval);
    heartbeat.tick().await;
    let mut shutting_down = false;

    loop {
        tokio::select! {
            result = &mut future => {
                let now = TimestampMillis::now();
                match result {
                    Ok(()) => { store.complete(&claim, now).await?; }
                    Err(_) if shutting_down => {}
                    Err(error) => { store.fail_with_kind(&claim, error, now, &kind).await?; }
                }
                return Ok(());
            }
            _ = heartbeat.tick() => {
                if !store.heartbeat_with_lease(&claim, TimestampMillis::now(), kind.lease()).await? {
                    return Ok(());
                }
            }
            () = shutdown.cancelled(), if !shutting_down => {
                shutting_down = true;
                cancellation.cancel();
            }
        }
    }
}
