use don_sim::systems::combat::{build_check_capture, cities_capture_prefix, damage_world};

#[path = "../src/systems/combat/cities_capture_local_award_notification.rs"]
mod cities_capture_local_award_notification;
#[path = "../src/systems/combat/cities_capture_plunder_award.rs"]
mod cities_capture_plunder_award;
#[path = "../src/systems/combat/cities_capture_plunder_gate.rs"]
mod cities_capture_plunder_gate;
#[path = "../src/systems/combat/cities_capture_residual.rs"]
mod cities_capture_residual;
#[path = "../src/systems/combat/cities_capture_swap_fork.rs"]
mod cities_capture_swap_fork;

use build_check_capture::CityKey;
use cities_capture_plunder_award::{
    CitiesCapturePlunderAwardContinuation, CitiesCapturePlunderAwardReceipt,
};
use cities_capture_plunder_gate::{
    CitiesCapturePlunderGateContinuation, CitiesCapturePlunderGateReceipt,
};
use cities_capture_residual::*;
use cities_capture_swap_fork::{CitiesCaptureSwapForkContinuation, CitiesCaptureSwapForkReceipt};
use damage_world::ObjectKey;

const OLD_CITY: CityKey = CityKey { who: 1, city: 6 };
const NEW_CITY: CityKey = CityKey { who: 2, city: 9 };
const OLD_CENTER: ObjectKey = ObjectKey { who: 1, o: 40 };
const OLD_MEMBER: ObjectKey = ObjectKey { who: 1, o: 41 };
const NEW_CENTER: ObjectKey = ObjectKey { who: 2, o: 70 };
const NEW_MEMBER: ObjectKey = ObjectKey { who: 2, o: 71 };

fn gate(
    continuation: CitiesCapturePlunderGateContinuation,
    qualifying_general: bool,
    russian_plunder_steal: bool,
) -> CitiesCapturePlunderGateReceipt {
    CitiesCapturePlunderGateReceipt {
        prior: CitiesCaptureSwapForkReceipt {
            old_city_objects: vec![OLD_CENTER, OLD_MEMBER],
            new_city_objects: vec![NEW_CENTER, NEW_MEMBER],
            new_city: Some(NEW_CITY),
            plunder_accumulator: 100,
            continuation: CitiesCaptureSwapForkContinuation::Converged0x00733d67,
            mutations: vec![],
        },
        old_city: OLD_CITY,
        new_owner: 2,
        captured_own_capital: false,
        qualifying_general,
        russian_plunder_steal,
        notification_raised: false,
        city_flags: CAPITAL_CITY_FLAG,
        capture_stamp: Some(0),
        continuation,
        events: vec![],
    }
}

fn alternate_prior() -> CitiesCaptureResidualPrior {
    CitiesCaptureResidualPrior::PlunderAward(CitiesCapturePlunderAwardReceipt {
        prior: gate(
            CitiesCapturePlunderGateContinuation::EnterPlunder0x00733fac,
            false,
            true,
        ),
        sized_capital_plunder: None,
        new_owner_award: None,
        old_owner_refund: None,
        continuation: CitiesCapturePlunderAwardContinuation::AlternatePlunder0x00734547,
        events: vec![],
    })
}

#[derive(Default)]
struct World {
    console: i32,
    balances: [[i32; 6]; 9],
    presentations: Vec<CapturePresentationRequest>,
    bucket_adds: Vec<BucketAddRequest>,
    lost_capital: bool,
    recaptured_capital: bool,
    disbanded: Vec<ObjectKey>,
    closed: Vec<CloseCapturedBuildRequest>,
    masked: Vec<ObjectKey>,
    corrupt_inspection: bool,
}

impl World {
    fn canonical() -> Self {
        let mut world = Self {
            console: 2,
            ..Self::default()
        };
        world.balances[1] = [10, 5, 99, -100, 30, 40];
        world.balances[2] = [100, 200, 300, 400, 500, 600];
        world
    }
}

impl CitiesCaptureResidualWorld for World {
    fn read_type_availability(
        &mut self,
        request: TypeAvailabilityRequest,
    ) -> Option<TypeAvailabilityReceipt> {
        Some(TypeAvailabilityReceipt {
            request,
            raw_availability: 2,
        })
    }

    fn read_resource_balance(
        &mut self,
        request: ResourceBalanceRequest,
    ) -> Option<ResourceBalanceReceipt> {
        Some(ResourceBalanceReceipt {
            request,
            decoded_balance: self.balances[usize::from(request.owner)][request.bucket as usize],
        })
    }

    fn bucket_add(&mut self, request: BucketAddRequest) -> Option<BucketAddReceipt> {
        self.bucket_adds.push(request);
        let balance = &mut self.balances[usize::from(request.owner)][request.bucket as usize];
        let before = *balance;
        *balance = balance.wrapping_add(request.amount);
        Some(BucketAddReceipt {
            request,
            balance_before: before,
            balance_after: *balance,
            mutation_applied: true,
        })
    }

    fn read_thedespot_plunder_rule(&mut self) -> Option<TheDespotPlunderRuleReceipt> {
        Some(TheDespotPlunderRuleReceipt { percent: 50 })
    }

    fn read_console_who(&mut self, request: ConsoleWhoRequest) -> Option<ConsoleWhoReceipt> {
        Some(ConsoleWhoReceipt {
            request,
            who: self.console,
        })
    }

    fn present_capture(
        &mut self,
        request: CapturePresentationRequest,
    ) -> Option<CapturePresentationReceipt> {
        self.presentations.push(request);
        Some(CapturePresentationReceipt {
            request,
            exact_call_order_attested: true,
            effects_complete: true,
        })
    }

    fn inspect_observer_capture(
        &mut self,
        request: ObserverCaptureRequest,
    ) -> Option<ObserverCaptureReceipt> {
        Some(ObserverCaptureReceipt {
            request,
            route: ObserverCaptureRoute::None,
            exact_query_order_attested: true,
        })
    }

    fn inspect_captured_object(
        &mut self,
        request: CapturedObjectInspectionRequest,
    ) -> Option<CapturedObjectInspectionReceipt> {
        if request.object == OLD_CENTER {
            Some(CapturedObjectInspectionReceipt {
                request,
                flags: 0,
                is_build: Some(true),
                is_type_0x1a1: Some(true),
                is_type_0x1a7: if self.corrupt_inspection {
                    Some(false)
                } else {
                    None
                },
                has_close_preserving_tribe_bonus: Some(true),
                exact_query_order_attested: true,
            })
        } else {
            Some(CapturedObjectInspectionReceipt {
                request,
                flags: 0,
                is_build: Some(false),
                is_type_0x1a1: None,
                is_type_0x1a7: None,
                has_close_preserving_tribe_bonus: None,
                exact_query_order_attested: true,
            })
        }
    }

    fn disband_object(&mut self, request: ObjectMutationRequest) -> Option<ObjectMutationReceipt> {
        self.disbanded.push(request.object);
        Some(ObjectMutationReceipt {
            request,
            effects_complete: true,
        })
    }

    fn close_captured_build(
        &mut self,
        request: CloseCapturedBuildRequest,
    ) -> Option<CloseCapturedBuildReceipt> {
        self.closed.push(request);
        Some(CloseCapturedBuildReceipt {
            request,
            effects_complete: true,
        })
    }

    fn mask_captured_wall(
        &mut self,
        request: MaskCapturedWallRequest,
    ) -> Option<MaskCapturedWallReceipt> {
        self.masked.push(request.object);
        Some(MaskCapturedWallReceipt {
            request,
            effects_complete: true,
        })
    }

    fn read_city_flags(&mut self, request: CityFlagsRequest) -> Option<CityFlagsReceipt> {
        Some(CityFlagsReceipt {
            request,
            flags: CAPITAL_CITY_FLAG,
        })
    }

    fn has_wonder(&mut self, request: HasWonderRequest) -> Option<HasWonderReceipt> {
        Some(HasWonderReceipt {
            request,
            raw_result: 0,
        })
    }

    fn find_capital(&mut self, request: FindCapitalRequest) -> Option<FindCapitalReceipt> {
        Some(FindCapitalReceipt {
            request,
            found_city: -1,
            found_owner: -1,
        })
    }

    fn lost_capital(&mut self, request: LostCapitalRequest) -> Option<LostCapitalReceipt> {
        self.lost_capital = true;
        Some(LostCapitalReceipt {
            request,
            effects_complete: true,
        })
    }

    fn recapture_capital(
        &mut self,
        request: RecaptureCapitalRequest,
    ) -> Option<RecaptureCapitalReceipt> {
        self.recaptured_capital = true;
        Some(RecaptureCapitalReceipt {
            request,
            effects_complete: true,
        })
    }

    fn read_city_center(&mut self, request: CityCenterRequest) -> Option<CityCenterReceipt> {
        Some(CityCenterReceipt {
            center: match request.phase {
                CenterReadPhase::OldCity0x007350e7 => OLD_CENTER,
                CenterReadPhase::NewCity0x00735189 => NEW_CENTER,
            },
            request,
        })
    }

    fn close_old_city(&mut self, request: CloseOldCityRequest) -> Option<CloseOldCityReceipt> {
        Some(CloseOldCityReceipt {
            request,
            effects_complete: true,
        })
    }

    fn trim_city_tail(&mut self, request: TrimCityTailRequest) -> Option<TrimCityTailReceipt> {
        Some(TrimCityTailReceipt {
            request,
            city_mark_before: 3,
            trailing_flags: vec![0, 0, 1],
            city_mark_after: 1,
            mutation_applied: true,
        })
    }

    fn read_build_city(&mut self, object: ObjectKey) -> Option<i16> {
        assert_eq!(object, OLD_CENTER);
        Some(6)
    }

    fn set_build_city(&mut self, request: SetBuildCityRequest) -> Option<SetBuildCityReceipt> {
        Some(SetBuildCityReceipt {
            request,
            mutation_applied: true,
        })
    }

    fn update_center_hits(
        &mut self,
        request: UpdateCenterHitsRequest,
    ) -> Option<UpdateCenterHitsReceipt> {
        Some(UpdateCenterHitsReceipt {
            request,
            effects_complete: true,
        })
    }

    fn read_center_hits(
        &mut self,
        request: ReadCenterHitsRequest,
    ) -> Option<ReadCenterHitsReceipt> {
        Some(ReadCenterHitsReceipt { request, hits: 500 })
    }

    fn read_center_damage(&mut self, object: ObjectKey) -> Option<i32> {
        assert_eq!(object, NEW_CENTER);
        Some(0)
    }

    fn set_center_damage(
        &mut self,
        request: SetCenterDamageRequest,
    ) -> Option<SetCenterDamageReceipt> {
        Some(SetCenterDamageReceipt {
            request,
            mutation_applied: true,
        })
    }

    fn update_center_los(
        &mut self,
        request: ObjectMutationRequest,
    ) -> Option<ObjectMutationReceipt> {
        Some(ObjectMutationReceipt {
            request,
            effects_complete: true,
        })
    }

    fn calc_pop_cap(&mut self, request: CalcPopCapRequest) -> Option<CalcPopCapReceipt> {
        Some(CalcPopCapReceipt {
            request,
            effects_complete: true,
        })
    }

    fn destroy_capture_array(
        &mut self,
        request: DestroyCaptureArrayRequest,
    ) -> Option<DestroyCaptureArrayReceipt> {
        Some(DestroyCaptureArrayReceipt {
            request,
            storage_released: true,
        })
    }
}

#[test]
fn exact_boundary_and_capstone_census_are_frozen() {
    assert_eq!(CITIES_CAPTURE_RESIDUAL_START, 0x0073_432d);
    assert_eq!(CITIES_CAPTURE_RESIDUAL_END, 0x0073_52be);
    assert_eq!(CITIES_CAPTURE_RESIDUAL_SIZE, 3_985);
    assert_eq!(CITIES_CAPTURE_RESIDUAL_INSTRUCTION_COUNT, 1_071);
    assert_eq!(CITIES_CAPTURE_RESIDUAL_CALL_COUNT, 116);
    assert_eq!(CAPTURE_CLOSE_TYPE_0, 0x1a1);
    assert_eq!(CAPTURE_CLOSE_TYPE_1, 0x1a7);
    assert_eq!(CAPTURE_CLOSE_TRIBE_BONUS, 0x13);
}

#[test]
fn skip_plunder_runs_common_presentation_cleanup_capitals_and_return() {
    let prior = CitiesCaptureResidualPrior::SkipPlunder(gate(
        CitiesCapturePlunderGateContinuation::SkipPlunder0x00734a3c,
        false,
        false,
    ));
    let plan = plan_cities_capture_residual(prior).unwrap();
    let mut world = World::canonical();
    let receipt = apply_cities_capture_residual(plan, &mut world).unwrap();

    assert_eq!(receipt.returned_city, i32::from(NEW_CITY.city));
    assert_eq!(
        world.presentations,
        vec![CapturePresentationRequest::LocalNewOwnerCapture {
            city: NEW_CITY,
            owner: 2,
            message_template: NEW_OWNER_CAPTURE_MESSAGE_OFFSET,
            bubble_template: NEW_OWNER_CAPTURE_BUBBLE_OFFSET,
            notice_internal_offset: NEW_OWNER_NOTICE_INTERNAL_BYTE_OFFSET,
            sound: NEW_OWNER_CAPTURE_SOUND,
        }]
    );
    assert_eq!(world.disbanded, vec![OLD_MEMBER]);
    assert_eq!(world.masked, vec![NEW_CENTER, NEW_MEMBER]);
    assert!(world.lost_capital);
    assert!(world.recaptured_capital);
    assert_eq!(world.closed[0].mode, 0);
    assert_eq!(world.closed.last().unwrap().mode, 5);
    assert!(matches!(
        receipt.events.last(),
        Some(CitiesCaptureResidualEvent::ReturnCity(9))
    ));
}

#[test]
fn russian_alternate_refund_chooses_smallest_nonknowledge_old_bucket() {
    let plan = plan_cities_capture_residual(alternate_prior()).unwrap();
    let mut world = World::canonical();
    world.console = 1;
    let receipt = apply_cities_capture_residual(plan, &mut world).unwrap();

    assert_eq!(receipt.returned_city, 9);
    assert_eq!(world.bucket_adds.len(), 1);
    assert_eq!(world.bucket_adds[0].owner, 1);
    assert_eq!(world.bucket_adds[0].bucket, 1);
    assert_eq!(world.bucket_adds[0].amount, 100);
    assert_eq!(world.balances[1][1], 105);
    assert_eq!(
        world.presentations[0],
        CapturePresentationRequest::ResourceAward {
            city: NEW_CITY,
            beneficiary: 1,
            bubble_owner: 2,
            color_owner: 2,
            bucket: 1,
            amount: 100,
            amount_template: RESOURCE_AMOUNT_TEMPLATE_OFFSET,
            city_template: RESOURCE_CITY_TEMPLATE_OFFSET,
        }
    );
    assert_eq!(
        world.presentations.len(),
        1,
        "refund notice suppresses common capture UI"
    );
}

#[test]
fn impossible_eager_second_type_query_fails_closed() {
    let prior = CitiesCaptureResidualPrior::SkipPlunder(gate(
        CitiesCapturePlunderGateContinuation::SkipPlunder0x00734a3c,
        false,
        false,
    ));
    let plan = plan_cities_capture_residual(prior).unwrap();
    let mut world = World::canonical();
    world.corrupt_inspection = true;
    assert_eq!(
        apply_cities_capture_residual(plan, &mut world),
        Err(CitiesCaptureResidualApplyError::InspectionShapeMismatch)
    );
}

#[test]
fn common_capture_rejects_nonplayable_console_owner() {
    let prior = CitiesCaptureResidualPrior::SkipPlunder(gate(
        CitiesCapturePlunderGateContinuation::SkipPlunder0x00734a3c,
        false,
        false,
    ));
    let plan = plan_cities_capture_residual(prior).unwrap();
    let mut world = World::canonical();
    world.console = -1;
    assert_eq!(
        apply_cities_capture_residual(plan, &mut world),
        Err(CitiesCaptureResidualApplyError::InvalidConsoleOwner)
    );
}

#[test]
fn capital_refund_entry_requires_a_real_refund_value() {
    let missing = CitiesCaptureResidualPrior::PlunderAward(CitiesCapturePlunderAwardReceipt {
        prior: gate(
            CitiesCapturePlunderGateContinuation::EnterPlunder0x00733fac,
            false,
            false,
        ),
        sized_capital_plunder: Some(100),
        new_owner_award: Some(100),
        old_owner_refund: None,
        continuation: CitiesCapturePlunderAwardContinuation::OldOwnerRefund0x0073432d,
        events: vec![],
    });
    assert_eq!(
        plan_cities_capture_residual(missing),
        Err(CitiesCaptureResidualPlanError::MissingRefund)
    );
}
