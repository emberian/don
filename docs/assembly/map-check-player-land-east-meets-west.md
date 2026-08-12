# East Meets West `Map::check_player_land`

## Result and boundary

This lane recovers the complete shipped `Map::check_player_land` leaf and the
East Meets West caller edge after all active-player starting locations have
been appended.  It executes the native call at `0x00697492`, returns to
`0x00697497`, executes the exact caller-local `String::close` cleanup, and
executes the first centroid-array `_free` at `0x006974c2` plus its caller-local
destructor bookkeeping, then executes the centroid-X `_free` at `0x00697502`
and the complete style-virtual epilogue through `ret 4` at `0x0069753c`.
Execution then runs the common driver's full `Regions::clear_all` and
`Regions::find_all` bodies and all six World territory-limit stores, follows
the style-19 fallthrough, executes complete `Map::fix_diag_land` at
`0x0068be75 -> 0x0069c250`, constructs the caller-local `map.cpp` String at
`0x0068be82 -> 0x00a1d660`, executes the typed `GameLog::say_checksum` host
observation at `0x0068be9e -> 0x00930b30`, clears the caller cleanup guard,
and freezes before the sole-owner `String::close` at
`0x0068bead -> 0x00a1cf40`. None of these tranches consumes RNG.

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

The receipt retains full before/after states for every changed Region and an
exact `(x,y,before,after)` record for every changed `WData` cell rather than
reducing either owner transaction to a count.

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

## Exact `String::close` and array cleanup

After the void return, the native caller resumes at `0x00697497`, writes its
local cleanup guard at `0x0069749a`, and calls `String::close`
`0x00a1cf40` at `0x006974a1`.  PDB gives `String` a 20-byte layout and names
the called body at `0x00a1cf40` with size 79.  Its exact half-open extent is
`0x00a1cf40..0x00a1cf8f`, 36 instructions, SHA-256
`80b55b224beca72346bcb001fb415310611060b52a0c49c8db67c66c71fbae7f`.

The local is not an invented empty string.  At `0x006967be..0x006967cc` the
same caller reads `[[int_str_array]+0x10] + 0x17660` and passes that 20-byte
entry to `String::operator=`.  `0x17660 / 0x14 = 4792`; shipped
`internal_strings.xml[4792]` is exactly `MapGen:EastMeetsWest enter`, declared
hash `39143929`, 26 UTF-16 units.  The XML-loaded table entry is heap-backed;
assignment adds the local `StringGuts` reference.  `String::close` therefore
takes its reference-decrement arm.  Its only child call, the conditional
`StringGuts` scalar deleting destructor at `0x00a1cf71 -> 0x004d3e90`, is
unreachable for this just-acquired table lease.  Assignment plus close leaves
the table's reference count unchanged, while the local data pointer, offset,
length, and both cached hashes are cleared.

The exact caller slice `0x00697497..0x006974c2` is 43 bytes / 11
instructions, SHA-256
`7d4bc060ac077efe0ba99a2900ff83acf9ab8f8887cccdfe5078b94ee17dc987`.
After the close it clears the EH guard at `0x006974a6`, loads the first
`SimpleArray<int>` list at `0x006974aa`, loads `__imp__free` from
`0x00ac5500`, restores the PDB-named `SimpleArray<int>` vftable
`0x00b22a60`, tests the list, and pushes it.  Argument order at the earlier
`Map::find_region_centroid` call identifies this first array as centroid Y:
the caller passes `&[ebp-0xdc]` first and `&[ebp-0xd8]` second, so the callee's
formal X result is `[ebp-0xd8]` and formal Y is `[ebp-0xdc]`.  The first
array's backing list is `[ebp-0x7c]`, part of the Y object based at
`[ebp-0x8c]`; the X list is `[ebp-0x98]`, based at `[ebp-0xa8]`.

The next exact half-open caller slice, `0x006974c2..0x00697502`, is 64 bytes /
12 instructions, SHA-256
`158e1582fb9c93e1485c5929ac0a4a3888878bca2b8c3b6b7f5182a9854674b4`.
PDB gives `SimpleArray<int>` size 28, with length at `+4`, size at `+8`, list
at `+16`, and flags at `+20`; its named destructor at `0x0041ff80` exhibits
the same clear schedule.  The caller calls `__imp__free` through IAT slot
`0x00ac5500`, pops the four-byte argument, and performs that inlined
`SimpleArray<int>::~SimpleArray<int>` bookkeeping for Y: list, size, length,
and flags are cleared at
`0x006974c7`, `0x006974ce`, `0x006974d8`, and `0x006974e2`, then the EH guard
is dismissed to `-1` at `0x006974e6`.  The caller loads the centroid-X list at
`0x006974ed`, restores its vftable at `0x006974f3`, tests it at
`0x006974fd`, and pushes it at `0x00697501`.  Both real receipts have nonempty
Y and X arrays, so execution reaches the still-unexecuted second `_free` at
`0x00697502`.

The final exact slice `0x00697502..0x0069753f` is 61 bytes / 14 instructions,
SHA-256
`8dfe4067651d56aabcf8819c42d9a569df1ac1d28cef3efe954f245aa38d2535`.
It calls the already loaded `__imp__free`, pops the four-byte argument, loads
the saved exception registration, restores EDI and ESI, then clears the X
list, size, length, and flags at `0x0069750c`, `0x00697516`, `0x00697520`, and
`0x0069752a`.  It restores `fs:[0]` at `0x00697531`, restores EBX and the stack
frame, and returns while popping the style virtual's single four-byte argument
at `0x0069753c`.  PDB's 3,839-byte
`MapEastMeetsWest::make_continents(int)` extent is exactly
`0x00696640..0x0069753f`, so no unmodeled instruction remains in the body.

## First common `Regions::clear_all`

The common `Map::make` caller invokes `Regions::clear_all` at
`0x0068be36 -> 0x00680060`, resumes at `0x0068be3b`, pushes an unread ECX word,
and invokes `Regions::find_all(int)` at
`0x0068be3c -> 0x0067eff0`.  PDB gives `Regions::clear_all` size 275; its exact
extent `0x00680060..0x00680173` contains 79 instructions, SHA-256
`86333bac51dacb209b15048ecb33aa57b77c2d143a1ad797631e1992994b7317`.

The body visits all 128 fixed 136-byte `Region` records.  Every record gets
flags, climate, and goodies zeroed and common/goody factors reset to eight.
Only a record whose old size is nonzero enters the conditional destructor
path: size becomes zero; a non-null `WCoordList` allocation is released via
`0x006800c5 -> __imp__free` at `0x00ac5500`; list, capacity, length, flags,
borders, and border id are cleared.  Increment and cursor are preserved.  It
then zeroes `WData::region` for every World cell without touching `region2`,
sets land-region count to zero and sea-region state to 64, and returns along
the nonempty-World path at `0x0068014e`.

The receipt retains every Region before/after record, every changed World
region label, and a typed logical coordinate-allocation release for each
executed `_free`; no host pointer is stored or compared.

## First common `Regions::find_all`

PDB gives `Regions::find_all(int)` size 2,009. Its exact half-open extent is
`0x0067eff0..0x0067f7c9`, containing 558 decoded instructions; the raw body
SHA-256 is
`3e356054293f63e1be4b36681473d000c1ec5bd6e4eb3fa368d8fce98b2cc1cd`.
The formal four-byte stack argument is never read, and the body returns with
`ret 4` at `0x0067f7c6`.

The admitted body allocates its shared eight-byte-coordinate flood queue,
finds every land and sea component, retains retail's 62/63 and 125/126
overflow consolidation rules, fills `region2` for half-water cells, and calls
`set_coastals`, `sort_regions`, `rebuild_coords`, and `do_all_non_input`. It
then releases the shared queue through `0x0067f788 -> __imp__free` and returns
to the caller. Both real East Meets West fixtures find two land components and
one sea component, make four total non-input pumps, and execute neither native
size-mismatch diagnostic path.

The receipt retains every Region record and every changed `region`/`region2`
label before and after. Its typed scratch receipt records the logical
`Unallocated -> Live -> Freed` lifecycle, requested element count and byte
width, but no host pointer. The caller's exact `push`/`call` slice
`0x0068be3b..0x0068be41` has SHA-256
`ff5c2df80ce0958c86143fb098cb30488633cc03971c09d64b1bf9f918a9b1da`.
The following read-only preparation loads at `0x0068be41` and `0x0068be47`
lead to the first World player-territory-limit store at `0x0068be4a`.

## World territory-limit stores

The exact caller tranche `0x0068be4a..0x0068be75` is 43 bytes and thirteen
instructions, SHA-256
`9cd159ea34e9aeebc982230cc0a6234d628c2b4588e0e01261611d85bf0cb5fa`.
It copies Map fields `+0x4c/+0x50/+0x54/+0x58/+0x5c/+0x60` into World fields
`+0x38/+0x3c/+0x40/+0x44/+0x48/+0x4c`, in order. The two real fixtures execute
all six stores with values `[44, 4, 4, 44, 4, 4]`; the stores are observable
instructions but idempotent over the already rule-seeded World Scalars.

The caller then compares map style to 23 at `0x0068be6b` and conditionally
branches at `0x0068be6f -> 0x0068c84a`. East Meets West is style 19, so the
branch is not taken and execution reaches the next native mutator call,
`0x0068be75 -> Map::fix_diag_land` (`0x0069c250`). The receipt binds every
source/destination field offset, load/store VA, before/after value, the exact
branch decision, unchanged RNG, and the World checksum on both sides.

## Complete diagonal-land repair

The admitted call executes the complete call-free retail body
`0x0069c250..0x0069c458`: 520 bytes, 173 decoded instructions, return at
`0x0069c457`, SHA-256
`cf6610b0c5d5df3e5bfeb40010cdf729e587f69cdf1c3c3eaefd8c72c4fe65dd`.
It binds the shipped `corner_x` / `corner_y` tables at `0x00adc3c4` /
`0x00adc3e4` (`[-1,1,1,-1]` / `[-1,-1,1,1]`), the exact X-major in-place
scan, and every full WData record changed by the 16-bit `land = 2,
land_sub = 0` store. It consumes no RNG and changes no checksum section other
than WData. Execution resumes at `0x0068be7a`, performs the two caller-local
argument-preparation instructions, and freezes before the next native
mutation, `String::String(char const*)` at `0x0068be82 -> 0x00a1d660`.

## Post-repair diagnostic string

The next tranche executes the full 33-byte, 15-instruction constructor
`0x00a1d660..0x00a1d681` (SHA-256
`354be1ff3375e00afd53c7dd2ce92e7ebccba375f1ddc813fa9034dff6e629fe`)
and its nonempty `String::init_const` path over the shipped `map.cpp` literal
at `0x00adde58`. The typed receipt binds the complete `init_const`, `reinit`,
`get_string_guts`, and `char_to_wchar` native extents/call graph, the two
`MultiByteToWideChar` imports, and a logical owned `StringGuts` allocation—no
host pointer is stored or compared. The resulting local is the seven UTF-16
code units for `map.cpp`, offset zero, length/capacity seven, flags/module ID
zero, and zero lazy hashes.

Caller execution stores cleanup guard 4 and stages the exact log arguments
through `0x0068be9e`. This 28-byte, seven-instruction slice has SHA-256
`98eba84d611b7e7c9ed1a9244a4b1f9712ba709e36426e8e0439020d9382497c`.
It stages `GameLog::say_checksum` at `0x0068be9e -> 0x00930b30`, with line
number `0x1e9b`, mode 1, GameLog owner `0x00eb1360`, and the sole-owned local
`map.cpp` allocation passed by const reference. World and RNG are unchanged.

## Typed checksum-log observation

PDB gives `GameLog::say_checksum(int, const String&, int)` size 4,472. Its
exact half-open extent `0x00930b30..0x00931ca8` contains 1,379 decoded
instructions, returns with `ret 0xc` at `0x00931ca5`, and has SHA-256
`8e58ddabcd6742238aab95311529e9f615c5f322771f070c4805fbc48f2ef868`.
The mandatory `GameLog::check_accept` child at
`0x00930b6d -> 0x009309a0` is 390 bytes / 98 instructions with SHA-256
`bdc002281b2d9ee6024de7fbdb549e7580b87f7a7d6a469f96d91171fffe51f6`.

The checksum logger is a host observation, not a World mutator. It
temporarily writes checksum category `0x1e` to `GameLog+0x48`, writes mode 1
to `GameLog+0x4c`, invokes the acceptance/output schedule, restores the prior
category and any nonnegative prior mode, and unconditionally increments the
checksum sequence at `GameLog+0x68`. Acceptance, the virtual sink beginning
at `0x00930c25`, and the conditional frame rollover at
`0x00931c86 -> 0x0092f2d0` depend on live GameLog/game state not present in
offline reconstruction. The typed owner receipt retains those branches as
host-state-dependent observations; it does not invent their concrete output
or store a host pointer. The `map.cpp` const-reference owner is unchanged.

After return, the caller stores cleanup guard `-1` at `0x0068bea3` and loads
the local address at `0x0068beaa`. The exact call/cleanup slice
`0x0068be9e..0x0068bead` is 15 bytes / three instructions, SHA-256
`3f130942dec6f49dc4774ad3eacbcee60a43d3181698848f155a44d835a1ace0`.
Execution freezes before `String::close` because this local is the sole owner:
unlike the earlier internal-table lease, its close may enter the
`StringGuts` deleting-destructor and allocator-release path. No later caller
tail is inferred.

## Typed residual and gates

The canonical continent continuation now executes this receipt immediately
after the frozen remaining-start loop.  `ContinentStop::AddStartingLocation`
retains the complete wrapper and cleanup receipts, including a typed allocator
receipt that names the logical Y-coordinate allocation and its exact values as
`Live -> Freed` without storing or comparing a host pointer.  The final receipt
does the same for X and additionally binds every local clear, callee-saved
register pop, SEH restoration, frame restoration, and `ret 4`.  The common
clear, find, and territory receipts then bind the complete Region/World
transitions; the diagonal receipt binds the final WData mutation and the
constructor receipt binds the caller-local allocation and arguments; the log
receipt binds the exact source/mode/line owner and GameLog-relative effects.
It exposes `next_va = 0x0068bead`, `next_mutator_va = 0x00a1cf40`. Owner transition
accepts the result only when both centroid allocations, every cleanup anchor,
both sets of 128 Region transitions, every WData region label, the typed
scratch lifecycle, all six scalar stores, the style-19 fallthrough, and
unchanged RNG chronology match; its implementation
digest includes the replay executor plus the sim Region and map-terrain
bodies. The
offline localizer consequently names the two style-19 endpoints
`map_team_continent_post_checksum_string_close`.

Validation gates:

- local real-corpus/string-cleanup test: 4/4, including both headers and the
  shipped positional string-table binder;
- local mutation-sensitive anomaly fixture: green;
- Persvati clean-HEAD overlay body/ABI fixture: green, job
  `map-player-land-audit-v3-20260811T212336Z-98376-3674-a7bf776e8e5b`;
- Persvati anomaly mutation fixture: green, job
  `map-player-land-audit-v3-20260811T212416Z-99446-4125-a7bf776e8e5b`.
- frozen-file Persvati anomaly gate after the two-header receipts were pinned:
  green, job
  `map-player-land-final-20260811T213819Z-14845-19044-a7bf776e8e5b`.
- integrated local focused pack: continent reconstruction 4/4, both real
  style-19 player-land receipts 3/3, remaining-start continuation 2/2, edge
  canals 4/4, and initial-item reconstruction 4/4;
- integrated owner-transition suite: 2/2, including the 21-checksummed-replay
  coherent-ledger audit;
- integrated full localizer: 62 recordings opened, 21 checksum-bearing,
  21/21 coherent owner ledgers, 265,619/265,619 same-group comparisons, and
  exact endpoints of 2 post-player-land cleanup / 19 mountain-range-list
  inputs for `Mountains::randomize_mountains`;
- Persvati clean-HEAD overlay integration compile/body gate: green, jobs
  `map-player-land-bind-20260811T215247Z-39654-24602-7ea9e9ab5b69` and
  `map-player-land-bind-20260811T215429Z-42990-10682-7ea9e9ab5b69`.
- post-restart Hbox recovery gates: full owner transition 2/2 in
  `map-player-land-owner-full-hbox-20260812T003150Z-51674-1002-70f09f227b1d`,
  and the 62-opened-recording localizer census in
  `map-player-land-localizer-v2-hbox-20260812T003205Z-52756-15460-ed73570f9d4e`.
- current centroid-X-free local owner audit: 2/2, including all 21
  checksum-bearing recordings; current localizer: 62 opened, 21 checksum-bearing, 21/21 coherent
  ledgers, 265,619/265,619 same-group comparisons, and exact endpoints of two
  `map_team_continent_territory_limit_store` / nineteen
  `place_all_mountains_add_mountain`.
- current clean-HEAD Hbox overlay: final source compile plus full 4/4 suite,
  including both real fixtures, green in
  `map-player-land-string-close-20260812T010753Z-5869-9508-c8a3c250f8bf`
  and
  `map-player-land-string-close-real-20260812T010942Z-7944-10909-34e003e4734b`.
- current clean-HEAD Hbox centroid-Y-free overlay: full 4/4 suite, including
  both real fixtures, green in
  `map-player-land-y-free-real-20260812T012804Z-33056-25206-a937d193cd03`.
- current clean-HEAD Hbox centroid-X-free/complete-epilogue overlay: full 4/4
  suite, including both real fixtures, green in
  `map-player-land-x-free-real-v2-20260812T015625Z-75503-436-7cad6dcf92f8`.
- current Cycle 6 local pack: continent reconstruction 4/4, both real
  player-land/find-all receipts 4/4, remaining-start continuation 2/2, edge
  canals 4/4, and initial-item reconstruction 4/4;
- current Cycle 6 owner transition: 2/2, including all 21 checksum-bearing
  recordings; full localizer: 62 opened, 21 checksum-bearing, 21/21 coherent
  ledgers, 265,619/265,619 same-group comparisons, and exact endpoints of two
  `map_team_continent_territory_limit_store` / nineteen
  `place_all_mountains_add_mountain`.
- current Cycle 7 local pack: continent reconstruction 4/4, both real
  player-land/territory receipts 4/4, remaining-start continuation 2/2, edge
  canals 4/4, and initial-item reconstruction 4/4; owner transition 2/2,
  including all 21 checksum-bearing recordings; full localizer: 62 opened, 21
  checksum-bearing, 21/21 coherent ledgers, 265,619/265,619 same-group
  comparisons, and exact endpoints of two `map_team_continent_fix_diag_land` /
  nineteen `place_all_mountains_add_mountain`.
- current Cycle 8 focused real-fixture gate: both shipped style-19 fixtures
  execute the complete 520-byte diagonal repair receipt and freeze at the
  caller-local `String` constructor; cargo check, all-test compilation,
  continent 4/4, initial 4/4, and owner transition 2/2 are green. Full
  localizer: 62 opened, 21 checksum-bearing, 21/21 coherent ledgers,
  265,619/265,619 same-group comparisons, with exact endpoints of two
  `map_team_continent_post_fix_diag_log_string` / nineteen
  `place_all_mountains_add_mountain`.
- current Cycle 9 focused real-fixture gate: both shipped style-19 fixtures
  execute the exact constructor/helper receipt and freeze before
  `GameLog::say_checksum`; focused player-land/constructor 5/5, continent 4/4,
  initial 4/4, cargo check, and owner transition 2/2 are green. Full
  localizer: 62 opened, 21 checksum-bearing, 21/21 coherent ledgers,
  265,619/265,619 same-group comparisons, with exact endpoints of two
  `map_team_continent_game_log_say_checksum` / nineteen
  `place_all_mountains_add_mountain`.
- current Cycle 10 exact source/all-test compile is green. The focused pack is
  continent 4/4, player-land/checksum-log 6/6, remaining starts 2/2, edge
  canals 4/4, and initial-item reconstruction 4/4; both shipped style-19
  fixtures retain the exact line/mode/string owner and unchanged World/RNG.
  Owner transition is 2/2, including all 21 checksum-bearing recordings. The
  installed full localizer opens 62 recordings, finds 21 checksum-bearing,
  retains 21/21 coherent owner ledgers and 265,619/265,619 same-group
  comparisons, and names exactly two
  `map_team_continent_post_checksum_string_close` / nineteen
  `place_all_mountains_add_mountain` endpoints.
