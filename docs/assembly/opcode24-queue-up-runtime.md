# Opcode 24 QueueUp runtime cohort

This tranche binds the 9-byte `QueueUpCommand` (`type@1`, `num@5`) to an executable,
fail-closed ordinary-Unit production transaction. It does **not** promote opcode 24 or the
`queue_up` action to complete: research/build requests, Library forwarding, scenario
ignore-orders pruning, aircraft/unarmed families, and uninstalled producer/type compatibility
remain open.

## Retail body map

| Function | RVA | PDB size | Reached behavior |
|---|---:|---:|---|
| `Group::action_queue_up` | `0x006FDBB0` | 1,516 | building gate; copy/stable sort by `BuildData+0x82`; `action_begin`; ordinary Unit repetition loop |
| `Build::queue_up` | `0x00620F40` | 4,322 | `can_pay`; `could_queue`; exact cost capture; queue record/counters |
| `BuildData::could_queue` | `0x0062DA50` | — | type compatibility; `queued < BuildQueue::num`; aircraft special gate |
| `BuildQueue::queue` | `0x006309F0` | — | type at `+0x04`; first three non-zero `(resource, amount)` pairs |

`Group::action_queue_up` copies the selected object indices once and stably sorts them by
their initial queued byte: the emitted comparison swaps only when the left byte is greater,
so equal lengths retain selection order. It then repeats that fixed order `num` times.
The ordinary Unit path ignores the return from each `Build::queue_up`, allowing later
producers/repetitions to proceed after a full queue or failed payment.

Within `Build::queue_up`, the observed ordering is:

1. the type `+0x84` virtual payment predicate;
2. `BuildData::could_queue(type)` at `0x006214E3`;
3. Unit cap/caravan/aircraft feedback (presentation-only in this cohort);
4. the six-good `+0xD4` cost transaction at `0x0062195A`;
5. increment `BuildData::queued`, then `BuildQueue::queue` at `0x00621999`;
6. increment `LeaderData::queued[type]` and the reached training-family counters.

## Installed boundary

The canonical adapter uses `LiveProductionRuntime` facts already required by production:
an installed ordinary Unit, exact `repeat_cost` (the same `action_queue(type, 0)` price), an
armed fixed training-site projection, matching registered Build types, allocated queue
records, leader resources, and all three stockpile mirrors. It preflights and recomputes the
whole admitted transaction before the first write.

The real command-path test sends opcode 0 to select two live Build objects, then sends an
actual 9-byte opcode-24 packet. It proves stable short-queue-first distribution, queue-record
payloads, resource debits, aggregate/family counters, `Group::action_begin`, and Bridge
accounting. A missing cost/type projection leaves every owner unchanged and retains the
typed open action tail.

No command table, schema row, tick owner, or save/load code changes in this tranche.
