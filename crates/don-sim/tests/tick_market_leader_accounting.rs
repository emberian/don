use don_sim::systems::leader_market_build_accounting::{
    MarketLeaderAccountingError, MarketLeaderRegionAuthority, BUILD_ACTIVATE_DIRTY_FLAG,
    BUILD_INIT_DIRTY_FLAG, GOLDEN_MARKET_OBJECT_ID, MARKET_BUILDING_SLOT, MARKET_GATHER_SLOT,
    MARKET_TYPE,
};
use don_sim::systems::production::{self, BuildData};
use don_sim::tick::Sim;

fn append_build(sim: &mut Sim, object_id: i16, type_index: i32, mut build: BuildData) -> usize {
    build.other[production::off::OBJECT_ID..production::off::OBJECT_ID + 2]
        .copy_from_slice(&object_id.to_le_bytes());
    let row = sim.spawn_build(0, build);
    sim.production_runtime.register_build(row, type_index);
    row
}

fn source() -> MarketLeaderRegionAuthority {
    MarketLeaderRegionAuthority {
        capture_revision: 11,
        native_trace_sha256: [0x51; 32],
        after_sim_sha256: [0x52; 32],
        owner: 0,
        object_id: GOLDEN_MARKET_OBJECT_ID,
        type_index: MARKET_TYPE,
        region: 7,
    }
}

#[test]
fn linked_active_city_zero_market_commits_every_canonical_leader_owner() {
    let mut sim = Sim::new(1, 4);
    sim.activate(0);
    append_build(
        &mut sim,
        2_000,
        414,
        BuildData {
            flags: production::flag::ACTIVE,
            city: 0,
            ..BuildData::default()
        },
    );
    let row = append_build(
        &mut sim,
        GOLDEN_MARKET_OBJECT_ID,
        MARKET_TYPE,
        BuildData {
            flags: production::flag::ACTIVE,
            city: 0,
            ..BuildData::default()
        },
    );
    assert_eq!(row, 1);
    sim.vic_leaders.slots[0].buildings_built = 1;

    let prepared = sim
        .prepare_golden_market_leader_accounting(source())
        .unwrap();
    let receipt = sim
        .commit_golden_market_leader_accounting(source(), prepared)
        .unwrap();

    let leader = &sim.vic_leaders.slots[0];
    let regional_index = 7 * 129 + MARKET_BUILDING_SLOT;
    assert_eq!(leader.buildings_built, 2);
    assert_eq!(leader.num_buildings[MARKET_BUILDING_SLOT], 1);
    assert_eq!(leader.reg_buildings[regional_index], 1);
    assert_eq!(leader.gather_slots[MARKET_GATHER_SLOT], 1);
    assert_eq!(leader.gather_slots_high[MARKET_GATHER_SLOT], 1);
    assert_eq!(leader.high_buildings[MARKET_BUILDING_SLOT], 1);
    assert_eq!(
        leader.leader_flags as u32, sim.step8.leaders[0].flags,
        "the checksum and step-8 mirrors commit together"
    );
    assert_eq!(
        leader.leader_flags as u32 & (BUILD_INIT_DIRTY_FLAG | BUILD_ACTIVATE_DIRTY_FLAG),
        BUILD_INIT_DIRTY_FLAG | BUILD_ACTIVATE_DIRTY_FLAG
    );
    assert_eq!(receipt.after_init.buildings_built, 2);
    assert_eq!(receipt.after_increment_stats.num_buildings, 1);
    assert_eq!(receipt.after_market_special.high_buildings, 1);
}

#[test]
fn build_link_type_active_and_city_are_live_preconditions() {
    let mut sim = Sim::new(1, 4);
    sim.activate(0);
    append_build(&mut sim, 2_000, 414, BuildData::default());
    append_build(
        &mut sim,
        GOLDEN_MARKET_OBJECT_ID,
        MARKET_TYPE,
        BuildData {
            flags: production::flag::ACTIVE,
            city: -1,
            ..BuildData::default()
        },
    );
    assert_eq!(
        sim.prepare_golden_market_leader_accounting(source()),
        Err(MarketLeaderAccountingError::LinkedBuildDisagreement)
    );
    sim.builds[1].city = 0;
    sim.builds[1].flags &= !production::flag::ACTIVE;
    assert_eq!(
        sim.prepare_golden_market_leader_accounting(source()),
        Err(MarketLeaderAccountingError::LinkedBuildDisagreement)
    );
}
