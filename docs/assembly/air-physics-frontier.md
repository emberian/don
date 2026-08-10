# `Unit::do_air_physics` transaction frontier

Status: **registered executable proof pack; RL air physics remains red**. The planner in
`crates/don-sim/src/systems/air_physics_frontier.rs` freezes the complete top-level control
flow and state-write order. Registration makes it available to product adapters; no live tick
or complete `don-env` adapter is claimed by this pack.

## Authority and extent

The only behavioral authority used here is the supported `ron-bin/riseofnations.exe` and its
matching `ron-bin/sbl/rise.pdb`. `schema/symbols.json` gives the exact PDB signature
`int Unit::do_air_physics(UnitOrder*, Coord, Coord)`, source lines 24548–24780 of `unit.cpp`,
VA `0x005E86D0`, and size 1,794. Direct PE32 disassembly ends at `0x005E8DD2`; the three return
sites are the fuel/landing stop at `0x005E8959`, kill-current stop at `0x005E8C76`, and success
at `0x005E8DA9`. `re/decomp-all/005e86d0.c` is used only as a mechanically generated companion
view.

The PDB fixes the fields that would otherwise be guesses:

| object | field | offset |
|---|---|---:|
| `SubObjectData` | `who`, `o`, `x_internal`, `y_internal`, `ptype` | `+9`, `+10`, `+16`, `+20`, `+24` |
| `UnitData` | `angle`, `unit_masks`, `path`, `orderlist`, `guys` | `+80`, `+104`, `+184`, `+200`, `+228` |
| `AirOrder` | `oxx`, `whose`, `cruising_alt`, `sharp_turn`, `old`, `returning` | `+4..+24` |
| `PathData` | `to_x`, `to_y`, `tolerance`, `flags` | `+0`, `+4`, `+8`, `+12` |
| `UnitTypeData` | `unit_flags` | `+0x2B4` |
| `GuyData` | `z`, `angle`, `bank`, `pitch` | `+20`, `+24`, `+68`, `+76` |

The direct callees are independently named by the PDB: `check_fuel` `0x005E9BE0`,
`land_plane` `0x005E9950`, `bank_aircraft` `0x005E9520`, `pitch_aircraft` `0x005E8DE0`,
`Guy::set_angle` `0x005D9010`, `UnitData::invalid_loc` `0x00607C30`,
`WorldData::restrict` `0x006B53A0`, `Unit::set_new_location` `0x005F8D20`,
`Unit::set_anim` `0x00616F40`, and `Unit::kill_current_order` `0x005E2CB0`.

## Exact top-level transaction

The shipped ordering is:

1. Obtain `AirOrder` through `UnitOrder::update_air_order`. Every eighth actor-phase frame,
   choose cruising altitude. The random arm is exactly `!is(BOMBER,0) &&
   (!is_animal || type==0x193) && !(unit_flags&0x20)` and draws
   `Random::get(0,0xFFFF)` before writing `(draw%7+13)*100`. The other arm writes `0x640`,
   adjusted by `+200` for animal type `0x192` and `-200` for every other animal.
2. Set `UnitData::path.length=0`. A non-animal then calls `check_fuel`; nonzero returns zero
   immediately. A continuing call may rewrite the aim, altitude goal, home pair, and returning
   state. Type `0x193` clears recharge and, with queue length one, sets returning and takes the
   terrain height at the actor cell as its altitude goal.
3. Clamp the aim to `[0,width*0x300-1] × [0,height*0x300-1]`, then obtain virtual speed and
   multiply it by `ai_speed` only when that global is greater than one. A returning aircraft
   lands when Manhattan distance is below `3*speed/2`, the aim was not clamped, and a helicopter's
   first Guy has `z-altitude_goal < 0x96`. This returns zero before appending a path point.
4. A returning nonnegative home pair whose object is active is marked with raw mask `0x100000`
   when the same aim-distance is below `0xC00`. Append one exact `PathData {aim_x,aim_y,0,0}`.
5. On a non-returning non-helicopter, an active AIR-domain current target accepted by
   `valid_target` sets the bank alignment bit only when it is inside `min_range*0xC0` and within
   a quarter turn. Desired bearing comes from `find_angle`. A turn over 45 degrees is suppressed
   inside `min_range*0xC0+0x300` (or just `0x300` while returning); nonzero `sharp_turn` finally
   overrides it by `sharp_turn*90°`.
6. Call `bank_aircraft`, then `pitch_aircraft`, then `Guy::set_angle(actor.angle,1)`. These
   calls may change actor angle and the by-reference speed, so later projection must use their
   observed outputs.
7. Non-helicopters always enter collision. Helicopters enter it only inside `0xC0` outbound or
   `0x30` returning; if another order is queued, that close arm kills the current order and
   returns zero. Collision projects one speed step, calls `invalid_loc`, and either clears
   `sharp_turn` or restricts the point. An invalid point with zero `sharp_turn` makes the second
   and only other direct RNG draw, setting `sharp_turn` to `+1` for an odd result and `-1` for an
   even result. `set_new_location(x,y,0,1)` follows; its integer return is ignored.
8. Zero recharge calls `set_anim(8,0,1)`. Type `0x193` then clears returning, stores recharge
   one, and kills the current order with return zero when its post-location Manhattan distance
   is below `0x30` and another order exists. Every other surviving path returns one.

`do_air_physics` has exactly two direct `Random::get` sites, at `0x005E8785` and
`0x005E8D04`. Neither the body nor its directly called `check_fuel`, `bank_aircraft`,
`pitch_aircraft`, or `land_plane` generated views contain another `Random::get` call.

## Minimal executable transaction

`plan_air_physics` accepts one versioned actor/order/path/queue/object/terrain/animation/effect/RNG
snapshot plus branch-conditional host facts. It emits the exact ordered top-level steps and an
integer-return disposition. Every reached nested mutation carries the same single-use transaction
token and a mutation digest. RNG draws must use the exact half-open `[0,0xFFFF)` request and the
plan derives the after epoch from the exact draw count.

An adapter must checkpoint every represented owner, execute the plan, revalidate the complete
snapshot immediately before publication, and publish all steps atomically. A failed nested call,
receipt mismatch, late capability failure, or epoch mismatch authorizes no partial actor, order,
path, object, Guy, terrain, animation, external-effect, or RNG mutation. The supplied
`AirPhysicsCommitReceipt` makes the minimum success condition executable; it deliberately does
not pretend to implement rollback for an adapter.

## External facts and honest `don-env` audit

| shipped read/effect | required authority | does `EnvWorld` genuinely supply it now? |
|---|---|---|
| actor `(who,o,uid)`, position, angle, type id, frame | live object columns | **yes** |
| all six `AirOrder` words and queue length | concrete AIR_PATROL payload/queue | **yes** for AIR_PATROL |
| map dimensions and clamp | initialized environment map extent | **yes** |
| main `Random` stream | `World::random`, not `EnvWorld`'s scaffold RNG | **stored yes; no atomic air transaction exposes it** |
| `is(BOMBER,0)` including upgrade membership | virtual type predicate | **no**; delegated to `AirPatrolHost::actor_is_type` |
| `is_animal` virtual and animal `think_bird` effects | subclass identity/override | **no**; the local contiguous-range test is not the virtual, and `think_bird` is a host call |
| raw `UnitTypeData::unit_flags & 0x20` | exact runtime type record | **yes statically**; `Rules::air_type_data` now loads the captured postload row, but no complete physics host consumes it yet |
| virtual `get_speed`, runtime `ai_speed` | actor/type/player/runtime modifiers | **no**; the environment's `speed` column is a compact move rate and has no `ai_speed` owner |
| `check_fuel` | fuel/mana, home object scans, containment/death/effects, aim and order rewrites | **no** |
| terrain height / `div_3_table` | authoritative terrain | **no** |
| mutable `UnitData::path` plus allocator semantics | checksum-visible path stack | **no** in `EnvWorld`; its order queue is a different object |
| home object active lookup and raw unit-mask mark | complete object pool/class layout | **partial only**; unit identity exists, but the generic retail object/class mutation does not |
| current order `is_air/get_target_order`, target active/domain, `valid_target`, `min_range` | typed order and full object/type/diplomacy target adapter | **partial only**; queue/typecap data exist, virtual validation does not |
| first `Guy` z/bank/pitch/angle and `Guy::set_angle` | materialized Guy array | **no** |
| `bank_aircraft` and `pitch_aircraft` | full Guy/type/constants flight state | **no** |
| `invalid_loc` and `WorldData::restrict` | terrain/collision/world occupancy | **clamp only**; collision authority is absent |
| `set_new_location` | occupancy, visibility, collision, and position transaction | **no**; a scalar position write is not equivalent |
| `land_plane` | containment/home/order/Guy/effect transaction | **no** |
| `set_anim(8,0,1)` | animation/Guy state | **no** |
| `kill_current_order(0)` | exact queue retirement plus nested side effects | **queue storage exists; full helper is not proven** |

The existing `AirPatrolHost::do_air_physics` callback therefore names the right coarse boundary
but does not itself establish fidelity. `frame_with_air_patrol_host` clones and restores
`EnvWorld` on an error, yet it cannot roll back state owned inside a host, and the current callback
returns only `bool`; it carries no actor/order/path/object/Guy/RNG receipt. A production integration
must replace that coarse promise with an atomic receipt at least as strong as this frontier before
the `env-air-patrol-physics` deviation can turn green.

## Validation boundary

The integration test imports the source file directly. It freezes the PDB extent,
ordinary success ordering, cruise-before-fuel RNG order, returning landing early exit, conditional
collision RNG, helicopter kill ordering, the type-`0x193` terrain/tail pair, fail-closed missing
facts and cross-transaction receipts, and whole-plan commit binding. No retail oracle run was made;
the evidence tier remains static executable/PDB reconstruction only.
