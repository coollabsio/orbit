use axum::Router;
use axum::body::Body;
use axum::extract::Request;
use axum::http::{Method, Response, StatusCode, header};
use axum::routing::get;
use base64::Engine;
use include_dir::{Dir, include_dir};
use orbit_platform::{Problem, RequestId};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

static FRONTEND: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/../web/dist");
pub const FRONTEND_REVISION: &str = match option_env!("ORBIT_BUILD_REVISION") {
    Some(revision) => revision,
    None => "development",
};

#[derive(Clone)]
pub struct StaticAssets {
    csp_script_hash: String,
}

#[derive(Debug, Error)]
pub enum StaticAssetError {
    #[error("embedded frontend build manifest is missing")]
    MissingManifest,
    #[error("embedded frontend build manifest is invalid")]
    InvalidManifest,
    #[error("embedded frontend expects API contract {actual}, but server provides {expected}")]
    ContractMismatch { expected: String, actual: String },
    #[error("embedded frontend revision {actual} does not match server revision {expected}")]
    RevisionMismatch { expected: String, actual: String },
    #[error("embedded frontend index is missing")]
    MissingIndex,
    #[error("embedded frontend bootstrap CSP hash does not match index.html")]
    CspHashMismatch,
}

#[derive(Deserialize)]
struct BuildManifest {
    contract: String,
    revision: String,
    csp_script_hash: String,
}

impl StaticAssets {
    pub fn verified(
        expected_contract: &str,
        expected_revision: &str,
    ) -> Result<Self, StaticAssetError> {
        let manifest = FRONTEND
            .get_file("orbit-build.json")
            .ok_or(StaticAssetError::MissingManifest)?;
        let manifest: BuildManifest = serde_json::from_slice(manifest.contents())
            .map_err(|_| StaticAssetError::InvalidManifest)?;
        if manifest.contract != expected_contract {
            return Err(StaticAssetError::ContractMismatch {
                expected: expected_contract.to_owned(),
                actual: manifest.contract,
            });
        }
        if manifest.revision != expected_revision {
            return Err(StaticAssetError::RevisionMismatch {
                expected: expected_revision.to_owned(),
                actual: manifest.revision,
            });
        }
        let index = FRONTEND
            .get_file("index.html")
            .ok_or(StaticAssetError::MissingIndex)?;
        let actual_hash =
            inline_script_hash(index.contents()).ok_or(StaticAssetError::CspHashMismatch)?;
        if manifest.csp_script_hash != actual_hash {
            return Err(StaticAssetError::CspHashMismatch);
        }
        Ok(Self {
            csp_script_hash: actual_hash,
        })
    }

    pub fn router(&self) -> Router {
        Router::new()
            .route("/", get(serve_index))
            .fallback(serve_fallback)
    }

    pub fn immutable_asset_path(&self) -> Option<String> {
        FRONTEND
            .get_dir("assets")?
            .files()
            .next()
            .map(|file| format!("/{}", file.path().to_string_lossy()))
    }

    #[must_use]
    pub fn csp_script_hash(&self) -> &str {
        &self.csp_script_hash
    }
}

fn inline_script_hash(index: &[u8]) -> Option<String> {
    let index = std::str::from_utf8(index).ok()?;
    let start = index.find("<script>")? + "<script>".len();
    let remainder = &index[start..];
    let end = remainder.find("</script>")?;
    let script = &remainder[..end];
    let digest = Sha256::digest(script.as_bytes());
    Some(format!(
        "sha256-{}",
        base64::engine::general_purpose::STANDARD.encode(digest)
    ))
}

async fn serve_index() -> Response<Body> {
    asset_response("index.html", false).expect("verified frontend contains index.html")
}

async fn serve_fallback(request: Request) -> Response<Body> {
    let path = request.uri().path().to_owned();
    let request_id = request.extensions().get::<RequestId>().cloned();
    if path == "/api" || path.starts_with("/api/") {
        return not_found_problem(&path, request_id.as_ref());
    }
    if !matches!(*request.method(), Method::GET | Method::HEAD) {
        return not_found_problem(&path, request_id.as_ref());
    }

    let asset_path = path.trim_start_matches('/');
    if !asset_path.is_empty()
        && let Some(response) = asset_response(asset_path, asset_path.starts_with("assets/"))
    {
        return response;
    }
    if asset_path
        .rsplit('/')
        .next()
        .is_some_and(|part| part.contains('.'))
    {
        return not_found_problem(&path, request_id.as_ref());
    }
    serve_index().await
}

fn asset_response(path: &str, immutable: bool) -> Option<Response<Body>> {
    let file = FRONTEND.get_file(path)?;
    let content_type = if path.ends_with(".html") {
        "text/html; charset=utf-8".to_owned()
    } else if path.ends_with(".css") {
        "text/css; charset=utf-8".to_owned()
    } else if path.ends_with(".js") {
        "text/javascript; charset=utf-8".to_owned()
    } else {
        mime_guess::from_path(path)
            .first_or_octet_stream()
            .to_string()
    };
    let cache = if matches!(
        path,
        "index.html" | "orbit-build.json" | "manifest.webmanifest"
    ) {
        "no-store"
    } else if immutable {
        "public, max-age=31536000, immutable"
    } else {
        "public, max-age=3600"
    };
    Some(
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, content_type)
            .header(header::CACHE_CONTROL, cache)
            .body(Body::from(file.contents()))
            .expect("embedded asset headers are valid"),
    )
}

fn not_found_problem(path: &str, request_id: Option<&RequestId>) -> Response<Body> {
    let generated_request_id;
    let request_id = match request_id {
        Some(request_id) => request_id,
        None => {
            generated_request_id = RequestId::new();
            &generated_request_id
        }
    };
    let problem = Problem {
        type_uri: "https://docs.orbit.dev/problems/route_not_found".to_owned(),
        title: "Route not found".to_owned(),
        status: StatusCode::NOT_FOUND.as_u16(),
        code: "route_not_found".to_owned(),
        detail: "The requested route does not exist.".to_owned(),
        instance: path.to_owned(),
        request_id: request_id.to_string(),
        errors: None,
        conflict: None,
    };
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header(header::CONTENT_TYPE, "application/problem+json")
        .body(Body::from(
            serde_json::to_vec(&problem).expect("Problem serialization cannot fail"),
        ))
        .expect("static Problem response is valid")
}
