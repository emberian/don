# Authoritative scenario movement-source tranche

Status: scenario-source setup was remotely validated; the additive action-source CAS wiring
below is source-complete and awaits independent compilation/execution.

## Delivered contract

`AuthoritativeScenarioSpec` is an additive wrapper around the stable `ScenarioSpec`. Each
`ScenarioMovementSource` names a unit by scenario allocation ordinal and carries the exact
`LiveCollisionSource` consumed by the Sim-owned collision runtime.

Construction now has these properties:

1. Every ordinal and duplicate is checked before an episode is allocated.
2. The ordinary episode validates and spawns every unit.
3. Sources are installed in declaration order through
   `Sim::install_movement_collision_source`; a fault destroys the private candidate.
4. The declarations remain owned by `AuthoritativeBackend`.
5. `reset()` constructs a complete replacement, including the captured sources, before
   replacing the current backend.

The original `AuthoritativeBackend::from_spec` remains available and produces a setup with no
captured sources. The older `install_movement_source` remains explicitly out-of-band and does
not become persistent by accident.

`unit_verb_mask(who, template)` is a conditional mask over the 34-value generated verb head.
It holds actor, destination, queue, and flags fixed and runs the same read-only preflight used
by `apply_unit`. NOOP is therefore always set; MOVE_TO is set only when ownership, liveness,
destination, queue/flags, complete collision sources, actor capability, and a readable
identity-bound source snapshot all pass. The source's current `moving/action` values are not
an admission requirement: the action transaction owns that transition. Every unhosted verb
is clear. Masking never borrows a mutable store or retains a backend sidecar.

The contract tests in
`crates/don-env/tests/authoritative_scenario_sources_contract.rs` freeze:

- immediate MOVE_TO availability after scenario construction from an idle/`NONE` source,
  with no manual installer or pre-priming;
- identical allocation handles, world digest, WData anchors/stamps, live sources, and MOVE_TO
  mask after reset;
- typed out-of-range, duplicate, and live-host source refusals;
- the exact `{NOOP, MOVE_TO}` mask for a ready request and MOVE_TO removal for a bad queue,
  destination, or owner;
- no world, WData, live-source, source-revision, or collision-order-state mutation during
  masking.

## Action source-state transaction

Scenario capture removes the out-of-band installation/reset problem without treating
`UnitData::moving` or `UnitData::action_type` as scenario constants. MOVE_TO/FLEE_TO now uses
the Sim-owned snapshot/CAS seam:

```rust
Sim::movement_source_state(actor)
Sim::compare_exchange_movement_source_state(actor, expected_revision, true, order_kind)
Sim::issue(actor, order)
```

Read-only preflight captures `{actor,row,revision,moving,action}`. Commit revalidates the
ordinary request facts, then compare-exchanges `{moving: true, action: MoveTo|FleeTo}` against
that exact revision immediately before `Sim::issue`. The only code between the successful CAS
and issue is the issue call itself. A stale plan receives
`LiveCollisionFault::StaleSourceRevision` before its source, order list, or path stack changes.
Successful same-value actions still advance the revision, so a consumed plan cannot be replayed.

`AuthoritativeBackend::prepare_unit` exposes an optional opaque caller-owned plan for
schedulers which separate planning from commit. `apply_unit` uses the same plan internally in
one mutable-backend call. Neither path stores a plan or state mirror in the backend, and
`unit_verb_mask` remains observational. A backend-local episode revision binds explicit plans
across reset; this is lifecycle identity, not a mirror of source fields. It prevents a plan
prepared at source revision zero from becoming spuriously current when deterministic reset
recreates the same handles and a fresh source revision zero.

Reset reconstructs the captured scenario source and therefore resets its owner-local revision
to zero. Contract tests freeze idle-to-MOVE_TO, idle-to-FLEE_TO, repeated same-value revision
advance, stale-plan atomic refusal, exact reset of both source facts and revision, and rejection
of a pre-reset plan whose handle/source revision happen to repeat.

## Honest remaining action boundaries

This closes only the source-state prerequisite for the generated MOVE_TO verb. The contract
remains deliberately narrow:

- queue positions `First` and `Last`, and order-modifier bits other than the recovered fleeing
  bit, are refused;
- multi-Guy movers, boat solving, attack slack, and missing repath facts remain fail-closed in
  the underlying movement host;
- MOVE_NEAR, FOLLOW, and GUARD still require their distinct command/group transactions even
  though their eventual movement source can use this setter;
- the other 32 generated unit verbs and all 16 player verbs retain their typed group,
  formation, combat-target, containment, gathering, construction, production, spell,
  diplomacy, market, tribute, and lifecycle boundaries;
- external policy observations remain incomplete until cloak/detection-aware visibility is
  hosted.

## Validation and benchmark evidence

`crates/don-env/examples/authoritative_source_bench.rs` measures three separately reported
operations over 64 fully sourced units:

- construct plus source installation;
- reset plus deterministic source reinstallation;
- one complete conditional verb-mask pass.

The scenario-source overlay before the CAS wiring passed on 2026-08-09:

- hbox job `rl-scenario-sources-20260809T210604Z-82538-25710-e01ec386c154`:
  13/13 tests across `authoritative_scenario_sources_contract`,
  `authoritative_backend_contract`, and `authoritative_episode_contract`;
- persvati job `rl-scenario-check-20260809T210605Z-82544-13533-e01ec386c154`:
  `cargo check --locked -p don-env --all-targets` completed successfully;
- persvati release job `rl-scenario-bench-20260809T210604Z-82535-28729-e01ec386c154`:
  64 sources, 200 construction iterations, 500 reset iterations, and 20,000 mask
  iterations.

The measured release results were 624,906.8 ns per construct/install, 556,058.3 ns per
reset/reinstall, and 795.9 ns per complete 34-value conditional mask. These numbers describe
that persvati job rather than a portable performance promise. They do not validate or measure
the later action-source CAS overlay.

The later CAS overlay passed both independent profiles on 2026-08-09:

- hbox `rl-movement-cas-20260809T222656Z-68771-29864-fff511f71587`;
- persvati release `rl-movement-cas-release-20260809T222656Z-68772-15666-fff511f71587`.

Each exited 0 with 6/6 authoritative-backend and 5/5 scenario-source tests. These receipts pin
the source transition immediately before `Sim::issue`, competing stale-plan non-mutation, and
reset-episode fencing; they do not close the other generated action hosts listed above.

The reusable commands are:

```sh
cargo test -p don-env --test authoritative_scenario_sources_contract
cargo run --release -p don-env --example authoritative_source_bench
```

The harness uses `black_box`, fixed scenario data, and independent iteration counts; it does
not mix frame stepping into setup or mask costs.
