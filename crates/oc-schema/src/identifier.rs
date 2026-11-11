//! Port of `packages/schema/src/identifier.ts`.
//!
//! Bit-twiddling below deliberately mirrors the TS implementation; pedantic
//! style lints are silenced where fidelity wins.
#![allow(
    clippy::map_unwrap_or,
    clippy::format_push_string,
    clippy::unnecessary_cast,
    clippy::needless_bool,
    clippy::if_same_then_else
)]
//!
//! v2 identifiers are 26-character strings: 12 hex characters encoding a
//! 48-bit `timestamp * 0x1000 + counter` value (optionally bitwise-inverted for
//! descending ids), followed by 14 characters drawn from a 62-symbol alphabet.

use std::sync::atomic::{AtomicI64, Ordering};

const LENGTH: usize = 26;
const CHARS: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

static LAST_TIMESTAMP: AtomicI64 = AtomicI64::new(0);
static COUNTER: AtomicI64 = AtomicI64::new(0);

thread_local! {
    static RNG_STATE: std::cell::Cell<u64> =
        std::cell::Cell::new(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0x9e37_79b9_7f4a_7c15, |d| d.as_nanos() as u64)
                | 1,
        );
}

fn random_byte() -> u8 {
    RNG_STATE.with(|state| {
        let mut x = state.get();
        // xorshift64*
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        state.set(x);
        (x.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 32) as u8
    })
}

fn create(descending: bool, timestamp_millis: i64) -> String {
    // G-380③: the naive check-then-act (load LAST_TIMESTAMP, then store
    // LAST_TIMESTAMP + COUNTER as two steps) lets two threads on the
    // same NEW millisecond both "transition" and both observe counter 1,
    // duplicating counter prefixes. The ref is single-threaded
    // (identifier.ts mutates plain lets); the port serializes exactly
    // that sequence with a mutex — atomic cleverness (CAS re-arming)
    // still leaves a window where a re-arm clobbers live counters.
    static CREATE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = CREATE_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let last = LAST_TIMESTAMP.load(Ordering::Relaxed);
    let counter = if last == timestamp_millis {
        COUNTER.fetch_add(1, Ordering::Relaxed) + 1
    } else {
        LAST_TIMESTAMP.store(timestamp_millis, Ordering::Relaxed);
        COUNTER.store(1, Ordering::Relaxed);
        1
    };

    let current = timestamp_millis * 0x1000 + counter;
    let value = if descending { !current } else { current };

    let mut out = String::with_capacity(LENGTH);
    for index in 0..6 {
        let byte = ((value >> (40 - 8 * index)) & 0xff) as u8;
        out.push_str(&format!("{byte:02x}")); // pedantic: format_push_string allowed above
    }
    for _ in 0..(LENGTH - 12) {
        out.push(CHARS[(random_byte() % 62) as usize] as char);
    }
    out
}

/// Monotonically increasing (ascending) identifier: later timestamps sort
/// lexicographically later.
pub fn ascending() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64);
    create(false, now)
}

/// Monotonically decreasing identifier: later timestamps sort lexicographically
/// earlier. Used for `SessionID` so newer sessions list first.
pub fn descending() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64);
    create(true, now)
}

/// Deterministic constructor used by tests: mirrors `Identifier.create`.
pub fn create_at(descending: bool, timestamp_millis: i64) -> String {
    create(descending, timestamp_millis)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascending_shape() {
        let id = create_at(false, 1_700_000_000_000);
        assert_eq!(id.len(), 26);
        assert!(id[..12].chars().all(|c| c.is_ascii_hexdigit()));
        assert!(id[12..].bytes().all(|b| CHARS.contains(&b)));
    }

    #[test]
    fn descending_inverts_timestamp() {
        let up = create_at(false, 1_700_000_000_000);
        let down = create_at(true, 1_700_000_000_000);
        assert_eq!(up.len(), 26);
        assert_eq!(down.len(), 26);
        // Same magnitude, inverted bit pattern: high hex digit differs.
        assert_ne!(up[..2].to_string(), down[..2].to_string());
    }

    #[test]
    fn ascending_orders_by_time() {
        let early = create_at(false, 1_700_000_000_000);
        let late = create_at(false, 1_700_000_010_000);
        assert!(early < late);
    }

    #[test]
    fn descending_orders_by_time_reversed() {
        let early = create_at(true, 1_700_000_000_000);
        let late = create_at(true, 1_700_000_010_000);
        assert!(late < early);
    }

    /// G-380③: concurrent generators on the SAME millisecond take unique
    /// counter slots — the encoded `timestamp * 0x1000 + counter` prefixes
    /// are all distinct (the old check-then-act duplicated counter 1).
    #[test]
    fn concurrent_same_millisecond_counters_are_unique() {
        const THREADS: usize = 16;
        const EACH: usize = 64;
        let prefixes = std::sync::Mutex::new(std::collections::HashSet::new());
        std::thread::scope(|scope| {
            for _ in 0..THREADS {
                scope.spawn(|| {
                    for _ in 0..EACH {
                        let id = create_at(false, 1_700_000_123_456);
                        prefixes.lock().unwrap().insert(id[..12].to_string());
                    }
                });
            }
        });
        let prefixes = prefixes.into_inner().unwrap();
        assert_eq!(
            prefixes.len(),
            THREADS * EACH,
            "every same-millisecond id must carry a distinct counter prefix"
        );
    }
}
