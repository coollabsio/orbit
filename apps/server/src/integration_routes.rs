use std::sync::Arc;

use axum::extract::{Extension, FromRequest, Request, State};
use axum::http::header::AUTHORIZATION;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use orbit_platform::{Problem, RequestId, TimestampMillis};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use utoipa::ToSchema;

use crate::repositories::api_tokens::{ApiTokenError, ApiTokenRepository, ApiTokenScope};
use crate::repositories::tasks::{DiscordTask, TaskError, TaskRecord, TaskRepository};

const INSTANCE: &str = "/api/v1/integrations/discord/events";
const MAX_EVENT_ID_CHARS: usize = 200;
const MAX_DESCRIPTION_CHARS: usize = 100_000;
const MAX_URL_CHARS: usize = 2_048;

#[derive(Clone)]
pub struct IntegrationState {
    tokens: Arc<ApiTokenRepository>,
    tasks: Arc<TaskRepository>,
}

impl IntegrationState {
    #[must_use]
    pub fn new(tokens: Arc<ApiTokenRepository>, tasks: Arc<TaskRepository>) -> Self {
        Self { tokens, tasks }
    }
}

pub fn integration_router(state: IntegrationState) -> Router {
    Router::new()
        .route(INSTANCE, post(create_discord_event))
        .with_state(state)
}

pub(crate) struct ApiJson<T>(T);

impl<S, T> FromRequest<S> for ApiJson<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = ApiError;

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        let request_id = request.extensions().get::<RequestId>().cloned();
        Json::<T>::from_request(request, state)
            .await
            .map(|Json(value)| Self(value))
            .map_err(|_| {
                ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "Invalid request",
                    "The request body is not valid for this endpoint.",
                    request_id.as_ref(),
                )
            })
    }
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DiscordEventBody {
    event_id: String,
    message: String,
    message_url: String,
    project_id: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct DiscordEventResponse {
    task: TaskRecord,
    duplicate: bool,
}

#[utoipa::path(
    post,
    path = "/api/v1/integrations/discord/events",
    request_body = DiscordEventBody,
    responses(
        (status = 201, description = "Task created", body = DiscordEventResponse),
        (status = 200, description = "Existing task returned for a repeated event", body = DiscordEventResponse)
    ),
    security(("bearerAuth" = []))
)]
pub(crate) async fn create_discord_event(
    State(state): State<IntegrationState>,
    headers: HeaderMap,
    request_id: Option<Extension<RequestId>>,
    ApiJson(body): ApiJson<DiscordEventBody>,
) -> Result<Response, ApiError> {
    let token = bearer_token(&headers).ok_or_else(|| {
        ApiError::new(
            StatusCode::UNAUTHORIZED,
            "api_token_required",
            "API token required",
            "Supply an API token with the Bearer authorization scheme.",
            request_id.as_ref().map(|value| &value.0),
        )
    })?;
    let principal = state
        .tokens
        .authenticate(token, ApiTokenScope::Write, TimestampMillis::now())
        .await
        .map_err(|error| token_problem(error, request_id.as_ref().map(|value| &value.0)))?;
    let project_id = match body.project_id.as_deref() {
        Some(value) => value
            .parse()
            .ok()
            .filter(|id| principal.project_ids.contains(id)),
        None if principal.project_ids.len() == 1 => principal.project_ids.first().copied(),
        None => None,
    }
    .ok_or_else(|| validation("project_id", request_id.as_ref().map(|value| &value.0)))?;
    let event_id = body.event_id.trim();
    let message = body.message.as_str();
    let message_url = body.message_url.trim();
    if event_id.is_empty() || event_id.chars().count() > MAX_EVENT_ID_CHARS {
        return Err(validation(
            "event_id",
            request_id.as_ref().map(|value| &value.0),
        ));
    }
    if message.trim().is_empty() {
        return Err(validation(
            "message",
            request_id.as_ref().map(|value| &value.0),
        ));
    }
    if message_url.chars().count() > MAX_URL_CHARS || !valid_discord_url(message_url) {
        return Err(validation(
            "message_url",
            request_id.as_ref().map(|value| &value.0),
        ));
    }
    let description = format!("{message}\n\nSource: {message_url}");
    if description.chars().count() > MAX_DESCRIPTION_CHARS {
        return Err(validation(
            "message",
            request_id.as_ref().map(|value| &value.0),
        ));
    }
    let payload_hash: [u8; 32] = Sha256::new()
        .chain_update(message.as_bytes())
        .chain_update([0])
        .chain_update(message_url.as_bytes())
        .finalize()
        .into();
    let (task, created) = state
        .tasks
        .create_discord_task(
            principal.workspace_id,
            principal.creator_id,
            principal.service_account_id,
            DiscordTask {
                event_id: event_id.to_owned(),
                payload_hash,
                project_id,
                title: discord_title(message),
                description,
            },
            request_id
                .as_ref()
                .map_or("unknown", |value| value.0.as_str()),
            TimestampMillis::now(),
        )
        .await
        .map_err(|error| task_problem(error, request_id.as_ref().map(|value| &value.0)))?;
    Ok((
        if created {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        },
        Json(DiscordEventResponse {
            task,
            duplicate: !created,
        }),
    )
        .into_response())
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(AUTHORIZATION)?.to_str().ok()?;
    let token = value.strip_prefix("Bearer ")?;
    (!token.is_empty() && !token.chars().any(char::is_whitespace)).then_some(token)
}

fn valid_discord_url(value: &str) -> bool {
    value.starts_with("https://discord.com/channels/") && !value.chars().any(char::is_whitespace)
}

fn discord_title(message: &str) -> String {
    let line = message
        .lines()
        .find_map(|line| {
            let collapsed = line.split_whitespace().collect::<Vec<_>>().join(" ");
            (!collapsed.is_empty()).then_some(collapsed)
        })
        .unwrap_or_else(|| "Discord message".to_owned());
    if line.chars().count() <= 40 {
        line
    } else {
        format!("{}...", line.chars().take(37).collect::<String>())
    }
}

fn token_problem(error: ApiTokenError, request_id: Option<&RequestId>) -> ApiError {
    match error {
        ApiTokenError::Forbidden => ApiError::new(
            StatusCode::FORBIDDEN,
            "api_token_scope_forbidden",
            "API token scope forbidden",
            "This API token does not have the write scope.",
            request_id,
        ),
        ApiTokenError::NotFound => ApiError::new(
            StatusCode::UNAUTHORIZED,
            "invalid_api_token",
            "Invalid API token",
            "The API token is invalid or revoked.",
            request_id,
        ),
        ApiTokenError::Unavailable(_) | ApiTokenError::InvalidIdentifier => {
            ApiError::internal(request_id)
        }
    }
}

fn task_problem(error: TaskError, request_id: Option<&RequestId>) -> ApiError {
    match error {
        TaskError::IntegrationConflict => ApiError::new(
            StatusCode::CONFLICT,
            "integration_event_conflict",
            "Integration event conflict",
            "This event ID was already used with different content.",
            request_id,
        ),
        TaskError::NotFound => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "integration_project_unavailable",
            "Integration project unavailable",
            "The token project or its initial status is unavailable.",
            request_id,
        ),
        TaskError::Invalid { field } => validation(field, request_id),
        TaskError::Conflict
        | TaskError::RestoreConflict { .. }
        | TaskError::VersionConflict { .. }
        | TaskError::InvalidCursor
        | TaskError::Unavailable(_) => ApiError::internal(request_id),
    }
}

fn validation(field: &'static str, request_id: Option<&RequestId>) -> ApiError {
    ApiError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        "validation_failed",
        "Validation failed",
        match field {
            "event_id" => "Event IDs must contain between 1 and 200 characters.",
            "message_url" => {
                "Message URLs must be Discord channel message URLs of at most 2048 characters."
            }
            _ => {
                "Messages must contain text and the stored description must not exceed 100000 characters."
            }
        },
        request_id,
    )
}

pub struct ApiError {
    status: StatusCode,
    problem: Problem,
}

impl ApiError {
    fn new(
        status: StatusCode,
        code: &str,
        title: &str,
        detail: &str,
        request_id: Option<&RequestId>,
    ) -> Self {
        Self {
            status,
            problem: Problem {
                type_uri: format!("https://docs.orbit.dev/problems/{code}"),
                title: title.to_owned(),
                status: status.as_u16(),
                code: code.to_owned(),
                detail: detail.to_owned(),
                instance: INSTANCE.to_owned(),
                request_id: request_id
                    .map_or_else(|| RequestId::new().to_string(), ToString::to_string),
                errors: None,
                conflict: None,
            },
        }
    }

    fn internal(request_id: Option<&RequestId>) -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "Internal server error",
            "An unexpected error occurred. Use the request ID when contacting support.",
            request_id,
        )
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            [(axum::http::header::CONTENT_TYPE, "application/problem+json")],
            Json(self.problem),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::discord_title;

    #[test]
    fn discord_titles_use_the_first_nonempty_line_and_unicode_character_limit() {
        assert_eq!(
            discord_title("\n  A   short title  \nbody"),
            "A short title"
        );
        let title = discord_title("1234567890123456789012345678901234567890extra");
        assert_eq!(title, "1234567890123456789012345678901234567...");
        assert_eq!(title.chars().count(), 40);
    }
}
