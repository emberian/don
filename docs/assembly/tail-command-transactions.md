# Tail command transactions: opcodes 70, 71, 73, 78, 80

Status: token-only planner and dispatcher integration, 2026-08-09.

This is the largest coherent fixed-size non-group cohort left after market and directly
addressed entity integration. It closes executable no-tail branches but does not relabel a
dynamic row green merely because one input branch is complete.

| opcode | command | bytes | executable branch | first open boundary | row closure |
|---:|---|---:|---|---|---|
| 70 | resign | 5 | diagnostic decode only | `Player::resign(0)`, `leave_game`, and possible `Leader::defeat` | `state_wired` |
| 71 | quit | 7 | all three wire fields retained | pre-quit restart gates, `Player::quit(0)`, report, and system-quit callback as one transaction | `state_wired` |
| 73 | leader options | 33 | exact row store when no observed cascade/local mirror is reached | object/unit cascades or local option mirror | `state_wired` |
| 78 | console command | 521 | diagnostic-only null-console branch | coordinate stores plus `ConsoleWin::parse_cmd` | `state_wired` |
| 80 | ungraceful drop | 3 | diagnostic-only non-network branch | `DropControl::process_drop` in network mode | `state_wired` |

`TailDecision::Apply` contains every effect of one reached no-tail branch. A host may return
an applied receipt only after committing that complete plan atomically. `TailDecision::Boundary`
is observational: its prefix authorizes no mutation, and an adapter must return unavailable.

Rows 70 and 71 are always boundary decisions. Rows 73, 78, and 80 remain red because their
decision depends on live facts even though their no-tail branches can apply.

## Exact closure delta

The static command closure changes exactly five rows:

```text
70  inert -> state_wired
71  inert -> state_wired
73  inert -> state_wired
78  inert -> state_wired
80  inert -> state_wired
```

No row becomes `complete`. `NUM_INLINE_COMMANDS` changes from 41 to 46. The complete-row
set is unchanged.

## Frozen paths

- `crates/don-sim/src/systems/tail_command_transactions.rs`
- `crates/don-sim/tests/tail_command_transactions.rs`
- `crates/don-sim/src/command.rs`
- `crates/don-sim/src/command_tables.rs`
- `crates/don-replay/src/bin/don-closure.rs`
- `crates/don-sim/tests/command_tail_dispatch.rs`
- `docs/assembly/tail-command-transactions.md`

The adapter composes, without modifying:

- `crates/don-sim/src/systems/adjacent_command_prefixes.rs`
- `crates/don-sim/src/systems/late_command_plans.rs`

## Validation commands

No Cargo, rustc, tests, formatters, or remote jobs were run in this lane. These commands are
frozen for the validation owner and are unexecuted here:

```text
cargo test -p don-sim --test tail_command_transactions
cargo test -p don-sim --test command_tail_dispatch
cargo test -p don-replay --bin don-closure
cargo run -q -p don-replay --bin don-closure | rg '^OPCODE\t(70|71|73|78|80)\t'
cargo fmt --all -- --check
```

The non-build whitespace check below was run successfully after the final edit:

```text
git diff --check -- crates/don-sim/src/systems/tail_command_transactions.rs crates/don-sim/tests/tail_command_transactions.rs crates/don-sim/src/command.rs crates/don-sim/src/command_tables.rs crates/don-replay/src/bin/don-closure.rs crates/don-sim/tests/command_tail_dispatch.rs docs/assembly/tail-command-transactions.md
```
