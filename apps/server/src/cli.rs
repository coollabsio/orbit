use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use clap::{Parser, Subcommand};
use fs2::FileExt;
use orbit_platform::{
    BackupService, Config, ConfigOverride, ConfigSources, Database, DatabaseConfig,
    EnvironmentMode, MigrationRunner, TimestampMillis, run_guarded_migrations,
};
use orbit_server::app::App;
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::repositories::tasks::{CreateTask, TaskRepository};
use orbit_server::repositories::workspaces::WorkspaceRepository;
use thiserror::Error;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Parser)]
#[command(name = "orbit", about = "Orbit server and operations CLI")]
pub struct Cli {
    #[arg(long, global = true, default_value = "config/orbit.toml")]
    config: PathBuf,
    #[arg(long, global = true)]
    database: Option<PathBuf>,
    #[arg(long, global = true)]
    attachments: Option<PathBuf>,
    #[arg(long, global = true)]
    attachment_max_file_bytes: Option<u64>,
    #[arg(long, global = true)]
    attachment_max_request_bytes: Option<u64>,
    #[arg(long, global = true)]
    backups: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Serve {
        #[arg(long)]
        listen: Option<std::net::SocketAddr>,
        #[arg(long)]
        origin: Option<String>,
    },
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    Migrate {
        #[command(subcommand)]
        command: MigrateCommand,
    },
    Backup {
        #[command(subcommand)]
        command: BackupCommand,
    },
    DbReset {
        #[arg(long)]
        yes: bool,
    },
    Seed,
    RecoveryLink {
        #[arg(long)]
        email: String,
        #[arg(long)]
        origin: Option<String>,
    },
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
    Check,
    Show,
}

#[derive(Debug, Subcommand)]
pub enum MigrateCommand {
    Status,
    Run {
        #[arg(long)]
        reset: bool,
        #[arg(long)]
        seed: bool,
        #[arg(long)]
        yes: bool,
    },
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
        #[arg(long)]
        origin: Option<String>,
    },
    Rotate {
        #[arg(long)]
        origin: Option<String>,
    },
}

#[derive(Debug, Error)]
pub enum CliError {
    #[error("{0}")]
    Operation(String),
}

pub async fn run(cli: Cli) -> Result<String, CliError> {
    match &cli.command {
        Command::Serve { listen, origin } => serve(&cli, *listen, origin.as_deref()).await,
        Command::Config { command } => config_command(&cli, command),
        Command::Migrate { command } => migrate(&cli, command).await,
        Command::Backup { command } => backup(&cli, command).await,
        Command::DbReset { yes } => reset(&cli, *yes).await,
        Command::Seed => seed(&cli).await,
        Command::RecoveryLink { email, origin } => {
            recovery_link(&cli, email, origin.as_deref()).await
        }
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

fn config_command(cli: &Cli, command: &ConfigCommand) -> Result<String, CliError> {
    let config = load_config_file(cli)?;
    match command {
        ConfigCommand::Check => Ok(format!("configuration is valid: {}", cli.config.display())),
        ConfigCommand::Show => Ok(redacted_config(&config)),
    }
}

async fn serve(
    cli: &Cli,
    listen: Option<std::net::SocketAddr>,
    origin: Option<&str>,
) -> Result<String, CliError> {
    let mut config = effective_config(cli)?;
    if let Some(listen) = listen {
        config.http.bind = listen.ip();
        config.http.port = listen.port();
    }
    if let Some(origin) = origin {
        config.http.public_origin = origin.to_owned();
    }
    config.validate().map_err(operation)?;

    let app = App::build(config).await.map_err(operation)?;
    if let Some(url) = app.setup_url() {
        eprintln!("Initial setup URL: {url}");
    }
    let shutdown = CancellationToken::new();
    let signal = shutdown.clone();
    tokio::spawn(async move {
        shutdown_signal().await;
        signal.cancel();
    });
    app.run(shutdown).await.map_err(operation)?;
    Ok(String::new())
}

#[cfg(unix)]
async fn shutdown_signal() {
    use tokio::signal::unix::{SignalKind, signal};

    let mut terminate = signal(SignalKind::terminate()).expect("SIGTERM handler installs");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = terminate.recv() => {}
    }
}

#[cfg(not(unix))]
async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

async fn setup_token(cli: &Cli, command: &SetupTokenCommand) -> Result<String, CliError> {
    let config = effective_config(cli)?;
    let database = open_database(&config.data.database).await?;
    migrate_implicitly(&config, &database).await?;
    let repository = IdentityRepository::new(database);
    let now = TimestampMillis::now();
    let (origin, issued) = match command {
        SetupTokenCommand::Init { origin } => (
            origin.as_deref().unwrap_or(&config.http.public_origin),
            repository
                .initialize_setup_token(now)
                .await
                .map_err(operation)?,
        ),
        SetupTokenCommand::Rotate { origin } => (
            origin.as_deref().unwrap_or(&config.http.public_origin),
            Some(
                repository
                    .rotate_setup_token(now)
                    .await
                    .map_err(operation)?,
            ),
        ),
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

async fn migrate(cli: &Cli, command: &MigrateCommand) -> Result<String, CliError> {
    let config = effective_config(cli)?;
    let (reset, seed, yes) = match command {
        MigrateCommand::Status => {
            let database = open_database(&config.data.database).await?;
            let runner = MigrationRunner::embedded(env!("CARGO_PKG_VERSION"));
            let pending = runner.pending(&database).await.map_err(operation)?;
            return Ok(if pending.is_empty() {
                "database schema is current".to_owned()
            } else {
                format!(
                    "pending migrations: {}",
                    pending
                        .iter()
                        .map(|migration| migration.version.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            });
        }
        MigrateCommand::Run { reset, seed, yes } => (*reset, *seed, *yes),
    };
    if reset || seed {
        require_development(&config)?;
    }
    if reset && !yes {
        return Err(CliError::Operation(
            "migrate run --reset requires --yes because it destroys all current data".to_owned(),
        ));
    }
    if reset {
        reset_data(&config)?;
    }

    let database = open_database(&config.data.database).await?;
    let runner = MigrationRunner::embedded(env!("CARGO_PKG_VERSION"));
    let count = run_guarded_migrations(&config, &database, &runner)
        .await
        .map_err(operation)?;
    let mut output = format!("applied {count} migration(s)");
    if seed {
        output.push('\n');
        output.push_str(&install_seed_data(database).await?);
    }
    Ok(output)
}

async fn backup(cli: &Cli, command: &BackupCommand) -> Result<String, CliError> {
    let config = effective_config(cli)?;
    let service = BackupService::new(&config.data.backups, &config.data.attachments);
    match command {
        BackupCommand::Create => {
            let database = open_database(&config.data.database).await?;
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
                .restore(id, &config.data.database)
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
    let config = effective_config(cli)?;
    require_development(&config)?;
    reset_data(&config)?;

    let database = open_database(&config.data.database).await?;
    migrate_implicitly(&config, &database).await?;
    Ok("database reset complete".to_owned())
}

fn reset_data(config: &Config) -> Result<(), CliError> {
    validate_reset_paths(config)?;
    let path = &config.data.database;
    fs::create_dir_all(parent_directory(path)).map_err(operation)?;
    let database_file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(path)
        .map_err(operation)?;
    database_file.try_lock_exclusive().map_err(|_| {
        CliError::Operation(format!(
            "database is owned by a serving Orbit process: {}",
            path.display()
        ))
    })?;
    database_file.set_len(0).map_err(operation)?;
    database_file.sync_all().map_err(operation)?;
    remove_sidecars(path)?;
    drop(database_file);
    match fs::remove_dir_all(&config.data.attachments) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(operation(error)),
    }
    Ok(())
}

async fn seed(cli: &Cli) -> Result<String, CliError> {
    let config = effective_config(cli)?;
    require_development(&config)?;
    let database = open_database(&config.data.database).await?;
    migrate_implicitly(&config, &database).await?;
    install_seed_data(database).await
}

async fn recovery_link(cli: &Cli, email: &str, origin: Option<&str>) -> Result<String, CliError> {
    let config = effective_config(cli)?;
    let database = open_database(&config.data.database).await?;
    migrate_implicitly(&config, &database).await?;
    let repository = IdentityRepository::new(database);
    let identity = repository
        .find_by_email(email)
        .await
        .map_err(operation)?
        .filter(|identity| !identity.suspended)
        .ok_or_else(|| CliError::Operation("active account not found".to_owned()))?;
    let token = orbit_platform::generate_opaque_token();
    let now = TimestampMillis::now();
    let expires_at = TimestampMillis::from_millis(
        now.as_millis() + Duration::from_secs(30 * 60).as_millis() as i64,
    );
    repository
        .store_recovery_token_audited(identity.id, &token, expires_at, "operator-cli")
        .await
        .map_err(operation)?;
    Ok(format!(
        "{}/recovery?token={token}",
        origin
            .unwrap_or(&config.http.public_origin)
            .trim_end_matches('/')
    ))
}

async fn migrate_implicitly(config: &Config, database: &Database) -> Result<(), CliError> {
    run_guarded_migrations(
        config,
        database,
        &MigrationRunner::embedded(env!("CARGO_PKG_VERSION")),
    )
    .await
    .map(|_| ())
    .map_err(operation)
}

fn require_development(config: &Config) -> Result<(), CliError> {
    if config.environment == EnvironmentMode::Development {
        Ok(())
    } else {
        Err(CliError::Operation(
            "this destructive data command is available only in development mode".to_owned(),
        ))
    }
}

async fn install_seed_data(database: Database) -> Result<String, CliError> {
    const EMAIL: &str = "test@example.com";
    const PASSWORD_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$WUH9bzy7j6QyIqQ+iO+WUA$E+J7gy74Md7ZnwnccCH7oMqGdoj/u2SDc5QLvOzKMpA";
    const TASK_TITLE: &str = "Review the Orbit foundation";
    const TASK_DESCRIPTION: &str = "A safe, idempotent development seed record.";
    let identity = IdentityRepository::new(database.clone());
    let now = TimestampMillis::now();
    let existing_developer = identity.find_by_email(EMAIL).await.map_err(operation)?;
    let setup =
        if existing_developer.is_none() && !identity.setup_complete().await.map_err(operation)? {
            let token = match identity
                .initialize_setup_token(now)
                .await
                .map_err(operation)?
            {
                Some(token) => token,
                None => identity.rotate_setup_token(now).await.map_err(operation)?,
            };
            Some(
                identity
                    .complete_setup(
                        SetupRequest {
                            token: token.token,
                            email: EMAIL.to_owned(),
                            display_name: "Orbit Developer".to_owned(),
                            password_hash: PASSWORD_HASH.to_owned(),
                            workspace_name: "Orbit Development".to_owned(),
                            project_name: "Foundation".to_owned(),
                        },
                        now,
                    )
                    .await
                    .map_err(operation)?,
            )
        } else {
            None
        };

    let user_id = match setup.as_ref() {
        Some(setup) => setup.user_id,
        None => match existing_developer {
            Some(developer) => developer.id,
            None => {
                let user_id = orbit_platform::Id::new_v7();
                sqlx::query(
                    "INSERT INTO users (id, email, normalized_email, display_name, password_hash, \
                     installation_admin, created_at, updated_at) VALUES (?, ?, ?, ?, ?, 1, ?, ?)",
                )
                .bind(user_id.to_string())
                .bind(EMAIL)
                .bind(EMAIL)
                .bind("Orbit Developer")
                .bind(PASSWORD_HASH)
                .bind(now.as_millis())
                .bind(now.as_millis())
                .execute(database.pool())
                .await
                .map_err(operation)?;
                user_id
            }
        },
    };
    let workspace_id = match setup.as_ref() {
        Some(setup) => setup.workspace_id,
        None => {
            let workspace = sqlx::query_scalar::<_, String>(
                "SELECT workspace_id FROM memberships WHERE user_id = ? ORDER BY created_at LIMIT 1",
            )
            .bind(user_id.to_string())
            .fetch_optional(database.pool())
            .await
            .map_err(operation)?;
            match workspace {
                Some(workspace) => workspace.parse().map_err(operation)?,
                None => {
                    WorkspaceRepository::new(database.clone())
                        .create(
                            user_id,
                            "Orbit Development".to_owned(),
                            "development-seed",
                            now,
                        )
                        .await
                        .map_err(operation)?
                        .id
                }
            }
        }
    };
    let project_id = match setup.as_ref() {
        Some(setup) => setup.project_id,
        None => sqlx::query_scalar::<_, String>(
            "SELECT id FROM projects WHERE workspace_id = ? ORDER BY created_at LIMIT 1",
        )
        .bind(workspace_id.to_string())
        .fetch_one(database.pool())
        .await
        .map_err(operation)?
        .parse()
        .map_err(operation)?,
    };
    let already_seeded: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tasks \
         JOIN audit_events ON audit_events.resource_id = tasks.id \
         WHERE tasks.workspace_id = ? AND audit_events.action = 'task.created' \
         AND audit_events.resource_type = 'task' AND audit_events.request_id = 'development-seed'",
    )
    .bind(workspace_id.to_string())
    .fetch_one(database.pool())
    .await
    .map_err(operation)?;
    if already_seeded == 0 {
        let status_id: String = sqlx::query_scalar(
            "SELECT id FROM task_statuses WHERE project_id = ? ORDER BY position LIMIT 1",
        )
        .bind(project_id.to_string())
        .fetch_one(database.pool())
        .await
        .map_err(operation)?;
        TaskRepository::new(database)
            .create_task(
                workspace_id,
                user_id,
                CreateTask {
                    project_id,
                    status_id: status_id.parse().map_err(operation)?,
                    title: TASK_TITLE.to_owned(),
                    description: TASK_DESCRIPTION.to_owned(),
                    priority: "medium".to_owned(),
                    position: None,
                    assignee_ids: vec![user_id],
                    label_ids: Vec::new(),
                    due_at: None,
                },
                "development-seed",
                now,
            )
            .await
            .map_err(operation)?;
    }
    Ok("development seed data installed\nemail: test@example.com\npassword: password".to_owned())
}

fn load_config_file(cli: &Cli) -> Result<Config, CliError> {
    let mut config = Config::load(ConfigSources {
        path: cli.config.clone(),
        env: std::env::vars().collect::<BTreeMap<_, _>>(),
        cli: ConfigOverride::default(),
    })
    .map_err(operation)?;
    apply_path_overrides(cli, &mut config);
    config.validate().map_err(operation)?;
    Ok(config)
}

fn effective_config(cli: &Cli) -> Result<Config, CliError> {
    if cli.config.exists() {
        return load_config_file(cli);
    }
    let mut config = Config::from_environment(std::env::vars().collect()).map_err(operation)?;
    apply_path_overrides(cli, &mut config);
    config.validate().map_err(operation)?;
    Ok(config)
}

fn apply_path_overrides(cli: &Cli, config: &mut Config) {
    if let Some(path) = &cli.database {
        config.data.database = path.clone();
    }
    if let Some(path) = &cli.attachments {
        config.data.attachments = path.clone();
    }
    if let Some(path) = &cli.backups {
        config.data.backups = path.clone();
    }
    if let Some(bytes) = cli.attachment_max_file_bytes {
        config.uploads.max_file_bytes = bytes;
    }
    if let Some(bytes) = cli.attachment_max_request_bytes {
        config.uploads.max_request_bytes = bytes;
    }
}

async fn open_database(path: &Path) -> Result<Database, CliError> {
    Database::open(&DatabaseConfig::new(path))
        .await
        .map_err(operation)
}

fn remove_sidecars(path: &Path) -> Result<(), CliError> {
    for suffix in ["-wal", "-shm", "-journal"] {
        let sidecar = PathBuf::from(format!("{}{suffix}", path.display()));
        match fs::remove_file(sidecar) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(operation(error)),
        }
    }
    Ok(())
}

fn validate_reset_paths(config: &Config) -> Result<(), CliError> {
    let database = canonicalize_future_path(&config.data.database)?;
    let attachments = canonicalize_future_path(&config.data.attachments)?;
    let backups = canonicalize_future_path(&config.data.backups)?;
    let current_directory =
        fs::canonicalize(std::env::current_dir().map_err(operation)?).map_err(operation)?;
    let unsafe_layout = attachments.parent().is_none()
        || current_directory.starts_with(&attachments)
        || database.starts_with(&attachments)
        || attachments == backups
        || attachments.starts_with(&backups)
        || backups.starts_with(&attachments);
    if unsafe_layout {
        return Err(CliError::Operation(format!(
            "unsafe attachment path for reset: {}",
            config.data.attachments.display()
        )));
    }
    Ok(())
}

fn canonicalize_future_path(path: &Path) -> Result<PathBuf, CliError> {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir().map_err(operation)?.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return Err(CliError::Operation(format!(
                        "unsafe attachment path for reset: {}",
                        path.display()
                    )));
                }
            }
        }
    }
    let mut existing = normalized.clone();
    let mut missing = Vec::<OsString>::new();
    while !existing.exists() {
        let name = existing.file_name().ok_or_else(|| {
            CliError::Operation(format!(
                "unsafe attachment path for reset: {}",
                path.display()
            ))
        })?;
        missing.push(name.to_owned());
        existing.pop();
    }
    let mut resolved = fs::canonicalize(&existing).map_err(operation)?;
    for component in missing.into_iter().rev() {
        resolved.push(component);
    }
    Ok(resolved)
}

fn parent_directory(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

pub fn redacted_config(config: &Config) -> String {
    let proxies = config
        .http
        .trusted_proxies
        .iter()
        .map(|proxy| format!("\"{proxy}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let mut output = format!(
        "environment = \"{}\"\n\n[http]\nbind = \"{}\"\nport = {}\npublic_origin = \"{}\"\ntrusted_proxies = [{}]\n\n[data]\ndatabase = \"{}\"\nattachments = \"{}\"\nbackups = \"{}\"\n\n[jobs]\nconcurrency = {}\n\n[uploads]\nmax_file_bytes = {}\nmax_request_bytes = {}\n\n[rate_limits]\nauthentication_per_minute = {}\nrecovery_per_minute = {}\ninvitation_per_minute = {}\nupload_per_minute = {}\ngeneral_per_minute = {}",
        config.environment.as_str(),
        config.http.bind,
        config.http.port,
        config.http.public_origin,
        proxies,
        config.data.database.display(),
        config.data.attachments.display(),
        config.data.backups.display(),
        config.jobs.concurrency,
        config.uploads.max_file_bytes,
        config.uploads.max_request_bytes,
        config.rate_limits.authentication_per_minute,
        config.rate_limits.recovery_per_minute,
        config.rate_limits.invitation_per_minute,
        config.rate_limits.upload_per_minute,
        config.rate_limits.general_per_minute,
    );
    output.push_str("\n\n[metrics]\n");
    if let Some(listen) = config.metrics.listen {
        output.push_str(&format!("listen = \"{listen}\""));
    } else {
        output.push_str("# listen is disabled");
    }
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
    use std::path::{Path, PathBuf};

    use clap::Parser;
    use orbit_platform::{
        BackupService, Config, ConfigOverride, ConfigSources, Database, DatabaseConfig,
        EnvironmentMode, Migration, MigrationRunner, TimestampMillis,
    };
    use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};

    use super::{
        BackupCommand, Cli, Command, ConfigCommand, MigrateCommand, SetupTokenCommand,
        failure_message, parent_directory, redacted_config, remove_sidecars, run,
        validate_reset_paths,
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
            Cli::try_parse_from(["orbit", "config", "check"])
                .unwrap()
                .command,
            Command::Config {
                command: ConfigCommand::Check
            }
        ));
        assert!(matches!(
            Cli::try_parse_from(["orbit", "migrate", "status"])
                .unwrap()
                .command,
            Command::Migrate {
                command: MigrateCommand::Status
            }
        ));
        assert!(matches!(
            Cli::try_parse_from(["orbit", "migrate", "run"])
                .unwrap()
                .command,
            Command::Migrate {
                command: MigrateCommand::Run {
                    reset: false,
                    seed: false,
                    yes: false,
                }
            }
        ));
        assert!(matches!(
            Cli::try_parse_from(["orbit", "migrate", "run", "--reset", "--seed", "--yes"])
                .unwrap()
                .command,
            Command::Migrate {
                command: MigrateCommand::Run {
                    reset: true,
                    seed: true,
                    yes: true,
                }
            }
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
            Cli::try_parse_from(["orbit", "recovery-link", "--email", "owner@example.com"])
                .unwrap()
                .command,
            Command::RecoveryLink { .. }
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
    fn parses_upload_limit_overrides_for_compatible_deployments() {
        let cli = Cli::try_parse_from([
            "orbit",
            "--attachment-max-file-bytes",
            "1024",
            "--attachment-max-request-bytes",
            "4096",
            "serve",
        ])
        .unwrap();

        assert_eq!(cli.attachment_max_file_bytes, Some(1024));
        assert_eq!(cli.attachment_max_request_bytes, Some(4096));
    }

    #[tokio::test]
    async fn setup_token_commands_initialize_once_and_rotate_the_unused_secret() {
        let database = std::env::temp_dir().join(format!(
            "orbit-setup-token-{}-{}.sqlite",
            std::process::id(),
            orbit_platform::Id::new_v7()
        ));
        let config = database.with_extension("toml");
        fs::write(&config, "environment = \"development\"\n").unwrap();
        let database_arg = database.to_string_lossy().into_owned();
        let config_arg = config.to_string_lossy().into_owned();
        let parse = |operation: &str| {
            Cli::try_parse_from([
                "orbit",
                "--config",
                config_arg.as_str(),
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
        let _ = fs::remove_file(config);
    }

    #[tokio::test]
    async fn recovery_link_command_issues_a_valid_operator_delivered_token() {
        let database_path = std::env::temp_dir().join(format!(
            "orbit-recovery-link-{}-{}.sqlite",
            std::process::id(),
            orbit_platform::Id::new_v7()
        ));
        let database = Database::open(&DatabaseConfig::new(&database_path))
            .await
            .unwrap();
        MigrationRunner::embedded(env!("CARGO_PKG_VERSION"))
            .run(&database)
            .await
            .unwrap();
        let repository = IdentityRepository::new(database.clone());
        let now = TimestampMillis::now();
        let setup = repository
            .initialize_setup_token(now)
            .await
            .unwrap()
            .unwrap();
        repository
            .complete_setup(
                SetupRequest {
                    token: setup.token,
                    email: "owner@example.com".to_owned(),
                    display_name: "Owner".to_owned(),
                    password_hash: "test-password-hash".to_owned(),
                    workspace_name: "Orbit".to_owned(),
                    project_name: "Tasks".to_owned(),
                },
                now,
            )
            .await
            .unwrap();
        drop(repository);
        drop(database);

        let database_arg = database_path.to_string_lossy().into_owned();
        let config_path = database_path.with_extension("toml");
        fs::write(&config_path, "environment = \"development\"\n").unwrap();
        let config_arg = config_path.to_string_lossy().into_owned();
        let cli = Cli::try_parse_from([
            "orbit",
            "--config",
            config_arg.as_str(),
            "--database",
            database_arg.as_str(),
            "recovery-link",
            "--email",
            "owner@example.com",
            "--origin",
            "https://orbit.test",
        ])
        .unwrap();
        let url = run(cli).await.unwrap();
        let token = url
            .strip_prefix("https://orbit.test/recovery?token=")
            .unwrap();

        let database = Database::open(&DatabaseConfig::new(&database_path))
            .await
            .unwrap();
        assert!(
            IdentityRepository::new(database)
                .recovery_token_valid(token, TimestampMillis::now())
                .await
                .unwrap()
        );
        for suffix in ["", "-wal", "-shm"] {
            let _ = fs::remove_file(format!("{}{suffix}", database_path.display()));
        }
        let _ = fs::remove_file(config_path);
    }

    #[test]
    fn config_output_redacts_secret_values() {
        let path = std::env::temp_dir().join(format!(
            "orbit-cli-config-{}-{}.toml",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        fs::write(
            &path,
            "environment = \"development\"\nhttp.port = 9000\nsecrets.token = \"very-secret\"",
        )
        .unwrap();
        let config = Config::load(ConfigSources {
            path: path.clone(),
            env: BTreeMap::new(),
            cli: ConfigOverride::default(),
        })
        .unwrap();
        let _ = fs::remove_file(path);

        let shown = redacted_config(&config);

        assert!(shown.contains("port = 9000"));
        assert!(shown.contains("[metrics]"));
        assert!(shown.contains("# listen is disabled"));
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

    #[test]
    fn reset_sidecar_cleanup_removes_wal_shm_and_rollback_journal_files() {
        let root = tempfile::TempDir::new().unwrap();
        let database = root.path().join("orbit.sqlite");
        for suffix in ["-wal", "-shm", "-journal"] {
            fs::write(format!("{}{suffix}", database.display()), b"stale").unwrap();
        }

        remove_sidecars(&database).unwrap();

        for suffix in ["-wal", "-shm", "-journal"] {
            assert!(!Path::new(&format!("{}{suffix}", database.display())).exists());
        }
    }

    #[test]
    fn reset_path_validation_rejects_filesystem_root_and_current_directory() {
        let root = tempfile::TempDir::new().unwrap();
        let mut config = Config {
            environment: EnvironmentMode::Development,
            ..Config::default()
        };
        config.data.database = root.path().join("data/orbit.sqlite");
        config.data.backups = root.path().join("backups");

        for attachments in [PathBuf::from("."), PathBuf::from("/")] {
            config.data.attachments = attachments;
            assert!(
                validate_reset_paths(&config)
                    .unwrap_err()
                    .to_string()
                    .contains("unsafe attachment path")
            );
        }
    }

    #[tokio::test]
    async fn destructive_implicit_migration_creates_a_verified_guard_backup() {
        let root = tempfile::TempDir::new().unwrap();
        let mut config = Config {
            environment: EnvironmentMode::Development,
            ..Config::default()
        };
        config.data.database = root.path().join("data/orbit.sqlite");
        config.data.attachments = root.path().join("data/attachments");
        config.data.backups = root.path().join("backups");
        fs::create_dir_all(&config.data.attachments).unwrap();
        let database = Database::open(&DatabaseConfig::new(&config.data.database))
            .await
            .unwrap();
        let runner = MigrationRunner::new(
            "test",
            vec![Migration::new(
                1,
                "CREATE TABLE guarded_migration (id INTEGER PRIMARY KEY);",
                true,
            )],
        );

        orbit_platform::run_guarded_migrations(&config, &database, &runner)
            .await
            .unwrap();

        let backups = BackupService::new(&config.data.backups, &config.data.attachments)
            .list_pre_migration()
            .await
            .unwrap();
        assert_eq!(backups.len(), 1);
        BackupService::new(&config.data.backups, &config.data.attachments)
            .verify(&backups[0].id)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn migrate_run_seed_installs_exact_credentials_and_preserves_edits() {
        let root = tempfile::TempDir::new().unwrap();
        let database = root.path().join("orbit.sqlite");
        let config_path = root.path().join("orbit.toml");
        fs::write(&config_path, "environment = \"development\"\n").unwrap();
        let args = || {
            Cli::try_parse_from([
                "orbit",
                "--config",
                config_path.to_str().unwrap(),
                "--database",
                database.to_str().unwrap(),
                "migrate",
                "run",
                "--seed",
            ])
            .unwrap()
        };

        let prepared = Database::open(&DatabaseConfig::new(&database))
            .await
            .unwrap();
        MigrationRunner::embedded(env!("CARGO_PKG_VERSION"))
            .run(&prepared)
            .await
            .unwrap();
        IdentityRepository::new(prepared.clone())
            .initialize_setup_token(TimestampMillis::now())
            .await
            .unwrap()
            .unwrap();
        drop(prepared);

        let first = run(args()).await.unwrap();
        assert!(first.contains("test@example.com"));
        assert!(first.contains("password"));

        let prepared = Database::open(&DatabaseConfig::new(&database))
            .await
            .unwrap();
        sqlx::query("UPDATE users SET display_name = 'Edited Developer'")
            .execute(prepared.pool())
            .await
            .unwrap();
        let edited_status: String =
            sqlx::query_scalar("SELECT id FROM task_statuses ORDER BY position DESC LIMIT 1")
                .fetch_one(prepared.pool())
                .await
                .unwrap();
        sqlx::query(
            "UPDATE tasks SET title = 'Edited task title', description = 'Edited description', \
             priority = 'urgent', position = 47, status_id = ?",
        )
        .bind(edited_status)
        .execute(prepared.pool())
        .await
        .unwrap();
        drop(prepared);

        let second = run(args()).await.unwrap();
        assert_eq!(first, second);

        let database = Database::open(&DatabaseConfig::new(database))
            .await
            .unwrap();
        assert_eq!(
            database
                .scalar::<i64>("SELECT COUNT(*) FROM users")
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            database
                .scalar::<i64>("SELECT COUNT(*) FROM workspaces")
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            database
                .scalar::<i64>("SELECT COUNT(*) FROM tasks")
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            database
                .scalar::<i64>(
                    "SELECT COUNT(*) FROM audit_events WHERE action = 'task.created' \
                     AND resource_type = 'task' AND request_id = 'development-seed'",
                )
                .await
                .unwrap(),
            1
        );
        let developer = IdentityRepository::new(database.clone())
            .find_by_email("test@example.com")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(developer.display_name, "Edited Developer");
        assert!(
            orbit_platform::PasswordService::default()
                .verify("password", &developer.password_hash)
                .unwrap()
                .valid
        );
        assert_eq!(
            database
                .scalar::<String>(
                    "SELECT title || '|' || description || '|' || priority || '|' || position \
                     FROM tasks",
                )
                .await
                .unwrap(),
            "Edited task title|Edited description|urgent|47"
        );
    }

    #[tokio::test]
    async fn migrate_run_seed_adds_development_data_without_changing_initialized_data() {
        let root = tempfile::TempDir::new().unwrap();
        let database_path = root.path().join("orbit.sqlite");
        let database = Database::open(&DatabaseConfig::new(&database_path))
            .await
            .unwrap();
        MigrationRunner::embedded(env!("CARGO_PKG_VERSION"))
            .run(&database)
            .await
            .unwrap();
        let identity = IdentityRepository::new(database.clone());
        let now = TimestampMillis::now();
        let setup_token = identity.initialize_setup_token(now).await.unwrap().unwrap();
        let original_hash = orbit_platform::PasswordService::default()
            .hash("an existing secure password")
            .unwrap();
        let original = identity
            .complete_setup(
                SetupRequest {
                    token: setup_token.token,
                    email: "owner@example.com".to_owned(),
                    display_name: "Existing Owner".to_owned(),
                    password_hash: original_hash.clone(),
                    workspace_name: "Existing Workspace".to_owned(),
                    project_name: "Existing Project".to_owned(),
                },
                now,
            )
            .await
            .unwrap();
        drop(identity);
        drop(database);

        let config_path = root.path().join("orbit.toml");
        fs::write(&config_path, "environment = \"development\"\n").unwrap();
        let cli = Cli::try_parse_from([
            "orbit",
            "--config",
            config_path.to_str().unwrap(),
            "--database",
            database_path.to_str().unwrap(),
            "migrate",
            "run",
            "--seed",
        ])
        .unwrap();

        run(cli).await.unwrap();

        let database = Database::open(&DatabaseConfig::new(database_path))
            .await
            .unwrap();
        let identity = IdentityRepository::new(database.clone());
        let owner = identity
            .find_by_email("owner@example.com")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(owner.id, original.user_id);
        assert_eq!(owner.display_name, "Existing Owner");
        assert_eq!(owner.password_hash, original_hash);
        assert!(
            identity
                .find_by_email("test@example.com")
                .await
                .unwrap()
                .is_some()
        );
        assert_eq!(
            database
                .scalar::<i64>("SELECT COUNT(*) FROM users")
                .await
                .unwrap(),
            2
        );
        assert_eq!(
            database
                .scalar::<i64>("SELECT COUNT(*) FROM workspaces")
                .await
                .unwrap(),
            2
        );
        assert_eq!(
            sqlx::query_scalar::<_, String>("SELECT name FROM workspaces WHERE id = ?")
                .bind(original.workspace_id.to_string())
                .fetch_one(database.pool())
                .await
                .unwrap(),
            "Existing Workspace"
        );
    }

    #[tokio::test]
    async fn migrate_run_reset_requires_yes_and_development_mode() {
        let root = tempfile::TempDir::new().unwrap();
        let database = root.path().join("orbit.sqlite");
        fs::write(&database, b"keep-me").unwrap();
        let development_config = root.path().join("development.toml");
        fs::write(&development_config, "environment = \"development\"\n").unwrap();
        let without_yes = Cli::try_parse_from([
            "orbit",
            "--config",
            development_config.to_str().unwrap(),
            "--database",
            database.to_str().unwrap(),
            "migrate",
            "run",
            "--reset",
        ])
        .unwrap();

        let error = run(without_yes).await.unwrap_err();
        assert!(error.to_string().contains("requires --yes"));
        assert_eq!(fs::read(&database).unwrap(), b"keep-me");

        let production_config = root.path().join("production.toml");
        fs::write(
            &production_config,
            "environment = \"production\"\nhttp.public_origin = \"https://orbit.example\"\nhttp.trusted_proxies = [\"127.0.0.1/32\"]\n",
        )
        .unwrap();
        for flag in ["--reset", "--seed"] {
            let cli = Cli::try_parse_from([
                "orbit",
                "--config",
                production_config.to_str().unwrap(),
                "--database",
                database.to_str().unwrap(),
                "migrate",
                "run",
                flag,
                "--yes",
            ])
            .unwrap();
            let error = run(cli).await.unwrap_err();
            assert!(error.to_string().contains("development mode"));
            assert_eq!(fs::read(&database).unwrap(), b"keep-me");
        }
    }

    #[tokio::test]
    async fn migrate_run_reset_then_seed_recreates_data_and_clears_attachments() {
        let root = tempfile::TempDir::new().unwrap();
        let database = root.path().join("orbit.sqlite");
        let attachments = root.path().join("attachments");
        fs::create_dir_all(&attachments).unwrap();
        fs::write(attachments.join("old.txt"), b"old attachment").unwrap();
        fs::write(&database, b"old database").unwrap();
        fs::write(format!("{}-wal", database.display()), b"old wal").unwrap();
        fs::write(format!("{}-shm", database.display()), b"old shm").unwrap();
        fs::write(format!("{}-journal", database.display()), b"old journal").unwrap();
        let config_path = root.path().join("orbit.toml");
        fs::write(&config_path, "environment = \"development\"\n").unwrap();
        let cli = Cli::try_parse_from([
            "orbit",
            "--config",
            config_path.to_str().unwrap(),
            "--database",
            database.to_str().unwrap(),
            "--attachments",
            attachments.to_str().unwrap(),
            "migrate",
            "run",
            "--reset",
            "--seed",
            "--yes",
        ])
        .unwrap();

        let output = run(cli).await.unwrap();

        assert!(output.contains("applied"));
        assert!(output.contains("test@example.com"));
        assert!(!attachments.join("old.txt").exists());
        for (suffix, old_contents) in [
            ("-wal", b"old wal".as_slice()),
            ("-shm", b"old shm".as_slice()),
            ("-journal", b"old journal".as_slice()),
        ] {
            let sidecar = format!("{}{suffix}", database.display());
            if Path::new(&sidecar).exists() {
                assert_ne!(fs::read(sidecar).unwrap(), old_contents);
            }
        }
        let database = Database::open(&DatabaseConfig::new(database))
            .await
            .unwrap();
        assert_eq!(
            database
                .scalar::<i64>(
                    "SELECT COUNT(*) FROM users WHERE normalized_email = 'test@example.com'"
                )
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            database
                .scalar::<i64>("SELECT COUNT(*) FROM tasks")
                .await
                .unwrap(),
            1
        );
    }

    #[tokio::test]
    async fn migrate_run_reset_rejects_unsafe_attachment_layout_before_changing_data() {
        for overlap in ["database", "backups"] {
            let root = tempfile::TempDir::new().unwrap();
            let database = root.path().join("live/orbit.sqlite");
            let backups = root.path().join("backups");
            fs::create_dir_all(database.parent().unwrap()).unwrap();
            fs::create_dir_all(&backups).unwrap();
            fs::write(&database, b"keep database").unwrap();
            fs::write(backups.join("keep backup"), b"keep backup").unwrap();
            let attachments = if overlap == "database" {
                database.parent().unwrap().to_owned()
            } else {
                let alias_parent = root.path().join("alias");
                fs::create_dir_all(&alias_parent).unwrap();
                alias_parent.join("../backups")
            };
            let config_path = root.path().join("orbit.toml");
            fs::write(&config_path, "environment = \"development\"\n").unwrap();
            let cli = Cli::try_parse_from([
                "orbit",
                "--config",
                config_path.to_str().unwrap(),
                "--database",
                database.to_str().unwrap(),
                "--attachments",
                attachments.to_str().unwrap(),
                "--backups",
                backups.to_str().unwrap(),
                "migrate",
                "run",
                "--reset",
                "--yes",
            ])
            .unwrap();

            let error = run(cli).await.unwrap_err();

            assert!(error.to_string().contains("unsafe attachment path"));
            assert_eq!(fs::read(&database).unwrap(), b"keep database");
            assert_eq!(
                fs::read(backups.join("keep backup")).unwrap(),
                b"keep backup"
            );
        }
    }

    #[tokio::test]
    async fn destructive_development_commands_reject_production_mode() {
        let root = tempfile::TempDir::new().unwrap();
        let database = root.path().join("orbit.sqlite");
        fs::write(&database, b"keep-me").unwrap();
        let config_path = root.path().join("orbit.toml");
        fs::write(
            &config_path,
            "environment = \"production\"\nhttp.public_origin = \"https://orbit.example\"\nhttp.trusted_proxies = [\"127.0.0.1/32\"]\n",
        )
        .unwrap();
        let cli = Cli::try_parse_from([
            "orbit",
            "--config",
            config_path.to_str().unwrap(),
            "--database",
            database.to_str().unwrap(),
            "db-reset",
            "--yes",
        ])
        .unwrap();

        let error = run(cli).await.unwrap_err();

        assert!(error.to_string().contains("development mode"));
        assert_eq!(fs::read(database).unwrap(), b"keep-me");
    }
}
