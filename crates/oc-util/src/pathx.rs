//! Path helpers ported from `packages/util/src/path.ts` (the generic pieces),
//! plus the tilde/relative display helpers used across v2.

use std::path::{Path, PathBuf};

fn trim_trailing_separators(value: &str) -> &str {
    value.trim_end_matches(['/', '\\'])
}

/// Final path segment, handling both separators and trailing separators.
#[must_use]
pub fn get_filename(path: &str) -> String {
    let trimmed = trim_trailing_separators(path);
    // The later of the two separator positions, mirroring
    // `Math.max(lastIndexOf("/"), lastIndexOf("\\"))`.
    let index = trimmed.rfind('/').max(trimmed.rfind('\\'));
    match index {
        Some(index) => trimmed[index + 1..].to_string(),
        None => trimmed.to_string(),
    }
}

/// Directory portion with a trailing `/`, normalized to forward slashes.
/// Empty input yields an empty string; a bare filename yields `/`.
#[must_use]
pub fn get_directory(path: &str) -> String {
    if path.is_empty() {
        return String::new();
    }
    let trimmed = trim_trailing_separators(path);
    let parts: Vec<&str> = trimmed.split(['/', '\\']).collect();
    let mut directory = parts[..parts.len() - 1].join("/");
    directory.push('/');
    directory
}

/// Filename shortened to `max_length` chars (ellipsis included) while keeping
/// the extension when it fits.
#[must_use]
pub fn get_filename_truncated(path: &str, max_length: usize) -> String {
    let filename = get_filename(path);
    let chars: Vec<char> = filename.chars().collect();
    if chars.len() <= max_length {
        return filename;
    }
    let extension: Vec<char> = match chars.iter().rposition(|c| *c == '.') {
        Some(index) if index > 0 => chars[index..].to_vec(),
        _ => Vec::new(),
    };
    let available = max_length.saturating_sub(extension.len() + 1);
    if available == 0 {
        let keep = max_length.saturating_sub(1);
        let head: String = chars[..keep].iter().collect();
        return format!("{head}…");
    }
    let head: String = chars[..available].iter().collect();
    let tail: String = extension.iter().collect();
    format!("{head}…{tail}")
}

/// Middle truncation keeping head and tail around a single ellipsis.
///
/// The TypeScript original splices the whole string back for `max_length <= 2`
/// (`slice(-0)` quirk); this port returns the sane `head…` form instead.
#[must_use]
pub fn truncate_middle(text: &str, max_length: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max_length {
        return text.to_string();
    }
    let available = max_length.saturating_sub(1);
    let start = available.div_ceil(2);
    let end = available / 2;
    let head: String = chars[..start.min(chars.len())].iter().collect();
    let tail: String =
        if end == 0 { String::new() } else { chars[chars.len() - end..].iter().collect() };
    format!("{head}…{tail}")
}

fn is_drive(value: &str) -> bool {
    let chars: Vec<char> = value.chars().collect();
    chars.len() == 2 && chars[1] == ':' && chars[0].is_ascii_alphabetic()
}

fn is_windows_path(value: &str) -> bool {
    let chars: Vec<char> = value.chars().collect();
    chars.get(1) == Some(&':') || value.starts_with(r"\\")
}

/// A comparable form of a directory path: forward slashes for Windows paths,
/// no trailing slash except on roots.
#[must_use]
pub fn comparable_path(path: &str) -> String {
    let value = if is_windows_path(path) { path.replace('\\', "/") } else { path.to_string() };
    let trimmed = value.trim_end_matches('/');
    if trimmed.is_empty() && value.starts_with('/') {
        return "/".to_string();
    }
    if is_drive(trimmed) {
        return format!("{trimmed}/");
    }
    trimmed.to_string()
}

fn is_drive_prefixed(key: &str) -> bool {
    let bytes = key.as_bytes();
    bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'/'
}

/// `child` is `parent` or inside it. Windows drive and UNC paths compare
/// case-insensitively.
#[must_use]
pub fn contains_directory(parent: &str, child: &str) -> bool {
    let normalize = |value: &str| {
        let key = comparable_path(value);
        if is_drive_prefixed(&key) || key.starts_with("//") {
            key.to_lowercase()
        } else {
            key
        }
    };
    let root = normalize(parent);
    let target = normalize(child);
    if target == root {
        return true;
    }
    let prefix = if root.ends_with('/') { root } else { format!("{root}/") };
    target.starts_with(&prefix)
}

/// Two paths denote the same directory.
#[must_use]
pub fn same_directory(a: &str, b: &str) -> bool {
    contains_directory(a, b) && contains_directory(b, a)
}

/// Percent-encodes a path segment like `encodeURIComponent` (unreserved set:
/// `A-Z a-z 0-9 - _ . ! ~ * ' ( )`).
fn encode_segment(segment: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(segment.len());
    for byte in segment.as_bytes() {
        let char = *byte as char;
        if char.is_ascii_alphanumeric()
            || matches!(char, '-' | '_' | '.' | '!' | '~' | '*' | '\'' | '(' | ')')
        {
            out.push(char);
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

fn is_drive_letter_start(value: &str) -> bool {
    let chars: Vec<char> = value.chars().collect();
    chars.len() >= 2 && chars[0].is_ascii_alphabetic() && chars[1] == ':'
}

fn is_drive_segment(segment: &str) -> bool {
    let chars: Vec<char> = segment.chars().collect();
    chars.len() == 2 && chars[0].is_ascii_alphabetic() && chars[1] == ':'
}

/// Encodes a path for use in a `file://` URL: backslashes become `/`, Windows
/// drive roots gain a leading `/`, and every segment is percent-encoded while
/// drive letters keep their colon.
#[must_use]
pub fn encode_file_path(filepath: &str) -> String {
    let normalized = filepath.replace('\\', "/");
    let rooted =
        if is_drive_letter_start(&normalized) { format!("/{normalized}") } else { normalized };
    rooted
        .split('/')
        .enumerate()
        .map(|(index, segment)| {
            if index == 1 && is_drive_segment(segment) {
                segment.to_string()
            } else {
                encode_segment(segment)
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Expands `~` and `$HOME` prefixes against `home`
/// (`packages/core/src/config/variable.ts`, `agent.ts`).
#[must_use]
pub fn expand_tilde(input: &str, home: &Path) -> PathBuf {
    let expand = |rest: &str| {
        let mut path = home.to_path_buf();
        if !rest.is_empty() {
            path.push(rest.trim_start_matches('/'));
        }
        path
    };
    if input == "~" || input == "$HOME" {
        return home.to_path_buf();
    }
    if let Some(rest) = input.strip_prefix("~/") {
        return expand(rest);
    }
    if let Some(rest) = input.strip_prefix("$HOME/") {
        return expand(rest);
    }
    PathBuf::from(input)
}

/// Displays `path` relative to `base` when inside it (`.` when equal),
/// otherwise the full path.
#[must_use]
pub fn relative_display(path: &Path, base: &Path) -> String {
    match path.strip_prefix(base) {
        Ok(relative) if relative.as_os_str().is_empty() => ".".to_string(),
        Ok(relative) => relative.to_string_lossy().replace('\\', "/"),
        Err(_) => path.to_string_lossy().replace('\\', "/"),
    }
}

/// Collapses the `home` prefix of `path` to `~` for display.
#[must_use]
pub fn tilde_collapse(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(relative) if relative.as_os_str().is_empty() => "~".to_string(),
        Ok(relative) => format!("~/{}", relative.to_string_lossy().replace('\\', "/")),
        Err(_) => path.to_string_lossy().replace('\\', "/"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Vectors below mirror packages/util/src/path.test.ts.

    #[test]
    fn filenames_across_separators() {
        for (path, filename) in [
            ("/repo/src/index.ts///", "index.ts"),
            ("C:\\repo\\src\\index.ts", "index.ts"),
            ("C:\\repo/src\\file", "file"),
            ("C:/repo\\src/file/\\", "file"),
            ("/", ""),
            ("\\", ""),
            ("/\\/\\", ""),
            ("C:\\", "C:"),
            ("\\\\server\\share\\file", "file"),
            ("file", "file"),
            ("", ""),
        ] {
            assert_eq!(get_filename(path), filename, "get_filename({path:?})");
        }
        assert_eq!(get_filename("/repo/src/index.ts"), "index.ts");
    }

    #[test]
    fn directories_across_separators() {
        assert_eq!(get_directory("/repo/src/index.ts"), "/repo/src/");
        assert_eq!(get_directory("C:\\repo\\src\\index.ts"), "C:/repo/src/");
        assert_eq!(get_directory("C:\\repo/src\\file"), "C:/repo/src/");
        assert_eq!(get_directory("\\\\server\\share\\file"), "//server/share/");
        assert_eq!(get_directory("file"), "/");
        assert_eq!(get_directory(""), "");
    }

    #[test]
    fn filename_truncation_is_stable() {
        assert_eq!(get_filename_truncated("/repo/long-component-name.tsx", 16), "long-compon….tsx");
        assert_eq!(get_filename_truncated("short.ts", 20), "short.ts");
        assert_eq!(get_filename_truncated("exactly20characte.rs", 20), "exactly20characte.rs");
    }

    #[test]
    fn middle_truncation_is_stable() {
        assert_eq!(truncate_middle("abcdefghijklmnop", 9), "abcd…mnop");
        assert_eq!(truncate_middle("short", 20), "short");
        assert_eq!(truncate_middle("abcd", 4), "abcd");
    }

    #[test]
    fn comparable_paths() {
        assert_eq!(comparable_path("/a/b/"), "/a/b");
        assert_eq!(comparable_path("/a/b"), "/a/b");
        assert_eq!(comparable_path("///"), "/");
        assert_eq!(comparable_path("/"), "/");
        assert_eq!(comparable_path("C:\\"), "C:/");
        assert_eq!(comparable_path("C:\\repo\\"), "C:/repo");
        // POSIX relative backslashes are untouched (only Windows-rooted paths
        // normalize their separators).
        assert_eq!(comparable_path(r"relative\path"), r"relative\path");
    }

    #[test]
    fn contains_and_same_directories() {
        assert!(contains_directory("/repo", "/repo"));
        assert!(contains_directory("/repo", "/repo/sub"));
        assert!(!contains_directory("/repo", "/repository"));
        assert!(!contains_directory("/repo/sub", "/repo"));
        assert!(same_directory("/repo/", "/repo"));
        assert!(!same_directory("/repo", "/repo/sub"));
        // Windows drive paths compare case-insensitively.
        assert!(contains_directory("C:\\Repo", "c:/repo/sub"));
        assert!(!contains_directory("C:\\Repo", "D:/repo/sub"));
    }

    #[test]
    fn encodes_file_paths() {
        for (input, expected) in [
            ("/home/user/project/README.md", "/home/user/project/README.md"),
            ("/home/user/file#name with spaces.txt", "/home/user/file%23name%20with%20spaces.txt"),
            ("/path/to/file?name.txt", "/path/to/file%3Fname.txt"),
            ("/path/to/file%name.txt", "/path/to/file%25name.txt"),
            ("/home/user/文档/README.md", "/home/user/%E6%96%87%E6%A1%A3/README.md"),
            ("/", "/"),
            ("", ""),
            ("//path//to///file.txt", "//path//to///file.txt"),
            ("src/components/App.tsx", "src/components/App.tsx"),
            ("src\\components\\App.tsx", "src/components/App.tsx"),
            ("D:\\dev\\projects\\opencode\\README.bs.md", "/D:/dev/projects/opencode/README.bs.md"),
            (
                "C:\\Program Files\\MyApp\\file with spaces.txt",
                "/C:/Program%20Files/MyApp/file%20with%20spaces.txt",
            ),
            ("C:\\", "/C:/"),
            ("c:\\users\\test\\file.txt", "/c:/users/test/file.txt"),
            ("D:", "/D:"),
            ("/D:/path/file.txt", "/D:/path/file.txt"),
        ] {
            assert_eq!(encode_file_path(input), expected, "encode_file_path({input:?})");
        }
    }

    #[test]
    fn expands_tilde_forms() {
        let home = Path::new("/home/dev");
        assert_eq!(expand_tilde("~", home), PathBuf::from("/home/dev"));
        assert_eq!(expand_tilde("~/", home), PathBuf::from("/home/dev"));
        assert_eq!(expand_tilde("~/src/main.rs", home), PathBuf::from("/home/dev/src/main.rs"));
        assert_eq!(expand_tilde("$HOME", home), PathBuf::from("/home/dev"));
        assert_eq!(expand_tilde("$HOME/x", home), PathBuf::from("/home/dev/x"));
        assert_eq!(expand_tilde("/etc/passwd", home), PathBuf::from("/etc/passwd"));
        assert_eq!(expand_tilde("relative", home), PathBuf::from("relative"));
    }

    #[test]
    fn relative_display_forms() {
        let base = Path::new("/repo");
        assert_eq!(relative_display(Path::new("/repo"), base), ".");
        assert_eq!(relative_display(Path::new("/repo/src/a.rs"), base), "src/a.rs");
        assert_eq!(relative_display(Path::new("/elsewhere"), base), "/elsewhere");
    }

    #[test]
    fn tilde_collapse_forms() {
        let home = Path::new("/home/dev");
        assert_eq!(tilde_collapse(Path::new("/home/dev"), home), "~");
        assert_eq!(tilde_collapse(Path::new("/home/dev/work/a.md"), home), "~/work/a.md");
        assert_eq!(tilde_collapse(Path::new("/usr/bin"), home), "/usr/bin");
    }
}
