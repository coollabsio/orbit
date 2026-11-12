//! The `Idempotency-Key` header of the task and comment create routes. The first successful
//! response is kept per token for 24 hours; a retry with the same key and request gets it again.
use axum::body::{Body, to_bytes};
use axum::extract::{Request, State};
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use orbit_platform::TimestampMillis;
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::{PublicApiState, Responder, problem_response};
use crate::repositories::api_idempotency::IdempotencyClaim;
use crate::repositories::api_tokens::{ApiTokenPrincipal, ApiTokenScope};
use crate::token_api::TokenApiError;

const HEADER: &str = "idempotency-key";
const MAX_KEY_LEN: usize = 255;
/// The size of the JSON body that axum's `Json` accepts, and of a response that Orbit keeps.
const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;
const REPLAYED: &str = "idempotent-replayed";

pub(super) async fn idempotency(
    State(state): State<PublicApiState>,
    request: Request,
    next: Next,
) -> Response {
    let mut keys = request.headers().get_all(HEADER).iter();
    // The layer sits on routes that also have a GET.
    let raw = keys.next().filter(|_| request.method() == Method::POST);
    let Some(raw) = raw else {
        return next.run(request).await;
    };
    let out = Responder::new(request.extensions(), request.uri());
    let key = match raw.to_str() {
        Ok(key)
            if keys.next().is_none()
                && (1..=MAX_KEY_LEN).contains(&key.len())
                && key.bytes().all(|byte| byte.is_ascii_graphic()) =>
        {
            key.to_owned()
        }
        _ => {
            return out.invalid(
                "Send one `Idempotency-Key` of 1 to 255 visible ASCII characters, such as a UUID.",
            );
        }
    };
    let Some(principal) = request.extensions().get::<ApiTokenPrincipal>().cloned() else {
        return out.error(TokenApiError::Internal(
            "idempotency layer ran without a token principal".into(),
        ));
    };
    // The handler answers 403 and reads no body.
    if !principal.allows(ApiTokenScope::TasksWrite) {
        return next.run(request).await;
    }
    let (parts, body) = request.into_parts();
    let Ok(body) = to_bytes(body, MAX_BODY_BYTES).await else {
        return problem_response(out.problem(
            StatusCode::PAYLOAD_TOO_LARGE,
            "payload_too_large",
            "Payload too large",
            "The request body is too large.",
        ));
    };
    let hash = request_hash(parts.uri.path(), &body);
    let repository = &state.tokens;
    let token_id = principal.token_id;
    let claim = repository
        .claim_idempotency_key(token_id, &key, &hash, TimestampMillis::now())
        .await;
    match claim {
        Ok(IdempotencyClaim::Claimed) => {}
        Ok(IdempotencyClaim::Replay { status, response }) => {
            return replay(status, response);
        }
        Ok(IdempotencyClaim::Mismatch) => {
            return problem_response(out.problem(
                StatusCode::UNPROCESSABLE_ENTITY,
                "idempotency_key_reused",
                "Idempotency key reused",
                "This `Idempotency-Key` was used for another request. Send a new key for a different request.",
            ));
        }
        Ok(IdempotencyClaim::Running) => {
            let mut response = problem_response(out.problem(
                StatusCode::CONFLICT,
                "idempotency_request_running",
                "Request still running",
                "The first request with this `Idempotency-Key` has not finished. Retry in a moment.",
            ));
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from_static("1"));
            return response;
        }
        Err(error) => {
            return out.error(TokenApiError::Internal(format!(
                "public API idempotency claim failed: {error}"
            )));
        }
    }

    let response = next.run(Request::from_parts(parts, Body::from(body))).await;
    let (head, body) = response.into_parts();
    let Ok(bytes) = to_bytes(body, MAX_BODY_BYTES).await else {
        release(&state, &principal, &key).await;
        return out.error(TokenApiError::Internal(
            "idempotency layer could not read the response body".into(),
        ));
    };
    // Only a success is kept: after an error nothing was made, so a retry may run again.
    match std::str::from_utf8(&bytes) {
        Ok(text) if head.status.is_success() => {
            if let Err(error) = repository
                .finish_idempotency_key(token_id, &key, head.status.as_u16(), text)
                .await
            {
                tracing::error!(%error, "public API idempotency response was not kept");
            }
        }
        _ => release(&state, &principal, &key).await,
    }
    Response::from_parts(head, Body::from(bytes))
}

async fn release(state: &PublicApiState, principal: &ApiTokenPrincipal, key: &str) {
    if let Err(error) = state
        .tokens
        .release_idempotency_key(principal.token_id, key)
        .await
    {
        tracing::error!(%error, "public API idempotency key was not released");
    }
}

fn replay(status: u16, response: String) -> Response {
    let mut response = (
        StatusCode::from_u16(status).unwrap_or(StatusCode::OK),
        [(header::CONTENT_TYPE, "application/json")],
        response,
    )
        .into_response();
    response
        .headers_mut()
        .insert(REPLAYED, HeaderValue::from_static("true"));
    response
}

/// The path and the JSON value of the body, so the order of fields and the spaces do not matter.
fn request_hash(path: &str, body: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(path.as_bytes());
    digest.update([0]);
    match serde_json::from_slice::<Value>(body) {
        // `Value` keeps object keys sorted.
        Ok(value) => digest.update(value.to_string().as_bytes()),
        Err(_) => digest.update(body),
    }
    digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
