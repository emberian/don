# The save game, and therefore the simulation-state schema

Lane: `savegame`. Target: `ron-bin/riseofnations.exe` (PE32 i386, image base `0x00400000`,
2024-06-20 MSVC rebuild), plus 13 real specimens. Every claim is marked **[measured]** (I
verified it here, against the binary or against real files) or **[reported]** (read, not
checked). Nothing here is "verified" in the proof-assistant sense — see `docs/CHARTER.md`.

---

## Headline

`docs/derivation/checksum.md` established that `CheckSum`, `SaveGame` and `LoadGame` are
siblings of one two-method visitor `DataWalk`, so **sim-critical state ≡ save-game state**,
and every `walk_data` in the binary is a machine-readable declaration of which bytes of a
class are simulation state. This lane cashed that in.

1. **The schema is recovered and machine-readable.** `schema/state-schema.json` — 278
   `walk_data` implementations (229 with a PDB class name), **1,421 ordered operations,
   899 byte ranges of which 647 fully resolved, 1,316 named fields, 38,275 bytes of
   sim-critical state**. Per class: the ordered field list with PDB names, types and
   sizes. [measured]
2. **A working parser** at `re/scripts/savegame_parse.py` parses the `SaveGame` prefix of
   `.rcx` recordings *and* of `.svx` save games, including 2003-era and Conquest saves.
   Seven `.svx` files were pulled out of the VM for this lane. [measured]
3. **`rise.pdb` is present, and it is this exact build's.** `ron-bin/sbl/rise.pdb` GUID
   `51D4F219-61C6-4F84-9D5B-C3361B0D291F` age 1 is byte-identical to the PE's own CodeView
   record, and it carries the **full TPI type stream** (305,734 type records, 22,771 named
   tags). So we have real class names, field names, field offsets and `sizeof` for the whole
   simulation. This changes the economics of every remaining lane. [measured]
4. **The section tag byte is now predictable, not just observed.** It is the low byte of
   `String::generate_hash`'s case-insensitive output for the section *name*:
   `hash_ci("game") & 0xff == 0x16`, `hash_ci("gameinfo") == 0x42`, `hash_ci("player") ==
   0x50` — the three tag bytes real files carry. This closes `replay-io.md` §7.4. [measured]
5. **`GameInfo+0x04` is literally named `seed`.** `replay-io.md` listed "GameInfo `+0x04` is
   the game seed" as *hypothesis, not measured*. It is `unsigned long GameInfo::seed`. [measured]
6. **`[[0x00c0618c]+0x1f4]` is `Objects::valid`, not `checksum_deep`.** `checksum.md` flagged
   the identification of the flag that gates four checksum channels as an unresolved
   hypothesis. It is refuted: `+0x1f4` is `int Objects::valid`, and `Objects::walk_data`
   serialises it as the first field of the section. [measured]

---

## 0. Evidence standard

"Parses to EOF with no residue" is nearly worthless on its own — a parser that consumes a
variable-length blob will always reach EOF. Everything below is backed by at least one of:

- **a fixed byte at a computed position** — the parser predicts an offset from structure
  alone, and a specific constant has to be sitting there — 8 player tag bytes per file ×
  the 12 specimens that have a `GameInfo` section = 96 independent 1-in-256 checks, plus
  the `Game` and `GameInfo` tags;
- **an internal arithmetic identity** — `Game::semaphore.bits == 8 * semaphore.size` with
  `size ≤ 32` because the PDB says the buffer is `unsigned char[32]`;
- **cross-file agreement** across 13 specimens spanning **four engine builds and twelve
  years** (2014 → 2026), including two *different container formats*;
- **agreement with artifacts derived independently and earlier** — `schema/bindings.json`,
  built months ago from the `log_data` descriptor visitor with no PDB available (98.8 % on
  571 comparable name↔offset bindings), and `docs/derivation/replay-io.md`'s hand-measured
  per-file header offsets (6 of 6 exact).

Reproduce all of it:

```sh
cd /Users/ember/dev/don
uv run --quiet --with pefile python re/scripts/savegame_parse.py --verify <files...>
```

Result: **55 predicates, 0 failures, 13 specimens** [measured]. See §6.

---

## 1. The artifacts

| path | what |
|---|---|
| `re/scripts/walk_extract.py` | scans every function in the image, recovers each `walk_data`'s ordered op list at the instruction level → `schema/walkops.json` |
| `re/scripts/gen_state_schema.py` | joins those ops with the PDB symbol table and TPI type stream → `schema/state-schema.json` |
| `re/scripts/pdb_layout.py` | flattens a PDB class layout to leaf fields (offset, size, name, type) |
| `re/scripts/savegame_parse.py` | the parser: `.rcx` / `.svx` / CTW map saves; also `--schema CLASS`, `--toc`, `--tag NAME`, `--verify` |

```sh
uv run --quiet --with pefile --with capstone python re/scripts/walk_extract.py
python3 re/scripts/gen_state_schema.py
uv run --quiet python re/scripts/pdb_layout.py Unit Player GameInfo
uv run --quiet python re/scripts/savegame_parse.py --schema Unit
uv run --quiet python re/scripts/savegame_parse.py --toc
uv run --quiet python re/scripts/savegame_parse.py ron-data/replays/today.rcx --hex
```

---

## 2. `rise.pdb`, and how far to trust it

`ron-bin/sbl/rise.pdb` is a **private** PDB — public symbols *and* the TPI type stream.
Verified against the binary [measured]:

| | value |
|---|---|
| PE `IMAGE_DEBUG_TYPE_CODEVIEW` | `RSDS`, GUID `51D4F219-61C6-4F84-9D5B-C3361B0D291F`, age `1`, `E:\agent\_work\2\s\main\game\rise.pdb` |
| PDB stream 1 (PDB info) | version `20000404`, age `1`, GUID `51D4F219-61C6-4F84-9D5B-C3361B0D291F` |

Identical GUID and age ⇒ the PDB describes **this** image, not a neighbouring build.

This is *vendor debug information*, which sits differently in the charter's tiers than
"derived from the binary". My rule for this lane:

- **Structure and order come from the instruction stream.** Which bytes get written, in
  what order, under what condition — all of that is read off the disassembly. The PDB is
  never the source of a *layout* claim.
- **The PDB supplies names and types for offsets we already recovered.** It is a decoder
  ring, not a derivation.
- **Where the two can disagree, I checked.** They agree — see §6.5 (98.8% agreement with
  `bindings.json` on 571 comparable name↔offset bindings, all 7 exceptions being
  array-element naming rather than layout disagreement).

Immediate confirmations of earlier lanes, each of which could have failed [measured]:

| earlier claim | its source | PDB says |
|---|---|---|
| `check_all` = `FUN_00936560` | UTF-16 log literals | `?check_all@CheckSums@@QAEKXZ` |
| `SaveGame::walk(begin,end)` = `0x0043d730` | RTTI + vftable | `?walk_function@SaveGame@@UAEXPAX0@Z` |
| `CheckSum::walk_tag` = `0x0041bfe0`, a no-op | vftable slot 1 | `?walk_test@CheckSum@@UAEXABVString@@@Z` |
| leader stride `0x6eec` | `check_all`'s loop bound | `sizeof(Leader) == 28396 == 0x6eec` |
| `Player` stride `0x8c` in GameInfo | replay-header arithmetic | `sizeof(Player) == 140 == 0x8c` |
| `[0x00c06188]+0x134` / `+0x138` / `+0x15c…` are the world SoA arrays | devirtualised checksum fast paths | `World::wdata`, `World::tdata`, `World::seen/seen2/seen3` |
| object vtable `+0x78` is a "should I walk this" predicate | decompiled `walk_data` bodies | `?must_walk@Unit@@UAEHPAUDataWalk@@@Z` |

The correct method names are **`walk_function`** (slot 0) and **`walk_test`** (slot 1), not
the `walk` / `walk_tag` we had been calling them. `walk_test` taking `const String&` is why
the tag is a String hash byte.

---

## 3. The serialisation grammar

All of it, in one place [measured]. `SaveGame::walk_function` is
`fwrite(begin, 1, end-begin, f)` (or `gzwrite`): **raw bytes, no framing, no alignment, no
type tags**. So the whole format is "the concatenation of the byte ranges the traversal
pushes, in program order".

| construct | encoding | provenance |
|---|---|---|
| integers / structs | raw little-endian, exactly `end-begin` bytes | `0x0043d730` |
| section tag | **one byte** = `String::module_id` (`String+0x10`) | `0x0043d840` |
| `String` | `u32 character_count`, then that many UTF-16LE code units, **not** NUL-terminated; count 0 emits only the `u32` | `String::walk_data` `0x00a1b2d0` |
| `Array<T>` / `SimpleArray<T>` / `ObjectArray<T>` / `PtrArray<T>` | `u32 length`; if non-zero: `u32 size`, `u16 increment`, `u8 flags`, then the elements | `0x00471c30`, `0x00473120`, `0x00474420`, `0x0045cce0` |
| `SimpleArray<T>` elements | ONE bulk `walk(list, list + length*sizeof(T))` | `0x00473120` tail |
| `Array<T>` / `ObjectArray<T>` elements | per-element `walk_data` | |
| `PtrArray<T>` elements | ids, never pointers | `0x0045cce0` |
| `Stack<T>` | `walk(this+4, this+0xd)` = 9 B `{int size; int length; char increment}`, then `length` × `sizeof(T)` | `0x0046d8b0` |
| var-length inline buffer | `walk(p, p+8)` then `walk(p+0xc, p + *(u32*)(p+4) + 0xc)` — i.e. an 8-byte header whose second dword is the payload length, payload at `p+0xc` | `Game::semaphore`, and ten instances inside `LeaderData::walk_data` |
| pointers | never serialised; either an id is extracted from the pointee and walked (`SubObject::walk_data` `0x006621d0`) or the pointee's `walk_data` is called | |

There is **no** self-describing structure beyond the one-byte section tags. A parser must
know the traversal.

### The tag byte, derived rather than observed [measured]

`walk_test` writes `String::module_id`, which `String::generate_hash` (`0x00a1b6b0`,
`?generate_hash@String@@SAKPB_WPAK@Z`) fills in. The function returns the case-**sensitive**
hash and stores the case-**insensitive** hash through its out-param; `walk_test` passes
`&String+0x10` as that out-param and writes its low byte. With `T` = the 50 `int`s at
`0x00b14500` (`127, 811, 1597, 2131, 2749, 4759, 5527, 5953, …`):

```
h = 0;  L = len
for u = len-1 downto 0:
    c = towlower(s[u])
    h += T[c % 50] * L + c * T[u % 50]          (mod 2^32)
    L -= 1
tag = h & 0xff
```

Two independent confirmations:

- `hash_ci("game") = 0x000a3c16` → **`0x16`**; `hash_ci("gameinfo") = 0x002c1942` →
  **`0x42`**; `hash_ci("player") = 0x001af750` → **`0x50`**. Those are precisely the three
  tag bytes that the 13 real files carry at those three positions. Three exact 1-in-256
  hits on names guessed from the class names.
- The shipped `Data/internal_strings.xml` (pulled from the VM, 7,621 entries) declares a
  `hash="…"` per string. My reimplementation of the **case-sensitive** branch of the same
  function reproduces **7,575 of 7,621** declared hashes exactly. (The ~46 misses are
  entries containing XML entities I did not unescape.)

```sh
uv run --quiet --with pefile python re/scripts/savegame_parse.py --tag gameinfo
# gameinfo   hash_ci = 0x002c1942   tag byte = 0x42
```

**Still open:** the tag *names* for the other 86 tag sites (87 distinct string-table
indices are used across the image). Each call site pushes
`int_str_array[i]` — `?int_str_array@@3PAVStringTable@@A` at `0x00c06378`, an array of
`String` at `+0x10`, stride 20 (= `sizeof(String)`, PDB-confirmed), with a compile-time
constant index (e.g. `0xd980/20 = 2784` for `game`). The index is *not* the
`internal_strings.xml` element order — I checked, and "game" is not even in that file. One
`ReadProcessMemory` of `[[0x00c06378]+0x10]` in the live game would dump all 87 names in a
single shot; I did not do it.

---

## 4. The containers

| file | container | opens with |
|---|---|---|
| `.rcx` recorded game | gzip, one member, offset 0 | **nothing** — straight into `Game::walk_data` |
| `.svx` save game | gzip, one member, offset 0 | `String` magic + `u32 sGameSaveVersion` |
| `.svx` CTW map backup | **raw, not compressed** | `String "RonCTWMapSave"` + `u32 version` |

`SaveGame::save_game` = `FUN_005a8220` [measured]:

```c
File::open(path, 3);                        // "wb" through the gz writer
magic = (game[0x822] & 2) ? L"RoNCTWSave"
      : (game[0x820] & 4) ? L"RoNMultiSave" : L"RoNSave";
String::write(magic);                       // u32 count + UTF-16LE
sGameSaveVersion = 0x10;                    // [0x00c06240]
fwrite(&sGameSaveVersion, 1, 4, f);
do_save();                                  // 0x005a81f0 -> WalkDataGame::walk_data 0x005a2360
```

`[0x00c06240]` is `?sGameSaveVersion@@3HA` [measured] — the save-format version. This build
writes **16**; the 2014 EE build wrote **15**. It is a real format switch, not a label:
`GameInfo::walk_data` branches on it at `0x005d6734` (§5).

Observed magics and versions across the seven `.svx` I pulled: `RoNSave` v16 (2024),
`RoNMultiSave` v15 (2014), `RoNCTWSave` v15 (2014), `RonCTWMapSave` v15 (2014, raw).

A recording carries **no magic and no version word** — it begins at `Game::walk_data`. For
`.rcx` the parser therefore infers the format version by trying both `GameInfo` tails and
keeping the one whose `Game` block satisfies `semaphore.bits == 8*semaphore.size, size ≤ 32`
(§6.2). It picks 15 for the 2003-engine specimen and 16 for the five EE ones. [measured]

---

## 5. The recovered layouts

### 5.1 `GameInfo::walk_data` — `FUN_005d6570`

Order and inclusion from the instruction stream (`0x005d6596`–`0x005d7065`); names, offsets
and types from the PDB. `this` = `game+0x0c`.

```
u8      <tag>                                       0x42 = hash_ci("gameinfo")
if (!checksum):
  String  version_string                            "(Version: 00.2024.06.2000)"
  u32     GameInfo::version              gi+0x00    format/compat stamp, constant per build
u32       GameInfo::seed                 gi+0x04
i32       GameInfo::checksum_deep        gi+0x08
i32       GameInfo::checksum_window_size gi+0x0c
i32       GameInfo::checksum_failure_threshold gi+0x10
u32       GameInfo::flags                gi+0x14    (CheckSum instead walks flags & ~1)
u8 x30    gi+0x18 .. gi+0x36                        29 separate 1-byte walks, then 1 more
          team_style, map_style, map_size, players, max_observers, game_speed, game_rules,
          difficulty, starting_town, starting_resources, starting_resources2, tech_cost,
          reveal_map, pop_limit, rush_rules, cannon_times, starting_technology,
          starting_technology2, ending_technology, elimination, victory, wonderwin,
          score_goal, popwin, time_limit, chairs, econwin, scenario_type, script_type, mods
if (checksum) return
8 x Player   p = gi+0x38 + i*0x8c:
      u8    <tag>                                   0x50 = hash_ci("player")
      u16   Player::flags                p+0x30
      if (flags & 1):
        57 B p+0x00 .. p+0x39                       synced_used_zoom_control,
              synced_frames_zoomed_in, synced_frames_zoomed_out, synced_clicks,
              synced_hotkeys, synced_minimap_clicks, synced_mainmap_clicks, synced_cheated,
              synced_control_groups_formed, synced_control_groups_activated, caravan_frame,
              pop_cap_frame, flags, tribe, who, team, handicap, play, pauses, diff
        String Player::name               p+0x40
if (sGameSaveVersion >= 16):                        0x005d68ac
      u32 mod_checksum; u32 mod_total_size
      String scenario_script (gi+0x4f4); String scenario_path (gi+0x508); String mod_name
      u32 mod2_checksum; u32 mod2_total_size; String mod2_name
else:                                               0x005d6fe5
      String scenario_script; String scenario_path; String scenario_dir; String mod_name
```

Note what is **excluded**: `Player::elo` (`p+0x3c`) sits inside the struct but outside the
57-byte walk. `Player::platform`, `platformID`, `net_player` and the `accum_*` counters are
likewise not sim state. The engine's own line between "simulation" and "presentation/metadata"
is drawn at `p+0x39`.

### 5.2 `Game::walk_data` — `FUN_00589600`

```
u8      <tag>                             0x16 = hash_ci("game")
        GameInfo::walk_data(game+0x0c)
404 B   game+0x550 .. game+0x6e4          frame, frame_to_break, playing, loading, tick,
                                          market_tick, market[6], ... world_cities,
                                          world_villages, total_units, everyone_mask,
                                          armageddon
if (!checksum):
  8 B   game+0x814 .. game+0x81c          semaphore.bits, semaphore.size
  N B   game+0x820, N = semaphore.size    semaphore.ptr (unsigned char[32])
  4 B   game+0x844 .. game+0x848          graphic_tick
```

A recording then writes `String::walk_data(game.info.save_name)` (`game+0x53c` =
`GameInfo+0x530`), and that is the end of the header. On `today.rcx` the header is
`0x349` bytes.

The production Rust reader now consumes this complete prefix in
`crates/don-replay/src/initial.rs`. It tries the two measured v16/v15 tails and accepts one
only when the following PDB-sized semaphore satisfies `bits == 8*size`, `0 <= size <= 32`;
all 61 structurally decoded command streams in the local corpus pass. The parser exposes
the setup as initial state and stops at `save_name`: it does not relabel the following
opaque block as a decoded save snapshot.

### 5.3 `WalkDataGame::walk_data` — `FUN_005a2360` — the save-game table of contents

**This function is absent from `schema/islands.jsonl`** — a Ghidra gap of exactly the kind
`README-LLM.md` warns about, and it is the root of the whole save format. It was recovered
by rebuilding the function list from the PDB's `.text` symbol starts. [measured]

`uv run --quiet python re/scripts/savegame_parse.py --toc` prints all 91 ops. In order:

```
GameInfo::walk_data
Console       [0x00c06210] +0x298..0x2b8 (32 B), +0x2b8..0x2c2 (10 B), +0x2c4..0x338 (116 B)
obj_base      [0x00c06198] +0x0..0xc  (12 B)      obj_end [0x00c06190] +0x0..0xc (12 B)
Game::walk_data
<tag>  ObjectArray<Tribe>  Leaders  Types  TileSet  Mountains
Constants     [0x00c061f0] +0x0..0xd40 (3392 B), +0x804..0x808
              [0x00c061c0] 4 B     [0x00c061c4] 4 B
Armies  Cities
<tag>  ObjectArray<Form>  PtrArray<Good>  PtrArray<Item>  Heroes
<tag>  PtrArray<Herd>  Specials  Wonders  Forts  Docks  OilWells  Supplies  Caravans
<tag>  ObjectArray<Land>  LeaderOptions  OptionInfo  Array<Group>
<tag>  Objects
<tag>  Array<HotKeyGroup>  World  [0x00c061bc] 40 B  [0x00c06184] 4 B
       GraphicEvents  Scene
<tag>  Array<FarmStruct>  UnbuiltWonders  UnbuiltCities  UnbuiltForts  ConquestGame
<tag>  [0x00c06200] +0x28c..0x370 (228 B), +0xb4..0x288 (468 B)
<tag>  Array<SelectGroup> x2  Array<Option>
<tag>  [0x00c06204] +0x20..0x7c (92 B), +0x7c..0x8e (18 B)
       CommandManager  PtrArray<River>  Terrain::walk_coord_data  MessageWin
       Terrain::walk_data  CliffsData  Doober
       [0x00c061b8] +0x18..0x20   ObjectArray<Region>  Array<WCoordData>
       Terrain::walk_roads
       Achieve  ScenarioData  RunTimeEnv
<tag>  [0x00c06180] +0x4..0xc (8 B), +0x14..0x169 (341 B)
       Game::walk_rules_data
```

The globals are named by the PDB: `?world@GameAccess@@`, `?objects@GameAccess@@`,
`?game@GameAccess@@`, `?constants@GameAccess@@`, `?console@MiscAccess@@`,
`?obj_base@GameAccess@@`, `?obj_end@GameAccess@@`. [measured]

### 5.4 Object classes — the sim-state schema proper

`schema/state-schema.json` carries all 278. Examples (`--schema CLASS`):

| class | `walk_data` | `sizeof` | sim-critical bytes | coverage |
|---|---|---|---|---|
| `LeaderData` | `0x006d6750` | 28,388 | 27,182 | 96 % |
| `Form` | `0x0072df10` | 3,736 | 3,688 | 99 % |
| `Tribe` | `0x006f1270` | 1,520 | 1,432 | 94 % |
| `UnitType` | `0x0061d190` | 1,496 | 792 | 53 % |
| `City` | `0x00489220` | 192 | 110 | 57 % |
| `Unit` | `0x0060cf40` | 344 | 111 | 32 % |
| `Objects` | `0x006541e0` | 824 | 142 | 17 % |
| `GameInfo` | `0x005d6570` | 1,348 | 83 | 6 % |
| `Personality` | `0x006d8700` | 96 | 96 | **100 %** |
| `Diplomacy` | `0x006d8a80` | 92 | 92 | **100 %** |

128 classes carry at least one PDB-named serialised field; the rest either walk only
sub-objects or walk ranges on globals rather than on `this`.

`Unit::walk_data`'s single 111-byte range `[0x48, 0xb7)` — the number `checksum.md` measured
with a completely different extractor — resolves to **55 named fields**: `collide_frame`,
`damage_frame`, `angle`, `dest_angle`, `trench_angle`, `tolerance`, `queue_time`,
`unit_masks`, `unit_masks2`, `orders_x/y`, `los_x/y`, `group`, `inside_up`, `supply`,
`collide`, `collide_o`, `collide_guy`, `o_up`, `o_down`, `gather_down`, `good_obj`,
`mana_burn`, `spell_time`, `myspeed`, `myarmor`, `attrition`, `num_queued`, `cavarch_o`,
`damage_o`, `cavarch_uid`, `cavarch_who`, `damage_who`, `form`, `form_mod`, `full`,
`waiting`, `recharging`, `path_recursion`, `idle`, `stance`, `safe`, `collide_who`,
`inside_up_who`, `guy_mark`, `play`, plus a union at `+0x54` (`rare` / `air_alt` /
`former_type`) and one at `+0x86` (`hero` / `cara` / `doober` / `special` / `herd`).

`Objects::walk_data` (`0x006541e0`) opens with `walk(this+0x1f4, this+0x1fc)` =
`Objects::valid`, `Objects::ammo_index`, and then walks `unit_mark`, `build_mark`,
`wall_mark` as **36 of 40 bytes** each — nine of ten `int` slots. That is the same 8-vs-9
leader-slot asymmetry `checksum.md` noted, seen from the other side.

---

## 6. Validation

### 6.1 The predicates

`savegame_parse.py --verify` over 13 specimens — 6 `.rcx` and 7 `.svx` [measured]:

```
today.rcx        (Version: 00.2024.06.2000)  3 players  prefix 0x349
mp2014.rcx       (Version: 03.02.03.2905)    8 players  prefix 0x5b0
mp2020.rcx       (Version: 00.2017.11.2900)  4 players  prefix 0x394
mp2024.rcx       (Version: 00.2017.11.2900)  2 players  prefix 0x2f8
solo2020.rcx     (Version: 00.2017.11.2900)  4 players  prefix 0x396
solo2024.rcx     (Version: 00.2017.11.2900)  4 players  prefix 0x396
ctw2014.svx      (Version: 00.2014.07.1000)  RoNCTWSave    v15
ctwback2014.svx  RonCTWMapSave, raw (no gzip, no GameInfo)
friend2014.svx   (Version: 00.2014.07.1000)  RoNMultiSave  v15
friend21_2014.svx / new2014a / new2014b      RoNMultiSave  v15
ref2024.svx      (Version: 00.2017.11.2900)  RoNSave       v16

55 predicates passed, 0 failed, over 13 specimens
```

Per file: `GameInfo` tag byte is `0x42`; all eight `Player` slots present with tag `0x50`;
`version_string` matches `^\(Version: [\d.]+\)$`; and for the `.rcx`: `Game` tag `0x16`,
`semaphore.bits == 8*semaphore.size ≤ 256`, `frame_to_break == -1`.

### 6.2 The identity that could have failed

`Game::semaphore` is `{int bits; int size; int flags; unsigned char ptr[32]}` per the PDB,
and `Game::walk_data` writes `bits`, `size`, then `size` bytes of `ptr`. If my `GameInfo`
layout were wrong by even one byte, the two dwords read at the wrong place would not satisfy
`bits == 8*size` with `size ≤ 32`. Six of six `.rcx` satisfy it (`bits=256, size=32`), and
that is exactly what the parser uses to *pick* the format-version variant — which
independently produces 15 for the 2003-engine file and 16 for the five EE files, matching the
`sGameSaveVersion` values the `.svx` files state explicitly.

`frame_to_break == -1` in all six is a second such check: it lands at the offset the layout
predicts, and it is the value a game that has never been asked to break on a frame should hold.

### 6.3 Cross-build agreement

`GameInfo::version` (`gi+0x00`) is constant per engine build and different across builds
[measured]:

| version string | `GameInfo::version` | seen in |
|---|---|---|
| `03.02.03.2905` | `0x051d0362` | `mp2014.rcx` |
| `00.2014.07.1000` | `0x05ee0106` | 4 × `.svx` |
| `00.2017.11.2900` | `0x06d2013a` | 4 × `.rcx` **and** `ref2024.svx` |
| `00.2024.06.2000` | `0x06ac012c` | `today.rcx` |

The `00.2017.11.2900` row is the strongest: the same word, at the same computed offset, in
both container formats, from files recorded four years apart.

The 2003-engine file is the sharpest single test of the whole layout: `mp2014.rcx`'s player
loop ends at exactly `0x2ac`, which is where `docs/derivation/replay-io.md` independently
measured its header to end — and the four Strings that follow decode as
`""`, `"C:\Users\corey_000\Documents\My Games\Rise of Nations\…"`, … i.e. the
`sGameSaveVersion < 16` tail, correctly selected by the semaphore identity.

### 6.4 Agreement with `docs/derivation/replay-io.md`

That lane measured, by hand, where each replay's player loop ends, using a different method
and before any PDB existed. My parser computes the same offset structurally. **6 of 6 match
exactly** [measured]:

| file | my parse | `replay-io.md` §3 |
|---|---|---|
| `today.rcx` | `0x165` | `0x165` |
| `solo2024.rcx` | `0x1b2` | `0x1b2` |
| `solo2020.rcx` | `0x1b2` | `0x1b2` |
| `mp2024.rcx` | `0x114` | `0x114` |
| `mp2020.rcx` | `0x1b0` | `0x1b0` |
| `mp2014.rcx` | `0x2ac` | `0x2ac` |

Everything *after* that offset — the mod block, the 404-byte `Game` block, the semaphore, the
save-name string — is new in this lane, and the parser now consumes `0x349` of `today.rcx`
where `replay-io.md` stopped at `0x165`.

### 6.5 Agreement with `schema/bindings.json`

`schema/bindings.json` was produced months ago by a different lane from the `log_data`
descriptor visitor, with no PDB in the repo. Joining its 1,224 `(function, name, offset)`
bindings to the PDB layouts of the classes those functions belong to [measured]:

- **571 comparable bindings** (excluding 463 where the bindings extractor recorded
  `field_off == 0` or an `[scan]` array name — its own known limitation — and 178 in classes
  with virtual bases my flattener does not resolve);
- **564 agree on both name and offset — 98.8 %**;
- the 7 exceptions are all element-index naming (`turret_angles[0]` at `+0x30` vs the PDB's
  `turret_angles` array at `+0x30`), not layout disagreements.

The most direct instance is the one this lane needed. `FUN_005d6040` — which
`checksum.md` called "the settings binder" — is `?log_data@GameInfo@@`, and it binds:

| bindings.json name | offset | my parse |
|---|---|---|
| `(int) version` | 0 | `version` |
| `(int)seed` | 4 | `seed` |
| `checksum_deep` | 8 | `checksum_deep` |
| `checksum_window_size` | 12 | `checksum_window_size` |
| `checksum_failure_threshold` | 16 | `checksum_failure_threshold` |
| `flags` | 20 | `flags` |

Six for six, from a completely independent extraction. That also settles a `checksum.md`
loose end: the object those three `checksum_*` settings live on **is** `GameInfo`.

---

## 7. Method, and what it does not do

`re/scripts/walk_extract.py` linearly abstract-interprets every function with capstone,
tracking `this` (entry `ECX`), the walker (`[ebp+8]`), frame- and global-relative values, and
the push stack. A `walk_data` is any function that calls `walker->vt[0](begin,end)` or
`walker->vt[1](tag)`.

Two things were needed to make it work and are worth remembering:

- **`schema/islands.jsonl` is not a complete function list** and the gap is load-bearing —
  `WalkDataGame::walk_data`, the root of the save format, is missing from it. Function bounds
  are now the union of the PDB's `.text` symbol starts and Ghidra's list.
- **A linear scan walks straight through the epilogue of an early return**, where `pop ebx`
  destroys the walker binding for every basic block after it. The scanner snapshots the
  register state once the walker binding is established and restores it at each `ret`.

Honest limits [measured that they exist]:

- No dataflow join. Ops whose operands are computed across branches are unresolved
  (`bytes: null` + listed in `unresolved_ops`).
- Loop trip counts are not recovered; `in_loop` and `guard_depth` are **over-approximations**
  computed from back-edge and forward-branch spans, so compiler loop rotation and tail
  merging can make a straight-line op look guarded. They are "look here", not control flow.
- Virtual dispatch (`+0x7c` `walk_data`, `+0x78` `must_walk`, `+0xac`, `+0xb0`) is recorded
  as a `virtual` op, not resolved per receiver.
- The ranges are the **SaveGame/LoadGame** path. `CheckSum` takes different branches
  (`walker+0x08 != 0`) and is further gated by the section mask at `walker+0x0c`.
- The parser decodes the *prefix* of a save: magic, version, `GameInfo`, `Console`, and for
  a recording the whole `Game::walk_data` header. It does **not** parse the ~1 MB body; that
  needs the container element walks and the virtual dispatches resolved.

Nothing here is Tier A or Tier B. There is no SMT proof and no differential test against the
retail reader. Getting to Tier B means calling `LoadGame::walk_function` on hbox against a
byte buffer and comparing.

---

## 8. What this buys

1. **The state schema is a build artifact now**, not a research task. `schema/state-schema.json`
   regenerates from the binary + PDB in two commands, and `--schema CLASS` prints any class's
   ordered serialised field list with names and types.
2. **`don-sim`'s SoA layout has a target.** For every class we implement, the union of its
   walked ranges is exactly the state that must be reproduced bit-for-bit to match retail's
   adler-32 per channel. Fields *outside* those ranges are free implementation choices.
3. **Save games are now usable as fixtures.** A `.svx` is a full world snapshot in the same
   grammar; seven of them are in hand, and the VM has more. A parsed save is a ready-made
   initial state for differential testing, far richer than anything we can construct.
4. **The PDB unblocks other lanes.** Damage, pathfinding, RNG and the rules loader all now
   have real field names for the offsets they have been calling `+0x54`.
5. **The tag hash is a name oracle.** Any one-byte section tag can be checked against a
   candidate name for free, and a live read of `int_str_array` would name all 87 sites.

---

## 9. Ledger entries for `docs/provenance-ledger.md`

| mechanic | source | tier | evidence |
|---|---|---|---|
| `SaveGame` stream = `walk_function` byte ranges in program order | `0x0043d730` | **C** [measured] | `fwrite(begin,1,end-begin,f)`; 13 specimens parse on the recovered layout |
| section tag byte = `String::generate_hash(name)` case-insensitive, low byte | `0x00a1b6b0` | **C** [measured] | `game`→`0x16`, `gameinfo`→`0x42`, `player`→`0x50` match real files; 7,575/7,621 declared hashes in `Data/internal_strings.xml` reproduced |
| `String::walk_data` = `u32` count + UTF-16LE, not NUL-terminated | `0x00a1b2d0` | **C** [measured] | every String in 13 files |
| `Array`/`SimpleArray`/`ObjectArray`/`PtrArray` header = `u32 len; [u32 size; u16 inc; u8 flags; elems]` | `0x00471c30` etc. | **C** [measured] | four independent implementations, identical shape |
| `.svx` = gzip( String magic, `u32 sGameSaveVersion`, `WalkDataGame::walk_data` ) | `0x005a8220` | **C** [measured] | 7 save files, 3 magics, 2 format versions |
| `GameInfo` layout, all 30 setting bytes + 8 player slots | `0x005d6570` + PDB | **C** [measured] | 55/55 predicates over 13 specimens; 6/6 vs `bindings.json` |
| `GameInfo::seed` = `gi+0x04` | PDB | **C** [measured] | promotes `replay-io.md` §7.3 from hypothesis |
| `Objects::valid` = `[[0x00c0618c]+0x1f4]` | PDB + `0x006541e0` | **C** [measured] | refutes the `checksum_deep` hypothesis in `checksum.md` §6 |
| `sizeof(Leader) == 0x6eec`, `sizeof(Player) == 0x8c` | PDB | **C** [measured] | matches two independently measured strides |
| `rise.pdb` matches this image | PE CodeView vs PDB stream 1 | structural [measured] | GUID `51D4F219-…-C3361B0D291F`, age 1, both sides |

Remove from "not yet derived": *"the closed `walk_data` traversal (first-order extraction
done, call-graph closure outstanding)"* — replace with *"the save-game body: container
element walks and per-receiver virtual dispatch inside `WalkDataGame::walk_data`"*.

---

## 10. Corpus

Seven `.svx` were pulled from the VM for this lane via the `certutil -encode` hop, one fresh
temp name per file, every one hash-verified against `certutil -hashfile … SHA256` in the
guest [measured]. They live in the lane scratchpad, **not** committed — `ron-data/` is
gitignored and these are game-derived user data.

| local name | source | bytes | plain |
|---|---|---|---|
| `ref2024.svx` | `Saves\Reference Midgame.SVX` | 373,892 | 2,999,927 |
| `friend2014.svx` | `Saves\friendgame.svx` | 657,101 | 4,089,643 |
| `friend21_2014.svx` | `Saves\friendgame21.svx` | 541,769 | 4,714,677 |
| `new2014a.svx` | `Saves\new save game 2014.08.04 …` | 1,217,478 | 6,978,886 |
| `new2014b.svx` | `Saves\new save game 2014.08.12 …` | 720,259 | 5,605,837 |
| `ctw2014.svx` | `Saves\autosaves\ctw - cmr\the entire world-autosave_ingame0.svx` | 332,668 | 2,744,866 |
| `ctwback2014.svx` | `…-autosave_back0.svx` | 108,473 | 108,473 (raw) |

Also pulled: `Data\internal_strings.xml` (467,764 B, sha256 `0e57a21f…15b6`), used only as an
independent check on the hash function.

The VM holds 11 more `.svx` and ~60 more `.rcx`. Extraction script:
`scratchpad/fetchsaves.py`.
