//! Per-subsystem ports of the retail simulation, one module per checksum channel.
//!
//! Each module here is derived from `ron-bin/riseofnations.exe` + `ron-bin/sbl/rise.pdb`
//! and states, in its own header, which `CheckSums::check_all` (`0x00936560`) channel it
//! serves and how its state is walked.
//!
//! **This file is shared across lanes.** It was created by the `mech:ammo` lane and lists
//! every sibling module present at that time. Lanes adding a module here must add their
//! own `pub mod` line; a missing line silently strands the module.
//!
//! Integration status is tracked in `docs/mechanics/COVERAGE.md` and `GOAL.md`. All modules
//! declared below compile in the workspace umbrella suite, but most are isolated ports rather
//! than executors reached from [`crate::world::World::step`]. A green module test is evidence
//! about its local derivation, not evidence that the retail tick runs it.
//!
//! Recovery note: `items.rs` and `naval.rs` also exist in this directory but are intentionally
//! not declared here yet. Post-cutoff isolated audits found unresolved test/contract problems
//! and fidelity gaps in both; see `docs/RECOVERY.md` before landing either one.

/// Added by `mech:air`. Serves no checksum channel of its own — it is the air-domain
/// slice of the `units` channel plus the `Ammo::init` anti-air gate that the `ammo`
/// channel depends on. Uses [`crate::rng::Random`], so it needs the crate to build.
pub mod air;
pub mod ammo;
pub mod borders_fog;
/// Added by `mech:combat`. `std`-only, no crate-root dependencies, so it builds as soon as
/// `lib.rs` declares `pub mod systems;`. Verified standalone with
/// `rustc --edition 2021 --test src/systems/combat.rs` — 66 tests, all green, including a
/// load of the real 493x493 balance matrix from `schema/live/balance-real.bin`.
pub mod combat;
pub mod economy;
pub mod groups_guys;
pub mod map_terrain;
pub mod movement;
/// Added by `build:sim-core` when wiring `systems` into `lib.rs`: both modules were
/// present on disk with no `pub mod` line, which is exactly the silent stranding this
/// file's header warns about.
pub mod production;
pub mod tech_cities;
/// Added by `mech:victory-score`. `std`-only, no crate-root dependencies, so it
/// builds as soon as `lib.rs` declares `pub mod systems;`. Verified standalone with
/// `rustc --edition 2021 --test src/systems/victory_score.rs` — 27 tests, all green.
pub mod victory_score;
/// Added by `mech:walls`. Depends only on `std` plus [`ammo::adler32`] — the two channels
/// share one hash primitive on purpose. Serves the `walls` channel of `check_all`, which
/// this lane measured to be **structurally empty** in this build (`obj_base[2] ==
/// obj_end[2] == 3000`); the walker is still exact because `BuildData::walk_data`
/// (`0x0062F270`) calls `WallData::walk_data` for every building in the `builds` channel.
pub mod walls;
