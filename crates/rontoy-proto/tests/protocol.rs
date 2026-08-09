use rontoy_proto::*;

fn evidence(confidence_bps: u16) -> Evidence {
    Evidence {
        source_id: 0,
        confidence_bps,
        calibration_id: "synthetic-fixture-v1".into(),
        freshness_ms: 9,
        flags: 0,
        unknown: vec![],
    }
}

fn sample_frame() -> Frame {
    let source = Source {
        kind: SourceKind::ProcessMemory,
        source_key: "econ-block-v1".into(),
        build_id: "rise.exe:test-fixture".into(),
        process_id: Some(4242),
        process_start_id: Some(1_785_000_000_000),
        image_base: Some(0x400000),
        capture_id: Some(77),
        detail: "engine display income; native 1/16 resource per 30 seconds; unbonused".into(),
        unknown: vec![],
    };
    let food = ResourceBalance {
        kind: ResourceKind::Food,
        stockpile_raw: Some(123),
        income_sixteenths_per_period: Some(672),
        income_basis: RateBasis::EngineDirect,
        gross_sixteenths_per_period: Some(700),
        support_sixteenths_per_period: Some(28),
        bonus_sixteenths_per_period: Some(0),
        leftover_sixteenths_per_period: Some(0),
        resource_cap_raw: Some(250),
        over_cap_status: Some(0),
        spend_sixteenths_per_period: None,
        reserved_raw: Some(50),
        evidence: evidence(9_950),
        unknown: vec![UnknownField {
            tag: 91,
            wire_type: 0,
            flags: 2,
            data: vec![9, 8, 7],
        }],
    };
    let player = PlayerEconomy {
        player_id: 1,
        name: "Ember".into(),
        nation_type_id: Some(12),
        age: Some(2),
        resources: vec![food],
        population: Population {
            used: 48,
            cap: 75,
            queued: 2,
            citizens: Some(31),
            military: Some(17),
            unknown: vec![],
        },
        workers: Workforce {
            gathering: 26,
            idle: 1,
            building: 3,
            scouting: 1,
            unknown: vec![],
        },
        gather_stamp: Some(12_390),
        gather_cache_age_frames: Some(10),
        city_count: Some(2),
        num_units: Some(48),
        num_buildings: Some(12),
        queued_type_counts: vec![TypeCount {
            type_id: 33,
            count: 1,
            valid: true,
            unknown: vec![],
        }],
        evidence: evidence(9_900),
        unknown: vec![],
    };
    let entity = Entity {
        object_id: 0xdead_beef,
        type_id: 101,
        owner_id: Some(1),
        kind: EntityKind::Building,
        x_fine: -12_500,
        y_fine: 44_125,
        hp_milli: Some(90_000),
        hp_max_milli: Some(100_000),
        build_progress_ppm: Some(1_000_000),
        state_flags: 4,
        queues: vec![ProductionQueue {
            queue_id: 0,
            capacity: Some(7),
            items: vec![QueueItem {
                type_id: 33,
                count: 1,
                progress_ppm: Some(500_000),
                eta_ms: Some(12_000),
                paused: false,
                unknown: vec![],
            }],
            unknown: vec![],
        }],
        evidence: evidence(9_800),
        visibility: VisibilityBasis::OwnOrNeutral,
        unknown: vec![],
    };
    let warning = Warning {
        warning_id: 8,
        code: "econ.population.blocked".into(),
        severity: Severity::Caution,
        headline: "Population cap soon".into(),
        detail: "Queued citizens will fill remaining capacity.".into(),
        player_id: Some(1),
        object_id: Some(0xdead_beef),
        expires_game_frame: Some(12_450),
        evidence: evidence(9_000),
        unknown: vec![],
    };
    let advice = Advice {
        advice_id: 9,
        rule_id: "econ.population.headroom".into(),
        rule_version: "1.0.0".into(),
        lifecycle: AdviceLifecycle::Active,
        priority_bps: 8_500,
        headline: "Build a house".into(),
        rationale: "Two queued citizens consume the remaining cap.".into(),
        actions: vec!["Select a citizen and place a house now.".into()],
        related_warning_ids: vec![8],
        valid_until_game_frame: Some(12_430),
        evidence: evidence(8_800),
        unknown: vec![],
    };
    Frame::new(
        [0x42; 16],
        19,
        1_785_000_000_123,
        Message::Snapshot(Snapshot {
            meta: SnapshotMeta {
                observed_unix_ms: 1_785_000_000_100,
                observed_monotonic_ns: 55_123,
                game_frame: Some(12_400),
                sampled_frame_start: Some(12_400),
                sampled_frame_end: Some(12_400),
                sim_time_ms: Some(830_800),
                world_revision: Some(7),
                coherence: Coherence::Coherent,
                partial: false,
                dropped_since_previous: 0,
                capture_duration_us: 410,
                retry_count: 0,
                capability_bits: 3,
                component_validity: 3,
                read_calls: 7,
                bytes_read: 4096,
                short_reads: 0,
                decode_errors: 0,
                game_mode: GameMode::SinglePlayer,
                unknown: vec![],
            },
            sources: vec![source],
            players: vec![player],
            entities: vec![entity],
            warnings: vec![warning],
            advice: vec![advice],
            scope: ObservationScope::OwnPlayerOnly,
            local_human: Some(HumanIdentity {
                player_id: 1,
                confirmed_local: true,
                active: true,
                identity_key: "leader-slot-and-local-control".into(),
                unknown: vec![],
            }),
            unknown: vec![UnknownField {
                tag: 60000,
                wire_type: 3,
                flags: 1,
                data: b"future".to_vec(),
            }],
        }),
    )
}

#[test]
fn complete_snapshot_round_trips_and_validates() {
    let frame = sample_frame();
    frame.validate(ValidationLimits::default()).unwrap();
    let encoded = encode_frame(&frame, WireLimits::default()).unwrap();
    assert_eq!(&encoded[..4], b"DONF");
    assert_eq!(
        decode_frame(&encoded, WireLimits::default()).unwrap(),
        frame
    );
}

#[test]
fn future_fields_survive_a_decode_encode_relay() {
    let frame = sample_frame();
    let encoded = encode_frame(&frame, WireLimits::default()).unwrap();
    let decoded = decode_frame(&encoded, WireLimits::default()).unwrap();
    let relayed = encode_frame(&decoded, WireLimits::default()).unwrap();
    assert_eq!(relayed, encoded);
}

#[test]
fn future_message_kind_is_lossless() {
    let frame = Frame::new(
        [3; 16],
        7,
        99,
        Message::Unknown {
            kind: 900,
            payload: vec![0, 1, 2, 255],
        },
    );
    let bytes = encode_frame(&frame, WireLimits::default()).unwrap();
    assert_eq!(decode_frame(&bytes, WireLimits::default()).unwrap(), frame);
}

#[test]
fn framing_rejects_truncation_trailing_data_and_limits() {
    let bytes = encode_frame(&sample_frame(), WireLimits::default()).unwrap();
    assert!(matches!(
        decode_frame(&bytes[..bytes.len() - 1], WireLimits::default()),
        Err(DecodeError::Truncated("payload"))
    ));
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(matches!(
        decode_frame(&trailing, WireLimits::default()),
        Err(DecodeError::TrailingBytes { .. })
    ));
    let tight = WireLimits {
        max_frame_bytes: bytes.len() - 1,
        ..WireLimits::default()
    };
    assert!(matches!(
        decode_frame(&bytes, tight),
        Err(DecodeError::FrameTooLarge { .. })
    ));
}

#[test]
fn advice_requires_a_coherent_complete_snapshot() {
    let mut frame = sample_frame();
    let Message::Snapshot(snapshot) = &mut frame.message else {
        unreachable!()
    };
    snapshot.meta.sampled_frame_end = Some(12_401);
    let error = frame.validate(ValidationLimits::default()).unwrap_err();
    assert_eq!(error.path, "snapshot.meta.coherence");

    let Message::Snapshot(snapshot) = &mut frame.message else {
        unreachable!()
    };
    snapshot.meta.coherence = Coherence::IncoherentDropped;
    let error = frame.validate(ValidationLimits::default()).unwrap_err();
    assert_eq!(error.path, "snapshot.advice");
}

#[test]
fn advice_is_single_player_local_human_own_scope_only() {
    let mut multiplayer = sample_frame();
    let Message::Snapshot(snapshot) = &mut multiplayer.message else {
        unreachable!()
    };
    snapshot.meta.game_mode = GameMode::Multiplayer;
    assert_eq!(
        multiplayer
            .validate(ValidationLimits::default())
            .unwrap_err()
            .path,
        "snapshot.advice"
    );

    let mut opponent = sample_frame();
    let Message::Snapshot(snapshot) = &mut opponent.message else {
        unreachable!()
    };
    let mut other = snapshot.players[0].clone();
    other.player_id = 2;
    snapshot.players.push(other);
    assert_eq!(
        opponent
            .validate(ValidationLimits::default())
            .unwrap_err()
            .path,
        "snapshot.advice"
    );

    let mut unconfirmed = sample_frame();
    let Message::Snapshot(snapshot) = &mut unconfirmed.message else {
        unreachable!()
    };
    snapshot.local_human.as_mut().unwrap().confirmed_local = false;
    assert_eq!(
        unconfirmed
            .validate(ValidationLimits::default())
            .unwrap_err()
            .path,
        "snapshot.advice"
    );

    let mut source_without_identity = sample_frame();
    let Message::Snapshot(snapshot) = &mut source_without_identity.message else {
        unreachable!()
    };
    snapshot.sources[0].process_start_id = None;
    assert_eq!(
        source_without_identity
            .validate(ValidationLimits::default())
            .unwrap_err()
            .path,
        "snapshot.advice"
    );
}

#[test]
fn advice_rejects_paused_mismatched_or_unhealthy_capture() {
    let corruptors: [fn(&mut SnapshotMeta); 7] = [
        |meta| meta.coherence = Coherence::Paused,
        |meta| meta.game_frame = Some(meta.game_frame.unwrap() + 1),
        |meta| meta.component_validity = 0,
        |meta| meta.read_calls = 0,
        |meta| meta.bytes_read = 0,
        |meta| meta.short_reads = 1,
        |meta| meta.decode_errors = 1,
    ];
    for corrupt in corruptors {
        let mut frame = sample_frame();
        let Message::Snapshot(snapshot) = &mut frame.message else {
            unreachable!()
        };
        corrupt(&mut snapshot.meta);
        assert!(!snapshot.advice_allowed());
        assert_eq!(
            frame
                .validate(ValidationLimits::default())
                .unwrap_err()
                .path,
            "snapshot.advice"
        );
    }
}

#[test]
fn retail_status_and_gather_cache_consistency_are_validated() {
    let mut bad_status = sample_frame();
    let Message::Snapshot(snapshot) = &mut bad_status.message else {
        unreachable!()
    };
    snapshot.players[0].resources[0].over_cap_status = Some(3);
    assert!(bad_status
        .validate(ValidationLimits::default())
        .unwrap_err()
        .path
        .ends_with("over_cap_status"));

    let mut bad_age = sample_frame();
    let Message::Snapshot(snapshot) = &mut bad_age.message else {
        unreachable!()
    };
    snapshot.players[0].gather_cache_age_frames = Some(11);
    assert!(bad_age
        .validate(ValidationLimits::default())
        .unwrap_err()
        .path
        .ends_with("gather_cache_age_frames"));
}

#[test]
fn rate_basis_and_source_reference_are_enforced() {
    let mut frame = sample_frame();
    let Message::Snapshot(snapshot) = &mut frame.message else {
        unreachable!()
    };
    snapshot.players[0].resources[0].income_basis = RateBasis::Unknown;
    let error = frame.validate(ValidationLimits::default()).unwrap_err();
    assert!(error.path.ends_with("income_basis"));

    let Message::Snapshot(snapshot) = &mut frame.message else {
        unreachable!()
    };
    snapshot.players[0].resources[0].income_basis = RateBasis::EngineDirect;
    snapshot.players[0].evidence.source_id = 1;
    let error = frame.validate(ValidationLimits::default()).unwrap_err();
    assert!(error.path.ends_with("source_id"));
}

#[test]
fn semantic_collection_and_unknown_limits_are_enforced() {
    let mut frame = sample_frame();
    let Message::Snapshot(snapshot) = &mut frame.message else {
        unreachable!()
    };
    snapshot.entities.push(snapshot.entities[0].clone());
    let limits = ValidationLimits {
        max_entities: 1,
        ..ValidationLimits::default()
    };
    assert_eq!(
        frame.validate(limits).unwrap_err().path,
        "snapshot.entities"
    );

    let Message::Snapshot(snapshot) = &mut frame.message else {
        unreachable!()
    };
    snapshot.entities.pop();
    let limits = ValidationLimits {
        max_unknown_bytes_per_record: 2,
        ..ValidationLimits::default()
    };
    assert!(frame.validate(limits).is_err());
}

#[test]
fn retail_resource_order_is_load_bearing() {
    let kinds = [
        ResourceKind::Food,
        ResourceKind::Timber,
        ResourceKind::Wealth,
        ResourceKind::Knowledge,
        ResourceKind::Metal,
        ResourceKind::Oil,
    ];
    assert_eq!(kinds.map(|kind| kind.retail_index()), [0, 1, 2, 3, 4, 5]);
}

#[test]
fn custom_resource_cannot_alias_a_retail_index() {
    let mut frame = sample_frame();
    let Message::Snapshot(snapshot) = &mut frame.message else {
        unreachable!()
    };
    snapshot.players[0].resources[0].kind = ResourceKind::Custom(0);
    assert!(frame
        .validate(ValidationLimits::default())
        .unwrap_err()
        .message
        .contains("collide"));
}

#[test]
fn unsupported_major_and_corrupt_payload_are_rejected_during_decode() {
    let mut major = encode_frame(&sample_frame(), WireLimits::default()).unwrap();
    major[4..6].copy_from_slice(&2u16.to_le_bytes());
    assert!(matches!(
        decode_frame(&major, WireLimits::default()),
        Err(DecodeError::UnsupportedMajor { .. })
    ));

    let mut corrupt = encode_frame(&sample_frame(), WireLimits::default()).unwrap();
    *corrupt.last_mut().unwrap() ^= 0x80;
    assert!(matches!(
        decode_frame(&corrupt, WireLimits::default()),
        Err(DecodeError::IntegrityMismatch { .. })
    ));

    let mut corrupt_header = encode_frame(&sample_frame(), WireLimits::default()).unwrap();
    corrupt_header[16] ^= 1;
    assert!(matches!(
        decode_frame(&corrupt_header, WireLimits::default()),
        Err(DecodeError::IntegrityMismatch { .. })
    ));

    let mut reserved = encode_frame(&sample_frame(), WireLimits::default()).unwrap();
    reserved[10] = 1;
    assert_eq!(
        decode_frame(&reserved, WireLimits::default()).unwrap_err(),
        DecodeError::ReservedHeader { value: 1 }
    );
}

#[test]
fn duplicate_known_scalar_is_rejected() {
    let hello = Hello {
        producer: "probe".into(),
        producer_version: "1".into(),
        capabilities: vec![],
        unknown: vec![UnknownField {
            tag: 1,
            wire_type: 3,
            flags: 0,
            data: b"duplicate".to_vec(),
        }],
    };
    let frame = Frame::new([1; 16], 1, 1, Message::Hello(hello));
    let bytes = encode_frame(&frame, WireLimits::default()).unwrap();
    assert!(matches!(
        decode_frame(&bytes, WireLimits::default()),
        Err(DecodeError::DuplicateSingularField { tag: 1 })
    ));
}

#[test]
fn heartbeat_golden_vector_is_stable() {
    let hex = include_str!("golden/heartbeat-v1.hex").trim();
    let expected: Vec<u8> = hex
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair).unwrap();
            u8::from_str_radix(text, 16).unwrap()
        })
        .collect();
    let frame = Frame::new(
        [0x11; 16],
        1,
        2,
        Message::Heartbeat(Heartbeat {
            last_snapshot_sequence: Some(0),
            producer_state: "attached".into(),
            unknown: vec![],
        }),
    );
    assert_eq!(
        encode_frame(&frame, WireLimits::default()).unwrap(),
        expected
    );
    assert_eq!(
        decode_frame(&expected, WireLimits::default()).unwrap(),
        frame
    );
}
