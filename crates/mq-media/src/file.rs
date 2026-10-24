use crate::{ProbeError, RangeReader};
use bytes::Bytes;
use futures_util::future::BoxFuture;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

/// `RangeReader` over a file on this device: a download played or probed
/// with no network (plans/offline-downloads.md 4.12). Its length is what
/// the file holds now, so a download still being written reads as far as
/// it got.
pub struct FileRangeReader {
    path: PathBuf,
}

impl FileRangeReader {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

fn failed(e: std::io::Error) -> ProbeError {
    ProbeError::Http(format!("file: {e}"))
}

impl RangeReader for FileRangeReader {
    fn len(&self) -> BoxFuture<'_, Result<u64, ProbeError>> {
        let path = self.path.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || std::fs::metadata(path).map(|m| m.len()))
                .await
                .map_err(|e| ProbeError::Http(e.to_string()))?
                .map_err(failed)
        })
    }

    fn read_at(&self, offset: u64, len: u64) -> BoxFuture<'_, Result<Bytes, ProbeError>> {
        let path = self.path.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                let mut file = std::fs::File::open(path)?;
                file.seek(SeekFrom::Start(offset))?;
                let mut out = Vec::new();
                file.take(len).read_to_end(&mut out)?;
                Ok(Bytes::from(out))
            })
            .await
            .map_err(|e| ProbeError::Http(e.to_string()))?
            .map_err(failed)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn reads_spans_and_stops_at_the_end() {
        let dir = std::env::temp_dir().join(format!("mq-file-reader-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("v.bin");
        std::fs::write(&path, b"0123456789").unwrap();
        let reader = FileRangeReader::new(&path);
        assert_eq!(reader.len().await.unwrap(), 10);
        assert_eq!(&reader.read_at(2, 3).await.unwrap()[..], b"234");
        assert_eq!(&reader.read_at(8, 10).await.unwrap()[..], b"89");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
