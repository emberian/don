//! `OrderIndex`, the order list, and `Unit::do_job`'s 28-entry jump table.
//!
//! # The two layers, which are easy to conflate
//!
//! A **command** (`CommandPackage`, ~70 `issue_*`, 82 opcodes) is player intent: it goes
//! over the wire once and is lockstep-ordered. An **order** (`UnitOrder` subclass, 27
//! concrete types plus `NONE`) is per-unit execution state that lives for many frames.
//! `issue_move_to` installs a `MoveOrder` on every selected unit; that order is then
//! advanced once per tick by `Unit::work` -> `Unit::do_job`. This module is the *order*
//! layer only.
//!
//! # Provenance
//!
//! The names and indices are `OrderNames`, a `String[]` at `0x00ECEBA0` built by the
//! dynamic initializer at `0x00405E50` (orders.cpp:51), read in index order [measured].
//! The PDB carries the same enum as `OrderIndex` with `NUM_UNIT_ORDERS = 28` and the
//! flag bits below. The executors are the jump table at `0x00617B94`, indexed directly
//! by the order type, reached from `Unit::do_job` `0x00617A10` [measured].
//!
//! Two entries in that table are surprises, and both are reproduced here:
//!
//! * **`FLEE_TO` and `MOVE_TO` share one executor** — `Unit::do_move` `0x005F7B30`.
//! * **`PATROL` (5) falls through to the default arm and does nothing.** Every patrol in
//!   the game is a `GROUP_PATROL` (22), which `Unit::do_patrol` `0x005F1910` serves.
//!   A port that "helpfully" implemented `PATROL` would diverge from retail.

use crate::Handle;
use std::fmt;

/// `NUM_UNIT_ORDERS` from the PDB.
pub const NUM_UNIT_ORDERS: usize = 28;

/// `OrderIndex`, in the order `OrderNames` `0x00ECEBA0` lists them.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(u8)]
pub enum OrderIndex {
    None = 0,
    MoveTo = 1,
    AttackTo = 2,
    ExploreTo = 3,
    FleeTo = 4,
    Patrol = 5,
    BuildAt = 6,
    Gather = 7,
    BoardShip = 8,
    AwaitBoard = 9,
    Attack = 10,
    Follow = 11,
    Guard = 12,
    Repair = 13,
    CastSpell = 14,
    TradeRoute = 15,
    Strafe = 16,
    AirPatrol = 17,
    ChangeForm = 18,
    GroupMove = 19,
    GroupAttack = 20,
    GroupAttackTo = 21,
    GroupPatrol = 22,
    AttackGround = 23,
    AirAttackGround = 24,
    SpecialAnim = 25,
    Garrison = 26,
    Think = 27,
}

/// `OrderNames`, index-aligned.
pub const ORDER_NAMES: [&str; NUM_UNIT_ORDERS] = [
    "NONE",
    "MOVE_TO",
    "ATTACK_TO",
    "EXPLORE_TO",
    "FLEE_TO",
    "PATROL",
    "BUILD_AT",
    "GATHER",
    "BOARD_SHIP",
    "AWAIT_BOARD",
    "ATTACK",
    "FOLLOW",
    "GUARD",
    "REPAIR",
    "CAST_SPELL",
    "TRADE_ROUTE",
    "STRAFE",
    "AIR_PATROL",
    "CHANGE_FORM",
    "GROUP_MOVE",
    "GROUP_ATTACK",
    "GROUP_ATTACK_TO",
    "GROUP_PATROL",
    "ATTACK_GROUND",
    "AIR_ATTACK_GROUND",
    "SPECIAL_ANIM",
    "GARRISON",
    "THINK",
];

impl OrderIndex {
    pub const ALL: [OrderIndex; NUM_UNIT_ORDERS] = [
        OrderIndex::None,
        OrderIndex::MoveTo,
        OrderIndex::AttackTo,
        OrderIndex::ExploreTo,
        OrderIndex::FleeTo,
        OrderIndex::Patrol,
        OrderIndex::BuildAt,
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
        OrderIndex::AirPatrol,
        OrderIndex::ChangeForm,
        OrderIndex::GroupMove,
        OrderIndex::GroupAttack,
        OrderIndex::GroupAttackTo,
        OrderIndex::GroupPatrol,
        OrderIndex::AttackGround,
        OrderIndex::AirAttackGround,
        OrderIndex::SpecialAnim,
        OrderIndex::Garrison,
        OrderIndex::Think,
    ];

    #[inline]
    pub fn index(self) -> usize {
        self as usize
    }

    #[inline]
    pub fn name(self) -> &'static str {
        ORDER_NAMES[self.index()]
    }

    pub fn from_index(i: usize) -> Option<OrderIndex> {
        OrderIndex::ALL.get(i).copied()
    }
}

impl fmt::Display for OrderIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

// `UnitOrder::flags`, a `char` at +4 [measured, PDB].
pub const ORDER_PATHED: u8 = 1;
pub const ORDER_FLEEING: u8 = 2;
pub const ORDER_GROUP: u8 = 4;
pub const ORDER_DEFENSIVE: u8 = 8;
pub const ORDER_MORE_WORK: u8 = 16;
pub const ORDER_DISEMBARK: u8 = 32;
pub const ORDER_PUSHED: u8 = 64;
pub const ORDER_FACING_TARGET: u8 = 128;

/// Complete concrete payload of `FollowOrder` (`sizeof=44`).
///
/// `TargetOrder` supplies the primary `(ox, whom, uid)` at `+0x08..+0x10`; FOLLOW adds
/// the secondary containment/fallback identity at `+0x14..+0x1C`. The descriptive
/// [`Order`] keeps the full-width fields together so a bridge round trip cannot truncate
/// or silently discard the identity used by `Unit::do_follow`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FollowOrderPayload {
    pub ox: i32,
    pub whom: i32,
    pub uid: u16,
    pub oxx: i32,
    pub whose: i32,
    pub uid2: u16,
}

/// `SpecialType`, the four-byte discriminator at `SpecialAnimOrder+0x08`.
///
/// `UnitData::is_entering_or_exiting` `0x0060A6F0` returns true for the first two values
/// and false for `SPECIAL_UNIT`. Keeping this enum on the order is required because those
/// branches differ even though all three orders report [`OrderIndex::SpecialAnim`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum SpecialAnimType {
    Enter = 0,
    Exit = 1,
    Unit = 2,
}

impl SpecialAnimType {
    #[inline]
    pub const fn from_raw(value: i32) -> Option<Self> {
        match value {
            0 => Some(Self::Enter),
            1 => Some(Self::Exit),
            2 => Some(Self::Unit),
            _ => None,
        }
    }

    #[inline]
    pub const fn is_entering_or_exiting(self) -> bool {
        matches!(self, Self::Enter | Self::Exit)
    }
}

/// The complete walked payload of `SpecialAnimOrder` `sizeof=44` after its eight-byte
/// `UnitOrder` base. Defaults are from `SpecialAnimOrder::clear` `0x00484EC0`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpecialAnimOrderState {
    pub special_type: SpecialAnimType,
    pub started: i32,
    pub frames: i32,
    pub data1: i32,
    pub data2: i32,
    pub data3: i32,
    pub data4: i32,
    pub ox: i32,
    pub whom: i32,
}

/// The checksum-visible fields unique to `FormOrder` (`sizeof=100`).
///
/// `angle` is inherited from `MoveOrder` at concrete `+0x0C`; `new_form` and `delay` are
/// the two `FormOrder` words at `+0x50/+0x54`.  The executor does not read `delay`, but the
/// order object owns and walks it, so the compact order representation must not discard it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FormOrderState {
    pub angle: i32,
    pub new_form: i32,
    pub delay: i32,
}

/// Complete scalar state of the executable `MoveOrder` / `GroupOrder` bases.
///
/// The production world historically retained only `x`, `y`, and `tolerance`, while the
/// command bridge and retail order walker also own the retry state machine, facing/origin
/// coordinates, and group-order identity.  Keeping this payload on [`Order`] makes the
/// command, tick, and save owners lossless without forcing a descriptive non-movement order
/// to acquire meaningless move fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MoveOrderState {
    pub angle: i32,
    pub dest: i32,
    pub pause: i32,
    pub retry: i32,
    pub attempts: i32,
    pub timer: i32,
    pub facing: i32,
    pub dest_x: i32,
    pub dest_y: i32,
    pub last_x: i32,
    pub last_y: i32,
    pub coll_x: i32,
    pub coll_y: i32,
    pub orig_x: i32,
    pub orig_y: i32,
    pub off_x: i16,
    pub off_y: i16,
    pub group_oxx: i32,
    pub group_whose: i32,
    pub group_id: i32,
    pub group_form_id: i32,
    pub group_angle: i32,
    pub in_group: i32,
}

impl Default for MoveOrderState {
    fn default() -> Self {
        Self {
            angle: 0,
            dest: 0,
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
        }
    }
}

impl MoveOrderState {
    /// Constructor image shared by `Unit::add_move_*_order` before action-specific writes.
    pub fn fresh(dest_x: i32, dest_y: i32) -> Self {
        Self {
            dest_x,
            dest_y,
            ..Self::default()
        }
    }
}

impl Default for SpecialAnimOrderState {
    fn default() -> Self {
        Self {
            special_type: SpecialAnimType::Unit,
            started: 0,
            frames: 0,
            data1: -1,
            data2: -1,
            data3: -1,
            data4: -1,
            ox: -1,
            whom: -1,
        }
    }
}

/// How faithfully this port implements one jump-table arm.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArmStatus {
    /// The arm's *observable behaviour* is implemented from a derivation.
    Implemented,
    /// Retail's arm genuinely does nothing (the default arm, or an empty stub).
    /// Doing nothing here is faithful, not missing.
    FaithfullyEmpty,
    /// Recognised and dispatched, but the executor's body is not ported. Hitting this
    /// increments a counter instead of pretending to act.
    Unimplemented,
}

/// One arm of the `0x00617B94` jump table.
#[derive(Clone, Copy, Debug)]
pub struct Executor {
    pub order: OrderIndex,
    /// The retail function this arm jumps to, or `None` for the default arm.
    pub va: Option<&'static str>,
    pub symbol: &'static str,
    pub status: ArmStatus,
    pub note: &'static str,
}

/// `Unit::do_job` `0x00617A10`, jump table at `0x00617B94`, indexed by `OrderIndex`
/// [measured]. Two callers only: `Unit::work` and `Animal::work` `0x005D7330`.
pub const EXECUTORS: [Executor; NUM_UNIT_ORDERS] = [
    Executor { order: OrderIndex::None, va: None, symbol: "virtual [unit+0x184] (do_idle)",
        status: ArmStatus::Implemented, note: "no order: the unit holds position" },
    Executor { order: OrderIndex::MoveTo, va: Some("0x005F7B30"), symbol: "Unit::do_move",
        status: ArmStatus::Implemented, note: "unit.cpp:15757, the locomotion funnel" },
    Executor { order: OrderIndex::AttackTo, va: Some("0x005F2320"), symbol: "Unit::do_attack_to",
        status: ArmStatus::Implemented,
        note: "exact do_move then 15-frame target/army/pause wrapper; mandatory transactional host" },
    Executor { order: OrderIndex::ExploreTo, va: Some("0x005F24A0"), symbol: "Unit::do_explore_to",
        status: ArmStatus::Implemented,
        note: "exact do_move then 15-frame identity/on-map explore tail; mandatory typed host receipt" },
    Executor { order: OrderIndex::FleeTo, va: Some("0x005F7B30"), symbol: "Unit::do_move",
        status: ArmStatus::Implemented, note: "SAME ARM as MOVE_TO [measured]" },
    Executor { order: OrderIndex::Patrol, va: None, symbol: "(default arm)",
        status: ArmStatus::FaithfullyEmpty,
        note: "OrderIndex::PATROL falls to the default arm and does nothing; live patrols are GROUP_PATROL" },
    Executor { order: OrderIndex::BuildAt, va: Some("0x005EEBF0"), symbol: "Unit::do_build",
        status: ArmStatus::Implemented,
        note: "exact builder head/retirement order; mandatory animation, reswarm, placement and lifecycle host" },
    Executor { order: OrderIndex::Gather, va: Some("0x005EF2A0"), symbol: "Unit::do_gather",
        status: ArmStatus::Unimplemented, note: "economy lane owns the gather rates" },
    Executor { order: OrderIndex::BoardShip, va: Some("0x005ED1F0"), symbol: "Unit::do_board",
        status: ArmStatus::Implemented,
        note: "exact rendezvous/retire/can_carry/go_inside transaction; world effects are mandatory callbacks" },
    Executor { order: OrderIndex::AwaitBoard, va: Some("0x005ED040"), symbol: "Unit::do_await_board",
        status: ArmStatus::Implemented,
        note: "exact two-pass reverse-link validation and asymmetric passenger cancellation" },
    Executor { order: OrderIndex::Attack, va: Some("0x005F1B80"), symbol: "Unit::do_attack",
        status: ArmStatus::Implemented,
        note: "unit.cpp:20156; range/recharge gate is ours, the damage arithmetic is the derived pipeline" },
    Executor { order: OrderIndex::Follow, va: Some("0x005E65D0"), symbol: "Unit::do_follow",
        status: ArmStatus::Implemented,
        note: "exact containment/fallback transaction, three-search close and same-tick do_move" },
    Executor { order: OrderIndex::Guard, va: Some("0x005E5C70"), symbol: "Unit::do_guard",
        status: ArmStatus::Implemented,
        note: "exact eleven-branch guard-post/idle/retry transaction; owns the QueuePos::First move insertion and same-tick do_move; mandatory typed host receipt. NOT a do_move delegate: one of 30 direct calls" },
    Executor { order: OrderIndex::Repair, va: Some("0x005EE420"), symbol: "Unit::do_repair",
        status: ArmStatus::Implemented,
        note: "exact planner with one mandatory atomic WorkWorld snapshot/commit receipt" },
    Executor { order: OrderIndex::CastSpell, va: Some("0x005EBFE0"), symbol: "Unit::do_cast",
        status: ArmStatus::Unimplemented, note: "" },
    Executor { order: OrderIndex::TradeRoute, va: Some("0x005ED270"), symbol: "Unit::do_trade",
        status: ArmStatus::Unimplemented, note: "" },
    Executor { order: OrderIndex::Strafe, va: Some("0x005EAB00"), symbol: "Unit::do_strafe",
        status: ArmStatus::Unimplemented, note: "aircraft" },
    Executor { order: OrderIndex::AirPatrol, va: Some("0x005EA620"), symbol: "Unit::do_air_patrol",
        status: ArmStatus::Implemented,
        note: "dynamic waypoints, air-physics boundary, 16/32-frame scans and STRAFE insertion" },
    Executor { order: OrderIndex::ChangeForm, va: Some("0x005E8670"), symbol: "Unit::do_form_change",
        status: ArmStatus::Implemented,
        note: "exact atomic form/angle/retirement/idle transaction" },
    Executor { order: OrderIndex::GroupMove, va: Some("0x005E79A0"), symbol: "Unit::do_group_move",
        status: ArmStatus::Implemented,
        note: "leader/follower orchestration; mandatory transactional host for Groups/object/terrain facts" },
    Executor { order: OrderIndex::GroupAttack, va: Some("0x005E75A0"), symbol: "Unit::do_group_attack",
        status: ArmStatus::Implemented,
        note: "exact leader/follower orchestration; mandatory transactional group/target host" },
    Executor { order: OrderIndex::GroupAttackTo, va: Some("0x005E74E0"), symbol: "Unit::do_group_attack_to",
        status: ArmStatus::Implemented,
        note: "exact wrapper; mandatory preflighted fight/do_attack_to_pause world callbacks" },
    Executor { order: OrderIndex::GroupPatrol, va: Some("0x005F1910"), symbol: "Unit::do_patrol",
        status: ArmStatus::Implemented,
        note: "the live ground patrol arm; inserts ATTACK_TO or delegates Group::action_move_to" },
    Executor { order: OrderIndex::AttackGround, va: Some("0x005F1410"), symbol: "Unit::do_attack_ground",
        status: ArmStatus::Implemented,
        note: "exact range/facing/recharge/fire/reposition transaction; mandatory typed host receipt" },
    Executor { order: OrderIndex::AirAttackGround, va: Some("0x005EA420"), symbol: "Unit::do_air_attack_ground",
        status: ArmStatus::Implemented,
        note: "exact post-air-physics angle/fire/recharge/mana transaction; mandatory typed host receipt" },
    Executor { order: OrderIndex::SpecialAnim, va: Some("0x005E5880"), symbol: "Unit::do_spec_anim",
        status: ArmStatus::Unimplemented,
        note: "production frame reaches UNIT and object-free EXIT; target relations/external world tails remain typed-unavailable" },
    Executor { order: OrderIndex::Garrison, va: Some("0x005E6B80"), symbol: "Unit::do_garrison",
        status: ArmStatus::Implemented,
        note: "exact fifteen-branch admission/approach/capacity/entry transaction; local bare kill, observed containment-chain retirement, mandatory typed host receipt" },
    Executor { order: OrderIndex::Think, va: Some("0x005E5BF0"), symbol: "Unit::do_think_order",
        status: ArmStatus::Implemented,
        note: "exact atomic retirement and four-type think_peasant tail" },
];

/// Complete identity retained by a target-taking authoritative order.
///
/// `(who,o,uid)` is retail's `TargetOrder` staleness contract.  `handle` is additive port
/// identity: it survives dense-row compaction and rejects a recycled object id before a
/// production executor can act on a reused owner-array slot.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OrderTargetIdentity {
    pub handle: Handle,
    pub who: i8,
    pub o: i16,
    pub uid: u16,
}

/// Failure to adopt a checksum-complete STRAFE payload into the flattened canonical queue.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StrafeOrderAdoptionError {
    Authority(crate::systems::strafe_runtime_authority::StrafeAuthorityError),
    TargetOutOfRange { o: i32, who: i32 },
}

impl From<crate::systems::strafe_runtime_authority::StrafeAuthorityError>
    for StrafeOrderAdoptionError
{
    fn from(error: crate::systems::strafe_runtime_authority::StrafeAuthorityError) -> Self {
        Self::Authority(error)
    }
}

/// One `UnitOrder`, flattened.
///
/// The retail hierarchy is 31 classes with a virtual `get_type()`, a virtually-inherited
/// `UnitOrder` subobject at the *tail* of each object, and 30 `update_<X>_order` slots.
/// None of that shape is load-bearing for us; the *fields* are, and this is their union.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Order {
    /// `RecycledOrderNode::metric`, walked before the concrete order in DoNSave v13.
    /// It belongs to this flattened queue node, not to any retail `UnitOrder` subclass.
    pub node_metric: u8,
    pub kind: OrderIndex,
    /// `UnitOrder::flags`, a `char` at +4. See the `ORDER_*` bits.
    pub flags: u8,
    /// Destination in world Coord units (`TargetOrder`-shaped orders).
    pub x: i32,
    pub y: i32,
    /// Target object, as the engine addresses one: owner slot plus index in that
    /// owner's list. `who < 0` means "no target".
    pub target_who: i8,
    pub target_o: i16,
    /// Retail `TargetOrder::uid`, the incarnation token paired with `(target_who,target_o)`.
    pub target_uid: u16,
    /// Additive compaction-stable port identity for the target. `None` denotes an older or
    /// descriptive order whose caller did not prove the authoritative Handle; ATTACK
    /// admission must not manufacture one from the retail scalar fields.
    pub target_handle: Option<Handle>,
    /// Arrival tolerance in Coord units; `UnitData::tolerance` is the per-unit default.
    pub tolerance: i32,
    /// Complete executable `MoveOrder`/`GroupOrder` scalar image.  `None` is accepted only
    /// as a legacy descriptive order; authoritative movement command installation publishes
    /// `Some` and DoNSave v12 preserves it byte-for-byte.
    pub move_state: Option<MoveOrderState>,
    /// Concrete payload for order 11. `None` on FOLLOW is malformed legacy state: the
    /// executor must fail closed rather than inventing its fallback identity.
    pub follow: Option<FollowOrderPayload>,
    /// Concrete payload for order 25. `None` on a `SpecialAnim` order is malformed legacy
    /// state and must fail any caller which needs the subtype rather than guessing UNIT.
    pub special_anim: Option<SpecialAnimOrderState>,
    /// Concrete payload for order 18. `None` on `CHANGE_FORM` is malformed: neither the
    /// form byte nor final angle may be guessed by the executor.
    pub form_order: Option<FormOrderState>,
    /// Complete walked payload of `AirPatrolOrder`, including both dynamic-array metadata
    /// records and the secondary `AirOrder` base. `Some` is valid only for AIR_PATROL.
    pub air_patrol: Option<crate::systems::air_runtime_authority::AirPatrolOrderPayload>,
    /// Exact walked `StrafeOrder` payload. Target identity is duplicated in the generic
    /// header and must agree; `Some` is valid only for STRAFE.
    pub strafe: Option<crate::systems::patrol::StrafeOrder>,
    /// Complete checksum-visible `GuardOrder` payload. The flattened target header is a
    /// second view of the same identity and must agree with `guard.target`.
    pub guard: Option<crate::systems::guard_order::GuardOrderState>,
    /// Exact economy-order suffix for BOARD_SHIP/AWAIT_BOARD/REPAIR/GATHER/CAST_SPELL/
    /// TRADE_ROUTE. Target-only variants are explicit so a foreign tag-0 order cannot be
    /// mistaken for a recovered economy node.
    pub economy: Option<crate::systems::economy_order_payload_authority::EconomyOrderPayload>,
}

impl Default for Order {
    fn default() -> Order {
        Order {
            node_metric: 0,
            kind: OrderIndex::None,
            flags: 0,
            x: 0,
            y: 0,
            target_who: -1,
            target_o: -1,
            target_uid: 0,
            target_handle: None,
            tolerance: 0,
            move_state: None,
            follow: None,
            special_anim: None,
            form_order: None,
            air_patrol: None,
            strafe: None,
            guard: None,
            economy: None,
        }
    }
}

impl Order {
    pub fn move_to(x: i32, y: i32, tolerance: i32) -> Order {
        Order {
            kind: OrderIndex::MoveTo,
            x,
            y,
            tolerance,
            move_state: Some(MoveOrderState::fresh(x, y)),
            ..Order::default()
        }
    }

    pub fn attack(target_who: i8, target_o: i16) -> Order {
        Order {
            kind: OrderIndex::Attack,
            target_who,
            target_o,
            ..Order::default()
        }
    }

    /// Construct an ATTACK whose queue node retains both authoritative and retail identity.
    ///
    /// The duplicated `(target_who,target_o)` fields are the flattened `TargetOrder` view
    /// used by the current production executor.  [`OrderTargetIdentity`] is the non-lossy
    /// transaction payload; consumers must require the two views to agree.
    pub fn attack_exact(target: OrderTargetIdentity) -> Order {
        Order {
            kind: OrderIndex::Attack,
            target_who: target.who,
            target_o: target.o,
            target_uid: target.uid,
            target_handle: Some(target.handle),
            ..Order::default()
        }
    }

    /// Return the complete target identity only when the stable Handle was retained.
    pub fn exact_target_identity(&self) -> Option<OrderTargetIdentity> {
        Some(OrderTargetIdentity {
            handle: self.target_handle?,
            who: self.target_who,
            o: self.target_o,
            uid: self.target_uid,
        })
    }

    /// One exact `FollowOrder`, retaining both identities and UID snapshots.
    pub fn follow(payload: FollowOrderPayload) -> Order {
        Order {
            kind: OrderIndex::Follow,
            flags: ORDER_GROUP,
            target_who: payload.whom.clamp(i8::MIN as i32, i8::MAX as i32) as i8,
            target_o: payload.ox.clamp(i16::MIN as i32, i16::MAX as i32) as i16,
            target_uid: payload.uid,
            follow: Some(payload),
            ..Order::default()
        }
    }

    /// Construct one exact `GuardOrder` node without borrowing the incompatible Move payload.
    pub fn guard(payload: crate::systems::guard_order::GuardOrderState) -> Result<Order, ()> {
        let target_who = i8::try_from(payload.target.who).map_err(|_| ())?;
        let target_o = i16::try_from(payload.target.o).map_err(|_| ())?;
        Ok(Order {
            kind: OrderIndex::Guard,
            flags: ORDER_GROUP,
            target_who,
            target_o,
            target_uid: payload.target.uid,
            guard: Some(payload),
            ..Order::default()
        })
    }

    /// `Unit::add_spec_anim_order(type, data1, data2, QueuePos)` `0x005E4160`.
    pub fn special_anim(special_type: SpecialAnimType, data1: i32, data2: i32) -> Order {
        Order {
            kind: OrderIndex::SpecialAnim,
            flags: ORDER_GROUP,
            special_anim: Some(SpecialAnimOrderState {
                special_type,
                data1,
                data2,
                ..SpecialAnimOrderState::default()
            }),
            ..Order::default()
        }
    }

    /// Construct one exact `FormOrder` payload.
    pub fn change_form(angle: i32, new_form: i32, delay: i32) -> Order {
        let move_state = MoveOrderState {
            angle,
            ..MoveOrderState::default()
        };
        Order {
            kind: OrderIndex::ChangeForm,
            move_state: Some(move_state),
            form_order: Some(FormOrderState {
                angle,
                new_form,
                delay,
            }),
            ..Order::default()
        }
    }

    /// Construct one canonical AIR_PATROL queue node from its complete walked payload.
    pub fn air_patrol(
        payload: crate::systems::air_runtime_authority::AirPatrolOrderPayload,
        group: bool,
    ) -> Result<Order, crate::systems::air_runtime_authority::AirRuntimeAuthorityError> {
        payload.validate()?;
        Ok(Order {
            kind: OrderIndex::AirPatrol,
            flags: if group { ORDER_GROUP } else { 0 },
            air_patrol: Some(payload),
            ..Order::default()
        })
    }

    /// Construct one canonical STRAFE queue node from its complete walked payload.
    pub fn strafe(
        payload: crate::systems::patrol::StrafeOrder,
        group: bool,
    ) -> Result<Order, StrafeOrderAdoptionError> {
        crate::systems::strafe_runtime_authority::validate_strafe_order(&payload)?;
        let target_who = i8::try_from(payload.target_who).map_err(|_| {
            StrafeOrderAdoptionError::TargetOutOfRange {
                o: payload.target_o,
                who: payload.target_who,
            }
        })?;
        let target_o = i16::try_from(payload.target_o).map_err(|_| {
            StrafeOrderAdoptionError::TargetOutOfRange {
                o: payload.target_o,
                who: payload.target_who,
            }
        })?;
        Ok(Order {
            kind: OrderIndex::Strafe,
            flags: if group { ORDER_GROUP } else { 0 },
            target_who,
            target_o,
            target_uid: payload.target_uid,
            strafe: Some(payload),
            ..Order::default()
        })
    }

    /// Narrow one validated v13 economy node into the canonical queue representation.
    /// Full-width retail identities are rejected when the current flattened header cannot
    /// represent them; no clamping or truncation is permitted.
    pub fn economy(
        node: crate::systems::economy_order_payload_authority::EconomyOrderNode,
    ) -> Result<Order, crate::systems::economy_order_payload_authority::EconomyOrderAuthorityError>
    {
        use crate::systems::economy_order_payload_authority::EconomyOrderAuthorityError;

        node.validate()?;
        let target_who = i8::try_from(node.header.primary.who).map_err(|_| {
            EconomyOrderAuthorityError::HeaderTargetOutOfRange {
                o: node.header.primary.o,
                who: node.header.primary.who,
            }
        })?;
        let target_o = i16::try_from(node.header.primary.o).map_err(|_| {
            EconomyOrderAuthorityError::HeaderTargetOutOfRange {
                o: node.header.primary.o,
                who: node.header.primary.who,
            }
        })?;
        Ok(Order {
            node_metric: node.metric,
            kind: node.header.kind,
            flags: node.header.flags,
            x: node.header.x,
            y: node.header.y,
            target_who,
            target_o,
            target_uid: node.header.primary.uid,
            target_handle: node.header.primary.handle,
            economy: Some(node.payload),
            ..Order::default()
        })
    }

    /// Exact result of `UnitData::is_entering_or_exiting` for this flattened order.
    ///
    /// `None` identifies a malformed `SpecialAnim` without its concrete discriminator.
    #[inline]
    pub const fn is_entering_or_exiting(&self) -> Option<bool> {
        if matches!(self.kind, OrderIndex::SpecialAnim) {
            match self.special_anim {
                Some(state) => Some(state.special_type.is_entering_or_exiting()),
                None => None,
            }
        } else {
            Some(false)
        }
    }

    #[inline]
    pub fn has(&self, bit: u8) -> bool {
        self.flags & bit != 0
    }
}

/// `UnitData::orderlist` at +200: a `LinkListBase<UnitOrder*, unsigned char,
/// RecycledOrderNode>` whose head is the current order.
///
/// Represented as a small vector because the linked-list shape only exists to serve
/// `OrdersMemManager`'s recycling, which we do not need — but the *ordering* semantics
/// (front is current, `kill_current_order` pops the front) are preserved, because those
/// are observable.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct OrderList {
    orders: Vec<Order>,
}

impl OrderList {
    pub fn new() -> OrderList {
        OrderList { orders: Vec::new() }
    }

    #[inline]
    pub fn current(&self) -> Option<&Order> {
        self.orders.first()
    }

    #[inline]
    pub fn current_mut(&mut self) -> Option<&mut Order> {
        self.orders.first_mut()
    }

    /// The type `UnitData::order_type` `0x00616E80` would return: the virtual
    /// `get_type()` of the head order, `NONE` when the list is empty.
    #[inline]
    pub fn order_type(&self) -> OrderIndex {
        self.orders.first().map_or(OrderIndex::None, |o| o.kind)
    }

    /// `Unit::add_*_order` — append behind whatever is queued.
    pub fn push(&mut self, o: Order) {
        self.orders.push(o);
    }

    /// Retail `LinkListBase<UnitOrder*>::add` (`0x0046D5A0`): insert immediately before
    /// the current node and publish the new node as current. Economy `QUEUE_LAST` installers
    /// use this primitive; the command chronology, not the vector's tail, defines that name.
    pub fn push_front(&mut self, o: Order) {
        self.orders.insert(0, o);
    }

    /// Replace the whole list, which is what an un-shifted command does.
    pub fn replace(&mut self, o: Order) {
        self.orders.clear();
        self.orders.push(o);
    }

    /// `Unit::kill_current_order` — drop the head; the next order becomes current.
    pub fn kill_current(&mut self) -> Option<Order> {
        if self.orders.is_empty() {
            None
        } else {
            Some(self.orders.remove(0))
        }
    }

    pub fn clear(&mut self) {
        self.orders.clear();
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.orders.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.orders.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Order> {
        self.orders.iter()
    }
}

/// How many times each jump-table arm was entered, and what happened.
///
/// This exists so coverage is **measured, not estimated**. An unimplemented arm still
/// increments its counter, so a run can report "5 of 28 arms carry behaviour, and they
/// absorbed 99.2% of dispatches" rather than a guess.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OrderCoverage {
    pub dispatches: [u64; NUM_UNIT_ORDERS],
    /// Dispatches that landed on an `Unimplemented` arm.
    pub unimplemented: u64,
    pub total: u64,
}

impl OrderCoverage {
    #[inline]
    pub fn record(&mut self, order: OrderIndex) -> ArmStatus {
        let i = order.index();
        self.dispatches[i] += 1;
        self.total += 1;
        let st = EXECUTORS[i].status;
        if st == ArmStatus::Unimplemented {
            self.unimplemented += 1;
        }
        st
    }

    pub fn merge(&mut self, other: &OrderCoverage) {
        for i in 0..NUM_UNIT_ORDERS {
            self.dispatches[i] += other.dispatches[i];
        }
        self.unimplemented += other.unimplemented;
        self.total += other.total;
    }

    /// Fraction of dispatches that reached an arm carrying behaviour.
    pub fn covered_fraction(&self) -> f64 {
        if self.total == 0 {
            return 0.0;
        }
        (self.total - self.unimplemented) as f64 / self.total as f64
    }

    /// Arms with at least one dispatch, most-used first.
    pub fn hot(&self) -> Vec<(OrderIndex, u64)> {
        let mut v: Vec<(OrderIndex, u64)> = OrderIndex::ALL
            .iter()
            .copied()
            .filter(|o| self.dispatches[o.index()] > 0)
            .map(|o| (o, self.dispatches[o.index()]))
            .collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_enum_matches_ordernames_index_for_index() {
        for (i, o) in OrderIndex::ALL.iter().enumerate() {
            assert_eq!(o.index(), i);
            assert_eq!(o.name(), ORDER_NAMES[i]);
            assert_eq!(
                EXECUTORS[i].order, *o,
                "executor table is misaligned at {i}"
            );
        }
        assert_eq!(OrderIndex::ALL.len(), NUM_UNIT_ORDERS);
    }

    /// The two derived surprises. If either of these ever "gets fixed", the port has
    /// silently stopped being a port.
    #[test]
    fn flee_to_shares_move_tos_executor_and_patrol_is_empty() {
        assert_eq!(
            EXECUTORS[OrderIndex::FleeTo.index()].va,
            EXECUTORS[OrderIndex::MoveTo.index()].va
        );
        assert_eq!(EXECUTORS[OrderIndex::Patrol.index()].va, None);
        assert_eq!(
            EXECUTORS[OrderIndex::Patrol.index()].status,
            ArmStatus::FaithfullyEmpty
        );
        assert_ne!(EXECUTORS[OrderIndex::GroupPatrol.index()].va, None);
    }

    #[test]
    fn order_list_head_is_the_current_order() {
        let mut l = OrderList::new();
        assert_eq!(l.order_type(), OrderIndex::None);
        l.push(Order::move_to(10, 20, 4));
        l.push(Order::attack(1, 7));
        assert_eq!(l.order_type(), OrderIndex::MoveTo);
        assert_eq!(l.kill_current().unwrap().kind, OrderIndex::MoveTo);
        assert_eq!(l.order_type(), OrderIndex::Attack);
        l.replace(Order::move_to(1, 1, 0));
        assert_eq!(l.len(), 1);
    }

    #[test]
    fn special_anim_preserves_the_discriminator_and_retail_clear_defaults() {
        let enter = Order::special_anim(SpecialAnimType::Enter, 17, 19);
        let state = enter.special_anim.unwrap();
        assert_eq!(enter.kind, OrderIndex::SpecialAnim);
        assert_eq!(enter.flags & ORDER_GROUP, ORDER_GROUP);
        assert_eq!((state.data1, state.data2), (17, 19));
        assert_eq!(
            (state.data3, state.data4, state.ox, state.whom),
            (-1, -1, -1, -1)
        );
        assert_eq!(enter.is_entering_or_exiting(), Some(true));
        assert_eq!(
            Order::special_anim(SpecialAnimType::Exit, 0, 0).is_entering_or_exiting(),
            Some(true)
        );
        assert_eq!(
            Order::special_anim(SpecialAnimType::Unit, 0, 0).is_entering_or_exiting(),
            Some(false)
        );

        let malformed = Order {
            kind: OrderIndex::SpecialAnim,
            ..Order::default()
        };
        assert_eq!(malformed.is_entering_or_exiting(), None);
    }

    #[test]
    fn follow_payload_preserves_both_full_width_identities() {
        let payload = FollowOrderPayload {
            ox: 70_000,
            whom: 300,
            uid: 17,
            oxx: 80_000,
            whose: 301,
            uid2: 19,
        };
        let order = Order::follow(payload);
        assert_eq!(order.kind, OrderIndex::Follow);
        assert_eq!(order.flags & ORDER_GROUP, ORDER_GROUP);
        assert_eq!(order.target_uid, payload.uid);
        assert_eq!(order.follow, Some(payload));
    }

    #[test]
    fn coverage_counts_unimplemented_separately() {
        let mut c = OrderCoverage::default();
        assert_eq!(c.record(OrderIndex::MoveTo), ArmStatus::Implemented);
        assert_eq!(c.record(OrderIndex::Patrol), ArmStatus::FaithfullyEmpty);
        assert_eq!(c.record(OrderIndex::Gather), ArmStatus::Unimplemented);
        assert_eq!(c.total, 3);
        assert_eq!(c.unimplemented, 1);
        assert!((c.covered_fraction() - 2.0 / 3.0).abs() < 1e-12);
        assert_eq!(c.hot()[0].1, 1);
    }
}
