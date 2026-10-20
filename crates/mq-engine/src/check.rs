//! A reader's check of the bytes it got against the torrent's piece hashes
//! (`mq play-magnet --walk`). Only pieces wholly inside the file are
//! checked: a piece across a file edge also holds a neighbour's bytes.

use sha1w::ISha1;

pub struct PieceCheck {
    piece_len: u64,
    /// The file's span in the torrent, and the torrent's length.
    file_start: u64,
    file_end: u64,
    total: u64,
    hashes: Vec<[u8; 20]>,
    /// The next torrent offset fed, and the hash of its piece while that
    /// piece was fed from its first byte.
    pos: u64,
    piece: Option<sha1w::Sha1>,
    /// Pieces hashed, and those whose hash was not the torrent's.
    pub checked: u32,
    pub wrong: u32,
}

impl PieceCheck {
    pub(crate) fn new(
        piece_len: u32,
        file_start: u64,
        file_len: u64,
        total: u64,
        hashes: Vec<[u8; 20]>,
    ) -> Self {
        let mut check = Self {
            piece_len: u64::from(piece_len.max(1)),
            file_start,
            file_end: file_start + file_len,
            total,
            hashes,
            pos: file_start,
            piece: None,
            checked: 0,
            wrong: 0,
        };
        check.seek(0);
        check
    }

    /// The next bytes fed start at `offset` in the file.
    pub fn seek(&mut self, offset: u64) {
        self.pos = self.file_start + offset;
        self.piece = self.start_piece();
    }

    fn piece_end(&self, start: u64) -> u64 {
        (start + self.piece_len).min(self.total)
    }

    /// A hash starts only at a piece's first byte. A piece that ends past
    /// the file never completes, since feeding stops at the file's end.
    fn start_piece(&self) -> Option<sha1w::Sha1> {
        self.pos
            .is_multiple_of(self.piece_len)
            .then(sha1w::Sha1::new)
    }

    /// File bytes, in order from the last `seek`.
    pub fn feed(&mut self, mut bytes: &[u8]) {
        while !bytes.is_empty() && self.pos < self.file_end {
            let start = self.pos / self.piece_len * self.piece_len;
            let end = self.piece_end(start);
            let n = (end - self.pos).min(bytes.len() as u64) as usize;
            if let Some(h) = &mut self.piece {
                h.update(&bytes[..n]);
            }
            self.pos += n as u64;
            bytes = &bytes[n..];
            if self.pos == end {
                if let Some(h) = self.piece.take() {
                    self.checked += 1;
                    let piece = (start / self.piece_len) as usize;
                    if self.hashes.get(piece) != Some(&h.finish()) {
                        self.wrong += 1;
                    }
                }
                self.piece = self.start_piece();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sha1(bytes: &[u8]) -> [u8; 20] {
        let mut h = sha1w::Sha1::new();
        h.update(bytes);
        h.finish()
    }

    /// Three files of 20, 50 and 31 bytes in pieces of 16: the middle file
    /// holds pieces 2 and 3 whole (bytes 32..64) and parts of 1 and 4; the
    /// last file ends the torrent in the short piece 6 (96..101).
    fn torrent() -> (Vec<u8>, Vec<[u8; 20]>) {
        let data: Vec<u8> = (0..101u32).map(|i| (i * 7 + 3) as u8).collect();
        let hashes = data.chunks(16).map(sha1).collect();
        (data, hashes)
    }

    #[test]
    fn checks_only_whole_pieces_inside_the_file_in_any_chunking() {
        let (data, hashes) = torrent();
        for step in [1, 5, 16, 50] {
            let mut c = PieceCheck::new(16, 20, 50, 101, hashes.clone());
            data[20..70].chunks(step).for_each(|b| c.feed(b));
            assert_eq!((c.checked, c.wrong), (2, 0), "step {step}");
        }
        // The last file: piece 5 (80..96) and the short piece 6.
        let mut c = PieceCheck::new(16, 70, 31, 101, hashes.clone());
        c.feed(&data[70..]);
        assert_eq!((c.checked, c.wrong), (2, 0));
    }

    #[test]
    fn a_wrong_byte_and_a_mid_piece_seek() {
        let (mut data, hashes) = torrent();
        data[40] ^= 1;
        let mut c = PieceCheck::new(16, 20, 50, 101, hashes.clone());
        c.feed(&data[20..70]);
        assert_eq!((c.checked, c.wrong), (2, 1));
        // From byte 38 of the torrent (18 in the file): piece 2 is skipped.
        let mut c = PieceCheck::new(16, 20, 50, 101, hashes);
        c.seek(18);
        c.feed(&data[38..70]);
        assert_eq!((c.checked, c.wrong), (1, 0));
    }
}
