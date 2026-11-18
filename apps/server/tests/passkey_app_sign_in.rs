//! The Orbit app's passkey sign-in, end to end over the auth router: the app starts with a PKCE challenge, the web
//! page proves the passkey, and only the app's verifier turns the handoff into a session. The passkeys come from a
//! software authenticator that signs like a real one.

use std::sync::Arc;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Request, StatusCode, header};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use orbit_platform::{PasswordService, TestDatabase, TimestampMillis};
use orbit_server::auth_routes::{AuthState, CookieMode, auth_router};
use orbit_server::repositories::identity::{IdentityRepository, SetupRequest};
use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tower::ServiceExt;

const ORIGIN: &str = "https://orbit.test";
const RP_ID: &str = "orbit.test";
const PASSWORD: &str = "correct horse battery";
const APP_REDIRECT: &str = "orbit://auth/callback";
const EXPO_REDIRECT: &str = "exp://100.64.0.5:8082/--/auth/callback";
const USER_PRESENT_AND_VERIFIED: u8 = 0x05;
const HAS_CREDENTIAL: u8 = 0x40;

fn b64(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

fn challenge(verifier: &str) -> String {
    b64(&Sha256::digest(verifier.as_bytes()))
}

/// A discoverable ES256 passkey that verifies the user, as a phone or a password manager makes.
struct Authenticator {
    key: SigningKey,
    credential_id: [u8; 16],
    user_handle: String,
    uses: u32,
}

impl Authenticator {
    fn new(seed: u8) -> Self {
        Self {
            key: SigningKey::from_slice(&[seed; 32]).unwrap(),
            credential_id: [seed; 16],
            user_handle: String::new(),
            uses: 0,
        }
    }

    fn client_data(kind: &str, options: &Value) -> Vec<u8> {
        json!({"type": kind, "challenge": options["publicKey"]["challenge"], "origin": ORIGIN, "crossOrigin": false})
            .to_string()
            .into_bytes()
    }

    fn authenticator_data(&mut self, flags: u8) -> Vec<u8> {
        self.uses += 1;
        let mut data = Sha256::digest(RP_ID.as_bytes()).to_vec();
        data.push(flags);
        data.extend(self.uses.to_be_bytes());
        data
    }

    /// The answer to `navigator.credentials.create`, with no attestation.
    fn create(&mut self, options: &Value) -> Value {
        self.user_handle = options["publicKey"]["user"]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let point = self.key.verifying_key().to_sec1_bytes();
        let (x, y) = point[1..].split_at(32);
        // COSE_Key: EC2, ES256, P-256, x, y.
        let mut cose = vec![0xa5, 0x01, 0x02, 0x03, 0x26, 0x20, 0x01, 0x21, 0x58, 0x20];
        cose.extend(x);
        cose.extend([0x22, 0x58, 0x20]);
        cose.extend(y);
        let mut data = self.authenticator_data(USER_PRESENT_AND_VERIFIED | HAS_CREDENTIAL);
        data.extend([0; 16]);
        data.extend(
            u16::try_from(self.credential_id.len())
                .unwrap()
                .to_be_bytes(),
        );
        data.extend(self.credential_id);
        data.extend(cose);
        // CBOR: {"fmt": "none", "attStmt": {}, "authData": bytes}
        let mut attestation = vec![0xa3, 0x63];
        attestation.extend(b"fmt");
        attestation.push(0x64);
        attestation.extend(b"none");
        attestation.push(0x67);
        attestation.extend(b"attStmt");
        attestation.extend([0xa0, 0x68]);
        attestation.extend(b"authData");
        attestation.extend([0x58, u8::try_from(data.len()).unwrap()]);
        attestation.extend(data);
        json!({
            "id": b64(&self.credential_id),
            "rawId": b64(&self.credential_id),
            "type": "public-key",
            "response": {
                "clientDataJSON": b64(&Self::client_data("webauthn.create", options)),
                "attestationObject": b64(&attestation),
            },
            "clientExtensionResults": {"credProps": {"rk": true}},
        })
    }

    /// The answer to `navigator.credentials.get`.
    fn get(&mut self, options: &Value) -> Value {
        let client_data = Self::client_data("webauthn.get", options);
        let data = self.authenticator_data(USER_PRESENT_AND_VERIFIED);
        let mut signed = data.clone();
        signed.extend(Sha256::digest(&client_data));
        let signature: Signature = self.key.sign(&signed);
        json!({
            "id": b64(&self.credential_id),
            "rawId": b64(&self.credential_id),
            "type": "public-key",
            "response": {
                "clientDataJSON": b64(&client_data),
                "authenticatorData": b64(&data),
                "signature": b64(signature.to_der().as_bytes()),
                "userHandle": self.user_handle,
            },
            "clientExtensionResults": {},
        })
    }
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
}

struct Fixture {
    database: TestDatabase,
    app: Router,
    owner_cookie: String,
}

impl Fixture {
    async fn new() -> Self {
        Self::at(ORIGIN).await
    }

    async fn at(origin: &str) -> Self {
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
        let auth = AuthState::new(identity, CookieMode::secure())
            .with_public_origin(origin)
            .with_native_redirects(&[EXPO_REDIRECT.to_owned()]);
        Self {
            database,
            app: auth_router(auth),
            owner_cookie: format!("__Host-orbit_session={}", setup.session.token),
        }
    }

    /// `browser`: with Origin, as the web page sends; the app sends none.
    async fn post(&self, path: &str, cookie: Option<&str>, browser: bool, body: Value) -> Reply {
        let mut request = Request::builder()
            .method("POST")
            .uri(path)
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(cookie) = cookie {
            request = request.header(header::COOKIE, cookie);
        }
        if browser {
            request = request.header(header::ORIGIN, ORIGIN);
        }
        let request = request.body(Body::from(body.to_string())).unwrap();
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

    async fn count(&self, sql: &str) -> i64 {
        self.database.scalar::<i64>(sql).await.unwrap()
    }

    /// The owner adds a passkey in the account settings.
    async fn owner_passkey(&self, seed: u8) -> Authenticator {
        let mut authenticator = Authenticator::new(seed);
        let start = self
            .post(
                "/api/v1/auth/passkeys/register/start",
                Some(&self.owner_cookie),
                true,
                json!({"password": PASSWORD}),
            )
            .await;
        assert_eq!(start.status, StatusCode::OK, "{}", start.body);
        let saved = self
            .post(
                "/api/v1/auth/passkeys/register/finish",
                Some(&self.owner_cookie),
                true,
                json!({
                    "challenge_token": start.body["challenge_token"],
                    "name": "Phone",
                    "credential": authenticator.create(&start.body["options"]),
                }),
            )
            .await;
        assert_eq!(saved.status, StatusCode::CREATED, "{}", saved.body);
        authenticator
    }

    /// The app starts; returns the flow token of the page it opens.
    async fn app_start(&self, verifier: &str, redirect: &str) -> String {
        let reply = self
            .post(
                "/api/v1/auth/passkey/app/start",
                None,
                false,
                json!({"code_challenge": challenge(verifier), "redirect": redirect}),
            )
            .await;
        assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
        reply.body["authorize_url"]
            .as_str()
            .unwrap()
            .strip_prefix(&format!("{ORIGIN}/login/app#flow="))
            .unwrap()
            .to_owned()
    }

    /// The web page: asks for a passkey and sends the answer with the flow token.
    async fn page_finish(
        &self,
        flow: &str,
        authenticator: &mut Authenticator,
        cookie: Option<&str>,
    ) -> Reply {
        let start = self
            .post("/api/v1/auth/passkey/login/start", None, true, json!({}))
            .await;
        assert_eq!(start.status, StatusCode::OK, "{}", start.body);
        self.post(
            "/api/v1/auth/passkey/app/finish",
            cookie,
            true,
            json!({
                "flow": flow,
                "challenge_token": start.body["challenge_token"],
                "credential": authenticator.get(&start.body["options"]),
            }),
        )
        .await
    }

    async fn exchange(&self, handoff: &str, verifier: &str) -> Reply {
        self.post(
            "/api/v1/auth/providers/exchange",
            None,
            false,
            json!({"handoff": handoff, "code_verifier": verifier}),
        )
        .await
    }

    async fn me(&self, token: &str) -> (StatusCode, Value) {
        let response = self
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
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }
}

fn handoff_of(reply: &Reply, redirect: &str) -> String {
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    reply.body["redirect_url"]
        .as_str()
        .unwrap()
        .strip_prefix(&format!("{redirect}?handoff="))
        .unwrap()
        .to_owned()
}

#[tokio::test]
async fn the_app_gets_a_session_token_for_the_passkey_the_page_proved() {
    let fixture = Fixture::new().await;
    let mut passkey = fixture.owner_passkey(3).await;
    let sessions_before = fixture.count("SELECT COUNT(*) FROM sessions").await;

    let flow = fixture.app_start("app-verifier", APP_REDIRECT).await;
    let finished = fixture.page_finish(&flow, &mut passkey, None).await;
    let handoff = handoff_of(&finished, APP_REDIRECT);
    // The browser step signs nobody in: no cookie, no session.
    assert!(finished.headers.get(header::SET_COOKIE).is_none());
    assert_eq!(
        fixture.count("SELECT COUNT(*) FROM sessions").await,
        sessions_before
    );

    let signed_in = fixture.exchange(&handoff, "app-verifier").await;
    assert_eq!(signed_in.status, StatusCode::OK, "{}", signed_in.body);
    assert!(signed_in.headers.get(header::SET_COOKIE).is_none());
    assert_eq!(signed_in.body["user"]["email"], "owner@example.com");
    let token = signed_in.body["session_token"].as_str().unwrap();
    let (status, me) = fixture.me(token).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["email"], "owner@example.com");
    assert_eq!(
        fixture.count("SELECT COUNT(*) FROM sessions").await,
        sessions_before + 1
    );
    assert_eq!(
        fixture
            .count("SELECT COUNT(*) FROM user_passkeys WHERE last_used_at IS NOT NULL")
            .await,
        1
    );

    let expo = fixture.app_start("expo-verifier", EXPO_REDIRECT).await;
    let finished = fixture.page_finish(&expo, &mut passkey, None).await;
    let handoff = handoff_of(&finished, EXPO_REDIRECT);
    assert_eq!(
        fixture.exchange(&handoff, "expo-verifier").await.status,
        StatusCode::OK
    );
}

#[tokio::test]
async fn a_handoff_is_single_use_and_needs_the_verifier_of_the_app_that_started() {
    let fixture = Fixture::new().await;
    let mut passkey = fixture.owner_passkey(3).await;
    let sessions_before = fixture.count("SELECT COUNT(*) FROM sessions").await;

    let flow = fixture.app_start("app-verifier", APP_REDIRECT).await;
    let handoff = handoff_of(
        &fixture.page_finish(&flow, &mut passkey, None).await,
        APP_REDIRECT,
    );
    // Whoever catches the redirect has the handoff but not the verifier. The wrong try also ends the handoff.
    let stolen = fixture.exchange(&handoff, "another-verifier").await;
    assert_eq!(stolen.status, StatusCode::BAD_REQUEST);
    assert_eq!(stolen.code(), "invalid_handoff");
    let late = fixture.exchange(&handoff, "app-verifier").await;
    assert_eq!(late.code(), "invalid_handoff");

    let flow = fixture.app_start("app-verifier", APP_REDIRECT).await;
    let handoff = handoff_of(
        &fixture.page_finish(&flow, &mut passkey, None).await,
        APP_REDIRECT,
    );
    assert_eq!(
        fixture.exchange(&handoff, "app-verifier").await.status,
        StatusCode::OK
    );
    let replayed = fixture.exchange(&handoff, "app-verifier").await;
    assert_eq!(replayed.code(), "invalid_handoff");
    // The flow token is used up too: the same page cannot make a second handoff.
    let again = fixture.page_finish(&flow, &mut passkey, None).await;
    assert_eq!(again.status, StatusCode::BAD_REQUEST);
    assert_eq!(again.code(), "invalid_app_sign_in");
    assert_eq!(
        fixture.count("SELECT COUNT(*) FROM sessions").await,
        sessions_before + 1
    );
}

#[tokio::test]
async fn a_signed_in_browser_cannot_finish_the_flow_without_the_passkey() {
    let fixture = Fixture::new().await;
    let mut passkey = fixture.owner_passkey(3).await;
    let flow = fixture.app_start("app-verifier", APP_REDIRECT).await;

    // The owner's browser session alone, with an answer that is not a passkey.
    let start = fixture
        .post("/api/v1/auth/passkey/login/start", None, true, json!({}))
        .await;
    let no_passkey = fixture
        .post(
            "/api/v1/auth/passkey/app/finish",
            Some(&fixture.owner_cookie),
            true,
            json!({"flow": flow, "challenge_token": start.body["challenge_token"], "credential": {}}),
        )
        .await;
    assert_eq!(no_passkey.status, StatusCode::UNAUTHORIZED);
    assert_eq!(no_passkey.code(), "invalid_passkey");

    // A passkey the server does not know.
    let mut unknown = Authenticator::new(9);
    unknown.user_handle = passkey.user_handle.clone();
    let refused = fixture.page_finish(&flow, &mut unknown, None).await;
    assert_eq!(refused.status, StatusCode::UNAUTHORIZED);
    assert_eq!(refused.code(), "invalid_passkey");

    // A wrong try leaves the flow open for the right passkey.
    let finished = fixture
        .page_finish(&flow, &mut passkey, Some(&fixture.owner_cookie))
        .await;
    handoff_of(&finished, APP_REDIRECT);
}

#[tokio::test]
async fn start_takes_only_the_allowlisted_app_redirects() {
    let fixture = Fixture::new().await;
    for redirect in [
        "web",
        "https://evil.example/auth/callback",
        "orbit://auth/callback/",
        "orbit://auth/callback?next=https://evil.example",
        "exp://10.0.0.9:8082/--/auth/callback",
    ] {
        let refused = fixture
            .post(
                "/api/v1/auth/passkey/app/start",
                None,
                false,
                json!({"code_challenge": challenge("v"), "redirect": redirect}),
            )
            .await;
        assert_eq!(
            refused.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{redirect}"
        );
        assert_eq!(refused.code(), "invalid_redirect", "{redirect}");
    }
    let refused = fixture
        .post(
            "/api/v1/auth/passkey/app/start",
            None,
            false,
            json!({"code_challenge": "short", "redirect": APP_REDIRECT}),
        )
        .await;
    assert_eq!(refused.code(), "invalid_code_challenge");
    assert_eq!(
        fixture
            .count("SELECT COUNT(*) FROM auth_pending WHERE payload_json LIKE '{\"kind\":\"app_sign_in\"%'")
            .await,
        0
    );

    // An IP address has no passkeys, so there is nothing to start.
    let fixture = Fixture::at("http://127.0.0.1:8080").await;
    let unavailable = fixture
        .post(
            "/api/v1/auth/passkey/app/start",
            None,
            false,
            json!({"code_challenge": challenge("v"), "redirect": APP_REDIRECT}),
        )
        .await;
    assert_eq!(unavailable.status, StatusCode::CONFLICT);
    assert_eq!(unavailable.code(), "passkeys_unavailable");
}

#[tokio::test]
async fn a_passkey_removed_before_the_exchange_does_not_sign_in() {
    let fixture = Fixture::new().await;
    let mut passkey = fixture.owner_passkey(3).await;
    let flow = fixture.app_start("app-verifier", APP_REDIRECT).await;
    let handoff = handoff_of(
        &fixture.page_finish(&flow, &mut passkey, None).await,
        APP_REDIRECT,
    );
    fixture
        .database
        .execute("DELETE FROM user_passkeys")
        .await
        .unwrap();
    let refused = fixture.exchange(&handoff, "app-verifier").await;
    assert_eq!(refused.status, StatusCode::UNAUTHORIZED);
    assert_eq!(refused.code(), "invalid_passkey");
}
