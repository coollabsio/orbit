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
            "/api/v1/integrations/discord/events"
                | "/api/v1/integrations/github/webhook"
                | "/api/v1/billing/stripe/webhook"
                | "/oauth/register"
                | "/oauth/token"
                | "/oauth/revoke"
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

        if is_ticket_socket(method, uri, headers) {
            return true;
        }

        let mut origins = headers.get_all(axum::http::header::ORIGIN).iter();
        let Some(origin) = origins.next() else {
            // Browsers send Origin on every POST and WebSocket upgrade, so a request without one
            // is not a browser. Without cookies it carries no ambient authority to forge: MCP
            // clients and native apps send a bearer secret, and native sign-in sends a password.
            // Present Origins still follow the normal policy and SDK validation.
            if uri.path() == "/mcp" {
                return true;
            }
            return !headers.contains_key(axum::http::header::COOKIE)
                && (bearer_token(headers).is_some()
                    || (*method == Method::POST && is_native_sign_in(uri.path())));
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

/// The sign-in POSTs a native app sends before it has a session: a password, a second factor, or a provider flow
/// (`/api/v1/auth/providers/{provider}/start` and the exchange, which needs the verifier the app holds).
fn is_native_sign_in(path: &str) -> bool {
    if matches!(
        path,
        "/api/v1/auth/login" | "/api/v1/auth/login/two-factor" | "/api/v1/auth/providers/exchange"
    ) {
        return true;
    }
    path.strip_prefix("/api/v1/auth/providers/")
        .and_then(|rest| rest.strip_suffix("/start"))
        .is_some_and(|provider| {
            !provider.is_empty() && provider.bytes().all(|byte| byte.is_ascii_lowercase())
        })
}

/// A co-editing socket opened with a socket ticket and no cookie, from any or no Origin: a
/// browser socket in a native app's WebView sends the WebView's Origin and cannot send the
/// session. The ticket is a short-lived secret the page obtained with the session, so the
/// request carries no ambient authority to forge; the socket verifies it.
fn is_ticket_socket(method: &Method, uri: &Uri, headers: &HeaderMap) -> bool {
    *method == Method::GET
        && headers.contains_key(axum::http::header::UPGRADE)
        && !headers.contains_key(axum::http::header::COOKIE)
        && uri.path().ends_with("/collab")
        && uri
            .query()
            .is_some_and(|query| query.split('&').any(|pair| pair.starts_with("ticket=")))
}

/// The token of the request's single `Authorization: Bearer` header. `None` when the header is
/// missing, repeated, of another scheme, or its token is empty or contains whitespace.
#[must_use]
pub fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let mut values = headers.get_all(axum::http::header::AUTHORIZATION).iter();
    let value = values.next()?;
    if values.next().is_some() {
        return None;
    }
    let (scheme, token) = value.to_str().ok()?.split_once(' ')?;
    (scheme.eq_ignore_ascii_case("bearer")
        && !token.is_empty()
        && !token.chars().any(char::is_whitespace))
    .then_some(token)
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
        "default-src 'self'; {script_source}style-src 'self'; style-src-attr 'unsafe-inline'; img-src 'self' data: blob: https:; frame-src https://www.youtube-nocookie.com; media-src 'self' https://video.twimg.com; frame-ancestors 'none'; object-src 'none'; base-uri 'none'; form-action 'self' https://github.com"
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

#[cfg(test)]
mod tests {
    use axum::http::header::{AUTHORIZATION, COOKIE, ORIGIN, UPGRADE};
    use axum::http::{HeaderMap, Method, Uri};

    use super::{OriginPolicy, bearer_token};

    fn permits(method: Method, path: &str, headers: &[(axum::http::HeaderName, &str)]) -> bool {
        let mut map = HeaderMap::new();
        for (name, value) in headers {
            map.append(name, value.parse().unwrap());
        }
        OriginPolicy::new("https://orbit.test").permits(
            &method,
            &path.parse::<Uri>().unwrap(),
            &map,
        )
    }

    const TASKS: &str = "/api/v1/workspaces/w/tasks";

    #[test]
    fn bearer_without_origin_or_cookie_is_allowed() {
        assert!(permits(
            Method::POST,
            TASKS,
            &[(AUTHORIZATION, "Bearer secret")]
        ));
        assert!(permits(
            Method::GET,
            "/api/v1/live",
            &[(AUTHORIZATION, "bearer secret"), (UPGRADE, "websocket")]
        ));
    }

    #[test]
    fn bearer_without_origin_but_with_a_cookie_is_rejected() {
        assert!(!permits(
            Method::POST,
            TASKS,
            &[
                (AUTHORIZATION, "Bearer secret"),
                (COOKIE, "__Host-orbit_session=s")
            ]
        ));
    }

    #[test]
    fn bearer_with_a_foreign_origin_is_rejected() {
        assert!(!permits(
            Method::POST,
            TASKS,
            &[
                (AUTHORIZATION, "Bearer secret"),
                (ORIGIN, "https://evil.example")
            ]
        ));
    }

    #[test]
    fn native_sign_in_without_origin_or_cookie_is_allowed() {
        assert!(permits(Method::POST, "/api/v1/auth/login", &[]));
        assert!(permits(Method::POST, "/api/v1/auth/login/two-factor", &[]));
        assert!(!permits(
            Method::POST,
            "/api/v1/auth/login",
            &[(COOKIE, "__Host-orbit_session=s")]
        ));
    }

    #[test]
    fn other_mutations_without_origin_are_rejected() {
        assert!(!permits(Method::POST, "/api/v1/auth/recovery/request", &[]));
        assert!(!permits(
            Method::POST,
            TASKS,
            &[(AUTHORIZATION, "Basic abc")]
        ));
    }

    const COLLAB: &str = "/api/v1/workspaces/w/pages/p/collab?v=1&epoch=e&ticket=t";

    #[test]
    fn ticket_sockets_are_allowed_from_any_or_no_origin_without_a_cookie() {
        for origin in [None, Some("null"), Some("https://evil.example")] {
            let mut headers = vec![(UPGRADE, "websocket")];
            headers.extend(origin.map(|origin| (ORIGIN, origin)));
            assert!(permits(Method::GET, COLLAB, &headers), "{origin:?}");
        }
        assert!(permits(
            Method::GET,
            "/api/v1/workspaces/w/tasks/t/collab?ticket=t&v=1",
            &[(UPGRADE, "websocket"), (ORIGIN, "file://")]
        ));
    }

    #[test]
    fn ticket_sockets_keep_the_origin_rules_otherwise() {
        let foreign = (ORIGIN, "https://evil.example");
        let upgrade = (UPGRADE, "websocket");
        // With a cookie.
        assert!(!permits(
            Method::GET,
            COLLAB,
            &[
                upgrade.clone(),
                foreign.clone(),
                (COOKIE, "orbit_session=s")
            ]
        ));
        // Without a ticket, or with a lookalike parameter.
        for path in [
            "/api/v1/workspaces/w/pages/p/collab?v=1&epoch=e",
            "/api/v1/workspaces/w/pages/p/collab?v=1&xticket=t",
            "/api/v1/workspaces/w/pages/p/collab",
        ] {
            assert!(
                !permits(Method::GET, path, &[upgrade.clone(), foreign.clone()]),
                "{path}"
            );
        }
        // Another socket or a plain request.
        assert!(!permits(
            Method::GET,
            "/api/v1/live?ticket=t",
            &[upgrade.clone(), foreign.clone()]
        ));
        assert!(!permits(
            Method::GET,
            "/api/v1/workspaces/w/pages/p/collab/x?ticket=t",
            &[upgrade.clone(), foreign.clone()]
        ));
        assert!(!permits(
            Method::POST,
            COLLAB,
            &[(ORIGIN, "https://evil.example")]
        ));
        assert!(!permits(Method::POST, COLLAB, &[upgrade, foreign]));
    }

    #[test]
    fn bearer_token_takes_exactly_one_well_formed_header() {
        let headers = |values: &[&str]| {
            let mut map = HeaderMap::new();
            for value in values {
                map.append(AUTHORIZATION, value.parse().unwrap());
            }
            map
        };
        assert_eq!(bearer_token(&headers(&["Bearer abc"])), Some("abc"));
        assert_eq!(bearer_token(&headers(&["BEARER abc"])), Some("abc"));
        assert_eq!(bearer_token(&headers(&["Bearer "])), None);
        assert_eq!(bearer_token(&headers(&["Bearer a b"])), None);
        assert_eq!(bearer_token(&headers(&["Basic abc"])), None);
        assert_eq!(bearer_token(&headers(&["Bearer abc", "Bearer abc"])), None);
        assert_eq!(bearer_token(&headers(&[])), None);
    }
}
