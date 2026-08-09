//! Retail nearby-placement and containment primitives around gathering workers.
//!
//! This is the collision/identity half of `Unit::come_out` (`0x00617C10`), not a second
//! movement system.  The placement loop is the ordinary land-unit arm of
//! `UnitType::find_nearby_spot` (`0x0061DE70`).  For gather filter 3 and a valid actor
//! identity, retail checks a candidate in this exact order:
//!
//! 1. bounds, optional region and `TData` land passability;
//! 2. `Objects::find_collision` (`0x0065B1B0`), whose land/zero-mode fast arm is
//!    `CollCheck::collide_here` (`0x00682540`);
//! 3. `Objects::find_ordered_collision` (`0x0065B440`).
//!
//! Both collision views are mandatory here.  The synchronized [`World`] owns the bitmap;
//! a caller-supplied [`OrderedCollision`] owns the second query, including ordered unit
//! coordinates and army membership.  An absent adapter is therefore a type error, not an
//! implicit "clear" answer.
//!
//! Ordinary Farm, Woodcutter/Camp and Mine Citizens do **not** enter containment in retail:
//! detaching them preserves their current position, world link and collision stamp. The
//! contained gather profile belongs to Scholar types `0x34`/`0x35`. This distinction is
//! enforced by [`ordinary_gather_release_in_place`] and [`plan_scholar_come_out`]; an Arena
//! model that marked an ordinary gatherer off-map cannot use this module to invent an exit.
//!
//! `Object::remove_from_inside` (`0x006480F0`) is exposed as a separately validated link
//! transaction.  A caller first plans placement and the link splice, then commits the
//! splice only after it is ready to perform the remaining retail world/add/location
//! effects.  A blocked search never mutates containment state.  This module deliberately
//! does not invent guy formation offsets: the final `Unit::set_new_location` phase must be
//! supplied by the unit-location owner.

use super::collision::{CollCheck, CollUnits, DOMAIN_LAND};
use super::gathering::{
    gather_nearby_probes, GatherFilterIndex, GatherInsideObject, GatherInsideReceipt,
    GatherNearbyPoint, GatherNearbySpotRequest,
};
use super::map_terrain::{tflag, UCoord, World};

/// Exact UnitType fields read by the supported land/gather arm of
/// `UnitType::find_nearby_spot`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NearbyUnitType {
    pub type_index: i32,
    /// `ObjectTypeData::domain` `+0x218`; this boundary supports the land arm only.
    pub domain: i32,
    /// `ObjectTypeData::big_radius` `+0x244`, used by automatic maximum-radius expansion.
    pub big_radius: i32,
    /// `ObjectTypeData::new_block_radius` `+0x248`, consumed by `find_collision`.
    pub block_radius: i32,
    /// `UnitTypeData::unit_flags` `+0x2B4`; bit `0x10` changes the zero-big-radius fallback.
    pub unit_flags: u32,
}

/// The exact second collision query after the bitmap is clear.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OrderedCollisionQuery {
    pub point: GatherNearbyPoint,
    pub actor: GatherInsideObject,
    pub actor_type: i32,
}

/// Mandatory host for `Objects::find_ordered_collision` (`0x0065B440`).
///
/// Implementations must inspect objects in retail WData intrusive-list order and include
/// the actor's ordered-coordinate/army-member tail.  `Ok(true)` means blocked.  There is no
/// default because an unavailable ordered object view cannot safely mean "clear".
pub trait OrderedCollision {
    type Error;

    fn find_ordered_collision(&mut self, query: OrderedCollisionQuery)
        -> Result<bool, Self::Error>;
}

/// Auditable counts from one deterministic nearby search.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NearbySearchTrace {
    pub probes: usize,
    pub bitmap_queries: usize,
    pub ordered_queries: usize,
}

/// Successful or exhausted nearby search, with the collision-call counts retained.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NearbySearchResult {
    pub point: Option<GatherNearbyPoint>,
    pub trace: NearbySearchTrace,
}

/// Fail-closed errors around the recovered finder arm.
#[derive(Debug, PartialEq, Eq)]
pub enum NearbySearchError<E> {
    OrderedCollision(E),
    WrongType {
        requested: i32,
        supplied: i32,
    },
    UnsupportedDomain(i32),
    UnsupportedFilter(i32),
    UnsupportedExpandedMode(i32),
    UnsupportedOverlapGate {
        actor_type: i32,
        overlap_o: i32,
    },
    InvalidActor(GatherInsideObject),
    MissingBitmapActor(GatherInsideObject),
    BitmapTypeMismatch {
        row_domain: i32,
        row_block_radius: i32,
        type_domain: i32,
        type_block_radius: i32,
    },
    NegativeRadius(i32),
}

/// Resolve the automatic radius fields at the head of `UnitType::find_nearby_spot`.
///
/// The supported gather arm always has `expanded == 0`, so the squad-size expansion is not
/// read.  The signed divide-by-eight is after `max >= min`; consequently C truncation and
/// integer division are identical for this valid-input domain.
fn resolve_radii(
    request: GatherNearbySpotRequest,
    ty: NearbyUnitType,
) -> Result<(i32, i32), NearbySearchError<std::convert::Infallible>> {
    if request.min_radius < 0 {
        return Err(NearbySearchError::NegativeRadius(request.min_radius));
    }
    let mut max_radius = request.max_radius;
    if (request.min_radius > 0 && max_radius == 0) || max_radius < 0 {
        max_radius = request
            .min_radius
            .wrapping_add(ty.big_radius.wrapping_mul(4));
        if ty.big_radius == 0 && ty.unit_flags & 0x10 == 0 {
            max_radius = request.min_radius.wrapping_add(0x240);
        }
    }
    max_radius = max_radius.max(request.min_radius);
    let mut step = request.radial_step;
    if step < 1 {
        step = max_radius.wrapping_sub(request.min_radius) / 8;
    }
    Ok((max_radius, step.max(1)))
}

/// The exact land/gather collision arm of `UnitType::find_nearby_spot` (`0x0061DE70`).
///
/// The concrete `world`/`collcheck`/`bitmap_units` inputs are the retail collision bitmap
/// side.  `ordered` is independently mandatory because a clear bitmap is not the final
/// answer.  The function consumes no RNG and does not mutate the actor or its inside chain.
///
/// Unsupported alternate arms (air/sea, expanded formations, filters other than 3/5) are
/// rejected rather than approximated with this land path.
pub fn find_gather_nearby_spot<U, O>(
    world: &mut World,
    collcheck: &mut CollCheck,
    bitmap_units: &U,
    ordered: &mut O,
    ty: NearbyUnitType,
    request: GatherNearbySpotRequest,
) -> Result<NearbySearchResult, NearbySearchError<O::Error>>
where
    U: CollUnits,
    O: OrderedCollision,
{
    if request.worker_type != ty.type_index {
        return Err(NearbySearchError::WrongType {
            requested: request.worker_type,
            supplied: ty.type_index,
        });
    }
    if ty.domain != DOMAIN_LAND {
        return Err(NearbySearchError::UnsupportedDomain(ty.domain));
    }
    if request.filter != GatherFilterIndex::GATHER && request.filter.0 != 5 {
        return Err(NearbySearchError::UnsupportedFilter(request.filter.0));
    }
    if request.expanded != 0 {
        return Err(NearbySearchError::UnsupportedExpandedMode(request.expanded));
    }
    if matches!(ty.type_index, 0x32 | 0x33) && request.overlap_o >= 0 {
        return Err(NearbySearchError::UnsupportedOverlapGate {
            actor_type: ty.type_index,
            overlap_o: request.overlap_o,
        });
    }
    let actor = GatherInsideObject {
        owner: request.worker_owner as i8,
        object: request.worker_o as i16,
    };
    if request.worker_owner < 0
        || request.worker_owner > i8::MAX as i32
        || request.worker_o < 0
        || request.worker_o > i16::MAX as i32
    {
        return Err(NearbySearchError::InvalidActor(actor));
    }
    let Some(actor_row) = bitmap_units.row(request.worker_owner, request.worker_o) else {
        return Err(NearbySearchError::MissingBitmapActor(actor));
    };
    if actor_row.domain != ty.domain || actor_row.block_radius != ty.block_radius {
        return Err(NearbySearchError::BitmapTypeMismatch {
            row_domain: actor_row.domain,
            row_block_radius: actor_row.block_radius,
            type_domain: ty.domain,
            type_block_radius: ty.block_radius,
        });
    }

    let (max_radius, step) = resolve_radii(request, ty).map_err(|error| match error {
        NearbySearchError::NegativeRadius(radius) => NearbySearchError::NegativeRadius(radius),
        _ => unreachable!("resolve_radii only validates radius"),
    })?;
    let mut trace = NearbySearchTrace::default();
    for probe in gather_nearby_probes(request, max_radius, step) {
        trace.probes += 1;
        let point = probe.point;
        if !world.valid_coord(point.x.0, point.y.0) {
            continue;
        }
        let ux = UCoord::from_coord(point.x).0;
        let uy = UCoord::from_coord(point.y).0;
        let tx = ux >> 2;
        let ty_coord = uy >> 2;
        if request.required_region >= 0
            && world.get_tregion(tx, ty_coord) != request.required_region
        {
            continue;
        }
        let terrain = world.tmask(tx, ty_coord);
        if terrain & tflag::BLOCKED != 0 || terrain & tflag::SURFACE_MASK == tflag::SURFACE_WATER {
            continue;
        }

        // Objects::find_collision(..., mode=0) takes the direct bitmap arm for land.
        trace.bitmap_queries += 1;
        if collcheck
            .collide_here(
                world,
                bitmap_units,
                request.worker_o,
                request.worker_owner,
                ux,
                uy,
                ty.block_radius,
                false,
            )
            .is_some()
        {
            continue;
        }

        trace.ordered_queries += 1;
        let blocked = ordered
            .find_ordered_collision(OrderedCollisionQuery {
                point,
                actor,
                actor_type: ty.type_index,
            })
            .map_err(NearbySearchError::OrderedCollision)?;
        if !blocked {
            return Ok(NearbySearchResult {
                point: Some(point),
                trace,
            });
        }
    }
    Ok(NearbySearchResult { point: None, trace })
}

/// An ordinary retail gatherer release has no placement side effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InPlaceGatherRelease {
    pub worker: GatherInsideObject,
    pub point: GatherNearbyPoint,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InPlaceGatherReleaseError {
    ScholarRequiresContainment(i32),
    MissingCollisionRow(GatherInsideObject),
    OrdinaryGathererWasContained(GatherInsideObject),
}

/// Prove the collision/placement half of an ordinary gather detach is a no-op.
///
/// `Unit::do_gather` (`0x005EF2A0`) and `Unit::do_non_flat_gather` (`0x005F0170`) leave
/// Farm, Woodcutter/Camp and Mine Citizens on-map. Their order/gather-chain detach is owned
/// by [`super::gathering`]; this function verifies the synchronized collision row remains
/// on-map and returns its unchanged anchor. It does not query for a nearby point, unlink the
/// world object, or repaint the bitmap.
pub fn ordinary_gather_release_in_place<U: CollUnits>(
    units: &U,
    worker: GatherInsideObject,
    worker_type: i32,
) -> Result<InPlaceGatherRelease, InPlaceGatherReleaseError> {
    if matches!(worker_type, 0x34 | 0x35) {
        return Err(InPlaceGatherReleaseError::ScholarRequiresContainment(
            worker_type,
        ));
    }
    let Some(row) = units.row(i32::from(worker.owner), i32::from(worker.object)) else {
        return Err(InPlaceGatherReleaseError::MissingCollisionRow(worker));
    };
    if !row.on_map {
        return Err(InPlaceGatherReleaseError::OrdinaryGathererWasContained(
            worker,
        ));
    }
    Ok(InPlaceGatherRelease {
        worker,
        point: GatherNearbyPoint {
            x: super::map_terrain::Coord(row.x),
            y: super::map_terrain::Coord(row.y),
        },
    })
}

/// The four bidirectional containment-link fields used by `Object::insert_inside` and
/// `Object::remove_from_inside`.
///
/// Retail clears only the two signed object indices when removing an object.  The owner
/// bytes remain stale and are preserved by [`apply_remove_from_inside`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InsideLinks {
    pub inside_up: i16,
    pub inside_up_who: i8,
    pub inside_down: i16,
    pub inside_down_who: i8,
}

impl Default for InsideLinks {
    fn default() -> Self {
        Self {
            inside_up: -1,
            inside_up_who: -1,
            inside_down: -1,
            inside_down_who: -1,
        }
    }
}

/// One object participating in the single retail inside chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InsideRow {
    pub id: GatherInsideObject,
    /// Result of the object-kind virtual gate traversed by `ObjectData::get_inside`.
    pub object_kind: bool,
    pub links: InsideLinks,
}

/// Small owner-local table for the exact containment link transaction.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InsideTable {
    pub rows: Vec<InsideRow>,
}

impl InsideTable {
    pub fn find(&self, id: GatherInsideObject) -> Option<usize> {
        self.rows.iter().position(|row| row.id == id)
    }

    pub fn row(&self, id: GatherInsideObject) -> Option<&InsideRow> {
        self.find(id).map(|index| &self.rows[index])
    }

    pub fn row_mut(&mut self, id: GatherInsideObject) -> Option<&mut InsideRow> {
        self.find(id).map(|index| &mut self.rows[index])
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InsidePlanError {
    MissingObject(GatherInsideObject),
    NotObjectKind(GatherInsideObject),
    NotInside(GatherInsideObject),
    BrokenParentBacklink {
        child: GatherInsideObject,
        parent: GatherInsideObject,
    },
    BrokenChildBacklink {
        parent: GatherInsideObject,
        child: GatherInsideObject,
    },
    Cycle(GatherInsideObject),
    StalePlan(GatherInsideObject),
}

/// Fully validated, rollback-free link splice for `Object::remove_from_inside`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RemoveInsidePlan {
    pub receipt: GatherInsideReceipt,
    pub nested_child: Option<GatherInsideObject>,
    expected_child: InsideLinks,
    expected_parent: InsideLinks,
    expected_nested_child: Option<InsideLinks>,
}

fn linked_id(object: i16, owner: i8) -> Option<GatherInsideObject> {
    (object >= 0).then_some(GatherInsideObject { owner, object })
}

/// Validate the complete direct splice and recover the outermost parent identity without
/// mutating anything.
pub fn plan_remove_from_inside(
    table: &InsideTable,
    child: GatherInsideObject,
) -> Result<RemoveInsidePlan, InsidePlanError> {
    let child_row = table
        .row(child)
        .ok_or(InsidePlanError::MissingObject(child))?;
    if !child_row.object_kind {
        return Err(InsidePlanError::NotObjectKind(child));
    }
    let parent = linked_id(child_row.links.inside_up, child_row.links.inside_up_who)
        .ok_or(InsidePlanError::NotInside(child))?;
    let parent_row = table
        .row(parent)
        .ok_or(InsidePlanError::MissingObject(parent))?;
    if !parent_row.object_kind {
        return Err(InsidePlanError::NotObjectKind(parent));
    }
    if linked_id(
        parent_row.links.inside_down,
        parent_row.links.inside_down_who,
    ) != Some(child)
    {
        return Err(InsidePlanError::BrokenParentBacklink { child, parent });
    }

    let nested_child = linked_id(child_row.links.inside_down, child_row.links.inside_down_who);
    let expected_nested_child = if let Some(nested) = nested_child {
        let nested_row = table
            .row(nested)
            .ok_or(InsidePlanError::MissingObject(nested))?;
        if !nested_row.object_kind {
            return Err(InsidePlanError::NotObjectKind(nested));
        }
        if linked_id(nested_row.links.inside_up, nested_row.links.inside_up_who) != Some(child) {
            return Err(InsidePlanError::BrokenChildBacklink {
                parent: child,
                child: nested,
            });
        }
        Some(nested_row.links)
    } else {
        None
    };

    // ObjectData::get_inside follows inside_up to the outermost object.  Retail assumes an
    // acyclic chain.  We bound it by the live table and fail closed on corruption.
    let mut deepest_parent = parent;
    let mut cursor = parent;
    for _ in 0..=table.rows.len() {
        let row = table
            .row(cursor)
            .ok_or(InsidePlanError::MissingObject(cursor))?;
        if !row.object_kind {
            return Err(InsidePlanError::NotObjectKind(cursor));
        }
        let Some(next) = linked_id(row.links.inside_up, row.links.inside_up_who) else {
            return Ok(RemoveInsidePlan {
                receipt: GatherInsideReceipt {
                    child,
                    container: parent,
                    deepest_parent,
                },
                nested_child,
                expected_child: child_row.links,
                expected_parent: parent_row.links,
                expected_nested_child,
            });
        };
        if next == child {
            return Err(InsidePlanError::Cycle(child));
        }
        deepest_parent = next;
        cursor = next;
    }
    Err(InsidePlanError::Cycle(cursor))
}

/// Commit the exact four-link splice from `Object::remove_from_inside`.
///
/// Every expected row is rechecked before the first write.  On success the direct parent
/// adopts the removed object's nested child, that child points up to the direct parent, and
/// the removed object's two object-index links become `-1`.  Its two owner bytes are not
/// cleared, matching `0x006481D8..0x00648207`.
pub fn apply_remove_from_inside(
    table: &mut InsideTable,
    plan: RemoveInsidePlan,
) -> Result<GatherInsideReceipt, InsidePlanError> {
    let child = plan.receipt.child;
    let parent = plan.receipt.container;
    if table.row(child).map(|row| row.links) != Some(plan.expected_child) {
        return Err(InsidePlanError::StalePlan(child));
    }
    if table.row(parent).map(|row| row.links) != Some(plan.expected_parent) {
        return Err(InsidePlanError::StalePlan(parent));
    }
    if let Some(nested) = plan.nested_child {
        if table.row(nested).map(|row| row.links) != plan.expected_nested_child {
            return Err(InsidePlanError::StalePlan(nested));
        }
    }

    {
        let parent_row = table
            .row_mut(parent)
            .expect("validated remove plan parent must exist");
        parent_row.links.inside_down = plan.expected_child.inside_down;
        parent_row.links.inside_down_who = plan.expected_child.inside_down_who;
    }
    if let Some(nested) = plan.nested_child {
        let nested_row = table
            .row_mut(nested)
            .expect("validated remove plan nested child must exist");
        nested_row.links.inside_up = parent.object;
        nested_row.links.inside_up_who = parent.owner;
    }
    {
        let child_row = table
            .row_mut(child)
            .expect("validated remove plan child must exist");
        child_row.links.inside_up = -1;
        child_row.links.inside_down = -1;
    }
    Ok(plan.receipt)
}

/// A complete preflight for the collision/containment half of Scholar `come_out(0)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScholarComeOutPlan {
    pub point: GatherNearbyPoint,
    pub search: NearbySearchTrace,
    pub containment: RemoveInsidePlan,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScholarComeOutPlanResult {
    Ready(ScholarComeOutPlan),
    Blocked(NearbySearchTrace),
}

#[derive(Debug, PartialEq, Eq)]
pub enum ScholarComeOutPlanError<E> {
    NotScholarType(i32),
    UnsupportedMode(i32),
    Search(NearbySearchError<E>),
    Inside(InsidePlanError),
}

/// Value inputs for one Scholar `come_out(0)` preflight.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScholarComeOutRequest {
    pub child: GatherInsideObject,
    pub mode: i32,
    pub unit_type: NearbyUnitType,
    pub nearby: GatherNearbySpotRequest,
}

/// Plan Scholar `come_out(0)` without offsets and without partial state.
///
/// This performs the authoritative search first.  Only a found, bitmap-clear,
/// ordered-collision-clear point is paired with a validated containment splice.  Applying
/// the returned link plan and the remaining unit-location transaction is intentionally a
/// separate explicit step.
pub fn plan_scholar_come_out<U, O>(
    world: &mut World,
    collcheck: &mut CollCheck,
    bitmap_units: &U,
    ordered: &mut O,
    inside: &InsideTable,
    request: ScholarComeOutRequest,
) -> Result<ScholarComeOutPlanResult, ScholarComeOutPlanError<O::Error>>
where
    U: CollUnits,
    O: OrderedCollision,
{
    if request.mode != 0 {
        return Err(ScholarComeOutPlanError::UnsupportedMode(request.mode));
    }
    if !matches!(request.unit_type.type_index, 0x34 | 0x35) {
        return Err(ScholarComeOutPlanError::NotScholarType(
            request.unit_type.type_index,
        ));
    }
    if request.nearby.worker_owner != i32::from(request.child.owner)
        || request.nearby.worker_o != i32::from(request.child.object)
    {
        return Err(ScholarComeOutPlanError::Search(
            NearbySearchError::InvalidActor(request.child),
        ));
    }
    let search = find_gather_nearby_spot(
        world,
        collcheck,
        bitmap_units,
        ordered,
        request.unit_type,
        request.nearby,
    )
    .map_err(ScholarComeOutPlanError::Search)?;
    let Some(point) = search.point else {
        return Ok(ScholarComeOutPlanResult::Blocked(search.trace));
    };
    let containment =
        plan_remove_from_inside(inside, request.child).map_err(ScholarComeOutPlanError::Inside)?;
    Ok(ScholarComeOutPlanResult::Ready(ScholarComeOutPlan {
        point,
        search: search.trace,
        containment,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systems::collision::{place, CollGuy, UnitRow, UnitTable};
    use crate::systems::map_terrain::{Coord, UCoord};

    #[derive(Default)]
    struct OrderedHost {
        answers: Vec<bool>,
        calls: Vec<OrderedCollisionQuery>,
    }

    impl OrderedCollision for OrderedHost {
        type Error = &'static str;

        fn find_ordered_collision(
            &mut self,
            query: OrderedCollisionQuery,
        ) -> Result<bool, Self::Error> {
            self.calls.push(query);
            Ok(if self.answers.is_empty() {
                false
            } else {
                self.answers.remove(0)
            })
        }
    }

    fn coord(ucell: i32) -> i32 {
        UCoord(ucell).centre().0
    }

    fn world() -> World {
        World::init(8, 8, 44, 4, 4)
    }

    fn actor_row(on_map: bool) -> UnitRow {
        UnitRow {
            who: 0,
            o: 7,
            x: coord(4),
            y: coord(4),
            down: -1,
            down_who: -1,
            domain: DOMAIN_LAND,
            block_radius: 1,
            on_map,
            active: true,
            ..UnitRow::default()
        }
    }

    fn ty() -> NearbyUnitType {
        NearbyUnitType {
            type_index: 50,
            domain: DOMAIN_LAND,
            big_radius: 0,
            block_radius: 1,
            unit_flags: 0x10,
        }
    }

    fn request_at(x: i32, y: i32) -> GatherNearbySpotRequest {
        GatherNearbySpotRequest {
            centre: GatherNearbyPoint {
                x: Coord(x),
                y: Coord(y),
            },
            min_radius: 0,
            max_radius: 0,
            radial_step: 0,
            base_angle: 0,
            filter: GatherFilterIndex::GATHER,
            worker_type: 50,
            worker_o: 7,
            worker_owner: 0,
            accept_without_collision: 0,
            expanded: 0,
            overlap_o: -1,
            overlap_owner: 0,
            required_region: -1,
        }
    }

    fn scholar_ty() -> NearbyUnitType {
        NearbyUnitType {
            type_index: 0x34,
            ..ty()
        }
    }

    fn scholar_request_at(x: i32, y: i32) -> GatherNearbySpotRequest {
        GatherNearbySpotRequest {
            worker_type: 0x34,
            ..request_at(x, y)
        }
    }

    #[test]
    fn finder_requires_bitmap_then_ordered_clear_in_retail_order() {
        let mut world = world();
        let mut units = UnitTable::default();
        units.rows.push(actor_row(false));
        let mut ordered = OrderedHost::default();
        let mut check = CollCheck::new();
        let request = request_at(coord(12), coord(13));

        let found =
            find_gather_nearby_spot(&mut world, &mut check, &units, &mut ordered, ty(), request)
                .unwrap();

        assert_eq!(found.point, Some(request.centre));
        assert_eq!(found.trace.probes, 1);
        assert_eq!(found.trace.bitmap_queries, 1);
        assert_eq!(found.trace.ordered_queries, 1);
        assert_eq!(check.queries, 1);
        assert_eq!(ordered.calls.len(), 1);
        assert_eq!(ordered.calls[0].point, request.centre);
    }

    #[test]
    fn occupied_bitmap_short_circuits_ordered_query() {
        let mut world = world();
        let mut units = UnitTable::default();
        units.rows.push(actor_row(false));
        place(
            &mut world,
            &mut units,
            UnitRow {
                who: 1,
                o: 9,
                x: coord(12),
                y: coord(13),
                down: -1,
                down_who: -1,
                domain: DOMAIN_LAND,
                block_radius: 1,
                on_map: true,
                active: true,
                ..UnitRow::default()
            },
            [CollGuy {
                x: coord(12),
                y: coord(13),
                block_radius: 1,
            }],
        );
        let mut ordered = OrderedHost::default();
        let mut check = CollCheck::new();

        let result = find_gather_nearby_spot(
            &mut world,
            &mut check,
            &units,
            &mut ordered,
            ty(),
            request_at(coord(12), coord(13)),
        )
        .unwrap();

        assert_eq!(result.point, None);
        assert_eq!(result.trace.bitmap_queries, 1);
        assert_eq!(result.trace.ordered_queries, 0);
        assert!(ordered.calls.is_empty());
    }

    #[test]
    fn ordered_blocker_exhausts_single_probe_without_mutation() {
        let mut world = world();
        let mut units = UnitTable::default();
        units.rows.push(actor_row(false));
        let mut ordered = OrderedHost {
            answers: vec![true],
            calls: Vec::new(),
        };
        let result = find_gather_nearby_spot(
            &mut world,
            &mut CollCheck::new(),
            &units,
            &mut ordered,
            ty(),
            request_at(coord(12), coord(13)),
        )
        .unwrap();
        assert_eq!(result.point, None);
        assert_eq!(result.trace.ordered_queries, 1);
    }

    #[test]
    fn automatic_radius_and_step_match_retail_inclusive_progression() {
        let mut world = world();
        let mut units = UnitTable::default();
        units.rows.push(actor_row(false));
        let mut ordered = OrderedHost {
            // Block every point so the full stream is counted.
            answers: vec![true; 9 * 31],
            calls: Vec::new(),
        };
        let mut request = request_at(coord(24), coord(24));
        request.min_radius = 0x30;
        request.max_radius = -1;
        request.radial_step = 0;
        let mut nearby_type = ty();
        nearby_type.big_radius = 2;

        // max = 0x30 + 2*4 = 0x38 and step = max(1, 8/8) = 1: nine
        // inclusive non-zero radii, each with the 31-phase stream.
        let result = find_gather_nearby_spot(
            &mut world,
            &mut CollCheck::new(),
            &units,
            &mut ordered,
            nearby_type,
            request,
        )
        .unwrap();
        assert_eq!(result.point, None);
        assert_eq!(result.trace.probes, 9 * 31);
        assert_eq!(result.trace.bitmap_queries, 9 * 31);
        assert_eq!(result.trace.ordered_queries, 9 * 31);
    }

    #[test]
    fn terrain_and_region_rejection_precede_both_collision_views() {
        let mut world = world();
        let mut units = UnitTable::default();
        units.rows.push(actor_row(false));
        let point = (coord(12), coord(13));
        let tx = UCoord::from_coord(Coord(point.0)).0 >> 2;
        let ty_coord = UCoord::from_coord(Coord(point.1)).0 >> 2;
        *world.tmask_mut(tx, ty_coord) = tflag::SURFACE_WATER;
        let mut ordered = OrderedHost::default();
        let mut check = CollCheck::new();

        let result = find_gather_nearby_spot(
            &mut world,
            &mut check,
            &units,
            &mut ordered,
            ty(),
            request_at(point.0, point.1),
        )
        .unwrap();
        assert_eq!(result.point, None);
        assert_eq!(result.trace.bitmap_queries, 0);
        assert_eq!(result.trace.ordered_queries, 0);
        assert_eq!(check.queries, 0);
        assert!(ordered.calls.is_empty());

        *world.tmask_mut(tx, ty_coord) = 0;
        let mut request = request_at(point.0, point.1);
        request.required_region = world.get_tregion(tx, ty_coord).wrapping_add(1);
        let result =
            find_gather_nearby_spot(&mut world, &mut check, &units, &mut ordered, ty(), request)
                .unwrap();
        assert_eq!(result.point, None);
        assert_eq!(result.trace.bitmap_queries, 0);
        assert_eq!(check.queries, 0);
    }

    #[test]
    fn missing_bitmap_actor_and_mismatched_spatial_type_fail_closed() {
        let mut world = world();
        let units = UnitTable::default();
        let mut ordered = OrderedHost::default();
        let error = find_gather_nearby_spot(
            &mut world,
            &mut CollCheck::new(),
            &units,
            &mut ordered,
            ty(),
            request_at(coord(12), coord(13)),
        )
        .unwrap_err();
        assert_eq!(
            error,
            NearbySearchError::MissingBitmapActor(GatherInsideObject {
                owner: 0,
                object: 7
            })
        );

        let mut units = UnitTable::default();
        units.rows.push(UnitRow {
            block_radius: 2,
            ..actor_row(false)
        });
        assert!(matches!(
            find_gather_nearby_spot(
                &mut world,
                &mut CollCheck::new(),
                &units,
                &mut ordered,
                ty(),
                request_at(coord(12), coord(13)),
            ),
            Err(NearbySearchError::BitmapTypeMismatch { .. })
        ));
    }

    #[test]
    fn ordinary_gather_release_preserves_the_on_map_anchor_and_rejects_seating() {
        let worker = GatherInsideObject {
            owner: 0,
            object: 7,
        };
        let mut units = UnitTable::default();
        units.rows.push(actor_row(true));
        let before = units.rows[0];

        let release = ordinary_gather_release_in_place(&units, worker, 50).unwrap();
        assert_eq!(
            release.point,
            GatherNearbyPoint {
                x: Coord(before.x),
                y: Coord(before.y),
            }
        );
        assert_eq!(
            units.rows[0], before,
            "release must not repaint or relocate"
        );

        units.rows[0].on_map = false;
        assert_eq!(
            ordinary_gather_release_in_place(&units, worker, 50),
            Err(InPlaceGatherReleaseError::OrdinaryGathererWasContained(
                worker
            ))
        );
        assert_eq!(
            ordinary_gather_release_in_place(&units, worker, 0x34),
            Err(InPlaceGatherReleaseError::ScholarRequiresContainment(0x34))
        );
    }

    fn id(owner: i8, object: i16) -> GatherInsideObject {
        GatherInsideObject { owner, object }
    }

    fn row(
        id: GatherInsideObject,
        up: Option<GatherInsideObject>,
        down: Option<GatherInsideObject>,
    ) -> InsideRow {
        InsideRow {
            id,
            object_kind: true,
            links: InsideLinks {
                inside_up: up.map_or(-1, |value| value.object),
                inside_up_who: up.map_or(6, |value| value.owner),
                inside_down: down.map_or(-1, |value| value.object),
                inside_down_who: down.map_or(5, |value| value.owner),
            },
        }
    }

    #[test]
    fn remove_inside_splices_nested_child_and_preserves_stale_owner_bytes() {
        let outer = id(2, 20);
        let parent = id(1, 10);
        let child = id(0, 7);
        let nested = id(3, 30);
        let mut table = InsideTable {
            rows: vec![
                row(outer, None, Some(parent)),
                row(parent, Some(outer), Some(child)),
                row(child, Some(parent), Some(nested)),
                row(nested, Some(child), None),
            ],
        };
        let stale_up_who = table.row(child).unwrap().links.inside_up_who;
        let stale_down_who = table.row(child).unwrap().links.inside_down_who;

        let plan = plan_remove_from_inside(&table, child).unwrap();
        assert_eq!(plan.receipt.container, parent);
        assert_eq!(plan.receipt.deepest_parent, outer);
        let receipt = apply_remove_from_inside(&mut table, plan).unwrap();

        assert_eq!(receipt.child, child);
        assert_eq!(
            linked_id(
                table.row(parent).unwrap().links.inside_down,
                table.row(parent).unwrap().links.inside_down_who,
            ),
            Some(nested)
        );
        assert_eq!(
            linked_id(
                table.row(nested).unwrap().links.inside_up,
                table.row(nested).unwrap().links.inside_up_who,
            ),
            Some(parent)
        );
        let child_links = table.row(child).unwrap().links;
        assert_eq!(child_links.inside_up, -1);
        assert_eq!(child_links.inside_down, -1);
        assert_eq!(child_links.inside_up_who, stale_up_who);
        assert_eq!(child_links.inside_down_who, stale_down_who);
    }

    #[test]
    fn stale_or_broken_inside_plan_never_partially_writes() {
        let parent = id(1, 10);
        let child = id(0, 7);
        let mut table = InsideTable {
            rows: vec![
                row(parent, None, Some(child)),
                row(child, Some(parent), None),
            ],
        };
        let plan = plan_remove_from_inside(&table, child).unwrap();
        table.row_mut(parent).unwrap().links.inside_down = -1;
        let before = table.clone();
        assert_eq!(
            apply_remove_from_inside(&mut table, plan),
            Err(InsidePlanError::StalePlan(parent))
        );
        assert_eq!(table, before);

        let broken = InsideTable {
            rows: vec![row(parent, None, None), row(child, Some(parent), None)],
        };
        assert_eq!(
            plan_remove_from_inside(&broken, child),
            Err(InsidePlanError::BrokenParentBacklink { child, parent })
        );
    }

    #[test]
    fn blocked_scholar_come_out_does_not_plan_or_apply_containment() {
        let parent = id(1, 10);
        let child = id(0, 7);
        let table = InsideTable {
            rows: vec![
                row(parent, None, Some(child)),
                row(child, Some(parent), None),
            ],
        };
        let original = table.clone();
        let mut world = world();
        let mut units = UnitTable::default();
        units.rows.push(actor_row(false));
        let mut ordered = OrderedHost {
            answers: vec![true],
            calls: Vec::new(),
        };
        let result = plan_scholar_come_out(
            &mut world,
            &mut CollCheck::new(),
            &units,
            &mut ordered,
            &table,
            ScholarComeOutRequest {
                child,
                mode: 0,
                unit_type: scholar_ty(),
                nearby: scholar_request_at(coord(12), coord(13)),
            },
        )
        .unwrap();
        assert!(matches!(result, ScholarComeOutPlanResult::Blocked(_)));
        assert_eq!(table, original);
    }
}
