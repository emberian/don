# Replay post-`nubify_forest` transition frontier

## Result

`crates/don-replay/src/post_nubify_transition_frontier.rs` reconstructs the
next complete `Map::make` mutation transaction after
`TerrainGroups::nubify_forest` returns to `0x0068c090`:

1. the caller checkpoint with source token `0x1eb9`;
2. conditional `TerrainGroups::change_forest_base`;
3. conditional `TerrainGroups::change_mountain_base`;
4. conditional `TerrainGroups::change_coast_base`;
5. unconditional `TerrainGroups::fix_transitions`, including its three exact
   RNG-consuming `nubify_transitions` calls and both inline eight-neighbour
   expansion passes; and
6. the next caller checksum deadline, source token `0x1ebe`.

All persistent writes are the native two-byte `WData::{land,land_sub}` word.
All RNG calls use the main simulation `Random` and are returned in instruction
order. The adapter stages World and RNG state and admits only `WData` checksum
movement before commit. Replay-sourced walked bytes do not increase.

The six selected-tileset tuning words are not serialized in the replay at this
point. They remain a typed live-fact boundary bound to the exact World
checksum, incoming RNG state, caller checkpoint, executable identity, and
independently captured evidence digest. The adapter validates the evidence
shape but does not authenticate caller-carried hashes.

Primary proof sources are the shipped executable and PDB, cross-read through
`re/decomp-all/0068bc90.c`, `006a9a60.c`, `006a9c10.c`, `006a9dc0.c`,
`006a9f70.c`, `006a1f30.c`, and `006b3050.c`. Local retail disassembly fixes
the cold-block control flow, exact call sites, implicit ECX subtype values,
loop bounds, and little-endian two-byte stores where the decompiler is
ambiguous.

## Caller schedule

The schedule is fixed by `Map::make` (`0x0068bc90`):

| address | operation |
|---:|---|
| `0x0068c090` | resume after `nubify_forest` |
| `0x0068c0b4` | checksum-log call with source token `0x1eb9` |
| `0x0068c0c8` | load selected-tileset data pointer |
| `0x0068c0cd` | test signed forest-base word at `+0x618` |
| `0x0068c0d6` | conditionally call `change_forest_base` |
| `0x0068c0e0` | test signed mountain-base word at `+0x620` |
| `0x0068c0e9` | conditionally call `change_mountain_base` |
| `0x0068c0f3` | test signed coast-base word at `+0x62c` |
| `0x0068c0fc` | conditionally call `change_coast_base` |
| `0x0068c101` | unconditionally call `fix_transitions` |
| `0x0068c106` | caller resume |
| `0x0068c12a` | next checksum-log call, source token `0x1ebe` |

The three negative-base gates skip their complete callee, including every RNG
draw. Chance words are not gates: once a base callee runs, every eligible
neighbour consumes a draw even when chance is negative or zero.

## Typed selected-tileset facts

The code reads six signed 32-bit words behind pointer slot `0x00e885f0`:

| offset | use |
|---:|---|
| `+0x618` | forest replacement subtype and caller enable gate |
| `+0x61c` | forest neighbour chance |
| `+0x620` | mountain replacement subtype and caller enable gate |
| `+0x624` | mountain neighbour chance |
| `+0x62c` | coast replacement subtype and caller enable gate |
| `+0x630` | coast neighbour chance |

The replacement subtype narrows to the low byte on store. There is no clamp.
Chance comparison is `Random::get(0,0xffff) % 100 < chance`; values at least
100 accept every eligible neighbour and negative values accept none while
still consuming draws.

A production retail capture is admitted only when it carries:

- the shipped executable SHA-256
  `30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`;
- a nonzero capture digest;
- the exact pointer-slot VA and a non-null captured object address;
- the independently reconstructed shipped Constants checkpoint;
- caller checkpoint `0x0068c0b4` and token `0x1eb9`; and
- the exact staged World checksum and incoming main-RNG state.

Synthetic evidence is accepted only under `cfg(test)`. A replay alone cannot
cross this boundary: it proves the Rules checkpoint but does not contain these
six runtime words.

## Three base spreaders

All three functions scan X outer, Y inner and inspect four neighbours in native
N, E, S, W order. A matching source cell is immediately normalized to
`land = 0` and `land_sub = base as u8`. Each in-bounds neighbour then applies
these gates in order:

1. it must not itself match the source classifier;
2. its signed `land` byte must not equal deep ocean `2`;
3. draw `Random::get(0,0xffff)` and reduce modulo 100;
4. admit only when the remainder is strictly below the corresponding chance;
5. write `land = 0`, and, because the caller base gate is nonnegative,
   `land_sub = base as u8`.

The classifiers differ:

| callee | VA / return | source classifier | RNG call |
|---|---|---|---:|
| `change_forest_base` | `0x006a9a60` / `0x006a9c02` | `WData.flags & FOREST` | `0x006a9b7c` |
| `change_mountain_base` | `0x006a9c10` / `0x006a9dbf` | all 16 underlying `TData` blocker fields equal MOUNTAIN | `0x006a9d2d` |
| `change_coast_base` | `0x006a9dc0` / `0x006a9f62` | `WData.flags & COAST` | `0x006a9edc` |

The mountain classifier is the function at `0x006b3050`, not the coarse
`World::is_mountains` WData-bit reader. It visits the source W cell's 4x4 tile
block in ordinals 0 through 15 and returns true only if every tile has blocker
kind two. Confusing those functions silently changes both mutations and RNG
cardinality.

## `fix_transitions`

`TerrainGroups::fix_transitions` spans `0x006a9f70..0x006aa205` and has this
exact sequence:

```text
nubify_transitions(3)       call 0x006a9f7e
spread subtype 3 -> 2       inline
nubify_transitions(2)       call 0x006aa0b4
spread subtype 2 -> 1       inline
nubify_transitions(1)       call 0x006aa1fa
```

The implicit subtype is passed in ECX. `nubify_transitions` is
`0x006a1f30..0x006a2314` and contains two distinct scan machines.

### X-major pass

The first pass scans X outer and Y inner. A run includes only cells with:

```text
!(flags & COAST) && land == 0 && land_sub == subtype
```

At run length five or greater it draws at `0x006a1fcb`, reduces modulo three,
and tries:

```text
primary  = (x - 1, y - 1 - remainder)
fallback = (x + 1, y - 1 - remainder)
```

### Y-major pass

The second pass scans Y outer and X inner. Its threshold is four, not five. It
draws at `0x006a21d0`, reduces modulo four, and tries:

```text
primary  = (x - remainder, y - 1)
fallback = (x - remainder, y + 1)
```

Retail compares both second-pass loop bounds with `World::xs`. Generated maps
are square; the adapter admits square worlds and fails typed on a non-square
shape instead of reproducing an out-of-bounds native read.

For both passes a candidate is eligible only when it is in bounds, not COAST,
has `land == 0`, and has a strictly lower unsigned subtype. An admitted
candidate gets the two-byte write `(land, land_sub) = (0, subtype)` and resets
the run to zero. If both candidates reject, the RNG draw survives and the run
does **not** reset, so the next matching scan cell attempts another draw.

## Inline eight-neighbour expansions

After subtype three is nubified, every exact `(land=0, sub=3)` source scans the
native eight-neighbour order NW, N, NE, E, SE, S, SW, W. A neighbour is skipped
when it is ordinary ocean (`land` 1 or 2 with no WATERHALF) and is not flagged
COAST. Otherwise it is left alone only when it is already `(land=0, sub=3 or
2)`; every other admitted cell receives the two-byte word `0x0200`, meaning
`land=0, land_sub=2` on little-endian x86.

The subtype-two pass is identical except that already-allowed subtypes are 3,
2, or 1 and the native word written is `0x0100`. Flags, regions, ownership,
resources, TData, and all other WData fields survive these stores.

## RNG and checksum accounting

Every direct draw is receipted with call site, stage, scan coordinate,
candidate pair, pre/post LCG state, raw half-open result, remainder, and admitted
candidate. Base-spreader draws occur first in forest, mountain, coast schedule
order. Transition draws then occur in this order:

1. subtype-three X-major draws;
2. subtype-three Y-major draws;
3. subtype-two X-major draws;
4. subtype-two Y-major draws;
5. subtype-one X-major draws; and
6. subtype-one Y-major draws.

All work begins from `NubifyForestReceipt::random_state_after`. The caller
checkpoint itself consumes no RNG and writes no World byte. Exact or rejected
writes may be byte-idempotent, so receipts retain all native stores separately
from the count of byte-changing stores.

The only permitted checksum movement is `WorldSection::WData`. TData affects
mountain classification but is never mutated. Sourced walked bytes are carried
through unchanged because neither live tuning nor computed mutations become
replay bytes.

## Mutation and corpus proofs

`crates/don-replay/tests/post_nubify_transition_frontier.rs` freezes:

- exact caller/callee order and both checksum tokens;
- forest/mountain/coast draw chronology from seed one;
- low-byte subtype narrowing and source-before-neighbour stores;
- chance-zero versus negative-base RNG behavior;
- the five-cell X-major threshold and draw-before-candidate-gates behavior;
- native eight-neighbour spread and two-byte-only mutation scope;
- fail-closed checksum/RNG binding for stale live facts; and
- admission of independently captured fact shape joined to the shipped
  Constants checkpoint extracted from a real supported replay.

The corpus test is explicit about the join: the `.rcx` supplies the real Rules
checkpoint, not the six tileset words. Their capture evidence remains a
separate required input and its digest is caller-carried.

## Integration handoff

These files intentionally do not edit shared module or replay scheduling
owners. A convergence owner should:

1. register `post_nubify_transition_frontier` after the pending
   `nubify_forest_frontier` module;
2. pass the exact `NubifyForestReceipt` without reconstructing its RNG state;
3. produce the six-word live capture at checkpoint `0x0068c0b4` or resolve an
   independently proven immutable static source for the same selected object;
4. thread the returned World checksum and RNG state to source token `0x1ebe`;
5. replace the broad terrain-repair schedule row with the explicit conditional
   and transition call sites; and
6. stop before claiming the later progress/UI setup and resource-placement
   portion of `Map::make` is reconstructed.

The eventual focused command is:

```text
cargo test -p don-replay --test post_nubify_transition_frontier
```

Root convergence validated the isolated proof after replacing invalid direct
`World` equality assertions with a complete rollback snapshot and removing a
shadowed test binding:

- hbox debug: `replay-post-nubify-v2-20260809T224302Z-83631-7332-ef54ec2d1035`, 6/6 passed;
- persvati release: `replay-post-nubify-release-v2-20260809T224302Z-83630-11262-ef54ec2d1035`, 6/6 passed.

Neither validation job ran the retail executable.

## Exclusive files

```text
crates/don-replay/src/post_nubify_transition_frontier.rs
crates/don-replay/tests/post_nubify_transition_frontier.rs
docs/assembly/replay-post-nubify-transition-frontier.md
```
