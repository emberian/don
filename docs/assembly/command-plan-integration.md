# Command-plan dispatcher integration

This integration makes the landed command proof plans reachable from the shared
`CommandPackage` dispatcher without moving any behavior into the tick, order executor, or
world store. The proof modules remain owned by `command.rs` through explicit path modules;
their tests no longer imply reachability by themselves.

## Closure decision

| Opcode | Command | Closure | Atomic transaction rule |
|---:|---|---|---|
| 37 | treaty | complete | Commit the full `DiplomacyPlanDecision::Apply` state and ordered effects. |
| 38 | declare | `state_wired` | Early no-op gates can apply; declaration cost/resource/`set_diplo` branches must return unavailable. |
| 39 | clear tributes | complete | Commit the full diplomacy plan. |
| 40 | clear all | complete | Commit the full diplomacy plan. |
| 41 | accept | `state_wired` | The transfer and `set_diplo` tail is not recovered and cannot apply. |
| 42 | reject | `state_wired` | The bounded pending-agreement branch can apply; hostile counterproposal branches cannot. |
| 43 | tribute | complete | Commit the full diplomacy plan, including refusal/no-op branches. |
| 44 | demand tribute | complete | Commit the full diplomacy plan. |
| 45 | propose attack | complete | Commit the full diplomacy plan. |
| 75 | rename city | complete | Commit lookup, optional city write, reached hot-key normalization/stamp, and both presentation calls as one receipt. |
| 77 | cannon time | complete | Commit every state, UI, audio, and wall-clock effect before returning the validated receipt. |

An unavailable receipt is a fail-closed no-op. An adapter must preflight the whole plan;
returning a forged or partially applied receipt violates the `Fleet` transaction contract.
Opcode 80 remains outside this integration because its network arm still delegates to the
unrecovered `DropControl::process_drop` tail.

## Frozen paths

- `crates/don-sim/src/command.rs` — path ownership, dispatcher routing, and atomic `Fleet`
  seams.
- `crates/don-sim/src/command_tables.rs` — per-opcode closure status.
- `crates/don-replay/src/bin/don-closure.rs` — emitted static closure and red-row
  assertions.
- `crates/don-sim/tests/command_plan_integration.rs` — transaction reachability and
  fail-closed boundary coverage.
- `docs/assembly/command-plan-integration.md` — this frozen integration ledger.

The landed proof inputs are read from these exact paths and are not modified by this lane:

- `crates/don-sim/src/systems/object_command_plans.rs`
- `crates/don-sim/src/systems/late_command_plans.rs`
- `crates/don-sim/src/systems/diplomacy_command_plans.rs`
- `crates/don-sim/src/systems/setup_diplomacy.rs`

## Validation commands

The token-only integration lane did not run Cargo, `rustc`, tests, formatters, or remote
jobs. The following commands are frozen for the later validation owner; they have not been
executed here:

```text
cargo test -p don-sim --test command_plan_integration
cargo test -p don-replay --bin don-closure
cargo run -q -p don-replay --bin don-closure | rg '^OPCODE\t(37|38|39|40|41|42|43|44|45|75|77)\t'
cargo fmt --all -- --check
git diff --check -- crates/don-sim/src/command.rs crates/don-sim/src/command_tables.rs crates/don-replay/src/bin/don-closure.rs crates/don-sim/tests/command_plan_integration.rs docs/assembly/command-plan-integration.md
```

Expected static statuses are `complete` for 37, 39, 40, 43, 44, 45, 75, and 77;
`state_wired` for 38, 41, and 42.
