# Authoritative ATTACK target transaction frontier

## Result and honest verb delta

ATTACK now has a non-lossy transaction up to, but not through, the production tick consumer.
The policy adapter can prepare one exact current target, retain its stable `Handle` and retail
`(who,o,uid)`, and carry the Sim episode revision plus the opaque visibility revision/frame/
ordinal through a scheduler phase.  It also proves the target is a distinct, active, currently
visible enemy Unit before producing an order payload.

The admitted verb delta is **0**.  `UNIT_INTEGRATION[ATTACK]` remains `SimAttackIssue`, but the
conditional mask is false and ordinary apply returns `AttackTargetCommitUnavailable` without
mutation.  This is required because `Sim::do_attack` still resolves only `(who,o)` and neither
checks the retained Handle/UID nor preflights every dependency which can currently return with an
unchanged order and world.

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
- the exact actor and target rows only as private same-borrow validation cursors, never as stored
  identity.

Only `QueuePosition::Replace` and zero order flags enter this one-unit route.  Missing/stale
visibility, changed identity, same-object targeting, and non-enemy relations have typed refusals.
`retained_order()` produces the non-lossy queue payload but is deliberately not a commit permit.

`apply_prepared_attack_target` revalidates the opaque visibility binding and the complete prepared
token.  A fresh visibility capture yields `StaleBinding`; deterministic episode reset yields
`StaleEpisodeRevision`.  A still-current token reaches the same `CombatTargetHost` refusal as
ordinary mask/apply, with no order or world mutation.

## Remaining production boundary

Before ATTACK can be admitted, the real `Sim::do_attack` path must consume
`Order::exact_target_identity()` and retire/refuse on a Handle, owner, object index, or UID
mismatch before damage, movement, ammo, or RNG mutation.  Its preflight must also prove every
currently silent dependency, including the attacker/defender type rows and balance entry, and
must host the complete retail can-hurt/target eligibility transaction.  Merely storing the right
identity would still permit accepted-no-effect actions, so it does not justify changing the mask.

## Focused validation

The source tranche is covered by:

```sh
cargo test -p don-sim --test attack_target_transaction
cargo test -p don-env --test authoritative_visibility_integration
```

The first target drives exact identity through `Sim::issue`, `OrderList`, `OrderRec` cursor reset,
publish, DoNSave load, and deterministic resave.  The second freezes preparation, commit refusal,
visibility-refresh staleness, and reset staleness while checking zero mutation at the red boundary.
