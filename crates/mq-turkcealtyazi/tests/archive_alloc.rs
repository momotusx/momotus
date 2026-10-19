//! Memory under hostile archives, counted by this binary's allocator. One
//! test only, so no other test allocates while it measures.

mod common;

use common::{Options, Rng, corpus, zip_with};
use mq_turkcealtyazi::{MAX_ENTRY, Playing, Want, pick};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Counting;

static NOW: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let now = NOW.fetch_add(layout.size(), Ordering::SeqCst) + layout.size();
        PEAK.fetch_max(now, Ordering::SeqCst);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        NOW.fetch_sub(layout.size(), Ordering::SeqCst);
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// The most memory `f` held at once beyond what was held before it.
fn peak<R>(f: impl FnOnce() -> R) -> (R, usize) {
    let before = NOW.load(Ordering::SeqCst);
    PEAK.store(before, Ordering::SeqCst);
    let out = f();
    (out, PEAK.load(Ordering::SeqCst) - before)
}

/// What one pick may hold: the capped file as `read_to_end` grows it,
/// counted at its last growth when the old buffer (up to the cap) and the
/// new one (up to twice the cap) are both held, and a little bookkeeping.
/// Inflating the bomb uncapped would hold 64 MB.
const BOUND: usize = 3 * MAX_ENTRY + 1024 * 1024;

#[test]
fn bombs_and_random_bytes_stay_within_the_cap() {
    // 64 MB of zeros deflate to about 64 KB: a bomb the read cap stops.
    let zeros = vec![0u8; 64 * 1024 * 1024];
    let bomb = zip_with(&[(b"bomb.srt", &zeros)], Options::default());
    drop(zeros);
    assert!(bomb.len() < 200 * 1024, "{}", bomb.len());
    let (result, held) = peak(|| pick(&bomb, Want::Single, &Playing::default()));
    assert!(result.is_err());
    assert!(held < BOUND, "the bomb held {held} bytes");

    // Random bytes, half of them dressed as a ZIP: always an error.
    let mut rng = Rng(0x5eed_7a17_2026_1019);
    for case in 0..3000 {
        let len = rng.below(4096);
        let mut bytes: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
        if case % 2 == 0 && bytes.len() >= 4 {
            bytes[..4].copy_from_slice(b"PK\x03\x04");
        }
        if case % 4 == 0 {
            let at = bytes.len().saturating_sub(22);
            bytes.truncate(at);
            bytes.extend(b"PK\x05\x06");
            bytes.extend((0..18).map(|_| rng.next() as u8));
        }
        let (result, held) = peak(|| pick(&bytes, Want::Single, &Playing::default()));
        assert!(result.is_err(), "case {case} read as a subtitle");
        assert!(held < BOUND, "case {case} held {held} bytes");
    }

    // The real archives with a few bytes changed: any answer but a panic,
    // within the bound.
    for name in ["episode.zip", "pack.zip", "multi.zip", "movie.zip"] {
        let real = corpus(name);
        for case in 0..1000 {
            let mut bent = real.clone();
            for _ in 0..1 + rng.below(8) {
                let at = rng.below(bent.len());
                bent[at] = rng.next() as u8;
            }
            let want = if case % 2 == 0 {
                Want::Single
            } else {
                Want::FromSeveral {
                    season: 1,
                    episode: 1,
                }
            };
            let (_, held) = peak(|| pick(&bent, want, &Playing::default()));
            assert!(held < BOUND, "{name} case {case} held {held} bytes");
        }
    }
}
