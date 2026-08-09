# Unit and guy `inc_time` — recovered step-15 evidence

Status: **research-only, Tier C, not wired**. The module declares the measured traversal and
animation-clock structure, but its composite drivers are compiled only for tests and named
`*_research_partial`; `RUNTIME_FIDELITY_READY` is `false`.

## Retail shape

`Objects::inc_time` at `0x0065DB70` is step 15 of `Game::do_frame`. Unlike step 14, it walks
owners in fixed order and does not visit the wall object band. Live units receive virtual
`inc_time` followed by `Unit::execute_events`; buildings receive their shared
`Wall::inc_time`; goods, ammo, deaths, farms, doobers, and surf then take their measured paths.

`Unit::inc_time` at `0x00610B40` dispatches `Guy::inc_time` over the live squad prefix and the
crew suffix while skipping the dead middle band. `Guy::inc_time` writes fields inside the
`guys` checksum channel. Its transitions call `Guy::set_anim`. Each function activation can
draw zero or one time from `game_random`: the three sites are in mutually exclusive
requested-class arms. A captain/uber path can recursively invoke `set_anim`, however, so a
root call can create additional activations and does not yet have a proven finite draw bound.
Animation timing is therefore simulation state, not harmless presentation.

| Entered `set_anim` arm | RNG call | Exact local gate and result |
|---|---:|---|
| default / class 0 | `0x005DAC75` | after the captain/group-idle fallback reaches this block, draw only when `UnitData::openlist` (`+0x104`) is null; otherwise use a synthetic percentage zero |
| attack / class 12 | `0x005DB22A` | third argument enables autoselection; `<30` selects attack 1, `30..=70` attack 2, `>70` attack 3 before packet validation |
| nature walk / class 8 | `0x005DB346` | owner 9 and type `0x192`, `0x193`, or `0x194`; `<50` walks and `>=50` jogs before unit-mask overrides and packet validation |

The third argument is the attack variation/autoselect gate. The second argument is the
separate same-class restart/override gate; the measured `Guy::inc_time` call sites pass the
second argument as zero. These local arms do not stand in for the rest of the 4.7 KiB
`set_anim` body: earlier returns, captain/uber coordination and recursion, hero/spell cases,
packet fallbacks, and the final state writes still have to be integrated.

`Unit::execute_events` at `0x0060EDC0` is now transcribed in full as an exact dispatcher. It
walks `[0, guy_mark)` and chooses either `Guy::execute_events` or
`GraphicEvents::verify_load(gpiece)` from the unit/on-map/freeze gate. The callback bodies are
not approximated and remain blocked below. The 131-byte wrapper itself has no RNG or
checksummed writes. `Guy::execute_events`, however, builds a 56-byte game-event package and
may dispatch release events. Projectile release reaches `Ammo::init` (measured at zero to four
`game_random` draws per projectile); unit release reaches `Unit::come_out` (a measured late
branch draws zero or one time, with deeper callees still unaudited). Both paths also write
checksummed object or guy state.

## Runtime blockers

- full `Guy::set_anim` integration, including captain/uber recursion and state writes
- `Guy::execute_events` `0x005D99C0` and `GraphicEvents::execute_game_events` `0x008E48E0`
- `GraphicEvents::verify_load` `0x008E4780`
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
