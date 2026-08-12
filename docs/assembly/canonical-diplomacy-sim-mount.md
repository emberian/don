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
mounted. Opcode 41 likewise remains fail-closed until each emitted `ConsiderTribute` and
`NotifyDeal` call is acknowledged; even zero-valued goods emit the former in retail order.

## DoNSave v14 leaf

The new leaf is a bounded, fixed-size payload with its own version and eight-slot count. It stores
only proposal records, escrow, repeated-DOW count, and tribute sent/received. Relations, leader
interaction rows, resources, shared vision, victory, armies, and objects remain in their existing
sections. The codec rejects truncation, trailing data, slot drift, and payload-version drift.

The coordinated shared change should make v14 require top-level chunk `0x000c`; a v13 stream with
no such chunk restores constructor diplomacy state and retains its original bytes. This helper
deliberately does not bump the shared format constant by itself so STRAFE/order authority and
diplomacy can take one version edge.

## Remaining surgical mount

- export `canonical_diplomacy_host` and the already-landed `diplomacy_accept_host`;
- add the small homeless retained state plus transient installed authority to `Sim`;
- project/fold all canonical owners and rebuild step-8 mirrors;
- add a Bridge whole-body receipt for opcodes 38/41 without weakening the old boundary receipt;
- add the v14 chunk and adjust step-8 save-view validation;
- flip opcode closure rows only after real Bridge packets execute before and after save/load.

Focused status: six tests pass, covering real op38/op41 packets, stale-owner and missing-authority
rollback, complete retained-state round-trip, v13 absence semantics, and malformed-leaf rejection.
