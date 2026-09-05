use std::fmt;
use std::fs::{self, OpenOptions};
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use orbit_platform::{
    AttachmentMutationCoordinator, BackupService, Config, Database, DatabaseConfig, HealthCheck,
    HealthRegistry, HttpLimits, IntegrityService, LocalBlobStore, MigrationRunner, OriginPolicy,
    UploadLimits, UploadService, WorkerConfig,
};
use thiserror::Error;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::attachment_routes::AttachmentState;
use crate::auth_routes::{AdminRecoveryDelivery, CookieMode, initialize_auth};
use crate::metrics::Metrics;
use crate::repositories::identity::IdentityRepository;
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
    workspaces: Arc<WorkspaceRepository>,
    attachments: AttachmentState,
    recovery_delivery: Arc<AdminRecoveryDelivery>,
    metrics: Metrics,
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
        let pending = migrations
            .pending(&database)
            .await
            .map_err(|error| AppError::Migration(error.to_string()))?;
        if pending.iter().any(|migration| migration.destructive) {
            BackupService::new(&config.data.backups, &config.data.attachments)
                .create_pre_migration(&database)
                .await
                .map_err(|error| AppError::PreMigrationBackup(error.to_string()))?;
        }
        migrations
            .run(&database)
            .await
            .map_err(|error| AppError::Migration(error.to_string()))?;

        let integrity = IntegrityService::new(database.clone());
        integrity
            .quick()
            .await
            .map_err(|error| AppError::Integrity(error.to_string()))?;

        let identity = Arc::new(IdentityRepository::new(database.clone()));
        let recovery_delivery = Arc::new(AdminRecoveryDelivery::new(128));
        let recovery: Arc<dyn crate::auth_routes::RecoveryDelivery> = recovery_delivery.clone();
        let (auth, setup) = initialize_auth(
            Arc::clone(&identity),
            cookie_mode,
            recovery,
            config.http.public_origin.clone(),
        )
        .await
        .map_err(|error| AppError::Authentication(error.to_string()))?;

        let store: Arc<dyn orbit_platform::BlobStore> =
            Arc::new(LocalBlobStore::new(&config.data.attachments));
        let workspaces = Arc::new(WorkspaceRepository::with_blob_store(
            database.clone(),
            Arc::clone(&store),
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
                AttachmentMutationCoordinator::default(),
                upload_limits,
            ),
            cookie_mode,
        );
        attachment_state
            .reconcile_at(orbit_platform::TimestampMillis::now().as_millis())
            .await
            .map_err(|error| AppError::WritableStorage(error.to_string()))?;

        let health = health_registry(&config, &database, &integrity);
        if !health.readiness().await.ready {
            return Err(AppError::Readiness);
        }

        let assets = StaticAssets::verified(crate::openapi::CONTRACT_ID, FRONTEND_REVISION)?;
        let mut origin_policy = OriginPolicy::new(&config.http.public_origin);
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
                ),
                tasks: TaskState::new(identity, cookie_mode),
                attachments: attachment_state.clone(),
            },
            health,
            assets,
            origin_policy,
            HttpLimits {
                max_body_bytes: usize::try_from(upload_limits.max_request_bytes())
                    .unwrap_or(usize::MAX),
                ..HttpLimits::default()
            },
        ));

        Ok(Self {
            config,
            database,
            router,
            setup_url: setup.map(|setup| setup.url),
            workspaces,
            attachments: attachment_state,
            recovery_delivery,
            metrics,
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
    pub fn recovery_delivery(&self) -> Arc<AdminRecoveryDelivery> {
        Arc::clone(&self.recovery_delivery)
    }

    #[must_use]
    pub fn listen_addr(&self) -> SocketAddr {
        self.config.http.listen_addr()
    }

    pub async fn run(self, shutdown: CancellationToken) -> Result<(), AppError> {
        let address = self.listen_addr();
        let listener = tokio::net::TcpListener::bind(address)
            .await
            .map_err(|source| AppError::Bind { address, source })?;
        self.serve(listener, shutdown).await
    }

    pub async fn serve(
        self,
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

        let workspaces = Arc::clone(&self.workspaces);
        let token = service_shutdown.clone();
        let worker_config = WorkerConfig::new(self.config.jobs.concurrency)
            .map_err(|error| AppError::Config(error.to_string()))?;
        services.spawn(async move {
            workspaces
                .run_retention_service(token, Duration::from_secs(24 * 60 * 60), worker_config)
                .await
                .map_err(|error| error.to_string())
        });
        let attachments = self.attachments.clone();
        let token = service_shutdown.clone();
        services.spawn(async move {
            attachments
                .run_reconciliation_service(token)
                .await
                .map_err(|error| error.to_string())
        });
        let backups = BackupService::new(&self.config.data.backups, &self.config.data.attachments);
        let database = self.database.clone();
        let token = service_shutdown.clone();
        services.spawn(async move { run_backup_service(backups, database, token).await });

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
        tokio::select! {
            result = &mut server => result.map_err(AppError::Http)?,
            () = shutdown.cancelled() => {
                http_shutdown.cancel();
                server.await.map_err(AppError::Http)?;
            }
            result = services.join_next() => {
                background_error = Some(joined_service_error(result));
                http_shutdown.cancel();
                server.await.map_err(AppError::Http)?;
            }
        }

        service_shutdown.cancel();
        let drain = async {
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
            services.abort_all();
            while services.join_next().await.is_some() {}
            return Err(AppError::DrainTimeout);
        }
        match background_error {
            Some(error) => Err(AppError::Background(error)),
            None => Ok(()),
        }
    }
}

fn cookie_mode(config: &Config) -> Result<CookieMode, AppError> {
    if config.http.public_origin.starts_with("https://") {
        return Ok(CookieMode::secure());
    }
    CookieMode::loopback_development(config.http.listen_addr())
        .map_err(|error| AppError::Config(error.to_string()))
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
            database
                .scalar::<i64>("SELECT COUNT(*) FROM schedules")
                .await
                .map(|_| ())
                .map_err(|error| error.to_string())
        }
    });
    let checked_config = config.clone();
    health.register(HealthCheck::CriticalConfig, move || {
        let config = checked_config.clone();
        async move { config.validate().map_err(|error| error.to_string()) }
    });
    health
}

async fn run_backup_service(
    backups: BackupService,
    database: Database,
    shutdown: CancellationToken,
) -> Result<(), String> {
    let mut interval = tokio::time::interval(Duration::from_secs(24 * 60 * 60));
    interval.tick().await;
    loop {
        tokio::select! {
            () = shutdown.cancelled() => return Ok(()),
            _ = interval.tick() => {
                backups.create(&database).await.map_err(|error| error.to_string())?;
            }
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
