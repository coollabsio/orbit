//! The HTML view document: mail HTML sanitized on each view (no HTML is stored), wrapped in a full document with
//! Orbit's base style sheet and nonce frame script (height, theme, quote folding), and the CSP header that goes
//! with it (spec §6.5).
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
    let plain = is_plain(&css, &body);
    let blocked = blocked.load(Ordering::Relaxed);
    Rendered {
        html: document(&css, &body, opts.nonce, plain, blocked),
        blocked_remote_images: blocked,
    }
}

/// The view document for a message without an HTML part: `text` escaped in a `<pre>`. A trailing quote (the lines
/// that start with `>`, with the "… wrote:" line before them) goes into a `.orbit-quote-text` block that the frame
/// script folds. The text document is always plain: it follows the app theme.
pub fn render_text_document(text: &str, nonce: &str) -> String {
    let mut body = String::with_capacity(text.len() + 64);
    body.push_str("<pre>");
    match text_quote_start(text) {
        Some(start) => {
            push_escaped(&mut body, text[..start].trim_end());
            body.push_str("<div class=\"orbit-quote-text\">");
            push_escaped(&mut body, text[start..].trim_end());
            body.push_str("</div>");
        }
        None => push_escaped(&mut body, text),
    }
    body.push_str("</pre>");
    document(TEXT_CSS, &body, nonce, true, 0)
}

/// The byte offset where the trailing quote of a plain-text message starts, or `None` when the text does not end
/// with `>` lines or nothing but the quote would stay visible.
fn text_quote_start(text: &str) -> Option<usize> {
    let mut lines = Vec::new();
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        lines.push((offset, line));
        offset += line.len();
    }
    let blank = |line: &str| line.trim().is_empty();
    let quoted = |line: &str| line.trim_start().starts_with('>');
    let mut end = lines.len();
    while end > 0 && blank(lines[end - 1].1) {
        end -= 1;
    }
    let mut start = end;
    while start > 0 && (quoted(lines[start - 1].1) || blank(lines[start - 1].1)) {
        start -= 1;
    }
    while start < end && blank(lines[start].1) {
        start += 1;
    }
    if start == end {
        return None;
    }
    // "On …, Ann wrote:" in any language: a short line that ends with a colon, blank lines between allowed
    let mut intro = start;
    while intro > 0 && blank(lines[intro - 1].1) {
        intro -= 1;
    }
    let mut fold = start;
    if intro > 0 {
        let line = lines[intro - 1].1.trim();
        if line.ends_with(':') && line.chars().count() < 200 {
            fold = intro - 1;
        }
    }
    let visible = lines[..fold].iter().any(|(_, line)| !blank(line));
    visible.then(|| lines[fold].0)
}

fn push_escaped(out: &mut String, text: &str) {
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            other => out.push(other),
        }
    }
}

/// A document is plain when the mail brings no colours of its own: no style sheet and no `style`, `bgcolor` or
/// `color` attribute in the sanitized body. A plain document follows the app theme; any other mail keeps the white
/// page it was written for. The body is `ammonia`'s serialization, so every attribute is written as ` name="…"`
/// and a `"` inside a value is `&quot;`; a match in the text only makes the document not plain, which is safe.
fn is_plain(css: &str, body: &str) -> bool {
    css.trim().is_empty()
        && ![" style=\"", " bgcolor=\"", " color=\""]
            .iter()
            .any(|attribute| body.contains(attribute))
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

/// Orbit's base style sheet. It comes before the mail's own CSS and uses low-specificity selectors, so the mail's
/// CSS wins. It has no `"`, nor has [`FRAME_SCRIPT`].
const BASE_CSS: &str = "body{margin:0;padding:12px 16px;\
font:14px/1.55 system-ui,-apple-system,'Segoe UI',Roboto,sans-serif;color:#1f2328;overflow-wrap:anywhere}\
img{max-width:100%;height:auto}\
blockquote{margin:8px 0;padding-left:12px;border-left:2px solid #d0d7de;color:#59636e}\
pre{white-space:pre-wrap}a{color:#0969da}[hidden]{display:none!important}\
.orbit-fold{display:block;height:22px;margin:8px 0;padding:0 10px;border:1px solid #d0d7de;border-radius:999px;\
background:#f6f8fa;color:#59636e;font:12px/1 system-ui,sans-serif;letter-spacing:1px;cursor:pointer}\
.orbit-fold:hover{background:#eaeef2}";
/// The app's dark theme, only in plain documents (the frame script sets `data-theme`). The page colour is a gradient
/// over an opaque base, so a see-through or bad `--orbit-bg` never lets a white backdrop show under the light text.
const DARK_CSS: &str = "html[data-theme=dark]{background-color:#0a0a0a;\
background-image:linear-gradient(var(--orbit-bg,#0a0a0a),var(--orbit-bg,#0a0a0a));color-scheme:dark}\
html[data-theme=dark] body{color:#e4e4e7}\
html[data-theme=dark] blockquote{border-left-color:#3f3f46;color:#a1a1aa}\
html[data-theme=dark] a{color:#8ab4f8}\
html[data-theme=dark] .orbit-fold{border-color:#3f3f46;background:#27272a;color:#a1a1aa}\
html[data-theme=dark] .orbit-fold:hover{background:#3f3f46}";
/// The plain-text document: the same sans font as HTML mail, not monospace.
const TEXT_CSS: &str = "pre{margin:0;font:inherit}";
/// The frame's one script, at the end of the body. It folds the quoted part of a reply behind a "•••" button (the
/// first `.orbit-quote-intro`, `.gmail_quote`, `blockquote.orbit-quote` or `.orbit-quote-text`, else a trailing
/// `blockquote` with its "… wrote:" line; never when nothing is visible before it), takes the app theme
/// (`{orbitMailTheme, orbitMailBackground}` from the parent only, `dark` or `light` and a colour), and posts
/// `{orbitMailHeight, orbitMailPlain, orbitMailBlocked, orbitMailTheme}` on every size change and after each theme
/// message: `orbitMailBlocked` is the server's count from `data-orbit-blocked`, and the theme is the one it shows, so
/// the app keeps a plain frame hidden until it is in the app theme (no white flash). The height is the content's
/// (`documentElement.scrollHeight` never drops below the frame's own height), so the frame shrinks again when the
/// quote folds. Its own `<script>` element (the body's last child) never counts as visible text.
const FRAME_SCRIPT: &str = r#"(()=>{const d=document,e=d.documentElement,b=d.body,p=e.hasAttribute('data-orbit-plain'),m=n=>n.nodeType==3?/\S/.test(n.data):n.nodeType==1&&n.nodeName!='SCRIPT'&&(/\S/.test(n.textContent)||n.matches('img')||!!n.querySelector('img')),z=n=>{while(n&&!m(n))n=n.previousSibling;return n};let s=b.querySelector('.orbit-quote-intro,.gmail_quote,blockquote.orbit-quote,.orbit-quote-text'),c=b,k;if(!s){while((k=z(c.lastChild))&&k.nodeName=='DIV')c=k;if(k&&k.nodeName=='BLOCKQUOTE'){s=k;const q=z(k.previousSibling),x=q?q.textContent.trim():'';if(x.length<200&&x.endsWith(':'))s=q}}if(s){let h=[],v=0;for(let a=s,o=s;a!=b;a=a.parentNode,o=a.nextSibling){for(let r=a.previousSibling;r;r=r.previousSibling)v|=m(r);for(;o;o=o.nextSibling)h.push(o)}if(v){h=h.flatMap(n=>{if(n.nodeType==1)return[n];if(!m(n))return[];const w=d.createElement('span');n.before(w);w.append(n);return[w]});const t=d.createElement('button'),f=x=>{t.setAttribute('aria-expanded',x);t.setAttribute('aria-label',(x?'Hide':'Show')+' quoted text');for(const n of h)n.hidden=!x};t.type='button';t.className='orbit-fold';t.textContent='\u2022\u2022\u2022';f(false);t.addEventListener('click',()=>f(t.getAttribute('aria-expanded')!='true'));h[0].before(t)}}const P=()=>parent.postMessage({orbitMailHeight:Math.max(e.offsetHeight,b.scrollHeight),orbitMailPlain:p,orbitMailBlocked:+(e.dataset.orbitBlocked||0),orbitMailTheme:e.dataset.theme||null},'*');addEventListener('message',ev=>{const g=ev.source==parent&&ev.data,t=g&&g.orbitMailTheme,c=g&&g.orbitMailBackground;if(t=='dark'||t=='light')e.dataset.theme=t;if(typeof c=='string'&&/^(#[0-9a-f]{3,8}|(rgba?|oklch|oklab|hsla?|lab|lch|color)\([0-9a-z.,%/ -]{1,80}\))$/i.test(c))e.style.setProperty('--orbit-bg',c);if(g)P()});const o=new ResizeObserver(P);o.observe(e);o.observe(b)})()"#;

/// The full view document. The script goes at the end of the body, after the sanitized mail, because it needs the
/// mail's DOM to fold the quote. `blocked` (the remote images the view removed) goes into `data-orbit-blocked` on the
/// `<html>` element, for the script to post.
fn document(css: &str, body: &str, nonce: &str, plain: bool, blocked: usize) -> String {
    let mut out = String::with_capacity(
        body.len() + css.len() + BASE_CSS.len() + DARK_CSS.len() + FRAME_SCRIPT.len() + 192,
    );
    out.push_str("<!DOCTYPE html><html");
    if plain {
        out.push_str(" data-orbit-plain");
    }
    if blocked > 0 {
        out.push_str(&format!(" data-orbit-blocked=\"{blocked}\""));
    }
    out.push_str("><head><meta charset=\"utf-8\"><style>");
    out.push_str(BASE_CSS);
    if plain {
        out.push_str(DARK_CSS);
    }
    out.push_str("</style>");
    if !css.is_empty() {
        out.push_str("<style>");
        out.push_str(css);
        out.push_str("</style>");
    }
    out.push_str("</head><body>");
    out.push_str(body);
    out.push_str("<script nonce=\"");
    out.push_str(&clean_nonce(nonce));
    out.push_str("\">");
    out.push_str(FRAME_SCRIPT);
    out.push_str("</script></body></html>");
    out
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
