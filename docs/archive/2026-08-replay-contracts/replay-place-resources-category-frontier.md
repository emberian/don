# Replay resource-category and placement-entry frontier

`crates/don-replay/src/place_resources_category_frontier.rs` is the registered
source proof extending the current `Map::place_resources` residual at
`0x00690225`. It owns the category cleanup/dispatch chain through `FISH` and
`GOODIES`, plus the exact deterministic prefixes of the two large placement callees.
It deliberately stops before their candidate-filter bodies.

Authority is shipped `ron-bin/riseofnations.exe`, SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`, its
GUID-matched `rise.pdb`, SHA-256
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`, direct
Capstone/`objdump -Mintel` instruction reads, and shipped
`ron-data/internal_strings.xml` plus `ron-data/mapstyles/*.xml`. The PDB names
`Map::place_resources` at `0x0068f4f0`, `Map::find_avail_regions` at
`0x0068f380`, `Map::place_region_resource` at `0x00690480`, and
`Map::place_player_resource` at `0x00691f70`.

## Exact category chronology

The native category ordinal is initialized once to zero and runs in this order:

| Ordinal | Category | Selected lookup | Default lookup |
|---:|---|---:|---:|
| 0 | `BONUSES` | internal string `+0x17a98` | `+0x17a98` |
| 1 | `FISH` | `+0x17aac` / call `0x0068f8f6` | `+0x17ac0` / call `0x0068f9b0` |
| 2 | `GOODIES` | `+0x17ad4` / call `0x0068f79b` | `+0x17ad4` / call `0x0068f855` |

The two FISH strings are observably different in the shipped internal-string file:
`+0x17aac` is `"FISH "` with a trailing space, while `+0x17ac0` is `"FISH"`.
The source preserves those distinct lookup facts instead of normalizing them, while
recording the returned element name separately as `FISH`. It does not mistake the
query token for the DOM element's name. Every category enumerates child `BONUS`
elements at call `0x0068fb98` using string offset `+0x17ae8`.

The shipped XML makes both later categories live, not merely theoretical. The default
has 2 FISH and 5 GOODIES rows. Selected files contain FISH/GOODIES row counts of
East Indies 2/2, East-West 2/absent, Great Lakes 1/absent, Himalayas 0/absent,
Mediterranean 2/6, and Old World 0/5. The source keeps absent and present-empty
sections distinct because they route differently through fallback.

After the last row, retail releases category tail/head and current-row tail/head at
calls `0x00690232`, `0x0069024c`, `0x00690266`, and `0x00690280`, increments the
ordinal at `0x00690290`, and dispatches again at `0x0068f754`. An empty category
falls from the row-count test at `0x0068fba8` directly back to `0x00690225`; a
nonempty one enters the shared row body at `0x0068fbb3`.

The chance-group key `[ebp-0x90]`, signed chance budget `[ebp-0x2c]`, and winner
flag `[ebp-0x28]` are initialized before the outer category loop. No instruction in
the category tail resets them. They therefore carry from `BONUSES` into `FISH` and
then `GOODIES`, together with the main RNG, World checksum, resource pool, placed
return count, and requested-resource counter. Resetting any of them per category is
not retail behavior.

After `GOODIES`, retail releases the selected/default document tail/head references
at calls `0x006902ad`, `0x006902c7`, `0x006902e1`, and `0x006902fb`, destroys the
remaining host XML temporaries, and returns the placed-resource count at
`0x00690476`. This cleanup consumes no RNG and writes no World bytes.

## Region placement prefix and the missing RNG call

`Map::place_region_resource` first classifies a concrete non-item good as water when
it occurs in `lands[2].rare`. It calls `Map::find_avail_regions` at `0x00690622`.
That helper scans:

- land regions `1..=63`, or water regions `64..=126`;
- only regions whose cell count is greater than five; and
- for pattern 3, excludes a region containing a player start coordinate.

`LinkList::add` at `0x0047afa0` prepends each accepted value. Because the region scan
is ascending, traversal begins in descending region-ID order.

Most importantly, `Map::find_avail_regions` itself calls the main
`Random::get(0, 0xffff)` at caller VA **`0x0068f498`** when more than one region is
available, and selects `raw % region_count`. This draw occurs before the previously
known region-point draw at `0x0069071a`. It is transitive to
`Map::place_region_resource` and is checksum chronology, not host noise. The existing
opaque placement receipt now admits the former only for the Region path and only as
the first placement-transcript draw; Player use, duplication, or a later occurrence
fails atomically.

After region selection, selector rows stop at the typed divvy-pool boundary
`0x006906ba`. Concrete-good rows read the selected region point count. Counts zero or
one consume no point draw; larger counts call `Random::get` at `0x0069071a`, choose
`raw % point_count`, and reach the candidate scan at `0x00690770`.

The prefix also freezes the caller-side region-pass rules:

- pattern 1 visits `available_region_count` passes;
- pattern 4 uses four passes;
- all other patterns use one pass; and
- for more than two requested resources with saturation zero, the per-region limit
  becomes `numrare >> 1` on the exact guarded paths in `0x00690638..0x0069065d`.

## Player placement prefix

`Map::place_player_resource` identifies only good 6 (Fish) and good 31 (Whales) as
water. It validates the first start coordinate against the World dimensions, stops
before the divvy-pool callback at `0x006920a0` for selector rows, and otherwise clamps
the spiral bounds:

- `group_spacing == 0` or `group_spacing > 64` becomes 64;
- `spacing` is reduced to at most that group spacing;
- `span = ring_end[group_spacing] - ring_end[spacing]`, using the shipped table at
  `0x00cbe330`.

When `span <= 0`, retail consumes no direct draw. For a positive span it calls the
main RNG at `0x00692114` and, unusually, takes `raw % (span + 1)`. The scan beginning
at `0x00692160` then reduces that value modulo `span`, so the first table entry has
two preimages. The source preserves this bias and leaves terrain, occupancy,
distance, allocation, and World writes at the typed candidate-scan residual.

## Proof and remaining boundary

`crates/don-replay/tests/place_resources_category_frontier.rs` freezes:

- exact FISH/GOODIES lookup tokens, call sites, order, fallback, and empty sections;
- category/document cleanup order and placed-count return;
- unchanged RNG/World/pool/counters and cross-category chance carry;
- water/land region domains, player-start exclusion, prepended list order, both
  ordered region RNG draws, and selector rollback boundary;
- player start validation, exact clamps, zero-span behavior, the `span + 1` RNG
  divisor, and selector boundary; and
- stale/incomplete evidence rejection before mutation.

Still external are the shared generic row adapter for FISH/GOODIES, divvy-selector
continuation in these exact prefix owners, both large candidate-filter bodies,
allocations, and caller checkpoint token `0x1ef7` at `0x0068c72d`. No checksum value
was fitted.

The canonical Map schedule now composes completed BONUSES cleanup through FISH
lookup/enumeration and, for a nonempty section, the exact first FISH row plus its
no-mutation recurrence through `0x0069021c`. Singleton FISH then composes cleanup and
GOODIES dispatch, then binds explicit document authority and executes selected/default
GOODIES lookup through row enumeration. It stops at `0x0068fbb3`/`0x00690225`. For empty FISH it composes this owner again through FISH cleanup and
GOODIES lookup/enumeration, stopping at `0x0068fbb3` or `0x00690225`. FISH recurrence,
later rows, and final GOODIES cleanup remain separately bounded.
