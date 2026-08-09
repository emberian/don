//! **The arena** — one world model, two players in it, headless, deterministic.
//!
//! # The blockage this removes
//!
//! `docs/tracks/analytics-v2.md` names the state of play exactly: *two world models, one
//! arena still needed*. `don-ai` shipped a transcribed opening book that plays
//! [`crate::game::Game`] and an optimiser-derived player, [`crate::optimum::player::CapFirst`],
//! that plays [`crate::optimum::econ::World`] — a completely different abstraction. The
//! 46-second gap between them was measured **inside the second model, by the same code
//! that chose the thresholds**. It was not a result about a game and could not be, because
//! neither model had a map, an opponent, or a way to lose.
//!
//! This module is that arena. Both openings now run as [`bots::Bot`]s inside
//! [`world::World`], which has terrain, fog, units that walk, and the real damage chain.
//!
//! # Layout
//!
//! | module | what |
//! |---|---|
//! | [`types`] | the roster, from the live type tables (`TypeIndex` + every combat field) |
//! | [`map`] | terrain, the shipped placement radii, a provably fair generator |
//! | `gather_runtime` | persistent retail gather identity plus transactional exact lifecycle/refusal |
//! | [`cmd`] | the command set — each variant *is* a `don-env` unit verb, round-trippable to its ten action heads |
//! | [`world`] | the simulation: economy, construction, production, movement, combat, fog, defeat |
//! | [`obs`] | the fog-limited view a bot decides from |
//! | [`retail_systems`] | fail-closed MODEL 6 inventory and host adapters; not a world caller |
//! | [`bots`] | the players: `ShippedOpening`, `CapFirst`, `Marshal`, and a policy adapter |
//! | [`match_run`] | running a match and reporting it honestly |
//!
//! # The design constraint that shapes all of it
//!
//! A learned policy must be able to replace any bot without the world noticing. So a bot
//! sees [`obs::Obs`] (a snapshot, fog-limited, no back-door to `World`) and emits
//! [`cmd::Cmd`]s (each one a real engine verb, encodable as the exact ten-head vector
//! `don-env` gives an RL agent). [`bots::HeadPolicy`] is the adapter that makes that
//! literal: it takes `[i32; 10]` vectors and is a `Bot`.

pub mod bots;
pub mod cmd;
pub mod eval;
mod gather_runtime;
pub mod map;
pub mod match_run;
pub mod obs;
pub mod retail_systems;
pub mod types;
pub mod world;

pub use cmd::{Cmd, EntId};
pub use map::{Map, MapParams, Spatial};
pub use match_run::{run_match, MatchConfig, MatchResult};
pub use obs::Obs;
pub use types::Types;
pub use world::{ArenaParams, World};
