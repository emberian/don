# Victory, score, diplomacy and match lifecycle

Lane `mech:victory-score`. Implementation:
`crates/don-sim/src/systems/victory_score.rs` (1,700 lines, 27 tests, all green).

Everything below is `[measured]` against `ron-bin/riseofnations.exe` +
`ron-bin/sbl/rise.pdb` on this Mac unless it says otherwise. No community
documentation was consulted; where a number came from `ron-data/*.xml` the file and
line are named.

---

## 1. What now works

| capability | state |
|---|---|
| **The score formula, exactly** — all 11 components, their divisors, clamps and rounding | complete |
| `TypeData::get_score_value`, both forms (plain + research-premium) | complete |
| The score **refresh throttle** (a 10-frame round robin phased by player slot) | complete |
| `get_team_score`, `get_mvp_score`, `get_team_terr`, `get_team_economic` | complete |
| Armageddon clock: threshold formula, score-zeroing, `Game::defeat_all` | complete |
| Diplomacy: `get_diplo` / `is_ally` / `is_enemy` / `is_peace`, mutual-minimum semantics | complete |
| Victory conditions: wonder, territory, score, time limit, economic, conquest/last-alliance | complete state machine; completed-Wonder value/net supply is tick-wired behind the mandatory `WonderWorld` boundary |
| Musical chairs cull | implemented, **weakest** part — see §8 |
| Elimination: capital-loss timer (`ELIMINATION_CAPITAL`) | complete |
| `Leader::victory` / `Leader::defeat` state transitions, queue cleanup, ally propagation, terminal `check_victory` | complete |
| Map-scaled timers (`wonder_timer`, `popwin_timer`, `retake_capital`) | complete |
| Checksum-channel byte emitters + `adler32` | complete |
| Tech-race victory (`VICTORY_TECH_RACE`, `VICTORY_BY_TECH_RACE`) | exact predicates, terminal transaction, typed notice, and same-step concrete queue cleanup are production-wired — see §8 |

**How it was measured.** Structure came from `re/decomp-all/<EA>.c` and was then
re-read at the instruction level with capstone for every arithmetic step — every
divisor in this document is a reciprocal-multiply sequence I decoded by hand, not a
Ghidra `/`. Field names, offsets and enum values are from the PDB type stream
(`schema/pdb-types.json`, `schema/types.json`). Rule values are from
`docs/derivation/rules-constants.json`, which records the parser and scale each
constant was loaded with.

**Fidelity tier: C.** Behaviourally faithful, derived from the instruction stream,
**not** differentially tested against retail. Nothing here has been through the
oracle. §9 says exactly what a Tier-B run would need.

---

## 2. The score formula

### 2.1 Where it lives

`Leader::compute_score(int force)` @ **`0x006EC560`**, `leaders.cpp`. Two callers:

* `Leaders::strategy_all` @ `0x006ED45B`, `force = 0`, once per **active** leader per
  sim frame (step 11 of `Game::do_frame`).
* `Game::defeat_all` @ `0x00592ADB`, `force = 1`, on armageddon.

The result is `LeaderData::score` at **`+0x18`**. That is the number the end-of-match
screen shows — the observed **766** is this field.

### 2.2 The eleven components

`LeaderData` `+0x18 .. +0x44`, names verbatim from the PDB, all `int`.
`Leader::reset_score` @ `0x006E37F0` zeroes exactly this block.

| off | field | written by | in the total? |
|---|---|---|---|
| `+0x18` | `score` | the sum below | — |
| `+0x1C` | `score_explored` | `compute_explore_score` `0x006BC5E0` — **a stub, writes 0** | yes (always 0) |
| `+0x20` | `score_territory` | `compute_score` inline | **yes** |
| `+0x24` | `score_units` | `compute_unit_score` `0x006BC500` | **yes** |
| `+0x28` | `score_units_2` | `compute_unit_score` | **NO** |
| `+0x2C` | `score_buildings` | `compute_build_score` `0x006BC3F0` | **yes** |
| `+0x30` | `score_economy` | `compute_economy_score` `0x006BC270` | **yes** |
| `+0x34` | `score_pop` | `compute_pop_score` `0x006BC190` — **a stub, writes 0** | yes (always 0) |
| `+0x38` | `score_unit_upgrades` | `compute_unit_upgrades_score` `0x006BC1A0` | **yes** |
| `+0x3C` | `score_research` | `compute_research_score` `0x006BC360` | **yes** |
| `+0x40` | `score_wonders` | `compute_build_score` (not `compute_wonder_score`) | **yes** |
| `+0x44` | `score_combat` | **nothing in the compute path** | yes (always 0 there) |

Four of the eleven are dead in retail, and this is not inference — it is what the
functions contain:

```
006bc190  mov dword [ecx+0x34], 0 ; ret     Leader::compute_pop_score
006bc250  ret                               Leader::compute_combat_score   (empty)
006bc260  ret                               Leader::compute_wonder_score   (empty)
006bc5e0  mov dword [ecx+0x1c], 0 ; ret     Leader::compute_explore_score
```

`compute_wonder_score` being empty is a trap: wonders **are** scored, just by
`compute_build_score`, which writes both `score_buildings` and `score_wonders`.

`score_units_2` is the military mirror of `score_units` (the subset with non-zero
`ObjectTypeData::attack`) and is **excluded from the total** — the sum at
`0x006EC66D` has ten terms and `+0x28` is not one of them. It exists for the AI /
stat window. Anything that adds it to the reward double-counts military units.

### 2.3 The total

Verbatim from `0x006EC66D`–`0x006EC68A`, in that addition order:

```
score = score_combat + score_wonders + score_research + score_unit_upgrades
      + score_economy + score_buildings + score_units + score_territory
      + score_pop + score_explored
```

With the four dead components substituted, the live formula is

```
score = score_units + score_buildings + score_wonders
      + score_research + score_unit_upgrades
      + score_economy + score_territory
```

### 2.4 The per-type value: `TypeData::get_score_value` @ `0x006673B0`

```
get_score_value(type t, int param):
    i2 = t
    if param == 0 and t in {0x42, 0x43, 0x44}: i2 = 0x32     # alternate citizens -> base citizen
    cost = sum(types[i2].costs[0..6])                        # TypeData::costs, +0x18
    if is_unit_type(t):                                      # TypeIndex 0x32..0x19D
        if param == 0: return unit_cost_factor * cost
        v = (research_premium * unit_cost_factor * cost) >>> 8
        return (v * unittypes[i2].research_premium_cost) >>> 8
    if is_build_type(t):  return build_cost_factor * cost     # 0x19E..0x21E
    if is_tech_type(t):   return tech_cost_factor  * cost     # 0x220..0x274
    if is_spell_type(t):  return spell_cost_factor * cost     # 0x275..0x2AB
    return tech_cost_factor * cost                            # default arm
```

`>>>` is `(v + (v>>31 & 0xff)) >> 8` — division by 256 truncating toward zero, the
8.8 fixed-point rescale for `research_premium` ("1/1", parsed at scale 256 → 256).

**`get_score_value(t, 0)` is the type's real total resource cost.** `TypeData::costs`
holds the raw `ron-data` numbers, and `*_cost_factor = 10` is the ×10 that
`unitrules.xml`'s own header comment describes (*"COST = Base cost of unit (multiply
by 10)"*, `ron-data/unitrules.xml:61`). The clincher is `Type::sum_rules_cost`
@ `0x006680E0` — dead code, zero callers, but a byte-for-byte twin of the
`param == 0` path, and its *name* states the semantics. Score value = rules cost.

`is_wonder_type` @ `0x00470780` is the range test `0x20E <= TypeIndex < 0x21F`
(526..542) devirtualised inline at `0x006BC4A9`.

⚠ Note the boundary: buildings are `< 0x21F` and techs are `> 0x21F`, so `TypeIndex
0x21F` falls through both range tests into the default arm.

### 2.5 `score_units` — `Leader::compute_unit_score` @ `0x006BC500`

```
score_units = 0; score_units_2 = 0
if game.armageddon >= get_armageddon(): return
for t in 50 ..= 401:                                   # loop bound cmp esi,0x192
    n = num_queued[t] + num_units[t-50]                # both u16, zero-extended
    if n == 0: continue
    base = get_score_value(t, 0)
    step = unittypes[t].support_cost[1] + support_cost[0]     # ObjectTypeData +0x270
    v    = (base*n + half_trunc((n-1) * step * n)) / 5
    score_units += v
    if unittypes[t].attack != 0: score_units_2 += v    # ObjectTypeData +0x1E8
```

`half_trunc` is `cdq; sub eax,edx; sar eax,1` — divide by 2 **truncating toward
zero**, not an arithmetic shift; they differ for negatives. The `/5` is
`0x66666667; imul; sar edx,1`.

The `(n-1)*step*n / 2` term is the **integral of the ramping support cost**: the
score of `n` units includes what the ramp made them cost, so a big army is worth
super-linearly more than `n` × the first one.

Worked example on shipped data — `Citizen` is `COST 2f`, `SUPPORT 1f`:
`base = 10*2 = 20`, `step = 1`. One citizen scores `20/5 = 4`; twenty score
`(400 + 190)/5 = 118`. This is the only magnitude anchor in the lane fed by real
rules values, and it is what makes an end-of-match **766 plausible**: score is
recomputed continuously, so a conquered player's units and buildings are already
gone when the number is frozen, leaving mostly territory, economy and research.

### 2.6 `score_buildings` and `score_wonders` — `compute_build_score` @ `0x006BC3F0`

```
score_buildings = 0; score_wonders = 0
if armageddon: return
for t in 414 ..= 542:                                  # cmp edi,0x21f
    n = num_queued[t] + num_buildings[t-414]
    if n == 0: continue
    base = get_score_value(t, 0)
    step = (buildtypes[t].support_cost[1] + support_cost[0]) * build_support_factor
    v    = base*n + half_trunc((n-1) * step * n)
    if 0x20E <= t < 0x21F: score_wonders   += v / 3     # 0x55555556 imul
    else:                  score_buildings += v / 5
```

Same shape as units, one extra multiply by `build_support_factor` (= 1 in retail),
and the split at the end. **A wonder is worth 5/3 of an equally costly building.**

### 2.7 `score_economy` — `compute_economy_score` @ `0x006BC270`

```
score_economy = 0
if armageddon: return
for i in 0..6:                                          # the six primary resources
    if not type_avail(i, 1): continue
    a = bucket[i] - game.starting[i]                    # LeaderDataEncrypt +0x00, Game +0x600
    v = a / 20;   clamp to [0, 100]
    score_economy += v
    w = income[i] / 80                                  # LeaderDataEncrypt +0x94
    if w > 200: w = 200                                 # cap only, NO floor
    score_economy += w
score_economy += popcount(rare) * 25                    # LeaderData::rare, BitMask<44> +0x6D98
```

Two asymmetries that matter for a reward signal:

* the **stockpile** term is clamped on both sides, so hoarding past `starting + 2000`
  of a resource is worth nothing;
* the **income** term is capped at 200 but has **no lower clamp** — a negative net
  income genuinely *subtracts* score, down to −∞ in principle.

Per-resource ceiling is therefore `100 + 200 = 300`, so the six resources cap at
1,800, plus 25 per rare resource controlled (44 rares → up to 1,100).

`LeaderDataEncrypt` stores its ints XOR-obfuscated: `bucket[i] ^ 0x8221`,
`income[i] ^ 0x90236` (the XORs at `0x006BC2C1` / `0x006BC305`). That is a **storage
detail, not arithmetic** — the XOR decodes to the real amount. I initially modelled
it as part of the formula and the tests caught it: every leader picked up a phantom
1,800 economy points from zeroed state. `decrypt_field` in the module exists for
anyone populating these from a live read.

### 2.8 `score_research` and `score_unit_upgrades`

```
compute_research_score @ 0x006BC360:
    for t in 544 ..= 628:
        if not (has_tech(t) or researching(t,-1,0,0)): continue
        if tech_at_start.test(t): continue                 # BitMask<806> +0x6C80, bits at +0x6C8C
        score_research += get_score_value(t, 0) / 5

compute_unit_upgrades_score @ 0x006BC1A0:
    for t in 50 ..= 401:
        if not (has_tech(t) or researching(t,-1,0,1)): continue
        if tech_at_start.test(t): continue
        if unittypes[t].unit_flags & UNITTYPE_NO_RESEARCH (0x80): continue
        score_unit_upgrades += get_score_value(t, 1) / 5    # NOTE: param 1
```

Both credit techs **in progress**, not just completed ones, and both subtract what
the player started with — a "Start Age: Gunpowder" game does not hand out free
research score. `tech_at_start` is a `BitMask<806>` whose 12-byte header (`bits`,
`size`, `flags`) puts the bit array at `+0x6C8C`, which is why the code indexes
`this + 0x6C8C + (t>>3)`.

### 2.9 `score_territory`

Computed inline in `compute_score` at `0x006EC5E7`:

```
score_territory = (leaders[who].territory * 1000) / world.land_size
```

`LeaderData::territory` is `+0x9D8`; `World::land_size` is `+0x78`. So it is
**per-mille of the world's land**, 0..1000, and it is recomputed on every refresh
regardless of the caching flags.

### 2.10 The refresh throttle — important for RL

```
006ec56c  if (game.frame != 0 && force == 0 && !(game.semaphore & GAME_OVER)
              && (who + game.frame) % 10 != 0)  return;
```

**The score only refreshes on frames where `(who + frame) % 10 == 0`** — a 10-frame
round robin phase-offset by player slot, the same shape as `Objects::process_all`'s
`(frame + i) % 10`. Bypassed on frame 0, when forced, and once the match is over.

Consequences for a reward signal:

* per-step score deltas are **zero on 9 frames out of 10** and lumpy on the tenth;
* different players update on different frames, so a naive "my score − their score"
  reward is comparing values up to 9 frames apart;
* at 15 frames/sim-second, the refresh period is **2/3 of a game second**.

Two further caches, on `leader_flags`:

* `score_units` / `score_units_2` recompute only when `LEADER_NEW_UNITS (0x800000)`
  **or** `LEADER_NEW_TECH (0x1000000)` is set; the pass then clears `NEW_UNITS` only.
* `score_unit_upgrades` / `score_research` recompute only when `LEADER_NEW_TECH` is
  set, and the pass clears it.

`score_buildings`, `score_wonders`, `score_economy` and `score_territory` are
recomputed on every refresh.

### 2.11 Derived scores

`LeaderData::get_team_score` @ `0x006D6520` — returns the player's own `score`
unless the team-scoring semaphore (`game[0x820] & 0x80`) is set, in which case it is
the **average** over self + team-mates.

`LeaderData::get_mvp_score` @ `0x006D5DE0` — the end-of-match MVP ranking:

```
if score == 0: return 0
v = score_combat*3 - score_economy/2 - score_buildings/2 + score_territory + score
if LEADER_WON:                              return v * 2
if GAME_OVER and LEADER_SURVIVED:           return (v*5)/4
return v
```

Note it weights `score_combat` ×3 — a component nothing in retail ever writes. MVP
is therefore, in the shipped game, `score + territory − (economy + buildings)/2`
with a winner/survivor multiplier.

---

## 3. Victory conditions

`GameInfo::victory` (`Game+0x38`) selects the mode. The mapping is pinned three ways
that agree: the `VictoryIndex` enum in the PDB, `<CATEGORIES id="victories">` in
`ron-data/rules.xml:1907`, and the ten one-line `ScenarioFuncSet::is_victory_*`
predicates at `0x009E5250..0x009E52E0`, each of which is literally
`cmp byte [game+0x38], <k>; sete al`.

| k | `VictoryIndex` | UI name |
|--:|---|---|
| 0 | `VICTORY_STANDARD` | Standard |
| 1 | `VICTORY_SUDDEN_DEATH` | Sudden Death |
| 2 | `VICTORY_CONQUEST` | Conquest |
| 3 | `VICTORY_SCORE` | Score |
| 4 | `VICTORY_TIME_LIMIT` | Time Limit |
| 5 | `VICTORY_MUSICAL_CHAIRS` | Musical Chairs |
| 6 | `VICTORY_WONDER` | Wonder |
| 7 | `VICTORY_POPULATION` | **Territory** (option category is `popwins` = "Territory Goal") |
| 8 | `VICTORY_ECONOMIC` | Economic |
| 9 | `VICTORY_TECH_RACE` | Tech Race |
| 10 | `VICTORY_SCENARIO` | Scenario Victory |

`Leader::victory(int, int)` records `VictoryTypeIndex` in `LeaderData::victory_type`
(`+0x7D8`): 0 `GENERIC`, 1 `BY_WONDER`, 2 `BY_TERRITORY`, 3 `BY_TECH_RACE`,
4 `BY_SCORE`, 5 `BY_ECONOMY`, 6 `BY_TIME_LIMIT`.

### 3.1 `GameDaemon::process_victory` @ `0x00730EF0` — the per-frame sweep

4,253 bytes, called from `GameDaemon::process_all` (step 12 of `Game::do_frame`),
before any unit moves. Blocks run in this order, and several are live under more
than one setting:

1. **Armageddon** — `if (get_armageddon() <= game.armageddon) Game::defeat_all()`.
2. **Wonder** — under `Standard | SuddenDeath | Wonder`.
3. **Territory** — under `Standard | SuddenDeath | Population`.
4. **Score** — under `Score`.
5. **Time limit** — under `TimeLimit`.
6. **Musical chairs** — under `MusicalChairs`.
7. **Economic** — under `Economic`.

So a **Standard** game is simultaneously running the wonder countdown and the
territory countdown; those two are not separate game modes, they are the standard
game's alternate win paths.

### 3.2 Wonder

`Game::wonder_winning` @ `0x005948A0` picks the leader whose
`LeaderData::get_wonder_net` (`0x006EBB10`) meets
`wonderwins.list[info.wonderwin].data[0]`. Ties: between **allies** the higher
`get_wonder_value` (`0x006EBB90`) wins; between **non-allies** nobody wins.

Dedicated `Wonder` and `SuddenDeath` victories fire immediately. `Standard` arms a
countdown only when the game is networked/recording (`Game` semaphore bit 2) or has more
than one nation. It also fires immediately if any valid member of the qualifying alliance
has prerequisite `0x2B9`; that bypass is passed as `instant=1` to `Leader::victory` and
therefore sets `INSTANT_VICTORY`. The countdown lives in `LeaderData::wonderwin_timer` (`+0x44C`) /
`wonderwin_stamp` (`+0x448`) and fires when `Game::wonder_timer() - (frame - stamp) <= 0`.
Losing qualification clears the timer and rewinds the stamp by `frame`. Warning
announcements are emitted at 4500 / 1800 / 900 frames remaining (`0x1194 / 0x708 /
0x384`), which is presentation.

Thresholds, `<CATEGORIES id="wonderwins">`: 1, 2, 3, 4, 6, 8, 10, 12, 14, 16, 20,
24, **9999** (= "No Wonder Victory").

### 3.3 Territory

Threshold `popwins.list[info.popwin].data[0]`, a **percentage**:
30, 35, 40, 45, 50, 55, 60, 65, 70, 75, 80, 90, **100** (= off).

```
mine = get_team_terr(who) * 100 / world.land_size
qualifies = mine >= pct  AND  no non-allied active leader also has >= pct
```

`LeaderData::get_team_terr` @ `0x006D62E0` is the **sum** of `territory` over self +
allies (not an average). Countdown lives in `popwin_timer` (`+0x444`) /
`popwin_stamp` (`+0x440`), length `Game::popwin_timer()`.

That "no rival also over the line" clause means the territory countdown *stalls*
whenever two hostile blocs are both above the threshold.

### 3.4 Score

`goal = scores.list[info.score_goal].data[0]` ∈ {1000…10000, 15000, 20000}.
Fires immediately when the best `get_team_score` reaches it. **This is the reward
scale a Score-victory RL environment is graded on.**

### 3.5 Time limit

`time_limits.list[info.time_limit].data[0]` is **minutes** (15, 30, 45, 60, 90, 120,
180, 240, 240) and is compared against `Game::tick` (`+0x560`, game seconds) times
60. When the limit is not a valid index the engine falls back to a network-supplied
value or 300 minutes. On expiry the highest team score wins
(`VICTORY_BY_TIME_LIMIT`). A separate arm handles `info.game_rules == 8`.

Remember the tick asymmetry: `Game::tick` increments every 15 frames, so a
"60 minute" limit is 54,000 sim frames = **60.3 real minutes** at Normal's 67 ms
tick.

### 3.6 Musical chairs

`chairs.list[info.chairs].data[0]` is minutes (2, 5, 8, 10, 15, 20, 25, 30); the
engine multiplies by **900 frames** (= 60 game seconds × 15). Every interval, the
lowest-scoring player is eliminated with `DEFEAT_MUSICAL_CHAIRS`. In free-for-all
team styles (`info.team_style ∈ {0, 8, 11}`) or when only one team is left, one
global lowest is culled; otherwise the lowest **within each of 4 teams**. An exact
tie eliminates nobody — the engine tracks a "was it a tie" flag and skips the call.
`game.musical_chairs` (`+0x6C4`) is the interval stamp, and **any** defeat resets it
(`Leader::defeat` writes `game.frame` there at `0x006ECB94`).

### 3.7 Economic

`econwins.list[info.econwin].data[0]` ∈ {100…900} average income. Fires when
`LeaderData::get_team_economic` @ `0x006D6400` (the **average** of
`LeaderData::get_economic` `0x006D6490` over the team) reaches it.

### 3.8 Conquest / last alliance standing

`Game::check_victory` @ `0x005926B0`. Called from `Leaders::strategy_all`
@ `0x006ED483` — but **only when `game[0x821] & 2` is set**; that gate is not in
`docs/derivation/architecture.md` §3.3's second-level listing. Logic:

* if every remaining alive leader is **mutually allied** with the first one found →
  set the game-over semaphore (`game[0x820] |= 0x40` at `0x00592838`), mark the
  survivors `LEADER_SURVIVED`, and, unless armageddon already fired,
  `Leader::victory(VICTORY_GENERIC, 0)` the winner;
* otherwise return without doing anything.

### 3.9 The map-size-scaled timers

`Game::wonder_timer` `0x005944F0`, `Game::popwin_timer` `0x005944B0` and
`Game::retake_capital` `0x00594530` share one body:

```
if base <= 0: return 0
s = map_sizes.list[3].data[0]          # 70, the "Standard (4-player)" edge — a FIXED index
return max(1, (World.xs * base + s/2) / s)
```

Note **index 3, not `info.map_size`** — the reference is the Standard map, and the
actual map's `World::xs` is the numerator. On a Standard map the timers are exactly
their rules values (4500 / 3600 / 3600 frames); on Big Huge (100) a wonder countdown
is 6,428 frames.

---

## 4. The armageddon clock — it *is* sim-critical

`Game::get_armageddon` @ `0x00594020`:

```
v = armageddon_per_team * num_sides + armageddon_per_nation * num_nations + armageddon
switch (info.starting_resources):            # GameInfo+0x21, Game+0x2D
    7  (Deathmatch):        v *= 3
    5,6,11,12 (x10, x20, Variable High, Random): v *= 2
    8  (Infinite):          v = max(v, 100)
return v
```

Retail rules: `ARMAGEDDON 4`, `ARMAGEDDON_PER_NATION 1`, `ARMAGEDDON_PER_TEAM 2`
(`ron-data/rules.xml:732-734`). A 6-player 2-team game ends at 4 + 6 + 4 = 14 nukes.

`Game::armageddon` (`+0x6E0`) counts nukes detonated. **Every single score function
opens with `if (game.armageddon >= get_armageddon()) return`,** and `compute_score`
then zeroes all eleven fields. So armageddon does not merely end the match — it
**erases everyone's score**. `Game::defeat_all` @ `0x00592AA0` defeats every active
undefeated leader with `DEFEAT_ARMAGEDDON` and force-recomputes (i.e. zeroes) every
score.

For an RL agent this is a genuine cliff in the reward landscape: crossing the nuke
threshold takes the reward to exactly 0 for everybody.

---

## 5. Diplomacy and how it gates targeting

`LeaderData::diplos[8]` at `+0x74`, values from `DiploButtonCats`:
`0 DIPLO_WAR`, `1 DIPLO_PEACE`, `2 DIPLO_ALLY`. The array is **directional** but
every predicate reads it as a **mutual minimum**:

```
LeaderData::get_diplo(j)  @0x006EBA50
    if j == who:                     return ALLY
    if diplos[j] == 0:               return WAR      # my declaration alone
    if leaders[j].diplos[who] == 0:  return WAR      # or theirs alone
    if both == ALLY:                 return ALLY
    return PEACE

is_enemy(j) @0x006EBAA0  : j != who && (diplos[j] == 0 || leaders[j].diplos[who] == 0)
is_ally(j)  @0x006EDB50  : j == who || (diplos[j] == 2 && leaders[j].diplos[who] == 2)
is_peace(j) @0x006E1200  : j != who && both != 0 && not both-ALLY
is_target(j)@0x006E12C0  : get_target() == j
is_neutral()@0x006EBAE0  : info.team_style == 7 && player[get_player()].flags&1
                           && player[get_player()].byte@0x78 == 8
```

A leader is **never its own enemy** and **always its own ally**, which is why
`is_enemy` is not simply `get_diplo == War`.

Where this gates behaviour, from the pathfinder lane's cost function
(`PathFinder::calc_cost` `0x00684E50` calls `is_enemy` / `is_ally` / `is_peace` /
`get_target`) and from the victory checks above:

* **targeting**: `is_enemy` is the predicate; declaring war on one side is enough to
  make both sides enemies, so a unilateral DOW makes you targetable immediately;
* **victory**: `is_ally` (strictly mutual) is the predicate for "we win together",
  for the territory-rival test, and for `check_victory`'s last-alliance test;
* **team score/territory/economy**: `is_team`, which reduces to `is_ally` on these
  paths.

`Leader::set_diplo` @ `0x006EC6A0` (783 bytes) additionally re-targets armies, drops
shared vision and emits chat; only the state write is ported here.

---

## 6. Defeat, elimination and match end

`DefeatTypeIndex` (`LeaderData::defeat_type`, `+0x7DC`): 0 `CONQUEST`, 1 `CAPITAL`,
2 `SUDDEN_DEATH_CAPITAL`, 3 `SUDDEN_DEATH`, 4 `MUSICAL_CHAIRS`, 5 `VICTORY`,
6 `RESIGN`, 7 `DISCONNECT`, 8 `ARMAGEDDON`, 9 `SCENARIO`, 10 `HERO`.

`EliminationIndex` (`GameInfo::elimination`, `Game+0x37`): 0 `CONQUEST`,
1 `CAPITAL`, 2 `CAPITAL_SUDDEN`, 3 `SUDDEN`.

`Leader::victory(vt, instant)` @ `0x006EC9B0`:

```
if leader_flags & (LEADER_WON | LEADER_DEFEATED): return
leader_flags |= LEADER_WON
leader_flags2 = instant ? |INSTANT_VICTORY : &~INSTANT_VICTORY
victory_type = vt
clean_queue(0) on every live owned Build                 # aggregate num_queued -> 0
for every other active leader:
    leader_flags |= LEADER_SURVIVED
    if is_ally(other, me):  other.victory(vt, instant)     # recursive — allies win too
    else:                   other.defeat(DEFEAT_VICTORY, -1, 0)
```

`Leader::defeat(dt, by, instant)` @ `0x006ECB00`:

```
defeat_by(by, dt)
if leader_flags & LEADER_DEFEATED: return
leader_flags2 = instant ? |INSTANT_DEFEAT : &~INSTANT_DEFEAT
leader_flags = (leader_flags & ~LEADER_ACTIVE) | LEADER_DEFEATED
leader_flags2 |= LEADER_UNIT_AI_OFF
game.musical_chairs = game.frame                # 0x006ECB94
defeat_type = dt
clean_queue(0) on every live owned Build
Armies::leader_defeated(me)                     # stop every valid standing Army
for every valid Unit owned by me:
    if UnitData::is_plane(): Unit::die(0, -1, 0.0)
    else:                    Unit::clear_orders()
    unit_masks &= ~0x40000
if GAME_OVER is not set: Game::check_victory()
```

The victory state now accumulates an eight-bit terminal-cleanup owner mask rather than
pretending the aggregate `LeaderData::num_queued` array is the concrete queue store. The
live production adapter drains each requested owner with the no-refund net transition of
`Build::clean_queue(0)`: visit valid owned Builds, zero progress in the former logical
prefix, decrement positive per-type queued counters, set logical `queued` to zero, and
clear `REPEAT_QUEUE`, while leaving allocated records and resources intact. The mask is
not cleared by the step-12 victory sweep, so a step-11 resolution cannot be lost before
the object store is flushed.

Defeat has a second accumulated owner mask for the Unit-band transaction. The live adapter
preflights every installed type and path sidecar before mutating the first row, kills true
planes through the existing Unit death/DeathObj transaction, and applies `clear_orders`'s
order/path/facing-latch net state to all other valid Units. Unknown type facts fail the whole
owner sweep closed and leave its request armed. See `docs/mechanics/defeat-cleanup.md`.
The step-14 Tech Race callback drains the same mask synchronously because its defeated-player
transaction occurs after the ordinary step-12 flush.

The World Government bypass is likewise no longer intended as a manually injected test
fact. `LiveProductionRuntime::leader_has_prerequisites(owner, 0x2B9)` resolves the
installed bonus row's prerequisite list against that owner's live `TechState`; missing
leader/type facts fail closed. The tick refreshes all eight derived `has_preq_2b9` values
immediately before `process_victory`, and drains the terminal-cleanup mask after either a
step-11 `check_victory` or the step-12 victory sweep.

`Leader::process_elimination` @ `0x006B8A20`, called per active leader from
`Leaders::process_all`:

```
if info.elimination != ELIMINATION_CAPITAL: return
if lost_capital_timer == 0: return                       # LeaderData +0x418
elapsed = game.frame - lost_capital_stamp                # LeaderData +0x414
if Game::retake_capital() - elapsed > 0: return
defeat(DEFEAT_CAPITAL, <capital>, 0)
```

`LEADER_*` flag bits, from the PDB `LeaderFlagIndex`: `VALID 1`, `ACTIVE 2`,
`HUMAN 4`, `WON 0x20`, `DEFEATED 0x40`, `SURVIVED 0x80`, `NEW_UNITS 0x800000`,
`NEW_TECH 0x1000000`. The idioms to recognise: `(flags & 3) == 3` is "in the game
and playing"; `(flags & 0x43) == 3` is "…and not yet defeated".

---

## 7. Checksum channels

**This lane's state is entirely inside channel 8.** `CheckSums::check_all`
(`0x00936560`) inlines `LeaderData::walk_data` (`0x006D6750`) as its 8th channel,
and per `schema/state-schema.json` that walker covers `[0, 8)` then `[8, 26922)` of
`LeaderData` — which contains every field here: the score block `+0x18..+0x44`,
`diplos +0x74`, the popwin/wonderwin/lost-capital stamps and timers `+0x414..+0x44C`,
`victory_type +0x7D8`, `defeat_type +0x7DC`, `territory +0x9D8`, and the
`num_units` / `num_buildings` / `num_queued` count arrays the score reads.

Match-level state rides `Game::walk_data` (`0x00589600`), which walks
`Game[0x550 .. 0x6E4)` — covering `frame`, `tick`, `on_team`, `starting`,
`num_nations`, `num_sides`, `musical_chairs` and `armageddon` — plus
`Game[0x814 .. 0x81C)`, the `semaphore` `BitMask<256>` header.

`Leaders::walk_bytes` / `Match::walk_bytes` emit these fields in engine field order,
and `adler32` (a port of `0x00A46830`, checked against the standard
`adler32("Wikipedia") == 0x11E60398` vector) folds them the way the engine does.
They are ordering-faithful, **not** byte-layout-faithful: the real walker hashes the
raw struct including everything the other lanes own. Treat them as a
lane-local regression hash until a full `LeaderData` layout exists.

---

## 8. Honest gaps

* **Tech Race victory (`VICTORY_TECH_RACE` = 9, `VICTORY_BY_TECH_RACE` = 3) is
  production-wired.** Retail triggers it synchronously in
  `Leader::gain_tech` at `0x006DE847..0x006DE997`, not in
  `GameDaemon::process_victory`. `systems::tech_race` owns both exact predicates,
  typed opponent-progress presentation, and the `Leaders::victory` terminal handoff.
  `production_runtime` invokes it before generic auto-unlocks, then restores the active
  producer and drains every requested concrete Build queue before step 14 continues. The
  remaining setup/UI/object-store boundaries are frozen in `docs/mechanics/tech-race.md`.
* **Musical chairs is the weakest block.** The interval, the 900-frame conversion,
  the free-for-all vs per-team split and the tie-skip are all read correctly, but the
  engine's team enumeration goes through `LeaderData::get_team` (`0x006EC040`) and
  `Game::on_team[8]`, which belong to the setup lane. `Leaders::team_of` is a
  documented stub returning `who`, so per-team culling degenerates to per-player.
  Do not trust this block in a team game until `get_team` is ported.
* **Wonder points remain explicit inputs to this module.** The completed-Wonder registry,
  live `get_wonder_value` queries, allied totals, hostile subtraction, and fail-closed tick
  supply now live in `systems::wonders`; direct `process_victory` callers still provide the
  two slices. See `docs/mechanics/wonders.md`.
* `LeaderData::type_avail`, `has_tech`, `researching`, `get_economic` and
  `find_capital` are modelled as **plain input fields**, not ported. `type_avail`
  (`0x006E33A0`) is 1,091 bytes of prerequisite logic and belongs to the tech lane.
* `Leader::defeat`'s Build and Unit traversals are live. The remaining deterministic
  continuation is `Armies::leader_defeated`: `Army::stop` is identified, but its complete
  standing-army/Group state transaction is not yet connected to this owner sweep.
* `get_team_terr` has a `team_style == 7` / frame-0 branch I collapsed to the
  common `is_ally` path; that branch only fires in the neutral-player game style.
* `Game::check_victory`'s early section counts connected human players and has a
  network-only arm (`0x009B7D30` / `0x009B8AC0`, Crossplay); not modelled — headless
  solo does not reach it.
* The `Game::semaphore` bit **names** are inferred from use sites; the engine's own
  enum for them is not in the type stream. The bit *positions* are measured.
* **The module is not yet reachable from `don-sim`.** `pub mod victory_score;` has
  been added to the shared `crates/don-sim/src/systems/mod.rs` (that file's own
  header asks each lane to add its own line), but `crates/don-sim/src/lib.rs` still
  has no `pub mod systems;`, so `cargo test` at the repo root does not build or run
  any `systems/` module. The owner of `lib.rs` needs to add one line:

  ```rust
  pub mod systems;
  ```

  Until then this file is verified by `rustc --edition 2021 --test
  crates/don-sim/src/systems/victory_score.rs` — **27 tests, all passing, zero
  warnings**, which is how the results in this report were produced. Unlike several
  sibling `systems/` modules, this one is `std`-only with **no crate-root
  dependencies** (no `crate::rng`, no `crate::world`), so it compiles unchanged the
  moment `systems` is declared.

---

## 9. What a Tier-B (differential) run would need

Nothing in this lane has been through the oracle; the tier is C. The blocker is that
every score function reaches through global singletons rather than taking its inputs
as arguments. A Tier-B harness would need `oracle` to gain a memory-poke step that,
before the call, writes into the mapped image:

| global | what to place |
|---|---|
| `[0x00C061EC]`, `[0x00C061E8]` | a fabricated `Game` (needs `+0x2D`, `+0x38`, `+0x550`, `+0x600`, `+0x6A0`, `+0x6A8`, `+0x6E0`, `+0x820`) |
| `[0x00C061F0]`, `[0x00C061E4]` | a fabricated `Constants` (needs `+0x354..0x360`, `+0x37C`, `+0x3A4`, `+0xD04..0xD1C`) |
| `[0x00C06188]` | a `World` with `xs` and `land_size` |
| `[0x00C061E0]` | the `leaders` base — this must be the **real** `0x00E3A390`, because `compute_unit_score` / `compute_build_score` address `num_units` / `num_buildings` / `num_queued` at absolute VAs inside it |
| `[0x00E85DDC]` | a `TypeData*` array; entries need real vtables from `schema/vtables.json` so the `is_*_type` virtuals resolve |
| `[0x00C061FC]`, `[0x00C061F4]` | `unittypes` / `buildtypes` `PtrArray`s |

Then `oracle call 0x006EC560 <force>` with `ecx = 0x00E3A390 + who*0x6EEC` returns
the retail score for a fabricated player, and every branch of §2 becomes
differentially testable. The image is mapped at its preferred base with relocations
applied and `Mapped::make_page_writable` already exists, so this is a bounded change
to `crates/oracle/src/main.rs` — I judged it out of budget for this lane and am
flagging it rather than claiming a tier I did not earn.

The single highest-value oracle case, if only one is built: `TypeData::get_score_value`
(`0x006673B0`) over a sweep of `(TypeIndex, param)`. It is the innermost function of
the reward, it needs only `types[]` and `constantsc`, and every other score number is
a small integer combination of its output.

---

## 10. Corrections to existing docs

1. `docs/derivation/architecture.md` §3.3 lists `Game::check_victory` as an
   unconditional tail call of `Leaders::strategy_all`. It is **conditional** on
   `game[0x821] & 2` (`test byte [eax+0x821],2; je` at `0x006ED47A`). The
   always-running victory sweep is `GameDaemon::process_victory`, inside
   `GameDaemon::process_all`.
2. `docs/derivation/architecture.md` §3.2 calls `Game+0x560` "`Game::seconds`". The
   PDB names it **`Game::tick`**. The frames→seconds relation is unchanged; the field
   name matters because `process_victory`'s time-limit block reads it by name.
3. Nothing else this lane touched contradicted an existing claim.
