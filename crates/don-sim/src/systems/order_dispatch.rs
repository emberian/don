//! `systems::order_dispatch` — `Unit::work` `0x0060D180` and a real `Unit::do_job`
//! `0x00617A10` dispatch, plus the `OrderList` maintenance the two of them ride on.
//!
//! Ported from `riseofnations.exe` (PE32 i386, image base `0x00400000`) with `ron-bin/sbl/
//! rise.pdb` supplying names, class layouts and code sizes. Claims are `[measured]` when
//! verified here against the instruction stream or a PDB type record, and `UNVERIFIED` when
//! taken from Ghidra structure that has not been checked against behaviour. No differential
//! test against retail has been run for this lane, so the fidelity tier is **C** throughout;
//! nothing here is verified in the proof-assistant sense.
//!
//! # Why this module exists
//!
//! `docs/mechanics/COVERAGE.md` §3 records that `Unit::work` `0x0060D180` (2,885 B) — "the
//! order-list driver that sits between `Unit::process` and `do_job`, and that owns
//! `update_order` / `repath` / `check_target_path` / `kill_current_order`" — was **uncited by
//! any Rust file**, so "even the three implemented arms have no retail-derived thing to
//! dispatch them". [`crate::world::World::unit_work`] reads the head order's type and matches
//! on it; that is the *jump table* but not the *driver*, and orders there never advance
//! through a queue, never retire on a path failure, and never notice a stale target.
//!
//! This module is the driver. It also gives [`crate::systems::movement`] — the derived unit
//! pathfinder and integrator — its first caller: [`do_move`] runs
//! [`movement::PathFinder::find_upath_prepare`], [`movement::PathFinder::astar_path_unit`],
//! [`movement::PathFinder::compress_path`] and [`movement::move_step`] against a real order
//! queue, so a `MOVE_TO` now consumes waypoints and retires on arrival.
//!
//! # The three retail objects this reproduces
//!
//! ```text
//! UnitData                              344 B   [PDB]
//!   +200 orderlist : OrderList           28 B
//!          +0   vfptr
//!          +4   LinkListBase<UnitOrder*, unsigned char, RecycledOrderNode>   24 B
//!                 +0  current_data   UnitOrder*        -> UnitData+204
//!                 +4  current_metric unsigned char     -> UnitData+208
//!                 +8  current_node   RecycledOrderNode*-> UnitData+212
//!                 +12 length         int               -> UnitData+216
//!                 +16 head_node      RecycledOrderNode*-> UnitData+220
//!                 +20 ordered        int               -> UnitData+224
//!   RecycledOrderNode                    16 B   { next@0, prev@4, data@8, metric@12 }
//! ```
//!
//! Every order-list access in `Unit::work`, `Unit::update_order` `0x006179D0`,
//! `UnitData::order_type` `0x00616E80`, `UnitData::get_action` `0x00608450`,
//! `Unit::update_action` `0x0060A870`, `Unit::repath` `0x005E29B0` and
//! `Unit::kill_current_order` `0x005E2CB0` begins with the same four-instruction inlined
//! idiom [measured, identical byte pattern in all seven]:
//!
//! ```text
//!   node          = head_node->prev      ; [head_node + 4]
//!   current_node  = node
//!   current_data  = node->data           ; [node + 8]
//!   current_metric= node->metric         ; [node + 12]
//! ```
//!
//! That is `LinkListBase::reset()`. **The list is circular and linked backwards**: iteration
//! advances by `->prev`, `head_node` is the *last* element in iteration order, and
//! `head_node->prev` is therefore the **front** — the order currently being executed. An
//! empty list is `head_node == NULL`. `UnitData::get_action` `0x00608450` walks with
//! `current_node = current_node->prev` and stops when `current_node == head_node`, i.e. after
//! visiting every node exactly once. [measured]
//!
//! [`OrderQueue`] reproduces those *semantics* — front is current, a cursor walks front to
//! back, `remove_current` unlinks — over a `Vec`, because the linked shape exists only to
//! serve `OrdersMemManager`'s node recycling (`Game::do_frame` step 21,
//! `OrdersMemManager::cycle` `0x00730E20`), which we do not need. **Caveat that is not
//! cosmetic:** `Array<T>`'s capacity and growth hint are checksummed elsewhere in the engine;
//! `OrderList` is a linked list, not an `Array`, so a `Vec` here does not carry that hazard,
//! and `Unit::walk_data` `0x0060CF40` walks 111 of `UnitData`'s 344 bytes and does not include
//! the list nodes.
//!
//! # `Unit::do_job`, verbatim
//!
//! `Unit::do_job(enum OrderIndex, class UnitOrder*)` `0x00617A10` is 500 bytes and is a bare
//! `switch` over the 28-entry table at `0x00617B94`. Its tail call from `Unit::work` is
//! [measured] at `0x0060DAD4`:
//!
//! ```text
//!   0060dad4  push edi        ; arg2 = UnitOrder*  (the head order)
//!   0060dad5  push esi        ; arg1 = OrderIndex  (its type)
//!   0060dad6  mov  ecx, ebx   ; this = Unit*
//!   0060dad8  call 0x617a10   ; Unit::do_job
//! ```
//!
//! Both surprises in the table survive here and are asserted in the tests: `FLEE_TO` (4) and
//! `MOVE_TO` (1) share `Unit::do_move` `0x005F7B30`, and `PATROL` (5) has **no case label** —
//! it falls to the `switch` default and does nothing. The jump-table addresses themselves
//! live in [`crate::order::EXECUTORS`]; this module owns only the per-arm *implementation*
//! status ([`ARMS`]) and cross-checks the two tables agree.
//!
//! # Failure versus completion, which is structural rather than a flag
//!
//! `Unit::kill_current_order(int)` takes one argument, but of the **120 call sites of
//! `kill_current_order` and `repath` in `.text`** [measured, direct `call rel32` scan] almost
//! every one passes `0`; the argument suppresses arrival bookkeeping, not failure semantics.
//! The distinction the engine actually draws is a *call pair*:
//!
//! | retail sequence | meaning |
//! |---|---|
//! | `kill_current_order(0)` alone | the order **completed**; the next queued order becomes current |
//! | `repath(); kill_current_order(0)` | the order **failed**; the queued movement legs are stripped first |
//!
//! `Unit::repath` `0x005E29B0` is not "recompute a path". It strips every *leading* order
//! whose type is in `{MOVE_TO, ATTACK_TO, EXPLORE_TO, FLEE_TO, CHANGE_FORM, GROUP_MOVE,
//! GROUP_ATTACK_TO}` and returns the moment the front order is not one of those [measured].
//! So `repath(); kill_current_order(0)` means "throw away the walk *and* the thing it was
//! walking to". That pair appears at `0x0060D948` in `Unit::work` itself (stale target),
//! inside `Unit::check_target_path` `0x005E22D0`, and at 14 other sites.
//!
//! [`KillReason`] names the distinction so a caller can count it; it is **our label on a
//! derived call pattern**, not a retail field.
//!
//! # The one cascade worth knowing
//!
//! [measured, `0x005F8B5F`] when `Unit::do_move`'s path search fails it retires the move and
//! then looks at what is now in front:
//!
//! ```text
//!   005f8b5f  call 0x5e2cb0     ; kill_current_order(0)      -- retire the failed move
//!   005f8b66  call 0x616e80     ; UnitData::order_type()
//!   005f8b6b  cmp  eax, 0xa     ; ATTACK
//!   005f8b6e  je   0x5f82a3     ; -> kill_current_order(0) again
//!   005f8b76  call 0x616e80
//!   005f8b7b  cmp  eax, 6       ; BUILD_AT
//!   005f8b7e  jne  0x5f82ac     ; -> return 1
//!   005f8b84  jmp  0x5f82a3     ; -> kill_current_order(0) again
//! ```
//!
//! An unreachable destination therefore cancels the follow-on `ATTACK` or `BUILD_AT` as well,
//! and only those two. [`do_move`] reproduces it.
//!
//! # RNG hazard, restated because it is easy to lose
//!
//! A failed unit search obliges the engine to draw `Random::get(0, 0xFFFF) % 3 + 6` from
//! `GameAccess::game_random` — the **main simulation stream** — in `astar_path`'s failure
//! epilogue (`0x006848C4`, `0x00684E02`). [`movement::PathFinder::pending_retry_draw`] raises
//! a flag rather than drawing, because that module owns no stream. This module **services**
//! the flag through [`WorkWorld::draw_path_retry_delay`], so a host that wires a real
//! [`crate::rng::Random`] keeps the stream aligned. Skipping the draw shifts every later draw
//! in the tick; it is a whole-sim desync, not a local inaccuracy.
//!
//! # What is *not* here
//!
//! * `Unit::fight` `0x005FD4D0` (8,157 B) and `Unit::find_attack_pos` `0x00601280` (7,124 B)
//!   — target *selection*. [`do_attack`] executes an order that already names a target.
//! * `Unit::detect_unit_collision` `0x00617060` / `Unit::resolve_unit_collision` `0x005F9D30`
//!   — [`movement`] does not port them either; a blocked step reports `Blocked`.
//! * `Caravan::build_road` `0x0073DB10`, the caravan arm of `Unit::work` at `0x0060D2xx`.
//! * `Unit::detect_boat_collision` `0x005FA8B0`, called just before `do_job` when the unit
//!   collided within the last four frames. The *gate* is reproduced and counted; the body is
//!   not ported.
//! * 10 of the 28 `do_job` arms. They dispatch and are counted; see [`ARMS`].

use crate::command::QueuePos;
use crate::order::{
    ArmStatus, FollowOrderPayload, FormOrderState, Order, OrderIndex, SpecialAnimOrderState,
    NUM_UNIT_ORDERS, ORDER_GROUP, ORDER_PATHED,
};
use crate::systems::construction::{BuilderFinish, ObjectKey};
use crate::systems::construction_builder::{
    self, AfterInvalidTarget, PreflightInput as BuildAtPreflightInput, PreflightPlan,
};
use crate::systems::follow_executor::{
    FollowActorFacts, FollowExecutorEffect, FollowExecutorReceipt, FollowExecutorRequest,
    FollowExecutorTransactionStatus, FollowIdentity, FollowMoveFacingTail, FollowOrderState,
};
use crate::systems::garrison_dispatch::GarrisonHostError;
use crate::systems::groups_guys::{GuyData, GuyEnv, UnitTypeStats};
use crate::systems::guard_dispatch::GuardHostError;
use crate::systems::movement::{
    self, vector_dist, Body, MoveStep, MoveTurnProfile, PathData, PathFinder, PathStack, PathUnit,
    SearchArgs, SearchResult, UPathOutcome, UnitWorld,
};
use crate::systems::patrol::{
    self, AirPatrolAction, AirPatrolAfterPhysics, AirPatrolOrder, AirPatrolTarget,
    GroundPatrolAction, GroupMoveRequest, GroupPatrolOrder, StrafeOrder,
};
use crate::systems::repair_order;
use crate::systems::special_anim_executor::{
    self, ActorFacts as SpecialAnimActorFacts, ObjectIdentity as SpecialAnimObjectIdentity,
    SpecialAnimBranch, SpecialAnimExecutorPlan, SpecialAnimExecutorReceipt,
    SpecialAnimExecutorRequest, SpecialAnimHostSnapshot, SpecialAnimKind, SpecialAnimState,
};
use crate::systems::targeted_order_plans::{
    self, AirAttackGroundFacts, AirAttackGroundOrderState, AttackGroundFacts,
    AttackGroundOrderState, ExploreToFacts, HostFact, OrderEffect,
};
use crate::systems::terminal_order_plans::{
    ChangeFormOrderFacts, TerminalOrderActor, TerminalOrderReceipt, TerminalOrderRequest,
    TerminalOrderStatus,
};

// ---------------------------------------------------------------------------
// 1. Field offsets, from the PDB type stream  [measured]
// ---------------------------------------------------------------------------

/// Byte offsets of every field `Unit::work` touches, so a live-memory reader and this port
/// cannot drift. All from `ron-bin/sbl/rise.pdb` LF_FIELDLIST records [measured].
///
/// `SubObjectData` is the root of `Object` (`SubObject` 40 B -> `ObjectData` 80 B ->
/// `Object` 80 B -> `UnitData` 344 B), which is why the small offsets are object-wide.
pub mod offsets {
    // ---- SubObjectData ----
    /// `SubObjectData::flags`, `unsigned char`. `Unit::work` clears bit 7 and bit 3.
    pub const OBJ_FLAGS: usize = 8;
    /// `SubObjectData::who`, `unsigned char` — the owning player slot.
    pub const OBJ_WHO: usize = 9;
    /// `SubObjectData::o`, `short` — index within the owner's object band. `Unit::work`
    /// uses it as the **phase** of its periodic checks.
    pub const OBJ_O: usize = 10;
    /// `SubObjectData::x_internal`, stored XOR `0x00063637`.
    pub const OBJ_X: usize = 16;
    /// `SubObjectData::y_internal`, stored XOR `0x00063637`.
    pub const OBJ_Y: usize = 20;
    /// `SubObjectData::ptype`, `ObjectType*`.
    pub const OBJ_PTYPE: usize = 24;
    // ---- ObjectData ----
    /// `ObjectData::visible`, `char`. Cleared every 32nd frame when `flags` bit 7 is clear.
    pub const OBJ_VISIBLE: usize = 64;
    // ---- UnitData ----
    pub const UD_COLLIDE_FRAME: usize = 72;
    pub const UD_ANGLE: usize = 80;
    pub const UD_DEST_ANGLE: usize = 88;
    /// `UnitData::tolerance` — the arrival radius `Unit::do_move` compares `vector_dist`
    /// against at `0x005F87xx` [measured].
    pub const UD_TOLERANCE: usize = 96;
    pub const UD_UNIT_MASKS: usize = 104;
    pub const UD_UNIT_MASKS2: usize = 108;
    /// `UnitData::orders_x` — written by `Unit::update_action` `0x0060A870`.
    pub const UD_ORDERS_X: usize = 112;
    pub const UD_ORDERS_Y: usize = 116;
    pub const UD_GROUP: usize = 128;
    pub const UD_INSIDE_UP: usize = 130;
    pub const UD_SPELL_TIME: usize = 152;
    pub const UD_MYSPEED: usize = 154;
    pub const UD_RECHARGING: usize = 174;
    pub const UD_IDLE: usize = 176;
    pub const UD_SAFE: usize = 178;
    /// `UnitData::path`, `Stack<PathData>`. On the `units` checksum channel.
    pub const UD_PATH: usize = 184;
    /// `UnitData::orderlist`, `OrderList` (28 B).
    pub const UD_ORDERLIST: usize = 200;
    /// `UnitData::openlist` — a parked `PathFinder` search. Non-null means a suspended path.
    pub const UD_OPENLIST: usize = 260;

    // ---- OrderList internals, absolute within UnitData ----
    pub const OL_CURRENT_DATA: usize = 204;
    pub const OL_CURRENT_METRIC: usize = 208;
    pub const OL_CURRENT_NODE: usize = 212;
    pub const OL_LENGTH: usize = 216;
    pub const OL_HEAD_NODE: usize = 220;
    pub const OL_ORDERED: usize = 224;

    // ---- elsewhere ----
    /// `Game::frame`, read through `[0x00C061EC]`. `Unit::work` reads it four times.
    pub const GAME_FRAME: usize = 0x550;
    /// `Leader` stride in the flat leader array at `0x00E3A390`.
    pub const LEADER_STRIDE: usize = 0x6EEC;
}

/// `UnitData::unit_masks` bits `Unit::work` reads or writes. **The bit positions are
/// [measured]; the names are ours** — none of these has a shipped name we have recovered, so
/// treat every name as a hypothesis and the position as fact.
pub mod masks {
    /// Cleared on every 32nd frame at `0x0060D1CB` (`and [ebx+0x68], 0xFFFFFFFB`).
    pub const PERIODIC_32: u32 = 0x0000_0004;
    /// Cleared by `Unit::do_move`'s arrival path; set by `kill_current_order`'s facing arm.
    pub const ARRIVED_FACING: u32 = 0x0000_0002;
    /// Marks the currently installed movement leg as usable. `Unit::move_step` clears it when
    /// `invalid_loc` rejects a tile, causing `Unit::do_move` to call `Unit::find_path` again on
    /// the following frame (`test [ebx+0x68], 8`). The historical Rust name predates recovery
    /// of that control flow and is therefore misleading; the bit position is measured.
    pub const PATH_EXHAUSTED: u32 = 0x0000_0008;
    /// Cleared at `0x0060D1F1` when the head order carries `ORDER_GROUP` and is not
    /// `EXPLORE_TO`.
    pub const GROUP_PENDING: u32 = 0x0000_0100;
    /// Suppresses the "wants work" flag set at the tail of `Unit::work`.
    pub const NO_AUTO_WORK: u32 = 0x0000_0800;
    /// Set at the tail of `Unit::work` when the unit has an order and its type opts in.
    pub const WANTS_WORK: u32 = 0x0000_1000;
    /// Cleared unconditionally by `Unit::kill_current_order` at its first instruction.
    pub const ORDER_TRANSIENT: u32 = 0x0002_0000;
    /// Selects the snap-to-destination arm of `Unit::work`'s movement branch.
    pub const SNAP_DEST: u32 = 0x0008_0000;
    /// `Unit::move_step` halves speed and clears this latch when the residual heading is under
    /// its distance-scaled threshold [measured `0x005FB39B..0x005FB3D0`].
    pub const HALF_SPEED_ON_TURN: u32 = 0x0010_0000;
    /// Selects the "facing move" fixup at `0x0060D36A`.
    pub const FACING_MOVE: u32 = 0x0400_0000;
    /// Cleared by the last instruction of `Unit::work` (`and [ebx+0x68], 0xFFFFFFEF`).
    pub const WORKED_THIS_FRAME: u32 = 0x0000_0010;
}

/// `SubObjectData::flags` bits `Unit::work` and `Unit::do_gather` touch. Positions
/// [measured]; names ours.
pub mod obj_flags {
    /// Tested at `0x0060DAE8` before the supply-spread nudge.
    pub const ACTIVE: u8 = 0x01;
    /// Set then cleared by the leader-notify block at `0x0060D733`.
    pub const NOTIFY_LEADER: u8 = 0x08;
    /// Set by `Unit::do_gather` when it abandons a gather because the node is full
    /// [measured, `0x005EF63E` and `0x005EF68F`: `or byte ptr [ebx+8], 0x10`].
    pub const GATHER_REFUSED: u8 = 0x10;
    /// Cleared at `0x0060D1D4` (`and byte ptr [ebx+8], 0x7f`) unless the order is
    /// `CAST_SPELL`.
    pub const CASTING: u8 = 0x80;
}

// ---------------------------------------------------------------------------
// 2. Order classification  [measured for MOVE_LIKE, UNVERIFIED for TARGETED]
// ---------------------------------------------------------------------------

/// The seven order types the engine treats as *movement*.
///
/// This exact set is compared, as a `cmp/je` chain, in **four** independent places
/// [measured]:
///
/// * `Unit::work` `0x0060D2Fx` — chooses the movement branch;
/// * `Unit::repath` `0x005E29Fx` — the orders it strips;
/// * `Unit::kill_current_order` `0x005E2CFx` — the arrival-bookkeeping arm;
/// * `Unit::work` `0x0060DAxx` — suppresses `detect_boat_collision`.
///
/// Four agreeing sites is why this is a set constant and not a guess. It is also the
/// override set of the virtual `UnitOrder::is_move` (vtable `+0x14`) as far as we can tell —
/// that inference is **UNVERIFIED**, since `is_move`'s per-class overrides are all folded to
/// the shared `mov eax,1; ret` stub at `0x0047EF8E` by identical-COMDAT folding.
pub const MOVE_LIKE: [OrderIndex; 7] = [
    OrderIndex::MoveTo,
    OrderIndex::AttackTo,
    OrderIndex::ExploreTo,
    OrderIndex::FleeTo,
    OrderIndex::ChangeForm,
    OrderIndex::GroupMove,
    OrderIndex::GroupAttackTo,
];

/// Order types whose concrete class derives from `TargetOrder` (32 B, `ox@8`, `whom@12`,
/// `uid@16`). **UNVERIFIED**: taken from which arms dereference a `TargetOrder` in the
/// decompiled bodies, not from the class hierarchy, because `UnitOrder::is_targeted`'s
/// overrides are ICF-folded exactly like `is_move`.
pub const TARGETED: [OrderIndex; 14] = [
    OrderIndex::AttackTo,
    OrderIndex::Gather,
    OrderIndex::BoardShip,
    OrderIndex::AwaitBoard,
    OrderIndex::Attack,
    OrderIndex::Follow,
    OrderIndex::Guard,
    OrderIndex::Repair,
    OrderIndex::CastSpell,
    OrderIndex::TradeRoute,
    OrderIndex::Strafe,
    OrderIndex::GroupAttack,
    OrderIndex::GroupAttackTo,
    OrderIndex::Garrison,
];

#[inline]
pub fn is_move_like(k: OrderIndex) -> bool {
    matches!(
        k,
        OrderIndex::MoveTo
            | OrderIndex::AttackTo
            | OrderIndex::ExploreTo
            | OrderIndex::FleeTo
            | OrderIndex::ChangeForm
            | OrderIndex::GroupMove
            | OrderIndex::GroupAttackTo
    )
}

#[inline]
pub fn is_targeted(k: OrderIndex) -> bool {
    TARGETED.contains(&k)
}

// ---------------------------------------------------------------------------
// 3. `UnitOrder`, flattened with the fields the executors actually read
// ---------------------------------------------------------------------------

/// One `UnitOrder`, flattened.
///
/// [`crate::order::Order`] is the descriptive union (kind, flags, x/y, target, tolerance).
/// This is the **executable** one: it adds the `MoveOrder` (92 B) retry state machine and the
/// `TargetOrder` (32 B) identity triple, because `Unit::do_move` and `Unit::check_target_path`
/// branch on them. Field names and offsets are from the PDB [measured]:
///
/// ```text
/// UnitOrder    8 B  { vfptr@0, flags@4 }              -- a virtual base at the TAIL of each
///                                                        concrete order (MoveOrder+84)
/// MoveOrder   92 B  { x@4 y@8 angle@12 dest@16 tolerance@20 pause@24 retry@28 attempts@32
///                     timer@36 facing@40 dest_x@44 dest_y@48 last_x@52 last_y@56
///                     coll_x@60 coll_y@64 orig_x@68 orig_y@72 off_x@76 off_y@78 }
/// TargetOrder 32 B  { ox@8, whom@12, uid@16 }
/// ```
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct OrderRec {
    pub kind: OrderIndex,
    /// `RecycledOrderNode::metric`, walked before the concrete order in DoNSave v13.
    pub node_metric: u8,
    /// `UnitOrder::flags` at `+4`. See `crate::order::ORDER_*`.
    pub flags: u8,

    // ---- MoveOrder ----
    /// `MoveOrder::x` / `::y` at `+4` / `+8` — the destination in world Coord units.
    pub x: i32,
    pub y: i32,
    /// `MoveOrder::angle` at `+12` — the facing to adopt on arrival.
    pub angle: i32,
    /// `MoveOrder::dest` at `+16` — non-zero once a path exists.
    pub dest: i32,
    /// `MoveOrder::tolerance` at `+20`. `0` means "use `UnitData::tolerance`".
    pub tolerance: i32,
    /// `MoveOrder::pause` at `+24`. A non-zero value is decremented and holds movement for the
    /// frame at `0x005F8C88`; collision resolution writes its retail wait here.
    pub pause: i32,
    /// `MoveOrder::retry` at `+28` — the 6-to-8 tick delay a failed search installs.
    pub retry: i32,
    /// `MoveOrder::attempts` at `+32`.
    pub attempts: i32,
    /// `MoveOrder::timer` at `+36`. `Unit::do_move` retires the order when it reaches 1
    /// [measured, `if (0 < timer) { if (timer == 1) { kill_current_order(0); ... } }`]. The
    /// `timer > 1` arm decrements the value and returns for this frame [measured
    /// `0x005F7FA7..0x005F7FB6`].
    pub timer: i32,
    /// `MoveOrder::facing` at `+40`.
    pub facing: i32,
    /// `MoveOrder::dest_x` / `::dest_y` at `+44` / `+48`.
    pub dest_x: i32,
    pub dest_y: i32,
    /// `MoveOrder::last_x` / `::last_y` at `+52` / `+56`. Set to `-1` by the retry arm.
    pub last_x: i32,
    pub last_y: i32,
    /// `MoveOrder::coll_x/coll_y` at `+60/+64` and `orig_x/orig_y` at `+68/+72`.
    pub coll_x: i32,
    pub coll_y: i32,
    pub orig_x: i32,
    pub orig_y: i32,
    /// Signed remainders of the initial destination modulo one WCoord cell (`0x300`), at
    /// `+76/+78`. The ground-patrol executor writes these directly.
    pub off_x: i16,
    pub off_y: i16,

    // ---- GroupOrder / GroupMoveOrder ----
    /// `GroupOrder::oxx` at concrete offset `+84`: the formation leader's owner-local
    /// object index.
    pub group_oxx: i32,
    /// `GroupOrder::whose` at concrete offset `+88`.
    pub group_whose: i32,
    /// `GroupOrder::id` at concrete offset `+92`. `Group::action_move_near` constructs
    /// this as `(Game::frame * 10 + group.id) * 100 + group.order_num`.
    pub group_id: i32,
    /// `GroupOrder::form_id` at concrete offset `+96`. Despite the PDB field name, the
    /// shipped GROUP_MOVE constructor receives `action_move_near`'s `set_angle` value.
    pub group_form_id: i32,
    /// `GroupOrder::group_angle` at concrete offset `+100`.
    pub group_angle: i32,
    /// `GroupMoveOrder::in_group` at concrete offset `+104`; its clear constructor
    /// initializes this to zero.
    pub in_group: i32,

    // ---- AttackOrder / GroupAttackOrder ----
    /// `AttackOrder::def_x/def_y` at concrete offsets `+20/+24`.
    pub attack_def_x: i32,
    pub attack_def_y: i32,
    /// `AttackOrder::{mandatory,defensive,in_range,ever_in_range,new_ord}` at `+28..+32`.
    pub attack_mandatory: u8,
    pub attack_defensive: u8,
    pub attack_in_range: u8,
    pub attack_ever_in_range: u8,
    pub attack_new_ord: u8,
    /// `GroupAttackOrder::{temporary,oxxx,whosoever}` at concrete offsets `+60/+64/+68`.
    pub group_attack_temporary: i32,
    pub group_attack_oxxx: i32,
    pub group_attack_whosoever: i32,

    // ---- TargetOrder ----
    /// `TargetOrder::ox` at `+8` — the target's index in its owner's object band.
    pub target_o: i32,
    /// `TargetOrder::whom` at `+12` — the target's owner slot.
    pub target_who: i32,
    /// `TargetOrder::uid` at `+16` — `ObjectData::uid`, the staleness token. A mismatch is
    /// what `Unit::work`'s tail treats as "the target you named is not there any more".
    pub target_uid: u16,
    /// Additive stable port identity retained with the retail `(who,o,uid)` triple.
    /// Production consumers must require every duplicated field to agree before acting.
    pub target_handle: Option<crate::Handle>,

    /// Complete concrete payload for `FollowOrder` (order 11). The duplicated primary
    /// identity must agree with `target_o/target_who/target_uid`; a mismatch is malformed.
    pub follow: Option<FollowOrderPayload>,

    /// Complete concrete payload for `SpecialAnimOrder` (order 25). This is separate from
    /// `x/y` and target identity because its nine walked words are not layout-compatible
    /// with either generic descriptive union.
    pub special_anim: Option<SpecialAnimOrderState>,

    /// Complete concrete payload for `FormOrder` (order 18). `angle` is duplicated in the
    /// common `MoveOrder` slot above because `update_action` reads it generically; the
    /// dispatcher rejects a mismatch rather than choosing one copy.
    pub form_order: Option<FormOrderState>,

    /// Concrete checksum-visible storage for the coordinate-target order classes. These
    /// classes are not layout-compatible with `MoveOrder` or `TargetOrder`, so retaining
    /// only the generic `x/y` union would discard `attack_unit` and the walked `AirOrder`
    /// base. A mismatched variant is rejected as [`ArmResult::MalformedOrder`].
    pub targeted_payload: TargetedOrderPayload,

    /// Concrete fields carried only by the three order classes patrol creates or executes.
    /// Retail stores these in dynamically-sized class instances; keeping the payload on the
    /// queue node preserves the same per-order ownership and permits routes of any length.
    pub patrol_payload: PatrolPayload,

    /// Exact suffix for the typed economy-order cohort.
    pub economy: Option<crate::systems::economy_order_payload_authority::EconomyOrderPayload>,

    /// Complete concrete payload for `GuardOrder` (order 12, `sizeof=56`). `dx/dy` and the
    /// snapped `guard_x/guard_y` post are not layout-compatible with `MoveOrder`'s `x/y`, and
    /// `idle`/`retry` are separate words from `MoveOrder::retry`, so GUARD gets its own node
    /// payload rather than borrowing the generic union.
    /// See [`crate::systems::guard_dispatch`].
    pub guard: Option<crate::systems::guard_order::GuardOrderState>,

    /// Complete concrete payload for `GarrisonOrder` (order 26, `sizeof=36`). The `search`
    /// word decides whether a full building looks for an alternate in the same city and is
    /// therefore checksum-visible state, not a call argument.
    /// See [`crate::systems::garrison_dispatch`].
    pub garrison: Option<crate::systems::garrison_order::GarrisonOrderState>,
}

/// Concrete storage owned by the three coordinate-target executor classes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TargetedOrderPayload {
    #[default]
    None,
    AttackGround(AttackGroundOrderState),
    AirAttackGround(AirAttackGroundOrderState),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum PatrolPayload {
    #[default]
    None,
    Group(GroupPatrolOrder),
    Air(AirPatrolOrder),
    Strafe(StrafeOrder),
}

impl PatrolPayload {
    /// Heap storage owned by the dynamically-sized patrol waypoint arrays.
    ///
    /// The order record itself is accounted for by [`OrderQueue::bytes_reserved`]; only
    /// the two `SimpleArray<Coord>`-shaped vector allocations live out of line.
    pub fn bytes_reserved(&self) -> usize {
        let points = match self {
            PatrolPayload::Group(order) => Some(&order.points),
            PatrolPayload::Air(order) => Some(&order.points),
            PatrolPayload::None | PatrolPayload::Strafe(_) => None,
        };
        points.map_or(0, |p| {
            (p.x.capacity() + p.y.capacity()) * std::mem::size_of::<i32>()
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatrolInstall {
    Replaced,
    AppendedOrder,
    ExtendedWaypoints,
}

impl Default for OrderRec {
    fn default() -> OrderRec {
        OrderRec {
            kind: OrderIndex::None,
            node_metric: 0,
            flags: 0,
            x: 0,
            y: 0,
            angle: 0,
            dest: 0,
            tolerance: 0,
            pause: 0,
            retry: 0,
            attempts: 0,
            timer: 0,
            facing: 0,
            dest_x: 0,
            dest_y: 0,
            last_x: -1,
            last_y: -1,
            coll_x: 0,
            coll_y: 0,
            orig_x: 0,
            orig_y: 0,
            off_x: 0,
            off_y: 0,
            group_oxx: -1,
            group_whose: -1,
            group_id: -1,
            group_form_id: 0,
            group_angle: 0,
            in_group: 0,
            attack_def_x: 0,
            attack_def_y: 0,
            attack_mandatory: 0,
            attack_defensive: 0,
            attack_in_range: 0,
            attack_ever_in_range: 0,
            attack_new_ord: 0,
            group_attack_temporary: 0,
            group_attack_oxxx: -1,
            group_attack_whosoever: -1,
            target_o: -1,
            target_who: -1,
            target_uid: 0,
            target_handle: None,
            follow: None,
            special_anim: None,
            form_order: None,
            targeted_payload: TargetedOrderPayload::None,
            patrol_payload: PatrolPayload::None,
            economy: None,
            guard: None,
            garrison: None,
        }
    }
}

impl OrderRec {
    pub fn move_to(x: i32, y: i32, tolerance: i32) -> OrderRec {
        OrderRec {
            kind: OrderIndex::MoveTo,
            x,
            y,
            dest_x: x,
            dest_y: y,
            tolerance,
            ..OrderRec::default()
        }
    }

    pub fn attack(who: i32, o: i32, uid: u16) -> OrderRec {
        OrderRec {
            kind: OrderIndex::Attack,
            target_who: who,
            target_o: o,
            target_uid: uid,
            ..OrderRec::default()
        }
    }

    pub fn gather(who: i32, o: i32, uid: u16) -> OrderRec {
        OrderRec {
            kind: OrderIndex::Gather,
            target_who: who,
            target_o: o,
            target_uid: uid,
            ..OrderRec::default()
        }
    }

    pub fn follow(payload: FollowOrderPayload) -> OrderRec {
        OrderRec {
            kind: OrderIndex::Follow,
            flags: ORDER_GROUP,
            target_o: payload.ox,
            target_who: payload.whom,
            target_uid: payload.uid,
            follow: Some(payload),
            ..OrderRec::default()
        }
    }

    /// Construct one complete `SpecialAnimOrder` node without narrowing its nine walked words.
    pub fn special_anim(state: SpecialAnimOrderState) -> OrderRec {
        OrderRec {
            kind: OrderIndex::SpecialAnim,
            flags: ORDER_GROUP,
            special_anim: Some(state),
            ..OrderRec::default()
        }
    }

    pub fn attack_ground(state: AttackGroundOrderState) -> OrderRec {
        OrderRec {
            kind: OrderIndex::AttackGround,
            x: state.att_x,
            y: state.att_y,
            targeted_payload: TargetedOrderPayload::AttackGround(state),
            ..OrderRec::default()
        }
    }

    pub fn air_attack_ground(state: AirAttackGroundOrderState) -> OrderRec {
        OrderRec {
            kind: OrderIndex::AirAttackGround,
            x: state.attack.att_x,
            y: state.attack.att_y,
            targeted_payload: TargetedOrderPayload::AirAttackGround(state),
            ..OrderRec::default()
        }
    }

    pub fn of_kind(kind: OrderIndex) -> OrderRec {
        OrderRec {
            kind,
            ..OrderRec::default()
        }
    }

    pub fn change_form(angle: i32, new_form: i32, delay: i32) -> OrderRec {
        OrderRec {
            kind: OrderIndex::ChangeForm,
            angle,
            form_order: Some(FormOrderState {
                angle,
                new_form,
                delay,
            }),
            ..OrderRec::default()
        }
    }

    pub fn group_patrol(order: GroupPatrolOrder) -> OrderRec {
        OrderRec {
            kind: OrderIndex::GroupPatrol,
            flags: ORDER_GROUP,
            patrol_payload: PatrolPayload::Group(order),
            ..OrderRec::default()
        }
    }

    pub fn air_patrol(order: AirPatrolOrder, group: bool) -> OrderRec {
        OrderRec {
            kind: OrderIndex::AirPatrol,
            flags: if group { ORDER_GROUP } else { 0 },
            patrol_payload: PatrolPayload::Air(order),
            ..OrderRec::default()
        }
    }

    pub fn strafe(order: StrafeOrder) -> OrderRec {
        OrderRec {
            kind: OrderIndex::Strafe,
            target_o: order.target_o,
            target_who: order.target_who,
            target_uid: order.target_uid,
            patrol_payload: PatrolPayload::Strafe(order),
            ..OrderRec::default()
        }
    }

    #[inline]
    pub fn has(&self, bit: u8) -> bool {
        self.flags & bit != 0
    }

    /// The virtual `UnitOrder::is_move` (vtable `+0x14`). See [`MOVE_LIKE`].
    #[inline]
    pub fn is_move(&self) -> bool {
        is_move_like(self.kind)
    }

    /// The virtual `UnitOrder::is_targeted`. See [`TARGETED`].
    #[inline]
    pub fn is_targeted(&self) -> bool {
        is_targeted(self.kind)
    }

    /// The virtual `UnitOrder::is_group` — the `ORDER_GROUP` bit of `UnitOrder::flags`.
    #[inline]
    pub fn is_group(&self) -> bool {
        self.has(ORDER_GROUP)
    }

    /// The virtual `UnitOrder::is_pathed` — the `ORDER_PATHED` bit.
    #[inline]
    pub fn is_pathed(&self) -> bool {
        self.has(ORDER_PATHED)
    }
}

/// The measured `Group::action_patrol` / `Unit::add_patrol_order` installation semantics.
///
/// Patrol is one of the exceptions to the generic group insert dance: `QUEUE_FIRST` is
/// normalized to `QUEUE_NEW`. `QUEUE_LAST` extends the active `GROUP_PATROL` when there is
/// one; otherwise it appends a distinct patrol order.
#[allow(clippy::too_many_arguments)]
pub fn install_group_patrol(
    u: &mut UnitWork,
    start_x: i32,
    start_y: i32,
    target_x: i32,
    target_y: i32,
    id: i32,
    form_id: i32,
    oxx: i32,
    whose: i32,
    queue: QueuePos,
) -> PatrolInstall {
    if queue == QueuePos::Last {
        let action = update_action(u);
        if action
            .as_ref()
            .is_some_and(|o| o.kind == OrderIndex::GroupPatrol)
        {
            if let Some(current) = u.orders.current_mut() {
                if let PatrolPayload::Group(order) = &mut current.patrol_payload {
                    // Group::action_patrol's extension arm writes the command Coord
                    // directly; it does not repeat add_patrol_order's UCoord centering.
                    order.points.push(target_x, target_y);
                    current.flags |= ORDER_GROUP;
                    update_action(u);
                    return PatrolInstall::ExtendedWaypoints;
                }
            }
        }
    }

    let order = OrderRec::group_patrol(patrol::new_group_patrol(
        start_x, start_y, target_x, target_y, id, form_id, oxx, whose,
    ));
    let result = if queue == QueuePos::Last {
        u.orders.push_back(order);
        PatrolInstall::AppendedOrder
    } else {
        u.orders.replace(order);
        clear_partial_path(u);
        PatrolInstall::Replaced
    };
    update_action(u);
    result
}

/// The measured `Group::action_air_patrol` / `Unit::add_air_patrol_order` installation
/// semantics. A compatible `QUEUE_LAST` grows the active waypoint array. Every other case
/// replaces the unit queue because the true-plane installer ignores its `QueuePos` argument
/// and calls `close_orders(0)` unconditionally.
#[allow(clippy::too_many_arguments)]
pub fn install_air_patrol(
    u: &mut UnitWork,
    target_x: i32,
    target_y: i32,
    home_o: i32,
    home_who: i32,
    home_pos: Option<(i32, i32)>,
    group: bool,
    queue: QueuePos,
) -> PatrolInstall {
    if queue == QueuePos::Last {
        let action = update_action(u);
        if action
            .as_ref()
            .is_some_and(|o| o.kind == OrderIndex::AirPatrol)
        {
            if let Some(current) = u.orders.current_mut() {
                if let PatrolPayload::Air(order) = &mut current.patrol_payload {
                    let (x, y) = match home_pos {
                        Some((hx, hy)) => (target_x.wrapping_sub(hx), target_y.wrapping_sub(hy)),
                        None => (target_x, target_y),
                    };
                    order.points.push(x, y);
                    if group {
                        current.flags |= ORDER_GROUP;
                    } else {
                        current.flags &= !ORDER_GROUP;
                    }
                    update_action(u);
                    return PatrolInstall::ExtendedWaypoints;
                }
            }
        }
    }

    let order = OrderRec::air_patrol(
        patrol::new_air_patrol(target_x, target_y, home_o, home_who, home_pos),
        group,
    );
    u.orders.replace(order);
    clear_partial_path(u);
    update_action(u);
    PatrolInstall::Replaced
}

impl From<Order> for OrderRec {
    /// Widen a descriptive [`crate::order::Order`]. The retry state machine starts clean.
    fn from(o: Order) -> OrderRec {
        let follow = o.follow;
        let attack = AttackGroundOrderState {
            att_x: o.x,
            att_y: o.y,
            accuracy: 0,
            attack_unit: 0,
        };
        let targeted_payload = match o.kind {
            OrderIndex::AttackGround => TargetedOrderPayload::AttackGround(attack),
            OrderIndex::AirAttackGround => {
                TargetedOrderPayload::AirAttackGround(AirAttackGroundOrderState {
                    attack,
                    air: crate::systems::air::AirOrderWalk::default(),
                    total_time: 0,
                    sx: 0,
                    sy: 0,
                })
            }
            _ => TargetedOrderPayload::None,
        };
        let move_state = o
            .move_state
            .unwrap_or_else(|| crate::order::MoveOrderState::fresh(o.x, o.y));
        let patrol_payload = o
            .air_patrol
            .as_ref()
            .and_then(|payload| patrol::AirPatrolOrder::try_from(payload).ok())
            .map_or(PatrolPayload::None, PatrolPayload::Air);
        OrderRec {
            kind: o.kind,
            node_metric: o.node_metric,
            flags: o.flags,
            x: o.x,
            y: o.y,
            dest: move_state.dest,
            tolerance: o.tolerance,
            pause: move_state.pause,
            retry: move_state.retry,
            attempts: move_state.attempts,
            timer: move_state.timer,
            facing: move_state.facing,
            dest_x: move_state.dest_x,
            dest_y: move_state.dest_y,
            last_x: move_state.last_x,
            last_y: move_state.last_y,
            coll_x: move_state.coll_x,
            coll_y: move_state.coll_y,
            orig_x: move_state.orig_x,
            orig_y: move_state.orig_y,
            off_x: move_state.off_x,
            off_y: move_state.off_y,
            group_oxx: move_state.group_oxx,
            group_whose: move_state.group_whose,
            group_id: move_state.group_id,
            group_form_id: move_state.group_form_id,
            group_angle: move_state.group_angle,
            in_group: move_state.in_group,
            target_o: follow.map_or(i32::from(o.target_o), |payload| payload.ox),
            target_who: follow.map_or(i32::from(o.target_who), |payload| payload.whom),
            target_uid: follow.map_or(o.target_uid, |payload| payload.uid),
            target_handle: o.target_handle,
            follow,
            special_anim: o.special_anim,
            angle: o.form_order.map_or(move_state.angle, |form| form.angle),
            form_order: o.form_order,
            targeted_payload,
            patrol_payload,
            economy: o.economy,
            ..OrderRec::default()
        }
    }
}

impl From<OrderRec> for Order {
    /// Narrow back to the descriptive form, for anything that speaks
    /// [`crate::order::OrderList`] (e.g. [`crate::world::World::issue`]).
    fn from(r: OrderRec) -> Order {
        let air_patrol = match &r.patrol_payload {
            PatrolPayload::Air(order) => {
                Some(crate::systems::air_runtime_authority::AirPatrolOrderPayload::from(order))
            }
            PatrolPayload::None | PatrolPayload::Group(_) | PatrolPayload::Strafe(_) => None,
        };
        let move_state = matches!(
            r.kind,
            OrderIndex::MoveTo
                | OrderIndex::AttackTo
                | OrderIndex::ExploreTo
                | OrderIndex::FleeTo
                | OrderIndex::ChangeForm
                | OrderIndex::GroupMove
                | OrderIndex::GroupAttackTo
        )
        .then_some(crate::order::MoveOrderState {
            angle: r.angle,
            dest: r.dest,
            pause: r.pause,
            retry: r.retry,
            attempts: r.attempts,
            timer: r.timer,
            facing: r.facing,
            dest_x: r.dest_x,
            dest_y: r.dest_y,
            last_x: r.last_x,
            last_y: r.last_y,
            coll_x: r.coll_x,
            coll_y: r.coll_y,
            orig_x: r.orig_x,
            orig_y: r.orig_y,
            off_x: r.off_x,
            off_y: r.off_y,
            group_oxx: r.group_oxx,
            group_whose: r.group_whose,
            group_id: r.group_id,
            group_form_id: r.group_form_id,
            group_angle: r.group_angle,
            in_group: r.in_group,
        });
        Order {
            node_metric: r.node_metric,
            kind: r.kind,
            flags: r.flags,
            x: r.x,
            y: r.y,
            target_who: r.target_who.clamp(i8::MIN as i32, i8::MAX as i32) as i8,
            target_o: r.target_o.clamp(i16::MIN as i32, i16::MAX as i32) as i16,
            target_uid: r.target_uid,
            target_handle: r.target_handle,
            tolerance: r.tolerance,
            move_state,
            follow: r.follow,
            special_anim: r.special_anim,
            form_order: r.form_order,
            air_patrol,
            economy: r.economy,
        }
    }
}

// ---------------------------------------------------------------------------
// 4. `OrderList` — the queue with retail's cursor semantics
// ---------------------------------------------------------------------------

/// `UnitData::orderlist` at `+200`.
///
/// Front (`head_node->prev`) is the order being executed. `cursor` is `current_node`: the
/// walk position that `reset()` returns to the front and `advance()` steps toward the back.
/// `at_tail()` is retail's `current_node == head_node` loop terminator. [measured]
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct OrderQueue {
    orders: Vec<OrderRec>,
    cursor: usize,
    /// `LinkListBase::ordered` at `UnitData+224`. Carried so a save/checksum reader has it;
    /// nothing in this port branches on it.
    pub ordered: i32,
}

impl OrderQueue {
    pub fn new() -> OrderQueue {
        OrderQueue::default()
    }

    /// `LinkListBase::reset()` — the four-instruction inlined idiom every accessor starts
    /// with. Puts the cursor on the front order.
    #[inline]
    pub fn reset(&mut self) {
        self.cursor = 0;
    }

    /// `LinkListBase::prev()` `0x0046D6E0` — step the cursor one node toward the back.
    /// Returns `false` when there was nowhere to go.
    #[inline]
    pub fn advance(&mut self) -> bool {
        if self.cursor + 1 < self.orders.len() {
            self.cursor += 1;
            true
        } else {
            false
        }
    }

    /// Retail's `current_node == head_node`: the cursor is on the last node.
    #[inline]
    pub fn at_tail(&self) -> bool {
        self.orders.is_empty() || self.cursor + 1 >= self.orders.len()
    }

    /// `current_data` at `UnitData+204`.
    #[inline]
    pub fn current(&self) -> Option<&OrderRec> {
        self.orders.get(self.cursor)
    }

    #[inline]
    pub fn current_mut(&mut self) -> Option<&mut OrderRec> {
        self.orders.get_mut(self.cursor)
    }

    /// The front order without disturbing the cursor.
    #[inline]
    pub fn front(&self) -> Option<&OrderRec> {
        self.orders.first()
    }

    #[inline]
    pub fn front_mut(&mut self) -> Option<&mut OrderRec> {
        self.orders.first_mut()
    }

    /// `LinkListBase::length` at `UnitData+216`.
    #[inline]
    pub fn len(&self) -> usize {
        self.orders.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.orders.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &OrderRec> {
        self.orders.iter()
    }

    /// Heap storage retained by the queue and every dynamic patrol payload it owns.
    pub fn bytes_reserved(&self) -> usize {
        self.orders.capacity() * std::mem::size_of::<OrderRec>()
            + self
                .orders
                .iter()
                .map(|order| order.patrol_payload.bytes_reserved())
                .sum::<usize>()
    }

    /// `Unit::add_*_order` with `QueuePos::BACK` — append behind whatever is queued.
    pub fn push_back(&mut self, o: OrderRec) {
        self.orders.push(o);
    }

    /// `Unit::add_*_order` with `QueuePos::FRONT` — become the current order, pushing the
    /// rest back. This is how an executor inserts a move leg ahead of its own action.
    pub fn push_front(&mut self, o: OrderRec) {
        self.orders.insert(0, o);
        self.cursor = 0;
    }

    /// What an **un-shifted** command does: discard the queue and install one order.
    /// A shift-clicked command uses [`OrderQueue::push_back`] instead.
    pub fn replace(&mut self, o: OrderRec) {
        self.orders.clear();
        self.orders.push(o);
        self.cursor = 0;
    }

    /// `LinkListBase::remove_current` `0x0046D620` — unlink the node under the cursor. The
    /// engine then hands the `UnitOrder*` to `OrdersMemManager` (`0x00730BB0`) for recycling;
    /// we drop it.
    pub fn remove_current(&mut self) -> Option<OrderRec> {
        if self.cursor >= self.orders.len() {
            return None;
        }
        let o = self.orders.remove(self.cursor);
        if self.cursor >= self.orders.len() {
            self.cursor = 0;
        }
        Some(o)
    }

    pub fn clear(&mut self) {
        self.orders.clear();
        self.cursor = 0;
    }

    /// Little-endian image of the queue, order kinds and flags only, so a caller can hash it.
    /// **This is not `Unit::walk_data`'s layout** — `Unit::walk_data` `0x0060CF40` does not
    /// walk the order nodes at all. It exists to make queue divergence visible in tests.
    pub fn debug_image(&self) -> Vec<u8> {
        let mut v = Vec::with_capacity(self.orders.len() * 2);
        for o in &self.orders {
            v.push(o.kind as u8);
            v.push(o.flags);
        }
        v
    }
}

// ---------------------------------------------------------------------------
// 5. The unit state `Unit::work` reads and writes
// ---------------------------------------------------------------------------

/// The slice of `UnitData` that `Unit::work` `0x0060D180` touches, at the offsets in
/// [`offsets`]. Every field carries its retail offset in its doc comment so a live-memory
/// crawl and this port can be diffed field by field.
#[derive(Clone, Debug)]
pub struct UnitWork {
    /// `SubObjectData::who` `+9`.
    pub who: u8,
    /// `SubObjectData::o` `+10`. Also the phase of every periodic check in `Unit::work`.
    pub o: i16,
    /// `SubObjectData::flags` `+8`.
    pub flags: u8,
    /// `ObjectData::visible` `+64`.
    pub visible: u8,
    /// `ObjectData::uid` `+48`. The token a `TargetOrder` stores to detect staleness.
    pub uid: u16,
    /// `ObjectData::inside_down` `+40`. `Unit::do_patrol` may scramble the contained
    /// aircraft named here after scheduling the next patrol leg.
    pub inside_down: i16,
    /// Stands in for `SubObjectData::ptype` `+24` (`ObjectType*`); a pointer cannot be a
    /// value, so the referent's global type id is carried instead.
    pub ptype: i32,

    /// `x_internal` `+16`, `y_internal` `+20` (**unmasked** here; retail stores them XOR
    /// `0x00063637`) plus `UnitData::angle` `+80`, in [`movement::Body`]'s shape.
    pub body: Body,
    /// `UnitData::dest_angle` `+88`.
    pub dest_angle: i32,
    /// `UnitData::tolerance` `+96` — the arrival radius.
    pub tolerance: i32,
    /// `UnitData::orders_x` `+112` / `orders_y` `+116`: where the unit will *end up* once the
    /// queued movement legs are walked. Written only by [`update_action`].
    pub orders_x: i32,
    pub orders_y: i32,
    /// `UnitData::unit_masks` `+104`. See [`masks`].
    pub unit_masks: u32,
    /// `UnitData::unit_masks2` `+108`.
    pub unit_masks2: u32,
    /// `UnitData::form` `+170` (`+0xAA`), written by `Unit::do_form_change`.
    pub form: i8,
    /// `UnitData::group` `+128`; `< 0` means ungrouped.
    pub group: i16,
    /// `UnitData::inside_up` `+130`; `< 0` means not garrisoned.
    pub inside_up: i16,
    /// `UnitData::collide_frame` `+72`.
    pub collide_frame: i32,
    /// `UnitData::mana_burn` `+150`, immediately before `spell_time`. Bombers add the
    /// rules-table bombing cost here after releasing an `AIR_ATTACK_GROUND` shot.
    pub mana_burn: i16,
    /// `UnitData::spell_time` `+152`.
    pub spell_time: i16,
    /// `UnitData::myspeed` `+154`, world units per frame.
    pub myspeed: i16,
    /// `UnitData::recharging` `+174`. Non-zero makes `Unit::work` return **before**
    /// `do_job` unless the head order is a group order [measured, `0x0060D1F9`].
    pub recharging: u8,
    /// `UnitData::idle` `+176`.
    pub idle: u8,
    /// `UnitData::safe` `+178`, decremented once per `Unit::work` while non-zero.
    pub safe: i8,
    /// `UnitData::path` `+184`, the waypoint stack. On the `units` checksum channel.
    pub path: PathStack,
    /// `UnitData::orderlist` `+200`.
    pub orders: OrderQueue,
    /// `UnitData::openlist` `+260` != NULL — a suspended `PathFinder` search is parked in
    /// this unit. `Unit::work` drops it when the current order is not a move.
    pub parked_search: bool,
    /// The five pathfinder containers and scalar continuation frame which retail moves from
    /// the singleton into `UnitData+0x104..+0x148` on suspension. Keeping this per unit lets
    /// any number of units suspend during the same tick without overwriting one another.
    pub parked_pathfinder: Option<Box<PathFinder>>,
    /// The `UnitType` fields the pathfinder reads.
    pub path_unit: PathUnit,
    /// The first squad body whose angle and `GuyData::turn_speed(0)` retail
    /// `Unit::move_step` reads through `UnitData::guys` at `+0xE4`. The compact movement body
    /// carries the same angle; [`do_move`] keeps the two views synchronized.
    pub lead_guy: GuyData,
    /// Static rules and global constants consumed by `GuyData::turn_speed` `0x005DE340`.
    /// `ut.turn_speed` is the runtime binary angle (`45° == 0x20000000`), not the XML degree
    /// integer; shipped `turn_scale` is 256 and `turn_scale2` is 2.
    pub guy_env: GuyEnv,
    /// Actor virtual `los()` at vtable `+0x128`, projected by the live type adapter. It is
    /// carried explicitly because LOS is not a checksum-owned `UnitData` field.
    pub follow_los: i32,
    /// `UnitTypeData::unit_flags +0x2B4 & 0x20`, the same-frame turn-and-translate gate.
    pub type_moves_while_turning: bool,
    /// Result of `Unit::move_step`'s virtual capability branch at `0x005FB288`. The shipped
    /// PDB does not name the predicate, so the state remains explicit rather than guessed.
    pub type_special_wide_turner: bool,
    /// `UnitType[+0x2B8] & 4` — selects `Unit::work`'s snap-to-destination arm.
    pub type_snap_arm: bool,
    /// `UnitType[+0x2B4] & 0x400` — exempts the unit from the `recharging` early-out.
    pub type_ignores_recharge: bool,
    /// `UnitType[+0x2B4] & 0x40000` and `& 0x4000` — the pair that decides whether the tail
    /// of `Unit::work` sets [`masks::WANTS_WORK`].
    pub type_wants_work: bool,
    pub type_blocks_work: bool,
    /// Virtual `SubObjectData::is_animal()` at vtable `+0x30`. Bird overrides take the
    /// short spell-time arm after air physics; ordinary aircraft do not.
    pub type_is_animal: bool,
}

impl UnitWork {
    /// A unit standing at `(x, y)` with an empty queue.
    pub fn at(who: u8, o: i16, x: i32, y: i32) -> UnitWork {
        let lead_guy = GuyData {
            who: who as i8,
            o,
            guy_num: 0,
            ..GuyData::default()
        };
        let guy_env = GuyEnv {
            ut: UnitTypeStats {
                // Citizen's shipped runtime value: 45 degrees in binary-angle form.
                turn_speed: 0x2000_0000,
                squad_size: 1,
                ..UnitTypeStats::default()
            },
            turn_scale: 256,
            turn_scale2: 2,
            ai_speed: 1,
            ..GuyEnv::default()
        };
        UnitWork {
            who,
            o,
            flags: obj_flags::ACTIVE,
            visible: 1,
            uid: 0,
            inside_down: -1,
            ptype: 0,
            body: Body {
                x,
                y,
                angle: 0,
                stuck_budget: 0,
            },
            dest_angle: 0,
            tolerance: movement::UCELL,
            orders_x: x,
            orders_y: y,
            unit_masks: 0,
            unit_masks2: 0,
            form: 0,
            group: -1,
            inside_up: -1,
            collide_frame: i32::MIN / 2,
            mana_burn: 0,
            spell_time: 0,
            myspeed: 24,
            recharging: 0,
            idle: 1,
            safe: 0,
            path: PathStack::new(),
            orders: OrderQueue::new(),
            parked_search: false,
            parked_pathfinder: None,
            path_unit: PathUnit::default(),
            lead_guy,
            guy_env,
            follow_los: 0,
            type_moves_while_turning: false,
            type_special_wide_turner: false,
            type_snap_arm: false,
            type_ignores_recharge: false,
            type_wants_work: false,
            type_blocks_work: false,
            type_is_animal: false,
        }
    }

    /// `UnitData::order_type` `0x00616E80`: reset the cursor, return the front order's
    /// virtual `get_type()`, or `NONE` when the list is empty.
    #[inline]
    pub fn order_type(&mut self) -> OrderIndex {
        self.orders.reset();
        self.orders.current().map_or(OrderIndex::None, |o| o.kind)
    }
}

impl Default for UnitWork {
    fn default() -> Self {
        Self::at(0, 0, 0, 0)
    }
}

/// `Unit::update_order` `0x006179D0` — `reset()` then return `current_data`.
/// Returns `None` when `head_node == NULL`. [measured, 62 bytes, verbatim]
#[inline]
pub fn update_order(u: &mut UnitWork) -> Option<OrderRec> {
    u.orders.reset();
    u.orders.current().cloned()
}

/// `Unit::update_action` `0x0060A870` and its const twin `UnitData::get_action`
/// `0x00608450`. [measured]
///
/// Walks the queue from the front, **skipping** every order that is `is_move() && !is_group()`
/// or whose type is `CHANGE_FORM`, and returns the first survivor — the *action* the unit is
/// walking toward. Along the way it writes the destination of each skipped move order into
/// `orders_x` / `orders_y` / `dest_angle`, so those three fields end up describing where the
/// unit will be standing when it starts the action. That is the whole point of the function
/// and it is why `Unit::work` calls it three times.
///
/// Retail seeds the three fields from the unit's own position and `angle` before walking
/// (`orders_x = x ^ 0x63637`, `orders_y = y ^ 0x63637`, `dest_angle = angle`), so a unit with
/// no queued movement reports its current position. Reproduced.
pub fn update_action(u: &mut UnitWork) -> Option<OrderRec> {
    u.orders_x = u.body.x;
    u.orders_y = u.body.y;
    u.dest_angle = u.body.angle;
    u.orders.reset();
    if u.orders.is_empty() {
        return None;
    }
    loop {
        let cur = u.orders.current().cloned()?;
        let skip = (cur.is_move() && !cur.is_group()) || cur.kind == OrderIndex::ChangeForm;
        if !skip {
            return Some(cur);
        }
        if cur.is_move() {
            u.orders_x = cur.x;
            u.orders_y = cur.y;
            u.dest_angle = cur.angle;
        }
        if u.orders.at_tail() {
            // Retail's loop exits with the cursor on `head_node`; if that last order is a
            // *group* move it is still returned as the action. [measured, `0x0060A9E0`]
            return if cur.is_group() && cur.is_move() {
                Some(cur)
            } else {
                None
            };
        }
        u.orders.advance();
    }
}

/// Why an order left the queue. **Our label on a derived call pattern**, not a retail field —
/// see the module header.
///
/// The label does *not* choose the call sequence: [`kill_current_order`] is always the bare
/// `kill_current_order(0)`, and [`abort_order_sequence`] is the `repath(); kill_current_order(0)`
/// pair. Conflating the two was a real defect in the first draft of this module — it made a
/// path failure eat the follow-on `GUARD` that retail leaves alone, because `repath` had
/// already retired the move by the time the bare kill ran.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KillReason {
    Completed,
    Failed,
}

/// `Unit::repath` `0x005E29B0` (499 B). [measured]
///
/// Despite the name it does not recompute a path: it **strips leading movement orders**. It
/// returns immediately if the front order's type is not in [`MOVE_LIKE`]; otherwise it kills
/// the front order and repeats. A `GROUP_MOVE` front order held by a unit with a valid group
/// first detaches the unit from the group (`0x007123F0`) — modelled here as clearing
/// `group`, because the group container is another lane's.
///
/// Returns how many orders it stripped.
pub fn repath(u: &mut UnitWork) -> u32 {
    let mut stripped = 0;
    loop {
        u.orders.reset();
        let Some(cur) = u.orders.current().cloned() else {
            return stripped;
        };
        if !is_move_like(cur.kind) {
            return stripped;
        }
        if cur.kind == OrderIndex::GroupMove && u.group >= 0 {
            // `Groups::remove_unit`-shaped detach; the next pass takes the kill branch.
            u.group = -1;
            continue;
        }
        kill_current_order_inner(u, KillReason::Completed);
        stripped += 1;
    }
}

/// `Unit::kill_current_order(int)` `0x005E2CB0` (1,312 B). [measured for the structure and
/// for the list surgery; the per-type epilogues are named but not ported.]
///
/// Order of operations, verbatim:
/// 1. `unit_masks &= ~0x20000`;
/// 2. `order_type()`, then the per-type epilogue — arrival bookkeeping for the seven
///    [`MOVE_LIKE`] types, gather-slot release for `GATHER`, `Unit::end_trade_route`
///    `0x005E3BD0` for `TRADE_ROUTE`, a spell refund for `CAST_SPELL`;
/// 3. `reset()`; if the list is non-empty, `remove_current()` then hand the node to
///    `OrdersMemManager` `0x00730BB0` and `reset()` again;
/// 4. `Unit::clear_partial_path` `0x005E3920`;
/// 5. `Unit::update_action` `0x0060A870`.
///
/// Steps 3–5 are ported exactly; step 2 is reduced to the `GATHER` leader-dirty notification
/// and the `TRADE_ROUTE` hook, both of which are boundaries into other lanes.
///
/// This is the **bare** call — `push 0; call 0x5e2cb0`, which is what 100+ of the 120 sites
/// do. For retail's `repath(); kill_current_order(0)` pair use [`abort_order_sequence`].
pub fn kill_current_order(u: &mut UnitWork, reason: KillReason) -> Option<OrderRec> {
    kill_current_order_inner(u, reason)
}

/// Retail's `repath(); kill_current_order(0)` pair — "throw away the walk *and* the thing it
/// was walking to". [measured at `0x0060D948` in `Unit::work`, inside
/// `Unit::check_target_path` at `0x005E2389` and `0x005E2C22`, and at 14 further sites.]
///
/// Returns how many orders left the queue.
pub fn abort_order_sequence(u: &mut UnitWork) -> u32 {
    let stripped = repath(u);
    let killed = kill_current_order_inner(u, KillReason::Failed).is_some() as u32;
    stripped + killed
}

fn kill_current_order_inner(u: &mut UnitWork, _reason: KillReason) -> Option<OrderRec> {
    u.unit_masks &= !masks::ORDER_TRANSIENT;
    let kind = u.order_type();

    // Step 2, the reachable part of the epilogue.
    if kind == OrderIndex::Gather {
        // `leader[who].flags |= 0x2000000` at `0x005E2D9x` — "recompute this player's
        // economy". The leader array is `economy.rs`'s; this raises the intent instead.
        u.flags |= obj_flags::NOTIFY_LEADER;
    }

    // Step 3.
    u.orders.reset();
    let removed = u.orders.remove_current();
    u.orders.reset();

    // Step 4 — `Unit::clear_partial_path` `0x005E3920` drops a parked search and the
    // waypoints belonging to the order that just died.
    clear_partial_path(u);

    // Step 5.
    update_action(u);
    removed
}

/// `Unit::clear_partial_path` `0x005E3920` (674 B). The reachable half: drop the parked
/// search and the waypoint stack. The engine also returns the `PathFinder` trees it had
/// adopted into the unit at `+0x104..+0x114`; [`UnitWork::parked_pathfinder`] owns the same
/// five containers and drops them here.
pub fn clear_partial_path(u: &mut UnitWork) {
    u.parked_search = false;
    u.parked_pathfinder = None;
    u.path.clear();
}

// ---------------------------------------------------------------------------
// 6. The world interface the arms need
// ---------------------------------------------------------------------------

/// What the world knows about an object a `TargetOrder` names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TargetState {
    pub x: i32,
    pub y: i32,
    /// `ObjectData::uid` `+48`. `Unit::work`'s tail compares the order's stored `uid` against
    /// this and treats a mismatch as "the slot was reused" [measured, `0x0060D8xx`].
    pub uid: u16,
    /// `SubObjectData::flags & 1`.
    pub active: bool,
    /// The virtual `SubObjectData::is_seen(who)` at vtable `+0x48`, which
    /// `Unit::check_target_path` gates on.
    pub seen: bool,
}

/// What one [`WorkWorld::attack`] produced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttackOutcome {
    /// A shot resolved; the payload is the damage the host's pipeline returned.
    Fired(i32),
    /// Out of range: keep closing.
    OutOfRange,
    /// Weapon not ready.
    Recharging,
    /// The target cannot be hurt by this attacker at all — retire the order.
    Impossible,
}

/// What one [`WorkWorld::gather`] produced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatherOutcome {
    /// Resources credited this frame.
    Yield(i32),
    /// The node is empty — the order **completed**.
    Exhausted,
    /// Every gather slot on the node is taken. Retail sets `flags |= 0x10` and retires
    /// [measured, `0x005EF63E`].
    SlotsFull,
}

/// Which retail target search `Unit::do_air_patrol` calls first on its mod-16 scan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AirPatrolSearch {
    /// Ordinary fighters: `find_new_air_target`, with the retail game-option fallback to
    /// `find_new_bomber_target` owned by the world callback.
    AirFirst,
    /// `TypeIndex::BOMBER` (304): `find_new_bomber_target`, with the inverse fallback.
    BomberFirst,
}

/// The current action observed on the passenger named by an `AWAIT_BOARD` order.
///
/// Retail first calls `UnitData::get_action`, checks its type, then calls
/// `Unit::update_action` and reads the `BoardOrder` target through vtable slot `+0x64`
/// [measured, `0x005ED0D5..0x005ED10F` and `0x005ED173..0x005ED1B2`]. A world adapter
/// returns both observations together because no retail mutation occurs between them. The
/// target is consulted only when `kind == BOARD_SHIP`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoardingAction {
    pub kind: OrderIndex,
    pub target: crate::systems::naval::TargetRef,
}

/// Why a grouped movement executor could not acquire its external group snapshot.
///
/// `Unit::do_group_move` reads the global `Groups` pool, the formation leader's live
/// object/order/path, terrain-region tables, and target/action state before it changes the
/// actor. None of those facts live in [`UnitWork`]. A host must therefore provide one
/// coherent preflight result; absence is an ordinary fail-closed result, not permission to
/// approximate the formation with a plain move.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupMoveHostError {
    Unavailable,
    InvalidState(&'static str),
}

/// The mutation-ready follower frame produced by the external half of
/// `Unit::do_group_move` `0x005E79A0`.
///
/// Retail builds this path from the leader's current position/path and the member's
/// `GroupData::curr_x/curr_y` entry, then validates the destination against terrain before
/// calling `Unit::move_step`. The path is supplied whole so a host cannot publish a
/// destination without also publishing the stack state from the same snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupMoveFollowerStep {
    pub path: PathStack,
    pub dest_x: i32,
    pub dest_y: i32,
    pub in_group: i32,
    pub speed: i32,
}

/// One coherent, read-only decision for the next `GROUP_MOVE` frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GroupMovePlan {
    /// The actor matches `GroupOrder::{whose,oxx}`. The boolean is the mod-32 close-target
    /// check which calls `Group::kill_group_move(id)` before `Unit::do_move`.
    Leader { kill_group_before_move: bool },
    /// The follower/leader relationship became invalid while the actor is still more than
    /// `0x5ff` Coord away: call `Group::refresh_group_order` and return.
    Refresh,
    /// Convert this group order to its ordinary `MOVE_TO`/`ATTACK_TO` counterpart.
    Ungroup,
    /// Retail's in-range attack action kills only this actor's current group move.
    KillCurrent,
    /// The target gates resolve to `Group::action_attack`.
    ActionAttack,
    /// An authoritative leader/formation/path snapshot reached `Unit::move_step`.
    Step(GroupMoveFollowerStep),
    /// A measured return branch (for example, the large-angle follower under `0x30`
    /// Coord) which keeps the order unchanged for this frame.
    Hold,
}

/// Infallible external effects reached after a successful [`GroupMovePlan`] preflight.
///
/// A host may mutate the actor as part of the real cross-unit/group operation (notably
/// `kill_group_move`, `refresh_group_order`, and `action_attack`). These callbacks cannot
/// reject: all fallible acquisition must happen in `group_move_preflight`, before the core
/// touches `UnitWork`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupMoveEffect {
    KillGroupMove { id: i32 },
    RefreshGroupOrder { id: i32 },
    UpdatePositions,
    ActionAttack,
    DistributeAttack,
}

/// What the post-movement target/order gates require. The call is made only after a
/// successful preflight, so it is deliberately infallible. The combined final variant is
/// the leader's measured `kill_group_move` then `distribute_attack` sequence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupMovePostStep {
    Continue,
    KillCurrent,
    Ungroup,
    ActionAttack,
    KillGroupAndDistribute,
}

/// Why `GROUP_ATTACK_TO` could not acquire the combat capabilities it may need after
/// grouped movement. Capability preflight occurs before `do_group_move`, because retail
/// calls `fight`/`do_attack_to_pause` only afterwards and a late "unavailable" result would
/// leave the formation half-mutated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupAttackToHostError {
    Unavailable,
    InvalidState(&'static str),
}

/// Why `ATTACK_TO` cannot guarantee its later phased target-search/pause transition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttackToHostError {
    Unavailable,
    InvalidState(&'static str),
}

/// The exact post-`do_move` branch of `Unit::do_attack_to` `0x005F2320`.
///
/// The host evaluates these facts against the moved actor: attack capability and virtual
/// `is_supply`, the army leash, or the two Group predicates in `do_attack_to_pause`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttackToPostMove {
    /// The special army leash returned before target selection because distance exceeded six.
    HoldForArmy,
    /// Call `find_melee_target(-1, nullptr, 0, 1, 0)`, including its action transition.
    FindMeleeTarget,
    /// The no-attack/supply branch calls `do_attack_to_pause`; true writes literal 15.
    Pause { set_pause: bool },
}

/// Why `BUILD_AT` cannot acquire one coherent builder/target/site transaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildAtHostError {
    Unavailable,
    InvalidState(&'static str),
}

/// Infallible unit/group effects reached after [`WorkWorld::build_at_preflight`].
///
/// Animation and facing are separate because retail performs both before testing the raw
/// `UNIT_DECOY` bit. `Reswarm` is reached only after the current BUILD_AT node was bare-killed;
/// `FinishTail` is likewise called after retirement so `check_build_order`/auto-gather/
/// `build_done` observe the newly exposed queue head.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildAtEffect {
    SetAnimation { animation: i32 },
    SetAngle { angle: i32 },
    Reswarm { preserve_group_flag: bool },
    FinishTail { reason: BuilderFinish },
}

/// Exact return partition of `Wall::do_construct` as consumed by `Unit::do_build`.
///
/// Placement, start/rejection, harmonic helper progress and `Build::activate(0,1,1)` are
/// owned by the mandatory construction host. Only `Completed` retires the builder order in
/// this call; a rejected site is observed as invalid by the builder on a later activation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildAtConstructResult {
    SiteRejected,
    Progressed {
        credited: u32,
        started_this_call: bool,
    },
    Completed {
        credited: u32,
        started_this_call: bool,
    },
}

/// Why `GROUP_ATTACK` could not acquire its global group/target snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupAttackHostError {
    Unavailable,
    InvalidState(&'static str),
}

/// One coherent external decision for `Unit::do_group_attack` `0x005E75A0`.
///
/// Target liveness/range, leader validity, nearby-target selection, and the mutable Groups
/// pool are deliberately outside [`UnitWork`]. The host resolves those reads before the core
/// writes actor angle state, queue state, or the GroupAttackOrder scratch target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupAttackPlan {
    LeaderFight {
        target_o: i32,
        target_who: i32,
    },
    FollowerFight {
        target_o: i32,
        target_who: i32,
    },
    LeaderKillMoveAndDistribute,
    KillGroupOrder,
    RefreshGroupOrder,
    /// The follower has no admitted target and takes the facing/idle tail.
    FollowerFace,
    /// The exceptional tail rotates this temporary GROUP_ATTACK behind its immediately
    /// queued GROUP_MOVE and calls `move_step(group_move, 1)` once.
    FollowerTailStep,
}

/// Infallible cross-object effects reached after successful [`GroupAttackPlan`] preflight.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupAttackEffect {
    /// The Groups-pool side effect inside `Unit::set_angle(group_angle, ?, 0)`. The Unit angle
    /// and mask toggle precede this callback; the lead Guy angle follows it, as in retail.
    SetAngle {
        angle: i32,
    },
    Fight {
        target_o: i32,
        target_who: i32,
        mandatory: u8,
        temporary: i32,
    },
    KillGroupMove {
        id: i32,
    },
    KillGroupOrder {
        id: i32,
    },
    DistributeAttack {
        target_o: i32,
        target_who: i32,
    },
    RefreshGroupOrder {
        id: i32,
    },
    /// The otherwise-discarded `UnitData::get_speed(1)` call immediately after unlinking the
    /// current GROUP_ATTACK. It is retained because a virtual getter may have host effects.
    ProbeSpeed {
        mode: i32,
    },
    SetIdleAnim,
}

/// Why a recovered coordinate-target executor could not acquire its mandatory host seam.
/// `Unavailable` is a normal fail-closed result; `InvalidState` means the host resolved a
/// slot/type combination which retail could not have used to construct this order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetedOrderHostError {
    Unavailable,
    InvalidState(&'static str),
}

/// Capability receipt acquired before `EXPLORE_TO` is allowed to call `do_move`.
///
/// The identity fields make the receipt single-use for this actor/order snapshot. The
/// post-move pointer and on-map facts are deliberately absent: retail reads both only after
/// movement, through [`WorkWorld::explore_to_post_move`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExploreToHostReceipt {
    pub actor_who: u8,
    pub actor_o: i16,
    pub actor_uid: u16,
    pub frame: i32,
    pub order: OrderRec,
}

/// Infallible post-move reads authorized by [`ExploreToHostReceipt`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExploreToPostMoveReceipt {
    /// Exact concrete-order pointer identity, not merely another `EXPLORE_TO` kind.
    pub current_order_is_same: bool,
    pub actor_is_on_map: bool,
}

/// One coherent read receipt for `Unit::do_attack_ground`.
///
/// [`AttackGroundFacts`] retains [`HostFact`] on branch-conditional reads, so an unused fact
/// may remain missing while a reached missing fact still fails closed. The duplicated actor
/// and order fields are checked before the planner can emit its first effect.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttackGroundHostReceipt {
    pub actor_who: u8,
    pub actor_o: i16,
    pub actor_uid: u16,
    pub order: AttackGroundOrderState,
    pub order_flags: u8,
    pub facts: AttackGroundFacts,
}

/// Capability token acquired before the mutating `do_air_physics` call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AirAttackGroundHostReceipt {
    pub actor_who: u8,
    pub actor_o: i16,
    pub actor_uid: u16,
    pub order: AirAttackGroundOrderState,
    pub order_flags: u8,
}

/// Exact actor/order observations returned by the mandatory air-physics host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AirAttackGroundPhysicsReceipt {
    pub order: AirAttackGroundOrderState,
    pub facts: AirAttackGroundFacts,
}

/// Why REPAIR cannot acquire one coherent target/diplomacy/city/economy snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepairHostError {
    Unavailable,
    InvalidState(&'static str),
}

/// Single-use snapshot and capability token for `Unit::do_repair` `0x005EE420`.
///
/// `snapshot_version` is host-owned and must bind every object, leader, terrain, city and
/// constants read represented by `facts`. The duplicated actor/order identity is checked
/// before the pure planner can emit its first mutation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepairHostReceipt {
    pub snapshot_version: u64,
    pub actor_who: u8,
    pub actor_o: i16,
    pub actor_uid: u16,
    /// UID observed on the target object in this snapshot, not merely copied from the order.
    pub target_uid: u16,
    pub order: OrderRec,
    pub facts: repair_order::RepairFacts,
}

/// Proof returned only after the WorkWorld host atomically applies the complete REPAIR
/// effect slice. A partial count is a broken host contract and is rejected loudly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RepairCommitReceipt {
    pub snapshot_version: u64,
    pub actor_who: u8,
    pub actor_o: i16,
    pub actor_uid: u16,
    pub target: repair_order::ObjectId,
    pub target_uid: u16,
    pub committed_effects: usize,
}

/// Why SPECIAL_ANIM could not acquire or publish one coherent actor/order/world transaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpecialAnimHostError {
    Unavailable,
    InvalidState(&'static str),
}

/// Attestation returned only after the host atomically publishes the complete SPECIAL_ANIM plan.
///
/// The host must revalidate the `SpecialAnimExecutorReceipt::snapshot` immediately before the
/// commit. A failed commit authorizes no actor/order/queue/path/Guy/terrain/external mutation and
/// consumes no RNG. The dispatcher checks the entire preflight image and exact effect count; a
/// partial count is a broken host contract rather than a successful animation frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpecialAnimCommitReceipt {
    pub snapshot: SpecialAnimHostSnapshot,
    pub request: SpecialAnimExecutorRequest,
    pub plan: SpecialAnimExecutorPlan,
    pub committed_steps: usize,
}

impl SpecialAnimCommitReceipt {
    pub fn applied(preflight: &SpecialAnimExecutorReceipt) -> Self {
        Self {
            snapshot: preflight.snapshot,
            request: preflight.request,
            plan: preflight.plan.clone(),
            committed_steps: preflight.plan.steps.len(),
        }
    }

    pub fn validates(&self, preflight: &SpecialAnimExecutorReceipt) -> bool {
        self.snapshot == preflight.snapshot
            && self.request == preflight.request
            && self.plan == preflight.plan
            && self.committed_steps == preflight.plan.steps.len()
    }
}

/// The queries the executors make of the surrounding world.
///
/// Everything the arms cannot derive from `UnitData` alone lives behind this trait, so the
/// module states its boundaries instead of inventing mechanics. It extends
/// [`movement::UnitWorld`] because [`do_move`] hands the same world straight to the
/// pathfinder.
pub trait WorkWorld: UnitWorld {
    /// `Game::frame` at `Game+0x550`, read through `[0x00C061EC]`.
    fn frame(&self) -> i32;

    /// Look up `objects[who][o]`. `None` when the slot is empty.
    fn target(&self, who: i32, o: i32) -> Option<TargetState>;

    /// One attack resolution — `Unit::do_attack`'s call into `ObjectData::get_damage`
    /// `0x00644130` and the damage application around it. See
    /// [`crate::mechanics::damage`].
    fn attack(&mut self, actor: &UnitWork, order: &OrderRec) -> AttackOutcome;

    /// One frame of gathering — the `economy.rs` side of `Unit::do_gather` `0x005EF2A0`.
    fn gather(&mut self, actor: &UnitWork, order: &OrderRec) -> GatherOutcome;

    /// **Service the pathfinder's RNG obligation.** A failed unit search must consume
    /// `Random::get(0, 0xFFFF) % 3 + 6` from `GameAccess::game_random` and store the result
    /// as a retry delay. A host with a real stream must draw here; a host without one must
    /// say so, because skipping it desyncs every later draw in the tick.
    fn draw_path_retry_delay(&mut self) -> i32;

    /// The virtual `Unit::think_bird(UnitOrder*, 0)` call at the head of
    /// `Unit::do_air_patrol`. It is empty for `Unit`, but animal subclasses override it.
    fn patrol_think_bird(&mut self, actor: &mut UnitWork, order: &mut AirPatrolOrder);

    /// `Unit::do_air_physics` `0x005E86D0`. Return its integer result: zero stops the
    /// patrol executor for this frame; non-zero opens waypoint/target processing.
    fn air_patrol_physics(
        &mut self,
        _actor: &mut UnitWork,
        _order: &mut AirPatrolOrder,
        _target_x: i32,
        _target_y: i32,
    ) -> bool;

    /// The mod-16 `find_new_air_target` / `find_new_bomber_target` boundary. The callback
    /// owns both the primary search and retail's game-option-controlled fallback.
    fn air_patrol_unit_target(
        &mut self,
        _actor: &UnitWork,
        _order: &AirPatrolOrder,
        _search_x: i32,
        _search_y: i32,
        _search: AirPatrolSearch,
    ) -> Option<AirPatrolTarget>;

    /// The mod-32 `ObjectsData::find_building_at(..., SearchIndexBH(3), actor.who, 0, 0)`
    /// boundary, including the returned building's actor-specific `ever_seen` bit.
    fn air_patrol_building_target(
        &mut self,
        _actor: &UnitWork,
        _order: &AirPatrolOrder,
        _waypoint_x: i32,
        _waypoint_y: i32,
    ) -> Option<AirPatrolTarget>;

    /// `Group::action_move_to(x,y,QUEUE_FIRST,0,0,ATTACK_TO,0,-1,-1,0)` from the grouped
    /// arm of `Unit::do_patrol`.
    fn group_patrol_move(&mut self, actor: &mut UnitWork, request: GroupMoveRequest);

    /// Virtual `ObjectData::is(TypeIndex, strict)`, used for BOMBER/FIGHTERBOMBER patrol
    /// search selection. Upgrade-line membership belongs to the type system, not this arm.
    fn patrol_actor_is_type(&self, actor: &UnitWork, type_id: i32, strict: bool) -> bool;

    /// The trailing contained-aircraft predicate and singleton `Group::action_scramble`.
    fn patrol_inside_is_scramblable(&self, actor_who: u8, inside_o: i16) -> bool;
    fn patrol_scramble_inside(&mut self, actor: &mut UnitWork, inside_o: i16);

    /// Acquire every external fact needed by `Unit::do_group_move` as one snapshot. The
    /// default is deliberately unavailable: a generic movement world does not secretly
    /// become a formation world.
    fn group_move_preflight(
        &mut self,
        _actor: &UnitWork,
        _order: &OrderRec,
    ) -> Result<GroupMovePlan, GroupMoveHostError> {
        Err(GroupMoveHostError::Unavailable)
    }

    /// Apply a cross-unit/group effect after successful preflight. Hosts that ever return
    /// `Ok` from [`WorkWorld::group_move_preflight`] must implement this callback.
    fn group_move_effect(
        &mut self,
        _actor: &mut UnitWork,
        _order: &OrderRec,
        _effect: GroupMoveEffect,
    ) {
        panic!("WorkWorld::group_move_effect is required after GROUP_MOVE preflight")
    }

    /// Resolve the target/order gates after a raw-zero leader `do_move` or follower
    /// `move_step`. Hosts that can reach either path must implement this callback. It may
    /// consult the newly moved actor but may not fail or roll back the already integrated
    /// step.
    fn group_move_post_step(
        &mut self,
        _actor: &UnitWork,
        _order: &OrderRec,
        _result: ArmResult,
    ) -> GroupMovePostStep {
        panic!("WorkWorld::group_move_post_step is required for GROUP_MOVE follower steps")
    }

    /// Prove that the post-movement `fight`/`do_attack_to_pause` callbacks are available. This
    /// must be read-only. The default keeps grouped attack-move orders intact and unmoved.
    fn group_attack_to_preflight(
        &mut self,
        _actor: &UnitWork,
        _order: &OrderRec,
    ) -> Result<(), GroupAttackToHostError> {
        Err(GroupAttackToHostError::Unavailable)
    }

    /// The periodic predicate `UnitType::attack != 0 && actor vfunc(+0xCC) == 0`, evaluated
    /// after `do_group_move` exactly where retail evaluates it.
    fn group_attack_to_calls_fight(&mut self, _actor: &UnitWork, _order: &OrderRec) -> bool {
        panic!("WorkWorld::group_attack_to_calls_fight requires successful preflight")
    }

    /// `Unit::fight(-1, 0, 0, 1, 0)` at `0x005E758C`. All five literal arguments are fixed
    /// by this call site; the host owns target selection and its cross-object mutations.
    fn group_attack_to_fight(&mut self, _actor: &mut UnitWork, _order: &OrderRec) -> ArmResult {
        panic!("WorkWorld::group_attack_to_fight requires successful preflight")
    }

    /// The two external Group/location predicates inside `Unit::do_attack_to_pause(order)`
    /// `0x005F22A0` (113 bytes). On true the local executor writes literal `15` to
    /// `MoveOrder::pause`. This is not the full `Unit::do_attack_to` executor at
    /// `0x005F2320`; PDB symbol/address agreement makes that distinction authoritative.
    fn group_attack_to_pause_gate(&mut self, _actor: &UnitWork, _order: &OrderRec) -> bool {
        panic!("WorkWorld::group_attack_to_pause_gate requires successful preflight")
    }

    /// Prove that all future phased `ATTACK_TO` target/army/pause facts and effects are
    /// available. This runs before movement even on a non-phase frame: otherwise a missing
    /// host on the next phase would silently skip retail's search and continue moving.
    fn attack_to_preflight(
        &mut self,
        _actor: &UnitWork,
        _order: &OrderRec,
    ) -> Result<(), AttackToHostError> {
        Err(AttackToHostError::Unavailable)
    }

    /// Resolve the post-movement branch on `(frame + actor.o) % 15 == 0`. It cannot reject;
    /// [`WorkWorld::attack_to_preflight`] already proved every lookup is available.
    fn attack_to_post_move(&mut self, _actor: &UnitWork, _order: &OrderRec) -> AttackToPostMove {
        panic!("WorkWorld::attack_to_post_move requires successful preflight")
    }

    /// Apply `find_melee_target(-1, nullptr, 0, 1, 0)`. The callback owns exact selection
    /// and any ATTACK/action node it installs, and is infallible after preflight.
    fn attack_to_find_melee_target(&mut self, _actor: &mut UnitWork, _order: &OrderRec) {
        panic!("WorkWorld::attack_to_find_melee_target requires successful preflight")
    }

    /// Acquire the single-use capability for `EXPLORE_TO` before movement mutates the actor.
    /// A successful receipt guarantees that the post-move query and every emitted host effect
    /// are available for this snapshot.
    fn explore_to_preflight(
        &mut self,
        _actor: &UnitWork,
        _order: &OrderRec,
    ) -> Result<ExploreToHostReceipt, TargetedOrderHostError> {
        Err(TargetedOrderHostError::Unavailable)
    }

    /// Perform the two reads retail makes after `do_move`. This cannot reject after a
    /// successful preflight, because movement cannot be rolled back at this boundary.
    fn explore_to_post_move(
        &mut self,
        _actor: &UnitWork,
        _order_before_move: &OrderRec,
        _receipt: &ExploreToHostReceipt,
    ) -> ExploreToPostMoveReceipt {
        panic!("WorkWorld::explore_to_post_move requires a successful preflight receipt")
    }

    /// Acquire every branch-conditional read and every effect capability reachable from the
    /// current `ATTACK_GROUND` snapshot. Missing reached facts fail before mutation.
    fn attack_ground_preflight(
        &mut self,
        _actor: &UnitWork,
        _order: &OrderRec,
    ) -> Result<AttackGroundHostReceipt, TargetedOrderHostError> {
        Err(TargetedOrderHostError::Unavailable)
    }

    /// Acquire the capability token which makes the mutating air-physics call and every
    /// deterministic tail effect infallible for this `AIR_ATTACK_GROUND` snapshot.
    fn air_attack_ground_preflight(
        &mut self,
        _actor: &UnitWork,
        _order: &OrderRec,
    ) -> Result<AirAttackGroundHostReceipt, TargetedOrderHostError> {
        Err(TargetedOrderHostError::Unavailable)
    }

    /// `Unit::do_air_physics(order, att_x, att_y)`. The callback updates the supplied walked
    /// order state and actor, then returns the exact post-physics facts consumed by the local
    /// planner. It cannot reject after [`WorkWorld::air_attack_ground_preflight`].
    fn air_attack_ground_physics(
        &mut self,
        _actor: &mut UnitWork,
        _order: &mut AirAttackGroundOrderState,
        _receipt: &AirAttackGroundHostReceipt,
    ) -> AirAttackGroundPhysicsReceipt {
        panic!("WorkWorld::air_attack_ground_physics requires a successful preflight receipt")
    }

    /// Apply one non-local effect emitted by the targeted-order planners. Successful typed
    /// preflight receipts guarantee this callback for every reachable effect. Local queue,
    /// flag, recharge, and mana stores remain owned by the dispatcher and are never sent here.
    fn targeted_order_effect(
        &mut self,
        _actor: &mut UnitWork,
        _order: &OrderRec,
        _effect: OrderEffect,
    ) {
        panic!("WorkWorld::targeted_order_effect requires a successful targeted-order receipt")
    }

    /// Acquire the coherent object/containment/speed/LOS/search snapshot and every
    /// non-local capability needed by `Unit::do_follow` before the dispatcher mutates the
    /// order or actor. `request.actor.speed/los` are host observations; all other actor and
    /// order fields are checked against live dispatcher state before the receipt is accepted.
    fn follow_preflight(
        &mut self,
        _actor: &UnitWork,
        request: FollowExecutorRequest,
    ) -> FollowExecutorReceipt {
        FollowExecutorReceipt::unavailable(request)
    }

    /// Apply the sole non-local FOLLOW effect, `Unit::set_anim`. An applied preflight
    /// receipt guarantees this callback cannot fail. Queue, fallback and same-tick movement
    /// effects remain local to the dispatcher.
    fn follow_set_anim(&mut self, _actor: &mut UnitWork, _anim: i32, _mode: i32, _choose: i32) {
        panic!("WorkWorld::follow_set_anim requires an applied FOLLOW receipt")
    }

    /// Acquire every conditional object/type/terrain/Guy fact, the canonical RNG observations,
    /// and the capability to publish every step reachable from this SPECIAL_ANIM snapshot.
    /// The default leaves the actor byte-for-byte unchanged.
    fn special_anim_preflight(
        &mut self,
        _actor: &UnitWork,
        _order: &OrderRec,
    ) -> Result<SpecialAnimExecutorReceipt, SpecialAnimHostError> {
        Err(SpecialAnimHostError::Unavailable)
    }

    /// Revalidate the preflight snapshot and atomically publish the complete ordered plan.
    ///
    /// This callback owns local-looking writes too: payload `frames/started`, queue retirement,
    /// actor angle/location, the same-tick recursive work call, and all external containment,
    /// death, terrain, Guy, and RNG effects. An error must publish none of them.
    fn special_anim_commit(
        &mut self,
        _actor: &mut UnitWork,
        _order: &OrderRec,
        _preflight: &SpecialAnimExecutorReceipt,
    ) -> Result<SpecialAnimCommitReceipt, SpecialAnimHostError> {
        panic!("WorkWorld::special_anim_commit requires a successful SPECIAL_ANIM preflight")
    }

    /// Acquire the object/type/terrain/trig/search facts, the canonical RNG observation, and
    /// the capability to publish every step reachable from this GUARD snapshot.
    ///
    /// The returned receipt is bound to a
    /// [`crate::systems::guard_dispatch::GuardHostSnapshot`]; the dispatcher recomputes both
    /// the snapshot's observable half and the plan itself before publishing anything.
    fn guard_preflight(
        &mut self,
        _actor: &UnitWork,
        _order: &OrderRec,
    ) -> Result<crate::systems::guard_dispatch::GuardDispatchReceipt, GuardHostError> {
        Err(GuardHostError::Unavailable)
    }

    /// Apply one non-local GUARD effect in the planner's order. Queue insertion, the inserted
    /// leg's `pause`, the concrete payload writes and the same-tick `Unit::do_move` stay with
    /// the dispatcher and never reach this callback. A validated receipt guarantees it.
    fn guard_effect(
        &mut self,
        _actor: &mut UnitWork,
        _order: &OrderRec,
        _step: crate::systems::guard_order::GuardHostStep,
    ) {
        panic!("WorkWorld::guard_effect requires a validated GUARD receipt")
    }

    /// Acquire every admission, diplomacy, capacity, city, terrain and containment fact this
    /// GARRISON snapshot can reach, plus the capability to publish the resulting steps.
    fn garrison_preflight(
        &mut self,
        _actor: &UnitWork,
        _order: &OrderRec,
    ) -> Result<crate::systems::garrison_order::GarrisonExecutorReceipt, GarrisonHostError> {
        Err(GarrisonHostError::Unavailable)
    }

    /// Apply one non-local GARRISON effect. Only the bare `kill_current_order(0)` is local;
    /// containment, search, feedback and the chain retirement all arrive here.
    fn garrison_effect(
        &mut self,
        _actor: &mut UnitWork,
        _order: &OrderRec,
        _step: crate::systems::garrison_order::GarrisonHostStep,
    ) {
        panic!("WorkWorld::garrison_effect requires a validated GARRISON receipt")
    }

    /// Acquire all branch facts and the capability to commit every effect which REPAIR can
    /// reach. This callback is read-only with respect to simulation state. The default makes
    /// a generic movement host fail closed with no animation, queue, target, or economy write.
    fn repair_preflight(
        &mut self,
        _actor: &UnitWork,
        _order: &OrderRec,
    ) -> Result<RepairHostReceipt, RepairHostError> {
        Err(RepairHostError::Unavailable)
    }

    /// Atomically apply the complete ordered effect slice from [`repair_order::plan_repair`].
    ///
    /// This one callback owns local-looking effects too: animation, bare order retirement,
    /// find-repair/gather/swarm queue changes, helper byte, both resource stores, target
    /// `repair_damage`, leader stamp, and local UI/sound. It must validate the same
    /// `snapshot_version` immediately before committing and may not partially succeed.
    fn repair_commit(
        &mut self,
        _actor: &mut UnitWork,
        _order: &OrderRec,
        _effects: &[repair_order::RepairEffect],
        _receipt: &RepairHostReceipt,
    ) -> RepairCommitReceipt {
        panic!("WorkWorld::repair_commit requires a successful REPAIR preflight receipt")
    }

    /// Acquire every external fact and capability which `Unit::do_build` may reach from this
    /// node. The returned local fields are checked against the actor/order before any effect.
    /// A successful host must also have infallible animation, facing, reswarm, placement,
    /// lifecycle and finish-tail callbacks ready for the branch selected by these facts.
    fn build_at_preflight(
        &mut self,
        _actor: &UnitWork,
        _order: &OrderRec,
    ) -> Result<BuildAtPreflightInput, BuildAtHostError> {
        Err(BuildAtHostError::Unavailable)
    }

    /// Apply one ordered Unit/Guys/Groups effect after successful BUILD_AT preflight.
    fn build_at_effect(
        &mut self,
        _actor: &mut UnitWork,
        _order: &OrderRec,
        _effect: BuildAtEffect,
    ) {
        panic!("WorkWorld::build_at_effect requires successful BUILD_AT preflight")
    }

    /// Execute the exact `Wall::do_construct(owner)` transaction reached by a ready builder.
    /// This includes mandatory blocked-site, start/disband, progress, and activation hosts,
    /// but not the caller's post-return BUILD_AT retirement/tail, which remains local here.
    fn build_at_construct(
        &mut self,
        _actor: &mut UnitWork,
        _order: &OrderRec,
    ) -> BuildAtConstructResult {
        panic!("WorkWorld::build_at_construct requires successful BUILD_AT preflight")
    }

    /// Acquire all cross-object facts used by `Unit::do_group_attack`. The default is
    /// unavailable and must leave the actor byte-for-byte unchanged.
    fn group_attack_preflight(
        &mut self,
        _actor: &UnitWork,
        _order: &OrderRec,
    ) -> Result<GroupAttackPlan, GroupAttackHostError> {
        Err(GroupAttackHostError::Unavailable)
    }

    /// Apply a Groups/object/action effect after successful preflight. It cannot reject;
    /// every fallible lookup belongs in [`WorkWorld::group_attack_preflight`].
    fn group_attack_effect(
        &mut self,
        _actor: &mut UnitWork,
        _order: &OrderRec,
        _effect: GroupAttackEffect,
    ) {
        panic!("WorkWorld::group_attack_effect requires successful preflight")
    }

    /// `Unit::set_anim(a, b, c)` at the head of both boarding executors. The shipped arms
    /// always pass `(0, 0, 1)` before any rendezvous or target probe. Animation state is not
    /// represented by [`UnitWork`], so a boarding-capable host must apply it here.
    fn boarding_set_anim(&mut self, _actor: &mut UnitWork, _a: i32, _b: i32, _c: i32) {
        panic!("WorkWorld::boarding_set_anim is required for boarding orders")
    }

    /// `Unit::check_meet_ship(target_o, target_who)` from `Unit::do_board` `0x005ED1F0`.
    /// The callback owns every rendezvous-side mutation performed by that function and
    /// returns its exact integer truth value (`true` means the rendezvous remains pending).
    fn board_check_meet_ship(
        &mut self,
        _actor: &mut UnitWork,
        _target: crate::systems::naval::TargetRef,
    ) -> bool {
        panic!("WorkWorld::board_check_meet_ship is required for BOARD_SHIP")
    }

    /// `ObjectData::can_carry(passenger_o, passenger_who)`. `carrier` is the target ship in
    /// `BOARD_SHIP` and the executing ship in `AWAIT_BOARD`; `passenger` is the inverse.
    /// For `BOARD_SHIP`, retail calls this only *after* retiring the passenger's order.
    fn boarding_can_carry(
        &mut self,
        _actor: &UnitWork,
        _carrier: crate::systems::naval::TargetRef,
        _passenger: crate::systems::naval::TargetRef,
    ) -> bool {
        panic!("WorkWorld::boarding_can_carry is required for boarding orders")
    }

    /// `Unit::go_inside(target_o, target_who, mode)` on the boarding passenger. The sole
    /// call in `Unit::do_board` passes mode zero after the order has already been retired.
    /// The callback must apply containment to both the passenger and carrier world state.
    fn board_go_inside(
        &mut self,
        _actor: &mut UnitWork,
        _carrier: crate::systems::naval::TargetRef,
        _mode: i32,
    ) {
        panic!("WorkWorld::board_go_inside is required for BOARD_SHIP")
    }

    /// The pair of virtual target probes at `0x005ED0AA/0x005ED0BC` and
    /// `0x005ED15A/0x005ED165`. This must return true only when the object slot resolves and
    /// both retail probes accept it as a live unit.
    fn boarding_target_is_live_unit(&mut self, _target: crate::systems::naval::TargetRef) -> bool {
        panic!("WorkWorld::boarding_target_is_live_unit is required for AWAIT_BOARD")
    }

    /// The passenger's current action observation described by [`BoardingAction`]. `None`
    /// is retail's null `get_action()` result. The callback is invoked only after the live
    /// unit probes succeed, and may be invoked twice on the same tick because retail really
    /// performs two validation passes.
    fn boarding_target_action(
        &mut self,
        _target: crate::systems::naval::TargetRef,
    ) -> Option<BoardingAction> {
        panic!("WorkWorld::boarding_target_action is required for AWAIT_BOARD")
    }

    /// Apply `passenger->repath(); passenger->kill_current_order(0)` in that order. Retail
    /// performs this cross-unit cancellation before retiring the executing ship's own
    /// `AWAIT_BOARD` order (`0x005ED1C4..0x005ED1E2`).
    fn boarding_abort_passenger(
        &mut self,
        _actor: &UnitWork,
        _passenger: crate::systems::naval::TargetRef,
    ) {
        panic!("WorkWorld::boarding_abort_passenger is required for AWAIT_BOARD")
    }

    /// Atomically execute the complete `CHANGE_FORM` or `THINK` terminal-order plan.
    ///
    /// `Applied` attests that every ordered effect was preflighted and committed, including
    /// the nested `set_angle`, `kill_current_order`, `do_idle`, and `think_peasant`
    /// lifecycles. `Unavailable` must leave both the actor and external world unchanged.
    /// The default is fail-closed so existing movement-only worlds cannot silently acquire
    /// either executor.
    fn apply_terminal_order_transaction(
        &mut self,
        _actor: &mut UnitWork,
        request: TerminalOrderRequest,
    ) -> TerminalOrderReceipt {
        TerminalOrderReceipt::unavailable(request)
    }

    /// The side-effecting `detect_unit_collision` -> `resolve_unit_collision` bridge used by
    /// `Unit::move_step`. The default preserves the older boolean occupancy host, but a
    /// fidelity host must apply blocker/order state on `Detect(MoveStep)` and consume it on
    /// the following `Resolve` event. The mutable receiver is intentional: retail mutates the
    /// collision bitmap cache, unit rows, order, RNG stream, and repath budgets here.
    fn move_collision(
        &mut self,
        _actor: &mut UnitWork,
        _pf: &mut PathFinder,
        event: movement::MoveCollisionEvent<'_>,
    ) -> movement::MoveCollisionReply {
        match event {
            movement::MoveCollisionEvent::Detect { x, y, .. } => {
                if self.unit_collides(x, y) {
                    movement::MoveCollisionReply::Hit
                } else {
                    movement::MoveCollisionReply::Clear
                }
            }
            movement::MoveCollisionEvent::Resolve { .. } => movement::MoveCollisionReply::Unhandled,
        }
    }
}

// ---------------------------------------------------------------------------
// 7. Per-arm implementation status
// ---------------------------------------------------------------------------

/// How faithfully **this dispatcher** implements each `do_job` arm.
///
/// [`crate::order::EXECUTORS`] owns the jump-table addresses and symbols; this owns only the
/// status, because the two answer different questions and drifted apart once already. A test
/// asserts the two tables stay index-aligned.
pub const ARMS: [ArmStatus; NUM_UNIT_ORDERS] = [
    ArmStatus::Implemented,     //  0 NONE            virtual do_idle
    ArmStatus::Implemented,     //  1 MOVE_TO         Unit::do_move 0x005F7B30
    ArmStatus::Implemented,     //  2 ATTACK_TO       Unit::do_attack_to 0x005F2320
    ArmStatus::Implemented,     //  3 EXPLORE_TO      Unit::do_explore_to 0x005F24A0
    ArmStatus::Implemented,     //  4 FLEE_TO         Unit::do_move -- SAME ARM as MOVE_TO
    ArmStatus::FaithfullyEmpty, //  5 PATROL          no case label; falls to the default
    ArmStatus::Implemented,     //  6 BUILD_AT        Unit::do_build 0x005EEBF0
    ArmStatus::Implemented,     //  7 GATHER          Unit::do_gather 0x005EF2A0
    ArmStatus::Implemented,     //  8 BOARD_SHIP      Unit::do_board 0x005ED1F0
    ArmStatus::Implemented,     //  9 AWAIT_BOARD     Unit::do_await_board 0x005ED040
    ArmStatus::Implemented,     // 10 ATTACK          Unit::do_attack 0x005F1B80
    ArmStatus::Implemented,     // 11 FOLLOW          Unit::do_follow 0x005E65D0
    ArmStatus::Implemented,     // 12 GUARD           Unit::do_guard 0x005E5C70
    ArmStatus::Implemented,     // 13 REPAIR          Unit::do_repair 0x005EE420
    ArmStatus::Unimplemented,   // 14 CAST_SPELL      Unit::do_cast 0x005EBFE0
    ArmStatus::Unimplemented,   // 15 TRADE_ROUTE     Unit::do_trade 0x005ED270
    ArmStatus::Unimplemented,   // 16 STRAFE          Unit::do_strafe 0x005EAB00
    ArmStatus::Implemented,     // 17 AIR_PATROL      Unit::do_air_patrol 0x005EA620
    ArmStatus::Implemented,     // 18 CHANGE_FORM     Unit::do_form_change 0x005E8670
    ArmStatus::Implemented,     // 19 GROUP_MOVE      Unit::do_group_move 0x005E79A0
    ArmStatus::Implemented,     // 20 GROUP_ATTACK    Unit::do_group_attack 0x005E75A0
    ArmStatus::Implemented,     // 21 GROUP_ATTACK_TO Unit::do_group_attack_to 0x005E74E0
    ArmStatus::Implemented,     // 22 GROUP_PATROL    Unit::do_patrol 0x005F1910
    ArmStatus::Implemented,     // 23 ATTACK_GROUND   Unit::do_attack_ground 0x005F1410
    ArmStatus::Implemented,     // 24 AIR_ATK_GROUND  Unit::do_air_attack_ground 0x005EA420
    // Production Sim reaches UNIT and object-free EXIT through the atomic host. Closure stays
    // red until target relation, ENTER/Airbase-EXIT object, Guy, terrain, containment/death, and
    // RNG tails land.
    ArmStatus::Unimplemented, // 25 SPECIAL_ANIM    Unit::do_spec_anim 0x005E5880
    ArmStatus::Implemented,   // 26 GARRISON        Unit::do_garrison 0x005E6B80
    ArmStatus::Implemented,   // 27 THINK           Unit::do_think_order 0x005E5BF0
];

/// Per-arm dispatch counts, so coverage is measured rather than estimated.
///
/// Distinct from [`crate::order::OrderCoverage`], which reads its status out of
/// [`crate::order::EXECUTORS`]. This one reads [`ARMS`], so it reports *this* dispatcher.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DispatchCoverage {
    pub dispatches: [u64; NUM_UNIT_ORDERS],
    pub total: u64,
    pub unimplemented: u64,
    /// `Unit::work` entries.
    pub work_calls: u64,
    /// `Unit::work` entries that returned before reaching `do_job`.
    pub early_outs: u64,
    /// Orders retired with a bare `kill_current_order(0)`.
    pub completed: u64,
    /// Orders retired through the `repath(); kill_current_order(0)` pair.
    pub failed: u64,
    /// Frames on which the pathfinder owed `game_random` a draw.
    pub path_retry_draws: u64,
    /// `Unit::detect_boat_collision` gates that opened and found no ported body.
    pub boat_collision_skipped: u64,
}

impl DispatchCoverage {
    #[inline]
    pub fn record(&mut self, k: OrderIndex) -> ArmStatus {
        let i = k.index();
        self.dispatches[i] += 1;
        self.total += 1;
        let st = ARMS[i];
        if st == ArmStatus::Unimplemented {
            self.unimplemented += 1;
        }
        st
    }

    pub fn covered_fraction(&self) -> f64 {
        if self.total == 0 {
            return 0.0;
        }
        (self.total - self.unimplemented) as f64 / self.total as f64
    }

    pub fn merge(&mut self, other: &DispatchCoverage) {
        for i in 0..NUM_UNIT_ORDERS {
            self.dispatches[i] += other.dispatches[i];
        }
        self.total += other.total;
        self.unimplemented += other.unimplemented;
        self.work_calls += other.work_calls;
        self.early_outs += other.early_outs;
        self.completed += other.completed;
        self.failed += other.failed;
        self.path_retry_draws += other.path_retry_draws;
        self.boat_collision_skipped += other.boat_collision_skipped;
    }
}

// ---------------------------------------------------------------------------
// 8. `Unit::find_path` — the pathfinder call `do_move` makes
// ---------------------------------------------------------------------------

/// The result of the fine UCoord path leg exposed by this module.
///
/// This is intentionally not labelled with `Unit::find_path`'s raw integers: the full retail
/// composition uses `0` for its normal synchronous completion, `1` for several early/direct
/// exits, and `2` for suspension. The Rust enum describes the narrower leg below.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathOutcome {
    /// Waypoints are on the stack (possibly just one, for a trivial hop).
    Found,
    /// No path. When [`PathFinder::pending_retry_draw`] is set, the caller owes
    /// `game_random` a retry draw; the pre-A* off-map/stalled exits consume none.
    Failed,
    /// The soft node budget ran out; the search would be parked in the unit.
    Suspended,
}

/// The fine UCoord leg that full retail `Unit::find_path(Coord, Coord)` `0x005FB910` may call.
/// It runs `PathFinder::find_upath` `0x00682F30` (straight-line probe and three-record search
/// frame), `PathFinder::astar_path` `0x00683770` when needed, then waypoint compression.
///
/// The 840-instruction `Unit::find_path` wrapper itself is not yet ported. It adjusts obstructed
/// endpoints, recursively composes route fragments, and crosses WCoord/TCoord domains through
/// `find_wpath`/`find_tpath`; `Unit::do_move`'s second-chance route rebuild depends on that full
/// composition. This function does not claim those behaviours.
///
/// The initial wrapper `PathFinder::find_upath` `0x00688EB0` installs a soft budget of
/// `500 / player_path_scale²` (halved for quick mode). `find_upath_restore` `0x00688F40`
/// installs `300 / player_path_scale²` and continues the parked open/closed trees. Both
/// budgets and the resume are reproduced here; the scale comes from the same per-player
/// `GameAccess::ai_speed` value already carried in [`GuyEnv::ai_speed`]. [measured]
pub fn find_path<W: UnitWorld>(
    pf: &mut PathFinder,
    w: &W,
    u: &mut UnitWork,
    dest_x: i32,
    dest_y: i32,
    quick: i32,
) -> PathOutcome {
    pf.pending_retry_draw = false;
    let unit = u.path_unit;
    let scale_sq = u.guy_env.ai_speed.wrapping_mul(u.guy_env.ai_speed).max(1);
    let args = SearchArgs { unit: &unit, quick };

    let out = if u.parked_search {
        let parked = u
            .parked_pathfinder
            .take()
            .expect("UnitData::openlist requires parked PathFinder containers");
        *pf = *parked;
        assert!(
            pf.suspended,
            "parked PathFinder must carry a continuation frame"
        );
        pf.soft_node_limit = 300 / scale_sq;
        pf.in_upath = 1;
        let searched = pf.astar_path_unit(w, &mut u.path, &args);
        pf.in_upath = 0;
        match searched {
            SearchResult::Found => {
                pf.compress_path(&mut u.path);
                PathOutcome::Found
            }
            SearchResult::Failed => PathOutcome::Failed,
            SearchResult::Suspended => PathOutcome::Suspended,
        }
    } else {
        pf.kill_lists();
        // `find_upath` expects the unit's current position on the stack as one record.
        u.path.clear();
        u.path.push(PathData {
            to_x: u.body.x,
            to_y: u.body.y,
            tolerance: u.tolerance,
            flags: PathData::FLAG_MORE,
        });
        match pf.find_upath_prepare(w, &mut u.path, &unit, dest_x, dest_y) {
            UPathOutcome::OffMap | UPathOutcome::Stalled => PathOutcome::Failed,
            UPathOutcome::Trivial => PathOutcome::Found,
            UPathOutcome::NeedsSearch => {
                pf.soft_node_limit = (500 / scale_sq) / if quick != 0 { 2 } else { 1 };
                pf.in_upath = 1;
                let searched = pf.astar_path_unit(w, &mut u.path, &args);
                pf.in_upath = 0;
                match searched {
                    SearchResult::Found => {
                        pf.compress_path(&mut u.path);
                        PathOutcome::Found
                    }
                    SearchResult::Failed => PathOutcome::Failed,
                    SearchResult::Suspended => PathOutcome::Suspended,
                }
            }
        }
    };
    u.parked_search = out == PathOutcome::Suspended;
    if u.parked_search {
        // `astar_path` stores the five containers on this unit, then replaces the singleton's
        // containers with fresh empty ones (`0x006845E5..0x006847F3`). Move the whole owned
        // Rust search for the same effect.
        u.parked_pathfinder = Some(Box::new(std::mem::replace(pf, PathFinder::new())));
    } else {
        u.parked_pathfinder = None;
    }

    // Reconcile the anchor with `Unit::do_move`'s arrival test.
    //
    // After a successful search the stack is `[anchor, w_n .. w_1]` and `emit_path` gives
    // every waypoint `FLAG_WAYPOINT` with **bit 0 clear**, while `Unit::do_move` treats
    // popping a record that carries `FLAG_MORE` (bit 0) as "this order is finished"
    // [measured, `0x005F8827: test byte ptr [eax+0xc], 1`]. Only the caller's own record
    // carries that bit, so the anchor is the record the walk *ends* on and it has to be the
    // order's destination — otherwise the unit walks the waypoints and then walks back to
    // where it started, which is exactly the oscillation this reconciliation was written to
    // stop.
    //
    // `PathFinder::find_upath_prepare` takes its probe *start* from the same record, so one
    // record cannot be both. It is written as the start before the call and rewritten as the
    // destination after. **This is a reconciliation, not a citation:** whether retail's
    // `Unit::find_path` `0x005FB910` pushes the start or the destination is not settled
    // here; what is settled is that `do_move`'s pop-test only terminates if the bottom record
    // is the destination.
    if out == PathOutcome::Found {
        if let Some(anchor) = u.path.records.first_mut() {
            anchor.to_x = dest_x;
            anchor.to_y = dest_y;
            anchor.tolerance = u.tolerance;
            anchor.flags = PathData::FLAG_MORE;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 9. The arms
// ---------------------------------------------------------------------------

/// What one `do_job` arm did. Reported rather than returned as an `int`, because retail's
/// `int` returns mean different things per arm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArmResult {
    /// The arm ran and the order is still current.
    Working,
    /// The unit translated this frame.
    Moved,
    /// The unit turned but did not translate.
    Turned,
    /// A body was in the way.
    Blocked,
    /// The order was retired.
    Retired(KillReason),
    /// The arm dispatched but has no ported body.
    NotPorted,
    /// The arm is ported, but its required cross-object host snapshot was unavailable.
    /// No local mutation has occurred when this is returned.
    HostUnavailable,
    /// The arm ran and deliberately did nothing (retail's default case).
    Empty,
    /// A shot resolved; payload is the damage.
    Fired(i32),
    /// Resources credited; payload is the amount.
    Gathered(i32),
    /// The arm could not run because the order was gone.
    NoOrder,
    /// The order kind and concrete payload disagree. Retail cannot construct this state;
    /// it is reported explicitly for malformed recovered/save input.
    MalformedOrder,
}

#[inline]
fn boarding_ref(u: &UnitWork) -> crate::systems::naval::TargetRef {
    crate::systems::naval::TargetRef {
        ox: u.o as i32,
        whom: u.who as i32,
        uid: u.uid,
    }
}

#[inline]
fn boarding_order_target(order: &OrderRec) -> crate::systems::naval::TargetRef {
    crate::systems::naval::TargetRef {
        ox: order.target_o,
        whom: order.target_who,
        uid: order.target_uid,
    }
}

#[inline]
fn same_boarding_slot(
    a: crate::systems::naval::TargetRef,
    b: crate::systems::naval::TargetRef,
) -> bool {
    // Both shipped executors compare only TargetOrder::ox/whom. Unit::work owns the
    // independent uid staleness pass before dispatch.
    a.ox == b.ox && a.whom == b.whom
}

#[inline]
fn retire_boarding_actor(u: &mut UnitWork, cov: &mut DispatchCoverage) -> ArmResult {
    // Every local retirement in both shipped executors is the bare
    // `kill_current_order(0)` call, never the repath/failure pair.
    kill_current_order(u, KillReason::Completed);
    cov.completed += 1;
    ArmResult::Retired(KillReason::Completed)
}

/// `Unit::do_board(BoardOrder*)` `0x005ED1F0` (114 bytes), arm 8.
///
/// Raw instruction order [measured against `riseofnations.exe`]:
///
/// 1. read the `BoardOrder` target and call `set_anim(0, 0, 1)`;
/// 2. call `check_meet_ship(target_o, target_who)` and hold the order if non-zero;
/// 3. retire the passenger's current order;
/// 4. ask the target ship `can_carry(passenger_o, passenger_who)`;
/// 5. on success call `passenger->go_inside(target_o, target_who, 0)`.
///
/// The capacity probe deliberately occurs after queue retirement. That surprising order is
/// asserted by the integration tests and encoded by [`crate::systems::naval::board_order_transaction`].
pub fn do_board<W: WorkWorld>(
    u: &mut UnitWork,
    w: &mut W,
    cov: &mut DispatchCoverage,
) -> ArmResult {
    let Some(order) = update_order(u) else {
        return ArmResult::NoOrder;
    };
    let target = boarding_order_target(&order);
    let passenger = boarding_ref(u);

    // The transaction carries the measured literal animation tuple. Its rendezvous arm has
    // no retirement or containment effect.
    let pending_tx = crate::systems::naval::board_order_transaction(target, true, false);
    let (a, b, c) = pending_tx.set_anim;
    w.boarding_set_anim(u, a, b, c);
    let meet_ship_pending = w.board_check_meet_ship(u, target);
    if meet_ship_pending {
        debug_assert_eq!(
            pending_tx.step,
            crate::systems::naval::BoardStep::Rendezvous
        );
        return ArmResult::Working;
    }

    // Retail kills first, then resolves the carrier and probes capacity. Keeping these as
    // separate host calls makes the intermediate queue state observable and reproducible.
    let result = retire_boarding_actor(u, cov);
    let target_can_carry = w.boarding_can_carry(&*u, target, passenger);
    let completed_tx =
        crate::systems::naval::board_order_transaction(target, false, target_can_carry);
    debug_assert_eq!(completed_tx.kill_current_order, Some(0));
    if let Some((carrier, mode)) = completed_tx.go_inside {
        w.board_go_inside(u, carrier, mode);
    }
    result
}

/// `Unit::do_await_board(AwaitBoardOrder*)` `0x005ED040` (432 bytes), arm 9.
///
/// This is the instruction-verified two-pass handshake, including its asymmetric
/// cancellation behaviour:
///
/// * a same-owner passenger that is a live unit, is carriable, and still has a matching
///   `BOARD_SHIP` action keeps both orders alive;
/// * a same-owner passenger boarding a *different* ship only retires this ship's await
///   order — its passenger order is not touched;
/// * every other path performs a second live-unit/action probe. A reverse link found there
///   triggers `passenger->repath(); passenger->kill_current_order(0)` before this ship's
///   own bare retirement.
///
/// The two probes are not deduplicated: retail repeats them, and a mutation-sensitive host
/// must see the same callback sequence.
pub fn do_await_board<W: WorkWorld>(
    u: &mut UnitWork,
    w: &mut W,
    cov: &mut DispatchCoverage,
) -> ArmResult {
    let Some(order) = update_order(u) else {
        return ArmResult::NoOrder;
    };
    let passenger = boarding_order_target(&order);
    let ship = boarding_ref(u);

    w.boarding_set_anim(u, 0, 0, 1);

    // `0x005ED069..0x005ED077`: the self-referential order goes straight to the ship kill.
    if same_boarding_slot(passenger, ship) {
        return retire_boarding_actor(u, cov);
    }

    // First pass, entered only for the same owner. A fully valid reverse link is the sole
    // holding return at 0x005ED123 -> 0x005ED1E7.
    if passenger.whom == ship.whom
        && w.boarding_target_is_live_unit(passenger)
        && w.boarding_can_carry(&*u, ship, passenger)
    {
        if let Some(action) = w.boarding_target_action(passenger) {
            if action.kind == OrderIndex::BoardShip {
                // Once a valid BOARD_SHIP action was observed on this fast path, either
                // target-field mismatch kills only the ship's await order. It does not fall
                // through to the cross-unit cancellation pass.
                if action.target.ox != ship.ox {
                    return retire_boarding_actor(u, cov);
                }
                if action.target.whom == ship.whom {
                    return ArmResult::Working;
                }
                return retire_boarding_actor(u, cov);
            }
        }
    }

    // Fallback pass. `repath(); kill_current_order(0)` is applied to the passenger only when
    // its current BOARD_SHIP action still points back to this ship. The executing ship is
    // retired afterwards on every path.
    if w.boarding_target_is_live_unit(passenger) {
        if let Some(action) = w.boarding_target_action(passenger) {
            if action.kind == OrderIndex::BoardShip && same_boarding_slot(action.target, ship) {
                w.boarding_abort_passenger(&*u, passenger);
            }
        }
    }
    retire_boarding_actor(u, cov)
}

/// `Unit::do_move(MoveOrder*)` `0x005F7B30` (4,582 B), arms 1 (`MOVE_TO`) and 4 (`FLEE_TO`).
///
/// The parts reproduced, in retail's order [measured unless marked]:
///
/// 1. **the timer.** `if (0 < order->timer)`; `timer == 1` retires the order and `timer > 1`
///    decrements it and holds the frame.
/// 2. **arrival.** `vector_dist(dx, dy) <= UnitData::tolerance` at `0x005F87xx`. On arrival:
///    `order->dest = 0`, pop the top `PathData`; if it does **not** carry `FLAG_MORE` there
///    are more waypoints and the order stays; if it does, clear `ORDER_PATHED` and
///    `kill_current_order(0)` — the order **completed**.
/// 3. **pathing.** With movement-leg bit 8 clear, run
///    [`find_path`]. On failure: retire the move, then if the new head order is `ATTACK` or
///    `BUILD_AT` retire that too (`0x005F8B5F`, the cascade in the module header), and
///    service the pathfinder's RNG obligation.
/// 4. **collision pause.** A non-zero `MoveOrder::pause` is decremented and holds the frame.
/// 5. **integration.** [`movement::move_step_profile_with_collision`] `0x005FAF30`, including
///    the side-effecting detect / waypoint-probe / resolve sequence supplied by [`WorkWorld`].
///
/// Not reproduced here: transport legs, the formation offset (`MoveOrder::off_x` / `off_y`),
/// and the coarse-route second-chance block at `0x005F8BD4`. A host may reproduce
/// `Unit::resolve_unit_collision` through [`WorkWorld::move_collision`]; the default boolean
/// host explicitly cannot.
fn apply_move_path_outcome<W: WorkWorld>(
    outcome: PathOutcome,
    u: &mut UnitWork,
    w: &mut W,
    pf: &mut PathFinder,
    cov: &mut DispatchCoverage,
) -> Option<ArmResult> {
    match outcome {
        PathOutcome::Found => {
            if let Some(c) = u.orders.front_mut() {
                c.dest = 1;
                c.flags |= ORDER_PATHED;
            }
            // `do_move` raises bit 8 when the path top differs from the current position
            // (`0x005F8AFD`). `move_step` clears it on an invalid tile, which re-enters this
            // path arm next frame even though the old stack still contains records.
            u.unit_masks |= masks::PATH_EXHAUSTED;
            None
        }
        PathOutcome::Suspended => Some(ArmResult::Working),
        PathOutcome::Failed => {
            if pf.pending_retry_draw {
                let delay = w.draw_path_retry_delay();
                if let Some(c) = u.orders.front_mut() {
                    c.retry = delay;
                    c.last_x = -1;
                    c.last_y = -1;
                }
                // Both A* failure epilogues add 0x1E to `UnitData::safe` after installing
                // the 6..8 frame retry [measured `0x006848ED`, `0x00684E29`]. It is a byte
                // add, so preserve retail wrapping rather than saturating.
                u.safe = u.safe.wrapping_add(0x1e);
                pf.pending_retry_draw = false;
                cov.path_retry_draws += 1;
            }
            u.unit_masks |= masks::PATH_EXHAUSTED;
            // Both kills here are bare `kill_current_order(0)` calls. A follow-on GUARD
            // survives; only ATTACK and BUILD_AT take the measured cascade.
            kill_current_order(u, KillReason::Failed);
            cov.failed += 1;
            let next = u.order_type();
            if next == OrderIndex::Attack || next == OrderIndex::BuildAt {
                kill_current_order(u, KillReason::Failed);
                cov.failed += 1;
            }
            Some(ArmResult::Retired(KillReason::Failed))
        }
    }
}

/// The shared `Unit::move_step` integration tail used by ordinary and grouped movement.
/// The boolean is retail `move_step`'s raw integer truth value: grouped movement runs its
/// target/order epilogue only on the zero returns. `MoveStep::Blocked` and
/// `MoveStep::InvalidTerrain` are the two recovered zero-return shapes; the other exits
/// return one.
fn integrate_move_step<W: WorkWorld>(
    u: &mut UnitWork,
    w: &mut W,
    pf: &mut PathFinder,
    target: (i32, i32),
    speed: i32,
) -> (ArmResult, bool) {
    let speed = speed.max(1);
    let mut turn_env = u.guy_env;
    turn_env.unit_speed = speed;
    turn_env.unit_mask_turn_scale2 =
        (u.unit_masks & crate::systems::groups_guys::UNIT_MASK_TURN_SCALE2) != 0;
    let turn_rate = u.lead_guy.turn_speed(&turn_env, 0) as i32;
    let mut turn_profile = MoveTurnProfile {
        unit_flags: if u.type_moves_while_turning {
            movement::UNIT_FLAG_MOVE_WHILE_TURNING
        } else {
            0
        },
        type_turn_speed: u.guy_env.ut.turn_speed as u32,
        domain: u.guy_env.ut.domain,
        special_wide_turner: u.type_special_wide_turner,
        speed_half_latch: (u.unit_masks & masks::HALF_SPEED_ON_TURN) != 0,
    };
    u.idle = 0;
    let mut body = u.body;
    let mut path = std::mem::take(&mut u.path);
    let step = movement::move_step_profile_with_collision(
        w,
        &mut body,
        &mut path,
        target,
        speed,
        turn_rate,
        &mut turn_profile,
        |world, event| world.move_collision(u, pf, event),
    );
    u.body = body;
    u.lead_guy.angle = body.angle;
    if turn_profile.speed_half_latch {
        u.unit_masks |= masks::HALF_SPEED_ON_TURN;
    } else {
        u.unit_masks &= !masks::HALF_SPEED_ON_TURN;
    }
    u.path = path;
    match step {
        MoveStep::TurnedOnly => (ArmResult::Turned, true),
        MoveStep::Moved => (ArmResult::Moved, true),
        MoveStep::Blocked => (ArmResult::Blocked, false),
        MoveStep::Arrived => (ArmResult::Working, true),
        MoveStep::InvalidTerrain => {
            // `0x005FB76E`: this is one of `move_step`'s zero returns and clears bit 8.
            u.unit_masks &= !masks::PATH_EXHAUSTED;
            (ArmResult::Blocked, false)
        }
        // The four out-of-bounds exits return one and retain bit 8.
        MoveStep::Refused => (ArmResult::Blocked, true),
    }
}

fn do_move_raw<W: WorkWorld>(
    u: &mut UnitWork,
    w: &mut W,
    pf: &mut PathFinder,
    cov: &mut DispatchCoverage,
) -> (ArmResult, bool) {
    let Some(ord) = update_order(u) else {
        return (ArmResult::NoOrder, false);
    };

    // `UnitData::openlist` is serviced at `0x005F7BDD`, before MoveOrder::timer and the rest
    // of `do_move`. `find_upath_restore` continues the same trees with its smaller per-frame
    // budget; it does not restart from the unit's new position. [measured]
    if u.parked_search {
        let outcome = find_path(pf, w, u, ord.x, ord.y, 0);
        if let Some(done) = apply_move_path_outcome(outcome, u, w, pf, cov) {
            let raw_nonzero = matches!(done, ArmResult::Working);
            return (done, raw_nonzero);
        }
    }

    // 1. the timer.
    if ord.timer > 0 {
        if ord.timer == 1 {
            kill_current_order(u, KillReason::Completed);
            cov.completed += 1;
            return (ArmResult::Retired(KillReason::Completed), false);
        }
        if let Some(c) = u.orders.front_mut() {
            c.timer -= 1;
        }
        return (ArmResult::Working, true);
    }

    // `MoveOrder::retry` at +0x1C is checked before any new route work. While non-zero the
    // unit does nothing; reaching zero adds three to `attempts` (+0x20). The following arm
    // decrements a non-zero `attempts` once per call. [measured `0x005F82C1..0x005F83BB`]
    if ord.retry != 0 {
        if let Some(c) = u.orders.front_mut() {
            c.retry = c.retry.wrapping_sub(1);
            if c.retry == 0 {
                c.attempts = c.attempts.wrapping_add(3);
            }
        }
        return (ArmResult::Working, true);
    }
    if ord.attempts != 0 {
        if let Some(c) = u.orders.front_mut() {
            c.attempts = c.attempts.wrapping_sub(1);
        }
    }

    // `MoveOrder::x/y` (+4/+8) remain the ultimate order destination. `dest_x/dest_y`
    // (+0x2c/+0x30) are the current path/collision step and may be overwritten by
    // `resolve_unit_collision::set_order_detour`. Retail's order-arrival test starts from
    // `piVar4[1]/[2]`, so completing a detour must never complete the whole move.
    let dest = (ord.x, ord.y);
    let dx = dest.0 - u.body.x;
    let dy = dest.1 - u.body.y;

    // 2. arrival.
    if vector_dist(dx, dy) <= u.tolerance {
        if let Some(c) = u.orders.front_mut() {
            c.dest = 0;
        }
        let popped = u.path.pop();
        let more = popped.is_none_or(|r| (r.flags & PathData::FLAG_MORE) != 0);
        if !more {
            // Waypoint reached but the path continues.
            return (ArmResult::Working, true);
        }
        if let Some(c) = u.orders.front_mut() {
            c.flags &= !ORDER_PATHED;
        }
        u.unit_masks &= !masks::ARRIVED_FACING;
        kill_current_order(u, KillReason::Completed);
        cov.completed += 1;
        u.idle = 1;
        return (ArmResult::Retired(KillReason::Completed), false);
    }

    // 3. pathing.
    //
    // Only search when the destination is more than one unit cell away. `find_upath`'s own
    // trivial arm is `|dqx - qx| + |dqy - qy| < 2` [measured `0x00683250`], so a search
    // inside one cell can only return the unit's own position — and a `do_move` that
    // re-searched every frame from inside the goal cell would never converge on a
    // `tolerance` finer than 48. The final leg is therefore a direct walk to the order's
    // destination, which is also what retail's straight-line probe produces.
    let far = vector_dist(dx, dy) > movement::UCELL;
    if far && (u.unit_masks & masks::PATH_EXHAUSTED) == 0 {
        let outcome = find_path(pf, w, u, dest.0, dest.1, 0);
        if let Some(done) = apply_move_path_outcome(outcome, u, w, pf, cov) {
            let raw_nonzero = matches!(done, ArmResult::Working);
            return (done, raw_nonzero);
        }
    }

    // `MoveOrder+0x18` at `0x005F8C88`: every non-zero pause is decremented and holds the
    // unit for this frame. The extra retail branches only choose animation/idle side effects;
    // all of them return before `move_step`. Collision resolution writes this field.
    if ord.pause != 0 {
        if let Some(c) = u.orders.front_mut() {
            c.pause = c.pause.wrapping_sub(1);
        }
        return (ArmResult::Working, true);
    }

    // 5. integration.
    let target = u
        .path
        .peek()
        .map(|r| (r.to_x, r.to_y))
        .unwrap_or((dest.0, dest.1));
    let speed = u.myspeed.max(1) as i32;
    integrate_move_step(u, w, pf, target, speed)
}

pub fn do_move<W: WorkWorld>(
    u: &mut UnitWork,
    w: &mut W,
    pf: &mut PathFinder,
    cov: &mut DispatchCoverage,
) -> ArmResult {
    do_move_raw(u, w, pf, cov).0
}

#[inline]
fn same_group_order(u: &UnitWork, original: &OrderRec) -> bool {
    u.orders.front().is_some_and(|current| {
        current.kind == original.kind && current.group_id == original.group_id
    })
}

/// `Unit::ungroup_move_order(id, 0)` `0x005FD140`, reduced to the executing captain.
///
/// Retail walks non-captain/subordinate objects as well; those objects are not represented
/// by one [`UnitWork`] and remain part of the group host's cross-object transaction. For the
/// executing node the conversion is exact: `GROUP_MOVE -> MOVE_TO`,
/// `GROUP_ATTACK_TO -> ATTACK_TO`, the leader alone retains flag bit zero, `dest` is cleared,
/// `orig_{x,y}` take the ultimate destination, and a follower drops its current path.
fn ungroup_current_move(u: &mut UnitWork, group_id: i32) -> bool {
    let Some(current) = u.orders.front().cloned() else {
        return false;
    };
    if current.group_id != group_id {
        return false;
    }
    let kind = match current.kind {
        OrderIndex::GroupMove => OrderIndex::MoveTo,
        OrderIndex::GroupAttackTo => OrderIndex::AttackTo,
        _ => return false,
    };
    let is_leader = current.group_oxx == i32::from(u.o) && current.group_whose == i32::from(u.who);
    let mut ordinary = current;
    ordinary.kind = kind;
    if !is_leader {
        ordinary.flags &= !ORDER_PATHED;
    }
    ordinary.dest = 0;
    ordinary.orig_x = ordinary.x;
    ordinary.orig_y = ordinary.y;
    ordinary.group_oxx = -1;
    ordinary.group_whose = -1;
    ordinary.group_id = -1;
    ordinary.group_form_id = 0;
    ordinary.group_angle = 0;
    ordinary.in_group = 0;
    *u.orders.front_mut().expect("front was cloned above") = ordinary;
    if !is_leader {
        clear_partial_path(u);
    }
    update_action(u);
    true
}

fn apply_group_move_post<W: WorkWorld>(
    u: &mut UnitWork,
    w: &mut W,
    cov: &mut DispatchCoverage,
    order: &OrderRec,
    result: ArmResult,
    post: GroupMovePostStep,
) -> ArmResult {
    match post {
        GroupMovePostStep::Continue => result,
        GroupMovePostStep::KillCurrent => {
            kill_current_order(u, KillReason::Completed);
            cov.completed += 1;
            ArmResult::Retired(KillReason::Completed)
        }
        GroupMovePostStep::Ungroup => {
            if ungroup_current_move(u, order.group_id) {
                ArmResult::Working
            } else {
                ArmResult::MalformedOrder
            }
        }
        GroupMovePostStep::ActionAttack => {
            w.group_move_effect(u, order, GroupMoveEffect::ActionAttack);
            ArmResult::Working
        }
        GroupMovePostStep::KillGroupAndDistribute => {
            w.group_move_effect(
                u,
                order,
                GroupMoveEffect::KillGroupMove { id: order.group_id },
            );
            w.group_move_effect(u, order, GroupMoveEffect::DistributeAttack);
            ArmResult::Working
        }
    }
}

/// `Unit::do_group_move(GroupMoveOrder*)` `0x005E79A0` (3,275 bytes), arm 19.
///
/// The unit-local instruction order is ported here; the global `Groups`/object/terrain half
/// is an explicit transaction on [`WorkWorld`]:
///
/// 1. an ungrouped actor converts the current node through `ungroup_move_order`;
/// 2. every grouped actor acquires one coherent external snapshot before local mutation;
/// 3. the leader performs the close-target group kill or delegates to [`do_move`], then
///    publishes `Group::update_positions` only while the same order remains current;
/// 4. a follower applies the snapshot's formation path/order fields, resets tolerance, and
///    calls the same recovered `move_step` integration as ordinary movement;
/// 5. only `move_step`'s raw zero exits run the post-step target/order gate.
///
/// Missing group facts return [`ArmResult::HostUnavailable`] with byte-for-byte unchanged
/// actor state. Callbacks after successful preflight are infallible by contract, preventing
/// a half-applied local/group transaction.
pub fn do_group_move<W: WorkWorld>(
    u: &mut UnitWork,
    w: &mut W,
    pf: &mut PathFinder,
    cov: &mut DispatchCoverage,
) -> ArmResult {
    let Some(order) = update_order(u) else {
        return ArmResult::NoOrder;
    };
    if !matches!(
        order.kind,
        OrderIndex::GroupMove | OrderIndex::GroupAttackTo
    ) {
        return ArmResult::MalformedOrder;
    }

    // `0x005E79C1`: this arm is the sole branch which needs no external group lookup.
    if u.group < 0 {
        return if ungroup_current_move(u, order.group_id) {
            ArmResult::Working
        } else {
            ArmResult::MalformedOrder
        };
    }

    // Mandatory and mutation-free. A callback may validate the group slot/id, leader order,
    // member index, terrain regions, destination, speed, and target gates from one lockstep
    // snapshot; it may not publish effects yet.
    let plan = match w.group_move_preflight(&*u, &order) {
        Ok(plan) => plan,
        Err(GroupMoveHostError::Unavailable) => return ArmResult::HostUnavailable,
        Err(GroupMoveHostError::InvalidState(_)) => return ArmResult::MalformedOrder,
    };

    let is_leader = order.group_oxx == i32::from(u.o) && order.group_whose == i32::from(u.who);
    match plan {
        GroupMovePlan::Leader {
            kill_group_before_move,
        } => {
            if !is_leader {
                return ArmResult::MalformedOrder;
            }
            if kill_group_before_move {
                w.group_move_effect(
                    u,
                    &order,
                    GroupMoveEffect::KillGroupMove { id: order.group_id },
                );
                return ArmResult::Working;
            }
            let (result, raw_nonzero) = do_move_raw(u, w, pf, cov);
            if same_group_order(u, &order) {
                if raw_nonzero {
                    w.group_move_effect(u, &order, GroupMoveEffect::UpdatePositions);
                } else {
                    let post = w.group_move_post_step(&*u, &order, result);
                    return apply_group_move_post(u, w, cov, &order, result, post);
                }
            }
            result
        }
        GroupMovePlan::Refresh => {
            if is_leader {
                return ArmResult::MalformedOrder;
            }
            w.group_move_effect(
                u,
                &order,
                GroupMoveEffect::RefreshGroupOrder { id: order.group_id },
            );
            ArmResult::Working
        }
        GroupMovePlan::Ungroup => {
            if ungroup_current_move(u, order.group_id) {
                ArmResult::Working
            } else {
                ArmResult::MalformedOrder
            }
        }
        GroupMovePlan::KillCurrent => {
            kill_current_order(u, KillReason::Completed);
            cov.completed += 1;
            ArmResult::Retired(KillReason::Completed)
        }
        GroupMovePlan::ActionAttack => {
            w.group_move_effect(u, &order, GroupMoveEffect::ActionAttack);
            ArmResult::Working
        }
        GroupMovePlan::Hold => ArmResult::Working,
        GroupMovePlan::Step(step) => {
            if is_leader || step.speed <= 0 {
                return ArmResult::MalformedOrder;
            }
            if !same_group_order(u, &order) {
                return ArmResult::MalformedOrder;
            }
            // All potentially failing validation happened above. Publish the recovered
            // follower mutations together immediately before move_step.
            u.path = step.path;
            u.tolerance = 0;
            let current = u
                .orders
                .front_mut()
                .expect("same_group_order verified a live front node");
            current.dest = 1;
            current.dest_x = step.dest_x;
            current.dest_y = step.dest_y;
            current.in_group = step.in_group;

            let (result, raw_nonzero) =
                integrate_move_step(u, w, pf, (step.dest_x, step.dest_y), step.speed);
            if raw_nonzero || !same_group_order(u, &order) {
                return result;
            }
            let post = w.group_move_post_step(&*u, &order, result);
            apply_group_move_post(u, w, cov, &order, result, post)
        }
    }
}

/// `Unit::do_attack_to(MoveOrder*)` `0x005F2320` (340 bytes), arm 2.
///
/// Retail first executes `do_move`. If the same node survives and
/// `(Game::frame + actor.o) % 15 == 0`, it either:
///
/// - returns behind the `unit_masks & 0x40000` army-distance leash;
/// - calls `find_melee_target(-1, nullptr, 0, 1, 0)` for an attacking non-supply unit; or
/// - calls `do_attack_to_pause`, whose two Group predicates may write `MoveOrder::pause=15`.
///
/// The spatial/type/Groups facts are mandatory host state. Capability is preflighted before
/// `do_move`, including on non-phase frames, so an unavailable host never leaves a partially
/// advanced attack-move that silently skipped its next target-selection phase.
pub fn do_attack_to<W: WorkWorld>(
    u: &mut UnitWork,
    w: &mut W,
    pf: &mut PathFinder,
    cov: &mut DispatchCoverage,
) -> ArmResult {
    let Some(order) = u.orders.front().cloned() else {
        return ArmResult::NoOrder;
    };
    if order.kind != OrderIndex::AttackTo {
        return ArmResult::MalformedOrder;
    }
    match w.attack_to_preflight(&*u, &order) {
        Ok(()) => {}
        Err(AttackToHostError::Unavailable) => return ArmResult::HostUnavailable,
        Err(AttackToHostError::InvalidState(_)) => return ArmResult::MalformedOrder,
    }

    let original_queue_len = u.orders.len();
    let move_result = do_move(u, w, pf, cov);
    if u.orders.len() != original_queue_len
        || !u
            .orders
            .front()
            .is_some_and(|current| current.kind == OrderIndex::AttackTo)
        || !phase_due(w.frame(), u.o, 15)
    {
        return move_result;
    }
    let current = u
        .orders
        .front()
        .cloned()
        .expect("the same ATTACK_TO node survived do_move");
    match w.attack_to_post_move(&*u, &current) {
        AttackToPostMove::HoldForArmy => ArmResult::Working,
        AttackToPostMove::FindMeleeTarget => {
            w.attack_to_find_melee_target(u, &current);
            ArmResult::Working
        }
        AttackToPostMove::Pause { set_pause } => {
            if set_pause {
                u.orders
                    .front_mut()
                    .expect("the same ATTACK_TO node survived post-move planning")
                    .pause = 15;
            }
            ArmResult::Working
        }
    }
}

#[inline]
fn explore_receipt_matches(
    actor: &UnitWork,
    order: &OrderRec,
    frame: i32,
    receipt: &ExploreToHostReceipt,
) -> bool {
    receipt.actor_who == actor.who
        && receipt.actor_o == actor.o
        && receipt.actor_uid == actor.uid
        && receipt.frame == frame
        && &receipt.order == order
}

#[inline]
fn attack_ground_receipt_matches(
    actor: &UnitWork,
    order: &OrderRec,
    state: AttackGroundOrderState,
    receipt: &AttackGroundHostReceipt,
) -> bool {
    let facts = &receipt.facts;
    let facing_matches = match &facts.order_facing_target {
        HostFact::Known(value) => *value == order.has(targeted_order_plans::ORDER_FACING_TARGET),
        HostFact::Missing(_) => true,
    };
    receipt.actor_who == actor.who
        && receipt.actor_o == actor.o
        && receipt.actor_uid == actor.uid
        && receipt.order == state
        && receipt.order_flags == order.flags
        && state.att_x == order.x
        && state.att_y == order.y
        && facts.actor_owner == i32::from(actor.who)
        && facts.actor_object == i32::from(actor.o)
        && facts.actor_angle == actor.body.angle as u32
        && facts.target_x == order.x
        && facts.target_y == order.y
        && facts.attack_unit == state.attack_unit
        && facts.recharge == actor.recharging
        && facing_matches
}

#[inline]
fn air_attack_ground_receipt_matches(
    actor: &UnitWork,
    order: &OrderRec,
    state: AirAttackGroundOrderState,
    receipt: &AirAttackGroundHostReceipt,
) -> bool {
    receipt.actor_who == actor.who
        && receipt.actor_o == actor.o
        && receipt.actor_uid == actor.uid
        && receipt.order == state
        && receipt.order_flags == order.flags
}

#[inline]
fn repair_receipt_matches(
    actor: &UnitWork,
    order: &OrderRec,
    frame: i32,
    receipt: &RepairHostReceipt,
) -> bool {
    let facts = &receipt.facts;
    receipt.actor_who == actor.who
        && receipt.actor_o == actor.o
        && receipt.actor_uid == actor.uid
        && receipt.target_uid == order.target_uid
        && &receipt.order == order
        && facts.repairer
            == (repair_order::ObjectId {
                o: i32::from(actor.o),
                who: i32::from(actor.who),
            })
        && facts.target
            == (repair_order::ObjectId {
                o: order.target_o,
                who: order.target_who,
            })
        && facts.more_work == (order.flags & ORDER_GROUP != 0)
        && facts.repairer_unit_masks == actor.unit_masks
        && facts.frame == frame
}

#[inline]
fn repair_commit_matches(
    actor_identity: (u8, i16, u16),
    receipt: &RepairHostReceipt,
    commit: &RepairCommitReceipt,
    effects: &[repair_order::RepairEffect],
) -> bool {
    commit.snapshot_version == receipt.snapshot_version
        && (commit.actor_who, commit.actor_o, commit.actor_uid) == actor_identity
        && commit.target == receipt.facts.target
        && commit.target_uid == receipt.target_uid
        && commit.committed_effects == effects.len()
}

/// Apply one ordered tail effect emitted by a coordinate-target planner.
///
/// Returns true when a bare `kill_current_order(0)` retired the executor's node. Every
/// non-local effect goes through the host capability proven by the arm's typed receipt.
fn apply_targeted_effect<W: WorkWorld>(
    actor: &mut UnitWork,
    world: &mut W,
    order_before: &OrderRec,
    effect: OrderEffect,
    cov: &mut DispatchCoverage,
) -> bool {
    match effect {
        OrderEffect::DoMove => {
            panic!("DoMove is executed by do_explore_to before its post-move plan")
        }
        OrderEffect::KillCurrentOrder(reason) => {
            debug_assert_eq!(reason, 0);
            kill_current_order(actor, KillReason::Completed);
            cov.completed += 1;
            true
        }
        OrderEffect::StoreAttackUnit(value) => {
            let current = actor
                .orders
                .front_mut()
                .expect("ATTACK_GROUND receipt guarantees a current order");
            let state = match &mut current.targeted_payload {
                TargetedOrderPayload::AttackGround(state) => state,
                _ => panic!("ATTACK_GROUND host effect lost its concrete current order"),
            };
            state.attack_unit = value;
            false
        }
        OrderEffect::UpdateAction => {
            update_action(actor);
            false
        }
        OrderEffect::OrObjectFlags(mask) => {
            actor.unit_masks |= mask;
            false
        }
        OrderEffect::OrOrderFlags(mask) => {
            actor
                .orders
                .front_mut()
                .expect("targeted-order receipt guarantees a current order")
                .flags |= mask;
            false
        }
        OrderEffect::StoreRecharge(value) => {
            actor.recharging = value;
            false
        }
        OrderEffect::AddManaBurn(value) => {
            actor.mana_burn = actor.mana_burn.wrapping_add(value);
            false
        }
        effect => {
            world.targeted_order_effect(actor, order_before, effect);
            false
        }
    }
}

/// `Unit::do_explore_to(MoveOrder*)` `0x005F24A0`, arm 3.
///
/// A capability receipt is acquired before `do_move`, so the phased post-move pointer/on-map
/// reads and `Unit::explore()` effect cannot become unavailable after movement has committed.
pub fn do_explore_to<W: WorkWorld>(
    actor: &mut UnitWork,
    world: &mut W,
    pf: &mut PathFinder,
    cov: &mut DispatchCoverage,
) -> ArmResult {
    let Some(order) = actor.orders.front().cloned() else {
        return ArmResult::NoOrder;
    };
    if order.kind != OrderIndex::ExploreTo {
        return ArmResult::MalformedOrder;
    }
    let frame = world.frame();
    let receipt = match world.explore_to_preflight(&*actor, &order) {
        Ok(receipt) => receipt,
        Err(TargetedOrderHostError::Unavailable) => return ArmResult::HostUnavailable,
        Err(TargetedOrderHostError::InvalidState(_)) => return ArmResult::MalformedOrder,
    };
    if !explore_receipt_matches(actor, &order, frame, &receipt) {
        return ArmResult::MalformedOrder;
    }

    let move_result = do_move(actor, world, pf, cov);
    if !targeted_order_plans::explore_scan_due(frame, actor.o) {
        return move_result;
    }
    let post = world.explore_to_post_move(&*actor, &order, &receipt);
    let effects = targeted_order_plans::plan_explore_to(ExploreToFacts {
        frame,
        object_index: actor.o,
        current_order_is_same: HostFact::known(post.current_order_is_same),
        actor_is_on_map: HostFact::known(post.actor_is_on_map),
    })
    .expect("successful EXPLORE_TO capability supplies both post-move facts");
    debug_assert_eq!(effects.first(), Some(&OrderEffect::DoMove));
    for effect in effects.into_iter().skip(1) {
        apply_targeted_effect(actor, world, &order, effect, cov);
    }
    move_result
}

/// `Unit::do_attack_ground(AttackGroundOrder*)` `0x005F1410`, arm 23.
/// All reached host reads are resolved before the first ordered effect.
pub fn do_attack_ground<W: WorkWorld>(
    actor: &mut UnitWork,
    world: &mut W,
    cov: &mut DispatchCoverage,
) -> ArmResult {
    let Some(order) = actor.orders.front().cloned() else {
        return ArmResult::NoOrder;
    };
    if order.kind != OrderIndex::AttackGround {
        return ArmResult::MalformedOrder;
    }
    let TargetedOrderPayload::AttackGround(state) = order.targeted_payload else {
        return ArmResult::MalformedOrder;
    };
    let receipt = match world.attack_ground_preflight(&*actor, &order) {
        Ok(receipt) => receipt,
        Err(TargetedOrderHostError::Unavailable) => return ArmResult::HostUnavailable,
        Err(TargetedOrderHostError::InvalidState(_)) => return ArmResult::MalformedOrder,
    };
    if !attack_ground_receipt_matches(actor, &order, state, &receipt) {
        return ArmResult::MalformedOrder;
    }
    let effects = match targeted_order_plans::plan_attack_ground(receipt.facts) {
        Ok(effects) => effects,
        Err(_) => return ArmResult::HostUnavailable,
    };
    let mut retired = false;
    for effect in effects {
        retired |= apply_targeted_effect(actor, world, &order, effect, cov);
    }
    if retired {
        ArmResult::Retired(KillReason::Completed)
    } else {
        ArmResult::Working
    }
}

/// `Unit::do_air_attack_ground(AirAttackGroundOrder*)` `0x005EA420`, arm 24.
/// The host token is acquired before mutating air physics; every following fact/effect is
/// infallible under that token.
pub fn do_air_attack_ground<W: WorkWorld>(
    actor: &mut UnitWork,
    world: &mut W,
    cov: &mut DispatchCoverage,
) -> ArmResult {
    let Some(order_before) = actor.orders.front().cloned() else {
        return ArmResult::NoOrder;
    };
    if order_before.kind != OrderIndex::AirAttackGround {
        return ArmResult::MalformedOrder;
    }
    let TargetedOrderPayload::AirAttackGround(mut state) = order_before.targeted_payload else {
        return ArmResult::MalformedOrder;
    };
    let receipt = match world.air_attack_ground_preflight(&*actor, &order_before) {
        Ok(receipt) => receipt,
        Err(TargetedOrderHostError::Unavailable) => return ArmResult::HostUnavailable,
        Err(TargetedOrderHostError::InvalidState(_)) => return ArmResult::MalformedOrder,
    };
    if !air_attack_ground_receipt_matches(actor, &order_before, state, &receipt) {
        return ArmResult::MalformedOrder;
    }

    let physics = world.air_attack_ground_physics(actor, &mut state, &receipt);
    assert_eq!(
        physics.order, state,
        "AIR_ATTACK_GROUND physics receipt must describe its committed walked order"
    );
    assert_eq!(
        physics.facts.recharge, actor.recharging,
        "AIR_ATTACK_GROUND physics receipt must describe the committed recharge byte"
    );
    assert_eq!(
        physics.facts.returning, state.air.returning,
        "AIR_ATTACK_GROUND physics receipt must describe AirOrder::returning"
    );
    assert_eq!(
        physics.facts.actor_angle, actor.body.angle as u32,
        "AIR_ATTACK_GROUND physics receipt must describe the committed actor angle"
    );
    let current = actor
        .orders
        .front_mut()
        .expect("air physics cannot remove the current AIR_ATTACK_GROUND order");
    assert_eq!(current.kind, OrderIndex::AirAttackGround);
    current.x = state.attack.att_x;
    current.y = state.attack.att_y;
    current.targeted_payload = TargetedOrderPayload::AirAttackGround(state);

    let effects = targeted_order_plans::plan_air_attack_ground(physics.facts)
        .expect("successful AIR_ATTACK_GROUND capability supplies every reached post-physics fact");
    for effect in effects {
        apply_targeted_effect(actor, world, &order_before, effect, cov);
    }
    ArmResult::Working
}

#[inline]
fn follow_state(payload: FollowOrderPayload) -> FollowOrderState {
    FollowOrderState {
        primary: FollowIdentity {
            o: payload.ox,
            who: payload.whom,
            uid: payload.uid,
        },
        fallback: FollowIdentity {
            o: payload.oxx,
            who: payload.whose,
            uid: payload.uid2,
        },
    }
}

#[inline]
fn follow_payload(state: FollowOrderState) -> FollowOrderPayload {
    FollowOrderPayload {
        ox: state.primary.o,
        whom: state.primary.who,
        uid: state.primary.uid,
        oxx: state.fallback.o,
        whose: state.fallback.who,
        uid2: state.fallback.uid,
    }
}

#[inline]
fn follow_payload_matches_order(order: &OrderRec, payload: FollowOrderPayload) -> bool {
    order.kind == OrderIndex::Follow
        && order.target_o == payload.ox
        && order.target_who == payload.whom
        && order.target_uid == payload.uid
}

#[inline]
fn follow_move_remainder(coord: i32) -> i16 {
    let low_coord = i32::from(coord as i16);
    let low_cells = i32::from((coord / movement::WCELL) as i16);
    low_coord.wrapping_sub(low_cells.wrapping_mul(movement::WCELL)) as i16
}

fn install_follow_move(
    actor: &mut UnitWork,
    x: i32,
    y: i32,
    angle: i32,
    tail: FollowMoveFacingTail,
) {
    // The planner/receipt pins this exact tuple. Keep the checks here as a second guard
    // against calling the local constructor with an unmodelled add_move_facing_order arm.
    assert_eq!(tail.arg4, 1);
    assert_eq!(tail.arg5, 0);
    assert_eq!(tail.queued, QueuePos::First as i32);
    assert_eq!(tail.arg7, 0);
    assert_eq!(tail.arg8, -1);
    assert_eq!(tail.coord9, -1);
    assert_eq!(tail.coord10, -1);
    assert_eq!(tail.arg11, 0);

    let world_x = x
        .wrapping_mul(movement::UCELL)
        .wrapping_add(movement::UCELL / 2);
    let world_y = y
        .wrapping_mul(movement::UCELL)
        .wrapping_add(movement::UCELL / 2);
    actor.orders.push_front(OrderRec {
        kind: OrderIndex::MoveTo,
        x: world_x,
        y: world_y,
        angle,
        dest_x: world_x,
        dest_y: world_y,
        facing: tail.arg8,
        orig_x: tail.coord9,
        orig_y: tail.coord10,
        off_x: follow_move_remainder(world_x),
        off_y: follow_move_remainder(world_y),
        ..OrderRec::default()
    });
    clear_partial_path(actor);
    update_action(actor);
}

/// `Unit::do_follow(FollowOrder*)` `0x005E65D0` (1,455 B), arm 11.
///
/// An applied host receipt freezes every reached cross-object observation and proves the
/// animation callback is available before the dispatcher publishes the updated primary /
/// fallback order identity. All other effects are exact local queue operations: bare kill,
/// full `work()` reentry, QUEUE_FIRST movement insertion, and immediate `do_move`.
pub fn do_follow<W: WorkWorld>(
    actor: &mut UnitWork,
    world: &mut W,
    pathfinder: &mut PathFinder,
    cov: &mut DispatchCoverage,
) -> ArmResult {
    let Some(order_before) = update_order(actor) else {
        return ArmResult::NoOrder;
    };
    let Some(payload_before) = order_before.follow else {
        return ArmResult::MalformedOrder;
    };
    if !follow_payload_matches_order(&order_before, payload_before) {
        return ArmResult::MalformedOrder;
    }

    let request = FollowExecutorRequest {
        actor: FollowActorFacts {
            o: actor.o,
            who: actor.who,
            x: actor.body.x,
            y: actor.body.y,
            speed: actor.guy_env.unit_speed,
            los: actor.follow_los,
        },
        order: follow_state(payload_before),
    };
    let receipt = world.follow_preflight(&*actor, request);
    if receipt.status != FollowExecutorTransactionStatus::Applied || !receipt.validates(&request) {
        return ArmResult::HostUnavailable;
    }
    let Some(plan) = receipt.plan else {
        return ArmResult::HostUnavailable;
    };

    let payload_after = follow_payload(plan.order);
    let Some(current) = actor.orders.front_mut() else {
        return ArmResult::HostUnavailable;
    };
    if *current != order_before {
        return ArmResult::HostUnavailable;
    }
    current.target_o = payload_after.ox;
    current.target_who = payload_after.whom;
    current.target_uid = payload_after.uid;
    current.follow = Some(payload_after);

    for effect in plan.effects {
        match effect {
            FollowExecutorEffect::KillCurrentOrder { flags } => {
                assert_eq!(flags, 0);
                kill_current_order(actor, KillReason::Failed);
                cov.failed += 1;
                return ArmResult::Retired(KillReason::Failed);
            }
            FollowExecutorEffect::ReenterWork => {
                return work(actor, world, pathfinder, cov).result;
            }
            FollowExecutorEffect::SetAnim { anim, mode, choose } => {
                world.follow_set_anim(actor, anim, mode, choose);
            }
            FollowExecutorEffect::AddMoveFacingOrder { x, y, angle, tail } => {
                install_follow_move(actor, x, y, angle, tail);
            }
            FollowExecutorEffect::UpdateOrderThenDoMove => {
                return do_move(actor, world, pathfinder, cov);
            }
        }
    }
    ArmResult::Working
}

/// `Unit::do_repair(RepairOrder*)` `0x005EE420`, arm 13.
///
/// The external reads are acquired as one versioned snapshot before retail's first
/// animation write. The pure planner then fixes the complete effect order, and one WorkWorld
/// callback commits that slice atomically. A missing or incoherent receipt is zero-mutation;
/// a successful preflight is an infallibility contract for the commit.
pub fn do_repair<W: WorkWorld>(
    actor: &mut UnitWork,
    world: &mut W,
    cov: &mut DispatchCoverage,
) -> ArmResult {
    let Some(order) = actor.orders.front().cloned() else {
        return ArmResult::NoOrder;
    };
    if order.kind != OrderIndex::Repair {
        return ArmResult::MalformedOrder;
    }

    let frame = world.frame();
    let receipt = match world.repair_preflight(&*actor, &order) {
        Ok(receipt) => receipt,
        Err(RepairHostError::Unavailable) => return ArmResult::HostUnavailable,
        Err(RepairHostError::InvalidState(_)) => return ArmResult::MalformedOrder,
    };
    if !repair_receipt_matches(actor, &order, frame, &receipt) {
        return ArmResult::HostUnavailable;
    }
    let plan = match repair_order::plan_repair(&receipt.facts) {
        Ok(plan) => plan,
        Err(_) => return ArmResult::MalformedOrder,
    };
    let actor_identity = (actor.who, actor.o, actor.uid);
    let commit = world.repair_commit(actor, &order, &plan.effects, &receipt);
    assert!(
        repair_commit_matches(actor_identity, &receipt, &commit, &plan.effects),
        "REPAIR host returned an incomplete or foreign atomic commit receipt"
    );

    let retired = plan.effects.iter().any(|effect| {
        matches!(
            effect,
            repair_order::RepairEffect::KillCurrentOrder { arg: 0 }
        )
    });
    if retired {
        cov.completed += 1;
        ArmResult::Retired(KillReason::Completed)
    } else {
        ArmResult::Working
    }
}

#[inline]
fn build_at_preflight_matches_actor(
    actor: &UnitWork,
    order: &OrderRec,
    input: &BuildAtPreflightInput,
) -> bool {
    input.builder
        == (ObjectKey {
            who: i32::from(actor.who),
            o: i32::from(actor.o),
            uid: actor.uid,
        })
        && input.target_order
            == (ObjectKey {
                who: order.target_who,
                o: order.target_o,
                uid: order.target_uid,
            })
        && input.order_flags == order.flags
        && input.builder_x == actor.body.x
        && input.builder_y == actor.body.y
        && input.builder_angle == actor.body.angle
        && input.unit_decoy == (actor.unit_masks & 1 != 0)
        // A true next-action result includes the external virtual-validity query; false is
        // allowed with a queued but invalid action, but true requires a physical next node.
        && (!input.has_next_action_after_retire || actor.orders.len() > 1)
}

#[inline]
fn retire_build_at(actor: &mut UnitWork, cov: &mut DispatchCoverage) {
    kill_current_order(actor, KillReason::Completed);
    cov.completed += 1;
}

/// `Unit::do_build(UnitOrder*)` `0x005EEBF0` (1,711 bytes), arm 6.
///
/// The external snapshot is converted through [`construction_builder::preflight`], which
/// preserves direct `do_build`'s deliberate `(who,o)`-only Wall validation (no UID check),
/// Farm footprint exception, animation/facing-before-DECOY order, and group-flag reswarm.
/// Every fallible capability is acquired before the first mutation. After that point the
/// host operations are infallible and retail-ordered:
///
/// - invalid/active: bare-kill first, then expose the next action to the required tail;
/// - reswarm: bare-kill, then temporary one-member Group + `action_swarm_around(FIRST)`;
/// - ready: animation, optional facing, then exact `Wall::do_construct`;
/// - completed: activation returns, bare-kill, then completion reassignment.
pub fn do_build_at<W: WorkWorld>(
    actor: &mut UnitWork,
    world: &mut W,
    cov: &mut DispatchCoverage,
) -> ArmResult {
    let Some(order) = actor.orders.front().cloned() else {
        return ArmResult::NoOrder;
    };
    if order.kind != OrderIndex::BuildAt {
        return ArmResult::MalformedOrder;
    }
    let input = match world.build_at_preflight(&*actor, &order) {
        Ok(input) => input,
        Err(BuildAtHostError::Unavailable) => return ArmResult::HostUnavailable,
        Err(BuildAtHostError::InvalidState(_)) => return ArmResult::MalformedOrder,
    };
    if !build_at_preflight_matches_actor(actor, &order, &input) {
        return ArmResult::MalformedOrder;
    }

    match construction_builder::preflight(input) {
        PreflightPlan::RetireInvalid { then } => {
            retire_build_at(actor, cov);
            if then == AfterInvalidTarget::BuildDone {
                world.build_at_effect(
                    actor,
                    &order,
                    BuildAtEffect::FinishTail {
                        reason: BuilderFinish::InvalidTarget,
                    },
                );
            }
            ArmResult::Retired(KillReason::Completed)
        }
        PreflightPlan::RetireActive => {
            retire_build_at(actor, cov);
            world.build_at_effect(
                actor,
                &order,
                BuildAtEffect::FinishTail {
                    reason: BuilderFinish::TargetAlreadyActive,
                },
            );
            ArmResult::Retired(KillReason::Completed)
        }
        PreflightPlan::Reswarm {
            preserve_group_flag,
        } => {
            retire_build_at(actor, cov);
            world.build_at_effect(
                actor,
                &order,
                BuildAtEffect::Reswarm {
                    preserve_group_flag,
                },
            );
            ArmResult::Working
        }
        PreflightPlan::AnimateFace {
            animation,
            set_angle,
            contribute,
        } => {
            world.build_at_effect(actor, &order, BuildAtEffect::SetAnimation { animation });
            if let Some(angle) = set_angle {
                world.build_at_effect(actor, &order, BuildAtEffect::SetAngle { angle });
            }
            if !contribute {
                return ArmResult::Working;
            }
            match world.build_at_construct(actor, &order) {
                BuildAtConstructResult::SiteRejected
                | BuildAtConstructResult::Progressed { .. } => ArmResult::Working,
                BuildAtConstructResult::Completed { .. } => {
                    // `Build::activate(0,1,1)` has already returned at this boundary.
                    retire_build_at(actor, cov);
                    world.build_at_effect(
                        actor,
                        &order,
                        BuildAtEffect::FinishTail {
                            reason: BuilderFinish::SiteCompleted,
                        },
                    );
                    ArmResult::Retired(KillReason::Completed)
                }
            }
        }
    }
}

/// `Unit::do_group_attack_to(GroupMoveOrder*)` `0x005E74E0` (192 bytes), arm 21.
///
/// The shipped wrapper has four observable stages:
///
/// 1. execute [`do_group_move`];
/// 2. continue only if the exact same order node remains current;
/// 3. continue only when `(Game::frame + actor.o) % 15 == 0`;
/// 4. when the unit has an attack and its virtual target predicate returns zero, call
///    `fight(-1,0,0,1,0)` and return; otherwise call `do_attack_to_pause(order)`.
///
/// `fight` and `do_attack_to_pause` remain external mechanics, but they are mandatory rather
/// than guessed. A grouped actor preflights both callbacks before stage 1, so an unavailable
/// host cannot advance movement and then fail. The exact ungrouped conversion needs neither
/// callback: it replaces the node with `ATTACK_TO`, causing stage 2 to return as in retail.
pub fn do_group_attack_to<W: WorkWorld>(
    u: &mut UnitWork,
    w: &mut W,
    pf: &mut PathFinder,
    cov: &mut DispatchCoverage,
) -> ArmResult {
    let Some(order) = u.orders.front().cloned() else {
        return ArmResult::NoOrder;
    };
    if order.kind != OrderIndex::GroupAttackTo {
        return ArmResult::MalformedOrder;
    }

    if u.group >= 0 {
        match w.group_attack_to_preflight(&*u, &order) {
            Ok(()) => {}
            Err(GroupAttackToHostError::Unavailable) => return ArmResult::HostUnavailable,
            Err(GroupAttackToHostError::InvalidState(_)) => return ArmResult::MalformedOrder,
        }
    }

    let move_result = do_group_move(u, w, pf, cov);
    if !same_group_order(u, &order) || !phase_due(w.frame(), u.o, 15) {
        return move_result;
    }
    if w.group_attack_to_calls_fight(&*u, &order) {
        return w.group_attack_to_fight(u, &order);
    }
    if u.group >= 0 && w.group_attack_to_pause_gate(&*u, &order) {
        let current = u
            .orders
            .front_mut()
            .expect("same_group_order verified a live GROUP_ATTACK_TO node");
        current.pause = 15;
    }
    ArmResult::Working
}

fn ungroup_current_attack(u: &mut UnitWork, group_id: i32) -> bool {
    let Some(current) = u.orders.front().cloned() else {
        return false;
    };
    if current.kind != OrderIndex::GroupAttack || current.group_id != group_id {
        return false;
    }
    // Retail allocates a fresh ATTACK and invokes AttackOrder::operator=: only UnitOrder's
    // flag, TargetOrder identity, and AttackOrder's own fields cross the conversion.
    let ordinary = OrderRec {
        kind: OrderIndex::Attack,
        flags: current.flags,
        target_o: current.target_o,
        target_who: current.target_who,
        target_uid: current.target_uid,
        attack_def_x: current.attack_def_x,
        attack_def_y: current.attack_def_y,
        attack_mandatory: current.attack_mandatory,
        attack_defensive: current.attack_defensive,
        attack_in_range: current.attack_in_range,
        attack_ever_in_range: current.attack_ever_in_range,
        attack_new_ord: current.attack_new_ord,
        ..OrderRec::default()
    };
    *u.orders.front_mut().expect("front was cloned above") = ordinary;
    true
}

fn set_group_attack_angle(u: &mut UnitWork, angle: i32) {
    let delta = (angle as u32).wrapping_sub(u.body.angle as u32);
    if delta > 0x3fff_ffff && delta < 0xc000_0001 {
        u.unit_masks ^= 2;
    }
    u.body.angle = angle;
}

fn group_attack_tail_target(u: &UnitWork, order: &OrderRec) -> Option<(i32, i32)> {
    if order.group_attack_temporary == 0 || u.path.is_empty() || u.orders.len() < 2 {
        return None;
    }
    // Retail begins on the list tail (the executing/oldest node), then inspects
    // `current_node->prev`: the immediately queued order in execution order.
    let queued = u.orders.iter().nth(1)?;
    if queued.kind != OrderIndex::GroupMove
        || (queued.dest_x == u.body.x && queued.dest_y == u.body.y)
    {
        return None;
    }
    Some((queued.dest_x, queued.dest_y))
}

fn group_attack_tail_step<W: WorkWorld>(
    u: &mut UnitWork,
    w: &mut W,
    pf: &mut PathFinder,
    original: OrderRec,
    target: (i32, i32),
) -> ArmResult {
    u.orders.reset();
    let removed = u.orders.remove_current();
    debug_assert_eq!(
        removed.as_ref().map(|order| order.kind),
        Some(OrderIndex::GroupAttack)
    );
    // The call is deliberately retained even though retail discards its return value.
    w.group_attack_effect(u, &original, GroupAttackEffect::ProbeSpeed { mode: 1 });
    // `tail()` now publishes the queued GROUP_MOVE as current before `move_step(order, 1)`.
    u.orders.reset();
    let result = integrate_move_step(u, w, pf, target, 1).0;
    // `orderlist.add(original)` makes the GROUP_ATTACK newest, hence last in execution order.
    u.orders.push_back(original);
    u.orders.reset();
    result
}

/// `Unit::do_group_attack(GroupAttackOrder*)` `0x005E75A0` (1,011 bytes), arm 20.
///
/// The recovered local orchestration is complete: recharge hold, group angle publication,
/// leader/follower split, scratch-target writes before `fight`, ordered group kill/
/// distribution/refresh effects, the facing tail, the exceptional GROUP_ATTACK rotation
/// behind a queued GROUP_MOVE plus `move_step`, and the ungrouped conversion to ATTACK.
/// Global target/group searches are supplied by one mandatory preflight plan, before any
/// actor mutation; all later effects are infallible.
pub fn do_group_attack<W: WorkWorld>(
    u: &mut UnitWork,
    w: &mut W,
    pf: &mut PathFinder,
    _cov: &mut DispatchCoverage,
) -> ArmResult {
    let Some(order) = u.orders.front().cloned() else {
        return ArmResult::NoOrder;
    };
    if order.kind != OrderIndex::GroupAttack {
        return ArmResult::MalformedOrder;
    }
    if u.recharging != 0 {
        return ArmResult::Working;
    }
    if u.group < 0 {
        set_group_attack_angle(u, order.group_angle);
        u.lead_guy.angle = order.group_angle;
        return if ungroup_current_attack(u, order.group_id) {
            ArmResult::Working
        } else {
            ArmResult::MalformedOrder
        };
    }

    let plan = match w.group_attack_preflight(&*u, &order) {
        Ok(plan) => plan,
        Err(GroupAttackHostError::Unavailable) => return ArmResult::HostUnavailable,
        Err(GroupAttackHostError::InvalidState(_)) => return ArmResult::MalformedOrder,
    };
    let is_leader = order.group_oxx == i32::from(u.o) && order.group_whose == i32::from(u.who);
    let role_matches = matches!(
        (&plan, is_leader),
        (
            GroupAttackPlan::LeaderFight { .. } | GroupAttackPlan::LeaderKillMoveAndDistribute,
            true
        ) | (
            GroupAttackPlan::FollowerFight { .. }
                | GroupAttackPlan::RefreshGroupOrder
                | GroupAttackPlan::FollowerFace
                | GroupAttackPlan::FollowerTailStep,
            false
        ) | (GroupAttackPlan::KillGroupOrder, true)
    );
    if !role_matches {
        return ArmResult::MalformedOrder;
    }
    let tail_target = match plan {
        GroupAttackPlan::FollowerTailStep => {
            let Some(target) = group_attack_tail_target(u, &order) else {
                return ArmResult::MalformedOrder;
            };
            Some(target)
        }
        _ => None,
    };
    if matches!(
        plan,
        GroupAttackPlan::LeaderFight {
            target_o,
            target_who
        } | GroupAttackPlan::FollowerFight {
            target_o,
            target_who
        } if target_o < 0 || target_who < 0
    ) {
        return ArmResult::MalformedOrder;
    }

    // `Unit::set_angle(group_angle, ?, 0)` precedes the shipped grouped branch.
    set_group_attack_angle(u, order.group_angle);
    w.group_attack_effect(
        u,
        &order,
        GroupAttackEffect::SetAngle {
            angle: order.group_angle,
        },
    );
    // `Guy::set_angle(group_angle, 0)` is the final call inside Unit::set_angle.
    u.lead_guy.angle = order.group_angle;
    match plan {
        GroupAttackPlan::LeaderFight {
            target_o,
            target_who,
        } => {
            let current = u
                .orders
                .front_mut()
                .expect("preflight kept the front node live");
            current.group_attack_oxxx = target_o;
            current.group_attack_whosoever = target_who;
            w.group_attack_effect(
                u,
                &order,
                GroupAttackEffect::Fight {
                    target_o,
                    target_who,
                    mandatory: order.attack_mandatory,
                    temporary: order.group_attack_temporary,
                },
            );
            ArmResult::Working
        }
        GroupAttackPlan::FollowerFight {
            target_o,
            target_who,
        } => {
            let current = u
                .orders
                .front_mut()
                .expect("preflight kept the front node live");
            current.group_attack_oxxx = target_o;
            current.group_attack_whosoever = target_who;
            w.group_attack_effect(
                u,
                &order,
                GroupAttackEffect::Fight {
                    target_o,
                    target_who,
                    mandatory: 1,
                    temporary: 1,
                },
            );
            ArmResult::Working
        }
        GroupAttackPlan::LeaderKillMoveAndDistribute => {
            w.group_attack_effect(
                u,
                &order,
                GroupAttackEffect::KillGroupMove { id: order.group_id },
            );
            w.group_attack_effect(
                u,
                &order,
                GroupAttackEffect::DistributeAttack {
                    target_o: order.target_o,
                    target_who: order.target_who,
                },
            );
            ArmResult::Working
        }
        GroupAttackPlan::KillGroupOrder => {
            w.group_attack_effect(
                u,
                &order,
                GroupAttackEffect::KillGroupOrder { id: order.group_id },
            );
            ArmResult::Working
        }
        GroupAttackPlan::RefreshGroupOrder => {
            w.group_attack_effect(
                u,
                &order,
                GroupAttackEffect::RefreshGroupOrder { id: order.group_id },
            );
            ArmResult::Working
        }
        GroupAttackPlan::FollowerFace => {
            if u.body.angle == order.group_angle {
                w.group_attack_effect(u, &order, GroupAttackEffect::SetIdleAnim);
            }
            ArmResult::Working
        }
        GroupAttackPlan::FollowerTailStep => {
            let target = tail_target.expect("validated above");
            group_attack_tail_step(u, w, pf, order, target)
        }
    }
}

/// `Unit::do_attack(AttackOrder*)` `0x005F1B80` (1,822 B), arm 10.
///
/// The order-layer half: validate the target's identity, hand the shot to the host's damage
/// pipeline, and retire on the two failures retail retires on — a target whose `uid` no
/// longer matches (`repath(); kill_current_order(0)` at `0x005F214B`) and a target that
/// cannot be hurt.
///
/// The target-*selection* half — `Unit::fight` `0x005FD4D0` and `Unit::find_attack_pos`
/// `0x00601280`, 15,281 bytes between them — is absent, here and everywhere in the tree.
pub fn do_attack<W: WorkWorld>(
    u: &mut UnitWork,
    w: &mut W,
    cov: &mut DispatchCoverage,
) -> ArmResult {
    let Some(ord) = update_order(u) else {
        return ArmResult::NoOrder;
    };
    if ord.target_who < 0 || ord.target_o < 0 {
        kill_current_order(u, KillReason::Failed);
        cov.failed += 1;
        return ArmResult::Retired(KillReason::Failed);
    }
    let Some(t) = w.target(ord.target_who, ord.target_o) else {
        kill_current_order(u, KillReason::Failed);
        cov.failed += 1;
        return ArmResult::Retired(KillReason::Failed);
    };
    if !t.active || t.uid != ord.target_uid {
        kill_current_order(u, KillReason::Failed);
        cov.failed += 1;
        return ArmResult::Retired(KillReason::Failed);
    }
    match w.attack(&*u, &ord) {
        AttackOutcome::Fired(d) => {
            u.idle = 0;
            ArmResult::Fired(d)
        }
        AttackOutcome::OutOfRange => ArmResult::Working,
        AttackOutcome::Recharging => ArmResult::Working,
        AttackOutcome::Impossible => {
            kill_current_order(u, KillReason::Failed);
            cov.failed += 1;
            ArmResult::Retired(KillReason::Failed)
        }
    }
}

/// `Unit::do_gather(GatherOrder*)` `0x005EF2A0` (2,600 B), arm 7.
///
/// The three exits retail has, all of them `kill_current_order` [measured]:
///
/// | site | condition | what retail does |
/// |---|---|---|
/// | `0x005EF39E` | the node object is gone or not a good | `kill_current_order(0)` then `Unit::find_gather_target` `0x005E3DF0` |
/// | `0x005EF63E`, `0x005EF68F` | `gatherers > max_gatherers` on the node | `kill_current_order(0)` **and** `flags |= 0x10` |
/// | `0x005EF9D6` | the carry was deposited | `kill_current_order(0)` |
///
/// The `flags |= 0x10` on the slots-full path is the only place in the order layer where a
/// retirement leaves a *distinguishing mark* on the object, which is why it is reproduced
/// exactly ([`obj_flags::GATHER_REFUSED`]). Travel is not this arm's job: retail reaches the
/// node through a `MOVE_TO` queued ahead of the `GATHER`, so a unit out of tolerance simply
/// waits.
pub fn do_gather<W: WorkWorld>(
    u: &mut UnitWork,
    w: &mut W,
    cov: &mut DispatchCoverage,
) -> ArmResult {
    let Some(ord) = update_order(u) else {
        return ArmResult::NoOrder;
    };
    if ord.target_who < 0 || ord.target_o < 0 {
        kill_current_order(u, KillReason::Failed);
        cov.failed += 1;
        return ArmResult::Retired(KillReason::Failed);
    }
    let Some(t) = w.target(ord.target_who, ord.target_o) else {
        kill_current_order(u, KillReason::Failed);
        cov.failed += 1;
        return ArmResult::Retired(KillReason::Failed);
    };
    if !t.active || t.uid != ord.target_uid {
        kill_current_order(u, KillReason::Failed);
        cov.failed += 1;
        return ArmResult::Retired(KillReason::Failed);
    }
    if vector_dist(t.x - u.body.x, t.y - u.body.y) > u.tolerance {
        // The queued MOVE_TO ahead of this order does the walking.
        return ArmResult::Working;
    }
    match w.gather(&*u, &ord) {
        GatherOutcome::Yield(n) => {
            u.idle = 0;
            ArmResult::Gathered(n)
        }
        GatherOutcome::Exhausted => {
            kill_current_order(u, KillReason::Completed);
            cov.completed += 1;
            ArmResult::Retired(KillReason::Completed)
        }
        GatherOutcome::SlotsFull => {
            kill_current_order(u, KillReason::Failed);
            cov.failed += 1;
            u.flags |= obj_flags::GATHER_REFUSED;
            ArmResult::Retired(KillReason::Failed)
        }
    }
}

fn store_patrol_payload(u: &mut UnitWork, payload: PatrolPayload) -> bool {
    u.orders.reset();
    let Some(front) = u.orders.current_mut() else {
        return false;
    };
    front.patrol_payload = payload;
    true
}

/// `Unit::do_patrol` `0x005F1910` (`GROUP_PATROL`, order 22).
pub fn do_group_patrol<W: WorkWorld>(u: &mut UnitWork, w: &mut W) -> ArmResult {
    u.orders.reset();
    let Some(front) = u.orders.current() else {
        return ArmResult::NoOrder;
    };
    let PatrolPayload::Group(mut order) = front.patrol_payload.clone() else {
        return ArmResult::MalformedOrder;
    };

    let scramblable = u.inside_down >= 0 && w.patrol_inside_is_scramblable(u.who, u.inside_down);
    let step = patrol::step_group_patrol(
        &mut order,
        u.body.x,
        u.body.y,
        u.o,
        u.who,
        u.group,
        u.inside_down,
        scramblable,
    );
    if !store_patrol_payload(u, PatrolPayload::Group(order)) {
        return ArmResult::NoOrder;
    }

    match step.action {
        GroundPatrolAction::InsertAttackTo(m) => {
            let leg = OrderRec {
                kind: OrderIndex::AttackTo,
                // `do_patrol` explicitly clears PATHED, GROUP and DISEMBARK.
                flags: 0,
                x: m.x,
                y: m.y,
                angle: m.angle,
                dest: m.dest,
                tolerance: m.tolerance,
                pause: m.pause,
                retry: m.retry,
                attempts: m.attempts,
                timer: m.timer,
                facing: m.facing,
                dest_x: m.dest_x,
                dest_y: m.dest_y,
                last_x: m.last_x,
                last_y: m.last_y,
                off_x: m.off_x,
                off_y: m.off_y,
                ..OrderRec::default()
            };
            // LinkListBase::add, clear_partial_path, reset current to head, update_action.
            u.orders.push_front(leg);
            clear_partial_path(u);
            update_action(u);
        }
        GroundPatrolAction::MoveGroup(request) => w.group_patrol_move(u, request),
        GroundPatrolAction::IdleAnimation => {
            // Retail calls set_anim(0,0,1); idle remains an animation concern rather than
            // pretending the order left the queue.
        }
    }
    if let Some(inside_o) = step.scramble_inside_down {
        w.patrol_scramble_inside(u, inside_o);
    }
    ArmResult::Working
}

fn relative_scan_point(
    point: (i32, i32),
    home: Option<(i32, i32)>,
    max_x: i32,
    max_y: i32,
    relative_to_home: bool,
) -> (i32, i32) {
    // Both scan-origin blocks test TypeIndex::FIGHTERBOMBER (0x134), not the broader
    // is-plane or non-animal predicate used for the flight target.
    if relative_to_home {
        if let Some((hx, hy)) = home {
            return (
                point.0.wrapping_add(hx).clamp(0, max_x.saturating_sub(1)),
                point.1.wrapping_add(hy).clamp(0, max_y.saturating_sub(1)),
            );
        }
    }
    point
}

/// `Unit::do_air_patrol` `0x005EA620` (`AIR_PATROL`, order 17).
pub fn do_air_patrol<W: WorkWorld>(u: &mut UnitWork, w: &mut W) -> ArmResult {
    u.orders.reset();
    let Some(front) = u.orders.current() else {
        return ArmResult::NoOrder;
    };
    let PatrolPayload::Air(mut order) = front.patrol_payload.clone() else {
        return ArmResult::MalformedOrder;
    };

    w.patrol_think_bird(u, &mut order);
    let home = if order.air.oxx >= 0 && order.air.whose >= 0 {
        w.target(order.air.whose, order.air.oxx)
            .filter(|t| t.active)
            .map(|t| (t.x, t.y))
    } else {
        None
    };
    let max_x = w.tiles_w().saturating_mul(movement::TILE);
    let max_y = w.tiles_h().saturating_mul(movement::TILE);
    let flight_target = patrol::air_patrol_target(&mut order, u.type_is_animal, home, max_x, max_y);
    if !w.air_patrol_physics(u, &mut order, flight_target.0, flight_target.1) {
        store_patrol_payload(u, PatrolPayload::Air(order));
        return ArmResult::Working;
    }

    let frame = w.frame();
    let mut input = AirPatrolAfterPhysics {
        actor_x: u.body.x,
        actor_y: u.body.y,
        actor_o: u.o,
        frame,
        is_animal: u.type_is_animal,
        spell_time: u.spell_time,
        order_list_len: u.orders.len(),
        unit_target: None,
        building_target: None,
    };
    if matches!(
        patrol::advance_air_patrol_waypoint_after_physics(&mut order, flight_target, &input),
        AirPatrolAction::KillCurrent
    ) {
        kill_current_order(u, KillReason::Completed);
        return ArmResult::Retired(KillReason::Completed);
    }
    let phase = (u.o as i32).wrapping_add(frame);
    let fighter_bomber = w.patrol_actor_is_type(u, 0x134, false);
    let mut unit_target = None;
    if !u.type_is_animal && order.air.returning == 0 && phase % 16 == 0 {
        let n = order.points.len();
        let last = (order.points.x[n - 1], order.points.y[n - 1]);
        let (sx, sy) = relative_scan_point(last, home, max_x, max_y, fighter_bomber);
        let search = if w.patrol_actor_is_type(u, 0x130, false) {
            AirPatrolSearch::BomberFirst
        } else {
            AirPatrolSearch::AirFirst
        };
        unit_target = w.air_patrol_unit_target(u, &order, sx, sy, search);
    }
    input.unit_target = unit_target;
    let unit_action = patrol::step_air_patrol_after_unit_search(&order, &input);

    let mut building_target = None;
    if unit_action == AirPatrolAction::Continue && !u.type_is_animal && phase % 32 == 0 {
        let cursor = order.points.clamp_air_cursor();
        let point = (order.points.x[cursor], order.points.y[cursor]);
        let (sx, sy) = relative_scan_point(point, home, max_x, max_y, fighter_bomber);
        building_target = w.air_patrol_building_target(u, &order, sx, sy);
    }

    input.building_target = building_target;
    let action = if unit_action != AirPatrolAction::Continue {
        unit_action
    } else {
        patrol::step_air_patrol_after_building_search(&order, &input)
    };

    match action {
        AirPatrolAction::KillCurrent => {
            kill_current_order(u, KillReason::Completed);
            ArmResult::Retired(KillReason::Completed)
        }
        AirPatrolAction::InsertStrafe { target, mandatory } => {
            let strafe =
                patrol::patrol_strafe_order(target, order.air.oxx, order.air.whose, mandatory);
            if !store_patrol_payload(u, PatrolPayload::Air(order)) {
                return ArmResult::NoOrder;
            }
            // add_strafe_order(..., QUEUE_FIRST, group=0): append, then reset current to
            // the newly inserted head. OrderQueue::push_front is that measured effect.
            u.orders.push_front(OrderRec::strafe(strafe));
            clear_partial_path(u);
            update_action(u);
            ArmResult::Working
        }
        AirPatrolAction::PrimeAnimalSpellTime => {
            u.spell_time = 1;
            store_patrol_payload(u, PatrolPayload::Air(order));
            ArmResult::Working
        }
        AirPatrolAction::Continue => {
            store_patrol_payload(u, PatrolPayload::Air(order));
            ArmResult::Working
        }
    }
}

pub(crate) fn special_anim_state(state: SpecialAnimOrderState) -> SpecialAnimState {
    SpecialAnimState {
        special_type: match state.special_type {
            crate::order::SpecialAnimType::Enter => SpecialAnimKind::Enter,
            crate::order::SpecialAnimType::Exit => SpecialAnimKind::Exit,
            crate::order::SpecialAnimType::Unit => SpecialAnimKind::Unit,
        },
        started: state.started,
        frames: state.frames,
        data1: state.data1,
        data2: state.data2,
        data3: state.data3,
        data4: state.data4,
        ox: state.ox,
        whom: state.whom,
    }
}

/// Narrow host surface used by the SPECIAL_ANIM dispatcher and the production tick bridge.
///
/// Keeping this separate from [`WorkWorld`] lets `Sim::do_frame` execute arm 25 without
/// pretending that its terrain/collision host can service every other order arm.  A production
/// host may return [`SpecialAnimHostError::Unavailable`] for a reached external tail; that is a
/// typed refusal and authorizes no local publication.
pub(crate) trait SpecialAnimWorld {
    fn special_anim_preflight(
        &mut self,
        actor: &UnitWork,
        order: &OrderRec,
    ) -> Result<SpecialAnimExecutorReceipt, SpecialAnimHostError>;

    fn special_anim_commit(
        &mut self,
        actor: &mut UnitWork,
        order: &OrderRec,
        preflight: &SpecialAnimExecutorReceipt,
    ) -> Result<SpecialAnimCommitReceipt, SpecialAnimHostError>;
}

struct WorkWorldSpecialAnim<'a, W>(&'a mut W);

impl<W: WorkWorld> SpecialAnimWorld for WorkWorldSpecialAnim<'_, W> {
    fn special_anim_preflight(
        &mut self,
        actor: &UnitWork,
        order: &OrderRec,
    ) -> Result<SpecialAnimExecutorReceipt, SpecialAnimHostError> {
        self.0.special_anim_preflight(actor, order)
    }

    fn special_anim_commit(
        &mut self,
        actor: &mut UnitWork,
        order: &OrderRec,
        preflight: &SpecialAnimExecutorReceipt,
    ) -> Result<SpecialAnimCommitReceipt, SpecialAnimHostError> {
        self.0.special_anim_commit(actor, order, preflight)
    }
}

/// State-wired SPECIAL_ANIM adapter for the complete [`WorkWorld`] dispatcher.
///
/// The pure module fixes the exact branch/effect sequence. This boundary accepts it only through
/// a snapshot-bound preflight followed by one atomic host commit.
pub fn do_special_anim<W: WorkWorld>(
    actor: &mut UnitWork,
    world: &mut W,
    cov: &mut DispatchCoverage,
) -> ArmResult {
    do_special_anim_with_host(actor, &mut WorkWorldSpecialAnim(world), cov)
}

/// Execute SPECIAL_ANIM through its narrow atomic host transaction.
///
/// This is crate-visible so the canonical `Sim::do_frame` owner can reach the same adapter
/// without implementing unrelated movement, patrol, combat, and group callbacks.
pub(crate) fn do_special_anim_with_host<H: SpecialAnimWorld>(
    actor: &mut UnitWork,
    world: &mut H,
    cov: &mut DispatchCoverage,
) -> ArmResult {
    let Some(order_before) = actor.orders.front().cloned() else {
        return ArmResult::NoOrder;
    };
    if order_before.kind != OrderIndex::SpecialAnim
        || !order_before.has(ORDER_GROUP)
        || order_before.follow.is_some()
        || order_before.form_order.is_some()
        || !matches!(&order_before.targeted_payload, TargetedOrderPayload::None)
        || !matches!(&order_before.patrol_payload, PatrolPayload::None)
    {
        return ArmResult::MalformedOrder;
    }
    let Some(walked) = order_before.special_anim else {
        return ArmResult::MalformedOrder;
    };
    let order = special_anim_state(walked);
    let actor_identity = SpecialAnimObjectIdentity {
        o: i32::from(actor.o),
        who: i32::from(actor.who),
        uid: actor.uid,
    };

    // The shipped SPECIAL_UNIT arm is a literal no-op and reaches no host surface.
    if order.special_type == SpecialAnimKind::Unit {
        let request = SpecialAnimExecutorRequest {
            order,
            actor: SpecialAnimActorFacts {
                identity: actor_identity,
            },
            enter_target: None,
            exit_target: None,
            random_draws: None,
            helicopter_samples: None,
            terrain_z: None,
        };
        return match special_anim_executor::plan_special_anim_executor(request) {
            Ok(plan) if plan.branch == SpecialAnimBranch::UnitNoOp && plan.steps.is_empty() => {
                ArmResult::Working
            }
            _ => ArmResult::MalformedOrder,
        };
    }

    let preflight = match world.special_anim_preflight(&*actor, &order_before) {
        Ok(receipt) => receipt,
        Err(SpecialAnimHostError::Unavailable) => return ArmResult::HostUnavailable,
        Err(SpecialAnimHostError::InvalidState(_)) => return ArmResult::MalformedOrder,
    };
    let recomputed = special_anim_executor::preflight_special_anim_executor(
        preflight.snapshot,
        preflight.request,
    );
    if preflight.request.actor.identity != actor_identity
        || preflight.request.order != order
        || !matches!(recomputed, Ok(ref receipt) if receipt == &preflight)
    {
        return ArmResult::MalformedOrder;
    }

    // `special_anim_commit` owns both local and external writes. Retain a local before-image so
    // a malformed receipt cannot strand the compact actor half-mutated; the host contract makes
    // the same guarantee for all external surfaces.
    let before = actor.clone();
    let commit = match world.special_anim_commit(actor, &order_before, &preflight) {
        Ok(receipt) => receipt,
        Err(SpecialAnimHostError::Unavailable) => {
            *actor = before;
            return ArmResult::HostUnavailable;
        }
        Err(SpecialAnimHostError::InvalidState(_)) => {
            *actor = before;
            return ArmResult::MalformedOrder;
        }
    };
    if !commit.validates(&preflight) {
        *actor = before;
        return ArmResult::MalformedOrder;
    }

    debug_assert_ne!(preflight.plan.branch, SpecialAnimBranch::UnitNoOp);
    cov.completed += 1;
    ArmResult::Retired(KillReason::Completed)
}

/// `Unit::do_job(enum OrderIndex, class UnitOrder*)` `0x00617A10`, the 28-entry jump table at
/// `0x00617B94`. [measured — the `switch` has 27 case labels; `PATROL` (5) has none.]
///
/// Every arm is present. Arms without a ported body dispatch, are counted, and return
/// [`ArmResult::NotPorted`] rather than pretending to act.
pub fn do_job<W: WorkWorld>(
    u: &mut UnitWork,
    w: &mut W,
    pf: &mut PathFinder,
    cov: &mut DispatchCoverage,
    kind: OrderIndex,
) -> ArmResult {
    let status = cov.record(kind);
    match kind {
        // Arm 0 is not a table entry: `do_job` calls the virtual at `[Unit+0x184]`
        // (`do_idle`). An idle unit holds position.
        OrderIndex::None => {
            u.idle = 1;
            ArmResult::Empty
        }
        // Arms 1 and 4 are the same jump-table entry.
        OrderIndex::MoveTo | OrderIndex::FleeTo => do_move(u, w, pf, cov),
        OrderIndex::AttackTo => do_attack_to(u, w, pf, cov),
        OrderIndex::ExploreTo => do_explore_to(u, w, pf, cov),
        OrderIndex::BuildAt => do_build_at(u, w, cov),
        OrderIndex::GroupMove => do_group_move(u, w, pf, cov),
        OrderIndex::GroupAttack => do_group_attack(u, w, pf, cov),
        OrderIndex::GroupAttackTo => do_group_attack_to(u, w, pf, cov),
        OrderIndex::Attack => do_attack(u, w, cov),
        OrderIndex::Gather => do_gather(u, w, cov),
        OrderIndex::Repair => do_repair(u, w, cov),
        OrderIndex::BoardShip => do_board(u, w, cov),
        OrderIndex::AwaitBoard => do_await_board(u, w, cov),
        OrderIndex::Follow => do_follow(u, w, pf, cov),
        OrderIndex::ChangeForm | OrderIndex::Think => do_terminal_order(u, w, cov, kind),
        OrderIndex::AirPatrol => do_air_patrol(u, w),
        OrderIndex::GroupPatrol => do_group_patrol(u, w),
        OrderIndex::AttackGround => do_attack_ground(u, w, cov),
        OrderIndex::AirAttackGround => do_air_attack_ground(u, w, cov),
        OrderIndex::SpecialAnim => do_special_anim(u, w, cov),
        OrderIndex::Guard => crate::systems::guard_dispatch::do_guard(u, w, pf, cov),
        OrderIndex::Garrison => crate::systems::garrison_dispatch::do_garrison(u, w, cov),
        // Arm 5 has no case label. Doing nothing here is faithful, not missing.
        OrderIndex::Patrol => ArmResult::Empty,
        _ => {
            debug_assert_eq!(status, ArmStatus::Unimplemented);
            ArmResult::NotPorted
        }
    }
}

fn do_terminal_order<W: WorkWorld>(
    u: &mut UnitWork,
    w: &mut W,
    cov: &mut DispatchCoverage,
    kind: OrderIndex,
) -> ArmResult {
    let Some(current) = u.orders.front().cloned() else {
        return ArmResult::NoOrder;
    };
    if current.kind != kind {
        return ArmResult::MalformedOrder;
    }
    let no_foreign_payload = current.follow.is_none()
        && current.special_anim.is_none()
        && matches!(&current.targeted_payload, TargetedOrderPayload::None)
        && matches!(&current.patrol_payload, PatrolPayload::None);
    if !no_foreign_payload {
        return ArmResult::MalformedOrder;
    }
    let actor = TerminalOrderActor {
        who: u.who,
        object: u.o,
        uid: u.uid,
    };
    let queue_before: Vec<_> = u.orders.iter().map(|order| order.kind).collect();
    let request = match kind {
        OrderIndex::ChangeForm => {
            let Some(form) = current.form_order else {
                return ArmResult::MalformedOrder;
            };
            if form.angle != current.angle {
                return ArmResult::MalformedOrder;
            }
            TerminalOrderRequest::ChangeForm {
                actor,
                order: ChangeFormOrderFacts {
                    angle: form.angle,
                    new_form: form.new_form,
                },
                queue_before,
            }
        }
        OrderIndex::Think => {
            if current.form_order.is_some() {
                return ArmResult::MalformedOrder;
            }
            TerminalOrderRequest::Think {
                actor,
                unit_type: u.ptype,
                queue_before,
            }
        }
        _ => unreachable!("only terminal arms call do_terminal_order"),
    };

    // A malformed or unavailable host is not allowed to leave the local actor half-mutated.
    // External rollback remains the WorkWorld transaction's responsibility.
    let before = u.clone();
    let receipt = w.apply_terminal_order_transaction(u, request.clone());
    if !receipt.validates(&request) {
        *u = before;
        return ArmResult::MalformedOrder;
    }
    if receipt.status != TerminalOrderStatus::Applied {
        *u = before;
        return ArmResult::HostUnavailable;
    }
    cov.completed += 1;
    ArmResult::Retired(KillReason::Completed)
}

// ---------------------------------------------------------------------------
// 10. `Unit::work`
// ---------------------------------------------------------------------------

/// The MSVC signed-modulo idiom `Unit::work` uses for its three periodic checks
/// [measured, `0x0060D1B3`]:
///
/// ```text
///   and eax, 0x8000001f     ; keep sign + low bits
///   jns +5
///   dec eax
///   or  eax, 0xffffffe0
///   inc eax
///   jne ...                 ; == 0 ?
/// ```
///
/// That is exactly `(frame + o) % period == 0` with C truncated-toward-zero semantics, which
/// is also Rust's `%`, so this is bit-identical and not an approximation. The phase is the
/// object's own index `o`, which is how the engine spreads per-unit work across frames.
#[inline]
pub fn phase_due(frame: i32, o: i16, period: i32) -> bool {
    (frame.wrapping_add(o as i32)) % period == 0
}

/// Why `Unit::work` returned before reaching `do_job`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EarlyOut {
    /// `recharging != 0`, the unit's type does not opt out, and there is no order at all.
    /// [measured, `0x0060D1F9` -> `0x0060D21x`]
    RechargingNoOrder,
    /// Same gate, but the head order is not a group order.
    RechargingNonGroupOrder,
}

/// What one `Unit::work` did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkReport {
    /// The `OrderIndex` handed to `do_job`, or `NONE` if it never got there.
    pub dispatched: OrderIndex,
    pub status: ArmStatus,
    pub result: ArmResult,
    pub early_out: Option<EarlyOut>,
    /// The `(frame + o) % 32 == 0` housekeeping ran.
    pub periodic_32: bool,
    /// The `(frame + o) % 16 == 0` action-path revalidation ran.
    pub periodic_16: bool,
    /// `check_target_path` changed the order list, so `work` re-read it.
    pub order_list_rescanned: bool,
}

/// `Unit::work()` `0x0060D180` (2,885 B), virtual slot `[Unit+0x188]`.
///
/// The driver. Structure, in retail's order [measured over the whole body]:
///
/// ```text
///  A  reset the cursor; order = current, type = order->get_type()
///  B  every 32nd frame (phase = o): if !(flags & 0x80) visible = 0; unit_masks &= ~4
///  C  if type != CAST_SPELL:
///       flags &= 0x7f
///       if (unit_masks & 0x100) && order->is_group() && type != EXPLORE_TO: unit_masks &= ~0x100
///       if recharging && !type_ignores_recharge:
///            if !order || !order->is_group(): RETURN            <-- early out
///  D  the caravan road arm (Caravan::build_road 0x0073DB10)     -- not ported
///  E  if type is MOVE_LIKE:
///       every 16th frame: act = update_action(); if act is targeted and not AWAIT_BOARD:
///            GUARD  -> every 64th frame: repath()
///            else   -> check_target_path(act); if it changed the list, re-read the order
///  F  tail: if type != NONE and (flags & 8): notify the leader, clear the bit
///  G  if a parked search exists and the order is not a move: clear_partial_path()
///  H  act = update_action(); if it names a stale uid: repath(); kill_current_order(0)
///  I  if safe: safe--
///  J  if type != NONE: idle = 0; maybe set unit_masks |= 0x1000
///  K  if collide_frame > frame - 4 and type is not MOVE_LIKE: detect_boat_collision()
///  L  do_job(type, order)                                       <-- 0x0060DAD4
///  M  the supply-spread nudge (ObjectsData::find_unit 0x0065CA80) -- not ported
///  N  unit_masks &= ~0x10
/// ```
///
/// Steps A, B, C, E, F, G, H, I, J, K(gate), L and N are ported. D and M are counted as
/// boundaries. The three modulo periods (32, 16, 64) and their `o` phase are the reason a
/// fixed-order scheduler and a fixed-phase scheduler both diverge from retail.
pub fn work<W: WorkWorld>(
    u: &mut UnitWork,
    w: &mut W,
    pf: &mut PathFinder,
    cov: &mut DispatchCoverage,
) -> WorkReport {
    cov.work_calls += 1;
    let frame = w.frame();

    // --- A ---
    let mut order = update_order(u);
    let mut kind = order.as_ref().map_or(OrderIndex::None, |o| o.kind);

    // --- B ---
    let periodic_32 = phase_due(frame, u.o, 32);
    if periodic_32 {
        if (u.flags & obj_flags::CASTING) == 0 {
            u.visible = 0;
        }
        u.unit_masks &= !masks::PERIODIC_32;
    }

    // --- C ---
    if kind != OrderIndex::CastSpell {
        u.flags &= !obj_flags::CASTING;
        if (u.unit_masks & masks::GROUP_PENDING) != 0
            && order.as_ref().is_some_and(|o| o.is_group())
            && kind != OrderIndex::ExploreTo
        {
            u.unit_masks &= !masks::GROUP_PENDING;
        }
        if u.recharging != 0 && !u.type_ignores_recharge {
            match order {
                None => {
                    cov.early_outs += 1;
                    return WorkReport {
                        dispatched: OrderIndex::None,
                        status: ArmStatus::Implemented,
                        result: ArmResult::NoOrder,
                        early_out: Some(EarlyOut::RechargingNoOrder),
                        periodic_32,
                        periodic_16: false,
                        order_list_rescanned: false,
                    };
                }
                Some(o) if !o.is_group() => {
                    cov.early_outs += 1;
                    return WorkReport {
                        dispatched: OrderIndex::None,
                        status: ArmStatus::Implemented,
                        result: ArmResult::Working,
                        early_out: Some(EarlyOut::RechargingNonGroupOrder),
                        periodic_32,
                        periodic_16: false,
                        order_list_rescanned: false,
                    };
                }
                _ => {}
            }
        }
    }

    // --- E ---
    let mut periodic_16 = false;
    let mut rescanned = false;
    if is_move_like(kind) && (!u.type_snap_arm || (u.unit_masks & masks::SNAP_DEST) != 0) {
        periodic_16 = phase_due(frame, u.o, 16);
        if periodic_16 {
            if let Some(act) = update_action(u) {
                if act.is_targeted() && act.kind != OrderIndex::AwaitBoard {
                    if act.kind == OrderIndex::Guard {
                        if phase_due(frame, u.o, 64) {
                            repath(u);
                            rescanned = true;
                        }
                    } else if check_target_path(u, w, &act) {
                        rescanned = true;
                    }
                }
            }
        }
        if rescanned {
            order = update_order(u);
            kind = order.as_ref().map_or(OrderIndex::None, |o| o.kind);
        }
    }

    // --- F ---
    if kind != OrderIndex::None && (u.flags & obj_flags::NOTIFY_LEADER) != 0 {
        u.flags &= !obj_flags::NOTIFY_LEADER;
    }

    // --- G ---
    if u.parked_search && order.as_ref().is_some_and(|o| !o.is_move()) {
        clear_partial_path(u);
    }

    // --- H --- the stale-target pass, `0x0060D8xx`
    if let Some(act) = update_action(u) {
        if !act.is_targeted() {
            if kind != OrderIndex::CastSpell {
                u.spell_time = 0;
            }
        } else {
            if kind != OrderIndex::CastSpell && act.kind != OrderIndex::CastSpell && u.who < 8 {
                u.spell_time = 0;
            }
            if act.target_o >= 0 && act.target_who >= 0 {
                let stale = match w.target(act.target_who, act.target_o) {
                    Some(t) => t.uid != act.target_uid,
                    None => true,
                };
                if stale {
                    // `0x0060D948`: repath then kill -- the canonical failure pair.
                    cov.failed += abort_order_sequence(u) as u64;
                    order = update_order(u);
                    kind = order.map_or(OrderIndex::None, |o| o.kind);
                    rescanned = true;
                }
            }
        }
    } else if kind != OrderIndex::CastSpell {
        u.spell_time = 0;
    }

    // --- I ---
    if u.safe != 0 {
        // Retail executes a byte-sized `dec`; the checksum-visible field wraps through
        // `0x80 -> 0x7f` instead of trapping in a checked Rust build.
        u.safe = u.safe.wrapping_sub(1);
    }

    // --- J ---
    if kind != OrderIndex::None {
        u.idle = 0;
        if !u.type_blocks_work
            && u.type_wants_work
            && (u.unit_masks & masks::NO_AUTO_WORK) == 0
            && (u.unit_masks2 & 0x8000) == 0
        {
            u.unit_masks |= masks::WANTS_WORK;
        }
    }

    // --- K --- the gate only; `Unit::detect_boat_collision` `0x005FA8B0` is not ported.
    if u.collide_frame > frame - 4 && !is_move_like(kind) {
        cov.boat_collision_skipped += 1;
    }

    // --- L --- `0x0060DAD4: push edi; push esi; mov ecx, ebx; call 0x617a10`
    let result = do_job(u, w, pf, cov, kind);
    let status = ARMS[kind.index()];

    // --- N --- the last instruction of the function.
    u.unit_masks &= !masks::WORKED_THIS_FRAME;

    WorkReport {
        dispatched: kind,
        status,
        result,
        early_out: None,
        periodic_32,
        periodic_16,
        order_list_rescanned: rescanned,
    }
}

/// `Unit::check_target_path(TargetOrder*)` `0x005E22D0` (1,616 B).
///
/// Returns non-zero when it **changed the order list**, which is why `Unit::work` re-reads
/// the head order afterwards. The reachable structure [measured]:
///
/// ```text
///   if (t->ox < 0) return 0
///   obj = objects[t->whom][t->ox]
///   if (!obj->is_seen(this->who)) return 0
///   switch (order->get_type()):
///     CAST_SPELL: if the target is inactive or no longer a legal spell target:
///                     repath(); kill_current_order(0); return 1
///     BOARD_SHIP: return 0                       -- boarding never re-paths
///     REPAIR with the target already under repair: return 0
///     otherwise:  if the target has moved out of the pathed corridor: repath(); return 1
/// ```
///
/// The "moved out of the corridor" test in retail is a `find_angle` / `is_in_range` pair over
/// the target's current position; this port reduces it to "the target moved further than the
/// unit's tolerance from where the order recorded it" and marks that **UNVERIFIED**.
pub fn check_target_path<W: WorkWorld>(u: &mut UnitWork, w: &W, act: &OrderRec) -> bool {
    if act.target_o < 0 || act.target_who < 0 {
        return false;
    }
    let Some(t) = w.target(act.target_who, act.target_o) else {
        abort_order_sequence(u);
        return true;
    };
    if !t.seen {
        return false;
    }
    match act.kind {
        OrderIndex::CastSpell => {
            if !t.active {
                // `0x005E2389`: `repath(); kill_current_order(0)`.
                abort_order_sequence(u);
                return true;
            }
            false
        }
        OrderIndex::BoardShip => false,
        OrderIndex::Repair => false,
        _ => {
            let drift = vector_dist(t.x - act.x, t.y - act.y);
            if (act.x != 0 || act.y != 0) && drift > u.tolerance.max(movement::UCELL) {
                repath(u);
                return true;
            }
            false
        }
    }
}

// ---------------------------------------------------------------------------
// 11. The bridge to `crate::order::OrderList`
// ---------------------------------------------------------------------------

/// Widen a [`crate::order::OrderList`] — which is what [`crate::world::World`] stores, one per
/// unit row — into the executable [`OrderQueue`].
///
/// This is the whole integration surface. A host that already holds `World` can run the
/// derived driver over a row with:
///
/// ```ignore
/// let mut u = /* UnitWork built from the row's UnitCols fields */;
/// u.orders = order_dispatch::adopt(world.orders(row));
/// order_dispatch::work(&mut u, &mut world_adapter, &mut pathfinder, &mut cov);
/// order_dispatch::publish(&u.orders, world.orders_mut(row));
/// ```
///
/// What it cannot supply is the `world_adapter`: [`WorkWorld`] extends
/// [`movement::UnitWorld`], and `World` has no `invalid_loc` / `unit_collides` / `tregion`
/// today — those belong to the terrain and collision lanes. That gap, not this bridge, is
/// what stands between this module and `World::step`.
///
/// The `MoveOrder`/`GroupMoveOrder` scalar image is lossless through
/// [`crate::order::MoveOrderState`]. AIR_PATROL's dynamic arrays and walked secondary base
/// are likewise lossless through `air_runtime_authority::AirPatrolOrderPayload`. Concrete
/// attack, group-patrol, guard, and other payloads are still outside this narrow bridge;
/// callers must not publish those classes until their typed [`crate::order::Order`] variants land.
/// Exact target identity is retained by `target_uid` plus `target_handle`; a legacy target
/// order which lacks a Handle still cannot grow one during widening. Widening a newly issued
/// ATTACK_GROUND or
/// AIR_ATTACK_GROUND order does create the correct zero-initialized concrete payload. Keep
/// [`OrderQueue`] as the owning representation and use this only at the boundary.
pub fn adopt(list: &crate::order::OrderList) -> OrderQueue {
    let mut q = OrderQueue::new();
    for o in list.iter() {
        q.push_back(OrderRec::from(o.clone()));
    }
    q
}

/// The inverse of [`adopt`]. Overwrites `list` with the queue's contents, front first.
pub fn publish(q: &OrderQueue, list: &mut crate::order::OrderList) {
    list.clear();
    for o in q.iter() {
        list.push(Order::from(o.clone()));
    }
}

// ---------------------------------------------------------------------------
// 12. Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::order::{EXECUTORS, ORDER_MORE_WORK};

    /// An open map with optional blocked tiles and bodies, plus a tiny object table.
    struct TestWorld {
        tiles: i32,
        frame: i32,
        blocked: Vec<(i32, i32)>,
        bodies: Vec<(i32, i32)>,
        objects: Vec<((i32, i32), TargetState)>,
        attack: AttackOutcome,
        gather: GatherOutcome,
        draws: u32,
        air_physics: bool,
        air_target: Option<AirPatrolTarget>,
        building_target: Option<AirPatrolTarget>,
        last_air_destination: Option<(i32, i32)>,
        group_moves: Vec<GroupMoveRequest>,
        group_plan: Result<GroupMovePlan, GroupMoveHostError>,
        group_effects: Vec<GroupMoveEffect>,
        group_post: GroupMovePostStep,
        group_attack_preflight: Result<(), GroupAttackToHostError>,
        group_attack_calls_fight: bool,
        group_attack_pause_gate: bool,
        group_attack_events: Vec<&'static str>,
        group_attack_fight_result: ArmResult,
        group_attack_plan: Result<GroupAttackPlan, GroupAttackHostError>,
        group_attack_effects: Vec<GroupAttackEffect>,
        group_attack_scratch_seen: Vec<Option<(i32, i32)>>,
        group_attack_angles_seen: Vec<(i32, i32)>,
        attack_to_preflight: Result<(), AttackToHostError>,
        attack_to_post: AttackToPostMove,
        attack_to_events: Vec<&'static str>,
        attack_to_target: Option<(i32, i32, u16)>,
        build_at_preflight: Option<Result<BuildAtPreflightInput, BuildAtHostError>>,
        build_at_effects: Vec<BuildAtEffect>,
        build_at_events: Vec<&'static str>,
        build_at_effect_fronts: Vec<Option<OrderIndex>>,
        build_at_construct: BuildAtConstructResult,
        repair_preflight: Option<Result<RepairHostReceipt, RepairHostError>>,
        repair_effects: Vec<repair_order::RepairEffect>,
        repair_events: Vec<&'static str>,
        repair_claimed_effects: Option<usize>,
        follow_receipt: Option<FollowExecutorReceipt>,
        follow_anims: Vec<(i32, i32, i32)>,
        scrambled: Vec<i16>,
    }

    impl TestWorld {
        fn open(tiles: i32) -> TestWorld {
            TestWorld {
                tiles,
                frame: 0,
                blocked: vec![],
                bodies: vec![],
                objects: vec![],
                attack: AttackOutcome::Fired(7),
                gather: GatherOutcome::Yield(3),
                draws: 0,
                air_physics: true,
                air_target: None,
                building_target: None,
                last_air_destination: None,
                group_moves: vec![],
                group_plan: Err(GroupMoveHostError::Unavailable),
                group_effects: vec![],
                group_post: GroupMovePostStep::Continue,
                group_attack_preflight: Err(GroupAttackToHostError::Unavailable),
                group_attack_calls_fight: false,
                group_attack_pause_gate: false,
                group_attack_events: vec![],
                group_attack_fight_result: ArmResult::Working,
                group_attack_plan: Err(GroupAttackHostError::Unavailable),
                group_attack_effects: vec![],
                group_attack_scratch_seen: vec![],
                group_attack_angles_seen: vec![],
                attack_to_preflight: Err(AttackToHostError::Unavailable),
                attack_to_post: AttackToPostMove::HoldForArmy,
                attack_to_events: vec![],
                attack_to_target: None,
                build_at_preflight: None,
                build_at_effects: vec![],
                build_at_events: vec![],
                build_at_effect_fronts: vec![],
                build_at_construct: BuildAtConstructResult::Progressed {
                    credited: 1,
                    started_this_call: false,
                },
                repair_preflight: None,
                repair_effects: vec![],
                repair_events: vec![],
                repair_claimed_effects: None,
                follow_receipt: None,
                follow_anims: vec![],
                scrambled: vec![],
            }
        }
        fn with_object(mut self, who: i32, o: i32, t: TargetState) -> TestWorld {
            self.objects.push(((who, o), t));
            self
        }
    }

    impl UnitWorld for TestWorld {
        fn tiles_w(&self) -> i32 {
            self.tiles
        }
        fn tiles_h(&self) -> i32 {
            self.tiles
        }
        fn wcells_w(&self) -> i32 {
            (self.tiles / 4).max(1)
        }
        fn invalid_loc(&self, tx: i32, ty: i32) -> bool {
            self.blocked.contains(&(tx, ty))
        }
        fn unit_collides(&self, x: i32, y: i32) -> bool {
            self.bodies
                .iter()
                .any(|&(bx, by)| (bx - x).abs() < 24 && (by - y).abs() < 24)
        }
        fn needs_transport(&self, _: i32, _: i32, _: i32, _: i32) -> i32 {
            0
        }
        fn tregion(&self, _: i32, _: i32) -> i32 {
            0
        }
    }

    impl WorkWorld for TestWorld {
        fn frame(&self) -> i32 {
            self.frame
        }
        fn target(&self, who: i32, o: i32) -> Option<TargetState> {
            self.objects
                .iter()
                .find(|((a, b), _)| *a == who && *b == o)
                .map(|(_, t)| *t)
        }
        fn attack(&mut self, _: &UnitWork, _: &OrderRec) -> AttackOutcome {
            self.attack
        }
        fn gather(&mut self, _: &UnitWork, _: &OrderRec) -> GatherOutcome {
            self.gather
        }
        fn draw_path_retry_delay(&mut self) -> i32 {
            self.draws += 1;
            6
        }
        fn follow_preflight(
            &mut self,
            _: &UnitWork,
            request: FollowExecutorRequest,
        ) -> FollowExecutorReceipt {
            self.follow_receipt
                .clone()
                .unwrap_or_else(|| FollowExecutorReceipt::unavailable(request))
        }
        fn follow_set_anim(&mut self, _: &mut UnitWork, anim: i32, mode: i32, choose: i32) {
            self.follow_anims.push((anim, mode, choose));
        }
        fn patrol_think_bird(&mut self, _: &mut UnitWork, _: &mut AirPatrolOrder) {}
        fn air_patrol_physics(
            &mut self,
            _: &mut UnitWork,
            _: &mut AirPatrolOrder,
            target_x: i32,
            target_y: i32,
        ) -> bool {
            self.last_air_destination = Some((target_x, target_y));
            self.air_physics
        }
        fn air_patrol_unit_target(
            &mut self,
            _: &UnitWork,
            _: &AirPatrolOrder,
            _: i32,
            _: i32,
            _: AirPatrolSearch,
        ) -> Option<AirPatrolTarget> {
            self.air_target
        }
        fn air_patrol_building_target(
            &mut self,
            _: &UnitWork,
            _: &AirPatrolOrder,
            _: i32,
            _: i32,
        ) -> Option<AirPatrolTarget> {
            self.building_target
        }
        fn group_patrol_move(&mut self, _: &mut UnitWork, request: GroupMoveRequest) {
            self.group_moves.push(request);
        }
        fn patrol_actor_is_type(&self, actor: &UnitWork, type_id: i32, _: bool) -> bool {
            actor.ptype == type_id
        }
        fn patrol_inside_is_scramblable(&self, _: u8, inside_o: i16) -> bool {
            inside_o >= 0
        }
        fn patrol_scramble_inside(&mut self, _: &mut UnitWork, inside_o: i16) {
            self.scrambled.push(inside_o);
        }
        fn group_move_preflight(
            &mut self,
            _: &UnitWork,
            _: &OrderRec,
        ) -> Result<GroupMovePlan, GroupMoveHostError> {
            self.group_plan.clone()
        }
        fn group_move_effect(&mut self, _: &mut UnitWork, _: &OrderRec, effect: GroupMoveEffect) {
            self.group_effects.push(effect);
        }
        fn group_move_post_step(
            &mut self,
            _: &UnitWork,
            _: &OrderRec,
            _: ArmResult,
        ) -> GroupMovePostStep {
            self.group_post
        }
        fn group_attack_to_preflight(
            &mut self,
            _: &UnitWork,
            _: &OrderRec,
        ) -> Result<(), GroupAttackToHostError> {
            self.group_attack_events.push("preflight");
            self.group_attack_preflight
        }
        fn group_attack_to_calls_fight(&mut self, _: &UnitWork, _: &OrderRec) -> bool {
            self.group_attack_events.push("predicate");
            self.group_attack_calls_fight
        }
        fn group_attack_to_fight(&mut self, _: &mut UnitWork, _: &OrderRec) -> ArmResult {
            self.group_attack_events.push("fight");
            self.group_attack_fight_result
        }
        fn group_attack_to_pause_gate(&mut self, _: &UnitWork, _: &OrderRec) -> bool {
            self.group_attack_events.push("attack_to_pause_gate");
            self.group_attack_pause_gate
        }
        fn attack_to_preflight(
            &mut self,
            _: &UnitWork,
            _: &OrderRec,
        ) -> Result<(), AttackToHostError> {
            self.attack_to_events.push("preflight");
            self.attack_to_preflight
        }
        fn attack_to_post_move(&mut self, _: &UnitWork, _: &OrderRec) -> AttackToPostMove {
            self.attack_to_events.push("post_move");
            self.attack_to_post
        }
        fn attack_to_find_melee_target(&mut self, actor: &mut UnitWork, _: &OrderRec) {
            self.attack_to_events.push("find_melee_target");
            if let Some((who, o, uid)) = self.attack_to_target {
                actor.orders.push_front(OrderRec::attack(who, o, uid));
            }
        }
        fn build_at_preflight(
            &mut self,
            _: &UnitWork,
            _: &OrderRec,
        ) -> Result<BuildAtPreflightInput, BuildAtHostError> {
            self.build_at_events.push("preflight");
            self.build_at_preflight
                .unwrap_or(Err(BuildAtHostError::Unavailable))
        }
        fn build_at_effect(
            &mut self,
            actor: &mut UnitWork,
            order: &OrderRec,
            effect: BuildAtEffect,
        ) {
            self.build_at_effect_fronts
                .push(actor.orders.front().map(|current| current.kind));
            self.build_at_effects.push(effect);
            match effect {
                BuildAtEffect::SetAnimation { .. } => {
                    self.build_at_events.push("animation");
                }
                BuildAtEffect::SetAngle { angle } => {
                    self.build_at_events.push("set_angle");
                    actor.body.angle = angle;
                    actor.lead_guy.angle = angle;
                }
                BuildAtEffect::Reswarm { .. } => {
                    self.build_at_events.push("reswarm");
                    actor.orders.push_front(order.clone());
                    update_action(actor);
                }
                BuildAtEffect::FinishTail { .. } => {
                    self.build_at_events.push("finish_tail");
                }
            }
        }
        fn build_at_construct(&mut self, _: &mut UnitWork, _: &OrderRec) -> BuildAtConstructResult {
            self.build_at_events.push("construct");
            self.build_at_construct
        }
        fn repair_preflight(
            &mut self,
            _: &UnitWork,
            _: &OrderRec,
        ) -> Result<RepairHostReceipt, RepairHostError> {
            self.repair_events.push("preflight");
            self.repair_preflight
                .clone()
                .unwrap_or(Err(RepairHostError::Unavailable))
        }
        fn repair_commit(
            &mut self,
            actor: &mut UnitWork,
            _: &OrderRec,
            effects: &[repair_order::RepairEffect],
            receipt: &RepairHostReceipt,
        ) -> RepairCommitReceipt {
            self.repair_events.push("commit");
            self.repair_effects.extend_from_slice(effects);
            for effect in effects {
                if matches!(
                    effect,
                    repair_order::RepairEffect::KillCurrentOrder { arg: 0 }
                ) {
                    kill_current_order(actor, KillReason::Completed);
                }
            }
            RepairCommitReceipt {
                snapshot_version: receipt.snapshot_version,
                actor_who: receipt.actor_who,
                actor_o: receipt.actor_o,
                actor_uid: receipt.actor_uid,
                target: receipt.facts.target,
                target_uid: receipt.target_uid,
                committed_effects: self.repair_claimed_effects.unwrap_or(effects.len()),
            }
        }
        fn group_attack_preflight(
            &mut self,
            _: &UnitWork,
            _: &OrderRec,
        ) -> Result<GroupAttackPlan, GroupAttackHostError> {
            self.group_attack_plan
        }
        fn group_attack_effect(
            &mut self,
            actor: &mut UnitWork,
            _: &OrderRec,
            effect: GroupAttackEffect,
        ) {
            self.group_attack_effects.push(effect);
            self.group_attack_angles_seen
                .push((actor.body.angle, actor.lead_guy.angle));
            self.group_attack_scratch_seen.push(
                actor
                    .orders
                    .front()
                    .map(|order| (order.group_attack_oxxx, order.group_attack_whosoever)),
            );
        }
        fn boarding_set_anim(&mut self, _: &mut UnitWork, _: i32, _: i32, _: i32) {}
        fn board_check_meet_ship(
            &mut self,
            _: &mut UnitWork,
            _: crate::systems::naval::TargetRef,
        ) -> bool {
            // The table-wide smoke test should hold BOARD_SHIP without demanding a carrier.
            true
        }
        fn boarding_can_carry(
            &mut self,
            _: &UnitWork,
            _: crate::systems::naval::TargetRef,
            _: crate::systems::naval::TargetRef,
        ) -> bool {
            false
        }
        fn board_go_inside(
            &mut self,
            _: &mut UnitWork,
            _: crate::systems::naval::TargetRef,
            _: i32,
        ) {
            unreachable!("TestWorld never admits a boarding carrier")
        }
        fn boarding_target_is_live_unit(&mut self, _: crate::systems::naval::TargetRef) -> bool {
            false
        }
        fn boarding_target_action(
            &mut self,
            _: crate::systems::naval::TargetRef,
        ) -> Option<BoardingAction> {
            None
        }
        fn boarding_abort_passenger(&mut self, _: &UnitWork, _: crate::systems::naval::TargetRef) {
            unreachable!("TestWorld never resolves a boarding passenger")
        }
    }

    fn live(t: TargetState) -> TargetState {
        t
    }

    fn tgt(x: i32, y: i32, uid: u16) -> TargetState {
        live(TargetState {
            x,
            y,
            uid,
            active: true,
            seen: true,
        })
    }

    // -- the tables --------------------------------------------------------

    #[test]
    fn arm_table_is_index_aligned_with_the_jump_table() {
        assert_eq!(ARMS.len(), NUM_UNIT_ORDERS);
        assert_eq!(EXECUTORS.len(), NUM_UNIT_ORDERS);
        for (i, e) in EXECUTORS.iter().enumerate() {
            assert_eq!(e.order.index(), i, "EXECUTORS misaligned at {i}");
        }
        // The two derived surprises, re-asserted here so this module fails too if either
        // ever "gets fixed".
        assert_eq!(
            EXECUTORS[OrderIndex::FleeTo.index()].va,
            EXECUTORS[OrderIndex::MoveTo.index()].va
        );
        assert_eq!(EXECUTORS[OrderIndex::Patrol.index()].va, None);
        assert_eq!(ARMS[OrderIndex::Patrol.index()], ArmStatus::FaithfullyEmpty);
        assert_eq!(
            ARMS[OrderIndex::FleeTo.index()],
            ARMS[OrderIndex::MoveTo.index()]
        );
    }

    #[test]
    fn this_dispatcher_handles_twenty_four_of_the_twenty_eight_arms() {
        let implemented = ARMS
            .iter()
            .filter(|s| **s == ArmStatus::Implemented)
            .count();
        let empty = ARMS
            .iter()
            .filter(|s| **s == ArmStatus::FaithfullyEmpty)
            .count();
        let absent = ARMS
            .iter()
            .filter(|s| **s == ArmStatus::Unimplemented)
            .count();
        // Twenty-three implemented; PATROL's missing jump-table arm is faithfully empty.
        // GUARD (12) and GARRISON (26) joined on 2026-08-11. The four this dispatcher still
        // does not carry are CAST_SPELL (14), TRADE_ROUTE (15), STRAFE (16) and
        // SPECIAL_ANIM (25) — the last of which dispatches but stays red because its
        // production host cannot serve most branches.
        assert_eq!((implemented, empty, absent), (23, 1, 4));
        assert_eq!(implemented + empty + absent, NUM_UNIT_ORDERS);
    }

    // -- the queue ---------------------------------------------------------

    #[test]
    fn front_is_current_and_the_cursor_walks_backwards_to_the_tail() {
        let mut q = OrderQueue::new();
        q.push_back(OrderRec::move_to(100, 0, 4));
        q.push_back(OrderRec::attack(1, 2, 9));
        q.push_back(OrderRec::of_kind(OrderIndex::Guard));
        q.reset();
        assert_eq!(q.current().unwrap().kind, OrderIndex::MoveTo);
        assert!(!q.at_tail());
        assert!(q.advance());
        assert_eq!(q.current().unwrap().kind, OrderIndex::Attack);
        assert!(q.advance());
        assert_eq!(q.current().unwrap().kind, OrderIndex::Guard);
        assert!(q.at_tail());
        assert!(!q.advance());
        assert_eq!(q.len(), 3);
    }

    #[test]
    fn replace_is_an_unshifted_command_and_push_back_is_a_shifted_one() {
        let mut q = OrderQueue::new();
        q.push_back(OrderRec::move_to(1, 1, 0));
        q.push_back(OrderRec::move_to(2, 2, 0));
        assert_eq!(q.len(), 2);
        q.replace(OrderRec::attack(0, 0, 1));
        assert_eq!(q.len(), 1);
        assert_eq!(q.front().unwrap().kind, OrderIndex::Attack);
    }

    #[test]
    fn patrol_queue_positions_follow_the_two_retail_exceptions() {
        let mut ground = UnitWork::at(1, 4, 24, 24);
        ground
            .orders
            .push_back(OrderRec::of_kind(OrderIndex::Guard));
        assert_eq!(
            install_group_patrol(&mut ground, 24, 24, 120, 72, 0, 0, 4, 1, QueuePos::First,),
            PatrolInstall::Replaced
        );
        assert_eq!(ground.orders.len(), 1, "QUEUE_FIRST is normalized to NEW");
        assert_eq!(
            install_group_patrol(&mut ground, 120, 72, 300, 400, 0, 0, 4, 1, QueuePos::Last,),
            PatrolInstall::ExtendedWaypoints
        );
        let PatrolPayload::Group(g) = &ground.orders.front().unwrap().patrol_payload else {
            panic!("missing group patrol payload");
        };
        assert_eq!(g.points.x, vec![24, 120, 300]);

        let mut air = UnitWork::at(1, 5, 0, 0);
        air.orders.push_back(OrderRec::of_kind(OrderIndex::Guard));
        assert_eq!(
            install_air_patrol(&mut air, 500, 600, -1, -1, None, false, QueuePos::Last,),
            PatrolInstall::Replaced,
            "add_air_patrol_order ignores QueuePos when no patrol can be extended"
        );
        assert_eq!(air.orders.len(), 1);
        assert_eq!(
            install_air_patrol(&mut air, 700, 800, -1, -1, None, false, QueuePos::Last,),
            PatrolInstall::ExtendedWaypoints
        );
        let PatrolPayload::Air(a) = &air.orders.front().unwrap().patrol_payload else {
            panic!("missing air patrol payload");
        };
        assert_eq!(a.points.x, vec![500, 700]);
    }

    #[test]
    fn remove_current_pops_the_front_after_a_reset() {
        let mut q = OrderQueue::new();
        q.push_back(OrderRec::move_to(1, 1, 0));
        q.push_back(OrderRec::attack(0, 0, 1));
        q.reset();
        let gone = q.remove_current().unwrap();
        assert_eq!(gone.kind, OrderIndex::MoveTo);
        assert_eq!(q.front().unwrap().kind, OrderIndex::Attack);
    }

    // -- update_action -----------------------------------------------------

    #[test]
    fn update_action_skips_move_legs_and_reports_where_the_unit_will_stand() {
        let mut u = UnitWork::at(0, 0, 0, 0);
        let mut a = OrderRec::move_to(400, 300, 4);
        a.angle = 0x1234;
        u.orders.push_back(a);
        u.orders.push_back(OrderRec::move_to(800, 600, 4));
        u.orders.push_back(OrderRec::gather(1, 5, 77));
        let act = update_action(&mut u).expect("an action survives the move legs");
        assert_eq!(act.kind, OrderIndex::Gather);
        // orders_x/orders_y took the LAST move leg's destination.
        assert_eq!((u.orders_x, u.orders_y), (800, 600));
    }

    #[test]
    fn update_action_on_a_pure_move_queue_reports_no_action_but_still_sets_the_endpoint() {
        let mut u = UnitWork::at(0, 0, 10, 20);
        u.orders.push_back(OrderRec::move_to(400, 300, 4));
        assert_eq!(update_action(&mut u), None);
        assert_eq!((u.orders_x, u.orders_y), (400, 300));
    }

    #[test]
    fn update_action_on_an_empty_queue_reports_the_units_own_position() {
        let mut u = UnitWork::at(0, 0, 33, 44);
        u.body.angle = 0x99;
        assert_eq!(update_action(&mut u), None);
        assert_eq!((u.orders_x, u.orders_y, u.dest_angle), (33, 44, 0x99));
    }

    // -- repath / kill -----------------------------------------------------

    #[test]
    fn repath_strips_leading_movement_orders_and_stops_at_the_action() {
        let mut u = UnitWork::at(0, 0, 0, 0);
        u.orders.push_back(OrderRec::move_to(1, 1, 0));
        u.orders.push_back(OrderRec::of_kind(OrderIndex::ExploreTo));
        u.orders
            .push_back(OrderRec::of_kind(OrderIndex::GroupAttackTo));
        u.orders.push_back(OrderRec::attack(1, 2, 3));
        u.orders.push_back(OrderRec::move_to(9, 9, 0));
        let stripped = repath(&mut u);
        assert_eq!(stripped, 3);
        assert_eq!(u.order_type(), OrderIndex::Attack);
        // The trailing move behind the action is untouched: repath only strips the FRONT.
        assert_eq!(u.orders.len(), 2);
    }

    #[test]
    fn repath_detaches_a_grouped_unit_before_killing_a_group_move() {
        let mut u = UnitWork::at(0, 0, 0, 0);
        u.group = 4;
        u.orders.push_back(OrderRec::of_kind(OrderIndex::GroupMove));
        u.orders.push_back(OrderRec::attack(0, 0, 1));
        let stripped = repath(&mut u);
        assert_eq!(u.group, -1);
        assert_eq!(stripped, 1);
        assert_eq!(u.order_type(), OrderIndex::Attack);
    }

    #[test]
    fn a_failure_strips_the_walk_and_the_thing_it_was_walking_to() {
        // The `repath(); kill_current_order(0)` pair from `0x0060D948`.
        let mut u = UnitWork::at(0, 0, 0, 0);
        u.orders.push_back(OrderRec::move_to(1, 1, 0));
        u.orders.push_back(OrderRec::move_to(2, 2, 0));
        u.orders.push_back(OrderRec::gather(1, 1, 5));
        u.orders.push_back(OrderRec::of_kind(OrderIndex::Guard));
        let removed = abort_order_sequence(&mut u);
        assert_eq!(removed, 3, "two move legs plus the action");
        assert_eq!(u.order_type(), OrderIndex::Guard);
        assert_eq!(u.orders.len(), 1);
    }

    #[test]
    fn a_completion_retires_exactly_one_order() {
        let mut u = UnitWork::at(0, 0, 0, 0);
        u.orders.push_back(OrderRec::move_to(1, 1, 0));
        u.orders.push_back(OrderRec::move_to(2, 2, 0));
        u.orders.push_back(OrderRec::gather(1, 1, 5));
        kill_current_order(&mut u, KillReason::Completed);
        assert_eq!(u.orders.len(), 2);
        assert_eq!(u.order_type(), OrderIndex::MoveTo);
    }

    #[test]
    fn killing_a_gather_raises_the_leader_economy_notification() {
        let mut u = UnitWork::at(0, 0, 0, 0);
        u.orders.push_back(OrderRec::gather(1, 1, 5));
        kill_current_order(&mut u, KillReason::Completed);
        assert!(u.flags & obj_flags::NOTIFY_LEADER != 0);
    }

    #[test]
    fn killing_an_order_clears_the_parked_search_and_the_waypoints() {
        let mut u = UnitWork::at(0, 0, 0, 0);
        u.parked_search = true;
        u.path.push(PathData::default());
        u.orders.push_back(OrderRec::move_to(1, 1, 0));
        kill_current_order(&mut u, KillReason::Completed);
        assert!(!u.parked_search);
        assert!(u.path.is_empty());
    }

    #[test]
    fn follow_dispatch_is_zero_mutation_without_a_receipt_then_commits_the_idle_effect() {
        use crate::systems::follow_executor::{
            plan_follow_executor, FollowExecutorFacts, FollowObjectFacts,
        };

        let payload = FollowOrderPayload {
            ox: 7,
            whom: 3,
            uid: 70,
            oxx: 7,
            whose: 3,
            uid2: 70,
        };
        let mut u = UnitWork::at(2, 9, 0, 0);
        u.guy_env.unit_speed = 8;
        u.follow_los = 4;
        u.orders.push_back(OrderRec::follow(payload));
        let request = FollowExecutorRequest {
            actor: FollowActorFacts {
                o: 9,
                who: 2,
                x: 0,
                y: 0,
                speed: 8,
                los: 4,
            },
            order: follow_state(payload),
        };

        let mut w = TestWorld::open(16);
        let mut pf = PathFinder::new();
        let mut cov = DispatchCoverage::default();
        let before = u.orders.clone();
        assert_eq!(
            do_follow(&mut u, &mut w, &mut pf, &mut cov),
            ArmResult::HostUnavailable
        );
        assert_eq!(u.orders, before);

        let primary = FollowIdentity {
            o: 7,
            who: 3,
            uid: 70,
        };
        let facts = FollowExecutorFacts {
            primary: Some(FollowObjectFacts {
                identity: primary,
                valid_unit: true,
                on_map: true,
                active: true,
                inside_up: -1,
                seen_by_actor: true,
                x: 1_000,
                y: 0,
                angle: 0x2000_0000,
                speed: 4,
                is_moving: false,
                captain_o: 7,
            }),
            ..FollowExecutorFacts::default()
        };
        let plan = plan_follow_executor(&request, &facts).unwrap();
        w.follow_receipt = Some(FollowExecutorReceipt {
            request,
            status: FollowExecutorTransactionStatus::Applied,
            facts: Some(facts),
            plan: Some(plan),
        });
        assert_eq!(
            do_follow(&mut u, &mut w, &mut pf, &mut cov),
            ArmResult::Working
        );
        assert_eq!(u.orders.front().unwrap().follow, Some(payload));
        assert_eq!(w.follow_anims, vec![(0, 0, 1)]);
    }

    // -- the phase gates ---------------------------------------------------

    #[test]
    fn the_periodic_gates_are_phased_by_the_object_index() {
        // Two units with different `o` never do their 32-frame housekeeping on the same
        // frame unless their indices are congruent mod 32.
        assert!(phase_due(0, 0, 32));
        assert!(!phase_due(0, 1, 32));
        assert!(phase_due(31, 1, 32));
        assert!(phase_due(16, 0, 16));
        assert!(phase_due(64, 0, 64));
        // The MSVC idiom keeps C's truncated modulo, so a negative sum is still tested
        // against zero and only an exact multiple passes.
        assert!(phase_due(-32, 0, 32));
        assert!(!phase_due(-31, 0, 32));
    }

    // -- do_move -----------------------------------------------------------

    #[test]
    fn a_move_order_walks_and_then_retires_on_arrival() {
        let mut w = TestWorld::open(16);
        let mut pf = PathFinder::new();
        let mut cov = DispatchCoverage::default();
        let mut u = UnitWork::at(0, 0, 24, 24);
        u.myspeed = 48;
        u.tolerance = 24;
        u.orders.push_back(OrderRec::move_to(24 + 48 * 6, 24, 24));

        let mut moved = 0;
        let mut retired = false;
        for f in 0..400 {
            w.frame = f;
            let r = work(&mut u, &mut w, &mut pf, &mut cov);
            match r.result {
                ArmResult::Moved => moved += 1,
                ArmResult::Retired(KillReason::Completed) => {
                    retired = true;
                    break;
                }
                _ => {}
            }
            if u.orders.is_empty() {
                retired = true;
                break;
            }
        }
        assert!(moved > 0, "the unit never translated");
        assert!(retired, "the move order never retired");
        assert!(u.orders.is_empty());
        assert_eq!(cov.completed, 1);
        assert!(cov.dispatches[OrderIndex::MoveTo.index()] > 1);
        assert!(u.body.x > 24, "the unit did not advance along +x");
    }

    fn attack_to_order(x: i32, y: i32) -> OrderRec {
        let mut order = OrderRec::move_to(x, y, 0);
        order.kind = OrderIndex::AttackTo;
        order
    }

    #[test]
    fn attack_to_missing_host_prevents_all_movement_mutation() {
        let mut w = TestWorld::open(16);
        w.frame = 1; // non-phase still preflights future target-search capability
        let mut pf = PathFinder::new();
        let mut cov = DispatchCoverage::default();
        let mut u = UnitWork::at(0, 0, 100, 200);
        u.myspeed = 8;
        u.tolerance = 0;
        u.body.angle = movement::find_angle(40, 0);
        u.path.push(PathData {
            to_x: 140,
            to_y: 200,
            tolerance: 0,
            flags: PathData::FLAG_WAYPOINT,
        });
        u.orders.push_back(attack_to_order(140, 200));
        let before_body = u.body;
        let before_orders = u.orders.clone();
        let before_path = u.path.clone();

        assert_eq!(
            do_attack_to(&mut u, &mut w, &mut pf, &mut cov),
            ArmResult::HostUnavailable
        );
        assert_eq!(
            (u.body.x, u.body.y, u.body.angle, u.body.stuck_budget),
            (
                before_body.x,
                before_body.y,
                before_body.angle,
                before_body.stuck_budget
            )
        );
        assert_eq!(u.orders, before_orders);
        assert_eq!(u.path, before_path);
        assert_eq!(w.attack_to_events, vec!["preflight"]);

        w.attack_to_preflight = Err(AttackToHostError::InvalidState("stale target index"));
        w.attack_to_events.clear();
        assert_eq!(
            do_attack_to(&mut u, &mut w, &mut pf, &mut cov),
            ArmResult::MalformedOrder
        );
        assert_eq!(
            (u.body.x, u.body.y, u.body.angle, u.body.stuck_budget),
            (
                before_body.x,
                before_body.y,
                before_body.angle,
                before_body.stuck_budget
            )
        );
        assert_eq!(u.orders, before_orders);
        assert_eq!(u.path, before_path);
        assert_eq!(w.attack_to_events, vec!["preflight"]);
    }

    #[test]
    fn attack_to_non_phase_moves_after_capability_preflight_only() {
        let mut w = TestWorld::open(16);
        w.frame = 1;
        w.attack_to_preflight = Ok(());
        let mut pf = PathFinder::new();
        let mut cov = DispatchCoverage::default();
        let mut u = UnitWork::at(0, 0, 100, 200);
        u.myspeed = 8;
        u.tolerance = 0;
        u.body.angle = movement::find_angle(40, 0);
        u.path.push(PathData {
            to_x: 140,
            to_y: 200,
            tolerance: 0,
            flags: PathData::FLAG_WAYPOINT,
        });
        u.orders.push_back(attack_to_order(140, 200));

        assert!(matches!(
            do_attack_to(&mut u, &mut w, &mut pf, &mut cov),
            ArmResult::Moved | ArmResult::Turned | ArmResult::Working
        ));
        assert!(u.body.x > 100);
        assert_eq!(w.attack_to_events, vec!["preflight"]);
    }

    #[test]
    fn attack_to_phase_moves_then_runs_melee_selection_transition() {
        let mut w = TestWorld::open(16);
        w.frame = 0;
        w.attack_to_preflight = Ok(());
        w.attack_to_post = AttackToPostMove::FindMeleeTarget;
        w.attack_to_target = Some((1, 7, 99));
        let mut pf = PathFinder::new();
        let mut cov = DispatchCoverage::default();
        let mut u = UnitWork::at(0, 0, 100, 200);
        u.myspeed = 8;
        u.tolerance = 0;
        u.body.angle = movement::find_angle(40, 0);
        u.path.push(PathData {
            to_x: 140,
            to_y: 200,
            tolerance: 0,
            flags: PathData::FLAG_WAYPOINT,
        });
        u.orders.push_back(attack_to_order(140, 200));

        assert_eq!(
            do_attack_to(&mut u, &mut w, &mut pf, &mut cov),
            ArmResult::Working
        );
        assert!(u.body.x > 100, "do_move must precede target selection");
        assert_eq!(
            w.attack_to_events,
            vec!["preflight", "post_move", "find_melee_target"]
        );
        let attack = u
            .orders
            .front()
            .expect("host installed an ATTACK transition");
        assert_eq!(attack.kind, OrderIndex::Attack);
        assert_eq!(
            (attack.target_who, attack.target_o, attack.target_uid),
            (1, 7, 99)
        );
    }

    #[test]
    fn attack_to_pause_army_hold_and_retirement_preserve_post_move_ordering() {
        let mut w = TestWorld::open(16);
        w.frame = 0;
        w.attack_to_preflight = Ok(());
        w.attack_to_post = AttackToPostMove::Pause { set_pause: true };
        let mut pf = PathFinder::new();
        let mut cov = DispatchCoverage::default();
        let mut u = UnitWork::at(0, 0, 100, 200);
        u.myspeed = 8;
        u.tolerance = 0;
        u.body.angle = movement::find_angle(40, 0);
        u.orders.push_back(attack_to_order(140, 200));

        assert_eq!(
            do_attack_to(&mut u, &mut w, &mut pf, &mut cov),
            ArmResult::Working
        );
        assert_eq!(u.orders.front().unwrap().pause, 15);
        assert_eq!(w.attack_to_events, vec!["preflight", "post_move"]);

        w.attack_to_events.clear();
        w.attack_to_post = AttackToPostMove::HoldForArmy;
        let mut leashed = UnitWork::at(0, 0, 100, 200);
        leashed.myspeed = 8;
        leashed.tolerance = 0;
        leashed.body.angle = movement::find_angle(40, 0);
        leashed.orders.push_back(attack_to_order(140, 200));
        assert_eq!(
            do_attack_to(&mut leashed, &mut w, &mut pf, &mut cov),
            ArmResult::Working
        );
        assert!(
            leashed.body.x > 100,
            "the army leash is evaluated after do_move"
        );
        assert_eq!(leashed.orders.front().unwrap().pause, 0);
        assert_eq!(w.attack_to_events, vec!["preflight", "post_move"]);

        w.attack_to_events.clear();
        let mut arrived = UnitWork::at(0, 0, 100, 200);
        arrived.orders.push_back(attack_to_order(100, 200));
        arrived.orders.push_back(attack_to_order(300, 200));
        assert_eq!(
            do_attack_to(&mut arrived, &mut w, &mut pf, &mut cov),
            ArmResult::Retired(KillReason::Completed)
        );
        assert_eq!(arrived.orders.len(), 1);
        assert_eq!(arrived.orders.front().unwrap().x, 300);
        assert_eq!(w.attack_to_events, vec!["preflight"]);
    }

    fn repair_order() -> OrderRec {
        let mut order = OrderRec::of_kind(OrderIndex::Repair);
        order.flags = ORDER_GROUP;
        order.target_who = 2;
        order.target_o = 2_001;
        order.target_uid = 20;
        order
    }

    fn repair_facts(actor: &UnitWork, order: &OrderRec, frame: i32) -> repair_order::RepairFacts {
        repair_order::RepairFacts {
            repairer: repair_order::ObjectId {
                o: i32::from(actor.o),
                who: i32::from(actor.who),
            },
            target: repair_order::ObjectId {
                o: order.target_o,
                who: order.target_who,
            },
            more_work: order.flags & ORDER_GROUP != 0,
            repairer_unit_masks: actor.unit_masks,
            target_damage: 50,
            target_is_repairer_team: true,
            repairer_relation_to_target_is_two: false,
            team_relation_to_target_is_two: false,
            target_has_object_interface: false,
            target_build_active: true,
            target_under_attack: false,
            territory_owner: None,
            target_owner_allied_with_territory: false,
            repairer_in_range: true,
            target_has_repair_interface_after_range: false,
            repair_numerator: 1,
            target_hit_capacity: 100,
            repair_scale_constant: 1,
            target_helpers: 0,
            target_build_flags: 0,
            city_repair_state_mismatch: false,
            leader_has_korean_repair_bonus: false,
            korean_repair_percent: 0,
            korean_skips_damage_penalties: false,
            target_build_masks: 0,
            frame,
            target_repair_state: 0,
            resources: [repair_order::RepairResourceFacts::default(); repair_order::RESOURCE_COUNT],
            leader_repair_stamp: 0,
            repairer_owner_is_local_player: false,
            lost_target_fallback: repair_order::LostTargetFallbackFacts {
                repairer_order_type: 1,
                repairer_state_f8: 2,
                target_has_object_interface: false,
                target_type_allows_gather: false,
                target_is_university: false,
            },
        }
    }

    fn repair_receipt(actor: &UnitWork, order: &OrderRec, frame: i32) -> RepairHostReceipt {
        RepairHostReceipt {
            snapshot_version: 77,
            actor_who: actor.who,
            actor_o: actor.o,
            actor_uid: actor.uid,
            target_uid: order.target_uid,
            order: order.clone(),
            facts: repair_facts(actor, order, frame),
        }
    }

    #[test]
    fn repair_missing_or_foreign_snapshot_is_zero_mutation() {
        let mut world = TestWorld::open(16);
        let mut cov = DispatchCoverage::default();
        let mut actor = UnitWork::at(1, 4, 100, 200);
        actor.uid = 10;
        actor.orders.push_back(repair_order());
        let before = actor.orders.clone();

        assert_eq!(
            do_repair(&mut actor, &mut world, &mut cov),
            ArmResult::HostUnavailable
        );
        assert_eq!(actor.orders, before);
        assert!(world.repair_effects.is_empty());
        assert_eq!(world.repair_events, ["preflight"]);

        let order = actor.orders.front().unwrap().clone();
        let mut foreign = repair_receipt(&actor, &order, world.frame);
        foreign.target_uid ^= 1;
        world.repair_preflight = Some(Ok(foreign));
        world.repair_events.clear();
        assert_eq!(
            do_repair(&mut actor, &mut world, &mut cov),
            ArmResult::HostUnavailable
        );
        assert_eq!(actor.orders, before);
        assert!(world.repair_effects.is_empty());
        assert_eq!(world.repair_events, ["preflight"]);
    }

    #[test]
    fn repair_full_atomic_receipt_retires_only_after_all_effects_commit() {
        let mut world = TestWorld::open(16);
        let mut cov = DispatchCoverage::default();
        let mut actor = UnitWork::at(1, 4, 100, 200);
        actor.uid = 10;
        actor.orders.push_back(repair_order());
        let order = actor.orders.front().unwrap().clone();
        world.repair_preflight = Some(Ok(repair_receipt(&actor, &order, world.frame)));

        assert_eq!(
            do_repair(&mut actor, &mut world, &mut cov),
            ArmResult::Retired(KillReason::Completed)
        );
        assert!(actor.orders.is_empty());
        assert_eq!(world.repair_events, ["preflight", "commit"]);
        assert_eq!(
            world.repair_effects,
            [
                repair_order::RepairEffect::SetAnimation {
                    animation: repair_order::REPAIR_ANIM,
                    arg0: 0,
                    arg1: 1,
                },
                repair_order::RepairEffect::KillCurrentOrder { arg: 0 },
            ]
        );
        assert_eq!(cov.completed, 1);
    }

    #[test]
    #[should_panic(expected = "incomplete or foreign atomic commit receipt")]
    fn repair_partial_commit_claim_is_a_broken_host_contract() {
        let mut world = TestWorld::open(16);
        let mut cov = DispatchCoverage::default();
        let mut actor = UnitWork::at(1, 4, 100, 200);
        actor.uid = 10;
        actor.orders.push_back(repair_order());
        let order = actor.orders.front().unwrap().clone();
        world.repair_preflight = Some(Ok(repair_receipt(&actor, &order, world.frame)));
        world.repair_claimed_effects = Some(1);
        let _ = do_repair(&mut actor, &mut world, &mut cov);
    }

    fn build_at_order() -> OrderRec {
        let mut order = OrderRec::of_kind(OrderIndex::BuildAt);
        order.target_who = 2;
        order.target_o = 2_001;
        order.target_uid = 20;
        order
    }

    fn build_at_ready_input(actor: &UnitWork, order: &OrderRec) -> BuildAtPreflightInput {
        BuildAtPreflightInput {
            builder: ObjectKey {
                who: i32::from(actor.who),
                o: i32::from(actor.o),
                uid: actor.uid,
            },
            target_order: ObjectKey {
                who: order.target_who,
                o: order.target_o,
                uid: order.target_uid,
            },
            target_is_valid_wall: true,
            target_is_active: false,
            has_next_action_after_retire: actor.orders.len() > 1,
            adjacent: true,
            builder_tile_is_covered: false,
            target_is_farm: false,
            order_flags: order.flags,
            builder_x: actor.body.x,
            builder_y: actor.body.y,
            target_x: actor.body.x + 100,
            target_y: actor.body.y,
            builder_angle: actor.body.angle,
            unit_decoy: actor.unit_masks & 1 != 0,
        }
    }

    #[test]
    fn build_at_missing_or_incoherent_host_is_zero_mutation() {
        let mut w = TestWorld::open(16);
        let mut cov = DispatchCoverage::default();
        let mut actor = UnitWork::at(2, 4, 100, 200);
        actor.uid = 10;
        actor.body.angle = 123;
        actor.orders.push_back(build_at_order());
        let before_body = actor.body;
        let before_orders = actor.orders.clone();
        let before_masks = actor.unit_masks;

        assert_eq!(
            do_build_at(&mut actor, &mut w, &mut cov),
            ArmResult::HostUnavailable
        );
        assert_eq!(actor.orders, before_orders);
        assert_eq!(actor.body.x, before_body.x);
        assert_eq!(actor.body.y, before_body.y);
        assert_eq!(actor.body.angle, before_body.angle);
        assert_eq!(actor.unit_masks, before_masks);
        assert_eq!(w.build_at_events, vec!["preflight"]);

        let order = actor.orders.front().unwrap().clone();
        let mut incoherent = build_at_ready_input(&actor, &order);
        incoherent.builder.uid ^= 1;
        w.build_at_preflight = Some(Ok(incoherent));
        w.build_at_events.clear();
        assert_eq!(
            do_build_at(&mut actor, &mut w, &mut cov),
            ArmResult::MalformedOrder
        );
        assert_eq!(actor.orders, before_orders);
        assert_eq!(actor.body.angle, before_body.angle);
        assert_eq!(actor.unit_masks, before_masks);
        assert_eq!(w.build_at_events, vec!["preflight"]);
    }

    #[test]
    fn build_at_ready_animates_faces_then_credits_site_without_retiring() {
        let mut w = TestWorld::open(16);
        let mut cov = DispatchCoverage::default();
        let mut actor = UnitWork::at(2, 4, 100, 200);
        actor.uid = 10;
        actor.body.angle = 123;
        actor.lead_guy.angle = 123;
        actor.orders.push_back(build_at_order());
        let order = actor.orders.front().unwrap().clone();
        w.build_at_preflight = Some(Ok(build_at_ready_input(&actor, &order)));
        w.build_at_construct = BuildAtConstructResult::Progressed {
            credited: 7,
            started_this_call: true,
        };

        assert_eq!(
            do_build_at(&mut actor, &mut w, &mut cov),
            ArmResult::Working
        );
        assert_eq!(actor.orders.front().unwrap().kind, OrderIndex::BuildAt);
        assert_eq!(actor.body.angle, crate::trig::find_angle(100, 0));
        assert_eq!(
            w.build_at_effects,
            vec![
                BuildAtEffect::SetAnimation {
                    animation: construction_builder::CHAR_BUILD
                },
                BuildAtEffect::SetAngle {
                    angle: crate::trig::find_angle(100, 0)
                }
            ]
        );
        assert_eq!(
            w.build_at_events,
            vec!["preflight", "animation", "set_angle", "construct"]
        );
        assert_eq!(cov.completed, 0);
    }

    #[test]
    fn build_at_decoy_still_animates_and_faces_before_suppressing_work() {
        let mut w = TestWorld::open(16);
        let mut cov = DispatchCoverage::default();
        let mut actor = UnitWork::at(2, 4, 100, 200);
        actor.uid = 10;
        actor.body.angle = 123;
        actor.unit_masks |= 1;
        actor.orders.push_back(build_at_order());
        let order = actor.orders.front().unwrap().clone();
        w.build_at_preflight = Some(Ok(build_at_ready_input(&actor, &order)));

        assert_eq!(
            do_build_at(&mut actor, &mut w, &mut cov),
            ArmResult::Working
        );
        assert_eq!(
            w.build_at_events,
            vec!["preflight", "animation", "set_angle"]
        );
        assert_eq!(actor.orders.front().unwrap().kind, OrderIndex::BuildAt);
        assert_eq!(cov.completed, 0);
    }

    #[test]
    fn build_at_reswarm_bare_kills_before_reinstalling_with_group_flag() {
        let mut w = TestWorld::open(16);
        let mut cov = DispatchCoverage::default();
        let mut actor = UnitWork::at(2, 4, 100, 200);
        actor.uid = 10;
        let mut order = build_at_order();
        order.flags |= ORDER_GROUP;
        actor.orders.push_back(order.clone());
        let mut input = build_at_ready_input(&actor, &order);
        input.adjacent = false;
        w.build_at_preflight = Some(Ok(input));

        assert_eq!(
            do_build_at(&mut actor, &mut w, &mut cov),
            ArmResult::Working
        );
        assert_eq!(
            w.build_at_effects,
            vec![BuildAtEffect::Reswarm {
                preserve_group_flag: true
            }]
        );
        assert_eq!(w.build_at_effect_fronts, vec![None]);
        assert_eq!(actor.orders.front().unwrap().kind, OrderIndex::BuildAt);
        assert_ne!(actor.orders.front().unwrap().flags & ORDER_GROUP, 0);
        assert_eq!(cov.completed, 1);
    }

    #[test]
    fn build_at_completion_retires_after_activation_before_finish_tail() {
        let mut w = TestWorld::open(16);
        let mut cov = DispatchCoverage::default();
        let mut actor = UnitWork::at(2, 4, 100, 200);
        actor.uid = 10;
        actor.orders.push_back(build_at_order());
        actor.orders.push_back(OrderRec::of_kind(OrderIndex::Guard));
        let order = actor.orders.front().unwrap().clone();
        w.build_at_preflight = Some(Ok(build_at_ready_input(&actor, &order)));
        w.build_at_construct = BuildAtConstructResult::Completed {
            credited: 9,
            started_this_call: false,
        };

        assert_eq!(
            do_build_at(&mut actor, &mut w, &mut cov),
            ArmResult::Retired(KillReason::Completed)
        );
        assert_eq!(actor.orders.front().unwrap().kind, OrderIndex::Guard);
        assert_eq!(
            w.build_at_effects.last(),
            Some(&BuildAtEffect::FinishTail {
                reason: BuilderFinish::SiteCompleted
            })
        );
        assert_eq!(
            w.build_at_effect_fronts.last(),
            Some(&Some(OrderIndex::Guard)),
            "completion tail must observe the newly exposed action"
        );
        assert_eq!(
            w.build_at_events,
            vec![
                "preflight",
                "animation",
                "set_angle",
                "construct",
                "finish_tail"
            ]
        );
        assert_eq!(cov.completed, 1);
    }

    #[test]
    fn build_at_invalid_target_bare_kills_before_build_done_tail() {
        let mut w = TestWorld::open(16);
        let mut cov = DispatchCoverage::default();
        let mut actor = UnitWork::at(2, 4, 100, 200);
        actor.uid = 10;
        actor.orders.push_back(build_at_order());
        let order = actor.orders.front().unwrap().clone();
        let mut input = build_at_ready_input(&actor, &order);
        input.target_is_valid_wall = false;
        w.build_at_preflight = Some(Ok(input));

        assert_eq!(
            do_build_at(&mut actor, &mut w, &mut cov),
            ArmResult::Retired(KillReason::Completed)
        );
        assert!(actor.orders.is_empty());
        assert_eq!(
            w.build_at_effects,
            vec![BuildAtEffect::FinishTail {
                reason: BuilderFinish::InvalidTarget
            }]
        );
        assert_eq!(w.build_at_effect_fronts, vec![None]);
        assert_eq!(w.build_at_events, vec!["preflight", "finish_tail"]);
        assert_eq!(cov.completed, 1);
    }

    #[test]
    fn build_at_active_target_retires_before_check_build_order_tail() {
        let mut w = TestWorld::open(16);
        let mut cov = DispatchCoverage::default();
        let mut actor = UnitWork::at(2, 4, 100, 200);
        actor.uid = 10;
        actor.orders.push_back(build_at_order());
        actor.orders.push_back(OrderRec::of_kind(OrderIndex::Guard));
        let order = actor.orders.front().unwrap().clone();
        let mut input = build_at_ready_input(&actor, &order);
        input.target_is_active = true;
        w.build_at_preflight = Some(Ok(input));

        assert_eq!(
            do_build_at(&mut actor, &mut w, &mut cov),
            ArmResult::Retired(KillReason::Completed)
        );
        assert_eq!(actor.orders.front().unwrap().kind, OrderIndex::Guard);
        assert_eq!(
            w.build_at_effects,
            vec![BuildAtEffect::FinishTail {
                reason: BuilderFinish::TargetAlreadyActive
            }]
        );
        assert_eq!(w.build_at_effect_fronts, vec![Some(OrderIndex::Guard)]);
        assert_eq!(w.build_at_events, vec!["preflight", "finish_tail"]);
        assert_eq!(cov.completed, 1);
    }

    #[test]
    fn suspended_search_containers_are_owned_per_unit() {
        let w = TestWorld::open(16);
        let mut singleton = PathFinder::new();
        let mut a = UnitWork::at(0, 1, movement::ucell_centre(2), movement::ucell_centre(2));
        let mut b = UnitWork::at(0, 2, movement::ucell_centre(2), movement::ucell_centre(10));
        a.path_unit.small_footprint = false;
        b.path_unit.small_footprint = false;
        // 500 / 100² and 300 / 100² are both zero: each call yields a small search slice.
        a.guy_env.ai_speed = 100;
        b.guy_env.ai_speed = 100;
        let ad = (movement::ucell_centre(40), movement::ucell_centre(2));
        let bd = (movement::ucell_centre(40), movement::ucell_centre(10));

        assert_eq!(
            find_path(&mut singleton, &w, &mut a, ad.0, ad.1, 0),
            PathOutcome::Suspended
        );
        assert_eq!(
            find_path(&mut singleton, &w, &mut b, bd.0, bd.1, 0),
            PathOutcome::Suspended
        );
        assert!(a.parked_pathfinder.is_some());
        assert!(b.parked_pathfinder.is_some());

        let mut ao = PathOutcome::Suspended;
        let mut bo = PathOutcome::Suspended;
        for _ in 0..128 {
            if ao == PathOutcome::Suspended {
                ao = find_path(&mut singleton, &w, &mut a, ad.0, ad.1, 0);
            }
            if bo == PathOutcome::Suspended {
                bo = find_path(&mut singleton, &w, &mut b, bd.0, bd.1, 0);
            }
            if ao != PathOutcome::Suspended && bo != PathOutcome::Suspended {
                break;
            }
        }
        assert_eq!(ao, PathOutcome::Found);
        assert_eq!(bo, PathOutcome::Found);
        assert!(!a.path.is_empty());
        assert!(!b.path.is_empty());
    }

    #[test]
    fn move_retry_countdown_adds_three_attempts_when_it_expires() {
        let mut w = TestWorld::open(16);
        let mut pf = PathFinder::new();
        let mut cov = DispatchCoverage::default();
        let mut u = UnitWork::at(0, 0, 100, 100);
        let mut order = OrderRec::move_to(1000, 1000, 0);
        order.retry = 2;
        u.orders.push_back(order);
        u.unit_masks |= masks::PATH_EXHAUSTED;

        assert_eq!(
            do_move(&mut u, &mut w, &mut pf, &mut cov),
            ArmResult::Working
        );
        assert_eq!(u.orders.front().unwrap().retry, 1);
        assert_eq!(
            do_move(&mut u, &mut w, &mut pf, &mut cov),
            ArmResult::Working
        );
        assert_eq!(u.orders.front().unwrap().retry, 0);
        assert_eq!(u.orders.front().unwrap().attempts, 3);
        let _ = do_move(&mut u, &mut w, &mut pf, &mut cov);
        assert_eq!(u.orders.front().unwrap().attempts, 2);
    }

    #[test]
    fn collision_pause_decrements_and_holds_the_frame() {
        let mut w = TestWorld::open(16);
        let mut pf = PathFinder::new();
        let mut cov = DispatchCoverage::default();
        let mut u = UnitWork::at(0, 0, 100, 100);
        let mut order = OrderRec::move_to(1000, 100, 0);
        order.pause = 2;
        u.orders.push_back(order);
        u.unit_masks |= masks::PATH_EXHAUSTED;
        let before = u.body;

        assert_eq!(
            do_move(&mut u, &mut w, &mut pf, &mut cov),
            ArmResult::Working
        );
        assert_eq!(u.orders.front().unwrap().pause, 1);
        assert_eq!((u.body.x, u.body.y), (before.x, before.y));
    }

    #[test]
    fn invalid_step_clears_the_path_bit_and_rebuilds_next_frame() {
        let mut w = TestWorld::open(16);
        w.blocked.push((1, 0));
        let mut pf = PathFinder::new();
        let mut cov = DispatchCoverage::default();
        let mut u = UnitWork::at(0, 0, 180, 24);
        u.body.angle = 0x4000_0000;
        u.myspeed = 48;
        u.tolerance = 0;
        u.orders.push_back(OrderRec::move_to(600, 24, 0));
        u.path.push(PathData {
            to_x: 220,
            to_y: 24,
            tolerance: 0,
            flags: PathData::FLAG_WAYPOINT,
        });
        u.unit_masks |= masks::PATH_EXHAUSTED;

        assert_eq!(
            do_move(&mut u, &mut w, &mut pf, &mut cov),
            ArmResult::Blocked
        );
        assert_eq!(u.unit_masks & masks::PATH_EXHAUSTED, 0);
        assert_eq!(u.path.peek().unwrap().to_x, 220);

        // The old stack remains exactly as in retail, but bit 8 makes the next frame enter
        // Unit::find_path. Remove the transient obstruction and verify a fresh leg is installed.
        w.blocked.clear();
        let _ = do_move(&mut u, &mut w, &mut pf, &mut cov);
        assert_ne!(u.path.peek().map(|p| p.to_x), Some(220));
        assert_ne!(u.unit_masks & masks::PATH_EXHAUSTED, 0);
    }

    #[test]
    fn reaching_collision_detour_does_not_complete_the_original_destination() {
        let mut w = TestWorld::open(16);
        let mut pf = PathFinder::new();
        let mut cov = DispatchCoverage::default();
        let mut u = UnitWork::at(0, 0, 220, 24);
        u.body.angle = movement::find_angle(0, 0);
        u.myspeed = 48;
        u.tolerance = 1;
        let mut order = OrderRec::move_to(600, 24, 1);
        order.dest = 1;
        // Collision's set_order_detour writes only the current step fields.
        order.dest_x = 220;
        order.dest_y = 24;
        u.orders.push_back(order);
        u.path.push(PathData {
            to_x: 600,
            to_y: 24,
            tolerance: 1,
            flags: PathData::FLAG_MORE,
        });
        u.path.push(PathData {
            to_x: 220,
            to_y: 24,
            tolerance: 0,
            flags: PathData::FLAG_WAYPOINT,
        });
        u.unit_masks |= masks::PATH_EXHAUSTED;

        assert_eq!(
            do_move(&mut u, &mut w, &mut pf, &mut cov),
            ArmResult::Working
        );
        assert_eq!(u.orders.len(), 1);
        assert_eq!(u.orders.front().unwrap().dest, 1);
        assert_eq!(u.path.len(), 1, "only the detour waypoint is consumed");
        assert_eq!(u.path.peek().unwrap().to_x, 600);
    }

    #[test]
    fn a_queued_order_becomes_current_when_the_move_retires() {
        let mut w = TestWorld::open(16).with_object(1, 3, tgt(24, 24, 42));
        let mut pf = PathFinder::new();
        let mut cov = DispatchCoverage::default();
        let mut u = UnitWork::at(0, 0, 24, 24);
        u.tolerance = 96;
        u.orders.push_back(OrderRec::move_to(24, 24, 96)); // already inside tolerance
        u.orders.push_back(OrderRec::gather(1, 3, 42));

        w.frame = 1;
        let r = work(&mut u, &mut w, &mut pf, &mut cov);
        assert_eq!(r.dispatched, OrderIndex::MoveTo);
        assert!(matches!(
            r.result,
            ArmResult::Retired(KillReason::Completed)
        ));
        assert_eq!(u.order_type(), OrderIndex::Gather);

        w.frame = 2;
        let r = work(&mut u, &mut w, &mut pf, &mut cov);
        assert_eq!(r.dispatched, OrderIndex::Gather);
        assert_eq!(r.result, ArmResult::Gathered(3));
    }

    // -- do_group_move ----------------------------------------------------

    fn group_move_order(actor_who: u8, leader_o: i16, id: i32, x: i32, y: i32) -> OrderRec {
        OrderRec {
            kind: OrderIndex::GroupMove,
            flags: ORDER_GROUP | ORDER_PATHED,
            x,
            y,
            dest_x: x,
            dest_y: y,
            group_oxx: i32::from(leader_o),
            group_whose: i32::from(actor_who),
            group_id: id,
            ..OrderRec::default()
        }
    }

    fn group_attack_order(
        actor_who: u8,
        leader_o: i16,
        id: i32,
        target_who: i32,
        target_o: i32,
    ) -> OrderRec {
        OrderRec {
            kind: OrderIndex::GroupAttack,
            flags: ORDER_GROUP,
            target_o,
            target_who,
            group_oxx: i32::from(leader_o),
            group_whose: i32::from(actor_who),
            group_id: id,
            group_angle: 0x1234_5678,
            ..OrderRec::default()
        }
    }

    #[test]
    fn group_move_missing_host_is_a_zero_mutation_transaction() {
        let mut w = TestWorld::open(16);
        let mut pf = PathFinder::new();
        let mut cov = DispatchCoverage::default();
        let mut u = UnitWork::at(2, 7, 100, 200);
        u.group = 3;
        u.safe = 41;
        u.tolerance = 17;
        u.unit_masks = 0x1234_5678;
        u.path.push(PathData {
            to_x: 111,
            to_y: 222,
            tolerance: 9,
            flags: PathData::FLAG_MORE,
        });
        u.orders
            .push_back(group_move_order(u.who, u.o, 91, 900, 700));
        let before_body = u.body;
        let before_orders = u.orders.clone();
        let before_path = u.path.clone();

        assert_eq!(
            do_group_move(&mut u, &mut w, &mut pf, &mut cov),
            ArmResult::HostUnavailable
        );
        assert_eq!(
            (u.body.x, u.body.y, u.body.angle, u.body.stuck_budget),
            (
                before_body.x,
                before_body.y,
                before_body.angle,
                before_body.stuck_budget
            )
        );
        assert_eq!(u.orders, before_orders);
        assert_eq!(u.path, before_path);
        assert_eq!((u.group, u.safe, u.tolerance), (3, 41, 17));
        assert_eq!(u.unit_masks, 0x1234_5678);
        assert!(w.group_effects.is_empty());
    }

    #[test]
    fn ungrouped_group_move_becomes_an_ordinary_move_without_a_host() {
        let mut w = TestWorld::open(16);
        let mut pf = PathFinder::new();
        let mut cov = DispatchCoverage::default();
        let mut u = UnitWork::at(2, 7, 100, 200);
        u.group = -1;
        let mut order = group_move_order(u.who, 3, 91, 900, 700);
        order.dest = 1;
        order.orig_x = -1;
        order.orig_y = -1;
        u.orders.push_back(order);
        u.path.push(PathData {
            to_x: 333,
            to_y: 444,
            tolerance: 0,
            flags: PathData::FLAG_WAYPOINT,
        });

        assert_eq!(
            do_group_move(&mut u, &mut w, &mut pf, &mut cov),
            ArmResult::Working
        );
        let ordinary = u.orders.front().unwrap();
        assert_eq!(ordinary.kind, OrderIndex::MoveTo);
        assert_eq!(ordinary.flags, ORDER_GROUP);
        assert_eq!(ordinary.dest, 0);
        assert_eq!((ordinary.orig_x, ordinary.orig_y), (900, 700));
        assert_eq!(ordinary.group_id, -1);
        assert!(
            u.path.is_empty(),
            "a follower conversion kills its old path"
        );
    }

    #[test]
    fn group_move_leader_delegates_to_move_then_updates_positions() {
        let mut w = TestWorld::open(16);
        w.group_plan = Ok(GroupMovePlan::Leader {
            kill_group_before_move: false,
        });
        let mut pf = PathFinder::new();
        let mut cov = DispatchCoverage::default();
        let mut u = UnitWork::at(2, 7, 100, 200);
        u.group = 3;
        u.tolerance = 0;
        u.body.angle = movement::find_angle(100, 0);
        u.orders
            .push_back(group_move_order(u.who, u.o, 91, 200, 200));

        let result = do_group_move(&mut u, &mut w, &mut pf, &mut cov);
        assert!(matches!(result, ArmResult::Moved | ArmResult::Turned));
        assert_eq!(w.group_effects, vec![GroupMoveEffect::UpdatePositions]);
        assert_eq!(u.orders.front().unwrap().group_id, 91);
    }

    #[test]
    fn group_move_leader_raw_zero_runs_kill_then_distribute_in_order() {
        let mut w = TestWorld::open(16);
        w.blocked.push((1, 0));
        w.group_plan = Ok(GroupMovePlan::Leader {
            kill_group_before_move: false,
        });
        w.group_post = GroupMovePostStep::KillGroupAndDistribute;
        let mut pf = PathFinder::new();
        let mut cov = DispatchCoverage::default();
        let mut u = UnitWork::at(2, 7, 180, 24);
        u.group = 3;
        u.tolerance = 0;
        u.body.angle = 0x4000_0000;
        u.unit_masks |= masks::PATH_EXHAUSTED;
        u.path.push(PathData {
            to_x: 220,
            to_y: 24,
            tolerance: 0,
            flags: PathData::FLAG_WAYPOINT,
        });
        u.orders
            .push_back(group_move_order(u.who, u.o, 91, 600, 24));

        assert_eq!(
            do_group_move(&mut u, &mut w, &mut pf, &mut cov),
            ArmResult::Working
        );
        assert_eq!(
            w.group_effects,
            vec![
                GroupMoveEffect::KillGroupMove { id: 91 },
                GroupMoveEffect::DistributeAttack
            ]
        );
    }

    #[test]
    fn group_move_follower_publishes_one_snapshot_before_move_step() {
        let mut w = TestWorld::open(16);
        let mut path = PathStack::new();
        path.push(PathData {
            to_x: 200,
            to_y: 200,
            tolerance: 0,
            flags: PathData::FLAG_WAYPOINT,
        });
        w.group_plan = Ok(GroupMovePlan::Step(GroupMoveFollowerStep {
            path: path.clone(),
            dest_x: 200,
            dest_y: 200,
            in_group: 1,
            speed: 24,
        }));
        let mut pf = PathFinder::new();
        let mut cov = DispatchCoverage::default();
        let mut u = UnitWork::at(2, 8, 100, 200);
        u.group = 3;
        u.tolerance = 77;
        u.body.angle = movement::find_angle(100, 0);
        u.orders.push_back(group_move_order(u.who, 7, 91, 900, 700));

        let result = do_group_move(&mut u, &mut w, &mut pf, &mut cov);
        assert!(matches!(result, ArmResult::Moved | ArmResult::Turned));
        let current = u.orders.front().unwrap();
        assert_eq!((current.dest_x, current.dest_y), (200, 200));
        assert_eq!(current.in_group, 1);
        assert_eq!(u.tolerance, 0);
        assert!(u.body.x >= 100);
        assert!(w.group_effects.is_empty());
    }

    // -- do_group_attack -------------------------------------------------

    #[test]
    fn group_attack_missing_host_is_a_zero_mutation_transaction() {
        let mut w = TestWorld::open(16);
        let mut pf = PathFinder::new();
        let mut cov = DispatchCoverage::default();
        let mut u = UnitWork::at(2, 7, 100, 200);
        u.group = 3;
        u.dest_angle = 99;
        u.safe = 29;
        u.path.push(PathData {
            to_x: 200,
            to_y: 200,
            tolerance: 0,
            flags: PathData::FLAG_WAYPOINT,
        });
        u.orders
            .push_back(group_attack_order(u.who, u.o, 93, 1, 12));
        let before_body = u.body;
        let before_orders = u.orders.clone();
        let before_path = u.path.clone();

        assert_eq!(
            do_group_attack(&mut u, &mut w, &mut pf, &mut cov),
            ArmResult::HostUnavailable
        );
        assert_eq!(
            (u.body.x, u.body.y, u.body.angle, u.body.stuck_budget),
            (
                before_body.x,
                before_body.y,
                before_body.angle,
                before_body.stuck_budget
            )
        );
        assert_eq!(u.orders, before_orders);
        assert_eq!(u.path, before_path);
        assert_eq!((u.group, u.safe, u.dest_angle), (3, 29, 99));
        assert!(w.group_attack_effects.is_empty());
    }

    #[test]
    fn group_attack_rejects_a_bad_preflight_plan_before_publishing_angle() {
        let mut w = TestWorld::open(16);
        w.group_attack_plan = Ok(GroupAttackPlan::LeaderFight {
            target_o: -1,
            target_who: 1,
        });
        let mut pf = PathFinder::new();
        let mut cov = DispatchCoverage::default();
        let mut u = UnitWork::at(2, 7, 100, 200);
        u.group = 3;
        u.dest_angle = 99;
        u.orders
            .push_back(group_attack_order(u.who, u.o, 93, 1, 12));
        let before = u.orders.clone();

        assert_eq!(
            do_group_attack(&mut u, &mut w, &mut pf, &mut cov),
            ArmResult::MalformedOrder
        );
        assert_eq!(u.dest_angle, 99);
        assert_eq!(u.orders, before);
        assert!(w.group_attack_effects.is_empty());
    }

    #[test]
    fn ungrouped_group_attack_converts_to_attack_without_a_host() {
        let mut w = TestWorld::open(16);
        let mut pf = PathFinder::new();
        let mut cov = DispatchCoverage::default();
        let mut u = UnitWork::at(2, 8, 100, 200);
        let mut order = group_attack_order(u.who, 7, 93, 1, 12);
        order.target_uid = 71;
        order.attack_def_x = 400;
        order.attack_def_y = 500;
        order.attack_mandatory = 2;
        order.attack_defensive = 1;
        order.attack_in_range = 1;
        order.attack_ever_in_range = 1;
        order.attack_new_ord = 1;
        order.group_attack_temporary = 7;
        order.group_attack_oxxx = 9;
        order.group_attack_whosoever = 3;
        u.orders.push_back(order);

        assert_eq!(
            do_group_attack(&mut u, &mut w, &mut pf, &mut cov),
            ArmResult::Working
        );
        let ordinary = u.orders.front().unwrap();
        assert_eq!(ordinary.kind, OrderIndex::Attack);
        assert_eq!(
            (
                ordinary.target_who,
                ordinary.target_o,
                ordinary.target_uid,
                ordinary.attack_def_x,
                ordinary.attack_def_y,
                ordinary.attack_mandatory,
                ordinary.attack_defensive,
                ordinary.attack_in_range,
                ordinary.attack_ever_in_range,
                ordinary.attack_new_ord,
            ),
            (1, 12, 71, 400, 500, 2, 1, 1, 1, 1)
        );
        assert_eq!(
            (
                ordinary.group_id,
                ordinary.group_oxx,
                ordinary.group_whose,
                ordinary.group_attack_temporary,
                ordinary.group_attack_oxxx,
                ordinary.group_attack_whosoever,
            ),
            (-1, -1, -1, 0, -1, -1)
        );
        assert_eq!(u.body.angle, 0x1234_5678);
        assert_eq!(u.lead_guy.angle, 0x1234_5678);
        assert_eq!(u.dest_angle, 0);
        assert!(w.group_attack_effects.is_empty());
    }

    #[test]
    fn group_attack_leader_writes_scratch_before_fight() {
        let mut w = TestWorld::open(16);
        w.group_attack_plan = Ok(GroupAttackPlan::LeaderFight {
            target_o: 17,
            target_who: 4,
        });
        let mut pf = PathFinder::new();
        let mut cov = DispatchCoverage::default();
        let mut u = UnitWork::at(2, 7, 100, 200);
        u.group = 3;
        let mut order = group_attack_order(u.who, u.o, 93, 1, 12);
        order.attack_mandatory = 2;
        order.group_attack_temporary = 7;
        u.orders.push_back(order);

        assert_eq!(
            do_group_attack(&mut u, &mut w, &mut pf, &mut cov),
            ArmResult::Working
        );
        assert_eq!(u.body.angle, 0x1234_5678);
        assert_eq!(u.lead_guy.angle, 0x1234_5678);
        assert_eq!(
            w.group_attack_effects,
            vec![
                GroupAttackEffect::SetAngle { angle: 0x1234_5678 },
                GroupAttackEffect::Fight {
                    target_o: 17,
                    target_who: 4,
                    mandatory: 2,
                    temporary: 7,
                },
            ]
        );
        assert_eq!(
            w.group_attack_scratch_seen,
            vec![Some((-1, -1)), Some((17, 4))]
        );
        assert_eq!(
            w.group_attack_angles_seen,
            vec![(0x1234_5678, 0), (0x1234_5678, 0x1234_5678)]
        );
    }

    #[test]
    fn group_attack_leader_kills_move_before_distributing_target() {
        let mut w = TestWorld::open(16);
        w.group_attack_plan = Ok(GroupAttackPlan::LeaderKillMoveAndDistribute);
        let mut pf = PathFinder::new();
        let mut cov = DispatchCoverage::default();
        let mut u = UnitWork::at(2, 7, 100, 200);
        u.group = 3;
        u.orders
            .push_back(group_attack_order(u.who, u.o, 93, 1, 12));

        assert_eq!(
            do_group_attack(&mut u, &mut w, &mut pf, &mut cov),
            ArmResult::Working
        );
        assert_eq!(
            w.group_attack_effects,
            vec![
                GroupAttackEffect::SetAngle { angle: 0x1234_5678 },
                GroupAttackEffect::KillGroupMove { id: 93 },
                GroupAttackEffect::DistributeAttack {
                    target_o: 12,
                    target_who: 1,
                },
            ]
        );
    }

    #[test]
    fn group_attack_follower_fight_forces_both_fight_flags_to_one() {
        let mut w = TestWorld::open(16);
        w.group_attack_plan = Ok(GroupAttackPlan::FollowerFight {
            target_o: 18,
            target_who: 5,
        });
        let mut pf = PathFinder::new();
        let mut cov = DispatchCoverage::default();
        let mut u = UnitWork::at(2, 8, 100, 200);
        u.group = 3;
        let mut order = group_attack_order(u.who, 7, 93, 1, 12);
        order.attack_mandatory = 0;
        order.group_attack_temporary = 0;
        u.orders.push_back(order);

        assert_eq!(
            do_group_attack(&mut u, &mut w, &mut pf, &mut cov),
            ArmResult::Working
        );
        assert_eq!(
            w.group_attack_effects,
            vec![
                GroupAttackEffect::SetAngle { angle: 0x1234_5678 },
                GroupAttackEffect::Fight {
                    target_o: 18,
                    target_who: 5,
                    mandatory: 1,
                    temporary: 1,
                },
            ]
        );
        assert_eq!(u.orders.front().unwrap().group_attack_oxxx, 18);
        assert_eq!(u.orders.front().unwrap().group_attack_whosoever, 5);
    }

    #[test]
    fn group_attack_follower_face_sets_idle_after_angle_publication() {
        let mut w = TestWorld::open(16);
        w.group_attack_plan = Ok(GroupAttackPlan::FollowerFace);
        let mut pf = PathFinder::new();
        let mut cov = DispatchCoverage::default();
        let mut u = UnitWork::at(2, 8, 100, 200);
        u.group = 3;
        u.body.angle = 0;
        u.orders.push_back(group_attack_order(u.who, 7, 93, 1, 12));

        assert_eq!(
            do_group_attack(&mut u, &mut w, &mut pf, &mut cov),
            ArmResult::Working
        );
        assert_eq!(u.body.angle, 0x1234_5678);
        assert_eq!(
            w.group_attack_effects,
            vec![
                GroupAttackEffect::SetAngle { angle: 0x1234_5678 },
                GroupAttackEffect::SetIdleAnim,
            ]
        );
    }

    #[test]
    fn group_attack_recharging_holds_without_demanding_a_host() {
        let mut w = TestWorld::open(16);
        let mut pf = PathFinder::new();
        let mut cov = DispatchCoverage::default();
        let mut u = UnitWork::at(2, 7, 100, 200);
        u.group = 3;
        u.recharging = 1;
        u.orders
            .push_back(group_attack_order(u.who, u.o, 93, 1, 12));
        let before = u.orders.clone();

        assert_eq!(
            do_group_attack(&mut u, &mut w, &mut pf, &mut cov),
            ArmResult::Working
        );
        assert_eq!(u.orders, before);
        assert_eq!(u.body.angle, 0);
        assert!(w.group_attack_effects.is_empty());
    }

    #[test]
    fn group_attack_follower_rotates_behind_queued_group_move() {
        let mut w = TestWorld::open(16);
        w.group_attack_plan = Ok(GroupAttackPlan::FollowerTailStep);
        let mut pf = PathFinder::new();
        let mut cov = DispatchCoverage::default();
        let mut u = UnitWork::at(2, 8, 100, 200);
        u.group = 3;
        u.body.angle = movement::find_angle(100, 0);
        u.path.push(PathData {
            to_x: 220,
            to_y: 200,
            tolerance: 0,
            flags: PathData::FLAG_WAYPOINT,
        });
        let mut attack = group_attack_order(u.who, 7, 93, 1, 12);
        attack.group_attack_temporary = 1;
        let queued = group_move_order(u.who, 7, 93, 220, 200);
        u.orders.push_back(attack.clone());
        u.orders.push_back(queued);
        u.orders.push_back(OrderRec::of_kind(OrderIndex::Guard));

        assert!(matches!(
            do_group_attack(&mut u, &mut w, &mut pf, &mut cov),
            ArmResult::Moved | ArmResult::Turned | ArmResult::Blocked | ArmResult::Working
        ));
        let kinds: Vec<_> = u.orders.iter().map(|order| order.kind).collect();
        assert_eq!(
            kinds,
            vec![
                OrderIndex::GroupMove,
                OrderIndex::Guard,
                OrderIndex::GroupAttack
            ]
        );
        assert_eq!(u.orders.iter().nth(2), Some(&attack));
        assert_eq!(
            w.group_attack_effects,
            vec![
                GroupAttackEffect::SetAngle { angle: 0x1234_5678 },
                GroupAttackEffect::ProbeSpeed { mode: 1 },
            ]
        );
    }

    // -- do_group_attack_to ----------------------------------------------

    #[test]
    fn group_attack_to_missing_combat_host_prevents_group_movement_mutation() {
        let mut w = TestWorld::open(16);
        // If combat capability were checked late, this plan would already mutate movement.
        w.group_plan = Ok(GroupMovePlan::Leader {
            kill_group_before_move: false,
        });
        let mut pf = PathFinder::new();
        let mut cov = DispatchCoverage::default();
        let mut u = UnitWork::at(2, 7, 100, 200);
        u.group = 3;
        u.safe = 29;
        u.path.push(PathData {
            to_x: 200,
            to_y: 200,
            tolerance: 0,
            flags: PathData::FLAG_WAYPOINT,
        });
        let mut order = group_move_order(u.who, u.o, 92, 900, 700);
        order.kind = OrderIndex::GroupAttackTo;
        u.orders.push_back(order);
        let before_body = u.body;
        let before_orders = u.orders.clone();
        let before_path = u.path.clone();

        assert_eq!(
            do_group_attack_to(&mut u, &mut w, &mut pf, &mut cov),
            ArmResult::HostUnavailable
        );
        assert_eq!(
            (u.body.x, u.body.y, u.body.angle, u.body.stuck_budget),
            (
                before_body.x,
                before_body.y,
                before_body.angle,
                before_body.stuck_budget
            )
        );
        assert_eq!(u.orders, before_orders);
        assert_eq!(u.path, before_path);
        assert_eq!((u.group, u.safe), (3, 29));
        assert!(w.group_effects.is_empty());
        assert_eq!(w.group_attack_events, vec!["preflight"]);
    }

    #[test]
    fn ungrouped_group_attack_to_converts_without_demanding_a_combat_host() {
        let mut w = TestWorld::open(16);
        let mut pf = PathFinder::new();
        let mut cov = DispatchCoverage::default();
        let mut u = UnitWork::at(2, 8, 100, 200);
        let mut order = group_move_order(u.who, 7, 92, 900, 700);
        order.kind = OrderIndex::GroupAttackTo;
        u.orders.push_back(order);

        assert_eq!(
            do_group_attack_to(&mut u, &mut w, &mut pf, &mut cov),
            ArmResult::Working
        );
        assert_eq!(u.orders.front().unwrap().kind, OrderIndex::AttackTo);
        assert!(w.group_attack_events.is_empty());
    }

    #[test]
    fn group_attack_to_phase_calls_fight_or_attack_to_pause_in_retail_order() {
        let mut w = TestWorld::open(16);
        w.frame = 7; // (frame + o=8) % 15 == 0
        w.group_plan = Ok(GroupMovePlan::Hold);
        w.group_attack_preflight = Ok(());
        w.group_attack_calls_fight = true;
        w.group_attack_fight_result = ArmResult::Fired(37);
        let mut pf = PathFinder::new();
        let mut cov = DispatchCoverage::default();
        let mut u = UnitWork::at(2, 8, 100, 200);
        u.group = 3;
        let mut order = group_move_order(u.who, 7, 92, 900, 700);
        order.kind = OrderIndex::GroupAttackTo;
        u.orders.push_back(order);

        assert_eq!(
            do_group_attack_to(&mut u, &mut w, &mut pf, &mut cov),
            ArmResult::Fired(37)
        );
        assert_eq!(
            w.group_attack_events,
            vec!["preflight", "predicate", "fight"]
        );

        w.group_attack_events.clear();
        w.group_attack_calls_fight = false;
        w.group_attack_pause_gate = true;
        assert_eq!(
            do_group_attack_to(&mut u, &mut w, &mut pf, &mut cov),
            ArmResult::Working
        );
        assert_eq!(
            w.group_attack_events,
            vec!["preflight", "predicate", "attack_to_pause_gate"]
        );
        assert_eq!(u.orders.front().unwrap().pause, 15);
    }

    #[test]
    fn group_attack_to_non_phase_stops_after_group_move() {
        let mut w = TestWorld::open(16);
        w.frame = 8; // (frame + o=8) % 15 != 0
        w.group_plan = Ok(GroupMovePlan::Hold);
        w.group_attack_preflight = Ok(());
        let mut pf = PathFinder::new();
        let mut cov = DispatchCoverage::default();
        let mut u = UnitWork::at(2, 8, 100, 200);
        u.group = 3;
        let mut order = group_move_order(u.who, 7, 92, 900, 700);
        order.kind = OrderIndex::GroupAttackTo;
        u.orders.push_back(order);

        assert_eq!(
            do_group_attack_to(&mut u, &mut w, &mut pf, &mut cov),
            ArmResult::Working
        );
        assert_eq!(w.group_attack_events, vec!["preflight"]);
    }

    #[test]
    fn safe_countdown_has_retails_byte_wrap_semantics() {
        let mut w = TestWorld::open(16);
        let mut pf = PathFinder::new();
        let mut cov = DispatchCoverage::default();
        let mut u = UnitWork::at(0, 0, 24, 24);
        u.safe = i8::MIN;

        let _ = work(&mut u, &mut w, &mut pf, &mut cov);

        assert_eq!(u.safe, i8::MAX);
    }

    #[test]
    fn an_unreachable_destination_cancels_the_follow_on_attack() {
        // The `0x005F8B5F` cascade. A wall of blocked tiles around the goal makes the
        // search fail; the move dies and so does the ATTACK behind it.
        let mut w = TestWorld::open(16);
        for ty in 0..16 {
            for tx in 8..16 {
                w.blocked.push((tx, ty));
            }
        }
        w.objects.push(((1, 3), tgt(9 * 192, 5 * 192, 7)));
        let mut pf = PathFinder::new();
        let mut cov = DispatchCoverage::default();
        let mut u = UnitWork::at(0, 0, 2 * 192, 5 * 192);
        u.tolerance = 24;
        u.orders
            .push_back(OrderRec::move_to(9 * 192 + 24, 5 * 192 + 24, 24));
        u.orders.push_back(OrderRec::attack(1, 3, 7));

        let mut last = ArmResult::Working;
        for frame in 1..=64 {
            w.frame = frame;
            last = work(&mut u, &mut w, &mut pf, &mut cov).result;
            if u.order_type() != OrderIndex::MoveTo {
                break;
            }
        }
        assert!(
            matches!(last, ArmResult::Retired(KillReason::Failed)),
            "expected a path failure, got {:?}",
            last
        );
        assert!(
            u.orders.is_empty(),
            "the follow-on ATTACK survived an unreachable move: {:?}",
            u.orders
        );
        assert_eq!(cov.failed, 2, "both orders should be counted as failures");
        assert_eq!(w.draws, 1, "the pathfinder RNG obligation was not serviced");
        assert_eq!(cov.path_retry_draws, 1);
        assert_eq!(u.safe, 30, "A* failure must add 0x1e to UnitData::safe");
    }

    #[test]
    fn a_path_failure_does_not_cancel_a_follow_on_guard() {
        // Only ATTACK (10) and BUILD_AT (6) are cancelled. GUARD is not.
        let mut w = TestWorld::open(16);
        for ty in 0..16 {
            for tx in 8..16 {
                w.blocked.push((tx, ty));
            }
        }
        let mut pf = PathFinder::new();
        let mut cov = DispatchCoverage::default();
        let mut u = UnitWork::at(0, 0, 2 * 192, 5 * 192);
        u.tolerance = 24;
        u.orders
            .push_back(OrderRec::move_to(9 * 192 + 24, 5 * 192 + 24, 24));
        u.orders.push_back(OrderRec::of_kind(OrderIndex::Guard));

        for frame in 1..=64 {
            w.frame = frame;
            work(&mut u, &mut w, &mut pf, &mut cov);
            if u.order_type() != OrderIndex::MoveTo {
                break;
            }
        }
        assert_eq!(u.order_type(), OrderIndex::Guard);
        assert_eq!(cov.failed, 1);
    }

    #[test]
    fn build_at_is_cancelled_by_an_unreachable_move_too() {
        let mut w = TestWorld::open(16);
        for ty in 0..16 {
            for tx in 8..16 {
                w.blocked.push((tx, ty));
            }
        }
        let mut pf = PathFinder::new();
        let mut cov = DispatchCoverage::default();
        let mut u = UnitWork::at(0, 0, 2 * 192, 5 * 192);
        u.tolerance = 24;
        u.orders
            .push_back(OrderRec::move_to(9 * 192 + 24, 5 * 192 + 24, 24));
        u.orders.push_back(OrderRec::of_kind(OrderIndex::BuildAt));
        for frame in 1..=64 {
            w.frame = frame;
            work(&mut u, &mut w, &mut pf, &mut cov);
            if u.order_type() != OrderIndex::MoveTo {
                break;
            }
        }
        assert!(u.orders.is_empty());
    }

    // -- do_attack / do_gather ---------------------------------------------

    #[test]
    fn an_attack_on_a_recycled_slot_fails_rather_than_hitting_the_new_occupant() {
        let mut w = TestWorld::open(16).with_object(1, 3, tgt(300, 300, /*uid*/ 99));
        let mut pf = PathFinder::new();
        let mut cov = DispatchCoverage::default();
        let mut u = UnitWork::at(0, 0, 280, 300);
        u.orders.push_back(OrderRec::attack(1, 3, /*stale uid*/ 42));
        w.frame = 1;
        let r = work(&mut u, &mut w, &mut pf, &mut cov);
        assert!(matches!(r.result, ArmResult::Retired(KillReason::Failed)) || u.orders.is_empty());
        assert!(u.orders.is_empty());
        assert!(cov.failed >= 1);
    }

    #[test]
    fn an_attack_with_a_live_target_fires_and_keeps_the_order() {
        let mut w = TestWorld::open(16).with_object(1, 3, tgt(300, 300, 42));
        let mut pf = PathFinder::new();
        let mut cov = DispatchCoverage::default();
        let mut u = UnitWork::at(0, 0, 280, 300);
        u.orders.push_back(OrderRec::attack(1, 3, 42));
        w.frame = 1;
        let r = work(&mut u, &mut w, &mut pf, &mut cov);
        assert_eq!(r.result, ArmResult::Fired(7));
        assert_eq!(u.order_type(), OrderIndex::Attack);
        assert_eq!(u.idle, 0);
    }

    #[test]
    fn a_full_gather_node_retires_the_order_and_marks_the_unit() {
        let mut w = TestWorld::open(16).with_object(2, 1, tgt(100, 100, 8));
        w.gather = GatherOutcome::SlotsFull;
        let mut pf = PathFinder::new();
        let mut cov = DispatchCoverage::default();
        let mut u = UnitWork::at(0, 0, 100, 100);
        u.tolerance = 48;
        u.orders.push_back(OrderRec::gather(2, 1, 8));
        w.frame = 1;
        let r = work(&mut u, &mut w, &mut pf, &mut cov);
        assert!(matches!(r.result, ArmResult::Retired(KillReason::Failed)));
        assert!(u.flags & obj_flags::GATHER_REFUSED != 0);
    }

    #[test]
    fn an_exhausted_gather_node_completes_rather_than_fails() {
        let mut w = TestWorld::open(16).with_object(2, 1, tgt(100, 100, 8));
        w.gather = GatherOutcome::Exhausted;
        let mut pf = PathFinder::new();
        let mut cov = DispatchCoverage::default();
        let mut u = UnitWork::at(0, 0, 100, 100);
        u.tolerance = 48;
        u.orders.push_back(OrderRec::gather(2, 1, 8));
        w.frame = 1;
        let r = work(&mut u, &mut w, &mut pf, &mut cov);
        assert!(matches!(
            r.result,
            ArmResult::Retired(KillReason::Completed)
        ));
        assert_eq!(cov.completed, 1);
        assert_eq!(cov.failed, 0);
        assert_eq!(u.flags & obj_flags::GATHER_REFUSED, 0);
    }

    #[test]
    fn group_patrol_inserts_the_retail_attack_to_leg_ahead_of_itself() {
        let mut w = TestWorld::open(16);
        let mut pf = PathFinder::new();
        let mut cov = DispatchCoverage::default();
        let mut u = UnitWork::at(1, 4, 24, 24);
        let patrol = patrol::new_group_patrol(24, 24, 120, 72, 0, 0, 4, 1);
        u.orders.push_back(OrderRec::group_patrol(patrol));
        w.frame = 1;

        let r = work(&mut u, &mut w, &mut pf, &mut cov);
        assert_eq!(r.dispatched, OrderIndex::GroupPatrol);
        assert_eq!(r.result, ArmResult::Working);
        assert_eq!(u.orders.len(), 2);
        let leg = u.orders.front().unwrap();
        assert_eq!(leg.kind, OrderIndex::AttackTo);
        assert_eq!((leg.x, leg.y), (120, 72));
        assert_eq!(
            leg.flags & (ORDER_PATHED | ORDER_GROUP | crate::order::ORDER_DISEMBARK),
            0
        );
        let PatrolPayload::Group(saved) = &u.orders.iter().nth(1).unwrap().patrol_payload else {
            panic!("patrol payload did not survive executor insertion");
        };
        assert_eq!(saved.points.waypoint, 1);
    }

    #[test]
    fn air_patrol_retires_at_the_last_waypoint_only_with_follow_on_work() {
        let mut w = TestWorld::open(16);
        let mut pf = PathFinder::new();
        let mut cov = DispatchCoverage::default();
        let mut u = UnitWork::at(1, 2, 100, 100);
        u.orders.push_back(OrderRec::air_patrol(
            patrol::new_air_patrol(100, 100, -1, -1, None),
            false,
        ));
        u.orders.push_back(OrderRec::of_kind(OrderIndex::Guard));
        w.frame = 1;

        let r = work(&mut u, &mut w, &mut pf, &mut cov);
        assert_eq!(r.dispatched, OrderIndex::AirPatrol);
        assert_eq!(
            r.result,
            ArmResult::Retired(KillReason::Completed),
            "order-list length, not an invented loop counter, retires the patrol"
        );
        assert_eq!(u.order_type(), OrderIndex::Guard);
        assert_eq!(w.last_air_destination, Some((100, 100)));
    }

    #[test]
    fn air_patrol_scan_inserts_a_checksum_complete_strafe_front_order() {
        let target = AirPatrolTarget {
            o: 7,
            who: 2,
            uid: 99,
            x: 800,
            y: 900,
            domain: 2,
            ever_seen_by_actor: true,
        };
        let mut w = TestWorld::open(16);
        w.air_target = Some(target);
        let mut pf = PathFinder::new();
        let mut cov = DispatchCoverage::default();
        let mut u = UnitWork::at(1, 3, 0, 0);
        u.orders.push_back(OrderRec::air_patrol(
            patrol::new_air_patrol(1000, 1000, -1, -1, None),
            false,
        ));
        w.frame = 13; // (o + frame) % 16 == 0

        let r = work(&mut u, &mut w, &mut pf, &mut cov);
        assert_eq!(r.dispatched, OrderIndex::AirPatrol);
        assert_eq!(u.order_type(), OrderIndex::Strafe);
        assert_eq!(u.orders.len(), 2);
        let PatrolPayload::Strafe(strafe) = &u.orders.front().unwrap().patrol_payload else {
            panic!("patrol target scan did not create a StrafeOrder payload");
        };
        assert_eq!(
            (strafe.target_o, strafe.target_who, strafe.target_uid),
            (7, 2, 99)
        );
        assert_eq!((strafe.xx, strafe.yy), (800, 900));
        assert_eq!(strafe.air.cruising_alt, 0x640);
        assert_eq!(strafe.mandatory, 0);
    }

    // -- the driver --------------------------------------------------------

    #[test]
    fn recharging_returns_before_do_job_unless_the_order_is_a_group_order() {
        let mut w = TestWorld::open(16);
        let mut pf = PathFinder::new();
        let mut cov = DispatchCoverage::default();

        let mut u = UnitWork::at(0, 0, 0, 0);
        u.recharging = 3;
        u.orders.push_back(OrderRec::move_to(1000, 0, 8));
        w.frame = 1;
        let r = work(&mut u, &mut w, &mut pf, &mut cov);
        assert_eq!(r.early_out, Some(EarlyOut::RechargingNonGroupOrder));
        assert_eq!(cov.total, 0, "do_job must not have been reached");

        // The same unit with the group bit set does reach do_job.
        let mut g = OrderRec::move_to(1000, 0, 8);
        g.flags |= ORDER_GROUP;
        u.orders.replace(g);
        let r = work(&mut u, &mut w, &mut pf, &mut cov);
        assert_eq!(r.early_out, None);
        assert_eq!(cov.total, 1);
    }

    #[test]
    fn a_cast_spell_order_bypasses_the_recharge_gate_entirely() {
        // `cmp esi, 0xa`... no: the gate is skipped when the order type is CAST_SPELL (14),
        // which is `cmp esi, 0xe` at 0x0060D1CF's sibling. An unimplemented arm still
        // dispatches.
        let mut w = TestWorld::open(16);
        let mut pf = PathFinder::new();
        let mut cov = DispatchCoverage::default();
        let mut u = UnitWork::at(0, 0, 0, 0);
        u.recharging = 3;
        u.orders.push_back(OrderRec::of_kind(OrderIndex::CastSpell));
        w.frame = 1;
        let r = work(&mut u, &mut w, &mut pf, &mut cov);
        assert_eq!(r.early_out, None);
        assert_eq!(r.dispatched, OrderIndex::CastSpell);
        assert_eq!(r.result, ArmResult::NotPorted);
        assert_eq!(cov.unimplemented, 1);
    }

    #[test]
    fn every_one_of_the_twenty_eight_arms_dispatches_and_is_counted() {
        let mut w = TestWorld::open(16);
        let mut pf = PathFinder::new();
        let mut cov = DispatchCoverage::default();
        for k in OrderIndex::ALL {
            let mut u = UnitWork::at(0, 0, 0, 0);
            if k != OrderIndex::None {
                u.orders.push_back(OrderRec::of_kind(k));
            }
            w.frame = 1;
            let r = work(&mut u, &mut w, &mut pf, &mut cov);
            assert_eq!(r.dispatched, k, "arm {k} did not dispatch as itself");
        }
        assert_eq!(cov.total, NUM_UNIT_ORDERS as u64);
        for k in OrderIndex::ALL {
            assert_eq!(cov.dispatches[k.index()], 1, "arm {k} was not counted");
        }
        // Four unimplemented arms, each hit once. The smoke actors take all three grouped
        // executors' exact ungrouped conversion; grouped actors without a snapshot fail
        // closed at the mandatory host seam. Both boarding and both live patrol arms run,
        // as do the recovered FOLLOW, REPAIR, CHANGE_FORM, THINK, GUARD and GARRISON
        // executors — the last two reject a payload-free smoke order at their own seam,
        // which is a dispatch, not a miss.
        assert_eq!(cov.unimplemented, 4);
        assert!((cov.covered_fraction() - 24.0 / 28.0).abs() < 1e-12);
    }

    #[test]
    fn an_idle_unit_holds_position_and_reports_idle() {
        let mut w = TestWorld::open(16);
        let mut pf = PathFinder::new();
        let mut cov = DispatchCoverage::default();
        let mut u = UnitWork::at(0, 0, 500, 500);
        w.frame = 7;
        let r = work(&mut u, &mut w, &mut pf, &mut cov);
        assert_eq!(r.dispatched, OrderIndex::None);
        assert_eq!(u.idle, 1);
        assert_eq!((u.body.x, u.body.y), (500, 500));
    }

    #[test]
    fn the_last_instruction_clears_the_worked_this_frame_bit() {
        let mut w = TestWorld::open(16);
        let mut pf = PathFinder::new();
        let mut cov = DispatchCoverage::default();
        let mut u = UnitWork::at(0, 0, 0, 0);
        u.unit_masks |= masks::WORKED_THIS_FRAME | masks::PERIODIC_32;
        w.frame = 0; // phase 0 -> the %32 housekeeping runs
        let r = work(&mut u, &mut w, &mut pf, &mut cov);
        assert!(r.periodic_32);
        assert_eq!(u.unit_masks & masks::WORKED_THIS_FRAME, 0);
        assert_eq!(u.unit_masks & masks::PERIODIC_32, 0);
        assert_eq!(u.visible, 0);
    }

    #[test]
    fn work_survives_a_long_run_without_leaking_orders() {
        // 25,000 work calls across 40 units, the scale the replay scoreboard runs at.
        let mut w = TestWorld::open(24);
        let mut pf = PathFinder::new();
        let mut cov = DispatchCoverage::default();
        let mut units: Vec<UnitWork> = (0..40)
            .map(|i| {
                let mut u = UnitWork::at(0, i as i16, 96 + (i % 8) * 192, 96 + (i / 8) * 192);
                u.myspeed = 32;
                u.tolerance = 24;
                u.orders
                    .push_back(OrderRec::move_to(96 + ((i + 3) % 8) * 192, 96 + 192, 24));
                u
            })
            .collect();
        for f in 0..625i32 {
            w.frame = f;
            for u in units.iter_mut() {
                work(u, &mut w, &mut pf, &mut cov);
            }
        }
        assert_eq!(cov.work_calls, 25_000);
        assert_eq!(
            cov.completed, 40,
            "every unit should have arrived and retired"
        );
        assert_eq!(cov.failed, 0);
        for u in &units {
            assert!(u.orders.is_empty());
            assert!(u.path.is_empty());
        }
    }

    #[test]
    fn order_rec_round_trips_through_the_descriptive_order_type() {
        let r = OrderRec::attack(3, 12, 55);
        let o: Order = r.clone().into();
        assert_eq!(o.kind, OrderIndex::Attack);
        assert_eq!((o.target_who, o.target_o), (3, 12));
        let back: OrderRec = o.into();
        assert_eq!(back.kind, r.kind);
        assert_eq!(back.target_who, r.target_who);
        assert_eq!(back.target_o, r.target_o);
        assert_eq!(back.target_uid, r.target_uid);
        // A legacy OrderRec did not prove a stable port Handle; conversion preserves that
        // absence rather than manufacturing one from the retail triple.
        assert_eq!(back.target_handle, None);
    }

    /// Retail's close-waypoint arm turns in place until aligned. Omitting it produces the
    /// artificial orbit that the earlier stripped integrator exhibited.
    #[test]
    fn a_slow_turner_uses_the_retail_near_waypoint_gate_and_arrives() {
        let mut w = TestWorld::open(24);
        let mut pf = PathFinder::new();
        let mut cov = DispatchCoverage::default();
        let mut slow = UnitWork::at(0, 5, 96 + 5 * 192, 96);
        slow.myspeed = 32;
        slow.tolerance = 24;
        slow.guy_env.ut.turn_speed = 0x0800_0000; // 11.25 deg/frame -> 163-unit circle
        slow.orders.push_back(OrderRec::move_to(96, 288, 24));

        let mut fast = slow.clone();
        fast.guy_env.ut.turn_speed = 0x2000_0000; // 45 deg/frame -> a 41-unit circle

        for f in 0..400i32 {
            w.frame = f;
            work(&mut slow, &mut w, &mut pf, &mut cov);
            work(&mut fast, &mut w, &mut pf, &mut cov);
        }
        assert!(
            slow.orders.is_empty(),
            "the slow-turning unit remained in an artificial orbit"
        );
        assert!(
            fast.orders.is_empty(),
            "the fast-turning unit should have arrived and retired"
        );
    }

    #[test]
    fn the_order_list_bridge_round_trips_the_queue_shape() {
        let mut list = crate::order::OrderList::new();
        list.push(Order::move_to(400, 300, 8));
        list.push(Order::attack(2, 9));
        let q = adopt(&list);
        assert_eq!(q.len(), 2);
        assert_eq!(q.front().unwrap().kind, OrderIndex::MoveTo);
        assert_eq!(q.iter().nth(1).unwrap().target_who, 2);

        let mut back = crate::order::OrderList::new();
        publish(&q, &mut back);
        assert_eq!(back.len(), 2);
        assert_eq!(back.order_type(), OrderIndex::MoveTo);
        assert_eq!(back.iter().nth(1).unwrap().target_o, 9);
    }

    #[test]
    fn the_order_list_bridge_is_lossless_for_every_move_and_group_move_scalar() {
        let expected = crate::order::MoveOrderState {
            angle: 101,
            dest: 102,
            pause: 103,
            retry: 104,
            attempts: 105,
            timer: 106,
            facing: 107,
            dest_x: 108,
            dest_y: 109,
            last_x: 110,
            last_y: 111,
            coll_x: 112,
            coll_y: 113,
            orig_x: 114,
            orig_y: 115,
            off_x: 116,
            off_y: 117,
            group_oxx: 118,
            group_whose: 119,
            group_id: 120,
            group_form_id: 121,
            group_angle: 122,
            in_group: 123,
        };
        let order = Order {
            kind: OrderIndex::GroupMove,
            flags: ORDER_GROUP,
            x: 500,
            y: 600,
            tolerance: 7,
            move_state: Some(expected),
            ..Order::default()
        };
        let mut source = crate::order::OrderList::new();
        source.push(order.clone());

        let executable = adopt(&source);
        let rec = executable.current().unwrap();
        assert_eq!(rec.angle, expected.angle);
        assert_eq!(rec.dest, expected.dest);
        assert_eq!(rec.pause, expected.pause);
        assert_eq!(rec.retry, expected.retry);
        assert_eq!(rec.attempts, expected.attempts);
        assert_eq!(rec.timer, expected.timer);
        assert_eq!(rec.facing, expected.facing);
        assert_eq!((rec.dest_x, rec.dest_y), (expected.dest_x, expected.dest_y));
        assert_eq!((rec.last_x, rec.last_y), (expected.last_x, expected.last_y));
        assert_eq!((rec.coll_x, rec.coll_y), (expected.coll_x, expected.coll_y));
        assert_eq!((rec.orig_x, rec.orig_y), (expected.orig_x, expected.orig_y));
        assert_eq!((rec.off_x, rec.off_y), (expected.off_x, expected.off_y));
        assert_eq!((rec.group_oxx, rec.group_whose), (118, 119));
        assert_eq!((rec.group_id, rec.group_form_id), (120, 121));
        assert_eq!((rec.group_angle, rec.in_group), (122, 123));

        let mut published = crate::order::OrderList::new();
        publish(&executable, &mut published);
        assert_eq!(published.current().unwrap(), &order);
    }

    #[test]
    fn a_run_through_the_bridge_advances_a_world_shaped_order_list() {
        // The integration this module exists to make possible: a `crate::order::OrderList`
        // goes in, the derived driver runs, and the list comes back one order shorter.
        let mut list = crate::order::OrderList::new();
        list.push(Order::move_to(24, 24, 96));
        list.push(Order::attack(1, 3));
        assert_eq!(list.len(), 2);

        let mut w = TestWorld::open(16).with_object(1, 3, tgt(24, 24, 0));
        let mut pf = PathFinder::new();
        let mut cov = DispatchCoverage::default();
        let mut u = UnitWork::at(0, 0, 24, 24);
        u.tolerance = 96;
        u.orders = adopt(&list);

        w.frame = 1;
        let r = work(&mut u, &mut w, &mut pf, &mut cov);
        assert!(matches!(
            r.result,
            ArmResult::Retired(KillReason::Completed)
        ));
        publish(&u.orders, &mut list);
        assert_eq!(list.len(), 1);
        assert_eq!(list.order_type(), OrderIndex::Attack);
    }

    #[test]
    fn move_like_membership_matches_the_four_agreeing_call_sites() {
        for k in OrderIndex::ALL {
            assert_eq!(
                is_move_like(k),
                MOVE_LIKE.contains(&k),
                "MOVE_LIKE and is_move_like disagree at {k}"
            );
        }
        assert_eq!(MOVE_LIKE.len(), 7);
        assert!(!is_move_like(OrderIndex::Attack));
        assert!(!is_move_like(OrderIndex::Patrol));
        assert!(is_move_like(OrderIndex::FleeTo));
    }

    #[test]
    fn the_unused_order_flag_constant_is_still_the_one_order_rs_defines() {
        // ORDER_MORE_WORK is not read by this driver; the assertion pins the bit so a
        // sibling lane renumbering `crate::order` breaks here rather than silently.
        assert_eq!(ORDER_MORE_WORK, 16);
        assert_eq!(ORDER_GROUP, 4);
        assert_eq!(ORDER_PATHED, 1);
    }
}
