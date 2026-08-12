# Canonical simple-Group opcode cohort

Status: source-only executable/PDB/corpus audit, 2026-08-11. No shared dispatcher, `Sim`,
save, checksum, or closure-table file is changed. In particular, this tranche makes **zero**
new `Complete` claims.

## Why these ten rows form one cohort

The selected rows are opcode 0 plus the nine fixed-size group receivers whose complete
action planners already exist without an open world cascade:

```text
0 GROUP
1 BEGIN             2 STANCE          12 HALT
14 SET_TRANSPORT    21 DISBAND        29 STOP_SPELL
30 FOLLOW           32 UNITMASK       33 BUILDMASK
```

This is the largest honest near-close cohort, not the ten most frequent commands. The
selection rule is ownership: every action consumes `CommandPackage::group`, and every
complete effect can be committed by the already-canonical `Sim` owners `CommandPackageState`,
the play-to-owner map, Game frame/RNG, fixed `groups_guys::Groups`, `World` object registry,
Unit/Build arrays, order lists, paths, leader flags, the revision-bound action-fact projection,
and typed presentation receipts. The active-building DISBAND arm reaches the production queue
nested in canonical `BuildData`; it does not create a new top-level store. None requires
movement/pathfinding RNG, terrain mutation, air physics, or diplomacy state.

The very uneven corpus distribution demonstrates why frequency was not the rule. BEGIN is
absent from the source-bound 61-recording artifact and SET_TRANSPORT occurs five times, while
DISBAND occurs 10,916 times. They still share the same transaction shell. Conversely,
MOVE_TO is more frequent than most of this cohort but reaches formation/path/collision
dependencies and stays in the separate movement transaction.

Opcode 0 is part of the cohort because a Group action packet is not a free-standing command.
Retail resolves the play-keyed received selection, validates the package player/owner, and
publishes a physical Group slot before the action. Reusing a Bridge-owned group would create
a second checksum authority. The landed canonical Group+Move host already supplies the
correct selection cache and fixed Groups allocator; this cohort generalizes that shell to a
strict `[GROUP][one admitted simple action]` transaction before admitting wider packages.

## Shipped executable and PDB boundaries

The source pair is exact:

| artifact | SHA-256 |
|---|---|
| `ron-bin/riseofnations.exe` | `30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079` |
| `ron-bin/sbl/rise.pdb` | `334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5` |

`schema/rise-procs.tsv` freezes the exact exclusive body ranges. The call column is the
instruction which transfers to the receiver; BEGIN is the virtual vtable `+0x14` call.

| op | handler body `[start,end)` | bytes | action call | action body `[start,end)` | bytes |
|---:|---|---:|---|---|---:|
| 0 | `0x0094A0C0..0x0094A700` | 1,600 | selection/allocator body | — | — |
| 1 | `0x00949FD0..0x0094A0B5` | 229 | `0x0094A0A6` | `action_begin` `0x00714100..0x00714108` | 8 |
| 2 | `0x00949ED0..0x00949FC7` | 247 | `0x00949FB4` | `action_stance` `0x0070D440..0x0070D7E0` | 928 |
| 12 | `0x00949140..0x00949227` | 231 | `0x00949216` | `action_halt` `0x0070D0C0..0x0070D36D` | 685 |
| 14 | `0x00948F60..0x0094904D` | 237 | `0x0094903C` | `action_set_transport` `0x007024B0..0x00702615` | 357 |
| 21 | `0x00948660..0x00948756` | 246 | `0x00948743` | `action_disband` `0x0070E260..0x0070E515` | 693 |
| 29 | `0x00947CC0..0x00947DA5` | 229 | `0x00947D94` | `action_stop_spell` `0x006FD7A0..0x006FD980` | 480 |
| 30 | `0x009479C0..0x00947AD2` | 274 | `0x00947ABF` | `action_follow` `0x006FD510..0x006FD795` | 645 |
| 32 | `0x00947790..0x00947894` | 260 | `0x00947881` | `action_unitmask` `0x006FCB90..0x006FCD24` | 404 |
| 33 | `0x00947680..0x00947784` | 260 | `0x00947771` | `action_buildmask` `0x006FC9A0..0x006FCB87` | 487 |

The source-only table in
`crates/don-replay/src/opcode_simple_group_cohort.rs` carries these values and the focused
test verifies the exact handler names/ranges against the PDB procedure inventory, resolves every
direct action call in the shipped PE, and compares identity, receiver, action, and wire width to
the 82-row command table. Wire sizes remain the dispatcher returns: variable GROUP, then 1, 5, 1,
5, 5, 1, 13, 9, and 9 bytes.

## Current closure table versus production ownership

All nine action rows currently read `Port::Complete`. That is internally consistent with the
definition in `command.rs`: the shadow `command::Bridge` plus a capable `Fleet` can reproduce
each receiver transaction. It does **not** prove that retail bytes enter `tick::Sim`'s fixed
Groups and World owners. `movement-command-closure-architecture.md` already identifies that
ownership split.

Therefore this audit neither promotes nor demotes those rows. It records two separate facts:

- receiver semantics: complete and recomputable;
- canonical packet integration: missing for this cohort, except the reusable opcode-0 shell
  demonstrated by the bounded Group+Move host.

Any closure report presented as production replay execution must require both facts. Merely
calling the Bridge from Sim and copying its private Group afterward is forbidden because a
mid-command failure could publish a Group without its Unit/Build/order/path mutations.

Decimal opcode **49** is deliberately not part of this cohort. It is `ComeOutCommand` (`0x31`),
an inline object/build receiver rather than a Group action, and remains `InlinePort::StateWired`.
Its recovered planner/executor work does not make it canonical until the package/Sim transaction,
save image, and tick hand-off share one owner.

## Source-bound corpus census

`schema/replay-validation.json` was produced from 61 decoded recordings and 5,055,253
commands. Its exact cohort counts are:

| opcode | command | count |
|---:|---|---:|
| 0 | Group | 78,197 |
| 1 | Begin | 0 |
| 2 | Stance | 74 |
| 12 | Halt | 69 |
| 14 | SetTransport | 5 |
| 21 | Disband | 10,916 |
| 29 | StopSpell | 113 |
| 30 | Follow | 7 |
| 32 | Unitmask | 297 |
| 33 | Buildmask | 320 |

The nine action rows total 11,801 commands. Counts are evidence of exercised wire arms, not
evidence that their state matches checksums.

The full local-corpus gate in
`crates/don-replay/tests/opcode_simple_group_cohort.rs` visits every currently present RCX
and separately reports load failures and undecoded package records rather than dropping
them. It also classifies every admitted action as adjacent Group, earlier Group, or
play-cache reuse. It is ignored in the default profile because local user recordings are
intentionally mutable; the exact invocation is below.

The 2026-08-11 local run covered all **64** RCX paths. Sixty-two opened, exposing 1,356,596
package records, of which 1,356,594 decoded. The integrity exceptions were exact:

- `playback___2014.08.08_22_02_25__fri_.rcx`: no viable obfuscation key;
- `playback___2014.08.12_19_59_21__tue_.rcx`: no command-package chain;
- `playback___2014.04.26_15_32_51__sat_.rcx`: 7,986 / 7,987 packages decoded;
- `playback___2014.08.08_20_54_48__fri_.rcx`: 19,042 / 19,043 packages decoded.

Every one of the **11,819** decoded local cohort actions was immediately adjacent to its
Group command. There were zero earlier-Group/intervening-command cases and zero cache-only
cases:

| opcode | local count | adjacent Group | earlier Group | cache only |
|---:|---:|---:|---:|---:|
| 1 | 0 | 0 | 0 | 0 |
| 2 | 74 | 74 | 0 | 0 |
| 12 | 69 | 69 | 0 | 0 |
| 14 | 5 | 5 | 0 | 0 |
| 21 | 10,920 | 10,920 | 0 | 0 |
| 29 | 113 | 113 | 0 | 0 |
| 30 | 7 | 7 | 0 | 0 |
| 32 | 302 | 302 | 0 | 0 |
| 33 | 329 | 329 | 0 | 0 |

The same run observed 78,872 Group commands overall. This establishes a narrow first host:
exactly `[Group][one admitted simple action]`. It does not license silently stripping a
future prefix or executing an action without Group; both remain typed refusals until direct
evidence extends the shell.

## Atomic transaction and remaining blockers

One request must carry `(play, lockstep_serial, exact command bytes)`. One preflight must
resolve the cached/explicit selection, fixed Group slot/id, every `(who,o,uid,Handle)` member,
and the complete reached owner projection. This includes the play-to-owner row, Game frame/RNG,
leader flags, scenario ignore-orders, revision/digest-bound type/capability facts, and the
active-Build queue arm; they may not be accepted as unbound booleans. Revalidation must cover all
before-images and Handle generations before the first write. The commit is assignment-only and
publishes Group, backlinks, Unit/Build state, orders, paths, receive cache, and receipts together.
RNG before/after must be equal for every row in this cohort.

The remaining adapter work is bounded per row:

| op | canonical facts/effects still needing the shared Sim adapter |
|---:|---|
| 0 | generalize the landed strict Group+Move shell without accepting/discarding unrelated commands; retain duplicate/empty play-cache semantics |
| 1 | fixed Group `disband = 0`; no object facts |
| 2 | representative stance type, modal option scan, leader flags, Unit/Build stance writes, mandatory/order/path retirement tail |
| 12 | scenario ignore-orders prelude, active/on-map/plane/special-animation facts, mask/order/path/action writes; reuse the exact halt plan already mounted for defeat cleanup |
| 14 | scenario prune projection, leader transport flag ladder, `can_ever_transport`, Unit mask writes |
| 21 | scenario prune projection, `validate_disband`, local receipt, `Object::disband`, and active-Build DISBAND queue effect; the production queue callback is a child inside this transaction, not a second owner |
| 29 | scenario prune projection, current CAST order/masks/type, order/path/action retirement, special gpiece update |
| 30 | scenario prune projection, stable target identity, QueueFirst halt/stash/replay, complete FOLLOW payload and target UID identities |
| 32 | ordered Unit mask toggles and mask-`0x100` order/path/action retirement |
| 33 | Build `valid_buildmask`, loop-carried toggle, Build mask writes, typed local feedback receipt |

The production save boundary must preserve every active typed payload reached by FOLLOW and
the generic order queues modified by STANCE/HALT/STOP_SPELL/UNITMASK. Existing save coverage
can be reused, but a packet test must prove save/reload/resume rather than infer it from a
planner test.

No shared hook should land until an exclusive canonical-host test demonstrates: all ten
wire decoders, every early no-op, every per-field mutation, forged/stale receipt rejection,
failure-before-mutation, duplicate/empty selection cache, RNG non-consumption, checksum
Groups after-image, and save/reload/resume.

## Gates

```sh
cargo test -p don-replay --test opcode_simple_group_cohort
cargo test -p don-replay --test opcode_simple_group_cohort \
  full_local_corpus_reports_the_exact_package_topology -- --ignored --nocapture
cargo fmt --all -- --check
git diff --check -- \
  crates/don-replay/src/opcode_simple_group_cohort.rs \
  crates/don-replay/tests/opcode_simple_group_cohort.rs \
  docs/assembly/opcode-simple-group-canonical-cohort.md
```

The first future shared hook should be one new `Sim::process_simple_group_package` sibling of
`Sim::process_command_package`, implemented by generalizing the existing canonical package
shell. It should accept exactly one of the nine action opcodes and return a typed receipt;
no command-table status changes belong in that hook tranche.
