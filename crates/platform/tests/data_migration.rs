use std::sync::atomic::{AtomicUsize, Ordering};

use orbit_platform::{
    DataMigration, Database, DatabaseConfig, Migration, MigrationError, MigrationRunner,
};

static RUNS: AtomicUsize = AtomicUsize::new(0);
// A separate counter, because tests in one binary run concurrently and would
// otherwise race on a shared tally.
static RERUNS: AtomicUsize = AtomicUsize::new(0);

fn copy_names(database: &Database) -> orbit_platform::DataMigrationFuture<'_> {
    Box::pin(async move {
        RUNS.fetch_add(1, Ordering::SeqCst);
        database
            .execute("UPDATE widgets SET label = 'copied:' || name")
            .await
            .map_err(|error| error.to_string())?;
        Ok(())
    })
}

fn failing(_database: &Database) -> orbit_platform::DataMigrationFuture<'_> {
    Box::pin(async move { Err("backfill exploded".to_owned()) })
}

async fn database() -> (tempfile::TempDir, Database) {
    let directory = tempfile::tempdir().unwrap();
    let database = Database::open(&DatabaseConfig::new(directory.path().join("db.sqlite")))
        .await
        .unwrap();
    (directory, database)
}

fn runner(data_migrations: Vec<DataMigration>) -> MigrationRunner {
    MigrationRunner::new(
        "test",
        vec![
            Migration::new(
                1,
                "CREATE TABLE widgets (id INTEGER PRIMARY KEY, name TEXT NOT NULL);\
                 INSERT INTO widgets (id, name) VALUES (1, 'alpha');",
                false,
            ),
            Migration::new(2, "ALTER TABLE widgets ADD COLUMN label TEXT;", true),
            Migration::new(3, "ALTER TABLE widgets DROP COLUMN name;", true),
        ],
    )
    .with_data_migrations(data_migrations)
}

#[tokio::test]
async fn a_data_migration_runs_between_the_schema_versions_that_bracket_it() {
    RUNS.store(0, Ordering::SeqCst);
    let (_directory, database) = database().await;

    runner(vec![DataMigration {
        after_version: 2,
        run: copy_names,
    }])
    .run(&database)
    .await
    .unwrap();

    assert_eq!(RUNS.load(Ordering::SeqCst), 1);
    assert_eq!(
        database
            .scalar::<String>("SELECT label FROM widgets WHERE id = 1")
            .await
            .unwrap(),
        "copied:alpha"
    );
    assert_eq!(
        database
            .scalar::<i64>("SELECT COUNT(*) FROM pragma_table_info('widgets') WHERE name = 'name'")
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn run_through_stops_at_the_requested_version() {
    let (_directory, database) = database().await;

    runner(Vec::new()).run_through(&database, 2).await.unwrap();

    assert_eq!(
        database
            .scalar::<i64>("SELECT MAX(version) FROM schema_migrations")
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        database
            .scalar::<i64>("SELECT COUNT(*) FROM pragma_table_info('widgets') WHERE name = 'name'")
            .await
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn a_failing_data_migration_stops_the_run_before_the_next_schema_change() {
    let (_directory, database) = database().await;

    let error = runner(vec![DataMigration {
        after_version: 2,
        run: failing,
    }])
    .run(&database)
    .await
    .unwrap_err();

    assert!(matches!(
        error,
        MigrationError::DataMigration { version: 2, ref message } if message == "backfill exploded"
    ));
    assert_eq!(
        database
            .scalar::<i64>("SELECT MAX(version) FROM schema_migrations")
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        database
            .scalar::<i64>("SELECT COUNT(*) FROM pragma_table_info('widgets') WHERE name = 'name'")
            .await
            .unwrap(),
        1
    );
}

fn count_reruns(_database: &Database) -> orbit_platform::DataMigrationFuture<'_> {
    Box::pin(async move {
        RERUNS.fetch_add(1, Ordering::SeqCst);
        Ok(())
    })
}

#[tokio::test]
async fn a_data_migration_does_not_run_again_on_a_current_database() {
    let (_directory, database) = database().await;
    let runner = runner(vec![DataMigration {
        after_version: 2,
        run: count_reruns,
    }]);

    runner.run(&database).await.unwrap();
    runner.run(&database).await.unwrap();

    assert_eq!(RERUNS.load(Ordering::SeqCst), 1);
}
