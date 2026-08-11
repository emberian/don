# Replay-driven validation: the per-turn checksum harness

Lane: `replay-validate`. Every number here is **[measured]** — produced by
`tools/replay-validate.sh` against the 61 recordings in `ron-data/replays/`,
and reproducible with one command. Nothing here is verified in the
proof-assistant sense; see `docs/CHARTER.md`.

---

## Headline — what now works

**The loop exists and runs.** A real `.rcx` lockstep command stream is stepped
turn by turn, our fifteen `DataWalk` checksum channels are computed over our
state with a traversal *generated* from `schema/state-schema.json` — except
`scenario_data` and `script_run_time`, whose walkers the extractor cannot reach and which
are hand-derived from the instruction stream — and each is
compared against the `CheckSumsCommand` the retail client recorded on that turn.
The first diverging turn and the diverging channel are reported per channel, per
file, and rolled up into `schema/replay-validation.json`.

Over the whole corpus — 61 files, 21 with checksums, **585,152 turns**,
**488,557 checksum packets**, 30 seconds wall clock:

| channel | best survival (turns) | matches / compares | trivial matches | unmodelled | non-trivial compares |
|---|---:|---:|---:|---:|---:|
| `walls` | **25,442** | 222,938 / 222,938 | 222,938 | 222,938 | 0 |
| `deaths` | **5,734** | 82,943 / 222,938 | 82,943 | 82,943 | 0 |
| `ammo` | **3,696** | 106,634 / 222,938 | 106,634 | 106,634 | 0 |
| `items` | 0 | 107,882 / 222,938 | 107,882 | 107,882 | 0 |
| **`rules`** | **25,442** | **222,938 / 222,938** | **0** | **0** | **222,938** |
| **`script_run_time`** | **18,069** | **60,342 / 222,938** | **0** | **0** | **222,938** |
| **`world`** | 0 | 0 / 222,938 | 0 | 0 | **222,938** |
| every other channel | 0 | 0 / 222,938 | 0 | 0 | 0 |

`survived` = consecutive agreeing turns from the recording's first checksummed
turn. **The honest headline is still 25,442 substantive turns on `rules`.** The
replay's static Rules SaveGame section is independently parsed and projected through the
retail checksum traversal; all 222,938 comparisons walk 997,846 bytes and agree. The
scoreboard now has 668,814 comparisons in which our walker touched a byte: `rules` agrees
throughout, `script_run_time` agrees on seven recordings and diverges on fourteen, and every
`world` comparison walks real prefix-derived state and exposes the first dynamic divergence.

The previous 25,442-turn `walls` headline remains a weak empty-state result: `walls` was
empty in every recorded game, and adler-32 over nothing is 1 on both sides. `trivial` counts
exactly that case.

`ammo` and `deaths` still measure absent producers, not mechanics: they read 1
until retail creates the first projectile or corpse. Their measured deadlines
remain useful, but those matches are explicitly labelled `unmodelled`.

`units`, `builds`, `leaders`, `cities`, `goods`, `scenario_data`, `groups`, `guys` diverge
on the **first** checksummed turn, because those are non-empty from game start and we hold
none of them. `world` also diverges on the first checksummed turn, for a better
reason: it now hashes 280,968–780,168 bytes per comparison (map-size dependent)
through the exact `World::walk_data` traversal. Only 52–76 of those bytes are
currently sourced from the prefix; generated terrain, resources, start arrays,
fog and collision remain explicitly unsourced.

### `script_run_time` now has a producer — and it is four bytes wide

Channel 15's walker is `RunTimeEnv::walk_data` `0x009c41a0`. Read off the instruction
stream, it emits a `walk_tag` (a no-op for `CheckSum`), calls `RunTimeEnv::close`
`0x009c40a0` (which frees transient interpreter state and hashes nothing), then
**unconditionally** walks the signed 32-bit element count of the global
`ScriptFile::script_files` array at `0x00c8cba0`, then one `ScriptFile::walk_data`
`0x009c63b0` per entry. So a runtime with no loaded script file is
`adler32(1, [0,0,0,0]) = 0x00040001` — **not** the adler-of-nothing `1`.

`SimBridge::populate` now installs exactly that. Our `don_sim::World` owns no `ScriptFile`
registry, so its count is zero; `SimBridge::populate_script_runtime` replaces the value
whenever an authoritative `don-sim` `ScriptRuntime` with a complete retail walk sidecar
exists.

The result, per recording:

- **7 of 21** recordings carry `0x00040001` on every checksummed turn and never diverge:
  18,069 / 12,728 / 9,789 / 9,511 / 9,097 / 1,111 / 37 turns, 60,342 agreements.
- **14 of 21** diverge on the **first** checksummed turn (turn 2), with expected values
  `0x6a91bf5d` (×7), `0x9f0fbf5d` (×4), `0xabbd5707` (×2) and `0x05857aba` (×1) against our
  `0x00040001`. Those games loaded script files and we hold none of their program state.

**State this at its real size.** The channel is not `trivial` and not `unmodelled` —
retail's own value is never `1` here, so `retail_empty_compares` is 0 and every one of the
222,938 compares walked a byte. But the byte content is a container header: what agrees is
the *shape* of the `RunTimeEnv` walk plus the claim that those seven recordings loaded no
BHS program. It is evidence about the traversal and about those recordings; it is **no**
evidence about BHS program semantics, and the fourteen divergences are the honest measure of
how much program state is still missing. `agreements_are_unmodelled_except_the_empty_script_file_count`
in `tests/corpus.rs` fails the moment any other channel starts agreeing without a producer.

### `scenario_data` is derived except for two shipped-data strings

Channel 14 reads `0x09922b90` on the first checksummed turn of **21 of 21** recordings —
five engine builds, six map styles, every team layout — so retail's initial `ScenarioData`
does not depend on the game setup and is therefore derivable from the binary.

`ScenarioFuncSet::init` `0x00a03c30` (sole caller `Game::init`; `ScenarioFuncSet::close`
`0x00a03650` is the end-of-game path and writes a *different* state) is now fully read:
every scalar, both 8×N counter tables, all 14 policy flags, the three colour constants, the
`BitMask<8>`, and every empty container. It is implemented as
`scenario_channel::RetailInitialScenario` and pinned by test. See
[`docs/assembly/scenario-initial-state.md`](../assembly/scenario-initial-state.md) for the
address-by-address table.

Two of the six checksummed `String`s are assigned from the runtime `StringTable`
`int_str_array` (`0x00c06378`) at ordinals **5958** (`general_powers_script_file`) and
**5959** (`temp_save`), i.e. from shipped `internal_strings.xml`, which is **not** in the
local `ron-data/` extraction. With both empty the derived state walks 8,321 bytes and
produces `0xba9c1111`, so those two strings are non-empty and carry roughly 6,783 of byte
weight. No attempt was made to guess them: two free strings against one 32-bit target is
curve-fitting, not confirmation. Extracting one XML file closes the channel.

### Replay-carried Rules and initial-world slices now on the scoreboard

Supported recordings carry an exact 1,024,221-byte static Rules SaveGame section immediately
before the command stream. The parser independently projects the checksum-visible portions in
retail order—806 Types, Constants, Balance, and 24 Tribes—while skipping tags and save-only
strings. Admission requires the measured intermediate checksums `0x72e0c3b6`, `0x50625668`,
and `0x56daabc1`, the final `0x12ba3104`, and exactly 997,846 walked bytes. All 21
checksum-bearing recordings pass; mutations in any component or section tag fail closed. This
proves replay-carried static Rules fidelity, not an independent rules loader or dynamic world.

`Replay::open` structurally parses the complete recording prefix through
`game.info.save_name`, inferring the v15/v16 `GameInfo` mod tail by the PDB
`Game::semaphore` invariant. Across all 61 command streams it recovers:

- `GameInfo::seed` and all 30 setup bytes, including map style/size, game rules
  and starting-town policy;
- all eight `Player` gates and every active player's synchronized counters,
  tribe, `who`, team, handicap, `play`, pauses, difficulty and name;
- the 404-byte `Game` block's named frame/tick/market/world counters plus the
  semaphore, graphic tick and save name.

The map-size selector is resolved through the shipped seven-entry world-edge
list `[40, 50, 60, 70, 80, 90, 100]`. `World::init` supplies the exact derived
dimension arithmetic, and `GameInfo::seed` enters through the oracle-backed
signed `Map::make` seed gate. Starting coordinates are not guessed: the newly
shipped, 250,011-trial oracle-backed `World::start_city_wcoord` supplies the
retail row-major/LSB-first accessor, and the 449,463-trial
`World::add_starting_location` case proves the returned index, all four array
appends and the exact 2×2 bit writes. The bit plane nevertheless stays empty in
`Replay::open`: a full-corpus structural search found **0/61** recordings with
a raw `World::walk_data` dimension/array/scalar prefix after `save_name`, which
agrees with the save-format derivation that this recording block is not a
decoded save snapshot. Until the map-style generator supplies the actual
coordinates for a replay seed, invoking the exact writer would still require
invented inputs and is therefore deliberately not done.

### CORRECTION (2026-08-10): the map generator is no longer unported

This document previously said the map generator was unported for every style and that
`.rcx` "contains no generated WData land plane on which to run the exact repair". **Both
statements are now false**, and the sentences above about `Map::fix_diag_land` are kept only
as the record of how the oracle case was earned.

The generator is executed from the replay's own pinned seed. `SimBridge`'s replay path runs
`InitialItemReconstruction::advance_continent_prefix_with_tilesets` against the
reconstructed `World`, and `schema/replay-validation.json` now records, per file, the exact
retail VA at which that execution stops. Measured over the current corpus:

| first unavailable primitive | files | of which checksum-bearing |
|---|---:|---:|
| `TerrainGroups::fill_fertile` `0x006a6f90` | 21 | **18** |
| `Map::team_continent_partition` (style 19) | 6 | 2 |
| `Map::east_indies_nonplayer_islands` (style 18) | 1 | 1 |
| static Rules only (non-checksummed / older builds) | 24 | 0 |
| prior seed state, custom scenario state | 9 | 0 |

So map styles **6, 9, 12 and 14** — Great Lakes among them — execute their **complete**
`make_continents`, including `Map::fix_diag_land`, `Map::make_coastlines`, pool elimination,
player-land and player-forest checks, resource scheduling and nubify, and stop at
`TerrainGroups::fill_fertile` `0x006a6f90`. That covers 18 of the 21 checksum-bearing
recordings; styles 18 and 19 stop earlier, at their own style-specific partition leaves.

Two things follow, and neither is optimism:

1. The `world` channel's unsourced-byte count is now a *shrinking* measured quantity rather
   than "everything after the dimensions". It is reported per compare as
   `our_unsourced_walked` against `our_bytes_walked`.
2. On this machine `fill_fertile` cannot run at all for **21 of 21** checksum-bearing
   recordings, and the reason is not a port gap: it is
   `ron-data/tilesets.xml: No such file or directory`. The fertility input is a shipped data
   file that has not been extracted. That is the same class of blocker as
   `internal_strings.xml` for channel 14, and it is cheaper to clear than any porting work
   in this document.

The next two start-placement leaves are now exact without changing that source boundary.
`WorldData::start_city_rad_wcoord` scans the writer's footprint arrays using retail's
integer `vector_dist * 4 < Constants::city_center_radius - 1` predicate, and
`MapFairness::calc_distances` writes the team-indexed binary32 distance table and strict
first-wins extrema. Both production sim methods execute against their complete retail
leaves in the oracle. Neither is called by `SimBridge`: `.rcx` still supplies no candidate
coordinates, and exact predicates over invented coordinates would not make the initial
world sourced.

`Map::place_start_in_region` is now exact as well: both passes, the canonical retail-built
circle table, prior-start spacing, concrete output coordinate and final RNG word are
differentially compared against the shipped 592-byte selector and its complete
`is_near_ocean` callee. This still does not authorize replay wiring. The recording prefix
contains neither the selector's `Region::coords` list nor its generated `WData.land` plane;
fabricating either would merely move the first unsourced byte behind an exact function.

---

## The correction this lane paid for: the cross-player floor is zero, not 21

`docs/tracks/headless-client.md` claim 7 records **"265,910 of 265,931
(0.999921) cross-player checksum agreement; the 21 disagreements are real
simulation drift, not decode error."**

**All 21 are a join error.** [measured]

A `CommandPackage` header carries two things that both look like "when":
`stamp` (`+0x00`, the local simulation frame the package was built on) and
`group` (`+0x0c`, the monotone per-turn serial). Joining two players' tuples on
`stamp` compares turn *N* of one client against turn *N±k* of the other whenever
their local frame counters have drifted apart — which they do, because `stamp`
is local wall-clock-driven and `group` is the lockstep turn.

```
aligned by CommandPackage::group (turn serial): 265619/265619 identical
aligned by CommandPackage::stamp (sim frame):   265910/265931 identical
  of the 21 by-stamp disagreements, 21 compare packages from DIFFERENT turns;
  8 stamp buckets mix turns
```

Twenty-one out of twenty-one. Not "mostly", not "probably" — every single
by-stamp disagreement is a comparison between two different turns. Under the
correct join the corpus is **265,619 / 265,619 = 100.0000 % identical**.

Consequences, and they are load-bearing:

1. **There is no retail desync in this corpus at all.** The four "offending"
   files (2018-11-17, 2018-12-01, 2024-03-20, 2024-04-10) are clean.
2. **There is no noise floor to hide behind.** Any mismatch our simulation
   produces is ours. The wave brief's caution — "do not treat a late-game
   mismatch as automatically our bug" — is, on this evidence, unnecessary.
3. `docs/tracks/headless-client.md` claim 7 and the `MIN_CHECKSUM_AGREEMENT`
   floor in `crates/don-net/tests/roundtrip.rs` should be restated: the correct
   join key is `group`, and the correct floor is 1.0.

The harness reports both alignments on every run so this cannot quietly regress:
`don-replay crossplay --corpus`, and the assertion
`cross_player_tuples_agree_when_joined_on_the_turn_serial` in
`crates/don-replay/tests/corpus.rs` fails if any by-stamp disagreement is *not*
a turn mismatch.

---

## How to run it

```sh
tools/replay-validate.sh                 # full corpus -> schema/replay-validation.json
tools/replay-validate.sh --status        # age + headline of the last record, runs nothing
tools/replay-validate.sh --limit 5       # quick pass

cargo run --release -p don-replay -- validate <file.rcx>     # one recording, full table
cargo run --release -p don-replay -- scan --corpus           # decode only
cargo run --release -p don-replay -- crossplay --corpus      # the control experiment
cargo run --release -p don-replay -- walkers                 # generated-table coverage

cargo test --release -p don-replay --all-targets             # 73 lib + 8 corpus + integration tests
```

Exit codes mirror `tools/oracle-regress.sh`: `0` ran, `2` corpus missing
(**SKIPPED is never a pass** — the banner says so), `3` harness failure.

Representative current output (`Playback___2018.11.17_13_21_42__Sat_.rcx`):

```
channel           survived first-div   expected        got  matches   bytes  unsourced
world                    0         2 0xd63a3a53 0x1389cbbb        0  780168     780092
rules                18069         - 0x12ba3104          -    18069  997846          0
script_run_time      18069         - 0x00040001          -    18069       4          0
scenario_data            0         2 0x09922b90 0x00000001        0       0          0
```

That world row is deliberately not credited as agreement. The initial prefix
sources 76 walked bytes; the remaining generated terrain bytes are reported as
unsourced until `TerrainGroups::fill_fertile` and the rest of `Map::make` run.
The `script_run_time` row is credited, but read the caveat under *Honest limits*
before quoting it: it is four bytes, and it is wrong on 14 of the 21 recordings.
The `scenario_data` row shows the target this lane derived but could not source.

---

## What was built

`crates/don-replay`, no external dependencies (same rule as `don-net`: a green
`cargo test` must never touch the registry; gzip shells out to the system tool).

| file | what |
|---|---|
| `src/checksum.rs` | `adler32` mirroring `0x00a46830` including the `NMAX = 5552` chunk boundary; the two-method `DataWalk` trait; `CheckSum` with `+0x0c` mask, `+0x10` adler, `+0x14` byte counter; `Channel`, `Channels`, `computed_total` |
| `src/walk.rs` + `src/walk_gen.rs` | **generated** traversal: 278 classes, all 1,421 ordered ops from `schema/state-schema.json`, run table-driven over object byte images |
| `src/initial.rs` | exact `Game`/`GameInfo`/Player prefix parser; map-size and seed reconstruction; sourced/unsourced accounting |
| `src/state.rs` | per-channel object lists plus direct dynamic walkers; `units` image bridge, exact `world` checksum bridge, and the empty-`RunTimeEnv` channel-15 install |
| `src/script_channel.rs` | `RunTimeEnv::walk_data` `0x009c41a0` / `ScriptFile::walk_data` `0x009c63b0`; `checksum_empty_runtime` is the four-byte empty-registry case, `checksum_program` the live `don-bhs` adapter |
| `src/scenario_channel.rs` | `ScenarioData::walk_data` `0x00997ad0` traversal plus `RetailInitialScenario`, the instruction-derived `ScenarioFuncSet::init` `0x00a03c30` state; blocked on two `internal_strings.xml` ordinals |
| `src/wire.rs` + `src/wire_gen.rs` | **generated** field table for all 82 `*Command` structs; typed field reads; `Order`; `CommandClass` |
| `src/replay.rs` | `.rcx` → authoritative initial setup + turns: framing, XOR/pad recovery, commands/checksums, both crossplay joins |
| `src/harness.rs` | the loop, `WorldSim::from_replay`, cached initial-world walk, and the divergence profile |
| `src/report.rs` | `schema/replay-validation.json` |
| `gen/gen_walk.py`, `gen/gen_wire.py` | the generators; re-run after any schema change |
| `tests/corpus.rs` | 8 corpus tests including mutation, initial-prefix, and non-empty-world gates |

**Both tables are generated, not typed.** Hand-writing a 1,421-op traversal is
precisely how a walker silently drifts from the binary; the generators are two
short Python scripts and re-running them is the only supported way to change the
traversal.

### The checksum model, stated plainly

Fifteen channels. Each one gets a **fresh** `CheckSum` whose adler starts at 1;
`check_all` sums the fifteen accumulators with wrapping 32-bit addition into
`all`. It is not one rolling hash, and `all` is not a hash of hashes. The wire
command carries sixteen `u32` — the fifteen channels plus that sum — in 65
bytes. All 488,557 decoded packets satisfy `word16 == Σ(words 1..15)` and all
fifteen values in all 488,557 are adler-32-shaped (both halves < 65521). At a
≈2⁻³² false-positive rate per packet, that is a strong statement about the
decode.

---

## Measurements that fell out

**Frames per multiplayer lockstep turn is 2, 4, 6 or 8, and it is a property of the
recording.**
Measured per file from `stamp` deltas across consecutive `group` values: 6.0 in
the 2025 and 2018/2019 games, 4.0 in most 2024 games, 2.0 in the
2024-02-23 game. The lockstep turn is **not** the simulation frame, and a
harness that assumes one frame per turn is wrong by a factor of 2 to 6. This is
why `frames_per_turn` is measured and passed to `Simulation::step_turn` rather
than hard-coded.

**The command worklist, ranked by real frequency** across 5,055,253 commands
(`schema/replay-validation.json` → `totals.opcode_counts`):

| opcode | struct | class | count |
|---|---|---|---:|
| `0x48` | `CameraCommand` | Presentation | 1,296,016 |
| `0x4a` | `TurnDataCommand` | Sim | 1,285,782 |
| `0x4f` | `PlayerSpeedCommand` | Sim | 1,011,134 |
| `0x3a` | `NextCheckSumCommand` | Lockstep | 796,957 |
| `0x39` | `CheckSumsCommand` | Lockstep | 488,557 |
| `0x00` | `GroupCommand` | Sim | 78,197 |
| `0x18` | `QueueUpCommand` | Sim | 28,993 |
| `0x19` | `BuildCommand` | Sim | 14,042 |
| `0x15` | `DisbandCommand` | Sim | 10,916 |
| `0x07` | `MoveToCommand` | Sim | 9,178 |
| `0x04` | `AttackCommand` | Sim | 5,715 |
| `0x2e` / `0x2f` | `BuyCommand` / `SellCommand` | Sim | 4,427 / 2,592 |

Totals: 2,468,461 Sim, 1,285,514 Lockstep, 1,301,278 Presentation. The three
per-turn bookkeeping opcodes (`0x4a`, `0x4f`, `0x48`) are 71 % of all traffic;
**the entire player-order stream is under 160,000 commands corpus-wide.** For an
order-application layer, `QueueUpCommand`, `BuildCommand`, `MoveToCommand` and
`AttackCommand` are the whole game.

`0x4a` and `0x4f` are classified `Sim` **conservatively** — their handlers are
unread here, and over-reporting the worklist is the safe direction. Settling
them is cheap and would move a million commands out of the sim set.

**Two channel walkers have no *generated* traversal.** `ScenarioData::walk_data`
(`0x00997ad0`) is `static __cdecl`, so `schema/state-schema.json`'s `this`-taint
never fires on it, and `RunTimeEnv::walk_data` (`0x009c41a0`) is likewise absent
from the 278 resolved classes. `don-replay walkers` prints this, and
`state::tests::channel_walker_resolution_is_thirteen_of_fifteen` fails if the
count changes in either direction. Both now have **hand-derived** traversals instead, in
`src/scenario_channel.rs` and `src/script_channel.rs` — read off the instruction stream and
tested field by field, but outside the generator's guarantee. That is a real difference in
kind and the reason those two modules carry their addresses in every doc comment.

---

## Honest limits

- **`matches` is not `survived`, and the gap is accidental agreement.** `items`
  has 107,882 matches corpus-wide and 0 survival: the channel returns to 1
  mid-game when the item list empties, and our permanently-empty model
  coincidentally agrees. Only `survived` is a progress metric.
- **Static Rules agreement is substantive; other agreement is still mostly empty-state.**
  The `rules` producer independently walks 997,846 replay-carried bytes on all 222,938
  comparisons and agrees. The `world` producer also walks bytes on every comparison but
  disagrees on the first checksummed turn. The remaining empty-channel agreements are not
  evidence about mechanics.
- **`script_run_time`'s 18,069 turns are four bytes wide.** Retail's value there is never
  `1`, so the harness cannot classify the agreement as `trivial`, and it is genuinely
  falsifiable — it is false on 14 of 21 recordings. But what agrees is the count word of an
  empty `ScriptFile::script_files` array. Do not read it as BHS fidelity, and do not let it
  displace `rules` as the headline: `rules` walks 997,846 bytes per compare and this walks
  four.
- **The checksum phase is a parameter, not a finding.** The sender builds its
  package during `PROCESS_TURN` and appends the tuple to the same package that
  carries that turn's new commands, but lockstep executes a turn's commands some
  turns later. `--phase before|after` and `--latency N` exist; the default is
  `before_commands, latency 0` and the run records which was used. **The latency
  is unmeasured.** It does not affect any number above, because our state is
  empty and never moves — it will matter the day it does, and settling it is the
  first thing the next lane should do.
- **The generated traversal is 216 executable ranges out of 1,421 ops**
  (`don-replay walkers`):

  | op kind | count | executable |
  |---|---:|---|
  | `this`-relative byte range | **216** | yes |
  | resolved length, unresolved base | 369 | no — a stack temporary or an untracked pointer |
  | resolved range on a **global** object | 62 | no — real offsets, but not into an object image |
  | fully unresolved | 252 | no |
  | sub-object call, class resolved | 361 | yes (see below) |
  | tag | 145 | yes (a no-op for `CheckSum`) |
  | virtual dispatch `[reg+0x7c]` | 9 | no — needs a concrete receiver |
  | sub-object, class unknown | 7 | no |

  `schema/state-schema.json` reports 647 "resolved byte ranges"; that counts
  every op with a known *length*. Only 216 also have a known `this`-relative
  *base*, which is what an image walk needs. The 62 global-base ranges were
  initially emitted as `this`-relative and would have hashed the wrong bytes
  with full confidence — they are now a distinct `WalkOp::Global` that refuses
  to execute. Closing the remaining gap means a proper abstract interpreter over
  the call graph from `check_all`, which the schema's own caveats already flag.
- **`WalkOp::Sub` recurses on the same image.** Correct for base-class chains
  (`Unit → Object → ObjectData`, all at `this+0`), wrong for member sub-objects
  at a non-zero offset; the schema records the target class but not the member
  offset. `WalkOutcome` counts every op it could not execute
  (`ops_unresolved`, `ops_global`, `ops_virtual`, `ops_sub_unknown`,
  `ops_out_of_range`) so a partial walk can never be mistaken for a complete one.
- **`SimBridge` produces four real channels, not a full initial save.** `units`
  images generated PDB columns. `world` executes `map_terrain::World::walk_data`
  directly because its dynamic arrays cannot be represented by a 372-byte flat
  image. `rules` uses the separately admitted replay-carried static projection.
  `script_run_time` produces the empty `ScriptFile::script_files` count word. The prefix
  proves world dimensions and seed, not generated contents; their walked zeros are counted as
  unsourced and the channel is expected to diverge.

---

## The next three moves, in order

Both of the first two are **shipped-data extraction, not porting.** That is the finding of
this pass: two of the three cheapest remaining channels are blocked on files that exist in
the owned install and not in `ron-data/`. Neither needs a line of new engine code, and
neither should be closed by guessing a value that makes a checksum agree.

1. **Extract `ron-data/internal_strings.xml` and close `scenario_data`.** Every scalar,
   table, flag, colour and container of retail's initial `ScenarioData` is derived from
   `ScenarioFuncSet::init` `0x00a03c30` and implemented in
   `scenario_channel::RetailInitialScenario`. The only unknowns are ordinals **5958**
   (`general_powers_script_file`) and **5959** (`temp_save`) of the shipped internal string
   table. Supply them, compute one checksum, compare against `0x09922b90` — a single
   pre-registered 32-bit test over 21 recordings, and if it passes the channel holds until
   the recording's first kill. Details and the exact procedure:
   [`docs/assembly/scenario-initial-state.md`](../assembly/scenario-initial-state.md).
2. **Extract `ron-data/tilesets.xml` and run `TerrainGroups::fill_fertile` `0x006a6f90`.**
   Eighteen of the 21 checksum-bearing recordings already execute their complete
   `make_continents` from the pinned seed and stop precisely there, and all 21 report the
   same missing-file error. This is the next real byte reduction on `world`, which is the
   only channel where a large fraction of a 280,968–780,168-byte walk is at stake.
3. **Measure the checksum latency, which is still a parameter and not a finding.**
   `script_run_time` is the first channel that both walks bytes and *changes* across a
   recording (2,458 distinct `scenario_data` values and non-constant script values exist in
   the corpus), so a producer for one of them finally makes `--phase` and `--latency`
   distinguishable instead of invisible. Settle it before any dynamic channel is credited
   with survival.

Deferred, and deliberately: **`deaths` and `ammo`**. Their divergence turns (847 / 1,584 in
the 2025 game) are still the first place a real mechanic has to be right, but both start
from initial object state we do not yet reproduce, so they cannot be attempted before the
`world`/`units` initial state exists.

### Hazards the next lane will hit, from sibling lanes

- **`Array<T>` capacity and growth hint are checksummed** — length, capacity,
  grow, flags, *then* elements. A Rust `Vec` with a different growth policy
  desyncs on identical logical state. Every channel that walks a growable array
  is exposed. Byte images sidestep this only if whoever fills them reproduces
  the engine's capacity, not just its length.
- **`Objects::process_all` rotates owner-slot order every frame** by
  `(frame + i) % 10`; a fixed-order scheduler diverges within one tick.
- **Ammo is not driven by `Objects::process_all`** — projectile motion and
  impact run in `Ammo::inc_time` at step 15 of `do_frame`, so projectile deaths
  land at the *end* of a tick in pool-slot order. Relevant directly to move (3).
- **Borders settle over many frames** under a hard global budget of 256
  cells/frame; expecting one-tick settling desyncs the `world` channel.
- **Score refreshes only when `(who + frame) % 10 == 0`.**
- **Sim-critical is a strict subset of save-game state**: some fields are gated
  on `DataWalk::checksum == 0` (city names are the known case), so a walker that
  emits everything `SaveGame` emits will over-hash. The generated table takes
  the `SaveGame`/`LoadGame` ranges; the `CheckSum`-only branches are not yet
  distinguished, and `schema/state-schema.json`'s own caveats say so.

---

## Ledger entries to add to `docs/provenance-ledger.md`

| mechanic | source | tier | evidence |
|---|---|---|---|
| Cross-player checksum tuples are **identical when joined on `CommandPackage::group`**; the 21 known disagreements are a `stamp`-join artifact | `crates/don-replay`, corpus | **C [measured]** | 265,619/265,619 identical by group; 265,910/265,931 by stamp, and **21 of 21** by-stamp disagreements compare packages from different turns; 8 stamp buckets mix turns |
| All 488,557 recorded `CheckSumsCommand` packets satisfy `word16 == Σ(words 1..15)` and are adler-32-shaped | `crates/don-replay/src/replay.rs`, corpus | **C [measured]** | 488,557/488,557 both tests; ≈2⁻³² false-positive rate per packet |
| `rules` channel = `0x12ba3104` in every recording that carries checksums | corpus | **C [measured]** | constant within and across all 21 checksummed files, two engine builds |
| Replay-carried Rules independently project the retail checksum traversal | `crates/don-replay`, corpus | **C [measured]** | 1,024,221 serialized bytes admitted by three intermediate checkpoints plus final `0x12ba3104`; 997,846 bytes walked; 222,938/222,938 non-trivial matches |
| A multiplayer lockstep turn spans **2, 4, 6 or 8 simulation frames**, per recording | `stamp`/`group` deltas | **C [measured]** | current corpus distribution: 1 / 6 / 37 / 16 files; the solo recording measures 1.0; 6.0 for the 00.2024.06.20 recording |
| Prefix-derived world reconstruction enters the checksum scoreboard | `tools/replay-validate.sh` | **C [measured]** | all 61 initial prefixes parsed; 222,938/222,938 `world` comparisons non-trivial; first divergence is the first checksummed turn; 280,968–780,168 bytes walked by map size, with generated bytes explicitly unsourced |
| `RunTimeEnv::walk_data` unconditionally hashes the four-byte `ScriptFile::script_files` count, so an empty script runtime is `0x00040001` and never `1` | `0x009c41a0`, `0x009c40a0`, `0x00c8cba0` | **C [measured]** | 222,938/222,938 `script_run_time` comparisons non-trivial and none `retail_empty`; 7 recordings agree for their whole length (60,342 turns, best 18,069); 14 diverge on the first checksummed turn |
| Retail's initial `ScenarioData` is game-setup-independent | corpus | **C [measured]** | channel 14 = `0x09922b90` on the first checksummed turn of 21/21 recordings across five engine builds, six map styles and every team layout |
| The complete checksum-visible initial `ScenarioData`, minus two shipped-data strings | `ScenarioFuncSet::init` `0x00a03c30`, sole caller `Game::init` | **C [measured]** | every field read off the instruction stream; derived state walks 8,321 bytes to `0xba9c1111`; blocked only on `internal_strings.xml` ordinals 5958/5959 |
| Map styles 6, 9, 12 and 14 execute their complete `make_continents` from the replay's pinned seed | `schema/replay-validation.json` | **C [measured]** | 18 of 21 checksum-bearing recordings stop at `TerrainGroups::fill_fertile` `0x006a6f90`; 2 at `Map::team_continent_partition`, 1 at `Map::east_indies_nonplayer_islands`; all 21 then blocked by a missing `ron-data/tilesets.xml` rather than by a port gap |

And **correct** in `docs/tracks/headless-client.md` claim 7: the 21 cross-player
disagreements are not simulation drift. Under the correct join key the corpus
shows no desync at all.
