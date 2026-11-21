//! The token user's inbox. Personal tokens only: a service-account token gets 403. A
//! notification shows only when the token may read what it points to (see
//! `token_api::notifications`).
use axum::Router;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::{get, post};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use super::{Ctx, List, PublicApiState, list, problem_response};
use crate::repositories::api_tokens::ApiTokenScope;
use crate::token_api::{NotificationView, TokenApiError};
use orbit_platform::Id;

pub(super) fn routes() -> Router<PublicApiState> {
    Router::new()
        .route("/notifications", get(notifications))
        .route("/notifications/read-all", post(read_all_notifications))
        .route(
            "/notifications/{notification_id}/read",
            post(read_notification),
        )
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(deny_unknown_fields)]
pub(super) struct NotificationQuery {
    /// `true`: only unread notifications.
    #[serde(default)]
    unread: bool,
    /// The `next_cursor` of the previous page.
    cursor: Option<String>,
    /// 1 to 100; 50 when unset.
    #[param(minimum = 1, maximum = 100)]
    limit: Option<usize>,
}

#[derive(Serialize, ToSchema)]
#[schema(as = PublicReadAllNotifications)]
pub(super) struct ReadAllOut {
    /// How many notifications changed from unread to read.
    updated: u64,
}

#[utoipa::path(
    get,
    path = "/api/public/v1/workspaces/{workspace_id}/notifications",
    operation_id = "public_list_notifications",
    tag = "public-api",
    summary = "The token user's notifications, newest first",
    description = "Only notifications that the token may read: a task notification needs the task's project in the grant, a doc mention `docs:read`, a chat mention or report `chat:read`, a GitHub notice a repository connected to an approved project. A page can hold fewer than `limit` items before the last one: follow `next_cursor`. Not for service-account tokens (`403`).",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), NotificationQuery),
    responses((status = 200, description = "A page of notifications", body = List<NotificationView>)),
    security(("bearerAuth" = []))
)]
pub(super) async fn notifications(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    query: Result<Query<NotificationQuery>, QueryRejection>,
) -> Response {
    let query = input!(ctx, ApiTokenScope::TasksRead, query);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksRead, &ctx.request_id)
            .await?;
        let page = api
            .notifications(query.unread, query.cursor.as_deref(), query.limit)
            .await?;
        Ok(list::<_, NotificationView>(page.items, page.next_cursor))
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    post,
    path = "/api/public/v1/workspaces/{workspace_id}/notifications/{notification_id}/read",
    operation_id = "public_read_notification",
    tag = "public-api",
    summary = "Mark a notification as read",
    description = "A notification that is already read keeps its `read_at`. Not for service-account tokens (`403`).",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`."), ("notification_id" = String, Path)),
    responses((status = 200, description = "The notification", body = NotificationView)),
    security(("bearerAuth" = []))
)]
pub(super) async fn read_notification(
    State(s): State<PublicApiState>,
    ctx: Ctx,
    Path((_workspace, notification)): Path<(String, String)>,
) -> Response {
    let notification = path_id!(ctx, notification);
    let result = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksWrite, &ctx.request_id)
            .await?;
        api.mark_notification_read(notification).await
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}

#[utoipa::path(
    post,
    path = "/api/public/v1/workspaces/{workspace_id}/notifications/read-all",
    operation_id = "public_read_all_notifications",
    tag = "public-api",
    summary = "Mark every notification that the token can list as read",
    description = "Notifications that the token cannot list stay unread. At most 5,000 per call. Not for service-account tokens (`403`).",
    params(("workspace_id" = String, Path, description = "The token's workspace, from `GET /me`.")),
    responses((status = 200, description = "How many changed", body = ReadAllOut)),
    security(("bearerAuth" = []))
)]
pub(super) async fn read_all_notifications(State(s): State<PublicApiState>, ctx: Ctx) -> Response {
    let result: Result<ReadAllOut, TokenApiError> = async {
        let api = s
            .api
            .authorize(&ctx.principal, ApiTokenScope::TasksWrite, &ctx.request_id)
            .await?;
        let updated = api.mark_notifications_read().await?;
        Ok(ReadAllOut { updated })
    }
    .await;
    ctx.reply(StatusCode::OK, result)
}
