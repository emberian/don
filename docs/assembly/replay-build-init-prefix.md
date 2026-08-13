# Replay Build initializer scalar prefix

This lane owns one chronological prerequisite between canonical Build allocation and the
starting-City transaction: the scalar writes performed by the shipped constructor and
`Build::init` chain through the instruction immediately before `Object::add_to_world`.
It supplies terrain Z, stable current-TypeIndex authority, and the inherited Object body
resets without pretending that the later Wall/Build initializer or activation has run.

## Shipped boundary

| VA | PDB symbol | evidence used here |
|---|---|---|
| `0x0062f370` | `BuildData::BuildData` (425 bytes) | constructor sentinels and empty containers |
| `0x00472260` | `MiningList::MiningList` (131 bytes) | empty array, increment `-1`, `mtn = cliff = -1` |
| `0x00629740` | `Build::init` (1,544 bytes) | founder, wonder/dock, max-age XOR, original type prelude |
| `0x0063e9b0` | `Wall::init` (744 bytes) | delegates first to `Object::init` |
| `0x00647750` | `Object::init` (211 bytes) | inherited body reset, UID allocation, detector flag |
| `0x00662300` | `SubObject::init` (147 bytes) | identity/current type and encoded XYZ |
| `0x0064d8c0` | `Object::add_to_world` (1,028 bytes) | first unowned world/list mutation; exclusive stop |

The PDB type record fixes the relevant offsets: `o +0x0a`, `z_internal +0x0c`,
`x_internal +0x10`, `y_internal +0x14`, `ptype +0x18`, `uid +0x30`, and the inherited
Object body through `launching +0x44`. `SubObject::init` stores **all three** coordinates
as `coordinate ^ 0x63637`; the terrain query's result is not stored plain. The outer
Build prelude stores `leader[0xdc] ^ 0x66` at `max_age +0x83`.

`Object::init` consumes two virtual/type facts rather than deriving them from the numeric
TypeIndex: `SubObject::init` may OR flag `0x20`, while the object-mask query may OR detector
flag `0x40`. The adapter therefore requires both booleans explicitly. The ptype field is a
process-local pointer, so `BuildData` holds no invented address; the receipt owns the stable
current TypeIndex and the canonical spawn transaction later installs that value in the Sim
production runtime.

One constructor detail spans the standalone and enclosing bodies: `MiningList::MiningList`
first installs length/capacity `0/0` and increment `-1`, then `BuildData::BuildData`
immediately grows the embedded array capacity to **5** (the `+0xa0 = 5`, `malloc(0x28)`
suffix). The prefix receipt therefore reports the final empty header `(0, 5, -1, 0)` plus
`mtn = cliff = -1`; it does not confuse the standalone child return with the completed
BuildData constructor state.

## Exact API and honest stop

`apply_build_init_prefix(BuildData&, BuildInitPrefixRequest)` remains the explicit-receipt
leaf used by isolated tests and externally measured call chains.  The replay-exact entrypoint
is now `apply_build_init_prefix_from_terrain`: its request has no Z field, it resolves the
signed Coord-to-TCoord conversion and exact two-vertex height query from a coherent
`TerrainHeightAuthority`, and only then applies the same scalar writes.  See
`docs/assembly/replay-terrain-height-runtime.md`.

Both paths preflight the playable owner and installed type-table range, then apply only writes
that have occurred at the chosen boundary. They return `BuildInitPrefixReceipt` with:

- lifecycle stage `BeforeObjectAddToWorld`;
- object owner/id, current TypeIndex, snapped and encoded X/Y, terrain and encoded Z;
- UID, flags, max age;
- explicit null-launching and full constructor mining facts (length/capacity/increment/flags
  and both signed tail bytes).

The City link remains `-1`. Fields without a retail write before the barrier are preserved,
including `construct_hits`, `ever_seen`, `ever_seen_completed`, `stance`, and both
infiltration bytes. This matters because a zero-filled Rust default is not evidence for a
C++ field that the constructor has not initialized yet.

Both `init_complete()` and `activation_complete()` are false. The later
`complete_starting_village_build_from_terrain` owner now consumes the source-backed prefix,
performs `Object::add_to_world`, the remaining Wall/Build initializer and activation, and
installs the exact 491-byte Village walk authority atomically.  The target replay still lacks
the completed-worldgen height plane and remaining setup/frame-zero authorities, so this
module makes no first-checkpoint agreement claim.
