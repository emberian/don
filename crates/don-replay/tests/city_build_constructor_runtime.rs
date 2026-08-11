#[path = "../src/city_build_constructor_runtime.rs"]
mod subject;

use don_sim::systems::map_terrain::{World, COORD_PER_WCELL};
use don_sim::systems::production::{self, BuildData};
use don_sim::systems::tech_cities;
use subject::{
    apply_city_fix_world_vals, apply_fresh_starting_village_projection,
    civ_specific_free_build_plan, plan_city_name, small_city_free_build_plan, CityNamePlanRequest,
    CityNameSelectionBranch, CivSpecificBuildingsRequest, ConstructorCall,
    FreshStartingVillageRequest, SmallCityBuildingsRequest, StartingCityConstructorError,
    BUILD_ACTIVATE_VA, BUILD_INIT_VA, CAPITAL_CITY_FLAGS, CITIES_INIT_CITY_VA,
    CITY_CARAVAN_INITIAL_CAPACITY, CITY_CENTER_OBJECT_FLAG, CITY_FIX_WORLD_VALS_VA, CITY_INIT_VA,
    FRESH_STARTING_CITY_CALL_ORDER, OBJECT_ADD_TO_WORLD_VA, SETUP_BUILD_CITIES_VA,
    WALL_ACTIVATE_VA,
};

const XOR: i32 = 0x63637;

fn write_i16(image: &mut [u8], offset: usize, value: i16) {
    image[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn write_i32(image: &mut [u8], offset: usize, value: i32) {
    image[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn starting_build(owner: u8, object_id: i16, position: (i32, i32)) -> BuildData {
    let mut build = BuildData {
        flags: production::flag::VALID | CITY_CENTER_OBJECT_FLAG,
        who: owner,
        orig_type: tech_cities::ty::VILLAGE,
        city: -1,
        city_down: -1,
        founder: owner as i8,
        ..BuildData::default()
    };
    write_i16(&mut build.other, production::off::OBJECT_ID, object_id);
    write_i32(
        &mut build.other,
        production::off::X_INTERNAL,
        position.0 ^ XOR,
    );
    write_i32(
        &mut build.other,
        production::off::Y_INTERNAL,
        position.1 ^ XOR,
    );
    build
}

#[test]
fn shipped_addresses_and_direct_call_order_are_pinned() {
    assert_eq!(SETUP_BUILD_CITIES_VA, 0x005a_b910);
    assert_eq!(OBJECT_ADD_TO_WORLD_VA, 0x0064_d8c0);
    assert_eq!(BUILD_INIT_VA, 0x0062_9740);
    assert_eq!(WALL_ACTIVATE_VA, 0x0063_e4b0);
    assert_eq!(BUILD_ACTIVATE_VA, 0x0062_3e20);
    assert_eq!(CITIES_INIT_CITY_VA, 0x0073_52c0);
    assert_eq!(CITY_INIT_VA, 0x0073_7050);
    assert_eq!(CITY_FIX_WORLD_VALS_VA, 0x0073_5aa0);
    assert_eq!(
        FRESH_STARTING_CITY_CALL_ORDER[0],
        ConstructorCall::ObjectAddToWorld
    );
    assert_eq!(
        FRESH_STARTING_CITY_CALL_ORDER.last(),
        Some(&ConstructorCall::ObjectUpdateSeenAlly)
    );
}

#[test]
fn first_city_name_uses_shared_tribe_ordinal_without_main_rng() {
    let receipt = plan_city_name(CityNamePlanRequest {
        owner: 3,
        tribes: [7, 9, 7, 7, 11, 12, 13, 14],
        city_name_counters: [0; 8],
        world_seed: 0x1234_5678,
        list_len: 20,
    })
    .unwrap();
    assert_eq!(receipt.branch, CityNameSelectionBranch::FirstCohortOrdinal);
    assert_eq!(receipt.counter_owner, 3);
    assert_eq!(receipt.name_ordinal, Some(3));
    assert_eq!(receipt.counters_after[3], 1);
    assert_eq!(receipt.main_rng_draws, 0);
}

#[test]
fn later_city_name_advances_highest_shared_counter_and_local_lfsr_only() {
    let receipt = plan_city_name(CityNamePlanRequest {
        owner: 3,
        tribes: [7, 9, 7, 7, 11, 12, 13, 14],
        city_name_counters: [3, 0, 1, 2, 0, 0, 0, 0],
        world_seed: 41,
        list_len: 20,
    })
    .unwrap();
    assert_eq!(receipt.branch, CityNameSelectionBranch::DeterministicLfsr);
    assert_eq!(receipt.counter_owner, 0);
    assert_eq!(receipt.counters_after[0], 4);
    assert!(receipt
        .name_ordinal
        .is_some_and(|ordinal| (3..=20).contains(&ordinal)));
    assert_eq!(receipt.main_rng_draws, 0);
}

#[test]
fn city_fix_world_vals_uses_the_regenerated_105_plus_132_village_template() {
    let mut world = World::init_default_rules(64, 64);
    for cell in &mut world.wdata {
        cell.val = 0xff;
    }
    let position = (32 * COORD_PER_WCELL + 96, 32 * COORD_PER_WCELL + 96);
    let receipt = apply_city_fix_world_vals(&mut world, position, 20).unwrap();
    assert_eq!(receipt.circle_index, 5);
    assert_eq!(receipt.inner_template_entries, 105);
    assert_eq!(receipt.total_template_entries, 237);
    assert_eq!(receipt.in_bounds_entries, 237);
    assert_eq!(receipt.changed_entries, 237);
    assert_eq!(
        world.wdata.iter().filter(|cell| cell.val == 0x3f).count(),
        105
    );
    assert_eq!(
        world.wdata.iter().filter(|cell| cell.val == 0x7f).count(),
        132
    );
}

#[test]
fn fresh_village_projection_links_build_and_emits_exact_city_walk_body() {
    let owner = 2;
    let object_id = 2000;
    let position = (30 * COORD_PER_WCELL + 96, 31 * COORD_PER_WCELL + 96);
    let mut build = starting_build(owner, object_id, position);
    build.job_counter = 123;
    build.job_counter_2 = 456;
    let mut world = World::init_default_rules(64, 64);
    for cell in &mut world.wdata {
        cell.val = 0xff;
    }
    let center = 31usize * 64 + 30;
    world.wdata[center].region = 17;

    let receipt = apply_fresh_starting_village_projection(
        &mut build,
        &mut world,
        FreshStartingVillageRequest {
            owner,
            city_slot: 0,
            current_type: tech_cities::ty::VILLAGE,
            city_name: "Athens".into(),
            city_id: String::new(),
            indian_radius_bonus: false,
        },
    )
    .unwrap();

    assert_eq!(receipt.object_id, object_id);
    assert_eq!(receipt.region, 17);
    assert_eq!(receipt.position, position);
    assert_eq!(receipt.city_link_before, -1);
    assert_eq!(receipt.city_link_after, 0);
    assert_eq!(build.city, 0);
    assert!(build.is_started());
    assert!(build.is_active());
    assert_eq!(build.job_counter, 0);
    assert_eq!(build.job_counter_2, 0);
    assert_ne!(build.build_masks & 0x1000, 0);

    let city = &receipt.city;
    assert_eq!(city.city_flags, CAPITAL_CITY_FLAGS);
    assert_eq!((city.city, city.o, city.reg), (0, object_id, 17));
    assert_eq!((city.x, city.y), position);
    assert_eq!((city.pop, city.who, city.race, city.founder), (1, 2, 2, 2));
    assert_eq!((city.land, city.filled), (9, 1));
    assert_eq!(city.conquest_node, -1);
    assert!(city.vans.items.is_empty());
    assert_eq!(city.vans.capacity, CITY_CARAVAN_INITIAL_CAPACITY);
    assert_eq!(city.vans.grow, -1);
    assert_eq!(city.name, "Athens");
    assert_eq!(receipt.effects.leader_pop_delta, 1);
    assert_eq!(receipt.effects.game_world_pop_delta, 1);
    assert_eq!(receipt.effects.region_border_caches_cleared, 64);
    assert_eq!(receipt.effects.leader_calc_pop_cap_calls, 2);
    assert_eq!(receipt.effects.main_rng_draws, 0);
    assert!(receipt.city_init_complete);
    assert!(!receipt.build_init_complete);
    assert!(!receipt.build_activation_complete);
    assert!(!receipt.builds_channel_ready);
}

#[test]
fn projection_refuses_prelinked_build_atomically() {
    let position = (10 * COORD_PER_WCELL + 96, 10 * COORD_PER_WCELL + 96);
    let mut build = starting_build(0, 2000, position);
    build.city = 4;
    let before = build.image();
    let mut world = World::init_default_rules(32, 32);
    let world_before = world.checksum();
    let err = apply_fresh_starting_village_projection(
        &mut build,
        &mut world,
        FreshStartingVillageRequest {
            owner: 0,
            city_slot: 0,
            current_type: tech_cities::ty::VILLAGE,
            city_name: String::new(),
            city_id: String::new(),
            indian_radius_bonus: false,
        },
    )
    .unwrap_err();
    assert_eq!(
        err,
        StartingCityConstructorError::BuildAlreadyLinked { city: 4 }
    );
    assert_eq!(build.image(), before);
    assert_eq!(world.checksum(), world_before);
}

#[test]
fn starting_town_two_follow_on_free_build_order_is_explicit() {
    assert_eq!(
        small_city_free_build_plan(SmallCityBuildingsRequest {
            bonus_20: false,
            bonus_20_farms: 0,
            bonus_19: false,
        }),
        vec![
            tech_cities::ty::WOODCUTTER,
            tech_cities::ty::FARM,
            tech_cities::ty::FARM,
            tech_cities::ty::FARM,
            tech_cities::ty::LIBRARY,
        ]
    );
    assert_eq!(
        civ_specific_free_build_plan(CivSpecificBuildingsRequest {
            bonus_4: true,
            bonus_5: true,
            bonus_5_rule: 2,
            bonus_16: true,
            bonus_16_rule: 2,
            bonus_7: true,
            bonus_7_rule: 2,
            bonus_10: true,
            bonus_10_rule: 2,
            bonus_18: true,
            bonus_18_rule: 1,
            ..Default::default()
        }),
        vec![
            tech_cities::ty::MARKET,
            tech_cities::ty::UNIVERSITY,
            tech_cities::ty::TEMPLE,
            tech_cities::ty::SMELTER_AGE0,
            tech_cities::ty::SMELTER_AGE1,
            tech_cities::ty::CAPITOL,
        ]
    );
}
