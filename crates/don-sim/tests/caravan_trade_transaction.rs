use don_sim::systems::economy::{
    award_new_caravan_contact, calc_city_resources, calc_gather, caravan_distance_class,
    caravan_route_distance_class, caravan_trade_value, city_caravan_trade_value, do_gather,
    recompute_city_caravan_income, CaravanIncomeGates, CaravanTradeCity, CaravanTradeRoute,
    CityResourceInputs, DoGatherContext, EconRules, GatherInputs, LeaderEcon, RES_WEALTH,
    TRADE_FORBIDDEN_CITY_TYPE, TRADE_METROPOLIS_TYPE, TRADE_TOWN_TYPE,
};

fn city(owner: i32, center_type: i32, num_buildings: i32, x: i32) -> CaravanTradeCity {
    CaravanTradeCity {
        live: true,
        owner,
        center_type,
        num_buildings,
        x,
        y: 0,
    }
}

#[test]
fn city_value_and_map_distance_buckets_are_the_retail_integer_rules() {
    assert_eq!(city_caravan_trade_value(0x19e, 8), 8);
    assert_eq!(city_caravan_trade_value(TRADE_TOWN_TYPE, 8), 10);
    assert_eq!(city_caravan_trade_value(TRADE_METROPOLIS_TYPE, 8), 12);
    assert_eq!(city_caravan_trade_value(TRADE_FORBIDDEN_CITY_TYPE, 8), 12);

    // Width 40 splits at 10, 20, and 32. Every comparison is strict.
    assert_eq!(caravan_distance_class(40, 9), 0);
    assert_eq!(caravan_distance_class(40, 10), 1);
    assert_eq!(caravan_distance_class(40, 19), 1);
    assert_eq!(caravan_distance_class(40, 20), 2);
    assert_eq!(caravan_distance_class(40, 31), 2);
    assert_eq!(caravan_distance_class(40, 32), 3);

    // Raw Coord positions are converted to WCoord (768 Coord per cell) before distance.
    assert_eq!(caravan_route_distance_class(40, 0, 0, 9 * 768, 0), 0);
    assert_eq!(caravan_route_distance_class(40, 0, 0, 10 * 768, 0), 1);
}

#[test]
fn foreign_distance_indian_and_spice_terms_truncate_in_retail_order() {
    let rules = EconRules::shipped();
    let receiver = city(0, TRADE_TOWN_TYPE, 8, 0); // 8 + 2 = 10
    let partner = city(1, TRADE_METROPOLIS_TYPE, 11, 10 * 768); // 11 + 4 = 15

    // class 1: (10 + 15) * 4/3 = 33; foreign: *3/2 = 49;
    // Indian: *115/100 = 56; spice: *120/100 = 67.
    assert_eq!(
        caravan_trade_value(
            &rules,
            40,
            &receiver,
            &partner,
            &CaravanIncomeGates {
                indian: true,
                spice: true,
            },
        ),
        67
    );
    assert_eq!(
        caravan_trade_value(
            &rules,
            40,
            &receiver,
            &partner,
            &CaravanIncomeGates::default(),
        ),
        49
    );
}

#[test]
fn city_recompute_skips_stale_routes_and_writes_wrapping_sixteenth_income() {
    let rules = EconRules::shipped();
    let good = CaravanTradeRoute {
        linked: true,
        first: city(0, TRADE_TOWN_TYPE, 8, 0),
        second: city(1, TRADE_METROPOLIS_TYPE, 11, 10 * 768),
    };
    let mut incomplete = good;
    incomplete.linked = false;
    let mut stale = good;
    stale.second.live = false;

    let mut trade_val = 123;
    let changed = recompute_city_caravan_income(
        &rules,
        40,
        0,
        &CaravanIncomeGates {
            indian: true,
            spice: true,
        },
        &[incomplete, good, stale],
        &mut trade_val,
    );
    assert!(changed);
    assert_eq!(
        trade_val,
        67 * 8,
        "City::compute_trade stores half in 1/16 units"
    );

    assert!(!recompute_city_caravan_income(
        &rules,
        40,
        0,
        &CaravanIncomeGates {
            indian: true,
            spice: true,
        },
        &[good],
        &mut trade_val,
    ));
}

#[test]
fn route_income_flows_through_city_gather_and_into_the_real_stockpile() {
    let rules = EconRules::shipped();
    let route = CaravanTradeRoute {
        linked: true,
        first: city(0, TRADE_TOWN_TYPE, 8, 0),
        second: city(0, TRADE_METROPOLIS_TYPE, 11, 10 * 768),
    };
    let mut trade_val = 0;
    recompute_city_caravan_income(
        &rules,
        40,
        0,
        &CaravanIncomeGates::default(),
        &[route],
        &mut trade_val,
    );
    // Same owner, class 1: (10 + 15) * 4/3 = 33; city receives half = 16.5 wealth.
    assert_eq!(trade_val, 33 * 8);

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
    assert_eq!(econ.stockpile[RES_WEALTH], 16);
    assert_eq!(
        econ.accumulator[RES_WEALTH], 3_600,
        "the remaining half is preserved"
    );
}

#[test]
fn first_contact_award_mutates_wealth_once_and_doubles_for_foreign_city() {
    let mut econ = LeaderEcon::new();
    econ.age = 2;
    let mut contacts = 0u32;

    assert_eq!(
        award_new_caravan_contact(&mut contacts, &mut econ, 0, 0, 5),
        Some(30)
    );
    assert_eq!(econ.stockpile[RES_WEALTH], 30);
    assert_eq!(
        award_new_caravan_contact(&mut contacts, &mut econ, 0, 0, 5),
        None,
        "the source-city bit makes repeat arrivals a no-op"
    );
    assert_eq!(
        award_new_caravan_contact(&mut contacts, &mut econ, 0, 1, 6),
        Some(60)
    );
    assert_eq!(econ.stockpile[RES_WEALTH], 90);
}
