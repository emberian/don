# Six-action group receiver frontier

This tranche audits six wire-reachable `Group::action_*` rows against their complete
retail bodies and advances only the portions with a recomputable transaction boundary.

## Exact closure delta

| action status | before | after | delta |
| --- | ---: | ---: | ---: |
| `Complete` | 7 | 8 | +1 |
| `Orders` | 15 | 15 | 0 |
| `StateWired` | 0 | 5 | +5 |
| `Todo` | 13 | 7 | -6 |
| `NotOnTheWire` | 7 | 7 | 0 |

The complete promotion is `stop_spell`. `transport`, `city_gather`,
`gather_point`, `eject_all`, and `alarm` now decode and execute their exact deterministic
group prefix, then retain a typed world-owning tail. They remain closure-red.

## Complete row

`Group::action_stop_spell` at `0x006FD7A0` runs the scenario ignore-orders prelude and
`action_begin`, then visits the selected members in order. A valid on-map unit whose
current action is `CAST_SPELL` clears mask `0x04000000`, its path anchor, order list,
partial path, action, and word `+0x98`, in that order. Types 61, 62, and 400 also set the
objects flag at `+0x22C` and update the group-piece view. `StopSpellReceipt` makes the
prelude snapshot and every member fact explicit; `Applied` validates by recomputing the
entire ordered plan.

## Five honest partials

- `transport` (`0x00702620`) still owns carrier type resolution, allocation, nearby
  placement, relocation/death, order clearing, and boarding.
- `city_gather` (`0x00701780`) still owns ordered city-chain gather-list clear/repopulation
  and local feedback. Its second wire dword is diagnostic-only.
- `gather_point` (`0x006FF1B0`) still owns map clamping/snapping, footprint gates,
  `Build::{clear,add}_gather`, feedback, and the action-3 `action_flight` delegate.
- `eject_all` (`0x00710B40`) still owns reverse bulk ejection, iterative dynamic
  containment traversal, `come_out`, and conditional empty-transport death.
- `alarm` (`0x0070EC30`) still owns both the city-building and `action_alarm_peasant`
  branches, including spatial scans, garrison lifecycle, cast/eject effects, sounds, and
  city flags.

For all five, the bridge commits only the exact prefix: `action_begin` clears
`GroupData::disband`; transport also resets formation for a unit group, and bulk eject
resets formation under the recovered owner predicate. `OpenGroupActionTail` names the
remaining owner. None is represented as `Complete`.

## Adjacent audit result

The other high-call `Orders` candidates were deliberately not bulk-promoted. Follow,
patrol, launch-patrol, scramble, attack-ground, board-ship, repair, and trade all have
receiver gates or multi-object tails absent from the current shared helpers. FOLLOW has a
dedicated exact planner, but it stays `Orders` in this tranche until the dispatcher host can
atomically preserve its raw target identity, raw nonzero queue value, pre-target formation
reset, and fail-closed member installation. The old `action_target` self-skip belongs only
to follow; it must not be used as evidence that the other target actions are complete.
