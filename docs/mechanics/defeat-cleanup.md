# Defeated-player object cleanup

This tranche corrects a broad earlier assumption: `Leader::defeat` does **not** raze every
object owned by the defeated player. The exact deterministic object work in
`riseofnations.exe` is narrower and now live.

## Retail transaction

`Leader::defeat` `0x006ECB00` performs these object-owned continuations after committing
the leader flags/stamps:

1. `0x006ECBB7..0x006ECC1C` walks the owner's Build band (object ids `>= 2000`) and calls
   `Build::clean_queue(0)` on every valid Build accepted by the Build virtual gate. This is
   the existing no-refund terminal production cleanup.
2. `0x006ECC1C..0x006ECCAF` walks the owner's Unit band. Invalid slots are skipped. The
   shipped `Unit` vtable `0x00B417D0` resolves `+0xC0` to `UnitData::is_plane`
   `0x0046CE40`; true calls `+0x158`, `Unit::die` `0x0060EDA0`, with literal arguments
   `(0, -1, 0.0)`. False calls `Unit::clear_orders` `0x005E3860`. Both branches finish by
   clearing `UnitData::unit_masks & 0x00040000`.

`UnitData::is_plane` is exact and intentionally includes missiles:

```text
type.domain == 2 && !(type.unit_flags & 0x20)
```

The `0x20` exception is the shipped Helicopter flag.

## Live integration

`systems::victory_score::Leaders` accumulates defeated owners separately from the Build
queue-cleanup mask. `tick::Sim` drains both in the same terminal flush. Before mutating a
Unit, it resolves the complete owner band against installed production type profiles; an
absent/unsupported type or missing path sidecar returns a typed `DefeatCleanupError`, leaves
every Unit in that owner sweep untouched, and re-arms the owner request.

Tech Race resolves from `Leader::gain_tech` during step 14, after the ordinary step-12
victory flush. Its production callback therefore drains this same Unit adapter before the
research completion returns; defeated aircraft do not survive until the next frame.

For admitted rows:

* planes use the existing live death transaction, including active-bit removal, optional
  aircraft wreck, and `DeathObj` insertion (the remaining type-specific suffixes inside
  retail's 3,984-byte `Unit::close` are not newly claimed by this tranche);
* other Units drop every generic order and their `PathStack`, clear the `0x04000000`
  facing-move latch from `Unit::clear_orders`, and recompute the now-empty action endpoint
  from current position/facing;
* both branches clear `0x00040000` after their branch, preserving retail ordering.

Mutation-sensitive tests cover plane death versus ground order closure, owner isolation,
the two distinct mask clears, corpse creation, accumulated recursive defeat requests, and
all-or-nothing preflight on an unknown type.

## Standing-Army stop transaction

Immediately before the Build/Unit sweeps, retail calls `Armies::leader_defeated`
`0x006F2F90`. Its 69-byte body walks every valid Army for the owner and calls `Army::stop`
`0x006F9180`. The live defeated-owner drain now executes that transaction against the
concrete Army, global Group, object registry, Unit, order, and `PathStack` stores before it
touches the later Unit band.

The name is easy to overread: `Army::stop` does **not** close an Army, invalidate it, unlink
its Groups, or change any `ArmyData` field. It walks the live Group-id prefix, skips empty
Groups, clears `GroupData::disband` through `Group::action_begin`, preserves building-group
formation state, and otherwise sets `form = -1`. Eligible on-map non-plane members then
receive the two instruction-ordered Unit mask clears around path/order/action reset. The
entire Army + Unit-band transaction is preflighted before the first mutation, so an unknown
installed type/path fact cannot leave half a standing Army stopped.

The remaining typed Army-stop boundary is `SpecialAnimOrder.type`: generic `Order` preserves
the `SPECIAL_ANIM` class but not whether it is `SPECIAL_ENTER`, `SPECIAL_EXIT`, or
`SPECIAL_UNIT`. The first two must be skipped and the last halted, so a reached non-plane
special animation defers the owner transaction instead of guessing. Scenario
`ScenarioData::ignore_orders` / `Group::kill` is also kept outside the ordinary multiplayer
adapter. Presentation/network notifications later in `Leader::defeat` remain intentionally
separate. The other frozen object boundary is the remainder of `Unit::close` after the
current live death/corpse adapter: type-specific counts, containment and Group teardown
still need their own exact host facts.
