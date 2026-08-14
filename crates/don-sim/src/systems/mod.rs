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
/// Exact `UnitData::is_busy` CastOrder spell-virtual OR and SpecialAnim tail.
pub mod air_busy_authority;
/// Atomic LaunchPatrol/Scramble transaction over an exact Group packet pair.
pub mod air_group_action_transaction;
/// Exact snapshot owner for AIR_PATROL's mod-32 `find_building_at` traversal and dynamic
/// `BuildData::ever_seen` gate. Product adapters still require one coherent spatial host.
pub mod air_patrol_building_search_frontier;
/// Exact AIR/BOMBER finder-call sequence, ordered scratch folding, inverse fallback, and
/// patrol acceptance contract. Object enumeration and target predicates remain hosted.
pub mod air_patrol_unit_search_frontier;
/// Complete top-level `Unit::do_air_physics` control/state transaction with nested flight,
/// collision, fuel, path, animation, and RNG mutations represented by atomic receipts.
pub mod air_physics_frontier;
/// Exact AIR_PATROL dynamic payload plus synchronized type/scenario authority codecs.
pub mod air_runtime_authority;
pub mod ammo;
/// Recovered from the cut-off `cg:armies` lane. `Army`/`Armies` is the standing
/// formation-of-groups layer executed at step 13 of `Game::do_frame`. The module keeps
/// unported target-selection/RNG work in an explicit gap ledger.
pub mod armies;
/// Exact zero-group close prefix of retail `Army::do_defending`.
pub mod army_do_defending;
/// Complete 337-byte no-RNG `Army::do_mustering` state-machine body.
pub mod army_do_mustering;
/// Exact ordinary `Unit::find_attack_pos` positioning transactions. Unit targets delegate
/// to the complete nearby-spot service; building targets retain perimeter order plus the
/// terrain/bitmap/ordered-collision/RNG gate order and fail closed without those views.
pub mod attack_position;
/// Canonical City/Build census for BHS builtin 386, `num_city_buildings`.
pub mod bhs_city_building_runtime;
/// Exact registrations 508--510 wrapper/prefix reversal. Runtime integration consumes this
/// split plan so the native post-clear authority boundary remains non-atomic.
pub mod bhs_create_unit_frontier;
/// Composition-bound, receipt-bearing execution of the fully owned create-unit prefix and
/// native rejection/zero-count subset. Positive allocation remains explicitly host-gated.
pub mod bhs_create_unit_runtime;
/// Canonical live-Unit census for BHS builtin 455, `find_num_idle_unit`.
pub mod bhs_idle_unit_runtime;
/// Exact owned prefix and native transaction boundary for BHS builtin 520.
pub mod bhs_place_building_runtime;
/// Canonical Build-queue census for BHS builtin 436, `num_type_queued`.
pub mod bhs_type_queue_runtime;
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
pub mod canonical_air_group_host;
/// Exact decoded retail package shell surrounding one canonical Group/AIR action pair.
pub mod canonical_air_package_shell;
/// Narrow canonical host for the current-STRAFE retarget arm of opcode-28 Flight.
pub mod canonical_flight_strafe_host;
/// Exact combined cached Patrol then fresh Flight aircraft transaction.
pub mod canonical_patrol_flight_host;
/// Canonical ordinary-aircraft AIR_PATROL activation and STRAFE insertion.
pub mod canonical_air_patrol_runtime;
/// Canonical bounded opcode-9 receiver and resumed ground-order hold frame.
pub mod canonical_attack_ground_runtime;
/// Installed, revision-bound production adapter for the fresh saved Village BUILD_AT tick.
pub mod canonical_build_at_work;
/// Atomic Board/Repair/Trade packet planner sharing the canonical Group→Move selector.
pub mod canonical_economy_group_host;
/// Sim-owned atomic opcode-0 Group plus opcode-7 MoveTo command transaction.
pub mod canonical_group_move_host;
/// Canonical bounded `[Group][GUARD]` installer and saved periodic-idle frame owner.
pub mod canonical_guard_runtime;
/// Installed, revision-bound production adapter for exact saved Camp Gather work.
pub mod canonical_gather_work;
/// Installed, revision-bound production adapter for the fresh Fishermen DEPLOY wait frame.
pub mod canonical_cast_work;
/// Canonical fixed-Group transaction for the strict Group plus UNITMASK packet cohort.
pub mod canonical_simple_group_host;
/// Bounded exact opcode-2 Group stance transaction for ordinary Unit stance types 0..=3.
pub mod canonical_stance_runtime;
/// Canonical live-World/path/RNG/animation/ammo adapter for STRAFE row 16.
pub mod canonical_strafe_runtime;
/// Exact fail-closed production adapter for a substantive `TRADE_ROUTE` Unit::work branch.
pub mod canonical_trade_route_runtime;
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
/// Atomic opcode-38 declaration transaction over command, resource, relation, vision,
/// victory, army, and object projections. Shared Bridge/Sim/save mounting remains gated.
pub mod canonical_diplomacy_host;
/// Real Bridge-to-Sim opcode-38 mount; reached external authority remains fail-closed.
pub mod canonical_diplomacy_runtime;
/// Full `Leader::action_respond(target, 1)` transaction reached by synchronized opcode 41.
pub mod diplomacy_accept_host;
/// Exact simulation/presentation split for the ordinary accepted-deal callbacks.
pub mod diplomacy_deal_callbacks;
pub mod diplomacy_declare_host;
/// Digest- and identity-bound callback answers for alliance-revocation Unit ejection.
pub mod diplomacy_ejection_authority;
/// Exact armies-off, empty-retirement, and empty-human-rally diplomacy processing.
pub mod diplomacy_force_army_authority;
pub mod economy;
/// Canonical fixed-`Groups` contract for economy/containment action transactions.
pub mod economy_containment_group_host;
/// Lossless v13 economy order-node payload and metric authority.
pub mod economy_order_payload_authority;
/// Canonical policy-facing external Unit frame owner, including exact cloak/detection/fog
/// projection and revision-bound target ordinals.
pub mod external_entity_visibility_frontier;
/// Exact direct land-unit volley geometry: Unit/Guy aim, live-squad damage multiplicity,
/// composed flank direction, and a fail-closed graphics-turret boundary.
pub mod fight;
/// Exact `Unit::do_follow` planner and atomic host receipt. The live order dispatcher owns
/// its concrete queue/effect integration.
pub mod follow_executor;
/// Detached, source-exact golden-2024 human Scout `think_spellcaster` transaction. It owns
/// the no-cast arms and stops at typed retail children or pool-14 allocation before CastOrder.
pub mod frame0_scout_spellcaster;
/// Revision-bound adjacent-call authority for the golden frame-one Scout's
/// `Caster::process_spells` child. Only a captured empty, unchanged engine array completes;
/// nonempty queues remain typed residuals.
pub mod frame1_caster_process;
/// `GameDaemon::calc_danger` `0x00732D10` and `GameDaemon::do_danger` `0x00732390` — the
/// step-12 child that rewrites the eight `World::danger` planes.
pub mod game_daemon_calc_danger;
/// Exact step-12 GameDaemon scheduler state and child-call transaction.
pub mod game_daemon_step12;
/// Detached post-`Wall::process` planner for the golden frames 2..=31 Village/Market
/// Build order, exact common-prefix children, and first type-specific continuation.
pub mod golden_build_post_wall;
/// Detached exact owner-0 early-return attrition transactions at golden frames 26..=32.
pub mod golden_phase32_attrition;
/// `do_job` arm 26: the snapshot-bound atomic host boundary and queue integration that turns
/// [`garrison_order`]'s pure transcription into a dispatched `Unit::do_garrison`.
pub mod garrison_dispatch;
/// Source-exact `Unit::do_garrison` `0x005E6B80` branch/effect transcription and install plan.
pub mod garrison_order;
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
/// Exact 1,022-byte `Group::action_trade` transaction and order-install chronology.
pub mod group_action_trade_frontier;
pub mod groups_guys;
/// Detached, fail-closed `Groups::process` normalization transaction over the canonical
/// fixed pool. Unit liveness/backlinks come from `World`; role and dynamic leader speed come
/// from the same Handle-bound authority used by the canonical Group-Move host.
pub mod groups_process_authority;
/// Fail-closed product authority join for canonical Group→Move packages.
pub mod group_move_authority;
/// `do_job` arm 12: the snapshot-bound atomic host boundary, concrete payload and same-tick
/// movement integration that turns [`guard_order`]'s pure transcription into a dispatched
/// `Unit::do_guard`.
pub mod guard_dispatch;
/// Source-exact `Unit::do_guard` `0x005E5C70` branch/effect transcription and install plan.
pub mod guard_order;
pub mod held_target;
/// `Group::action_hotkey` `0x006FA7A0` — the control-group store `Console::on_key_down`
/// reaches, and the same state transition opcode 34 inlines.
pub mod hotkey_group_action;
/// Recovered from an interrupted lane and audited as a Tier-C goody-box registry/checksum
/// primitive. Object-chain, movement-caller, replay, and terrain-transaction integration
/// remain explicit boundaries; see `docs/mechanics/items.md`.
pub mod items;
/// Option-independent `Leader::init` diplomacy prefix for active frame-zero teammates.
pub mod leader_init_diplomacy;
/// Complete source-only `Leader::init` diplomacy/shared-vision eight-target loop.
pub mod leader_init_diplomacy_loop;
/// Complete read-only `LeaderData::get_target` leaf over Game start order.
pub mod leader_get_target_runtime;
/// Added by `tick11-production-ai`. `Leader::plan_strategy` `0x006B9620`'s entry and
/// dispatch skeleton plus the whole 628-byte `Leader::production_ai` `0x006C1960` step
/// machine — the only door into retail's compiled production pipeline, which nothing but
/// tick step 11 can open. The eight stage bodies stay named boundaries.
pub mod leader_production_ai;
/// Atomic `Leader::set_diplo` `0x006EC6A0` transaction, plus the `action_respond(..., 1)`
/// resource movement opcode 41 reaches. Registered 2026-08-11: the body was written but had
/// no `mod` declaration anywhere in the library, so it compiled only from its own test file
/// and no consumer could reach the one authority the diplomacy blockers name.
pub mod leader_set_diplo;
/// Exact canonical Leader tribe-bonus query and complete `LeaderData::get_diff`.
pub mod leader_tribe_bonus_runtime;
/// Atomic publication of production-owned current tech and ages into the victory and
/// step-8 duplicate views. Production completion preflights this before mutation.
pub mod leader_tech_sync;
/// Atomic dual-mirror Leader pending-bit transaction reached by type-50/51 `Unit::think`.
pub mod leader_unit_think_pending;
/// Atomic canonical Leader accounting for the golden frame-zero free Market activation.
pub mod leader_market_build_accounting;
/// `Leader::process_taunt` `0x006B8CC0`, whole — step 8's last unported child. The tribute
/// arms stage a two-sided `Diplomacy::offers` ledger through `Leader::action_clear_all`
/// `0x006D15E0` and `Leader::action_offer` `0x006D1780`; the build arms rewrite and clamp
/// the six `LeaderData` AI build-priority scalars. Presentation leaves through a typed
/// outbox. `Leader::action_respond` `0x006D03C0` is the one named boundary.
pub mod leader_process_taunt;
/// Exact type-owned `BuildTypeData::find_friends` early return and generic first-object child.
pub mod build_type_find_friends;
/// Exact negative-City-constraint `BuildTypeData::blocked_site` entry prefix.
pub mod leader_produce_building_blocked_site_prefix;
/// Exact candidate rejection loop through the first `BuildTypeData::blocked_site` call.
pub mod leader_produce_building_candidate_prefix;
/// Exact read-only City/target-Type gate at the start of `Leader::produce_building`.
pub mod leader_produce_building_prefix;
/// Exact frame-zero coordinate/radius/footprint setup before the placement candidate loop.
pub mod leader_produce_building_search_setup;
/// Added by `economy-step8`. `Leaders::process_all` `0x006ED2A0` — step 8 of `do_frame`,
/// the caller `economy.rs` never had, plus the second level `Leader::gather` reaches:
/// the `BitMask<44>` union that arms the two stat-dirty bits, `calc_wall_stats`,
/// `calc_unit_stats`, `calc_attrition` and `calc_anti_attrition`. Serves the `leaders`
/// channel jointly with `economy` and `victory_score`; it deliberately does **not**
/// re-port `Leader::process_elimination`, which `victory_score` already owns.
pub mod leaders;
/// Source-only opening cone of `Leader::diplomacy` `0x006BC950`. Registered by
/// `tick11-production-ai`: like `leader_set_diplo` before it, the body existed with no
/// `mod` declaration anywhere in the library and compiled only from its own test file, so
/// step 11 could not reach the one derivation of its own diplomacy child. `strategy_all`
/// now executes its three-condition entry gate; the scan and target loop still want a
/// tick-side hook (see `docs/assembly/leader-production-ai-step11.md`).
pub mod leaders_diplomacy_opening_frontier;
/// Exact eight-record `Game::do_frame` step-19 reconstruction. The canonical Leader owner
/// adapts through this receipt-bearing executor; its exact ordered outbox owns every reached
/// presentation/achievement call without fabricating wall-clock audio behavior.
pub mod leaders_process_event_frame_step19;
pub mod map_terrain;
/// Exact mode-4 `Mountains::add_mountain` mutation runtime. The sixteen shipped
/// displacement-template producer remains an explicit external evidence boundary.
pub mod mountain_add_runtime;
/// Installed-content producer for the retail-derived MountainRange displacement templates.
pub mod mountain_template_producer;
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
/// Exact validator for the complete all-`-1` `Objects::init_unit` receipt chronology.
/// Product callers must still supply a retail-complete Unit-init receipt and canonical
/// after-image; registering this source does not turn `World::allocate_typed_at` into retail.
pub mod objects_init_unit_authority_frontier;
/// Added by `assembly:order-dispatch`. `Unit::work` `0x0060D180` and a real `Unit::do_job`
/// `0x00617A10` dispatch over an `OrderList` with retail's cursor semantics — the driver
/// `docs/mechanics/COVERAGE.md` §3 records as uncited. It is [`movement`]'s first caller.
pub mod order_dispatch;
/// `AirPatrolOrder` / `GroupPatrolOrder` dynamic waypoint payloads and the exact
/// `Unit::do_air_patrol` / `Unit::do_patrol` state transitions. Kept separate from the
/// dispatcher because air physics and group movement are explicit host boundaries.
pub mod patrol;
/// Exact `GameInfo::player` flags/owner constants shared by Sim command admission.
pub mod player_lifecycle_tails;
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
/// Typed adjacent-child requests for the exact frame-one setup Unit idle prefix.
pub mod setup_idle_prefix;
/// Stable retail `(owner, band, o)` object identity beside the dense simulation rows.
/// Allocation remains on the legacy dense path until save/digest/lookup consumers migrate.
pub mod sparse_object_bands_authority_frontier;
/// Source-exact SPECIAL_ANIM planner plus the typed atomic receipt consumed by the live order
/// dispatcher. The production [`crate::tick::Sim`] frame bridge remains an explicit red seam.
pub mod special_anim_executor;
/// Exact, non-mutating Unit/detector frontier for the retail phase-33 visibility producer.
pub mod step12_visibility_producer_frontier;
/// Sim-attached revisioned join for Step-12 Unit visibility facts. Plane publication remains
/// fail-closed until Build/Wall and reveal-fog effects can join the same transaction.
pub mod step12_visibility_runtime;
/// Registered by lane `come-out` on 2026-08-11 after the orchestrator's UNREACHABLE finding:
/// this module existed on disk with no `mod` line and was compiled only from its own test
/// file. Source-only `Object::eject_contents` `0x0064CD20` transaction plan under the fixed
/// step-8 `Wall::update_hits` arguments `(1, -1, 0, 1)`. `std`-only, no crate-root
/// dependencies. It does **not** cover the `Group::action_eject_all` argument set — see
/// `docs/mechanics/step8-eject-contents.md` §"What this does not cover".
pub mod step8_eject_contents;
/// Atomic detached-image STRAFE executor transaction.
pub mod strafe_executor_transaction;
/// Exact recovered `Unit::do_strafe` CFG/fact planner.
pub mod strafe_order_frontier;
/// Lossless retail walk and DoNSave v13 tag-8 authority for `StrafeOrder`.
pub mod strafe_runtime_authority;
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
/// Complete source reconstruction of `Unit::do_trade`; production adapters admit only proven
/// subpaths whose required owners are installed.
pub mod trade_order_frontier;
/// Registered by lane `come-out` on 2026-08-11. `Unit::action_come_out` `0x005E20B0`, the
/// 532-byte opcode-49 wrapper. Its `Unit::come_out(0)` call is the authority step the four
/// `unit_come_out_*_frontier` modules below now tile completely.
pub mod unit_action_come_out_frontier;
/// Registered by lane `come-out` on 2026-08-11. Second tranche of `Unit::come_out`
/// `0x00617C10`: the common release prologue `0x006186B4..0x00618B22` plus three outlined
/// virtual-call islands. 1,169 logical bytes.
pub mod unit_come_out_common_release_frontier;
/// Registered by lane `come-out` on 2026-08-11. First tranche of `Unit::come_out`
/// `0x00617C10`: entry cleanup, captain redirection, uncontained search, the Oil Platform
/// transport bridge and contained placement/unlink, `0x00617C10..0x006186B4`. 2,724
/// sequential bytes plus the 72 bytes of outlined islands it never billed — 2,796 in total.
pub mod unit_come_out_full_frontier;
/// Registered by lane `come-out` on 2026-08-11. Third tranche of `Unit::come_out`
/// `0x00617C10`: the gather-point selection loop `0x00618B22..0x006191A5` plus two outlined
/// islands. 1,683 logical bytes.
pub mod unit_come_out_gather_selection_frontier;
/// Added by lane `come-out` on 2026-08-11. Fourth and final tranche of `Unit::come_out`
/// `0x00617C10`: the post-placement order-installation dispatcher, the SPECIAL_ANIM order
/// tail and the terminal `Unit::add_to_army` RNG gate. 4,277 logical bytes — together with
/// the three tranches above this tiles all 9,925 bytes of the body with no gap and no
/// overlap, which [`unit_come_out_body_map`] asserts byte by byte.
pub mod unit_come_out_release_tail_frontier;
/// Added by lane `come-out` on 2026-08-11. The address-space accounting that ties the four
/// `unit_come_out_*_frontier` tranches to the retail body, and the typed boundary every
/// caller of `Unit::come_out` stops at today.
pub mod unit_come_out_body_map;
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
/// Detached exact zero-spawn prefix of the per-32-frame wildlife block.
pub mod wildlife_spawn_frontier;
/// Exact `World::set_oil_at` plus retail Goods-slot allocation/retirement owner.
pub mod world_oil_goods;
/// Full `UnitData::speed` authority over exact type, terrain, leader, hero, and Constants owners.
pub mod land_speed_authority;
