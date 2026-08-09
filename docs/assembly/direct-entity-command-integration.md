# Command rows 46–49: atomic adapter boundary

Status: **46/47 deterministic tails hostable; 48/49 prefixes complete and action tails
explicitly open.**

This note covers the exclusive adapter in
`crates/don-sim/src/systems/direct_entity_command_integration.rs` and its path-import pins in
`crates/don-sim/tests/direct_entity_command_integration.rs`.  That frozen source-proof lane
did not edit the dispatcher.  The subsequent executable wiring is recorded separately in
`docs/assembly/direct-entity-command-dispatch.md`.

## Closure classification

| row | completed here | fail-closed inputs | receipt status |
|---:|---|---|---|
| 46 buy | exact gate/embargo plan plus the complete bounded `economy::do_buy` loop | selected leader, signed resource bounds, rules, market, leader economy, demand counter, `MarketPriceGates` | `Applied` |
| 47 sell | exact lazy gate/embargo plan plus the complete bounded `economy::do_sell` loop | selected leader, signed resource bounds, rules, market, leader economy, supply counter, `MarketPriceGates` | `Applied` |
| 48 unqueue | signed address validation, concrete Unit/Build class, active/UID guard, target type, exact delegate arguments | safe owner/object resolution, concrete entity fact, reached type-table fact | inactive/stale `Complete`; reached action `OpenTail` |
| 49 come out | signed address validation, Unit-only ABI, active/UID guard, target type, Scholar/general containment classification | safe unit resolution, concrete entity fact, reached type-table fact | inactive/stale `Complete`; reached action `OpenTail` |

`Unavailable` is empty by construction: no plan, facts, presentation receipt, or state receipt
is retained, and no economy state was mutated.  Facts on short-circuited paths stay lazy.  An
ineligible or embargoed market command does not require economy bindings; an inactive or stale
entity command does not read a type-table fact.

## Rows 46/47 atomicity

`MarketEconomyBinding` identifies the selected leader and resource and carries optional
references to every state owner.  The executor checks all of the following before its first
write:

1. the pure prefix replans exactly;
2. the reached delegate is present and matches the decoded side/leader/resource;
3. the signed resource converts into `0..economy::NUM_RESOURCES`;
4. the bound leader and resource match the delegate;
5. rules, shared market, leader economy, price gates, and counter all exist;
6. Buy received a demand counter or Sell received a supply counter.

After that preflight, the tail is infallible: it calls the existing `do_buy`/`do_sell`
primitive until the planner's limit or the first `TradeResult::Refused`.  The receipt stores
the exact rules/gates, economy/market/counter before and after images, call count, completed
count, refusal stop bit, and both leader-economy and market Adler-32 values.  `validates`
reruns those same existing primitives on copies and recomputes all four checksums; changing
any deterministic tail field invalidates the receipt.

The ordered non-state effects are emitted as `DirectEntityPresentationReceipt`.  Diagnostics,
embargo UI, and sound requests are therefore visible without borrowing the sound RNG in the
simulation adapter.  A product layer may deliver the receipts after the state transaction.

## Why rows 48/49 remain open

The available subsystem work is narrower than the command action bodies:

- production owns queue compaction/refund primitives, including routed Library unqueue, but
  `Build::action_unqueue(type)` also owns repeat-latch presentation and selector/count routing;
- production contains a narrow empty-Carrier `Unit::action_unqueue(1)` proof, while the general
  Unit action also updates queued/type/category counters and can reach destruction;
- containment can preflight the Scholar nearby-placement and inside-link splice, but
  `Unit::action_come_out()` also clears launch/action state, may repair Scholar chains, and
  finishes through the general unit-location transaction.

Consequently the adapter returns one of four typed open tails:

- `ProductionUnitActionUnqueue { argument: 1 }`;
- `ProductionBuildActionUnqueue { selector: wire_type }`;
- `ContainmentScholarActionComeOut` for type `0x34`/`0x35`;
- `ContainmentGeneralActionComeOut` for every other resolved Unit type.

These names identify the next owner; they are not applied receipts.  This prevents the
dispatcher from relabelling a partial queue/containment primitive as a complete opcode.

## Fleet handoff

`command::Fleet` now carries the fail-closed method below, using the adapter as a nested
public command module.  The standalone `DirectEntityFleetHandoff` trait still compile-checks
the signature and its default independently:

```rust
fn apply_direct_entity_command_transaction(
    &mut self,
    request: DirectEntityFleetRequest,
) -> DirectEntityFleetReceipt {
    DirectEntityFleetReceipt::unavailable(request)
}
```

`DirectEntityFleetRequest` has exactly two variants: `Market { request, frame }` and
`Entity { request, frame }`.  Carrying the bridge frame prevents a host from validating a
receipt against a different simulation instant.  `DirectEntityFleetReceipt::validates`
rejects a receipt for the other variant, rejects an applied receipt with the wrong frame, and
delegates to the recomputable transaction receipt.

Dispatcher treatment is deliberately narrow:

- rows 46/47 may count as applied only for a validating `MarketTransactionStatus::Applied`;
- rows 48/49 inactive/stale paths may count as complete only for a validating
  `DirectEntityTransactionStatus::Complete`;
- `OpenTail` remains unported and must not increment the applied/acted counter;
- `Unavailable` or any invalid receipt performs no bridge-side success transition.

The method was copied directly onto the existing `Fleet` trait rather than making `Fleet`
inherit a new supertrait, so external Fleet implementations retain the fail-closed default.

Per lane constraint, no compiler, test runner, formatter, or remote job was invoked.
