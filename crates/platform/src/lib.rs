//! Reusable platform interfaces for Orbit.

mod auth;
mod backup;
mod config;
mod db;
mod files;
mod health;
mod http;
mod id;
mod integrity;
mod jobs;
mod observability;
mod problem;
mod time;

pub use auth::{
    AuthenticatedUser, COMMON_PASSWORD_DATASET_VERSION, InMemorySessionStore, IssuedSession,
    IssuedToken, LoginReservation, LoginThrottler, OneTimeTokenStore, PasswordError,
    PasswordExecutor, PasswordService, PasswordVerification, SessionError, SessionRecord,
    SessionStore, ThrottleDecision, TokenError, TokenKind, generate_opaque_token, normalize_email,
};
pub use backup::{
    AttachmentMutationCoordinator, AttachmentMutationGuard, BackupError, BackupFile, BackupKind,
    BackupManifest, BackupService, BackupSnapshot,
};
pub use config::{
    Config, ConfigError, ConfigOverride, ConfigSources, DataConfig, EnvironmentMode, HttpConfig,
    JobsConfig, MetricsConfig, RateLimitConfig, Secret, UploadConfig,
};
pub use db::{
    Database, DatabaseConfig, DatabaseError, GuardedMigrationError, Migration, MigrationError,
    MigrationRunner, PendingMigration, TestDatabase, TestDatabaseError, run_guarded_migrations,
};
pub use files::{
    AuthorizedAttachment, BlobDownload, BlobFuture, BlobObject, BlobReader, BlobStore,
    BlobStoreError, ContentDisposition, DownloadMetadata, FinalizedAttachment, FinalizedBlob,
    LocalBlobStore, NewAttachmentReference, ReconcileResult, StagedUpload, StoredObject,
    UploadError, UploadFinalization, UploadLimitError, UploadLimits, UploadService,
};
pub use health::{HealthCheck, HealthCheckResult, HealthRegistry, ReadinessReport};
pub use http::{
    ClientIp, HttpLimits, HttpPlatformLayer, OriginPolicy, RequestId, RequestTransport,
};
pub use id::{Id, ParseIdError};
pub use integrity::{IntegrityError, IntegrityService};
pub use jobs::{
    CatchUpMode, Claim, ClaimSelection, CronSchedule, Job, JobContext, JobError, JobKind,
    JobKindRegistrationError, JobKindRegistry, JobPriority, JobQueue, JobState, JobStore,
    JobStoreError, RecurringSchedule, ScheduleError, Scheduler, Worker, WorkerConfig,
    WorkerConfigError, WorkerError,
};
pub use observability::{TracingFormat, init_tracing};
pub use problem::{ConflictMetadata, Problem};
pub use time::TimestampMillis;
