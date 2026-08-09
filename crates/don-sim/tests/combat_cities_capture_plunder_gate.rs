use don_sim::systems::combat::{build_check_capture, cities_capture_prefix, damage_world};

#[path = "../src/systems/combat/cities_capture_plunder_gate.rs"]
mod cities_capture_plunder_gate;
#[path = "../src/systems/combat/cities_capture_swap_fork.rs"]
mod cities_capture_swap_fork;

use build_check_capture::CityKey;
use cities_capture_plunder_gate::{
    apply_cities_capture_plunder_gate, game_version_is_at_most_retail,
    plan_cities_capture_plunder_gate, CaptureFrameReceipt, CaptureStampReceipt,
    CaptureStampRequest, CapturedCenterGeneralInspectionReceipt,
    CapturedCenterGeneralInspectionRequest, CapturedCityFlagsReceipt, CapturedCityFlagsRequest,
    CitiesCapturePlunderGateApplyError, CitiesCapturePlunderGateContinuation,
    CitiesCapturePlunderGateEvent, CitiesCapturePlunderGateWorld, FindDespotReceipt,
    FindDespotRequest, HasSpitamenesReceipt, HasSpitamenesRequest, RussianPlunderInspectionReceipt,
    RussianPlunderInspectionRequest, CAPITAL_CITY_FLAG, CAPTURE_PLUNDER_COOLDOWN_FRAMES,
    CITIES_CAPTURE_PLUNDER_GATE_END, CITIES_CAPTURE_PLUNDER_GATE_RESIDUAL_SIZE,
    CITIES_CAPTURE_PLUNDER_GATE_SIZE, CITIES_CAPTURE_PLUNDER_GATE_START, DESPOT_ROSTER_SLOT,
    HERO_SEARCH_EXTENT_SCALE, PLUNDER_PROTECTED_CITY_FLAG, RUSSIAN_TRIBE_BONUS_INDEX,
    SPITAMENES_ROSTER_SLOT, SPITAMENES_TYPE, THE_DESPOT_TYPE,
};
use cities_capture_swap_fork::{CitiesCaptureSwapForkContinuation, CitiesCaptureSwapForkReceipt};
use damage_world::ObjectKey;

const OLD_CITY: CityKey = CityKey { who: 1, city: 6 };
const NEW_CITY: CityKey = CityKey { who: 2, city: 9 };
const NEW_CENTER: ObjectKey = ObjectKey { who: 2, o: 70 };

fn prior(plunder_accumulator: i32) -> CitiesCaptureSwapForkReceipt {
    CitiesCaptureSwapForkReceipt {
        old_city_objects: vec![ObjectKey { who: 1, o: 40 }],
        new_city_objects: vec![NEW_CENTER],
        new_city: Some(NEW_CITY),
        plunder_accumulator,
        continuation: CitiesCaptureSwapForkContinuation::Converged0x00733d67,
        mutations: vec![],
    }
}

fn center_request() -> CapturedCenterGeneralInspectionRequest {
    CapturedCenterGeneralInspectionRequest {
        new_city: NEW_CITY,
        expected_center: NEW_CENTER,
        new_owner: 2,
    }
}

fn direct_despot() -> CapturedCenterGeneralInspectionReceipt {
    CapturedCenterGeneralInspectionReceipt {
        request: center_request(),
        exact_query_order_attested: true,
        object_is_unit: true,
        object_is_hero: Some(true),
        object_is_despot: Some(true),
        object_is_build: None,
        build_size_x: None,
        build_size_y: None,
        center_x: 1_000,
        center_y: 2_000,
        center_owner_who: 2,
        despot_roster_count: None,
        find_despot: None,
        spitamenes_roster_count: 0,
        has_spitamenes: None,
    }
}

fn russian_request(captured_own_capital: bool) -> RussianPlunderInspectionRequest {
    RussianPlunderInspectionRequest {
        old_owner: 1,
        captured_own_capital,
        tribe_bonus_index: RUSSIAN_TRIBE_BONUS_INDEX,
    }
}

fn russian_enabled(captured_own_capital: bool) -> RussianPlunderInspectionReceipt {
    RussianPlunderInspectionReceipt {
        request: russian_request(captured_own_capital),
        has_russian_bonus: true,
        russian_plunder_steal_rule: Some(1),
        game_version: Some(0x0102_0304),
        retail_version: Some(0x0102_0304),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HostEvent {
    Center(CapturedCenterGeneralInspectionRequest),
    Russian(RussianPlunderInspectionRequest),
    Flags(CapturedCityFlagsRequest),
    Stamp(CaptureStampRequest),
    Frame,
}

struct World {
    events: Vec<HostEvent>,
    center: CapturedCenterGeneralInspectionReceipt,
    russian: RussianPlunderInspectionReceipt,
    city_flags: u16,
    capture_stamp: i32,
    frame: i32,
}

impl CitiesCapturePlunderGateWorld for World {
    fn inspect_captured_center_generals(
        &mut self,
        request: CapturedCenterGeneralInspectionRequest,
    ) -> Option<CapturedCenterGeneralInspectionReceipt> {
        self.events.push(HostEvent::Center(request));
        Some(self.center)
    }

    fn inspect_russian_plunder(
        &mut self,
        request: RussianPlunderInspectionRequest,
    ) -> Option<RussianPlunderInspectionReceipt> {
        self.events.push(HostEvent::Russian(request));
        Some(self.russian)
    }

    fn read_captured_city_flags(
        &mut self,
        request: CapturedCityFlagsRequest,
    ) -> Option<CapturedCityFlagsReceipt> {
        self.events.push(HostEvent::Flags(request));
        Some(CapturedCityFlagsReceipt {
            request,
            city_flags: self.city_flags,
        })
    }

    fn read_capture_stamp(&mut self, request: CaptureStampRequest) -> Option<CaptureStampReceipt> {
        self.events.push(HostEvent::Stamp(request));
        Some(CaptureStampReceipt {
            request,
            capture_stamp: self.capture_stamp,
        })
    }

    fn read_capture_frame(&mut self) -> Option<CaptureFrameReceipt> {
        self.events.push(HostEvent::Frame);
        Some(CaptureFrameReceipt { frame: self.frame })
    }
}

fn world(plunder_capital: bool) -> World {
    World {
        events: vec![],
        center: direct_despot(),
        russian: russian_enabled(plunder_capital),
        city_flags: CAPITAL_CITY_FLAG,
        capture_stamp: 0,
        frame: 10_000,
    }
}

#[test]
fn exact_gate_and_residual_addresses_are_frozen() {
    assert_eq!(CITIES_CAPTURE_PLUNDER_GATE_START, 0x0073_3D67);
    assert_eq!(CITIES_CAPTURE_PLUNDER_GATE_END, 0x0073_3FAC);
    assert_eq!(CITIES_CAPTURE_PLUNDER_GATE_SIZE, 0x245);
    assert_eq!(CITIES_CAPTURE_PLUNDER_GATE_RESIDUAL_SIZE, 0x1312);
    assert_eq!(DESPOT_ROSTER_SLOT, 302);
    assert_eq!(SPITAMENES_ROSTER_SLOT, 315);
}

#[test]
fn direct_despot_orders_queries_and_three_local_initializers() {
    let plan = plan_cities_capture_plunder_gate(prior(200), OLD_CITY, 2, true).unwrap();
    let mut world = world(true);
    let receipt = apply_cities_capture_plunder_gate(plan, &mut world).unwrap();

    assert!(receipt.qualifying_general);
    assert!(receipt.russian_plunder_steal);
    assert!(!receipt.notification_raised);
    assert_eq!(receipt.old_city, OLD_CITY);
    assert_eq!(receipt.new_owner, 2);
    assert!(receipt.captured_own_capital);
    assert_eq!(
        receipt.continuation,
        CitiesCapturePlunderGateContinuation::EnterPlunder0x00733fac
    );
    assert_eq!(
        world.events,
        vec![
            HostEvent::Center(center_request()),
            HostEvent::Russian(russian_request(true)),
            HostEvent::Flags(CapturedCityFlagsRequest { old_city: OLD_CITY }),
            HostEvent::Stamp(CaptureStampRequest { old_city: OLD_CITY }),
        ]
    );
    assert!(matches!(
        receipt.events.as_slice(),
        [
            CitiesCapturePlunderGateEvent::InitializeRussianPlunderSteal(false),
            CitiesCapturePlunderGateEvent::InitializeNotificationRaised(false),
            CitiesCapturePlunderGateEvent::InitializeQualifyingGeneral(false),
            CitiesCapturePlunderGateEvent::InspectCapturedCenter(_),
            CitiesCapturePlunderGateEvent::SetQualifyingGeneral(true),
            CitiesCapturePlunderGateEvent::InspectRussianPlunder(_),
            CitiesCapturePlunderGateEvent::SetRussianPlunderSteal(true),
            CitiesCapturePlunderGateEvent::ReadCapturedCityFlags(_),
            CitiesCapturePlunderGateEvent::ReadCaptureStamp(_),
        ]
    ));
}

#[test]
fn fallback_build_uses_exact_find_extent_then_spitamenes_projection() {
    let extent = (2_i32 + 3).wrapping_mul(HERO_SEARCH_EXTENT_SCALE);
    let find_request = FindDespotRequest {
        x: 1_000,
        y: 2_000,
        center_owner_who: 2,
        zero: 0,
        type_index: THE_DESPOT_TYPE,
        search_extent: extent,
    };
    let has_request = HasSpitamenesRequest {
        center: NEW_CENTER,
        first: 0,
        type_index: SPITAMENES_TYPE,
    };
    let mut world = world(false);
    world.center = CapturedCenterGeneralInspectionReceipt {
        request: center_request(),
        exact_query_order_attested: true,
        object_is_unit: false,
        object_is_hero: None,
        object_is_despot: None,
        object_is_build: Some(true),
        build_size_x: Some(2),
        build_size_y: Some(3),
        center_x: 1_000,
        center_y: 2_000,
        center_owner_who: 2,
        despot_roster_count: Some(1),
        find_despot: Some(FindDespotReceipt {
            request: find_request,
            result_o: -1,
        }),
        spitamenes_roster_count: 1,
        has_spitamenes: Some(HasSpitamenesReceipt {
            request: has_request,
            result_o: 77,
        }),
    };
    world.russian = RussianPlunderInspectionReceipt {
        request: russian_request(false),
        has_russian_bonus: false,
        russian_plunder_steal_rule: None,
        game_version: None,
        retail_version: None,
    };
    let plan = plan_cities_capture_plunder_gate(prior(200), OLD_CITY, 2, false).unwrap();
    let receipt = apply_cities_capture_plunder_gate(plan, &mut world).unwrap();
    assert!(receipt.qualifying_general);
    assert!(!receipt.russian_plunder_steal);
}

#[test]
fn mutated_find_hero_tuple_is_rejected_before_later_host_queries() {
    let mut world = world(false);
    world.center = CapturedCenterGeneralInspectionReceipt {
        request: center_request(),
        exact_query_order_attested: true,
        object_is_unit: false,
        object_is_hero: None,
        object_is_despot: None,
        object_is_build: Some(false),
        build_size_x: None,
        build_size_y: None,
        center_x: 10,
        center_y: 20,
        center_owner_who: 2,
        despot_roster_count: Some(1),
        find_despot: Some(FindDespotReceipt {
            request: FindDespotRequest {
                x: 10,
                y: 20,
                center_owner_who: 2,
                zero: 1,
                type_index: THE_DESPOT_TYPE,
                search_extent: 0,
            },
            result_o: 40,
        }),
        spitamenes_roster_count: 0,
        has_spitamenes: None,
    };
    let plan = plan_cities_capture_plunder_gate(prior(200), OLD_CITY, 2, false).unwrap();
    assert_eq!(
        apply_cities_capture_plunder_gate(plan, &mut world),
        Err(CitiesCapturePlunderGateApplyError::FindDespotReceiptMismatch)
    );
    assert_eq!(world.events, vec![HostEvent::Center(center_request())]);
}

#[test]
fn protected_and_zero_noncapital_cities_skip_stamp_and_frame_reads() {
    for (plunder, flags) in [(200, PLUNDER_PROTECTED_CITY_FLAG), (0, 0)] {
        let plan = plan_cities_capture_plunder_gate(prior(plunder), OLD_CITY, 2, false).unwrap();
        let mut world = world(false);
        world.russian.request = russian_request(false);
        world.city_flags = flags;
        let receipt = apply_cities_capture_plunder_gate(plan, &mut world).unwrap();
        assert_eq!(
            receipt.continuation,
            CitiesCapturePlunderGateContinuation::SkipPlunder0x00734a3c
        );
        assert!(receipt.capture_stamp.is_none());
        assert!(!world
            .events
            .iter()
            .any(|event| matches!(event, HostEvent::Stamp(_) | HostEvent::Frame)));
    }
}

#[test]
fn signed_cooldown_comparison_is_inclusive_and_wrapping() {
    let plan = plan_cities_capture_plunder_gate(prior(1), OLD_CITY, 2, false).unwrap();
    let mut world = world(false);
    world.russian.request = russian_request(false);
    world.capture_stamp = 10_000;
    world.frame = 10_000_i32.wrapping_add(CAPTURE_PLUNDER_COOLDOWN_FRAMES);
    let receipt = apply_cities_capture_plunder_gate(plan, &mut world).unwrap();
    assert_eq!(
        receipt.continuation,
        CitiesCapturePlunderGateContinuation::SkipPlunder0x00734a3c
    );
    assert!(matches!(world.events.last(), Some(HostEvent::Frame)));
}

#[test]
fn russian_receipt_shape_and_bytewise_version_order_are_frozen() {
    assert!(game_version_is_at_most_retail(0x0000_0100, 0x0000_00FF));
    assert!(!game_version_is_at_most_retail(0x0000_00FF, 0x0000_0100));

    let plan = plan_cities_capture_plunder_gate(prior(1), OLD_CITY, 2, false).unwrap();
    let mut world = world(false);
    world.russian = RussianPlunderInspectionReceipt {
        request: russian_request(false),
        has_russian_bonus: false,
        russian_plunder_steal_rule: Some(1),
        game_version: None,
        retail_version: None,
    };
    assert_eq!(
        apply_cities_capture_plunder_gate(plan, &mut world),
        Err(CitiesCapturePlunderGateApplyError::RussianInspectionShapeMismatch)
    );
}
