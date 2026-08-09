# GUARD order reconstruction

Status: **source-only proof pack; strict order row remains red**.  The pure planners in
`crates/don-sim/src/systems/guard_order.rs` freeze the one-unit installer and the complete
branch structure of one `Unit::do_guard` activation.  They are intentionally not registered
in `systems/mod.rs`, and no table is promoted: the concrete payload, group installer, save
format, atomic world receipts, and production `Sim::do_frame` route are not integrated.

## Ground truth

The authority for this lane is the shipped PE32 image and matching compiler PDB:

- `ron-bin/riseofnations.exe`, SHA-256 prefix recorded by `schema/pdb-types.json` as
  `30478a44..625079`;
- `ron-bin/sbl/rise.pdb`;
- `schema/symbols.json`, `schema/rise-procs.tsv`, and `schema/pdb-types.json`;
- `re/decomp-all/005e3220.c`, `005e3e40.c`, `005e5c70.c`, `006fcd30.c`, and `009478a0.c`;
- direct `objdump -d -Mintel` instruction reads over the same VAs.

| layer | shipped symbol | VA | bytes |
|---|---|---:|---:|
| wire decode | `CommandPackage::process_guard` | `0x009478A0` | 276 |
| group install/update | `Group::action_guard` | `0x006FCD30` | 2,012 |
| find queued GUARD | `Unit::update_guard_order` | `0x005E3220` | 225 |
| allocate one GUARD | `Unit::add_guard_order` | `0x005E3E40` | 277 |
| multi-frame executor | `Unit::do_guard` | `0x005E5C70` | 2,392 |

This separation is mandatory.  Opcode `0x1F` is a 13-byte `GuardCommand`
(`ox:i32@+1`, `whom:i32@+5`, `queued:i32@+9`) and merely calls
`Group::action_guard(ox, whom, queued, 0)`.  It does not execute a GUARD frame.

## Concrete order state

`GuardOrder` is 56 bytes.  Its virtual `UnitOrder` base is at concrete `+0x30`; the flag byte
is at `+0x34`.

| concrete offset | field | clear value |
|---:|---|---:|
| `+0x08` | `TargetOrder::ox : i32` | `-1` |
| `+0x0C` | `TargetOrder::whom : i32` | `-1` |
| `+0x10` | `TargetOrder::uid : u16` | `0xFFFF` |
| `+0x14` | `dx : i32` | `0` |
| `+0x18` | `dy : i32` | `0` |
| `+0x1C` | `guard_x : Coord` | `0` |
| `+0x20` | `guard_y : Coord` | `0` |
| `+0x24` | `idle : i32` | `0` |
| `+0x28` | `retry : i32` | `0` |

`GuardOrder::walk_data` walks one flag byte, the ten TargetOrder bytes, and all six i32s:
35 checksum-visible bytes.  A generic `(target_o,target_who)` representation is insufficient.
The current flattened `OrderRec` also physically aliases these locations with Move fields,
so accesses must become variant-checked rather than treating the overlap as shared meaning.

## One-unit installation

`Unit::add_guard_order(ox, whom, dx, dy, queue_pos, dead_arg6)` does the following in order:

1. For `QueuePos::New=2`, clear actor mask `0x04000000`, store zero at actor `+0xC0`, call
   `close_orders(0)`, `clear_partial_path`, and `update_action`.
2. Allocate order kind 12 through `OrdersMemManager::get_obj`.
3. Store full-width `ox/whom`.
4. If either component is negative, store UID `0xFFFF` and actor X/Y.  Otherwise capture the
   addressed target's UID and decoded X/Y.
5. Store `dx/dy`, zero `idle/retry`, and OR `ORDER_GROUP=4` into the virtual-base flag byte.
6. Add the order.  For `QueuePos::First=0`, clear the partial path and rotate the inserted
   node to the execution head.  Finish with `update_action`.

The sixth argument is proven unread.  `update_guard_order(ox,whom)` scans the complete
circular queue, not just its head.  When the group path finds a matching GUARD it changes
only `dx/dy`; UID, guard point, idle, and retry survive.

`Group::action_guard` is a separate open tail.  Its full install includes group insertion,
alliance/on-map/plane gates, captain canonicalization, temporary formation computation, and
the existing-order update path.  Unit targets use `FormData.off_x/off_y`; non-unit targets
install `dx=dy=-1`.  The shipped formation call also passes decoded target Y through an ABI
slot typed as `QueuePos`; integration must preserve that raw word rather than normalize it.

## One executor frame

`Unit::do_guard(UnitOrder*)` obtains the concrete `GuardOrder` through virtual slot `+0x74`.
It does not validate UID; surrounding `Unit::work` owns stale-target validation.

### Admission, retry, and phase work

1. `ox<0` or an inactive target takes the terminal tail: `set_anim(0,0,1)` followed by bare
   `kill_current_order(0)`.  There is no `repath` here.
2. On an active target, retail calls the valid-unit virtual and, conditionally, `is_on_map`
   once before the retry gate; the first on-map result is discarded.
3. Any nonzero `retry` decrements with 32-bit wrapping and returns.  Valid retail after-images
   are positive, but the instruction is `test/jz`, not `retry > 0`.
4. With retry zero, `moving = valid_unit && on_map && target-vslot(+0xD8)`.
5. Nonmoving targets phase on signed `actor.o + game.frame`:
   - `(phase+8)%16 == 0` calls `find_melee_target(-1,null,0,1,0)` and returns;
   - otherwise `phase%16 == 0` increments `GuardOrder::idle` and returns.

The melee arm wins if both modular expressions could alias under malformed signed state.

### Wall/build replacement arm

Target vslot `+0x1C` is the PDB's `is_wallbuild`.  Retail returns unchanged unless
`attack_dist(target)>0x600` and the actor's virtual `is_captain()` is nonzero.  It then calls
`find_nearby_spot` around the footprint:

```text
min  = (x_size + y_size)      * 48
max  = (x_size + y_size + 32) * 48
step = 0
mask = 0x55555555
filter = 3
tail = 0, 1, -1, 0, -1
```

Nonzero search return falls back to target X/Y.  A temporary one-member Group receives
`action_move_near(x,y,0,New,0,0,MOVE_TO,0,-1,-1,0)`.  `New` is consequential: this replaces
the GUARD rather than leaving it behind.

### Projected guard point

For a non-wall target, mirror `dx` when target `unit_masks&2`, then use the exact shipped trig
calls:

```text
gx = target.x + sinx(target.angle, dy) + cosx(target.angle, effective_dx)
gy = target.y - cosx(target.angle, dy) + sinx(target.angle, effective_dx)
```

Retail maps the candidate through `div_3_table`, supplies the mapped values shifted right two
to `invalid_loc`, and on invalid locations uses this order:

1. Accept the projected cell centre directly only when actor type flag `0x10` is set and
   terrain flags `(flags&0x30) != 0x20`.
2. Otherwise search around the projection's snapped cell centre at radii `0xC0..0x180`,
   step `0x60`.
3. On failure search around target at `vector_dist(dx,dy)..+0x180`, step `0xC0`.
   The `+0x14/+0x18` loads immediately before `0x0046CFF0` prove the inputs are `dx/dy`.
4. On second failure use actor X/Y.

Both search tails are `0,0,-1,0,-1`.  Every result is finally stored as:

```text
guard_x = div_3_table[chosen_x >> 4] * 48 + 24
guard_y = div_3_table[chosen_y >> 4] * 48 + 24
```

The proof planner imports trig/table/search outputs as named facts instead of guessing a world
implementation.  Their exact call requests and branch use are receipt-recomputable.

### Facing, movement, and retry RNG

Desired facing is target angle when `moving`; otherwise it is
`find_angle(guard_x-target.x, guard_y-target.y)`.  Actors with mask `0x40000` and any of the
three exact virtual/type predicates at `+0x10C`, actor `+0xCC`, or actor `+0xC4` force target
angle.

When actor and guard are in different snapped cells:

1. zero `GuardOrder::idle`;
2. call `add_move_facing_order(guard_x,guard_y,angle,2,0,First,0,-1,-1,-1,0)`;
3. set the inserted Move pause to `30*max(1, coarse_manhattan)`;
4. call `update_order` and `do_move` in the same activation;
5. if GUARD is not exposed, return without an RNG draw;
6. if GUARD is exposed, `set_anim(0,0,1)`, draw canonical
   `game_random.get(0,0xFFFF)`, and store `retry = draw%3+6` (`6..=8`).

Deferring movement or drawing speculatively changes tick and RNG ordering.

### Same-cell and cast arm

For a nonmoving target whose desired angle differs, call `set_angle(desired,dead,0)`.  Then
increment Guard `idle`.  Auto-cast requires all four gates: type flags `+0x2B8&4`, actor
vslot `+0x100==0`, actor mask `0x80000`, and `!is_unpacking()`.  Threshold is 30 when
`actor.is(0x7B,0)`, otherwise 70.  Reaching it queues
`add_cast_order(-1,-1,-1,-1,0x28C,First,0)`; otherwise retail calls `set_anim(0,0,1)`.

## Explicit open tails

`GUARD_OPEN_TAILS` is part of the source proof and must shrink only as real receipts land:

1. `GroupActionGuardFormationAndUpdate`
2. `ObjectVirtualPredicates`
3. `TrigDiv3TerrainAndInvalidLoc`
4. `FindMeleeTarget`
5. `FindNearbySpot`
6. `TemporaryGroupActionMoveNear`
7. `AddMoveFacingAndSameTickDoMove`
8. `AutoCastInsertion`
9. `CanonicalGameRandom`
10. `ConcretePayloadSaveAndLiveTickAdapter`

The last item is not paperwork.  Production `Sim::do_frame` still reaches the old narrow
`World::unit_work` switch; there is no production call to `systems::order_dispatch::work`.
Flipping `EXECUTORS[12]` before a real tick adapter would convert GUARD from visible red to a
silent no-op.

## Frozen shared integration map

These edits are deliberately deferred to a coordinated shared tranche:

1. `order.rs`: add a full `GuardOrderState` payload and variant validation.  Do not change
   `EXECUTORS[12]` yet.
2. `systems/mod.rs`: register this module only when the shared integration begins.
3. `order_dispatch.rs`: carry the full payload through `OrderRec`, reject duplicated identity
   mismatches, add versioned fact/commit receipts, and dispatch GUARD atomically.
4. `command.rs`: replace generic `action_target` construction with the exact selected-member
   GUARD installer; keep group action/opcode closure at Orders until `Group::action_guard` is
   bounded.
5. `save_load.rs`: serialize all target and six concrete fields.  It is currently dirty with
   DoNSave v6 work: include GUARD before that layout lands or bump to v7 afterward.
6. `tick.rs`/`world.rs`: make the retail-derived executor reachable from the actual frame path
   and prove command -> `do_frame` state advance plus save/load/resume equivalence.
7. Only then regenerate `schema/simulation-closure.json` from the closure tool.

## Validation commands

The proof pack itself needs no shared-file overlay:

```sh
tools/swarm-cargo-remote submit hbox guard-plan \
  --path crates/don-sim/src/systems/guard_order.rs \
  --path crates/don-sim/tests/guard_order_reconstruction.rs \
  --jobs 12 -- test -p don-sim --test guard_order_reconstruction
```

After shared integration, run independent remote lanes for:

```sh
cargo test -p don-sim --test guard_order_dispatch
cargo test -p don-sim --lib
cargo test -p don-sim --tests
cargo run -q -p don-replay --bin don-closure
```

Overlay every dirty shared/new file for those runs.  Do not infer full closure from the pure
planner test.

## Honest closure delta

- This source-only pack: **0** order rows, **0** group-action rows, **0** opcode rows.
- Dispatcher-only integration without a live frame route: still **0**.
- Full payload + command + save + atomic world + live-tick evidence can earn exactly one order
  row (`21/28 -> 22/28`, and global red `127 -> 126` at the ledger snapshot that assigned this
  lane).  `Group::action_guard` and wire opcode 31 remain separate completion gates.
