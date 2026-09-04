use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use orbit_platform::{Config, ConfigOverride, ConfigSources};

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
