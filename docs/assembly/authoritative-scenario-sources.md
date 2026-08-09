# Authoritative scenario movement-source tranche

Status: implemented and remotely validated in `don-env`.

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
destination, queue/flags, complete collision sources, actor capability, and installed action
state all pass. Every unhosted verb is clear. Masking never borrows a mutable store.

The contract tests in
`crates/don-env/tests/authoritative_scenario_sources_contract.rs` freeze:

- immediate MOVE_TO availability after scenario construction, with no manual installer;
- identical allocation handles, world digest, WData anchors/stamps, live sources, and MOVE_TO
  mask after reset;
- typed out-of-range, duplicate, and live-host source refusals;
- the exact `{NOOP, MOVE_TO}` mask for a ready request and MOVE_TO removal for a bad queue,
  destination, or owner;
- no world, WData, live-source, or collision-order-state mutation during masking.

## Honest red boundary: action source-state transition

Scenario capture removes the out-of-band installation/reset problem. It does not make
`UnitData::moving` or `UnitData::action_type` into scenario constants. Those are current-action
state and must transition when MOVE_TO or FLEE_TO is issued.

The backend cannot implement that mutation honestly today:

- `LiveCollisionRuntime::source(row)` returns only `&LiveCollisionSource`;
- `install(...)` is one-shot and returns `SourceAlreadyInstalled` on replacement;
- `InstalledSource` and its row store are owned by `don-sim`;
- the retail-ordered movement tick reads that installed source, so a `don-env` sidecar would
  be dead state.

`ActionSourceStateRequest { actor, state: { moving, action } }` and
`ACTION_SOURCE_STATE_SETTER` freeze this as
`IntegrationBoundary::MovementSourceStateHost`, with this required core seam:

```rust
Sim::set_movement_source_state(
    actor: Handle,
    moving: bool,
    action: OrderIndex,
) -> Result<usize, LiveCollisionFault>
```

The core implementation must resolve the live handle, require an active installed source, and
validate everything before changing `facts.moving` and `facts.action`. On fault, both runtime
source and world must remain byte-for-byte unchanged. The backend integration order is then:

1. run the existing immutable request/collision/actor preflight, excluding the old equality
   requirement for the already-installed action state;
2. set `{moving: true, action: MoveTo|FleeTo}` through the Sim-owned seam;
3. call `Sim::issue` (the preflight has already proved the handle live, which is the only
   rejection in `Sim::issue`);
4. remove `MovementSourceState` as a pre-priming requirement from MOVE_TO masking.

Until that core-owned setter lands, captured sources can make MOVE_TO ready deterministically
by capturing the currently observed state, but the typed red boundary prevents claiming that
arbitrary action-state transitions are hosted.

## Validation and benchmark evidence

`crates/don-env/examples/authoritative_source_bench.rs` measures three separately reported
operations over 64 fully sourced units:

- construct plus source installation;
- reset plus deterministic source reinstallation;
- one complete conditional verb-mask pass.

The exact overlay passed on 2026-08-09:

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
that persvati job rather than a portable performance promise.

The reusable commands are:

```sh
cargo test -p don-env --test authoritative_scenario_sources_contract
cargo run --release -p don-env --example authoritative_source_bench
```

The harness uses `black_box`, fixed scenario data, and independent iteration counts; it does
not mix frame stepping into setup or mask costs.
