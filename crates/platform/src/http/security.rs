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
    allow_any_http_origin: bool,
    trusted_proxies: Vec<IpNet>,
}

impl OriginPolicy {
    #[must_use]
    pub fn new(application_origin: impl Into<String>) -> Self {
        let mut allowed_origins = BTreeSet::new();
        let application_origin = application_origin.into();
        allowed_origins.insert(application_origin);
        Self {
            allowed_origins,
            allow_any_http_origin: false,
            trusted_proxies: Vec::new(),
        }
    }

    #[must_use]
    pub fn allow_any_http_origin(mut self) -> Self {
        self.allow_any_http_origin = true;
        self
    }

    #[must_use]
    pub fn allow_origin(mut self, origin: impl Into<String>) -> Self {
        self.allowed_origins.insert(origin.into());
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

    pub(crate) fn permits(&self, method: &Method, uri: &Uri, headers: &HeaderMap) -> bool {
        if matches!(
            uri.path(),
            "/api/v1/integrations/discord/events" | "/api/v1/integrations/github/webhook"
        ) {
            return true;
        }
        if matches!(
            *method,
            Method::GET | Method::HEAD | Method::OPTIONS | Method::TRACE
        ) && !headers.contains_key(axum::http::header::UPGRADE)
        {
            return true;
        }

        let mut origins = headers.get_all(axum::http::header::ORIGIN).iter();
        let Some(origin) = origins.next() else {
            return false;
        };
        origins.next().is_none()
            && origin.to_str().ok().is_some_and(|origin| {
                if self.allow_any_http_origin {
                    is_valid_http_origin(origin)
                } else {
                    is_valid_http_origin(origin) && self.allowed_origins.contains(origin)
                }
            })
    }
}

pub(crate) struct RequestSecurity {
    pub client_ip: ClientIp,
    pub request_id: RequestId,
    pub transport: RequestTransport,
}

pub(crate) fn inspect_request(
    headers: &HeaderMap,
    peer: Option<IpAddr>,
    policy: &OriginPolicy,
) -> Result<RequestSecurity, ()> {
    let trusted = policy.is_trusted_proxy(peer);
    let mut client_ip = peer.unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
    let mut request_id = RequestId::new();
    // The composed listener is plain TCP. Request-target text is not connection metadata and an
    // absolute-form `https://` URI must never be accepted as proof of TLS.
    let mut transport = RequestTransport::Http;

    if trusted {
        let chain = forwarded_for(headers)?;
        if !chain.is_empty() {
            let first = *chain.first().ok_or(())?;
            client_ip = chain
                .iter()
                .rev()
                .copied()
                .find(|ip| !policy.is_trusted_ip(*ip))
                .unwrap_or(first);
        }
        if let Some(value) = consistent_header(headers, X_REQUEST_ID)? {
            request_id =
                RequestId::from_trusted_header(value.to_str().map_err(|_| ())?).ok_or(())?;
        }
        if let Some(forwarded_transport) = forwarded_transport(headers)? {
            transport = forwarded_transport;
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

fn consistent_header<'a>(
    headers: &'a HeaderMap,
    name: &str,
) -> Result<Option<&'a HeaderValue>, ()> {
    let mut values = headers.get_all(name).iter();
    let value = values.next();
    if value.is_some_and(|value| values.any(|candidate| candidate != value)) {
        return Err(());
    }
    Ok(value)
}

fn forwarded_for(headers: &HeaderMap) -> Result<Vec<IpAddr>, ()> {
    headers
        .get_all(X_FORWARDED_FOR)
        .iter()
        .map(|value| value.to_str().map_err(|_| ()))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .flat_map(|value| value.split(',').map(str::trim))
        .map(|value| value.parse().map_err(|_| ()))
        .collect()
}

fn forwarded_transport(headers: &HeaderMap) -> Result<Option<RequestTransport>, ()> {
    let mut transport = None;
    for value in headers.get_all(X_FORWARDED_PROTO) {
        for candidate in value.to_str().map_err(|_| ())?.split(',').map(str::trim) {
            let candidate = match candidate {
                "https" | "wss" => RequestTransport::Https,
                "http" | "ws" => RequestTransport::Http,
                _ => return Err(()),
            };
            if transport.is_some_and(|transport| transport != candidate) {
                return Err(());
            }
            transport = Some(candidate);
        }
    }
    Ok(transport)
}

pub(crate) fn add_security_headers(
    headers: &mut HeaderMap,
    secure: bool,
    csp_script_hash: Option<&str>,
) {
    let script_source = csp_script_hash
        .map(|hash| format!("script-src 'self' '{hash}'; "))
        .unwrap_or_else(|| "script-src 'self'; ".to_owned());
    let policy = format!(
        "default-src 'self'; {script_source}style-src 'self'; style-src-attr 'unsafe-inline'; img-src 'self' data: blob: https:; frame-ancestors 'none'; object-src 'none'; base-uri 'none'; form-action 'self' https://github.com"
    );
    headers.insert(
        CONTENT_SECURITY_POLICY,
        HeaderValue::from_str(&policy).expect("validated CSP values form a valid header"),
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

fn is_valid_http_origin(origin: &str) -> bool {
    let Ok(uri) = origin.parse::<Uri>() else {
        return false;
    };
    matches!(uri.scheme_str(), Some("http" | "https"))
        && uri.authority().is_some_and(|authority| {
            let host = authority.host();
            let port = &authority.as_str()[host.len()..];
            !host.is_empty()
                && !authority.as_str().contains('@')
                && (port.is_empty()
                    || port
                        .strip_prefix(':')
                        .is_some_and(|port| !port.is_empty() && port.parse::<u16>().is_ok()))
        })
        && uri.path() == "/"
        && uri.query().is_none()
}
