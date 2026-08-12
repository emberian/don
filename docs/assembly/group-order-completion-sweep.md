# Group/order completion sweep: the fourteen `orders_partial` actions

Status: **exclusive evidence/decoder tranche; zero closure delta**. The source module is not
exported by `don-replay::lib`, and this sweep does not edit the command bridge, Sim, save schema,
or `simulation-closure.json`.

This is the coherent 14-row cohort selected from the current closure ledger: every remaining
`Group::action_*` row whose status is `orders_partial`. Together their retail action bodies are
29,143 bytes. The tranche freezes exact packet images, retail receiver-to-action calls, reachable
order families, and the dependency gates that remain between a packet and a resumed production
tick. It deliberately does not turn packet knowledge into runtime credit.

## Retail executable, PDB, and packet evidence

The matched executable is `riseofnations.exe`, SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`; the matched PDB SHA-256
is `334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`. For every row below,
the test maps the PDB VA through the actual PE section table, hashes the complete process and
action bodies, verifies the process body contains the stated direct `E8` action call, and checks
the decoded relative target. The combined 1,134-byte body manifest hashes to
`0bb0e83a596ba0b54ddd59f828e1184dd47ff215633f77717840a11ef6356af1`. Capstone 5 consumed
every PDB byte with no undecoded suffix: 1,503 handler instructions and 8,105 action
instructions.

| opcode | command / wire bytes | process VA / bytes | call VA | action VA / bytes | source-bound RCX count |
|---:|---|---:|---:|---:|---:|
| 3 | `FormCommand` / 13 | `0x00949d90` / 307 | `0x00949eb0` | `action_form` `0x00707220` / 746 | 21 |
| 4 | `AttackCommand` / 17 | `0x00949c30` / 339 | `0x00949d70` | `action_attack` `0x00712490` / 3,833 | 5,715 |
| 7 | `MoveToCommand` / 22 | `0x009497c0` / 421 | `0x00949952` | `action_move_to` `0x0070fba0` / 49 | 9,178 |
| 8 | `MoveNearCommand` / 26 | `0x009495c0` / 500 | `0x009497a1` | `action_move_near` `0x00704990` / 9,205 | 0 |
| 9 | `AttackGroundCommand` / 10 | `0x009494a0` / 274 | `0x0094959f` | `action_attack_ground` `0x00704520` / 1,133 | 235 |
| 10 | `PatrolCommand` / 10 | `0x00949380` / 274 | `0x0094947f` | `action_patrol` `0x007030c0` / 1,215 | 169 |
| 11 | `LaunchPatrolCommand` / 25 | `0x00949230` / 333 | `0x0094936a` | `action_launch_patrol` `0x00703580` / 2,043 | 163 |
| 15 | `BoardShipCommand` / 9 | `0x00948e00` / 339 | `0x00948f3e` | `action_board_ship` `0x00700010` / 1,149 | 0 |
| 16 | `RepairCommand` / 13 | `0x00948cb0` / 324 | `0x00948de1` | `action_repair` `0x007020c0` / 999 | 0 |
| 17 | `TradeCommand` / 21 | `0x00948b20` / 400 | `0x00948c9d` | `action_trade` `0x00701cc0` / 1,022 | 8 |
| 19 | `GatherCommand` / 9 | `0x009488b0` / 339 | `0x009489ee` | `action_gather` `0x00700b90` / 3,052 | 963 |
| 20 | `GarrisonCommand` / 13 | `0x00948760` / 326 | `0x00948893` | `action_garrison` `0x00700490` / 1,791 | 127 |
| 31 | `GuardCommand` / 13 | `0x009478a0` / 276 | `0x009479a1` | `action_guard` `0x006fcd30` / 2,012 | 108 |
| 36 | `ScrambleCommand` / 1 | `0x00947ae0` / 229 | `0x00947bb4` | `action_scramble` `0x007111c0` / 894 | 58 |

The 61-file source-bound replay artifact contains 16,745 commands from this cohort and exercises
11 of 14 packet shapes. `MoveNear`, `BoardShip`, and `Repair` have zero source-bound observations;
their packet authority comes from PE/PDB plus both generated command tables, not invented replay
coverage. The ignored full-corpus test also decodes every matching command in the live corpus,
including newly added retail replays, and rejects any truncation or trailing byte.

The 64-path live corpus run on 2026-08-11 admitted 16,947 cohort commands:
`Form=21`, `Attack=5,771`, `MoveTo=9,231`, `AttackGround=239`, `Patrol=169`,
`LaunchPatrol=163`, `Trade=8`, `Gather=966`, `Garrison=129`, `Guard=108`, and
`Scramble=142`; the same three packet gaps remained zero. Its 372,543-byte command manifest
hashes to `f8e65103867a0457a9f92fc545c567ea02e2e0049576b2c1b973e9f42947c34d`. Relative to the
source-bound artifact, the finished solo replay adds 56 Attack, 53 MoveTo, 4 AttackGround,
3 Gather, 2 Garrison, and 84 Scramble commands.

The strict decoder preserves all raw signed fields. It neither turns missing bytes into zero nor
normalizes raw queue/formation/order bytes. A successful result therefore means exactly one full
fixed-size retail command was admitted.

## Dependency DAG and owner split

```text
RCX command bytes
  -> exact fixed packet decoder                         [this tranche: 14/14]
  -> CommandPackage chronology and opcode-0 selection [production route: bounded]
       -> play-keyed (o,uid) cache
       -> canonical fixed-slot Groups allocation/backlinks
  -> Group receiver world/type/formation branches     [per-action authority]
  -> complete typed Order payload installation        [Order is the payload owner]
  -> Unit::do_job concrete executor                   [executor, not payload owner]
  -> canonical Sim save/reload of Group+Order+cache   [persistence owner]
  -> production scheduler tick, world callbacks, RNG  [observable completion]
```

These stages are not interchangeable:

- `CommandPackage` owns routing, chronology, player mapping, and selection-cache semantics. It
  must not become a second Group or Order owner.
- `groups_guys::Groups`, the World unit columns, and the per-unit `OrderList` own persistent
  action state. A detached planner receipt is not authoritative gameplay state.
- an implemented `do_job` arm proves only execution after installation. It does not prove the
  packet selected the right objects or that the payload survives a save.
- a typed payload proves data fidelity, not that its executor's entire reachable order cone is
  implemented. In particular, `AirPatrol` can install `Strafe`.
- package-to-Sim application earns no closure until the same state saves, reloads, and produces
  the same next production tick.

## Current gate matrix

`packet` means exact wire authority. `route` means the bounded canonical production host admits
the command today, rather than an isolated planner. `executor` includes every possible concrete
order reached by the receiver/executor cone. `save` means all such payloads have a canonical
round trip. A check in one column does not waive later columns.

| action | packet | route | executor cone | save | principal open dependencies |
|---|:---:|:---:|:---:|:---:|---|
| form | yes | no | yes | yes | route, receiver world tail, RNG transaction, production tick |
| attack | yes | no | no | no | route, world tail, attack-position RNG, `CastSpell`/`Strafe`, save, tick |
| move-to | yes | yes | yes | yes | receiver world tail, RNG transaction, tick |
| move-near | yes | no | yes | yes | route, receiver world tail, RNG transaction, tick |
| attack-ground | yes | no | yes | no | route, receiver world tail, payload save, tick |
| patrol | yes | no | no | no | route, world/RNG tail, transitive `Strafe`, save, tick |
| launch-patrol | yes | no | no | no | route, containment/RNG tail, transitive `Strafe`, save, tick |
| board-ship | yes | no | yes | yes | route, receiver world/containment tail, tick |
| repair | yes | no | no | no | route, world tail, optional `CastSpell`, save, tick |
| trade | yes | no | no | no | route, world tail, `TradeRoute`, save, tick |
| gather | yes | no | no | no | route, world/RNG tail, `Gather`/`CastSpell`, save, tick |
| garrison | yes | no | yes | no | route, receiver world/containment/RNG tail, save, tick |
| guard | yes | no | yes | no | route, receiver world/RNG tail, save, tick |
| scramble | yes | no | no | no | route, containment/RNG tail, transitive `Strafe`, save, tick |

This table is intentionally stricter than `order.rs` saying that `AirPatrol` itself is
implemented: an AirPatrol tick can install the still-unimplemented `Strafe` order, so Patrol,
LaunchPatrol, and Scramble cannot claim a complete production executor cone.

## Fresh savegame evidence

The fresh retail v16 save
`new save game 2026.08.11 15'42'57 (Tue).SVX` has compressed SHA-256
`161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7` and decompressed SHA-256
`fa31f89f9bf435e28847e9e54d62cfe712e6bafc6cedb9ab98c8ab6730adb7c8`. Its exact Groups image
is at `0x4610b..0x4f21f`: 512 retained fixed slots, 12 live Groups,
`last_group = [4,65,130,192,256,320,384,448]`, and `proc_group = 47`.

That is strong evidence for the canonical Groups allocator/save owner. It is **not** OrderList
evidence. The current SVX parser has not localized each Unit's `OrderList::walk_data` substream,
so this artifact supplies zero payload-save or action-closure credit. The next save investigation
must find those substreams from retail structure, not byte-pattern guessing.

## Largest truthful integration sequence

The 14-way packet decoder is the largest stateless atomic tranche available now. It can land
without creating another gameplay owner. The following production work should remain split at
owner boundaries:

1. Generalize the already canonical opcode-0 selection shell to dispatch typed action packets
   while preserving package prefixes, play-keyed cache reuse, fixed-slot allocation, old-Group
   removal, and backlinks in one staged transaction.
2. Mount the seven receiver families whose concrete executors are already complete: Form,
   MoveTo, MoveNear, AttackGround, BoardShip, Garrison, and Guard. This is the shortest route to
   seven honest end-to-end rows, but only after their world branches and payload saves exist.
3. Add canonical typed save leaves for AttackGround, GroupPatrol/AirPatrol, Guard, Garrison, Cast,
   Gather, and the two-endpoint TradeRoute. Do not flatten targets or second endpoints.
4. Close the shared executor blockers: `CastSpell`, `TradeRoute`, `Gather`, and `Strafe`. Those
   four bodies unlock seven currently blocked actions (Attack, Patrol, LaunchPatrol, Repair,
   Trade, Gather, Scramble).
5. Mount world/containment and RNG callbacks as revalidated authorities, then run each real packet
   through `CommandPackage -> Sim -> save -> reload -> production tick` and compare complete
   before/after state and next-tick digests.
6. Only after those tests pass should action/opcode rows move in the closure ledger. No schema or
   closure status is changed by this sweep.

The practical swarm split is therefore wider than one opcode per lane: package routing, payload
save leaves, the four executor blockers, SVX OrderList localization, and resumed-tick witnesses
can proceed independently, then integrate at the final packet-to-tick gate.

## Verification

```sh
cargo test -p don-replay --test group_order_completion_sweep
cargo test -p don-replay --test group_order_completion_sweep \
  every_shipped_corpus_record_in_the_cohort_decodes_exactly -- --ignored --nocapture
```

The focused test also checks that all 14 rows remain the complete `orders_partial` set. If another
lane legitimately promotes a row, this frozen sweep fails and must be regenerated rather than
quietly retaining stale readiness claims.
