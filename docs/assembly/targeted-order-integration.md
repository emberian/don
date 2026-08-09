# Targeted order dispatcher integration

This lane connects the already recovered planners in
`systems::targeted_order_plans` to the authoritative `Unit::do_job` dispatcher. It closes
executor rows 3, 23, and 24; it does not claim that every command producer or product tick
host can issue them yet.

## Closure table

| order | retail body | pre-mutation receipt | deterministic tails | dispatcher result |
|---|---|---|---|---|
| `EXPLORE_TO` (3) | `Unit::do_explore_to` `0x005F24A0` | `ExploreToHostReceipt` acquired before `do_move`; actor/order/frame fields must match | local `do_move`; infallible post-move exact-order identity and on-map receipt; hosted `Unit::explore` | implemented, fail-closed before movement |
| `ATTACK_GROUND` (23) | `Unit::do_attack_ground` `0x005F1410` | `AttackGroundHostReceipt`; concrete `AttackGroundOrderState` and duplicated actor facts must match | local kill/order-flag/attack-unit/recharge/action/object-mask stores; hosted set-attack, cast insertion, angle, animation, ammo, move-facing insertion, and local-player error | implemented; any reached missing `HostFact` is zero mutation |
| `AIR_ATTACK_GROUND` (24) | `Unit::do_air_attack_ground` `0x005EA420` | `AirAttackGroundHostReceipt` acquired before mutating air physics | hosted air physics plus exact post-physics receipt; hosted set-attack, animation/ammo, and death; local walked order, recharge, and mana-burn stores | implemented; unavailable capability cannot enter physics |

The host effect callback is deliberately infallible after a successful typed receipt. A
generic movement world defaults every preflight to `Unavailable`; it cannot inherit a
permissive negative answer or a no-op presentation tail. Receipt identity mismatch returns
`MalformedOrder` before mutation. A host violating an already-issued air capability is a
contract failure, because returning `HostUnavailable` after air physics would falsely claim
rollback.

`OrderRec::targeted_payload` retains the concrete walked fields which the generic order union
previously discarded. `ATTACK_GROUND` carries all four `AttackGroundOrder` fields;
`AIR_ATTACK_GROUND` additionally carries its `AirOrderWalk`, `total_time`, `sx`, and `sy`.

## Frozen lane files

- `crates/don-sim/src/systems/order_dispatch.rs`
- `crates/don-sim/src/order.rs`
- `crates/don-sim/tests/targeted_order_dispatch_receipts.rs`
- `docs/assembly/targeted-order-integration.md`

No command or tick adapter belongs to this lane.

## Root validation commands

The integration lane did not run compilers, formatters, or tests. The root should run:

```sh
cargo test -p don-sim --test targeted_order_dispatch_receipts
cargo test -p don-sim targeted_order_plans
cargo test -p don-sim order_dispatch
python3 -m unittest tools/test_simulation_closure.py
python3 tools/simulation-closure.py --write
git diff --check -- crates/don-sim/src/systems/order_dispatch.rs crates/don-sim/src/order.rs crates/don-sim/tests/targeted_order_dispatch_receipts.rs docs/assembly/targeted-order-integration.md
```

The generated simulation-closure inventory is expected to remain globally red; the relevant
assertion is that order rows 3, 23, and 24 are now emitted as `implemented` with their typed
receipt notes.
