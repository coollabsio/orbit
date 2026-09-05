use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use fs2::FileExt;
use orbit_platform::{
    AttachmentMutationCoordinator, BackupService, Config, ConfigOverride, ConfigSources, Database,
    DatabaseConfig, HttpLimits, HttpPlatformLayer, LocalBlobStore, MigrationRunner, OriginPolicy,
    TimestampMillis, UploadLimits, UploadService,
};
use orbit_server::attachment_routes::{AttachmentState, attachment_router};
use orbit_server::auth_routes::{AdminRecoveryDelivery, CookieMode, auth_router, initialize_auth};
use orbit_server::repositories::identity::IdentityRepository;
use orbit_server::repositories::workspaces::WorkspaceRepository;
use orbit_server::task_routes::{TaskState, task_router};
use orbit_server::workspace_routes::{WorkspaceState, workspace_router};
use thiserror::Error;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Parser)]
#[command(name = "orbit", about = "Orbit operations command-line interface")]
pub struct Cli {
    #[arg(long, global = true, default_value = "orbit.toml")]
    config: PathBuf,
    #[arg(long, global = true, default_value = "orbit.sqlite")]
    database: PathBuf,
    #[arg(long, global = true, default_value = "attachments")]
    attachments: PathBuf,
    #[arg(long, global = true, default_value_t = 25 * 1024 * 1024)]
    attachment_max_file_bytes: u64,
    #[arg(long, global = true, default_value_t = 100 * 1024 * 1024)]
    attachment_max_request_bytes: u64,
    #[arg(long, global = true, default_value = "backups")]
    backups: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Serve {
        #[arg(long, default_value = "127.0.0.1:3000")]
        listen: std::net::SocketAddr,
        #[arg(long, default_value = "https://localhost")]
        origin: String,
    },
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    Migrate,
    Backup {
        #[command(subcommand)]
        command: BackupCommand,
    },
    DbReset {
        #[arg(long)]
        yes: bool,
    },
    Seed,
    Openapi {
        #[arg(long)]
        output: PathBuf,
    },
    SetupToken {
        #[command(subcommand)]
        command: SetupTokenCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    Show,
}

#[derive(Debug, Subcommand)]
pub enum BackupCommand {
    Create,
    List {
        #[arg(long)]
        pre_migration: bool,
    },
    Verify {
        id: String,
    },
    Restore {
        id: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum SetupTokenCommand {
    Init {
        #[arg(long, default_value = "https://localhost")]
        origin: String,
    },
    Rotate {
        #[arg(long, default_value = "https://localhost")]
        origin: String,
    },
}

#[derive(Debug, Error)]
pub enum CliError {
    #[error("{0}")]
    Operation(String),
}

pub async fn run(cli: Cli) -> Result<String, CliError> {
    match &cli.command {
        Command::Serve { listen, origin } => serve(&cli, *listen, origin).await,
        Command::Config {
            command: ConfigCommand::Show,
        } => {
            let config = Config::load(ConfigSources {
                path: cli.config.clone(),
                env: std::env::vars().collect::<BTreeMap<_, _>>(),
                cli: ConfigOverride::default(),
            })
            .map_err(operation)?;
            Ok(redacted_config(&config))
        }
        Command::Migrate => migrate(&cli).await,
        Command::Backup { command } => backup(&cli, command).await,
        Command::DbReset { yes } => reset(&cli, *yes).await,
        Command::Seed => seed(&cli).await,
        Command::Openapi { output } => write_openapi(output),
        Command::SetupToken { command } => setup_token(&cli, command).await,
    }
}

fn write_openapi(output: &Path) -> Result<String, CliError> {
    fs::create_dir_all(parent_directory(output)).map_err(operation)?;
    fs::write(
        output,
        orbit_server::openapi::openapi_json().map_err(operation)?,
    )
    .map_err(operation)?;
    Ok(format!("wrote {}", output.display()))
}

async fn serve(cli: &Cli, listen: std::net::SocketAddr, origin: &str) -> Result<String, CliError> {
    let upload_limits = UploadLimits::new(
        cli.attachment_max_file_bytes,
        cli.attachment_max_request_bytes,
    )
    .map_err(operation)?;
    let database = open_database(&cli.database).await?;
    MigrationRunner::embedded(env!("CARGO_PKG_VERSION"))
        .run(&database)
        .await
        .map_err(operation)?;
    let identity = std::sync::Arc::new(IdentityRepository::new(database.clone()));
    let (auth_state, setup) = initialize_auth(
        std::sync::Arc::clone(&identity),
        CookieMode::secure(),
        std::sync::Arc::new(AdminRecoveryDelivery::new(128)),
        origin.to_owned(),
    )
    .await
    .map_err(operation)?;
    if let Some(setup) = setup {
        eprintln!("Initial setup URL: {}", setup.url);
    }
    let store: std::sync::Arc<dyn orbit_platform::BlobStore> =
        std::sync::Arc::new(LocalBlobStore::new(&cli.attachments));
    let workspaces = std::sync::Arc::new(WorkspaceRepository::with_blob_store(
        database.clone(),
        std::sync::Arc::clone(&store),
    ));
    let attachment_state = AttachmentState::new(
        std::sync::Arc::clone(&identity),
        UploadService::new(
            database,
            store,
            AttachmentMutationCoordinator::default(),
            upload_limits,
        ),
        CookieMode::secure(),
    );
    let app = auth_router(auth_state)
        .merge(workspace_router(WorkspaceState::with_repository(
            std::sync::Arc::clone(&identity),
            std::sync::Arc::clone(&workspaces),
            origin.to_owned(),
            CookieMode::secure(),
        )))
        .merge(task_router(TaskState::new(identity, CookieMode::secure())))
        .merge(attachment_router(attachment_state.clone()))
        .layer(
            HttpPlatformLayer::new(OriginPolicy::new(origin))
                .with_contract_id(orbit_server::openapi::CONTRACT_ID)
                .with_limits(HttpLimits {
                    max_body_bytes: usize::try_from(upload_limits.max_request_bytes())
                        .unwrap_or(usize::MAX),
                    ..HttpLimits::default()
                }),
        );
    let listener = tokio::net::TcpListener::bind(listen)
        .await
        .map_err(operation)?;
    let shutdown = CancellationToken::new();
    let signal_shutdown = shutdown.clone();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        signal_shutdown.cancel();
    });
    let maintenance_shutdown = shutdown.clone();
    let maintenance = tokio::spawn(async move {
        let result = workspaces
            .run_production_retention_service(maintenance_shutdown.clone())
            .await;
        if result.is_err() {
            maintenance_shutdown.cancel();
        }
        result
    });
    let attachment_shutdown = shutdown.clone();
    let attachment_maintenance = tokio::spawn(async move {
        let result = attachment_state
            .run_reconciliation_service(attachment_shutdown.clone())
            .await;
        if result.is_err() {
            attachment_shutdown.cancel();
        }
        result
    });
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown.clone().cancelled_owned())
        .await
        .map_err(operation)?;
    shutdown.cancel();
    maintenance.await.map_err(operation)?.map_err(operation)?;
    attachment_maintenance
        .await
        .map_err(operation)?
        .map_err(operation)?;
    Ok(String::new())
}

async fn setup_token(cli: &Cli, command: &SetupTokenCommand) -> Result<String, CliError> {
    let database = open_database(&cli.database).await?;
    MigrationRunner::embedded(env!("CARGO_PKG_VERSION"))
        .run(&database)
        .await
        .map_err(operation)?;
    let repository = IdentityRepository::new(database);
    let now = TimestampMillis::now();
    let (origin, issued) = match command {
        SetupTokenCommand::Init { origin } => {
            let issued = repository
                .initialize_setup_token(now)
                .await
                .map_err(operation)?;
            (origin, issued)
        }
        SetupTokenCommand::Rotate { origin } => {
            let issued = repository
                .rotate_setup_token(now)
                .await
                .map_err(operation)?;
            (origin, Some(issued))
        }
    };
    Ok(issued.map_or_else(
        || "setup token already exists or setup is complete".to_owned(),
        |issued| {
            format!(
                "{}/setup?token={}",
                origin.trim_end_matches('/'),
                issued.token
            )
        },
    ))
}

async fn migrate(cli: &Cli) -> Result<String, CliError> {
    let database = open_database(&cli.database).await?;
    let runner = MigrationRunner::embedded(env!("CARGO_PKG_VERSION"));
    let pending = runner.pending(&database).await.map_err(operation)?;
    if pending.iter().any(|migration| migration.destructive) {
        BackupService::new(&cli.backups, &cli.attachments)
            .create_pre_migration(&database)
            .await
            .map_err(operation)?;
    }
    let count = pending.len();
    runner.run(&database).await.map_err(operation)?;
    Ok(format!("applied {count} migration(s)"))
}

async fn backup(cli: &Cli, command: &BackupCommand) -> Result<String, CliError> {
    let service = BackupService::new(&cli.backups, &cli.attachments);
    match command {
        BackupCommand::Create => {
            let database = open_database(&cli.database).await?;
            let snapshot = service.create(&database).await.map_err(operation)?;
            Ok(snapshot.id)
        }
        BackupCommand::List { pre_migration } => {
            let snapshots = if *pre_migration {
                service.list_pre_migration().await
            } else {
                service.list().await
            }
            .map_err(operation)?;
            Ok(snapshots
                .into_iter()
                .map(|snapshot| {
                    format!(
                        "{}\t{}\t{:?}",
                        snapshot.id, snapshot.manifest.created_at, snapshot.manifest.kind
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"))
        }
        BackupCommand::Verify { id } => {
            service.verify(id).await.map_err(operation)?;
            Ok(format!("backup {id} verified"))
        }
        BackupCommand::Restore { id } => {
            service
                .restore(id, &cli.database)
                .await
                .map_err(operation)?;
            Ok(format!("backup {id} restored"))
        }
    }
}

async fn reset(cli: &Cli, yes: bool) -> Result<String, CliError> {
    if !yes {
        return Err(CliError::Operation(
            "db-reset requires --yes because it destroys all current data".to_owned(),
        ));
    }
    fs::create_dir_all(parent_directory(&cli.database)).map_err(operation)?;
    let database_file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&cli.database)
        .map_err(operation)?;
    database_file.try_lock_exclusive().map_err(|_| {
        CliError::Operation(format!(
            "database is owned by a serving Orbit process: {}",
            cli.database.display()
        ))
    })?;
    database_file.set_len(0).map_err(operation)?;
    database_file.sync_all().map_err(operation)?;
    remove_sidecars(&cli.database)?;
    drop(database_file);

    let database = open_database(&cli.database).await?;
    MigrationRunner::embedded(env!("CARGO_PKG_VERSION"))
        .run(&database)
        .await
        .map_err(operation)?;
    Ok("database reset complete".to_owned())
}

async fn seed(cli: &Cli) -> Result<String, CliError> {
    let database = open_database(&cli.database).await?;
    MigrationRunner::embedded(env!("CARGO_PKG_VERSION"))
        .run(&database)
        .await
        .map_err(operation)?;
    database
        .execute(
            "INSERT INTO installation_state (id, initialized, initialized_at) \
             VALUES (1, 1, CAST(strftime('%s', 'now') AS INTEGER) * 1000) \
             ON CONFLICT(id) DO UPDATE SET initialized = 1, \
             initialized_at = excluded.initialized_at",
        )
        .await
        .map_err(operation)?;
    Ok("seed data installed".to_owned())
}

async fn open_database(path: &Path) -> Result<Database, CliError> {
    Database::open(&DatabaseConfig::new(path))
        .await
        .map_err(operation)
}

fn remove_sidecars(path: &Path) -> Result<(), CliError> {
    for suffix in ["-wal", "-shm"] {
        let sidecar = PathBuf::from(format!("{}{suffix}", path.display()));
        match fs::remove_file(sidecar) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(operation(error)),
        }
    }
    Ok(())
}

fn parent_directory(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

pub fn redacted_config(config: &Config) -> String {
    let mut output = format!("[http]\nport = {}", config.http.port);
    if !config.secrets.is_empty() {
        output.push_str("\n\n[secrets]");
        for name in config.secrets.keys() {
            output.push_str(&format!("\n{name} = \"[REDACTED]\""));
        }
    }
    output
}

pub fn failure_message(error: &str) -> String {
    format!(
        "orbit: {error}\nRecovery: stop any serving Orbit process, verify the configured paths, and retry; no automatic repair was attempted."
    )
}

fn operation(error: impl std::fmt::Display) -> CliError {
    CliError::Operation(error.to_string())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;

    use clap::Parser;
    use orbit_platform::{Config, ConfigOverride, ConfigSources};

    use super::{
        BackupCommand, Cli, Command, ConfigCommand, SetupTokenCommand, failure_message,
        parent_directory, redacted_config, run,
    };

    #[test]
    fn parses_all_operations_subcommands() {
        assert!(matches!(
            Cli::try_parse_from(["orbit", "serve"]).unwrap().command,
            Command::Serve { .. }
        ));
        assert!(matches!(
            Cli::try_parse_from(["orbit", "config", "show"])
                .unwrap()
                .command,
            Command::Config {
                command: ConfigCommand::Show
            }
        ));
        assert!(matches!(
            Cli::try_parse_from(["orbit", "migrate"]).unwrap().command,
            Command::Migrate
        ));
        assert!(matches!(
            Cli::try_parse_from(["orbit", "backup", "create"])
                .unwrap()
                .command,
            Command::Backup {
                command: BackupCommand::Create
            }
        ));
        assert!(matches!(
            Cli::try_parse_from(["orbit", "db-reset", "--yes"])
                .unwrap()
                .command,
            Command::DbReset { yes: true }
        ));
        assert!(matches!(
            Cli::try_parse_from(["orbit", "seed"]).unwrap().command,
            Command::Seed
        ));
        assert!(matches!(
            Cli::try_parse_from(["orbit", "openapi", "--output", "openapi.json"])
                .unwrap()
                .command,
            Command::Openapi { .. }
        ));
        assert!(matches!(
            Cli::try_parse_from([
                "orbit",
                "setup-token",
                "rotate",
                "--origin",
                "https://orbit.test"
            ])
            .unwrap()
            .command,
            Command::SetupToken {
                command: SetupTokenCommand::Rotate { .. }
            }
        ));
    }

    #[test]
    fn parses_installation_attachment_upload_limits() {
        let cli = Cli::try_parse_from([
            "orbit",
            "--attachment-max-file-bytes",
            "1048576",
            "--attachment-max-request-bytes",
            "2097152",
            "serve",
        ])
        .unwrap();

        assert_eq!(cli.attachment_max_file_bytes, 1_048_576);
        assert_eq!(cli.attachment_max_request_bytes, 2_097_152);
    }

    #[tokio::test]
    async fn setup_token_commands_initialize_once_and_rotate_the_unused_secret() {
        let database = std::env::temp_dir().join(format!(
            "orbit-setup-token-{}-{}.sqlite",
            std::process::id(),
            orbit_platform::Id::new_v7()
        ));
        let database_arg = database.to_string_lossy().into_owned();
        let parse = |operation: &str| {
            Cli::try_parse_from([
                "orbit",
                "--database",
                database_arg.as_str(),
                "setup-token",
                operation,
                "--origin",
                "https://orbit.test",
            ])
            .unwrap()
        };

        let initial = run(parse("init")).await.unwrap();
        let repeated = run(parse("init")).await.unwrap();
        let rotated = run(parse("rotate")).await.unwrap();

        assert!(initial.starts_with("https://orbit.test/setup?token="));
        assert_eq!(repeated, "setup token already exists or setup is complete");
        assert!(rotated.starts_with("https://orbit.test/setup?token="));
        assert_ne!(initial, rotated);
        for suffix in ["", "-wal", "-shm"] {
            let _ = fs::remove_file(format!("{}{suffix}", database.display()));
        }
    }

    #[test]
    fn config_output_redacts_secret_values() {
        let path = std::env::temp_dir().join(format!(
            "orbit-cli-config-{}-{}.toml",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        fs::write(&path, "http.port = 9000\nsecrets.token = \"very-secret\"").unwrap();
        let config = Config::load(ConfigSources {
            path: path.clone(),
            env: BTreeMap::new(),
            cli: ConfigOverride::default(),
        })
        .unwrap();
        let _ = fs::remove_file(path);

        let shown = redacted_config(&config);

        assert!(shown.contains("port = 9000"));
        assert!(shown.contains("token = \"[REDACTED]\""));
        assert!(!shown.contains("very-secret"));
    }

    #[test]
    fn operational_failures_include_a_concise_recovery_message() {
        let message = failure_message("database is owned");

        assert!(message.contains("database is owned"));
        assert!(message.contains("Recovery:"));
    }

    #[test]
    fn a_bare_database_filename_uses_the_current_directory_as_its_parent() {
        assert_eq!(
            parent_directory(std::path::Path::new("orbit.sqlite")),
            std::path::Path::new(".")
        );
    }
}
