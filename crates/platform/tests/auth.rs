use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use std::time::Duration;

use orbit_platform::{
    AuthenticatedUser, InMemorySessionStore, LoginThrottler, OneTimeTokenStore, PasswordError,
    PasswordService, SessionStore, ThrottleDecision, TimestampMillis, TokenKind,
};

const SECOND: i64 = 1_000;
const DAY: i64 = 24 * 60 * 60 * SECOND;

#[test]
fn password_policy_counts_characters_and_rejects_common_passwords() {
    let passwords = PasswordService::default();

    assert_eq!(passwords.hash("short"), Err(PasswordError::InvalidLength));
    assert!(passwords.hash(&"a".repeat(128)).is_ok());
    assert_eq!(
        passwords.hash(&"a".repeat(129)),
        Err(PasswordError::InvalidLength)
    );
    assert_eq!(
        passwords.hash("password1234"),
        Err(PasswordError::CommonPassword)
    );

    // Twelve user-perceived characters are valid even though each occupies several UTF-8 bytes.
    assert!(passwords.hash(&"🦀".repeat(12)).is_ok());
}

#[test]
fn passwords_are_not_unicode_normalized() {
    let passwords = PasswordService::default();
    let composed = "Caf\u{e9}-blue-sky";
    let decomposed = "Cafe\u{301}-blue-sky";
    let hash = passwords.hash(composed).unwrap();

    assert!(passwords.verify(composed, &hash).unwrap().valid);
    assert!(!passwords.verify(decomposed, &hash).unwrap().valid);
}

#[test]
fn argon2id_hashes_verify_and_old_parameters_request_rehash() {
    let old = PasswordService::with_argon2_params(8 * 1024, 1, 1).unwrap();
    let current = PasswordService::with_argon2_params(12 * 1024, 2, 1).unwrap();
    let hash = old.hash("correct horse battery").unwrap();

    assert!(hash.starts_with("$argon2id$"));
    let verified = current.verify("correct horse battery", &hash).unwrap();
    assert!(verified.valid);
    assert!(verified.replacement_hash.is_some());
    assert!(
        !current
            .verify("incorrect horse battery", &hash)
            .unwrap()
            .valid
    );
}

#[tokio::test]
async fn setup_token_can_only_win_one_concurrent_consumer() {
    let tokens = Arc::new(OneTimeTokenStore::new());
    let now = TimestampMillis::from_millis(1_000);
    let issued = tokens
        .issue(
            TokenKind::Setup,
            "installation",
            now,
            Duration::from_secs(60),
        )
        .unwrap();

    let first_store = Arc::clone(&tokens);
    let first_token = issued.token.clone();
    let second_store = Arc::clone(&tokens);
    let second_token = issued.token.clone();
    let (first, second) = tokio::join!(
        tokio::spawn(async move { first_store.consume(TokenKind::Setup, &first_token, now) }),
        tokio::spawn(async move { second_store.consume(TokenKind::Setup, &second_token, now) }),
    );

    let successes = [first.unwrap(), second.unwrap()]
        .into_iter()
        .filter(Result::is_ok)
        .count();
    assert_eq!(successes, 1);
}

#[test]
fn token_store_persists_only_hashes_and_tokens_are_single_use() {
    let tokens = OneTimeTokenStore::new();
    let now = TimestampMillis::from_millis(2_000);
    let issued = tokens
        .issue(TokenKind::Recovery, "user-1", now, Duration::from_secs(60))
        .unwrap();

    let persisted = tokens.persisted_hashes();
    assert_eq!(persisted.len(), 1);
    assert_ne!(persisted[0].as_slice(), issued.token.as_bytes());
    assert!(!format!("{issued:?}").contains(&issued.token));
    assert_eq!(
        tokens
            .consume(TokenKind::Recovery, &issued.token, now)
            .unwrap(),
        "user-1"
    );
    assert!(
        tokens
            .consume(TokenKind::Recovery, &issued.token, now)
            .is_err()
    );
}

#[test]
fn token_kind_mismatch_does_not_consume_a_valid_token_and_setup_never_reopens() {
    let tokens = OneTimeTokenStore::new();
    let now = TimestampMillis::from_millis(3_000);
    let recovery = tokens
        .issue(TokenKind::Recovery, "user-1", now, Duration::from_secs(60))
        .unwrap();
    assert!(
        tokens
            .consume(TokenKind::Setup, &recovery.token, now)
            .is_err()
    );
    assert_eq!(
        tokens
            .consume(TokenKind::Recovery, &recovery.token, now)
            .unwrap(),
        "user-1"
    );

    let setup = tokens
        .issue(
            TokenKind::Setup,
            "installation",
            now,
            Duration::from_secs(60),
        )
        .unwrap();
    tokens.consume(TokenKind::Setup, &setup.token, now).unwrap();
    assert!(
        tokens
            .issue(
                TokenKind::Setup,
                "installation",
                now,
                Duration::from_secs(60)
            )
            .is_err()
    );
}

#[test]
fn sessions_use_sliding_idle_expiry_bounded_by_absolute_expiry() {
    let sessions = InMemorySessionStore::new();
    let now = TimestampMillis::from_millis(10_000);
    let user = user();
    let issued = sessions.create(user.clone(), now).unwrap();

    assert_eq!(
        issued.idle_expires_at.as_millis(),
        now.as_millis() + 30 * DAY
    );
    assert_eq!(
        issued.absolute_expires_at.as_millis(),
        now.as_millis() + 90 * DAY
    );
    assert_eq!(
        sessions
            .authenticate(
                &issued.token,
                TimestampMillis::from_millis(now.as_millis() + 29 * DAY)
            )
            .unwrap(),
        user
    );
    let record = sessions.get(issued.id).unwrap();
    assert_eq!(
        record.idle_expires_at.as_millis(),
        now.as_millis() + 59 * DAY
    );

    assert!(
        sessions
            .authenticate(
                &issued.token,
                TimestampMillis::from_millis(now.as_millis() + 90 * DAY)
            )
            .is_err()
    );
}

#[test]
fn session_activity_writes_are_throttled_and_revocation_is_immediate() {
    let sessions = InMemorySessionStore::new();
    let now = TimestampMillis::from_millis(20_000);
    let issued = sessions.create(user(), now).unwrap();
    assert!(!format!("{issued:?}").contains(&issued.token));

    sessions
        .authenticate(
            &issued.token,
            TimestampMillis::from_millis(now.as_millis() + 60 * SECOND),
        )
        .unwrap();
    assert_eq!(sessions.activity_write_count(issued.id), Some(0));

    sessions
        .authenticate(
            &issued.token,
            TimestampMillis::from_millis(now.as_millis() + 5 * 60 * SECOND),
        )
        .unwrap();
    assert_eq!(sessions.activity_write_count(issued.id), Some(1));

    sessions.revoke(issued.id).unwrap();
    assert!(sessions.authenticate(&issued.token, now).is_err());
}

#[test]
fn progressive_email_throttle_uses_exact_delays_and_cap() {
    let mut throttle = LoginThrottler::new();
    let ip = IpAddr::V4(Ipv4Addr::LOCALHOST);
    let email = "owner@example.com";
    let mut now = TimestampMillis::from_millis(0);

    for _ in 0..4 {
        assert_eq!(throttle.check(email, ip, now), ThrottleDecision::Allowed);
        throttle.record_failure(email, ip, now);
    }

    let expected = [5, 10, 20, 40, 80, 160, 320, 640, 900, 900];
    for delay in expected {
        assert_eq!(throttle.check(email, ip, now), ThrottleDecision::Allowed);
        throttle.record_failure(email, ip, now);
        assert_eq!(
            throttle.check(email, ip, now),
            ThrottleDecision::RetryAfter(Duration::from_secs(delay))
        );
        now = TimestampMillis::from_millis(now.as_millis() + delay as i64 * SECOND);
    }

    throttle.record_success(email);
    assert_eq!(throttle.check(email, ip, now), ThrottleDecision::Allowed);
}

#[test]
fn ip_throttle_allows_fifty_failures_in_a_rolling_fifteen_minutes() {
    let mut throttle = LoginThrottler::new();
    let ip = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 4));
    let now = TimestampMillis::from_millis(0);

    for attempt in 0..50 {
        let email = format!("person-{attempt}@example.com");
        assert_eq!(throttle.check(&email, ip, now), ThrottleDecision::Allowed);
        throttle.record_failure(&email, ip, now);
    }
    assert_eq!(
        throttle.check("next@example.com", ip, now),
        ThrottleDecision::RetryAfter(Duration::from_secs(15 * 60))
    );
    assert_eq!(
        throttle.check(
            "next@example.com",
            ip,
            TimestampMillis::from_millis(15 * 60 * SECOND)
        ),
        ThrottleDecision::Allowed
    );
}

fn user() -> AuthenticatedUser {
    AuthenticatedUser {
        id: orbit_platform::Id::new_v7(),
        email: "Owner@Example.com".to_owned(),
        display_name: "Owner".to_owned(),
    }
}
