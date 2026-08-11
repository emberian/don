# Canonical group-air runtime integration

Status: typed authority and executable codec frozen; production integration and closure rows
remain red.

This tranche follows the already-landed atomic contract in
`systems/air_group_action_transaction.rs`.  It does not add another command-side object table.
It freezes the state which must move onto the canonical `Sim`, `World::Order`, and DoNSave owners
before `Group::action_launch_patrol`, `Group::action_scramble`, opcode 11, or opcode 36 can be
called complete.

## Independent retail specimens

The completed replay and fresh save are deliberately not joined:

| specimen | SHA-256 | GameInfo seed | evidence used here |
|---|---|---:|---|
| `Playback - 2026.08.11 11'44'38 (Tue).rcx` | `558e0cd53dbed4f820e8757c0327a58384d433e5beb8e9eeef5d64df4d67bd54` | `0x007f93e0` | exact Group/Scramble packet pairs |
| `new save game 2026.08.11 15'42'57 (Tue).SVX` | `161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7` | `0x014810ac` | contemporary v16 save/Groups structure; no extracted AIR_PATROL payload |

The different seeds prove these are different matches.  In particular, the save cannot answer an
empty Group packet or provide a missing air-order payload for the replay.

The replay decoder tiled all 60,402 packages / 68,811 commands.  All 84 Scramble commands have a
preceding Group in the same package.  Eighty-one carry an empty opcode-0 list and therefore depend
on the per-`play` cached `(o,uid)` selection.  The three explicit pairs are:

| frame / package | Group bytes | action | selected Build-band objects |
|---|---|---|---|
| 47,706 / 48,065 | `000100df07` | `24` | 2015 |
| 51,868 / 52,254 | `0002002a082b08` | `24` | 2090, 2091 |
| 57,968 / 58,402 | `0003002a082b086308` | `24` | 2090, 2091, 2147 |

The new fixture test decodes those exact bytes through the landed transaction decoder.  Opcode 11
does not occur in this replay; the fixture for it therefore proves the shipped 25-byte wire shape,
not occurrence in this specimen.

## Newly recovered AIR_PATROL walk ownership

The PDB gives these layouts:

| type | size | relevant layout |
|---|---:|---|
| `SimpleArray<Coord>` | 28 | vptr; `length@4`, `size@8`, `increment@12`, `list@16`, `flags@20`, `cur_index@24` |
| `PatrolOrder` | 76 | x-array `+4`, y-array `+32`, `waypoint +60`, virtual `UnitOrder` |
| `AirOrder` | 40 | six walked dwords `+4..+28`, virtual `UnitOrder` |
| `AirPatrolOrder` | 104 | `PatrolOrder +0`, secondary `AirOrder +64`, shared virtual `UnitOrder` |

`AirPatrolOrder::walk_data` `0x00483ED0` is 114 bytes of code.  In emitted call order it walks:

1. `UnitOrder::flags`, reached through the primary `PatrolOrder` base;
2. `PatrolOrder::waypoint`;
3. x `SimpleArray<Coord>` through helper `0x00483CC0`;
4. y `SimpleArray<Coord>` through the same helper;
5. the shared `UnitOrder::flags` again, reached through the secondary `AirOrder` base;
6. the flat 24-byte `AirOrder` scalar range.

The array helper is not just `len + values`.  It walks `length:i32`, `increment:i16`, a flags byte
with bit `0x40` cleared, then exactly `length` `Coord` values.  It omits capacity, pointer and
`cur_index`.  `PatrolOrder::PatrolOrder` `0x00483720` initializes both increments to `-1` and both
flags to zero, but load/save state still owns these fields.  The existing `PatrolPoints { x: Vec,
y: Vec, waypoint }` loses them and therefore cannot yet be a checksum- or save-complete production
payload.

`air_runtime_authority.rs` preserves both dynamic arrays and their walked metadata, the waypoint,
and all six `AirOrder` dwords.  Its DoNSave leaf uses reserved tag 6/version 1 and rejects:

- unequal or empty coordinate arrays;
- unknown tag/version, truncation, or trailing bytes;
- allocator flag `0x40` in a supposedly normalized persisted image;
- incoherent home addresses;
- a decoded point count above the stream resource limit before allocation.

That point limit protects the decoder only.  It is not a gameplay cap, and the runtime type remains
dynamically sized.

## Canonical type and scenario authority

The launch receivers read four independent synchronized type answers:

- `ObjectTypeData::domain +0x218`;
- `ObjectTypeData::obj_masks +0x1E4`;
- `UnitTypeData::unit_flags +0x2B4`;
- non-strict `ObjectData::is(BIPLANE=0x11F/BOMBER=0x130/HELICOPTER=0x136, 0)`.

A raw type id cannot reproduce the last three hierarchy answers.  The new
`AirTypeAuthority` is revision- and composition-digest-bound and rejects missing or duplicate type
rows.  It supplies no defaults.  `domain == 2`, the missile bit `0x08000000`, and helicopter flag
`0x20` remain distinct predicates.

The launch-patrol entry begins with `ScenarioData::ignore_orders`.  When armed, retail visits the
owner's ordered `objects_ignoring_orders` list and runs the already recovered `Group::kill` plan.
Duplicates and negative tombstones are observable input and are retained.  The new scenario
authority owns the scalar plus all eight lists; its codec deliberately does not persist the
process-local revision.  A production transaction must plan and commit the resulting Group and
object-backlink changes in the same checkpoint as the air-order installs.

## Exact shared hook request

The next merge should be one coordinated shared-owner change after Group-to-Move's DoNSave v13 and
order metrics are frozen.  The required hooks are:

1. **`order.rs`** — add the dynamic AIR_PATROL payload to canonical `Order`.  `Order` can no longer
   be `Copy`; callers which snapshot queue nodes must clone.  Preserve both walked array metadata
   records, not only `Vec<i32>` coordinates.
2. **`order_dispatch.rs`** — make `Order <-> OrderRec` preserve `PatrolPayload::Air` losslessly.
   Today `adopt` constructs no patrol payload and `publish` drops it.  Kind/payload mismatches must
   be malformed, not zero-initialized.
3. **`save_load.rs`** — implement reserved tag 6/version 1 with a pre-allocation point-count bound,
   deterministic resave, and tag/version/length mutation rejection.  Add scenario scalar/lists to
   a typed root section or the canonical scenario section; do not reconstruct them as clear.
4. **`tick::Sim`** — install revisioned `AirTypeAuthority` and canonical scenario authority; expose
   the opcode 0 + 11/36 package route beside the Group-to-Move route.  Explicit and cached Group
   selection must share the existing per-`play` cache and fixed 512-slot `Groups` pool.
5. **canonical object adapter** — resolve selected Unit/Build addresses and every
   `inside_down/inside_down_who` link through `World::object_bands`, retaining Unit Handles and
   Build-row identity.  A stale generation or containment link aborts before mutation.
6. **commit** — apply the recovered AIR_PATROL and helicopter MOVE_TO branches as complete queue,
   mask, action, and path after-images.  The old `ProductionRuntime::air_patrol_orders` and command
   `ObjectTable` air state must not remain competing gameplay owners.
7. **`Sim::unit_work`** — dispatch AIR_PATROL through the existing recovered executor with the real
   air-physics and target-search hosts.  A dispatch counter or a host-unavailable no-op is not an
   observable execution proof.

## Gates before four closure flips

All four rows stay red until one production integration suite proves:

- exact replay Group/Scramble packets and exact opcode-11 bytes enter the canonical package route;
- explicit Build selection and cached reselection resolve the intended fixed Group;
- clear `ignore_orders` is a proven no-op, while armed state commits exact ordered pruning or rolls
  back;
- type classification, containment, target order revisions and stable identities are revalidated
  immediately before commit;
- mixed AIR_PATROL/MOVE installs are atomic and consume no RNG on every refusal;
- AIR_PATROL reaches `Sim::unit_work` and makes an observable canonical physics/waypoint/action/path
  transition;
- save, load and immediate resave preserve queue order, both array metadata records, every
  waypoint, waypoint cursor, six AirOrder dwords, Group/cache state and scenario state;
- the first post-load tick matches the unsaved branch in state digest, receipt and RNG state;
- packet, UID/Handle/generation, group walk, type/scenario revision, tag/version, array count,
  payload byte, and save truncation mutations all reject with zero partial change.

Until those gates pass, this file is integration-ready evidence, not closure evidence.
