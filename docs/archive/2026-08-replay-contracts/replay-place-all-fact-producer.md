# Replay `place_all` prerequisite producer

## Result

`crates/don-replay/src/place_all_facts.rs` separates the prerequisite boundary
into facts that installed content can reconstruct and facts that exist only in
the running game. It produces `PreparedReplayPlaceAll` only when every required
source is present; otherwise it retains the exact reconstructed terrain catalog
and returns a typed `UnavailablePlaceAllFact` row for each missing producer.

No empty group list, zero TData plane, default mountain cursor, disabled helping
flag, zero score table, or empty host-resolution stream is inferred from an
absent source. An authoritative captured empty/disabled value is represented by
`Some(empty)` or `Some(CapturedHelpingFacts::Disabled)` and remains distinct
from `None`.

## Installed map-style producer

`TerrainGroups::init_terrain_data` is instruction-bounded at `0x006a6540`.
The shipped PDB fixes `TerrainGroup` at 156 bytes and gives all scalar offsets.
The disassembly establishes the following XML projection:

| XML | native field |
|---|---|
| `type` | `type +0x04`: trees 4, mountains 5, rocks 6, oil 7, cliffs 8 |
| `chance`, `grouping` | `+0x08`, `+0x0c` |
| `min_clumps`, `max_clumps` | `+0x10`, `+0x14` |
| `pattern` | `+0x18`: player 0, continent 1, world 2, nonplayer 3, corner 4 |
| `min_size`, `max_size` | `+0x1c`, `+0x20` |
| `city_keep_away`, `city_stay_near` | `start_min +0x24`, `start_max +0x28` |
| four `*_space` attributes | forest/mountain/rock/coast `+0x2c..+0x38` |
| center, edge and corner min/max pairs | `+0x3c..+0x50` |
| rock `min_oil`, `max_oil` | `+0x54`, `+0x58` |
| cliff `cliff_face` | `+0x5c`; absent/-1 becomes zero |

`touching_mountain +0x60` becomes `-1`; the constructor-supplied empty `placed`
and tile lists remain empty. Native clamps all four spacing fields to `[0,64]`
and clamps only the upper side of `start_max`, `cent_max`, `edge_max`, and
`corner_max` to 64. The producer preserves those asymmetric rules.

The value helper at `0x006a0320` recognizes shipped `SCALE` and `AREA` values.
Their callees use the recovered standard 70-cell map edge:

```text
axis = max(1, (35 + xs * value) / 70)             for value >= 1
area = max(1, (2450 + (xs * ys) * value) / 4900) for value >= 1
```

The descriptive suffixes used by the shipped group rows (`min`, `max`,
`minsize`, `mindist`, `maxdist`, `mntspace`, `forestspace`, `rockspace`, the
shipped typo `rockspacee`, `coastspace`, `space`, and `spacing`) do not match a
native scaling keyword and retain the parsed integer. Unknown suffixes fail
closed rather than being guessed.

The input `MapStyleStaticData` has already validated the 23-entry catalog and
default/selected section replacement rule. The receipt identifies whether the
selected or default `TERRAIN_GROUPS` section supplied the rows and retains that
file's path, byte count, and Adler checksum.

## Live capture producers

All capture values are admitted only when their evidence names the checked-in
shipped executable SHA-256, the replay map style and selected tileset, entry
`0x006a70d0`, and reporting anchor `0x006a8f12`.

| fact | shipped address/layout |
|---|---|
| terrain subtype arrays | `game_map` pointer `0x00caa34c`; `TerrainGroups` at `Map+0x140`; three `Array<int>` rows at group object `+0x24` |
| `console_info` | same group object `+0x78` |
| mountain lists/cursors | global `Mountains` at `0x00e85f60`; three virtual-base `LinkList<int,u8>` states consumed by `randomize_mountains` `0x0089ca70` |
| TData | global World `0x00c097e8`; dimensions `+0x18/+0x1c/+0x20`, pointer `+0x138`, exactly `tile_size` little-endian `u16` cells |
| doober/treeify rules | global TileSet `0x00e885d0`; `cur_tileset +0x20`, then `TileSetData::group_data +0x614`; PDB fields `+0x14` and `+0x20..+0x3c` |
| progress | first stack argument at `TerrainGroups::place_all` entry, forwarded from Map::make argument three at call site `0x0068c009` |
| helping state | `is_helping 0x00cae708`, `lowest_player[5] 0x00cbe440`, and entry `player_scores[8][5] 0x00cbe480` |
| reporting scores | final `player_scores[8][5]` at `0x00cbe480` when execution reaches `0x006a8f12` |
| host resolutions | ordered typed player/region subsystem results across the selected group calls |

The replay supplies `num_players`; disassembly at `0x0068c007` proves that the
ordinary `Map::make` caller pushes literal one for `place_players`. Helping
captures enabled with a different player count are rejected.

The exact fertility fractal and partitions already attached to the replay plan
are retained in the produced `TerrainGroups`; captured subtype arrays,
`console_info`, and the installed XML group catalog complete that runtime
object. Captured TData is shape-checked against the replay World before a ready
bundle can be returned.

## Remaining boundary

The checked-in extraction has lawful map-style XML but no `tilesets.xml` or
runtime memory snapshot. It can therefore reconstruct and mutation-pin every
terrain-group scalar today, while returning nine concrete live-only missing
rows. A capture implementation must populate those typed rows; this tranche
does not read process memory, mutate the VM, or pretend that live linked-list
and host-transaction state can be derived from static XML.

## Frozen paths

```text
crates/don-replay/src/place_all_facts.rs
crates/don-replay/src/lib.rs
crates/don-replay/tests/place_all_fact_producer.rs
docs/assembly/replay-place-all-fact-producer.md
```
