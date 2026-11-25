//! The https origins whose pages a workspace may show as custom pages (#94). The app's `frame-src` allows them,
//! and custom page URLs must be on one of them. The root user saves them in Admin, or `ORBIT_FRAME_ORIGINS` sets
//! them; either can change while the server runs, so every response reads the current list.

use std::sync::{Arc, PoisonError, RwLock};

use axum::http::Uri;

/// The current list, shared by the CSP layer and the routes that check page URLs.
#[derive(Clone, Debug, Default)]
pub struct FrameOrigins(Arc<RwLock<Arc<[String]>>>);

impl FrameOrigins {
    #[must_use]
    pub fn get(&self) -> Arc<[String]> {
        Arc::clone(&self.0.read().unwrap_or_else(PoisonError::into_inner))
    }

    pub fn set(&self, origins: Vec<String>) {
        *self.0.write().unwrap_or_else(PoisonError::into_inner) = origins.into();
    }
}

/// Most origins in the list: each one is a `frame-src` source on every response.
pub const MAX_FRAME_ORIGINS: usize = 20;

/// `value` as the browser writes the origin (`https://grafana.example.com`, with a port only when it is not 443),
/// or `None` when it is not an https origin without a path, query, credentials or wildcard.
#[must_use]
pub fn frame_origin(value: &str) -> Option<String> {
    let value = value.trim().trim_end_matches('/').to_ascii_lowercase();
    if value.contains(['@', '*', ' ', ';', ',', '\'']) {
        return None;
    }
    let uri = value.parse::<Uri>().ok()?;
    let authority = uri.authority()?;
    if uri.scheme_str() != Some("https")
        || uri.path() != "/"
        || uri.query().is_some()
        || authority.host().is_empty()
    {
        return None;
    }
    Some(match authority.port_u16() {
        None | Some(443) => format!("https://{}", authority.host()),
        Some(port) => format!("https://{}:{port}", authority.host()),
    })
}

/// Parses a list (comma- or line-separated); `Err` names the first entry that is not an origin, is `public_origin`
/// or is one too many.
pub fn frame_origin_list<'a>(
    entries: impl IntoIterator<Item = &'a str>,
    public_origin: &str,
) -> Result<Vec<String>, String> {
    let own = frame_origin(public_origin);
    let mut origins: Vec<String> = Vec::new();
    for entry in entries
        .into_iter()
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
    {
        let origin = frame_origin(entry)
            .filter(|origin| Some(origin) != own.as_ref())
            .ok_or_else(|| entry.to_owned())?;
        if !origins.contains(&origin) {
            if origins.len() == MAX_FRAME_ORIGINS {
                return Err(entry.to_owned());
            }
            origins.push(origin);
        }
    }
    Ok(origins)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins_are_written_as_the_browser_writes_them() {
        assert_eq!(
            frame_origin(" https://Grafana.example.com/ ").as_deref(),
            Some("https://grafana.example.com")
        );
        assert_eq!(
            frame_origin("https://grafana.example.com:443").as_deref(),
            Some("https://grafana.example.com")
        );
        assert_eq!(
            frame_origin("https://grafana.example.com:8443").as_deref(),
            Some("https://grafana.example.com:8443")
        );
        for bad in [
            "http://grafana.example.com",
            "https://grafana.example.com/d/1",
            "https://grafana.example.com?x=1",
            "https://*.example.com",
            "https://user@grafana.example.com",
            "grafana.example.com",
            "https://a.example.com https://b.example.com",
            "https://a.example.com;script-src",
            "",
        ] {
            assert_eq!(frame_origin(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_list_refuses_orbit_itself_and_drops_duplicates() {
        assert_eq!(
            frame_origin_list(
                ["https://a.example.com", "", "https://A.example.com/"],
                "https://orbit.example.com"
            ),
            Ok(vec!["https://a.example.com".to_owned()])
        );
        assert_eq!(
            frame_origin_list(["https://orbit.example.com/"], "https://orbit.example.com"),
            Err("https://orbit.example.com/".to_owned())
        );
        let many: Vec<String> = (0..=MAX_FRAME_ORIGINS)
            .map(|n| format!("https://{n}.example.com"))
            .collect();
        assert!(
            frame_origin_list(many.iter().map(String::as_str), "https://orbit.example.com")
                .is_err()
        );
    }

    #[test]
    fn every_clone_sees_a_new_list() {
        let origins = FrameOrigins::default();
        let reader = origins.clone();
        origins.set(vec!["https://a.example.com".to_owned()]);
        assert_eq!(&*reader.get(), ["https://a.example.com".to_owned()]);
    }
}
