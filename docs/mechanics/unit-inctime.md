# Unit and guy `inc_time` — recovered step-15 evidence

Status: **research-only, Tier C, not wired**. The module declares the measured traversal and
animation-clock structure, but its composite drivers are crate-private and named
`*_research_partial`; `RUNTIME_FIDELITY_READY` is `false`.

## Retail shape

`Objects::inc_time` at `0x0065DB70` is step 15 of `Game::do_frame`. Unlike step 14, it walks
owners in fixed order and does not visit the wall object band. Live units receive virtual
`inc_time` followed by `Unit::execute_events`; buildings receive their shared
`Wall::inc_time`; goods, ammo, deaths, farms, doobers, and surf then take their measured paths.

`Unit::inc_time` at `0x00610B40` dispatches `Guy::inc_time` over the live squad prefix and the
crew suffix while skipping the dead middle band. `Guy::inc_time` writes fields inside the
`guys` checksum channel. Its transitions call `Guy::set_anim`, whose unresolved head can draw
zero to three times from `game_random` per call. Animation timing is therefore simulation
state, not harmless presentation.

## Runtime blockers

- `Guy::set_anim` head, including its conditional `game_random` draws
- `Unit::execute_events` `0x0060EDC0`
- shipped `AnimationPacket` / `.anm` durations
- `Wall::inc_time` `0x0063FB60`
- `DeathObj::inc_time` `0x008D5240`
- `Farms::inc_time` `0x008D8600`, including its two `game_random` sites
- the recursive squad path of `Guy::set_new_location` `0x005D86F0`
- `Doober::inc_time` `0x00846770`
- `Surf::inc_time` `0x008A1A00`

`MissingAnimData` reproduces retail's missing-asset fallback only. It is a research fixture,
not a model of the shipped game.

## Admission rule

Do not wire the partial drivers into the tick, replay validation, the RL environment, or the
playable edition. Admission requires shipped animation data, exact RNG consumption, all named
step-15 children, and retail differential evidence. Green local tests establish control-flow
consistency, not retail fidelity.

