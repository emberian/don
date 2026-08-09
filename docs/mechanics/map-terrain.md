# The map: coordinate ladder, terrain, fog, and the `world` checksum channel

Lane: `map-terrain`. Module: `crates/don-sim/src/systems/map_terrain.rs`.
Checksum channel served: **`world`** — channel 12 of `CheckSums::check_all` `0x00936560`,
packet offset `+0x2d`.

Every claim below is **[measured]** on this Mac against `ron-bin/riseofnations.exe`
(sha256 `30478a44…625079`) and `ron-bin/sbl/rise.pdb`, unless marked **[reported]**.
Nothing here comes from community documentation. Fidelity tier is **C** throughout —
behaviourally faithful, structure and constants read off the binary — with **no oracle
differential yet**; see §9.

---

## 0. Headline

We had no map at all. We now have the whole world representation, and it is small: the
sim's terrain is **two integer arrays and four byte planes**.

| | |
|---|---|
| **The coordinate ladder is closed and self-consistent.** | 1 tile = 192 fine units; the W cell = 4×4 tiles = 768 units; fog = 2 tiles; region = 8 tiles; the pathfinder's movement grid = 48 units = ¼ tile. All five conversions are one shift plus one `div_3_table` lookup. |
| **Map generation is deterministic from a single 32-bit seed — yes.** | `Map::make` `0x0068bc90` writes the seed into `World::seed` **and directly into `game_random`'s LCG state word**. Worldgen then runs on the main simulation RNG stream. |
| **Worldgen carries no IEEE hazard.** | 38 float instructions in 26,430 across all of `map.cpp`, all IEEE-exact binary32 (`cvtdq2ps`/`mulss`/`movss`/`comiss`), and **zero calls to any of the eight `_libm_sse2_*_precise` transcendentals**. |
| **Generated terrain is integer by construction.** | The entire `World::set_*` write API is **0 floating-point instructions over 1,929**, and `WData`/`TData` have no float field. The quantisation boundary *is* the setter API. |
| **There is no sim-side heightmap.** | `Terrain::master_land_heights : SimpleArray<float>` lives in the render class and never enters the checksum. The sim's only vertical structure is the discrete cliff/mountain bits. |
| **But four `Terrain` fields *are* lockstep-critical.** | `World::walk_data` sections 10–13 checksum `Terrain::{halfland_locs, halfland_types, halfland_subtypes, nuke_hits}`. New finding; see §6. |

23 unit tests, all passing (§8).

---

## 1. The coordinate ladder — solved, and it explains the pathfinder

`Coord`, `WCoord`, `TCoord` are each a `class` wrapping one `int` (PDB: size 4, single
field `value`). `FCoord`, `UCoord`, `RCoord` exist in the type stream but every method is
inlined, so they were recovered from their *use sites* instead.

The conversions are **not** plain shifts. The engine indexes a runtime table
`div_3_table` (`0x00cae5fc`) with a power-of-two arithmetic shift of the `Coord`:

```
Coord::operator WCoord  0x00461460 :  sar ecx,8 ; mov ecx,[div_3_table + ecx*4]
Coord::operator TCoord  0x0046cd40 :  sar ecx,6 ; mov ecx,[div_3_table + ecx*4]
```

`init_coord_lookup_array` `0x00681db0` fills that table with `t[i] = i/3` for `i ≥ 0` and
`t[j] = (j-2)/3` (C truncation) for `j < 0` — which is **`floor(j/3)` in both halves**.
So each conversion is `floor(c / (2^k · 3))`:

| class | shift | Coord units per cell | in tiles | role |
|---|---:|---:|---:|---|
| `Coord` | — | 1 | 1/192 | position unit; `Guy`/`Unit` coordinates |
| `UCoord` | 4 | **48** | ¼ | the pathfinder's 8-connected movement grid |
| `TCoord` | 6 | **192** | 1 | `TData` — the terrain bit grid |
| `FCoord` | 7 | **384** | 2 | the three fog planes |
| `WCoord` | 8 | **768** | 4 | `WData` — land, region, owner, collision |
| `RCoord` | 9 | **1536** | 8 | the per-player `danger` maps |

Shift-count census over `re/decomp-all/`, by how often each appears adjacent to
`div_3_table`: `>>6` ×485, `>>8` ×443, `>>4` ×122, `>>7` ×83, `>>9` ×10.

### This retires an open question in `architecture.md`

`docs/derivation/architecture.md` §8 records the general pathfinder as "parameterised
48/192/768" with no explanation. **Those are the U, T and W cell sizes**, and they match
the three entry points name-for-name: `find_upath` (48), `find_tpath` (192),
`find_wpath` (768). The pathfinder is not using magic numbers; it is searching on three
different rungs of this ladder.

### Two independent corroborations of the 192/768 scale

- `WorldData::is_valid(Coord, Coord)` `0x0043f360` bounds a raw `Coord` by
  `tile_xs * 0xc0` — and `0xc0 = 192`.
- `World::set_oil_at` `0x006b2a10` spawns the oil `Good` at
  `(wx * 0x300 + 0x180, wy * 0x300 + 0x180)` — cell size `0x300 = 768`, centre offset
  `0x180 = 384`.

### Intra-cell traversal — the +2 that will bite you

| function | VA | result |
|---|---|---|
| `WCoord::operator TCoord` | `0x004613b0` | `t = w*4 + 2` — **the centre tile, not the corner** |
| `WCoord::get_tcorner` | `0x0046f080` | `t = w*4` |
| `TCoord::operator WCoord` | `0x0046fab0` | `w = t >> 2` |
| `WCoord::traverse_x(i)` | `0x0046eef0` | `t = w*4 + (i mod 4)`, sign-correct |
| `WCoord::traverse_y(i)` | `0x0046eed0` | `t = w*4 + floor(i/4)` |
| `WCoord::traverse()` | `0x0042a610` | returns **16** |

Implicit `WCoord → TCoord` conversion lands on the cell **centre**. Anything that wants the
cell's tile block must call `get_tcorner`. Getting this wrong shifts every world→tile
conversion by half a world cell, silently.

### The neighbour rings

```
8-ring (NW,N,NE,E,SE,S,SW,W)   dx @0x00adcaf4 = [-1, 0, 1, 1, 1, 0,-1,-1]
                               dy @0x00adc404 = [-1,-1,-1, 0, 1, 1, 1, 0]
4-ring (N,E,S,W)               dx @0x00add254 = [ 0, 1, 0,-1]
                               dy @0x00add214 = [-1, 0, 1, 0]
```

---

## 2. `World::init` — the grid dimensions, from the instruction stream

`World::init(unsigned short xs, unsigned short ys)` `0x006b76f0`, read at
`0x006b7718`–`0x006b779a`:

```
xs, ys          = arguments                       World+0x00, +0x04
size            = xs * ys                                +0x08
tile_xs         = xs * 4      tile_ys = ys * 4           +0x18, +0x1c
tile_size       = tile_xs * tile_ys                      +0x20
fog_xs          = (tile_xs * 2) / 4   ( = xs * 2)        +0x0c, +0x10
fog_size        = fog_xs * fog_ys                        +0x14
reg_xs          = tile_xs / 8         ( = xs / 2)        +0x24, +0x28
reg_size        = reg_xs * reg_ys                        +0x2c
sea_map         = -1                                     +0x34
player_territory_limit{,_civic,_city}       <- Constants +0x118, +0x11c, +0x120
colonized_territory_limit{,_civic,_city}    <- the SAME three Constants fields
player_reg = resource_reg = land_size = seed = 0
```

`fog_xs` and `reg_xs` are computed from `tile_xs` by **signed** division, so they round
toward zero — which only matters for odd `xs`, and is reproduced exactly in the port.

Those three `Constants` offsets are already in `docs/derivation/rules-constants.json`:
`territory_limit_base = 44` ("44 tiles"), `territory_limit_civic = 4`,
`territory_limit_city = 4`. The `colonized_*` triple is **initialised from the same three
fields**, not from separate rules — worth knowing before someone goes hunting for
`colonized_territory_limit` in `rules.xml`.

Allocation order in `init`: `wdata` (`size × 28` bytes + a 4-byte count header, elements
constructed by `0x006b2310`), `tdata` (`tile_size × 2`, `memset` 0), then the fog planes.

---

## 3. `WData` — 28 bytes per world cell, 21 checksummed

PDB layout, verbatim:

| off | size | field | notes |
|---|---:|---|---|
| `+0x00` | 2 | `flags` | see the bit table below |
| `+0x02` | 1 | `land` | 0 dry, 1 shallow, 2 deep — `is_ocean` accepts **1 and 2** |
| `+0x03` | 1 | `land_sub` | |
| `+0x04` | 2 | `region` | land region id |
| `+0x06` | 2 | `region2` | water region id, used only for a `WATERHALF` cell's water tiles |
| `+0x08` | 2 | `down` | |
| `+0x0a` | 2 | `down_who` | |
| `+0x0c` | 1 | `val` | |
| `+0x0d` | 1 | `goods` | |
| `+0x0e` | 1 | `light` | |
| `+0x0f` | 1 | `who` | **territory owner**, `-1` = unowned |
| `+0x10` | 1 | `who2` | |
| `+0x11` | 1 | `blocked` | count of the cell's 16 tiles carrying `BLOCKED` |
| `+0x12` | 1 | `bad` | count of the cell's 16 tiles carrying `BAD_PATH` |
| `+0x13` | 1 | `solid` | maintained ±1 — see the sign note below |
| `+0x14` | 1 | `was_seen` | |
| `+0x15` | 3 | *padding* | **not checksummed** |
| `+0x18` | 4 | `block` | `CollBlock*`, lazily allocated |

Indexing is `wdata[wy * xs + wx]`, stride 28 — `World::get_wdata` `0x0046d220`
(`lea edx,[esi*8]; sub edx,esi; lea eax,[eax+edx*4]` = ×28).

`World::walk_data` §5 walks `[wdata + 28·i, +0x15)` — **21 bytes**, i.e. `flags` through
`was_seen`, stopping before the padding and the pointer.

### `WData::flags` bits

| bit | name | evidence |
|---|---|---|
| `0x0002` | `DEAD_BUILD` | `WorldData::is_dead_build` `0x006b2390` |
| `0x0004` | `COAST` | `is_coast` `0x006b3020` |
| `0x0008` | `ROCKS` | `is_rocks` `0x006b4380` |
| `0x0010` | `MOUNTAINS` | `is_mountains` `0x006b5590` |
| `0x0020` | `FOREST` | `is_forest` `0x006b5560` |
| `0x0040` | *impassable, unnamed* | groups with `MOUNTAINS` in `get_land`'s `& 0x50` and in `& 0x70`. **No writer identified** — measured in readers only. |
| `0x0080` | `HAS_ROAD` | `set_road_at` `0x006b43b0`, cell-level rollup |
| `0x0100` | `WATERHALF` | `set_waterhalf` `0x006b1c90` / `clear_waterhalf` `0x006b1bd0` |
| `0x0400` | `ORIG_COAST` | `set_orig_coast` `0x006b1b80` clears it; `set_land(COAST)` sets it |
| `0x0800` | `OIL` | `set_oil_at` `0x006b2a10`; `is_oil_at` `0x00472af0` |
| `0x1000` | `HAS_RIVER` | `set_river_at` `0x006b1f80`, cell-level rollup |

Composite masks the engine actually tests:

```
LAND_CLASS  0x003c   set_land clears these four, then ORs its class argument in
IMPASSABLE  0x0070   is_passable  0x006b23c0  ==  (flags & 0x70) == 0
NO_BUILD    0x0078   buildings_allowed 0x006b2340 == (flags & 0x78) == 0
```

So **rocks are passable but not buildable** — `0x08` sits inside `0x78` and outside `0x70`.

### The `solid` sign, as measured

`set_blocked_at` **increments** `solid`; `set_tree_at` **decrements** it. That is what the
disassembly says and it is reproduced faithfully, but it means `solid` is *not* a plain
blocker count, and I could not determine what it is. Flagged, not explained.

### `World::wipe` defaults, at instruction level

`World::wipe` `0x006b2c00`, inner loop `0x006b2ce2`–`0x006b2d03`:

```
mov dword [edx+2],   0x400002   ; land=2, land_sub=0, region=64
mov word  [edx],     0          ; flags = 0
mov dword [edx+0xa], 0          ; down_who=0, val=0, goods=0
mov word  [edx+6],   0          ; region2 = 0
mov dword [edx+0xf], 0xffff     ; who=-1, who2=-1, blocked=0, bad=0
mov word  [edx+0x13],0          ; solid=0, was_seen=0
mov byte  [edx+0xe], 0          ; light = 0
mov word  [edx+8],   -1         ; down = -1
add edx, 0x1c
```

Default `region` is **64**, not 0 — an easy thing to get wrong and it is checksummed.

`WorldData::offmap_world`, the static `WData` at `0x00c899a0` returned for out-of-bounds
reads, is `{land: 2, down: -1, who: -1, who2: -1, rest 0}` — off-map reads as deep water.
Its 28 bytes were read directly and are asserted in a test.

---

## 4. `TData` — one `u16` per tile

`TData` is 2 bytes: a single `mask`. Indexing is `tdata[ty * tile_xs + tx]`.

Bits 0–1 are a **2-bit blocker kind** and bits 4–5 a **2-bit surface kind**. Treating them
as independent bits is the mistake to avoid — `set_building_at` writes `mask |= 3`, which
is the *value* 3, not "cliff and mountain".

| bits | field | values | setter |
|---|---|---|---|
| `0x0003` | blocker | 0 none, **1 cliff**, **2 mountain**, **3 building** | `set_cliff_at` `0x006b1eb0`, `set_mountain_at` `0x006b1f00`, `set_building_at` `0x006b45c0` |
| `0x0030` | surface | 0 plain, **0x10 road**, **0x20 water**, **0x30 trees** | `set_road_at` `0x006b43b0`, `set_tocean` `0x006b1c60`, `set_tree_at` `0x006b2060` |

| bit | name | setter |
|---|---|---|
| `0x0004` / `0x0008` | `BEHIND_A` / `BEHIND_B` | `set_behind` `0x006b4230` (`0x8` is also set by `set_tree_at`) |
| `0x0040` | `STARTED2` | `set_started2_at` `0x006b4530` |
| `0x0080` | `STARTED` | `set_started_at` `0x006b4570` |
| `0x0100` | `CITY` | `set_city_at` `0x006b4180` |
| `0x0200` | `RESOURCE` | `set_resource_at` `0x006b3a80` |
| `0x0400` | `COASTAL` | `set_coastal` `0x006b1d10` |
| `0x0800` | `RIVER` | `set_river_at` `0x006b1f80` |
| `0x1000` | `GATHERED` | `set_gathered_at` `0x006b46b0` |
| `0x2000` | `BAD_PATH` | `set_bad_path` `0x006b4610` |
| `0x4000` | `BLOCKED` | `set_blocked_at` `0x006b4900` |
| `0x8000` | `GATHER_EDGE` | `set_gather_edge` `0x006b2110` (set-only; no clear path exists) |

### The two-level invariants the setters maintain

These are the parts a naive port gets wrong, because the flag lives at one resolution and
the rollup at another:

- **`BLOCKED` ⇒ neighbour `BAD_PATH`.** `set_blocked_at` clears the tile's own `BAD_PATH`,
  removes any road, then paints `BAD_PATH` on all 8 in-bounds neighbours — and on clear,
  re-evaluates each neighbour through `has_blocked_neighbors` `0x006b2990` rather than
  blindly clearing. `WData::blocked` and `WData::bad` are counters kept in step with every
  transition.
- **Cell-level rollups are rescans, not refcounts.** Clearing the last road tile in a cell
  makes `set_road_at` scan all 16 tiles before clearing `HAS_ROAD`; `set_river_at` does the
  same for `HAS_RIVER`. Both are reproduced as scans.
- **`get_tregion`** `0x006b52e0` returns `region2` **only** when the owning cell is
  `WATERHALF` *and* the tile itself is water; otherwise `region`. This is how a coastal
  cell belongs to a land region and a sea region at once.

### Rivers and combat — the one terrain→combat coupling found

`Constants+0x68` is `river_modifier`, parser `scaled`, scale 256, stored value **512**
(`rules.xml`: `2/1 (units take more damage in rivers)`). Its **only** reader in the whole
image is `ObjectData::get_damage` `0x00644130`, at `0x00644..` in the decompiled listing:

```
if (target is a unit) && ((int)(target->[+0xc] ^ 0x63637) < 0):
    dmg = (dmg * river_modifier) >> 8        // with the toward-zero rounding fixup
```

`+0xc` is XOR-obfuscated with the same `0x00063637` key the live process uses for object
coordinates. This is the damage lane's step to own — recorded here because it is the
terrain hook, and **not implemented in this module** (`mechanics.rs` is another lane's
file).

Cross-check: `RIVER_RESOURCE_VALUE` at `Constants+3136` is stored as `2`, and its own
`rules.xml` comment asks "does this do anything anymore?".

---

## 5. Fog — three planes at F resolution, one at W

| plane | offset | size | reader |
|---|---|---|---|
| `seen` | `+0x15c` | `fog_size` bytes | `WorldData::is_really_seen` `0x006b42c0` |
| `seen2` | `+0x160` | `fog_size` bytes | `WorldData::was_seen` `0x006b53f0` |
| `seen3` | `+0x164` | `fog_size` bytes | `WorldData::is_detected` `0x006b48c0` |
| `wcoord_seen` | `+0x168` | `size` bytes | `WorldData::is_seen_wcoord` `0x006b2540` |

Each byte is a **bitmask over the 8 players**; the mask for player *p* is the byte at
`leader[p] + 0x6929`. Indexing is `seen[fy * fog_xs + fx]` (`World::get_seen`
`0x006b4290`). Note `F >> 1 == W`, which is how `is_seen` reaches the owning cell's `who`
for its ally short-circuit.

All four planes are inside the `world` channel — confirming the sibling lane's note that
all three fog planes are checksummed, and adding the W-resolution fourth.

The port implements the plane-and-mask core. The leader-level short-circuits in `is_seen`
(reveal-all flag `0x800`, the `0x2000` ally-vision path, the game-mode override) read
`Leader` state and belong to the leader lane; they are deliberately **not** modelled here.

`World::clear_seen` `0x006b2250` zeroes the live plane once per tick, from
`GameDaemon::update_all_seen` — so `seen` is rebuilt every frame while `seen2`/`seen3`
persist.

---

## 6. The `world` checksum channel, section by section

`World::walk_data(DataWalk*, int section)` `0x006b5cf0`, called from `check_all` as
`0x006b5cf0(world, -1)` — `-1` means "all sections". The channel is **conditional**: it
is skipped entirely when `[[0x00c06188]+0x134] == 0`, i.e. when `wdata` is unallocated.

| § | bytes walked |
|---|---|
| 1 | `walk(world+0, +8)` — `xs`, `ys` |
| 2 | `start_x`, `start_y`, `start_city_x`, `start_city_y` — `SimpleArray<WCoord>::walk_data` `0x0047c660` |
| 3 | `oil_x`, `oil_y` |
| 4 | `walk(world+8, +0x80)` — **120 contiguous bytes**, `size` … `seed` |
| 5 | per W cell: `walk(wdata + 28·i, +0x15)` — 21 bytes |
| 6 | `tdata` (`tile_size·2` B), then `seen`, `seen2`, `seen3` (`fog_size` B each) |
| 7 | `wcoord_seen` (`size` B) |
| 8 | `danger[p]`, `p in 0..8`, `reg_size·4` B each |
| 9 | per W cell: an `i32` presence flag, then the block's `[0,8)` and `[0xc, 0xc+size)` |
| 10–13 | four `Terrain` arrays — see below |

Section 4 being **one contiguous 120-byte range** means the *field order* of the scalar
block is load-bearing for the checksum, not just the values. The port stores it as an
ordered array to make that explicit.

### `SimpleArray<T>::walk_data`, on the checksum path

Read off `0x0047c660` (`WCoord`) and `0x00473120` (`int`):

```
walk(&length, 4)
if length != 0:
    walk(&size, 4)                 # capacity
    walk(this+0xc, 2)              # increment : short
    flags &= ~0x40 ; walk(&flags, 1)
    walk(list, length * sizeof(T))
```

`Array<WCoordData>::walk_data` `0x00478990` has the identical header and walks its 8-byte
elements **one at a time** — byte-identical input to adler-32, so the same result.

### `CollBlock`

`CollBlock : BitMask<768>` — `{bits: i32, size: i32, flags: i32, ptr: u8[96]}`.
`World::new_coll_block` `0x0046d250` writes `bits = 0x300` (768), `size = 0x60` (96),
zeroes the payload, then `flags = 1`; the load path sets `flags = 2` instead. The walk
covers `[0, 8)` and `[0xc, 0xc+size)` — **`flags` is deliberately skipped**, which is why
save-then-load does not desync despite the 1→2 change.

### New finding: four `Terrain` fields are lockstep-critical

Sections 10–13 dispatch through `[0x00c06218]`, which the PDB names
`MiscAccess::terrain : Terrain&`:

| § | offset | field |
|---|---|---|
| 10 | `+0x4b80` | `Terrain::halfland_locs : WCoordList` (`Array<WCoordData>`) |
| 11 | `+0x4b9c` | `Terrain::halfland_types : SimpleArray<int>` |
| 12 | `+0x4bb8` | `Terrain::halfland_subtypes : SimpleArray<int>` |
| 13 | `+0x4bd4` | `Terrain::nuke_hits : SimpleArray<int>` |

`architecture.md` §2 warns that `MiscAccess` is not "presentation" and that `terrain`
carries sim-critical state. This pins down **exactly which four fields**, and it is the
whole of `Terrain`'s contribution — the other 27 KB (render states, render helpers,
textures, vertex streams, and `master_land_heights`) is not walked.

---

## 7. Map generation — deterministic from the seed. Yes.

This was the coordinator's headline question. The answer is at the top of `Map::make`
`0x0068bc90`, `0x0068bcbf`–`0x0068bcd0` [measured, instruction level]:

```
0068bcbf  test ecx, ecx            ; ecx = the seed argument
0068bcc1  js   0x68bcd2            ; negative seed => keep whatever is already there
0068bcc3  mov  eax, [0x00c06188]   ; GameAccess::world
0068bcc8  mov  [eax + 0x7c], ecx   ; World::seed = seed
0068bccb  mov  eax, [0x00c06184]   ; GameAccess::game_random
0068bcd0  mov  [eax], ecx          ; the LCG state word = seed
```

`[0x00c06184]` is `GameAccess::game_random : Random&` — the **main simulation stream**
(`s ← s·1664525 + 1013904223`), not a private worldgen generator. So:

- **Map generation is a pure function of one 32-bit seed** plus the static generation
  inputs, and it consumes the same RNG stream the sim later uses.
- The generation inputs are all in `GameInfo`: `+0x04 seed : unsigned long`,
  `+0x19 map_style`, `+0x1a map_size`, `+0x1b players`, `+0x1e game_rules`,
  `+0x20 starting_town`. The reproducibility tuple is exactly those fields plus the rules
  load.
- Immediately after `Map::load_map_data`, if `Map+0x44 < 0` the generator draws
  `Random::get(0, 0xffff) & 3` for a map orientation — a *further* consumer of the same
  seeded stream, so orientation is reproducible too.
- `Map::make` is the common driver: it appears in **21 vtables** (one per map style, hence
  0 direct callers), with `make_continents` as the per-style virtual hook.

### Is the generated terrain quantised to integers? Yes, by construction.

Two measurements:

| scan | floating-point instructions |
|---|---|
| all `map.cpp` generator functions > 200 bytes (26,430 instructions) | **38** (0.14%) |
| the entire `World::set_*` / `init` / `wipe` / `walk_data` write API (1,929 instructions) | **0** |

And a third: **no `map.cpp` function calls any of the eight `_libm_sse2_*_precise`
transcendental imports** (scan of every indirect call in every `map.cpp` function against
the IAT slots at `0xac5518`–`0xac5534`). `README-LLM.md` establishes those eight as the
project's entire IEEE hazard surface.

So the picture is:

- The **stored** terrain is integer with no exception: `WData` and `TData` contain no float
  field, and nothing that writes them touches floating point. The quantisation boundary
  *is* the `World::set_*` API.
- The **generator** does use a little float — concentrated in
  `MapWarringStates::make_continents` (21), `MapFairness::calc_distances` (8) and
  `Map::scale_number` (5). Inspected, `calc_distances` is `cvtdq2ps → mulss → movss` then
  `comiss` comparisons: a float distance metric used to bias start positions. Every one of
  those opcodes is IEEE-exact binary32, and there are no transcendentals, so the float in
  worldgen is **bit-reproducible**, not a hazard.

Conclusion for replay validation: **generating the engine's maps from a seed is achievable
and blocked only on porting the generator logic, not on any numerical obstacle.** That is
the difference between starting replay validation from a captured state and starting it
from scratch.

### 7.1 Executable world-generation oracle boundary

The structural result above now has two fork-isolated retail cases in
`crates/oracle`; neither case substitutes simplified map logic.

| case | retail bytes executed | exact claim | deliberately not claimed |
|---|---|---|---|
| `map_make_seed_prefix` | `Map::make` entry `0x0068bc90` through the seed write at `0x0068bcd0` | the signed-negative preserve gate; `Map+0x110 = map_arg`; and identical nonnegative seed writes to `World+0x7c` and `game_random+0` | terrain construction, RNG consumption, orientation, continents, fairness or starts |
| `start_city_wcoord` | complete leaf `0x006b30e0`–`0x006b311d` | valid coordinates flatten as `y * world_xs + x`; `start_city_locs` is LSB-first | coordinate selection, radius tests, or placement policy |
| `add_starting_location` | complete writer `0x006b2de0`–`0x006b3019` | returned start index; all four walked-array append sequences; the exact 2×2 row-major LSB-first occupancy writes | coordinate selection, map-style placement, or allocator execution (fixture supplies measured spare capacity) |

The seed-prefix case isolates an exact instruction boundary rather than invoking a fake
constructor. In its already-forked case process it replaces the first instruction *after*
the two writes (`0x0068bcd2`) with a five-byte jump to `Map::make`'s original epilogue at
`0x0068c84a`. The entry, SEH setup, signed branch, map-argument store and seed stores are
the relocated retail instructions. The boundary bytes are checked before patching, and
the case record states the patch and limitation. The Rust side is the shipped
`World::seed_map_generation`, not another transcription in the harness.

`start_city_wcoord` needs only the two retail World reference globals and an owned bit
plane. Its Rust model is now the shipped
`don_sim::systems::map_terrain::World::start_city_wcoord`, so the differential case tests
the implementation used by replay reconstruction rather than an oracle-local copy. The
valid-domain predicate is exact: positive width, in-bounds nonnegative x/y, and enough bit-plane storage. Retail
has no bounds check, so invalid coordinates are excluded rather than converted into a
made-up policy.

`add_starting_location` uses a World fixture whose four destination arrays have measured
`SimpleArray<WCoord>` layouts and spare capacity. The retail allocator branches therefore stay
untaken, but every append and bit write in the shipped writer executes. The production Rust
method owns the same walked metadata and uses the separately disassembled growth rule: these
World arrays start at capacity 0 with `increment == -1`, then grow 0→4→8→16. `World::init`
also now mirrors `DynamicBitMask::init` (`0x00a3a3c0`) by allocating and zeroing
`ceil(xs*ys/8)` bytes instead of leaving a convenient empty plane.

This establishes the following evidence ladder for a pinned-seed world oracle:

1. **Landed:** prove seed installation and negative-seed preservation without entering
   the unconstructed map body.
2. **Landed:** prove the final start-city occupancy representation and indexing.
3. **Landed:** `World::add_starting_location` appends the player coordinate, the ordered
   city-footprint coordinates `(x,y)`, `(x-1,y)`, `(x,y-1)`, `(x-1,y-1)`, and the matching
   occupancy bits, returning the original player-start index.
4. **Next distance/exclusion leaves:** `WorldData::start_city_rad_wcoord` `0x006b3850`
   walks the recorded start-city coordinate arrays and applies the engine's integer
   `vector_dist` threshold; `MapFairness::calc_distances` `0x0068a1c0` writes its binary32
   distance table and extrema. These require exact array/count and Constants fields, but
   no guessed ring geometry.
5. **Only after those fixtures are executable:** expand into
   `Map::place_start_in_region` and the per-style continent hooks, recording the consumed
   RNG state and complete integer terrain/start arrays after each stage.
6. **Full constructor last:** `Map::make` is 3,021 bytes and requires the selected one of
   21 map-style objects, `GameInfo`, Rules/Constants, `RString` leaves, engine arrays and
   allocators. Until those dependencies are real or exactly substituted, the full seeded
   terrain/start comparison remains a machine-readable `known_gap` in
   `schema/oracle-regression.json`.

No annulus, ring, fairness, continent or start-position rule is inferred from a convenient
shape in this plan. A stage advances only when its retail inputs and side effects can be
executed and compared.

---

## 8. What the Rust module does, and how it was measured

`crates/don-sim/src/systems/map_terrain.rs`, **28 focused tests, all passing**:

```
rustc --edition 2021 --test crates/don-sim/src/systems/map_terrain.rs -o /tmp/mt && /tmp/mt
test result: ok. 23 passed; 0 failed
```

Implemented:

- The six coordinate newtypes with the engine's exact shift-then-`div_3` conversion, the
  `traverse_x/y`/`get_tcorner`/centre helpers, and both neighbour rings.
- `WData` (21-byte checksum serialisation), `TData` bit constants, `CollBlock`,
  `WorldData::offmap_world`.
- `World::init` (all derived dimensions from the instruction stream) and `World::wipe`.
- 22 terrain predicates — `is_cliff_at`, `is_mountain_at`, `is_tree_at`, `is_tocean`,
  `is_river`, `is_built_at`, `is_blocked_at`, `has_blocked_neighbors`, `is_ocean`,
  `is_forest`, `is_mountains`, `is_rocks`, `is_coast`, `is_passable`, `is_impassable`,
  `is_pass_land`, `is_flat`, `buildings_allowed`, `num_waterhalf`, `get_region`,
  `get_region2`, `get_tregion`, `get_who`, `get_goods`, `get_land`, `get_coll_block`.
- 20 mutators, with the counter and rollup invariants of §4 —
  `set_blocked_at`, `set_bad_path`, `set_cliff_at`, `set_mountain_at`, `set_building_at`,
  `set_road_at`, `set_tree_at`, `set_tocean`, `set_waterhalf`, `clear_waterhalf`,
  `set_river_at`, `set_coastal`, `set_resource_at`, `set_city_at`, `set_started_at`,
  `set_started2_at`, `set_gathered_at`, `set_gather_edge`, `set_behind`, `set_land`,
  `set_oil_at`, `set_down`, `new_coll_block`.
- The fog planes and their accessors, plus `clear_seen` / `clear_danger`.
- **The `world` checksum channel**: a `DataWalk` trait, adler-32 (NMAX 5552 as measured at
  `0x00a46854`), and `World::walk` reproducing all 13 sections in order.
- **The placement predicates the production lane is blocked on** — see §8.1.

Tests that carry real evidence rather than restating the code:

| test | what it pins |
|---|---|
| `div_3_matches_floor` | `div_3_table` semantics across zero |
| `coord_conversions_are_floor_division` | shift-then-div-3 == `floor(c/scale)` for all five rungs, 5,700 values including negatives |
| `ladder_is_consistent` | `TCoord→WCoord` agrees with `Coord→WCoord` directly, 4,600 values |
| `wcell_traversal_covers_sixteen_tiles` | `traverse_x/y` enumerate the 4×4 block exactly once each; centre vs corner |
| `world_init_dimensions` | every derived dimension for an 80×60 map |
| `wipe_defaults_match_the_disassembly` | the `0x400002` / `0xffff` writes, byte for byte |
| `offmap_world_bytes` | the 28 bytes read from `0x00c899a0` |
| `blocker_field_is_exclusive`, `surface_field_is_exclusive` | the two 2-bit fields are enums, not bit sets |
| `blocked_propagates_bad_path_to_the_eight_ring` | the 8-ring `BAD_PATH` invariant and both counters, set and clear |
| `river_cell_flag_tracks_its_sixteen_tiles` | the rollup rescan |
| `tregion_splits_waterhalf_cells` | `region`/`region2` selection |
| `adler32_known_vector` | the hash primitive |
| `world_channel_byte_count` | the exact byte total of the traversal — catches a section-order regression by size as well as by hash |
| `world_checksum_is_deterministic_and_sensitive` | stability, and that padding is excluded while block *presence* is included |

### 8.1 For the production lane: the placement predicates

`BuildTypeData::blocked_location` `0x006375b0` (5,120 B) and `blocked_tcoord`
`0x00636db0` (2,044 B) live in `buildtype.cpp` and are production's to port — but they run
on top of two `world.cpp` predicates, and **both are now implemented here**:

**`WorldData::space_at_corner(TCoord, TCoord, who, …, need_city)` `0x006b27f0`** grades the
4×4 tile block anchored at `(tx, ty)`. Its 16 probe offsets (`int[16]` at `0x00adecf0` /
`0x00aded30`) tile that block, and the *probe order* is the point:

```
         dx=0  dx=1  dx=2  dx=3
  dy=0 :   4     5     6     7
  dy=1 :  12     0     1    13
  dy=2 :  14     2     3    15
  dy=3 :   8     9    10    11
```

Indices **0–3 are the inner 2×2 core**, and the engine `return 0`s on the *first* blocked
core probe without scoring the ring at all. A probe counts as blocked when: it is out of
bounds; `need_city` is set and the tile lacks `CITY`; the tile is a building or has
`STARTED`; the owning cell's `who >= 0` and differs; or the tile has `BLOCKED`.

Grades: `4` all 16 clear; `3` at least one of four 5-cell **approach L** corridors is
entirely clear (the groups at `0x00adeca0`: bottom-right, bottom-left, top-left,
top-right); `2` otherwise; `0` core blocked.

**`WorldData::check_building_wcoord` `0x006b26e0`** is the W-cell gate in front of it. It
rejects outright when the cell is foreign-owned, impassable (`flags & 0x70`), or has
**all sixteen** tiles blocked (`WData::blocked == 16`), then sweeps `dx ∈ [-rx, rx]`,
`dy ∈ [-ry, ry]` skipping offsets whose Manhattan distance exceeds `max_dist` (axes
exempt), returning the best grade and short-circuiting on 4.

**`WorldData::has_gather_access` `0x006b4e50`** (the `mode != 0` arm) is also implemented:
a tile is gatherable when it is trees / mountain / cliff **and** at least one of its four
orthogonal neighbours is neither water nor blocked.

Four tests cover these: `space_probes_tile_a_4x4_block`, `space_at_corner_grades`,
`check_building_wcoord_gates`, `gather_access_needs_a_dry_neighbour`.

---

## 9. Honest gaps

The module is **Tier C** — structure and constants read off the binary, behaviour
reproduced, **no differential test against retail**. Specifically:

1. **No oracle run.** Nothing here has been executed against retail machine code. The
   highest-value next step is an `oracle` case for `World::init` (pure, 5 scalar outputs
   from 2 inputs) and one for `space_at_corner` over a synthesised `tdata`/`wdata` pair —
   both are self-contained enough to call directly.
2. **`WData::flags 0x0040` has no identified writer.** It is measured in four readers
   (`is_flat`, `is_passable`, `is_impassable`, `get_land`) and is impassable, but nothing
   was found that sets it. Do not assume it is dead.
3. **`WData::solid`'s sign convention is measured but unexplained** — blocking increments,
   trees decrement. Reproduced faithfully; not understood.
4. **`WalkedArray::capacity` / `increment` / `flags` are checksummed and we do not model
   the engine's growth policy.** For the six `SimpleArray<WCoord>` members plus the four
   `Terrain` arrays, a non-empty array's checksum depends on the *capacity* the engine's
   `increase_size` happened to land on, not just the contents. Empty arrays (the common
   case at init) walk only their 4-byte length, so this does not bite until start
   positions are populated. **This is the single biggest known obstacle to a bit-exact
   `world` channel** and it belongs to whoever ports `basic/arraybase.h`.
5. **The `DataWalk+0x0c` section mask.** `check_all` passes `-1` for the world channel, so
   all 13 sections run; other callers may pass a single section index. Only the `-1` path
   is implemented.
6. **The leader-side fog short-circuits are not modelled** (reveal-all, ally vision,
   game-mode override). The plane-and-mask core is exact; the wrapper is the leader lane's.
7. **`World::compute_reg_territory` `0x006b0bb0` (4,039 B) is not ported.** Territory
   ownership writes `WData::who`, which is checksummed, so the `world` channel cannot be
   bit-exact in a live game without it. The sibling borders lane has the caps (44 plain /
   96 fully teched, 256 cells/frame budget); this module supplies the storage and the
   `get_who`/`get_region` accessors it needs.
8. **The generator itself is not ported** — only the proof that porting it is worthwhile.
   `Map::make`'s subsystem order and the 21 per-style `make_continents` overrides are
   mapped but not implemented.
9. **`land` values 0/1/2 are named from behaviour**, not from a definition. `is_ocean`
   accepts 1 and 2, `wipe` and `offmap_world` use 2. The PDB's `TileSetLandTypes`
   (`eTILE_FERTILE=0, eTILE_COASTAL=1, eTILE_OCEAN=2`) is a *tileset* enum and is only a
   cross-check — I have not proven the two enums are the same.

## 10. Wiring

The module is **not** referenced from `lib.rs` — `crates/don-sim/src/lib.rs` and
`world.rs` are owned by a parallel wave, so I did not touch them, and I did not create
`systems/mod.rs` in case a sibling lane is creating it too. To wire it up:

```rust
// crates/don-sim/src/systems/mod.rs
pub mod map_terrain;

// crates/don-sim/src/lib.rs
pub mod systems;
```

Until then it compiles and tests standalone with the `rustc --test` line in §8.

## 11. Ledger entries to add to `docs/provenance-ledger.md`

| mechanic | source | tier | evidence |
|---|---|---|---|
| Coordinate ladder 48 / 192 / 384 / 768 / 1536 Coord units | `Coord::operator {W,T}Coord` `0x00461460` / `0x0046cd40`; `init_coord_lookup_array` `0x00681db0` | structural [measured] | `div_3_table[i] == floor(i/3)`; shift census 6/8/4/7/9; corroborated by `is_valid`'s `0xc0` and `set_oil_at`'s `0x300`/`0x180` |
| `World::init` derived dimensions | `0x006b76f0`, `0x006b7718`–`0x006b779a` | structural [measured] | `tile = xs*4`, `fog = xs*2`, `reg = xs/2`; territory limits from `Constants+0x118/11c/120` |
| `WData` 28-byte layout, 21 checksummed | PDB TPI; `World::walk_data` §5 | structural [measured] | `walk(wdata+28i, +0x15)` |
| `TData` 2-bit blocker and surface fields | the 20 `World::set_*_at` setters | structural [measured] | per-setter mask writes, tabulated in §4 |
| `World::wipe` cell defaults (incl. `region = 64`) | `0x006b2c00` @ `0x006b2ce2` | structural [measured] | `mov dword [edx+2], 0x400002` etc. |
| `world` checksum channel, 13 sections | `World::walk_data` `0x006b5cf0` | structural [measured] | full disassembly; §4 is one 120-byte range |
| `Terrain::{halfland_locs,halfland_types,halfland_subtypes,nuke_hits}` are in the `world` channel | `0x006b5ff4`+, via `[0x00c06218]` = `MiscAccess::terrain` | structural [measured] | offsets `+0x4b80/4b9c/4bb8/4bd4` |
| Map generation is seeded from one 32-bit value into `game_random` | `Map::make` `0x0068bc90` @ `0x0068bcbf` | structural [measured] | `mov [world+0x7c], ecx` ; `mov [game_random], ecx` |
| Worldgen has no transcendental calls; sim terrain is integer-only | scan of all `map.cpp` functions vs IAT `0xac5518`–`0xac5534`; FP census of `World::set_*` | structural [measured] | 38 FP / 26,430 in generators, 0 FP / 1,929 in the write API, 0 transcendental calls |
| `space_at_corner` probe layout and approach groups | `0x006b27f0`; tables `0x00adeca0`, `0x00adecf0`, `0x00aded30` | structural [measured] | 4×4 tiling with indices 0–3 as the core; four 5-cell L groups |
| `river_modifier` (×2, `Constants+0x68`) is read only by `get_damage` | `0x00644130` | structural [measured] | sole reader across `re/decomp-all/`; gated on `(obj[+0xc] ^ 0x63637) < 0` |

And **remove** from the "not yet derived" list: *the map / terrain representation*.
Replace with: *`World::compute_reg_territory`; the map generator itself; the
`ArrayBase` growth policy that the `world` channel's array capacities depend on*.
