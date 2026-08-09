# Live movement/collision driver

`systems::movement_driver` closes the transaction between the recovered move-step callback
sequence, detector, and deterministic detour/wait/repath resolver body:

1. `Unit::move_step` `0x005FAF30` emits a side-effecting proposed-step detection;
2. it may emit one read-only waypoint probe for the close-waypoint escape;
3. otherwise it invokes `Unit::resolve_unit_collision` `0x005F9D30` after the detector has
   persisted the blocker identity and proposed order destination.

The bridge maps those calls to `DetectArgs::MOVE_STEP` and `DetectArgs::DETOUR_PROBE`, applies
the detector mutation before returning `Hit`, and hands the same authoritative actor row to the
resolver.  The resolver receives the live path stack, game RNG, invalid-location query and
`find_upath` callback.  A repath snap is copied back to the integrator body.

## Mandatory host boundaries

The driver does not approximate object storage.  Every mutating event emits an `ActorCommit`
with before/after `UnitRow` images.  The host must make `after` authoritative before the next
event.  When coordinates differ, that means the full `Unit::set_new_location` transaction:
world-list relocation plus per-guy collision-stamp movement, not a coordinate-only write.

`CollisionPathHost` supplies the two virtual/external operations reached by the deterministic
resolver body:

- `UnitData::invalid_loc(tile_x, tile_y)` for local-detour candidates;
- `PathFinder::find_upath(path, quick)`, mapping both found and suspended retail non-zero
  results to `true`, and only failure to `false`.

Missing actor rows, stale actor/body coordinates, and out-of-order waypoint/resolve events fail
closed.  A detector failure returns `Hit`; a resolver failure returns `Unhandled`, retaining
`move_step`'s blocked/stuck fallback.

## Executed coverage and remaining boundary

Focused tests execute the full callback sequence through
`move_step_profile_with_collision`: bitmap hit, persisted blocker, same-owner wait resolution,
actor commit ordering, and zero RNG consumption.  A separate repath case pins UCoord-centre
snap writeback and the exact `quick` flag.

This is the exact collision transaction seam, not the whole 4,582-byte `Unit::do_move` wrapper.
Transport/coarse-route legs remain in the order-dispatch gap ledger. The production tick now
reaches this driver through `movement_live`: it resolves object identity against the generated
columns and registry, persists collision/order mutations, and relocates WData anchors and Guy
stamps atomically. Missing sources, moving multi-Guy formations, boats, and repath requests fail
closed at typed boundaries; see `docs/mechanics/movement-live.md`. The special captain/blocker
virtual prefix which precedes the resolver's recovered detour/wait/repath arms remains in the
collision lane's own gap ledger; the driver neither skips it by claim nor invents its effects.
