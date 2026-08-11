# New-Unit graphics refresh and initial Guy location

Status: exact source-only setup continuation. This tranche consumes the identity-bound
`UnitGuyInitPrefixReceipt` at `Unit::init +0xBCC1`, continues through
`Unit::update_gpiece` and the setup-selected path of `Unit::set_new_location`, and returns
the complete synchronized Guy image plus ordered terrain/collision receipts. It performs no
shared collision mutation and installs no replay channel.

## Runnable receipt

- `crates/don-replay/src/unit_init_location_deep_re.rs`
- `crates/don-replay/tests/unit_init_location_deep_re.rs`
- `cargo test -p don-replay --test unit_init_location_deep_re`

The source imports the landed `setup_place_unit_deep_re` receipt. The isolated test mounts
both modules by path. Eventual registration needs one
`pub mod unit_init_location_deep_re;` line in `crates/don-replay/src/lib.rs`; that shared
hook is intentionally not part of this tranche.

Authority is the supported shipped `ron-bin/riseofnations.exe` (SHA-256
`30478a44...625079`) and `ron-bin/sbl/rise.pdb`. The complete bodies reread for this seam
are:

| procedure | VA | bytes |
|---|---:|---:|
| `Unit::update_gpiece` | `0x005E2920` | 138 |
| `Guy::update_gpiece` | `0x005D8530` | 363 |
| `Unit::set_new_location` | `0x005F8D20` | 1,757 |
| `Guy::set_angle` | `0x005D9010` | 550 |
| `Guy::set_new_location` | `0x005D86F0` | 899 |

Capstone/PE call scanning finds no `game_random` call in any of these four continuation
bodies. The receipt therefore takes the prefix's exact `rng_after_guys` state and returns
the identical word. It does not accept an unrelated replacement RNG state.

## Why the generic Unit movement body collapses during initialization

`Unit::init` first normalizes each requested coordinate to a 48-unit UCoord center:

```text
div_3_table[coord >> 4] * 48 + 24
```

It passes those normalized values into `Object::init -> SubObject::init`. `SubObject::init`
stores them at object `+0x10/+0x14`, queries initial terrain Z, and adds the object to the
world. Later, `Unit::init` passes the *same two normalized values* to
`Unit::set_new_location(x,y,1,1)` at `0x00612CD4`.

Consequently both comparisons in `Unit::set_new_location` are false:

- old and new WCoords (`div_3_table[coord >> 8]`) are equal;
- old and new TCoords (`div_3_table[coord >> 6]`) are equal.

The generic eject/remove/add-world, water transition, cast-order, goody, fog/CEO, and
visibility arms are therefore unreachable on this particular new-Unit call. This is a
control-flow proof from the two complete caller/callee bodies, not a fitted setup shortcut.
The selected path stores the same encoded coordinates, calls
`TerrainOut::find_tcoord_z(tx,ty,1)` at `0x005F9069`, stores the returned Unit Z, and enters
Guy materialization.

Terrain is not guessed. Every `find_tcoord_z` and `find_data_z` answer is an explicit receipt
with ordinal, call/body VA, coordinate domain, arguments, and return value. A missing,
reordered, coordinate-mismatched, or surplus answer rejects the whole pure transaction.

## `Unit::update_gpiece`

The function first visits live squad slots `[0,guy_mark)`. It then reads current
`squad_size` and `crew_size` and visits crew slots
`[squad_size,min(guys.length,squad_size+crew_size))`. Each slot calls
`Guy::update_gpiece`, then unconditionally clears the presentation hint at `GuyOut+0xD0`.
For a new Unit, `guy_mark == squad_size` and `guys.length == squad_size+crew_size`, so each
non-null slot is visited exactly once in pointer order.

This refresh is load-bearing. `Guy::init_real` previously called `update_gpiece` and then
zeroed `track_dx/track_dy`; the Unit-level refresh restores the graphics-derived track
offsets immediately before location propagation. Guy 0 remains special: its update path
forces track offsets to zero. Every other slot receives the coherent, supported-hash
`ExtractedGuyGraphics` gpiece and track pair. Pivot angles/flags survive from
`Guy::init_real`; `Unit::update_gpiece` does not reset them.

## Squad lattice

`Unit::init` set `UnitData::angle = 0x55555555` earlier. The final call passes both boolean
arguments as one, so every live squad Guy receives that desired/current/last angle and is
teleported.

When `guy_mark == 1`, retail uses its short fast path: Guy 0's destination is the Unit
anchor. Otherwise the complete lattice is:

```text
spacing = type.guy_spacing * (form == 5 ? 2 : 1)
a = sinx(angle - 90 degrees, spacing)
b = sinx(angle, spacing)
if unit_masks & 2: a = -a; b = -b

columns = form == 8 ? (guy_mark == 1 ? 1 : 2) : min(guy_mark,3)
last_row = (guy_mark - 1) / columns
center_x = (last_row*b)/2 - ((columns-1)*a)/2
center_y = (last_row*a)/2 + ((columns-1)*b)/2

row = i / columns; column = i % columns
x = anchor_x + column*a - row*b + center_x
y = anchor_y - column*b - row*a + center_y
```

Arithmetic wraps like x86 and signed halving truncates toward zero. Each destination is
clamped to `[0, world_max-1]` before `Guy::set_new_location`. The producer reuses the
existing instruction-derived `UnitGuys::initial_squad_locations` implementation and then
executes the previously missing recursive tail.

## Two recursive crew waves

Both `Guy::set_angle` and `Guy::set_new_location` recurse only when `guy_num == 0`. They
iterate every crew pointer from `squad_size` through `guys.length-1`. For a crew Guy with
graphics track pair `(dx,dy)`, its destination relative to Guy 0 is:

```text
x = base_x + sinx(angle+90,dx) + sinx(angle+180,dy)
y = base_y + sinx(angle,dx)    + sinx(angle+90,dy)
```

If either track component is nonzero, the result is clamped to the world. If both are zero,
retail does not run the clamp block; this matters during the first wave because Guy 0 still
has `Guy::clear` coordinates `(-1536,-1536)`.

Initialization produces two observable waves in this exact order:

1. Unit calls `Guy0::set_angle(angle,1)`. Guy 0 still sits at `(-1536,-1536)`, so each crew
   destination is derived from that base, then the crew Guy is angle-set and teleported.
2. Unit installs Guy 0's final squad destination and calls
   `Guy0::set_new_location(...,1)`. Guy 0 is moved first; crew destinations are recomputed
   from the final Guy-0 coordinate, and every crew Guy is angle-set and teleported again.
3. Remaining squad Guys are angle-set and moved in increasing pointer-slot order, without
   crew recursion.

The first wave is not optimized away in the producer even though the second overwrites its
final position: it performs terrain-height reads and therefore belongs to deterministic call
chronology.

For a non-air squad Guy whose old/new UCoords differ, `Guy::set_new_location` calls
`CollCheck::move_unit(old_x,old_y,new_x,new_y,new_block_radius)` at `0x005D8797`. Crew never
stamp collision because `guy_num >= squad_size`; domain 2 suppresses the collision call for
squad Guys as well. The source emits these shared mutations as ordered
`CollisionMoveUnitRequest`s rather than modifying a shared collision owner.

Height writes after each Guy coordinate store are exact:

- domain 1 stores Z zero and makes no Guy terrain call;
- domain 2 with type flag `+0x2B4 & 0x20` moves toward `ground+1000`, clamped to `+/-30`;
- domain 2 without that flag retains the prior Z;
- every other domain stores `TerrainOut::find_data_z(x,y,0)` directly.

Snap mode then copies `x/y/z` into `last_x/last_y/last_z`. Destinations, all three angles,
graphics gpiece/track values, and the earlier pivot/animation state remain in the returned
155-byte synchronized Guy images.

## Exact residuals and evidence limit

For ordinary land starting Units, the first unapplied shared mutation is the first
`CollCheck::move_unit` request, normally Guy 0 moving from its clear sentinel cell to its
initial lattice coordinate. All such requests are journaled; applying them atomically belongs
to the collision/world owner.

After the location body returns, the exact next instruction is `0x00612CD9` in `Unit::init`,
which begins the post-location sentinel/field stores. That is the next code residual. No
claim is made about the remainder of the 3,732-byte Unit initializer or either replay
checksum channel.

Existing coherent retail evidence—not a new live/VM operation in this tranche—observed
type-62 Units with `guy_mark=1`, pointer-array lengths two or three, Guy 0 at the Unit anchor,
and stable crew-relative positions across independently sampled Units. The replay Guys corpus
has 21 checksum-bearing recordings and 222,938 Guy comparisons with distinct nonzero first
values. Those facts make the crew tail mandatory; the formulas, double-wave chronology, and
addresses here come from the shipped PE rather than fitting those observations.
