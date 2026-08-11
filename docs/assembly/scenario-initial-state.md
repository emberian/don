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

## 8. §5 steps 2–5, run (2026-08-11)

The pre-registered test passes.

```text
int_str_array[5958] = "./scenario/scriptlibrary/general_powers.bhs"
int_str_array[5959] = "editor_scratch_file.svx"
walked 8453 bytes -> 0x09922b90   (target 0x09922b90)
```

`crates/don-replay/tests/scenario_channel_initial.rs::the_derived_game_init_scenario_state_is_the_value_retail_carries`,
with the two strings bound positionally through `don_content::string_table::parse_string_table_xml`
and every other byte a named store in §2. There were no free parameters: `8,321` derived
fixed bytes plus `2 × (42 + 23)` UTF-16 payload bytes, one 32-bit comparison, no field
touched after the value was computed. The §4 residual prediction — "roughly 6,783 of byte
weight, on the order of 65 UTF-16 code units between them" — was 65 code units exactly.

It is installed as `SimBridge::populate_scenario_initial` and reaches the corpus scoreboard
through `WorldSim::from_replay`. The channel now walks 8,453 real bytes on **every** compare;
none of its agreement is empty-state.

### The deadline splits the corpus exactly in two, on AI players

Over the whole corpus, `scenario_data` goes from 0 matches to **24,245**, all of them
non-trivial, best survival **6,320** turns. The per-file split is not a spread — it is a
clean partition, and the discriminator is whether the game had computer players
(`initial.active_players` minus the play slots that issue commands):

| recording | AI | `scenario_data` survived | first divergence | `script_run_time` |
|---|---:|---:|---:|---|
| 2018.11.17 | 0 | **6,320** | turn 6,322 `0x151c27b9` | whole recording |
| 2018.12.01 | 0 | **6,059** | turn 6,061 `0xb2f1287d` | whole recording |
| 2019.03.24 | 0 | **4,922** | turn 4,924 `0x443427a2` | whole recording |
| 2020.02.21 | 0 | **4,210** | turn 4,212 `0x9a902885` | whole recording |
| 2020.02.08 | 0 | **1,572** | turn 1,574 `0x7a54287c` | whole recording |
| 2024.02.23 20:49 | 0 | **1,111** | *never* | whole recording |
| 2024.03.29 21:52 | 0 | **37** | *never* | whole recording |
| the other 14 | 2–5 | **1** | turn 3 | diverges turn 2 |

All seven zero-AI recordings are also exactly the seven on which `script_run_time` agrees
throughout, i.e. the seven that loaded **no BHS program**. All fourteen AI recordings load a
program and lose both channels immediately. The two divergences are the same event seen from
two channels: **an AI player's script runs, and `ScenarioFuncSet` builtins write
`ScenarioData`.** That also explains why the turn-3 values repeat across unrelated games
(`0x01d5286b` in six of them, `0x70b41e2b` and friends later) — same shipped scripts, same
first calls — and it means channel 14's next increment is BHS work, not scenario work.

### One derived hypothesis for turn 3, tested and rejected

`Setup::build_game` `0x005ac190` is the only writer of `ScenarioData::general_powers_script`
(`0xe8d41c`) — five of the eleven references to that global, against one read each in
`Game::do_frame` and the walker. Its instruction stream:

```text
0x005ad94d  push 0xe8d46c            ; general_powers_script_file
0x005ad952  mov  ecx, 0xeb6a90
0x005ad957  call Compiler::compile   ; 0x009bf160, (String const&, ScriptReloadType=1)
0x005ad95e  jne  0x005ad9b6          ; only on compile() == 0
0x005ad960  push 0xe8d46c
0x005ad965  mov  ecx, 0xe8d41c
0x005ad96a  call String::operator=   ; general_powers_script = general_powers_script_file
0x005ad972  mov  ecx, 0xe8d41c
0x005ad978  call String::get_file    ; 0x00a1d320, basename
0x005ad981  mov  ecx, 0xe8d41c
0x005ad987  call String::operator=   ; general_powers_script = basename
0x005ad99b  push 0x2e                ; L'.'
0x005ad99d  mov  ecx, 0xe8d41c
0x005ad9a2  call String::find_index_reverse   ; 0x00a16800
0x005ad9b1  call String::truncate    ; 0x00a1afb0, strip the extension
```

So retail's post-setup value is the basename with its extension stripped:
`general_powers`. That is a fully determined transition with no free parameter, which makes
it a legitimate single hypothesis rather than a fit. It is **wrong**:

| `general_powers_script` | walked | channel |
|---|---:|---|
| `""` (`ScenarioFuncSet::init`) | 8,453 | `0x09922b90` ✓ turn 2 |
| `general_powers` (derived above) | 8,481 | `0x1f9b317b` |
| `./scenario/scriptlibrary/general_powers` | 8,531 | `0x030a3b2d` |
| `./scenario/scriptlibrary/general_powers.bhs` | 8,539 | `0x747d3c9c` |
| `general_powers.bhs` | 8,489 | `0x42b632ea` |

None is `0x01d5286b`. Reading the target as a constraint: `s1` **falls** from 11,152 to
10,347, so the net byte weight *decreases* by 805, while any string assignment can only
raise it. Whatever happens at turn 3 lowers a `0xffffffff`-initialized field.

## 9. Residual analysis — what the two adler halves say the writer touched

This section identifies fields; it installs nothing. Nothing below is in the producer.

An adler-32 over a fixed-length buffer gives two independent sums, so a *structural*
hypothesis with one unknown value is solvable rather than searchable. Take the hypothesis
"one `int` field that `ScenarioFuncSet::init` left at `-1` now holds `V`, `0 ≤ V < 65536`,
and the walked length is unchanged". Then with `D = Δs1`, `N = 8453` and the field at offset
`p`, `d₂ = d₃ = −255` and

```text
d₀ + d₁ = D + 510            Σ i·dᵢ = (N − p)·D − Δs₂
```

is two equations in two unknowns — one `V` per offset, or none. Scanning only the 233 real
`ff ff ff ff` windows of the derived image and keeping solutions that land on a *named*
field:

| recording | first-divergence value | unique named solution |
|---|---|---|
| 2018.11.17 turn 6,322 | `0x151c27b9` | `last_razed[0] = 2077` |
| 2018.12.01 turn 6,061 | `0xb2f1287d` | `last_razed[2] = 2018` |
| 2019.03.24 turn 4,924 | `0x443427a2` | `last_razed[1] = 2054` |
| 2020.02.21 turn 4,212 | `0x9a902885` | `last_razed[0] = 2026` (also `find_counters[10] = 23701`) |
| 2020.02.08 turn 1,574 | `0x7a54287c` | `last_razed[0] = 2017` |
| AI turn 3, six recordings | `0x01d5286b` | `find_counters[24] = 2000` |
| AI turn 3, five other values | — | **no** single-field solution |

`last_razed` (`0xcc2190`) is written by exactly `Object::disband` `0x006455c0` and
`Object::take_damage` `0x00652020`, and read by `ScenarioFuncSet::get_last_razed_building`
`0x009f2980`. So §5 step 4's prediction — the channel expires when a building is razed — is
right for human-only games, and the five independent recordings land on the same field with
object-id-shaped values 2017–2077. `find_counters` is the scenario/BHS *find* mechanism
(`ScenarioGroup::find_counter` is walked beside it), which is exactly what an AI script
touches first.

Weigh this correctly. It is an inference from two 16-bit sums plus one structural
hypothesis, over a field set this document enumerates completely. Uniqueness within that
set across five independent recordings is strong evidence and **not** proof: the hypothesis
excludes multi-field changes by construction, and five of the AI values have no solution at
all, which proves at least those are multi-field. The next lane should confirm from the
writer — read `Object::disband`'s `last_razed` store and the `ScenarioFuncSet` find
builtins — and must not install a value chosen to make a checksum agree. That is why none of
these numbers is in `RetailInitialScenario`.

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
