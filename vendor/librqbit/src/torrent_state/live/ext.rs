//! Marquee fork: hook surface for an external block scheduler.
//!
//! When [`crate::AddTorrentOptions::ext_sched`] is set, the torrent reports
//! peer and piece events on [`ExtSched::tx`]. With `drive: true` librqbit's
//! own piece picker is off for that torrent: the owner decides every request
//! through [`TorrentStateLive::ext_request`] and [`TorrentStateLive::ext_cancel`],
//! and blocks of one piece may come from many peers. With `drive: false` the
//! events are only observed and the stock picker keeps working.
//!
//! Everything the fork adds lives here, plus one-line calls at the event
//! sites in `torrent_state/live/mod.rs` (see the fork's NOTICE).

use std::net::SocketAddr;

use librqbit_core::lengths::ChunkInfo;
use peer_binary_protocol::{Message, Request};
use tokio::sync::mpsc::UnboundedSender;

use crate::{ManagedTorrent, peer_connection::WriterRequest};

use super::TorrentStateLive;

/// Wiring for an external scheduler.
#[derive(Clone, Debug)]
pub struct ExtSched {
    pub tx: UnboundedSender<ExtEvent>,
    /// Disable the built-in picker; requests come only from the owner.
    pub drive: bool,
}

/// Events in the order librqbit sees them, per peer connection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExtEvent {
    /// The peer's first message arrived: its bitfield (BEP 3 bytes), or an
    /// empty vector when it started with anything else.
    PeerConnected {
        peer: SocketAddr,
        bitfield: Vec<u8>,
    },
    /// `reqq` from the BEP 10 extended handshake (may precede `PeerConnected`).
    PeerReqq {
        peer: SocketAddr,
        reqq: u32,
    },
    PeerHave {
        peer: SocketAddr,
        piece: u32,
    },
    /// In drive mode, every request to this peer is forgotten on choke.
    PeerChoked {
        peer: SocketAddr,
    },
    PeerUnchoked {
        peer: SocketAddr,
    },
    PeerGone {
        peer: SocketAddr,
    },
    /// A payload block arrived (requested, late after a cancel, or duplicate).
    Block {
        peer: SocketAddr,
        piece: u32,
        begin: u32,
    },
    /// A piece was hashed after its last block arrived.
    PieceChecked {
        piece: u32,
        ok: bool,
    },
    /// A piece left the disk (verified or partly written,
    /// plans/streaming-window.md 2.5): every block of it is needed again.
    /// Sent before any later `PieceChecked` of it.
    PieceEvicted {
        piece: u32,
    },
}

impl TorrentStateLive {
    pub(crate) fn ext_emit(&self, ev: ExtEvent) {
        if let Some(ext) = &self.shared.options.ext_sched {
            let _ = ext.tx.send(ev);
        }
    }

    pub(crate) fn ext_drives(&self) -> bool {
        self.shared
            .options
            .ext_sched
            .as_ref()
            .is_some_and(|e| e.drive)
    }

    fn ext_chunk(&self, piece: u32, begin: u32, len: u32) -> Option<ChunkInfo> {
        let piece = self.lengths.validate_piece_index(piece)?;
        self.lengths
            .chunk_info_from_received_data(piece, begin, len)
    }

    /// Send a REQUEST to a live peer. `false` when the peer is not live or
    /// the block is invalid, so the caller can release it.
    pub fn ext_request(&self, peer: SocketAddr, piece: u32, begin: u32, len: u32) -> bool {
        let Some(chunk) = self.ext_chunk(piece, begin, len) else {
            return false;
        };
        self.peers
            .with_live_mut(peer, "ext_request", |live| {
                // Already present after a choke the owner has not seen yet:
                // the remote dropped it, so send it again.
                live.add_inflight_request(chunk);
                live.tx
                    .send(WriterRequest::Message(Message::Request(Request {
                        index: piece,
                        begin,
                        length: len,
                    })))
                    .is_ok()
            })
            .unwrap_or(false)
    }

    /// Send a CANCEL if the request is still outstanding.
    pub fn ext_cancel(&self, peer: SocketAddr, piece: u32, begin: u32, len: u32) {
        if let Some(chunk) = self.ext_chunk(piece, begin, len) {
            self.peers.with_live_mut(peer, "ext_cancel", |live| {
                live.cancel_inflight_request(&chunk)
            });
        }
    }
}

impl ManagedTorrent {
    /// Verified pieces as BEP 3 bitfield bytes, when the torrent is paused or
    /// live (pieces found on disk at start included).
    pub fn ext_have_pieces(&self) -> Option<Vec<u8>> {
        self.with_chunk_tracker(|ct| ct.get_have_pieces().as_bytes().to_vec())
            .ok()
    }
}
