use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr};

use axum::http::header::{
    CONTENT_SECURITY_POLICY, HeaderName, HeaderValue, REFERRER_POLICY, X_CONTENT_TYPE_OPTIONS,
};
use axum::http::{HeaderMap, Method, Uri};
use ipnet::IpNet;

use super::RequestId;

const X_FORWARDED_FOR: &str = "x-forwarded-for";
const X_FORWARDED_PROTO: &str = "x-forwarded-proto";
const X_REQUEST_ID: &str = "x-request-id";

/// The client address after applying the explicitly trusted proxy policy.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ClientIp(pub IpAddr);

/// Canonical original request transport after applying trusted proxy headers.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum RequestTransport {
    Http,
    Https,
}

impl RequestTransport {
    #[must_use]
    pub const fn is_secure(self) -> bool {
        matches!(self, Self::Https)
    }
}

/// Same-origin browser policy and explicitly trusted reverse-proxy networks.
#[derive(Clone, Debug)]
pub struct OriginPolicy {
    allowed_origins: BTreeSet<String>,
    trusted_proxies: Vec<IpNet>,
}

impl OriginPolicy {
    #[must_use]
    pub fn new(application_origin: impl Into<String>) -> Self {
        let mut allowed_origins = BTreeSet::new();
        let application_origin = application_origin.into();
        allowed_origins.insert(normalize_origin(&application_origin));
        Self {
            allowed_origins,
            trusted_proxies: Vec::new(),
        }
    }

    #[must_use]
    pub fn allow_origin(mut self, origin: impl Into<String>) -> Self {
        let origin = origin.into();
        self.allowed_origins.insert(normalize_origin(&origin));
        self
    }

    #[must_use]
    pub fn trust_proxy(mut self, network: IpNet) -> Self {
        self.trusted_proxies.push(network);
        self
    }

    pub(crate) fn is_trusted_proxy(&self, peer: Option<IpAddr>) -> bool {
        peer.is_some_and(|ip| self.is_trusted_ip(ip))
    }

    fn is_trusted_ip(&self, ip: IpAddr) -> bool {
        self.trusted_proxies
            .iter()
            .any(|network| network.contains(&ip))
    }

    pub(crate) fn permits(&self, method: &Method, headers: &HeaderMap) -> bool {
        if matches!(
            *method,
            Method::GET | Method::HEAD | Method::OPTIONS | Method::TRACE
        ) {
            return true;
        }

        let mut origins = headers.get_all(axum::http::header::ORIGIN).iter();
        let Some(origin) = origins.next() else {
            let browser_request = headers.contains_key(axum::http::header::COOKIE)
                || headers
                    .keys()
                    .any(|name| name.as_str().starts_with("sec-fetch-"));
            return !browser_request;
        };
        origins.next().is_none()
            && origin
                .to_str()
                .ok()
                .map(normalize_origin)
                .is_some_and(|origin| self.allowed_origins.contains(&origin))
    }
}

pub(crate) struct RequestSecurity {
    pub client_ip: ClientIp,
    pub request_id: RequestId,
    pub transport: RequestTransport,
}

pub(crate) fn inspect_request(
    headers: &HeaderMap,
    uri: &Uri,
    peer: Option<IpAddr>,
    policy: &OriginPolicy,
) -> Result<RequestSecurity, ()> {
    let trusted = policy.is_trusted_proxy(peer);
    let mut client_ip = peer.unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
    let mut request_id = RequestId::new();
    let mut transport = if uri.scheme_str() == Some("https") {
        RequestTransport::Https
    } else {
        RequestTransport::Http
    };

    if trusted {
        if let Some(value) = single_header(headers, X_FORWARDED_FOR)? {
            let value = value.to_str().map_err(|_| ())?;
            let chain = value
                .split(',')
                .map(str::trim)
                .map(str::parse)
                .collect::<Result<Vec<IpAddr>, _>>()
                .map_err(|_| ())?;
            let first = *chain.first().ok_or(())?;
            client_ip = chain
                .iter()
                .rev()
                .copied()
                .find(|ip| !policy.is_trusted_ip(*ip))
                .unwrap_or(first);
        }
        if let Some(value) = single_header(headers, X_REQUEST_ID)? {
            request_id =
                RequestId::from_trusted_header(value.to_str().map_err(|_| ())?).ok_or(())?;
        }
        if let Some(value) = single_header(headers, X_FORWARDED_PROTO)? {
            transport = match value.to_str().map_err(|_| ())? {
                "https" => RequestTransport::Https,
                "http" => RequestTransport::Http,
                _ => return Err(()),
            };
        }
    }

    Ok(RequestSecurity {
        client_ip: ClientIp(client_ip),
        request_id,
        transport,
    })
}

pub(crate) fn strip_forwarding_headers(headers: &mut HeaderMap) {
    headers.remove(X_REQUEST_ID);
    headers.remove(X_FORWARDED_FOR);
    headers.remove(X_FORWARDED_PROTO);
}

fn single_header<'a>(headers: &'a HeaderMap, name: &str) -> Result<Option<&'a HeaderValue>, ()> {
    let mut values = headers.get_all(name).iter();
    let value = values.next();
    if values.next().is_some() {
        return Err(());
    }
    Ok(value)
}

pub(crate) fn add_security_headers(headers: &mut HeaderMap, secure: bool) {
    headers.insert(
        CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'self'; frame-ancestors 'none'; object-src 'none'; base-uri 'none'; form-action 'self'",
        ),
    );
    headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    headers.insert(
        REFERRER_POLICY,
        HeaderValue::from_static("strict-origin-when-cross-origin"),
    );
    headers.insert(
        HeaderName::from_static("permissions-policy"),
        HeaderValue::from_static("camera=(), microphone=(), geolocation=(), payment=()"),
    );
    if secure {
        headers.insert(
            HeaderName::from_static("strict-transport-security"),
            HeaderValue::from_static("max-age=31536000; includeSubDomains"),
        );
    }
}

fn normalize_origin(origin: &str) -> String {
    origin.trim_end_matches('/').to_ascii_lowercase()
}
