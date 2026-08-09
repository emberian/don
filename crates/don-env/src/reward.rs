//! Reward: a named term vector plus a linear combination over it.
//!
//! # Why not a callback
//!
//! A Python reward callback would run once per agent per env per step and would put the
//! GIL on the hot path — at the throughput this env is built for that is the whole budget.
//! Instead the env emits the raw term vector (deltas of the engine's own `LeaderData`
//! counters, plus terminal win/loss) and the shaping is a weight vector the trainer sets.
//! Anything not expressible as a linear combination can still be computed in Python from
//! the exposed term array, which is returned every step; nothing is hidden behind the
//! weights.

use crate::generated as g;
use crate::state::{EnvWorld, ScoreTerms};

/// Reward term names, in the order they appear in the term buffer.
///
/// Terms 0..11 are the eleven `LeaderData` score fields, per-step *deltas*. Terms 11..17
/// are deltas of the `LeaderData` outcome counters. The last three are terminal.
pub const TERMS: [&str; 20] = [
    "d_score_explored",
    "d_score_territory",
    "d_score_units",
    "d_score_units_2",
    "d_score_buildings",
    "d_score_economy",
    "d_score_pop",
    "d_score_unit_upgrades",
    "d_score_research",
    "d_score_wonders",
    "d_score_combat",
    "d_units_built",
    "d_units_killed",
    "d_units_lost",
    "d_buildings_built",
    "d_buildings_lost",
    "d_econ_total",
    "win",
    "loss",
    "alive",
];
pub const N_TERMS: usize = TERMS.len();
pub const IDX_WIN: usize = 17;
pub const IDX_LOSS: usize = 18;
pub const IDX_ALIVE: usize = 19;

/// Snapshot of one player, taken before a step so the deltas are exact.
#[derive(Clone, Copy, Default)]
pub struct RewardSnapshot {
    score: [i32; 11],
    units_built: i32,
    units_killed: i32,
    units_lost: i32,
    buildings_built: i32,
    buildings_lost: i32,
    econ_total: i32,
    alive: bool,
}

pub fn snapshot(w: &EnvWorld, who: u8) -> RewardSnapshot {
    let p = &w.players[who as usize];
    RewardSnapshot {
        score: p.score.as_array(),
        units_built: p.units_built,
        units_killed: p.units_killed,
        units_lost: p.units_lost,
        buildings_built: p.buildings_built,
        buildings_lost: p.buildings_lost,
        econ_total: p.econ.iter().sum(),
        alive: p.alive,
    }
}

/// Fill `terms` (length [`N_TERMS`]) with the deltas since `before`, and return whether
/// the episode ended for this agent.
pub fn write_terms(w: &EnvWorld, who: u8, before: &RewardSnapshot, terms: &mut [f32]) -> bool {
    let p = &w.players[who as usize];
    let now = p.score.as_array();
    for k in 0..11 {
        terms[k] = (now[k] - before.score[k]) as f32;
    }
    terms[11] = (p.units_built - before.units_built) as f32;
    terms[12] = (p.units_killed - before.units_killed) as f32;
    terms[13] = (p.units_lost - before.units_lost) as f32;
    terms[14] = (p.buildings_built - before.buildings_built) as f32;
    terms[15] = (p.buildings_lost - before.buildings_lost) as f32;
    terms[16] = (p.econ.iter().sum::<i32>() - before.econ_total) as f32;

    let alive: Vec<usize> = (0..g::NUM_PLAYERS)
        .filter(|&i| w.players[i].alive)
        .collect();
    let last_standing = alive.len() == 1 && alive[0] == who as usize;
    let just_died = before.alive && !p.alive;
    terms[IDX_WIN] = if last_standing { 1.0 } else { 0.0 };
    terms[IDX_LOSS] = if just_died || (!p.alive && before.alive) {
        1.0
    } else {
        0.0
    };
    terms[IDX_ALIVE] = if p.alive { 1.0 } else { 0.0 };
    last_standing || !p.alive
}

/// Linear shaping over [`TERMS`].
#[derive(Clone, Debug)]
pub struct RewardSpec {
    pub weights: [f32; N_TERMS],
}

impl Default for RewardSpec {
    /// Sparse outcome reward. Shaping is opt-in on purpose: a shaped default is a hidden
    /// prior on how the game should be played, and this project has no measurement to
    /// justify one.
    fn default() -> Self {
        let mut weights = [0.0f32; N_TERMS];
        weights[IDX_WIN] = 1.0;
        weights[IDX_LOSS] = -1.0;
        RewardSpec { weights }
    }
}

impl RewardSpec {
    pub fn set(&mut self, name: &str, w: f32) -> bool {
        match TERMS.iter().position(|t| *t == name) {
            Some(i) => {
                self.weights[i] = w;
                true
            }
            None => false,
        }
    }
    #[inline]
    pub fn combine(&self, terms: &[f32]) -> f32 {
        self.weights.iter().zip(terms).map(|(a, b)| a * b).sum()
    }
}

/// The engine's own score for a player: the eleven `LeaderData` fields plus our unweighted
/// total. `Leader::compute_score` `0x006EC560` is unread, so the *aggregation* is ours;
/// the eleven components are the engine's.
pub fn engine_score(w: &EnvWorld, who: u8) -> (ScoreTerms, i32) {
    let s = w.players[who as usize].score;
    (s, s.total())
}
