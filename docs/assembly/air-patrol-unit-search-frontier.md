# AIR_PATROL mod-16 unit-search frontier

Status: **registered executable owner; live host search remains red**. The source-backed
module freezes `Unit::do_air_patrol`'s exact unit-search call chain, both target finders'
ordered passes, candidate fold, game-option fallback, and final acceptance. Registration makes
the contract available to product adapters but does not by itself change the live dispatcher,
save state, or strict closure.

## Authority and extent

Authority is the shipped `ron-bin/riseofnations.exe` (SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`) with
`ron-bin/sbl/rise.pdb`, `schema/symbols.json`, PDB local/parameter records, direct PE32
disassembly, and the mechanically generated `re/decomp-all` bodies.

| symbol | VA | bytes | role |
|---|---:|---:|---|
| `Unit::do_air_patrol` | `0x005EA620` | 1,248 | cadence, primary/fallback, acceptance |
| `Unit::find_new_bomber_target` | `0x005EB960` | 781 | build-candidate search and weighted fold |
| `Unit::find_new_air_target` | `0x005EBC70` | 873 | three-pass unit-candidate search |
| `Objects::find_builds` | `0x0065A120` | 1,274 | ordered build scratch list |
| `Objects::find_units` | `0x0065A620` | 1,309 | ordered unit scratch list |
| `Object::valid_target` | `0x00648BA0` | 457 | target admission |
| `Object::compare_target` | `0x0064E5C0` | 3,553 | strict-greater priority |

The executable frontier is
`crates/don-sim/src/systems/air_patrol_unit_search_frontier.rs`; its focused integration test
also loads that source directly so the proof remains usable independently of product wiring.

## Exact `do_air_patrol` chain

The unit arm is due only when the actor is not an animal, `AirOrder::returning == 0`, and
`(actor.o + Game::frame) % 16 == 0`. The primary origin is the last AIR_PATROL waypoint, after
the already-recovered fighter-bomber/home-relative projection and world restriction.

1. Type `0x130` (BOMBER) calls `find_new_bomber_target`; every other aircraft calls
   `find_new_air_target`. Both primary calls pass `guard_o=-1`, so their optional guarded pass
   is unreachable from AIR_PATROL.
2. Retail immediately calls `Object::valid_target(primary_o, primary_who, 0)`.
3. Only if that validation fails and `Game+0x821 & 2` is nonzero does retail call the inverse
   finder. The fallback origin is **the actor's current coordinate**, not the waypoint.
4. Retail reaches a common second `Object::valid_target` call even when the first validation
   succeeded. A valid candidate is accepted when the updated waypoint cursor is at the last
   point **or** the candidate's `ObjectTypeData+0x218` domain is 2 (air).
5. Acceptance installs STRAFE with `mandatory=0`; a non-air target before the last waypoint is
   ignored. The already-landed patrol code owns that queue insertion and UID-bearing payload.

The fallback option is therefore neither a preference nor an unconditional two-search union.
With bit `0x2` clear, a failed primary stops. With it set, the inverse search happens only after
the primary result fails retail admission.

One low-level detail is frozen because it can matter to an assembly adapter: if a finder rejects
a requested origin as too far from the actor, it returns `o=-1` without writing `*targ_who`.
In `do_air_patrol`, that stack slot previously held the zero `returning` value, so the following
validation observes `(-1,0)`. A completed miss writes `(-1,-1)`.

## `find_new_air_target`

`Constants+0x2C` is the PDB/rules field `AIRCRAFT_RESPOND_RANGE` (shipped 10 tiles). If actor
`ObjectData+0x68 & 0x40000` is set, the function first replaces its requested origin with the
actor coordinate. The passes are ordered:

| pass | centre | radius | `find_units` arguments | fold |
|---|---|---:|---|---|
| local | actor | `R*0xC0` | search 3, actor owner, mask 1, filter 13, data `(2,0)`, ignore animals 1 | valid; explicit actor distance `<=R*0xC0`; raw compare score |
| guarded | origin | `R*0x3C0` | filter 11, data `(guard_o,guard_who)` | valid; raw compare score |
| general | origin | `R*0xC0` | filter 0, data `(0,0)` | valid; explicit origin distance `<=R*0xC0`; raw compare score |

The local pass always executes first. Only after it has no winner does retail test whether the
requested origin is within `R*0x3C0` of the actor. The guarded pass exists only when
`guard_o>=0`; AIR_PATROL passes `-1`, while other callers may reach it. The guarded pass has no
redundant explicit distance test after enumeration. Every pass traverses the global Objects
scratch array in returned order, starts `best_val=-1`, and replaces the winner only on strict
`score > best_val`; the first equal-score candidate wins.

PDB gives `Objects::find_units`' named parameters through `filter_data2`, then
`ignore_animals`; the optimized body has one unnamed stack argument between them and does not
read it. Some callsites leave that argument as register residue. The frontier records it as
such rather than inventing deterministic state.

## `find_new_bomber_target`

`Constants+0x30` is `BOMBER_RESPOND_RANGE` (shipped 12 tiles), and the same `0x40000` origin
override applies. Unlike AIR, BOMBER tests origin-to-actor distance against `R*0x3C0` before
enumerating anything.

| pass | condition | radius | finder | fold |
|---|---|---:|---|---|
| guarded builds | `guard_o>=0` | `R*0x3C0` | `find_builds`, search 3, actor owner, mask/filter/data all zero, `add_to_list=0` | valid; `compare_target / (distance/0xC0 + 1)` |
| short builds | no earlier winner | `R*0xC0` | same | valid; explicit origin distance `<=R*0xC0`; same weighted score |

`guard_who` is not read anywhere in the 781-byte function, and `guard_o` only enables the broad
pass; neither value is forwarded as filter data. AIR_PATROL passes `guard_o=-1`, so its bomber
call reaches only the short build pass. Candidate order and the strict-greater rule again make
the first equal weighted score win. Neither finder nor either fold draws RNG.

The distance kernel is the already-established `vector_dist` `0x0046CFF0`, and the compiler's
magic divide sequence is source-equivalent to integer `distance / 192`. The isolated owner
includes that exact kernel. It fails closed outside the normal world-coordinate arithmetic
domain if the kernel overflows negative, rather than inventing process-level x86 divide-fault
behavior.

## Executable seam and open facts

The maximal exact seam is `run_patrol_unit_scan`: it owns cadence, primary kind, both finder
algorithms, option fallback, the common revalidation, domain/last-waypoint acceptance, and the
`mandatory=0` result. Its `UnitSearchHost` has no defaults and requires:

1. `Objects::find_units` / `find_builds` with exact scratch-array ordering;
2. `Object::valid_target`, including diplomacy, visibility, and targetability;
3. `Object::compare_target` backed by coherent live object/type/leader facts;
4. coherent object identity, UID, position, and `ObjectTypeData+0x218` domain.

Those are host/integration facts, not unresolved control flow. The repository already has an
executable `compare_target` arithmetic core, but its own documentation correctly leaves
`valid_target` and live-world acquisition as host gates. Splicing an all-objects scan, an
always-true admission flag, or an unordered candidate vector here would change target choice
and is not authorized by this frontier.

Publication remains separate: converge the `order_dispatch` callback on this owner, provide an
atomic `AirPatrolHost` adapter in `don-env`, and prove save/replay equivalence. Until those
steps land, the strict closure delta is **0** and
`env-air-patrol-unit-target-search` remains red.

## Focused validation

`cargo test -p don-sim --test air_patrol_unit_search_frontier` passes 10 tests. They freeze
symbol extents and shipped ranges, cadence, AIR pass/query order, the guarded-pass asymmetry,
the local-before-origin-gate order, BOMBER tile-distance weighting, first-on-tie behavior,
both fallback directions and their actor origin, final domain/waypoint acceptance, origin
override, and fail-closed missing-host behavior. Retail was not executed.
