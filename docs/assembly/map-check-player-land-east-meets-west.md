# East Meets West `Map::check_player_land`

## Result and boundary

This lane recovers the complete shipped `Map::check_player_land` leaf and the
East Meets West caller edge after all active-player starting locations have
been appended.  It executes the native call at `0x00697492`, returns to
`0x00697497`, and stops at a typed local-string-cleanup residual.  The leaf
consumes no RNG.

Evidence is the shipped executable
`ron-bin/riseofnations.exe` (SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`),
`ron-bin/sbl/rise.pdb`, GNU i386 disassembly, the existing Ghidra control-flow
view, mutation-sensitive synthetic fixtures, and two independent real
style-19 headers.  No live process was touched and no recorded checksum was
used to select a produced byte.  This is fidelity Tier C: exact PE/PDB
transcription with executable local tests, not a retail process differential.

## Native extent and ABI

The PDB names `?check_player_land@Map@@SAXHHHH@Z` at `0x0068ef00`, with a size
of 1,145 bytes.  The exact half-open extent is
`0x0068ef00..0x0068f379`, containing 301 decoded instructions; its raw body
SHA-256 is
`9577c20dbe3e6cd13409355c2f45983cbc01e573c30e3b0e16958967fb109140`.
The sole native return is the bare `ret` at `0x0068f378`; stack cleanup remains
with the caller.

East Meets West reaches the leaf with this exact sequence:

| VA | operation |
|---:|---|
| `0x00697484` | load the style's `Map*` local |
| `0x00697487` | push the current ECX value; formal argument four is unread |
| `0x00697488` | push `-1`; the leaf resolves this to radius five |
| `0x0069748a` | set ECX to `1`, enabling the foreign-continent exclusion scan |
| `0x0069748f` | load EDX from `Map+0x30` (`avoid_continent`, 16 for both fixtures) |
| `0x00697492` | call `Map::check_player_land` |
| `0x00697497` | pop the two stack arguments in the caller |

Thus the represented call is
`check_player_land(1, 16, -1, unread)`.  The leaf caps the second argument to
ten and resolves the negative third argument to five.

## Complete body schedule

The body visits the parallel `World::start_city_x/y` arrays, not the single
centre coordinate arrays.  `World::add_starting_location` contributes four
footprint coordinates per active player.

For every footprint, retail performs these operations in order:

1. Read the footprint cell's land-region id.
2. If it is zero, scan `move_x/move_y[1..49]` in shipped order for the first
   in-bounds region in `1..=63`.
3. When found, visit `move_x/move_y[0..9]`.  For every in-bounds cell, zero the
   16-bit `land/land_sub` pair, call `Array<WCoordData>::remove` at
   `0x0068f089 -> 0x0046d4f0` on the evolving local region, decrement its
   nonzero `Region::size`, append to the target region, increment target size,
   write `WData::region`, and replace the local region with the target.  That
   evolving-region behavior intentionally preserves retail's observable
   size/list mismatch.
4. Visit the combat-circle band `[ring_end[0], ring_end[radius])`.  For every
   in-bounds candidate, scan `move_x/move_y[1..ring_count[min(avoid,10)]]` when
   exclusion is enabled.  Reject when a traversable neighbor belongs to a
   different region whose coordinate list contains more than four elements.
5. For an accepted candidate, zero `land/land_sub`, append it to the local
   region, increment `Region::size`, and write `WData::region`.

The body has one direct child call, the coordinate removal above.  Its two
indirect calls at `0x0068f0cf` and `0x0068f2ea` are the coordinate-array growth
virtual.  There is no call to `Random::get` and no read or write of a Random
object.

## The corrected ring-table defect

The earlier port generated ideal square rings.  That is not byte-compatible
with the executable: the shipped 441-entry tables contain anomalies in rings
8–10.  East Meets West passes 16 and the leaf caps it to ten, so the anomalous
suffix is live, not documentary.  In particular, shipped entry 288 is
`(-8,-16)` while a generated ring produces `(-8,-7)`.

`player_land.rs` now consumes the anomaly-preserving
`collision::{RING_COUNT,RING_X,RING_Y}` arrays.  The synthetic mutation test
places a populated foreign region only at the shipped `(-8,-16)` offset.  The
corrected port rejects the targeted outer candidate and leaves it ocean; the
old generated scan cannot see that foreign region and would accept it.

## Mutation ownership

The caller wrapper validates the post-start shape before entering the leaf:
`start_x.len == start_y.len == players` and
`start_city_x.len == start_city_y.len == 4 * players`.

On success:

- `World::start_x/y` and `World::start_city_x/y`, including their walked
  metadata, are read-only;
- the only changed World checksum section is `WData`;
- only `Region::size`, `Region::coords` contents, and coordinate-array capacity
  may change in the Regions owner;
- `Regions::{sea,land,coords}` remain unchanged;
- the incoming RNG word is returned exactly and the direct-RNG site list is
  empty.

The receipt retains full before/after states for every changed Region rather
than reducing the transaction to a count.

## Two real style-19 receipts

The inputs are independent corpus headers, not expected-output blobs:

| recording | file SHA-256 | seed | teams |
|---|---|---:|---|
| `Playback___2018.12.01_18_33_16__Sat_.rcx` | `bc2c1f1a8bfb4b7e0d83a3f2ff69fb18ead1f2041507f6b3e864a1a069ee6089` | `0x01e1c76d` | `[0,0,1,1]` |
| `Playback___2019.03.24_11_56_19__Sun_.rcx` | `dab1c282556642300a5bc153f1f432f417fa039b265b4d72cb5876dd643ec055` | `0x00bb378d` | `[0,1,1,0]` |

Both independently execute all four starting-location appends and then the
complete leaf.  Frozen generated receipts are:

| recording | RNG before/after | World | isolated WData | changed Regions `(id,size,capacity)` |
|---|---:|---:|---:|---|
| 2018 | `0x20481b28` / unchanged | `0x4eb610df -> 0x885c07e8` | `0x2eaaf2e7 -> 0x5292e9f0` | `(1,3164->3996,3164->6328)`, `(2,2787->3619,2787->5574)` |
| 2019 | `0x18a56595` / unchanged | `0x660c7097 -> 0x2a9f6868` | `0x69f054b2 -> 0x25d14c83` | `(1,2779->3611,2779->5558)`, `(2,2793->3625,2793->5586)` |

Each body visits 16 footprint entries, considers and writes 1,664 outer
candidates, performs no seed-patch transaction, and sees no foreign-region
rejection.  StartArrays remain byte-identical.  These values are regression
outputs of the binary-derived schedule; the recordings' later retail World
checksums were not used as targets.

## Typed residual and gates

After the void return, the native caller resumes at `0x00697497`, writes its
local cleanup guard at `0x0069749a`, and calls `String::close`
`0x00a1cf40` at `0x006974a1`.  The receipt exposes this as
`PostCallCleanup`; it does not pretend the rest of the style virtual ran.

Validation gates:

- local real-corpus test: both headers green;
- local mutation-sensitive anomaly fixture: green;
- Persvati clean-HEAD overlay body/ABI fixture: green, job
  `map-player-land-audit-v3-20260811T212336Z-98376-3674-a7bf776e8e5b`;
- Persvati anomaly mutation fixture: green, job
  `map-player-land-audit-v3-20260811T212416Z-99446-4125-a7bf776e8e5b`.
- frozen-file Persvati anomaly gate after the two-header receipts were pinned:
  green, job
  `map-player-land-final-20260811T213819Z-14845-19044-a7bf776e8e5b`.
