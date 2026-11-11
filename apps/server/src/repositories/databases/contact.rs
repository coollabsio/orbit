//! Email and phone properties (spec P3 item 10): text values with a shape check, shown as
//! `mailto:` and `tel:` links.

use serde_json::Value;

use super::CellValue;

pub const MAX_EMAIL_CHARS: usize = 320;
pub const MAX_PHONE_CHARS: usize = 50;

/// One `@` with text on both sides, no whitespace.
pub(super) fn valid_email(text: &str) -> bool {
    text.chars().count() <= MAX_EMAIL_CHARS
        && !text.chars().any(char::is_whitespace)
        && text.split_once('@').is_some_and(|(local, domain)| {
            !local.is_empty() && !domain.is_empty() && !domain.contains('@')
        })
}

/// Digits, spaces and `+ - ( ) .`, with at least one digit.
pub(super) fn valid_phone(text: &str) -> bool {
    text.chars().count() <= MAX_PHONE_CHARS
        && text.chars().any(|c| c.is_ascii_digit())
        && text
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, ' ' | '+' | '-' | '(' | ')' | '.'))
}

/// A string value that `valid` accepts; `""` clears.
pub(super) fn parse(raw: &Value, valid: fn(&str) -> bool) -> Result<Option<CellValue>, ()> {
    let text = raw.as_str().ok_or(())?;
    if text.is_empty() {
        Ok(None)
    } else if valid(text) {
        Ok(Some(CellValue::Text(text.to_owned())))
    } else {
        Err(())
    }
}
