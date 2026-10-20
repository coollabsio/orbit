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
    AuthenticatedUser, COMMON_PASSWORD_DATASET_VERSION, EmailFailures, InMemorySessionStore,
    IssuedSession, IssuedToken, LoginReservation, LoginThrottler, OneTimeTokenStore, PasswordError,
    PasswordExecutor, PasswordService, PasswordVerification, SessionError, SessionRecord,
    SessionStore, THROTTLE_WINDOW_MILLIS, ThrottleDecision, TokenError, TokenKind,
    generate_opaque_token, normalize_email,
};
pub use backup::{
    AttachmentMutationCoordinator, AttachmentMutationGuard, BackupError, BackupFile, BackupKind,
    BackupManifest, BackupService, BackupSnapshot,
};
pub use config::{
    AppKey, Config, ConfigError, DataConfig, EnvironmentMode, HttpConfig, JobsConfig,
    MetricsConfig, RateLimitConfig, UploadConfig, redact_database_url,
};
pub use db::{
    Access, Backend, Column, Database, DatabaseConfig, DatabaseError, Flag, GuardedMigrationError,
    Migration, MigrationError, MigrationRunner, NotBool, PendingMigration, Row, Separated,
    Singleton, SqlBuilder, TestDatabase, TestDatabaseError, lock_database_ownership,
    run_guarded_migrations, testing_postgres,
};
pub use files::{
    AuthorizedAttachment, BLOB_REFERENCE_COUNT, BlobDownload, BlobFuture, BlobMoveStatus,
    BlobObject, BlobReader, BlobStore, BlobStoreError, ContentDisposition, DownloadMetadata,
    FinalizedAttachment, FinalizedBlob, INLINE_IMAGE_TYPES, LocalBlobStore, NewAttachmentReference,
    ObjectStorage, ObjectStorageState, ReconcileResult, S3_DELETE_DELAY_MILLIS, S3Bucket, S3Config,
    S3Error, S3Object, StagedUpload, StoredObject, TieredBlobStore, UploadError,
    UploadFinalization, UploadLimitError, UploadLimits, UploadService, start_fake_s3,
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
