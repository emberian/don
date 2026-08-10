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
/// Exact snapshot owner for AIR_PATROL's mod-32 `find_building_at` traversal and dynamic
/// `BuildData::ever_seen` gate. Product adapters still require one coherent spatial host.
pub mod air_patrol_building_search_frontier;
/// Exact AIR/BOMBER finder-call sequence, ordered scratch folding, inverse fallback, and
/// patrol acceptance contract. Object enumeration and target predicates remain hosted.
pub mod air_patrol_unit_search_frontier;
/// Complete top-level `Unit::do_air_physics` control/state transaction with nested flight,
/// collision, fuel, path, animation, and RNG mutations represented by atomic receipts.
pub mod air_physics_frontier;
pub mod ammo;
/// Recovered from the cut-off `cg:armies` lane. `Army`/`Armies` is the standing
/// formation-of-groups layer executed at step 13 of `Game::do_frame`. The module keeps
/// unported target-selection/RNG work in an explicit gap ledger.
pub mod armies;
/// Exact ordinary `Unit::find_attack_pos` positioning transactions. Unit targets delegate
/// to the complete nearby-spot service; building targets retain perimeter order plus the
/// terrain/bitmap/ordered-collision/RNG gate order and fail closed without those views.
pub mod attack_position;
/// Exact registrations 508--510 wrapper/prefix reversal. Runtime integration consumes this
/// split plan so the native post-clear authority boundary remains non-atomic.
pub mod bhs_create_unit_frontier;
/// Composition-bound, receipt-bearing execution of the fully owned create-unit prefix and
/// native rejection/zero-count subset. Positive allocation remains explicitly host-gated.
pub mod bhs_create_unit_runtime;
/// Exact fail-closed Types prefix of checksum channel 13 over the canonical mutable owner.
pub mod bhs_type_channel13_frontier;
/// Exact synchronized-rules producer and immutable-backup provenance for the BHS type owner.
pub mod bhs_type_factory;
/// Exact declaration gate, dispatch receipts, and persistence/checksum admission for that owner.
pub mod bhs_type_runtime;
/// Instruction-derived BHS type-stat transactions and their canonical owner commit boundary.
pub mod bhs_type_stat_frontier;
/// Canonical mutable `Types[806]` / Leader-mask owner for the recovered BHS type cohort.
pub mod bhs_type_table;
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
/// Persistent live bridge for retail's collision-block reaper cursor.
pub mod collision_blocks_live;
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
/// Exact `DeathObj::inc_time` corpse-clock transaction and fail-closed live host seam.
pub mod death_inctime;
/// Exact defeated-player Unit-band dispatch: true planes die, other valid units close
/// orders, and both branches clear `unit_masks & 0x40000`.
pub mod defeat_cleanup;
pub mod economy;
/// Canonical policy-facing external Unit frame owner, including exact cloak/detection/fog
/// projection and revision-bound target ordinals.
pub mod external_entity_visibility_frontier;
/// Exact direct land-unit volley geometry: Unit/Guy aim, live-squad damage multiplicity,
/// composed flank direction, and a fail-closed graphics-turret boundary.
pub mod fight;
/// Exact `Unit::do_follow` planner and atomic host receipt. The live order dispatcher owns
/// its concrete queue/effect integration.
pub mod follow_executor;
/// Exact step-12 GameDaemon scheduler state and child-call transaction.
pub mod game_daemon_step12;
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
/// Exact eight-record `Game::do_frame` step-19 reconstruction. The canonical Leader owner
/// adapts through this receipt-bearing executor so product reads and presentation tails stay
/// explicit instead of being collapsed into successful headless calls.
pub mod leaders_process_event_frame_step19;
pub mod map_terrain;
/// Exact `Mountains::randomize_mountains` three-list RNG/cursor transaction and
/// the return-then-advance `get_range` primitive used by terrain placement.
pub mod mountains;
pub mod movement;
/// Typed, fail-closed transaction joining `Unit::move_step`'s event sequence to the recovered
/// unit collision detector/resolver, including actor-store, pathfinder and RNG writeback.
pub mod movement_driver;
/// Authoritative generated-column/ObjectRegistry/Guy-stamp adapter which makes the recovered
/// movement collision transaction reachable from the executable tick without empty-map defaults.
pub mod movement_live;
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
/// Canonical frame-zero PlayerSetup owner and atomic manual cohort/team activation.
pub mod player_setup;
/// Added by `build:sim-core` when wiring `systems` into `lib.rs`: both modules were
/// present on disk with no `pub mod` line, which is exactly the silent stranding this
/// file's header warns about.
pub mod production;
/// Exact post-coastline `Regions::clear_all` / `find_all` worldgen stage, including
/// component floods, overflow aggregation, coasts, ranks and coordinate rebuilds.
pub mod regions;
/// Exact `Unit::do_repair` branch/arithmetic planner plus its atomic dispatcher receipt.
/// The separately recovered `check_target_path(REPAIR)` predicate remains an open seam.
pub mod repair_order;
/// Complete step-22 `Roads::scan_and_kill_stray_roads` dispatcher/body over the PDB road
/// candidate lattice. Missing renderer-owned candidate facts fail closed rather than being
/// guessed from terrain adjacency.
pub mod roads;
/// Retail `ChunkHeader`-exact, fail-closed deterministic save/load for the currently
/// authoritative seed/frame/RNG, terrain, unit/object/order/path, and leader-economy state.
/// It is explicitly not a partial `.svx` writer: unsupported live subsystems are refused.
pub mod save_load;
/// Exact retail setup-team lookup and runtime/static team predicate.
pub mod setup_diplomacy;
/// Source-exact SPECIAL_ANIM planner plus the typed atomic receipt consumed by the live order
/// dispatcher. The production [`crate::tick::Sim`] frame bridge remains an explicit red seam.
pub mod special_anim_executor;
/// Added by `assembly:target-selection`. `Object::find_nearby_target` `0x00648DA0`,
/// `Object::compare_target` `0x0064E5C0` and the `World::wdata` acquisition grid — the
/// half of combat that chooses what `crate::mechanics::damage` is pointed at. Depends on
/// [`combat`], [`crate::mechanics`] and [`crate::trig`].
pub mod target;
/// Exact, host-fact-checked plans for targeted order executors recovered independently of
/// the shared order dispatcher integration.
pub mod targeted_order_plans;
/// Deterministic non-ranked `Game::init_teams` mutation plan consumed by `player_setup`.
pub mod team_setup_mutation;
pub mod tech_cities;
/// Exact Tech Race tail of `Leader::gain_tech`: end-age/all-epoch predicates,
/// typed opponent progress notices, and synchronous handoff to the terminal victory
/// transaction. The step-14 production adapter remains an explicit live seam.
pub mod tech_race;
/// Exact planners for the terminal CHANGE_FORM and THINK order arms.
pub mod terminal_order_plans;
/// Exact first (bush-fringe) pass of `TerrainGroups::add_doobers`, through the
/// second pass's external doober-occupancy query.
pub mod terrain_doobers;
pub mod terrain_drop_tile;
/// Exact `TerrainGroups::fill_fertile`, mountain/list RNG, grouped selection,
/// external host-event order, and clump-size preparation through placement kernels.
pub mod terrain_groups;
pub mod terrain_player_group;
pub mod terrain_player_growth;
pub mod terrain_player_mountain_retry;
pub mod terrain_region_continuation;
pub mod terrain_region_patterns;
pub mod terrain_region_placement;
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
/// Completed-Wonder registry reached from `Build::activate`, including exact slot reuse,
/// timers, close/trim behavior, and the mandatory object/type/game-store bridge that
/// supplies `victory_score` with live Wonder value/net inputs.
pub mod wonders;
