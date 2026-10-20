//! OAuth authorization code flow for Orbit's read-only MCP resource.
//! The application owns consent and persistence; oxide-auth validates S256 PKCE.
use std::borrow::Cow;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::Arc;

use axum::extract::{DefaultBodyLimit, Extension, Form, Path, Query, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use base64::Engine;
use orbit_platform::{
    Backend, ClientIp, Database, Flag, Id, Row, TimestampMillis, generate_opaque_token,
};
use oxide_auth::code_grant::extensions::Pkce;
use oxide_auth::primitives::grant::Value as PkceValue;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{Any, Transaction};
use url::{Host, Url};

use crate::audit::{self, AuditOutcome};
use crate::auth_routes::CookieMode;
use crate::repositories::api_tokens::{ApiTokenPrincipal, ApiTokenScope};
use crate::repositories::identity::IdentityRepository;
use crate::task_routes::authenticate_session;

const SCOPE: &str = "tasks:read";
const ACCESS_SECONDS: i64 = 15 * 60;
const REFRESH_MILLIS: i64 = 30 * 24 * 60 * 60 * 1000;
const REQUEST_MILLIS: i64 = 10 * 60 * 1000;
const DAY_MILLIS: i64 = 24 * 60 * 60 * 1000;
/// Registered clients on the whole instance: the last guard, after the per-IP limit and pruning.
const MAX_CLIENTS: i64 = 10_000;
/// Clients one IP address (an IPv6 /64) may register in a day, on all servers together. The times
/// are kept in `auth_throttle_ips` under this scope.
const REGISTRATIONS_PER_IP: i64 = 20;
const REGISTRATION_SCOPE: &str = "oauth.register";

#[derive(Clone)]
pub struct OAuthState {
    database: Database,
    identity: Arc<IdentityRepository>,
    cookie_mode: CookieMode,
    origin: String,
    resource: String,
}

impl OAuthState {
    pub fn new(identity: Arc<IdentityRepository>, cookie_mode: CookieMode, origin: &str) -> Self {
        let origin = origin.trim_end_matches('/').to_owned();
        Self {
            database: identity.database().clone(),
            identity,
            cookie_mode,
            resource: format!("{origin}/mcp"),
            origin,
        }
    }

    pub fn challenge(&self) -> String {
        format!(
            "Bearer resource_metadata=\"{}/.well-known/oauth-protected-resource/mcp\", scope=\"{SCOPE}\"",
            self.origin
        )
    }

    /// Opaque tokens are audience-bound in SQLite, not taken from an external issuer.
    pub async fn authenticate(&self, token: &str) -> Result<Option<ApiTokenPrincipal>, OAuthError> {
        let now = TimestampMillis::now().as_millis();
        let row = sqlx::query("SELECT g.id, g.user_id, g.workspace_id, g.project_ids FROM oauth_tokens t JOIN oauth_grants g ON g.id = t.grant_id JOIN users u ON u.id = g.user_id JOIN memberships m ON m.user_id = g.user_id AND m.workspace_id = g.workspace_id JOIN workspaces w ON w.id = g.workspace_id WHERE t.access_hash = $1 AND t.access_expires_at > $2 AND g.revoked_at IS NULL AND g.resource = $3 AND g.scope = 'tasks:read' AND u.suspended_at IS NULL AND w.deleted_at IS NULL")
            .bind(hash(token)).bind(now).bind(&self.resource).fetch_optional(self.database.pool()).await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let project_ids: Vec<String> =
            serde_json::from_str(row.get("project_ids")).map_err(|_| OAuthError::server())?;
        let mut approved = Vec::new();
        for id in project_ids {
            let Flag(active) = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM projects WHERE id = $1 AND workspace_id = $2 AND deleted_at IS NULL)")
                .bind(&id).bind(row.get::<String, _>("workspace_id")).fetch_one(self.database.pool()).await?;
            if active {
                approved.push(id.parse().map_err(|_| OAuthError::server())?);
            }
        }
        if approved.is_empty() {
            return Ok(None);
        }
        Ok(Some(ApiTokenPrincipal {
            token_id: row
                .get::<String, _>("id")
                .parse()
                .map_err(|_| OAuthError::server())?,
            workspace_id: row
                .get::<String, _>("workspace_id")
                .parse()
                .map_err(|_| OAuthError::server())?,
            project_ids: approved,
            creator_id: row
                .get::<String, _>("user_id")
                .parse()
                .map_err(|_| OAuthError::server())?,
            service_account_id: None,
            scopes: vec![ApiTokenScope::Read],
        }))
    }
}

pub fn router(state: OAuthState) -> Router {
    let public = Router::new()
        .route(
            "/.well-known/oauth-authorization-server",
            get(server_metadata),
        )
        .route(
            "/.well-known/oauth-protected-resource",
            get(resource_metadata),
        )
        .route(
            "/.well-known/oauth-protected-resource/mcp",
            get(resource_metadata),
        )
        .route("/oauth/register", post(register))
        .route("/oauth/authorize", get(authorize))
        .route("/oauth/token", post(token))
        .route("/oauth/revoke", post(revoke))
        .layer(middleware::from_fn(public_cors));
    public
        .route(
            "/api/v1/oauth/consent/{request_id}",
            get(consent).post(approve),
        )
        .route("/api/v1/oauth/grants", get(grants))
        .route("/api/v1/oauth/grants/{grant_id}", delete(disconnect))
        .layer(DefaultBodyLimit::max(16 * 1024))
        .layer(middleware::from_fn(no_store))
        .with_state(state)
}

async fn no_store(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    response
}

// These endpoints do not use cookies. Browser consent endpoints are deliberately
// outside this layer and retain Orbit's same-origin/session policy.
async fn public_cors(request: Request, next: Next) -> Response {
    let mut response = if request.method() == axum::http::Method::OPTIONS {
        StatusCode::NO_CONTENT.into_response()
    } else {
        next.run(request).await
    };
    let headers = response.headers_mut();
    headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*".parse().unwrap());
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        "GET, POST, OPTIONS".parse().unwrap(),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        "Content-Type, Authorization".parse().unwrap(),
    );
    headers.insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    headers.insert(header::PRAGMA, "no-cache".parse().unwrap());
    response
}

pub struct OAuthError {
    status: StatusCode,
    code: &'static str,
}
impl OAuthError {
    fn invalid(code: &'static str) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code,
        }
    }
    fn throttled() -> Self {
        Self {
            status: StatusCode::TOO_MANY_REQUESTS,
            code: "temporarily_unavailable",
        }
    }
    fn unauthorized() -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code: "login_required",
        }
    }
    fn server() -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "server_error",
        }
    }
}
impl From<sqlx::Error> for OAuthError {
    fn from(error: sqlx::Error) -> Self {
        tracing::error!(%error, "OAuth persistence failed");
        Self::server()
    }
}
impl IntoResponse for OAuthError {
    fn into_response(self) -> Response {
        (
            self.status,
            [(header::CACHE_CONTROL, "no-store")],
            Json(json!({"error": self.code})),
        )
            .into_response()
    }
}

async fn server_metadata(State(state): State<OAuthState>) -> Json<Value> {
    Json(
        json!({ "issuer": state.origin, "authorization_endpoint": format!("{}/oauth/authorize", state.origin),
        "token_endpoint": format!("{}/oauth/token", state.origin), "registration_endpoint": format!("{}/oauth/register", state.origin),
        "revocation_endpoint": format!("{}/oauth/revoke", state.origin), "scopes_supported": [SCOPE],
        "response_types_supported": ["code"], "grant_types_supported": ["authorization_code", "refresh_token"],
        "token_endpoint_auth_methods_supported": ["none"], "code_challenge_methods_supported": ["S256"],
        "authorization_response_iss_parameter_supported": true }),
    )
}
async fn resource_metadata(State(state): State<OAuthState>) -> Json<Value> {
    Json(
        json!({"resource": state.resource, "authorization_servers": [state.origin],
        "scopes_supported": [SCOPE], "bearer_methods_supported": ["header"], "resource_name": "Orbit read-only tasks"}),
    )
}

#[derive(Deserialize)]
struct Registration {
    redirect_uris: Vec<String>,
    client_name: Option<String>,
    token_endpoint_auth_method: Option<String>,
    grant_types: Option<Vec<String>>,
    response_types: Option<Vec<String>>,
}
async fn register(
    State(state): State<OAuthState>,
    client_ip: Option<Extension<ClientIp>>,
    Json(input): Json<Registration>,
) -> Result<Response, OAuthError> {
    if input.redirect_uris.is_empty()
        || input.redirect_uris.len() > 10
        || input.redirect_uris.iter().any(|uri| !valid_redirect(uri))
        || input
            .token_endpoint_auth_method
            .as_deref()
            .is_some_and(|method| method != "none")
        || input.grant_types.as_ref().is_some_and(|types| {
            types.is_empty()
                || types
                    .iter()
                    .any(|t| !matches!(t.as_str(), "authorization_code" | "refresh_token"))
        })
        || input
            .response_types
            .as_ref()
            .is_some_and(|types| types.as_slice() != ["code"])
    {
        return Err(OAuthError::invalid("invalid_client_metadata"));
    }
    let name = input.client_name.unwrap_or_else(|| "MCP client".into());
    if name.trim().is_empty() || name.chars().count() > 100 {
        return Err(OAuthError::invalid("invalid_client_metadata"));
    }
    let id = format!("orbit-client-{}", Id::new_v7());
    let now = TimestampMillis::now().as_millis();
    let ip = registration_ip(client_ip.map(|Extension(ip)| ip.0));
    let mut tx = state.database.immediate_transaction().await?;
    sqlx::query("DELETE FROM auth_throttle_ips WHERE scope = $1 AND failed_at <= $2")
        .bind(REGISTRATION_SCOPE)
        .bind(now - DAY_MILLIS)
        .execute(&mut *tx)
        .await?;
    let from_ip: i64 =
        sqlx::query_scalar("SELECT count(*) FROM auth_throttle_ips WHERE scope = $1 AND ip = $2")
            .bind(REGISTRATION_SCOPE)
            .bind(&ip)
            .fetch_one(&mut *tx)
            .await?;
    if from_ip >= REGISTRATIONS_PER_IP {
        return Err(OAuthError::throttled());
    }
    if client_count(&mut tx).await? >= MAX_CLIENTS {
        // Under pressure, a client that is not in use goes after the time an authorization takes.
        prune(&mut tx, now, REQUEST_MILLIS).await?;
        if client_count(&mut tx).await? >= MAX_CLIENTS {
            tx.commit().await?;
            return Err(OAuthError::invalid("temporarily_unavailable"));
        }
    }
    sqlx::query("INSERT INTO auth_throttle_ips (scope, ip, failed_at) VALUES ($1, $2, $3)")
        .bind(REGISTRATION_SCOPE)
        .bind(&ip)
        .bind(now)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO oauth_clients (id, name, redirect_uris, created_at) VALUES ($1, $2, $3, $4)",
    )
    .bind(&id)
    .bind(&name)
    .bind(json!(input.redirect_uris).to_string())
    .bind(now)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(json!({"client_id": id, "client_name": name, "client_id_issued_at": now / 1000,
        "redirect_uris": input.redirect_uris, "token_endpoint_auth_method": "none",
        "grant_types": ["authorization_code", "refresh_token"], "response_types": ["code"], "scope": SCOPE}))).into_response())
}

/// The address that registrations count against: an IPv6 client usually holds a whole /64.
fn registration_ip(ip: Option<IpAddr>) -> String {
    match ip.unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED)) {
        IpAddr::V6(ip) => Ipv6Addr::from(u128::from(ip) & (u128::MAX << 64)).to_string(),
        ip => ip.to_string(),
    }
}

async fn client_count(tx: &mut Transaction<'_, Any>) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT count(*) FROM oauth_clients")
        .fetch_one(&mut **tx)
        .await
}

/// Deletes the clients that are not in use (with their grants, tokens and requests): registered
/// over a day ago, with no authorization in progress and no grant that was made, revoked or
/// could refresh in the last day. Runs with the daily retention job.
pub async fn prune_clients(database: &Database) -> Result<u64, sqlx::Error> {
    let mut tx = database.immediate_transaction().await?;
    let deleted = prune(&mut tx, TimestampMillis::now().as_millis(), DAY_MILLIS).await?;
    tx.commit().await?;
    Ok(deleted)
}

/// Deletes the clients that have been unused for `idle` milliseconds; see [`prune_clients`].
async fn prune(tx: &mut Transaction<'_, Any>, now: i64, idle: i64) -> Result<u64, sqlx::Error> {
    let deleted = sqlx::query(
        "DELETE FROM oauth_clients WHERE created_at <= $1 \
         AND NOT EXISTS (SELECT 1 FROM oauth_requests r WHERE r.client_id = oauth_clients.id AND r.expires_at > $2) \
         AND NOT EXISTS (SELECT 1 FROM oauth_grants g WHERE g.client_id = oauth_clients.id \
         AND (g.created_at > $1 OR g.revoked_at > $1 OR (g.revoked_at IS NULL \
         AND EXISTS (SELECT 1 FROM oauth_tokens t WHERE t.grant_id = g.id AND t.refresh_expires_at > $1))))",
    )
    .bind(now - idle)
    .bind(now)
    .execute(&mut **tx)
    .await?
    .rows_affected();
    Ok(deleted)
}

fn valid_redirect(raw: &str) -> bool {
    if raw.len() > 2048 {
        return false;
    }
    let Ok(url) = Url::parse(raw) else {
        return false;
    };
    let loopback = match url.host() {
        Some(Host::Ipv4(ip)) => ip.is_loopback(),
        Some(Host::Ipv6(ip)) => ip.is_loopback(),
        Some(Host::Domain("localhost")) => true,
        _ => false,
    };
    url.host().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none()
        && (url.scheme() == "https" || (url.scheme() == "http" && loopback))
        && !url
            .query_pairs()
            .any(|(name, _)| matches!(name.as_ref(), "code" | "state" | "iss" | "error"))
}

#[derive(Deserialize)]
struct AuthorizationRequest {
    response_type: String,
    client_id: String,
    redirect_uri: String,
    code_challenge: String,
    code_challenge_method: String,
    state: Option<String>,
    scope: Option<String>,
    resource: Option<String>,
}
async fn authorize(
    State(state): State<OAuthState>,
    Query(input): Query<AuthorizationRequest>,
) -> Result<Redirect, OAuthError> {
    if input.response_type != "code"
        || input.code_challenge_method != "S256"
        || input.code_challenge.len() != 43
        || base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(&input.code_challenge)
            .map_or(true, |bytes| bytes.len() != 32)
        || input.state.as_ref().is_some_and(|value| value.len() > 2048)
    {
        return Err(OAuthError::invalid("invalid_request"));
    }
    if input.scope.as_deref().is_some_and(|scope| scope != SCOPE) {
        return Err(OAuthError::invalid("invalid_scope"));
    }
    if input
        .resource
        .as_ref()
        .is_some_and(|resource| resource != &state.resource)
    {
        return Err(OAuthError::invalid("invalid_target"));
    }
    let redirects: Option<String> =
        sqlx::query_scalar("SELECT redirect_uris FROM oauth_clients WHERE id = $1")
            .bind(&input.client_id)
            .fetch_optional(state.database.pool())
            .await?;
    let redirects: Vec<String> =
        serde_json::from_str(&redirects.ok_or_else(|| OAuthError::invalid("invalid_client"))?)
            .map_err(|_| OAuthError::server())?;
    if !redirects.contains(&input.redirect_uri) {
        return Err(OAuthError::invalid("invalid_request"));
    }
    let pkce = Pkce::required()
        .challenge(
            Some(Cow::Borrowed("S256")),
            Some(Cow::Borrowed(&input.code_challenge)),
        )
        .map_err(|_| OAuthError::invalid("invalid_request"))?
        .and_then(|value| value.into_private_value().ok().flatten())
        .ok_or_else(OAuthError::server)?;
    let id = generate_opaque_token();
    let now = TimestampMillis::now().as_millis();
    let mut tx = state.database.immediate_transaction().await?;
    sqlx::query("DELETE FROM oauth_requests WHERE expires_at <= $1")
        .bind(now)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO oauth_requests (id, client_id, redirect_uri, state, pkce, resource, expires_at) VALUES ($1, $2, $3, $4, $5, $6, $7)")
        .bind(&id).bind(&input.client_id).bind(&input.redirect_uri).bind(&input.state).bind(pkce)
        .bind(&state.resource).bind(now + REQUEST_MILLIS).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Redirect::to(&format!(
        "{}/oauth/consent?request={id}",
        state.origin
    )))
}

async fn user(state: &OAuthState, headers: &HeaderMap) -> Result<Id, OAuthError> {
    authenticate_session(
        &state.identity,
        state.cookie_mode,
        headers,
        "/api/v1/oauth",
        None,
    )
    .await
    .map(|session| session.user.id)
    .map_err(|_| OAuthError::unauthorized())
}

#[derive(Serialize)]
struct ConsentProject {
    id: String,
    name: String,
    workspace_id: String,
    workspace_name: String,
}
async fn consent(
    State(state): State<OAuthState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, OAuthError> {
    let user = user(&state, &headers).await?;
    let now = TimestampMillis::now().as_millis();
    let nonce = generate_opaque_token();
    let mut tx = state.database.immediate_transaction().await?;
    let updated = sqlx::query("UPDATE oauth_requests SET user_id = $1, csrf_hash = $2 WHERE id = $3 AND expires_at > $4 AND code_hash IS NULL AND (user_id IS NULL OR user_id = $5)")
        .bind(user.to_string()).bind(hash(&nonce)).bind(&id).bind(now).bind(user.to_string()).execute(&mut *tx).await?;
    if updated.rows_affected() != 1 {
        return Err(OAuthError::invalid("invalid_request"));
    }
    let client: String = sqlx::query_scalar("SELECT c.name FROM oauth_clients c JOIN oauth_requests r ON r.client_id = c.id WHERE r.id = $1")
        .bind(&id).fetch_one(&mut *tx).await?;
    let rows = sqlx::query("SELECT p.id, p.name, p.workspace_id, w.name AS workspace_name FROM projects p JOIN workspaces w ON w.id = p.workspace_id JOIN memberships m ON m.workspace_id = w.id WHERE m.user_id = $1 AND p.deleted_at IS NULL AND w.deleted_at IS NULL ORDER BY w.name, p.name, p.id")
        .bind(user.to_string()).fetch_all(&mut *tx).await?;
    let projects: Vec<_> = rows
        .into_iter()
        .map(|row| ConsentProject {
            id: row.get("id"),
            name: row.get("name"),
            workspace_id: row.get("workspace_id"),
            workspace_name: row.get("workspace_name"),
        })
        .collect();
    tx.commit().await?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(json!({"client_name": client, "scope": SCOPE, "csrf_token": nonce, "projects": projects}))).into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Approval {
    csrf_token: String,
    approved: bool,
    workspace_id: Option<Id>,
    #[serde(default)]
    project_ids: Vec<Id>,
}
async fn approve(
    State(state): State<OAuthState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(input): Json<Approval>,
) -> Result<Json<Value>, OAuthError> {
    let user = user(&state, &headers).await?;
    let now = TimestampMillis::now().as_millis();
    let mut tx = state.database.immediate_transaction().await?;
    let row = sqlx::query("SELECT client_id, redirect_uri, state, resource FROM oauth_requests WHERE id = $1 AND user_id = $2 AND csrf_hash = $3 AND expires_at > $4 AND code_hash IS NULL")
        .bind(&id).bind(user.to_string()).bind(hash(&input.csrf_token)).bind(now).fetch_optional(&mut *tx).await?
        .ok_or_else(|| OAuthError::invalid("invalid_request"))?;
    let mut callback = Url::parse(row.get("redirect_uri")).map_err(|_| OAuthError::server())?;
    if input.approved {
        let workspace = input
            .workspace_id
            .ok_or_else(|| OAuthError::invalid("invalid_request"))?;
        if input.project_ids.is_empty() || input.project_ids.len() > 100 {
            return Err(OAuthError::invalid("invalid_request"));
        }
        for project in &input.project_ids {
            let Flag(allowed) = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM projects p JOIN workspaces w ON w.id = p.workspace_id JOIN memberships m ON m.workspace_id = w.id WHERE p.id = $1 AND p.workspace_id = $2 AND m.user_id = $3 AND p.deleted_at IS NULL AND w.deleted_at IS NULL)")
                .bind(project.to_string()).bind(workspace.to_string()).bind(user.to_string()).fetch_one(&mut *tx).await?;
            if !allowed {
                return Err(OAuthError::invalid("invalid_request"));
            }
        }
        let grant = Id::new_v7();
        let code = generate_opaque_token();
        sqlx::query("INSERT INTO oauth_grants (id, client_id, user_id, workspace_id, project_ids, resource, scope, created_at) VALUES ($1, $2, $3, $4, $5, $6, 'tasks:read', $7)")
            .bind(grant.to_string()).bind(row.get::<String, _>("client_id")).bind(user.to_string()).bind(workspace.to_string())
            .bind(json!(input.project_ids).to_string()).bind(row.get::<String, _>("resource")).bind(now).execute(&mut *tx).await?;
        sqlx::query("UPDATE oauth_requests SET code_hash = $1, grant_id = $2, csrf_hash = NULL, expires_at = $3 WHERE id = $4")
            .bind(hash(&code)).bind(grant.to_string()).bind(now + 60_000).bind(&id).execute(&mut *tx).await?;
        audit::record(
            &mut tx,
            workspace,
            Some(user),
            "oauth.grant.created",
            AuditOutcome::Success,
            "oauth_grant",
            Some(grant),
            "oauth-consent",
            json!({"scope": SCOPE}),
            TimestampMillis::from_millis(now),
        )
        .await?;
        callback.query_pairs_mut().append_pair("code", &code);
    } else {
        sqlx::query("DELETE FROM oauth_requests WHERE id = $1")
            .bind(&id)
            .execute(&mut *tx)
            .await?;
        callback
            .query_pairs_mut()
            .append_pair("error", "access_denied");
    }
    if let Some(client_state) = row.get::<Option<String>, _>("state") {
        callback
            .query_pairs_mut()
            .append_pair("state", &client_state);
    }
    callback.query_pairs_mut().append_pair("iss", &state.origin);
    tx.commit().await?;
    Ok(Json(json!({"redirect_uri": callback.as_str()})))
}

#[derive(Deserialize)]
struct TokenRequest {
    grant_type: String,
    client_id: String,
    code: Option<String>,
    redirect_uri: Option<String>,
    code_verifier: Option<String>,
    refresh_token: Option<String>,
    resource: Option<String>,
    scope: Option<String>,
}
async fn token(
    State(state): State<OAuthState>,
    Form(input): Form<TokenRequest>,
) -> Result<Json<Value>, OAuthError> {
    if input
        .resource
        .as_ref()
        .is_some_and(|resource| resource != &state.resource)
    {
        return Err(OAuthError::invalid("invalid_target"));
    }
    if input.scope.as_deref().is_some_and(|scope| scope != SCOPE) {
        return Err(OAuthError::invalid("invalid_scope"));
    }
    let now = TimestampMillis::now().as_millis();
    let mut tx = state.database.immediate_transaction().await?;
    let grant = match input.grant_type.as_str() {
        "authorization_code" => {
            let code = input
                .code
                .ok_or_else(|| OAuthError::invalid("invalid_request"))?;
            let verifier = input
                .code_verifier
                .ok_or_else(|| OAuthError::invalid("invalid_request"))?;
            if !(43..=128).contains(&verifier.len())
                || !verifier
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-._~".contains(&b))
            {
                return Err(OAuthError::invalid("invalid_grant"));
            }
            let row = sqlx::query("SELECT grant_id, pkce, used_at FROM oauth_requests WHERE code_hash = $1 AND client_id = $2 AND redirect_uri = $3 AND expires_at > $4 AND resource = $5")
                .bind(hash(&code)).bind(&input.client_id).bind(input.redirect_uri.ok_or_else(|| OAuthError::invalid("invalid_request"))?)
                .bind(now).bind(&state.resource).fetch_optional(&mut *tx).await?.ok_or_else(|| OAuthError::invalid("invalid_grant"))?;
            Pkce::required()
                .verify(
                    Some(PkceValue::private(Some(row.get("pkce")))),
                    Some(Cow::Borrowed(&verifier)),
                )
                .map_err(|_| OAuthError::invalid("invalid_grant"))?;
            let grant: String = row.get("grant_id");
            if row.get::<Option<i64>, _>("used_at").is_some() {
                revoke_grant(&mut tx, &grant, now).await?;
                tx.commit().await?;
                return Err(OAuthError::invalid("invalid_grant"));
            }
            sqlx::query("UPDATE oauth_requests SET used_at = $1 WHERE code_hash = $2")
                .bind(now)
                .bind(hash(&code))
                .execute(&mut *tx)
                .await?;
            grant
        }
        "refresh_token" => {
            let refresh = input
                .refresh_token
                .ok_or_else(|| OAuthError::invalid("invalid_request"))?;
            let row = sqlx::query("SELECT t.grant_id, t.refresh_used_at FROM oauth_tokens t JOIN oauth_grants g ON g.id = t.grant_id WHERE t.refresh_hash = $1 AND t.refresh_expires_at > $2 AND g.client_id = $3 AND g.resource = $4 AND g.revoked_at IS NULL")
                .bind(hash(&refresh)).bind(now).bind(&input.client_id).bind(&state.resource).fetch_optional(&mut *tx).await?.ok_or_else(|| OAuthError::invalid("invalid_grant"))?;
            let grant: String = row.get("grant_id");
            if row.get::<Option<i64>, _>("refresh_used_at").is_some() {
                revoke_grant(&mut tx, &grant, now).await?;
                tx.commit().await?;
                return Err(OAuthError::invalid("invalid_grant"));
            }
            sqlx::query("UPDATE oauth_tokens SET refresh_used_at = $1 WHERE refresh_hash = $2")
                .bind(now)
                .bind(hash(&refresh))
                .execute(&mut *tx)
                .await?;
            grant
        }
        _ => return Err(OAuthError::invalid("unsupported_grant_type")),
    };
    let valid_query = match state.database.backend() {
        Backend::Sqlite => {
            "SELECT EXISTS(SELECT 1 FROM oauth_grants g JOIN users u ON u.id = g.user_id JOIN memberships m ON m.user_id = g.user_id AND m.workspace_id = g.workspace_id JOIN workspaces w ON w.id = g.workspace_id WHERE g.id = $1 AND g.client_id = $2 AND g.revoked_at IS NULL AND u.suspended_at IS NULL AND w.deleted_at IS NULL AND EXISTS(SELECT 1 FROM projects p, json_each(g.project_ids) j WHERE p.id = j.value AND p.workspace_id = g.workspace_id AND p.deleted_at IS NULL))"
        }
        Backend::Postgres => {
            "SELECT EXISTS(SELECT 1 FROM oauth_grants g JOIN users u ON u.id = g.user_id JOIN memberships m ON m.user_id = g.user_id AND m.workspace_id = g.workspace_id JOIN workspaces w ON w.id = g.workspace_id WHERE g.id = $1 AND g.client_id = $2 AND g.revoked_at IS NULL AND u.suspended_at IS NULL AND w.deleted_at IS NULL AND EXISTS(SELECT 1 FROM projects p, jsonb_array_elements_text(g.project_ids::jsonb) AS j(value) WHERE p.id = j.value AND p.workspace_id = g.workspace_id AND p.deleted_at IS NULL))"
        }
    };
    let Flag(valid) = sqlx::query_scalar(valid_query)
        .bind(&grant)
        .bind(&input.client_id)
        .fetch_one(&mut *tx)
        .await?;
    if !valid {
        return Err(OAuthError::invalid("invalid_grant"));
    }
    let access = format!("orbit-mcp-a-{}", generate_opaque_token());
    let refresh = format!("orbit-mcp-r-{}", generate_opaque_token());
    sqlx::query("DELETE FROM oauth_tokens WHERE refresh_expires_at <= $1")
        .bind(now)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO oauth_tokens (access_hash, refresh_hash, grant_id, access_expires_at, refresh_expires_at) VALUES ($1, $2, $3, $4, $5)")
        .bind(hash(&access)).bind(hash(&refresh)).bind(&grant).bind(now + ACCESS_SECONDS * 1000).bind(now + REFRESH_MILLIS).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(
        json!({"access_token": access, "token_type": "Bearer", "expires_in": ACCESS_SECONDS, "refresh_token": refresh, "scope": SCOPE}),
    ))
}

#[derive(Deserialize)]
struct Revocation {
    token: String,
    client_id: String,
}
async fn revoke(
    State(state): State<OAuthState>,
    Form(input): Form<Revocation>,
) -> Result<StatusCode, OAuthError> {
    sqlx::query("UPDATE oauth_grants SET revoked_at = $1 WHERE client_id = $2 AND id IN (SELECT grant_id FROM oauth_tokens WHERE access_hash = $3 OR refresh_hash = $4)")
        .bind(TimestampMillis::now().as_millis()).bind(input.client_id).bind(hash(&input.token)).bind(hash(&input.token)).execute(state.database.pool()).await?;
    Ok(StatusCode::OK)
}

async fn revoke_grant(
    tx: &mut Transaction<'_, Any>,
    grant: &str,
    now: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE oauth_grants SET revoked_at = $1 WHERE id = $2")
        .bind(now)
        .bind(grant)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn grants(
    State(state): State<OAuthState>,
    headers: HeaderMap,
) -> Result<Json<Value>, OAuthError> {
    let user = user(&state, &headers).await?;
    let query = match state.database.backend() {
        Backend::Sqlite => {
            "SELECT g.id, c.name AS client_name, w.name AS workspace_name, g.created_at, (SELECT json_group_array(p.name) FROM projects p JOIN json_each(g.project_ids) j ON j.value = p.id WHERE p.workspace_id = g.workspace_id AND p.deleted_at IS NULL) AS project_names FROM oauth_grants g JOIN oauth_clients c ON c.id = g.client_id JOIN workspaces w ON w.id = g.workspace_id JOIN memberships m ON m.workspace_id = w.id AND m.user_id = g.user_id WHERE g.user_id = $1 AND g.revoked_at IS NULL AND w.deleted_at IS NULL ORDER BY g.created_at DESC"
        }
        Backend::Postgres => {
            "SELECT g.id, c.name AS client_name, w.name AS workspace_name, g.created_at, (SELECT COALESCE(json_agg(p.name)::text, '[]') FROM projects p JOIN jsonb_array_elements_text(g.project_ids::jsonb) AS j(value) ON j.value = p.id WHERE p.workspace_id = g.workspace_id AND p.deleted_at IS NULL) AS project_names FROM oauth_grants g JOIN oauth_clients c ON c.id = g.client_id JOIN workspaces w ON w.id = g.workspace_id JOIN memberships m ON m.workspace_id = w.id AND m.user_id = g.user_id WHERE g.user_id = $1 AND g.revoked_at IS NULL AND w.deleted_at IS NULL ORDER BY g.created_at DESC"
        }
    };
    let rows = sqlx::query(query)
        .bind(user.to_string())
        .fetch_all(state.database.pool())
        .await?;
    let items: Vec<_> = rows.into_iter().map(|row| json!({"id": row.get::<String, _>("id"), "client_name": row.get::<String, _>("client_name"), "workspace_name": row.get::<String, _>("workspace_name"), "scope": SCOPE, "project_names": serde_json::from_str::<Value>(row.get("project_names")).unwrap_or_else(|_| json!([])), "created_at": row.get::<i64, _>("created_at")})).collect();
    Ok(Json(json!({"items": items})))
}

async fn disconnect(
    State(state): State<OAuthState>,
    Path(grant): Path<Id>,
    headers: HeaderMap,
) -> Result<StatusCode, OAuthError> {
    let user = user(&state, &headers).await?;
    let mut tx = state.database.immediate_transaction().await?;
    let workspace: Option<String> =
        sqlx::query_scalar("SELECT workspace_id FROM oauth_grants WHERE id = $1 AND user_id = $2")
            .bind(grant.to_string())
            .bind(user.to_string())
            .fetch_optional(&mut *tx)
            .await?;
    let workspace: Id = workspace
        .ok_or_else(|| OAuthError::invalid("invalid_request"))?
        .parse()
        .map_err(|_| OAuthError::server())?;
    revoke_grant(
        &mut tx,
        &grant.to_string(),
        TimestampMillis::now().as_millis(),
    )
    .await?;
    audit::record(
        &mut tx,
        workspace,
        Some(user),
        "oauth.grant.revoked",
        AuditOutcome::Success,
        "oauth_grant",
        Some(grant),
        "oauth-disconnect",
        json!({}),
        TimestampMillis::now(),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

fn hash(secret: &str) -> Vec<u8> {
    Sha256::digest(secret.as_bytes()).to_vec()
}
