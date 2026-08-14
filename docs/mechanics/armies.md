# Armies — recovered step-13 evidence

Status: **executable Tier C dispatcher and deterministic prefix**. `Armies::process_all`
runs from tick step 13. `RUNTIME_FIDELITY_READY` remains `false`: the instruction-derived
outer/state-machine control flow is runnable, while reached AI bodies and an unattached live
Army host remain explicit gaps rather than approximations.

## Retail shape

`Armies::process_all` at `0x006F3B00` is step 13 of `Game::do_frame`. Retail preallocates 16
`Army` objects per player for eight players. An `ArmyData` is 152 bytes and contains up to 16
group ids; units remain owned by groups. A closed army slot becomes invalid and is reused.

The recovered module transcribes the measured container layout, save walk, owner gates,
128/256-frame phasing, membership, aggregate normalization, group ordering, merge/retirement
structure, targeting prologue, and command fan-out. `CheckSums::check_armies` at `0x00936CF0`
has no callers; `Armies::walk_data` is reached by save/load verification and
`GameLog::say_checksum`, but not by `CheckSums::check_all`.

The save/log walk now preserves the complete `PtrArray<Army>` stream. For each of the eight
owners it hashes length, capacity, increment, masked flags, sixteen pointer-presence bytes,
then capacity and increment again before recursing into the sixteen Army records. The fixed
post-init all-invalid image is 520 bytes; every live Army adds 150 bytes. Because Rust does
not retain the original container metadata, the walk admits only the exact `Armies::init`
shape of eight lists with sixteen non-null preallocated pointers and fails before changing
the checksum otherwise. This correction does not add Armies to `CheckSums::check_all`.

## What is not derived

`Army::do_mustering` `0x006F4260` is now recovered whole. Its external City/rally,
Leader strategy/difficulty/flags, and `find_muster_spot` calls are typed fail-closed seams;
the 337-byte body itself consumes no RNG.

`Army::do_defending` `0x006F4070` now has one instruction-derived executable prefix. Retail
begins with `count(2, 0)` and tail-calls `Army::close` when the result is below five. A
zero-group Army proves that count is zero, so the canonical path closes before `is_engaged`,
object search, muster search, or RNG. Nonempty Armies still stop before mutation.

The following retail bodies are reached by the step-13 control flow and remain absent:

- the remainder of `Army::do_defending` `0x006F4070` after its zero-group close prefix
- `Army::do_marching` `0x006F3DF0`
- `Army::do_forming` `0x006F43C0`
- `Army::do_transporting` `0x006F4690`
- `Army::engagement` `0x006F5160`
- `Army::use_generals` `0x006F4C30`
- `Army::use_spies` `0x006F4AF0`
- `Army::use_scouts` `0x006F49A0`
- `Army::find_muster_spot` `0x006F5CC0`
- the body of `Army::find_target` `0x006F69B0`

`Army::find_target` is especially load-bearing: it is 7,571 bytes and is the class's only
consumer of `game_random`. Counting its skipped calls does not preserve RNG position.

`Armies::leader_defeated` and the state-changing ordinary-multiplayer body of `Army::stop`
are executable through the defeated-owner drain. The outer scan preserves Army-slot and
Group-list order; the live adapter applies `Group::action_begin`/halt and Unit order/path
effects without changing `ArmyData`. The remaining stop-only boundary is the scenario
`ignore_orders` prelude. The complete `SpecialAnimOrder` payload now distinguishes the
ENTER/EXIT skip arms from the UNIT halt arm.

## Executable boundary

The public `Armies::process_all` accepts a complete `ArmyWorld` and executes the recovered
deterministic prefix, returning every reached unresolved body in `ArmyProcessTrace::gaps`.
The lightweight tick owns the exact 8-by-16 preallocated Army store, uses live Leader flags,
an explicit `leader_flags2` input, and scans invalid slots exactly. This is the ordinary
state until the still-unported `Leader::plan_strategy` creates an Army.

A valid Army reaches Group, Unit, City, diplomacy and type-table facts. Until that composite
host is attached, the tick counts valid records first and refuses the whole Army transaction:
no hurry bit, timer, target or group state is partially changed. This is a runtime boundary,
not retail-oracle evidence; `RUNTIME_FIDELITY_READY` remains false until the named bodies and
oracle work close.

## Verification

Focused module tests cover the PDB image, complete save walk and its fail-closed post-init
authority, live-field mutation sensitivity, dormant invalid tails, container lifecycle,
membership and aggregates, group sorting, movement/engagement tests, action fan-out,
targeting prologue, owner gates, both phase schedules, hurry, retirement, merge, retarget and
the production dispatcher trace. Real-tick tests pin active/vacuous dispatch, the
`leader_flags2 & 0xA` gate, 16 slots per enabled owner, and transactional preservation of a
valid Army when its live host is unavailable; defeated-owner tests additionally pin
standing-Army preservation and Group/member stop effects.
