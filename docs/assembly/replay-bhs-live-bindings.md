# Replay BHS production call and first live bindings

## Result

The stock `economic.bhs` Program image can now be entered through the exact
four-argument `Leader::production_ai` boundary. The replay adapter owns twenty
ScenarioFuncSet handlers on the smallest reached subdomain. Installed `rules.xml`
owns `get_mapstyle`; replay setup owns the reached conquest and starting-option gates.
Execution now owns dynamic builtin 377, `find_city_id`, plus the exact joined
type-counter cohort 259--261, the canonical population reads 245--246, and the reached
Tech/Other branch of 362, `have_tech`, and the canonical timer pair 78--79. It stops
strictly at builtin 357, `research_tech_with_cost`.

No replay checksum match is claimed. The adapter is not registered in the default
harness, and a failed prefix atomically rolls back BHS static-variable writes, the
external `ref step` cell, and the unique script timer owner including its private cursor.

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
8. `377 find_city_id(capital_name)`

The helper immediately repeats builtin 377 for the second- and third-city names even
when those ordinal reads returned empty strings. The measured bytecode then calls builtin
261, `num_type_with_queued(who, "Citizen")`, twice before that helper exits. The main
script next calls builtin 259, `num_type(who, "Market")`, and reaches builtin 245,
`population(who)`. The now-owned continuation is:

1. `245 population(who)`
2. `259 num_type(who, "University")`
3. `259 num_type(who, "Dock")`
4. `362 have_tech(who, "The Art of War")`
5. `259 num_type(who, "Tower")`
6. `323 find_nation(who)`
7. `78 stop_timer(who)`
8. `79 timer_expired(who)`
9. `248 age(who)`
10. `362 have_tech(who, "Written Word")`
11. `357 research_tech_with_cost(who, "Written Word")` — the next honest unsupported boundary

The BHS source supplies integer `who` to both timer calls even though their native
declarations take a String. Retail inserts `OP_CAST String`; `ScriptInt::get_string`
formats signed base-10 text, so the reached value is `"1"`. DoN's compiler does not yet
insert builtin-argument casts, so the canonical timer handler performs the equivalent
`Value::as_string` conversion at its boundary while the trace honestly retains `Int(1)`.

That path uses replay settings `starting_resources=1`, `starting_town=2`, no conquest
or scenario semaphore bit, one live city, and a sea map. It changes `step` from 1 to 6,
then enters `train_unit_with_need`. The failed run proves the external ref cell and the
candidate Program, and the removed `"1"` timer plus its cursor all roll back after the
later missing `research_tech_with_cost` call.

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

## City ID owner

The shipped PDB identifies `0x009ef580` as
`ScenarioFuncSet::find_city_id(String const&)`. Capstone over the pinned PE establishes a
single stack argument and `ret 4`, then the complete ordered scan:

1. all eight Leader slots, accepting `leader_flags & 1` without requiring the process bit;
2. every entry in that owner's `Cities::lists[who0]` pointer array;
3. only City rows whose `city_flags & 1` is set;
4. `CityData::id` at +0xa4 first, then `CityData::name` at +0x90, both through
   `String::ignore(..., -1)` and therefore `_wcsicmp` after the insensitive-hash prefilter;
5. sign-extended `CityData::o` at +0x08 on the first match, otherwise -1.

The PDB independently gives `CityData::o` as a signed short and the two complete String
layouts at those offsets. An empty query can therefore match an active City with an empty
`id`; this is pinned rather than silently special-cased. The adapter admits ASCII City
identifiers, which covers the reached installed names, and fails closed for Unicode/locale
comparison instead of claiming to emulate Windows `_wcsicmp`.

## Joined type-counter owner

The shipped PDB identifies builtins 259--261 at `0x009e9320`, `0x009e94a0`, and
`0x009e9630`. Capstone and the PE bodies establish one semantic cohort:

1. generate hashes for a nonempty query and scan all 806 `Types::list` rows in ascending
   order, selecting the first case-insensitive internal-name match;
2. validate one-based `who` and require both low Leader flags;
3. for 260 and 261 only, call `LeaderData::current_upgrade(source)` and then
   `LeaderData::get_graft(current)`;
4. classify the final Type virtually and read the active `u16` Unit or Build counter, or
   the decoded `LeaderDataEncrypt` Good value;
5. for 261 only, add `num_queued[final_type]` to Unit and Build results. Good values do not
   receive a queued addition.

The replay binder consumes the canonical `TypeBuiltinState::types` rows, the complete
8-by-806 current-upgrade/graft projection already owned by `BhsCreateUnitRuntime`, and the
exact `victory_score::LeaderState` `num_units`, `num_buildings`, and `num_queued` arrays.
It also consumes that Sim owner's six decoded primary-resource buckets. Good indices 6--49
remain fail-closed because the canonical Sim does not model the adjacent encrypted fields;
they are not silently zero-filled.

One retail layout edge is pinned explicitly. `is_unit_type` accepts indices 50..413, but
`num_units[352]` covers only 50..401. The raw address calculation for 402..413 therefore
aliases onto `num_queued[0..11]`; builtin 261 adds the ordinary queue cell for the final
type after that aliased active read. The focused test exercises this, first-match duplicate
name selection, a two-hop current-upgrade/graft resolution, Unit and Build queues, a primary
Good, and a resolved non-counter Type. This is a full-table adapter, not a `Citizen` stub.

Builtin 362, `have_tech`, uses the same ordered internal-name lookup and Leader validity
gate, but its body is not generally a raw tech-mask read. `LeaderData::has_tech` returns
true directly for Good indices below 50, calls `tribe_can_type` for Unit and Build rows,
and reads the Leader tech bit for ordinary Tech/Other rows. The reached Art-of-War row is
index 572 and therefore belongs to that last exact branch. The adapter admits only the
proven Good and Tech/Other domains; Unit and Build queries fail closed until the tribe
owner is joined.

## Population authority

The shipped PDB identifies builtin 245 at `0x009e8e70` and `LeaderData::control` at
`+0x940`. Capstone over the pinned PE establishes a 55-byte leaf body: wrapping-decrement
the one-based `who`, unsigned-check 0..7, require Leader flag bits 0 and 1 independently,
then return the signed dword at `Leader+0x940`. It does not read the queue, effective
population, `pop` at `+0x95c`, or population cap, and it performs no clamp or arithmetic.
Builtin 246 applies the same gate and directly reads the signed dword at `+0x7e4`.

The replay binder does not copy `production_runtime.leaders[].control`. That field is a
queue-completion sidecar mutated by allocation/destruction and is not synchronized with
the instruction-derived owner, `step8.leaders[].ai.control`. Instead it consumes the
existing `RuntimeLeadersFrontier` receipt, which already rejects disagreement between
the victory and step-8 copies of flags and population cap and projects the production-AI
control byte range. The resulting call-scoped image reads `+0`, `+0x7e4`, and `+0x940`
from one reconciled row and introduces no mutable population counter.

Builtin 247, `set_population_cap`, remains red. It must transactionally update the
ScenarioData override, misery policy, effective cap, and all canonical mirrors before a
replay host can safely own it. Likewise no full support-count builtin is claimed: the
runtime frontier owns decoded expense/support cells, but the active/queued family join
does not yet have one complete current-owner provenance.

## Canonical timer transaction

The shipped PDB identifies builtin 78 at `0x009e4c10` as the 104-byte
`ScenarioFuncSet::stop_timer(String const&)`. The handler copies the input into the
process scratch String, calls the timer list's case-insensitive remove operation, and
returns 1 on removal or -1 on a miss. There is no empty-name or player-range gate. The
reached `Int(1)` is therefore the timer key `"1"`, not player slot zero or an invented
integer-keyed timer.

The complete timer container already lives in `don_bhs::scenario::ScriptTimers`: ordered
nodes, absolute expiry values, overflow count, and the persistent current-node cursor.
Its canonical execution owner is the private `ScriptRuntime::timers` used by step-4 game
and general-powers scripts. The replay call no longer accepts a detached `ScriptTimers`
or a mutable Program. Instead a narrow `ScriptRuntime` bridge clones its private Program
and timers together, intercepts only builtins 78--79 through the shared ScenarioFuncSet
implementation, validates the VM outcome and ref-argument cells, and commits both owners
only after every check succeeds. Callers can observe a read-only timer receipt but receive
no timer or Program mutation handle.

Builtin 79 is the 37-byte `ScenarioFuncSet::timer_expired(String const&)` at
`0x009e4c80`. Its wrapper copies the input to the process scratch String, then
unconditionally reads signed `Game+0x560` before calling `ScriptTimers::check`; even an
empty list or missing name cannot bypass the clock owner. The bridge therefore accepts
only an opaque per-call clock receipt joining `Sim.world.seconds` to its
`vic_match.tick` mirror. Missing owners, disagreement, and negative/pre-epoch values are
typed refusals; the negative restriction is a conservative admitted-domain policy because
retail itself performs a raw signed comparison. Frame number and a default zero are not
substitutes.

`check` has three observable results and two mutation shapes:

- missing returns -1 and leaves the existing cursor unchanged;
- pending (`now < expiry`, signed) returns 0, retains the node, and moves the cursor to it;
- due (`now >= expiry`) returns 1, consumes the node, and advances the cursor to its successor.

Focused owner tests pin all three results, pending cursor movement, and due consumption.
The strict fixture deliberately admits one timer named `"1"`. Builtin 78 removes it in
the candidate and reports 1; builtin 79 still reads the admitted `(0, 0)` clock, then
misses and reports -1. `age(1)` is zero, and the synthetic type table name-misses
`"Written Word"`, so its exact #362 result is -1 and unary `!` enters the research arm.
Builtin 357 is the next unsupported boundary. That rejected downstream call restores the
timer node and cursor, initialized Program statics, and external ref step. Unit/Build
`have_tech` and full support count remain red.

## Validation

The eleven focused replay tests pass over the local installed content, including the
`economic.bhs` trace, ordered map-style catalog, and 21-recording census. All five focused
canonical timer tests pass in Persvati job
`replay-bhs-timer79-sim-full-20260811T194100Z-37465-30999-9abe48a50e9a`, covering clock
refusal, missing/pending/due, cursor movement, successful owner persistence, and downstream
rollback. Persvati replay overlay job
`replay-bhs-timer79-v1-20260811T193955Z-33776-10114-b1251b875d71`
establishes the seven content-independent tests; the installed map-style, installed BHS,
replay-corpus census, and replay-derived population-owner tests print explicit
**SKIPPED — NOT A PASS** notices because those content assets are absent. The required
ignored `final-balance-runtime.bin` compile-time asset was supplied through the SHA-pinned
remote asset allowlist.

Fidelity remains Tier C: instruction-level static recovery plus corpus shape. No live
retail execution or VM attachment was used in this tranche.
