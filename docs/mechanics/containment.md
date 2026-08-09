# Retail containment and collision-valid release

This boundary covers deterministic nearby placement, the no-placement release of an
ordinary gatherer, and the identity-link work needed before a genuinely contained Scholar
can be released without a fabricated tile offset. It is implemented in
`crates/don-sim/src/systems/containment.rs`.

## Ordinary gatherers do not come out

Farm, Woodcutter/Camp and Mine Citizens remain on-map and collidable while gathering in
retail. `Unit::do_gather` `0x005EF2A0` and `Unit::do_non_flat_gather` `0x005F0170` attach and
move them without `go_inside`; releasing them detaches the gather chain/order while
preserving the current anchor, WData world link, guy positions and collision bitmap stamp.

`ordinary_gather_release_in_place` proves that collision-side invariant and returns the
unchanged point. It rejects an ordinary worker whose collision row was marked off-map. That
is a model defect, not permission to run a nearby search and teleport the worker out of an
invented seat. Scholar TypeIndexes `0x34` and `0x35` are rejected from this no-op path because
they are the actual gather containment profile.

## Recovered placement arm

`UnitType::find_nearby_spot` at `0x0061DE70` takes a centre, inclusive minimum/maximum
radius, radial step, binary angle, filter, actor identity, three mode fields, an optional
overlap object and an optional region. It returns zero on success and consumes no RNG.

The supported boundary is the land, non-expanded Gather arm (`FilterIndex == 3`; filter 5
has the same collision selection). The probe sequence is:

1. resolve an automatic maximum as `min + big_radius*4`; when `big_radius == 0` and unit
   flag `0x10` is clear, use `min + 0x240` instead;
2. resolve an automatic step as `max(1, (max-min)/8)`;
3. search every inclusive radius, using phases `0,+1,-1,...,+15,-15` for non-zero radii;
4. project with retail `sinx`/`cosx` and snap each axis to a UCoord centre;
5. reject out-of-bounds points, a mismatched required region, `TData::BLOCKED`, or water;
6. call `Objects::find_collision` `0x0065B1B0`;
7. only when that is clear, call `Objects::find_ordered_collision` `0x0065B440`.

For a land actor and `find_collision` mode zero, step 6 is directly
`CollCheck::collide_here` `0x00682540`. The API consequently requires a concrete synchronized
`map_terrain::World`, `CollCheck`, and `CollUnits` view. Step 7 is a separate mandatory
`OrderedCollision` adapter because its tail reads ordered unit coordinates and army
membership not present in the occupancy bitmap. Missing either view is not represented by a
clear result.

Air, sea, expanded-formation, and the TypeIndex `0x32/0x33` optional overlap-footprint arm
remain rejected at this entry point. They must be ported from their own branches rather than
inheriting land behavior.

## Exact inside-link splice

Retail containment is one bidirectional chain of owner-local `(who, o)` identities:

| Field | Offset | Meaning |
|---|---:|---|
| `ObjectData::inside_down` | `+0x28` | next contained object index |
| `ObjectData::inside_down_who` | `+0x3E` | owner of `inside_down` |
| `UnitData::inside_up` | `+0x82` | immediate container index / on-map sentinel |
| `UnitData::inside_up_who` | `+0xB4` | owner of `inside_up` |

`Object::insert_inside` `0x00647E90` removes the entering object from the world, follows
`inside_down` to its bottom object, and links that bottom to the requested container.
`Object::remove_from_inside` `0x006480F0` performs the inverse splice:

1. the direct parent adopts the removed object's `inside_down` identity;
2. when that nested child exists, its `inside_up` identity becomes the direct parent;
3. the removed object's `inside_up` and `inside_down` indices become `-1`;
4. the removed object is added to the world.

The two owner bytes on the removed object are deliberately not cleared; retail leaves their
stale values after clearing the signed indices. The port validates every forward/back link
and the complete outer-parent walk before the first mutation, then rechecks the planned rows
at commit time. Broken or changed chains fail with no partial writes.

## Scholar come-out transaction boundary

`plan_scholar_come_out` accepts only TypeIndex `0x34`/`0x35` and explicit mode zero. It first
completes the collision-valid search, then prepares the link splice. The caller supplies the
full nearby tuple because the common `come_out` ordering is recovered but the complete
Scholar-specific branch of the 9,925-byte routine is not yet reduced to one constructor. If
no point is valid, the function returns `Blocked` and the inside table is untouched. A ready
plan contains the exact snapped point, search-call counts, and the validated splice.

The remaining `Unit::come_out` work is intentionally explicit. Retail writes the selected
anchor, calls `remove_from_inside`, then calls `Unit::set_new_location`, which relocates the
world link and every live guy using the unit's formation state. The containment module does
not fabricate guy offsets or claim that changing only an Arena entity centre completes that
transaction. An integration must apply the plan together with the authoritative unit-location
owner; until that adapter is present, a ready plan is not itself an on-map unit.

The remaining factual gaps are the Scholar branch's conditional RNG-tail reachability,
several statistics/dirty counters after unlinking, and the other recursive/type/mode profiles
inside `Unit::come_out`. None is represented by a permissive default.
