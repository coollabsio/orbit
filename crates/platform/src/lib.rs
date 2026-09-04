//! Reusable platform interfaces for Orbit.

mod config;
mod db;
mod health;
mod http;
mod id;
mod jobs;
mod observability;
mod problem;
mod time;

pub use config::{Config, ConfigError, ConfigOverride, ConfigSources, HttpConfig, Secret};
pub use db::{
    Database, DatabaseConfig, DatabaseError, Migration, MigrationError, MigrationRunner,
    PendingMigration, TestDatabase, TestDatabaseError,
};
pub use health::{HealthCheck, HealthCheckResult, HealthRegistry, ReadinessReport};
pub use http::{
    ClientIp, HttpLimits, HttpPlatformLayer, OriginPolicy, RequestId, RequestTransport,
};
pub use id::{Id, ParseIdError};
pub use jobs::{
    CatchUpMode, Claim, ClaimSelection, CronSchedule, Job, JobContext, JobError, JobKind,
    JobKindRegistry, JobPriority, JobQueue, JobState, JobStore, JobStoreError, RecurringSchedule,
    ScheduleError, Scheduler, Worker, WorkerConfig, WorkerConfigError, WorkerError,
};
pub use observability::{TracingFormat, init_tracing};
pub use problem::{ConflictMetadata, Problem};
pub use time::TimestampMillis;
