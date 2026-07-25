//! `@`-file mention autocomplete: filesystem path completion for the prompt.
//!
//! When the user types `@src/re…` in the prompt, [`suggest`] lists entries of
//! the referenced directory (relative to the session's workdir), fuzzy-ranked
//! against the partial filename. The worker row shows the top hits and Tab
//! commits the best one — directories keep a trailing `/` so the user can
//! drill in, files get a trailing space.

use std::path::Path;

use crate::fuzzy;

/// A single file/directory completion candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSuggestion {
    /// Path text to substitute after the `@`, relative to the base dir —
    /// e.g. `"src/render/"` for a directory or `"Cargo.toml"` for a file.
    /// Directories carry a trailing `/`.
    pub display: String,
    /// Whether this entry is a directory (drives Tab's trailing char).
    pub is_dir: bool,
}

/// Rank filesystem entries for the `@`-mention `query` under `base`.
///
/// `query` is the text typed after `@` (may contain `/` to descend into
/// subdirectories, and may be empty right after typing `@`). Hidden entries
/// (dotfiles) are only offered when the partial name itself starts with `.`.
/// Returns at most `limit` results, best match first. Any I/O error (missing
/// or unreadable directory) yields an empty list rather than an error.
#[must_use]
pub fn suggest(base: &Path, query: &str, limit: usize) -> Vec<FileSuggestion> {
    // Split the query into a directory prefix (kept verbatim, trailing `/`
    // included) and the partial filename we actually fuzzy-match.
    let (dir_part, file_part) = match query.rfind('/') {
        Some(i) => (&query[..=i], &query[i + 1..]),
        None => ("", query),
    };

    let dir = base.join(dir_part);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };

    let mut scored: Vec<(f64, FileSuggestion)> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        // Hide dotfiles unless the user is explicitly typing a leading dot.
        if name.starts_with('.') && !file_part.starts_with('.') {
            continue;
        }
        let Some(score) = fuzzy::score(file_part, name) else {
            continue;
        };
        let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
        let mut display = format!("{dir_part}{name}");
        if is_dir {
            display.push('/');
        }
        scored.push((score, FileSuggestion { display, is_dir }));
    }

    scored.sort_by(|a, b| a.0.total_cmp(&b.0));
    scored.truncate(limit);
    scored.into_iter().map(|(_, s)| s).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Build a throwaway directory tree under the OS temp dir. Uses the test
    /// name as a unique suffix (workflow scripts forbid `Date::now`, but tests
    /// run under cargo so a fixed-per-test name is enough given cleanup).
    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("codeoid-mention-{tag}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("src/render")).unwrap();
        fs::write(dir.join("Cargo.toml"), "").unwrap();
        fs::write(dir.join("README.md"), "").unwrap();
        fs::write(dir.join(".hidden"), "").unwrap();
        fs::write(dir.join("src/main.rs"), "").unwrap();
        fs::write(dir.join("src/render/mod.rs"), "").unwrap();
        dir
    }

    #[test]
    fn empty_query_lists_top_level_visible_entries() {
        let dir = scratch("empty");
        let out = suggest(&dir, "", 50);
        let names: Vec<&str> = out.iter().map(|s| s.display.as_str()).collect();
        assert!(names.contains(&"Cargo.toml"));
        assert!(names.contains(&"README.md"));
        assert!(
            names.contains(&"src/"),
            "dirs get a trailing slash: {names:?}"
        );
        // Dotfiles stay hidden unless requested.
        assert!(!names.iter().any(|n| n.starts_with('.')));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn fuzzy_ranks_filename() {
        let dir = scratch("rank");
        let out = suggest(&dir, "carg", 10);
        assert_eq!(out.first().map(|s| s.display.as_str()), Some("Cargo.toml"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn descends_into_subdirectory_prefix() {
        let dir = scratch("descend");
        let out = suggest(&dir, "src/ma", 10);
        // dir_part ("src/") is preserved on the completion.
        assert_eq!(out.first().map(|s| s.display.as_str()), Some("src/main.rs"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn nested_directory_keeps_trailing_slash() {
        let dir = scratch("nested");
        let out = suggest(&dir, "src/rend", 10);
        assert_eq!(out.first().map(|s| s.display.as_str()), Some("src/render/"));
        assert!(out[0].is_dir);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn leading_dot_reveals_hidden() {
        let dir = scratch("hidden");
        let out = suggest(&dir, ".hid", 10);
        assert_eq!(out.first().map(|s| s.display.as_str()), Some(".hidden"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_directory_is_empty_not_error() {
        let out = suggest(Path::new("/no/such/path/anywhere"), "x", 10);
        assert!(out.is_empty());
    }

    #[test]
    fn limit_is_respected() {
        let dir = scratch("limit");
        let out = suggest(&dir, "", 1);
        assert_eq!(out.len(), 1);
        fs::remove_dir_all(&dir).ok();
    }
}
