# Leader Unit-stat runtime source boundary

Status: source-archive compile boundary closed; runtime product installation remains explicit.

`Leader::calc_unit_stats` needs ten scalar/relation fields from every post-load retail
`UnitTypeData` row. Five helicopter flag bits are added during retail initialization, so parsing
`unitrules.xml` alone is not an exact replacement for the observed post-load table.

The raw capture at `schema/live/live-tables-unit.tsv` remains tracked local evidence and remains
`export-ignore`. `don-sim` no longer reads it with `include_str!`, embeds its bytes, or assumes it
exists beside a compiler. Instead, the product host may construct `leaders::UnitTypeStatSource`
at runtime from the user's local capture and install it on `Sim`.

Admission is fail closed:

- the caller supplies the supported retail executable SHA-256 and the SHA-256 it computed over
  the exact TSV bytes;
- the source accepts only those two supported identities;
- parsing requires all ten named columns, exactly 364 rows, every consecutive type id from 50
  through 413 exactly once, valid signed integers, and only in-table `from`/`graft` relations;
- installation is one-time and is refused after the first frame, player activation, or object
  population, so a live checksum channel cannot switch sources silently;
- the catalog is absent by default; an automatic query then clears stale generated packages,
  records a miss, and leaves prior walked speed/armor values unchanged;
- DoNSave rejects an installed external source because the current format has no provenance or
  row owner for it.

This is a data-supply boundary, not a completion claim for step 8. Building/wall query
population, construction time, reached ejection, and the remaining Unit stat tails retain their
existing closure status.

Focused verification:

```text
cargo test -p don-sim systems::leaders::tests
cargo test -p don-sim --test leaders_unit_stat_source
cargo test -p don-sim --test bhs_type_stat_integration
git archive HEAD | tar -x -C <empty-directory>
cargo check -p don-sim --manifest-path <archive>/Cargo.toml
cargo test -p don-sim --manifest-path <archive>/Cargo.toml --test leaders_unit_stat_source
cargo test -p don-sim --manifest-path <archive>/Cargo.toml --test bhs_type_stat_integration
```

Those archive commands compile and run with no `schema/live/live-tables-unit.tsv`. The one
local-capture agreement unit test reports a skip in that environment; structural admission,
provenance refusal, positive installed-source execution, and absent-source behavior remain covered
by source-only fixtures. The former `gather_terrain.rs` test-only include of export-ignored
`ron-data/rules.xml` is closed the same way: an archive-safe synthetic parser fixture plus an
optional runtime local-file agreement test.
