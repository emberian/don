# BHS mutable type table and restore ownership

Status: isolated owner validated, not integrated. The implementation is
`crates/don-sim/src/systems/bhs_type_table.rs`; its isolated tests are
`crates/don-sim/tests/bhs_type_table.rs`. No runtime export or handler is claimed.

## Result and honest coverage

The owner represents the exact state touched by registrations 284, 286, 288–291, and 815–819:
806 live rows, their non-strict `is` relations, immutable restore records, 24 tribes, and the
relevant masks for exactly eight Leader slots. Registration 289 is exact for every non-spell
row and has a typed fail-closed boundary for retail's anomalous spell-only `leaders[-1]` read.

The arity-resolved shipped-script census is:

| registration | signature | calls | files |
|---:|---|---:|---:|
| 284 | `disable_type(string)` | 239 | 45 |
| 286 | `enable_type(string)` | 20 | 4 |
| 288 | `rename_type(string,string)` | 11 | 10 |
| 289 | `type_build_time(string)` | 126 | 16 |
| 290 | `set_type_build_time(string,int)` | 383 | 21 |
| 291 | `set_type_job_time(string,int)` | 4 | 4 |
| 815 | `disable_type_by_tribe(string,string)` | 471 | 44 |
| 816 | `enable_type_by_tribe(string,string)` | 119 | 10 |
| 817 | `enable_type_by_tribe(string,string,string,int,int)` | 235 | 20 |
| 818 | `enable_type_by_tribe_with_type_name(string,string)` | 0 | 0 |
| 819 | `enable_type_by_tribe_with_type_name(string,string,string,int,int)` | 6 | 2 |
| | **total** | **1,614** | |

The earlier 1,215 roll-up included the 125 calls to registration 247,
`set_population_cap`; it is not one of the registrations above. A name-only census also
collapses the overloads into 354 calls under 816 and 6 under 818. The table above is the honest
registration/arity split.

The 288–291 counts are comment/string-stripped and all 524 calls have the exact registered
arity. Their raw textual counts are respectively 12, 133, 392, and 4.

Because the module is not exported and `script_runtime.rs` has not been changed, immediate BHS
coverage gained is **zero**. Integration of 815–819 can unlock 831 calls. Integration of the
canonical relation and exact backups also unlocks 284/286's 259 calls. The new owner-local
288–291 cohort contributes another 524 calls, for 1,614 total once every external boundary is
connected.

## Retail bodies

| registration | function | VA | size |
|---:|---|---:|---:|
| 284 | `ScenarioFuncSet::disable_type` | `0x009EA130` | 374 B |
| 286 | `ScenarioFuncSet::enable_type` | `0x009EA3E0` | 423 B |
| 288 | `ScenarioFuncSet::rename_type` | `0x009EA6C0` | 145 B |
| 289 | `ScenarioFuncSet::type_build_time` | `0x009EA760` | 100 B |
| 290 | `ScenarioFuncSet::set_type_build_time` | `0x009EA7D0` | 174 B |
| 291 | `ScenarioFuncSet::set_type_job_time` | `0x009EA880` | 171 B |
| 815 | `ScenarioFuncSet::disable_type_by_tribe` | `0x00A006A0` | 764 B |
| 816 | `ScenarioFuncSet::enable_type_by_tribe` | `0x00A009A0` | 764 B |
| 817 | five-argument overload | `0x00A00CA0` | 230 B |
| 818 | `enable_type_by_tribe_with_type_name` | `0x00A00D90` | 767 B |
| 819 | five-argument overload | `0x00A01090` | 230 B |

All type searches walk `Types[0..806)` in ascending order. The ordinary handlers compare the
argument with `TypeData::name` at `+0x60`; 818/819 compare with `type_name` at `+0xB0`.
The search instructions are visible at `0x009EA150..0x009EA16C`,
`0x00A006C0..0x00A006DC`, and `0x00A00DB0..0x00A00DCF`. Each uses
`String::operator==` at `0x00A1F140`, which first requires equal UTF-16 length and then uses
`_wcsicmp`. Empty input fails. The module implements this exactly for the ASCII-only shipped
names and rejects non-ASCII mod names at a typed boundary rather than substituting Rust Unicode
case folding.

`Tribes::find` at `0x006EF2D0` scans 24 records, in order, with the same String equality.
Valid targets are concrete Unit rows `50..414` or Build rows `414..543`. Other first matches
return `-1` rather than continuing to a later duplicate.

### Registrations 288–291

All four handlers reject an empty lookup string and perform the same first-match `name +0x60`
search across rows 0 through 805. They have no player/Leader gate and no Unit/Build domain gate.
Registrations 288, 290, and 291 then scan all 806 candidate rows in ascending order and test
`candidate.is(selected, 0)`; this is intentionally a different candidate range from 284/286.
None of the three mutations writes `TypeData::modified`.

Registration 288 copies its second String verbatim to `display_name +0x74` on every related
candidate (`0x009EA713..0x009EA741`). Internal lookup `name +0x60` remains unchanged. Empty and
non-ASCII replacement display strings are accepted; only the first lookup argument uses the
current ASCII fail-closed query boundary. Once the selected row is found, the handler returns
literal 1 even if no relation row was changed.

Registration 289 calls virtual `TypeData::time(-1)` through vtable slot `+0x6C`. The ordinary
path at `0x00663F43` returns wrapping unsigned `job_time * 100`; the builtin returns that exact
32-bit pattern as `int`. This covers Unit, Build, Tech, Age, Government, and every other
non-spell row—shipped calls visibly include names such as `Barter`, `The Art of War`,
`Despotism`, and `Gunpowder Age`, so an `Other`-domain rejection would be wrong.

Spell rows 629 through 683 take the different branch at `0x00663F4C`: retail calls
`LeaderData::has_preq` at `0x006E0BC0` and chooses `job_time` when true or `res_time +0x0C`
when false. Because registration 289 passes `who=-1`, the address calculation uses the
`leaders` base `0x00E3A390`, stride `0x6EEC`, and reaches pre-array address `0x00E334A4`.
The owner returns `SpellTimeRequiresLeaderMinusOne { slot }` for that range rather than
inventing state at this anomalous address.

Registration 290 computes signed `seconds / 100`, truncating toward zero, stores the quotient
bit pattern into unsigned `job_time +0x08`, and substitutes 1 only when the quotient is exactly
zero (`0x009EA837..0x009EA861`). Consequently `-100` stores `0xFFFFFFFF`, while `-99..99`
store 1. The original seconds argument is returned.

Registration 291 is not an alias: signed inputs below 200 store 1; only inputs at least 200 are
divided by 100 (`0x009EA8E7..0x009EA912`). Thus every negative value stores 1. It also returns
the original argument.

### Registration 284

After resolving the selected row, retail scans ordinary-unit candidates `50..402` when the
selected row is a Unit, or Build candidates `414..543` when it is a Build. Gaia rows 402..414
can be selected but are not candidates. For every candidate whose canonical
`candidate.is(selected, 0)` is true, the stores at `0x009EA244..0x009EA26A` occur in this order:

1. `preq[0]` at `+0x30` becomes `-2`;
2. `tribe_mask` at `+0x10` becomes zero;
3. `modified` at `+0x58` becomes one.

The selected TypeIndex is returned even if its relation yields no candidate.
The zero strictness argument selects the canonical non-strict ObjectType relation backed by
`ObjectTypeData::is_list` at `+0x27C`; `is_strict_list` is the separate array at `+0x298` and
is not substituted. Both arrays are aggregate fields absent from the generated scalar stores.

### Registration 286 and immutable backup

Registration 286 uses the same candidate ranges and relation. It restores every matching live
row from the separate `typesbak` array; it is not an availability-bit toggle.

The PDB layouts and restore bodies close the exact ownership needed by this handler:

| live field | live offset | backup field/offset | restore body |
|---|---:|---:|---:|
| `job_time` | `+0x08` | `TypeBak +0x000` | `0x00668000` |
| `tribe_mask` | `+0x10` | `TypeBak +0x008` | `0x00668000` |
| `display_name` | `+0x74` | `TypeBak::name +0x20C` | `0x00668000` |
| `preq[0..3]` | `+0x30..+0x38` | `+0x2D4..+0x2DC` | `0x00668000` |
| `costs[0..6]` | `+0x18..+0x2C` | `+0x2E0..+0x2F4` | `0x00668000` |
| `attack` | `+0x1E8` | `ObjectTypeBak +0x2F8` | `0x0065FB30` |
| `min_range`, `max_range` | `+0x1F8`, `+0x1FC` | `+0x2FC`, `+0x300` | `0x0065FB30` |
| `hits`, `armor` | `+0x210`, `+0x214` | `+0x304`, `+0x308` | `0x0065FB30` |
| `los`, `science_los` | `+0x21C`, `+0x220` | `+0x30C`, `+0x310` | `0x0065FB30` |
| `moves`, `turn_speed` | `+0x2C0`, `+0x2C4` | `UnitTypeBak +0x314`, `+0x318` | `0x009EA494..0x009EA4A6` |
| `mana`, `control_cost` | `+0x2EC`, `+0x2F0` | `+0x31C`, `+0x320` | `0x009EA4AC..0x009EA4C5` |
| `town_hits` | `+0x2B4` | `BuildTypeBak +0x314` | `0x00631F80` |
| `plunder_value`, `plunder_good` | `+0x2D4`, `+0x2D8` | `+0x318`, `+0x31C` | `0x00631F80` |
| `garrison_max`, `base_arrows` | `+0x2C8`, `+0x2CC` | `+0x320`, `+0x324` | `0x00631F80` |
| `most_shots`, `wonder_val` | `+0x2C4`, `+0x2D0` | `+0x328`, `+0x32C` | `0x00631F80` |

The backup structures are 760, 788, 804, and 816 bytes respectively, but only the listed
fields are consumed by these restore bodies. `TypeBak::type` and `type_name` are not restored.
Neither are the live internal `name`, live `type_name`, `where`, grid bytes, or `modified`.
Notably, `Type::restore` assigns `TypeData::display_name`, not the lookup `name`.

Retail has 566 backup rows for ordinary units 50..401, Builds 414..542, and Techs 544..628.
The module requires a domain-compatible immutable backup for every candidate 284/286 can reach
and refuses construction when one is absent. Backup capture belongs at the end of pristine
rules composition, never lazily after a script mutation.

### Registrations 815, 816, and 818

After first-match type and tribe resolution and domain validation:

- 815 clears the tribe bit in `tribe_mask`, then sets `modified=1`;
- 816/818 set the tribe bit, then set `modified=1`.

They inspect exactly Leader slots 0 through 7. A Leader qualifies only when
`leader_flags & 1 != 0` and `leader.tribe == resolved_tribe`.

815 clears the resolved type bit in `LeaderData::tech`, whose `BitMask<806>` starts at
`+0x6C0C` and whose 101-byte payload starts at `+0x6C18`. If the mask flags at `+0x6C14`
were zero they become 2; any nonzero value is preserved. The first slot is visible at
`0x00A00770..0x00A007A9`.

816/818 instead clear the bit in `LeaderData::obs_flags`, starting at `+0x6CF4`, with payload
at `+0x6D00` and flags at `+0x6CFC`. The corresponding first-slot code is
`0x00A00A70..0x00A00AA9` / `0x00A00E63..0x00A00E9D`. They do not clear `tech`.

The base handlers return the selected TypeIndex. The implementation owns the complete
`BitMask<806>` header (`bits`, `size`, `flags`) and inline 101-byte payload so these writes do
not land in a cache facade.

### Registrations 817 and 819

Each five-argument wrapper calls its two-argument base first. The base mutations are retained
if later validation fails.

- A Build target immediately returns 1 and does not inspect `building_type`, row, or column.
- A Unit target resolves `building_type` by ordinary `TypeData::name`, even for registration
  819. Missing or non-Build resolution returns `-1` after the base mutation.
- Success writes `where = building index` at `+0x40`, `grid_x = low_byte(column)` at `+0x5C`,
  and `grid_y = low_byte(row)` at `+0x5D`, then returns 1.

The reversed x/y versus row/column labels and byte truncation are pinned by
`0x00A01126..0x00A01151` (and the identical 817 body at `0x00A00D36`). Values are not clamped.

## Existing projections and single-owner boundary

`generated::state::{object_type, unit_type, build_type}` already materialises many scalar
columns at their PDB offsets. Those stores are useful loader adapters, but they do not provide
one 806-row table and deliberately omit aggregate String and `SimpleArray` fields. In
particular, the builtins need `name`, `type_name`, `display_name`, `job_time`, and the exact
non-strict `is_list`, which are not materialised there as one canonical owner.
`systems::tech_cities::TechRule`, production type facades, and victory-score `TypeTable`
similarly cover narrower read projections. `res_time` is deliberately not added merely to hide
registration 289's unowned spell/Leader dependency.

Integration must populate `TypeRow` from those existing scalar projections plus the rules
String/relation source, then make `TypeBuiltinState` canonical. It must not mirror mutations
back and forth between independent owners.

## Determinism, checksum, and save boundary

The checksum-visible mutated Type rule fields belong to retail checksum channel 13:
`Game::walk_rules_data` at `0x00589550` calls `Types::walk_rules_data` at `0x00669800`, which
visits all 806 rows. `Type::walk_rules_data` hashes `[+4,+94)`, so `job_time`, `tribe_mask`,
costs, prerequisites, `modified`, and the grid bytes are direct channel-13 state.
`ObjectType::walk_rules_data` also hashes `[+484,+636)`, covering all seven object restore
fields. Its Unit and Build tails cover every concrete restore field listed above. The String at
`+0x74` is gated off on the checksum path, so restored `display_name` is not a direct checksum
byte. A digest adapter must merge the canonical live fields into the complete existing type
projection and walk the rows in retail order. The immutable backup is restore input, not live
mutated rules state.

Registrations 290 and 291 therefore mutate direct channel-13 state even though they leave
`modified` unchanged. Registration 288 mutates canonical save state, but its `display_name`
String is not a direct checksum byte. Registration 289 is read-only.

`LeaderData::tech` and `obs_flags` lie outside the retail `LeaderData::walk_data` prefix and
must not be silently added to checksum channel 8. Their later effects remain deterministic, but
they are not direct channel-8 bytes in retail.

DoNSave v6 currently owns neither the mutable type rows nor these Leader mask payloads/flags.
The module exposes `is_dirty()` so save admission can fail closed after a successful mutation.
A faithful save implementation must serialize and restore the live row state and Leader masks;
silently reconstructing them from pristine rules would erase scenario effects. Immutable
backup data can be reconstructed only from the exact synchronized rules/mod input.

No handler directly consumes game RNG. The mutation changes later eligibility, production,
research, and object behaviour, so downstream RNG and checksum consequences can differ; no
draw is added at the builtin call itself.

## Frozen minimal integration map

No item below is performed by this source-only tranche.

1. Export the module once its owner is placed in the canonical Sim state; do not add another
   script-only table.
2. During synchronized rules/mod composition, construct exactly 806 rows and 24 tribes, import
   the canonical non-strict relation lists, and capture the immutable backups before scripts run.
3. Adapt the eight canonical Leader slots' tribe, `tech`, and `obs_flags` fields to this owner.
4. Route script registrations 284, 286, 288–291, and 815–819 to the methods with their exact
   return conventions and overload arities. Keep registration 289 spell queries mapped to its
   typed fail-closed error until the anomalous dependency is resolved.
5. Make channel 13 consume these live fields in its complete type projection. Do not fold the
   Leader masks into channel 8.
6. Extend DoNSave ownership for the mutable rows and Leader masks, or reject save while
   `TypeBuiltinState::is_dirty()` is true.
7. Only after the script bridge, checksum, and save boundaries are connected may the 1,614
   shipped calls be marked handled.

## Red boundary

The retail algorithms and the full restore subset they consume are source-complete here. The
lane remains red end to end because no shared runtime export, canonical Sim field, rules loader,
channel-13 adapter, script dispatch, or save/load integration was authorized. Non-ASCII mod
names also remain an explicit typed failure until an exact Windows `_wcsicmp`-compatible fold is
provided. Registration 288 replacement display strings are unrestricted and are not affected by
that lookup-only boundary. Registration 289 spell rows remain an explicit typed failure.

## Isolated validation

The committed 284/286/815–819 baseline passed all eight mutation-sensitive tests in both debug
and release profiles on 2026-08-09:

- hbox `bhs-type-owner-20260809T211027Z-89382-528-41090dd4ab15`;
- persvati `bhs-type-owner-release-20260809T211027Z-89384-23210-41090dd4ab15`.

The 288–291 extension's five additional mutation tests bring the suite to 13/13 in both debug
and release profiles:

- hbox `bhs-type-local-2-20260809T212713Z-12027-16124-b1e98fb890c4`;
- persvati `bhs-type-local-2-release-20260809T212713Z-12021-10960-b1e98fb890c4`.

They cover the all-806 scan, unrestricted display strings, wrapping readback, both distinct
signed setter boundaries, typed spell failure, and mutation-free error paths. Neither proof
changes the zero immediate runtime-coverage claim above.
