# GARRISON order reconstruction

Status: **source-only proof pack; strict order row remains red**.  The pure planners in
`crates/don-sim/src/systems/garrison_order.rs` freeze the concrete payload, one-unit installer,
wire gate, the member install tuples reached by `Group::action_garrison`, and the complete branch
structure of one `Unit::do_garrison` activation.  The file is intentionally absent from
`systems/mod.rs`; no executor table or closure inventory is promoted by this proof pack.

## Ground truth

The authority is the shipped PE32 image and its matching compiler PDB:

- `ron-bin/riseofnations.exe` and `ron-bin/sbl/rise.pdb`;
- `schema/rise-symbols.tsv`, `schema/symbols.json`, and `schema/types.json`;
- `re/decomp-all/00484690.c`, `00484820.c`, `005e2bd0.c`, `005e4080.c`,
  `005e6b80.c`, `006fd980.c`, `00700490.c`, and `00948760.c`;
- direct `objdump -d -Mintel` reads over the same virtual addresses.

| layer | shipped symbol | VA | bytes |
|---|---|---:|---:|
| concrete walk | `GarrisonOrder::walk_data` | `0x00484690` | 79 |
| one-unit install | `Unit::add_garrison_order` | `0x005E4080` | 221 |
| one-frame executor | `Unit::do_garrison` | `0x005E6B80` | 2,387 |
| group install | `Group::action_garrison` | `0x00700490` | 1,791 |
| wire decode | `CommandPackage::process_garrison` | `0x00948760` | 326 |

Command installation and multi-frame execution are different operations.  The 13-byte command
only admits a group action; it never runs `Unit::do_garrison` itself.

## Concrete order and checksum surface

`GarrisonOrder` is 36 bytes.  The virtual `UnitOrder` base starts at concrete `+0x1C` and its
flag byte is at `+0x20`.

| offset | field | clear value |
|---:|---|---:|
| `+0x08` | `TargetOrder::ox : i32` | `-1` |
| `+0x0C` | `TargetOrder::whom : i32` | `-1` |
| `+0x10` | `TargetOrder::uid : u16` | `0xFFFF` |
| `+0x12` | padding | unspecified |
| `+0x14` | `search : i32` | `0` |
| `+0x1C` | virtual `UnitOrder` vptr | compiler-owned |
| `+0x20` | `UnitOrder::flags : u8` | `0` |

`walk_data` visits exactly the flag byte, the ten bytes from `ox` through `uid`, and the four
bytes of `search`: 15 checksum-visible bytes.  Neither padding nor vptrs participate.  A generic
target pair is insufficient because full-width `ox`, full-width `whom`, UID, `search`, and the
group flag all affect retail behavior or sync state.

## One-unit installation

`Unit::add_garrison_order(ox,whom,search,queue_pos,group_flag)` executes in this order:

1. For `QueuePos::New=2`, clear actor mask `0x04000000`, zero actor `+0xC0`, call
   `close_orders(0)`, `clear_partial_path`, and `update_action`.
2. Allocate order kind 26 through `OrdersMemManager::get_obj`.
3. Store full-width `ox/whom`; snapshot the addressed target UID, or `0xFFFF` if either component
   is negative; store `search`.
4. Clear flag bit `4` when `group_flag==0`, otherwise set it.
5. Append the order.  For `QueuePos::First=0`, clear the partial path and rotate the inserted node
   to the execution head.  Finish with `update_action`.

The installer does not snapshot target position and does not draw RNG.

## Wire and group installer

The packed command has `command_type:u8@+0`, `ox:i32@+1`, `whom:i32@+5`, and
`queued:i32@+9`; the command type is `0x14`.  `process_garrison` always logs and records its replay
trace.  If its package has a group, it calls `Group::action_garrison(ox,whom,queued,0)` when either
target component is negative or the addressed object has active flag bit 0.  A negative component
short-circuits the target lookup.  The function consumes exactly 13 bytes.  Thus network commands
always enter with `search=0`.

`Group::action_garrison` performs group normalization/insertion, owner/alliance admission, target
garrison-limit or Dock `is(0x1B0,0)` admission, and the `BuildData::is_unassimilated` rejection before
walking selected members.  Each member must be live, on-map, not entering/exiting, pass the exact
type-class gate, and satisfy `UnitTypeData::can_garrison`.  An existing current GARRISON is repathed
and killed before reinstallation.  A nonzero fourth caller argument may replace the requested
building through `Unit::find_garrison_build`.

`QueuePos::First` first enters the Group insertion transaction, then recursively performs
`action_garrison(ox,whom,QueuePos::New,0)`.  The caller's search word is discarded; this is not
equivalent to directly prepending every selected unit in the loop.

The member loop converges on four exact calls, all with `group_flag=1`:

| path | search word | queue word |
|---|---:|---:|
| ordinary | caller `search` | caller queue |
| Queue-New worker | raw `EDX` left by `ObjectData::is_worker` | First (`0`) |
| Queue-New still unpacking | caller `search` | New (`2`) |
| Queue-New packing, not unpacking | caller `search` | Last (`1`) |

The worker `EDX` word is an instruction-level fact.  It is not reloaded from the caller and must
remain opaque until an ABI-compatible host receipt supplies it.  In the Queue-New arm, unnamed
shipped option byte `global+0x821&8` makes a member that fits call
`go_inside(chosen_o,target_who,0)` immediately; a member that does not fit is skipped.  Outside
that option, packing members can have their queue repaired before the Last install.  The proof
helper freezes all four final call tuples without guessing names for the option or callee-clobbered
registers.

`Group::alarm_peasant` is another shipped producer.  It bypasses the helper, allocates kind 26,
stores a found replacement building and live UID, writes `search=1`, sets `ORDER_GROUP`, appends,
clears the partial path, rotates to First, updates action, and sets leader mask `0x02000000`.
Other direct helper producers are `Unit::come_out`, which installs
`(target_o,target_who,0,Last,0)` down its linked unit chain, and a transport branch of
`Group::action_move_near`, which installs `(transport_o,group_who,0,New,0)`.  Executor
auto-retarget is the only producer that writes `(found_o,target_owner,1,First,preserved_group)`.

## One `Unit::do_garrison` activation

The executor calls mutable `UnitOrder::update_garrison_order()` at virtual `+0xAC`, reads
`ox/whom/search` from the returned concrete order, and resolves the current target.  Despite the
mutable accessor, this body performs no payload write and no direct RNG draw.  The surrounding
`Unit::work` path owns stale-UID validation; malformed negative indices reaching this body can
retail-crash.  The planner therefore rejects missing, negative, slot-mismatched, or externally
unvalidated target facts without pretending that fail-closed rejection is a retail kill arm.

### Helicopter and Airbase special arm

When the actor has the helicopter bit `UnitTypeData::unit_flags+0x2B4&0x20`, both target
components are nonnegative, and
`target.is(0x1BF,0)` is true, retail calls `target.can_carry(actor.o,actor.who)`:

- true: `kill_current_order(0)`, then
  `add_strafe_order(-1,-1,ox,whom,1,First,1)`, `update_order`, fetch the inserted StrafeOrder,
  and store `1` at Strafe offset `+0x3C`;
- false: `kill_current_order(0)`, then
  `add_move_order(target.x,target.y,1,0,First,0,target_ptr,-1,-1)`, then unconditional product
  feedback (there is no local-player gate on this arm).

The raw target pointer really occupies the seventh move argument.  The proof preserves the ABI
word rather than assigning it invented game meaning.  Both arms return immediately.

The five executor feedback arms use localized rows 3689 through 3693 at consecutive
`loc_str_array_orig.list` offsets `+0x12034`, `+0x12048`, `+0x1205C`, `+0x12070`, and `+0x12084`.
They are product/UI effects but their position relative to queue mutation remains part of the
retail call ordering.

### Normal admission spine

The following gates occur in order; any failure performs one bare `kill_current_order(0)`:

1. target `SubObjectData::is_valid_wall()` (virtual `+0x0C`) and then
   `SubObjectData::is_active()` (virtual `+0x4C`) are nonzero;
2. actor owner equals target owner, or both shipped diplomacy-table values equal `2`;
3. `target.ptype.get_garrison_limit(target owner)` is nonzero;
4. actor type `can_garrison(target type/index)` is nonzero.

Actor virtual `+0x170(ox,whom)` then selects the approach or containment tail.

### Approach and two searches

Let `n=min(target x_size,target y_size)`.  The ordinary radii are
`min=n*0x60+0x30`, `max=-1`.  If the target is Dock `is(0x1B0,0)`, retail adds `0x180` to min and sets
`max=min+0x180`.  The angle is `find_angle(actor.x-target.x,actor.y-target.y)`.

The first `find_nearby_spot` request is:

```text
(target.x,target.y,&outx,&outy,min,max,0,angle,3,
 actor.o,actor.who,0,0,-1,0,-1)
```

Zero return is success.  On nonzero return, retail repeats the same request with only the second
tail word changed from `0` to `1`.  A zero from either call inserts
`add_move_order(outx,outy,1,0,First,0,post_call_ECX,-1,-1)` and returns.  Two nonzero returns take
the common bare-kill tail.  The call-clobbered `ECX` forwarding is mutation-sensitive and is
explicit in the typed search result.

### City gate, capacity, and alternate building

Once in range, retail calls `target.get_build()` and Build object flag `+0x08&0x20` enables a
city-specific gate.  `BuildData::city` at `+0x72` indexes `cities[target_owner]`; the selected
`CityData::race` byte at `+0x5F` must equal target owner.  Then exact PDB virtuals require
`target.hits_left() >= signed_trunc(target.hits(0)/10)`.  Either failure emits local feedback
before the common kill; remote actors silently kill.

Capacity is recomputed as:

```text
limit = target.ptype.get_garrison_limit(target owner)
cost  = 0 if actor.unit_masks&1 else actor.ptype.control_cost(+0x2F0)
used  = target.num_inside()
```

If `used+cost > limit`, nonzero `order.search` may follow `BuildData::city` to an active city
object and call `actor.find_garrison_build(city_index,target_owner)`.  A nonnegative result reads
the current order's virtual group flag, kills the current order, then installs
`add_garrison_order(found_o,target_owner,1,First,preserved_group)`.  All other full-capacity paths
emit local formatted feedback and `S_INVALID_ORDER=0x40` when applicable, then take the common
kill.

### Territory and successful entry

When capacity fits, retail reads the signed terrain owner byte under the target.  A nonnegative
owner different from the target owner is rejected when the target owner is not allied with it.
This arm kills first, then emits local feedback/sound, unlike the city and capacity failures.
Its sound category is also `S_INVALID_ORDER=0x40`.

Otherwise retail calls actor `ObjectData::get_captain()` at virtual `+0xE4`, resolves that unit,
then executes:

1. `captain.go_inside(target_o,target_who,0)`;
2. query target `SubObjectData::is_build()` at virtual `+0x20`; if nonzero store
   `Options::rebuild=1`;
3. `captain.kill_garrison_order(0)`.

The final kill is on the captain, not the target and not necessarily the selected actor.

## Typed receipt contract

The proof planner distinguishes every branch-conditional fact as missing versus known false.
Search failure/success, city activity, alternate result, terrain owner/alliance, captain actor,
and target slot `+0x20` cannot default permissively.  A preflight receipt binds:

- full actor/target identities and payload;
- actor and target versions;
- queue and path digests;
- external-effect epoch;
- RNG epoch, even though the executor consumes zero direct draws;
- the recomputed ordered host plan.

Commit must compare the entire snapshot and recompute the plan.  A stale version, queue/path
change, altered host fact, extra/missing search result, or spliced step rejects the receipt and
authorizes no actor, queue, path, external-effect, or RNG mutation.

## Explicit host tails

`GARRISON_OPEN_TAILS` is an integration inventory, not missing reversal:

1. `GroupActionGarrisonHostAdapter`
2. `ObjectAndTypeVirtualPredicates`
3. `DiplomacyTables`
4. `FindNearbySpotAndOpaqueEcx`
5. `MoveAndStrafeInsertion`
6. `CityMetricsAndTerrainOwner`
7. `FindAlternateGarrisonBuild`
8. `GoInsideAndCanonicalCaptain`
9. `LocalProductFeedback`
10. `ConcretePayloadSaveAndLiveTickAdapter`

Each item needs a real host adapter/receipt.  None licenses substituting `false`, dropping an
effect, consuming speculative RNG, or mutating before validation.

## Frozen shared integration map

The coordinated shared tranche must land all of the following before changing strict closure:

1. `order.rs`: add a variant-owned `GarrisonOrderState { ox:i32, whom:i32, uid:u16, search:i32 }`
   plus group flag.  Do not flatten into current narrow target fields and do not promote arm 26.
2. `systems/mod.rs`: register this module only when its host adapter lands.
3. `order_dispatch.rs`: carry the complete payload through executable conversion; reject missing,
   foreign, or duplicated-identity mismatches; add the explicit GARRISON arm and versioned atomic
   receipt commit.
4. `command.rs` and command tables: decode/install through the dedicated 13-byte group path, not a
   generic target action.  Preserve `search=0` on wire and all group member call tuples.
5. `save_load.rs`: serialize target identity, UID, search, and group flag.  Reject payload-kind and
   identity mismatch; prove save/load/resume equivalence.
6. `tick.rs`/`world.rs`: route production `Sim::do_frame` into the executor and atomically publish
   queue/path/object/global effects.  Direct dispatcher tests are insufficient.
7. Add command -> `do_frame`, alternate-building, Airbase, two-search, captain-entry, and
   save/load/resume integration tests.  Only then regenerate `schema/simulation-closure.json` from
   the closure tool.

The current proof pack earns **closure delta 0**.  Full payload, command/group installation,
save/load, atomic host receipts, and live tick wiring may eventually earn exactly one strict order
row.  It earns no independent command opcode, group-action, BHS, or web row.

## Validation commands

The source-only test imports the unregistered module by path and can be validated independently:

```sh
tools/swarm-cargo-remote submit hbox garrison-plan \
  --path crates/don-sim/src/systems/garrison_order.rs \
  --path crates/don-sim/tests/garrison_order_reconstruction.rs \
  --jobs 12 -- test -p don-sim --test garrison_order_reconstruction
```

The frozen source pack passed both independent remote overlays on 2026-08-09:

- hbox `garrison-plan-20260809T212827Z-14071-9475-b1e98fb890c4`: 18/18 tests;
- persvati `garrison-plan-release-20260809T212827Z-14067-23783-b1e98fb890c4`: 18/18 tests.

Both jobs exited 0.  The only diagnostics were dead-code warnings for the evidence-only VA
constants in the path-imported module; no closure or shared integration claim is implied.

After shared integration, use distinct remote lanes and overlay every changed file:

```sh
tools/swarm-cargo-remote submit hbox garrison-dispatch --jobs 12 -- \
  test -p don-sim --test garrison_order_dispatch
tools/swarm-cargo-remote submit persvati garrison-lib --jobs 12 -- test -p don-sim --lib
tools/swarm-cargo-remote submit hbox garrison-tests --jobs 12 -- test -p don-sim --tests
```

Run the closure generator only after the production frame path is proven.  Secondary prose
coverage files are not the authority for strict completion.
