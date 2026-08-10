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

The row body after `0x0068fb9d` remains open. It parses BONUS attributes, performs
later resource placement and RNG work, proceeds through GOODIES and FISH, returns to
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
zero RNG through the pool and XML bootstrap, followed by an open row body at
`0x0068fb9d` before the known direct and callee RNG sites.

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
- joint pool/host rollback for stale XML evidence.

The focused local commands are:

```text
cargo test -p don-replay --test map_make_resource_schedule_integration
cargo test -p don-replay --test map_make_resource_caller_gap_frontier
cargo test -p don-replay --test place_resources_pool_frontier
cargo test -p don-replay --test place_resources_xml_frontier
cargo test -p don-replay --test map_make_nubify_integration
```

No retail process is required or used by these source proofs.

## Files

```text
crates/don-replay/src/lib.rs
crates/don-replay/src/map_make_resource_schedule_integration.rs
crates/don-replay/src/map_style.rs
crates/don-replay/src/place_resources_xml_frontier.rs
crates/don-replay/tests/map_make_resource_schedule_integration.rs
docs/assembly/replay-map-make-resource-schedule-integration.md
docs/assembly/replay-place-resources-pool-frontier.md
```
