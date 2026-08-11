# Replay `Map::place_player_resource` body frontier

## Scope

`crates/don-replay/src/place_player_resource_body_frontier.rs` owns the complete
shipped `Map::place_player_resource` body at `0x00691f70..0x00692e68`. It consumes
the registered player prefix, executes all later players and requests, composes the
registered Early/Late pool selectors, walks the full spiral candidate loop, applies
every terrain/occupancy/distance filter, and validates both Good and Item allocation
transactions.

The owner is source-only pending the small shared `lib.rs` registration hunk. It does
not touch the VM or live harness and does not fit a checksum value.

## Shipped authority

The evidence is the supported retail PE/PDB pair:

- executable SHA-256
  `30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`;
- PDB SHA-256
  `334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`;
- PDB procedure `Map::place_player_resource` at `0x00691f70`, size 3,833; and
- a Capstone 32-bit decode of those 3,833 PE bytes: 1,272 instructions ending at
  `0x00692e69`.

The direct calls frozen by the body are:

| Caller VA | PDB callee | Role |
|---|---|---|
| `0x006920b5` | `ResourceDivvyPool::get_early` (`0x0068a640`) | selector 2 |
| `0x006920bc` | `ResourceDivvyPool::get_late` (`0x0068a570`) | every other nonzero selector |
| `0x00692114` | `Random::get` (`0x00a39d70`) | spiral start |
| `0x0069285c` | `Terrain::has_south_forest` (`0x00850060`) | terrain veto |
| `0x00692c06` | `WorldData::has_mountain_tcoords` (`0x006b3050`) | mountain veto |
| `0x00692d90` | `Objects::init_good` (`0x00653f30`) | Good allocation |
| `0x00692db5` | `Objects::init_item` (`0x00653e00`) | Goody allocation |

The decompiler output at `re/decomp-all/00691f70.c` was used only as a flow map;
branch order, signed comparisons, constants, and call sites were settled from the
PE decode.

## Corpus reach

The seven shipped `ron-data/mapstyles/*.xml` files contain 31 explicit
player-placement rows: 18 `BONUSES` Early/Late rows and 13 Goody rows. Their unscaled
`numrare` bases total 54 across explicitly present rows. This is the largest missing
player/region resource subdomain after the World/region body.

The implementation covers both halves:

- Early/Late rows execute the existing exact pool-selection owner per attempt; and
- Goody rows use `good_id == 0x21f` and the Item allocation path.

## Exact chronology

Retail computes the water-class boolean once from entry-time `param_2`: only IDs 6
and 31 are water. A selector can replace `param_2` later without recomputing this
boolean. For every valid player and each `numrare` request, chronology is:

1. if selector is nonzero, call Early only for selector 2 and Late otherwise;
2. consume all transitive selector draws at `0x0068a66c` or `0x0068a59d`, marking
   pool bits before reading the selected good and clearing the lane after exhaustion;
3. clamp `player_stay_near == 0` or `> 64` to 64, then clamp
   `player_keep_away` down to that value;
4. compute `span = ring_end[stay_near] - ring_end[keep_away]`;
5. for positive spans, draw once at `0x00692114` using the unusual modulus
   `span + 1`; and
6. scan exactly `span` entries beginning at
   `(draw_remainder + scan_offset) % span + ring_end[keep_away]`.

The first concrete draw is re-executed and compared with the registered prefix.
Later concrete draws, and all selector-path player draws, are owned here. Candidate
rejections never draw. An invalid start skips its complete inner loop and proceeds
to the next player.

## Candidate filters

Each candidate receipt preserves executable order:

1. World bounds, start-bit occupancy, and primary `0x1000` occupancy;
2. the six distinct shadow masks `0x10`, `0x20`, `0x08`, `0x800`, `0x1000`, and
   `0x40`, followed by the shadow occupant byte;
3. distance from every player start, current-player outer radius, and strict nearest
   player ownership (`other_distance > own_distance`);
4. center, edge, and corner keep-away/stay-near tests;
5. south-forest veto, spacing from placements made by this invocation, and border;
6. spacing from active same-class Goods or Items (Good 5 is skipped; land/water
   matching uses the entry-time water boolean);
7. the four cardinal entries among the first eight native neighbor offsets for
   non-Items, then the mountain veto;
8. Good terrain-class membership or the separate Item terrain mask; and
9. exact WData down-chain traversal, accepting only terminal index `>= -1`.

Two retail quirks are explicit: edge distance uses `width - x` / `height - y`, and
the final border test compares both x and y against `width - 1`. The native land-ID
equality at `0x00692b57` is unreachable in the admitted domain because entry zero is
first clamped to 64.

## Allocation receipts and state

The body uses a two-phase host receipt. Good allocations bind call `0x00692d90`,
callee `0x00653f30`, the good-specific `0x120`/`0x180` coordinate offset, slot, Good
digest, and World result. Item allocations bind call `0x00692db5`, callee
`0x00653e00`, fixed `0x180` offset, slot, and Item digest.

Except for Good 5, a successful receipt must change only World section `WData` and
must carry the exact occupancy writes:

- Good: `0x0065403e`, `0x00654043`, marker `-2`;
- Item: `0x00653f05`, `0x00653f0c`, marker `-3`.

The staged transaction advances main RNG, concrete pool state and digest, World,
the appropriate Good/Item digest, and allocated count. Sourced bytes, requested
count, and all cross-category chance locals carry unchanged. Any stale evidence,
wrong prefix, invalid pool state, unavailable allocation, or malformed receipt
refuses the complete transaction atomically.

## Proof

`crates/don-replay/tests/place_player_resource_body_frontier.rs` freezes:

- full two-player Item placement and exact WData/Item receipts;
- Early pool draw(s) before the spiral draw on every attempt, including lane reset;
- selector-resolved Good allocation and Good-vs-Item digest isolation;
- invalid-first-start continuation;
- linear rejection scan without an extra RNG call;
- candidate filter order and exact call/return VAs; and
- atomic rejection for a corrupted allocation or prefix.

Persvati job
`replay-player-resource-body-20260811T183535Z-26130-25942-967e8f4b7b97`
passes all six tests.

## Remaining typed boundary

Shared integration is limited to registering the module in `don-replay/src/lib.rs`
and adapting the generic placement receipt to this concrete body receipt. Region
patterns other than the already-owned World path remain separate resource lanes.
