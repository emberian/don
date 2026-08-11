# `Group::action_attack` — complete body receipt

Lane `setup_place_unit_deep_re`, 2026-08-11. Source-only authority; not installed in a
runtime channel.

## Shipped identities and runnable producers

| artifact | identity |
|---|---|
| `ron-bin/riseofnations.exe` | SHA-256 `30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079` |
| `ron-bin/sbl/rise.pdb` | SHA-256 `334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5` |
| PDB procedure | `void Group::action_attack(int ox, int whom, int mandatory, QueuePos queued, int ignore)` |
| PE body | `0x00712490..0x00713388`, 3,833 bytes; PDB `groups.cpp:5170..5564` |

The receipts can be regenerated without a VM or a live process:

```sh
shasum -a 256 ron-bin/riseofnations.exe ron-bin/sbl/rise.pdb
objdump --disassemble --start-address=0x712490 --stop-address=0x713389 \
  ron-bin/riseofnations.exe
sed -n '1,900p' re/decomp-all/00712490.c
cargo test -p don-sim --test group_action_attack_deep_re
```

The exclusive implementation is
`crates/don-sim/src/systems/group_action_attack_deep_re.rs`; its integration test imports
that file by path, so `systems/mod.rs`, command, order, tick, save and schema owners remain
untouched.

## Exact body chronology

### Entry and building receiver

1. `0x007124B0` calls `GroupData::is_on_map` `0x0070C450`; false returns. `ox < 0` at
   `0x007124BD` also returns.
2. When `ScenarioData::ignore_orders` `0x00CC02F8` is set and `who < 8`, the owner-row
   list at `0x00ED6574/0x00ED6580` is walked in order. Every nonnegative id calls virtual
   `Group::kill(o, who, 0, 0)` at `0x00712515`. The group count is tested only after those
   calls.
3. `num <= 0` returns. Virtual `Group::action_begin` at `0x0071253D` resolves to the exact
   eight-byte body `0x00714100`: it writes `GroupData::disband +0x28 = 0` and returns.
4. A buildings group walks the `num`-sized i16 list at `GroupData +0x8CC`. A non-build
   member reaches only the retail assertion. A build member obtains its `Build*` at virtual
   `+0xAC`, then performs exactly:

   ```text
   Build +0x60 |= 4
   Build +0x7C  = (i16)ox
   Build +0x81  = (u8)whom
   ```

   The first local-player member failing the range probe emits feedback string
   `GameStrings +0x837C` and sound category `0x40`; that is presentation-only. The body
   returns after the list.

### Queue-first wrapper — parameter correction

The branch at `0x00712739` compares `[ebp+0x14]`, which is **`queued`**, not `mandatory`.
For `QUEUE_FIRST == 0` retail constructs an `OrderList`, then executes:

```text
Group::set_up_insert(&orderlist)          0x0070E520
Group::action_halt(ignore)                0x0070D0C0
action_attack(ox, whom, mandatory,
              QUEUE_NEW /* 2 */, ignore) 0x00712490
Group::finish_insert(&orderlist)          0x0070E620
```

The recursive call preserves `mandatory`; `2` is the replacement `QueuePos`. The integer
given to `action_halt` is the fifth argument, `ignore`. The planner represents this as one
atomic delegation seam and does not pretend that a per-unit queue can be committed before
the stash/replay succeeds.

### Leader, attack-position probe, capture move

For `QUEUE_LAST/QUEUE_NEW`, `GroupData::find_leader(0)` `0x0070CCB0` supplies the source
object. Missing leader returns. Its encoded x/y are decoded with xor `0x63637`; when
`mandatory == 1`, `GroupData::get_loc_to` `0x0070C5D0` replaces that origin.

Retail first calls `ObjectData::is_in_range` `0x006486B0`. On false it calls the full
`Unit::find_attack_pos` at `0x007128A5`. The resulting coordinates are not later used by
this optimized owner, but the call is not removable: its candidate scan reads world,
terrain, collisions and object/type data and can advance synchronized RNG.

If the target is a build, is active, and `BuildData::check_capture_eligible` succeeds, the
body calls `Build::check_capture(leader, group.who)` and immediately delegates:

```text
Group::action_move_to(target.x, target.y, queued,
                      0, 0, MOVE_TO, 1, -1, -1, 0)
```

That call is at `0x007129FA`; the function then returns. Otherwise virtual
`target->is_seen(group.who, 0)` at `0x00712A2A` defines `move_only = !seen`.

### Three domain passes

The main nest is exact and order-sensitive:

```text
for dom in 0..=2:
    for member in GroupData::list[0..num]:
        ...
```

It is not a single member pass. Before the domain comparison each member must be valid and
on-map, then the `ignore` bitset filters:

| ignore bit | skipped predicate |
|---:|---|
| `4` | `UnitTypeData::is_defense`, virtual type slot `+0x10C` |
| `2` | `UnitData::is_special`, object slot `+0xD4` |
| `1` | `ObjectData::is(0x3A, 0)`, object slot `+0xB8` |

The plane arm also precedes the domain comparison. For the default `UnitData::is_plane`
vslot, retail devirtualizes to `UnitTypeData +0x218 == 2` and clear
`UnitTypeData +0x2B4 & 0x20`; an override is actually called. A qualifying member with
`max(mana() - *(i16 *)(Unit+0x96), 0) != 0` and current `STRAFE` order mutates the same
`StrafeOrder` payload:

```text
+0x08 = ox
+0x0C = whom
+0x10 = target Object uid at +0x30
+0x3C = 0
```

Then it continues. Because this is before `type_domain == dom`, the same qualifying plane
is retargeted **three times**, once in each outer pass. The test pins `[0, 1, 2]`.

### Existing attack preservation

For a member in the current domain, retail reads `UnitData::get_action`. An ATTACK action
whose `(o, who)` equals the requested target, byte `AttackOrder +0x1C` is nonzero and
`Unit +0xD8 <= 2` enters two range probes. If the first is out of range, the member is
left untouched when `+0xD8 == 1`, or when the current order is attack-shaped and the
payload-bounds range probe succeeds. No new order is installed on those exits.

### Packing/deploy and ordinary members

Only `is_packing_or_unpacking && queued == QUEUE_NEW` reaches the special arm.

| target state | member state | exact ordered result |
|---|---|---|
| unseen (`move_only`) | packing | optional build capture probe; `CAST_SPELL(0x28B, NEW)`; `MOVE_TO(target, LAST)` |
| unseen (`move_only`) | not packing | optional build capture probe; `MOVE_TO(target, LAST)` |
| seen | packing, out of range | `clear_orders`; `CAST_SPELL(0x28B, NEW)`; `ATTACK(target, LAST, mandatory, 1)` |
| seen | not packing, in range | `clear_orders`; `CAST_SPELL(0x28C, NEW)`; `ATTACK(target, LAST, mandatory, 1)` |
| seen | other | `clear_orders`; `ATTACK(target, LAST, mandatory, 1)` |

The cast calls are `add_cast_order(-1,-1,-1,-1,spell,QUEUE_NEW,0)` at
`0x00712F1B/0x007130A8`.

The ordinary branch splits by target kind:

- Build + unseen: an in-range member first calls `Build::check_capture(member, who)`, then
  installs `MOVE_TO(target.x,target.y,queued)`.
- Build + seen: installs `ATTACK(ox,whom,queued,mandatory,1)`.
- Non-build + unseen: installs `MOVE_TO` and then still installs `ATTACK`, in that order.
- Non-build + seen: installs `ATTACK` only.
- On a non-build and `mandatory == 0`, the target of that final ATTACK is first offered to
  `Unit::find_melee_target` `0x005FF9C0`. Distance is
  `min(vector_dist(abs(dx),abs(dy)) + 0xC0, WorldData(+0x18) * 0x240)`; the final selector
  is `2` for a wallbuild target and `1` otherwise. A negative returned object or owner falls
  back to the requested target.

After all three passes, and only on this non-delegating tail, `GroupData::order_num +0x2C`
is incremented with ordinary 32-bit wrapping at `0x00713373`.

## Order-kind accounting

The complete direct sites are:

| kind/call | owner call sites |
|---|---|
| `CAST_SPELL` | `0x00712F1B`, `0x007130A8` |
| `MOVE_TO` through `Unit::add_move_order` | `0x00712F75`, `0x007131B8`, `0x0071322E` |
| `ATTACK` | `0x007130CF`, `0x0071333C` |
| `MOVE_TO` through `Group::action_move_to` | `0x007129FA` |

`Unit::add_move_facing_order` can allocate `MOVE_TO`, `ATTACK_TO`, `EXPLORE_TO` or
`FLEE_TO` from its selector. **All three calls owned by `action_attack` pass literal
selector `1`, so the latter three are unreachable from this body.** They remain enumerated
in the receipt API because the movement adapter owns the complete family, but converting an
attack intent to `ATTACK_TO`, `EXPLORE_TO` or `FLEE_TO` here would contradict the PE.

## RNG chronology

There is no direct call from `0x00712490..0x00713388` to `Random`.
`Unit::find_attack_pos` contains the one reachable RNG instruction:

```text
0x00602124  call Random::get(int,int) 0x00A39D70
             arguments (0, 0xFFFF)
```

It executes once for each accepted collision-free candidate inside the delegated scan, not
once per action. The `AttackPositionReceipt` therefore carries an ordered vector of
`(call_va, low, high, value, state_before, state_after)` and accepts zero or more draws.
`Unit::find_melee_target` and `Group::action_move_to` have no direct RNG call. A producer
that cannot provide the exact candidate/RNG receipt returns a boundary; it must not sample
or fit a destination.

## Atomic ownership and first residuals

The module is a pure request-to-plan authority. It neither creates another Groups pool nor
owns an object table. The intended future adapter is canonical Sim `groups_guys::Groups`
plus `World::OrderList<Order>`:

1. Snapshot group slot/id/revision/digest, exact ordered sparse members, target identity and
   every `HOST_READS` fact.
2. Plan. Unavailable facts return `PlanError::External`, never a default predicate.
3. Revalidate group, target, member and order revisions/digests before `steps[0]`.
4. Apply all steps in emission order inside one transaction, or none.

There are three useful definitions of the first residual:

- **First external read:** call `0x007124B0` to `GroupData::is_on_map` `0x0070C450`.
- **First sequential external mutation when the scenario branch is live:** virtual call
  `0x00712515` to `Group::kill` `0x00714110`. That 573-byte callee can recursively remove a
  captain/subordinate, compacts all parallel Group arrays, updates object group backlinks,
  recomputes speed, and can clear the group. A producer must supply the chronological
  post-prune projection before planning downstream; filtering only the i16 list is not an
  acceptable substitute.
- **First external mutation otherwise:** `action_begin` is fully owned as the literal
  disband store. The next boundary is branch-dependent: `Build::check_capture`
  `0x006276A0`, the queue-first set-up/halt/finish transaction, `Group::action_move_to`, or
  one of `Unit::clear_orders`/`Unit::add_*_order`.

Those callees are explicit plan steps, not hidden fallbacks. Until one Sim adapter can
revalidate and commit every reached seam atomically, this receipt must stay unregistered and
the existing command bridge must not claim whole-body closure.

## Verification

`cargo test -p don-sim --test group_action_attack_deep_re` passes 9/9. The pins cover the
queue-first argument correction, domain-major member order, unseen move-then-attack,
PACK/DEPLOY chronology, three-pass STRAFE mutation, capture delegation, existing-attack
preservation, mandatory-zero melee substitution, buildings fields, first-boundary failure
and exact RNG call arguments.
