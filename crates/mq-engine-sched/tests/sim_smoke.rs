//! Smoke test for the simulator in `examples/sim`: it runs, DWH reaches
//! playback, and DWH's own `Ready` signal fires exactly when the simulated
//! player can start.

#![allow(dead_code)]

#[path = "../examples/sim/baselines.rs"]
mod baselines;
#[path = "../examples/sim/swarm.rs"]
mod swarm;

use swarm::{HEALTHY, Kind, MIB, SEEKS, run};

#[test]
fn dwh_ready_matches_the_player() {
    for seed in 0..2 {
        for piece in [256 * 1024, 4 << 20] {
            let r = run(&HEALTHY, piece, seed, Kind::Dwh);
            assert!(r.ttp.is_some(), "seed {seed} piece {piece}: never playable");
            assert_eq!(r.ready_ttp, r.ttp, "seed {seed} piece {piece}");
            assert_eq!(r.seeks.len(), SEEKS);
            assert_eq!(r.ready_seeks, r.seeks, "seed {seed} piece {piece}");
            assert!(r.seeks.iter().all(Option::is_some));
            // Duplicates stay near the 5% hedge budget.
            assert!(
                r.dup <= r.payload / 20 + MIB,
                "dup {} of {}",
                r.dup,
                r.payload
            );
        }
    }
}

#[test]
fn baselines_reach_playback() {
    for kind in [Kind::Rqbit, Kind::Stremio] {
        let r = run(&HEALTHY, 256 * 1024, 0, kind);
        assert!(r.ttp.is_some(), "{kind:?} never playable");
    }
}
