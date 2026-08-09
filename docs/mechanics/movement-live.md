# Live movement collision store

The executable tick no longer answers `Unit::detect_unit_collision` with an empty-map
boolean. A unit can enter the recovered detect/resolve transaction after the host installs a
complete `LiveCollisionSource`. The adapter then reads and writes the generated `UnitCols`,
resolves `(who, o)` through `ObjectRegistry`, persists collision order fields, and uses the same
simulation RNG as the enclosing movement step.

## Installation contract

Installation is atomic and fail-closed. Before mutating the world it validates the handle, active
and on-map state, domain and collision radii, the exact `guy_mark`, every non-null Guy location,
and the unit anchor. A successful install:

- links the unit into the correct `WData` object chain;
- initializes the generated no-collision sentinels (`collide_o = -1`, `collide_who = -1`);
- stamps every supplied live Guy into the terrain collision bitmap; and
- keeps exact non-column type, action, body, and invalid-location facts beside the generated row.

Every active generated unit must have a source and path before a live actor moves. Missing or
inconsistent facts hold the actor in place and charge the named collision gap; they never become
"no blocker" answers.

## Spatial commit

Detector/resolver writes use `ActorCommit`, which rejects stale snapshots. A position-changing
commit first proves the old `WData` splice, then relocates the unit anchor, moves the Guy footprint,
updates the installed Guy location, and finally writes generated collision counters. The enclosing
successful `move_step` uses the same transaction for its ordinary translation and facing write, so
the next unit in object traversal observes the new spatial state in the same frame.

The moving-actor surface is currently exact for air or ordinary-land actors with exactly one live
Guy. Multi-Guy units are installed as exact blockers, but attempting to move one fails closed until
the full formation-producing `Unit::set_new_location` body is attached. Boat collision similarly
remains behind an explicit typed fault.

## Remaining host boundary

Local detour resolution executes and persists `step_dest`, detour, wait, retry, collision identity,
and collision counters. A resolver branch that needs `find_upath` stops at
`MissingRepathHost`: the compact tick does not yet park and restore the retail pathfinder's mutable
containers per unit. This boundary preserves every deterministic mutation that precedes repathing,
but does not claim a fabricated path.
