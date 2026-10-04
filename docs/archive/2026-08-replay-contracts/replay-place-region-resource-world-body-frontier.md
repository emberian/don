# Replay `place_region_resource` World-body frontier

`crates/don-replay/src/place_region_resource_world_body_frontier.rs` continues the
registered region prefix at `0x00690770` through the complete concrete-good,
selector-zero, pattern-1 (`World`) body of `Map::place_region_resource`. This is a
source-only owner until its long body is integrated deliberately. Its exclusive test is
`crates/don-replay/tests/place_region_resource_world_body_frontier.rs`.

Authority is shipped `ron-bin/riseofnations.exe`, SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`, and the
GUID-matched `ron-bin/sbl/rise.pdb`, SHA-256
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`.
The PDB names `Map::place_region_resource` at `0x00690480`, size 6,883,
`Objects::init_good(TypeIndex, Coord, Coord)` at `0x00653f30`, size 680,
`WorldData::has_mountain_tcoords` at `0x006b3050`, size 129, and
`Terrain::has_south_forest` at `0x00850060`, size 190. Direct Capstone/Intel
instruction reads settle argument order, branch polarity, integer rounding, the two
static neighbor tables, and every address below; the Ghidra C is used only as a
control-flow map.

## Owned chronology

The prefix has already consumed `Map::find_avail_regions`' conditional draw at
`0x0068f498` and the first region-point draw at `0x0069071a`. This owner replays and
validates both against the entering main-RNG state before doing anything else. Pattern 1
then visits exactly the number of available regions, following the circular linked list
from the randomly selected node. Since `find_avail_regions` scanned ascending IDs while
`LinkList::add` prepended, the list projection is descending, rotated at the selected
node. Every region performs exactly `num_rare` attempts.

Each attempt chooses its starting point with `Random::get(0, 0xffff)` at
`0x0069071a` when the region has more than one point. A rejected point advances with the
native one-based recurrence:

```text
if index & 1: index ^= 0x16800
index >>= 1
discard values greater than point_count
stop when the starting index recurs
```

That permutation consumes no RNG. The receipt records every attempted one-based index
and every filter observation, so a host cannot replace the native scan with a random
retry or linear walk.

The filter order is exact:

| Stage | Retail VA | Exact gate |
|---|---:|---|
| Start occupancy | `0x006907a0` | primary WData flag `0x1000` |
| Player keep-away | `0x006907b7` | reject distance `< threshold` |
| Secondary WData | `0x006908b7` | flags `0x10`, `0x68`, `0x1800`, then occupant byte |
| Player stay-near | `0x006908f3` | reject any start distance `> threshold` |
| Center keep/stay | `0x006909c2`, `0x00690a80` | `< keep`, `> stay` |
| Edge keep/stay | `0x00690b31`, `0x00690b78` | minimum of x, y, width-x, height-y |
| Corner keep/stay | `0x00690bb9`, `0x00690ca4` | nearest of four `(0/1)*width,height` corners, seeded at 9,999 |
| New-group spacing | `0x00690d93` | reject distance `< group_spacing` |
| Existing-resource spacing | `0x00690e4c` | active matching land/water goods, excluding good 5 |
| Land/water neighborhood | `0x00691098`, `0x00691158` | first 8 or all 24 shipped offsets |
| Mountain | call `0x006911c7` | named callee `0x006b3050` |
| Border | `0x006911db` | x/y zero or x/y equal to **width** minus one |
| South forest | call `0x00691207` | named callee `0x00850060` |
| Terrain rare list | `0x00691214` | derived terrain class contains the good ID |
| Down chain | `0x00691334` | complete object chain terminates at `-1`, not below |
| Saturation owner | `0x0069137f` | after the native attempt limit, nearest start equals current preferred owner |

The border's second maximum comparison really uses width rather than height at
`0x006911f7`; the owner preserves that instruction-level fact. The repeated distance
calculation is also kept byte-semantically: `max + min²/(2*max)` while `min < 60000`,
otherwise `(min + 2*max) >> 1`, using wrapping unsigned intermediates.

The two shipped neighbor arrays are at `0x00adc404` and `0x00adcaf4`. Instructions add
the first array to candidate y and the second to candidate x, so the receipt's `(dx,dy)`
order begins `(-1,-1), (0,-1), (1,-1), ...`; merely zipping them in address/name order
would transpose the chronology. Corner multipliers at `0x00add2a0` / `0x00add2b0` are
`(0,0), (0,1), (1,0), (1,1)`.

## Good, World, pool, and carry receipts

An accepted normal good reaches `Objects::init_good` at caller VA `0x0069157c`. Its
coordinates are `cell * 0x300 + 0x120` when good-data flag `+0x234 & 1` is set and
`cell * 0x300 + 0x180` otherwise. The two-phase Good receipt binds the exact type,
coordinates, returned slot, prior Good projection digest, prior World checksum, and
source-walk byte count. Except for good 5, `Objects::init_good` writes WData `down = -2`
at `0x0065403e` and `down_who = slot` at `0x00654043`; the accepted checksum delta must
be exactly `WorldSection::WData`. Good 5 has no WData occupancy write and must leave the
World checksum unchanged.

Every successful allocation is added immediately to the live existing-good projection,
and its WData down marker is visible to later attempts. The per-start distance sums are
updated after placement, and the preferred start is the first strict maximum, matching
the native zero-initialized nine-element accumulator and strict comparisons.

The function returns the placed-coordinate-list length at `0x00691f52`; the caller adds
it to its placed count at `0x006901f1`. The owner applies that accumulation to
`DeterministicResourceState::allocated_resources`. It carries the main RNG, final World
checksum, and Good digest forward while proving that the resource-pool digest, sourced
walked bytes, requested-resource count, `last_chance_group`, signed chance budget, and
winner flag are unchanged. Consequently a BONUSES placement can flow into FISH and
GOODIES without a hidden category reset.

## Boundary and tests

Still external are selector-backed pool transactions, item good `0x21f`, region patterns
0/2/3/4, and the player-placement body. Those paths reject before mutation with typed
errors rather than borrowing pattern-1 behavior. Host allocation is observational: an
unavailable or malformed Good/World receipt rejects atomically, even after the complete
candidate scan. No checksum value is fitted, and no VM or live process was used.

The focused test freezes the full two-region circular walk, all repeated RNG draws, four
Good allocations and WData deltas, cross-category carry, the exact LFSR retry after a
shadow-flag rejection, static tables, and atomic refusal of stale prefix/evidence,
unsupported paths, and a corrupted down write.
