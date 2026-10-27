//! opencode util: XDG roots, retry, hash, wildcard, flock, determinism traits.

pub mod bunman;
pub mod channel;
pub mod determinism;
pub mod encode;
pub mod flock;
pub mod glob;
pub mod global_roots;
pub mod hash;
pub mod localecmp;
pub mod npm_config;
pub mod pathx;
pub mod procutil;
pub mod retry;
pub mod wildcard;

/// Shared guard serializing environment-mutating tests across modules
/// (`cargo test` runs test threads inside one process).
#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};

    static ENV: OnceLock<Mutex<()>> = OnceLock::new();

    pub fn lock_env() -> MutexGuard<'static, ()> {
        ENV.get_or_init(|| Mutex::new(())).lock().unwrap_or_else(PoisonError::into_inner)
    }
}
