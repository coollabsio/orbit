use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use fs2::FileExt;
use orbit_platform::{
    BackupService, Config, ConfigOverride, ConfigSources, Database, DatabaseConfig, MigrationRunner,
};
use thiserror::Error;

#[derive(Debug, Parser)]
#[command(name = "orbit", about = "Orbit operations command-line interface")]
pub struct Cli {
    #[arg(long, global = true, default_value = "orbit.toml")]
    config: PathBuf,
    #[arg(long, global = true, default_value = "orbit.sqlite")]
    database: PathBuf,
    #[arg(long, global = true, default_value = "attachments")]
    attachments: PathBuf,
    #[arg(long, global = true, default_value = "backups")]
    backups: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
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

#[derive(Debug, Error)]
pub enum CliError {
    #[error("{0}")]
    Operation(String),
}

pub async fn run(cli: Cli) -> Result<String, CliError> {
    match &cli.command {
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
    }
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
        BackupCommand, Cli, Command, ConfigCommand, failure_message, parent_directory,
        redacted_config,
    };

    #[test]
    fn parses_all_operations_subcommands() {
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
