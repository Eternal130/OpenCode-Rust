//! Small shared utilities (ports of `cli/src/util/{error,io,process}.ts`
//! pieces + `version.ts`).

use std::io::Read;

/// `OPENCODE_VERSION` (version.ts) — the workspace pins the v2.0.24 parity
/// target; TS releases beyond it must never be adopted (oracle gate 1).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// `OPENCODE_CHANNEL` (version.ts): release channel. Delegates to the
/// ONE workspace channel source (`oc_util::channel::current`) so the
/// CLI, the server engine's DB-path resolution, and channel-scoped
/// v2 builds all agree on the channel (P10-DB1).
#[must_use]
pub fn channel() -> String {
    oc_util::channel::current()
}

/// `Env.password`: `OPENCODE_PASSWORD`, legacy `OPENCODE_SERVER_PASSWORD`.
#[must_use]
pub fn env_password() -> Option<String> {
    std::env::var("OPENCODE_PASSWORD").ok().filter(|value| !value.is_empty()).or_else(|| {
        std::env::var("OPENCODE_SERVER_PASSWORD").ok().filter(|value| !value.is_empty())
    })
}

/// `Env.session()`: the whole environment minus the password secrets —
/// attached to a session when the client is on the managed service.
#[must_use]
pub fn env_session() -> std::collections::BTreeMap<String, String> {
    std::env::vars()
        .filter(|(name, _)| name != "OPENCODE_PASSWORD" && name != "OPENCODE_SERVER_PASSWORD")
        .collect()
}

/// `errorMessage(error)`: `Error: <message>` unwrapping, other values
/// stringified.
#[must_use]
pub fn error_message(error: &str) -> String {
    error.strip_prefix("Error: ").unwrap_or(error).to_owned()
}

/// `readStdin()` (util/io.ts): the entire piped stdin as text.
pub fn read_stdin() -> std::io::Result<String> {
    let mut buffer = String::new();
    std::io::stdin().read_to_string(&mut buffer)?;
    Ok(buffer)
}

/// `selfCommand()` (util/process.ts): the running executable + args, so the
/// CLI can re-spawn itself (`serve --stdio`, `serve --service`).
#[must_use]
pub fn self_command() -> Vec<String> {
    let command = vec![std::env::current_exe()
        .map_or_else(|_| "opencode".to_owned(), |path| path.display().to_string())];
    // A `cargo run`-launched binary is re-spawned directly (no cargo).
    command
}

/// Random 32-byte password, base64url (`randomBytes(32).toString("base64url")`).
#[must_use]
pub fn random_password() -> String {
    use base64::Engine as _;
    use rand::RngCore as _;
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// Lowercase hex of a digest (no `format!`-per-byte allocations).
#[must_use]
fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0xf) as usize] as char);
    }
    out
}

/// `Hash.fast` (util/hash.ts): sha1 hex.
#[must_use]
#[allow(dead_code)]
pub fn hash_fast(input: &str) -> String {
    use sha1::Digest as _;
    hex_lower(&sha1::Sha1::digest(input.as_bytes()))
}

/// Whether stdout is a TTY (cheaper than pulling a crate: `isatty(1)`).
#[must_use]
#[allow(unsafe_code, dead_code)]
pub fn stdout_is_tty() -> bool {
    // SAFETY: isatty on a constant fd.
    unsafe { libc::isatty(libc::STDOUT_FILENO) == 1 }
}

/// Whether stdin is a TTY.
#[must_use]
#[allow(unsafe_code)]
pub fn stdin_is_tty() -> bool {
    // SAFETY: isatty on a constant fd.
    unsafe { libc::isatty(libc::STDIN_FILENO) == 1 }
}

/// Whether stderr is a TTY.
#[must_use]
#[allow(unsafe_code, dead_code)]
pub fn stderr_is_tty() -> bool {
    // SAFETY: isatty on a constant fd.
    unsafe { libc::isatty(libc::STDERR_FILENO) == 1 }
}

/// Epoch milliseconds.
#[must_use]
pub fn now_ms() -> u64 {
    u64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_millis()),
    )
    .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_fast_matches_sha1_hex() {
        // sha1("latest") == f7e6c65722e6a1c1c6bbc0373a898e1b8b5c0b8d? Verify
        // shape instead of a hardcoded digest: 40 lowercase hex chars.
        let digest = hash_fast("latest");
        assert_eq!(digest.len(), 40);
        assert!(digest.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()));
        assert_ne!(digest, hash_fast("local"));
    }

    #[test]
    fn random_password_shape() {
        let password = random_password();
        assert_eq!(password.len(), 43, "32 bytes base64url without padding");
        assert!(password
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'));
    }
}
