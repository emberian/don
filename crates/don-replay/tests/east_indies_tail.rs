// SPDX-License-Identifier: GPL-3.0-or-later
//! Mutation-sensitive proof pack for the East Indies non-player-island tail.

// The production module is intentionally not registered until the continent caller owns
// the integration hook.  This shim gives its `crate::growth` import the same shape here.
mod growth {
    pub use don_replay::growth::*;
}

#[path = "../src/east_indies_tail.rs"]
mod east_indies_tail;

use don_sim::rng::Random;
use don_sim::systems::map_terrain::{land, World, WorldSection};
use don_sim::systems::regions::Regions;
use east_indies_tail::{
    execute_east_indies_tail, EastIndiesAreaAdjustmentKind, EastIndiesDirectDrawKind,
    EastIndiesRegionDefaults, EastIndiesRngSpanKind, EastIndiesTailCall, EastIndiesTailError,
    EastIndiesTailReturn, EAST_INDIES_ISLAND_AREA_RNG_VA, EAST_INDIES_ISLAND_X_RNG_VA,
    EAST_INDIES_ISLAND_Y_RNG_VA, EAST_INDIES_NONPLAYER_ISLANDS_VA, EAST_INDIES_RETURN_VA,
    REGIONS_CLEAR_ALL_VA,
};
use growth::MapGrowthConfig;

fn config() -> MapGrowthConfig {
    MapGrowthConfig {
        avoid_center: 0,
        avoid_continent: 4,
        edge_avoid: [2; 4],
        base_edge: 0,
        edge_jitter: 0,
    }
}

fn call(players: i32) -> EastIndiesTailCall {
    EastIndiesTailCall {
        player_regions: players,
        region_defaults: EastIndiesRegionDefaults {
            common_factor: 4,
            goody_factor: 8,
            flags: 0,
        },
    }
}

fn seeded_players(edge: i32, seed: i32, players: i32) -> (World, Regions) {
    seeded_players_dims(edge, edge, seed, players)
}

fn seeded_players_dims(xs: i32, ys: i32, seed: i32, players: i32) -> (World, Regions) {
    let mut world = World::init_default_rules(xs, ys);
    world.seed_map_generation(seed);
    world.wipe();
    let mut regions = Regions::default();
    regions.clear_all(&mut world);
    for region in 1..=players {
        let x = xs * region / (players + 1);
        let y = ys / 2;
        let cell = world.wdata_mut(x, y);
        cell.land = land::FERTILE;
        cell.land_sub = 0;
        cell.region = region as i16;
        let row = &mut regions.list[region as usize];
        row.coords.capacity = 4;
        row.coords.items.push((x, y));
        row.size = 1;
        regions.land = region;
    }
    (world, regions)
}

#[test]
fn tail_mutates_only_wdata_and_records_one_unbroken_rng_chronology() {
    let seed = 0x1234_5678;
    let (mut world, mut regions) = seeded_players(48, seed, 2);
    let before = world.checksum_sections();
    let starts_before = (
        world.start_x.clone(),
        world.start_y.clone(),
        world.start_city_x.clone(),
        world.start_city_y.clone(),
    );
    let mut rng = Random::new(seed);
    let mut growth = config();
    let receipt =
        execute_east_indies_tail(&mut world, &mut regions, &mut rng, &mut growth, call(2)).unwrap();

    assert_eq!(receipt.entry_va, EAST_INDIES_NONPLAYER_ISLANDS_VA);
    assert_eq!(receipt.return_va, EAST_INDIES_RETURN_VA);
    assert_eq!(receipt.common_continuation_va, REGIONS_CLEAR_ALL_VA);
    assert_eq!(
        receipt.return_reason,
        EastIndiesTailReturn::ConsecutiveSeedFailureBudget,
        "requested={} placed={} remaining={} valid_calls={} direct_draws={}",
        receipt.requested_islands,
        receipt.placed_islands,
        receipt.remaining_islands,
        receipt.grow_valid_calls.len(),
        receipt.direct_draws.len(),
    );
    assert_eq!(
        (
            receipt.requested_islands,
            receipt.placed_islands,
            receipt.remaining_islands
        ),
        (10, 9, 1),
        "native's bounded escape returns with the residual explicitly visible"
    );
    assert_eq!(receipt.placed_islands as usize, receipt.islands.len());
    assert_eq!(receipt.grow_valid_calls.len(), 105);
    assert_eq!(receipt.direct_draws.len(), 12_899);
    assert_eq!(
        receipt.rng_chronology.len(),
        receipt.direct_draws.len() + receipt.grow_valid_calls.len() + receipt.islands.len(),
        "every direct draw and helper call occupies exactly one chronological span"
    );
    assert_eq!(regions.land, 2 + receipt.placed_islands);
    assert_eq!(receipt.world_sections_changed, [WorldSection::WData]);
    assert_eq!(
        before.differing_sections(&world.checksum_sections()),
        [WorldSection::WData]
    );
    assert_eq!(
        starts_before,
        (
            world.start_x.clone(),
            world.start_y.clone(),
            world.start_city_x.clone(),
            world.start_city_y.clone(),
        ),
        "the tail owns WData only; player starts were already installed"
    );

    assert_eq!(
        receipt.direct_draws.first().unwrap().kind,
        EastIndiesDirectDrawKind::IslandCountParity
    );
    assert_eq!(
        receipt.direct_draws.first().unwrap().call_va,
        EAST_INDIES_NONPLAYER_ISLANDS_VA
    );
    assert!(receipt.direct_draws.iter().skip(1).all(|draw| matches!(
        (draw.kind, draw.call_va),
        (
            EastIndiesDirectDrawKind::CandidateX,
            EAST_INDIES_ISLAND_X_RNG_VA
        ) | (
            EastIndiesDirectDrawKind::CandidateY,
            EAST_INDIES_ISLAND_Y_RNG_VA
        ) | (
            EastIndiesDirectDrawKind::NextIslandArea,
            EAST_INDIES_ISLAND_AREA_RNG_VA
        )
    )));
    assert!(receipt
        .rng_chronology
        .windows(2)
        .all(|pair| pair[0].state_after == pair[1].state_before));
    let chronological_direct = receipt
        .rng_chronology
        .iter()
        .filter_map(|span| match span.kind {
            EastIndiesRngSpanKind::Direct(kind) => Some((kind, span)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(chronological_direct.len(), receipt.direct_draws.len());
    for ((kind, span), draw) in chronological_direct.into_iter().zip(&receipt.direct_draws) {
        assert_eq!(kind, draw.kind);
        assert_eq!(span.state_before, draw.state_before);
        assert_eq!(span.state_after, draw.state_after);
        assert_eq!(span.call_sites, [draw.call_va]);
    }
    assert_eq!(receipt.rng_initial, seed);
    assert_eq!(receipt.rng_final, rng.state());
    assert!(receipt
        .rng_chronology
        .iter()
        .any(|span| matches!(span.kind, EastIndiesRngSpanKind::GrowValid { .. })));
    assert!(receipt
        .rng_chronology
        .iter()
        .any(|span| matches!(span.kind, EastIndiesRngSpanKind::GrowRegion { .. })));

    for island in &receipt.islands {
        let row = &regions.list[island.region as usize];
        assert_eq!((row.common_factor, row.goody_factor), (5, 5));
        assert_eq!((row.flags, row.climate), (0, 1));
        assert_eq!(row.size as usize, row.coords.items.len());
        assert_eq!(
            world
                .wdata
                .iter()
                .filter(|cell| cell.region == island.region as i16)
                .count(),
            row.size as usize
        );
        assert!(island.actual_land_distance >= island.required_land_distance);
        assert_eq!(island.growth.call.max_distance, world.xs);
        assert_eq!(island.growth.call.target_area, island.target_area);
    }
}

#[test]
fn thousand_candidate_budget_drives_the_exact_two_step_area_floor_exit() {
    let seed = 73;
    let (mut world, mut regions) = seeded_players(20, seed, 1);
    // Make every cell land. `land_dist` therefore returns zero before any ring walk,
    // forcing exactly 1,000 rejected candidates per adaptive-area pass.
    for cell in &mut world.wdata {
        cell.land = land::FERTILE;
        cell.region = 1;
    }
    let before = world.checksum_image().0;
    let mut rng = Random::new(seed);
    let mut growth = config();
    let receipt =
        execute_east_indies_tail(&mut world, &mut regions, &mut rng, &mut growth, call(1)).unwrap();

    assert_eq!(
        receipt.return_reason,
        EastIndiesTailReturn::ReducedAreaBelowTwenty
    );
    assert_eq!(receipt.placed_islands, 0);
    assert_eq!(receipt.remaining_islands, receipt.requested_islands);
    assert_eq!(receipt.world_sections_changed, []);
    assert_eq!(world.checksum_image().0, before);
    assert_eq!(receipt.area_adjustments.len(), 2);
    assert_eq!(
        receipt.area_adjustments[0].kind,
        EastIndiesAreaAdjustmentKind::CollapseToMinimum
    );
    assert_eq!(
        (
            receipt.area_adjustments[0].target_before,
            receipt.area_adjustments[0].minimum_before,
            receipt.area_adjustments[0].target_after,
        ),
        (90, 24, 24)
    );
    assert_eq!(
        receipt.area_adjustments[1].kind,
        EastIndiesAreaAdjustmentKind::ReduceMinimum
    );
    assert_eq!(
        (
            receipt.area_adjustments[1].minimum_before,
            receipt.area_adjustments[1].minimum_after,
        ),
        (24, 19)
    );
    assert_eq!(receipt.grow_valid_calls.len(), 0);
    assert_eq!(receipt.direct_draws.len(), 1 + 2 * 2 * 1000);
}

#[test]
fn post_draw_capacity_error_is_atomic_across_all_four_owned_stores() {
    let seed = 99;
    // Deliberately overfill the admitted land-region id domain without allocating a
    // giant World. The capacity check occurs only after retail's parity draw is staged.
    let (mut world, mut regions) = seeded_players_dims(672, 2, seed, 1);
    let world_before = world.checksum_image().0;
    let regions_before = regions.clone();
    let mut rng = Random::new(seed);
    let rng_before = rng;
    let mut growth = config();
    let growth_before = growth.clone();

    let error = execute_east_indies_tail(&mut world, &mut regions, &mut rng, &mut growth, call(1))
        .unwrap_err();
    assert!(matches!(
        error,
        EastIndiesTailError::LandRegionCapacity { .. }
    ));
    assert_eq!(world.checksum_image().0, world_before);
    assert_eq!(regions, regions_before);
    assert_eq!(
        rng, rng_before,
        "the parity draw was staged, then rolled back"
    );
    assert_eq!(growth, growth_before);
}

#[test]
fn integration_hook_and_residual_are_address_exact() {
    assert_eq!(
        EAST_INDIES_NONPLAYER_ISLANDS_VA,
        don_replay::continent::EAST_INDIES_NONPLAYER_ISLANDS_VA
    );
    assert_eq!(EAST_INDIES_RETURN_VA, 0x0069_7da4);
    assert_eq!(REGIONS_CLEAR_ALL_VA, 0x0068_0060);
    assert_ne!(
        EAST_INDIES_RETURN_VA, REGIONS_CLEAR_ALL_VA,
        "the style tail returns; the common driver owns region rebuilding"
    );
}
