//! The HTML view document: mail HTML sanitized on each view (no HTML is stored), wrapped in a full document with
//! Orbit's nonce height script, and the CSP header that goes with it (spec §6.5).
//!
//! `ammonia` removes every element and attribute that is not on the allow list. It cannot filter whole style sheets
//! (<https://docs.rs/ammonia/4.2.4/ammonia/struct.Builder.html#method.filter_style_properties>), so the content of
//! `<style>` elements and every `style` attribute go through [`filter_css`], a conservative filter of our own: it
//! drops any rule or declaration that could load something (`url(`, `@import`, `@font-face`, …). The CSP header
//! (`default-src 'none'`, `font-src 'none'`, a fixed `img-src`) blocks what might still get through.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use base64::Engine as _;
use rsa::rand_core::{OsRng, RngCore as _};

/// Maps the content ID of a `cid:` image (without angle brackets) to the attachment URL, or `None` when the message
/// has no such part.
pub type CidUrl = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;
/// Maps a remote `http(s)` image URL to its signed image proxy URL.
pub type ProxyUrl = Arc<dyn Fn(&str) -> String + Send + Sync>;

/// How [`render_document`] rewrites images. The URL callbacks are `Arc`s because `ammonia` keeps its attribute
/// filter as a `'static` closure.
#[derive(Clone)]
pub struct RenderOptions<'a> {
    pub cid_url: CidUrl,
    /// `None` until the reader clicks **Load images**: remote images are removed and counted.
    pub proxy_url: Option<ProxyUrl>,
    /// Goes only into the height script; use the same value in [`csp_header`]. See [`new_nonce`].
    pub nonce: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    pub html: String,
    /// Remote images removed because `proxy_url` was `None` (drives the **Load images** bar).
    pub blocked_remote_images: usize,
}

/// Longest style sheet text kept from one message; longer CSS is dropped entirely.
const MAX_CSS_BYTES: usize = 256 * 1024;
/// Passes of [`filter_css`] before it gives up and drops the CSS (each pass that changes the text shortens it).
const MAX_CSS_PASSES: usize = 8;
/// Image types that may stay inline as `data:` URLs (the inline image types of the spec; no SVG).
const DATA_IMAGE_TYPES: [&str; 5] = [
    "image/png",
    "image/jpeg",
    "image/gif",
    "image/webp",
    "image/avif",
];
/// Text that must not appear in CSS, checked lowercase and without whitespace. Each can load a resource or run code.
const CSS_FORBIDDEN: [&str; 14] = [
    "url(",
    "image(",
    "image-set(",
    "cross-fade(",
    "src(",
    "attr(",
    "expression(",
    "javascript:",
    "@import",
    "@font-face",
    "@namespace",
    "behavior",
    "-moz-binding",
    "binding:",
];

/// A fresh CSP nonce: 16 random bytes, base64url.
pub fn new_nonce() -> String {
    let mut bytes = [0_u8; 16];
    OsRng.fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// Sanitizes `html` and returns the full view document.
pub fn render_document(html: &str, opts: &RenderOptions<'_>) -> Rendered {
    let blocked = Arc::new(AtomicUsize::new(0));
    let body = body_builder(opts, Arc::clone(&blocked))
        .clean(html)
        .to_string();
    let css = style_sheets(html);
    Rendered {
        html: document(&css, &body, opts.nonce),
        blocked_remote_images: blocked.load(Ordering::Relaxed),
    }
}

/// The view document for a message without an HTML part: `text` escaped in a `<pre>`.
pub fn render_text_document(text: &str, nonce: &str) -> String {
    let mut body = String::with_capacity(text.len() + 16);
    body.push_str("<pre>");
    for character in text.chars() {
        match character {
            '&' => body.push_str("&amp;"),
            '<' => body.push_str("&lt;"),
            '>' => body.push_str("&gt;"),
            other => body.push(other),
        }
    }
    body.push_str("</pre>");
    document(
        "pre{white-space:pre-wrap;word-wrap:break-word}",
        &body,
        nonce,
    )
}

/// The `Content-Security-Policy` of the view document. `img_sources` is a space-separated source list (the image
/// proxy and attachment origins, may be empty); `frame_ancestors` is the Orbit app origin that may frame it.
pub fn csp_header(nonce: &str, img_sources: &str, frame_ancestors: &str) -> String {
    let img_sources = csp_sources(img_sources);
    let img_src = if img_sources.is_empty() {
        "data:".to_owned()
    } else {
        format!("{img_sources} data:")
    };
    format!(
        "sandbox allow-scripts allow-popups allow-popups-to-escape-sandbox; default-src 'none'; \
         script-src 'nonce-{}'; img-src {img_src}; style-src 'unsafe-inline'; font-src 'none'; \
         form-action 'none'; base-uri 'none'; frame-ancestors {}",
        clean_nonce(nonce),
        csp_sources(frame_ancestors)
    )
}

fn document(css: &str, body: &str, nonce: &str) -> String {
    let style = if css.is_empty() {
        String::new()
    } else {
        format!("<style>{css}</style>")
    };
    format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\">{style}<script nonce=\"{}\">new ResizeObserver(()=>parent.postMessage({{orbitMailHeight:document.documentElement.scrollHeight}},\"*\")).observe(document.documentElement)</script></head><body>{body}</body></html>",
        clean_nonce(nonce)
    )
}

/// Keeps only base64 characters, so a nonce can neither leave its attribute nor add a CSP directive.
fn clean_nonce(nonce: &str) -> String {
    nonce
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=' | '-' | '_'))
        .collect()
}

/// Removes characters that would end the directive or start another policy.
fn csp_sources(sources: &str) -> String {
    sources
        .split(|c: char| c == ';' || c == ',' || c.is_ascii_whitespace() || c.is_control())
        .filter(|source| !source.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

// ---------------------------------------------------------------- body

fn body_builder(opts: &RenderOptions<'_>, blocked: Arc<AtomicUsize>) -> ammonia::Builder<'static> {
    let mut builder = ammonia::Builder::default();
    builder
        .rm_tags(["area", "map"])
        .add_tags(["font", "center"])
        .clean_content_tags(HashSet::from(["script", "style", "title", "noscript"]))
        .generic_attributes(HashSet::from(["lang", "title", "dir", "class", "style"]))
        .url_schemes(HashSet::from(["http", "https", "mailto", "cid", "data"]))
        .add_tag_attributes("font", ["color", "face", "size"])
        .add_tag_attributes("img", ["border", "hspace", "vspace"])
        .add_tag_attributes(
            "table",
            [
                "width",
                "border",
                "cellpadding",
                "cellspacing",
                "bgcolor",
                "align",
            ],
        )
        .add_tag_attributes("tr", ["bgcolor", "align", "valign"])
        .add_tag_attributes(
            "td",
            ["width", "height", "bgcolor", "align", "valign", "nowrap"],
        )
        .add_tag_attributes(
            "th",
            ["width", "height", "bgcolor", "align", "valign", "nowrap"],
        )
        .add_tag_attributes("div", ["align"])
        .add_tag_attributes("p", ["align"])
        .set_tag_attribute_value("a", "target", "_blank")
        .link_rel(Some("noopener noreferrer"));
    let cid_url = Arc::clone(&opts.cid_url);
    let proxy_url = opts.proxy_url.clone();
    builder.attribute_filter(
        move |element, attribute, value| match (element, attribute) {
            (_, "style") => {
                let css = filter_css(value);
                (!css.is_empty()).then_some(Cow::Owned(css))
            }
            ("img", "src") => {
                image_src(value, &cid_url, proxy_url.as_ref(), &blocked).map(Cow::Owned)
            }
            (_, "href") => is_link(value).then_some(Cow::Borrowed(value)),
            _ => Some(Cow::Borrowed(value)),
        },
    );
    builder
}

fn image_src(
    value: &str,
    cid_url: &CidUrl,
    proxy_url: Option<&ProxyUrl>,
    blocked: &AtomicUsize,
) -> Option<String> {
    let value = value.trim();
    let lower = value.to_ascii_lowercase();
    if let Some(content_id) = strip_prefix_ignore_case(value, "cid:") {
        return cid_url(&percent_decode(content_id));
    }
    if let Some(data) = lower.strip_prefix("data:") {
        let media_type = data.split([';', ',']).next().unwrap_or_default();
        return DATA_IMAGE_TYPES
            .contains(&media_type.trim())
            .then(|| value.to_owned());
    }
    let remote = if lower.starts_with("https://") || lower.starts_with("http://") {
        value.to_owned()
    } else if value.starts_with("//") {
        format!("https:{value}")
    } else {
        return None;
    };
    match proxy_url {
        Some(proxy_url) => Some(proxy_url(&remote)),
        None => {
            blocked.fetch_add(1, Ordering::Relaxed);
            None
        }
    }
}

fn is_link(value: &str) -> bool {
    let lower = value.trim().to_ascii_lowercase();
    lower.starts_with("https://") || lower.starts_with("http://") || lower.starts_with("mailto:")
}

fn strip_prefix_ignore_case<'v>(value: &'v str, prefix: &str) -> Option<&'v str> {
    let head = value.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &value[prefix.len()..])
}

/// RFC 2392: a `cid:` URL holds the URL-encoded content ID.
fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && let Some(byte) = value
                .get(index + 1..index + 3)
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            out.push(byte);
            index += 3;
            continue;
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

// ---------------------------------------------------------------- style sheets

/// The filtered content of every `<style>` element, in order. A second `ammonia` pass that allows only `style`
/// (raw text, no attributes) finds them the way a browser parses them; all other text comes out escaped, so the
/// only `<style>` and `</style>` in its output are the elements' own tags.
fn style_sheets(html: &str) -> String {
    let mut builder = ammonia::Builder::empty();
    builder
        .add_tags(["style"])
        .clean_content_tags(HashSet::from(["script", "title", "noscript"]))
        .tag_attributes(HashMap::new())
        .generic_attributes(HashSet::new())
        .link_rel(None);
    let extracted = builder.clean(html).to_string();
    let mut css = String::new();
    let mut rest = extracted.as_str();
    while let Some(start) = rest.find("<style>") {
        rest = &rest[start + "<style>".len()..];
        let end = rest.find("</style>").unwrap_or(rest.len());
        css.push_str(&rest[..end]);
        css.push('\n');
        rest = &rest[end..];
    }
    if css.len() > MAX_CSS_BYTES {
        return String::new();
    }
    filter_css(css.trim_end())
}

/// Returns `css` without escapes, comments and `<`, and without any rule or declaration that contains a
/// [`CSS_FORBIDDEN`] text. It runs until the text no longer changes, so the result is checked as the browser reads
/// it (removing a comment or an escape can form new text such as `url(`).
pub(crate) fn filter_css(css: &str) -> String {
    if css.len() > MAX_CSS_BYTES {
        return String::new();
    }
    let mut current = css.to_owned();
    for _ in 0..MAX_CSS_PASSES {
        let next = drop_forbidden(&strip_comments(&unescape(&current)));
        if next == current {
            return next;
        }
        current = next;
    }
    String::new()
}

/// Decodes CSS escapes (CSS Syntax §4.3.7) and removes `<` and control characters, so the result has no `\`.
fn unescape(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut chars = css.chars().peekable();
    while let Some(character) = chars.next() {
        if character != '\\' {
            out.push(character);
            continue;
        }
        match chars.peek().copied() {
            None | Some('\n' | '\r' | '\x0c') => {
                chars.next();
            }
            Some(next) if next.is_ascii_hexdigit() => {
                let mut code = 0_u32;
                for _ in 0..6 {
                    match chars.peek().and_then(|c| c.to_digit(16)) {
                        Some(digit) => {
                            code = code * 16 + digit;
                            chars.next();
                        }
                        None => break,
                    }
                }
                if matches!(chars.peek(), Some(' ' | '\t' | '\n' | '\r' | '\x0c')) {
                    chars.next();
                }
                out.push(match char::from_u32(code) {
                    Some(decoded) if code != 0 => decoded,
                    _ => char::REPLACEMENT_CHARACTER,
                });
            }
            Some(next) => {
                out.push(next);
                chars.next();
            }
        }
    }
    out.retain(|c| {
        c != '<' && c != '\\' && (!c.is_control() || matches!(c, '\t' | '\n' | '\r' | '\x0c'))
    });
    out
}

/// Removes `/* … */` comments; an unclosed comment runs to the end.
fn strip_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        match rest[start + 2..].find("*/") {
            Some(end) => rest = &rest[start + 2 + end + 2..],
            None => rest = "",
        }
    }
    out.push_str(rest);
    out
}

/// Splits the CSS at `{`, `}` and `;` and drops every piece that contains a forbidden text; a forbidden rule prelude
/// (`@font-face {`) drops its whole block. The forbidden texts contain none of the split characters, so none can span
/// two pieces.
fn drop_forbidden(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut depth = 0_usize;
    let mut skip_to_depth: Option<usize> = None;
    let mut segment = String::new();
    for character in css.chars() {
        if !matches!(character, '{' | '}' | ';') {
            segment.push(character);
            continue;
        }
        if let Some(target) = skip_to_depth {
            segment.clear();
            match character {
                '{' => depth += 1,
                '}' => {
                    depth = depth.saturating_sub(1);
                    if depth == target {
                        skip_to_depth = None;
                    }
                }
                _ => {}
            }
            continue;
        }
        let allowed = !is_forbidden(&segment);
        match character {
            '{' if allowed => {
                out.push_str(&segment);
                out.push('{');
                depth += 1;
            }
            '{' => {
                skip_to_depth = Some(depth);
                depth += 1;
            }
            '}' => {
                if allowed {
                    out.push_str(&segment);
                }
                if depth > 0 {
                    out.push('}');
                    depth -= 1;
                }
            }
            _ => {
                if allowed {
                    out.push_str(&segment);
                    out.push(';');
                }
            }
        }
        segment.clear();
    }
    if skip_to_depth.is_none() && !is_forbidden(&segment) {
        out.push_str(&segment);
    }
    // Close the blocks that are still open in the output (a skipped block was never written).
    let open = skip_to_depth.unwrap_or(depth);
    out.extend(std::iter::repeat_n('}', open));
    out
}

fn is_forbidden(segment: &str) -> bool {
    let normalized: String = segment
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    CSS_FORBIDDEN
        .iter()
        .any(|needle| normalized.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::filter_css;

    #[test]
    fn keeps_plain_css() {
        assert_eq!(filter_css("p{color:red}"), "p{color:red}");
        assert_eq!(
            filter_css("@media (max-width:600px){.a{color:red;margin:0}}"),
            "@media (max-width:600px){.a{color:red;margin:0}}"
        );
    }

    #[test]
    fn drops_forbidden_declarations_and_blocks() {
        assert_eq!(
            filter_css("a{color:red;background:url(x)}"),
            "a{color:red;}"
        );
        assert_eq!(
            filter_css("@font-face{src:url(x)}p{color:red}"),
            "p{color:red}"
        );
        assert_eq!(filter_css("@import 'x';p{}"), "p{}");
        assert_eq!(
            filter_css("@media print{@font-face{a:b}}q{}"),
            "@media print{}q{}"
        );
    }

    #[test]
    fn decodes_escapes_before_checking() {
        assert_eq!(filter_css("a{b:\\75 rl(x)}"), "a{}");
        assert_eq!(filter_css("a{b:u\\rl(x)}"), "a{}");
        assert_eq!(
            filter_css("a{content:'\\3c/style>'}"),
            "a{content:'/style>'}"
        );
        assert_eq!(filter_css("a{b:ur/**/l(x)}"), "a{}");
        // A browser reads `/*/**/` as one comment, as the filter does.
        assert_eq!(filter_css("a{b:ur/*/**/*/l(x)}"), "a{b:ur*/l(x)}");
    }

    #[test]
    fn closes_open_blocks_and_ignores_stray_braces() {
        assert_eq!(filter_css("}a{color:red"), "a{color:red}");
        assert_eq!(filter_css("@font-face{src:x"), "");
        assert_eq!(filter_css("@media x{@font-face{src:x"), "@media x{}");
    }
}
