//! Safe, unique file and folder names inside an export archive, and relative links between them.

use std::collections::HashSet;

/// Longest file-name stem, in characters (the extension and a " (n)" suffix come on top).
pub const STEM_MAX_CHARS: usize = 80;
/// Folder that holds a page's files next to its sub-pages.
pub const ASSETS_DIR: &str = "assets";

const WINDOWS_RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// A single path segment built from user text: no separators, control or Windows-reserved
/// characters, no leading/trailing dots or spaces, no reserved device names, at most
/// `max_chars` characters. Empty results become `fallback`.
#[must_use]
pub fn sanitize_segment(text: &str, max_chars: usize, fallback: &str) -> String {
    let cleaned: String = text
        .chars()
        .map(|character| match character {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            character if character.is_control() => ' ',
            character => character,
        })
        .collect();
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let truncated: String = collapsed.chars().take(max_chars).collect();
    let trimmed = truncated.trim_matches(|character: char| character == '.' || character == ' ');
    if trimmed.is_empty() {
        return fallback.to_owned();
    }
    let base = trimmed
        .split('.')
        .next()
        .unwrap_or(trimmed)
        .to_ascii_uppercase();
    if WINDOWS_RESERVED.contains(&base.as_str()) {
        return format!("_{trimmed}");
    }
    trimmed.to_owned()
}

/// Splits `name.ext` into a sanitized stem and a short lowercase extension (letters/digits only).
#[must_use]
pub fn split_file_name(name: &str) -> (String, String) {
    let name = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let (stem, extension) = match name.rsplit_once('.') {
        Some((stem, extension))
            if !stem.trim().is_empty()
                && (1..=10).contains(&extension.len())
                && extension
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric()) =>
        {
            (stem, extension.to_ascii_lowercase())
        }
        _ => (name, String::new()),
    };
    (sanitize_segment(stem, STEM_MAX_CHARS, "file"), extension)
}

/// Hands out names that are unique within one folder, ignoring case (for case-insensitive
/// file systems): the second "Notes" becomes "Notes (2)".
#[derive(Default)]
pub struct NameSet {
    taken: HashSet<String>,
}

impl NameSet {
    /// A set in which `reserved` names (e.g. [`ASSETS_DIR`]) are already taken.
    #[must_use]
    pub fn with_reserved(reserved: &[&str]) -> Self {
        Self {
            taken: reserved.iter().map(|name| name.to_lowercase()).collect(),
        }
    }

    /// A unique stem for `stem`: every `stem.extension` (an empty extension is the bare stem,
    /// e.g. `["md", ""]` for a page's file and folder) must be free; all are then taken.
    pub fn claim(&mut self, stem: &str, extensions: &[&str]) -> String {
        for attempt in 1_usize.. {
            let candidate = if attempt == 1 {
                stem.to_owned()
            } else {
                format!("{stem} ({attempt})")
            };
            let names: Vec<String> = extensions
                .iter()
                .map(|extension| with_extension(&candidate, extension).to_lowercase())
                .collect();
            if names.iter().all(|name| !self.taken.contains(name)) {
                self.taken.extend(names);
                return candidate;
            }
        }
        unreachable!("an unbounded counter always finds a free name")
    }
}

/// `stem.extension`, or just `stem` without an extension.
#[must_use]
pub fn with_extension(stem: &str, extension: &str) -> String {
    if extension.is_empty() {
        stem.to_owned()
    } else {
        format!("{stem}.{extension}")
    }
}

/// The path of `target` relative to the folder `from_dir` (both archive paths with `/`),
/// with every segment percent-encoded for use as a Markdown link destination.
#[must_use]
pub fn relative_link(from_dir: &[String], target: &[String]) -> String {
    let common = from_dir
        .iter()
        .zip(target)
        .take_while(|(left, right)| left == right)
        .count();
    let mut parts: Vec<String> = Vec::new();
    for _ in common..from_dir.len() {
        parts.push("..".to_owned());
    }
    for segment in &target[common..] {
        parts.push(encode_segment(segment));
    }
    parts.join("/")
}

/// Percent-encodes everything but unreserved characters and non-ASCII letters (which Markdown
/// renderers and file browsers handle), so the link survives any Markdown dialect.
#[must_use]
pub fn encode_segment(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for character in segment.chars() {
        if character.is_ascii_alphanumeric()
            || matches!(character, '-' | '.' | '_' | '~')
            || (!character.is_ascii() && !character.is_control())
        {
            out.push(character);
        } else {
            let mut buffer = [0; 4];
            for byte in character.encode_utf8(&mut buffer).bytes() {
                out.push_str(&format!("%{byte:02X}"));
            }
        }
    }
    out
}

/// An ASCII-only download file name (quotes, backslashes, controls and non-ASCII replaced)
/// for the plain `filename=` parameter.
#[must_use]
pub fn ascii_file_name(name: &str) -> String {
    let ascii: String = name
        .chars()
        .map(|character| match character {
            '"' | '\\' | ';' | '%' => '_',
            character if character.is_ascii() && !character.is_ascii_control() => character,
            _ => '_',
        })
        .collect();
    ascii
}

/// RFC 5987 `ext-value` encoding (UTF-8, percent-encoded attr-chars) for `filename*=`.
#[must_use]
pub fn rfc5987(value: &str) -> String {
    let mut out = String::from("UTF-8''");
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"!#$&+-.^_`|~".contains(&byte) {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}
