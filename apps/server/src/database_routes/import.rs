//! CSV import routes (spec P2 9.3): `POST …/databases/import` creates a database from a CSV,
//! `POST …/databases/{database_id}/import` adds a CSV's rows to a database. The client parses the
//! file and sends its cells as text with a column mapping; the server converts and writes them in
//! one transaction.

use axum::extract::{DefaultBodyLimit, Extension, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use orbit_platform::{RequestId, TimestampMillis};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::{BASE, DatabaseState, database_problem, scope};
use crate::page_routes::{page_problem, space_request};
use crate::repositories::databases::{
    CsvImport, ImportColumn, ImportResult, ImportTarget, MAX_IMPORT_COLUMNS, MAX_NAME_CHARS,
    PropertyType,
};
use crate::repositories::pages::{CreatePage, PageKind, PageRecord, PageRepository};
use crate::task_routes::{
    ApiError, ApiJson, authenticate_session, bounded, request_id_value, validation,
};

/// A 5 MB CSV as JSON text with its mapping.
const IMPORT_BODY_LIMIT: usize = 8 * 1024 * 1024;

pub(super) fn routes() -> Router<DatabaseState> {
    Router::new()
        .route(
            "/api/v1/workspaces/{workspace_id}/databases/import",
            post(create_database_from_csv).layer(DefaultBodyLimit::max(IMPORT_BODY_LIMIT)),
        )
        .route(
            &format!("{BASE}/import"),
            post(import_database_csv).layer(DefaultBodyLimit::max(IMPORT_BODY_LIMIT)),
        )
}

/// Where a CSV column goes: an existing property, a new property of a type, or nowhere.
#[derive(Deserialize, ToSchema)]
#[serde(untagged)]
#[schema(as = DatabaseImportTarget)]
enum ImportTargetBody {
    Property(PropertyTargetBody),
    New(NewTargetBody),
    Skip(SkipTarget),
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
#[schema(as = DatabaseImportPropertyTarget)]
struct PropertyTargetBody {
    property_id: String,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
#[schema(as = DatabaseImportNewTarget)]
struct NewTargetBody {
    /// The new property's type; it is named after the column.
    new: PropertyType,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
#[schema(as = DatabaseImportSkip)]
enum SkipTarget {
    Skip,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
#[schema(as = DatabaseImportColumn)]
struct ImportColumnBody {
    /// The CSV header; the name of a new property.
    name: String,
    target: ImportTargetBody,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct ImportDatabaseCsvBody {
    /// One per CSV column, in order (at most 100). Exactly one maps to the title property.
    columns: Vec<ImportColumnBody>,
    /// Cell text per row (at most 5,000 rows); a row may be shorter than `columns`.
    rows: Vec<Vec<String>>,
    /// Converts and counts without writing anything.
    #[serde(default)]
    dry_run: bool,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct CreateDatabaseFromCsvBody {
    /// With a parent the database joins the parent's space.
    parent_id: Option<String>,
    /// Root databases only: the target teamspace.
    teamspace_id: Option<String>,
    /// Root databases only: `true` creates it in the caller's private space.
    private: Option<bool>,
    #[serde(default)]
    title: String,
    /// Index among the new siblings; omitted appends the database last.
    position: Option<i64>,
    /// The column that becomes the title property; its `target` is ignored.
    title_column: usize,
    /// One per CSV column; targets are `{new: type}` or `"skip"`.
    columns: Vec<ImportColumnBody>,
    rows: Vec<Vec<String>>,
    /// Converts and counts without creating anything (`database` is then `null`).
    #[serde(default)]
    dry_run: bool,
}

#[derive(Serialize, ToSchema)]
#[schema(as = DatabaseCsvCreated)]
struct CsvDatabaseResponse {
    #[schema(required = true)]
    database: Option<PageRecord>,
    result: ImportResult,
}

fn columns(
    input: Vec<ImportColumnBody>,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> Result<Vec<ImportColumn>, ApiError> {
    if input.len() > MAX_IMPORT_COLUMNS {
        return Err(validation("columns", instance, request_id));
    }
    input
        .into_iter()
        .enumerate()
        .map(|(index, column)| {
            let target = match column.target {
                ImportTargetBody::Property(body) => {
                    ImportTarget::Property(body.property_id.parse().map_err(|_| {
                        validation("columns", instance, request_id)
                            .with_detail(format!("columns[{index}].target"))
                    })?)
                }
                ImportTargetBody::New(body) => ImportTarget::New(body.new),
                ImportTargetBody::Skip(SkipTarget::Skip) => ImportTarget::Skip,
            };
            let name = bounded(
                column.name,
                MAX_NAME_CHARS * 4,
                MAX_NAME_CHARS * 16,
                "columns",
                instance,
                request_id,
            )?;
            Ok(ImportColumn { name, target })
        })
        .collect()
}

/// Adds the CSV's rows to the database, all or nothing (a merge never changes existing rows).
/// Cells convert with the type-change rules: values that do not fit stay empty and are counted
/// in `cleared`; select options are matched by name (case ignored) or created; person cells
/// match member names or emails. 409 `database_row_limit` when the rows do not fit.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/databases/{database_id}/import", params(("workspace_id" = String, Path), ("database_id" = String, Path)), request_body = ImportDatabaseCsvBody, responses((status = 200, body = ImportResult)))]
async fn import_database_csv(
    State(state): State<DatabaseState>,
    Path((workspace, database)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<ImportDatabaseCsvBody>,
) -> Result<Response, ApiError> {
    let request = request_id.as_ref();
    let instance = format!("/api/v1/workspaces/{workspace}/databases/{database}/import");
    let (workspace_id, database_id, actor_id) =
        scope(&state, &headers, &workspace, &database, &instance, request).await?;
    let input = CsvImport {
        columns: columns(body.columns, &instance, request)?,
        rows: body.rows,
    };
    state
        .databases
        .import_csv(
            workspace_id,
            database_id,
            actor_id,
            input,
            body.dry_run,
            request_id_value(request),
            TimestampMillis::now(),
        )
        .await
        .map(|result| Json(result).into_response())
        .map_err(|error| database_problem(error, instance, request))
}

/// Creates a database from a CSV in one transaction: a property per mapped column (the title
/// column first) and one table view, then a row per CSV row, converted as in
/// `POST …/databases/{database_id}/import`.
#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/databases/import", params(("workspace_id" = String, Path)), request_body = CreateDatabaseFromCsvBody, responses((status = 201, body = CsvDatabaseResponse), (status = 200, body = CsvDatabaseResponse)))]
async fn create_database_from_csv(
    State(state): State<DatabaseState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<CreateDatabaseFromCsvBody>,
) -> Result<Response, ApiError> {
    let request = request_id.as_ref();
    let instance = format!("/api/v1/workspaces/{workspace}/databases/import");
    let session = authenticate_session(
        &state.identity,
        state.cookie_mode,
        &headers,
        &instance,
        request,
    )
    .await?;
    let not_found = || {
        page_problem(
            crate::repositories::pages::PageError::NotFound,
            instance.clone(),
            request,
        )
    };
    let workspace_id = workspace.parse().map_err(|_| not_found())?;
    let actor_id = session.user.id;
    let mut mapped = columns(body.columns, &instance, request)?;
    let title_column = mapped
        .get_mut(body.title_column)
        .ok_or_else(|| validation("title_column", &instance, request))?;
    title_column.target = ImportTarget::New(PropertyType::Title);
    let input = CreatePage {
        kind: PageKind::Database,
        parent_id: body
            .parent_id
            .map(|value| value.parse().map_err(|_| not_found()))
            .transpose()?,
        space: space_request(body.teamspace_id, body.private, &instance, request)?,
        title: bounded(body.title, 500, 2000, "title", &instance, request)?,
        icon: None,
        position: match body.position {
            Some(value) if value < 0 => return Err(validation("position", &instance, request)),
            other => other,
        },
    };
    let (schema, result) = state
        .databases
        .csv_schema(
            workspace_id,
            actor_id,
            CsvImport {
                columns: mapped,
                rows: body.rows,
            },
        )
        .await
        .map_err(|error| database_problem(error, instance.clone(), request))?;
    if body.dry_run {
        let response = CsvDatabaseResponse {
            database: None,
            result,
        };
        return Ok(Json(response).into_response());
    }
    let page = PageRepository::new(state.identity.database().clone())
        .create_database_with_id(
            orbit_platform::Id::new_v7(),
            workspace_id,
            actor_id,
            input,
            &schema,
            request_id_value(request),
            TimestampMillis::now(),
        )
        .await
        .map_err(|error| page_problem(error, instance, request))?;
    let response = CsvDatabaseResponse {
        database: Some(page),
        result,
    };
    Ok((StatusCode::CREATED, Json(response)).into_response())
}
