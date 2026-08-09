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

## What is not derived

The following retail bodies are reached by the step-13 control flow and remain absent:

- `Army::do_mustering` `0x006F4260`
- `Army::do_defending` `0x006F4070`
- `Army::do_marching` `0x006F3DF0`
- `Army::do_forming` `0x006F43C0`
- `Army::do_transporting` `0x006F4690`
- `Army::engagement` `0x006F5160`
- `Army::use_generals` `0x006F4C30`
- `Army::use_spies` `0x006F4AF0`
- `Army::use_scouts` `0x006F49A0`
- `Army::find_muster_spot` `0x006F5CC0`
- the body of `Army::find_target` `0x006F69B0`
- `Army::stop` `0x006F9180`

`Army::find_target` is especially load-bearing: it is 7,571 bytes and is the class's only
consumer of `game_random`. Counting its skipped calls does not preserve RNG position.

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

Forty-nine focused module tests cover the PDB image, save walk, container lifecycle,
membership and aggregates, group sorting, movement/engagement tests, action fan-out,
targeting prologue, owner gates, both phase schedules, hurry, retirement, merge, retarget and
the production dispatcher trace. Four real-tick tests pin active/vacuous dispatch, the
`leader_flags2 & 0xA` gate, 16 slots per enabled owner, and transactional preservation of a
valid Army when its live host is unavailable.
