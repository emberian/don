//! Exact prefix of the golden frame-zero Merchant `Unit::detect_unit_collision` child.
//!
//! The preceding transaction proves the call shape `(tx*192,ty*192,1,1,0,0,0)` and the
//! Type-62 land/non-siege/non-hero/non-supply predicates. Retail then checks the Unit path
//! stack, `UnitData+0xb2 safe`, and the target/current UCoord cells in that order. The golden
//! setup request already requires an empty path, while `safe` lives in the canonical Unit SoA.
//!
//! A nonzero `safe` or an unchanged UCoord returns zero without mutation. Otherwise the first
//! remaining child is `CollCheck::collide_here` `0x00682540`. That call joins the Unit footprint
//! with mutable CollBlock memoization (or pathfinder scratch in its overlay arm), so this module
//! emits it as a typed request rather than manufacturing an empty bitmap answer. No RNG is
//! consumed before or by this prefix.

#![forbid(unsafe_code)]

use std::fmt;

use don_sim::systems::movement::ucell_of;
use don_sim::systems::world_oil_goods::OilGoodRuntime;
use don_sim::tick::Sim;

use crate::setup_2024_frame0_merchant_good_lookup::{
    advance_frame0_merchant_search_after_good_steps, frame0_merchant_collision_request_digest,
    Frame0MerchantCollisionRequest, Frame0MerchantGoodLookupError,
    Frame0MerchantSearchContinuationFrontier, Frame0MerchantSearchGoodStep,
};
use crate::setup_2024_frame0_merchant_unpack::Frame0MerchantSpotRequest;
use crate::setup_unit_member_authority::CanonicalSetupUnitMemberReceipt;
use crate::world_owner_frontier::sha256;

pub const COLL_CHECK_COLLIDE_HERE_VA: u32 = 0x0068_2540;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame0MerchantCollisionClearReason {
    SafeRetryDelay,
    SameUCoord,
}

/// Complete read-only prefix through either a local zero return or the bitmap child.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame0MerchantCollisionPrefixReceipt {
    pub receipt_sha256: [u8; 32],
    pub collision_request_sha256: [u8; 32],
    pub actor_row: usize,
    pub actor_who: u8,
    pub actor_o: i16,
    pub path_len: i32,
    /// Retail does not dereference the top record when the path length is zero.
    pub path_top_flags_read: Option<u8>,
    pub safe: i8,
    /// Absent when `safe != 0` returns before coordinate conversion.
    pub target_ucell: Option<(i32, i32)>,
    pub current_ucell: Option<(i32, i32)>,
    pub clear_reason: Option<Frame0MerchantCollisionClearReason>,
    pub return_value: Option<i32>,
    pub random_state_before: i32,
    pub random_state_after: i32,
    pub rng_draws: u32,
}

/// Exact first mixed footprint/CollBlock child reached by the golden probe.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame0MerchantCollideHereRequest {
    pub request_sha256: [u8; 32],
    pub collision_request_sha256: [u8; 32],
    pub collision_prefix_receipt_sha256: [u8; 32],
    pub setup_composition_digest: [u8; 32],
    pub setup_authority_revision: u64,
    pub invocation_ordinal: usize,
    pub actor_who: u8,
    pub actor_o: i16,
    pub actor_uid: u16,
    pub call_va: u32,
    pub target_ucell_x: i32,
    pub target_ucell_y: i32,
    pub current_ucell_x: i32,
    pub current_ucell_y: i32,
    pub new_block_radius: i32,
    pub output_x_requested: bool,
    pub output_y_requested: bool,
    pub scratch_overlay: bool,
    pub random_state: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame0MerchantCollisionPrefixFrontier {
    Clear(Frame0MerchantCollisionPrefixReceipt),
    NeedsCollideHere {
        prefix: Frame0MerchantCollisionPrefixReceipt,
        request: Frame0MerchantCollideHereRequest,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame0MerchantCollisionPrefixError {
    Search(Frame0MerchantGoodLookupError),
    InvalidCollisionRequestDigest,
    StaleCollisionRequest,
    NonEmptyGoldenPath { len: i32 },
    StaleActor,
}

impl fmt::Display for Frame0MerchantCollisionPrefixError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "2024 frame-zero Merchant collision prefix refused: {self:?}"
        )
    }
}

impl std::error::Error for Frame0MerchantCollisionPrefixError {}

impl From<Frame0MerchantGoodLookupError> for Frame0MerchantCollisionPrefixError {
    fn from(value: Frame0MerchantGoodLookupError) -> Self {
        Self::Search(value)
    }
}

/// Stable digest over every read made by the local collision prefix.
pub fn frame0_merchant_collision_prefix_receipt_digest(
    receipt: &Frame0MerchantCollisionPrefixReceipt,
) -> [u8; 32] {
    let mut image = b"don-2024-frame0-merchant-collision-prefix-v1".to_vec();
    image.extend_from_slice(&receipt.collision_request_sha256);
    image.extend_from_slice(&(receipt.actor_row as u64).to_le_bytes());
    image.push(receipt.actor_who);
    image.extend_from_slice(&receipt.actor_o.to_le_bytes());
    image.extend_from_slice(&receipt.path_len.to_le_bytes());
    match receipt.path_top_flags_read {
        None => image.push(0),
        Some(flags) => {
            image.push(1);
            image.push(flags);
        }
    }
    image.push(receipt.safe as u8);
    for value in [receipt.target_ucell, receipt.current_ucell] {
        match value {
            None => image.push(0),
            Some((x, y)) => {
                image.push(1);
                image.extend_from_slice(&x.to_le_bytes());
                image.extend_from_slice(&y.to_le_bytes());
            }
        }
    }
    image.push(match receipt.clear_reason {
        None => 0,
        Some(Frame0MerchantCollisionClearReason::SafeRetryDelay) => 1,
        Some(Frame0MerchantCollisionClearReason::SameUCoord) => 2,
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

/// Stable digest over the exact `CollCheck::collide_here` child and call shape.
pub fn frame0_merchant_collide_here_request_digest(
    request: &Frame0MerchantCollideHereRequest,
) -> [u8; 32] {
    let mut image = b"don-2024-frame0-merchant-collide-here-request-v1".to_vec();
    image.extend_from_slice(&request.collision_request_sha256);
    image.extend_from_slice(&request.collision_prefix_receipt_sha256);
    image.extend_from_slice(&request.setup_composition_digest);
    image.extend_from_slice(&request.setup_authority_revision.to_le_bytes());
    image.extend_from_slice(&(request.invocation_ordinal as u64).to_le_bytes());
    image.push(request.actor_who);
    image.extend_from_slice(&request.actor_o.to_le_bytes());
    image.extend_from_slice(&request.actor_uid.to_le_bytes());
    for value in [
        request.call_va as i32,
        request.target_ucell_x,
        request.target_ucell_y,
        request.current_ucell_x,
        request.current_ucell_y,
        request.new_block_radius,
        request.random_state,
    ] {
        image.extend_from_slice(&value.to_le_bytes());
    }
    image.push(u8::from(request.output_x_requested));
    image.push(u8::from(request.output_y_requested));
    image.push(u8::from(request.scratch_overlay));
    sha256(&image)
}

fn prefix_receipt(
    request: &Frame0MerchantCollisionRequest,
    actor_row: usize,
    path_len: i32,
    safe: i8,
    target_ucell: Option<(i32, i32)>,
    current_ucell: Option<(i32, i32)>,
    clear_reason: Option<Frame0MerchantCollisionClearReason>,
    random_state: i32,
) -> Frame0MerchantCollisionPrefixReceipt {
    let mut receipt = Frame0MerchantCollisionPrefixReceipt {
        receipt_sha256: [0; 32],
        collision_request_sha256: request.request_sha256,
        actor_row,
        actor_who: request.actor_who,
        actor_o: request.actor_o,
        path_len,
        path_top_flags_read: None,
        safe,
        target_ucell,
        current_ucell,
        clear_reason,
        return_value: clear_reason.map(|_| 0),
        random_state_before: random_state,
        random_state_after: random_state,
        rng_draws: 0,
    };
    receipt.receipt_sha256 = frame0_merchant_collision_prefix_receipt_digest(&receipt);
    receipt
}

/// Re-evaluate the complete ordered Merchant search and advance its collision child through
/// the last source-owned scalar/grid prefix.
#[allow(clippy::too_many_arguments)]
pub fn advance_frame0_merchant_collision_prefix(
    sim: &Sim,
    parent: &Frame0MerchantSpotRequest,
    member: &CanonicalSetupUnitMemberReceipt,
    goods: &OilGoodRuntime,
    steps: &[Frame0MerchantSearchGoodStep],
    request: &Frame0MerchantCollisionRequest,
) -> Result<Frame0MerchantCollisionPrefixFrontier, Frame0MerchantCollisionPrefixError> {
    if request.request_sha256 != frame0_merchant_collision_request_digest(request) {
        return Err(Frame0MerchantCollisionPrefixError::InvalidCollisionRequestDigest);
    }
    let current =
        advance_frame0_merchant_search_after_good_steps(sim, parent, member, goods, steps)?;
    let Frame0MerchantSearchContinuationFrontier::NeedsCollision(current) = current else {
        return Err(Frame0MerchantCollisionPrefixError::StaleCollisionRequest);
    };
    if current != *request {
        return Err(Frame0MerchantCollisionPrefixError::StaleCollisionRequest);
    }

    let path_len = parent.actor.path.len();
    if path_len != 0 {
        return Err(Frame0MerchantCollisionPrefixError::NonEmptyGoldenPath { len: path_len });
    }
    let row = parent.actor.row;
    let safe = *sim
        .world
        .units
        .safe()
        .get(row)
        .ok_or(Frame0MerchantCollisionPrefixError::StaleActor)?;
    let random_state = sim.world.random.state();
    if safe != 0 {
        return Ok(Frame0MerchantCollisionPrefixFrontier::Clear(
            prefix_receipt(
                request,
                row,
                path_len,
                safe,
                None,
                None,
                Some(Frame0MerchantCollisionClearReason::SafeRetryDelay),
                random_state,
            ),
        ));
    }

    let target_ucell = (ucell_of(request.coord_x), ucell_of(request.coord_y));
    let current_ucell = (ucell_of(parent.actor.x), ucell_of(parent.actor.y));
    if target_ucell == current_ucell {
        return Ok(Frame0MerchantCollisionPrefixFrontier::Clear(
            prefix_receipt(
                request,
                row,
                path_len,
                safe,
                Some(target_ucell),
                Some(current_ucell),
                Some(Frame0MerchantCollisionClearReason::SameUCoord),
                random_state,
            ),
        ));
    }

    let new_block_radius = member.type_facts.new_block_radius;
    let prefix = prefix_receipt(
        request,
        row,
        path_len,
        safe,
        Some(target_ucell),
        Some(current_ucell),
        None,
        random_state,
    );
    let mut child = Frame0MerchantCollideHereRequest {
        request_sha256: [0; 32],
        collision_request_sha256: request.request_sha256,
        collision_prefix_receipt_sha256: prefix.receipt_sha256,
        setup_composition_digest: request.setup_composition_digest,
        setup_authority_revision: request.setup_authority_revision,
        invocation_ordinal: request.invocation_ordinal,
        actor_who: request.actor_who,
        actor_o: request.actor_o,
        actor_uid: request.actor_uid,
        call_va: COLL_CHECK_COLLIDE_HERE_VA,
        target_ucell_x: target_ucell.0,
        target_ucell_y: target_ucell.1,
        current_ucell_x: current_ucell.0,
        current_ucell_y: current_ucell.1,
        new_block_radius,
        output_x_requested: true,
        output_y_requested: true,
        scratch_overlay: false,
        random_state,
    };
    child.request_sha256 = frame0_merchant_collide_here_request_digest(&child);
    Ok(Frame0MerchantCollisionPrefixFrontier::NeedsCollideHere {
        prefix,
        request: child,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collision_request() -> Frame0MerchantCollisionRequest {
        let mut request = Frame0MerchantCollisionRequest {
            request_sha256: [0; 32],
            parent_request_sha256: [1; 32],
            search_trace_sha256: [2; 32],
            setup_composition_digest: [3; 32],
            setup_authority_revision: 4,
            invocation_ordinal: 1,
            actor_who: 0,
            actor_o: 2,
            actor_uid: 9,
            actor_type: 62,
            candidate_index: 3,
            tile_x: 11,
            tile_y: 17,
            good_merchant_spot_call_va: 0x0060_68a0,
            gather_receipt_sha256: [4; 32],
            invalid_loc:
                crate::setup_2024_frame0_merchant_good_lookup::Frame0MerchantInvalidLocReceipt {
                    receipt_sha256: [5; 32],
                    call_va: 0x0060_7c30,
                    candidate_index: 3,
                    tile_x: 11,
                    tile_y: 17,
                    orders_empty: true,
                    domain: 0,
                    world_x: 2,
                    world_y: 4,
                    world_flags: 0,
                    tile_mask: 0x200,
                    cliff_predicate_read: Some(false),
                    unit_masks: 0x0008_0000,
                    return_value: 0,
                    random_state_before: 19,
                    random_state_after: 19,
                    rng_draws: 0,
                },
            call_va: 0x0061_7060,
            coord_x: 11 * 192,
            coord_y: 17 * 192,
            footprint_x: 1,
            footprint_y: 1,
            arg5: 0,
            arg6: 0,
            arg7: 0,
            type_domain: 0,
            type_unit_flags: 0x0120_1882,
            type_unit_flags2: 6,
            type_is_siege: false,
            unit_is_hero: false,
            unit_is_supply: false,
            random_state: 19,
        };
        request.request_sha256 = frame0_merchant_collision_request_digest(&request);
        request
    }

    #[test]
    fn safe_return_does_not_claim_coordinate_reads() {
        let request = collision_request();
        let receipt = prefix_receipt(
            &request,
            2,
            0,
            1,
            None,
            None,
            Some(Frame0MerchantCollisionClearReason::SafeRetryDelay),
            19,
        );
        assert_eq!(receipt.path_top_flags_read, None);
        assert_eq!(receipt.target_ucell, None);
        assert_eq!(receipt.current_ucell, None);
        assert_eq!(receipt.return_value, Some(0));
        assert_eq!(receipt.rng_draws, 0);
    }

    #[test]
    fn collide_here_digest_binds_footprint_and_scratch_arm() {
        let request = collision_request();
        let prefix = prefix_receipt(&request, 2, 0, 0, Some((44, 68)), Some((43, 68)), None, 19);
        let mut child = Frame0MerchantCollideHereRequest {
            request_sha256: [0; 32],
            collision_request_sha256: request.request_sha256,
            collision_prefix_receipt_sha256: prefix.receipt_sha256,
            setup_composition_digest: [3; 32],
            setup_authority_revision: 4,
            invocation_ordinal: 1,
            actor_who: 0,
            actor_o: 2,
            actor_uid: 9,
            call_va: COLL_CHECK_COLLIDE_HERE_VA,
            target_ucell_x: 44,
            target_ucell_y: 68,
            current_ucell_x: 43,
            current_ucell_y: 68,
            new_block_radius: 1,
            output_x_requested: true,
            output_y_requested: true,
            scratch_overlay: false,
            random_state: 19,
        };
        child.request_sha256 = frame0_merchant_collide_here_request_digest(&child);
        let digest = child.request_sha256;
        child.new_block_radius = 2;
        assert_ne!(digest, frame0_merchant_collide_here_request_digest(&child));
        child.new_block_radius = 1;
        child.scratch_overlay = true;
        assert_ne!(digest, frame0_merchant_collide_here_request_digest(&child));
    }
}
