use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use orbit_platform::{Config, ConfigOverride, ConfigSources, EnvironmentMode};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

fn fixture_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "orbit-platform-{name}-{}-{}",
        std::process::id(),
        NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
    ))
}

fn load_fixture<const N: usize>(
    toml: &str,
    env: [(&str, &str); N],
    cli_port: Option<u16>,
) -> Result<Config, orbit_platform::ConfigError> {
    let path = fixture_path("config.toml");
    let toml = if toml.contains("environment =") {
        toml.to_owned()
    } else {
        format!("environment = \"development\"\n{toml}")
    };
    fs::write(&path, toml).unwrap();
    let result = Config::load(ConfigSources {
        path: path.clone(),
        env: env
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value.to_owned()))
            .collect(),
        cli: ConfigOverride {
            http_port: cli_port,
        },
    });
    let _ = fs::remove_file(path);
    result
}

#[test]
fn environment_mode_must_be_explicit() {
    let path = fixture_path("missing-environment.toml");
    fs::write(&path, "http.port = 8080").unwrap();
    let error = Config::load(ConfigSources {
        path: path.clone(),
        env: BTreeMap::new(),
        cli: ConfigOverride::default(),
    })
    .unwrap_err();
    let _ = fs::remove_file(path);

    assert!(error.to_string().contains("environment"));
}

#[test]
fn cli_overrides_env_and_toml() {
    let cfg = load_fixture(
        "http.port = 8080",
        [("ORBIT__HTTP__PORT", "8081")],
        Some(8082),
    )
    .unwrap();

    assert_eq!(cfg.http.port, 8082);
}

#[test]
fn environment_overrides_toml() {
    let cfg = load_fixture("http.port = 8080", [("ORBIT__HTTP__PORT", "8081")], None).unwrap();

    assert_eq!(cfg.http.port, 8081);
    assert_eq!(cfg.http.public_origin, "http://127.0.0.1:8081");
}

#[test]
fn unknown_toml_keys_are_rejected() {
    let error = load_fixture("http.port = 8080\nhttp.potr = 8081", [], None).unwrap_err();

    assert!(matches!(
        error,
        orbit_platform::ConfigError::ParseConfig { .. }
    ));
}

#[test]
fn setting_secret_and_secret_file_is_rejected() {
    let secret_path = fixture_path("secret");
    fs::write(&secret_path, "from-file").unwrap();
    let path_value = secret_path.to_string_lossy().into_owned();
    let result = load_fixture(
        "http.port = 8080",
        [
            ("ORBIT__SECRETS__TOKEN", "from-env"),
            ("ORBIT__SECRETS__TOKEN_FILE", path_value.as_str()),
        ],
        None,
    );
    let _ = fs::remove_file(secret_path);

    let error = result.unwrap_err();
    assert!(error.to_string().contains("ORBIT__SECRETS__TOKEN"));
}

#[test]
fn secret_file_removes_exactly_one_trailing_newline_and_debug_is_redacted() {
    let secret_path = fixture_path("secret");
    fs::write(&secret_path, "sensitive\n\n").unwrap();
    let path_value = secret_path.to_string_lossy().into_owned();
    let cfg = load_fixture(
        "http.port = 8080",
        [("ORBIT__SECRETS__TOKEN_FILE", path_value.as_str())],
        None,
    )
    .unwrap();
    let _ = fs::remove_file(secret_path);

    let token = &cfg.secrets["token"];
    assert_eq!(token.expose(), "sensitive\n");
    assert_eq!(format!("{token:?}"), "[REDACTED]");
}

#[test]
fn malformed_environment_override_is_rejected() {
    let env = BTreeMap::from([("ORBIT__HTTP__PORT".to_owned(), "not-a-port".to_owned())]);
    let path = fixture_path("config.toml");
    fs::write(&path, "http.port = 8080").unwrap();

    let result = Config::load(ConfigSources {
        path: path.clone(),
        env,
        cli: ConfigOverride::default(),
    });
    let _ = fs::remove_file(path);

    assert!(result.is_err());
}

#[test]
fn unrelated_environment_file_pairs_are_ignored() {
    let cfg = load_fixture(
        "http.port = 8080",
        [("EDITOR", "vim"), ("EDITOR_FILE", "/tmp/editor")],
        None,
    )
    .unwrap();

    assert_eq!(cfg.http.port, 8080);
}

#[test]
fn malformed_toml_error_does_not_echo_secret_contents() {
    let error = load_fixture(
        "http.port = 8080\nsecrets.token = \"dont-print-me",
        [],
        None,
    )
    .unwrap_err();

    assert!(!error.to_string().contains("dont-print-me"));
}

#[test]
fn deployment_config_parses_tailscale_friendly_bind_and_durable_paths() {
    let cfg = load_fixture(
        r#"
        environment = "production"

        [http]
        bind = "0.0.0.0"
        port = 8080
        public_origin = "https://orbit.tailnet.example"
        trusted_proxies = ["127.0.0.1/32", "100.64.0.0/10"]

        [data]
        database = "/var/lib/orbit/orbit.sqlite"
        attachments = "/var/lib/orbit/attachments"
        backups = "/var/backups/orbit"

        [jobs]
        concurrency = 6

        [uploads]
        max_file_bytes = 10485760
        max_request_bytes = 52428800

        [metrics]
        listen = "127.0.0.1:9090"
        "#,
        [],
        None,
    )
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
    let cfg = load_fixture(
        "http.port = 8080",
        [
            ("ORBIT__ENVIRONMENT", "production"),
            ("ORBIT__HTTP__BIND", "100.64.0.12"),
            ("ORBIT__HTTP__PUBLIC_ORIGIN", "https://orbit.example"),
            ("ORBIT__HTTP__TRUSTED_PROXIES", "100.64.0.0/10"),
            ("ORBIT__DATA__DATABASE", "/srv/orbit.sqlite"),
            ("ORBIT__DATA__ATTACHMENTS", "/srv/attachments"),
            ("ORBIT__DATA__BACKUPS", "/mnt/backups"),
            ("ORBIT__JOBS__CONCURRENCY", "2"),
            ("ORBIT__UPLOADS__MAX_FILE_BYTES", "8388608"),
            ("ORBIT__UPLOADS__MAX_REQUEST_BYTES", "33554432"),
            ("ORBIT__METRICS__LISTEN", "100.64.0.12:9100"),
        ],
        None,
    )
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
    let development = load_fixture("environment = \"development\"", [], None).unwrap();
    assert_eq!(development.environment, EnvironmentMode::Development);

    let insecure = load_fixture(
        "environment = \"production\"\nhttp.public_origin = \"http://127.0.0.1:8080\"",
        [],
        None,
    )
    .unwrap_err();
    assert!(insecure.to_string().contains("production"));

    let missing_proxy = load_fixture(
        "environment = \"production\"\nhttp.public_origin = \"https://orbit.example\"",
        [],
        None,
    )
    .unwrap_err();
    assert!(missing_proxy.to_string().contains("trusted proxy"));

    let exposed_development = load_fixture(
        "environment = \"development\"\nhttp.bind = \"0.0.0.0\"",
        [],
        None,
    )
    .unwrap_err();
    assert!(exposed_development.to_string().contains("loopback"));
}

#[test]
fn endpoint_class_rate_limits_are_configurable() {
    let cfg = load_fixture(
        r#"
        [rate_limits]
        authentication_per_minute = 2
        recovery_per_minute = 3
        invitation_per_minute = 4
        upload_per_minute = 5
        general_per_minute = 6
        "#,
        [],
        None,
    )
    .unwrap();

    assert_eq!(cfg.rate_limits.authentication_per_minute, 2);
    assert_eq!(cfg.rate_limits.recovery_per_minute, 3);
    assert_eq!(cfg.rate_limits.invitation_per_minute, 4);
    assert_eq!(cfg.rate_limits.upload_per_minute, 5);
    assert_eq!(cfg.rate_limits.general_per_minute, 6);
}

#[test]
fn endpoint_class_rate_limits_reject_unsafe_values() {
    let error = load_fixture("[rate_limits]\ngeneral_per_minute = 1000001", [], None).unwrap_err();

    assert!(error.to_string().contains("cannot exceed 1000000"));
}

#[test]
fn metrics_listener_is_disabled_by_default() {
    assert_eq!(
        load_fixture("http.port = 8080", [], None)
            .unwrap()
            .metrics
            .listen,
        None
    );
}

#[test]
fn zero_worker_concurrency_is_rejected_during_config_load() {
    let error = load_fixture("jobs.concurrency = 0", [], None).unwrap_err();

    assert!(error.to_string().contains("jobs.concurrency"));
}

#[test]
fn upload_request_limit_must_cover_an_individual_file() {
    let error = load_fixture(
        "uploads.max_file_bytes = 100\nuploads.max_request_bytes = 99",
        [],
        None,
    )
    .unwrap_err();

    assert!(error.to_string().contains("uploads.max_request_bytes"));
}

#[test]
fn public_origin_must_be_an_http_origin_without_path_or_query() {
    for origin in [
        "https://",
        "https://orbit.example/path",
        "https://orbit.example?debug=true",
        "ftp://orbit.example",
    ] {
        let error =
            load_fixture(&format!("http.public_origin = \"{origin}\""), [], None).unwrap_err();
        assert!(error.to_string().contains("http.public_origin"), "{origin}");
    }
}

#[test]
fn notion_api_origin_and_rate_are_configurable() {
    let defaults = load_fixture("", [], None).unwrap();
    assert_eq!(defaults.notion.api_base, "https://api.notion.com");
    assert_eq!(defaults.notion.requests_per_minute, 180);

    let cfg = load_fixture(
        "[notion]\nrequests_per_minute = 600",
        [("ORBIT__NOTION__API_BASE", "http://127.0.0.1:9999/")],
        None,
    )
    .unwrap();
    assert_eq!(cfg.notion.api_base, "http://127.0.0.1:9999");
    assert_eq!(cfg.notion.requests_per_minute, 600);

    for (toml, env) in [
        ("", [("ORBIT__NOTION__API_BASE", "ftp://notion.example")]),
        (
            "",
            [("ORBIT__NOTION__API_BASE", "https://notion.example/v1")],
        ),
        ("", [("ORBIT__NOTION__REQUESTS_PER_MINUTE", "0")]),
        (
            "[notion]\nrequests_per_minute = 6001",
            [("ORBIT__NOTION__API_BASE", "https://api.notion.com")],
        ),
    ] {
        let error = load_fixture(toml, env, None).unwrap_err();
        assert!(error.to_string().contains("notion"), "{error}");
    }
}
