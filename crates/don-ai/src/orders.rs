//! The order interface — the single seam every actor drives the game through.
//!
//! Both the transcribed BHS script and (later) an RL policy emit [`Order`]s;
//! nothing mutates [`crate::game::Game`] except [`crate::game::Game::submit`].
//! That is the whole point of this module: the scripted opponent and the learned
//! one must be *interchangeable at the same interface*, or behaviour cloning
//! trains on a channel the agent will not have at inference time.
//!
//! # Relationship to the engine's own command layer
//!
//! Retail routes player intent through `CommandPackage` — 82 opcodes with exact
//! wire layouts in `schema/command-wire.json`. This enum is **not** that: it is
//! the *script-visible* command set, i.e. exactly the mutating host functions
//! `economic.bhs` and `aibestbuildlibrary.bhs` call, each named for the host
//! function it comes from and carrying its implementation address. When the
//! netcode lane's command encoder is ready, this is the natural place to bridge:
//! one `Order` should lower to one `CommandPackage` opcode.
//!
//! # Return convention
//!
//! [`OrderResult`] mirrors the engine's: `1`/positive for success, `0` for
//! "refused but affordable", `-1` for invalid. The scripts branch on all three
//! (`place_dock` documents the tri-state in its own comment), so collapsing them
//! to a bool would change control flow.

/// One command the production layer can issue.
///
/// Every variant is a host function in `crates/don-ai/data/script-functions.json`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Order {
    /// `place_building_with_cost(who, build_type, city_name)` @ `0x009F54A0`.
    PlaceBuilding { who: i32, build_type: String, city_name: String },
    /// `place_orphan_building_with_cost(who, build_type, build_o)` @ `0x009F5520`.
    PlaceOrphanBuilding { who: i32, build_type: String, near: i32 },
    /// `place_building_upgrade_with_cost(who, build_type, city_name)` @ `0x009F5680`.
    PlaceBuildingUpgrade { who: i32, build_type: String, city_name: String },
    /// `place_city_with_cost(who)` @ `0x009F5860`.
    PlaceCity { who: i32 },
    /// `train_unit_with_cost(who, num, unit_type)` @ `0x009F42D0`.
    TrainUnit { who: i32, num: i32, unit_type: String },
    /// `train_unit_at_with_cost(who, num, unit_type, build_o)` @ `0x009F4460`.
    TrainUnitAt { who: i32, num: i32, unit_type: String, build_o: i32 },
    /// `research_tech_with_cost(who, tech)` @ `0x009EE700`.
    ResearchTech { who: i32, tech: String },
    /// `destroy_building(who, build_o)` @ `0x009F6FC0`.
    DestroyBuilding { who: i32, build_o: i32 },
    /// `unit_move_order(who, unit_o, x, y)`. In this model a citizen "moved to"
    /// a gatherer building is assigned to one of its worker slots, which is what
    /// the order accomplishes in retail once the unit arrives.
    MoveUnit { who: i32, unit_o: i32, x: i32, y: i32 },
    /// `citizen_repair_order(who, unit_o, build_o_target)` @ `0x009F85B0`.
    CitizenRepair { who: i32, unit_o: i32, build_o_target: i32 },
}

impl Order {
    /// The `who` this order belongs to. Used to reject cross-player orders.
    pub fn who(&self) -> i32 {
        match self {
            Order::PlaceBuilding { who, .. }
            | Order::PlaceOrphanBuilding { who, .. }
            | Order::PlaceBuildingUpgrade { who, .. }
            | Order::PlaceCity { who }
            | Order::TrainUnit { who, .. }
            | Order::TrainUnitAt { who, .. }
            | Order::ResearchTech { who, .. }
            | Order::DestroyBuilding { who, .. }
            | Order::MoveUnit { who, .. }
            | Order::CitizenRepair { who, .. } => *who,
        }
    }

    /// A stable short name, for logs and for a future action-space encoding.
    pub fn kind(&self) -> &'static str {
        match self {
            Order::PlaceBuilding { .. } => "place_building",
            Order::PlaceOrphanBuilding { .. } => "place_orphan_building",
            Order::PlaceBuildingUpgrade { .. } => "place_building_upgrade",
            Order::PlaceCity { .. } => "place_city",
            Order::TrainUnit { .. } => "train_unit",
            Order::TrainUnitAt { .. } => "train_unit_at",
            Order::ResearchTech { .. } => "research_tech",
            Order::DestroyBuilding { .. } => "destroy_building",
            Order::MoveUnit { .. } => "move_unit",
            Order::CitizenRepair { .. } => "citizen_repair",
        }
    }
}

/// What the game did with an order.
///
/// The numeric values are the engine's, because the scripts compare against
/// them directly (`if (place_building_with_cost(...) > 0)`, and elsewhere the
/// bare-truthiness form that makes `-1` behave as *true* under C rules).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrderResult {
    /// The object id the order produced, or `1` when there is no id. `> 0`.
    Ok(i32),
    /// Refused, but the player could pay for it — the engine's `0`.
    Refused,
    /// Invalid player, unknown type, unmet prerequisite — the engine's `-1`.
    Invalid,
}

impl OrderResult {
    pub fn as_i32(self) -> i32 {
        match self {
            OrderResult::Ok(v) => v.max(1),
            OrderResult::Refused => 0,
            OrderResult::Invalid => -1,
        }
    }
}
