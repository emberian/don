use don_sim::objects::BUILD_BAND_BASE;
use don_sim::systems::{
    game_daemon_calc_danger::{region_of, COORD_XOR, STRENGTH_FIXED},
    leaders::{StatObject, WallConstructTimeInputs, WallHitInputs},
    player_setup::ManualPlayerSetup,
    production::{self, BuildData},
    save_load::{load_sim, save_sim},
};
use don_sim::tick::{Gap, Sim, StepRun};

fn active_build_sim(with_tower_answer: bool) -> (Sim, i32, i32) {
    let mut sim = Sim::new(0x732d_1020, 16);
    sim.map.world.seed = 0x732d_1020;
    let mut setup = ManualPlayerSetup {
        active_mask: 1,
        local_player_setup_slot: 0,
        ..ManualPlayerSetup::default()
    };
    setup.teams[0] = 0;
    sim.start_manual_player_setup(setup).unwrap();

    let x = 2 * 1536 + 111;
    let y = 3 * 1536 + 222;
    let mut build = BuildData {
        flags: production::flag::VALID | production::flag::STARTED | production::flag::ACTIVE,
        myhits: 240,
        damage: 40,
        city: 0,
        gather_down: -1,
        city_down: -1,
        wonder: -1,
        dock: -1,
        attack_ox: -1,
        attack_whom: -1,
        ..BuildData::default()
    };
    build.other[production::off::OBJECT_ID..production::off::OBJECT_ID + 2]
        .copy_from_slice(&(BUILD_BAND_BASE as i16).to_le_bytes());
    build.other[0x28..0x2a].copy_from_slice(&(-1i16).to_le_bytes());
    build.other[production::off::X_INTERNAL..production::off::X_INTERNAL + 4]
        .copy_from_slice(&(x ^ COORD_XOR).to_le_bytes());
    build.other[production::off::Y_INTERNAL..production::off::Y_INTERNAL + 4]
        .copy_from_slice(&(y ^ COORD_XOR).to_le_bytes());
    sim.spawn_build(0, build);

    // These are exact type-query packages already owned by step 8's canonical object view.
    // `sync_step8_inputs` fills the object fields while retaining the installed answers.
    sim.step8_env.leaders[0].objects.band_2000.push(StatObject {
        wall_construct_time_inputs: Some(WallConstructTimeInputs {
            is_fort: false,
            ..WallConstructTimeInputs::default()
        }),
        wall_hit_inputs: with_tower_answer.then_some(WallHitInputs {
            is_tower_1b7: true,
            ..WallHitInputs::default()
        }),
        ..StatObject::default()
    });
    (sim, x, y)
}

fn remove_reinstallable_type_answers(sim: &mut Sim) {
    // The bounded DoNSave build owner does not yet carry a live City attachment. The danger
    // value has already committed; detach this deliberately synthetic test-only admission fact
    // before exercising the independent World-channel roundtrip.
    sim.builds[0].city = -1;
    let build = &sim.builds[0];
    sim.step8_env.leaders[0].objects.band_2000[0] = StatObject {
        active: build.is_valid(),
        wall_active: build.is_active(),
        wall_started: build.flags & production::flag::STARTED != 0,
        wall_city_flag: build.flags & production::flag::CAPTURED != 0,
        owner_in_game: true,
        myhits: build.myhits,
        mylos: build.other[0x3c] as i8,
        job_counter: build.job_counter,
        constr_time: build.constr_time,
        construct_hits: build.construct_hits,
        damage: build.damage,
        inside_down: i16::from_le_bytes([build.other[0x28], build.other[0x29]]),
        ..StatObject::default()
    };
}

#[test]
fn scheduled_tick_writes_canonical_danger_and_roundtrips_the_resumed_frame() {
    let (mut original, x, y) = active_build_sim(true);
    original.map.world.danger[0].fill(77);
    let checksum_before = original.map.world.checksum();

    let frame = original.do_frame();

    assert_eq!(frame.steps[12], StepRun::Executed);
    assert_eq!(original.cover.gaps[Gap::GameDaemonCalcDanger.index()], 0);
    assert_ne!(original.map.world.checksum(), checksum_before);
    let rx = region_of(x ^ COORD_XOR);
    let ry = region_of(y ^ COORD_XOR);
    let centre = (ry * original.map.world.reg_xs + rx) as usize;
    assert_eq!(original.map.world.danger[0][centre], -STRENGTH_FIXED);
    for (dx, dy) in [
        (-1, -1),
        (0, -1),
        (1, -1),
        (1, 0),
        (1, 1),
        (0, 1),
        (-1, 1),
        (-1, 0),
    ] {
        let cell = ((ry + dy) * original.map.world.reg_xs + rx + dx) as usize;
        assert_eq!(original.map.world.danger[0][cell], -(STRENGTH_FIXED / 2));
    }

    // Step-8 query packages are reinstallable adapters, not save state. Removing them leaves
    // exactly the canonical post-sync mirror admitted by DoNSave; danger itself remains in the
    // checksum-owned World chunk.
    remove_reinstallable_type_answers(&mut original);
    let saved = save_sim(&original).unwrap();
    let mut resumed = load_sim(&saved).unwrap();
    assert_eq!(resumed.map.world.checksum(), original.map.world.checksum());
    assert_eq!(save_sim(&resumed).unwrap(), saved);

    original.do_frame();
    resumed.do_frame();
    assert_eq!(resumed.channel_digest(), original.channel_digest());
    assert_eq!(resumed.map.world.danger, original.map.world.danger);
    assert_eq!(save_sim(&resumed).unwrap(), save_sim(&original).unwrap());
}

#[test]
fn missing_reached_tower_answer_refuses_before_the_plane_clear() {
    let (mut sim, _, _) = active_build_sim(false);
    sim.map.world.danger[0].fill(0x1234_5678);
    let danger_before = sim.map.world.danger.clone();
    let daemon_before = sim.game_daemon.clone();

    let frame = sim.do_frame();

    assert_eq!(
        frame.steps[12],
        StepRun::Unimplemented(Gap::GameDaemonCalcDanger)
    );
    assert_eq!(sim.map.world.danger, danger_before);
    assert_eq!(sim.game_daemon, daemon_before);
    assert_eq!(sim.cover.gaps[Gap::GameDaemonCalcDanger.index()], 1);
}

#[test]
fn inactive_owner_objects_do_not_demand_unreached_type_answers() {
    let mut sim = Sim::new(0x732d_10ff, 16);
    sim.spawn_unit(0, 77, 1536, 1536, 1).unwrap();
    sim.map.world.danger[0][0] = 909;

    let frame = sim.do_frame();

    assert_eq!(frame.steps[12], StepRun::Executed);
    assert_eq!(sim.cover.gaps[Gap::GameDaemonCalcDanger.index()], 0);
    assert_eq!(sim.map.world.danger[0][0], 909);
}
