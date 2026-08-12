# STRAFE executor transaction

Status: **exclusive source-only atomic body; dispatcher/tick/save remain red**. This tranche does
not register a module or change an executor status. It turns the recovered `Unit::do_strafe`
planner into a prepare/revalidate/assignment-commit transaction over detached canonical images.

## Authorities consumed

The transaction reuses, rather than duplicates:

- `strafe_order_frontier.rs` for the exact `0x005EAB00..0x005EB95C` body, frame cones, ordered
  plan, and `StrafeExecutorReceipt`;
- `strafe_runtime_authority.rs` for the one canonical `patrol::StrafeOrder` representation and
  its 57-byte retail walk validation; and
- registered `air_physics_frontier.rs` for the nested `0x005E86D0..0x005E8DD2` plan and
  `AirPhysicsCommitReceipt`.

The shipped executable and PDB hashes remain respectively
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079` and
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`.

During this integration pass one payload omission was corrected in the existing frontier:
`InsertStrafeFirst` now retains the search hit's `x/y` as well as `(o,who,uid)`. Retail writes
those coordinates to walked `StrafeOrder.xx/yy`; an identity-only insertion could not produce a
checksum-complete order.

## Atomic boundary

`prepare_strafe_frame` binds the actor/order/object/queue/path/effect/RNG snapshot, validates the
complete current STRAFE node (including duplicated target header), recomputes the retail plan, and
applies locally-owned mutations to a clone:

- target pair, UID, live target position, and returning-latch writes;
- current-order retirement;
- exact Queue-First STRAFE construction with hit coordinates;
- attack-result latch and wrapping bomber spell-time writes.

Effects whose child body owns additional state require an ordered `StrafeExternalReceipt` with
the full before and after canonical image. These are `think_bird`, air physics, AIR_PATROL
installation, recursive `work`, death, animation, and ammo. The receipt's step must be the next
exact planner step and its transaction id must match. No opaque callback runs during commit.

The air-physics receipt is additionally checked for:

- the same transaction and actor identity;
- an internally complete `AirPhysicsCommitReceipt`;
- identical pre/post RNG epochs;
- `Continue` exactly when STRAFE observed a nonzero physics return; and
- the nested final `AirOrder` image when the STRAFE node survives.

`commit_strafe_frame` compares the complete snapshot and canonical before-image, builds its stable
receipt, then publishes one assignment. A stale actor scalar, queue node, path, target epoch,
external epoch, or RNG epoch authorizes zero writes.

## Remaining shared hooks

This tranche deliberately stops before shared integration. The next coordinated tranche must:

1. register the payload authority, frontier, and transaction together;
2. add the tag-8 payload to canonical `Order` and DoNSave v13, with save/load/resume mutation
   tests;
3. implement the live `Sim` adapter which constructs the full nested receipts from the one World,
   order/path, air-physics, ammo, animation, target-search, and RNG owners;
4. route order row 16 through `order_dispatch::do_job` and then `Sim::unit_work`; and
5. only after those gates turn `ARMS[16]` green and run replay checksum comparisons.

Until all five land, STRAFE remains unimplemented in the strict dispatcher table. The present
transaction is the exclusive body those hooks can mount; it is not a closure claim.

## Focused gates

```sh
cargo test -p don-sim --test strafe_order_frontier
cargo test -p don-sim --test strafe_runtime_authority
cargo test -p don-sim --test strafe_executor_transaction
git diff --check -- \
  crates/don-sim/src/systems/strafe_order_frontier.rs \
  crates/don-sim/src/systems/strafe_executor_transaction.rs \
  crates/don-sim/tests/strafe_order_frontier.rs \
  crates/don-sim/tests/strafe_executor_transaction.rs \
  docs/assembly/strafe-order-frontier.md \
docs/assembly/strafe-executor-transaction.md
```
