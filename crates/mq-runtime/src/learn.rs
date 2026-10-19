//! Learning from the user's picks (plans/m13-stream-choice.md 3.9): one
//! counter per kind of choice and screen class, synced with the profile.

use mq_streams::StreamPrefs;
use serde::{Deserialize, Serialize};

/// What a pick that differs from the recommendation says the user wants.
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LearnedChoice {
    /// A file ready on the streaming service over a torrent.
    Ready,
    /// More lines.
    Height,
    /// HDR the screen shows.
    Hdr,
    /// A bigger file.
    Size,
}

/// The counters of one screen class, each in -5..5. A counter stamped with
/// an older epoch than the profile's reads as zero.
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Learned {
    pub ready: i8,
    pub height: i8,
    pub hdr: i8,
    pub size: i8,
    /// Picks counted in this class.
    pub picks: u32,
    pub epoch: u32,
    /// The last move, for the decay of 1 per 60 days.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub at_ms: u64,
    /// Choices the user undid: they no longer learn.
    pub muted: Vec<LearnedChoice>,
}

/// Counters by screen class (3.3): 2160, 1080, 720 and 480 lines. Each is
/// its own synced leaf (`learned.uhd`), so two devices learning on
/// different screens both keep their counts.
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LearnedByClass {
    pub uhd: Learned,
    pub fhd: Learned,
    pub hd: Learned,
    pub sd: Learned,
}

/// The one-time note when a counter crosses 3: what changed, for which
/// screen class, and the preferences now in effect there. Undo mutes it.
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LearnedNote {
    pub screen_class: u32,
    pub choice: LearnedChoice,
    /// Toward more (bigger files, more lines, ready files, HDR).
    pub up: bool,
    pub prefs: StreamPrefs,
}

/// Counters move by one per pick and stay in -5..5.
const BOUND: i8 = 5;
/// At this many in one direction the preference follows.
const EFFECT: i8 = 3;
/// A counter loses 1 toward zero for every 60 days without a move.
const DECAY_MS: u64 = 60 * 86_400_000;

impl LearnedChoice {
    fn counter(self, l: &mut Learned) -> &mut i8 {
        match self {
            Self::Ready => &mut l.ready,
            Self::Height => &mut l.height,
            Self::Hdr => &mut l.hdr,
            Self::Size => &mut l.size,
        }
    }
}

/// The synced leaf of a screen class: `uhd` for 2160 lines, `fhd` 1080,
/// `hd` 720, `sd` below.
pub fn class_key(screen_class: u32) -> &'static str {
    match screen_class {
        c if c >= 2160 => "uhd",
        c if c >= 1080 => "fhd",
        c if c >= 720 => "hd",
        _ => "sd",
    }
}

impl LearnedByClass {
    pub fn get(&self, screen_class: u32) -> &Learned {
        match class_key(screen_class) {
            "uhd" => &self.uhd,
            "fhd" => &self.fhd,
            "hd" => &self.hd,
            _ => &self.sd,
        }
    }

    fn all(&self) -> [&Learned; 4] {
        [&self.uhd, &self.fhd, &self.hd, &self.sd]
    }

    /// The reset count in effect: the profile's leaf, or a newer one a
    /// counter was stamped with (r4 P3: the leaf is last-writer, so an
    /// offline device's older reset cannot lower it).
    pub fn epoch(&self, leaf: u32) -> u32 {
        self.all().iter().map(|l| l.epoch).fold(leaf, u32::max)
    }
}

/// A class's counters as they count now: zero when stamped before the
/// last reset, each 1 nearer zero per 60 days since the last move.
pub fn effective(l: &Learned, epoch: u32, now_ms: u64) -> Learned {
    if l.epoch < epoch {
        return Learned {
            epoch,
            ..Learned::default()
        };
    }
    let decay = (now_ms.saturating_sub(l.at_ms) / DECAY_MS).min(BOUND as u64) as i8;
    let toward_zero = |v: i8| v.signum() * (v.abs() - decay).max(0);
    Learned {
        ready: toward_zero(l.ready),
        height: toward_zero(l.height),
        hdr: toward_zero(l.hdr),
        size: toward_zero(l.size),
        ..l.clone()
    }
}

/// The preferences a class's counters move (3.9), at 3 or more either way.
pub fn apply(prefs: &mut StreamPrefs, l: &Learned, screen_class: u32) {
    const STEPS: [u32; 4] = [480, 720, 1080, 2160];
    let cap = prefs.max_height.unwrap_or(screen_class).min(screen_class);
    let at = STEPS
        .iter()
        .position(|h| *h >= cap)
        .unwrap_or(STEPS.len() - 1);
    if l.height >= EFFECT {
        let up = STEPS[(at + 1).min(STEPS.len() - 1)].min(screen_class);
        prefs.max_height = (up < screen_class).then_some(up);
    } else if l.height <= -EFFECT {
        prefs.max_height = Some(STEPS[at.saturating_sub(1)]);
    }
    if l.size >= EFFECT {
        prefs.size_limit_bytes = prefs.size_limit_bytes.map(|b| b.saturating_mul(2));
    } else if l.size <= -EFFECT {
        prefs.size_limit_bytes = Some(prefs.size_limit_bytes.map_or(10 << 30, |b| b / 2));
    }
    if l.ready.abs() >= EFFECT {
        prefs.prefer_ready = l.ready > 0;
    }
    if l.hdr.abs() >= EFFECT {
        prefs.hdr = l.hdr > 0;
    }
}

/// What the user's pick `chose` over the recommendation `over` says, the
/// first difference in this order (3.9): ready, lines, HDR, size. One
/// counter per pick, so 1080p SDR over 4K HDR moves lines only.
pub fn choice(
    chose: &mq_streams::RankedRow,
    over: &mq_streams::RankedRow,
    caps: &mq_streams::DeviceCaps,
) -> Option<(LearnedChoice, i8)> {
    use mq_streams::{ReasonLine, SourceClass};
    let ready = |r: &mq_streams::RankedRow| r.class == SourceClass::Ready;
    if ready(chose) != ready(over) {
        return Some((LearnedChoice::Ready, if ready(chose) { 1 } else { -1 }));
    }
    let lines = |r: &mq_streams::RankedRow| r.stream.release.resolution.height();
    if lines(chose) != lines(over) {
        let more = lines(chose) > lines(over);
        return Some((LearnedChoice::Height, if more { 1 } else { -1 }));
    }
    let hdr = |r: &mq_streams::RankedRow| {
        !caps.display_hdr.is_empty()
            && r.stream
                .release
                .hdr
                .iter()
                .any(|h| *h != mq_relparse::Hdr::Sdr)
            && r.line != ReasonLine::WashedOut
    };
    if hdr(chose) != hdr(over) {
        return Some((LearnedChoice::Hdr, if hdr(chose) { 1 } else { -1 }));
    }
    let (Some(c), Some(o)) = (chose.stream.size_bytes, over.stream.size_bytes) else {
        return None;
    };
    if c as f64 > o as f64 * 1.5 {
        Some((LearnedChoice::Size, 1))
    } else if (c as f64) < o as f64 / 1.5 {
        Some((LearnedChoice::Size, -1))
    } else {
        None
    }
}

/// Counts one pick into `l` (already effective at `epoch`); a muted choice
/// does not learn. The note is for a counter that just reached 3.
pub fn count(
    l: &Learned,
    choice: LearnedChoice,
    delta: i8,
    epoch: u32,
    now_ms: u64,
) -> (Learned, bool) {
    let mut next = Learned { epoch, ..l.clone() };
    if next.muted.contains(&choice) {
        return (next, false);
    }
    let counter = choice.counter(&mut next);
    let before = *counter;
    *counter = (before + delta).clamp(-BOUND, BOUND);
    let crossed = counter.abs() == EFFECT && before.abs() < EFFECT;
    next.picks = next.picks.saturating_add(1);
    next.at_ms = now_ms;
    (next, crossed)
}

/// Undo of a note: the counter goes back to zero and no longer learns.
pub fn mute(l: &Learned, choice: LearnedChoice) -> Learned {
    let mut next = l.clone();
    *choice.counter(&mut next) = 0;
    if !next.muted.contains(&choice) {
        next.muted.push(choice);
    }
    next
}

/// A preference changed by hand zeroes its counter in every class.
pub fn zero(l: &Learned, choice: LearnedChoice) -> Learned {
    let mut next = l.clone();
    *choice.counter(&mut next) = 0;
    next
}

impl crate::Core {
    /// The preferences in effect on a screen of `screen_class`: the
    /// profile's, moved by what its picks there taught (3.9).
    pub(crate) fn learned_prefs(&self, mut prefs: StreamPrefs, screen_class: u32) -> StreamPrefs {
        if !prefs.learn || self.policy().kids() {
            return prefs;
        }
        let all = self.prefs();
        let epoch = all.learned.epoch(all.learned_epoch);
        let l = effective(
            all.learned.get(screen_class),
            epoch,
            mq_library::hlc::now_ms(),
        );
        apply(&mut prefs, &l, screen_class);
        prefs
    }

    /// Counts one pick on a screen of `screen_class`; the note when a
    /// counter just reached 3, with the preferences now in effect there.
    pub(crate) fn learn_pick(
        &self,
        screen_class: u32,
        choice: LearnedChoice,
        delta: i8,
        prefs: StreamPrefs,
    ) -> Option<LearnedNote> {
        let all = self.prefs();
        let epoch = all.learned.epoch(all.learned_epoch);
        let now = mq_library::hlc::now_ms();
        let l = effective(all.learned.get(screen_class), epoch, now);
        let (next, crossed) = count(&l, choice, delta, epoch, now);
        let patch = serde_json::json!({ "learned": { class_key(screen_class): next } });
        if let Err(e) = self.set_prefs(&patch) {
            tracing::warn!("learning write failed: {e}");
            return None;
        }
        crossed.then(|| LearnedNote {
            screen_class,
            choice,
            up: delta > 0,
            prefs: self.learned_prefs(prefs, screen_class),
        })
    }

    /// "Forget what was learned": a new epoch, above every stamp there is,
    /// and zero counters stamped with it, so an offline device's older
    /// counters read as zero when they arrive.
    pub fn forget_learned(&self) -> Result<crate::Prefs, crate::CoreError> {
        let all = self.prefs();
        let epoch = all.learned.epoch(all.learned_epoch) + 1;
        let zero = Learned {
            epoch,
            ..Learned::default()
        };
        self.set_prefs(&serde_json::json!({
            "learnedEpoch": epoch,
            "learned": { "uhd": zero, "fhd": zero, "hd": zero, "sd": zero },
        }))
    }

    /// Undo of a learning note: that counter goes back to zero on that
    /// screen class and no longer learns.
    pub fn undo_learned(
        &self,
        screen_class: u32,
        choice: LearnedChoice,
    ) -> Result<crate::Prefs, crate::CoreError> {
        let all = self.prefs();
        let epoch = all.learned.epoch(all.learned_epoch);
        let l = effective(
            all.learned.get(screen_class),
            epoch,
            mq_library::hlc::now_ms(),
        );
        let patch = serde_json::json!({ "learned": { class_key(screen_class): mute(&l, choice) } });
        self.set_prefs(&patch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counters_count_once_per_pick_clamp_and_decay() {
        let mut l = Learned::default();
        let mut crossed = Vec::new();
        for i in 0..7 {
            let (next, c) = count(&l, LearnedChoice::Size, 1, 0, i);
            l = next;
            crossed.push(c);
        }
        assert_eq!(l.size, 5, "clamped");
        assert_eq!(crossed, [false, false, true, false, false, false, false]);
        assert_eq!(l.picks, 7);
        // 120 days later it counts 3.
        assert_eq!(effective(&l, 0, 6 + 2 * DECAY_MS).size, 3);
        // An older epoch reads as zero.
        assert_eq!(effective(&l, 1, 6).size, 0);
        // A muted choice no longer learns.
        let muted = mute(&l, LearnedChoice::Size);
        assert_eq!(muted.size, 0);
        assert_eq!(count(&muted, LearnedChoice::Size, 1, 0, 9).0.size, 0);
    }

    #[test]
    fn three_in_one_direction_move_the_preference() {
        let base = StreamPrefs::default();
        let mut p = base.clone();
        apply(
            &mut p,
            &Learned {
                size: 3,
                ..Learned::default()
            },
            2160,
        );
        assert_eq!(p.size_limit_bytes, Some(10 << 30));
        let mut p = StreamPrefs {
            size_limit_bytes: None,
            ..base.clone()
        };
        apply(
            &mut p,
            &Learned {
                size: -3,
                ..Learned::default()
            },
            2160,
        );
        assert_eq!(p.size_limit_bytes, Some(10 << 30), "no limit becomes 10 GB");
        let mut p = base.clone();
        apply(
            &mut p,
            &Learned {
                height: -3,
                ..Learned::default()
            },
            2160,
        );
        assert_eq!(p.max_height, Some(1080), "one step below the screen");
        let mut p = StreamPrefs {
            max_height: Some(720),
            ..base.clone()
        };
        apply(
            &mut p,
            &Learned {
                height: 3,
                ..Learned::default()
            },
            2160,
        );
        assert_eq!(
            p.max_height,
            Some(1080),
            "one step up, capped at the screen"
        );
        let mut p = base.clone();
        apply(
            &mut p,
            &Learned {
                ready: -3,
                hdr: -4,
                ..Learned::default()
            },
            1080,
        );
        assert!(!p.prefer_ready && !p.hdr);
        let mut p = base.clone();
        apply(
            &mut p,
            &Learned {
                size: 2,
                ..Learned::default()
            },
            1080,
        );
        assert_eq!(p, base, "under 3 nothing moves");
    }

    #[test]
    fn the_epoch_in_effect_is_the_newest_stamp() {
        let by = LearnedByClass {
            fhd: Learned {
                epoch: 4,
                ..Learned::default()
            },
            ..LearnedByClass::default()
        };
        assert_eq!(by.epoch(2), 4, "an older leaf cannot lower it");
        assert_eq!(by.epoch(6), 6);
    }
}
