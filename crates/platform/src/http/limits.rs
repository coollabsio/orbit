use std::time::Duration;

use axum::http::header::{CONTENT_LENGTH, CONTENT_TYPE, UPGRADE};
use axum::http::{Method, Request};

/// Conservative request limits applied before application handlers run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HttpLimits {
    /// The largest `multipart/form-data` body: file uploads. The upload routes check their own
    /// limits too.
    pub max_body_bytes: usize,
    /// The largest body of any other request (JSON, forms, webhooks). `max_body_bytes` caps it too.
    pub max_other_body_bytes: usize,
    pub max_uri_bytes: usize,
    pub max_header_count: usize,
    pub max_header_bytes: usize,
    /// How long a normal request may take until its response starts; see [`HttpLimits::request_timeout`].
    pub request_timeout: Option<Duration>,
}

impl Default for HttpLimits {
    fn default() -> Self {
        Self {
            max_body_bytes: 1024 * 1024,
            // Axum's default for its JSON and form extractors.
            max_other_body_bytes: 2 * 1024 * 1024,
            max_uri_bytes: 8 * 1024,
            max_header_count: 100,
            max_header_bytes: 32 * 1024,
            request_timeout: Some(Duration::from_secs(60)),
        }
    }
}

impl HttpLimits {
    /// The body limit of `request`: only uploads (multipart) may use the large one.
    pub(crate) fn body_limit<B>(&self, request: &Request<B>) -> usize {
        let multipart = request
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| {
                value
                    .trim_start()
                    .to_ascii_lowercase()
                    .starts_with("multipart/form-data")
            });
        if multipart {
            self.max_body_bytes
        } else {
            self.max_other_body_bytes.min(self.max_body_bytes)
        }
    }

    /// The timeout of `request`. None for WebSockets, uploads, downloads and exports (which can
    /// take long by design), or the admin routes (backups, storage moves, image compression).
    pub(crate) fn request_timeout<B>(&self, request: &Request<B>) -> Option<Duration> {
        let path = request.uri().path();
        let multipart = request
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.to_ascii_lowercase().starts_with("multipart/"));
        let long = request.headers().contains_key(UPGRADE)
            || request.method() == Method::CONNECT
            || multipart
            || path.starts_with("/api/v1/admin/")
            || path.ends_with("/download")
            || path.ends_with("/export");
        self.request_timeout.filter(|_| !long)
    }

    pub(crate) fn request_is_too_large<B>(&self, request: &Request<B>) -> bool {
        if request.uri().to_string().len() > self.max_uri_bytes
            || request.headers().len() > self.max_header_count
        {
            return true;
        }

        let header_bytes = request
            .headers()
            .iter()
            .fold(0usize, |total, (name, value)| {
                total.saturating_add(name.as_str().len() + value.as_bytes().len())
            });
        if header_bytes > self.max_header_bytes {
            return true;
        }

        request
            .headers()
            .get(CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<usize>().ok())
            .is_some_and(|length| length > self.body_limit(request))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(content_type: &str, length: usize) -> Request<()> {
        Request::builder()
            .method("POST")
            .uri("/api/v1/anything")
            .header(CONTENT_TYPE, content_type)
            .header(CONTENT_LENGTH, length)
            .body(())
            .unwrap()
    }

    #[test]
    fn long_running_and_streaming_requests_have_no_request_timeout() {
        let limits = HttpLimits::default();
        let request = |method: &str, path: &str, header: Option<(&str, &str)>| {
            let mut builder = Request::builder().method(method).uri(path);
            if let Some((name, value)) = header {
                builder = builder.header(name, value);
            }
            builder.body(()).unwrap()
        };
        for timed in [
            request("GET", "/api/v1/workspaces/w/tasks", None),
            request(
                "POST",
                "/api/v1/workspaces/w/tasks",
                Some(("content-type", "application/json")),
            ),
        ] {
            assert!(limits.request_timeout(&timed).is_some(), "{}", timed.uri());
        }
        for excluded in [
            request(
                "GET",
                "/api/v1/workspaces/w/live",
                Some(("upgrade", "websocket")),
            ),
            request(
                "POST",
                "/api/v1/workspaces/w/chat/files",
                Some(("content-type", "multipart/form-data; boundary=x")),
            ),
            request("POST", "/api/v1/admin/backups", None),
            request("GET", "/api/v1/workspaces/w/exports/e/download", None),
            request("GET", "/api/v1/workspaces/w/pages/p/export", None),
        ] {
            assert!(
                limits.request_timeout(&excluded).is_none(),
                "{}",
                excluded.uri()
            );
        }
    }

    #[test]
    fn only_multipart_bodies_may_use_the_upload_limit() {
        let limits = HttpLimits {
            max_body_bytes: 100 * 1024 * 1024,
            ..HttpLimits::default()
        };
        let three_mib = 3 * 1024 * 1024;
        assert!(limits.request_is_too_large(&request("application/json", three_mib)));
        assert!(!limits.request_is_too_large(&request("application/json", 1024)));
        assert!(
            !limits.request_is_too_large(&request("multipart/form-data; boundary=x", three_mib))
        );
        assert!(limits.request_is_too_large(&request(
            "multipart/form-data; boundary=x",
            101 * 1024 * 1024
        )));
    }
}
