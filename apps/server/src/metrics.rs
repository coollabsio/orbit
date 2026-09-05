use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use axum::Router;
use axum::extract::{Request, State};
use axum::http::{Response, header};
use axum::middleware::{self, Next};
use axum::routing::get;

#[derive(Clone, Debug, Default)]
pub struct Metrics {
    inner: Arc<MetricsInner>,
}

#[derive(Debug, Default)]
struct MetricsInner {
    requests: AtomicU64,
    in_flight: AtomicUsize,
}

impl Metrics {
    pub fn instrument(&self, router: Router) -> Router {
        router.layer(middleware::from_fn_with_state(self.clone(), track_request))
    }

    pub fn router(&self) -> Router {
        Router::new()
            .route("/metrics", get(render_metrics))
            .with_state(self.clone())
    }
}

async fn track_request(
    State(metrics): State<Metrics>,
    request: Request,
    next: Next,
) -> axum::response::Response {
    metrics.inner.requests.fetch_add(1, Ordering::Relaxed);
    metrics.inner.in_flight.fetch_add(1, Ordering::Relaxed);
    let response = next.run(request).await;
    metrics.inner.in_flight.fetch_sub(1, Ordering::Relaxed);
    response
}

async fn render_metrics(State(metrics): State<Metrics>) -> Response<String> {
    let body = format!(
        "# HELP orbit_http_requests_total Public HTTP requests handled.\n\
         # TYPE orbit_http_requests_total counter\n\
         orbit_http_requests_total {}\n\
         # HELP orbit_http_requests_in_flight Public HTTP requests currently running.\n\
         # TYPE orbit_http_requests_in_flight gauge\n\
         orbit_http_requests_in_flight {}\n",
        metrics.inner.requests.load(Ordering::Relaxed),
        metrics.inner.in_flight.load(Ordering::Relaxed),
    );
    Response::builder()
        .header(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )
        .body(body)
        .expect("metrics headers are valid")
}
