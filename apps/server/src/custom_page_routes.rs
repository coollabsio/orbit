//! Custom pages in the sidebar (#94). The web app frames each page's URL in a sandboxed iframe; the URL must be on
//! one of the custom page origins ([`crate::frame_origins`]), which the app's `frame-src` allows.

use std::sync::Arc;

use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch};
use axum::{Json, Router};
use orbit_platform::{FrameOrigins, Id, RequestId, TimestampMillis};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::auth_routes::CookieMode;
use crate::repositories::custom_pages::{
    CustomPageChanges, CustomPageError, CustomPageRecord, CustomPageRepository, NewCustomPage,
    https_url, is_slug,
};
use crate::repositories::identity::IdentityRepository;
use crate::task_routes::{
    ApiError, ApiJson, ApiQuery, MutationQuery, authenticate_session, bounded,
    deserialize_source_patch, request_id_value, validation,
};

#[derive(Clone)]
pub struct CustomPageState {
    identity: Arc<IdentityRepository>,
    pages: Arc<CustomPageRepository>,
    cookie_mode: CookieMode,
    /// The current custom page origins ([`crate::frame_origins`]).
    frame_origins: FrameOrigins,
}

impl CustomPageState {
    #[must_use]
    pub fn new(identity: Arc<IdentityRepository>, cookie_mode: CookieMode) -> Self {
        Self {
            pages: Arc::new(CustomPageRepository::new(identity.database().clone())),
            frame_origins: crate::frame_origins::of(identity.database()),
            identity,
            cookie_mode,
        }
    }
}

pub fn custom_page_router(state: CustomPageState) -> Router {
    Router::new()
        .route(
            "/api/v1/workspaces/{workspace_id}/custom-pages",
            get(list_custom_pages).post(create_custom_page),
        )
        .route(
            "/api/v1/workspaces/{workspace_id}/custom-pages/{page_id}",
            patch(update_custom_page).delete(delete_custom_page),
        )
        .with_state(state)
}

#[derive(Serialize, ToSchema)]
pub struct CustomPageList {
    pub items: Vec<CustomPageRecord>,
    /// Whether the caller may add, change and remove pages.
    pub can_manage: bool,
    /// The origins a page URL may use (Admin, or `ORBIT_FRAME_ORIGINS`). A page whose origin left the list is not framed.
    pub frame_origins: Vec<String>,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct CreateCustomPageBody {
    /// 1 to 48 lowercase letters, digits and single hyphens; the page opens at `/pages/{slug}`.
    slug: String,
    /// 1 to 60 characters after trimming.
    name: String,
    icon: Option<String>,
    /// An https URL on one of `frame_origins`.
    url: String,
    #[serde(default)]
    admins_only: bool,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct CustomPageUpdateBody {
    expected_version: u64,
    slug: Option<String>,
    name: Option<String>,
    /// Absent: unchanged. `null`: cleared.
    #[serde(default, deserialize_with = "deserialize_source_patch")]
    icon: Option<Option<String>>,
    url: Option<String>,
    admins_only: Option<bool>,
}

type Request<'a> = Option<&'a Extension<RequestId>>;

#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/custom-pages", params(("workspace_id" = String, Path)), responses((status = 200, body = CustomPageList)))]
async fn list_custom_pages(
    State(state): State<CustomPageState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<Json<CustomPageList>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/custom-pages");
    let request = request_id.as_ref();
    let (workspace_id, actor_id) = scope(&state, &headers, &workspace, &instance, request).await?;
    let (items, can_manage) = state
        .pages
        .pages(workspace_id, actor_id)
        .await
        .map_err(|error| page_problem(error, instance, request))?;
    Ok(Json(CustomPageList {
        items,
        can_manage,
        frame_origins: state.frame_origins.get().to_vec(),
    }))
}

#[utoipa::path(post, path = "/api/v1/workspaces/{workspace_id}/custom-pages", params(("workspace_id" = String, Path)), request_body = CreateCustomPageBody, responses((status = 201, body = CustomPageRecord)))]
async fn create_custom_page(
    State(state): State<CustomPageState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<CreateCustomPageBody>,
) -> Result<Response, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/custom-pages");
    let request = request_id.as_ref();
    let (workspace_id, actor_id) = scope(&state, &headers, &workspace, &instance, request).await?;
    let page = NewCustomPage {
        slug: slug(body.slug, &instance, request)?,
        name: name(body.name, &instance, request)?,
        icon: body
            .icon
            .map(|value| icon(value, &instance, request))
            .transpose()?,
        url: page_url(&state, &body.url, &instance, request)?,
        admins_only: body.admins_only,
    };
    state
        .pages
        .create_page(
            workspace_id,
            actor_id,
            page,
            request_id_value(request),
            TimestampMillis::now(),
        )
        .await
        .map(|record| (StatusCode::CREATED, Json(record)).into_response())
        .map_err(|error| page_problem(error, instance, request))
}

#[utoipa::path(patch, path = "/api/v1/workspaces/{workspace_id}/custom-pages/{page_id}", params(("workspace_id" = String, Path), ("page_id" = String, Path)), request_body = CustomPageUpdateBody, responses((status = 200, body = CustomPageRecord)))]
async fn update_custom_page(
    State(state): State<CustomPageState>,
    Path((workspace, page)): Path<(String, String)>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<CustomPageUpdateBody>,
) -> Result<Json<CustomPageRecord>, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/custom-pages/{page}");
    let request = request_id.as_ref();
    let (workspace_id, actor_id) = scope(&state, &headers, &workspace, &instance, request).await?;
    let page_id = parse_id(&page, &instance, request)?;
    let changes = CustomPageChanges {
        slug: body
            .slug
            .map(|value| slug(value, &instance, request))
            .transpose()?,
        name: body
            .name
            .map(|value| name(value, &instance, request))
            .transpose()?,
        icon: body
            .icon
            .map(|value| {
                value
                    .map(|value| icon(value, &instance, request))
                    .transpose()
            })
            .transpose()?,
        url: body
            .url
            .map(|value| page_url(&state, &value, &instance, request))
            .transpose()?,
        admins_only: body.admins_only,
    };
    state
        .pages
        .update_page(
            workspace_id,
            page_id,
            actor_id,
            body.expected_version,
            changes,
            request_id_value(request),
            TimestampMillis::now(),
        )
        .await
        .map(Json)
        .map_err(|error| page_problem(error, instance, request))
}

#[utoipa::path(delete, path = "/api/v1/workspaces/{workspace_id}/custom-pages/{page_id}", params(MutationQuery, ("workspace_id" = String, Path), ("page_id" = String, Path)), responses((status = 204)))]
async fn delete_custom_page(
    State(state): State<CustomPageState>,
    Path((workspace, page)): Path<(String, String)>,
    ApiQuery(query): ApiQuery<MutationQuery>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
) -> Result<StatusCode, ApiError> {
    let instance = format!("/api/v1/workspaces/{workspace}/custom-pages/{page}");
    let request = request_id.as_ref();
    let (workspace_id, actor_id) = scope(&state, &headers, &workspace, &instance, request).await?;
    let page_id = parse_id(&page, &instance, request)?;
    state
        .pages
        .delete_page(
            workspace_id,
            page_id,
            actor_id,
            query.expected_version,
            request_id_value(request),
            TimestampMillis::now(),
        )
        .await
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(|error| page_problem(error, instance, request))
}

async fn scope(
    state: &CustomPageState,
    headers: &HeaderMap,
    workspace: &str,
    instance: &str,
    request: Request<'_>,
) -> Result<(Id, Id), ApiError> {
    let session = authenticate_session(
        &state.identity,
        state.cookie_mode,
        headers,
        instance,
        request,
    )
    .await?;
    Ok((parse_id(workspace, instance, request)?, session.user.id))
}

fn parse_id(value: &str, instance: &str, request: Request<'_>) -> Result<Id, ApiError> {
    value
        .parse()
        .map_err(|_| not_found(instance.to_owned(), request))
}

fn slug(value: String, instance: &str, request: Request<'_>) -> Result<String, ApiError> {
    if is_slug(&value) {
        Ok(value)
    } else {
        Err(validation("slug", instance, request))
    }
}

fn name(value: String, instance: &str, request: Request<'_>) -> Result<String, ApiError> {
    let value = value.trim().to_owned();
    if value.is_empty() {
        return Err(validation("name", instance, request));
    }
    bounded(value, 60, 240, "name", instance, request)
}

fn icon(value: String, instance: &str, request: Request<'_>) -> Result<String, ApiError> {
    if value.trim().is_empty() {
        return Err(validation("icon", instance, request));
    }
    bounded(value, 64, 256, "icon", instance, request)
}

/// An https URL without credentials ([`https_url`]) on one of the operator's frame origins.
fn page_url(
    state: &CustomPageState,
    value: &str,
    instance: &str,
    request: Request<'_>,
) -> Result<String, ApiError> {
    let url = https_url(value).ok_or_else(|| validation("url", instance, request))?;
    let origin = url.origin().ascii_serialization();
    if !state.frame_origins.get().contains(&origin) {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "custom_page_origin_not_allowed",
            "Origin not allowed",
            "The URL's origin is not an allowed custom page origin. Ask the server administrator to add it in Admin.",
            instance.to_owned(),
            request,
        ));
    }
    Ok(url.into())
}

fn not_found(instance: String, request: Request<'_>) -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        "custom_page_not_found",
        "Custom page not found",
        "The requested custom page was not found.",
        instance,
        request,
    )
}

fn page_problem(error: CustomPageError, instance: String, request: Request<'_>) -> ApiError {
    match error {
        CustomPageError::NotFound => not_found(instance, request),
        CustomPageError::Forbidden => ApiError::new(
            StatusCode::FORBIDDEN,
            "workspace_action_forbidden",
            "Workspace action forbidden",
            "Only workspace owners and administrators may manage custom pages.",
            instance,
            request,
        ),
        CustomPageError::SlugTaken => ApiError::new(
            StatusCode::CONFLICT,
            "custom_page_slug_taken",
            "Address taken",
            "Another custom page already uses this address.",
            instance,
            request,
        ),
        CustomPageError::VersionConflict { current } => {
            ApiError::version_conflict(*current, instance, request)
        }
        error @ (CustomPageError::Unavailable(_) | CustomPageError::Corrupt) => ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "Internal server error",
            "An unexpected error occurred. Use the request ID when contacting support.",
            instance,
            request,
        )
        .cause(error),
    }
}
