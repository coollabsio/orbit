use std::fmt;
use std::fs::{self, OpenOptions};
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use orbit_platform::{
    BackupService, Config, Database, DatabaseConfig, HealthCheck, HealthRegistry, HttpLimits,
    IntegrityService, JobError, JobKind, JobStore, LocalBlobStore, MigrationRunner, OriginPolicy,
    RecurringSchedule, Scheduler, TimestampMillis, UploadLimits, UploadService, Worker,
    WorkerConfig, run_guarded_migrations,
};
use serde_json::json;
use thiserror::Error;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::attachment_routes::AttachmentState;
use crate::auth_routes::{CookieMode, initialize_auth};
use crate::integration_routes::IntegrationState;
use crate::metrics::Metrics;
use crate::repositories::api_tokens::ApiTokenRepository;
use crate::repositories::identity::IdentityRepository;
use crate::repositories::tasks::TaskRepository;
use crate::repositories::workspaces::WorkspaceRepository;
use crate::router::{ApiRoutes, production_router};
use crate::static_assets::{FRONTEND_REVISION, StaticAssetError, StaticAssets};
use crate::task_routes::TaskState;
use crate::workspace_routes::WorkspaceState;

const BACKGROUND_DRAIN: Duration = Duration::from_secs(30);

pub struct App {
    config: Config,
    database: Database,
    router: Router,
    setup_url: Option<String>,
    attachments: AttachmentState,
    metrics: Metrics,
    backups: BackupService,
    workspaces: Arc<WorkspaceRepository>,
    production_services: Option<ProductionServices>,
}

struct ProductionServices {
    worker: Worker,
    scheduler: Scheduler,
    integrity_failure: CancellationToken,
}

impl fmt::Debug for App {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("App")
            .field("listen", &self.config.http.listen_addr())
            .field("database", &self.config.data.database)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Error)]
pub enum AppError {
    #[error("invalid configuration: {0}")]
    Config(String),
    #[error("database initialization failed: {0}")]
    Database(String),
    #[error("migration failed before listener bind: {0}")]
    Migration(String),
    #[error("pre-migration backup failed before listener bind: {0}")]
    PreMigrationBackup(String),
    #[error("database integrity check failed before listener bind: {0}")]
    Integrity(String),
    #[error("writable storage readiness failed before listener bind: {0}")]
    WritableStorage(String),
    #[error("authentication initialization failed: {0}")]
    Authentication(String),
    #[error("embedded frontend validation failed: {0}")]
    StaticAssets(#[from] StaticAssetError),
    #[error("application readiness failed before listener bind")]
    Readiness,
    #[error("failed to bind {address}: {source}")]
    Bind {
        address: SocketAddr,
        source: std::io::Error,
    },
    #[error("failed to bind metrics listener {address}: {source}")]
    MetricsBind {
        address: SocketAddr,
        source: std::io::Error,
    },
    #[error("HTTP server failed: {0}")]
    Http(std::io::Error),
    #[error("background service failed: {0}")]
    Background(String),
    #[error("background services did not drain within 30 seconds")]
    DrainTimeout,
    #[error("production service initialization failed before listener bind: {0}")]
    ProductionServices(String),
}

impl App {
    pub async fn build(config: Config) -> Result<Self, AppError> {
        config
            .validate()
            .map_err(|error| AppError::Config(error.to_string()))?;
        let cookie_mode = cookie_mode(&config)?;
        ensure_writable_directory(&config.data.attachments)?;
        ensure_writable_directory(&config.data.backups)?;
        ensure_safe_data_layout(
            &config.data.database,
            &config.data.attachments,
            &config.data.backups,
        )?;

        let database = Database::open(&DatabaseConfig::new(&config.data.database))
            .await
            .map_err(|error| AppError::Database(error.to_string()))?;
        let migrations = MigrationRunner::embedded(env!("CARGO_PKG_VERSION"));
        run_guarded_migrations(&config, &database, &migrations)
            .await
            .map_err(|error| AppError::Migration(error.to_string()))?;

        let integrity = IntegrityService::new(database.clone());
        integrity
            .quick()
            .await
            .map_err(|error| AppError::Integrity(error.to_string()))?;
        let unfinished_integrity_jobs: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM jobs WHERE kind = 'integrity.weekly' AND state != 'succeeded'",
        )
        .fetch_one(database.pool())
        .await
        .map_err(|error| AppError::Integrity(error.to_string()))?;
        if unfinished_integrity_jobs > 0 {
            integrity
                .full()
                .await
                .map_err(|error| AppError::Integrity(error.to_string()))?;
        }

        let identity = Arc::new(IdentityRepository::new(database.clone()));
        let (auth, setup) = initialize_auth(
            Arc::clone(&identity),
            cookie_mode,
            config.http.public_origin.clone(),
        )
        .await
        .map_err(|error| AppError::Authentication(error.to_string()))?;

        let backups = BackupService::new(&config.data.backups, &config.data.attachments);
        let store: Arc<dyn orbit_platform::BlobStore> =
            Arc::new(LocalBlobStore::new(&config.data.attachments));
        let workspaces = Arc::new(WorkspaceRepository::with_blob_store_and_mutations(
            database.clone(),
            Arc::clone(&store),
            backups.attachment_mutations(),
        ));
        let upload_limits = UploadLimits::new(
            config.uploads.max_file_bytes,
            config.uploads.max_request_bytes,
        )
        .map_err(|error| AppError::Config(error.to_string()))?;
        let attachment_state = AttachmentState::new(
            Arc::clone(&identity),
            UploadService::new(
                database.clone(),
                store,
                backups.attachment_mutations(),
                upload_limits,
            ),
            cookie_mode,
        );
        attachment_state
            .reconcile_at(orbit_platform::TimestampMillis::now().as_millis())
            .await
            .map_err(|error| AppError::WritableStorage(error.to_string()))?;

        let production_services = initialize_production_services(
            &database,
            Arc::clone(&workspaces),
            integrity.clone(),
            config.jobs.concurrency,
        )
        .await?;

        let health = health_registry(&config, &database, &integrity);
        if !health.readiness().await.ready {
            return Err(AppError::Readiness);
        }

        let assets = StaticAssets::verified(crate::openapi::CONTRACT_ID, FRONTEND_REVISION)?;
        let mut origin_policy = OriginPolicy::new(&config.http.public_origin);
        if config.environment == orbit_platform::EnvironmentMode::Development {
            origin_policy = origin_policy.allow_any_http_origin();
        }
        for proxy in &config.http.trusted_proxies {
            origin_policy = origin_policy.trust_proxy(*proxy);
        }
        let metrics = Metrics::default();
        let router = metrics.instrument(production_router(
            ApiRoutes {
                auth,
                workspaces: WorkspaceState::with_repository(
                    Arc::clone(&identity),
                    Arc::clone(&workspaces),
                    config.http.public_origin.clone(),
                    cookie_mode,
                    backups.clone(),
                ),
                tasks: TaskState::new(Arc::clone(&identity), cookie_mode),
                attachments: attachment_state.clone(),
                integrations: IntegrationState::new(
                    Arc::new(ApiTokenRepository::new(database.clone())),
                    Arc::new(TaskRepository::new(database.clone())),
                ),
            },
            health,
            assets,
            origin_policy,
            HttpLimits {
                max_body_bytes: usize::try_from(upload_limits.max_request_bytes())
                    .unwrap_or(usize::MAX),
                ..HttpLimits::default()
            },
            config.rate_limits,
            config.environment == orbit_platform::EnvironmentMode::Production,
        ));

        Ok(Self {
            config,
            database,
            router,
            setup_url: setup.map(|setup| setup.url),
            attachments: attachment_state,
            metrics,
            backups,
            workspaces,
            production_services: Some(production_services),
        })
    }

    pub fn router(&self) -> Router {
        self.router.clone()
    }

    pub fn metrics_router(&self) -> Router {
        self.metrics.router()
    }

    #[must_use]
    pub fn database(&self) -> &Database {
        &self.database
    }

    #[must_use]
    pub fn setup_url(&self) -> Option<&str> {
        self.setup_url.as_deref()
    }

    #[must_use]
    pub fn listen_addr(&self) -> SocketAddr {
        self.config.http.listen_addr()
    }

    #[must_use]
    pub fn production_services_ready(&self) -> bool {
        self.production_services.is_some()
    }

    pub async fn create_backup(&self) -> Result<String, AppError> {
        self.backups
            .create(&self.database)
            .await
            .map(|snapshot| snapshot.id)
            .map_err(|error| AppError::Background(error.to_string()))
    }

    pub async fn run_retention_maintenance(&self) -> Result<(), AppError> {
        self.workspaces
            .run_retention_maintenance()
            .await
            .map_err(|error| AppError::Background(error.to_string()))
    }

    pub async fn run(self, shutdown: CancellationToken) -> Result<(), AppError> {
        let address = self.listen_addr();
        let listener = tokio::net::TcpListener::bind(address)
            .await
            .map_err(|source| AppError::Bind { address, source })?;
        self.serve(listener, shutdown).await
    }

    pub async fn serve(
        mut self,
        listener: tokio::net::TcpListener,
        shutdown: CancellationToken,
    ) -> Result<(), AppError> {
        let service_shutdown = CancellationToken::new();
        let mut services = JoinSet::new();

        if let Some(address) = self.config.metrics.listen {
            let listener = tokio::net::TcpListener::bind(address)
                .await
                .map_err(|source| AppError::MetricsBind { address, source })?;
            let router = self.metrics.router();
            let token = service_shutdown.clone();
            services.spawn(async move {
                axum::serve(listener, router)
                    .with_graceful_shutdown(token.cancelled_owned())
                    .await
                    .map_err(|error| error.to_string())
            });
        }

        let production = self.production_services.take().ok_or_else(|| {
            AppError::ProductionServices("production services were already started".to_owned())
        })?;
        let integrity_failure = production.integrity_failure.clone();
        let token = service_shutdown.clone();
        services.spawn(async move {
            production
                .worker
                .run(token)
                .await
                .map_err(|error| error.to_string())
        });
        let database = self.database.clone();
        let scheduler = production.scheduler;
        let token = service_shutdown.clone();
        services.spawn(async move { run_scheduler_service(scheduler, database, token).await });
        let attachments = self.attachments.clone();
        let token = service_shutdown.clone();
        services.spawn(async move {
            attachments
                .run_reconciliation_service(token)
                .await
                .map_err(|error| error.to_string())
        });
        let http_shutdown = CancellationToken::new();
        let server = axum::serve(
            listener,
            self.router
                .into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(http_shutdown.clone().cancelled_owned())
        .into_future();
        tokio::pin!(server);

        let mut background_error = None;
        let mut http_result = None;
        tokio::select! {
            result = &mut server => http_result = Some(result),
            () = shutdown.cancelled() => {}
            () = integrity_failure.cancelled() => {
                background_error = Some("weekly full database integrity check failed".to_owned());
            }
            result = services.join_next() => {
                background_error = Some(joined_service_error(result));
            }
        }

        http_shutdown.cancel();
        service_shutdown.cancel();
        let drain = async {
            if http_result.is_none() {
                http_result = Some(server.await);
            }
            while let Some(result) = services.join_next().await {
                if background_error.is_none() {
                    let error = joined_service_error(Some(result));
                    if error != "background service stopped after shutdown" {
                        background_error = Some(error);
                    }
                }
            }
        };
        if tokio::time::timeout(BACKGROUND_DRAIN, drain).await.is_err() {
            abort_services(&mut services);
            return Err(AppError::DrainTimeout);
        }
        http_result
            .expect("HTTP server result is recorded during drain")
            .map_err(AppError::Http)?;
        match background_error {
            Some(error) => Err(AppError::Background(error)),
            None => Ok(()),
        }
    }
}

fn cookie_mode(config: &Config) -> Result<CookieMode, AppError> {
    match config.environment {
        orbit_platform::EnvironmentMode::Production => Ok(CookieMode::secure()),
        orbit_platform::EnvironmentMode::Development => {
            CookieMode::loopback_development(config.http.listen_addr())
                .map_err(|error| AppError::Config(error.to_string()))
        }
        orbit_platform::EnvironmentMode::Unspecified => Err(AppError::Config(
            "environment must be explicitly configured".to_owned(),
        )),
    }
}

fn ensure_writable_directory(path: &Path) -> Result<(), AppError> {
    fs::create_dir_all(path)
        .map_err(|error| AppError::WritableStorage(format!("{}: {error}", path.display())))?;
    let probe = path.join(format!(
        ".orbit-write-probe-{}-{}",
        std::process::id(),
        orbit_platform::Id::new_v7()
    ));
    let file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&probe)
        .map_err(|error| AppError::WritableStorage(format!("{}: {error}", path.display())))?;
    file.sync_all()
        .map_err(|error| AppError::WritableStorage(format!("{}: {error}", path.display())))?;
    drop(file);
    fs::remove_file(&probe)
        .map_err(|error| AppError::WritableStorage(format!("{}: {error}", path.display())))?;
    Ok(())
}

fn ensure_safe_data_layout(
    database: &Path,
    attachments: &Path,
    backups: &Path,
) -> Result<(), AppError> {
    let attachments = fs::canonicalize(attachments)
        .map_err(|error| AppError::WritableStorage(error.to_string()))?;
    let backups =
        fs::canonicalize(backups).map_err(|error| AppError::WritableStorage(error.to_string()))?;
    if attachments == backups
        || attachments.starts_with(&backups)
        || backups.starts_with(&attachments)
    {
        return Err(AppError::Config(
            "backup directory must be outside live attachment storage".to_owned(),
        ));
    }
    let database_name = database
        .file_name()
        .ok_or_else(|| AppError::Config("database path must name a file".to_owned()))?;
    let database_parent = database
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(database_parent)
        .map_err(|error| AppError::WritableStorage(error.to_string()))?;
    let database = fs::canonicalize(database_parent)
        .map_err(|error| AppError::WritableStorage(error.to_string()))?
        .join(database_name);
    if database.starts_with(&attachments) || database.starts_with(&backups) {
        return Err(AppError::Config(
            "database file must be outside attachment and backup storage".to_owned(),
        ));
    }
    Ok(())
}

fn health_registry(
    config: &Config,
    database: &Database,
    integrity: &IntegrityService,
) -> HealthRegistry {
    let migrations_database = database.clone();
    let mut health = HealthRegistry::new().with_check(HealthCheck::Migrations, move || {
        let database = migrations_database.clone();
        async move {
            let pending = MigrationRunner::embedded(env!("CARGO_PKG_VERSION"))
                .pending(&database)
                .await
                .map_err(|error| error.to_string())?;
            if pending.is_empty() {
                Ok(())
            } else {
                Err("pending migrations".to_owned())
            }
        }
    });
    integrity.register_readiness(&mut health);
    let attachment_path = config.data.attachments.clone();
    let backup_path = config.data.backups.clone();
    health.register(HealthCheck::WritableStorage, move || {
        let attachment_path = attachment_path.clone();
        let backup_path = backup_path.clone();
        async move {
            tokio::task::spawn_blocking(move || {
                ensure_writable_directory(&attachment_path).map_err(|error| error.to_string())?;
                ensure_writable_directory(&backup_path).map_err(|error| error.to_string())
            })
            .await
            .map_err(|error| error.to_string())?
        }
    });
    let scheduler_database = database.clone();
    health.register(HealthCheck::Scheduler, move || {
        let database = scheduler_database.clone();
        async move {
            let count = database
                .scalar::<i64>(
                    "SELECT COUNT(DISTINCT job_kind) FROM schedules WHERE enabled = 1 \
                     AND job_kind IN ('workspace.retention', 'integrity.weekly')",
                )
                .await
                .map_err(|error| error.to_string())?;
            if count == 2 {
                Scheduler::new(JobStore::new(database))
                    .validate_enabled()
                    .await
                    .map_err(|error| error.to_string())
            } else {
                Err("critical production schedules are not initialized".to_owned())
            }
        }
    });
    let checked_config = config.clone();
    health.register(HealthCheck::CriticalConfig, move || {
        let config = checked_config.clone();
        async move { config.validate().map_err(|error| error.to_string()) }
    });
    health
}

async fn initialize_production_services(
    database: &Database,
    workspaces: Arc<WorkspaceRepository>,
    integrity: IntegrityService,
    concurrency: usize,
) -> Result<ProductionServices, AppError> {
    let store = JobStore::new(database.clone());
    let scheduler = Scheduler::new(store.clone());
    let failure = CancellationToken::new();
    let retention_repository = Arc::clone(&workspaces);
    let integrity_service = integrity;
    let integrity_failure = failure.clone();
    let worker_config = WorkerConfig::new(concurrency)
        .map_err(|error| AppError::ProductionServices(error.to_string()))?;
    let worker = Worker::new(store, worker_config)
        .with_handler(maintenance_kind("workspace.retention"), move |_| {
            let repository = Arc::clone(&retention_repository);
            async move {
                repository
                    .run_retention_maintenance()
                    .await
                    .map_err(|_| JobError::Retryable("retention maintenance failed".to_owned()))
            }
        })
        .and_then(|worker| {
            worker.with_handler(integrity_kind(), move |_| {
                let integrity = integrity_service.clone();
                let failure = integrity_failure.clone();
                async move {
                    integrity.full().await.map_err(|error| {
                        failure.cancel();
                        JobError::Permanent(error.to_string())
                    })
                }
            })
        })
        .map_err(|error| AppError::ProductionServices(error.to_string()))?;

    let now = database
        .database_now()
        .await
        .map_err(|error| AppError::ProductionServices(error.to_string()))?;
    ensure_schedule(
        database,
        &scheduler,
        "workspace.retention",
        Duration::from_secs(24 * 60 * 60),
        now,
    )
    .await?;
    database
        .execute("UPDATE schedules SET enabled = 0 WHERE job_kind = 'backup.daily'")
        .await
        .map_err(|error| AppError::ProductionServices(error.to_string()))?;
    database
        .execute("DELETE FROM jobs WHERE kind = 'backup.daily'")
        .await
        .map_err(|error| AppError::ProductionServices(error.to_string()))?;
    ensure_schedule(
        database,
        &scheduler,
        "integrity.weekly",
        Duration::from_secs(7 * 24 * 60 * 60),
        now,
    )
    .await?;
    scheduler
        .validate_enabled()
        .await
        .map_err(|error| AppError::ProductionServices(error.to_string()))?;

    Ok(ProductionServices {
        worker,
        scheduler,
        integrity_failure: failure,
    })
}

async fn ensure_schedule(
    database: &Database,
    scheduler: &Scheduler,
    kind: &str,
    cadence: Duration,
    now: TimestampMillis,
) -> Result<(), AppError> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM schedules WHERE job_kind = ?")
        .bind(kind)
        .fetch_one(database.pool())
        .await
        .map_err(|error| AppError::ProductionServices(error.to_string()))?;
    if count > 1 {
        return Err(AppError::ProductionServices(format!(
            "multiple durable schedules exist for {kind}"
        )));
    }
    if count == 0 {
        scheduler
            .upsert(&RecurringSchedule::interval(
                if kind == "integrity.weekly" {
                    integrity_kind()
                } else {
                    maintenance_kind(kind)
                },
                json!({}),
                cadence,
                now,
            ))
            .await
            .map_err(|error| AppError::ProductionServices(error.to_string()))?;
    }
    Ok(())
}

fn maintenance_kind(name: &str) -> JobKind {
    JobKind::new(name).with_concurrency_limit(1)
}

fn integrity_kind() -> JobKind {
    JobKind::new("integrity.weekly")
        .with_retry_policy(1, Vec::new())
        .with_concurrency_limit(1)
}

async fn run_scheduler_service(
    scheduler: Scheduler,
    database: Database,
    shutdown: CancellationToken,
) -> Result<(), String> {
    loop {
        let now = database
            .database_now()
            .await
            .map_err(|error| error.to_string())?;
        scheduler
            .materialize_due(now)
            .await
            .map_err(|error| error.to_string())?;
        tokio::select! {
            () = shutdown.cancelled() => return Ok(()),
            () = tokio::time::sleep(Duration::from_secs(60)) => {}
        }
    }
}

fn joined_service_error(
    result: Option<Result<Result<(), String>, tokio::task::JoinError>>,
) -> String {
    match result {
        None => "all background services stopped unexpectedly".to_owned(),
        Some(Ok(Ok(()))) => "background service stopped after shutdown".to_owned(),
        Some(Ok(Err(error))) => error,
        Some(Err(error)) => error.to_string(),
    }
}

fn abort_services(services: &mut JoinSet<Result<(), String>>) {
    services.abort_all();
}

#[cfg(test)]
async fn drain_services(services: &mut JoinSet<Result<(), String>>, deadline: Duration) -> bool {
    let drain = async { while services.join_next().await.is_some() {} };
    if tokio::time::timeout(deadline, drain).await.is_err() {
        abort_services(services);
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbit_platform::{EnvironmentMode, Id};
    use tempfile::TempDir;
    use tokio::io::AsyncWriteExt;

    fn test_config(root: &TempDir) -> Config {
        let mut config = Config {
            environment: EnvironmentMode::Development,
            ..Config::default()
        };
        config.data.database = root.path().join("data/orbit.sqlite");
        config.data.attachments = root.path().join("data/attachments");
        config.data.backups = root.path().join("backups");
        config
    }

    #[tokio::test]
    async fn composed_backup_waits_for_a_live_upload_mutation() {
        let root = TempDir::new().unwrap();
        let app = App::build(test_config(&root)).await.unwrap();
        let uploads = app.attachments.uploads.clone();
        let (mut writer, reader) = tokio::io::duplex(64);
        writer.write_all(b"in progress").await.unwrap();
        let upload = tokio::spawn(async move {
            uploads
                .stage(Id::new_v7(), Id::new_v7(), "progress.txt", reader)
                .await
        });
        let temporary = root.path().join("data/attachments/temporary");
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if std::fs::read_dir(&temporary).is_ok_and(|mut entries| entries.next().is_some()) {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();

        let backups = app.backups.clone();
        let database = app.database.clone();
        let mut backup = tokio::spawn(async move { backups.create(&database).await });
        assert!(
            tokio::time::timeout(Duration::from_millis(50), &mut backup)
                .await
                .is_err(),
            "backup must wait while the composed upload service holds the mutation guard"
        );

        drop(writer);
        upload.await.unwrap().unwrap();
        tokio::time::timeout(Duration::from_secs(2), backup)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn composed_backup_waits_for_retention_attachment_mutation() {
        let root = TempDir::new().unwrap();
        let app = App::build(test_config(&root)).await.unwrap();
        let transaction = app.database.immediate_transaction().await.unwrap();
        let workspaces = Arc::clone(&app.workspaces);
        let retention = tokio::spawn(async move { workspaces.run_retention_maintenance().await });
        tokio::time::sleep(Duration::from_millis(50)).await;

        let backups = app.backups.clone();
        let database = app.database.clone();
        let mut backup = tokio::spawn(async move { backups.create(&database).await });
        assert!(
            tokio::time::timeout(Duration::from_millis(50), &mut backup)
                .await
                .is_err(),
            "backup must wait while retention holds the shared mutation guard"
        );

        transaction.rollback().await.unwrap();
        retention.await.unwrap().unwrap();
        tokio::time::timeout(Duration::from_secs(2), backup)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shutdown_deadline_does_not_join_a_non_yielding_service() {
        let mut services = JoinSet::new();
        services.spawn(async {
            std::thread::sleep(Duration::from_millis(500));
            Ok::<(), String>(())
        });

        let started = std::time::Instant::now();
        assert!(drain_services(&mut services, Duration::from_millis(20)).await);
        assert!(started.elapsed() < Duration::from_millis(250));
    }
}
