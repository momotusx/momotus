//! Stream selection: normalize addon streams into typed facts, gate what
//! cannot or should not play, rank the rest with device capabilities, and
//! explain the pick. Pure: no I/O, no clock.

mod model;
mod normalize;
mod rank;

pub use model::*;
pub use normalize::{key_digest, normalize, target_digest};
pub use rank::{
    RankInput, by_engine, guessed_rate, kind, rank, rank_kept, screen_class, title_similarity,
    window_rate,
};
