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

Over the whole corpus — 62 files, 21 with `0x39` checksums, **645,554 turns**,
**488,557 checksum packets** in the full fidelity run:

| channel | best survival (turns) | matches / compares | trivial matches | unmodelled | non-trivial compares |
|---|---:|---:|---:|---:|---:|
| `walls` | **25,442** | 222,938 / 222,938 | 222,938 | 222,938 | 0 |
| `deaths` | **5,734** | 82,943 / 222,938 | 82,943 | 82,943 | 0 |
| `ammo` | **3,696** | 106,634 / 222,938 | 106,634 | 106,634 | 0 |
| `items` | 0 | 107,882 / 222,938 | 107,882 | 107,882 | 0 |
| **`rules`** | **25,442** | **222,938 / 222,938** | **0** | **0** | **222,938** |
| **`script_run_time`** | **18,069** | **60,342 / 222,938** | **0** | **0** | **222,938** |
| **`scenario_data`** | **6,320** | **24,245 / 222,938** | **0** | **0** | **222,938** |
| **`groups`** | **64** | **140 / 222,938** | **0** | **0** | **222,938** |
| `cities` | 0 | 0 / 222,938 | 0 | 0 | **36,955** |
| **`world`** | 0 | 0 / 222,938 | 0 | 0 | **222,938** |
| every remaining channel | 0 | 0 / 222,938 | 0 | 0 | 0 |

`survived` = consecutive agreeing turns from the recording's first checksummed
turn. **The honest headline is still 25,442 substantive turns on `rules`.** The
replay's static Rules SaveGame section is independently parsed and projected through the
retail checksum traversal; all 222,938 comparisons walk 997,846 bytes and agree. The
scoreboard now has **1,151,645** comparisons in which our walker touched a byte: `rules`
agrees throughout, `script_run_time` agrees on seven recordings and diverges on fourteen,
`scenario_data` walks the derived 8,453-byte `Game::init` image on every compare and holds
for thousands of turns on those same seven, `groups` walks the derived 36,896-byte
`Groups::clear` image on every compare and holds for 7–64 turns on those same seven, and
the conditional `cities` producer walks 228 fully sourced constructor bytes on 36,955
comparisons. Every `world` comparison walks real prefix-derived state and exposes the first
dynamic divergence.

The previous 25,442-turn `walls` headline remains a weak empty-state result: `walls` was
empty in every recorded game, and adler-32 over nothing is 1 on both sides. `trivial` counts
exactly that case.

`ammo` and `deaths` still measure absent producers, not mechanics: they read 1
until retail creates the first projectile or corpse. Their measured deadlines
remain useful, but those matches are explicitly labelled `unmodelled`.

`units`, `builds`, `leaders`, `cities`, `goods`, `guys` diverge on the **first**
checksummed turn. Cities is no longer absent: three recordings now compare an exact frozen
constructor producer, and the first-turn loss measures its still-missing pre-checkpoint
schedule. We still hold none of the other five. `scenario_data` now agrees on the first
checksummed turn and diverges on the
second; `groups` agrees on the first checksummed turn of the seven recordings with no AI
players and holds for 7–64 turns there. `world` also diverges on the first checksummed turn, for a better
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

### `scenario_data` now has a producer, and the corpus splits on AI players

Channel 14 reads `0x09922b90` on the first checksummed turn of **21 of 21** recordings —
five engine builds, six map styles, every team layout — so retail's initial `ScenarioData`
does not depend on the game setup and is therefore derivable from the binary.

`ScenarioFuncSet::init` `0x00a03c30` (sole caller `Game::init`; `ScenarioFuncSet::close`
`0x00a03650` is the end-of-game path and writes a *different* state) is fully read: every
scalar, both 8×N counter tables, all 14 policy flags, the three colour constants, the
`BitMask<8>`, and every empty container. Its last two inputs were the shipped
`internal_strings.xml` ordinals **5958** (`general_powers_script_file`) and **5959**
(`temp_save`), which `init` assigns by fixed byte offset into `int_str_array`
(`0x00c06378`). The file is now extracted, and the ordinals bind to
`./scenario/scriptlibrary/general_powers.bhs` and `editor_scratch_file.svx`.

The single 32-bit test that
[`docs/assembly/scenario-initial-state.md`](../assembly/scenario-initial-state.md) §5
pre-registered before those strings were available now passes: the derived state walks
**8,453 bytes** and produces exactly `0x09922b90`. No field was tuned, and the recorded wire
checksum is never an input to the producer — `SimBridge::populate_scenario_initial` installs
bytes derived from the initializer, not a value copied off the wire.

**This agreement is substantive, not empty-state.** An absent channel walks zero bytes and
reads `1`; this one hands the visitor 8,453 real bytes on every comparison, and the
scoreboard records `trivial = 0` and `unmodelled = 0` for it.

**And the producer is frozen at `Game::init`, so it is guaranteed to expire.** Nothing in
`don-sim` writes `units_killed`, `builds_destroyed`, `last_razed` or `city_lost_to`, so the
claim it makes is "no scenario state has moved yet". Measured, the corpus splits **exactly**
on whether the game had computer players:

| recordings | AI players | `scenario_data` survived | `script_run_time` |
|---|---:|---|---|
| 7 | 0 | 37 – **6,320** turns; two never diverge | agrees throughout |
| 14 | 2–5 | 1 turn (diverges turn 3) | diverges turn 2 |

Those are the same seven recordings on both channels, and the seven that loaded no BHS
program. So the fourteen AI games lose channels 14 and 15 to one event: **an AI script runs
and its `ScenarioFuncSet` builtins write `ScenarioData`.** Channel 14's next increment is
therefore BHS work, not scenario work. One derived hypothesis for the turn-3 value —
`Setup::build_game` `0x005ac190` assigning `general_powers_script` the extension-stripped
basename of the script path — was computed and **rejected**.

For the human-only games, §5 step 4's prediction holds: solving the two adler halves against
the enumerated field set gives a unique named answer at every one of the five divergences,
and it is `last_razed[who]` — written by `Object::disband` `0x006455c0` and
`Object::take_damage` `0x00652020` — carrying object-id-shaped values 2017–2077. That is a
residual *analysis*, not a producer: none of those values is installed, because a value
chosen to make a checksum agree is worse than a divergence. See §8 and §9 of
[`docs/assembly/scenario-initial-state.md`](../assembly/scenario-initial-state.md).

### `groups` now has a producer, and it is 36,896 bytes wide

Channel 5's walker is `CheckSums::check_groups` `0x00937530`. It is the third channel whose
initial state turned out to be **setup-independent**, and the corpus said so before any
disassembly did: taking each of the 21 checksum-bearing recordings' value on its first
checksummed turn, `units`/`builds`/`guys`/`leaders`/`cities`/`goods`/`items`/`world` each
have **21 distinct** values, while `groups` has 15 — and its repeated value `0x1c78f3f5`
occurs in **exactly the seven zero-AI recordings**, the same seven that survive on
`scenario_data` and `script_run_time`.

`Game::init` `0x0058c480` calls `Groups::clear` `0x00713f20`, which forces the
`Array<Group>` count to **512**, calls `Group::clear` `0x00713e80` on each slot
(`id = i`, `army = -1`, `form = -1`, everything else zero), re-zeros `stamp` and
`priority` per slot — erasing `Group::clear`'s one non-constant store, `Game+0x550` — and
writes `last_group[8] = {0, 0x40, …, 0x1c0}`. `Group::walk_data` `0x00708400` walks
`[this+4, this+0x4c)` = 72 bytes and, only when `num != 0`, six `num`-length member
arrays; a cleared slot has `num == 0`. `check_groups` then hashes 32 bytes through
`GroupsData::const_last_group`. `512 × 72 + 32 = 36,896` bytes, and the adler-32 over them
is `0x1c78f3f5` — **derived, with no free parameter and without consulting the recorded
value**. Full derivation:
[`docs/assembly/groups-initial-state.md`](../assembly/groups-initial-state.md).

`SimBridge::populate_groups_live` independently walks the canonical tick-owned `Sim.groups`
pool after `Sim::do_frame` reaches its per-frame `Groups::process` pass. The fresh pool derives
to the same value above; later in-memory mutations are no longer hidden by a frozen direct
checksum. The current scheduler callback shell still supplies conservative `Keep`/no-speed
answers, so source-backed object-removal and leader-speed authority remain required once live
groups reach normalization. The result, per recording:

| recording | AI | `groups` survived | first divergence |
|---|---:|---:|---|
| 2024.02.23 20:49 | 0 | **64** | turn 66 `0x22a5074d` |
| 2019.03.24 | 0 | **18** | turn 20 `0x624df46d` |
| 2024.03.29 21:52 | 0 | **15** | turn 17 `0x1feaf88c` |
| 2018.11.17 | 0 | **14** | turn 16 `0x1118f44b` |
| 2020.02.21 | 0 | **14** | turn 16 `0x11d9f454` |
| 2018.12.01 | 0 | **8** | turn 10 `0x123cf3cf` |
| 2020.02.08 | 0 | **7** | turn 9 `0xdffdf4f4` |
| the other 14 | 2–5 | **0** | turn 2 |

**State this at its real size.** 140 matches corpus-wide is a small number and is the
honest one. What the channel is *not* is empty-state: retail's own `groups` value is never
1 (it always has 512 slots), so `retail_empty_compares` is 0, and our walker hands the
visitor 36,896 real bytes on all 222,938 comparisons — `trivial` and `unmodelled` are both
0. What agrees is the live Sim owner's derived `Game::init` image plus the claim "no group
slot has been touched yet". The producer itself is no longer frozen, but the real replay cannot
yet execute its first selected-member package because post-worldgen Unit/content authority is
missing. `GroupCommand` `0x00` is the corpus's largest player-order opcode (78,197 commands).
The 140 matches therefore remain evidence about the traversal, initializer and live ownership
path; they are **not yet** evidence about a retail Group action.

Two measurements fell out that are worth keeping. The fourteen AI recordings lose channel 5
on turn **2** while they lose channel 14 on turn **3** — so an AI's first group creation
precedes its first `ScenarioFuncSet` write. And `CheckSums::check_groups` never calls
`Array<Group>::walk_data` `0x0047ea30`: it reads the count as a loop bound and walks
elements only, so the standing "`Array<T>` capacity and growth metadata are checksummed"
hazard, which is real for `Groups::walk_data`'s SaveGame path, **does not apply to this
channel**.

### `cities` now has a conditional exact producer — and a measured frame-zero loss

Channel 9 (zero-based id 8) is `CheckSums::check_cities` `0x00937600`. The exact dynamic
walker was already recovered, including the checksum-only omission of City names and the
checksum-visible allocation metadata in `Array<CaravanLink>`. What was missing was a lawful
bridge: `don-sim::tick::Sim` now owns the canonical `CityPool`, but the replay scorer still
treated Cities as absent.

`SimBridge::populate_sim_cities` now validates that pool against all Sim-owned Leader-validity
mirrors and each live center Build's registry id, owner, City link, position, and production
type. It then hashes the City records in retail owner/slot order. The bridge clears the old
channel before validation, so a broken join cannot leave a stale checksum installed. No
recorded checksum enters the producer.

Three ordinary all-land, two-human recordings admit the existing starting-City constructor:

| recording | compares | bytes / compare | first retail value | frozen constructor value |
|---|---:|---:|---:|---:|
| 2018.11.17 | 18,069 | 228 | `0x53130d2c` | `0x71020a46` |
| 2020.02.08 | 9,789 | 228 | `0xdd170c24` | `0x247c0992` |
| 2020.02.21 | 9,097 | 228 | `0x7d2d0bd4` | `0xcd970956` |

That is **36,955 non-trivial and substantive comparisons**, up from zero; all have zero
unsourced bytes, a complete walk, and an exact producer. Matches and survival remain **0**.
This remains the intended measured result: the state is the fresh constructor, while retail
runs the frame-zero `Leader::plan_strategy` Unit and terrain census before its first checkpoint.
An exact bounded starting-Unit census transaction now owns `peasant_dist`, `free`, `busy`, and
`gatherers`, but it is not mounted because the setup host does not yet materialize its
receipt-backed canonical Units. The setup host now installs the exact activation-time TData
`CITY` disc (not a WData flag); the terrain census reaches grade four and then refuses at the
first missing generated-content `World::gather_at` fact. Final territory is also not joined.
The producer remains the same frozen constructor, so a full corpus regeneration keeps the
**36,955 / 0 matches / 0 survival** metrics unchanged. Full channel derivation and the
fail-closed bridge mutation proof are in
[`docs/assembly/replay-cities-sim-channel.md`](../assembly/replay-cities-sim-channel.md).
The new transaction and its atomic refusal boundary are in
[`docs/assembly/replay-starting-city-unit-census.md`](../assembly/replay-starting-city-unit-census.md).

### The corpus has a **second** checksum stream, and 39 recordings were carrying it

`CheckSumsCommand` `0x39` is not the only lockstep checksum in the corpus.
`NextCheckSumCommand` `0x3a` — six bytes, a `CheckSumTypes` index and one subsystem's
checksum — appears in **39 of the 62 recordings**, 796,957 records, and in **no** recording
that carries a `0x39` tuple. The two are disjoint by engine build: `0x3a` is the
`03.02.03.2905` / `00.2014.07.1000` / `00.2014.10.0200` recorders, `0x39` is
`00.2017.11.2900` / `00.2024.06.2000`. So "62 files, 21 with `0x39` checksums" is not the
whole corpus result: **60 of 62 recordings carry lockstep checksums**, in one of two
formats. Full derivation:
[`docs/assembly/next-checksum-stream.md`](../assembly/next-checksum-stream.md).

The shipped `riseofnations.exe` still *processes* `0x3a`
(`CommandPackage::process_next_check_sum` `0x00945e20`, which stores the value into
`[0x00cbee90]` indexed by the package's player) but never *issues* one: none of the 102 call
sites of the package-append helper `0x0094bae0` appends six bytes.

Each recording emits one record per player per turn from turn 2, sweeping the types in
`CheckSumTypes` order — 0 non-contiguous turn steps across 440 runs, non-decreasing in 38 of
39 files. A type's run of turns is `elements + 1`, pinned twice: `walls`, empty in every
recorded game, runs exactly **1** turn in 30 of 30 files, and `leaders` runs exactly **9** in
25 of 25 while `CheckSums::check_leaders` `0x009375a0` iterates exactly **8** slots. So a
**one-turn run carries the whole channel**, and only those are compared. The other 796,489
records sit inside longer runs whose per-element meaning is deliberately left uninterpreted.

What that scores, kept in its own `totals.next_checksum` section so it can never be added to
the numbers above:

| channel | whole-channel turns | compares | matches | what it means |
|---|---:|---:|---:|---|
| `rules` | 38 | **5** | **5** | 997,846 bytes walked per compare |
| `groups` | 27 | 27 | 0 | live Sim pool still at `Game::init` against a turn-300+ record |
| `world` | 25 | 18 | 0 | retail reads `1` here; we walk 780k bytes |
| `walls` / `ammo` / `deaths` | 76 | 0 | 0 | **no producer — not scored** |

That last row is the point. All 76 would have agreed (`1 == 1`), and every one of them is
vacuous, so they are counted as `no_producer` rather than as matches;
`an_absent_producer_is_never_credited_with_a_match` fails if that changes.

The five `rules` agreements are substantive but small. **The falsifiable half is the
refusals**: the `0x3a` recordings carry **nine distinct rulesets** where the `0x39` corpus
carries one, and our producer admitted the carried Rules section in exactly the recordings
whose own client reported the shipped `0x12ba3104` — 29 chances to admit a foreign ruleset,
none taken, and zero disagreements where it did admit. It also found a gap: **3 recordings
report `0x12ba3104` on the wire while `Replay::open` locates no Rules section in them**, so
the locator misses a container layout the older recorder uses.

### CORRECTION (2026-08-11): the corpus does contain retail desyncs — on the other stream

The `0x39` cross-player result below stands exactly as written: 265,619 of 265,619 identical
under the `group` join. It is a statement about **21** recordings. The same control
experiment on the `0x3a` stream, over the other 39, does not come out clean:

```
445,347 comparisons, 445,332 identical, 15 disagreements
```

Every one of the 15 is within **two turns of its recording's last turn** — the client
detects the divergence and the recording stops. Two of them are the same story told twice:
`Playback___2017.07.14_23_34_08` and `playback___2014.04.26_16_11_55` disagree on
**`rules` at turn 3** and are over by turn 4 and turn 3. Two more
(`…23_35_56`, `…23_37_35`) disagree on `units` at turns 31 and 37 and end at 33 and 39. The
remaining five are `script_run_time` records on the recording's own final turn.

So "there is no retail desync in this corpus at all" must be read as "in the 21
`CheckSumsCommand` recordings". On the other 39 there are four games that ended in one — and
none of them involves our simulation, so it is still true that any mismatch **we** produce is
ours.

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
cargo run --release -p don-replay -- nextsum --corpus        # the 0x3a stream + its desyncs
cargo run --release -p don-replay -- walkers                 # generated-table coverage

cargo test --release -p don-replay --all-targets             # 73 lib + 8 corpus + integration tests
```

Exit codes mirror `tools/oracle-regress.sh`: `0` ran, `2` corpus missing
(**SKIPPED is never a pass** — the banner says so), `3` harness failure.

Representative current output (`Playback___2018.11.17_13_21_42__Sat_.rcx`):

```
channel           survived first-div   expected        got  matches   bytes  unsourced
world                    0         2 0xd63a3a53 0x4e2a1e45        0  780276     780200
rules                18069         - 0x12ba3104          -    18069  997846          0
script_run_time      18069         - 0x00040001          -    18069       4          0
scenario_data         6320      6322 0x151c27b9 0x09922b90     6320    8453          0
groups                  14        16 0x1118f44b 0x1c78f3f5       14   36896          0
```

That world row is deliberately not credited as agreement. The initial prefix
sources 76 walked bytes; the remaining generated terrain bytes are reported as
unsourced until `TerrainGroups::fill_fertile` and the rest of `Map::make` run.
The `script_run_time` row is credited, but read the caveat under *Honest limits*
before quoting it: it is four bytes, and it is wrong on 14 of the 21 recordings.
The `scenario_data` row is the derived `ScenarioFuncSet::init` image: 8,453 sourced bytes,
zero unsourced, 6,320 turns of agreement in this recording, and a producer that is frozen at
`Game::init` and therefore guaranteed to expire. The `groups` row is the derived
`Groups::clear` image, on the same terms and with a much shorter fuse: 36,896 sourced bytes
and 14 turns, because a group command lands within seconds of the start.

---

## What was built

`crates/don-replay`, no *direct* third-party dependencies (same rule as `don-net`; gzip
shells out to the system tool). Its workspace path dependencies are `don-sim`, `don-net`,
`don-bhs` and — since channel 14 — `don-content`, whose `parse_string_table_xml` is the
crate's one retail-derived positional `StringTable` binder. A second XML parser here would
have been a second chance to be off by the eight self-closing `<STRING/>` entries.

| file | what |
|---|---|
| `src/checksum.rs` | `adler32` mirroring `0x00a46830` including the `NMAX = 5552` chunk boundary; the two-method `DataWalk` trait; `CheckSum` with `+0x0c` mask, `+0x10` adler, `+0x14` byte counter; `Channel`, `Channels`, `computed_total` |
| `src/walk.rs` + `src/walk_gen.rs` | **generated** traversal: 278 classes, all 1,421 ordered ops from `schema/state-schema.json`, run table-driven over object byte images |
| `src/initial.rs` | exact `Game`/`GameInfo`/Player prefix parser; map-size and seed reconstruction; sourced/unsourced accounting |
| `src/state.rs` | per-channel object lists plus direct dynamic walkers; `units` image bridge, exact `world` checksum bridge, and the empty-`RunTimeEnv` channel-15 install |
| `src/script_channel.rs` | `RunTimeEnv::walk_data` `0x009c41a0` / `ScriptFile::walk_data` `0x009c63b0`; `checksum_empty_runtime` is the four-byte empty-registry case, `checksum_program` the live `don-bhs` adapter |
| `src/groups_channel.rs` | `CheckSums::check_groups` `0x00937530` / `Group::walk_data` `0x00708400`, including the six `num`-gated member arrays the generated table leaves `Unresolved`; `RetailInitialGroups` is the 512-slot state `Groups::clear` `0x00713f20` + `Group::clear` `0x00713e80` leave at `Game::init`, and `InitialGroupsChannel` walks it |
| `src/scenario_channel.rs` | `ScenarioData::walk_data` `0x00997ad0` traversal, `RetailInitialScenario` (the instruction-derived `ScenarioFuncSet::init` `0x00a03c30` state), and `InitialScenarioChannel`, which binds `internal_strings.xml` ordinals 5958/5959 through `don-content`'s retail `StringTable` parser and fails closed when the shipped table is absent |
| `src/next_checksum.rs` | `NextCheckSumCommand` `0x3a`: the `CheckSumTypes` → channel binding, the sweep-run reconstruction, the `0x3a` cross-player experiment, and a scorer that compares **only** one-turn runs and **only** where a producer is installed |
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

**A third traversal is now partly hand-derived, for a different reason.** `Group` *is* one
of the 278 resolved classes, but only 1 of its 7 ops is executable: the six `num`-length
member walks have ends computed at run time, so the extractor emits `Unresolved` for all of
them. `src/groups_channel.rs` resolves those six from `Group::walk_data`'s instruction
stream and pins its op 0 against the generated `WalkSpec` in
`groups_channel::group_ops_agree_with_the_generated_table`, so the hand-derived half cannot
drift from the extraction without a test failing. The initial state has `num == 0` on every
slot, so today only op 0 executes; the other six are written, tested and dormant.

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
- **`groups`' 64 turns are 36,896 bytes wide and 140 compares long.** The width makes the
  agreement substantive — retail's value there is never 1, `trivial` and `unmodelled` are
  both 0, and the channel is falsifiable and false on 14 of 21 recordings. The length makes
  it small: 140 matches out of 222,938 compares. Both numbers are the result. Do not quote
  the byte count without the match count, and do not let it displace `rules`.
- **The `0x3a` stream adds 50 comparisons, not 796,957.** 39 recordings carry it, but only
  one-turn runs are interpretable and only three channels have a producer that reaches one:
  `rules` 5, `groups` 27, `world` 18. The other 796,489 records are inside longer runs whose
  per-element meaning is deliberately not modelled — reading one of those as a whole-channel
  value would compare a single element's walk against a channel. `CHECKSUM_ALL`'s 39 records
  are likewise uninterpreted: `check_all` `0x00936560` *returns* channel 15 while the `0x39`
  tuple's sixteenth word is the **sum**, and which one the older builds recorded is unsettled.
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
- **`SimBridge` produces five real channels, not a full initial save.** `units`
  images generated PDB columns. `world` executes `map_terrain::World::walk_data`
  directly because its dynamic arrays cannot be represented by a 372-byte flat
  image. `rules` uses the separately admitted replay-carried static projection.
  `script_run_time` produces the empty `ScriptFile::script_files` count word.
  `scenario_data` produces the derived `ScenarioFuncSet::init` image and then **freezes**:
  no `don-sim` path writes `units_killed`, `builds_destroyed`, `last_razed` or
  `city_lost_to`, and no `ScenarioFuncSet` builtin reaches it, so it is
  the `Game::init` state and nothing after it. `groups` differs: the checksum is re-projected
  from canonical `Sim.groups` after every scheduled `Groups::process` pass. The exact walker and
  ownership join are complete; the scheduler's object-removal and leader-speed callback facts
  are not. The channel remains at the fresh image on the corpus because replay setup cannot yet
  provide the Unit/content receipts required by `Groups::push_group` and the first
  `Group::action_*`. The prefix proves world dimensions and seed, not generated contents; their
  walked zeros are counted as unsourced and the channel is expected to diverge.

---

## The next three moves, in order

1. **Make `ScenarioFuncSet` builtins write `ScenarioData`, and channels 14 and 15 move
   together.** The initial state is closed — extracting `internal_strings.xml` and binding
   ordinals 5958/5959 made the derived `ScenarioFuncSet::init` image reproduce `0x09922b90`
   exactly, and it now survives up to 6,320 turns. Every remaining `scenario_data` loss is
   on one of the fourteen recordings with computer players, on the same turn their BHS
   program loads and `script_run_time` dies. That is one subsystem holding two channels,
   and the shared turn-3 values across unrelated games (`0x01d5286b` in six of them) say the
   first few builtin calls are setup-independent and therefore reproducible. Start from the
   `ScenarioFuncSet` `find`/`involved_who` writers, not from the value space. Details:
   [`docs/assembly/scenario-initial-state.md`](../assembly/scenario-initial-state.md) §8–§9.
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

And a fourth, newly sharp because channel 5 now reads canonical state: **finish the replay
setup-to-package Unit authority join.** Every one of the seven human recordings loses the
channel at its first `GroupCommand`, between turns 9 and 66. The exact walker, live Groups
owner, full move-near decompilation and bounded Sim transaction now exist. What remains is the
source-backed Unit/content/formation/order/path state at that package frame, including preceding
Farm packages for the strict 2018 witness, followed by the object-removal and leader-speed facts
used when a live slot reaches `Groups::process`. The exact receipts and chronology are in
[`docs/assembly/replay-groups-pre-pair-unit-authority.md`](../assembly/replay-groups-pre-pair-unit-authority.md).

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
| The initial `groups` channel is `0x1c78f3f5`, the 512 slots `Groups::clear` leaves at `Game::init` plus the 32-byte `last_group` tail | `CheckSums::check_groups` `0x00937530`, `Groups::clear` `0x00713f20`, `Group::clear` `0x00713e80`, `Group::walk_data` `0x00708400` | **C [measured]** | derived state walks 36,896 bytes to `0x1c78f3f5` with no free parameter; occurs on the first checksummed turn of exactly the 7 zero-AI recordings and none of the 14 AI ones; 222,938/222,938 `groups` comparisons non-trivial, `trivial = unmodelled = retail_empty = 0`, 140 matches, best survival 64 turns |
| `CheckSums::check_groups` does **not** walk the `Array<Group>` header, so the capacity/growth-metadata hazard does not reach channel 5 | `0x00937530` vs `Groups::walk_data` `0x00713e30` → `Array<Group>::walk_data` `0x0047ea30` | **C [measured]** | the checksum walker reads `[0x00e85f14]` as a loop bound and walks elements only; the SaveGame walker walks count, size, the 2-byte growth increment and a flags byte, plus `proc_group`, which the checksum omits |
| `CheckSums::check_groups`' 32-byte tail bypasses `CheckSum::walk_function`, so retail's own byte counter under-reports the channel by 32 | `0x00937530`, `adler32` `0x005089d0` | **C [measured]** | the tail loop reads and writes only `CheckSum+0x10`, never `+0x14`; the binary carries two byte-identical `adler32` copies (`0x005089d0`, `0x00a46830`) |
| Map styles 6, 9, 12 and 14 execute their complete `make_continents` from the replay's pinned seed | `schema/replay-validation.json` | **C [measured]** | 18 of 21 checksum-bearing recordings stop at `TerrainGroups::fill_fertile` `0x006a6f90`; 2 at `Map::team_continent_partition`, 1 at `Map::east_indies_nonplayer_islands`; all 21 then blocked by a missing `ron-data/tilesets.xml` rather than by a port gap |

| `NextCheckSumCommand` `0x3a` is a **second, per-subsystem lockstep checksum stream** carried by 39 of the 61 recordings and by none that carries `0x39` | `crates/don-replay/src/next_checksum.rs`, corpus | **C [measured]** | 796,957 records over 39 files, all with `checksum_packets == 0`; `checksum_type` ∈ `0..=14` = `CheckSumTypes` (`schema/types.json`); the shipped binary processes it at `0x00945e20` and has no six-byte `0x0094bae0` append, so it never issues one |
| The `0x3a` sweep spends `elements + 1` turns per subsystem, so a **one-turn run carries the whole channel** | corpus + `CheckSums::check_leaders` `0x009375a0` | **C [measured]** | one record per player per turn from turn 2, 0 non-contiguous steps over 440 runs, non-decreasing in 38/39; `walls` (empty in every recorded game) runs 1 turn in 30/30 and `leaders` runs 9 in 25/25 while `check_leaders` iterates `(0xe71af0−0xe3a390)/0x6eec = 8` slots |
| The replay-carried Rules producer is **fail-closed correct across nine rulesets** | `crates/don-replay/src/rules_channel.rs` vs the `0x3a` stream | **C [measured]** | 5 whole-channel turns admitted and equal to the wire value; 29 refused where the wire value is not `0x12ba3104`; 0 admitted with a foreign ruleset; 3 refused although the wire says shipped, a named gap in `Replay::open`'s section locator |
| **The corpus contains retail desyncs**, on the `0x3a` stream, and each recording ends at one | corpus | **C [measured]** | 445,347 cross-player comparisons, 445,332 identical, 15 disagreements, all within 2 turns of their recording's last turn; two are `rules` mismatches on turn 3 in games that end on turns 3 and 4; two are `units` mismatches at turns 31 and 37 in games that end at 33 and 39 |
| The `world` channel reads `1` on every `0x3a` recording that reaches it, while never reading `1` on `0x39` | corpus, `issue_check_sums` `0x00940770` | **C [measured]** | 25 of 25 whole-channel `world` records are `1`; the world walk is the one channel behind a guard, `if ([[0x00c06188] + 0x134] != 0)`, so the channel can keep its initial `1` without the world being empty |

And **correct** in `docs/tracks/headless-client.md` claim 7: the 21 cross-player
disagreements are not simulation drift. Under the correct join key the corpus
shows no desync at all **on the `CheckSumsCommand` stream**. On the
`NextCheckSumCommand` stream, over a disjoint 39 recordings, there are four games that ended
in a real one — see the 2026-08-11 correction above.
