# STRAFE executor frontier

Status: **source-only proof pack; strict order row 16 remains red**.  The isolated planner in
`crates/don-sim/src/systems/strafe_order_frontier.rs` freezes the complete concrete payload,
reachable `Unit::do_strafe` control-flow cones, nested air-physics RNG boundary, and an atomic
receipt contract.  It is intentionally not registered in `systems/mod.rs` and does not change
the executor table, save format, live tick, or closure ledger.

The planner directly reuses the landed `systems::patrol::StrafeOrder` and
`systems::air::AirOrderWalk`; it does not create a competing payload or save-state owner.

## Authority and extent

The authority is `ron-bin/riseofnations.exe` plus the matching `ron-bin/sbl/rise.pdb`, checked
against `schema/rise-symbols.tsv`, `schema/symbols.json`, `schema/types.json`, direct PE32
disassembly, and the mechanically generated `re/decomp-all` views.

| symbol | VA | bytes | role |
|---|---:|---:|---|
| `Unit::add_strafe_order` | `0x005E48C0` | 405 | concrete constructor/install path |
| `Unit::do_air_physics` | `0x005E86D0` | 1,794 | movement, collision, path, conditional RNG |
| `Unit::do_strafe` | `0x005EAB00` | 3,676 | one STRAFE activation |
| `Unit::find_new_bomber_target` | `0x005EB960` | 781 | bomber search host cone |
| `Unit::find_new_air_target` | `0x005EBC70` | 873 | ordinary air search host cone |

The function ends at `0x005EB95C`; `0x005EB960` is the next PDB procedure.  The planner's
`STRAFE_REACHABLE_CFG` records the semantic block heads from entry through both retarget tails.

## Concrete payload

`StrafeOrder` is 84 bytes.  It combines `AttackOrder@+0`, `AirOrder@+0x24`, the virtual
`UnitOrder@+0x4C`, and two final coordinates.  The six `AirOrder` words are checksum-visible and
cannot be collapsed into a generic target payload.

| concrete offset | field |
|---:|---|
| `+0x08/+0x0C/+0x10` | target object, owner, UID |
| `+0x14/+0x18` | `def_x`, `def_y` |
| `+0x1C..+0x20` | mandatory, defensive, in-range, ever-in-range, new-order bytes |
| `+0x28..+0x3C` | home object/owner, altitude, sharp-turn, old, returning |
| `+0x40/+0x44` | last target/saved flight position |
| `+0x50` | virtual-base order flags |

`StrafeOrder::walk_data` walks six concrete ranges totaling 57 bytes: the virtual flag byte,
target ten-byte identity, the 13 attack-state bytes, the virtual AirOrder flag, its flat 24-byte
payload, and the final eight coordinates.  Padding and vptrs are excluded.

## Reachable frame ordering

Every activation first obtains the mutable Strafe payload and calls `think_bird(order,0)`.
When `returning==0`, retail enters the target cone:

1. An allied live target is refreshed.  If its slot object is inactive, retail may replace the
   stored object index with `get_captain()` while preserving the owner.  A failed repair either
   returns to the saved `xx/yy` through a new AIR_PATROL or latches `returning=1` and clears the
   target pair.
2. The refreshed live target position is written to `xx/yy` before movement.  Helicopters can
   project a 0x30 lead point.  A specific active building/action-domain cone can instead project
   toward its action point by `(distance-0xC00)/3`; both projections are world-restricted.
3. `(actor.o + Game::frame) % 16 == 0` performs the first target search.  BOMBER (`0x130`) chooses
   `find_new_bomber_target` first; other aircraft choose `find_new_air_target`.  A valid hit
   inserts a new STRAFE at Queue-First with the complete hit identity, hit coordinates, and
   preserved home pair, then returns before physics. The coordinates are required because
   `StrafeOrder.xx/yy` are walked state; an identity-only insertion is not admissible.

The invalid-target cone is not a common kill:

- animals call `think_bird(order,1)` and conditionally `work()`;
- missiles die through the virtual death slot;
- helicopters kill the current order, then either work the remaining queue or install AIR_PATROL
  at their current coordinate;
- a queue with another order kills, idles, and works;
- otherwise valid saved `xx/yy` is converted to AIR_PATROL, or the existing order becomes a
  targetless returning sortie.

All surviving paths call `do_air_physics(order, aim_x, aim_y)`.  Zero returns immediately.
Nonzero enters queued-order leash, bearing/range, munition, and reacquisition cones.  Retail can
kill a sortie that is incompatible with the queued AIR_PATROL/STRAFE target, fire ammo, select
attack animation, self-destruct a missile, store virtual attack-result `AL+1` at actor `+0xAE`,
and add the bomber timing constant to `spell_time` in that exact order.

The second search is distinct: it is gated by
`(Game::frame + 2*actor.o) % 32 == 0`, derives an origin from the queued AIR_PATROL last waypoint
or queued STRAFE target, and uses the same bomber-versus-air search split.  A hit rewrites target
object, owner, UID, and clears `returning`; a miss reaches the common kill-and-idle tail.

## RNG and air-physics boundary

`Unit::do_strafe` contains **zero direct random calls**.  `find_new_air_target` and
`find_new_bomber_target` enumerate/scored candidates without drawing RNG.  However,
`Unit::do_air_physics` conditionally calls `Random::get(0,0xFFFF)` for cruise-altitude and
collision-turn choices.  It also mutates path, air-order, actor movement, animation, and possibly
the current order.

The proof therefore requires one atomic `AirPhysicsReceipt`: mutation digest, exact half-open
draw records, and before/after RNG epochs.  The frame planner rejects an out-of-range draw, an
epoch delta that differs from draw count, a physics result on a path that returned earlier, or a
post-physics cone paired with a zero result.  No fallback draw is synthesized.

## Mutation receipt

Preflight binds actor identity/version, order version, object-pool epoch, queue and path digests,
external-effect epoch, RNG epoch, the complete frame facts, and the exact ordered plan.  Commit
must compare that whole snapshot and recompute the plan.  Any target/captain change, payload
write, queue/path mutation, physics draw, search result, or effect reorder invalidates the
receipt and authorizes no partial publication.

Captain repair writes only the target pair and preserves the old UID, exactly as retail does at
`0x005EAC18`.  Missing-target return writes `returning=1` and clears only the target pair.  The
post-scan reacquire arm is different: it atomically writes target pair plus the live UID and then
sets `returning=0`.  Separate typed plan tokens prevent a production adapter from conflating the
three mutation shapes.

## Honest open tails and integration map

The ten `STRAFE_OPEN_TAILS` are host work, not missing reversal:

1. object captain and identity adapter;
2. leader alliance and target validity;
3. air-target search cone;
4. world projection/restriction;
5. atomic air-physics/RNG adapter;
6. queued-air-order leash;
7. range/bearing/ammo adapter;
8. missile death and animation effects;
9. concrete payload save/resume;
10. live-tick atomic commit.

Future coordinated integration must register the module, converge on the already-landed full
payload instead of creating a second representation, serialize all checksum-visible fields,
add an explicit dispatcher arm, publish the full receipt atomically, and prove save/load/resume
and replay checksum equivalence.  Until all of those land, the potential strict closure delta is
**0**.

## Validation boundary

Root convergence formatted the isolated files. The first remote run caught a fixture selecting
the bomber search for a non-bomber actor; the retail planner correctly refused that splice. After
the fixture was corrected to `AirFirst`, persvati release job
`strafe-order-v2-20260809T225917Z-96780-19521-71d773e61575` passed all 14 tests. Retail was not
run, and the strict closure delta remains zero.
