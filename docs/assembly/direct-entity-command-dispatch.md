# Direct market/entity command dispatcher integration

This closes the dispatcher seam for command rows 46 through 49 without overstating opcode 49's
remaining addressed-object tail.

## Executable boundary

`command::Bridge` now decodes all four exact wire layouts and sends one typed envelope to
`Fleet::apply_direct_entity_command_transaction`.  The envelope includes `Bridge::frame`,
so the returned receipt can be recomputed against the command's actual simulation frame.
The bridge retains the expected envelope, observed receipt, and validation result in
`DirectEntityReceiptRecord`.

The callback is atomic and fail-closed by default.  A host which does not own all reached
facts returns `Unavailable` and performs no mutation.

## Closure classification

| Opcode | Row | Dispatcher status | Why |
| --- | --- | --- | --- |
| 46 | buy | `complete` | The reached tail executes the existing deterministic `economy::do_buy` loop after a single complete preflight. |
| 47 | sell | `complete` | The reached tail executes the existing deterministic `economy::do_sell` loop after the sell-specific eligibility preflight. |
| 48 | unqueue | `complete` | Decode, target identity, active/UID guard, inactive/stale no-op, and both complete active Unit/Carrier and Build receivers are production-wired. |
| 49 | come out | `state_wired` | Decode, unit identity, active/UID guard, inactive/stale no-op, and the complete 532-byte `Unit::action_come_out` preflight route are exact; mandatory general `Unit::come_out(0)` remains open. |

Row 49 therefore stays red in `don-closure`. Opcode 48 reports `Complete` only after the canonical
production host has committed a validating active Unit/Carrier or Build transaction; missing
reached facts remain atomic `Unavailable`.

## Focused proof

`command_direct_entity_dispatch.rs` exercises the original packet walker, market transactions,
inactive/stale arms, and a host-supplied opcode-49 wrapper preflight which the bridge validates
while retaining `GeneralUnitComeOutTransaction { argument: 0 }` as the open tail.
`command_carrier_unqueue_integration.rs` drives a
matching active Unit through that same Bridge/Fleet callback into the canonical Sim owner and
checks the exact aggregate/family/refund/scratch mutations plus fail-closed lazy edges. The
same test now drives an active Build packet through selector/queue/counter/refund mutation. The
closure binary separately pins 46/47/48 green and 49 `state_wired`.
