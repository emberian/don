# Tech tree and cities

Lane `mech:tech-cities`. Checksum channel served: **`cities`** (channel 9 of
`CheckSums::check_all` `0x00936560`).

Implementation: `crates/don-sim/src/systems/tech_cities.rs` — 38 tests, all passing
(`rustc --edition 2021 --test` on the file; two of them load the real
`ron-data/techrules.xml`). Zero warnings as a library. The file is **not referenced from
`lib.rs`** — see §9.

Everything below is `[measured]` against `ron-bin/riseofnations.exe`,
`ron-bin/sbl/rise.pdb` and the shipped `ron-data/*.xml` on this machine, except where a
structural claim comes from `re/decomp-all/*.c`, which is marked `[structure]`. Nothing
here has been run against the oracle: **no Tier A or B claim is made anywhere in this
document.** The strongest claim is "this is what the instruction stream and the PDB say,
and it is reproduced in Rust with the same integer arithmetic".

---

## 1. What now works

| thing | address | state |
|---|---|---|
| `cities` checksum channel, end to end | `0x00937600` | reproduced byte-for-byte in structure, incl. the `Array<CaravanLink>` sub-walk and the two `String` members the sync walk *skips* |
| `adler32` + `CheckSum::walk_function` | `0x00A46830` / `0x00936FF0` | ported, cross-checked against an independent RFC-1950 implementation over 10 lengths straddling `NMAX` |
| `vector_dist` — the proximity primitive | `0x0046CFF0` | ported exactly, incl. the 59999 overflow branch |
| the 160-slot `Cities` pool and its slot allocator | `0x007358C0` / `0x007352C0` / `0x00733310` | ported, incl. `city_mark` high-water semantics |
| city level ladder, upgrade thresholds, radius, pop value, pop cap, farm limit, taxes, literacy, trade value | 11 `CityData::*` | ported with the retail constant values |
| gather enhancer tables (granary / lumber mill / smelter / refinery) | `0x00737C60` | ported, incl. the off-by-one table base and the hard-zero refinery |
| auto-gather slot census | `0x00737DC0` | ported, incl. knowledge being excluded from the total |
| city capture state transfer, incl. capital handover | `0x00736C40` | ported (sim fields only) |
| the **recapture damage modifier** | `0x00644130` tail | located and ported — it is *not* in the city code, it is the last branch of `ObjectData::get_damage` |
| city minimum spacing and its relaxation rule | `0x006375B0` | ported |
| tech bitmask, epoch counters, research categories | `0x006E0C80` / `0x006D2850` / `0x0066CBA0` | ported |
| city limit from Civic techs | `0x006D6130` | ported — this closes "how do you get more cities" |
| age advancement requirement | `0x006D7280` inside `0x006DB810` | ported; full-game ladder is `age*4 + 2` library techs |
| `techrules.xml` loader (85 techs, costs, prereqs, job times, tribe masks) | data | parsed and validated (cost totals, acyclic prereq graph, age chain) |

## 2. The cities checksum channel, exactly

`CheckSums::check_all` builds one `CheckSum` on its stack —
`{ vftable, input = 0, checksum = 1, flags = -1, accum, size }` — and **resets
`accum = 1; size = 0` before every channel**, logging each channel's accumulator
individually (`"  cities: %u"`) and returning the **sum** of them. So each channel is an
independent adler32 seeded at 1. [measured, `0x00936560`]

`CheckSum::walk_function(begin, end)` `0x00936FF0` is exactly:

```
this->size  += end - begin;
this->accum  = adler32(this->accum, begin, end - begin);
```

`adler32` is BHG's own copy at `main/basic/misc.cpp:464`, `BASE = 65521`,
`NMAX = 0x15B0 = 5552`, and **a null buffer returns 1** rather than passing the
accumulator through.

`CheckSums::check_cities` `0x00937600`:

```
for who in 0..8:                               # leaders[] at 0x00E3A390, stride 0x6EEC
    if !(leaders[who].leader_flags & 1): continue
    for i in 0 .. cities[who].length:           # NOTE: length, not city_mark
        c = cities[who].data[i]
        if !(c->city_flags & 1): continue
        if c->vfptr == City::vftable:           # inlined City::walk_data
            walk(c+4,  c+6)                     #   city_flags, 2 bytes
            if (c->city_flags & 1):
                walk(c+6, c+0x72)               #   108-byte POD window
                if (walker->checksum == 0):     #   FALSE for the sync checksum
                    String::walk_data(&c->name)
                    String::walk_data(&c->id)
                Array<CaravanLink>::walk_data(&c->vans)
        else: c->vtable[1](walker)
```

`Cities` is at `0x00C09960`: **8 × `PtrArray<City>`, 28 bytes each**, ending at
`0x00C09A40`. `Cities::init` reserves **20** per array and constructs 20 `City` objects in
each — that is the "160-slot pool". `sizeof(City) == 192`, allocated as `malloc(0xC4)` with
a 4-byte vbase cookie.

Two things a naive port gets wrong:

* **The iteration bound is the `PtrArray` length, not `LeaderData::city_mark`.** `city_mark`
  (`LeaderData +1032`) bounds *allocation* (`Cities::init_city`) and *most gameplay loops*
  (`BuildTypeData::blocked_location` scans to `city_mark`), but the checksum walks every
  allocated slot and filters on `city_flags & 1`. Dead slots contribute nothing regardless,
  so the two agree in value — but only because the flag test is inside the loop.
* **City names are NOT in the sync checksum.** `DataWalk::checksum` is 1 for `check_all`
  and the two `String` walks are gated on `== 0`. They *are* in the save game. This is the
  first concrete case I have seen of `sim-critical ⊂ save-game` (architecture.md §8.6)
  biting at the field level rather than the object level.

### 2.1 The `CityData` window

Field names and offsets are the compiler's own (PDB TPI). The walked window is `[+4, +6)`
then `[+6, +114)` — 2 + 108 bytes — and there is **no padding**: `space[3]` at `+105` runs
exactly into `ter[6]` at `+108`, which ends at `+114`.

```
+4   short  city_flags     +76  short  scouted        +94  char  who
+6   short  city           +78  short  in_port        +95  char  race
+8   short  o              +80  short  peasant_dist   +96  char  founder
+10  short  reg            +82  short  trade_val      +97  uchar plundered
+12  Coord  x              +84  short  conquest_node  +98  uchar ocean
+16  Coord  y              +86  uchar  granary        +99  uchar land
+20  int    attack_stamp   +87  uchar  lumber_mill    +100 uchar filled
+24  int    raid_stamp     +88  uchar  smelter        +101 uchar bordering
+28  int    reduce_stamp   +89  uchar  refinery       +102 uchar ocean_filled
+32  int    capture_stamp  +90  uchar  free           +103 uchar dock_tile
+36  int    assimilation_timer                        +104 uchar was_capital_flags
+40  int    capture_strength  +91 uchar busy          +105 uchar space[3]
+44  int    traded_with[8]    +92 uchar gatherers     +108 uchar ter[6]
                              +93 uchar pop
--- past the checksum window ---
+116 Array<CaravanLink> vans   (walked)
+144 String name               (NOT walked by the sync checksum)
+164 String id                 (NOT walked by the sync checksum)
```

### 2.2 `Array<CaravanLink>::walk_data` `0x00489040` — a determinism hazard

The array walk emits, in order: `length:i32`; and if length ≠ 0 also **`capacity:i32`**,
**`grow:i16`**, **`flags & 0xBF : u8`**, then 8 bytes per element (`{int cara; int who;}`).

**The allocated capacity and the growth hint are part of the checksum.** A reimplementation
that stores caravan links in a `Vec` and lets it grow by its own policy will produce a
different `cities` channel value from a bit-identical game state. This is captured in
`CaravanLinkArray` (which carries `capacity` and `grow` explicitly) and asserted by a test.
It is the single most likely source of a false desync in this channel.

## 3. City levels, upgrades, and the auto-gather

City centers are three build types plus one wonder:
`VILLAGE = 414`, `TOWN = 415`, `METROPOLIS = 416`, and `FORBIDDENCITY = 531` which
`CityData::get_level` treats as level 3. [measured, `0x00739340`]

* `CityData::upgrades_to` `0x007364F0`: VILLAGE → TOWN → METROPOLIS → (none).
* `CityData::num_kinds_needed` `0x00736640`: **`CITY_BUILDINGS + 1` = 6** distinct building
  kinds for VILLAGE → TOWN, **`METRO_BUILDINGS + 1` = 10** for TOWN → METROPOLIS. The
  `+ 1` is in the code, not the data; the rules.xml values are 5 and 9.
* `CityData::enough_kinds` `0x00736540` counts **distinct `TypeIndex`** over the city's
  building list (linked through `BuildData::city_down` `+116`), caps its scan at 24, and
  requires the building to be active and to pass two virtual filters.
* `CityData::get_pop_value` `0x00738450`: **1 / 3 / 5** by level. `City::check_upgrade`
  `0x00738B20` applies the delta to `LeaderData::pop` (`+2396`), `Game::pop` (`Game+0x6CC`)
  and `LeaderData::reg_pop[reg]` (`+3682`) — three counters, all three must move together.
* `CityData::pop_cap` `0x00737D40`: `VILLAGE_POP * level`. **`VILLAGE_POP` is 0 in retail
  rules**, so shipped cities grant no population cap at all. (`MIN_POP_LIMIT`,
  `MILITARY_POP` and `GRANARY_POP` are also 0; `MAX_POP_LIMIT` is 300.) Whatever drives the
  RoN population limit, it is not these constants — flagged as an open question.
* `LeaderData::get_radius` `0x006DB790`:
  `CITY_CENTER_RADIUS + (level-1)*CITY_CENTER_POP_RADIUS`, `+ INDIANS_CITY_RADIUS` for
  tribe bonus 0x15, **clamped to 64 tiles**. Retail: 20 / 24 / 28.
* `CityData::get_farm_limit` `0x00738230`:
  `LeaderData::get_farm_limit() + (level-1)*FARMS_PER_CITY_LEVEL`, and
  `FARMS_PER_CITY_LEVEL` is 0 in retail, so it is flat at 5 (7 Egyptian, +1 with Olive Oil).

### 3.1 The gather enhancer tables have an off-by-one base

`City::calc_gather` `0x00737C60` writes four bytes into `CityData`:

```
granary     = (city_flags & 0x200) ? constants[0x2B8 + granary_level*4] : 0
lumber_mill = (city_flags & 0x400) ? constants[0x2CC + lumber_level*4]  : 0
smelter     = has_smelter_for_age  ? constants[0x2E0 + smelter_level*4] : 0
refinery    = 0                                   <-- literal zero, always
```

`granary_bonus[5]` starts at `+700`, but the base pushed is `+0x2B8 = 696`; likewise
`lumbermill_bonus[5]` at `+720` against base `+0x2CC = 716`, and `smelter_bonus[5]` at
`+740` against base `+0x2E0 = 736`. So **the tables are indexed `level - 1`** and the level
accessors are 1-based, with 0 meaning "no such building" and short-circuited to a 0 bonus by
the `city_flags` test. Retail tables: granary and lumber mill `20/50/100/200/250` %, smelter
`50/100/150/200/250` %.

**`CityData::refinery` is stored as a literal 0 in this build**, so `REFINERY_BONUS = 33%`
is dead in the city path even though the constant is loaded. `CityData::lumber_level`
`0x00736820` only ever returns 1..4 (three upgrade techs at `0x2C2/0x2C3/0x2C4`), so the
fifth table entry (250%) is unreachable from the lumber mill ladder. Both are recorded as
facts, not bugs to work around.

### 3.2 Auto-gather slots

`City::count_gather_slots` `0x00737DC0` walks the city's building list and maps:

| building | `TypeIndex` | resource slot |
|---|---|---|
| `FARM` | 417 | FOOD (0) |
| `WOODCUTTER` | 418 | TIMBER (1) |
| `MINE` | 419 | METAL (4) |
| `UNIVERSITY` | 420 | KNOWLEDGE (3) |
| `OIL_WELL` / `OIL_PLATFORM` | 421 / 422 | OIL (5) |

Slots come from `BuildData::gather_max` (`+128`), free slots from
`gather_max - BuildData::num_gatherers()` (`0x00630450`). **The function's return value
excludes knowledge** — the accumulate is skipped when the slot index is 3 — which is why
universities do not count toward a city's gatherer total.

`City::calc_gather` then tail-calls `LeaderData::calc_city_resources` `0x006D5530` with
`(x, y, city_index)`. That function (1,428 bytes) is the actual per-city yield computation
and is **not** derived in this lane — see §8.

## 4. Capture, recapture, plunder

### 4.1 `City::capture` `0x00736C40`

Signature `City::capture(City* old, short city, char who, short o)`. It transfers state
into a fresh slot rather than mutating in place. Sim-relevant behaviour:

* `x`/`y` are re-read from the new center object, **XOR-obfuscated with `0x63637`** in the
  object (`obj+0x10`, `obj+0x14`). Coordinate storage in `Object` is obfuscated; `CityData`
  stores them plain.
* `attack_stamp` and `raid_stamp` are both stamped with `Game::frame`.
* `pop`, `reg`, `scouted`, `founder`, `plundered`, `was_capital_flags` and the whole stamp
  block `+44..+76` carry over.
* `city_flags = old & 0xF13F`, then `|= 0x100` unless the two players are on one team.
  **`0x100` is inside the keep-mask**, so a later recapture does not clear it — only
  `City::assimilate` `0x00738E90` does.
* **Capital handover:** if the captured city was a capital (`0x10`), the previous owner's
  bit is recorded in `was_capital_flags`, `0x8000` is set if `0x4000` was, and the capital
  bits are cleared. If the *new* owner has a bit in `was_capital_flags`, the capital is
  restored to them (and `race` is re-homed). This is how "take your capital back" works.
* Tail: the city-center building's hit points are set to **`max_hits - 10`**.

`race` (`+95`) is the field that matters most. It re-homes to the capturing player when the
capture is by the same player, between mutual `diplos == 2` allies, under
`leader_flags & 0x200000`, or with `has_preq(0x2B9)`; otherwise it is preserved from the
old record.

### 4.2 The recapture damage modifier is in the combat code

`RECAPTURE_CITY_MODIFIER` is `Constants +108`, `"2/1"` parsed by `String::fraction(256)`
to **512**. An exhaustive grep of the decompiled corpus finds exactly **one** reader:
the final branch of `ObjectData::get_damage` `0x00644130`.

```
if (target has Object flag bit 2)
&& (target build type flags & 0x20)                       # is a city center
&& (BuildData::city  >= 0)                                # +114, the city index
&& (cities[owner][city]->race == attacker->who)           # City +95
    return (RECAPTURE * dmg + ((RECAPTURE * dmg) >> 31 & 0xFF)) >> 8;
return dmg;
```

So: **a player doing damage to a city whose `race` equals their own player number deals
double damage**, applied as a final rescale after every other term including the armor
subtraction. The rescale expression is the engine's truncate-toward-zero divide by 256, so
a modded `3/2` (384) gives 5 → 7 and −5 → −7, not 8 / −8.

This corrects an assumption worth stating plainly: the recapture modifier is **not** a city
mechanic that `City::*` applies, and anyone porting `get_damage` without the city pool
available will silently drop it. `crates/don-sim/src/mechanics.rs` `damage()` does not
have it. It needs `cities[target_owner][build.city].race`, which now exists in this module.

### 4.3 Plunder — partially derived, flagged

`CITY_PLUNDER_PER_LEVEL` (`Constants +772`, 100) and `CAPITAL_PLUNDER` (`+776`, 500) are
`[measured]` values with PDB-supplied names, but **no reader of `Constants + 0x304` appears
anywhere in `re/decomp-all/`**, so the multiplication site is unlocated. The helper in the
module is marked `[unverified]` and should not be trusted.

What *is* read: `Build::plunder` `0x00623660` uses `BuildTypeData::plunder_value` (`+724`)
and `plunder_good` (`+728`), scales by `hits/max_hits` **in float** for buildings without
`build_masks & 0x1000`, and applies `LeaderData::plunder_scale` (`+2324`, `float`) — one of
the three known float fields inside walked state. City centers presumably take the
`& 0x1000` branch (a flat `constants[0x3B0] * v / 100`), with a special case that suppresses
the city scaling when the razer owns a city whose `race` matches. Not finished; not ported.

## 5. City placement and minimum spacing

`BuildTypeData::blocked_location` `0x006375B0` (5,120 bytes) is the placement validator.
Its `is_city()` arm, `[structure]`:

```
for w in 0..8 where leaders[w].leader_flags & 1:
    spacing = CITY_SPACING                                  # 24 tiles
    if regions[reg].size * 9 / 10 <= leaders[w].reg_terr[reg]:
        spacing -= RELAX_CITY_SPACING                       # -> 18 tiles
    for each UnbuiltCity in unbuilt_cities.lists[w]:        # planned cities count
        ... same-region + vector_dist <= spacing  ->  return 20
    if w == me or leaders[w].reg_cities[reg] != 0:
        for i in 0 .. leaders[w].city_mark:                 # built cities
            c = cities[w][i]
            if c.city_flags & 1 and c.reg == my_reg
               and vector_dist(...) <= spacing:  ->  return 20
```

Three facts worth carrying:

* **Spacing relaxes to 18 tiles against a player who already holds ≥ 90 % of the region's
  tiles.** The predicate is `region_size * 9 / 10 <= reg_terr[reg]`, integer division.
* **Only cities in the same region block.** The region check is inside both loops.
* **Planned (`UnbuiltCity`) cities block too.** `UnbuiltCities` is 8 `Array<UnbuiltCity>`,
  `UnbuiltCity` is `{short o; char who;}`, 4 bytes. It has its own `walk_data`
  (`0x00460DC0`) but is *not* a `check_all` channel — it rides the save game only.

The distance function is `int vector_dist(int, int)` `0x0046CFF0`, and it is used by
essentially every proximity rule in the game, so it is worth having exactly:

```
lo, hi = sorted(|dx|, |dy|)
hi == 0        -> 0
lo > 59999     -> (lo + 2*hi) >> 1          # overflow guard
otherwise      -> hi + lo*lo / (2*hi)       # integer truncating
```

`FIRST_CITY_NEAR_COAST` (16 tiles) is read by the same function and by
`MapConquest::add_additional_players`; `FORT_TO_ENEMY_CITY_SPACING` (32) and
`CITY_CAPTURE_RADIUS` (10, read by `Build::check_capture` `0x006276A0` and `Unit::fight`
`0x005FD4D0`) are captured as constants but their consumers are not ported here.

1 tile = **`0x300` = 768 world units** [measured, `Leader::produce_city` `0x006CB120` passes
`tile * 0x300` and `+0x180` for a tile centre].

## 6. Tech

### 6.1 The state is a bitmask plus four obfuscated counters

`LeaderData::tech : BitMask<806>` at `+27660`, payload **101 bytes at `+27672` (`0x6C18`)**,
**bit index = `TypeIndex`**. [measured: `LeaderData::LeaderData` `0x006D7540` does
`memset(this + 0x6C18, 0, 0x65)`; `LeaderData::walk_data` `0x006D6750` walks
`[+0x6C18, +0x6C0C + len + 0xC)`.] `BitMask::set` is `0x00450360`.

The counters live in `LeaderDataEncrypt` (`LeaderData +28344`), XOR-obfuscated:

| field | offset | XOR key | meaning |
|---|---|---|---|
| `ages` | `+220` | — | |
| `epochs` | `+224` | `0x69587` | total library techs researched |
| `discovered` | `+228` | `0x13985` | non-library techs |
| `epoch[4]` | `+232` | `0x63187` | library techs per research category |

### 6.2 The four research categories, and where they come from

`TechType::set_research` `0x0066CBA0` derives `TypeData::cat` (`+20`) arithmetically from
the type index:

```
if 551 <= t < 579:                       # BASE_EPOCHTYPES .. END_EPOCHTYPES
    q = (t - 551) / 7
    cat = {0: Science(3), 1: Commerce(2), 2: Civic(1), 3: Military(0)}[q]
else:
    cat = Science(3)                     # the fallback bucket
```

So the 28 library techs are exactly 4 categories × 7 levels:

| cat | value | `TypeIndex` range | first tech |
|---|--:|---|---|
| Military | 0 | 572..578 | `THE_ART_OF_WAR` |
| Civic | 1 | 565..571 | `CITY_STATE` |
| Commerce | 2 | 558..564 | `BARTER` |
| Science | 3 | 551..557 | `WRITTEN_WORD` |

### 6.3 City limit — the direct answer

`LeaderData::get_city_limit` `0x006D6130`:

```
n = data_encrypted->epoch[1] ^ 0x63187       # epoch[Civic]
if tribe >= 0 and has_tribe_bonus(3) /*Bantu*/ and n != 0:  n += BANTU_CITY_LIMIT
n += 1
if has_wonder(PYRAMIDS=526):  n += PYRAMIDS_CITY_LIMIT
```

`LeaderData::at_city_limit` `0x006E0D30` is `get_city_limit() <= get_total_cities()`.
So the city limit is **one plus the number of Civic library techs**, which matches
`rules.xml`'s own category blurb (`<CATEGORY name="Civic" desc="City Limit, National
Borders"/>`). The Bantu bonus is conditional on already having at least one Civic tech.

### 6.4 Age advancement

Age types are `TypeIndex` 544..550 (`CLASSICAL_AGE`..`INFORMATION_AGE`), and `TechTypeData`
carries `age` at `+456`. The gate lives inside `LeaderData::has_preq` `0x006DB810`:

```
if 543 < type < 551:                                        # it is an age type
    if (data_encrypted->epochs ^ 0x69587) < techs_per_age(type):  return 0
```

`LeaderData::techs_per_age` `0x006D7280`:

```
v = starting_age; e = options[0x36]           # game options at [0x00C061E8]
if v == 0 and e > 6:  return age * 4 + 2
n = e - v + 1;  if n == 0: return 0
r = ((age - v) + 1) * (28 / n) - 2
return (v != 0 or age != 0 or r != 1) ? r : 2
```

The `28` is the epoch-tech count. In a full game the ladder is **2, 6, 10, 14, 18, 22, 26**
library techs for Classical through Information.

`Leader::set_age` `0x006D25A0` and `Leader::set_epoch` `0x006D26F0` show the shape of a
"grant everything up to X" operation, and they end with three revalidation sweeps that drop
any held type whose `has_preq` no longer holds: `[544, 629)`, `[50, 402)` (units) and
`[414, 543)` (buildings and wonders).

Those two operations are now executable as `TechState::execute_set_age` and
`TechState::execute_set_epoch`. The port preserves the non-obvious transaction order:
remove the selected ladder's suffix descending, grant its prefix ascending, then perform
the three prerequisite sweeps ascending against the state produced by every earlier
removal. Age types `544..550` are skipped by the first sweep, matching its vtable
`is_age` gate. The mandatory host receives each gain/loss one-shot effect and the final
`reset_obs_flags -> calc_unit_stats -> calc_wall_stats -> Camera::outdate` tail; the
remaining boundary is still the unported body of `Leader::gain_tech`, not the ladder or
revalidation transaction itself.

### 6.5 The prerequisite graph

`LeaderData::has_preq` `0x006DB810` is the evaluator. Core:

```
for i in 0 .. type->num_preq():
    p = TypeData::get_preq(i, who)        # 0x00668700 — TRIBE-ADJUSTED, not the raw preq
    ... LeaderData::special_preq(t, &p)   # 0x006D7030 — substitution hook
    if !has_tech(p): return 0
# then the age gate (6.4), then the mutually-exclusive government pairs at [623, 628]
```

Two structural points a data-driven port must not miss: prerequisites are **resolved per
tribe** through `TypeData::get_preq(i, who)`, and there is a substitution hook
(`special_preq`) before the check. The three `TypeData::preq[3]` slots at `+48` are the
*declared* prerequisites, not the effective ones.

`LeaderData::has_tech` `0x006E0C80` is not a plain bit test either:
`t == -1 → true` ("no requirement"), `t == -2 → false` ("never"), `t < 50 → true`
(resources and gaia types are always held), build/wonder-range types defer to `has_preq`,
and only otherwise is it the raw bit.

`Leader::tech_avail` `0x006DA060` is the "can I start researching this" predicate:
not disabled for this player, not already held, not already queued
(`LeaderData::has_tech_queued` `0x006D73B0`), the paired alternative not taken, then
`LeaderData::type_avail` `0x006E33A0` (which returns 0 / 2 / 4, not a bool).

### 6.6 How tech effects apply — the answer, and it is not what a data-driven port expects

**There is no effect table.** I looked for one and it does not exist. The mechanism is two
halves:

1. **Query-site hard-coding.** Every consumer asks about a *literal* type index at the
   point of use. `CityData::lumber_level` does
   `if has_preq(0x2C4) return 4; if has_preq(0x2C3) return 3; ...`.
   `LeaderData::get_farm_limit` does `if has_wonder(0x21B) n += constants[0x508]`.
   `CityData::get_taxes` does `if has_wonder(0x215) m = (constants[0x4B4]+100)*m/100`.
   The pattern is always `if <has_tech|has_preq|has_wonder|has_tribe_bonus>(<literal>)
   then <field> op constants[<literal offset>]`. The already-known
   `get_attack += 10*level` / `get_armor += 1*level` military upgrades are the same shape,
   not a special case.
2. **`Leader::gain_tech` `0x006DCB60` for one-shot state changes** — 15,001 bytes, the
   third-largest function in the game. It is **not** a jump table: 4,828 instructions, zero
   indirect jumps on the tech index, 45 distinct comparison immediates. It is a hand-written
   if-else chain that calls `TypeData::get_preq` 54×, `LeaderData::type_eligible` 35×,
   **itself 35×** (gaining a tech cascades into gaining others), `has_preq` 31× and
   `has_tribe_bonus` 24×.

Consequence for the port: **tech effects cannot be data-driven from `techrules.xml`.** The
XML carries cost, time, prerequisites, tribe mask and research building — it carries no
effect field at all, because the effects are in the C++. Any port must transcribe the query
sites one at a time. This module transcribes the eleven city-side ones; the rest are open.

Notably, `gain_tech` does not itself write the tech bit (an exhaustive scan of its 4,828
instructions finds no reference to `+0x6C18` and no `bts` against the mask). The bit is set
through `BitMask<806>::set` from a helper; `Leader::lose_tech` `0x006D2850` clears it and
decrements the matching counter, which is what the module's counter bookkeeping mirrors.

### 6.7 `techrules.xml`

85 `<TECH>` entries. `TypeData` receives `job_time +8` (frames; 15 frames = 1 game second),
`tribe_mask +16`, `cat +20`, `costs[6] +24`, `preq[3] +48`, `where +64`, `name +96`, and
`TechTypeData::age +456`.

Cost strings are `"25k/25f"`, `"250k/100o"`, `"120t"`. The letters are exactly the six
`<ABBREV>` values in `ron-data/resourcerules.xml` — `F T G K M O` for Food, Timber,
**Wealth (G)**, Knowledge, Metal, Oil — in `TypeIndex` order 0..5. The parser here is a
**reimplementation from the observed strings, not a port**: `Type::load_cost` `0x00663BA0`
was located but its tokenizer was not decoded. Marked `[structure]` in the code.

Validated by test against the shipped file: 85 techs; the first seven are the ages in
`TypeIndex` order with `AGE` 0..6 and a linear prerequisite chain; every tech has a nonzero
cost, a nonzero job time and a nonempty tribe mask; 39 techs research at the Library
(7 ages + 28 epochs + 4 others); the in-file prerequisite graph is acyclic.

## 7. Corrections and cautions for other lanes

1. **`ObjectData::get_damage` is missing its last branch.** `crates/don-sim/src/mechanics.rs`
   does not implement the recapture rescale (§4.2). It needs the city pool. Two sentences,
   as instructed — flagged, not fixed, because `mechanics.rs` is another lane's file.
2. **`check_all` has 15 channels, not 16.** The lane brief says 16; `architecture.md` §7.3
   and the disassembly both give 15 (14 `CheckSums::check_*` plus `LeaderData::walk_data`
   inline at position 8). `cities` is channel 9. If a 16th is being counted it is probably
   the `SyncLogger` marker between channels or the returned sum itself.
3. **Each channel restarts adler32 at 1 and `check_all` returns the sum.** A harness that
   expects one rolling checksum across all channels will never match.
4. **`Array<T>` capacity is checksummed.** See §2.2. This will bite every channel that
   walks a growable array, not just cities.
5. **`Object` coordinates are XOR-obfuscated with `0x63637`; `LeaderDataEncrypt` counters
   use `0x69587` / `0x13985` / `0x63187`; `LeaderData::get_city_limit` reads
   `epoch[1] ^ 0x63187`.** Any live-memory read of these fields must de-obfuscate.

## 8. Honest gaps

* **`LeaderData::calc_city_resources` `0x006D5530`** (1,428 bytes) is the actual per-city
  resource yield and is **not derived**. `City::calc_gather` only fills the four enhancer
  bytes and then calls it. Without it, "the auto-gather cities perform" is half-answered:
  the *slot census* and the *enhancer percentages* are done, the *yield* is not.
* **`Cities::capture_city` `0x00733380`** (7,998 bytes) — the outer capture driver that
  reassigns every building in the city, awards plunder, and fires the diplomacy/score
  effects — is decompiled but not read. Only the inner `City::capture` is ported.
* **`City::assimilate` `0x00738E90`** and `assimilation_timer` / `REASSIMILATION` (300 %)
  are not derived. This is what clears `city_flags & 0x100`.
* **`City::init` `0x00737050`**, **`City::close` `0x00737550`**, **`City::find_buildings`
  `0x007384C0`**, **`City::compute_trade` `0x00739640`**, **`City::new_caravan`
  `0x00739750`**, **`City::generate_name` `0x00735C90`** (1,884 bytes, an RNG consumer —
  relevant to stream position even though names are not checksummed) — all unread.
* **City plunder** — §4.3.
* **`Leader::research_techs` `0x006C6BA0`** (3,758 bytes) and **`Leader::produce_tech`
  `0x006CA980`** are the AI/queue drivers and are unread; research *progress* (how
  `job_time` is consumed, and by what) is therefore not derived.
* **`Leader::gain_tech`'s 45 branches** are not enumerated. §6.6 establishes the *mechanism*
  and rules out a data-driven one; it does not enumerate the effects.
* **`VILLAGE_POP == 0`** means the shipped population cap does not come from cities. Where
  it does come from is open (`Leader::calc_pop_cap` `0x006DC490` is the function).
* **Everything is `[unverified]`.** No oracle run, no differential test, no live-process
  comparison. The `cities` channel in particular can only be proven by the replay harness.

## 9. Wiring

`crates/don-sim/src/systems/tech_cities.rs` is standalone and dependency-free. It is
**not referenced from `lib.rs`** (per lane file-ownership rules). To wire it in:

```rust
// crates/don-sim/src/systems/mod.rs   (new file)
pub mod tech_cities;

// crates/don-sim/src/lib.rs
pub mod systems;
```

Until then, the tests run with:

```sh
CARGO_MANIFEST_DIR=$PWD/crates/don-sim \
  rustc --edition 2021 --test crates/don-sim/src/systems/tech_cities.rs -o /tmp/tct && /tmp/tct
```

38 tests, all passing. Two of them read `ron-data/techrules.xml` and skip cleanly if it is
absent (it is gitignored).

## 10. Reproducing the derivation

```sh
python3 tools/pdb/lookup.py --name 'City'          # the 60-odd City*/Cities* methods
python3 tools/pdb/lookup.py 937600 936ff0 46cff0   # check_cities, walk_function, vector_dist
cat re/decomp-all/00937600.c                        # the channel
cat re/decomp-all/00489040.c                        # Array<CaravanLink>::walk_data
cat re/decomp-all/00736c40.c                        # City::capture
grep -n -B12 -A12 'c061e4 + 0x6c)' re/decomp-all/00644130.c   # the recapture modifier
grep -n -B25 -A8 'c061e4 + 0x138)' re/decomp-all/006375b0.c   # city spacing
cat re/decomp-all/006d6130.c re/decomp-all/006d7280.c         # city limit, techs per age
```

The `Leader::gain_tech` structural result (no jump table, 4,828 instructions, 45 compare
immediates, 35 recursive calls) came from a capstone pass over
`0x006DCB60 .. +15001`; the function exceeds the 8,192-byte decompiler budget and is
`skipped_large` in `re/decomp-all/MANIFEST.jsonl`.
