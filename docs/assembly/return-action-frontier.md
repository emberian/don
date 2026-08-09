# Group return action frontier

Status: **source-only complete receiver plan; runtime integration and opcode 35 remain red**.

This pack closes the explicit `Group::action_return` tail exposed by the isolated recall proof.
It recovers the full 1,307-byte receiver CFG into an atomic, recomputable effect plan, without
editing the frozen recall pack, dispatcher, action table, order runtime, tick, or save state.

The exclusive source is `crates/don-sim/src/systems/return_action_frontier.rs`; its mutation pins
are `crates/don-sim/tests/return_action_frontier.rs`.  The module is deliberately absent from
`systems/mod.rs` and is compiled only through the test crate's local `#[path]` declaration.

## Authority and extent

Claims are Tier C **[measured]** from `ron-bin/riseofnations.exe` (SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`) and matching
`ron-bin/sbl/rise.pdb`, cross-checked with `schema/symbols.json`, `schema/types.json`, and direct
PE32 disassembly.

| symbol | VA | bytes | role |
|---|---:|---:|---|
| `Group::action_return` | `0x006FAD40` | 1,307 | recovered receiver |
| `Group::action_begin` | `0x00714100` | 8 | leading `disband = 0` store |
| `UnitData::is_plane` | `0x0046CE40` | 30 | optimized concrete predicate |
| `ObjectData::get_inside` | `0x00651A80` | 244 | contained-aircraft lookup, called twice on that arm |
| `Unit::close_orders` | `0x005E37F0` | 100 | queue reset |
| `Unit::clear_partial_path` | `0x005E3920` | 674 | path reset child |
| `Unit::update_action` | `0x0060A870` | 485 | action refresh |
| `Unit::update_order` | `0x006179D0` | 62 | airborne current/new-order resolver |
| `Unit::add_strafe_order` | `0x005E48C0` | 405 | returning STRAFE installation |
| `StrafeOrder::StrafeOrder` | `0x004800C0` | 245 | temporary constructor |
| `StrafeOrder::~StrafeOrder` | `0x0047EC80` | 202 | temporary destructor |

There is no further `Group::action_*` call in this receiver.  The plan therefore has no open
action tail, though its stateful unit/order/list children remain product-host capabilities which
must commit atomically.

## Entry and scenario ordering

`action_return` calls the group `action_begin` virtual at `0x006FAD60` before reading scenario
`ignore_orders` at `0x006FAD65`.  That order differs from `action_recall`, whose scenario prelude
comes first.  When enabled for owner slots below eight, return walks the owner's global scenario
selection and calls `kill(o,who,0,0)` for each nonnegative entry.  It then reads the resulting
group `num`; `num <= 0` returns immediately.

`ReturnFacts::group_after_ignore_orders` is the exact after-image after both the leading
`disband = 0` store and all scenario kills.  When no kill call occurs, the planner requires it to
equal the request group with only `disband` cleared.  Owner and member identities are rebound to
that after-image before any per-unit effect is emitted.

## Exact plane dispatch cones

For every post-scenario `GroupData::list` member, retail first calls object virtual `+0x08`
(`is_valid_unit`).  At `0x006FAE2C` it loads virtual slot `+0xC0` and compares the function pointer
to concrete `UnitData::is_plane` `0x0046CE40`.  The two cones are observably different and remain
separate in `ReturnPlanePredicate`:

- **Concrete slot:** `domain(+0x218) == 2` and `unit_flags(+0x2B4)&0x20 == 0` identifies an
  ordinary plane.  Only this optimized success cone additionally reads
  `obj_masks(+0x1E4)&0x08000000` and skips missiles when set.  Concrete predicate failure then
  rereads the `0x20` type flag and enters the helicopter route when it is set.
- **Dynamic slot:** retail calls the override.  A true result jumps straight to ordinary-plane
  handling without reading either post-predicate type word.  A false result reads
  `unit_flags&0x20` and enters the helicopter route only when set.

Branch-sensitive `Option` fields reject evidence for words retail did not read.  The planner also
rejects a route which disagrees with the measured predicate cone.

## Contained ordinary aircraft

The first `get_inside(&who)` at `0x006FAE92` branches solely on its signed return value.  A
nonnegative object enters the contained path:

1. clear instance `unit_masks & 0x04000000`;
2. write `UnitData::path.length = 0` at concrete `Unit+0xC0`;
3. `close_orders(0)`;
4. `clear_partial_path()`;
5. `update_action()`;
6. call `get_inside` again;
7. resolve the second object's `ObjectData::launching` pointer;
8. if non-null and its integer array contains the selected actor object index, remove it.

The second inside lookup occurs after the destructive reset and retail does not compare it with
the first result.  Both observations are therefore recorded independently.  Retail directly
dereferences the second address; the safe planner rejects a negative object or owner outside
`0..8` instead of modeling an access violation.  A null launching pointer or missing array value
still retains the five preceding reset effects.

## Airborne ordinary aircraft

A negative first `get_inside` result resolves an AirOrder.  If the linked-list head's first order
is `SPECIAL_ANIM (25)`, retail advances one node and calls virtual `get_air_order`; otherwise it
calls `Unit::update_order` first.  The update-order result and the AirOrder result have distinct
null-return arms and both are pinned.

For a non-null AirOrder, retail preserves the adjusted secondary-base fields
`oxx(+0x04)`, `whose(+0x08)`, `cruising_alt(+0x0C)`, and `sharp_turn(+0x10)`.  It then performs the
same five reset effects as the contained path and calls:

```text
add_strafe_order(-1, -1, oxx, whose, 0, QUEUE_NEW, 0)
update_order()->get_air_order()
new_air.cruising_alt = old_air.cruising_alt
new_air.sharp_turn   = old_air.sharp_turn
```

Unlike `action_recall`'s base-selection arm, `action_return` does not first write the old
AirOrder `returning` field.  Unlike the contained path, it does not touch an inside object's
launching array.

## Helicopter route

When the ordinary-plane predicate fails and `unit_flags(+0x2B4)&0x20` is set, retail performs the
five reset effects before calling virtual `is_on_map`.  If that final predicate succeeds it adds
the targetless order:

```text
add_strafe_order(-1, -1, -1, -1, 1, QUEUE_NEW, 0)
```

There is no following `update_order` or field restoration on this branch.  An off-map helicopter
therefore still has its mask, queue, path, and action reset before returning.

## Atomic receipt, RNG, and integration map

`ReturnReceipt::Applied` recomputes the whole group after-image and exact ordered effect vector.
Changing an identity, branch-sensitive type fact, second inside address, null stage, STRAFE
argument, preserved field, or effect order invalidates the receipt.  `Unavailable` contains no
facts or plan and authorizes no group, launching-list, mask, queue, path, action, or order change.

Neither this receiver nor any modeled direct child draws from the canonical RNG;
`direct_rng_draws` is exactly zero.

Future integration must:

1. register one private planner import without exporting a duplicate module;
2. give the product host branch-sensitive object/type/order/inside snapshots;
3. preflight every scenario, group, launching-list, queue, path, mask, action, and STRAFE effect;
4. apply `action_recall`'s prefix and this return transaction under one outer receipt on the
   selected-air-leader branch;
5. only then wire opcode 35 and regenerate closure evidence.

The isolated return receiver can support a future complete action adapter, but opcode 35 cannot
be promoted from this file alone: its recall prefix, product host, checksum-visible order state,
save/load resume, and dispatcher accounting are still outside this tranche.

## Honest closure delta and parent validation commands

The library/runtime module graph and generated ledgers are unchanged.  `Group::action_return`
remains `NotOnTheWire`, recall remains `Todo`, opcode 35 remains `unimplemented`, and the honest
closure delta is **zero**.

Root convergence formatted the isolated files and validated all 11 tests in persvati batch
`gen7-five-pack-20260809T231109Z-3866-5144-5f896c0277b5`. Retail was not run. The focused
reproduction is:

```sh
cargo test -p don-sim --test return_action_frontier
```

The runtime/closure status remains unchanged pending the integration described above.
