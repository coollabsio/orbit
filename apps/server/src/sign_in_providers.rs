//! Google, GitHub and one OpenID Connect provider (`ORBIT_OIDC_*`) as sign-in providers: the authorization URL, the
//! code exchange and the profile. Everything that knows a provider's wire format is here. Callers get a
//! [`ProviderProfile`] or a [`ProviderFailure`].

use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use url::Url;
use utoipa::ToSchema;

/// The redirect URI the Orbit app registers. Native sign-ins may only return here, or to an exact
/// `ORBIT_DEV_NATIVE_REDIRECTS` entry.
pub const APP_REDIRECT: &str = "orbit://auth/callback";

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Google,
    Github,
    /// Any OpenID Connect identity provider, set only with `ORBIT_OIDC_*`.
    Oidc,
}

impl Provider {
    pub const ALL: [Self; 3] = [Self::Google, Self::Github, Self::Oidc];

    /// The value in paths, JSON and the `provider` columns.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Google => "google",
            Self::Github => "github",
            Self::Oidc => "oidc",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|provider| provider.as_str() == value)
    }

    /// `{public_origin}/api/v1/auth/providers/{provider}/callback`: what the admin registers at the provider.
    #[must_use]
    pub fn callback_url(self, public_origin: &str) -> String {
        format!(
            "{}/api/v1/auth/providers/{}/callback",
            public_origin.trim_end_matches('/'),
            self.as_str()
        )
    }
}

/// The OAuth client an admin saved, decrypted.
pub struct ProviderCredentials {
    pub client_id: String,
    pub client_secret: String,
    /// `ORBIT_OIDC_ISSUER`, for [`Provider::Oidc`] only.
    pub issuer: Option<Url>,
}

/// Where the providers are. `Default` is production; tests point every URL at one local fake server.
#[derive(Clone, Debug)]
pub struct ProviderEndpoints {
    google_authorize: Url,
    google_token: Url,
    google_userinfo: Url,
    github_authorize: Url,
    github_token: Url,
    github_api: Url,
}

impl Default for ProviderEndpoints {
    fn default() -> Self {
        let url = |value: &str| Url::parse(value).expect("static provider URL");
        Self {
            google_authorize: url("https://accounts.google.com/o/oauth2/v2/auth"),
            google_token: url("https://oauth2.googleapis.com/token"),
            google_userinfo: url("https://openidconnect.googleapis.com/v1/userinfo"),
            github_authorize: url("https://github.com/login/oauth/authorize"),
            github_token: url("https://github.com/login/oauth/access_token"),
            github_api: url("https://api.github.com/"),
        }
    }
}

impl ProviderEndpoints {
    /// Every endpoint under `base`: `{base}/google/authorize`, `/google/token`, `/google/userinfo`,
    /// `/github/authorize`, `/github/token`, and the GitHub API at `{base}/github/api/`.
    #[must_use]
    pub fn local(base: &str) -> Self {
        let base = base.trim_end_matches('/');
        let url = |path: &str| Url::parse(&format!("{base}{path}")).expect("local provider URL");
        Self {
            google_authorize: url("/google/authorize"),
            google_token: url("/google/token"),
            google_userinfo: url("/google/userinfo"),
            github_authorize: url("/github/authorize"),
            github_token: url("/github/token"),
            github_api: url("/github/api/"),
        }
    }
}

/// What a provider proved about a person.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProviderProfile {
    pub provider: Provider,
    /// Google `sub`, GitHub `id`: the identity key. Never the email.
    pub subject: String,
    /// Normalized, and present only when the provider vouches for it: Google `email_verified`, GitHub the
    /// `primary && verified` entry of `/user/emails`. Linking and sign-up need it.
    pub verified_email: Option<String>,
    /// For Connected accounts: the verified email, else the address or login the provider reported.
    pub email: Option<String>,
    /// Seeds the display name of a new account.
    pub display_name: String,
}

/// The provider step did not produce a profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderFailure {
    /// The person declined, or the provider sent an error to the callback.
    Denied,
    /// Unreachable, a non-2xx answer, a GitHub 200 with `error`, a profile we cannot read, or the provider was
    /// removed in the meantime.
    Unavailable,
}

/// Why the authorization step runs. A confirmation must not pass on a silent provider session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Freshness {
    Any,
    ForceLogin,
}

/// A new PKCE verifier (RFC 7636): 32 random bytes, base64url.
#[must_use]
pub fn pkce_verifier() -> String {
    orbit_platform::generate_opaque_token()
}

/// `base64url(sha256(verifier))`, the S256 code challenge.
#[must_use]
pub fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// Whether `value` has the shape of an S256 challenge.
#[must_use]
pub fn is_pkce_challenge(value: &str) -> bool {
    value.len() == 43
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

/// The redirect URIs a native client may ask for, compared exactly. No prefixes or patterns: a pattern is how an
/// open redirect hands a handoff code to someone else.
#[derive(Clone, Debug)]
pub struct NativeRedirects(Vec<String>);

impl NativeRedirects {
    #[must_use]
    pub fn new(extra: &[String]) -> Self {
        let mut allowed = vec![APP_REDIRECT.to_owned()];
        allowed.extend(extra.iter().cloned());
        Self(allowed)
    }

    #[must_use]
    pub fn allows(&self, candidate: &str) -> bool {
        self.0.iter().any(|allowed| allowed == candidate)
    }
}

impl Default for NativeRedirects {
    fn default() -> Self {
        Self::new(&[])
    }
}

/// The HTTP side: no redirects, short timeouts, rustls.
#[derive(Clone)]
pub struct ProviderClient {
    http: reqwest::Client,
    endpoints: ProviderEndpoints,
}

impl Default for ProviderClient {
    fn default() -> Self {
        Self::new(ProviderEndpoints::default())
    }
}

impl ProviderClient {
    #[must_use]
    pub fn new(endpoints: ProviderEndpoints) -> Self {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(20))
            .user_agent("Orbit")
            .build()
            .expect("the provider HTTP client builds");
        Self { http, endpoints }
    }

    /// The URL the browser opens. Scopes: Google and OpenID Connect `openid email profile`; GitHub
    /// `read:user user:email`. OpenID Connect reads the authorization endpoint from discovery first.
    pub async fn authorization_url(
        &self,
        provider: Provider,
        credentials: &ProviderCredentials,
        redirect_uri: &str,
        state: &str,
        pkce_challenge: &str,
        freshness: Freshness,
    ) -> Result<Url, ProviderFailure> {
        let (mut url, scope) = match provider {
            Provider::Google => (
                self.endpoints.google_authorize.clone(),
                "openid email profile",
            ),
            Provider::Github => (
                self.endpoints.github_authorize.clone(),
                "read:user user:email",
            ),
            Provider::Oidc => (
                self.discover(credentials).await?.authorization_endpoint,
                "openid email profile",
            ),
        };
        {
            let mut query = url.query_pairs_mut();
            query
                .append_pair("client_id", &credentials.client_id)
                .append_pair("redirect_uri", redirect_uri)
                .append_pair("response_type", "code")
                .append_pair("scope", scope)
                .append_pair("state", state)
                .append_pair("code_challenge", pkce_challenge)
                .append_pair("code_challenge_method", "S256");
            match (provider, freshness) {
                (Provider::Google, Freshness::Any) => {
                    query.append_pair("prompt", "select_account");
                }
                (Provider::Google, Freshness::ForceLogin) => {
                    query
                        .append_pair("prompt", "select_account")
                        .append_pair("max_age", "0");
                }
                (Provider::Github, _) => {
                    query.append_pair("prompt", "select_account");
                }
                // The identity provider's own session is the point of single sign-on: no prompt by default.
                (Provider::Oidc, Freshness::Any) => {}
                (Provider::Oidc, Freshness::ForceLogin) => {
                    query
                        .append_pair("prompt", "login")
                        .append_pair("max_age", "0");
                }
            }
        }
        Ok(url)
    }

    /// Code, then access token, then profile. Google: token endpoint, then userinfo. GitHub: token endpoint with
    /// `Accept: application/json` (errors come back as 200 with `error`), then `/user` and `/user/emails`.
    /// OpenID Connect: discovery, then the token and userinfo endpoints it names.
    pub async fn profile(
        &self,
        provider: Provider,
        credentials: &ProviderCredentials,
        redirect_uri: &str,
        code: &str,
        pkce_verifier: &str,
    ) -> Result<ProviderProfile, ProviderFailure> {
        let (token_url, discovery) = match provider {
            Provider::Google => (self.endpoints.google_token.clone(), None),
            Provider::Github => (self.endpoints.github_token.clone(), None),
            Provider::Oidc => {
                let discovery = self.discover(credentials).await?;
                (discovery.token_endpoint.clone(), Some(discovery))
            }
        };
        let mut form = vec![
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("code_verifier", pkce_verifier),
        ];
        let mut request = self
            .http
            .post(token_url)
            .header(reqwest::header::ACCEPT, "application/json");
        if discovery
            .as_ref()
            .is_some_and(OidcDiscovery::takes_basic_auth)
        {
            // RFC 6749 section 2.3.1: both parts are form-encoded before they go into the header.
            let encode = |value: &str| {
                url::form_urlencoded::byte_serialize(value.as_bytes()).collect::<String>()
            };
            request = request.basic_auth(
                encode(&credentials.client_id),
                Some(encode(&credentials.client_secret)),
            );
        } else {
            form.push(("client_id", credentials.client_id.as_str()));
            form.push(("client_secret", credentials.client_secret.as_str()));
        }
        let token = request
            .form(&form)
            .send()
            .await
            .map_err(|error| unavailable(provider, "token request", &error))?;
        let token: TokenResponse = read_json(provider, "token", token).await?;
        let access_token = token.access_token.ok_or_else(|| {
            tracing::warn!(provider = provider.as_str(), error = ?token.error, "provider refused the code");
            ProviderFailure::Unavailable
        })?;
        match (provider, discovery) {
            (Provider::Google, _) => self.google_profile(&access_token).await,
            (Provider::Github, _) => self.github_profile(&access_token).await,
            (Provider::Oidc, Some(discovery)) => self.oidc_profile(&discovery, &access_token).await,
            (Provider::Oidc, None) => Err(ProviderFailure::Unavailable),
        }
    }

    /// `{issuer}/.well-known/openid-configuration`, read on every use so a change at the identity provider needs no
    /// restart. The document must name the configured issuer (OpenID Connect Discovery 1.0, section 4.3).
    async fn discover(
        &self,
        credentials: &ProviderCredentials,
    ) -> Result<OidcDiscovery, ProviderFailure> {
        let provider = Provider::Oidc;
        let issuer = credentials
            .issuer
            .as_ref()
            .ok_or(ProviderFailure::Unavailable)?;
        let issuer = issuer.as_str().trim_end_matches('/');
        let response = self
            .http
            .get(format!("{issuer}/.well-known/openid-configuration"))
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|error| unavailable(provider, "discovery", &error))?;
        let discovery: OidcDiscovery = read_json(provider, "discovery", response).await?;
        if discovery.issuer.trim_end_matches('/') != issuer {
            tracing::warn!(
                expected = issuer,
                found = discovery.issuer,
                "the OpenID Connect discovery document names another issuer"
            );
            return Err(ProviderFailure::Unavailable);
        }
        if discovery.userinfo_endpoint.is_none() {
            tracing::warn!(
                issuer,
                "the OpenID Connect provider has no userinfo endpoint"
            );
            return Err(ProviderFailure::Unavailable);
        }
        Ok(discovery)
    }

    /// The identity key is the issuer and `sub` together (OpenID Connect Core 1.0, section 5.7): `sub` is unique
    /// only at one issuer, so pointing `ORBIT_OIDC_ISSUER` at another identity provider cannot reuse old links.
    async fn oidc_profile(
        &self,
        discovery: &OidcDiscovery,
        access_token: &str,
    ) -> Result<ProviderProfile, ProviderFailure> {
        let provider = Provider::Oidc;
        let userinfo = discovery
            .userinfo_endpoint
            .clone()
            .ok_or(ProviderFailure::Unavailable)?;
        let response = self
            .http
            .get(userinfo)
            .bearer_auth(access_token)
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|error| unavailable(provider, "userinfo", &error))?;
        let user: OidcUserInfo = read_json(provider, "userinfo", response).await?;
        if user.sub.is_empty() || user.sub.len() > 256 {
            return Err(ProviderFailure::Unavailable);
        }
        let email = user.email.filter(|email| !email.trim().is_empty());
        let email_verified = matches!(&user.email_verified, Some(serde_json::Value::Bool(true)))
            || matches!(&user.email_verified, Some(serde_json::Value::String(value)) if value == "true");
        let verified_email = email
            .as_deref()
            .filter(|_| email_verified)
            .map(orbit_platform::normalize_email);
        let username = user
            .preferred_username
            .filter(|name| !name.trim().is_empty());
        let name = user
            .name
            .as_deref()
            .filter(|name| !name.trim().is_empty())
            .or(username.as_deref());
        Ok(ProviderProfile {
            provider,
            subject: format!("{} {}", discovery.issuer.trim_end_matches('/'), user.sub),
            display_name: display_name(name, email.as_deref()),
            email: verified_email.clone().or(email).or(username),
            verified_email,
        })
    }

    async fn google_profile(&self, access_token: &str) -> Result<ProviderProfile, ProviderFailure> {
        let provider = Provider::Google;
        let response = self
            .http
            .get(self.endpoints.google_userinfo.clone())
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|error| unavailable(provider, "userinfo", &error))?;
        let user: GoogleUserInfo = read_json(provider, "userinfo", response).await?;
        if user.sub.is_empty() || user.sub.len() > 256 {
            return Err(ProviderFailure::Unavailable);
        }
        let email = user.email.filter(|email| !email.trim().is_empty());
        let verified_email = email
            .as_deref()
            .filter(|_| user.email_verified == Some(true))
            .map(orbit_platform::normalize_email);
        Ok(ProviderProfile {
            provider,
            subject: user.sub,
            display_name: display_name(user.name.as_deref(), email.as_deref()),
            email: verified_email.clone().or(email),
            verified_email,
        })
    }

    async fn github_profile(&self, access_token: &str) -> Result<ProviderProfile, ProviderFailure> {
        let provider = Provider::Github;
        let get = |path: &str| {
            let url = self
                .endpoints
                .github_api
                .join(path)
                .expect("relative GitHub API path");
            self.http
                .get(url)
                .bearer_auth(access_token)
                .header(reqwest::header::ACCEPT, "application/vnd.github+json")
                .header("X-GitHub-Api-Version", "2022-11-28")
                .send()
        };
        let response = get("user")
            .await
            .map_err(|error| unavailable(provider, "user", &error))?;
        let user: GithubUser = read_json(provider, "user", response).await?;
        let response = get("user/emails")
            .await
            .map_err(|error| unavailable(provider, "user emails", &error))?;
        let emails: Vec<GithubEmail> = read_json(provider, "user emails", response).await?;
        let verified_email = emails
            .iter()
            .find(|email| email.primary && email.verified)
            .map(|email| orbit_platform::normalize_email(&email.email));
        let name = user.name.as_deref().filter(|name| !name.trim().is_empty());
        Ok(ProviderProfile {
            provider,
            subject: user.id.to_string(),
            display_name: display_name(name.or(Some(user.login.as_str())), None),
            email: verified_email.clone().or(Some(user.login)),
            verified_email,
        })
    }
}

fn unavailable(provider: Provider, step: &str, error: &reqwest::Error) -> ProviderFailure {
    tracing::warn!(provider = provider.as_str(), step, %error, "provider request failed");
    ProviderFailure::Unavailable
}

async fn read_json<T: serde::de::DeserializeOwned>(
    provider: Provider,
    step: &str,
    response: reqwest::Response,
) -> Result<T, ProviderFailure> {
    let status = response.status();
    if !status.is_success() {
        tracing::warn!(provider = provider.as_str(), step, %status, "provider answered with an error");
        return Err(ProviderFailure::Unavailable);
    }
    response
        .json()
        .await
        .map_err(|error| unavailable(provider, step, &error))
}

/// The name a new account starts with: the provider's name, else the email's local part, at most 100 characters.
fn display_name(name: Option<&str>, email: Option<&str>) -> String {
    let name = name
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .or_else(|| email.and_then(|email| email.split('@').next()))
        .filter(|name| !name.is_empty())
        .unwrap_or("Orbit user");
    name.chars().take(100).collect()
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct GoogleUserInfo {
    sub: String,
    email: Option<String>,
    email_verified: Option<bool>,
    name: Option<String>,
}

#[derive(Deserialize)]
struct OidcDiscovery {
    issuer: String,
    authorization_endpoint: Url,
    token_endpoint: Url,
    userinfo_endpoint: Option<Url>,
    #[serde(default)]
    token_endpoint_auth_methods_supported: Option<Vec<String>>,
}

impl OidcDiscovery {
    /// `client_secret_basic` is the default when the provider lists no methods (Discovery 1.0, section 3), and the
    /// one every provider must support; `client_secret_post` only when the provider lists it and not basic.
    fn takes_basic_auth(&self) -> bool {
        self.token_endpoint_auth_methods_supported
            .as_ref()
            .is_none_or(|methods| {
                methods.iter().any(|method| method == "client_secret_basic")
                    || !methods.iter().any(|method| method == "client_secret_post")
            })
    }
}

/// Some providers send `email_verified` as the string `"true"`.
#[derive(Deserialize)]
struct OidcUserInfo {
    sub: String,
    email: Option<String>,
    email_verified: Option<serde_json::Value>,
    name: Option<String>,
    preferred_username: Option<String>,
}

#[derive(Deserialize)]
struct GithubUser {
    id: u64,
    login: String,
    name: Option<String>,
}

#[derive(Deserialize)]
struct GithubEmail {
    email: String,
    primary: bool,
    verified: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_challenge_matches_the_rfc_7636_example() {
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        assert!(is_pkce_challenge(
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        ));
        assert!(!is_pkce_challenge(
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-c"
        ));
        assert!(!is_pkce_challenge(
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw+cM"
        ));
    }

    #[test]
    fn native_redirects_match_exactly() {
        let redirects =
            NativeRedirects::new(&["exp://100.64.0.5:8082/--/auth/callback".to_owned()]);
        assert!(redirects.allows("orbit://auth/callback"));
        assert!(redirects.allows("exp://100.64.0.5:8082/--/auth/callback"));
        for refused in [
            "orbit://auth/callback/",
            "orbit://auth/callback?x=1",
            "orbit://evil",
            "https://evil.example/auth/callback",
            "exp://100.64.0.5:8082/--/auth/callback2",
            "exp://evil.example:8082/--/auth/callback",
        ] {
            assert!(!redirects.allows(refused), "{refused}");
        }
        assert!(!NativeRedirects::default().allows("exp://100.64.0.5:8082/--/auth/callback"));
    }

    #[test]
    fn new_accounts_get_a_short_non_empty_name() {
        assert_eq!(display_name(Some("  Ada Lovelace "), None), "Ada Lovelace");
        assert_eq!(display_name(Some(" "), Some("ada@example.com")), "ada");
        assert_eq!(display_name(None, None), "Orbit user");
        assert_eq!(display_name(Some(&"x".repeat(300)), None).len(), 100);
    }

    #[tokio::test]
    async fn the_authorization_url_carries_state_pkce_and_scope() {
        let client = ProviderClient::new(ProviderEndpoints::default());
        let credentials = ProviderCredentials {
            client_id: "id.apps.googleusercontent.com".to_owned(),
            client_secret: "secret".to_owned(),
            issuer: None,
        };
        let url = client
            .authorization_url(
                Provider::Google,
                &credentials,
                "https://orbit.example/api/v1/auth/providers/google/callback",
                "state-1",
                "challenge-1",
                Freshness::ForceLogin,
            )
            .await
            .unwrap();
        assert_eq!(
            url.as_str(),
            "https://accounts.google.com/o/oauth2/v2/auth?client_id=id.apps.googleusercontent.com\
             &redirect_uri=https%3A%2F%2Forbit.example%2Fapi%2Fv1%2Fauth%2Fproviders%2Fgoogle%2Fcallback\
             &response_type=code&scope=openid+email+profile&state=state-1&code_challenge=challenge-1\
             &code_challenge_method=S256&prompt=select_account&max_age=0"
        );
    }
}
