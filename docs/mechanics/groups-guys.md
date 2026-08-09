# Groups and Guys

**Lane:** `mech:groups-guys`. **Channels served:** `CheckSums::check_groups` (channel 6) and
`CheckSums::check_guys` (channel 7) of the fifteen in `CheckSums::check_all` `0x00936560`.
**Code:** `crates/don-sim/src/systems/groups_guys.rs`. **Tests:** 54, all passing.

---

## 1. What now works

Both channels are **byte-exact-by-construction**: the two checksum drivers and all three
`walk_data` implementations under them are transcribed from the retail machine code, and
the Rust emits the same byte ranges in the same order. On top of that, the parts of the two
subsystems that *change* those bytes every frame are ported: the guy movement integrator,
the guy turret settle, group membership pruning and compaction, and the formation rotation.

| thing | status | where |
|---|---|---|
| `CheckSums::check_guys` `0x00937430` traversal | ported | `check_guys` |
| `CheckSums::check_groups` `0x00937530` traversal | ported | `Groups::check_groups` |
| `PtrArray<Guy>::walk_data` `0x0046DF30` (checksum side) | ported | `UnitGuys::walk` |
| `GuyData::walk_data` `0x005E0210` — the flat 155-byte window | ported | `GuyData::walk_bytes` |
| `Group::walk_data` `0x00708400` — length-prefixed, six arrays | ported | `GroupData::walk` |
| Guy population = `squad_size + crew_size`; `guy_mark` semantics | ported | `UnitGuys::set_type` / `spawn_full` |
| `Unit::set_new_location` `0x005F8D20` initial squad lattice | ported + live state checked | `initial_squad_locations` / `set_initial_locations` |
| `Guy::move` `0x005D9240` integrator | ported | `GuyData::move` |
| `Guy::process` `0x005E0230` turret settle + block-registration gate | ported (registration hook stubbed) | `GuyData::process` |
| `GuyData::turn_speed` `0x005DE340` | ported | `GuyData::turn_speed` |
| `Guy::turn_towards` / `turn_angles` `0x005D9720` / `0x005D98C0` | ported | `turn_towards` / `turn_angles` |
| `GuyData::get_speed` `0x005DE410` | ported | `GuyData::get_speed` |
| `vector_dist` `0x0046CFF0` | ported | `vector_dist` |
| `sinx` / `cosx` / `angle_diff` `0x0092D100` / `0x0092D0C0` / `0x0092D0B0` | ported (**new — see §5**) | `sinx` / `cosx` / `angle_diff` |
| `Groups::process` `0x006FA210` round-robin | ported | `Groups::process` |
| `Group::normalize` `0x00711540` / `get_num` `0x00714700` | ported | `GroupData::normalize` / `get_num` |
| `Group::add` `0x00714350` | ported (state half) | `GroupData::add` |
| `Group::update_positions` `0x00713810` formation rotation | ported | `GroupData::update_positions` |
| `Group::compute_speed` `0x00707F80` | ported | `GroupData::compute_speed` |
| `Group::action_form` `0x00707220` form selection | ported (selection half) | `resolve_form` |
| `Form::categorize` / `Form::compute` layout | **not ported** | §6 |
| collision-block bit writes | **not ported** (hook) | §6 |
| graphics-derived gpiece, track offsets, and turret pivot state | exact extractor boundary | `graphics_turret`; [graphics-turrets.md](graphics-turrets.md) |

### How it was measured

Everything is `[measured]` on this Mac against `ron-bin/riseofnations.exe`
(sha256 `30478a44…625079`) and `ron-bin/sbl/rise.pdb`, by capstone disassembly. Structure
was taken from `re/decomp-all/*.c` where the control flow is large, and every *value* was
read back out of the instruction stream. Field names and offsets are the PDB's.

The Rust was compiled and tested **out of tree**, because `crates/don-sim/src/lib.rs` is
another lane's file and does not yet declare `pub mod systems;`. A shim crate re-roots
`generated::state::SINE_TABLE`, `trig`, and this module by `#[path]` and runs
`rustc --edition 2021 --test`:

```
test result: ok. 48 passed; 0 failed; 0 ignored; 0 measured
```

The tests are behavioural, not tautological: the checksum tests assert that a change to
the state (a soldier dying, a guy moving one world unit, a member joining the last group of
the last player, `last_group[7]` changing) **moves** the channel, and that a change the
engine does not hash (`proc_group`, array entries past `num`) **does not**.

**Fidelity is Tier C except for the Tier-B initial-materialization state check in §2.4.**
There is still no injected retail call oracle for a `Guy` or `Group` entry point. Most tests
are therefore evidence that the Rust matches the x86 reading; the coherent live samples add
independent state evidence only for raw Guy coordinates, `guy_mark`, zero `off_x/off_y`, and
the single-live-squad-at-anchor cases described below.

---

## 2. Guys — a unit is a squad, and the squad is real state

A `Unit` in Rise of Nations is a squad of individual soldiers. Each soldier is a `Guy`,
240 bytes, held in `UnitData::guys : PtrArray<Guy>` at `+0xE4`. `Guy` has its own
`process`/`move`, called at the tail of `Unit::process` `0x00610BC0`, and its own
`walk_data`, so guys are lockstep-critical state, not decoration.

### 2.1 How many guys, and which are which

`Unit::set_type` `0x00612FA0` is the allocator, and it is unambiguous [measured]:

```
guys.length = unittypes[t]->squad_size + unittypes[t]->crew_size;
for (i = squad_size; i < guys.length; i++) guys.list[i] = Recycler<Guy>::pop();  // crew
guy_mark = min(guy_mark, squad_size);                                            // squad
for (p = 0; p < guy_mark;   p++) { guys.list[p]->type = t; init_real(); guys.list[p]->guy_num = p; }
for (i = squad_size; i < guys.length; i++) { ...->who = unit.who; ...->o = unit.o; init_real(); }
```

so:

| quantity | source | offset |
|---|---|---|
| total guys | `squad_size + crew_size` | `UnitTypeData +0x304` + `+0x30C` |
| fighting soldiers | `squad_size` | `+0x304` |
| live soldiers right now | `UnitData::guy_mark` | `UnitData +0xB5`, a `char` |
| crew (drivers, gunners) | `crew_size`, at indices `[squad_size, total)` | `+0x30C` |

`guy_num < squad_size` is the predicate that separates the two populations, and it gates
**three** different things: the type-derived turn speed (`GuyData::turn_speed`), the `+9`
speed bonus (`GuyData::get_speed`), and collision-block registration (`Guy::process`). Crew
guys are bodies that ride along; they do not block.

`guy_mark` is written only in `Unit::Unit`, `Unit::init`, `Unit::set_type` and
`Unit::swap_team` [measured, exhaustive scan of `re/decomp-all` for stores to `+0xB5`].

### 2.2 Guy count and hit points — the honest answer

**I could not close this.** The brief asked how guy count relates to unit hit points and to
a "3-sub-unit damage division". What I can state:

- `Objects::kill_guy` `0x00659410` has exactly **five** callers: `Unit::close` (×2),
  `Unit::set_type` (×2), `ConsoleWin::run_cmd` [measured, `tools/pdb/callers.py`]. **None
  of them is a damage path.** So individual soldiers are not killed by `Object::do_damage`
  in this build; they die when the whole unit dies or when the unit changes type.
- `guy_mark` is likewise not written by any damage function.
- Therefore the visible "squad shrinks as it takes damage" effect is either computed
  on the fly in `GuyOut::draw` `0x005DC490` (presentation, out of scope and correctly so),
  or it does not exist in this build. I did not read `GuyOut::draw` to settle which.
- `UnitTypeData::squad_size` is **not** in `unitrules.xml` — the adjacent `UBER_SIZE`
  (`+0x308`) and `CREW_SIZE` (`+0x30C`) are, `SQUAD_SIZE` is not, and the offset table binds
  a registered name `squad_size` at load site `0x0061CEEE`. So squad size comes from
  somewhere other than the unit XML (most likely the graphics/animation binding, given
  `GRAPH`). **Unresolved.**
- I found **no** "3-sub-unit damage division" anywhere in the guy or group code. If it
  exists it is in `object.cpp`, which is the combat lane's ground. I am not going to invent
  a relationship to fill the gap.

`UnitTypeData::uber_size` `+0x308` sits between `squad_size` and `crew_size` and is *not*
read by any function I ported. It is a plausible home for a sub-unit division and is worth
one hour from whoever owns combat.

### 2.3 The guy checksum image

`GuyData::walk_data` `0x005E0210` is four instructions of substance:

```asm
005e0216 lea eax, [ecx + 0xa3]
005e021e lea eax, [ecx + 8]
005e0226 call dword ptr [esi]      ; DataWalk::walk_function(begin, end)
```

One flat range, `[this+0x08, this+0xA3)` — **155 bytes**, no mask bits, no field-wise walk.
Every offset in that window is a named field, so there are no padding holes, and the image
is the raw struct bytes.

Notable: the window **includes five `f32`s** — `turret_inc` `+0x40`, `bank` `+0x44`,
`last_bank` `+0x48`, `pitch` `+0x4C`, `last_pitch` `+0x50`. Those are sim-critical bytes
even though they are floats. None of the paths ported here write them, but a complete port
must, bit-exactly, or the guys channel diverges. This is a genuine float in walked state,
alongside the two already known in `LeaderData`.

The window **excludes** `last_norm` `+0xA4`, `curr_bbox`, `last_good_river*`, and all of
`GuyOut`.

`PtrArray<Guy>::walk_data` `0x0046DF30` wraps it. The checksum side emits, in order:

1. `length` (4 bytes) — **and stops here if it is zero**
2. `size` (4)
3. `increment` (2, at `+0x0C`)
4. `flags & ~0x40` (1) — the `0x40` bit is cleared in the object first
5. `length` presence bytes, one per slot, `1` if the pointer is non-null
6. `size` and `increment` **again**, as one six-byte range `[this+8, this+0xE)`
7. each non-null guy's 155-byte image

Step 6 is not a transcription slip. `call [edx]` with `(this+8, this+0xE)` re-hashes bytes
already hashed at steps 2 and 3. Retail hashes them twice; so does the port, and there is a
test that fails if you collapse it.

Step 5 is the reason a dead soldier is visible to the checksum: killed squad slots become
genuine null pointers, and the presence byte flips.

### 2.4 Initial squad materialization is a rotated lattice

`Unit::init` allocates and initializes the complete Guy pointer array, then calls
`Unit::set_new_location` `0x005F8D20` with both the set-angle and teleport flags. The latter
routine lays out the `guy_mark` live squad prefix. It does not use `x_spacing` or
`y_spacing`, and it does not store the lattice in `GuyData::off_x/off_y`:

```text
spacing = type.guy_spacing * (unit.form == Sparse ? 2 : 1)
a = sinx(unit.angle - 90deg, spacing)
b = sinx(unit.angle,         spacing)
if (unit.unit_masks & 2) { a = -a; b = -b; }

columns = unit.form == Column ? (guy_mark == 1 ? 1 : 2) : min(guy_mark, 3)
last_row = (guy_mark - 1) / columns
center_x = (last_row*b)/2 - ((columns-1)*a)/2
center_y = (last_row*a)/2 + ((columns-1)*b)/2

for i in 0..guy_mark {
    row = i / columns; col = i % columns
    x = anchor_x + col*a - row*b + center_x
    y = anchor_y - col*b - row*a + center_y
    clamp x/y to the world; set angle; set destination; teleport
}
```

The divisions are signed x86 truncation toward zero and the intermediate arithmetic wraps.
`UnitGuys::initial_squad_locations` reproduces that arithmetic; `set_initial_locations`
also materializes `x/y`, destination, last position, angle and last angle.

`Guy::clear` `0x005DB590` zeroes `off_x` and `off_y` together at `Guy+0x92`. Neither
`Unit::init` nor this lattice path assigns them afterward. A coherent paused retail sample
at frame 163 checked three live units: every Guy had `off=(0,0)`; both independently sampled
type-62 units had the same crew-relative positions. In those units `guy_mark=1` while the
pointer-array lengths were 2 or 3, proving that only Guy 0 was squad and the remaining
bodies were crew. Guy 0 was exactly at the unit anchor in every sample.

Crew are a separate recursive tail: Guy 0's `set_angle` / `set_new_location` propagates to
indices `[squad_size, length)` using each crew Guy's graphics-derived `track_dx/track_dy` at
`+0x54/+0x58`. Those graph-packet values are not present in `UnitTypeStats`. The port leaves
crew untouched rather than inventing attachments; crew do not stamp collision footprints.

### 2.5 `Guy::move` — the integrator

`Guy::move` `0x005D9240` is 1,233 bytes and is **pure integer**. Structure from
`re/decomp-all/005d9240.c`, values from the instruction stream:

```
last_x = x; last_y = y; if (!(guy_flags & 0x40)) last_z = z; last_angle = angle;

if (des_x == x && des_y == y) {                       // idle
    last_speed = 0;
    ...animation selection...
    if (!(guy_flags & 2)) turn_towards(des_angle);
}
else if (guy_num == 0 || (track_dx == 0 && track_dy == 0)) {
    last_speed = vector_dist(des_x - x, des_y - y);
    set_new_location(des_x, des_y);                   // guy 0 and untracked guys teleport
}
else {
    a   = find_angle(des_x - x, des_y - y);
    rem = turn_towards(a);
    if (rem > turn_speed(1) * 2) return;              // turn only — and NO avg_speed update
    step = get_speed() * 11 / 8;   last_speed = step;
    if (ai_speed > 1) step *= ai_speed;               // last_speed keeps the UNSCALED value
    if (|dy| + |dx| <= step) set_new_location(des_x, des_y);
    else {
        sx = sinx(angle, step);  sy = cosx(angle, step);
        if (|dx| < |sx|) sx =  dx;
        if (|dy| < |sy|) sy = -dy;
        x -= off_x;  y -= off_y;                      // subtract the squad micro-offset
        if (world.valid(x + sx, y - sy)) set_new_location(x + sx, y - sy);
        else stopped = 0;
    }
}
avg_speed = (avg_speed*3 + last_speed) / 4;           // truncating toward zero
```

Five details that are load-bearing and are reproduced:

- **`guy_num == 0` teleports.** The squad's first soldier is snapped straight to the
  destination each frame; only guys `1..` with a non-zero `track_*` actually integrate.
  The first guy is effectively the unit's anchor.
- **`speed * 11 / 8`.** Not the raw speed. `imul ecx,11; sar 3` with the sign correction.
- **The off-angle early return skips `avg_speed`.** A guy that spends a frame turning does
  not decay its smoothed speed, which changes `turn_speed`'s damping next frame.
- **`ai_speed` multiplies the applied step but not `last_speed`**, so the *checksummed*
  `last_speed`/`avg_speed` are independent of the AI speed cheat while the position is not.
- **`x -= off_x; y -= off_y` is not undone on a blocked step.** `GuyData::off_x`/`off_y`
  (`i16` at `+0x92`/`+0x94`) are the guy's offset inside its squad's formation, the stored
  position carries it, and the step is taken from the unit anchor. A blocked step leaves the
  guy at `(x - off_x, y - off_y)`. That is retail.

### 2.6 `Guy::process` — turrets and blocks

```
Guy::move();
guy_flags &= ~0x0002;
if (guy_flags & 0x0100)
    for t in 0..4:                                    // turret_angles vs des_turret_angles
        step toward the desired angle by 0x0AAAAAAA, snapping inside one step,
        and set node_flags bit t when settled
if ((game.frame + o) % 64 == 0 && avg_speed == 0) {
    if (type->domain != 2 && guy_num < squad_size && type->new_block_radius != 0)
        register this guy's collision-block bits;
    guy_flags &= ~0x0020;
}
```

`0x0AAAAAAA` is **exactly `2^32 / 24`** — one twenty-fourth of a turn, 15 degrees of turret
slew per frame. The angle space is a full-turn `u32` (a binary angular measure), so that
constant is unambiguous.

The `(frame + o) % 64` phase means a guy refreshes its collision block once every 64 frames
— about 4.3 game seconds — on a per-unit phase spread across the object array. That is one
more ordering fact a deterministic scheduler has to reproduce.

### 2.7 `GuyData::turn_speed` — the angular budget

```
step = 0x40000000;                                    // crew default: a quarter turn
if (guy_num < squad_size) {
    step = (type->turn_speed >> 8) * constants[+0x08];
    if (unit->unit_masks & 0x80000) step *= constants[+0x0C];
} else if (track_dx || track_dy) return 0x40000000;
if (last_speed == 0 && (guy_flags & 0x10)) return 0x80000000;   // face instantly
if (arg) return step;
floor = constants[+0x08] * 0xB60B;
q     = step / (avg_speed/4 + 1);                     // UNSIGNED divide
return q > floor ? q : floor;                         // UNSIGNED compare
```

The `>> 8` maps a `<TURN_SPEED>` rule value (Citizen: 45) into binary-angle space before
the rule multiplier. The damped form — the `arg == 0` path — makes a *slow* guy turn
*faster*, which is the right sign for a formation keeping cohesion.

`constants[+0x08]` and `constants[+0x0C]` are two `Constants` fields I did **not** name;
they need a pass over `Constants::init` `0x00569A90`'s registration order. They are
parameters in the port (`GuyEnv::turn_scale`, `turn_scale2`), not baked.

`Guy::turn_towards` `0x005D9720` snaps when the remaining angle is under `0x02222220`
(= `2^32/120`, three degrees) and otherwise steps by `turn_speed(1)`, returning what is
left. The magnitude fold is `not`, not `neg` — `if (d > 0x80000000) d = ~d` — so the
reflected half is one short of a true absolute value. Every caller inherits that, including
`angle_diff` `0x0092D0B0`, and the port keeps it.

---

## 3. Groups — the selection layer the RL action space sits on

A `Group` (`sizeof(GroupData)` 2508, `sizeof(Group)` 2516) is a selection plus its
formation state. `CommandPackage::process_X` rebuilds the target `Group` and calls
`Group::action_X`, which fans the command out into per-unit `UnitOrder`s. 45 `action_*`
methods exist (`groups.cpp:2029–12221`).

### 3.1 Shape

```
GroupData +0x04  id  army  num  form  stamp  ox  oy  o_dist  o_angle  disband
          +0x2C  order_num  priority  role  think_frame  new_speed  speed  form_num
          +0x48  facing  buildings  who  march              (four u8)
          +0x04C off_x  [i32; 128]     lateral formation offset, in 48-unit cells
          +0x24C off_y  [i32; 128]     forward formation offset
          +0x44C curr_x [i32; 128]     rotated world-space offset
          +0x64C curr_y [i32; 128]
          +0x84C angles [i8;  128]
          +0x8CC list   [i16; 128]     object indices into objects.lists[who]
```

**128 members maximum** — `Group::add` refuses at `num >= 0x80` and every array is 128 wide.

**512 groups total: 8 players × 64.** `Groups::process` indexes
`groups.list[proc_group + who*0x40]` and wraps `proc_group` at `0x3F`, so the array is
player-major with a 64-group stride. `GroupsData::last_group : int[8]` at `+0x1C` is the
per-player "current group" cursor, and it is hashed.

### 3.2 The group checksum image

`Group::walk_data` `0x00708400`, exactly:

```
walk(this+0x04, this+0x4C)                     ; 72-byte scalar header
if (num != 0) {
    walk(this+0x8CC, this + (num+0x466)*2)     ; list[0..num]     i16
    walk(this+0x04C, this + (num+0x013)*4)     ; off_x[0..num]
    walk(this+0x24C, this + (num+0x093)*4)     ; off_y[0..num]
    walk(this+0x44C, this + (num+0x113)*4)     ; curr_x[0..num]
    walk(this+0x64C, this + (num+0x193)*4)     ; curr_y[0..num]
    walk(this+0x84C, this + 0x84C + num)       ; angles[0..num]   i8
}
```

Two consequences, both tested:

- **The walk is length-prefixed by `num`.** Entries past the live count are invisible, so a
  port need not clear them on removal — and a port that *does* clear them still matches.
- **`list` is hashed before the offsets**, which is not the declaration order. Get this
  wrong and the channel diverges on every non-empty group.

`CheckSums::check_groups` `0x00937530` then walks every slot in `groups.list` (stride
`0x9D4`) and folds `last_group[8]` in as **eight separate four-byte `adler32` calls**,
reached through the `const_last_group` pointer at `+0x3C`. Adler-32 is a rolling sum with
the state threaded through the return value, so the chunking is transparent — but only
because it is threaded, which it is.

**`proc_group` is not in the checksum.** `Groups::walk_data` `0x00713E30` (the save path)
walks `[+0x1C, +0x3C)` *and* `[+0x40, +0x44)`; `check_groups` walks only the first. Two
clients could disagree about the round-robin cursor without desyncing — a real, small
asymmetry between save state and sync state, and another instance of
`sim-critical ⊂ save-game` (`architecture.md` §8.6).

### 3.3 `Groups::process` — one group per player per frame

Called last inside `GameDaemon::process_all`, i.e. *before* any unit moves.

```
for who in 0..8:
    if (leaders[who].flags & 1) {
        g = groups.list[proc_group + who*64];
        <Group::normalize inlined verbatim>
        g.find_role();
        g.compute_speed();
    }
proc_group++;  if (proc_group > 0x3F) proc_group = 0;
```

MSVC inlined `Group::normalize` `0x00711540`, `Group::find_role` `0x007081F0` and the
`compute_speed` tail into one body; the two functions are byte-identical where they overlap.

**A group is therefore re-validated once every 64 frames — about 4.3 game seconds.** Its
`num` can name dead units for that long. Anything reading `GroupData::num` as a live count
without calling `get_num` is reading stale data, and that staleness is *checksummed*, so a
port that eagerly prunes will desync.

`Group::normalize` scans **backwards** from `num-1` and, on each drop, shifts all six
parallel arrays down by one. The predicate:

```
drop if list[i] < 0
drop if !(object->flags & 1)                                  // dead
drop if priority == 0 && id >= 0
        && (!object->is_unit() || object->group != group.id)
drop if object->vt[+0x20]()
```

`Group::get_num` `0x00714700` adds a shortcut worth reproducing: a group with **fewer than
four** members and `id >= 0` runs the *full* `normalize`, while a larger group runs only the
cheap alive-only scan. Small named groups are pruned more aggressively than large ones.

### 3.4 `Group::add`

```
if (num != 0 && who != group.who) return;      // one owner per group
if (member(o, who, 1)) return;                 // no duplicates
if (num >= 0x80) return;                       // hard cap
if (num == 0) group.who = who;
off_x[num] = off_y[num] = curr_x[num] = curr_y[num] = angles[num] = 0;
list[num] = (short)o;   num++;
buildings = unit->is_building();
stamp     = Game::frame;
role     |= unittype->role;                    // an OR accumulator, never cleared on add
...then recursively add the unit at unit->down (+0x90) if it is alive...
if (id >= 0) compute_speed();
```

`GroupData::role` is an OR of every member type's `UnitTypeData::role` `+0x2C8`, and it is
in the checksum header — so it is state, not a cache.

### 3.5 Formations

`rules.xml` `<FORMATIONS>` gives ten, in the index order `GroupData::form` and
`UnitData::form` `+0xAA` use:

| 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 |
|---|---|---|---|---|---|---|---|---|---|
| Line | Refused | Envelop | Echelon Right | Echelon Left | Sparse | Square | Wedge | Column | Mob |

`Group::action_form` `0x00707220` cycles with `% 5`, so **the FORM hotkey only reaches the
first five**; Sparse through Mob are set by the AI, by scenario script, or by the unit type's
`base_form` `+0x310`. The two cycle directions wrap differently, and the port keeps both:

```
form == -1 (next) : form = (current + 1) % 5
form == -3 (prev) : form = current - 1;  if (form < 0) form = 4
form == -2        : form = leader_unit->form            // UnitData +0xAA
```

They agree over 0..4 and diverge above it: from 7, `-1` gives 3 and `-3` gives 6.

`action_form` then writes the resolved value to **every member unit's** `UnitData::form`
`+0xAA` and, if the queue argument is 1 or 2, follows with `Group::action_move_to` — i.e. a
FORM order is a per-unit field write plus an optional group move, not an order type of its
own. (`OrderIndex::CHANGE_FORM` = 18 exists and is executed by `Unit::do_form_change`
`0x005E8670`; `action_form` does not install it.)

### 3.6 `Group::update_positions` — the formation rotation

```
angle = leader_unit->angle;                                   // UnitData +0x50
if the leader has a targeted order:
    angle = find_angle(order.target_x - unit.x, order.target_y - unit.y);
for (i = 0; i < form_num; i++) {
    curr_x[i] = cosx(angle, off_x[i]*48) + sinx(angle, off_y[i]*48);
    curr_y[i] = sinx(angle, off_x[i]*48) - cosx(angle, off_y[i]*48);
}
```

This is the engine's own basis. With angle 0 pointing north (`-y`) and `0x40000000` east,
"right" is `(cos, sin)` and "forward" is `(sin, -cos)`, so `off_x` is a **lateral** offset
and `off_y` a **forward** one, both in cells of **48 world units** — the same quarter tile
the unit pathfinder's `astar_path` steps with.

**The loop bound is `form_num` `+0x44`, not `num` `+0x0C`.** The formation can be wider than
the live membership, and the port tests that.

---

## 4. Incidental discovery: object `Coord`s are XOR-obfuscated

`Group::update_positions` reads the leader unit's position as

```asm
007138f3 mov eax, dword ptr [ecx + 0x14]
007138f6 xor eax, 0x63637
007138fd mov eax, dword ptr [ecx + 0x10]
00713903 xor eax, 0x63637
```

**`SubObject`'s `x` `+0x10` and `y` `+0x14` are stored XORed with `0x00063637`** — a
cheap anti-cheat obfuscation. `GuyData`'s coords at `+0x0C`/`+0x10` are **not** obfuscated;
`Guy::move` reads them raw.

This is not my lane's state, but any lane doing a live-memory read of unit positions, or
any typed heap crawl using `schema/vtables.json`, will read garbage without it. It is
exported as `GroupData::COORD_XOR`.

---

## 5. Correction: `crate::trig`'s raw entries are wrong for two of the four quadrants

`crates/don-sim/src/trig.rs` (another lane) already ports `sin_table` `0x00A46A00`,
`cos_table` `0x00A469F0`, `find_angle` `0x0092D130` and the generated `SINE_TABLE`. I
independently re-derived the table from `trig_init` `0x00A46980` — it computes
`trunc(sin(i * 1.570796327 / 255.0) * 65535.0)` two entries at a time in SSE doubles — and
**the 256 values in `generated::state::SINE_TABLE` match exactly**. That artifact is good.

What is missing is the pair retail actually calls. `sinx` `0x0092D100` and `cosx`
`0x0092D0C0` are not thin aliases of `sin_table`; they **pre-fold** the angle:

```
if (mag == 0) return 0;
if (angle < 0) { mag = -mag; angle &= 0x7FFFFFFF; }
a = (angle & 0x40000000) ? 0x7FFFFFFF - angle : angle;
return sin_table(a, mag);
```

Because `a` always lands in `[0, 0x3FFFFFFF]`, `sin_table`'s second-quarter branch — the
one that computes `0xFFFF - T[i] + delta`, which is *not* a mirror — is **never reached
through them**. Calling `sin_table`/`cos_move_step` directly where retail calls `sinx`/`cosx`
is wrong wherever the angle is in quarter 1 or 3.

Measured, on the five direction vectors in `trig.rs`'s own `angle_then_step_stays_on_the_ray`
test, as normalised cross-product against the true ray:

```
raw sin_table / cos_move_step   worst cross = 0.3041
folded sinx / cosx              worst cross = 0.0105
```

Four of `trig.rs`'s eight tests currently **fail** in the tree for this reason
(`angle_then_step_stays_on_the_ray`, `sin_table_tracks_sin_except_at_the_wrap`,
`the_index_255_wrap_is_reproduced`, `find_angle_tracks_atan2_within_a_third_of_a_degree`).
Three are expectation bugs, not code bugs:

- `the_index_255_wrap_is_reproduced` asserts the index-255 wrap yields a *wrong* value. It
  does not. `delta = (T[0] - T[255]) * 0x3FFFFF = -65535 * 4194303` **overflows `i32` and
  wraps to `+4,259,839`**, which `>> 22` is `1`, and `1 + 65535 = 65536`, so
  `(65536 * r) >> 16 == r`. The overflow lands on the exactly correct answer. That is why
  `cosx(0, r) == r` and `sinx(90°, r) == r` come out exact, and it is why the code must keep
  `wrapping_mul` — but the assertion `assert_ne!` is inverted.
- `sin_table_tracks_sin_except_at_the_wrap` measures peak error 417/1000. That is the
  non-mirror quarter-1 branch, reachable only by callers that skip the fold.
- `find_angle_tracks_atan2_within_a_third_of_a_degree` measures 0.448°; the bound should be
  half a degree.

Also, `trig.rs` documents `cos_move_step` as `Unit::move_step` inlining a bare
`sin_table(angle + 0x40000000)`. `Group::update_positions` demonstrably inlines the **full**
`cosx` fold (`0x00713948`–`0x00713965`: the `neg`, the `& 0x7FFFFFFF`, the
`0x7FFFFFFF - a`, the `cmove`), and `Unit::move_step`'s decompilation shows the same
`if (mag == 0) {0,0} else {sin_table(); sin_table();}` zero-guard that only the fold has. I
believe `cos_move_step` is a partial read of that site, but I did not disassemble
`move_step` far enough to say so flatly — flagging it for the movement lane rather than
changing their file.

My module adds `sinx`, `cosx` and `angle_diff` and leaves `trig.rs` untouched.

---

## 6. What is not derived

Listed so nobody mistakes silence for coverage.

1. **Graphics hierarchy host integration.** Initial squad placement is exact, and the old
   heuristic `placeholder_offsets` has been deleted. The supported installed XML catalog,
   per-Guy gpiece/track/pivot materialization transaction and live pivot-aim arithmetic are
   recovered in `graphics_turret`; see [graphics-turrets.md](graphics-turrets.md). Arena still
   needs a provider backed by the loaded `.bh3` hierarchy, so no attachment vector or node
   transform is synthesized.
2. **`squad_size`'s data source.** Bound at `0x0061CEEE` to `UnitTypeData +0x304`, absent
   from `unitrules.xml`. Needs a read of `UnitType::init` `0x0061AB50`.
3. **Guy count vs hit points, and the "3-sub-unit damage division".** See §2.2. No evidence
   found; nothing invented.
4. **`Form::categorize` `0x0072E250` (1,678 bytes) and `Form::compute` `0x0072E8E0`.** These
   are the actual formation-shape layout: they fill `FormData`'s `num_category[18]`,
   `x_spacing[18]`, `y_spacing[18]`, `per[18]`, `space[4][18]`, `cat_id[128]`,
   `category[128]`, `to_x/to_y/off_x/off_y[128]`, `wedge`, `across`, `reverse`, driven by
   `Form::compute_rows_and_columns` `0x0072D910` and `Form::compute_dests` `0x0072CBA0`
   (3,431 bytes). `Group::compute_form` `0x00707C80` picks the `Form` out of the global
   `forms` array (`0x00E3A120`, stride `0xE98`), `memset`s `+0x30` for `0xE60` bytes, calls
   those two, and negates every offset when the requested facing reverses. The 18
   `FormCatIndex` values are recovered (`FORM_CAT_MECH` … `FORM_CAT_AIR_RANGED`). **The
   layout maths itself is unread.** This is the largest remaining piece of the group lane
   and it is the piece the RL action space needs to predict where units end up.
5. **Collision-block bit writes in `Guy::process`.** The gating predicate is exact; the body
   walks a radius-indexed offset table (`0x00ADD1E0` counts, `0x00ADC400` / `0x00ADCAF0`
   offsets) into `World::new_coll_block` `0x0046D250`. That is world state and belongs to
   the world lane; the port exposes a `register_blocks` hook.
6. **`Guy::do_turn` `0x005D97A0`, `Guy::set_anim` `0x005DA300` (4,723 bytes),
   `Guy::set_new_location` `0x005D86F0`, `Guy::inc_time`, `Guy::execute_events`.** The port
   models `do_turn` as an angle store. `set_anim` drives `cur_anim`, which **is** in the
   checksum window, so animation selection is sim-critical and is currently a gap in the
   guys channel. `Guy::inc_time` writes `cur_time`/`end_time`/`last_time`, also in the
   window, and is called from `Objects::inc_time` (tick step 15), not from `Guy::process`.
7. **`Constants +0x08` and `+0x0C`**, the two turn-speed scales. Parameters, not constants,
   in the port.
8. **The 44 `Group::action_*` other than `action_form`.** `architecture.md` §10 already
   flags that only the `move_to` leg has been traced end to end; that is still true.
9. **`Group::find_role` `0x007081F0`** is called by `Groups::process` and not ported — it
   recomputes `role` from the members. `compute_speed` is ported.
10. **No call oracle coverage.** The main-thread live samples in §2.4 validate settled state,
    not an injected invocation of `Unit::set_new_location`, `Guy::move`, or a Group method.

---

## 7. Wiring

The module is at `crates/don-sim/src/systems/groups_guys.rs` and its `pub mod` line is added
to `crates/don-sim/src/systems/mod.rs` (the shared file whose header invites exactly that).

It needs **two lines in `lib.rs`, which is not this lane's file**:

```rust
pub mod systems;
pub mod trig;
```

`systems/mod.rs`'s own header already records that the sibling modules cannot build until
`lib.rs` lands their crate-root dependencies. This module's only crate dependency is
`crate::trig`, which exists on disk.

Until then it builds and tests out of tree:

```sh
# shim.rs re-roots generated::state::SINE_TABLE, trig, and systems::groups_guys by #[path]
rustc --edition 2021 --test shim.rs -o gg && ./gg
# test result: ok. 48 passed; 0 failed
```

## 8. Checksum-harness contract

For the replay-validation harness, the two channels are:

```rust
// channel 6
let mut cs = CheckSum { value: <incoming> };
groups.check_groups(&mut cs);

// channel 7
check_guys(&mut cs, objects_valid, &leader_active, &owners);
```

`check_all` threads one `CheckSum` through all fifteen channels in order, with
`SyncLogger::logToMemory` between each; channels 6 and 7 are consecutive
(`check_groups` then `check_guys`), and channel 8 is `LeaderData::walk_data` inlined.
