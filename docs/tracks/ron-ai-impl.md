# Lane report: `ron-ai` — the shipped AI, sized with symbols, and a player that runs

**Deliverables** — all under `crates/don-ai/`, 31 tests green, `cargo test --workspace
--exclude don-net --exclude don-ai` green:

| new file | what |
|---|---|
| `src/rules.rs` | loader for `ron-data/` (`buildingrules`/`unitrules`/`techrules`/`rules`/`citytemplates`). No number is typed in; all come from shipped data. |
| `src/orders.rs` | `Order` / `OrderResult` — **the one mutation seam**. Script and future policy drive the game through the same enum. |
| `src/game.rs` | a headless economic game: cities, buildings with worker slots, citizens, build/train/research queues, ages, population, six-resource economy stepped by **`don-sim`'s derived** `Leader::do_gather`. Implements `ScriptWorld`. |
| `src/scheduler.rs` | `Leader::plan_strategy` + `Leader::production_ai` — the 12-stage cycle, with real names. |
| `src/bin/ai_match.rs` | `cargo run -p don-ai --bin ai-match` — headless AI-vs-AI, plus `--income-probe` and `--sweep-camp-size`. |
| changed | `library.rs` (`assign_idle` + `city_placement` now transcribed), `api.rs` (+3 host fns), `probe.rs`, `lib.rs`, `Cargo.toml` (depends on `don-sim`). |

This continues `docs/tracks/ron-ai.md`; that report's addresses have been re-resolved against
`ron-bin/sbl/rise.pdb`, and where it was wrong or incomplete this says so.

---

## 1. Division of labour — the question, answered with counts

### The one-line answer

**The three shipped `.bhs` scripts are a 2,400-line, one-shot *opening book* that runs as
stage 1 of 12 in the production AI, calls 54 of the engine's 783 host functions, and can
only ever mutate the world through **12** of them. Everything else the AI does is
**306 compiled C++ functions totalling 173,424 bytes of x86 in a single source file,
`main\game\leaders.cpp`.**** `[measured]`

### The compiled side, named

`schema/symbols.json` resolves the whole thing. Every AI function lives in
`e:\agent\_work\2\s\main\game\leaders.cpp` — 405 functions, 252,246 bytes:

| slice | funcs | bytes | note |
|---|---|---|---|
| **core AI** (`Leader`, `LeaderData`, `Personality`, `MakeObject`, `MakeList`) | **306** | **173,424** | the decision code |
| serialization (`*::log_data` / `walk_data`) | 6 | 32,106 | `LeaderData::log_data` alone is 26,650 |
| presentation (`LeaderOut::*`) | 17 | 5,244 | chat, warnings |
| `ConquestLeader::*` (Conquer-the-World metagame, `conquestleader.cpp`) | 78 | 36,740 | separate AI, not skirmish |

The fifteen largest core functions — these are the AI:

| function | VA | bytes | `leaders.cpp` line |
|---|---|---|---|
| `Leader::diplomacy` | `0x006BC950` | 20,348 | 23630 |
| `Leader::gain_tech` | `0x006DCB60` | 15,001 | 7274 |
| `Leader::plan_strategy` | `0x006B9620` | 11,108 | 26880 |
| `Leader::create_buildings` | `0x006C1BE0` | 9,405 | 22015 |
| `Leader::create_units` | `0x006C40A0` | 9,104 | 20413 |
| `Leader::produce_building` | `0x006E1400` | 7,406 | 16555 |
| `Leader::init` | `0x006E3930` | 6,102 | 12566 |
| `Leader::action_respond` | `0x006D03C0` | 3,988 | 11551 |
| `Leader::research_techs` | `0x006C6BA0` | 3,776 | 19390 |
| `Leader::compute_site_stats` | `0x006CD040` | 3,121 | 15129 |
| `Leader::produce_unit` | `0x006CB9E0` | 2,961 | 16103 |
| `Leader::process_taunt` | `0x006B8CC0` | 2,340 | 28050 |
| `Leader::calc_gather` | `0x006CEEE0` | 2,260 | 13897 |
| `Leader::check_orphaned_buildings` | `0x006C9F20` | 2,037 | 17723 |
| `Leader::produce_tech` | `0x006CA980` | 1,947 | 17508 |

**Every `FUN_006Cxxxx` in the previous report now has a name.** The 12-stage cycle is:

| stage | function | VA | bytes |
|---|---|---|---|
| 1 | **the BHS script** | — | — |
| 2 | `Leader::production_ai_setup` then `MakeList::clear` | `0x006C83E0`, `0x006C9DB0` | 1,807 + 210 |
| 3 | `Leader::found_cities` | `0x006C7A60` | 1,708 |
| 4 | `Leader::research_techs` | `0x006C6BA0` | 3,776 |
| 5 | `Leader::upgrade_units` | `0x006C6430` | 1,902 |
| 6, 9 | `Leader::create_units` | `0x006C40A0` | 9,104 |
| 7, 10 | `Leader::create_buildings` | `0x006C1BE0` | 9,405 |
| 8, 11 | `Leader::make_stuff` | `0x006C8AF0` | 1,732 |

driven by `Leader::production_ai` `0x006C1960` (628 bytes) and scheduled by
`Leader::plan_strategy` `0x006B9620`. The stage counter is
`LeaderData::production_step` at `+0x788` — the field names are in the PDB too, so the
earlier offset-only account can be retired:

| offset | field | role |
|---|---|---|
| `+0x000` / `+0x004` | `leader_flags` / `leader_flags2` | human bit / AI-subsystem disable bits |
| `+0x008` / `+0x00C` | `who` / `tribe` | 0-based index / nation id |
| `+0x050` | `multi_diff` | per-leader difficulty |
| `+0x788` | `production_step` | the 12-stage counter |
| `+0x78C` | `prod_script_run` | "the script still has work" |
| `+0x790` | `script_step` | the script's `ref int step` |
| `+0x9E0` | `effective_pop` | `queued_units() + control + 1` |
| `+0x6DD4` | `pers` | `Personality`, 24 ints |
| `+0x6EA4` | `prod_script` | the script name (`"economic"`) |
| `+0x6EC8` | `make_list` | `MakeList`, the AI's want-list |

### The script side, sized against the API

`crates/don-ai/data/script-functions.json` has **783 distinct host-function names**
(842 registrations, 35 names being arity overloads). Scanning the three shipped scripts
against that table `[measured]`:

* **54 distinct host functions used**, 809 call sites.
* **12 of the 54 mutate anything**: `place_building_with_cost`,
  `place_orphan_building_with_cost`, `place_building_upgrade_with_cost`,
  `place_city_with_cost`, `train_unit_with_cost`, `train_unit_at_with_cost`,
  `research_tech_with_cost`, `destroy_building`, `citizen_repair_order`,
  `unit_move_order`, `set_timer`, `stop_timer`.
* The other 42 are queries. The most-called call in all 2,400 lines is
  `get_is_no_nation_powers()` (87 sites) — a nation-power on/off check.

So the script's entire authority is: *place these buildings, train these units, research
these techs, in this order, in these cities.* No unit ever moves except the one
`unit_move_order` sweep in `assign_idle`; nothing is ever attacked; no army is ever
composed.

### Three facts that make the split sharper than "one stage of twelve"

**(a) `SCRIPT_DONE` is a one-way latch — the script is a *one-shot opening*.**
`[measured]` A per-function disassembly scan of every reference to `LeaderData
+0x78C` across all 22,750 functions finds exactly four sites:

```
Leader::init          0x006E4966   mov [ebx+0x78C], ecx
Leader::init          0x006E4CC7   mov [ebx+0x78C], 1
Leader::production_ai 0x006C19B1   cmp [edi+0x78C], 0
Leader::production_ai 0x006C1A9F   mov [ebx+0x78C], 0
```

plus one further *read* in `Leader::gain_tech` `0x006DED9B`. Set to 1 once at
`Leader::init`, cleared once on `SCRIPT_DONE`, and **nothing anywhere re-arms it**. The
first `SCRIPT_DONE` — whether the 36-step build order finished or the script's own
300-second hang watchdog fired — retires the BHS layer for the rest of the match. This
corrects the earlier report's "until something sets that flag again": nothing does.

**(b) While the script *is* alive it starves the compiled pipeline.**
`BLOCK_ON_THIS` resets `production_step` to 0, so stages 2..11 never run that cycle — and
`economic.bhs` sets `return_value = BLOCK_ON_THIS` in essentially every case of its
36-step machine. Measured in our runner: a player whose script never returns `SCRIPT_DONE`
executes the ten compiled stages **zero** times in 30 minutes (P2/P3 in §4), while a
player whose script retired executes them 123 times. The two layers are near-exclusive in
time, not cooperative.

**(c) Age advance is not in the production cycle at all.** `Leader::plan_strategy` has a
second, independent path that runs every 30 ticks of the player's phase and is *not*
vetoed by `BLOCK_ON_THIS` `[measured, 0x006B969E..0x006B96ED]`:

```asm
mov  eax,[ebx+0x6ED8]      ; make_list data ptr (LeaderData::make_list +0x10)
mov  esi,[eax]             ; head entry
lea  eax,[esi-0x220] ; cmp eax,0x54 ; ja out     ; is it a tech index in [0x220,0x274]?
push 0 ; call 0x6C9B90     ; Leader::can_pay
call 0x6E0C80              ; LeaderData::has_tech(tech)      -> skip if held
push -1 ; call 0x6DB510    ; LeaderData::researching(tech,…) -> skip if in progress
call 0x6C94F0              ; Leader::make_this               -> queue it
```

That is a fast lane off the head of `LeaderData::make_list`, and it is how an AI whose
script is blocked forever still ages up.

### Verdict

Replicating "the RoN AI" is **not** transcribing 2,400 lines of BHS. The scripts are one
stage of twelve, in one of four independently switchable subsystems (production / combat /
unit / city — only production has any script surface), they hold the wheel only until they
say `SCRIPT_DONE` once, and they can only issue 12 kinds of command. The 2,400 lines are
now fully transcribed and executable; the 173,424 bytes are not, and that is where the
opponent lives.

---

## 2. Difficulty scaling and the cheat, quantified

### Where difficulty lives `[measured]`

| function | VA | behaviour |
|---|---|---|
| `LeaderData::get_diff` | `0x006EC000` | effective difficulty: per-leader `multi_diff` (`+0x50`) if the game flags allow, else the global byte `Game+0x2B` |
| `LeaderData::get_gather_handicap` | `0x006D66A0` | the income bonus table below |
| `LeaderData::get_handicap` | `0x006DA740` | the separate `handicaps` ladder, `5*N`, **not AI-only** |
| `Leader::do_gather` | `0x006CE450` | applies the bonus, once per frame per resource |

The prior lane's `FUN_006D66A0` = `LeaderData::get_gather_handicap`, confirmed by name.
The table stands:

| difficulty | stored | bonus |
|---|---|---|
| Easiest | 0 | **−35 %** |
| Easy | 1 | **−15 %** |
| Moderate | 2 | **−7 %** |
| Tough | 3 | **0 %** |
| Tougher | 4 | **+25 %** |
| Toughest | 5 | **+50 %** |

Humans get 0 unless the no-rush game option is on. The bonus is applied *after* the
displayed gather rate is cached, so it never shows in the AI's own economy readout.

### Measured, end to end, in our runner

`cargo run --release -p don-ai --bin ai-match -- --minutes 30 --income-probe --fast-economy`
runs six identical starting towns with **no AI at all**, so the only difference between rows
is `get_gather_handicap`:

```
difficulty,bonus_pct,food,timber,food_ratio,timber_ratio,nominal_ratio
Easiest, -35, 1439, 360, 0.6485, 0.6000, 0.6500
Easy,    -15, 1859, 480, 0.8378, 0.8000, 0.8500
Moderate, -7, 2039, 540, 0.9189, 0.9000, 0.9300
Tough,     0, 2219, 600, 1.0000, 1.0000, 1.0000
Tougher,  25, 2759, 720, 1.2434, 1.2000, 1.2500
Toughest, 50, 3299, 900, 1.4867, 1.5000, 1.5000
```

Two things worth naming:

1. **Toughest gathers 1.49× what Tough gathers; Easiest gathers 0.65×.** The spread
   between the two ends is **2.29×** on the same buildings and the same worker count.
   That is a large, flat economic cheat — not a smarter opponent, a richer one.
2. **The truncation is visible and matters at small incomes.** The engine computes
   `income = (pct + 100) * income / 100` with C truncation. On the timber column, whose
   income is a single-digit number, Easiest lands on 0.60× rather than the nominal 0.65×
   and Easy on 0.80× rather than 0.85× — the low difficulties are punished slightly harder
   than the table reads. This is exactly the sort of thing hand-computing an expected value
   would have hidden; it fell out of running the derived code.

### The other handicap, and what it is not

`ron-data/rules.xml` `<CATEGORIES id="handicaps">` is a separate `5*N` ladder indexed by
`LeaderData +0x4C`, read by `LeaderData::get_handicap` `0x006DA740` and applied to *costs*
(`ObjectData::train_time` `0x006508C0`: `cost = (200 - h) * cost / 200`). It is available to
humans as well and is a lobby setting, not an AI cheat. `get_handicap` additionally shifts
the index by difficulty when `Game[0x820]&4` is clear — Easiest gives the **human** +10
steps, Easy +5; Tougher gives the **AI** +10, Toughest +16. So at the extremes the AI is
getting a cost discount *and* an income bonus.

### Personality — the per-leader knobs, now typed

`LeaderData::pers` at `+0x6DD4` is a `Personality`: **24 `int`s, 96 bytes**
(`schema/types.json`), each a small signed axis:

```
rush cities upgrades arms army army_size raid invade target strategy raze spells
forts nukes air naval market scouts civilians early_army
friendly_human alliance_human friendly_ai alliance_ai
```

`Leader::random_personality` `0x006CFD00` rolls them at game start, almost all as
`random(0,0xFFFF) % 3 - 1`, i.e. uniform on `{-1, 0, +1}`. Exceptions read straight off the
decompilation `[Tier C]`: `arms` gets a wider `-2..+2` on a coin flip; `early_army` and
`raze` are forced to 0; `friendly_ai` copies `friendly_human` and `alliance_ai` copies
`alliance_human`; `upgrades` is **never assigned** by this function; and several axes are
forced by nation (`forts = 1` for tribe 6, `air = 1` for tribe 0xC, `scouts = 1` for tribe
9, `naval` re-rolled to `0..2` for tribes 9/0xB/0xF, `raid` forced to 1 for tribes 0/0x11).
`rush` also gets nation-specific and map-size-specific nudges.

**`boom_vs_rush` is `Personality::rush`.** `Leader::production_ai` passes
`player[0x6DD4] + 2` as the script's third parameter, and `+0x6DD4` is exactly
`pers.rush`. That closes the earlier report's open question about that channel — and
`economic.bhs` still never reads it.

---

## 3. What was built, and what it does

### The order interface

`orders::Order` has ten variants, one per mutating host function, each carrying the
implementation VA. `Game::submit` is the only thing that mutates the world.
`OrderResult` keeps the engine's tri-state (`>0` ok / `0` refused-but-affordable /
`-1` invalid) because the scripts branch on all three — `place_dock` documents the
tri-state in its own comment. A future policy plugs in at exactly this seam; nothing in
the game is reachable any other way.

### The game

`game::Game` + `game::PlayerView` (which implements `ScriptWorld`). Rules come from
`ron-data/` at load time — building/unit/tech costs (`COST` × the `*_COST_FACTOR` of 10),
`JOB_TIME`, prerequisites, `WHERE`, `POP`, `SUPPORT`, `BUILD_FLAGS`, `CITY_GATHER`,
`PEASANT_RATE`, `GATHER_RATE`, `POP_CAP`, `COMMERCE_CAP`, `STARTING_GOODS`, and the
starting town from `citytemplates.xml`. The economy is **`don-sim`'s derived**
`resource_tick` / `credit_resource` / `commerce_cap` — not a re-derivation.

Two shipped-data facts fell out and are worth recording elsewhere:

* **Resource slots 4 and 5 are Metal and Oil.** `don-sim` pins 0..3 from the binary and
  explicitly declines to name 4 and 5. `resourcerules.xml` lists `Food, Timber, Wealth,
  Knowledge, Metal, Oil` in that order with abbreviations `F T G K M O`, and
  `rules.xml`'s `CITY_GATHER` is `entry0="10food" entry1="10timb" entry2="0gd"
  entry3="0know" entry4="0met" entry5="0oil"`. Shipped data, same ordering twice.
* **All 17 `citytemplates.xml` templates are `size="small"` and every one is 3 Farms +
  1 Library.** That is the standard start, not a guess.

### The model boundary (there is no map)

Stated in full in `crates/don-ai/src/game.rs`; six numbered `DEVIATION`s, each marked at
its site. The load-bearing ones:

* Placement never fails for spatial reasons, so the scripts' fallback ladders are dead code.
* `BuildData::gather_max` (`+0x80`, the byte `max_workers_at_building` `0x009F2520`
  returns) is terrain-derived in retail; here it is a per-type constant. Farm = 1 is
  implied by the shipped script's own arithmetic (`train_unit_with_need` treats a Farm as
  needing a worker only when it has zero); Woodcutter's Camp = 5 is the script's own
  `min_size` target. Mine and University are ours.
* Which resource a gatherer yields is a per-type constant, not the tiles beneath it.
* The ten compiled stages are named no-op stubs that count their invocations.

### One open calibration number, flagged not hidden

`don-sim`'s `resource_period` is `GATHER_RATE << 4` = 7200 (`0x006CE7B9`), and income is
clamped against `COMMERCE_CAP`, whose age-0 value is the raw `70`. Taken together a player
pinned at the age-0 commerce cap gathers **4.4 resources per 30 s = 8.75/min**, which is
far slower than the shipped game plays. `don-sim`'s own doc comment says the producers of
`gross` are not derived, so the missing factor may be there rather than in the shift. It is
exposed as `ModelParams::gather_period_shift` (default `4`, the literal derived value;
`--fast-economy` sets `0`) and **neither setting is verified**. All §2 and §4 numbers use
`0`, because at `4` nothing interesting happens inside an hour of game time.

---

## 4. Running it: what actually happens in an AI-vs-AI match

```sh
cargo run --release -p don-ai --bin ai-match -- \
  --minutes 30 --players Romans,Greeks,Bantu,Koreans --trigger-model --fast-economy
```

30 game-minutes, 4 players, Tough, in ~1 s wall clock. Verbatim:

| | script step | script runs | blocked | done | compiled stages run | orders ok/refused/invalid | cities | citizens | techs |
|---|---|---|---|---|---|---|---|---|---|
| Romans | 18 | 13 | 12 | **1** | **123 each** | 18 / 49 / 0 | 2 | 11 | Barter, City State, Written Word |
| Greeks | 20 | 136 | 136 | 0 | **0 each** | 16 / 29 / 657 | 2 | 11 | Barter, City State |
| Bantu | 17 | 136 | 136 | 0 | **0 each** | 16 / 15 / 9 | **6** | 10 | Barter, City State |
| Koreans | 18 | 12 | 11 | **1** | 125 each | 17 / 48 / 0 | 2 | 11 | Barter, City State, Written Word |

Matches are deterministic (there is a test: two identical runs produce identical state).

**Four things this measured that reading the script would not have told you:**

1. **The compiled/script exclusion is total.** Greeks and Bantu, whose scripts never
   returned `SCRIPT_DONE`, ran the ten compiled stages **zero** times in 30 minutes.
   Romans and Koreans, whose scripts retired, ran them 123–125 times. `BLOCK_ON_THIS` is
   not a hint; it is a hard veto, and `economic.bhs` asserts it on almost every path.

2. **The nation branches really diverge.** Bantu took the `step = 10` self-loop and founded
   **six** cities; Greeks diverted to step 18/20 and never researched Written Word. Same
   script, same start, three different games. The per-nation branches are the interesting
   content of `economic.bhs` and they are load-bearing.

3. **`economic.bhs` step 20 can spin forever with its own watchdog disarmed.** The Greeks'
   657 invalid orders are all one call: step 20 places a University, whose `PREQ0` is
   `Classical Age`, before holding it. The engine returns `-1` for a failed prerequisite;
   the script tests `== 0` (refused-but-affordable) and so never bails — and it sets
   `old_step = 0`, which is precisely the signal that stops the 300-second hang watchdog
   from arming. In retail this is survivable only because of the every-30-ticks age-tech
   fast lane in `Leader::plan_strategy` (§1c), which is outside the production cycle. With
   that lane stubbed, the Greeks are stuck. **This is a real shipped-script fragility and
   it is not in the previous report's bug list.**

4. **`city_placement` gates the entire opening.** `economic.bhs` step 10 advances only on
   `city_placement(who) > 0`, and the shipped body returns `-1` and arms two `trigger`
   bodies whose scheduling is *still* not derived. With the faithful stub, every player
   parks on step 10 and 300 script-seconds later the hang watchdog retires the script —
   permanently, per §1a. Steps 11..36 are unreachable. There is a test asserting exactly
   this (`city_placement_is_the_gate_on_the_whole_opening`): step 10 without the model,
   past 10 with it. **Recovering the BHS trigger mechanism is the single highest-value
   remaining item in this lane.**

`--trigger-model` supplies one documented, **unverified** reading of the trigger pair as a
per-player state machine. It is off by default. Per-player is itself a guess: BHS `static`s
are per script *function*, which is exactly why `economic.bhs` hand-rolls a per-player
array, so shared trigger state is equally plausible and would serialise all eight players'
city founding.

### Sensitivity of the biggest model number

`--sweep-camp-size` (20 min, one player):

```
camp_size  citizens  food  timber  buildings  orders_ok
    1         7      2212    725       10        19
    2         8      2171   1081       10        20
    3         9      2118   1438       10        21
    4        10      2125   1828       10        20
    5        11      2466   2426       10        18
    6        12      2410   2426       10        19
    7        14      2438   2426       10        21
    8        14      2300   2426       10        21
```

Citizen count tracks camp capacity almost 1:1 — the script trains exactly enough citizens
to fill the slots it can see, which is the clearest confirmation that
`max_workers_at_building` is the script's main economic input. Timber saturates at camp
size 5, where the age-0 `COMMERCE_CAP` of 70 binds (2 cities × `CITY_GATHER` 10 + 5 workers
× `PEASANT_RATE` 10 = 70). The shipped script's `min_size = 5` target lands exactly on the
commerce cap. That is either a nice piece of design or a nice coincidence; it is a
measurement either way.

---

## 5. Corrections and additions to `docs/tracks/ron-ai.md`

1. **`assign_idle` is not "not transcribed" for lack of a body.** The earlier pass saw only
   the forward declaration at `aibestbuildlibrary.bhs:4`; the body is at line 310. It is now
   transcribed, including three behaviours worth naming: the camp loop `return 4`s *before
   assigning anyone*, `been_here` is a plain local so its `> 1` test is dead, and `wood_camp`
   stays 0 when the player owns no camps so the trailing sweep addresses object id 0.
2. **`prod_script_run` is never re-armed** (§1a). The earlier "until something sets that
   flag again" is wrong; four write/read sites in the whole binary, and none re-arm.
3. **`boom_vs_rush` = `Personality::rush`** (§2). The channel is typed, not mysterious —
   it is still dead in `economic.bhs`, which never reads the parameter.
4. **The ten unnamed stages are named** (§1). So is the scheduler, so are the eleven
   `LeaderData` fields the driver touches.
5. **`Fishermen` is plural in `unitrules.xml`.** The shipped `"Citizens"` typo (5 sites) is
   still a typo, but the type-name plural is not uniform, which makes it plausible enough to
   have shipped. Note also that the maintenance call at `economic.bhs:297` uses the correct
   singular `"Citizen"` with the *same* `needed_citizens` the buggy steps set, so the bug is
   partly masked — the next script invocation trains what the previous one asked for.
6. **New**: the every-30-ticks age-tech fast lane in `Leader::plan_strategy` (§1c), and the
   step-20 watchdog-disarming spin (§4.3).

## 6. What is still not established

1. **BHS `trigger` / `enable_trigger` semantics** — now demonstrably the gate on the whole
   opening (§4.4), and the top of the list.
2. **The ten compiled stages' bodies.** 173,424 bytes, named but unread. `Leader::diplomacy`
   (20,348 bytes) and `Leader::gain_tech` (15,001) are the two biggest single functions in
   the AI and neither is touched.
3. **BHS integer truthiness.** Still routed through `api::bhs_true` (`!= 0`), still
   unverified, and still load-bearing wherever a `-1` sentinel is tested bare.
4. **The `gross` income scale** (§3) — the one number that makes wall-clock economy right.
5. **Combat / unit / city AI.** Untouched. One incidental measurement from the earlier
   pass stands: each unit re-evaluates every 32 ticks, and several tactical branches are
   gated on effective difficulty `> 1`, so Moderate and above unlock behaviours Easiest and
   Easy do not have. Not quantified.
6. **How a player is assigned a production script.** `defensive.bhs` is never named in the
   executable and we still do not know what selects it.

## 7. For the RL lane

* **There is now a real environment.** `Game` + `Order` + `AiSet` is a deterministic,
  headless, order-driven economic game with the shipped opponent already in it. A 30-minute
  4-player match runs in about a second.
* **The baseline is weak on Tough and beatable without cheating back.** At Tough the AI
  gets a 0 % income bonus; the economic cheat only bites at Tougher/Toughest (1.24× /
  1.49×). An agent that matches the opening and plays better tactically needs no economy edge.
* **Behaviour cloning off the script buys an opening book and nothing more**, and a *short*
  one: the script retires permanently on its first `SCRIPT_DONE` (§1a). Cloning the whole
  opponent means cloning `leaders.cpp`.
* **Curriculum value is real and cheap**: 36 named build-order milestones, per-nation
  branches that genuinely diverge, and `script_step` as a free shaped-reward signal.

## 8. Reproduction

```sh
cargo test -p don-ai                                   # 31 tests
cargo run --release -p don-ai --bin ai-match -- --minutes 30 \
  --players Romans,Greeks,Bantu,Koreans --trigger-model --fast-economy
cargo run --release -p don-ai --bin ai-match -- --minutes 30 --income-probe --fast-economy
cargo run --release -p don-ai --bin ai-match -- --minutes 20 --sweep-camp-size --fast-economy
cargo run --release -p don-ai --bin ai-match -- --minutes 10 --players Romans --timeline

python3 tools/pdb/lookup.py 6b9620 6c1960 6c83e0 6c7a60 6c6ba0 6c6430 6c40a0 6c1be0 6c8af0
```

The `+0x78C` latch scan and the compiled-AI inventory are both one pass over
`schema/symbols.json`; the disassembly must be done **per function** from the symbol table,
because a linear sweep of `.text` derails after ~300 K instructions and silently reports
zero hits.

**Fidelity: Tier C throughout.** Transcription of shipped script source, shipped data files,
and disassembled/decompiled engine code, plus `don-sim`'s derived economy. Nothing in this
lane is differentially tested against retail and nothing is verified.
