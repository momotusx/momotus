//! Hybrid logical clock (Kulkarni et al. 2014). Timestamps stay close to wall
//! time, increase monotonically per device, and encode to fixed-width hex so
//! string comparison equals causal order. SQLite compares them as text.

use std::fmt;
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Hlc {
    pub wall_ms: u64,
    pub counter: u16,
    pub device: u64,
}

impl Hlc {
    pub fn encode(&self) -> String {
        format!(
            "{:012x}{:04x}{:016x}",
            self.wall_ms, self.counter, self.device
        )
    }

    /// The lowest timestamp of a device: for writes that only cache what the
    /// device saw (a title it played, its first profile row), so any real
    /// edit from any device wins over them.
    pub fn zero(device: u64) -> Self {
        Self {
            wall_ms: 0,
            counter: 0,
            device,
        }
    }

    pub fn decode(s: &str) -> Option<Self> {
        // Lowercase only: hlcs are compared as text, and "A" sorts before "a".
        if s.len() != 32 || !s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
            return None;
        }
        Some(Self {
            wall_ms: u64::from_str_radix(&s[..12], 16).ok()?,
            counter: u16::from_str_radix(&s[12..16], 16).ok()?,
            device: u64::from_str_radix(&s[16..], 16).ok()?,
        })
    }
}

impl fmt::Display for Hlc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.encode())
    }
}

/// Highest wall time a clock takes (year 6429). Far beyond any real clock,
/// and low enough that bumping it never needs a 13th hex digit, so a relay
/// that reports an absurd time cannot break text ordering.
pub const MAX_WALL_MS: u64 = 0x7fff_ffff_ffff;

/// Clock for one device. Wall time is injected so tests are deterministic.
pub struct HlcClock {
    device: u64,
    last: Mutex<Hlc>,
    /// Added to the system time; tests use it to give devices skewed clocks.
    offset_ms: std::sync::atomic::AtomicI64,
}

impl HlcClock {
    pub fn new(device: u64) -> Self {
        Self {
            device,
            last: Mutex::new(Hlc {
                wall_ms: 0,
                counter: 0,
                device,
            }),
            offset_ms: Default::default(),
        }
    }

    /// This device's wall time: the system time, plus the test offset.
    pub fn wall_now(&self) -> u64 {
        let offset = self.offset_ms.load(std::sync::atomic::Ordering::Relaxed);
        (now_ms() as i64 + offset).max(0) as u64
    }

    /// Skews this clock's wall time (tests).
    pub fn set_offset_ms(&self, offset: i64) {
        self.offset_ms
            .store(offset, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn device(&self) -> u64 {
        self.device
    }

    /// Timestamp for a local event.
    pub fn tick(&self, now_ms: u64) -> Hlc {
        let now_ms = now_ms.min(MAX_WALL_MS);
        let mut last = self.last.lock().unwrap_or_else(|p| p.into_inner());
        let next = if now_ms > last.wall_ms {
            Hlc {
                wall_ms: now_ms,
                counter: 0,
                device: self.device,
            }
        } else {
            bump(last.wall_ms, last.counter, self.device)
        };
        *last = next;
        next
    }

    /// Merges a timestamp received from another device or the server.
    pub fn observe(&self, remote: Hlc, now_ms: u64) -> Hlc {
        let mut last = self.last.lock().unwrap_or_else(|p| p.into_inner());
        let remote_wall = remote.wall_ms.min(MAX_WALL_MS);
        let wall = now_ms.min(MAX_WALL_MS).max(last.wall_ms).max(remote_wall);
        let next = match (wall == last.wall_ms, wall == remote_wall) {
            (true, true) => bump(wall, last.counter.max(remote.counter), self.device),
            (true, false) => bump(wall, last.counter, self.device),
            (false, true) => bump(wall, remote.counter, self.device),
            (false, false) => Hlc {
                wall_ms: wall,
                counter: 0,
                device: self.device,
            },
        };
        *last = next;
        next
    }
}

impl Hlc {
    /// The first timestamp of `device` that sorts after this one.
    pub fn next_for(&self, device: u64) -> Hlc {
        bump(self.wall_ms, self.counter, device)
    }
}

/// The next timestamp after `(wall_ms, counter)`. When the counter is used up
/// the wall time moves one millisecond instead, so a clock never repeats a
/// timestamp (a repeat would lose last-writer-wins ties).
fn bump(wall_ms: u64, counter: u16, device: u64) -> Hlc {
    match counter.checked_add(1) {
        Some(counter) => Hlc {
            wall_ms,
            counter,
            device,
        },
        None => Hlc {
            wall_ms: wall_ms + 1,
            counter: 0,
            device,
        },
    }
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoding_sorts_like_the_clock() {
        let a = Hlc {
            wall_ms: 5,
            counter: 9,
            device: 2,
        };
        let b = Hlc {
            wall_ms: 6,
            counter: 0,
            device: 1,
        };
        assert!(a < b);
        assert!(a.encode() < b.encode());
        assert_eq!(Hlc::decode(&a.encode()), Some(a));
        assert_eq!(Hlc::decode("zz"), None);
    }

    #[test]
    fn monotonic_under_clock_skew() {
        let clock = HlcClock::new(1);
        let t1 = clock.tick(1000);
        let t2 = clock.tick(900); // wall clock went backwards
        assert!(t2 > t1);
        let remote = Hlc {
            wall_ms: 5000,
            counter: 3,
            device: 7,
        };
        let t3 = clock.observe(remote, 1001);
        assert!(t3 > remote && t3 > t2);
        assert_eq!(t3.wall_ms, 5000);
        assert_eq!(t3.counter, 4);
        let t4 = clock.tick(1002);
        assert!(t4 > t3, "local ticks stay ahead of what we observed");
    }

    #[test]
    fn a_used_up_counter_moves_the_wall_time() {
        let clock = HlcClock::new(1);
        let far = Hlc {
            wall_ms: 10,
            counter: u16::MAX,
            device: 2,
        };
        let t1 = clock.observe(far, 5);
        assert!(t1 > far);
        let t2 = clock.tick(5);
        assert!(t2 > t1, "{t1:?} then {t2:?}");
        assert_eq!(Hlc::decode(&t2.encode().to_uppercase()), None);
    }
}
