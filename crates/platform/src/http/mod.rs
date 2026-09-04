mod limits;
mod request_id;
mod security;

use std::convert::Infallible;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};

use axum::body::{Body, to_bytes};
use axum::extract::ConnectInfo;
use axum::http::header::{CONTENT_TYPE, HeaderName, HeaderValue};
use axum::http::{Request, Response, StatusCode};
use axum::response::Response as AxumResponse;
use tower::{Layer, Service};

use crate::Problem;

pub use limits::HttpLimits;
pub use request_id::RequestId;
pub use security::{ClientIp, OriginPolicy, RequestTransport};
use security::{add_security_headers, inspect_request, strip_forwarding_headers};

/// Applies Orbit's request identity, proxy, origin, limit, and response policy.
#[derive(Clone, Debug)]
pub struct HttpPlatformLayer {
    origin_policy: OriginPolicy,
    limits: HttpLimits,
}

impl HttpPlatformLayer {
    #[must_use]
    pub fn new(origin_policy: OriginPolicy) -> Self {
        Self {
            origin_policy,
            limits: HttpLimits::default(),
        }
    }

    #[must_use]
    pub fn with_limits(mut self, limits: HttpLimits) -> Self {
        self.limits = limits;
        self
    }
}

impl<S> Layer<S> for HttpPlatformLayer {
    type Service = HttpPlatformService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        HttpPlatformService {
            inner,
            origin_policy: self.origin_policy.clone(),
            limits: self.limits,
            readiness: InnerReadiness::Checking,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum InnerReadiness {
    Checking,
    Ready,
    Failed,
}

#[derive(Debug)]
pub struct HttpPlatformService<S> {
    inner: S,
    origin_policy: OriginPolicy,
    limits: HttpLimits,
    readiness: InnerReadiness,
}

impl<S: Clone> Clone for HttpPlatformService<S> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            origin_policy: self.origin_policy.clone(),
            limits: self.limits,
            readiness: InnerReadiness::Checking,
        }
    }
}

impl<S> Service<Request<Body>> for HttpPlatformService<S>
where
    S: Service<Request<Body>, Response = AxumResponse> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Send + 'static,
{
    type Response = AxumResponse;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, context: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.readiness {
            InnerReadiness::Ready | InnerReadiness::Failed => Poll::Ready(Ok(())),
            InnerReadiness::Checking => match self.inner.poll_ready(context) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(Ok(())) => {
                    self.readiness = InnerReadiness::Ready;
                    Poll::Ready(Ok(()))
                }
                Poll::Ready(Err(_)) => {
                    self.readiness = InnerReadiness::Failed;
                    Poll::Ready(Ok(()))
                }
            },
        }
    }

    fn call(&mut self, request: Request<Body>) -> Self::Future {
        let readiness = std::mem::replace(&mut self.readiness, InnerReadiness::Checking);
        let clone = self.inner.clone();
        let mut inner = std::mem::replace(&mut self.inner, clone);
        let policy = self.origin_policy.clone();
        let limits = self.limits;

        Box::pin(async move {
            let peer = request
                .extensions()
                .get::<ConnectInfo<SocketAddr>>()
                .map(|connect| connect.0.ip());
            let directly_secure = request.uri().scheme_str() == Some("https");
            let inspected = inspect_request(request.headers(), request.uri(), peer, &policy);
            let (request_id, client_ip, transport) = match inspected {
                Ok(values) => (values.request_id, values.client_ip, values.transport),
                Err(()) => {
                    let request_id = RequestId::new();
                    return Ok(finish_response(
                        problem_response(
                            StatusCode::BAD_REQUEST,
                            "invalid_proxy_headers",
                            "Invalid proxy headers",
                            "A trusted proxy supplied invalid forwarding headers.",
                            &request_id,
                            request.uri().path(),
                        ),
                        &request_id,
                        directly_secure,
                    ));
                }
            };

            if !policy.permits(request.method(), request.headers()) {
                return Ok(finish_response(
                    problem_response(
                        StatusCode::FORBIDDEN,
                        "origin_forbidden",
                        "Forbidden origin",
                        "The request origin is not allowed.",
                        &request_id,
                        request.uri().path(),
                    ),
                    &request_id,
                    transport.is_secure(),
                ));
            }

            if limits.request_is_too_large(&request) {
                return Ok(finish_response(
                    too_large_response(&request_id, request.uri().path()),
                    &request_id,
                    transport.is_secure(),
                ));
            }

            if matches!(readiness, InnerReadiness::Failed) {
                tracing::error!(
                    request_id = request_id.as_str(),
                    error_type = std::any::type_name::<S::Error>(),
                    "inner HTTP service readiness failed"
                );
                return Ok(finish_response(
                    Problem::internal(request_id.as_str(), request.uri().path()).into_response(),
                    &request_id,
                    transport.is_secure(),
                ));
            }

            let (mut parts, body) = request.into_parts();
            let instance = parts.uri.path().to_owned();
            let method = parts.method.clone();
            let body = match to_bytes(body, limits.max_body_bytes).await {
                Ok(body) => body,
                Err(_) => {
                    return Ok(finish_response(
                        too_large_response(&request_id, &instance),
                        &request_id,
                        transport.is_secure(),
                    ));
                }
            };
            strip_forwarding_headers(&mut parts.headers);
            parts.extensions.insert(request_id.clone());
            parts.extensions.insert(client_ip);
            parts.extensions.insert(transport);

            tracing::debug!(
                request_id = request_id.as_str(),
                %method,
                "http request"
            );
            let response = match inner
                .call(Request::from_parts(parts, Body::from(body)))
                .await
            {
                Ok(response) => response,
                Err(_) => {
                    tracing::error!(
                        request_id = request_id.as_str(),
                        error_type = std::any::type_name::<S::Error>(),
                        "inner HTTP service failed"
                    );
                    return Ok(finish_response(
                        Problem::internal(request_id.as_str(), &instance).into_response(),
                        &request_id,
                        transport.is_secure(),
                    ));
                }
            };
            let response = if response.status() == StatusCode::INTERNAL_SERVER_ERROR {
                let status = response.status();
                tracing::error!(request_id = request_id.as_str(), %status, "http request failed");
                Problem::internal(request_id.as_str(), &instance).into_response()
            } else {
                response
            };

            Ok(finish_response(
                response,
                &request_id,
                transport.is_secure(),
            ))
        })
    }
}

fn too_large_response(request_id: &RequestId, instance: &str) -> AxumResponse {
    problem_response(
        StatusCode::PAYLOAD_TOO_LARGE,
        "request_too_large",
        "Request too large",
        "The request exceeds the configured limit.",
        request_id,
        instance,
    )
}

fn problem_response(
    status: StatusCode,
    code: &str,
    title: &str,
    detail: &str,
    request_id: &RequestId,
    instance: &str,
) -> AxumResponse {
    let problem = Problem {
        type_uri: format!("https://docs.orbit.dev/problems/{code}"),
        title: title.to_owned(),
        status: status.as_u16(),
        code: code.to_owned(),
        detail: detail.to_owned(),
        instance: instance.to_owned(),
        request_id: request_id.to_string(),
        errors: None,
        conflict: None,
    };
    let body = serde_json::to_vec(&problem).expect("Problem serialization cannot fail");
    Response::builder()
        .status(status)
        .header(CONTENT_TYPE, "application/problem+json")
        .body(Body::from(body))
        .expect("static Problem response is valid")
}

trait ProblemResponse {
    fn into_response(self) -> AxumResponse;
}

impl ProblemResponse for Problem {
    fn into_response(self) -> AxumResponse {
        let body = serde_json::to_vec(&self).expect("Problem serialization cannot fail");
        Response::builder()
            .status(self.status)
            .header(CONTENT_TYPE, "application/problem+json")
            .body(Body::from(body))
            .expect("Problem response is valid")
    }
}

fn finish_response(
    mut response: AxumResponse,
    request_id: &RequestId,
    secure: bool,
) -> AxumResponse {
    let request_id = HeaderValue::from_str(request_id.as_str())
        .expect("validated request IDs are valid header values");
    response
        .headers_mut()
        .insert(HeaderName::from_static("x-request-id"), request_id);
    add_security_headers(response.headers_mut(), secure);
    response
}
