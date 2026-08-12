# Canonical diplomacy Sim mount

`canonical_diplomacy_host` is the exclusive, compile-green projection prepared for the shared
Bridge/Sim/DoNSave mount of retail diplomacy opcodes 38 and 41. It composes the already-landed
whole-body planners; it does not introduce a second `ObjectTable` or `Leader` state.

## Owner map

| Retail mutation domain | Canonical Sim owner at the shared mount | Detached transaction field |
|---|---|---|
| Resources | `Sim.leaders[who].econ.stockpile`, with the existing step-8/victory/production mirrors validated and rewritten together | `DiplomacyOwnerImage::resources` |
| Directional relations and flags | `Sim.vic_leaders.slots[who]`; `step8.leaders[who].diplo` becomes a validated derived view | `setup`, `leader_flags2` |
| Response counters, DOW/peace/attack frames, agendas | The existing `vic_leaders.slots[who].init_diplomacy` fields already walked by `LEADER_MATCH` | `leader_diplomacy` |
| Proposals and escrow | Exact step-8 `LeaderData` fields `taunt.dip` and `taunt.tributes` | `retained.proposals`, `retained.reserved_resources` |
| Only homeless retained scalars | New small Sim diplomacy state | `repeated_targets`, `sent_raw`, `received_scaled` |
| Shared vision | `vic_leaders.slots[who].init_diplomacy.ally_mask` | `shared_vision` |
| Victory bit 22 | `Sim.vic_match.semaphore` | `victory_mask` |
| Armies | `Sim.armies.lists[who][slot].valid` | `valid_armies` |
| Contained objects/orders | canonical `Sim.world` registry, stable generation, containment and order state | `ejection_units` |

Runtime query results—DOW costs, availability, tribute scale, console treaty, no-war gate, team
counts, neutral classification, and shared-vision prerequisite—are installed revision-bound facts.
They are reset after load and are never serialized as simulation state.

## Atomic boundary

`prepare_diplomacy_transaction` accepts an exact fixed-wire command. The focused integration tests
use the measured corpus bytes `26020000000500000000000000` and `290200000003000000`. It builds one
op41 aggregate image and executes the complete op38 or op41 planner over clones, including nested
`set_diplo` and recursive declarations. It retains the exact ordered authority list.

`commit_diplomacy_transaction` then:

1. compares the full live owner image with the prepared before-image;
2. requires byte-for-byte equality with the ordered completed-authority list;
3. validates every overlapping resource/relation projection in the after-image;
4. folds into a clone; and
5. replaces the caller's owner once.

Missing or stale authority leaves resources, proposals, relations, vision, victory, armies, and
objects unchanged. The shared Sim receiver must initially reject `ComeOut`, `KillContainedUnit`,
`AddAirStrafeOrder`, `Victory`, and `ForceArmyProcess` unless their complete staged host has been
mounted. `ConsiderTribute` is now executed inside the prepared owner image, including its
transposed positive-value tribute stamp and conditional gift stamp; even zero-valued goods emit
the callback in retail order but take its complete no-op arm. `NotifyDeal` is now lowered to its
complete local-only presentation envelope and does not pretend to mutate lockstep state.

## DoNSave v14 leaf

The new leaf is a bounded, fixed-size payload with its own version and eight-slot count. It stores
only proposal records, escrow, repeated-DOW count, and tribute sent/received. Relations, leader
interaction rows, resources, shared vision, victory, armies, and objects remain in their existing
sections. The codec rejects truncation, trailing data, slot drift, and payload-version drift.

DoNSave v14 now requires top-level chunk `0x000c`; a v13 stream with no such chunk restores
constructor diplomacy state and re-encodes byte-for-byte as v13. The same edge extends each
existing `LEADER_MATCH` row by the five production-AI scalars not already saved beside
`leader_flags2`. AIR, STRAFE, command-cache, metric, and economy order leaves retain their v13
layout in v14; focused tests pin byte equality and malformed-v14 refusal.

## Mounted and remaining boundary

- exported `canonical_diplomacy_host`, `diplomacy_accept_host`, and the two callback bodies;
- added the small homeless retained state plus transient installed authority to `Sim`;
- mounted v14 chunk `0x000c` and the coordinated production-AI Leader row extension;
- retained exact v13 root/order bytes and v7-v13 load behavior;
- mounted real opcode-38 packets through `Bridge::process_all` into the canonical Sim owner;
- project/fold all canonical owners and rebuild the exact step-8 relation/taunt mirrors;
- added a self-validating whole-body receipt which recomputes the prepared transaction;
- flip opcode closure rows only after real Bridge packets execute before and after save/load.

Opcode 38 now executes production transactions whose ordered external-authority list is empty.
Any reached `ComeOut`, contained-unit kill/Strafe, victory, or forced-army call refuses before
publication, so the static row remains `StateWired`. Opcode 41 remains wholly outside the
production adapter and red until all of those external authorities can be committed atomically.

Focused status: the callback/aggregate suites pass 12/12; the economy forward-compatibility suite
passes 11/11; LeaderMatch integration passes 3/3; production AI passes 11/11; and all 42 private
save/load tests pass. Opcodes 38/41 deliberately remain command-table red until the live Sim/Bridge
adapter can execute every reached external authority rather than acknowledging scalar stubs.
