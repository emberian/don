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

//! # Layout, in one paragraph
//!
//! Units are stored dense: live rows are `0..live_count`, with identity carried by a
//! generational [`Handle`] rather than by a row number, so a tick system is a branch-free
//! pass over a contiguous prefix. Kernels for those passes live in [`simd`], each with a
//! scalar reference and a per-target vector path asserted bit-identical to it. Worlds are
//! independent, so [`Batch`] gets its parallelism from the batch dimension and produces
//! the same bits at every thread count.
//!
//! `docs/derivation/simd-batch.md` records the measurements behind those choices,
//! including the ones that did not pay.

pub mod batch;
pub mod generated;
pub mod interleave;
pub mod mechanics;
pub mod simd;
pub mod world;

pub use batch::Batch;
pub use interleave::LaneBatch;
pub use mechanics::{
    balance_index, damage, damage_traced, entrench_dir_level, flank_level, get_armor,
    get_attack, hash_into_range, CombatRules, DamageInput, DamagePredicates, UnreachedTerms,
    STEP_NAMES,
};
pub use mechanics::{
    attrition_fires, attrition_interval_scale, attrition_period_frames,
    attrition_recompute_due, clamp_cost_to_ramp_ceiling, commerce_cap, cost_ramp_ceiling,
    credit_resource, merge_attrition_period, ramped_rate, rate_after_game_option,
    resource_period, resource_tick, AttritionInput, AttritionPredicates, CommerceCapGates,
    EconomyRules, ResourceTick, ResourceTickInput, RES_FOOD, RES_KNOWLEDGE, RES_TIMBER,
    RES_WEALTH,
};
pub use world::{Handle, World, MAP_SPAN, MAX_UNITS, TICK_HZ};
