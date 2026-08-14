//! Exact read-only `ObjectsData::find_good_at(WCoord,WCoord,who,0,0)` continuation for
//! the golden frame-zero Merchants.
//!
//! The emitted call has its fifth argument clear. Retail therefore does not scan
//! `0..ObjectsData::good_mark` and does not compare Good coordinates. It reads the requested
//! WData cell, follows its nonnegative `(down_who, down)` Object addresses in order, and
//! accepts only the terminal `down == -2` base-Good marker. The terminal Good must be active,
//! must have a live type pointer, and must not be TypeIndex 5 (Oil). With the fourth argument
//! clear and the Merchant owner nonnegative, the first remaining child is
//! `LeaderData::type_avail(good_type, 1)` (`0x006e33a0`).
//!
//! Live Unit, Build, and Wall links are resolved from the canonical `Sim` sparse object
//! registry and their owning rows. The terminal base Good is read from `OilGoodRuntime`, whose
//! name is historical: it retains every base-Good slot, not only Oil. No state or RNG is
//! mutated. A typed, digest-bound type-availability request is emitted rather than guessing
//! the Leader/type graph.

#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::fmt;

use don_sim::systems::sparse_object_bands_authority_frontier::{RetailBand, RetailObjectAddress};
use don_sim::systems::world_oil_goods::{OilGoodRuntime, OIL_GOOD_TYPE};
use don_sim::tick::Sim;
use don_sim::world::{Handle, WorldObjectIdentity};

use crate::setup_2024_frame0_merchant_search::{
    validate_frame0_merchant_good_lookup_request, Frame0MerchantGoodLookupRequest,
    Frame0MerchantSearchError, OBJECTS_FIND_GOOD_AT_WCOORD_VA,
};
use crate::setup_2024_frame0_merchant_unpack::Frame0MerchantSpotRequest;
use crate::setup_unit_member_authority::CanonicalSetupUnitMemberReceipt;
use crate::world_owner_frontier::sha256;

pub const LEADER_TYPE_AVAIL_VA: u32 = 0x006e_33a0;
pub const GOOD_TERMINAL: i16 = -2;
pub const NOT_FOUND: i32 = -1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame0MerchantObjectIdentity {
    Unit {
        handle: Handle,
        row: usize,
        uid: u16,
    },
    Build {
        row: usize,
        uid: u16,
    },
    Wall {
        row: usize,
        uid: u16,
    },
}

/// One exact `ObjectData::{down,down_who}` read in retail chain order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame0MerchantObjectLinkRead {
    pub ordinal: usize,
    pub owner: i16,
    pub object: i16,
    pub identity: Frame0MerchantObjectIdentity,
    pub next_object: i16,
    pub next_owner: i16,
}

/// The terminal Good reads actually reached by the fifth-argument-zero arm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame0MerchantGoodSlotRead {
    pub logical_length: usize,
    pub slot: i16,
    pub flags: u8,
    /// `None` when the active-bit filter returned before reading `ptype`.
    pub ptype_present: Option<bool>,
    /// `None` when the active-bit filter returned before dereferencing `ptype + 4`.
    pub type_index: Option<i32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame0MerchantGoodNotFoundReason {
    TerminalWasNotGood,
    InactiveGood,
    OilGood,
    TypeUnavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame0MerchantGoodLookupOutcome {
    FoundType(i32),
    NotFound(Frame0MerchantGoodNotFoundReason),
}

/// Complete read-only result of the emitted `find_good_at` child.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame0MerchantGoodLookupReceipt {
    pub receipt_sha256: [u8; 32],
    pub lookup_request_sha256: [u8; 32],
    pub call_va: u32,
    pub world_x: i32,
    pub world_y: i32,
    pub world_index: usize,
    pub initial_object: i16,
    pub initial_owner: i16,
    pub object_links: Vec<Frame0MerchantObjectLinkRead>,
    pub terminal_object: i16,
    pub terminal_owner: i16,
    pub good: Option<Frame0MerchantGoodSlotRead>,
    pub type_avail_checked: Option<bool>,
    pub type_avail_capture: Option<Frame0MerchantTypeAvailCapture>,
    pub outcome: Frame0MerchantGoodLookupOutcome,
    pub return_value: i32,
    pub random_state_before: i32,
    pub random_state_after: i32,
    pub rng_draws: u32,
}

/// Exact first child after a valid, active, non-Oil terminal Good.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame0MerchantTypeAvailRequest {
    pub request_sha256: [u8; 32],
    pub lookup_request_sha256: [u8; 32],
    pub traversal_prefix_sha256: [u8; 32],
    pub call_va: u32,
    pub leader: i32,
    pub type_index: i32,
    pub strict: i32,
    pub good_slot: i16,
    pub random_state: i32,
}

/// Detached authority answer for one exact `LeaderData::type_avail(type, 1)` request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame0MerchantTypeAvailCapture {
    pub capture_sha256: [u8; 32],
    pub request_sha256: [u8; 32],
    pub installed_query_input_sha256: [u8; 32],
    pub available: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame0MerchantGoodLookupFrontier {
    Resolved(Frame0MerchantGoodLookupReceipt),
    NeedsTypeAvail(Frame0MerchantTypeAvailRequest),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame0MerchantGoodLookupError {
    Parent(Frame0MerchantSearchError),
    UnsupportedCallShape,
    WorldCellOutOfBounds { world_x: i32, world_y: i32 },
    ObjectChainCycle { owner: i16, object: i16 },
    UnsupportedObjectAddress { owner: i16, object: i16 },
    StaleObjectRegistry { owner: i16, object: i16 },
    MissingGoodSlot { slot: i16, logical_length: usize },
    NullGoodTypePointer { slot: i16 },
    InvalidTypeAvailRequestDigest,
    StaleTypeAvailRequest,
    MissingTypeAvailAuthorityDigest,
    InvalidTypeAvailCaptureDigest,
}

impl fmt::Display for Frame0MerchantGoodLookupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "2024 frame-zero Merchant Good lookup refused: {self:?}")
    }
}

impl std::error::Error for Frame0MerchantGoodLookupError {}

impl From<Frame0MerchantSearchError> for Frame0MerchantGoodLookupError {
    fn from(value: Frame0MerchantSearchError) -> Self {
        Self::Parent(value)
    }
}

fn append_identity(image: &mut Vec<u8>, identity: Frame0MerchantObjectIdentity) {
    match identity {
        Frame0MerchantObjectIdentity::Unit { handle, row, uid } => {
            image.push(0);
            image.extend_from_slice(&handle.id.to_le_bytes());
            image.extend_from_slice(&handle.generation.to_le_bytes());
            image.extend_from_slice(&(row as u64).to_le_bytes());
            image.extend_from_slice(&uid.to_le_bytes());
        }
        Frame0MerchantObjectIdentity::Build { row, uid } => {
            image.push(1);
            image.extend_from_slice(&(row as u64).to_le_bytes());
            image.extend_from_slice(&uid.to_le_bytes());
        }
        Frame0MerchantObjectIdentity::Wall { row, uid } => {
            image.push(2);
            image.extend_from_slice(&(row as u64).to_le_bytes());
            image.extend_from_slice(&uid.to_le_bytes());
        }
    }
}

fn append_link(image: &mut Vec<u8>, link: &Frame0MerchantObjectLinkRead) {
    image.extend_from_slice(&(link.ordinal as u64).to_le_bytes());
    image.extend_from_slice(&link.owner.to_le_bytes());
    image.extend_from_slice(&link.object.to_le_bytes());
    append_identity(image, link.identity);
    image.extend_from_slice(&link.next_object.to_le_bytes());
    image.extend_from_slice(&link.next_owner.to_le_bytes());
}

fn append_good(image: &mut Vec<u8>, good: Option<Frame0MerchantGoodSlotRead>) {
    let Some(good) = good else {
        image.push(0);
        return;
    };
    image.push(1);
    image.extend_from_slice(&(good.logical_length as u64).to_le_bytes());
    image.extend_from_slice(&good.slot.to_le_bytes());
    image.push(good.flags);
    match good.ptype_present {
        None => image.push(0),
        Some(value) => {
            image.push(1);
            image.push(u8::from(value));
        }
    }
    match good.type_index {
        None => image.push(0),
        Some(value) => {
            image.push(1);
            image.extend_from_slice(&value.to_le_bytes());
        }
    }
}

fn append_prefix(
    image: &mut Vec<u8>,
    child: &Frame0MerchantGoodLookupRequest,
    world_index: usize,
    initial_object: i16,
    initial_owner: i16,
    links: &[Frame0MerchantObjectLinkRead],
    terminal_object: i16,
    terminal_owner: i16,
    good: Option<Frame0MerchantGoodSlotRead>,
    random_state: i32,
) {
    image.extend_from_slice(&child.request_sha256);
    image.extend_from_slice(&OBJECTS_FIND_GOOD_AT_WCOORD_VA.to_le_bytes());
    image.extend_from_slice(&child.world_x.to_le_bytes());
    image.extend_from_slice(&child.world_y.to_le_bytes());
    image.extend_from_slice(&(world_index as u64).to_le_bytes());
    image.extend_from_slice(&initial_object.to_le_bytes());
    image.extend_from_slice(&initial_owner.to_le_bytes());
    image.extend_from_slice(&(links.len() as u64).to_le_bytes());
    for link in links {
        append_link(image, link);
    }
    image.extend_from_slice(&terminal_object.to_le_bytes());
    image.extend_from_slice(&terminal_owner.to_le_bytes());
    append_good(image, good);
    image.extend_from_slice(&random_state.to_le_bytes());
}

#[allow(clippy::too_many_arguments)]
fn traversal_prefix_digest(
    child: &Frame0MerchantGoodLookupRequest,
    world_index: usize,
    initial_object: i16,
    initial_owner: i16,
    links: &[Frame0MerchantObjectLinkRead],
    terminal_object: i16,
    terminal_owner: i16,
    good: Option<Frame0MerchantGoodSlotRead>,
    random_state: i32,
) -> [u8; 32] {
    let mut image = Vec::with_capacity(192 + links.len() * 48);
    append_prefix(
        &mut image,
        child,
        world_index,
        initial_object,
        initial_owner,
        links,
        terminal_object,
        terminal_owner,
        good,
        random_state,
    );
    sha256(&image)
}

pub fn frame0_merchant_type_avail_request_digest(
    request: &Frame0MerchantTypeAvailRequest,
) -> [u8; 32] {
    let mut image = Vec::with_capacity(128);
    image.extend_from_slice(&request.lookup_request_sha256);
    image.extend_from_slice(&request.traversal_prefix_sha256);
    image.extend_from_slice(&request.call_va.to_le_bytes());
    image.extend_from_slice(&request.leader.to_le_bytes());
    image.extend_from_slice(&request.type_index.to_le_bytes());
    image.extend_from_slice(&request.strict.to_le_bytes());
    image.extend_from_slice(&request.good_slot.to_le_bytes());
    image.extend_from_slice(&request.random_state.to_le_bytes());
    sha256(&image)
}

pub fn frame0_merchant_type_avail_capture_digest(
    capture: &Frame0MerchantTypeAvailCapture,
) -> [u8; 32] {
    let mut image = Vec::with_capacity(96);
    image.extend_from_slice(&capture.request_sha256);
    image.extend_from_slice(&capture.installed_query_input_sha256);
    image.push(u8::from(capture.available));
    sha256(&image)
}

pub fn frame0_merchant_good_lookup_receipt_digest(
    receipt: &Frame0MerchantGoodLookupReceipt,
) -> [u8; 32] {
    let mut image = Vec::with_capacity(224 + receipt.object_links.len() * 48);
    image.extend_from_slice(&receipt.lookup_request_sha256);
    image.extend_from_slice(&receipt.call_va.to_le_bytes());
    image.extend_from_slice(&receipt.world_x.to_le_bytes());
    image.extend_from_slice(&receipt.world_y.to_le_bytes());
    image.extend_from_slice(&(receipt.world_index as u64).to_le_bytes());
    image.extend_from_slice(&receipt.initial_object.to_le_bytes());
    image.extend_from_slice(&receipt.initial_owner.to_le_bytes());
    image.extend_from_slice(&(receipt.object_links.len() as u64).to_le_bytes());
    for link in &receipt.object_links {
        append_link(&mut image, link);
    }
    image.extend_from_slice(&receipt.terminal_object.to_le_bytes());
    image.extend_from_slice(&receipt.terminal_owner.to_le_bytes());
    append_good(&mut image, receipt.good);
    match receipt.type_avail_checked {
        None => image.push(0),
        Some(value) => {
            image.push(1);
            image.push(u8::from(value));
        }
    }
    match receipt.type_avail_capture {
        None => image.push(0),
        Some(capture) => {
            image.push(1);
            image.extend_from_slice(&capture.capture_sha256);
            image.extend_from_slice(&capture.request_sha256);
            image.extend_from_slice(&capture.installed_query_input_sha256);
            image.push(u8::from(capture.available));
        }
    }
    match receipt.outcome {
        Frame0MerchantGoodLookupOutcome::FoundType(value) => {
            image.push(0);
            image.extend_from_slice(&value.to_le_bytes());
        }
        Frame0MerchantGoodLookupOutcome::NotFound(reason) => {
            image.push(1);
            image.push(match reason {
                Frame0MerchantGoodNotFoundReason::TerminalWasNotGood => 0,
                Frame0MerchantGoodNotFoundReason::InactiveGood => 1,
                Frame0MerchantGoodNotFoundReason::OilGood => 2,
                Frame0MerchantGoodNotFoundReason::TypeUnavailable => 3,
            });
        }
    }
    image.extend_from_slice(&receipt.return_value.to_le_bytes());
    image.extend_from_slice(&receipt.random_state_before.to_le_bytes());
    image.extend_from_slice(&receipt.random_state_after.to_le_bytes());
    image.extend_from_slice(&receipt.rng_draws.to_le_bytes());
    sha256(&image)
}

fn band_for(object: i16) -> Option<RetailBand> {
    let object = i32::from(object);
    RetailBand::ALL
        .into_iter()
        .find(|band| band.contains(object))
}

fn read_i16(image: &[u8], offset: usize) -> i16 {
    i16::from_le_bytes(
        image[offset..offset + 2]
            .try_into()
            .expect("fixed ObjectData i16 window"),
    )
}

fn canonical_object_link(
    sim: &Sim,
    ordinal: usize,
    owner: i16,
    object: i16,
) -> Result<Frame0MerchantObjectLinkRead, Frame0MerchantGoodLookupError> {
    let owner_u8 = u8::try_from(owner)
        .map_err(|_| Frame0MerchantGoodLookupError::UnsupportedObjectAddress { owner, object })?;
    let band = band_for(object)
        .ok_or(Frame0MerchantGoodLookupError::UnsupportedObjectAddress { owner, object })?;
    let address = RetailObjectAddress::new(owner_u8, band, i32::from(object));
    if !address.is_well_formed() {
        return Err(Frame0MerchantGoodLookupError::UnsupportedObjectAddress { owner, object });
    }
    let identity = sim
        .world
        .object_bands()
        .live_identity(address)
        .ok_or(Frame0MerchantGoodLookupError::StaleObjectRegistry { owner, object })?;
    let (identity, next_object, next_owner) = match identity {
        WorldObjectIdentity::Unit { id, generation } if band == RetailBand::Unit => {
            let handle = Handle { id, generation };
            let row = sim
                .world
                .row_of(handle)
                .ok_or(Frame0MerchantGoodLookupError::StaleObjectRegistry { owner, object })?;
            if sim.world.units.get_who(row) != owner_u8 || sim.world.units.o()[row] != object {
                return Err(Frame0MerchantGoodLookupError::StaleObjectRegistry { owner, object });
            }
            (
                Frame0MerchantObjectIdentity::Unit {
                    handle,
                    row,
                    uid: sim.world.units.get_uid(row),
                },
                sim.world.units.down()[row],
                sim.world.units.down_who()[row],
            )
        }
        WorldObjectIdentity::BuildRow(row) if band == RetailBand::Build => {
            let row = row as usize;
            let build = sim
                .builds
                .get(row)
                .ok_or(Frame0MerchantGoodLookupError::StaleObjectRegistry { owner, object })?;
            if build.who != owner_u8 || build.object_id() != object {
                return Err(Frame0MerchantGoodLookupError::StaleObjectRegistry { owner, object });
            }
            (
                Frame0MerchantObjectIdentity::Build {
                    row,
                    uid: build.uid,
                },
                read_i16(&build.other, 0x2c),
                read_i16(&build.other, 0x2e),
            )
        }
        WorldObjectIdentity::WallRow(row) if band == RetailBand::Wall => {
            let row = row as usize;
            let wall = sim
                .walls
                .get(row)
                .ok_or(Frame0MerchantGoodLookupError::StaleObjectRegistry { owner, object })?;
            if wall.who != owner_u8 {
                return Err(Frame0MerchantGoodLookupError::StaleObjectRegistry { owner, object });
            }
            (
                Frame0MerchantObjectIdentity::Wall { row, uid: wall.uid },
                wall.down,
                wall.down_who,
            )
        }
        _ => {
            return Err(Frame0MerchantGoodLookupError::StaleObjectRegistry { owner, object });
        }
    };
    Ok(Frame0MerchantObjectLinkRead {
        ordinal,
        owner,
        object,
        identity,
        next_object,
        next_owner,
    })
}

#[allow(clippy::too_many_arguments)]
fn resolved_receipt(
    child: &Frame0MerchantGoodLookupRequest,
    world_index: usize,
    initial_object: i16,
    initial_owner: i16,
    object_links: Vec<Frame0MerchantObjectLinkRead>,
    terminal_object: i16,
    terminal_owner: i16,
    good: Option<Frame0MerchantGoodSlotRead>,
    type_avail_checked: Option<bool>,
    type_avail_capture: Option<Frame0MerchantTypeAvailCapture>,
    outcome: Frame0MerchantGoodLookupOutcome,
    random_state: i32,
) -> Frame0MerchantGoodLookupReceipt {
    let return_value = match outcome {
        Frame0MerchantGoodLookupOutcome::FoundType(value) => value,
        Frame0MerchantGoodLookupOutcome::NotFound(_) => NOT_FOUND,
    };
    let mut receipt = Frame0MerchantGoodLookupReceipt {
        receipt_sha256: [0; 32],
        lookup_request_sha256: child.request_sha256,
        call_va: OBJECTS_FIND_GOOD_AT_WCOORD_VA,
        world_x: child.world_x,
        world_y: child.world_y,
        world_index,
        initial_object,
        initial_owner,
        object_links,
        terminal_object,
        terminal_owner,
        good,
        type_avail_checked,
        type_avail_capture,
        outcome,
        return_value,
        random_state_before: random_state,
        random_state_after: random_state,
        rng_draws: 0,
    };
    receipt.receipt_sha256 = frame0_merchant_good_lookup_receipt_digest(&receipt);
    receipt
}

fn evaluate_prevalidated(
    sim: &Sim,
    child: &Frame0MerchantGoodLookupRequest,
    goods: &OilGoodRuntime,
) -> Result<Frame0MerchantGoodLookupFrontier, Frame0MerchantGoodLookupError> {
    if child.find_good_at_call_va != OBJECTS_FIND_GOOD_AT_WCOORD_VA
        || child.lookup_arg4 != 0
        || child.lookup_arg5 != 0
        || child.lookup_owner < 0
    {
        return Err(Frame0MerchantGoodLookupError::UnsupportedCallShape);
    }
    let world = &sim.map.world;
    if !world.valid_w(child.world_x, child.world_y) {
        return Err(Frame0MerchantGoodLookupError::WorldCellOutOfBounds {
            world_x: child.world_x,
            world_y: child.world_y,
        });
    }
    let random_state = sim.world.random.state();
    let world_index = world.w_index(child.world_x, child.world_y);
    let cell = &world.wdata[world_index];
    let initial_object = cell.down;
    let initial_owner = cell.down_who;
    let mut terminal_object = initial_object;
    let mut terminal_owner = initial_owner;
    let mut links = Vec::new();
    let mut seen = BTreeSet::new();
    while terminal_object >= 0 {
        if !seen.insert((terminal_owner, terminal_object)) {
            return Err(Frame0MerchantGoodLookupError::ObjectChainCycle {
                owner: terminal_owner,
                object: terminal_object,
            });
        }
        let link = canonical_object_link(sim, links.len(), terminal_owner, terminal_object)?;
        terminal_object = link.next_object;
        terminal_owner = link.next_owner;
        links.push(link);
    }

    if terminal_object != GOOD_TERMINAL {
        return Ok(Frame0MerchantGoodLookupFrontier::Resolved(
            resolved_receipt(
                child,
                world_index,
                initial_object,
                initial_owner,
                links,
                terminal_object,
                terminal_owner,
                None,
                None,
                None,
                Frame0MerchantGoodLookupOutcome::NotFound(
                    Frame0MerchantGoodNotFoundReason::TerminalWasNotGood,
                ),
                random_state,
            ),
        ));
    }

    let slot = usize::try_from(terminal_owner).map_err(|_| {
        Frame0MerchantGoodLookupError::MissingGoodSlot {
            slot: terminal_owner,
            logical_length: goods.slots.len(),
        }
    })?;
    let terminal_good =
        goods
            .slots
            .get(slot)
            .ok_or(Frame0MerchantGoodLookupError::MissingGoodSlot {
                slot: terminal_owner,
                logical_length: goods.slots.len(),
            })?;
    if !terminal_good.active() {
        let good = Frame0MerchantGoodSlotRead {
            logical_length: goods.slots.len(),
            slot: terminal_owner,
            flags: terminal_good.node.flags,
            ptype_present: None,
            type_index: None,
        };
        return Ok(Frame0MerchantGoodLookupFrontier::Resolved(
            resolved_receipt(
                child,
                world_index,
                initial_object,
                initial_owner,
                links,
                terminal_object,
                terminal_owner,
                Some(good),
                None,
                None,
                Frame0MerchantGoodLookupOutcome::NotFound(
                    Frame0MerchantGoodNotFoundReason::InactiveGood,
                ),
                random_state,
            ),
        ));
    }
    if !terminal_good.ptype_present {
        return Err(Frame0MerchantGoodLookupError::NullGoodTypePointer {
            slot: terminal_owner,
        });
    }
    let type_index = terminal_good.node.type_index;
    let good = Frame0MerchantGoodSlotRead {
        logical_length: goods.slots.len(),
        slot: terminal_owner,
        flags: terminal_good.node.flags,
        ptype_present: Some(true),
        type_index: Some(type_index),
    };
    if type_index == OIL_GOOD_TYPE {
        return Ok(Frame0MerchantGoodLookupFrontier::Resolved(
            resolved_receipt(
                child,
                world_index,
                initial_object,
                initial_owner,
                links,
                terminal_object,
                terminal_owner,
                Some(good),
                None,
                None,
                Frame0MerchantGoodLookupOutcome::NotFound(
                    Frame0MerchantGoodNotFoundReason::OilGood,
                ),
                random_state,
            ),
        ));
    }

    let prefix = traversal_prefix_digest(
        child,
        world_index,
        initial_object,
        initial_owner,
        &links,
        terminal_object,
        terminal_owner,
        Some(good),
        random_state,
    );
    let mut request = Frame0MerchantTypeAvailRequest {
        request_sha256: [0; 32],
        lookup_request_sha256: child.request_sha256,
        traversal_prefix_sha256: prefix,
        call_va: LEADER_TYPE_AVAIL_VA,
        leader: child.lookup_owner,
        type_index,
        strict: 1,
        good_slot: terminal_owner,
        random_state,
    };
    request.request_sha256 = frame0_merchant_type_avail_request_digest(&request);
    Ok(Frame0MerchantGoodLookupFrontier::NeedsTypeAvail(request))
}

/// Advance one exact Merchant Good lookup against canonical owners without mutation.
pub fn advance_frame0_merchant_good_lookup(
    sim: &Sim,
    parent: &Frame0MerchantSpotRequest,
    member: &CanonicalSetupUnitMemberReceipt,
    child: &Frame0MerchantGoodLookupRequest,
    goods: &OilGoodRuntime,
) -> Result<Frame0MerchantGoodLookupFrontier, Frame0MerchantGoodLookupError> {
    validate_frame0_merchant_good_lookup_request(sim, parent, member, child)?;
    evaluate_prevalidated(sim, child, goods)
}

fn complete_prevalidated(
    sim: &Sim,
    child: &Frame0MerchantGoodLookupRequest,
    goods: &OilGoodRuntime,
    request: &Frame0MerchantTypeAvailRequest,
    capture: &Frame0MerchantTypeAvailCapture,
) -> Result<Frame0MerchantGoodLookupReceipt, Frame0MerchantGoodLookupError> {
    if request.request_sha256 != frame0_merchant_type_avail_request_digest(request) {
        return Err(Frame0MerchantGoodLookupError::InvalidTypeAvailRequestDigest);
    }
    let Frame0MerchantGoodLookupFrontier::NeedsTypeAvail(current) =
        evaluate_prevalidated(sim, child, goods)?
    else {
        return Err(Frame0MerchantGoodLookupError::StaleTypeAvailRequest);
    };
    if current != *request {
        return Err(Frame0MerchantGoodLookupError::StaleTypeAvailRequest);
    }
    if capture.installed_query_input_sha256 == [0; 32] {
        return Err(Frame0MerchantGoodLookupError::MissingTypeAvailAuthorityDigest);
    }
    if capture.request_sha256 != request.request_sha256
        || capture.capture_sha256 != frame0_merchant_type_avail_capture_digest(capture)
    {
        return Err(Frame0MerchantGoodLookupError::InvalidTypeAvailCaptureDigest);
    }

    let world = &sim.map.world;
    let world_index = world.w_index(child.world_x, child.world_y);
    let cell = &world.wdata[world_index];
    let mut terminal_object = cell.down;
    let mut terminal_owner = cell.down_who;
    let mut links = Vec::new();
    while terminal_object >= 0 {
        let link = canonical_object_link(sim, links.len(), terminal_owner, terminal_object)?;
        terminal_object = link.next_object;
        terminal_owner = link.next_owner;
        links.push(link);
    }
    let terminal_good = &goods.slots[terminal_owner as usize];
    let good = Frame0MerchantGoodSlotRead {
        logical_length: goods.slots.len(),
        slot: terminal_owner,
        flags: terminal_good.node.flags,
        ptype_present: Some(true),
        type_index: Some(terminal_good.node.type_index),
    };
    let outcome = if capture.available {
        Frame0MerchantGoodLookupOutcome::FoundType(request.type_index)
    } else {
        Frame0MerchantGoodLookupOutcome::NotFound(Frame0MerchantGoodNotFoundReason::TypeUnavailable)
    };
    Ok(resolved_receipt(
        child,
        world_index,
        cell.down,
        cell.down_who,
        links,
        terminal_object,
        terminal_owner,
        Some(good),
        Some(capture.available),
        Some(*capture),
        outcome,
        sim.world.random.state(),
    ))
}

/// Complete the typed Leader child against an installed, digest-bound answer.
///
/// The entire World/Object/Good prefix is rerun first. A mutation to any reached input makes
/// the supplied request stale before its answer is observed.
#[allow(clippy::too_many_arguments)]
pub fn complete_frame0_merchant_good_lookup(
    sim: &Sim,
    parent: &Frame0MerchantSpotRequest,
    member: &CanonicalSetupUnitMemberReceipt,
    child: &Frame0MerchantGoodLookupRequest,
    goods: &OilGoodRuntime,
    request: &Frame0MerchantTypeAvailRequest,
    capture: &Frame0MerchantTypeAvailCapture,
) -> Result<Frame0MerchantGoodLookupReceipt, Frame0MerchantGoodLookupError> {
    validate_frame0_merchant_good_lookup_request(sim, parent, member, child)?;
    complete_prevalidated(sim, child, goods, request, capture)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::setup_2024_frame0_merchant_search::{
        frame0_merchant_good_lookup_digest, Frame0MerchantCalcGatherScan,
    };
    use don_sim::systems::economy::GoodNode;
    use don_sim::systems::world_oil_goods::{OilGoodSlot, CLOSED_COORD_INTERNAL};

    fn child() -> Frame0MerchantGoodLookupRequest {
        let mut request = Frame0MerchantGoodLookupRequest {
            request_sha256: [0; 32],
            parent_request_sha256: [1; 32],
            setup_composition_digest: [2; 32],
            setup_authority_revision: 7,
            invocation_ordinal: 1,
            actor_who: 0,
            actor_o: 1,
            actor_uid: 3,
            actor_type: 62,
            calc_gather_call_va: 0x0060_9180,
            calc_coord_x: 384,
            calc_coord_y: 384,
            upgrade_level: 0,
            gather_radius: 4,
            cached_good_obj: -1,
            scan: Frame0MerchantCalcGatherScan::OrderedCircle,
            circle_index: 0,
            tile_x: 2,
            tile_y: 2,
            tile_mask: 0x200,
            find_good_at_call_va: OBJECTS_FIND_GOOD_AT_WCOORD_VA,
            world_x: 0,
            world_y: 0,
            lookup_owner: 0,
            lookup_arg4: 0,
            lookup_arg5: 0,
        };
        request.request_sha256 = frame0_merchant_good_lookup_digest(&request);
        request
    }

    fn active_good(type_index: i32) -> OilGoodSlot {
        OilGoodSlot {
            node: GoodNode {
                flags: 1,
                who: u8::MAX,
                o: 0,
                z: CLOSED_COORD_INTERNAL,
                x: CLOSED_COORD_INTERNAL,
                y: CLOSED_COORD_INTERNAL,
                type_index,
                ever_seen: 0,
            },
            ptype_present: true,
            cur_time: 0,
        }
    }

    #[test]
    fn non_good_terminal_resolves_not_found_without_rng() {
        let sim = Sim::new(0x1234_5678, 8);
        let request = child();
        let before = sim.world.random.state();
        let out = evaluate_prevalidated(&sim, &request, &OilGoodRuntime::default()).unwrap();
        let Frame0MerchantGoodLookupFrontier::Resolved(receipt) = out else {
            panic!("empty WData must resolve locally");
        };
        assert_eq!(receipt.terminal_object, -1);
        assert_eq!(receipt.return_value, NOT_FOUND);
        assert_eq!(receipt.rng_draws, 0);
        assert_eq!(receipt.random_state_before, before);
        assert_eq!(receipt.random_state_after, before);
    }

    #[test]
    fn direct_inactive_and_oil_good_apply_filters_in_read_order() {
        let mut sim = Sim::new(9, 8);
        let request = child();
        sim.map.world.wdata[0].down = GOOD_TERMINAL;
        sim.map.world.wdata[0].down_who = 0;
        let mut goods = OilGoodRuntime::default();
        goods.slots.push(OilGoodSlot::default());
        let Frame0MerchantGoodLookupFrontier::Resolved(inactive) =
            evaluate_prevalidated(&sim, &request, &goods).unwrap()
        else {
            panic!("inactive Good must resolve locally");
        };
        assert_eq!(
            inactive.outcome,
            Frame0MerchantGoodLookupOutcome::NotFound(
                Frame0MerchantGoodNotFoundReason::InactiveGood
            )
        );
        assert_eq!(inactive.good.unwrap().ptype_present, None);

        goods.slots[0] = active_good(OIL_GOOD_TYPE);
        let Frame0MerchantGoodLookupFrontier::Resolved(oil) =
            evaluate_prevalidated(&sim, &request, &goods).unwrap()
        else {
            panic!("Oil Good must resolve locally");
        };
        assert_eq!(
            oil.outcome,
            Frame0MerchantGoodLookupOutcome::NotFound(Frame0MerchantGoodNotFoundReason::OilGood)
        );
    }

    #[test]
    fn live_unit_link_is_followed_before_terminal_good() {
        let mut sim = Sim::new(11, 8);
        sim.activate(0);
        let unit = sim.spawn_unit(0, 69, 384, 384, 1).unwrap();
        let row = sim.world.row_of(unit).unwrap();
        let object = sim.world.units.o()[row];
        sim.world.units.down_mut()[row] = GOOD_TERMINAL;
        sim.world.units.down_who_mut()[row] = 0;
        sim.map.world.wdata[0].down = object;
        sim.map.world.wdata[0].down_who = 0;
        let mut goods = OilGoodRuntime::default();
        goods.slots.push(active_good(17));
        let Frame0MerchantGoodLookupFrontier::NeedsTypeAvail(child) =
            evaluate_prevalidated(&sim, &child(), &goods).unwrap()
        else {
            panic!("rare Good must reach type_avail");
        };
        assert_eq!(child.leader, 0);
        assert_eq!(child.type_index, 17);
        assert_eq!(child.strict, 1);
        assert_eq!(child.good_slot, 0);
    }

    #[test]
    fn unit_build_wall_links_use_canonical_registry_identity_in_order() {
        let mut sim = Sim::new(12, 8);
        sim.activate(0);
        let unit = sim.spawn_unit(0, 69, 384, 384, 1).unwrap();
        let unit_row = sim.world.row_of(unit).unwrap();
        let unit_object = sim.world.units.o()[unit_row];

        let mut build = don_sim::systems::production::BuildData::default();
        build.other[0x2c..0x2e].copy_from_slice(&3_000i16.to_le_bytes());
        build.other[0x2e..0x30].copy_from_slice(&0i16.to_le_bytes());
        let build_row = sim.spawn_build(0, build);

        let wall = don_sim::systems::walls::WallState {
            down: GOOD_TERMINAL,
            down_who: 0,
            ..Default::default()
        };
        let wall_row = sim.spawn_wall(0, wall);

        sim.world.units.down_mut()[unit_row] = 2_000;
        sim.world.units.down_who_mut()[unit_row] = 0;
        sim.map.world.wdata[0].down = unit_object;
        sim.map.world.wdata[0].down_who = 0;

        let mut goods = OilGoodRuntime::default();
        goods.slots.push(active_good(17));
        let Frame0MerchantGoodLookupFrontier::NeedsTypeAvail(_) =
            evaluate_prevalidated(&sim, &child(), &goods).unwrap()
        else {
            panic!("rare Good must reach type_avail");
        };

        let mut object = unit_object;
        let mut owner = 0;
        let mut observed = Vec::new();
        while object >= 0 {
            let link = canonical_object_link(&sim, observed.len(), owner, object).unwrap();
            object = link.next_object;
            owner = link.next_owner;
            observed.push(link);
        }
        assert_eq!(observed.len(), 3);
        assert!(matches!(
            observed[0].identity,
            Frame0MerchantObjectIdentity::Unit { row, .. } if row == unit_row
        ));
        assert_eq!(
            observed[1].identity,
            Frame0MerchantObjectIdentity::Build {
                row: build_row,
                uid: sim.builds[build_row].uid,
            }
        );
        assert_eq!(
            observed[2].identity,
            Frame0MerchantObjectIdentity::Wall {
                row: wall_row,
                uid: sim.walls[wall_row].uid,
            }
        );
        assert_eq!((object, owner), (GOOD_TERMINAL, 0));
    }

    #[test]
    fn linked_object_mutation_changes_the_typed_child() {
        let mut sim = Sim::new(13, 8);
        sim.activate(0);
        let unit = sim.spawn_unit(0, 69, 384, 384, 1).unwrap();
        let row = sim.world.row_of(unit).unwrap();
        sim.map.world.wdata[0].down = sim.world.units.o()[row];
        sim.map.world.wdata[0].down_who = 0;
        sim.world.units.down_mut()[row] = GOOD_TERMINAL;
        sim.world.units.down_who_mut()[row] = 0;
        let mut goods = OilGoodRuntime::default();
        goods.slots.push(active_good(17));
        let Frame0MerchantGoodLookupFrontier::NeedsTypeAvail(first) =
            evaluate_prevalidated(&sim, &child(), &goods).unwrap()
        else {
            panic!("rare Good must reach type_avail");
        };
        sim.world.units.down_who_mut()[row] = 1;
        goods.slots.push(active_good(17));
        let Frame0MerchantGoodLookupFrontier::NeedsTypeAvail(second) =
            evaluate_prevalidated(&sim, &child(), &goods).unwrap()
        else {
            panic!("mutated rare Good must still reach type_avail");
        };
        assert_ne!(first.request_sha256, second.request_sha256);
        assert_ne!(
            first.traversal_prefix_sha256,
            second.traversal_prefix_sha256
        );
    }

    #[test]
    fn cycle_is_refused_instead_of_hanging() {
        let mut sim = Sim::new(15, 8);
        sim.activate(0);
        let unit = sim.spawn_unit(0, 69, 384, 384, 1).unwrap();
        let row = sim.world.row_of(unit).unwrap();
        let object = sim.world.units.o()[row];
        sim.map.world.wdata[0].down = object;
        sim.map.world.wdata[0].down_who = 0;
        sim.world.units.down_mut()[row] = object;
        sim.world.units.down_who_mut()[row] = 0;
        assert_eq!(
            evaluate_prevalidated(&sim, &child(), &OilGoodRuntime::default()),
            Err(Frame0MerchantGoodLookupError::ObjectChainCycle { owner: 0, object })
        );
    }

    #[test]
    fn active_good_with_null_type_pointer_fails_closed() {
        let mut sim = Sim::new(16, 8);
        sim.map.world.wdata[0].down = GOOD_TERMINAL;
        sim.map.world.wdata[0].down_who = 0;
        let mut goods = OilGoodRuntime::default();
        let mut good = active_good(17);
        good.ptype_present = false;
        goods.slots.push(good);
        assert_eq!(
            evaluate_prevalidated(&sim, &child(), &goods),
            Err(Frame0MerchantGoodLookupError::NullGoodTypePointer { slot: 0 })
        );
    }

    #[test]
    fn typed_capture_digest_binds_available_bit() {
        let mut capture = Frame0MerchantTypeAvailCapture {
            capture_sha256: [0; 32],
            request_sha256: [3; 32],
            installed_query_input_sha256: [4; 32],
            available: true,
        };
        capture.capture_sha256 = frame0_merchant_type_avail_capture_digest(&capture);
        let valid = capture.capture_sha256;
        capture.available = false;
        assert_ne!(valid, frame0_merchant_type_avail_capture_digest(&capture));
    }

    #[test]
    fn typed_capture_resolves_found_or_not_found_and_stales_on_good_mutation() {
        let mut sim = Sim::new(17, 8);
        sim.map.world.wdata[0].down = GOOD_TERMINAL;
        sim.map.world.wdata[0].down_who = 0;
        let mut goods = OilGoodRuntime::default();
        goods.slots.push(active_good(17));
        let lookup = child();
        let Frame0MerchantGoodLookupFrontier::NeedsTypeAvail(request) =
            evaluate_prevalidated(&sim, &lookup, &goods).unwrap()
        else {
            panic!("rare Good must reach type_avail");
        };

        let mut capture = Frame0MerchantTypeAvailCapture {
            capture_sha256: [0; 32],
            request_sha256: request.request_sha256,
            installed_query_input_sha256: [0x91; 32],
            available: true,
        };
        capture.capture_sha256 = frame0_merchant_type_avail_capture_digest(&capture);
        let found = complete_prevalidated(&sim, &lookup, &goods, &request, &capture).unwrap();
        assert_eq!(
            found.outcome,
            Frame0MerchantGoodLookupOutcome::FoundType(17)
        );
        assert_eq!(found.return_value, 17);
        assert_eq!(found.type_avail_capture, Some(capture));

        capture.available = false;
        capture.capture_sha256 = frame0_merchant_type_avail_capture_digest(&capture);
        let absent = complete_prevalidated(&sim, &lookup, &goods, &request, &capture).unwrap();
        assert_eq!(
            absent.outcome,
            Frame0MerchantGoodLookupOutcome::NotFound(
                Frame0MerchantGoodNotFoundReason::TypeUnavailable
            )
        );
        assert_eq!(absent.return_value, NOT_FOUND);

        goods.slots[0].node.type_index = 18;
        assert_eq!(
            complete_prevalidated(&sim, &lookup, &goods, &request, &capture),
            Err(Frame0MerchantGoodLookupError::StaleTypeAvailRequest)
        );
    }
}
