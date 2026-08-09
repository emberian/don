# Step 8 wall ejection: fixed retail edge

Status: **source-frozen, not integrated**.  This tranche recovers the complete
step-8-reachable control surface of `Object::eject_contents`; it does not claim that the
9,925-byte `Unit::come_out` child or the death/type/order owners are closed.

Implementation: `crates/don-sim/src/systems/step8_eject_contents.rs`

Focused source test: `crates/don-sim/tests/step8_eject_contents.rs`

## Retail identity and reachability

| Body / edge | VA | Size / evidence |
|---|---:|---|
| `Wall::update_hits(int)` | `0x0063F0D0` | step-8 caller |
| fixed argument pushes | `0x0063F64A..0x0063F64F` | `1, -1, 0, 1` after call-order decoding |
| direct `Object::eject_contents` call | `0x0063F653` | actual tick-reached anchor |
| `Object::eject_contents(int,int,int,int)` | `0x0064CD20` | `0xB92` / 2,962 bytes, `object.cpp:3461-3663` |

The call is exactly:

```text
eject_contents(kill_failed=1, filter=-1, transfer=0, reset=1)
```

The caller has already established `inside_down >= 0` and
`carrier.can_carry(DOMAIN_AIR=2) == 0`.  The carrier is the Wall/Build whose hits are being
updated, so `is_build == 1` and `is_unit == 0`.  These facts are part of the module's input
contract rather than guesses made inside the planner.

They make the following general-purpose callee subtrees statically unreachable:

- the non-negative type/filter selector;
- the `reset == 0` repath/cast arm;
- the `kill_failed == 0` failure return;
- the transfer/order scratch-group arm selected by `param3`;
- the complete carrier-Unit suffix: same-damage/doober inheritance, transport flags,
  order/path/group transfer, Group membership, sound and hot-unit replacement.

The last exclusion matters: this recovery does not overlap the active GARRISON lane.

## Exact reachable machine

The entry first calls virtual `ObjectData::is_on_map` (`vtable +0xBC`).  An off-map carrier
returns without a write.  An on-map carrier then splits on `Game +0x821 & 0x08`:

1. **Bit clear / normal mode.**  The fixed caller facts satisfy the deferred-build arm.
   Retail repeats `can_carry(2)`, ORs `WallData::build_masks +0x60` with `0x4000`, and
   returns.  Step 14's `Build::process_ejection` later consumes this bit.
2. **Bit set / immediate-execute mode.**  Retail repeatedly re-reads the carrier's
   `inside_down +0x28` and `inside_down_who +0x3E`.  It never pre-saves or sorts a `next`
   list.  Nested contents exposed by a splice are therefore observed in actual head order.

For every observed passenger head, the mutation/call order is:

1. `UnitData::unit_masks +0x68 &= ~0x04000000`;
2. `UnitData::path +0xC0 = 0`;
3. `Unit::close_orders(0)` at `0x005E37F0`;
4. `Unit::clear_partial_path()` at `0x005E3920`;
5. `Unit::update_action()` at `0x0060A870`;
6. `Unit::come_out(0)` at `0x00617C10`.

A non-zero `come_out` result takes the fixed failure call
`die(0, -1, 0.0)` (`Object::die` implementation `0x00647080`), then re-reads the carrier
head.  A zero result takes the following successful tail, then re-reads the head.  Since
the selector is disabled and failure kills are enabled, the loop ends only after the
carrier head becomes negative.  A receipt that leaves the same passenger at the head is
therefore rejected as a cycle/no-progress condition.

### Successful specialist and Airbase tail

The exact type branch covers only `0x32..0x35` on this Build carrier:

- Citizen `0x32/0x33` first resolves the tribe-grafted Militia `0x42`, tests the owner
  leader's `tech` mask (`LeaderData +0x6C18` at the tested word), resolves
  `current_upgrade`, and if successful calls `set_type(upgrade, 0)` then `clear_orders`.
  This arm bypasses the city search.
- Scholar `0x34/0x35`, and Citizens that fail the militia gate, use the city arm.  If the
  carrier city is valid and `ObjectsData::find_city` (`0x0065BA90`) returns a city, retail
  issues the fixed `Unit::add_move_order` (`0x00616ED0`) to that city's coordinates.
- No city is a valid no-action outcome.

After that branch, `carrier.is(AIRBASE=0x1BF, 0)` conditionally emits
`add_strafe_order(-1,-1,-1,-1,1,QueuePos(2),0)` at `0x005E48C0`.  The subsequent
`carrier.is_unit()` is known false and jumps straight back to the head read.  In
particular, `replace_hotunit` is not reached by this step-8 edge.

## Fail-closed ownership and mutation protocol

`WallEjectSnapshot` binds the canonical carrier identity and revision, the actual repeated
head observations, each passenger UID/type/reset fields, and the initial deterministic
stamp.  The pure planner emits one of:

- `OffMapNoOp`;
- `DeferredMask { before, after }`;
- `Drained { ops, initial_stamp, final_stamp }`.

Every external child/effect has a non-zero, carrier-and-passenger-bound receipt.  Receipt
stamps must form one exact continuous chain.  A stamp carries both RNG seed/draw count and
all fifteen ordered `check_all` channel values.  This prevents a host from reporting a
plausible `come_out` result while omitting its RNG draw, collision mutation or Group side
effect.  Preflight accepts only zero or one `come_out` draw, requires zero draws in the
specialist/Airbase tail, rejects a regressing draw counter, and rejects a changed seed
without a recorded draw.  The host `commit` contract is deliberately stronger than the
retail call surface: it must re-check revision, identities and tokens, then apply the whole
drain atomically or apply nothing.  This is the safe headless seam until all child owners
are canonical in one `Sim` transaction.

Rejected before commit:

- an invalid owner/object identity, non-Unit passenger type, non-Build carrier, caller that
  can carry air, or missing initial head;
- a stale/mismatched object identity or repeated passenger;
- a receipt with zero token, wrong scope/effect or discontinuous stamp;
- a successful release with no specialist/Airbase receipt;
- a failed release with no death receipt;
- Militia conversion for Scholar/non-specialist types, or specialist actions for other
  types;
- an Airbase action that disagrees with the carrier fact;
- any head transition that cycles, makes no progress, or fails to end empty.

## RNG and checksum consequences

`Object::eject_contents` contains **no direct `Random::get` call**.  That is not a
zero-draw guarantee for this edge.  The late tail of `Unit::come_out` can reach exactly one
of two mutually exclusive `Random::get(0, 0xFFFF)` sites (modulo 3 versus modulo 2), and a
frame-bit branch can then call `Unit::add_to_army`.  Death and other child calls also remain
receipt-owned.  Consequently the transaction records before/after seed and draw count; an
opaque outcome without them is invalid.

Known deterministic ownership is:

| Effect | Retail `check_all` ownership |
|---|---|
| carrier `build_masks` and changed `inside_down` | builds, channel 2 (band-2000 Build) |
| passenger mask/path/orders/type/inside state | units, channel 1 |
| conditional `add_to_army` from `come_out` | groups, channel 6 |
| successful world insertion/location bookkeeping | guys, channel 7; world/collision, channel 12 |
| failed death tail | units plus any death/leader/population owners enumerated by its receipt |

The channel list is a minimum direct map, not permission to omit further transitive deltas.

## Frozen minimal integration map

No shared file is changed by this tranche.  A later landing lane must:

1. export the new module from `systems/mod.rs`;
2. replace the step-8 `Wall::update_hits` placeholder exactly at the retail call anchor
   `0x0063F653`, after the inside/carry guards and before its `construct_hits` return;
3. snapshot carrier/passengers from the canonical band-2000 Build and Unit/object stores,
   preserving repeated-head order and UID validation;
4. adapt the existing containment collision/link recovery to the complete
   `Unit::come_out(0)` owner, including its conditional RNG and Group tail;
5. bind leader tech/current-upgrade, city lookup, orders, death, Guys and World collision
   owners into one rollback-capable commit;
6. make channel/save ownership cover every mutated field before admitting a synchronized
   or saved game.

## Honest remaining red boundary

This closes the **bounded 2,962-byte parent decision body for the exact tick-reached call
shape**, not the whole transitive cone.  The still-red boundary is:

- general `Unit::come_out(int)`, `0x00617C10`, 9,925 bytes (only its collision/link subset
  is currently recovered elsewhere);
- failed-release `Object::die` consequences;
- authoritative militia graft/tech/current-upgrade and city lookup receipts;
- concrete type/order/Group/Guys/World mutation adapters;
- shared step-8 scheduler/export and save/checksum integration.

Until those owners land, the module can prove and serialize the intended transaction but
must fail closed when an authoritative receipt or atomic commit is unavailable.

## Validation evidence

The frozen source pack passed independent debug and release overlays on 2026-08-09:

- hbox `step8-eject-20260809T213900Z-25118-19633-f0c299985015`: 8/8 tests;
- persvati `step8-eject-release-20260809T213900Z-25121-10274-f0c299985015`: 8/8 tests.

Both jobs exited 0.  Diagnostics were limited to evidence-only dead-code warnings from the
path-imported module.  These receipts validate the bounded planner and mutation contracts; they
do not promote the still-unwired step-8 scheduler row.
