# economy — the tick, executed

Lane: **mech:economy**. Module: `crates/don-sim/src/systems/economy.rs` (5,116 lines,
90 in-module tests plus 15 economy integration tests, **105 passed / 0 failed**). Checksum
channels served: **leaders** (channel 8),
**goods** (channel 11).

All addresses are preferred-base VAs in `ron-bin/riseofnations.exe`
(sha256 `30478a44…625079`, image base `0x00400000`). Claims are **[measured]** — verified
here against this binary, this PDB or this data — or **[reported]** — read in another
lane's output and not re-checked. Nothing here is verified in the proof-assistant sense,
and nothing here has been executed against retail. See §7 for the honest tier.

---

## 0. What now works

The economy **tick** runs, end to end, as ported integer code: compose gross income →
cap it → pay it out through the fractional accumulator → move the market → quote prices →
trade → create, activate, recompute, tear down, and recycle caravan routes. The focused
caravan transactions add thirteen integration tests, including a complete
route-to-city-to-gather-to-stockpile execution.

| mechanic | engine function | VA | state |
|---|---|---|---|
| resource tick period & scheduling | `Leader::calc_gather` head | `0x006CEEE0` | **ported** |
| gross-income composition | `Leader::calc_gather` | `0x006CEEE0` | **ported** (CityPool loop executes; three object loops remain inputs) |
| per-city yield | `LeaderData::calc_city_resources` | `0x006D5530` | **ported** (enhancer sum is an input) |
| live-city income collection | city loop inside `Leader::calc_gather` | `0x006CF12A` | **ported against checksum-owned CityPool** |
| per-node rare yield | `LeaderData::calc_rare` | `0x006E08D0` | **ported** |
| worker crowding divisor | `UnitData::calc_gather` | `0x00609180` | **ported** (spatial search is an input) |
| leader-wide bonuses | `LeaderData::calc_resource_bonuses` | `0x006DB030` | **ported** |
| commerce caps | `Leader::calc_resource_caps` | `0x006CE900` | **ported** (wonder terms are inputs) |
| the payout / accumulator | `Leader::do_gather` | `0x006CE450` | **ported** |
| per-frame entry | `Leader::gather` | `0x006CE280` | **ported** |
| market cycle | `GameDaemon::calc_markets` | `0x00732180` | **ported** |
| market trend roll (RNG) | `GameDaemon::calc_market` | `0x00732270` | **ported, exact draw count** |
| buy/sell price quote | `LeaderData::calc_market_prices` | `0x006DC2A0` | **ported** |
| buy / sell | `Leader::do_buy` / `do_sell` | `0x006CFBD0` / `0x006CFC60` | **ported** |
| city taxes | `CityData::get_taxes` | `0x00737B50` | **ported** |
| taxation level | `LeaderData::get_taxation` | `0x006D6E20` | **ported** |
| territory taxation | inside `calc_gather` | `0x006CF4E8` | **ported** |
| tribute scaling | `LeaderData::scale_tribute` | `0x006D5240` | **ported** |
| caravan limit | `LeaderData::get_caravan_limit` | `0x006DCA50` | **ported** |
| caravan route value | `Caravan::trade_value` / `distance` | `0x0073D9D0` / `0x0073D300` | **ported** |
| city caravan income | `City::compute_trade` | `0x00739640` | **ported** |
| first-route wealth award | `City::new_caravan` | `0x00739750` | **ported** |
| caravan pool allocation/recycling | `Caravans::init_caravan` / `close_caravan` | `0x0073E1F0` / `0x0073E350` | **ported** |
| city route link creation/teardown | `Unit::do_trade` / `end_trade_route` | `0x005ED270` / `0x005E3BD0` | **ported** |
| caravan death cleanup | `Unit::close` → `close_orders` | `0x0060EE50` / `0x005E37F0` | **ported** |
| per-worker gather rate | `BuildTypeData::calc_gather` fragment | `0x00639E40` | **partial** — see §5.2 |
| resource substitution | inside `calc_gather` | `0x006CF64C` | **ported** |
| checksum images | `Leader::walk_data` / `Good::walk_data` | `0x006D6750` / `0x0066E5D0` | **framed**, see §6 |

**138 distinct VAs are cited in the module**, each at the site it was ported from.

### How it was measured

1. **Structure** from `re/decomp-all/<VA>.c` — the bulk-decompiled corpus. Every economy
   function I needed was present (nothing exceeded the 8,192-byte decompiler cap except
   `TypeData::get_cost`, which is out of scope).
2. **Every non-obvious control-flow decision and every constant re-read at the instruction
   level** with capstone against the shipped exe. This was not ceremony: it changed the
   answer four times (§4).
3. **Constants cross-checked against `crates/don-rules`.** The 102 rule slots the module
   embeds were compared, by byte offset, against `don-rules`'s generated `SHIPPED` array —
   which is itself byte-for-byte validated against a running match's `RULES` object per
   `docs/provenance-ledger.md`. **102 slots, 0 mismatches** [measured].
4. **Compile and test** with the real `crates/don-sim/src/rng.rs`, not a copy.

```sh
# in-tree, against the real crate (systems/mod.rs and lib.rs are now wired by siblings)
cd /Users/ember/dev/don && cargo test -p don-sim --lib systems::economy
cd /Users/ember/dev/don && cargo test -p don-sim --test caravan_trade_transaction
cd /Users/ember/dev/don && cargo test -p don-sim --test caravan_route_lifecycle
cd /Users/ember/dev/don && cargo test -p don-sim --test caravan_city_unit_lifecycle
cd /Users/ember/dev/don && cargo test -p don-sim --test city_income_lifecycle
# -> 90 in-module + 15 integration tests passed; 0 failed.

# constants cross-check (0 mismatches)
cd /Users/ember/dev/don && python3 - <<'PY'
import re
src = open('crates/don-sim/src/systems/economy.rs').read()
block = src.split('pub const SHIPPED_ECONOMY_SLOTS',1)[1].split('];',1)[0]
mine = {int(o): (n, int(v)) for o, n, v in
        re.findall(r'\((\d+),\s*"([^"]+)",\s*(-?\d+)\)', block)}
m = re.search(r'pub const SHIPPED:\s*\[i32;\s*RULES_DWORDS\]\s*=\s*\[(.*?)\];',
              open('crates/don-rules/src/rules.rs').read(), re.S)
nums = [int(x) for x in re.findall(r'-?\d+', m.group(1))]
bad = [(o, n, v, nums[o//4]) for o, (n, v) in sorted(mine.items()) if nums[o//4] != v]
print(f"{len(mine)} slots, {len(bad)} mismatches", bad)
PY
```

The module uses the crate's shared RNG, checksum, coordinate-ladder and exact integer
distance implementations; `don-rules` remains a dev-only cross-check rather than a runtime
dependency.

---

## 1. The lead result: `Leader::calc_gather` is no longer a hole

`docs/derivation/sim-economy.md` §7 named `FUN_006CEEE0` as *"the doorway to the
worker→income composition, the single largest hole left in the economy"*. It is
`Leader::calc_gather` `0x006CEEE0`, and it is ported.

The composition, in retail order — every step truncates, so it is not reorderable:

| # | step | VA |
|---:|---|---|
| 1 | `out[i] = BASIC_GATHER[i] << 4` | `0x006CF0A6` |
| 2 | Lakota: `out[food] += units * LAKOTA_FOOD * 16` | `0x006CF0C0` |
| 3 | Americans: `+= n * AMERICANS_BARRACKS_GATHER * 16` into slots **0, 1, 4, 2** | `0x006CF0E2` |
| 4 | four object loops: cities, gather buildings, special builds `0x1A2`/`0x1A3`, gatherers | `0x006CF12A`–`0x006CF43F` |
| 5 | `out[oil] = out[oil] * (refineries * REFINERY_BONUS + 100) / 100` | `0x006CF48C` |
| 6 | capitalism: `out[oil] += CAPITALISM_OIL_PROD * 16` | `0x006CF4A6` |
| 7 | Inca: `out[wealth] += out[metal]`, **gated `INCA_WEALTH_PER_MINER < 0`** | `0x006CF4B9` |
| 8 | rare loop: 44 bits, one `calc_rare(bit + 6, …)` each | `0x006CF4DC` |
| 9 | coffee: every slot `* (COFFEE_INCOME_BONUS + 100) / 100` | `0x006CF5B7` |
| 10 | territory taxes → wealth; Mongol territory → food | `0x006CF4E8` / `0x006CF55E` |
| 11 | `LeaderData::calc_resource_bonuses` | `0x006CF5C6` |
| 12 | resource substitution, then zero the source slot | `0x006CF64C` |

### 1.1 The resource tick period is not what anyone assumed

There are **two** periods and they had been conflated into one.

* **The accumulator period** is `GATHER_RATE * 16 = 7,200` — already known
  (`sim-economy.md` §3.1). Income is in sixteenths-per-450-frames; `income / 7200` whole
  units land per frame and the remainder accrues.
* **The recomposition period** is new [measured, `0x006CEEF7`–`0x006CEF3D`]. Recomputing
  gross income walks every city, building, gatherer and rare, so the engine does it
  *rarely* and staggers it across players:

```
if (leader[0] & 0x2000000) {            // the "economy is dirty" bit
    due = frame != 0 && (slot + frame) % 8 == 0
} else {
    due = frame >= last_calc_frame + 300 && (frame + slot*8) % 256 == 0
}
```

**Every 256 frames per leader normally (≈17 s), every 8 frames while dirty (≈0.53 s)**,
with a 300-frame floor on the clean path, phase-offset by `slot * 8` so no two players
recompute on the same frame. `Leader::do_gather` then pays out *every* frame from the
cached gross.

This has a consequence a replay harness will hit immediately: **income lags by up to 256
frames after any change**, and the lag is per-player and phase-dependent. The module's
end-to-end test demonstrates it — a leader starting dirty at frame 0 earns nothing until
frame 8, because frame 0 is excluded from the dirty schedule.

### 1.2 All six resource slots are now pinned

`sim-economy.md` §3.2 named slots 0–3 and explicitly **refused** to name 4 and 5, on the
grounds that "the wiki's ordering is not evidence." Correct then; resolved now, by four
independent witnesses [measured]:

| witness | what it shows |
|---|---|
| `BASIC_GATHER` / `CITY_GATHER` entry strings, in slot order | `0food` `0timb` `0gd` `0know` `0met` `0oil` |
| `AMERICANS_BARRACKS_GATHER` XML text vs. its code | text says *"2 each of food, timber, metal, and gold"*; code at `0x006CF0E2` adds to slots **0, 1, 4, 2** in that order |
| `REFINERY_BONUS` (`"33% per refinery"`) | scales slot **5** ⇒ oil |
| `INCA_WEALTH_PER_MINER` | moves slot **4** into slot **2** ⇒ metal → wealth |

Plus a fifth: `Leader::do_buy` pays from `econ + 8`, i.e. slot 2 ⇒ wealth.

**Order: 0 food · 1 timber · 2 wealth · 3 knowledge · 4 metal · 5 oil.**

The four-way agreement between the American bonus's prose and its emission order is the
strongest single piece of evidence: the XML lists four resource names, the code writes four
specific slots, and the mapping is forced.

---

## 2. The market — fully ported, and it is an RNG consumer

This subsystem was not previously derived at all. It is the part of this lane most likely
to cause a desync if reimplemented from intuition, because **it draws from the shared
simulation stream** (`GameAccess::game_random`, `[0x00C06184]` → `0x00E37A8C`).

### 2.1 State, on the `Game` object [measured]

| offset | meaning |
|---|---|
| `Game + 0x564` | market cycle counter (**not** the frame counter) |
| `Game + 0x568 + res*4` | base price |
| `Game + 0x580 + res*4` | spread — the wandering offset |
| `Game + 0x598 + res*4` | trend target |
| `Game + 0x5B0 + res*4` | trend step per cycle |
| `Game + 0x5C8 + res*4` | cycles left before a new trend is rolled |

Quotes: `sell = base + spread`, `buy = 2*base + spread`.

### 2.2 Three nested periodicities [measured, `0x00732180`]

* **frame gate** — `frame == 0 || MARKET_CYCLE_RATE <= 1 || frame % MARKET_CYCLE_RATE == 0`.
  Shipped `MARKET_CYCLE_RATE` is 1, so: every frame.
* **per-resource gate** — a resource is serviced when `cycle == 0` or `(cycle + res) & 7 == 0`.
  **Every 8 cycles, phase-offset by the resource index** — which is why the six prices never
  move together.
* **equilibrium walk** — when `(cycle + res) & 0xFF == 0`, i.e. every 256 cycles, the base
  price takes one step toward `MARKET_EQUILIBRIUM` (65): down by `price / 65`, up by 1 —
  or by **2** if a single step would still leave it under `MARKET_BASEMENT` (10).

Both moduli are taken on the low byte (`test al, 7` / `test al, al`).

### 2.3 The RNG draw count is data-dependent — and that is the hazard

`GameDaemon::calc_market` `0x00732270` draws `Random::get(0, 0xFFFF)`:

* **twice** if `variance > 0` (the two halves of the trend target), else not at all;
* **once more** if `MARKET_TREND_RANGE - 1 > 0` (the trend duration jitter).

With shipped rules that is **three draws, every time**. But the count genuinely branches,
so a port that always draws three — or that draws them in a different order — shifts the
shared stream position for *every downstream consumer*, not just the market. The module
tests all three arities explicitly by replaying the generator and comparing states.

```
00732292  v = base_price / 2                      (truncate toward zero)
00732299  v = max(v, MARKET_MIN_VARIANCE)         (cmovg)
0073229c  v = (v + 1) / 2                         (truncate toward zero)
007322bf  a = rng(0,0xFFFF) % (v+1)               if v > 0 else 0
007322e9  b = rng(0,0xFFFF) % (v+1)               if v > 0 else 0
00732300  trend_target = b - (v+1) + a            -> range [-(v+1), v-1]
00732329  r = rng(0,0xFFFF) % MARKET_TREND_RANGE  if RANGE-1 > 0 else 0
00732345  duration = MARKET_MIN_TREND + r
0073234e  delta = trend_target - spread
0073237f  trend_step = ((duration - 1) * sign(delta) + delta) / duration
```

The last line rounds **away from zero**, which is what makes a trend arrive in exactly
`duration` cycles instead of stalling one short.

### 2.4 `Random::get(int, int)` is `[lo, hi)`, from the low 16 bits only

Re-derived here at `0x00A39E83`–`0x00A39EC0` before using it:

```
state = state*0x19660D + 0x3C6EF35F
return ((state & 0xFFFF) * (hi - lo)) >> 16 + lo
```

Only the **low 16 bits** of the LCG feed the result — an LCG's *worst* bits — and the range
is half-open, so `get(0, 0xFFFF)` never returns `0xFFFF`. A sibling lane had already landed
exactly this reading in `crates/don-sim/src/rng.rs`; the two derivations were independent
and agree, which is worth recording. This module uses that file rather than a copy.

### 2.5 The house edge

`calc_market_prices` floors the buy quote at `sell + 10` (`0x006DC3A5`). Combined with
`buy = 2*base + spread`, **every buy/sell round trip loses wealth at every price level** —
there is no arbitrage window at any point in the trend cycle. Tested.

Each 100-unit trade kicks the base price by `MARKET_SUPPLY_DEMAND` (3) — up on a buy, down
on a sell, floored at 0. A refused trade (insufficient wealth or stock) changes **nothing**,
including the price.

---

## 3. Taxation, tribute, caravans

**Taxation** is two independent things:

* *Territory* tax, inside `calc_gather`: `wealth += territory_tiles * rate * 16 / total_land_tiles`,
  where `rate = TERRITORY_TAXES[level]` = `0 / 50 / 100 / 200 / 300` percent, indexed by
  `LeaderData::get_taxation` (a descending chain of four `has_preq` tests, first hit wins).
  British doubles the rate; `total_land_tiles == 0` disables the whole block, divide
  included.
* *City* tax, `CityData::get_taxes`: `VILLAGE_TAXES + buildings * BUILDING_TAXES`, plus
  `MARKET_TAXES` if the city has a market (×4 with the Porcelain Tower), plus
  `TEMPLE_TAXES`. **With shipped data only the market term is non-zero**: 10 wealth per
  market, 40 with Porcelain.

**Tribute** — `LeaderData::scale_tribute` `0x006D5240` — has a three-regime rounding bias
that is not decoration:

```
pct = age * COMMERCE_TRIBUTE(7) + BASE_TRIBUTE(51)
amount < 20   -> (amount*pct)      / 100     truncate
amount < 100  -> (amount*pct + 50) / 100     round to nearest
otherwise     -> (amount*pct + 99) / 100     round up
```

So a 19-unit gift is rounded *down*, a 99-unit gift to nearest, a 100-unit gift *up*.
Tribute becomes lossless at age 7 (51 + 7×7 = 100) and from age 8 the `> 100` early-out
returns the amount unchanged — it never becomes profitable, just free.

**Caravans** — `LeaderData::get_caravan_limit` `0x006DCA50`: `age + 1` plus wonder/rare/civ
bonuses, hard-capped at **99**, then — when the caller asks for it — capped again by
`C(n, 2)` over the city count. **The pair cap is the binding one in practice**: caravans run
*between* cities, so three cities support three routes regardless of age.

The route's wealth value is now executable too. `Caravan::trade_value` starts with both
cities' `num_buildings + {0,2,4}` level values. It converts their raw positions to the
four-tile `WCoord` grid and chooses distance class 0/1/2/3 at `< map_width/4`, `< /2`,
`< 4*width/5`, or beyond. Nonzero classes scale the sum by `(class+3)/3`; a foreign route
then scales by `3/2`. Indian (+15%) and spice (+20%) terms apply sequentially for the
player receiving that endpoint's share, with truncation after every step.

`City::compute_trade` clears its signed 16-bit `trade_val`, walks the checksummed caravan
link array in insertion order, skips incomplete or stale endpoints, and adds
`(trade_value << 4) / 2` per route. A changed cache dirties the owner economy. That field is
consumed by the already-executable `calc_city_resources` and `do_gather` chain, so caravan
commerce now reaches the real wealth accumulator and stockpile rather than ending as a
detached calculator. `City::new_caravan` also executes its first-contact transaction: one
bit per source-city slot and caravan owner, awarding `10*(age+1)` wealth domestically or
`20*(age+1)` at a foreign destination exactly once.

The route is no longer handed to that calculation as a detached fixture. Eight
player-ordered `Caravan` pools now execute retail's stable-slot ownership rules: twenty
records and pointer capacity are allocated initially, capacity doubles on demand, the
first inactive record below the high-water mark is reused, and close shrinks only an
inactive tail. `Unit::do_trade`'s transaction appends
the `{caravan slot, owner}` identity to both cities in order, writes both endpoint object
handles, and marks the route established. Destination arrival independently marks it
earning. `Unit::end_trade_route` clears established/earning, removes the first exact link
from each city while preserving suffix order, and feeds the resulting arrays directly
back into `City::compute_trade`. Constructor state is capacity 0 / grow -1, but an ordinary
active `City::init` preallocates ten links; full live-city arrays therefore grow 10 → 20 →
40 and never shrink on removal.

Unit death now executes against `tech_cities::CityPool`, the ordinary checksum owner,
rather than a parallel fixture. The order is surprising and observable: `Unit::close`
calls `Caravans::close_caravan` first, resetting owner/endpoints and possibly shrinking the
pool high-water mark; only near its tail does `close_orders` kill order 15 and call
`end_trade_route`. The endpoint identities must consequently come from the still-live
`TradeOrder`, while the caravan is found through its allocated pointer-array slot without
an active/high-water predicate. That second phase clears unit flag `0x200` and caravan
flags `0x02/0x04`, removes the exact ordered link from both real City arrays, and recomputes
both `trade_val` fields. A focused test proves the Cities checksum returns to its baseline
after route creation, income activation, unit death, and teardown.

The last parallel cache between that field and gathering is gone. The city object loop at
`Leader::calc_gather +0x24A` walks exactly `0..city_mark` in increasing slot order, skips
inactive City records, invokes the per-city calculator, and wrapping-adds all six dwords.
The executable collector now does that against `tech_cities::CityPool`; in particular,
caravan wealth is sign-extended directly from the checksum-owned `CityRecord::trade_val`,
not supplied by a host-side fixture. Object/type facts that are not fields of City remain
an explicit fail-closed resolver input.

`City::compute_trade`'s other side effect is now connected too: it sets the owning
leader's `0x02000000` economy-dirty bit only when the signed trade cache changes. Route
creation and teardown therefore reach the eight-frame dirty recomposition schedule, then
the ordinary fractional gather accumulator and stockpile. The lifecycle test proves the
whole route → City cache → dirty frame 8 → gross wealth → stockpile path, plus the inverse
dirty/recompute on route end.

---

## 4. Corrections and new findings

Each of these came from disassembling the address rather than trusting prose. They are the
places where a transcription would have shipped a wrong simulation.

### 4.1 `Player::TickResource` and `Player::UpdateCommerceCaps` are not real names

The PDB names them `Leader::do_gather` `0x006CE450` and `Leader::calc_resource_caps`
`0x006CE900`. `docs/derivation/economy.md` §3 and `sim-economy.md` §3.1–3.2 use the invented
names throughout. Also renamed: `FUN_006CEEE0` → `Leader::calc_gather`, `FUN_006CE280` →
`Leader::gather`, `FUN_006D66A0` → `LeaderData::get_gather_handicap`, `FUN_006E1370` →
`LeaderData::has_tribe_bonus`, `FUN_006E33A0` → `LeaderData::type_avail`, `FUN_00664090` →
`TypeData::get_cost`, `FUN_006508C0` → `ObjectData::train_time`.

That last one **closes an open question**: `sim-economy.md` §3.3 said of the rate ramp
*"It is not train time… `x` comes from `UnitType` vtable `+0x6C`/`+0x70`"*. The PDB says the
enclosing function **is** `ObjectData::train_time`. The ramp is train-time arithmetic after
all. (Out of this lane's scope; flagged for whoever owns production.)

### 4.2 `LeaderData::calc_city_resources` has two modes and the census drops knowledge

`0x006D5530`. The `city` argument selects:

* **city mode** — walk the city's building list, then Forbidden City ×1.25, hero ×1.5,
  `CITY_GATHER`, Roman/German civ terms, `get_taxes`, `get_literacy`.
* **census mode** (`city < 0`) — collect buildings belonging to *no* city, sum their
  `BuildData::calc_gather`, add `VILLAGE_TAXES`, **return**.

Everything from `CITY_GATHER` onward is city-mode only, and knowledge enters *solely*
through `get_literacy` — so **the census contributes no knowledge at all**. This confirms
the tech-cities lane's observation and gives it its cause.

Two ordering facts that a plausible-looking port gets wrong:

* The Forbidden City multiplier is applied **before** `CITY_GATHER` is added, so it only
  ever scales the buildings' contribution.
* `FORBIDDEN_CITY_BASE_GATHER` **replaces** `CITY_GATHER[i]` rather than adding to it —
  and only where `CITY_GATHER[i]` is already non-zero, because the rule-is-zero guard comes
  first. The override cannot introduce income where there was none.

Retail oddity, reported not smoothed: `CityData::get_level` is called and stored at
`0x006D5570` on the *city* path, but the only reader of that slot is the `VILLAGE_TAXES`
term on the *census* path, where the value is the literal 1. The level is computed and
discarded. Shipped `VILLAGE_TAXES` is 0, so nothing observable rides on it.

### 4.3 The merchant multiplier replaces the baseline, it does not stack

`LeaderData::calc_rare` `0x006E08D0` sets `mult[res] = MERCHANTS_BONUS[level]` — it does not
add to 100. At level 4 a rare yields ×3.00, not ×4.00. The tail is also asymmetric
[measured, `0x006E0A80`–`0x006E0B4B`]: **slot 0 takes `bonus_a + bonus_b`, slots 1–5 take
only `bonus_b`**, so `FISHERMEN_BONUS` is food-only and never reaches the other five. Each
slot is guarded by `bonus != 0 || mult > 100`, so an untouched slot is left exactly alone
rather than round-tripped through `× 100 / 100`.

And the fish special case: for good types **6** and **0x1F**, the merchant multiplier applies
to every slot *except* food.

### 4.4 The gather-enhancer tables are indexed `level - 1`

[measured, `0x0063A43E`] `RULES[0x284 + (level-1)*4]` — `SCHOLAR_RATE[6]` is stored
in 8.8 as `{1280,1792,2560,3840,5120,6400}`, displaying `{5,7,10,15,20,25}`.
This is the same off-by-one the tech-cities lane found on `GRANARY_BONUS`,
`LUMBERMILL_BONUS`, `SMELTER_BONUS` and `FISHERMEN_BONUS`; confirmed here independently on
a fifth table. A port that indexes by `level` reads the next tier's number for every
building in the game.

### 4.5 The AI difficulty cheat is asymmetric under truncation

`LeaderData::get_gather_handicap` `0x006D66A0` returns a percentage added to 100, measured
by the difficulty lane at −35 (Easiest) to +50 (Toughest), a 2.29× end-to-end income spread
[reported]. The step is `income * (100 + h) / 100` with C truncation, so at negative `h` the
leftover fraction is **always discarded** and the effective penalty is strictly worse than
nominal, while at positive `h` the bonus is merely rounded down. Low difficulties are
harsher than the number suggests, and the error is largest for small incomes — early game.
A separate cost handicap (`LeaderData::get_handicap`) stacks at the extremes and is *not*
part of this function; not modelled here.

The module also pins that **the displayed income is captured before the handicap is
applied** (`0x006CE723` precedes `0x006CE72C`), so the HUD number is not the number that
pays.

### 4.6 `INCA_WEALTH_PER_MINER` is inert in shipped data

The gate at `0x006CF4B9` is `if (INCA_WEALTH_PER_MINER < 0)`, and the shipped value is
`10`. So the `wealth += metal` transfer **never fires** in an unmodded game. Same shape:
`RUSSIAN_COMMUNISM` ships as 0, so the fixed-100 market prices at `0x006DC3E8` are
unreachable. Both are implemented and both are tested in the off *and* forced-on states.

### 4.7 Smaller ones

* `Leader::do_gather`'s `×3/2` game-setting step is a plain `(x*3)/2` idiv (`0x006CE783`),
  not the bias-and-shift form; the knowledge `×3/4` at `0x006CE755` *is* bias-and-shift.
* `taj_caravan` is one of the seven fields present in the binary but absent from shipped
  `rules.xml` (`economy.md` §1.4), so it keeps the loader's `-1` default. The module takes
  it as an explicit argument rather than reading a slot that shipped data never writes.
* The commerce-cap civ branches are **mutually exclusive** and slots 4 and 5 have none.
* `LeaderData::calc_resource_bonuses`'s Conquer-the-World term is *additive from the
  pre-update value* (`out[i] += stacks * rate * out[i] / 100`), not a `× (100+x)/100`.
* `Leader::gather` **zeroes the six expense slots every frame** (`0x006CE2E8`), which is why
  expenses are per-frame rather than cumulative.

---

## 5. Not implemented, and why

Listed rather than guessed, per the charter. Each has a real gap behind it.

### 5.1 The three remaining object-graph loops inside `calc_gather`

The checksum-owned CityPool loop now executes. Gather-enhancing buildings, the band-2000
special builds of type `0x1A2`/`0x1A3`, and gathering units still need the world grid,
object registry and per-owner object bands. They are summed into one accumulator with no
intervening arithmetic, so the module retains one six-slot input for those three paths.
Inside each City, `BuildData::calc_gather` remains an explicit resolved sum.

### 5.2 `BuildTypeData::calc_gather` `0x00639E40` — the per-worker payout

7,668 bytes; the single largest remaining economy target. What I did extract [measured]:

* **`PEASANT_RATE` = 2560 at scale 256 → 10 resources per `GATHER_RATE`** (30 game-seconds)
  per worker, unscaled at `0x00639E40 +0x18D` as `(rate + bias) >> 8`.
* **`OIL_RATE` = 8960 → 35**, selected for oil gatherers at `+0x9E5`.
* `SCHOLAR_RATE[level-1]` as in §4.4.
* The civ/wonder terms it reads: `IROQUOIS_FOOD`, `FRENCH_WOODIES`, `TAJ_FARMS`,
  `KREMLIN_FARMS`, `GERMAN_MINERS`, `EGYPTIAN_FARM_WEALTH`, `RIVER_RESOURCE_VALUE`,
  `JAPANESE_FISHING_BOATS`, `INCA_WEALTH_PER_MINER`.

The scholar path loads that runtime 8.8 value, shifts it left four, then performs the signed
truncating `/256` idiom (`0x0063A445`–`0x0063A451`). The exact gross sequence is therefore
`{80,112,160,240,320,400}`; sixteen gross units become one stockpile resource over the
450-frame period, giving 5/7/10/15/20/25 knowledge per scholar. It multiplies by
`min(active_scholars, 7)` at `0x0063A4C2`–`0x0063A4D5`.

### 5.3 Resource depletion

Normal Farm/Woodcutter/Mine/University/Oil gathering has **no depletion pool or decrement**.
`WorldData::is_gathered_from` tests TData bit `0x1000`; `Build::find_gather_tiles` sets it,
and clear/verify paths remove it. It is an occupancy claim, not remaining yield. Worker
execution rotates the ordered `gather_from` coordinates without debiting them, while
`World::gather_at` is a pure six-slot terrain-value reader.

The goods checksum model is now exact too: `Good::walk_data` contributes `ever_seen +0x20`,
then `SubObject::walk_data` contributes flags, owner/object identity, position, and the
resolved `TypeIndex`, for 21 bytes. `GoodData` has no remaining-amount member. The separate
rare/merchant spatial path is still unreduced, so this does not assert when or why retail
may remove a rare Good object.

### 5.4 Remaining caravan movement and merchant targeting

Caravan ownership, checksum-owned City links, income activation, city recomputation,
unit-death teardown, record recycling, both `distance` overloads, `trade_value`, and the
`City::new_caravan` first-contact award execute. What remains is autonomous route selection and physical unit
choreography: `Caravan::restart_trade_route` `0x0073D070`, `Unit::think_caravan`
`0x005F5650`, the movement arms of `Unit::do_trade` `0x005ED270`, road construction, and
`PathFinder::astar_caravan_road` `0x00685990`. The road A\* draws RNG **per edge
relaxation** (`calc_road_cost` `0x00686341`), so this remaining movement layer is a heavy
lockstep-critical RNG consumer; it is no longer an income-formula gap.

Merchants: `UnitData::good_merchant_spot` `0x006068A0` and `ObjectData::is_merchant`
`0x0046D370` are the hooks; the merchant contribution enters through the same
`UnitData::calc_gather` path as other gatherers, so it is covered by the crowding divisor
but not by a derivation of its own.

### 5.5 Others

| thing | why not |
|---|---|
| `UnitData::calc_gather`'s spatial search | needs the world grid, the 8-way offset tables at `0x00CB7E90`/`0x00CBB0E0`, and `ObjectsData::find_good_at`. Only the crowding divisor is ported. |
| The Dutch interest threshold | built at `0x006CE666`/`0x006CE684`/`0x006CE69A` from a game-config table. Supplied whole. |
| The additive wonder terms on commerce caps | each has its own wonder/tech check; passing 0 models "no wonders" honestly. |
| `game[0x2D] == 8` infinite-resources mode | sets every stockpile to 99999 and skips the tick. Its gate is an underived game-config byte. |
| Every `has_tribe_bonus` / `has_wonder` / `has_preq` / `type_avail` query | player-property lookups; inputs, like the damage chain's predicates. |
| `LeaderData::get_handicap` (cost handicap) | a different function; named in §4.5, not modelled. |
| The `leader + 0x480` rate tracker | the per-resource consumption-spread counter at the tail of `do_gather` (`0x006CE868`), display-oriented; not ported. |

---

## 6. The checksum channels

### 6.1 What is served

* **leaders** (channel 8) — `LeaderEcon::image()` produces the byte image of the economy
  block; `leaders_channel()` adler-32s it across leaders in slot order.
* **goods** (channel 11) — `goods_channel()` over a `GoodNode` list, insertion-ordered.

`adler32` is reproduced from the algorithm (the checksum lane difftested `0x00A46830` at
500,000 calls / 0 mismatches) and is pinned here against four published vectors plus a
buffer that crosses the 5,552-byte `NMAX` boundary.

### 6.2 The obfuscation is load-bearing

**The economy block is XOR-obfuscated per field, and `CheckSums::check_all` hashes the
obfuscated bytes.** A checksum over plain values is wrong on frame 1. Recovered masks and
offsets [measured]:

| offset | field | mask |
|---|---|---|
| `econ + 0x00 + res*4` | stockpile | `^0x8221` |
| `econ + 0x18 + res*4` | fractional accumulator | `^0x3421` |
| `econ + 0x30 + res*4` | commerce cap | `^0x1281` |
| `econ + 0x4C + res*4` | capped flag (0/1/2) | `^0x8932` |
| `econ + 0x64 + res*4` | gross income | `^0x872` |
| `econ + 0x7C + res*4` | expense | `^0x26076` |
| `econ + 0x94 + res*4` | displayed income | `^0x90236` |
| `econ + 0xC4 + res*4` | per-source breakdown | `^0x6722` |
| `econ + 0xDC` | age (alt) | `^0x62766` |
| `econ + 0xF0` | age | `^0x63187` |

The block itself is reached as `*(Leader + 0x6EB8)` — a **pointer**, not an inline struct.
A zeroed field is therefore *not* zero in the image, which the module tests directly.

### 6.3 Honest limits — this does not yet match retail

Three gaps, all stated in the code:

1. **Unmodelled dwords.** `econ + 0x48`, `econ + 0xAC..0xC4` and `econ + 0xE0..0xF0` are
   inside the modelled range and we have not identified them; they are emitted as zero. So
   the leaders checksum is comparable between two runs of *this* implementation and is
   **not yet** comparable against the engine's. One live read of a running match's econ
   block closes this — it is the single highest-value next measurement for this lane.
2. **Reach.** `Leader::walk_data` `0x006D6750` hashes `leader[0x00..0x08]` then
   `leader[0x08..0x6932]` (26,914 bytes) then eight `0x5C`-byte sub-walks from
   `leader + 0x692C`. The econ block sits past the flat run at `+0x6EB8`, so it arrives via
   one of the eight sub-walks; which one is unresolved. Our framing hashes the same bytes,
   not retail's layout.
3. **Two hazards the full channel carries that this module does not supply**, both flagged
   in the code so they cannot be forgotten:
   * `LeaderData::anti_att` and `LeaderData::plunder_scale` are **`f32` inside walked
     state** — their bit patterns are hashed, so they must be reproduced bit-exactly and
     stored as `f32`, never promoted to `f64`.
   * **`Array<T>` hashes its capacity and its growth hint**, not just its live elements. Any
     growable container in sim state needs the engine's growth schedule; a Rust `Vec`
     diverges as soon as one push crosses a boundary, even with every element matching.

`market_adler32` is also exposed. The market lives on `Game`, so it is not itself a channel
— it reaches the checksum through the prices leaders trade at — but having it separately
lets a divergence be localised to the market instead of hunted through stockpiles.

---

## 7. Fidelity — the honest tier

**Tier C. Structure `[measured]`, behaviour never executed against retail.**

* No oracle run. Nothing in this file has been compared against the shipped machine code
  executing.
* The 95 focused tests verify *internal* consistency, ordering, truncation direction, RNG draw
  counts and determinism. They are not fidelity evidence and no test in the file claims to
  be. Where a test could only have been written by hand-computing what retail does, I
  removed it: two expectations I had hand-computed were **wrong** (the accumulator's
  payout schedule and the eight-frame recomposition lead-in), the implementation was right,
  and both are now invariant assertions instead. That is the "capture, do not calculate"
  rule biting in real time — worth recording, since it is the second time this project has
  paid for it.
* The rule constants are the strongest link: **102 slots, 0 mismatches** against
  `don-rules`'s live-validated table.

### What would raise it

1. **A `Leader::do_gather` / `Leader::calc_gather` oracle harness.** Same shape as
   `crates/oracle/src/damage_env.rs`: a fabricated leader array (stride `0x6EEC`), an econ
   block at `+0x6EB8`, `Game` at `[0x00C061EC]`, `RULES`, and stubs for `type_avail`,
   `has_tribe_bonus`, `get_gather_handicap`. Takes the whole pipeline to Tier B in one go.
2. **`GameDaemon::calc_market` is a much cheaper first target** — it takes one `int`, reads
   `RULES` and `Game`, and calls only `Random::get`. It would validate the market
   arithmetic *and* the RNG draw count, which is the highest-desync-risk item in the lane.
3. **A live read of one leader's econ block across a few frames.** Settles the unmodelled
   dwords (§6.3) and gives an integrated runtime check of the now-recovered income units.

---

## 8. Files

| path | what |
|---|---|
| `crates/don-sim/src/systems/economy.rs` | the module: 5,116 lines, 90 in-module tests, 149 cited VAs |
| `crates/don-sim/src/systems/tech_cities.rs` | fixes constructor `CaravanLinkArray::grow = -1` for the real Cities checksum state |
| `crates/don-sim/tests/caravan_trade_transaction.rs` | 5 executable caravan route, award, and stockpile integration tests |
| `crates/don-sim/tests/caravan_route_lifecycle.rs` | 5 executable pool, ordered-link, route lifecycle, and income reachability tests |
| `crates/don-sim/tests/caravan_city_unit_lifecycle.rs` | 3 executable ordinary-CityPool, checksum-restoration, and unit-death-order tests |
| `crates/don-sim/tests/city_income_lifecycle.rs` | 2 executable CityPool-order, trade-dirty, gather-schedule, and stockpile tests |
| `docs/mechanics/economy.md` | this report |

Nothing else was written, nothing staged, nothing committed. The module is wired
(`lib.rs` → `pub mod systems;`, `systems/mod.rs` → `pub mod economy;`, both landed by
sibling lanes) and green in-tree: the five focused commands above pass **105 tests, 0 failed**.
