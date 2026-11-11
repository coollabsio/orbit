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

use crate::asana_import_routes::{AsanaImportState, asana_import_router};
use crate::attachment_routes::{AttachmentState, attachment_router};
use crate::auth_routes::{AuthState, auth_router};
use crate::backup_item_routes::{BackupItemState, backup_item_router};
use crate::chat_routes::{ChatState, chat_router};
use crate::database_routes::{DatabaseState, database_router};
use crate::export_routes::{ExportState, export_router};
use crate::import_people_routes::{ImportPeopleState, import_people_router};
use crate::import_routes::{ImportState, import_router};
use crate::integration_routes::{IntegrationState, integration_router};
use crate::linear_import_routes::{LinearImportState, linear_import_router};
use crate::obsidian_import_routes::{ObsidianImportState, obsidian_import_router};
use crate::orbit_import_routes::{OrbitImportState, orbit_import_router};
use crate::page_comment_routes::{PageCommentState, page_comment_router};
use crate::page_file_routes::{PageFileState, page_file_router};
use crate::page_routes::{PageState, page_router};
use crate::static_assets::StaticAssets;
use crate::task_routes::{TaskState, task_router};
use crate::teamspace_routes::{TeamspaceState, teamspace_router};
use crate::trello_import_routes::{TrelloImportState, trello_import_router};
use crate::view_routes::view_router;
use crate::workspace_export_routes::{WorkspaceExportState, workspace_export_router};
use crate::workspace_routes::{WorkspaceState, workspace_router};

pub struct ApiRoutes {
    /// Orbit Cloud billing: its routes and the read-only guard. `None` on a self-hosted server.
    pub billing: Option<crate::billing::Billing>,
    pub mcp: Router,
    /// `/api/public/v1`, for API tokens.
    pub public_api: Router,
    pub oauth: Router,
    pub auth: AuthState,
    pub workspaces: WorkspaceState,
    pub backup_items: BackupItemState,
    pub tasks: TaskState,
    pub pages: PageState,
    pub page_files: PageFileState,
    pub page_comments: PageCommentState,
    pub exports: ExportState,
    pub workspace_exports: WorkspaceExportState,
    pub teamspaces: TeamspaceState,
    pub attachments: AttachmentState,
    pub integrations: IntegrationState,
    pub imports: ImportState,
    pub asana_imports: AsanaImportState,
    pub linear_imports: LinearImportState,
    pub trello_imports: TrelloImportState,
    pub obsidian_imports: ObsidianImportState,
    pub import_people: ImportPeopleState,
    pub orbit_imports: OrbitImportState,
    pub chat: ChatState,
    pub webhooks: crate::webhooks::routes::WebhookState,
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
    let session_cookie = api.auth.cookie_mode().session_cookie_name();
    let database = api.auth.database().clone();
    let realtime = crate::realtime::router(api.tasks.clone());
    let views = view_router(api.tasks.clone());
    let identity = std::sync::Arc::clone(&api.tasks.identity);
    let cookie_mode = api.tasks.cookie_mode;
    let router = auth_router(api.auth)
        .merge(api.mcp)
        .merge(api.public_api)
        .merge(api.oauth)
        .merge(realtime)
        .merge(workspace_router(api.workspaces))
        .merge(backup_item_router(api.backup_items))
        .merge(task_router(api.tasks))
        .merge(views)
        .merge(page_router(api.pages))
        .merge(database_router(DatabaseState::new(
            std::sync::Arc::clone(&identity),
            cookie_mode,
        )))
        .merge(page_file_router(api.page_files))
        .merge(page_comment_router(api.page_comments))
        .merge(export_router(api.exports))
        .merge(workspace_export_router(api.workspace_exports))
        .merge(teamspace_router(api.teamspaces))
        .merge(attachment_router(api.attachments))
        .merge(integration_router(api.integrations))
        .merge(import_router(api.imports))
        .merge(asana_import_router(api.asana_imports))
        .merge(linear_import_router(api.linear_imports))
        .merge(trello_import_router(api.trello_imports))
        .merge(obsidian_import_router(api.obsidian_imports))
        .merge(import_people_router(api.import_people))
        .merge(orbit_import_router(api.orbit_imports))
        .merge(crate::live::socket::live_router(api.chat.clone()))
        .merge(chat_router(api.chat))
        .merge(crate::webhooks::routes::webhook_router(api.webhooks))
        .merge(
            Router::new()
                .route("/health/live", get(liveness))
                .route("/health/ready", get(readiness))
                .with_state(health),
        )
        .merge(assets.router())
        .method_not_allowed_fallback(method_not_allowed);
    let router = match api.billing {
        Some(billing) => crate::billing::guard::layer(
            router.merge(crate::billing::router(
                billing.clone(),
                std::sync::Arc::clone(&identity),
                cookie_mode,
            )),
            billing,
            identity,
            cookie_mode,
        ),
        None => router,
    };
    router.layer({
        let layer = HttpPlatformLayer::new(origin_policy)
            .with_contract_id(crate::openapi::CONTRACT_ID)
            .with_limits(limits)
            .with_rate_limits(rate_limits)
            .with_session_cookie(session_cookie)
            .with_shared_rate_limits(database)
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
