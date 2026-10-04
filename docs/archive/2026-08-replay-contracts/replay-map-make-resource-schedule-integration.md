# Replay `Map::make` resource schedule integration

## Result

The replay crate now owns one typed source path from the landed post-nubify
transition receipt through the deterministic pool and XML bootstrap inside
`Map::place_resources`:

```text
post_nubify_transitions checkpoint 0x0068c12a / token 0x1ebe
  -> no-RNG/no-Map-write caller gap 0x0068c12f..0x0068c707
  -> conditional direct call at 0x0068c707
  -> Map::place_resources entry 0x0068f4f0
  -> deterministic ResourceDivvyPool prefix
  -> typed selected/default XML document and BONUSES-row owner
  -> typed open row-body boundary 0x0068fb9d
  -> optional typed first BONUS row through recurrence seam 0x00690215
  -> repeated typed later BONUS rows with replayed carry
  -> final no-RNG recurrence fallthrough at category tail 0x00690225
  -> typed BONUSES cleanup and FISH lookup/enumeration
  -> exact first FISH row through recurrence seam 0x00690215
  -> exact no-RNG row recurrence to 0x0068fbb3 or 0x00690225
  -> singleton FISH cleanup/GOODIES dispatch to first XML lookup
  -> capture-bound selected/default GOODIES lookup and row enumeration
     or, when FISH is empty, typed FISH cleanup and GOODIES lookup/enumeration
```

`execute_map_make_resource_schedule` constructs the caller-gap prior receipt
directly from `PostNubifyTransitionReceipt::next_checkpoint_*`,
`random_state_after`, `checksum_after`, and `sourced_walked_bytes`. It never
reconstructs the RNG state from a seed or equates independent captures by
convention. The caller evidence must bind the external Map/World/Game object
identities and the immutable Map digest at token `0x1ebe` to those values.

The caller gap then freezes both internal checksum deadlines (`0x1ec5` and
`0x1ec8`), the lazy bit-9/CTW/no-rare gate, and the exact callee entry. The
pool frontier receives that entry through a typed conversion, not through a
second caller-supplied RNG or World summary.

## Transaction and residual ownership

Three outcomes are explicit:

- `Skipped` preserves Map digest, World checksum, sourced-byte count, and RNG,
  then stops at the common caller continuation `0x0068c70c`.
- `EntryOpen` means retail admitted the direct call but the 44-good catalog and
  Ocean rare-list live facts were unavailable. The pool is untouched.
- `BodyOpen` owns the complete six-field `ResourceDivvyPool` prefix and stops
  at `0x0068f597` when XML facts are unavailable.
- `XmlRowsOpen` owns that same pool prefix plus path normalization, selected/default
  document lookup, BONUSES fallback, ordered BONUS rows, and live XML host references,
  then stops at `0x0068fb9d`.
- `FirstBonusRowOpen` owns the first row's chance/attribute/placement transaction and
  stops before `add esi, 0x28` at `0x00690215`. It retains the XML boundary and the
  authoritative concrete post-placement pool while token `0x1ef7` remains pending.
- `BonusRowsOpen` retains every later row's behavior facts and mutation receipt, the
  exact carried chance bucket, concrete pool, and recurrence chronology. Before a new
  row is admitted, the schedule reconstructs the first row and replays every stored
  later row against its placement receipt. The final row adds an explicit
  `0x00690215..0x00690225` fallthrough receipt and stops before category cleanup.
  A singleton array uses a tail-only continuation from its first-row boundary.
- `FishCategoryOpen` replays that complete BONUSES history, binds exact document-host
  handles to the category capture, performs the four conditional category/row releases,
  increments the ordinal, and performs the selected/default FISH lookup and ordered row
  enumeration. It stops before FISH row zero at `0x0068fbb3`, or at the next
  `0x00690225` cleanup when the selected/fallback FISH section is empty.
- `FirstFishRowOpen` replays the complete category boundary, reconstructs the inherited
  chance locals, composes row zero with the canonical exact Player/Region body, and commits
  canonical state plus the concrete pool atomically. It stops at `0x00690215` before the
  independently owned recurrence/next-category child.
- `FishRecurrenceOpen` first replays row zero from its exact before-image and retained
  two-phase allocation transcript. It then owns only `0x00690215..0x0069021c`: multi-row
  FISH stops before the next row at `0x0068fbb3`, while singleton FISH stops before cleanup
  at `0x00690225`. RNG, World, pool, counters, Good, and Item authority do not change.
- `FishCleanupOpen` consumes only the completed singleton branch. It releases the retained
  FISH category/current-row handles, increments and dispatches to GOODIES, and stops before
  the selected/default GOODIES `get_element` call at `0x0068f79b` or `0x0068f855`. It needs
  no section capture and preserves RNG, World, pool, counters, chance locals, documents,
  Good, and Item authority.
- `SingletonGoodiesCategoryOpen` replays the complete FISH row/recurrence/cleanup history,
  binds the original selected/default document handles to the GOODIES category capture, and
  executes the exact selected lookup plus default fallback when required. It stops before
  GOODIES row zero at `0x0068fbb3`, or at GOODIES cleanup `0x00690225` when no rows exist,
  with all deterministic and Goods authority unchanged.
- `GoodiesCategoryOpen` is the source-complete empty-FISH branch. It authenticates the
  unchanged document authority, performs FISH cleanup and GOODIES lookup/enumeration with
  no RNG/World/pool change, then stops at GOODIES row zero (`0x0068fbb3`) or GOODIES cleanup
  (`0x00690225`) when that section is empty.

Later continuations also revalidate the state-carrying caller, pool-prefix, and XML
RNG/World/pool chronology against each other. Descriptive capture provenance retained
inside the upstream receipts is admitted by the original constructor and is not
re-read from retail during continuation; “history replay” below refers specifically to
the behavior-driving BONUS row transactions.

Pool and XML execution use staged copies. Bad catalog shape, stale evidence, or any
continuity failure leaves both the caller's pool and XML host unchanged. A successful
XML boundary carries the exact entry Map digest, complete pool mutation receipt,
canonical digest of all six logical pool fields, validated XML capture evidence, and
the selected/default source plus live host ownership. It does not claim that the
pre-mutation whole-Map digest remains the post-mutation digest.

`resource_divvy_pool_digest` uses a domain-separated, field-tagged, length-delimited
FNV-1a encoding of the logical masks and ordered good IDs. It is a cross-host receipt
digest, not a hash of retail allocator-dependent native bytes. The XML evidence must
bind that digest together with the exact entry RNG state, World checksum, and sourced
walked-byte count before the XML owner can execute.

`continue_map_make_resource_schedule_first_bonus` binds the digest-only XML handoff
back to `pool_prefix.pool_after`, executes the row against a two-phase placement host,
and commits the caller's mutable pool only after every selector subreceipt re-executes.
Each selector subreceipt includes its exact before/after six-field pool, selected lane,
good/index, retry draws, and exhaustion clear. A selector row may therefore advance the
public pool without leaving it at the prefix state while publishing a newer digest.

All rows in a nonempty current `BONUSES` array, BONUSES cleanup/FISH dispatch, the first
nonempty FISH row, and its recurrence can now execute in the compiled schedule. That branch's
exact residual is `0x0068fbb3` when another FISH row remains. Singleton cleanup advances to
GOODIES row zero at `0x0068fbb3` (or cleanup `0x00690225` when empty). The empty-FISH branch advances to `0x0068fbb3` for nonempty GOODIES or
`0x00690225` for empty GOODIES. The zero-row BONUSES XML-to-category-tail bridge remains
open. Retail then executes FISH recurrence/later rows or GOODIES rows before returning to
`0x0068c70c`, and eventually reaches caller checkpoint `0x0068c72d` / token `0x1ef7`.
The boundary records that checkpoint as pending, never as completed.

## Schedule ownership

`MAP_MAKE_SCHEDULE` now places `resource_caller_gap` between
`post_nubify_transitions` and `place_resources`. The gap row has no terminal
checkpoint because its last internal checkpoint at token `0x1ec8` precedes
diagnostics and the endpoint gate. Both internal deadlines remain exact in
`MapMakeResourceCallerGapReceipt` rather than being misrepresented as a
stage-ending checkpoint.

The `place_resources` schedule row now states the executable ownership split:
zero RNG through the pool and XML bootstrap, typed direct/callee RNG and concrete pool
continuity for every row of a nonempty current BONUS array, the exact category-tail
fallthrough, BONUSES cleanup/FISH dispatch, and exact first FISH row. Empty FISH cleanup
and GOODIES enumeration are also owned. The zero-row BONUSES bridge, FISH recurrence/later
rows, GOODIES row bodies, and final document cleanup remain open.

## Focused proof

`crates/don-replay/tests/map_make_resource_schedule_integration.rs` freezes:

- schedule ordering from post-nubify transitions through resource placement;
- token `0x1ebe`, both caller-gap internal deadlines, exact entry/call VAs,
  and the pending token `0x1ef7` chronology;
- unchanged RNG, World checksum, and sourced-byte provenance through the gap
  and pool prefix;
- Map object identity and entry-digest continuity without a fabricated
  post-mutation Map digest;
- the skipped, missing-live-facts, pool-open, and XML-row-open typed residuals;
- canonical six-field pool digest and exact selected/default XML source/row ownership;
- unchanged RNG/World/sourced-byte provenance through `0x0068fb9d`; and
- first-row compiled continuation through `0x00690215`, including both a chance-miss
  path and a selector path that mutates the caller-visible concrete pool; and
- end-to-end public-pool rollback when a selector subreceipt is corrupted after the
  row's direct chance draw; and
- repeated later-row continuation with full BONUS-row history replay, exact shared-RNG chaining,
  concrete-pool continuity, final recurrence fallthrough to `0x00690225`, and refusal
  to execute another row after the category is complete;
- singleton tail-only advancement and rejection of a removed or forged category-tail receipt;
- exact BONUSES host cleanup and selected FISH dispatch with unchanged RNG, World, pool,
  counters, and chance carry; capture-bound document-host continuity; and an explicit stop
  before the first FISH row;
- exact carried first-FISH chance/placement composition with atomic canonical/pool commit and
  an explicit `0x00690215` recurrence residual;
- retained allocation proposals, exact row-zero replay, and no-mutation FISH recurrence to the
  next row or category cleanup;
- singleton FISH handle cleanup and GOODIES dispatch through the first unresolved XML lookup;
- selected/default GOODIES lookup fallback, capture-bound document authority, and ordered rows;
- empty-FISH cleanup and GOODIES lookup/enumeration with unchanged RNG, World, and pool;
- a later selector row whose direct chance draw, callee pool draw, concrete bitmask
  mutation, and published digest form one chronology, plus schedule-level rollback for
  a corrupted selector subreceipt;
- fail-closed later-row evidence rollback before public pool or schedule carry changes;
- rejection of state-carrying caller/XML prefix tampering before a later-row host call;
- joint pool/host rollback for stale XML evidence.

The focused local commands are:

```text
cargo test -p don-replay --test map_make_resource_schedule_integration
cargo test -p don-replay --test map_make_resource_caller_gap_frontier
cargo test -p don-replay --test place_resources_pool_frontier
cargo test -p don-replay --test place_resources_xml_frontier
cargo test -p don-replay --test resource_divvy_pool_selection_frontier
cargo test -p don-replay --test place_resources_bonus_mutation_frontier
cargo test -p don-replay --test place_resources_bonus_rows_mutation_frontier
cargo test -p don-replay --test map_make_nubify_integration
```

No retail process is required or used by these source proofs.

## Files

```text
crates/don-replay/src/lib.rs
crates/don-replay/src/map_make_resource_schedule_integration.rs
crates/don-replay/src/map_style.rs
crates/don-replay/src/place_resources_bonus_mutation_frontier.rs
crates/don-replay/src/place_resources_bonus_rows_mutation_frontier.rs
crates/don-replay/src/resource_divvy_pool_selection_frontier.rs
crates/don-replay/src/place_resources_xml_frontier.rs
crates/don-replay/tests/map_make_resource_schedule_integration.rs
crates/don-replay/tests/resource_divvy_pool_selection_frontier.rs
crates/don-replay/tests/place_resources_bonus_rows_mutation_frontier.rs
docs/assembly/replay-map-make-resource-schedule-integration.md
docs/assembly/replay-place-resources-pool-frontier.md
docs/assembly/replay-resource-divvy-pool-selection-frontier.md
```
