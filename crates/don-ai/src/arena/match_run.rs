//! Running a match, and reporting it without flattering anyone.
//!
//! # The result rule, stated once
//!
//! **An elimination is the only decisive result.** A player is eliminated when it owns no
//! city and no citizen — it can no longer rebuild anything. Everything else is a
//! [`Outcome::Timeout`], and a timeout is reported with its components (cities, army
//! value, buildings, resources gathered, damage dealt) rather than collapsed into one
//! invented number. A single "score" would let the arena decide who is better by choosing
//! weights, which is exactly the failure this project keeps paying for elsewhere.
//!
//! For head-to-head tables a timeout is ranked by *cities standing*, then *damage dealt*,
//! then *resources gathered*, and any run where that tiebreak decides the row says so.

use super::bots::Bot;
use super::cmd::Cmd;
use super::map::{Map, MapParams, Spatial};
use super::obs::Obs;
use super::types::Types;
use super::world::{ArenaParams, Score, World, FPS};
use crate::orders::OrderResult;
use don_sim::balance::BalanceTable;

/// One match's setup.
#[derive(Clone, Debug)]
pub struct MatchConfig {
    pub minutes: i64,
    pub map: MapParams,
    pub arena: ArenaParams,
    /// One nation index per player. Tribe 6 is the nation whose ancient roster is
    /// Hoplites / Bowmen / Slingers.
    pub tribes: Vec<u8>,
    pub logging: bool,
}

impl Default for MatchConfig {
    fn default() -> Self {
        MatchConfig {
            minutes: 20,
            map: MapParams::default(),
            arena: ArenaParams::default(),
            tribes: vec![6, 6],
            logging: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// One player was eliminated; the winner is the survivor.
    Elimination { winner: usize, frame: i64 },
    /// Time ran out.
    Timeout,
}

#[derive(Clone, Debug)]
pub struct MatchResult {
    pub outcome: Outcome,
    pub frames: i64,
    pub scores: Vec<Score>,
    pub names: Vec<String>,
    pub orders_ok: Vec<u32>,
    pub orders_refused: Vec<u32>,
    pub orders_invalid: Vec<u32>,
    pub commands: Vec<u64>,
    /// Frame each bot reported its boom goal met, if it tracks one.
    pub boom_frame: Vec<Option<i64>>,
    /// Flank levels the derived damage chain actually took, 0/1/2.
    pub flank_hist: [u64; 3],
    pub shots: u64,
    /// `(player, verb, "refused"|"invalid") -> count`. A bot spending its commands on
    /// orders the world rejects is broken in a way no score line shows.
    pub rejects: std::collections::BTreeMap<(u8, &'static str, &'static str), u32>,
    pub log: Vec<super::world::Event>,
}

impl MatchResult {
    /// The head-to-head verdict for a two-player run, with the reason.
    pub fn verdict(&self) -> (Option<usize>, &'static str) {
        match self.outcome {
            Outcome::Elimination { winner, .. } => (Some(winner), "elimination"),
            Outcome::Timeout => {
                if self.scores.len() != 2 {
                    return (None, "timeout");
                }
                let (a, b) = (&self.scores[0], &self.scores[1]);
                if a.cities != b.cities {
                    return (Some(usize::from(b.cities > a.cities)), "cities standing");
                }
                if a.damage_dealt != b.damage_dealt {
                    return (
                        Some(usize::from(b.damage_dealt > a.damage_dealt)),
                        "damage dealt",
                    );
                }
                if a.resources != b.resources {
                    return (Some(usize::from(b.resources > a.resources)), "resources");
                }
                (None, "dead heat")
            }
        }
    }
}

/// Load everything the arena needs from the repo's gitignored game data.
pub fn load_world(cfg: &MatchConfig) -> Result<World, String> {
    let types = Types::load_default()?;
    let spatial = Spatial::load(&crate::rules::default_data_dir().join("rules.xml"))?;
    let mut mp = cfg.map;
    mp.players = cfg.tribes.len();
    let map = Map::generate(mp, spatial);
    let balance = BalanceTable::load_default().map_err(|e| e.to_string())?;
    let mut w = World::new(types, map, balance, &cfg.tribes, cfg.arena)?;
    w.logging = cfg.logging;
    Ok(w)
}

/// Run one match. Deterministic: same config and same bots give the same result.
pub fn run_match(cfg: &MatchConfig, bots: &mut [Box<dyn Bot>]) -> Result<MatchResult, String> {
    let mut w = load_world(cfg)?;
    if bots.len() != w.players.len() {
        return Err(format!(
            "{} bots for {} players",
            bots.len(),
            w.players.len()
        ));
    }
    let limit = cfg.minutes * 60 * FPS;
    let mut commands = vec![0u64; bots.len()];
    let mut buf: Vec<Cmd> = Vec::new();
    let mut outcome = Outcome::Timeout;

    while w.frame < limit {
        for pi in 0..bots.len() {
            if !w.players[pi].alive {
                continue;
            }
            let period = bots[pi].decide_period().max(1);
            if w.frame % period != pi as i64 % period {
                continue;
            }
            buf.clear();
            {
                let obs = Obs::of(&w, pi);
                bots[pi].act(&obs, &mut buf);
            }
            for c in buf.drain(..) {
                commands[pi] += 1;
                let _: OrderResult = w.submit(pi as u8, c);
            }
        }
        w.step();
        let alive: Vec<usize> = (0..w.players.len())
            .filter(|&i| w.players[i].alive)
            .collect();
        if alive.len() <= 1 {
            outcome = match alive.first() {
                Some(&winner) => Outcome::Elimination {
                    winner,
                    frame: w.frame,
                },
                None => Outcome::Timeout,
            };
            break;
        }
    }

    Ok(MatchResult {
        outcome,
        frames: w.frame,
        scores: (0..w.players.len()).map(|i| w.score(i)).collect(),
        names: bots.iter().map(|b| b.name()).collect(),
        orders_ok: w.players.iter().map(|p| p.orders_ok).collect(),
        orders_refused: w.players.iter().map(|p| p.orders_refused).collect(),
        orders_invalid: w.players.iter().map(|p| p.orders_invalid).collect(),
        commands,
        boom_frame: vec![None; w.players.len()],
        flank_hist: w.flank_hist,
        shots: w.shots,
        rejects: w.rejects,
        log: w.log,
    })
}

/// `mm:ss` for a frame count.
pub fn clock(frames: i64) -> String {
    let s = frames / FPS;
    format!("{}:{:02}", s / 60, s % 60)
}
