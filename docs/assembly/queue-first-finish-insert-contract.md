# Queue-first / finish-insert instruction contract

This is a Tier-C, source-only contract for `Group::set_up_insert(OrderList*)`
`0x0070E520..0x0070E61D`, `Group::finish_insert(OrderList*)`
`0x0070E620..0x0070EA02`, and `copy_order` `0x0072F900..0x0072FCAC` in the pinned
`ron-bin/riseofnations.exe`. The executable was read with Capstone against the shipped PDB.
No live game or checksum was used.

The contract is implemented in
`crates/don-sim/src/systems/queue_first_finish_insert_contract.rs` and remains deliberately
unregistered. It is evidence for the canonical command/order owners, not another queue.

## Exact chronology

1. `set_up_insert` calls `Group::find_leader(0)` at `0x0070E52A` and reads only that
   leader's canonical `OrderList`.
2. It starts at the circular list head and follows node `next`
   (`0x0070E55D..0x0070E57F`, `0x0070E5DA..0x0070E613`). Normal order execution resets to
   `head->prev` and follows `prev`, so this is tail-to-front physical traversal.
3. Only nodes whose `UnitOrder::flags & 4` test succeeds at `0x0070E5A3` reach
   `copy_order` at `0x0070E5A9` and the temporary list's `LinkListBase::add` at
   `0x0070E5B2`.
4. The enclosing action halts the group and recursively issues the requested order with
   literal `QUEUE_NEW` (2).
5. `finish_insert` resets the temporary cursor to its head at `0x0070E62E..0x0070E648`.
   Each node is removed at `0x0070E657` before its kind is read and dispatched at
   `0x0070E664..0x0070E675`.
6. The saved actions are therefore reissued old-tail to old-front. The ordinary raw queue
   1 list-add shape makes each reissued order the execution front. For old execution queue
   `[A,B,C]` and recursively installed order `N`, the final queue is `[A,B,C,N]`, not
   `[N,A,B,C]`.
7. The clone is recycled at `0x0070E9E8` after the Group action returns.

This disproves the current all-member raw-vector stash in `command.rs`: retail saves only
group-flagged leader orders and reissues actions, allowing every member's new order to
recapture current UIDs and reset constructor history.

## Gather, Cast, and Trade

- Gather: `copy_order` arm `0x0072FA56` is complete. `finish_insert`
  `0x0070E7F2..0x0070E805` calls `Group::action_gather(saved.ox, 1)`. It does not forward
  target owner/UID or the 20-byte Gather suffix. Those values are recaptured/reset by the
  action and `Unit::add_gather_order`.
- Cast: `copy_order` arm `0x0072FB64` is complete. `finish_insert`
  `0x0070E8E7..0x0070E904` calls `Group::action_spell(spell, ox, whom, x, y)`. The Group
  action has no queue operand here, and the saved `paid` word is not forwarded.
- Trade: kind 15 maps to `copy_order`'s null arm `0x0072FCA2`. Although
  `finish_insert` has a Trade reissue arm at `0x0070E8C3`, no copied Trade node can reach
  it. A generic splice that preserves an old `TRADE_ROUTE` is divergent.

## Refusal boundary

The pure planner returns no plan unless leader identity, the complete canonical leader
queue, circular-list orientation, every required concrete clone image, and Gather/Cast
history are present. Other action kinds remain at a typed source-owned boundary. This is
intentional: accepting a partial payload would turn the proof contract into a shadow queue.

The production integration should consume the extension-safe DoNSave v12 `Order` envelope,
derive this transaction from the canonical leader queue, and call existing Group actions.
It must not persist the temporary list or copy per-member `OrderRec` vectors.
