//! Previews of the URLs in chat messages. The server reads the page for the members: an X post
//! through the FxTwitter API, a YouTube video through oEmbed, any other page through its Open
//! Graph tags.
//!
//! A member picks the URL, so the fetch must not reach the server's own network (SSRF). Only
//! `http(s)` URLs on the default ports are read; the DNS resolver drops every address that is not
//! public, so a redirect and a DNS rebind are covered too; an IP address in a URL is checked
//! directly.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use orbit_platform::{PublicResolver, is_public_ip};
use reqwest::header::{ACCEPT, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;
use url::{Host, Url};
use utoipa::ToSchema;

/// The page bytes that are read. The tags are in the head.
const MAX_BODY_BYTES: usize = 512 * 1024;
const MAX_REDIRECTS: usize = 5;
const MAX_URL_BYTES: usize = 2048;
const HIT_TTL: Duration = Duration::from_secs(60 * 60);
const MISS_TTL: Duration = Duration::from_secs(10 * 60);
const MAX_CACHE_ENTRIES: usize = 2048;
/// Fetches that run at the same time. More wait.
const MAX_FETCHES: usize = 8;
const MAX_FETCH_WAIT: Duration = Duration::from_secs(4);
const USER_AGENT: &str = "Mozilla/5.0 (compatible; OrbitBot/1.0; link preview)";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum LinkPreviewKind {
    /// A page with Open Graph tags.
    Link,
    /// A post on X.
    X,
    /// A YouTube video: `video_id` plays it.
    Youtube,
    /// The URL is an image.
    Image,
}

/// What a URL shows as a card. Image URLs are always `https`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct LinkPreview {
    pub url: String,
    pub kind: LinkPreviewKind,
    pub site_name: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub image_url: Option<String>,
    /// The page asks for a wide image (`summary_large_image`).
    pub large_image: bool,
    pub icon_url: Option<String>,
    pub author_name: Option<String>,
    pub author_handle: Option<String>,
    pub author_avatar_url: Option<String>,
    pub video_id: Option<String>,
    /// The video of a post, as an `mp4` on X's video host. `image_url` is its poster.
    pub video_url: Option<String>,
    pub video_width: Option<u32>,
    pub video_height: Option<u32>,
    /// Unix seconds.
    pub created_at: Option<i64>,
    pub replies: Option<u64>,
    pub reposts: Option<u64>,
    pub likes: Option<u64>,
}

impl LinkPreview {
    fn new(url: &Url, kind: LinkPreviewKind) -> Self {
        Self {
            url: url.to_string(),
            kind,
            site_name: None,
            title: None,
            description: None,
            image_url: None,
            large_image: false,
            icon_url: None,
            author_name: None,
            author_handle: None,
            author_avatar_url: None,
            video_id: None,
            video_url: None,
            video_width: None,
            video_height: None,
            created_at: None,
            replies: None,
            reposts: None,
            likes: None,
        }
    }
}

/// Where the previews come from. The defaults are the public services; tests name local ones.
#[derive(Clone, Debug)]
pub struct LinkPreviewConfig {
    pub x_api: String,
    pub youtube_oembed: String,
    /// Tests only: lets a URL name a local address and any port.
    pub allow_local: bool,
}

impl Default for LinkPreviewConfig {
    fn default() -> Self {
        Self {
            x_api: "https://api.fxtwitter.com".to_owned(),
            youtube_oembed: "https://www.youtube.com/oembed".to_owned(),
            allow_local: false,
        }
    }
}

type Cache = Mutex<HashMap<String, (Instant, Option<LinkPreview>)>>;

pub struct LinkPreviewer {
    client: reqwest::Client,
    config: LinkPreviewConfig,
    cache: Cache,
    fetches: Semaphore,
}

impl Default for LinkPreviewer {
    fn default() -> Self {
        Self::new(LinkPreviewConfig::default())
    }
}

impl LinkPreviewer {
    #[must_use]
    pub fn new(config: LinkPreviewConfig) -> Self {
        let allow_local = config.allow_local;
        let mut builder = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(8))
            .connect_timeout(Duration::from_secs(5))
            // A proxy from the environment would resolve the host itself, past the checks here.
            .no_proxy()
            .redirect(reqwest::redirect::Policy::custom(move |attempt| {
                if attempt.previous().len() >= MAX_REDIRECTS {
                    attempt.error("too many redirects")
                } else if allowed_target(attempt.url(), allow_local) {
                    attempt.follow()
                } else {
                    attempt.error("redirect to a target that is not public")
                }
            }));
        if !allow_local {
            builder = builder.dns_resolver(Arc::new(PublicResolver));
        }
        Self {
            client: builder.build().expect("the link preview client builds"),
            config,
            cache: Mutex::new(HashMap::new()),
            fetches: Semaphore::new(MAX_FETCHES),
        }
    }

    /// The preview of `raw`, or `None`: not a URL that may be read, not reachable, or a page
    /// with nothing to show. Both answers are kept for a while.
    pub async fn preview(&self, raw: &str) -> Option<LinkPreview> {
        let url = parse_target(raw, self.config.allow_local)?;
        let key = url.to_string();
        if let Some(cached) = self.cached(&key) {
            return cached;
        }
        let preview = {
            // A request that cannot start soon gives no preview; it does not join an endless queue.
            let _permit = tokio::time::timeout(MAX_FETCH_WAIT, self.fetches.acquire())
                .await
                .ok()?
                .ok()?;
            // A request that waited may find the answer of the one before it.
            if let Some(cached) = self.cached(&key) {
                return cached;
            }
            self.fetch(&url).await
        };
        self.store(key, preview.clone());
        preview
    }

    fn cached(&self, key: &str) -> Option<Option<LinkPreview>> {
        let cache = self.cache.lock().expect("link preview cache lock");
        let (at, preview) = cache.get(key)?;
        (at.elapsed() < ttl(preview)).then(|| preview.clone())
    }

    fn store(&self, key: String, preview: Option<LinkPreview>) {
        let mut cache = self.cache.lock().expect("link preview cache lock");
        if cache.len() >= MAX_CACHE_ENTRIES {
            cache.retain(|_, (at, preview)| at.elapsed() < ttl(preview));
            if cache.len() >= MAX_CACHE_ENTRIES {
                cache.clear();
            }
        }
        cache.insert(key, (Instant::now(), preview));
    }

    async fn fetch(&self, url: &Url) -> Option<LinkPreview> {
        if let Some(path) = x_status_path(url) {
            return self.fetch_x(url, &path).await;
        }
        if let Some(video_id) = youtube_video_id(url) {
            return self.fetch_youtube(url, video_id).await;
        }
        self.fetch_page(url).await
    }

    async fn fetch_x(&self, url: &Url, path: &str) -> Option<LinkPreview> {
        let response = self
            .client
            .get(format!("{}{path}", self.config.x_api))
            .send()
            .await
            .ok()?;
        if !response.status().is_success() {
            return None;
        }
        let body: FxResponse = serde_json::from_slice(&read_body(response).await?).ok()?;
        Some(x_preview(url, body.tweet?))
    }

    async fn fetch_youtube(&self, url: &Url, video_id: String) -> Option<LinkPreview> {
        let watch = format!("https://www.youtube.com/watch?v={video_id}");
        let response = self
            .client
            .get(&self.config.youtube_oembed)
            .query(&[("url", watch.as_str()), ("format", "json")])
            .send()
            .await
            .ok()?;
        if !response.status().is_success() {
            return None;
        }
        let body: OEmbed = serde_json::from_slice(&read_body(response).await?).ok()?;
        let mut preview = LinkPreview::new(url, LinkPreviewKind::Youtube);
        preview.site_name = Some("YouTube".to_owned());
        preview.title = Some(clean(&body.title?, 200)?);
        preview.author_name = body.author_name.and_then(|name| clean(&name, 100));
        preview.image_url = Some(format!("https://i.ytimg.com/vi/{video_id}/hqdefault.jpg"));
        preview.video_id = Some(video_id);
        Some(preview)
    }

    async fn fetch_page(&self, url: &Url) -> Option<LinkPreview> {
        let response = self
            .client
            .get(url.clone())
            .header(ACCEPT, "text/html,application/xhtml+xml,image/*;q=0.8")
            .send()
            .await
            .ok()?;
        if !response.status().is_success() {
            return None;
        }
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if content_type.starts_with("image/") {
            let mut preview = LinkPreview::new(url, LinkPreviewKind::Image);
            preview.image_url = Some(https_url(url, url.as_str())?);
            return Some(preview);
        }
        if !(content_type.starts_with("text/html")
            || content_type.starts_with("application/xhtml+xml"))
        {
            return None;
        }
        // Relative image URLs are relative to the page that answered, after the redirects.
        let base = response.url().clone();
        let body = read_body(response).await?;
        page_preview(url, &base, &String::from_utf8_lossy(&body))
    }
}

fn ttl(preview: &Option<LinkPreview>) -> Duration {
    if preview.is_some() { HIT_TTL } else { MISS_TTL }
}

/// The start of the body, up to [`MAX_BODY_BYTES`].
async fn read_body(mut response: reqwest::Response) -> Option<Vec<u8>> {
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        let room = MAX_BODY_BYTES - body.len();
        body.extend_from_slice(&chunk[..chunk.len().min(room)]);
        if body.len() >= MAX_BODY_BYTES {
            break;
        }
    }
    Some(body)
}

/// The URL of a message as a target that may be read.
fn parse_target(raw: &str, allow_local: bool) -> Option<Url> {
    if raw.len() > MAX_URL_BYTES {
        return None;
    }
    let mut url = Url::parse(raw).ok()?;
    url.set_fragment(None);
    allowed_target(&url, allow_local).then_some(url)
}

/// An `http(s)` URL without credentials, on the default port, whose host is not a local address.
/// A host name is checked when it resolves ([`PublicResolver`]).
fn allowed_target(url: &Url, allow_local: bool) -> bool {
    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }
    if !url.username().is_empty() || url.password().is_some() {
        return false;
    }
    if allow_local {
        return url.host().is_some();
    }
    if url.port().is_some() {
        return false;
    }
    match url.host() {
        Some(Host::Domain(_)) => true,
        Some(Host::Ipv4(ip)) => is_public_ip(IpAddr::V4(ip)),
        Some(Host::Ipv6(ip)) => is_public_ip(IpAddr::V6(ip)),
        None => false,
    }
}

fn host_of(url: &Url) -> Option<String> {
    let host = url.host_str()?.to_ascii_lowercase();
    Some(host.strip_prefix("www.").unwrap_or(&host).to_owned())
}

/// `/{user}/status/{id}` of a post on X, for the FxTwitter API.
fn x_status_path(url: &Url) -> Option<String> {
    let host = host_of(url)?;
    if !matches!(
        host.as_str(),
        "x.com" | "twitter.com" | "mobile.twitter.com" | "mobile.x.com"
    ) {
        return None;
    }
    let mut segments = url.path_segments()?;
    let user = segments.next()?;
    let (status, id) = (segments.next()?, segments.next()?);
    let name = |value: &str| {
        !value.is_empty() && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    };
    (name(user) && status == "status" && !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()))
        .then(|| format!("/{user}/status/{id}"))
}

fn youtube_video_id(url: &Url) -> Option<String> {
    let host = host_of(url)?;
    let mut segments = url.path_segments()?;
    let id = match host.as_str() {
        "youtu.be" => segments.next()?.to_owned(),
        "youtube.com" | "m.youtube.com" | "music.youtube.com" => match segments.next()? {
            "watch" => url
                .query_pairs()
                .find(|(key, _)| key == "v")
                .map(|(_, value)| value.into_owned())?,
            "shorts" | "live" | "embed" => segments.next()?.to_owned(),
            _ => return None,
        },
        _ => return None,
    };
    (id.len() == 11
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'))
    .then_some(id)
}

#[derive(Deserialize)]
struct OEmbed {
    title: Option<String>,
    author_name: Option<String>,
}

#[derive(Deserialize)]
struct FxResponse {
    tweet: Option<FxTweet>,
}

#[derive(Deserialize)]
struct FxTweet {
    #[serde(default)]
    text: String,
    author: Option<FxAuthor>,
    replies: Option<u64>,
    retweets: Option<u64>,
    likes: Option<u64>,
    created_timestamp: Option<i64>,
    media: Option<FxMedia>,
}

#[derive(Deserialize)]
struct FxAuthor {
    name: Option<String>,
    screen_name: Option<String>,
    avatar_url: Option<String>,
}

#[derive(Deserialize)]
struct FxMedia {
    #[serde(default)]
    photos: Vec<FxPhoto>,
    #[serde(default)]
    videos: Vec<FxVideo>,
}

#[derive(Deserialize)]
struct FxPhoto {
    url: Option<String>,
}

#[derive(Deserialize)]
struct FxVideo {
    url: Option<String>,
    thumbnail_url: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    #[serde(default)]
    formats: Vec<FxFormat>,
}

#[derive(Deserialize)]
struct FxFormat {
    url: Option<String>,
    bitrate: Option<u64>,
    container: Option<String>,
}

/// The host of X's videos. The web client may play media of this host only (CSP `media-src`).
const X_VIDEO_HOST: &str = "video.twimg.com";
/// About 720p. A chat card needs no more.
const MAX_VIDEO_BITRATE: u64 = 2_500_000;

/// The `mp4` to play: the best one up to [`MAX_VIDEO_BITRATE`], else the smallest, else the default.
fn x_video_url(video: &FxVideo) -> Option<String> {
    let mut formats: Vec<(u64, &str)> = video
        .formats
        .iter()
        .filter(|format| format.container.as_deref() == Some("mp4"))
        .filter_map(|format| Some((format.bitrate?, format.url.as_deref()?)))
        .collect();
    formats.sort_unstable_by_key(|(bitrate, _)| *bitrate);
    let picked = formats
        .iter()
        .rev()
        .find(|(bitrate, _)| *bitrate <= MAX_VIDEO_BITRATE)
        .or(formats.first())
        .map(|(_, url)| *url)
        .or(video.url.as_deref())?;
    let url = Url::parse(picked).ok()?;
    (url.scheme() == "https" && url.host_str() == Some(X_VIDEO_HOST)).then(|| url.into())
}

fn x_preview(url: &Url, tweet: FxTweet) -> LinkPreview {
    let mut preview = LinkPreview::new(url, LinkPreviewKind::X);
    preview.site_name = Some("X".to_owned());
    preview.description = clean_lines(&tweet.text, 1000);
    if let Some(author) = tweet.author {
        preview.author_name = author.name.and_then(|name| clean(&name, 100));
        preview.author_handle = author.screen_name.and_then(|name| clean(&name, 50));
        preview.author_avatar_url = author.avatar_url.and_then(|src| https_url(url, &src));
    }
    if let Some(media) = tweet.media {
        // A post's video goes before its photos: the poster shows, and the reader can play it.
        let video = media.videos.into_iter().next();
        let poster = video.as_ref().and_then(|video| video.thumbnail_url.clone());
        if let Some(video) = video.filter(|_| poster.is_some()) {
            preview.video_url = x_video_url(&video);
            preview.video_width = video.width;
            preview.video_height = video.height;
        }
        let photo = media.photos.into_iter().find_map(|photo| photo.url);
        let image = if preview.video_url.is_some() {
            poster
        } else {
            photo.or(poster)
        };
        preview.image_url = image.and_then(|src| https_url(url, &src));
        preview.large_image = preview.image_url.is_some();
    }
    // Within what a date can show (to the year 2100).
    preview.created_at = tweet
        .created_timestamp
        .filter(|seconds| (0..4_102_444_800).contains(seconds));
    preview.replies = tweet.replies;
    preview.reposts = tweet.retweets;
    preview.likes = tweet.likes;
    preview
}

/// The card of a page from its Open Graph and Twitter tags. `None` without a title.
fn page_preview(url: &Url, base: &Url, html: &str) -> Option<LinkPreview> {
    let head = HtmlHead::parse(html);
    let meta = |names: &[&str]| {
        names
            .iter()
            .filter_map(|name| head.meta.get(*name))
            .map(|content| content.trim())
            // An empty tag (`og:title=""`) says nothing: the next name is read.
            .find(|content| !content.is_empty())
    };
    let title = meta(&["og:title", "twitter:title"])
        .or(head.title.as_deref())
        .and_then(|title| clean(title, 200))?;
    let mut preview = LinkPreview::new(url, LinkPreviewKind::Link);
    preview.title = Some(title);
    preview.description = meta(&["og:description", "twitter:description", "description"])
        .and_then(|description| clean(description, 400));
    preview.site_name = meta(&["og:site_name"])
        .and_then(|name| clean(name, 60))
        .or_else(|| host_of(base));
    preview.image_url = meta(&[
        "og:image:secure_url",
        "og:image",
        "og:image:url",
        "twitter:image",
        "twitter:image:src",
    ])
    .and_then(|src| https_url(base, src));
    preview.large_image =
        preview.image_url.is_some() && meta(&["twitter:card"]) == Some("summary_large_image");
    preview.icon_url = https_url(base, head.icon.as_deref().unwrap_or("/favicon.ico"));
    Some(preview)
}

/// `src` as an absolute `https` URL. The web client loads no `http` image (CSP), so an `http`
/// one is asked for over `https`.
fn https_url(base: &Url, src: &str) -> Option<String> {
    let mut url = base.join(src.trim()).ok()?;
    if url.scheme() == "http" {
        url.set_scheme("https").ok()?;
    }
    (url.scheme() == "https" && url.as_str().len() <= MAX_URL_BYTES).then(|| url.into())
}

/// One line of text: entities decoded, white space collapsed, at most `max` characters.
fn clean(text: &str, max: usize) -> Option<String> {
    // Twice: some sites (GitHub) escape their tags two times (`&amp;amp;`).
    let text = decode_entities(&decode_entities(text));
    truncate(text.split_whitespace().collect::<Vec<_>>().join(" "), max)
}

/// Text that keeps its line breaks (a post).
fn clean_lines(text: &str, max: usize) -> Option<String> {
    truncate(decode_entities(text).trim().to_owned(), max)
}

fn truncate(text: String, max: usize) -> Option<String> {
    if text.is_empty() {
        return None;
    }
    if text.chars().count() <= max {
        return Some(text);
    }
    let mut short: String = text.chars().take(max - 1).collect();
    short.truncate(short.trim_end().len());
    short.push('…');
    Some(short)
}

fn decode_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        // Only the next bytes are read for the `;`: a page of `&` must not cost a scan to its end each time.
        let decoded = rest.as_bytes()[1..]
            .iter()
            .take(11)
            .position(|byte| *byte == b';')
            .and_then(|end| Some((entity(&rest[1..=end])?, end + 2)));
        match decoded {
            Some((character, length)) => {
                out.push(character);
                rest = &rest[length..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn entity(name: &str) -> Option<char> {
    match name {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some(' '),
        _ => {
            let number = name.strip_prefix('#')?;
            let code = match number.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => number.parse().ok()?,
            };
            char::from_u32(code)
        }
    }
}

/// The `<meta>`, `<title>` and icon `<link>` tags of a page. This reads tags only as far as a
/// preview needs: it is not an HTML parser.
#[derive(Default)]
struct HtmlHead {
    /// `property` or `name` (lower case) to `content`. The first tag of a name wins.
    meta: HashMap<String, String>,
    title: Option<String>,
    icon: Option<String>,
}

impl HtmlHead {
    fn parse(html: &str) -> Self {
        let mut head = Self::default();
        let mut rest = html;
        while let Some(start) = rest.find('<') {
            rest = &rest[start + 1..];
            if let Some(after) = rest.strip_prefix("!--") {
                rest = after.find("-->").map_or("", |end| &after[end + 3..]);
                continue;
            }
            let name_end = rest
                .find(|c: char| !c.is_ascii_alphanumeric())
                .unwrap_or(rest.len());
            let name = rest[..name_end].to_ascii_lowercase();
            match name.as_str() {
                "body" => break,
                "meta" | "link" => {
                    let (attributes, after) = attributes(&rest[name_end..]);
                    rest = after;
                    if name == "meta" {
                        let key = attributes.get("property").or(attributes.get("name"));
                        if let (Some(key), Some(content)) = (key, attributes.get("content")) {
                            head.meta
                                .entry(key.to_ascii_lowercase())
                                .or_insert_with(|| content.clone());
                        }
                    } else if head.icon.is_none()
                        && attributes.get("rel").is_some_and(|rel| {
                            rel.to_ascii_lowercase()
                                .split_whitespace()
                                .any(|word| word == "icon")
                        })
                    {
                        head.icon = attributes.get("href").cloned();
                    }
                }
                "title" if head.title.is_none() => {
                    let Some(open) = rest.find('>') else { break };
                    let text = &rest[open + 1..];
                    let close = text.find("</").unwrap_or(text.len());
                    head.title = Some(text[..close].to_owned());
                    rest = &text[close..];
                }
                // The text of these can hold `<`.
                "script" | "style" => {
                    let close = format!("</{name}");
                    rest = find_ignore_case(rest, &close).map_or("", |end| &rest[end..]);
                }
                _ => {}
            }
        }
        head
    }
}

fn find_ignore_case(text: &str, pattern: &str) -> Option<usize> {
    text.as_bytes()
        .windows(pattern.len())
        .position(|window| window.eq_ignore_ascii_case(pattern.as_bytes()))
}

/// The attributes of a tag (names in lower case) and the text after the tag.
fn attributes(mut rest: &str) -> (HashMap<String, String>, &str) {
    let mut attributes = HashMap::new();
    loop {
        rest = rest.trim_start_matches(|c: char| c.is_whitespace() || c == '/');
        if rest.is_empty() {
            return (attributes, rest);
        }
        if let Some(after) = rest.strip_prefix('>') {
            return (attributes, after);
        }
        let name_end = rest
            .find(|c: char| c.is_whitespace() || matches!(c, '=' | '>' | '/'))
            .unwrap_or(rest.len());
        let name = rest[..name_end].to_ascii_lowercase();
        rest = rest[name_end..].trim_start();
        let mut value = String::new();
        if let Some(after) = rest.strip_prefix('=') {
            let after = after.trim_start();
            let quote = after.chars().next().filter(|c| matches!(c, '"' | '\''));
            let (raw, tail) = match quote {
                Some(quote) => match after[1..].find(quote) {
                    Some(end) => (&after[1..=end], &after[end + 2..]),
                    None => (&after[1..], ""),
                },
                None => {
                    let end = after
                        .find(|c: char| c.is_whitespace() || c == '>')
                        .unwrap_or(after.len());
                    (&after[..end], &after[end..])
                }
            };
            value = raw.to_owned();
            rest = tail;
        }
        if name.is_empty() {
            // A character that starts no attribute: step over it.
            let mut characters = rest.chars();
            characters.next();
            rest = characters.as_str();
        } else {
            attributes.entry(name).or_insert(value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(value: &str) -> Url {
        Url::parse(value).unwrap()
    }

    #[test]
    fn only_public_http_targets_on_the_default_port_are_read() {
        for allowed in [
            "https://example.com/a?b=c",
            "http://example.com",
            "https://93.184.216.34/",
            "https://[2606:4700::1111]/",
        ] {
            assert!(parse_target(allowed, false).is_some(), "{allowed}");
        }
        for refused in [
            "ftp://example.com",
            "file:///etc/passwd",
            "https://user:secret@example.com",
            "https://example.com:8443",
            "http://127.0.0.1",
            "http://10.0.0.5/admin",
            "http://172.16.0.1",
            "http://192.168.1.1",
            "http://169.254.169.254/latest/meta-data",
            "http://100.64.0.1",
            "http://0.0.0.0",
            "http://2130706433",
            "http://0x7f.1",
            "http://[::1]",
            "http://[::ffff:127.0.0.1]",
            "http://[64:ff9b::7f00:1]",
            "http://[fd00::1]",
            "http://[fe80::1]",
            "not a url",
        ] {
            assert!(parse_target(refused, false).is_none(), "{refused}");
        }
        assert_eq!(
            parse_target("https://example.com/a#part", false)
                .unwrap()
                .as_str(),
            "https://example.com/a"
        );
    }

    #[test]
    fn posts_on_x_and_youtube_videos_are_recognized() {
        let x = |value: &str| x_status_path(&url(value));
        assert_eq!(
            x("https://x.com/jack/status/20").as_deref(),
            Some("/jack/status/20")
        );
        assert_eq!(
            x("https://www.twitter.com/jack/status/20/photo/1?s=20").as_deref(),
            Some("/jack/status/20")
        );
        assert_eq!(
            x("https://x.com/i/status/20").as_deref(),
            Some("/i/status/20")
        );
        assert_eq!(x("https://x.com/jack"), None);
        assert_eq!(x("https://x.com/jack/status/abc"), None);
        assert_eq!(x("https://notx.com/jack/status/20"), None);

        let youtube = |value: &str| youtube_video_id(&url(value));
        for video in [
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=10s",
            "https://youtu.be/dQw4w9WgXcQ?si=abc",
            "https://m.youtube.com/shorts/dQw4w9WgXcQ",
            "https://youtube.com/live/dQw4w9WgXcQ",
            "https://www.youtube.com/embed/dQw4w9WgXcQ",
        ] {
            assert_eq!(youtube(video).as_deref(), Some("dQw4w9WgXcQ"), "{video}");
        }
        assert_eq!(youtube("https://www.youtube.com/@RickAstleyYT"), None);
        assert_eq!(youtube("https://www.youtube.com/watch?v=short"), None);
        assert_eq!(youtube("https://youtu.be/dQw4w9WgXc\"Q"), None);
    }

    #[test]
    fn a_page_becomes_a_card_from_its_open_graph_tags() {
        let html = r#"<!doctype html><html><head>
            <!-- <meta property="og:title" content="In a comment"> -->
            <script>if (a < b) document.write('<meta property="og:title" content="In a script">')</script>
            <title>The  page
              title</title>
            <META PROPERTY="og:title" CONTENT="Tom &amp; Jerry &#8212; &quot;Episode&quot; &#x31;">
            <meta content='A   description
              on two lines' name=description />
            <meta property=og:image content=/images/card.png>
            <meta name="twitter:card" content="summary_large_image">
            <meta property="og:site_name" content="Example Site">
            <link rel="stylesheet" href="/app.css">
            <link rel="shortcut icon" href="icons/fav.png">
            </head><body><meta property="og:description" content="In the body"></body></html>"#;
        let page = url("http://example.com/posts/1");
        let preview = page_preview(&page, &url("http://example.com/blog/post"), html).unwrap();
        assert_eq!(preview.kind, LinkPreviewKind::Link);
        assert_eq!(preview.url, "http://example.com/posts/1");
        assert_eq!(
            preview.title.as_deref(),
            Some("Tom & Jerry — \"Episode\" 1")
        );
        assert_eq!(
            preview.description.as_deref(),
            Some("A description on two lines")
        );
        assert_eq!(preview.site_name.as_deref(), Some("Example Site"));
        // Relative to the page that answered, and never plain http.
        assert_eq!(
            preview.image_url.as_deref(),
            Some("https://example.com/images/card.png")
        );
        assert!(preview.large_image);
        assert_eq!(
            preview.icon_url.as_deref(),
            Some("https://example.com/blog/icons/fav.png")
        );
    }

    #[test]
    fn a_page_without_tags_uses_its_title_and_one_without_a_title_gives_nothing() {
        let page = url("https://www.example.com/a");
        let preview = page_preview(&page, &page, "<html><title>Plain</title><p>Text</p>").unwrap();
        assert_eq!(preview.title.as_deref(), Some("Plain"));
        assert_eq!(preview.site_name.as_deref(), Some("example.com"));
        assert_eq!(
            preview.icon_url.as_deref(),
            Some("https://www.example.com/favicon.ico")
        );
        assert_eq!(preview.image_url, None);
        assert!(!preview.large_image);

        let empty_tag = r#"<title>Rust</title><meta property="og:title" content=""><meta name="twitter:title" content=" ">"#;
        let preview = page_preview(&page, &page, empty_tag).unwrap();
        assert_eq!(preview.title.as_deref(), Some("Rust"));

        assert!(page_preview(&page, &page, "<html><body>No title</body></html>").is_none());
        assert!(page_preview(&page, &page, "<title>  </title>").is_none());
        // Broken markup ends the read; it does not panic.
        assert!(page_preview(&page, &page, "<meta property=\"og:title\" content=\"Open").is_some());
        assert!(page_preview(&page, &page, "<title").is_none());
        assert!(page_preview(&page, &page, "<meta ===== \"é\" <<").is_none());
    }

    #[test]
    fn long_text_is_cut_at_the_limit() {
        let long = "é".repeat(300);
        let cut = clean(&long, 200).unwrap();
        assert_eq!(cut.chars().count(), 200);
        assert!(cut.ends_with('…'));
        assert_eq!(
            clean("a &unknown; b & c", 50).as_deref(),
            Some("a &unknown; b & c")
        );
        assert_eq!(
            clean("&&&&é&amp&#x26;;", 50).as_deref(),
            Some("&&&&é&amp&;")
        );
        assert_eq!(
            clean("Heroku &amp;amp; Netlify", 50).as_deref(),
            Some("Heroku & Netlify")
        );
    }

    #[test]
    fn a_post_on_x_becomes_a_card_from_the_fxtwitter_answer() {
        let body: FxResponse = serde_json::from_str(
            r#"{"code":200,"message":"OK","tweet":{"url":"https://x.com/jack/status/20","text":"line one\nline two",
            "author":{"screen_name":"jack","name":"jack","avatar_url":"https://pbs.twimg.com/a.jpg"},
            "replies":1,"retweets":2,"likes":3,"created_timestamp":1142974214,
            "media":{"photos":[{"url":"https://pbs.twimg.com/photo.jpg"}],
            "videos":[{"url":"https://video.twimg.com/1080.mp4","thumbnail_url":"https://pbs.twimg.com/poster.jpg","width":1920,"height":1080,
            "formats":[{"url":"https://video.twimg.com/pl.m3u8","container":"m3u8"},
            {"url":"https://video.twimg.com/270.mp4","bitrate":256000,"container":"mp4"},
            {"url":"https://video.twimg.com/720.mp4","bitrate":2176000,"container":"mp4"},
            {"url":"https://video.twimg.com/1080.mp4","bitrate":10368000,"container":"mp4"}]}]}}}"#,
        )
        .unwrap();
        let preview = x_preview(&url("https://x.com/jack/status/20"), body.tweet.unwrap());
        assert_eq!(preview.kind, LinkPreviewKind::X);
        assert_eq!(preview.description.as_deref(), Some("line one\nline two"));
        assert_eq!(preview.author_handle.as_deref(), Some("jack"));
        assert_eq!(
            preview.author_avatar_url.as_deref(),
            Some("https://pbs.twimg.com/a.jpg")
        );
        assert_eq!(
            preview.image_url.as_deref(),
            Some("https://pbs.twimg.com/poster.jpg")
        );
        // The 720p file, not the largest one.
        assert_eq!(
            preview.video_url.as_deref(),
            Some("https://video.twimg.com/720.mp4")
        );
        assert_eq!(
            (preview.video_width, preview.video_height),
            (Some(1920), Some(1080))
        );

        // Only X's video host plays (CSP), and without the formats the default file does.
        let video = |url: &str| FxVideo {
            url: Some(url.to_owned()),
            thumbnail_url: None,
            width: None,
            height: None,
            formats: Vec::new(),
        };
        assert_eq!(
            x_video_url(&video("https://video.twimg.com/a.mp4")).as_deref(),
            Some("https://video.twimg.com/a.mp4")
        );
        assert_eq!(x_video_url(&video("https://evil.example/a.mp4")), None);
        assert_eq!(x_video_url(&video("http://video.twimg.com/a.mp4")), None);
        assert_eq!(
            (preview.replies, preview.reposts, preview.likes),
            (Some(1), Some(2), Some(3))
        );
        assert_eq!(preview.created_at, Some(1_142_974_214));
    }
}
