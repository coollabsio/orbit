//! Which sites a workspace may show as custom pages (#94). The root user picks a [`FramePolicy`] in Admin, or
//! `ORBIT_FRAME_ORIGINS` sets it. The app's `frame-src` allows only the origins of the pages that the signed-in user
//! sees ([`CspFrameSources`]), so the policy itself never goes into a header.

use axum::http::Uri;

/// Which https URLs a custom page may use. No mode allows Orbit's own origin.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum FramePolicy {
    /// Any https site.
    #[default]
    Any,
    /// Only these origins (as [`frame_origin`] writes them).
    Listed(Vec<String>),
    /// No custom pages.
    Off,
}

impl FramePolicy {
    /// `any`, `listed` or `off`.
    #[must_use]
    pub fn mode(&self) -> &'static str {
        match self {
            Self::Any => "any",
            Self::Listed(_) => "listed",
            Self::Off => "off",
        }
    }

    /// The listed origins; empty for the other modes.
    #[must_use]
    pub fn origins(&self) -> &[String] {
        match self {
            Self::Listed(origins) => origins,
            Self::Any | Self::Off => &[],
        }
    }

    /// Whether a page may frame `origin` (as [`frame_origin`] writes it).
    #[must_use]
    pub fn allows(&self, origin: &str) -> bool {
        match self {
            Self::Any => true,
            Self::Listed(origins) => origins.iter().any(|listed| listed == origin),
            Self::Off => false,
        }
    }

    /// `ORBIT_FRAME_ORIGINS`: `any`, `off`, or a comma-separated list of origins. `Err` names the bad entry.
    pub fn from_env(value: &str, public_origin: &str) -> Result<Self, String> {
        match value.trim() {
            "any" => Ok(Self::Any),
            "off" => Ok(Self::Off),
            list => frame_origin_list(list.split(','), public_origin).map(Self::Listed),
        }
    }
}

/// A response extension: the custom page origins that this document's `frame-src` also allows. The server sets it on
/// the app's HTML document for the signed-in user; the platform layer writes only entries that are origins.
#[derive(Clone, Debug, Default)]
pub struct CspFrameSources(pub Vec<String>);

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

/// Parses a list (comma- or line-separated); `Err` names the first entry that is not an origin or is `public_origin`.
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
        let many: Vec<String> = (0..500)
            .map(|n| format!("https://{n}.example.com"))
            .collect();
        assert_eq!(
            frame_origin_list(many.iter().map(String::as_str), "https://orbit.example.com"),
            Ok(many.clone())
        );
    }

    #[test]
    fn the_environment_names_a_mode_or_a_list() {
        let orbit = "https://orbit.example.com";
        assert_eq!(FramePolicy::from_env(" any ", orbit), Ok(FramePolicy::Any));
        assert_eq!(FramePolicy::from_env("off", orbit), Ok(FramePolicy::Off));
        let listed =
            FramePolicy::from_env("https://A.example.com/,https://b.example.com", orbit).unwrap();
        assert_eq!(
            listed.origins(),
            ["https://a.example.com", "https://b.example.com"]
        );
        assert!(listed.allows("https://a.example.com") && !listed.allows("https://c.example.com"));
        assert!(
            FramePolicy::Any.allows("https://c.example.com")
                && !FramePolicy::Off.allows("https://c.example.com")
        );
        assert_eq!(
            FramePolicy::from_env("anything", orbit),
            Err("anything".to_owned())
        );
    }
}
