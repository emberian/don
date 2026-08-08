//! The order archetype set: what an entity is currently *doing*.
//!
//! # Where the set comes from
//!
//! The engine dispatches per-unit behaviour through an `Order` class hierarchy. Every
//! concrete order overrides `OrderIndex X::get_type() const`, and `schema/symbols.json`
//! contains exactly **27** such overrides [measured]; `UnitData` carries four separate
//! `OrderIndex` fields (`order_type`, `action_type`, `activity_type`, `next_action_type`).
//! Adding "no order" gives the **28** archetypes below.
//!
//! ```text
//! python3 -c "import json,re;d=json.load(open('schema/symbols.json'));…"   # 27 get_type overrides
//! ```
//!
//! **The names are [measured]; the numeric values here are ours.** The `OrderIndex` enum
//! itself is not in `schema/pdb-types.json` (36 enums were extracted and it is not among
//! them), so the engine's own numbering is *not* known and nothing here should be read as a
//! claim about it. Ordering is alphabetical with `Idle = 0`. When the enum is recovered this
//! table is renumbered and nothing else changes — the partitioning machinery only needs the
//! key to be a small dense integer.
//!
//! # Why this exists at all
//!
//! Not for the mechanics — they are placeholders (see [`Shape`]). It exists because *order
//! type is the branch*: the engine's per-unit step is a virtual call through this table, and
//! a batch simulator that keeps that as a branch inside the per-entity loop inherits a
//! mispredict on every entity. Made a **partition key** instead, the same work becomes 28
//! dense homogeneous kernels with no branch inside any of them. That restructuring is what
//! [`crate::partition`] and [`crate::arena`] implement, and it is the answer to
//! "order execution has to stay on the CPU because it is branchy": branchiness is a property
//! of the layout, not of the problem.

/// Number of order archetypes: 27 concrete `Order` subclasses plus `Idle`.
pub const ORDER_COUNT: usize = 28;

/// Order archetype. Names from the binary; numbering is ours (see the module docs).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Order {
    Idle = 0,
    AirAttackGround,
    AirPatrol,
    Attack,
    AttackGround,
    AttackTo,
    AwaitBoard,
    Board,
    Build,
    Cast,
    ExploreTo,
    FleeTo,
    Follow,
    Form,
    Garrison,
    Gather,
    GroupAttack,
    GroupAttackTo,
    GroupMove,
    GroupPatrol,
    Guard,
    Move,
    Patrol,
    Repair,
    SpecialAnim,
    Strafe,
    Think,
    Trade,
}

/// Order names in table order, for reporting.
pub const ORDER_NAMES: [&str; ORDER_COUNT] = [
    "Idle",
    "AirAttackGround",
    "AirPatrol",
    "Attack",
    "AttackGround",
    "AttackTo",
    "AwaitBoard",
    "Board",
    "Build",
    "Cast",
    "ExploreTo",
    "FleeTo",
    "Follow",
    "Form",
    "Garrison",
    "Gather",
    "GroupAttack",
    "GroupAttackTo",
    "GroupMove",
    "GroupPatrol",
    "Guard",
    "Move",
    "Patrol",
    "Repair",
    "SpecialAnim",
    "Strafe",
    "Think",
    "Trade",
];

/// The shape of work an order does, i.e. which kernel runs for its bucket.
///
/// **PLACEHOLDER semantics.** No order behaviour in Rise of Nations has been derived; these
/// five shapes were chosen because they cover the *structural* cases a batch scheduler has
/// to handle, which is what this module is measuring:
///
/// | shape | structural case |
/// |---|---|
/// | [`Shape::Nothing`] | pure no-op — a bucket that costs only its own existence |
/// | [`Shape::Steer`] | per-entity, no interaction — trivially parallel |
/// | [`Shape::Strike`] | **many-to-one write** into another entity's state (damage into HP) |
/// | [`Shape::Draw`] | **many-to-one draw from an exhaustible pool** (workers on a resource) |
/// | [`Shape::Churn`] | per-entity but with a data-dependent inner trip count |
///
/// The two many-to-one shapes are the point. They are the cases where a GPU-shaped
/// implementation reaches for atomics and thereby loses determinism; [`crate::reduce`] does
/// them without atomics instead.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shape {
    Nothing,
    Steer,
    Strike,
    Draw,
    Churn,
}

/// Per-order constants. Distinct per order on purpose: it keeps the branchy reference
/// honestly branchy (28 arms that do not fold into one) rather than a strawman.
#[derive(Clone, Copy, Debug)]
pub struct OrderParams {
    pub shape: Shape,
    /// Movement step in 1/192-tile units per frame.
    pub step: i32,
    /// Damage per strike, or amount requested per draw.
    pub amount: i32,
    /// Frames between actions.
    pub period: i16,
}

const fn p(shape: Shape, step: i32, amount: i32, period: i16) -> OrderParams {
    OrderParams { shape, step, amount, period }
}

/// The dispatch table. Index by `order as usize`.
pub const ORDER_PARAMS: [OrderParams; ORDER_COUNT] = [
    p(Shape::Nothing, 0, 0, 0),   // Idle
    p(Shape::Strike, 11, 17, 9),  // AirAttackGround
    p(Shape::Steer, 13, 0, 0),    // AirPatrol
    p(Shape::Strike, 5, 23, 12),  // Attack
    p(Shape::Strike, 0, 19, 15),  // AttackGround
    p(Shape::Strike, 6, 21, 11),  // AttackTo
    p(Shape::Nothing, 0, 0, 0),   // AwaitBoard
    p(Shape::Steer, 4, 0, 0),     // Board
    p(Shape::Draw, 0, 7, 5),      // Build
    p(Shape::Churn, 0, 3, 7),     // Cast
    p(Shape::Steer, 7, 0, 0),     // ExploreTo
    p(Shape::Steer, 9, 0, 0),     // FleeTo
    p(Shape::Steer, 5, 0, 0),     // Follow
    p(Shape::Steer, 3, 0, 0),     // Form
    p(Shape::Steer, 4, 0, 0),     // Garrison
    p(Shape::Draw, 2, 5, 4),      // Gather
    p(Shape::Strike, 5, 25, 10),  // GroupAttack
    p(Shape::Strike, 6, 22, 13),  // GroupAttackTo
    p(Shape::Steer, 5, 0, 0),     // GroupMove
    p(Shape::Steer, 5, 0, 0),     // GroupPatrol
    p(Shape::Strike, 0, 13, 14),  // Guard
    p(Shape::Steer, 6, 0, 0),     // Move
    p(Shape::Steer, 6, 0, 0),     // Patrol
    p(Shape::Draw, 0, 9, 6),      // Repair
    p(Shape::Churn, 0, 1, 3),     // SpecialAnim
    p(Shape::Strike, 12, 15, 8),  // Strafe
    p(Shape::Churn, 0, 2, 17),    // Think
    p(Shape::Draw, 3, 11, 9),     // Trade
];

impl Order {
    #[inline]
    pub fn from_u8(v: u8) -> Order {
        assert!((v as usize) < ORDER_COUNT, "order {v} out of range");
        // SAFETY-free: exhaustive table lookup rather than a transmute.
        ORDER_TABLE[v as usize]
    }
    #[inline]
    pub fn params(self) -> OrderParams {
        ORDER_PARAMS[self as usize]
    }
    #[inline]
    pub fn name(self) -> &'static str {
        ORDER_NAMES[self as usize]
    }
}

const ORDER_TABLE: [Order; ORDER_COUNT] = [
    Order::Idle,
    Order::AirAttackGround,
    Order::AirPatrol,
    Order::Attack,
    Order::AttackGround,
    Order::AttackTo,
    Order::AwaitBoard,
    Order::Board,
    Order::Build,
    Order::Cast,
    Order::ExploreTo,
    Order::FleeTo,
    Order::Follow,
    Order::Form,
    Order::Garrison,
    Order::Gather,
    Order::GroupAttack,
    Order::GroupAttackTo,
    Order::GroupMove,
    Order::GroupPatrol,
    Order::Guard,
    Order::Move,
    Order::Patrol,
    Order::Repair,
    Order::SpecialAnim,
    Order::Strafe,
    Order::Think,
    Order::Trade,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_consistent_and_covers_every_archetype() {
        assert_eq!(ORDER_NAMES.len(), ORDER_COUNT);
        assert_eq!(ORDER_PARAMS.len(), ORDER_COUNT);
        assert_eq!(ORDER_TABLE.len(), ORDER_COUNT);
        for i in 0..ORDER_COUNT {
            assert_eq!(ORDER_TABLE[i] as usize, i, "order table is not identity at {i}");
        }
        let mut names = ORDER_NAMES.to_vec();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), ORDER_COUNT, "duplicate order name");
    }

    /// Every structural shape must actually appear, or the partition benchmark is measuring
    /// a narrower problem than it claims to.
    #[test]
    fn every_shape_is_represented() {
        for s in [Shape::Nothing, Shape::Steer, Shape::Strike, Shape::Draw, Shape::Churn] {
            assert!(ORDER_PARAMS.iter().any(|p| p.shape == s), "no order has shape {s:?}");
        }
    }
}
