//! Exact bounded frame-zero Dutch-Merchant `think_merchant -> unpack_merchant` transaction.
//!
//! Retail reaches `Unit::think_merchant` at `0x005f4740` from the type-62 arm of
//! `Unit::think`.  With `unit_masks & 0x0008_0000 != 0`, its first child is
//! `Unit::unpack_merchant(3)` at `0x006038e0`.  The first input not owned by the replay is
//! `Unit::find_merchant_spot` at `0x00603ab0`: it reads terrain, gather, invalid-location,
//! and ordered Unit-collision state.  This module stops there and emits a typed request.
//!
//! A revisioned retail receipt can answer that request.  A false result is mutation-free but
//! continues into `think_merchant`'s unported Good-object scan, so it produces a typed tail
//! request rather than pretending that the frame is complete.  A true result admits the exact
//! no-search-scratch suffix as one atomic World/order/path commit.  It never fabricates the
//! golden post-frame-zero snapshot or consumes a recorded checksum word.

#![forbid(unsafe_code)]

use std::fmt;

use don_sim::order::{MoveOrderState, Order, OrderIndex, OrderList};
use don_sim::systems::economy_order_payload_authority::{CastOrderPayload, EconomyOrderPayload};
use don_sim::systems::map_terrain::WorldChecksum;
use don_sim::systems::movement::PathStack;
use don_sim::systems::unit_inctime::SUPPORTED_RETAIL_EXE_SHA256;
use don_sim::tick::Sim;
use don_sim::trig::find_angle;
use don_sim::world::{Handle, OBJ_FLAG_ACTIVE};

use crate::setup_2024_frame379::{OWNER, REPLAY_FILE_SHA256};
use crate::world_owner_frontier::sha256;

pub const THINK_MERCHANT_VA: u32 = 0x005f_4740;
pub const UNPACK_MERCHANT_VA: u32 = 0x0060_38e0;
pub const FIND_MERCHANT_SPOT_VA: u32 = 0x0060_3ab0;
pub const FAILED_UNPACK_TAIL_VA: u32 = 0x005f_476e;
pub const UNIT_THINK_SUCCESS_TAIL_VA: u32 = 0x005f_761a;

pub const MERCHANT_RADIUS: i32 = 3;
pub const DUTCH_MERCHANT_TYPE: i32 = 62;
pub const DUTCH_MERCHANT_OS: [i32; 2] = [1, 2];
pub const MERCHANT_THINK_MASK: u32 = 0x0008_0000;
pub const UNPACK_CLEAR_MASK: u32 = 0x0000_0100;
pub const QUEUE_NEW_CLEAR_MASK: u32 = 0x0400_0000;
pub const PACK_DEPLOY_BASE: i32 = 0x028c;
pub const DUTCH_DEPLOY_SPELL: i32 = 0x0290;
pub const FAST_DEPLOY_SPELL: i32 = 0x028e;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame0MerchantSpotSource {
    /// Supported retail entered the named child synchronously from `unpack_merchant(3)`.
    CompleteRetailFindMerchantSpotAtUnpackMerchant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MerchantSpotOutcome {
    NotFound,
    Found { tile_x: i32, tile_y: i32 },
}

/// Exact canonical actor image at the `think_merchant` entry used by this bounded adapter.
///
/// The queue and path are required to be empty for the current golden cone.  Their complete
/// allocation headers remain here because `Stack<PathData>::clear` preserves them and stale
/// setup state must not be normalized silently.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame0MerchantActorImage {
    pub handle: Handle,
    pub row: usize,
    pub who: u8,
    pub o: i16,
    pub uid: u16,
    pub type_index: i32,
    pub flags: u8,
    pub x: i32,
    pub y: i32,
    pub angle: i32,
    pub unit_masks: u32,
    pub unit_masks2: u32,
    pub orders_x: i32,
    pub orders_y: i32,
    pub dest_angle: i32,
    pub orders: OrderList,
    pub path: PathStack,
}

/// Typed request at the first unsourced child.
///
/// The two local digests are computed from the current canonical owners.  They are request
/// staleness stamps, not claims that the incomplete port already equals retail.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame0MerchantSpotRequest {
    pub request_sha256: [u8; 32],
    pub replay_file_sha256: [u8; 32],
    pub setup_composition_digest: [u8; 32],
    pub object_world_digest: u64,
    pub terrain_world_checksum: WorldChecksum,
    pub invocation_ordinal: usize,
    pub actor: Frame0MerchantActorImage,
    pub radius: i32,
    pub random_state: i32,
}

/// Retail answer for one request.
///
/// `query_input_sha256` is produced by the native capture over the complete terrain/gather/
/// location/collision input queried by `find_merchant_spot`.  Don cannot recompute it until
/// those World services have one exact owner.  `partial_path_104_was_null` separately attests
/// the runtime-only search pointer which controls `clear_partial_path`'s recycler mutations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame0MerchantSpotCapture {
    pub revision: u64,
    pub composition_digest: [u8; 32],
    pub source: Frame0MerchantSpotSource,
    pub replay_file_sha256: [u8; 32],
    pub executable_sha256: [u8; 32],
    pub request_sha256: [u8; 32],
    pub actor_o: i32,
    /// Exact native canonical-Sim save image at this call boundary.  It comes only from the
    /// retail capture; the port does not guess or reconstruct it.
    pub pre_call_retail_sim_sha256: [u8; 32],
    pub query_input_sha256: [u8; 32],
    pub outcome: MerchantSpotOutcome,
    /// Reached only after a successful spot result.  Type 62 then selects 0x290 when false
    /// and 0x28e when true; its special-object match precedes the 0x13d query.
    pub has_type_attribute_0x7b: Option<bool>,
    pub partial_path_104_was_null: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame0MerchantSuccessImage {
    pub flags: u8,
    pub unit_masks: u32,
    pub unit_masks2: u32,
    pub orders_x: i32,
    pub orders_y: i32,
    pub dest_angle: i32,
    pub orders: OrderList,
    pub path: PathStack,
    pub random_state: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedFrame0MerchantSuccess {
    pub request: Frame0MerchantSpotRequest,
    pub capture: Frame0MerchantSpotCapture,
    pub after: Frame0MerchantSuccessImage,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame0MerchantTailRequest {
    pub request: Frame0MerchantSpotRequest,
    pub capture: Frame0MerchantSpotCapture,
    pub next_va: u32,
    pub reason: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame0MerchantResolution {
    Success(PreparedFrame0MerchantSuccess),
    FailedUnpackTail(Frame0MerchantTailRequest),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame0MerchantCommitReceipt {
    pub request_sha256: [u8; 32],
    pub capture_digest: [u8; 32],
    pub actor: Handle,
    pub before: Frame0MerchantActorImage,
    pub after: Frame0MerchantSuccessImage,
    pub think_merchant_returned_one: bool,
    pub unit_think_success_tail_va: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame0MerchantError {
    MissingSetupCompositionDigest,
    WrongFrame { expected: i32, actual: i32 },
    StaleActor,
    WrongOwner { expected: u8, actual: u8 },
    WrongObjectIndex(i32),
    WrongType { world: i32, projection: i32 },
    InactiveActor,
    ThinkMerchantNotReached,
    NonEmptyOrders,
    MissingPath,
    NonEmptyPath,
    InvalidRequestDigest,
    MissingCaptureRevision,
    MissingCaptureDigest,
    WrongCaptureSource,
    ReplayMismatch,
    UnsupportedExecutable,
    CaptureRequestMismatch,
    MissingRetailSnapshotDigest,
    MissingQueryInputDigest,
    StaleQueryInput,
    InvalidSpotResult,
    MissingTypeAttributeReceipt,
    UnexpectedTypeAttributeReceipt,
    PartialPathScratchUnmodelled,
    StalePreparedInput,
}

impl fmt::Display for Frame0MerchantError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "2024 frame-zero Merchant unpack refused: {self:?}")
    }
}

impl std::error::Error for Frame0MerchantError {}

fn append_actor(image: &mut Vec<u8>, actor: &Frame0MerchantActorImage) {
    image.extend_from_slice(&actor.handle.id.to_le_bytes());
    image.extend_from_slice(&actor.handle.generation.to_le_bytes());
    image.extend_from_slice(&(actor.row as u64).to_le_bytes());
    image.push(actor.who);
    image.extend_from_slice(&actor.o.to_le_bytes());
    image.extend_from_slice(&actor.uid.to_le_bytes());
    image.extend_from_slice(&actor.type_index.to_le_bytes());
    image.push(actor.flags);
    for value in [
        actor.x,
        actor.y,
        actor.angle,
        actor.unit_masks as i32,
        actor.unit_masks2 as i32,
        actor.orders_x,
        actor.orders_y,
        actor.dest_angle,
        actor.path.capacity,
        actor.path.len(),
        i32::from(actor.path.increment),
    ] {
        image.extend_from_slice(&value.to_le_bytes());
    }
    image.extend_from_slice(&(actor.orders.len() as u64).to_le_bytes());
}

/// Stable digest over every public request claim other than the digest itself.
pub fn frame0_merchant_request_digest(request: &Frame0MerchantSpotRequest) -> [u8; 32] {
    let mut image = b"don-2024-frame0-merchant-spot-request-v1".to_vec();
    image.extend_from_slice(&request.replay_file_sha256);
    image.extend_from_slice(&request.setup_composition_digest);
    image.extend_from_slice(&request.object_world_digest.to_le_bytes());
    image.extend_from_slice(&request.terrain_world_checksum.full.to_le_bytes());
    image.extend_from_slice(&request.terrain_world_checksum.bytes.to_le_bytes());
    for section in &request.terrain_world_checksum.per_section {
        image.extend_from_slice(&section.adler.to_le_bytes());
        image.extend_from_slice(&section.bytes.to_le_bytes());
    }
    image.extend_from_slice(&(request.invocation_ordinal as u64).to_le_bytes());
    append_actor(&mut image, &request.actor);
    image.extend_from_slice(&request.radius.to_le_bytes());
    image.extend_from_slice(&request.random_state.to_le_bytes());
    sha256(&image)
}

/// Stable digest over every public native-capture claim other than the digest itself.
pub fn frame0_merchant_capture_digest(capture: &Frame0MerchantSpotCapture) -> [u8; 32] {
    let mut image = b"don-2024-frame0-merchant-spot-capture-v1".to_vec();
    image.extend_from_slice(&capture.revision.to_le_bytes());
    image.push(match capture.source {
        Frame0MerchantSpotSource::CompleteRetailFindMerchantSpotAtUnpackMerchant => 1,
    });
    image.extend_from_slice(&capture.replay_file_sha256);
    image.extend_from_slice(&capture.executable_sha256);
    image.extend_from_slice(&capture.request_sha256);
    image.extend_from_slice(&capture.actor_o.to_le_bytes());
    image.extend_from_slice(&capture.pre_call_retail_sim_sha256);
    image.extend_from_slice(&capture.query_input_sha256);
    match capture.outcome {
        MerchantSpotOutcome::NotFound => image.push(0),
        MerchantSpotOutcome::Found { tile_x, tile_y } => {
            image.push(1);
            image.extend_from_slice(&tile_x.to_le_bytes());
            image.extend_from_slice(&tile_y.to_le_bytes());
        }
    }
    match capture.has_type_attribute_0x7b {
        None => image.push(0),
        Some(value) => {
            image.push(1);
            image.push(u8::from(value));
        }
    }
    image.push(u8::from(capture.partial_path_104_was_null));
    sha256(&image)
}

fn actor_image(sim: &Sim, actor: Handle) -> Result<Frame0MerchantActorImage, Frame0MerchantError> {
    let row = sim
        .world
        .row_of(actor)
        .ok_or(Frame0MerchantError::StaleActor)?;
    let who = sim.world.units.get_who(row);
    if who != OWNER {
        return Err(Frame0MerchantError::WrongOwner {
            expected: OWNER,
            actual: who,
        });
    }
    let o = sim.world.units.o()[row];
    if !DUTCH_MERCHANT_OS.contains(&i32::from(o)) {
        return Err(Frame0MerchantError::WrongObjectIndex(i32::from(o)));
    }
    let world_type = sim
        .world
        .unit_type_id(row)
        .ok_or(Frame0MerchantError::WrongType {
            world: -1,
            projection: -1,
        })?;
    let projection = sim.unit_type.get(row).copied().unwrap_or(-1);
    if world_type != DUTCH_MERCHANT_TYPE || projection != world_type {
        return Err(Frame0MerchantError::WrongType {
            world: world_type,
            projection,
        });
    }
    if sim.world.units.get_flags(row) & OBJ_FLAG_ACTIVE == 0 {
        return Err(Frame0MerchantError::InactiveActor);
    }
    let orders = sim.world.orders(row).clone();
    if !orders.is_empty() {
        return Err(Frame0MerchantError::NonEmptyOrders);
    }
    let path = sim
        .paths
        .get(row)
        .cloned()
        .ok_or(Frame0MerchantError::MissingPath)?;
    if !path.is_empty() {
        return Err(Frame0MerchantError::NonEmptyPath);
    }
    let unit_masks = sim.world.units.get_unit_masks(row);
    if unit_masks & MERCHANT_THINK_MASK == 0 {
        return Err(Frame0MerchantError::ThinkMerchantNotReached);
    }
    Ok(Frame0MerchantActorImage {
        handle: actor,
        row,
        who,
        o,
        uid: sim.world.units.get_uid(row),
        type_index: world_type,
        flags: sim.world.units.get_flags(row),
        x: sim.world.units.x_internal()[row],
        y: sim.world.units.y_internal()[row],
        angle: sim.world.units.angle()[row],
        unit_masks,
        unit_masks2: sim.world.units.get_unit_masks2(row),
        orders_x: sim.world.units.orders_x()[row],
        orders_y: sim.world.units.orders_y()[row],
        dest_angle: sim.world.units.dest_angle()[row],
        orders,
        path,
    })
}

/// Construct the first-unsourced-child request without mutating the Sim.
pub fn request_frame0_merchant_spot(
    sim: &Sim,
    actor: Handle,
    setup_composition_digest: [u8; 32],
) -> Result<Frame0MerchantSpotRequest, Frame0MerchantError> {
    if setup_composition_digest == [0; 32] {
        return Err(Frame0MerchantError::MissingSetupCompositionDigest);
    }
    if sim.world.frame != 0 {
        return Err(Frame0MerchantError::WrongFrame {
            expected: 0,
            actual: sim.world.frame,
        });
    }
    let actor = actor_image(sim, actor)?;
    let invocation_ordinal = DUTCH_MERCHANT_OS
        .iter()
        .position(|&o| o == i32::from(actor.o))
        .expect("actor_image admitted one of the two golden Merchant indices");
    let mut request = Frame0MerchantSpotRequest {
        request_sha256: [0; 32],
        replay_file_sha256: REPLAY_FILE_SHA256,
        setup_composition_digest,
        object_world_digest: sim.world.digest(),
        terrain_world_checksum: sim.map.world.checksum_sections(),
        invocation_ordinal,
        actor,
        radius: MERCHANT_RADIUS,
        random_state: sim.world.random.state(),
    };
    request.request_sha256 = frame0_merchant_request_digest(&request);
    Ok(request)
}

fn validate_capture(
    request: &Frame0MerchantSpotRequest,
    capture: &Frame0MerchantSpotCapture,
) -> Result<(), Frame0MerchantError> {
    if request.request_sha256 != frame0_merchant_request_digest(request) {
        return Err(Frame0MerchantError::InvalidRequestDigest);
    }
    if capture.revision == 0 {
        return Err(Frame0MerchantError::MissingCaptureRevision);
    }
    if capture.composition_digest == [0; 32]
        || capture.composition_digest != frame0_merchant_capture_digest(capture)
    {
        return Err(Frame0MerchantError::MissingCaptureDigest);
    }
    if capture.source != Frame0MerchantSpotSource::CompleteRetailFindMerchantSpotAtUnpackMerchant {
        return Err(Frame0MerchantError::WrongCaptureSource);
    }
    if capture.replay_file_sha256 != REPLAY_FILE_SHA256
        || request.replay_file_sha256 != REPLAY_FILE_SHA256
    {
        return Err(Frame0MerchantError::ReplayMismatch);
    }
    if capture.executable_sha256 != SUPPORTED_RETAIL_EXE_SHA256 {
        return Err(Frame0MerchantError::UnsupportedExecutable);
    }
    if capture.request_sha256 != request.request_sha256
        || capture.actor_o != i32::from(request.actor.o)
    {
        return Err(Frame0MerchantError::CaptureRequestMismatch);
    }
    if capture.pre_call_retail_sim_sha256 == [0; 32] {
        return Err(Frame0MerchantError::MissingRetailSnapshotDigest);
    }
    if capture.query_input_sha256 == [0; 32] {
        return Err(Frame0MerchantError::MissingQueryInputDigest);
    }
    match (capture.outcome, capture.has_type_attribute_0x7b) {
        (MerchantSpotOutcome::NotFound, None) => {}
        (MerchantSpotOutcome::NotFound, Some(_)) => {
            return Err(Frame0MerchantError::UnexpectedTypeAttributeReceipt)
        }
        (MerchantSpotOutcome::Found { tile_x, tile_y }, Some(_))
            if tile_x >= 0
                && tile_y >= 0
                && tile_x <= i32::MAX / 192
                && tile_y <= i32::MAX / 192 => {}
        (MerchantSpotOutcome::Found { .. }, None) => {
            return Err(Frame0MerchantError::MissingTypeAttributeReceipt)
        }
        (MerchantSpotOutcome::Found { .. }, Some(_)) => {
            return Err(Frame0MerchantError::InvalidSpotResult)
        }
    }
    Ok(())
}

fn cast_order(spell: i32) -> Order {
    Order {
        kind: OrderIndex::CastSpell,
        x: -1,
        y: -1,
        target_who: -1,
        target_o: -1,
        target_uid: u16::MAX,
        economy: Some(EconomyOrderPayload::CastSpell(CastOrderPayload {
            paid: 0,
            spell,
        })),
        ..Order::default()
    }
}

fn move_order(actor: &Frame0MerchantActorImage, tile_x: i32, tile_y: i32) -> Order {
    // find_merchant_spot returns TCoord.  unpack_merchant computes its angle against the
    // uncentred Coord (TCoord * 192), then uses div_3_table[(Coord >> 4)] * 48 + 24 for the
    // MoveOrder endpoint.  div_3_table is constructed at 0x00681db0 as signed integer / 3;
    // a valid non-negative TCoord therefore becomes tile * 192 + 24 exactly.
    let raw_x = tile_x.wrapping_mul(192);
    let raw_y = tile_y.wrapping_mul(192);
    let x = raw_x.wrapping_add(24);
    let y = raw_y.wrapping_add(24);
    let angle = find_angle(raw_x.wrapping_sub(actor.x), raw_y.wrapping_sub(actor.y));
    let state = MoveOrderState {
        angle,
        dest: 0,
        pause: 0,
        retry: 0,
        attempts: 0,
        timer: 0,
        facing: -1,
        dest_x: x,
        dest_y: y,
        last_x: -1,
        last_y: -1,
        coll_x: 0,
        coll_y: 0,
        orig_x: -1,
        orig_y: -1,
        off_x: (x % 0x300) as i16,
        off_y: (y % 0x300) as i16,
        ..MoveOrderState::default()
    };
    Order {
        kind: OrderIndex::MoveTo,
        flags: 0,
        x,
        y,
        tolerance: 0,
        move_state: Some(state),
        ..Order::default()
    }
}

fn success_image(
    request: &Frame0MerchantSpotRequest,
    capture: &Frame0MerchantSpotCapture,
) -> Result<Frame0MerchantSuccessImage, Frame0MerchantError> {
    let MerchantSpotOutcome::Found { tile_x, tile_y } = capture.outcome else {
        return Err(Frame0MerchantError::InvalidSpotResult);
    };
    if !capture.partial_path_104_was_null {
        return Err(Frame0MerchantError::PartialPathScratchUnmodelled);
    }
    let attr = capture
        .has_type_attribute_0x7b
        .ok_or(Frame0MerchantError::MissingTypeAttributeReceipt)?;
    let spell = if attr {
        FAST_DEPLOY_SPELL
    } else {
        DUTCH_DEPLOY_SPELL
    };
    let mover = move_order(&request.actor, tile_x, tile_y);
    let mut orders = OrderList::new();
    // Retail installs Cast with QueuePos::New, appends Move, then rotates the linked-list head.
    // Flattened current-first order is therefore Move followed by Cast.
    orders.push(mover.clone());
    orders.push(cast_order(spell));
    let mut path = request.actor.path.clone();
    path.clear();
    Ok(Frame0MerchantSuccessImage {
        // unpack succeeds -> think_merchant returns 1 -> Unit::think's common success
        // tail clears ObjectData::flags bit 0x10.
        flags: request.actor.flags & !0x10,
        unit_masks: request.actor.unit_masks & !UNPACK_CLEAR_MASK & !QUEUE_NEW_CLEAR_MASK,
        unit_masks2: request.actor.unit_masks2,
        orders_x: mover.x,
        orders_y: mover.y,
        dest_angle: mover
            .move_state
            .expect("move_order always materializes its scalar image")
            .angle,
        orders,
        path,
        random_state: request.random_state,
    })
}

/// Validate a retail answer and prepare either the exact success image or the unported tail.
/// No state is mutated here.
pub fn prepare_frame0_merchant_resolution(
    sim: &Sim,
    request: Frame0MerchantSpotRequest,
    capture: Frame0MerchantSpotCapture,
) -> Result<Frame0MerchantResolution, Frame0MerchantError> {
    let current =
        request_frame0_merchant_spot(sim, request.actor.handle, request.setup_composition_digest)?;
    if current != request {
        return Err(Frame0MerchantError::StalePreparedInput);
    }
    validate_capture(&request, &capture)?;
    if capture.outcome == MerchantSpotOutcome::NotFound {
        return Ok(Frame0MerchantResolution::FailedUnpackTail(
            Frame0MerchantTailRequest {
                request,
                capture,
                next_va: FAILED_UNPACK_TAIL_VA,
                reason:
                    "unpack_merchant returned 0; think_merchant continues with the Good-object scan",
            },
        ));
    }
    let after = success_image(&request, &capture)?;
    Ok(Frame0MerchantResolution::Success(
        PreparedFrame0MerchantSuccess {
            after,
            request,
            capture,
        },
    ))
}

/// Commit the prepared success image atomically after repeating every current-state check.
pub fn commit_frame0_merchant_success(
    sim: &mut Sim,
    prepared: PreparedFrame0MerchantSuccess,
    installed_query_input_sha256: [u8; 32],
) -> Result<Frame0MerchantCommitReceipt, Frame0MerchantError> {
    let current = request_frame0_merchant_spot(
        sim,
        prepared.request.actor.handle,
        prepared.request.setup_composition_digest,
    )
    .map_err(|_| Frame0MerchantError::StalePreparedInput)?;
    if current != prepared.request {
        return Err(Frame0MerchantError::StalePreparedInput);
    }
    validate_capture(&prepared.request, &prepared.capture)?;
    if success_image(&prepared.request, &prepared.capture)? != prepared.after {
        return Err(Frame0MerchantError::StalePreparedInput);
    }
    if installed_query_input_sha256 != prepared.capture.query_input_sha256 {
        return Err(Frame0MerchantError::StaleQueryInput);
    }
    if !prepared.capture.partial_path_104_was_null
        || !matches!(prepared.capture.outcome, MerchantSpotOutcome::Found { .. })
    {
        return Err(Frame0MerchantError::StalePreparedInput);
    }

    let orders_after = prepared.after.orders.clone();
    let path_after = prepared.after.path.clone();
    // All fallible/allocation work is complete.  The stores below are the entire canonical
    // mutation set.
    let row = prepared.request.actor.row;
    sim.world
        .units
        .set_unit_masks(row, prepared.after.unit_masks);
    *sim.world.orders_mut(row) = orders_after;
    sim.paths[row] = path_after;
    sim.world.units.orders_x_mut()[row] = prepared.after.orders_x;
    sim.world.units.orders_y_mut()[row] = prepared.after.orders_y;
    sim.world.units.dest_angle_mut()[row] = prepared.after.dest_angle;
    sim.world.units.set_flags(row, prepared.after.flags);

    Ok(Frame0MerchantCommitReceipt {
        request_sha256: prepared.request.request_sha256,
        capture_digest: prepared.capture.composition_digest,
        actor: prepared.request.actor.handle,
        before: prepared.request.actor,
        after: prepared.after,
        think_merchant_returned_one: true,
        unit_think_success_tail_va: UNIT_THINK_SUCCESS_TAIL_VA,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (Sim, Handle, Handle, [u8; 32]) {
        let mut sim = Sim::new(0x1234_5678, 16);
        sim.activate(OWNER as usize);
        let _scout = sim.spawn_unit(OWNER as usize, 69, 1_000, 2_000, 4).unwrap();
        let o1 = sim
            .spawn_unit(OWNER as usize, DUTCH_MERCHANT_TYPE, 2_000, 3_000, 4)
            .unwrap();
        let o2 = sim
            .spawn_unit(OWNER as usize, DUTCH_MERCHANT_TYPE, 4_000, 5_000, 4)
            .unwrap();
        for actor in [o1, o2] {
            let row = sim.world.row_of(actor).unwrap();
            sim.world.units.set_unit_masks(
                row,
                MERCHANT_THINK_MASK | UNPACK_CLEAR_MASK | QUEUE_NEW_CLEAR_MASK | 0x20,
            );
            sim.world.units.set_unit_masks2(row, 0x55aa_0011);
            sim.world
                .units
                .set_flags(row, OBJ_FLAG_ACTIVE | 0x10 | 0x40);
            sim.paths[row] = PathStack::with_header(17, 9).unwrap();
        }
        (sim, o1, o2, [0x51; 32])
    }

    fn capture(
        request: &Frame0MerchantSpotRequest,
        outcome: MerchantSpotOutcome,
        attr: Option<bool>,
    ) -> Frame0MerchantSpotCapture {
        let mut capture = Frame0MerchantSpotCapture {
            revision: 7,
            composition_digest: [0; 32],
            source: Frame0MerchantSpotSource::CompleteRetailFindMerchantSpotAtUnpackMerchant,
            replay_file_sha256: REPLAY_FILE_SHA256,
            executable_sha256: SUPPORTED_RETAIL_EXE_SHA256,
            request_sha256: request.request_sha256,
            actor_o: i32::from(request.actor.o),
            pre_call_retail_sim_sha256: [0x39; 32],
            query_input_sha256: [0xa4; 32],
            outcome,
            has_type_attribute_0x7b: attr,
            partial_path_104_was_null: true,
        };
        capture.composition_digest = frame0_merchant_capture_digest(&capture);
        capture
    }

    #[test]
    fn o1_and_o2_requests_are_distinct_and_mutation_free() {
        let (sim, o1, o2, setup_digest) = setup();
        let r1 = request_frame0_merchant_spot(&sim, o1, setup_digest).unwrap();
        let r2 = request_frame0_merchant_spot(&sim, o2, setup_digest).unwrap();
        assert_eq!(r1.invocation_ordinal, 0);
        assert_eq!(r2.invocation_ordinal, 1);
        assert_eq!((r1.actor.who, r1.actor.o), (OWNER, 1));
        assert_eq!((r2.actor.who, r2.actor.o), (OWNER, 2));
        assert_ne!(r1.request_sha256, r2.request_sha256);
        assert_eq!(
            r1,
            request_frame0_merchant_spot(&sim, o1, setup_digest).unwrap()
        );
    }

    #[test]
    fn found_spot_commits_move_then_cast_and_only_the_owned_fields() {
        let (mut sim, o1, _, setup_digest) = setup();
        let request = request_frame0_merchant_spot(&sim, o1, setup_digest).unwrap();
        let rng_before = sim.world.random.state();
        let other_row = sim.world.unit_row_at(OWNER.into(), 2).unwrap();
        let other_masks = sim.world.units.get_unit_masks(other_row);
        let resolution = prepare_frame0_merchant_resolution(
            &sim,
            request,
            capture(
                &request_frame0_merchant_spot(&sim, o1, setup_digest).unwrap(),
                MerchantSpotOutcome::Found {
                    tile_x: 12,
                    tile_y: 20,
                },
                Some(false),
            ),
        )
        .unwrap();
        let Frame0MerchantResolution::Success(prepared) = resolution else {
            panic!("expected success")
        };
        let expected_angle = find_angle(12 * 192 - 2_000, 20 * 192 - 3_000);
        assert_eq!(prepared.after.orders_x, 12 * 192 + 24);
        assert_eq!(prepared.after.orders_y, 20 * 192 + 24);
        assert_eq!(prepared.after.dest_angle, expected_angle);
        let query_digest = prepared.capture.query_input_sha256;
        let receipt = commit_frame0_merchant_success(&mut sim, prepared, query_digest).unwrap();
        let row = sim.world.row_of(o1).unwrap();
        assert_eq!(sim.world.units.get_flags(row), OBJ_FLAG_ACTIVE | 0x40);
        assert_eq!(
            sim.world.units.get_unit_masks(row),
            MERCHANT_THINK_MASK | 0x20
        );
        assert_eq!(sim.world.units.get_unit_masks2(row), 0x55aa_0011);
        assert_eq!(sim.paths[row].checksum_header(), (17, 0, 9));
        let orders: Vec<_> = sim.world.orders(row).iter().collect();
        assert_eq!(orders.len(), 2);
        assert_eq!(orders[0].kind, OrderIndex::MoveTo);
        assert_eq!((orders[0].x, orders[0].y), (2_328, 3_864));
        let move_state = orders[0].move_state.unwrap();
        assert_eq!((move_state.orig_x, move_state.orig_y), (-1, -1));
        assert_eq!((move_state.off_x, move_state.off_y), (24, 24));
        assert_eq!(orders[1].kind, OrderIndex::CastSpell);
        assert_eq!(
            orders[1].economy,
            Some(EconomyOrderPayload::CastSpell(CastOrderPayload {
                paid: 0,
                spell: DUTCH_DEPLOY_SPELL,
            }))
        );
        assert_eq!(sim.world.random.state(), rng_before);
        assert_eq!(sim.world.units.get_unit_masks(other_row), other_masks);
        assert!(receipt.think_merchant_returned_one);
        assert_eq!(
            receipt.unit_think_success_tail_va,
            UNIT_THINK_SUCCESS_TAIL_VA
        );
    }

    #[test]
    fn attr_0x7b_precedes_the_type62_special_object_normalization() {
        let (sim, o1, _, setup_digest) = setup();
        let request = request_frame0_merchant_spot(&sim, o1, setup_digest).unwrap();
        let receipt = capture(
            &request,
            MerchantSpotOutcome::Found {
                tile_x: 3,
                tile_y: 4,
            },
            Some(true),
        );
        let Frame0MerchantResolution::Success(prepared) =
            prepare_frame0_merchant_resolution(&sim, request, receipt).unwrap()
        else {
            panic!("expected success")
        };
        assert_eq!(
            prepared.after.orders.iter().nth(1).unwrap().economy,
            Some(EconomyOrderPayload::CastSpell(CastOrderPayload {
                paid: 0,
                spell: FAST_DEPLOY_SPELL,
            }))
        );
    }

    #[test]
    fn not_found_is_a_typed_tail_and_does_not_mutate() {
        let (sim, o1, _, setup_digest) = setup();
        let request = request_frame0_merchant_spot(&sim, o1, setup_digest).unwrap();
        let before = request.clone();
        let receipt = capture(&request, MerchantSpotOutcome::NotFound, None);
        let Frame0MerchantResolution::FailedUnpackTail(tail) =
            prepare_frame0_merchant_resolution(&sim, request, receipt).unwrap()
        else {
            panic!("failed unpack must not be collapsed to success")
        };
        assert_eq!(tail.next_va, FAILED_UNPACK_TAIL_VA);
        assert_eq!(
            before,
            request_frame0_merchant_spot(&sim, o1, setup_digest).unwrap()
        );
    }

    #[test]
    fn nonnull_partial_path_scratch_refuses_before_any_store() {
        let (sim, o1, _, setup_digest) = setup();
        let request = request_frame0_merchant_spot(&sim, o1, setup_digest).unwrap();
        let before = request.clone();
        let mut receipt = capture(
            &request,
            MerchantSpotOutcome::Found {
                tile_x: 3,
                tile_y: 4,
            },
            Some(false),
        );
        receipt.partial_path_104_was_null = false;
        receipt.composition_digest = frame0_merchant_capture_digest(&receipt);
        assert_eq!(
            prepare_frame0_merchant_resolution(&sim, request, receipt),
            Err(Frame0MerchantError::PartialPathScratchUnmodelled)
        );
        assert_eq!(
            before,
            request_frame0_merchant_spot(&sim, o1, setup_digest).unwrap()
        );
    }

    #[test]
    fn stale_commit_is_rejected_without_partial_mutation() {
        let (mut sim, o1, _, setup_digest) = setup();
        let request = request_frame0_merchant_spot(&sim, o1, setup_digest).unwrap();
        let receipt = capture(
            &request,
            MerchantSpotOutcome::Found {
                tile_x: 3,
                tile_y: 4,
            },
            Some(false),
        );
        let Frame0MerchantResolution::Success(prepared) =
            prepare_frame0_merchant_resolution(&sim, request, receipt).unwrap()
        else {
            panic!("expected success")
        };
        let row = sim.world.row_of(o1).unwrap();
        sim.world.units.set_unit_masks2(row, 0x1122_3344);
        let stale_image = request_frame0_merchant_spot(&sim, o1, setup_digest).unwrap();
        assert_eq!(
            commit_frame0_merchant_success(&mut sim, prepared, [0xa4; 32]),
            Err(Frame0MerchantError::StalePreparedInput)
        );
        assert_eq!(
            stale_image,
            request_frame0_merchant_spot(&sim, o1, setup_digest).unwrap()
        );
    }

    #[test]
    fn stale_query_authority_is_rejected_without_partial_mutation() {
        let (mut sim, o1, _, setup_digest) = setup();
        let request = request_frame0_merchant_spot(&sim, o1, setup_digest).unwrap();
        let receipt = capture(
            &request,
            MerchantSpotOutcome::Found {
                tile_x: 3,
                tile_y: 4,
            },
            Some(false),
        );
        let Frame0MerchantResolution::Success(prepared) =
            prepare_frame0_merchant_resolution(&sim, request, receipt).unwrap()
        else {
            panic!("expected success")
        };
        let before = request_frame0_merchant_spot(&sim, o1, setup_digest).unwrap();
        assert_eq!(
            commit_frame0_merchant_success(&mut sim, prepared, [0x77; 32]),
            Err(Frame0MerchantError::StaleQueryInput)
        );
        assert_eq!(
            before,
            request_frame0_merchant_spot(&sim, o1, setup_digest).unwrap()
        );
    }

    #[test]
    fn tampered_after_image_is_rejected_without_partial_mutation() {
        let (mut sim, o1, _, setup_digest) = setup();
        let request = request_frame0_merchant_spot(&sim, o1, setup_digest).unwrap();
        let receipt = capture(
            &request,
            MerchantSpotOutcome::Found {
                tile_x: 3,
                tile_y: 4,
            },
            Some(false),
        );
        let Frame0MerchantResolution::Success(mut prepared) =
            prepare_frame0_merchant_resolution(&sim, request, receipt).unwrap()
        else {
            panic!("expected success")
        };
        prepared.after.unit_masks ^= 1;
        let before = request_frame0_merchant_spot(&sim, o1, setup_digest).unwrap();
        assert_eq!(
            commit_frame0_merchant_success(&mut sim, prepared, [0xa4; 32]),
            Err(Frame0MerchantError::StalePreparedInput)
        );
        assert_eq!(
            before,
            request_frame0_merchant_spot(&sim, o1, setup_digest).unwrap()
        );
    }
}
