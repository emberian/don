use don_sim::systems::combat::{build_check_capture, cities_capture_prefix, damage_world};

#[path = "../src/systems/combat/cities_capture_swap_fork.rs"]
mod cities_capture_swap_fork;

use build_check_capture::{CaptureCityBoundaryRequest, CityKey};
use cities_capture_prefix::{CitiesCapturePrefixContinuation, CitiesCapturePrefixReceipt};
use cities_capture_swap_fork::{
    apply_cities_capture_swap_fork, nearby_unit_radius_threshold, plan_cities_capture_swap_fork,
    same_or_mutual_allies, ActivateCapturedMemberRequest, ArmyCityReferenceUpdateReceipt,
    ArmyCityReferenceUpdateRequest, CaptureCircleScanReceipt, CaptureCircleScanRequest,
    CaptureMemberChainReceipt, CaptureMemberChainRequest, CitiesCaptureSwapForkContinuation,
    CitiesCaptureSwapForkMutation, CitiesCaptureSwapForkWorld, CityCaptureEcxResidue,
    CityRecordCaptureReceipt, CityRecordCaptureRequest, FindCapturedCityBuildingsReceipt,
    FindCapturedCityBuildingsRequest, NearbyUnitFindCityReceipt, OrderedCapturedMember,
    OrderedNearbyUnit, SuccessfulCenterForkInput, SwapCapturedMemberReceipt,
    SwapCapturedMemberRequest, SwapCapturedMemberResult, SwapNearbyUnitReceipt,
    SwapNearbyUnitRequest, CAPTURED_MEMBER_PLUNDER, CAPTURE_MEMBER_SKIP_FLAG,
    CITIES_CAPTURE_RESIDUAL_SIZE, CITIES_CAPTURE_SWAP_FORK_END, CITIES_CAPTURE_SWAP_FORK_SIZE,
    CITIES_CAPTURE_SWAP_FORK_START, FAILED_CAPTURE_MEMBER_SKIP_FLAG,
};
use damage_world::ObjectKey;

const OLD_CENTER: ObjectKey = ObjectKey { who: 1, o: 40 };
const NEW_CENTER: ObjectKey = ObjectKey { who: 2, o: 70 };
const OLD_CITY: CityKey = CityKey { who: 1, city: 6 };
const NEW_CITY: CityKey = CityKey { who: 2, city: 9 };

fn prefix(continuation: CitiesCapturePrefixContinuation) -> CitiesCapturePrefixReceipt {
    let success = continuation == CitiesCapturePrefixContinuation::CenterSwapSucceeded0x00733755;
    CitiesCapturePrefixReceipt {
        request: CaptureCityBoundaryRequest {
            new_owner: 2,
            old_city: OLD_CITY,
            old_owner: 1,
            strength_winner: 2,
        },
        old_city_was_capital: false,
        captured_own_capital: false,
        capture_radius: 20,
        capture_radius_ring: 5,
        old_city_objects: vec![OLD_CENTER],
        new_city_objects: if success { vec![NEW_CENTER] } else { vec![] },
        new_city: success.then_some(NEW_CITY),
        plunder_accumulator: if success { 200 } else { 0 },
        continuation,
        mutations: vec![],
    }
}

fn member(o: i16, ordinal: u32) -> OrderedCapturedMember {
    OrderedCapturedMember {
        old_member: ObjectKey { who: 1, o },
        chain_ordinal: ordinal,
        build_type_flags: 0,
        is_farm: false,
        is_granary: false,
        is_active_build: true,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Event {
    ReadChain(CaptureMemberChainRequest),
    Capture(CityRecordCaptureRequest),
    Armies(ArmyCityReferenceUpdateRequest),
    SwapMember(SwapCapturedMemberRequest),
    Activate(ActivateCapturedMemberRequest),
    Find(FindCapturedCityBuildingsRequest),
    Scan(CaptureCircleScanRequest),
    SwapUnit(SwapNearbyUnitRequest),
}

#[derive(Default)]
struct World {
    events: Vec<Event>,
    members: Vec<OrderedCapturedMember>,
    scan_units: Option<Vec<OrderedNearbyUnit>>,
}

impl CitiesCaptureSwapForkWorld for World {
    fn read_capture_member_chain(
        &mut self,
        request: CaptureMemberChainRequest,
    ) -> Option<CaptureMemberChainReceipt> {
        self.events.push(Event::ReadChain(request));
        Some(CaptureMemberChainReceipt {
            request,
            walked_exact_city_down_chain: true,
            members: self.members.clone(),
        })
    }

    fn capture_city_record(
        &mut self,
        request: CityRecordCaptureRequest,
    ) -> Option<CityRecordCaptureReceipt> {
        self.events.push(Event::Capture(request));
        Some(CityRecordCaptureReceipt {
            request,
            capture_applied: true,
            ignored_ecx_residue: CityCaptureEcxResidue(0x1234),
        })
    }

    fn update_army_city_references(
        &mut self,
        request: ArmyCityReferenceUpdateRequest,
    ) -> Option<ArmyCityReferenceUpdateReceipt> {
        self.events.push(Event::Armies(request));
        Some(ArmyCityReferenceUpdateReceipt {
            request,
            update_applied: true,
        })
    }

    fn swap_captured_member(
        &mut self,
        request: SwapCapturedMemberRequest,
    ) -> Option<SwapCapturedMemberReceipt> {
        self.events.push(Event::SwapMember(request));
        Some(SwapCapturedMemberReceipt {
            request,
            result: SwapCapturedMemberResult::Succeeded {
                new_member: ObjectKey {
                    who: request.new_owner,
                    o: request.old_member.o + 100,
                },
            },
            ownership_applied: true,
            object_copy_applied: true,
        })
    }

    fn activate_captured_member(&mut self, request: ActivateCapturedMemberRequest) {
        self.events.push(Event::Activate(request));
    }

    fn find_captured_city_buildings(
        &mut self,
        request: FindCapturedCityBuildingsRequest,
    ) -> Option<FindCapturedCityBuildingsReceipt> {
        self.events.push(Event::Find(request));
        Some(FindCapturedCityBuildingsReceipt {
            request,
            rebuild_applied: true,
        })
    }

    fn scan_capture_circle(
        &mut self,
        request: CaptureCircleScanRequest,
    ) -> Option<CaptureCircleScanReceipt> {
        self.events.push(Event::Scan(request));
        let units = self.scan_units.clone().unwrap_or_else(|| {
            vec![OrderedNearbyUnit {
                circle_ordinal: 3,
                chain_ordinal: 0,
                unit: ObjectKey { who: 1, o: 91 },
                is_valid_unit: true,
                is_on_map: true,
                type_index: 0x36,
                distance_from_new_center: i32::MAX,
                // The one-city allied bypass ignores both distance and this result.
                find_city: None,
            }]
        });
        Some(CaptureCircleScanReceipt {
            request,
            used_retail_circle_tables: true,
            units,
        })
    }

    fn swap_nearby_unit(
        &mut self,
        request: SwapNearbyUnitRequest,
    ) -> Option<SwapNearbyUnitReceipt> {
        self.events.push(Event::SwapUnit(request));
        Some(SwapNearbyUnitReceipt {
            request,
            new_unit: Some(ObjectKey {
                who: request.new_owner,
                o: 191,
            }),
            ownership_applied: true,
            object_copy_applied: true,
        })
    }
}

#[test]
fn exact_fork_and_residual_addresses_are_frozen() {
    assert_eq!(CITIES_CAPTURE_SWAP_FORK_START, 0x0073_3749);
    assert_eq!(CITIES_CAPTURE_SWAP_FORK_END, 0x0073_3D67);
    assert_eq!(CITIES_CAPTURE_SWAP_FORK_SIZE, 0x61E);
    assert_eq!(CITIES_CAPTURE_RESIDUAL_SIZE, 0x1557);
}

#[test]
fn success_orders_city_armies_members_find_buildings_and_circle_units() {
    let mut members = vec![member(41, 0), member(42, 1), member(43, 2)];
    members[1].build_type_flags = CAPTURE_MEMBER_SKIP_FLAG;
    members[2].is_farm = true;
    let plan = plan_cities_capture_swap_fork(
        prefix(CitiesCapturePrefixContinuation::CenterSwapSucceeded0x00733755),
        Some(SuccessfulCenterForkInput {
            old_owner_has_lakota_bonus: true,
            old_leader_who: 1,
            old_to_new_diplomacy: 2,
            new_to_old_diplomacy: 2,
            old_owner_city_num: 1,
            new_owner_center_radius: 20,
        }),
    )
    .unwrap();
    let mut world = World {
        members,
        ..World::default()
    };
    let receipt = apply_cities_capture_swap_fork(plan, &mut world).unwrap();

    assert_eq!(
        receipt.old_city_objects,
        vec![
            OLD_CENTER,
            ObjectKey { who: 1, o: 41 },
            ObjectKey { who: 1, o: 43 }
        ]
    );
    assert_eq!(
        receipt.new_city_objects,
        vec![NEW_CENTER, ObjectKey { who: 2, o: 141 }]
    );
    assert_eq!(receipt.plunder_accumulator, 200 + CAPTURED_MEMBER_PLUNDER);
    assert_eq!(receipt.new_city, Some(NEW_CITY));
    assert_eq!(
        receipt.continuation,
        CitiesCaptureSwapForkContinuation::Converged0x00733d67
    );
    assert!(matches!(
        receipt.mutations.as_slice(),
        [
            CitiesCaptureSwapForkMutation::CaptureCityRecord(_),
            CitiesCaptureSwapForkMutation::UpdateArmyCityReferences(_),
            CitiesCaptureSwapForkMutation::ReadMemberChain(_),
            CitiesCaptureSwapForkMutation::AppendOldObject(_),
            CitiesCaptureSwapForkMutation::SwapMember(_),
            CitiesCaptureSwapForkMutation::ActivateMember(_),
            CitiesCaptureSwapForkMutation::AppendNewObject(_),
            CitiesCaptureSwapForkMutation::AddPlunder(25),
            CitiesCaptureSwapForkMutation::AppendOldObject(_),
            CitiesCaptureSwapForkMutation::FindBuildings(_),
            CitiesCaptureSwapForkMutation::SwapNearbyUnit(_),
        ]
    ));
    assert!(matches!(world.events[0], Event::Capture(_)));
    assert!(matches!(world.events[1], Event::Armies(_)));
    assert!(matches!(world.events[2], Event::ReadChain(_)));
    assert!(matches!(world.events[3], Event::SwapMember(_)));
    assert!(matches!(world.events[4], Event::Activate(_)));
    assert!(matches!(world.events[5], Event::Find(_)));
    assert!(matches!(world.events[6], Event::Scan(_)));
    assert!(matches!(world.events[7], Event::SwapUnit(_)));
}

#[test]
fn failed_swap_census_only_reads_the_chain_and_uses_the_distinct_flag() {
    let mut kept = member(44, 0);
    kept.build_type_flags = CAPTURE_MEMBER_SKIP_FLAG;
    let mut skipped = member(45, 1);
    skipped.build_type_flags = FAILED_CAPTURE_MEMBER_SKIP_FLAG;
    let plan = plan_cities_capture_swap_fork(
        prefix(CitiesCapturePrefixContinuation::CenterSwapFailed0x00733ce2),
        None,
    )
    .unwrap();
    let mut world = World {
        members: vec![kept, skipped],
        ..World::default()
    };
    let receipt = apply_cities_capture_swap_fork(plan, &mut world).unwrap();

    assert!(matches!(world.events.as_slice(), [Event::ReadChain(_)]));
    assert_eq!(receipt.old_city_objects, vec![OLD_CENTER, kept.old_member]);
    assert!(receipt.new_city_objects.is_empty());
    assert_eq!(receipt.plunder_accumulator, CAPTURED_MEMBER_PLUNDER);
    assert!(matches!(
        receipt.mutations.as_slice(),
        [
            CitiesCaptureSwapForkMutation::ReadMemberChain(_),
            CitiesCaptureSwapForkMutation::AppendOldObject(_),
            CitiesCaptureSwapForkMutation::AddPlunder(CAPTURED_MEMBER_PLUNDER),
        ]
    ));
}

#[test]
fn diplomacy_and_post_capture_radius_gates_preserve_retail_arithmetic() {
    assert!(same_or_mutual_allies(1, 2, 2, 2));
    assert!(!same_or_mutual_allies(1, 2, 2, 1));
    assert!(same_or_mutual_allies(2, 2, 0, 0));
    assert_eq!(nearby_unit_radius_threshold(20), 3_840);
    assert_eq!(nearby_unit_radius_threshold(100), 12_288);
    assert_eq!(nearby_unit_radius_threshold(-1), -192);
}

#[test]
fn multi_city_locality_binds_the_exact_nine_argument_find_city_tuple() {
    let successful = SuccessfulCenterForkInput {
        old_owner_has_lakota_bonus: false,
        old_leader_who: 1,
        old_to_new_diplomacy: 2,
        new_to_old_diplomacy: 2,
        old_owner_city_num: 2,
        new_owner_center_radius: 20,
    };
    let plan = plan_cities_capture_swap_fork(
        prefix(CitiesCapturePrefixContinuation::CenterSwapSucceeded0x00733755),
        Some(successful),
    )
    .unwrap();
    let unit = ObjectKey { who: 1, o: 92 };
    let mut world = World {
        scan_units: Some(vec![OrderedNearbyUnit {
            circle_ordinal: 4,
            chain_ordinal: 0,
            unit,
            is_valid_unit: true,
            is_on_map: true,
            type_index: 0x32,
            distance_from_new_center: 1_000,
            find_city: Some(NearbyUnitFindCityReceipt {
                unit,
                x: 2_000,
                y: 3_000,
                search_index_bh: 1,
                new_owner: 2,
                repeated_y: 3_000,
                zero_tail: [0; 4],
                result: Some(NEW_CITY),
            }),
        }]),
        ..World::default()
    };
    let receipt = apply_cities_capture_swap_fork(plan, &mut world).unwrap();
    assert!(matches!(
        receipt.mutations.last(),
        Some(CitiesCaptureSwapForkMutation::SwapNearbyUnit(_))
    ));
}
