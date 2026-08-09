use don_sim::systems::economy::{
    activate_caravan_income, close_caravan_unit_in_city_pool,
    end_caravan_route_in_city_pool_from_order, establish_caravan_route_in_city_pool,
    initialize_city_caravan_links, recompute_city_pool_caravan_income, CaravanEndpoint,
    CaravanIncomeGates, CaravanPools, CaravanTradeCity, EconRules, CARAVAN_EARNING,
    CARAVAN_ESTABLISHED, INITIAL_CITY_CARAVAN_CAPACITY, TRADE_METROPOLIS_TYPE, TRADE_TOWN_TYPE,
    UNIT_HAS_TRADE_ROUTE,
};
use don_sim::systems::tech_cities::{check_cities, CityPool};

fn endpoint(city: i16, owner: i16) -> CaravanEndpoint {
    CaravanEndpoint { city, owner }
}

fn make_live_city(cities: &mut CityPool, ep: CaravanEndpoint, x: i32) {
    let city = &mut cities.slots[ep.owner as usize][ep.city as usize];
    city.city_flags = 1;
    city.city = ep.city;
    city.who = ep.owner as i8;
    city.x = x;
    city.y = 0;
    initialize_city_caravan_links(&mut city.vans);
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
        (1, 8) => Some(CaravanTradeCity {
            live: true,
            owner: 1,
            center_type: TRADE_TOWN_TYPE,
            num_buildings: 6,
            x: 20 * 768,
            y: 0,
        }),
        _ => None,
    }
}

#[test]
fn ordinary_city_route_death_closes_pool_then_uses_order_endpoints_to_restore_checksum() {
    let rules = EconRules::shipped();
    let source = endpoint(3, 0);
    let destination = endpoint(7, 1);
    let mut cities = CityPool::new();
    make_live_city(&mut cities, source, 0);
    make_live_city(&mut cities, destination, 10 * 768);
    let mut leaders = [false; 8];
    leaders[0] = true;
    leaders[1] = true;
    let baseline_city_checksum = check_cities(&cities, &leaders);

    let mut pools = CaravanPools::default();
    let route = pools.init_caravan(0, 42).unwrap();
    establish_caravan_route_in_city_pool(&mut pools, &mut cities, route, source, destination)
        .unwrap();
    assert_eq!(
        cities.slots[0][3].vans.capacity,
        INITIAL_CITY_CARAVAN_CAPACITY
    );
    assert_eq!(cities.slots[0][3].vans.grow, -1);
    assert_eq!(cities.slots[0][3].vans.items[0].cara, route.slot);
    assert_eq!(cities.slots[0][3].vans.items[0].who, route.owner);

    activate_caravan_income(&mut pools, route).unwrap();
    recompute_city_pool_caravan_income(
        &rules,
        40,
        source,
        &CaravanIncomeGates::default(),
        &pools,
        &mut cities,
        resolved,
    )
    .unwrap();
    recompute_city_pool_caravan_income(
        &rules,
        40,
        destination,
        &CaravanIncomeGates::default(),
        &pools,
        &mut cities,
        resolved,
    )
    .unwrap();
    assert_eq!(cities.slots[0][3].trade_val, 49 * 8);
    assert_ne!(check_cities(&cities, &leaders), baseline_city_checksum);

    let mut unit_flags = UNIT_HAS_TRADE_ROUTE | 0x40;
    let mut leader_dirty = [false; 8];
    let receipt = close_caravan_unit_in_city_pool(
        &rules,
        40,
        &mut pools,
        &mut cities,
        route,
        Some([source, destination]),
        true,
        &mut unit_flags,
        &mut leader_dirty,
        resolved,
        |_| CaravanIncomeGates::default(),
    )
    .unwrap();
    assert!(receipt.pool_closed);
    let end = receipt.route_end.unwrap();
    assert_eq!(end.endpoints, [source, destination]);
    assert_eq!(end.links_removed, [true, true]);
    assert_eq!(end.income_changed, [true, true]);
    assert_eq!(unit_flags, 0x40);
    assert!(leader_dirty[0]);
    assert!(leader_dirty[1]);

    // This lookup is deliberately allocation-based: close_caravan already shrank the
    // high-water mark to zero before close_orders reached end_trade_route.
    let closed = pools.allocated_record(route).unwrap();
    assert!(!closed.is_active());
    assert!(!closed.is_established());
    assert!(!closed.is_earning());
    assert_eq!(closed.owner, -1);
    assert_eq!(closed.endpoints, [CaravanEndpoint::NONE; 2]);
    assert_eq!(pools.pool(0).unwrap().high_water(), 0);

    for ep in [source, destination] {
        let city = &cities.slots[ep.owner as usize][ep.city as usize];
        assert!(city.vans.items.is_empty());
        assert_eq!(city.vans.capacity, INITIAL_CITY_CARAVAN_CAPACITY);
        assert_eq!(city.trade_val, 0);
    }
    assert_eq!(check_cities(&cities, &leaders), baseline_city_checksum);
}

#[test]
fn death_removes_only_its_exact_link_and_recomputes_the_surviving_route() {
    let rules = EconRules::shipped();
    let source = endpoint(3, 0);
    let first_destination = endpoint(7, 1);
    let second_destination = endpoint(8, 1);
    let mut cities = CityPool::new();
    make_live_city(&mut cities, source, 0);
    make_live_city(&mut cities, first_destination, 10 * 768);
    make_live_city(&mut cities, second_destination, 20 * 768);

    let mut pools = CaravanPools::default();
    let first = pools.init_caravan(0, 42).unwrap();
    let second = pools.init_caravan(0, 43).unwrap();
    establish_caravan_route_in_city_pool(&mut pools, &mut cities, first, source, first_destination)
        .unwrap();
    establish_caravan_route_in_city_pool(
        &mut pools,
        &mut cities,
        second,
        source,
        second_destination,
    )
    .unwrap();
    activate_caravan_income(&mut pools, first).unwrap();
    activate_caravan_income(&mut pools, second).unwrap();
    recompute_city_pool_caravan_income(
        &rules,
        40,
        source,
        &CaravanIncomeGates::default(),
        &pools,
        &mut cities,
        resolved,
    )
    .unwrap();
    let both = cities.slots[0][3].trade_val;

    let mut unit_flags = UNIT_HAS_TRADE_ROUTE;
    let mut leader_dirty = [false; 8];
    close_caravan_unit_in_city_pool(
        &rules,
        40,
        &mut pools,
        &mut cities,
        first,
        Some([source, first_destination]),
        true,
        &mut unit_flags,
        &mut leader_dirty,
        resolved,
        |_| CaravanIncomeGates::default(),
    )
    .unwrap();
    assert_eq!(cities.slots[0][3].vans.items.len(), 1);
    assert_eq!(cities.slots[0][3].vans.items[0].cara, second.slot);
    assert!(cities.slots[1][7].vans.items.is_empty());
    assert_eq!(cities.slots[1][8].vans.items[0].cara, second.slot);
    assert!(cities.slots[0][3].trade_val > 0);
    assert!(cities.slots[0][3].trade_val < both);
    assert!(pools.record(second).unwrap().is_earning());
}

#[test]
fn inactive_simulation_gate_leaves_order_cleanup_for_the_later_close_orders_pass() {
    let rules = EconRules::shipped();
    let source = endpoint(3, 0);
    let destination = endpoint(7, 1);
    let mut cities = CityPool::new();
    make_live_city(&mut cities, source, 0);
    make_live_city(&mut cities, destination, 10 * 768);
    let mut pools = CaravanPools::default();
    let route = pools.init_caravan(0, 42).unwrap();
    establish_caravan_route_in_city_pool(&mut pools, &mut cities, route, source, destination)
        .unwrap();
    activate_caravan_income(&mut pools, route).unwrap();

    let mut unit_flags = UNIT_HAS_TRADE_ROUTE;
    let mut leader_dirty = [false; 8];
    let receipt = close_caravan_unit_in_city_pool(
        &rules,
        40,
        &mut pools,
        &mut cities,
        route,
        Some([source, destination]),
        false,
        &mut unit_flags,
        &mut leader_dirty,
        resolved,
        |_| CaravanIncomeGates::default(),
    )
    .unwrap();
    assert!(receipt.route_end.is_none());
    assert_eq!(unit_flags, UNIT_HAS_TRADE_ROUTE);
    let between = pools.allocated_record(route).unwrap();
    assert_eq!(
        between.flags & (CARAVAN_ESTABLISHED | CARAVAN_EARNING),
        CARAVAN_ESTABLISHED | CARAVAN_EARNING
    );
    assert_eq!(cities.slots[0][3].vans.items.len(), 1);
    assert_eq!(leader_dirty, [false; 8]);

    let mut resolve = resolved;
    let mut gates = |_| CaravanIncomeGates::default();
    let end = end_caravan_route_in_city_pool_from_order(
        &rules,
        40,
        &mut pools,
        &mut cities,
        route,
        [source, destination],
        &mut unit_flags,
        &mut leader_dirty,
        &mut resolve,
        &mut gates,
    )
    .unwrap();
    assert_eq!(end.links_removed, [true, true]);
    assert_eq!(unit_flags, 0);
    assert_eq!(end.income_changed, [false, false]);
    assert_eq!(
        leader_dirty, [false; 8],
        "both never-computed trade caches stayed zero, so retail dirties neither owner"
    );
}
