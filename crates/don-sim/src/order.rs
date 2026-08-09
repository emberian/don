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
        status: ArmStatus::Unimplemented, note: "calls through to do_move" },
    Executor { order: OrderIndex::FleeTo, va: Some("0x005F7B30"), symbol: "Unit::do_move",
        status: ArmStatus::Implemented, note: "SAME ARM as MOVE_TO [measured]" },
    Executor { order: OrderIndex::Patrol, va: None, symbol: "(default arm)",
        status: ArmStatus::FaithfullyEmpty,
        note: "OrderIndex::PATROL falls to the default arm and does nothing; live patrols are GROUP_PATROL" },
    Executor { order: OrderIndex::BuildAt, va: Some("0x005EEBF0"), symbol: "Unit::do_build",
        status: ArmStatus::Unimplemented, note: "" },
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
        status: ArmStatus::Unimplemented, note: "calls through to do_move" },
    Executor { order: OrderIndex::Guard, va: Some("0x005E5C70"), symbol: "Unit::do_guard",
        status: ArmStatus::Unimplemented, note: "calls through to do_move" },
    Executor { order: OrderIndex::Repair, va: Some("0x005EE420"), symbol: "Unit::do_repair",
        status: ArmStatus::Unimplemented, note: "" },
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
        status: ArmStatus::Unimplemented, note: "" },
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
        status: ArmStatus::Unimplemented, note: "" },
    Executor { order: OrderIndex::AirAttackGround, va: Some("0x005EA420"), symbol: "Unit::do_air_attack_ground",
        status: ArmStatus::Unimplemented, note: "aircraft" },
    Executor { order: OrderIndex::SpecialAnim, va: Some("0x005E5880"), symbol: "Unit::do_spec_anim",
        status: ArmStatus::Unimplemented, note: "" },
    Executor { order: OrderIndex::Garrison, va: Some("0x005E6B80"), symbol: "Unit::do_garrison",
        status: ArmStatus::Unimplemented, note: "" },
    Executor { order: OrderIndex::Think, va: Some("0x005E5BF0"), symbol: "Unit::do_think_order",
        status: ArmStatus::Unimplemented, note: "unit AI entry" },
];

/// One `UnitOrder`, flattened.
///
/// The retail hierarchy is 31 classes with a virtual `get_type()`, a virtually-inherited
/// `UnitOrder` subobject at the *tail* of each object, and 30 `update_<X>_order` slots.
/// None of that shape is load-bearing for us; the *fields* are, and this is their union.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Order {
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
    /// Arrival tolerance in Coord units; `UnitData::tolerance` is the per-unit default.
    pub tolerance: i32,
}

impl Default for Order {
    fn default() -> Order {
        Order {
            kind: OrderIndex::None,
            flags: 0,
            x: 0,
            y: 0,
            target_who: -1,
            target_o: -1,
            tolerance: 0,
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
