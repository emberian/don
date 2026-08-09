use super::*;

const OWNER: u8 = 1;
const WORKER: GatherObjectKey = GatherObjectKey { owner: OWNER, o: 7 };
const FARM: GatherObjectKey = GatherObjectKey { owner: OWNER, o: 3 };

fn farm_source() -> AuthoritativeGatherPayoutSource {
    AuthoritativeGatherPayoutSource {
        site_type: AuthoritativeGatherSiteType {
            type_index: 417,
            kind: OrdinaryGatherKind::Farm,
            x_size: 2,
            y_size: 2,
            gather_radius: None,
        },
        placement: AuthoritativeGatherSitePlacement {
            centre_x: Coord(0x480),
            centre_y: Coord(0x480),
            corner_tx: 5,
            corner_ty: 5,
            region: 2,
        },
    }
}

fn active_farm_runtime() -> ArenaGatherRuntime {
    let mut runtime = ArenaGatherRuntime::default();
    runtime.register_worker(WORKER, 0x32).unwrap();
    runtime
        .register_site(
            FARM,
            11,
            OrdinaryGatherKind::Farm,
            GatherCapacityAuthority::FlatFarmOne,
        )
        .unwrap();
    runtime.stage_exact_order(WORKER, FARM).unwrap();
    assert_eq!(
        runtime.attach_staged_worker(WORKER, FARM),
        Ok(AttachResult::Attached)
    );
    runtime.workers[0]
        .assignment
        .as_mut()
        .expect("staged assignment")
        .been_there = true;
    runtime
}

#[derive(Default)]
struct RecordingEvaluator {
    calls: usize,
}

impl GatherPerWorkerEvaluator for RecordingEvaluator {
    type Error = &'static str;

    fn evaluate(
        &mut self,
        request: GatherPerWorkerEvaluationRequest<'_>,
    ) -> Result<[i32; NUM_RESOURCES], Self::Error> {
        self.calls += 1;
        assert_eq!(request.site_key, FARM);
        assert_eq!(request.source, farm_source());
        assert_eq!(request.active_workers, 1);
        assert_eq!(request.authoritative_capacity, 1);
        assert!(request.mining.is_empty());

        // This test host stands in for the still-unported retail evaluator.  It returns
        // a complete vector; the Arena boundary neither chooses a resource nor inserts a
        // rate.  160 is the shipped PEASANT_RATE after the recovered 8.8 and <<4 steps.
        let mut evaluated = [0; NUM_RESOURCES];
        evaluated[0] = gathering::base_worker_gross(&EconRules::shipped(), false);
        Ok(evaluated)
    }
}

#[test]
fn evaluator_boundary_is_mandatory_and_failure_is_transactional() {
    let mut runtime = active_farm_runtime();
    let initial = GatherLeaderPayoutState {
        leader_slot: 1,
        econ: LeaderEcon::new(),
        last_calc_frame: 123,
        dirty: false,
    };
    runtime
        .bind_authoritative_leader_economy(OWNER, initial)
        .unwrap();

    let mut should_not_run =
        |_request: GatherPerWorkerEvaluationRequest<'_>| -> Result<
            [i32; NUM_RESOURCES],
            &'static str,
        > { panic!("missing source must fail before the evaluator") };
    assert_eq!(
        runtime.evaluate_authoritative_per_worker(FARM, &mut should_not_run),
        Err(GatherPerWorkerEvaluationError::Runtime(
            GatherRuntimeError::MissingAuthoritativePayoutSource(FARM)
        ))
    );

    runtime
        .bind_authoritative_payout_source(FARM, farm_source())
        .unwrap();
    let before = runtime.authoritative_leader_economy(OWNER).unwrap();
    let mut failing = |_request: GatherPerWorkerEvaluationRequest<'_>| {
        Err::<[i32; NUM_RESOURCES], _>("retail evaluator unavailable")
    };
    assert_eq!(
        runtime.evaluate_authoritative_per_worker(FARM, &mut failing),
        Err(GatherPerWorkerEvaluationError::Evaluator(
            "retail evaluator unavailable"
        ))
    );
    assert_eq!(runtime.authoritative_leader_economy(OWNER), Ok(before));
    assert_eq!(
        runtime.authoritative_owner_gross(OWNER),
        Err(GatherRuntimeError::MissingAuthoritativePayout(FARM))
    );
}

#[test]
fn leader_transaction_matches_don_sim_caps_carry_stockpile_and_checksum_image() {
    let rules = EconRules::shipped();
    let mut runtime = active_farm_runtime();
    let mut econ = LeaderEcon::new();
    econ.age = 2;
    econ.age_alt = 2;
    econ.stockpile = [4, 5, 6, 7, 8, 9];
    econ.accumulator[0] = economy::accumulator_period(&rules) - 1;
    econ.expense = [91, 92, 93, 94, 95, 96];
    econ.gross = [3, 4, 5, 6, 7, 8];
    runtime
        .bind_authoritative_leader_economy(
            OWNER,
            GatherLeaderPayoutState {
                leader_slot: 1,
                econ,
                last_calc_frame: 10_000,
                dirty: false,
            },
        )
        .unwrap();
    runtime
        .bind_authoritative_payout_source(FARM, farm_source())
        .unwrap();

    let mut evaluator = RecordingEvaluator::default();
    let evaluation = runtime
        .evaluate_authoritative_per_worker(FARM, &mut evaluator)
        .unwrap();
    assert_eq!(evaluator.calls, 1);
    assert_eq!(evaluation.evaluated, [160, 0, 0, 0, 0, 0]);

    let frame = 7; // (leader slot 1 + frame 7) % 8 == 0 on the dirty schedule.
    let mut non_site = GatherInputs::default();
    non_site.object_income = [13, 17, 19, 23, 29, 31];
    let caps = CapGates::default();
    let context = DoGatherContext::default();

    let before = runtime.authoritative_leader_economy(OWNER).unwrap();
    assert!(
        before.dirty,
        "source/evaluation changes must dirty Leader economy"
    );
    let site_gross = [160, 0, 0, 0, 0, 0];
    let mut expected_inputs = non_site.clone();
    for (income, site) in expected_inputs.object_income.iter_mut().zip(site_gross) {
        *income = income.wrapping_add(site);
    }
    let mut expected = before;
    let expected_payouts = economy::leader_gather(
        &rules,
        &mut expected.econ,
        frame,
        expected.leader_slot,
        &mut expected.last_calc_frame,
        &mut expected.dirty,
        &expected_inputs,
        &caps,
        &context,
    );

    let receipt = runtime
        .execute_authoritative_owner_payout(OWNER, &rules, frame, &non_site, &caps, &context)
        .unwrap();
    let after = runtime.authoritative_leader_economy(OWNER).unwrap();

    assert_eq!(
        after, expected,
        "Arena must not duplicate Leader arithmetic"
    );
    assert_eq!(receipt.evaluated_site_gross, site_gross);
    assert_eq!(
        receipt.composed_object_income,
        expected_inputs.object_income
    );
    assert!(receipt.gross_recomputed);
    assert_eq!(receipt.payouts, expected_payouts);
    assert_eq!(receipt.commerce_cap, expected.econ.commerce_cap);
    assert_eq!(receipt.expenses, [0; NUM_RESOURCES]);
    assert_eq!(receipt.stockpile, expected.econ.stockpile);
    assert_eq!(receipt.accumulators, expected.econ.accumulator);
    assert_eq!(receipt.modelled_econ_adler32_before, before.econ.adler32());
    assert_eq!(receipt.modelled_econ_adler32_after, after.econ.adler32());
    assert_ne!(
        receipt.modelled_econ_adler32_before,
        receipt.modelled_econ_adler32_after
    );
    assert!(after.econ.stockpile[0] > before.econ.stockpile[0]);
}
