# The coverage ledger

What of *Rise of Nations* we actually have, measured from the engine's own structure rather
than estimated. `tools/coverage-ledger.py` writes `schema/coverage.json`; its binary and
citation counts are generated, while its small channel/schedule tables and source partition
are explicitly hand-classified. None of its Rust-side inventories is a runtime call graph.

```sh
cd /Users/ember/dev/don/ron-bin
uv run --quiet --with capstone --with pefile python ../tools/coverage-ledger.py
```

Snapshot: **2026-08-08**, against `riseofnations.exe` sha256 `30478a44…625079` and
`ron-bin/sbl/rise.pdb`, regenerated after the interrupted-wave audit. The Rust-side counts
move with the worktree; the binary-side counts do not.

---

## 0. The headline, in three layers

The denominator below is a **seeded potential-reachability approximation**: 1,746,457 bytes
of x86 across `main/game/` files classified as simulation. It follows direct calls from known
roots but also seeds broad virtual-method name families; it therefore has both false positives
and false negatives and must not be called “tick-reachable” (§4).

| layer | question it answers | measure | share |
|---|---|---:|---:|
| **named** | has a lane read this retail function and written Rust citing it? | 487,997 B | **27.9 %** |
| **runtime-core inventory** | is the citing source in a small reviewed frontend/runtime allowlist? | 89,241 B | **5.1 %** |
| **tested** | is it differentially tested against retail machine code? | 4,158 B | **0.24 %** |

> **The honest one-line summary: about a quarter of the seeded simulation closure is cited,
> runtime execution coverage is not yet mechanically measured, and about a four-hundredth
> is checked against the real thing.**

Method, stated so it can be attacked:

* **named** counts every `0x00xxxxxx` literal in `crates/**/*.rs`, resolved to the PDB
  procedure whose `[va, va+size)` contains it. It is a claim of derivation, so it is an
  **upper bound on fidelity** — a citation in a doc comment is not a proof. It is also
  slightly generous in aggregate: `game/constants.cpp` alone is 90,256 B of it and is 100 %
  “covered” because `don-rules` cites the whole rules loader. One broad citation can therefore
  move the aggregate substantially.
* **runtime-core inventory** is a manual source-file allowlist, not call-graph evidence.
  It is retained only as a rough inventory (§5); do not quote it as execution coverage.
* **tested** is `schema/oracle-regression.json`: 12 registered cases, 16,236,396 trials,
  all passing, covering **11 distinct retail functions of which 6 are in `main/game/`**
  (`ObjectData::get_damage`, `Balance::return_modifier`, `Doober::get_num`, `flanking`,
  `vector_dist`, `ObjectData::get_captain`).

The gap between *named* and actual runtime execution remains the important question, but this
generator does not currently quantify the latter; §5 records the narrower evidence we do have.

---

## 1. The fifteen checksum channels

`CheckSums::check_all` `0x00936560` was re-disassembled for this audit and its direct calls
taken in address order, dropping `SyncLogger::logToMemory`. **Fifteen channels, confirmed
independently** [measured].

`walked` is the byte count `schema/state-schema.json` recovers for the element class's
`walk_data`; `sizeof` is the PDB class size. A low `walked/sizeof` ratio is not a defect —
it is the engine telling us how much of a class is sim-critical.

| # | channel | checker | element walker | walked / sizeof | Rust module | status |
|--:|---|---|---|---:|---|---|
| 1 | `units` | `check_units` `0x009371D0` | `Unit::walk_data` `0x0060CF40` | 111 / 344 | `movement`, `groups_guys`, `combat`, `air`, `naval` | partial |
| 2 | `builds` | `check_builds` `0x00937290` | `BuildData::walk_data` `0x0062F270` | 28 / 220 | `production` | partial |
| 3 | `walls` | `check_walls` `0x00937360` | `WallData::walk_data` `0x00642510` | 30 / 112 | `walls`, `production` | partial |
| 4 | `ammo` | `check_ammo` `0x009374E0` | `AmmoData::walk_data` `0x0067AB50` | 100 / 108 | `ammo`, `air` | partial |
| 5 | `deaths` | `check_deaths` `0x00936BB0` | inline, `DeathObjData` | — | `combat` | partial |
| 6 | `groups` | `check_groups` `0x00937530` | `Group::walk_data` `0x00708400` | 72 / 2516 | `groups_guys` | partial |
| 7 | `guys` | `check_guys` `0x00937430` | `GuyData::walk_data` `0x005E0210` | 155 / 188 | `groups_guys` | partial |
| 8 | `leaders` | inline in `check_all` | `LeaderData::walk_data` `0x006D6750` | 27182 / 28388 | `economy`, `tech_cities`, `victory_score` | partial |
| 9 | `cities` | `check_cities` `0x00937600` | `City::walk_data` `0x00489220` | 110 / 192 | `tech_cities` | partial |
| 10 | `items` | `check_items` `0x00937790` | `Item::walk_data` `0x00677150` | 22 bytes per live item (re-audited) / 44 | `items` | partial; compiled, no runtime producer |
| 11 | `goods` | `check_goods` `0x00937710` | `Good::walk_data` `0x0066E5D0` | 1 / 48 | `economy` (`goods_channel`) | partial |
| 12 | `world` | inline | `World::walk_data` `0x006B5CF0` | unresolved / 372 | `borders_fog` **and** `map_terrain` | partial, **contested** |
| 13 | `rules` | inline | `Game::walk_rules_data` `0x00589550` | 997,846 bytes in the live shipped walk | `rules_channel` | partial; exact walker, incomplete checked-in inputs |
| 14 | `scenario` | inline | `ScenarioData::walk_data` `0x00997AD0` | pointer-rich structural walk | `scenario_channel` | partial; no runtime producer |
| 15 | `script` | inline | `RunTimeEnv::walk_data` `0x009C41A0` | pointer-rich structural walk | `script_channel` | partial; no BHS execution/runtime producer |

**Fifteen partial, zero complete.** No
module reproduces a whole integrated channel, and nothing composes the fifteen into a
`check_all` equivalent — there is no Rust function that returns a comparable checksum for a
whole world.

Three findings that fall out of the table:

* **Channel 12 has two owners.** `borders_fog.rs` and `map_terrain.rs` both declare
  themselves the `world` channel in their module headers, and each carries its own
  `world_checksum` / `checksum` and its own `adler32`. They will not agree. One of them has
  to become the walker and the other its caller before either can be validated.
* **`adler32` is implemented nine times across compiled modules** — in `ammo`,
  `borders_fog`, `economy`, `groups_guys`, `items`, `movement`, `production`,
  `tech_cities`, and `victory_score`. The checksum primitive is exactly the thing that must
  be singular.
* **Channels 5 and 10 (`deaths`, `items`) now have compiled claimants but no integrated producer.**
  `combat.rs` implements `DeathRing`/`check_deaths`; `items.rs` implements a
  goody-box walker but remains disconnected from runtime item state and replay setup.

### 1.1 Correction: `check_all` does not return the sum

`README-LLM.md` and the lane brief carry: *"EACH channel restarts adler32 at 1 and
`check_all` returns the SUM of the per-channel accumulators."* The first half is right, the
second is wrong, and the wire format explains where the confusion came from.

Traced over the whole 1,614-byte body [measured]: the accumulator is the `CheckSum` object
field at `+0x10` (`[ebp-0x18]`), and it is `mov dword ptr [ebp-0x18], 1` before **every**
channel — sixteen resets, one pre-loop init plus fifteen channels. The running sum is
`edi`: `mov edi,[ebp-0x18]` after channel 1 (`0x00936658`), then `add edi,[ebp-0x18]`
fourteen times, last at `0x00936B49`. But `edi` is stored to `[ebp-0x10]` **only to feed the
final `SyncLogger::logToMemory`**, and the epilogue is:

```
00936b53  mov  dword ptr [ebp - 0x10], edi     ; sum -> sync log only
00936b8c  mov  eax, dword ptr [ebp - 0x18]     ; RETURN = channel 15 (RunTimeEnv)
00936bad  ret
```

**`CheckSums::check_all` returns the fifteenth channel's adler value.** The sum is computed
and logged, never returned.

The sum *is* real and it *does* go on the wire — but from a different function.
`CommandManager::issue_check_sums` `0x00940770` (opcode 57, 65 bytes) runs its own
`CheckSum` object over all fifteen channels and packs the record at `ebp-0x70` [measured]:

| offset | field |
|---|---|
| `+0` | `u8` opcode `0x39` = 57 |
| `+1, +5, +9 … +57` | `u32` × 15, one per channel, in `check_all` order |
| `+61` | `u32` **total** — `add eax, esi` at `0x009409F0`, stored at `[ebp-0x33]` |

`1 + 16×4 = 65`, exactly the recorded wire size, and `CommandPackage::process_check_sums`
`0x009459D0` reads back at `[edi+1]`, `[edi+5]`, `[edi+9]`, … . So **fifteen channels and
sixteen dwords**: that is where "is it 15 or 16?" comes from, and both answers were half
right. Anything reproducing the desync protocol must ship all sixteen.

---

## 2. `Game::do_frame` — the 29 ordered subsystem calls

Order and addresses from `docs/derivation/architecture.md` §3.3. The status column here is
**audited, not self-declared**: `crates/don-sim/src/schedule.rs` carries its own status
field, and where the two disagree this table is the one that was checked against what the
code actually does.

| # | call | VA | status | note |
|--:|---|---|---|---|
| 0 | `AutoSave::restore` | `0x005A20C0` | out of scope | |
| 1 | `GameLog::begin_frame` | `0x00932A70` | out of scope | desync log |
| 2 | `Random::get` (artificial lag) | `0x00A39D70` | out of scope | debug lag injector, **not a sim draw** |
| 3 | `CommandManager::issue_player_speed` | `0x00943100` | out of scope | |
| 4 | `RunTimeEnv::run_script` ×2 | `0x0043D0E0` | module exists, runtime call unverified | structural checksum primitive only; no BHS execution |
| 5 | `ConquestGame::place_reinforcements` | `0x00798880` | out of scope | CTW |
| 6 | `TutorialPromptWin::exec` | `0x007C2810` | out of scope | |
| 7 | `SteamLeaderboards::UploadScore` | `0x00A36190` | out of scope | |
| 8 | **`Leaders::process_all`** | `0x006ED2A0` | module exists, runtime call unverified | `economy.rs` |
| 9 | `NetDaemon::process_all` | `0x00951300` | out of scope | ×5 per tick |
| 10 | AI diplomacy chat | — | out of scope | |
| 11 | **`Leaders::strategy_all`** | `0x006ED430` | **absent** | `Leader::diplomacy` uncited |
| 12 | **`GameDaemon::process_all`** | `0x00732700` | module exists, runtime call unverified | `borders_fog.rs` |
| 13 | `Armies::process_all` | `0x006F3B00` | **absent** | |
| 14 | **`Objects::process_all`** | `0x0065DCE0` | **implemented** | rotation `(frame+i)%10`, `crate::objects` |
| 15 | `Objects::inc_time` | `0x0065DB70` | module exists, runtime call unverified | `ammo.rs` lives here |
| 16 | `GraphicEvents::process` | `0x008E50A0` | out of scope | |
| 17 | `Leaders::end_process_all` | `0x006ED070` | **absent** | |
| 18 | `Achieve::capture_data` | `0x007AF980` | out of scope | |
| 19 | `Leader::process_event_frame` | `0x006EC180` | **absent** | 8 leaders, stride `0x6EEC` |
| 20 | **`Game::frame++`** | `0x005924BF` | **implemented** | after step 14 — rotation uses pre-increment frame |
| 21 | `OrdersMemManager::cycle` | `0x00730E20` | out of scope | |
| 22 | `Roads::scan_and_kill_stray_roads` | `0x008956A0` | **absent** | |
| 23 | `frame % 15 → seconds++` | `0x005924CF` | **implemented** | |
| 24 | `TurnControl::check_cannon_time` | `0x009579E0` | module exists, runtime call unverified | `victory_score.rs` |
| 25 | `SaveGame` / `LoadGame` | `0x005A8220` | out of scope | |
| 26 | `GameLog::end_frame` | `0x009329D0` | out of scope | |
| 27 | `Game::process_end_game` | `0x00591CE0` | module exists, runtime call unverified | `victory_score.rs` |
| 28 | `Scene::process_capture_sequence` | `0x008C13C0` | out of scope | |

**Hand-table tally: 3 implemented, 6 have a module whose runtime path is unverified, 5
absent, 15 out of scope.** This classifies artifacts, not executable call paths.

Of the 14 in-scope steps, three run. And the three that run — `Objects::process_all`'s
rotation, the frame counter, the seconds counter — are the *ordering* of the tick, not its
*content*. There is at present no execution path in the repository that performs the tick
of §3.3: `World::step` (`crates/don-sim/src/world.rs:346`) is three placeholder SIMD
kernels and `self.frame += 1`, and its own doc comment says so.

---

## 3. `Unit::do_job` — the 28-entry jump table

Jump table at `0x00617B94`, indexed directly by `OrderIndex`, reached from `Unit::do_job`
`0x00617A10`. Status audited against `crates/don-sim/src/order.rs` and the movement lane.

| ord | executor | status | ord | executor | status |
|--:|---|---|--:|---|---|
| 0 `NONE` | virtual `do_idle` | implemented | 14 `CAST_SPELL` | `do_cast` | absent |
| 1 `MOVE_TO` | **`do_move` `0x005F7B30`** | implemented | 15 `TRADE_ROUTE` | `do_trade` | absent |
| 2 `ATTACK_TO` | `do_attack_to` | absent | 16 `STRAFE` | `do_strafe` | absent |
| 3 `EXPLORE_TO` | `do_explore_to` | absent | 17 `AIR_PATROL` | `do_air_patrol` | absent |
| 4 `FLEE_TO` | **`do_move` — same arm** | implemented | 18 `CHANGE_FORM` | `do_form_change` | absent |
| 5 `PATROL` | *(default arm)* | **faithfully empty** | 19 `GROUP_MOVE` | `do_group_move` | absent |
| 6 `BUILD_AT` | `do_build` | absent | 20 `GROUP_ATTACK` | `do_group_attack` | absent |
| 7 `GATHER` | `do_gather` | absent | 21 `GROUP_ATTACK_TO` | `do_group_attack_to` | absent |
| 8 `BOARD_SHIP` | `do_board` | absent | 22 `GROUP_PATROL` | `do_patrol` | absent |
| 9 `AWAIT_BOARD` | `do_await_board` | absent | 23 `ATTACK_GROUND` | `do_attack_ground` | absent |
| 10 `ATTACK` | `do_attack` `0x005F1B80` | **partial** | 24 `AIR_ATTACK_GROUND` | `do_air_attack_ground` | absent |
| 11 `FOLLOW` | `do_follow` | absent | 25 `SPECIAL_ANIM` | `do_spec_anim` | absent |
| 12 `GUARD` | `do_guard` | absent | 26 `GARRISON` | `do_garrison` | absent |
| 13 `REPAIR` | `do_repair` | absent | 27 `THINK` | `do_think_order` | absent |

**3 implemented, 1 partial, 1 faithfully empty, 23 absent.**

`ATTACK` is *partial* rather than implemented: the damage arithmetic is the derived
`ObjectData::get_damage` pipeline, but `DamagePredicates` sit at their defaults so only the
spine runs (balance term, ×10 attack, mid-chain armor subtraction, conditional floor of 1);
overkill, flank, entrenchment, height, river and recapture are guarded and unreached. And
the target-selection half of attacking is missing outright: **`Unit::fight` `0x005FD4D0`
(8,157 B) and `Unit::find_attack_pos` `0x00601280` (7,124 B) are uncited by any Rust file.**

`Unit::work` `0x0060D180` (2,885 B) — the order-list driver that sits between
`Unit::process` and `do_job`, and that owns `update_order` / `repath` / `check_target_path`
/ `kill_current_order` — is **also uncited**. So even the three implemented arms have no
retail-derived thing to dispatch them.

### 3.1 Reconciling the rl-env lane's 34 %

`docs/tracks/rl-env.md` reports **34 % of applied actions are `accepted_no_effect`**,
dominated by `GARRISON`, `GUARD`, `FOLLOW`, `REPAIR`, `GATHER`. **That number is consistent
with this table, and it is measuring a different axis — the number is fine, the framing
around it needs one correction.**

The env's action space is the **command** layer, not the order layer:
`crates/don-env/src/generated.rs` defines 33 unit verbs and 16 player verbs, derived from
the 82 `CommandTypes` opcodes. Auditing `apply_unit` / `apply_player` in
`crates/don-env/src/action.rs`, the arms with real dynamics are:

* unit (13 of 33): `MOVE_TO`, `MOVE_NEAR`, `PATROL`, `LAUNCH_PATROL` (all four collapse to
  one move), `ATTACK`, `SIEGE_ATTACK`, `SWARM_AROUND` (all three collapse to one attack),
  `HALT`, `STANCE`, `FORM`, `DISBAND`, `QUEUE_UP`, `BUILD`.
* player (4 of 16): `TREATY`, `DECLARE`, `TRIBUTE`, `RESIGN`.

**17 of 49 verbs = 34.7 % of the verb space carries dynamics; the other 32 fall to the
`_ =>` arm that increments `accepted_no_effect`.** That the *observed* rate of
`accepted_no_effect` is also ~34 % is a coincidence of two different quantities landing on
the same number — the observed rate is share of applied actions under a masked sampler, and
masking suppresses many unimplemented verbs before they are ever emitted (you cannot
`GATHER` with no gatherable in range). The two agreeing is not evidence of anything.

Consistency with §3: every one of the 20 unhandled *unit* verbs maps to an order whose
`do_job` arm is also absent — `GATHER`→`do_gather`, `REPAIR`→`do_repair`,
`GARRISON`→`do_garrison`, `FOLLOW`→`do_follow`, `GUARD`→`do_guard`,
`BOARD_SHIP`→`do_board`, `TRADE`→`do_trade`, `SPELL`→`do_cast`,
`ATTACK_GROUND`→`do_attack_ground`. **The two tables agree exactly**, which is the useful
check: the env is dropping precisely the verbs whose executors are unported, and nothing
else.

One divergence the 34 % hides, worth recording separately because it is *wrong* rather than
*missing*: the env routes four distinct verbs (`MOVE_TO`, `MOVE_NEAR`, `PATROL`,
`LAUNCH_PATROL`) to `OrderIndex::MoveTo`. Retail sends `LAUNCH_PATROL` to an air order and
live patrols to `GROUP_PATROL` (22); `OrderIndex::PATROL` (5) is the dead arm. Those
actions are counted as `applied`, so they are invisible in the 34 % while being less
faithful than the verbs that honestly report no effect.

**Better metric for the RL lane:** `accepted_no_effect` measures the *command* surface.
The order-layer figure is 4 of 28 `do_job` arms. The hand-classified `do_frame` table marks
3 of 14 in-scope entries implemented, but that label does not prove the runnable Rust tick
executes retail-equivalent work. Reporting the distinction stops one number carrying more
weight than it can.

---

## 4. The size of the simulation, as opposed to the binary

`main/game/`: **603 source files, 11,761 distinct code addresses, 5,196,471 code bytes.**

That is smaller than the architecture map's 606 / 12,425 / 5,207,950 and the difference is
worth naming: those are *procedure records*, and MSVC identical-COMDAT-folding gives many
records the same address. Deduplicating by VA (keeping the largest extent) removes 661
folded aliases and three files that contribute nothing but aliases. Both numbers are right;
this ledger uses distinct addresses because a folded alias is not code to port.

Files are labelled by the class-suffix rule of `architecture.md` §1.2 — `XData` = sim
state, `XOut`/`*Win` = presentation, `XType` = rules, plain `X` = sim behaviour — aggregated
to the file by code bytes, with three overrides applied first because they are whole-file
concerns the suffix cannot encode (editor, campaign/scenario, netcode), and with **any file
containing a `::walk_data` forced to `sim`**, since that is the engine's own declaration of
sim-critical state and it outranks the heuristic.

| label | files | funcs | bytes | % | seeded-closure B | named B | named % |
|---|---:|---:|---:|---:|---:|---:|---:|
| **sim** | 335 | 7,192 | 2,926,855 | 56.3 % | 1,746,457 | 485,227 | **27.8 %** |
| presentation | 148 | 2,663 | 1,389,046 | 26.7 % | 508,857 | 2,235 | 0.4 % |
| campaign / scenario | 60 | 974 | 543,262 | 10.5 % | 192,175 | 5,862 | 3.1 % |
| netcode | 34 | 591 | 180,691 | 3.5 % | 30,882 | 1,370 | 4.4 % |
| editor | 26 | 341 | 156,617 | 3.0 % | 59,618 | 675 | 1.1 % |

**So the simulation is ~2.93 MB of x86 across 335 files, of which 1.75 MB enters the seeded
closure.** The whole binary is 47,177 Ghidra functions and ~6.2 MB of first-party `.text`;
the thing we actually have to reimplement is under a third of that.

Two cautions on this table, both real:

* The suffix rule is generous to `sim`. Plain `X` classes are labelled simulation behaviour
  because that is BHG's convention, but `Scene`, `Camera`, `Surf` and `TileSet` are plain
  classes too. The file-name presentation override catches most; the residue inflates `sim`
  by perhaps 5–8 %. The `walk_data` override pushes the other way and is exact.
* The seeded closure follows **direct calls only** — `call`/`jmp rel32` — so virtual
  dispatch is invisible. Seeding every `main/game/` `X::process`, `inc_time`, `work`,
  `walk_data` and `do_*` compensates imperfectly: it includes methods a tick may never call
  while still missing other indirect targets. It is neither a lower nor an upper bound.

The eight heaviest sim files, by reachable bytes and how much is named:

| file | seeded-closure B | named B | named % | `walk_data` |
|---|---:|---:|---:|---:|
| `game/leaders.cpp` | 204,952 | 89,399 | 43.6 % | 7 |
| `game/unit.cpp` | 192,469 | 82,863 | 43.1 % | 1 |
| `game/groups.cpp` | 105,245 | 4,673 | **4.4 %** | 2 |
| `game/constants.cpp` | 90,256 | 90,256 | 100 % | 1 |
| `game/object.cpp` | 52,899 | 30,787 | 58.2 % | 1 |
| `game/options.cpp` | 52,467 | 247 | 0.5 % | 4 |
| `game/game.cpp` | 39,945 | 5,206 | 13.0 % | 1 |
| `game/commandpackage.cpp` | 38,226 | 8,261 | 21.6 % | 1 |

`groups.cpp` is the standout: 105 KB reachable, 4.4 % named, and it holds
`Group::action_move_near` `0x00704990` (9,205 B, 23 call sites) — the single funnel through
which every command becomes a unit order — plus `Groups::process`, which runs at the tail
of `GameDaemon::process_all` every tick.

---

## 5. The finding that dominates the others: derived code lacks proven execution

The generator's old “wired/unwired” split was a manual filename allowlist mistakenly
presented as a call graph. It has been renamed to a source partition and must not be used as
an execution percentage. Its useful observation survives only in this narrower form:

| source inventory | lines | retail procs cited | retail bytes cited |
|---|---:|---:|---:|
| reviewed runtime-core allowlist | generated in `schema/coverage.json` | generated | generated |
| all other `don-sim` sources | generated in `schema/coverage.json` | generated | generated |

The allowlist is `mechanics.rs`, `world.rs`, `simd.rs`, `objects.rs`, `balance.rs`,
`rng.rs`, `batch.rs`, `lib.rs`. It is an inventory convenience, not proof that every line
is reached or that other files are not—`world.rs`, for example, calls `trig.rs`.

The other bucket includes **all fourteen declared `systems/*.rs` modules** — `air`, `ammo`,
`borders_fog`, `combat`, `economy`, `groups_guys`, `map_terrain`, `movement`, `production`,
`tech_cities`, `victory_score`, `walls`, `items`, `naval` — plus `schedule.rs`, `order.rs`,
`interleave.rs`, `container.rs`, `generated/state.rs`. Grepping for
`systems::` outside `crates/don-sim/src/systems/` finds three textual matches: two
`#[deprecated]` strings and one `pub use`. Those are not tick calls. Direct review confirms
at least air and walls have no runtime caller, but a real Rust call/path analysis is still
required before assigning an execution percentage to the whole tree.

Those modules are not mere scaffolding: they carry hundreds of cited retail procedures,
per-field walk reproductions, and hundreds of passing unit tests. Many are isolated; others
may be reached indirectly or through re-exports. Integration work needs targeted path review,
not another grep-derived headline.

`crates/don-sim/src/schedule.rs` is the shape the connection should take: it already holds
`DO_FRAME` as data with a VA and status per step, and a `ScheduleCoverage` counter. Nothing
executes it.

### 5.1 Build state at audit time

The current recovery gates are `cargo test -p don-sim --lib`: **759 passed, 0 failed**, and
`cargo test --workspace --all-targets`: **931 passed, 0 failed**. This includes 49 focused
items tests, 65 focused naval tests, six Rules-channel unit tests, and one Rules corpus gate.

Note what a green suite does and does not say. Local tests exercise modules against their own
derivations, while the runnable `World::step` is only a partial driver and does not establish
retail-equivalent system execution. Retail comparison is the oracle's job; it has 12 cases.

---

## 6. The ten highest-value unimplemented things, ranked

Ranked by (value to a runnable, faithful sim) ÷ (work), not by byte count alone.

1. **Wire the tick.** Execute `schedule::DO_FRAME` and call the declared `systems/*` modules
   from it. This turns reviewed isolated ports into a running simulation; exact integration
   still requires checking prerequisites, order, state ownership, and fidelity gaps.
2. **`Unit::work` `0x0060D180` (2,885 B) + a real `do_job` dispatch.** Uncited today. It is
   the order-list driver — `update_order`, `repath`, `check_target_path`,
   `kill_current_order` — and without it the three implemented arms have nothing dispatching
   them and orders never advance or retire.
3. **`Group::action_move_near` `0x00704990` (9,205 B) and the ~44 other `Group::action_*`.**
   The command→order bridge, 23 call sites, and the reason `groups.cpp` is 4.4 % named
   despite 105 KB reachable. Only the `move_to` leg has ever been traced end to end.
4. **The five runtime-orphan channels — `deaths` (5), `items` (10), `rules` (13),
   `scenario` (14), `script` (15).** Deaths and items have compiled isolated implementations;
   rules now has an exact compiled walker with a live match at `0x12ba3104`, but still lacks
   checked-in builders for all type and Tribe inputs. Scenario and script remain absent. None
   of the five is connected to runtime checksum state.
5. **Consolidate channel 12 and the eight `adler32`s.** `borders_fog` and `map_terrain`
   both claim `world` with incompatible implementations. Until one walker wins, neither
   can be validated, and the checksum primitive must be singular by construction.
6. **`Unit::fight` `0x005FD4D0` (8,157 B) + `Unit::find_attack_pos` `0x00601280`
   (7,124 B).** Both uncited. `get_damage` is the best-tested thing in the project
   (7.99 M trials) and it is fed by target selection that does not exist, so combat cannot
   run even though its arithmetic is solved.
7. **`Balance::type_damage` `0x0057FB50` (8,524 B).** `game/balance.cpp` is 0.5 % named.
   The port reads `final_balance_table` directly; the engine reads it through
   `type_damage` + `compute_modifier` + `return_pack`. Whether those agree is untested, and
   it is the input to the one mechanic we claim to have.
8. **`Leaders::process_all` step 8 second level** — `Leader::gather` → `calc_wall_stats` →
   `calc_unit_stats` → `process_elimination` → `process_taunt`. `economy.rs` exists and is
   isolated; this is the per-player economy and it is step 8 of 29, before anything moves.
9. **A `check_all` equivalent.** One function returning the 15 per-channel values plus the
   sum, in retail's order and wire layout (§1.1). This is what turns every other item into
   something measurable against a real game instead of against our own tests.
10. **`Cities::capture_city` `0x00733380` (7,998 B) and `Army::find_target` `0x006F69B0`
    (7,571 B).** Both uncited, both squarely sim, both gating whole categories of RL
    behaviour (territory flips, army-level aggression) that currently cannot happen.

Deliberately *not* on this list: `Leader::diplomacy` `0x006BC950`. At 20,348 B it is the
largest function in the game and the largest single uncited sim item, but it is AI
behaviour — a strong self-play agent replaces it rather than needing it.

---

## 7. Corrections to existing artifacts

Two sentences each, per the standing rule.

* **`README-LLM.md` / lane briefs — "`check_all` returns the SUM".** It returns the
  fifteenth channel's accumulator; the sum goes to `SyncLogger` and, separately, to the wire
  as a sixteenth dword written by `CommandManager::issue_check_sums`. See §1.1 for the
  instruction-level trace.
* **`docs/derivation/architecture.md` §10 — "`PathFinder::calc_cost` is completely
  unread".** Stale: `crates/don-sim/src/systems/movement.rs` implements `calc_cost`,
  `astar_path_unit`, `valid_ucoord` and `move_step`, with an 860-instruction / 0-float-op
  accounting for `calc_cost` in its header. That open question should be closed and
  re-pointed at whether the port is *right*, which is untested.
* **`crates/don-sim/src/systems/mod.rs` header — "only `ammo` was compiled and tested".**
  Stale in the other direction: fourteen declared modules now compile in the crate suite,
  including the recovered `items` and `naval` modules. The header's real warning — that a
  missing `pub mod` line silently strands a module — remains correct and worth keeping.

---

## 8. What this ledger does not measure

* **Fidelity.** Every "named" byte is a claim. 6 `main/game/` functions are differentially
  tested; the rest are Tier C at best. This document counts *presence*, not *correctness*,
  and the two must never be conflated in a summary.
* **Data coverage.** `ron-data/*.xml`, the 493×493 balance table, the 1,223 rule offsets
  and the 719 constants are all outside the code-byte denominator. `don-rules` covers that
  surface well and it is why `constants.cpp` scores 100 %.
* **Exact execution reachability.** The seeded closure follows direct calls only. Functions
  reached solely through an unseeded vtable slot or function-pointer table are invisible,
  while broad method-name seeds can include code a retail tick never reaches.
* **The `basic/` and `bighuge/` roots.** 6,237 more functions of containers, `String`,
  `Random` and math. Some (`Array<T>` growth, `adler32`, `String::fraction`) are
  sim-critical and are cited; the denominator here is `main/game/` only.
