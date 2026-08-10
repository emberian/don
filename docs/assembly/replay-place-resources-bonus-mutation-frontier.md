# Replay `Map::place_resources` first `BONUS` mutation frontier

`crates/don-replay/src/place_resources_bonus_mutation_frontier.rs` continues the
source-only XML handoff at exactly `0x0068fb9d`. It owns one first-category row through
the iteration tail at `0x00690215`, including the first direct shared-RNG draw and the
transaction seam around the two large placement callees.

The authority is shipped `ron-bin/riseofnations.exe`, SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`, and its
GUID-matched shipped `rise.pdb`, SHA-256
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`.
The PDB names the enclosing procedure `Map::place_resources` at `0x0068f4f0`, size
3,977, and the callees named below. Direct `objdump -d -Mintel` reads cover every
address in this document.

## Exact row chronology

The previous owner leaves ordered `BONUS` elements and live XML document/category
handles at `0x0068fb9d`. Retail then:

1. zeroes the per-category winner flag and snapshots the row count at
   `0x0068fb9d..0x0068fbaa`;
2. assigns the first row to the global current element at `0x0068fbb3..0x0068fc0d`
   (old tail/head release calls `0x0068fbc0`/`0x0068fbda`, then new head/tail acquire
   calls `0x0068fbf7`/`0x0068fc0a`);
3. reads `type` with `XMLElement::get_attrib` at `0x0068fc30` and resolves it with
   `Types::good_key(..., 1)` at `0x0068fc5a`;
4. for a non-catalog name, executes the ordered `String::ignore` fallbacks at
   `0x0068fc81`, `0x0068fcac`, `0x0068fcd3`, and `0x0068fcfa`. They select one of
   three resource-pool selectors, the special item good `0x21f`, or skip the row;
5. reads `chance` and its bucket key with default `-1` at calls `0x0068fd27` and
   `0x0068fd40`;
6. conditionally draws `game_random.get(0, 0xffff)` at `0x0068fd64`, applies signed
   `% 100`, and subtracts `chance` at `0x0068fd98`;
7. only when the budget becomes negative, parses the placement fields and calls either
   `Map::place_player_resource` at `0x006900dc` or
   `Map::place_region_resource` at `0x006901ec`;
8. converges at the row iteration tail `0x00690215`.

At this exact first-row seam the last bucket key is `-1`, the budget is zero, and the
winner flag is false. Consequently a bucket key of `-1` consumes no direct draw and
starts from budget zero; any other key consumes exactly one draw. A key of zero also
forces a fresh draw on later rows, which remains for the next owner.

The draw is the main simulation stream reached through `[0x00c06184]`, not a private
map RNG. `Random::get` is the half-open retail implementation at `0x00a39d70`; the
receipt records its state before, raw result, signed `% 100`, and state after.

## Placement attribute projection

The owner freezes the ten `Map::scale_number` callbacks in retail order:

| Attribute | caller VA |
|---|---:|
| `numrare` | `0x0068fdcf` |
| `group_spacing` | `0x0068ff67` |
| `player_keep_away` | `0x0068ff9a` |
| `player_stay_near` | `0x0068ffc7` |
| `center_keep_away` | `0x0068fff4` |
| `center_stay_near` | `0x00690017` |
| `corner_keep_away` | `0x0069003a` |
| `corner_stay_near` | `0x0069005d` |
| `edge_keep_away` | `0x00690080` |
| `edge_stay_near` | `0x006900a3` |

The other reads are `pattern` at `0x0068fe35`, `saturate` at `0x0068ff21`, and
`spacing` at `0x0068ff45`. Retail maps the five recognized patterns to integers
`0..=4`; zero is the player path and every nonzero value is the region path. Negative
`saturate` becomes zero. Player-path negative `group_spacing` becomes zero, and both
player keep/stay-near values are clamped to at least one. The remaining scaled fields
are passed without a caller-side clamp.

After `numrare` is scaled at `0x0068fdcf`, a zero result jumps directly to the row
tail at `0x0068fdd7`. The winner flag has already been set and therefore remains true,
but neither the remaining placement attributes nor either placement body is called.

The typed placement request names every argument instead of relying on positional
intuition. It also binds the entry RNG state, complete World section checksum,
sourced/walked byte count, and six-field resource-pool digest.

## Opaque placement transaction

`Map::place_region_resource` (`0x00690480`, 6,883 bytes) and
`Map::place_player_resource` (`0x00691f70`, 3,833 bytes) are not disguised as completed
ports. `PlacementHost` is a two-phase boundary: it returns a receipt, and the caller
commits nothing until that receipt validates.

The receipt must carry the complete ordered RNG transcript. The only admitted direct
call sites in this cone are:

- region placement `0x0069071a`;
- player placement `0x00692114`;
- `ResourceDivvyPool::get_water` `0x0068a4cd`;
- `ResourceDivvyPool::get_late` `0x0068a59d`;
- `ResourceDivvyPool::get_early` `0x0068a66c`.

Every admitted draw is exactly `Random::get(0, 0xffff)` and must reproduce the retail
LCG state transition. Pool-call draws are rejected when the row did not select a pool
alias. For a catalog good, the pool digest must remain unchanged.

The same receipt records every object allocation. The placement call sites are:

| Path | Good allocation | Item allocation |
|---|---:|---:|
| region | `0x0069157c` -> `Objects::init_good` | `0x00691596` -> `Objects::init_item` |
| player | `0x00692d90` -> `Objects::init_good` | `0x00692db5` -> `Objects::init_item` |

`Objects::init_good` is `0x00653f30`; `Objects::init_item` is `0x00653e00`. Their
checksum-visible World writes are not left implicit:

- good: `WData.down = -2` at `0x0065403e`, then `down_who = slot` at
  `0x00654043`;
- item: `WData.down = -3` at `0x00653f05`, then `down_who = slot` at
  `0x00653f0c`;
- good type 5 deliberately performs no WData occupancy write.

The receipt binds raw `Coord` values to the exact World cell conversion
`div_3(coord >> 8)`, records both old words and both new words, and requires the World
checksum delta to be exactly section 5 (`WData`) when any occupancy write occurs. With
no occupancy write, the World checksum must remain identical. Runtime object arrays are
not part of the World channel, so allocation identity is carried separately rather than
invented as a World byte.

Rejected evidence, a broken RNG chain, an alien allocation callsite, a bad occupancy
marker, an unexpected checksum section, or a catalog-good pool mutation leaves the
local RNG/checksum/pool state unchanged. The host must itself be observational or
two-phase; an external side effect made while manufacturing a rejected receipt cannot
be rolled back by this adapter.

## Residual and remaining red work

`FirstBonusMutationReceipt` stops before executing `add esi, 0x28` at `0x00690215`.
It names the next edge as `0x0068fbb3` when another row exists, otherwise the category
tail at `0x00690225`.

Still red:

- schedule integration of the recovered later-row chance-bucket carry and zero-key
  forced-redraw owner;
- exact ports of the two placement bodies rather than typed receipts;
- the full resource-divvy bitmask mutation, candidate scans, and all transitive RNG
  draws inside those bodies;
- `GOODIES` and `FISH`, category/document cleanup, the final return count, and the
  caller checkpoint at `0x0068c72d`.

`crates/don-replay/tests/place_resources_bonus_mutation_frontier.rs` freezes the direct
chance hit/miss paths, the special no-draw `-1` bucket, unknown-type early exit,
resource-pool RNG/digest handoff, exact allocation/WData write, and atomic rejection of
mutated callee RNG and occupancy receipts. These files remain source-only and are not
wired into `lib.rs`; the shared integration lane should connect them only after running
the focused proof with the preceding XML owner.
