//! The custom page origins of this server (#94): `ORBIT_FRAME_ORIGINS`, or else the list that the root user saves in
//! Admin. The CSP layer and the custom page routes share one [`FrameOrigins`] handle per database. A save reloads it
//! here and tells the other servers to reload theirs ([`crate::cluster::Message::FrameOriginsChanged`]).

use orbit_platform::{Database, FrameOrigins, frame_origin};

use crate::repositories::environment_settings::EnvironmentSettings;

/// The handle of `database`; empty until [`reload`] runs.
#[must_use]
pub fn of(database: &Database) -> FrameOrigins {
    (*database.extension(FrameOrigins::default)).clone()
}

/// Reads the list again: the environment's, or the saved one.
pub async fn reload(database: &Database) -> Result<(), sqlx::Error> {
    let origins = match &EnvironmentSettings::of(database).frame_origins {
        Some(origins) => origins.clone(),
        None => saved(database).await?,
    };
    of(database).set(origins);
    Ok(())
}

/// The list saved in Admin. Entries were checked when saved; one that no longer parses is skipped.
pub async fn saved(database: &Database) -> Result<Vec<String>, sqlx::Error> {
    let text: String =
        sqlx::query_scalar("SELECT frame_origins FROM instance_settings WHERE id = 1")
            .fetch_one(database.pool())
            .await?;
    Ok(text.lines().filter_map(frame_origin).collect())
}
