//! Ordinary on-map gathering attachment, building approach, and payout activation.
//!
//! This module is the executable seam between [`super::gathering`]'s owner-local gather
//! chain and [`super::containment`]'s exact collision-valid nearby finder.  It covers the
//! ordinary Citizen profile (TypeIndex `0x32`/`0x33`) for Farm (`0x1A1`), Woodcutter/Camp
//! (`0x1A2`), and Mine (`0x1A3`).  Those workers remain on-map and collidable throughout;
//! attachment never means containment.
//!
//! The ordering comes from `Unit::add_gather_order` (`0x0061A5C0`), `Unit::do_gather`
//! (`0x005EF2A0`), and `Unit::do_non_flat_gather` (`0x005F0170`):
//!
//! 1. validate the generational target and link the worker with `Build::add_gatherer`;
//! 2. for Camp/Mine, ask the mandatory `Unit::is_at` host boundary;
//! 3. if not there, run `UnitType::find_nearby_spot` with the exact bitmap and ordered
//!    collision views and emit `add_move_order(x,y,1,0,0,0,-1,-1)`;
//! 4. set `GatherOrder::been_there` only on the Camp/Mine arrival tick.  Farm is different:
//!    its first `do_gather` tick sets `been_there` before its on-footprint queued move.
//!
//! A move plan is never applied here.  In particular, no path replaces it with
//! `set_new_location`, a fabricated seat, or an offset from the building.  Missing
//! `Unit::is_at`, bitmap state, ordered collision, type data, or object identity is an
//! error.  A retail finder exhaustion is a modeled result and retains the worker's world
//! link and collision stamp.

use crate::rng::Random;

use super::collision::{CollCheck, CollUnits, DOMAIN_LAND};
use super::containment::{
    find_gather_nearby_spot, ordinary_gather_release_in_place, InPlaceGatherReleaseError,
    NearbySearchError, NearbySearchTrace, NearbyUnitType, OrderedCollision,
};
use super::gathering::{
    attach_worker, begin_non_flat_gather_tick, gather_building_approach_radius,
    initial_farm_gather_move, num_gatherers, retire_gather_order, AttachResult, GatherCount,
    GatherInsideObject, GatherNearbyPoint, GatherNearbySpotRequest, GatherRetirement, GatherSite,
    GatherTile, GatherWorker, NonFlatGatherState,
};
use super::map_terrain::{TCoord, World};
use super::movement::{find_angle, vector_dist};

/// The three ordinary building-gather properties admitted by this lifecycle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrdinaryGatherKind {
    Farm,
    Camp,
    Mine,
}

impl OrdinaryGatherKind {
    /// `ObjectTypeData::property` stored in `GatherOrder::build_type`.
    pub const fn property(self) -> i32 {
        match self {
            Self::Farm => 0x1a1,
            Self::Camp => 0x1a2,
            Self::Mine => 0x1a3,
        }
    }

    const fn initial_non_flat(self) -> u8 {
        match self {
            Self::Farm => 0,
            Self::Camp | Self::Mine => 1,
        }
    }

    const fn initial_dist_mod(self) -> u8 {
        match self {
            Self::Farm => 0,
            Self::Camp => 4,
            Self::Mine => 10,
        }
    }
}

/// Exact target fields read by the supported ordinary branches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OrdinaryGatherTarget {
    pub kind: OrdinaryGatherKind,
    pub centre: GatherNearbyPoint,
    /// Top-left footprint tile.  Camp/Mine building approach does not consume this field;
    /// Farm's footprint movement does.
    pub corner: GatherTile,
    pub x_size: i32,
    pub y_size: i32,
    /// `ObjectTypeData::domain` for the target building.
    pub domain: i32,
    /// Result of the target's validity/building/completed virtual gates.
    pub completed: bool,
}

/// GatherOrder constructor state after `add_gather_order` successfully performs its
/// immediate ordinary-site attachment for the shipped Farm/Camp/Mine profiles.
///
/// This is not a fallback for an attachment that failed.  `ensure_ordinary_attachment`
/// reports that failure explicitly; callers install this state only for an accepted order.
pub const fn attached_ordinary_order_state(kind: OrdinaryGatherKind) -> NonFlatGatherState {
    NonFlatGatherState {
        tx: -1,
        ty: -1,
        build_type: kind.property(),
        wait: 0,
        goto_build: 1,
        non_flat_gather: kind.initial_non_flat(),
        dist_mod: kind.initial_dist_mod(),
        been_there: 0,
    }
}

/// Exact tail of the gather call to `Unit::add_move_order` (`0x00616ED0`).
///
/// The PDB identifies the function and the two leading `Coord` parameters; the remaining
/// values are retained in call order instead of assigning speculative semantic names.
pub const ORDINARY_GATHER_MOVE_TAIL: [i32; 6] = [1, 0, 0, 0, -1, -1];

/// A queued retail movement request.  Applying it is owned by the order-list adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OrdinaryGatherMovePlan {
    pub destination: GatherNearbyPoint,
    pub add_move_order_tail: [i32; 6],
}

impl OrdinaryGatherMovePlan {
    const fn new(destination: GatherNearbyPoint) -> Self {
        Self {
            destination,
            add_move_order_tail: ORDINARY_GATHER_MOVE_TAIL,
        }
    }
}

/// Exact read required for the virtual `Unit::is_at(Object*)` gate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OrdinaryIsAtQuery {
    pub worker: GatherInsideObject,
    pub target: GatherInsideObject,
    pub worker_anchor: GatherNearbyPoint,
    pub target_centre: GatherNearbyPoint,
}

/// Mandatory host boundary for `Unit::is_at` (vtable `+0x170`).
///
/// This method has no geometric fallback.  Retail's predicate includes live object/type
/// state beyond a centre-distance comparison, so an Arena radius guess is not admitted.
pub trait AuthoritativeOrdinaryGatherGeometry {
    type Error;

    fn unit_is_at_gather_target(&self, query: OrdinaryIsAtQuery) -> Result<bool, Self::Error>;
}

/// Result of the attachment boundary.  `Full` is a normal retail refusal, not missing
/// evidence.  The other two statuses both mean the worker is linked when this returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OrdinaryAttachment {
    pub status: AttachResult,
    pub anchor: GatherNearbyPoint,
}

/// Fail-closed lifecycle errors.  A geometry/search error can occur after an attachment
/// was committed because retail attaches before either query; callers must retain that
/// gather-chain state and retry/abort through the normal order boundary.
#[derive(Debug, PartialEq, Eq)]
pub enum OrdinaryGatherLifecycleError<G, O> {
    MissingWorker(u8, i16),
    NotOrdinaryWorker(i32),
    WorkerTypeMismatch {
        row_type: i32,
        supplied_type: i32,
    },
    TargetNotCompleted,
    UnsupportedTargetDomain(i32),
    InvalidFootprint(i32, i32),
    InvalidTargetCentre(GatherNearbyPoint),
    InvalidWorkerAnchor(GatherNearbyPoint),
    AssignmentDoesNotTargetSite,
    SplitBeenThere {
        order: bool,
        phase: bool,
    },
    WrongBuildType {
        expected: i32,
        actual: i32,
    },
    WrongNonFlatPhase {
        goto_build: u8,
        non_flat_gather: u8,
        wait: i32,
    },
    InvalidGatherChain(&'static str),
    AttachmentRejected(AttachResult),
    InPlace(InPlaceGatherReleaseError),
    Geometry(G),
    Nearby(NearbySearchError<O>),
    /// Only the measured `FarmData::update == 1` arm is executable here.  The alternate
    /// farm-cell animation/table arm must be supplied by its own exact host before use.
    UnsupportedFarmUpdateResult(i32),
}

fn worker_pos(workers: &[GatherWorker], owner: u8, unit_o: i16) -> Option<usize> {
    workers
        .iter()
        .position(|worker| worker.owner == owner && worker.unit_o == unit_o)
}

fn linked_to_site(
    site: &GatherSite,
    workers: &[GatherWorker],
    unit_o: i16,
) -> Result<bool, &'static str> {
    let mut current = site.gather_down;
    for _ in 0..=workers.len() {
        if current < 0 {
            return Ok(false);
        }
        if current == unit_o {
            return Ok(true);
        }
        let Some(pos) = worker_pos(workers, site.owner, current) else {
            return Err("gather chain references a missing owner-local unit");
        };
        current = workers[pos].gather_down;
    }
    Err("gather chain contains a cycle")
}

fn validate_target<G, O>(
    world: Option<&World>,
    target: OrdinaryGatherTarget,
) -> Result<(), OrdinaryGatherLifecycleError<G, O>> {
    if !target.completed {
        return Err(OrdinaryGatherLifecycleError::TargetNotCompleted);
    }
    if target.domain != DOMAIN_LAND {
        return Err(OrdinaryGatherLifecycleError::UnsupportedTargetDomain(
            target.domain,
        ));
    }
    if target.x_size <= 0 || target.y_size <= 0 {
        return Err(OrdinaryGatherLifecycleError::InvalidFootprint(
            target.x_size,
            target.y_size,
        ));
    }
    if world.is_some_and(|world| !world.valid_coord(target.centre.x.0, target.centre.y.0)) {
        return Err(OrdinaryGatherLifecycleError::InvalidTargetCentre(
            target.centre,
        ));
    }
    Ok(())
}

/// Validate and, if necessary, attach one ordinary worker without touching world location.
///
/// The already-linked check precedes the capacity check, matching `Unit::do_gather`'s
/// `is_gathered_by` branch.  This matters when a site is exactly full: an existing member
/// remains a member instead of being reported as a new over-capacity request.
fn ensure_ordinary_attachment_impl<U, G, O>(
    bitmap_units: &U,
    site: &mut GatherSite,
    workers: &mut [GatherWorker],
    unit_o: i16,
    target: OrdinaryGatherTarget,
    unit_type: NearbyUnitType,
) -> Result<OrdinaryAttachment, OrdinaryGatherLifecycleError<G, O>>
where
    U: CollUnits,
{
    validate_target(None, target)?;
    let Some(pos) = worker_pos(workers, site.owner, unit_o) else {
        return Err(OrdinaryGatherLifecycleError::MissingWorker(
            site.owner, unit_o,
        ));
    };
    let worker = workers[pos];
    if !matches!(worker.type_index, 0x32 | 0x33) {
        return Err(OrdinaryGatherLifecycleError::NotOrdinaryWorker(
            worker.type_index,
        ));
    }
    if worker.type_index != unit_type.type_index {
        return Err(OrdinaryGatherLifecycleError::WorkerTypeMismatch {
            row_type: worker.type_index,
            supplied_type: unit_type.type_index,
        });
    }
    if unit_type.domain != DOMAIN_LAND {
        return Err(OrdinaryGatherLifecycleError::UnsupportedTargetDomain(
            unit_type.domain,
        ));
    }
    if !worker
        .assignment
        .is_some_and(|assignment| assignment.targets_live_site(site))
    {
        return Err(OrdinaryGatherLifecycleError::AssignmentDoesNotTargetSite);
    }

    let id = GatherInsideObject {
        owner: site.owner as i8,
        object: unit_o,
    };
    let release = ordinary_gather_release_in_place(bitmap_units, id, worker.type_index)
        .map_err(OrdinaryGatherLifecycleError::InPlace)?;
    let already = linked_to_site(site, workers, unit_o)
        .map_err(OrdinaryGatherLifecycleError::InvalidGatherChain)?;
    if already {
        return Ok(OrdinaryAttachment {
            status: AttachResult::AlreadyAttached,
            anchor: release.point,
        });
    }
    let status = attach_worker(site, workers, unit_o);
    match status {
        AttachResult::Attached | AttachResult::Full => Ok(OrdinaryAttachment {
            status,
            anchor: release.point,
        }),
        AttachResult::AlreadyAttached => Ok(OrdinaryAttachment {
            status,
            anchor: release.point,
        }),
        AttachResult::Invalid => Err(OrdinaryGatherLifecycleError::AttachmentRejected(status)),
    }
}

/// Public attachment-only form. The two impossible error parameters are fixed so callers
/// do not need to name unrelated geometry or ordered-collision error types.
pub fn ensure_ordinary_attachment<U>(
    bitmap_units: &U,
    site: &mut GatherSite,
    workers: &mut [GatherWorker],
    unit_o: i16,
    target: OrdinaryGatherTarget,
    unit_type: NearbyUnitType,
) -> Result<
    OrdinaryAttachment,
    OrdinaryGatherLifecycleError<std::convert::Infallible, std::convert::Infallible>,
>
where
    U: CollUnits,
{
    ensure_ordinary_attachment_impl(bitmap_units, site, workers, unit_o, target, unit_type)
}

/// `UnitData::can_transport` (`0x0046F960`) reduced to the three fields it reads.
#[inline]
pub const fn ordinary_can_transport(unit_masks: u32, object_masks: u32, unit_flags: u32) -> bool {
    !(((unit_masks & 0x0080_0000) == 0 || (object_masks & 0x2000) != 0) && (unit_flags & 0x10) == 0)
}

fn active_count<G, O>(
    site: &GatherSite,
    workers: &[GatherWorker],
) -> Result<i32, OrdinaryGatherLifecycleError<G, O>> {
    num_gatherers(site, workers, GatherCount::Active, 0)
        .map_err(OrdinaryGatherLifecycleError::InvalidGatherChain)
}

fn validate_phase<G, O>(
    kind: OrdinaryGatherKind,
    worker: &GatherWorker,
    state: &NonFlatGatherState,
    require_non_flat: bool,
) -> Result<(), OrdinaryGatherLifecycleError<G, O>> {
    if state.build_type != kind.property() {
        return Err(OrdinaryGatherLifecycleError::WrongBuildType {
            expected: kind.property(),
            actual: state.build_type,
        });
    }
    let order_been = worker
        .assignment
        .ok_or(OrdinaryGatherLifecycleError::AssignmentDoesNotTargetSite)?
        .been_there;
    let phase_been = state.been_there != 0;
    if order_been != phase_been {
        return Err(OrdinaryGatherLifecycleError::SplitBeenThere {
            order: order_been,
            phase: phase_been,
        });
    }
    if state.goto_build != 1
        || (require_non_flat && state.non_flat_gather != 1)
        || (!require_non_flat && state.non_flat_gather != 0)
        || (require_non_flat && state.wait < 0)
    {
        return Err(OrdinaryGatherLifecycleError::WrongNonFlatPhase {
            goto_build: state.goto_build,
            non_flat_gather: state.non_flat_gather,
            wait: state.wait,
        });
    }
    Ok(())
}

fn store_been_there(worker: &mut GatherWorker, state: &mut NonFlatGatherState) -> bool {
    if state.been_there != 0 {
        return false;
    }
    state.been_there = 1;
    if let Some(assignment) = worker.assignment.as_mut() {
        assignment.been_there = true;
    }
    true
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CampMineDisposition {
    /// The caller must queue the returned MoveOrder; the worker was not relocated here.
    QueueMove(OrdinaryGatherMovePlan),
    /// `Unit::is_at` was true. `been_there` now admits this worker to payout.
    Arrived,
    /// The normal finder exhausted while the worker was within `0x600`, or this was a
    /// later pass. Retail stores `wait=-1` and keeps the order/attachment.
    NearSearchBlocked,
    /// Both the normal and far (`min=0x600`) searches exhausted. Retail kills the order;
    /// the gather retirement epilogue has been applied here.
    RetiredFarBlocked(GatherRetirement),
    /// The site refused a new link at its authoritative signed-byte capacity. The Gather
    /// retirement epilogue has cleared the staged order state; the host still queues
    /// retail's follow-on Think order.
    RetiredAtCapacity(GatherRetirement),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CampMineApproachOutcome {
    pub attachment: OrdinaryAttachment,
    pub disposition: CampMineDisposition,
    pub primary_search: Option<NearbySearchTrace>,
    pub far_search: Option<NearbySearchTrace>,
    pub leader_economy_dirty: bool,
    pub latched_site_recharge: bool,
    pub active_workers_after: i32,
}

/// Execute the exact Camp/Mine building-approach and arrival boundary.
///
/// The supplied state must be the `goto_build=1, non_flat_gather=1, wait>=0` phase.  Tile
/// selection and the later terrain-tile approach remain in [`super::gathering`].
#[allow(clippy::too_many_arguments)]
pub fn camp_mine_building_approach<U, O, G>(
    world: &mut World,
    collcheck: &mut CollCheck,
    bitmap_units: &U,
    ordered: &mut O,
    geometry: &G,
    site: &mut GatherSite,
    workers: &mut [GatherWorker],
    unit_o: i16,
    state: &mut NonFlatGatherState,
    target: OrdinaryGatherTarget,
    unit_type: NearbyUnitType,
    object_masks: u32,
) -> Result<CampMineApproachOutcome, OrdinaryGatherLifecycleError<G::Error, O::Error>>
where
    U: CollUnits,
    O: OrderedCollision,
    G: AuthoritativeOrdinaryGatherGeometry,
{
    if !matches!(
        target.kind,
        OrdinaryGatherKind::Camp | OrdinaryGatherKind::Mine
    ) {
        return Err(OrdinaryGatherLifecycleError::WrongBuildType {
            expected: OrdinaryGatherKind::Camp.property(),
            actual: target.kind.property(),
        });
    }
    validate_target(Some(world), target)?;
    let pos = worker_pos(workers, site.owner, unit_o).ok_or(
        OrdinaryGatherLifecycleError::MissingWorker(site.owner, unit_o),
    )?;
    validate_phase(target.kind, &workers[pos], state, true)?;
    let attachment = ensure_ordinary_attachment_impl::<U, G::Error, O::Error>(
        bitmap_units,
        site,
        workers,
        unit_o,
        target,
        unit_type,
    )?;
    if attachment.status == AttachResult::Full {
        let owner = site.owner;
        let retirement = retire_gather_order(Some(site), workers, owner, unit_o, None)
            .map_err(OrdinaryGatherLifecycleError::InvalidGatherChain)?;
        return Ok(CampMineApproachOutcome {
            attachment,
            disposition: CampMineDisposition::RetiredAtCapacity(retirement),
            primary_search: None,
            far_search: None,
            leader_economy_dirty: true,
            latched_site_recharge: false,
            active_workers_after: active_count(site, workers)?,
        });
    }
    if !world.valid_coord(attachment.anchor.x.0, attachment.anchor.y.0) {
        return Err(OrdinaryGatherLifecycleError::InvalidWorkerAnchor(
            attachment.anchor,
        ));
    }
    let begin = begin_non_flat_gather_tick(site, &mut workers[pos], state);

    let worker_id = GatherInsideObject {
        owner: site.owner as i8,
        object: unit_o,
    };
    let target_id = GatherInsideObject {
        owner: site.owner as i8,
        object: site.build_o,
    };
    let at_target = geometry
        .unit_is_at_gather_target(OrdinaryIsAtQuery {
            worker: worker_id,
            target: target_id,
            worker_anchor: attachment.anchor,
            target_centre: target.centre,
        })
        .map_err(OrdinaryGatherLifecycleError::Geometry)?;
    if at_target {
        state.wait = state.wait.wrapping_sub(1);
        let dirty = store_been_there(&mut workers[pos], state);
        return Ok(CampMineApproachOutcome {
            attachment,
            disposition: CampMineDisposition::Arrived,
            primary_search: None,
            far_search: None,
            leader_economy_dirty: begin.leader_economy_dirty || dirty,
            latched_site_recharge: begin.latched_site_recharge,
            active_workers_after: active_count(site, workers)?,
        });
    }

    let worker_tx = TCoord::from_coord(attachment.anchor.x).0;
    let worker_ty = TCoord::from_coord(attachment.anchor.y).0;
    let target_tx = TCoord::from_coord(target.centre.x).0;
    let target_ty = TCoord::from_coord(target.centre.y).0;
    let can_transport =
        ordinary_can_transport(workers[pos].unit_masks, object_masks, unit_type.unit_flags);
    if world.get_tregion(worker_tx, worker_ty) != world.get_tregion(target_tx, target_ty)
        && can_transport
    {
        return Ok(CampMineApproachOutcome {
            attachment,
            disposition: CampMineDisposition::QueueMove(OrdinaryGatherMovePlan::new(target.centre)),
            primary_search: None,
            far_search: None,
            leader_economy_dirty: begin.leader_economy_dirty,
            latched_site_recharge: begin.latched_site_recharge,
            active_workers_after: active_count(site, workers)?,
        });
    }

    let base_angle = find_angle(
        target.centre.x.0.wrapping_sub(attachment.anchor.x.0),
        target.centre.y.0.wrapping_sub(attachment.anchor.y.0),
    ) as u32;
    let normal = GatherNearbySpotRequest::building(
        target.centre,
        gather_building_approach_radius(target.x_size, target.y_size),
        base_angle,
        unit_type.type_index,
        i32::from(unit_o),
        i32::from(site.owner),
    );
    let primary =
        find_gather_nearby_spot(world, collcheck, bitmap_units, ordered, unit_type, normal)
            .map_err(OrdinaryGatherLifecycleError::Nearby)?;
    if let Some(point) = primary.point {
        return Ok(CampMineApproachOutcome {
            attachment,
            disposition: CampMineDisposition::QueueMove(OrdinaryGatherMovePlan::new(point)),
            primary_search: Some(primary.trace),
            far_search: None,
            leader_economy_dirty: begin.leader_economy_dirty,
            latched_site_recharge: begin.latched_site_recharge,
            active_workers_after: active_count(site, workers)?,
        });
    }

    let distance = vector_dist(
        target.centre.x.0.wrapping_sub(attachment.anchor.x.0),
        target.centre.y.0.wrapping_sub(attachment.anchor.y.0),
    );
    if state.been_there == 0 && distance > 0x600 {
        let far = GatherNearbySpotRequest::building(
            target.centre,
            0x600,
            base_angle,
            unit_type.type_index,
            i32::from(unit_o),
            i32::from(site.owner),
        );
        let far = find_gather_nearby_spot(world, collcheck, bitmap_units, ordered, unit_type, far)
            .map_err(OrdinaryGatherLifecycleError::Nearby)?;
        if let Some(point) = far.point {
            return Ok(CampMineApproachOutcome {
                attachment,
                disposition: CampMineDisposition::QueueMove(OrdinaryGatherMovePlan::new(point)),
                primary_search: Some(primary.trace),
                far_search: Some(far.trace),
                leader_economy_dirty: begin.leader_economy_dirty,
                latched_site_recharge: begin.latched_site_recharge,
                active_workers_after: active_count(site, workers)?,
            });
        }
        let retirement = retire_gather_order(
            Some(site),
            workers,
            worker_id.owner as u8,
            worker_id.object,
            None,
        )
        .map_err(OrdinaryGatherLifecycleError::InvalidGatherChain)?;
        return Ok(CampMineApproachOutcome {
            attachment,
            disposition: CampMineDisposition::RetiredFarBlocked(retirement),
            primary_search: Some(primary.trace),
            far_search: Some(far.trace),
            leader_economy_dirty: true,
            latched_site_recharge: begin.latched_site_recharge,
            active_workers_after: active_count(site, workers)?,
        });
    }

    state.wait = -1;
    Ok(CampMineApproachOutcome {
        attachment,
        disposition: CampMineDisposition::NearSearchBlocked,
        primary_search: Some(primary.trace),
        far_search: None,
        leader_economy_dirty: begin.leader_economy_dirty,
        latched_site_recharge: begin.latched_site_recharge,
        active_workers_after: active_count(site, workers)?,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FarmFirstTickDisposition {
    Active {
        /// `None` is the measured low-byte gate declining a move, not a missing path.
        move_order: Option<OrdinaryGatherMovePlan>,
        action: i32,
        rng_draws: usize,
    },
    RetiredAtCapacity(GatherRetirement),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FarmFirstTickOutcome {
    pub attachment: OrdinaryAttachment,
    pub disposition: FarmFirstTickDisposition,
    pub leader_economy_dirty: bool,
    pub active_workers_after: i32,
}

/// Execute Farm's admitted first-tick branch while keeping the worker on-map.
///
/// `farm_update_result` is the exact return from `0x008D9160`.  Result `1` selects action
/// `0x23` and the recovered one-in-256 footprint move.  Other results select a separate
/// farm-cell animation/table branch which this API rejects rather than treating as result
/// `1`.  The existing general [`super::gathering::farm_gather_relocation`] primitive can
/// be used only after that table branch is recovered by its owner.
#[allow(clippy::too_many_arguments)]
pub fn farm_first_gather_tick<U, G, O>(
    bitmap_units: &U,
    site: &mut GatherSite,
    workers: &mut [GatherWorker],
    unit_o: i16,
    state: &mut NonFlatGatherState,
    target: OrdinaryGatherTarget,
    unit_type: NearbyUnitType,
    farm_update_result: i32,
    game_gate_value: i32,
    rng: &mut Random,
) -> Result<FarmFirstTickOutcome, OrdinaryGatherLifecycleError<G, O>>
where
    U: CollUnits,
{
    if target.kind != OrdinaryGatherKind::Farm {
        return Err(OrdinaryGatherLifecycleError::WrongBuildType {
            expected: OrdinaryGatherKind::Farm.property(),
            actual: target.kind.property(),
        });
    }
    validate_target(None, target)?;
    if farm_update_result != 1 {
        return Err(OrdinaryGatherLifecycleError::UnsupportedFarmUpdateResult(
            farm_update_result,
        ));
    }
    let pos = worker_pos(workers, site.owner, unit_o).ok_or(
        OrdinaryGatherLifecycleError::MissingWorker(site.owner, unit_o),
    )?;
    validate_phase(target.kind, &workers[pos], state, false)?;
    let attachment = ensure_ordinary_attachment_impl::<U, G, O>(
        bitmap_units,
        site,
        workers,
        unit_o,
        target,
        unit_type,
    )?;
    if attachment.status == AttachResult::Full {
        let owner = site.owner;
        let retirement = retire_gather_order(Some(site), workers, owner, unit_o, None)
            .map_err(OrdinaryGatherLifecycleError::InvalidGatherChain)?;
        return Ok(FarmFirstTickOutcome {
            attachment,
            disposition: FarmFirstTickDisposition::RetiredAtCapacity(retirement),
            leader_economy_dirty: true,
            active_workers_after: active_count(site, workers)?,
        });
    }
    let dirty = store_been_there(&mut workers[pos], state);
    let movement = initial_farm_gather_move(
        game_gate_value,
        i32::from(unit_o),
        i32::from(site.owner),
        target.corner.tx,
        target.corner.ty,
        target.x_size,
        target.y_size,
        rng,
    )
    .map_err(|error| match error {
        super::gathering::GatherTerrainError::InvalidFootprint(x, y) => {
            OrdinaryGatherLifecycleError::InvalidFootprint(x, y)
        }
        _ => unreachable!("initial Farm move only validates the footprint"),
    })?;
    let (move_order, rng_draws) = movement
        .map(|movement| {
            (
                Some(OrdinaryGatherMovePlan::new(movement.destination)),
                movement.rng_draws,
            )
        })
        .unwrap_or((None, 0));
    Ok(FarmFirstTickOutcome {
        attachment,
        disposition: FarmFirstTickDisposition::Active {
            move_order,
            action: 0x23,
            rng_draws,
        },
        leader_economy_dirty: dirty,
        active_workers_after: active_count(site, workers)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systems::collision::{place, CollGuy, UnitRow, UnitTable};
    use crate::systems::containment::{OrderedCollision, OrderedCollisionQuery};
    use crate::systems::gathering::{GatherAssignment, NO_OBJECT};
    use crate::systems::map_terrain::{Coord, UCoord};

    #[derive(Default)]
    struct OrderedHost {
        blocked: bool,
        fail: bool,
        calls: Vec<OrderedCollisionQuery>,
    }

    impl OrderedCollision for OrderedHost {
        type Error = &'static str;

        fn find_ordered_collision(
            &mut self,
            query: OrderedCollisionQuery,
        ) -> Result<bool, Self::Error> {
            self.calls.push(query);
            if self.fail {
                Err("ordered collision unavailable")
            } else {
                Ok(self.blocked)
            }
        }
    }

    struct Geometry(Result<bool, &'static str>);

    impl AuthoritativeOrdinaryGatherGeometry for Geometry {
        type Error = &'static str;

        fn unit_is_at_gather_target(&self, _query: OrdinaryIsAtQuery) -> Result<bool, Self::Error> {
            self.0
        }
    }

    fn coord(ucell: i32) -> i32 {
        UCoord(ucell).centre().0
    }

    fn setup(kind: OrdinaryGatherKind) -> (World, UnitTable, GatherSite, Vec<GatherWorker>) {
        let mut world = World::init(8, 8, 44, 4, 4);
        let mut units = UnitTable::default();
        place(
            &mut world,
            &mut units,
            UnitRow {
                who: 0,
                o: 7,
                x: coord(10),
                y: coord(10),
                down: NO_OBJECT,
                down_who: NO_OBJECT,
                domain: DOMAIN_LAND,
                block_radius: 1,
                on_map: true,
                active: true,
                ..UnitRow::default()
            },
            [CollGuy {
                x: coord(10),
                y: coord(10),
                block_radius: 1,
            }],
        );
        let mut site = GatherSite::new(0, 4);
        site.uid = 99;
        site.set_authoritative_capacity(2);
        let mut worker = GatherWorker::new(0, 7, 0x32);
        worker.assignment = Some(GatherAssignment {
            target_owner: 0,
            target_build: 4,
            target_uid: 99,
            been_there: false,
            inside_target: None,
        });
        let state = attached_ordinary_order_state(kind);
        assert_eq!(state.been_there, 0);
        (world, units, site, vec![worker])
    }

    fn target(kind: OrdinaryGatherKind, ucell: i32) -> OrdinaryGatherTarget {
        OrdinaryGatherTarget {
            kind,
            centre: GatherNearbyPoint {
                x: Coord(coord(ucell)),
                y: Coord(coord(ucell)),
            },
            corner: GatherTile { tx: 10, ty: 11 },
            x_size: 4,
            y_size: 4,
            domain: DOMAIN_LAND,
            completed: true,
        }
    }

    fn ty() -> NearbyUnitType {
        NearbyUnitType {
            type_index: 0x32,
            domain: DOMAIN_LAND,
            big_radius: 1,
            block_radius: 1,
            unit_flags: 0x10,
        }
    }

    fn collision_payload(world: &World) -> Vec<Option<[u8; 96]>> {
        let mut result = Vec::new();
        for wy in 0..world.ys {
            for wx in 0..world.xs {
                result.push(world.wdata(wx, wy).block.as_ref().map(|block| block.ptr));
            }
        }
        result
    }

    #[test]
    fn attachment_precedes_search_and_move_plan_preserves_anchor_and_stamp() {
        let (mut world, units, mut site, mut workers) = setup(OrdinaryGatherKind::Camp);
        let mut state = attached_ordinary_order_state(OrdinaryGatherKind::Camp);
        let rows_before = units.rows.clone();
        let guys_before = units.guys.clone();
        let bitmap_before = collision_payload(&world);
        let mut ordered = OrderedHost::default();

        let outcome = camp_mine_building_approach(
            &mut world,
            &mut CollCheck::new(),
            &units,
            &mut ordered,
            &Geometry(Ok(false)),
            &mut site,
            &mut workers,
            7,
            &mut state,
            target(OrdinaryGatherKind::Camp, 24),
            ty(),
            0,
        )
        .unwrap();

        assert_eq!(outcome.attachment.status, AttachResult::Attached);
        assert_eq!(site.gather_down, 7);
        let CampMineDisposition::QueueMove(plan) = outcome.disposition else {
            panic!("expected queued approach");
        };
        assert_eq!(plan.add_move_order_tail, [1, 0, 0, 0, -1, -1]);
        assert_eq!(
            Some(plan.destination),
            ordered.calls.first().map(|call| call.point)
        );
        assert!(!workers[0].assignment.unwrap().been_there);
        assert_eq!(outcome.active_workers_after, 0);
        assert_eq!(units.rows, rows_before);
        assert_eq!(units.guys, guys_before);
        assert_eq!(collision_payload(&world), bitmap_before);
    }

    #[test]
    fn camp_mine_activation_occurs_only_on_authoritative_arrival_tick() {
        let (mut world, units, mut site, mut workers) = setup(OrdinaryGatherKind::Mine);
        let mut state = attached_ordinary_order_state(OrdinaryGatherKind::Mine);
        workers[0].group = 8;
        let mut ordered = OrderedHost::default();
        let outcome = camp_mine_building_approach(
            &mut world,
            &mut CollCheck::new(),
            &units,
            &mut ordered,
            &Geometry(Ok(true)),
            &mut site,
            &mut workers,
            7,
            &mut state,
            target(OrdinaryGatherKind::Mine, 24),
            ty(),
            0,
        )
        .unwrap();

        assert_eq!(outcome.disposition, CampMineDisposition::Arrived);
        assert!(outcome.leader_economy_dirty);
        assert_eq!(state.wait, -1);
        assert_eq!(state.been_there, 1);
        assert!(workers[0].assignment.unwrap().been_there);
        assert_eq!(outcome.active_workers_after, 1);
        assert_eq!(workers[0].group, NO_OBJECT);
        assert!(ordered.calls.is_empty());
    }

    #[test]
    fn active_non_flat_entry_latches_recharge_once_and_detaches_the_group() {
        let (mut world, units, mut site, mut workers) = setup(OrdinaryGatherKind::Camp);
        let mut state = attached_ordinary_order_state(OrdinaryGatherKind::Camp);
        state.been_there = 1;
        workers[0].assignment.as_mut().unwrap().been_there = true;
        workers[0].group = 3;
        let outcome = camp_mine_building_approach(
            &mut world,
            &mut CollCheck::new(),
            &units,
            &mut OrderedHost::default(),
            &Geometry(Ok(true)),
            &mut site,
            &mut workers,
            7,
            &mut state,
            target(OrdinaryGatherKind::Camp, 24),
            ty(),
            0,
        )
        .unwrap();
        assert!(outcome.latched_site_recharge);
        assert!(!outcome.leader_economy_dirty);
        assert_eq!(site.recharging, 1);
        assert_eq!(site.build_masks & 0x800, 0x800);
        assert_eq!(workers[0].group, NO_OBJECT);
        assert_eq!(outcome.active_workers_after, 1);
    }

    #[test]
    fn already_attached_full_site_remains_a_member() {
        let (_world, units, mut site, mut workers) = setup(OrdinaryGatherKind::Camp);
        site.set_authoritative_capacity(1);
        assert_eq!(
            ensure_ordinary_attachment(
                &units,
                &mut site,
                &mut workers,
                7,
                target(OrdinaryGatherKind::Camp, 24),
                ty(),
            )
            .unwrap()
            .status,
            AttachResult::Attached
        );
        assert_eq!(
            ensure_ordinary_attachment(
                &units,
                &mut site,
                &mut workers,
                7,
                target(OrdinaryGatherKind::Camp, 24),
                ty(),
            )
            .unwrap()
            .status,
            AttachResult::AlreadyAttached
        );
    }

    #[test]
    fn new_worker_at_capacity_retires_staged_gather_state() {
        let (mut world, units, mut site, mut workers) = setup(OrdinaryGatherKind::Camp);
        site.set_authoritative_capacity(0);
        let mut state = attached_ordinary_order_state(OrdinaryGatherKind::Camp);
        let outcome = camp_mine_building_approach(
            &mut world,
            &mut CollCheck::new(),
            &units,
            &mut OrderedHost::default(),
            &Geometry(Err("must not query geometry")),
            &mut site,
            &mut workers,
            7,
            &mut state,
            target(OrdinaryGatherKind::Camp, 24),
            ty(),
            0,
        )
        .unwrap();
        let CampMineDisposition::RetiredAtCapacity(retirement) = outcome.disposition else {
            panic!("capacity refusal must retire the staged Gather");
        };
        assert!(!retirement.detached);
        assert_eq!(workers[0].assignment, None);
        assert_eq!(site.gather_down, NO_OBJECT);
        assert_eq!(outcome.active_workers_after, 0);
    }

    #[test]
    fn missing_geometry_fails_after_retail_attachment_without_fabricated_motion() {
        let (mut world, units, mut site, mut workers) = setup(OrdinaryGatherKind::Camp);
        let mut state = attached_ordinary_order_state(OrdinaryGatherKind::Camp);
        let result = camp_mine_building_approach(
            &mut world,
            &mut CollCheck::new(),
            &units,
            &mut OrderedHost::default(),
            &Geometry(Err("Unit::is_at unavailable")),
            &mut site,
            &mut workers,
            7,
            &mut state,
            target(OrdinaryGatherKind::Camp, 24),
            ty(),
            0,
        );
        assert_eq!(
            result,
            Err(OrdinaryGatherLifecycleError::Geometry(
                "Unit::is_at unavailable"
            ))
        );
        assert_eq!(site.gather_down, 7, "retail attachment happens first");
        assert!(!workers[0].assignment.unwrap().been_there);
    }

    #[test]
    fn ordered_collision_is_mandatory_and_an_error_never_teleports() {
        let (mut world, units, mut site, mut workers) = setup(OrdinaryGatherKind::Mine);
        let mut state = attached_ordinary_order_state(OrdinaryGatherKind::Mine);
        let row_before = units.rows[0];
        let mut ordered = OrderedHost {
            fail: true,
            ..OrderedHost::default()
        };
        let result = camp_mine_building_approach(
            &mut world,
            &mut CollCheck::new(),
            &units,
            &mut ordered,
            &Geometry(Ok(false)),
            &mut site,
            &mut workers,
            7,
            &mut state,
            target(OrdinaryGatherKind::Mine, 24),
            ty(),
            0,
        );
        assert!(matches!(
            result,
            Err(OrdinaryGatherLifecycleError::Nearby(
                NearbySearchError::OrderedCollision("ordered collision unavailable")
            ))
        ));
        assert_eq!(units.rows[0], row_before);
        assert_eq!(site.gather_down, 7);
    }

    #[test]
    fn near_exhaustion_keeps_attachment_inactive_and_enters_tile_retry_phase() {
        let (mut world, units, mut site, mut workers) = setup(OrdinaryGatherKind::Camp);
        let mut state = attached_ordinary_order_state(OrdinaryGatherKind::Camp);
        let mut ordered = OrderedHost {
            blocked: true,
            ..OrderedHost::default()
        };
        let outcome = camp_mine_building_approach(
            &mut world,
            &mut CollCheck::new(),
            &units,
            &mut ordered,
            &Geometry(Ok(false)),
            &mut site,
            &mut workers,
            7,
            &mut state,
            target(OrdinaryGatherKind::Camp, 24),
            ty(),
            0,
        )
        .unwrap();
        assert_eq!(outcome.disposition, CampMineDisposition::NearSearchBlocked);
        assert!(outcome.primary_search.is_some());
        assert!(outcome.far_search.is_none());
        assert_eq!(state.wait, -1);
        assert_eq!(site.gather_down, 7);
        assert!(!workers[0].assignment.unwrap().been_there);
    }

    #[test]
    fn far_double_exhaustion_runs_the_gather_retirement_epilogue() {
        let (mut world, units, mut site, mut workers) = setup(OrdinaryGatherKind::Mine);
        let mut state = attached_ordinary_order_state(OrdinaryGatherKind::Mine);
        let mut ordered = OrderedHost {
            blocked: true,
            ..OrderedHost::default()
        };
        let outcome = camp_mine_building_approach(
            &mut world,
            &mut CollCheck::new(),
            &units,
            &mut ordered,
            &Geometry(Ok(false)),
            &mut site,
            &mut workers,
            7,
            &mut state,
            target(OrdinaryGatherKind::Mine, 50),
            ty(),
            0,
        )
        .unwrap();
        let CampMineDisposition::RetiredFarBlocked(retirement) = outcome.disposition else {
            panic!("far double exhaustion must retire Gather");
        };
        assert!(retirement.detached);
        assert!(outcome.primary_search.is_some());
        assert!(outcome.far_search.is_some());
        assert_eq!(site.gather_down, NO_OBJECT);
        assert_eq!(workers[0].assignment, None);
        assert_eq!(outcome.active_workers_after, 0);
    }

    #[test]
    fn farm_is_active_before_its_optional_queued_footprint_move() {
        let (_world, units, mut site, mut workers) = setup(OrdinaryGatherKind::Farm);
        let mut state = attached_ordinary_order_state(OrdinaryGatherKind::Farm);
        let mut rng = Random::new(123);
        // gate = value + worker_o*7 + owner; -49 makes the low byte zero.
        let outcome = farm_first_gather_tick::<_, (), ()>(
            &units,
            &mut site,
            &mut workers,
            7,
            &mut state,
            target(OrdinaryGatherKind::Farm, 24),
            ty(),
            1,
            -49,
            &mut rng,
        )
        .unwrap();
        let FarmFirstTickDisposition::Active {
            move_order,
            action,
            rng_draws,
        } = outcome.disposition
        else {
            panic!("Farm should activate");
        };
        assert_eq!(action, 0x23);
        assert!(move_order.is_some());
        assert_eq!(rng_draws, 2);
        assert!(workers[0].assignment.unwrap().been_there);
        assert_eq!(state.been_there, 1);
        assert_eq!(outcome.active_workers_after, 1);
    }

    #[test]
    fn farm_low_byte_miss_draws_no_rng_but_is_still_active() {
        let (_world, units, mut site, mut workers) = setup(OrdinaryGatherKind::Farm);
        let mut state = attached_ordinary_order_state(OrdinaryGatherKind::Farm);
        let mut rng = Random::new(123);
        let before = rng.state();
        let outcome = farm_first_gather_tick::<_, (), ()>(
            &units,
            &mut site,
            &mut workers,
            7,
            &mut state,
            target(OrdinaryGatherKind::Farm, 24),
            ty(),
            1,
            0,
            &mut rng,
        )
        .unwrap();
        assert_eq!(
            outcome.disposition,
            FarmFirstTickDisposition::Active {
                move_order: None,
                action: 0x23,
                rng_draws: 0,
            }
        );
        assert_eq!(rng.state(), before);
        assert_eq!(outcome.active_workers_after, 1);
    }

    #[test]
    fn seated_ordinary_worker_is_a_model_error_not_an_exit_request() {
        let (_world, mut units, mut site, mut workers) = setup(OrdinaryGatherKind::Camp);
        units.rows[0].on_map = false;
        let result = ensure_ordinary_attachment(
            &units,
            &mut site,
            &mut workers,
            7,
            target(OrdinaryGatherKind::Camp, 24),
            ty(),
        );
        assert_eq!(
            result,
            Err(OrdinaryGatherLifecycleError::InPlace(
                InPlaceGatherReleaseError::OrdinaryGathererWasContained(GatherInsideObject {
                    owner: 0,
                    object: 7,
                })
            ))
        );
        assert_eq!(site.gather_down, NO_OBJECT);
    }

    #[test]
    fn can_transport_matches_the_measured_three_field_gate() {
        assert!(!ordinary_can_transport(0, 0, 0));
        assert!(ordinary_can_transport(0x0080_0000, 0, 0));
        assert!(!ordinary_can_transport(0x0080_0000, 0x2000, 0));
        assert!(ordinary_can_transport(0, 0, 0x10));
    }
}
