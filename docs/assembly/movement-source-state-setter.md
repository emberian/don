# Sim-owned movement source-state setter

Status: implemented as a source-only convergence handoff; remote compilation and execution are
still required before promotion.

## Owner and transaction

`LiveCollisionRuntime` remains the sole owner of the `LiveCollisionSource` consumed by the
retail-ordered collision and movement adapter. Installation now binds each source to the exact
generational `Handle` that created it and starts an owner-local action-state revision at zero.

The exact mutation seam is:

```rust
Sim::movement_source_state(actor)
Sim::compare_exchange_movement_source_state(
    actor,
    expected_revision,
    moving,
    action,
)
```

The immutable snapshot identifies `{actor, row, revision, moving, action}`. The compare-exchange
validates the live handle, active generated row, installed identity, and expected revision before
changing either action field. Its receipt contains complete before and after images. A successful
request advances the wrapping revision exactly once, including a same-value request, so the same
prepared action cannot be replayed silently.

`Sim::set_movement_source_state(actor, moving, action)` preserves the previously documented small
surface as a current-revision wrapper. Backend transactions should use the snapshot/CAS pair.

## Atomic refusals

The owner distinguishes:

- a stale generational handle (`StaleActor`);
- a live row whose installed source belongs to a different handle after compaction
  (`ForeignSource`);
- a request prepared against an earlier owner revision (`StaleSourceRevision`);
- the existing inactive-row and missing-source faults.

Every refusal occurs before either `facts.moving`, `facts.action`, or the revision changes.
The ordinary all-source preflight also checks the installed handle at every active row, so a
compaction mismatch cannot reach the collision adapter through a non-setter path.

## Checksum boundary

The generated world does not materialize separate `UnitData::moving` or `action_type` columns.
The installed source is therefore the state actually read by `movement_live`; the new API does
not create an environment sidecar. The walked `OrderList` remains the checksum-visible action
owner. Consequently this setter deliberately leaves `World::digest()` unchanged, and the future
authoritative action transaction must perform the revision-checked source transition immediately
before `Sim::issue`, whose order replacement changes the walked digest.

`crates/don-sim/tests/movement_source_state_setter.rs` freezes coherent receipts, stale-revision
atomicity, stale/foreign identity refusal after row compaction, digest ownership, and the simple
compatibility wrapper.

Independent convergence gates passed on 2026-08-09: hbox
`movement-source-state-20260809T215933Z-42529-17110-9f11e3ea7d82` and persvati release
`movement-source-state-release-20260809T215933Z-42532-24374-9f11e3ea7d82`, each with 4/4
focused tests and exit 0.
