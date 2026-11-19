//! The engine on a second `Registers` and `Bookkeeping`: a small store in
//! memory, as an app other than Momotus brings it (records it shapes itself
//! in one object, the engine's bookkeeping in another, as an app may keep
//! them in two databases), against a real relay. The engine's rules hold
//! with it: what one device writes reaches the other, the newer write wins,
//! an apply queues nothing back, a device's first contact sends what it held
//! before, a join pulls and settles before its first push, a write from a
//! clock far ahead is restamped (its own) or waits queued (another
//! device's), a rotation reaches every device, the app's shape version
//! decides what it reads, and leaving forgets the account in both.

use mq_http::{ClientConfig, HttpClient};
use mq_library::hlc::{Hlc, HlcClock, now_ms};
use mq_library::{LibraryError, OutboxPage, SignedEntry, SyncRecord};
use mq_sync::pair::{Recovery, make_kit, recover_with};
use mq_sync::{Bookkeeping, Registers, Store, SyncConfig, leave_here, remove_devices, sync_once};
use mq_sync_server::{Config, router};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::Mutex;

/// The collection these records go in. Not 1: the relay refuses Momotus's
/// whole addon lists, which older builds sent there.
const COLL: u8 = 40;

/// The app's records, and what the engine asked of them, in order.
#[derive(Default)]
struct MemRegisters(Mutex<Regs>);

#[derive(Default)]
struct Regs {
    registers: BTreeMap<(u8, String), (Value, String)>,
    outbox: Vec<(i64, u8, String)>,
    next_id: i64,
    join_pending: bool,
    /// What the registers held when the join settled.
    settled_with: Option<usize>,
    log: Vec<&'static str>,
}

impl Regs {
    fn queue(&mut self, coll: u8, key: &str) {
        self.next_id += 1;
        self.outbox.push((self.next_id, coll, key.to_owned()));
    }
}

/// The engine's bookkeeping, in its own object.
#[derive(Default)]
struct MemBook(Mutex<Book>);

#[derive(Default)]
struct Book {
    state: BTreeMap<String, String>,
    roster: BTreeMap<String, Vec<SignedEntry>>,
}

fn newer(a: &str, b: &str) -> bool {
    Hlc::decode(a) > Hlc::decode(b)
}

fn refused(why: &str) -> LibraryError {
    LibraryError::Json(serde_json::Error::io(std::io::Error::other(why.to_owned())))
}

impl MemRegisters {
    fn lock(&self) -> std::sync::MutexGuard<'_, Regs> {
        self.0.lock().unwrap()
    }

    /// A local edit at `hlc`, queued.
    fn write_at(&self, key: &str, value: Value, hlc: String) {
        let mut regs = self.lock();
        regs.registers.insert((COLL, key.to_owned()), (value, hlc));
        regs.queue(COLL, key);
    }

    fn get(&self, key: &str) -> Option<Value> {
        self.lock()
            .registers
            .get(&(COLL, key.to_owned()))
            .map(|(v, _)| v.clone())
    }

    fn hlc(&self, key: &str) -> Option<Hlc> {
        let regs = self.lock();
        Hlc::decode(&regs.registers.get(&(COLL, key.to_owned()))?.1)
    }

    fn saw(&self, event: &str) -> Option<usize> {
        self.lock().log.iter().position(|e| *e == event)
    }
}

impl Registers for MemRegisters {
    fn outbox(&self, limit: usize) -> Result<OutboxPage, LibraryError> {
        let mut regs = self.lock();
        regs.log.push("outbox");
        let queued = &regs.outbox[..regs.outbox.len().min(limit)];
        let newest = |coll: &u8, key: &String| {
            queued
                .iter()
                .rev()
                .find(|(_, c, k)| c == coll && k == key)
                .map(|q| q.0)
        };
        let records = queued
            .iter()
            .filter(|(id, coll, key)| newest(coll, key) == Some(*id))
            .filter_map(|(id, coll, key)| {
                let (value, hlc) = regs.registers.get(&(*coll, key.clone()))?;
                let record = SyncRecord {
                    coll: *coll,
                    key: key.clone(),
                    value: value.clone(),
                    hlc: hlc.clone(),
                };
                Some((*id, record))
            })
            .collect();
        let last_id = queued.last().map(|q| q.0);
        Ok(OutboxPage { records, last_id })
    }

    fn outbox_done(&self, id: i64) -> Result<(), LibraryError> {
        self.lock().outbox.retain(|q| q.0 > id);
        Ok(())
    }

    fn apply(&self, records: &[SyncRecord]) -> Result<usize, LibraryError> {
        let mut regs = self.lock();
        regs.log.push("apply");
        let mut changed = 0;
        for r in records {
            let slot = (r.coll, r.key.clone());
            if regs
                .registers
                .get(&slot)
                .is_none_or(|(_, held)| newer(&r.hlc, held))
            {
                regs.registers
                    .insert(slot, (r.value.clone(), r.hlc.clone()));
                changed += 1;
            }
        }
        Ok(changed)
    }

    fn requeue(&self, coll: u8, key: &str) -> Result<(), LibraryError> {
        let mut regs = self.lock();
        regs.log.push("requeue");
        regs.queue(coll, key);
        Ok(())
    }

    fn queue_all(&self) -> Result<(), LibraryError> {
        let mut regs = self.lock();
        let all: Vec<(u8, String)> = regs.registers.keys().cloned().collect();
        for (coll, key) in all {
            if !regs.outbox.iter().any(|q| q.1 == coll && q.2 == key) {
                regs.queue(coll, &key);
            }
        }
        Ok(())
    }

    fn restamp(&self, coll: u8, key: &str, hlc: &str) -> Result<(), LibraryError> {
        let mut regs = self.lock();
        regs.log.push("restamp");
        if let Some(held) = regs.registers.get_mut(&(coll, key.to_owned())) {
            held.1 = hlc.to_owned();
        }
        regs.queue(coll, key);
        Ok(())
    }

    fn forget(&self) -> Result<(), LibraryError> {
        let mut regs = self.lock();
        regs.outbox.clear();
        regs.join_pending = false;
        Ok(())
    }

    fn join_defaults_pending(&self) -> Result<bool, LibraryError> {
        Ok(self.lock().join_pending)
    }

    fn settle_join_defaults(&self, _device: u64) -> Result<(), LibraryError> {
        let mut regs = self.lock();
        regs.log.push("settle");
        regs.join_pending = false;
        regs.settled_with = Some(regs.registers.len());
        Ok(())
    }
}

impl MemBook {
    fn lock(&self) -> std::sync::MutexGuard<'_, Book> {
        self.0.lock().unwrap()
    }
}

impl Bookkeeping for MemBook {
    fn sync_state(&self, key: &str) -> Result<Option<String>, LibraryError> {
        Ok(self.lock().state.get(key).cloned())
    }
    fn set_sync_state(&self, key: &str, value: &str) -> Result<(), LibraryError> {
        self.lock().state.insert(key.to_owned(), value.to_owned());
        Ok(())
    }
    fn sync_states(&self, prefix: &str) -> Result<Vec<(String, String)>, LibraryError> {
        let book = self.lock();
        Ok(book
            .state
            .iter()
            .filter(|(k, _)| k.starts_with(prefix))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect())
    }
    fn delete_sync_state(&self, key: &str) -> Result<(), LibraryError> {
        self.lock().state.remove(key);
        Ok(())
    }
    fn move_sync_state(&self, from: &str, to: &str, value: &str) -> Result<(), LibraryError> {
        let mut book = self.lock();
        book.state.remove(from);
        book.state.insert(to.to_owned(), value.to_owned());
        Ok(())
    }
    fn roster_genesis(&self) -> Result<Option<String>, LibraryError> {
        let book = self.lock();
        Ok(book
            .roster
            .iter()
            .find(|(g, e)| !g.starts_with("fork:") && !e.is_empty())
            .map(|(g, _)| g.clone()))
    }
    fn roster_entries(&self, genesis: &str) -> Result<Vec<SignedEntry>, LibraryError> {
        Ok(self.lock().roster.get(genesis).cloned().unwrap_or_default())
    }
    fn add_roster_entry(
        &self,
        genesis: &str,
        seq: u64,
        body: &[u8],
        sig: &[u8],
    ) -> Result<(), LibraryError> {
        let mut book = self.lock();
        let entries = book.roster.entry(genesis.to_owned()).or_default();
        match entries.get(seq as usize) {
            Some(held) if held.0 == body && held.1 == sig => Ok(()),
            Some(_) => Err(refused("another entry at that seq")),
            None if entries.len() == seq as usize => {
                entries.push((body.to_vec(), sig.to_vec()));
                Ok(())
            }
            None => Err(refused("a gap in the roster")),
        }
    }
    fn replace_roster(&self, genesis: &str, entries: &[SignedEntry]) -> Result<(), LibraryError> {
        self.lock()
            .roster
            .insert(genesis.to_owned(), entries.to_vec());
        Ok(())
    }
    fn forget(&self) -> Result<(), LibraryError> {
        let mut book = self.lock();
        book.state
            .retain(|k, _| k == "device_id" || k.starts_with("relay:"));
        book.roster.clear();
        Ok(())
    }
}

fn http() -> HttpClient {
    HttpClient::new(ClientConfig::default()).unwrap()
}

struct Dir(std::path::PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

async fn relay() -> (String, Dir) {
    let mut b = [0u8; 8];
    getrandom::fill(&mut b).unwrap();
    let dir = std::env::temp_dir().join(format!("mq-registers-{}", hex::encode(b)));
    std::fs::create_dir_all(&dir).unwrap();
    let app = router(&dir.join("relay.sqlite"), Config::default()).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { mq_sync_server::serve(listener, app).await.unwrap() });
    (url, Dir(dir))
}

struct Device {
    regs: MemRegisters,
    book: MemBook,
    record: SyncConfig,
    clock: HlcClock,
    shapes: u16,
}

impl Device {
    fn new(record: SyncConfig, device: u64) -> Self {
        Self {
            regs: MemRegisters::default(),
            book: MemBook::default(),
            record,
            clock: HlcClock::new(device),
            shapes: 1,
        }
    }

    fn store(&self) -> Store<'_> {
        Store {
            registers: &self.regs,
            book: &self.book,
            shapes: self.shapes,
        }
    }

    fn write(&self, key: &str, value: Value) {
        let hlc = self.clock.tick(now_ms()).encode();
        self.regs.write_at(key, value, hlc);
    }

    async fn sync(&mut self) -> mq_sync::SyncReport {
        loop {
            let report = sync_once(self.store(), &http(), &self.record, &self.clock)
                .await
                .unwrap();
            if let Some(record) = report.record.clone() {
                self.record = record;
            }
            if !report.again {
                return report;
            }
        }
    }

    /// A new device joined by a kit this one makes (`settle` set as an app
    /// sets it on a join).
    async fn join(&mut self, url: &str, device: u64, settle: bool) -> Device {
        let (kit, _, record) = make_kit(self.store(), &http(), &self.record, &self.clock)
            .await
            .unwrap();
        self.record = record;
        let (record, _) = recover_with(&http(), url, Recovery::Kit(kit), "B", None)
            .await
            .unwrap();
        let joined = Device::new(record, device);
        joined.regs.lock().join_pending = settle;
        joined
    }
}

async fn two_devices(url: &str, settle: bool) -> (Device, Device) {
    let record = mq_sync::create_account(&http(), url, None, "A")
        .await
        .unwrap();
    let mut a = Device::new(record, 1);
    let b = a.join(url, 2, settle).await;
    (a, b)
}

#[tokio::test]
async fn two_devices_converge_on_a_second_store() {
    let (url, _dir) = relay().await;
    let (mut a, mut b) = two_devices(&url, false).await;
    a.write("x", json!(1));
    // Written before B's first contact: sent then.
    b.write("early", json!("b"));
    assert!(a.sync().await.pushed >= 1);
    let first = b.sync().await;
    assert_eq!(b.regs.get("x"), Some(json!(1)));
    assert!(first.pushed >= 1, "{first:?}");
    // An apply queues nothing back.
    assert_eq!(b.regs.lock().outbox.len(), 0);
    a.sync().await;
    assert_eq!(a.regs.get("early"), Some(json!("b")));

    // The newer write wins on both: A writes after it saw B's, and B's
    // older value never comes back.
    b.write("x", json!(2));
    b.sync().await;
    a.sync().await;
    assert_eq!(a.regs.get("x"), Some(json!(2)));
    a.write("x", json!(3));
    a.sync().await;
    b.sync().await;
    a.sync().await;
    assert_eq!(a.regs.get("x"), Some(json!(3)));
    assert_eq!(b.regs.get("x"), Some(json!(3)));
}

#[tokio::test]
async fn a_join_pulls_and_settles_before_its_first_push() {
    let (url, _dir) = relay().await;
    let (mut a, mut b) = two_devices(&url, true).await;
    a.write("x", json!(1));
    a.write("y", json!(1));
    a.sync().await;
    b.write("mine", json!(1));
    b.sync().await;
    // Settled once, with the account's two records already applied, and
    // before the outbox was first read.
    assert_eq!(b.regs.lock().settled_with, Some(3));
    let (settle, outbox) = (b.regs.saw("settle").unwrap(), b.regs.saw("outbox").unwrap());
    assert!(
        b.regs.saw("apply").unwrap() < settle && settle < outbox,
        "{:?}",
        b.regs.lock().log
    );
    a.sync().await;
    assert_eq!(a.regs.get("mine"), Some(json!(1)));
}

#[tokio::test]
async fn a_write_from_a_clock_far_ahead_is_restamped_or_waits() {
    let (url, _dir) = relay().await;
    let (mut a, mut b) = two_devices(&url, false).await;
    let ahead = |device| {
        Hlc {
            wall_ms: now_ms() + 3_600_000,
            counter: 0,
            device,
        }
        .encode()
    };
    // Its own: stamped again at the relay's time, and sent.
    a.regs.write_at("own", json!(1), ahead(1));
    // Another device's (an import): never restamped, it waits queued.
    a.regs.write_at("theirs", json!(2), ahead(7));
    let report = a.sync().await;
    assert_eq!(report.waiting, 1, "{report:?}");
    assert!(a.regs.saw("restamp").is_some() && a.regs.saw("requeue").is_some());
    let own = a.regs.hlc("own").unwrap();
    assert!(
        own.wall_ms < now_ms() + 11 * 60 * 1000,
        "restamped near the relay's time"
    );
    assert!(
        a.regs.lock().outbox.iter().any(|q| q.2 == "theirs"),
        "still queued"
    );
    // The round stopped at what waits; the next one sends the restamped write.
    a.sync().await;
    b.sync().await;
    assert_eq!(b.regs.get("own"), Some(json!(1)));
    assert_eq!(b.regs.get("theirs"), None);
}

#[tokio::test]
async fn a_rotation_reaches_every_device_on_a_second_store() {
    let (url, _dir) = relay().await;
    let (mut a, mut b) = two_devices(&url, false).await;
    let c = a.join(&url, 3, false).await;
    b.sync().await;
    let epoch = a.record.keys.epoch;
    a.record = remove_devices(
        a.store(),
        &http(),
        &a.record,
        &a.clock,
        vec![c.record.secret().id()],
    )
    .await
    .unwrap();
    assert!(a.record.keys.epoch > epoch);
    a.write("after", json!(1));
    a.sync().await;
    // B takes the new key from its box and reads the new epoch's record.
    b.sync().await;
    assert_eq!(b.regs.get("after"), Some(json!(1)));
    assert!(b.record.keys.epoch > epoch);
    assert!(!b.book.lock().roster.is_empty());
}

#[tokio::test]
async fn the_app_shape_version_decides_what_a_device_reads() {
    let (url, _dir) = relay().await;
    let (mut a, mut b) = two_devices(&url, false).await;
    a.shapes = 2;
    a.write("new", json!({ "v": 2 }));
    a.sync().await;
    // A reader of shape 1 skips it; updated to 2, it reads everything again.
    assert_eq!(b.sync().await.newer, 1);
    assert_eq!(b.regs.get("new"), None);
    b.shapes = 2;
    b.sync().await;
    assert_eq!(b.regs.get("new"), Some(json!({ "v": 2 })));
}

#[tokio::test]
async fn leaving_here_forgets_the_account_in_both_and_keeps_the_records() {
    let (url, _dir) = relay().await;
    let (mut a, _) = two_devices(&url, false).await;
    a.write("x", json!(1));
    a.sync().await;
    a.write("unsent", json!(1));
    a.book.lock().state.insert("device_id".into(), "d".into());
    a.regs.lock().join_pending = true;
    leave_here(a.store(), &a.record).unwrap();
    let book = a.book.lock();
    assert!(book.roster.is_empty());
    assert!(
        book.state
            .keys()
            .all(|k| k == "device_id" || k.starts_with("relay:")),
        "{:?}",
        book.state
    );
    assert!(book.state.contains_key("device_id"));
    let regs = a.regs.lock();
    assert!(regs.outbox.is_empty() && !regs.join_pending);
    assert_eq!(regs.registers.len(), 2);
}
