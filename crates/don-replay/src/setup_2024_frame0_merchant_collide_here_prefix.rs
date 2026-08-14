//! Exact mutable prefix of the golden non-overlay `CollCheck::collide_here` call.
//!
//! Retail first resolves up to four World collision-block slots with `fill_slots`, then calls
//! mutating `BitMask<768>::empty` on every live slot in ordinal order. `empty` memoizes its
//! answer in `CollBlock+0x08 flags`; those flags are deliberately not part of the World
//! checksum walk, but they are real execution state and are recorded here before and after.
//! The canonical `LiveCollisionRuntime::check` slots are updated at the same boundary.
//!
//! If every slot is dead/empty, `collide_here` returns zero locally. Otherwise this module
//! stops at `0x006825fd`, immediately before the non-overlay arm resolves the caller Unit and
//! chooses the adjacent-edge or heterogeneous footprint walk. No bitmap result, object row,
//! or scratch overlay is guessed. This prefix consumes no RNG.

#![forbid(unsafe_code)]

use std::fmt;

use don_sim::systems::collision::{block_empty, CollCheck};
use don_sim::systems::map_terrain::CollBlock;
use don_sim::systems::world_oil_goods::OilGoodRuntime;
use don_sim::tick::Sim;

use crate::setup_2024_frame0_merchant_collision_prefix::{
    advance_frame0_merchant_collision_prefix, frame0_merchant_collide_here_request_digest,
    Frame0MerchantCollideHereRequest, Frame0MerchantCollisionPrefixError,
    Frame0MerchantCollisionPrefixFrontier,
};
use crate::setup_2024_frame0_merchant_good_lookup::{
    Frame0MerchantCollisionRequest, Frame0MerchantSearchGoodStep,
};
use crate::setup_2024_frame0_merchant_unpack::Frame0MerchantSpotRequest;
use crate::setup_unit_member_authority::CanonicalSetupUnitMemberReceipt;
use crate::world_owner_frontier::sha256;

pub const COLLIDE_HERE_FOOTPRINT_FRONTIER_VA: u32 = 0x0068_25fd;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame0MerchantCollCheckImage {
    pub valid: [bool; 4],
    pub slots: [Option<usize>; 4],
    pub scratch_present: [bool; 4],
    pub queries: u64,
}

impl From<&CollCheck> for Frame0MerchantCollCheckImage {
    fn from(value: &CollCheck) -> Self {
        Self {
            valid: value.valid,
            slots: value.slot,
            scratch_present: value.scratch.map(|block| block.is_some()),
            queries: value.queries,
        }
    }
}

/// One reached `BitMask<768>::empty` call and its exact memoized flag transition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame0MerchantCollBlockMemo {
    pub slot_ordinal: usize,
    pub world_index: usize,
    pub flags_before: i32,
    pub flags_after: i32,
    /// `BitMask::empty` reads `size` only when the flag cache is indeterminate.
    pub scanned_size: Option<i32>,
    /// Digest of precisely `max(size, 0)` reached payload bytes, not the unread tail.
    pub scanned_payload_sha256: Option<[u8; 32]>,
    pub empty: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame0MerchantCollideHerePrefixOutcome {
    SizeZeroClear,
    AllSlotsEmptyClear,
    NeedsFootprint,
}

/// Applied prefix receipt. Every CollBlock memo transition and CollCheck scratch write has
/// already been committed to the canonical owners when this receipt is returned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame0MerchantCollideHerePrefixReceipt {
    pub receipt_sha256: [u8; 32],
    pub collide_here_request_sha256: [u8; 32],
    pub call_va: u32,
    pub check_before: Frame0MerchantCollCheckImage,
    pub check_after: Frame0MerchantCollCheckImage,
    pub block_memos: Vec<Frame0MerchantCollBlockMemo>,
    /// `local_30[0..4]`; absent on the earlier `size == 0` return.
    pub dead_slots: Option<[bool; 4]>,
    pub single_low_slot: Option<bool>,
    pub low_block_x: Option<i32>,
    pub low_block_y: Option<i32>,
    pub outcome: Frame0MerchantCollideHerePrefixOutcome,
    pub return_value: Option<i32>,
    pub random_state_before: i32,
    pub random_state_after: i32,
    pub rng_draws: u32,
}

/// Exact first child after nonempty collision blocks survive memoization.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame0MerchantFootprintRequest {
    pub request_sha256: [u8; 32],
    pub collide_here_request_sha256: [u8; 32],
    pub prefix_receipt_sha256: [u8; 32],
    pub setup_composition_digest: [u8; 32],
    pub setup_authority_revision: u64,
    pub invocation_ordinal: usize,
    pub actor_who: u8,
    pub actor_o: i16,
    pub actor_uid: u16,
    pub next_va: u32,
    pub target_ucell_x: i32,
    pub target_ucell_y: i32,
    pub new_block_radius: i32,
    pub low_block_x: i32,
    pub low_block_y: i32,
    pub dead_slots: [bool; 4],
    pub single_low_slot: bool,
    pub non_overlay_actor_lookup: bool,
    pub output_x_requested: bool,
    pub output_y_requested: bool,
    pub random_state: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame0MerchantCollideHerePrefixFrontier {
    Clear(Frame0MerchantCollideHerePrefixReceipt),
    NeedsFootprint {
        prefix: Frame0MerchantCollideHerePrefixReceipt,
        request: Frame0MerchantFootprintRequest,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame0MerchantCollideHerePrefixError {
    Collision(Frame0MerchantCollisionPrefixError),
    InvalidRequestDigest,
    StaleRequest,
    UnsupportedScratchOverlay,
    MissingCollBlock { world_index: usize },
    InvalidCollBlockSize { world_index: usize, size: i32 },
}

impl fmt::Display for Frame0MerchantCollideHerePrefixError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "2024 Merchant collide_here prefix refused: {self:?}")
    }
}

impl std::error::Error for Frame0MerchantCollideHerePrefixError {}

impl From<Frame0MerchantCollisionPrefixError> for Frame0MerchantCollideHerePrefixError {
    fn from(value: Frame0MerchantCollisionPrefixError) -> Self {
        Self::Collision(value)
    }
}

fn memoize_block(
    slot_ordinal: usize,
    world_index: usize,
    before: CollBlock,
) -> Result<(Frame0MerchantCollBlockMemo, CollBlock), Frame0MerchantCollideHerePrefixError> {
    let scans = before.flags & 1 == 0 && before.flags != 0;
    if scans && before.size > before.ptr.len() as i32 {
        return Err(Frame0MerchantCollideHerePrefixError::InvalidCollBlockSize {
            world_index,
            size: before.size,
        });
    }
    let scanned_size = scans.then_some(before.size);
    let scanned_payload_sha256 = scans.then(|| {
        let len = before.size.max(0) as usize;
        sha256(&before.ptr[..len])
    });
    let mut after = before;
    let empty = block_empty(&mut after);
    Ok((
        Frame0MerchantCollBlockMemo {
            slot_ordinal,
            world_index,
            flags_before: before.flags,
            flags_after: after.flags,
            scanned_size,
            scanned_payload_sha256,
            empty,
        },
        after,
    ))
}

fn append_check(image: &mut Vec<u8>, check: Frame0MerchantCollCheckImage) {
    for valid in check.valid {
        image.push(u8::from(valid));
    }
    for slot in check.slots {
        match slot {
            None => image.push(0),
            Some(index) => {
                image.push(1);
                image.extend_from_slice(&(index as u64).to_le_bytes());
            }
        }
    }
    for present in check.scratch_present {
        image.push(u8::from(present));
    }
    image.extend_from_slice(&check.queries.to_le_bytes());
}

fn append_memo(image: &mut Vec<u8>, memo: Frame0MerchantCollBlockMemo) {
    image.extend_from_slice(&(memo.slot_ordinal as u64).to_le_bytes());
    image.extend_from_slice(&(memo.world_index as u64).to_le_bytes());
    image.extend_from_slice(&memo.flags_before.to_le_bytes());
    image.extend_from_slice(&memo.flags_after.to_le_bytes());
    match memo.scanned_size {
        None => image.push(0),
        Some(size) => {
            image.push(1);
            image.extend_from_slice(&size.to_le_bytes());
        }
    }
    match memo.scanned_payload_sha256 {
        None => image.push(0),
        Some(digest) => {
            image.push(1);
            image.extend_from_slice(&digest);
        }
    }
    image.push(u8::from(memo.empty));
}

pub fn frame0_merchant_collide_here_prefix_receipt_digest(
    receipt: &Frame0MerchantCollideHerePrefixReceipt,
) -> [u8; 32] {
    let mut image = b"don-2024-frame0-merchant-collide-here-prefix-v1".to_vec();
    image.extend_from_slice(&receipt.collide_here_request_sha256);
    image.extend_from_slice(&receipt.call_va.to_le_bytes());
    append_check(&mut image, receipt.check_before);
    append_check(&mut image, receipt.check_after);
    image.extend_from_slice(&(receipt.block_memos.len() as u64).to_le_bytes());
    for memo in &receipt.block_memos {
        append_memo(&mut image, *memo);
    }
    match receipt.dead_slots {
        None => image.push(0),
        Some(dead) => {
            image.push(1);
            for value in dead {
                image.push(u8::from(value));
            }
        }
    }
    match receipt.single_low_slot {
        None => image.push(0),
        Some(value) => {
            image.push(1);
            image.push(u8::from(value));
        }
    }
    for value in [receipt.low_block_x, receipt.low_block_y] {
        match value {
            None => image.push(0),
            Some(value) => {
                image.push(1);
                image.extend_from_slice(&value.to_le_bytes());
            }
        }
    }
    image.push(match receipt.outcome {
        Frame0MerchantCollideHerePrefixOutcome::SizeZeroClear => 0,
        Frame0MerchantCollideHerePrefixOutcome::AllSlotsEmptyClear => 1,
        Frame0MerchantCollideHerePrefixOutcome::NeedsFootprint => 2,
    });
    match receipt.return_value {
        None => image.push(0),
        Some(value) => {
            image.push(1);
            image.extend_from_slice(&value.to_le_bytes());
        }
    }
    image.extend_from_slice(&receipt.random_state_before.to_le_bytes());
    image.extend_from_slice(&receipt.random_state_after.to_le_bytes());
    image.extend_from_slice(&receipt.rng_draws.to_le_bytes());
    sha256(&image)
}

pub fn frame0_merchant_footprint_request_digest(
    request: &Frame0MerchantFootprintRequest,
) -> [u8; 32] {
    let mut image = b"don-2024-frame0-merchant-footprint-request-v1".to_vec();
    image.extend_from_slice(&request.collide_here_request_sha256);
    image.extend_from_slice(&request.prefix_receipt_sha256);
    image.extend_from_slice(&request.setup_composition_digest);
    image.extend_from_slice(&request.setup_authority_revision.to_le_bytes());
    image.extend_from_slice(&(request.invocation_ordinal as u64).to_le_bytes());
    image.push(request.actor_who);
    image.extend_from_slice(&request.actor_o.to_le_bytes());
    image.extend_from_slice(&request.actor_uid.to_le_bytes());
    for value in [
        request.next_va as i32,
        request.target_ucell_x,
        request.target_ucell_y,
        request.new_block_radius,
        request.low_block_x,
        request.low_block_y,
        request.random_state,
    ] {
        image.extend_from_slice(&value.to_le_bytes());
    }
    for dead in request.dead_slots {
        image.push(u8::from(dead));
    }
    image.push(u8::from(request.single_low_slot));
    image.push(u8::from(request.non_overlay_actor_lookup));
    image.push(u8::from(request.output_x_requested));
    image.push(u8::from(request.output_y_requested));
    sha256(&image)
}

/// Apply the exact slot-fill and mutable emptiness-memo prefix to the canonical collision
/// owners. All fallible validation and block discovery precedes the first store.
#[allow(clippy::too_many_arguments)]
pub fn advance_frame0_merchant_collide_here_prefix(
    sim: &mut Sim,
    parent: &Frame0MerchantSpotRequest,
    member: &CanonicalSetupUnitMemberReceipt,
    goods: &OilGoodRuntime,
    steps: &[Frame0MerchantSearchGoodStep],
    collision_request: &Frame0MerchantCollisionRequest,
    request: &Frame0MerchantCollideHereRequest,
) -> Result<Frame0MerchantCollideHerePrefixFrontier, Frame0MerchantCollideHerePrefixError> {
    if request.request_sha256 != frame0_merchant_collide_here_request_digest(request) {
        return Err(Frame0MerchantCollideHerePrefixError::InvalidRequestDigest);
    }
    if request.scratch_overlay {
        return Err(Frame0MerchantCollideHerePrefixError::UnsupportedScratchOverlay);
    }
    let current = advance_frame0_merchant_collision_prefix(
        sim,
        parent,
        member,
        goods,
        steps,
        collision_request,
    )?;
    let Frame0MerchantCollisionPrefixFrontier::NeedsCollideHere {
        prefix,
        request: current,
    } = current
    else {
        return Err(Frame0MerchantCollideHerePrefixError::StaleRequest);
    };
    if current != *request || prefix.receipt_sha256 != request.collision_prefix_receipt_sha256 {
        return Err(Frame0MerchantCollideHerePrefixError::StaleRequest);
    }

    let check_before = sim.movement_collision.check;
    let mut check_after = check_before;
    check_after.queries = check_after.queries.wrapping_add(1);
    let random_state = sim.world.random.state();
    let size = request.new_block_radius;

    let (memos, after_blocks, dead_slots, single, low_x, low_y, outcome) = if size == 0 {
        (
            Vec::new(),
            Vec::new(),
            None,
            None,
            None,
            None,
            Frame0MerchantCollideHerePrefixOutcome::SizeZeroClear,
        )
    } else {
        check_after.fill_slots(
            &sim.map.world,
            request.target_ucell_x,
            request.target_ucell_y,
            size,
            false,
        );
        let mut memos = Vec::new();
        let mut after_blocks = Vec::new();
        let mut dead = [true; 4];
        for ordinal in 0..4 {
            if !check_after.valid[ordinal] {
                continue;
            }
            let Some(world_index) = check_after.slot[ordinal] else {
                continue;
            };
            let before = sim.map.world.wdata[world_index]
                .block
                .as_deref()
                .copied()
                .ok_or(Frame0MerchantCollideHerePrefixError::MissingCollBlock { world_index })?;
            let (memo, after) = memoize_block(ordinal, world_index, before)?;
            dead[ordinal] = memo.empty;
            memos.push(memo);
            after_blocks.push((world_index, after.flags));
        }
        let single = dead[1] && dead[2] && dead[3];
        let all_empty = single && dead[0];
        (
            memos,
            after_blocks,
            Some(dead),
            Some(single),
            Some(request.target_ucell_x.wrapping_sub(size) >> 4),
            Some(request.target_ucell_y.wrapping_sub(size) >> 4),
            if all_empty {
                Frame0MerchantCollideHerePrefixOutcome::AllSlotsEmptyClear
            } else {
                Frame0MerchantCollideHerePrefixOutcome::NeedsFootprint
            },
        )
    };

    let return_value = match outcome {
        Frame0MerchantCollideHerePrefixOutcome::SizeZeroClear
        | Frame0MerchantCollideHerePrefixOutcome::AllSlotsEmptyClear => Some(0),
        Frame0MerchantCollideHerePrefixOutcome::NeedsFootprint => None,
    };
    let mut receipt = Frame0MerchantCollideHerePrefixReceipt {
        receipt_sha256: [0; 32],
        collide_here_request_sha256: request.request_sha256,
        call_va: request.call_va,
        check_before: (&check_before).into(),
        check_after: (&check_after).into(),
        block_memos: memos,
        dead_slots,
        single_low_slot: single,
        low_block_x: low_x,
        low_block_y: low_y,
        outcome,
        return_value,
        random_state_before: random_state,
        random_state_after: random_state,
        rng_draws: 0,
    };
    receipt.receipt_sha256 = frame0_merchant_collide_here_prefix_receipt_digest(&receipt);

    // Commit only after the whole current-owner prefix has been validated and planned.
    sim.movement_collision.check = check_after;
    for (world_index, flags) in after_blocks {
        sim.map.world.wdata[world_index]
            .block
            .as_deref_mut()
            .expect("preflight retained the exact block")
            .flags = flags;
    }

    if outcome != Frame0MerchantCollideHerePrefixOutcome::NeedsFootprint {
        return Ok(Frame0MerchantCollideHerePrefixFrontier::Clear(receipt));
    }
    let dead_slots = receipt
        .dead_slots
        .expect("footprint outcome has evaluated slots");
    let mut child = Frame0MerchantFootprintRequest {
        request_sha256: [0; 32],
        collide_here_request_sha256: request.request_sha256,
        prefix_receipt_sha256: receipt.receipt_sha256,
        setup_composition_digest: request.setup_composition_digest,
        setup_authority_revision: request.setup_authority_revision,
        invocation_ordinal: request.invocation_ordinal,
        actor_who: request.actor_who,
        actor_o: request.actor_o,
        actor_uid: request.actor_uid,
        next_va: COLLIDE_HERE_FOOTPRINT_FRONTIER_VA,
        target_ucell_x: request.target_ucell_x,
        target_ucell_y: request.target_ucell_y,
        new_block_radius: size,
        low_block_x: low_x.expect("nonzero size computes low block"),
        low_block_y: low_y.expect("nonzero size computes low block"),
        dead_slots,
        single_low_slot: single.expect("nonzero size computes single mode"),
        non_overlay_actor_lookup: true,
        output_x_requested: request.output_x_requested,
        output_y_requested: request.output_y_requested,
        random_state,
    };
    child.request_sha256 = frame0_merchant_footprint_request_digest(&child);
    Ok(Frame0MerchantCollideHerePrefixFrontier::NeedsFootprint {
        prefix: receipt,
        request: child,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitmask_empty_memoization_records_empty_and_nonempty_transitions() {
        let mut empty = CollBlock::default();
        empty.flags = 2;
        let (empty_memo, empty_after) = memoize_block(0, 9, empty).unwrap();
        assert!(empty_memo.empty);
        assert_eq!((empty_memo.flags_before, empty_memo.flags_after), (2, 1));
        assert_eq!(empty_memo.scanned_size, Some(96));
        assert!(empty_memo.scanned_payload_sha256.is_some());
        assert_eq!(empty_after.flags, 1);

        let mut occupied = CollBlock::default();
        occupied.flags = 2;
        occupied.ptr[0] = 1;
        let (occupied_memo, occupied_after) = memoize_block(1, 10, occupied).unwrap();
        assert!(!occupied_memo.empty);
        assert_eq!(
            (occupied_memo.flags_before, occupied_memo.flags_after),
            (2, 0)
        );
        assert_eq!(occupied_after.flags, 0);
    }

    #[test]
    fn memo_digest_binds_only_payload_bytes_reached_by_indeterminate_flags() {
        let mut block = CollBlock::default();
        block.flags = 1;
        let (cached, _) = memoize_block(0, 9, block).unwrap();
        assert_eq!(cached.scanned_size, None);
        assert_eq!(cached.scanned_payload_sha256, None);

        block.flags = 2;
        let (memo, _) = memoize_block(0, 9, block).unwrap();
        block.ptr[17] = 1;
        let (mutant, _) = memoize_block(0, 9, block).unwrap();
        assert_ne!(memo.scanned_payload_sha256, mutant.scanned_payload_sha256);
        assert_ne!(memo.empty, mutant.empty);
    }
}
