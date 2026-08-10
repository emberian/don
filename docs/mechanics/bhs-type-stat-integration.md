# BHS type-stat canonical integration

Status: executable through the opaque session with focused validation complete. Checksum channel 13 and
DoNSave v6 remain deliberately refused.

This integration makes registrations 529, 531–535, 538, and 814 executable through the one
`BhsSession` owner. It does not create a second type table or a detached gameplay facade.

## Transaction boundary

`plan_type_stat_mutation` still derives the instruction-equivalent relation-family writes from an
immutable `TypeBuiltinState`. `TypeBuiltinState::apply_type_stat_plan` now preflights every old
field value and every `modified` value before the first store. A stale row returns the typed
`StaleWrite` fault with no partial writes, no dirty bit, and no revision advance. An admitted plan
commits the full family and advances the owner revision once.

`TypeBuiltinRuntime` admits only the exact generated `don_bhs::builtin(index)` descriptor: name,
arity, scalar parameter types, return type, and handler VA all remain generation-bound. Its typed
request records the recovered `TypeStatBuiltin`, lookup name, and signed input. A direct runtime
call publishes the required Leader tail with `leader_recalcs_applied=false`; detached tests cannot
silently claim production completion.

## Immediate Leader tail

The live script host applies every emitted recalculation before returning the integer result to
the VM. For registrations 529, 531–535, and 814 that is wall then unit; registration 538 is unit
only. Slots and order come from the exact `(leader_flags & 3) == 3` scan frozen in the plan.

Because BHS runs at step 4, before the normal step-8 view refresh, the host refreshes each selected
owner's unit/build/wall bands from the authoritative object registry first. It projects mutated
hits, LOS, armor, and movement speed from the canonical type rows, invokes the recovered
`calc_wall_stats` / `calc_unit_stats` bodies, and commits their live object outputs before the
script resumes. Structural unit facts come from the provenance-bound post-load Unit source that
the product host explicitly installs from the user's local retail data; it is not part of the
redistributable source tree. Without that source, cache derivation records a miss and preserves
the walked values. Mutable armor and movement scalars are substituted only after the exact
structural package is rebuilt.

After application, the runtime confirms only the currently published index/request/revision
receipt. A delayed acknowledgement cannot bless a newer mutation. `BhsSession` therefore exposes
`leader_recalcs_applied=true` only after the joined `Sim` performed the immediate tail.

Session construction also compares every witnessed `LeaderTypeMasks::leader_flags` value with the
consumed simulation's canonical Leader flags before installation. A split gate owner is rejected;
the transaction planner and the live cache host cannot silently disagree about which slots retail
would recalculate.

## Boundaries preserved

- `BhsSession` still owns the only `Sim`, runtime, type state, and immutable composition
  provenance; it exposes no `Sim` escape hatch.
- DoNSave v6 refuses pristine and mutated installed owners because it has no type/provenance
  section.
- The partial checksum digest refuses every installed owner until the complete retail channel-13
  projection exists.
- Non-ASCII lookup remains a typed failure rather than an invented substitute for `_wcsicmp`.
- The shipped 194-call lexical census is reachability evidence. This source integration does not
  claim those calls dynamically executed across the shipped script corpus.

## Focused validation

Root convergence formatted the frozen overlap. Persvati batch
`gen7-integration-batch-v2-20260809T233915Z-61775-9459-05c01f206acb` passed all 23 BHS session,
runtime, frontier, and live-cache tests before a separate PlayerSetup fixture stopped the batch.
That unrelated fixture was fixed and validated separately. The focused reproduction is:

```text
cargo test -p don-sim --test bhs_type_stat_frontier
cargo test -p don-sim --test bhs_type_stat_integration
cargo test -p don-sim --test bhs_type_runtime_integration
cargo test -p don-sim --test bhs_session_owner
```

The integration test executes all eight
generated registrations through `BhsSession`, checks one atomic owner revision, exact return
identity, the wall/unit variation, cache-tail acknowledgement, retained provenance, and continued
save/digest refusal.
