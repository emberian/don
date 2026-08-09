# REPAIR order reconstruction

Status: deterministic planner complete from a frozen host snapshot; live order dispatch is
intentionally not wired in this lane.  Primary evidence is `Unit::do_repair` at
`0x005EE420` (`re/decomp-all/005ee420.c`, 1,998 bytes; `schema/rise-procs.tsv`).

## Exact control flow

- `0x005EE420-0x005EE462` reads `RepairOrder.ox/whom` and always requests animation
  `0x22,0,1` before testing the target.
- `0x005EE466-0x005EE4CA` treats zero target damage or failed diplomacy as a lost target.
  A foreign target requires both relation lookups to equal `2`; matching the repairer's
  team bypasses those lookups.
- `0x005EE4CD-0x005EE523` kills the order when target vslot `+0x1c` is false or the returned
  Build is inactive. `0x005EE526-0x005EE569` also kills an attacked target unless the
  RepairOrder flag byte contains `0x04`.
- `0x005EE56B-0x005EE5DC` rejects a nonnegative territory owner that is neither the target
  owner nor its ally. `0x005EE5E2-0x005EE687` tests repair range; failure kills the order,
  constructs a one-unit Group, and calls `Group::action_swarm_around(target, First,
  REPAIR=13, flags&4)`.
- `0x005EE68A-0x005EE68E` returns immediately when `UnitData.unit_masks&1` is set.
- `0x005EE694-0x005EE865` computes the repair divisor. Without the second `+0x1c`
  interface query it is `256`. Otherwise retail uses unsigned 32-bit
  `((vslot_18c(1)<<9)/(vslot_11c(0)*constants[0x22c]))*(helpers+1)`, wrapping intermediates.
  Build flag `0x20` doubles it and unequal City bytes `+0x5f/+0x5e` double it again.
  Tribe bonus `0x10` and nonzero constants `+0x7e0` apply `(100-pct)/100`. Unless the bonus
  exists *and* constants `+0x7dc` is nonzero, under-attack and Build mask `0x10` each add
  an independent x4. The helper byte increments after this calculation, including on a
  zero-quantum frame.
- `0x005EE865-0x005EE8AC` repairs all current damage when the divisor is nonpositive;
  otherwise the frame quantum is exactly
  `(frame*256)/divisor - (frame*256-256)/divisor` with signed truncation.
- `0x005EE8BB-0x005EEA2F` scans resources 0..5. Unavailable types and a zero target-type
  cost basis cost zero. Other resources cost one exactly when SSE-truncated
  `repair_state/(hits/basis)` is less than `(repair_state+amount)/(hits/basis)`. The first
  unaffordable resource stops the scan. Success debits every available stock and clamps
  the parallel accumulator to zero before target vslot `+0x168(amount,0,1)`.
- `0x005EEA62-0x005EEAEF` kills an unaffordable order, updates `LeaderData.repair_stamp`
  only after 151 frames, and emits UI/sound feedback only for the local player.
- `0x005EEB02-0x005EEBDB` is the lost-target tail. After killing, mask `0x40000` invokes
  `Unit::find_repair_spot`. Otherwise a same-owner idle low-state repairer may queue Gather
  at `QueuePos::New=2`, provided the target/type queries pass and the type is not
  `UNIVERSITY=0x1a4`.

Resolved callees: `Unit::kill_current_order` `0x005E2CB0`, `Unit::find_repair_spot`
`0x00604320`, `UnitData::order_type` `0x00616E80`, `Unit::set_anim` `0x00616F40`,
`Unit::add_gather_order` `0x0061A5C0`, `LeaderData::has_tribe_bonus` `0x006E1370`,
`LeaderData::type_avail` `0x006E33A0`, `LeaderData::is_ally` `0x006EDB50`,
`Group::action_swarm_around` `0x0070FBE0`, and target repair vslot `+0x168`
(`Build::repair_damage`/object repair boundary at `0x00628130`).

## Honest boundary and integration map

`repair_order.rs` is independent of Arena/World and exposes `plan_repair`, ordered
`RepairEffect`s, and one `AtomicRepairHost::commit_repair` receipt. The planner is complete
only when its `RepairFacts` are a coherent same-version snapshot. The adapter must still:

1. add `pub mod repair_order;` to `systems/mod.rs`;
2. replace `OrderIndex::Repair => Unimplemented` in `order_dispatch.rs` with a host-fact
   snapshot, `execute_repair`, and a `Port::Complete` receipt;
3. make one atomic callback own order removal, fallback Group construction/swarm, target
   helper mutation, decoded/encrypted stock writes, repair damage, repair stamp, and local
   UI/sound; and
4. change `check_target_path(REPAIR)` only after its separately recovered retail path
   predicate is wired. `Unit::do_repair` does not prove that predicate.

Still host-owned/opaque by design: the semantic names behind target vslots `+0x78`,
`+0x114`, `+0x11c`, `+0x18c`, unit vslot `+0xf8`, the two City bytes, live diplomacy and
terrain-owner lookup, and local UI/sound construction. Their exact returned values and
branch use are preserved; guessing their broader meaning is not required for the planner.
Until the atomic adapter exists, the honest closure class is isolated deterministic
`Port::Planned`, not live `Port::Complete`.
