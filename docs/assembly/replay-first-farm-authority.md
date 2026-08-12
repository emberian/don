# First 2018 Farm package authority

This owner pins one transaction only: serial 14, frame 79, player `play=1`/owner 0 in
`Playback___2018.11.17_13_21_42__Sat_.rcx`. The raw recording SHA-256 is
`c006ecb860273605d2b48bf69f5dcb048596de5fc748aa664fa0a04452df2da0`.
It does not extrapolate the seven later Farm packages.

## Exact green stages

The strict group-build history proves that the only opcode-25 package before serial 15 is
command index 0 with selected object `[4]`, type `0x1a1` (Farm), `queued=1`, and both
coordinate pairs `(22286, 66184)`. The package has no leading shell command and is followed
by opcodes `[0x4a, 0x48]`.

The replay-carried Rules section is admitted by its existing exact offset, size, SHA-256,
and checkpoints. A variable Type-table walk now extracts the Farm row rather than using a
name or installed catalog as authority. Its relevant exact facts are a 4-by-4 footprint,
job time 150, cost `[0, 4, 0, 0, 0, 0]`, 400 hits, and Build flags `0x10000049`.
The same Rules owner resolves tribe 14, its Citizen type 50, and Scout type 69.

The command geometry then produces these exact stages:

| stage | value |
|---|---|
| requested coordinate | `(22286, 66184)` |
| footprint corner tile | `(114, 342)` |
| snapped coordinate | `(22272, 66048)` |
| placement WData cell | `(29, 86)` |

The first replay camera for `play=1` carries the starting Village center at
`(21600, 64608)`, WData cell `(28, 84)`. The canonical setup owner identifies this as
fresh center object 2000 and City slot 0. The largest replay-prefix World still contains
region 64 at that cell; the later source-proven Himalayas region pass produces region 1.
That mismatch is retained as evidence that the prefix World is not a frame-79 City image.

The exact setup outer schedule calls the base Scout producer once and the simple Citizen
producer four times. Thus object 4 corresponds to the fourth Citizen only after the five
individual allocation receipts and intervening lifetime are bound. The adapter records
that conditional provenance but does not promote it to a Unit identity.

## Frame-79 builder receipt join

The adapter now provides a read-only, fail-closed join for a future genuine setup/replay
execution owner. It first validates the complete five-call `BuildUnitsPrefixReceipt`, requires
one direct placement draw and one spawned member at each ordinal, and requires the native
allocation sequence `(owner=0,o=0..4)` with types `[Scout 69, Citizen 50 x4]`. It then resolves
the fifth receipt's minted `{id,generation}` through the canonical Sim object bands at frame 79.

Admission captures the live UID, Group/form/masks, position/angle/order coordinates, full order
list, and path image. A recycled object-4 slot, mismatched Sim/World type, failed/queued setup
call, sparse allocation, or missing canonical path is rejected. The caller must also supply a
nonzero revision and composition digest for state produced by the validated setup followed by
the complete canonical replay schedule. These are external provenance, not facts inferred from
the frame scalar or recorded checksum.

This receipt producer does **not** make the real 2018 join green by itself: the recording still
lacks the post-worldgen RNG/map inputs needed to produce the five actual placement receipts and
the intervening 79-tick canonical state. A fresh save can exercise the same native address shape,
but cannot authorize this different 2018 match.

## First setup placement producer

The next adapter now consumes a revisioned canonical setup-entry authority instead of a detached
placement fixture. It requires frame zero, an empty owner-0 Unit band, dense-equivalent sparse
object bands, and the live Village at `(owner=0,o=2000)` with its canonical Build row, registered
type, and replay-camera position. It independently checks the complete synchronized map checksum
and the main RNG state; the latter is separate because it is not part of the map walk.

After those checks, the adapter projects every WData cell plus each center-TCoord collision word
directly from `Sim::map.world`, hashes that exact placement snapshot, and runs the source-owned
`Setup::place_unit` producer for Scout ordinal zero. Its result contains every consumed RNG draw,
candidate offset, rejection in native order, and the exact first external residual:
`Objects::init_unit` on success or centered `Build::get_build -> Build::train` after exhaustion.

This closes the executable injection seam, not the historical input gap. A valid authority must
come from completed canonical world generation followed by starting-City setup and must carry an
independently composed transaction digest. The current repository cannot derive that authority
for this recording because `TerrainGroups::place_all` and later worldgen/setup runtime inputs are
not all present. The adapter therefore does not allocate a Unit or clear the existing ordered
`PostWorldgenRandomState` / `SetupPlacementWorldSnapshot` blockers merely because a synthetic test
can exercise the API.

## First Scout `Objects::init_unit` receipt join

The complete instruction-derived `Objects::init_unit` receipt validator is now compiled through
the normal `don-sim::systems` surface. The first-Farm adapter consumes that detailed chronology
only when the placement receipt reached the exact all-`-1` initializer request. For the first
Scout it requires one `find_free` allocation from mark 0 to 1, a complete 3,732-byte `Unit::init`
extent for native `o=0`, the `previous=-1` link, and the final captain resolution back to `o=0`.

The join is read-only: it accepts canonical Sim states immediately before and after an externally
executed complete receiver. The after-state authority separately binds the map checksum and main
RNG, while a nonzero composition digest attests the nested Guy/graphics, collision, visibility,
Leader-accounting, and object-publication effects that are not represented by a scalar Unit
allocation. The adapter then checks owner-0 mark 1, active Scout type 69, initializer and final
captain position/angle/masks, empty initial orders/path, and resolves native `o=0` to the
canonical minted Handle/UID before returning the full Unit image.

`Sim::spawn_unit` appears only in the synthetic mutation fixture used to exercise this join. It
does not produce the authority digest and is not claimed to implement retail `Objects::init_unit`.
The actual 2018 chain remains red until a completed-worldgen authority and a retail-complete
initializer/after-image receipt are captured or executed. Consequently no replay channel is
installed and the `ObjectsInitUnitReceipts` blocker remains in the ordered discovery ledger.

## Checksum chronology

The package records Groups `0x1c78f3f5` and Units `0x2bc45014`. Independently walking
`Groups::clear` also produces `0x1c78f3f5`. Therefore the recorded Groups value is a
pre-issue chronology observation, not a post-Farm checksum oracle. No checksum is used to
fill state, choose a placement result, or claim execution.

## Fail-closed runtime boundary

The adapter refuses `GroupBuildRuntimeAuthority` until all of these inputs exist in native
order: post-worldgen RNG; the setup-placement World snapshot; `Objects::init_unit`
receipts; frame-79 Unit/Handle state; intervening frame chronology; frame-79 City state;
the WData object-list head; `validate_build` probe chronology; `Objects::init_build`
after-image; and the builder swarm-search after-image. It also keeps
`LeaderData::current_upgrade` unbound.

Consequently this tranche acquires exact per-package evidence but deliberately does not
execute the package or install a substantive Groups/Units checksum channel. Runtime
execution becomes valid only when the missing producer receipts can construct the landed
atomic opcode-25 host without defaults.

## Verification

`groups_first_farm_authority.rs` runs the strict recording through the complete discovery
path, asserts the stage map and both recorded checksums, and asserts the ordered red
boundary. Its mutations change the in-memory opcode-25 body and replay-carried Farm footprint,
then exercise a wrong setup map checksum/RNG/center type/allocation chronology, wrong frame,
wrong initializer request/body extent/final captain/RNG/type/Unit mark, missing direct placement
draw, nonconsecutive setup allocation, stale Handle generation, and split Sim/World types. All are
rejected before runtime promotion.
