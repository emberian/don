# ScenarioFuncSet::num_city_buildings (builtin 386)

## Retail identity

- Registration: `num_city_buildings(int who, String city, String type, int include_unfinished) -> int`
- Handler: `0x009F0150`
- Size: 129 bytes (`0x009F0150..0x009F01D1`)
- City resolver: `ScenarioFuncSet::get_city_index` at `0x009E2BA0`
- Type resolver: `ScenarioFuncSet::get_type_index` at `0x00A03480`
- Census body: `CityData::count_buildings` at `0x00739390`

The handler subtracts one from `who`, rejects the unsigned result when it exceeds seven, and
requires `LeaderData::leader_flags & 1`. It resolves the City before the Type. A failed Leader,
City, or Type lookup returns `-1`; a valid empty City chain returns zero.

`get_city_index` scans the active City rows below `LeaderData::city_mark` in ascending slot order.
Each row compares `CityData::id` first and then `CityData::name`, both case-insensitively. The Type
lookup scans all 806 rows in ascending order and compares the internal `TypeData::name`; an empty
or absent name fails.

## Exact City/Build traversal

The handler calls `CityData::count_buildings(type, 0, fourth_argument == 0)`. This last comparison
is the important branch inversion: script argument zero counts only completed/active buildings,
while any nonzero argument also counts unfinished valid buildings.

The census starts at `CityData::o` and uses the City's stored owner. Each visited object is resolved
through the canonical Build band, and the next link is always read from `BuildData::city_down`, even
when that Build is invalid. A row contributes only when:

1. `BuildData::flags` has the valid bit (`1`);
2. the completed/active bit (`4`) is present when the fourth script argument was zero; and
3. `ObjectData::is(requested_type, 0)` succeeds.

The second `count_buildings` argument is zero, so the Type test is non-strict and uses the canonical
`TypeRow::is_list` ancestry relation. The implementation preflights the City/Build identities,
external production type projection, and complete linked chain. A missing projection, wrong object
identity, out-of-range row, or cycle fails closed rather than publishing a plausible scalar. The
transaction itself is read-only.

## Canonical owners and transaction boundary

- City slots and marks: `Sim::cities` (`CityPool`)
- Build object mapping: `Sim::world.objects`, Build band
- Build rows and `city_down`: `Sim::builds`
- Current Build Type: `LiveProductionRuntime::build_types`
- Type names and non-strict ancestry: `TypeBuiltinState::types`
- Leader activation mirrors: Type owner, victory owner, and step-8 owner

The existing replay-selected `ScriptRuntime` owns the persistent Program/ref/timer transaction.
Builtin 386 is mounted into that host beside builtins 357 and 455; it does not add a second
ScenarioData, City, Build, or scalar counter owner. A later VM failure rolls the whole script call
back, while the City and Build census is proven unchanged.

## Save and installed-content evidence

The focused save gate serializes the canonical City and Build chain, reloads it, reinstalls the
external Build-Type projection, and obtains the same result. Separate tests cover the inverted
finished gate, a derived non-strict Build type, City id-before-name resolution, missing type
projection, and cycle rejection.

The installed replay `Playback___2020.07.25_19_30_12__Sat_.rcx` selects the shipped
`economic.bhs`. After Written Word / City State research and builtin 455's idle-Unit census, it
executes three builtin-386 calls for `Farm`, `Woodcutter's Camp`, and `Mine`. Each reads the empty
canonical City/Build chain as zero. Execution then reaches the actual next unowned builtin,
`num_type_queued` (registration 436).
