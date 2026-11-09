//! Docs database routes (`/api/v1/workspaces/{workspace_id}/databases/{database_id}/…`). The
//! database id is the database page's id; access follows that page (see
//! [`crate::repositories::databases`]).

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use orbit_platform::{Id, RequestId, TimestampMillis};
use serde::{Deserialize, Deserializer};
use serde_json::Value;
use utoipa::{IntoParams, ToSchema};

use crate::auth_routes::CookieMode;
use crate::repositories::databases::{
    DatabaseDetail, DatabaseError, DatabaseProperty, DatabaseRepository, DatabaseRow, DatabaseView,
    LinkedViews, MAX_CELL_ITEMS, MAX_NAME_CHARS, MAX_TRASH_ROWS, NewProperty, NewRow, NewView,
    PropertyChanges, PropertyType, PropertyUpdate, RowPlacement, ViewChanges, ViewConfig, ViewType,
};
use crate::repositories::identity::IdentityRepository;
use crate::task_routes::{
    ApiError, ApiJson, ApiQuery, authenticate_session, bounded, deserialize_source_patch,
    request_id_value, validation,
};

#[derive(Clone)]
pub struct DatabaseState {
    identity: Arc<IdentityRepository>,
    databases: Arc<DatabaseRepository>,
    cookie_mode: CookieMode,
}

impl DatabaseState {
    #[must_use]
    pub fn new(identity: Arc<IdentityRepository>, cookie_mode: CookieMode) -> Self {
        Self {
            databases: Arc::new(DatabaseRepository::new(identity.database().clone())),
            identity,
            cookie_mode,
        }
    }
}

const BASE: &str = "/api/v1/workspaces/{workspace_id}/databases/{database_id}";

pub fn database_router(state: DatabaseState) -> Router {
    Router::new()
        .route(BASE, get(get_database))
        .route(
            &format!("{BASE}/properties"),
            post(create_database_property),
        )
        .route(
            &format!("{BASE}/properties/{{property_id}}"),
            patch(update_database_property).delete(delete_database_property),
        )
        .route(&format!("{BASE}/links"), post(create_database_link))
        .route(
            &format!("{BASE}/links/{{link_id}}"),
            axum::routing::delete(delete_database_link),
        )
        .route(&format!("{BASE}/views"), post(create_database_view))
        .route(
            &format!("{BASE}/views/{{view_id}}"),
            patch(update_database_view).delete(delete_database_view),
        )
        .route(&format!("{BASE}/rows"), post(create_database_row))
        .route(&format!("{BASE}/rows/trash"), post(trash_database_rows))
        .route(
            &format!("{BASE}/rows/{{row_id}}/values"),
            patch(update_database_row_values),
        )
        .route(
            &format!("{BASE}/rows/{{row_id}}/move"),
            post(move_database_row),
        )
        .with_state(state)
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct CreateDatabasePropertyBody {
    /// 1 to 200 characters. Required unless `duplicate_of` is set (the copy is then named
    /// "<name> (copy)").
    name: Option<String>,
    /// Any type but `title` (a database has exactly one). Required unless `duplicate_of` is set.
    #[serde(rename = "type")]
    property_type: Option<PropertyType>,
    /// The type's config (see `DatabaseProperty.config`); omitted gives the default (a new
    /// `status` gets "Not started", "In progress" and "Done"). Options without an `id` get one.
    #[schema(value_type = Option<Object>)]
    config: Option<Value>,
    /// Index among the properties; omitted appends it. The property is added to every view.
    position: Option<i64>,
    /// A property of this database (not the title) to copy with its config and values; the copy
    /// goes right after it, among the properties and in every view. Cannot be sent with `type`,
    /// `config` or `position` (422).
    duplicate_of: Option<String>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct UpdateDatabasePropertyBody {
    name: Option<String>,
    /// A new type converts the values (spec: text in between; person and computed types clear
    /// them) and derives the config: send `config` in a later request.
    #[serde(rename = "type")]
    property_type: Option<PropertyType>,
    /// The whole config of the current type. Options left out are removed from every value.
    #[schema(value_type = Option<Object>)]
    config: Option<Value>,
    position: Option<i64>,
    /// Returns what the change would do (`converted`, `cleared`) without saving it.
    #[serde(default)]
    dry_run: bool,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct CreateDatabaseViewBody {
    /// 1 to 200 characters.
    name: String,
    #[serde(rename = "type")]
    view_type: ViewType,
    /// Omitted: the config of `duplicate_of`, or every property as a visible column.
    #[schema(value_type = Option<ViewConfig>)]
    config: Option<Value>,
    /// A view of this database whose config the new view copies.
    duplicate_of: Option<String>,
    /// Adds the view to this linked view block instead of the database's own views.
    link_id: Option<String>,
}

/// The views a request reads or changes: a linked view block's instead of the database's own.
#[derive(Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
struct LinkQuery {
    /// A `database_links` id of this database.
    #[serde(rename = "link")]
    #[param(rename = "link")]
    link_id: Option<String>,
}

/// The view a request changes belongs to this linked view block.
#[derive(Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
struct ViewLinkQuery {
    /// A `database_links` id of this database.
    link_id: Option<String>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct CreateDatabaseLinkBody {
    /// The page whose block shows the link; the caller edits it.
    host_page_id: String,
    /// An own view whose type and config the link's first view copies (default: the first one).
    from_view_id: Option<String>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct UpdateDatabaseViewBody {
    name: Option<String>,
    /// The whole config; shared by every member at once.
    #[schema(value_type = Option<ViewConfig>)]
    config: Option<Value>,
    /// Index among the views.
    position: Option<i64>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct CreateDatabaseRowBody {
    #[serde(default)]
    title: String,
    icon: Option<String>,
    /// Values by property id, as in `PATCH …/rows/{row_id}/values`.
    #[serde(default)]
    #[schema(value_type = Object)]
    values: BTreeMap<String, Value>,
    /// Omitted: the row goes last. `null`: first. Otherwise right after that row.
    #[serde(default, deserialize_with = "deserialize_source_patch")]
    #[schema(value_type = Option<String>)]
    after_row_id: Option<Option<String>>,
}

/// Values by property id; `null` clears a value. At most 100 properties per request.
#[derive(Deserialize, ToSchema)]
#[serde(transparent)]
#[schema(value_type = Object)]
struct DatabaseRowValuesBody(BTreeMap<String, Value>);

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct TrashDatabaseRowsBody {
    /// 1 to 500 row ids. Ids that are not live rows of this database are skipped.
    row_ids: Vec<String>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct MoveDatabaseRowBody {
    /// `null` moves the row first; otherwise right after that row.
    #[serde(deserialize_with = "required_nullable")]
    #[schema(required = true)]
    after_row_id: Option<String>,
    /// Values written with the move, as in `PATCH …/rows/{row_id}/values` (a board card dropped
    /// into another column sets its group value).
    #[serde(default)]
    #[schema(value_type = Object)]
    values: BTreeMap<String, Value>,
}

/// The database's properties, views and live rows (at most 5,000), for clients that sort and
/// filter themselves.
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/databases/{database_id}", params(("workspace_id" = String, Path), ("database_id" = String, Path), LinkQuery), responses((status = 200, body = DatabaseDetail)))]
async fn get_database(
    State(state): State<DatabaseState>,
    Path((workspace, database)): Path<(String, String)>,
    ApiQuery(query): ApiQuery<LinkQuery>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<DatabaseDetail>, ApiError> {
    let request = request_id.as_ref();
    let instance = format!("/api/v1/workspaces/{workspace}/databases/{database}");
    let (workspace_id, database_id, actor_id) =
        scope(&state, &headers, &workspace, &database, &instance, request).await?;
    let link = link_id(query.link_id, &instance, request)?;
    state
        .databases
        .get(workspace_id, database_id, actor_id, link)
        .await
        .map(Json)
        .map_err(|error| database_problem(error, instance, request))
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/databases/{database_id}/properties", params(("workspace_id" = String, Path), ("database_id" = String, Path)), request_body = CreateDatabasePropertyBody, responses((status = 201, body = DatabaseProperty)))]
async fn create_database_property(
    State(state): State<DatabaseState>,
    Path((workspace, database)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<CreateDatabasePropertyBody>,
) -> Result<Response, ApiError> {
    let request = request_id.as_ref();
    let instance = format!("/api/v1/workspaces/{workspace}/databases/{database}/properties");
    let (workspace_id, database_id, actor_id) =
        scope(&state, &headers, &workspace, &database, &instance, request).await?;
    let input = match body.duplicate_of {
        Some(source) => {
            if body.property_type.is_some() || body.config.is_some() || body.position.is_some() {
                return Err(validation("duplicate_of", &instance, request));
            }
            NewProperty::Duplicate {
                source: source
                    .parse()
                    .map_err(|_| validation("duplicate_of", &instance, request))?,
                name: body
                    .name
                    .map(|value| name(value, &instance, request))
                    .transpose()?,
            }
        }
        None => NewProperty::Fresh {
            name: name(
                body.name
                    .ok_or_else(|| validation("name", &instance, request))?,
                &instance,
                request,
            )?,
            property_type: body
                .property_type
                .ok_or_else(|| validation("type", &instance, request))?,
            config: body.config,
            position: body
                .position
                .map(|value| position(value, &instance, request))
                .transpose()?,
        },
    };
    state
        .databases
        .create_property(
            workspace_id,
            database_id,
            actor_id,
            input,
            request_id_value(request),
            TimestampMillis::now(),
        )
        .await
        .map(|property| (StatusCode::CREATED, Json(property)).into_response())
        .map_err(|error| database_problem(error, instance, request))
}

#[utoipa::path(patch, path = "/api/v1/workspaces/{workspace_id}/databases/{database_id}/properties/{property_id}", params(("workspace_id" = String, Path), ("database_id" = String, Path), ("property_id" = String, Path)), request_body = UpdateDatabasePropertyBody, responses((status = 200, body = PropertyUpdate)))]
async fn update_database_property(
    State(state): State<DatabaseState>,
    Path((workspace, database, property)): Path<(String, String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<UpdateDatabasePropertyBody>,
) -> Result<Json<PropertyUpdate>, ApiError> {
    let request = request_id.as_ref();
    let instance =
        format!("/api/v1/workspaces/{workspace}/databases/{database}/properties/{property}");
    let (workspace_id, database_id, actor_id) =
        scope(&state, &headers, &workspace, &database, &instance, request).await?;
    let property_id = child_id(
        &property,
        DatabaseError::PropertyNotFound,
        &instance,
        request,
    )?;
    let changes = PropertyChanges {
        name: body
            .name
            .map(|value| name(value, &instance, request))
            .transpose()?,
        property_type: body.property_type,
        config: body.config,
        position: body
            .position
            .map(|value| position(value, &instance, request))
            .transpose()?,
        dry_run: body.dry_run,
    };
    state
        .databases
        .update_property(
            workspace_id,
            database_id,
            property_id,
            actor_id,
            changes,
            request_id_value(request),
            TimestampMillis::now(),
        )
        .await
        .map(Json)
        .map_err(|error| database_problem(error, instance, request))
}

/// Deletes a property with its values; views drop it. The title property cannot be deleted.
#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/databases/{database_id}/properties/{property_id}", params(("workspace_id" = String, Path), ("database_id" = String, Path), ("property_id" = String, Path)), responses((status = 204)))]
async fn delete_database_property(
    State(state): State<DatabaseState>,
    Path((workspace, database, property)): Path<(String, String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<StatusCode, ApiError> {
    let request = request_id.as_ref();
    let instance =
        format!("/api/v1/workspaces/{workspace}/databases/{database}/properties/{property}");
    let (workspace_id, database_id, actor_id) =
        scope(&state, &headers, &workspace, &database, &instance, request).await?;
    let property_id = child_id(
        &property,
        DatabaseError::PropertyNotFound,
        &instance,
        request,
    )?;
    state
        .databases
        .delete_property(
            workspace_id,
            database_id,
            property_id,
            actor_id,
            request_id_value(request),
            TimestampMillis::now(),
        )
        .await
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(|error| database_problem(error, instance, request))
}

/// Links the database to a block of a page (a linked view block), with one view of its own.
/// Needs read access to the database and edit access to the host page.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/databases/{database_id}/links", params(("workspace_id" = String, Path), ("database_id" = String, Path)), request_body = CreateDatabaseLinkBody, responses((status = 201, body = LinkedViews)))]
async fn create_database_link(
    State(state): State<DatabaseState>,
    Path((workspace, database)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<CreateDatabaseLinkBody>,
) -> Result<Response, ApiError> {
    let request = request_id.as_ref();
    let instance = format!("/api/v1/workspaces/{workspace}/databases/{database}/links");
    let (workspace_id, database_id, actor_id) =
        scope(&state, &headers, &workspace, &database, &instance, request).await?;
    let host_page_id = body
        .host_page_id
        .parse()
        .map_err(|_| validation("host_page_id", &instance, request))?;
    let from_view = body
        .from_view_id
        .map(|value| {
            value
                .parse()
                .map_err(|_| validation("from_view_id", &instance, request))
        })
        .transpose()?;
    state
        .databases
        .create_link(
            workspace_id,
            database_id,
            actor_id,
            host_page_id,
            from_view,
            request_id_value(request),
            TimestampMillis::now(),
        )
        .await
        .map(|linked| (StatusCode::CREATED, Json(linked)).into_response())
        .map_err(|error| database_problem(error, instance, request))
}

/// Deletes a link and its views. The editor never calls it (undo and page history can bring a
/// deleted block back); it is for cleanup tools.
#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/databases/{database_id}/links/{link_id}", params(("workspace_id" = String, Path), ("database_id" = String, Path), ("link_id" = String, Path)), responses((status = 204)))]
async fn delete_database_link(
    State(state): State<DatabaseState>,
    Path((workspace, database, link)): Path<(String, String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<StatusCode, ApiError> {
    let request = request_id.as_ref();
    let instance = format!("/api/v1/workspaces/{workspace}/databases/{database}/links/{link}");
    let (workspace_id, database_id, actor_id) =
        scope(&state, &headers, &workspace, &database, &instance, request).await?;
    let link_id = child_id(&link, DatabaseError::LinkNotFound, &instance, request)?;
    state
        .databases
        .delete_link(
            workspace_id,
            database_id,
            link_id,
            actor_id,
            request_id_value(request),
            TimestampMillis::now(),
        )
        .await
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(|error| database_problem(error, instance, request))
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/databases/{database_id}/views", params(("workspace_id" = String, Path), ("database_id" = String, Path)), request_body = CreateDatabaseViewBody, responses((status = 201, body = DatabaseView)))]
async fn create_database_view(
    State(state): State<DatabaseState>,
    Path((workspace, database)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<CreateDatabaseViewBody>,
) -> Result<Response, ApiError> {
    let request = request_id.as_ref();
    let instance = format!("/api/v1/workspaces/{workspace}/databases/{database}/views");
    let (workspace_id, database_id, actor_id) =
        scope(&state, &headers, &workspace, &database, &instance, request).await?;
    let input = NewView {
        name: name(body.name, &instance, request)?,
        view_type: body.view_type,
        config: body
            .config
            .map(|value| view_config(value, &instance, request))
            .transpose()?,
        duplicate_of: body
            .duplicate_of
            .map(|value| {
                value
                    .parse()
                    .map_err(|_| validation("duplicate_of", &instance, request))
            })
            .transpose()?,
        link: link_id(body.link_id, &instance, request)?,
    };
    state
        .databases
        .create_view(
            workspace_id,
            database_id,
            actor_id,
            input,
            request_id_value(request),
            TimestampMillis::now(),
        )
        .await
        .map(|view| (StatusCode::CREATED, Json(view)).into_response())
        .map_err(|error| database_problem(error, instance, request))
}

#[utoipa::path(patch, path = "/api/v1/workspaces/{workspace_id}/databases/{database_id}/views/{view_id}", params(("workspace_id" = String, Path), ("database_id" = String, Path), ("view_id" = String, Path), ViewLinkQuery), request_body = UpdateDatabaseViewBody, responses((status = 200, body = DatabaseView)))]
async fn update_database_view(
    State(state): State<DatabaseState>,
    Path((workspace, database, view)): Path<(String, String, String)>,
    ApiQuery(query): ApiQuery<ViewLinkQuery>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<UpdateDatabaseViewBody>,
) -> Result<Json<DatabaseView>, ApiError> {
    let request = request_id.as_ref();
    let instance = format!("/api/v1/workspaces/{workspace}/databases/{database}/views/{view}");
    let (workspace_id, database_id, actor_id) =
        scope(&state, &headers, &workspace, &database, &instance, request).await?;
    let view_id = child_id(&view, DatabaseError::ViewNotFound, &instance, request)?;
    let changes = ViewChanges {
        name: body
            .name
            .map(|value| name(value, &instance, request))
            .transpose()?,
        config: body
            .config
            .map(|value| view_config(value, &instance, request))
            .transpose()?,
        position: body
            .position
            .map(|value| position(value, &instance, request))
            .transpose()?,
        link: link_id(query.link_id, &instance, request)?,
    };
    state
        .databases
        .update_view(
            workspace_id,
            database_id,
            view_id,
            actor_id,
            changes,
            request_id_value(request),
            TimestampMillis::now(),
        )
        .await
        .map(Json)
        .map_err(|error| database_problem(error, instance, request))
}

/// Deletes a view; the last view of a database or of a linked view block cannot be deleted
/// (409 `database_last_view`).
#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/databases/{database_id}/views/{view_id}", params(("workspace_id" = String, Path), ("database_id" = String, Path), ("view_id" = String, Path), ViewLinkQuery), responses((status = 204)))]
async fn delete_database_view(
    State(state): State<DatabaseState>,
    Path((workspace, database, view)): Path<(String, String, String)>,
    ApiQuery(query): ApiQuery<ViewLinkQuery>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<StatusCode, ApiError> {
    let request = request_id.as_ref();
    let instance = format!("/api/v1/workspaces/{workspace}/databases/{database}/views/{view}");
    let (workspace_id, database_id, actor_id) =
        scope(&state, &headers, &workspace, &database, &instance, request).await?;
    let view_id = child_id(&view, DatabaseError::ViewNotFound, &instance, request)?;
    let link = link_id(query.link_id, &instance, request)?;
    state
        .databases
        .delete_view(
            workspace_id,
            database_id,
            view_id,
            link,
            actor_id,
            request_id_value(request),
            TimestampMillis::now(),
        )
        .await
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(|error| database_problem(error, instance, request))
}

/// Adds a row (a sub-page of the database). At most 5,000 live rows (409 `database_row_limit`).
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/databases/{database_id}/rows", params(("workspace_id" = String, Path), ("database_id" = String, Path)), request_body = CreateDatabaseRowBody, responses((status = 201, body = DatabaseRow)))]
async fn create_database_row(
    State(state): State<DatabaseState>,
    Path((workspace, database)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<CreateDatabaseRowBody>,
) -> Result<Response, ApiError> {
    let request = request_id.as_ref();
    let instance = format!("/api/v1/workspaces/{workspace}/databases/{database}/rows");
    let (workspace_id, database_id, actor_id) =
        scope(&state, &headers, &workspace, &database, &instance, request).await?;
    let placement = match body.after_row_id {
        None => RowPlacement::Last,
        Some(None) => RowPlacement::First,
        Some(Some(id)) => RowPlacement::After(
            id.parse()
                .map_err(|_| validation("after_row_id", &instance, request))?,
        ),
    };
    let input = NewRow {
        title: bounded(body.title, 500, 2000, "title", &instance, request)?,
        icon: body
            .icon
            .map(|value| bounded(value, 64, 256, "icon", &instance, request))
            .transpose()?,
        values: values(body.values, &instance, request)?,
        placement,
    };
    state
        .databases
        .create_row(
            workspace_id,
            database_id,
            actor_id,
            input,
            request_id_value(request),
            TimestampMillis::now(),
        )
        .await
        .map(|row| (StatusCode::CREATED, Json(row)).into_response())
        .map_err(|error| database_problem(error, instance, request))
}

/// Sets or clears (`null`) values of a row; last write wins. Each value is checked against its
/// property (422 with `values.<property_id>`). The title and computed properties are not values:
/// change the title with `PATCH /pages/{row_id}`.
#[utoipa::path(patch, path = "/api/v1/workspaces/{workspace_id}/databases/{database_id}/rows/{row_id}/values", params(("workspace_id" = String, Path), ("database_id" = String, Path), ("row_id" = String, Path)), request_body = DatabaseRowValuesBody, responses((status = 200, body = DatabaseRow)))]
async fn update_database_row_values(
    State(state): State<DatabaseState>,
    Path((workspace, database, row)): Path<(String, String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(DatabaseRowValuesBody(body)): ApiJson<DatabaseRowValuesBody>,
) -> Result<Json<DatabaseRow>, ApiError> {
    let request = request_id.as_ref();
    let instance = format!("/api/v1/workspaces/{workspace}/databases/{database}/rows/{row}/values");
    let (workspace_id, database_id, actor_id) =
        scope(&state, &headers, &workspace, &database, &instance, request).await?;
    let row_id = child_id(&row, DatabaseError::NotFound, &instance, request)?;
    let input = values(body, &instance, request)?;
    state
        .databases
        .update_values(
            workspace_id,
            database_id,
            row_id,
            actor_id,
            input,
            request_id_value(request),
            TimestampMillis::now(),
        )
        .await
        .map(Json)
        .map_err(|error| database_problem(error, instance, request))
}

/// Moves rows to the trash, each as its own trash entry; restoring one brings it back into the
/// database.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/databases/{database_id}/rows/trash", params(("workspace_id" = String, Path), ("database_id" = String, Path)), request_body = TrashDatabaseRowsBody, responses((status = 204)))]
async fn trash_database_rows(
    State(state): State<DatabaseState>,
    Path((workspace, database)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<TrashDatabaseRowsBody>,
) -> Result<StatusCode, ApiError> {
    let request = request_id.as_ref();
    let instance = format!("/api/v1/workspaces/{workspace}/databases/{database}/rows/trash");
    let (workspace_id, database_id, actor_id) =
        scope(&state, &headers, &workspace, &database, &instance, request).await?;
    if body.row_ids.is_empty() || body.row_ids.len() > MAX_TRASH_ROWS {
        return Err(validation("row_ids", &instance, request));
    }
    let row_ids = body
        .row_ids
        .iter()
        .map(|id| id.parse::<Id>())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| validation("row_ids", &instance, request))?;
    state
        .databases
        .trash_rows(
            workspace_id,
            database_id,
            actor_id,
            &row_ids,
            request_id_value(request),
            TimestampMillis::now(),
        )
        .await
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(|error| database_problem(error, instance, request))
}

/// Manual row order: puts the row first or right after another row, and writes `values` with it.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/databases/{database_id}/rows/{row_id}/move", params(("workspace_id" = String, Path), ("database_id" = String, Path), ("row_id" = String, Path)), request_body = MoveDatabaseRowBody, responses((status = 200, body = DatabaseRow)))]
async fn move_database_row(
    State(state): State<DatabaseState>,
    Path((workspace, database, row)): Path<(String, String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<MoveDatabaseRowBody>,
) -> Result<Json<DatabaseRow>, ApiError> {
    let request = request_id.as_ref();
    let instance = format!("/api/v1/workspaces/{workspace}/databases/{database}/rows/{row}/move");
    let (workspace_id, database_id, actor_id) =
        scope(&state, &headers, &workspace, &database, &instance, request).await?;
    let row_id = child_id(&row, DatabaseError::NotFound, &instance, request)?;
    let after = body
        .after_row_id
        .map(|id| {
            id.parse()
                .map_err(|_| validation("after_row_id", &instance, request))
        })
        .transpose()?;
    let input = values(body.values, &instance, request)?;
    state
        .databases
        .move_row(
            workspace_id,
            database_id,
            row_id,
            actor_id,
            after,
            input,
            request_id_value(request),
            TimestampMillis::now(),
        )
        .await
        .map(Json)
        .map_err(|error| database_problem(error, instance, request))
}

/// The session's user and the parsed workspace and database ids (an unparseable id names no
/// database: 404).
async fn scope(
    state: &DatabaseState,
    headers: &HeaderMap,
    workspace: &str,
    database: &str,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<(Id, Id, Id), ApiError> {
    let session = authenticate_session(
        &state.identity,
        state.cookie_mode,
        headers,
        instance,
        request_id,
    )
    .await?;
    let not_found = || database_problem(DatabaseError::NotFound, instance.to_owned(), request_id);
    let workspace_id = workspace.parse().map_err(|_| not_found())?;
    let database_id = database.parse().map_err(|_| not_found())?;
    Ok((workspace_id, database_id, session.user.id))
}

fn child_id(
    value: &str,
    missing: DatabaseError,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<Id, ApiError> {
    value
        .parse()
        .map_err(|_| database_problem(missing, instance.to_owned(), request_id))
}

/// A property or view name: 1 to 200 characters after trimming.
fn name(
    value: String,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<String, ApiError> {
    let value = value.trim().to_owned();
    if value.is_empty() {
        return Err(validation("name", instance, request_id));
    }
    bounded(
        value,
        MAX_NAME_CHARS,
        MAX_NAME_CHARS * 4,
        "name",
        instance,
        request_id,
    )
}

/// A link id from a query or body: an unparseable id names no link of the database (404).
fn link_id(
    value: Option<String>,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<Option<Id>, ApiError> {
    value
        .map(|value| child_id(&value, DatabaseError::LinkNotFound, instance, request_id))
        .transpose()
}

fn position(
    value: i64,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<i64, ApiError> {
    if value < 0 {
        Err(validation("position", instance, request_id))
    } else {
        Ok(value)
    }
}

fn view_config(
    value: Value,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<ViewConfig, ApiError> {
    serde_json::from_value(value).map_err(|_| validation("config", instance, request_id))
}

fn values(
    input: BTreeMap<String, Value>,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<Vec<(Id, Value)>, ApiError> {
    if input.len() > MAX_CELL_ITEMS {
        return Err(validation("values", instance, request_id));
    }
    input
        .into_iter()
        .map(|(key, value)| match key.parse::<Id>() {
            Ok(id) => Ok((id, value)),
            Err(_) => {
                Err(validation("values", instance, request_id).with_detail(format!("values.{key}")))
            }
        })
        .collect()
}

fn required_nullable<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer)
}

fn database_problem(
    error: DatabaseError,
    instance: String,
    request_id: Option<&Extension<RequestId>>,
) -> ApiError {
    let (status, code, title, detail) = match error {
        DatabaseError::Invalid { field } => {
            return validation("field", &instance, request_id).with_detail(field);
        }
        DatabaseError::TrialLimit(reached) => {
            return ApiError::trial_limit(reached, instance, request_id);
        }
        DatabaseError::NotFound => (
            StatusCode::NOT_FOUND,
            "page_not_found",
            "Page not found",
            "The requested page was not found.",
        ),
        DatabaseError::PropertyNotFound => (
            StatusCode::NOT_FOUND,
            "database_property_not_found",
            "Database property not found",
            "The requested property was not found in this database.",
        ),
        DatabaseError::ViewNotFound => (
            StatusCode::NOT_FOUND,
            "database_view_not_found",
            "Database view not found",
            "The requested view was not found in this database.",
        ),
        DatabaseError::LinkNotFound => (
            StatusCode::NOT_FOUND,
            "database_link_not_found",
            "Database link not found",
            "The requested linked view was not found in this database.",
        ),
        DatabaseError::Locked => (
            StatusCode::LOCKED,
            "page_locked",
            "Page locked",
            "The database is locked. Unlock it to change its properties and views.",
        ),
        DatabaseError::RowLimit => (
            StatusCode::CONFLICT,
            "database_row_limit",
            "Database row limit",
            "A database holds at most 5,000 rows.",
        ),
        DatabaseError::PropertyLimit => (
            StatusCode::CONFLICT,
            "database_property_limit",
            "Database property limit",
            "A database holds at most 100 properties.",
        ),
        DatabaseError::ViewLimit => (
            StatusCode::CONFLICT,
            "database_view_limit",
            "Database view limit",
            "A database holds at most 50 views.",
        ),
        DatabaseError::LinkViewLimit => (
            StatusCode::CONFLICT,
            "database_link_view_limit",
            "Linked view limit",
            "A linked view holds at most 20 views.",
        ),
        DatabaseError::LastView => (
            StatusCode::CONFLICT,
            "database_last_view",
            "Last database view",
            "A database keeps at least one view.",
        ),
        DatabaseError::TitleProperty => (
            StatusCode::CONFLICT,
            "database_title_property",
            "Title property",
            "The title property cannot be deleted or change its type.",
        ),
        DatabaseError::Corrupt | DatabaseError::Unavailable(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "Internal server error",
            "An unexpected error occurred. Use the request ID when contacting support.",
        ),
    };
    ApiError::new(status, code, title, detail, instance, request_id)
}
