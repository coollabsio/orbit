//! Builds the migration catalog from the files in `apps/server/migrations/`.
//!
//! File name: `<version>_<name>.sql`, or `<version>_<name>.destructive.sql` for a migration that
//! needs a backup first. Versions up to `LAST_SEQUENTIAL_VERSION` are the old 4-digit numbers.
//! New versions are UTC timestamps (`YYYYMMDDHHMMSS`), so parallel branches do not collide.
//! Make a new one with `just new-migration <name>`.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::{env, fs};

const LAST_SEQUENTIAL_VERSION: i64 = 99;
/// The Postgres catalog starts with one baseline at this version.
const POSTGRES_BASELINE_VERSION: i64 = 46;

struct File {
    path: PathBuf,
    destructive: bool,
}

fn main() {
    let root =
        Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).join("../../apps/server/migrations");
    let root = root.canonicalize().expect("apps/server/migrations exists");
    let postgres_root = root.join("postgres");
    println!("cargo:rerun-if-changed={}", root.display());
    println!("cargo:rerun-if-changed={}", postgres_root.display());

    let sqlite = read_catalog(&root);
    let postgres = read_catalog(&postgres_root);

    for version in sqlite.keys().filter(|v| **v > POSTGRES_BASELINE_VERSION) {
        assert!(
            postgres.contains_key(version),
            "migration {version} has no Postgres file: add apps/server/migrations/postgres/{} \
             (use `SELECT 1;` if Postgres needs no change)",
            file_name(&sqlite[version].path)
        );
    }
    for version in postgres.keys().filter(|v| **v != POSTGRES_BASELINE_VERSION) {
        assert!(
            sqlite.contains_key(version),
            "Postgres migration {version} has no SQLite file in apps/server/migrations/"
        );
    }

    let mut out = String::new();
    write_list(&mut out, "SQLITE", &sqlite);
    write_list(&mut out, "POSTGRES", &postgres);
    let out_path = Path::new(&env::var("OUT_DIR").unwrap()).join("migrations.rs");
    fs::write(out_path, out).expect("write migration catalog");
}

fn read_catalog(dir: &Path) -> BTreeMap<i64, File> {
    let mut catalog = BTreeMap::new();
    for entry in fs::read_dir(dir).expect("read migration directory") {
        let path = entry.expect("read migration directory entry").path();
        if path.is_dir() {
            continue;
        }
        let name = file_name(&path);
        let Some(stem) = name.strip_suffix(".sql") else {
            continue;
        };
        let (stem, destructive) = match stem.strip_suffix(".destructive") {
            Some(stem) => (stem, true),
            None => (stem, false),
        };
        let (version, label) = stem
            .split_once('_')
            .unwrap_or_else(|| panic!("migration {name}: expected `<version>_<name>.sql`"));
        assert!(
            !label.is_empty()
                && label
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
            "migration {name}: use lowercase letters, digits and `_` in the name"
        );
        let number: i64 = version
            .parse()
            .unwrap_or_else(|_| panic!("migration {name}: the version must be a number"));
        let sequential = version.len() == 4 && (1..=LAST_SEQUENTIAL_VERSION).contains(&number);
        let timestamp = version.len() == 14 && version.starts_with("20");
        assert!(
            sequential || timestamp,
            "migration {name}: a new migration needs a UTC timestamp version (YYYYMMDDHHMMSS). \
             Make it with `just new-migration <name>`"
        );
        let file = File {
            path: path.clone(),
            destructive,
        };
        if let Some(other) = catalog.insert(number, file) {
            panic!(
                "migration version {number} appears more than once: {} and {name}",
                file_name(&other.path)
            );
        }
    }
    catalog
}

fn write_list(out: &mut String, name: &str, catalog: &BTreeMap<i64, File>) {
    writeln!(out, "pub(super) const {name}: &[(i64, &str, bool)] = &[").unwrap();
    for (version, file) in catalog {
        writeln!(
            out,
            "    ({version}, include_str!({:?}), {}),",
            file.path.display().to_string(),
            file.destructive
        )
        .unwrap();
    }
    writeln!(out, "];").unwrap();
}

fn file_name(path: &Path) -> String {
    path.file_name().unwrap().to_string_lossy().into_owned()
}
