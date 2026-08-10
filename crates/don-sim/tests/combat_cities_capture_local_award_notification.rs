use don_sim::systems::combat::{build_check_capture, cities_capture_prefix, damage_world};

#[path = "../src/systems/combat/cities_capture_local_award_notification.rs"]
mod cities_capture_local_award_notification;
#[path = "../src/systems/combat/cities_capture_plunder_award.rs"]
mod cities_capture_plunder_award;
#[path = "../src/systems/combat/cities_capture_plunder_gate.rs"]
mod cities_capture_plunder_gate;
#[path = "../src/systems/combat/cities_capture_swap_fork.rs"]
mod cities_capture_swap_fork;

use build_check_capture::CityKey;
use cities_capture_local_award_notification::*;
use cities_capture_plunder_award::{
    CitiesCapturePlunderAwardContinuation, CitiesCapturePlunderAwardReceipt,
};
use cities_capture_plunder_gate::{
    CitiesCapturePlunderGateContinuation, CitiesCapturePlunderGateReceipt,
};
use cities_capture_swap_fork::{CitiesCaptureSwapForkContinuation, CitiesCaptureSwapForkReceipt};
use damage_world::ObjectKey;

const OLD_CITY: CityKey = CityKey { who: 1, city: 6 };
const NEW_CITY: CityKey = CityKey { who: 2, city: 9 };

fn prior_award(
    continuation: CitiesCapturePlunderAwardContinuation,
) -> CitiesCapturePlunderAwardReceipt {
    let swap = CitiesCaptureSwapForkReceipt {
        old_city_objects: vec![ObjectKey { who: 1, o: 40 }],
        new_city_objects: vec![ObjectKey { who: 2, o: 70 }],
        new_city: Some(NEW_CITY),
        plunder_accumulator: 500,
        continuation: CitiesCaptureSwapForkContinuation::Converged0x00733d67,
        mutations: vec![],
    };
    let gate = CitiesCapturePlunderGateReceipt {
        prior: swap,
        old_city: OLD_CITY,
        new_owner: 2,
        captured_own_capital: false,
        qualifying_general: false,
        russian_plunder_steal: false,
        notification_raised: false,
        city_flags: 0x10,
        capture_stamp: Some(0),
        continuation: CitiesCapturePlunderGateContinuation::EnterPlunder0x00733fac,
        events: vec![],
    };
    CitiesCapturePlunderAwardReceipt {
        prior: gate,
        sized_capital_plunder: Some(500),
        new_owner_award: Some(500),
        old_owner_refund: Some(0),
        continuation,
        events: vec![],
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HostEvent {
    ModuleId,
    Localized(LocalizedStringRequest),
    Assign(StringAssignRequest),
    ParseAmount(ParseAmountRequest),
    CityName(CityNameRequest),
    ParseCityAmount(ParseCityAmountRequest),
    Close(StringCloseRequest),
    PopBubble,
    BubbleText(BubbleTextWriteRequest),
    BubbleOwner(BubbleOwnerWriteRequest),
    ColorIndex(OwnerColorIndexRequest),
    Neon(NeonColorRequest),
    Coordinates(CityCoordinatesRequest),
    AddMessage(AddMessageRequest),
}

struct World {
    events: Vec<HostEvent>,
    corrupt_assignment: bool,
    suppress_add_message: bool,
    corrupt_close: Option<LocalStringSlot>,
}

impl Default for World {
    fn default() -> Self {
        Self {
            events: vec![],
            corrupt_assignment: false,
            suppress_add_message: false,
            corrupt_close: None,
        }
    }
}

impl CitiesCaptureLocalAwardNotificationWorld for World {
    fn read_string_module_id(&mut self) -> Option<StringModuleIdReceipt> {
        self.events.push(HostEvent::ModuleId);
        Some(StringModuleIdReceipt { module_id: 7 })
    }

    fn read_localized_string(
        &mut self,
        request: LocalizedStringRequest,
    ) -> Option<LocalizedStringReceipt> {
        self.events.push(HostEvent::Localized(request));
        let value = match request.byte_offset {
            AWARD_BUBBLE_TEMPLATE_BYTE_OFFSET => StringValueToken(10),
            AWARD_MESSAGE_TEMPLATE_BYTE_OFFSET => StringValueToken(20),
            other => panic!("unexpected localized-string offset {other:#x}"),
        };
        Some(LocalizedStringReceipt { request, value })
    }

    fn assign_string(&mut self, request: StringAssignRequest) -> Option<StringAssignReceipt> {
        self.events.push(HostEvent::Assign(request));
        Some(StringAssignReceipt {
            request,
            value_after: if self.corrupt_assignment {
                StringValueToken(request.source.0 + 1)
            } else {
                request.source
            },
            mutation_applied: true,
        })
    }

    fn parse_amount(&mut self, request: ParseAmountRequest) -> Option<ParseAmountReceipt> {
        self.events.push(HostEvent::ParseAmount(request));
        Some(ParseAmountReceipt {
            request,
            parsed: StringValueToken(30),
        })
    }

    fn read_city_name(&mut self, request: CityNameRequest) -> Option<CityNameReceipt> {
        self.events.push(HostEvent::CityName(request));
        Some(CityNameReceipt {
            request,
            name: StringValueToken(40),
        })
    }

    fn parse_city_amount(
        &mut self,
        request: ParseCityAmountRequest,
    ) -> Option<ParseCityAmountReceipt> {
        self.events.push(HostEvent::ParseCityAmount(request));
        Some(ParseCityAmountReceipt {
            request,
            parsed: StringValueToken(50),
        })
    }

    fn close_string(&mut self, request: StringCloseRequest) -> Option<StringCloseReceipt> {
        self.events.push(HostEvent::Close(request));
        Some(StringCloseReceipt {
            request,
            closed: self.corrupt_close != Some(request.slot),
        })
    }

    fn pop_text_bubble(&mut self) -> Option<TextBubblePopReceipt> {
        self.events.push(HostEvent::PopBubble);
        Some(TextBubblePopReceipt {
            bubble: TextBubbleToken(60),
        })
    }

    fn write_bubble_text(
        &mut self,
        request: BubbleTextWriteRequest,
    ) -> Option<BubbleTextWriteReceipt> {
        self.events.push(HostEvent::BubbleText(request));
        Some(BubbleTextWriteReceipt {
            request,
            mutation_applied: true,
        })
    }

    fn write_bubble_owner(
        &mut self,
        request: BubbleOwnerWriteRequest,
    ) -> Option<BubbleOwnerWriteReceipt> {
        self.events.push(HostEvent::BubbleOwner(request));
        Some(BubbleOwnerWriteReceipt {
            request,
            mutation_applied: true,
        })
    }

    fn read_owner_color_index(
        &mut self,
        request: OwnerColorIndexRequest,
    ) -> Option<OwnerColorIndexReceipt> {
        self.events.push(HostEvent::ColorIndex(request));
        Some(OwnerColorIndexReceipt {
            request,
            color_index: 4,
        })
    }

    fn get_neon_color(&mut self, request: NeonColorRequest) -> Option<NeonColorReceipt> {
        self.events.push(HostEvent::Neon(request));
        Some(NeonColorReceipt {
            request,
            color: ColorToken(70),
        })
    }

    fn read_city_coordinates(
        &mut self,
        request: CityCoordinatesRequest,
    ) -> Option<CityCoordinatesReceipt> {
        self.events.push(HostEvent::Coordinates(request));
        Some(CityCoordinatesReceipt {
            request,
            x: 1_000,
            y: 2_000,
        })
    }

    fn add_message(&mut self, request: AddMessageRequest) -> Option<AddMessageReceipt> {
        self.events.push(HostEvent::AddMessage(request));
        Some(AddMessageReceipt {
            request,
            mutation_applied: !self.suppress_add_message,
        })
    }
}

fn run(
    world: &mut World,
) -> Result<CitiesCaptureLocalAwardNotificationReceipt, CitiesCaptureLocalAwardNotificationApplyError>
{
    let plan = plan_cities_capture_local_award_notification(prior_award(
        CitiesCapturePlunderAwardContinuation::LocalAwardNotification0x00734152,
    ))
    .unwrap();
    apply_cities_capture_local_award_notification(plan, world)
}

#[test]
fn exact_local_award_notification_boundary_and_residual_are_frozen() {
    assert_eq!(CITIES_CAPTURE_LOCAL_AWARD_NOTIFICATION_START, 0x0073_4152);
    assert_eq!(CITIES_CAPTURE_LOCAL_AWARD_NOTIFICATION_END, 0x0073_432D);
    assert_eq!(CITIES_CAPTURE_LOCAL_AWARD_NOTIFICATION_SIZE, 0x1DB);
    assert_eq!(CITIES_CAPTURE_LOCAL_AWARD_NOTIFICATION_RESIDUAL_SIZE, 0xF91);
    assert_eq!(AWARD_BUBBLE_TEMPLATE_BYTE_OFFSET, 0x1504);
    assert_eq!(AWARD_MESSAGE_TEMPLATE_BYTE_OFFSET, 0x1518);
    assert_eq!(CITY_NAME_BYTE_OFFSET, 0x90);
}

#[test]
fn local_award_orders_strings_bubble_color_message_and_reverse_cleanup() {
    let mut world = World::default();
    let receipt = run(&mut world).unwrap();

    assert_eq!(receipt.new_city, NEW_CITY);
    assert_eq!(receipt.new_owner, 2);
    assert_eq!(receipt.award, 500);
    assert_eq!(receipt.old_owner_refund, 0);
    assert!(receipt.notification_raised);
    assert_eq!(
        receipt.continuation,
        CitiesCaptureLocalAwardNotificationContinuation::OldOwnerRefund0x0073432d
    );
    assert!(matches!(
        receipt.events.as_slice(),
        [
            CitiesCaptureLocalAwardNotificationEvent::ReadStringModuleId(_),
            CitiesCaptureLocalAwardNotificationEvent::InitializeString(
                LocalStringSlot::Message0x00734152
            ),
            CitiesCaptureLocalAwardNotificationEvent::InitializeString(
                LocalStringSlot::Bubble0x00734192
            ),
            CitiesCaptureLocalAwardNotificationEvent::InitializeString(
                LocalStringSlot::Unused0x007341c1
            ),
            ..,
            CitiesCaptureLocalAwardNotificationEvent::AddMessage(_),
            CitiesCaptureLocalAwardNotificationEvent::SetNotificationRaised(true),
            CitiesCaptureLocalAwardNotificationEvent::CloseString(StringCloseReceipt {
                request: StringCloseRequest {
                    slot: LocalStringSlot::Unused0x007341c1,
                    ..
                },
                ..
            }),
            CitiesCaptureLocalAwardNotificationEvent::CloseString(StringCloseReceipt {
                request: StringCloseRequest {
                    slot: LocalStringSlot::Bubble0x00734192,
                    ..
                },
                ..
            }),
            CitiesCaptureLocalAwardNotificationEvent::CloseString(StringCloseReceipt {
                request: StringCloseRequest {
                    slot: LocalStringSlot::Message0x00734152,
                    ..
                },
                ..
            }),
        ]
    ));

    let phases: Vec<_> = world
        .events
        .iter()
        .filter_map(|event| match event {
            HostEvent::Assign(request) => Some(request.phase),
            _ => None,
        })
        .collect();
    assert_eq!(
        phases,
        vec![
            StringAssignPhase::BubbleTemplate0x007341ee,
            StringAssignPhase::BubbleParsedAmount0x00734214,
            StringAssignPhase::MessageTemplate0x00734239,
            StringAssignPhase::MessageParsedCityAmount0x00734278,
        ]
    );
}

#[test]
fn message_call_freezes_coordinates_constants_color_identity_and_bubble() {
    let mut world = World::default();
    run(&mut world).unwrap();
    let request = world
        .events
        .iter()
        .find_map(|event| match event {
            HostEvent::AddMessage(request) => Some(*request),
            _ => None,
        })
        .unwrap();
    let team_color = TeamColorKey {
        color_index: 4,
        entry_stride: TEAM_COLOR_ENTRY_STRIDE,
    };
    assert_eq!(
        request,
        AddMessageRequest {
            message: StringValueToken(50),
            x: 1_000,
            y: 2_000,
            category: MESSAGE_CATEGORY,
            color: ColorToken(70),
            duration: MESSAGE_DURATION,
            unused_team_color_0: team_color,
            height: MESSAGE_HEIGHT,
            bubble: TextBubbleToken(60),
            unused_team_color_1: team_color,
        }
    );
}

#[test]
fn city_name_is_read_before_bubble_mutations_but_coordinates_are_read_after_neon() {
    let mut world = World::default();
    run(&mut world).unwrap();
    let position =
        |predicate: fn(&HostEvent) -> bool| world.events.iter().position(predicate).unwrap();
    let name = position(|event| matches!(event, HostEvent::CityName(_)));
    let bubble = position(|event| matches!(event, HostEvent::PopBubble));
    let neon = position(|event| matches!(event, HostEvent::Neon(_)));
    let coordinates = position(|event| matches!(event, HostEvent::Coordinates(_)));
    let message = position(|event| matches!(event, HostEvent::AddMessage(_)));
    assert!(name < bubble);
    assert!(bubble < neon);
    assert!(neon < coordinates);
    assert!(coordinates < message);
}

#[test]
fn planner_rejects_every_nonlocal_or_impossible_prior_seam() {
    assert_eq!(
        plan_cities_capture_local_award_notification(prior_award(
            CitiesCapturePlunderAwardContinuation::OldOwnerRefund0x0073432d,
        )),
        Err(CitiesCaptureLocalAwardNotificationPlanError::PriorContinuationMismatch)
    );

    let mut zero =
        prior_award(CitiesCapturePlunderAwardContinuation::LocalAwardNotification0x00734152);
    zero.new_owner_award = Some(0);
    assert_eq!(
        plan_cities_capture_local_award_notification(zero),
        Err(CitiesCaptureLocalAwardNotificationPlanError::ZeroAward)
    );

    let mut missing_refund =
        prior_award(CitiesCapturePlunderAwardContinuation::LocalAwardNotification0x00734152);
    missing_refund.old_owner_refund = None;
    assert_eq!(
        plan_cities_capture_local_award_notification(missing_refund),
        Err(CitiesCaptureLocalAwardNotificationPlanError::MissingOldOwnerRefund)
    );

    let mut already_raised =
        prior_award(CitiesCapturePlunderAwardContinuation::LocalAwardNotification0x00734152);
    already_raised.prior.notification_raised = true;
    assert_eq!(
        plan_cities_capture_local_award_notification(already_raised),
        Err(CitiesCaptureLocalAwardNotificationPlanError::NotificationAlreadyRaised)
    );

    let mut mismatched =
        prior_award(CitiesCapturePlunderAwardContinuation::LocalAwardNotification0x00734152);
    mismatched.prior.prior.new_city = Some(CityKey { who: 3, city: 9 });
    assert_eq!(
        plan_cities_capture_local_award_notification(mismatched),
        Err(CitiesCaptureLocalAwardNotificationPlanError::NewOwnerMismatch)
    );
}

#[test]
fn assignment_message_and_cleanup_receipts_fail_closed_at_commit_points() {
    let mut assignment = World {
        corrupt_assignment: true,
        ..World::default()
    };
    assert_eq!(
        run(&mut assignment),
        Err(CitiesCaptureLocalAwardNotificationApplyError::StringAssignmentMismatch)
    );

    let mut message = World {
        suppress_add_message: true,
        ..World::default()
    };
    assert_eq!(
        run(&mut message),
        Err(CitiesCaptureLocalAwardNotificationApplyError::AddMessageEffectsIncomplete)
    );

    let mut close = World {
        corrupt_close: Some(LocalStringSlot::Unused0x007341c1),
        ..World::default()
    };
    assert_eq!(
        run(&mut close),
        Err(CitiesCaptureLocalAwardNotificationApplyError::StringCloseEffectsIncomplete)
    );
}
