//! Exact read-only prefix of frame-zero `Unit::find_merchant_spot`.
//!
//! `find_merchant_spot` (`0x00603ab0`) first calls `UnitData::calc_gather`
//! (`0x00609180`).  For the supported Type-62 Merchant, the static replay row proves an
//! upgrade level of zero, hence a gather radius of four.  The prefix below reproduces the
//! cached-`good_obj` probe and then the retail `circle_x/circle_y` scan against the canonical
//! terrain owner.  The first fact that owner cannot supply is the result of
//! `ObjectsData::find_good_at(WCoord,WCoord,who,0,0)` (`0x0065bec0`): `Sim` has no canonical
//! frame-zero base-Good object pool.  That exact call is emitted as a digest-bound child.
//!
//! If no terrain row can reach `find_good_at`, `calc_gather` returns zero, so
//! `find_merchant_spot` returns zero without entering its 49-candidate loop.  That locally
//! proven result is composed directly into the existing failed-unpack tail.  No native
//! snapshot/checksum word is manufactured and no state is mutated.

#![forbid(unsafe_code)]

use std::fmt;

use don_sim::systems::collision::{RING_COUNT, RING_X, RING_Y};
use don_sim::systems::combat::circle_table;
use don_sim::systems::map_terrain::{tflag, Coord, TCoord, World};
use don_sim::tick::Sim;

use crate::setup_2024_frame0_merchant_unpack::{
    validate_frame0_merchant_spot_request, Frame0MerchantError, Frame0MerchantSpotRequest,
    MerchantSpotOutcome, DUTCH_MERCHANT_TYPE, FAILED_UNPACK_TAIL_VA, FIND_MERCHANT_SPOT_VA,
};
use crate::setup_2024_frame379::REPLAY_FILE_SHA256;
use crate::setup_unit_member_authority::{
    CanonicalSetupMemberSource, CanonicalSetupUnitMemberReceipt,
};
use crate::world_owner_frontier::sha256;

pub const UNIT_CALC_GATHER_VA: u32 = 0x0060_9180;
pub const OBJECTS_FIND_GOOD_AT_WCOORD_VA: u32 = 0x0065_bec0;
pub const UNIT_GOOD_MERCHANT_SPOT_VA: u32 = 0x0060_68a0;
pub const UNIT_INVALID_LOC_VA: u32 = 0x0060_7c30;
pub const UNIT_DETECT_COLLISION_VA: u32 = 0x0061_7060;

/// Type-62 takes `4 * ObjectTypeData::upgrade_level() + 4`.
pub const GOLDEN_MERCHANT_UPGRADE_LEVEL: i32 = 0;
pub const GOLDEN_MERCHANT_GATHER_RADIUS: i32 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame0MerchantCalcGatherScan {
    /// `UnitData::good_obj` was nonnegative and inside the radius endpoint.
    CachedGoodObj,
    /// The subsequent ordered scan beginning at `circle_x/circle_y[0]`.
    OrderedCircle,
}

/// Exact first unsourced call reached by the locally evaluated `calc_gather` prefix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame0MerchantGoodLookupRequest {
    pub request_sha256: [u8; 32],
    pub parent_request_sha256: [u8; 32],
    pub setup_composition_digest: [u8; 32],
    pub setup_authority_revision: u64,
    pub invocation_ordinal: usize,
    pub actor_who: u8,
    pub actor_o: i16,
    pub actor_uid: u16,
    pub actor_type: i32,
    pub calc_gather_call_va: u32,
    pub calc_coord_x: i32,
    pub calc_coord_y: i32,
    pub upgrade_level: i32,
    pub gather_radius: i32,
    pub cached_good_obj: i16,
    pub scan: Frame0MerchantCalcGatherScan,
    pub circle_index: usize,
    pub tile_x: i32,
    pub tile_y: i32,
    pub tile_mask: u16,
    pub find_good_at_call_va: u32,
    pub world_x: i32,
    pub world_y: i32,
    pub lookup_owner: i32,
    pub lookup_arg4: i32,
    pub lookup_arg5: i32,
}

/// A locally proven `calc_gather == 0` and its exact caller continuation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame0MerchantSearchTailRequest {
    pub parent: Frame0MerchantSpotRequest,
    pub local_search_proof_sha256: [u8; 32],
    pub calc_gather_returned_zero: bool,
    pub find_merchant_spot_outcome: MerchantSpotOutcome,
    pub next_va: u32,
    pub reason: &'static str,
}

/// Maximal currently source-owned progress through the Merchant search.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame0MerchantSearchFrontier {
    NeedsGoodLookup(Frame0MerchantGoodLookupRequest),
    FailedUnpackTail(Frame0MerchantSearchTailRequest),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame0MerchantSearchError {
    Parent(Frame0MerchantError),
    MissingSetupAuthorityRevision,
    StaleSetupMember,
    MissingUpgradeGraph { from_type: i32 },
    InvalidGoodLookupDigest,
    StaleGoodLookup,
}

impl fmt::Display for Frame0MerchantSearchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "2024 frame-zero Merchant search refused: {self:?}")
    }
}

impl std::error::Error for Frame0MerchantSearchError {}

impl From<Frame0MerchantError> for Frame0MerchantSearchError {
    fn from(value: Frame0MerchantError) -> Self {
        Self::Parent(value)
    }
}

fn append_lookup(image: &mut Vec<u8>, request: &Frame0MerchantGoodLookupRequest) {
    image.extend_from_slice(&request.parent_request_sha256);
    image.extend_from_slice(&request.setup_composition_digest);
    image.extend_from_slice(&request.setup_authority_revision.to_le_bytes());
    image.extend_from_slice(&(request.invocation_ordinal as u64).to_le_bytes());
    image.push(request.actor_who);
    image.extend_from_slice(&request.actor_o.to_le_bytes());
    image.extend_from_slice(&request.actor_uid.to_le_bytes());
    image.extend_from_slice(&request.actor_type.to_le_bytes());
    for value in [
        request.calc_gather_call_va as i32,
        request.calc_coord_x,
        request.calc_coord_y,
        request.upgrade_level,
        request.gather_radius,
        i32::from(request.cached_good_obj),
        request.circle_index as i32,
        request.tile_x,
        request.tile_y,
        i32::from(request.tile_mask),
        request.find_good_at_call_va as i32,
        request.world_x,
        request.world_y,
        request.lookup_owner,
        request.lookup_arg4,
        request.lookup_arg5,
    ] {
        image.extend_from_slice(&value.to_le_bytes());
    }
    image.push(match request.scan {
        Frame0MerchantCalcGatherScan::CachedGoodObj => 0,
        Frame0MerchantCalcGatherScan::OrderedCircle => 1,
    });
}

pub fn frame0_merchant_good_lookup_digest(request: &Frame0MerchantGoodLookupRequest) -> [u8; 32] {
    let mut image = Vec::with_capacity(192);
    append_lookup(&mut image, request);
    sha256(&image)
}

pub fn validate_frame0_merchant_good_lookup_digest(
    request: &Frame0MerchantGoodLookupRequest,
) -> Result<(), Frame0MerchantSearchError> {
    if request.request_sha256 != frame0_merchant_good_lookup_digest(request) {
        return Err(Frame0MerchantSearchError::InvalidGoodLookupDigest);
    }
    Ok(())
}

fn validate_member(
    request: &Frame0MerchantSpotRequest,
    member: &CanonicalSetupUnitMemberReceipt,
) -> Result<(), Frame0MerchantSearchError> {
    if member.authority_revision == 0 {
        return Err(Frame0MerchantSearchError::MissingSetupAuthorityRevision);
    }
    let identity = &member.unit.identity;
    if member.authority_digest != request.setup_composition_digest
        || member.source
            != CanonicalSetupMemberSource::ReplayRulesCompleteInitReceiptAndCanonicalSim
        || member.replay_file_sha256 != REPLAY_FILE_SHA256
        || member.frame != 0
        || member.row != request.actor.row
        || identity.handle != request.actor.handle
        || identity.who != request.actor.who
        || identity.o != request.actor.o
        || identity.uid != request.actor.uid
        || member.current_type != DUTCH_MERCHANT_TYPE
        || member.type_facts.type_index != DUTCH_MERCHANT_TYPE
    {
        return Err(Frame0MerchantSearchError::StaleSetupMember);
    }
    if member.type_facts.from_type >= 0 {
        return Err(Frame0MerchantSearchError::MissingUpgradeGraph {
            from_type: member.type_facts.from_type,
        });
    }
    Ok(())
}

fn eligible_good_lookup(
    world: &World,
    request: &Frame0MerchantSpotRequest,
    setup_authority_revision: u64,
    cached_good_obj: i16,
    scan: Frame0MerchantCalcGatherScan,
    circle_index: usize,
    tile_x: i32,
    tile_y: i32,
) -> Option<Frame0MerchantGoodLookupRequest> {
    if !world.valid_t(tile_x, tile_y) {
        return None;
    }
    let tile_mask = world.tmask(tile_x, tile_y);
    // Type 62 takes UnitData::is_merchant's special arm: unlike an ordinary gatherer it
    // rejects exactly surface bits 0x20 and admits the other three 0x30 combinations.
    if tile_mask & 0x30 == 0x20 || tile_mask & tflag::RESOURCE == 0 {
        return None;
    }
    let mut child = Frame0MerchantGoodLookupRequest {
        request_sha256: [0; 32],
        parent_request_sha256: request.request_sha256,
        setup_composition_digest: request.setup_composition_digest,
        setup_authority_revision,
        invocation_ordinal: request.invocation_ordinal,
        actor_who: request.actor.who,
        actor_o: request.actor.o,
        actor_uid: request.actor.uid,
        actor_type: request.actor.type_index,
        calc_gather_call_va: UNIT_CALC_GATHER_VA,
        calc_coord_x: request.actor.x,
        calc_coord_y: request.actor.y,
        upgrade_level: GOLDEN_MERCHANT_UPGRADE_LEVEL,
        gather_radius: GOLDEN_MERCHANT_GATHER_RADIUS,
        cached_good_obj,
        scan,
        circle_index,
        tile_x,
        tile_y,
        tile_mask,
        find_good_at_call_va: OBJECTS_FIND_GOOD_AT_WCOORD_VA,
        world_x: tile_x >> 2,
        world_y: tile_y >> 2,
        lookup_owner: i32::from(request.actor.who),
        lookup_arg4: 0,
        lookup_arg5: 0,
    };
    child.request_sha256 = frame0_merchant_good_lookup_digest(&child);
    Some(child)
}

fn first_head_good_lookup(
    world: &World,
    request: &Frame0MerchantSpotRequest,
    setup_authority_revision: u64,
    cached_good_obj: i16,
) -> Option<Frame0MerchantGoodLookupRequest> {
    let table = circle_table();
    let endpoint = table.ring_end[GOLDEN_MERCHANT_GATHER_RADIUS as usize] as usize;
    let origin_x = TCoord::from_coord(Coord(request.actor.x)).0;
    let origin_y = TCoord::from_coord(Coord(request.actor.y)).0;

    if cached_good_obj >= 0 {
        let index = cached_good_obj as usize;
        if index < endpoint {
            let tile_x = origin_x.wrapping_add(i32::from(table.x[index]));
            let tile_y = origin_y.wrapping_add(i32::from(table.y[index]));
            if let Some(child) = eligible_good_lookup(
                world,
                request,
                setup_authority_revision,
                cached_good_obj,
                Frame0MerchantCalcGatherScan::CachedGoodObj,
                index,
                tile_x,
                tile_y,
            ) {
                return Some(child);
            }
        }
    }

    for index in 0..endpoint {
        let tile_x = origin_x.wrapping_add(i32::from(table.x[index]));
        let tile_y = origin_y.wrapping_add(i32::from(table.y[index]));
        if let Some(child) = eligible_good_lookup(
            world,
            request,
            setup_authority_revision,
            cached_good_obj,
            Frame0MerchantCalcGatherScan::OrderedCircle,
            index,
            tile_x,
            tile_y,
        ) {
            return Some(child);
        }
    }
    None
}

fn local_zero_proof(
    request: &Frame0MerchantSpotRequest,
    setup_authority_revision: u64,
    cached_good_obj: i16,
) -> [u8; 32] {
    let mut image = Vec::with_capacity(96);
    image.extend_from_slice(&request.request_sha256);
    image.extend_from_slice(&request.setup_composition_digest);
    image.extend_from_slice(&setup_authority_revision.to_le_bytes());
    image.extend_from_slice(&cached_good_obj.to_le_bytes());
    image.extend_from_slice(&GOLDEN_MERCHANT_UPGRADE_LEVEL.to_le_bytes());
    image.extend_from_slice(&GOLDEN_MERCHANT_GATHER_RADIUS.to_le_bytes());
    image.extend_from_slice(&UNIT_CALC_GATHER_VA.to_le_bytes());
    image.extend_from_slice(&FIND_MERCHANT_SPOT_VA.to_le_bytes());
    sha256(&image)
}

/// Evaluate the source-owned head of `find_merchant_spot` without mutating `sim`.
///
/// The setup member supplies the replay-carried `from_type` row. A nonnegative `from_type`
/// is deliberately rejected because `ObjectTypeData::upgrade_level` then needs the complete
/// live type-relation graph; the supported Type-62 row is terminal and yields level zero.
pub fn advance_frame0_merchant_search(
    sim: &Sim,
    request: &Frame0MerchantSpotRequest,
    member: &CanonicalSetupUnitMemberReceipt,
) -> Result<Frame0MerchantSearchFrontier, Frame0MerchantSearchError> {
    validate_frame0_merchant_spot_request(sim, request)?;
    validate_member(request, member)?;
    let cached_good_obj = sim
        .world
        .units
        .good_obj()
        .get(request.actor.row)
        .copied()
        .ok_or(Frame0MerchantSearchError::StaleSetupMember)?;
    if let Some(child) = first_head_good_lookup(
        &sim.map.world,
        request,
        member.authority_revision,
        cached_good_obj,
    ) {
        return Ok(Frame0MerchantSearchFrontier::NeedsGoodLookup(child));
    }

    Ok(Frame0MerchantSearchFrontier::FailedUnpackTail(
        Frame0MerchantSearchTailRequest {
            parent: request.clone(),
            local_search_proof_sha256: local_zero_proof(
                request,
                member.authority_revision,
                cached_good_obj,
            ),
            calc_gather_returned_zero: true,
            find_merchant_spot_outcome: MerchantSpotOutcome::NotFound,
            next_va: FAILED_UNPACK_TAIL_VA,
            reason: "calc_gather found no resource Good candidate; find_merchant_spot returned 0 and think_merchant continues with its Good-object scan",
        },
    ))
}

/// Repeat the complete canonical prefix and require the same first child.
pub fn validate_frame0_merchant_good_lookup_request(
    sim: &Sim,
    parent: &Frame0MerchantSpotRequest,
    member: &CanonicalSetupUnitMemberReceipt,
    child: &Frame0MerchantGoodLookupRequest,
) -> Result<(), Frame0MerchantSearchError> {
    validate_frame0_merchant_good_lookup_digest(child)?;
    match advance_frame0_merchant_search(sim, parent, member)? {
        Frame0MerchantSearchFrontier::NeedsGoodLookup(current) if current == *child => Ok(()),
        _ => Err(Frame0MerchantSearchError::StaleGoodLookup),
    }
}

/// The four source-exact terrain gates at the head of `good_merchant_spot`.
///
/// The remaining child is another fixed-shape `calc_gather` call at
/// `((tile_x - 1) * 192, (tile_y - 1) * 192)`.
pub fn good_merchant_spot_terrain_prefix(world: &World, tile_x: i32, tile_y: i32) -> bool {
    for (x, y) in [
        (tile_x, tile_y),
        (tile_x.wrapping_sub(1), tile_y),
        (tile_x, tile_y.wrapping_sub(1)),
        (tile_x.wrapping_sub(1), tile_y.wrapping_sub(1)),
    ] {
        if !world.valid_t(x, y) {
            return false;
        }
        let mask = world.tmask(x, y);
        if mask & 0x4000 != 0 || mask & 3 == 3 || mask & 0x80 != 0 {
            return false;
        }
    }
    true
}

/// Exact radius-table order for the outer `find_merchant_spot` candidate loop.
pub fn frame0_merchant_candidate_tiles(
    coord_x: i32,
    coord_y: i32,
    radius: i32,
) -> Option<Vec<(i32, i32)>> {
    let endpoint = *RING_COUNT.get(radius as usize)? as usize;
    let origin_x = TCoord::from_coord(Coord(coord_x)).0;
    let origin_y = TCoord::from_coord(Coord(coord_y)).0;
    Some(
        (0..endpoint)
            .map(|index| {
                (
                    origin_x.wrapping_add(RING_X[index]),
                    origin_y.wrapping_add(RING_Y[index]),
                )
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::setup_2024_frame0_merchant_unpack::{
        request_frame0_merchant_spot, MERCHANT_RADIUS, MERCHANT_THINK_MASK,
    };
    use don_sim::systems::movement::PathStack;
    use don_sim::world::{Handle, OBJ_FLAG_ACTIVE};

    fn setup() -> (Sim, Handle, Frame0MerchantSpotRequest) {
        let mut sim = Sim::new(0x1234_5678, 16);
        sim.activate(0);
        let _scout = sim.spawn_unit(0, 69, 1_000, 2_000, 4).unwrap();
        let actor = sim
            .spawn_unit(0, DUTCH_MERCHANT_TYPE, 2_000, 3_000, 4)
            .unwrap();
        let row = sim.world.row_of(actor).unwrap();
        sim.world.units.set_unit_masks(row, MERCHANT_THINK_MASK);
        sim.world.units.set_flags(row, OBJ_FLAG_ACTIVE);
        sim.paths[row] = PathStack::with_header(17, 9).unwrap();
        let request = request_frame0_merchant_spot(&sim, actor, [0x51; 32]).unwrap();
        (sim, actor, request)
    }

    #[test]
    fn head_calc_gather_stops_at_first_exact_good_lookup() {
        let (mut sim, _, request) = setup();
        let tx = TCoord::from_coord(Coord(request.actor.x)).0;
        let ty = TCoord::from_coord(Coord(request.actor.y)).0;
        *sim.map.world.tmask_mut(tx, ty) = tflag::RESOURCE;
        let child = first_head_good_lookup(&sim.map.world, &request, 7, -1).unwrap();
        assert_eq!(child.scan, Frame0MerchantCalcGatherScan::OrderedCircle);
        assert_eq!(child.circle_index, 0);
        assert_eq!((child.tile_x, child.tile_y), (tx, ty));
        assert_eq!((child.world_x, child.world_y), (tx >> 2, ty >> 2));
        assert_eq!(
            (child.lookup_owner, child.lookup_arg4, child.lookup_arg5),
            (0, 0, 0)
        );
        validate_frame0_merchant_good_lookup_digest(&child).unwrap();
    }

    #[test]
    fn cached_good_obj_probe_precedes_the_full_circle() {
        let (mut sim, _, request) = setup();
        let table = circle_table();
        let tx = TCoord::from_coord(Coord(request.actor.x)).0 + i32::from(table.x[8]);
        let ty = TCoord::from_coord(Coord(request.actor.y)).0 + i32::from(table.y[8]);
        *sim.map.world.tmask_mut(tx, ty) = tflag::RESOURCE;
        let child = first_head_good_lookup(&sim.map.world, &request, 7, 8).unwrap();
        assert_eq!(child.scan, Frame0MerchantCalcGatherScan::CachedGoodObj);
        assert_eq!(child.circle_index, 8);
    }

    #[test]
    fn no_resource_bit_proves_head_calc_gather_zero() {
        let (sim, _, request) = setup();
        assert_eq!(
            first_head_good_lookup(&sim.map.world, &request, 7, -1),
            None
        );
    }

    #[test]
    fn lookup_digest_catches_candidate_mutation() {
        let (mut sim, _, request) = setup();
        let tx = TCoord::from_coord(Coord(request.actor.x)).0;
        let ty = TCoord::from_coord(Coord(request.actor.y)).0;
        *sim.map.world.tmask_mut(tx, ty) = tflag::RESOURCE;
        let mut child = first_head_good_lookup(&sim.map.world, &request, 7, -1).unwrap();
        child.tile_x += 1;
        assert_eq!(
            validate_frame0_merchant_good_lookup_digest(&child),
            Err(Frame0MerchantSearchError::InvalidGoodLookupDigest)
        );
    }

    #[test]
    fn outer_radius_three_uses_all_49_shipped_offsets_in_order() {
        let candidates =
            frame0_merchant_candidate_tiles(12 * 192, 20 * 192, MERCHANT_RADIUS).unwrap();
        assert_eq!(candidates.len(), 49);
        assert_eq!(candidates[0], (12, 20));
        assert_eq!(candidates[1], (11, 19));
        assert_eq!(candidates[48], (9, 18));
    }

    #[test]
    fn good_spot_prefix_checks_all_four_corner_masks() {
        let (mut sim, _, _) = setup();
        for y in 19..=20 {
            for x in 11..=12 {
                *sim.map.world.tmask_mut(x, y) = 0;
            }
        }
        assert!(good_merchant_spot_terrain_prefix(&sim.map.world, 12, 20));
        *sim.map.world.tmask_mut(11, 19) = 0x80;
        assert!(!good_merchant_spot_terrain_prefix(&sim.map.world, 12, 20));
    }
}
