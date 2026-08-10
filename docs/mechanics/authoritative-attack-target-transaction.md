# Authoritative ATTACK target transaction frontier

## Result and honest verb delta

ATTACK now has a non-lossy transaction through the first production tick consumer.  The policy
adapter can prepare one exact current target, retain its stable `Handle` and retail `(who,o,uid)`,
and carry the Sim episode revision plus the opaque visibility revision/frame/ordinal through a
scheduler phase.  It also proves the target is a distinct, active, currently visible enemy Unit
before producing an order payload.  `Sim::do_attack` consumes that complete identity before its
first mutable execution gate.  The policy-side prepared transaction also binds the captured
target public row back to live Sim state and proves every optional combat-table dependency which
the current executor can otherwise leave silently.

The admitted verb delta is **0**.  `UNIT_INTEGRATION[ATTACK]` remains `SimAttackIssue`, but the
conditional mask is false and ordinary apply returns `AttackTargetCommitUnavailable` without
mutation.  This remains required because the production path does not yet preflight every
prepared fact atomically and Step 12 does not yet produce the authoritative freshness consumed by
ATTACK.  A policy-side proof alone cannot close that execution-time gap.

## First lossy owner closed

The descriptive `don_sim::order::Order` previously held only `target_who` and `target_o`, even
though the executable `OrderRec` already held retail's `TargetOrder::uid`.  That made an ordinary
ATTACK unable to distinguish its original target from a later object reusing the same owner-array
slot.

`Order` now retains:

- `target_who`, `target_o`, and `target_uid`, matching the walked retail identity;
- optional `target_handle`, the port's dense-compaction-stable identity;
- `Order::attack_exact(OrderTargetIdentity)` as the only constructor which proves all four;
- `Order::exact_target_identity()`, which returns `None` rather than inventing a Handle for a
  legacy order.

`OrderRec` and both `adopt`/`publish` conversions preserve UID and Handle.  Queue cursor reset does
not affect the payload.  DoNSave format **7** serializes the UID and optional Handle before the
existing SPECIAL_ANIM, FORM, and FOLLOW option payloads; their relative encoding order is
unchanged.  Load/resave retains the exact target transaction bit-for-bit.

## Prepared policy transaction

`AuthoritativeBackend::prepare_attack_target` owns the read-only half.  Its opaque
`PreparedAttackTargetTransaction` freezes:

- actor Handle, viewer, and generated request;
- Sim episode revision;
- visibility owner revision, captured frame, policy ordinal, and binding;
- target Handle plus `(who,o,uid)` after a live Sim revalidation;
- the current retail mutual diplomacy result, which must be `Diplo::War`;
- the complete captured target public row, re-read byte-for-byte from live Sim;
- both exact `UnitTypeStats` rows, the installed balance-table cell, combat-rule snapshot,
  direct/projectile mode, recharge result, and positive damage result produced by the same
  default-predicate pipeline as the current executor;
- the exact actor and target rows only as private same-borrow validation cursors, never as stored
  identity.

Only `QueuePosition::Replace` and zero order flags enter this one-unit route.  Missing/stale
visibility, changed identity, same-object targeting, and non-enemy relations have typed refusals.
Missing balance/type rows, an out-of-domain balance cell, changed captured public state, a
non-live target, and a non-positive damage result also refuse before mutation.  Combat sources
are explicit immutable setup inputs retained across deterministic reset; replacing them
invalidates every outstanding visibility ordinal.
`retained_order()` produces the non-lossy queue payload but is deliberately not a commit permit.

`apply_prepared_attack_target` revalidates the opaque visibility binding and the complete prepared
token.  A fresh visibility capture yields `StaleBinding`; deterministic episode reset yields
`StaleEpisodeRevision`.  A still-current token reaches the same `CombatTargetHost` refusal as
ordinary mask/apply, with no order or world mutation.

## Remaining production boundary

The real `Sim::do_attack` path now consumes `Order::exact_target_identity()`.  It requires the
Handle row, owner Unit registry row, `(who,o)`, and live UID to name the same active, distinct
object incarnation.  A mismatch retires the order before recharge, balance, movement, ammo,
damage, or RNG state can change.  A valid exact order retains its attested row for the existing
executor.  Handle-less legacy orders keep the previous `(who,o)` compatibility path, explicitly
without becoming authoritative.

The RL preflight now proves the current executor's optional type/balance dependencies and its
positive can-hurt result, and `apply_prepared_attack_target` recomputes the complete opaque token.
Before ATTACK can be admitted, the production execution transaction itself must consume that
proof after authoritative Step-12 visibility freshness and before any of the tick stages which
can change its inputs.  Exact identity consumption plus a policy proof closes two hazards, but
does not make them one atomic production transaction, so neither justifies changing the mask.

## Focused validation

The source tranche is covered by:

```sh
cargo test -p don-sim --test attack_target_transaction
cargo test -p don-sim --test attack_target_execution
cargo test -p don-env --test authoritative_attack_execution_preflight
cargo test -p don-env --test authoritative_visibility_integration
```

The first target drives exact identity through `Sim::issue`, `OrderList`, `OrderRec` cursor reset,
publish, DoNSave load, and deterministic resave.  The second reaches the real object-work tick and
proves that valid exact identities cross the preflight while Handle, owner, object-index, and UID
mismatches retire before recharge or movement; it also freezes the legacy compatibility path.
The third proves typed missing-table/type refusals, full prepared execution facts, source-change
staleness, zero mutation, and reset retention.  The fourth freezes preparation, commit refusal,
visibility-refresh staleness, and reset staleness while checking zero mutation at the red boundary.
