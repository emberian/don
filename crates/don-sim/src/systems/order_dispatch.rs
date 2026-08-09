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
//! * 21 of the 28 `do_job` arms. They dispatch and are counted; see [`ARMS`].

use crate::command::QueuePos;
use crate::order::{ArmStatus, Order, OrderIndex, NUM_UNIT_ORDERS, ORDER_GROUP, ORDER_PATHED};
use crate::systems::groups_guys::{GuyData, GuyEnv, UnitTypeStats};
use crate::systems::movement::{
    self, vector_dist, Body, MoveStep, MoveTurnProfile, PathData, PathFinder, PathStack, PathUnit,
    SearchArgs, SearchResult, UPathOutcome, UnitWorld,
};
use crate::systems::patrol::{
    self, AirPatrolAction, AirPatrolAfterPhysics, AirPatrolOrder, AirPatrolTarget,
    GroundPatrolAction, GroupMoveRequest, GroupPatrolOrder, StrafeOrder,
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

    // ---- TargetOrder ----
    /// `TargetOrder::ox` at `+8` — the target's index in its owner's object band.
    pub target_o: i32,
    /// `TargetOrder::whom` at `+12` — the target's owner slot.
    pub target_who: i32,
    /// `TargetOrder::uid` at `+16` — `ObjectData::uid`, the staleness token. A mismatch is
    /// what `Unit::work`'s tail treats as "the target you named is not there any more".
    pub target_uid: u16,

    /// Concrete fields carried only by the three order classes patrol creates or executes.
    /// Retail stores these in dynamically-sized class instances; keeping the payload on the
    /// queue node preserves the same per-order ownership and permits routes of any length.
    pub patrol_payload: PatrolPayload,
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
            target_o: -1,
            target_who: -1,
            target_uid: 0,
            patrol_payload: PatrolPayload::None,
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

    pub fn of_kind(kind: OrderIndex) -> OrderRec {
        OrderRec {
            kind,
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
        OrderRec {
            kind: o.kind,
            flags: o.flags,
            x: o.x,
            y: o.y,
            dest_x: o.x,
            dest_y: o.y,
            tolerance: o.tolerance,
            target_o: o.target_o as i32,
            target_who: o.target_who as i32,
            ..OrderRec::default()
        }
    }
}

impl From<OrderRec> for Order {
    /// Narrow back to the descriptive form, for anything that speaks
    /// [`crate::order::OrderList`] (e.g. [`crate::world::World::issue`]).
    fn from(r: OrderRec) -> Order {
        Order {
            kind: r.kind,
            flags: r.flags,
            x: r.x,
            y: r.y,
            target_who: r.target_who.clamp(i8::MIN as i32, i8::MAX as i32) as i8,
            target_o: r.target_o.clamp(i16::MIN as i32, i16::MAX as i32) as i16,
            tolerance: r.tolerance,
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
    /// `UnitData::group` `+128`; `< 0` means ungrouped.
    pub group: i16,
    /// `UnitData::inside_up` `+130`; `< 0` means not garrisoned.
    pub inside_up: i16,
    /// `UnitData::collide_frame` `+72`.
    pub collide_frame: i32,
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
            group: -1,
            inside_up: -1,
            collide_frame: i32::MIN / 2,
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
    ) -> Option<AirPatrolTarget> {
        None
    }

    /// The mod-32 `ObjectsData::find_building_at(..., SearchIndexBH(3), actor.who, 0, 0)`
    /// boundary, including the building type's owner-target bit in the returned record.
    fn air_patrol_building_target(
        &mut self,
        _actor: &UnitWork,
        _order: &AirPatrolOrder,
        _waypoint_x: i32,
        _waypoint_y: i32,
    ) -> Option<AirPatrolTarget> {
        None
    }

    /// `Group::action_move_to(x,y,QUEUE_FIRST,0,0,ATTACK_TO,0,-1,-1,0)` from the grouped
    /// arm of `Unit::do_patrol`.
    fn group_patrol_move(&mut self, actor: &mut UnitWork, request: GroupMoveRequest);

    /// Virtual `ObjectData::is(TypeIndex, strict)`, used for BOMBER/FIGHTERBOMBER patrol
    /// search selection. Upgrade-line membership belongs to the type system, not this arm.
    fn patrol_actor_is_type(&self, actor: &UnitWork, type_id: i32, strict: bool) -> bool;

    /// The trailing contained-aircraft predicate and singleton `Group::action_scramble`.
    fn patrol_inside_is_scramblable(&self, actor_who: u8, inside_o: i16) -> bool;
    fn patrol_scramble_inside(&mut self, actor: &mut UnitWork, inside_o: i16);

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
    ArmStatus::Unimplemented,   //  2 ATTACK_TO       Unit::do_attack_to 0x005F2320
    ArmStatus::Unimplemented,   //  3 EXPLORE_TO      Unit::do_explore_to 0x005F24A0
    ArmStatus::Implemented,     //  4 FLEE_TO         Unit::do_move -- SAME ARM as MOVE_TO
    ArmStatus::FaithfullyEmpty, //  5 PATROL          no case label; falls to the default
    ArmStatus::Unimplemented,   //  6 BUILD_AT        Unit::do_build 0x005EEBF0
    ArmStatus::Implemented,     //  7 GATHER          Unit::do_gather 0x005EF2A0
    ArmStatus::Unimplemented,   //  8 BOARD_SHIP      Unit::do_board 0x005ED1F0
    ArmStatus::Unimplemented,   //  9 AWAIT_BOARD     Unit::do_await_board 0x005ED040
    ArmStatus::Implemented,     // 10 ATTACK          Unit::do_attack 0x005F1B80
    ArmStatus::Unimplemented,   // 11 FOLLOW          Unit::do_follow 0x005E65D0
    ArmStatus::Unimplemented,   // 12 GUARD           Unit::do_guard 0x005E5C70
    ArmStatus::Unimplemented,   // 13 REPAIR          Unit::do_repair 0x005EE420
    ArmStatus::Unimplemented,   // 14 CAST_SPELL      Unit::do_cast 0x005EBFE0
    ArmStatus::Unimplemented,   // 15 TRADE_ROUTE     Unit::do_trade 0x005ED270
    ArmStatus::Unimplemented,   // 16 STRAFE          Unit::do_strafe 0x005EAB00
    ArmStatus::Implemented,     // 17 AIR_PATROL      Unit::do_air_patrol 0x005EA620
    ArmStatus::Unimplemented,   // 18 CHANGE_FORM     Unit::do_form_change 0x005E8670
    ArmStatus::Unimplemented,   // 19 GROUP_MOVE      Unit::do_group_move 0x005E79A0
    ArmStatus::Unimplemented,   // 20 GROUP_ATTACK    Unit::do_group_attack 0x005E75A0
    ArmStatus::Unimplemented,   // 21 GROUP_ATTACK_TO Unit::do_group_attack_to 0x005E74E0
    ArmStatus::Implemented,     // 22 GROUP_PATROL    Unit::do_patrol 0x005F1910
    ArmStatus::Unimplemented,   // 23 ATTACK_GROUND   Unit::do_attack_ground 0x005F1410
    ArmStatus::Unimplemented,   // 24 AIR_ATK_GROUND  Unit::do_air_attack_ground 0x005EA420
    ArmStatus::Unimplemented,   // 25 SPECIAL_ANIM    Unit::do_spec_anim 0x005E5880
    ArmStatus::Unimplemented,   // 26 GARRISON        Unit::do_garrison 0x005E6B80
    ArmStatus::Unimplemented,   // 27 THINK           Unit::do_think_order 0x005E5BF0
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

pub fn do_move<W: WorkWorld>(
    u: &mut UnitWork,
    w: &mut W,
    pf: &mut PathFinder,
    cov: &mut DispatchCoverage,
) -> ArmResult {
    let Some(ord) = update_order(u) else {
        return ArmResult::NoOrder;
    };

    // `UnitData::openlist` is serviced at `0x005F7BDD`, before MoveOrder::timer and the rest
    // of `do_move`. `find_upath_restore` continues the same trees with its smaller per-frame
    // budget; it does not restart from the unit's new position. [measured]
    if u.parked_search {
        let outcome = find_path(pf, w, u, ord.x, ord.y, 0);
        if let Some(done) = apply_move_path_outcome(outcome, u, w, pf, cov) {
            return done;
        }
    }

    // 1. the timer.
    if ord.timer > 0 {
        if ord.timer == 1 {
            kill_current_order(u, KillReason::Completed);
            cov.completed += 1;
            return ArmResult::Retired(KillReason::Completed);
        }
        if let Some(c) = u.orders.front_mut() {
            c.timer -= 1;
        }
        return ArmResult::Working;
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
        return ArmResult::Working;
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
            return ArmResult::Working;
        }
        if let Some(c) = u.orders.front_mut() {
            c.flags &= !ORDER_PATHED;
        }
        u.unit_masks &= !masks::ARRIVED_FACING;
        kill_current_order(u, KillReason::Completed);
        cov.completed += 1;
        u.idle = 1;
        return ArmResult::Retired(KillReason::Completed);
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
            return done;
        }
    }

    // `MoveOrder+0x18` at `0x005F8C88`: every non-zero pause is decremented and holds the
    // unit for this frame. The extra retail branches only choose animation/idle side effects;
    // all of them return before `move_step`. Collision resolution writes this field.
    if ord.pause != 0 {
        if let Some(c) = u.orders.front_mut() {
            c.pause = c.pause.wrapping_sub(1);
        }
        return ArmResult::Working;
    }

    // 5. integration.
    let target = u
        .path
        .peek()
        .map(|r| (r.to_x, r.to_y))
        .unwrap_or((dest.0, dest.1));
    let speed = u.myspeed.max(1) as i32;
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
        MoveStep::TurnedOnly => ArmResult::Turned,
        MoveStep::Moved => ArmResult::Moved,
        MoveStep::Blocked => ArmResult::Blocked,
        MoveStep::Arrived => ArmResult::Working,
        MoveStep::InvalidTerrain => {
            // `0x005FB76E`: an invalid terrain transition clears UnitData mask bit 8 before
            // returning. The next `do_move` therefore rebuilds the route; retaining the old
            // bit/path here produced a permanent retry of the same invalid waypoint.
            u.unit_masks &= !masks::PATH_EXHAUSTED;
            ArmResult::Blocked
        }
        // The four out-of-bounds exits at `0x005FB4E8..0x005FB52F` return without clearing
        // bit 8, unlike the later invalid-terrain exit.
        MoveStep::Refused => ArmResult::Blocked,
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

    let mut building_target = None;
    if !u.type_is_animal && phase % 32 == 0 {
        let cursor = order.points.clamp_air_cursor();
        let point = (order.points.x[cursor], order.points.y[cursor]);
        let (sx, sy) = relative_scan_point(point, home, max_x, max_y, fighter_bomber);
        building_target = w.air_patrol_building_target(u, &order, sx, sy);
    }

    let input = AirPatrolAfterPhysics {
        actor_x: u.body.x,
        actor_y: u.body.y,
        actor_o: u.o,
        frame,
        is_animal: u.type_is_animal,
        spell_time: u.spell_time,
        order_list_len: u.orders.len(),
        unit_target,
        building_target,
    };
    let action = patrol::step_air_patrol_after_physics(&mut order, flight_target, &input);

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

/// `Unit::do_job(enum OrderIndex, class UnitOrder*)` `0x00617A10`, the 28-entry jump table at
/// `0x00617B94`. [measured — the `switch` has 27 case labels; `PATROL` (5) has none.]
///
/// Every arm is present. The 21 without a ported body dispatch, are counted, and return
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
        OrderIndex::Attack => do_attack(u, w, cov),
        OrderIndex::Gather => do_gather(u, w, cov),
        OrderIndex::AirPatrol => do_air_patrol(u, w),
        OrderIndex::GroupPatrol => do_group_patrol(u, w),
        // Arm 5 has no case label. Doing nothing here is faithful, not missing.
        OrderIndex::Patrol => ArmResult::Empty,
        _ => {
            debug_assert_eq!(status, ArmStatus::Unimplemented);
            ArmResult::NotPorted
        }
    }
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
        u.safe -= 1;
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
/// **Lossy in one direction, stated plainly:** [`crate::order::Order`] has no `target_uid`,
/// no `timer` and no `retry`, so a round trip through it drops the staleness token and the
/// retry state machine. Keep [`OrderQueue`] as the owning representation and use this only at
/// the boundary.
pub fn adopt(list: &crate::order::OrderList) -> OrderQueue {
    let mut q = OrderQueue::new();
    for o in list.iter() {
        q.push_back(OrderRec::from(*o));
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
    fn this_dispatcher_handles_eight_of_the_twenty_eight_arms() {
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
        // Seven implemented, including the two live patrols; PATROL remains faithfully empty.
        assert_eq!((implemented, empty, absent), (7, 1, 20));
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
            owner_target_bit: true,
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
        // 20 unimplemented arms, each hit once. AIR_PATROL and GROUP_PATROL are live.
        assert_eq!(cov.unimplemented, 20);
        assert!((cov.covered_fraction() - 8.0 / 28.0).abs() < 1e-12);
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
        // uid does not survive the narrow form; that is why OrderRec exists.
        assert_eq!(back.target_uid, 0);
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
