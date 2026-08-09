# BHS type-stat mutation frontier

Status: exact source-only transaction planning; canonical owner application, Leader cache
execution, runtime dispatch, channel-13 projection, and save/load remain red.

The isolated implementation is
`crates/don-sim/src/systems/bhs_type_stat_frontier.rs`; its proof pack is
`crates/don-sim/tests/bhs_type_stat_frontier.rs`.  It consumes the existing canonical
`TypeBuiltinState` by immutable reference and emits stale-state-bindable row writes.  It does not
create a second type table and cannot silently mark any handler complete.

## Selection and corpus value

This is the largest coherent untouched cohort whose immediate state dependency is owner-local:
the ObjectType/UnitType stat setters at registrations 529, 531–535, 538, plus the matching line
of sight setter at 814.  The comment/string-stripped lexical census over the 363 shipped BHS
files (93,649 lines) is:

| registration | exact signature | calls | files |
|---:|---|---:|---:|
| 529 | `set_object_type_max_health(string,int)` | 124 | 31 |
| 531 | `set_object_type_armor(string,int)` | 4 | 3 |
| 532 | `set_object_type_attack(string,int)` | 4 | 3 |
| 533 | `set_object_type_max_range(string,int)` | 4 | 3 |
| 534 | `set_object_type_min_range(string,int)` | 0 | 0 |
| 535 | `set_unit_type_speed(string,int)` | 5 | 4 |
| 538 | `set_unit_type_max_craft(string,int)` | 9 | 9 |
| 814 | `set_type_line_of_sight(string,int)` | 44 | 6 |
| | **total** | **194** | |

Registration 530, `object_max_health(int,int)`, is deliberately excluded.  Its body resolves a
live player/object pair and calls an Object virtual at `+0x11C`; treating it as a type-table read
would erase the user's requested no-live-object boundary.  Registration 537 mutates a live Unit
and is excluded for the same reason.

## Binary evidence

Ground truth is `ron-bin/riseofnations.exe`, 9,925,120 bytes, SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.
`schema/bhs-builtins.json` freezes every registration, arity, return type, and handler VA;
`schema/rise-procs.tsv` supplies the PDB procedure extents.

| registration | retail VA | bytes | PDB field | offset |
|---:|---:|---:|---|---:|
| 529 | `0x009F5DA0` | 433 | `ObjectType::hits` | `+0x210` |
| 531 | `0x009F5FB0` | 433 | `ObjectType::armor` | `+0x214` |
| 532 | `0x009F6170` | 433 | `ObjectType::attack` | `+0x1E8` |
| 533 | `0x009F6330` | 433 | `ObjectType::max_range` | `+0x1FC` |
| 534 | `0x009F64F0` | 433 | `ObjectType::min_range` | `+0x1F8` |
| 535 | `0x009F66B0` | 369 | `UnitTypeData::moves` | `+0x2C0` |
| 538 | `0x009F6980` | 291 | `UnitTypeData::mana` | `+0x2EC` |
| 814 | `0x00A00550` | 327 | `ObjectType::los` | `+0x21C` |

The registration-538 catalogue name calls `UnitTypeData::mana` “maximum craft”; the frontier
keeps both identities visible instead of renaming the canonical PDB field.

## Shared setter body: 529 and 531–534

Each body performs these steps in this order:

1. Reject an empty name, otherwise scan all 806 `Types` rows in ascending order and stop at the
   first case-insensitive `TypeData::name +0x60` match.
2. Reject a value less than or equal to zero.
3. Accept a selected Unit row in 50 through 413 or a Build row in 414 through 542.  An invalid
   first duplicate does not fall through to a later valid duplicate.
4. For a Unit selection, scan candidate rows 50 through 401.  For a Build selection, scan 414
   through 542.  Gaia Units 402 through 413 can be relation roots but never write candidates.
5. Call canonical virtual `candidate.is(selected, 0)` (`vtable +0x60`).  On true, write the
   handler's field and then write `TypeData::modified +0x58 = 1`.
6. Scan exactly eight Leader slots.  For each slot with `(leader_flags & 3) == 3`, call
   `Leader::calc_wall_stats` `0x006CF7C0`, then `Leader::calc_unit_stats` `0x006CF970`.
7. Return the selected TypeIndex, even if its relation produced no candidate writes.

The five bodies are byte-for-byte structural clones apart from branch targets and the field
store.  The mutation loop is visible at `0x009F5EA5..0x009F5EDE` for registration 529 and
`0x009F626F..0x009F62AE` for registration 532.  The Leader tail is
`0x009F5EE0..0x009F5F15` in 529.

## Unit-only variations: 535 and 538

Registration 535 keeps the same positive-value gate, name search, Unit selection range
50 through 413, relation-candidate range 50 through 401, `modified=1`, both Leader recalculation
calls, and selected-index return.  Build and Other selections return `-1`.

Registration 538 has the same Unit-only admission and candidate range.  It writes PDB `mana`
at `+0x2EC`, sets `modified=1`, and calls only `Leader::calc_unit_stats` for qualifying Leaders;
it does not call `calc_wall_stats`.  The candidate loop is `0x009F6A09..0x009F6A50` and the
Leader tail is `0x009F6A52..0x009F6A87`.

## Line-of-sight variation: 814

Registration 814 is intentionally not normalised into the positive-only template:

- it clamps the signed input into inclusive range 0 through 64 before name lookup;
- it accepts the first matching row without a Unit/Build domain rejection;
- a Unit selection scans candidates 50 through 401, while every non-Unit selection scans Build
  candidates 414 through 542;
- each related candidate receives clamped `los` and `modified=1`;
- qualifying Leaders receive wall then unit recalculation;
- success returns literal 1 rather than the selected TypeIndex, including when no relation row
  was changed.

The clamp is `0x00A0055D..0x00A00571`, candidate range selection is
`0x00A005B2..0x00A00602`, writes are `0x00A00628..0x00A0063A`, and the Leader tail is
`0x00A00648..0x00A0067E`.

## Ownership and completion gates

The frontier returns a plan containing the selected row, exact old/new field and `modified`
values, ordered candidate indices, and ordered Leader recalculation slots.  Planning is pure;
tests pin that `is_dirty` and the mutation revision remain unchanged.  Production integration
must land atomically inside `TypeBuiltinState` so it can stale-check every expected value, apply
every write, advance the owner receipt once, and then execute the recalculation tail.

The following gates remain red:

1. registration dispatch for 529, 531–535, 538, and 814 is not wired;
2. `Leader::calc_wall_stats` and `Leader::calc_unit_stats` have not been connected to the
   authoritative Leader caches;
3. checksum channel 13 does not yet consume the canonical mutated type rows;
4. DoNSave v6 does not serialize these rule mutations or their synchronized rules/mod
   provenance;
5. non-ASCII mod-name lookup remains typed failure until Windows `_wcsicmp` compatibility is
   owned;
6. no runtime or shipped-script coverage may be claimed from this source-only proof.

All eight fields are direct type-rule checksum state in the retail Type/ObjectType/UnitType
walks.  Leader recalculation produces derived caches after the writes; it is a required ordered
effect, not permission to hash new Leader bytes into channel 8.

## Validation order

Root convergence formatted the two Rust files and validated all eight tests in persvati batch
`gen7-five-pack-20260809T231109Z-3866-5144-5f896c0277b5`. Retail was not run. Only after that
proof and the five ownership gates above may the 194 shipped calls move from reversed to handled.
