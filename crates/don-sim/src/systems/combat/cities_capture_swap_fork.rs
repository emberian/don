//! `Cities::capture_city` center-swap fork, `0x00733749..0x00733D67`.
//!
//! This module consumes the transaction-prefix receipt and converges the successful
//! and failed center-swap arms.  It retains the two SimpleArray orders and every
//! mutation before the common continuation at `0x00733D67`.  The nested retail
//! routines remain identity-bound host tails; this is not a replacement for their
//! bodies.

use super::build_check_capture::CityKey;
use super::cities_capture_prefix::{CitiesCapturePrefixContinuation, CitiesCapturePrefixReceipt};
use super::damage_world::ObjectKey;

pub const CITIES_CAPTURE_SWAP_FORK_START: u32 = 0x0073_3749;
pub const CITIES_CAPTURE_SWAP_FORK_END: u32 = 0x0073_3D67;
pub const CITIES_CAPTURE_SWAP_FORK_SIZE: u32 =
    CITIES_CAPTURE_SWAP_FORK_END - CITIES_CAPTURE_SWAP_FORK_START;
pub const CITIES_CAPTURE_RESIDUAL_SIZE: u32 =
    super::cities_capture_prefix::CITIES_CAPTURE_CITY_END - CITIES_CAPTURE_SWAP_FORK_END;

pub const CAPTURE_MEMBER_SKIP_FLAG: u32 = 0x0000_2000;
pub const FAILED_CAPTURE_MEMBER_SKIP_FLAG: u32 = 0x0000_0010;
pub const CAPTURED_MEMBER_PLUNDER: i32 = 25;
pub const FARM_TYPE: i32 = 0x1A1;
pub const GRANARY_TYPE: i32 = 0x1A7;
pub const LAKOTA_TRIBE_BONUS_INDEX: i32 = 0x13;
pub const MUTUAL_ALLY_DIPLOMACY: i32 = 2;
pub const PEASANT_TYPE: i32 = 0x32;
pub const PEASANT_KOREAN_TYPE: i32 = 0x33;
pub const SCHOLAR_TYPE: i32 = 0x34;
pub const SCHOLAR_KOREAN_TYPE: i32 = 0x35;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OrderedCapturedMember {
    /// Identity in the pre-capture owner's `BuildData::city_down` chain.
    pub old_member: ObjectKey,
    /// The next identity was already followed by the host snapshot.  The vector order
    /// must therefore be the exact retail `city_down` walk order.
    pub chain_ordinal: u32,
    /// `BuildTypeData::build_flags` at `+0x2C0`.
    pub build_type_flags: u32,
    /// Exact `ObjectData::is(type, 0)` results.  Retail asks FARM before GRANARY.
    pub is_farm: bool,
    pub is_granary: bool,
    /// Virtual active-build predicate (`flags & 4` on the known Build vtable fast path).
    pub is_active_build: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureMemberChainMode {
    SuccessfulCenterSwap,
    FailedCenterSwap,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptureMemberChainRequest {
    pub old_center: ObjectKey,
    pub old_owner: u8,
    pub mode: CaptureMemberChainMode,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptureMemberChainReceipt {
    pub request: CaptureMemberChainRequest,
    /// Attests that the host began at `old_center.get_build()->city_down` and followed
    /// each old-owner Build's next `city_down` identity to `-1` without reordering.
    pub walked_exact_city_down_chain: bool,
    pub members: Vec<OrderedCapturedMember>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CityRecordCaptureRequest {
    pub old_city: CityKey,
    pub new_city: CityKey,
    pub old_center: ObjectKey,
    pub new_center: ObjectKey,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CityCaptureEcxResidue(pub i32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CityRecordCaptureReceipt {
    pub request: CityRecordCaptureRequest,
    /// `City::capture` `0x00736C40` completed synchronously.
    pub capture_applied: bool,
    /// The following five-argument `Armies::update_city` call pushes ECX as its fifth
    /// argument.  That callee never reads `[EBP+0x18]`; retain, but do not interpret, it.
    pub ignored_ecx_residue: CityCaptureEcxResidue,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArmyCityReferenceUpdateRequest {
    pub old_center: ObjectKey,
    pub new_center: ObjectKey,
    pub ignored_ecx_residue: CityCaptureEcxResidue,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArmyCityReferenceUpdateReceipt {
    pub request: ArmyCityReferenceUpdateRequest,
    /// `Armies::update_city` `0x006F2D70` completed its ordered all-player scan.
    pub update_applied: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SwapCapturedMemberRequest {
    pub old_member: ObjectKey,
    pub new_owner: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwapCapturedMemberResult {
    Failed,
    Succeeded { new_member: ObjectKey },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SwapCapturedMemberReceipt {
    pub request: SwapCapturedMemberRequest,
    pub result: SwapCapturedMemberResult,
    pub ownership_applied: bool,
    pub object_copy_applied: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActivateCapturedMemberRequest {
    pub new_member: ObjectKey,
    pub first: i32,
    pub second: i32,
    pub third: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FindCapturedCityBuildingsRequest {
    pub new_city: CityKey,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FindCapturedCityBuildingsReceipt {
    pub request: FindCapturedCityBuildingsRequest,
    /// `City::find_buildings` `0x007384C0` completed synchronously.
    pub rebuild_applied: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptureCircleScanRequest {
    pub new_city: CityKey,
    pub new_center: ObjectKey,
    /// Prefix `(min(old_radius, 64) + 3) / 4` result selecting the global circle tables.
    pub radius_ring: i32,
    /// Post-`City::capture` new-owner radius.  Retail clamps it only above 64.
    pub new_owner_center_radius: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NearbyUnitFindCityReceipt {
    pub unit: ObjectKey,
    pub x: i32,
    pub y: i32,
    /// `SearchIndexBH` raw value pushed by retail.
    pub search_index_bh: i32,
    pub new_owner: u8,
    /// Retail pushes Y a second time as the fifth argument.
    pub repeated_y: i32,
    /// Arguments six through nine are all zero.
    pub zero_tail: [i32; 4],
    pub result: Option<CityKey>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OrderedNearbyUnit {
    /// Circle-table entry order, then tile `ObjectData::down` chain order.
    pub circle_ordinal: u32,
    pub chain_ordinal: u32,
    pub unit: ObjectKey,
    pub is_valid_unit: bool,
    pub is_on_map: bool,
    pub type_index: i32,
    /// Exact `vector_dist(abs(dx), abs(dy))` result in retail coordinates.
    pub distance_from_new_center: i32,
    /// Exact nine-argument `ObjectsData::find_city` call/return when locality is
    /// required.  The receipt itself may be absent only on the one-city bypass.
    pub find_city: Option<NearbyUnitFindCityReceipt>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptureCircleScanReceipt {
    pub request: CaptureCircleScanRequest,
    /// Attests the exact `CIRCLE_COUNT`, signed X-offset, and signed Y-offset table walk.
    pub used_retail_circle_tables: bool,
    /// Includes only in-bounds tiles, but preserves their retail circle ordinals.
    pub units: Vec<OrderedNearbyUnit>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SwapNearbyUnitRequest {
    pub old_unit: ObjectKey,
    pub new_owner: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SwapNearbyUnitReceipt {
    pub request: SwapNearbyUnitRequest,
    /// The return value of virtual `SubObject::swap_team`; negative means no mutation.
    pub new_unit: Option<ObjectKey>,
    pub ownership_applied: bool,
    pub object_copy_applied: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SuccessfulCenterForkInput {
    pub old_owner_has_lakota_bonus: bool,
    pub old_leader_who: u8,
    pub old_to_new_diplomacy: i32,
    pub new_to_old_diplomacy: i32,
    pub old_owner_city_num: i32,
    pub new_owner_center_radius: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CitiesCaptureSwapForkPlan {
    pub prefix: CitiesCapturePrefixReceipt,
    pub successful: Option<SuccessfulCenterForkInput>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CitiesCaptureSwapForkPlanError {
    MissingSuccessInput,
    UnexpectedSuccessInput,
    PrefixSuccessStateIncomplete,
    PrefixFailureStateMismatch,
    PrefixOldCenterStateMismatch,
}

/// Validate prefix-owned branch state before the first new host mutation.
pub fn plan_cities_capture_swap_fork(
    prefix: CitiesCapturePrefixReceipt,
    successful: Option<SuccessfulCenterForkInput>,
) -> Result<CitiesCaptureSwapForkPlan, CitiesCaptureSwapForkPlanError> {
    if prefix.old_city_objects.len() != 1
        || prefix.old_city_objects[0].who != prefix.request.old_owner
        || prefix.old_city_objects[0].o < 0
    {
        return Err(CitiesCaptureSwapForkPlanError::PrefixOldCenterStateMismatch);
    }
    match prefix.continuation {
        CitiesCapturePrefixContinuation::CenterSwapSucceeded0x00733755 => {
            if successful.is_none() {
                return Err(CitiesCaptureSwapForkPlanError::MissingSuccessInput);
            }
            if !matches!(
                prefix.new_city,
                Some(city) if city.who == prefix.request.new_owner && city.city >= 0
            ) || prefix.new_city_objects.len() != 1
                || prefix.new_city_objects[0].who != prefix.request.new_owner
                || prefix.new_city_objects[0].o < 0
            {
                return Err(CitiesCaptureSwapForkPlanError::PrefixSuccessStateIncomplete);
            }
        }
        CitiesCapturePrefixContinuation::CenterSwapFailed0x00733ce2 => {
            if successful.is_some() {
                return Err(CitiesCaptureSwapForkPlanError::UnexpectedSuccessInput);
            }
            if prefix.new_city.is_some()
                || !prefix.new_city_objects.is_empty()
                || prefix.plunder_accumulator != 0
            {
                return Err(CitiesCaptureSwapForkPlanError::PrefixFailureStateMismatch);
            }
        }
    }
    Ok(CitiesCaptureSwapForkPlan { prefix, successful })
}

pub trait CitiesCaptureSwapForkWorld {
    fn read_capture_member_chain(
        &mut self,
        request: CaptureMemberChainRequest,
    ) -> Option<CaptureMemberChainReceipt>;
    fn capture_city_record(
        &mut self,
        request: CityRecordCaptureRequest,
    ) -> Option<CityRecordCaptureReceipt>;
    fn update_army_city_references(
        &mut self,
        request: ArmyCityReferenceUpdateRequest,
    ) -> Option<ArmyCityReferenceUpdateReceipt>;
    fn swap_captured_member(
        &mut self,
        request: SwapCapturedMemberRequest,
    ) -> Option<SwapCapturedMemberReceipt>;
    fn activate_captured_member(&mut self, request: ActivateCapturedMemberRequest);
    fn find_captured_city_buildings(
        &mut self,
        request: FindCapturedCityBuildingsRequest,
    ) -> Option<FindCapturedCityBuildingsReceipt>;
    fn scan_capture_circle(
        &mut self,
        request: CaptureCircleScanRequest,
    ) -> Option<CaptureCircleScanReceipt>;
    fn swap_nearby_unit(&mut self, request: SwapNearbyUnitRequest)
        -> Option<SwapNearbyUnitReceipt>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CitiesCaptureSwapForkMutation {
    ReadMemberChain(CaptureMemberChainReceipt),
    CaptureCityRecord(CityRecordCaptureReceipt),
    UpdateArmyCityReferences(ArmyCityReferenceUpdateReceipt),
    AppendOldObject(ObjectKey),
    SwapMember(SwapCapturedMemberReceipt),
    ActivateMember(ActivateCapturedMemberRequest),
    AppendNewObject(ObjectKey),
    AddPlunder(i32),
    FindBuildings(FindCapturedCityBuildingsReceipt),
    SwapNearbyUnit(SwapNearbyUnitReceipt),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CitiesCaptureSwapForkContinuation {
    Converged0x00733d67,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CitiesCaptureSwapForkReceipt {
    pub old_city_objects: Vec<ObjectKey>,
    pub new_city_objects: Vec<ObjectKey>,
    pub new_city: Option<CityKey>,
    pub plunder_accumulator: i32,
    pub continuation: CitiesCaptureSwapForkContinuation,
    pub mutations: Vec<CitiesCaptureSwapForkMutation>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CitiesCaptureSwapForkApplyError {
    MissingMemberChainReceipt,
    MemberChainReceiptMismatch,
    MemberChainWalkNotAttested,
    InvalidMemberOwner,
    InvalidMemberIdentity,
    MemberChainOrderMismatch,
    MissingCityCaptureReceipt,
    CityCaptureReceiptMismatch,
    CityCaptureEffectsIncomplete,
    MissingArmyUpdateReceipt,
    ArmyUpdateReceiptMismatch,
    ArmyUpdateEffectsIncomplete,
    MissingMemberSwapReceipt,
    MemberSwapReceiptMismatch,
    FailedMemberSwapEffectsPresent,
    MemberSwapEffectsIncomplete,
    MemberSwapIdentityMismatch,
    MissingFindBuildingsReceipt,
    FindBuildingsReceiptMismatch,
    FindBuildingsEffectsIncomplete,
    MissingCircleScanReceipt,
    CircleScanReceiptMismatch,
    CircleTablesNotAttested,
    CircleOrderMismatch,
    CircleUnitIdentityMismatch,
    FindCityQueryMismatch,
    MissingNearbyUnitSwapReceipt,
    NearbyUnitSwapReceiptMismatch,
    FailedNearbyUnitSwapEffectsPresent,
    NearbyUnitSwapEffectsIncomplete,
    NearbyUnitSwapIdentityMismatch,
}

#[inline]
pub const fn same_or_mutual_allies(
    old_leader_who: u8,
    new_owner: u8,
    old_to_new: i32,
    new_to_old: i32,
) -> bool {
    old_leader_who == new_owner
        || (old_to_new == MUTUAL_ALLY_DIPLOMACY && new_to_old == MUTUAL_ALLY_DIPLOMACY)
}

#[inline]
pub const fn nearby_unit_type_gate(type_index: i32, same_or_mutual: bool) -> bool {
    same_or_mutual
        || matches!(
            type_index,
            PEASANT_TYPE | PEASANT_KOREAN_TYPE | SCHOLAR_TYPE | SCHOLAR_KOREAN_TYPE
        )
}

/// `min(radius,64) * 3 * 64`, preserving retail wrapping and its lack of a low clamp.
#[inline]
pub const fn nearby_unit_radius_threshold(radius: i32) -> i32 {
    let radius = if radius > 64 { 64 } else { radius };
    radius.wrapping_mul(3).wrapping_shl(6)
}

fn validate_member_swap(
    receipt: SwapCapturedMemberReceipt,
    request: SwapCapturedMemberRequest,
) -> Result<Option<ObjectKey>, CitiesCaptureSwapForkApplyError> {
    if receipt.request != request {
        return Err(CitiesCaptureSwapForkApplyError::MemberSwapReceiptMismatch);
    }
    match receipt.result {
        SwapCapturedMemberResult::Failed => {
            if receipt.ownership_applied || receipt.object_copy_applied {
                return Err(CitiesCaptureSwapForkApplyError::FailedMemberSwapEffectsPresent);
            }
            Ok(None)
        }
        SwapCapturedMemberResult::Succeeded { new_member } => {
            if !receipt.ownership_applied || !receipt.object_copy_applied {
                return Err(CitiesCaptureSwapForkApplyError::MemberSwapEffectsIncomplete);
            }
            if new_member.who != request.new_owner || new_member.o < 0 {
                return Err(CitiesCaptureSwapForkApplyError::MemberSwapIdentityMismatch);
            }
            Ok(Some(new_member))
        }
    }
}

fn read_member_chain<W: CitiesCaptureSwapForkWorld + ?Sized>(
    world: &mut W,
    request: CaptureMemberChainRequest,
) -> Result<CaptureMemberChainReceipt, CitiesCaptureSwapForkApplyError> {
    let receipt = world
        .read_capture_member_chain(request)
        .ok_or(CitiesCaptureSwapForkApplyError::MissingMemberChainReceipt)?;
    if receipt.request != request {
        return Err(CitiesCaptureSwapForkApplyError::MemberChainReceiptMismatch);
    }
    if !receipt.walked_exact_city_down_chain {
        return Err(CitiesCaptureSwapForkApplyError::MemberChainWalkNotAttested);
    }
    for (ordinal, member) in receipt.members.iter().enumerate() {
        if member.old_member.who != request.old_owner {
            return Err(CitiesCaptureSwapForkApplyError::InvalidMemberOwner);
        }
        if member.old_member.o < 0 {
            return Err(CitiesCaptureSwapForkApplyError::InvalidMemberIdentity);
        }
        if member.chain_ordinal != ordinal as u32 {
            return Err(CitiesCaptureSwapForkApplyError::MemberChainOrderMismatch);
        }
    }
    Ok(receipt)
}

/// Execute the recovered fork in retail instruction order.
pub fn apply_cities_capture_swap_fork<W: CitiesCaptureSwapForkWorld + ?Sized>(
    plan: CitiesCaptureSwapForkPlan,
    world: &mut W,
) -> Result<CitiesCaptureSwapForkReceipt, CitiesCaptureSwapForkApplyError> {
    let mut old_city_objects = plan.prefix.old_city_objects.clone();
    let mut new_city_objects = plan.prefix.new_city_objects.clone();
    let mut plunder = plan.prefix.plunder_accumulator;
    let mut mutations = Vec::new();

    if plan.prefix.continuation == CitiesCapturePrefixContinuation::CenterSwapFailed0x00733ce2 {
        let chain_request = CaptureMemberChainRequest {
            old_center: plan.prefix.old_city_objects[0],
            old_owner: plan.prefix.request.old_owner,
            mode: CaptureMemberChainMode::FailedCenterSwap,
        };
        let chain = read_member_chain(world, chain_request)?;
        let members = chain.members.clone();
        mutations.push(CitiesCaptureSwapForkMutation::ReadMemberChain(chain));
        for member in members {
            if member.build_type_flags & FAILED_CAPTURE_MEMBER_SKIP_FLAG == 0 {
                old_city_objects.push(member.old_member);
                mutations.push(CitiesCaptureSwapForkMutation::AppendOldObject(
                    member.old_member,
                ));
                plunder = plunder.wrapping_add(CAPTURED_MEMBER_PLUNDER);
                mutations.push(CitiesCaptureSwapForkMutation::AddPlunder(
                    CAPTURED_MEMBER_PLUNDER,
                ));
            }
        }
        return Ok(CitiesCaptureSwapForkReceipt {
            old_city_objects,
            new_city_objects,
            new_city: None,
            plunder_accumulator: plunder,
            continuation: CitiesCaptureSwapForkContinuation::Converged0x00733d67,
            mutations,
        });
    }

    let successful = plan.successful.expect("success continuation was validated");
    let old_center = plan.prefix.old_city_objects[0];
    let new_center = plan.prefix.new_city_objects[0];
    let new_city = plan
        .prefix
        .new_city
        .expect("success continuation was validated");
    let capture_request = CityRecordCaptureRequest {
        old_city: plan.prefix.request.old_city,
        new_city,
        old_center,
        new_center,
    };
    let capture = world
        .capture_city_record(capture_request)
        .ok_or(CitiesCaptureSwapForkApplyError::MissingCityCaptureReceipt)?;
    if capture.request != capture_request {
        return Err(CitiesCaptureSwapForkApplyError::CityCaptureReceiptMismatch);
    }
    if !capture.capture_applied {
        return Err(CitiesCaptureSwapForkApplyError::CityCaptureEffectsIncomplete);
    }
    mutations.push(CitiesCaptureSwapForkMutation::CaptureCityRecord(capture));

    let army_request = ArmyCityReferenceUpdateRequest {
        old_center,
        new_center,
        ignored_ecx_residue: capture.ignored_ecx_residue,
    };
    let army = world
        .update_army_city_references(army_request)
        .ok_or(CitiesCaptureSwapForkApplyError::MissingArmyUpdateReceipt)?;
    if army.request != army_request {
        return Err(CitiesCaptureSwapForkApplyError::ArmyUpdateReceiptMismatch);
    }
    if !army.update_applied {
        return Err(CitiesCaptureSwapForkApplyError::ArmyUpdateEffectsIncomplete);
    }
    mutations.push(CitiesCaptureSwapForkMutation::UpdateArmyCityReferences(
        army,
    ));

    let chain_request = CaptureMemberChainRequest {
        old_center,
        old_owner: plan.prefix.request.old_owner,
        mode: CaptureMemberChainMode::SuccessfulCenterSwap,
    };
    let chain = read_member_chain(world, chain_request)?;
    let members = chain.members.clone();
    mutations.push(CitiesCaptureSwapForkMutation::ReadMemberChain(chain));
    for member in members {
        if member.build_type_flags & CAPTURE_MEMBER_SKIP_FLAG != 0 {
            continue;
        }
        old_city_objects.push(member.old_member);
        mutations.push(CitiesCaptureSwapForkMutation::AppendOldObject(
            member.old_member,
        ));

        let lakota_resource_building =
            (member.is_farm || member.is_granary) && successful.old_owner_has_lakota_bonus;
        if lakota_resource_building || !member.is_active_build {
            continue;
        }

        let swap_request = SwapCapturedMemberRequest {
            old_member: member.old_member,
            new_owner: plan.prefix.request.new_owner,
        };
        let swap = world
            .swap_captured_member(swap_request)
            .ok_or(CitiesCaptureSwapForkApplyError::MissingMemberSwapReceipt)?;
        let new_member = validate_member_swap(swap, swap_request)?;
        mutations.push(CitiesCaptureSwapForkMutation::SwapMember(swap));
        let Some(new_member) = new_member else {
            continue;
        };

        let activate = ActivateCapturedMemberRequest {
            new_member,
            first: 1,
            second: 1,
            third: 0,
        };
        world.activate_captured_member(activate);
        mutations.push(CitiesCaptureSwapForkMutation::ActivateMember(activate));
        new_city_objects.push(new_member);
        mutations.push(CitiesCaptureSwapForkMutation::AppendNewObject(new_member));
        plunder = plunder.wrapping_add(CAPTURED_MEMBER_PLUNDER);
        mutations.push(CitiesCaptureSwapForkMutation::AddPlunder(
            CAPTURED_MEMBER_PLUNDER,
        ));
    }

    let find_request = FindCapturedCityBuildingsRequest { new_city };
    let find = world
        .find_captured_city_buildings(find_request)
        .ok_or(CitiesCaptureSwapForkApplyError::MissingFindBuildingsReceipt)?;
    if find.request != find_request {
        return Err(CitiesCaptureSwapForkApplyError::FindBuildingsReceiptMismatch);
    }
    if !find.rebuild_applied {
        return Err(CitiesCaptureSwapForkApplyError::FindBuildingsEffectsIncomplete);
    }
    mutations.push(CitiesCaptureSwapForkMutation::FindBuildings(find));

    let same_or_mutual = same_or_mutual_allies(
        successful.old_leader_who,
        plan.prefix.request.new_owner,
        successful.old_to_new_diplomacy,
        successful.new_to_old_diplomacy,
    );
    if same_or_mutual {
        let scan_request = CaptureCircleScanRequest {
            new_city,
            new_center,
            radius_ring: plan.prefix.capture_radius_ring,
            new_owner_center_radius: successful.new_owner_center_radius,
        };
        let scan = world
            .scan_capture_circle(scan_request)
            .ok_or(CitiesCaptureSwapForkApplyError::MissingCircleScanReceipt)?;
        if scan.request != scan_request {
            return Err(CitiesCaptureSwapForkApplyError::CircleScanReceiptMismatch);
        }
        if !scan.used_retail_circle_tables {
            return Err(CitiesCaptureSwapForkApplyError::CircleTablesNotAttested);
        }
        let mut last_order = None;
        for candidate in scan.units {
            let order = (candidate.circle_ordinal, candidate.chain_ordinal);
            if last_order.is_some_and(|last| order <= last) {
                return Err(CitiesCaptureSwapForkApplyError::CircleOrderMismatch);
            }
            last_order = Some(order);
            if candidate.unit.o < 0 {
                return Err(CitiesCaptureSwapForkApplyError::CircleUnitIdentityMismatch);
            }
            if candidate.unit.who != plan.prefix.request.old_owner
                || !candidate.is_valid_unit
                || !candidate.is_on_map
                || !nearby_unit_type_gate(candidate.type_index, same_or_mutual)
            {
                continue;
            }

            let one_city_bypass = same_or_mutual && successful.old_owner_city_num <= 1;
            if !one_city_bypass {
                let query = candidate
                    .find_city
                    .ok_or(CitiesCaptureSwapForkApplyError::FindCityQueryMismatch)?;
                if query.unit != candidate.unit
                    || query.search_index_bh != 1
                    || query.new_owner != plan.prefix.request.new_owner
                    || query.repeated_y != query.y
                    || query.zero_tail != [0; 4]
                {
                    return Err(CitiesCaptureSwapForkApplyError::FindCityQueryMismatch);
                }
                if candidate.distance_from_new_center
                    > nearby_unit_radius_threshold(successful.new_owner_center_radius)
                    || query.result != Some(new_city)
                {
                    continue;
                }
            }

            // Retail repeats the diplomacy gate immediately before this virtual call.
            if !same_or_mutual {
                continue;
            }
            let request = SwapNearbyUnitRequest {
                old_unit: candidate.unit,
                new_owner: plan.prefix.request.new_owner,
            };
            let receipt = world
                .swap_nearby_unit(request)
                .ok_or(CitiesCaptureSwapForkApplyError::MissingNearbyUnitSwapReceipt)?;
            if receipt.request != request {
                return Err(CitiesCaptureSwapForkApplyError::NearbyUnitSwapReceiptMismatch);
            }
            if let Some(new_unit) = receipt.new_unit {
                if !receipt.ownership_applied || !receipt.object_copy_applied {
                    return Err(CitiesCaptureSwapForkApplyError::NearbyUnitSwapEffectsIncomplete);
                }
                if new_unit.who != request.new_owner || new_unit.o < 0 {
                    return Err(CitiesCaptureSwapForkApplyError::NearbyUnitSwapIdentityMismatch);
                }
            } else if receipt.ownership_applied || receipt.object_copy_applied {
                return Err(CitiesCaptureSwapForkApplyError::FailedNearbyUnitSwapEffectsPresent);
            }
            mutations.push(CitiesCaptureSwapForkMutation::SwapNearbyUnit(receipt));
        }
    }

    Ok(CitiesCaptureSwapForkReceipt {
        old_city_objects,
        new_city_objects,
        new_city: Some(new_city),
        plunder_accumulator: plunder,
        continuation: CitiesCaptureSwapForkContinuation::Converged0x00733d67,
        mutations,
    })
}
