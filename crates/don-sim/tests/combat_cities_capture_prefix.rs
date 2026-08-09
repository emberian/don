use don_sim::systems::combat::build_check_capture::{CaptureCityBoundaryRequest, CityKey};
use don_sim::systems::combat::cities_capture_prefix::{
    apply_cities_capture_prefix, capture_radius_ring, plan_cities_capture_prefix,
    ActivateCapturedCenterRequest, CaptureAchievementEvent, CaptureAchievementEventRequest,
    CaptureCounter, CapturedCenterState, CitiesCapturePrefixApplyError,
    CitiesCapturePrefixContinuation, CitiesCapturePrefixInput, CitiesCapturePrefixMutation,
    CitiesCapturePrefixPlanError, CitiesCapturePrefixWorld, CityCapturePlatformGate,
    ClearCaptureDiplomacyRequest, IncrementCaptureCounterRequest, MarkCaptureTouchedLeaderRequest,
    PersianCapitalLookup, PlatformAchievement, SwapCityCenterReceipt, SwapCityCenterRequest,
    SwapCityCenterResult, UnlockCityCaptureAchievementRequest, CAPITAL_CITY_FLAG,
    CAPTURE_TOUCHED_LEADER_FLAG, CITIES_CAPTURE_CITY_END, CITIES_CAPTURE_CITY_SIZE,
    CITIES_CAPTURE_CITY_START, CITIES_CAPTURE_PREFIX_END, CITIES_CAPTURE_PREFIX_SIZE,
    CITY_NAME_FIELD_OFFSET, CONQUEROR_ACHIEVEMENT_ID,
};
use don_sim::systems::combat::damage_world::ObjectKey;

const OLD_CITY: CityKey = CityKey { who: 1, city: 6 };
const OLD_CENTER: ObjectKey = ObjectKey { who: 1, o: 40 };

fn request() -> CaptureCityBoundaryRequest {
    CaptureCityBoundaryRequest {
        new_owner: 2,
        old_city: OLD_CITY,
        old_owner: 1,
        strength_winner: 2,
    }
}

fn input() -> CitiesCapturePrefixInput {
    CitiesCapturePrefixInput {
        request: request(),
        old_city_flags: 0,
        old_center: OLD_CENTER,
        old_owner_diplomacy_agree: 0,
        old_owner_has_persian_bonus: false,
        persian_capital_lookup: None,
        old_center_radius: 20,
        city_plunder_per_level: 100,
        platform: CityCapturePlatformGate {
            active_player: None,
            game_flags_0x820: 0,
            mode_0x69c: 0,
            mode_0x6a8: 0,
            setup_player_flag_0xcb: 0,
        },
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Event {
    Clear(ClearCaptureDiplomacyRequest),
    Counter(IncrementCaptureCounterRequest),
    Unlock(UnlockCityCaptureAchievementRequest),
    Mark(MarkCaptureTouchedLeaderRequest),
    Swap(SwapCityCenterRequest),
    Activate(ActivateCapturedCenterRequest),
    Achievement(CaptureAchievementEventRequest),
}

#[derive(Default)]
struct World {
    events: Vec<Event>,
    swap_receipt: Option<SwapCityCenterReceipt>,
}

impl CitiesCapturePrefixWorld for World {
    fn clear_capture_diplomacy(&mut self, request: ClearCaptureDiplomacyRequest) {
        self.events.push(Event::Clear(request));
    }

    fn increment_capture_counter(&mut self, request: IncrementCaptureCounterRequest) {
        self.events.push(Event::Counter(request));
    }

    fn unlock_city_capture_achievement(&mut self, request: UnlockCityCaptureAchievementRequest) {
        self.events.push(Event::Unlock(request));
    }

    fn mark_capture_touched_leader(&mut self, request: MarkCaptureTouchedLeaderRequest) {
        self.events.push(Event::Mark(request));
    }

    fn swap_city_center(
        &mut self,
        request: SwapCityCenterRequest,
    ) -> Option<SwapCityCenterReceipt> {
        self.events.push(Event::Swap(request));
        self.swap_receipt
    }

    fn activate_captured_center(&mut self, request: ActivateCapturedCenterRequest) {
        self.events.push(Event::Activate(request));
    }

    fn add_capture_achievement_event(&mut self, request: CaptureAchievementEventRequest) {
        self.events.push(Event::Achievement(request));
    }
}

fn failed_swap(
    plan: don_sim::systems::combat::cities_capture_prefix::CitiesCapturePrefixPlan,
) -> World {
    let swap_request = SwapCityCenterRequest {
        old_center: plan.old_center,
        new_owner: plan.request.new_owner,
    };
    World {
        swap_receipt: Some(SwapCityCenterReceipt {
            request: swap_request,
            result: SwapCityCenterResult::Failed,
            ownership_applied: false,
            object_copy_applied: false,
        }),
        ..World::default()
    }
}

#[test]
fn address_receipt_freezes_the_exact_prefix_and_residual() {
    assert_eq!(CITIES_CAPTURE_PREFIX_SIZE, 0x3C9);
    assert_eq!(CITIES_CAPTURE_CITY_SIZE, 7_998);
    assert_eq!(CITIES_CAPTURE_PREFIX_END, CITIES_CAPTURE_CITY_START + 0x3C9);
    assert_eq!(CITIES_CAPTURE_CITY_END, CITIES_CAPTURE_CITY_START + 7_998);
    assert_eq!(
        PlatformAchievement::Conqueror as i32,
        CONQUEROR_ACHIEVEMENT_ID
    );
}

#[test]
fn persian_capital_lookup_is_mandatory_before_any_mutation() {
    let mut input = input();
    input.old_city_flags = CAPITAL_CITY_FLAG;
    input.old_owner_has_persian_bonus = true;
    assert_eq!(
        plan_cities_capture_prefix(input),
        Err(CitiesCapturePrefixPlanError::MissingPersianCapitalLookup)
    );

    input.persian_capital_lookup = Some(PersianCapitalLookup {
        found_city: 4,
        found_owner: 1,
    });
    let plan = plan_cities_capture_prefix(input).unwrap();
    assert!(plan.old_city_was_capital);
    assert!(plan.captured_own_capital);
}

#[test]
fn failed_center_swap_still_commits_stats_flags_and_both_events() {
    let mut input = input();
    input.old_owner_diplomacy_agree = 1;
    input.platform.active_player = Some(2);
    let plan = plan_cities_capture_prefix(input).unwrap();
    let mut world = failed_swap(plan);

    let receipt = apply_cities_capture_prefix(plan, &mut world).unwrap();
    assert_eq!(
        receipt.continuation,
        CitiesCapturePrefixContinuation::CenterSwapFailed0x00733ce2
    );
    assert_eq!(receipt.old_city_objects, vec![OLD_CENTER]);
    assert!(receipt.new_city_objects.is_empty());
    assert_eq!(receipt.new_city, None);
    assert_eq!(receipt.plunder_accumulator, 0);

    assert_eq!(
        world.events,
        vec![
            Event::Clear(ClearCaptureDiplomacyRequest {
                old_owner: 1,
                new_owner: 2,
            }),
            Event::Counter(IncrementCaptureCounterRequest {
                who: 1,
                counter: CaptureCounter::CitiesLost,
                amount: 1,
            }),
            Event::Counter(IncrementCaptureCounterRequest {
                who: 2,
                counter: CaptureCounter::CitiesCaptured,
                amount: 1,
            }),
            Event::Unlock(UnlockCityCaptureAchievementRequest {
                achievement: PlatformAchievement::Conqueror,
                unlock: true,
            }),
            Event::Mark(MarkCaptureTouchedLeaderRequest {
                who: 2,
                or_mask: CAPTURE_TOUCHED_LEADER_FLAG,
            }),
            Event::Mark(MarkCaptureTouchedLeaderRequest {
                who: 1,
                or_mask: CAPTURE_TOUCHED_LEADER_FLAG,
            }),
            Event::Swap(SwapCityCenterRequest {
                old_center: OLD_CENTER,
                new_owner: 2,
            }),
            Event::Achievement(CaptureAchievementEventRequest {
                event: CaptureAchievementEvent::CityCaptured,
                who: 2,
                city_name_source: OLD_CITY,
                city_name_field_offset: CITY_NAME_FIELD_OFFSET,
            }),
            Event::Achievement(CaptureAchievementEventRequest {
                event: CaptureAchievementEvent::CityLost,
                who: 1,
                city_name_source: OLD_CITY,
                city_name_field_offset: CITY_NAME_FIELD_OFFSET,
            }),
        ]
    );
}

#[test]
fn successful_swap_activates_before_events_and_seeds_level_minus_one_plunder() {
    let mut input = input();
    input.old_center_radius = 81;
    let plan = plan_cities_capture_prefix(input).unwrap();
    assert_eq!(plan.capture_radius, 64);
    assert_eq!(plan.capture_radius_ring, 16);

    let swap_request = SwapCityCenterRequest {
        old_center: OLD_CENTER,
        new_owner: 2,
    };
    let new_center = ObjectKey { who: 2, o: 77 };
    let new_city = CityKey { who: 2, city: 9 };
    let mut world = World {
        swap_receipt: Some(SwapCityCenterReceipt {
            request: swap_request,
            result: SwapCityCenterResult::Succeeded(CapturedCenterState {
                new_center,
                new_city,
                new_city_level: 3,
            }),
            ownership_applied: true,
            object_copy_applied: true,
        }),
        ..World::default()
    };

    let receipt = apply_cities_capture_prefix(plan, &mut world).unwrap();
    assert_eq!(
        receipt.continuation,
        CitiesCapturePrefixContinuation::CenterSwapSucceeded0x00733755
    );
    assert_eq!(receipt.new_city_objects, vec![new_center]);
    assert_eq!(receipt.new_city, Some(new_city));
    // Retail seeds with CITY_PLUNDER_PER_LEVEL * (level - 1), not level.
    assert_eq!(receipt.plunder_accumulator, 200);
    assert!(matches!(
        receipt.mutations.as_slice(),
        [
            CitiesCapturePrefixMutation::IncrementCounter(_),
            CitiesCapturePrefixMutation::IncrementCounter(_),
            CitiesCapturePrefixMutation::MarkLeader(_),
            CitiesCapturePrefixMutation::MarkLeader(_),
            CitiesCapturePrefixMutation::SwapCenter(_),
            CitiesCapturePrefixMutation::ActivateCenter(_),
            CitiesCapturePrefixMutation::AchievementEvent(_),
            CitiesCapturePrefixMutation::AchievementEvent(_),
        ]
    ));
    assert!(matches!(world.events[5], Event::Activate(_)));
    assert!(matches!(world.events[6], Event::Achievement(_)));
}

#[test]
fn every_platform_gate_is_load_bearing_and_success_receipt_is_identity_bound() {
    let mut base = input();
    base.platform.active_player = Some(2);
    assert!(
        plan_cities_capture_prefix(base)
            .unwrap()
            .unlock_achievement_11
    );

    let mut cases = [base; 5];
    cases[0].platform.active_player = Some(1);
    cases[1].platform.game_flags_0x820 = 0x10;
    cases[2].platform.mode_0x69c = 1;
    cases[3].platform.mode_0x6a8 = 1;
    cases[4].platform.setup_player_flag_0xcb = 1;
    for case in cases {
        assert!(
            !plan_cities_capture_prefix(case)
                .unwrap()
                .unlock_achievement_11
        );
    }

    let plan = plan_cities_capture_prefix(input()).unwrap();
    let mut world = World {
        swap_receipt: Some(SwapCityCenterReceipt {
            request: SwapCityCenterRequest {
                old_center: OLD_CENTER,
                new_owner: 2,
            },
            result: SwapCityCenterResult::Succeeded(CapturedCenterState {
                new_center: ObjectKey { who: 3, o: 77 },
                new_city: CityKey { who: 2, city: 9 },
                new_city_level: 2,
            }),
            ownership_applied: true,
            object_copy_applied: true,
        }),
        ..World::default()
    };
    assert_eq!(
        apply_cities_capture_prefix(plan, &mut world),
        Err(CitiesCapturePrefixApplyError::SuccessfulSwapIdentityMismatch)
    );
}

#[test]
fn signed_radius_ring_matches_the_retail_truncation_sequence() {
    assert_eq!(capture_radius_ring(0), 0);
    assert_eq!(capture_radius_ring(1), 1);
    assert_eq!(capture_radius_ring(4), 1);
    assert_eq!(capture_radius_ring(5), 2);
    assert_eq!(capture_radius_ring(64), 16);
    assert_eq!(capture_radius_ring(100), 16);
    assert_eq!(capture_radius_ring(-8), -1);
}
