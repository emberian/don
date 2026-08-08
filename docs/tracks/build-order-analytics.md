# Build-order analytics — a computational study of the Rise of Nations opening

Lane: **analytics**. Code: `/Users/ember/dev/don/analysis/`. Everything numeric in this
report was produced by that code from `riseofnations.exe`, the shipped data files, a live
read of the running process, or a shipped recorded game. Every claim is marked
**[measured]** (I verified it here, on this binary / this data / this replay) or
**[reported]** (I read it in another lane's document and did not re-check). Nothing here
is "verified" in the proof-assistant sense — see `docs/CHARTER.md`.

---

## 0. What this lane established

**Lead result, and it is not the optimisation.** Reducing the opening to an optimisation
problem forced me to name every quantity the engine actually uses, and doing that turned up
**three corrections to `docs/derivation/economy.md`** and **one closure of the biggest
open hole in `docs/derivation/sim-economy.md`**. In order of how much they matter:

| # | finding | tier |
|---|---|---|
| **F1** | `COMMERCE_CAP` is **not indexed by the age**. `econ+0xE8/0xEC/0xF0/0xF4` are the **four library-column levels**; `econ+0xE8` indexes `POP_CAP`, `econ+0xEC + 1` is the **city limit**, and `econ+0xF0` (which indexes `COMMERCE_CAP`) is therefore a tech-column level, not an age. Confirmed live: an Ancient-Age match HUD reads 13/**25** pop and **1/1** cities. | [measured], structural + one live observation |
| **F2** | The **worker→income composition** that `sim-economy.md` §7 calls "the single largest hole left in the economy" is recovered end to end: `FUN_006CEEE0` → `FUN_00737C60` → `FUN_006D5530` → `FUN_00639E40`. One worked gather slot = `PEASANT_RATE/256` = **10 resources per 30 s**. | [measured], structural |
| **F3** | Resource slot order is **0 food, 1 timber, 2 wealth, 3 knowledge, 4 metal, 5 oil**, confirmed three independent ways. `sim-economy.md` §3.2 declined to name slots 4 and 5; they can be named now. | [measured] |
| **F4** | `JOB_EXTRA_TIME` for the Citizen is **10** in live memory, not 1. `economy.md` §4.2 predicted 1 on the assumption that `unitrules.xml` fields go through the plain `_wtoi` path. `"1/10tsx"` at scale 100 gives exactly 10, so `unitrules.xml` has scaled fields of its own. | [measured] |

The optimisation results are in §4–§7. The headline: **the shipped AI's boom order is
about 66 s slower to a fixed target state than the best order my search found (5:05 vs 4:00, +27 %), and
the whole difference is one decision — the shipped AI opens with Science I (Written Word)
where the optimum opens with Commerce I (Barter).**

**Fidelity honesty, up front.** The economy model here is **Tier C at best, and parts of it
are below that.** It is built from instruction-level reads of the shipped binary plus one
live read of the running process's type tables; **no part of it has been executed against
retail.** The search is a **beam search, not an exact optimiser** — §5 measures its own
noise floor at ~30–40 s, which is comparable to some of the effects being reported, and I
say so wherever that matters. Do not quote any number in §4–§7 as a fact about Rise of
Nations; quote them as facts about *this model*, whose inputs are listed in §2.

---

## 1. Reproduction

```sh
cd /Users/ember/dev/don
python3 analysis/derive.py            # -> analysis/derived.json
python3 analysis/study.py             # all sections, ~10 min (S3b dominates)
python3 analysis/study.py S3          # one section
python3 analysis/replay_order.py ron-data/replays/today.rcx
```

`analysis/derive.py` reads only `docs/derivation/rules-constants.json` and
`schema/live/{unit,building,tech}-attributes.txt`. It writes `analysis/derived.json`,
which is the single source every other file in `analysis/` uses. No repo file outside
`analysis/` and this document was touched.

The instruction-level reads below were done with a 25-line capstone script (scratchpad,
throwaway); each is reproducible as:

```sh
cd /Users/ember/dev/don/ron-bin
uv run --quiet --with capstone --with pefile python <disp.py> 0x6ce450 0x6ce8d0   # TickResource
uv run --quiet --with capstone --with pefile python <disp.py> 0x6d5530 0x6d6100   # per-city gather
uv run --quiet --with capstone --with pefile python <disp.py> 0x639e40 0x63bfff   # per-building gather
uv run --quiet --with capstone --with pefile python <disp.py> 0x664090 0x667000   # cost + ramp
uv run --quiet --with capstone --with pefile python <disp.py> 0x6ce900 0x6ce9c0   # commerce caps
```

---

## 2. The opening as an optimisation problem, from derived quantities only

### 2.1 The clock and the accumulator [measured]

`Player::TickResource` (`0x006CE450`) runs **once per frame per player** — the call chain
`FUN_00591EF0 → FUN_006ED2A0 → FUN_006CE280 → FUN_006CE450` has no modulo gate
([reported] from `sim-economy.md` §3.1; I re-read `FUN_006ED2A0` and agree). Its payout
core:

```asm
006ce7b9  mov  ecx, [rules + 0x27c]     ; GATHER_RATE = 450
006ce7c1  shl  ecx, 4                   ; period = 7200
006ce7c5  idiv ecx                      ; whole = income / 7200, rem in edx
006ce7de  add  ecx, edx                 ; acc += rem      (econ+0x18+r*4 ^0x3421)
006ce810  ...  while (acc >= 7200) { acc -= 7200; whole++ }
006ce855  add  eax, whole               ; stockpile += whole  (^0x8221)
```

So over `GATHER_RATE = 450` frames a player banks `income·450/7200 = income/16` resources.
**`income` is in sixteenths of a resource per 450-frame period.** 450 frames is 30 s at the
engine's 15 fps (`0x005924BF`/`0x005924CF`: frame counter at `game+0x550`, `idiv 15`).

I also observed the frame counter advancing in the live process across three reads — 4181,
4236, 4256 — which is consistent with 15 fps but is not a measurement of it.

### 2.2 The income composition [measured] — closes `sim-economy.md` §7 item 3

`FUN_006CEEE0` is the gross-income builder. It writes a six-element vector that
`FUN_006CE280` stores at `econ+0x64..0x78` (`^0x872`) and `TickResource` consumes.

```asm
006cf05c  mov  edx, [0xc061f0]          ; g_rules
006cf062  mov  ecx, 0xc4
006cf06b  mov  eax, [edx + ecx + 0x188] ; 0xC4+0x188 = 0x24C = 588 = BASIC_GATHER[r]
006cf072  shl  eax, 4
006cf075  mov  [ecx + ebp - 0xc4], eax  ; out[r] = BASIC_GATHER[r] * 16
006cf08c  cmp  ecx, 0xdc                ; six resources
```

then, per city, `FUN_00737C60 → FUN_006D5530`:

```asm
006d592f  lea  eax, [esi + ebp]         ; &RULES[0x264 + r*4]  (0x264 = 612 = CITY_GATHER)
006d5932  cmp  [eax + edi], edx         ; skip if zero
006d5979  shl  eax, 4
006d597c  add  [ebp], eax               ; out[r] += CITY_GATHER[r] * 16
```

and, per gather building, `FUN_0062D360 → FUN_00639E40`:

```asm
0063a1ba  cmp  dword [ebp - 0x10], 5    ; r == 5 (oil)?
0063a1c6  mov  edx, [ecx + 0x29c]       ;   OIL_RATE   = 8960 = 35.0 in 8.8
0063a1d9  mov  ecx, [eax + 0x280]       ;   PEASANT_RATE = 2560 = 10.0 in 8.8
0063a1ef  imul eax, [ebp - 0x90]        ; slots16 * RATE
0063a1f6  cdq / and edx,0xff / add / sar eax,8      ; >> 8, the 8.8 unscale
0063a249  imul edx, [ebp + ecx*4 - 0x1dc]           ; * (100 + cityBonus[r])
0063a25f  idiv [ebp + esi*4 - 0x1c4]                ; / 100
```

`slots16` is the worker count in **1/16-slot fixed point** — the display path at
`0x0063B8D4` recovers the integer count as `(slots16 + 8) >> 4`. So one worked slot
contributes `16 · PEASANT_RATE / 256 = 160` income units = **10 resources per 30 s**.

The city bonus comes from `FUN_00738360`, which is a five-case switch on the resource:

| case | city byte | source | shipped level-1 value |
|---|---|---|---|
| 0 | `city+0x56` | `GRANARY_BONUS[lvl-1]` (`0x2B8 + lvl*4`, set in `FUN_00737C60`) | +20 % |
| 1 | `city+0x57` | `LUMBERMILL_BONUS[lvl-1]` | +20 % |
| 4 | `city+0x58` | `SMELTER_BONUS[lvl-1]` | +50 % |
| 5 | `city+0x59` | (written 0; refineries are applied at player level) | — |

and it returns `(bonus + 100) * 100 / 100`, i.e. a straight percentage. **`CITY_GATHER` is
added after the per-building loop and is therefore NOT multiplied by the granary /
lumber-mill / smelter bonus** — a detail a plausible re-implementation would get wrong.

Two more per-city terms, both returning whole resources that the caller shifts `<<4`:

* `FUN_00737B50` (wealth) `= VILLAGE_TAXES + n_buildings·BUILDING_TAXES + [city has type 436 → MARKET_TAXES] + [type 437 → TEMPLE_TAXES]`. Shipped: everything is 0 except **`MARKET_TAXES = 10`**, so **a Market is worth 10 wealth per 30 s** and in the Ancient age it is the *only* wealth income a player has.
* `FUN_00737C00` (knowledge) `= VILLAGE_LITERACY + [type 420 → UNIVERSITY_LITERACY] + [type 435 → LIBRARY_LITERACY]`. Shipped: **`UNIVERSITY_LITERACY = 10`**, library 0. So **a University is worth 10 knowledge per 30 s**.

Type ids 436 = Market, 437 = Temple, 420 = University, 435 = Library all check out against
`schema/live/building-attributes.txt`, which is an independent artefact.

### 2.3 F3 — the resource slot order [measured]

Three independent confirmations, any of which could have disagreed:

1. `STARTING_GOODS` entry prose in `rules.xml`: `200food`, `200timb`, `100gd`, `100know`, `100met`, `100oil`.
2. `FUN_00738360` maps case 0 → the **granary** byte, case 1 → the **lumber mill** byte, case 4 → the **smelter** byte. Granary is food, lumber mill is timber, smelter is metal ⇒ slot 4 = metal.
3. `FUN_006CEEE0` applies `AMERICANS_BARRACKS_GATHER` — whose shipped prose is *"2 each of food, timber, metal, and gold"* — to slots **0, 1, 4, 2** in that order (`0x006CF0D7` onwards). `REFINERY_BONUS` and `CAPITALISM_OIL_PROD` both apply to slot **5**, and `OIL_RATE` is selected for `r == 5`.

`sim-economy.md` §3.2 says the crate "refuses to name 4 and 5". It can name them.

### 2.4 F1 — what `COMMERCE_CAP` is actually indexed by [measured]

`Player::UpdateCommerceCaps` (`0x006CE900`):

```asm
006ce90b  mov  eax, [esi + 0x6eb8]      ; econ (a POINTER, not an inline block)
006ce911  mov  eax, [eax + 0xf0]
006ce917  xor  eax, 0x63187             ; "the AGE" per economy.md 3.3
006ce940  mov  ecx, [edi + eax*4 + 0x400]   ; RULES.COMMERCE_CAP[that]
```

It is not the age. Evidence:

* `econ + 0xE8 .. 0xF4` are read as **one four-element array** — `*(uint*)(econ + 0xe8 + i*4) ^ 0x63187` in `FUN_006C6BA0` — i.e. four counters with the same obfuscation key. Rise of Nations has exactly four library columns (Military, Civic, Commerce, Science).
* **`econ+0xE8` indexes `POP_CAP`**: `uVar5 = *(econ + 0xe8) ^ 0x63187; iVar3 = RULES[uVar5*4 + 0x3c4]` (`0x3C4` = 964 = `POP_CAP`), stored to `player+0x7E4`. Military techs raise the population limit; ages do not.
* **`econ+0xEC + 1` is the city limit**: `FUN_006D6130` returns `(*(econ+0xEC) ^ 0x63187) + 1`, plus `RULES[0x42C]` if a wonder property holds and `RULES[0x5A4]` for one nation. Those two constants are named **`pyramids_city_limit`** ("1 city") and **`bantu_city_limit`** in `rules-constants.json`. That is a could-have-failed check and it passed.
* The shipped AI agrees, twice: `economic.bhs` gates city #2 on `have_tech(who, "City State")` (Civic 1) and city #3 on `have_tech(who, "Empire")` (Civic 2), and researches "The Art of War" (Military 1) exactly when `population(who) >= 23` — against `POP_CAP[0] = 25`.

* **A live screenshot agrees.** Another lane captured `schema/live/gamestate-pid148.png` while a match was running — the same process I was reading. Its HUD reads **"Dutch: Ancient Age"**, **13/25** population and **1/1** cities. `POP_CAP[0] = 25` and city limit `= Civic 0 + 1 = 1`. The **1/1** is the decisive half: it is what `FUN_006D6130` predicts and it is not something an age-indexed reading produces. [measured]

Given `0xE8` = Military and `0xEC` = Civic, `0xF0` is Commerce or Science; it indexes a
constant literally named `COMMERCE_CAP`, and the Commerce column is the one whose techs are
Barter / Coinage / Trade / … So: **`COMMERCE_CAP` is indexed by the Commerce tech level.**
I have not proved `0xF0` is Commerce rather than Science — that residual is real and is
listed in §8.

`docs/derivation/economy.md` §3.3 should be corrected in two places: `COMMERCE_CAP` is not
"indexed by age", and `econ+0xF0` is not "the player's AGE, obfuscated".

### 2.5 The cost model [measured, structural]

`FUN_00664090` is ~12 KB and the bulk decompiler skips it. Reading it at the instruction
level, the shipped-data-relevant core is:

```asm
006645ae  mov  eax, [rules + 0x354]     ; UNIT_COST_FACTOR = 10  (0x358 = BUILD, 0x35C = TECH)
006645b4  imul ebx, eax                 ; base[r] = COST_field[r] * factor
006645bf  mov  [ebp - 0x14], ebx
00665966  movzx edx, [players + ...*2 + 0x5a22]   ; count of this type
0066596e  movzx eax, [players + ...*2 + 0x5222]   ; + queued
00665405  imul ecx, [ebp - 0x14]        ; ceiling = base * RAMP_MAX / 100
0066541c  lea  ecx, [UnitType + 0x270]  ; the two (resource, amount) SUPPORT pairs
00665440  cmp  [ecx - 8], edx           ; supportResource[i] == r ?
0066544b  mov  eax, [ecx] / imul eax, edi / add eax, edx    ; SUPPORTVALUE * count + extra
00665452  test esi, esi / cmovl ...     ; a ZERO ceiling means NO ceiling
006654a3  add  ebx, eax
```

i.e.

```
cost(type, r) = COST[r] * COST_FACTOR
              + min( SUPPORTVALUE[r] * (owned + queued) + progressive_extra,
                     COST[r] * COST_FACTOR * RAMP_MAX / 100 )      # ceiling 0 => none
```

`RAMP_MAX` is picked by unit class: scholar 2000 %, worker 500 %, other civilian 200 %,
military 125 %. There is a *progressive* variant (`[UnitType+0x2F4] & 2` → the count is
replaced by `n(n+1)/2` at `0x00665355`) and a scholar-specific triangular-in-7s term at
`0x006656E0`; neither applies to the Citizen (`PROGRESSION = 0` in live memory).

For the Citizen that reduces to **20 food, +1 per citizen already owned or queued, capped
at +100**. Which `RAMP_MAX` a *building* falls into is **not derived** — it is a class
lookup I did not resolve — so the study treats it as an assumption and sweeps it (§5).

### 2.6 The quantities, with sources

Generated by `python3 analysis/study.py S1`:

| quantity | value | source |
|---|---|---|
| `GATHER_RATE` | 450 frames = 30 s | `rules.xml`; used at `0x006CE7B9` |
| accumulator period | 7200 = `GATHER_RATE << 4` | `0x006CE7C1` |
| one worked gather slot | **10 resources / 30 s** | `PEASANT_RATE` 2560, `0x0063A1D9` |
| one oil slot | 35 / 30 s | `OIL_RATE` 8960, `0x0063A1C6` |
| one city, free | 10 food + 10 timber / 30 s | `CITY_GATHER`, `0x006D5979` |
| one Market | 10 wealth / 30 s | `MARKET_TAXES`, `FUN_00737B50` |
| one University | 10 knowledge / 30 s | `UNIVERSITY_LITERACY`, `FUN_00737C00` |
| commerce clamp | 70 / 100 / 150 / 200 / 260 / 320 / 400 / 500 per resource per 30 s | `COMMERCE_CAP`, `0x006CE940`, indexed by Commerce level |
| global income ceiling | 16000 units = 1000 resources / 30 s | `0x006CE706` |
| population cap | 25 / 50 / 75 / 100 / 125 / 150 / 175 / 200 | `POP_CAP` indexed by Military level |
| city limit | Civic level + 1 | `FUN_006D6130` |
| farms per city | 5 | `FARMS_PER_CITY_BASE`; `aibestbuildlibrary.bhs` `place_farm` uses the same 5 |
| starting goods | 200 f / 200 t / 100 w / 100 k / 100 m / 100 o | `STARTING_GOODS` |
| granary / lumber mill / smelter, level 1 | +20 % / +20 % / +50 % | `FUN_00738360` |

Costs, from the **live type tables** (`schema/live/*-attributes.txt`, read out of a running
`riseofnations.exe`), `COST` field × cost factor 10:

| item | food | timber | wealth | knowledge | metal | job_time (frames) | ramp per existing |
|---|---:|---:|---:|---:|---:|---:|---|
| Citizen | 20 | | | | | 50 | +1 food |
| Scholar | | | 30 | | | 75 | +2 wealth |
| Farm | | 40 | | | | 150 | +4 timber |
| Woodcutter's Camp | 50 | | | | | 150 | +20 food |
| Mine | | 50 | | | | 150 | +20 timber |
| Small City | 10 | 10 | | | | 600 | +50 food, +50 timber |
| Library | | 60 | | | | 420 | +20 timber |
| Market | | 80 | | | | 420 | +30 timber |
| University | | 60 | 30 | | | 420 | +20 timber, +20 wealth |
| Granary | | 60 | 40 | | | 1000 | +40 timber |

| tech | food | timber | wealth | knowledge | job_time |
|---|---:|---:|---:|---:|---:|
| Classical Age | 250 | | | | 400 |
| Written Word (Science I) | | 120 | 50 | | 200 |
| City State (Civic I) | 120 | | | | 200 |
| Barter (Commerce I) | 60 | 60 | | | 200 |
| The Art of War (Military I) | 120 | | | | 200 |
| Mathematics (Science II) | | | 120 | 80 | 275 |
| Coinage (Commerce II) | | 60 | | 140 | 275 |
| Empire (Civic II) | 160 | | | | 275 |
| Medieval Age | 250 | | | 250 | 550 |

---

## 3. The one open question I could not close, and how I bounded it

`TickResource` clamps the income to the commerce cap at `0x006CE509`:

```asm
006ce4c8  mov  esi, [ecx + ebx*4 + 0x64]  ; gross income  (1/16 resource per 450 frames)
006ce509  mov  eax, [ecx + ebx*4 + 0x30]  ; the cap
006ce50d  xor  eax, 0x1281
006ce512  cmp  esi, eax                   ; > cap -> esi = cap  (0x006CE61C)
```

`0x006CE940` wrote that cap **straight from `RULES.COMMERCE_CAP` with no `<<4`**, while the
income it is compared against *is* `<<4`-scaled. Two readings:

* **H1** — the comparison is missing a `<<4`; the intended cap is `COMMERCE_CAP` resources per 30 s (70 at Commerce 0).
* **H2** — the code means what it says; the effective cap is `COMMERCE_CAP/16` resources per 30 s (4.375 at Commerce 0).

I could not settle this by disassembly, and a live read of a running match failed for
mechanical reasons (§8). So I falsified H2 against a **shipped recorded game** instead —
`ron-data/replays/today.rcx`, parsed with `re/scripts/rcx_parse.py`:

```
recorded game: today.rcx, 60 queue commands, last at frame 9827 = 10:55 = 21.8 gather periods
priced spend  food=1585  timber=1134  wealth=532  knowledge=80
H1: cap =  70.000 food/30s -> at most 1528.6 food earned; player needed >= 1385  ==> CONSISTENT
H2: cap =   4.375 food/30s -> at most   95.5 food earned; player needed >= 1385  ==> REFUTED (14.5x over)
```

and a floor that uses **no cost model at all**: the recorded player queued **30 Citizens**,
and the Citizen `COST` field is `2 × UNIT_COST_FACTOR 10 = 20` food *before* a ramp that only
adds, so food earned ≥ 400 against H2's whole-game ceiling of 96. H2 would also make the
engine's own global ceiling (`16000` = 1000 resources / 30 s at `0x006CE706`) unreachable
dead code, since `COMMERCE_CAP` maxes at 500.

**H2 is refuted. The study uses H1.**

A second, weaker witness points the same way. The live HUD capture
`schema/live/gamestate-pid148.png` shows a green **"+100"** readout in the top bar of an
Ancient-Age match. 100 is `COMMERCE_CAP[1]` exactly; H2's Commerce-1 ceiling would be
`100/16 = 6.25`. I did **not** identify which HUD widget that number belongs to, so I
record it as suggestive, not decisive — but no reading of that screen is compatible with a
per-resource ceiling of six.

H1 is not comfortable either, and I will not paper over it: the implied average food income
in that replay is **63.4 per 30 s against a cap of 70**, i.e. 91 % of the cap sustained from
frame 0, which cannot literally happen. Two ways out, both plausible: our Citizen ramp is
steeper than retail's, or that game did not start from the shipped `STARTING_GOODS` (a game
option scales it — `economic.bhs` tests `get_starting_resources(who) > 4`). A related
asymmetry stays on record: `DUTCH_INTEREST_CAP` **is** shifted `<<4` at `0x006CE6FC` while
the commerce cap it is added to is not — `sim-economy.md` §4.2 flagged the same thing.

---

## 4. Method, and what it omits

`analysis/econ.py` is a frame-stepped model of the above. `analysis/plan.py` is a
**three-server discrete-event simulator plus a beam search over purchase sequences**.

**Why not LP/MILP.** Three things break a continuous relaxation: the accumulator and the
purchases are integral (a citizen pays nothing until a *whole* slot exists); the commerce
clamp is a kink whose position moves with a discrete tech level, so income is
piecewise-linear and non-convex in the decisions; and the cost ramps are step functions of
counts. A MILP over 15 fps time steps would need ~10⁴ binaries per building type and would
still have to linearise the ramp. The state that matters is small (five building counts,
four tech levels, a worker split, six stockpiles), so simulate-and-search dominates: it
relaxes no integrality and every assumption stays inspectable. The cost is that **the
search is heuristic — results are "best found", not "optimal"** — which §5 quantifies.

Three servers, because real play overlaps them: **CITY** (trains citizens), **CREW**
(citizens on a construction site — they stop gathering for the duration, which is modelled),
**LIBRARY** (research, one tech at a time).

**What the model omits.** Each of these makes the model optimistic about income or
pessimistic about time; the direction is stated so a reader can sign the bias:

| omitted | direction |
|---|---|
| walking, pathing, gather-point distance | model is **fast** (optimistic) |
| caravans, merchants, rare resources | model is **poor** (pessimistic on wealth) |
| territory taxes (needs map area and territory tiles) | pessimistic on wealth |
| the 8–300 frame lag on the gross-income recompute (`FUN_006CEEE0`'s gate) | optimistic |
| granary / lumber-mill bonuses applied globally rather than per city | optimistic (not reached in the Ancient age) |
| scouting, ruins, plunder, everything military | pessimistic |
| the game-speed multiplier, difficulty scaling, nation powers | neutral for a generic nation on normal |

**Assumptions**, all declared in `Assumptions` in `analysis/econ.py` and swept in §5:

| assumption | default | why that default |
|---|---|---|
| commerce clamp scale | 16 (H1) | §3 |
| slots per Farm | 1 | forced by `FARMS_PER_CITY_BASE = 5` and by the shipped AI's one-farm-one-worker bookkeeping |
| slots per Woodcutter's Camp | 5 | `aibestbuildlibrary.bhs` `place_woodcutter` **destroys and re-places** a camp whose `max_workers_at_building` is under `min_size = 5`, falling back to 4 then 3 — 5 is the designers' own target |
| slots per Mine | 4 | no source; unused in the Ancient age |
| construction with *k* citizens | `JOB_TIME / k`, k ≤ 4 | `buildingrules.xml` documents `JOB_TIME` as "how long for one citizen to build, in 1/15 sec"; the speedup shape is not derived |
| unit train time | `JOB_TIME` frames, no ramp | `economy.md` §4.2 and `sim-economy.md` §3.3 both say the `FUN_006508C0` ramp is a *rate*, not confirmed to be train time |
| building ramp ceiling | none (0) | `0x00665452`: a zero ceiling means no ceiling; which class a building is in is not derived |
| starting citizens | 5 | `economic.bhs` pins it twice: the nomad path (case 4) builds "up to 5 peasants" to reconstruct a standard town, and case 8 ("Build 4 Citizens") sets `needed_citizens = 9` |
| starting buildings | Small City + 3 Farms + 1 Library + 1 Woodcutter's Camp | `citytemplates.xml` gives 3 Farms + 1 Library; the camp comes from `economic.bhs`'s own bookkeeping — case 13 is "Build Woodcutter **#2**", and case 17 says "got **5** already" about farms after cases 9 and 12 each added one to a base of 3 |
| scholar knowledge rate | `PEASANT_RATE` (10 / 30 s) | **`SCHOLAR_RATE` (offset 644) has no read site** in a linear capstone scan of `.text`. [measured negative] — treat with suspicion, a linear sweep mis-decodes some data |

---

## 5. Result 1 — the fastest complete Ancient boom

**Objective.** Minimise the frame at which *all* of these hold: Classical Age, Written Word,
City State and Barter researched; ≥ 2 cities, ≥ 14 citizens, ≥ 7 farms, ≥ 2 woodcutter's
camps, ≥ 1 market. That is **exactly the state `economic.bhs` cases 6–18 build**, so the
shipped order and the searched order are asked the same question.

**Best found: 3604 frames = 4:00.**

| frame | t | action |
|---:|---:|---|
| 0 | 0:00 | Farm |
| 0 | 0:00 | Citizen |
| 37 | 0:02 | Farm |
| 50 | 0:03 | Citizen |
| 50 | 0:03 | **research Barter (Commerce I)** |
| 251 | 0:16 | research City State (Civic I) |
| 454 | 0:30 | Citizen |
| 904 | 1:00 | City #2 |
| 1112 | 1:14 | Farm, Citizen |
| 1274 | 1:24 | Citizen |
| 1690 | 1:52 | Farm, Citizen |
| 1740 | 1:56 | Citizen |
| 2232 | 2:28 | Market, Citizen |
| 2282 | 2:32 | Citizen |
| 2941 | 3:16 | research Written Word |
| 3204 | 3:33 | research Classical Age |
| 3554 | 3:56 | Woodcutter's Camp #2 |

End state: 14 citizens (7 food, 7 timber), 7 farms, 2 camps, 2 cities, 1 market; income
90 food + 90 timber + 10 wealth per 30 s, under the Commerce-1 cap of 100.

**A note on a degenerate objective.** "Minimum time to the Classical Age" *on its own* is
not an interesting question in this game: `STARTING_GOODS` is 200 food and the age costs
250, so a player who builds nothing banks the difference in about 35 s and the age lands at
about **1:04**. The age is cheap; the economy is not. Any age-time claim about Rise of
Nations has to be conditioned on an economy, which is why the objective above is a *state*,
not a tech.

### 5.1 How much of this is search noise

The same assumptions at four beam widths (`analysis/study.py S3b`):

| beam width | boom complete |
|---:|---:|
| 100 | 4:30 |
| 250 | 4:13 |
| 500 | 4:01 |
| 700 | 4:00 |

**The search's own noise floor is ~30 s.** Only differences larger than that are evidence.
The 66 s gap to the shipped AI in §7 clears it; most of the sensitivity deltas in §5.2 do
not.

### 5.2 Sensitivity to every assumption

Beam width 250 throughout, so the BASE row need not equal §5:

| assumption | value | boom complete | Δ |
|---|---:|---:|---:|
| BASE | | 4:13 | — |
| slots per Woodcutter's Camp | 3 | 4:32 | +18 s |
| | 4 | 4:20 | +7 s |
| | 6 | 4:02 | −11 s |
| starting citizens | 2 | 4:51 | +38 s |
| | 3 | 4:40 | +27 s |
| | 6 | 3:53 | −21 s |
| | 8 | 3:45 | −29 s |
| max builders on one site | 1 | 4:31 | +18 s |
| | 2 | 4:25 | +12 s |
| | 8 | 4:02 | −12 s |
| building ramp ceiling | 200 % | 3:55 | −19 s |
| | 500 % | 4:13 | 0 (never binds) |
| walk overhead per build | 30 frames | 4:11 | −2 s |
| | 75 | 4:14 | +1 s |
| | 150 | 4:26 | +12 s |
| **commerce clamp scale** | **8 (half of H1)** | **6:19** | **+125 s** |
| commerce clamp scale | 32 (double H1) | 4:00 | −14 s |

Every sweep is monotone in the expected direction, which is a weak sanity signal that the
model is wired the way it claims to be. But against a ~30 s search-noise band, **only the
commerce clamp scale is unambiguously load-bearing** (+125 s when halved). Everything
else — starting citizens, camp slots, builder count, the building ramp ceiling, walk
overhead — moves the answer by 10–40 s, i.e. by about as much as re-running the search with
a wider beam does. Treat those rows as "no evidence of a large effect", not "no effect".

That the clamp matters far more downward (+125 s at half) than upward (−14 s at double) is
itself informative: under H1 the Ancient economy is already pressed against the cap, so
loosening it buys almost nothing and tightening it is crippling.

---

## 6. Result 2 — where citizens stop being worth anything

Income is `10 × cities + 10 × workers` per 30 s per resource, and the clamp is per player
per resource. Solving for where the clamp binds:

| Commerce level | tech | cap / 30 s | max **useful** food workers, 1 city | 2 cities | 3 cities |
|---:|---|---:|---:|---:|---:|
| 0 | — | 70 | **6** | 5 | 4 |
| 1 | Barter | 100 | 9 | 8 | 7 |
| 2 | Coinage | 150 | 14 | 13 | 12 |
| 3 | Trade | 200 | 19 | 18 | 17 |

**At Commerce level 0 with one city, the seventh food gatherer produces literally nothing**,
and the same holds independently for timber. That is the single most load-bearing structural
fact this study produced, and it reframes the opening: the Ancient age is not a race to add
workers, it is a race to raise the ceiling that stops them counting.

It also produces a counter-intuitive corollary. `CITY_GATHER` gives each city 10 free food
and 10 free timber — but those free resources **consume cap headroom**. At Commerce 0, a
second city adds 10 + 10 of free income and simultaneously removes one usable food gatherer
and one usable timber gatherer: **net zero income**, minus 120 food for City State and
10 + 10 for the city itself. Under this model, expanding before raising the Commerce cap is
economically negative in the Ancient age.

### Marginal value of the Nth citizen

Citizen cost is `20 + min(1 × (owned + queued), 100)` food; a worked slot yields 10 resources
per 30 s; a new food slot usually costs a Farm at `40 + 4 × (farms owned)` timber.

| N | citizen cost | payback | farm # | farm cost | payback | combined |
|---:|---:|---:|---:|---:|---:|---:|
| 1 | 20 food | 1.00 min | 1 | 40 timber | 2.00 min | 3.00 min |
| 5 | 24 | 1.20 | 5 | 56 | 2.80 | 4.00 |
| 10 | 29 | 1.45 | 10 | 76 | 3.80 | 5.25 |
| 15 | 34 | 1.70 | 15 | 96 | 4.80 | 6.50 |
| 25 | 44 | 2.20 | 25 | 136 | 6.80 | 9.00 |
| 50 | 69 | 3.45 | 50 | 236 | 11.80 | 15.25 |
| 100 | 119 | 5.95 | 100 | 436 | 21.80 | 27.75 |
| 110+ | 120 (capped) | 6.00 | 110 | 476 | 23.80 | 29.80 |

The citizen ramp is gentle — it never exceeds 6 minutes of payback, and it caps outright at
120 food. **The price of a citizen is not what limits the opening.** What limits it is, in
order: the commerce clamp (above), `FARMS_PER_CITY_BASE = 5`, and `POP_CAP[Military 0] = 25`.
(The farm column past #10 is the ramp *curve*; it is only reachable with enough cities.)

### Best allocation over the first 10 minutes

Objective: (food + timber) banked at t = 10:00, since in the Ancient age every purchase is
priced in exactly those two. Note that the food/timber split is **not** a free variable — a
citizen gathers whatever the slot it stands in gathers, and slots come from buildings, so
the split is chosen by choosing buildings.

| scenario | best N | food wk | timber wk | farms | camps | food @10m | timber @10m | total |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 city, no techs | 11 | 5 | 6 | 5 | 3 | 1069 | 1427 | **2496** |
| 1 city, **Barter** | 14 | 5 | 9 | 5 | 3 | 904 | 1776 | **2680** |
| 2 cities (City State) | 10 | 5 | 5 | 5 | 2 | 1180 | 1328 | **2508** |
| 2 cities + Barter | 15 | 7 | 8 | 7 | 3 | 1112 | 1407 | **2519** |
| worst in each sweep | 24 | — | — | — | — | — | — | 1014–1452 |

Two things fall out. **Barter pays for itself inside 10 minutes and expansion does not**
(+184 for Barter on one city; +12 for a second city, and −161 if you take the city *and*
Barter versus Barter alone). And **over-training is expensive**: the worst point in every
sweep is "train to 24 citizens", which loses 40–60 % of the banked total to citizens that
cost food and stand in no slot, or stand in a slot whose output the clamp discards.

---

## 7. Cross-check against the shipped AI — and the one place they disagree

`ron-data/ai-scripts/economic.bhs` is a human-authored boom order by the game's own
designers (header: *"Mark Sobota / Mike Engle / boom script"*). Its generic-nation,
land-map, size ≥ 2 path is a step machine, cases 6 → 18:

| case | action | case | action |
|---:|---|---:|---|
| 6 | Science I — **Written Word** | 13 | Woodcutter's Camp #2 |
| 7 | Civic I — City State | 14 | Commerce I — **Barter** |
| 8 | citizens → 9 ("→ Timber") | 15 | Market #1 |
| 9 | Farm | 16 | citizens → 14 ("→ Timber") |
| 10 | City #2 | 17 | farms → 7 |
| 11 | citizens → 11 | 18 | **Classical Age** |
| 12 | Farm | | |

Run through the same model:

| | shipped `economic.bhs` | best found by search |
|---|---|---|
| Commerce I (Barter) | **2:24** | **0:03** |
| Science I (Written Word) | 0:00 | 3:16 |
| Civic I (City State) | 0:13 | 0:16 |
| City #2 | 0:56 | 1:00 |
| Market #1 | 2:37 | 2:28 |
| Classical Age queued | 4:39 | 3:33 |
| **target state complete** | **5:05** | **4:00** |

**The shipped order is 66 s slower — 5:05 against 4:00, +27 % — and essentially the whole gap is one decision.**
Both orders spend the Library's time from frame 0 — the Library is a free server, so the
question is only *which* tech goes first. The shipped AI spends it on **Written Word**:
120 timber + 50 wealth for, in the Ancient age, no economic return at all (it unlocks the
Temple and the Science column). The search spends it on **Barter**: 60 food + 60 timber,
which raises the per-resource income ceiling from 70 to 100 — a 43 % lift on the binding
constraint — and unlocks the Market, the only Ancient-age source of wealth. Written Word's
50 wealth then has to come out of the fixed 100 the player starts with, which delays
everything that needs wealth until a Market exists.

### Why the designers may still be right, and I am not claiming they are wrong

Four reasons the shipped order can be correct for a game and wrong for my model:

1. **My model has no opponent.** Written Word opens the Science column and the Temple, which
   is territory and border pressure. `economic.bhs` is a *general-purpose* boom script that
   is also the fallback when the player is attacked (its very first branch builds a Barracks
   on `was_city_attacked`). An order that is a minute slower economically but much safer against
   an early rush is a rational trade my objective cannot see.
2. **My model has no map.** Barter also unlocks the Dock, and the script's sea-map branch
   goes to the Dock at case 35 immediately after Barter. On a water map the Commerce-first
   ordering may already be what the script does, by a different route.
3. **My wealth model is impoverished.** Caravans, merchants, rare resources and territory
   taxes are all real Ancient-age wealth and none of them is in the model (§4). The script
   builds caravans continuously (`caravan_lim = n(n−1)/2`) and merchants at 100 wealth. If
   caravans are worth much, expansion looks better than my §6 says, and Written Word's
   wealth cost hurts less.
4. **The commerce clamp reading is the model's weakest link** (§3). Barter's entire value in
   my model is +30 per resource per 30 s of ceiling. If H1 is wrong in either direction, the
   ordering claim moves with it — and §5.2 shows halving the clamp costs 74 s, which is the
   same order as the gap I am attributing to Barter.

What I will claim is narrower and I think it survives all four: **under a commerce clamp
that binds at 70 per resource per 30 s, Commerce I is the highest-value first tech in the
Ancient age, and Science I is close to the lowest.** That is a testable prediction about the
real game — run two AI games with `case 6` and `case 14` swapped and compare resource totals
at 10:00 — and it is cheap to run now that `donhook`/`donject` exist.

### A real recorded game, for scale

`ron-data/replays/today.rcx` (`analysis/replay_order.py`), a single-player game. Every
type id in the command stream resolves against `schema/live/*-attributes.txt`, which is an
independent artefact — that is a cross-check that could have failed:

| t | what | t | what |
|---:|---|---:|---|
| 0:14–0:21 | 6 × Citizen | 3:43 | 2 × Citizen |
| 0:31 | Farm | 3:48 | Written Word |
| 1:47 | Woodcutter's Camp | **7:23** | **Classical Age** |
| 1:49–1:50 | 4 × Citizen | 7:27–7:32 | 5 × Farm |
| 2:00 | Farm | 7:34–7:35 | 7 × Citizen |
| 2:35 | City State | 7:46–7:58 | Library, Temple, Market, University, Mine |
| 3:15 | City #2 | 8:29 | Mathematics, The Art of War |

The recorded human reaches the Classical Age at **7:23**, against 4:39 for the shipped AI
order and 3:33 for the search, in the same model. That ordering — human slower than the
scripted AI, scripted AI slower than the search — is what one would expect, and it is the
only external sanity check on the absolute time scale that this lane has. It is one game.

---

## 8. What I could **not** establish

* **The commerce clamp's scale (§3).** H2 is refuted; H1 is *assumed*, not measured. The decisive experiment is a live read of `econ+0x30` (the cap) and `econ+0x64` (the gross income) in a running match, plus watching `econ+0x18` (the accumulator) roll over. I attempted it and did not finish. Three things are worth recording for whoever picks it up:
  * **`prlctl exec` hangs on long command lines.** A `-EncodedCommand` payload of 3121 bytes returned in 3.5 s; payloads of 6.6 KB and 8 KB never returned at all, three times. Keep the script under ~3 KB of base64, use `Reflection.Emit` to define the `ReadProcessMemory` P/Invoke (an `Add-Type` block costs a csc compile *and* pushes you over the limit), write a **binary** dump to a guest file, and stream it out with `certutil -encode` + `type`. That path works and dumped 57 KB and 196 KB reliably in under 4 s.
  * **`econ` is a pointer.** `economy.md` §3.3 says "the econ sub-block is `player + 0x6EB8`"; the code loads it (`mov ecx, [edi + 0x6eb8]` at `0x006CE4C2`, `in_ECX[0x1bae]` in `FUN_006CE280`), so it is `*(player + 0x6EB8)`. Correcting for that got me a candidate pointer per player but the 512 bytes at it decoded to zeros.
  * **A signature search is the cheap way in.** The knowledge commerce cap is hardcoded 999 (`0x006CE92C`), stored obfuscated as `999 ^ 0x1281 = 0x1166`, so the econ block is findable by scanning for that dword. I scanned 196 KB around the candidate pointer and the whole 57 KB of players 0–1 and found **zero** hits, which means the block is neither inline in the player struct nor near that pointer. A whole-heap scan (donscan can already walk the regions) would settle it in one pass.
* **Whether `econ+0xF0` is the Commerce column or the Science column.** Both are consistent with the four-column structure; only the constant's name and the tech tree's shape pick Commerce.
* **Which `RAMP_MAX` class each building falls into.** This is the largest single lever in §5.2 (−69 s) and it is a class lookup in `FUN_00664090` I did not resolve.
* **Gather slots per building.** Terrain-derived inside `FUN_00639E40`'s slot walk; not in any data file. The Woodcutter's Camp default of 5 comes from the shipped AI's own `min_size`, which is a designer's target, not an engine value.
* **`SCHOLAR_RATE`.** Offset 644, shipped `[5,7,10,15,20]`, and a linear capstone scan of `.text` found **no read site at all**, indexed or otherwise. Either it is dead in this build or the scan mis-decoded its site. Scholars are modelled as ordinary gather slots.
* **Unit train-time ramping.** `FUN_006508C0` computes `clamp(base + count·JOB_EXTRA_TIME·UNIT_RATE_PROGRESSION, 0, 3·base)`, and with the *live* `JOB_EXTRA_TIME = 10` (F4) that saturates the 3× clamp after a single unit. Both prior lanes say do not implement it as train time; I did not.
* **Caravans, merchants, rare resources, territory taxes.** All are real Ancient-age income and none is derived. `FUN_006CEEE0`'s `0x1A2`/`0x1A3` loop (caravan/merchant) and its 44-entry rare-resource loop via `FUN_006E08D0` are the next targets.
* **Starting citizens and starting resources for a real match.** Both inferred (from `economic.bhs`'s arithmetic and from `STARTING_GOODS`); both are game options the replay header probably encodes and I did not decode.

---

## 9. Ledger deltas requested

Additions:

* **Worker → income composition**, `FUN_006CEEE0` / `FUN_00737C60` / `FUN_006D5530` / `FUN_00639E40` — [measured] structural, **not** differentially tested. One worked gather slot = `PEASANT_RATE/256` = 10 resources per `GATHER_RATE`; `CITY_GATHER` added per city and **not** scaled by the granary/lumber-mill/smelter bonus; `MARKET_TAXES` and `UNIVERSITY_LITERACY` added per city that holds type 436 / 420.
* **Resource slot order** 0 food, 1 timber, 2 wealth, 3 knowledge, 4 metal, 5 oil — [measured], three independent witnesses (§2.3).
* **Cost + ramp shape**, `FUN_00664090` — [measured] structural: `cost = COST·FACTOR + min(SUPPORTVALUE·(owned+queued) + extra, COST·FACTOR·RAMP_MAX/100)`, ceiling 0 = no ceiling.
* **`econ` is a pointer at `player + 0x6EB8`**, not an inline block.

Corrections to `docs/derivation/economy.md`:

* §3.3 — `COMMERCE_CAP` is **not indexed by the age**. `econ+0xE8/0xEC/0xF0/0xF4` are the four library-column levels; `econ+0xE8` indexes `POP_CAP`, `econ+0xEC + 1` is the city limit (`FUN_006D6130`, whose bonus constants are literally `pyramids_city_limit` and `bantu_city_limit`), and `econ+0xF0` indexes `COMMERCE_CAP`.
* §3.3 — "the econ sub-block is `player + 0x6EB8`" should be "`*(player + 0x6EB8)`".
* §4.2 — `JOB_EXTRA_TIME` for the Citizen is **10** in live memory, not 1; `unitrules.xml` has scaled fields of its own and `"1/10tsx"` at scale 100 gives 10.

---

## 10. Files written

| path | what |
|---|---|
| `analysis/derive.py` | collects the derived quantities + live type tables into `analysis/derived.json`, with provenance strings |
| `analysis/derived.json` | generated; 300 units, 64 buildings, 85 techs, 34 rules arrays |
| `analysis/econ.py` | the frame-stepped economy: accumulator, income composition, caps, cost ramp, and the `Assumptions` block |
| `analysis/plan.py` | three-server discrete-event simulator + beam search |
| `analysis/study.py` | every table in this report (`S1`…`S7`) |
| `analysis/replay_order.py` | frame-stamped build order out of a `.rcx` recorded game |
| `docs/tracks/build-order-analytics.md` | this file |

Nothing else was written; nothing was committed or staged; no Rust was touched.
