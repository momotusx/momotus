use bytes::{Bytes, BytesMut};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, MutexGuard};

/// Cache granularity. Readers fetch whole aligned blocks, so a block shorter
/// than this is always the last block of its source.
pub const BLOCK_SIZE: u64 = 64 * 1024;

/// Bounded LRU of aligned blocks keyed by (source id, offset). Overlapping
/// and unaligned reads are assembled from the blocks they touch.
pub struct ByteCache {
    max_bytes: u64,
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    sources: HashMap<Arc<str>, HashMap<u64, (Bytes, u64)>>,
    lru: BTreeMap<u64, (Arc<str>, u64)>,
    tick: u64,
    bytes: u64,
}

impl ByteCache {
    pub fn new(max_bytes: u64) -> Self {
        Self {
            max_bytes,
            inner: Mutex::default(),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The bytes at `offset..offset + len` when every block is cached. The
    /// result is shorter only when the range runs past a cached end of file.
    pub fn get(&self, source: &str, offset: u64, len: u64) -> Option<Bytes> {
        if len == 0 {
            return Some(Bytes::new());
        }
        let end = offset.checked_add(len)?;
        let mut inner = self.lock();
        let mut parts = Vec::new();
        for block in offset / BLOCK_SIZE..=(end - 1) / BLOCK_SIZE {
            let data = inner.sources.get(source)?.get(&block)?.0.clone();
            let short = (data.len() as u64) < BLOCK_SIZE;
            parts.push((block, data));
            if short {
                break;
            }
        }
        let mut out = BytesMut::new();
        for (block, data) in &parts {
            inner.touch(source, *block);
            let base = block * BLOCK_SIZE;
            let from = offset.saturating_sub(base).min(data.len() as u64) as usize;
            let to = (end - base).min(data.len() as u64) as usize;
            if parts.len() == 1 {
                return Some(data.slice(from..to.max(from)));
            }
            out.extend_from_slice(&data[from..to.max(from)]);
        }
        Some(out.freeze())
    }

    /// The aligned span from the first to the last uncached block of a range,
    /// or `None` when it is fully cached.
    pub fn missing(&self, source: &str, offset: u64, len: u64) -> Option<(u64, u64)> {
        let end = offset.checked_add(len)?;
        if len == 0 {
            return None;
        }
        let inner = self.lock();
        let blocks = inner.sources.get(source);
        let mut span: Option<(u64, u64)> = None;
        for block in offset / BLOCK_SIZE..=(end - 1) / BLOCK_SIZE {
            match blocks.and_then(|b| b.get(&block)) {
                Some((data, _)) if (data.len() as u64) < BLOCK_SIZE => break,
                Some(_) => {}
                None => span = Some((span.map_or(block, |s| s.0), block)),
            }
        }
        span.map(|(first, last)| (first * BLOCK_SIZE, (last + 1) * BLOCK_SIZE))
    }

    /// Stores `data` read at a block-aligned `offset`. A trailing partial
    /// block is kept and marks the end of the source.
    pub fn insert(&self, source: &str, offset: u64, data: &Bytes) {
        debug_assert_eq!(offset % BLOCK_SIZE, 0);
        let mut inner = self.lock();
        let key: Arc<str> = inner
            .sources
            .get_key_value(source)
            .map_or_else(|| source.into(), |(k, _)| k.clone());
        for (i, start) in (0..data.len()).step_by(BLOCK_SIZE as usize).enumerate() {
            let chunk = data.slice(start..(start + BLOCK_SIZE as usize).min(data.len()));
            inner.put(key.clone(), offset / BLOCK_SIZE + i as u64, chunk);
        }
        while inner.bytes > self.max_bytes {
            let Some((_, (src, block))) = inner.lru.pop_first() else {
                break;
            };
            inner.remove(&src, block);
        }
    }
}

impl Inner {
    fn touch(&mut self, source: &str, block: u64) -> Option<()> {
        self.tick += 1;
        let key = self.sources.get_key_value(source)?.0.clone();
        let entry = self.sources.get_mut(source)?.get_mut(&block)?;
        self.lru.remove(&entry.1);
        entry.1 = self.tick;
        self.lru.insert(self.tick, (key, block));
        Some(())
    }

    fn put(&mut self, source: Arc<str>, block: u64, data: Bytes) {
        self.remove(&source, block);
        self.tick += 1;
        self.bytes += data.len() as u64;
        self.lru.insert(self.tick, (source.clone(), block));
        self.sources
            .entry(source)
            .or_default()
            .insert(block, (data, self.tick));
    }

    fn remove(&mut self, source: &str, block: u64) {
        let Some(entries) = self.sources.get_mut(source) else {
            return;
        };
        if let Some((data, tick)) = entries.remove(&block) {
            self.bytes -= data.len() as u64;
            self.lru.remove(&tick);
        }
        if entries.is_empty() {
            self.sources.remove(source);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assembles_overlapping_reads_and_evicts() {
        let cache = ByteCache::new(3 * BLOCK_SIZE);
        let data: Bytes = (0..(2 * BLOCK_SIZE + 10)).map(|i| i as u8).collect();
        cache.insert("a", 0, &data);
        let mid = cache.get("a", BLOCK_SIZE - 5, 10).unwrap();
        assert_eq!(
            &mid[..],
            &data[(BLOCK_SIZE - 5) as usize..(BLOCK_SIZE + 5) as usize]
        );
        // Past the short final block is end of file, not a miss.
        assert_eq!(cache.get("a", 2 * BLOCK_SIZE, 100).unwrap().len(), 10);
        assert_eq!(cache.missing("a", 0, 5 * BLOCK_SIZE), None);
        assert_eq!(cache.missing("b", 10, 1), Some((0, BLOCK_SIZE)));

        // Touch block 0 so block 1 is the oldest, then overflow the budget.
        cache.get("a", 0, 1).unwrap();
        cache.insert("b", 0, &data.slice(..BLOCK_SIZE as usize));
        assert!(cache.get("a", 0, 1).is_some());
        assert!(cache.get("a", BLOCK_SIZE, 1).is_none());
        assert_eq!(
            cache.missing("a", 0, 3 * BLOCK_SIZE),
            Some((BLOCK_SIZE, 2 * BLOCK_SIZE))
        );
    }
}
