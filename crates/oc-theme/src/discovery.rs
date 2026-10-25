//! Custom theme discovery (`tui/theme/discovery.ts` +
//! `tui/util/config-directories.ts`).
//!
//! The reference's `discoverThemes(directories)` scans `<directory>/themes`
//! for `.json` files (regular files and symlinks), sorts each directory's
//! entries by full path, and folds them into a name-keyed record — so a
//! theme with the same name in a LATER directory overrides an earlier one.
//! Missing `themes/` directories (ENOENT) are simply skipped.
//!
//! Divergence from the reference (documented, terminal-context): ref's
//! `JSON.parse` failure and read failures propagate out of `discoverThemes`
//! and abort theme loading entirely; the consuming Rust pipeline keeps the
//! app alive by skipping unreadable/invalid files (logged by the caller),
//! matching how ref's `setCustomThemes` drops invalid sources one entry
//! later. Parsing/validation stays with the caller — this module returns
//! raw JSON sources, mirroring what ref's `ThemeDocumentSource` records
//! hold before `parseTheme` runs.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A discovered custom theme: `(name, raw JSON source)`. The name is the
/// file stem (`path.basename(file, ".json")` in ref).
pub type DiscoveredTheme = (String, String);

/// Discover custom themes across the given directories
/// (`discovery.ts discoverThemes`).
///
/// For each directory the `<directory>/themes/*.json` files (files +
/// symlinks, sorted by full path) are collected; a name found in a later
/// directory replaces the earlier entry (ref's record fold), while the
/// first-appearance ordering is kept. Unreadable files are skipped.
#[must_use]
pub fn discover_themes(directories: &[PathBuf]) -> Vec<DiscoveredTheme> {
    let mut found: Vec<DiscoveredTheme> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for directory in directories {
        for (name, source) in discover_in_directory(directory) {
            match index.get(&name) {
                Some(&slot) => found[slot] = (name, source),
                None => {
                    index.insert(name.clone(), found.len());
                    found.push((name, source));
                }
            }
        }
    }
    found
}

/// Scan one directory's `themes/` folder (`readdir` + ENOENT → empty).
fn discover_in_directory(directory: &Path) -> Vec<DiscoveredTheme> {
    let theme_directory = directory.join("themes");
    let Ok(entries) = std::fs::read_dir(&theme_directory) else {
        return Vec::new(); // ENOENT / unreadable: skipped like ref's catch
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_type()
                .is_ok_and(|kind| kind.is_file() || kind.is_symlink())
                && entry.path().extension().is_some_and(|ext| ext == "json")
        })
        .map(|entry| entry.path())
        .collect();
    files.sort(); // ref sorts the full paths before folding
    files
        .into_iter()
        .filter_map(|file| {
            let name = file.file_stem()?.to_str()?.to_owned();
            std::fs::read_to_string(&file).ok().map(|source| (name, source))
        })
        .collect()
}

/// The TUI config directories for theme discovery
/// (`config-directories.ts configDirectories`): the config root, then a
/// `.opencode` directory in every cwd ancestor, ordered root → leaf so
/// later (nearer) directories win in [`discover_themes`]. Duplicates are
/// deduped (`new Set` in ref, which also keeps first occurrence order).
#[must_use]
pub fn config_directories(config_root: &Path, cwd: &Path) -> Vec<PathBuf> {
    let mut directories: Vec<PathBuf> = vec![config_root.to_path_buf()];
    let cwd = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    let mut ancestors: Vec<PathBuf> = cwd.ancestors().map(Path::to_path_buf).collect();
    ancestors.reverse(); // filesystem root first, like ref's `ancestors()`
    for ancestor in ancestors {
        let directory = ancestor.join(".opencode");
        if !directories.contains(&directory) {
            directories.push(directory);
        }
    }
    directories
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir()
            .join("oc-theme-discovery-tests")
            .join(format!("{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn discovers_json_files_sorted_and_skips_other_extensions() {
        let root = temp_root("sorted");
        let themes = root.join("themes");
        fs::create_dir_all(&themes).unwrap();
        fs::write(themes.join("b.json"), r#"{"base":1}"#).unwrap();
        fs::write(themes.join("a.json"), r#"{"base":2}"#).unwrap();
        fs::write(themes.join("c.txt"), "not a theme").unwrap();
        let found = discover_themes(&[root.clone()]);
        assert_eq!(
            found,
            vec![
                ("a".to_owned(), r#"{"base":2}"#.to_owned()),
                ("b".to_owned(), r#"{"base":1}"#.to_owned()),
            ]
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_themes_directory_is_skipped() {
        let root = temp_root("missing");
        assert_eq!(discover_themes(&[root.clone()]), Vec::<DiscoveredTheme>::new());
        // A completely absent directory is equally fine (ENOENT → empty).
        assert_eq!(
            discover_themes(&[root.join("nope")]),
            Vec::<DiscoveredTheme>::new()
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn later_directories_override_earlier_names() {
        let first = temp_root("first");
        let second = temp_root("second");
        fs::create_dir_all(first.join("themes")).unwrap();
        fs::create_dir_all(second.join("themes")).unwrap();
        fs::write(first.join("themes/aura.json"), r#"{"from":"first"}"#).unwrap();
        fs::write(first.join("themes/only.json"), r#"{"from":"first"}"#).unwrap();
        fs::write(second.join("themes/aura.json"), r#"{"from":"second"}"#).unwrap();
        let found = discover_themes(&[first.clone(), second.clone()]);
        assert_eq!(
            found,
            vec![
                ("aura".to_owned(), r#"{"from":"second"}"#.to_owned()),
                ("only".to_owned(), r#"{"from":"first"}"#.to_owned()),
            ]
        );
        let _ = fs::remove_dir_all(&first);
        let _ = fs::remove_dir_all(&second);
    }

    #[test]
    fn config_directories_walk_cwd_ancestors() {
        let root = temp_root("ancestors");
        let cwd = root.join("a/b/c");
        fs::create_dir_all(&cwd).unwrap();
        let cwd = fs::canonicalize(&cwd).unwrap();
        let directories = config_directories(&root.join("cfg"), &cwd);
        let mut expected = vec![root.join("cfg")];
        let mut ancestors: Vec<PathBuf> = cwd.ancestors().map(Path::to_path_buf).collect();
        ancestors.reverse();
        expected.extend(ancestors.into_iter().map(|a| a.join(".opencode")));
        assert_eq!(directories, expected);
        // Root → leaf order: the cwd's `.opencode` is last (later wins).
        assert_eq!(directories.last(), Some(&cwd.join(".opencode")));
        let _ = fs::remove_dir_all(&root);
    }
}
