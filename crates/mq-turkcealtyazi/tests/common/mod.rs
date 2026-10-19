//! Archives built for the tests: a small ZIP writer, as the site's archives
//! are made (local headers, then the central directory and its end
//! record), with the variations the reader must survive.

#![allow(dead_code)]

use flate2::Compression;
use flate2::write::DeflateEncoder;
use std::io::Write;

#[derive(Clone, Copy, Default)]
pub struct Options {
    /// Stored instead of deflated.
    pub stored: bool,
    /// Sizes and CRC after the data (flag bit 3), zeros in the local header.
    pub descriptor: bool,
}

pub fn zip_with(files: &[(&[u8], &[u8])], options: Options) -> Vec<u8> {
    let (mut out, mut central) = (Vec::new(), Vec::new());
    let flags: u16 = if options.descriptor { 8 } else { 0 };
    let method: u16 = if options.stored { 0 } else { 8 };
    for (name, data) in files {
        let mut crc = flate2::Crc::new();
        crc.update(data);
        let body = if options.stored {
            data.to_vec()
        } else {
            let mut z = DeflateEncoder::new(Vec::new(), Compression::default());
            z.write_all(data).unwrap();
            z.finish().unwrap()
        };
        let sums = [crc.sum(), body.len() as u32, data.len() as u32];
        let offset = out.len() as u32;
        out.extend(b"PK\x03\x04\x14\x00");
        out.extend(flags.to_le_bytes());
        out.extend(method.to_le_bytes());
        out.extend([0u8; 4]);
        for value in if options.descriptor { [0; 3] } else { sums } {
            out.extend(value.to_le_bytes());
        }
        out.extend((name.len() as u16).to_le_bytes());
        out.extend(0u16.to_le_bytes());
        out.extend(*name);
        out.extend(&body);
        if options.descriptor {
            out.extend(b"PK\x07\x08");
            for value in sums {
                out.extend(value.to_le_bytes());
            }
        }
        central.extend(b"PK\x01\x02\x14\x00\x14\x00");
        central.extend(flags.to_le_bytes());
        central.extend(method.to_le_bytes());
        central.extend([0u8; 4]);
        for value in sums {
            central.extend(value.to_le_bytes());
        }
        central.extend((name.len() as u16).to_le_bytes());
        central.extend([0u8; 12]);
        central.extend(offset.to_le_bytes());
        central.extend(*name);
    }
    let start = out.len() as u32;
    out.extend(&central);
    out.extend(b"PK\x05\x06\x00\x00\x00\x00");
    out.extend((files.len() as u16).to_le_bytes());
    out.extend((files.len() as u16).to_le_bytes());
    out.extend((central.len() as u32).to_le_bytes());
    out.extend(start.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    out
}

pub fn zip(files: &[(&str, &[u8])]) -> Vec<u8> {
    let files: Vec<(&[u8], &[u8])> = files.iter().map(|(n, d)| (n.as_bytes(), *d)).collect();
    zip_with(&files, Options::default())
}

pub fn corpus(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/corpus/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(path).unwrap()
}

/// The offset of the first `signature` in `bytes`.
pub fn find(bytes: &[u8], signature: &[u8; 4]) -> usize {
    bytes.windows(4).position(|w| w == signature).unwrap()
}

/// A seeded xorshift64*: the same cases on every run.
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}
