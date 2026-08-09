# Group command prefix integration

Status: live dispatcher prefix wired; all seven reached action bodies remain open.

This tranche connects the previously isolated recovery in
`systems/unimplemented_group_command_plans.rs` to `command::Bridge` for opcodes 5, 6,
23, 24, 25, 28, and 35. It changes no action-body closure claim.

## Executable boundary

`Bridge::process_one` now routes these rows through their exact-length decoder before the
generic group action path. The bridge recomputes branches which read no object locally.
Only a nonnegative package group plus a nonnegative target pair crosses the read-only
`Fleet::group_command_prefix_receipt` fact boundary for the addressed object's `+0x08`
flag bit. The bridge accepts the result only when the receipt recomputes and echoes the
current package group.

The validated disposition is one of:

| disposition | meaning | mutation authorized |
|---|---|---|
| `ExactNoAction` | negative package group, or a resolved positive target whose flag bit 0 is clear | none; the retail handler returns before its action call |
| `OpenActionTail(DelegatedGroupAction)` | exact gates reach the recorded `Group::action_*` ABI | none; the action body is still outside this tranche and remains dynamically `unported` |
| `Unavailable` | target facts are absent, receipt recomputation fails, or the echoed group differs | none; fail closed and increment `unported` |

The in-memory `ObjectTable` host resolves the addressed-object flag in the same read-only
callback. The bridge plans requests which do not read that object locally, so the exact
negative-group and negative-target branches do not acquire a new dependency.

Each accepted receipt is drainable through
`Bridge::take_group_command_prefix_receipts`. Dynamic counters distinguish exact no-op
prefixes from reached open action tails; `acted` is not incremented for either, and
`ported_fraction` remains red for every reached open tail.

## Exact reached calls

| opcode | action | retained ABI detail | closure |
|---:|---|---|---|
| 5 | `siege_attack` | `(ox, whom, queued)` | `StateWired` |
| 6 | `swarm_around` | adds literal retail mode `1` after `(ox, whom, queued, orders)` | `StateWired` |
| 23 | `spell` | reorders wire fields to `(type_index, ox, whom, x, y)` | `StateWired` |
| 24 | `queue_up` | `(type_index, num)` | `StateWired` |
| 25 | `build` | `(x, y, x2, y2, type_index, queued)` | `StateWired` |
| 28 | `flight` | reorders to `(ox, whom, orders, shift, ctrl, alt)` | `StateWired` |
| 35 | `recall` | no arguments | `StateWired` |

`command_tables.rs` and the static `don-closure` inventory therefore move seven group
actions and their seven opcode rows from `unimplemented` to `state_wired`. The exact action
port cardinalities move from `[9, 14, 0, 5, 7, 7]` to `[9, 14, 0, 12, 0, 7]` in
`[Complete, Orders, State, StateWired, Todo, NotOnTheWire]` order.

Closure delta: **7 unimplemented to StateWired; 0 to Complete**.

## Focused pins

`tests/group_command_prefix_integration.rs` pins all seven live dispatch arms, including
the ABI reorderings, exact no-action versus unavailable distinction, negative-group and
negative-target object-read bypasses, forged/malformed receipt rejection, action counters,
and the zero-Complete table invariant. This lane was source-only by order: no compiler,
formatter, or test command was run here. The parent integration lane owns compilation and
runtime verification.

Root convergence passed both independent focused profiles on 2026-08-09: hbox
`group-prefix-integration-20260809T215933Z-42526-17128-9f11e3ea7d82` and persvati release
`group-prefix-integration-release-20260809T215933Z-42536-1196-9f11e3ea7d82`, each with 5/5
tests and exit 0. The checked-in closure JSON still requires regeneration from the compiled
inventory before this tranche is committed. The compiled closure inventory also passed 8/8 on
hbox as `group-prefix-closure-final-20260809T220301Z-45010-5395-9f11e3ea7d82` after supplying
its required `schema/live/final-balance-runtime.bin` asset.
