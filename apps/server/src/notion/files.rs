//! Which file URLs the import may download, and how downloaded files are named.
//!
//! Only Notion-hosted files (`type: "file"`) are downloaded. `external` file URLs are never
//! fetched by the server (they stay links), so a Notion page cannot make Orbit request an
//! arbitrary host (SSRF).
//!
//! Allowlist (HTTPS on the default port only, no credentials in the URL):
//! - `file.notion.so`: Notion's signed file proxy; it redirects to the S3 bucket below.
//! - `prod-files-secure.s3.us-west-2.amazonaws.com`: the bucket behind Notion-hosted uploads.
//! - `s3.us-west-2.amazonaws.com` / `s3-us-west-2.amazonaws.com` path-style URLs, only when the
//!   first path segment is a Notion bucket (`secure.notion-static.com`,
//!   `public.notion-static.com`, `prod-files-secure`); this is the form in Notion's file-object
//!   docs. Other buckets on the same S3 host belong to anyone, so the bucket is checked.
//! - `notion-static.com` and its subdomains (Notion's own static domain).

use reqwest::Url;

const EXACT_HOSTS: &[&str] = &[
    "file.notion.so",
    "prod-files-secure.s3.us-west-2.amazonaws.com",
    "notion-static.com",
];
const OWNED_DOMAIN_SUFFIXES: &[&str] = &[".notion-static.com"];
const PATH_STYLE_S3_HOSTS: &[&str] = &["s3.us-west-2.amazonaws.com", "s3-us-west-2.amazonaws.com"];
const NOTION_BUCKETS: &[&str] = &[
    "secure.notion-static.com",
    "public.notion-static.com",
    "prod-files-secure",
];

/// True when `url` is a Notion-hosted file URL the import may download.
#[must_use]
pub fn is_allowed_file_url(url: &str) -> bool {
    Url::parse(url).is_ok_and(|url| is_allowed_notion_file(&url))
}

pub(crate) fn is_allowed_notion_file(url: &Url) -> bool {
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some_and(|port| port != 443)
    {
        return false;
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    if EXACT_HOSTS.contains(&host.as_str())
        || OWNED_DOMAIN_SUFFIXES
            .iter()
            .any(|suffix| host.ends_with(suffix))
    {
        return true;
    }
    if PATH_STYLE_S3_HOSTS.contains(&host.as_str()) {
        let bucket = url
            .path_segments()
            .and_then(|mut segments| segments.next())
            .unwrap_or_default();
        return NOTION_BUCKETS.contains(&bucket);
    }
    false
}

/// A safe file name for a downloaded file: `downloadName` query parameter (file.notion.so),
/// else the last path segment, percent-decoded and stripped of path separators and control
/// characters. Falls back to `file`.
#[must_use]
pub fn suggested_file_name(url: &Url) -> String {
    let from_query = url
        .query_pairs()
        .find(|(key, _)| key == "downloadName")
        .map(|(_, value)| value.into_owned());
    let from_path = || {
        url.path_segments()
            .and_then(|mut segments| segments.next_back())
            .map(percent_decode)
    };
    sanitize_file_name(&from_query.or_else(from_path).unwrap_or_default())
}

/// Removes path separators and control characters, trims dots/spaces, caps the length.
#[must_use]
pub fn sanitize_file_name(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if c == '/' || c == '\\' || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches(|c: char| c == '.' || c.is_whitespace());
    if trimmed.is_empty() {
        return "file".to_owned();
    }
    trimmed.chars().take(200).collect()
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Some(high), Some(low)) =
                (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
        {
            out.push(high * 16 + low);
            index += 3;
            continue;
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_notion_hosted_files() {
        for url in [
            "https://prod-files-secure.s3.us-west-2.amazonaws.com/ws/id/image.png?X-Amz-Signature=abc",
            "https://s3.us-west-2.amazonaws.com/secure.notion-static.com/9bc6c6e0/brocolli.jpeg?x=1",
            "https://s3-us-west-2.amazonaws.com/public.notion-static.com/865e85fc/3c67.png",
            "https://file.notion.so/f/f/ws/id/doc.pdf?downloadName=doc.pdf",
            "https://secure.notion-static.com/e6a352a8.jpg",
            "https://FILE.NOTION.SO:443/f/x",
        ] {
            assert!(is_allowed_file_url(url), "{url}");
        }
    }

    #[test]
    fn rejects_other_hosts_schemes_and_tricks() {
        for url in [
            "http://prod-files-secure.s3.us-west-2.amazonaws.com/ws/id/image.png",
            "https://s3.us-west-2.amazonaws.com/attacker-bucket/image.png",
            "https://attacker.s3.us-west-2.amazonaws.com/image.png",
            "https://example.com/image.png",
            "https://file.notion.so.evil.com/f/x",
            "https://evilnotion-static.com/x",
            "https://user:pass@file.notion.so/f/x",
            "https://file.notion.so:8443/f/x",
            "https://127.0.0.1/x",
            "https://www.notion.so/image.png",
            "file:///etc/passwd",
            "not a url",
        ] {
            assert!(!is_allowed_file_url(url), "{url}");
        }
    }

    #[test]
    fn names_files_from_url() {
        let url = Url::parse("https://s3.us-west-2.amazonaws.com/secure.notion-static.com/id/My%20Report%2Fv2.pdf?sig=1").unwrap();
        assert_eq!(suggested_file_name(&url), "My Report_v2.pdf");
        let url =
            Url::parse("https://file.notion.so/f/f/a/b/x?downloadName=brocolli.jpeg").unwrap();
        assert_eq!(suggested_file_name(&url), "brocolli.jpeg");
        let url = Url::parse("https://file.notion.so/").unwrap();
        assert_eq!(suggested_file_name(&url), "file");
        assert_eq!(sanitize_file_name("../../etc/passwd"), "_.._etc_passwd");
    }
}
