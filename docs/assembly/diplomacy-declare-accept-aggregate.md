# Declare/Accept aggregate and corpus gate

This note fixes the evidence and ownership boundary for the remaining diplomacy command
rows, opcode 38 (`DeclareCommand`) and opcode 41 (`AcceptCommand`).  It does not mark both
rows complete.  Opcode 38 now has a source-only, atomic aggregate transaction;
opcode 41 remains unavailable until the complete reciprocal-handshake tail can use the same
owner.  Registering a relation-only or resource-only approximation would be a false closure.

## Full replay-corpus identity

`crates/don-replay/tests/retail_diplomacy_cohort.rs` decodes the replay corpus through the
shipped multiplayer XOR-and-pad reader.  For every opcode-38 or opcode-41 command it appends
this filename-independent record in corpus, turn, player, and command order:

```text
<sha256 compressed recording><i32 lockstep serial><u32 simulation frame>
<i32 play><u16 wire length><complete command bytes>
```

The current corpus contains 64 recording paths: 62 decode completely and two legacy files
are explicit decode refusals.  The normalized cohort has these fixed properties:

| evidence | result |
|---|---:|
| opcode 38 commands | 101 in 24 files |
| opcode 38 treaty 0 / treaty 1 | 94 / 7 |
| opcode 41 commands | 1,415 in 26 files |
| manifest length | 83,784 bytes |
| manifest SHA-256 | `92f255aa5e94bc503cfed8262ed47daeaa74d2819c654deefe5cd683151a97a9` |

The fresh completed solo recording contributes neither opcode; it still participates in the
decoded-file count.  The fixture also pins complete real Bridge packets, not reconstructed
field tuples:

```text
26020000000500000000000000  Declare(2, 5, WAR)
26010000000600000001000000  Declare(1, 6, PEACE)
290200000003000000          Accept(2, 3)
```

## One persistent owner, several validated projections

The command host currently stores `DiplomacyCommandState` in `command::ObjectTable`, while
`Sim` separately owns resource, relation, vision, army, object, victory, and leader-stat
state.  That split is acceptable for a read-only planner but not for an applied 38/41
receipt.  The canonical owner must be a `Sim`-resident diplomacy aggregate.  `ObjectTable`
may borrow or snapshot it for the Bridge; it must not remain a second persistent gameplay
store.

| aggregate domain | canonical persisted data | installed/derived facts, not duplicated state |
|---|---|---|
| command/deal | both directional 92-byte proposal records; `+0x314/+0x334` response counters; declaration/deal/attack frames; the `+0xB4+other*4` agenda/ack bit touched by first accept; reserved-resource dwords | local presentation recipient |
| resources | decoded authoritative buckets, `LeaderData+0x468` mirror, sent/received statistics | good availability, DOW cost and tribute-scale content queries |
| relations | both directional declaration cells and shared-vision bytes | scary-console treaty query and shared-vision prerequisite results |
| declaration statistics | per-target counts and repeated-target count | none |
| transitive effects | victory bit, interface-dirty byte, authoritative army/object state changed by mandatory child calls | ordered ejection roster, valid-army roster, `come_out`/order-query results |
| game gates | frame, no-rush interval, war-allowed state, configured/runtime-team inputs | localized text, audio, and notification rendering |

While these values still project through multiple existing types,
`diplomacy_declare_host::validate_projection` requires identity, flags, relation cells,
local player, frame, and authoritative buckets to agree before planning.  It also validates
the after-image.  A projection disagreement is an unavailable transaction, never a
last-writer-wins repair.

The save owner must retain the retail-shaped values, not planner scratch. The directly
reached `LeaderData` offsets are:

| offset | retained value |
|---:|---|
| `+0x074..+0x090` | eight directional relation declarations |
| `+0x0B4+other*4` | first-accept agenda/ack bit |
| `+0x1B4/+0x1D4` matrices | accepted attack frame and counterpart identity |
| `+0x20C`, `+0x270..+0x28C` | repeated-declaration and per-target DOW statistics |
| `+0x2B0..+0x2CC`, `+0x2D0..+0x2EC` | declaration/alliance-break and accepted-peace frames |
| `+0x314..+0x330`, `+0x334..+0x350` | response counters |
| `+0x468..+0x47C`, `+0x498..+0x4AC` | resource mirror and deal reserve |
| `+0x860/+0x864` | tribute sent/received statistics |
| `+0x6929` | shared-vision/ally mask byte |
| `+0x692C..+0x6C0B` | eight complete 92-byte proposal records |

The decoded authoritative resource store, victory/semaphore state, interface-dirty state,
and nested army/object mutations live outside `LeaderData` and must be serialized by their
canonical owners in the same save generation. Installed type availability, cost functions,
tribute scaling inputs, and ordered host rosters are reinstalled facts and must be absent from
the gameplay chunk.

## Opcode 38 transaction now available for mounting

`crates/don-sim/src/systems/diplomacy_declare_host.rs` composes the already recovered
command prefix, declaration payment, and `Leader::set_diplo` owners in retail order:

1. already-enemy, allied-war rewrite, and no-rush prefix;
2. `LeaderData::afford_dow`, then `Game::war_allowed`, no-op paths;
3. one paid six-good `pay_dow` plan (and the recovered general double-payment arm);
4. declaration counters and allied declaration-frame stamp;
5. root `set_diplo`;
6. slot-ordered third-party runtime-team fanout, each using the previous after-image;
7. typed local presentation.

The transaction compares the complete before-image, recomputes the plan, requires the exact
ordered list of mandatory `SetDiploAuthority` children, and only then replaces the aggregate.
A stale snapshot, missing/extra child call, missing reached fact, or planner error leaves every
projection unchanged.  The standalone tests exercise the two real corpus packets, war denial,
single-good affordability reporting, allied rewrite/stamp, projection refusal, stale state,
and missing authority rollback.

This is source/test closure, not command-table closure.  Opcode 38 remains red in the shared
ledger until the Bridge calls this aggregate transaction and `Sim` owns and saves its state.

## Why opcode 41 remains red

`plan_accepted_deal_resources` is exact for the six-good transfer loop after reciprocal
validation and reservation.  That is not the whole `action_respond(target, 1)` transaction.
The retail body also includes the following observable stages. Addresses are from the
supported executable and function names are fixed by the shipped PDB.

| VA range | stage | required owner/effect |
|---|---|---|
| `0x006D03D8..0x006D03E7` | response prefix | clear accepter `+0x334[target]`, then `+0x314[target]` |
| `0x006D05A9..0x006D0893` | first-accept reservation | if the directional record is not pending, scan six goods and eight attack/DOW obligations, refuse before mutation on shortage, debit both authoritative and `+0x468` resource images, credit `+0x498` reserves, write directional DOW reservations, set pending, and OR the reciprocal `+0xB4` agenda/ack bit with 4 |
| `0x006D0894..0x006D0D05` | reciprocal/conflict decision | scan third-party attack conflicts; on conflict clear agreements and records atomically; otherwise inspect reciprocal pending/treaty/offers/attacks and take the exact local-notification early returns |
| `0x006D0D06..0x006D0DF8` | alliance admissibility | treaty 2 compares `LeaderData::num_team_members(1)` with `num_allies`; an invalid alliance clears both agreements and both records before notification |
| `0x006D0E36..0x006D0FFA` | accepted resources | declaration-cost refunds/clears, both scaled tribute directions, sent/received statistics, `Leader::consider_tribute`, and reserve clamps for six goods |
| `0x006D1000..0x006D10A6` | root treaty | two `set_diplo(max(proposal,current_raw))` calls; treaty 1 stamps both `LeaderData+0x2D0` cells |
| `0x006D10A7..0x006D11BB` | neutral team fanout | if either party is `LeaderData::is_neutral`, scan present third parties, apply worse declarations through `set_diplo`, then call `Leader::notify_deal` |
| `0x006D11BC..0x006D12AE` | attack continuations | after `Game::war_allowed`, stamp both attack matrices and party identities, then recursively call `action_declare(candidate, WAR, no_payment, 0)` for each non-enemy party |
| `0x006D12AF..0x006D1340` | commit tail | clear both 92-byte records, issue the local accepted-deal text/chat/sound, and call `notify_deal` for both parties when the deal carried content |

The first accept can stop after reserving resources; a reciprocal accept can consume the
deal.  Consequently an `Accept { who, whom }` scalar plus a resource-plan receipt cannot
represent save/resume or rollback.  Opcode 41 must stay unavailable until all eight stages
operate on the canonical aggregate and every recursive declaration and `set_diplo` authority
is nested in the same receipt.

## Required surgical shared integration

After the exclusive transaction tests and corpus gate are green, the shared change should be
small but semantically strict:

1. export `systems::diplomacy_declare_host`;
2. add one `Fleet` aggregate snapshot/apply callback and have Bridge route boundary 38 to it;
3. place the canonical aggregate on `Sim` and adapt `ObjectTable` to that owner instead of
   retaining a divergent image;
4. add the next-version, next-free save chunk for every persistent field in the ownership
   table, validating all projections on write and after read (today's DoNSave v13
   `LEADERS` section serializes economy only, so these bytes cannot be reconstructed from it);
5. change the closure ledger only after a real Bridge packet applies through that path and a
   save/load/resume test produces the same next transaction.

The minimum later transaction contract is deliberately one callback, not a sequence of
projection setters:

```text
snapshot() -> (revision, DeclareAuthorityImage)
preflight(before, wire) -> (DeclarePlan, ordered SetDiploAuthority[])
execute_children(ordered SetDiploAuthority[]) -> exact acknowledgements
compare_and_commit(revision, before, plan.after, acknowledgements) -> DeclareReceipt
```

`compare_and_commit` must re-run `plan_declare`, require byte-identical `before`, require the
complete ordered child list, and publish all persistent fields under one revision. There is no
valid callback for “write relations”, “debit resources”, or “finish fanout” separately. The
Bridge counts the packet as acted only when `DeclareReceipt::validates()` succeeds.

A direct `Sim` adapter test is not honest yet: `Sim` has no diplomacy proposal/relation
aggregate and its only package entry point is the specialized Group→Move transaction, while
the general Bridge reaches a separate `ObjectTable: Fleet`. Adding a fake adapter would merely
recreate that split owner. The first valid adapter test must land with the held `Sim` field and
prove a real packet, stale rollback, and save/load/next-packet equivalence against that one
owner.

Only the production module export is mounted now. Shared command, tick, save, schema, and
command-table files remain unchanged; their adjacent owners must land before this contract is
adapted into `Sim`.
