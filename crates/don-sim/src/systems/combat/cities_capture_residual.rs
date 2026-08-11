//! Complete residual of `Cities::capture_city`, `0x0073432D..0x007352BE`.
//!
//! This instruction-bounded transaction begins at the old-owner refund test and owns
//! every remaining driver effect through the returned new-city index. Nested engine
//! calls (`City::close`, `Leader::{lost,recapture}_capital`, object close/disband, and
//! presentation allocation) remain typed, synchronous host receipts: their call sites
//! and arguments are part of this body, but their separately named bodies are not.

use super::build_check_capture::CityKey;
use super::cities_capture_local_award_notification::{
    CitiesCaptureLocalAwardNotificationContinuation, CitiesCaptureLocalAwardNotificationReceipt,
};
use super::cities_capture_plunder_award::{
    CitiesCapturePlunderAwardContinuation, CitiesCapturePlunderAwardReceipt,
};
use super::cities_capture_plunder_gate::{
    CitiesCapturePlunderGateContinuation, CitiesCapturePlunderGateReceipt,
};
use super::cities_capture_prefix::CITIES_CAPTURE_CITY_END;
use super::damage_world::ObjectKey;

pub const CITIES_CAPTURE_RESIDUAL_START: u32 = 0x0073_432D;
pub const CITIES_CAPTURE_RESIDUAL_END: u32 = CITIES_CAPTURE_CITY_END;
pub const CITIES_CAPTURE_RESIDUAL_SIZE: u32 =
    CITIES_CAPTURE_RESIDUAL_END - CITIES_CAPTURE_RESIDUAL_START;
pub const CITIES_CAPTURE_RESIDUAL_INSTRUCTION_COUNT: u32 = 1_071;
pub const CITIES_CAPTURE_RESIDUAL_CALL_COUNT: u32 = 116;

pub const RESOURCE_TYPE_FIRST: i32 = 0;
pub const RESOURCE_TYPE_END: i32 = 6;
pub const KNOWLEDGE_RESOURCE_TYPE: i32 = 3;
pub const TYPE_AVAIL_MODE: i32 = 1;
pub const MINIMUM_BUCKET_SENTINEL: i32 = 999_999;
pub const CAPITAL_CITY_FLAG: u16 = 0x0010;
pub const OBJECT_CLOSED_FLAG: u8 = 0x20;
pub const CAPTURE_CLOSE_TYPE_0: i32 = 0x1A1;
pub const CAPTURE_CLOSE_TYPE_1: i32 = 0x1A7;
pub const CAPTURE_CLOSE_TRIBE_BONUS: i32 = 0x13;
pub const FORBIDDEN_CITY_TYPE: i32 = 0x213;

pub const RESOURCE_AMOUNT_TEMPLATE_OFFSET: u32 = 0x1554;
pub const RESOURCE_CITY_TEMPLATE_OFFSET: u32 = 0x1568;
pub const CAPITAL_REFUND_AMOUNT_TEMPLATE_OFFSET: u32 = 0x152C;
pub const CAPITAL_REFUND_CITY_TEMPLATE_OFFSET: u32 = 0x1540;
pub const NEW_OWNER_CAPTURE_MESSAGE_OFFSET: u32 = 0x157C;
pub const NEW_OWNER_CAPTURE_BUBBLE_OFFSET: u32 = 0x1590;
pub const OLD_OWNER_CAPTURE_MESSAGE_OFFSET: u32 = 0x15A4;
pub const OLD_OWNER_CAPTURE_BUBBLE_OFFSET: u32 = 0x15B8;
pub const ALLIED_NEW_OWNER_BUBBLE_OFFSET: u32 = 0x15CC;
pub const NEW_OWNER_NOTICE_INTERNAL_BYTE_OFFSET: u32 = 0x2828;
pub const OLD_OWNER_NOTICE_INTERNAL_BYTE_OFFSET: u32 = 0x283C;
pub const NEW_OWNER_CAPTURE_SOUND: i32 = 0x7C;
pub const OLD_OWNER_CAPTURE_SOUND: i32 = 0x86;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CitiesCaptureResidualPrior {
    /// The local new-owner award cone ended at `0x0073432D`.
    LocalAward(CitiesCaptureLocalAwardNotificationReceipt),
    /// Either the direct refund entry or alternate ordinary-plunder entry.
    PlunderAward(CitiesCapturePlunderAwardReceipt),
    /// The cooldown/protected-city edge enters the common notification at `0x00734A3C`.
    SkipPlunder(CitiesCapturePlunderGateReceipt),
}

impl CitiesCaptureResidualPrior {
    fn gate(&self) -> &CitiesCapturePlunderGateReceipt {
        match self {
            Self::LocalAward(receipt) => &receipt.prior.prior,
            Self::PlunderAward(receipt) => &receipt.prior,
            Self::SkipPlunder(receipt) => receipt,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CitiesCaptureResidualEntry {
    OldOwnerRefund0x0073432d {
        refund: i32,
        notification_raised: bool,
    },
    AlternatePlunder0x00734547,
    CommonCaptureNotification0x00734a3c,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CitiesCaptureResidualPlan {
    pub prior: CitiesCaptureResidualPrior,
    pub entry: CitiesCaptureResidualEntry,
    pub old_city: CityKey,
    pub new_city: CityKey,
    pub old_owner: u8,
    pub new_owner: u8,
    pub old_city_objects: Vec<ObjectKey>,
    pub new_city_objects: Vec<ObjectKey>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CitiesCaptureResidualPlanError {
    PriorContinuationMismatch,
    NotificationStateMismatch,
    MissingRefund,
    MissingNewCity,
    OwnerMismatch,
    MissingOldCenter,
    MissingNewCenter,
}

pub fn plan_cities_capture_residual(
    prior: CitiesCaptureResidualPrior,
) -> Result<CitiesCaptureResidualPlan, CitiesCaptureResidualPlanError> {
    let entry = match &prior {
        CitiesCaptureResidualPrior::LocalAward(receipt) => {
            if receipt.continuation
                != CitiesCaptureLocalAwardNotificationContinuation::OldOwnerRefund0x0073432d
            {
                return Err(CitiesCaptureResidualPlanError::PriorContinuationMismatch);
            }
            if !receipt.notification_raised {
                return Err(CitiesCaptureResidualPlanError::NotificationStateMismatch);
            }
            CitiesCaptureResidualEntry::OldOwnerRefund0x0073432d {
                refund: receipt.old_owner_refund,
                notification_raised: receipt.notification_raised,
            }
        }
        CitiesCaptureResidualPrior::PlunderAward(receipt) => match receipt.continuation {
            CitiesCapturePlunderAwardContinuation::OldOwnerRefund0x0073432d => {
                if receipt.prior.notification_raised {
                    return Err(CitiesCaptureResidualPlanError::NotificationStateMismatch);
                }
                CitiesCaptureResidualEntry::OldOwnerRefund0x0073432d {
                    refund: receipt
                        .old_owner_refund
                        .ok_or(CitiesCaptureResidualPlanError::MissingRefund)?,
                    notification_raised: receipt.prior.notification_raised,
                }
            }
            CitiesCapturePlunderAwardContinuation::AlternatePlunder0x00734547 => {
                if receipt.prior.notification_raised {
                    return Err(CitiesCaptureResidualPlanError::NotificationStateMismatch);
                }
                CitiesCaptureResidualEntry::AlternatePlunder0x00734547
            }
            CitiesCapturePlunderAwardContinuation::LocalAwardNotification0x00734152 => {
                return Err(CitiesCaptureResidualPlanError::PriorContinuationMismatch);
            }
        },
        CitiesCaptureResidualPrior::SkipPlunder(receipt) => {
            if receipt.continuation != CitiesCapturePlunderGateContinuation::SkipPlunder0x00734a3c {
                return Err(CitiesCaptureResidualPlanError::PriorContinuationMismatch);
            }
            if receipt.notification_raised {
                return Err(CitiesCaptureResidualPlanError::NotificationStateMismatch);
            }
            CitiesCaptureResidualEntry::CommonCaptureNotification0x00734a3c
        }
    };

    let gate = prior.gate();
    let old_city = gate.old_city;
    let old_owner = old_city.who;
    let new_owner = gate.new_owner;
    let new_city = gate
        .prior
        .new_city
        .ok_or(CitiesCaptureResidualPlanError::MissingNewCity)?;
    if new_city.who != new_owner || old_city.who != old_owner {
        return Err(CitiesCaptureResidualPlanError::OwnerMismatch);
    }
    if gate
        .prior
        .old_city_objects
        .first()
        .is_none_or(|o| o.who != old_owner)
    {
        return Err(CitiesCaptureResidualPlanError::MissingOldCenter);
    }
    if gate
        .prior
        .new_city_objects
        .first()
        .is_none_or(|o| o.who != new_owner)
    {
        return Err(CitiesCaptureResidualPlanError::MissingNewCenter);
    }

    Ok(CitiesCaptureResidualPlan {
        old_city_objects: gate.prior.old_city_objects.clone(),
        new_city_objects: gate.prior.new_city_objects.clone(),
        prior,
        entry,
        old_city,
        new_city,
        old_owner,
        new_owner,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AvailabilityPhase {
    CapitalRefund0x00734340,
    AlternateNewAward0x007345a2,
    AlternateOldRefund0x00734802,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TypeAvailabilityRequest {
    pub phase: AvailabilityPhase,
    pub owner: u8,
    pub type_index: i32,
    pub mode: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TypeAvailabilityReceipt {
    pub request: TypeAvailabilityRequest,
    pub raw_availability: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourceBalanceRequest {
    pub phase: AvailabilityPhase,
    pub owner: u8,
    pub bucket: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourceBalanceReceipt {
    pub request: ResourceBalanceRequest,
    pub decoded_balance: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BucketAddRequest {
    pub phase: AvailabilityPhase,
    pub owner: u8,
    pub bucket: i32,
    pub amount: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BucketAddReceipt {
    pub request: BucketAddRequest,
    pub balance_before: i32,
    pub balance_after: i32,
    pub mutation_applied: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TheDespotPlunderRuleReceipt {
    pub percent: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsoleReadPhase {
    CapitalRefund0x0073438c,
    AlternateNewAward0x0073461e,
    AlternateOldRefund0x00734880,
    CommonCapture0x00734a67,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConsoleWhoRequest {
    pub phase: ConsoleReadPhase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConsoleWhoReceipt {
    pub request: ConsoleWhoRequest,
    pub who: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapturePresentationRequest {
    CapitalRefund {
        city: CityKey,
        owner: u8,
        amount: i32,
        amount_template: u32,
        city_template: u32,
    },
    ResourceAward {
        city: CityKey,
        beneficiary: u8,
        bubble_owner: u8,
        color_owner: u8,
        bucket: i32,
        amount: i32,
        amount_template: u32,
        city_template: u32,
    },
    LocalNewOwnerCapture {
        city: CityKey,
        owner: u8,
        message_template: u32,
        bubble_template: u32,
        notice_internal_offset: u32,
        sound: i32,
    },
    LocalOldOwnerCapture {
        city: CityKey,
        owner: u8,
        message_template: u32,
        bubble_template: u32,
        notice_internal_offset: u32,
        sound: i32,
    },
    AlliedNewOwnerCapture {
        city: CityKey,
        console_owner: u8,
        color_owner: u8,
        bubble_template: u32,
        event_flags: i32,
    },
    AlliedOldOwnerCapture {
        city: CityKey,
        console_owner: u8,
        color_owner: u8,
        bubble_template: u32,
        event_flags: i32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapturePresentationReceipt {
    pub request: CapturePresentationRequest,
    /// Attests the String temporaries, recycler allocation, color/coordinate reads,
    /// MessageWin mutation, optional IFace notice/sound, and reverse close order.
    pub exact_call_order_attested: bool,
    pub effects_complete: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObserverCaptureRequest {
    pub city: CityKey,
    pub console_owner: u8,
    pub new_owner: u8,
    pub old_owner: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObserverCaptureRoute {
    None,
    AlliedNewOwner,
    AlliedOldOwner,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObserverCaptureReceipt {
    pub request: ObserverCaptureRequest,
    pub route: ObserverCaptureRoute,
    /// Attests the compiled short-circuit order: console leader identity, forward and
    /// reverse diplomacy values, then `CityData::is_seen(console)` for each admitted arm.
    pub exact_query_order_attested: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapturedObjectInspectionRequest {
    pub object: ObjectKey,
    pub new_owner: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapturedObjectInspectionReceipt {
    pub request: CapturedObjectInspectionRequest,
    pub flags: u8,
    pub is_build: Option<bool>,
    pub is_type_0x1a1: Option<bool>,
    pub is_type_0x1a7: Option<bool>,
    pub has_close_preserving_tribe_bonus: Option<bool>,
    pub exact_query_order_attested: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjectMutationRequest {
    pub object: ObjectKey,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjectMutationReceipt {
    pub request: ObjectMutationRequest,
    pub effects_complete: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CloseCapturedBuildRequest {
    pub object: ObjectKey,
    pub mode: u8,
    pub killer: i32,
    pub zero_float_bits: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CloseCapturedBuildReceipt {
    pub request: CloseCapturedBuildRequest,
    pub effects_complete: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MaskCapturedWallRequest {
    pub object: ObjectKey,
    pub mask: i32,
    pub regen_roads: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MaskCapturedWallReceipt {
    pub request: MaskCapturedWallRequest,
    pub effects_complete: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapitalFlagsPhase {
    OldCity0x00735044,
    NewCity0x0073509a,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CityFlagsRequest {
    pub phase: CapitalFlagsPhase,
    pub city: CityKey,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CityFlagsReceipt {
    pub request: CityFlagsRequest,
    pub flags: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HasWonderRequest {
    pub owner: u8,
    pub type_index: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HasWonderReceipt {
    pub request: HasWonderRequest,
    pub raw_result: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FindCapitalRequest {
    pub owner: u8,
    pub excluded_city: CityKey,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FindCapitalReceipt {
    pub request: FindCapitalRequest,
    pub found_city: i32,
    pub found_owner: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LostCapitalRequest {
    pub old_owner: u8,
    pub captor: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LostCapitalReceipt {
    pub request: LostCapitalRequest,
    pub effects_complete: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecaptureCapitalRequest {
    pub new_owner: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecaptureCapitalReceipt {
    pub request: RecaptureCapitalRequest,
    pub effects_complete: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CenterReadPhase {
    OldCity0x007350e7,
    NewCity0x00735189,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CityCenterRequest {
    pub phase: CenterReadPhase,
    pub city: CityKey,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CityCenterReceipt {
    pub request: CityCenterRequest,
    pub center: ObjectKey,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CloseOldCityRequest {
    pub city: CityKey,
    /// `City::close`'s first stack word is volatile at this site and is not read by the
    /// shipped callee. This is the only callee-observable argument.
    pub captor: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CloseOldCityReceipt {
    pub request: CloseOldCityRequest,
    pub effects_complete: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrimCityTailRequest {
    pub owner: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrimCityTailReceipt {
    pub request: TrimCityTailRequest,
    pub city_mark_before: i32,
    /// Flags read at `city_mark-1`, in descending index order, ending at the first live
    /// row when present. Only preceding inactive rows are removed.
    pub trailing_flags: Vec<u16>,
    pub city_mark_after: i32,
    pub mutation_applied: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SetBuildCityRequest {
    pub object: ObjectKey,
    pub city_before: i16,
    pub city_after: i16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SetBuildCityReceipt {
    pub request: SetBuildCityRequest,
    pub mutation_applied: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UpdateCenterHitsRequest {
    pub object: ObjectKey,
    pub arg: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UpdateCenterHitsReceipt {
    pub request: UpdateCenterHitsRequest,
    pub effects_complete: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadCenterHitsRequest {
    pub object: ObjectKey,
    pub arg: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadCenterHitsReceipt {
    pub request: ReadCenterHitsRequest,
    pub hits: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SetCenterDamageRequest {
    pub object: ObjectKey,
    pub damage_before: i32,
    pub damage_after: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SetCenterDamageReceipt {
    pub request: SetCenterDamageRequest,
    pub mutation_applied: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CalcPopCapRequest {
    pub owner: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CalcPopCapReceipt {
    pub request: CalcPopCapRequest,
    pub effects_complete: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureArrayKind {
    NewOwnerObjects,
    OldOwnerObjects,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DestroyCaptureArrayRequest {
    pub kind: CaptureArrayKind,
    pub objects: Vec<ObjectKey>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DestroyCaptureArrayReceipt {
    pub request: DestroyCaptureArrayRequest,
    pub storage_released: bool,
}

pub trait CitiesCaptureResidualWorld {
    fn read_type_availability(
        &mut self,
        request: TypeAvailabilityRequest,
    ) -> Option<TypeAvailabilityReceipt>;
    fn read_resource_balance(
        &mut self,
        request: ResourceBalanceRequest,
    ) -> Option<ResourceBalanceReceipt>;
    fn bucket_add(&mut self, request: BucketAddRequest) -> Option<BucketAddReceipt>;
    fn read_thedespot_plunder_rule(&mut self) -> Option<TheDespotPlunderRuleReceipt>;
    fn read_console_who(&mut self, request: ConsoleWhoRequest) -> Option<ConsoleWhoReceipt>;
    fn present_capture(
        &mut self,
        request: CapturePresentationRequest,
    ) -> Option<CapturePresentationReceipt>;
    fn inspect_observer_capture(
        &mut self,
        request: ObserverCaptureRequest,
    ) -> Option<ObserverCaptureReceipt>;
    fn inspect_captured_object(
        &mut self,
        request: CapturedObjectInspectionRequest,
    ) -> Option<CapturedObjectInspectionReceipt>;
    fn disband_object(&mut self, request: ObjectMutationRequest) -> Option<ObjectMutationReceipt>;
    fn close_captured_build(
        &mut self,
        request: CloseCapturedBuildRequest,
    ) -> Option<CloseCapturedBuildReceipt>;
    fn mask_captured_wall(
        &mut self,
        request: MaskCapturedWallRequest,
    ) -> Option<MaskCapturedWallReceipt>;
    fn read_city_flags(&mut self, request: CityFlagsRequest) -> Option<CityFlagsReceipt>;
    fn has_wonder(&mut self, request: HasWonderRequest) -> Option<HasWonderReceipt>;
    fn find_capital(&mut self, request: FindCapitalRequest) -> Option<FindCapitalReceipt>;
    fn lost_capital(&mut self, request: LostCapitalRequest) -> Option<LostCapitalReceipt>;
    fn recapture_capital(
        &mut self,
        request: RecaptureCapitalRequest,
    ) -> Option<RecaptureCapitalReceipt>;
    fn read_city_center(&mut self, request: CityCenterRequest) -> Option<CityCenterReceipt>;
    fn close_old_city(&mut self, request: CloseOldCityRequest) -> Option<CloseOldCityReceipt>;
    fn trim_city_tail(&mut self, request: TrimCityTailRequest) -> Option<TrimCityTailReceipt>;
    fn read_build_city(&mut self, object: ObjectKey) -> Option<i16>;
    fn set_build_city(&mut self, request: SetBuildCityRequest) -> Option<SetBuildCityReceipt>;
    fn update_center_hits(
        &mut self,
        request: UpdateCenterHitsRequest,
    ) -> Option<UpdateCenterHitsReceipt>;
    fn read_center_hits(&mut self, request: ReadCenterHitsRequest)
        -> Option<ReadCenterHitsReceipt>;
    fn read_center_damage(&mut self, object: ObjectKey) -> Option<i32>;
    fn set_center_damage(
        &mut self,
        request: SetCenterDamageRequest,
    ) -> Option<SetCenterDamageReceipt>;
    fn update_center_los(
        &mut self,
        request: ObjectMutationRequest,
    ) -> Option<ObjectMutationReceipt>;
    fn calc_pop_cap(&mut self, request: CalcPopCapRequest) -> Option<CalcPopCapReceipt>;
    fn destroy_capture_array(
        &mut self,
        request: DestroyCaptureArrayRequest,
    ) -> Option<DestroyCaptureArrayReceipt>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CitiesCaptureResidualEvent {
    InitializeOldOwnerRefund(i32),
    ReadTheDespotRule(TheDespotPlunderRuleReceipt),
    SetNewOwnerAward(i32),
    SetOldOwnerRefund(i32),
    ReadAvailability(TypeAvailabilityReceipt),
    ReadBalance(ResourceBalanceReceipt),
    BucketAdd(BucketAddReceipt),
    ReadConsoleWho(ConsoleWhoReceipt),
    Present(CapturePresentationReceipt),
    InspectObserver(ObserverCaptureReceipt),
    InspectObject(CapturedObjectInspectionReceipt),
    DisbandObject(ObjectMutationReceipt),
    CloseBuild(CloseCapturedBuildReceipt),
    MaskWall(MaskCapturedWallReceipt),
    ReadCityFlags(CityFlagsReceipt),
    HasWonder(HasWonderReceipt),
    FindCapital(FindCapitalReceipt),
    LostCapital(LostCapitalReceipt),
    RecaptureCapital(RecaptureCapitalReceipt),
    ReadCityCenter(CityCenterReceipt),
    CloseOldCity(CloseOldCityReceipt),
    TrimCityTail(TrimCityTailReceipt),
    SetBuildCity(SetBuildCityReceipt),
    UpdateCenterHits(UpdateCenterHitsReceipt),
    ReadCenterHits(ReadCenterHitsReceipt),
    SetCenterDamage(SetCenterDamageReceipt),
    UpdateCenterLos(ObjectMutationReceipt),
    CalcPopCap(CalcPopCapReceipt),
    DestroyArray(DestroyCaptureArrayReceipt),
    ReturnCity(i32),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CitiesCaptureResidualReceipt {
    pub prior: CitiesCaptureResidualPrior,
    pub old_city: CityKey,
    pub new_city: CityKey,
    pub returned_city: i32,
    pub events: Vec<CitiesCaptureResidualEvent>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CitiesCaptureResidualApplyError {
    MissingReceipt,
    ReceiptMismatch,
    EffectsIncomplete,
    AvailabilityValueInvalid,
    InspectionShapeMismatch,
    InvalidCenterIdentity,
    InvalidCityTailReceipt,
    InvalidConsoleOwner,
}

fn availability<W: CitiesCaptureResidualWorld + ?Sized>(
    world: &mut W,
    events: &mut Vec<CitiesCaptureResidualEvent>,
    request: TypeAvailabilityRequest,
) -> Result<bool, CitiesCaptureResidualApplyError> {
    let receipt = world
        .read_type_availability(request)
        .ok_or(CitiesCaptureResidualApplyError::MissingReceipt)?;
    if receipt.request != request {
        return Err(CitiesCaptureResidualApplyError::ReceiptMismatch);
    }
    if !matches!(receipt.raw_availability, 0 | 2 | 4) {
        return Err(CitiesCaptureResidualApplyError::AvailabilityValueInvalid);
    }
    events.push(CitiesCaptureResidualEvent::ReadAvailability(receipt));
    Ok(receipt.raw_availability != 0)
}

fn add_bucket<W: CitiesCaptureResidualWorld + ?Sized>(
    world: &mut W,
    events: &mut Vec<CitiesCaptureResidualEvent>,
    request: BucketAddRequest,
    expected_before: Option<i32>,
) -> Result<(), CitiesCaptureResidualApplyError> {
    let receipt = world
        .bucket_add(request)
        .ok_or(CitiesCaptureResidualApplyError::MissingReceipt)?;
    if receipt.request != request
        || expected_before.is_some_and(|before| before != receipt.balance_before)
        || receipt.balance_after != receipt.balance_before.wrapping_add(request.amount)
    {
        return Err(CitiesCaptureResidualApplyError::ReceiptMismatch);
    }
    if !receipt.mutation_applied {
        return Err(CitiesCaptureResidualApplyError::EffectsIncomplete);
    }
    events.push(CitiesCaptureResidualEvent::BucketAdd(receipt));
    Ok(())
}

fn console<W: CitiesCaptureResidualWorld + ?Sized>(
    world: &mut W,
    events: &mut Vec<CitiesCaptureResidualEvent>,
    phase: ConsoleReadPhase,
) -> Result<i32, CitiesCaptureResidualApplyError> {
    let request = ConsoleWhoRequest { phase };
    let receipt = world
        .read_console_who(request)
        .ok_or(CitiesCaptureResidualApplyError::MissingReceipt)?;
    if receipt.request != request {
        return Err(CitiesCaptureResidualApplyError::ReceiptMismatch);
    }
    events.push(CitiesCaptureResidualEvent::ReadConsoleWho(receipt));
    Ok(receipt.who)
}

fn present<W: CitiesCaptureResidualWorld + ?Sized>(
    world: &mut W,
    events: &mut Vec<CitiesCaptureResidualEvent>,
    request: CapturePresentationRequest,
) -> Result<(), CitiesCaptureResidualApplyError> {
    let receipt = world
        .present_capture(request)
        .ok_or(CitiesCaptureResidualApplyError::MissingReceipt)?;
    if receipt.request != request || !receipt.exact_call_order_attested {
        return Err(CitiesCaptureResidualApplyError::ReceiptMismatch);
    }
    if !receipt.effects_complete {
        return Err(CitiesCaptureResidualApplyError::EffectsIncomplete);
    }
    events.push(CitiesCaptureResidualEvent::Present(receipt));
    Ok(())
}

fn minimum_bucket<W: CitiesCaptureResidualWorld + ?Sized>(
    world: &mut W,
    events: &mut Vec<CitiesCaptureResidualEvent>,
    phase: AvailabilityPhase,
    new_owner: u8,
    old_owner: u8,
    balance_owner: u8,
) -> Result<(i32, i32), CitiesCaptureResidualApplyError> {
    let mut minimum = MINIMUM_BUCKET_SENTINEL;
    let mut selected = RESOURCE_TYPE_FIRST;
    for type_index in RESOURCE_TYPE_FIRST..RESOURCE_TYPE_END {
        let new_available = availability(
            world,
            events,
            TypeAvailabilityRequest {
                phase,
                owner: new_owner,
                type_index,
                mode: TYPE_AVAIL_MODE,
            },
        )?;
        if !new_available {
            continue;
        }
        let old_available = availability(
            world,
            events,
            TypeAvailabilityRequest {
                phase,
                owner: old_owner,
                type_index,
                mode: TYPE_AVAIL_MODE,
            },
        )?;
        if !old_available || type_index == KNOWLEDGE_RESOURCE_TYPE {
            continue;
        }
        let request = ResourceBalanceRequest {
            phase,
            owner: balance_owner,
            bucket: type_index,
        };
        let receipt = world
            .read_resource_balance(request)
            .ok_or(CitiesCaptureResidualApplyError::MissingReceipt)?;
        if receipt.request != request {
            return Err(CitiesCaptureResidualApplyError::ReceiptMismatch);
        }
        events.push(CitiesCaptureResidualEvent::ReadBalance(receipt));
        if receipt.decoded_balance < minimum {
            minimum = receipt.decoded_balance;
            selected = type_index;
        }
    }
    Ok((selected, minimum))
}

fn validate_object_inspection(
    receipt: CapturedObjectInspectionReceipt,
) -> Result<(), CitiesCaptureResidualApplyError> {
    if !receipt.exact_query_order_attested {
        return Err(CitiesCaptureResidualApplyError::ReceiptMismatch);
    }
    let open = receipt.flags & OBJECT_CLOSED_FLAG == 0;
    if receipt.is_build.is_some() != open {
        return Err(CitiesCaptureResidualApplyError::InspectionShapeMismatch);
    }
    let build = receipt.is_build == Some(true);
    if receipt.is_type_0x1a1.is_some() != build {
        return Err(CitiesCaptureResidualApplyError::InspectionShapeMismatch);
    }
    let first_missed = receipt.is_type_0x1a1 == Some(false);
    if receipt.is_type_0x1a7.is_some() != first_missed {
        return Err(CitiesCaptureResidualApplyError::InspectionShapeMismatch);
    }
    let close_preserving_type =
        receipt.is_type_0x1a1 == Some(true) || receipt.is_type_0x1a7 == Some(true);
    if receipt.has_close_preserving_tribe_bonus.is_some() != close_preserving_type {
        return Err(CitiesCaptureResidualApplyError::InspectionShapeMismatch);
    }
    Ok(())
}

fn valid_tail(receipt: &TrimCityTailReceipt) -> bool {
    if receipt.city_mark_before < 0 || receipt.city_mark_after < 0 {
        return false;
    }
    let removed = receipt.city_mark_before - receipt.city_mark_after;
    if removed < 0
        || receipt.trailing_flags.is_empty() && (removed != 0 || receipt.city_mark_before != 0)
    {
        return false;
    }
    let inactive_prefix = receipt
        .trailing_flags
        .iter()
        .take_while(|flags| **flags & 1 == 0)
        .count() as i32;
    let expected_removed = inactive_prefix.min(receipt.city_mark_before);
    removed == expected_removed
        && (expected_removed as usize == receipt.trailing_flags.len()
            || receipt.trailing_flags[expected_removed as usize] & 1 != 0)
}

/// Execute the complete `0x0073432D..0x007352BE` retail residual.
pub fn apply_cities_capture_residual<W: CitiesCaptureResidualWorld + ?Sized>(
    plan: CitiesCaptureResidualPlan,
    world: &mut W,
) -> Result<CitiesCaptureResidualReceipt, CitiesCaptureResidualApplyError> {
    let gate = plan.prior.gate().clone();
    let mut events = Vec::new();
    let mut notification_raised = false;
    let mut skip_common_presentation = false;

    match plan.entry {
        CitiesCaptureResidualEntry::OldOwnerRefund0x0073432d {
            refund,
            notification_raised: prior_notification,
        } => {
            notification_raised = prior_notification;
            if refund != 0 {
                for type_index in RESOURCE_TYPE_FIRST..RESOURCE_TYPE_END {
                    let new_available = availability(
                        world,
                        &mut events,
                        TypeAvailabilityRequest {
                            phase: AvailabilityPhase::CapitalRefund0x00734340,
                            owner: plan.new_owner,
                            type_index,
                            mode: TYPE_AVAIL_MODE,
                        },
                    )?;
                    if !new_available {
                        continue;
                    }
                    let old_available = availability(
                        world,
                        &mut events,
                        TypeAvailabilityRequest {
                            phase: AvailabilityPhase::CapitalRefund0x00734340,
                            owner: plan.old_owner,
                            type_index,
                            mode: TYPE_AVAIL_MODE,
                        },
                    )?;
                    if old_available && type_index != KNOWLEDGE_RESOURCE_TYPE {
                        add_bucket(
                            world,
                            &mut events,
                            BucketAddRequest {
                                phase: AvailabilityPhase::CapitalRefund0x00734340,
                                owner: plan.old_owner,
                                bucket: type_index,
                                amount: refund,
                            },
                            None,
                        )?;
                    }
                }
                if console(
                    world,
                    &mut events,
                    ConsoleReadPhase::CapitalRefund0x0073438c,
                )? == i32::from(plan.old_owner)
                {
                    present(
                        world,
                        &mut events,
                        CapturePresentationRequest::CapitalRefund {
                            city: plan.new_city,
                            owner: plan.old_owner,
                            amount: refund,
                            amount_template: CAPITAL_REFUND_AMOUNT_TEMPLATE_OFFSET,
                            city_template: CAPITAL_REFUND_CITY_TEMPLATE_OFFSET,
                        },
                    )?;
                    skip_common_presentation = true;
                }
            }
        }
        CitiesCaptureResidualEntry::AlternatePlunder0x00734547 => {
            let plunder = gate.prior.plunder_accumulator;
            let mut award = plunder;
            let mut refund = 0;
            events.push(CitiesCaptureResidualEvent::InitializeOldOwnerRefund(0));
            if gate.russian_plunder_steal {
                refund = plunder;
                award = if gate.qualifying_general { plunder } else { 0 };
            } else if gate.qualifying_general {
                let rule = world
                    .read_thedespot_plunder_rule()
                    .ok_or(CitiesCaptureResidualApplyError::MissingReceipt)?;
                events.push(CitiesCaptureResidualEvent::ReadTheDespotRule(rule));
                award = plunder.wrapping_mul(rule.percent) / 100;
            }
            events.push(CitiesCaptureResidualEvent::SetOldOwnerRefund(refund));
            events.push(CitiesCaptureResidualEvent::SetNewOwnerAward(award));

            if award != 0 {
                let (bucket, before) = minimum_bucket(
                    world,
                    &mut events,
                    AvailabilityPhase::AlternateNewAward0x007345a2,
                    plan.new_owner,
                    plan.old_owner,
                    plan.new_owner,
                )?;
                add_bucket(
                    world,
                    &mut events,
                    BucketAddRequest {
                        phase: AvailabilityPhase::AlternateNewAward0x007345a2,
                        owner: plan.new_owner,
                        bucket,
                        amount: award,
                    },
                    (before != MINIMUM_BUCKET_SENTINEL).then_some(before),
                )?;
                if console(
                    world,
                    &mut events,
                    ConsoleReadPhase::AlternateNewAward0x0073461e,
                )? == i32::from(plan.new_owner)
                {
                    present(
                        world,
                        &mut events,
                        CapturePresentationRequest::ResourceAward {
                            city: plan.new_city,
                            beneficiary: plan.new_owner,
                            bubble_owner: plan.new_owner,
                            color_owner: plan.new_owner,
                            bucket,
                            amount: award,
                            amount_template: RESOURCE_AMOUNT_TEMPLATE_OFFSET,
                            city_template: RESOURCE_CITY_TEMPLATE_OFFSET,
                        },
                    )?;
                    notification_raised = true;
                }
            }
            if refund != 0 {
                let (bucket, before) = minimum_bucket(
                    world,
                    &mut events,
                    AvailabilityPhase::AlternateOldRefund0x00734802,
                    plan.new_owner,
                    plan.old_owner,
                    plan.old_owner,
                )?;
                add_bucket(
                    world,
                    &mut events,
                    BucketAddRequest {
                        phase: AvailabilityPhase::AlternateOldRefund0x00734802,
                        owner: plan.old_owner,
                        bucket,
                        amount: refund,
                    },
                    (before != MINIMUM_BUCKET_SENTINEL).then_some(before),
                )?;
                if console(
                    world,
                    &mut events,
                    ConsoleReadPhase::AlternateOldRefund0x00734880,
                )? == i32::from(plan.old_owner)
                {
                    present(
                        world,
                        &mut events,
                        CapturePresentationRequest::ResourceAward {
                            city: plan.new_city,
                            beneficiary: plan.old_owner,
                            // Measured at 0x007349CA/0x007349D6: the old-owner refund
                            // message attributes its bubble and neon color to the captor.
                            bubble_owner: plan.new_owner,
                            color_owner: plan.new_owner,
                            bucket,
                            amount: refund,
                            amount_template: RESOURCE_AMOUNT_TEMPLATE_OFFSET,
                            city_template: RESOURCE_CITY_TEMPLATE_OFFSET,
                        },
                    )?;
                    skip_common_presentation = true;
                }
            }
        }
        CitiesCaptureResidualEntry::CommonCaptureNotification0x00734a3c => {}
    }

    if !skip_common_presentation && !notification_raised {
        let console_owner = console(
            world,
            &mut events,
            ConsoleReadPhase::CommonCapture0x00734a67,
        )?;
        if console_owner == i32::from(plan.new_owner) {
            present(
                world,
                &mut events,
                CapturePresentationRequest::LocalNewOwnerCapture {
                    city: plan.new_city,
                    owner: plan.new_owner,
                    message_template: NEW_OWNER_CAPTURE_MESSAGE_OFFSET,
                    bubble_template: NEW_OWNER_CAPTURE_BUBBLE_OFFSET,
                    notice_internal_offset: NEW_OWNER_NOTICE_INTERNAL_BYTE_OFFSET,
                    sound: NEW_OWNER_CAPTURE_SOUND,
                },
            )?;
        } else if console_owner == i32::from(plan.old_owner) {
            present(
                world,
                &mut events,
                CapturePresentationRequest::LocalOldOwnerCapture {
                    city: plan.new_city,
                    owner: plan.old_owner,
                    message_template: OLD_OWNER_CAPTURE_MESSAGE_OFFSET,
                    bubble_template: OLD_OWNER_CAPTURE_BUBBLE_OFFSET,
                    notice_internal_offset: OLD_OWNER_NOTICE_INTERNAL_BYTE_OFFSET,
                    sound: OLD_OWNER_CAPTURE_SOUND,
                },
            )?;
        } else {
            let console_owner = u8::try_from(console_owner)
                .ok()
                .filter(|owner| *owner < 8)
                .ok_or(CitiesCaptureResidualApplyError::InvalidConsoleOwner)?;
            let request = ObserverCaptureRequest {
                city: plan.new_city,
                console_owner,
                new_owner: plan.new_owner,
                old_owner: plan.old_owner,
            };
            let observer = world
                .inspect_observer_capture(request)
                .ok_or(CitiesCaptureResidualApplyError::MissingReceipt)?;
            if observer.request != request || !observer.exact_query_order_attested {
                return Err(CitiesCaptureResidualApplyError::ReceiptMismatch);
            }
            events.push(CitiesCaptureResidualEvent::InspectObserver(observer));
            let request = match observer.route {
                ObserverCaptureRoute::None => None,
                ObserverCaptureRoute::AlliedNewOwner => {
                    Some(CapturePresentationRequest::AlliedNewOwnerCapture {
                        city: plan.new_city,
                        console_owner,
                        color_owner: plan.new_owner,
                        bubble_template: ALLIED_NEW_OWNER_BUBBLE_OFFSET,
                        event_flags: 0xC00,
                    })
                }
                ObserverCaptureRoute::AlliedOldOwner => {
                    Some(CapturePresentationRequest::AlliedOldOwnerCapture {
                        city: plan.new_city,
                        console_owner,
                        // Retail still selects the new owner's neon color here.
                        color_owner: plan.new_owner,
                        bubble_template: OLD_OWNER_CAPTURE_BUBBLE_OFFSET,
                        event_flags: 0xC00,
                    })
                }
            };
            if let Some(request) = request {
                present(world, &mut events, request)?;
            }
        }
    }

    for object in &plan.old_city_objects {
        let request = CapturedObjectInspectionRequest {
            object: *object,
            new_owner: plan.new_owner,
        };
        let inspection = world
            .inspect_captured_object(request)
            .ok_or(CitiesCaptureResidualApplyError::MissingReceipt)?;
        if inspection.request != request {
            return Err(CitiesCaptureResidualApplyError::ReceiptMismatch);
        }
        validate_object_inspection(inspection)?;
        events.push(CitiesCaptureResidualEvent::InspectObject(inspection));
        if inspection.flags & OBJECT_CLOSED_FLAG != 0 {
            continue;
        }
        if inspection.is_build != Some(true) {
            let request = ObjectMutationRequest { object: *object };
            let receipt = world
                .disband_object(request)
                .ok_or(CitiesCaptureResidualApplyError::MissingReceipt)?;
            if receipt.request != request || !receipt.effects_complete {
                return Err(CitiesCaptureResidualApplyError::EffectsIncomplete);
            }
            events.push(CitiesCaptureResidualEvent::DisbandObject(receipt));
            continue;
        }
        let preserve = inspection.has_close_preserving_tribe_bonus == Some(true);
        let request = CloseCapturedBuildRequest {
            object: *object,
            mode: if preserve { 0 } else { 5 },
            killer: -1,
            zero_float_bits: 0,
        };
        let receipt = world
            .close_captured_build(request)
            .ok_or(CitiesCaptureResidualApplyError::MissingReceipt)?;
        if receipt.request != request || !receipt.effects_complete {
            return Err(CitiesCaptureResidualApplyError::EffectsIncomplete);
        }
        events.push(CitiesCaptureResidualEvent::CloseBuild(receipt));
    }

    for object in &plan.new_city_objects {
        let request = MaskCapturedWallRequest {
            object: *object,
            mask: 1,
            regen_roads: 1,
        };
        let receipt = world
            .mask_captured_wall(request)
            .ok_or(CitiesCaptureResidualApplyError::MissingReceipt)?;
        if receipt.request != request || !receipt.effects_complete {
            return Err(CitiesCaptureResidualApplyError::EffectsIncomplete);
        }
        events.push(CitiesCaptureResidualEvent::MaskWall(receipt));
    }

    let old_flags_request = CityFlagsRequest {
        phase: CapitalFlagsPhase::OldCity0x00735044,
        city: plan.old_city,
    };
    let old_flags = world
        .read_city_flags(old_flags_request)
        .ok_or(CitiesCaptureResidualApplyError::MissingReceipt)?;
    if old_flags.request != old_flags_request {
        return Err(CitiesCaptureResidualApplyError::ReceiptMismatch);
    }
    events.push(CitiesCaptureResidualEvent::ReadCityFlags(old_flags));
    if old_flags.flags & CAPITAL_CITY_FLAG != 0 {
        let request = HasWonderRequest {
            owner: plan.old_owner,
            type_index: FORBIDDEN_CITY_TYPE,
        };
        let wonder = world
            .has_wonder(request)
            .ok_or(CitiesCaptureResidualApplyError::MissingReceipt)?;
        if wonder.request != request {
            return Err(CitiesCaptureResidualApplyError::ReceiptMismatch);
        }
        events.push(CitiesCaptureResidualEvent::HasWonder(wonder));
        if wonder.raw_result == 0 || wonder.raw_result == 2 {
            let request = FindCapitalRequest {
                owner: plan.old_owner,
                excluded_city: plan.old_city,
            };
            let found = world
                .find_capital(request)
                .ok_or(CitiesCaptureResidualApplyError::MissingReceipt)?;
            if found.request != request {
                return Err(CitiesCaptureResidualApplyError::ReceiptMismatch);
            }
            events.push(CitiesCaptureResidualEvent::FindCapital(found));
            if found.found_city < 0 || found.found_owner != i32::from(plan.old_owner) {
                let request = LostCapitalRequest {
                    old_owner: plan.old_owner,
                    captor: plan.new_owner,
                };
                let receipt = world
                    .lost_capital(request)
                    .ok_or(CitiesCaptureResidualApplyError::MissingReceipt)?;
                if receipt.request != request || !receipt.effects_complete {
                    return Err(CitiesCaptureResidualApplyError::EffectsIncomplete);
                }
                events.push(CitiesCaptureResidualEvent::LostCapital(receipt));
            }
        }
    }

    let new_flags_request = CityFlagsRequest {
        phase: CapitalFlagsPhase::NewCity0x0073509a,
        city: plan.new_city,
    };
    let new_flags = world
        .read_city_flags(new_flags_request)
        .ok_or(CitiesCaptureResidualApplyError::MissingReceipt)?;
    if new_flags.request != new_flags_request {
        return Err(CitiesCaptureResidualApplyError::ReceiptMismatch);
    }
    events.push(CitiesCaptureResidualEvent::ReadCityFlags(new_flags));
    if new_flags.flags & CAPITAL_CITY_FLAG != 0 {
        let request = FindCapitalRequest {
            owner: plan.new_owner,
            excluded_city: plan.new_city,
        };
        let found = world
            .find_capital(request)
            .ok_or(CitiesCaptureResidualApplyError::MissingReceipt)?;
        if found.request != request {
            return Err(CitiesCaptureResidualApplyError::ReceiptMismatch);
        }
        events.push(CitiesCaptureResidualEvent::FindCapital(found));
        if found.found_city < 0 || found.found_owner != i32::from(plan.new_owner) {
            let request = RecaptureCapitalRequest {
                new_owner: plan.new_owner,
            };
            let receipt = world
                .recapture_capital(request)
                .ok_or(CitiesCaptureResidualApplyError::MissingReceipt)?;
            if receipt.request != request || !receipt.effects_complete {
                return Err(CitiesCaptureResidualApplyError::EffectsIncomplete);
            }
            events.push(CitiesCaptureResidualEvent::RecaptureCapital(receipt));
        }
    }

    let old_center_request = CityCenterRequest {
        phase: CenterReadPhase::OldCity0x007350e7,
        city: plan.old_city,
    };
    let old_center = world
        .read_city_center(old_center_request)
        .ok_or(CitiesCaptureResidualApplyError::MissingReceipt)?;
    if old_center.request != old_center_request || old_center.center.who != plan.old_owner {
        return Err(CitiesCaptureResidualApplyError::InvalidCenterIdentity);
    }
    events.push(CitiesCaptureResidualEvent::ReadCityCenter(old_center));

    let close_request = CloseOldCityRequest {
        city: plan.old_city,
        captor: plan.new_owner,
    };
    let close = world
        .close_old_city(close_request)
        .ok_or(CitiesCaptureResidualApplyError::MissingReceipt)?;
    if close.request != close_request || !close.effects_complete {
        return Err(CitiesCaptureResidualApplyError::EffectsIncomplete);
    }
    events.push(CitiesCaptureResidualEvent::CloseOldCity(close));

    let trim_request = TrimCityTailRequest {
        owner: plan.old_owner,
    };
    let trim = world
        .trim_city_tail(trim_request.clone())
        .ok_or(CitiesCaptureResidualApplyError::MissingReceipt)?;
    if trim.request != trim_request || !valid_tail(&trim) {
        return Err(CitiesCaptureResidualApplyError::InvalidCityTailReceipt);
    }
    if !trim.mutation_applied && trim.city_mark_before != trim.city_mark_after {
        return Err(CitiesCaptureResidualApplyError::EffectsIncomplete);
    }
    events.push(CitiesCaptureResidualEvent::TrimCityTail(trim));

    let city_before = world
        .read_build_city(old_center.center)
        .ok_or(CitiesCaptureResidualApplyError::MissingReceipt)?;
    let set_city_request = SetBuildCityRequest {
        object: old_center.center,
        city_before,
        city_after: -1,
    };
    let set_city = world
        .set_build_city(set_city_request)
        .ok_or(CitiesCaptureResidualApplyError::MissingReceipt)?;
    if set_city.request != set_city_request || !set_city.mutation_applied {
        return Err(CitiesCaptureResidualApplyError::EffectsIncomplete);
    }
    events.push(CitiesCaptureResidualEvent::SetBuildCity(set_city));

    let close_center_request = CloseCapturedBuildRequest {
        object: old_center.center,
        mode: 5,
        killer: -1,
        zero_float_bits: 0,
    };
    let close_center = world
        .close_captured_build(close_center_request)
        .ok_or(CitiesCaptureResidualApplyError::MissingReceipt)?;
    if close_center.request != close_center_request || !close_center.effects_complete {
        return Err(CitiesCaptureResidualApplyError::EffectsIncomplete);
    }
    events.push(CitiesCaptureResidualEvent::CloseBuild(close_center));

    let new_center_request = CityCenterRequest {
        phase: CenterReadPhase::NewCity0x00735189,
        city: plan.new_city,
    };
    let new_center = world
        .read_city_center(new_center_request)
        .ok_or(CitiesCaptureResidualApplyError::MissingReceipt)?;
    if new_center.request != new_center_request || new_center.center.who != plan.new_owner {
        return Err(CitiesCaptureResidualApplyError::InvalidCenterIdentity);
    }
    events.push(CitiesCaptureResidualEvent::ReadCityCenter(new_center));

    let update_hits_request = UpdateCenterHitsRequest {
        object: new_center.center,
        arg: 0,
    };
    let update_hits = world
        .update_center_hits(update_hits_request)
        .ok_or(CitiesCaptureResidualApplyError::MissingReceipt)?;
    if update_hits.request != update_hits_request || !update_hits.effects_complete {
        return Err(CitiesCaptureResidualApplyError::EffectsIncomplete);
    }
    events.push(CitiesCaptureResidualEvent::UpdateCenterHits(update_hits));

    let hits_request = ReadCenterHitsRequest {
        object: new_center.center,
        arg: 0,
    };
    let hits = world
        .read_center_hits(hits_request)
        .ok_or(CitiesCaptureResidualApplyError::MissingReceipt)?;
    if hits.request != hits_request {
        return Err(CitiesCaptureResidualApplyError::ReceiptMismatch);
    }
    events.push(CitiesCaptureResidualEvent::ReadCenterHits(hits));
    let damage_before = world
        .read_center_damage(new_center.center)
        .ok_or(CitiesCaptureResidualApplyError::MissingReceipt)?;
    let damage_request = SetCenterDamageRequest {
        object: new_center.center,
        damage_before,
        damage_after: hits.hits.wrapping_sub(10),
    };
    let damage = world
        .set_center_damage(damage_request)
        .ok_or(CitiesCaptureResidualApplyError::MissingReceipt)?;
    if damage.request != damage_request || !damage.mutation_applied {
        return Err(CitiesCaptureResidualApplyError::EffectsIncomplete);
    }
    events.push(CitiesCaptureResidualEvent::SetCenterDamage(damage));

    let los_request = ObjectMutationRequest {
        object: new_center.center,
    };
    let los = world
        .update_center_los(los_request)
        .ok_or(CitiesCaptureResidualApplyError::MissingReceipt)?;
    if los.request != los_request || !los.effects_complete {
        return Err(CitiesCaptureResidualApplyError::EffectsIncomplete);
    }
    events.push(CitiesCaptureResidualEvent::UpdateCenterLos(los));

    let pop_request = CalcPopCapRequest {
        owner: plan.new_owner,
    };
    let pop = world
        .calc_pop_cap(pop_request)
        .ok_or(CitiesCaptureResidualApplyError::MissingReceipt)?;
    if pop.request != pop_request || !pop.effects_complete {
        return Err(CitiesCaptureResidualApplyError::EffectsIncomplete);
    }
    events.push(CitiesCaptureResidualEvent::CalcPopCap(pop));

    for request in [
        DestroyCaptureArrayRequest {
            kind: CaptureArrayKind::NewOwnerObjects,
            objects: plan.new_city_objects.clone(),
        },
        DestroyCaptureArrayRequest {
            kind: CaptureArrayKind::OldOwnerObjects,
            objects: plan.old_city_objects.clone(),
        },
    ] {
        let receipt = world
            .destroy_capture_array(request.clone())
            .ok_or(CitiesCaptureResidualApplyError::MissingReceipt)?;
        if receipt.request != request || !receipt.storage_released {
            return Err(CitiesCaptureResidualApplyError::EffectsIncomplete);
        }
        events.push(CitiesCaptureResidualEvent::DestroyArray(receipt));
    }

    let returned_city = i32::from(plan.new_city.city);
    events.push(CitiesCaptureResidualEvent::ReturnCity(returned_city));
    Ok(CitiesCaptureResidualReceipt {
        prior: plan.prior,
        old_city: plan.old_city,
        new_city: plan.new_city,
        returned_city,
        events,
    })
}
