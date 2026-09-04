//! Reusable platform interfaces for Orbit.

mod config;
mod db;
mod health;
mod http;
mod id;
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
pub use observability::{TracingFormat, init_tracing};
pub use problem::{ConflictMetadata, Problem};
pub use time::TimestampMillis;
