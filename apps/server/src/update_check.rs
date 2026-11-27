//! Asks the coolLabs CDN for the latest Orbit release, so Admin can show "Update available" on a self-hosted
//! server. `ORBIT_UPDATE_CHECK=false` and Orbit Cloud turn it off.

use std::time::{Duration, Instant};

use serde::Deserialize;
use tokio::sync::Mutex;

/// The latest release: `{"version": "0.5.0"}`, kept in the `coollabs-cdn` repository at `json/orbit/version.json`.
pub const VERSION_URL: &str = "https://cdn.coollabs.io/orbit/version.json";
/// How long an answer is kept before the next request asks again.
const CACHE_FOR: Duration = Duration::from_secs(24 * 60 * 60);
/// A failure (CDN down, timeout) is kept for a shorter time, so one error does not hide an update for a day.
const RETRY_AFTER: Duration = Duration::from_secs(60 * 60);

pub struct UpdateCheck {
    url: String,
    client: reqwest::Client,
    /// Until when the last answer holds, and the version the CDN gave (`None` after a failure).
    cache: Mutex<Option<(Instant, Option<semver::Version>)>>,
}

#[derive(Deserialize)]
struct VersionFile {
    version: String,
}

impl UpdateCheck {
    /// # Panics
    /// When the HTTP client cannot start (no TLS backend).
    #[must_use]
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            client: reqwest::Client::builder()
                .user_agent(concat!("orbit/", env!("CARGO_PKG_VERSION")))
                .timeout(Duration::from_secs(5))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("the update check HTTP client starts"),
            cache: Mutex::new(None),
        }
    }

    /// The latest release when it is newer than this server; `None` when it is not, or the CDN did not answer.
    pub async fn newer_version(&self) -> Option<String> {
        let mut cache = self.cache.lock().await;
        let latest = match &*cache {
            Some((until, latest)) if Instant::now() < *until => latest.clone(),
            _ => {
                let latest = self.fetch().await;
                let keep = if latest.is_some() {
                    CACHE_FOR
                } else {
                    RETRY_AFTER
                };
                *cache = Some((Instant::now() + keep, latest.clone()));
                latest
            }
        };
        newer_than_running(latest?)
    }

    async fn fetch(&self) -> Option<semver::Version> {
        let answer = async {
            let file: VersionFile = self
                .client
                .get(&self.url)
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            Ok::<_, reqwest::Error>(file.version)
        }
        .await;
        match answer {
            Ok(version) => version
                .trim()
                .trim_start_matches('v')
                .parse()
                .ok()
                .or_else(|| {
                    tracing::warn!(%version, "the update check got a version that is not semver");
                    None
                }),
            Err(error) => {
                tracing::warn!(%error, "the update check could not read the latest Orbit version");
                None
            }
        }
    }
}

fn newer_than_running(latest: semver::Version) -> Option<String> {
    let running: semver::Version = env!("CARGO_PKG_VERSION").parse().ok()?;
    (latest > running).then(|| latest.to_string())
}

#[cfg(test)]
mod tests {
    use super::newer_than_running;

    #[test]
    fn only_a_newer_release_is_an_update() {
        let running: semver::Version = env!("CARGO_PKG_VERSION").parse().unwrap();
        let mut newer = running.clone();
        newer.minor += 1;
        assert_eq!(newer_than_running(newer.clone()), Some(newer.to_string()));
        assert_eq!(newer_than_running(running.clone()), None);
        let mut older = running.clone();
        older.major = 0;
        older.minor = 0;
        older.patch = 0;
        assert_eq!(newer_than_running(older), None);
        // a pre-release of the running version is older than it
        let mut pre = running;
        pre.pre = semver::Prerelease::new("rc.1").unwrap();
        assert_eq!(newer_than_running(pre), None);
    }
}
