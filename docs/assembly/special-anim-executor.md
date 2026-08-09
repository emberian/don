# SPECIAL_ANIM order executor reconstruction

Status: **complete source proof of the concrete executor CFG; not live-integrated**.

This note bounds `SpecialAnimOrder` installation and every reachable branch of
`Unit::do_spec_anim` in the shipped Rise of Nations binary. The source-only planner and its
mutation-sensitive tests do not change the strict order ledger. The strict row must remain red
until the typed receipt is connected to the production frame path and all host tails below are
implemented atomically.

## Ground truth

The matching shipped artifacts are `ron-bin/riseofnations.exe` and `ron-bin/sbl/rise.pdb`.
Primary evidence is:

- `SpecialAnimOrder::walk_data` `0x004849A0`;
- `SpecialAnimOrder::clear` `0x00484EC0`;
- `Unit::add_spec_anim_order` `0x005E4160`, 92 bytes;
- `Unit::do_spec_anim` `0x005E5880`, 864 bytes;
- `Unit::do_spec_unit_anim` `0x005E5BE0`, a three-byte return stub;
- `Unit::land_plane` `0x005E9950`, the sole direct installer caller;
- `re/decomp-all/005e5880.c`, checked against direct disassembly and PDB vtable names;
- `Unit::set_angle` `0x00605400`, `Unit::set_new_location` `0x005F8D20`,
  `TerrainOut::find_data_z` `0x00866560`, `Guy::set_new_z` `0x005D86C0`,
  `Random::get` `0x00A39D70`, and `Unit::work` `0x0060D180`.

The concrete class is 44 bytes. Its walked state is the flag byte at `+0x04` plus nine
contiguous 32-bit words at `+0x08..+0x2B`, 37 bytes total:

| offset | field | clear value |
|---:|---|---:|
| `0x08` | `SpecialType type` | `SPECIAL_UNIT` (2) |
| `0x0C` | `started` | 0 |
| `0x10` | `frames` | 0 |
| `0x14` | `data1` | -1 |
| `0x18` | `data2` | -1 |
| `0x1C` | `data3` | -1 |
| `0x20` | `data4` | -1 |
| `0x24` | `ox` | -1 |
| `0x28` | `whom` | -1 |

`Unit::add_spec_anim_order(type, data1, data2, QueuePos)` allocates arm 25, sets
`ORDER_GROUP`, writes only `type/data1/data2`, appends, clears the partial path, rotates the
inserted node to the head, and updates the action. The ABI carries `QueuePos`, but the wrapper
never reads it: all values have exact FIRST behavior.

There is no player-wire command for this internal order. Its sole direct installer caller is
`Unit::land_plane`. That caller installs ENTER, uses mode/data2 `3` for an actor whose
`unit_flags & 0x20` is set and `1` otherwise, stores the target's zero-argument `get_gpiece()`
result in data1, then patches data3/data4 to the target `(o,who)`. Optimized code pre-pushes mode
and reuses the ignored QueuePos stack word for the target pointer before evaluating `get_gpiece`;
neither word is an argument to that virtual call, and the raw pointer has no queue meaning.

## Reachable executor

`SPECIAL_UNIT` returns immediately. It does not write the payload, query a host object, draw RNG,
or emit an effect.

`SPECIAL_ENTER` and `SPECIAL_EXIT` first write `frames = 10`, then `started = 1`.
Neither reachable arm reads `data1` or `data2`; both words remain walked and are preserved.

For ENTER, `(data3,data4)` is the target slot. Retail queries `is_valid_build()` and only when it
is false queries `is(AIRCRAFTCARRIER=0x15F, 0)`. An accepted target receives
`actor.go_inside(data3,data4,0)` followed by `kill_current_order(0)`. A rejected target performs
`kill_current_order(0)` first and then `actor.die(0,-1,0.0)`. The source planner treats a missing
or malformed target as unavailable host evidence instead of reproducing a retail crash.

For EXIT, the Airbase target lookup `(ox,whom)` occurs only when `ox >= 0`. A missing slot or a
non-Airbase target ends with `kill_current_order(0)`. An Airbase exit uses `data3/data4` as the
base coordinates:

1. Retail samples the actor's `unit_flags & 0x20`. A false first sample uses offset `(-192,0)`
   and consumes no RNG. A true first sample consumes exactly two consecutive
   `Random::get(0,0xFFFF)` results and uses offsets
   `(first % 11 - 197, second % 11 - 5)`.
2. After both conditional draws, retail calls `set_angle(0,dead,1)`, then
   `set_new_location(data3+xoff,data4+yoff,1,1)`.
3. `find_data_z` uses the unoffset base `(data3,data4,0)`. The first Guy is assigned that Z.
   Retail then samples the helicopter flag a second time; a true second sample assigns the same
   Guy `current_z + 200`. The two observations are not collapsed: external calls occur between
   them, so the receipt records both in order.
4. Guy pitch is copied to last-pitch and zeroed, then bank is copied to last-bank and zeroed.
5. Retail kills the current order and immediately calls virtual `Unit::work()` in the same tick.

Each shipped `Random::get(0,0xFFFF)` advances the global LCG once with
`state = state * 0x19660D + 0x3C6EF35F (mod 2^32)` and returns
`((state & 0xFFFF) * 65535) >> 16`. Thus supplied results are `0..65534`; the two-draw branch
realizes x offsets `-197..-187` and y offsets `-5..5`.

`Unit::set_angle` does not read its second ABI argument; the planner deliberately records no
invented semantic value for that dead slot.

## Unreachable compiled tail

The binary contains a continuation block at `0x005E5ADA..0x005E5BBF`, but its controlling signed
comparison is unreachable for all valid discriminators. ENTER/EXIT have already written
`started = 1`. The threshold is normally zero and becomes one only for an EXIT whose `(ox,whom)`
object is an Airbase. Thus `started < threshold` is either `1 < 0` or `1 < 1`. SPECIAL_UNIT
returned before the write and comparison. The proof API and tests pin this result instead of
mistaking dead machine code for an executable branch. For binary-CFG fidelity the proof also
freezes both structural halves:

- EXIT would set location to unoffset `data3/data4`, sample terrain Z, resolve Guy 0 unchecked,
  assign/roll Guy Z according to the old `started`, set angle, roll/zero bank, and increment
  `started`. It would not reset pitch, draw RNG, or retire the order.
- The non-EXIT half would require `is_valid_build()` with no Airbase fallback, kill then die if
  invalid, otherwise decode the target XYZ with XOR key `0x63637`, relocate, shift Guy Z, set
  angle, roll/zero bank, and increment `started` without retiring the order.

## Fail-closed transaction boundary

Conditional host facts are represented as `Option`, never permissive booleans. Valid-build ENTER
must not supply a carrier result; negative-`ox` and non-Airbase EXIT must not supply terrain or
RNG results; an Airbase EXIT must supply both ordered helicopter observations, and a true first
observation must supply exactly two in-range draws. Receipt snapshots
bind actor/target versions, current order and queue/path/Guy digests, object/terrain/external
epochs, and the RNG epoch. Validation recomputes the complete plan. Any mismatch authorizes no
local or external mutation and consumes no RNG.

The explicit unresolved host adapters are:

- internal `land_plane` installation and its `get_gpiece` observation;
- object lookup and `is_valid_build`/type predicates;
- canonical game RNG;
- angle and location mutation;
- terrain and primary-Guy mutation;
- `go_inside` and `die`;
- queue retirement plus same-tick `Unit::work`;
- dispatcher and production live-tick publication.

## Frozen shared integration map

The proof files are exclusive and source-only. A later shared integration must:

1. register `systems::special_anim_executor` in `systems/mod.rs`;
2. add a typed SPECIAL_ANIM preflight/commit arm in `systems/order_dispatch.rs`, validate the
   existing complete `SpecialAnimOrderState`, publish the receipt atomically, flip `ARMS[25]`,
   and update the dynamic `(implemented, partial, unimplemented)` count from `(21,1,6)` to
   `(22,1,5)`;
3. bridge that arm into the actual `Sim::do_frame` / `World::unit_work` path; the core Sim/World
   production frame path does not currently reach `order_dispatch::work`, so a dispatcher-only
   flip is a silent no-op there (the separate `don-ai` arena runtime already calls it);
4. only after the live frame test passes, flip `order.rs::EXECUTORS[25]` and regenerate
   `schema/simulation-closure.json` with the normal closure generator.

No payload, save/load, or player-command format change belongs to this lane. The full nine-word
payload already survives `Order`/`OrderRec` conversion and DoNSave, and the installer has no
player wire opcode.

## Validation and honest delta

Run validation remotely in distinct lanes after the proof is landed:

```text
tools/swarm-cargo-remote submit hbox special-anim-proof \
  --path crates/don-sim/src/systems/special_anim_executor.rs \
  --path crates/don-sim/tests/special_anim_executor_reconstruction.rs \
  --jobs 12 -- test -p don-sim --test special_anim_executor_reconstruction

tools/swarm-cargo-remote submit persvati special-anim-tests \
  --path crates/don-sim/src/systems/special_anim_executor.rs \
  --path crates/don-sim/src/systems/mod.rs \
  --path crates/don-sim/src/systems/order_dispatch.rs \
  --path crates/don-sim/src/order.rs \
  --path crates/don-sim/tests/special_anim_executor_reconstruction.rs \
  --jobs 12 -- test -p don-sim --tests
```

The source proof earns **zero** strict closure rows. Full host adapters, atomic live-tick wiring,
and evidence can earn exactly **orders +1** (`21/28` to `22/28`, assuming no concurrent ledger
movement). It earns no command/group-action or opcode row.

The frozen proof passed both independent convergence profiles on 2026-08-09: hbox
`special-anim-executor-20260809T215933Z-42540-32075-9f11e3ea7d82` and persvati release
`special-anim-executor-release-20260809T215933Z-42546-23998-9f11e3ea7d82`, each with 16/16
focused tests and exit 0. Evidence-only VA constants produced warnings; no strict row was promoted.
