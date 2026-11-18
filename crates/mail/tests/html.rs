use std::sync::Arc;

use orbit_mail::{RenderOptions, Rendered, csp_header, render_document, render_text_document};

const NONCE: &str = "r4nd0mN0nce";

fn options(proxy: bool) -> RenderOptions<'static> {
    RenderOptions {
        cid_url: Arc::new(|cid: &str| {
            (cid == "logo@example.com").then(|| "https://orbit.test/att/1?inline=1".to_owned())
        }),
        proxy_url: proxy.then(|| {
            Arc::new(|url: &str| format!("https://orbit.test/image?u={}&sig=s", url.len()))
                as Arc<dyn Fn(&str) -> String + Send + Sync>
        }),
        nonce: NONCE,
    }
}

fn render(html: &str) -> Rendered {
    render_document(html, &options(false))
}

/// The document without Orbit's own parts (the base style sheet and the frame script), so assertions see only what
/// came from the mail.
fn mail_part(html: &str) -> String {
    let rendered = render(html).html;
    let script = frame_script(&rendered).to_owned();
    let base = base_style(&rendered).to_owned();
    rendered.replace(&script, "").replacen(&base, "", 1)
}

/// Orbit's frame script element: exactly one, with the nonce, at the end of the body.
fn frame_script(document: &str) -> &str {
    let open = format!("<script nonce=\"{NONCE}\">");
    assert_eq!(document.matches(&open).count(), 1, "{document}");
    let start = document.find(&open).unwrap();
    let end = start + document[start..].find("</script>").unwrap() + "</script>".len();
    assert_eq!(&document[end..], "</body></html>", "{document}");
    &document[start..end]
}

/// Orbit's base style sheet: the first `<style>` element, right after the charset.
fn base_style(document: &str) -> &str {
    let start = document.find("<style>").unwrap();
    assert!(
        document[..start].ends_with("<head><meta charset=\"utf-8\">"),
        "{document}"
    );
    let end = start + document[start..].find("</style>").unwrap() + "</style>".len();
    &document[start..end]
}

/// The `<pre>` of a text document.
fn text_body(text: &str) -> String {
    let out = render_text_document(text, NONCE);
    let start = out.find("<body>").unwrap() + "<body>".len();
    let end = out.find("<script nonce=").unwrap();
    out[start..end].to_owned()
}

fn assert_absent(html: &str, needles: &[&str]) {
    let out = mail_part(html);
    let lower = out.to_ascii_lowercase();
    for needle in needles {
        assert!(
            !lower.contains(&needle.to_ascii_lowercase()),
            "{needle:?} survived in {out}"
        );
    }
}

// ---------------------------------------------------------------- document

#[test]
fn document_has_the_frame_script_with_the_nonce() {
    let rendered = render("<p>Hello</p>");
    assert!(
        rendered.html.starts_with("<!DOCTYPE html>"),
        "{}",
        rendered.html
    );
    assert!(
        rendered
            .html
            .contains("<body><p>Hello</p><script nonce=\"r4nd0mN0nce\">"),
        "{}",
        rendered.html
    );
    let script = frame_script(&rendered.html);
    for needle in [
        "orbitMailHeight:",
        "orbitMailPlain:p",
        "ev.source==parent",
        "orbitMailTheme",
        "t=='dark'||t=='light'",
        "e.dataset.theme=t",
        "'.orbit-quote-intro,.gmail_quote,blockquote.orbit-quote,.orbit-quote-text'",
        "createElement('button')",
        "t.type='button'",
        "'aria-expanded'",
        "' quoted text'",
        "addEventListener('click'",
        "new ResizeObserver(",
        "orbitMailBlocked:b.querySelectorAll('img:not([src])').length",
        "orbitMailTheme:e.dataset.theme||null",
        "if(g)P()",
    ] {
        assert!(script.contains(needle), "{needle:?} missing in {script}");
    }
    // one element: no quote can end an attribute, no handler attribute, no network load
    let inner = &script[script.find('>').unwrap() + 1..script.len() - "</script>".len()];
    // (`img:not([src])` only counts the images the view removed; setting a source would load one)
    for needle in [
        "\"", "</", "<!--", "onclick", "fetch(", ".src", "src=", "import",
    ] {
        assert!(!inner.contains(needle), "{needle:?} in {inner}");
    }
    assert_eq!(rendered.blocked_remote_images, 0);
}

#[test]
fn base_style_sheet_comes_before_the_mail_css() {
    let out = render("<style>body{color:red;font-family:Georgia}</style><p>x</p>").html;
    let base = base_style(&out);
    for needle in [
        "body{margin:0;padding:12px 16px;font:14px/1.55 system-ui,-apple-system,'Segoe UI',Roboto,sans-serif;",
        "overflow-wrap:anywhere",
        "img{max-width:100%;height:auto}",
        "blockquote{margin:8px 0;padding-left:12px;border-left:2px solid",
        "pre{white-space:pre-wrap}",
        "a{color:",
        "[hidden]{display:none!important}",
        ".orbit-fold{display:block;height:22px;",
        "border-radius:999px",
    ] {
        assert!(base.contains(needle), "{needle:?} missing in {base}");
    }
    let base_at = out.find(base).unwrap();
    let mail_at = out
        .find("<style>body{color:red;font-family:Georgia}</style>")
        .unwrap();
    assert!(
        base_at < mail_at && mail_at < out.find("</head>").unwrap(),
        "{out}"
    );
}

#[test]
fn mail_without_colours_is_plain_and_follows_the_theme() {
    for html in [
        "<p>Hello <b>there</b></p><blockquote>quote</blockquote>",
        "<div class=\"x\"><font face=\"Arial\">Hi</font></div>",
        // the only CSS is dropped by the filter
        "<style>@import url(https://evil.test/a.css);</style><p>x</p>",
    ] {
        let out = render(html).html;
        assert!(
            out.starts_with("<!DOCTYPE html><html data-orbit-plain><head>"),
            "{out}"
        );
        assert!(
            base_style(&out).contains("html[data-theme=dark] body{color:"),
            "{out}"
        );
        assert!(
            base_style(&out).contains(
                "html[data-theme=dark]{background:var(--orbit-bg,#0a0a0a);color-scheme:dark}"
            ),
            "{out}"
        );
    }
    for html in [
        "<style>p{margin:0}</style><p>x</p>",
        "<p style=\"color:#333\">x</p>",
        "<p style=\"padding:4px\">x</p>",
        "<table bgcolor=\"#ffffff\"><tr><td>x</td></tr></table>",
        "<font color=\"#333\">x</font>",
    ] {
        let out = render(html).html;
        assert!(
            out.starts_with("<!DOCTYPE html><html><head>"),
            "{html}: {out}"
        );
        // no dark colours for mail that brings its own
        assert!(!base_style(&out).contains("data-theme"), "{html}: {out}");
    }
}

#[test]
fn text_document_is_plain_with_the_sans_font() {
    let out = render_text_document("Hello", NONCE);
    assert!(
        out.starts_with("<!DOCTYPE html><html data-orbit-plain><head>"),
        "{out}"
    );
    assert!(base_style(&out).contains("html[data-theme=dark]"), "{out}");
    assert!(
        out.contains("</style><style>pre{margin:0;font:inherit}</style></head>"),
        "{out}"
    );
    assert!(!out.contains("monospace"), "{out}");
    assert_eq!(text_body("Hello"), "<pre>Hello</pre>");
}

#[test]
fn text_document_folds_the_trailing_quote() {
    assert_eq!(
        text_body("Thanks!\n\nOn Mon, Ann <ann@x.test> wrote:\n> hi <b>\n>\n> & bye\n\n"),
        "<pre>Thanks!<div class=\"orbit-quote-text\">On Mon, Ann &lt;ann@x.test&gt; wrote:\n\
         &gt; hi &lt;b&gt;\n&gt;\n&gt; &amp; bye</div></pre>"
    );
    // any language, a blank line between the intro and the quote
    assert_eq!(
        text_body("Merci\nLe lun., Ann a écrit :\n\n> salut\n"),
        "<pre>Merci<div class=\"orbit-quote-text\">Le lun., Ann a écrit :\n\n&gt; salut</div></pre>"
    );
    // no intro line: only the `>` lines fold
    assert_eq!(
        text_body("Yes.\n  > a\n> b"),
        "<pre>Yes.<div class=\"orbit-quote-text\">  &gt; a\n&gt; b</div></pre>"
    );
    // a line of 200 characters is not an intro
    let long = format!("{}:", "x".repeat(199));
    assert_eq!(
        text_body(&format!("Hi\n{long}\n> a")),
        format!("<pre>Hi\n{long}<div class=\"orbit-quote-text\">&gt; a</div></pre>")
    );
}

#[test]
fn text_document_does_not_fold_without_a_trailing_quote_or_other_text() {
    for text in [
        "> a\n> b",
        "\n  \nOn Mon, Ann wrote:\n> a\n",
        "> quoted\nmy answer below the quote",
        "no quote at all:",
        "",
    ] {
        let body = text_body(text);
        assert!(!body.contains("orbit-quote-text"), "{text:?}: {body}");
    }
}

#[test]
fn text_document_quote_cannot_break_out_of_the_fold() {
    let body = text_body(
        "hi\n> </div></pre><script>alert(1)</script>\n> <div class=\"orbit-quote-text\">",
    );
    assert_eq!(
        body,
        "<pre>hi<div class=\"orbit-quote-text\">&gt; &lt;/div&gt;&lt;/pre&gt;&lt;script&gt;alert(1)&lt;/script&gt;\n\
         &gt; &lt;div class=\"orbit-quote-text\"&gt;</div></pre>"
    );
}

#[test]
fn mail_cannot_add_a_script_with_the_nonce() {
    let html = format!(
        "<script nonce=\"{NONCE}\">alert(1)</script><p nonce=\"{NONCE}\">x</p>\
         <scr<script>ipt>alert(2)</script><noscript><script nonce=\"{NONCE}\">alert(3)</script></noscript>"
    );
    let out = render(&html).html;
    assert_eq!(out.matches("<script").count(), 1, "{out}");
    assert_eq!(out.matches("nonce").count(), 1, "{out}");
    assert!(
        !out.contains("alert(1)") && !out.contains("alert(3)"),
        "{out}"
    );
}

#[test]
fn a_bad_nonce_cannot_break_out_of_the_attribute() {
    let opts = RenderOptions {
        nonce: "abc\"><img src=x onerror=alert(1)>",
        ..options(false)
    };
    let out = render_document("<p>x</p>", &opts).html;
    assert!(
        out.contains("<script nonce=\"abcimgsrc=xonerror=alert1\">"),
        "{out}"
    );
    assert_eq!(out.matches("<img").count(), 0, "{out}");
    // the charset and the nonce attribute; the base style sheet and the script have none
    assert_eq!(out.matches('"').count(), 4, "{out}");
}

#[test]
fn text_document_escapes_the_body() {
    let out = render_text_document("a <b>bold</b> & <script>alert(1)</script>", NONCE);
    assert!(
        out.contains(
            "<pre>a &lt;b&gt;bold&lt;/b&gt; &amp; &lt;script&gt;alert(1)&lt;/script&gt;</pre>"
        ),
        "{out}"
    );
    frame_script(&out);
    assert_eq!(out.matches("<script").count(), 1);
}

// ---------------------------------------------------------------- spec §11 attack strings

#[test]
fn scripts_are_removed() {
    assert_absent(
        "<script>alert(1)</script><SCRIPT SRC=//evil.test/x.js></SCRIPT><p>ok</p>",
        &["<script", "alert(1)", "evil.test"],
    );
}

#[test]
fn event_handlers_are_removed() {
    assert_absent(
        "<img src=x onerror=alert(1)><p onclick=\"alert(2)\" ONMOUSEOVER=alert(3)>x</p><body onload=alert(4)>",
        &["onerror", "onclick", "onmouseover", "onload", "alert("],
    );
}

#[test]
fn javascript_links_are_removed() {
    assert_absent(
        "<a href=\"javascript:alert(1)\">a</a><a href=\" JaVaScRiPt:alert(2)\">b</a>\
         <a href=\"java&#x09;script:alert(3)\">c</a><a href=\"data:text/html,<script>alert(4)</script>\">d</a>\
         <a href=\"vbscript:x\">e</a>",
        &["javascript", "alert(", "data:", "vbscript"],
    );
}

#[test]
fn css_url_is_removed_in_style_elements() {
    let out = mail_part(
        "<style>.a{color:red;background:url(https://evil.test/1.png)}\
         .b{background-image:URL( \"https://evil.test/2.png\" )}\
         .c{background:u\\rl(https://evil.test/3.png)}\
         .d{background:\\75 rl(https://evil.test/4.png)}\
         .e{background:ur/**/l(https://evil.test/5.png)}\
         .f{background:image-set(\"https://evil.test/6.png\" 1x)}\
         .g{width:expression(alert(1))}\
         .h{behavior:url(x.htc);-moz-binding:url(x.xml#y)}\
         .i{background:src(\"https://evil.test/7.png\")}</style><p class=a>x</p>",
    );
    assert!(out.contains(".a{color:red;"), "{out}");
    for needle in [
        "evil.test",
        "url",
        "expression",
        "behavior",
        "binding",
        "image-set",
        "src(",
    ] {
        assert!(
            !out.to_ascii_lowercase().contains(needle),
            "{needle:?} survived in {out}"
        );
    }
}

#[test]
fn css_url_is_removed_in_style_attributes() {
    let out = mail_part(
        "<p style=\"color:blue;background:url(https://evil.test/1.png)\">a</p>\
         <p style=\"background:u&#114;l(https://evil.test/2.png)\">b</p>\
         <p style=\"background:u\\rl(https://evil.test/3.png)\">c</p>\
         <p style=\"background:image-set('https://evil.test/4.png' 1x)\">d</p>",
    );
    assert!(out.contains("style=\"color:blue;"), "{out}");
    assert!(
        !out.contains("evil.test") && !out.to_ascii_lowercase().contains("url"),
        "{out}"
    );
}

#[test]
fn css_import_and_font_face_are_removed() {
    let out = mail_part(
        "<style>@import url(https://evil.test/a.css);@IMPORT \"https://evil.test/b.css\";\
         @\\69mport 'https://evil.test/c.css';\
         @font-face{font-family:x;src:url(https://evil.test/f.woff)}\
         @media (max-width:600px){.m{color:green}}\
         p{color:red}</style><p>x</p>",
    );
    assert!(out.contains("p{color:red}"), "{out}");
    assert!(
        out.contains("@media (max-width:600px){.m{color:green}}"),
        "{out}"
    );
    for needle in ["evil.test", "@import", "@font-face", "font-family:x"] {
        assert!(
            !out.to_ascii_lowercase().contains(needle),
            "{needle:?} survived in {out}"
        );
    }
}

#[test]
fn style_element_cannot_close_itself_after_filtering() {
    let out = mail_part("<style>a{}<\\/style><script>alert(1)</script><!-- --></style><p>x</p>");
    assert_eq!(out.matches("</style>").count(), 1, "{out}");
    assert!(!out.contains("<script"), "{out}");
}

#[test]
fn meta_refresh_base_and_link_are_removed() {
    assert_absent(
        "<meta http-equiv=\"refresh\" content=\"0;url=https://evil.test\"><base href=\"https://evil.test/\">\
         <link rel=stylesheet href=\"https://evil.test/a.css\"><p>x</p>",
        &["<meta http-equiv", "refresh", "<base", "<link", "evil.test"],
    );
}

#[test]
fn forms_and_embedded_content_are_removed() {
    assert_absent(
        "<form action=\"https://evil.test/post\"><input name=p type=password><button formaction=x>Go</button>\
         <textarea>t</textarea><select><option>o</option></select></form>\
         <iframe src=\"https://evil.test/f\"></iframe><object data=\"https://evil.test/o\"></object>\
         <embed src=\"https://evil.test/e\">",
        &[
            "<form",
            "<input",
            "<button",
            "<textarea",
            "<select",
            "<iframe",
            "<object",
            "<embed",
            "evil.test",
        ],
    );
}

#[test]
fn svg_and_math_are_removed() {
    assert_absent(
        "<svg><script>alert(1)</script><a xlink:href=\"javascript:alert(2)\"><text>t</text></a>\
         <style><img src=x onerror=alert(3)></style></svg>\
         <math><mtext><table><mglyph><style><img src=x onerror=alert(4)></style></mglyph></table></mtext></math>",
        &[
            "<svg",
            "<math",
            "<script",
            "javascript",
            "onerror",
            "alert(",
        ],
    );
}

#[test]
fn srcset_and_background_attributes_are_removed() {
    assert_absent(
        "<img src=\"cid:logo@example.com\" srcset=\"https://evil.test/a.png 2x\">\
         <picture><source srcset=\"https://evil.test/b.png\"></picture>\
         <table background=\"https://evil.test/c.png\"><tr><td background=\"https://evil.test/d.png\">x</td></tr></table>\
         <body background=\"https://evil.test/e.png\">",
        &["srcset", "background", "evil.test", "<source"],
    );
}

// ---------------------------------------------------------------- images and links

#[test]
fn cid_images_are_rewritten() {
    let out = mail_part(
        "<img src=\"cid:logo@example.com\" alt=Logo><img src=\"CID:logo%40example.com\"><img src=\"cid:unknown@x\">",
    );
    assert_eq!(
        out.matches("src=\"https://orbit.test/att/1?inline=1\"")
            .count(),
        2,
        "{out}"
    );
    assert!(!out.contains("cid:"), "{out}");
    assert_eq!(out.matches("<img").count(), 3, "{out}");
}

#[test]
fn remote_images_are_removed_and_counted_without_a_proxy() {
    let rendered = render(
        "<img src=\"https://tracker.test/p.gif\"><img src=\"http://tracker.test/q.gif\">\
         <img src=\"//tracker.test/r.gif\"><img src=\"/local.png\"><img src=\"cid:logo@example.com\">",
    );
    assert!(!rendered.html.contains("tracker.test"), "{}", rendered.html);
    assert!(!rendered.html.contains("local.png"), "{}", rendered.html);
    assert_eq!(rendered.blocked_remote_images, 3);
}

#[test]
fn remote_images_go_through_the_proxy() {
    let rendered = render_document(
        "<img src=\"https://tracker.test/p.gif\"><img src=\"cid:logo@example.com\">",
        &options(true),
    );
    assert!(
        rendered
            .html
            .contains("src=\"https://orbit.test/image?u=26&amp;sig=s\""),
        "{}",
        rendered.html
    );
    assert!(!rendered.html.contains("tracker.test"), "{}", rendered.html);
    assert_eq!(rendered.blocked_remote_images, 0);
}

#[test]
fn data_images_keep_only_allowed_types() {
    let out = mail_part(
        "<img src=\"data:image/png;base64,iVBORw0KGgo=\"><img src=\"data:image/svg+xml;base64,PHN2Zz4=\">\
         <img src=\"data:text/html;base64,PHNjcmlwdD4=\">",
    );
    assert!(
        out.contains("src=\"data:image/png;base64,iVBORw0KGgo=\""),
        "{out}"
    );
    assert!(!out.contains("svg") && !out.contains("text/html"), "{out}");
}

#[test]
fn links_open_in_a_new_tab_without_referrer() {
    let out = mail_part(
        "<a href=\"https://example.com/a\" target=\"_self\" rel=\"opener\">a</a><a href=\"mailto:ann@example.com\">m</a>\
         <a href=\"/relative\">r</a>",
    );
    assert!(out.contains("href=\"https://example.com/a\""), "{out}");
    assert!(out.contains("href=\"mailto:ann@example.com\""), "{out}");
    assert!(!out.contains("relative\""), "{out}");
    assert!(
        !out.contains("_self") && !out.contains("\"opener\""),
        "{out}"
    );
    assert_eq!(out.matches("target=\"_blank\"").count(), 3, "{out}");
    assert_eq!(
        out.matches("rel=\"noopener noreferrer\"").count(),
        3,
        "{out}"
    );
}

#[test]
fn common_mail_layout_survives() {
    let out = mail_part(
        "<table width=\"600\" cellpadding=\"0\" cellspacing=\"0\" border=\"0\" bgcolor=\"#ffffff\" align=\"center\">\
         <tr><td valign=\"top\" class=\"hero\" style=\"padding:8px\"><font color=\"#333\" face=\"Arial\">Hi</font></td></tr></table>",
    );
    for needle in [
        "width=\"600\"",
        "cellpadding=\"0\"",
        "bgcolor=\"#ffffff\"",
        "valign=\"top\"",
        "class=\"hero\"",
        "style=\"padding:8px\"",
        "<font color=\"#333\" face=\"Arial\">",
    ] {
        assert!(out.contains(needle), "{needle:?} missing in {out}");
    }
}

#[test]
fn head_title_does_not_leak_into_the_body() {
    let out =
        mail_part("<html><head><title>Secret title</title></head><body><p>Body</p></body></html>");
    assert!(!out.contains("Secret title"), "{out}");
    assert!(out.contains("<p>Body</p>"), "{out}");
}

// ---------------------------------------------------------------- CSP

#[test]
fn csp_header_is_exact() {
    assert_eq!(
        csp_header(NONCE, "https://orbit.test", "https://app.orbit.test"),
        "sandbox allow-scripts allow-popups allow-popups-to-escape-sandbox; default-src 'none'; \
         script-src 'nonce-r4nd0mN0nce'; img-src https://orbit.test data:; style-src 'unsafe-inline'; \
         font-src 'none'; form-action 'none'; base-uri 'none'; frame-ancestors https://app.orbit.test"
    );
}

#[test]
fn csp_header_refuses_directive_injection() {
    let csp = csp_header("n;x", "https://a.test; script-src *", "https://b.test, *");
    assert_eq!(csp.matches(';').count(), 8, "{csp}");
    assert!(!csp.contains(','), "{csp}");
    assert!(csp.contains("script-src 'nonce-nx'"), "{csp}");
}

#[test]
fn csp_header_without_image_sources_allows_only_data() {
    assert!(csp_header(NONCE, "", "'self'").contains("; img-src data:; "));
}
