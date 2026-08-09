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
/// Exact ordinary `Unit::find_attack_pos` positioning transactions. Unit targets delegate
/// to the complete nearby-spot service; building targets retain perimeter order plus the
/// terrain/bitmap/ordered-collision/RNG gate order and fail closed without those views.
pub mod attack_position;
pub mod borders_fog;
/// Exact completed-Farm arm of `BuildTypeData::calc_gather`: six-slot LandData/river
/// evaluation, city/Japanese scaling, and the Egyptian Wealth side output. Unsupported
/// building types and type-data shapes fail closed.
pub mod building_gather;
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
/// Authoritative `BUILD_AT` lifecycle over the recovered production arithmetic.  Large
/// placement/start/activation/disband transactions are mandatory fail-loud host effects;
/// the module cannot silently fall back to builder-frame construction.
pub mod construction;
/// Exact unit-side `do_build`, completion, `build_done`, and candidate-balancing policy
/// around the fail-closed construction lifecycle.
pub mod construction_builder;
/// Exact placement, start, rejection/refund, activation, and farm-spawn transactions
/// behind construction's mandatory world-store boundary.
pub mod construction_lifecycle;
/// Source-backed `UnitType::find_nearby_spot`, exact bidirectional containment splice, and
/// the ordinary-gather release invariant (preserve its on-map collision stamp). Scholar
/// placement requires both the retail bitmap and ordered-collision host; an ordinary worker
/// modeled as seated is rejected rather than teleported.
pub mod containment;
pub mod economy;
/// Exact direct land-unit volley geometry: Unit/Guy aim, live-squad damage multiplicity,
/// composed flank direction, and a fail-closed graphics-turret boundary.
pub mod fight;
/// Exact ordinary Farm/Camp/Mine on-map attachment, building approach, queued movement,
/// and payout-activation boundary. It composes the gathering chain with both retail
/// collision views and never turns attachment into containment or teleportation.
pub mod gather_lifecycle;
/// Source-backed `rules.xml` LandData and generated Mountain/Cliff object materialization
/// for gathering. This is deliberately separate from Arena map storage: object identity,
/// list order and four-slot land payloads must survive ingestion before Arena can adapt it.
pub mod gather_terrain;
/// Retail gathering-site capacity boundary, owner-local worker chains, worker/order
/// lifecycle, rate scaling, and checksum-visible GatherOrder payload.
pub mod gathering;
/// Exact, fail-closed bridge from the supported installed `unit_graphics.xml` and a
/// retail hierarchy resolver to checksum-visible Guy graphics/turret state.
pub mod graphics_turret;
pub mod groups_guys;
pub mod held_target;
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
/// Exact `Mountains::randomize_mountains` three-list RNG/cursor transaction and
/// the return-then-advance `get_range` primitive used by terrain placement.
pub mod mountains;
pub mod movement;
/// Recovered from an interrupted lane and audited as Tier-C naval primitives and guardrail
/// proxies. It is intentionally not a retail-complete naval executor; see
/// `docs/mechanics/naval.md`.
pub mod naval;
/// Added by `assembly:order-dispatch`. `Unit::work` `0x0060D180` and a real `Unit::do_job`
/// `0x00617A10` dispatch over an `OrderList` with retail's cursor semantics — the driver
/// `docs/mechanics/COVERAGE.md` §3 records as uncited. It is [`movement`]'s first caller.
pub mod order_dispatch;
/// `AirPatrolOrder` / `GroupPatrolOrder` dynamic waypoint payloads and the exact
/// `Unit::do_air_patrol` / `Unit::do_patrol` state transitions. Kept separate from the
/// dispatcher because air physics and group movement are explicit host boundaries.
pub mod patrol;
/// Added by `build:sim-core` when wiring `systems` into `lib.rs`: both modules were
/// present on disk with no `pub mod` line, which is exactly the silent stranding this
/// file's header warns about.
pub mod production;
/// Exact post-coastline `Regions::clear_all` / `find_all` worldgen stage, including
/// component floods, overflow aggregation, coasts, ranks and coordinate rebuilds.
pub mod regions;
/// Added by `assembly:target-selection`. `Object::find_nearby_target` `0x00648DA0`,
/// `Object::compare_target` `0x0064E5C0` and the `World::wdata` acquisition grid — the
/// half of combat that chooses what `crate::mechanics::damage` is pointed at. Depends on
/// [`combat`], [`crate::mechanics`] and [`crate::trig`].
pub mod target;
pub mod tech_cities;
/// Exact `TerrainGroups::fill_fertile` worldgen pass plus the instruction-pinned,
/// fail-closed `place_all` boundary at the missing Mountains RNG/list state.
pub mod terrain_groups;
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
