# ScenarioFuncSet::find_num_idle_unit (builtin 455)

## Retail identity

- Registration: `find_num_idle_unit(int who, String unit_type) -> int`
- Handler: `0x009F3390`
- Size: 449 bytes (`0x009F3390..0x009F3551`)
- PDB procedure: `int __thiscall ScenarioFuncSet::find_num_idle_unit(int const &, String const &)`

The body first resolves `unit_type` by scanning all 806 `Types` rows in ascending order with
the retail case-insensitive string comparison. An empty or absent name resolves to `-1`. It then
subtracts one from `who`, rejects values outside `0..7`, requires both low Leader validity bits,
and accepts only a concrete Unit row in the explicit type range `50..414`.

## Exact live census

For each entry in `Objects::object_list[who]`, paired with the same ordinal in `Units`, retail
applies these gates in order:

| Gate | Retail evidence | Canonical owner |
|---|---|---|
| valid Unit | virtual `+0x08`, `UnitData::is_valid_unit` `0x0046CDA0`, `flags & 1` | `World::units.flags` |
| on map | virtual `+0xBC`, `UnitData::is_on_map` `0x0046CE30`, bit 15 of `inside_up` | `World::units.inside_up` |
| requested type | virtual `+0xB8`, `ObjectData::is(type, 0)` `0x00653790` | canonical `TypeRow::is_list` relation over `Sim::unit_type` |
| idle | `UnitData::order_type` `0x00616E80` equals `OrderIndex::None` | `World::orders(row)` |
| captain | virtual `+0xE8`, `UnitData::is_captain` `0x0046CEB0`, `o_up < 0` | `World::units.o_up` |

Only a row passing all five gates increments the result. The implementation in
`bhs_idle_unit_runtime` preflights the complete owner band and the World/Sim type mirrors before
publishing a receipt. A missing row, wrong owner, missing type, invalid type, or mirror mismatch
fails closed instead of returning a plausible scalar. The transaction is read-only; the enclosing
replay script transaction still rolls back Program/ref/timer and all production state if a later
builtin fails.

## Global evidence

- `GameAccess::objects`: `0x00C0618C`
- `Units units`: `0x00C0AEB0`
- `TypeData::is_unit_type`: `0x004707E0`
- `ObjectData::is`: `0x00653790`
- `UnitData::order_type`: `0x00616E80`
- `UnitData::is_captain`: `0x0046CEB0`

The product path is the existing replay-selected economic ScriptRuntime bridge; this work does not
add a second ScenarioData owner or a second find-counter authority.

## Installed economic continuation

The installed AI replay `Playback___2020.07.25_19_30_12__Sat_.rcx` selects the shipped
`economic.bhs`. After the successful Written Word / City State research cohort, execution calls
`find_num_idle_unit(3, "Citizens")`. The test Sim's canonical owner-2 Unit band is empty, so the
live census returns zero. Execution then stops at the actual next unowned builtin:
`num_city_buildings` (registration 386), rather than at builtin 455.
