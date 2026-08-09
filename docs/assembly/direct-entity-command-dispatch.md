# Direct market/entity command dispatcher integration

This closes the dispatcher seam for command rows 46 through 49 without overstating the
two addressed-object tails.

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
| 48 | unqueue | `state_wired` | Decode, target identity, active/UID guard, and inactive/stale no-op are exact; reached `Unit::action_unqueue` / `Build::action_unqueue` remains open. |
| 49 | come out | `state_wired` | Decode, unit identity, active/UID guard, and inactive/stale no-op are exact; reached `Unit::action_come_out` remains open. |

Rows 48 and 49 therefore stay red in `don-closure`.  A receipt may validly report
`Complete` for an inactive or stale target, but a matching active target is represented as
`OpenTail`; the static row cannot become green until every reachable action body is ported.

## Focused proof

`command_direct_entity_dispatch.rs` exercises the real packet walker and dispatcher.  It
proves that buy and sell mutate market/leader/counter state through the existing economy
primitives, that their receipts validate at the bridge frame, and that inactive/stale
entity arms complete while a live matching target remains an open tail.  The closure
binary separately pins 46/47 green and 48/49 `state_wired`.
