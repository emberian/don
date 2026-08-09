# Graphics-turret Guy materialization

**Code:** `crates/don-sim/src/systems/graphics_turret.rs`
**Retail build:** `riseofnations.exe` SHA-256 `30478a44…625079`
**Installed data:** `Data/unit_graphics.xml`, 3,440,809 bytes, SHA-256
`f01b091f…08e54d`

## Result

The simulation now has an exact, fail-closed boundary for the graphics state that makes a
Guy a turret body. It does not classify a UnitType as a tank or ship, and it does not
substitute zero node offsets. It accepts only the supported installed XML plus an exact
retail or `.bh3` hierarchy resolver.

The API separates two identities which the installed file proves are not interchangeable:

- the UnitType `<GRAPH>` selects `<UNIT name="GRAPH-…">` records;
- `GraphicPieces::get_type(gpiece)` selects a pivot-restriction graph.

For example, UnitType graph `BATTLESHIP` selects a model whose pivot graph is
`SuperBattleship`. Turret state therefore belongs to each selected Guy gpiece, not to one
unit-wide numeric flag.

## Retail initialization

`Guy::init_real` `0x005DB6B0` calls the helper at `0x005D8530` to select an `RData`/gpiece.
PDB layouts and the instruction stream establish:

- `RData +0x90` is `gpiece`, `+0x88` is scale, and `+0x94` is `UnitRDataStruct*`;
- `UnitRDataStruct +0x0C/+0x10` are `track_offsetx/track_offsety`;
- Guy 0 gets zero track offsets; later Guys receive the float product of installed track
  offsets, the retail world scale and `RData.scale`, truncated by x86 conversion;
- current and desired pivot arrays are initialized from the selected loaded hierarchy;
- `GraphicPieces::get_type(gpiece)-0x32` indexes `pivot_restrictions`;
- a non-empty list sets raw `GuyData +0x9A & 0x0100` (`GUY_FLAG_TURRETS`).

The supported XML contains 1,435 `<UNIT>` rows, 355 UnitType graph prefixes and 81 ordered
`<RESTRICTION>` rows across 66 pivot graph names. Restriction nodes are sequential `4..=7`.
Retail appends them in file order. Name matching goes through `String::operator==`
`0x00A1F140`, whose equal-length arm calls `_wcsicmp`; relevant shipped names are ASCII, so
the catalog uses ASCII case-insensitive keys.

`UnitGraphicsCatalog::from_installed_xml` admits only the supported executable/data
identity, coherent capture flag, exact file length, exact row counts, valid numeric fields
and sequential nodes. `materialize_unit_graphics` then validates every null slot, `guy_num`,
gpiece, pivot graph and node flag before committing any write to `UnitGuys`. Errors leave the
entire unit unchanged.

## Live pivot aiming

PDB names `0x005D8BC0` `Guy::set_all_pivots`. For each restriction node it:

1. calls `GraphicPieces::get_position(gpiece, node, …)` with
   `float(angle_to_degrees(guy.angle - 0x80000000))`;
2. truncates the returned local x/y offset and computes target bearing from the owning Unit
   anchor minus that offset;
3. stores the bearing relative to the Guy body when it falls inside the inclusive
   restriction interval, including wrapped intervals where `min >= max`;
4. clears then rebuilds desired/current node bits; the current bit uses the retail folded
   unsigned delta and strict `< 0x0AAAAAAA` comparison;
5. returns unaligned when any desired angle is outside ±45 degrees or outside its pivot
   restriction.

`resolve_turret_aim` ports those integer and wrapping rules. `PivotOffsetProvider` is the one
mandatory host seam: it must evaluate the selected loaded `.bh3` hierarchy. All requested
offsets are gathered and validated before Guy state changes, so a missing node or provider
failure cannot leave a partial pivot update. The returned `aligned` bit can be passed directly
as `AimMode::GraphicsTurret { aligned }`.

## Exact remaining boundary

`unit_graphics.xml` names `.bh3` models but does not contain their hierarchy transforms.
Consequently XML alone cannot produce initial pivot angles, crew attachment positions or the
per-frame local node offsets. Arena must still provide:

- a coherent per-Guy extractor result for gpiece, track offsets and initial pivot state;
- the pivot graph identity returned for that gpiece; and
- a `GraphicPieces::get_position`-equivalent provider over loaded `.bh3`/RData.

Until that host exists, `AimMode::UnresolvedGraphicsTurret` remains the correct fidelity-mode
result. Treating the body as an ordinary non-turret Guy or using a zero-offset provider is not
an accepted fallback.

## Verification

- Ten hermetic catalog/materialization/aim tests cover case-insensitive names, ordered and
  circular restrictions, transactional failure, non-turret rejection, strict slew flags,
  provider arguments and non-mutation on hierarchy failure.
- An ignored installed-data smoke test parses the user's exact supported file and verifies
  355 UnitType graph prefixes, 66 pivot graphs and all 81 restriction rows. Run it with
  `RON_UNIT_GRAPHICS_XML=/path/to/unit_graphics.xml cargo test -p don-sim \
  supported_installed_catalog_matches_measured_shape --lib -- --ignored`.
