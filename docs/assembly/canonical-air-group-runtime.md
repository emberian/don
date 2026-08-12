# Canonical LaunchPatrol/Scramble runtime

This tranche mounts the recovered LaunchPatrol (opcode 11) and Scramble (opcode 36)
transaction on the real simulation owners for one bounded selection cone: opcode-0 selects
ordinary Unit-band carrier objects, and the aircraft are ordinary Unit objects in each
carrier's `inside_down` chain.

The production route is:

```text
exact [Group][11|36] bytes
  -> persisted CommandPackage selection cache
  -> fixed groups_guys::Groups slot
  -> Handle-bound containment/type/scenario snapshot
  -> checkpointed World/order/path/Group publish
  -> typed v13 AIR_PATROL tag 6
  -> canonical Unit::work row 17
  -> shared air physics + revision-bound Unit search
  -> typed queue-first STRAFE tag 8
  -> canonical Unit::work row 16 effect
```

Both package commands consume zero RNG. AIR_PATROL and STRAFE share one
`StrafeRuntimeAuthority`, including the type table, exact actor/frame search observations,
RNG epoch and external-effect epoch. The common air-physics adapter is therefore not copied
into a second runtime authority. AIR_PATROL prepares a detached order/path/Unit/RNG image,
revalidates the full World digest and authority, then publishes the after-image once.

## Retail comparison and the closure boundary

The executable bodies are still the authority for the contained-object walk and the two
install branches. Scramble installs AIR_PATROL above the selected member position for every
eligible non-helicopter child. LaunchPatrol consumes all six unaligned dwords and retains
its filter/force-all/single-best planner. Helicopters receive the recovered MOVE_TO image.

Replay evidence prevents an opcode closure claim here. All explicit recorded Scramble
Group packets in the audited corpus select Build-band airbases/carriers (objects 2015,
2090, 2091 and 2147), while the current canonical fixed-Group selector admits Unit-band
members only. In addition, an armed `ScenarioData::ignore_orders` prelude still requires an
atomic canonical prune transaction, and the general `UnitData::is_busy` answer requires
the CastOrder/SpecialAnim spell-type projection. This adapter refuses those cases before
mutation. The opcode 11/36 and Group action ledger rows therefore remain incomplete.

## Focused evidence

`canonical_air_group_save_resume` proves:

- Scramble packet -> fixed Group/cache -> typed AIR_PATROL, with no RNG draw;
- v13 save/load/resave and reinstalled external authority;
- the first resumed row-17 tick performs air physics and inserts typed STRAFE at queue first;
- the following row-16 tick executes the landed STRAFE runtime with identical loaded and
  uninterrupted World/order/path/RNG state;
- opcode 11 retains all six dwords and installs the expected relative patrol waypoint;
- armed ignore-orders blocks without Group/order/RNG mutation; and
- a changed canonical World between prepare and commit rejects without publishing the
  detached after-image.

No closure flag is changed by this tranche.
