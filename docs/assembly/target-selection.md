# Target selection — assembly report

Lane `assembly:target-selection`. The executable module is
`crates/don-sim/src/systems/target.rs` (2,738 lines, 45 passing tests + 1 ignored benchmark).
The Cycle-5 integration added the phased automatic-acquisition contract and closed the
spellcaster priority arm.

---

## What now runs that did not before

**A unit acquires a target, walks to it, and lands a hit that goes through the real
`ObjectData::get_damage` chain.** That is
`systems::target::tests::the_whole_chain_runs_selection_into_get_damage`: it builds a world,
links two objects into the acquisition grid, drives `Engagement::step` for 120 frames, and
asserts that shots resolved with non-zero damage from `crate::mechanics::damage`. Before this
lane, `get_damage` — 7.99 M oracle trials, the best-tested thing in the project — had nothing
that could choose what to point it at.

Concretely, these execute for the first time:

* `Object::find_nearby_target` `0x00648DA0`'s spiral over `World::wdata` cells, in retail
  cell order and retail chain order, with retail's ring budget and retail's early-out.
* `Object::compare_target` `0x0064E5C0`'s priority arithmetic, transcribed branch for branch.
* `Unit::find_melee_target` `0x005FF9C0`'s respond-range selection.
* The `find_nearby_target` weighting *around* `compare_target`: the `targeted` spreading
  penalty, the min/max-range reshaping, the last-order-target halving, the facing weights.
* `attack_dir` as `Unit::fight` `0x005FD4D0` actually computes it, composed into a flank tier
  that applies the caller's pre-guard.
* A bridge from `combat::CombatConstants` to `mechanics::CombatRules`, without which the
  damage chain could not be called from anything holding the shipped rules.

**Current focused gate:** `cargo test -p don-sim systems::target::tests --lib` is 45 passed,
0 failed, 1 ignored; `cargo test -p don-sim --lib --no-run` and
`cargo check -p don-ai --lib` are green, and `git diff --check` is clean.

---

## The finding that mattered most: acquisition is a bucketed grid, not a scan

The brief flagged that the web lane measured target acquisition — not the damage chain — as
the quadratic cost, 30× throughput loss from 128 to 4,096 units. Retail does not have that
problem, and the reason is structural.

`Object::find_nearby_target` walks **`World::wdata`**, a `WData[xs*ys]` array at `World+0x134`
(28 bytes per record, `schema/pdb-types.json`). Each cell holds an intrusive object-list head
at `WData +0x08 down` / `+0x0A down_who`, and each object carries the next link at
`ObjectData +0x2C down` / `+0x2E down_who`. The cell index is
`div_3_table[(coord ^ 0x63637) >> 8]`, and `div_3_table` `0x00CAE5FC` is literally a
divide-by-three lookup built by `init_coord_lookup_array` `0x00681DB0` (`t[k] = k/3`, both
directions from zero, C truncation) — so **a cell is `(c >> 8) / 3` = 768 world units = four
tiles**. The same table at `>> 6` gives tiles and at `>> 4` gives quarter-tiles; that is why
one global serves three different grids.

The search then walks cells in the `circle_init` `0x006817F0` spiral (already ported as
`combat::circle_table`) out to `ring_end[rings]`, with

```
rings = (max_dist + 0x2FF) / 0x300                     ; ceil(max_dist / 768)
      + (attacker is a building) + (has_objmask 0x80000000) + (stance == 3)
      capped at 0x20, or exactly 0x20 when max_dist == 0
```

and — the part that actually bounds the cost — **it stops as soon as more than ten *unit*
candidates have been scored and it holds any best target** (`0x0064986C`). That is retail's
own answer to dense-world acquisition, and it is not an optimisation a port may skip: it
changes *which* target is chosen, not just how long the choice takes.

### Measured

`cargo test -p don-sim --release --lib systems::target::tests::acquisition_scaling --
--ignored --nocapture`, this Mac (arm64, release), 200 acquisitions per point, enemies packed
on a 96-unit lattice around the searcher, both paths ranking with the same `rank_candidate`:

| units | grid acquisition | naive all-objects scan | speedup |
|---:|---:|---:|---:|
| 128 | 366 ns | 349 ns | 1.0× |
| 512 | 580 ns | 1.3 µs | 2.3× |
| 1,024 | 570 ns | 2.9 µs | 5.0× |
| 2,048 | 577 ns | 5.2 µs | 9.0× |
| 4,096 | 552 ns | 10.4 µs | 18.9× |

The grid cost is **flat** from 512 units up (552–580 ns) because it is bounded by cells and
by the ten-unit limit, not by population; the naive scan is linear. At 128 units the grid is
marginally *slower* — the spiral's fixed cell walk is not free — which is worth saying rather
than rounding away.

This is a like-for-like comparison of two acquisition structures inside this crate; it is not
a claim about retail's own throughput, which we have not measured.

---

## `attack_dir`, settled

The web lane measured that with `defender_facing = 0`, `attack_dir = 0` scores flank tier 1
and a half turn scores tier 0, and flagged it as "not what the name suggests". The name is
fine; the convention is the thing that was missing.

**`attack_dir` is the direction the attack *travels*, attacker → target.** Traced in capstone
at `0x005FE872..0x005FE89B` inside `Unit::fight` [measured]:

```
005fe875  mov edx,[esi+0x14]; xor edx,0x63637    ; target.y
005fe875  mov eax,[ebx+0x14]; xor eax,0x63637    ; attacker.y
005fe889  sub edx,eax                            ; dy = target.y - attacker.y
005fe880  mov ecx,[esi+0x10]; xor ecx,0x63637    ; target.x
005fe891  mov eax,[ebx+0x10]; xor eax,0x63637    ; attacker.x
005fe899  sub ecx,eax                            ; dx = target.x - attacker.x
005fe89b  call find_angle                        ; __fastcall(ecx=dx, edx=dy)
005fe8a4  mov [ebp+0x14],eax
005fec4a  mov esi,[ebp+0x14]                     ; reloaded ...
005fedd5  push esi                               ; ... as do_damage's 3rd argument
```

So `attack_dir == defender_facing` means the attack is travelling *the way the defender is
looking* — the attacker is **behind** it. Substituting `bearing = attack_dir - half turn`
into `combat::flank_delta` gives `delta = defender_facing - bearing_to_attacker`, and the
arcs read plainly:

| attacker's bearing from the defender's nose | width | tier | infantry bonus |
|---|---|---|---|
| within ±60° of dead ahead | 120° | none (pre-guard) | +0 % |
| 60°–135° off, either side | 2 × 75° | **2** | +100 % |
| within ±45° of dead astern | 90° | **1** | +50 % |

**The sides are worth twice the rear, and the front is worth nothing.** That is what "flank"
means, and it is exactly what the `rules.xml` comment "max bonus is twice this number"
describes. The alternative reading — `attack_dir` as the bearing *to* the attacker — would
make a frontal charge earn the flanking bonus, which is the reading the raw arc table
invites.

This closes the open question in `combat::flank_delta`'s doc comment ("Which of those arcs is
the defender's front is **not established**"). `target::flank_tier` is the composed helper,
and it applies the caller's `delta >= 0x2AAAAAAA` pre-guard at `0x00644B1D` that
`mechanics::flank_level` alone does not — call `flank_level` directly and a head-on hit scores
tier 2.

One degenerate case worth recording: `find_angle(0, 0)` returns `HALF_TURN`, so a
co-located attacker and defender read as a head-on hit. A melee port that closes to distance
exactly 0 therefore never earns a flank bonus.

---

## `Object::compare_target`, reduced

`compare_target(o, who, check_path, mode)` returns a **priority, higher is better**;
`find_nearby_target` takes the maximum. Floor 15, with explicit demotions to 1 and 2 for
"never shoot this". Reading it at all required resolving nineteen vtable slots, which is the
reusable part:

| slot | Unit | Build | Wall | meaning |
|---|---|---|---|---|
| `+0x08` | `flags8 & 1` | 0 | 0 | `is_live_unit` |
| `+0x0C` | 0 | `flags8 & 1` | `flags8 & 1` | `is_live_build` |
| `+0x18` | 1 | 0 | 0 | **`is_unit`** |
| `+0x1C` | 0 | 1 | 1 | `is_build` (walls included) |
| `+0x20` | 0 | 1 | 0 | **`is_building`** (walls excluded) |
| `+0x2C` | 0 | `BuildData::is_wonder` | 0 | `is_wonder` |
| `+0x4C` | `flags8 & 1` | `flags8 & 1` | `flags8 & 1` | `is_alive` |
| `+0xAC`, `+0xB0` | 0 | `return this` | — | building self-cast |

(`0x0041BFF0` is `xor eax,eax; ret`; `0x0041E0E0` is `mov eax,1; ret`; `0x0041C000` is
`mov eax,ecx; ret`; `0x0046CDA0` is `return this->flags8 & 1`. All ICF-folded, which is why
the PDB names on those slots are nonsense like `std::codecvt::do_encoding`.)

The shape, in retail order:

1. `base = objecttypes[t.type].vf[0xD0]() << 2`; a wonder divides by **25**.
2. If the target is a **live building**: two sea-attacker vetoes return 0, `mode` multiplies
   the base by 10, and then a class multiplier — city centre **×5** (or a flat `100` in
   `mode`), defensive wall **×4** (`×40` in `mode`), military trainer **×3** (**×15** when
   active), training building **×2**. Suppressed entirely when the attacker is in combat
   stance 3 or the owner carries `leaders[who] & 4`.
3. Threat/fragility: `v = attack × v × 100 / hits_left` — so a wounded, dangerous target
   outranks a healthy harmless one. A building target against a stance-3 attacker takes
   `v /= 20` instead.
4. Tower arm (attacker is a building): its own current target is `×2` or `/2` depending on
   `BuildData::get_garrison_arrows() < 2`; an already-damaged target `×3/2`; a moving target
   `/4`; a supply unit **×5000**.
5. `v = damage × v`, or in `mode` `v = v / damage` with a zero-damage veto.
6. Separate building and unit tails with the large additive terms (`+100000`, `+900000`,
   `+1000000`, `+9000000`), then `v /= (UnitData::full + 1)` on the unit tail.
7. Clamps: an alive-but-zero-hits shell pins at 99999; a negative (overflowed) score
   saturates to 9999999; `check_path` and out-of-range divides by 5; `(v + 99) / 100`; a knee
   at 100000 where growth drops to one fifth (`(v - 99996)/5 + 100000`, continuous at the
   boundary); floor 15; then an undetected stealth unit is **1** and an air target without
   `has_objmask(0x80000000)` is **2**.

The class multipliers were the payoff of naming `WallData::is_defensive` `0x00473650`,
`BuildTypeData::is_military_trainer` `0x0063BCF0` and `BuildTypeData::is_training_building`
`0x00639DE0`, all of which the decompile shows only as `FUN_`.

Every branch carries its VA in the source. Inputs are pre-resolved into
`CompareTargetInput`, the same pattern `combat::PoorTargetInput` already uses, because retail
reaches all of it through virtual dispatch and four global tables this crate does not model.

### Spellcaster action arm

The former `COMPARE_TARGET_UNREACHED` gap is closed. Capstone at
`0x0064EC9D..0x0064ED27` establishes that both `UnitData::get_action` calls operate on the
candidate target loaded from `objects[param_2][param_1]`, while EDI continues to hold the
attacker. A spellcaster target whose current activity is CastSpell (`OrderIndex 0xE`) gets
`v *= 20`; its activity's virtual `+0xF4` payload is then compared with the attacker's
`ObjectData +0x0A/+0x09` identity and adds `10,000,000` on a match. A spellcaster not
currently casting instead adds `6,000,000` when `target->is(0x3A)` succeeds.

`CompareTargetInput` carries those virtual/order reads as explicit pre-resolved fields. This
keeps the arithmetic exact without coupling the scorer to one host's order container.

---

## The named pipeline

Chasing this lane resolved a chain the repo had only as `FUN_` addresses:

```
Unit::find_new_target         0x005FF6A0
 └─ Unit::find_melee_target   0x005FF9C0   picks the search radius from stance + max_range
     └─ Object::find_nearby_target 0x00648DA0
         ├─ Object::valid_target    0x00648BA0   domain / diplomacy gate
         ├─ Object::check_target    0x00649E00   distance + region + territory gate
         │   ├─ ObjectData::attack_dist 0x006488F0 (4-arg) / 0x0064C880 (2-arg)
         │   ├─ ObjectData::is_in_range 0x006486B0 (43-byte wrapper at 0x00648D70)
         │   ├─ WorldData::get_tregion 0x006B52E0
         │   └─ Object::poor_target     0x0064A270  (already in combat.rs)
         └─ Object::compare_target  0x0064E5C0
             └─ ObjectData::get_damage 0x00644130
Unit::fight                   0x005FD4D0
 ├─ find_angle                0x0092D130 -> attack_dir
 └─ Object::do_damage         0x0064A480
```

Also named on the way: `Object::get_army` `0x00649D70`, `Unit::on_duty` `0x005FFF70`,
`UnitData::get_action` `0x00608450`, `UnitData::action_type` `0x0060A850`,
`UnitData::get_activity` `0x00608370`, `UnitData::is_detected` `0x0060A630`,
`ObjectData::num_inside` `0x00646D50`, `ObjectData::is_worker` `0x0046FA10`.

---

## Two engine facts worth carrying forward

**Target selection writes checksummed state.** `ObjectData +0x34 near_o`, `+0x36 near_who`
and `+0x3D targeted` all sit inside `Object::walk_data`'s `[32, 66)` window, which
`combat.rs` already records as the `units` channel payload. So a divergent target choice
desyncs the `units` channel *directly*, not merely through its damage consequences. Any
replay validation of combat needs the selection to be right, not just plausible.

**`targeted` is a spreading term, and it is quantitative.** `find_nearby_target` adds
`(targeted + 8) × 0x30` to the candidate's distance before dividing the priority by
`penalty / 192 + 1`. Each attacker already aimed at an object therefore adds 48 world units —
a quarter tile — of virtual distance. That is how a mob spreads along a line instead of
stacking on one victim, and it is a checksummed counter that saturates at 100.

---

## Corrections to existing artifacts (two sentences each, per the standing rule)

* **`crates/don-sim/src/mechanics.rs`, `DamagePredicates::attacker_vf_0x18`** is documented
  as *"alive-shaped"*. Slot `+0x18` is `is_unit` — `Unit` returns 1, `Build` and `Wall`
  return 0 — and the alive test is slot `+0x4C`; `combat.rs`'s
  `PoorTargetInput::both_are_units` already reads it correctly. The field is an input either
  way, so no behaviour changes; only the doc comment is wrong.

* **`crates/don-sim/src/mechanics.rs`, the three "name not established" rules.** `rule_0x558`
  is `SUPER_IMMUNE` (shipped 0), `rule_0x76c` is `RUSSIAN_COSSACK_DAMAGE` (25), `rule_0xb98`
  is `ANTIPATER_ENTRENCH_BONUS` (204), and in `UnreachedTerms`, `rule_0xbbc` is
  `WELLINGTON_SIEGE_ATTACK` (1) and `rule_0x794` is `JAPANESE_DAMAGE` (-5). The names were
  sitting in `combat::CombatConstants` at the same `Constants` offsets the whole time;
  `target::combat_rules` now maps them and a test asserts the five values.

* **`combat.rs` header, "Target ranking … were mapped but not reduced".** Now reduced, in
  `systems::target`; the header was updated to point there (the only edit made to that file).

---

## Honest gaps

* **Tier C throughout. Nothing here has been executed against retail.** The oracle cannot
  reach `compare_target` (needs the object table, the leader array, the world and nineteen
  virtuals) or `find_nearby_target` (needs a populated `World::wdata`). Do not promote any of
  it by proximity to `crate::mechanics::damage`.
* **Cell insertion order is derived.** `Object::add_to_world` `0x0064D8C0` installs the
  previous `WData +0x08/+0x0A` pair into the entering object's
  `ObjectData +0x2C/+0x2E`, then writes the entering object as the new cell head. The
  executable `TargetWorld::{place_at,relocate,remove}` mirror preserves that exact chain
  order; equal-score ties therefore follow retail head insertion rather than an entity scan.
* **`Object::valid_target` `0x00648BA0` and `Object::check_target` `0x00649E00` are not
  ported**, only their position in the pipeline. They are the `admit` closure's
  responsibility, because they need diplomacy, fog and the region map. `check_target`'s
  distance computation *is* ported (`attack_dist`); its four gates are not.
* **`ObjectData::attack_dist` `0x006488F0` is now ported for resolved ordinary objects** in
  `systems::held_target`. Capstone shows that retail snaps both anchors to 48-unit cell
  centres, subtracts target and attacker footprints from the x and y legs independently,
  then calls `vector_dist`; a rectangular building is therefore not a scalar
  `max(x_size,y_size)` subtraction. The early vtable-`+0xC0`/objmask-`0x08000000` bypass is
  explicit, but a host must still resolve that raw virtual/type gate from the real object.
* **The range/pursuit seam is only part of `Unit::fight`.** `systems::held_target` now carries
  `ObjectData::is_in_range`'s preconditions, fixed `0x66/0xF6` reach, ranged `-6/+6` edges,
  unit `big_radius` minimum rescue, and `Unit::fight`'s exact retire-after-lost-contact gate.
  `systems::fight` carries direct land-volley Guy/facing geometry. Projectile spawning,
  splash/retaliation and the remaining duty/retarget/order arms are still outside those two
  APIs and fail closed where the common slice cannot choose a retail branch.
* **`Unit::find_attack_pos` `0x00601280` (7,124 B) is now bounded, not implemented.** Its
  ordinary unit arm calls `UnitType::find_nearby_spot`; its building arm scans a perimeter,
  checks terrain and ordered collision, and draws from `game_random` while scoring candidates.
  `HeldTargetStep::FindAttackPosition` emits the exact ordinary six-argument wrapper request,
  never a guessed target or reflected back-off destination. A faithful spatial provider is
  still required before Arena may turn that request into a move order.
* **The `mode` (`param_4`) flag's meaning is transcribed, not understood.** It is
  `find_nearby_target`'s `local_40`, set when the owner is not AI-flagged, `0x006EC000`
  returns 0, and `Game +0x821 & 2` is clear. It flips `compare_target` from multiplying by
  damage to dividing by it, which reads like "effort to kill" rather than "value of killing",
  but no name is established.

---

## Files

* **New:** `/Users/ember/dev/don/crates/don-sim/src/systems/target.rs`
* **Shared-file edits, both minimal:**
  * `/Users/ember/dev/don/crates/don-sim/src/systems/mod.rs` — one `pub mod target;` plus its
    doc comment, exactly as that file's header instructs.
  * `/Users/ember/dev/don/crates/don-sim/src/systems/combat.rs` — **doc comment only**, five
    lines in the "deliberately not here" section, repointing target ranking at the new module.
    No code change; the file still builds standalone with no `crate::` imports.
* **This report:** `/Users/ember/dev/don/docs/assembly/target-selection.md`
