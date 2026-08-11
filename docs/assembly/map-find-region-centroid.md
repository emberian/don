# `Map::find_region_centroid` and the East Meets West caller loop

## Evidence and fidelity boundary

This lane recovers the complete 587-byte `Map::find_region_centroid` body at
`0x0068ae50` and its only caller, `MapEastMeetsWest::make_continents`, through
the caller-owned centroid arrays.  Evidence is the supported shipped image
`ron-bin/riseofnations.exe` (SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`),
`ron-bin/sbl/rise.pdb`, direct Capstone disassembly, the mechanically generated
`re/decomp-all/0068ae50.c` control-flow view, and both checksum-bearing style-19
replay headers.  No replay checksum word selected a branch or output.

The implementation is Tier C: exact PE/PDB transcription with
mutation-sensitive tests, but no executable differential.  It does not touch
the running retail process.

## Complete leaf

The PDB gives `Region` size `0x88`, `Region::size` at `+0x14`, `Region::coords`
at `+0x6c`, and the `WCoordList` data pointer at the resulting record offset
`+0x7c`.  `WCoordData` is `(WCoord x, WCoord y)`, two signed 32-bit words.
The body never reads its `Map *this`; it reaches `Regions::list` through the
global game pointer and uses the passed integer solely as a region index.

For a lawful positive region size `n`, retail:

1. consumes `n / 8` groups of eight coordinates with SSE2 `paddd`;
2. consumes `(n % 8) / 2` scalar pairs;
3. consumes the final coordinate when `n` is odd;
4. reduces both axes modulo 2^32;
5. performs signed `idiv n`, truncating each result toward zero;
6. writes X first at `0x0068b08c`, then Y at `0x0068b091`, and returns at
   `0x0068b098..0x0068b09a`.

The pointer-array length is never read.  `Region::size` is both the read bound
and divisor, so zero is a native divide fault and a short coordinate allocation
is an invalid native state.  The safe port refuses those malformed cases before
reading.  It intentionally ignores any suffix beyond `Region::size`.

The routine consumes no RNG and mutates no checksum-bearing state.

## Only caller and the next exact boundary

At `0x00696c65..0x00696d07`, East Meets West loops one-based over the number of
team continents.  Each call receives the region id, an X output pointer and a Y
output pointer.  The caller appends X to its first `SimpleArray<int>` and Y to
its second, preserving region order.

The exact continuation is:

| address | operation |
|---:|---|
| `0x00696c82` | first `find_region_centroid(region, &x, &y)` call |
| `0x00696d0a` | `eliminate_pools(EntireWorld, dead)` |
| `0x00696d0f` | `eliminate_edge_canals()` |

PDB exposes `eliminate_pools(Map::ElimPoolParam, int)`.  Direct inspection of
the complete 1,021-byte callee shows that it reads `[ebp+8]` (the enum) but
never `[ebp+0xc]`; the second integer is dead.  The enum pushed at the style-19
call site is literal `2`, `EntireWorld`.  `eliminate_pools` already has an exact
transactional port. `eliminate_edge_canals` is now an exact transactional body;
its four passes and mandatory region rebuild lead to the first
`Map::place_start_in_region` call at `0x00696fe3`.

## Replay evidence and residual

`crates/don-replay/tests/region_centroid.rs` regenerates the complete style-19
prefix from the two independently parsed retail headers and shipped
`eastwest.xml`, beginning from the exact post-tileset-selection main RNG word.
Both headers execute two centroid calls.  The 2018 header produces sizes
`[2777,2776]`, sums `[(218660,138867),(53145,130347)]`, X `[78,19]` and Y
`[50,46]`; the 2019 header produces sizes `[2778,2776]`, sums
`[(119816,58098),(146051,222666)]`, X `[43,52]` and Y `[20,80]`.  The differing
sizes are retained rather than normalized to the nominal 2,776-cell second-pass
target.  The test binds those arrays, the body schedule, literal pool mode,
dead-argument fact, and next external address without consulting the recorded
World checksum.

The first selector and first World append are now executed exactly; both real
fixtures return success. The remaining style-19 tail starts at caller return
`0x00697466`, before its stack bookkeeping. No part of that residual is
synthesized here. The admitted edge, selector and append bodies are documented in
`docs/assembly/map-eliminate-edge-canals.md` and
`docs/assembly/map-place-start-in-region-replay.md` and
`docs/assembly/world-add-starting-location-replay.md`.
