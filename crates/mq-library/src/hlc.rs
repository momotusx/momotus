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

    pub fn decode(s: &str) -> Option<Self> {
        if s.len() != 32 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
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

/// Clock for one device. Wall time is injected so tests are deterministic.
pub struct HlcClock {
    device: u64,
    last: Mutex<Hlc>,
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
        }
    }

    /// Timestamp for a local event.
    pub fn tick(&self, now_ms: u64) -> Hlc {
        let mut last = self.last.lock().unwrap_or_else(|p| p.into_inner());
        let next = if now_ms > last.wall_ms {
            Hlc {
                wall_ms: now_ms,
                counter: 0,
                device: self.device,
            }
        } else {
            Hlc {
                wall_ms: last.wall_ms,
                counter: last.counter.saturating_add(1),
                device: self.device,
            }
        };
        *last = next;
        next
    }

    /// Merges a timestamp received from another device or the server.
    pub fn observe(&self, remote: Hlc, now_ms: u64) -> Hlc {
        let mut last = self.last.lock().unwrap_or_else(|p| p.into_inner());
        let wall = now_ms.max(last.wall_ms).max(remote.wall_ms);
        let counter = match (wall == last.wall_ms, wall == remote.wall_ms) {
            (true, true) => last.counter.max(remote.counter).saturating_add(1),
            (true, false) => last.counter.saturating_add(1),
            (false, true) => remote.counter.saturating_add(1),
            (false, false) => 0,
        };
        let next = Hlc {
            wall_ms: wall,
            counter,
            device: self.device,
        };
        *last = next;
        next
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
}
