use std::path::PathBuf;

use crate::{
    api::TorrentIdOrHash,
    bitv::{BitV, DiskBackedBitV},
    spawn_utils::BlockingSpawner,
    type_aliases::BF,
};

#[async_trait::async_trait]
pub trait BitVFactory: Send + Sync {
    async fn load(&self, id: TorrentIdOrHash) -> anyhow::Result<Option<Box<dyn BitV>>>;
    async fn clear(&self, id: TorrentIdOrHash) -> anyhow::Result<()>;
    async fn store_initial_check(
        &self,
        id: TorrentIdOrHash,
        b: BF,
    ) -> anyhow::Result<Box<dyn BitV>>;
}

pub struct NonPersistentBitVFactory {}

#[async_trait::async_trait]
impl BitVFactory for NonPersistentBitVFactory {
    async fn load(&self, _: TorrentIdOrHash) -> anyhow::Result<Option<Box<dyn BitV>>> {
        Ok(None)
    }

    async fn clear(&self, _id: TorrentIdOrHash) -> anyhow::Result<()> {
        Ok(())
    }

    async fn store_initial_check(
        &self,
        _id: TorrentIdOrHash,
        b: BF,
    ) -> anyhow::Result<Box<dyn BitV>> {
        Ok(Box::new(b))
    }
}

/// Marquee fork (engine review r3, P2-2): fast resume with no session store.
/// A torrent's have-bitfield is `<folder>/<info hash>.have`, beside its data
/// folder (never in it: a torrent may name a file anything), so a relaunch
/// hashes a sample of the pieces it had instead of every file again.
/// A torrent known by id only keeps nothing.
pub(crate) struct FolderBitVFactory {
    pub(crate) folder: PathBuf,
    pub(crate) spawner: BlockingSpawner,
}

impl FolderBitVFactory {
    fn filename(&self, id: TorrentIdOrHash) -> Option<PathBuf> {
        match id {
            TorrentIdOrHash::Hash(h) => Some(self.folder.join(format!("{}.have", h.as_string()))),
            TorrentIdOrHash::Id(_) => None,
        }
    }
}

#[async_trait::async_trait]
impl BitVFactory for FolderBitVFactory {
    async fn load(&self, id: TorrentIdOrHash) -> anyhow::Result<Option<Box<dyn BitV>>> {
        let Some(filename) = self.filename(id) else {
            return Ok(None);
        };
        // Missing or unreadable: the full check, as with none.
        Ok(DiskBackedBitV::new(filename, self.spawner.clone())
            .await
            .ok()
            .map(|b| b.into_dyn()))
    }

    async fn clear(&self, id: TorrentIdOrHash) -> anyhow::Result<()> {
        let Some(filename) = self.filename(id) else {
            return Ok(());
        };
        match tokio::fs::remove_file(&filename).await {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        }
    }

    async fn store_initial_check(
        &self,
        id: TorrentIdOrHash,
        b: BF,
    ) -> anyhow::Result<Box<dyn BitV>> {
        let Some(filename) = self.filename(id) else {
            return Ok(Box::new(b));
        };
        let tmp = filename.with_extension("have.tmp");
        let stored = async {
            tokio::fs::write(&tmp, b.as_raw_slice()).await?;
            tokio::fs::rename(&tmp, &filename).await?;
            DiskBackedBitV::new(filename.clone(), self.spawner.clone()).await
        };
        match stored.await {
            Ok(bitv) => Ok(bitv.into_dyn()),
            // A full disk costs the next launch a full check, not this play.
            Err(e) => {
                tracing::warn!(?filename, "no fast resume for this torrent: {e:#}");
                Ok(Box::new(b))
            }
        }
    }
}
