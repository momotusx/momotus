//! What the engine keeps on the device, behind two traits so an app other
//! than Momotus can sync its own records: `Registers` are the app's records
//! (what syncs), `Bookkeeping` is the engine's own state (cursors, pins,
//! approvals and the verified roster). Momotus's `Library` is both.

use mq_library::{Library, LibraryError, OutboxPage, SignedEntry, SyncRecord};

/// The app's sync records: last-writer-wins registers keyed by
/// `(coll, key)`, stamped with an HLC. Coll 1 is not free: every relay
/// refuses it (Momotus's old whole addon lists went there).
pub trait Registers: Send + Sync {
    /// Up to `limit` queued registers with their current values, oldest
    /// queue entry first; a register queued several times is read once.
    fn outbox(&self, limit: usize) -> Result<OutboxPage, LibraryError>;
    /// Drops queue entries up to `id` once the relay has them.
    fn outbox_done(&self, id: i64) -> Result<(), LibraryError>;
    /// Applies pulled registers; each replaces local state only when its HLC
    /// is newer, and none is queued again. Returns how many changed. A
    /// record that cannot be read is skipped; any other failure fails the
    /// call, so the page is pulled again.
    fn apply(&self, records: &[SyncRecord]) -> Result<usize, LibraryError>;
    /// Queues a register again (one the relay could not take yet).
    fn requeue(&self, coll: u8, key: &str) -> Result<(), LibraryError>;
    /// Queues every register: on first contact, and when the relay lost records.
    fn queue_all(&self) -> Result<(), LibraryError>;
    /// Gives a register a new hlc and queues it, even when that is lower.
    fn restamp(&self, coll: u8, key: &str, hlc: &str) -> Result<(), LibraryError>;
    /// The device left the account: the queue and anything the app keeps
    /// for it go, and a pending join ends (`join_defaults_pending` turns
    /// false); the records stay.
    fn forget(&self) -> Result<(), LibraryError>;
    /// Whether a join waits for `settle_join_defaults`: the engine then
    /// pulls before its first push.
    fn join_defaults_pending(&self) -> Result<bool, LibraryError>;
    /// After that first pull: the app settles what this device brought.
    fn settle_join_defaults(&self, device: u64) -> Result<(), LibraryError>;
}

/// The engine's state on the device.
pub trait Bookkeeping: Send + Sync {
    fn sync_state(&self, key: &str) -> Result<Option<String>, LibraryError>;
    fn set_sync_state(&self, key: &str, value: &str) -> Result<(), LibraryError>;
    /// Keys starting with `prefix`, with their values.
    fn sync_states(&self, prefix: &str) -> Result<Vec<(String, String)>, LibraryError>;
    fn delete_sync_state(&self, key: &str) -> Result<(), LibraryError>;
    /// Replaces the state at `from` with `value` at `to`, atomically.
    fn move_sync_state(&self, from: &str, to: &str, value: &str) -> Result<(), LibraryError>;
    /// The genesis key of the roster held (not a `fork:` copy), if any.
    fn roster_genesis(&self) -> Result<Option<String>, LibraryError>;
    /// The entries stored under `genesis`, in order.
    fn roster_entries(&self, genesis: &str) -> Result<Vec<SignedEntry>, LibraryError>;
    /// Stores an entry at `seq`; the same entry again changes nothing,
    /// another one at a taken seq fails.
    fn add_roster_entry(
        &self,
        genesis: &str,
        seq: u64,
        body: &[u8],
        sig: &[u8],
    ) -> Result<(), LibraryError>;
    /// Replaces the entries stored under `genesis`, atomically.
    fn replace_roster(&self, genesis: &str, entries: &[SignedEntry]) -> Result<(), LibraryError>;
    /// The device left the account: every state but `device_id` and the
    /// `relay:` pins goes, and every roster.
    fn forget(&self) -> Result<(), LibraryError>;
}

/// What every engine call works on: the app's registers, the engine's
/// bookkeeping, and the shape version of the register values this build
/// writes (newer ones are skipped until the app updates).
#[derive(Clone, Copy)]
pub struct Store<'a> {
    pub registers: &'a dyn Registers,
    pub book: &'a dyn Bookkeeping,
    pub shapes: u16,
}

impl Store<'_> {
    /// Both forget, the registers first, so a crash in between never leaves
    /// a join to settle on the next account. Momotus's `Library` is both: its
    /// `forget_sync` clears queue, state and roster in one transaction, and
    /// the second call finds nothing.
    pub(crate) fn forget(&self) -> Result<(), LibraryError> {
        self.registers.forget()?;
        self.book.forget()
    }
}

/// Momotus: one database holds both, at shape version `SHAPES`.
impl<'a> From<&'a Library> for Store<'a> {
    fn from(library: &'a Library) -> Self {
        Store {
            registers: library,
            book: library,
            shapes: crate::engine::SHAPES,
        }
    }
}

impl Registers for Library {
    fn outbox(&self, limit: usize) -> Result<OutboxPage, LibraryError> {
        Library::outbox(self, limit)
    }
    fn outbox_done(&self, id: i64) -> Result<(), LibraryError> {
        Library::outbox_done(self, id)
    }
    fn apply(&self, records: &[SyncRecord]) -> Result<usize, LibraryError> {
        Library::apply(self, records)
    }
    fn requeue(&self, coll: u8, key: &str) -> Result<(), LibraryError> {
        Library::requeue(self, coll, key)
    }
    fn queue_all(&self) -> Result<(), LibraryError> {
        Library::queue_all(self)
    }
    fn restamp(&self, coll: u8, key: &str, hlc: &str) -> Result<(), LibraryError> {
        Library::restamp(self, coll, key, hlc)
    }
    fn forget(&self) -> Result<(), LibraryError> {
        self.forget_sync()
    }
    fn join_defaults_pending(&self) -> Result<bool, LibraryError> {
        Library::join_defaults_pending(self)
    }
    fn settle_join_defaults(&self, device: u64) -> Result<(), LibraryError> {
        Library::settle_join_defaults(self, device)
    }
}

impl Bookkeeping for Library {
    fn sync_state(&self, key: &str) -> Result<Option<String>, LibraryError> {
        Library::sync_state(self, key)
    }
    fn set_sync_state(&self, key: &str, value: &str) -> Result<(), LibraryError> {
        Library::set_sync_state(self, key, value)
    }
    fn sync_states(&self, prefix: &str) -> Result<Vec<(String, String)>, LibraryError> {
        Library::sync_states(self, prefix)
    }
    fn delete_sync_state(&self, key: &str) -> Result<(), LibraryError> {
        Library::delete_sync_state(self, key)
    }
    fn move_sync_state(&self, from: &str, to: &str, value: &str) -> Result<(), LibraryError> {
        Library::move_sync_state(self, from, to, value)
    }
    fn roster_genesis(&self) -> Result<Option<String>, LibraryError> {
        Library::roster_genesis(self)
    }
    fn roster_entries(&self, genesis: &str) -> Result<Vec<SignedEntry>, LibraryError> {
        Library::roster_entries(self, genesis)
    }
    fn add_roster_entry(
        &self,
        genesis: &str,
        seq: u64,
        body: &[u8],
        sig: &[u8],
    ) -> Result<(), LibraryError> {
        Library::add_roster_entry(self, genesis, seq, body, sig)
    }
    fn replace_roster(&self, genesis: &str, entries: &[SignedEntry]) -> Result<(), LibraryError> {
        Library::replace_roster(self, genesis, entries)
    }
    fn forget(&self) -> Result<(), LibraryError> {
        self.forget_sync()
    }
}
