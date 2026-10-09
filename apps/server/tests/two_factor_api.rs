use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use orbit_platform::{AuthenticatedUser, Id, PasswordService, TestDatabase, TimestampMillis};
use orbit_server::auth_routes::{AuthState, CookieMode, auth_router};
use orbit_server::repositories::identity::{AdminAccountError, IdentityRepository, SetupRequest};
use serde_json::{Value, json};
use totp_rs::{Builder, Secret};
use tower::ServiceExt;

const PASSWORD: &str = "correct horse battery";

struct Fixture {
    _database: TestDatabase,
    identity: Arc<IdentityRepository>,
    app: axum::Router,
    cookie: String,
}

impl Fixture {
    async fn new() -> Self {
        Self::with_state(|state| state.with_app_key([7; 32])).await
    }

    async fn with_state(configure: impl FnOnce(AuthState) -> AuthState) -> Self {
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
        let app = auth_router(configure(AuthState::new(
            Arc::clone(&identity),
            CookieMode::secure(),
        )));
        Self {
            _database: database,
            identity,
            app,
            cookie: format!("__Host-orbit_session={}", setup.session.token),
        }
    }

    async fn post(&self, path: &str, cookie: Option<&str>, body: Value) -> (StatusCode, Value) {
        let mut builder = Request::builder()
            .method("POST")
            .uri(path)
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(cookie) = cookie {
            builder = builder.header(header::COOKIE, cookie);
        }
        self.send(builder.body(Body::from(body.to_string())).unwrap())
            .await
    }

    async fn get(&self, path: &str, cookie: &str) -> (StatusCode, Value) {
        let request = Request::builder()
            .uri(path)
            .header(header::COOKIE, cookie)
            .body(Body::empty())
            .unwrap();
        self.send(request).await
    }

    async fn send(&self, request: Request<Body>) -> (StatusCode, Value) {
        let response = self.app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn login(&self) -> (StatusCode, Value) {
        self.post(
            "/api/v1/auth/login",
            None,
            json!({"email": "owner@example.com", "password": PASSWORD}),
        )
        .await
    }

    /// Sets up and turns on the authenticator app; returns its secret and the recovery codes.
    async fn enable_totp(&self) -> (String, Vec<String>) {
        let (status, setup) = self
            .post(
                "/api/v1/auth/two-factor/totp/setup",
                Some(&self.cookie),
                json!({"password": PASSWORD}),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{setup}");
        assert!(
            setup["otpauth_url"]
                .as_str()
                .unwrap()
                .starts_with("otpauth://totp/Orbit:owner%40example.com?")
        );
        assert!(
            setup["qr_code"]
                .as_str()
                .unwrap()
                .starts_with("data:image/png;base64,")
        );
        let secret = setup["secret"].as_str().unwrap().to_owned();
        // The previous step: the one-step skew accepts it, and the current step stays unused for a sign-in.
        let (status, enabled) = self
            .post(
                "/api/v1/auth/two-factor/totp/enable",
                Some(&self.cookie),
                json!({"code": code(&secret, -1)}),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{enabled}");
        let codes = enabled["recovery_codes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|code| code.as_str().unwrap().to_owned())
            .collect();
        (secret, codes)
    }
}

/// The app code `steps` 30-second steps away from now.
fn code(secret: &str, steps: i64) -> String {
    let totp = Builder::new()
        .with_secret(Secret::try_from_base32(secret).unwrap())
        .build()
        .unwrap();
    let now = TimestampMillis::now().as_millis() / 1_000;
    totp.generate(u64::try_from(now + steps * 30).unwrap())
        .to_string()
}

#[tokio::test]
async fn sign_in_asks_for_a_code_once_the_authenticator_app_is_on() {
    let fixture = Fixture::new().await;
    let (status, problem) = fixture
        .post(
            "/api/v1/auth/two-factor/totp/setup",
            Some(&fixture.cookie),
            json!({"password": "wrong password value"}),
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(problem["code"], "invalid_credentials");

    // Setup alone changes nothing: the password still signs in.
    let (status, setup) = fixture
        .post(
            "/api/v1/auth/two-factor/totp/setup",
            Some(&fixture.cookie),
            json!({"password": PASSWORD}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, problem) = fixture
        .post(
            "/api/v1/auth/two-factor/totp/enable",
            Some(&fixture.cookie),
            json!({"code": if code(setup["secret"].as_str().unwrap(), 0) == "000000" { "111111" } else { "000000" }}),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(problem["code"], "invalid_two_factor_code");
    assert_eq!(fixture.login().await.0, StatusCode::OK);

    let (secret, codes) = fixture.enable_totp().await;
    assert_eq!(codes.len(), 10);
    let (_, status_body) = fixture
        .get("/api/v1/auth/two-factor", &fixture.cookie)
        .await;
    assert_eq!(
        status_body,
        json!({"totp_enabled": true, "recovery_codes_left": 10})
    );

    let (status, challenge) = fixture.login().await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let token = challenge["two_factor_token"].as_str().unwrap();

    // The code that turned the app on cannot sign in again.
    let (status, problem) = fixture
        .post(
            "/api/v1/auth/login/two-factor",
            None,
            json!({"two_factor_token": token, "code": code(&secret, -1)}),
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(problem["code"], "invalid_two_factor_code");

    // The challenge stays open after a wrong code; the right one signs in.
    let (status, signed_in) = fixture
        .post(
            "/api/v1/auth/login/two-factor",
            None,
            json!({"two_factor_token": token, "code": code(&secret, 0)}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{signed_in}");
    assert_eq!(signed_in["user"]["email"], "owner@example.com");

    // The challenge is used up.
    let (status, problem) = fixture
        .post(
            "/api/v1/auth/login/two-factor",
            None,
            json!({"two_factor_token": token, "code": code(&secret, 0)}),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(problem["code"], "invalid_two_factor_token");
}

#[tokio::test]
async fn a_recovery_code_signs_in_once() {
    let fixture = Fixture::new().await;
    let (_, codes) = fixture.enable_totp().await;
    for expected in [StatusCode::OK, StatusCode::UNAUTHORIZED] {
        let (_, challenge) = fixture.login().await;
        let (status, _) = fixture
            .post(
                "/api/v1/auth/login/two-factor",
                None,
                // Typed loosely: upper case and spaces around it.
                json!({
                    "two_factor_token": challenge["two_factor_token"],
                    "code": format!(" {} ", codes[0].to_uppercase()),
                }),
            )
            .await;
        assert_eq!(status, expected);
    }
    let (_, status_body) = fixture
        .get("/api/v1/auth/two-factor", &fixture.cookie)
        .await;
    assert_eq!(status_body["recovery_codes_left"], 9);
}

#[tokio::test]
async fn wrong_codes_end_the_challenge_after_five_tries() {
    let fixture = Fixture::new().await;
    let (secret, _) = fixture.enable_totp().await;
    let (_, challenge) = fixture.login().await;
    let wrong = if code(&secret, 0) == "123456" {
        "654321"
    } else {
        "123456"
    };
    for _ in 0..5 {
        let (status, _) = fixture
            .post(
                "/api/v1/auth/login/two-factor",
                None,
                json!({"two_factor_token": challenge["two_factor_token"], "code": wrong}),
            )
            .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }
    let (status, problem) = fixture
        .post(
            "/api/v1/auth/login/two-factor",
            None,
            json!({"two_factor_token": challenge["two_factor_token"], "code": code(&secret, 0)}),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(problem["code"], "invalid_two_factor_token");
}

#[tokio::test]
async fn turning_two_factor_off_needs_the_password() {
    let fixture = Fixture::new().await;
    fixture.enable_totp().await;
    let (status, _) = fixture
        .post(
            "/api/v1/auth/two-factor/disable",
            Some(&fixture.cookie),
            json!({"password": "wrong password value"}),
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(fixture.login().await.0, StatusCode::ACCEPTED);

    let (status, _) = fixture
        .post(
            "/api/v1/auth/two-factor/disable",
            Some(&fixture.cookie),
            json!({"password": PASSWORD}),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(fixture.login().await.0, StatusCode::OK);
    let (_, status_body) = fixture
        .get("/api/v1/auth/two-factor", &fixture.cookie)
        .await;
    assert_eq!(
        status_body,
        json!({"totp_enabled": false, "recovery_codes_left": 0})
    );
}

#[tokio::test]
async fn an_admin_resets_another_users_two_factor() {
    let fixture = Fixture::new().await;
    let owner = fixture
        .identity
        .find_by_email("owner@example.com")
        .await
        .unwrap()
        .unwrap();
    let member = Id::new_v7();
    let now = TimestampMillis::now();
    sqlx::query(
        "INSERT INTO users (id, email, normalized_email, display_name, password_hash, created_at, updated_at) \
         VALUES (?, 'member@example.com', 'member@example.com', 'Member', 'unused', ?, ?)",
    )
    .bind(member.to_string())
    .bind(now.as_millis())
    .bind(now.as_millis())
    .execute(fixture.identity.database().pool())
    .await
    .unwrap();
    fixture
        .identity
        .begin_totp_setup(member, b"encrypted", now)
        .await
        .unwrap();
    fixture
        .identity
        .enable_totp(member, 1, "test", now)
        .await
        .unwrap()
        .unwrap();
    assert!(
        fixture
            .identity
            .two_factor_status(member)
            .await
            .unwrap()
            .totp_enabled
    );

    assert!(matches!(
        fixture
            .identity
            .reset_two_factor_as_admin(member, owner.id, "test")
            .await,
        Err(AdminAccountError::Forbidden)
    ));
    fixture
        .identity
        .reset_two_factor_as_admin(owner.id, member, "test")
        .await
        .unwrap();
    let status = fixture.identity.two_factor_status(member).await.unwrap();
    assert!(!status.totp_enabled);
    assert_eq!(status.recovery_codes_left, 0);
    // The member can sign in with the password alone again, so a session works as before.
    fixture
        .identity
        .create_session(
            &AuthenticatedUser {
                id: member,
                email: "member@example.com".to_owned(),
                display_name: "Member".to_owned(),
            },
            now,
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn passkeys_need_a_domain_name() {
    for (origin, enabled) in [
        (Some("https://orbit.test"), true),
        (Some("http://127.0.0.1:8080"), false),
        (None, false),
    ] {
        let fixture = Fixture::with_state(|state| match origin {
            Some(origin) => state.with_public_origin(origin),
            None => state,
        })
        .await;
        let (_, options) = fixture.get("/api/v1/auth/options", &fixture.cookie).await;
        assert_eq!(options["passkeys_enabled"], enabled, "{origin:?}");
        let (status, start) = fixture
            .post("/api/v1/auth/passkey/login/start", None, json!({}))
            .await;
        if !enabled {
            assert_eq!(status, StatusCode::CONFLICT);
            assert_eq!(start["code"], "passkeys_unavailable");
            continue;
        }
        assert_eq!(status, StatusCode::OK);
        assert_eq!(start["options"]["publicKey"]["rpId"], "orbit.test");
        assert_eq!(
            start["options"]["publicKey"]["userVerification"],
            "required"
        );
        assert!(start["options"].get("mediation").is_none());
        assert_eq!(start["options"]["publicKey"]["extensions"], json!({}));

        // An answer that is not a credential does not sign in, and the challenge is used up.
        let (status, problem) = fixture
            .post(
                "/api/v1/auth/passkey/login/finish",
                None,
                json!({"challenge_token": start["challenge_token"], "credential": {}}),
            )
            .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(problem["code"], "invalid_passkey");

        let (status, registration) = fixture
            .post(
                "/api/v1/auth/passkeys/register/start",
                Some(&fixture.cookie),
                json!({"password": PASSWORD}),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        let selection = &registration["options"]["publicKey"]["authenticatorSelection"];
        assert_eq!(selection["residentKey"], "preferred");
        assert_eq!(selection["userVerification"], "required");
        // Only the extensions every passkey provider understands.
        assert_eq!(
            registration["options"]["publicKey"]["extensions"],
            json!({"credProps": true})
        );
        assert_eq!(
            registration["options"]["publicKey"]["user"]["name"],
            "owner@example.com"
        );
    }
}
