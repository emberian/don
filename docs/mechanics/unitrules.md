# `unitrules.xml` structural binder

Status: the source registry boundary and the recovered pass-2 scalar materializer are
implemented and fail-closed. The 55 text cells are retained exactly. The materializer covers
the instruction-backed scalar tranche listed below; it does not claim the remaining
`UnitType::init` transforms or later relative/link passes.

Implementation: `crates/don-content/src/unitrules.rs`.

## Closed shipped shape

The extracted shipped `ron-data/unitrules.xml` is 629,942 bytes with SHA-256
`09e0b35c20d149083fafac12c5d2182951d1f1878bfc372ad99d6004e2aceabb`. It has exactly 364
`UNIT` records. Their source positions map directly to global `TypeIndex` 50 through 413,
matching the 364 live `UnitType` records and the PDB enum boundaries.

Every record has exactly these 55 children, in this order:

```text
NAME GRAPH OBJ_MASK FLAGS WHERE ATTACK HITS MOVES SUPPORT COST JOB_TIME PREQ0 PREQ1
FROM JUMP GRAFT RANGE LOS SCIENCE_LOS FLY_HIGH FLY_LOW RECHARGE ARMOR DOMAIN TO_HIT
ATTENUATE CAT PROGRESSION SPLASH SPLASH_PERCENT AMMO_PER_ATT TURN_SPEED PROJ_SPEED
CARRY_SIZE POP RESEARCH_PREMIUM_TIME RESEARCH_PREMIUM_COST JOB_EXTRA_TIME MANA TRIBE_MASK
CARRY GUY_SPACING X_SPACING Y_SPACING CIRCLE_RADIUS BLOCK_RADIUS TARGET_SIZE UBER_SIZE
CREW_SIZE GRID_X GRID_Y UPGRADE PUSH_SIZE PUSH_CIRCLES TYPENAME
```

The stale inline DTD is not used as a schema: it omits shipped fields and names legacy fields
that are absent from all 364 records. The parser accepts no wildcard field, attribute, nesting,
reordering, missing record, or surplus record. `FLAGS` and the legacy `UPGRADE` cell are the
only fields allowed to be empty in this shipped boundary.

## Identity and repeated names

`NAME` is deliberately non-unique: 364 rows contain 300 distinct exact names, in 33 repeated
groups (64 rows beyond the first occurrence). `TYPENAME` has only 352 distinct values and
`GRAPH` only 351, so neither repairs the collision. The content catalog therefore exposes two
identities:

- runtime identity: `TypeIndex = 50 + source_row`;
- source identity: `(exact NAME text, zero-based same-NAME occurrence in source order)`.

Lookup is exact and case-sensitive. No row is merged or overwritten by a name-keyed map.

## Five passes, consecutive-name reuse, and independent `GRAFT`

This corrects both earlier explanations of the six German-variant/live-table scalar
differences. `GRAFT` correlates with those rows, but it does not cause the scalar reuse.

`Types::init` calls `UnitType::init` five times for every TypeIndex 50..413, with pass values
0 through 4. Passes 0 and 1 use each row's own XML element. For passes 2 through 4,
`0x0066B620..0x0066B647` compares the current and retained source `NAME` through
`String::operator==` (`0x00A1F140`). An equal name preserves the retained element and source
TypeIndex; a different name replaces both with the current row. The retained state persists
across the loop, so every consecutive equal-name run uses its first row. A later,
non-consecutive occurrence begins a new run. This is an instruction-level property, not a
name-keyed catalog merge: runtime identity remains the row's own TypeIndex.

Pass 1 independently parses `GRAFT`. `Types::unit_key` searches `TYPENAME` in TypeIndex
50..401 order and returns the first case-insensitive match; Gaia rows are not candidates.
`none` and `disable` resolve to `-1` and `-2`. The implementation matches the captured live
`graft +0x25C` value for all 364 rows. The six formerly attributed scalar differences occur
where this separate graft target happens also to be the first row of the equal-`NAME` run.

## Recovered scalar tranche

`UnitRuntimeCatalog::materialize_runtime_scalars` applies the pass-2 source rule and the
recovered integer transforms for job time; object/unit masks; attack, hit, armor, LOS,
recharge, splash, ammo and projectile fields; spacing and radii; movement/carry; premium
cost/time; job-extra time; mana/control/progression; push/target size; and uber/crew size.
It takes the three size constants from parsed `rules.xml`, rejects zero/trapping derived
radius division, and uses checked `_wtoi`/`AsScaled` admission at the still-underived CRT
overflow boundary.

Against the retained live table, all compared values match for all 364 shipped records.
This is Tier C instruction recovery plus a captured live-state comparison, not a retail
differential. `unit_flags_from_xml` is deliberately only the XML bit subset;
`UnitType::init_final_flags` adds five shipped helicopter bits later.

Still outside the runtime catalog are `SUPPORT`, `COST`, `RANGE`, `TURN_SPEED`, domain and
reference/link fields, tribe masks, grid placement, and the complete effects of passes 3 and
4. Consumers needing a full `UnitTypeData` image must continue to fail closed at those
boundaries.
