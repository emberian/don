# Tick red audit: `GameDaemon::process_all` step 12

This note freezes the token-only step-12 lane. No compiler, formatter, test runner, or remote
job was used while producing it.

## Why this body

The compiled 29-row tick inventory currently has nine red rows:

| step | retail entry | present frontier | honest blocker |
|---:|---|---|---|
| 4 | `RunTimeEnv::run_script` `0x0043D0E0` | runtime wired | unrecovered reached `ScenarioFuncSet` builtins |
| 8 | `Leaders::process_all` `0x006ED2A0` | `systems/leaders.rs`, reached from `tick.rs` | child host coverage, especially stat/taunt inputs |
| 11 | `Leaders::strategy_all` `0x006ED430` | exact dispatcher plus recovered children | full `plan_strategy`/diplomacy host |
| 12 | `GameDaemon::process_all` `0x00732700` | exact shell and collision reaper reached from `Sim::do_frame` | `calc_danger` body and full detector/visibility facts remain |
| 13 | `Armies::process_all` `0x006F3B00` | exact slot dispatcher in `systems/armies.rs` | live army object/type host |
| 15 | `Objects::inc_time` `0x0065DB70` | deep `systems/unit_inctime.rs` port plus ammo path | authoritative object-band binding for all six loops |
| 17 | `Leaders::end_process_all` `0x006ED070` | body in `systems/leaders.rs` | presentation/GameInfo receipts and closure evidence |
| 19 | `Leader::process_event_frame` `0x006EC180` | body in `systems/leaders.rs` | authoritative per-leader event inputs and closure evidence |
| 22 | `Roads::scan_and_kill_stray_roads` `0x008956A0` | body in `systems/roads.rs` | renderer-owned `RoadElementCandidate` facts for live road tiles |

Step 12 is the widest coherent new adapter: it composes seven deterministic child systems and
owns local checksum-visible state that the old ad-hoc tick method omitted. Re-porting steps 15,
17, 19, or 22 would duplicate larger bodies already present on disk.

## Frozen retail facts

`schema/rise-procs.tsv` and the PDB give `GameDaemon::process_all` address `0x00732700`, size
315, source `gamedaemon.cpp:1403`. `schema/pdb-types.json` gives this exact 44-byte logical
layout:

| offset | PDB field | effect in this pass |
|---:|---|---|
| `+0x00` | `int repaths[8]` | signed `/ 2`; values whose quotient is below 3 become 0 |
| `+0x20` | `int empty_colls` | persistent cursor passed to `process_coll_blocks` |
| `+0x24` | `int borders` | reset by `check_borders`, then counts processed territory cells |
| `+0x28` | `int busy` | if non-zero, x86 `dec` once |

The executable disassembly freezes the successful-path order:

1. `0x00732711..0x007327A1`: decrement `busy`; decay all eight `repaths`.
2. `0x007327A4`: `process_victory` `0x00730EF0`, always.
3. `0x007327BA..0x007327C1`: `calc_danger` `0x00732D10` iff signed `frame % 200 == 0`.
4. `0x007327D7..0x007327E1`: `update_all_seen` `0x00732840` iff signed
   `frame % 100 == 33`.
5. `0x007327E6`: `calc_markets` `0x00732180`, always.
6. `0x007327EB..0x00732820`: all 64 `Region` records, 136-byte stride: clear flag `0x10`;
   if flag `0x20` was set, clear it and set `0x10`.
7. `0x00732824`: `check_borders` `0x00732060`.
8. `0x0073282B`: `process_coll_blocks` `0x00731F90`.
9. `0x00732830`: `Groups::process` `0x006FA210`.

There is no empty-world/active-leader gate in retail. A headless adapter must not turn this
unconditional pass into `Vacuous` merely because its current reduced world has no objects.

## Implemented boundary

`crates/don-sim/src/systems/game_daemon_step12.rs` now contains:

- the named PDB state and exact local mutations;
- allocation-free frame scheduling and exact child order;
- the fixed 64-region flag rollover;
- a typed child host, including the real persistent collision cursor and returned border
  counter;
- an all-children preflight so missing live facts fail before any local mutation;
- focused source tests for both frame phases, signed arithmetic, flag rollover and atomic
  refusal.

The real `Sim::do_frame` now uses this shell. Its live bridge retains the existing victory and
terminal-cleanup transaction, schedules fog/market/borders/groups through the typed callbacks,
and runs `collision_blocks_live` against the authoritative `map.world`. The 315-byte shell is
complete on a successful host receipt, but the compiled tick row remains red while a reached
child is missing.

## Landed bounded integration

1. `Sim` owns the PDB-shaped daemon record and `CollisionBlockRuntime`; their `empty_colls`
   scalars are preflighted equal before every pass, and the runtime result is committed back to
   the daemon mirror in the same callback.
2. `Sim::new` preserves the populated reduced region in slot zero and materializes the remaining
   63 default records. This is required by the unconditional retail rollover.
3. `SimGameDaemonHost` is the borrow-split bridge for victory, scheduled danger, fog, markets,
   borders, collision-block collection, and groups. It removes the old non-retail empty-world
   return; `Groups::proc_group` now advances even when no leader is active.
4. `GameDaemonProcessCollBlocks` is a dormant schema slot on the successful live path. It is
   emitted only when region cardinality or the cursor-mirror invariant refuses the transaction.
5. `GameDaemonCalcDanger` is charged only when the exact scheduler reaches it (`frame % 200 ==
   0`), rather than once on every frame.
6. `crates/don-sim/tests/game_daemon_step12_live.rs` freezes real-tick reachability, live-world
   reaping, cursor persistence/mirroring, group-cursor advance, phase-correct danger charging,
   and atomic bridge refusal.

The largest remaining bridge is `GameDaemon::calc_danger` `0x00732D10` (1,476 bytes), followed
by supplying exact `update_all_seen` detector/visibility facts. The collision-block child is
already ported in `systems/collision.rs`; this adapter exposes the state it needs, so continuing
to charge that gap after integration would be stale. Step 14's collision/repath admission must
also consume this same `GameDaemonState::repaths` array; that downstream binding is not claimed
by the step-12 integration.
