//! GIFs and stickers from KLIPY for the composer. The server holds the API key and searches; the browser shows
//! the images from KLIPY's CDN.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, Uri};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use super::{Call, ChatState, RequestIdExtension};
use crate::klipy::{KlipyCatalog, KlipyPage};
use crate::repositories::chat::ChatError;
use crate::repositories::instance_settings::InstanceSettingsRepository;
use crate::task_routes::{ApiError, ApiQuery};

/// Characters of a search.
const MAX_QUERY_CHARS: usize = 100;
/// The last page that can be read.
const MAX_PAGE: u32 = 50;

#[derive(Serialize, ToSchema)]
#[schema(as = ChatKlipyStatus)]
pub(crate) struct ChatKlipyStatus {
    /// A KLIPY API key is set: the composer can offer GIFs and stickers.
    enabled: bool,
}

#[derive(Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub(crate) struct ChatKlipyQuery {
    /// Search words, at most 100 characters; blank or absent lists what is trending.
    q: Option<String>,
    /// 1–50; default 1.
    page: Option<u32>,
}

/// Whether the composer can offer GIFs and stickers from KLIPY.
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/chat/klipy", params(("workspace_id" = String, Path)), responses((status = 200, body = ChatKlipyStatus)))]
pub(crate) async fn get_chat_klipy(
    State(state): State<ChatState>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
) -> Result<Json<ChatKlipyStatus>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    state
        .chat
        .require_member(call.workspace_id, call.actor_id)
        .await
        .map_err(|error| call.problem(error))?;
    Ok(Json(ChatKlipyStatus {
        enabled: api_key(&state, &call).await?.is_some(),
    }))
}

/// Trending GIFs or stickers (`catalog` is `gifs` or `stickers`), or search results for `q`. 404
/// `klipy_not_configured` without a KLIPY API key, 502 `klipy_unavailable` when KLIPY does not answer.
#[utoipa::path(get, path = "/api/v1/workspaces/{workspace_id}/chat/klipy/{catalog}", params(ChatKlipyQuery, ("workspace_id" = String, Path), ("catalog" = String, Path, description = "`gifs` or `stickers`")), responses((status = 200, body = KlipyPage)))]
pub(crate) async fn list_chat_klipy(
    State(state): State<ChatState>,
    Path((workspace, catalog)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    request_id: RequestIdExtension,
    ApiQuery(query): ApiQuery<ChatKlipyQuery>,
) -> Result<Json<KlipyPage>, ApiError> {
    let call = Call::enter(&state, &headers, &uri, &workspace, &request_id).await?;
    state
        .chat
        .require_member(call.workspace_id, call.actor_id)
        .await
        .map_err(|error| call.problem(error))?;
    let catalog = KlipyCatalog::parse(&catalog).ok_or_else(|| call.problem(ChatError::NotFound))?;
    let q = query.q.as_deref().map(str::trim).unwrap_or_default();
    if q.chars().count() > MAX_QUERY_CHARS {
        return Err(call.problem(ChatError::Invalid { field: "q" }));
    }
    let page = query.page.unwrap_or(1).clamp(1, MAX_PAGE);
    let Some(api_key) = api_key(&state, &call).await? else {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "klipy_not_configured",
            "GIFs are not set up",
            "The installation admin has not set a KLIPY API key.",
            &call.instance,
            call.request_id,
        ));
    };
    match state.klipy.browse(&api_key, catalog, q, page).await {
        Ok(page) => Ok(Json(page)),
        Err(error) => {
            // The error never names the URL, which holds the key.
            tracing::warn!(error = %error, "KLIPY search");
            Err(ApiError::new(
                StatusCode::BAD_GATEWAY,
                "klipy_unavailable",
                "KLIPY is unavailable",
                "KLIPY did not answer. Try again in a moment.",
                &call.instance,
                call.request_id,
            ))
        }
    }
}

/// The KLIPY API key, decrypted; `None` when none is set or the server has no app key to read it.
async fn api_key(state: &ChatState, call: &Call<'_>) -> Result<Option<String>, ApiError> {
    let sealed = InstanceSettingsRepository::new(state.identity.database().clone())
        .klipy_api_key()
        .await
        .map_err(|error| call.problem(ChatError::Unavailable(error)))?;
    let (Some(sealed), Some(app_key)) = (sealed, state.app_key.as_ref()) else {
        return Ok(None);
    };
    match crate::secret_box::decrypt_secret(app_key, &sealed) {
        Ok(key) => Ok(Some(key)),
        Err(()) => {
            tracing::warn!("the saved KLIPY API key cannot be decrypted with the app key");
            Ok(None)
        }
    }
}
