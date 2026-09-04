use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde::Serialize;

type CheckFuture = Pin<Box<dyn Future<Output = Result<(), String>> + Send>>;
type CheckFn = Arc<dyn Fn() -> CheckFuture + Send + Sync>;

/// The complete readiness boundary required before Orbit receives traffic.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum HealthCheck {
    Migrations,
    DatabaseIntegrity,
    WritableStorage,
    Scheduler,
    CriticalConfig,
}

impl HealthCheck {
    const ALL: [Self; 5] = [
        Self::Migrations,
        Self::DatabaseIntegrity,
        Self::WritableStorage,
        Self::Scheduler,
        Self::CriticalConfig,
    ];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Migrations => "migrations",
            Self::DatabaseIntegrity => "database_integrity",
            Self::WritableStorage => "writable_storage",
            Self::Scheduler => "scheduler",
            Self::CriticalConfig => "critical_config",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct HealthCheckResult {
    pub ready: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ReadinessReport {
    pub ready: bool,
    pub checks: BTreeMap<String, HealthCheckResult>,
}

/// Owns the named asynchronous checks used for traffic readiness.
#[derive(Clone, Default)]
pub struct HealthRegistry {
    checks: BTreeMap<HealthCheck, CheckFn>,
}

impl HealthRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_check<F, Fut>(mut self, name: HealthCheck, check: F) -> Self
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), String>> + Send + 'static,
    {
        self.register(name, check);
        self
    }

    pub fn register<F, Fut>(&mut self, name: HealthCheck, check: F)
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), String>> + Send + 'static,
    {
        self.checks
            .insert(name, Arc::new(move || Box::pin(check())));
    }

    /// Liveness deliberately depends only on the responding process.
    #[must_use]
    pub const fn is_live(&self) -> bool {
        true
    }

    pub async fn readiness(&self) -> ReadinessReport {
        let mut ready = true;
        let mut checks = BTreeMap::new();
        for name in HealthCheck::ALL {
            let result = match self.checks.get(&name) {
                Some(check) => check().await,
                None => Err("check is not registered".to_owned()),
            };
            let check = match result {
                Ok(()) => HealthCheckResult { ready: true },
                Err(_) => {
                    ready = false;
                    HealthCheckResult { ready: false }
                }
            };
            checks.insert(name.as_str().to_owned(), check);
        }
        ReadinessReport { ready, checks }
    }
}
