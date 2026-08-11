# `scenario_data` channel 14: the state retail starts a game in

Lane: `replay-channels`, 2026-08-10. Every address and value below is **[measured]** on
this Mac against `ron-bin/riseofnations.exe` (sha256 `30478a44…625079`, image base
`0x00400000`) and `ron-bin/sbl/rise.pdb`. Disassembly is Capstone over the mapped image;
`re/decomp-all/` was used only for orientation. Nothing here is oracle-executed or
live-captured: the tier is **C — structure and constants read off the binary**, and the
divergence boundary is §5.

---

## 0. Why this channel, and what the corpus already says

`schema/replay-validation.json` records, for every checksum-bearing recording, the value
retail carried on the channel's first checksummed turn. For `scenario_data` that value is

```
0x09922b90   in 21 of 21 recordings
```

across five engine builds (`00.2014.*` through `00.2024.06.20`), six map styles, map
sizes 40–100, and every team layout in the corpus. Whatever the initial `ScenarioData` is,
it does **not** depend on the game setup — which is the property that makes it derivable
from the binary rather than from the recording. (`ScenarioData::log_data` confirms the same
field set; 2,458 distinct values occur across the corpus's 488,557 packets, so the channel
is mutable, not a constant.)

## 1. The traversal — `ScenarioData::walk_data` `0x00997ad0`

Already implemented in `crates/don-replay/src/scenario_channel.rs` by an earlier lane and
re-read here instruction by instruction; it is faithful. Order and extents:

| walked | preferred VA span | shape |
|---|---|---|
| `load_scenario_script`, `msg_time`, `game_msg_time`, `hilite_option`, `hilite_object`, `highlight_x`, `highlight_y`, `involved_who` | `0xcc21b0`, `0xcc21ec`, `0xcc2290`, `0xcc218c`, `0xcc2294`, `0xcc02fc`, `0xcc2188`, `0xcc228c` | 8 × i32 |
| `camera_init_x/y/zoom` interleaved per player | `0xcc22e0`, `0xcc0300`, `0xcc21c0`, stride 4, `iVar < 0x20` | 8 × 3 × i32 |
| `custom_time_limit` | `0xcc21b4` | i32 |
| `find_counters` | `0xcc2210 .. 0xcc228c` | 31 × i32 |
| `last_razed` | `0xcc2190 .. 0xcc21b0` | 8 × i32 |
| `city_lost_to` | `0xcc0320 .. 0xcc0330` | 8 × i16 |
| `units_killed` | `0xcc0330 .. 0xcc1930`, inner count `0x160` | 8 × 352 × i16 |
| `builds_destroyed` | `0xcc1970 .. 0xcc2180`, inner count `0x81` | 8 × 129 × i16 |
| `reinforcements_arrived` | `0xcc2180` | 8 × u8 |
| `war_blocked` | `0xcc22a0 .. 0xcc22e0` | 8 × 8 × u8 |
| `ally_mask`, `diplomacy_setting` | `0xcc21b8`, `0xcc21e0` | 8 × u8 each |
| 14 policy flags | `0xcb195a`, `0xcb195b`, `0xcb4ba9`, `0xcb4bab`, `0xcbe329`, `0xcb7df9`, `0xcb4baa`, `0xcbe32b`, `0xcbe5af`, `0xcb7dfb`, `0xcb7dfa`, `0xcbb0d9`, `0xcbb0da`, `0xcbb0db` | u8 each, in that order |
| 6 `String`s | `0xe885bc`, `0xe88afc`, `0xe8a74c`, `0xe8a77c`, `0xe8d41c`, `0xe8d46c` | `String::walk_data` `0x00a1b2d0` |
| 3 colours | `0xe8fe34`, `0xeaffbc`, `0xeb1abc` | 10 bytes each |
| timers, components, messages | `LinkList<String,int>` `0x004c8070`, `ObjectArray<ScenarioComponent>` `0x004c7060`, `LinkList<ScenarioMessage,int>` `0x004c7270` | |
| *(`user_warnings` skipped)* | `0x00997e5f: if (DataWalk[2] == 0)` | not on the `CheckSum` path |
| `extra_starting_locs`, `city_lost` | `Array<WCoordData>` `0x00478990`, `BitMask<8>` at `0xc8cb60` | |
| objectives ×8, `scenario_groups`, `involved_objects` | `0xed60c0` stride `0x1c`, `0x004c7db0`, `0x004c75e0` | |
| reveal ×8, attrition ×8, ignoring-orders ×8, `ignore_orders` | `0xed63f0`/`0xed64b0` stride `0x18`, `0xed6570` stride `0x1c`, `0xcc02f8` | |

`String::walk_data` on the checksum direction emits a zero-extended 32-bit length followed
by `length` UTF-16 code units, and nothing else. An empty container emits only its 4-byte
zero count — `ObjectArray::walk_data` `0x004c709b` branches out before touching capacity,
grow, or flags.

## 2. The initializer — `ScenarioFuncSet::init` `0x00a03c30`

`tools/pdb/callers.py` gives it exactly one caller, `Game::init`, and gives
`ScenarioFuncSet::close` `0x00a03650` exactly one, `Game::end_game_close`. So `init` is the
state at the start of a recorded game, and `close` is **not** interchangeable with it —
`close` writes zeroed cameras (`0x00a03ad8 mov [0xcc22fc], 0`) where `init` writes `-1`.

Every checksum-visible value `init` writes, from the instruction stream:

| field | value | evidence |
|---|---|---|
| `msg_time` | `200` | `0x00a03e48 mov dword [0xcc21ec], 0xc8` |
| `game_msg_time` | `12000` | `0x00a03e3e mov dword [0xcc2290], 0x2ee0` |
| `hilite_option`, `hilite_object`, `highlight_x`, `highlight_y` | `-1` | `0x00a03e52`, `0x00a03e5c`, `[0xcc02fc]`, `[0xcc2188]` |
| `involved_who` | `-1` | `0x00a04243 mov dword [0xcc228c], 0xffffffff` |
| `camera_init_x[i]`, `camera_init_y[i]` | `-1` | `0x00a03f18 …0x00a03fea`, eight unrolled pairs |
| `camera_init_zoom[i]` | `5` | `0x00a03f2c mov dword [0xcc21c0], 5`, eight unrolled |
| `find_counters[0..31]` | `-1` | `0x00a03e05 movaps xmm0, [0xb69d40]` (sixteen `0xff` bytes in `.rdata`), seven `movaps` + one `movq` + `0x00a03f0e mov dword [0xcc2288], 0xffffffff` = `0xcc2210..0xcc228c` |
| `last_razed[i]` | `-1` | `0x00a041ca mov dword [esi*4 + 0xcc2190], 0xffffffff` |
| `city_lost_to[i]` | `-1` | `0x00a04109 or eax, 0xffffffff` / `0x00a0410d mov word [esi*2 + 0xcc0320], ax` |
| `units_killed` | all zero | `0x00a0418f mov ecx, 0xb0` + `0x00a041a4 rep stosd` from `0xcc0330`, `0x00a041b0 add [ebp-0x18], 0x2c0` per player |
| `builds_destroyed` | all zero | `0x00a041a9 mov ecx, 0x40` + `rep stosd` from `0xcc1970`, `0x00a041c5 stosw`, `add [ebp-0x1c], 0x102` per player |
| `reinforcements_arrived[i]` | `0` | `0x00a041be mov byte [esi + 0xcc2180], 0` |
| `war_blocked` | all zero | `0x00a0419e movq [eax], xmm0` with `xmm0 = 0`, base `0xcc22a0` `+= 8` per player |
| `ally_mask[i]` | `0` | `0x00a04141 mov byte [esi + 0xcc21b8], 0` |
| `diplomacy_setting[i]` | `0` | `0x00a0413a mov byte [esi + 0xcc21e0], 0` |
| `plunder`, `building_unit_bonus`, `building_resource_bonus`, `buildings_gather`, `display_bubble_text` | `1` | `[0xcb195a]`, `[0xcb195b]`, `[0xcb4ba9]`, `[0xcbe329]`, `0x00a04008 mov byte [0xcbb0d9], 1` |
| `buildings_free`, `units_free`, `techs_free`, `speed_control_disabled`, `pause_disabled`, `mouse_selection_disabled`, `hotkey_selection_disabled`, `highlight_visible`, `highlight_active` | `0` | the corresponding `mov byte …, 0` in the same block |
| `msg_color` | `00 00 00 ff 00 00 00 00 02 00` | `0x00a03de2 movq xmm0, [0xc8d260]` → `[0xe8fe34]`; `0x00a03df8 mov eax, [0xc8d268]` → `[0xe8fe3c]`; only `0xe8fe34..0xe8fe3e` is walked |
| `game_msg_color` = `objective_color` | `ff ff ff ff ff 7f ff ff 02 00` | `movq xmm1, [0xc8d26c]` → `[0xeaffbc]`/`[0xeb1abc]`; `ecx = [0xc8d274]` → `[0xeaffc4]`/`[0xeb1ac4]` |
| `general_powers_script` | **empty** | `0x00a03e18 mov ecx, 0xe8d41c` + `0x00a03e1d push 0xeb437c` (`class String const EMPTY_STRING`) + `0x00a0400f call String::operator=` |
| `city_lost` | `bits = 8`, `size = 1`, payload `00` | `.data` image at `0xc8cb60` is `08 00 00 00 | 01 00 00 00`; `0x00a041fd memset(0xc8cb6c, 0, (bits+7)>>3)` |
| every container | empty | the `length = 0` stores and `LinkList::clear` calls throughout the per-player loop |
| `ignore_orders` | `0` | `0x00a04084 mov dword [0xcc02f8], 0` |

Two scalars `init` never writes, `load_scenario_script` (`0xcc21b0`) and
`custom_time_limit` (`0xcc21b4`), are BSS (the `.data` section's raw size ends at
`0x00caa000`, well below them) and are written back to `0` by `ScenarioFuncSet::close`
(`0x00a03aec`), so `0` holds both at the first `Game::init` and after every completed game.
`scenario_name`, `victory_message` and `defeat_message` are likewise untouched by `init`
and set to `EMPTY_STRING` by `close` (`0x00a03942`, `0x00a03976`).

All of the above is implemented as `scenario_channel::RetailInitialScenario` and pinned by
`the_retail_initial_state_is_the_derived_byte_image`.

## 3. What is still unsourced: two `internal_strings.xml` ordinals

`init` assigns two of the six checksummed `String`s from the runtime `StringTable`
`int_str_array` (`0x00c06378`) by fixed byte offset into its **20-byte `String` array**
(`docs/tracks/mod-story.md` §2.5: `StringTable::init` `0x00A28520` loads
`internal_strings.xml` at startup, 7,630 entries, identity is the ordinal, never reloaded):

```
0x00a04014  mov eax, [0xc06378]      ; int_str_array
0x00a04019  mov eax, [eax + 0x10]
0x00a0401c  add eax, 0x1d178         ; 0x1d178 / 0x14 = ordinal 5958
0x00a04021  push eax
0x00a04022  mov ecx, 0xe8d46c        ; ScenarioData::general_powers_script_file
0x00a04027  call String::operator=

0x00a04231  mov eax, [eax + 0x10]
0x00a04234  add eax, 0x1d18c         ; 0x1d18c / 0x14 = ordinal 5959
0x00a04239  push eax
0x00a0423a  call String::operator=   ; ecx = 0xe88afc, ScenarioData::temp_save
```

`ron-data/` does not contain `internal_strings.xml`. **That single missing shipped-data
file is the whole remaining gap for channel 14.**

## 4. The measurement that isolates it

With both strings empty and every other field as derived above, the traversal walks
**8,321 bytes** and produces **`0xba9c1111`**, not `0x09922b90`. Both are pinned by
`the_two_unsourced_internal_strings_are_what_still_blocks_the_channel`.

That is a real result rather than a failure: it says the two internal strings are
non-empty. Reading it as a constraint, retail's `s1 = 0x2b90` implies a total walked byte
sum of `11151 (mod 65521)` against the derived `4368`, so the two strings carry roughly
6,783 of byte weight — on the order of 65 UTF-16 code units between them, i.e. two
path-like strings rather than short tokens.

**No attempt was made to guess them.** Two free strings against one 32-bit target is not a
confirmation, it is curve-fitting, and a hit would be indistinguishable from a coincidence
over any candidate space large enough to contain the answer.

## 5. To close it

1. Extract `internal_strings.xml` from the owned install into `ron-data/` using the
   `certutil` hop in `docs/binary-ground-truth.md` (the Parallels VM was **paused** when
   this lane ran and was deliberately not resumed — a live retail session may be in
   flight). The binder is positional: `<STRING>` elements in document order, `hash` and
   `needed` ignored by retail.
2. Feed ordinals 5958 and 5959 to `scenario_channel::ScenarioInitialStrings`.
3. Compute `scenario_checksum(RetailInitialScenario::new().state(strings))` and compare
   against `CORPUS_INITIAL_SCENARIO_CHANNEL = 0x09922b90`. With every other byte already
   fixed, that is a single pre-registered 32-bit test.
4. If it matches, install it through a `SimBridge::populate_scenario_initial`, and expect
   the channel to hold until the first counter it tracks moves — `units_killed` and
   `builds_destroyed` are written by `Object::take_damage` `0x00652020`,
   `Object::disband` `0x006455c0` and `City::capture` `0x00736c40`, so the deadline is the
   recording's first kill, not its first frame.
5. If it does not match, the residual is a *localised* discrepancy in a fully enumerated
   field set — report it rather than tuning fields until it agrees.

## 6. Ledger entry

| mechanic | source | tier | evidence |
|---|---|---|---|
| Retail's initial `ScenarioData` is game-setup-independent | corpus | **C [measured]** | channel 14 = `0x09922b90` on the first checksummed turn of 21/21 recordings, five engine builds, six map styles, all team layouts |
| Complete checksum-visible initial `ScenarioData`, minus two shipped-data strings | `ScenarioFuncSet::init` `0x00a03c30`, sole caller `Game::init` | **C [measured]** | every field in §2 read off the instruction stream; derived state walks 8,321 bytes; two `int_str_array` ordinals (5958, 5959) unavailable locally |

## 7. The two strings, resolved (2026-08-10)

`internal_strings.xml` was not in `ron-data/`. It is now, extracted from the supported
install and byte-verified against the guest:

| file | size | SHA-256 |
|---|---:|---|
| `internal_strings.xml` | 467,764 | `0e57a21f2458141a31acd507c8f0c6e3c42b61da634179939b7c9dedf08215b6` |
| `tilesets.xml` | 124,334 | `60f0d076863772d03c107eb1aaa0ed123482e22e679fa8c3505d0273988eba26` |

Extraction protocol: base64 the file in the guest with `[Convert]::ToBase64String`, read it
back through `prlctl exec ... type`, decode locally, and require the SHA-256 to equal the
guest's `Get-FileHash` before installing. Both files are gitignored proprietary shipped
data — the protocol is committed, the bytes are not.

**Parse it as XML, not with a line regex.** A `<STRING hash="…">` regex finds 7,622
elements; `ElementTree` finds 7,630, matching the raw `<STRING` tag count. The eight it
drops all precede ordinal 5950, so a regex-derived index is off by exactly eight there —
enough to silently select the wrong string.

With a real parse, §5 step 1 is done and the positional binder predicted above holds:

| ordinal | value | field |
|---:|---|---|
| 5958 | `./scenario/scriptlibrary/general_powers.bhs` | `general_powers_script_file` |
| 5959 | `editor_scratch_file.svx` | `temp_save` |

This is a derivation, not a fit. The two ordinals came from `ScenarioFuncSet::init`'s
instruction stream with no reference to this file, and the values standing at them match
the field semantics independently. Guessing by name would have failed: the file also
contains `.\conquest\temp\ctw_replay_temp_save.SVX` at ordinal 2839, a better-looking
"temp save" that is not the one the code reads.

§5 steps 2–5 remain open. The single pre-registered 32-bit test against
`CORPUS_INITIAL_SCENARIO_CHANNEL = 0x09922b90` has not been run, so nothing here claims the
channel agrees — only that its last missing input is now local.

### `tilesets.xml` moved the generator a stage, and moved no channel

Two separate things, and an earlier revision of this section conflated them.

It **did** advance the generator. Counting the per-file `initial.items.boundary` across the
corpus, before and after installing the file:

```text
before   21 terrain_groups_fill_fertile   6 map_team_continent_partition   1 east_indies …
after    21 terrain_groups_place_all      6 map_team_continent_partition   1 east_indies …
```

All 21 checksum-bearing recordings advance a whole stage, `TerrainGroups::fill_fertile`
`0x006a6f90` → `TerrainGroups::place_all` `0x006a70d0`, and the 28 `No such file` reads
disappear. The fertility stage now actually runs rather than aborting.

It moved **no channel**: `world` stays at 0 matches over 222,938 non-trivial compares,
because `place_all` is itself unported. So the file was a real prerequisite, not merely a
mask — but a channel needs the next function, not the next file.

(Counting boundary *substrings* in the record rather than the `boundary` field says nothing
moved, because the name of the reached stage and the name of the next one both appear. Read the
field.)
