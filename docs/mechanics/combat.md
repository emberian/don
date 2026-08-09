# Combat: the resolution loop around `ObjectData::get_damage`

Lane `mech:combat`. Module: `crates/don-sim/src/systems/combat.rs`.
Checksum channels served: **`units`** (`CheckSums::check_units` `0x009371D0`) and
**`deaths`** (`CheckSums::check_deaths` `0x00936BB0`).

Everything below is `[measured]` against `ron-bin/riseofnations.exe`
(sha256 `30478a44…625079`), `ron-bin/sbl/rise.pdb` and `ron-data/rules.xml`, unless a line
says otherwise. Fidelity is **Tier C throughout** — structure and constants read out of the
binary, never executed. There is no oracle harness for `Object::do_damage` or
`Object::take_damage`, and I did not build one; do not read a fidelity claim into the fact
that this sits next to the Tier-B damage chain.

---

## What now works, and how it was measured

`crates/don-sim/src/systems/combat.rs`, **66 tests, all green**, two ways:

```sh
rustc --edition 2021 --test -o /tmp/combat-test crates/don-sim/src/systems/combat.rs && /tmp/combat-test
cargo test -p don-sim --lib combat      # 66 passed, after adding `pub mod combat;` to systems/mod.rs
```

The module is `std`-only with **no `crate::` imports**, so it builds standalone while
sibling lanes are mid-flight. `pub mod combat;` is added to `crates/don-sim/src/systems/mod.rs`
(that file's own header instructs lanes to do this); `cargo check -p don-sim` is clean.

| what | retail source | state |
|---|---|---|
| The combat rules block, 32 constants with offsets, parser and stored value | `Constants` `+0x18…+0xC20`, `Constants::init` `0x00569A90` | done, `CombatConstants::shipped()` |
| `vector_dist`, the integer hypot every range test uses | `0x0046CFF0` / `0x0046D060` | done, exact |
| The global tile-scan spiral, generated | `circle_init` `0x006817F0` → `0x00CB7E90`/`0x00CBB0E0`/`0x00CBE330` | done, exact |
| `RECHARGE` and the artillery penalty | `UnitData::recharge` `0x0060FDF0` | done |
| The attack cycle (`recharging`, byte-wrapping) | `Unit::fight` `0x005FD4D0` @ `0x005FF0A4` | done |
| Range gates in `× 192` space | `poor_target` `0x0064A270`, `attack_dist` `0x006488F0` | done |
| Held-target positioning transaction | `Unit::find_attack_pos` `0x00601280` | unit arm executable; building perimeter stream is a mandatory host boundary |
| `Object::poor_target` — the "don't chase" rule | `0x0064A270` | done |
| Overkill: window anchor, stamp, last-damager pair, attenuation predicate | `0x0064A5EB…0x0064A641`, `get_damage` step 23 `0x00644B94` | done |
| The applier's scaling: 8.8 multiplier, `0x100` floor, rounds, `uber_size`, sixteenths | `0x0064A6F9…0x0064A7F1` | done, both branches |
| Splash: scan set, radii, per-victim multiplier, whole-attacker gates | `0x0064C10C…0x0064C4DD` | done |
| Flank gate + arcs + the cavalry/vehicle sub-multiplier | `get_damage` `0x00644A9B…0x00644B7B` | done |
| Height and entrenchment guards and terms | `0x00644CF5`, `0x00644D7F` | done |
| Hit points, the 1/16 accumulator, death, uber cascade | `Object::take_damage` `0x00652020` | done |
| Corpse slot hold | `Object::die` `0x00647080` | done |
| `DeathObj` record, ring, slot-eviction policy | `Objects::add_death` `0x00653B60` | done |
| `adler32` + the `CheckSum` channel object + `check_deaths` | `0x00A46830`, `0x00936560`, `0x00936BB0` | done |
| Combat's byte-exact contribution to the `units` walk | `Unit::walk_data` `0x0060CF40` | done, as offset patches |

The balance matrix is **not** embedded and **not** re-implemented: `crate::balance` already
loads `schema/live/balance-real.bin`, so `balance_percent()` takes its slice. A test reads
the real file: 486,098 bytes, 243,049 `int16`, range **5…2574**, no negatives, 120,872
entries at exactly 100. It skips with a message if the (gitignored) file is absent.

---

## The findings that change other people's work

### 1. The overkill constant and window, exactly

`OVERKILL_FRAMES = 30` sim frames = **2 game seconds** (`Constants +0x5C`, `wtoi`).
`OVERKILL_DAMAGE = 85`, from `String::fraction(256)` of the XML text `"1/3"` — so the
multiplier is `85/256 = 0.33203125`, applied as `D * 85 / 256` at `0x00644C35`. Not `1/3`.

Three things about the mechanism that the community formula gets wrong:

* **The window is anchored to the first hit, never slid forward.** `Object::do_damage`
  re-stamps `UnitData +0x4C damage_frame` *only* when `frame - damage_frame >= 30`
  (`0x0064A605`). Two hits at frames 1000 and 1029 leave the anchor at 1000; a hit at 1030
  re-anchors.
* **It only fires for a *different* attacker.** The step-23 predicate requires
  `attacker->get_captain() != defender->damage_o` (`0x00644C0D`). One unit sustaining fire
  on one target is never attenuated. `damage_o`/`damage_who` (`UnitData +0xA4`/`+0xA9`) are
  written at `0x0064A62E`/`0x0064A641` — this is the "last damager" pair, and it stores the
  attacker's **captain**, not the attacker.
* **The `rules.xml` clause "does not apply to damage from buildings" is two gates**:
  `attacker->is_unit()` *and* `attacker->max_range() != 0`. The second one also excludes
  melee, whose `max_range` is 0.

Extra, both at the applier: `defender.unit_masks & 0x10` **doubles** the damage
(`0x0064A5DD`) immediately before the stamp; and the splash recursion passes `1` in
`do_damage`'s 8th argument, so **splash victims never stamp the window and never
retaliate** (`0x0064A59F`).

### 2. `uber_size` is very much read — the groups-guys lane's negative is wrong on this field

A per-procedure disassembly of the whole `.text` (20,413 PDB procedures, disassembled from
each procedure's own entry so the linear-sweep desync does not hide hits) for
`[reg + 0x308]` finds **28 sim-side sites**, including:

```
0x0060A803  UnitData::uber_size          mov eax,[eax+0x308]     <- a dedicated accessor
0x006448B9  ObjectData::get_damage       idiv dword [ecx+0x308]  <- step 13, the "splash divisor"
0x005F949D  Unit::same_damage            idiv dword [ecx+0x308]
0x0060E19E  Unit::repair_damage          idiv dword [ecx+0x308]
0x0065293E  Object::take_damage          idiv dword [ecx+0x308]
0x006528C9  Object::take_damage          mov esi,[eax+0x308]
0x0064A75C  Object::do_damage            mov ecx,[eax+0x308]
0x00609CD7  UnitData::total_damage       mov esi,[ecx+0x308]
0x00610A97  UnitData::get_capture_value  mov eax,[eax+0x308]
+ `cmp …,1` guards in Unit::come_out x4, Unit::go_inside, Unit::suffer_attrition,
  Object::eject_contents, Guy::set_anim, Objects::init_unit, UnitType::find_nearby_spot x2
```

I think both lanes are right about different things. `uber_size` is **not** `guy_mark` and
has nothing to do with `Guy`s: an "uber" unit is several *`Object` slots* sharing a captain.
`Object::take_damage` divides `hits(0)` by `uber_size` to get one sub-object's hit points,
and forwards the overflow to `get_captain()` with a nested `take_damage(overflow, 0, …)` at
`0x006533E6` — never to a `Guy`. So "soldiers are not killed by damage in this build" can be
true at the same time as "`uber_size` splits a unit's hit points N ways".

This also closes `docs/derivation/combat.md` §9's *"What `defenderType[+0x308]` is … Not
named by any loader binding I could find"*: the PDB names it **`UnitTypeData::uber_size`**.

`Object::do_damage` `0x0064A766` divides by the **attacker's** `uber_size`, and
`get_damage` `0x006448B9` by the one belonging to whichever object `get_damage` is called
on. Worth someone re-checking which is which in `mechanics.rs`'s
`DamageInput::defender_splash_divisor`.

Structure for the take_damage cascade is from `re/decomp-all/00652020.c` (a hypothesis); the
four `idiv` sites are from capstone.

### 3. `ATTENUATE` and `TO_HIT` belong to `Ammo`, and nowhere else

Same scan for `[reg + 0x1F0]` (`ObjectTypeData::attenuate`) and `[reg + 0x1EC]` (`to_hit`).
Outside constructors, `UnitType::init`/`BuildType::init`, `Objects::init/clear` and
`log_data`, there are exactly three consumers, all in one function:

```
0x0067C47E  Ammo::init   mov  esi,[eax+0x1EC]      ; to_hit
0x0067C4B5  Ammo::init   imul ecx,[eax+0x1F0]      ; attenuate
0x0067C4F2  Ammo::init   mov  ecx,[eax+0x1F0]
```

That resolves `docs/derivation/combat.md` §9's *"`to_hit` and `attenuate` are never read by
`FUN_00644130` … They most likely belong to the projectile/hit-resolution code (`Ammo.cpp`).
**Unresolved**"* — the guess was right. **Accuracy is a hit/miss decision made in `Ammo`,
never a damage-magnitude term**, which means my lane cannot implement "ATTENUATE accuracy":
it is the ammo lane's, and `RANGE_INACCURACY` (`Constants +0x40`, `fraction(256)` of
`"1/100"` = stored **2**) is its rules-side companion. `systems::combat` records this as
`RANGE_INACCURACY_NOTE` rather than implementing a guess.

### 4. `SPLASH_AREA` is not what sizes `Object::do_damage`'s splash

`Object::do_damage`'s splash scan bound is the **absolute** `[0x00CBE334]` at `0x0064C14B`
and `0x0064C4D7`. `0x00CBE330` is `ring_end[]`, built by `circle_init` `0x006817F0`, so that
address is `ring_end[1]` — a **fixed 3×3 block, nine tiles**, independent of the weapon.

Nine, not five, because the metric is `vector_dist`, which puts the diagonals at distance 1:
`1 + 1·1/(2·1) = 1 + 0 = 1`. The module generates the whole spiral and asserts this.

`SPLASH_AREA` (`type +0x200`) *is* consumed — by `Ammo::do_damage` at `0x006784F0`, which
sizes its own ring as `ring_end[clamp(splash_area + 1, 0, 64)]` (`0x0067851B`) and its
graphic radius as `splash_area * 192` (`0x006784F6`). So the projectile path has a
weapon-sized blast; the direct/melee applier has a fixed one.

`SPLASH_PERCENT` (`type +0x204`) has **exactly one** consumer in the entire binary:
`ObjectData::get_damage` `0x006448DF`, step 14.

Per-victim splash multiplier, both truncating signed divides of `do_damage`'s 8.8 argument:
`scale/4` for a **land** attacker whose type `is_siege()` (`0x0064C3DB`, and that arm skips
the reach test entirely), `scale/8` otherwise (`0x0064C497`). Whole-attacker vetoes: air
never splashes, and sea + siege never splashes (`0x0064C39A`). Geometric gates: distance
from the **primary target** ≤ `max(target.x_size, target.y_size) * 192`, then distance from
the **attacker** ≤ `max(attacker.max_range * 192, 0x180)`.

### 5. The damage the applier hands to `take_damage` is in sixteenths

`Object::do_damage` `0x0064A6F9…0x0064A78D` (unit branch):

```
D = max(D * scale_8_8, 0x100)                 ; imul; cmp 0x100; cmovle   <- floor BEFORE any divide
if (ammo_index >= 0) D /= type.ammo_per_att   ; 0x0064A735
q     = D / type.uber_size                    ; 0x0064A766
q     = trunc(q / 16)
frac  = q % 16          -> take_damage arg2 (char)
whole = trunc(q / 16)   -> take_damage arg1 (int)
```

`Unit::fight` passes `scale_8_8 = 0x100` for a normal hit and `ammo_index = -1` for melee.
The building branch (`0x0064A792`) has **no `uber_size` divide and no `0x100` floor**.

`Object::take_damage` then does, at `0x006522F2`:

```
if (damage < 1 && frac < 1) frac = 1;            ; every hit lands for at least 1/16 hp
u = (char)(this->damage_frac + frac);            ; wraps in a signed byte, unclamped
damage += trunc(u/16);  this->damage_frac = u % 16;  this->damage += damage;
```

**A hit always removes at least 1/16 of a hit point.** That is a different rule from
`get_damage`'s conditional floor of 1 at step 28, and unlike it, it is unconditional. It is
also the mechanism by which the coordinator's per-projectile-armor observation stops short
of total immunity: a volley that computes to zero still bleeds 1/16 per round.

`damage_frac` is a plain `char` and nothing clamps it before the add, so a large enough
`frac` genuinely wraps negative and *heals*. The module has a test recording that rather
than papering over it.

### 6. Death, and why the corpse keeps its slot

`Object::die` `0x00647080` sets `ObjectData +0x32 hold_frames` to

```
max(current, 1, max over live Ammo aimed at this object of (total_time - cur_time) + 1 + [0x00C0A888])
```

so **the dead object's slot is held until every projectile already in the air toward it has
landed**. Any port that frees the slot at death resolves in-flight ammo against whatever
reuses the index. `hold_frames` is inside `Object::walk_data`'s `[32,66)` window, so it is
checksummed and a wrong hold desyncs.

`Objects::add_death` `0x00653B60` slot policy: first `valid == 0` slot wins; otherwise the
oldest `first_frame` among corpses whose type does **not** `blocks_while_dead()`
(`UnitTypeData::blocks_while_dead`, type vtable `+0x120`, `0x00470440`); and if every slot is
occupied *and* blocking, `best` was initialised to 0 and `best_frame` to `frame + 1`, so it
silently overwrites slot 0. `DeathObj::clear_blocking` `0x008D4AC0` must run on an evicted
blocking corpse before its slot is reused.

### 7. The flank arcs, and the sub-multipliers everyone gets wrong by 6×

Combining `flank_level` (`0x0092CFE0`, Tier B) with the caller's own `delta >= 0x2AAAAAAA`
pre-guard at `0x00644B1D`, on the biased delta `(defender_facing - attack_dir) - 0x80000000`:

| biased `delta` | arc | level |
|---|---|---|
| `0xD5555556` … wrapping through 0 … `0x2AAAAAA9` | 120° | none |
| `0x2AAAAAAA … 0x5FFFFFFF` | 75° | **2** |
| `0x60000000 … 0xA0000000` | 90° | **1** |
| `0xA0000001 … 0xD5555555` | 75° | **2** |

Which arc is the defender's *front* depends on `attack_dir`'s sign convention inside
`Unit::fight`, which I did not derive. The arithmetic is measured; the semantic label is
not, so the module says so instead of guessing.

`CAVALRY_FLANK_BONUS = 40` and `VEHICLE_FLANK_BONUS = 33` are parsed with **`wtoi`, not
`fraction`**, and applied with a `/256` at `0x00644B4F`/`0x00644B42`. So cavalry get
`50 * 40 / 256 = 7`% per level and vehicles `50 * 33 / 256 = 6`% — not 40% and 33% of 50%.
Reading the XML alone (`"40% (of base flank bonus)"`) gives an answer 6× too large. Infantry
get the full `+50%` / `+100%`, which is what `rules.xml`'s "max bonus is twice this number"
means.

### 8. Entrenchment: flamethrowers ignore it

`get_damage` step 26's suppressor `attacker_tech_0x83` is `TypeIndex 0x83 = FLAMETHROWER`
(PDB `TypeIndex` enum). The direction test uses `trench_angle` (`UnitData +0x5C`), a
different field from `angle` (`+0x50`), and a different classifier (`entrench_dir_level`)
from the flank one — the two disagree at `delta = 0`, so conflating them is an invisible
error. `RULES +0xB98`, applied inside that branch, is **`ANTIPATER_ENTRENCH_BONUS`**
(`fraction(256)` of `"80/100"` = 204).

### 9. Recharge

`UnitData::recharge` `0x0060FDF0` overrides `ObjectData::recharge` (`0x00646C30`, which is
just `type[+0x1F4]`):

```
if (!type->is_siege()) return recharge;                                   ; type vtable +0x10C
if ((RULES.artillery_under_attack_fires_slowly == 0 || !(unit_masks2 & 1))
    && UnitData::in_supply(out))            return recharge;             ; 0x00609EF0
return this->is(BOMBARD) ? recharge * 2 : (recharge * 3) / 2;             ; TypeIndex 0x10B
```

`ARTILLERY_UNDER_ATTACK_FIRES_SLOWLY` is `Constants +0xC20`, shipped value **1**, XML
comment *"0 = fires normal speed, 1 = fires as if out of supply"*. `Unit::fight` stores the
result into `UnitData +0xAE recharging` with a **byte** store at `0x005FF0A4`, so a recharge
above 255 frames wraps. That is retail behaviour and the port reproduces it.

---

## Precise patch list for `crates/don-sim/src/mechanics.rs` (owned by another lane — I did not edit it)

The PDB names every remaining "name not established" and every `FUN_` in the damage port.
All of these are one-line comment/field-name changes; none changes an arithmetic result.

**`CombatRules` fields:**

| current | real name | `Constants` offset | shipped value |
|---|---|---|---|
| `rule_0x558` | `super_immune` | `+0x558` | 0 (disabled) |
| `rule_0x76c` | `russian_cossack_damage` | `+0x76C` | 25 |
| `rule_0xb98` | `antipater_entrench_bonus` | `+0xB98` | 204 (`fraction(256)` of `80/100`) |

**`UnreachedTerms`:**

| current | real name | offset | value |
|---|---|---|---|
| `rule_0xbbc` | `wellington_siege_attack` | `+0xBBC` | 1 |
| `rule_0x794` | `japanese_damage` | `+0x794` | -5 ("negative is per age") |

**`get_attack` / `get_armor`'s `rules_0x8b8`** is `dutch_attack_bonus` (`+0x8B8`, value 1;
XML: *"+1 to attack and armor for dutch merchants, caravans, and wagons for every age"*).

**`DamagePredicates` — every one of these is now named:**

```
attacker_vf_0x18 / defender_vf_0x18   -> is-unit    (Unit,Animal true; Build,Wall false)
attacker_vf_0x1c / defender_vf_0x1c   -> is-build   (Build,Wall true)
attacker_vf_0x20 / defender_vf_0x20   -> Build true, Wall false            [name not pinned]
defender_vf_0xcc                      -> UnitData::is_supply   0x0046CE80
defender_vf_0xd0                      -> UnitData::is_caravan  0x0046CE90
defender_vf_0xd8                      -> UnitData::is_moving   0x00610AF0
defender_vf_0x120                     -> UnitData::attack      (vtable +0x120)
attacker_vf_0x130                     -> UnitData::max_range   (vtable +0x130)
attacker_vf_0xe4                      -> ObjectData::get_captain (vtable +0xE4)
attacker_type_vf_0x10c / defender_..  -> UnitTypeData::is_siege  0x00470460 (type vtable +0x10C)
defender_build_flag                   -> vt[0x3C] against Build::vftable 0x00B42174
step10_bonus_applies                  -> ObjectData::has_general 0x00646B00   (Wellington)
step11_player_prop_0xf                -> LeaderData::has_tribe_bonus(0xF) 0x006E1370 (Japanese)
step27_player_prop_0xd                -> LeaderData::has_tribe_bonus(0xD)         (Cossack)
step12_team_differs                   -> LeaderData::get_target() 0x006DA000 != attacker player
attacker_tech_0x42                    -> is(MILITIA      0x042)
attacker_tech_0x139                   -> is(V2ROCKET     0x139)
attacker_tech_0x83                    -> is(FLAMETHROWER 0x083)   <- "flame ignores trenches"
defender_tech_0x216                   -> is(REDFORT      0x216)
defender_tech_0x143                   -> is(BARK         0x143)
defender_tech_0x109                   -> is(CATAPULT     0x109)
attacker_type_id 0x1BB / 0x1BC        -> FORTX / CASTLE  (the mask fixup)
defender_type_id 0x21D                -> the step-29 SUPER_IMMUNE target
```

**`DamageInput` fields:** `attacker_masks`/`defender_masks` are `ObjectTypeData +0x1E4
obj_masks`; `defender_splash_divisor` is `UnitTypeData +0x308 uber_size`;
`attacker_splash_percent` is `+0x204 splash_percent`; `defender_overkill_stamp` is
`UnitData +0x4C damage_frame`; `defender_word_0xa4` is `+0xA4 damage_o`;
`defender_facing` is `+0x50 angle`; `defender_facing_entrench` is `+0x5C trench_angle`;
`defender_flags_0x68` is `+0x68 unit_masks`; `defender_flags_0x6c_bit12` is
`+0x6C unit_masks2`; `attacker_z`/`defender_z` are `SubObjectData +0x0C z_internal`
(XOR `0x00063637`); `attacker_type_0x40` is `TypeData +0x40`.

**On the coordinator's note that `damage()` is missing the `RECAPTURE_CITY_MODIFIER` branch:**
the copy of `mechanics.rs` I read (2026-08-08, `damage_traced` step 30 at the end of the
function) *does* have it — `return (div256(r.recapture_city_modifier.wrapping_mul(d)), t)` at
the `0x00645024` site, gated on `defender_vf_0x20 && defender_build_flag &&
defender_build_0x20 && recapture_owner_matches`. Either that lane has since rewritten the
file or the note is about a different copy. Flagging rather than acting, since the file is
not mine.

---

## Honest gaps

* **Target ranking is not implemented.** `Object::compare_target` (`0x0064E5C0`, 3,553 B)
  and `Object::find_nearby_target` (`0x00648DA0`, 4,042 B) were located and their call
  graphs read, but not reduced to a scoring rule. Only the cheap gates are ported
  (`poor_target`, the range tests, `valid_target`'s shape). Without `compare_target` an
  RL agent picks targets by our policy, not the engine's — fine for training, fatal for a
  replay match. This is the single largest remaining piece of my brief.
  Companions still unread: `Unit::target_opportunity` `0x005FFFC0` (3,695 B),
  `Unit::find_melee_target` `0x005FF9C0`, `Unit::find_new_target` `0x005FF6A0`,
  `Object::check_target` `0x00649E00`.
* **Most of `Object::do_damage`'s middle third is not ported.** The first complete
  post-`take_damage` world transaction, `0x0064BA18…0x0064BBFA`, and the adjacent
  survivor-only `Armies::emergency` gate through `0x0064BC17` now live in
  `combat::damage_world`. The adapter preflights typed object/type/diplomacy facts, then applies
  `buildings_razed`, the asymmetric `/10` combat-score mutation, optional `Build::plunder`, and
  the two `u16` current-frame rate counters in address order. A lethal non-building returns before
  those reads, matching retail. The immediately following flamethrower transaction
  `0x0064BC17…0x0064BEB7` is also ported: entrenched units clear the exact three checksum-visible
  mask bits before `GraphicEvents::remove_entrench`; non-air-carrying buildings synchronously eject
  land occupants and synthesize/contain/release Citizens under the exact death-ring cap, then close
  successful allocations in a deferred second loop. The replay adapter preflights the post-eject
  spawn facts so missing state cannot leave a partially ejected simulation. `0x0064AA60…0x0064BA17`
  remains presentation-entangled, while later post-hit/stat arms and the separate capture branch
  remain unported; those need their own bounded transactions rather than an invented catch-all
  world adapter.
* **`attack_dist` `0x006488F0` is ported for resolved ordinary objects** in
  `systems::held_target`. The `0x00CAE5FC` read is the same measured divide-three table used
  by the movement lane: `T[coord >> 4] * 0x30 + 0x18` snaps to a 48-unit-cell centre. Retail
  subtracts target and attacker footprints per axis before `vector_dist`. Unit vtable slot
  `+0xC0` is PDB `UnitData::is_plane` `0x0046CE40`, exactly `domain == 2 &&
  !(unit_flags & 0x20)`; the no-footprint branch additionally requires type
  `obj_masks & 0x08000000` to be clear. Automatic target ranking now requires those exact
  facts and rejects a candidate when its footprint cannot be resolved.
* **The `CheckSum::label` question.** `check_units` and `check_deaths` both call
  `DataWalk::label(const char*)` (vtable `+4`) once per record with a `StringTable` pointer.
  Whether `CheckSum`'s implementation folds bytes into `accum` is **not established** —
  `schema/state-schema.json`'s extractor annotates it "walk_test -> 1 tag byte". My
  `check_deaths` hashes payload only, and `DEATHS_LABEL_UNRESOLVED` records it. **The replay
  harness must resolve this before an exact channel match is possible.** It is one small
  disassembly of `CheckSum::vftable`'s slot 1.
* **No experience or veterancy exists.** `ObjectData` has no experience field, `UnitData` has
  no kill counter, and nothing in `object.cpp`/`unit.cpp` reads one. The only per-unit
  progression is the player-wide `military_level` term already in `get_attack`/`get_armor`.
  Recording this as a measured *absence* so nobody implements it from folklore.
* **Nothing here was executed.** Tier C. The oracle can reach `get_damage`; it cannot reach
  `do_damage` or `take_damage` without an object table, a leader array and a world.

## Wiring

`crates/don-sim/src/systems/mod.rs` now has `pub mod combat;` (added per that file's own
instruction). `lib.rs` already declares `pub mod systems;` — no change needed there, and I
made none. Nothing else in the tree was touched.
