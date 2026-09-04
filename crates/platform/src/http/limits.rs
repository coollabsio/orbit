use axum::http::Request;

/// Conservative request limits applied before application handlers run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HttpLimits {
    pub max_body_bytes: usize,
    pub max_uri_bytes: usize,
    pub max_header_count: usize,
    pub max_header_bytes: usize,
}

impl Default for HttpLimits {
    fn default() -> Self {
        Self {
            max_body_bytes: 1024 * 1024,
            max_uri_bytes: 8 * 1024,
            max_header_count: 100,
            max_header_bytes: 32 * 1024,
        }
    }
}

impl HttpLimits {
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
            .get(axum::http::header::CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<usize>().ok())
            .is_some_and(|length| length > self.max_body_bytes)
    }
}
