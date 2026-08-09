# Armies — recovered step-13 evidence

Status: **research-only, Tier C, not wired**. The declared Rust module is an evidence-bearing
transcription, not a runnable implementation of `Armies::process_all`. Its composite drivers
are crate-private and named `*_research_partial`; `RUNTIME_FIDELITY_READY` is `false`.

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

## Admission rule

Do not wire the research drivers into `Game::do_frame`, replay validation, the RL environment,
or the playable edition. Admission requires all machine-readable blockers to be removed and
retail differential evidence for the completed entry points. Passing local control-flow tests
only establishes internal consistency of the transcription.

