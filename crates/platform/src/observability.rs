use tracing_subscriber::EnvFilter;
use tracing_subscriber::util::SubscriberInitExt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TracingFormat {
    Compact,
    Json,
}

/// Installs local logging without configuring any external telemetry exporter.
pub fn init_tracing(format: TracingFormat) -> Result<(), tracing_subscriber::util::TryInitError> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    match format {
        TracingFormat::Compact => tracing_subscriber::fmt()
            .compact()
            .with_env_filter(filter)
            .finish()
            .try_init(),
        TracingFormat::Json => tracing_subscriber::fmt()
            .json()
            .with_env_filter(filter)
            .finish()
            .try_init(),
    }
}
