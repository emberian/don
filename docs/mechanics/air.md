# Air recovery boundary

Status: **Tier C; compiled primitives, not an integrated air simulation.** The implementation
lives in `crates/don-sim/src/systems/air.rs` and is declared by `systems/mod.rs`. Its focused
suite passes 37 tests. No air routine in this module has been differentially exercised against
retail, and no air order here is dispatched by `World::step`.

The evidence is deliberately split by source:

- `riseofnations.exe` supplies instruction-level branch structure and literal values;
- the matching shipped PDB supplies function and field identities, not behavior;
- `ron-data/unitrules.xml`, `buildingrules.xml`, and `rules.xml` supply shipped rule values;
- local Rust tests check the transcription and its internal invariants only.

No community formula is used. These sources support Tier C, not Tier B.

## Recovered primitives

| area | retail source | implemented boundary |
|---|---|---|
| anti-air dud/RNG gate | `Ammo::init` `0x0067BBF0`, gate at `0x0067BE49..0x0067C16A` | the 0/1/2-draw branch structure, short-circuit, and `FLAG_NO_DAMAGE` result |
| air/flag predicates | `UnitData::is_plane` `0x0046CE40`, `is_flying_low` `0x0060A140`, `is_flying_high` `0x0060A310` | domain, helicopter/missile exclusions, on-map bit, and low/high band decision over caller-supplied order context |
| order state | `is_air(OrderIndex)` `0x0046F000`, `AirOrder::walk_data` `0x0047F2D0` | three air order indices and the six walked `i32` fields of `AirOrder` |
| patrol cadence | `Unit::do_air_patrol` `0x005EA620` | 16/32-frame target-scan gates and waypoint-cursor step |
| host capacity | `ObjectData::num_aircraft_limit` `0x006454A0`, `num_aircraft_here` `0x00645330`, `Build::train` `0x0062F9B0` | strict carrier/airbase/silo capacities, hosted-aircraft count, and launch predicate |
| fuel | `UnitData::mana` `0x00609A50`, `Unit::process` `0x00610BC0`, `Unit::check_fuel` `0x005E9BE0` | cap, burn/recharge, return latch, and caller-supplied host-search verdict |
| air attack ground | `Unit::do_air_attack_ground` `0x005EA420` | release-angle gate, bombing fuel constant, and missile self-destruction predicate |
| detection tag | mask reads plus shipped unit/building rows | the `Z` detector predicate only; not the fog algorithm |

The anti-air gate is the highest-value piece because it consumes the main simulation RNG.
For an ordinary non-AA shooter, the first draw is checked against the target's flight-band
percentage and can short-circuit the second draw. Ground AA uses one draw against the
shooter's percentage. Air-domain AA, helicopters, missiles, non-air targets, and the two
ground-attack orders follow zero-draw paths under the conditions encoded in the module.

## The fingerprint test is not retail evidence

`gate_stream_fingerprint_is_frozen` expects:

```text
(total_draws, duds, final_rng_state) = (154, 88, -2024157294)
```

That tuple was observed from this Rust implementation over its fixed synthetic matrix. It is
a port change detector: it catches changes to branch order, draw count, or the Rust RNG call
sequence. It was not captured from `riseofnations.exe` and must not be described as a retail
fingerprint or Tier-B comparison.

## Wiring status

`arena::retail_systems` now provides a fail-closed host adapter over the anti-air and fuel
primitives: it obtains `AirTypeData` from the live tables, borrows the caller's main RNG, and
requires an explicit completed host-search verdict. This is integration scaffolding, not a
world caller. In particular, `systems/ammo.rs` still does not invoke `apply_antiair_gate` from
its `Ammo::init` model, and no current world tick executes the full airframe/order path.

Declaring the module means its types and 37 local tests compile in the workspace. It does not
mean:

- air combat consumes the recovered RNG draws during simulation;
- `STRAFE`, `AIR_PATROL`, or `AIR_ATTACK_GROUND` executes from `Unit::do_job`;
- aircraft state is reconstructed from a replay/save;
- the `units` or `ammo` checksum channel is non-trivial because of this module.

## Honest gaps before integration

1. Insert the gate at the measured point inside the live `Ammo::init` implementation, using
   the same main-stream `Random` instance as the rest of the tick. Respect `init_aborted` so
   the wrapper does not materialize a projectile on retail's early-return paths.
2. Port or connect the retail order-list driver and dispatch the three air orders. The current
   helpers flatten world/object/order lookups into caller-supplied values; they do not perform
   those mutations themselves.
3. Complete `UnitData::is_flying_low`'s order accessors and target lookup against real object
   state. Their role is identified, but the accessor bodies remain underived in this lane.
4. Complete the non-carrier queued-aircraft term in `num_aircraft_here`; the module exposes it
   as `leader_queue_adjust` rather than guessing the two `LeaderData` counters.
5. Implement the nearest-host scans and their exact tie/ordering behavior for `check_fuel`.
6. Reconstruct and walk the relevant `UnitData`, `AirOrder`, and `AmmoData` state in stable
   retail order, then compare replay channel values.
7. Add registered oracle cases before promoting any function above Tier C. At minimum, cover
   all anti-air arms and stream positions, the flight-band boundaries, fuel transitions, and
   capacity edge cases.

Until those steps land, this file is a trustworthy inventory of recovered local primitives,
not a claim that aircraft work end to end.
