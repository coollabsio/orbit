//! Safe reading of a stored export archive. Nothing is extracted to disk: entries are listed
//! once with the same checks as an Obsidian vault (no `..`, absolute paths, symlinks, encrypted
//! entries, too many entries, or a suspicious compression ratio), and each file is read into
//! memory on demand, its real size checked against the header.

use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::sync::{Arc, Mutex};

use serde::de::DeserializeOwned;
use thiserror::Error;
use zip::ZipArchive;

use super::model::{self, Manifest};
use crate::obsidian::archive::LocalArchive;

/// Entries one archive may hold: every task, page (two files each), conversation and blob.
const MAX_ENTRIES: usize = 1_000_000;
/// Largest JSON entry read into memory.
pub const MAX_JSON_BYTES: u64 = 64 * 1024 * 1024;
/// Entries smaller than this are not checked for their compression ratio.
const RATIO_FLOOR: u64 = 1024 * 1024;
const MAX_RATIO: u64 = 100;

#[derive(Debug, Error)]
pub enum ArchiveError {
    #[error("The file is not a valid zip archive.")]
    NotZip,
    #[error("The archive has more than {0} entries.")]
    TooManyEntries(usize),
    #[error("The archive contains an unsafe path: {0}")]
    UnsafePath(String),
    #[error("The archive is password protected.")]
    Encrypted,
    #[error("The archive looks like a zip bomb: {0} expands too much.")]
    Bomb(String),
    #[error("The archive has no {0}.")]
    Missing(&'static str),
    /// Not a workspace export, or a version this server does not read.
    #[error("The archive is not an Orbit workspace export this server can import.")]
    Unsupported,
    #[error("The archive could not be read: {0}")]
    Read(String),
    #[error("{0} is larger than {1} bytes.")]
    EntryTooLarge(String, u64),
}

/// A zip entry name as `/`-separated components, or why it is refused.
fn normalize(raw: &str) -> Result<String, ArchiveError> {
    let unsafe_path = || ArchiveError::UnsafePath(raw.chars().take(200).collect());
    if raw.starts_with('/') || raw.starts_with('\\') || raw.contains('\0') {
        return Err(unsafe_path());
    }
    let mut parts = Vec::new();
    for part in raw.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => return Err(unsafe_path()),
            _ if parts.is_empty() && part.len() == 2 && part.ends_with(':') => {
                return Err(unsafe_path());
            }
            _ => parts.push(part),
        }
    }
    Ok(parts.join("/"))
}

fn read_error(error: zip::result::ZipError) -> ArchiveError {
    match error {
        zip::result::ZipError::InvalidArchive(_) | zip::result::ZipError::UnsupportedArchive(_) => {
            ArchiveError::NotZip
        }
        zip::result::ZipError::InvalidPassword => ArchiveError::Encrypted,
        other => ArchiveError::Read(other.to_string()),
    }
}

/// An export archive opened for one job attempt. Reads run on blocking threads.
pub struct Archive {
    zip: Mutex<ZipArchive<File>>,
    /// Normalized path → (entry index, uncompressed size).
    entries: HashMap<String, (usize, u64)>,
    _file: LocalArchive,
}

impl Archive {
    /// Lists and checks every entry. Blocking.
    pub fn open(file: LocalArchive) -> Result<Arc<Self>, ArchiveError> {
        let reader =
            File::open(&file.path).map_err(|error| ArchiveError::Read(error.to_string()))?;
        let mut zip = ZipArchive::new(reader).map_err(read_error)?;
        if zip.len() > MAX_ENTRIES {
            return Err(ArchiveError::TooManyEntries(MAX_ENTRIES));
        }
        let mut entries = HashMap::with_capacity(zip.len());
        for index in 0..zip.len() {
            let entry = zip.by_index_raw(index).map_err(read_error)?;
            let name = entry
                .name()
                .map_err(|_| ArchiveError::UnsafePath("(invalid name)".to_owned()))?
                .into_owned();
            let path = normalize(&name)?;
            if entry.is_symlink() {
                return Err(ArchiveError::UnsafePath(name.chars().take(200).collect()));
            }
            if entry.encrypted() {
                return Err(ArchiveError::Encrypted);
            }
            if entry.is_dir() || path.is_empty() {
                continue;
            }
            let size = entry.size();
            if size > RATIO_FLOOR && size / entry.compressed_size().max(1) > MAX_RATIO {
                return Err(ArchiveError::Bomb(name.chars().take(200).collect()));
            }
            entries.insert(path, (index, size));
        }
        if !entries.contains_key("manifest.json") {
            return Err(ArchiveError::Unsupported);
        }
        Ok(Arc::new(Self {
            zip: Mutex::new(zip),
            entries,
            _file: file,
        }))
    }

    /// The entry's declared size, `None` when the archive has no such entry.
    #[must_use]
    pub fn size(&self, path: &str) -> Option<u64> {
        self.entries.get(path).map(|(_, size)| *size)
    }

    /// Entries under `prefix` whose name ends with `suffix`, sorted.
    #[must_use]
    pub fn paths(&self, prefix: &str, suffix: &str) -> Vec<String> {
        let mut paths: Vec<String> = self
            .entries
            .keys()
            .filter(|path| path.starts_with(prefix) && path.ends_with(suffix))
            .cloned()
            .collect();
        paths.sort();
        paths
    }

    /// Reads one entry, refusing more bytes than its header declared or than `max`. Blocking.
    pub fn read(&self, path: &str, max: u64) -> Result<Vec<u8>, ArchiveError> {
        let (index, size) = *self
            .entries
            .get(path)
            .ok_or_else(|| ArchiveError::Read(format!("{path} is not in the archive")))?;
        if size > max {
            return Err(ArchiveError::EntryTooLarge(path.to_owned(), max));
        }
        let mut zip = self
            .zip
            .lock()
            .map_err(|_| ArchiveError::Read("archive lock poisoned".to_owned()))?;
        let entry = zip.by_index(index).map_err(read_error)?;
        let mut bytes = Vec::with_capacity(usize::try_from(size).unwrap_or(0).min(1 << 20));
        entry
            .take(size.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|error| ArchiveError::Read(error.to_string()))?;
        if bytes.len() as u64 > size {
            return Err(ArchiveError::Bomb(path.chars().take(200).collect()));
        }
        Ok(bytes)
    }

    /// Parses a JSON entry. Blocking.
    pub fn read_json<T: DeserializeOwned>(&self, path: &str) -> Result<T, ArchiveError> {
        let bytes = self.read(path, MAX_JSON_BYTES)?;
        serde_json::from_slice(&bytes)
            .map_err(|error| ArchiveError::Read(format!("{path}: {error}")))
    }

    /// The manifest, checked to be a format and version this server imports. Blocking.
    pub fn manifest(&self) -> Result<Manifest, ArchiveError> {
        let manifest: Manifest = self
            .read_json("manifest.json")
            .map_err(|_| ArchiveError::Unsupported)?;
        if manifest.format != model::FORMAT || manifest.version != model::VERSION {
            return Err(ArchiveError::Unsupported);
        }
        if manifest.workspace_id.parse::<orbit_platform::Id>().is_err() {
            return Err(ArchiveError::Unsupported);
        }
        Ok(manifest)
    }

    pub fn workspace(&self) -> Result<model::Workspace, ArchiveError> {
        if self.size("workspace.json").is_none() {
            return Err(ArchiveError::Missing("workspace.json"));
        }
        self.read_json("workspace.json")
    }
}

/// Runs a blocking read of the archive on the blocking pool.
pub async fn blocking<T, F>(archive: &Arc<Archive>, f: F) -> Result<T, ArchiveError>
where
    T: Send + 'static,
    F: FnOnce(&Archive) -> Result<T, ArchiveError> + Send + 'static,
{
    let archive = Arc::clone(archive);
    tokio::task::spawn_blocking(move || f(&archive))
        .await
        .map_err(|error| ArchiveError::Read(error.to_string()))?
}
