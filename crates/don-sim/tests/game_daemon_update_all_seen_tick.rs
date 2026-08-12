//! Real-tick proof for the exact all-leaders-inactive and bounded active Build-plus-Unit
//! paths through `GameDaemon::update_all_seen` `0x00732840`.

use don_sim::objects::BUILD_BAND_BASE;
use don_sim::systems::save_load::{load_sim, save_sim};
use don_sim::systems::step12_visibility_runtime::{
    ActiveUnitProducerFault, RevealFogNoEffectBlocker, Step12VisibilityAuthority,
    Step12VisibilityPreflightError, VisibilityAuthorityFault, VisibilityConstants,
    VisibilityTypeProjection, UNIT_TYPE_BASE,
};
use don_sim::systems::{
    map_terrain::tflag,
    player_setup::ManualPlayerSetup,
    production::{
        self,
        runtime::{LiveBuildVisibilityTypeFacts, LiveProductionType},
        Footprint,
    },
};
use don_sim::tick::{Gap, Sim, StepRun};

fn advance_to_phase_33(sim: &mut Sim) {
    while sim.world.frame < 33 {
        sim.do_frame();
    }
    assert_eq!(sim.world.frame, 33);
}

fn seed_fog_planes(sim: &mut Sim) -> Vec<u8> {
    sim.map.world.seen.fill(0x51);
    sim.map.world.seen2.fill(0xa2);
    sim.map.world.seen3.fill(0xc4);
    sim.map.world.wcoord_seen.fill(0x38);
    sim.map.world.seen2.clone()
}

#[test]
fn inactive_phase33_clears_canonical_planes_and_roundtrips_the_resumed_frame() {
    let mut original = Sim::new(0x7328_4033, 8);
    advance_to_phase_33(&mut original);
    original.game_daemon.busy = 19;
    let seen2_before = seed_fog_planes(&mut original);
    let checksum_before = original.map.world.checksum();

    let trace = original.do_frame();

    assert_eq!(trace.steps[12], StepRun::Executed);
    assert_eq!(original.game_daemon.busy, 4);
    assert_eq!(original.cover.gaps[Gap::GameDaemonUpdateAllSeen.index()], 0);
    assert!(original.map.world.seen.iter().all(|&byte| byte == 0));
    assert!(original.map.world.seen3.iter().all(|&byte| byte == 0));
    assert!(original.map.world.wcoord_seen.iter().all(|&byte| byte == 0));
    assert_eq!(original.map.world.seen2, seen2_before);
    assert_ne!(original.map.world.checksum(), checksum_before);

    let saved = save_sim(&original).unwrap();
    let mut resumed = load_sim(&saved).unwrap();
    assert_eq!(resumed.map.world.checksum(), original.map.world.checksum());
    assert_eq!(save_sim(&resumed).unwrap(), saved);

    original.do_frame();
    resumed.do_frame();
    assert_eq!(resumed.channel_digest(), original.channel_digest());
    assert_eq!(resumed.map.world.seen, original.map.world.seen);
    assert_eq!(resumed.map.world.seen2, original.map.world.seen2);
    assert_eq!(resumed.map.world.seen3, original.map.world.seen3);
    assert_eq!(
        resumed.map.world.wcoord_seen,
        original.map.world.wcoord_seen
    );
    assert_eq!(save_sim(&resumed).unwrap(), save_sim(&original).unwrap());
}

#[test]
fn active_empty_leader_executes_the_exact_empty_unit_cohort() {
    let mut sim = Sim::new(0x7328_40ff, 8);
    sim.activate(0);
    sim.world.frame = 33;
    sim.game_daemon.busy = 19;
    seed_fog_planes(&mut sim);
    let seen2_before = sim.map.world.seen2.clone();

    let trace = sim.do_frame();

    assert_eq!(trace.steps[12], StepRun::Executed);
    assert_eq!(sim.game_daemon.busy, 4);
    assert_eq!(sim.cover.gaps[Gap::GameDaemonUpdateAllSeen.index()], 0);
    assert!(sim.map.world.seen.iter().all(|&byte| byte == 0));
    assert_eq!(sim.map.world.seen2, seen2_before);
    assert!(sim.map.world.seen3.iter().all(|&byte| byte == 0));
    assert!(sim.map.world.wcoord_seen.iter().all(|&byte| byte == 0));
    assert_eq!(sim.step12_visibility_error, None);
}

fn active_unit_phase33() -> Sim {
    let mut sim = Sim::new(0x7328_4133, 8);
    let mut setup = ManualPlayerSetup {
        active_mask: 1,
        local_player_setup_slot: 0,
        ..ManualPlayerSetup::default()
    };
    setup.teams[0] = 0;
    sim.start_manual_player_setup(setup).unwrap();
    advance_to_phase_33(&mut sim);

    sim.replace_step12_visibility_type_source(
        1,
        0x7328_4133,
        VisibilityConstants {
            ptolemy_los_bonus: 0,
            the_ceo_unit_los: 0,
        },
        vec![VisibilityTypeProjection {
            type_index: UNIT_TYPE_BASE,
            object_masks: 0,
            domain: 0,
            unit_flags2: 0,
            role: 0,
            is_siege: false,
            local_seen_radius: 2,
        }],
    )
    .unwrap();
    let unit = sim.spawn_unit(0, UNIT_TYPE_BASE, 6_000, 6_000, 6).unwrap();
    sim.materialize_step12_object_init(unit).unwrap();

    for cell in &mut sim.map.world.wdata {
        cell.who = 0;
    }
    for tile in &mut sim.map.world.tdata {
        *tile &= !tflag::RESOURCE;
    }
    sim
}

fn active_build_phase33(active: bool) -> Sim {
    let mut sim = Sim::new(0x7328_4233, 8);
    let mut setup = ManualPlayerSetup {
        active_mask: 1,
        local_player_setup_slot: 0,
        ..ManualPlayerSetup::default()
    };
    setup.teams[0] = 0;
    sim.start_manual_player_setup(setup).unwrap();
    advance_to_phase_33(&mut sim);

    let mut flags = production::flag::VALID | production::flag::STARTED | 0x40;
    if active {
        flags |= production::flag::ACTIVE;
    }
    let mut build = production::BuildData {
        flags,
        myhits: 640,
        construct_hits: 640,
        gather_down: -1,
        city: -1,
        city_down: -1,
        wonder: -1,
        dock: -1,
        attack_ox: -1,
        attack_whom: -1,
        ..production::BuildData::default()
    };
    build.other[production::off::OBJECT_ID..production::off::OBJECT_ID + 2]
        .copy_from_slice(&(BUILD_BAND_BASE as i16).to_le_bytes());
    build.other[0x28..0x2a].copy_from_slice(&(-1i16).to_le_bytes());
    build.other[production::off::X_INTERNAL..production::off::X_INTERNAL + 4]
        .copy_from_slice(&(6_000i32 ^ 0x63637).to_le_bytes());
    build.other[production::off::Y_INTERNAL..production::off::Y_INTERNAL + 4]
        .copy_from_slice(&(6_000i32 ^ 0x63637).to_le_bytes());
    build.other[production::off::MYLOS] = 6;
    sim.spawn_build(0, build);

    for tile in &mut sim.map.world.tdata {
        *tile &= !tflag::RESOURCE;
    }
    if active {
        install_build_visibility_type(&mut sim, 0x19e, None);
    }
    sim
}

fn install_build_visibility_type(
    sim: &mut Sim,
    type_index: i32,
    visibility: Option<LiveBuildVisibilityTypeFacts>,
) {
    let mut facts = LiveProductionType::in_place_building(type_index, 1);
    facts.build_visibility = visibility;
    sim.production_runtime.install_type(facts);
    sim.production_runtime.register_build(0, type_index);
}

#[test]
fn active_unit_phase33_stamps_canonical_planes_and_roundtrips_the_resumed_frame() {
    let mut original = active_unit_phase33();
    original.game_daemon.busy = 19;
    seed_fog_planes(&mut original);
    let checksum_before = original.map.world.checksum();

    let trace = original.do_frame();

    assert_eq!(trace.steps[12], StepRun::Executed);
    assert_eq!(original.game_daemon.busy, 4);
    assert_eq!(original.cover.gaps[Gap::GameDaemonUpdateAllSeen.index()], 0);
    assert_eq!(original.step12_visibility_error, None);
    assert!(original.map.world.seen.iter().any(|&byte| byte & 1 != 0));
    assert!(original.map.world.seen2.iter().any(|&byte| byte == 0xa3));
    assert!(original.map.world.seen3.iter().all(|&byte| byte == 0));
    assert!(original
        .map
        .world
        .wcoord_seen
        .iter()
        .any(|&byte| byte & 1 != 0));
    assert_ne!(original.map.world.checksum(), checksum_before);

    // This authority is an externally reinstallable rules projection and deliberately is
    // not serialized. The checksum-visible result above must survive independently.
    original.step12_visibility = Step12VisibilityAuthority::default();
    let saved = save_sim(&original).unwrap();
    let mut resumed = load_sim(&saved).unwrap();
    assert_eq!(resumed.map.world.checksum(), original.map.world.checksum());
    assert_eq!(save_sim(&resumed).unwrap(), saved);

    original.do_frame();
    resumed.do_frame();
    assert_eq!(resumed.channel_digest(), original.channel_digest());
    assert_eq!(save_sim(&resumed).unwrap(), save_sim(&original).unwrap());
}

#[test]
fn active_build_phase33_stamps_canonical_planes_and_roundtrips_the_resumed_frame() {
    let mut original = active_build_phase33(true);
    original.game_daemon.busy = 19;
    seed_fog_planes(&mut original);
    let checksum_before = original.map.world.checksum();

    let trace = original.do_frame();

    assert_eq!(trace.steps[12], StepRun::Executed);
    assert_eq!(original.game_daemon.busy, 4);
    assert_eq!(original.cover.gaps[Gap::GameDaemonUpdateAllSeen.index()], 0);
    assert_eq!(original.step12_visibility_error, None);
    assert!(original.map.world.seen.iter().any(|&byte| byte & 1 != 0));
    assert!(original.map.world.seen2.iter().any(|&byte| byte == 0xa3));
    assert!(
        original.map.world.seen3.iter().any(|&byte| byte & 1 != 0),
        "Build ObjectData flags bit 0x40 owns the detector stamp"
    );
    assert_ne!(original.map.world.checksum(), checksum_before);

    let saved = save_sim(&original).unwrap();
    let mut resumed = load_sim(&saved).unwrap();
    assert_eq!(resumed.map.world.checksum(), original.map.world.checksum());
    assert_eq!(save_sim(&resumed).unwrap(), saved);

    original.do_frame();
    resumed.do_frame();
    assert_eq!(resumed.channel_digest(), original.channel_digest());
    assert_eq!(save_sim(&resumed).unwrap(), save_sim(&original).unwrap());
}

#[test]
fn incomplete_build_without_current_type_refuses_before_daemon_or_plane_mutation() {
    let mut sim = active_build_phase33(false);
    sim.game_daemon.busy = 19;
    seed_fog_planes(&mut sim);
    let daemon_before = sim.game_daemon;
    let checksum_before = sim.map.world.checksum();

    let trace = sim.do_frame();

    assert_eq!(
        trace.steps[12],
        StepRun::Unimplemented(Gap::GameDaemonUpdateAllSeen)
    );
    assert_eq!(sim.game_daemon, daemon_before);
    assert_eq!(sim.map.world.checksum(), checksum_before);
    assert_eq!(
        sim.step12_visibility_error,
        Some(Step12VisibilityPreflightError::ActiveUnitCohort(
            ActiveUnitProducerFault::MissingBuildType {
                row: 0,
                who: 0,
                object_o: BUILD_BAND_BASE as i16,
            }
        ))
    );
}

#[test]
fn incomplete_nonwonder_build_takes_the_exact_no_visibility_branch() {
    let mut sim = active_build_phase33(false);
    install_build_visibility_type(&mut sim, 0x19e, None);
    sim.game_daemon.busy = 19;
    let seen2_before = seed_fog_planes(&mut sim);

    let trace = sim.do_frame();

    assert_eq!(trace.steps[12], StepRun::Executed);
    assert_eq!(sim.game_daemon.busy, 4);
    assert_eq!(sim.step12_visibility_error, None);
    assert!(sim.map.world.seen.iter().all(|&byte| byte == 0));
    assert_eq!(sim.map.world.seen2, seen2_before);
    assert!(sim.map.world.seen3.iter().all(|&byte| byte == 0));
    assert!(sim.map.world.wcoord_seen.iter().all(|&byte| byte == 0));
}

#[test]
fn unstarted_wonder_returns_before_footprint_authority() {
    let mut sim = active_build_phase33(false);
    sim.builds[0].flags &= !production::flag::STARTED;
    install_build_visibility_type(&mut sim, 0x21e, None);
    sim.game_daemon.busy = 19;
    let seen2_before = seed_fog_planes(&mut sim);

    let trace = sim.do_frame();

    assert_eq!(trace.steps[12], StepRun::Executed);
    assert_eq!(sim.step12_visibility_error, None);
    assert!(sim.map.world.seen.iter().all(|&byte| byte == 0));
    assert_eq!(sim.map.world.seen2, seen2_before);
    assert!(sim.map.world.seen3.iter().all(|&byte| byte == 0));
}

#[test]
fn started_wonder_stamps_its_exact_footprint_and_roundtrips_resumed_frame() {
    let mut original = active_build_phase33(false);
    install_build_visibility_type(
        &mut original,
        0x21e,
        Some(LiveBuildVisibilityTypeFacts {
            footprint: Some(Footprint {
                x_size: 6,
                y_size: 6,
            }),
            is_fort: Some(false),
        }),
    );
    original.builds[0].other[production::off::X_INTERNAL..production::off::X_INTERNAL + 4]
        .copy_from_slice(&(3_000i32 ^ 0x63637).to_le_bytes());
    original.builds[0].other[production::off::Y_INTERNAL..production::off::Y_INTERNAL + 4]
        .copy_from_slice(&(3_000i32 ^ 0x63637).to_le_bytes());
    original.game_daemon.busy = 19;
    seed_fog_planes(&mut original);
    let checksum_before = original.map.world.checksum();

    let trace = original.do_frame();

    assert_eq!(
        trace.steps[12],
        StepRun::Executed,
        "visibility error: {:?}",
        original.step12_visibility_error
    );
    assert_eq!(original.game_daemon.busy, 4);
    assert_eq!(original.step12_visibility_error, None);
    assert_eq!(
        original
            .map
            .world
            .seen
            .iter()
            .filter(|&&byte| byte == u8::MAX)
            .count(),
        25,
        "64 retail tile visits around the 6x6 footprint fold onto exactly 5x5 fog cells"
    );
    assert_eq!(
        original
            .map
            .world
            .seen2
            .iter()
            .filter(|&&byte| byte == u8::MAX)
            .count(),
        25
    );
    assert!(original.map.world.seen3.iter().all(|&byte| byte == 0));
    assert_ne!(original.map.world.checksum(), checksum_before);

    // Current type/footprint facts are reinstallable type authority. The canonical
    // World after-image and Build row must survive without serializing that projection.
    original.production_runtime = Default::default();
    let saved = save_sim(&original).unwrap();
    let mut resumed = load_sim(&saved).unwrap();
    assert_eq!(resumed.map.world.checksum(), original.map.world.checksum());
    assert_eq!(save_sim(&resumed).unwrap(), saved);

    original.do_frame();
    resumed.do_frame();
    assert_eq!(resumed.channel_digest(), original.channel_digest());
    assert_eq!(save_sim(&resumed).unwrap(), save_sim(&original).unwrap());
}

#[test]
fn captured_started_wonder_takes_the_explored_only_mask_branch() {
    let mut sim = active_build_phase33(false);
    install_build_visibility_type(
        &mut sim,
        0x21e,
        Some(LiveBuildVisibilityTypeFacts {
            footprint: Some(Footprint {
                x_size: 6,
                y_size: 6,
            }),
            is_fort: None,
        }),
    );
    sim.builds[0].flags |= production::flag::CAPTURED;
    sim.builds[0].ever_seen = 0b0000_0010;
    sim.builds[0].other[production::off::VISIBLE] = 0b0000_0100;
    sim.builds[0].other[production::off::X_INTERNAL..production::off::X_INTERNAL + 4]
        .copy_from_slice(&(3_000i32 ^ 0x63637).to_le_bytes());
    sim.builds[0].other[production::off::Y_INTERNAL..production::off::Y_INTERNAL + 4]
        .copy_from_slice(&(3_000i32 ^ 0x63637).to_le_bytes());
    sim.game_daemon.busy = 19;
    seed_fog_planes(&mut sim);
    sim.map.world.seen2.fill(0);

    let trace = sim.do_frame();

    assert_eq!(trace.steps[12], StepRun::Executed);
    assert_eq!(sim.step12_visibility_error, None);
    assert!(sim.map.world.seen.iter().all(|&byte| byte == 0));
    assert_eq!(
        sim.map
            .world
            .seen2
            .iter()
            .filter(|&&byte| byte == 0b0000_0111)
            .count(),
        25,
        "visible | ever_seen | owner mask is written to explored state"
    );
    assert!(sim.map.world.seen3.iter().all(|&byte| byte == 0));
    assert!(sim.map.world.wcoord_seen.iter().all(|&byte| byte == 0));
}

#[test]
fn started_wonder_without_footprint_facts_refuses_atomically() {
    let mut sim = active_build_phase33(false);
    install_build_visibility_type(
        &mut sim,
        0x21e,
        Some(LiveBuildVisibilityTypeFacts {
            footprint: None,
            is_fort: Some(false),
        }),
    );
    sim.game_daemon.busy = 19;
    seed_fog_planes(&mut sim);
    let daemon_before = sim.game_daemon;
    let checksum_before = sim.map.world.checksum();

    let trace = sim.do_frame();

    assert_eq!(
        trace.steps[12],
        StepRun::Unimplemented(Gap::GameDaemonUpdateAllSeen)
    );
    assert_eq!(sim.game_daemon, daemon_before);
    assert_eq!(sim.map.world.checksum(), checksum_before);
    assert_eq!(
        sim.step12_visibility_error,
        Some(Step12VisibilityPreflightError::ActiveUnitCohort(
            ActiveUnitProducerFault::BuildLocalSeenNeedsFootprintAuthority {
                row: 0,
                type_index: 0x21e,
            }
        ))
    );
}

#[test]
fn uncaptured_started_wonder_without_fort_authority_refuses_atomically() {
    let mut sim = active_build_phase33(false);
    install_build_visibility_type(
        &mut sim,
        0x21e,
        Some(LiveBuildVisibilityTypeFacts {
            footprint: Some(Footprint {
                x_size: 6,
                y_size: 6,
            }),
            is_fort: None,
        }),
    );
    sim.builds[0].other[production::off::X_INTERNAL..production::off::X_INTERNAL + 4]
        .copy_from_slice(&(3_000i32 ^ 0x63637).to_le_bytes());
    sim.builds[0].other[production::off::Y_INTERNAL..production::off::Y_INTERNAL + 4]
        .copy_from_slice(&(3_000i32 ^ 0x63637).to_le_bytes());
    sim.game_daemon.busy = 19;
    seed_fog_planes(&mut sim);
    let daemon_before = sim.game_daemon;
    let checksum_before = sim.map.world.checksum();

    let trace = sim.do_frame();

    assert_eq!(
        trace.steps[12],
        StepRun::Unimplemented(Gap::GameDaemonUpdateAllSeen)
    );
    assert_eq!(sim.game_daemon, daemon_before);
    assert_eq!(sim.map.world.checksum(), checksum_before);
    assert_eq!(
        sim.step12_visibility_error,
        Some(Step12VisibilityPreflightError::ActiveUnitCohort(
            ActiveUnitProducerFault::BuildWonderNeedsFortAuthority {
                row: 0,
                type_index: 0x21e,
            }
        ))
    );
}

#[test]
fn malformed_started_wonder_footprint_refuses_atomically() {
    let mut sim = active_build_phase33(false);
    install_build_visibility_type(
        &mut sim,
        0x21e,
        Some(LiveBuildVisibilityTypeFacts {
            footprint: Some(Footprint {
                x_size: i32::MAX,
                y_size: 2,
            }),
            is_fort: Some(false),
        }),
    );
    sim.game_daemon.busy = 19;
    seed_fog_planes(&mut sim);
    let daemon_before = sim.game_daemon;
    let checksum_before = sim.map.world.checksum();

    let trace = sim.do_frame();

    assert_eq!(
        trace.steps[12],
        StepRun::Unimplemented(Gap::GameDaemonUpdateAllSeen)
    );
    assert_eq!(sim.game_daemon, daemon_before);
    assert_eq!(sim.map.world.checksum(), checksum_before);
    assert_eq!(
        sim.step12_visibility_error,
        Some(Step12VisibilityPreflightError::ActiveUnitCohort(
            ActiveUnitProducerFault::InvalidBuildLocalSeenFootprint {
                row: 0,
                x_size: i32::MAX,
                y_size: 2,
            }
        ))
    );
}

#[test]
fn active_build_visible_local_seen_executes_before_its_ordinary_stamp() {
    let mut original = active_build_phase33(true);
    install_build_visibility_type(
        &mut original,
        0x19e,
        Some(LiveBuildVisibilityTypeFacts {
            footprint: Some(Footprint {
                x_size: 4,
                y_size: 4,
            }),
            is_fort: None,
        }),
    );
    original.builds[0].ever_seen = 0b0000_0010;
    original.builds[0].other[production::off::VISIBLE] = 0b0000_0100;
    original.builds[0].other[production::off::X_INTERNAL..production::off::X_INTERNAL + 4]
        .copy_from_slice(&(3_000i32 ^ 0x63637).to_le_bytes());
    original.builds[0].other[production::off::Y_INTERNAL..production::off::Y_INTERNAL + 4]
        .copy_from_slice(&(3_000i32 ^ 0x63637).to_le_bytes());
    original.game_daemon.busy = 19;
    seed_fog_planes(&mut original);
    original.map.world.seen2.fill(0);
    let checksum_before = original.map.world.checksum();

    let trace = original.do_frame();

    assert_eq!(trace.steps[12], StepRun::Executed);
    assert_eq!(original.step12_visibility_error, None);
    assert!(original
        .map
        .world
        .seen2
        .iter()
        .any(|&byte| byte == 0b0000_0111));
    assert!(original.map.world.seen.iter().all(|&byte| byte & 4 == 0));
    assert!(original
        .map
        .world
        .wcoord_seen
        .iter()
        .all(|&byte| byte & 4 == 0));
    assert_ne!(original.map.world.checksum(), checksum_before);

    original.production_runtime = Default::default();
    let saved = save_sim(&original).unwrap();
    let mut resumed = load_sim(&saved).unwrap();
    assert_eq!(resumed.map.world.checksum(), original.map.world.checksum());
    original.do_frame();
    resumed.do_frame();
    assert_eq!(resumed.channel_digest(), original.channel_digest());
    assert_eq!(save_sim(&resumed).unwrap(), save_sim(&original).unwrap());
}

#[test]
fn active_complete_wonder_implicitly_runs_local_seen_with_zero_visible_byte() {
    let mut sim = active_build_phase33(true);
    install_build_visibility_type(
        &mut sim,
        0x21e,
        Some(LiveBuildVisibilityTypeFacts {
            footprint: Some(Footprint {
                x_size: 6,
                y_size: 6,
            }),
            is_fort: Some(false),
        }),
    );
    sim.builds[0].other[production::off::X_INTERNAL..production::off::X_INTERNAL + 4]
        .copy_from_slice(&(3_000i32 ^ 0x63637).to_le_bytes());
    sim.builds[0].other[production::off::Y_INTERNAL..production::off::Y_INTERNAL + 4]
        .copy_from_slice(&(3_000i32 ^ 0x63637).to_le_bytes());
    assert_eq!(sim.builds[0].other[production::off::VISIBLE], 0);
    sim.game_daemon.busy = 19;
    seed_fog_planes(&mut sim);
    sim.map.world.seen2.fill(0);

    let trace = sim.do_frame();

    assert_eq!(trace.steps[12], StepRun::Executed);
    assert_eq!(sim.step12_visibility_error, None);
    assert_eq!(
        sim.map
            .world
            .seen
            .iter()
            .filter(|&&byte| byte == u8::MAX)
            .count(),
        25
    );
    assert_eq!(
        sim.map
            .world
            .seen2
            .iter()
            .filter(|&&byte| byte == u8::MAX)
            .count(),
        25
    );
}

#[test]
fn active_build_missing_current_type_refuses_before_los_or_plane_mutation() {
    let mut sim = active_build_phase33(true);
    sim.production_runtime = Default::default();
    sim.builds[0].other[production::off::MYLOS] = 0;
    sim.builds[0].other[production::off::VISIBLE] = 1;
    sim.game_daemon.busy = 19;
    seed_fog_planes(&mut sim);
    let daemon_before = sim.game_daemon;
    let checksum_before = sim.map.world.checksum();

    let trace = sim.do_frame();

    assert_eq!(
        trace.steps[12],
        StepRun::Unimplemented(Gap::GameDaemonUpdateAllSeen)
    );
    assert_eq!(sim.game_daemon, daemon_before);
    assert_eq!(sim.map.world.checksum(), checksum_before);
    assert_eq!(
        sim.step12_visibility_error,
        Some(Step12VisibilityPreflightError::ActiveUnitCohort(
            ActiveUnitProducerFault::MissingBuildType {
                row: 0,
                who: 0,
                object_o: BUILD_BAND_BASE as i16,
            }
        ))
    );
}

#[test]
fn zero_los_build_returns_before_visible_is_read() {
    let mut sim = active_build_phase33(true);
    sim.game_daemon.busy = 19;
    let seen2_before = seed_fog_planes(&mut sim);
    sim.builds[0].other[production::off::MYLOS] = 0;
    sim.builds[0].other[production::off::VISIBLE] = 1;

    let trace = sim.do_frame();

    assert_eq!(trace.steps[12], StepRun::Executed);
    assert_eq!(sim.game_daemon.busy, 4);
    assert_eq!(sim.step12_visibility_error, None);
    assert!(sim.map.world.seen.iter().all(|&byte| byte == 0));
    assert_eq!(sim.map.world.seen2, seen2_before);
    assert!(sim.map.world.seen3.iter().all(|&byte| byte == 0));
}

#[test]
fn effectful_reveal_fog_refuses_before_daemon_or_plane_mutation() {
    let mut sim = active_unit_phase33();
    sim.game_daemon.busy = 19;
    seed_fog_planes(&mut sim);
    sim.map.world.tdata.fill(tflag::RESOURCE);
    let daemon_before = sim.game_daemon;
    let seen_before = sim.map.world.seen.clone();
    let seen2_before = sim.map.world.seen2.clone();
    let seen3_before = sim.map.world.seen3.clone();
    let wcoord_seen_before = sim.map.world.wcoord_seen.clone();

    let trace = sim.do_frame();

    assert_eq!(
        trace.steps[12],
        StepRun::Unimplemented(Gap::GameDaemonUpdateAllSeen)
    );
    assert_eq!(sim.game_daemon, daemon_before);
    assert_eq!(sim.map.world.seen, seen_before);
    assert_eq!(sim.map.world.seen2, seen2_before);
    assert_eq!(sim.map.world.seen3, seen3_before);
    assert_eq!(sim.map.world.wcoord_seen, wcoord_seen_before);
    assert!(matches!(
        sim.step12_visibility_error,
        Some(Step12VisibilityPreflightError::ActiveUnitCohort(
            ActiveUnitProducerFault::RevealFogEffectful {
                blocker: RevealFogNoEffectBlocker::ResourceTile,
                ..
            }
        ))
    ));
}

#[test]
fn visible_unit_executes_exact_local_seen_disc_before_ordinary_stamp_and_roundtrips() {
    let mut original = active_unit_phase33();
    original.game_daemon.busy = 19;
    seed_fog_planes(&mut original);
    original.map.world.seen2.fill(0);
    original.world.units.visible_mut()[0] = 0b0000_0100;
    let checksum_before = original.map.world.checksum();

    let trace = original.do_frame();

    assert_eq!(trace.steps[12], StepRun::Executed);
    assert_eq!(original.step12_visibility_error, None);
    assert!(original.map.world.seen.iter().any(|&byte| byte & 4 != 0));
    assert!(original.map.world.seen2.iter().any(|&byte| byte & 4 != 0));
    assert!(original
        .map
        .world
        .wcoord_seen
        .iter()
        .any(|&byte| byte & 4 != 0));
    assert!(original.map.world.seen3.iter().all(|&byte| byte & 4 == 0));
    assert!(original
        .map
        .world
        .wdata
        .iter()
        .any(|cell| cell.was_seen & 4 != 0));
    assert_ne!(original.map.world.checksum(), checksum_before);

    original.step12_visibility = Step12VisibilityAuthority::default();
    let saved = save_sim(&original).unwrap();
    let mut resumed = load_sim(&saved).unwrap();
    assert_eq!(resumed.map.world.checksum(), original.map.world.checksum());
    original.do_frame();
    resumed.do_frame();
    assert_eq!(resumed.channel_digest(), original.channel_digest());
    assert_eq!(save_sim(&resumed).unwrap(), save_sim(&original).unwrap());
}

#[test]
fn visible_unit_with_out_of_range_type_radius_refuses_atomically() {
    let mut sim = active_unit_phase33();
    sim.replace_step12_visibility_type_source(
        2,
        0x7328_4134,
        VisibilityConstants {
            ptolemy_los_bonus: 0,
            the_ceo_unit_los: 0,
        },
        vec![VisibilityTypeProjection {
            type_index: UNIT_TYPE_BASE,
            object_masks: 0,
            domain: 0,
            unit_flags2: 0,
            role: 0,
            is_siege: false,
            local_seen_radius: 65,
        }],
    )
    .unwrap();
    sim.world.units.visible_mut()[0] = 1;
    sim.game_daemon.busy = 19;
    seed_fog_planes(&mut sim);
    let daemon_before = sim.game_daemon;
    let checksum_before = sim.map.world.checksum();

    let trace = sim.do_frame();

    assert_eq!(
        trace.steps[12],
        StepRun::Unimplemented(Gap::GameDaemonUpdateAllSeen)
    );
    assert_eq!(sim.game_daemon, daemon_before);
    assert_eq!(sim.map.world.checksum(), checksum_before);
    assert!(matches!(
        sim.step12_visibility_error,
        Some(Step12VisibilityPreflightError::Authority(
            VisibilityAuthorityFault::Frontier(
                don_sim::systems::step12_visibility_producer_frontier::LiveStep12PrepareFault::InvalidLocalSeenRadius {
                    row: 0,
                    radius: 65,
                }
            )
        ))
    ));
}

#[test]
fn clear_item_flag_returns_before_source_auto_explore_mask_is_read() {
    let mut sim = active_unit_phase33();
    sim.game_daemon.busy = 19;
    seed_fog_planes(&mut sim);
    sim.world.units.set_unit_masks(0, 0x100);

    let trace = sim.do_frame();

    assert_eq!(trace.steps[12], StepRun::Executed);
    assert_eq!(sim.game_daemon.busy, 4);
    assert_eq!(sim.step12_visibility_error, None);
    assert!(sim.map.world.seen.iter().any(|&byte| byte & 1 != 0));
}
