# `GameDaemon::calc_danger` — the AI threat map

Tick step 12's shell (`GameDaemon::process_all` `0x00732700`) has executed since the
`game_daemon_step12` lane. Six of its seven children had bodies. The seventh —
`calc_danger` `0x00732D10` — was charged as `Gap::GameDaemonCalcDanger`. This document is
that body and its fail-closed canonical tick adapter.

Ground truth: capstone disassembly of `0x00732D10` (1,476 B) and `0x00732390` (195 B) in
`ron-bin/riseofnations.exe` sha256 `30478a44…625079`, with layouts and vtable slots from
`ron-bin/sbl/rise.pdb` via `schema/types.json` and `schema/symbols.json`.
**Tier C** — read off the instruction stream, executed against nothing but this repo's own
tests. No oracle case covers it and none of it may be called verified.

Rust: `crates/don-sim/src/systems/game_daemon_calc_danger.rs`.

---

## 1. What the danger map is

Eight `int` planes at `World +0x13C`, `reg_size` entries each, indexed
`ry * reg_xs + rx` in `RCoord` — 1,536 Coord = 8 tiles, the coarsest grid the engine
carries. `map_terrain.rs` already models them (`World::danger`, `World::clear_danger`
`0x006B22E0`); nothing wrote them.

They are **signed**, and the sign is the whole design:

| relationship | effect on `danger[to]` |
|---|---|
| the object's own owner (`to == from`) | **subtract** the full amount |
| `leaders[from].diplos[to] == 2` | **subtract** `amount / 2` |
| `leaders[from].diplos[to] == 1` | add `amount / 2` |
| otherwise | add `amount` |
| and, orthogonally, `!obj->is_seen(to, 0)` | halve again before adding |

So a player's own army digs a well in that player's own plane, allies dig shallower wells,
and an enemy the target *cannot see* still deposits, at half strength. A "reasonable"
threat map has none of these properties. Do not smooth them.

## 2. The three passes

### Pass 1 — wipe, `0x00732D30`..`0x00732D5D`

```
for who in 0..8:
    if leaders[who].flags & 2 and world.danger[who] != NULL:
        memset(world.danger[who], 0, world.reg_size * 4)
```

Two gates, not one. `flags & 2` is `PROCESS`, and a **null plane is skipped rather than
treated as zeros** (`test ecx,ecx; je` at `0x00732D38`) — an eliminated player keeps a
stale plane rather than getting a clean one.

### Pass 2 — the unit band, `0x00732D90`..`0x00732F01`

```
for who in 0..8:
    if leaders[who].flags & 1 == 0: continue          ; IN_GAME, not PROCESS
    for o in 0 .. objects.unit_mark[who]:             ; band base 0
        if !objects.lists[who][o]->is_valid_unit(): continue     ; vtable +0x08
        if !objects.lists[who][o]->is_on_map():     continue     ; vtable +0xBC
        if (units.lists[who][o]->ptype->role & 0x10000) == 0: continue
        cell = RCoord(y) * reg_xs + RCoord(x)
        for to in 0..8:
            if leaders[to].flags & 2 == 0: continue
            if to == leaders[who].slot: continue
            if leaders[who].diplos[to] != 0 and leaders[to].diplos[leaders[who].slot] != 0:
                continue
            do_danger(o, who, to, cell, (attack() * 5) / 10)     ; vtable +0x120
```

`UnitTypeData::role` is `ptype +0x2C8`; bit 16 is the gate. A unit whose role lacks it —
villagers, most support — contributes nothing at all.

**The self-skip is against `leaders[who].slot` (`LeaderData +0x08`), not the loop index**
(`0x00732E50`), and the reciprocal diplomacy lookup indexes the *other* leader's `diplos`
by that same `slot` (`0x00732E5A`). `do_danger`'s own identity test, by contrast, compares
the loop index. When record and slot disagree the two disagree with each other, and the
"self" target reaches the subtracting arm. This is exactly the trap `leaders.rs` already
flagged for the step-8 diplomacy scan; it recurs here.

The diplomacy gate is an **or**: either direction being `0` admits the pair.

### Pass 3 — the building band, `0x00732F20`..`0x00733299`

```
for who in 0..8:
    if leaders[who].flags & 1 == 0: continue
    for o in obj_base[1] .. obj_mark[1][who]:         ; 2000 .. build_mark[who]
        if !b->is_valid_wall(): continue              ; vtable +0x0C
        if !b->is_active():     continue              ; vtable +0x4C
        if b->get_build()->city < 0 and (ptype->build_flags & 0x10) == 0: continue
        cell = RCoord(y) * reg_xs + RCoord(x)
        amount = <ladder>
        for to in 0..8:
            if leaders[to].flags & 2 == 0: continue   ; no self-skip, no diplomacy gate
            for k in 0..8:
                if the neighbour is in range:
                    do_danger(o, who, to, neighbour, amount / 2)
            do_danger(o, who, to, cell, amount)
```

The band is `obj_mark[1]` = `build_mark`, reached through `*(Objects +0x1EC)`, and the
start is `obj_base[1]` = 2000 (`GameAccess::obj_base` `0x00C06198`, `+4`). `walls.rs`
already establishes `obj_base = {0, 2000, 3000}`. The gate reads `is_valid_wall` because
`BuildData` derives from `Wall` — a building *is* a wall in this hierarchy.

`BuildData::city` is a `short` at `+0x72` and the test is signed (`cmp word` / `jge`).

**Pass 3 has no self-skip and no diplomacy gate.** Every `flags & 2` leader, including the
owner, is visited, and `do_danger` sorts out the sign.

The eight neighbours come from the tables at `0x00ADCAF4` (x) and `0x00ADC404` (y), read
out of the image: `dx = [-1, 0, 1, 1, 1, 0, -1, -1]`, `dy = [-1, -1, -1, 0, 1, 1, 1, 0]` —
north-west, clockwise. Only the neighbours are bounds-checked (`0x007331E0`..`0x007331F0`);
see §5.

### The strength ladder, `0x00732FF4`..`0x00733198`

Evaluated once per building, before the target loop, short-circuiting in this order:

| # | test | site | result |
|--:|---|---|---|
| 1 | `get_wall()->ptype->is_fort()` | `0x00732FFF` | `hits_left() / 2` |
| 2 | `get_build()->is(TOWER, 0)` | `0x0073303A` | `hits_left() / 2` |
| 3 | `get_build()->flags & 0x20` | `0x00733058` | `100` |
| 4 | `get_build()->is(AIRBASE, 0)` | `0x0073308F` | `100` |
| 5 | `get_build()->is(DOCK, 0)` | `0x007330C7` | `100` |
| 6 | otherwise | `0x0073311D` | `50` if `buildtypes[ptype->basic_type()]->build_flags & 0x40000000` else `10` |

`TOWER` `0x1B7`, `AIRBASE` `0x1BF`, `DOCK` `0x1B0` — resolved from the PDB's own
`TypeIndex` enum (869 enumerators), not guessed from the immediates.

## 3. Three slot readings the decompiler gets wrong

The PDB names things; the receiver decides which vtable you are reading.

* **`+0xFC` on `wall->ptype` is `ObjectTypeData::is_fort`,** not `ObjectData::get_caster_stance`.
  The receiver is a *type* record, so the slot belongs to the type hierarchy.
* **`+0x60` on `ptype` is `TypeData::is(int, int)`.** It appears in `.text` as a
  devirtualised `Build::is` (`0x00476E50` is literally `this->ptype->vf(0x60)` tail-jumped),
  guarded by `cmp edx, 0xB42174` against `Build::vftable`.
* **`GameDaemon::do_danger` is declared a member and is not one.** `ret 0x14`, five stack
  arguments, and the emitted body never reads `ECX`. This is the third instance of the
  standing finding; it keeps being true.

Two more inlines that read as noise until you name them:
`cmp eax, 0x6535C0` is `ObjectData::hits_left` and the surrounding block is that function's
body (`min(hits(0) - damage, hits(0))`, clamped to zero if either term is negative);
`cmp eax, 0x639970` is `BuildTypeData::basic_type` and the block is
`from < 0 ? type : buildtypes[from]->basic_type()`.

## 4. Two globals worth reusing

* `div_3_table` is `0x00CAE5FC`, and `RCoord(c) = div_3_table[(c ^ 0x63637) >> 9]`.
  `map_terrain.rs` already proved that composition equals `floor(c / 1536)`; the only new
  part here is that `calc_danger` reads the *stored* `SubObjectData::x_internal` /
  `y_internal` fields, which are XOR `0x00063637`.
* `[0x00C0AAA0]` is `PtrArray<BuildType> buildtypes` `+0x10` — the element pointer, not a
  separate table. That is the array `basic_type()` indexes.
* `[0x00C0AEC0]` is `Units units` `+0x10` — `Units::lists` is `PtrArray<Unit>[10]`, so
  `0x00C0AEC0 + 0x1C*who` is `units.lists[who].data`. Pass 2 reads validity from
  `objects.lists[who][o]` and `ptype->role` from `units.lists[who][o]` at the **same
  index**; `Objects::lists` is a `MultiPtrArray<Object>` over the same entities.

## 5. What this port deliberately does not do

* **It does not reproduce the unchecked centre store.** Retail bounds-checks the eight
  neighbours and not the centre: it computes `ry * reg_xs + rx` from the object position
  and stores through it. This port refuses an out-of-plane centre and counts it in
  `CalcDangerTrace::unmapped_centre_cells`. Named divergence at the memory-safety floor.
* **It does not invent an object host.** `attack()`, `hits_left()`, `is(...)`,
  `basic_type()`, `is_seen()` and both band bounds come from `CalcDangerHost`, whose
  `preflight` must refuse the whole child before any plane is touched. The Sim adapter
  snapshots every reached answer before the step-12 shell: sparse/dense band identity from
  `World`, leaders from the exact step-8 records, Unit roles from the step-12 type authority,
  Build state from `BuildData`, and fort/tower answers from step 8's existing object query
  packages. A reached `UnitData::attack`, late Airbase/Dock/basic-type strength rung, or missing
  type package still leaves `Gap::GameDaemonCalcDanger` charged. The raw static type attack is
  deliberately not substituted for the 1,219-byte `UnitData::attack` override.
* **It does not resolve the `role` bit or the two `build_flags` bits to shipped names.**
  `role & 0x10000`, `build_flags & 0x10` and `build_flags & 0x40000000` are used as the
  binary uses them. Mapping them to `rules.xml` attribute names is a separate derivation.

## 6. Tick hook

Wired in `crates/don-sim/src/tick.rs`. On `frame % 200 == 0`, the adapter builds one immutable
projection before `GameDaemon::process_all` preflight. The shell refuses before victory, plane,
market, border, collision, or Group mutation if any reached answer is absent. After acceptance,
the callback runs `calc_danger(&mut sim.map.world, &prepared)`; the prepared host's own trait
preflight is infallible, so canonical facts are not queried a second time after victory work.

`game_daemon_calc_danger_tick.rs` exercises an active band-2000 Tower through the real 29-step
tick. The center receives `-100`, each valid neighbor `-50`, the World checksum changes, the
danger image survives save/load/resave, and the direct and resumed simulations agree after the
next frame. Its paired mutation test removes the reached tower answer and proves that both the
daemon record and all eight danger planes remain byte-for-byte unchanged. This is executable
integration evidence, not a retail oracle; the tier remains C.
