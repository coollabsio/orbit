//! The custom page policy of this server (#94): `ORBIT_FRAME_ORIGINS`, or else the one that the root user saves in
//! Admin. Every read goes to the database, so all servers use a saved change at once. The app's HTML document gets
//! the origins of the pages that the signed-in user sees in its `frame-src` ([`document_frame_sources`]).

use axum::extract::{Request, State};
use axum::http::header::CONTENT_TYPE;
use axum::middleware::Next;
use axum::response::Response;
use orbit_platform::{CspFrameSources, Database, FramePolicy, frame_origin};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::custom_page_routes::CustomPageState;
use crate::repositories::environment_settings::EnvironmentSettings;

/// Which URLs workspace admins may use for custom pages.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum CustomPageMode {
    /// Any https site.
    Any,
    /// Only the listed origins.
    Listed,
    /// No custom pages.
    Off,
}

impl CustomPageMode {
    /// The `instance_settings.custom_page_mode` value.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Any => "any",
            Self::Listed => "listed",
            Self::Off => "off",
        }
    }
}

/// The policy as the API shows it.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct CustomPagePolicy {
    pub mode: CustomPageMode,
    /// The listed origins: the ones the `listed` mode allows. Admin keeps them in the other modes too.
    pub origins: Vec<String>,
}

/// The policy in force: the environment's, or the saved one.
pub async fn policy(database: &Database) -> Result<FramePolicy, sqlx::Error> {
    if let Some(policy) = &EnvironmentSettings::of(database).frame_policy {
        return Ok(policy.clone());
    }
    Ok(match saved(database).await? {
        CustomPagePolicy {
            mode: CustomPageMode::Any,
            ..
        } => FramePolicy::Any,
        CustomPagePolicy {
            mode: CustomPageMode::Off,
            ..
        } => FramePolicy::Off,
        CustomPagePolicy { origins, .. } => FramePolicy::Listed(origins),
    })
}

/// The policy saved in Admin. Origins were checked when saved; one that no longer parses is skipped.
pub async fn saved(database: &Database) -> Result<CustomPagePolicy, sqlx::Error> {
    let (mode, origins): (String, String) = sqlx::query_as(
        "SELECT custom_page_mode, frame_origins FROM instance_settings WHERE id = 1",
    )
    .fetch_one(database.pool())
    .await?;
    Ok(CustomPagePolicy {
        mode: match mode.as_str() {
            "listed" => CustomPageMode::Listed,
            "off" => CustomPageMode::Off,
            _ => CustomPageMode::Any,
        },
        origins: origins.lines().filter_map(frame_origin).collect(),
    })
}

/// `policy` as the API shows it.
#[must_use]
pub fn view(policy: &FramePolicy) -> CustomPagePolicy {
    CustomPagePolicy {
        mode: match policy {
            FramePolicy::Any => CustomPageMode::Any,
            FramePolicy::Listed(_) => CustomPageMode::Listed,
            FramePolicy::Off => CustomPageMode::Off,
        },
        origins: policy.origins().to_vec(),
    }
}

/// The origin of a page URL as `frame-src` writes it, when `policy` lets the app frame it. Never Orbit's own origin:
/// a sandbox with scripts and the same origin is no sandbox.
#[must_use]
pub fn framed_origin(policy: &FramePolicy, url: &str, public_origin: &str) -> Option<String> {
    let origin = frame_origin(&url::Url::parse(url).ok()?.origin().ascii_serialization())?;
    (policy.allows(&origin) && Some(&origin) != frame_origin(public_origin).as_ref())
        .then_some(origin)
}

/// On the app's HTML document, adds the origins of the custom pages that the signed-in user sees to `frame-src`. A
/// document loaded before a page was added does not have its origin: the app loads the document again.
pub async fn document_frame_sources(
    State(state): State<CustomPageState>,
    request: Request,
    next: Next,
) -> Response {
    let headers = request.headers().clone();
    let mut response = next.run(request).await;
    let is_document = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("text/html"));
    if !is_document {
        return response;
    }
    match state.frame_sources(&headers).await {
        Ok(sources) if !sources.is_empty() => {
            response.extensions_mut().insert(CspFrameSources(sources));
        }
        Ok(_) => {}
        Err(error) => tracing::warn!(%error, "custom page origins for the app document"),
    }
    response
}
