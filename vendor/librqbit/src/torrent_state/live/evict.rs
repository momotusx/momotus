//! Marquee fork (plans/streaming-window.md 2.5): pieces leave the disk, so
//! a streaming window gives space back while it plays.
//!
//! The lock rule: a piece's write lock is only ever tried, under the
//! torrent write lock, and let go before it; nothing blocks on it. Readers
//! (`FileStream`) and block writers take a piece's read lock under the
//! torrent read lock. So a round skips a piece someone is in, and anyone
//! after the round's first step sees the piece gone.
//!
//! A round, for verified pieces:
//! 1. Gate, in memory, under the torrent write lock: `have` cleared, chunk
//!    bits kept set (a late block is then dropped, `write_to_disk`).
//! 2. The bitfield without them written and fsynced (the flusher answers).
//! 3. Their whole blocks punched, with no lock: merged where gated pieces
//!    touch, rounded inward, padding files skipped.
//! 4. Release, under the torrent write lock: chunk bits cleared, queued
//!    again, `ExtEvent::PieceEvicted` sent inside the lock.
//!
//! A partly written piece (an orphan) gets every chunk bit in step 1 and
//! skips step 2: it was never claimed.
//!
//! Every exit after step 1 leaves a piece in one of two states. No punch
//! was tried on its bytes (the flusher failed, or no whole block lies in
//! them): a verified piece is claimed again and its waiting streams are
//! woken. Otherwise, a failed punch included (it may have freed part of its
//! range), and for every orphan: bits cleared, queued, `PieceEvicted`. A
//! round that stops early (its future dropped, a panic, a failed release)
//! takes the second exit for every gated piece, through a guard.
//!
//! Preconditions: upload off (the fork then sends no BITFIELD or HAVE, and
//! serves no piece a round could punch under a peer's read) and drive mode
//! (librqbit's own picker could reserve a gated piece).

use std::ops::Range;
use std::sync::atomic::Ordering;

use librqbit_core::lengths::ValidPieceIndex;

use super::TorrentStateLive;
use crate::{ExtEvent, ManagedTorrent};

/// What an eviction round did with each piece it was asked for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Evicted {
    /// Left the disk: fetched again when read. An orphan is here even when
    /// none of its bytes could be freed.
    pub evicted: Vec<u32>,
    /// Not touched: someone was reading or writing it, or it was neither
    /// verified nor partly written, or its hash may be running.
    pub skipped: Vec<u32>,
    /// Verified pieces gated and claimed again whole, with no byte freed: a
    /// flusher failure, a refused punch, or no whole block inside them.
    pub kept: Vec<u32>,
    pub punched_bytes: u64,
    /// The failure that kept pieces, if any.
    pub error: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Gate {
    Verified,
    Orphan,
}

impl TorrentStateLive {
    /// One eviction round over `pieces` (2.5). The caller keeps rounds
    /// and pauses apart.
    pub async fn evict_pieces(&self, pieces: &[u32]) -> anyhow::Result<Evicted> {
        anyhow::ensure!(
            self.shared.options.disable_upload() && self.ext_drives(),
            "eviction needs upload off and drive mode"
        );
        self.evicts.store(true, Ordering::Relaxed);
        let mut out = Evicted::default();
        let (gated, ack) = {
            let mut g = self.lock_write("evict_gate");
            let ct = g.get_pieces_mut()?.chunks_mut();
            let mut gated = Vec::new();
            for &p in pieces {
                let Some(idx) = self.lengths.validate_piece_index(p) else {
                    out.skipped.push(p);
                    continue;
                };
                let Some(_piece) = self.piece_lock(idx).try_write() else {
                    out.skipped.push(p);
                    continue;
                };
                if ct.is_piece_have(idx) {
                    let len = ct.gate_verified(idx, &self.metadata.file_infos);
                    self.stats.have_bytes.fetch_sub(len, Ordering::Relaxed);
                    gated.push((idx, Gate::Verified));
                } else if ct.gate_orphan(idx) {
                    gated.push((idx, Gate::Orphan));
                } else {
                    out.skipped.push(p);
                }
            }
            let ack = gated
                .iter()
                .any(|(_, k)| *k == Gate::Verified)
                .then(|| ct.get_have_pieces_mut().flush_acked());
            (gated, ack)
        };
        if gated.is_empty() {
            return Ok(out);
        }
        let mut guard = Abandoned {
            live: self,
            pieces: gated.iter().map(|g| g.0).collect(),
        };
        // The torrent needs these pieces again: a finished one dropped its
        // seeders as not needed.
        self.reconnect_all_not_needed_peers();
        self.hook(Point::Gated);
        let acked = match ack {
            None => Ok(()),
            Some(rx) => rx
                .await
                .unwrap_or_else(|_| Err("the bitfield's flusher is gone".to_owned())),
        };
        self.hook(Point::Acked);
        let freed = match acked {
            Ok(()) => self
                .shared
                .spawner
                .block_in_place(|| self.punch_gated(&gated, &mut out)),
            Err(e) => {
                out.error = Some(e);
                vec![false; gated.len()]
            }
        };
        self.hook(Point::Punched);
        {
            let mut g = self.lock_write("evict_release");
            let ct = g.get_pieces_mut()?.chunks_mut();
            for (&(idx, kind), &freed) in gated.iter().zip(&freed) {
                if kind == Gate::Verified && !freed {
                    let len = ct.restore_verified(idx, &self.metadata.file_infos);
                    self.stats.have_bytes.fetch_add(len, Ordering::Relaxed);
                    self.streams
                        .wake_streams_on_piece_completed(idx, &self.lengths);
                    out.kept.push(idx.get());
                } else {
                    ct.release_evicted(idx);
                    self.ext_emit(ExtEvent::PieceEvicted { piece: idx.get() });
                    out.evicted.push(idx.get());
                }
            }
            guard.pieces.clear();
        }
        self.hook(Point::Released);
        Ok(out)
    }

    /// Step 3: the gated pieces' bytes per file, merged where they touch,
    /// each range rounded inward by the storage, padding files skipped.
    /// Per gated piece, whether any byte of it was freed.
    fn punch_gated(&self, gated: &[(ValidPieceIndex, Gate)], out: &mut Evicted) -> Vec<bool> {
        let piece_len = u64::from(self.lengths.default_piece_length());
        let spans: Vec<Vec<(usize, u64, u64)>> = gated
            .iter()
            .map(|(idx, _)| {
                let start = u64::from(idx.get()) * piece_len;
                let end = start + u64::from(self.lengths.piece_length(*idx));
                let files = self.metadata.file_infos.iter().enumerate();
                files
                    .filter(|(_, fi)| !fi.attrs.padding)
                    .filter_map(|(i, fi)| {
                        let (a, b) = (
                            start.max(fi.offset_in_torrent),
                            end.min(fi.offset_in_torrent + fi.len),
                        );
                        (a < b).then(|| (i, a - fi.offset_in_torrent, b - fi.offset_in_torrent))
                    })
                    .collect()
            })
            .collect();
        let mut all: Vec<(usize, u64, u64)> = spans.iter().flatten().copied().collect();
        all.sort_unstable();
        let mut merged: Vec<(usize, u64, u64)> = Vec::new();
        for s in all {
            match merged.last_mut() {
                Some(m) if m.0 == s.0 && m.2 >= s.1 => m.2 = m.2.max(s.2),
                _ => merged.push(s),
            }
        }
        let mut freed: Vec<(usize, Range<u64>)> = Vec::new();
        for (file, a, b) in merged {
            match self.files.punch(file, a, b - a) {
                Ok(r) if r.start < r.end => {
                    out.punched_bytes += r.end - r.start;
                    freed.push((file, r));
                }
                Ok(_) => {}
                // A failed call may have freed part of its range: its
                // pieces leave as if punched, never claimed again.
                Err(e) => {
                    out.error.get_or_insert_with(|| format!("punch: {e:#}"));
                    freed.push((file, a..b));
                }
            }
        }
        spans
            .iter()
            .map(|s| {
                s.iter().any(|&(f, a, b)| {
                    freed
                        .iter()
                        .any(|(ff, r)| *ff == f && r.start < b && a < r.end)
                })
            })
            .collect()
    }

    #[cfg(feature = "sync-log")]
    fn hook(&self, point: Point) {
        let dir = &self.shared.options.output_folder;
        self.shared
            .spawner
            .block_in_place(|| crate::sync_log::pass(dir, point));
    }

    #[cfg(not(feature = "sync-log"))]
    fn hook(&self, _point: Point) {}
}

/// The second exit for every piece still listed when dropped: a round that
/// stops before its release (future dropped, panic, failed release). It
/// never claims a piece again, since a punch may have touched it.
struct Abandoned<'a> {
    live: &'a TorrentStateLive,
    pieces: Vec<ValidPieceIndex>,
}

impl Drop for Abandoned<'_> {
    fn drop(&mut self) {
        if self.pieces.is_empty() {
            return;
        }
        let mut g = self.live.lock_write("evict_abandoned");
        let Ok(pt) = g.get_pieces_mut() else {
            // Paused or failed: the pause released them.
            return;
        };
        let ct = pt.chunks_mut();
        for &idx in &self.pieces {
            ct.release_evicted(idx);
            self.live
                .ext_emit(ExtEvent::PieceEvicted { piece: idx.get() });
        }
    }
}

#[cfg(feature = "sync-log")]
use crate::sync_log::Point;

/// The round's steps, for the sync-log feature's holds.
#[cfg(not(feature = "sync-log"))]
#[derive(Clone, Copy)]
enum Point {
    Gated,
    Acked,
    Punched,
    Released,
}

impl ManagedTorrent {
    /// Marquee fork: [`TorrentStateLive::evict_pieces`] on a live torrent;
    /// on any other, every piece is skipped.
    pub async fn evict_pieces(&self, pieces: &[u32]) -> anyhow::Result<Evicted> {
        match self.live() {
            Some(live) => live.evict_pieces(pieces).await,
            None => Ok(Evicted {
                skipped: pieces.to_vec(),
                ..Default::default()
            }),
        }
    }
}
