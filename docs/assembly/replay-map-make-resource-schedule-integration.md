# Replay `Map::make` resource schedule integration

## Result

The replay crate now owns one typed source path from the landed post-nubify
transition receipt to the first unresolved instruction inside
`Map::place_resources`:

```text
post_nubify_transitions checkpoint 0x0068c12a / token 0x1ebe
  -> no-RNG/no-Map-write caller gap 0x0068c12f..0x0068c707
  -> conditional direct call at 0x0068c707
  -> Map::place_resources entry 0x0068f4f0
  -> deterministic ResourceDivvyPool prefix
  -> typed open body boundary 0x0068f597
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
  at `0x0068f597`.

Pool execution uses a staged copy. Bad catalog shape, stale evidence, or any
continuity failure leaves the caller's pool unchanged. A successful body
boundary carries the exact entry Map digest plus the complete pool mutation
receipt. It does not claim that the pre-mutation whole-Map digest remains the
post-mutation digest.

The body after `0x0068f597` remains open. It reads selected map/default XML,
performs later resource placement and RNG work, returns to `0x0068c70c`, and
eventually reaches caller checkpoint `0x0068c72d` / token `0x1ef7`. The body
boundary records that checkpoint as pending, never as completed.

## Schedule ownership

`MAP_MAKE_SCHEDULE` now places `resource_caller_gap` between
`post_nubify_transitions` and `place_resources`. The gap row has no terminal
checkpoint because its last internal checkpoint at token `0x1ec8` precedes
diagnostics and the endpoint gate. Both internal deadlines remain exact in
`MapMakeResourceCallerGapReceipt` rather than being misrepresented as a
stage-ending checkpoint.

The `place_resources` schedule row now states the executable ownership split:
zero RNG through the pool prefix, followed by an open body at `0x0068f597`
before the known direct and callee RNG sites.

## Focused proof

`crates/don-replay/tests/map_make_resource_schedule_integration.rs` freezes:

- schedule ordering from post-nubify transitions through resource placement;
- token `0x1ebe`, both caller-gap internal deadlines, exact entry/call VAs,
  and the pending token `0x1ef7` chronology;
- unchanged RNG, World checksum, and sourced-byte provenance through the gap
  and pool prefix;
- Map object identity and entry-digest continuity without a fabricated
  post-mutation Map digest;
- the skipped, missing-live-facts, and body-open typed residuals; and
- rollback for stale post-nubify or pool evidence.

The focused local commands are:

```text
cargo test -p don-replay --test map_make_resource_schedule_integration
cargo test -p don-replay --test map_make_resource_caller_gap_frontier
cargo test -p don-replay --test place_resources_pool_frontier
cargo test -p don-replay --test map_make_nubify_integration
```

No retail process is required or used by these source proofs.

## Files

```text
crates/don-replay/src/lib.rs
crates/don-replay/src/map_make_resource_schedule_integration.rs
crates/don-replay/src/map_style.rs
crates/don-replay/tests/map_make_resource_schedule_integration.rs
docs/assembly/replay-map-make-resource-schedule-integration.md
docs/assembly/replay-place-resources-pool-frontier.md
```
