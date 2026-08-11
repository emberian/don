# Retail Group → MoveTo package fixtures and canonical host contract

Date: 2026-08-11. Scope: read-only evidence for the future canonical
`Sim::process_command_package(play, stamp, payload)` owner. No live process mutation was
needed. The recording and save remain external artifacts under `ron-data`; the repository
contains only compact derived metadata and tests.

## Evidence identities

The finished recording is:

```text
ron-data/replays/Playback - 2026.08.11 11'44'38 (Tue).rcx
gzip SHA-256  558e0cd53dbed4f820e8757c0327a58384d433e5beb8e9eeef5d64df4d67bd54
plain SHA-256 5dbef00c8283ba213e929bd01df6f9cffafcf6b8d3beaa0fbff90f74348b2927
plain bytes   2,793,834
stream offset 1,025,163
```

All 60,402 packages decode, with 68,811 commands, XOR key zero, and no padding RNG.
The fresh retail v16 save is:

```text
ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX
gzip SHA-256  161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7
plain SHA-256 fa31f89f9bf435e28847e9e54d62cfe712e6bafc6cedb9ab98c8ab6730adb7c8
plain bytes   2,923,161
```

The replay seed is `0x007f93e0`; the save seed is `0x014810ac`. They have the same build,
settings shape, and active `(slot, who, play)` rows `(0,0,0)` through `(3,3,3)`, but they
are different matches. The replay and save independently prove the identity grammar; an
object from one must never be joined to an object from the other.

## Exact replay cohort

There are 675 Group commands and 53 MoveTo commands. Every MoveTo is terminal and is
immediately preceded by Group inside the same package. There are no unpaired MoveTo rows.

| complete decoded package shape | count |
|---|---:|
| `[Group, MoveTo]` | 41 |
| `[Camera, Group, MoveTo]` | 11 |
| `[Camera, PlayerSpeed, Group, MoveTo]` | 1 |

All 53 packages have header `play=0`, `valid=0`; all 53 Group commands carry `who=0`.
That equality is evidence for this one solo recording, not permission to collapse `play`
and `who`. The package header field named `group` by the executable is a monotone package
serial here—exactly `package_index0 + 1`—and reaches 58,844 in this cohort. It is not a
canonical Groups slot and must never be used as one.

Group `num` has distribution:

```text
0:10, 1:32, 2:1, 4:2, 5:1, 6:1,
26:1, 29:1, 37:1, 38:1, 52:1, 55:1
```

The 43 explicit selections contain 290 member occurrences, 129 distinct values, range
`0..324`, no negative values, and no within-selection duplicates. Each is an owner-local
`i16 o` under the Group command's `who`; it is not a global `Handle`.

The complete MoveTo tail `(set_angle, angle, orders, queued, form, width, disembark)` is:

```text
(0, 0,          1, 2,  0, 50, 0) x47
(0, 0,          2, 1, -1, -1, 0) x1   package 38933
(1, 0x03510000, 1, 2,  0, 50, 0) x1   package 40451
(1, 0x00ee0000, 1, 2,  0, 50, 0) x1   package 41790
(1, 0x067c0000, 1, 2,  0, 50, 0) x1   package 41815
(1, 0x761f0000, 1, 2,  0, 50, 0) x1   package 41925
(1, 0x08b10000, 1, 2,  0, 50, 0) x1   package 51326
```

Thus a host which hard-codes the dominant `orders=1, queued=2, form=0, width=50` shape is
already refuted by this single recording.

### Empty selection chronology

Ten Group commands have `num=0`. They reuse the last explicit selection for the same
owner, across package boundaries and across unrelated actions. The left package points to
the last preceding explicit Group package and its wire candidate count:

```text
6145  <- 6123  (1)
14153 <- 14138 (1)
38933 <- 38912 (4)   preceding action was Build
40451 <- 40439 (4)
41925 <- 41918 (2)
42169 <- 42147 (1)   preceding action was Attack
42188 <- 42147 (1)   repeated empty does not replace the cache
47024 <- 47008 (67)  preceding action was Attack
48621 <- 48604 (5)
50138 <- 50044 (13)  preceding action was Attack
```

The wire only stores `o`. The command host must retain the exact retail cache identity
`(o, uid)` per `who`, and an empty Group must revalidate both UID and aliveness in the
canonical World. A stale or recycled `o` is skipped. An empty Group does not overwrite the
cache, which the repeated 42169/42188 pair makes mutation-sensitive.

Two compact all-row digests prevent a representative fixture from hiding a corpus drift:

```text
<u32 package_index0><u32 frame><i32 package_serial>
<u16 group_len><Group><u16 move_len><MoveTo>
    2,753 bytes, SHA-256 a6da97b0cfa65677e46520b7be7c67152dcef93e05504e87da606bfce812b48f

<u16 complete_payload_len><complete decoded package payload>
    2,140 bytes, SHA-256 f14a3c09b04b4df3f375989fe6f0746d153cc73131c3fd8f34bf38b48de0765c
```

## Exact fresh-save Groups image

Scanning all 4,191 occurrences of `u32 512` in the decompressed save produces exactly one
valid 512-record `Array<Group>` under the recovered retail grammar.

```text
Array<Group>             0x4610b
length,size              512,512
increment,flags          -1,0
records                  0x46116..0x4f1fa
records SHA-256          5555f94c193c9df5838e1daf2a125e3fe2d4b1ad5acbba721ed2d54182c40926
walk_test("groups")      0xef at 0x4f1fa
last_group[8]            0x4f1fb [4,65,130,192,256,320,384,448]
proc_group               0x4f21b 47
section end              0x4f21f
section SHA-256          401ae136297c2028c0a21b72cd7a26b768a0910c556e6958eb41cc32782faf17
```

All 512 `Group.id` values equal their physical global pool slot. All 12 nonempty groups
satisfy `who == slot / 64`, and all are singletons:

| slot | who | owner-local `o` | stamp | form | order_num | buildings |
|---:|---:|---:|---:|---:|---:|---:|
| 0 | 0 | 1 | 0 | 0 | 1 | 0 |
| 1 | 0 | 16 | 1055 | 0 | 1 | 0 |
| 2 | 0 | 0 | 1087 | 0 | 1 | 0 |
| 4 | 0 | 14 | 1145 | 0 | 1 | 0 |
| 6 | 0 | 2 | 1030 | 0 | 1 | 0 |
| 64 | 1 | 0 | 1110 | 0 | 1 | 0 |
| 65 | 1 | 1 | 1124 | -1 | 1 | 0 |
| 128 | 2 | 0 | 137 | 0 | 7 | 0 |
| 129 | 2 | 2000 | 951 | -1 | 0 | 1 |
| 130 | 2 | 2005 | 1151 | -1 | 0 | 1 |
| 192 | 3 | 2000 | 926 | -1 | 0 | 1 |
| 194 | 3 | 0 | 756 | 0 | 1 | 0 |

The owner-local values 2000 and 2005 are especially useful guards against accidentally
treating the Group list as global pool slots. The three building rows are selected but
idle. The Groups section does not serialize Unit OrderLists, so `order_num` is not evidence
of a concrete active Move payload.

`Game.frame` is 1199 at `0x4d3`, and `1199 % 64 == proc_group == 47`. This independently
confirms that `proc_group` is live save/resume state, not reconstructible decoration.
`last_group` is the canonical global-slot allocation state shared by command selection,
checksum, and save; a second command-side owner is not admissible.

## Canonical package transaction

The narrow host should be owned by `Sim`, because only `Sim` simultaneously owns the
canonical World, `groups_guys::Groups`, ticking, OrderLists, RNG, and DoNSave:

```rust,ignore
impl Sim {
    pub fn process_command_package(
        &mut self,
        play: i32,
        stamp: u32,
        payload: &[u8],
    ) -> Result<PackageReceipt, PackageError>;
}
```

This signature intentionally has no `group` argument. The on-disk header's `group` is a
lockstep serial. The runtime Group scratch begins unset for each package and is populated
only by opcode 0.

The transaction has five ordered phases:

1. Decode the complete payload with exact variable command lengths. Reject truncation,
   residue, foreign opcodes, or any unsupported simulation mutation before changing state.
2. Snapshot/revision-pin every state owner a supported command can touch. Presentation
   prefixes may be routed separately, but must not clear the later Group scratch. A
   supported deterministic prefix such as PlayerSpeed participates in the same package
   transaction; an unsupported one rejects the package atomically.
3. For explicit Group, resolve each `(who,o)` against the canonical World, retain the
   retail `(o,uid)` cache image, and pin stable Handle/generation/revision facts for commit
   revalidation. For empty Group, consult that cache without replacing it and skip stale
   `(o,uid)` rows. `play` and `who` remain distinct fields even when a Player mapping is
   used to authorize the sender.
4. Plan canonical group allocation and MoveTo installation. Allocation mutates the one
   `groups_guys::Groups` pool, `last_group`, displaced old groups, and every touched
   Object's group backlink. MoveTo consumes all nine wire fields and installs the typed,
   lossless `MoveOrderState` in the tick-owned Unit OrderList. The existing approximate
   command-side `get_open_slot` is not authority: its still-open object predicates must be
   recovered before this phase is claimed exact.
5. Revalidate every pinned revision/digest, then commit all package mutations or none. A
   malformed Move tail may not leave a newly allocated Group, a changed cache, altered
   backlinks, or a prefix mutation behind.

The state ownership matrix is therefore:

| state | one owner | package responsibility |
|---|---|---|
| decoded command list / scratch | transient Sim transaction | whole-package parse; Group scratch lives only to package end |
| last explicit `(o,uid)` cache | Sim command host, per `who` | explicit replaces; empty reads only; required across packages |
| Group pool / `last_group` / `proc_group` | `groups_guys::Groups` | allocate/replace atomically; checksum and save see the same bytes |
| object liveness, UID, Handle, group backlink | canonical World | preflight and revision revalidation |
| Unit OrderList / Move payload | canonical World + typed order owner | lossless install, tick, v12 save/reload/resume |
| package prefix state | its canonical Sim subsystem | participate atomically or fail closed before mutation |

One additional save boundary is now explicit: after the canonical host exists, a DoNSave
made after an explicit Group but before a later empty Group must preserve the per-owner
`(o,uid)` cache, or must refuse that state. Groups and MoveOrderState alone cannot resume
that future command honestly.

## Required mounting tests

The new exclusive test proves the evidence and does not pretend the host exists. Mounting
is complete only when shared-owner work adds these tests against real `Sim` state:

1. Apply minimal explicit singleton package 6283 and witness one canonical group slot,
   exact `(who=0,o=1,uid,Handle)`, typed MoveOrderState, and next-tick movement.
2. Apply a prior explicit selection then empty package 6145; mutate UID or liveness and
   prove the stale member is skipped. Apply two empty packages and prove the cache source
   remains the last explicit package.
3. Apply the Camera-prefix fixture and the Camera+PlayerSpeed-prefix fixture; prove prefix
   processing neither clears Group scratch nor escapes rollback.
4. Apply package 38933 and all five facing variants; mutate each Move field separately and
   prove a changed order image or an explicit refusal. No default-tail inference.
5. Truncate or foreign-tag the Move tail after a valid Group and prove byte-identical
   Groups, cache, World backlinks, Unit OrderLists, RNG, and subsystem prefix state.
6. Save after install, reload, and resume through a real tick. Separately save after an
   explicit selection and prove a later empty Group resolves identically before/after
   reload.
7. Feed the browser's exact 27-byte `[Group(one owner-local o), MoveTo]` payload through
   this same API. No Wasm selection vector and no shadow Groups adapter are accepted.

Current evidence gates are in
`crates/don-replay/tests/retail_group_move_package_fixtures.rs`; compact metadata is in
`crates/don-replay/tests/fixtures/retail_group_move.rs`.
