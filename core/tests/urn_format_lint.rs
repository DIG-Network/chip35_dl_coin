//! Regression guard for the canonical NFT resource-identifier format (dig_ecosystem #688/#686).
//!
//! An NFT's `data_uris` / `metadata_uris` name their media by a **bare root-pinned DIG URN**
//! (`urn:dig:chia:<storeId>:<root>/<key>`), letting the `dig-urn-resolver` decide WHERE to fetch.
//! The malformed double-scheme (a `dig://` prefix wrapping a `urn:dig:` URN) is the bug this guard
//! prevents from ever returning: `dig://` is the §21 remote-transport locator, not a content/
//! resource scheme, so wrapping a URN in it produces a nonsensical identifier.
//!
//! The chip35 builders never construct these URNs themselves — the URI list is caller-supplied and
//! merely cloned through — so the contract lives in this crate's fixtures, doc comments, and docs.
//! This test scans the whole repository's source + documentation and fails on any surviving
//! double-scheme literal, and asserts every NFT URN literal is root-pinned.
//!
//! NOTE: the forbidden prefix is assembled from parts (`concat!`) so this guard's own source does
//! not contain the contiguous literal it forbids (which would make it self-fail).

use std::fs;
use std::path::{Path, PathBuf};

/// The forbidden double-scheme prefix: a `dig://` wrapping a `urn:dig:` URN. Assembled from parts
/// so this file's own source does not contain the contiguous literal it forbids.
const FORBIDDEN_DOUBLE_SCHEME: &str = concat!("dig:", "//", "urn:dig:");

/// The canonical NFT resource-identifier prefix (bare, root-pinned URN).
const CANONICAL_URN_PREFIX: &str = "urn:dig:chia:";

/// Directories that are not part of the authored source and must not be scanned (build output,
/// vendored dependencies, VCS metadata).
const SKIP_DIRS: &[&str] = &["target", "venv", ".git", "node_modules", "pkg", "dist"];

/// File extensions whose contents express the canonical-format contract to humans and consumers.
const SCANNED_EXTENSIONS: &[&str] = &["rs", "md", "ts", "json"];

/// This guard's own file is excluded: it necessarily names the forbidden and canonical patterns
/// (including negative examples) to describe what it checks, and must not flag itself.
const SELF_FILE: &str = "urn_format_lint.rs";

#[test]
fn no_malformed_dig_scheme_wrapping_a_urn_anywhere_in_repo() {
    let offenders: Vec<String> = repo_source_files()
        .into_iter()
        .filter_map(|path| {
            let contents = fs::read_to_string(&path).ok()?;
            contents
                .contains(FORBIDDEN_DOUBLE_SCHEME)
                .then(|| path.display().to_string())
        })
        .collect();

    assert!(
        offenders.is_empty(),
        "found the malformed `{FORBIDDEN_DOUBLE_SCHEME}` double-scheme (dig:// wrapping a URN) — \
         NFT media URIs must be a bare root-pinned URN `{CANONICAL_URN_PREFIX}<store>:<root>/<key>` \
         (dig_ecosystem #688). Offending files:\n  {}",
        offenders.join("\n  ")
    );
}

#[test]
fn every_nft_urn_literal_is_root_pinned() {
    // A root-pinned URN carries the rootHash after the storeId — `urn:dig:chia:<store>:<root>/…` —
    // which the rpc.dig.net resolution tier requires. A rootless `urn:dig:chia:<store>/…` (a colon
    // then directly a slash) would fail rpc-tier resolution, so it is rejected here.
    let violations: Vec<String> = repo_source_files()
        .into_iter()
        .flat_map(|path| {
            let contents = fs::read_to_string(&path).unwrap_or_default();
            contents
                .match_indices(CANONICAL_URN_PREFIX)
                .filter_map(|(idx, _)| {
                    let tail = &contents[idx + CANONICAL_URN_PREFIX.len()..];
                    (!is_root_pinned(tail))
                        .then(|| format!("{}: …{}", path.display(), snippet(tail)))
                })
                .collect::<Vec<_>>()
        })
        .collect();

    assert!(
        violations.is_empty(),
        "found NFT URN literal(s) that are not root-pinned (missing the `:<root>` segment before \
         the path) — required for rpc-tier resolution (dig_ecosystem #688):\n  {}",
        violations.join("\n  ")
    );
}

/// True when the text following `urn:dig:chia:` has a `<storeId>:<root>` shape — i.e. a second
/// colon appears before the first path slash (or before the literal ends).
fn is_root_pinned(tail_after_prefix: &str) -> bool {
    let boundary = tail_after_prefix
        .find(['/', '"', '\'', ' ', '?'])
        .unwrap_or(tail_after_prefix.len());
    tail_after_prefix[..boundary].contains(':')
}

/// A short, single-line excerpt for diagnostics.
fn snippet(tail: &str) -> String {
    tail.chars().take(40).collect::<String>().replace('\n', " ")
}

/// Every authored source/doc file in the repository (crate root = `core/`, so the repo root is its
/// parent), excluding build output and vendored trees.
fn repo_source_files() -> Vec<PathBuf> {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("core/ always has a parent (the repo root)")
        .to_path_buf();
    let mut files = Vec::new();
    collect_scannable_files(&repo_root, &mut files);
    files
}

/// True for an authored file this guard should scan: a tracked extension, and not the guard itself.
fn is_scannable_file(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str());
    if name == Some(SELF_FILE) {
        return false;
    }
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| SCANNED_EXTENSIONS.contains(&ext))
}

fn collect_scannable_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let skip = path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|name| SKIP_DIRS.contains(&name));
            if !skip {
                collect_scannable_files(&path, out);
            }
        } else if is_scannable_file(&path) {
            out.push(path);
        }
    }
}
