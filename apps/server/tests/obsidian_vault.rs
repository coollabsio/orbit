//! Safe reading of uploaded vault zips, Obsidian link resolution, folder notes and the scan tree.

use std::io::{Cursor, Write};

use orbit_server::obsidian::scan::{ObsidianImportNodeKind, build_tree, scan};
use orbit_server::obsidian::vault::{
    ObsidianSkippedFiles, Resolved, Vault, VaultError, VaultLimits, open_archive,
};
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

/// `(path, kind, note path, parent path, child count)` of a tree node.
type Row<'a> = (
    &'a str,
    ObsidianImportNodeKind,
    Option<&'a str>,
    Option<&'a str>,
    u32,
);

const LIMITS: VaultLimits = VaultLimits::new(1024 * 1024);

fn zip(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, bytes) in files {
        writer.start_file(*name, options).unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn open(bytes: Vec<u8>) -> Result<Vault, VaultError> {
    let mut archive = open_archive(Cursor::new(bytes))?;
    Vault::open(&mut archive, LIMITS)
}

fn vault_zip() -> Vec<u8> {
    zip(&[
        ("Vault/.obsidian/app.json", b"{}"),
        ("Vault/.DS_Store", b"x"),
        (
            "Vault/Projects/Projects.md",
            b"Folder note with ![[diagram.png]]",
        ),
        ("Vault/Projects/Plan.md", b"See [[Inbox]] and [[Missing]]"),
        ("Vault/Projects/assets/diagram.png", b"png"),
        ("Vault/Inbox.md", b"# Inbox"),
        ("Vault/inbox.MD", b"duplicate"),
        ("Vault/Board.canvas", b"{}"),
        ("Vault/data.csv", b"a,b"),
        ("Vault/unused.png", b"png"),
    ])
}

fn paths(files: &[orbit_server::obsidian::vault::VaultFile]) -> Vec<&str> {
    files.iter().map(|file| file.path.as_str()).collect()
}

#[test]
fn strips_the_wrapper_folder_and_skips_system_files() {
    let vault = open(vault_zip()).unwrap();
    assert_eq!(
        paths(&vault.notes),
        ["Projects/Projects.md", "Projects/Plan.md", "Inbox.md"]
    );
    assert_eq!(
        paths(&vault.attachments),
        ["Projects/assets/diagram.png", "unused.png"]
    );
    assert_eq!(
        vault.skipped,
        ObsidianSkippedFiles {
            system: 2,
            canvas: 1,
            csv: 1,
            unreferenced: 0,
            too_large: 0,
            duplicates: 1,
        }
    );
}

#[test]
fn keeps_paths_when_there_is_no_single_wrapper() {
    let vault = open(zip(&[("A/one.md", b"1"), ("two.md", b"2")])).unwrap();
    assert_eq!(paths(&vault.notes), ["A/one.md", "two.md"]);
}

#[test]
fn refuses_parent_and_absolute_paths() {
    for name in [
        "../evil.md",
        "notes/../../evil.md",
        "/etc/evil.md",
        "C:/evil.md",
    ] {
        let error = open(zip(&[("ok.md", b"ok"), (name, b"x")])).unwrap_err();
        assert!(
            matches!(error, VaultError::UnsafePath(_)),
            "{name}: {error:?}"
        );
        assert!(
            error
                .to_string()
                .starts_with("The archive contains an unsafe path")
        );
    }
}

#[test]
fn refuses_symlinks() {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .add_symlink("link.md", "/etc/passwd", SimpleFileOptions::default())
        .unwrap();
    let bytes = writer.finish().unwrap().into_inner();
    assert_eq!(
        open(bytes).unwrap_err(),
        VaultError::Symlink("link.md".to_owned())
    );
}

#[test]
fn refuses_zip_bombs_and_oversized_archives() {
    let zeros = vec![0u8; 20 * 1024 * 1024];
    let error = open(zip(&[("note.md", b"x"), ("bomb.bin", &zeros)])).unwrap_err();
    assert_eq!(error, VaultError::Bomb("bomb.bin".to_owned()));
    assert_eq!(
        error.to_string(),
        "The archive looks like a zip bomb: bomb.bin expands too much."
    );

    let limits = VaultLimits {
        max_total_bytes: 10,
        ..LIMITS
    };
    let mut archive = open_archive(Cursor::new(zip(&[
        ("a.md", b"123456"),
        ("b.md", b"123456"),
    ])))
    .unwrap();
    assert!(matches!(
        Vault::open(&mut archive, limits),
        Err(VaultError::TooLarge(_))
    ));

    let limits = VaultLimits {
        max_entries: 1,
        ..LIMITS
    };
    let mut archive = open_archive(Cursor::new(zip(&[("a.md", b"1"), ("b.md", b"2")]))).unwrap();
    assert_eq!(
        Vault::open(&mut archive, limits).unwrap_err(),
        VaultError::TooManyEntries(1)
    );
}

#[test]
fn skips_files_over_the_file_limit() {
    let limits = VaultLimits {
        max_file_bytes: 4,
        ..LIMITS
    };
    let mut archive =
        open_archive(Cursor::new(zip(&[("a.md", b"ok"), ("big.pdf", b"123456")]))).unwrap();
    let vault = Vault::open(&mut archive, limits).unwrap();
    assert_eq!(vault.skipped.too_large, 1);
    assert!(vault.attachments.is_empty());
}

#[test]
fn refuses_non_zip_and_empty_vaults() {
    assert_eq!(open(b"not a zip".to_vec()).unwrap_err(), VaultError::NotZip);
    assert_eq!(
        open(zip(&[("image.png", b"png")])).unwrap_err(),
        VaultError::Empty
    );
}

#[test]
fn resolves_links_like_obsidian() {
    let vault = open(zip(&[
        ("Projects/Plan.md", b""),
        ("Projects/assets/diagram.png", b""),
        ("Inbox.md", b""),
        ("a/Same.md", b""),
        ("b/Same.md", b""),
        ("a/x.md", b""),
    ]))
    .unwrap();
    let note = |found: Option<Resolved<'_>>| match found {
        Some(Resolved::Note(file)) => Some(file.path.clone()),
        Some(Resolved::Attachment(file)) => Some(format!("file:{}", file.path)),
        None => None,
    };
    let from = "Projects/Plan.md";
    assert_eq!(
        note(vault.resolve(from, "Inbox")).as_deref(),
        Some("Inbox.md")
    );
    assert_eq!(
        note(vault.resolve(from, "inbox.md")).as_deref(),
        Some("Inbox.md")
    );
    assert_eq!(
        note(vault.resolve(from, "Inbox#Heading")).as_deref(),
        Some("Inbox.md")
    );
    assert_eq!(
        note(vault.resolve(from, "#Heading")).as_deref(),
        Some("Projects/Plan.md")
    );
    assert_eq!(
        note(vault.resolve(from, "assets/diagram.png")).as_deref(),
        Some("file:Projects/assets/diagram.png")
    );
    assert_eq!(
        note(vault.resolve("Inbox.md", "diagram.png")).as_deref(),
        Some("file:Projects/assets/diagram.png")
    );
    assert_eq!(
        note(vault.resolve("Inbox.md", "Projects/Plan")).as_deref(),
        Some("Projects/Plan.md")
    );
    assert_eq!(
        note(vault.resolve("Inbox.md", "../Inbox")),
        None,
        "a path above the vault does not resolve"
    );
    assert_eq!(
        note(vault.resolve("Inbox.md", "Same")),
        None,
        "ambiguous name"
    );
    assert_eq!(
        note(vault.resolve("a/x.md", "Same")).as_deref(),
        Some("a/Same.md"),
        "a relative path wins"
    );
    assert_eq!(note(vault.resolve(from, "Missing")), None);
}

#[test]
fn folders_become_pages_and_folder_notes_their_content() {
    let vault = open(vault_zip()).unwrap();
    let tree = build_tree(&vault);
    let nodes: Vec<Row<'_>> = tree
        .nodes
        .iter()
        .map(|node| {
            (
                node.path.as_str(),
                node.kind,
                node.note_path.as_deref(),
                node.parent_path.as_deref(),
                node.child_count,
            )
        })
        .collect();
    assert_eq!(
        nodes,
        [
            (
                "Projects",
                ObsidianImportNodeKind::Folder,
                Some("Projects/Projects.md"),
                None,
                1
            ),
            (
                "Projects/Plan.md",
                ObsidianImportNodeKind::Note,
                Some("Projects/Plan.md"),
                Some("Projects"),
                0
            ),
            (
                "Inbox.md",
                ObsidianImportNodeKind::Note,
                Some("Inbox.md"),
                None,
                0
            ),
        ]
    );
    assert_eq!(tree.nodes[1].title, "Plan");
}

#[test]
fn scan_counts_attachments_and_unresolved_links() {
    let mut archive = open_archive(Cursor::new(vault_zip())).unwrap();
    let tree = scan(&mut archive, LIMITS).unwrap();
    assert_eq!(tree.attachments, 1);
    assert_eq!(tree.unresolved_links, 1);
    assert_eq!(tree.skipped.unreferenced, 1);
    assert_eq!(tree.nodes.len(), 3);
}
