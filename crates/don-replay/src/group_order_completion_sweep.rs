//! Frozen authority for the fourteen `Group::action_*` rows whose closure class is
//! `orders_partial`.
//!
//! This module is deliberately not installed in `don-replay::lib`: it is an integration
//! boundary, not a closure promotion.  It owns the exact fixed wire images and the dependency
//! DAG that separates packet routing, order payload ownership, executor coverage, save/reload,
//! and production tick/world tails.  A caller either receives one complete decoded record or an
//! error; no prefix is published.

use don_sim::order::OrderIndex;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupOrderAction {
    Form,
    Attack,
    MoveTo,
    MoveNear,
    AttackGround,
    Patrol,
    LaunchPatrol,
    BoardShip,
    Repair,
    Trade,
    Gather,
    Garrison,
    Guard,
    Scramble,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenDependency {
    /// No Sim-owned package transaction equivalent to the canonical opcode-0 -> opcode-7 host.
    CanonicalPackageRoute,
    /// The receiver still has target/type/formation/containment branches outside its typed plan.
    GroupReceiverWorldTail,
    /// At least one possible concrete order lacks a complete production `do_job` arm.
    ConcreteExecutor,
    /// The complete concrete order image cannot round-trip through the canonical Sim save owner.
    SaveReload,
    /// The order cannot yet run from restored production state through the real frame scheduler.
    ProductionTick,
    /// A branch consumes the main simulation RNG without a mounted transactional owner.
    RandomStream,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActionDef {
    pub action: GroupOrderAction,
    pub name: &'static str,
    pub opcode: u8,
    pub command: &'static str,
    pub process_va: u32,
    pub process_size: u32,
    pub action_call_va: u32,
    pub wire_len: usize,
    pub action_va: u32,
    pub action_size: u32,
    pub direct_or_transitive_orders: &'static [OrderIndex],
    /// Exact packet body and concrete payload shapes are both frozen.
    pub payload_authority: bool,
    /// Every order kind this receiver can emit has a complete executor arm.
    pub executors_complete: bool,
    /// The current bounded canonical package host admits this command.
    pub canonical_package_route: bool,
    /// Every possible concrete payload can survive the current canonical save format.
    pub save_reload_complete: bool,
    pub open: &'static [OpenDependency],
}

const FORM_ORDERS: &[OrderIndex] = &[
    OrderIndex::ChangeForm,
    OrderIndex::MoveTo,
    OrderIndex::GroupMove,
];
const ATTACK_ORDERS: &[OrderIndex] = &[
    OrderIndex::MoveTo,
    OrderIndex::AttackTo,
    OrderIndex::ExploreTo,
    OrderIndex::FleeTo,
    OrderIndex::GroupAttackTo,
    OrderIndex::Attack,
    OrderIndex::CastSpell,
    OrderIndex::Strafe,
];
const MOVE_ORDERS: &[OrderIndex] = &[
    OrderIndex::MoveTo,
    OrderIndex::AttackTo,
    OrderIndex::ExploreTo,
    OrderIndex::FleeTo,
    OrderIndex::GroupMove,
    OrderIndex::GroupAttackTo,
    OrderIndex::Garrison,
];
const ATTACK_GROUND_ORDERS: &[OrderIndex] =
    &[OrderIndex::AttackGround, OrderIndex::AirAttackGround];
const PATROL_ORDERS: &[OrderIndex] = &[
    OrderIndex::MoveTo,
    OrderIndex::AttackTo,
    OrderIndex::ExploreTo,
    OrderIndex::FleeTo,
    OrderIndex::GroupPatrol,
    OrderIndex::AirPatrol,
    OrderIndex::Strafe,
];
const LAUNCH_PATROL_ORDERS: &[OrderIndex] = &[
    OrderIndex::MoveTo,
    OrderIndex::AirPatrol,
    OrderIndex::Strafe,
];
const BOARD_ORDERS: &[OrderIndex] = &[OrderIndex::BoardShip, OrderIndex::AwaitBoard];
const REPAIR_ORDERS: &[OrderIndex] = &[OrderIndex::Repair, OrderIndex::CastSpell];
const TRADE_ORDERS: &[OrderIndex] = &[OrderIndex::TradeRoute];
const GATHER_ORDERS: &[OrderIndex] = &[
    OrderIndex::Gather,
    OrderIndex::CastSpell,
    OrderIndex::MoveTo,
    OrderIndex::AttackTo,
    OrderIndex::ExploreTo,
    OrderIndex::FleeTo,
];
const GARRISON_ORDERS: &[OrderIndex] = &[OrderIndex::Garrison, OrderIndex::MoveTo];
const GUARD_ORDERS: &[OrderIndex] = &[OrderIndex::Guard, OrderIndex::MoveTo];
const SCRAMBLE_ORDERS: &[OrderIndex] = &[
    OrderIndex::MoveTo,
    OrderIndex::AirPatrol,
    OrderIndex::Strafe,
];

const ROUTE_WORLD_SAVE_TICK: &[OpenDependency] = &[
    OpenDependency::CanonicalPackageRoute,
    OpenDependency::GroupReceiverWorldTail,
    OpenDependency::SaveReload,
    OpenDependency::ProductionTick,
];
const ROUTE_WORLD_SAVE_TICK_RNG: &[OpenDependency] = &[
    OpenDependency::CanonicalPackageRoute,
    OpenDependency::GroupReceiverWorldTail,
    OpenDependency::SaveReload,
    OpenDependency::ProductionTick,
    OpenDependency::RandomStream,
];
const MOVE_OPEN: &[OpenDependency] = &[
    OpenDependency::GroupReceiverWorldTail,
    OpenDependency::ProductionTick,
    OpenDependency::RandomStream,
];
const EXECUTOR_OPEN: &[OpenDependency] = &[
    OpenDependency::CanonicalPackageRoute,
    OpenDependency::GroupReceiverWorldTail,
    OpenDependency::ConcreteExecutor,
    OpenDependency::SaveReload,
    OpenDependency::ProductionTick,
];
const ROUTE_WORLD_TICK: &[OpenDependency] = &[
    OpenDependency::CanonicalPackageRoute,
    OpenDependency::GroupReceiverWorldTail,
    OpenDependency::ProductionTick,
];
const ROUTE_WORLD_TICK_RNG: &[OpenDependency] = &[
    OpenDependency::CanonicalPackageRoute,
    OpenDependency::GroupReceiverWorldTail,
    OpenDependency::ProductionTick,
    OpenDependency::RandomStream,
];
const EXECUTOR_RNG_OPEN: &[OpenDependency] = &[
    OpenDependency::CanonicalPackageRoute,
    OpenDependency::GroupReceiverWorldTail,
    OpenDependency::ConcreteExecutor,
    OpenDependency::SaveReload,
    OpenDependency::ProductionTick,
    OpenDependency::RandomStream,
];

/// The exact fourteen `orders_partial` rows in `schema/simulation-closure.json` at the time
/// this sweep was frozen.  Retail sizes and VAs come from the matching PDB procedures; wire
/// lengths and process VAs come from the PE dispatcher/PDB `*Command` agreement.
pub const ACTIONS: [ActionDef; 14] = [
    ActionDef {
        action: GroupOrderAction::Form,
        name: "form",
        opcode: 3,
        command: "FormCommand",
        process_va: 0x0094_9d90,
        process_size: 307,
        action_call_va: 0x0094_9eb0,
        wire_len: 13,
        action_va: 0x0070_7220,
        action_size: 746,
        direct_or_transitive_orders: FORM_ORDERS,
        payload_authority: true,
        executors_complete: true,
        canonical_package_route: false,
        save_reload_complete: true,
        open: ROUTE_WORLD_TICK_RNG,
    },
    ActionDef {
        action: GroupOrderAction::Attack,
        name: "attack",
        opcode: 4,
        command: "AttackCommand",
        process_va: 0x0094_9c30,
        process_size: 339,
        action_call_va: 0x0094_9d70,
        wire_len: 17,
        action_va: 0x0071_2490,
        action_size: 3_833,
        direct_or_transitive_orders: ATTACK_ORDERS,
        payload_authority: true,
        executors_complete: false,
        canonical_package_route: false,
        save_reload_complete: false,
        open: EXECUTOR_RNG_OPEN,
    },
    ActionDef {
        action: GroupOrderAction::MoveTo,
        name: "move_to",
        opcode: 7,
        command: "MoveToCommand",
        process_va: 0x0094_97c0,
        process_size: 421,
        action_call_va: 0x0094_9952,
        wire_len: 22,
        action_va: 0x0070_fba0,
        action_size: 49,
        direct_or_transitive_orders: MOVE_ORDERS,
        payload_authority: true,
        executors_complete: true,
        canonical_package_route: true,
        save_reload_complete: true,
        open: MOVE_OPEN,
    },
    ActionDef {
        action: GroupOrderAction::MoveNear,
        name: "move_near",
        opcode: 8,
        command: "MoveNearCommand",
        process_va: 0x0094_95c0,
        process_size: 500,
        action_call_va: 0x0094_97a1,
        wire_len: 26,
        action_va: 0x0070_4990,
        action_size: 9_205,
        direct_or_transitive_orders: MOVE_ORDERS,
        payload_authority: true,
        executors_complete: true,
        canonical_package_route: false,
        save_reload_complete: true,
        open: &[
            OpenDependency::CanonicalPackageRoute,
            OpenDependency::GroupReceiverWorldTail,
            OpenDependency::ProductionTick,
            OpenDependency::RandomStream,
        ],
    },
    ActionDef {
        action: GroupOrderAction::AttackGround,
        name: "attack_ground",
        opcode: 9,
        command: "AttackGroundCommand",
        process_va: 0x0094_94a0,
        process_size: 274,
        action_call_va: 0x0094_959f,
        wire_len: 10,
        action_va: 0x0070_4520,
        action_size: 1_133,
        direct_or_transitive_orders: ATTACK_GROUND_ORDERS,
        payload_authority: true,
        executors_complete: true,
        canonical_package_route: false,
        save_reload_complete: false,
        open: ROUTE_WORLD_SAVE_TICK,
    },
    ActionDef {
        action: GroupOrderAction::Patrol,
        name: "patrol",
        opcode: 10,
        command: "PatrolCommand",
        process_va: 0x0094_9380,
        process_size: 274,
        action_call_va: 0x0094_947f,
        wire_len: 10,
        action_va: 0x0070_30c0,
        action_size: 1_215,
        direct_or_transitive_orders: PATROL_ORDERS,
        payload_authority: true,
        executors_complete: false,
        canonical_package_route: false,
        save_reload_complete: false,
        open: EXECUTOR_RNG_OPEN,
    },
    ActionDef {
        action: GroupOrderAction::LaunchPatrol,
        name: "launch_patrol",
        opcode: 11,
        command: "LaunchPatrolCommand",
        process_va: 0x0094_9230,
        process_size: 333,
        action_call_va: 0x0094_936a,
        wire_len: 25,
        action_va: 0x0070_3580,
        action_size: 2_043,
        direct_or_transitive_orders: LAUNCH_PATROL_ORDERS,
        payload_authority: true,
        executors_complete: false,
        canonical_package_route: false,
        save_reload_complete: false,
        open: EXECUTOR_RNG_OPEN,
    },
    ActionDef {
        action: GroupOrderAction::BoardShip,
        name: "board_ship",
        opcode: 15,
        command: "BoardShipCommand",
        process_va: 0x0094_8e00,
        process_size: 339,
        action_call_va: 0x0094_8f3e,
        wire_len: 9,
        action_va: 0x0070_0010,
        action_size: 1_149,
        direct_or_transitive_orders: BOARD_ORDERS,
        payload_authority: true,
        executors_complete: true,
        canonical_package_route: false,
        save_reload_complete: true,
        open: ROUTE_WORLD_TICK,
    },
    ActionDef {
        action: GroupOrderAction::Repair,
        name: "repair",
        opcode: 16,
        command: "RepairCommand",
        process_va: 0x0094_8cb0,
        process_size: 324,
        action_call_va: 0x0094_8de1,
        wire_len: 13,
        action_va: 0x0070_20c0,
        action_size: 999,
        direct_or_transitive_orders: REPAIR_ORDERS,
        payload_authority: true,
        executors_complete: false,
        canonical_package_route: false,
        save_reload_complete: false,
        open: EXECUTOR_OPEN,
    },
    ActionDef {
        action: GroupOrderAction::Trade,
        name: "trade",
        opcode: 17,
        command: "TradeCommand",
        process_va: 0x0094_8b20,
        process_size: 400,
        action_call_va: 0x0094_8c9d,
        wire_len: 21,
        action_va: 0x0070_1cc0,
        action_size: 1_022,
        direct_or_transitive_orders: TRADE_ORDERS,
        payload_authority: true,
        executors_complete: false,
        canonical_package_route: false,
        save_reload_complete: false,
        open: EXECUTOR_OPEN,
    },
    ActionDef {
        action: GroupOrderAction::Gather,
        name: "gather",
        opcode: 19,
        command: "GatherCommand",
        process_va: 0x0094_88b0,
        process_size: 339,
        action_call_va: 0x0094_89ee,
        wire_len: 9,
        action_va: 0x0070_0b90,
        action_size: 3_052,
        direct_or_transitive_orders: GATHER_ORDERS,
        payload_authority: true,
        executors_complete: false,
        canonical_package_route: false,
        save_reload_complete: false,
        open: EXECUTOR_RNG_OPEN,
    },
    ActionDef {
        action: GroupOrderAction::Garrison,
        name: "garrison",
        opcode: 20,
        command: "GarrisonCommand",
        process_va: 0x0094_8760,
        process_size: 326,
        action_call_va: 0x0094_8893,
        wire_len: 13,
        action_va: 0x0070_0490,
        action_size: 1_791,
        direct_or_transitive_orders: GARRISON_ORDERS,
        payload_authority: true,
        executors_complete: true,
        canonical_package_route: false,
        save_reload_complete: false,
        open: ROUTE_WORLD_SAVE_TICK_RNG,
    },
    ActionDef {
        action: GroupOrderAction::Guard,
        name: "guard",
        opcode: 31,
        command: "GuardCommand",
        process_va: 0x0094_78a0,
        process_size: 276,
        action_call_va: 0x0094_79a1,
        wire_len: 13,
        action_va: 0x006f_cd30,
        action_size: 2_012,
        direct_or_transitive_orders: GUARD_ORDERS,
        payload_authority: true,
        executors_complete: true,
        canonical_package_route: false,
        save_reload_complete: false,
        open: ROUTE_WORLD_SAVE_TICK_RNG,
    },
    ActionDef {
        action: GroupOrderAction::Scramble,
        name: "scramble",
        opcode: 36,
        command: "ScrambleCommand",
        process_va: 0x0094_7ae0,
        process_size: 229,
        action_call_va: 0x0094_7bb4,
        wire_len: 1,
        action_va: 0x0071_11c0,
        action_size: 894,
        direct_or_transitive_orders: SCRAMBLE_ORDERS,
        payload_authority: true,
        executors_complete: false,
        canonical_package_route: false,
        save_reload_complete: false,
        open: EXECUTOR_RNG_OPEN,
    },
];

/// Counts frozen by the source-bound 61-file retail corpus in
/// `schema/replay-validation.json`, in `ACTIONS` order.  A zero is evidence that the corpus
/// does not exercise that wire image, not evidence that the action is absent from retail.
pub const SOURCE_BOUND_COUNTS: [usize; 14] = [
    21, 5_715, 9_178, 0, 235, 169, 163, 0, 0, 8, 963, 127, 108, 58,
];

/// Full-body x86 instruction counts from Capstone 5 in `ACTIONS` order. Both columns consumed
/// the corresponding PDB span exactly, with zero undecoded or alignment bytes.
pub const CAPSTONE_INSTRUCTION_COUNTS: [(usize, usize); 14] = [
    (95, 228),
    (111, 1_079),
    (145, 18),
    (154, 2_475),
    (91, 318),
    (91, 367),
    (113, 581),
    (105, 317),
    (104, 282),
    (132, 299),
    (105, 847),
    (105, 483),
    (86, 561),
    (66, 250),
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecodedGroupOrder {
    Form {
        form: i32,
        rotate: i32,
        queued: i32,
    },
    Attack {
        ox: i32,
        whom: i32,
        ignore: i32,
        queued: i32,
    },
    MoveTo {
        to_x: i32,
        to_y: i32,
        set_angle: i32,
        angle: i32,
        orders: i8,
        queued: i8,
        form: i8,
        width: i8,
        disembark: i8,
    },
    MoveNear {
        to_x: i32,
        to_y: i32,
        tolerance: i32,
        set_angle: i32,
        angle: i32,
        orders: i8,
        queued: i8,
        form: i8,
        width: i8,
        disembark: i8,
    },
    AttackGround {
        to_x: i32,
        to_y: i32,
        queued: i8,
    },
    Patrol {
        to_x: i32,
        to_y: i32,
        queued: i8,
    },
    LaunchPatrol {
        to_x: i32,
        to_y: i32,
        queued: i32,
        shift: i32,
        ctrl: i32,
        alt: i32,
    },
    BoardShip {
        ox: i32,
        queued: i32,
    },
    Repair {
        ox: i32,
        whom: i32,
        queued: i32,
    },
    Trade {
        ox: i32,
        whom: i32,
        oxx: i32,
        whose: i32,
        queued: i32,
    },
    Gather {
        ox: i32,
        queued: i32,
    },
    Garrison {
        ox: i32,
        whom: i32,
        queued: i32,
    },
    Guard {
        ox: i32,
        whom: i32,
        queued: i32,
    },
    Scramble,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    UnsupportedOpcode(u8),
    WrongLength {
        opcode: u8,
        expected: usize,
        actual: usize,
    },
}

fn i32_at(bytes: &[u8], at: usize) -> i32 {
    i32::from_le_bytes(
        bytes[at..at + 4]
            .try_into()
            .expect("validated fixed command"),
    )
}

/// Strictly decode one complete fixed-size command.  Unlike the old bridge helpers this refuses
/// a truncated body and trailing bytes rather than defaulting absent fields to zero.
pub fn decode_group_order(bytes: &[u8]) -> Result<DecodedGroupOrder, DecodeError> {
    let opcode = bytes.first().copied().unwrap_or(u8::MAX);
    let def = ACTIONS
        .iter()
        .find(|row| row.opcode == opcode)
        .ok_or(DecodeError::UnsupportedOpcode(opcode))?;
    if bytes.len() != def.wire_len {
        return Err(DecodeError::WrongLength {
            opcode,
            expected: def.wire_len,
            actual: bytes.len(),
        });
    }
    Ok(match opcode {
        3 => DecodedGroupOrder::Form {
            form: i32_at(bytes, 1),
            rotate: i32_at(bytes, 5),
            queued: i32_at(bytes, 9),
        },
        4 => DecodedGroupOrder::Attack {
            ox: i32_at(bytes, 1),
            whom: i32_at(bytes, 5),
            ignore: i32_at(bytes, 9),
            queued: i32_at(bytes, 13),
        },
        7 => DecodedGroupOrder::MoveTo {
            to_x: i32_at(bytes, 1),
            to_y: i32_at(bytes, 5),
            set_angle: i32_at(bytes, 9),
            angle: i32_at(bytes, 13),
            orders: bytes[17] as i8,
            queued: bytes[18] as i8,
            form: bytes[19] as i8,
            width: bytes[20] as i8,
            disembark: bytes[21] as i8,
        },
        8 => DecodedGroupOrder::MoveNear {
            to_x: i32_at(bytes, 1),
            to_y: i32_at(bytes, 5),
            tolerance: i32_at(bytes, 9),
            set_angle: i32_at(bytes, 13),
            angle: i32_at(bytes, 17),
            orders: bytes[21] as i8,
            queued: bytes[22] as i8,
            form: bytes[23] as i8,
            width: bytes[24] as i8,
            disembark: bytes[25] as i8,
        },
        9 => DecodedGroupOrder::AttackGround {
            to_x: i32_at(bytes, 1),
            to_y: i32_at(bytes, 5),
            queued: bytes[9] as i8,
        },
        10 => DecodedGroupOrder::Patrol {
            to_x: i32_at(bytes, 1),
            to_y: i32_at(bytes, 5),
            queued: bytes[9] as i8,
        },
        11 => DecodedGroupOrder::LaunchPatrol {
            to_x: i32_at(bytes, 1),
            to_y: i32_at(bytes, 5),
            queued: i32_at(bytes, 9),
            shift: i32_at(bytes, 13),
            ctrl: i32_at(bytes, 17),
            alt: i32_at(bytes, 21),
        },
        15 => DecodedGroupOrder::BoardShip {
            ox: i32_at(bytes, 1),
            queued: i32_at(bytes, 5),
        },
        16 => DecodedGroupOrder::Repair {
            ox: i32_at(bytes, 1),
            whom: i32_at(bytes, 5),
            queued: i32_at(bytes, 9),
        },
        17 => DecodedGroupOrder::Trade {
            ox: i32_at(bytes, 1),
            whom: i32_at(bytes, 5),
            oxx: i32_at(bytes, 9),
            whose: i32_at(bytes, 13),
            queued: i32_at(bytes, 17),
        },
        19 => DecodedGroupOrder::Gather {
            ox: i32_at(bytes, 1),
            queued: i32_at(bytes, 5),
        },
        20 => DecodedGroupOrder::Garrison {
            ox: i32_at(bytes, 1),
            whom: i32_at(bytes, 5),
            queued: i32_at(bytes, 9),
        },
        31 => DecodedGroupOrder::Guard {
            ox: i32_at(bytes, 1),
            whom: i32_at(bytes, 5),
            queued: i32_at(bytes, 9),
        },
        36 => DecodedGroupOrder::Scramble,
        _ => unreachable!("opcode was selected from ACTIONS"),
    })
}

pub fn action_def(opcode: u8) -> Option<&'static ActionDef> {
    ACTIONS.iter().find(|row| row.opcode == opcode)
}
