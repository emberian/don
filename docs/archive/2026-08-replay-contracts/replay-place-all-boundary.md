# Inside `TerrainGroups::place_all` `0x006a70d0`: the executed replay boundary

Lane: `replay-placeall`. Addresses are against `ron-bin/riseofnations.exe`
(sha256 `30478a44…625079`, image base `0x00400000`) and `ron-bin/sbl/rise.pdb`, by Capstone
over the mapped image plus the PDB type stream. There is no decompile for `place_all` —
`re/decomp-all/006a70d0.c` does not exist, the bulk pass skipped it, and the directory jumps
from `006a6f90.c` to `006a93b0.c`.

**Provenance is split and marked.** Findings this lane disassembled itself are
**[measured here]**: `Mountains::randomize_mountains`'s draw predicate and RNG object, the
`Map::make` → `load_map_data` → `load_tileset` → `Mountains::init` call chain,
`LinkListBase::add`'s head insertion and zero metric, `Mountains::add_range`'s return and
16-cap, the `<MOUNTAINS>` element census in `ron-data/effects_graphics.xml`, and the
`TileSetGroupData` PDB layout. Findings inherited from two sibling research passes and used
without re-derivation are **[reported]** and named as such at each use — chiefly the
`World::wipe` → `Mountains::clear` chain, the `place_all` instruction-scan results for
`+0x24`/`+0x78`/`progress`/`is_helping`, and the internal-string-table ordinals. Nothing here
is oracle-executed; the fidelity tier is **C**, with the divergence boundary stated in §6.

---

## 0. What moved

`schema/replay-validation.json` previously recorded, for every checksum-bearing recording
whose style completes its `make_continents`, the boundary `terrain_groups_place_all` at
`0x006a70d0`. That is the **entry address of an 8,916-byte function**: it said only "somewhere
in there", and it could not distinguish "one instruction in" from "eight thousand bytes in".

The reconstruction now *enters* the call, runs it from the replay-owned World and RNG, and
reports the first retail primitive it cannot execute:

| | before | after (map styles 6, 9, 12) | after (map style 14) |
|---|---|---|---|
| boundary name | `terrain_groups_place_all` | `place_all_mountains_add_mountain` | `place_all_world_set_oil_at` |
| exact retail VA | `0x006a70d0` (entry) | `0x0089c2e0` (`Mountains::add_mountain`) | `0x006b2a10` (`World::set_oil_at`) |
| reached via | — | `place_region_group` `0x006a2f60`, group 0 | `place_player_group` `0x006a4190` growth tail, group 0 |

Measured over the whole corpus by `tools/replay-validate.sh`, from the per-file
`initial.items.boundary` field:

```text
before   21 terrain_groups_place_all          (0x006a70d0)
after    15 place_all_mountains_add_mountain  (0x0089c2e0)
          6 place_all_world_set_oil_at        (0x006b2a10)
```

Restricted to the 21 checksum-bearing recordings the split is 12 / 6, with the remaining 3
still stopping earlier at their own style primitives (2 `map_team_continent_partition`,
1 `map_east_indies_nonplayer_islands`) exactly as before. Per style, checksum-bearing:
Old World (6) × 1, Himalayas (9) × 3 and Mediterranean (12) × 8 reach `add_mountain`;
Great Lakes (14) × 6 reach `set_oil_at`. Over all 61 files the style-12 count is 11. Every one
of the 21 records `mountain_range_lengths [1, 8, 7]` and `mountain_randomize_draws 2`.

Getting there required three producers that were previously believed to need a live capture
and are in fact derivable, one of them decisively:

1. **The three `Mountains` range lists**, from shipped `ron-data/effects_graphics.xml` (§1).
   Without them `place_all`'s very first call cannot run.
2. **The helping globals**, which `place_all` initialises itself (§2).
3. **`place_all`'s `progress` argument**, which is `1` in a normal game start and reaches
   nothing (§2).

**The `world` channel did not change and was not expected to.** `place_all` is fail-closed in
`don-sim`: a stop commits no World, terrain-group, mountain or RNG state, so the walked bytes
and the checksum are byte-identical before and after this lane
(`place_all_survey_commits_no_world_or_checksum_bytes`). A boundary that names its blocker is
worth having on its own; it is not a fidelity gain and is not reported as one.

## 1. The `Mountains` range lists are shipped data, and they cost exactly two RNG draws

`place_all`'s first substantive act is `0x006a7330  call 0x0089ca70`
(`Mountains::randomize_mountains`). Disassembled here in full, the body repeats one shape
three times, once per `LinkList<int, unsigned char>` range list:

```text
0089ca7a  mov  esi, [eax + 0xe85f74]   ; small_ranges.length   (MountainsData +0xe860e0)
0089ca80  lea  eax, [esi - 1]
0089ca83  test eax, eax
0089ca85  jg   0x89ca8b                ; draw only when length - 1 > 0
0089ca87  xor  edx, edx                ; else index 0, and NO Random::get
0089ca8b  mov  ecx, [0x00c06184]       ; GameAccess::game_random -- the MAIN sim LCG
0089ca91  push 0xffff ; push 0 ; call 0x00a39d70   ; Random::get(0, 0xffff)
0089caa3  cdq ; idiv esi               ; signed remainder by length
0089cab0  call 0x0046f0e0              ; LinkListBase<int,u8>::seek_index
```

The same shape follows at `0x0089cac4` (medium, base `+0xe85f80`) and `0x0089cb08` (large,
base `+0xe85f98`). So the three **lengths** decide how many main-stream words the call
consumes, and a wrong count shifts every later draw in `place_all`: group selection, clump
counts, clump sizes, candidate coordinates.

### Where the lists come from

`Mountains::init` `0x0089ad70` builds them through `Mountains::add_range` `0x008992b0`, and it
is reached before `place_all` on a chain each edge of which was disassembled here:

```text
Map::make 0x0068bc90
  -> 0x0068bd52  call Map::load_map_data 0x0069dad0
       -> 0x0069dd9d push 0 ; 0x0069dda0 call TileSet::load_tileset 0x0087b020
            -> 0x0087b24e call Mountains::init 0x0089ad70
  -> 0x0068c010  call TerrainGroups::place_all 0x006a70d0
```

`Mountains::init` is a pure XML parse of the shipped `MOUNTAINS` section — its filenames come
from the internal string table (`int_str_array` `0x00c06378`) rather than `.text` literals,
which is why a literal grep finds nothing. **[reported by a sibling research pass, not
re-derived here]** the decoded ordinals are 2768 `effects_graphics.xml`, 4939 `MOUNTAINS`,
4940 `MOUNTAIN`, 4944 `area`, 4945–4948 `sm`/`sml`/`med`/`lg`.

**[measured here]** `ron-data/effects_graphics.xml` contains exactly one `<MOUNTAINS>` block
holding exactly 16 `<MOUNTAIN>` elements: `area="lg"` at document positions 0–6 (7),
`area="med"` at 7–14 (8), `area="sm"` at 15 (1). That is the whole file; there is no other
`<MOUNTAIN` anywhere in it.

**So `small = 1`, `medium = 8`, `large = 7`, and `randomize_mountains` consumes exactly TWO
main-stream words, not three** — the one-element small list takes the `xor edx, edx` arm at
`0x0089ca87` without touching `game_random`. Both facts are asserted by
`the_shipped_mountain_section_gives_one_small_eight_medium_and_seven_large_ranges`.

### Node payload and order

**[measured here]** `LinkListBase<int,unsigned char>::add` `0x004a4af0` writes
`metric = 0` unconditionally (`0x004a4b00  mov byte ptr [edx+0xc], 0`), stores its argument as
the node payload (`0x004a4b04`), and makes the new node the **head** on both the empty and
non-empty paths (`0x004a4b4a`, `0x004a4b37`). `Mountains::add_range` `0x008992b0` returns the
free `ranges` slot index it filled (`0x00899363  mov eax, esi`, store at `0x0089936e`), and
caps at 16 (`0x008992f9  cmp esi, 0x10`).

Head-first order is therefore reverse insertion order with every metric zero:
`small = [15]`, `medium = [14 … 7]`, `large = [6 … 0]`.

**[reported, not verified here]** that `Mountains::init` walks the `<MOUNTAIN>` elements in
document order and passes each `add_range` return into the list its `area` selects. Only the
payload *values and order* rest on this; the lengths and the two-draw RNG cost do not, and the
payload is first consumed by `Mountains::get_range` at `0x006a82c5` — at or after the
`add_mountain` boundary. `LinkListBase::seek_index` `0x0046f0e0` restarts from `head_node`
(`0x0046f0e6  mov edx, [edi+0x10]`), so any cursor carried from a previous game is
unobservable.

### What is *not* missing — the placement arrays are provably empty

The `Mountains` global is `0x00e85f60`; its virtual base `MountainsData` is at `0x00e860d0`.
All 21 map-style `make_continents` overrides call `World::wipe` `0x006b2c00` on their
straight-line path, and `World::wipe` calls `Mountains::clear` `0x0089bec0` at **`0x006b2d97`**,
which zeroes the four per-game placement array counts (`mountain_locs.length` `0x00e86174`,
`mountain_types.length` `0x00e86190`, `mountain_loc_wcoords_x.length` `0x00e8613c`,
`mountain_loc_wcoords_y.length` `0x00e86158`). `Mountains::add_mountain` `0x0089c2e0` is only
reachable during map generation from inside `place_all` itself. So those arrays are empty at
entry for every style, and `Mountains::default()`-shaped placement state is derived, not
assumed. What `clear` does **not** touch is the range lists — hence §1.

## 2. The unavailable-fact matrix was mostly wrong

`crates/don-replay/src/place_all_facts.rs` listed nine producers as requiring a live capture
tied to the running executable. Eight of the nine do not.

| fact | status after this lane | evidence |
|---|---|---|
| `TerrainSubtypeFrequencies` (`TerrainGroups+0x24`, three `Array<int>`) | **not read by `place_all` at all**; also pure shipped data | Full Capstone pass over `0x006a70d0`–`0x006a93a3`: every `this` dereference is `[this+0x10]`, `[this+0x04]`, or the single `[this+0x78]` write. Their only consumer is the CDF at `+0xf8` that `fill_fertile` `0x006a6f90` walks, and `Map::make` runs that at `0x0068bfb8`, before `place_all` at `0x0068c010`. `init_tileset_data` `0x006a61f0` fills row *N* from the map style's `TILESET_DATA/<tileset>/LANDKEY[name]/frequency_<i>`; rows are 0 `BASELAND`, 1 `SANDY`, 2 `OCEAN` per `Lands::init` `0x0067e730` and the `land_key` literals at `0x00ecda20` |
| `TerrainConsoleInfo` (`+0x78`) | **gates nothing** | Exactly one writer in the image — `place_all` `0x006a91b5`, storing `0` — and no reader anywhere. The `TerrainGroups` constructor `0x0047a9c0` skips `+0x78`/`+0x7c`, so before the first call it is allocator residue |
| `ProgressArgument` | **derived: `1`** in a normal game start, and presentation-only | `Map::make` forwards param 3 with `push esi` `0x0068c009`; `Setup::build_game` dispatches at `0x005ac657` with its own arg 1, which `Game::init` computes as `sete al` on `param_3 == 0` at `0x0058cab9`–`0x0058cac2` and pushes at `0x0058d1b0`. Both `Game::run` dispatches (`0x00584b52`, `0x00584db6`) pass `param_3 = 0`. Tested only at `0x006a79f1` / `0x006a86f1` / `0x006a8a61`, each guarding a splash-caption block with no RNG draw and no simulation write |
| `HelpingGlobals` | **derived**: `place_all` initialises all of them before reading them | `num_players` `0x00cae70c` = `world.start_x.length` at `0x006a72eb`–`0x006a72fb`; `lowest_player[5]` `0x00cbe440` zeroed at `0x006a75ab`/`0x006a75b4` and never read by `place_all`; `player_scores` rows `[0, num_players)` zeroed at `0x006a75ee`–`0x006a760c`; `is_helping` `0x00cae708` set to `0` at `0x006a7618`, first read `0x006a82d0`, thereafter recomputed from `help_lowest_index[5]` `0x00cbe460` which `place_all` also builds |
| `PlayerCountAndPlacePlayers` | derived | `place_players` is the literal `push 1` at `0x0068c007` |
| `DooberTilesetRules` | **shipped data** | PDB `TileSetGroupData` is sixteen `int`s at offsets 0..60; the selected `TILESET/TERRAINGROUP` in `Data/tilesets.xml` declares exactly sixteen `<NAME value="N"/>` children matching those field names *in offset order*, and field 0 (`clump_factor` ↔ `CLUMP_FACTOR`) was already independently confirmed by the fertility path |
| `TDataPlane` | replay-owned | the reconstruction's own `World` |
| `ReportingScores` | **derived inside the owned transaction** | `player_scores[8][5]` is zeroed by `place_all` and accumulated by the recovered `place_region_group` `0x006a2f60` helping-score update. The owned adapter forwards that final table to the reporting tail; nothing outside `place_all` supplies it |
| `MountainRangeListsAndCursors` | **shipped data** — `ron-data/effects_graphics.xml`, lengths `1 / 8 / 7`, two RNG draws | §1 |
| `HostGroupResolutions` | **still unavailable** | §4 |

The `required_source` strings in `place_all_facts.rs` now carry these corrections. The
`PlaceAllLiveFacts` *shape* was deliberately left alone: it is another lane's refactor, and
rewriting an API at the end of a lane is how a correction turns into a regression.

## 3. `player_scores` residue — a byte-exactness trap

`place_all` zeroes only rows `[0, num_players)` of `player_scores[8][5]` `0x00cbe480`
(`0x006a75ee`–`0x006a760c`, row stride `0x14`). Rows at or above `num_players` retain values
from the previous game in the same process. Nothing reads them, so zero is correct over the
whole read domain — but a future byte-exact memory reconstruction of that global must not
assume the full 160 bytes are zero on anything but the first game.

## 4. What the driver does, and the boundary vocabulary

`crates/don-replay/src/place_all_advance.rs` runs the shipped `don-sim` transaction from the
replay-owned World and RNG. `TerrainGroups::place_all`'s composed adapter needs one caller row
per selected terrain group; the driver discovers them by re-running the deterministic
transaction with one more row than the last attempt, each row carrying an **empty** external
list. A row asserts nothing: the shipped kernel decides whether it needs an external
resolution, and if it does, that becomes the stop.

| stop | primitive | retail VA |
|---|---|---|
| `place_all_mountain_range_lists` | `Mountains::randomize_mountains` | `0x0089ca70` |
| `place_all_region_helping_globals` | `is_helping` and friends | `0x00cae708` |
| `place_all_mountains_add_mountain` | `Mountains::add_mountain` | `0x0089c2e0` |
| `place_all_cliffs_verify_defensive` | `Cliffs::verify_defensive_position` | `0x008a8680` |
| `place_all_cliffs_position_cliff` | `Cliffs::position_cliff` | `0x008a8bc0` |
| `place_all_world_set_oil_at` | `World::set_oil_at` | `0x006b2a10` |
| `place_all_player_growth_kernel` | `TerrainGroup::place_player_group` | `0x006a4190` |
| `place_all_region_return_control` | `TerrainGroup::place_region_group` | `0x006a2f60` |
| `place_all_add_doobers` | `TerrainGroups::add_doobers` | `0x006a1540` |
| `place_all_treeify_mountains` | `TerrainGroups::treeify_mountains` | `0x006a1cc0` |
| `place_all_reporting_tail` | the localized reporting tail | `0x006a8f12` |
| `place_all_complete` | `return 1` | `0x006a937d` |

### Exact owners supersede recorded oil/mountain answers

`World::set_oil_at` `0x006b2a10` sets or clears `WData::OIL` — a `world`-channel byte the
shipped `don-sim` setter already writes — and additionally closes or creates a `Good` object,
which belongs to the Goods channel. `don-sim` surfaces this as an external whose
resolution carries only an echo of the request. The compatibility survey uses
`OilGoodPolicy::Stop` when no owner is mounted, so a recorded row never invents the object effect.
`OilGoodPolicy::ContinueRecordingGoodEffects` exists for the survey in §5 and records every
crossed request; the record states which policy produced each row.

The current owned entry instead mounts the cold-process `OilGoodRuntime` and, when explicit
installed geometry is available, `MountainAddRuntime`. Those owners execute at the exact
player/region call site and retain typed receipts; they do not consume the policy's asserted
resolution rows. A cold Great Lakes survey therefore crosses the oil calls and stops first at
group 2's missing installed mountain catalog.

When every selected group, both doober passes, and the map-style treeification gate complete,
`PlaceAllPreviewReceipt::post_placement_authority` retains the final staged World/checksum, RNG
word, mountain-range cursors, TerrainGroup state, and exact mounted owner state. The remaining
localized reporting transaction reads the internally accumulated score table, emits strings, and
clears `console_info`; it cannot mutate World, RNG, Mountains, or the subsystem owners. The owned
survey now supplies that derived table, reaches native `return 1`, and emits both the final
authority and a `ReplayPlaceAllReceipt` in the exact shape consumed by setup entry.

## 5. Where each dominant style stops, and what is beyond it

Measured with the derived range lists installed, so these are real RNG states, not a
hypothesis. The arm order is the data-driven terrain-group order from
`ron-data/mapstyles/*.xml`.

- **Mediterranean (style 12, 8 of the 21 checksum-bearing recordings, 11 of 61 files)** — 15 groups, all
  `chance="100"`. Group 0 is `type="mountains" pattern="nonplayer"`, so it enters
  `place_region_group` `0x006a2f60` and stops at `Mountains::add_mountain` `0x0089c2e0`
  before completing any group.
- **Great Lakes (style 14, 6 of the 21)** — 10 groups, all `chance="100"`. Group 0 is
  `type="trees" pattern="player"`, and its growth tail reaches `World::set_oil_at`
  `0x006b2a10`. The exact cold-process Good owner now crosses those calls. Groups 0 and 1
  complete at the dispatcher edge `0x006a8ee5`; group 2, `type="mountains"`, is the first cold
  stop at `Mountains::add_mountain`. A synthetic explicit installed catalog exercises the
  mode-5 owner, derives the reporting table, and reaches native `return 1` with final authority.

So after the range lists, the common cold blocker is
`Mountains::add_mountain` `0x0089c2e0`. `add_mountain` needs the mountain
template geometry — the `.\art\*_disp_0.tga` art the `MOUNTAINS` section names, which
`ron-data/` does not contain. If that holds, it is the next extraction question and the same
class of blocker `tilesets.xml` and `internal_strings.xml` were before they were extracted.
`Cliffs::position_cliff` `0x008a8bc0` is behind both, and **[reported by the `don-sim` port's
own note]** it consumes zero or one main-stream draw depending on how many cliff templates
are eligible — so unlike the oil acknowledgement it cannot be crossed without a real return.

## 6. What is deliberately not claimed

- **No world bytes were sourced.** The `world` channel is unchanged: 0 matches over 222,938
  non-trivial compares, first divergence still the first checksummed turn. This lane moved a
  boundary and corrected an evidence matrix; it did not generate terrain.
- **Nothing here is oracle-executed.** `Mountains::randomize_mountains`,
  `Mountains::add_mountain`, `Cliffs::position_cliff` and `place_all` itself have no
  differential retail case. Tier **C**.
- **The `TileSetGroupData` XML parser body was not read.** The sixteen-element binding in §2
  rests on a 16-of-16 name-and-order correspondence with the PDB layout plus the independently
  confirmed `CLUMP_FACTOR`, not on the parser's instructions.
- **The mountain range-list payload order is inherited, not verified here.** The lengths and
  the two-draw RNG cost are measured; that `Mountains::init` walks the `<MOUNTAIN>` elements
  in document order was reported by a sibling research pass and not re-derived. It affects
  only which template a given RNG value selects, first read at `0x006a82c5`.
- **The mountain template geometry is not held.** `Mountains::add_mountain` needs the
  `MOUNTAINS` section's `.tga` displacement art, which `ron-data/` does not contain.
- **The installed template pixels are still absent.** The recovered mode-4/mode-5 runtime owns
  `verify_bits`, including exact set/clear discipline and the mode-5 cross-candidate transaction,
  but no checked-in source can supply the 16 canonical displacement surfaces.
- **Per-group `is_helping` for late groups is untested.** `don-sim` recomputes it per clump
  from its own threshold; the derived entry value is only exercised on the first group,
  because the corpus stops there.

## 7. Ledger entries to add to `docs/provenance-ledger.md`

| mechanic | source | tier | evidence |
|---|---|---|---|
| The `Mountains` per-game placement arrays are empty at `place_all` entry for every map style | `World::wipe` `0x006b2c00` @ `0x006b2d97` → `Mountains::clear` `0x0089bec0` | structural [measured] | all 21 `make_continents` overrides call `World::wipe` on the straight-line path; `add_mountain` `0x0089c2e0` is only reachable from inside `place_all` |
| `Mountains::randomize_mountains` draws from the **main** stream, once per range list of length > 1 | `0x0089ca70`, called at `0x006a7330`; `GameAccess::game_random` `0x00c06184` | structural [measured] | `lea eax,[esi-1]; test eax,eax; jg` at `0x0089ca80`/`0x0089cac4`/`0x0089cb08`, then `Random::get(0,0xffff)` and `cdq; idiv` |
| The three mountain range lists are shipped data: `ron-data/effects_graphics.xml` `<MOUNTAINS>` holds 16 `<MOUNTAIN>` elements, `area` `lg`×7 / `med`×8 / `sm`×1, so lengths are `1 / 8 / 7` and `randomize_mountains` costs exactly **two** main-stream draws | `Mountains::init` `0x0089ad70` → `add_range` `0x008992b0`, reached at `0x0087b24e` ← `0x0069dda0` ← `0x0068bd52` | structural [measured] | shipped element census plus the disassembled load chain; head-first node order and `metric = 0` from `LinkListBase::add` `0x004a4af0` |
| `place_all`'s `progress` argument is `1` in a normal game start and is presentation-only | `0x0068c009` ← `0x005ac657` ← `0x0058d1b0` / `0x0058cab9`; tested at `0x006a79f1`, `0x006a86f1`, `0x006a8a61` | structural [measured] | both `Game::run` dispatches pass `Game::init param_3 = 0`; the three guarded blocks are splash captions with no RNG draw or simulation write |
| `is_helping`, `lowest_player[5]` and `player_scores[8][5]` are initialised by `place_all` itself, not by game setup | `0x006a7618`, `0x006a75ab`/`0x006a75b4`, `0x006a75ee`–`0x006a760c`, `0x006a72eb`–`0x006a72fb` | structural [measured] | first `is_helping` read is `0x006a82d0`; only rows `[0, num_players)` of `player_scores` are zeroed |
| `TerrainGroups::console_info` `+0x78` gates nothing | sole writer `place_all` `0x006a91b5`; no reader in the image | structural [measured] | Capstone over all 15 `TerrainGroups` methods plus a whole-`.text` scan for `[reg+0x1b8]` on a `Map` |
| `TerrainGroups+0x24`'s three `Array<int>` are never touched by `place_all` | Capstone over `0x006a70d0`–`0x006a93a3` | structural [measured] | every `this` dereference is `[this+0x10]`, `[this+0x04]`, or the `[this+0x78]` write |
| The selected tileset's `TileSetGroupData` is the sixteen `TERRAINGROUP` children of `Data/tilesets.xml`, in PDB offset order | PDB `TileSetGroupData` (16 `int`s, size 64) | structural [measured], parser body unread | 16-of-16 name and order match; field 0 independently confirmed as `CLUMP_FACTOR` by the fertility partition path |
