//! The wasm surface checked under the TV's Nodes (`wasm-test.sh` drives it
//! with `tests/wasm/run.js` as the host). A failed check panics, and the
//! panic hook reports it through `mqHost.died`.

use crate::{
    Instant, JoinSet, StdInstant, interval, now_ms, sleep, sleep_until, spawn, spawn_blocking,
    timeout, yield_now,
};
use futures_util::FutureExt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;
use wasm_bindgen::prelude::*;

const MS: Duration = Duration::from_millis(1);

#[wasm_bindgen]
pub async fn selftest() -> String {
    crate::set_panic_hook();
    let mut passed = Vec::new();

    let t = Instant::now();
    sleep(30 * MS).await;
    assert!(
        t.elapsed() >= 30 * MS,
        "sleep woke early: {:?}",
        t.elapsed()
    );
    passed.push("sleep");

    assert!(
        sleep_until(Instant::now() - 5 * MS)
            .now_or_never()
            .is_some()
    );
    assert!(sleep(Duration::ZERO).now_or_never().is_some());
    passed.push("past deadline ready at once");

    // Armed with setTimeout's longest delay, then cleared on drop.
    assert!(
        sleep(Duration::from_secs(40 * 86_400))
            .now_or_never()
            .is_none()
    );
    passed.push("long sleep armed and dropped");

    assert_eq!(timeout(Duration::ZERO, async { 7 }).await, Ok(7));
    let t = Instant::now();
    let late = timeout(20 * MS, sleep(Duration::from_secs(5))).await;
    assert!(late.is_err() && t.elapsed() >= 20 * MS);
    passed.push("timeout");

    assert_eq!(spawn(async { 41 + 1 }).await.unwrap(), 42);
    let ran = Arc::new(AtomicBool::new(false));
    let flag = ran.clone();
    let task = spawn(async move {
        sleep(Duration::from_secs(5)).await;
        flag.store(true, Ordering::SeqCst);
    });
    let handle = task.abort_handle();
    assert!(!task.is_finished());
    task.abort();
    let err = task.await.unwrap_err();
    assert!(err.is_cancelled() && !err.is_panic() && err.id() == handle.id());
    assert!(handle.is_finished() && !ran.load(Ordering::SeqCst));
    passed.push("spawn, abort");

    let order = Arc::new(AtomicU32::new(0));
    let mut set = JoinSet::new();
    let mut ids = Vec::new();
    for (n, ms) in [(1u32, 40u32), (2, 10), (3, 25)] {
        let order = order.clone();
        ids.push((
            n,
            set.spawn(async move {
                sleep(ms * MS).await;
                order.fetch_add(1, Ordering::SeqCst);
                n
            })
            .id(),
        ));
    }
    assert_eq!(set.len(), 3);
    assert!(set.try_join_next().is_none());
    let (id, first) = set.join_next_with_id().await.unwrap().unwrap();
    assert_eq!(first, 2);
    assert!(ids.contains(&(2, id)));
    assert_eq!(set.join_next().await.unwrap().unwrap(), 3);
    assert_eq!(set.join_next().await.unwrap().unwrap(), 1);
    assert!(set.join_next().await.is_none() && set.is_empty());
    passed.push("join set order and ids");

    let dropped = Arc::new(AtomicBool::new(false));
    let flag = dropped.clone();
    let mut set = JoinSet::new();
    set.spawn(async move {
        sleep(20 * MS).await;
        flag.store(true, Ordering::SeqCst);
    });
    set.spawn(async {});
    yield_now().await;
    assert!(set.try_join_next().is_some());
    drop(set);
    sleep(40 * MS).await;
    assert!(
        !dropped.load(Ordering::SeqCst),
        "a dropped set's task ran on"
    );
    passed.push("join set aborts on drop");

    let t = Instant::now();
    let mut every = interval(20 * MS);
    every.tick().await;
    assert!(t.elapsed() < 15 * MS, "first tick waited");
    every.tick().await;
    assert!(t.elapsed() >= 20 * MS);
    sleep(70 * MS).await;
    // Burst: the missed ticks come at once, then the schedule goes on.
    let before = Instant::now();
    every.tick().await;
    every.tick().await;
    assert!(before.elapsed() < 15 * MS, "missed ticks were not a burst");
    passed.push("interval");

    let t = Instant::now();
    let blocking = spawn_blocking(|| 5);
    assert!(blocking.is_finished());
    assert_eq!(blocking.await.unwrap(), 5);
    assert!(t.elapsed() < 15 * MS);
    passed.push("spawn_blocking inline");

    let other = Arc::new(AtomicBool::new(false));
    let flag = other.clone();
    let _detached = spawn(async move { flag.store(true, Ordering::SeqCst) });
    yield_now().await;
    yield_now().await;
    assert!(other.load(Ordering::SeqCst));
    passed.push("yield_now");

    let std_now = StdInstant::now();
    assert_eq!(Instant::from_std(std_now).into_std(), std_now);
    assert!(now_ms() > 1_700_000_000_000);
    assert!(crate::in_runtime());
    passed.push("clocks");

    crate::log(3, "mq-rt selftest done");
    passed.join(", ")
}
