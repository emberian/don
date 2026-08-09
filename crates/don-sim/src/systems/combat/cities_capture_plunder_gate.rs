//! `Cities::capture_city` plunder gate, `0x00733D67..0x00733FAC`.
//!
//! This tranche classifies the captured center's government generals, derives the
//! Russian plunder-steal local, and applies the captured-city cooldown gates.  It
//! stops before the first persistent leader mutation at `0x00733FDC`.

use super::build_check_capture::CityKey;
use super::cities_capture_prefix::CITIES_CAPTURE_CITY_END;
use super::cities_capture_swap_fork::{
    CitiesCaptureSwapForkContinuation, CitiesCaptureSwapForkReceipt,
};
use super::damage_world::ObjectKey;

pub const CITIES_CAPTURE_PLUNDER_GATE_START: u32 = 0x0073_3D67;
pub const CITIES_CAPTURE_PLUNDER_GATE_END: u32 = 0x0073_3FAC;
pub const CITIES_CAPTURE_PLUNDER_GATE_SIZE: u32 =
    CITIES_CAPTURE_PLUNDER_GATE_END - CITIES_CAPTURE_PLUNDER_GATE_START;
pub const CITIES_CAPTURE_PLUNDER_GATE_RESIDUAL_SIZE: u32 =
    CITIES_CAPTURE_CITY_END - CITIES_CAPTURE_PLUNDER_GATE_END;

pub const CAPITAL_CITY_FLAG: u16 = 0x0010;
pub const PLUNDER_PROTECTED_CITY_FLAG: u16 = 0x0100;
pub const CAPTURE_PLUNDER_COOLDOWN_FRAMES: i32 = 0x1194;
pub const RUSSIAN_TRIBE_BONUS_INDEX: i32 = 13;
pub const THE_DESPOT_TYPE: i32 = 0x160;
pub const SPITAMENES_TYPE: i32 = 0x16D;
pub const DESPOT_ROSTER_SLOT: usize = 302;
pub const SPITAMENES_ROSTER_SLOT: usize = 315;
pub const HERO_SEARCH_EXTENT_SCALE: i32 = 96;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FindDespotRequest {
    pub x: i32,
    pub y: i32,
    /// Raw `LeaderData::who` value at `+0x08` for the center object's owner.
    pub center_owner_who: i32,
    pub zero: i32,
    pub type_index: i32,
    /// `(BuildTypeData[+0x234] + BuildTypeData[+0x238]) * 96`, or zero.
    pub search_extent: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FindDespotReceipt {
    pub request: FindDespotRequest,
    /// Raw `HeroesData::find_hero` return; every nonnegative value qualifies.
    pub result_o: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HasSpitamenesRequest {
    pub center: ObjectKey,
    pub first: i32,
    pub type_index: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HasSpitamenesReceipt {
    pub request: HasSpitamenesRequest,
    /// Raw `ObjectData::has_general` return; every nonnegative value qualifies.
    pub result_o: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapturedCenterGeneralInspectionRequest {
    pub new_city: CityKey,
    pub expected_center: ObjectKey,
    pub new_owner: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapturedCenterGeneralInspectionReceipt {
    pub request: CapturedCenterGeneralInspectionRequest,
    /// Attests vtable order `+0x18`, conditional `+0xC4`, conditional `is(0x160,0)`,
    /// fallback `+0x20`, optional `HeroesData::find_hero`, then optional
    /// `ObjectData::has_general(0,0x16D)`.
    pub exact_query_order_attested: bool,
    pub object_is_unit: bool,
    pub object_is_hero: Option<bool>,
    pub object_is_despot: Option<bool>,
    pub object_is_build: Option<bool>,
    pub build_size_x: Option<i32>,
    pub build_size_y: Option<i32>,
    pub center_x: i32,
    pub center_y: i32,
    pub center_owner_who: i32,
    /// Raw `LeaderData::num_units[302]`, read only on the fallback path.
    pub despot_roster_count: Option<u16>,
    pub find_despot: Option<FindDespotReceipt>,
    /// Raw `LeaderData::num_units[315]` for the new owner.
    pub spitamenes_roster_count: u16,
    pub has_spitamenes: Option<HasSpitamenesReceipt>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RussianPlunderInspectionRequest {
    pub old_owner: u8,
    pub captured_own_capital: bool,
    pub tribe_bonus_index: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RussianPlunderInspectionReceipt {
    pub request: RussianPlunderInspectionRequest,
    pub has_russian_bonus: bool,
    /// Present exactly when `has_russian_bonus` is true.
    pub russian_plunder_steal_rule: Option<i32>,
    /// Both versions are present exactly when the rule is nonzero.
    pub game_version: Option<u32>,
    pub retail_version: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapturedCityFlagsRequest {
    pub old_city: CityKey,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapturedCityFlagsReceipt {
    pub request: CapturedCityFlagsRequest,
    pub city_flags: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptureStampRequest {
    pub old_city: CityKey,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptureStampReceipt {
    pub request: CaptureStampRequest,
    pub capture_stamp: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptureFrameReceipt {
    pub frame: i32,
}

pub trait CitiesCapturePlunderGateWorld {
    fn inspect_captured_center_generals(
        &mut self,
        request: CapturedCenterGeneralInspectionRequest,
    ) -> Option<CapturedCenterGeneralInspectionReceipt>;
    fn inspect_russian_plunder(
        &mut self,
        request: RussianPlunderInspectionRequest,
    ) -> Option<RussianPlunderInspectionReceipt>;
    fn read_captured_city_flags(
        &mut self,
        request: CapturedCityFlagsRequest,
    ) -> Option<CapturedCityFlagsReceipt>;
    fn read_capture_stamp(&mut self, request: CaptureStampRequest) -> Option<CaptureStampReceipt>;
    fn read_capture_frame(&mut self) -> Option<CaptureFrameReceipt>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CitiesCapturePlunderGatePlan {
    pub prior: CitiesCaptureSwapForkReceipt,
    pub old_city: CityKey,
    pub new_owner: u8,
    pub captured_own_capital: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CitiesCapturePlunderGatePlanError {
    PriorContinuationMismatch,
    MissingNewCity,
    MissingNewCenter,
    MissingOldCenter,
    NewOwnerMismatch,
    OldOwnerMismatch,
    InvalidOldCity,
}

pub fn plan_cities_capture_plunder_gate(
    prior: CitiesCaptureSwapForkReceipt,
    old_city: CityKey,
    new_owner: u8,
    captured_own_capital: bool,
) -> Result<CitiesCapturePlunderGatePlan, CitiesCapturePlunderGatePlanError> {
    if prior.continuation != CitiesCaptureSwapForkContinuation::Converged0x00733d67 {
        return Err(CitiesCapturePlunderGatePlanError::PriorContinuationMismatch);
    }
    let new_city = prior
        .new_city
        .ok_or(CitiesCapturePlunderGatePlanError::MissingNewCity)?;
    if new_city.who != new_owner {
        return Err(CitiesCapturePlunderGatePlanError::NewOwnerMismatch);
    }
    let center = prior
        .new_city_objects
        .first()
        .ok_or(CitiesCapturePlunderGatePlanError::MissingNewCenter)?;
    if center.who != new_owner || center.o < 0 {
        return Err(CitiesCapturePlunderGatePlanError::NewOwnerMismatch);
    }
    let old_center = prior
        .old_city_objects
        .first()
        .ok_or(CitiesCapturePlunderGatePlanError::MissingOldCenter)?;
    if old_center.who != old_city.who || old_center.o < 0 {
        return Err(CitiesCapturePlunderGatePlanError::OldOwnerMismatch);
    }
    if old_city.city < 0 {
        return Err(CitiesCapturePlunderGatePlanError::InvalidOldCity);
    }
    Ok(CitiesCapturePlunderGatePlan {
        prior,
        old_city,
        new_owner,
        captured_own_capital,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CitiesCapturePlunderGateEvent {
    InitializeRussianPlunderSteal(bool),
    InitializeNotificationRaised(bool),
    InitializeQualifyingGeneral(bool),
    InspectCapturedCenter(CapturedCenterGeneralInspectionReceipt),
    SetQualifyingGeneral(bool),
    InspectRussianPlunder(RussianPlunderInspectionReceipt),
    SetRussianPlunderSteal(bool),
    ReadCapturedCityFlags(CapturedCityFlagsReceipt),
    ReadCaptureStamp(CaptureStampReceipt),
    ReadCaptureFrame(CaptureFrameReceipt),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CitiesCapturePlunderGateContinuation {
    EnterPlunder0x00733fac,
    SkipPlunder0x00734a3c,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CitiesCapturePlunderGateReceipt {
    pub prior: CitiesCaptureSwapForkReceipt,
    pub old_city: CityKey,
    pub new_owner: u8,
    pub captured_own_capital: bool,
    pub qualifying_general: bool,
    pub russian_plunder_steal: bool,
    pub notification_raised: bool,
    pub city_flags: u16,
    pub capture_stamp: Option<i32>,
    pub continuation: CitiesCapturePlunderGateContinuation,
    pub events: Vec<CitiesCapturePlunderGateEvent>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CitiesCapturePlunderGateApplyError {
    MissingCenterInspection,
    CenterInspectionRequestMismatch,
    CenterQueryOrderNotAttested,
    CenterPredicateShapeMismatch,
    CenterBuildShapeMismatch,
    FindDespotReceiptMismatch,
    SpitamenesReceiptMismatch,
    MissingRussianInspection,
    RussianInspectionRequestMismatch,
    RussianInspectionShapeMismatch,
    MissingCityFlagsReceipt,
    CityFlagsReceiptMismatch,
    MissingCaptureStampReceipt,
    CaptureStampReceiptMismatch,
    MissingCaptureFrameReceipt,
}

/// Retail compares the four packed version bytes from least to most significant.
#[inline]
pub const fn game_version_is_at_most_retail(game: u32, retail: u32) -> bool {
    let game = game.to_le_bytes();
    let retail = retail.to_le_bytes();
    let mut i = 0;
    while i < 4 {
        if game[i] < retail[i] {
            return true;
        }
        if game[i] > retail[i] {
            return false;
        }
        i += 1;
    }
    true
}

fn validate_center_inspection(
    receipt: CapturedCenterGeneralInspectionReceipt,
    request: CapturedCenterGeneralInspectionRequest,
) -> Result<bool, CitiesCapturePlunderGateApplyError> {
    if receipt.request != request {
        return Err(CitiesCapturePlunderGateApplyError::CenterInspectionRequestMismatch);
    }
    if !receipt.exact_query_order_attested {
        return Err(CitiesCapturePlunderGateApplyError::CenterQueryOrderNotAttested);
    }

    let expected_hero = receipt.object_is_unit;
    if receipt.object_is_hero.is_some() != expected_hero {
        return Err(CitiesCapturePlunderGateApplyError::CenterPredicateShapeMismatch);
    }
    let expected_despot = receipt.object_is_hero == Some(true);
    if receipt.object_is_despot.is_some() != expected_despot {
        return Err(CitiesCapturePlunderGateApplyError::CenterPredicateShapeMismatch);
    }
    let direct_despot = receipt.object_is_despot == Some(true);
    if receipt.object_is_build.is_some() == direct_despot {
        return Err(CitiesCapturePlunderGateApplyError::CenterPredicateShapeMismatch);
    }

    let fallback = !direct_despot;
    let is_build = receipt.object_is_build == Some(true);
    if receipt.despot_roster_count.is_some() != fallback
        || receipt.build_size_x.is_some() != is_build
        || receipt.build_size_y.is_some() != is_build
    {
        return Err(CitiesCapturePlunderGateApplyError::CenterBuildShapeMismatch);
    }
    let search_extent = if is_build {
        receipt
            .build_size_x
            .expect("shape checked")
            .wrapping_add(receipt.build_size_y.expect("shape checked"))
            .wrapping_mul(HERO_SEARCH_EXTENT_SCALE)
    } else {
        0
    };
    let expected_find = fallback && receipt.despot_roster_count != Some(0);
    if receipt.find_despot.is_some() != expected_find {
        return Err(CitiesCapturePlunderGateApplyError::FindDespotReceiptMismatch);
    }
    if let Some(find) = receipt.find_despot {
        let expected = FindDespotRequest {
            x: receipt.center_x,
            y: receipt.center_y,
            center_owner_who: receipt.center_owner_who,
            zero: 0,
            type_index: THE_DESPOT_TYPE,
            search_extent,
        };
        if find.request != expected {
            return Err(CitiesCapturePlunderGateApplyError::FindDespotReceiptMismatch);
        }
    }

    let expected_spitamenes = receipt.spitamenes_roster_count != 0;
    if receipt.has_spitamenes.is_some() != expected_spitamenes {
        return Err(CitiesCapturePlunderGateApplyError::SpitamenesReceiptMismatch);
    }
    if let Some(has) = receipt.has_spitamenes {
        let expected = HasSpitamenesRequest {
            center: request.expected_center,
            first: 0,
            type_index: SPITAMENES_TYPE,
        };
        if has.request != expected {
            return Err(CitiesCapturePlunderGateApplyError::SpitamenesReceiptMismatch);
        }
    }

    let found_despot = direct_despot || receipt.find_despot.is_some_and(|find| find.result_o >= 0);
    let found_spitamenes = receipt.has_spitamenes.is_some_and(|has| has.result_o >= 0);
    Ok(found_despot || found_spitamenes)
}

fn validate_russian_inspection(
    receipt: RussianPlunderInspectionReceipt,
    request: RussianPlunderInspectionRequest,
) -> Result<bool, CitiesCapturePlunderGateApplyError> {
    if receipt.request != request {
        return Err(CitiesCapturePlunderGateApplyError::RussianInspectionRequestMismatch);
    }
    if receipt.russian_plunder_steal_rule.is_some() != receipt.has_russian_bonus {
        return Err(CitiesCapturePlunderGateApplyError::RussianInspectionShapeMismatch);
    }
    let enabled = receipt
        .russian_plunder_steal_rule
        .is_some_and(|rule| rule != 0);
    if receipt.game_version.is_some() != enabled || receipt.retail_version.is_some() != enabled {
        return Err(CitiesCapturePlunderGateApplyError::RussianInspectionShapeMismatch);
    }
    if !enabled {
        return Ok(false);
    }
    let game = receipt.game_version.expect("shape checked");
    let retail = receipt.retail_version.expect("shape checked");
    Ok(game_version_is_at_most_retail(game, retail) || !request.captured_own_capital)
}

/// Execute `0x00733D67..0x00733FAC` in retail query/local-write order.
pub fn apply_cities_capture_plunder_gate<W: CitiesCapturePlunderGateWorld + ?Sized>(
    plan: CitiesCapturePlunderGatePlan,
    world: &mut W,
) -> Result<CitiesCapturePlunderGateReceipt, CitiesCapturePlunderGateApplyError> {
    let mut events = vec![
        CitiesCapturePlunderGateEvent::InitializeRussianPlunderSteal(false),
        CitiesCapturePlunderGateEvent::InitializeNotificationRaised(false),
        CitiesCapturePlunderGateEvent::InitializeQualifyingGeneral(false),
    ];
    let new_city = plan.prior.new_city.expect("plan checked");
    let expected_center = plan.prior.new_city_objects[0];
    let center_request = CapturedCenterGeneralInspectionRequest {
        new_city,
        expected_center,
        new_owner: plan.new_owner,
    };
    let center = world
        .inspect_captured_center_generals(center_request)
        .ok_or(CitiesCapturePlunderGateApplyError::MissingCenterInspection)?;
    let qualifying_general = validate_center_inspection(center, center_request)?;
    events.push(CitiesCapturePlunderGateEvent::InspectCapturedCenter(center));
    events.push(CitiesCapturePlunderGateEvent::SetQualifyingGeneral(
        qualifying_general,
    ));

    let russian_request = RussianPlunderInspectionRequest {
        old_owner: plan.old_city.who,
        captured_own_capital: plan.captured_own_capital,
        tribe_bonus_index: RUSSIAN_TRIBE_BONUS_INDEX,
    };
    let russian = world
        .inspect_russian_plunder(russian_request)
        .ok_or(CitiesCapturePlunderGateApplyError::MissingRussianInspection)?;
    let russian_plunder_steal = validate_russian_inspection(russian, russian_request)?;
    events.push(CitiesCapturePlunderGateEvent::InspectRussianPlunder(
        russian,
    ));
    events.push(CitiesCapturePlunderGateEvent::SetRussianPlunderSteal(
        russian_plunder_steal,
    ));

    let flags_request = CapturedCityFlagsRequest {
        old_city: plan.old_city,
    };
    let flags = world
        .read_captured_city_flags(flags_request)
        .ok_or(CitiesCapturePlunderGateApplyError::MissingCityFlagsReceipt)?;
    if flags.request != flags_request {
        return Err(CitiesCapturePlunderGateApplyError::CityFlagsReceiptMismatch);
    }
    events.push(CitiesCapturePlunderGateEvent::ReadCapturedCityFlags(flags));

    let mut capture_stamp = None;
    let mut continuation = CitiesCapturePlunderGateContinuation::EnterPlunder0x00733fac;
    if (plan.prior.plunder_accumulator == 0 && flags.city_flags & CAPITAL_CITY_FLAG == 0)
        || flags.city_flags & PLUNDER_PROTECTED_CITY_FLAG != 0
    {
        continuation = CitiesCapturePlunderGateContinuation::SkipPlunder0x00734a3c;
    } else {
        let stamp_request = CaptureStampRequest {
            old_city: plan.old_city,
        };
        let stamp = world
            .read_capture_stamp(stamp_request)
            .ok_or(CitiesCapturePlunderGateApplyError::MissingCaptureStampReceipt)?;
        if stamp.request != stamp_request {
            return Err(CitiesCapturePlunderGateApplyError::CaptureStampReceiptMismatch);
        }
        capture_stamp = Some(stamp.capture_stamp);
        events.push(CitiesCapturePlunderGateEvent::ReadCaptureStamp(stamp));
        if stamp.capture_stamp != 0 {
            let frame = world
                .read_capture_frame()
                .ok_or(CitiesCapturePlunderGateApplyError::MissingCaptureFrameReceipt)?;
            events.push(CitiesCapturePlunderGateEvent::ReadCaptureFrame(frame));
            if frame.frame.wrapping_sub(stamp.capture_stamp) <= CAPTURE_PLUNDER_COOLDOWN_FRAMES {
                continuation = CitiesCapturePlunderGateContinuation::SkipPlunder0x00734a3c;
            }
        }
    }

    Ok(CitiesCapturePlunderGateReceipt {
        prior: plan.prior,
        old_city: plan.old_city,
        new_owner: plan.new_owner,
        captured_own_capital: plan.captured_own_capital,
        qualifying_general,
        russian_plunder_steal,
        notification_raised: false,
        city_flags: flags.city_flags,
        capture_stamp,
        continuation,
        events,
    })
}
