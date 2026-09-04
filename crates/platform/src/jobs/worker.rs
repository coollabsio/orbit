use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use thiserror::Error;
use tokio::task::{Id as TaskId, JoinError, JoinSet};
use tokio_util::sync::CancellationToken;

use super::{
    Claim, ClaimSelection, DEFAULT_HEARTBEAT, Job, JobError, JobKind, JobKindRegistrationError,
    JobPriority, JobStore, JobStoreError,
};

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

#[derive(Debug, Error)]
pub enum WorkerError {
    #[error(transparent)]
    Store(#[from] JobStoreError),
    #[error("job handler panicked for kind {kind}")]
    HandlerPanicked { kind: String },
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

    pub fn with_handler<F, Fut>(
        mut self,
        kind: JobKind,
        handler: F,
    ) -> Result<Self, JobKindRegistrationError>
    where
        F: Fn(JobContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), JobError>> + Send + 'static,
    {
        let run: HandlerFn = Arc::new(move |context| Box::pin(handler(context)));
        self.store.register_kind(kind.clone())?;
        self.handlers
            .insert(kind.name().to_owned(), RegisteredHandler { kind, run });
        Ok(self)
    }

    pub async fn run(self, shutdown: CancellationToken) -> Result<(), WorkerError> {
        let mut tasks: JoinSet<Result<(), JobStoreError>> = JoinSet::new();
        let mut task_metadata = HashMap::<TaskId, TaskMetadata>::new();
        let mut active_by_kind = BTreeMap::<String, usize>::new();
        let mut active_noncritical = 0usize;
        let mut first_error = None;

        loop {
            while let Some(result) = tasks.try_join_next_with_id() {
                handle_join(
                    result,
                    &mut task_metadata,
                    &mut active_by_kind,
                    &mut active_noncritical,
                    &mut first_error,
                );
            }
            if shutdown.is_cancelled() {
                break;
            }

            let excluded = excluded_kinds(&self.handlers, &active_by_kind);
            let mut claimed = false;
            if tasks.len() < self.config.concurrency {
                let claim = tokio::select! {
                    biased;
                    () = shutdown.cancelled() => break,
                    result = self.store.claim_excluding_at_database_time(
                        ClaimSelection::Critical,
                        &excluded,
                    ) => result?,
                };
                if let Some(claim) = claim {
                    tokio::select! {
                        biased;
                        () = shutdown.cancelled() => break,
                        result = self.spawn_claim(
                            &mut tasks,
                            &mut task_metadata,
                            &mut active_by_kind,
                            &mut active_noncritical,
                            claim,
                            shutdown.clone(),
                        ) => result?,
                    }
                    claimed = true;
                }
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
                let claim = tokio::select! {
                    biased;
                    () = shutdown.cancelled() => break,
                    result = self.store.claim_excluding_at_database_time(
                        ClaimSelection::NonCritical,
                        &excluded,
                    ) => result?,
                };
                if let Some(claim) = claim {
                    tokio::select! {
                        biased;
                        () = shutdown.cancelled() => break,
                        result = self.spawn_claim(
                            &mut tasks,
                            &mut task_metadata,
                            &mut active_by_kind,
                            &mut active_noncritical,
                            claim,
                            shutdown.clone(),
                        ) => result?,
                    }
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
            while let Some(result) = tasks.join_next_with_id().await {
                handle_join(
                    result,
                    &mut task_metadata,
                    &mut active_by_kind,
                    &mut active_noncritical,
                    &mut first_error,
                );
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
        tasks: &mut JoinSet<Result<(), JobStoreError>>,
        task_metadata: &mut HashMap<TaskId, TaskMetadata>,
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
        let task = tasks.spawn(execute_claim(
            store,
            job,
            claim,
            handler,
            heartbeat_interval,
            shutdown,
        ));
        task_metadata.insert(
            task.id(),
            TaskMetadata {
                kind: kind_name,
                priority,
            },
        );
        Ok(())
    }
}

#[derive(Debug)]
struct TaskMetadata {
    kind: String,
    priority: JobPriority,
}

fn handle_join(
    result: Result<(TaskId, Result<(), JobStoreError>), JoinError>,
    metadata: &mut HashMap<TaskId, TaskMetadata>,
    active_by_kind: &mut BTreeMap<String, usize>,
    active_noncritical: &mut usize,
    first_error: &mut Option<WorkerError>,
) {
    let (task_id, task_result, panicked) = match result {
        Ok((task_id, result)) => (task_id, Some(result), false),
        Err(error) => (error.id(), None, error.is_panic()),
    };
    let Some(metadata) = metadata.remove(&task_id) else {
        return;
    };
    decrement(active_by_kind, &metadata.kind);
    if metadata.priority != JobPriority::Critical {
        *active_noncritical = active_noncritical.saturating_sub(1);
    }
    if panicked {
        tracing::error!(job_kind = metadata.kind, "job handler panicked");
        first_error.get_or_insert(WorkerError::HandlerPanicked {
            kind: metadata.kind,
        });
    } else if let Some(Err(error)) = task_result {
        first_error.get_or_insert(WorkerError::Store(error));
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
    let mut heartbeat = tokio::time::interval(heartbeat_interval);
    heartbeat.tick().await;
    let mut shutting_down = false;

    loop {
        tokio::select! {
            result = &mut future => {
                let now = store.database_now().await?;
                match result {
                    Ok(()) => { store.complete(&claim, now).await?; }
                    Err(_) if shutting_down => {}
                    Err(error) => { store.fail(&claim, error, now).await?; }
                }
                return Ok(());
            }
            _ = heartbeat.tick() => {
                let now = store.database_now().await?;
                if !store.heartbeat(&claim, now).await? {
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
