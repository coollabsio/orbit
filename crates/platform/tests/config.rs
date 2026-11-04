use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use orbit_platform::{Config, EnvironmentMode};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

fn fixture_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "orbit-platform-{name}-{}-{}",
        std::process::id(),
        NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
    ))
}

fn load<const N: usize>(env: [(&str, &str); N]) -> Result<Config, orbit_platform::ConfigError> {
    let mut env: BTreeMap<String, String> = env
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect();
    env.entry("ORBIT_ENV".to_owned())
        .or_insert_with(|| "development".to_owned());
    if !env.contains_key("ORBIT_APP_KEY_FILE") {
        env.entry("ORBIT_APP_KEY".to_owned())
            .or_insert_with(|| APP_KEY.to_owned());
    }
    Config::from_environment(env)
}

const APP_KEY: &str = "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=";

#[test]
fn environment_mode_must_be_explicit() {
    let error = Config::from_environment(BTreeMap::from([(
        "ORBIT_PORT".to_owned(),
        "8080".to_owned(),
    )]))
    .unwrap_err();

    assert!(error.to_string().contains("ORBIT_ENV"), "{error}");
}

#[test]
fn port_sets_the_default_public_origin() {
    let cfg = load([("ORBIT_PORT", "8081")]).unwrap();

    assert_eq!(cfg.http.port, 8081);
    assert_eq!(cfg.http.public_origin, "http://127.0.0.1:8081");
}

#[test]
fn unknown_environment_keys_are_rejected() {
    let error = load([("ORBIT_POTR", "8081")]).unwrap_err();

    assert!(matches!(
        error,
        orbit_platform::ConfigError::UnknownEnvironment(ref key) if key == "ORBIT_POTR"
    ));
}

#[test]
fn setting_secret_and_secret_file_is_rejected() {
    let secret_path = fixture_path("secret");
    fs::write(&secret_path, "from-file").unwrap();
    let path_value = secret_path.to_string_lossy().into_owned();
    let result = load([
        ("ORBIT_APP_KEY", APP_KEY),
        ("ORBIT_APP_KEY_FILE", path_value.as_str()),
    ]);
    let _ = fs::remove_file(secret_path);

    let error = result.unwrap_err();
    assert!(error.to_string().contains("ORBIT_APP_KEY"));
}

#[test]
fn app_key_file_is_read_and_debug_is_redacted() {
    let secret_path = fixture_path("secret");
    fs::write(&secret_path, format!("{APP_KEY}\n")).unwrap();
    let path_value = secret_path.to_string_lossy().into_owned();
    let cfg = load([("ORBIT_APP_KEY_FILE", path_value.as_str())]).unwrap();
    let _ = fs::remove_file(secret_path);

    let key = cfg.app_key.unwrap();
    assert_eq!(key.bytes(), &[1; 32]);
    assert_eq!(format!("{key:?}"), "[REDACTED]");
}

#[test]
fn app_key_is_required_and_must_be_32_base64_bytes() {
    let without_key = Config::from_environment(BTreeMap::from([(
        "ORBIT_ENV".to_owned(),
        "development".to_owned(),
    )]))
    .unwrap_err()
    .to_string();
    assert!(without_key.contains("ORBIT_APP_KEY"), "{without_key}");

    for invalid in ["", "not base64", "AQEBAQ=="] {
        let error = load([("ORBIT_APP_KEY", invalid)]).unwrap_err().to_string();
        assert!(error.contains("ORBIT_APP_KEY"), "{error}");
        assert!(invalid.is_empty() || !error.contains(invalid), "{error}");
    }
}

#[test]
fn malformed_environment_override_is_rejected() {
    assert!(load([("ORBIT_PORT", "not-a-port")]).is_err());
}

#[test]
fn unrelated_environment_file_pairs_are_ignored() {
    let cfg = load([
        ("ORBIT_PORT", "8080"),
        ("ORBIT_DEV_API_PORT", "18080"),
        ("ORBIT_TEST_POSTGRES_URL", "postgres://test"),
        ("EDITOR", "vim"),
        ("EDITOR_FILE", "/tmp/editor"),
    ])
    .unwrap();

    assert_eq!(cfg.http.port, 8080);
}

#[test]
fn deployment_config_parses_tailscale_friendly_bind_and_durable_paths() {
    let cfg = load([
        ("ORBIT_ENV", "production"),
        ("ORBIT_BIND", "0.0.0.0"),
        ("ORBIT_PORT", "8080"),
        ("ORBIT_PUBLIC_ORIGIN", "https://orbit.tailnet.example"),
        ("ORBIT_TRUSTED_PROXIES", "127.0.0.1/32, 100.64.0.0/10"),
        ("ORBIT_DATABASE_PATH", "/var/lib/orbit/orbit.sqlite"),
        ("ORBIT_ATTACHMENTS_PATH", "/var/lib/orbit/attachments"),
        ("ORBIT_BACKUPS_PATH", "/var/backups/orbit"),
        ("ORBIT_JOBS_CONCURRENCY", "6"),
        ("ORBIT_UPLOAD_MAX_FILE_BYTES", "10485760"),
        ("ORBIT_UPLOAD_MAX_REQUEST_BYTES", "52428800"),
        ("ORBIT_METRICS_LISTEN", "127.0.0.1:9090"),
    ])
    .unwrap();

    assert_eq!(cfg.http.bind.to_string(), "0.0.0.0");
    assert_eq!(cfg.http.public_origin, "https://orbit.tailnet.example");
    assert_eq!(
        cfg.http
            .trusted_proxies
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        ["127.0.0.1/32", "100.64.0.0/10"]
    );
    assert_eq!(
        cfg.data.database,
        PathBuf::from("/var/lib/orbit/orbit.sqlite")
    );
    assert_eq!(
        cfg.data.attachments,
        PathBuf::from("/var/lib/orbit/attachments")
    );
    assert_eq!(cfg.data.backups, PathBuf::from("/var/backups/orbit"));
    assert_eq!(cfg.jobs.concurrency, 6);
    assert_eq!(cfg.uploads.max_file_bytes, 10 * 1024 * 1024);
    assert_eq!(cfg.uploads.max_request_bytes, 50 * 1024 * 1024);
    assert_eq!(cfg.metrics.listen.unwrap().to_string(), "127.0.0.1:9090");
}

#[test]
fn deployment_environment_overrides_bind_origin_paths_and_worker_count() {
    let cfg = load([
        ("ORBIT_ENV", "production"),
        ("ORBIT_BIND", "100.64.0.12"),
        ("ORBIT_PUBLIC_ORIGIN", "https://orbit.example"),
        ("ORBIT_TRUSTED_PROXIES", "100.64.0.0/10"),
        ("ORBIT_DATABASE_PATH", "/srv/orbit.sqlite"),
        ("ORBIT_ATTACHMENTS_PATH", "/srv/attachments"),
        ("ORBIT_BACKUPS_PATH", "/mnt/backups"),
        ("ORBIT_JOBS_CONCURRENCY", "2"),
        ("ORBIT_UPLOAD_MAX_FILE_BYTES", "8388608"),
        ("ORBIT_UPLOAD_MAX_REQUEST_BYTES", "33554432"),
        ("ORBIT_METRICS_LISTEN", "100.64.0.12:9100"),
    ])
    .unwrap();

    assert_eq!(cfg.http.bind.to_string(), "100.64.0.12");
    assert_eq!(cfg.http.public_origin, "https://orbit.example");
    assert_eq!(cfg.data.database, PathBuf::from("/srv/orbit.sqlite"));
    assert_eq!(cfg.data.attachments, PathBuf::from("/srv/attachments"));
    assert_eq!(cfg.data.backups, PathBuf::from("/mnt/backups"));
    assert_eq!(cfg.jobs.concurrency, 2);
    assert_eq!(cfg.uploads.max_file_bytes, 8 * 1024 * 1024);
    assert_eq!(cfg.uploads.max_request_bytes, 32 * 1024 * 1024);
    assert_eq!(cfg.metrics.listen.unwrap().to_string(), "100.64.0.12:9100");
}

#[test]
fn environment_mode_restricts_cookie_and_proxy_boundaries() {
    let development = load([("ORBIT_ENV", "development")]).unwrap();
    assert_eq!(development.environment, EnvironmentMode::Development);

    let insecure = load([
        ("ORBIT_ENV", "production"),
        ("ORBIT_PUBLIC_ORIGIN", "http://127.0.0.1:8080"),
    ])
    .unwrap_err();
    assert!(insecure.to_string().contains("production"));

    let missing_proxy = load([
        ("ORBIT_ENV", "production"),
        ("ORBIT_PUBLIC_ORIGIN", "https://orbit.example"),
    ])
    .unwrap_err();
    assert!(missing_proxy.to_string().contains("trusted proxy"));

    let exposed_development =
        load([("ORBIT_ENV", "development"), ("ORBIT_BIND", "0.0.0.0")]).unwrap_err();
    assert!(exposed_development.to_string().contains("loopback"));
}

#[test]
fn endpoint_class_rate_limits_are_configurable() {
    let cfg = load([
        ("ORBIT_RATE_LIMIT_AUTHENTICATION_PER_MINUTE", "2"),
        ("ORBIT_RATE_LIMIT_RECOVERY_PER_MINUTE", "3"),
        ("ORBIT_RATE_LIMIT_INVITATION_PER_MINUTE", "4"),
        ("ORBIT_RATE_LIMIT_UPLOAD_PER_MINUTE", "5"),
        ("ORBIT_RATE_LIMIT_GENERAL_PER_MINUTE", "6"),
    ])
    .unwrap();

    assert_eq!(cfg.rate_limits.authentication_per_minute, 2);
    assert_eq!(cfg.rate_limits.recovery_per_minute, 3);
    assert_eq!(cfg.rate_limits.invitation_per_minute, 4);
    assert_eq!(cfg.rate_limits.upload_per_minute, 5);
    assert_eq!(cfg.rate_limits.general_per_minute, 6);
}

#[test]
fn principal_workspace_and_email_limits_have_defaults_and_overrides() {
    let defaults = load([]).unwrap().rate_limits;
    assert_eq!(defaults.workspace_multiplier, 10);
    assert_eq!(defaults.account_email_cooldown_seconds, 60);
    assert_eq!(defaults.account_emails_per_day, 5);
    assert_eq!(defaults.invitation_emails_per_workspace_per_day, 100);
    assert_eq!(defaults.invitation_emails_per_inviter_per_day, 50);

    let cfg = load([
        ("ORBIT_RATE_LIMIT_WORKSPACE_MULTIPLIER", "4"),
        ("ORBIT_RATE_LIMIT_ACCOUNT_EMAILS_PER_DAY", "3"),
        ("ORBIT_RATE_LIMIT_ACCOUNT_EMAIL_COOLDOWN_SECONDS", "120"),
        (
            "ORBIT_RATE_LIMIT_INVITATION_EMAILS_PER_WORKSPACE_PER_DAY",
            "7",
        ),
        (
            "ORBIT_RATE_LIMIT_INVITATION_EMAILS_PER_INVITER_PER_DAY",
            "8",
        ),
    ])
    .unwrap();
    assert_eq!(cfg.rate_limits.workspace_multiplier, 4);
    assert_eq!(cfg.rate_limits.account_emails_per_day, 3);
    assert_eq!(cfg.rate_limits.account_email_cooldown_seconds, 120);
    assert_eq!(cfg.rate_limits.invitation_emails_per_workspace_per_day, 7);
    assert_eq!(cfg.rate_limits.invitation_emails_per_inviter_per_day, 8);

    for invalid in [
        ("ORBIT_RATE_LIMIT_WORKSPACE_MULTIPLIER", "0"),
        ("ORBIT_RATE_LIMIT_ACCOUNT_EMAILS_PER_DAY", "0"),
        ("ORBIT_RATE_LIMIT_ACCOUNT_EMAIL_COOLDOWN_SECONDS", "86401"),
        ("ORBIT_RATE_LIMIT_UNKNOWN", "1"),
    ] {
        assert!(load([invalid]).is_err(), "{invalid:?}");
    }
}

#[test]
fn endpoint_class_rate_limits_reject_unsafe_values() {
    let error = load([("ORBIT_RATE_LIMIT_GENERAL_PER_MINUTE", "1000001")]).unwrap_err();

    assert!(error.to_string().contains("ORBIT_RATE_LIMIT_*"), "{error}");
    assert!(error.to_string().contains("cannot exceed 1000000"));
}

#[test]
fn metrics_listener_is_disabled_by_default() {
    assert_eq!(load([]).unwrap().metrics.listen, None);
}

#[test]
fn zero_worker_concurrency_is_rejected_during_config_load() {
    let error = load([("ORBIT_JOBS_CONCURRENCY", "0")]).unwrap_err();

    assert!(
        error.to_string().contains("ORBIT_JOBS_CONCURRENCY"),
        "{error}"
    );
}

#[test]
fn upload_request_limit_must_cover_an_individual_file() {
    let error = load([
        ("ORBIT_UPLOAD_MAX_FILE_BYTES", "100"),
        ("ORBIT_UPLOAD_MAX_REQUEST_BYTES", "99"),
    ])
    .unwrap_err();

    assert!(
        error.to_string().contains("ORBIT_UPLOAD_MAX_REQUEST_BYTES"),
        "{error}"
    );
}

#[test]
fn public_origin_must_be_an_http_origin_without_path_or_query() {
    for origin in [
        "https://",
        "https://orbit.example/path",
        "https://orbit.example?debug=true",
        "ftp://orbit.example",
    ] {
        let error = load([("ORBIT_PUBLIC_ORIGIN", origin)]).unwrap_err();
        assert!(
            error.to_string().contains("ORBIT_PUBLIC_ORIGIN"),
            "{origin}"
        );
    }
}

#[test]
fn notion_import_has_no_operator_settings() {
    // The Notion API origin and request rate are fixed in the client (api.notion.com, 180/min):
    // the old keys are unknown settings now, like any typo.
    let error = load([("ORBIT_NOTION_API_BASE", "https://api.notion.com")]).unwrap_err();
    assert!(
        error.to_string().contains("ORBIT_NOTION_API_BASE"),
        "{error}"
    );
    let error = load([("ORBIT_NOTION_REQUESTS_PER_MINUTE", "600")]).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("ORBIT_NOTION_REQUESTS_PER_MINUTE"),
        "{error}"
    );
}

#[test]
fn postgres_database_url_comes_from_environment_and_is_redacted() {
    let from_environment = load([(
        "ORBIT_DATABASE_URL",
        "postgres://orbit:hunter2@db.internal:5432/orbit",
    )])
    .unwrap();
    assert_eq!(
        from_environment.data.database_url.as_deref(),
        Some("postgres://orbit:hunter2@db.internal:5432/orbit")
    );
    assert!(!format!("{from_environment:?}").contains("hunter2"));
    let database = orbit_platform::DatabaseConfig::for_data(&from_environment.data);
    assert_eq!(database.path, from_environment.data.database);
    assert_eq!(database.postgres_url, from_environment.data.database_url);
    assert!(!format!("{database:?}").contains("hunter2"));
    assert_eq!(
        orbit_platform::redact_database_url("postgres://orbit:hunter2@db.internal/orbit"),
        "postgres://orbit:REDACTED@db.internal/orbit"
    );

    let postgresql = load([("ORBIT_DATABASE_URL", "postgresql://orbit@127.0.0.1/orbit")]).unwrap();
    assert_eq!(
        postgresql.data.database_url.as_deref(),
        Some("postgresql://orbit@127.0.0.1/orbit")
    );
    assert_eq!(load([]).unwrap().data.database_url, None);
    assert!(load([("ORBIT_DATABASE_URL", "mysql://x")]).is_err());
}

const STRIPE: [(&str, &str); 3] = [
    ("ORBIT_STRIPE_SECRET_KEY", "sk_test_123"),
    ("ORBIT_STRIPE_WEBHOOK_SECRET", "whsec_123"),
    (
        "ORBIT_STRIPE_PRICES",
        "team:price_team, business-2:price_business",
    ),
];

#[test]
fn billing_is_off_by_default() {
    assert_eq!(load([]).unwrap().billing, None);
    assert_eq!(load([("ORBIT_CLOUD", "false")]).unwrap().billing, None);
}

#[test]
fn cloud_mode_reads_stripe_settings_in_order() {
    let [secret, webhook, prices] = STRIPE;
    let cfg = load([("ORBIT_CLOUD", "true"), secret, webhook, prices]).unwrap();

    let billing = cfg.billing.unwrap();
    assert_eq!(billing.secret_key.expose(), "sk_test_123");
    assert_eq!(billing.webhook_secret.expose(), "whsec_123");
    assert_eq!(
        billing.plans,
        [
            orbit_platform::Plan {
                key: "team".to_owned(),
                price_id: "price_team".to_owned()
            },
            orbit_platform::Plan {
                key: "business-2".to_owned(),
                price_id: "price_business".to_owned()
            },
        ]
    );
    assert_eq!(billing.api_base.as_str(), "https://api.stripe.com/");
    assert!(!format!("{billing:?}").contains("sk_test_123"));
    assert!(!format!("{billing:?}").contains("whsec_123"));
}

#[test]
fn stripe_secrets_can_come_from_files() {
    let secret_path = fixture_path("stripe-secret");
    let webhook_path = fixture_path("stripe-webhook");
    fs::write(&secret_path, "sk_test_file\n").unwrap();
    fs::write(&webhook_path, "whsec_file\n").unwrap();
    let secret = secret_path.to_string_lossy().into_owned();
    let webhook = webhook_path.to_string_lossy().into_owned();
    let result = load([
        ("ORBIT_CLOUD", "true"),
        ("ORBIT_STRIPE_SECRET_KEY_FILE", secret.as_str()),
        ("ORBIT_STRIPE_WEBHOOK_SECRET_FILE", webhook.as_str()),
        STRIPE[2],
    ]);
    let both = load([
        ("ORBIT_CLOUD", "true"),
        ("ORBIT_STRIPE_SECRET_KEY_FILE", secret.as_str()),
        STRIPE[0],
        STRIPE[1],
        STRIPE[2],
    ]);
    let _ = fs::remove_file(secret_path);
    let _ = fs::remove_file(webhook_path);

    let billing = result.unwrap().billing.unwrap();
    assert_eq!(billing.secret_key.expose(), "sk_test_file");
    assert_eq!(billing.webhook_secret.expose(), "whsec_file");
    assert!(matches!(
        both.unwrap_err(),
        orbit_platform::ConfigError::AmbiguousSecret { ref key } if key == "ORBIT_STRIPE_SECRET_KEY"
    ));
}

#[test]
fn cloud_mode_requires_every_stripe_setting() {
    let [secret, webhook, prices] = STRIPE;
    for (missing, env) in [
        ("ORBIT_STRIPE_SECRET_KEY", [webhook, prices]),
        ("ORBIT_STRIPE_WEBHOOK_SECRET", [secret, prices]),
        ("ORBIT_STRIPE_PRICES", [secret, webhook]),
    ] {
        let error = load([("ORBIT_CLOUD", "true"), env[0], env[1]])
            .unwrap_err()
            .to_string();
        assert!(error.contains(missing), "{error}");
    }
}

#[test]
fn stripe_settings_without_cloud_mode_are_rejected() {
    for setting in STRIPE {
        let error = load([setting]).unwrap_err().to_string();
        assert!(
            error.contains(setting.0) && error.contains("ORBIT_CLOUD"),
            "{error}"
        );
    }
    let error = load([("ORBIT_CLOUD", "yes")]).unwrap_err().to_string();
    assert!(error.contains("ORBIT_CLOUD"), "{error}");
}

#[test]
fn stripe_prices_must_be_unique_lowercase_plan_keys() {
    let [secret, webhook, _] = STRIPE;
    for prices in [
        "",
        "team",
        "Team:price_1",
        "team:price_1,team:price_2",
        "team:price_1,business:price_1",
        "te am:price_1",
        "team:",
    ] {
        let error = load([
            ("ORBIT_CLOUD", "true"),
            secret,
            webhook,
            ("ORBIT_STRIPE_PRICES", prices),
        ])
        .unwrap_err()
        .to_string();
        assert!(error.contains("ORBIT_STRIPE_PRICES"), "{prices:?}: {error}");
    }
}

#[test]
fn trial_has_defaults_and_can_be_changed_in_cloud_mode() {
    let [secret, webhook, prices] = STRIPE;
    let cloud = ("ORBIT_CLOUD", "true");
    let defaults = load([cloud, secret, webhook, prices]).unwrap();
    assert_eq!(
        defaults.billing.unwrap().trial,
        orbit_platform::TrialConfig {
            days: 7,
            tasks: 50,
            pages: 20,
            members: 3
        }
    );

    let custom = load([
        cloud,
        secret,
        webhook,
        prices,
        ("ORBIT_TRIAL_DAYS", "0"),
        ("ORBIT_TRIAL_TASKS", "0"),
        ("ORBIT_TRIAL_PAGES", "100"),
        ("ORBIT_TRIAL_MEMBERS", "1"),
    ])
    .unwrap();
    assert_eq!(
        custom.billing.unwrap().trial,
        orbit_platform::TrialConfig {
            days: 0,
            tasks: 0,
            pages: 100,
            members: 1
        }
    );
}

#[test]
fn trial_settings_reject_values_out_of_range_and_non_numbers() {
    let [secret, webhook, prices] = STRIPE;
    for (key, value, detail) in [
        ("ORBIT_TRIAL_DAYS", "366", "must be 0 to 365 days"),
        ("ORBIT_TRIAL_DAYS", "-1", "\"-1\" must be a whole number"),
        (
            "ORBIT_TRIAL_DAYS",
            "seven",
            "\"seven\" must be a whole number",
        ),
        ("ORBIT_TRIAL_TASKS", "2.5", "\"2.5\" must be a whole number"),
        ("ORBIT_TRIAL_TASKS", "-1", "must be 0 or more"),
        ("ORBIT_TRIAL_PAGES", "-5", "must be 0 or more"),
        (
            "ORBIT_TRIAL_MEMBERS",
            "0",
            "must be 1 or more (the owner counts)",
        ),
        ("ORBIT_TRIAL_MEMBERS", "", "\"\" must be a whole number"),
    ] {
        let error = load([
            ("ORBIT_CLOUD", "true"),
            secret,
            webhook,
            prices,
            (key, value),
        ])
        .unwrap_err()
        .to_string();
        assert_eq!(
            error,
            format!("invalid configuration setting {key}: {detail}")
        );
    }
}

#[test]
fn trial_settings_without_cloud_mode_are_rejected() {
    for key in [
        "ORBIT_TRIAL_DAYS",
        "ORBIT_TRIAL_TASKS",
        "ORBIT_TRIAL_PAGES",
        "ORBIT_TRIAL_MEMBERS",
    ] {
        let error = load([(key, "1")]).unwrap_err().to_string();
        assert_eq!(
            error,
            format!("invalid configuration setting {key}: is only used with ORBIT_CLOUD=true")
        );
    }
}

#[test]
fn dev_native_redirects_are_parsed_and_refused_in_production() {
    let development = load([(
        "ORBIT_DEV_NATIVE_REDIRECTS",
        " exp://100.64.0.5:8082/--/auth/callback ,,exp://127.0.0.1:8081/--/auth/callback",
    )])
    .unwrap();
    assert_eq!(
        development.http.dev_native_redirects,
        vec![
            "exp://100.64.0.5:8082/--/auth/callback".to_owned(),
            "exp://127.0.0.1:8081/--/auth/callback".to_owned(),
        ]
    );

    let production = load([
        ("ORBIT_ENV", "production"),
        ("ORBIT_PUBLIC_ORIGIN", "https://orbit.example"),
        ("ORBIT_TRUSTED_PROXIES", "10.0.0.0/8"),
        (
            "ORBIT_DEV_NATIVE_REDIRECTS",
            "exp://100.64.0.5:8082/--/auth/callback",
        ),
    ])
    .unwrap_err();
    assert_eq!(
        production.to_string(),
        "invalid configuration setting ORBIT_DEV_NATIVE_REDIRECTS: is for development only and cannot be set in production mode"
    );
}
