# Replay `TerrainGroups::nubify_forest` frontier

## Result and honest stopping point

`crates/don-replay/src/nubify_forest_frontier.rs` freezes the exact caller-side
body of `TerrainGroups::nubify_forest(int)` at `0x006a93b0`, including both
World scans, every main-RNG draw, the ordered candidate gates, inline WData
normalisation, and the otherwise dead temporary coordinate arrays.

The first unresolved semantic dependency is
`World::is_edge_of_region(const WCoord&, const WCoord&, int, int)` at
`0x006b3180`. This pack does not enter or infer that callee. Each invocation is
an opaque, typed receipt supplied by an external producer and bound to the exact
request and evolving staged World checksum. A missing, mismatched, or invalidly
provenanced receipt fails closed before committing the authoritative World or
returning an advanced RNG handoff. Effects in a mutable external host are not
part of that rollback.

This is a receipt-chained continuation of `Map::check_player_forest`, not a new
replay-source boundary. It consumes no new serialized bytes and does not move
the existing `InitialItemBoundary`.

## Native boundary and evidence

| address | operation |
|---:|---|
| `0x0068c08b` | `Map::make` calls `TerrainGroups::nubify_forest` |
| `0x006a93b0` | callee entry |
| `0x006a93b0..0x006a9471` | construct two temporary `SimpleArray<int>` objects |
| `0x006a9472..0x006a96bb` | column pass: X outer, Y inner (half-open) |
| `0x006a96bb..0x006a993e` | row pass: Y outer, X inner (half-open) |
| `0x006a993e..0x006a99bb` | free Y scratch, free X scratch, return |
| `0x006a99b9` | hot-path `ret 4` |
| `0x006a99bc..0x006a9a50` | cold direction-selection blocks reached from the two passes |
| `0x006a9a51` | half-open PDB function extent (1,697 bytes) |
| `0x0068c090` | caller resume |

The PDB signature is `void TerrainGroups::nubify_forest(int)`, at
`terraingroups.cpp` lines 3774–3902. Neither `this` nor the sole stack argument
is read; the argument at this caller is merely an arbitrary live ECX value.
There is no semantic return value. Persistent input comes from the global World
and main RNG.

Primary proof sources are:

- `re/decomp-all/006a93b0.c`, cross-checked against local retail disassembly;
- `re/decomp-all/0068bc90.c`, which fixes the call and caller schedule;
- the shipped PDB symbol/signature/line extent above;
- `Random::get(int,int)` at `0x00a39d70`; and
- the unresolved PDB callee symbol at `0x006b3180`, without inspecting or using
  its body.

The decompiler presents the physically late cold blocks poorly, so control-flow,
draw sites, and state survival were frozen from the instruction graph rather
than decompiler block order.

## Exact scan machines

Both passes visit only interior cells. Dimensions at most two make the relevant
range empty.

```text
column pass: x = 1 .. xs-2, then y = 1 .. ys-2
row pass:    y = 1 .. ys-2, then x = 1 .. xs-2
```

Each new outer-loop line starts with `direction = 0, run = 0`. A non-FOREST
current cell clears `run` but preserves `direction`.

In the column pass, east is direction `+1` and west is `-1`:

```text
if current is FOREST and the neighbour at retained direction is FOREST:
    run += 1
else if current is FOREST and east and west are both FOREST:
    run = 0
else if current is FOREST:
    run = 1
    direction = east ? +1 : west ? -1 : 0
```

The retained-direction condition is evaluated before the both-neighbours
reset. Thus both sides continue an already selected `+1/-1` run, but reset a
run whose retained direction is zero.

The row pass substitutes south `+1` and north `-1`. When neither neighbour
matches the retained direction it starts `run = 1`; if both south and north are
FOREST while direction is zero, retail consumes a direction draw and selects
odd `=> +1`, even `=> -1`. An existing `+1/-1` continues before this tie case.

At `run >= 4`, the pass attempts exactly one candidate and then resets only
`run` to zero, whether the candidate is accepted or rejected:

```text
column: offset = get(0, 0xffff) % 4
        candidate = (x + direction, y - offset)

row:    offset = get(0, 0xffff) % 4
        candidate = (x - offset, y + direction)
```

## RNG chronology

`Random::get(0, 0xffff)` is the canonical half-open draw and advances the LCG
once. The exact retail call sites are:

| call site | use |
|---:|---|
| `0x006a952e` | column four-cell-run offset |
| `0x006a9783` | row four-cell-run offset |
| `0x006a9a0f` | row both-neighbour direction tie |

All column draws occur first in X-major/Y-inner order. Row direction and offset
draws then interleave in Y-major/X-inner order. Every offset draw occurs before
the start-city, terrain, and region-edge gates. Offset reduction is the low two
bits because the draw is nonnegative. Direction reduction is draw parity.

The synthetic 8x8 proof fixture starts the RNG at one and freezes this exact
schedule:

```text
column offset: raw 22891 -> 3, state 0x3c88596c
column offset: raw 34266 -> 2, state 0x5e8885db
row tie:       raw   381 -> +1, state 0x8116017e
row tie:       raw 15044 -> -1, state 0xb4733ac5
```

## Candidate gate and opaque receipt

After the offset draw, both passes apply the same ordered gate:

1. reject when the candidate bit in `WorldData::start_city_locs` is set;
2. reject signed land 1 or 2 unless `WATERHALF (0x0100)` is set;
3. call `World::is_edge_of_region(&candidate_x, &candidate_y, 0, 0xffff)`;
4. reject every nonzero callee result; zero admits the write.

The opaque callee is called at `0x006a95d0` in the column pass and
`0x006a982d` in the row pass. `WCoord` is one signed 32-bit word. The receipt
freezes call site, callee VA, ordinal, pass, scan coordinate, candidate,
literal arguments, exact staged `WorldChecksum`, a copy of the unwalked
start-city mask, result, and typed producer evidence. The admitted evidence
forms are a retail executable/capture SHA-256 pair, an exact-port implementation
SHA-256 plus proof-document identity, or a nonempty synthetic fixture identity
available only under `cfg(test)`. These are caller-carried evidence fields; the
adapter validates their shape but does not independently authenticate hashes.
The host also receives an immutable reference to the staged World.

No body-level claim about `is_edge_of_region` is made here. In particular, this
pack does not substitute a guessed neighbour/region rule for its result.

## Persistent write and scratch behavior

On an admitted receipt, retail performs the equivalent inline write:

```text
new_land = old_flags & COAST ? 2 : signed(old_land)
if new_land >= 0:
    land = new_land
land_sub is not read or written
flags = (old_flags & 0xffc3) | FOREST
```

`COAST`, `ROCKS`, `MOUNTAINS`, and the previous forest-class bit are the cleared
`0x003c` field before `FOREST` is restored. `WATERHALF`, `ORIG_COAST`, and all
bits outside that field survive; negative non-coast land is not stored.
Regions, TData, ownership/resource fields, `land_sub`, and `forest_size` are
untouched.

The chosen side cell was already observed as FOREST. A canonical write can
therefore be byte-idempotent even though RNG advanced, the external predicate
was called, and a write was accepted. The mutation-sensitive test deliberately
uses a conflicting `FOREST | ROCKS | COAST | WATERHALF` sentinel: the admitted
write clears the conflicting class bits, converts coastal land to 2, preserves
WATERHALF and `land_sub`, and changes only `WorldSection::WData`. This sentinel
is a proof device, not a claim that valid generated worlds normally combine
those class bits.

After each admitted write retail appends candidate X to the first scratch array
and candidate Y to the second. The arrays are never read or exposed. They grow
logically `0 -> 4 -> 8 -> ...` by doubling and are freed Y then X. The receipt
records accepted writes in append order and the final logical capacity without
depending on Rust allocator capacity.

## Transaction, provenance, and external facts

The executor first validates the exact `CheckPlayerForestReceipt` handoff:
entry, return, caller resume, next VA, zero-draw invariant, unchanged RNG,
current stored checksum, sourced-byte count, and the upstream WData-only
checksum scope. World dimensions, WData length, and start-city mask length must
also agree.

All RNG and World work happens against local staged state. Success recomputes
the checksum, admits only WData movement, and commits. Any structural failure,
unavailable/mismatched/invalidly-provenanced opaque receipt, or checksum-scope
failure leaves authoritative World, Regions, stored checksum, sourced-byte
count, and the upstream RNG handoff unchanged. The mutable host can still have
observations or side effects from earlier requests; those are explicitly
outside this transaction. A production integration that needs atomic external
coordination must use a side-effect-free preproduced receipt stream or add its
own prepare/commit/rollback protocol.

There are **no new static Rules, Types, TerrainGroups-member, map-style, or
caller-argument facts**. Existing instruction-derived runtime type facts are
the World dimensions/indexing, `WData` fields and flag values, one-word signed
`WCoord`, bit-packed start-city mask, World checksum sections, and canonical
`Random`. The sole new external dependency is dynamic behavior: the opaque
`is_edge_of_region` result with typed, caller-carried receipt evidence. Sourced
walked bytes remain unchanged.

## Replay integration map

The exclusive source deliberately does not edit shared wiring. A later
convergence owner should:

1. add `pub mod nubify_forest_frontier;` to
   `crates/don-replay/src/lib.rs` (and optionally re-export its typed surface);
2. after `execute_check_player_forest`, construct an authoritative,
   side-effect-free
   `EdgeOfRegionHost` and pass the returned receipt directly to
   `execute_nubify_forest_frontier`;
3. thread `random_state_after`, `checksum_after`, and unchanged
   `sourced_walked_bytes` from the nubify receipt into the caller continuation;
4. preserve the caller checkpoint after resume `0x0068c090`: source token
   `0x1eb9` at the checksum-log call around `0x0068c0b4`; and
5. replace or split the stale `MAP_MAKE_SCHEDULE` `terrain_repairs` row in
   `crates/don-replay/src/map_style.rs`: pin `check_player_forest` to call site
   `0x0068c04d` with no RNG, pin `nubify_forest` to call site `0x0068c08b` with
   branch-dependent draws at the three frozen RNG sites, and add a shared
   schedule-owner test for both entries; and
6. stop again before claiming the conditional tail is reconstructed:
   `change_forest_base (0x006a9a60)`, `change_mountain_base (0x006a9c10)`,
   `change_coast_base (0x006a9dc0)`, then unconditional
   `fix_transitions (0x006a9f70)`.

Do not add an `InitialItemBoundary` or increment replay-source coverage for this
receipt-chained computation. There is intentionally no invented single
`next_va` after nubify: the caller performs a checksum checkpoint and then the
three rule-gated calls before `fix_transitions`.

After shared wiring, the focused gate is:

```text
cargo test -p don-replay --test nubify_forest_frontier
```

The authoring lane did not run that command or a build. Root convergence later passed both
independent profiles on 2026-08-09: hbox
`replay-nubify-forest-v2-20260809T221941Z-63141-29419-f4bdd2feb022` and persvati release
`replay-nubify-forest-release-v2-20260809T221940Z-63139-27559-f4bdd2feb022`, each with 8/8
tests and exit 0. The first compile attempt exposed only invalid test-level `World` equality;
v2 uses a complete rollback snapshot over WData, TData, dimensions, start-city state, and checksum.

## Exclusive files

```text
crates/don-replay/src/nubify_forest_frontier.rs
crates/don-replay/tests/nubify_forest_frontier.rs
docs/assembly/replay-nubify-forest-frontier.md
```
