// SPDX-License-Identifier: GPL-3.0-or-later
//! Host-backed reconstruction of the garrison and path-scatter tail of
//! `Group::action_move_near` (`0x00704990`).
//!
//! This module deliberately owns no production hook.  It freezes two large, atomic seams
//! which the current command bridge still approximates:
//!
//! * the army/city garrison decision at `0x007054C7..0x00705748`; and
//! * the post-install `Stack<PathData>` transaction at
//!   `0x00706015..0x00706D5B`.
//!
//! The latter is 3,399 consecutive bytes.  It seeds a global path stack, calls
//! `PathFinder::find_wpath` in one of three exact shapes, scatters every popped path node by
//! each member's formation offset, performs the terrain-region/`invalid_loc` repair, appends
//! nodes to unit paths, and inverts those paths.  The planner below imports pathfinder and
//! world answers through a versioned host and commits the resulting state only after a full
//! recomputation.  Missing facts are errors, never permissive defaults.
//!
//! Capstone settles an ABI defect which the PDB cannot name: after `OrderIndex`, retail's
//! four integers are `group`, `form`, `width`, `disembark`, at stack offsets
//! `+0x24/+0x28/+0x2C/+0x30`.  Wire handlers pass `(1, form, width, disembark)`.  The current
//! Rust bridge omits `group`, shifting the final three values.  [`MoveNearAbi`] makes that
//! fact executable without changing the shared bridge before its coordinated hook.

use std::collections::BTreeSet;

pub const ACTION_MOVE_NEAR_VA: u32 = 0x0070_4990;
pub const ACTION_MOVE_NEAR_BYTES: usize = 9_205;
pub const GARRISON_SLICE_START: u32 = 0x0070_54c7;
pub const GARRISON_SLICE_END: u32 = 0x0070_5749;
pub const GARRISON_SLICE_BYTES: usize = (GARRISON_SLICE_END - GARRISON_SLICE_START) as usize;
pub const DISEMBARK_SLICE_START: u32 = 0x0070_6015;
pub const DISEMBARK_SLICE_END: u32 = 0x0070_6d5c;
pub const DISEMBARK_SLICE_BYTES: usize = (DISEMBARK_SLICE_END - DISEMBARK_SLICE_START) as usize;
pub const PATH_DATA_BYTES: usize = 16;
pub const FIND_WPATH_VA: u32 = 0x0068_8fc0;
pub const FIND_WPATH_STACK_BYTES: u32 = 0x14;
pub const ASTAR_PATH_VA: u32 = 0x0068_3770;
pub const GAME_RANDOM_GET_VA: u32 = 0x00a3_9d70;
pub const ADD_GARRISON_ORDER_VA: u32 = 0x005e_4080;
pub const ADD_MOVE_ORDER_VA: u32 = 0x0061_6ed0;
pub const PATH_STACK_PUSH_VA: u32 = 0x0046_d820;
pub const PATH_STACK_INVERT_VA: u32 = 0x0046_d9a0;

pub const QUEUE_NEW: i32 = 2;
pub const MOVE_TO: i32 = 1;
pub const ATTACK_ORDER: i32 = 10;
pub const DOMAIN_SEA: i32 = 1;
pub const FORM_SCATTER_PERMUTATION: i32 = 8;
pub const FORM_NO_DISTANCE_PRUNE: i32 = 9;
pub const LEADER_FLAG_DISABLE_SEA_LEADER_ONLY: u32 = 4;

/// Exact post-`OrderIndex` stack layout.  `ret 0x2c` proves eleven stack dwords total;
/// the loads and constructor pushes prove the names below.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MoveNearAbi {
    pub orders: i32,
    pub group: i32,
    pub form: i32,
    pub width: i32,
    pub disembark: i32,
}

impl MoveNearAbi {
    pub const ORDERS_STACK_OFFSET: u8 = 0x20;
    pub const GROUP_STACK_OFFSET: u8 = 0x24;
    pub const FORM_STACK_OFFSET: u8 = 0x28;
    pub const WIDTH_STACK_OFFSET: u8 = 0x2c;
    pub const DISEMBARK_STACK_OFFSET: u8 = 0x30;

    /// Both wire handlers inject literal one between `orders` and the three wire bytes.
    pub const fn from_wire(orders: i32, form: i32, width: i32, disembark: i32) -> Self {
        Self {
            orders,
            group: 1,
            form,
            width,
            disembark,
        }
    }

    pub const fn stack_tail(self) -> [i32; 5] {
        [
            self.orders,
            self.group,
            self.form,
            self.width,
            self.disembark,
        ]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UnitKey {
    pub owner: u8,
    pub object: i16,
}

impl UnitKey {
    pub const fn new(owner: u8, object: i16) -> Self {
        Self { owner, object }
    }

    fn safe(self) -> bool {
        self.owner < 8 && self.object >= 0
    }
}

/// PDB `PathData`, exactly 16 bytes: two `Coord`s and two `int`s.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PathData {
    pub to_x: i32,
    pub to_y: i32,
    pub tolerance: i32,
    pub flags: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CityTarget {
    pub key: UnitKey,
    pub type_index: i32,
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NearbySpotRequest {
    pub centre_x: i32,
    pub centre_y: i32,
    pub min_radius: i32,
    pub max_radius: i32,
    pub step: i32,
    pub angle: u32,
    pub filter: i32,
    pub actor: UnitKey,
    pub tail: [i32; 5],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NearbySpotAnswer {
    /// Retail's zero return is success.
    pub return_value: i32,
    pub out_x: i32,
    pub out_y: i32,
}

pub trait ArmyGarrisonHost {
    fn find_angle(&self, dx: i32, dy: i32) -> Option<u32>;
    fn find_nearby_spot(&self, request: &NearbySpotRequest) -> Option<NearbySpotAnswer>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArmyMemberFacts {
    pub actor: UnitKey,
    pub x: i32,
    pub y: i32,
    pub valid: bool,
    pub on_map: bool,
    /// The devirtualised `is_plane` result.  Helicopters are false here.
    pub plane_excluded: bool,
    pub is_supply: bool,
    pub is_siege: bool,
    pub is_hero: bool,
    pub current_order: Option<i32>,
    /// Required only for a supply/siege/hero and a found city target.
    pub can_garrison_city: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GarrisonInstall {
    pub actor: UnitKey,
    pub target: UnitKey,
    pub search: i32,
    pub queue: i32,
    pub group: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MoveInstall {
    pub actor: UnitKey,
    pub x: i32,
    pub y: i32,
    pub set_angle: i32,
    pub angle: i32,
    pub queue: i32,
    pub group: i32,
    /// Capstone proves retail forwards the selected Y coordinate in this unnamed ABI slot.
    pub raw_arg7: i32,
    pub original_x: i32,
    pub original_y: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArmyMemberDecision {
    /// Continue into the ordinary formation/order path at `0x00705749`.
    ContinueRegular,
    /// The member loop advances without installing another order.
    Skip,
    InstallGarrison(GarrisonInstall),
    InstallMove {
        install: MoveInstall,
        angle_request: (i32, i32),
        angle: u32,
        nearby_request: NearbySpotRequest,
        nearby_answer: NearbySpotAnswer,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArmyMemberPlanError {
    UnsafeActor(UnitKey),
    UnsafeCity(UnitKey),
    MissingCanGarrison,
    UnexpectedCanGarrison,
    MissingAngle,
    MissingNearbySpot,
}

/// Plan the complete army/city fork for one member.
///
/// `army_mode` is the local set only when leader flag bit four is clear and `Group::army`
/// is nonnegative. `city_lookup_ran` is the separate local set before `ObjectsData::find_city`;
/// a negative return still differs from a lookup which never ran.
pub fn plan_army_city_member<H: ArmyGarrisonHost>(
    army_mode: bool,
    city_lookup_ran: bool,
    city: Option<CityTarget>,
    member: &ArmyMemberFacts,
    host: &H,
) -> Result<ArmyMemberDecision, ArmyMemberPlanError> {
    if !member.actor.safe() {
        return Err(ArmyMemberPlanError::UnsafeActor(member.actor));
    }
    if let Some(city) = city {
        if !city.key.safe() {
            return Err(ArmyMemberPlanError::UnsafeCity(city.key));
        }
    }
    if !member.valid || !member.on_map || member.plane_excluded {
        if member.can_garrison_city.is_some() {
            return Err(ArmyMemberPlanError::UnexpectedCanGarrison);
        }
        return Ok(ArmyMemberDecision::Skip);
    }
    if !army_mode {
        if member.can_garrison_city.is_some() {
            return Err(ArmyMemberPlanError::UnexpectedCanGarrison);
        }
        return Ok(ArmyMemberDecision::ContinueRegular);
    }

    let candidate = member.is_supply || member.is_siege || member.is_hero;
    if city_lookup_ran {
        if let Some(city) = city {
            if !candidate {
                if member.can_garrison_city.is_some() {
                    return Err(ArmyMemberPlanError::UnexpectedCanGarrison);
                }
                return Ok(ArmyMemberDecision::ContinueRegular);
            }
            let can_garrison = member
                .can_garrison_city
                .ok_or(ArmyMemberPlanError::MissingCanGarrison)?;
            if can_garrison {
                return Ok(ArmyMemberDecision::InstallGarrison(GarrisonInstall {
                    actor: member.actor,
                    target: city.key,
                    search: 0,
                    queue: QUEUE_NEW,
                    group: 0,
                }));
            }

            let dx = member.x.wrapping_sub(city.x);
            let dy = member.y.wrapping_sub(city.y);
            let angle = host
                .find_angle(dx, dy)
                .ok_or(ArmyMemberPlanError::MissingAngle)?;
            let request = NearbySpotRequest {
                centre_x: city.x,
                centre_y: city.y,
                min_radius: 0x300,
                max_radius: 0x600,
                step: 0,
                angle,
                filter: 3,
                actor: member.actor,
                tail: [0, 0, -1, 0, -1],
            };
            let answer = host
                .find_nearby_spot(&request)
                .ok_or(ArmyMemberPlanError::MissingNearbySpot)?;
            let (x, y) = if answer.return_value == 0 {
                (answer.out_x, answer.out_y)
            } else {
                (city.x, city.y)
            };
            return Ok(ArmyMemberDecision::InstallMove {
                install: MoveInstall {
                    actor: member.actor,
                    x,
                    y,
                    set_angle: 1,
                    angle: 0,
                    queue: QUEUE_NEW,
                    group: 0,
                    raw_arg7: y,
                    original_x: -1,
                    original_y: -1,
                },
                angle_request: (dx, dy),
                angle,
                nearby_request: request,
                nearby_answer: answer,
            });
        }
    }

    if member.can_garrison_city.is_some() {
        return Err(ArmyMemberPlanError::UnexpectedCanGarrison);
    }
    if member.is_siege && member.current_order == Some(ATTACK_ORDER) {
        return Ok(ArmyMemberDecision::Skip);
    }
    Ok(ArmyMemberDecision::ContinueRegular)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TailUnit {
    pub key: UnitKey,
    pub valid: bool,
    pub on_map: bool,
    pub plane_excluded: bool,
    pub domain: i32,
    pub is_supply: bool,
    pub is_siege: bool,
    pub is_hero: bool,
    pub current_order: Option<i32>,
    /// Current order field `+0x1C`.  `astar_path` conditionally writes this from RNG.
    pub current_order_pause: Option<i32>,
    /// Unit byte `+0xB2`; `astar_path` conditionally adds thirty with byte wrapping.
    pub pathfinder_counter_b2: u8,
    /// Exact result of the unnamed virtual at `0x00607B40`; branch-lazy in retail.
    pub movement_gate_607b40: i32,
    pub x: i32,
    pub y: i32,
    pub path: Vec<PathData>,
    pub version: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormationTail {
    pub leader_index: usize,
    pub to_x: Vec<i32>,
    pub to_y: Vec<i32>,
    /// The static `SimpleArray<int>` at `0x00EE155C`, populated by `compute_form`.
    pub permutation: Vec<usize>,
    pub resolved_form: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TailWorldShape {
    /// Fields `World+0/+4`, used by the final clamp at `* 0x300`.
    pub tile_width: i32,
    pub tile_height: i32,
    /// Fields `World+0x18/+0x1C`, used by the first bound test at `* 0xC0`.
    pub coarse_width: i32,
    pub coarse_height: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MoveNearTailState {
    pub owner: u8,
    pub members: Vec<i16>,
    pub army_mode: bool,
    pub city_lookup_ran: bool,
    pub city: Option<UnitKey>,
    pub leader_flags: u32,
    pub orders: i32,
    pub tolerance: i32,
    pub origin_x: i32,
    pub origin_y: i32,
    pub formation: FormationTail,
    pub world: TailWorldShape,
    pub units: Vec<TailUnit>,
    pub group_order_num: i32,
    /// `memset(form + 0x30, 0, 0xE60)` at the end of the body.
    pub form_scratch: Vec<u8>,
    pub external_epoch: u64,
    pub rng_epoch: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FindWPathRequest {
    pub stack: Vec<PathData>,
    pub start_x: i32,
    pub start_y: i32,
    pub owner: u8,
    pub object: i16,
    /// Retail toggles `[0x00E85EB0]` only around the leader request for an army.
    pub army_path_mode: bool,
    /// Canonical RNG cursor on entry.  `find_wpath` itself draws none, but its one
    /// `astar_path` child has conditional `Random::get(0, 0xffff)` sites.
    pub rng_epoch: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PathFinderPauseMutation {
    pub before: i32,
    pub draw: u16,
    pub after: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PathFinderUnitMutation {
    pub actor: UnitKey,
    pub counter_b2_before: u8,
    pub counter_b2_after: u8,
    /// Present exactly when the reached `astar_path` branch writes current-order `+0x1C`.
    pub pause: Option<PathFinderPauseMutation>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FindWPathAnswer {
    pub return_value: i32,
    /// Complete stack after `find_wpath`; its return value is ignored by this caller.
    pub stack: Vec<PathData>,
    /// Exact canonical draws consumed transitively by `astar_path`, in order.
    pub rng_draws: Vec<u16>,
    pub rng_epoch_after: u64,
    /// The two RNG-bearing exits are mutually exclusive.  Either exit may also increment
    /// byte `+0xB2` without drawing, so the mutation is independent of `rng_draws`.
    pub unit_mutation: Option<PathFinderUnitMutation>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerrainRegionRequest {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidLocRequest {
    pub actor: UnitKey,
    pub x: i32,
    pub y: i32,
    pub raw: [i32; 6],
}

pub trait DisembarkTailHost {
    fn epoch(&self) -> u64;
    fn find_wpath(&self, request: &FindWPathRequest) -> Option<FindWPathAnswer>;
    fn terrain_region(&self, request: TerrainRegionRequest) -> Option<u16>;
    fn invalid_loc(&self, request: InvalidLocRequest) -> Option<bool>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TailHostCall {
    FindWPath {
        request: FindWPathRequest,
        answer: FindWPathAnswer,
    },
    TerrainRegion {
        request: TerrainRegionRequest,
        answer: u16,
    },
    InvalidLoc {
        request: InvalidLocRequest,
        answer: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MoveNearTailPlan {
    pub after: MoveNearTailState,
    pub host_calls: Vec<TailHostCall>,
    pub used_group_path: bool,
    pub straight_path: bool,
    /// Exact draws made by the transitive `astar_path` children, in call order.
    pub rng_draws: Vec<u16>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MoveNearTailReceipt {
    pub before: MoveNearTailState,
    pub host_epoch: u64,
    pub plan: MoveNearTailPlan,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TailPlanError {
    UnsafeOwner(u8),
    UnsafeMember(i16),
    DuplicateMember(i16),
    DuplicateUnit(UnitKey),
    MissingUnit(UnitKey),
    ForeignUnit(UnitKey),
    MemberUnitSetMismatch,
    InvalidFormationLengths,
    InvalidLeaderIndex(usize),
    InvalidPermutation,
    InvalidWorldShape,
    InvalidFormScratchLength(usize),
    MissingFindWPath(FindWPathRequest),
    InvalidFindWPathRngEpoch {
        before: u64,
        draws: usize,
        after: u64,
    },
    UnexpectedFindWPathDrawCount(usize),
    InvalidPathFinderMutationActor {
        expected: UnitKey,
        actual: UnitKey,
    },
    InvalidPathFinderCounter {
        actor: UnitKey,
        before: u8,
        after: u8,
    },
    MissingPathFinderPause(UnitKey),
    InvalidPathFinderPause {
        actor: UnitKey,
        before: i32,
        draw: u16,
        after: i32,
    },
    MissingTerrainRegion(TerrainRegionRequest),
    MissingInvalidLoc(InvalidLocRequest),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TailCommitError {
    StateChanged,
    HostEpochChanged,
    RecomputeChanged,
    RecomputeFailed,
}

fn unit_index(state: &MoveNearTailState, key: UnitKey) -> Option<usize> {
    state.units.iter().position(|unit| unit.key == key)
}

fn unit<'a>(state: &'a MoveNearTailState, key: UnitKey) -> Result<&'a TailUnit, TailPlanError> {
    state
        .units
        .iter()
        .find(|unit| unit.key == key)
        .ok_or(TailPlanError::MissingUnit(key))
}

fn base_eligible(unit: &TailUnit) -> bool {
    unit.valid && unit.on_map && !unit.plane_excluded
}

fn army_path_eligible(state: &MoveNearTailState, unit: &TailUnit) -> bool {
    if !state.army_mode {
        return true;
    }
    if state.city_lookup_ran && state.city.is_some() {
        return !unit.is_supply && !unit.is_siege && !unit.is_hero;
    }
    !(unit.is_siege && unit.current_order == Some(ATTACK_ORDER))
}

fn retail_vector_dist(dx: i32, dy: i32) -> i32 {
    let ax = dx.unsigned_abs() as u64;
    let ay = dy.unsigned_abs() as u64;
    let (large, small) = if ax >= ay { (ax, ay) } else { (ay, ax) };
    if large == 0 {
        return 0;
    }
    let value = if small >= 60_000 {
        (small + large.wrapping_mul(2)) >> 1
    } else {
        large + small.wrapping_mul(small) / large.wrapping_mul(2)
    };
    value as i32
}

fn path_start(unit: &TailUnit, state: &MoveNearTailState) -> (i32, i32) {
    unit.path
        .last()
        .map(|node| (node.to_x, node.to_y))
        .unwrap_or((state.origin_x, state.origin_y))
}

fn call_find_wpath<H: DisembarkTailHost>(
    host: &H,
    calls: &mut Vec<TailHostCall>,
    after: &mut MoveNearTailState,
    rng_draws: &mut Vec<u16>,
    request: FindWPathRequest,
) -> Result<FindWPathAnswer, TailPlanError> {
    let answer = host
        .find_wpath(&request)
        .ok_or_else(|| TailPlanError::MissingFindWPath(request.clone()))?;
    let expected_epoch = request
        .rng_epoch
        .wrapping_add(answer.rng_draws.len() as u64);
    if answer.rng_epoch_after != expected_epoch {
        return Err(TailPlanError::InvalidFindWPathRngEpoch {
            before: request.rng_epoch,
            draws: answer.rng_draws.len(),
            after: answer.rng_epoch_after,
        });
    }
    match answer.unit_mutation {
        None => {
            if !answer.rng_draws.is_empty() {
                return Err(TailPlanError::UnexpectedFindWPathDrawCount(
                    answer.rng_draws.len(),
                ));
            }
        }
        Some(mutation) => {
            let expected_actor = UnitKey::new(request.owner, request.object);
            if mutation.actor != expected_actor {
                return Err(TailPlanError::InvalidPathFinderMutationActor {
                    expected: expected_actor,
                    actual: mutation.actor,
                });
            }
            let index = unit_index(after, mutation.actor)
                .ok_or(TailPlanError::MissingUnit(mutation.actor))?;
            let target = &mut after.units[index];
            if target.pathfinder_counter_b2 != mutation.counter_b2_before
                || mutation.counter_b2_after != mutation.counter_b2_before.wrapping_add(30)
            {
                return Err(TailPlanError::InvalidPathFinderCounter {
                    actor: mutation.actor,
                    before: mutation.counter_b2_before,
                    after: mutation.counter_b2_after,
                });
            }
            match mutation.pause {
                None => {
                    if !answer.rng_draws.is_empty() {
                        return Err(TailPlanError::UnexpectedFindWPathDrawCount(
                            answer.rng_draws.len(),
                        ));
                    }
                }
                Some(pause) => {
                    if answer.rng_draws.as_slice() != [pause.draw] {
                        return Err(TailPlanError::UnexpectedFindWPathDrawCount(
                            answer.rng_draws.len(),
                        ));
                    }
                    if target.current_order_pause != Some(pause.before) {
                        return Err(TailPlanError::MissingPathFinderPause(mutation.actor));
                    }
                    let expected_after = i32::from(pause.draw) % 3 + 6;
                    if pause.after != expected_after {
                        return Err(TailPlanError::InvalidPathFinderPause {
                            actor: mutation.actor,
                            before: pause.before,
                            draw: pause.draw,
                            after: pause.after,
                        });
                    }
                    target.current_order_pause = Some(pause.after);
                }
            }
            target.pathfinder_counter_b2 = mutation.counter_b2_after;
            target.version = target.version.wrapping_add(1);
        }
    }
    rng_draws.extend_from_slice(&answer.rng_draws);
    after.rng_epoch = answer.rng_epoch_after;
    calls.push(TailHostCall::FindWPath {
        request,
        answer: answer.clone(),
    });
    Ok(answer)
}

fn call_region<H: DisembarkTailHost>(
    host: &H,
    calls: &mut Vec<TailHostCall>,
    request: TerrainRegionRequest,
) -> Result<u16, TailPlanError> {
    let answer = host
        .terrain_region(request)
        .ok_or(TailPlanError::MissingTerrainRegion(request))?;
    calls.push(TailHostCall::TerrainRegion { request, answer });
    Ok(answer)
}

fn call_invalid<H: DisembarkTailHost>(
    host: &H,
    calls: &mut Vec<TailHostCall>,
    request: InvalidLocRequest,
) -> Result<bool, TailPlanError> {
    let answer = host
        .invalid_loc(request)
        .ok_or(TailPlanError::MissingInvalidLoc(request))?;
    calls.push(TailHostCall::InvalidLoc { request, answer });
    Ok(answer)
}

fn clamp_scatter_axis(candidate: i32, coarse: i32, tiles: i32) -> i32 {
    if candidate < 0 || candidate >= coarse.wrapping_mul(0xc0) {
        candidate.clamp(0, tiles.wrapping_mul(0x300).wrapping_sub(1))
    } else {
        candidate
    }
}

fn validate_state(state: &MoveNearTailState) -> Result<(), TailPlanError> {
    if state.owner >= 8 {
        return Err(TailPlanError::UnsafeOwner(state.owner));
    }
    let mut members = BTreeSet::new();
    for &object in &state.members {
        if object < 0 {
            return Err(TailPlanError::UnsafeMember(object));
        }
        if !members.insert(object) {
            return Err(TailPlanError::DuplicateMember(object));
        }
    }
    let mut units = BTreeSet::new();
    for unit in &state.units {
        if !unit.key.safe() || unit.key.owner != state.owner {
            return Err(TailPlanError::ForeignUnit(unit.key));
        }
        if unit.current_order.is_some() != unit.current_order_pause.is_some() {
            return Err(TailPlanError::MissingPathFinderPause(unit.key));
        }
        if !units.insert(unit.key) {
            return Err(TailPlanError::DuplicateUnit(unit.key));
        }
    }
    let expected: BTreeSet<_> = state
        .members
        .iter()
        .map(|&object| UnitKey::new(state.owner, object))
        .collect();
    if units != expected {
        return Err(TailPlanError::MemberUnitSetMismatch);
    }
    let n = state.members.len();
    if state.formation.to_x.len() != n
        || state.formation.to_y.len() != n
        || state.formation.permutation.len() != n
    {
        return Err(TailPlanError::InvalidFormationLengths);
    }
    if n != 0 && state.formation.leader_index >= n {
        return Err(TailPlanError::InvalidLeaderIndex(
            state.formation.leader_index,
        ));
    }
    let permutation: BTreeSet<_> = state.formation.permutation.iter().copied().collect();
    if permutation.len() != n || permutation.iter().copied().ne(0..n) {
        return Err(TailPlanError::InvalidPermutation);
    }
    if state.world.tile_width <= 0
        || state.world.tile_height <= 0
        || state.world.coarse_width <= 0
        || state.world.coarse_height <= 0
    {
        return Err(TailPlanError::InvalidWorldShape);
    }
    if state.form_scratch.len() != 0xe60 {
        return Err(TailPlanError::InvalidFormScratchLength(
            state.form_scratch.len(),
        ));
    }
    Ok(())
}

fn scatter_node<H: DisembarkTailHost>(
    state: &MoveNearTailState,
    unit: &TailUnit,
    member_index: usize,
    leader: &TailUnit,
    node: PathData,
    straight: bool,
    host: &H,
    calls: &mut Vec<TailHostCall>,
) -> Result<Option<PathData>, TailPlanError> {
    if !base_eligible(unit) || !army_path_eligible(state, unit) {
        return Ok(None);
    }
    let leader_key = leader.key;
    if state.leader_flags & LEADER_FLAG_DISABLE_SEA_LEADER_ONLY == 0
        && unit.domain == DOMAIN_SEA
        && node.flags & 1 == 0
        && unit.key != leader_key
    {
        return Ok(None);
    }

    let formation_index = if state.formation.resolved_form == FORM_SCATTER_PERMUTATION
        && node.flags & 1 == 0
        && !straight
    {
        state.formation.permutation[member_index]
    } else {
        member_index
    };
    let leader_index = state.formation.leader_index;
    let dx = state.formation.to_x[formation_index].wrapping_sub(state.formation.to_x[leader_index]);
    let dy = state.formation.to_y[formation_index].wrapping_sub(state.formation.to_y[leader_index]);
    let mut x = node.to_x.wrapping_add(dx);
    let mut y = node.to_y.wrapping_add(dy);
    x = clamp_scatter_axis(x, state.world.coarse_width, state.world.tile_width);
    y = clamp_scatter_axis(y, state.world.coarse_height, state.world.tile_height);

    let node_region_request = TerrainRegionRequest {
        x: node.to_x,
        y: node.to_y,
    };
    let candidate_region_request = TerrainRegionRequest { x, y };
    let node_region = call_region(host, calls, node_region_request)?;
    let candidate_region = call_region(host, calls, candidate_region_request)?;
    if node_region != candidate_region {
        let request = InvalidLocRequest {
            actor: unit.key,
            x,
            y,
            raw: [1, 1, 0, 0, 0, 0],
        };
        if call_invalid(host, calls, request)? && !straight {
            x = x.wrapping_add(
                node.to_x
                    .wrapping_div(0x300)
                    .wrapping_sub(x.wrapping_div(0x300))
                    .wrapping_mul(0x300),
            );
            y = y.wrapping_add(
                node.to_y
                    .wrapping_div(0x300)
                    .wrapping_sub(y.wrapping_div(0x300))
                    .wrapping_mul(0x300),
            );
        }
    }

    if unit.key != leader_key && node.flags & 1 == 0 {
        if state.orders == MOVE_TO
            && unit.movement_gate_607b40 == 0
            && unit.domain != DOMAIN_SEA
            && state.formation.resolved_form != FORM_NO_DISTANCE_PRUNE
        {
            return Ok(None);
        }
        if retail_vector_dist(unit.x.wrapping_sub(leader.x), unit.y.wrapping_sub(leader.y)) >= 0x600
        {
            return Ok(None);
        }
    }

    Ok(Some(PathData {
        to_x: x,
        to_y: y,
        ..node
    }))
}

/// Preflight the complete 3,399-byte disembark/path-scatter transaction.
pub fn preflight_disembark_tail<H: DisembarkTailHost>(
    state: &MoveNearTailState,
    host: &H,
) -> Result<MoveNearTailReceipt, TailPlanError> {
    validate_state(state)?;
    let mut after = state.clone();
    let mut calls = Vec::new();
    let mut rng_draws = Vec::new();
    let mut group_stack = Vec::new();
    let mut straight = false;

    let leader = state
        .members
        .get(state.formation.leader_index)
        .copied()
        .map(|object| UnitKey::new(state.owner, object));
    if let Some(leader_key) = leader {
        let leader_unit = unit(state, leader_key)?;
        if base_eligible(leader_unit) {
            let seed = PathData {
                to_x: state.formation.to_x[state.formation.leader_index],
                to_y: state.formation.to_y[state.formation.leader_index],
                tolerance: state.tolerance,
                flags: 1,
            };
            group_stack.push(seed);
            let (start_x, start_y) = path_start(leader_unit, state);
            if retail_vector_dist(
                start_x.wrapping_sub(seed.to_x),
                start_y.wrapping_sub(seed.to_y),
            ) < 0x900
            {
                straight = true;
            } else {
                let request = FindWPathRequest {
                    stack: group_stack,
                    start_x,
                    start_y,
                    owner: state.owner,
                    object: leader_key.object,
                    army_path_mode: state.army_mode,
                    rng_epoch: after.rng_epoch,
                };
                group_stack =
                    call_find_wpath(host, &mut calls, &mut after, &mut rng_draws, request)?.stack;
            }
        }
    }

    let used_group_path = !group_stack.is_empty();
    if used_group_path {
        let leader_key = leader.expect("non-empty group path has a leader");
        let leader_unit = unit(state, leader_key)?.clone();
        while let Some(node) = group_stack.pop() {
            for (index, &object) in state.members.iter().enumerate() {
                let key = UnitKey::new(state.owner, object);
                let current = unit(state, key)?;
                if let Some(scattered) = scatter_node(
                    state,
                    current,
                    index,
                    &leader_unit,
                    node,
                    straight,
                    host,
                    &mut calls,
                )? {
                    let out_index = unit_index(&after, key).expect("validated exact unit set");
                    after.units[out_index].path.push(scattered);
                    after.units[out_index].version = after.units[out_index].version.wrapping_add(1);
                }
            }
        }
        // Retail's separate `0x007068F0` loop inverts every valid, on-map, non-plane unit,
        // including a unit filtered out of the scatter loop by an army predicate.
        for out in &mut after.units {
            let original = unit(state, out.key)?;
            if base_eligible(original) {
                out.path.reverse();
                out.version = out.version.wrapping_add(1);
            }
        }
    } else {
        for (index, &object) in state.members.iter().enumerate() {
            let key = UnitKey::new(state.owner, object);
            let current = unit(state, key)?;
            if !base_eligible(current) || !army_path_eligible(state, current) {
                continue;
            }
            let seed = PathData {
                to_x: state.formation.to_x[index],
                to_y: state.formation.to_y[index],
                tolerance: state.tolerance,
                flags: 1,
            };
            let (start_x, start_y) = path_start(current, state);
            let request = FindWPathRequest {
                stack: vec![seed],
                start_x,
                start_y,
                owner: state.owner,
                object,
                army_path_mode: false,
                rng_epoch: after.rng_epoch,
            };
            let mut stack =
                call_find_wpath(host, &mut calls, &mut after, &mut rng_draws, request)?.stack;
            let out_index = unit_index(&after, key).expect("validated exact unit set");
            if stack.is_empty() {
                after.units[out_index].path.push(seed);
                after.units[out_index].version = after.units[out_index].version.wrapping_add(1);
            } else {
                while let Some(node) = stack.pop() {
                    after.units[out_index].path.push(node);
                    after.units[out_index].version = after.units[out_index].version.wrapping_add(1);
                }
            }
            after.units[out_index].path.reverse();
            after.units[out_index].version = after.units[out_index].version.wrapping_add(1);
        }
    }

    after.group_order_num = after.group_order_num.wrapping_add(1);
    after.form_scratch.fill(0);
    // `find_wpath` owns no RNG itself.  Its one `astar_path` child conditionally draws,
    // and `call_find_wpath` has already imported those draws and coupled side effects.
    after.external_epoch = state.external_epoch;

    Ok(MoveNearTailReceipt {
        before: state.clone(),
        host_epoch: host.epoch(),
        plan: MoveNearTailPlan {
            after,
            host_calls: calls,
            used_group_path,
            straight_path: straight,
            rng_draws,
        },
    })
}

/// Commit only the exact receipt which a fresh preflight reproduces.
pub fn commit_disembark_tail<H: DisembarkTailHost>(
    state: &mut MoveNearTailState,
    receipt: &MoveNearTailReceipt,
    host: &H,
) -> Result<(), TailCommitError> {
    if *state != receipt.before {
        return Err(TailCommitError::StateChanged);
    }
    if host.epoch() != receipt.host_epoch {
        return Err(TailCommitError::HostEpochChanged);
    }
    let recomputed =
        preflight_disembark_tail(state, host).map_err(|_| TailCommitError::RecomputeFailed)?;
    if recomputed != *receipt {
        return Err(TailCommitError::RecomputeChanged);
    }
    *state = receipt.plan.after.clone();
    Ok(())
}

/// Integration inventory.  No item can be removed by a source-only planner.
pub const MOVE_NEAR_TAIL_OPEN_TAILS: [&str; 6] = [
    "CorrectFiveWordActionMoveNearAbi",
    "ProductionFormationAndPermutationHost",
    "ProductionPathFinderAndWorldRegionHost",
    "AtomicUnitPathAndGroupCommit",
    "CommandAndSaveRoundTrip",
    "LiveTickAndReplayChecksumEvidence",
];
