//! Deterministic seams for time, randomness, and identifiers.
//!
//! Every module that needs "now", jitter, or a fresh ID should depend on these
//! traits so tests can inject [`FixedClock`], [`SequentialRng`], and
//! [`SequentialIdGen`].

use std::fmt::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

/// Wall-clock time source.
pub trait Clock: Send + Sync {
    /// Current wall time.
    fn now(&self) -> SystemTime;

    /// Milliseconds since the unix epoch (negative before 1970), like `Date.now()`.
    fn unix_ms(&self) -> i64 {
        match self.now().duration_since(UNIX_EPOCH) {
            Ok(delta) => i64::try_from(delta.as_millis()).unwrap_or(i64::MAX),
            Err(err) => -i64::try_from(err.duration().as_millis()).unwrap_or(i64::MAX),
        }
    }
}

/// Randomness source (unit jitter, wide integers).
pub trait RngSource: Send + Sync {
    /// Uniform `f64` in `[0, 1)`.
    fn next_f64(&self) -> f64;

    /// Uniform `u128`.
    fn next_u128(&self) -> u128;
}

/// Prefixed identifier generator (`ses_...`, `msg_...`, ...).
pub trait IdGen: Send + Sync {
    /// Fresh identifier with the given prefix.
    fn new_id(&self, prefix: &str) -> String;
}

/// Production clock backed by the operating system.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}

/// Production randomness backed by `rand`.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemRng;

impl RngSource for SystemRng {
    fn next_f64(&self) -> f64 {
        use rand::Rng;
        rand::rng().random::<f64>()
    }

    fn next_u128(&self) -> u128 {
        use rand::Rng;
        rand::rng().random::<u128>()
    }
}

/// Production ID generator: `prefix_<uuid v4 hex>`.
#[derive(Debug, Clone, Copy, Default)]
pub struct UuidV4IdGen;

impl IdGen for UuidV4IdGen {
    fn new_id(&self, prefix: &str) -> String {
        format!("{}_{}", prefix, uuid::Uuid::new_v4().simple())
    }
}

/// The 62-character alphabet from `packages/schema/src/identifier.ts`.
const BASE62: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// Production ID generator ported from `packages/schema/src/identifier.ts`:
/// 26 chars where the first 12 hex chars carry `(timestamp << 12) + counter`
/// (lexicographically orderable within a process) and the remaining 14 chars
/// are random base 62.
pub struct AscendingIdGen {
    clock: Arc<dyn Clock>,
    rng: Arc<dyn RngSource>,
    state: Mutex<(i64, u64)>,
}

impl AscendingIdGen {
    /// Creates a generator from the given seams.
    pub fn new(clock: Arc<dyn Clock>, rng: Arc<dyn RngSource>) -> Self {
        Self { clock, rng, state: Mutex::new((0, 0)) }
    }

    /// Generates a raw 26-char identifier; ascending sorts forward in time,
    /// descending (bitwise complement) sorts backward.
    pub fn generate(&self, descending: bool) -> String {
        let timestamp = self.clock.unix_ms();
        let counter = {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            if timestamp != state.0 {
                state.0 = timestamp;
                state.1 = 0;
            }
            state.1 += 1;
            state.1
        };
        let current = (i128::from(timestamp) << 12).wrapping_add(i128::from(counter));
        let value = if descending { !current } else { current };
        let mut id = String::with_capacity(26);
        for index in 0..6 {
            let shift = 40 - 8 * index;
            let byte = u8::try_from((value >> shift) & 0xff).expect("masked to a byte");
            let _ = write!(id, "{byte:02x}");
        }
        for _ in 0..14 {
            let byte = u8::try_from(self.rng.next_u128() & 0xff).expect("masked to a byte");
            id.push(char::from(BASE62[usize::from(byte % 62)]));
        }
        id
    }
}

impl IdGen for AscendingIdGen {
    fn new_id(&self, prefix: &str) -> String {
        format!("{}_{}", prefix, self.generate(false))
    }
}

/// Test clock pinned to a fixed instant.
#[derive(Debug, Clone)]
pub struct FixedClock(SystemTime);

impl FixedClock {
    /// Creates a clock stuck at `time`.
    #[must_use]
    pub fn new(time: SystemTime) -> Self {
        Self(time)
    }

    /// Creates a clock stuck at the unix epoch.
    #[must_use]
    pub fn epoch() -> Self {
        Self(UNIX_EPOCH)
    }
}

impl Clock for FixedClock {
    fn now(&self) -> SystemTime {
        self.0
    }
}

/// Test RNG that returns an increasing, fully predictable sequence.
#[derive(Debug, Default)]
pub struct SequentialRng {
    counter: AtomicU64,
}

impl SequentialRng {
    /// Creates a generator counting from zero.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl RngSource for SequentialRng {
    #[allow(clippy::cast_precision_loss)] // deterministic test values only
    fn next_f64(&self) -> f64 {
        let n = self.counter.fetch_add(1, Ordering::SeqCst);
        f64::from(u32::try_from(n % 1_000_000).unwrap_or(0)) / 1_000_000.0
    }

    fn next_u128(&self) -> u128 {
        u128::from(self.counter.fetch_add(1, Ordering::SeqCst))
    }
}

/// Test ID generator producing `prefix_1`, `prefix_2`, ...
#[derive(Debug, Default)]
pub struct SequentialIdGen {
    counter: AtomicU64,
}

impl SequentialIdGen {
    /// Creates a generator counting from one.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl IdGen for SequentialIdGen {
    fn new_id(&self, prefix: &str) -> String {
        let n = self.counter.fetch_add(1, Ordering::SeqCst) + 1;
        format!("{prefix}_{n}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn system_clock_moves_forward() {
        let clock = SystemClock;
        let first = clock.unix_ms();
        std::thread::sleep(Duration::from_millis(2));
        assert!(clock.unix_ms() >= first + 2);
    }

    #[test]
    fn fixed_clock_is_frozen() {
        let clock = FixedClock::new(UNIX_EPOCH + Duration::from_mins(28_750_000));
        assert_eq!(clock.unix_ms(), 1_725_000_000_000);
        assert_eq!(clock.now(), UNIX_EPOCH + Duration::from_mins(28_750_000));
    }

    #[test]
    fn fixed_clock_before_epoch_is_negative() {
        let clock = FixedClock::new(UNIX_EPOCH - Duration::from_millis(5));
        assert_eq!(clock.unix_ms(), -5);
    }

    #[test]
    fn sequential_rng_is_predictable() {
        let rng = SequentialRng::new();
        assert_eq!(rng.next_u128(), 0);
        assert_eq!(rng.next_u128(), 1);
        // The f64 stream shares the same counter, continuing from 2.
        assert!((rng.next_f64() - 0.000_002).abs() < 1e-9);
        assert!((rng.next_f64() - 0.000_003).abs() < 1e-9);
    }

    #[test]
    fn system_rng_bounds() {
        let rng = SystemRng;
        for _ in 0..64 {
            let value = rng.next_f64();
            assert!((0.0..1.0).contains(&value));
        }
    }

    #[test]
    fn uuid_id_shape() {
        let gen = UuidV4IdGen;
        let id = gen.new_id("ses");
        let tail = &id[4..];
        assert_eq!(id.len(), 4 + 32);
        assert!(tail.chars().all(|c| c.is_ascii_hexdigit()), "{id}");
        assert_ne!(gen.new_id("ses"), id);
    }

    #[test]
    fn sequential_id_gen() {
        let gen = SequentialIdGen::new();
        assert_eq!(gen.new_id("msg"), "msg_1");
        assert_eq!(gen.new_id("msg"), "msg_2");
        assert_eq!(gen.new_id("ses"), "ses_3");
    }

    #[test]
    fn ascending_ids_are_ordered_and_shaped() {
        let clock =
            Arc::new(FixedClock::new(UNIX_EPOCH + Duration::from_millis(1_725_000_000_123)));
        let rng = Arc::new(SequentialRng::new());
        let gen = AscendingIdGen::new(clock, rng);
        let first = gen.generate(false);
        let second = gen.generate(false);
        assert_eq!(first.len(), 26);
        assert!(first.chars().take(12).all(|c| c.is_ascii_hexdigit()));
        assert!(first.chars().skip(12).all(|c| c.is_ascii_alphanumeric()));
        // Same millisecond: counter increments, so ordering holds.
        assert!(first < second);
        // Counters 1 and 2 occupy the low 12 bits.
        assert_eq!(&first[9..12], "001");
        assert_eq!(&second[9..12], "002");
    }

    #[test]
    fn ascending_counter_resets_on_new_millisecond() {
        let clock = Arc::new(SteppedClock::default());
        let rng = Arc::new(SequentialRng::new());
        let gen = AscendingIdGen::new(clock, rng);
        let first = gen.generate(false);
        let second = gen.generate(false);
        // Different millisecond: both restart the counter at 1.
        assert_eq!(&first[9..12], "001");
        assert_eq!(&second[9..12], "001");
        assert_ne!(first, second);
    }

    #[test]
    fn descending_ids_invert_order() {
        let clock =
            Arc::new(FixedClock::new(UNIX_EPOCH + Duration::from_millis(1_725_000_000_123)));
        let rng = Arc::new(SequentialRng::new());
        let gen = AscendingIdGen::new(clock, rng);
        let ascending = gen.generate(false);
        let descending = gen.generate(true);
        // ~current flips the time bits; the low hex digits differ wildly.
        assert_ne!(ascending, descending);
        assert_eq!(descending.len(), 26);
    }

    #[test]
    fn ascending_id_gen_prefixes() {
        let clock = Arc::new(FixedClock::epoch());
        let rng = Arc::new(SequentialRng::new());
        let gen = AscendingIdGen::new(clock, rng);
        let id = gen.new_id("msg");
        assert!(id.starts_with("msg_"));
        assert_eq!(id.len(), 4 + 26);
    }

    /// Clock that advances one millisecond per call.
    #[derive(Default)]
    struct SteppedClock {
        ms: AtomicU64,
    }

    impl Clock for SteppedClock {
        fn now(&self) -> SystemTime {
            let ms = self.ms.fetch_add(1, Ordering::SeqCst);
            UNIX_EPOCH + Duration::from_millis(1_725_000_000_000 + ms)
        }
    }
}
