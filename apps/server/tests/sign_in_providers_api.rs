//! Sign in with Google and GitHub, end to end over the auth and admin routers, against a local stand-in for both
//! providers that checks the client secret, the redirect URI and PKCE like the real ones.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::body::{Body, to_bytes};
use axum::extract::State;
use axum::http::{HeaderMap, Request, StatusCode, header};
use axum::routing::{get, post};
use axum::{Form, Json, Router};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use orbit_platform::{PasswordService, TestDatabase, TimestampMillis};
use orbit_server::auth_routes::{AuthState, CookieMode, auth_router};
use orbit_server::mail::Mailer;
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use orbit_server::sign_in_providers::ProviderEndpoints;
use orbit_server::workspace_routes::{WorkspaceState, workspace_router};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use totp_rs::{Builder, Secret};
use tower::ServiceExt;

const ORIGIN: &str = "https://orbit.test";
const PASSWORD: &str = "correct horse battery";
const CLIENT_SECRET: &str = "provider-client-secret";
const EXPO_REDIRECT: &str = "exp://100.64.0.5:8082/--/auth/callback";
const APP_KEY: [u8; 32] = [7; 32];

#[derive(Clone, Debug)]
enum FakeUser {
    Google {
        sub: &'static str,
        email: &'static str,
        verified: bool,
    },
    Github {
        id: u64,
        login: &'static str,
        emails: Vec<(&'static str, bool, bool)>,
    },
}

fn google(sub: &'static str, email: &'static str) -> FakeUser {
    FakeUser::Google {
        sub,
        email,
        verified: true,
    }
}

fn github(id: u64, email: &'static str) -> FakeUser {
    FakeUser::Github {
        id,
        login: "octocat",
        emails: vec![(email, true, true)],
    }
}

struct Grant {
    challenge: String,
    redirect_uri: String,
    user: FakeUser,
}

/// Google and GitHub in one local server. A code is good once and only with the right secret and verifier.
#[derive(Clone, Default)]
struct FakeProviders {
    grants: Arc<Mutex<HashMap<String, Grant>>>,
    tokens: Arc<Mutex<HashMap<String, FakeUser>>>,
}

impl FakeProviders {
    async fn token(
        State(fake): State<FakeProviders>,
        Form(form): Form<HashMap<String, String>>,
    ) -> Json<Value> {
        let grant = fake.grants.lock().unwrap().remove(&form["code"]);
        let Some(grant) = grant else {
            return Json(json!({"error": "bad_verification_code"}));
        };
        if form["client_secret"] != CLIENT_SECRET
            || form["redirect_uri"] != grant.redirect_uri
            || challenge(&form["code_verifier"]) != grant.challenge
        {
            return Json(json!({"error": "invalid_grant"}));
        }
        let token = format!("token-{}", form["code"]);
        fake.tokens
            .lock()
            .unwrap()
            .insert(token.clone(), grant.user);
        Json(json!({"access_token": token, "token_type": "bearer"}))
    }

    fn user(&self, headers: &HeaderMap) -> FakeUser {
        let token = headers[header::AUTHORIZATION]
            .to_str()
            .unwrap()
            .strip_prefix("Bearer ")
            .unwrap();
        self.tokens.lock().unwrap()[token].clone()
    }

    async fn start() -> (Self, String) {
        let fake = Self::default();
        let router = Router::new()
            .route("/google/token", post(Self::token))
            .route("/github/token", post(Self::token))
            .route(
                "/google/userinfo",
                get(|State(fake): State<FakeProviders>, headers: HeaderMap| async move {
                    let FakeUser::Google { sub, email, verified } = fake.user(&headers) else {
                        panic!("GitHub user at Google");
                    };
                    Json(json!({"sub": sub, "email": email, "email_verified": verified, "name": "Ada Google"}))
                }),
            )
            .route(
                "/github/api/user",
                get(|State(fake): State<FakeProviders>, headers: HeaderMap| async move {
                    let FakeUser::Github { id, login, .. } = fake.user(&headers) else {
                        panic!("Google user at GitHub");
                    };
                    Json(json!({"id": id, "login": login, "name": null}))
                }),
            )
            .route(
                "/github/api/user/emails",
                get(|State(fake): State<FakeProviders>, headers: HeaderMap| async move {
                    let FakeUser::Github { emails, .. } = fake.user(&headers) else {
                        panic!("Google user at GitHub");
                    };
                    Json(Value::Array(
                        emails
                            .into_iter()
                            .map(|(email, primary, verified)| {
                                json!({"email": email, "primary": primary, "verified": verified})
                            })
                            .collect(),
                    ))
                }),
            )
            .with_state(fake.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        (fake, url)
    }

    /// Plays the consent screen: the person accepts as `user`. Returns the callback path Orbit receives.
    fn consent(&self, authorize_url: &str, user: FakeUser) -> String {
        let url = url::Url::parse(authorize_url).unwrap();
        let query: HashMap<String, String> = url.query_pairs().into_owned().collect();
        assert_eq!(query["code_challenge_method"], "S256");
        let code = format!(
            "code-{}",
            self.grants.lock().unwrap().len() + self.tokens.lock().unwrap().len()
        );
        self.grants.lock().unwrap().insert(
            code.clone(),
            Grant {
                challenge: query["code_challenge"].clone(),
                redirect_uri: query["redirect_uri"].clone(),
                user,
            },
        );
        let callback = query["redirect_uri"].strip_prefix(ORIGIN).unwrap();
        format!("{callback}?state={}&code={code}", query["state"])
    }
}

fn challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn totp_code(secret: &str, steps: i64) -> String {
    let totp = Builder::new()
        .with_secret(Secret::try_from_base32(secret).unwrap())
        .build()
        .unwrap();
    let now = TimestampMillis::now().as_millis() / 1_000;
    totp.generate(u64::try_from(now + steps * 30).unwrap())
        .to_string()
}

struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: Value,
}

impl Reply {
    fn code(&self) -> &str {
        self.body["code"].as_str().unwrap_or_default()
    }

    fn location(&self) -> &str {
        self.headers[header::LOCATION].to_str().unwrap()
    }

    fn session_cookie(&self) -> String {
        let cookie = self.headers[header::SET_COOKIE].to_str().unwrap();
        cookie.split(';').next().unwrap().to_owned()
    }
}

/// How the exchange is called: a browser sends Origin and gets a cookie; an app sends neither.
#[derive(Clone, Copy, PartialEq)]
enum Client {
    Web,
    Native,
}

struct Fixture {
    database: TestDatabase,
    app: Router,
    fake: FakeProviders,
    /// The root account (owner@example.com, with a password).
    owner_cookie: String,
}

impl Fixture {
    async fn new() -> Self {
        let database = TestDatabase::new().await.unwrap();
        let identity = Arc::new(IdentityRepository::new((*database).clone()));
        let now = TimestampMillis::now();
        identity
            .store_setup_token(
                "operator-secret",
                TimestampMillis::from_millis(now.as_millis() + 60_000),
            )
            .await
            .unwrap();
        let setup = identity
            .complete_setup(
                SetupRequest {
                    token: "operator-secret".to_owned(),
                    email: "owner@example.com".to_owned(),
                    display_name: "Owner".to_owned(),
                    password_hash: PasswordService::default().hash(PASSWORD).unwrap(),
                    workspace_name: "Orbit".to_owned(),
                    project_name: "General".to_owned(),
                },
                now,
            )
            .await
            .unwrap();
        let (fake, url) = FakeProviders::start().await;
        let auth = AuthState::new(Arc::clone(&identity), CookieMode::secure())
            .with_app_key(APP_KEY)
            .with_public_origin(ORIGIN)
            .with_provider_endpoints(ProviderEndpoints::local(&url))
            .with_native_redirects(&[EXPO_REDIRECT.to_owned()]);
        let workspaces = WorkspaceState::new(
            Arc::clone(&identity),
            ORIGIN.to_owned(),
            CookieMode::secure(),
        )
        .with_mailer(Mailer::new((*database).clone(), Some(APP_KEY), ORIGIN));
        let fixture = Self {
            database,
            app: auth_router(auth).merge(workspace_router(workspaces)),
            fake,
            owner_cookie: format!("__Host-orbit_session={}", setup.session.token),
        };
        for provider in ["google", "github"] {
            let reply = fixture
                .admin_save(provider, json!({"client_id": format!("{provider}-client"), "client_secret": CLIENT_SECRET}))
                .await;
            assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
        }
        fixture
    }

    async fn send(
        &self,
        method: &str,
        path: &str,
        cookie: Option<&str>,
        origin: bool,
        body: Option<Value>,
    ) -> Reply {
        let mut request = Request::builder().method(method).uri(path);
        if let Some(cookie) = cookie {
            request = request.header(header::COOKIE, cookie);
        }
        if origin {
            request = request.header(header::ORIGIN, ORIGIN);
        }
        let request = match body {
            Some(body) => request
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string())),
            None => request.body(Body::empty()),
        }
        .unwrap();
        let response = self.app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        Reply {
            status,
            headers,
            body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        }
    }

    async fn admin_save(&self, provider: &str, body: Value) -> Reply {
        self.send(
            "PUT",
            &format!("/api/v1/admin/settings/sign-in-providers/{provider}"),
            Some(&self.owner_cookie),
            true,
            Some(body),
        )
        .await
    }

    async fn scalar_i64(&self, sql: &str) -> i64 {
        self.database.scalar::<i64>(sql).await.unwrap()
    }

    async fn start(&self, provider: &str, body: Value, cookie: Option<&str>) -> Reply {
        self.send(
            "POST",
            &format!("/api/v1/auth/providers/{provider}/start"),
            cookie,
            true,
            Some(body),
        )
        .await
    }

    /// Start, consent as `user`, and the callback. Returns the callback's answer and the verifier.
    async fn through_callback(
        &self,
        provider: &str,
        intent: &str,
        redirect: &str,
        user: FakeUser,
        cookie: Option<&str>,
    ) -> (Reply, String) {
        let verifier = format!("verifier-{}", orbit_platform::generate_opaque_token());
        let started = self
            .start(
                provider,
                json!({"intent": intent, "code_challenge": challenge(&verifier), "redirect": redirect, "password": (intent == "connect").then_some(PASSWORD)}),
                cookie,
            )
            .await;
        assert_eq!(started.status, StatusCode::OK, "{}", started.body);
        let callback = self
            .fake
            .consent(started.body["authorize_url"].as_str().unwrap(), user);
        let reply = self.send("GET", &callback, None, false, None).await;
        assert_eq!(reply.status, StatusCode::SEE_OTHER);
        (reply, verifier)
    }

    async fn exchange(
        &self,
        handoff: &str,
        verifier: &str,
        client: Client,
        cookie: Option<&str>,
    ) -> Reply {
        self.send(
            "POST",
            "/api/v1/auth/providers/exchange",
            cookie,
            client == Client::Web,
            Some(json!({"handoff": handoff, "code_verifier": verifier})),
        )
        .await
    }

    /// A whole web flow for `intent`.
    async fn flow(
        &self,
        provider: &str,
        intent: &str,
        user: FakeUser,
        cookie: Option<&str>,
    ) -> Reply {
        let (callback, verifier) = self
            .through_callback(provider, intent, "web", user, cookie)
            .await;
        self.exchange(
            &web_handoff(callback.location()),
            &verifier,
            Client::Web,
            cookie,
        )
        .await
    }

    /// Signs up a new account with Google (registration open) and returns its session cookie.
    async fn provider_only_account(&self, sub: &'static str, email: &'static str) -> String {
        self.database
            .execute("UPDATE instance_settings SET registration_open = 1 WHERE id = 1")
            .await
            .unwrap();
        let reply = self
            .flow("google", "sign_in", google(sub, email), None)
            .await;
        assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
        reply.session_cookie()
    }
}

fn web_handoff(location: &str) -> String {
    let fragment = location
        .strip_prefix(&format!("{ORIGIN}/login/callback#"))
        .unwrap_or_else(|| panic!("not the web return page: {location}"));
    url::form_urlencoded::parse(fragment.as_bytes())
        .find(|(key, _)| key == "handoff")
        .unwrap()
        .1
        .into_owned()
}

#[tokio::test]
async fn admin_saves_providers_with_an_encrypted_secret_that_an_empty_save_keeps() {
    let fixture = Fixture::new().await;
    let options = fixture
        .send("GET", "/api/v1/auth/options", None, false, None)
        .await;
    assert_eq!(options.body["providers"], json!(["github", "google"]));
    assert_eq!(options.body["provider_registration_open"], false);

    let stored = fixture
        .database
        .scalar::<Vec<u8>>("SELECT client_secret FROM sign_in_providers WHERE provider = 'google'")
        .await
        .unwrap();
    assert_eq!(stored.len(), 12 + CLIENT_SECRET.len() + 16);
    assert!(
        !stored
            .windows(CLIENT_SECRET.len())
            .any(|window| window == CLIENT_SECRET.as_bytes())
    );

    let saved = fixture
        .admin_save(
            "google",
            json!({"client_id": "google-client-2", "client_secret": ""}),
        )
        .await;
    assert_eq!(saved.status, StatusCode::OK, "{}", saved.body);
    assert_eq!(
        saved.body["sign_in_providers"],
        json!([
            {"provider": "github", "client_id": "github-client", "secret_set": true,
             "callback_url": "https://orbit.test/api/v1/auth/providers/github/callback"},
            {"provider": "google", "client_id": "google-client-2", "secret_set": true,
             "callback_url": "https://orbit.test/api/v1/auth/providers/google/callback"},
        ])
    );
    assert_eq!(
        fixture
            .database
            .scalar::<Vec<u8>>(
                "SELECT client_secret FROM sign_in_providers WHERE provider = 'google'"
            )
            .await
            .unwrap(),
        stored
    );
    // The kept secret still opens: the fake provider only answers to the right one.
    let signed_in = fixture
        .flow(
            "google",
            "sign_in",
            google("g-owner", "owner@example.com"),
            None,
        )
        .await;
    assert_eq!(signed_in.status, StatusCode::OK, "{}", signed_in.body);

    let removed = fixture
        .send(
            "DELETE",
            "/api/v1/admin/settings/sign-in-providers/github",
            Some(&fixture.owner_cookie),
            true,
            None,
        )
        .await;
    assert_eq!(removed.status, StatusCode::OK);
    let first_save = fixture
        .admin_save("github", json!({"client_id": "github-client"}))
        .await;
    assert_eq!(first_save.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(first_save.code(), "invalid_sign_in_provider");
    let options = fixture
        .send("GET", "/api/v1/auth/options", None, false, None)
        .await;
    assert_eq!(options.body["providers"], json!(["google"]));
    let start = fixture
        .start(
            "github",
            json!({"intent": "sign_in", "code_challenge": challenge("x"), "redirect": "web"}),
            None,
        )
        .await;
    assert_eq!(start.status, StatusCode::NOT_FOUND);
    assert_eq!(start.code(), "provider_not_configured");
    assert_eq!(
        fixture
            .scalar_i64(
                "SELECT COUNT(*) FROM audit_events WHERE action = 'instance.sign_in_provider_saved'"
            )
            .await,
        3
    );
}

#[tokio::test]
async fn a_verified_email_links_once_and_the_subject_signs_in_after() {
    let fixture = Fixture::new().await;
    let first = fixture
        .flow(
            "google",
            "sign_in",
            google("g-owner", "Owner@Example.com"),
            None,
        )
        .await;
    assert_eq!(first.status, StatusCode::OK, "{}", first.body);
    assert_eq!(first.body["user"]["email"], "owner@example.com");
    assert_eq!(first.body["user"]["has_password"], true);
    assert!(first.body.get("session_token").is_none());
    assert!(first.session_cookie().starts_with("__Host-orbit_session="));
    let owner_id = first.body["user"]["id"].as_str().unwrap().to_owned();

    // The provider email changed; the subject still finds the account.
    let second = fixture
        .flow(
            "google",
            "sign_in",
            google("g-owner", "renamed@gmail.com"),
            None,
        )
        .await;
    assert_eq!(second.status, StatusCode::OK, "{}", second.body);
    assert_eq!(second.body["user"]["id"], owner_id.as_str());
    assert_eq!(
        fixture
            .scalar_i64("SELECT COUNT(*) FROM user_identities")
            .await,
        1
    );
    assert_eq!(
        fixture
            .scalar_i64(
                "SELECT COUNT(*) FROM audit_events WHERE action = 'authentication.login' \
                 AND outcome = 'success' AND metadata_json LIKE '%google%'"
            )
            .await,
        2
    );

    fixture
        .database
        .execute("UPDATE users SET suspended_at = 1")
        .await
        .unwrap();
    let suspended = fixture
        .flow(
            "google",
            "sign_in",
            google("g-owner", "owner@example.com"),
            None,
        )
        .await;
    assert_eq!(suspended.status, StatusCode::FORBIDDEN);
    assert_eq!(suspended.code(), "account_suspended");
}

#[tokio::test]
async fn an_unverified_email_is_never_linked() {
    let fixture = Fixture::new().await;
    let reply = fixture
        .flow(
            "google",
            "sign_in",
            FakeUser::Google {
                sub: "g-1",
                email: "owner@example.com",
                verified: false,
            },
            None,
        )
        .await;
    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(reply.code(), "provider_email_unverified");

    let reply = fixture
        .flow(
            "github",
            "sign_in",
            FakeUser::Github {
                id: 42,
                login: "octocat",
                emails: vec![
                    ("owner@example.com", true, false),
                    ("owner@example.com", false, true),
                ],
            },
            None,
        )
        .await;
    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(reply.code(), "provider_email_unverified");
    assert_eq!(
        fixture
            .scalar_i64("SELECT COUNT(*) FROM user_identities")
            .await,
        0
    );
}

#[tokio::test]
async fn a_new_account_needs_open_registration_and_has_no_password() {
    let fixture = Fixture::new().await;
    let closed = fixture
        .flow("github", "sign_in", github(7, "new@example.com"), None)
        .await;
    assert_eq!(closed.status, StatusCode::FORBIDDEN);
    assert_eq!(closed.code(), "registration_closed");
    assert_eq!(fixture.scalar_i64("SELECT COUNT(*) FROM users").await, 1);

    fixture
        .database
        .execute("UPDATE instance_settings SET registration_open = 1 WHERE id = 1")
        .await
        .unwrap();
    let options = fixture
        .send("GET", "/api/v1/auth/options", None, false, None)
        .await;
    assert_eq!(options.body["registration_open"], false);
    assert_eq!(options.body["provider_registration_open"], true);
    let opened = fixture
        .flow("github", "sign_in", github(7, "New@Example.com"), None)
        .await;
    assert_eq!(opened.status, StatusCode::OK, "{}", opened.body);
    assert_eq!(opened.body["user"]["email"], "new@example.com");
    assert_eq!(opened.body["user"]["display_name"], "octocat");
    assert_eq!(opened.body["user"]["has_password"], false);
    assert_eq!(
        fixture
            .database
            .scalar::<String>("SELECT password_hash FROM users WHERE email = 'new@example.com'")
            .await
            .unwrap(),
        "!"
    );
    assert_eq!(
        fixture
            .scalar_i64(
                "SELECT COUNT(*) FROM users WHERE email = 'new@example.com' AND email_verified_at IS NOT NULL"
            )
            .await,
        1
    );
    assert_eq!(
        fixture
            .database
            .scalar::<String>(
                "SELECT metadata_json FROM audit_events WHERE action = 'account.registered'"
            )
            .await
            .unwrap(),
        r#"{"method":"github"}"#
    );

    // No password: password sign-in fails like a wrong password.
    let login = fixture
        .send(
            "POST",
            "/api/v1/auth/login",
            None,
            false,
            Some(json!({"email": "new@example.com", "password": "!"})),
        )
        .await;
    assert_eq!(login.status, StatusCode::UNAUTHORIZED);
    assert_eq!(login.code(), "invalid_credentials");

    // A fresh session sets a first password without a current one; then password sign-in works.
    let cookie = opened.session_cookie();
    let set = fixture
        .send(
            "POST",
            "/api/v1/auth/password",
            Some(&cookie),
            true,
            Some(json!({"new_password": "a brand new horse battery"})),
        )
        .await;
    assert_eq!(set.status, StatusCode::NO_CONTENT, "{}", set.body);
    let login = fixture
        .send(
            "POST",
            "/api/v1/auth/login",
            None,
            false,
            Some(json!({"email": "new@example.com", "password": "a brand new horse battery"})),
        )
        .await;
    assert_eq!(login.status, StatusCode::OK, "{}", login.body);
}

#[tokio::test]
async fn two_factor_runs_before_a_new_link_is_written() {
    let fixture = Fixture::new().await;
    let setup = fixture
        .send(
            "POST",
            "/api/v1/auth/two-factor/totp/setup",
            Some(&fixture.owner_cookie),
            true,
            Some(json!({"password": PASSWORD})),
        )
        .await;
    assert_eq!(setup.status, StatusCode::OK, "{}", setup.body);
    let secret = setup.body["secret"].as_str().unwrap().to_owned();
    let enabled = fixture
        .send(
            "POST",
            "/api/v1/auth/two-factor/totp/enable",
            Some(&fixture.owner_cookie),
            true,
            Some(json!({"code": totp_code(&secret, -1)})),
        )
        .await;
    assert_eq!(enabled.status, StatusCode::OK, "{}", enabled.body);

    let challenge = fixture
        .flow(
            "google",
            "sign_in",
            google("g-owner", "owner@example.com"),
            None,
        )
        .await;
    assert_eq!(challenge.status, StatusCode::ACCEPTED, "{}", challenge.body);
    assert_eq!(
        fixture
            .scalar_i64("SELECT COUNT(*) FROM user_identities")
            .await,
        0
    );
    let token = challenge.body["two_factor_token"].as_str().unwrap();
    let wrong = fixture
        .send(
            "POST",
            "/api/v1/auth/login/two-factor",
            None,
            true,
            Some(json!({"two_factor_token": token, "code": "000000"})),
        )
        .await;
    assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);
    assert_eq!(wrong.code(), "invalid_two_factor_code");
    assert_eq!(
        fixture
            .scalar_i64("SELECT COUNT(*) FROM user_identities")
            .await,
        0
    );
    let right = fixture
        .send(
            "POST",
            "/api/v1/auth/login/two-factor",
            None,
            true,
            Some(json!({"two_factor_token": token, "code": totp_code(&secret, 0)})),
        )
        .await;
    assert_eq!(right.status, StatusCode::OK, "{}", right.body);
    assert_eq!(right.body["user"]["email"], "owner@example.com");
    assert_eq!(
        fixture
            .database
            .scalar::<String>("SELECT provider || ':' || subject FROM user_identities")
            .await
            .unwrap(),
        "google:g-owner"
    );
}

#[tokio::test]
async fn connect_links_to_the_session_that_started_it() {
    let fixture = Fixture::new().await;
    let owner = fixture.owner_cookie.clone();
    let connected = fixture
        .flow(
            "github",
            "connect",
            github(42, "octo@users.example"),
            Some(&owner),
        )
        .await;
    assert_eq!(
        connected.status,
        StatusCode::NO_CONTENT,
        "{}",
        connected.body
    );
    let accounts = fixture
        .send(
            "GET",
            "/api/v1/auth/connected-accounts",
            Some(&owner),
            false,
            None,
        )
        .await;
    assert_eq!(accounts.status, StatusCode::OK);
    assert_eq!(accounts.body[0]["provider"], "github");
    assert_eq!(accounts.body[0]["email"], "octo@users.example");
    assert_eq!(accounts.body.as_array().unwrap().len(), 1);

    let other = fixture
        .provider_only_account("g-other", "other@example.com")
        .await;
    let in_use = fixture
        .flow(
            "github",
            "connect",
            github(42, "octo@users.example"),
            Some(&other),
        )
        .await;
    assert_eq!(in_use.status, StatusCode::CONFLICT);
    assert_eq!(in_use.code(), "identity_in_use");

    // Started by one session, finished by another session of the same account.
    let (callback, verifier) = fixture
        .through_callback(
            "google",
            "connect",
            "web",
            google("g-owner", "o@gmail.com"),
            Some(&owner),
        )
        .await;
    let second = fixture
        .send(
            "POST",
            "/api/v1/auth/login",
            None,
            true,
            Some(json!({"email": "owner@example.com", "password": PASSWORD})),
        )
        .await
        .session_cookie();
    let mismatch = fixture
        .exchange(
            &web_handoff(callback.location()),
            &verifier,
            Client::Web,
            Some(&second),
        )
        .await;
    assert_eq!(mismatch.status, StatusCode::FORBIDDEN);
    assert_eq!(mismatch.code(), "session_mismatch");
    assert_eq!(
        fixture
            .scalar_i64("SELECT COUNT(*) FROM user_identities WHERE provider = 'google' AND subject = 'g-owner'")
            .await,
        0
    );
}

#[tokio::test]
async fn connect_confirms_the_password_or_a_fresh_sign_in_first() {
    let fixture = Fixture::new().await;
    let owner = fixture.owner_cookie.clone();
    let start = |password: Option<&'static str>, cookie: String| {
        let fixture = &fixture;
        async move {
            let body = json!({"intent": "connect", "code_challenge": challenge("verifier-connect"), "redirect": "web", "password": password});
            fixture.start("github", body, Some(&cookie)).await
        }
    };

    let missing = start(None, owner.clone()).await;
    assert_eq!(missing.status, StatusCode::UNAUTHORIZED);
    assert_eq!(missing.code(), "password_required");
    let wrong = start(Some("not the password"), owner.clone()).await;
    assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);
    assert_eq!(wrong.code(), "invalid_credentials");
    let wrong_passkey = fixture
        .send(
            "POST",
            "/api/v1/auth/passkeys/register/start",
            Some(&owner),
            true,
            Some(json!({"password": "not the password"})),
        )
        .await;
    assert_eq!(wrong_passkey.code(), wrong.code());
    let right = start(Some(PASSWORD), owner.clone()).await;
    assert_eq!(right.status, StatusCode::OK, "{}", right.body);
    assert!(right.body["authorize_url"].is_string());

    let solo = fixture
        .provider_only_account("g-solo", "solo@example.com")
        .await;
    let fresh = start(None, solo.clone()).await;
    assert_eq!(fresh.status, StatusCode::OK, "{}", fresh.body);
    let eleven_minutes_ago = TimestampMillis::now().as_millis() - 11 * 60 * 1_000;
    fixture
        .database
        .execute(&format!(
            "UPDATE sessions SET created_at = {eleven_minutes_ago}"
        ))
        .await
        .unwrap();
    let stale = start(None, solo).await;
    assert_eq!(stale.status, StatusCode::UNAUTHORIZED);
    assert_eq!(stale.code(), "reauthentication_required");
}

#[tokio::test]
async fn sign_in_options_hide_a_provider_whose_secret_another_app_key_saved() {
    let fixture = Fixture::new().await;
    let identity = Arc::new(IdentityRepository::new((*fixture.database).clone()));
    let options = |app_key: [u8; 32]| {
        let auth = AuthState::new(Arc::clone(&identity), CookieMode::secure())
            .with_app_key(app_key)
            .with_public_origin(ORIGIN);
        async move {
            let request = Request::get("/api/v1/auth/options")
                .body(Body::empty())
                .unwrap();
            let response = auth_router(auth).oneshot(request).await.unwrap();
            let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            serde_json::from_slice::<Value>(&bytes).unwrap()
        }
    };
    assert_eq!(
        options(APP_KEY).await["providers"],
        json!(["github", "google"])
    );
    let rotated = options([9; 32]).await;
    assert_eq!(rotated["providers"], json!([]));
    assert_eq!(rotated["provider_registration_open"], false);
}

#[tokio::test]
async fn the_last_sign_in_method_cannot_be_removed() {
    let fixture = Fixture::new().await;
    let cookie = fixture
        .provider_only_account("g-solo", "solo@example.com")
        .await;
    let last = fixture
        .send(
            "DELETE",
            "/api/v1/auth/connected-accounts/google",
            Some(&cookie),
            true,
            None,
        )
        .await;
    assert_eq!(last.status, StatusCode::CONFLICT);
    assert_eq!(last.code(), "last_sign_in_method");

    let user_id = fixture
        .database
        .scalar::<String>("SELECT id FROM users WHERE email = 'solo@example.com'")
        .await
        .unwrap();
    let passkey_id = orbit_platform::Id::new_v7();
    fixture
        .database
        .execute(&format!(
            "INSERT INTO user_passkeys (id, user_id, credential_id, name, passkey, created_at) \
             VALUES ('{passkey_id}', '{user_id}', 'credential-1', 'Laptop', '{{}}', 1)"
        ))
        .await
        .unwrap();
    let removed = fixture
        .send(
            "DELETE",
            "/api/v1/auth/connected-accounts/google",
            Some(&cookie),
            true,
            None,
        )
        .await;
    assert_eq!(removed.status, StatusCode::NO_CONTENT, "{}", removed.body);
    let passkey = fixture
        .send(
            "DELETE",
            &format!("/api/v1/auth/passkeys/{passkey_id}"),
            Some(&cookie),
            true,
            None,
        )
        .await;
    assert_eq!(passkey.status, StatusCode::CONFLICT);
    assert_eq!(passkey.code(), "last_sign_in_method");
    assert_eq!(
        fixture
            .scalar_i64("SELECT COUNT(*) FROM user_passkeys")
            .await,
        1
    );
    let missing = fixture
        .send(
            "DELETE",
            "/api/v1/auth/connected-accounts/google",
            Some(&cookie),
            true,
            None,
        )
        .await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    assert_eq!(missing.code(), "identity_not_found");
}

#[tokio::test]
async fn a_handoff_is_single_use_and_needs_the_starting_verifier() {
    let fixture = Fixture::new().await;
    let (callback, verifier) = fixture
        .through_callback(
            "google",
            "sign_in",
            "web",
            google("g-owner", "owner@example.com"),
            None,
        )
        .await;
    assert_eq!(callback.headers[header::CACHE_CONTROL], "no-store");
    assert_eq!(callback.headers[header::REFERRER_POLICY], "no-referrer");
    let handoff = web_handoff(callback.location());
    let wrong = fixture
        .exchange(&handoff, "someone-elses-verifier", Client::Web, None)
        .await;
    assert_eq!(wrong.status, StatusCode::BAD_REQUEST);
    assert_eq!(wrong.code(), "invalid_handoff");
    let spent = fixture
        .exchange(&handoff, &verifier, Client::Web, None)
        .await;
    assert_eq!(spent.status, StatusCode::BAD_REQUEST);
    assert_eq!(spent.code(), "invalid_handoff");
    assert_eq!(fixture.scalar_i64("SELECT COUNT(*) FROM sessions").await, 1);

    let denied_start = fixture
        .start(
            "google",
            json!({"intent": "sign_in", "code_challenge": challenge("v"), "redirect": "web", "return_to": "/oauth/authorize?x=1"}),
            None,
        )
        .await;
    let url = url::Url::parse(denied_start.body["authorize_url"].as_str().unwrap()).unwrap();
    let state = url
        .query_pairs()
        .find(|(key, _)| key == "state")
        .unwrap()
        .1
        .into_owned();
    let denied = fixture
        .send(
            "GET",
            &format!("/api/v1/auth/providers/google/callback?state={state}&error=access_denied"),
            None,
            false,
            None,
        )
        .await;
    assert!(
        denied
            .location()
            .ends_with("&return_to=%2Foauth%2Fauthorize%3Fx%3D1")
    );
    let reply = fixture
        .exchange(&web_handoff(denied.location()), "v", Client::Web, None)
        .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST);
    assert_eq!(reply.code(), "provider_denied");
}

#[tokio::test]
async fn an_unknown_or_expired_state_returns_to_the_sign_in_page() {
    let fixture = Fixture::new().await;
    let expired = format!("{ORIGIN}/login?sign_in_error=provider_expired");
    let unknown = fixture
        .send(
            "GET",
            "/api/v1/auth/providers/google/callback?state=nope&code=c",
            None,
            false,
            None,
        )
        .await;
    assert_eq!(unknown.status, StatusCode::SEE_OTHER);
    assert_eq!(unknown.location(), expired);

    let started = fixture
        .start(
            "google",
            json!({"intent": "sign_in", "code_challenge": challenge("v"), "redirect": "web"}),
            None,
        )
        .await;
    let callback = fixture.fake.consent(
        started.body["authorize_url"].as_str().unwrap(),
        google("g-owner", "owner@example.com"),
    );
    fixture
        .database
        .execute("UPDATE auth_pending SET expires_at = 1")
        .await
        .unwrap();
    let reply = fixture.send("GET", &callback, None, false, None).await;
    assert_eq!(reply.status, StatusCode::SEE_OTHER);
    assert_eq!(reply.location(), expired);
}

#[tokio::test]
async fn native_sign_in_returns_to_an_allowlisted_app_uri_and_gets_a_token() {
    let fixture = Fixture::new().await;
    let (callback, verifier) = fixture
        .through_callback(
            "google",
            "sign_in",
            "orbit://auth/callback",
            google("g-owner", "owner@example.com"),
            None,
        )
        .await;
    let handoff = callback
        .location()
        .strip_prefix("orbit://auth/callback?handoff=")
        .unwrap()
        .to_owned();
    let reply = fixture
        .exchange(&handoff, &verifier, Client::Native, None)
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert!(reply.headers.get(header::SET_COOKIE).is_none());
    let token = reply.body["session_token"].as_str().unwrap();
    let me = fixture
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/auth/me")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(me.status(), StatusCode::OK);

    let (expo, _) = fixture
        .through_callback(
            "google",
            "sign_in",
            EXPO_REDIRECT,
            google("g-owner", "owner@example.com"),
            None,
        )
        .await;
    assert!(
        expo.location()
            .starts_with(&format!("{EXPO_REDIRECT}?handoff="))
    );

    for redirect in [
        "https://evil.example/auth/callback",
        "orbit://auth/callback/",
        "exp://10.0.0.9:8082/--/auth/callback",
    ] {
        let refused = fixture
            .start(
                "google",
                json!({"intent": "sign_in", "code_challenge": challenge("v"), "redirect": redirect}),
                None,
            )
            .await;
        assert_eq!(
            refused.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{redirect}"
        );
        assert_eq!(refused.code(), "invalid_redirect");
    }
}

#[tokio::test]
async fn a_provider_confirmation_unlocks_security_changes_for_a_password_less_account() {
    let fixture = Fixture::new().await;
    let cookie = fixture
        .provider_only_account("g-solo", "solo@example.com")
        .await;
    // Right after the sign-in, the session is fresh enough.
    let fresh = fixture
        .send(
            "POST",
            "/api/v1/auth/two-factor/totp/setup",
            Some(&cookie),
            true,
            Some(json!({})),
        )
        .await;
    assert_eq!(fresh.status, StatusCode::OK, "{}", fresh.body);

    let eleven_minutes_ago = TimestampMillis::now().as_millis() - 11 * 60 * 1_000;
    fixture
        .database
        .execute(&format!(
            "UPDATE sessions SET created_at = {eleven_minutes_ago}"
        ))
        .await
        .unwrap();
    let stale = fixture
        .send(
            "POST",
            "/api/v1/auth/two-factor/totp/setup",
            Some(&cookie),
            true,
            Some(json!({})),
        )
        .await;
    assert_eq!(stale.status, StatusCode::UNAUTHORIZED);
    assert_eq!(stale.code(), "reauthentication_required");

    let other_account = fixture
        .flow(
            "google",
            "confirm",
            google("g-someone-else", "x@example.com"),
            Some(&cookie),
        )
        .await;
    assert_eq!(other_account.status, StatusCode::FORBIDDEN);
    assert_eq!(other_account.code(), "identity_mismatch");

    let confirmed = fixture
        .flow(
            "google",
            "confirm",
            google("g-solo", "solo@example.com"),
            Some(&cookie),
        )
        .await;
    assert_eq!(
        confirmed.status,
        StatusCode::NO_CONTENT,
        "{}",
        confirmed.body
    );
    assert_eq!(
        fixture
            .scalar_i64("SELECT COUNT(*) FROM sessions WHERE reauthenticated_at IS NOT NULL")
            .await,
        1
    );
    let unlocked = fixture
        .send(
            "POST",
            "/api/v1/auth/two-factor/totp/setup",
            Some(&cookie),
            true,
            Some(json!({})),
        )
        .await;
    assert_eq!(unlocked.status, StatusCode::OK, "{}", unlocked.body);

    // An account with a password still has to send it.
    let owner = fixture
        .send(
            "POST",
            "/api/v1/auth/two-factor/totp/setup",
            Some(&fixture.owner_cookie),
            true,
            Some(json!({})),
        )
        .await;
    assert_eq!(owner.status, StatusCode::UNAUTHORIZED);
    assert_eq!(owner.code(), "password_required");
}
