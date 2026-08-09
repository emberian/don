//! Replay-driven validation for the Descent of Nations simulation.
//!
//! # What this crate does
//!
//! A multiplayer `.rcx` is a lockstep command stream plus, on **every turn, for
//! every player**, a 65-byte `CheckSumsCommand` carrying sixteen 32-bit words:
//! fifteen `DataWalk` adler-32 channels over the whole simulation state, and
//! their wrapping sum. That is a per-turn, per-subsystem ground-truth oracle
//! that costs a parser rather than an emulator.
//!
//! This crate feeds that stream to a simulation, computes the same sixteen
//! channels over *our* state with the traversal generated from
//! `schema/state-schema.json`, and reports the **first diverging turn and which
//! channel diverged**.
//!
//! ```text
//!   .rcx ──gzip──▶ payload ──find_stream──▶ CommandPackage records
//!                                              │
//!                    ┌─────────────────────────┴──────────────────────┐
//!                    ▼                                                ▼
//!            commands ──▶ Order ──▶ Simulation::apply            CheckSumsCommand
//!                                        │                        (16 × u32)
//!                                        ▼                             │
//!                                 Simulation::check_all ──── compare ──┘
//!                                                              │
//!                                                    per-channel survival
//! ```
//!
//! # What the number means
//!
//! `survived` is the count of consecutive turns, from the recording's first
//! checksummed turn, on which our channel value equalled retail's. It is
//! reported per channel, with the subset that agreed **only because both sides
//! walked zero bytes** broken out as `trivial`. Early divergence is the
//! expected result today and is not hidden; the number is the project's
//! progress metric, so flattering it would defeat the point.
//!
//! # Fidelity
//!
//! Tier C. The checksum *primitive* is Tier B (500,000 differential calls into
//! the retail `adler32` at `0x00a46830`, 0 mismatches). The *traversal* is
//! generated from a static extraction with documented gaps. Nothing here is
//! verified in the proof-assistant sense.

#![forbid(unsafe_code)]

pub mod check_all;
pub mod checksum;
pub mod continent;
pub mod harness;
pub mod image;
pub mod initial;
pub mod map_style;
pub mod replay;
pub mod report;
pub mod rules_channel;
pub mod scenario_channel;
pub mod script_channel;
pub mod state;
pub mod walk;
pub mod wire;

pub mod walk_gen;
mod wire_gen;

pub use check_all::{check_all, CheckAll, CheckSumsRecord};
pub use checksum::{adler32, Channel, Channels, CheckSum, DataWalk, CHANNEL_NAMES};
pub use continent::{
    execute_continent_prefix, ContinentError, ContinentReceipt, ContinentStop, LandDistanceCall,
    RegionSeedCall,
};
pub use harness::{format_table, run, NullSim, Phase, RunResult, Simulation};
pub use initial::{
    InitialGame, InitialGameInfo, InitialItemBoundary, InitialItemReconstruction,
    InitialItemReconstructionError, InitialPlayer, InitialState, InitialWorld,
    InitialWorldgenInputs, ReplayByteSpan, WorldgenSourceSpans,
};
pub use map_style::{
    MapGenerationStage, MapStyleIdentity, MapStyleLoadError, MapStyleStaticData,
    StaticFileEvidence, StaticXmlEntry, MAP_MAKE_SCHEDULE, SHIPPED_MAP_STYLE_CATALOG,
};
pub use replay::{corpus, Replay};
pub use state::SimState;
pub use walk::{walk_class, WalkOp, WalkOutcome, WalkSpec};
pub use walk_gen::{class_index, SPECS};
pub use wire::{classify, CommandClass, CommandView, Order};
