//! The migration runner every Orbit entry point uses. `MigrationRunner::embedded`
//! alone is not enough: migration 15 drops the Markdown columns, so the rich text
//! backfill has to be registered as a data migration that runs after 14.
use orbit_platform::MigrationRunner;

use crate::repositories::rich_text_backfill::RICH_TEXT_BACKFILL;

#[must_use]
pub fn migration_runner() -> MigrationRunner {
    MigrationRunner::embedded(env!("CARGO_PKG_VERSION"))
        .with_data_migrations(vec![RICH_TEXT_BACKFILL])
}
