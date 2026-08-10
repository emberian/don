//! `Cities::capture_city` local new-owner award notification, `0x00734152..0x0073432D`.
//!
//! The tranche owns the complete localized `String`/`TextBubble`/`MessageWin` cone
//! entered when the console player is the new owner. It stops at the converged
//! old-owner-refund test and leaves that forward seam typed.

use super::build_check_capture::CityKey;
use super::cities_capture_plunder_award::{
    CitiesCapturePlunderAwardContinuation, CitiesCapturePlunderAwardReceipt,
};
use super::cities_capture_prefix::CITIES_CAPTURE_CITY_END;

pub const CITIES_CAPTURE_LOCAL_AWARD_NOTIFICATION_START: u32 = 0x0073_4152;
pub const CITIES_CAPTURE_LOCAL_AWARD_NOTIFICATION_END: u32 = 0x0073_432D;
pub const CITIES_CAPTURE_LOCAL_AWARD_NOTIFICATION_SIZE: u32 =
    CITIES_CAPTURE_LOCAL_AWARD_NOTIFICATION_END - CITIES_CAPTURE_LOCAL_AWARD_NOTIFICATION_START;
pub const CITIES_CAPTURE_LOCAL_AWARD_NOTIFICATION_RESIDUAL_SIZE: u32 =
    CITIES_CAPTURE_CITY_END - CITIES_CAPTURE_LOCAL_AWARD_NOTIFICATION_END;

pub const AWARD_BUBBLE_TEMPLATE_BYTE_OFFSET: u32 = 0x1504;
pub const AWARD_MESSAGE_TEMPLATE_BYTE_OFFSET: u32 = 0x1518;
pub const CITY_NAME_BYTE_OFFSET: u32 = 0x90;
pub const TEAM_COLOR_ENTRY_STRIDE: u32 = 100;
pub const MESSAGE_CATEGORY: i32 = -7;
pub const MESSAGE_DURATION: i32 = 1;
pub const MESSAGE_HEIGHT: i32 = 0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StringValueToken(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextBubbleToken(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColorToken(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalStringSlot {
    Message0x00734152,
    Bubble0x00734192,
    Unused0x007341c1,
    ParseAmountTemporary0x007341f6,
    ParseMessageTemporary0x00734261,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StringModuleIdReceipt {
    pub module_id: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalizedStringRequest {
    pub byte_offset: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalizedStringReceipt {
    pub request: LocalizedStringRequest,
    pub value: StringValueToken,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StringAssignPhase {
    BubbleTemplate0x007341ee,
    BubbleParsedAmount0x00734214,
    MessageTemplate0x00734239,
    MessageParsedCityAmount0x00734278,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StringAssignRequest {
    pub phase: StringAssignPhase,
    pub target: LocalStringSlot,
    pub value_before: Option<StringValueToken>,
    pub source: StringValueToken,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StringAssignReceipt {
    pub request: StringAssignRequest,
    pub value_after: StringValueToken,
    pub mutation_applied: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParseAmountRequest {
    pub template: StringValueToken,
    pub amount: i32,
    pub temporary: LocalStringSlot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParseAmountReceipt {
    pub request: ParseAmountRequest,
    pub parsed: StringValueToken,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CityNameRequest {
    pub city: CityKey,
    pub byte_offset: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CityNameReceipt {
    pub request: CityNameRequest,
    pub name: StringValueToken,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParseCityAmountRequest {
    pub template: StringValueToken,
    pub city_name: StringValueToken,
    pub amount: i32,
    pub temporary: LocalStringSlot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParseCityAmountReceipt {
    pub request: ParseCityAmountRequest,
    pub parsed: StringValueToken,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StringCloseRequest {
    pub slot: LocalStringSlot,
    pub value: Option<StringValueToken>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StringCloseReceipt {
    pub request: StringCloseRequest,
    pub closed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextBubblePopReceipt {
    pub bubble: TextBubbleToken,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BubbleTextWriteRequest {
    pub bubble: TextBubbleToken,
    pub value: StringValueToken,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BubbleTextWriteReceipt {
    pub request: BubbleTextWriteRequest,
    pub mutation_applied: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BubbleOwnerWriteRequest {
    pub bubble: TextBubbleToken,
    pub owner: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BubbleOwnerWriteReceipt {
    pub request: BubbleOwnerWriteRequest,
    pub mutation_applied: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OwnerColorIndexRequest {
    pub owner: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OwnerColorIndexReceipt {
    pub request: OwnerColorIndexRequest,
    pub color_index: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TeamColorKey {
    pub color_index: u8,
    pub entry_stride: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NeonColorRequest {
    pub team_color: TeamColorKey,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NeonColorReceipt {
    pub request: NeonColorRequest,
    pub color: ColorToken,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CityCoordinatesRequest {
    pub city: CityKey,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CityCoordinatesReceipt {
    pub request: CityCoordinatesRequest,
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AddMessageRequest {
    pub message: StringValueToken,
    pub x: i32,
    pub y: i32,
    pub category: i32,
    pub color: ColorToken,
    pub duration: i32,
    /// The compiled call passes the same `TeamColor*` identity in both unused
    /// integer ABI positions. Keep the identity typed instead of serializing a VA.
    pub unused_team_color_0: TeamColorKey,
    pub height: i32,
    pub bubble: TextBubbleToken,
    pub unused_team_color_1: TeamColorKey,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AddMessageReceipt {
    pub request: AddMessageRequest,
    pub mutation_applied: bool,
}

pub trait CitiesCaptureLocalAwardNotificationWorld {
    fn read_string_module_id(&mut self) -> Option<StringModuleIdReceipt>;
    fn read_localized_string(
        &mut self,
        request: LocalizedStringRequest,
    ) -> Option<LocalizedStringReceipt>;
    fn assign_string(&mut self, request: StringAssignRequest) -> Option<StringAssignReceipt>;
    fn parse_amount(&mut self, request: ParseAmountRequest) -> Option<ParseAmountReceipt>;
    fn read_city_name(&mut self, request: CityNameRequest) -> Option<CityNameReceipt>;
    fn parse_city_amount(
        &mut self,
        request: ParseCityAmountRequest,
    ) -> Option<ParseCityAmountReceipt>;
    fn close_string(&mut self, request: StringCloseRequest) -> Option<StringCloseReceipt>;
    fn pop_text_bubble(&mut self) -> Option<TextBubblePopReceipt>;
    fn write_bubble_text(
        &mut self,
        request: BubbleTextWriteRequest,
    ) -> Option<BubbleTextWriteReceipt>;
    fn write_bubble_owner(
        &mut self,
        request: BubbleOwnerWriteRequest,
    ) -> Option<BubbleOwnerWriteReceipt>;
    fn read_owner_color_index(
        &mut self,
        request: OwnerColorIndexRequest,
    ) -> Option<OwnerColorIndexReceipt>;
    fn get_neon_color(&mut self, request: NeonColorRequest) -> Option<NeonColorReceipt>;
    fn read_city_coordinates(
        &mut self,
        request: CityCoordinatesRequest,
    ) -> Option<CityCoordinatesReceipt>;
    fn add_message(&mut self, request: AddMessageRequest) -> Option<AddMessageReceipt>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CitiesCaptureLocalAwardNotificationPlan {
    pub prior: CitiesCapturePlunderAwardReceipt,
    pub new_city: CityKey,
    pub new_owner: u8,
    pub award: i32,
    /// Preserved unchanged for the first instruction at the continuation seam.
    pub old_owner_refund: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CitiesCaptureLocalAwardNotificationPlanError {
    PriorContinuationMismatch,
    MissingAward,
    ZeroAward,
    MissingOldOwnerRefund,
    NotificationAlreadyRaised,
    MissingNewCity,
    NewOwnerMismatch,
}

pub fn plan_cities_capture_local_award_notification(
    prior: CitiesCapturePlunderAwardReceipt,
) -> Result<CitiesCaptureLocalAwardNotificationPlan, CitiesCaptureLocalAwardNotificationPlanError> {
    if prior.continuation != CitiesCapturePlunderAwardContinuation::LocalAwardNotification0x00734152
    {
        return Err(CitiesCaptureLocalAwardNotificationPlanError::PriorContinuationMismatch);
    }
    let award = prior
        .new_owner_award
        .ok_or(CitiesCaptureLocalAwardNotificationPlanError::MissingAward)?;
    if award == 0 {
        return Err(CitiesCaptureLocalAwardNotificationPlanError::ZeroAward);
    }
    let old_owner_refund = prior
        .old_owner_refund
        .ok_or(CitiesCaptureLocalAwardNotificationPlanError::MissingOldOwnerRefund)?;
    if prior.prior.notification_raised {
        return Err(CitiesCaptureLocalAwardNotificationPlanError::NotificationAlreadyRaised);
    }
    let new_owner = prior.prior.new_owner;
    let new_city = prior
        .prior
        .prior
        .new_city
        .ok_or(CitiesCaptureLocalAwardNotificationPlanError::MissingNewCity)?;
    if new_city.who != new_owner {
        return Err(CitiesCaptureLocalAwardNotificationPlanError::NewOwnerMismatch);
    }
    Ok(CitiesCaptureLocalAwardNotificationPlan {
        prior,
        new_city,
        new_owner,
        award,
        old_owner_refund,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CitiesCaptureLocalAwardNotificationEvent {
    ReadStringModuleId(StringModuleIdReceipt),
    InitializeString(LocalStringSlot),
    ReadLocalizedString(LocalizedStringReceipt),
    AssignString(StringAssignReceipt),
    ParseAmount(ParseAmountReceipt),
    ReadCityName(CityNameReceipt),
    ParseCityAmount(ParseCityAmountReceipt),
    CloseString(StringCloseReceipt),
    PopTextBubble(TextBubblePopReceipt),
    WriteBubbleText(BubbleTextWriteReceipt),
    WriteBubbleOwner(BubbleOwnerWriteReceipt),
    ReadOwnerColorIndex(OwnerColorIndexReceipt),
    GetNeonColor(NeonColorReceipt),
    ReadCityCoordinates(CityCoordinatesReceipt),
    AddMessage(AddMessageReceipt),
    SetNotificationRaised(bool),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CitiesCaptureLocalAwardNotificationContinuation {
    OldOwnerRefund0x0073432d,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CitiesCaptureLocalAwardNotificationReceipt {
    pub prior: CitiesCapturePlunderAwardReceipt,
    pub new_city: CityKey,
    pub new_owner: u8,
    pub award: i32,
    /// Exact input to the `cmp [ebp-0x20], 0` at `0x0073432D`.
    pub old_owner_refund: i32,
    pub notification_raised: bool,
    pub continuation: CitiesCaptureLocalAwardNotificationContinuation,
    pub events: Vec<CitiesCaptureLocalAwardNotificationEvent>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CitiesCaptureLocalAwardNotificationApplyError {
    MissingStringModuleId,
    MissingLocalizedString,
    LocalizedStringReceiptMismatch,
    MissingStringAssignment,
    StringAssignmentMismatch,
    StringAssignmentEffectsIncomplete,
    MissingAmountParse,
    AmountParseMismatch,
    MissingCityName,
    CityNameMismatch,
    MissingCityAmountParse,
    CityAmountParseMismatch,
    MissingStringClose,
    StringCloseMismatch,
    StringCloseEffectsIncomplete,
    MissingTextBubble,
    MissingBubbleTextWrite,
    BubbleTextWriteMismatch,
    BubbleTextWriteEffectsIncomplete,
    MissingBubbleOwnerWrite,
    BubbleOwnerWriteMismatch,
    BubbleOwnerWriteEffectsIncomplete,
    MissingOwnerColorIndex,
    OwnerColorIndexMismatch,
    MissingNeonColor,
    NeonColorMismatch,
    MissingCityCoordinates,
    CityCoordinatesMismatch,
    MissingAddMessage,
    AddMessageMismatch,
    AddMessageEffectsIncomplete,
}

fn assign<W: CitiesCaptureLocalAwardNotificationWorld + ?Sized>(
    world: &mut W,
    events: &mut Vec<CitiesCaptureLocalAwardNotificationEvent>,
    request: StringAssignRequest,
) -> Result<StringValueToken, CitiesCaptureLocalAwardNotificationApplyError> {
    let receipt = world
        .assign_string(request)
        .ok_or(CitiesCaptureLocalAwardNotificationApplyError::MissingStringAssignment)?;
    if receipt.request != request || receipt.value_after != request.source {
        return Err(CitiesCaptureLocalAwardNotificationApplyError::StringAssignmentMismatch);
    }
    if !receipt.mutation_applied {
        return Err(
            CitiesCaptureLocalAwardNotificationApplyError::StringAssignmentEffectsIncomplete,
        );
    }
    events.push(CitiesCaptureLocalAwardNotificationEvent::AssignString(
        receipt,
    ));
    Ok(receipt.value_after)
}

fn close<W: CitiesCaptureLocalAwardNotificationWorld + ?Sized>(
    world: &mut W,
    events: &mut Vec<CitiesCaptureLocalAwardNotificationEvent>,
    request: StringCloseRequest,
) -> Result<(), CitiesCaptureLocalAwardNotificationApplyError> {
    let receipt = world
        .close_string(request)
        .ok_or(CitiesCaptureLocalAwardNotificationApplyError::MissingStringClose)?;
    if receipt.request != request {
        return Err(CitiesCaptureLocalAwardNotificationApplyError::StringCloseMismatch);
    }
    if !receipt.closed {
        return Err(CitiesCaptureLocalAwardNotificationApplyError::StringCloseEffectsIncomplete);
    }
    events.push(CitiesCaptureLocalAwardNotificationEvent::CloseString(
        receipt,
    ));
    Ok(())
}

/// Execute `0x00734152..0x0073432D` in retail call, mutation, and cleanup order.
pub fn apply_cities_capture_local_award_notification<
    W: CitiesCaptureLocalAwardNotificationWorld + ?Sized,
>(
    plan: CitiesCaptureLocalAwardNotificationPlan,
    world: &mut W,
) -> Result<CitiesCaptureLocalAwardNotificationReceipt, CitiesCaptureLocalAwardNotificationApplyError>
{
    let mut events = Vec::new();
    let module_id = world
        .read_string_module_id()
        .ok_or(CitiesCaptureLocalAwardNotificationApplyError::MissingStringModuleId)?;
    events.push(CitiesCaptureLocalAwardNotificationEvent::ReadStringModuleId(module_id));
    for slot in [
        LocalStringSlot::Message0x00734152,
        LocalStringSlot::Bubble0x00734192,
        LocalStringSlot::Unused0x007341c1,
    ] {
        events.push(CitiesCaptureLocalAwardNotificationEvent::InitializeString(
            slot,
        ));
    }

    let bubble_template_request = LocalizedStringRequest {
        byte_offset: AWARD_BUBBLE_TEMPLATE_BYTE_OFFSET,
    };
    let bubble_template = world
        .read_localized_string(bubble_template_request)
        .ok_or(CitiesCaptureLocalAwardNotificationApplyError::MissingLocalizedString)?;
    if bubble_template.request != bubble_template_request {
        return Err(CitiesCaptureLocalAwardNotificationApplyError::LocalizedStringReceiptMismatch);
    }
    events.push(CitiesCaptureLocalAwardNotificationEvent::ReadLocalizedString(bubble_template));
    let bubble_template_value = assign(
        world,
        &mut events,
        StringAssignRequest {
            phase: StringAssignPhase::BubbleTemplate0x007341ee,
            target: LocalStringSlot::Bubble0x00734192,
            value_before: None,
            source: bubble_template.value,
        },
    )?;

    let amount_parse_request = ParseAmountRequest {
        template: bubble_template_value,
        amount: plan.award,
        temporary: LocalStringSlot::ParseAmountTemporary0x007341f6,
    };
    let amount_parse = world
        .parse_amount(amount_parse_request)
        .ok_or(CitiesCaptureLocalAwardNotificationApplyError::MissingAmountParse)?;
    if amount_parse.request != amount_parse_request {
        return Err(CitiesCaptureLocalAwardNotificationApplyError::AmountParseMismatch);
    }
    events.push(CitiesCaptureLocalAwardNotificationEvent::ParseAmount(
        amount_parse,
    ));
    let bubble_text = assign(
        world,
        &mut events,
        StringAssignRequest {
            phase: StringAssignPhase::BubbleParsedAmount0x00734214,
            target: LocalStringSlot::Bubble0x00734192,
            value_before: Some(bubble_template_value),
            source: amount_parse.parsed,
        },
    )?;
    close(
        world,
        &mut events,
        StringCloseRequest {
            slot: LocalStringSlot::ParseAmountTemporary0x007341f6,
            value: Some(amount_parse.parsed),
        },
    )?;

    let message_template_request = LocalizedStringRequest {
        byte_offset: AWARD_MESSAGE_TEMPLATE_BYTE_OFFSET,
    };
    let message_template = world
        .read_localized_string(message_template_request)
        .ok_or(CitiesCaptureLocalAwardNotificationApplyError::MissingLocalizedString)?;
    if message_template.request != message_template_request {
        return Err(CitiesCaptureLocalAwardNotificationApplyError::LocalizedStringReceiptMismatch);
    }
    events.push(CitiesCaptureLocalAwardNotificationEvent::ReadLocalizedString(message_template));
    let message_template_value = assign(
        world,
        &mut events,
        StringAssignRequest {
            phase: StringAssignPhase::MessageTemplate0x00734239,
            target: LocalStringSlot::Message0x00734152,
            value_before: None,
            source: message_template.value,
        },
    )?;

    let city_name_request = CityNameRequest {
        city: plan.new_city,
        byte_offset: CITY_NAME_BYTE_OFFSET,
    };
    let city_name = world
        .read_city_name(city_name_request)
        .ok_or(CitiesCaptureLocalAwardNotificationApplyError::MissingCityName)?;
    if city_name.request != city_name_request {
        return Err(CitiesCaptureLocalAwardNotificationApplyError::CityNameMismatch);
    }
    events.push(CitiesCaptureLocalAwardNotificationEvent::ReadCityName(
        city_name,
    ));
    let city_parse_request = ParseCityAmountRequest {
        template: message_template_value,
        city_name: city_name.name,
        amount: plan.award,
        temporary: LocalStringSlot::ParseMessageTemporary0x00734261,
    };
    let city_parse = world
        .parse_city_amount(city_parse_request)
        .ok_or(CitiesCaptureLocalAwardNotificationApplyError::MissingCityAmountParse)?;
    if city_parse.request != city_parse_request {
        return Err(CitiesCaptureLocalAwardNotificationApplyError::CityAmountParseMismatch);
    }
    events.push(CitiesCaptureLocalAwardNotificationEvent::ParseCityAmount(
        city_parse,
    ));
    let message = assign(
        world,
        &mut events,
        StringAssignRequest {
            phase: StringAssignPhase::MessageParsedCityAmount0x00734278,
            target: LocalStringSlot::Message0x00734152,
            value_before: Some(message_template_value),
            source: city_parse.parsed,
        },
    )?;
    close(
        world,
        &mut events,
        StringCloseRequest {
            slot: LocalStringSlot::ParseMessageTemporary0x00734261,
            value: Some(city_parse.parsed),
        },
    )?;

    let bubble = world
        .pop_text_bubble()
        .ok_or(CitiesCaptureLocalAwardNotificationApplyError::MissingTextBubble)?;
    events.push(CitiesCaptureLocalAwardNotificationEvent::PopTextBubble(
        bubble,
    ));
    let bubble_text_request = BubbleTextWriteRequest {
        bubble: bubble.bubble,
        value: bubble_text,
    };
    let bubble_text_write = world
        .write_bubble_text(bubble_text_request)
        .ok_or(CitiesCaptureLocalAwardNotificationApplyError::MissingBubbleTextWrite)?;
    if bubble_text_write.request != bubble_text_request {
        return Err(CitiesCaptureLocalAwardNotificationApplyError::BubbleTextWriteMismatch);
    }
    if !bubble_text_write.mutation_applied {
        return Err(
            CitiesCaptureLocalAwardNotificationApplyError::BubbleTextWriteEffectsIncomplete,
        );
    }
    events.push(CitiesCaptureLocalAwardNotificationEvent::WriteBubbleText(
        bubble_text_write,
    ));

    let bubble_owner_request = BubbleOwnerWriteRequest {
        bubble: bubble.bubble,
        owner: plan.new_owner,
    };
    let bubble_owner_write = world
        .write_bubble_owner(bubble_owner_request)
        .ok_or(CitiesCaptureLocalAwardNotificationApplyError::MissingBubbleOwnerWrite)?;
    if bubble_owner_write.request != bubble_owner_request {
        return Err(CitiesCaptureLocalAwardNotificationApplyError::BubbleOwnerWriteMismatch);
    }
    if !bubble_owner_write.mutation_applied {
        return Err(
            CitiesCaptureLocalAwardNotificationApplyError::BubbleOwnerWriteEffectsIncomplete,
        );
    }
    events.push(CitiesCaptureLocalAwardNotificationEvent::WriteBubbleOwner(
        bubble_owner_write,
    ));

    let color_index_request = OwnerColorIndexRequest {
        owner: plan.new_owner,
    };
    let color_index = world
        .read_owner_color_index(color_index_request)
        .ok_or(CitiesCaptureLocalAwardNotificationApplyError::MissingOwnerColorIndex)?;
    if color_index.request != color_index_request {
        return Err(CitiesCaptureLocalAwardNotificationApplyError::OwnerColorIndexMismatch);
    }
    events.push(CitiesCaptureLocalAwardNotificationEvent::ReadOwnerColorIndex(color_index));
    let team_color = TeamColorKey {
        color_index: color_index.color_index,
        entry_stride: TEAM_COLOR_ENTRY_STRIDE,
    };
    let neon_request = NeonColorRequest { team_color };
    let neon = world
        .get_neon_color(neon_request)
        .ok_or(CitiesCaptureLocalAwardNotificationApplyError::MissingNeonColor)?;
    if neon.request != neon_request {
        return Err(CitiesCaptureLocalAwardNotificationApplyError::NeonColorMismatch);
    }
    events.push(CitiesCaptureLocalAwardNotificationEvent::GetNeonColor(neon));

    let coordinates_request = CityCoordinatesRequest {
        city: plan.new_city,
    };
    let coordinates = world
        .read_city_coordinates(coordinates_request)
        .ok_or(CitiesCaptureLocalAwardNotificationApplyError::MissingCityCoordinates)?;
    if coordinates.request != coordinates_request {
        return Err(CitiesCaptureLocalAwardNotificationApplyError::CityCoordinatesMismatch);
    }
    events.push(CitiesCaptureLocalAwardNotificationEvent::ReadCityCoordinates(coordinates));
    let add_message_request = AddMessageRequest {
        message,
        x: coordinates.x,
        y: coordinates.y,
        category: MESSAGE_CATEGORY,
        color: neon.color,
        duration: MESSAGE_DURATION,
        unused_team_color_0: team_color,
        height: MESSAGE_HEIGHT,
        bubble: bubble.bubble,
        unused_team_color_1: team_color,
    };
    let add_message = world
        .add_message(add_message_request)
        .ok_or(CitiesCaptureLocalAwardNotificationApplyError::MissingAddMessage)?;
    if add_message.request != add_message_request {
        return Err(CitiesCaptureLocalAwardNotificationApplyError::AddMessageMismatch);
    }
    if !add_message.mutation_applied {
        return Err(CitiesCaptureLocalAwardNotificationApplyError::AddMessageEffectsIncomplete);
    }
    events.push(CitiesCaptureLocalAwardNotificationEvent::AddMessage(
        add_message,
    ));
    events.push(CitiesCaptureLocalAwardNotificationEvent::SetNotificationRaised(true));

    close(
        world,
        &mut events,
        StringCloseRequest {
            slot: LocalStringSlot::Unused0x007341c1,
            value: None,
        },
    )?;
    close(
        world,
        &mut events,
        StringCloseRequest {
            slot: LocalStringSlot::Bubble0x00734192,
            value: Some(bubble_text),
        },
    )?;
    close(
        world,
        &mut events,
        StringCloseRequest {
            slot: LocalStringSlot::Message0x00734152,
            value: Some(message),
        },
    )?;

    Ok(CitiesCaptureLocalAwardNotificationReceipt {
        prior: plan.prior,
        new_city: plan.new_city,
        new_owner: plan.new_owner,
        award: plan.award,
        old_owner_refund: plan.old_owner_refund,
        notification_raised: true,
        continuation: CitiesCaptureLocalAwardNotificationContinuation::OldOwnerRefund0x0073432d,
        events,
    })
}
