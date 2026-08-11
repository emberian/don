# Replay-selected BHS runtime image

## Result

`crates/don-replay/src/replay_bhs_runtime.rs` now reconstructs the global shipped BHS
program registry for the stock replay shape without copying a checksum or inventing a
builtin result.

The replay prefix does not serialize `LeaderData+0x6ea4`, the production-script string.
It does serialize every player and the `LeaderData::leader_flags & 4` HUMAN gate.  The
selection boundary is therefore deliberately narrow:

| replay setup | admitted registry |
|---|---|
| stock (`scenario_type=0`, `script_type=0`, `mods=0`), no active non-human player | empty `ScriptFile::script_files` |
| same stock setup, one or more active non-human players | `ai/scripts/economic.bhs`, its `aibestbuildlibrary.bhs` include, then `scenario/scriptlibrary/general_powers.bhs` |
| anything else | `UnsupportedSetup`; no guessed scenario/mod script |

The `economic` choice is not inferred from the recorded checksum.  A live ordinary
skirmish read found the eight-character value `economic` at `LeaderData+0x6ea4`, and
`Leaders::init_production_script` `0x006ed490` compiles that field once for every active
AI.  `Setup::build_game` `0x005ac190` compiles general powers afterwards.  The observed
retail global registry has count 3, matching those two compilation units and economic's
one include.  See `docs/tracks/ron-ai.md` and `docs/mechanics/bhs-script-load-path.md`.

## Program merge and bindings

Both source-compiler results carry a channel-15 `ProgramWalkMeta`.  Arbitrary programs
cannot safely append those sidecars because linked-file indices are global.  The replay
adapter accepts only the two fresh `load_script` results, verifies that neither changed
the default process-global type-name registry, shifts every non-negative index in the
later sidecar by the first program's file count, appends files and metadata, and lets
`Program::set_walk_meta` rebuild the live resolved-link table.

The resulting bindings are intentionally different:

- `economic` is file 0 and is a **four-argument per-leader production-AI entry**.  It is
  not installed as `Game::do_frame`'s zero-argument game script.
- `general_powers` is file 2 and is the zero-argument second step-4 entry.

`ReplayBhsProgram::into_script_runtime` therefore transfers the entire global registry
to `don_sim::ScriptRuntime`, binds only general powers, and returns the economic binding
beside it as production-stage provenance until the four live AI arguments have an exact
owner. The loader also verifies the recovered entries have arity four and zero,
respectively.

## Canonical builtin collapse

`don-sim/src/script_runtime.rs` no longer owns a second timer implementation.  Its
`ScriptRuntime` stores `don_bhs::scenario::ScriptTimers`, and the overlapping builtin
indices 77 (`set_timer`), 78 (`stop_timer`), 79 (`timer_expired`) and 298 (`time_sec`)
dispatch through `don_bhs::scenario::call_scenario`.

The dispatch is intentionally limited to those four indices.  In particular, victory
indices 96 through 105 remain on `Sim`: routing every canonical index before exposing
the corresponding `GameInfo` host fields would replace working simulation reads with
`HostError::Unimplemented`.

## Corpus measurement

The 61-file corpus contains 21 checksum-bearing recordings.  The boundary test opens the
exact 21 identified by independently recorded `checksum_packets > 0`; it never reads an
expected checksum while selecting or compiling a script.

| cohort | recordings | prior producer | loaded image | first-turn matches |
|---|---:|---|---|---:|
| no AI | 7 | 0 files, 4 bytes, `0x00040001` | unchanged | 7 |
| AI | 14 | falsely empty | 3 files, 42,177 bytes, `0x6a25ab2b` | 0 |

The fourteen recorded first-turn targets are:

| target | recordings |
|---|---:|
| `0x6a91bf5d` | 7 |
| `0x9f0fbf5d` | 4 |
| `0xabbd5707` | 2 |
| `0x05857aba` | 1 |

This is a real checksum movement, not yet a compatibility match.  The default harness has
not been switched to the loaded runtime in this tranche, so the headline validation JSON
still reports 60,342 aggregate channel-15 matches and the net scoreboard delta is **zero**.
Installing the compile-only image immediately would also produce zero matches on the AI
cohort; it would merely replace a known-false empty image with the correctly selected
source state.  Runtime statics and compiler byte identity remain measured discrepancies.

Channel 14 also remains unchanged: the derived 8,453-byte `ScenarioData::init` image agrees
at turn 2 and AI recordings diverge at turn 3.  Loading a program does not mutate scenario
state, so the adapter does not pretend otherwise.

## Exact next boundary

The first production entry is not a zero-argument tick script.  Its live call has four
arguments (`who`, retained `step`, and two production-policy values) and immediately needs
world-backed ScenarioFuncSet facts.  The first useful cohort is:

1. `num_cities` (258),
2. `find_city_with_num` (383), a direct ordinal read from the owner's City pointer array,
3. `was_city_attacked` (713) and `was_city_raided` (712),
4. `find_nation` (323),
5. `get_techs_per_age` (358), then the already owned `age` (248).

The replay harness currently owns `don_sim::World`, not the full `tick::Sim` leader/city/
tech graph, and it does not own the four production arguments.  Each missing read must stay
`HostError::Unimplemented`.  Supplying a plausible city count, city name, nation or tech
value would move both channels but would fabricate the very state the replay oracle is
supposed to test.

## Fidelity

Tier C.  Load order and handler bodies are static-analysis results; the live `economic`
selection and three-file count are measured retail state.  The compiler-generated program
image has not been byte-compared against retail's compiler, and no claim in this adapter is
Tier B.
