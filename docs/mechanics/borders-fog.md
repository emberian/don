# Borders, territory, supply, attrition and fog of war

**Lane:** `mech:borders-fog` · **Checksum channel served:** `world` — `CheckSums::check_all`
`0x00936560` channel 12, i.e. `World::walk_data` `0x006B5CF0` (world.cpp:1265).
**Module:** `crates/don-sim/src/systems/borders_fog.rs` · **Fidelity tier: C** throughout.

---

## 1. What now works, and how it was measured

Everything below is implemented in the module, and the module compiles clean and passes
**46 tests**, `rustfmt` clean, `clippy` clean.

How that was measured, exactly — the module is *not* reachable from `don-sim`'s `lib.rs`
yet (see §9), so `cargo test` at the repo root does not touch it. It was built and tested
through a scratch crate that `#[path]`-includes the real file rather than a copy, so the
thing tested is the shipped file:

```sh
# /private/tmp/.../scratchpad/bfcheck/src/lib.rs
#[path = "/Users/ember/dev/don/crates/don-sim/src/systems/borders_fog.rs"]
pub mod borders_fog;
```

```
running 46 tests ..............................................
test result: ok. 46 passed; 0 failed
cargo clippy --lib   ->  0 warnings
```

| area | state |
|---|---|
| The four co-registered grids and the fine-unit ladder | **done**, cross-checked two ways |
| Territory / border computation, incl. per-leader bonus precompute | **done**, all constants named and valued |
| Incremental region scheduling + the 256-tile/frame budget | **done** |
| Territory ownership queries (`get_who`/`get_who2`/`is_enemy_territory`) | **done** |
| Fog planes, `set_seen`/`set_was_seen`/`clear_seen`, all queries | **done** |
| LOS disc table (`circle_init`) incl. its truncation quirk | **done**, regenerated not dumped |
| Per-object LOS stamping (`Object::update_seen`), full path | **done** |
| Stealth detection plane (`seen3`) | **done** |
| Attrition rate, period, firing test, damage shape | **done** |
| Anti-attrition (`Leader::calc_anti_attrition`) | **done** |
| Supply predicate structure | **done**; the supplier *search* is a caller input |
| `world` channel byte stream + adler32, per section | **done for 6 of 9 sections** |

Honest gaps are in §8. Nothing here has been through the oracle: this is transcription from
the instruction stream and the decompiled corpus, so it is Tier C — behaviourally faithful,
divergence **unmeasured**. Do not call it verified.

---

## 2. The headline result: borders are a *cap* plus a *contest*, not a spline

RTTI shows a `BorderSpline`, and the standing assumption was that borders are spline-based
and therefore had no closed form. **`BorderSpline` is presentation.** `game/borders.cpp` is
73 functions and `docs/derivation/architecture.md` already classifies it *presentation
only* — it is the code that draws the smooth border ribbon. The simulation underneath is a
per-cell integer ownership assignment with no splines in it at all.

The real mechanism has two independent parts, and separating them is the finding:

**(a) A hard radius cap** decides how far an *uncontested* border reaches. It is pure
addition, in tiles:

```
city_cap = territory_limit_base
         + gov_level        * territory_limit_civic
         + (city_level - 1) * territory_limit_city
         + temple_level     * territory_limit_city      (only if the city has its temple)
         + per-wonder / per-rare-resource extensions
fort_cap = fort_level * territory_limit_city + city_cap
```

**(b) A score function** decides *where two borders meet*. Lower wins:

```
score = (territory_den * d' * 256) / (B * multiplier + territory_num)
```

`d'` is the compressed distance (§4.3), `B` the summed border bonuses, `multiplier` is
`city_territory_multiplier` or `fort_territory_multiplier`. A claim is only admissible while
`score <= territory_base << 8`, which rearranges to exactly the comment shipped in
`rules.xml` next to `TERRITORY_NUM`:

> `(Numerator + (CityorFortMultiplier * BorderBonuses)) / Denominator`

**The cap always binds first.** Measured across every configuration the rules allow:

| configuration | B | effective radius | score gate would allow | hard cap |
|---|--:|--:|--:|--:|
| small city, temple, gov 0 | 2 | **48 tiles** | 91 | 48 |
| small city, no temple, gov 0 | 0 | **44 tiles** | 52 | 44 |
| major city, temple, gov 0 | 8 | **56 tiles** | 206 | 56 |
| major city, temple L4, Democracy (gov 7) | 29 | **96 tiles** | 609 | 96 |
| fort, no upgrades, gov 0 | 2 | **44 tiles** | 91 | 44 |
| fort L4, gov 7 | 23 | **84 tiles** | 494 | 84 |

So the score function is *never* what stops a lone city's border; it only arbitrates
frontiers. That is why the border looks like a fixed disc in an empty map and like a
pressure boundary between two players — and it is pinned by the test
`the_hard_cap_binds_not_the_score_gate`.

Units: **1 tile = 192 fine units**, and the whole table above is in tiles.

---

## 3. The coordinate ladder (this unlocked everything else)

`World::init(u16 xs, u16 ys)` `0x006B76F0` derives four co-registered grids:

| grid | dims | cell size | holds |
|---|---|---|---|
| **WCoord** | `xs × ys` | 4 tiles | `WData[size]` — **territory**, land, region, blocked, `was_seen` |
| **FCoord** | `2xs × 2ys` | 2 tiles | `seen`, `seen2`, `seen3` — **the fog planes** |
| **TCoord** | `4xs × 4ys` | 1 tile | `TData[tile_size]`, 2 bytes each |
| region | `xs/2 × ys/2` | 8 tiles | `danger[8][reg_size]` |

Fine-unit conversion is `div_3_table[c >> k]` where `int *div_3_table` is `0x00CAE5FC` and
`div_3_table[i] == i / 3`. The three shifts in the corpus are `>>6`, `>>7`, `>>8`, which
after the `/3` are `/192`, `/384`, `/768` — tile, fog cell, WCoord cell. Object positions
are XOR-obfuscated with `0x63637` and must be deobfuscated first.

Two independent confirmations that a tile is 192 fine units:

1. `String::fraction` scale 192 is used for `unit_formation_spacing` (`"1/16 tile"` stores
   12 = 192/16) and `unit_move_speed` (`"1/192 tile"` stores 1).
2. `Object::update_seen` computes its LOS radius as `(los * 0xC0) / 0x180` — literally
   `192 / 384`, one tile over one fog cell.

**Correction to carry forward.** `README-LLM.md` says the pathfinder step "48 = ¼ tile" and
notes the search is parameterised `48 / 192 / 768`. With 1 tile = 192, that ladder is
`¼ tile / 1 tile / 1 WCoord cell` — 48 is a **quarter tile**, which is what the README says,
but 192 is a *whole* tile and 768 is a whole WCoord cell, which is worth stating because the
three numbers are exactly the three grid steps rather than an arbitrary triple.

---

## 4. Territory — `World::compute_reg_territory` `0x006B0BB0`

4,039 bytes, the largest function in `world.cpp`. Structure from
`re/decomp-all/006b0bb0.c`, every constant offset resolved against
`docs/derivation/rules-constants.json` and re-confirmed against `ron-data/rules.xml`, and
the three load-bearing offsets re-read from the instruction stream.

### 4.1 The constant block is contiguous and self-confirming

The `rules.xml` declaration order maps onto the `Constants` offsets with **no gaps**, which
independently confirms the whole block:

```
0x0B8 FORT_UPGRADE_TERR     {2,4,6,9}          0x114 TERRITORY_BASE        24 tiles
0x0C8 TEMPLE_UPGRADE_TERR   {2,4,6,9,12}       0x118 TERRITORY_LIMIT_BASE  44 tiles
0x0DC CIVIC_UPGRADE_TERR    {0,1,2,4,6,8,11,14} 0x11C TERRITORY_LIMIT_CIVIC 4 tiles
0x0FC CAPITAL_TERRITORY_BONUS 6                0x120 TERRITORY_LIMIT_CITY   4 tiles
0x100 CITY_UPGRADE_TERR     {0,3,6}            0x124 TERRITORY_DEN          5
0x10C FORT_TERRITORY_MULTIPLIER 4              0x128 TERRITORY_NUM          11
0x110 CITY_TERRITORY_MULTIPLIER 4
```

`0x100` was the one hole — `docs/derivation/rules-constants.json` lists it as
`city_level_territory_bonus` with **no captured value**, because the loader name and the XML
tag differ. It is `CITY_UPGRADE_TERR entry0="0" entry1="3" entry2="6"`
(`ron-data/rules.xml:70`). The hole is closed.

`World::init` seeds *both* limit triples — `player_territory_limit{,_civic,_city}` at
`World +0x38/0x3C/0x40` and `colonized_territory_limit*` at `+0x44/0x48/0x4C` — from
`Constants +0x118/0x11C/0x120`, i.e. they start equal. `Region` flag bit `4` selects which
triple a region uses.

### 4.2 Two corrections to what the decompiler suggested

Both re-read from the instruction stream, because both change *which rule* drives a mechanic.

**(i) The "capital" bonus in the leader precompute is actually the Gems rare resource.**
At `0x006B0F3F`:

```asm
test byte ptr [esi + 0x6da6], 0x80
jne  0x6b0f54
test byte ptr [esi + 0x6dce], 0x80
je   0x6b0f7c
...
mov  eax, dword ptr [eax + 0x930]     ; GEMS_TERRITORY_BONUS = 2
```

`+0x930`'s `rules.xml` neighbours are `DIAMONDS_COMMERCE` and `ALUMINUM_AIR_COST`, so this
is unambiguously the rare-resource block, and `+0x6DA6` / `+0x6DCE` are the two rare-resource
slots. `capital_territory_bonus` (`+0xFC`) is **not** used here — it is a *per-city* term,
applied inside the tile loop when the city carries the capital flag.

**(ii) Tikal's border bonus is driven by `TIKAL_TEMPLE_HP`, not `TIKAL_TEMPLE_BORDERS`.**
At `0x006B0DC9`, under `has_wonder(0x214)`:

```asm
mov ecx, dword ptr [ecx + 0x4a0]      ; TIKAL_TEMPLE_HP
```

XML declaration order puts `TIKAL_TEMPLE_BORDERS` at `+0x498`, `TIKAL_TEMPLE_RANGE` at
`+0x49C` and `TIKAL_TEMPLE_HP` at `+0x4A0`. Both ship as `50%`, so **no shipped number
moves** — but this is a wiring bug in the retail game: editing `TIKAL_TEMPLE_BORDERS` has no
effect on borders, and editing `TIKAL_TEMPLE_HP` silently changes them.

### 4.3 The per-tile algorithm

For each WCoord cell, at tile position `(wx*4 + 2, wy*4 + 2)`:

1. **Leader scan order rotates with tile x**: `slot = (k + wx) & 7` for `k in 0..8`. This is
   the same species of determinism device as `Objects::process_all`'s `(frame + i) % 10`, and
   it is what breaks ties without an RNG draw. Reproduce it or diverge.
2. For each live city / fort of that slot: `d = hypot_approx(|dx|, |dy|)` in tiles.
3. Reject if `d > cap` (the §2(a) cap).
4. **Compress**: `if d<13: d=2d/3` then `if d<9: d=2d/3` then `if d<5: d/=2`. Only bites
   inside ~3¼ tiles; it makes the cells nearest a source claim overwhelmingly hard.
5. Score (§2(b)); track best and runner-up.
6. Write `WData::who` = winner, `WData::who2` = runner-up.

`hypot_approx(a,b)` is `mn²/(2·mx) + mx` (with a `mn < 60000` overflow guard falling back to
`(mn + 2·mx) >> 1`) — the second-order Taylor form of `sqrt`. **The identical function
appears in `circle_init`**, so the fog disc and the border disc use the same metric.

A city's `+0x5F` field selects *whose bonus table* applies. When it disagrees with the slot
the city is filed under, the claim is recorded as owner **`-2`**, not as that player. `-1` is
unowned. Faithfully reproduced; the meaning of `-2` (contested? captured-but-unassigned?) is
not yet pinned.

### 4.4 Scheduling — borders settle over many frames

`GameDaemon::check_borders` `0x00732060` runs once per tick inside
`GameDaemon::process_all`, *before any unit moves*. It walks a **fixed 64 region slots**
(`stride 0x88`, loop bound `0x2200` = 64 × 136) and, for each region with
`region.size > region.borders`, resumes `compute_reg_territory` from that region's cursor.

`GameDaemon +0x24` is a **global budget of 256 cells per frame** (`if (budget >= 0x100)
return`). So on a 32×32 WCoord map a full border resolve takes 4 frames, and on a large map
borders visibly *creep* after a city is founded or razed. Any replay comparison that expects
territory to settle within one tick will desync.

---

## 5. Fog of war

### 5.1 Three planes, one bit per player

`WorldData` holds three `fog_size`-byte planes, each byte a bitmask over the 8 player slots,
tested against `LeaderData +0x6929` (a single-bit player mask):

| plane | offset | meaning | cleared |
|---|---|---|---|
| `seen` | `+0x15C` | currently visible **this frame** | every tick, `World::clear_seen` |
| `seen2` | `+0x160` | ever explored | never |
| `seen3` | `+0x164` | **detected** — the stealth counter-plane | every tick |

Plus `wcoord_seen` (`+0x168`, `size` bytes) and `WData::was_seen` (`+0x14`), both coarse
explored bitmasks at WCoord resolution.

`World::set_seen(fx, fy, player, detect)` `0x006B3C60` writes all of them in one go and
**returns whether the explored plane changed** — that boolean is the "newly explored, run
`reveal_fog`" trigger, which is how goodie huts and first-sighting events fire.

Query semantics, each with its own short-circuit set (they are *not* interchangeable):

* `is_really_seen` `0x006B42C0` — raw visibility, honours `see_all` (`0x800`), the reveal
  counter, and `see_own_territory` (`0x2000`, grants vision over allied-owned WCoord cells).
* `is_seen` `0x006B55C0` — the above plus the `Game +0x30` fog option (`3` ⇒ always true).
* `was_really_seen` / `was_seen` — explored, with the option threshold at `>= 2` / `3`.
* `is_detected` `0x006B48C0` — **no short-circuits at all**. `see_all` does *not* grant
  detection, so a cloaked unit stays hidden even from an all-revealing observer unless a real
  detector covers it. Pinned by a test.
* `is_detected_by_enemy` `0x006B50F0` — the same plane masked with the *complement* of your
  bit.

### 5.2 The LOS disc is a precomputed table, and it truncates

`circle_init` `0x006817F0` fills `char *circle_x` `0x00CB7E90`, `char *circle_y`
`0x00CBB0E0` and `int *circle_radius` `0x00CBE330`: every integer offset with
`hypot_approx(|dx|,|dy|) == r`, emitted ring by ring, with `circle_radius[r]` the cumulative
count out to radius `r` (0..64).

The module **regenerates** this rather than dumping bytes — `circle_init` is small,
self-contained and touches nothing else, so reproducing it is strictly better than shipping
a 12 KB blob we would have to keep in sync.

Quirk worth knowing, and reproduced: the routine has a capacity guard
`if (count > 0x3248) { fill radius[r..=64]; return; }`, and **it fires part-way through
ring 64** — 392 offsets qualify at radius 64, only 300 fit. So a maximum-radius LOS disc is
*clipped* in the retail engine. Measured, and pinned by
`circle_init_truncates_its_own_outermost_ring`.

`Object::update_seen` `0x00651B80` stamps `circle_radius[r]` cells around the object where
`r = (los_tiles * 192) / 384`, clamped to 64 — i.e. **radius in fog cells is LOS in tiles
halved**, because a fog cell is two tiles. A detector stamps its whole disc into `seen3`.

### 5.3 Per-player observability for the RL environment

`Fog::visible_mask_for(p)` and `explored_mask_for(p)` are one byte-test per cell over the
raw plane, no allocation, returning iterators. That is the cheap partial-observability view
the env needs; the planes are already stored player-major-by-bit rather than per-player, so
there is nothing to transpose.

---

## 6. Supply and attrition

### 6.1 The chain, in `Unit::process` order

```
Unit::process_attrition  -> writes Unit +0x9E, the attrition period in frames
Unit::process            -> if period != 0 && (Game::frame + unit_id) % period == 0:
                                if !Unit::process_supply():  Unit::suffer_attrition()
                                else:                        unit.flags2 |= 0x40000
```

**Supply gates attrition**: an in-supply unit takes none. The `+ unit_id` phase offset is
what stops a stack all ticking on the same frame; it is deterministic and load-bearing.

### 6.2 The rate — and `anti_att` is 256-scaled

`UnitData::get_attrition(by_player)` `0x00608FD0` returns a scalar the caller turns into a
period:

```
period = max(1, (Constants::attrition * v) >> 8)          attrition = 48 frames
```

`v` is built from a fixed-point `scale` (0x100 normally, `25600/(100 - siege_attrition)` for
the siege class) times the victim's `anti_att` times the `binary32` at `0x00B69430`.

That constant is `0x3B800000` = **1/256**, read from the image, and the instruction sequence
is `cvtdq2ps / mulss [leader+0x7F4] / mulss [0xB69430] / cvttss2si` — so the association
order is not a Ghidra reordering artefact. The two 256s cancel **because `anti_att` is
itself 256-scaled**: `Leader::calc_anti_attrition` `0x006CDCC0` seeds it with the immediate
`0x43800000` = `256.0f`. This is the detail that makes the whole chain come out right, and I
got it wrong first: a hand-written test that assumed `anti_att ≈ 1.0` produced a 1-frame
attrition period, which is what sent me to the instruction stream. *Capture, do not
calculate.*

Measured consequences, all pinned by tests:

| situation | period |
|---|--:|
| baseline (`anti_att` 256, attacker `attrition` 1) | **48 frames** = 3.2 game seconds |
| attacker attrition 2 / 4 | 24 / 12 frames |
| Supply upgrade (`attrition_upgrade[1]` = 50%) → `anti_att` 512 | doubles the period |
| militia (`get_bonus(0x42)`) — `militia_attrition` 300% | 12 frames |
| siege class — `siege_attrition` 50% reduction | doubles the period |
| Statue of Liberty (`liberty_attrition` 100%) → `anti_att` 0 | **never**, `v = 0` |
| attacker one age ahead ×4 (`attrition_aged_up` 25%) | rate ×2 |

`anti_att` is one of the three floats the project already flags as living inside walked
state, so it is kept as `f32` — rounding it to an integer here would be a silent fidelity
loss.

### 6.3 The damage shape

`Unit::suffer_attrition` `0x005E1A10` calls
`Object::take_damage(int damage, char, char, …)` in two distinct shapes:

* `ObjectType +0x308 == 1` → `take_damage(1, 0, …)` — a flat point.
* otherwise → `take_damage(0, n, …)` with `n = 6` if `UnitData::curr_uber_size()` is 3, else
  `16 / curr_uber_size()`.

The `int damage` slot being **0** in the common case says the `char` argument is a
*fractional* damage form. What that fraction denominates is `Object::take_damage`'s
business and is **not derived here** — flagged for the combat lane.

### 6.4 Supply

`Unit::process_supply` `0x005E0560` is in supply if, in this order: not already flagged
(`Unit +0x68 & 0x400000`), not an always-supplied type (`ObjectType +0x2B8 & 0x40`), not
militia (`get_bonus(0x42)`), and then **any** of `Supplies::find_supply(x, y, owner) >= 0`
or a radius-qualified owned hero of type `0x16B`, `0x176`, `0x16E` from the walked
`HeroData` registry.

Relevant constants: `supply_radius` 14 tiles, `supply_radius_upgrade` 2 tiles,
`supply_hp_upgrade` `{0,20,40,60}`, `supply_heal_rate` 0 frames (= do not heal),
`french_free_supply` 1, `versailles_supply_heal_rate` / `french_supply_heal_rate` 20 frames.

The application point is `UnitData::recharge` `0x0060FDF0`. Non-siege units return their
type's base recharge without a supply query. Siege units return the base when the
under-attack rule permits it and `UnitData::in_supply` `0x00609EF0` succeeds; otherwise
Bombards (`ObjectData::is(0x10B, 0)`) use **2×** base and other siege units use signed
truncating **3/2×** base. `in_supply` returns true for non-land units and friendly WData
territory, then walks only `Supplies::find_supply`—the hero arm above is not part of this
narrower predicate.

The supply-specific arm of `Unit::process_healing` is at
`0x005E0F1A..0x005E0FFC`. Its shipped base rate is zero; the French bonus adds 20 frames,
and completed Versailles contributes 20 alone or combines as `(rate + 20) / 4`. On the
per-object due phase it requires `Supplies::find_supply >= 0` and calls
`Unit::repair_damage(1, 1, 1)`. Other healing families earlier in the function can
pre-empt or compose with this arm and must remain separate host inputs.

---

## 7. The `world` checksum channel

`World::walk_data` `0x006B5CF0` is a chain of `if (sec < 0 || sec == N)` guards;
`check_all` passes `-1` (all). Recovered section list, with lengths:

| § | content | length |
|--:|---|---|
| 1 | `xs`, `ys` | 8 B |
| 2 | 4 × `SimpleArray<WCoord>` start positions | var |
| 3 | 2 × oil position arrays | var |
| 4 | `[World+0x08, +0x80)` — all derived sizes, **the six territory limits**, resource totals, `seed` | 120 B |
| 5 | `WData[size]`, bytes `[0, 0x15)` each — **`who`, `who2`, `was_seen` ride here** | `size × 21` |
| 6 | `TData[tile_size]` then **`seen`, `seen2`, `seen3`** | `tile_size×2 + 3×fog_size` |
| 7 | `wcoord_seen[size]` | `size` |
| 8 | `danger[8][reg_size]` | `8 × reg_size × 4` |
| 9 | per-`WData` `CollBlock` | var |

Two things worth stating plainly:

* **All three fog planes are inside the sync checksum.** `docs/derivation/architecture.md`
  §7.3 notes that `CheckSums::check_seen` runs only from the save/load verifiers, which is
  true — but it is a *different* function. The planes themselves are walked by
  `World::walk_data` section 6, which is channel 12 of `check_all`. Fog is lockstep-critical.
* **`danger` is per-region, not per-tile** — 8 planes of `reg_size` ints, one per player. The
  region grid is 8 tiles per cell.

`WorldChecksum` reproduces the byte stream for sections 1, 4, 5, 6, 7, 8 and adler32s each
separately plus the concatenation, so a replay harness can diff *per section* instead of
staring at one mismatched 32-bit word. Sections 2, 3 and 9 belong to the worldgen and
collision lanes and are emitted empty — so `partial` is deliberately named: it is **not** the
complete channel-12 digest yet.

The adler32 is the standard zlib one (`adler32` `0x00A46830` is vendored zlib), verified
against three known vectors. **Assumption not yet checked:** that `CheckSum::walk_function`
feeds each walked range into a single running adler32 in walk order. That is the natural
reading of the `DataWalk` interface but it has not been confirmed against
`CheckSums::check_all`'s actual accumulation, and if it is wrong the section digests are
still useful but the combination is not.

---

## 8. Honest gaps

1. **No oracle coverage.** Nothing here is differentially tested against retail. Tier C.
   The highest-value oracle case is `UnitData::get_attrition` `0x00608FD0` — pure integer
   plus one float, small argument surface, and it is the single number the whole attrition
   system hangs on.
2. **`Object::update_seen`'s incremental path is not ported.** Only the full restamp. The
   engine's `incremental != 0` branch swaps in the `ring_init` `0x00681920` tables
   (`0x00CB1960` / `0x00CB4BB0` / `0x00CB7E00`) and starts at `circle_radius[r-1]` so only
   the outermost ring is redone. It is an optimisation over the same planes, **but it
   changes which cells get `set_seen` called on them and therefore which cells fire
   `reveal_fog`** — so it must be ported before a replay comparison is trusted.
3. **`don-sim` still does not own the walked `Supplies`/`HeroData` registries.** Its local
   kernels take resolved inputs. The Arena world now owns and walks those live registries
   for process-supply, recharge and the isolated supply-healing arm; other hosts must still
   provide the same ordered object lookup.
4. **Non-friendly attrition-period selection and the other healing families remain open.**
   Arena executes the exact 32-frame reset/friendly-territory return and fails closed at
   the diplomacy/leader/object-graph boundary. Its healing host likewise rejects cases
   where an unintegrated earlier healing family or multi-slot object could compose with the
   isolated supply arm.
5. **`reveal_fog` `0x006B3D30` is only partly understood.** The module records *which* cells
   newly explored; the function's own body (goodie-hut pickup, first-sighting messages,
   `Good` reveal) is not ported.
6. **The meaning of `who == -2`** is reproduced but not explained.
7. **`GameDaemon::update_all_seen`'s ally-vision merge** (the final loop, gated on
   `Game::frame == 0`) is not ported — it ORs `seen2` across team members at game start.
8. **`Region` flag bit 4** selects `player_*` over `colonized_*` limits; what sets it is not
   derived. Since `World::init` seeds both triples identically it currently makes no
   difference, but Conquer-the-World presumably diverges them.
9. `WData` fields outside this lane (`land`, `region`, `blocked`, `val`, …) are carried as
   inert state so the checksum stream is byte-exact, but are written by worldgen, not here.

---

## 9. One line needed from the lane that owns `lib.rs`

`crates/don-sim/src/systems/mod.rs` already exists (created by `mech:ammo`) and **already
lists `pub mod borders_fog;`** — nothing to add there. But `crates/don-sim/src/lib.rs` does
not declare the `systems` tree at all:

```rust
pub mod systems;     // <- needs adding to crates/don-sim/src/lib.rs
```

Until that lands, nothing under `systems/` is reachable from the crate and `cargo test` at
the repo root does not cover it.

`borders_fog.rs` has **zero `crate::` or `super::` dependencies** outside its own test
module, so it compiles standalone regardless of the state of its sibling modules — which
matters, because `systems/mod.rs` warns that several siblings depend on crate-root items
that do not exist yet. If the tree does not build, this module is not the reason.

---

## 10. Function index

| VA | symbol | ported |
|---|---|:--:|
| `0x006B76F0` | `World::init(u16,u16)` | grids only |
| `0x006B0BB0` | `World::compute_reg_territory(int,int)` | ✔ |
| `0x00732060` | `GameDaemon::check_borders()` | ✔ |
| `0x006B5700` | `World::compute_all_territory()` | via `check_borders` |
| `0x006B3C60` | `World::set_seen` | ✔ |
| `0x006B41D0` | `World::set_was_seen` | ✔ |
| `0x006B2250` | `World::clear_seen` | ✔ |
| `0x006B3D30` | `World::reveal_fog` | trigger only |
| `0x00732840` | `GameDaemon::update_all_seen` | partial (no ally merge) |
| `0x00651B80` | `Object::update_seen(int)` | full path only |
| `0x006817F0` | `circle_init` | ✔ |
| `0x00681920` | `ring_init` | ✘ |
| `0x006B55C0` | `WorldData::is_seen` | ✔ |
| `0x006B53F0` | `WorldData::was_seen` | ✔ |
| `0x006B42C0` | `WorldData::is_really_seen` | ✔ |
| `0x006B54F0` | `WorldData::was_really_seen` | ✔ |
| `0x006B48C0` | `WorldData::is_detected` | ✔ |
| `0x006B50F0` | `WorldData::is_detected_by_enemy` | ✔ |
| `0x006B4700` | `WorldData::get_who` | ✔ |
| `0x006B2510` | `WorldData::get_who2` | ✔ |
| `0x006B4D80` | `WorldData::get_whose` | ✔ (identical to `get_who`) |
| `0x006B2490` | `WorldData::is_enemy_territory` | ✔ |
| `0x0063ECA0` | `WallData::in_unfriendly_territory` | ✔ (same predicate) |
| `0x005E11A0` | `Unit::process_attrition` | period selection |
| `0x00608FD0` | `UnitData::get_attrition` | ✔ |
| `0x006CDCC0` | `Leader::calc_anti_attrition` | ✔ |
| `0x005E1A10` | `Unit::suffer_attrition` | damage shape |
| `0x005E0560` | `Unit::process_supply` | predicate structure |
| `0x005E0670` | `Unit::process_healing` | supply arm integrated in Arena |
| `0x00609EF0` | `UnitData::in_supply` | integrated in Arena |
| `0x0060FDF0` | `UnitData::recharge` | integrated in Arena |
| `0x0073ABA0` | `Supplies::find_supply` | walked Arena host; no shared don-sim registry owner |
| `0x006B5CF0` | `World::walk_data` | 6 of 9 sections |
