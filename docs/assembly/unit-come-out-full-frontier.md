# `Unit::come_out(int)` full-body frontier

Status: measured prefix, source-only transaction plan; not integrated and not a full-function
closure.

## Provenance and boundary

This tranche was recovered from the shipped PE32 image
`ron-bin/riseofnations.exe` (SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`, image base
`0x00400000`) with its matching shipped `ron-bin/sbl/rise.pdb` (GUID
`51D4F219-61C6-4F84-9D5B-C3361B0D291F`, age 1). The PDB identifies
`int Unit::come_out(int)` at `0x00617C10`, size 9,925 (`0x26C5`) bytes, ending at
`0x0061A2D5`. Binary CFG recovery gives 348 basic blocks and cyclomatic complexity 223.

The bounded tranche is `0x00617C10..0x006186B3`, 2,724 (`0xAA4`) bytes. It stops at
`0x006186B4`, where the common release path begins pushing the arguments for
`Unit::set_new_location(x, y, 1, 1)` (call at `0x006186BC`). The typed continuation therefore
leaves 7,201 bytes, `0x006186B4..0x0061A2D4`, still unrecovered by this owner.

The model is isolated in
`crates/don-sim/src/systems/unit_come_out_full_frontier.rs`. It consumes explicit world/host
facts and returns either the retail early result or
`ContinueAtCommonRelease { resume_va: 0x006186B4, ... }`. It does not publish simulation
writes by itself.

## Control flow and writes

| Retail VA | Recovered operation |
|---|---|
| `0x00617C66` | `Group::clear(-1)` on the function-local scratch group |
| `0x00617C8F` | set every Guy's `anim_index_hints.length` (`Guy +0xD0`) to zero |
| `0x00617CF7` | for argument zero and a non-captain, call same-owner captain `come_out(0)` and return its exact result |
| `0x00617D14` | resolve the direct containing object with `ObjectData::get_inside(&who)` |
| `0x00617D82`, `0x00617DD1` | uncontained strict/permissive placement probes |
| `0x00617EED` | Oil Platform land-unit bridge: allocate the current Transport Barge upgrade, falling back to type `0x140` |
| `0x00617F28` | copy actor damage to the new transport with `Unit::same_damage` |
| `0x00617F41` | temporarily insert the new transport inside the actor |
| `0x00617F52` | recursively call the transport's `come_out(0)` |
| `0x00617F84` | on recursive failure, dispatch transport death and return 1 |
| `0x00617F99`, `0x00617FA8` | on success, remove actor from Oil Platform, insert actor in transport, return 0 |
| `0x00618003` | direct-location object mask `0x08000000`: write `90.0f` bits (`0x42B40000`) to first Guy `turret_inc` (`Guy +0x40`) |
| `0x006180EA` | University (`0x1A4`) or Oil Platform (`0x1A6`) build exit: owner leader flags OR `0x02000000` |
| `0x006181F6` | captain using an object-backed gather point with owner below 8: copy hot-key groups from that object |
| `0x0061827F`, `0x00618314` | zero/nonzero actor-block-radius gather-direction probes |
| `0x006182E3`, `0x00618374` | store exact `find_angle(found - container)` result into the placement container angle |
| `0x006184EE`, `0x0061855C` | zero actor-block-radius strict/permissive final placement probes |
| `0x006185DE`, `0x00618642` | nonzero actor-block-radius strict/permissive final placement probes |
| `0x00618667`, `0x00618675` | store selected actor X/Y |
| `0x00618694`, `0x006186A4` | call `TerrainOut::find_tcoord_z` and store actor Z |
| `0x006186A7` | remove actor from its containing object |
| `0x006186B4` | typed continuation into the common release tail |

Entry cleanup precedes every exit, including captain redirection: the scratch group is cleared
and every Guy hint length is zeroed before the recursive captain call. A successful uncontained
search carries only X/Y to the continuation; it performs no unlink or actor-coordinate store in
this prefix. A successful contained search stores X/Y/Z and unlinks before continuing.

The direct targets used to name these operations are also fixed in this image:

| Target VA | Shipped-PDB identity |
|---|---|
| `0x00713E80` | `Group::clear` |
| `0x00651A80` | `ObjectData::get_inside` |
| `0x0061DE70` | `UnitType::find_nearby_spot` |
| `0x006E3140` | `LeaderData::current_upgrade` |
| `0x0065E0C0` | `Objects::init_unit` |
| `0x005F9400` | `Unit::same_damage` |
| `0x00647E90` | `Object::insert_inside` |
| `0x00647080` | base `Object::die` fast target; the call remains virtual when overridden |
| `0x006480F0` | `Object::remove_from_inside` |
| `0x0046F180` | `BuildData::gather_inside` |
| `0x0046F0C0` | gather-list first-node accessor |
| `0x00714FD0` | `HotKeyGroups::copy` |
| `0x0092D130` | `find_angle` |
| `0x008544A0` | `TerrainOut::find_tcoord_z` |
| `0x005F8D20` | `Unit::set_new_location` (first residual call) |

### Oil Platform bridge

The special branch requires the direct container to match Oil Platform and actor domain to be
land (`0`). `LeaderData::current_upgrade(0x140)` selects the transport type; a negative result
falls back to `0x140`. Allocation failure returns 1. After allocation, the ordering is fixed:

1. `Objects::init_unit`;
2. `same_damage`;
3. insert the transport inside the land actor;
4. recursive transport `come_out(0)`;
5. either transport death and return 1, or remove actor from the platform, insert actor in the
   transport, and return 0.

The typed child receipts make this ordering and their RNG continuity reviewable without claiming
that these transitive callees are closed here.

### Container selection and gather point

The direct-location-mask branch uses the direct container immediately. Otherwise the placement
container is the direct container for a captain, or the same-owner `get_captain()` object for a
non-captain. Its `gpiece` is carried across the boundary on this general branch. The uncontained
and direct-location-mask branches skip that virtual call and retain the local's initialized zero.

A build container reaches its first gather-list node only when the list is non-empty and
`BuildData::gather_inside()` returns zero. Gather action 3 treats the node X/Y fields as an object
identity and obtains coordinates from that object; other actions use literal coordinates. The
probe's returned point is converted by retail `find_angle` at `0x0092D130`. The planner requires
that authoritative integer-angle result as a host fact. It rejects a found point without the
angle rather than approximating the retail trig convention.

## Exact nearby-search tuples

All calls below use actor UnitType as receiver, radial step 0, actor identity as the collision
object, overlap object `-1`, overlap owner `0`, and required region `-1`.

| Branch/call | Centre | Minimum / maximum | Base angle | Filter | Accept despite collision | Expanded |
|---|---|---|---:|---:|---:|---:|
| uncontained `0x00617D82` | actor point | actor `block_radius`; `Constants +0x9C + min` | `0x80000000` | 3 | 0 | 0 |
| uncontained retry `0x00617DD1` | actor point | unchanged | `0x80000000` | 3 | 1 | 0 |
| gather, zero block `0x0061827F` | gather point | 0; `0x600` | `0x55555555` | 0 | 0 | 1 |
| gather, nonzero block `0x00618314` | gather point | 0; `0x600` | `0x55555555` | 3 | 0 | 0 |
| final zero block `0x006184EE` | placement-container point | derived below | container/gather angle | 0 | 0 | 0 |
| final zero retry `0x0061855C` | placement-container point | both radii doubled | same | 0 | 1 | 0 |
| final nonzero block `0x006185DE` | placement-container point | derived below | container/gather angle | 3 | 0 | 0 |
| final nonzero retry `0x00618642` | placement-container point | unchanged | same | 3 | 1 | 0 |

An ordinary placement container uses its type `block_radius` as minimum and
`Constants +0x9C + minimum` as maximum. A wall-build instead begins with
`span = (x_size + y_size) * 0x30`:

- water actor: `inner = Constants+0x8C + span + actor.big_radius`, and
  `maximum = (Constants+0x90 - Constants+0x8C) + inner`;
- other actor: `inner = Constants+0x84 + span`, and
  `maximum = (Constants+0x88 - Constants+0x84) + inner`;
- minimum is `inner` only when the container low object-flags byte has bit zero set; otherwise
  it is zero.

If both zero-block-radius probes fail, retail falls back to the placement-container point. If
both nonzero-block-radius probes fail, retail returns 1 without unlinking.

## RNG chronology and host tails

There is no direct `Random::get` instruction in `0x00617C10..0x006186B3`.
`UnitType::find_nearby_spot` (`0x0061DE70`) is a zero-draw child. Therefore the ordinary
uncontained and contained-search paths preserve the incoming RNG stamp through the boundary.

The following children remain explicit host receipts because their own implementations may
advance RNG or mutate wider world state:

- captain recursive `Unit::come_out(0)` at `0x00617CF7`;
- `Objects::init_unit` at `0x00617EED`;
- transport recursive `Unit::come_out(0)` at `0x00617F52`;
- virtual transport death at `0x00617F84`.

Receipts must be continuous in that exact order. `find_angle`, `find_nearby_spot`, hot-key copy,
terrain Z, and containment/link operations are also modeled as authoritative observations or
typed mutations, not locally invented world data.

The full function does contain direct RNG later: calls at `0x0061A1BB` and `0x0061A1D5` feed
mutually exclusive modulo-3/modulo-2 tails. They are outside this prefix and are not covered by
its zero-direct-draw statement.

## Continuation and consumers

Opcode 49's `Unit::action_come_out` wrapper and Step 8's eject-contents edge both reach the same
general `Unit::come_out(0)` transaction. Their command/receipt integration remains owned by the
existing command and direct-entity lanes. This tranche supplies the next seam:

```text
plan_unit_come_out_prefix(facts)
  -> Returned { value, rng }
  |  ContinueAtCommonRelease {
         resume_va = 0x006186B4,
         actor, point, z,
         direct_container, placement_container,
         container_gpiece, rng
     }
```

`ContinueAtCommonRelease` is still open. In particular it does not authorize publication of the
common `set_new_location` call, later group/army/order mutations, or the late conditional RNG
tail. Closing those remaining 7,201 bytes is required before the general transaction, opcode 49,
or Step 8 can claim retail-complete `come_out` behavior.
