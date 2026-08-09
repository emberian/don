# `unitrules.xml` structural binder

Status: the source registry boundary is implemented and fail-closed. The 55 text cells are
retained exactly, but the full `UnitType::init` text-to-memory transforms and the later graft
copy set are not yet claimed.

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

## Important correction: `GRAFT`, not same-name inheritance

The live-table derivation originally called several inherited runtime values “same-`NAME`
canonicalisation.” Subsequent value-level comparison resolved the mechanism: the runtime
`graft` field matches the XML `GRAFT` target in all 364 rows, and all six observed scalar
divergences are consistent with copying from that graft source. This is not a generic
first-same-name-wins rule.

The structural binder retains the exact `GRAFT` cell and all rows. It does not yet apply a
graft pass because the precise set and order of fields copied by retail remain underived.
Likewise it does not turn prose-bearing cells such as `SUPPORT`, `RANGE`, or `JOB_EXTRA_TIME`
into approximate numbers. Callers that need a runtime `UnitTypeData` image must fail closed
until those loader transforms and post-load passes are recovered.
