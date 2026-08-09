use don_sim::systems::economy::{
    activate_caravan_income, collect_city_pool_income, end_caravan_route_in_city_pool_from_order,
    establish_caravan_route_in_city_pool, initialize_city_caravan_links, leader_gather,
    recompute_city_pool_caravan_income_and_dirty, CaravanEndpoint, CaravanIncomeGates,
    CaravanPools, CaravanTradeCity, CityPoolIncomeError, CityPoolIncomeFacts, DoGatherContext,
    EconRules, GatherInputs, LeaderEcon, RES_FOOD, RES_KNOWLEDGE, RES_TIMBER, RES_WEALTH,
    TRADE_TOWN_TYPE, UNIT_HAS_TRADE_ROUTE,
};
use don_sim::systems::tech_cities::CityPool;

fn endpoint(city: i16, owner: i16) -> CaravanEndpoint {
    CaravanEndpoint { city, owner }
}

fn init_city(cities: &mut CityPool, owner: usize, x: i32) -> CaravanEndpoint {
    let city_index = cities.alloc_slot(owner);
    let city = &mut cities.slots[owner][city_index];
    city.city_flags = 1;
    city.city = city_index as i16;
    city.who = owner as i8;
    city.x = x;
    city.y = 0;
    initialize_city_caravan_links(&mut city.vans);
    endpoint(city_index as i16, owner as i16)
}

fn resolve_trade_city(ep: CaravanEndpoint) -> Option<CaravanTradeCity> {
    match (ep.owner, ep.city) {
        (0, 0) => Some(CaravanTradeCity {
            live: true,
            owner: 0,
            center_type: TRADE_TOWN_TYPE,
            num_buildings: 8,
            x: 0,
            y: 0,
        }),
        (1, 0) => Some(CaravanTradeCity {
            live: true,
            owner: 1,
            center_type: TRADE_TOWN_TYPE,
            num_buildings: 8,
            x: 10 * 768,
            y: 0,
        }),
        _ => None,
    }
}

#[test]
fn city_pool_loop_uses_mark_slot_order_active_gate_and_the_checksummed_trade_field() {
    let rules = EconRules::shipped();
    let mut cities = CityPool::new();
    cities.city_mark[0] = 4;
    cities.slots[0][1].city_flags = 1;
    cities.slots[0][1].trade_val = -2;
    cities.slots[0][3].city_flags = 1;
    cities.slots[0][3].trade_val = 5;

    let mut visited = Vec::new();
    let income = collect_city_pool_income(&rules, &cities, 0, |city_index, _city| {
        visited.push(city_index);
        Some(CityPoolIncomeFacts {
            taxes: 10 + city_index as i32,
            literacy: city_index as i32,
            ..Default::default()
        })
    })
    .unwrap();

    assert_eq!(visited, [1, 3]);
    assert_eq!(income[RES_FOOD], 2 * 10 * 16);
    assert_eq!(income[RES_TIMBER], 2 * 10 * 16);
    assert_eq!(income[RES_WEALTH], (-2 + 11 * 16) + (5 + 13 * 16));
    assert_eq!(income[RES_KNOWLEDGE], (1 + 3) * 16);

    cities.city_mark[0] = 21;
    assert_eq!(
        collect_city_pool_income(&rules, &cities, 0, |_, _| Some(Default::default())),
        Err(CityPoolIncomeError::CityMarkOutsidePool)
    );
}

#[test]
fn route_income_dirties_recomposes_on_frame_eight_pays_stockpile_and_dirties_on_end() {
    let rules = EconRules::shipped();
    let mut cities = CityPool::new();
    let source = init_city(&mut cities, 0, 0);
    let destination = init_city(&mut cities, 1, 10 * 768);
    let mut pools = CaravanPools::default();
    let route = pools.init_caravan(0, 42).unwrap();
    establish_caravan_route_in_city_pool(&mut pools, &mut cities, route, source, destination)
        .unwrap();
    activate_caravan_income(&mut pools, route).unwrap();

    let mut dirty = [false; 8];
    assert!(recompute_city_pool_caravan_income_and_dirty(
        &rules,
        40,
        source,
        &CaravanIncomeGates::default(),
        &pools,
        &mut cities,
        &mut dirty,
        resolve_trade_city,
    )
    .unwrap());
    assert!(recompute_city_pool_caravan_income_and_dirty(
        &rules,
        40,
        destination,
        &CaravanIncomeGates::default(),
        &pools,
        &mut cities,
        &mut dirty,
        resolve_trade_city,
    )
    .unwrap());
    assert!(dirty[0]);
    assert!(dirty[1]);

    dirty[0] = false;
    assert!(!recompute_city_pool_caravan_income_and_dirty(
        &rules,
        40,
        source,
        &CaravanIncomeGates::default(),
        &pools,
        &mut cities,
        &mut dirty,
        resolve_trade_city,
    )
    .unwrap());
    assert!(!dirty[0], "an unchanged cache does not dirty the economy");

    let city_income = collect_city_pool_income(&rules, &cities, 0, |_, _| {
        Some(CityPoolIncomeFacts::default())
    })
    .unwrap();
    assert_eq!(
        city_income[RES_WEALTH],
        i32::from(cities.slots[0][0].trade_val)
    );
    assert!(city_income[RES_WEALTH] > 0);

    let mut gather = GatherInputs::default();
    gather.object_income = city_income;
    let mut econ = LeaderEcon::new();
    let mut last_calc_frame = -1;
    let mut owner_dirty = true;
    leader_gather(
        &rules,
        &mut econ,
        7,
        0,
        &mut last_calc_frame,
        &mut owner_dirty,
        &gather,
        &Default::default(),
        &DoGatherContext::default(),
    );
    assert_eq!(last_calc_frame, -1);
    assert!(owner_dirty);
    leader_gather(
        &rules,
        &mut econ,
        8,
        0,
        &mut last_calc_frame,
        &mut owner_dirty,
        &gather,
        &Default::default(),
        &DoGatherContext::default(),
    );
    assert_eq!(last_calc_frame, 8);
    assert!(!owner_dirty);
    assert_eq!(econ.gross[RES_WEALTH], city_income[RES_WEALTH]);
    // Age-0 commerce caps this wealth stream to 70 gross units per frame. Including
    // frame 8, 103 payouts are therefore required to cross the shipped 7,200-unit
    // accumulator period: 103 * 70 = 7,210.
    for frame in 9..=110 {
        leader_gather(
            &rules,
            &mut econ,
            frame,
            0,
            &mut last_calc_frame,
            &mut owner_dirty,
            &gather,
            &Default::default(),
            &DoGatherContext::default(),
        );
    }
    assert!(econ.stockpile[RES_WEALTH] > 0);

    dirty = [false; 8];
    let mut unit_flags = UNIT_HAS_TRADE_ROUTE;
    let mut resolve = resolve_trade_city;
    let mut gates = |_| CaravanIncomeGates::default();
    let ended = end_caravan_route_in_city_pool_from_order(
        &rules,
        40,
        &mut pools,
        &mut cities,
        route,
        [source, destination],
        &mut unit_flags,
        &mut dirty,
        &mut resolve,
        &mut gates,
    )
    .unwrap();
    assert_eq!(ended.income_changed, [true, true]);
    assert!(dirty[0]);
    assert!(dirty[1]);

    gather.object_income = collect_city_pool_income(&rules, &cities, 0, |_, _| {
        Some(CityPoolIncomeFacts::default())
    })
    .unwrap();
    owner_dirty = dirty[0];
    leader_gather(
        &rules,
        &mut econ,
        111,
        0,
        &mut last_calc_frame,
        &mut owner_dirty,
        &gather,
        &Default::default(),
        &DoGatherContext::default(),
    );
    assert_eq!(econ.gross[RES_WEALTH], city_income[RES_WEALTH]);
    leader_gather(
        &rules,
        &mut econ,
        112,
        0,
        &mut last_calc_frame,
        &mut owner_dirty,
        &gather,
        &Default::default(),
        &DoGatherContext::default(),
    );
    assert_eq!(econ.gross[RES_WEALTH], 0);
    assert!(!owner_dirty);
}
