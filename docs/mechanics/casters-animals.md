# Casters, stealth, and animals recovery boundary

Status: **Tier C; coherent isolated primitives, not a caster/animal executor.** The recovered
implementation is `crates/don-sim/src/systems/casters_animals.rs`. It has not been declared in
`systems/mod.rs`, connected to `World::step`, populated by the replay bridge, or compared with a
retail differential oracle.

What now works is deliberately narrower than the original lane brief:

- the shipped TypeIndex 629..=644 ability block is frozen with its XML mana/range/timing,
  research-cost and cast-cost columns kept distinct;
- mana capacity/remaining-mana arithmetic, cloak state, and the real allied detection-mask
  predicate are executable;
- `Caster::process_spells` logical expiry, reverse removal order, Ambush/Forced March flag
  cleanup, and Jam Radar pulse cadence are executable;
- the golden human Scout's frame-zero `Unit::think_spellcaster` path is a separate detached
  transaction: it may request a Unit CastOrder but never populates this active-spell array;
- all 12 Gaia TypeIndex values are frozen;
- the 64-frame herd scheduler, two-draw herd migration, spawn counts, and conditional angle
  draw are executable;
- an eligible farm produces the retail five-animal plan with the exact 15-draw
  selector/Y/X order and attachment fields.

The standalone gate is **23 passed, 0 failed**: 17 tests in the recovered module plus the six
tests from the real `don-sim` RNG source included by path. These are transcription tests, not
retail measurements, so every behavioural claim remains Tier C.

## Recovery provenance

The interrupted mechanics lane had completed its PDB/type/XML/decompilation pass but had not
written source or a report. Recovery used its local scratch artifacts and re-read the named
functions it had selected. It did not expand into unrelated caster or animal behaviour. Raw
agent/session identifiers and local transcript paths are intentionally not repository data.

Ground truth is the shipped `riseofnations.exe` (sha256 `30478a44…625079`), matching
`ron-bin/sbl/rise.pdb`, `schema/pdb-types.json`, the PDB `TypeIndex` enumeration, and the
shipped XML in `ron-data/`. Community formulas were not used.

| recovered fact | evidence |
|---|---|
| `CasterData` owns `Array<ActiveSpell>` at `+0x04`; an element is `{TypeIndex, start, frame}` and is 12 bytes | PDB TPI / `schema/pdb-types.json` |
| expiry, Jam Radar cadence, reverse scan, Ambush/March cleanup | `Caster::process_spells` `0x00739AD0`; `Array<ActiveSpell>::remove` `0x0048A960` |
| mana capacity and remaining counter | `UnitData::mana` `0x00609A50`; `UnitData::mana_left` `0x00609A30`; `SpellType::pay_cast_costs` `0x00676C40` |
| cloak and detection predicates | `UnitData::is_cloaked` `0x0060A6A0`; `UnitData::is_detected` `0x0060A630`; `LeaderData::ally_mask +0x6929` |
| ability values and disabled prerequisites | first 16 `ron-data/craftrules.xml` `<CRAFT>` rows; PDB TypeIndex 629..=644 |
| Gaia IDs 402..=413 | PDB `TypeIndex`: `BASE_GAIATYPES=402`, `NUM_GAIATYPES=12` |
| herd data and scheduler | PDB `HerdData`; `Herd::process` `0x00741760`; `Herds::process` `0x00741CC0` |
| herd creation counts and angle RNG | `Herd::create_units` `0x007417F0` |
| five farm animals and exact draw order | `Farms::add_animals(int)` `0x008D8F30`; all-farms wrapper `0x008D92D0` |
| farm-animal 128-frame movement gate | `Animal::think_farm_animal` `0x005D7700` |

## Ability data: what “cost” means

The shipped XML is unusually explicit:

- `COST` is the **research** cost, and its own comment says no current spells use it;
- `COST2` is the resource cost to **cast**;
- `MANA` is the “craft” consumed by an ability;
- `JOB_TIME` is in 1/15-second units;
- `SPELL_RANGE` is in tile coordinates.

This matters because Ambush, Forced March, Entrench, Create Decoys, and Informer contain the
raw `20g/20w` string in `COST`, while their `COST2` fields are empty. The module preserves that
as `research_cost` and does **not** claim those abilities pay `20g/20w` on cast. Restock
Supplies is the inverse: it has an empty research cost and raw cast cost
`2w/2m/2c/2o/2f`.

Six of the sixteen rows have `<PREQ0>Disable</PREQ0>` (case varies): Pilfer, Assimilate,
Rally, Restock Supplies, Blow Tread, and Paradrop. They remain in TypeIndex and the rules
table, but `AbilityFact::enabled` is false. A data row is not evidence that
`SpellTypeData::is_castable` accepts it.

### Mana

`UnitData::mana` starts from `UnitTypeData::mana +0x2EC`:

- zero returns immediately;
- an air unit with Space Program receives `(100 + SPACE_AIR_RANGE)%` of base mana;
- a ground supply unit multiplies base mana by `supply_upgrade + 1`;
- the special-craft nation bonus then applies by percentage, but only to a General.

The shipped percentage constants are both zero, but the arithmetic is retained because they
are rules data. `mana_left` subtracts the signed short at `UnitData+0x96` and clamps at zero.
The module also freezes the wrapping short addition performed when a unit pays an ability's
mana.

It does **not** implement the full recharge/consumption section of `Unit::process`
`0x00610BC0`. That section branches on air landing state, supply/hero traits, tribe bonuses,
prerequisites, object flags, and the alternating frame. Reducing it to the
`AIR_UNIT_MANA_RECHARGE=2` constant would be plausible but incomplete.

## Stealth and detection

`UnitData::is_cloaked` is a four-way OR:

1. `ObjectData+0x68 & 0x800`;
2. `UnitTypeData::unit_flags & 0x4000`;
3. `ObjectData+0x6C & 0x8000`;
4. `UnitTypeData::unit_flags & 0x40000` **and no current order**.

The last branch is idle-only cloak, not another unconditional flag.

`UnitData::is_detected(viewer)` first accepts the owner. For anyone else it intersects the
fog cell's `seen3`/detected byte with that viewer's `LeaderData::ally_mask`. Detection is
therefore shared across allied bits. There is no `see_all` shortcut: revealing the map does
not by itself reveal cloaked units. This matches the independent fog derivation in
`docs/mechanics/borders-fog.md`.

The shipped `OBJ_MASK` alphabet assigns detector capability to uppercase `Z`, encoded as
`1 << ('Z' - 'A') = 0x02000000` in `ObjectTypeData::obj_masks +0x1E4`; the module freezes
that mask and predicate. Stamping its LOS into `seen3` remains a fog/world integration step.

## Active-spell lifetime

The frame-zero golden Scout producer is documented in
[`replay-frame0-scout-spellcaster.md`](../assembly/replay-frame0-scout-spellcaster.md). Its
successful Counterintel arm calls `Unit::add_cast_order`; it does not append an `ActiveSpell`.
Consequently an empty setup `CasterData+0x04` array remains empty when this frame-one processor
begins, independent of whether the Scout order queue changed.

`Caster::process_spells` scans from the last element to the first. A spell remains active
while `end_frame >= current_frame`; expiration starts one frame later. The force path removes
everything regardless of end frame.

Retail removes by passing the whole 12-byte `ActiveSpell` value to
`Array<ActiveSpell>::remove`, which searches from the beginning for the first equal triplet.
The recovered function preserves this detail rather than calling `remove(current_index)`.
Duplicate equal triplets therefore retain retail's otherwise surprising behavior.

Two expired types mutate sim state:

- Ambush (635) clears object bits `0x2800`;
- Forced March (636) clears object bit `0x8000`.

If either occurred, retail calls `Leader::verify_spell_flags` once after the scan and marks a
global visibility/cache word dirty. The Rust function reports those required effects to its
caller. Active Jam Radar (643) emits a presentation pulse when
`(start_frame - current_frame + 1) & 31 == 0`; the module reports the pulse count but creates
no graphics or sounds.

The function accepts a `Vec<ActiveSpell>` only as a logical transition surface. It explicitly
does not claim that a Rust vector represents the walked retail container. The real
`Array<ActiveSpell>` capacity, increment, flags, and current-index metadata must be owned by
the shared engine container when this module is integrated.

## Gaia and animals

The 12 Gaia TypeIndex entries are:

| id | type |
|---:|---|
| 402 | Bird |
| 403 | Flock Bird |
| 404 | Gull Bird |
| 405 | Farm Pig |
| 406 | Farm Chicken |
| 407 | Herd Horse |
| 408 | Herd Sheep |
| 409 | Herd Bison |
| 410 | Herd Bear |
| 411 | Herd Fish |
| 412 | Herd Whale |
| 413 | Herd Peacock |

This is a class/type inventory, **not twelve huntable food animals**. Birds and farm
decorations are in the same block, and the interrupted lane did not finish the hunting-to-food
resource path.

### Herd scheduling and migration

`Herds::process` runs only when `frame & 63 == 0`. It selects:

```text
index = (frame / 64) % max(number_of_herds, 5)
```

and does nothing if that index falls in the five-slot floor rather than an actual array slot,
or if the selected slot's `herd_flags & 1` active bit is clear.

`Herd::process` always consumes two main-stream draws first:

```text
candidate_x = cx - 1 + Random::get(0, 65535) % 3
candidate_y = cy - 1 + Random::get(0, 65535) % 3
```

It updates `wx/wy` only when the candidate is in bounds, the W-cell flags have no bit in
`0x70`, and the cell owner is below 8. Even an out-of-bounds first coordinate has already
consumed both draws.

`Herd::create_units` creates its units under special object-owner slot **8**: one whale, three
fish, or four units for every other type. Fish
and whales consume no angle RNG: they pass raw angle inputs `0` and `0x80000000`. Other types
consume one draw, reduce it modulo 360, then call the shared
`degrees_to_angle(degrees, 1)`. The recovered API stops at a `Degrees` seed rather than
inventing that shared conversion.

### Farm spawn

The two overloads gate a farm record on presence, a state byte equal to one, and the farm
building's complete predicate. An eligible farm creates exactly five Gaia-owned animals. For
each animal the main stream is consumed in this order:

1. `Random::get(0, 65535)`: even selects Farm Chicken 406, odd selects Farm Pig 405;
2. another draw supplies `y += draw % 384 - 192`;
3. another draw supplies `x += draw % 384 - 192`.

That is 15 draws per eligible farm. The unit allocation uses special object-owner slot **9**;
the spawned `AnimalData` then receives the farm object index at `+0x150`, farm owner at
`+0x152`, and batch index 0..4 at `+0x154`. The independent movement
routine later passes its first gate only when
`(frame + farm_object * (animal_id + 1)) & 127 == 0`; no movement RNG is consumed before that
gate.

## Tests

The module is intentionally standalone because this recovery lane did not own
`systems/mod.rs`:

```sh
rustc --edition 2021 --test \
  crates/don-sim/src/systems/casters_animals.rs \
  -o /tmp/don-casters-animals-tests
/tmp/don-casters-animals-tests
```

Current result: **23 passed, 0 failed**. The gate covers dense ability IDs, PDB POD sizes,
disabled rows,
research/cast cost separation, mana branch order, allied detection, spell end-frame
inclusivity, duplicate removal, forced cleanup, Jam Radar cadence, the five-slot herd
scheduler, two-draw rejection, cell acceptance, herd counts/angle draws, 15-draw farm plans,
and exact advancement through the crate's real `Random` implementation.

## Explicit gaps before integration

1. Port and test the complete 2,033-byte `SpellTypeData::is_castable` gate and the relevant
   arms of `SpellType::cast`; the static table is not a substitute.
2. Integrate active spells with the real engine-shaped array so capacity/growth metadata and
   `cur_index` are walked exactly; then wire `Leader::verify_spell_flags` and the dirty word.
3. Port the full `Unit::process` mana recharge/consumption branches and verify their position
   in the object scheduler.
4. Connect `is_detected` to the fog cell selected from the XOR-decoded object coordinates and
   the real leader ally mask.
5. Integrate herd unit allocation, nearby-spot search, angle conversion, placement, herd
   backlink, stable object slots, and failure behavior. The current functions freeze the
   scheduler and RNG seams only.
6. Port `Animal::do_idle`, `think_farm_animal`, and `think_bird` around the real order list,
   terrain/collision queries, movement tables, and their conditional RNG consumers.
7. Derive hunting, death, carcass/food accounting, and gather transfer from named retail
   functions. Nothing in this slice models an animal as a food source.
8. Populate the animal/herd/caster state from save/replay setup, connect the relevant checksum
   channels, and add retail differential cases before promoting any claim above Tier C.

Until those gates land, declaring this module means “compile and exercise the recovered
primitives,” not “casters and animals are simulated.”
