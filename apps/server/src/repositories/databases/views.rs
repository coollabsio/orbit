//! A database's views: the repository methods that create, change and delete them, and the
//! queries that read and write them.

use orbit_platform::Row;
use orbit_platform::{Id, TimestampMillis};
use sqlx::{Any, Transaction};

use super::{
    DatabaseError, DatabaseRepository, DatabaseView, MAX_LINK_VIEWS, MAX_VIEWS, NewView, Property,
    ViewChanges, ViewConfig, ViewType, Views, insert_index, parse_id, properties_in_tx, record,
    renumber_rows, writable_views,
};
use super::{board, templates};

impl DatabaseRepository {
    pub async fn create_view(
        &self,
        workspace_id: Id,
        database_id: Id,
        actor_id: Id,
        input: NewView,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<DatabaseView, DatabaseError> {
        let mut tx = self.database.immediate_transaction().await?;
        let set = writable_views(&mut tx, workspace_id, database_id, actor_id, input.link).await?;
        let properties = properties_in_tx(&mut tx, database_id).await?;
        let views = raw_views(&mut tx, database_id, set).await?;
        match set {
            Views::Link(_) if views.len() >= MAX_LINK_VIEWS => {
                return Err(DatabaseError::LinkViewLimit);
            }
            _ if views.len() >= MAX_VIEWS => return Err(DatabaseError::ViewLimit),
            _ => {}
        }
        let source = input
            .duplicate_of
            .map(|source| {
                views
                    .iter()
                    .find(|view| view.id == source)
                    .ok_or_else(|| DatabaseError::invalid("duplicate_of"))
            })
            .transpose()?;
        let view_type = source.map_or(input.view_type, |view| view.view_type);
        let mut config = match (input.config, source) {
            (Some(config), _) => config,
            (None, Some(source)) => source.config.clone(),
            (None, None) => ViewConfig::for_properties(&properties),
        };
        config
            .validate(view_type, &properties)
            .map_err(DatabaseError::invalid)?;
        board::drop_former_members(&mut tx, database_id, &properties, &mut [&mut config]).await?;
        templates::retain_default(&mut tx, database_id, &mut config).await?;
        let view = DatabaseView {
            id: Id::new_v7(),
            name: input.name,
            view_type,
            config,
            position: views.len() as i64,
        };
        insert_view(&mut tx, workspace_id, database_id, set.link(), &view, now).await?;
        record(
            &mut tx,
            workspace_id,
            actor_id,
            "database.view_created",
            database_id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(view)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_view(
        &self,
        workspace_id: Id,
        database_id: Id,
        view_id: Id,
        actor_id: Id,
        changes: ViewChanges,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<DatabaseView, DatabaseError> {
        let mut tx = self.database.immediate_transaction().await?;
        let set =
            writable_views(&mut tx, workspace_id, database_id, actor_id, changes.link).await?;
        let properties = properties_in_tx(&mut tx, database_id).await?;
        let mut views = raw_views(&mut tx, database_id, set).await?;
        let index = views
            .iter()
            .position(|view| view.id == view_id)
            .ok_or(DatabaseError::ViewNotFound)?;
        let mut view = views.remove(index);
        if let Some(mut config) = changes.config {
            config
                .validate(view.view_type, &properties)
                .map_err(DatabaseError::invalid)?;
            board::drop_former_members(&mut tx, database_id, &properties, &mut [&mut config])
                .await?;
            templates::retain_default(&mut tx, database_id, &mut config).await?;
            view.config = config;
        }
        if let Some(name) = changes.name {
            view.name = name;
        }
        sqlx::query(
            "UPDATE database_views SET name = $1, config_json = $2, updated_at = $3 WHERE id = $4",
        )
        .bind(&view.name)
        .bind(serde_json::to_string(&view.config).map_err(|_| DatabaseError::Corrupt)?)
        .bind(now.as_millis())
        .bind(view_id.to_string())
        .execute(&mut *tx)
        .await?;
        let index = insert_index(changes.position.or(Some(index as i64)), views.len());
        view.position = index as i64;
        let mut ids: Vec<Id> = views.iter().map(|view| view.id).collect();
        ids.insert(index, view.id);
        renumber_rows(&mut tx, "database_views", &ids).await?;
        record(
            &mut tx,
            workspace_id,
            actor_id,
            "database.view_updated",
            database_id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        view.config.repair(view.view_type, &properties);
        Ok(view)
    }

    /// Deletes an own view (`link` `None`) or a view of a linked view block.
    #[allow(clippy::too_many_arguments)]
    pub async fn delete_view(
        &self,
        workspace_id: Id,
        database_id: Id,
        view_id: Id,
        link: Option<Id>,
        actor_id: Id,
        request_id: &str,
        now: TimestampMillis,
    ) -> Result<(), DatabaseError> {
        let mut tx = self.database.immediate_transaction().await?;
        let set = writable_views(&mut tx, workspace_id, database_id, actor_id, link).await?;
        let mut views = raw_views(&mut tx, database_id, set).await?;
        let index = views
            .iter()
            .position(|view| view.id == view_id)
            .ok_or(DatabaseError::ViewNotFound)?;
        if views.len() == 1 {
            return Err(DatabaseError::LastView);
        }
        sqlx::query("DELETE FROM database_views WHERE id = $1")
            .bind(view_id.to_string())
            .execute(&mut *tx)
            .await?;
        views.remove(index);
        let ids: Vec<Id> = views.iter().map(|view| view.id).collect();
        renumber_rows(&mut tx, "database_views", &ids).await?;
        record(
            &mut tx,
            workspace_id,
            actor_id,
            "database.view_deleted",
            database_id,
            request_id,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }
}

impl DatabaseView {
    /// A view from its `database_views` columns, config as saved.
    pub(in crate::repositories) fn from_stored(
        id: &str,
        name: String,
        view_type: &str,
        config_json: &str,
        position: i64,
    ) -> Result<Self, DatabaseError> {
        Ok(Self {
            id: parse_id(id)?,
            name,
            view_type: ViewType::parse(view_type).ok_or(DatabaseError::Corrupt)?,
            config: serde_json::from_str(config_json).map_err(|_| DatabaseError::Corrupt)?,
            position,
        })
    }
}

/// The stored views of `set`, configs as saved.
pub(super) async fn raw_views(
    tx: &mut Transaction<'_, Any>,
    database_id: Id,
    set: Views,
) -> Result<Vec<DatabaseView>, DatabaseError> {
    let query = match set {
        Views::Own => sqlx::query(
            "SELECT id, name, type, config_json, position FROM database_views \
             WHERE database_id = $1 AND link_id IS NULL ORDER BY position, id",
        ),
        Views::Link(_) => sqlx::query(
            "SELECT id, name, type, config_json, position FROM database_views \
             WHERE database_id = $1 AND link_id = $2 ORDER BY position, id",
        ),
        Views::All => sqlx::query(
            "SELECT id, name, type, config_json, position FROM database_views \
             WHERE database_id = $1 ORDER BY position, id",
        ),
    };
    let query = query.bind(database_id.to_string());
    let query = match set {
        Views::Link(link_id) => query.bind(link_id.to_string()),
        Views::Own | Views::All => query,
    };
    let rows = query.fetch_all(&mut **tx).await?;
    rows.into_iter()
        .map(|row| {
            DatabaseView::from_stored(
                &row.get::<String, _>("id"),
                row.get("name"),
                &row.get::<String, _>("type"),
                &row.get::<String, _>("config_json"),
                row.get("position"),
            )
        })
        .collect()
}

/// The views as clients see them: repaired against the current properties.
pub(super) async fn views_in_tx(
    tx: &mut Transaction<'_, Any>,
    database_id: Id,
    set: Views,
    properties: &[Property],
) -> Result<Vec<DatabaseView>, DatabaseError> {
    let mut views = raw_views(tx, database_id, set).await?;
    for view in &mut views {
        view.config.repair(view.view_type, properties);
    }
    let mut configs: Vec<_> = views.iter_mut().map(|view| &mut view.config).collect();
    board::drop_former_members(tx, database_id, properties, &mut configs).await?;
    Ok(views)
}

pub(super) async fn write_view_config(
    tx: &mut Transaction<'_, Any>,
    view_id: Id,
    config: &ViewConfig,
    now: TimestampMillis,
) -> Result<(), DatabaseError> {
    sqlx::query("UPDATE database_views SET config_json = $1, updated_at = $2 WHERE id = $3")
        .bind(serde_json::to_string(config).map_err(|_| DatabaseError::Corrupt)?)
        .bind(now.as_millis())
        .bind(view_id.to_string())
        .execute(&mut **tx)
        .await?;
    Ok(())
}

pub(super) async fn insert_view(
    tx: &mut Transaction<'_, Any>,
    workspace_id: Id,
    database_id: Id,
    link: Option<Id>,
    view: &DatabaseView,
    now: TimestampMillis,
) -> Result<(), DatabaseError> {
    sqlx::query(
        "INSERT INTO database_views (id, workspace_id, database_id, link_id, name, type, config_json, position, created_at, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
    )
    .bind(view.id.to_string())
    .bind(workspace_id.to_string())
    .bind(database_id.to_string())
    .bind(link.map(|link_id| link_id.to_string()))
    .bind(&view.name)
    .bind(view.view_type.as_str())
    .bind(serde_json::to_string(&view.config).map_err(|_| DatabaseError::Corrupt)?)
    .bind(view.position)
    .bind(now.as_millis())
    .bind(now.as_millis())
    .execute(&mut **tx)
    .await?;
    Ok(())
}
