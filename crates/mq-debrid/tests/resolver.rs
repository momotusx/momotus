use mq_debrid::{
    CacheStatus, Capabilities, DebridError, ManualClock, Priority, ResolveRequest, ResolvedLink,
    Resolver, ResolverConfig, Store, StoreId, StoreUser, TorrentFile, TorrentHandle, TorrentInfo,
    TorrentStatus,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const HASH: &str = "dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c";

struct State {
    id: StoreId,
    instant_check: bool,
    check_result: CacheStatus,
    add_delay: Duration,
    add_error: Mutex<Option<DebridError>>,
    statuses: Vec<TorrentStatus>,
    files: Vec<TorrentFile>,
    link_expires_at: Mutex<Option<u64>>,
    adds: AtomicUsize,
    checks: AtomicUsize,
    torrent_calls: AtomicUsize,
    add_priorities: Mutex<Vec<Priority>>,
    selects: Mutex<Vec<Vec<u32>>>,
    linked: Mutex<Vec<u32>>,
}

impl State {
    fn new() -> Self {
        State {
            id: StoreId::RealDebrid,
            instant_check: false,
            check_result: CacheStatus::Unknown,
            add_delay: Duration::ZERO,
            add_error: Mutex::new(None),
            statuses: vec![TorrentStatus::Ready],
            files: vec![
                file(1, "Movie/movie.mkv", 4000),
                file(2, "Movie/movie.nfo", 1),
            ],
            link_expires_at: Mutex::new(None),
            adds: AtomicUsize::new(0),
            checks: AtomicUsize::new(0),
            torrent_calls: AtomicUsize::new(0),
            add_priorities: Mutex::new(Vec::new()),
            selects: Mutex::new(Vec::new()),
            linked: Mutex::new(Vec::new()),
        }
    }

    fn adds(&self) -> usize {
        self.adds.load(Ordering::SeqCst)
    }
}

fn file(index: u32, path: &str, size: u64) -> TorrentFile {
    TorrentFile {
        index,
        path: path.to_owned(),
        size,
        selected: false,
        link: None,
    }
}

struct Mock {
    state: Arc<State>,
    priority: Priority,
}

impl Store for Mock {
    fn id(&self) -> StoreId {
        self.state.id
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            instant_check: self.state.instant_check,
            file_select: true,
            ip_locked: false,
            device_code_login: false,
            rate_limit_per_min: None,
        }
    }

    async fn user(&self) -> Result<StoreUser, DebridError> {
        unimplemented!()
    }

    async fn check(&self, hashes: &[String]) -> Result<Vec<CacheStatus>, DebridError> {
        self.state.checks.fetch_add(1, Ordering::SeqCst);
        Ok(vec![self.state.check_result; hashes.len()])
    }

    async fn add_magnet(&self, info_hash: &str) -> Result<TorrentHandle, DebridError> {
        self.state.adds.fetch_add(1, Ordering::SeqCst);
        self.state
            .add_priorities
            .lock()
            .unwrap()
            .push(self.priority);
        tokio::time::sleep(self.state.add_delay).await;
        if let Some(err) = self.state.add_error.lock().unwrap().take() {
            return Err(err);
        }
        Ok(TorrentHandle {
            id: "T1".into(),
            hash: info_hash.to_owned(),
        })
    }

    async fn torrent(&self, id: &str) -> Result<TorrentInfo, DebridError> {
        let n = self.state.torrent_calls.fetch_add(1, Ordering::SeqCst);
        let status = self.state.statuses[n.min(self.state.statuses.len() - 1)].clone();
        Ok(TorrentInfo {
            id: id.to_owned(),
            hash: HASH.to_owned(),
            name: "t".into(),
            status,
            progress: 0.0,
            files: self.state.files.clone(),
        })
    }

    async fn select_files(&self, _id: &str, file_ids: &[u32]) -> Result<(), DebridError> {
        self.state.selects.lock().unwrap().push(file_ids.to_vec());
        Ok(())
    }

    async fn link(
        &self,
        torrent: &TorrentInfo,
        file_index: u32,
    ) -> Result<ResolvedLink, DebridError> {
        self.state.linked.lock().unwrap().push(file_index);
        let file = torrent
            .files
            .iter()
            .find(|f| f.index == file_index)
            .unwrap();
        Ok(ResolvedLink {
            url: format!("https://cdn.example/{}/{}", torrent.id, file.path),
            size: Some(file.size),
            filename: file.path.rsplit('/').next().unwrap().to_owned(),
            expires_at: *self.state.link_expires_at.lock().unwrap(),
        })
    }

    async fn remove(&self, _id: &str) -> Result<(), DebridError> {
        unimplemented!()
    }
}

fn config() -> ResolverConfig {
    ResolverConfig {
        link_ttl: Duration::from_secs(900),
        poll_interval: Duration::from_millis(2),
        ready_timeout: Duration::from_millis(80),
    }
}

fn resolver(state: &Arc<State>) -> Resolver {
    let state = state.clone();
    Resolver::new(config()).add_store(move |priority| Mock {
        state: state.clone(),
        priority,
    })
}

fn req() -> ResolveRequest {
    ResolveRequest::new(HASH)
}

#[tokio::test]
async fn ten_concurrent_identical_resolves_cause_one_add() {
    let mut state = State::new();
    state.add_delay = Duration::from_millis(50);
    let state = Arc::new(state);
    let resolver = resolver(&state);

    let tasks: Vec<_> = (0..10)
        .map(|_| {
            let resolver = resolver.clone();
            tokio::spawn(async move { resolver.resolve(req()).await })
        })
        .collect();
    let mut urls = Vec::new();
    for task in tasks {
        urls.push(task.await.unwrap().unwrap().url);
    }
    assert_eq!(state.adds(), 1, "one add for ten concurrent callers");
    assert!(urls.iter().all(|u| u == &urls[0]));
    assert_eq!(urls[0], "https://cdn.example/T1/Movie/movie.mkv");

    // A later call is served from the cache.
    resolver.resolve(req()).await.unwrap();
    assert_eq!(state.adds(), 1);
}

#[tokio::test]
async fn dedupe_shares_a_failure_and_does_not_cache_it() {
    let mut state = State::new();
    state.add_delay = Duration::from_millis(30);
    *state.add_error.lock().unwrap() = Some(DebridError::Auth);
    let state = Arc::new(state);
    let resolver = resolver(&state);

    let (a, b) = tokio::join!(resolver.resolve(req()), resolver.resolve(req()));
    assert!(matches!(a, Err(DebridError::Auth)));
    assert!(matches!(b, Err(DebridError::Auth)));
    assert_eq!(state.adds(), 1);

    // The error was one-shot; a retry resolves for real.
    resolver.resolve(req()).await.unwrap();
    assert_eq!(state.adds(), 2);
}

#[tokio::test]
async fn link_cache_expires_by_injected_time() {
    let state = Arc::new(State::new());
    let clock = Arc::new(ManualClock::new(1_000_000));
    let resolver = resolver(&state).with_clock(clock.clone());

    resolver.resolve(req()).await.unwrap();
    clock.advance(899);
    resolver.resolve(req()).await.unwrap();
    assert_eq!(state.adds(), 1, "still inside the 900 s TTL");

    clock.advance(2);
    resolver.resolve(req()).await.unwrap();
    assert_eq!(state.adds(), 2, "TTL elapsed, resolved again");
}

#[tokio::test]
async fn cache_never_outlives_the_provider_expiry() {
    let state = Arc::new(State::new());
    let clock = Arc::new(ManualClock::new(1_000_000));
    *state.link_expires_at.lock().unwrap() = Some(1_000_060);
    let resolver = resolver(&state).with_clock(clock.clone());

    resolver.resolve(req()).await.unwrap();
    clock.advance(59);
    resolver.resolve(req()).await.unwrap();
    assert_eq!(state.adds(), 1);

    clock.advance(2);
    resolver.resolve(req()).await.unwrap();
    assert_eq!(
        state.adds(),
        2,
        "provider expiry (60 s) beats the 900 s TTL"
    );
}

#[tokio::test]
async fn an_already_expired_link_is_returned_but_not_cached() {
    let state = Arc::new(State::new());
    let clock = Arc::new(ManualClock::new(1_000_000));
    *state.link_expires_at.lock().unwrap() = Some(999_999);
    let resolver = resolver(&state).with_clock(clock);

    resolver.resolve(req()).await.unwrap();
    resolver.resolve(req()).await.unwrap();
    assert_eq!(state.adds(), 2);
}

#[tokio::test]
async fn walks_metadata_selection_and_download_then_links_the_picked_episode() {
    let mut state = State::new();
    state.statuses = vec![
        TorrentStatus::Resolving,
        TorrentStatus::WaitingFileSelection,
        TorrentStatus::Downloading,
        TorrentStatus::Ready,
    ];
    state.files = vec![
        file(1, "Show.S01/Show.S01E01.mkv", 900),
        file(2, "Show.S01/Show.S01E02.mkv", 800),
        file(3, "Show.S01/Sample/Show.S01E02.sample.mkv", 10),
        file(4, "Show.S01/Show.S01E03.mkv", 950),
    ];
    let state = Arc::new(state);
    let resolver = resolver(&state);
    let mut request = req();
    request.season = Some(1);
    request.episode = Some(2);

    let link = resolver.resolve(request).await.unwrap();
    assert_eq!(link.filename, "Show.S01E02.mkv");
    assert_eq!(*state.selects.lock().unwrap(), vec![vec![2]]);
    assert_eq!(*state.linked.lock().unwrap(), vec![2]);
    assert_eq!(state.torrent_calls.load(Ordering::SeqCst), 4);
}

#[tokio::test]
async fn skips_selection_when_the_provider_has_files_ready() {
    let state = Arc::new(State::new());
    resolver(&state).resolve(req()).await.unwrap();
    assert!(state.selects.lock().unwrap().is_empty());
    assert_eq!(*state.linked.lock().unwrap(), vec![1]);
}

#[tokio::test]
async fn waiting_out_an_uncached_torrent_ends_in_not_cached() {
    let mut state = State::new();
    state.statuses = vec![TorrentStatus::Downloading];
    let state = Arc::new(state);
    let err = resolver(&state).resolve(req()).await.unwrap_err();
    assert!(matches!(err, DebridError::NotCached), "{err:?}");
}

#[tokio::test]
async fn stuck_metadata_is_a_timeout_and_a_dead_torrent_is_a_provider_error() {
    let mut stuck = State::new();
    stuck.statuses = vec![TorrentStatus::Resolving];
    let err = resolver(&Arc::new(stuck)).resolve(req()).await.unwrap_err();
    assert!(matches!(err, DebridError::Timeout), "{err:?}");

    let mut dead = State::new();
    dead.statuses = vec![TorrentStatus::Failed("dead".into())];
    let err = resolver(&Arc::new(dead)).resolve(req()).await.unwrap_err();
    assert!(
        matches!(err, DebridError::Provider { ref code, ref message } if code == "torrent_failed" && message == "dead"),
        "{err:?}"
    );
}

#[tokio::test]
async fn no_playable_file_is_reported() {
    let mut state = State::new();
    state.files = vec![file(1, "readme.txt", 5), file(2, "cover.jpg", 50)];
    let err = resolver(&Arc::new(state)).resolve(req()).await.unwrap_err();
    assert!(matches!(err, DebridError::NoPlayableFile), "{err:?}");
}

#[tokio::test]
async fn reliable_cache_check_fails_fast_without_an_add() {
    let mut state = State::new();
    state.instant_check = true;
    state.check_result = CacheStatus::NotCached;
    let state = Arc::new(state);
    let err = resolver(&state).resolve(req()).await.unwrap_err();
    assert!(matches!(err, DebridError::NotCached), "{err:?}");
    assert_eq!(state.checks.load(Ordering::SeqCst), 1);
    assert_eq!(state.adds(), 0);
}

#[tokio::test]
async fn cached_or_unknown_check_results_proceed_to_add() {
    for result in [CacheStatus::Cached, CacheStatus::Unknown] {
        let mut state = State::new();
        state.instant_check = true;
        state.check_result = result;
        let state = Arc::new(state);
        resolver(&state).resolve(req()).await.unwrap();
        assert_eq!(state.adds(), 1, "{result:?}");
    }
}

#[tokio::test]
async fn distinct_requests_do_not_share_flights_or_cache() {
    let state = Arc::new(State::new());
    let resolver = resolver(&state);
    let mut a = req();
    a.season = Some(1);
    a.episode = Some(1);
    let mut b = req();
    b.season = Some(1);
    b.episode = Some(2);
    let _ = tokio::join!(resolver.resolve(a), resolver.resolve(b));
    assert_eq!(state.adds(), 2);
}

#[tokio::test]
async fn store_selection_defaults_to_the_first_and_honours_the_request() {
    let first = Arc::new(State::new());
    let mut second = State::new();
    second.id = StoreId::TorBox;
    let second = Arc::new(second);
    let (s1, s2) = (first.clone(), second.clone());
    let resolver = Resolver::new(config())
        .add_store(move |priority| Mock {
            state: s1.clone(),
            priority,
        })
        .add_store(move |priority| Mock {
            state: s2.clone(),
            priority,
        });

    resolver.resolve(req()).await.unwrap();
    assert_eq!((first.adds(), second.adds()), (1, 0));

    let mut wanted = req();
    wanted.store = Some(StoreId::TorBox);
    resolver.resolve(wanted).await.unwrap();
    assert_eq!((first.adds(), second.adds()), (1, 1));

    let mut missing = req();
    missing.store = Some(StoreId::Premiumize);
    let err = resolver.resolve(missing).await.unwrap_err();
    assert!(
        matches!(err, DebridError::Provider { ref code, .. } if code == "store_not_configured"),
        "{err:?}"
    );
}

#[tokio::test]
async fn prefetch_warms_the_cache_at_prefetch_priority() {
    let state = Arc::new(State::new());
    let warmed = resolver(&state);
    warmed.prefetch(req()).await.unwrap();
    warmed.resolve(req()).await.unwrap();
    assert_eq!(state.adds(), 1);
    assert_eq!(
        *state.add_priorities.lock().unwrap(),
        vec![Priority::Prefetch]
    );

    let user_state = Arc::new(State::new());
    resolver(&user_state).resolve(req()).await.unwrap();
    assert_eq!(
        *user_state.add_priorities.lock().unwrap(),
        vec![Priority::Resolve]
    );
}

#[tokio::test]
async fn invalid_hash_is_rejected_before_any_store_call() {
    let state = Arc::new(State::new());
    let err = resolver(&state)
        .resolve(ResolveRequest::new("nothex"))
        .await
        .unwrap_err();
    assert!(matches!(err, DebridError::Provider { ref code, .. } if code == "invalid_info_hash"));
    assert_eq!(state.adds(), 0);
}

fn mock(state: &Arc<State>) -> impl Fn(Priority) -> Mock + use<> {
    let state = state.clone();
    move |priority| Mock {
        state: state.clone(),
        priority,
    }
}

#[tokio::test]
async fn old_generation_results_are_never_published() {
    use tokio_util::sync::CancellationToken;
    // Cached: a link resolved under one connection never serves the next.
    let state = Arc::new(State::new());
    let gone = CancellationToken::new();
    let old = Resolver::new(config()).add_connection("A", gone.clone(), mock(&state));
    old.resolve(req()).await.unwrap();
    gone.cancel();
    old.prune();
    let new = old
        .sharing()
        .add_connection("B", CancellationToken::new(), mock(&state));
    new.resolve(req()).await.unwrap();
    assert_eq!(state.adds(), 2, "the new connection resolves on its own");
    assert!(
        old.resolve(req()).await.is_err(),
        "a gone connection resolves nothing"
    );

    // In flight: cancelling stops the resolve and caches nothing.
    let mut slow = State::new();
    slow.add_delay = Duration::from_millis(150);
    let slow = Arc::new(slow);
    let gone = CancellationToken::new();
    let old = Resolver::new(config()).add_connection("A", gone.clone(), mock(&slow));
    let flight = tokio::spawn({
        let old = old.clone();
        async move { old.resolve(req()).await }
    });
    tokio::time::sleep(Duration::from_millis(30)).await;
    gone.cancel();
    let err = flight.await.unwrap().unwrap_err();
    assert!(
        matches!(&err, DebridError::Provider { code, .. } if code == "disconnected"),
        "{err:?}"
    );
    let new = old
        .sharing()
        .add_connection("B", CancellationToken::new(), mock(&slow));
    new.resolve(req()).await.unwrap();
    assert_eq!(slow.adds(), 2, "nothing of the cancelled flight was kept");
}

#[tokio::test]
async fn a_waiter_resuming_after_disconnect_gets_no_link() {
    use futures_util::FutureExt;
    use tokio_util::sync::CancellationToken;
    let mut slow = State::new();
    slow.add_delay = Duration::from_millis(30);
    let slow = Arc::new(slow);
    let gone = CancellationToken::new();
    let resolver = Resolver::new(config()).add_connection("A", gone.clone(), mock(&slow));
    // The waiter joins the flight, then is not polled while it finishes.
    let mut waiter = Box::pin(resolver.resolve(req()));
    assert!((&mut waiter).now_or_never().is_none());
    resolver.resolve(req()).await.unwrap();
    gone.cancel();
    resolver.prune();
    let err = waiter.await.unwrap_err();
    assert!(
        matches!(&err, DebridError::Provider { code, .. } if code == "disconnected"),
        "{err:?}"
    );
}

#[tokio::test]
async fn prune_drops_an_abandoned_flight() {
    use futures_util::FutureExt;
    use tokio_util::sync::CancellationToken;
    let mut slow = State::new();
    slow.add_delay = Duration::from_secs(30);
    let slow = Arc::new(slow);
    let gone = CancellationToken::new();
    let resolver = Resolver::new(config()).add_connection("A", gone.clone(), mock(&slow));
    // A caller that started a flight and went away.
    let mut abandoned = Box::pin(resolver.resolve(req()));
    assert!((&mut abandoned).now_or_never().is_none());
    drop(abandoned);
    gone.cancel();
    resolver.prune();
    drop(resolver);
    // Nothing holds the store any more: not the flight, not the resolver.
    assert_eq!(Arc::strong_count(&slow), 1);
}
