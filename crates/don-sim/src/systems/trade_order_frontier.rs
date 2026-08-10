// SPDX-License-Identifier: GPL-3.0-or-later
//! Source-only reconstruction frontier for `Unit::do_trade`.
//!
//! This module deliberately stays out of `systems/mod.rs`.  It freezes the concrete
//! `TradeOrder`, destination scan, route-establishment ordering, arrival transitions, and
//! road/move host boundary without pretending that the caravan road A*, object registries,
//! and production tick adapter already exist.

pub const TRADE_ORDER_INDEX: i32 = 15;
pub const UNIT_ADD_TRADE_ORDER_VA: u32 = 0x005e_4dc0;
pub const UNIT_DO_TRADE_VA: u32 = 0x005e_d270;
pub const UNIT_DO_TRADE_BYTES: usize = 4_519;
pub const UNIT_DO_TRADE_END_VA: u32 = 0x005e_e417;
pub const CARAVAN_BUILD_ROAD_VA: u32 = 0x0073_db10;
pub const CARAVAN_ROAD_ASTAR_VA: u32 = 0x0068_5990;
pub const TRADE_ORDER_SIZE: usize = 52;
pub const TRADE_ORDER_WALKED_BYTES: usize = 29;
/// OrderList framing after its four-byte list length: type (4), node metric (1), payload (29).
pub const TRADE_LIST_NODE_WALKED_BYTES: usize = 34;
pub const CARAVAN_TRADE_PREREQ: i32 = 0x2ac;
pub const UNIT_HAS_TRADE_ROUTE: u32 = 0x200;
pub const UNIT_TRANSPORT_RECOVERY: u32 = 0x0004_0000;
pub const QUEUE_FIRST: i32 = 0;
pub const QUEUE_LAST: i32 = 1;
pub const QUEUE_NEW: i32 = 2;
pub const TRADE_ANIMATION: i32 = 0;
pub const TRADE_NO_DESTINATION_IDLE: u8 = 99;
pub const TRADE_CITY_RADIUS_BASE: i32 = 0x306;
pub const TRADE_NEARBY_RADIUS_PAD: i32 = 0xc0;
pub const TRADE_NEARBY_ACCEPT_PAD: i32 = 0xc6;
pub const TRADE_PATH_SMOOTH_LIMIT: i32 = 0xc1;

pub mod offsets {
    pub const OX: usize = 0x08;
    pub const WHOM: usize = 0x0c;
    pub const UID: usize = 0x10;
    pub const OXX: usize = 0x14;
    pub const WHOSE: usize = 0x18;
    pub const STARTED: usize = 0x1c;
    pub const LOADED: usize = 0x20;
    pub const UID2: usize = 0x24;
    pub const VTORDISP: usize = 0x28;
    pub const UNIT_ORDER_VBASE: usize = 0x2c;
    pub const FLAGS: usize = 0x30;
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TradeIdentity {
    pub o: i32,
    pub who: i32,
    pub uid: u16,
}

impl TradeIdentity {
    pub const NONE: Self = Self {
        o: -1,
        who: -1,
        uid: u16::MAX,
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TradeOrderState {
    pub first: TradeIdentity,
    pub second: TradeIdentity,
    pub started: i32,
    pub loaded: i32,
    pub flags: u8,
}

impl Default for TradeOrderState {
    fn default() -> Self {
        Self {
            first: TradeIdentity::NONE,
            second: TradeIdentity::NONE,
            started: 0,
            loaded: 0,
            flags: 0,
        }
    }
}

/// The three exact half-open regions submitted by `TradeOrder::walk_data`.
pub const TRADE_WALK_RANGES: [(usize, usize); 3] = [
    (offsets::FLAGS, offsets::FLAGS + 1),
    (offsets::OX, offsets::UID + 2),
    (offsets::OXX, offsets::UID2 + 2),
];

/// `Group::set_up_insert` reaches `copy_order`'s default arm for kind 15. Although
/// `finish_insert` has a TRADE_ROUTE reissue arm, no old trade order can reach it because
/// the clone is null. A generic queue-first splice that preserves TRADE_ROUTE is divergent.
pub const GROUP_QUEUE_FIRST_COPIES_TRADE_ORDER: bool = false;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TradeInstallRequest {
    pub first: TradeIdentity,
    pub second: TradeIdentity,
    pub queue: i32,
    pub group: bool,
    /// Exact result of the installer's cross-terrain transport capability cone.
    pub mark_transport_route: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TradeInstallStep {
    /// `flags &= ~0x04000000; +0xC0=0; close_orders(0); clear_partial_path;
    /// update_action`.
    ClearNewOrderState,
    ClearUnitTradeRouteFlag,
    AllocateOrder(i32),
    StoreFirst(TradeIdentity),
    StoreLoaded(i32),
    StoreSecond(TradeIdentity),
    StoreStarted(i32),
    SetGroupFlag(bool),
    SetTransportRouteFlag,
    Append,
    UpdateAction,
}

pub fn plan_trade_install(request: TradeInstallRequest) -> Vec<TradeInstallStep> {
    let mut steps = Vec::new();
    if request.queue == QUEUE_NEW {
        steps.push(TradeInstallStep::ClearNewOrderState);
    }
    steps.push(TradeInstallStep::ClearUnitTradeRouteFlag);
    steps.push(TradeInstallStep::AllocateOrder(TRADE_ORDER_INDEX));
    let mut first = request.first;
    first.uid = installed_primary_uid(first.o, first.who, first.uid);
    steps.push(TradeInstallStep::StoreFirst(first));
    steps.push(TradeInstallStep::StoreLoaded(0));
    let mut second = request.second;
    second.uid = installed_second_uid(second.o, first.who, second.who, second.uid);
    steps.push(TradeInstallStep::StoreSecond(second));
    steps.push(TradeInstallStep::StoreStarted(0));
    steps.push(TradeInstallStep::SetGroupFlag(request.group));
    if request.mark_transport_route {
        steps.push(TradeInstallStep::SetTransportRouteFlag);
    }
    steps.push(TradeInstallStep::Append);
    steps.push(TradeInstallStep::UpdateAction);
    steps
}

pub fn installed_primary_uid(ox: i32, whom: i32, resolved_uid: u16) -> u16 {
    if ox >= 0 && whom >= 0 {
        resolved_uid
    } else {
        u16::MAX
    }
}

/// Shipped `add_trade_order` asymmetry at `0x005E4E67..0x005E4E8F`.
///
/// The second UID is loaded only when `oxx >= 0 && primary_whom >= 0`; the second owner
/// (`whose`) is used for the lookup but is not itself tested. This can crash on malformed
/// direct callers and must not be silently replaced by the intuitive `whose >= 0` gate.
pub fn installed_second_uid(oxx: i32, primary_whom: i32, _whose: i32, resolved_uid: u16) -> u16 {
    if oxx >= 0 && primary_whom >= 0 {
        resolved_uid
    } else {
        u16::MAX
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TradeActorFacts {
    pub identity: TradeIdentity,
    pub x: i32,
    pub y: i32,
    pub flags: u32,
    pub caravan_slot: i16,
    /// `Unit + 0x68 & 1`; route earning/contact publication is skipped while inside.
    pub inside: bool,
    /// Result of `UnitData::get_order()` after a common retirement.
    pub has_order_after_kill: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TradeCaravanFacts {
    pub owner: i32,
    pub slot: i32,
    pub flags: u8,
    /// `CaravanData::making_road +0x20`; nonzero holds before endpoint work.
    pub making_road: i32,
    pub route_digest: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TradeEndpointFacts {
    pub identity: TradeIdentity,
    pub live: bool,
    pub is_build: bool,
    pub city: i32,
    /// Current city-centre object. Before establishment retail repairs only `first.o` to
    /// this value; it deliberately does not refresh the retained UID.
    pub canonical_o: i32,
    pub city_live: bool,
    /// `ObjectData::is_active_wallbuild` followed by the build virtual at `+0x24`.
    /// Retail requires both before the `loaded == 0` endpoint transition.
    pub unloaded_departure_gate: bool,
    pub x: i32,
    pub y: i32,
    /// `CityData::x/y`, distinct from the live centre object's coordinates.
    pub city_x: i32,
    pub city_y: i32,
    pub x_size: i32,
    pub y_size: i32,
    pub empty_route: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TradeCityEndpoint {
    pub city: i32,
    pub owner: i32,
}

impl TradeEndpointFacts {
    pub fn radius(self) -> i32 {
        self.x_size
            .max(self.y_size)
            .wrapping_mul(0x60)
            .wrapping_add(TRADE_CITY_RADIUS_BASE)
    }
}

/// One city visited by the owner-major, city-minor destination scan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TradeCandidate {
    pub owner: i32,
    pub city: i32,
    pub endpoint: TradeIdentity,
    /// Registry preflight only; retail has no separate candidate-leader liveness branch.
    pub leader_live: bool,
    pub allied: bool,
    pub actor_has_foreign_prereq: bool,
    pub city_live: bool,
    pub seen: bool,
    /// Exact `1 - established_match_count`; retail admits every nonzero result.
    pub candidate_empty_route_result: i32,
    pub source_empty_route_result: i32,
    pub transport_compatible: bool,
    /// Exact `Caravan::trade_value` result, already multiplied by four on retail's
    /// actor-owner/source-owner scoring arm.
    pub score: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TradeSelectionContext {
    pub actor_owner: i32,
    pub source_owner: i32,
    pub source_city: i32,
}

fn candidate_admitted(context: TradeSelectionContext, candidate: TradeCandidate) -> bool {
    if !candidate.allied || !candidate.city_live {
        return false;
    }
    if candidate.owner != context.actor_owner
        && (!candidate.actor_has_foreign_prereq || context.actor_owner != context.source_owner)
    {
        return false;
    }
    if candidate.owner == context.source_owner && candidate.city == context.source_city {
        return false;
    }
    candidate.seen
        && candidate.candidate_empty_route_result != 0
        && candidate.source_empty_route_result != 0
        && candidate.transport_compatible
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TradeSelection {
    pub scan_index: usize,
    pub candidate: TradeCandidate,
}

/// Exact strict-greater fold after the binary's owner-major/city-minor admission scan.
/// A tie retains the earlier city and all negative scores are rejected (`best=-1`).
pub fn select_trade_destination(
    context: TradeSelectionContext,
    candidates: &[TradeCandidate],
) -> Option<TradeSelection> {
    let mut best_score = -1;
    let mut best = None;
    for (scan_index, &candidate) in candidates.iter().enumerate() {
        if candidate_admitted(context, candidate) && best_score < candidate.score {
            best_score = candidate.score;
            best = Some(TradeSelection {
                scan_index,
                candidate,
            });
        }
    }
    best
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoadBuildStatus {
    /// `Caravan::build_road` returned -1. The parked A* state may have advanced, but the
    /// unit is sent directly toward the first endpoint for this frame.
    SearchPending,
    /// Return 0: no road stack was produced.
    NoRoad,
    /// Return 1: the road stack and terrain road writes were published.
    Built,
}

/// Atomic evidence for the nested road search. `Unit::do_trade` does not draw directly;
/// the half-open draw interval belongs to `calc_road_cost` inside the caravan A*.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoadBuildReceipt {
    pub status: RoadBuildStatus,
    pub search_before: u64,
    pub search_after: u64,
    pub road_before: u64,
    pub road_after: u64,
    pub terrain_epoch_before: u64,
    pub terrain_epoch_after: u64,
    pub rng_epoch_before: u64,
    pub rng_epoch_after: u64,
    pub draw_count: u32,
}

impl RoadBuildReceipt {
    pub fn valid(self) -> bool {
        self.rng_epoch_after.wrapping_sub(self.rng_epoch_before) == u64::from(self.draw_count)
            && match self.status {
                RoadBuildStatus::SearchPending => self.road_before == self.road_after,
                RoadBuildStatus::NoRoad => true,
                RoadBuildStatus::Built => self.road_before != self.road_after,
            }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RouteAdmissionFacts {
    pub actor_has_foreign_prereq: bool,
    pub source_empty_route_result: i32,
    pub destination_empty_route_result: i32,
    pub actor_distance_to_source: i32,
    pub actor_distance_to_destination: i32,
    pub road: RoadBuildReceipt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoadMovementFacts {
    /// The caravan owns a road stack. The receipt covers orientation, assignment to the
    /// unit path, close-vertex `/3` smoothing, pop/peek, optional flag-4 join point,
    /// inversion, and the inserted MOVE order.
    Stack {
        road_before: u64,
        road_after: u64,
        unit_path_before: u64,
        unit_path_after: u64,
        add_move_x: i32,
        add_move_y: i32,
        smoothed_vertices: u32,
        pushed_join: bool,
    },
    /// Empty road stack: retail asks `find_nearby_spot` around the destination building.
    Nearby {
        /// Retail ignores this call's return value and checks only the written point.
        return_nonzero: bool,
        x: i32,
        y: i32,
        actor_distance: i32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TradeRecoveryFacts {
    pub local_feedback: bool,
    pub transport_next_city: Option<TradeIdentity>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TradeFrameFacts {
    pub actor: TradeActorFacts,
    pub order: TradeOrderState,
    pub caravan: Option<TradeCaravanFacts>,
    pub first: Option<TradeEndpointFacts>,
    pub second: Option<TradeEndpointFacts>,
    pub candidates: Option<Vec<TradeCandidate>>,
    /// Actor-to-first-object terrain/transport gate reached before auto-selection.
    pub source_transport_compatible: Option<bool>,
    pub transport_compatible: Option<bool>,
    pub route_admission: Option<RouteAdmissionFacts>,
    pub movement: Option<RoadMovementFacts>,
    pub recovery: Option<TradeRecoveryFacts>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TradePlanError {
    MissingCaravan,
    CaravanOwnerMismatch,
    MissingFirstEndpoint,
    FirstIdentityMismatch,
    MissingCandidateScan,
    MissingSecondEndpoint,
    SecondIdentityMismatch,
    MissingTransportFact,
    MissingRouteAdmission,
    InvalidRoadReceipt,
    MissingMovementFacts,
    MissingRecoveryFacts,
    UnexpectedRouteAdmission,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TradeHostStep {
    SetAnimation {
        animation: i32,
        arg0: i32,
        arg1: i32,
    },
    RepairFirstObjectOnly {
        o: i32,
    },
    StoreSelectedSecond(TradeIdentity),
    SetIdle(u8),
    LocalFeedback {
        sound: i32,
    },
    KillCurrentOrder {
        arg: i32,
    },
    ThinkCaravan {
        forced: i32,
    },
    GoToCity {
        city: i32,
        owner: i32,
        queue: i32,
    },
    AppendCityLink {
        city: i32,
        owner: i32,
    },
    StoreCaravanEndpoints {
        first: TradeCityEndpoint,
        second: TradeCityEndpoint,
    },
    SetCaravanFlags(u8),
    SetOrderStarted(i32),
    ComputeTrade {
        city: i32,
        owner: i32,
    },
    BuildRoad(RoadBuildReceipt),
    SetOrderLoaded(i32),
    SetUnitTradeRouteFlag,
    NewCaravanContact {
        receiver_city: i32,
        receiver_owner: i32,
        partner_city: i32,
        partner_owner: i32,
    },
    AssignAndAdvanceRoad {
        road_before: u64,
        road_after: u64,
        unit_path_before: u64,
        unit_path_after: u64,
        smoothed_vertices: u32,
        pushed_join: bool,
    },
    FindNearbySpot {
        centre_x: i32,
        centre_y: i32,
        min_radius: i32,
        max_radius: i32,
        return_nonzero: bool,
        x: i32,
        y: i32,
    },
    AddMoveOrder {
        x: i32,
        y: i32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TradeExecutorBranch {
    HoldResetRoad,
    InvalidRetire,
    NoDestination,
    EstablishmentRejected,
    RoadSearchPending,
    Moving,
    FirstEndpointTransition,
    SecondEndpointTransition,
    HoldOutsideNearbyRadius,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TradeExecutorPlan {
    pub order_after: TradeOrderState,
    pub branch: TradeExecutorBranch,
    pub steps: Vec<TradeHostStep>,
}

fn common_retire(
    request: &TradeFrameFacts,
    order: TradeOrderState,
    feedback: bool,
) -> TradeExecutorPlan {
    let mut steps = Vec::new();
    if feedback {
        steps.push(TradeHostStep::LocalFeedback { sound: 0x40 });
    }
    steps.push(TradeHostStep::KillCurrentOrder { arg: 0 });
    if !request.actor.has_order_after_kill {
        steps.push(TradeHostStep::ThinkCaravan { forced: 1 });
    }
    TradeExecutorPlan {
        order_after: order,
        branch: TradeExecutorBranch::InvalidRetire,
        steps,
    }
}

fn movement_steps(
    footprint: TradeEndpointFacts,
    target: TradeEndpointFacts,
    movement: RoadMovementFacts,
    steps: &mut Vec<TradeHostStep>,
) -> TradeExecutorBranch {
    match movement {
        RoadMovementFacts::Stack {
            road_before,
            road_after,
            unit_path_before,
            unit_path_after,
            add_move_x,
            add_move_y,
            smoothed_vertices,
            pushed_join,
        } => {
            steps.push(TradeHostStep::AssignAndAdvanceRoad {
                road_before,
                road_after,
                unit_path_before,
                unit_path_after,
                smoothed_vertices,
                pushed_join,
            });
            steps.push(TradeHostStep::AddMoveOrder {
                x: add_move_x,
                y: add_move_y,
            });
            TradeExecutorBranch::Moving
        }
        RoadMovementFacts::Nearby {
            return_nonzero,
            x,
            y,
            actor_distance,
        } => {
            let base = footprint.x_size.max(footprint.y_size).wrapping_mul(0x60);
            steps.push(TradeHostStep::FindNearbySpot {
                centre_x: target.x,
                centre_y: target.y,
                min_radius: base,
                max_radius: base.wrapping_add(TRADE_NEARBY_RADIUS_PAD),
                return_nonzero,
                x,
                y,
            });
            if actor_distance <= base.wrapping_add(TRADE_NEARBY_ACCEPT_PAD) {
                steps.push(TradeHostStep::AddMoveOrder { x, y });
                TradeExecutorBranch::Moving
            } else {
                TradeExecutorBranch::HoldOutsideNearbyRadius
            }
        }
    }
}

/// Plan one complete `Unit::do_trade` activation around explicit registry, A*, and path
/// observations. The plan is mutation ordered; a future host must preflight and publish it
/// atomically rather than applying the locally convenient subset.
pub fn plan_trade_executor(request: &TradeFrameFacts) -> Result<TradeExecutorPlan, TradePlanError> {
    let mut order = request.order;
    let mut caravan_flags = request.caravan.map_or(0, |caravan| caravan.flags);
    let mut steps = vec![TradeHostStep::SetAnimation {
        animation: TRADE_ANIMATION,
        arg0: 0,
        arg1: 1,
    }];
    let caravan = request.caravan.ok_or(TradePlanError::MissingCaravan)?;
    if caravan.owner != request.actor.identity.who
        || caravan.slot != i32::from(request.actor.caravan_slot)
    {
        return Err(TradePlanError::CaravanOwnerMismatch);
    }
    if caravan.making_road != 0 {
        return Ok(TradeExecutorPlan {
            order_after: order,
            branch: TradeExecutorBranch::HoldResetRoad,
            steps,
        });
    }

    let first = request.first.ok_or(TradePlanError::MissingFirstEndpoint)?;
    if first.identity.o != order.first.o || first.identity.who != order.first.who {
        return Err(TradePlanError::FirstIdentityMismatch);
    }
    if first.city < 0 {
        let mut retired = common_retire(request, order, false);
        steps.append(&mut retired.steps);
        retired.steps = steps;
        return Ok(retired);
    }
    if order.started == 0 && first.canonical_o != order.first.o {
        order.first.o = first.canonical_o;
        steps.push(TradeHostStep::RepairFirstObjectOnly {
            o: first.canonical_o,
        });
    }
    if !request
        .source_transport_compatible
        .ok_or(TradePlanError::MissingTransportFact)?
    {
        let mut retired = common_retire(request, order, false);
        steps.append(&mut retired.steps);
        retired.steps = steps;
        return Ok(retired);
    }

    if order.second.o < 0 {
        let candidates = request
            .candidates
            .as_deref()
            .ok_or(TradePlanError::MissingCandidateScan)?;
        let context = TradeSelectionContext {
            actor_owner: request.actor.identity.who,
            source_owner: order.first.who,
            source_city: first.city,
        };
        let Some(selected) = select_trade_destination(context, candidates) else {
            let recovery = request
                .recovery
                .ok_or(TradePlanError::MissingRecoveryFacts)?;
            if recovery.local_feedback {
                steps.push(TradeHostStep::LocalFeedback { sound: 0x40 });
            }
            steps.push(TradeHostStep::SetIdle(TRADE_NO_DESTINATION_IDLE));
            steps.push(TradeHostStep::KillCurrentOrder { arg: 0 });
            if request.actor.flags & UNIT_TRANSPORT_RECOVERY != 0 {
                if let Some(city) = recovery.transport_next_city {
                    steps.push(TradeHostStep::GoToCity {
                        city: city.o,
                        owner: city.who,
                        queue: QUEUE_LAST,
                    });
                }
            }
            return Ok(TradeExecutorPlan {
                order_after: order,
                branch: TradeExecutorBranch::NoDestination,
                steps,
            });
        };
        order.second = selected.candidate.endpoint;
        steps.push(TradeHostStep::StoreSelectedSecond(order.second));
    }

    let second = request
        .second
        .ok_or(TradePlanError::MissingSecondEndpoint)?;
    if second.identity.o != order.second.o || second.identity.who != order.second.who {
        return Err(TradePlanError::SecondIdentityMismatch);
    }
    // Retail stores an auto-selected second endpoint before these active-city checks.
    if !first.live
        || !first.is_build
        || !first.city_live
        || !second.live
        || !second.is_build
        || second.city < 0
        || !second.city_live
    {
        let mut retired = common_retire(request, order, false);
        steps.append(&mut retired.steps);
        retired.steps = steps;
        return Ok(retired);
    }
    if !request
        .transport_compatible
        .ok_or(TradePlanError::MissingTransportFact)?
    {
        let mut retired = common_retire(request, order, false);
        steps.append(&mut retired.steps);
        retired.steps = steps;
        return Ok(retired);
    }

    let mut effective_first = first;
    let mut effective_second = second;

    if order.started == 0 {
        let admission = request
            .route_admission
            .ok_or(TradePlanError::MissingRouteAdmission)?;
        if !admission.road.valid() {
            return Err(TradePlanError::InvalidRoadReceipt);
        }
        let domestic = order.first.who == request.actor.identity.who
            && order.second.who == request.actor.identity.who;
        if (!domestic && !admission.actor_has_foreign_prereq)
            || admission.source_empty_route_result == 0
            || admission.destination_empty_route_result == 0
        {
            let recovery = request
                .recovery
                .ok_or(TradePlanError::MissingRecoveryFacts)?;
            let mut retired = common_retire(request, order, recovery.local_feedback);
            retired.branch = TradeExecutorBranch::EstablishmentRejected;
            steps.append(&mut retired.steps);
            retired.steps = steps;
            return Ok(retired);
        }
        let (near, far) =
            if admission.actor_distance_to_destination < admission.actor_distance_to_source {
                (second, first)
            } else {
                (first, second)
            };
        // The swap is local to this activation; TradeOrder first/second are unchanged.
        effective_first = near;
        effective_second = far;
        steps.push(TradeHostStep::AppendCityLink {
            city: near.city,
            owner: near.identity.who,
        });
        steps.push(TradeHostStep::AppendCityLink {
            city: far.city,
            owner: far.identity.who,
        });
        steps.push(TradeHostStep::StoreCaravanEndpoints {
            first: TradeCityEndpoint {
                city: near.city,
                owner: near.identity.who,
            },
            second: TradeCityEndpoint {
                city: far.city,
                owner: far.identity.who,
            },
        });
        caravan_flags |= 3;
        steps.push(TradeHostStep::SetCaravanFlags(caravan_flags));
        order.started = 1;
        steps.push(TradeHostStep::SetOrderStarted(1));
        steps.push(TradeHostStep::ComputeTrade {
            city: near.city,
            owner: near.identity.who,
        });
        steps.push(TradeHostStep::ComputeTrade {
            city: far.city,
            owner: far.identity.who,
        });
        steps.push(TradeHostStep::BuildRoad(admission.road));
        if admission.road.status == RoadBuildStatus::SearchPending {
            steps.push(TradeHostStep::AddMoveOrder {
                x: near.city_x,
                y: near.city_y,
            });
            return Ok(TradeExecutorPlan {
                order_after: order,
                branch: TradeExecutorBranch::RoadSearchPending,
                steps,
            });
        }
    } else if request.route_admission.is_some() {
        return Err(TradePlanError::UnexpectedRouteAdmission);
    }

    let arrival_endpoint = if order.loaded != 0 {
        effective_first
    } else {
        effective_second
    };
    if order.loaded == 0 && !effective_second.unloaded_departure_gate {
        let mut retired = common_retire(request, order, false);
        steps.append(&mut retired.steps);
        retired.steps = steps;
        return Ok(retired);
    }
    let reached = (request.actor.x.wrapping_sub(arrival_endpoint.x)).wrapping_abs()
        <= arrival_endpoint.radius()
        && (request.actor.y.wrapping_sub(arrival_endpoint.y)).wrapping_abs()
            <= arrival_endpoint.radius();
    let mut arrival_branch = None;
    if reached {
        if order.loaded != 0 {
            order.loaded = 0;
            steps.push(TradeHostStep::SetOrderLoaded(0));
            steps.push(TradeHostStep::SetUnitTradeRouteFlag);
            if !request.actor.inside {
                caravan_flags |= 4;
                steps.push(TradeHostStep::SetCaravanFlags(caravan_flags));
                steps.push(TradeHostStep::ComputeTrade {
                    city: effective_first.city,
                    owner: effective_first.identity.who,
                });
                steps.push(TradeHostStep::ComputeTrade {
                    city: effective_second.city,
                    owner: effective_second.identity.who,
                });
                steps.push(TradeHostStep::NewCaravanContact {
                    receiver_city: effective_first.city,
                    receiver_owner: effective_first.identity.who,
                    partner_city: effective_second.city,
                    partner_owner: effective_second.identity.who,
                });
            }
            arrival_branch = Some(TradeExecutorBranch::FirstEndpointTransition);
        } else {
            order.loaded = 1;
            steps.push(TradeHostStep::SetOrderLoaded(1));
            if !request.actor.inside {
                steps.push(TradeHostStep::NewCaravanContact {
                    receiver_city: effective_second.city,
                    receiver_owner: effective_second.identity.who,
                    partner_city: effective_first.city,
                    partner_owner: effective_first.identity.who,
                });
            }
            arrival_branch = Some(TradeExecutorBranch::SecondEndpointTransition);
        }
    }

    // The transition check is at the endpoint being departed; every surviving frame then
    // advances toward the opposite endpoint.
    let movement_endpoint = if request.order.loaded != 0 {
        effective_second
    } else {
        effective_first
    };
    let movement = request
        .movement
        .ok_or(TradePlanError::MissingMovementFacts)?;
    let branch = movement_steps(arrival_endpoint, movement_endpoint, movement, &mut steps);
    Ok(TradeExecutorPlan {
        order_after: order,
        branch: arrival_branch.unwrap_or(branch),
        steps,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TradeAtomicSnapshot {
    pub actor_version: u64,
    pub order_version: u64,
    pub object_epoch: u64,
    pub leader_epoch: u64,
    pub city_epoch: u64,
    pub caravan_epoch: u64,
    pub path_epoch: u64,
    pub terrain_epoch: u64,
    pub rng_epoch: u64,
    pub effect_epoch: u64,
    pub facts: TradeFrameFacts,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TradeExecutorReceipt {
    pub snapshot: TradeAtomicSnapshot,
    pub plan: TradeExecutorPlan,
}

impl TradeExecutorReceipt {
    pub fn preflight(snapshot: TradeAtomicSnapshot) -> Result<Self, TradePlanError> {
        let plan = plan_trade_executor(&snapshot.facts)?;
        Ok(Self { snapshot, plan })
    }

    pub fn validates(&self, current: &TradeAtomicSnapshot) -> bool {
        &self.snapshot == current
            && plan_trade_executor(&current.facts).is_ok_and(|plan| plan == self.plan)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TradeOpenTail {
    ConcretePayloadSaveResume,
    CommandWireAndGroupActionTrade,
    GroupActionTradeAndQueueFirstCopyQuirk,
    DestinationCityAndLeaderRegistryScan,
    TerrainTransportAndCityVirtuals,
    CaravanRoadAStarParkedStateAndRng,
    RoadTerrainWritesAndRendererEffects,
    PathStackAssignmentSmoothingAndMoveInsertion,
    FindNearbySpotAndUnitMovement,
    CaravanPoolAndCityLinkAtomicAdapter,
    CityComputeTradeAndFirstContactEffects,
    UnitWorkRoadSearchContinuation,
    LiveTickAtomicCommit,
}

pub const TRADE_OPEN_TAILS: [TradeOpenTail; 13] = [
    TradeOpenTail::ConcretePayloadSaveResume,
    TradeOpenTail::CommandWireAndGroupActionTrade,
    TradeOpenTail::GroupActionTradeAndQueueFirstCopyQuirk,
    TradeOpenTail::DestinationCityAndLeaderRegistryScan,
    TradeOpenTail::TerrainTransportAndCityVirtuals,
    TradeOpenTail::CaravanRoadAStarParkedStateAndRng,
    TradeOpenTail::RoadTerrainWritesAndRendererEffects,
    TradeOpenTail::PathStackAssignmentSmoothingAndMoveInsertion,
    TradeOpenTail::FindNearbySpotAndUnitMovement,
    TradeOpenTail::CaravanPoolAndCityLinkAtomicAdapter,
    TradeOpenTail::CityComputeTradeAndFirstContactEffects,
    TradeOpenTail::UnitWorkRoadSearchContinuation,
    TradeOpenTail::LiveTickAtomicCommit,
];
