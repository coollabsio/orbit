//! GIFs and stickers from KLIPY (<https://klipy.com>). The API key is a path segment of KLIPY's URLs, so the
//! browser never sees it: Orbit searches for the chat composer, and the browser shows the images straight from
//! KLIPY's CDN. The key never appears in an error or a log line: a `reqwest` error names the URL, so every
//! failure becomes a [`KlipyError`] without it.
//!
//! The answers are kept for a minute in memory, per server.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use orbit_platform::PublicResolver;
use serde::Serialize;
use serde_json::Value;
use thiserror::Error;
use url::{Host, Url};
use utoipa::ToSchema;

use crate::repositories::chat::ChatMediaKind;

/// KLIPY's API; the key and the catalog follow.
pub const KLIPY_API_BASE: &str = "https://api.klipy.com/api/v1";
const USER_AGENT: &str = "Orbit/1.0 (chat GIFs)";
const PER_PAGE: &str = "24";
const RATING: &str = "pg-13";
/// The most bytes of one answer.
const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;
/// The most bytes of the image a message shows, when KLIPY has no `md` size.
const MAX_FULL_BYTES: u64 = 4 * 1024 * 1024;
/// The largest width or height of an image a message shows.
pub const MAX_MEDIA_SIDE: u32 = 4096;
/// The most bytes of a media URL.
pub const MAX_MEDIA_URL_BYTES: usize = 2048;
const CACHE_TTL: Duration = Duration::from_secs(60);
const MAX_CACHE_ENTRIES: usize = 200;
/// The formats Orbit shows, best first.
const FORMATS: [&str; 2] = ["webp", "gif"];

/// The two KLIPY catalogs that chat uses.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum KlipyCatalog {
    Gifs,
    Stickers,
}

impl KlipyCatalog {
    /// The catalog of a path segment: `gifs` or `stickers`.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "gifs" => Some(Self::Gifs),
            "stickers" => Some(Self::Stickers),
            _ => None,
        }
    }

    const fn path(self) -> &'static str {
        match self {
            Self::Gifs => "gifs",
            Self::Stickers => "stickers",
        }
    }

    const fn kind(self) -> ChatMediaKind {
        match self {
            Self::Gifs => ChatMediaKind::Gif,
            Self::Stickers => ChatMediaKind::Sticker,
        }
    }
}

/// One size of a GIF or sticker on KLIPY's CDN.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, ToSchema)]
#[schema(as = KlipyImage)]
pub struct KlipyImage {
    /// An https URL on `klipy.com` or one of its subdomains.
    pub url: String,
    pub width: u32,
    pub height: u32,
}

/// A GIF or sticker that a chat message can show: send `full` with its `kind` and `title` as the message's
/// `media`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, ToSchema)]
#[schema(as = KlipyItem)]
pub struct KlipyItem {
    /// KLIPY's id.
    pub id: String,
    /// May be empty.
    pub title: String,
    pub kind: ChatMediaKind,
    /// A small size for the picker grid.
    pub preview: KlipyImage,
    /// The size a message shows.
    pub full: KlipyImage,
}

/// One page of trending GIFs or stickers, or of search results.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, ToSchema)]
#[schema(as = KlipyPage)]
pub struct KlipyPage {
    pub items: Vec<KlipyItem>,
    /// Another page follows.
    pub has_next: bool,
}

/// Why KLIPY gave no page. None of them names the URL: it has the API key.
#[derive(Debug, Error, Eq, PartialEq)]
pub enum KlipyError {
    #[error("the KLIPY request failed (connection error)")]
    Connection,
    #[error("KLIPY answered with HTTP {0}")]
    Status(u16),
    #[error("KLIPY returned an unexpected payload")]
    Payload,
}

type CacheKey = (KlipyCatalog, String, u32);
type Cache = Mutex<HashMap<CacheKey, (Instant, KlipyPage)>>;

pub struct KlipyClient {
    client: reqwest::Client,
    base: String,
    cache: Cache,
}

impl Default for KlipyClient {
    fn default() -> Self {
        Self::new(KLIPY_API_BASE)
    }
}

impl KlipyClient {
    /// A client of the API at `base`. Only public addresses are reached.
    #[must_use]
    pub fn new(base: &str) -> Self {
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(5))
            // A proxy from the environment would resolve the host itself, past the resolver here.
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .dns_resolver(Arc::new(PublicResolver))
            .build()
            .expect("the KLIPY client builds");
        Self {
            client,
            base: base.trim_end_matches('/').to_owned(),
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// Trending when `query` is blank, search results otherwise. `page` starts at 1.
    pub async fn browse(
        &self,
        api_key: &str,
        catalog: KlipyCatalog,
        query: &str,
        page: u32,
    ) -> Result<KlipyPage, KlipyError> {
        let query = query.trim();
        let key = (catalog, query.to_lowercase(), page);
        if let Some(cached) = self.cached(&key) {
            return Ok(cached);
        }
        let endpoint = if query.is_empty() {
            "trending"
        } else {
            "search"
        };
        let url = format!("{}/{api_key}/{}/{endpoint}", self.base, catalog.path());
        let page_text = page.to_string();
        let mut params = vec![
            ("page", page_text.as_str()),
            ("per_page", PER_PAGE),
            ("rating", RATING),
        ];
        if !query.is_empty() {
            params.push(("q", query));
        }
        // The error of `send` names the URL, and so the key: it is dropped here.
        let response = self
            .client
            .get(url)
            .query(&params)
            .send()
            .await
            .map_err(|_| KlipyError::Connection)?;
        if !response.status().is_success() {
            return Err(KlipyError::Status(response.status().as_u16()));
        }
        let body = read_body(response).await?;
        let value: Value = serde_json::from_slice(&body).map_err(|_| KlipyError::Payload)?;
        let result = normalize_page(&value, catalog.kind()).ok_or(KlipyError::Payload)?;
        self.store(key, result.clone());
        Ok(result)
    }

    fn cached(&self, key: &CacheKey) -> Option<KlipyPage> {
        let cache = self.cache.lock().expect("KLIPY cache lock");
        let (at, page) = cache.get(key)?;
        (at.elapsed() < CACHE_TTL).then(|| page.clone())
    }

    fn store(&self, key: CacheKey, page: KlipyPage) {
        let mut cache = self.cache.lock().expect("KLIPY cache lock");
        if cache.len() >= MAX_CACHE_ENTRIES {
            cache.retain(|_, (at, _)| at.elapsed() < CACHE_TTL);
            if cache.len() >= MAX_CACHE_ENTRIES {
                cache.clear();
            }
        }
        cache.insert(key, (Instant::now(), page));
    }
}

/// The whole body, or [`KlipyError::Payload`] above [`MAX_BODY_BYTES`].
async fn read_body(mut response: reqwest::Response) -> Result<Vec<u8>, KlipyError> {
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| KlipyError::Connection)? {
        if body.len() + chunk.len() > MAX_BODY_BYTES {
            return Err(KlipyError::Payload);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// The URL as Orbit stores it when it is a KLIPY image: https on `klipy.com` or one of its subdomains, without
/// a user, a password or a port, at most [`MAX_MEDIA_URL_BYTES`] bytes.
#[must_use]
pub fn media_url(raw: &str) -> Option<String> {
    if raw.len() > MAX_MEDIA_URL_BYTES {
        return None;
    }
    let url = Url::parse(raw).ok()?;
    let Some(Host::Domain(host)) = url.host() else {
        return None;
    };
    let klipy = host == "klipy.com" || host.ends_with(".klipy.com");
    (klipy
        && url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
        && url.as_str().len() <= MAX_MEDIA_URL_BYTES)
        .then(|| url.into())
}

/// One size of an item: its tier (`hd`, `md`, `sm`, `xs`), the image, and its bytes when KLIPY says.
struct Variant {
    tier: String,
    image: KlipyImage,
    size: Option<u64>,
}

/// `data.data[]` and `data.has_next` of a KLIPY answer. Items without a usable image (ads among them) are
/// skipped; `None` when the answer has no such list.
fn normalize_page(value: &Value, kind: ChatMediaKind) -> Option<KlipyPage> {
    let data = value.get("data")?;
    let items = data.get("data")?.as_array()?;
    Some(KlipyPage {
        items: items
            .iter()
            .filter_map(|item| normalize_item(item, kind))
            .collect(),
        has_next: data
            .get("has_next")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

fn normalize_item(item: &Value, kind: ChatMediaKind) -> Option<KlipyItem> {
    if item
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|kind| kind.eq_ignore_ascii_case("ad"))
    {
        return None;
    }
    let id = match item.get("id") {
        Some(Value::String(id)) if !id.is_empty() => id.clone(),
        Some(Value::Number(id)) => id.to_string(),
        _ => item
            .get("slug")?
            .as_str()
            .filter(|slug| !slug.is_empty())?
            .to_owned(),
    };
    let variants: Vec<Variant> = item
        .get("file")?
        .as_object()?
        .iter()
        .filter_map(|(tier, formats)| variant(tier, formats))
        .collect();
    let by_tier = |name: &str| variants.iter().find(|variant| variant.tier == name);
    // Smallest first: by bytes, or by area when KLIPY gives no size.
    let weight = |variant: &&Variant| {
        variant
            .size
            .unwrap_or_else(|| u64::from(variant.image.width) * u64::from(variant.image.height))
    };
    let full = by_tier("md")
        .or_else(|| {
            variants
                .iter()
                .filter(|variant| variant.size.is_some_and(|size| size <= MAX_FULL_BYTES))
                .max_by_key(weight)
        })
        .or_else(|| variants.iter().min_by_key(weight))?;
    let preview = by_tier("sm")
        .or_else(|| by_tier("xs"))
        .or_else(|| variants.iter().min_by_key(weight))?;
    Some(KlipyItem {
        id,
        title: item
            .get("title")
            .and_then(Value::as_str)
            .map(|title| title.trim().chars().take(200).collect())
            .unwrap_or_default(),
        kind,
        preview: preview.image.clone(),
        full: full.image.clone(),
    })
}

/// The best format of one tier (`file[tier][format] = {url, width, height, size}`) that a message may show.
fn variant(tier: &str, formats: &Value) -> Option<Variant> {
    let formats = formats.as_object()?;
    FORMATS.iter().find_map(|format| {
        let file = formats.get(*format)?;
        let url = media_url(file.get("url")?.as_str()?)?;
        let side = |name: &str| {
            file.get(name)
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())
                .filter(|value| (1..=MAX_MEDIA_SIDE).contains(value))
        };
        Some(Variant {
            tier: tier.to_owned(),
            image: KlipyImage {
                url,
                width: side("width")?,
                height: side("height")?,
            },
            size: file.get("size").and_then(Value::as_u64),
        })
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn file(url: &str, width: u64, height: u64, size: u64) -> Value {
        json!({"url": url, "width": width, "height": height, "size": size})
    }

    fn fixture() -> Value {
        json!({
            "result": true,
            "data": {
                "data": [
                    {
                        "id": 4_211_234,
                        "slug": "happy-cat",
                        "title": " Happy cat ",
                        "type": "gif",
                        "file": {
                            "hd": {
                                "gif": file("https://static.klipy.com/hd.gif", 498, 280, 5_000_000),
                                "webp": file("https://static.klipy.com/hd.webp", 498, 280, 900_000),
                            },
                            "md": {
                                "gif": file("https://static.klipy.com/md.gif", 320, 180, 800_000),
                                "webp": file("https://static.klipy.com/md.webp", 320, 180, 300_000),
                                "mp4": file("https://static.klipy.com/md.mp4", 320, 180, 100_000),
                            },
                            "sm": {
                                "gif": file("https://static.klipy.com/sm.gif", 220, 124, 200_000),
                            },
                            "xs": {
                                "webp": file("https://static.klipy.com/xs.webp", 90, 50, 10_000),
                            },
                        },
                    },
                    {
                        "id": "st-9",
                        "title": "Wave",
                        "file": {
                            "hd": {"webp": file("https://static.klipy.com/st-hd.webp", 512, 512, 3_000_000)},
                            "xs": {"webp": file("https://static.klipy.com/st-xs.webp", 64, 64, 4_000)},
                        },
                    },
                    {"id": 77, "type": "ad", "file": {"md": {"gif": file("https://static.klipy.com/ad.gif", 300, 250, 1)}}},
                    {"id": 78, "title": "empty"},
                    {"id": 79, "file": {"md": {"mp4": file("https://static.klipy.com/x.mp4", 320, 180, 1)}}},
                    {"id": 80, "file": {"md": {"gif": file("https://evil.example/x.gif", 320, 180, 1)}}},
                ],
                "current_page": 1,
                "per_page": 24,
                "has_next": true,
            }
        })
    }

    #[test]
    fn items_get_a_preview_and_a_full_size_and_ads_are_skipped() {
        let page = normalize_page(&fixture(), ChatMediaKind::Gif).unwrap();
        assert!(page.has_next);
        assert_eq!(
            page.items,
            vec![
                KlipyItem {
                    id: "4211234".to_owned(),
                    title: "Happy cat".to_owned(),
                    kind: ChatMediaKind::Gif,
                    preview: KlipyImage {
                        url: "https://static.klipy.com/sm.gif".to_owned(),
                        width: 220,
                        height: 124,
                    },
                    full: KlipyImage {
                        url: "https://static.klipy.com/md.webp".to_owned(),
                        width: 320,
                        height: 180,
                    },
                },
                KlipyItem {
                    id: "st-9".to_owned(),
                    title: "Wave".to_owned(),
                    kind: ChatMediaKind::Gif,
                    preview: KlipyImage {
                        url: "https://static.klipy.com/st-xs.webp".to_owned(),
                        width: 64,
                        height: 64,
                    },
                    full: KlipyImage {
                        url: "https://static.klipy.com/st-hd.webp".to_owned(),
                        width: 512,
                        height: 512,
                    },
                },
            ]
        );
    }

    #[test]
    fn a_sticker_page_without_md_takes_the_largest_size_under_the_cap() {
        let value = json!({"data": {"data": [{
            "id": "s1",
            "file": {
                "hd": {"webp": file("https://static.klipy.com/a.webp", 1000, 1000, 9_000_000)},
                "sm": {"webp": file("https://static.klipy.com/b.webp", 200, 200, 100_000)},
                "lg": {"gif": file("https://static.klipy.com/c.gif", 400, 400, 2_000_000)},
            },
        }]}});
        let page = normalize_page(&value, ChatMediaKind::Sticker).unwrap();
        assert!(!page.has_next);
        let item = &page.items[0];
        assert_eq!(item.kind, ChatMediaKind::Sticker);
        assert_eq!(item.full.url, "https://static.klipy.com/c.gif");
        assert_eq!(item.preview.url, "https://static.klipy.com/b.webp");
    }

    #[test]
    fn an_answer_without_a_list_is_a_payload_error() {
        assert!(normalize_page(&json!({"data": {}}), ChatMediaKind::Gif).is_none());
        assert!(normalize_page(&json!({"error": "bad key"}), ChatMediaKind::Gif).is_none());
    }

    #[test]
    fn only_https_klipy_urls_are_media() {
        assert_eq!(
            media_url("https://static.klipy.com/ii/a.webp").as_deref(),
            Some("https://static.klipy.com/ii/a.webp")
        );
        assert!(media_url("https://klipy.com/a.gif").is_some());
        for bad in [
            "https://evil.com/x.gif",
            "https://klipy.com.evil.com/x.gif",
            "https://evilklipy.com/x.gif",
            "http://static.klipy.com/x.gif",
            "https://user:pass@static.klipy.com/x.gif",
            "https://user@static.klipy.com/x.gif",
            "https://static.klipy.com:8443/x.gif",
            "https://1.2.3.4/x.gif",
            "not a url",
        ] {
            assert_eq!(media_url(bad), None, "{bad}");
        }
        let long = format!("https://static.klipy.com/{}", "a".repeat(2048));
        assert_eq!(media_url(&long), None);
    }
}
