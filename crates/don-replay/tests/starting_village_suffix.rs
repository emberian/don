use don_replay::build_spawn_runtime::{spawn_canonical_build, CanonicalBuildSpawnRequest};
use don_sim::systems::production::{self, BuildData};
use don_sim::systems::tech_cities::{self, CityRecord};
use don_sim::tick::Sim;

#[path = "../src/starting_village_suffix.rs"]
mod subject;

const OWNER: u8 = 0;
const POSITION: (i32, i32) = (0x1260, 0x1ce0);

fn fresh_sim_and_city() -> (Sim, CityRecord) {
    let mut sim = Sim::new(7, 64);
    sim.activate(usize::from(OWNER));
    let spawn = spawn_canonical_build(
        &mut sim,
        CanonicalBuildSpawnRequest {
            owner: OWNER,
            type_index: tech_cities::ty::VILLAGE,
            snapped_x: POSITION.0,
            snapped_y: POSITION.1,
            build: BuildData {
                flags: subject::FRESH_CENTER_FLAGS,
                orig_type: tech_cities::ty::VILLAGE,
                city: -1,
                city_down: -1,
                wonder: -1,
                dock: -1,
                attack_ox: -1,
                attack_whom: -1,
                ..BuildData::default()
            },
        },
    )
    .expect("fresh center spawn");
    sim.builds[spawn.row].city = 0;

    let city = CityRecord {
        city_flags: subject::FRESH_CAPITAL_CITY_FLAGS,
        city: 0,
        o: spawn.object_id,
        reg: 1,
        x: POSITION.0,
        y: POSITION.1,
        conquest_node: -1,
        pop: 1,
        who: OWNER as i8,
        race: OWNER as i8,
        founder: OWNER as i8,
        land: 9,
        filled: 1,
        ..CityRecord::default()
    };
    (sim, city)
}

#[test]
fn one_center_path_has_no_city_build_world_leader_or_rng_suffix() {
    let (sim, city) = fresh_sim_and_city();
    let city_before = city.pod_bytes();

    let receipt = subject::attest_fresh_starting_village_suffix(
        &sim,
        &city,
        subject::FreshVillageUpgradeFacts {
            town_type_available: true,
        },
    )
    .expect("source-bounded fresh suffix");

    assert_eq!(receipt.scanned_build_rows, 1);
    assert_eq!(receipt.rejected_city_center_rows, 1);
    assert_eq!(receipt.add_to_city_calls, 0);
    assert_eq!(receipt.city_pod_writes, 0);
    assert_eq!(receipt.build_city_link_writes, 0);
    assert_eq!(receipt.distinct_active_kinds, 1);
    assert_eq!(receipt.required_distinct_kinds, 6);
    assert_eq!(receipt.can_upgrade_calls, 1);
    assert!(!receipt.check_upgrade_body_entered);
    assert_eq!(receipt.regen_members_walked, 0);
    assert_eq!(receipt.regen_build_mask_writes, 0);
    assert_eq!((receipt.leader_writes, receipt.world_writes), (0, 0));
    assert_eq!(receipt.main_rng_draws, 0);
    assert!(receipt.city_suffix_complete);
    assert!(!receipt.first_checksum_city_image_ready);
    assert!(!receipt.constructor_transaction_ready());
    assert_eq!(city.pod_bytes(), city_before);
}

#[test]
fn unavailable_town_short_circuits_before_can_upgrade_but_is_still_inert() {
    let (sim, city) = fresh_sim_and_city();
    let receipt = subject::attest_fresh_starting_village_suffix(
        &sim,
        &city,
        subject::FreshVillageUpgradeFacts {
            town_type_available: false,
        },
    )
    .expect("unavailable Town is inert");

    assert_eq!(receipt.type_avail_calls, 1);
    assert_eq!(receipt.can_upgrade_calls, 0);
    assert!(!receipt.check_upgrade_body_entered);
}

#[test]
fn a_non_center_build_refuses_the_empty_scan_receipt() {
    let (mut sim, city) = fresh_sim_and_city();
    let spawn = spawn_canonical_build(
        &mut sim,
        CanonicalBuildSpawnRequest {
            owner: OWNER,
            type_index: tech_cities::ty::FARM,
            snapped_x: POSITION.0 + 0x300,
            snapped_y: POSITION.1,
            build: BuildData {
                flags: production::flag::VALID
                    | production::flag::STARTED
                    | production::flag::ACTIVE,
                orig_type: tech_cities::ty::FARM,
                city: -1,
                city_down: -1,
                ..BuildData::default()
            },
        },
    )
    .expect("non-center spawn");

    let err = subject::attest_fresh_starting_village_suffix(
        &sim,
        &city,
        subject::FreshVillageUpgradeFacts {
            town_type_available: true,
        },
    )
    .expect_err("Farm reaches the general find_buildings fork");
    assert_eq!(
        err,
        subject::FreshVillageSuffixError::ScannedNonCenterBuild {
            row: spawn.row,
            owner: OWNER,
            object_id: spawn.object_id,
            flags: 0x07,
        }
    );
}

#[test]
fn a_nonempty_city_down_chain_refuses_the_regen_receipt() {
    let (mut sim, city) = fresh_sim_and_city();
    sim.builds[0].city_down = 2001;

    let err = subject::attest_fresh_starting_village_suffix(
        &sim,
        &city,
        subject::FreshVillageUpgradeFacts {
            town_type_available: true,
        },
    )
    .expect_err("regen_roads would visit a member");
    assert_eq!(
        err,
        subject::FreshVillageSuffixError::CenterChainNotEmpty { city_down: 2001 }
    );
}

#[test]
fn source_addresses_and_shipped_threshold_are_frozen() {
    assert_eq!(subject::CITY_FIND_BUILDINGS_VA, 0x0073_84c0);
    assert_eq!(subject::CITY_READY_TO_UPGRADE_VA, 0x0073_6480);
    assert_eq!(subject::CITY_CAN_UPGRADE_VA, 0x0073_83c0);
    assert_eq!(subject::CITY_ENOUGH_KINDS_VA, 0x0073_6540);
    assert_eq!(subject::CITY_CHECK_UPGRADE_VA, 0x0073_8b20);
    assert_eq!(subject::CITY_REGEN_ROADS_VA, 0x0073_8aa0);
    assert_eq!(subject::BUILD_ADD_TO_CITY_VA, 0x0062_2380);
    assert_eq!(subject::LEADER_PLAN_STRATEGY_VA, 0x006b_9620);
    assert_eq!(subject::SHIPPED_CITY_BUILDINGS, 5);
    assert_eq!(subject::TOWN_REQUIRED_DISTINCT_KINDS, 6);
}
