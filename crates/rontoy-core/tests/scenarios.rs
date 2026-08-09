use rontoy_core::*;

fn provenance() -> FieldProvenance {
    FieldProvenance::direct(TelemetrySource::ProcessMemory)
}

fn observed<T>(value: T) -> Observed<T> {
    Observed::new(value, provenance())
}

fn economy() -> EconomyTelemetry {
    EconomyTelemetry {
        stock: ResourceVector::from_whole([100; 6]),
        income_per_minute: ResourceVector::from_whole([10; 6]),
        planned_unpaid_cost: ResourceVector::ZERO,
        commerce_cap: ResourceVector::from_whole([500; 6]),
        commerce_status: CommerceStatus::default(),
    }
}

fn population() -> PopulationTelemetry {
    PopulationTelemetry {
        used: 50,
        cap: 100,
        paid_queue_population: 0,
        blocked_paid_population: 0,
        next_paid_completion_ms: None,
        incoming_capacity: 0,
        incoming_capacity_eta_ms: None,
    }
}

fn labor() -> LaborTelemetry {
    LaborTelemetry {
        citizens: 50,
        idle_citizens: 0,
        idle_for_ms: 0,
        assigned: ResourceWorkers([10, 10, 10, 10, 5, 5]),
        free_gather_slots: ResourceWorkers::default(),
        rebalance: None,
    }
}

fn snapshot(time_ms: u64) -> TelemetrySnapshot {
    TelemetrySnapshot {
        meta: SnapshotMeta {
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            match_id: 7,
            sample_sequence: time_ms / 100 + 1,
            game_tick: time_ms / 16,
            captured_at_ms: time_ms,
            analyzed_at_ms: time_ms,
            paused: false,
            coherent: true,
            completeness: SnapshotCompleteness::Complete,
            identity: HumanIdentity::Confirmed,
            game_mode: GameMode::SinglePlayer,
        },
        economy: Some(observed(economy())),
        population: Some(observed(population())),
        labor: Some(observed(labor())),
        production: Some(observed(ProductionTelemetry { sites: vec![] })),
        progression: Some(observed(ProgressionTelemetry {
            current_age: 2,
            opportunities: vec![],
        })),
        pressure: Some(observed(PressureTelemetry {
            under_attack: false,
            own_local_strength: 0,
            observed_enemy_local_strength: 0,
            enemy_observation_age_ms: 0,
            enemy_visibility: EnemyVisibility::Unknown,
            recent_economy_spend_permille: 0,
        })),
    }
}

fn fast_config() -> CoachConfig {
    CoachConfig {
        sustain_ms: 100,
        resolve_hysteresis_ms: 100,
        advice_ttl_ms: 500,
        per_key_cooldown_ms: 500,
        update_cooldown_ms: 500,
        global_raise_interval_ms: 0,
        ..CoachConfig::default()
    }
}

fn raised_key(batch: &AdviceBatch) -> Option<&AdviceKey> {
    batch.events.iter().find_map(|event| match event {
        AdviceEvent::Raised(card) => Some(&card.key),
        _ => None,
    })
}

fn set_idle(snapshot: &mut TelemetrySnapshot, idle: u32, duration_ms: u64, free_food: u16) {
    let labor = &mut snapshot.labor.as_mut().unwrap().value;
    labor.idle_citizens = idle;
    labor.idle_for_ms = duration_ms;
    labor.free_gather_slots.0[Resource::Food as usize] = free_food;
}

#[test]
fn retail_resource_order_is_an_explicit_abi() {
    let vector = ResourceVector::from_milli([11, 22, 33, 44, 55, 66]);
    assert_eq!(vector.get(Resource::Food), 11);
    assert_eq!(vector.get(Resource::Timber), 22);
    assert_eq!(vector.get(Resource::Wealth), 33);
    assert_eq!(vector.get(Resource::Knowledge), 44);
    assert_eq!(vector.get(Resource::Metal), 55);
    assert_eq!(vector.get(Resource::Oil), 66);
}

#[test]
fn confidence_is_bounded_and_resource_math_is_integer_only() {
    assert_eq!(Confidence::new(65_535).permille(), 1_000);
    let income = ResourceVector::from_milli([227_000, 326_000, 352_875, 340_000, 370_000, 0]);
    assert_eq!(income.get(Resource::Wealth), 352_875);
}

#[test]
fn live_dutch_fixture_is_observation_only_when_partial() {
    // Coherent human witness, but current population and queue semantics were
    // not available.  Exact stock/rate order is food,timber,wealth,knowledge,metal,oil.
    let mut s = snapshot(1_000);
    s.meta.completeness = SnapshotCompleteness::Partial;
    let e = &mut s.economy.as_mut().unwrap().value;
    e.stock = ResourceVector::from_whole([352, 333, 950, 1_997, 3_556, 0]);
    e.income_per_minute =
        ResourceVector::from_milli([227_000, 326_000, 352_875, 340_000, 370_000, 0]);
    let result = AdviceEngine::new(fast_config()).analyze(&s);
    assert!(result.active.is_empty());
    assert_eq!(result.suppressed, Some(SuppressionReason::PartialSnapshot));
}

#[test]
fn every_global_quality_failure_suppresses_advice() {
    let cases = [
        ("schema", SuppressionReason::UnsupportedSchema),
        ("partial", SuppressionReason::PartialSnapshot),
        ("racy", SuppressionReason::IncoherentSnapshot),
        ("ambiguous", SuppressionReason::AmbiguousIdentity),
        ("ai", SuppressionReason::NonHuman),
        ("unknown-mode", SuppressionReason::UnknownGameMode),
        ("multiplayer", SuppressionReason::Multiplayer),
        ("paused", SuppressionReason::Paused),
        ("stale", SuppressionReason::StaleSnapshot),
        ("invalid", SuppressionReason::InvalidValues),
    ];
    for (case, expected) in cases {
        let mut s = snapshot(10_000);
        set_idle(&mut s, 10, 30_000, 10);
        match case {
            "schema" => s.meta.schema_version += 1,
            "partial" => s.meta.completeness = SnapshotCompleteness::Partial,
            "racy" => s.meta.coherent = false,
            "ambiguous" => s.meta.identity = HumanIdentity::Ambiguous,
            "ai" => s.meta.identity = HumanIdentity::NonHuman,
            "unknown-mode" => s.meta.game_mode = GameMode::Unknown,
            "multiplayer" => s.meta.game_mode = GameMode::Multiplayer,
            "paused" => s.meta.paused = true,
            "stale" => s.meta.captured_at_ms = 1_000,
            "invalid" => s.economy.as_mut().unwrap().value.commerce_status.0[0] = 3,
            _ => unreachable!(),
        }
        let result = AdviceEngine::new(fast_config()).analyze(&s);
        assert_eq!(result.suppressed, Some(expected), "case {case}");
        assert!(result.active.is_empty(), "case {case}");
    }
}

#[test]
fn a_claimed_complete_snapshot_with_a_missing_group_is_still_partial() {
    let mut s = snapshot(1_000);
    s.economy = None;
    let result = AdviceEngine::new(fast_config()).analyze(&s);
    assert_eq!(result.suppressed, Some(SuppressionReason::PartialSnapshot));
    assert!(result.active.is_empty());
}

#[test]
fn leader_r1_can_advise_with_optional_groups_absent() {
    let mut engine = AdviceEngine::new(fast_config());
    for time in [0, 100] {
        let mut s = snapshot(time);
        s.production = None;
        s.progression = None;
        s.pressure = None;
        set_idle(&mut s, 2, 10_000, 4);
        let batch = engine.analyze(&s);
        assert_eq!(batch.suppressed, None);
        if time == 100 {
            assert_eq!(raised_key(&batch), Some(&AdviceKey::IdleCitizens));
        }
    }
}

#[test]
fn source_can_be_stuck_while_sequence_increases() {
    let mut engine = AdviceEngine::new(fast_config());
    let first = snapshot(1_000);
    engine.analyze(&first);

    let mut stuck = snapshot(4_000);
    stuck.meta.sample_sequence = first.meta.sample_sequence + 1;
    stuck.meta.game_tick = first.meta.game_tick;
    stuck.meta.captured_at_ms = first.meta.captured_at_ms;
    let result = engine.analyze(&stuck);
    assert_eq!(result.suppressed, Some(SuppressionReason::StaleSnapshot));
}

#[test]
fn repeated_unpaused_game_frame_cannot_satisfy_sustain_time() {
    let mut engine = AdviceEngine::new(fast_config());
    let mut first = snapshot(0);
    set_idle(&mut first, 5, 10_000, 5);
    engine.analyze(&first);

    let mut frozen = snapshot(100);
    frozen.meta.game_tick = first.meta.game_tick;
    set_idle(&mut frozen, 5, 10_000, 5);
    assert_eq!(
        engine.analyze(&frozen).suppressed,
        Some(SuppressionReason::SampleWentBackwards)
    );

    let mut resumed = snapshot(200);
    set_idle(&mut resumed, 5, 10_000, 5);
    // The frozen source cleared pending hysteresis; this is first-seen again.
    assert!(engine.analyze(&resumed).active.is_empty());
}

#[test]
fn frame_or_clock_rewind_is_suppressed_without_poisoning_next_sample() {
    let mut engine = AdviceEngine::new(fast_config());
    engine.analyze(&snapshot(1_000));

    let mut rewind = snapshot(1_100);
    rewind.meta.game_tick = 1;
    assert_eq!(
        engine.analyze(&rewind).suppressed,
        Some(SuppressionReason::SampleWentBackwards)
    );

    let valid = snapshot(1_200);
    assert_eq!(engine.analyze(&valid).suppressed, None);

    let mut clock_rewind = snapshot(1_300);
    clock_rewind.meta.analyzed_at_ms = 900;
    clock_rewind.meta.captured_at_ms = 900;
    assert_eq!(
        engine.analyze(&clock_rewind).suppressed,
        Some(SuppressionReason::TimeWentBackwards)
    );
}

#[test]
fn match_change_retracts_old_cards_and_resets_cooldowns() {
    let mut engine = AdviceEngine::new(fast_config());
    let mut s0 = snapshot(0);
    set_idle(&mut s0, 5, 10_000, 5);
    engine.analyze(&s0);
    let mut s1 = snapshot(100);
    set_idle(&mut s1, 5, 10_000, 5);
    assert_eq!(
        raised_key(&engine.analyze(&s1)),
        Some(&AdviceKey::IdleCitizens)
    );

    let mut next_match = snapshot(200);
    next_match.meta.match_id = 8;
    next_match.meta.sample_sequence = 1;
    next_match.meta.game_tick = 1;
    let batch = engine.analyze(&next_match);
    assert!(batch.events.iter().any(|event| matches!(
        event,
        AdviceEvent::Retracted {
            key: AdviceKey::IdleCitizens,
            reason: RetractionReason::MatchChanged
        }
    )));
    assert!(batch.active.is_empty());
}

#[test]
fn one_frame_idle_blip_does_not_raise_and_valid_slots_are_required() {
    let mut engine = AdviceEngine::new(fast_config());
    let mut s0 = snapshot(0);
    set_idle(&mut s0, 3, 10_000, 3);
    assert!(engine.analyze(&s0).active.is_empty());
    assert!(engine.analyze(&snapshot(100)).active.is_empty());

    let mut no_slots0 = snapshot(200);
    set_idle(&mut no_slots0, 3, 10_000, 0);
    engine.analyze(&no_slots0);
    let mut no_slots1 = snapshot(300);
    set_idle(&mut no_slots1, 3, 10_000, 0);
    assert!(engine.analyze(&no_slots1).active.is_empty());
}

#[test]
fn sustained_idle_with_slots_raises_evidenced_card() {
    let mut engine = AdviceEngine::new(fast_config());
    let mut s0 = snapshot(0);
    set_idle(&mut s0, 2, 10_000, 4);
    engine.analyze(&s0);
    let mut s1 = snapshot(100);
    set_idle(&mut s1, 2, 10_000, 4);
    let batch = engine.analyze(&s1);
    let card = &batch.active[0];
    assert_eq!(card.key, AdviceKey::IdleCitizens);
    assert_eq!(card.severity, Severity::Notice);
    assert_eq!(card.confidence, Confidence::new(900));
    assert_eq!(card.rule.id, "labor.sustained_idle");
    assert_eq!(card.rule.version, 1);
    assert_eq!(card.confidence_breakdown.transport, Confidence::MAX);
    assert!(card.expires_at_ms > card.updated_at_ms);
    assert!(card
        .evidence
        .iter()
        .all(|item| item.provenance.source == TelemetrySource::ProcessMemory));
}

#[test]
fn legal_over_cap_is_silent_without_an_evidenced_paid_queue() {
    let mut engine = AdviceEngine::new(fast_config());
    for time in [0, 100] {
        let mut s = snapshot(time);
        let p = &mut s.population.as_mut().unwrap().value;
        p.used = 180;
        p.cap = 150;
        assert!(engine.analyze(&s).active.is_empty());
    }
}

#[test]
fn paid_queue_block_is_the_population_trigger() {
    let mut engine = AdviceEngine::new(fast_config());
    for time in [0, 100] {
        let mut s = snapshot(time);
        let p = &mut s.population.as_mut().unwrap().value;
        p.used = 100;
        p.cap = 100;
        p.paid_queue_population = 3;
        p.blocked_paid_population = 3;
        let batch = engine.analyze(&s);
        if time == 100 {
            assert_eq!(raised_key(&batch), Some(&AdviceKey::PopulationBlock));
        }
    }
}

#[test]
fn incoming_capacity_before_paid_queue_completion_suppresses_pop_warning() {
    let mut engine = AdviceEngine::new(fast_config());
    for time in [0, 100] {
        let mut s = snapshot(time);
        let p = &mut s.population.as_mut().unwrap().value;
        p.used = 98;
        p.cap = 100;
        p.paid_queue_population = 5;
        p.next_paid_completion_ms = Some(10_000);
        p.incoming_capacity = 10;
        p.incoming_capacity_eta_ms = Some(5_000);
        assert!(engine.analyze(&s).active.is_empty());
    }
}

#[test]
fn aggregate_attack_class_counts_cannot_masquerade_as_producer_queues() {
    // The adapter may observe aggregate [combat,barracks,stable,...] counters,
    // but rontoy-core has no field for them. Only a validated ProductionSite
    // can produce idle-production advice.
    let mut engine = AdviceEngine::new(fast_config());
    for time in [0, 100] {
        let mut s = snapshot(time);
        s.production.as_mut().unwrap().value.sites.clear();
        assert!(engine.analyze(&s).active.is_empty());
    }
}

#[test]
fn empty_production_requires_intent_affordability_and_sustained_idle() {
    let mut engine = AdviceEngine::new(fast_config());
    for time in [0, 100] {
        let mut s = snapshot(time);
        s.production
            .as_mut()
            .unwrap()
            .value
            .sites
            .push(ProductionSite {
                stable_id: 42,
                class: ProductionClass::LandMilitary,
                enabled: true,
                expected_active: true,
                queue_len: 0,
                idle_for_ms: 20_000,
                affordable_option_observed: true,
            });
        let batch = engine.analyze(&s);
        if time == 100 {
            assert_eq!(raised_key(&batch), Some(&AdviceKey::IdleProduction(42)));
        }
    }
}

#[test]
fn falling_stock_does_not_create_a_fake_bottleneck_after_spending() {
    let mut engine = AdviceEngine::new(fast_config());
    for (time, stock) in [(0, 1_000), (100, 10)] {
        let mut s = snapshot(time);
        s.economy.as_mut().unwrap().value.stock = ResourceVector::from_whole([stock; 6]);
        // There is no selected future cost. Historical deltas are deliberately
        // absent from the contract because spending confounds them.
        assert!(engine.analyze(&s).active.is_empty());
    }
}

#[test]
fn sustained_planned_cost_shortfall_is_labeled_as_a_model() {
    let mut engine = AdviceEngine::new(fast_config());
    for time in [0, 100] {
        let mut s = snapshot(time);
        let e = &mut s.economy.as_mut().unwrap().value;
        e.stock.set(Resource::Food, 10_000);
        e.income_per_minute.set(Resource::Food, 10_000);
        e.planned_unpaid_cost.set(Resource::Food, 100_000);
        let batch = engine.analyze(&s);
        if time == 100 {
            let card = batch
                .active
                .iter()
                .find(|card| card.key == AdviceKey::ResourceBottleneck(Resource::Food))
                .unwrap();
            assert!(card.message.contains("approximate"));
            assert!(card.message.contains("fixed income"));
            assert!(card.message.contains("no other spending"));
        }
    }
}

#[test]
fn direct_commerce_over_cap_is_advice_but_not_a_worker_assignment() {
    let mut engine = AdviceEngine::new(fast_config());
    for time in [0, 100] {
        let mut s = snapshot(time);
        s.economy.as_mut().unwrap().value.commerce_status.0[Resource::Timber as usize] = 1;
        let batch = engine.analyze(&s);
        if time == 100 {
            assert_eq!(
                raised_key(&batch),
                Some(&AdviceKey::CommerceCap(Resource::Timber))
            );
            assert!(!batch.active[0].message.contains("spend"));
            assert!(batch.active[0].message.contains("status is 1"));
        }
    }
}

#[test]
fn rebalance_requires_slots_path_gain_and_headroom() {
    for invalid_case in 0..4 {
        let mut engine = AdviceEngine::new(fast_config());
        for time in [0, 100] {
            let mut s = snapshot(time);
            let mut opportunity = RebalanceOpportunity {
                from: Resource::Food,
                to: Resource::Timber,
                workers: 2,
                destination_free_slots: 2,
                path_feasible: true,
                marginal_gain_milli_per_minute: 5_000,
                commerce_headroom_milli: 10_000,
            };
            match invalid_case {
                0 => opportunity.destination_free_slots = 1,
                1 => opportunity.path_feasible = false,
                2 => opportunity.marginal_gain_milli_per_minute = 0,
                3 => opportunity.commerce_headroom_milli = 0,
                _ => unreachable!(),
            }
            s.labor.as_mut().unwrap().value.rebalance = Some(opportunity);
            assert!(engine.analyze(&s).active.is_empty());
        }
    }
}

#[test]
fn invalid_rebalance_and_duplicate_stable_ids_are_rejected() {
    for case in 0..3 {
        let mut s = snapshot(1_000);
        match case {
            0 => {
                s.labor.as_mut().unwrap().value.rebalance = Some(RebalanceOpportunity {
                    from: Resource::Food,
                    to: Resource::Food,
                    workers: 1,
                    destination_free_slots: 1,
                    path_feasible: true,
                    marginal_gain_milli_per_minute: 1,
                    commerce_headroom_milli: 1,
                });
            }
            1 => {
                let site = ProductionSite {
                    stable_id: 5,
                    class: ProductionClass::Citizen,
                    enabled: true,
                    expected_active: false,
                    queue_len: 0,
                    idle_for_ms: 0,
                    affordable_option_observed: false,
                };
                s.production.as_mut().unwrap().value.sites = vec![site.clone(), site];
            }
            2 => {
                let opportunity = ProgressionOpportunity {
                    stable_id: 5,
                    name: "test".into(),
                    kind: OpportunityKind::EconomyTech,
                    cost: ResourceVector::ZERO,
                    tracked_goal: false,
                    already_queued: false,
                };
                s.progression.as_mut().unwrap().value.opportunities =
                    vec![opportunity.clone(), opportunity];
            }
            _ => unreachable!(),
        }
        assert_eq!(
            AdviceEngine::new(fast_config()).analyze(&s).suppressed,
            Some(SuppressionReason::InvalidValues)
        );
    }
}

#[test]
fn progression_is_observational_and_requires_a_tracked_goal() {
    let mut untracked = AdviceEngine::new(fast_config());
    for time in [0, 100] {
        let mut s = snapshot(time);
        s.progression
            .as_mut()
            .unwrap()
            .value
            .opportunities
            .push(ProgressionOpportunity {
                stable_id: 9,
                name: "Age IV".into(),
                kind: OpportunityKind::Age,
                cost: ResourceVector::from_whole([50; 6]),
                tracked_goal: false,
                already_queued: false,
            });
        assert!(untracked.analyze(&s).active.is_empty());
    }

    let mut tracked = AdviceEngine::new(fast_config());
    for time in [0, 100] {
        let mut s = snapshot(time);
        s.progression
            .as_mut()
            .unwrap()
            .value
            .opportunities
            .push(ProgressionOpportunity {
                stable_id: 9,
                name: "Age IV".into(),
                kind: OpportunityKind::Age,
                cost: ResourceVector::from_whole([50; 6]),
                tracked_goal: true,
                already_queued: false,
            });
        let batch = tracked.analyze(&s);
        if time == 100 {
            let card = &batch.active[0];
            assert_eq!(card.key, AdviceKey::ProgressionAffordable(9));
            assert_eq!(card.message, "Tracked goal 'Age IV' is now affordable.");
        }
    }
}

#[test]
fn military_and_economy_pressure_are_ranked_from_direct_evidence() {
    let mut engine = AdviceEngine::new(CoachConfig {
        global_raise_interval_ms: 200,
        enable_pressure_advice: true,
        ..fast_config()
    });
    for time in [0, 100, 300] {
        let mut s = snapshot(time);
        s.pressure.as_mut().unwrap().value = PressureTelemetry {
            under_attack: true,
            own_local_strength: 100,
            observed_enemy_local_strength: 200,
            enemy_observation_age_ms: 500,
            enemy_visibility: EnemyVisibility::DirectlyVisible,
            recent_economy_spend_permille: 800,
        };
        let batch = engine.analyze(&s);
        if time == 100 {
            assert_eq!(raised_key(&batch), Some(&AdviceKey::LocalMilitaryPressure));
            assert_eq!(batch.active[0].severity, Severity::Critical);
        }
        if time == 300 {
            assert!(batch
                .active
                .iter()
                .any(|card| card.key == AdviceKey::EconomyUnderPressure));
        }
    }
}

#[test]
fn hidden_or_unenabled_pressure_never_produces_a_card() {
    for (enabled, visibility) in [
        (false, EnemyVisibility::DirectlyVisible),
        (true, EnemyVisibility::Remembered),
        (true, EnemyVisibility::Unknown),
    ] {
        let mut engine = AdviceEngine::new(CoachConfig {
            enable_pressure_advice: enabled,
            ..fast_config()
        });
        for time in [0, 100] {
            let mut s = snapshot(time);
            s.pressure.as_mut().unwrap().value = PressureTelemetry {
                under_attack: true,
                own_local_strength: 10,
                observed_enemy_local_strength: 1_000,
                enemy_observation_age_ms: 0,
                enemy_visibility: visibility,
                recent_economy_spend_permille: 1_000,
            };
            assert!(engine.analyze(&s).active.is_empty());
        }
    }
}

#[test]
fn lifecycle_raises_updates_resolves_and_respects_cooldown() {
    let mut engine = AdviceEngine::new(fast_config());
    let mut s0 = snapshot(0);
    set_idle(&mut s0, 2, 10_000, 4);
    assert!(engine.analyze(&s0).events.is_empty());

    let mut s1 = snapshot(100);
    set_idle(&mut s1, 2, 10_000, 4);
    assert_eq!(
        raised_key(&engine.analyze(&s1)),
        Some(&AdviceKey::IdleCitizens)
    );

    // Escalation updates immediately despite the ordinary update cooldown.
    let mut s2 = snapshot(200);
    set_idle(&mut s2, 10, 10_000, 4);
    let update = engine.analyze(&s2);
    assert!(update.events.iter().any(|event| matches!(
        event,
        AdviceEvent::Updated(card) if card.key == AdviceKey::IdleCitizens && card.severity == Severity::Warning
    )));

    assert_eq!(engine.analyze(&snapshot(300)).active.len(), 1);
    let resolved = engine.analyze(&snapshot(400));
    assert!(resolved.events.iter().any(|event| matches!(
        event,
        AdviceEvent::Retracted {
            key: AdviceKey::IdleCitizens,
            reason: RetractionReason::Resolved
        }
    )));

    // The condition returns but cannot raise before the per-key cooldown.
    for time in [500, 600] {
        let mut s = snapshot(time);
        set_idle(&mut s, 10, 10_000, 4);
        assert!(raised_key(&engine.analyze(&s)).is_none());
    }
    let mut s700 = snapshot(700);
    set_idle(&mut s700, 10, 10_000, 4);
    assert_eq!(
        raised_key(&engine.analyze(&s700)),
        Some(&AdviceKey::IdleCitizens)
    );
}

#[test]
fn confidence_facet_change_emits_update_when_overall_score_is_unchanged() {
    let mut engine = AdviceEngine::new(CoachConfig {
        update_cooldown_ms: 0,
        ..fast_config()
    });
    for time in [0, 100] {
        let mut s = snapshot(time);
        let economy_provenance = &mut s.economy.as_mut().unwrap().provenance;
        economy_provenance.transport_confidence = Confidence::new(800);
        economy_provenance.semantic_confidence = Confidence::new(900);
        s.progression
            .as_mut()
            .unwrap()
            .value
            .opportunities
            .push(ProgressionOpportunity {
                stable_id: 99,
                name: "tracked".into(),
                kind: OpportunityKind::EconomyTech,
                cost: ResourceVector::ZERO,
                tracked_goal: true,
                already_queued: false,
            });
        engine.analyze(&s);
    }

    let mut changed = snapshot(200);
    let economy_provenance = &mut changed.economy.as_mut().unwrap().provenance;
    economy_provenance.transport_confidence = Confidence::new(800);
    economy_provenance.semantic_confidence = Confidence::new(800);
    changed
        .progression
        .as_mut()
        .unwrap()
        .value
        .opportunities
        .push(ProgressionOpportunity {
            stable_id: 99,
            name: "tracked".into(),
            kind: OpportunityKind::EconomyTech,
            cost: ResourceVector::ZERO,
            tracked_goal: true,
            already_queued: false,
        });
    let batch = engine.analyze(&changed);
    assert!(batch.events.iter().any(|event| matches!(
        event,
        AdviceEvent::Updated(card)
            if card.key == AdviceKey::ProgressionAffordable(99)
                && card.confidence == Confidence::new(800)
                && card.confidence_breakdown.semantic == Confidence::new(800)
    )));
}

#[test]
fn losing_an_optional_source_immediately_retracts_only_its_card() {
    let mut engine = AdviceEngine::new(fast_config());
    for time in [0, 100] {
        let mut s = snapshot(time);
        s.production
            .as_mut()
            .unwrap()
            .value
            .sites
            .push(ProductionSite {
                stable_id: 7,
                class: ProductionClass::LandMilitary,
                enabled: true,
                expected_active: true,
                queue_len: 0,
                idle_for_ms: 20_000,
                affordable_option_observed: true,
            });
        engine.analyze(&s);
    }

    let mut lost = snapshot(200);
    lost.production = None;
    let batch = engine.analyze(&lost);
    assert_eq!(batch.suppressed, None);
    assert!(batch.events.iter().any(|event| matches!(
        event,
        AdviceEvent::Retracted {
            key: AdviceKey::IdleProduction(7),
            reason: RetractionReason::DataQuality(SuppressionReason::PartialSnapshot)
        }
    )));
}

#[test]
fn missing_card_expires_when_ttl_precedes_resolution_hysteresis() {
    let mut engine = AdviceEngine::new(CoachConfig {
        resolve_hysteresis_ms: 5_000,
        advice_ttl_ms: 200,
        ..fast_config()
    });
    for time in [0, 100] {
        let mut s = snapshot(time);
        set_idle(&mut s, 2, 10_000, 4);
        engine.analyze(&s);
    }
    engine.analyze(&snapshot(200));
    let expired = engine.analyze(&snapshot(300));
    assert!(expired.events.iter().any(|event| matches!(
        event,
        AdviceEvent::Retracted {
            key: AdviceKey::IdleCitizens,
            reason: RetractionReason::Expired
        }
    )));
}

#[test]
fn stale_or_paused_input_retracts_visible_advice() {
    for paused in [false, true] {
        let mut engine = AdviceEngine::new(fast_config());
        for time in [0, 100] {
            let mut s = snapshot(time);
            set_idle(&mut s, 2, 10_000, 4);
            engine.analyze(&s);
        }
        let mut bad = snapshot(3_000);
        if paused {
            bad.meta.paused = true;
        } else {
            bad.meta.captured_at_ms = 0;
        }
        let batch = engine.analyze(&bad);
        assert!(batch.active.is_empty());
        assert!(batch.events.iter().any(|event| matches!(
            event,
            AdviceEvent::Retracted {
                key: AdviceKey::IdleCitizens,
                reason: RetractionReason::DataQuality(_)
            }
        )));
    }
}

#[test]
fn global_rate_limit_allows_only_one_new_card_per_interval() {
    let mut engine = AdviceEngine::new(CoachConfig {
        global_raise_interval_ms: 200,
        ..fast_config()
    });
    for time in [0, 100, 200, 300] {
        let mut s = snapshot(time);
        set_idle(&mut s, 2, 10_000, 4);
        s.economy.as_mut().unwrap().value.commerce_status.0[0] = 1;
        let batch = engine.analyze(&s);
        let raises = batch
            .events
            .iter()
            .filter(|event| matches!(event, AdviceEvent::Raised(_)))
            .count();
        assert!(raises <= 1);
        if time == 100 {
            assert_eq!(batch.active.len(), 1);
        }
        if time == 300 {
            assert_eq!(batch.active.len(), 2);
        }
    }
}

#[test]
fn identical_histories_produce_identical_ranked_cards_and_events() {
    let history: Vec<_> = [0, 100, 200, 300]
        .into_iter()
        .map(|time| {
            let mut s = snapshot(time);
            set_idle(&mut s, 4, 10_000, 4);
            s.economy.as_mut().unwrap().value.commerce_status.0[Resource::Metal as usize] = 2;
            s
        })
        .collect();
    let mut left = AdviceEngine::new(fast_config());
    let mut right = AdviceEngine::new(fast_config());
    for s in history {
        assert_eq!(left.analyze(&s), right.analyze(&s));
    }
}

#[test]
fn old_component_suppresses_the_snapshot_instead_of_leaving_old_cards_visible() {
    let mut engine = AdviceEngine::new(fast_config());
    for time in [0, 100] {
        let mut s = snapshot(time);
        set_idle(&mut s, 2, 10_000, 4);
        engine.analyze(&s);
    }

    let mut old = snapshot(200);
    old.economy.as_mut().unwrap().provenance.age_ms = 20_000;
    set_idle(&mut old, 2, 10_000, 4);
    let batch = engine.analyze(&old);
    assert_eq!(batch.suppressed, Some(SuppressionReason::StaleSnapshot));
    assert!(batch.active.is_empty());
    assert!(batch.events.iter().any(|event| matches!(
        event,
        AdviceEvent::Retracted {
            key: AdviceKey::IdleCitizens,
            reason: RetractionReason::DataQuality(SuppressionReason::StaleSnapshot)
        }
    )));
}
