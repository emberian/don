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
boundary. Its mutation case changes the in-memory opcode-25 body and the replay-carried
Farm footprint byte; both are rejected before runtime promotion.
