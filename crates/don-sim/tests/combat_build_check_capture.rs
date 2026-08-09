use don_sim::systems::combat::build_check_capture::{
    apply_build_check_capture, plan_build_check_capture, BuildCheckCaptureInput,
    BuildCheckCaptureMutation, BuildCheckCaptureOutcomePlan, BuildCheckCapturePlan,
    BuildCheckCaptureWorld, BuildLifecycleRequest, BuildMaskRequest, BuildSwapBoundaryReceipt,
    CaptureAnnouncementRequest, CaptureBuildSnapshot, CaptureCallerEdge,
    CaptureCityBoundaryRequest, CaptureCitySnapshot, CaptureLeaderSnapshot, CaptureNeighborhood,
    CaptureObjectBoundaryRequest, CaptureObjectSnapshot, CaptureRules, CaptureSound,
    CaptureTileSnapshot, CaptureZeroReason, CityCaptureBoundaryReceipt, CityCaptureStrengthRequest,
    CityKey, MarkContainedBuildRequest, RevealCaptureDefenderRequest, CITY_CENTER_FLAG,
    CONTAINED_BUILD_CAPTURE_MASK, NUM_CAPTURE_PLAYERS, OBJECT_ALIVE_FLAG,
};
use don_sim::systems::combat::damage_world::{CaptureCheckRequest, ObjectKey};

const VICTIM: ObjectKey = ObjectKey { who: 1, o: 40 };
const ATTACKER: ObjectKey = ObjectKey { who: 2, o: 17 };

fn leader(who: usize) -> CaptureLeaderSnapshot {
    let mut allies = [false; NUM_CAPTURE_PLAYERS];
    allies[who] = true;
    let mut enemies = [true; NUM_CAPTURE_PLAYERS];
    enemies[who] = false;
    CaptureLeaderSnapshot {
        who_field: who as i32,
        leader_flags: 3,
        diplos: [0; NUM_CAPTURE_PLAYERS],
        allies,
        enemies,
    }
}

fn object(key: ObjectKey, capture_value: i32) -> CaptureObjectSnapshot {
    CaptureObjectSnapshot {
        key,
        flags: OBJECT_ALIVE_FLAG,
        x: 0,
        y: 0,
        domain: 0,
        is_unit: true,
        unit_masks: 0,
        object_masks: 0,
        attack: 1,
        capture_value,
        is_build: false,
        is_fort: false,
        garrison_inside_domain_1: 0,
        visible_mask: 0,
        passes_capture_filter: true,
    }
}

fn input(city_center: bool, attacker_capture_value: i32) -> BuildCheckCaptureInput {
    let flags = 0x4 | if city_center { CITY_CENTER_FLAG } else { 0 };
    BuildCheckCaptureInput {
        request: CaptureCheckRequest {
            victim: VICTIM,
            attacker: ATTACKER,
        },
        victim: CaptureBuildSnapshot {
            key: VICTIM,
            flags,
            inside_down: -1,
            city: 6,
            x: 0,
            y: 0,
            region: 9,
            type_is_city: true,
            is_active: true,
            health_level: 6,
        },
        attacker: object(ATTACKER, attacker_capture_value),
        city: CaptureCitySnapshot {
            key: CityKey { who: 1, city: 6 },
            capture_stamp: 0,
            capture_strength: 2,
            founder: 1,
            race: 1,
        },
        leaders: std::array::from_fn(leader),
        frame: 500,
        rules: CaptureRules {
            unit_respond_range: 12,
            city_capture_radius: 10,
        },
        // Both shipped-style examples select ceil(rule*0xC0 / 0x300) == ring 3.
        neighborhood: CaptureNeighborhood {
            radius_tiles: 3,
            tiles: Vec::new(),
        },
        console_who: None,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Event {
    Mark(MarkContainedBuildRequest),
    Reveal(RevealCaptureDefenderRequest),
    City(CaptureCityBoundaryRequest),
    Strength(CityCaptureStrengthRequest),
    Swap(CaptureObjectBoundaryRequest),
    Kill(ObjectKey),
    Sound(CaptureSound),
    Announce(CaptureAnnouncementRequest),
    Activate(BuildLifecycleRequest),
    Close(BuildLifecycleRequest),
    Mask(BuildMaskRequest),
}

#[derive(Default)]
struct World {
    events: Vec<Event>,
    city_receipt: Option<CityCaptureBoundaryReceipt>,
    swap_receipt: Option<BuildSwapBoundaryReceipt>,
}

impl BuildCheckCaptureWorld for World {
    fn mark_contained_build(&mut self, request: MarkContainedBuildRequest) {
        self.events.push(Event::Mark(request));
    }

    fn reveal_capture_defender(&mut self, request: RevealCaptureDefenderRequest) {
        self.events.push(Event::Reveal(request));
    }

    fn capture_city(
        &mut self,
        request: CaptureCityBoundaryRequest,
    ) -> Option<CityCaptureBoundaryReceipt> {
        self.events.push(Event::City(request));
        self.city_receipt
    }

    fn set_city_capture_strength(&mut self, request: CityCaptureStrengthRequest) {
        self.events.push(Event::Strength(request));
    }

    fn swap_build_team(
        &mut self,
        request: CaptureObjectBoundaryRequest,
    ) -> Option<BuildSwapBoundaryReceipt> {
        self.events.push(Event::Swap(request));
        self.swap_receipt
    }

    fn kill_build_after_failed_swap(&mut self, build: ObjectKey) {
        self.events.push(Event::Kill(build));
    }

    fn play_capture_sound(&mut self, sound: CaptureSound) {
        self.events.push(Event::Sound(sound));
    }

    fn announce_capture(&mut self, request: CaptureAnnouncementRequest) {
        self.events.push(Event::Announce(request));
    }

    fn activate_captured_build(&mut self, request: BuildLifecycleRequest) {
        self.events.push(Event::Activate(request));
    }

    fn close_old_build(&mut self, request: BuildLifecycleRequest) {
        self.events.push(Event::Close(request));
    }

    fn mask_captured_build(&mut self, request: BuildMaskRequest) {
        self.events.push(Event::Mask(request));
    }
}

#[test]
fn contained_build_marks_0x4000_then_returns_into_capture_zero() {
    let mut input = input(true, 10);
    input.victim.inside_down = 7;
    let plan = plan_build_check_capture(&input).unwrap();
    assert_eq!(
        plan.outcome,
        BuildCheckCaptureOutcomePlan::ReturnZero(CaptureZeroReason::ContainedBuild)
    );

    let mut world = World::default();
    let receipt = apply_build_check_capture(&plan, &mut world).unwrap();
    assert_eq!(receipt.return_value, 0);
    assert_eq!(
        receipt.caller_edge,
        CaptureCallerEdge::CaptureZeroFallthrough
    );
    assert!(!receipt.capture_attempt_receipt().returned_nonzero);
    assert_eq!(
        world.events,
        vec![Event::Mark(MarkContainedBuildRequest {
            build: VICTIM,
            or_mask: CONTAINED_BUILD_CAPTURE_MASK,
        })]
    );
}

#[test]
fn defender_held_attempt_still_reveals_fort_before_returning_zero() {
    let mut input = input(true, 10);
    let mut fort = object(ObjectKey { who: 1, o: 51 }, 1);
    fort.is_unit = false;
    fort.is_build = true;
    fort.is_fort = true;
    fort.garrison_inside_domain_1 = 2;
    input.neighborhood.tiles.push(CaptureTileSnapshot {
        valid: true,
        region: 9,
        down_chain: vec![fort],
    });

    let plan = plan_build_check_capture(&input).unwrap();
    let census = plan.census.as_ref().unwrap();
    // Defender seed 2 + build 1 + 6 + fort 6 + garrison 2.
    assert_eq!(census.defender_side, 17);
    assert_eq!(census.attacker_side, 10);
    assert_eq!(census.reveals.len(), 1);
    assert_eq!(
        plan.outcome,
        BuildCheckCaptureOutcomePlan::ReturnZero(CaptureZeroReason::DefenderHeld)
    );

    let mut world = World::default();
    let receipt = apply_build_check_capture(&plan, &mut world).unwrap();
    assert_eq!(
        receipt.caller_edge,
        CaptureCallerEdge::CaptureZeroFallthrough
    );
    assert!(matches!(world.events.as_slice(), [Event::Reveal(_)]));
    assert!(matches!(
        receipt.mutations.as_slice(),
        [BuildCheckCaptureMutation::RevealDefender(_)]
    ));
}

#[test]
fn strongest_ally_then_founder_rehome_and_original_attacker_strength_are_distinct() {
    let mut input = input(true, 4);
    // Player 3 is an active attacker ally and has the strongest individual bucket.
    input.leaders[2].allies[3] = true;
    input.leaders[1].enemies[3] = true;
    input.leaders[3].allies[2] = true;
    // Player 4 is an active founder allied to the strength winner.
    input.leaders[3].allies[4] = true;
    input.city.founder = 4;
    let ally = object(ObjectKey { who: 3, o: 60 }, 9);
    input.neighborhood.tiles.push(CaptureTileSnapshot {
        valid: true,
        region: 9,
        down_chain: vec![ally],
    });

    let plan = plan_build_check_capture(&input).unwrap();
    let request = match plan.outcome {
        BuildCheckCaptureOutcomePlan::CaptureCity(request) => request,
        other => panic!("expected city capture, got {other:?}"),
    };
    assert_eq!(request.strength_winner, 3);
    assert_eq!(request.new_owner, 4);

    let new_city = CityKey { who: 4, city: 8 };
    let mut world = World {
        city_receipt: Some(CityCaptureBoundaryReceipt {
            request,
            new_city,
            buildings_reassigned: true,
            economy_and_plunder_applied: true,
            diplomacy_and_score_applied: true,
            object_transfer_applied: true,
        }),
        ..World::default()
    };
    let receipt = apply_build_check_capture(&plan, &mut world).unwrap();
    assert_eq!(receipt.caller_edge, CaptureCallerEdge::StopObjectDoDamage);
    assert!(receipt.capture_attempt_receipt().returned_nonzero);
    assert_eq!(
        world.events,
        vec![
            Event::City(request),
            Event::Strength(CityCaptureStrengthRequest {
                city: new_city,
                // Retail stores the original attacker's 4, not aggregate 13 or winner 9.
                value: 4,
            }),
        ]
    );
}

fn planned_object_capture(console_who: Option<u8>) -> BuildCheckCapturePlan {
    let mut input = input(false, 10);
    input.console_who = console_who;
    plan_build_check_capture(&input).unwrap()
}

#[test]
fn negative_swap_kills_old_build_but_returns_zero_to_damage_fallthrough() {
    let plan = planned_object_capture(None);
    let request = match plan.outcome {
        BuildCheckCaptureOutcomePlan::CaptureObject(request) => request,
        other => panic!("expected object capture, got {other:?}"),
    };
    let mut world = World {
        swap_receipt: Some(BuildSwapBoundaryReceipt {
            request,
            new_build: None,
            ownership_applied: false,
            object_copy_applied: false,
        }),
        ..World::default()
    };

    let receipt = apply_build_check_capture(&plan, &mut world).unwrap();
    assert_eq!(receipt.return_value, 0);
    assert_eq!(
        receipt.caller_edge,
        CaptureCallerEdge::CaptureZeroFallthrough
    );
    assert_eq!(
        world.events,
        vec![Event::Swap(request), Event::Kill(VICTIM)]
    );
}

#[test]
fn successful_object_capture_orders_presentation_activate_close_and_mask() {
    let plan = planned_object_capture(Some(ATTACKER.who));
    let request = match plan.outcome {
        BuildCheckCaptureOutcomePlan::CaptureObject(request) => request,
        other => panic!("expected object capture, got {other:?}"),
    };
    let new_build = ObjectKey { who: 2, o: 77 };
    let mut world = World {
        swap_receipt: Some(BuildSwapBoundaryReceipt {
            request,
            new_build: Some(new_build),
            ownership_applied: true,
            object_copy_applied: true,
        }),
        ..World::default()
    };

    let receipt = apply_build_check_capture(&plan, &mut world).unwrap();
    assert_eq!(receipt.caller_edge, CaptureCallerEdge::StopObjectDoDamage);
    assert_eq!(
        world.events,
        vec![
            Event::Swap(request),
            Event::Sound(CaptureSound::CapturedObject0x7c),
            Event::Announce(CaptureAnnouncementRequest {
                old_build: VICTIM,
                new_build,
                red_localized_message: true,
            }),
            Event::Activate(BuildLifecycleRequest {
                build: new_build,
                first: 0,
                second: 1,
                third: 0,
            }),
            Event::Close(BuildLifecycleRequest {
                build: VICTIM,
                first: 0,
                second: -1,
                third: 0,
            }),
            Event::Mask(BuildMaskRequest {
                build: new_build,
                first: 1,
                second: 2,
            }),
        ]
    );
}
