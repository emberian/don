# Opcode 49 `Unit::action_come_out` frontier

Status: **exact wrapper preflight integrated into the opcode-49 DirectEntity/Fleet receipt;
zero opcode-closure delta**. The planner at
`crates/don-sim/src/systems/unit_action_come_out_frontier.rs` freezes the complete 532-byte
`Unit::action_come_out()` body at `0x005E20B0`. Its path-import tests are
`crates/don-sim/tests/unit_action_come_out_frontier.rs`. The planner is registered as a nested
command module, and the dispatcher can validate its exact snapshot and plan; no wrapper
mutation is authorized until the mandatory general release can commit atomically.

## Why opcode 49 cannot become complete yet

Every return path in the wrapper reaches `Unit::come_out(0)` at `0x005E22AB`. That callee is
9,925 bytes at `0x00617C10` and owns collision, containment-link removal, Guys/world insertion,
conditional Group work, and RNG. Existing production and containment helpers cover narrower
call shapes but do not reproduce that whole transaction. Promoting opcode 49 would therefore
publish cleared orders/path state before an unmodelled general release.

This is the smallest exact active command tail after excluding the just-landed opcode-48 and
RECALL/RETURN work: `Unit::action_come_out` is 532 bytes, versus `Group::action_siege_attack`
549, `Group::action_eject_all` 766, and `Group::action_transport` 932. The honest closure delta
is **0**; opcode 49 remains `state_wired`.

## Exact instruction order

The supported executable is `ron-bin/riseofnations.exe`, SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`; names and sizes come
from the matching shipped PDB. Direct disassembly of `0x005E20B0..0x005E22C3` gives:

1. `UnitData::unit_masks &= ~0x04000000` at `0x005E20C0`;
2. clear the `UnitData::path` anchor dword at `+0xC0`;
3. `Unit::close_orders(0)`, `clear_partial_path()`, then `update_action()`;
4. call `ObjectData::get_inside(&owner)`;
5. only when the actor is type `0x34/0x35`, the returned container belongs to the actor,
   `container.is_build()` is nonzero, and `container.get_build()->is(0x1A4, 0)` is nonzero,
   run the Scholar/University animation repair;
6. unconditionally call `Unit::come_out(0)`.

The University repair first ORs the owner's `LeaderData::leader_flags` with `0x02000000` and
sets the actor's first `Guy::anim_index_hints.length` to zero. It then follows the actual
`inside_down/inside_down_who` chain. At each Scholar/Korean Scholar child it clears that
child's first-Guy hint length, saves the child's `end_time`/`cur_anim`, writes the carried
parent values, and carries the saved values onward. Non-Scholar nodes do not reset the carried
pair, so they are transparent between two Scholar nodes. The last displaced pair is discarded.

## Fail-closed transaction contract

The planner requires object/type predicates only when retail reaches them. A missing container
does not require build facts; a different-owner or non-Scholar container does not invoke either
virtual predicate; a same-owner Scholar container requires `is_build`, and a positive build
result then requires the University predicate. The animation branch binds the exact linked
chain, every Scholar first-Guy image, and the leader flags. Malformed links, unsafe identities,
missing Scholar Guy state, or stale object/order/containment/Guy/leader epochs refuse the plan.

The resulting plan always ends in typed `AuthorityUnitComeOut { argument: 0 }`. A future host
must preflight and commit the wrapper plus that general release atomically. Presentation is not
involved, and no prefix-only receipt authorizes mutation.

## Remaining integration boundary

Opcode 49 can become green only after all four owners converge:

- complete general `Unit::come_out(0)` transaction, including exact conditional RNG;
- canonical object/type virtual queries and actual `inside_down` traversal;
- live order/path/Guy/leader mutation adapter with save/checksum ownership;
- one atomic live host adapter that revalidates the wrapper epochs and commits the wrapper
  plus general release together; the dispatcher receipt-level preflight route is complete.

No retail process was launched or modified for this source recovery.
