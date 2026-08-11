# GUARD and GARRISON: the two order arms wired on 2026-08-11

Status: **dispatched from `Unit::do_job`, behind a mandatory typed host receipt.** Fidelity
tier **C** — derived from `ron-bin/riseofnations.exe` + `ron-bin/sbl/rise.pdb`, exercised by
tests against this port, **not** differentially tested against retail. Nothing here is
verified in the proof-assistant sense.

| order | retail | size | planner (already landed) | adapter (new) |
|---|---|---:|---|---|
| 12 `GUARD` | `Unit::do_guard` `0x005E5C70` | 2,392 B | `systems/guard_order.rs` | `systems/guard_dispatch.rs` |
| 26 `GARRISON` | `Unit::do_garrison` `0x005E6B80` | 2,387 B | `systems/garrison_order.rs` | `systems/garrison_dispatch.rs` |

Both planners were complete decision transcriptions that had been landed **unregistered** —
absent from `systems/mod.rs`, compiled only through a `#[path]` include in their own test
file. `Unit::do_job` therefore fell to its default arm for both, and a `GUARD` or `GARRISON`
order sat in a queue mutating nothing forever. The two new modules are the missing half.

## Correction: `GUARD` does not "call through to `do_move`"

The closure inventory carried `GUARD` with the note *"calls through to do_move"*. That is
false as a characterization of the arm, and it under-sized the work by more than an order of
magnitude.

[measured — `r2 -q -e scr.color=0 -c "s 0x005E5C70; pD 2392" ron-bin/riseofnations.exe`]
the body issues **30 direct `call rel32`s**. Exactly **one** targets `Unit::do_move`
`0x005F7B30`, at `0x005E6532`, and it is reachable only after the arm has recomputed the
guard post, stored it into `GuardOrder::guard_x/guard_y` (`+0x1C`/`+0x20`), inserted a
movement node with `Unit::add_move_facing_order` `0x005E55C0` at `0x005E64AA`, and re-read
the head through `Unit::update_order` `0x006179D0` at `0x005E64B1`. `GUARD` **installs** a
move and drives it in the same tick; it does not delegate.

The complete call set, resolved against the PDB:

| VA | symbol | sites |
|---|---|---:|
| `0x0064C880` | `ObjectData::attack_dist` | 1 |
| `0x005FF9C0` | `Unit::find_melee_target` | 1 |
| `0x0061DE70` | `UnitType::find_nearby_spot` | 3 |
| `0x00713E80` / `0x00714350` / `0x0070F9E0` / `0x00704990` | `Group::clear` / `Group::add` / `Groups::push_group` / `Group::action_move_near` | 1 each |
| `0x0092D100` / `0x0092D0C0` | `sinx` / `cosx` | 2 each |
| `0x00607C30` | `UnitData::invalid_loc` | 1 |
| `0x0046CFF0` | `vector_dist` | 1 |
| `0x0092D130` | `find_angle` | 1 |
| `0x00605400` | `Unit::set_angle` | 1 |
| `0x0060A4B0` | `UnitData::is_unpacking` | 1 |
| `0x005E4A60` | `Unit::add_cast_order` | 1 |
| `0x00616F40` | `Unit::set_anim` | 3 |
| `0x005E55C0` | `Unit::add_move_facing_order` | 1 |
| `0x006179D0` | `Unit::update_order` | 3 |
| `0x005F7B30` | `Unit::do_move` | **1** |
| `0x00616E80` | `UnitData::order_type` | 1 |
| `0x00A39D70` | `Random::get` | 1 |
| `0x005E2CB0` | `Unit::kill_current_order` | 1 |

Two consequences the old note hid, both pinned by tests:

* **`GUARD` consumes the canonical `game_random` stream.** `Random::get(0, 0xffff)` at
  `0x005E6566` feeds `GuardOrder::retry = draw % 3 + 6` (`+0x28`). Any lockstep host that
  budgets draws per frame must count this arm.
* **`GUARD` can insert a `CAST_SPELL` order.** A stationary unit whose type opts in
  (`UnitType[+0x2B8] & 4`, `unit_masks & 0x80000`, not unpacking) issues
  `Unit::add_cast_order(-1, -1, -1, -1, 0x28C, QueuePos::First, 0)` once `idle` reaches 30
  ticks (type `0x7B`) or 70 ticks (everything else).

## The `GUARD` same-tick tail, verbatim

```text
  005e64aa  call 0x5e55c0   ; Unit::add_move_facing_order(guard_x, guard_y, angle, 2,0,First,0,-1,-1,-1,0)
  005e64b1  call 0x6179d0   ; Unit::update_order()  -> the inserted MoveOrder
            ...             ; MoveOrder::pause (+0x24) = max(1, coarse manhattan) * 30
  005e652c  call 0x6179d0   ; Unit::update_order()
  005e6532  call 0x5f7b30   ; Unit::do_move(head)
  005e6539  call 0x616e80   ; UnitData::order_type()
            cmp eax, 0xc    ; != GUARD -> return; the inserted move is still current
  005e654b  call 0x616f40   ; Unit::set_anim(0, 0, 1)
  005e6552  call 0x6179d0   ; Unit::update_order()  -> the GUARD order again
  005e6566  call 0xa39d70   ; Random::get(0, 0xffff)
            ...             ; GuardOrder::retry (+0x28) = draw % 3 + 6
```

`retry` is the one payload word written **after** the same-tick `do_move`, which is why
`guard_dispatch` holds it back instead of publishing the planner's whole after-image up
front. The post-`do_move` head type is **observed**, not predicted: the adapter runs
`do_move`, reads `order_type()`, and rejects a host whose `head_is_guard` fact disagrees.

## The split of responsibility, per arm

`GUARD` and `GARRISON` sit at opposite ends of the local/external axis, and saying so plainly
is the point of this section.

**`GUARD`** — the dispatcher owns the concrete payload writes (`guard_x`, `guard_y`, `idle`,
`retry`), the `QueuePos::First` movement insertion, that leg's `pause`, the same-tick
`Unit::do_move`, and the bare `Unit::kill_current_order(0)`. Everything else —
`find_nearby_spot`, `find_melee_target`, the temporary `Group` approach, `set_anim`,
`set_angle`, `add_cast_order`, `Random::get` — is a `WorkWorld::guard_effect` callback.

**`GARRISON`** — the dispatcher owns exactly one local effect, the bare
`Unit::kill_current_order(0)`. `Unit::go_inside` `0x0061A2E0`, `Unit::find_garrison_build`
`0x00605040`, `BuildTypeData::get_garrison_limit` `0x00633F50`, `ObjectData::num_inside`
`0x00646D50`, `LeaderData::is_ally` `0x006EDB50` and the local-player feedback are all
`WorkWorld::garrison_effect`.

Two ordering facts that a naive loop would get wrong, and which are pinned by tests:

1. **`HostileTerritory` kills first, then emits feedback.** Every other terminal `GARRISON`
   branch emits feedback and kills last. A step loop that returned on the first
   `KillCurrentOrder` would silently drop the feedback on exactly one branch.
2. **The `Entered` branch issues no `kill_current_order` of its own.** Retirement happens
   inside `Unit::kill_garrison_order` `0x005E2BD0`, which walks the *containment chain*: it
   reaches each unit's queue through `UnitData::get_action` `0x00608450`, and when the action
   is `GARRISON` (`0x1a`) it issues the failure pair `Unit::repath` `0x005E29B0` then
   `Unit::kill_current_order(0)`, then recurses into the container. Whether that reaches the
   acting unit depends on state the dispatcher does not hold, so `garrison_dispatch`
   re-reads its own head order after the host steps and reports what actually happened.

## The receipt contract

Both adapters take one snapshot-bound receipt from the host, then:

* **recompute the plan themselves** from the receipt's facts and refuse a mismatch — the
  dispatcher never trusts the host's plan, only its observations;
* **re-derive every fact they can observe** and refuse a disagreement: actor identity, actor
  `x`/`y`(/`angle`), `unit_masks`, `Game::frame`, the concrete order payload, the order's
  flags byte, an FNV-1a digest of the order queue's `debug_image`, and an FNV-1a digest of
  the unit's `Stack<PathData>`;
* **cross-check the projected type words** that `UnitWork` already carries —
  `UnitType[+0x2B4] & 0x20` against `type_moves_while_turning`, and `UnitType[+0x2B8] & 4`
  against `type_snap_arm`;
* for `GARRISON`, **re-run the stale-UID gate** that `Unit::work` block H owns, through the
  same `UnitWorld::target` lookup, and refuse a receipt whose `outer_uid_validated` claim
  does not match the world.

What remains **host-attested rather than verified** is named explicitly on the snapshot
types: `external_epoch` and `rng_epoch` (and `actor_version` / `target_version` for
`GARRISON`). The dispatcher cannot observe those, and the code says so rather than implying
a stronger guarantee.

A refusal at any of those points is zero-mutation; the tests assert that with a full
projection of the actor's checksum-visible state, not a spot check.

## What is still open

`guard_dispatch::GUARD_DISPATCH_OPEN_TAILS` (7 entries) and
`garrison_dispatch::GARRISON_DISPATCH_OPEN_TAILS` (9 entries) are the machine-readable
version of this list. The two that matter most for the next lane:

* **No production `Sim::do_frame` bridge.** `crates/don-sim/src/tick.rs` reaches `MOVE_TO`,
  `ATTACK`, `BUILD_AT` and `SPECIAL_ANIM` only. Wiring `GUARD`/`GARRISON` there needs a
  `SimGuardHost` / `SimGarrisonHost` in the shape of the existing `SimSpecialAnimHost`, and
  `tick.rs` was held by another lane on 2026-08-11.
* **No save/load or command-construction path for the two concrete payloads.**
  `OrderRec::guard` and `OrderRec::garrison` exist and round-trip inside the dispatcher, but
  `crate::order::Order` (the descriptive/save shape) carries neither, so a DoNSave round trip
  still drops them. `Group::action_guard` `0x006FCD30` and `Group::action_garrison`
  `0x00700490` remain unported installers.
