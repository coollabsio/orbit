use std::future::Ready;
use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::str::FromStr;
use std::task::{Context, Poll, Waker};

use axum::body::{Body, to_bytes};
use axum::extract::ConnectInfo;
use axum::http::header::{CONTENT_LENGTH, CONTENT_SECURITY_POLICY, CONTENT_TYPE};
use axum::http::{HeaderMap, HeaderValue, Request, StatusCode};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use ipnet::IpNet;
use orbit_platform::{
    ClientIp, HealthCheck, HealthRegistry, HttpLimits, HttpPlatformLayer, Id, OriginPolicy,
    RequestId, RequestTransport,
};
use serde_json::Value;
use tower::{Layer, Service, ServiceExt, service_fn};

#[derive(Clone)]
struct PendingService;

impl Service<Request<Body>> for PendingService {
    type Response = axum::response::Response;
    type Error = io::Error;
    type Future = Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _context: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Pending
    }

    fn call(&mut self, _request: Request<Body>) -> Self::Future {
        panic!("a pending service must not be called")
    }
}

#[derive(Clone)]
struct ReadinessErrorService;

impl Service<Request<Body>> for ReadinessErrorService {
    type Response = axum::response::Response;
    type Error = io::Error;
    type Future = Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _context: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Err(io::Error::other("database password=hunter2")))
    }

    fn call(&mut self, _request: Request<Body>) -> Self::Future {
        panic!("a service with failed readiness must not be called")
    }
}

fn test_app(policy: OriginPolicy) -> Router {
    Router::new()
        .route(
            "/probe",
            get(
                |Extension(request_id): Extension<RequestId>,
                 Extension(ip): Extension<ClientIp>| async move {
                    format!("{request_id}|{}", ip.0)
                },
            )
            .post(|| async { StatusCode::NO_CONTENT }),
        )
        .route(
            "/failure",
            get(|| async {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "database password=hunter2",
                )
            }),
        )
        .route(
            "/unavailable",
            get(|| async { StatusCode::SERVICE_UNAVAILABLE }),
        )
        .route(
            "/forwarding-view",
            get(
                |headers: HeaderMap,
                 Extension(request_id): Extension<RequestId>,
                 Extension(ip): Extension<ClientIp>,
                 Extension(transport): Extension<RequestTransport>| async move {
                    Json(serde_json::json!({
                        "request_id": request_id.as_str(),
                        "client_ip": ip.0.to_string(),
                        "secure": transport.is_secure(),
                        "has_request_id_header": headers.contains_key("x-request-id"),
                        "has_forwarded_for_header": headers.contains_key("x-forwarded-for"),
                        "has_forwarded_proto_header": headers.contains_key("x-forwarded-proto"),
                    }))
                },
            ),
        )
        .layer(HttpPlatformLayer::new(policy))
}

fn request(method: &str, uri: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .body(Body::empty())
        .unwrap()
}

async fn body_json(response: axum::response::Response) -> Value {
    let bytes = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn unsafe_requests_reject_foreign_and_missing_origins_as_problem_details() {
    let app = test_app(OriginPolicy::new("https://orbit.test"));

    for origin in [Some("https://evil.test"), None] {
        let mut request = request("POST", "/probe");
        if let Some(origin) = origin {
            request
                .headers_mut()
                .insert("origin", HeaderValue::from_static(origin));
        } else {
            request
                .headers_mut()
                .insert("cookie", HeaderValue::from_static("session=browser"));
        }

        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(response.headers()[CONTENT_TYPE], "application/problem+json");
        let problem = body_json(response).await;
        assert_eq!(problem["code"], "origin_forbidden");
        assert!(
            problem["request_id"]
                .as_str()
                .is_some_and(|id| !id.is_empty())
        );
    }
}

#[tokio::test]
async fn non_browser_unsafe_request_does_not_require_an_origin() {
    let app = test_app(OriginPolicy::new("https://orbit.test"));

    let response = app.oneshot(request("POST", "/probe")).await.unwrap();

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn same_origin_unsafe_request_reaches_the_handler() {
    let app = test_app(OriginPolicy::new("https://orbit.test"));
    let mut request = request("POST", "/probe");
    request
        .headers_mut()
        .insert("origin", HeaderValue::from_static("https://orbit.test"));

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn unsafe_request_rejects_ambiguous_multiple_origin_headers() {
    let app = test_app(OriginPolicy::new("https://orbit.test"));
    let mut request = request("POST", "/probe");
    request
        .headers_mut()
        .append("origin", HeaderValue::from_static("https://orbit.test"));
    request
        .headers_mut()
        .append("origin", HeaderValue::from_static("https://evil.test"));

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn untrusted_forwarded_headers_are_ignored_and_a_uuid_v7_request_id_is_generated() {
    let policy =
        OriginPolicy::new("https://orbit.test").trust_proxy(IpNet::from_str("10.0.0.0/8").unwrap());
    let app = test_app(policy);
    let mut request = request("GET", "/forwarding-view");
    request
        .extensions_mut()
        .insert(ConnectInfo(SocketAddr::from((
            Ipv4Addr::new(192, 0, 2, 10),
            4123,
        ))));
    request
        .headers_mut()
        .insert("x-request-id", HeaderValue::from_static("upstream-id"));
    request
        .headers_mut()
        .insert("x-forwarded-for", HeaderValue::from_static("203.0.113.9"));
    request
        .headers_mut()
        .insert("x-forwarded-proto", HeaderValue::from_static("https"));

    let response = app.oneshot(request).await.unwrap();
    let response_id = response.headers()["x-request-id"]
        .to_str()
        .unwrap()
        .to_owned();
    assert_ne!(response_id, "upstream-id");
    assert!(Id::from_str(&response_id).is_ok());
    assert!(
        response
            .headers()
            .get("strict-transport-security")
            .is_none()
    );
    let body = body_json(response).await;
    assert_eq!(body["request_id"], response_id);
    assert_eq!(body["client_ip"], "192.0.2.10");
    assert_eq!(body["secure"], false);
    assert_eq!(body["has_request_id_header"], false);
    assert_eq!(body["has_forwarded_for_header"], false);
    assert_eq!(body["has_forwarded_proto_header"], false);
}

#[tokio::test]
async fn trusted_proxy_headers_are_validated_and_used() {
    let policy =
        OriginPolicy::new("https://orbit.test").trust_proxy(IpNet::from_str("10.0.0.0/8").unwrap());
    let app = test_app(policy);
    let mut request = request("GET", "/forwarding-view");
    request
        .extensions_mut()
        .insert(ConnectInfo(SocketAddr::from((
            Ipv4Addr::new(10, 1, 2, 3),
            4123,
        ))));
    request
        .headers_mut()
        .insert("x-request-id", HeaderValue::from_static("edge-request-123"));
    request
        .headers_mut()
        .insert("x-forwarded-for", HeaderValue::from_static("203.0.113.9"));
    request
        .headers_mut()
        .insert("x-forwarded-proto", HeaderValue::from_static("https"));

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.headers()["x-request-id"], "edge-request-123");
    assert!(response.headers().contains_key("strict-transport-security"));
    let body = body_json(response).await;
    assert_eq!(body["request_id"], "edge-request-123");
    assert_eq!(body["client_ip"], "203.0.113.9");
    assert_eq!(body["secure"], true);
    assert_eq!(body["has_request_id_header"], false);
    assert_eq!(body["has_forwarded_for_header"], false);
    assert_eq!(body["has_forwarded_proto_header"], false);
}

#[tokio::test]
async fn trusted_proxy_rejects_ambiguous_forwarded_headers() {
    let policy =
        OriginPolicy::new("https://orbit.test").trust_proxy(IpNet::from_str("10.0.0.0/8").unwrap());
    let app = test_app(policy);
    let mut request = request("GET", "/probe");
    request
        .extensions_mut()
        .insert(ConnectInfo(SocketAddr::from((
            Ipv4Addr::new(10, 1, 2, 3),
            4123,
        ))));
    request
        .headers_mut()
        .append("x-forwarded-proto", HeaderValue::from_static("https"));
    request
        .headers_mut()
        .append("x-forwarded-proto", HeaderValue::from_static("http"));

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(body_json(response).await["code"], "invalid_proxy_headers");
}

#[tokio::test]
async fn forwarded_chain_cannot_spoof_an_address_before_the_nearest_untrusted_hop() {
    let policy =
        OriginPolicy::new("https://orbit.test").trust_proxy(IpNet::from_str("10.0.0.0/8").unwrap());
    let app = test_app(policy);
    let mut request = request("GET", "/probe");
    request
        .extensions_mut()
        .insert(ConnectInfo(SocketAddr::from((
            Ipv4Addr::new(10, 1, 2, 3),
            4123,
        ))));
    request.headers_mut().insert(
        "x-forwarded-for",
        HeaderValue::from_static("192.0.2.66, 203.0.113.9, 10.2.3.4"),
    );

    let response = app.oneshot(request).await.unwrap();
    let body = to_bytes(response.into_body(), 1024).await.unwrap();

    assert!(
        String::from_utf8(body.to_vec())
            .unwrap()
            .ends_with("|203.0.113.9")
    );
}

#[tokio::test]
async fn security_headers_are_added_to_every_response() {
    let app = test_app(OriginPolicy::new("https://orbit.test"));

    let response = app.oneshot(request("GET", "/probe")).await.unwrap();

    let csp = response.headers()[CONTENT_SECURITY_POLICY]
        .to_str()
        .unwrap();
    assert!(csp.contains("default-src 'self'"));
    assert!(csp.contains("frame-ancestors 'none'"));
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    assert_eq!(
        response.headers()["referrer-policy"],
        "strict-origin-when-cross-origin"
    );
    assert!(response.headers().contains_key("permissions-policy"));
}

#[tokio::test]
async fn internal_errors_are_replaced_with_safe_problem_details_and_the_request_id() {
    let app = test_app(OriginPolicy::new("https://orbit.test"));

    let response = app.oneshot(request("GET", "/failure")).await.unwrap();

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(response.headers()[CONTENT_TYPE], "application/problem+json");
    let response_id = response.headers()["x-request-id"]
        .to_str()
        .unwrap()
        .to_owned();
    let problem = body_json(response).await;
    assert_eq!(problem["code"], "internal_error");
    assert_eq!(problem["request_id"], response_id);
    assert!(!problem.to_string().contains("hunter2"));
}

#[tokio::test]
async fn fallible_inner_service_error_becomes_a_safe_correlated_problem_response() {
    let service = HttpPlatformLayer::new(OriginPolicy::new("https://orbit.test")).layer(
        service_fn(|_request: Request<Body>| async {
            Err::<axum::response::Response, _>(io::Error::other("database password=hunter2"))
        }),
    );

    let response = service
        .oneshot(request("GET", "/fallible"))
        .await
        .expect("the platform boundary must absorb inner service errors");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(response.headers()[CONTENT_TYPE], "application/problem+json");
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    assert!(response.headers().contains_key(CONTENT_SECURITY_POLICY));
    let response_id = response.headers()["x-request-id"]
        .to_str()
        .unwrap()
        .to_owned();
    assert!(Id::from_str(&response_id).is_ok());
    let problem = body_json(response).await;
    assert_eq!(problem["request_id"], response_id);
    assert_eq!(problem["code"], "internal_error");
    assert!(!problem.to_string().contains("hunter2"));
}

#[test]
fn pending_inner_readiness_applies_backpressure_at_the_platform_boundary() {
    let mut service =
        HttpPlatformLayer::new(OriginPolicy::new("https://orbit.test")).layer(PendingService);
    let mut context = Context::from_waker(Waker::noop());

    assert!(matches!(service.poll_ready(&mut context), Poll::Pending));
}

#[tokio::test]
async fn inner_readiness_error_becomes_a_safe_correlated_problem_response() {
    let service = HttpPlatformLayer::new(OriginPolicy::new("https://orbit.test"))
        .layer(ReadinessErrorService);

    let response = service
        .oneshot(request("GET", "/readiness-error"))
        .await
        .expect("the platform boundary must absorb inner readiness errors");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(response.headers()[CONTENT_TYPE], "application/problem+json");
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    let response_id = response.headers()["x-request-id"]
        .to_str()
        .unwrap()
        .to_owned();
    let problem = body_json(response).await;
    assert_eq!(problem["request_id"], response_id);
    assert_eq!(problem["code"], "internal_error");
    assert!(!problem.to_string().contains("hunter2"));
}

#[tokio::test]
async fn expected_operational_unavailability_keeps_its_status() {
    let app = test_app(OriginPolicy::new("https://orbit.test"));

    let response = app.oneshot(request("GET", "/unavailable")).await.unwrap();

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn oversized_request_is_rejected_before_the_handler() {
    let layer =
        HttpPlatformLayer::new(OriginPolicy::new("https://orbit.test")).with_limits(HttpLimits {
            max_body_bytes: 3,
            ..HttpLimits::default()
        });
    let app = Router::new()
        .route("/probe", post(|| async { StatusCode::NO_CONTENT }))
        .layer(layer);
    let request = Request::builder()
        .method("POST")
        .uri("/probe")
        .header("origin", "https://orbit.test")
        .header(CONTENT_LENGTH, "4")
        .body(Body::from("four"))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(body_json(response).await["code"], "request_too_large");
}

#[tokio::test]
async fn readiness_requires_every_named_platform_check() {
    let registry = HealthRegistry::new()
        .with_check(HealthCheck::Migrations, || async { Ok(()) })
        .with_check(HealthCheck::DatabaseIntegrity, || async { Ok(()) })
        .with_check(HealthCheck::WritableStorage, || async { Ok(()) })
        .with_check(HealthCheck::Scheduler, || async {
            Err("worker stopped".to_owned())
        })
        .with_check(HealthCheck::CriticalConfig, || async { Ok(()) });

    assert!(registry.is_live());
    let readiness = registry.readiness().await;
    assert!(!readiness.ready);
    assert_eq!(readiness.checks.len(), 5);
    assert!(!readiness.checks["scheduler"].ready);
}
