use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use orbit_platform::{HealthRegistry, HttpLimits, HttpPlatformLayer, OriginPolicy};

use crate::attachment_routes::{AttachmentState, attachment_router};
use crate::auth_routes::{AuthState, auth_router};
use crate::static_assets::StaticAssets;
use crate::task_routes::{TaskState, task_router};
use crate::workspace_routes::{WorkspaceState, workspace_router};

pub struct ApiRoutes {
    pub auth: AuthState,
    pub workspaces: WorkspaceState,
    pub tasks: TaskState,
    pub attachments: AttachmentState,
}

pub fn production_router(
    api: ApiRoutes,
    health: HealthRegistry,
    assets: StaticAssets,
    origin_policy: OriginPolicy,
    limits: HttpLimits,
) -> Router {
    auth_router(api.auth)
        .merge(workspace_router(api.workspaces))
        .merge(task_router(api.tasks))
        .merge(attachment_router(api.attachments))
        .merge(
            Router::new()
                .route("/health/live", get(liveness))
                .route("/health/ready", get(readiness))
                .with_state(health),
        )
        .merge(assets.router())
        .layer(
            HttpPlatformLayer::new(origin_policy)
                .with_contract_id(crate::openapi::CONTRACT_ID)
                .with_limits(limits),
        )
}

async fn liveness() -> Json<serde_json::Value> {
    Json(serde_json::json!({"live": true}))
}

async fn readiness(
    axum::extract::State(health): axum::extract::State<HealthRegistry>,
) -> (StatusCode, Json<orbit_platform::ReadinessReport>) {
    let report = health.readiness().await;
    let status = if report.ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (status, Json(report))
}
