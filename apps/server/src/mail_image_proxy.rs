//! The mail image proxy (spec §7 Content, §10): after **Load images**, remote images in a mail
//! load through Orbit, so the sender sees Orbit's address instead of the reader's, and gets no
//! cookie and no `Referer`.
//!
//! The sender picks the URL, so the fetch must not reach the server's own network (SSRF):
//!
//! - only `http(s)` URLs without credentials; an IP address in the URL is checked directly
//!   ([`is_public`]: no private, loopback, link-local, CGNAT, multicast, unspecified, broadcast,
//!   documentation or metadata address, also as IPv4-mapped IPv6 or ULA `fc00::/7`);
//! - a host name is resolved by [`CheckedResolver`], which refuses the name when any of its
//!   addresses is not public. reqwest connects to exactly the addresses it returns, so there is
//!   no second lookup a DNS rebind could change;
//! - reqwest follows no redirect: [`ImageProxy::fetch`] follows up to [`MAX_REDIRECTS`] itself
//!   and checks each hop the same way;
//! - no proxy from the environment (it would resolve the host itself), no cookie store (reqwest's
//!   `cookies` feature is off), no `Referer`;
//! - [`FETCH_TIMEOUT`] for all hops together, the body is read in chunks and dropped past
//!   [`MAX_IMAGE_BYTES`], and the bytes must sniff as PNG, JPEG, GIF, WebP or AVIF (never SVG).
//!
//! reqwest 0.12 `dns::Resolve` (<https://docs.rs/reqwest/0.12.28/reqwest/dns/trait.Resolve.html>):
//! an explicit port in the URL replaces the port of the resolved addresses.

use std::collections::HashMap;
use std::error::Error as _;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::header::{ACCEPT, LOCATION};
use thiserror::Error;
use tokio::sync::Semaphore;
use url::{Host, Url};

use crate::link_preview::is_public;

/// Largest image the proxy passes on.
pub const MAX_IMAGE_BYTES: usize = 5 * 1024 * 1024;
/// Redirects followed for one image.
pub const MAX_REDIRECTS: usize = 3;
/// Time for one image, every hop included.
pub const FETCH_TIMEOUT: Duration = Duration::from_secs(10);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_URL_BYTES: usize = 4096;
/// Fetches at once on this server; more wait.
const MAX_FETCHES: usize = 32;
const USER_AGENT: &str = "Mozilla/5.0 (compatible; OrbitImageProxy/1.0)";
const IMAGE_ACCEPT: &str = "image/avif,image/webp,image/png,image/jpeg,image/gif";

#[derive(Debug, Error)]
pub enum ImageProxyError {
    /// Not an `http(s)` URL, or a target that is not public (also after a redirect).
    #[error("the image URL names a target that is not public")]
    Refused,
    #[error("the image redirects more than {MAX_REDIRECTS} times")]
    TooManyRedirects,
    #[error("the image host answered {0}")]
    Status(u16),
    #[error("the image is larger than {MAX_IMAGE_BYTES} bytes")]
    TooLarge,
    /// The body is not one of the allowed image types.
    #[error("the response is not an allowed image")]
    NotImage,
    #[error("the image could not be fetched: {0}")]
    Fetch(String),
}

/// A fetched image.
#[derive(Debug)]
pub struct ProxiedImage {
    /// The type sniffed from the bytes.
    pub content_type: &'static str,
    pub bytes: Vec<u8>,
}

pub struct ImageProxy {
    client: reqwest::Client,
    fetches: Semaphore,
}

impl Default for ImageProxy {
    fn default() -> Self {
        Self::new()
    }
}

impl ImageProxy {
    #[must_use]
    pub fn new() -> Self {
        Self::build(HashMap::new())
    }

    /// Tests only: each host name in `hosts` resolves to its address without the public address
    /// check (a local test server); every other target is checked as usual.
    #[doc(hidden)]
    #[must_use]
    pub fn with_test_hosts(hosts: HashMap<String, SocketAddr>) -> Self {
        Self::build(hosts)
    }

    fn build(hosts: HashMap<String, SocketAddr>) -> Self {
        let test_hosts: Arc<HashMap<String, SocketAddr>> = Arc::new(
            hosts
                .into_iter()
                .map(|(host, address)| (host.to_ascii_lowercase(), address))
                .collect(),
        );
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(FETCH_TIMEOUT)
            .connect_timeout(CONNECT_TIMEOUT)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .referer(false)
            .dns_resolver(Arc::new(CheckedResolver { test_hosts }))
            .build()
            .expect("the image proxy client builds");
        Self {
            client,
            fetches: Semaphore::new(MAX_FETCHES),
        }
    }

    /// Fetches the image at `raw` (see the module docs for the rules).
    pub async fn fetch(&self, raw: &str) -> Result<ProxiedImage, ImageProxyError> {
        let url = target(raw)?;
        let _permit = self
            .fetches
            .acquire()
            .await
            .map_err(|_| ImageProxyError::Fetch("the proxy is closed".to_owned()))?;
        tokio::time::timeout(FETCH_TIMEOUT, self.follow(url))
            .await
            .map_err(|_| ImageProxyError::Fetch("timed out".to_owned()))?
    }

    async fn follow(&self, mut url: Url) -> Result<ProxiedImage, ImageProxyError> {
        for hop in 0..=MAX_REDIRECTS {
            let response = self
                .client
                .get(url.clone())
                .header(ACCEPT, IMAGE_ACCEPT)
                .send()
                .await
                .map_err(fetch_error)?;
            let status = response.status();
            if status.is_redirection() {
                if hop == MAX_REDIRECTS {
                    return Err(ImageProxyError::TooManyRedirects);
                }
                let location = response
                    .headers()
                    .get(LOCATION)
                    .and_then(|value| value.to_str().ok())
                    .ok_or(ImageProxyError::Status(status.as_u16()))?;
                let next = url.join(location).map_err(|_| ImageProxyError::Refused)?;
                url = target(next.as_str())?;
                continue;
            }
            if !status.is_success() {
                return Err(ImageProxyError::Status(status.as_u16()));
            }
            return read_image(response).await;
        }
        Err(ImageProxyError::TooManyRedirects)
    }
}

/// `raw` as a URL that may be fetched; a host name is checked when it resolves.
fn target(raw: &str) -> Result<Url, ImageProxyError> {
    if raw.len() > MAX_URL_BYTES {
        return Err(ImageProxyError::Refused);
    }
    let mut url = Url::parse(raw).map_err(|_| ImageProxyError::Refused)?;
    url.set_fragment(None);
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(ImageProxyError::Refused);
    }
    let allowed = match url.host() {
        Some(Host::Domain(_)) => true,
        Some(Host::Ipv4(ip)) => is_public(IpAddr::V4(ip)),
        Some(Host::Ipv6(ip)) => is_public(IpAddr::V6(ip)),
        None => false,
    };
    if !allowed {
        return Err(ImageProxyError::Refused);
    }
    Ok(url)
}

async fn read_image(mut response: reqwest::Response) -> Result<ProxiedImage, ImageProxyError> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_IMAGE_BYTES as u64)
    {
        return Err(ImageProxyError::TooLarge);
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(fetch_error)? {
        if bytes.len() + chunk.len() > MAX_IMAGE_BYTES {
            return Err(ImageProxyError::TooLarge);
        }
        bytes.extend_from_slice(&chunk);
    }
    let content_type = orbit_mail::sniff_image(&bytes).ok_or(ImageProxyError::NotImage)?;
    Ok(ProxiedImage {
        content_type,
        bytes,
    })
}

/// A refusal by [`CheckedResolver`], found again in the error chain of reqwest.
#[derive(Debug, Error)]
#[error("the host has an address that is not public")]
struct RefusedAddress;

fn fetch_error(error: reqwest::Error) -> ImageProxyError {
    let mut source = error.source();
    while let Some(cause) = source {
        if cause.downcast_ref::<RefusedAddress>().is_some() {
            return ImageProxyError::Refused;
        }
        source = cause.source();
    }
    if error.is_timeout() {
        ImageProxyError::Fetch("timed out".to_owned())
    } else {
        ImageProxyError::Fetch(error.to_string())
    }
}

/// Resolves a host name and refuses it when any address is not public.
struct CheckedResolver {
    test_hosts: Arc<HashMap<String, SocketAddr>>,
}

impl Resolve for CheckedResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let test_address = self
            .test_hosts
            .get(&name.as_str().to_ascii_lowercase())
            .copied();
        Box::pin(async move {
            if let Some(address) = test_address {
                return Ok(Box::new(std::iter::once(address)) as Addrs);
            }
            let addresses: Vec<SocketAddr> =
                tokio::net::lookup_host((name.as_str(), 0)).await?.collect();
            if addresses.is_empty() || addresses.iter().any(|address| !is_public(address.ip())) {
                return Err(Box::new(RefusedAddress) as Box<dyn std::error::Error + Send + Sync>);
            }
            Ok(Box::new(addresses.into_iter()) as Addrs)
        })
    }
}
