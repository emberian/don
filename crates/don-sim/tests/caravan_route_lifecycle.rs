use don_sim::systems::economy::{
    activate_caravan_income, calc_city_resources, calc_gather, do_gather, end_caravan_route,
    establish_caravan_route, recompute_city_caravan_income_from_state, CaravanCityLinks,
    CaravanEndpoint, CaravanIncomeGates, CaravanLink, CaravanPools, CaravanRouteError,
    CaravanTradeCity, CityResourceInputs, DoGatherContext, EconRules, GatherInputs, LeaderEcon,
    CARAVAN_ACTIVE, CARAVAN_EARNING, CARAVAN_ESTABLISHED, INITIAL_CARAVAN_POOL_SIZE, RES_WEALTH,
    TRADE_METROPOLIS_TYPE, TRADE_TOWN_TYPE,
};

fn endpoint(city: i16, owner: i16) -> CaravanEndpoint {
    CaravanEndpoint { city, owner }
}

fn resolved(ep: CaravanEndpoint) -> Option<CaravanTradeCity> {
    match (ep.owner, ep.city) {
        (0, 3) => Some(CaravanTradeCity {
            live: true,
            owner: 0,
            center_type: TRADE_TOWN_TYPE,
            num_buildings: 8,
            x: 0,
            y: 0,
        }),
        (1, 7) => Some(CaravanTradeCity {
            live: true,
            owner: 1,
            center_type: TRADE_METROPOLIS_TYPE,
            num_buildings: 11,
            x: 10 * 768,
            y: 0,
        }),
        _ => None,
    }
}

#[test]
fn pool_uses_first_inactive_slot_and_shrinks_only_the_inactive_tail() {
    let mut pools = CaravanPools::default();
    assert_eq!(
        pools.pool(0).unwrap().allocated_len(),
        INITIAL_CARAVAN_POOL_SIZE
    );
    assert_eq!(pools.pool(0).unwrap().high_water(), 0);
    assert_eq!(
        pools.pool(0).unwrap().logical_capacity(),
        INITIAL_CARAVAN_POOL_SIZE
    );
    assert!(pools
        .pool(0)
        .unwrap()
        .allocated_records()
        .iter()
        .all(|record| record.slot == 0 && !record.is_active()));

    let a = pools.init_caravan(0, 10).unwrap();
    let b = pools.init_caravan(0, 11).unwrap();
    let c = pools.init_caravan(0, 12).unwrap();
    assert_eq!((a.slot, b.slot, c.slot), (0, 1, 2));

    pools.close_caravan(b).unwrap();
    assert_eq!(pools.pool(0).unwrap().high_water(), 3);
    assert_eq!(pools.pool(0).unwrap().allocated_records()[1].slot, 1);
    let reused = pools.init_caravan(0, 99).unwrap();
    assert_eq!(reused, b, "retail scans inactive slots from zero");
    assert_eq!(pools.record(reused).unwrap().unit_object, 99);

    pools.close_caravan(c).unwrap();
    assert_eq!(pools.pool(0).unwrap().high_water(), 2);
    pools.close_caravan(reused).unwrap();
    assert_eq!(pools.pool(0).unwrap().high_water(), 1);
    pools.close_caravan(a).unwrap();
    assert_eq!(pools.pool(0).unwrap().high_water(), 0);
    assert_eq!(
        pools.pool(0).unwrap().allocated_len(),
        INITIAL_CARAVAN_POOL_SIZE
    );
}

#[test]
fn pool_expands_after_twenty_live_routes_without_renumbering_them() {
    let mut pools = CaravanPools::default();
    let links: Vec<_> = (0..=INITIAL_CARAVAN_POOL_SIZE)
        .map(|unit| pools.init_caravan(2, unit as i16).unwrap())
        .collect();
    assert_eq!(links[0].slot, 0);
    assert_eq!(links[INITIAL_CARAVAN_POOL_SIZE].slot, 20);
    assert_eq!(pools.pool(2).unwrap().high_water(), 21);
    assert_eq!(pools.pool(2).unwrap().allocated_len(), 21);
    assert_eq!(pools.pool(2).unwrap().logical_capacity(), 40);
}

#[test]
fn city_link_array_grows_four_then_doubles_and_removes_the_first_match_in_order() {
    let mut links = CaravanCityLinks::default();
    let repeated = CaravanLink { slot: 4, owner: 1 };
    links.add(repeated);
    for slot in 5..8 {
        links.add(CaravanLink { slot, owner: 1 });
    }
    assert_eq!(links.logical_capacity(), 4);
    links.add(repeated);
    assert_eq!(links.logical_capacity(), 8);

    assert!(links.remove(repeated));
    assert_eq!(
        links.as_slice(),
        &[
            CaravanLink { slot: 5, owner: 1 },
            CaravanLink { slot: 6, owner: 1 },
            CaravanLink { slot: 7, owner: 1 },
            repeated,
        ]
    );
    assert!(!links.remove(CaravanLink { slot: 99, owner: 1 }));
    assert_eq!(
        links.logical_capacity(),
        8,
        "remove does not shrink allocation"
    );
}

#[test]
fn establish_arrive_end_and_close_drive_real_city_income() {
    let rules = EconRules::shipped();
    let mut pools = CaravanPools::default();
    let route = pools.init_caravan(0, 42).unwrap();
    let mut source_links = CaravanCityLinks::default();
    let mut destination_links = CaravanCityLinks::default();

    establish_caravan_route(
        &mut pools,
        route,
        endpoint(3, 0),
        endpoint(7, 1),
        &mut source_links,
        &mut destination_links,
    )
    .unwrap();
    let record = pools.record(route).unwrap();
    assert_eq!(record.flags, CARAVAN_ACTIVE | CARAVAN_ESTABLISHED);
    assert_eq!(source_links.as_slice(), &[route]);
    assert_eq!(destination_links.as_slice(), &[route]);

    let mut trade_val = 0;
    assert!(!recompute_city_caravan_income_from_state(
        &rules,
        40,
        0,
        &CaravanIncomeGates::default(),
        &pools,
        &source_links,
        resolved,
        &mut trade_val,
    )
    .unwrap());
    assert_eq!(trade_val, 0, "a road-in-progress route does not earn");

    activate_caravan_income(&mut pools, route).unwrap();
    assert_eq!(
        pools.record(route).unwrap().flags,
        CARAVAN_ACTIVE | CARAVAN_ESTABLISHED | CARAVAN_EARNING
    );
    assert!(recompute_city_caravan_income_from_state(
        &rules,
        40,
        0,
        &CaravanIncomeGates::default(),
        &pools,
        &source_links,
        resolved,
        &mut trade_val,
    )
    .unwrap());
    // (10 + 15) * 4/3 = 33, foreign *3/2 = 49; city receives half in 1/16 units.
    assert_eq!(trade_val, 49 * 8);

    let city_income = calc_city_resources(
        &rules,
        Some(&CityResourceInputs {
            city_wealth_field: i32::from(trade_val),
            ..Default::default()
        }),
    );
    let gather = calc_gather(
        &rules,
        &GatherInputs {
            object_income: city_income,
            ..Default::default()
        },
    );
    let mut econ = LeaderEcon::new();
    econ.gross = gather.gross;
    econ.commerce_cap = [1_000; 6];
    for _ in 0..450 {
        do_gather(&rules, &mut econ, &DoGatherContext::default());
    }
    assert_eq!(econ.stockpile[RES_WEALTH], 24);
    assert_eq!(
        econ.accumulator[RES_WEALTH], 3_600,
        "the route's remaining half-wealth survives in the real accumulator"
    );

    let endpoints =
        end_caravan_route(&mut pools, route, &mut source_links, &mut destination_links).unwrap();
    assert_eq!(endpoints, [endpoint(3, 0), endpoint(7, 1)]);
    assert!(source_links.as_slice().is_empty());
    assert!(destination_links.as_slice().is_empty());
    assert_eq!(pools.record(route).unwrap().flags, CARAVAN_ACTIVE);
    assert!(recompute_city_caravan_income_from_state(
        &rules,
        40,
        0,
        &CaravanIncomeGates::default(),
        &pools,
        &source_links,
        resolved,
        &mut trade_val,
    )
    .unwrap());
    assert_eq!(trade_val, 0);

    pools.close_caravan(route).unwrap();
    assert_eq!(pools.pool(0).unwrap().high_water(), 0);
    assert_eq!(pools.record(route), Err(CaravanRouteError::InvalidSlot));
}

#[test]
fn rejected_second_establishment_does_not_duplicate_city_links() {
    let mut pools = CaravanPools::default();
    let route = pools.init_caravan(0, 42).unwrap();
    let mut first = CaravanCityLinks::default();
    let mut second = CaravanCityLinks::default();
    establish_caravan_route(
        &mut pools,
        route,
        endpoint(3, 0),
        endpoint(7, 1),
        &mut first,
        &mut second,
    )
    .unwrap();
    assert_eq!(
        establish_caravan_route(
            &mut pools,
            route,
            endpoint(7, 1),
            endpoint(3, 0),
            &mut first,
            &mut second,
        ),
        Err(CaravanRouteError::AlreadyEstablished)
    );
    assert_eq!(first.as_slice(), &[route]);
    assert_eq!(second.as_slice(), &[route]);
}
