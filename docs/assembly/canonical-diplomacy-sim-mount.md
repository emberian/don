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
| Object identity for diplomacy ejection | canonical `Sim.world` registry plus transient digest/identity-bound callback authority for contained Units | `ejection_units` |

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
objects unchanged. A contained-Unit producer now binds `come_out(0)`, lazy type-domain, and lazy
AIR_PATROL-query answers to the exact current-save-reloaded Unit UID/type/generational identity, retail
address, containment link, and channel digest. It exposes the complete instruction-ordered
`ComeOut`/`KillContainedUnit`/`AddAirStrafeOrder` cone from a real opcode-38 packet. The shared Sim
receiver still rejects those mutation calls. `ForceArmyProcess` admits the exact v17-owned
early-return arm where `human_frame` decrements and leader bit `0x40` returns before normalization;
that bit is also retail's `DEFEATED` bit, so a preceding Victory call on the same winner takes its
exact no-op return. It also admits an active zero-city owner's empty non-mustering Army: normalize
zeros the five derived aggregates and the zero-standard path closes the formation without reaching
a Group, Unit, terrain, AI, or RNG host. An empty mustering Army with more than one human-order
frame remaining takes another exact arm: normalize zeros those aggregates, `send_here` clamps its
rally using the saved World dimensions, and the zero-group fanout returns before dispatch. The
expired empty naval muster and the saved-strategy-safe empty land muster both preserve retail's
post-`do_mustering` status re-read: they enter `do_marching`, obtain zero from its first mobile
count, and close before target selection or RNG. The released land arm receipts and stale-CAS binds
the exact `LeaderData::strategy[ArmyData::reg]` word. The remaining general AI body stays
unavailable.
Generic Victory is admitted only through its canonical Leader/Match transaction and the explicitly
supported defeated-owner cleanup branches. `ConsiderTribute` is executed inside the prepared owner
image, including its
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

## DoNSave v17 Armies owner

The additive top-level chunk `0x000f` persists the canonical eight owner lists and all sixteen
preallocated Army slots per owner. Its bytes follow retail `Armies::walk_data`: exact fixed
`PtrArray` allocation history, one pointer-presence byte per slot, every `valid:i16`, and the
remaining 150-byte `ArmyData` image only for live slots. Decode validates live owner/slot identity,
bounded prefixes, Group backlinks, and unique Group predecessors; malformed references and
duplicate/cyclic ownership fail before constructing a Sim. Formats v7 through v16 retain their
original root bytes and restore the constructor-empty Army owner.

## DoNSave v21 Leader strategy owner

The existing `LEADER_MATCH` row now carries `LeaderData::strategy[64]` at the v21 edge. Older
streams restore the retail constructor-zero array. The array remains in the canonical Leader owner,
participates in the Leader checksum walk at its real `+0xA68` order, and is not duplicated in the
Armies or diplomacy sections.

## Mounted and remaining boundary

- exported `canonical_diplomacy_host`, `diplomacy_accept_host`, and the two callback bodies;
- added the small homeless retained state plus transient installed authority to `Sim`;
- mounted v14 chunk `0x000c` and the coordinated production-AI Leader row extension;
- mounted v17 chunk `0x000f` for the exact canonical Armies owner;
- retained exact v13 root/order bytes and v7-v13 load behavior;
- mounted real opcode-38 packets and authority-empty opcode-41 transactions through
  `Bridge::process_all` into the canonical Sim owner;
- project/fold all canonical owners and rebuild the exact step-8 relation/taunt mirrors;
- added a self-validating whole-body receipt which recomputes the prepared transaction;
- flip opcode closure rows only after real Bridge packets execute before and after save/load.

Opcodes 38 and 41 now execute production transactions whose ordered external-authority list is
empty, consists only of staged generic-Victory authority, or consists only of exact
bounded `ForceArmyProcess(1)` calls. They execute the exact mixed cohort in which a defeated
winner's Victory call returns unchanged before its armies-off calls decrement their v17-owned
countdowns, and the active-winner cohort in which substantive Victory precedes empty-Army
normalization/retirement or an outstanding empty-Army human rally. An active winner whose human
countdown expires on entry, or whose forced processing reaches any Group/AI body, still refuses the
entire transaction, including staged Victory. This includes the complete ordinary reciprocal peace acceptance: both resource directions,
the two root relation calls, peace stamps, `consider_tribute`, `notify_deal`, and reciprocal record
clears publish together. Generic alliance Victory is also mounted when its staged canonical
Leader/Match transaction has an exact staged defeated-player Army/Unit cleanup: relation rows are
folded into the staged Leader image before `Leader::victory`, terminal Build queues are cleaned on
staged clones, and defeated owners may have empty Unit bands, active on-map ground Units, and
standing Armies whose reached Groups contain only authority-complete halt members. The transaction
clones `Groups`, `World`, and every path stack, resolves the whole Army/Group/member and Unit-band
fact cone first, then applies `Army::stop` without closing or unlinking the Army. Group action state,
member order/path/leash fields, and the following exact `Unit::clear_orders` net transition publish
together: orders and paths clear, facing latch `0x0400_0000` and defeat leash `0x0004_0000` are
removed, and the empty action endpoint is rebuilt from current position/facing. A typed per-owner
cleanup receipt records every stopped Army, Group, member, and Unit-band action. All owners and
cleanup clones publish only after the diplomacy stale-owner CAS. Separately, the exact
`Armies::diplo_change` owner gate and ascending valid-slot order feed the mounted forced-process
arm. Each non-zero `ArmyData::human_frame` decrements once; zero remains zero. Full Army bytes and
both Leader flag words participate in the same stale CAS before the v17 Army image is replaced. The
empty-retirement branch additionally binds the lazily read zero `LeaderData::city_num`; the
empty-human-rally branch instead binds the lazily read World width/height. The armies-off branch
binds neither. Planes, contained-Unit mutation calls, missing type/path facts, or
forced-army calls that proceed past exact empty normalization into the remaining AI state machine
refuse before publication, so both static
rows remain `StateWired`. Opcode 41's ordinary accepted-deal roots use
`max(accepted,current)`, while its attack-conflict preflight rejects attacks on either party's
ally. The measured contained-ejection branch is therefore reached by opcode-38 alliance
revocation, not manufactured through an impossible op41 downgrade.

Focused status: the live opcode-38/opcode-41 runtime suite passes 16/16 and includes a real opcode-38
packet-to-current-v18-save/load-to-resume ejection projection covering success, failure/kill, and
successful air/Strafe arms. A second real opcode-38 war packet crosses the same current save/load,
whose Army section remains the exact v17 owner, reinstalls only transient query facts, decrements
the exact armies-off countdown, and matches uninterrupted save bytes and channel digest; the
focused Army transaction suite passes 8/8, including stale Army, Leader, and lazily read city-count
and World-size rollback. It also
covers multi-opponent alliance Victory with empty cleanup, active-ground-Unit save/resume, and
standing-Army packet-to-current-save/load-to-resume equality; plane and missing-type paths remain
atomic refusals. Real opcode-41 packets cross save/load through the ordered no-op Victory plus
armies-off transaction, substantive Victory plus empty-Army retirement, and substantive Victory
plus an empty-Army human rally. A countdown-expiry counterpart proves staged Victory and the Army
countdown both roll back when the deeper body is reached.
The callback/aggregate
suites pass 12/12; the economy forward-compatibility suite
passes 11/11; LeaderMatch integration passes 3/3; production AI passes 11/11; and all 46 private
save/load tests pass. Opcodes 38/41 deliberately remain command-table red until the live Sim/Bridge
adapter can execute every reached external authority rather than acknowledging scalar stubs.
