# Projectiles — the `ammo` checksum channel

**Lane:** `mech:ammo`. **Channel served:** `ammo` (channel 4 of 15 in `CheckSums::check_all`,
wire offset `+0x0D` in `CheckSumsCommand`). **Module:**
`/Users/ember/dev/don/crates/don-sim/src/systems/ammo.rs`.

Everything below is `[measured]` against `ron-bin/riseofnations.exe` (sha256 `30478a44…625079`)
and `ron-bin/sbl/rise.pdb` unless it says `UNDERIVED`. No community documentation was consulted.

---

## 1. What now works, and how it was measured

The focused crate gate is:

```sh
cargo test -p don-sim --lib systems::ammo
# test result: ok. 53 passed; 0 failed
cargo test -p don-sim --test ammo_splash_transaction
# test result: ok. 2 passed; 0 failed
cargo test -p don-sim --test ammo_spline_transaction
# test result: ok. 12 passed; 0 failed
cargo test -p don-sim --test ammo_live_spline_sim
# test result: ok. 5 passed; 0 failed
```

The module shares the authoritative RNG with `crate::rng` and composes the recovered flight-band
gate in `systems::air`; it is referenced by the real tick driver.

| capability | function | how it was derived | how it is checked |
|---|---|---|---|
| The `ammo` checksum channel | `AmmoPool::checksum` | `CheckSums::check_ammo` `0x009374E0` + `AmmoData::walk_data` `0x0067AB50`, walk order read from the decompile | layout asserted field-by-field against `schema/pdb-types.json`; channel asserted order-sensitive and ULP-sensitive |
| Exact `AmmoData` memory layout | `AmmoWalk` (`#[repr(C)]`) | PDB TPI record, 27 fields, `sizeof` 108 | `assert_layout()` checks all 27 offsets at test time |
| Pool allocation / slot identity | `AmmoPool::alloc_slot` | `Objects::add_ammo` `0x00658B10`, `Objects::init` `0x0065EA80` | first-free-slot reuse and past-200 growth |
| `vector_dist` | `vector_dist` | full disassembly of `0x0046CFF0`, 105 bytes, zero FP | edge cases + the `>= 60000` overflow arm |
| `Random::get(lo,hi)` | `Rng::in_range` | `0x00A39D70`, already Tier-B in `docs/derivation/rng.md` | half-open range; `lo == hi` does not advance |
| adler-32 | `adler32` | `0x00A46830`, already Tier-B | four known vectors |
| Spawn count + muzzle points | `fire_ammo_spawns` | `Object::fire_ammo` `0x0064C8B0` | unit path RNG-free; building path exactly 2 draws/round |
| Range attenuation of accuracy | `accuracy` | `Ammo::init`, MSVC magic-divide by −192 decoded | per-tile loss and the floor of 5 |
| Aim scatter radius | `miss_radius_formula` | `Ammo::init` `0x0067C5C5` | monotone in accuracy; the `>100` quarter |
| Anti-air dud gate in the launch path | `ammo_init_targeted` | `Ammo::init` `0x0067BE49`–`0x0067C16A`, composed with `systems::air::antiair_dud_gate` | exact 0/1/2-draw short-circuit before scatter, invalid-target early return, dud checksum mutation |
| Ballistic solve | `arc_total_time`, `arc_ballistics`, `z_at`, `xy_at` | `Ammo::init` `0x0067CF1A`ff | `z(T) == ez` to 0.05; arcs above both ends |
| Flight integration + terrain clip | `ammo_inc_time` | `Ammo::inc_time` `0x0067D380` | impacts exactly at `total_time`; hillside clip rewrites `total_time` |
| Hit tests | `hit_target`, `check_hit` | `0x00678F90`, `0x00678D90` | building rectangle vs unit radius; `accuracy > 100` doubling |
| Impact + miss behaviour | `ammo_do_damage_single` | `Ammo::do_damage` `0x00678060` | hit costs 0 draws, miss costs exactly 2 |
| Splash falloff | `splash_scale`, `splash_ring` | `0x006788E0`–`0x0067892D` | linear from the bounding box; negative skips |
| Live splash transaction | `ammo_do_damage_splash_execute` | `Ammo::do_damage` `0x00678629`–`0x00678B56` | damage is interleaved with the exact table/down-chain cursor; mutation can change later admissions; projectile closes last |
| Cruise B-spline transaction | `ammo_init_cruise_spline`, `ammo_step_cruise_spline`, `RetailSpline::walk_checksum` | `0x00913960`, `0x00912F00`, `0x00911820`, `0x00911F60`, `0x009132B0`, spline arm of `0x0067D380` | exact constructed-empty array growth, knots, Cox–de Boor samples, tangent normals, nested checksum walk and indexed flight sample |
| Both nuke B-spline constructors | `ammo_init_nuke_spline_high_arc`, `ammo_init_nuke_spline_terrain` | both arms of `Spline::calc_nuke_spline` `0x00913AD0` | high arc pins all 12 controls/custom knot widths; terrain arm pins truncated query order, flag-tier priority, all-30 knot header, fail-closed host facts, and complete checksum |
| Live spline ownership and selection | `select_retail_spline_family`, `cruise_spline_inputs`, `AmmoPool::{install_nuke_spline,install_cruise_launch,step_spline_slot,recycle_if_closed,checksum_complete}` | `Ammo::init` `0x0067BBF0`, `Ammo::inc_time` `0x0067D380`, `Recycler<Spline>::pop` `0x00478360`, `CheckSums::check_ammo` `0x009374E0` | graphic flag 8 priority, exact pitch/yaw quaternion order and reach/min-segment gates, nuke object-mask fallback, same-slot ownership, LIFO capacity-preserving reuse, indexed step, close/free, missing-fact fail-close, and nested live checksum |
| `AMMO_PER_ATT` damage split | `split_damage` | `Object::do_damage` `0x0064A49E`–`0x0064A7F1` | volley sums back to the raw number; sixteenths carry |

**Tier: C throughout** — behaviourally faithful, derived from the instruction stream, but not
differentially tested against retail. Nothing here has been through the oracle. Do not promote
any of it without `tools/oracle-regress.sh` cases; see §7.

---

## 2. Corrections to the brief

**The pool is 200 slots, not 400.** `Objects::init` (`0x0065EA80`) does `malloc(800)` for the
pointer table, sets `PtrArray::length = 200`, and constructs 200 × `malloc(0x7C)` (a 4-byte
array cookie plus `sizeof(Ammo) == 120`). `increment` is `0xFFFF` = `-1` as `short`, i.e.
"grow by the current size" when the pool is exhausted — it *is* growable, so 200 is the
preallocation, not a hard cap.

**Ammo is not driven by `Objects::process_all`.** `Ammo::process` (`0x0067D370`) is **one
byte** — a bare `ret`, an empty body. All projectile motion, impact and damage happen in
`Ammo::inc_time` (`0x0067D380`), reached from `Objects::inc_time` (`0x0065DB70`), which is
**step 15** of `Game::do_frame` — *after* every unit and building has processed. This is a
real ordering fact for the scheduler: a projectile spawned this frame is stepped in the same
frame, and every projectile impact lands after all unit logic in that tick.

```
Game::do_frame
  14  Objects::process_all  ->  Unit::process / Build::process  ->  Object::fire_ammo   (SPAWN)
  15  Objects::inc_time     ->  Ammo::inc_time                                         (FLY + HIT + DAMAGE)
```

---

## 3. The channel: what is hashed, and the float problem

`CheckSums::check_ammo` iterates the pool in **slot order** `0..length` and calls the virtual
walker **only for slots where `flags & 3 != 0`**. `AmmoData::walk_data` then walks:

| # | bytes | engine range | contents |
|--:|------:|---|---|
| 1 | 1 | `[+0x04, +0x05)` | `flags` |
| 2 | 99 | `[+0x05, +0x68)` | `rolling` … `start_roll_angle` — **gated again on `flags & 3`** |
| 3 | 1 | stack byte | `ammo_path != nullptr` |
| 4 | — | | `Spline::walk_data` if that byte is set |

101 bytes per live projectile, adler-32 seeded to 1 at the top of the channel and carried
across slots. Free slots contribute **zero bytes** — the guard is in `check_ammo`, before the
virtual call — and the pool length is not itself hashed, so two runs differing only in trailing
free slots agree on this channel.

### The floats are inside the checksum

`v1z` (`+0x54`), `dx` (`+0x58`), `bank_dx` (`+0x5C`), `bank_dy` (`+0x60`) sit **inside** the
99-byte block. They are hashed bit-for-bit. This is the direct answer to the lane question
"establish exactly which `AmmoData` ballistics fields are float and whether they are
sim-critical":

- **Which:** exactly those four, and no others. Every other walked field is an integer.
- **Sim-critical:** yes, in the strongest available sense — a 1-ULP difference in any of them
  is a checksum divergence and therefore a desync. The module has a test that flips one ULP of
  `v1z` and asserts the channel moves.

This makes `ammo` the one walked channel where f32 arithmetic must be reproduced *exactly*
rather than closely. The good news is that the arithmetic is short and IEEE-clean:

```
T   = (float)total_time
v1z = ((float)(ez - sz) - G*0.5*T*T) / T          ; G = f32::from_bits(0xC127CCCD)
dx  = sqrtf((float)(ex-sx)^2 + (float)(ey-sy)^2) / T
```

`sqrtf` is IEEE-exact and safe (`README-LLM` §"Established ground truth"); the rest is
multiply/divide in binary32. The hazard is **not** the algorithm — it is operand order and x87
double-rounding. The port keeps the engine's operand order literally. This binary is SSE
binary32, not x87, so a modern host reproduces it; the risk would be a build that lets an
optimiser contract or reassociate these expressions.

### Gravity has an odd provenance — do not "fix" it

`GRAVITY` is `DAT_00CAB378`, written as the raw bit pattern `0xC127CCCD` (≈ `-10.4875`) by
**`GraphicPieces::init` (`0x008FFCC0`)**. A *presentation* class initialises the constant the
*simulation* ballistic solver depends on. A headless port must still perform this assignment.
It is not read from `rules.xml`.

---

## 4. Spawn: two completely different shapes

`Object::fire_ammo(int target_o, int target_who)` (`0x0064C8B0`). The gate is
`(target_o >= 0 && target_who >= 0) || (is_unit && order ∈ {ATTACK_GROUND(23), AIR_ATTACK_GROUND(24)})`.

**Unit shooter** — one projectile **per live guy** (`UnitData::guy_mark`, `+0xB5`), launched
from that guy's exact position (`Guy + 0x0C/0x10/0x14`) plus `z + 100`. **Draws no RNG.**

**Building shooter** — `ObjectType::ammo_per_att` (`+0x208`) projectiles, each from a uniformly
random point over the footprint, at `obj.z + 250`:

```
w = objtype[+0x234] * 0x60          ; x_size, half-tile units
if (w - 1 < 1) rx = 0 else { rx = Random::get(0,0xFFFF); rx %= w; }
x = (obj.x ^ 0x63637) - w/2 + rx
```

**Exactly two `game_random` draws per projectile, x then y — and the draw is *skipped* when the
extent is `<= 1`.** So a 1×1 building consumes no RNG and a 2×3 building consumes 2 per round.
Stream position depends on building footprint size, which is the kind of detail that
desynchronises silently.

Object position fields are XOR-obfuscated with `0x63637` throughout; every read in the ammo
path is `stored ^ 0x63637`.

### Aircraft wrecks use a third constructor — and it is an arc

`Ammo::init_crash(Guy*, slot, graph_index)` (`0x0067B800`) mutates a recycled ammo slot for a
destroyed aircraft. Despite the earlier inventory label, it does **not** allocate or calculate a
`Spline`: retail writes `traj = TRAJ_ARC`, `v1z = 0`, `splash_area = 2`, and never touches the
existing `ammo_path` pointer. It likewise preserves the walked `accuracy` field and all flag bits
outside `0x1C`; these stale-slot semantics are checksum-visible and the port keeps them.

The endpoint solve is:

```
speed = UnitType.moves * Constants.unit_move_speed
fall_time = sqrtf((-2 * guy.z) / GRAVITY)
vx =  sinx(guy.angle, speed)
vy = -cosx(guy.angle, speed)
end = owner_object.xy + trunc((vx,vy) * fall_time)
end = clamp(end, (0,0), (world.xs*768-1, world.ys*768-1))
distance = sqrtf((end.x-guy.x)^2 + (end.y-guy.y)^2)
total_time = unsigned_max(trunc(distance / speed), 1)
dx = distance / total_time
```

The mixed coordinate bases are literal: `sx/sy/sz` and the distance origin are the individual
PDB `GuyData` coordinates, while the extrapolated endpoint is added to the owning Unit object's
XOR-decoded x/y. Flattening both to “the aircraft position” changes flight time and checksum.
The endpoint is clamped in four-tile `WCoord` dimensions before `TerrainOut::find_data_z`.

RNG is deliberately split. `rolling` consumes exactly one global
`game_random.get(0,0xFFFF)` draw and maps it through `% 7 - 3`. Then a temporary
`Random(guy.x + guy.y + guy.z)` supplies `bank_dx` followed by `bank_dy` from `[-30,30)`;
those two draws never advance the global stream. Finally, `start_roll_angle` is
`-trunc(guy[0].bank)` only for current orders `Strafe(16)`, `AirPatrol(17)`, and
`AirAttackGround(24)`; all other orders write zero.

`Objects::kill_guy` (`0x00659410`) owns the surrounding gate: the dying guy's PDB
`TypeData::cat` must be 8 and the owning type must not carry object-mask bit `0x08000000`. Once
admitted, retail claims the lowest `flags & 3 == 0` ammo slot, calls `init_crash` with the old
`Objects::ammo_index`, then increments that counter. `plan_ammo_crash` now performs that exact
gate and flattens the PDB Guy, owning Unit, lead-Guy bank, and both type records without accepting
an ammo or RNG reference; `ammo_spawn_crash` then closes the pool mutation.

The live `Sim::apply_damage` seam invokes the pair **before** clearing the Unit active bit and
filing its corpse. Its current whole-unit damage model can identify the retail `kill_guy`
casualty only when there is exactly one live Guy. For that shape, callers may install an exact
`CrashUnitSource`, `CrashTypeRule` records, and `CrashEnv`; the driver then mutates the lowest
free ammo slot and advances the real simulation RNG once. A missing Guy body, type record,
gpiece, conditionally required lead Guy, terrain provider, or inconsistent world dimensions
suppresses the wreck before pool, counter, RNG, or the port-only damage sidecar changes. Known
retail rejections (`cat != 8` or the mask bit) remain distinct from missing facts. Multi-Guy
casualty selection remains fail-closed until the damage path models which squad member died.

---

## 5. Flight, hit, and miss

### Accuracy attenuates per tile

```
tiles = dist / -192            ; MSVC magic multiply -0x2AAAAAAB, sar 5, sign fixup
acc   = to_hit + tiles*attenuate
if (acc < 5) acc = 5
```

`to_hit` is `ObjectType + 0x1EC`, `attenuate` is `+0x1F0`. So: **base accuracy minus one
`attenuate` per full tile of range, floored at 5, never 0.** `dist` is
`ObjectData::attack_dist` (`0x0064C880`) for a targeted shot and plain `vector_dist` for
attack-ground.

### Aim scatter

```
R = (constants[+0x38] * 100) / ((100 - acc)/5 + acc)      ; constants[+0x38] = target_radius = 96
if (acc > 100) R /= 4
ex += (R<=1 ? 0 : Random::get(0,0xFFFF) % R) - R/2
ey += (R<=1 ? 0 : Random::get(0,0xFFFF) % R) - R/2
```

acc 100 → R 96; acc 50 → R 160; acc 5 → R 400; acc 200 → R 13. Branch overrides, in priority
order: shooter `UnitType + 0x2B4 & 0x400000` → `R = 0`; a non-unit target or a target with
`domain != 0` → `R = 192` flat; aircraft shooter (`obj_masks & 0x8000000`) → `R *= 2` or `0`.

### `vector_dist` is an approximation, on purpose

`0x0046CFF0`, 105 bytes, **zero floating point**:

```
hi = max(|dx|,|dy|), lo = min(|dx|,|dy|)
if (hi == 0) return 0
if (lo >= 60000) return (2*hi + lo) >> 1
return hi + lo*lo / (2*hi)                 ; signed imul low-32, UNSIGNED divide
```

The first-order expansion of `sqrt(hi² + lo²)`. It over-estimates — `vector_dist(1000,1000)`
returns 1500 where the true distance is 1414. Every range check, hit test and splash falloff in
the game uses this, so "range 8" means 8 tiles of *this* metric. Reproducing `sqrt` here would
be wrong.

### `vector_dist` and the flight loop

`Ammo::inc_time`, `TRAJ_ARC` path:

```
cur_time++
if (o >= 0 && who >= 0 && !(shooter.flags & 1)) shooter.hold_frames++
if (!(flags & FLYING)) { if (cur_time < 200) return; close(); return }
if (cur_time >= total_time) goto arrival
if (bank_dx != 0 || bank_dy != 0) { ex += (int)bank_dx; ey += (int)bank_dy; restrict(); recompute dx }
t = cur_time; frac = t/total_time
nx = (ex-sx)*frac + sx ; ny = (ey-sy)*frac + sy
if (terrain_z(nx,ny) <= v1z*t + sz + G*0.5*t*t) return          ; still airborne
ex = nx; ey = ny; ez = terrain_z; total_time = cur_time + 1      ; CLIP INTO THE HILLSIDE
```

`total_time` is **rewritten** by the terrain clip rather than an early exit being taken, so the
`total_time` in the checksum is not the launch value once terrain intervenes. Note a
consequence the tests pin down: because `arc_ballistics` solves `v1z` so the parabola passes
through both endpoints under negative gravity, the arc is strictly *above* the chord — on flat
ground below both ends **the clip branch is unreachable**. It exists for rising terrain.

### Hit resolution

`Ammo::hit_target` (`0x00678F90`) — is the recorded target still where I aimed?

- building: **rectangle**, `|ex - t.x| <= x_size*0x60 && |ey - t.y| <= y_size*0x60`
- unit: **radius**, `vector_dist <= UnitType::target_size (+0x300)`
- `accuracy > 100` halves the measured distance, i.e. doubles the effective hit radius
- on failure it **mutates the projectile in place**: `whom = ox = -1`

That mutation matters for debugging a desync: a divergent hit test shows up on the `ammo`
channel one frame *before* it shows up on `units`.

`Ammo::check_hit` (`0x00678D90`) — did I land on anything at all? `find_unit` within `0x180`
(two tiles), rejected unless the distance is within the candidate's own `target_size`; failing
that, `find_building_at` on the impact tile. Tile index comes from a LUT at `DAT_00CAE5FC`
indexed by `coord >> 6` — a division by 192 expressed as a table over 64-unit cells.

### Miss behaviour, and the RNG asymmetry

`Ammo::do_damage`, single-target path, when nothing is left to hit:

```
ix = ex + Random::get(0,0xFFFF) % 0x29 - 0x14        ; +/- 20
iy = ey + Random::get(0,0xFFFF) % 0x29 - 0x14
restrict(); puncture_ground()
if (valid && !water) { flags = 1; cur_time = 0; return }     ; linger as a ground marker
```

**A hit draws nothing; a miss draws exactly twice.** Getting the hit test wrong therefore
desynchronises `game_random` for every later consumer in the frame, not just the projectile —
this is the single most dangerous divergence in the subsystem.

`flags = 1` is an **assignment**, not an OR: it clears `FLYING | OVERSHOOT | MISSED | NO_DAMAGE`
in one store while leaving the slot occupied. The spent marker then **stays in the ammo
checksum for 200 more frames** before `Ammo::close` frees it. A replay harness that expects the
ammo channel to return to idle the instant a shot lands will be wrong by 200 frames.

The `FLAG_OVERSHOOT` path (set for ground-unit targets) lets a round that missed keep flying —
extrapolating past the aim point, testing terrain each frame — until it lands or `cur_time`
exceeds `3 × total_time`, at which point the slot is closed with no damage.

---

## 6. The damage handoff — this is the `AMMO_PER_ATT` answer

`Ammo::do_damage` calls `Object::do_damage` (`0x0064A480`) with `this` = **the shooter**:

```
Object::do_damage(victim_o, victim_who, angle, num_guys, ammo_slot, scale256, secondary, 0)
```

recovered from the three call sites (`0x00678982`, `0x00678B0A`, `0x00678C6C`). `scale256` is
`0x100` for a direct hit and the splash falloff otherwise; `secondary` is 1 when the victim is
collateral; `ammo_slot` is the projectile's pool index, and **a negative value there means
"melee, do not split"**.

`Object::do_damage`'s head, `0x0064A49E`–`0x0064A7F1`:

```asm
cmp [ebp+0x1c], 0 ; jle <return>            ; scale <= 0 -> NOTHING HAPPENS AT ALL
call ObjectData::get_damage(o, who, angle, secondary, 1, &out)

; shooter is a Unit:
imul eax, scale                             ; D *= scale
cmp eax, 0x100 ; cmovle eax, 0x100          ; floor of 256 == one hit point
cmp ammo_slot, 0 ; jl <skip>                ; melee skips the split
idiv [shooter_objtype + 0x208]              ; / ammo_per_att
idiv [shooter_unittype + 0x308]             ; / uber_size
sar eax, 4 ; <keep low 4 bits> ; sar eax, 4 ; / 16 twice, remainder -> damage_frac

; shooter is a Build:
imul eax, scale ; idiv [objtype + 0x208]    ; no uber_size, NO floor of 256
sar eax, 4 ; <frac> ; sar eax, 4
```

Read as one expression with `scale = 256`:

| shooter | applied per projectile |
|---|---|
| unit | `max(D·scale, 256) / ammo_per_att / uber_size / 256` |
| building | `D·scale / ammo_per_att / 256` |

### Armor is applied *per projectile*, and that is the whole point

Armor is **not** touched by any of the above. It is subtracted inside `ObjectData::get_damage`
(`0x00644130`) at step 22 of 31, on the **raw, undivided, per-projectile** number — and each
projectile makes its own `get_damage` call. So a 4-round volley subtracts armor four times and
keeps a quarter of each result.

Algebraically that is identical to `(D − armor)/4` — *until the conditional floor of 1 inside
`get_damage` bites*, which is exactly when armor eats the round. At that point every projectile
independently bottoms out, and the many-small-projectiles attacker collapses against a
high-armor target far faster than a naive `(D − armor)/N` model predicts. This is the mechanism
behind heavy armour shrugging off massed light fire, and it is a structural consequence of
where the divide sits relative to the armor subtraction.

### The `/16` twice is not `/256` in disguise

The intermediate is truncated and its **low 4 bits are kept** as `ObjectData::damage_frac`
(`+0x3B`, a `char`) — sub-hit-point damage accumulates across shots instead of vanishing. The
port returns `SplitDamage { hits, frac16 }`; whoever owns `ObjectData` needs to accumulate the
`frac16` term. Flagged for the `units` lane.

### Squad casualties reduce damage mechanically

A unit fires one projectile per **live guy** but divides by **`uber_size`** (the full squad
size). A full squad delivers `D/ammo_per_att` in aggregate; a half-dead squad delivers half
that. Attrition is expressed through the projectile *count*, not through a damage multiplier.

### Splash falloff has separate building and unit arms

```
dx = max(0, |impact.x - victim.x| - victim.x_size*192)
dy = max(0, |impact.y - victim.y| - victim.y_size*192)
scale = 256 - (vector_dist(dx,dy) << 8) / (splash_area * 192)
```

Linear falloff measured from the victim's **bounding box**, not its centre, so large buildings
take full splash much further out. A negative result skips the victim rather than clamping.
Note the two extents genuinely differ: splash insets by `size*192` (a full tile per size unit),
`hit_target` by `size*96`.

That formula is the `is_live_build` (`vt+0x0C`) arm. Units use a radial inset instead:

```
distance = max(0, vector_dist(impact.x - unit.x, impact.y - unit.y)
                  - unit.block_radius - 192)
scale = 256 - (distance << 8) / (splash_area * 192)
```

The unit arm additionally requires `is_live_unit` (`vt+0x08`) and `is_on_map` (`vt+0xBC`),
keeps the candidate in the same air/non-air partition as the target domain, and rejects the
`0x08000000` missile object-mask bit. Buildings are rejected when the target domain is air.

One measured inconsistency worth recording rather than smoothing over: the two splash call
sites disagree by one instruction. `0x00678937` is `js` (skip only when negative — a victim at
exactly `scale == 0` still gets a call); `0x00678AC3` is `test ecx,ecx ; jle` (skips at zero
too). Observable damage is identical because `Object::do_damage` itself returns on `scale <= 0`,
but the *call count* differs, which matters to anything counting damage events. The port follows
the `js` form in the rectangular compatibility helper; the exact scan preserves `js` for builds
and `jle` for units.

The ring walk is now executable in `ammo_do_damage_splash_scan`. It converts the impact to a
four-tile `WCoord` (`div_3_table[coord >> 8]`), then uses the shipped offset tables
`DAT_00ADCAF0` / `DAT_00ADC400`. Each in-bounds `WData` head is followed through the complete
PDB-named `ObjectData::{down,down_who}` chain. There is no deduplication: if an object is linked
from two cells it is hit twice. Shooter self is skipped; the recorded primary bypasses
`LeaderData::is_enemy`; all other victims must be enemies and owners `0..7`.

The loop's endpoint is a retail bug/quirk worth preserving. `DAT_00ADD1E0[ring]` contains
`1,9,25,...,441`, but the back-edge at `0x00678B50` is `jle`, so the index is inclusive. Rings
1 through 9 also probe the first entry of the next ring. Ring 10 probes index 441, the bytes
immediately after the tables (`(0x00610076,120)`), which are out of bounds on retail maps.
The port reuses the byte-digest-pinned tables from `systems::collision` and preserves that
sentinel read.

For shooter unit types with flag `0x2000`, the scan centre is the truncating signed midpoint
between muzzle and impact; otherwise it is the impact. Every candidate identity is written to
`AmmoData::{whom,ox}` before filtering and the original primary is not restored. These writes
are mutation-tested because the fields live in the ammo checksum until the projectile closes.

`ammo_do_damage_splash_execute` closes the scan/application ordering gap. Its cursor captures a
node's `down` identity before issuing that node's `Object::do_damage` call, then re-enters the
world adapter before inspecting the captured successor. This is not equivalent to first
collecting a `Vec<DamageCall>`: an early hit can kill/remove the successor or change diplomacy,
and those mutations affect the remainder of the same projectile's walk. The focused mutation
test severs the current live link and kills the next node during the first callback; the cursor
still reaches that captured node, rejects its new dead state, follows its own link, dispatches
the tail, leaves the tail identity in `AmmoData::{whom,ox}`, and only then calls `Ammo::close`.
The function returns `None` without mutation for `splash_area <= 0`, preventing a caller from
accidentally closing a projectile that belongs to the distinct single-target branch. The splash
walk itself consumes no RNG; all ordering sensitivity here is world and checksum mutation order.

### Cruise and nuke splines are generated and walked, not approximated

`ammo_init_cruise_spline` reproduces the complete non-nuke constructor transaction. A recycled
`Spline` is cleared, its `flags` becomes `0x10`, and `calc_from_dir` installs start/control,
optional fourth control (only when its vector length is nonzero), and end. `set_min_seg_length`
stores the raw `u16` depth, while `calc_spline` temporarily clamps generation depth to
`control_verts.length * 4` and restores the raw value afterward. That distinction is visible:
the raw depth is checksummed even though the temporary depth determines sample count.

All six nested arrays begin exactly as `SplineData::SplineData` constructs them: length and
capacity zero, growth increment `-1`. Their first append therefore grows capacity to 4, followed
by 8, 16, 32—not the ordinary `SimpleArray` capacity 5 and not Rust `Vec` growth. The shipped
clamped-knot builder and recursive Cox–de Boor basis generate `spline_verts`; the cruise flag
makes `build_normals` use tangent vectors and its measured smoothing/final-normal rules. The
focused fixture pins control `(4,4)`, generated knots `(8,8)`, vertices `(17,32)`, normals
`(17,32)`, raw depth 95, total-length float bits, and complete checksum digests.

`AmmoPool::checksum_with_splines` walks the slot's non-null byte and its `RetailSpline`
immediately afterward. It returns `MissingSpline` rather than silently substituting an empty
path. `ammo_step_cruise_spline` is the isolated live arm after `cur_time` increments: it reads
`spline_verts[cur_time]` only while `length > cur_time + 1`, then applies three `cvttss2si`
coordinate truncations. The last sample is deliberately retained for the impact boundary. The
constructor and step consume no RNG.

`calc_nuke_spline` has two constructor arms and both feed that same generator. The fixed arm
installs the measured 12-point high arc, raw degree/depth `(3,120)`, and 17 custom widths
`[100,30,15,10…]`. The terrain arm truncates both endpoints before computing
`vector_dist / 768`, retains the original f32 endpoints for interpolation, and truncates each
interior sample before its terrain query. It lifts ordinary cells by 250, `flags & 0x30 == 0x30`
cells by 750, and cells with both flag bit 14 and low bits `3` by 1250; the 1250 arm has priority
when both predicates match. It then calls `set_min_seg_length(96.0)` and installs an all-30
knot-width array whose length and capacity are exactly `control_len + 2 + degree`.

`NukeSplineEnv` supplies the ordered height/flag facts without coupling the primitive to the
SoA world. Missing terrain fails before the caller's `Ammo` is changed, while successful
construction installs `TRAJ_SPLINE`, the non-null sidecar bit, and
`total_time = spline_verts.length - 1`. Neither nuke arm consumes RNG.

`AmmoPool` now owns a slot-aligned `Option<RetailSpline>` sidecar plus the retail LIFO recycler.
The recycler retains all six nested-array capacities: an invalid reconstruction returns the
same cleared allocation, while a later successful construction pops and reuses it. The selector
gives graphic-piece table flag 8 priority, then tests the shooter's `obj_masks & 0x08000000` nuke
bit, otherwise choosing the ordinary arc. `Sim::launch_ammo` installs fixed or terrain-aware
nuke paths through a live `NukeSplineEnv` adapter, step 15 samples the indexed vertex after the
ordinary time increment, close returns the path to the recycler before the slot is reused, and
the live ammo channel walks each non-null sidecar immediately after its owning `AmmoData`.

The live graphic-piece cruise arm now derives all four `calc_from_dir` vectors in the shipped
order. Start and end are the initialized ammo coordinates; the optional control begins as the
all-zero vector. The middle control begins `(0,-reach,0)`, rotates first around X using a fixed
45-degree land pitch or the truncated non-land lead-Guy pitch, then around Z using the negated
coarse `fast_angle_to_degrees` of the ammo angle, and only then adds the start coordinate. Both
rotations use retail's peculiar
negative half-degree normalization (`-1` maps to table index 358). Land reach is
`attack_dist / 2`; non-land unit reach is `proj_speed * unit_move_speed * 5`, with `0x480` for
the nuke-mask/other arm. Minimum segment length is `UnitData::speed()` for the nuke-mask arm,
otherwise unit projectile speed or the non-unit constant 150. Disassembly resolves a misleading
decompile here: the four future vector arguments are pushed before the zero-argument `speed()`
call and deliberately remain on the stack for the following `Spline::calc_from_dir` call.

---

## 7. Honest gaps

Ordered by how much they would cost a replay harness.

1. **Nothing here has been through the oracle.** Tier C across the board. The highest-value
   oracle cases, in order: `vector_dist` (`0x0046CFF0` — trivially callable, pure integer, two
   arguments, would go straight to Tier B), the accuracy magic-divide, `splash_scale`, and
   `split_damage`. `vector_dist` alone is used by half the combat code and is a ~20-line
   registry entry in `crates/oracle/src/registry.rs`.
2. **The complete launch path must call `ammo_init_targeted`.** Its anti-air gate is executable
   and mutation-tested, but the tick driver's current compatibility call still uses the older
   post-gate `ammo_init` adapter. Until that call site passes `UnitData::order_type()` and the
   target's recovered flight band, live air combat still bypasses the gate.
3. **The later dynamic cruise retarget/rebuild arm is not yet modeled.** Initial graphic-piece
   selection, exact orientation input derivation, pool ownership, construction, stepping,
   release, and checksum walking are live. The remaining spline seam is the branch inside
   `Ammo::inc_time` (`0x0067D380`) that detects a target leaving the sampled path envelope,
   mirrors the next spline vertex, rebuilds from the target's current position, copies the new
   vertex count into `total_time`, and resets `cur_time` to zero. Aircraft crashes are unrelated:
   `Ammo::init_crash` writes an ordinary arc and is executable. `Ammo::init` only ever writes
   `traj` 1 or 2 — `TRAJ_STRAIGHT` (0) is never set by `init`.
4. **The tick driver does not yet install a live `SplashDamageEnv`.** The exact interleaved
   executor is implemented and mutation-pinned in `ammo_do_damage_splash_execute`; the remaining
   seam is to expose the live WData/down-chain/diplomacy reads and `Object::do_damage` mutation
   callback from the world adapter. `ammo_do_damage_splash_scan` remains the non-mutating
   inspection form, while the older `ammo_do_damage_splash` compatibility helper still accepts
   a preselected victim list and does not have the exact scan's gates or mutations.
5. `Objects::ammo_index` — I have not established whether it is walked by `Objects::walk_data`
   (`0x006541E0`) and therefore whether it is on any channel. It monotonically increases and
   never rewinds, so if it *is* walked, save/load round-tripping must preserve it.
6. The exact `DataWalk` section-mask value `check_all` installs before the ammo channel is not
   confirmed; `AmmoData::walk_data` reads the *direction* field (`+4`), not the mask (`+0xC`),
   so it should not matter, but it is unverified.

---

## 8. Integration notes for the orchestrator

- `systems::ammo` and `systems::air` are both exported by the crate. The remaining immediate
  integration seam is the tick launch call described in §7.2; callers that have live order and
  flight-band state should use `ammo_init_targeted`, not the post-gate compatibility adapter.
- The module takes world access through the `AmmoEnv` trait (object lookup, terrain height,
  world bounds, unit/building search, water test), `SplashEnv` (WData heads, down-chain objects,
  diplomacy), `SplashDamageEnv` (synchronous `Object::do_damage` dispatch), `CrashEnv`
  (WCoord bounds and terrain height), and `NukeSplineEnv` (ordered terrain height/cell flags)
  rather than reaching into the SoA world. Whoever owns the world implements those adapters.
  The crash adapter is wired at the live death seam and activates
  only when its exact optional Guy/type/terrain sources are installed; it does not reuse
  `AmmoView`'s documented flat-terrain compatibility substitution.
- Single-target and compatibility splash paths still **emit** `DamageCall` values. The exact
  live splash path applies those same argument packets synchronously through `SplashDamageEnv`
  and returns an `Impact` audit log of calls already issued. Delaying those callbacks until after
  victim extraction is not retail-equivalent.
- For the `deaths` channel: projectile impacts resolve in `Objects::inc_time`, **after** every
  `Unit::process`, so a unit killed by a projectile dies at the end of the tick, later than one
  killed by melee. Death ordering within a tick is `Objects::process_all` deaths first, then
  ammo deaths in **pool-slot order** — which is the allocation order from
  `AmmoPool::alloc_slot`, i.e. lowest free slot first, not spawn order. Any death-ordering
  model that assumes spawn order will diverge after the first slot is recycled.
