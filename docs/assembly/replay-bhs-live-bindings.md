# Replay BHS production call and first live bindings

## Result

The stock `economic.bhs` Program image can now be entered through the exact
four-argument `Leader::production_ai` boundary. The replay adapter owns eleven
ScenarioFuncSet handlers on the smallest reached subdomain. Installed `rules.xml`
owns `get_mapstyle`; replay setup owns the reached conquest and starting-option gates.
Execution now stops strictly at dynamic builtin 377, `find_city_id`.

No replay checksum match is claimed. The adapter is not registered in the default
harness, and a failed prefix atomically rolls back both BHS static-variable writes and
the external `ref step` cell.

## The four live arguments

`Leader::production_ai` `0x006c1960` constructs four `ScriptInt` cells and passes them
in this logical order:

| argument | retail source | ownership |
|---|---|---|
| `who` | `LeaderData+0x008 + 1` | replay `InitialPlayer::who + 1` |
| `ref step` | `LeaderData+0x790` | persistent Leader owner; not in the replay setup prefix |
| `boom_vs_rush` | `Personality::rush` at `LeaderData+0x6dd4`, plus 2 | persistent Leader owner; not in the prefix |
| `num_loops` | literal 5 at `0x006c1a14` | exact constant |

The VM already had the required pointer semantics in `Vm::run_script_index_mut`:
the compiler's `OP_INIT` aliases `step`, while `OP_INIT_COPY` copies the other three.
The replay adapter therefore does not add a second ref-parameter mechanism.

Across the 21 checksum-bearing recordings, 14 contain AI players and supply 43
one-based `who` identities:

| `who` | AI player instances |
|---:|---:|
| 2 | 5 |
| 3 | 2 |
| 4 | 11 |
| 5 | 10 |
| 6 | 8 |
| 7 | 1 |
| 8 | 6 |

That measurement makes **zero** retained-step or personality claims. Those two live
fields must come from reconstructed Leader state.

## Strict first-invocation trace

With an explicit live image stating `num_cities(who) >= 1`, no prior attack and no
prior raid, a fresh `economic` Program first reaches:

1. `258 num_cities(who)`
2. `383 find_city_with_num(who, 1)`
3. `713 was_city_attacked(who, "", -1)`
4. `712 was_city_raided(who, "", -1)`
5. `323 find_nation(who)`
6. `358 get_techs_per_age(who)`
7. `248 age(who)`
8. `258 num_cities(who)`
9. `258 num_cities(who)`
10. `81 get_mapstyle()`

`get_techs_per_age` is first-invocation-only: `needed_techs` is one function static
guarded by `OP_JUMP_IF_INITED`, shared by every player invoking `economic`. The initial
city-count comparison short-circuits `num_type_with_queued` when a city exists; a false
attack predicate forces evaluation of the raid predicate.

For the measured Mediterranean path, the map-style expression invokes builtin 81
eight times before its final comparison succeeds. It then reads city ordinals 2 and 3,
followed by:

1. `147 is_conquest_scenario()`
2. `255 get_starting_resources(who)`
3. `258 num_cities(who)`
4. `254 get_starting_town_size(who)`
5. `383 find_city_with_num(who, 1)`
6. `383 find_city_with_num(who, 2)`
7. `383 find_city_with_num(who, 3)`
8. `377 find_city_id(capital_name)` — the next unsupported boundary

That path uses replay settings `starting_resources=1`, `starting_town=2`, no conquest
or scenario semaphore bit, one live city, and a sea map. It changes `step` from 1 to 6,
then enters `train_unit_with_need`. The failed run proves the external ref cell and the
candidate Program both roll back after the missing dynamic call.

## Installed and replay setup bindings

Builtin 81 at `0x009e4cc0` indexes `Rules::map_styles[GameInfo::map_style]` at a
0x58-byte stride and returns the entry name at +0x14. The adapter accepts that name only
from `MapStyleStaticData`, after its complete ordered 23-entry `rules.xml` category and
the replay selector agree with the shipped catalog.

The reached setup reads remain distinct from live city state:

- builtin 147 reads Game semaphore bit 17, matching the canonical decode already owned
  by `don_bhs::scenario`;
- builtin 254 at `0x009e9170` reads `starting_town`, except live leader flags2 bit
  `0x80` forces 0 and semaphore bit 12 or 17 maps the result to live city presence;
- builtin 255 at `0x009e91f0` reads `starting_resources`, except game-rules mode 8 and
  a live minor-power result select `starting_resources2`.

The complete 32-byte semaphore and all four option bytes bind from `InitialState`.
`Leader::is_major_power` is still an explicit optional live fact; mode 8 fails closed
if execution needs it and the caller has not reconstructed it.

The admitted attack/raid subdomain is deliberately only the shipped call shape
`(who, "", -1)`: scan all active City rows and answer whether the selected timestamp is
nonzero. Named-city and positive-timeout forms remain red.

## Cursor correction

`find_city_with_num` `0x009eff00` has no persistent cursor. It validates `who - 1`,
bounds-checks `city_num - 1` against `LeaderData::city_num`, directly indexes
`Cities::lists[who0]`, checks the City active bit, and returns `CityData+0x90` or the
global empty string.

The process-global cursor at `0x00cc2214` belongs to builtin 311, `find_unit`
`0x009ebe10`. The earlier replay-runtime note attached that cursor to builtin 383 and
has been corrected.

## Exact next boundary

Builtin 377 needs the global live object registry and the exact case-insensitive City
name lookup/ID result. Do not infer it from the ordinal City row alone. The Program must
ultimately share one owner with the step-4 game/general-powers runtime; duplicating the
Program would split its checksummed static state.

## Validation

The focused local suite passes 7/7 against the installed BHS, ordered map-style catalog,
and all 21 checksum-bearing recordings. Persvati overlay job
`replay-bhs-live-bindings-v2-20260811T171116Z-76750-31116-7245c2afc2de` passes the four
content-independent adapter tests; the three installed-content/corpus tests were
explicitly filtered rather than reported as remote passes.

Fidelity remains Tier C: instruction-level static recovery plus corpus shape. No live
retail execution or VM attachment was used in this tranche.
