//! Docs page export: `GET .../pages/{page_id}/export?format=markdown&children=` streams a ZIP of
//! Markdown files (see `crate::export`). Access follows the page's visibility.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::Response;
use axum::routing::get;
use orbit_platform::{Id, RequestId, TimestampMillis, UploadService};
use serde::Deserialize;
use tokio_util::io::ReaderStream;
use utoipa::{IntoParams, ToSchema};

use crate::auth_routes::CookieMode;
use crate::export::names::{ascii_file_name, rfc5987};
use crate::export::{ExportLimits, plan, write_archive};
use crate::repositories::identity::IdentityRepository;
use crate::repositories::page_export::{PageExportError, PageExportRepository};
use crate::repositories::page_files::PageFileRepository;
use crate::task_routes::{ApiError, ApiQuery, authenticate_session};

#[derive(Clone)]
pub struct ExportState {
    identity: Arc<IdentityRepository>,
    exports: Arc<PageExportRepository>,
    files: Arc<PageFileRepository>,
    cookie_mode: CookieMode,
    /// Absolute links to pages and files outside the export use this origin.
    public_origin: String,
    limits: ExportLimits,
}

impl ExportState {
    #[must_use]
    pub fn new(
        identity: Arc<IdentityRepository>,
        uploads: UploadService,
        cookie_mode: CookieMode,
        public_origin: String,
    ) -> Self {
        let database = identity.database().clone();
        Self {
            exports: Arc::new(PageExportRepository::new(database.clone())),
            files: Arc::new(PageFileRepository::new(database, uploads)),
            identity,
            cookie_mode,
            public_origin,
            limits: ExportLimits::default(),
        }
    }

    #[must_use]
    pub fn with_limits(mut self, limits: ExportLimits) -> Self {
        self.limits = limits;
        self
    }
}

pub fn export_router(state: ExportState) -> Router {
    Router::new()
        .route(
            "/api/v1/workspaces/{workspace_id}/pages/{page_id}/export",
            get(export_page),
        )
        .with_state(state)
}

#[derive(Clone, Copy, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
enum ExportFormat {
    Markdown,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
struct ExportQuery {
    /// Only `markdown`: one `.md` file per page with its files under `assets/`.
    #[allow(dead_code)]
    #[param(inline)]
    format: ExportFormat,
    /// Also export the page's live sub-pages (in folders named after their parent). Defaults
    /// to `false`.
    #[serde(default)]
    children: bool,
}

#[derive(ToSchema)]
#[schema(value_type = String, format = Binary)]
#[allow(dead_code)]
struct PageExportArchive(Vec<u8>);

/// ZIP archive of the page as Markdown (`<Title>.md`); with `children=true` its sub-pages go
/// into the `<Title>/` folder next to it, each page's files into `<Title>/assets/`. Links
/// between exported pages are relative, other page links absolute. At most 500 pages and
/// 200 MiB uncompressed (413 otherwise).
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/pages/{page_id}/export", params(ExportQuery, ("workspace_id" = String, Path), ("page_id" = String, Path)), responses((status = 200, body = PageExportArchive, content_type = "application/zip")))]
async fn export_page(
    State(state): State<ExportState>,
    Path((workspace, page)): Path<(String, String)>,
    ApiQuery(query): ApiQuery<ExportQuery>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Response, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/pages/{page}/export");
    let request_id = request_id.as_ref();
    let session = authenticate_session(
        &state.identity,
        state.cookie_mode,
        &headers,
        &instance,
        request_id,
    )
    .await?;
    let actor_id = session.user.id;
    let parse = |value: &str| -> Result<Id, ApiError> {
        value
            .parse()
            .map_err(|_| problem(PageExportError::NotFound, &instance, request_id))
    };
    let workspace_id = parse(&workspace)?;
    let page_id = parse(&page)?;
    let fail = |error| problem(error, &instance, request_id);
    let source = state
        .exports
        .load(
            workspace_id,
            page_id,
            actor_id,
            query.children,
            state.limits.max_pages,
        )
        .await
        .map_err(fail)?;
    let exported_at = TimestampMillis::now();
    let plan = plan(
        &source,
        workspace_id,
        &state.public_origin,
        exported_at,
        state.limits,
    )
    .map_err(fail)?;
    let name = format!("{}.zip", plan.name);
    let file = write_archive(
        plan,
        &state.files,
        workspace_id,
        actor_id,
        exported_at,
        state.limits,
    )
    .await
    .map_err(fail)?;
    let length = file.metadata().map(|metadata| metadata.len()).ok();
    let disposition = format!(
        "attachment; filename=\"{}\"; filename*={}",
        ascii_file_name(&name),
        rfc5987(&name)
    );
    let mut response = Response::new(Body::from_stream(ReaderStream::new(
        tokio::fs::File::from_std(file),
    )));
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/zip"),
    );
    if let Ok(value) = HeaderValue::from_str(&disposition) {
        headers.insert(header::CONTENT_DISPOSITION, value);
    }
    if let Some(length) = length {
        headers.insert(header::CONTENT_LENGTH, HeaderValue::from(length));
    }
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    Ok(response)
}

fn problem(
    error: PageExportError,
    instance: &str,
    request_id: Option<&Extension<RequestId>>,
) -> ApiError {
    match error {
        PageExportError::NotFound => ApiError::new(
            StatusCode::NOT_FOUND,
            "page_not_found",
            "Page not found",
            "The requested page was not found.",
            instance,
            request_id,
        ),
        PageExportError::TooManyPages { .. } => ApiError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "export_too_many_pages",
            "Export too large",
            "The page has too many sub-pages to export at once (at most 500). Export a smaller part of the tree.",
            instance,
            request_id,
        ),
        PageExportError::TooLarge { .. } => ApiError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "export_too_large",
            "Export too large",
            "The pages and their files exceed the export size limit (200 MiB uncompressed). Export fewer sub-pages.",
            instance,
            request_id,
        ),
        PageExportError::Corrupt | PageExportError::Io(_) | PageExportError::Unavailable(_) => {
            tracing::error!(error = %error, "page export failed");
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Internal server error",
                "An unexpected error occurred. Use the request ID when contacting support.",
                instance,
                request_id,
            )
        }
    }
}
