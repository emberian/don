//! Simulation core: batch-parallel world storage and the fixed-tick scheduler.
//!
//! # What this crate is, and what it is not
//!
//! This is the **data layout and scheduler**, not the game mechanics. Layout is derived:
//! the engine binds each named attribute to a pointer held at a fixed struct offset (see
//! `docs/binary-ground-truth.md`), and those attribute arrays are parallel and
//! per-entity — a structure-of-arrays shape, which is also what batch simulation wants.
//!
//! **No mechanic in this crate is derived yet.** The systems below move bytes in the
//! access pattern the real systems will have, so the scheduler and layout can be measured
//! and tuned, but they compute nothing with fidelity to the original. Every one is marked
//! `PLACEHOLDER`. Per `docs/CHARTER.md` a mechanic may only be implemented once it is
//! derived from the binary — the oracle in `crates/oracle` is how that happens — so these
//! exist to be *replaced*, function by function, each arriving with a provenance ledger
//! entry and a fidelity tier. Do not read a fidelity claim into any number here.
//!
//! What the benchmark measures is therefore an **upper bound on throughput** for this
//! layout: the cost of touching the state a real tick must touch. Real mechanics only add
//! work, so treat it as a ceiling, never as a projected simulation speed.
//!
//! # Tick rate
//!
//! The engine's time unit is the *frame*, 1/15 s at normal speed — stated in the shipped
//! `rules.xml` header, which is game data rather than community documentation, so it is
//! usable ground truth. It has not yet been confirmed against the binary.

pub mod batch;
pub mod mechanics;
pub mod world;

pub use batch::Batch;
pub use mechanics::hash_into_range;
pub use world::{World, MAX_UNITS, TICK_HZ};
