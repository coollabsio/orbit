use axum::body::Body;
use axum::extract::Request;
use axum::http::{StatusCode, header};
use axum::response::Response;
use axum::routing::get;
use axum::{Json, Router};
use orbit_platform::{
    HealthRegistry, HttpLimits, HttpPlatformLayer, OriginPolicy, Problem, RateLimitConfig,
    RequestId,
};

use crate::attachment_routes::{AttachmentState, attachment_router};
use crate::auth_routes::{AuthState, auth_router};
use crate::export_routes::{ExportState, export_router};
use crate::import_routes::{ImportState, import_router};
use crate::integration_routes::{IntegrationState, integration_router};
use crate::page_comment_routes::{PageCommentState, page_comment_router};
use crate::page_file_routes::{PageFileState, page_file_router};
use crate::page_routes::{PageState, page_router};
use crate::static_assets::StaticAssets;
use crate::task_routes::{TaskState, task_router};
use crate::teamspace_routes::{TeamspaceState, teamspace_router};
use crate::workspace_routes::{WorkspaceState, workspace_router};

pub struct ApiRoutes {
    pub auth: AuthState,
    pub workspaces: WorkspaceState,
    pub tasks: TaskState,
    pub pages: PageState,
    pub page_files: PageFileState,
    pub page_comments: PageCommentState,
    pub exports: ExportState,
    pub teamspaces: TeamspaceState,
    pub attachments: AttachmentState,
    pub integrations: IntegrationState,
    pub imports: ImportState,
}

pub fn production_router(
    api: ApiRoutes,
    health: HealthRegistry,
    assets: StaticAssets,
    origin_policy: OriginPolicy,
    limits: HttpLimits,
    rate_limits: RateLimitConfig,
    require_secure_transport: bool,
) -> Router {
    let csp_script_hash = assets.csp_script_hash().to_owned();
    let realtime = crate::realtime::router(api.tasks.clone());
    auth_router(api.auth)
        .merge(realtime)
        .merge(workspace_router(api.workspaces))
        .merge(task_router(api.tasks))
        .merge(page_router(api.pages))
        .merge(page_file_router(api.page_files))
        .merge(page_comment_router(api.page_comments))
        .merge(export_router(api.exports))
        .merge(teamspace_router(api.teamspaces))
        .merge(attachment_router(api.attachments))
        .merge(integration_router(api.integrations))
        .merge(import_router(api.imports))
        .merge(
            Router::new()
                .route("/health/live", get(liveness))
                .route("/health/ready", get(readiness))
                .with_state(health),
        )
        .merge(assets.router())
        .method_not_allowed_fallback(method_not_allowed)
        .layer({
            let layer = HttpPlatformLayer::new(origin_policy)
                .with_contract_id(crate::openapi::CONTRACT_ID)
                .with_limits(limits)
                .with_rate_limits(rate_limits)
                .with_csp_script_hash(csp_script_hash);
            if require_secure_transport {
                layer.require_secure_transport()
            } else {
                layer
            }
        })
}

async fn method_not_allowed(request: Request) -> Response<Body> {
    let request_id = request
        .extensions()
        .get::<RequestId>()
        .cloned()
        .unwrap_or_else(RequestId::new);
    let problem = Problem {
        type_uri: "https://docs.orbit.dev/problems/method_not_allowed".to_owned(),
        title: "Method not allowed".to_owned(),
        status: StatusCode::METHOD_NOT_ALLOWED.as_u16(),
        code: "method_not_allowed".to_owned(),
        detail: "The requested method is not supported by this route.".to_owned(),
        instance: request.uri().path().to_owned(),
        request_id: request_id.to_string(),
        errors: None,
        conflict: None,
    };
    Response::builder()
        .status(StatusCode::METHOD_NOT_ALLOWED)
        .header(header::CONTENT_TYPE, "application/problem+json")
        .body(Body::from(
            serde_json::to_vec(&problem).expect("Problem serialization cannot fail"),
        ))
        .expect("method Problem response is valid")
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
