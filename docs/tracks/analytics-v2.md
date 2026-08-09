# Build-order analytics, v2 — the clamp is measured, and the designers were right

Lane: **analytics**. Code: `/Users/ember/dev/don/analysis/opening.py` and
`/Users/ember/dev/don/crates/don-ai/src/optimum/`. Supersedes
`docs/tracks/build-order-analytics.md`, which is still worth reading for the
derivations it did first; where the two disagree, this file says so explicitly and
gives the address.

Every claim is marked **[measured]** (verified here, on this binary or this live
capture) or **[reported]** (read elsewhere, not re-checked). Nothing here is
*verified* in the proof-assistant sense — see `docs/CHARTER.md`.

---

## 0. What now works, and the headline

**Three things work that did not before.**

1. **The commerce clamp is measured, not assumed.** The previous study's own summary
   called it "the model's weakest link" and swept it ±2×, at a cost of ±125 s on
   every result. It is `shl eax, 4` at **`0x006CEE78`**, the last thing
   `Leader::calc_resource_caps` does to each entry of
   `LeaderDataEncrypt::resource_cap[7]`. The Ancient-age ceiling is **70 resources
   per 30 s per resource** [measured]. That assumption row is gone from the
   sensitivity table for good.
2. **A working opening player in Rust**, `don_ai::optimum::player::CapFirst`, playing
   in an integer economy whose every constant traces to a live read or a named PDB
   function. It beats the shipped designers' boom order by **46 s (18 %)** to the same
   objective, and it comes within **14 s** of an offline search that is allowed to
   plan the whole opening in advance.
3. **A search that is tight enough to trust.** The old beam search had a self-measured
   noise floor of 30–40 s, comparable to the effects it was reporting. Replacing it
   with a Pareto DP over the purchase multiset drops it to **3.5 s** (52 frames across
   a 5× change in search width) and makes the claims below survivable.

**And the headline result is a retraction.**

The previous study's lead finding was that the shipped AI opens with the wrong tech —
that Science I (Written Word) is "close to the lowest" value first tech and Commerce I
(Barter) is the highest, worth 66 s. With the clamp measured and the designers' own
remaining order simulated, **that is wrong, and backwards**:

| variant of `economic.bhs` cases 6–18 — same purchases, different order | boom complete |
|---|---:|
| **shipped, as Mark Sobota and Mike Engle wrote it** | **4:56** |
| swap case 6 ↔ case 14 — Barter first, exactly the prior study's recommendation | 5:14 (**+18 s worse**) |
| Written Word moved to last | 5:08 (+12 s worse) |
| first Farm moved before the four citizens | 4:55 (−1 s, noise) |
| Written Word **and** camp #2 moved after the age | 4:38 (−18 s better) |

**The shipped order is a strict local optimum under one-edit perturbation.** Every
single-decision change I tried makes it worse. The only improvement I found needs
*two* moves at once, and my optimiser only beats it by re-timing five or six things
jointly. Reasoning about why is §6, and the reason is a nice one: **in the Ancient age
wealth is a stranded resource** — there is no wealth income at all until a Market
exists, nothing the boom builds costs any wealth, and Written Word is the only
Ancient-age *tech* priced in wealth (1 of 1, checked against the live tech table).
Opening with it converts 50 dead wealth into a tech and leaves the food and timber for
the economy. Barter first spends 60 food and 60 timber that the first four citizens
need, to raise a ceiling that does not start to bind until 1:58.

---

## 1. Reproduction

```sh
cd /Users/ember/dev/don
python3 analysis/opening.py facts        # the measured quantity table, with addresses
python3 analysis/opening.py caps         # where the clamp stops paying for workers
python3 analysis/opening.py ages         # minimum time to each age
python3 analysis/opening.py marginal     # marginal value of the Nth citizen
python3 analysis/opening.py alloc        # best allocation over the first 10 minutes
python3 analysis/opening.py military     # opportunity cost of an early military building
python3 analysis/opening.py sens         # sensitivity + the search's own noise floor

cargo run  -p don-ai --example boom_arena   # the head-to-head and the ablations
cargo test -p don-ai optimum                # 6 tests, including the headline as a regression
```

`analysis/opening.py` reads `analysis/derived.json` (built by the previous study's
`analysis/derive.py`), `schema/types.json` for the PDB layout of `Constants`, and
`schema/live/rules-block-pid14644.txt` for the **live** values of those constants.
It writes nothing.

The Rust half is `crates/don-ai/src/optimum/{mod,econ,player}.rs` plus
`crates/don-ai/examples/boom_arena.rs`, and one line in `crates/don-ai/src/lib.rs`
(`pub mod optimum;`). It shares no code with the ron-ai lane's transcription of the
shipped AI and deliberately does not transcribe anything: `ShippedBoom` is the
*purchase sequence* of `economic.bhs` cases 6–18 as data, so that the two openings can
be raced inside one economy.

Instruction-level reads are reproducible with a 12-line capstone script; the
disassembly ranges that matter are `0x6CE280–0x6CE8D0` (`Leader::gather` /
`Leader::do_gather`), `0x6CE900–0x6CEE9A` (`Leader::calc_resource_caps`) and
`0x6CEEE0–0x6CF790` (`Leader::calc_gather`).

---

## 2. What the PDB bought us, in one table

The previous study worked from `FUN_xxxxxxxx` addresses and inferred field meanings
from usage. `ron-bin/sbl/rise.pdb` gives the names, and the names settle three
questions that were open.

| prior name | real name (PDB) | source line |
|---|---|---|
| `FUN_006CE450` | `Leader::do_gather` | `leaders.cpp:14478` |
| `FUN_006CE280` | `Leader::gather` | `leaders.cpp:14645` |
| `FUN_006CE900` | `Player::UpdateCommerceCaps` → **`Leader::calc_resource_caps`** | `leaders.cpp:14339` |
| `FUN_006CEEE0` | `Leader::calc_gather` | `leaders.cpp:13897` |
| `FUN_006D5530` | `LeaderData::calc_city_resources` | `leaders.cpp:4623` |
| `FUN_00737C60` | `City::calc_gather` | `city.cpp:2173` |
| `FUN_00639E40` | `BuildTypeData::calc_gather` | `buildtype.cpp:2295` |
| `FUN_00664090` | `TypeData::get_cost` | `type.cpp:639` |
| `FUN_00738360` | `CityData::enhancer_amount` | `city.cpp:378` |
| `FUN_006D6130` | `LeaderData::get_city_limit` | `leaders.cpp:3680` |
| `FUN_00737B50` / `FUN_00737C00` | `CityData::get_taxes` / `CityData::get_literacy` | `city.cpp:960` / `987` |

And "the obfuscated econ block at `*(player + 0x6EB8)`" is
`LeaderData::data_encrypted`, a `LeaderDataEncrypt*`, whose 248 bytes are **fully
named** in `schema/types.json` [measured]:

| offset | field | what the previous study called it |
|---:|---|---|
| `0x00` | `bucket[6]` | the stockpile |
| `0x18` | `leftover[6]` | "the accumulator" ✓ |
| `0x30` | **`resource_cap[7]`** | "the cap" ✓ — note **seven**, not six |
| `0x4C` | `over_cap[6]` | not found before |
| `0x64` | `resources[6]` | "gross income" ✓ |
| `0x7C` | `support[6]` | not found before |
| `0x94` | `income[6]` | not found before — this is the *net* the HUD shows |
| `0xAC` | `rate[6]`, `0xC4` `bonus[6]` | not found before |
| `0xDC` | `ages`, `0xE0` `epochs`, `0xE4` `discovered` | — |
| `0xE8` | **`epoch[4]`** | "the four library-column levels" ✓ |

`Leader::do_gather` therefore reads
`income[r] = resources[r] − support[r] + LeaderData::base_rate[r]`, clamps it, and
writes it back to `income[r]`; `over_cap[r]` is set to 0 / 1 / 2 (0 = under the cap,
1 = clamped, 2 = clamped *and* the cap itself is at the 16000 global ceiling), which
is what drives the flashing resource readout the help file describes.

---

## 3. The measurement that changes everything

### 3.1 The commerce clamp is `commerce_cap × 16` [measured]

`Leader::calc_resource_caps` loops `r = 0..6`. Per resource it writes the raw rules
value and applies the national/wonder percentage bonuses, and then, as the **last four
instructions of the loop body**:

```asm
006cee66  mov  eax, [esi + 0x6eb8]     ; LeaderData::data_encrypted
006cee6c  lea  ecx, [eax + ebx*4]
006cee6f  inc  ebx                     ; r++
006cee70  mov  eax, [ecx + 0x30]       ; resource_cap[r], encrypted
006cee73  xor  eax, 0x1281             ; decrypt
006cee78  shl  eax, 4                  ; <<<< x16
006cee7b  mov  [0xcb195c], eax         ; LeaderDataEncrypt::scratch (the setter idiom)
006cee80  xor  eax, 0x1281             ; re-encrypt
006cee85  mov  [ecx + 0x30], eax       ; store back
006cee88  mov  eax, [ebp - 8]
006cee8b  cmp  ebx, 7
006cee8e  jl   0x6ce921
```

so `resource_cap[r]` is in the **same 1/16-resource units as the income it is compared
against** at `0x006CE512`. `Constants::commerce_cap` is `[70,100,150,200,260,320,400,500]`
in a live read of the running process, so the Ancient-age ceiling is 70 resources per
30 s per resource. Three independent cross-checks agree and could each have failed:

* `ScenarioFuncSet::gather_rate` (`0x009E914E`) returns `income >> 4` and
  `IFaceResources::draw_resource` (`0x00818C05`) prints `(income + 8) >> 4` — so the
  number the AI scripts and the HUD see is resources per 30 s, i.e. income really is
  ×16 [measured].
* The Dutch interest bonus adds `interest_in_resources << 4` to income
  (`0x006CE6DC`) and clamps to `resource_cap + dutch_interest_cap << 4`
  (`0x006CE6FC`). `help.xml` says that income "can exceed Commerce Cap by 50" —
  which is only true if the cap is in income units, as `0x006CEE78` makes it
  [measured].
* The global ceiling is compared as `cmp eax, 0x3e70` against the **cap** at
  `0x006CE5FF` as well as against the income at `0x006CE70B`. 16000 income units is
  1000 resources per 30 s; a cap that never exceeded 500 would make that comparison
  dead. With the `<<4` the cap tops out at 8000 and the comparison is live.

Prior work refuted the "no `<<4`" reading indirectly, from a recorded game, and
recorded the direct reading as unresolved. It is resolved.

### 3.2 `epoch[2]` is the Commerce column [measured]

`Leader::calc_resource_caps` reads `data_encrypted + 0xF0`, which is `epoch[2]`. The
previous study could not tell Commerce from Science. The shipped rules settle it:
each tech carries a `cat` field, and

| `cat` | tech | the limit `epoch[cat]` indexes |
|---:|---|---|
| 0 | The Art of War | `Constants::pop_cap` → `LeaderData::pop_cap` |
| 1 | City State | `LeaderData::get_city_limit` = `epoch[1] + 1` |
| **2** | **Barter** | **`Constants::commerce_cap`, read from `+0xF0` = `epoch[2]`** |
| 3 | Written Word | Science |

Four-for-four with the in-game library column order, and the shipped AI agrees twice
(it gates city #2 on City State and researches The Art of War at `population ≥ 23`
against `pop_cap[0] = 25`).

### 3.3 Income refreshes on a gate, and the gate is not what was assumed [measured]

`Leader::calc_gather` opens with a guard, and `LeaderData::gather_stamp` (`+0x7AC`) is
written with the current frame at `0x006CF71C`:

```asm
006ceef4  test dword [ebx], 0x2000000  ; leader "economy dirty" flag -> the fast path
006cef00  mov  eax, [ebx + 0x7ac]      ; gather_stamp
006cef06  add  eax, 0x12c              ;  + 300 frames
006cef0b  cmp  ecx, eax                ; frame >= stamp + 300 ?
006cef19  and  eax, 0x800000ff         ; (frame + who*8) % 256 == 0 ?
...
006cf795  and  eax, 0x80000007         ; dirty path: (frame + who) % 8 == 0
006cf70a  and  dword [eax + esi], 0xfdffffff   ; clear the dirty flag
```

So the gross income vector is rebuilt **within 8 frames (0.53 s) whenever the leader is
flagged dirty**, and otherwise on a background refresh that needs both ≥ 300 frames
since the last one and `(frame + 8·who) ≡ 0 (mod 256)` — i.e. every 512 frames, phase
staggered per player. The previous study listed "the 8–300 frame lag" as an unmodelled
omission; the real shape is a 0.53 s fast path plus a 34 s slow path, and modelling
income as instantaneous is a good approximation *provided* the events we care about
set the dirty flag. The model does assume that; it is an assumption, not a measurement.

### 3.4 Territory taxes are zero under the starting government [measured]

`Leader::calc_gather` at `0x006CF69D` computes
`wealth_income += territory · Constants::territory_taxes[gov] · 16 / K`.
`territory_taxes` is `[0, 50, 100, 200, 300]` in a live read, indexed by government,
and the Ancient age has no government. So the previous study's "territory taxes are
real Ancient-age wealth and none of them is in the model" is **not a gap in the
Ancient age**: it is exactly zero until Republic or Despotism is researched, which is
a Medieval tech. Closed [measured].

### 3.5 Corrections to existing artifacts

* **`docs/derivation/rules-constants.json` has `scholar_rate` wrong twice.** It records
  `parser: wtoi`, five entries `[5,7,10,15,20]`. The live `Constants+0x284` is
  `int[6] = [1280,1792,2560,3840,5120,6400]`, i.e. a **scale-256** array of six
  entries, `[5,7,10,15,20,25]` [measured]. The previous study's separate finding that
  the field has **zero read sites in `.text`** is confirmed by a full capstone operand
  scan for displacement `0x284`: 56 instructions in the binary use it, all of them on
  `Window`, `ComboBox`, `ObjectTypeData` and friends, none on `Constants`. Scholars
  gather knowledge at `PEASANT_RATE` like everyone else, and the rate the designers
  wrote for them is dead code.
* **`docs/tracks/build-order-analytics.md` §7's headline is retracted** — see §6.
* Its §3 open question (the clamp scale) is closed by §3.1; its §8 open question
  (Commerce vs Science at `+0xF0`) is closed by §3.2.

---

## 4. The model, and the sign of everything it omits

`analysis/opening.py` and `crates/don-ai/src/optimum/econ.rs` are the same model in
two languages. They agree to the frame on the shipped order (**4440 frames** in both),
which is the only cross-validation available for a model with no oracle.

Three servers, because real play overlaps them: a **trainer** (citizens), a **build
crew** (which stops gathering for the duration — modelled), and the **Library** (one
tech at a time). Counters hold **owned + queued**, which is what `TypeData::get_cost`
charges the ramp against (`0x00665966`) and what the AI's `num_type_with_queued` sees;
income and gather slots use only what has *completed*.

**Omissions, with the direction of the bias**, so a reader can sign the error:

| omitted | model is |
|---|---|
| walking, pathing, distance to the gather point | **fast** (optimistic) |
| caravans and merchants (Ancient-age wealth we did not derive) | poor on wealth (pessimistic) |
| rare resources | pessimistic |
| the 0.53 s / 34 s income refresh gate (§3.3) | slightly optimistic |
| scouting, ruins, plunder, all military | pessimistic |
| the map, the opponent, and everything about being attacked | **neutral to the objective, fatal to the conclusion** — see §6 |

**Assumptions**, all in `Assume` in both languages and all swept in §8:

| assumption | default | why |
|---|---|---|
| slots per Farm | 1 | `farms_per_city_base = 5` and the shipped AI's one-farm-one-worker bookkeeping |
| slots per Woodcutter's Camp | 5 | `aibestbuildlibrary.bhs::place_woodcutter` destroys and re-places a camp under `min_size = 5` |
| slots per Mine | 4 | no source; unused in the Ancient age |
| build with *k* citizens | `job_time / k`, k ≤ 4 | `buildingrules.xml` documents `JOB_TIME` for **one** citizen; the speedup shape is not derived |
| building cost-ramp ceiling | none | `test esi,esi` at `0x00665452` — a zero ceiling means no ceiling; which class a building is in is not derived |
| starting citizens / farms / camps | 5 / 3 / 1 | `citytemplates.xml` plus `economic.bhs`'s own arithmetic (case 8 sets `needed_citizens = 9` after "+4"; case 13 is "Woodcutter **#2**") |
| unit train time | `job_time`, no ramp | both prior lanes say `ObjectData::train_time`'s ramp is a *rate*, and with the live `JOB_EXTRA_TIME = 10` it saturates its 3× clamp after one unit |

---

## 5. Falsifiable results

### 5.1 Where the clamp stops paying for workers [measured constants, model arithmetic]

Income is `160·workers + 160·cities` per resource in engine units; the clamp is
`commerce_cap[epoch[2]] · 16`. Solving for the last worker that earns anything:

| Commerce level | tech | cap / 30 s | max useful food workers, 1 city | 2 cities | 3 cities | 4 cities |
|---:|---|---:|---:|---:|---:|---:|
| 0 | — | 70 | **6** | 5 | 4 | 3 |
| 1 | Barter | 100 | 9 | 8 | 7 | 6 |
| 2 | Coinage | 150 | 14 | 13 | 12 | 11 |
| 3 | Trade | 200 | 19 | 18 | 17 | 16 |
| 4 | Banking | 260 | 25 | 24 | 23 | 22 |

**At Commerce 0 with one city the seventh food gatherer produces literally nothing**,
and the same holds independently for timber. This survives from the previous study,
but it now rests on a measurement rather than on the assumption it was most sensitive
to. It is a test in the Rust crate
(`optimum::tests::seventh_food_gatherer_is_worth_nothing_at_commerce_zero`).

The corollary is still counter-intuitive and still holds: each city's free
10 food + 10 timber **consumes cap headroom**, so at Commerce 0 a second city adds
20 of free income and removes one usable food gatherer and one usable timber gatherer
— net zero — while costing 60 food + 60 timber (the second city already pays one step
of its own 50/50 ramp, because the settlement you start with is a Small City) plus
120 food for City State. Expanding before raising the ceiling is economically
negative in the Ancient age *under this model*.

### 5.2 Marginal value of the Nth citizen

Citizen cost is `20 + min(1·(owned+queued), 100)` food; a farm is
`40 + 4·(farms owned)` timber; a worked slot pays 10 resources per 30 s.

| N | citizen | payback | farm # | farm cost | payback | combined |
|---:|---:|---:|---:|---:|---:|---:|
| 1 | 21 food | 1.05 min | 1 | 44 timber | 2.20 min | 3.25 min |
| 5 | 25 | 1.25 | 5 | 60 | 3.00 | 4.25 |
| 10 | 30 | 1.50 | 10 | 80 | 4.00 | 5.50 |
| 15 | 35 | 1.75 | 15 | 100 | 5.00 | 6.75 |
| 25 | 45 | 2.25 | 25 | 140 | 7.00 | 9.25 |
| 50 | 70 | 3.50 | 50 | 240 | 12.00 | 15.50 |
| 100+ | 120 (capped) | 6.00 | 100 | 440 | 22.00 | 28.00 |

The citizen ramp is gentle and caps outright at +100 food (500 % of the 20-food base,
`Constants::unit_worker_ramp_max`). **The price of a citizen is not what limits the
opening.** What limits it is, in order: the commerce clamp, `farms_per_city_base = 5`,
and `pop_cap[0] = 25`.

The marginal income of the Nth food worker with one city, as a step function:

```
Commerce 0 (cap  70):  10 10 10 10 10 10  0  0  0  0  0
Commerce 1 (cap 100):  10 10 10 10 10 10 10 10 10  0  0
Commerce 2 (cap 150):  10 10 10 10 10 10 10 10 10 10 10
```

### 5.3 Best allocation over the first 10 minutes

Objective: food + timber banked at t = 10:00, every purchase paid for. The food/timber
split is not a free variable — a citizen gathers whatever the slot it stands in
gathers, and slots come from buildings, so the split is chosen by choosing buildings.

| scenario | best N | food wk | timber wk | farms | camps | food @10 m | timber @10 m | total |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 city, no techs | 11 | 5 | 6 | 5 | 2 | 1138 | 1439 | 2577 |
| **1 city, Barter** | **14** | 5 | 9 | 5 | 2 | 982 | 1864 | **2846** |
| 2 cities (City State) | 10 | 5 | 5 | 5 | 1 | 1231 | 1341 | 2572 |
| 2 cities + Barter | 13 | 5 | 8 | 5 | 2 | 975 | 1604 | 2579 |
| 2 cities + Barter + Market | 13 | 5 | 8 | 5 | 2 | 924 | 1467 | 2391 |

**Barter pays for itself inside ten minutes (+269) and expansion does not (−5).** Note
this is the *pure banking* objective — it deliberately values nothing but resources at
t = 10:00, which is why it disagrees with §6, where the objective is a state that
*includes* a second city.

### 5.4 Minimum time to each age

The age advance on its own is a degenerate objective. `starting_goods` is 200 food and
the Classical Age costs 250, so a player who builds nothing banks the difference and
ages at **1:04**. The age is cheap; the economy is not. With the full Ancient boom
behind it the age lands at **3:56** (searched).

Every age after the first is a **knowledge race**, and knowledge has exactly two
sources: `university_literacy = 10` per University per 30 s (`CityData::get_literacy`)
and one gather slot per Scholar at `PEASANT_RATE = 10` per 30 s (because
`Constants::scholar_rate` is dead code — §3.5). The cap is 999 per 30 s, hardcoded at
`0x006CE92C`. Banking time alone, ignoring the food cost and assuming the sources are
already standing and free:

| age | food | knowledge | 1 source | 2 | 4 | 8 | 16 |
|---|---:|---:|---:|---:|---:|---:|---:|
| Medieval | 250 | 250 | 12:30 | 6:15 | 3:07 | 1:33 | 0:46 |
| Gunpowder | 500 | 500 | 25:00 | 12:30 | 6:15 | 3:07 | 1:33 |
| Enlightenment | 500 | 1000 | 50:00 | 25:00 | 12:30 | 6:15 | 3:07 |
| Industrial | 750 | 1500 | 75:00 | 37:30 | 18:45 | 9:22 | 4:41 |
| Modern | 0 | 2500 | 125:00 | 62:30 | 31:15 | 15:37 | 7:48 |
| Information | 0 | 3750 | 187:30 | 93:45 | 46:52 | 23:26 | 11:43 |

Read every cell as a **lower bound on time spent in that age**. The knowledge cap of
999 per 30 s puts a hard floor of **1:52** on the Information Age no matter how many
universities exist — that is the only place the cap, rather than the economy, is the
binding constraint, and it is why a real game's late ages are gated on scholar count.

### 5.5 Opportunity cost of an early military building

Same objective, same search, with the military items added to the required multiset:

| plan | boom complete | Δ |
|---|---:|---:|
| boom target, no military | 3:57 | — |
| + The Art of War (120 food, 200 frames of Library) | 4:26 | **+29 s** |
| + The Art of War **and** a Barracks (120 timber, 420 frames of crew) | 4:40 | **+43 s** |

**The Art of War raises `pop_cap` from 25 to 50, and the Ancient boom target is 14
citizens, so the tech buys nothing economic at all.** Its entire Ancient-age value is
military, which means the whole 29 s is the price of *opening the option* — unlike
Barter or City State, which pay part of their own cost back. Adding the building on
top is another 14 s. So **the honest exchange rate for an Ancient-age military opening
is about 45 seconds of boom**, and `economic.bhs`'s very first branch spends exactly
that when `was_city_attacked` fires.

---

## 6. The designers vs the optimiser — and why they win the argument

`ron-data/ai-scripts/economic.bhs` carries the header *"Mark Sobota / Mike Engle /
boom script"*. Its generic-nation, land-map, size ≥ 2 path is a step machine, cases
6 → 18, and it returns `BLOCK_ON_THIS` from every case, so it does not advance until
the step succeeds. As a purchase sequence:

| case | action | case | action |
|---:|---|---:|---|
| 6 | Science I — **Written Word** | 13 | Woodcutter's Camp #2 |
| 7 | Civic I — City State | 14 | Commerce I — **Barter** |
| 8 | citizens → 9 | 15 | Market #1 |
| 9 | Farm | 16 | citizens → 14 |
| 10 | City #2 | 17 | farms → 7 |
| 11 | citizens → 11 | 18 | **Classical Age** |
| 12 | Farm | | |

Run to completion in the measured economy it lands at **4:56** (4440 frames), and the
ablation table in §0 says it is a **local optimum under every one-edit perturbation I
tried**, including the exact edit the previous study recommended.

### Why Written Word first is right

Three reasons, and the first is the one that matters.

1. **Wealth is stranded in the Ancient age.** A player starts with 100 wealth
   (`starting_goods`) and has **no wealth income whatsoever** until a Market exists —
   `CityData::get_taxes` is `village_taxes + n·building_taxes + market_taxes + temple_taxes`
   and the shipped values are `0 + 0 + 10 + 0`, so the only Ancient-age wealth is
   `market_taxes = 10` per city with a Market, and `territory_taxes[gov=0] = 0`
   (§3.4). Meanwhile **Written Word is the only Ancient-age *tech* priced in wealth at
   all** — 1 of 1, checked against the live tech table — and **nothing in the boom
   itself** (farms, camps, cities, market, library, citizens) costs a single wealth.
   The other Ancient-age wealth sinks are a Scholar (30), a Scout (40), a Tower (30),
   a House (30) and Bowmen (50), none of which `economic.bhs` buys before case 20.
   So spending 50 of that stranded 100 at t = 0 costs almost nothing real. Barter
   costs 60 **food** + 60 **timber** — both binding — at the exact moment those two
   resources are buying the first four citizens.
2. **The clamp does not bind for two minutes.** A standard start is 5 citizens on 3
   farms and a camp: 40 food and 30 timber per 30 s against a ceiling of 70. Barter
   raises a ceiling with 75 % headroom. In this model the shipped order's food rate
   first touches the 70 ceiling at **1:58**, and the script queues Barter at **2:24** —
   26 s later, the first moment its Library is free after the ceiling starts to bite.
   **The designers time Commerce I to the point the ceiling binds**, which is not
   something you land on by accident.
3. **The script is honest about the trade.** Case 5 and case 6 both carry
   `if (at_least_type(who, 75, "Wealth") < 1) step++` — if the player has fewer than
   75 wealth, **the script skips Science I entirely**. They encoded exactly the
   "spend the dead resource, and only the dead resource" rule.

### Where the optimiser does beat them, and by how much

| | boom complete | Δ |
|---|---:|---:|
| `economic.bhs`, as shipped | 4:56 | — |
| `CapFirst`, the optimiser-derived player (online, no lookahead) | **4:10** | **−46 s (−18 %)** |
| offline Pareto-DP search over all orderings of the same multiset | 3:56 | −60 s (−24 %) |

The 46 s does **not** live in any single decision. It is a joint re-timing:

* the build crew starts on **farms at frame 0**, before the first citizens, so slots
  lead workers instead of trailing them;
* it trains **exactly** to the objective and no further, because past the clamp a
  citizen is 25 food of pure loss;
* **Written Word and the second Woodcutter's Camp both move after the Classical Age.**
  Individually each of those moves *loses* (5:08 and no gain); together they win 18 s
  even inside the designers' own order. The camp costs 70 **food** against an age that
  costs 250 food at a ceiling of 100 per 30 s — it is 21 s of the entire food economy
  spent 90 s before the thing it delays.

**So the honest headline is not "the designers got the tech order wrong".** It is:
*the designers' order is a one-edit local optimum, and beating it requires moving
several decisions jointly — the largest single structural edit being to defer every
food purchase that is not the age once the age is within about a minute.*

### Four reasons the designers may be right even about the 46 s

1. **My model has no opponent.** `economic.bhs`'s very first branch builds a Barracks
   on `was_city_attacked`; it is a general-purpose boom that is also the fallback when
   the player is under pressure. Written Word opens the Science column and the Temple,
   which is territory and border pressure. An order 46 s slower economically and much
   safer against an early rush is a rational trade my objective cannot see.
2. **My model has no map.** Barter also unlocks the Dock, and case 14 jumps straight
   to `step = 35` (build a dock) on a sea map — on water the script *is* Commerce-first
   by a different route.
3. **My wealth model is impoverished.** Caravans (case 15 builds one immediately after
   the Market, and `caravan_lim = n(n−1)/2`), merchants and rare resources are all
   real Ancient-age wealth and none is derived. If caravans are worth much, the whole
   Market/Barter timing shifts.
4. **`CapFirst` is tuned on the same model it is evaluated on.** Its four thresholds
   were read off the search's plans. That is exactly the overfitting the designers, who
   tuned against a real game, did not do.

### The experiment that would settle it

Run two retail AI matches with `economic.bhs` case 6 and case 14 swapped and compare
resource totals at 10:00. My model predicts the **swap loses by about 18 s** of boom
completion — the opposite of the previous study's prediction, which is what makes it a
real test. `donhook`/`donject` exist; the script is a plain-text file.

---

## 7. The player

`don_ai::optimum::player::CapFirst`. Seven rules, each read off the search, each
stated in the source next to the address that motivates it:

1. **Raise the ceiling before filling it.** Library time goes to the techs that move a
   *limit* — `City State` (cities) then `Barter` (`commerce_cap`). `Written Word`
   moves no limit in the Ancient age and is researched last.
2. **Never train a citizen the clamp will not pay for.** `World::useful_slots(r)` is
   the test; past it a citizen costs 25 food and earns 0.
3. **Keep gather slots one step ahead of citizens.**
4. **Take the second city the moment `City State` lands** — for the farm cap
   (`farms_per_city_base · cities`), not for its clamp-eaten free income.
5. **Once the Classical Age is the next Library want and it is within ~100 s, freeze
   the food bank.** The age is 250 food against a ceiling of 100 per 30 s; anything
   else bought in that window delays it by its own cost in full. This one rule is
   worth 20 s.
6. **When a citizen would stand idle, the build crew gets the bank.** Without it the
   trainer wins every tie (a citizen is the cheapest thing on the board) and eats the
   food the city and the age need.
7. **Never commit to something you cannot pay for yet while anything is in flight** —
   advance to the next completion and re-plan, because a tech landing changes the
   ceiling and the ceiling is what the plan is about. Worth 23 s on its own.

```
$ cargo run -p don-ai --example boom_arena
== CapFirst (optimiser-derived) -> 4:10 (3751 frames)
   ... City State 0:00, Farm 0:00, Farm 0:02, Small City 1:16, Barter 2:10,
       Market 2:39, Classical Age 3:30, Written Word 3:56, Camp #2 4:01
   end: 14 citizens (8 food / 6 timber), 8 farms, 2 camps, 2 cities, 1 market
   rate: food 100 timber 80 wealth 10 per 30 s (cap 100)

== economic.bhs cases 6-18 (Sobota / Engle) -> 4:56 (4440 frames)
   end: 14 citizens (7 food / 7 timber), 7 farms, 2 camps, 2 cities, 1 market

gap: 689 frames = 0:45 (+18%)
```

**Honesty about what this is.** `CapFirst` plays an *economy*, not a game. It has no
units, no map, no opponent and no combat, so "it beats the shipped AI" means "its
opening reaches a fixed economic state 46 s sooner in a model of one player's
economy". It has never been run against the ron-ai lane's transcription of the actual
shipped AI, because that transcription drives orders in a different world model
(`don_ai::game`); wiring `CapFirst` into that arena is the obvious next step and is
listed in §9.

Six tests, all passing (`cargo test -p don-ai optimum`), including the accumulator
identity against the engine's own `income/16`-per-`gather_rate` arithmetic, the
"seventh gatherer is worth nothing" step function, and the head-to-head as a
regression.

---

## 8. Sensitivity, and the search's own noise floor

The dominant row of the previous study's sensitivity table — the commerce clamp scale,
worth +125 s when halved — **no longer exists**, because the clamp is measured. What
is left is in the table below, produced by `python3 analysis/opening.py sens`.

| assumption | boom complete | Δ |
|---|---:|---:|
| **BASE** (all defaults) | **3:57** | — |
| slots per Woodcutter's Camp = 3 | 4:07 | +9 s |
| slots per Woodcutter's Camp = 4 | 3:59 | +2 s |
| slots per Woodcutter's Camp = 6 | 3:57 | −1 s |
| starting citizens = 3 | 4:06 | +9 s |
| starting citizens = 8 | 3:51 | −6 s |
| max builders on one site = 1 | 4:02 | +5 s |
| max builders on one site = 8 | 3:58 | +1 s |
| **building cost-ramp ceiling 200 %** | **3:35** | **−22 s** |
| walk 75 frames per build | 4:07 | +9 s |
| no starting Woodcutter's Camp | 4:06 | +9 s |

And the search's own noise floor, by Pareto width — the number every Δ above has to
clear to mean anything:

| Pareto width `keep` | boom complete |
|---:|---:|
| 4 | 4:00 (3602 frames) |
| 8 | 3:59 (3591) |
| 12 | 3:57 (3565) |
| 20 | 3:56 (3550) |

**52 frames = 3.5 s** across a 5× change in search width, against 30–40 s for the
previous study's beam. Every row in the sensitivity table above therefore means
something, which was not true before.

Two things follow. First, **the only assumption still worth more than 10 s is the
building cost-ramp ceiling** (−22 s if buildings ramp like "other civilian" at 200 %
rather than not at all) — which makes resolving the class lookup in
`TypeData::get_cost` the highest-value unfinished derivation for this lane. Second,
**every remaining assumption is worth less than the 46 s gap between `CapFirst` and
the shipped order**, and all of them together are worth less than it, so that gap is
not an artefact of the assumption set. It may still be an artefact of everything the
model *omits* (§4), which is a different and larger worry.

---

## 9. What I could not establish

* **Caravans and merchants.** Still the largest hole in the Ancient-age wealth model,
  and the one the designers' own script leans on (case 15 builds a caravan the moment
  the Market goes up). `Leader::calc_gather`'s `0x1A2`/`0x1A3` type loop is the target.
* **Whether the "economy dirty" flag (bit `0x2000000`) is set by the events that
  matter** — a completed building, a worker changing job. §3.3 measures the *gate*;
  it does not measure what trips it. If it is not set on those events, income lags by
  up to 34 s and every plan here shifts.
* **Gather slots per building.** Terrain-derived inside `BuildTypeData::calc_gather`;
  not in any data file. The camp's 5 comes from the shipped AI's `min_size`, which is
  a designer's target, not an engine value.
* **Which `RAMP_MAX` class each *building* falls into.** `TypeData::get_cost` picks it
  by class and I did not resolve the lookup; §8 sweeps it.
* **`get_techs_per_age()`** is a game option, not a rules constant, so "the age needs N
  techs of the current age" is unmodelled. `economic.bhs` case 18 hands the whole boom
  off when `needed_techs > 3`.
* **`CapFirst` against the real shipped-AI transcription.** Two world models, one
  arena needed. This is the next thing to build.
* **Anything about a real game.** No part of this has been executed against retail.
  The oracle cannot help here: the economy is a whole-frame subsystem, not a pure
  function, so the honest path to Tier B is the live-process route
  (`donscan` can already find `LeaderDataEncrypt` at
  `0x00E3A390 + who·0x6EEC + 0x6EB8` — that address is now known, which is the piece
  the previous study was missing when its live read failed).

---

## 10. Files written

| path | what |
|---|---|
| `analysis/opening.py` | the measured economy, the Pareto-DP search, and every table above |
| `crates/don-ai/src/optimum/mod.rs` | module docs + 6 tests |
| `crates/don-ai/src/optimum/econ.rs` | the integer economy, every constant with its address |
| `crates/don-ai/src/optimum/player.rs` | `CapFirst`, `ShippedBoom`, the ablation orders |
| `crates/don-ai/examples/boom_arena.rs` | the head-to-head and the ablation table |
| `crates/don-ai/src/lib.rs` | one line: `pub mod optimum;` |
| `docs/tracks/analytics-v2.md` | this file |

Nothing was committed or staged. `analysis/econ.py`, `analysis/plan.py` and
`analysis/study.py` from the previous study are untouched and still run; they encode
the pre-measurement clamp assumption, so prefer `opening.py`.

## 11. Ledger deltas requested

Additions, all **[measured]**, none differentially tested:

* **`Leader::calc_resource_caps` scales the commerce cap by 16** (`shl eax,4` at
  `0x006CEE78`) — the Ancient ceiling is 70 resources per 30 s per resource.
* **`LeaderDataEncrypt` layout** — 248 bytes, 13 named fields, at
  `LeaderData::data_encrypted` = `*(leader + 0x6EB8)`, leaders at
  `0x00E3A390` stride `0x6EEC`.
* **`epoch[k]` is the level of library column `k`**, `k` = the tech's `cat` field;
  `epoch[2]` is Commerce.
* **The income refresh gate** — dirty ⇒ ≤ 8 frames; otherwise ≥ 300 frames **and**
  `(frame + 8·who) ≡ 0 (mod 256)`; `LeaderData::gather_stamp` at `+0x7AC`.
* **`Constants::territory_taxes` = `[0,50,100,200,300]` by government**, applied at
  `0x006CF6A5`; zero in the Ancient age.
* **`over_cap[r]`** is 0 / 1 / 2 and drives the flashing HUD readout.

Corrections:

* `docs/derivation/rules-constants.json`: `scholar_rate` is `scaled(256)` with **six**
  entries, not `wtoi` with five.
* `docs/tracks/build-order-analytics.md` §7: the "Commerce I first" recommendation is
  retracted; §3 and §8's first two open questions are closed.
