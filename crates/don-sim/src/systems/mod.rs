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
//! Recovery note: `items.rs` and `naval.rs` are compiled here after post-cutoff audits fixed
//! their local contract defects. Both remain Tier-C integration boundaries: declaring them
//! runs their tests, but does not wire either subsystem into `World::step` or replay state.

/// Added by `mech:air`. Serves no checksum channel of its own — it is the air-domain
/// slice of the `units` channel plus the `Ammo::init` anti-air gate that the `ammo`
/// channel depends on. Uses [`crate::rng::Random`], so it needs the crate to build.
pub mod air;
pub mod ammo;
/// Recovered from the cut-off `cg:armies` lane. `Army`/`Armies` is the standing
/// formation-of-groups layer executed at step 13 of `Game::do_frame`. The module keeps
/// unported target-selection/RNG work in an explicit gap ledger.
pub mod armies;
pub mod borders_fog;
/// Tier-C spell-lifecycle and wildlife/RNG primitives recovered from the interrupted
/// casters/animals lane. Full casting, object allocation, hunting, tick and checksum
/// integration remain explicit boundaries; see `docs/mechanics/casters-animals.md`.
pub mod casters_animals;
/// Recovered from the cut-off retail collision lane. Ports the occupancy bitmap,
/// per-guy footprint stamping, blocker detection, local detour/wait/repath resolver, and
/// collision-block reaper; remaining integration boundaries are explicit in the module.
pub mod collision;
/// Added by `mech:combat`. `std`-only, no crate-root dependencies, so it builds as soon as
/// `lib.rs` declares `pub mod systems;`. Verified standalone with
/// `rustc --edition 2021 --test src/systems/combat.rs` — 66 tests, all green, including a
/// load of the real 493x493 balance matrix from `schema/live/balance-real.bin`.
pub mod combat;
pub mod economy;
pub mod groups_guys;
/// Recovered from an interrupted lane and audited as a Tier-C goody-box registry/checksum
/// primitive. Object-chain, movement-caller, replay, and terrain-transaction integration
/// remain explicit boundaries; see `docs/mechanics/items.md`.
pub mod items;
/// Added by `economy-step8`. `Leaders::process_all` `0x006ED2A0` — step 8 of `do_frame`,
/// the caller `economy.rs` never had, plus the second level `Leader::gather` reaches:
/// the `BitMask<44>` union that arms the two stat-dirty bits, `calc_wall_stats`,
/// `calc_unit_stats`, `calc_attrition` and `calc_anti_attrition`. Serves the `leaders`
/// channel jointly with `economy` and `victory_score`; it deliberately does **not**
/// re-port `Leader::process_elimination`, which `victory_score` already owns.
pub mod leaders;
pub mod map_terrain;
pub mod movement;
/// Recovered from an interrupted lane and audited as Tier-C naval primitives and guardrail
/// proxies. It is intentionally not a retail-complete naval executor; see
/// `docs/mechanics/naval.md`.
pub mod naval;
/// Added by `assembly:order-dispatch`. `Unit::work` `0x0060D180` and a real `Unit::do_job`
/// `0x00617A10` dispatch over an `OrderList` with retail's cursor semantics — the driver
/// `docs/mechanics/COVERAGE.md` §3 records as uncited. It is [`movement`]'s first caller.
pub mod order_dispatch;
/// Added by `build:sim-core` when wiring `systems` into `lib.rs`: both modules were
/// present on disk with no `pub mod` line, which is exactly the silent stranding this
/// file's header warns about.
pub mod production;
/// Added by `assembly:target-selection`. `Object::find_nearby_target` `0x00648DA0`,
/// `Object::compare_target` `0x0064E5C0` and the `World::wdata` acquisition grid — the
/// half of combat that chooses what `crate::mechanics::damage` is pointed at. Depends on
/// [`combat`], [`crate::mechanics`] and [`crate::trig`].
pub mod target;
pub mod tech_cities;
/// Recovered from the cut-off `cg:unit-inctime` lane. Ports `Unit::inc_time` and the
/// fixed-owner traversal of step 15 while recording the still-missing animation RNG and
/// event-execution paths.
pub mod unit_inctime;
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
