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

### Nested first-Scout Guy prefix

The first nested segment is now executable once those two authorities exist. The adapter reads
the strict replay Rules row for Scout 69 and requires its exact one-squad/one-crew shape. It
converts the complete initializer join's minted Unit Handle plus native `(owner=0,o=0)` address
into the stable identity consumed by `produce_unit_guy_init_prefix`, then requires two coherent,
hash-gated installed-graphics extractions and their PE-derived animation/flag predicates.

That producer allocates the exact two-entry Guy array, constructs both synchronized Guy images,
and consumes the two `Guy::init_real` main-RNG draws in slot order. The calculated RNG after-state
must equal the complete `Objects::init_unit` after-image authority. This joins the exact nested
chronology and detects a fabricated graphics receipt or inconsistent full-body RNG span; it does
not mutate the canonical Sim or infer graphics from replay bytes.

The local test graphics and `Sim::spawn_unit` after-image are explicitly synthetic API fixtures.
The recording has neither the installed `.bh3` hierarchy nor completed-worldgen/setup state, so
it cannot construct either upstream authority for the 2018 match. The segment therefore remains
a source-only receipt producer: no replay channel is installed and the real first-Farm execution
boundary remains red.

### Nested first-Scout location continuation

The next adapter composes that Guy prefix with the landed source-only
`produce_unit_init_location_continuation`. The replay Rules projection now includes
`ObjectTypeData::new_block_radius +0x248`; alongside domain, spacings, flags, squad/crew sizes,
and base formation, this supplies every static type fact consumed by the location body. The
placement request is normalized with retail's signed 48-unit-center formula, and the canonical
complete-initializer after-image must independently agree with the resulting position, angle
`0x55555555`, and base formation.

Composing these receipts exposed and corrected an older abstraction error in the detailed
`Objects::init_unit` validator: its `UnitAfterInit` position had been equated to the raw receiver
arguments. It now validates the normalized post-`Unit::init` coordinates, including signed
negative table division, while retaining the raw arguments on `UnitInitReceipt`. The nearby-spot
fallback test is pinned to the normalized after-image rather than the call inputs.

Fresh coherent graphics receipts restore the two Guys' gpieces and crew track offsets. Every
terrain-height answer remains an explicit ordered receipt, so neither the replay-prefix World nor
a flat default can silently authorize the transaction. The result contains both final synchronized
Guy images, unchanged post-Guy RNG, and the exact Guy-0 `CollCheck::move_unit` request. That shared
collision mutation is journaled but not applied; visibility and the remaining Unit tail are still
outside this adapter.

As above, the local height values and installed-graphics extractions are synthetic contract tests,
not evidence for the 2018 run. The recording carries neither input, and no completed-worldgen
authority is available. This therefore closes an executable nested segment without promoting the
real initializer, executing the Farm package, or installing a checksum channel.

### Nested first-Scout collision and common tail

The next adapter composes the location receipt with the canonical
`produce_unit_init_collision_tail` owner. Its mutable collision World must be a revisioned,
checksum-bound image at the exact `Unit::init +0xBCD9` seam: completed world generation and the
preceding setup/Unit work have run, but the journaled Guy collision and common tail have not. The
adapter clones this World, runs the complete collision/tail transaction on the clone, validates the
resulting identity and deferred visibility request, and commits only after every check passes.

Strict replay Rules supplies Scout 69's domain, type identity, and `unit_flags2`. Type-inheritance
queries, Leader flags/bonuses, the four address-bound stat returns, mana inputs, and earlier instance
fields remain explicit inputs covered by the collision-seam composition digest. They are not guessed
from the checksum or from the synthetic after-image. A forged World checksum or stat body leaves the
canonical collision owner unchanged.

On success the receipt contains the allocated/stamped collision-block delta, all common Unit-tail
stores, updated Leader flags, unchanged RNG, and the exact `Object::update_seen(0)` request. It stops
there: fog projection and `World::reveal_fog` side effects are still owned by the visibility runtime
and are not skipped or treated as inert.

The local collision World, scalar values, graphics, and terrain remain synthetic contract fixtures.
The 2018 recording supplies none of those dynamic seam inputs, so this composition does not make the
historical initializer executable, clear its ordered authority blocker, or install a checksum
channel.

### Nested first-Scout visibility transaction

Visibility is now composed as well because every owner can be bound without defaults. The
authority checksum-binds the immutable canonical World and carries exact preimages for Goods,
Items, and dynamic Leader arrays, whose standalone checksum projections do not cover this seam. It
also supplies the complete LOS, Leader, object-chain, rare-class, and type-availability authority
consumed by the existing source-owned visibility producer.

The adapter clones collision World, Goods, Items, and dynamic Leaders, executes the collision/tail
stage on those clones, then runs `Object::update_seen`, fog stamping, and every reached
`World::reveal_fog` effect. It commits all four mutable owners only after visibility chronology and
RNG continuity pass. A malformed visibility authority therefore rolls back the prior collision
allocation/stamp as part of the same outer transaction.

The returned boundary is `None` when no newly revealed goody requests auto-explore. If such a goody
is reached, the exact `Unit::get_goody_box -> Group` requests remain a named Groups residual; this
adapter does not cross that owner boundary. Module registration exposes the source through the
replay library but installs no checksum channel.

Again, the repository has no historical 2018 preimages for these owners. Synthetic empty registries
and fog planes prove atomic composition, not the recorded match. The real first-Scout setup and
first Farm package therefore remain red despite the now-runnable nested transaction.

### Complete first-Scout initializer receipt

The outer adapter now closes the source-owned chronology back through the complete
`Objects::init_unit` receiver instead of leaving the nested visibility transaction detached from
its caller receipt. It independently validates the detailed 1,603-byte receiver, retains the
admitted 3,732-byte `Unit::init` step, and requires its after-image to equal the collision/common
tail's final position, angle, identity, and complete `unit_masks` word. This caught a meaningful
seam distinction: Scout 69 starts the location body with replay-Rules `obj_masks`, and the common
tail subsequently adds the default-map bit. The completed after-image must carry the latter word;
it cannot be fed back as an earlier location default.

The canonical after-Sim is also checked field by field for every synchronized Unit tail scalar
represented by the engine store: collision sentinels, links, cavalry-archer backlink, play,
pathfinding coordinates, announcement frame, hits/LOS/speed/armor, spell time, stance, and both
mask words. The final captain must be native object 0 at the same normalized location with the
strict replay-Rules collision radius. The post-visibility World checksum and unchanged RNG must
equal the independently admitted complete after-image.

Only after all of those checks pass does the adapter publish collision/fog and the heterogeneous
Good/Item/Leader mutations. A late canonical-tail mismatch therefore rolls the entire nested
transaction back. Success emits an exact `InitUnitAuthorityReceipt` for setup ordinal zero,
including the stable Unit/Guys authority key, two Guy identities, empty ten-entry path stack,
empty orders, and Unit mark `0 -> 1`. This is the first receipt needed by the five-call setup
schedule; it does not fabricate the four Citizen receipts.

This remains an executable evidence adapter rather than historical execution. The contract tests
still use synthetic graphics hierarchy outputs, terrain-height answers, collision World, and
visibility registries. Neither the RCX nor its checksum tuple supplies those preimages, and no
completed-worldgen/setup attestation currently binds them to the 2018 run. Consequently the
`ObjectsInitUnitReceipts` blocker is narrowed to the remaining setup calls but not cleared, and no
frame-79 state, opcode-25 execution, or checksum-channel match is claimed.

### First Citizen allocation join

Setup ordinal one now chains from the completed Scout rather than restarting from the earlier
post-worldgen placeholder. The adapter requires the typed Scout receipt to contain the exact
1,603-byte receiver extent, mark `0 -> 1`, stable owner-0/object-0 identity, Scout type 69, and
the complete tail receipt. Its canonical after-Sim must independently retain the same
generational Handle, full synchronized Unit tail, World checksum, frame-zero state, and RNG.

Only then does the source capture the new placement WData/collision projection and execute the
second `Setup::place_unit` probe schedule for Citizen 50 from the Scout's post-initializer RNG.
The result preserves every consumed draw and stops at the exact owner-0 Citizen
`Objects::init_unit` request. A changed Scout LOS/mask/link scalar, recycled object 0, altered
World, or changed RNG refuses the second call before allocation authority is considered.

The adjacent join admits a complete detailed receiver only when it allocates native object 1,
advances the Unit mark `1 -> 2`, returns captain 1, resolves the exact Citizen type and collision
radius from the replay Rules, and matches the canonical Unit/Handle, empty orders/path, map
checksum, and RNG after-image. It also rechecks that Citizen creation did not rewrite the prior
Scout tail. This produces a stable first-Citizen allocation receipt suitable for the next nested
Guy/location tranche.

That next tranche is now source-owned as well. Strict Rules fixes Citizen 50 to one squad Guy,
zero crew Guys, and one uber member. The initializer starts from the placement receipt's RNG,
requires one hash-gated installed-graphics row and one predicate row, and consumes exactly one
`Guy::init_real` draw. It then executes the existing complete graphics/location continuation with
the Rules-derived formation, spacing, domain, masks, and collision radius. The ground single-Guy
path consumes exactly two ordered terrain receipts (Unit tile height, then Guy coordinate height)
and ends with the exact collision request journal; malformed graphics, reordered terrain, a stale
prior Scout, or divergent initializer RNG refuses the whole projection.

The Citizen's shared collision/common-tail/visibility body now composes through the same atomic
owners as the Scout. The source transaction applies collision blocks, executes the deferred
`Object::update_seen` body, requires the complete 3,732-byte `Unit::init` after-image and captain,
and emits an ordinary `InitUnitAuthorityReceipt` for setup ordinal one. A canonical tail mismatch
rolls the collision and visibility owners back together.

The graphics, terrain, tail-stat, Leader, and visibility receipts are hash-gated but are not
serialized by the 2018 recording, and the tests exercise this interface with synthetic exact
values. Thus ordinal one has a complete executable receipt shape without claiming those inputs
came from the real 2018 run. Three later Citizens, completed-worldgen/setup provenance, frame-79
execution, and the Farm/checksum channel remain red.

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
