# Frame-zero starting-Unit City census

Lane: `checksum-cities` · replay-correctness · 2026-08-13.

## Result

`starting_city_unit_census` executes the source-owned starting-Unit portion of the first
`Leader::plan_strategy` City census. It clears each active City's `peasant_dist`, `free`,
`busy`, and `gatherers`, then walks the canonical Scout/Citizen allocation identities emitted
by the validated `Setup::build_units` plan and receipts. Empty-action Citizens are assigned to
the closest active same-owner, same-WData-region City and update `free` and `peasant_dist`.

The transaction is exact but not yet mounted in `StartingSetupState`. That host does not
materialize the setup allocation receipts as canonical `Sim::world` Units. A sibling setup
lane now has a complete ordinary init receipt for only the first 2018 Citizen; the remaining
four-call prefix and the admitted 2020 recordings are still absent, so that partial receipt
cannot lawfully activate this census. The separate terrain census now has the exact
activation-time TData `CITY` footprint, but stops at absent generated-content gather facts.
Consequently the replay scoreboard remains at **36,955 substantive Cities comparisons, zero
matches, and zero survival**. This increment deliberately does not consume a recorded checksum
or claim that the first checkpoint image is complete.

## Instruction-derived walk

The supported executable and matching PDB place `Leader::plan_strategy` at `0x006B9620`.
The City clear-loop prelude begins at `0x006B9746`; its active-row arm writes:

| address | City field | value |
|---:|---|---:|
| `0x006B976F` | `gatherers +0x5C` | `0` |
| `0x006B9789` | `busy +0x5B` | `0` |
| `0x006B97A3` | `free +0x5A` | `0` |
| `0x006B97BD` | `peasant_dist +0x50` | `City slot + 100` (low signed short) |

The instruction immediately before the City-pointer loads is
`lea esi,[edx+0x64]`; the final store copies `si`, not a loop-invariant constant. The
current replay fixtures have only City slot zero and therefore still receive 100, while
the generic owner preserves the slot-dependent value for later Cities.

The owning Unit-loop prelude begins at `0x006B9E2C`, with the per-row body at `0x006B9E50`.
The body admits an active, valid Unit only after the `unit_masks & 1 == 0` gate and the
`UnitTypeData::control_cost +0x2F0 != 0` gate. Only TypeIndex `0x32` and `0x33` then enter the
Citizen arm. The producer reads `control_cost` directly from the replay's immutable Rules
UnitType row; it is not a built-in balance constant.

For an empty `UnitData::get_action` result, retail calls `ObjectsData::find_city` at
`0x0065BA90` as `find_city(x, y, 1, who, who, 0x200, 0, 0, 0)`. `SearchIndexBH == 1`
requires the same owner and filter `0x200` requires the same WData region. Distance is the
integer `vector_dist` result from `0x0046CFF0`, divided by `0x300` before the signed-short
minimum against `peasant_dist`. `find_city` compares with `<=`, so the later City slot wins an
exact tie. The producer preserves each of those details.

Contained Citizens and nonempty Citizen actions remain typed refusals. Their busy/gatherer
arms depend on concrete action payload and target semantics that this bounded owner does not
possess. Refusal happens against a staged City pool, so even the preceding clear loop cannot
leak a partial mutation.

## Authority and joins

`StartingCityUnitCensusAuthority::from_build_units` revalidates every plan/receipt pair and
accepts only complete, successful, consecutive empty-owner allocations beginning at object
zero. Execution then proves, before publishing any City write:

1. every active Leader has exactly one setup authority and no inactive Leader has one;
2. the canonical sparse Unit band is dense-equivalent and its mark equals the receipt span;
3. every row's `{owner, o, id, generation, ptype}` agrees across receipt, World, and Sim;
4. every row is active, and the replay-carried UnitType facts contain its exact ptype;
5. the canonical Sim-owned City pool passes its existing Leader/Build/registry join; and
6. every admitted Citizen position maps to an in-bounds WData cell.

The receipt separately reports joined Scouts, Citizens, free-Citizen assignments, every
before/after City POD image, changed bytes, and zero World, Build/registry, or RNG writes. It
keeps `first_checksum_city_image_ready` false because the terrain census and remaining setup
schedule are separate owners.

## Verification

`crates/don-replay/tests/starting_city_unit_census.rs` freezes a canonical Scout plus two
Citizen allocation schedule, validates their stable Sim identities, and proves that two
empty-action Citizens produce `free = 2`, `busy = gatherers = 0`, and the exact
`vector_dist / 0x300` minimum. Mutation tests prove that a nonempty Citizen order and a stale
setup identity both refuse atomically without changing any City byte. The replay-carried
`control_cost` offset is also exercised against a real recording in
`groups_pre_pair_unit_authority.rs`.
