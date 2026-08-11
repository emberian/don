use don_ai::arena::bots::ai::Ai;
use don_ai::arena::bots::Bot;
use don_ai::arena::gather_upgrades::{
    self, CompletedBaseEnhancers, GatherEnhancerLevels, GatherUpgradeSourceError, AGRICULTURE_TYPE,
    CARPENTRY_TYPE, CHEMISTRY_TYPE, COLD_CASTING_TYPE, CROP_ROTATION_TYPE, FOOD_INDUSTRY_TYPE,
    GATHER_RESEARCH_TYPES, GRANARY_TYPE, LOGGING_INDUSTRY_TYPE, LUMBER_MILL_TYPE, MATHEMATICS_TYPE,
    METAL_ALLOYS_TYPE, PAPERMILL_TYPE, SMELTER_TYPE, STEEL_TYPE,
};
use don_ai::arena::map::MapParams;
use don_ai::arena::match_run::{load_world, MatchConfig};
use don_ai::arena::obs::Obs;
use don_ai::arena::world::{Job, PlaceErr, World, FPS};
use don_ai::arena::{Cmd, EntId};
use don_ai::orders::OrderResult;
use don_sim::systems::economy::{RES_FOOD, RES_METAL, RES_TIMBER, RES_WEALTH};
use don_sim::systems::production;

fn world(seed: u32) -> World {
    load_world(&MatchConfig {
        map: MapParams {
            seed,
            ..MapParams::default()
        },
        logging: false,
        ..MatchConfig::default()
    })
    .expect("local shipped data loads")
}

fn legal_site(world: &World, pi: usize, type_id: i32) -> (i32, i32) {
    for ty in 0..world.map.h {
        for tx in 0..world.map.w {
            if world.placement_ok(pi, type_id, tx, ty).is_ok() {
                return (tx, ty);
            }
        }
    }
    panic!("generated map has no legal site for type {type_id}");
}

fn legal_site_near(
    world: &World,
    pi: usize,
    type_id: i32,
    centre: (i32, i32),
) -> Option<(i32, i32)> {
    let mut sites: Vec<(i32, i32)> = (0..world.map.h)
        .flat_map(|ty| (0..world.map.w).map(move |tx| (tx, ty)))
        .collect();
    sites.sort_by_key(|&(tx, ty)| (tx - centre.0).abs().max((ty - centre.1).abs()));
    sites
        .into_iter()
        .find(|&(tx, ty)| world.placement_ok(pi, type_id, tx, ty).is_ok())
}

fn mark_complete(world: &mut World, site: EntId, founder: EntId) {
    let ent = &mut world.ents[site.index().expect("site handle")];
    ent.complete = true;
    ent.build_left = 0;
    ent.build_order = None;
    let build = ent.build.as_mut().expect("building has BuildData");
    build.flags |= production::flag::STARTED | production::flag::ACTIVE;
    build.build_masks |= 0x1000;

    let founder = &mut world.ents[founder.index().expect("founder handle")];
    founder.job = Job::Idle;
    founder.assigned_to = EntId::NONE;
    founder.build_order = None;
    founder
        .motion
        .as_mut()
        .expect("founding Citizen keeps motion")
        .orders
        .clear();
}

fn install_completed(world: &mut World, pi: usize, type_id: i32) -> EntId {
    let preq = world.types.get(type_id).expect("source row").preq.clone();
    world.players[pi].techs.extend(preq);
    if type_id == world.ids.small_city {
        world.players[pi].techs.insert(world.ids.city_state);
    }
    world.players[pi].stock = [20_000; 6];
    let founder = world
        .own_ents(pi)
        .find(|ent| ent.type_id == world.ids.citizen && ent.job == Job::Idle)
        .map(|ent| ent.id)
        .expect("idle founding Citizen");
    let (tx, ty) = legal_site(world, pi, type_id);
    assert!(matches!(
        world.submit(
            pi as u8,
            Cmd::Build {
                worker: founder,
                type_id,
                tx,
                ty,
            },
        ),
        OrderResult::Ok(_)
    ));
    let site = world
        .own_ents(pi)
        .filter(|ent| ent.type_id == type_id)
        .map(|ent| ent.id)
        .max()
        .expect("accepted Build installed a site");
    mark_complete(world, site, founder);
    site
}

#[test]
fn shipped_rows_costs_and_exact_base_arithmetic_fail_closed_on_mutation() {
    let mut world = world(0x5eed_0001);
    let sources = gather_upgrades::validate_shipped_sources(|type_id| world.types.get(type_id))
        .expect("installed live rows match the shipped tranche");
    assert_eq!(sources.granary, GRANARY_TYPE);
    assert_eq!(sources.lumber_mill, LUMBER_MILL_TYPE);
    assert_eq!(sources.smelter, SMELTER_TYPE);
    assert_eq!(sources.mathematics, MATHEMATICS_TYPE);
    assert_eq!(sources.chemistry, CHEMISTRY_TYPE);
    assert_eq!(sources.research, GATHER_RESEARCH_TYPES);

    assert_eq!(
        world.types.get(GRANARY_TYPE).unwrap().cost,
        [0, 60, 40, 0, 0, 0]
    );
    assert_eq!(
        world.types.get(LUMBER_MILL_TYPE).unwrap().cost,
        [60, 0, 0, 0, 40, 0]
    );
    assert_eq!(
        world.types.get(SMELTER_TYPE).unwrap().cost,
        [0, 70, 50, 0, 0, 0]
    );
    for type_id in [GRANARY_TYPE, LUMBER_MILL_TYPE, SMELTER_TYPE] {
        let row = world.types.get(type_id).unwrap();
        assert_eq!(row.job_time, 1000);
        assert_eq!((row.x_size, row.y_size), (5, 5));
        assert_eq!(row.build_flags, 0x0800_0201);
    }
    for (type_id, name, job_time, cost, preq, where_) in [
        (
            CARPENTRY_TYPE,
            "Carpentry",
            300,
            [150, 0, 0, 0, 150, 0],
            &[CHEMISTRY_TYPE][..],
            LUMBER_MILL_TYPE,
        ),
        (
            LOGGING_INDUSTRY_TYPE,
            "Logging Industry",
            350,
            [250, 0, 0, 0, 250, 0],
            &[554, CARPENTRY_TYPE][..],
            LUMBER_MILL_TYPE,
        ),
        (
            PAPERMILL_TYPE,
            "Papermill",
            450,
            [450, 0, 0, 0, 450, 0],
            &[556, LOGGING_INDUSTRY_TYPE][..],
            LUMBER_MILL_TYPE,
        ),
        (
            AGRICULTURE_TYPE,
            "Agriculture",
            300,
            [0, 150, 0, 0, 150, 0],
            &[CHEMISTRY_TYPE][..],
            GRANARY_TYPE,
        ),
        (
            CROP_ROTATION_TYPE,
            "Crop Rotation",
            350,
            [0, 250, 0, 0, 250, 0],
            &[554, AGRICULTURE_TYPE][..],
            GRANARY_TYPE,
        ),
        (
            FOOD_INDUSTRY_TYPE,
            "Food Industry",
            450,
            [0, 450, 0, 0, 450, 0],
            &[556, CROP_ROTATION_TYPE][..],
            GRANARY_TYPE,
        ),
        (
            METAL_ALLOYS_TYPE,
            "Metal Alloys",
            350,
            [250, 250, 0, 0, 0, 0],
            &[554][..],
            SMELTER_TYPE,
        ),
        (
            COLD_CASTING_TYPE,
            "Cold Casting",
            400,
            [350, 350, 0, 0, 0, 0],
            &[555, METAL_ALLOYS_TYPE][..],
            SMELTER_TYPE,
        ),
        (
            STEEL_TYPE,
            "Steel",
            450,
            [450, 450, 0, 0, 0, 0],
            &[556, COLD_CASTING_TYPE][..],
            SMELTER_TYPE,
        ),
    ] {
        let row = world.types.get(type_id).unwrap();
        assert_eq!(row.name, name);
        assert_eq!(row.job_time, job_time);
        assert_eq!(row.cost, cost);
        assert_eq!(row.preq, preq);
        assert_eq!(row.where_, where_);
    }

    let rules_path = don_ai::rules::default_data_dir().join("rules.xml");
    let rules = std::fs::read_to_string(rules_path).expect("shipped rules.xml");
    gather_upgrades::validate_shipped_rule_text(&rules)
        .expect("shipped BonusType and nation descriptors match");
    let mutated_rules = rules.replacen("preq0=\"Carpentry\"", "preq0=\"Agriculture\"", 1);
    assert_eq!(
        gather_upgrades::validate_shipped_rule_text(&mutated_rules),
        Err(GatherUpgradeSourceError::RulesMismatch(
            "BonusType prerequisite"
        ))
    );
    let mutated_nation = rules.replacen(
        "GREEK_RESEARCH_COST value=\"10% cost reduction\"",
        "GREEK_RESEARCH_COST value=\"9% cost reduction\"",
        1,
    );
    assert_eq!(
        gather_upgrades::validate_shipped_rule_text(&mutated_nation),
        Err(GatherUpgradeSourceError::RulesMismatch(
            "nation-power descriptor"
        ))
    );

    let all = CompletedBaseEnhancers {
        granary: true,
        lumber_mill: true,
        smelter: true,
    };
    assert_eq!(gather_upgrades::enhancer_percent(all, RES_FOOD), 120);
    assert_eq!(gather_upgrades::enhancer_percent(all, RES_TIMBER), 120);
    assert_eq!(gather_upgrades::enhancer_percent(all, RES_METAL), 150);
    assert_eq!(gather_upgrades::enhancer_percent(all, 2), 100);
    assert_eq!(
        gather_upgrades::enhancer_amount(all, RES_FOOD, 11),
        13,
        "CityData multiplies before truncating the division"
    );
    assert_eq!(gather_upgrades::base_city_tax_gross(7, false, true), 0);
    assert_eq!(gather_upgrades::base_city_tax_gross(7, true, true), 160);

    world.types.rows.get_mut(&GRANARY_TYPE).unwrap().cost[2] += 1;
    assert_eq!(
        gather_upgrades::validate_shipped_sources(|type_id| world.types.get(type_id)),
        Err(GatherUpgradeSourceError::Mismatch {
            type_id: GRANARY_TYPE,
            field: "cost",
        })
    );

    world.types.rows.get_mut(&GRANARY_TYPE).unwrap().cost[2] -= 1;
    world.types.rows.get_mut(&MATHEMATICS_TYPE).unwrap().id = -1;
    assert_eq!(
        gather_upgrades::validate_shipped_sources(|type_id| world.types.get(type_id)),
        Err(GatherUpgradeSourceError::Mismatch {
            type_id: -1,
            field: "type_id",
        })
    );
}

#[test]
fn roman_research_levels_follow_shipped_query_precedence_and_refuse_other_tribes() {
    let all = CompletedBaseEnhancers {
        granary: true,
        lumber_mill: true,
        smelter: true,
    };
    assert_eq!(
        gather_upgrades::roman_enhancer_levels(6, all, |_| false),
        Some(GatherEnhancerLevels {
            granary: 1,
            lumber_mill: 1,
            smelter: 1,
        })
    );
    let first = [AGRICULTURE_TYPE, CARPENTRY_TYPE, METAL_ALLOYS_TYPE];
    let levels = gather_upgrades::roman_enhancer_levels(6, all, |id| first.contains(&id)).unwrap();
    assert_eq!(levels.granary, 2);
    assert_eq!(levels.lumber_mill, 2);
    assert_eq!(levels.smelter, 2);
    let exact = gather_upgrades::researched_enhancers(levels);
    assert_eq!(
        (exact.granary, exact.lumber_mill, exact.smelter),
        (50, 50, 100)
    );

    let highest = [FOOD_INDUSTRY_TYPE, PAPERMILL_TYPE, STEEL_TYPE];
    assert_eq!(
        gather_upgrades::roman_enhancer_levels(6, all, |id| highest.contains(&id)),
        Some(GatherEnhancerLevels {
            granary: 4,
            lumber_mill: 4,
            smelter: 4,
        })
    );
    for tribe in [5, 7, 10] {
        assert_eq!(
            gather_upgrades::roman_enhancer_levels(tribe, all, |_| true),
            None,
            "nation-power research/grant paths stay red for tribe {tribe}"
        );
    }
}

#[test]
fn exact_queue_cost_timer_and_completion_install_the_held_research() {
    let mut world = world(0x5eed_0001);
    let lumber_mill = install_completed(&mut world, 0, LUMBER_MILL_TYPE);
    world.players[0].techs.insert(CHEMISTRY_TYPE);
    world.players[0].stock = [1_000; 6];
    let before = world.players[0].stock;
    assert_eq!(
        world.submit(
            0,
            Cmd::Queue {
                producer: lumber_mill,
                type_id: CARPENTRY_TYPE,
                count: 1,
            },
        ),
        OrderResult::Ok(1)
    );
    assert_eq!(
        world.players[0].stock,
        std::array::from_fn(|resource| {
            before[resource] - world.types.get(CARPENTRY_TYPE).unwrap().cost[resource]
        })
    );
    assert_eq!(world.ent(lumber_mill).unwrap().queue[0].frames_left, 300);
    for _ in 0..299 {
        world.step();
    }
    assert!(!world.players[0].techs.contains(&CARPENTRY_TYPE));
    world.step();
    assert!(world.players[0].techs.contains(&CARPENTRY_TYPE));
    assert!(world.ent(lumber_mill).unwrap().queue.is_empty());
}

#[test]
fn only_a_completed_same_city_market_pays_exact_base_tax() {
    let mut world = world(0x5eed_0001);
    let market_type = world.ids.market;
    let market = install_completed(&mut world, 0, market_type);

    // An incomplete Market contributes nothing and cannot prime the exact subchannel.
    world.ents[market.index().unwrap()].complete = false;
    world.players[0].stock[RES_WEALTH] = 0;
    world.players[0].gathered[RES_WEALTH] = 0;
    for _ in 0..450 {
        world.step();
    }
    assert_eq!(world.players[0].gathered[RES_WEALTH], 0);

    // `10 << 4` gross over the exact `450 << 4` period credits ten wealth in 450
    // frames. No Citizen, Caravan, Merchant or ordinary PEASANT_RATE term participates.
    world.ents[market.index().unwrap()].complete = true;
    for _ in 0..450 {
        world.step();
    }
    assert_eq!(world.players[0].gathered[RES_WEALTH], 10);
    assert_eq!(world.players[0].stock[RES_WEALTH], 10);

    // Same owner and completion are insufficient when the Market is not in a live city
    // census. This also pins that the exact source is city-owned, not a global type count.
    world.ents[market.index().unwrap()].city = EntId::NONE;
    for _ in 0..450 {
        world.step();
    }
    assert_eq!(world.players[0].gathered[RES_WEALTH], 10);
}

#[test]
fn completed_same_city_granary_scales_only_the_model_farm_term() {
    let mut world = world(0x5eed_0001);
    let granary = install_completed(&mut world, 0, GRANARY_TYPE);
    let farm = world
        .own_ents(0)
        .find(|ent| ent.type_id == world.ids.farm)
        .map(|ent| ent.id)
        .expect("shipped start Farm");
    assert_eq!(
        world.ent(granary).unwrap().city,
        world.ent(farm).unwrap().city
    );

    let citizen = world
        .own_ents(0)
        .find(|ent| ent.type_id == world.ids.citizen && ent.job == Job::Idle)
        .map(|ent| ent.id)
        .expect("idle Farm worker");
    assert_eq!(
        world.submit(
            0,
            Cmd::Gather {
                unit: citizen,
                target: farm,
            },
        ),
        OrderResult::Ok(1)
    );
    for _ in 0..2000 {
        if world.ent(farm).unwrap().workers == 1 {
            break;
        }
        world.step();
    }
    assert_eq!(world.ent(farm).unwrap().workers, 1);

    // Mutation-sensitive ordering: 11 * 120 / 100 is 13. CITY_GATHER remains outside
    // the multiplier, so toggling only completion changes one Farm term by exactly two.
    world.types.constants.peasant_rate = 11;
    world.ents[granary.index().unwrap()].complete = false;
    world.players[0].stock[RES_FOOD] = 0;
    world.players[0].acc[RES_FOOD] = 0;
    world.players[0].gathered[RES_FOOD] = 0;
    for _ in 0..450 {
        world.step();
    }
    let without = world.players[0].gathered[RES_FOOD];

    world.ents[granary.index().unwrap()].complete = true;
    world.players[0].stock[RES_FOOD] = 0;
    world.players[0].acc[RES_FOOD] = 0;
    world.players[0].gathered[RES_FOOD] = 0;
    for _ in 0..450 {
        world.step();
    }
    let with = world.players[0].gathered[RES_FOOD];
    assert_eq!(with - without, 2);

    // Agriculture moves the same completed Granary from level 1 (+20) to level 2
    // (+50). It is read dynamically from the owner's completed research, not frozen
    // when either the Farm or Granary was installed.
    world.players[0].techs.insert(AGRICULTURE_TYPE);
    world.players[0].stock[RES_FOOD] = 0;
    world.players[0].acc[RES_FOOD] = 0;
    world.players[0].gathered[RES_FOOD] = 0;
    for _ in 0..450 {
        world.step();
    }
    let researched = world.players[0].gathered[RES_FOOD];
    assert_eq!(researched - without, 5);
}

#[test]
fn conservative_city_projection_refuses_duplicate_and_admits_sibling_city() {
    let mut world = world(0x5eed_0001);
    let capital = world
        .own_ents(0)
        .find(|ent| ent.type_id == world.ids.small_city)
        .map(|ent| (ent.id, ent.tile()))
        .expect("starting capital");
    let granary = install_completed(&mut world, 0, GRANARY_TYPE);
    assert_eq!(world.ent(granary).unwrap().city, capital.0);

    assert!((0..world.map.h).any(|ty| {
        (0..world.map.w)
            .any(|tx| world.placement_ok(0, GRANARY_TYPE, tx, ty) == Err(PlaceErr::DuplicateInCity))
    }));

    let small_city = world.ids.small_city;
    let sibling = install_completed(&mut world, 0, small_city);
    let sibling_tile = world.ent(sibling).unwrap().tile();
    assert_ne!(sibling, capital.0);
    let sibling_site = legal_site_near(&world, 0, GRANARY_TYPE, sibling_tile)
        .expect("a second complete city admits its own Granary projection");
    assert!(
        (sibling_site.0 - sibling_tile.0)
            .abs()
            .max((sibling_site.1 - sibling_tile.1).abs())
            < (sibling_site.0 - capital.1 .0)
                .abs()
                .max((sibling_site.1 - capital.1 .1).abs()),
        "admitted site belongs to the sibling's nearer catchment"
    );
}

fn policy_commands(mut world: World, mutate_granary_cost: bool) -> Vec<Cmd> {
    let university = world.ids.university;
    install_completed(&mut world, 0, university);
    world.players[0].techs.insert(world.ids.city_state);
    world.players[0].techs.insert(world.ids.classical_age);
    world.players[0].techs.insert(MATHEMATICS_TYPE);
    world.players[0].techs.insert(CHEMISTRY_TYPE);
    world.players[0].stock = [1_000; 6];
    if mutate_granary_cost {
        world.types.rows.get_mut(&GRANARY_TYPE).unwrap().cost = [2_000; 6];
    }
    let mut ai = Ai::default();
    let mut commands = Vec::new();
    ai.act(&Obs::of(&world, 0), &mut commands);
    commands
}

#[test]
fn ai_policy_reads_affordability_and_naturally_reaches_chemistry_across_fixed_seeds_and_seats() {
    let baseline = policy_commands(world(0x5eed_0001), false);
    assert!(baseline
        .iter()
        .any(|command| matches!(command, Cmd::Build { type_id, .. } if *type_id == GRANARY_TYPE)));
    let mutated = policy_commands(world(0x5eed_0001), true);
    assert!(!mutated
        .iter()
        .any(|command| matches!(command, Cmd::Build { type_id, .. } if *type_id == GRANARY_TYPE)));

    let mut multi_seed_reached_upgrade = false;
    let mut multi_seed_reached_chemistry = false;
    let mut diagnostics = Vec::new();
    for seed_index in 0_u32..2 {
        let seed = 0x5EED_0001u32.wrapping_add(seed_index.wrapping_mul(0x9E37_79B9));
        for seat in 0..2 {
            let mut world = world(seed);
            let mut ai = Ai::default();
            let mut commands = Vec::new();
            let mut accepted = [false; 3];
            while world.frame < 23 * 60 * FPS && world.players[seat].alive {
                if world.frame % ai.decide_period().max(1)
                    == seat as i64 % ai.decide_period().max(1)
                {
                    commands.clear();
                    ai.act(&Obs::of(&world, seat), &mut commands);
                    for command in commands.drain(..) {
                        let type_id = match command {
                            Cmd::Build { type_id, .. } | Cmd::Queue { type_id, .. } => {
                                Some(type_id)
                            }
                            _ => None,
                        };
                        if matches!(world.submit(seat as u8, command), OrderResult::Ok(_)) {
                            match type_id {
                                Some(GRANARY_TYPE) => accepted[0] = true,
                                Some(LUMBER_MILL_TYPE) => accepted[1] = true,
                                Some(SMELTER_TYPE) => accepted[2] = true,
                                _ => {}
                            }
                        }
                    }
                }
                world.step();
            }
            if accepted.iter().any(|&value| value) {
                multi_seed_reached_upgrade = true;
                assert!(world.players[seat].techs.contains(&MATHEMATICS_TYPE));
                assert!(world
                    .own_ents(seat)
                    .any(|ent| ent.type_id == world.ids.market && ent.complete));
            }
            multi_seed_reached_chemistry |= world.players[seat].techs.contains(&CHEMISTRY_TYPE);
            diagnostics.push(format!(
                "seed={seed_index} seat={seat} accepted={accepted:?} techs={:?} stock={:?} sites={:?}",
                world.players[seat].techs,
                world.players[seat].stock,
                world
                    .own_ents(seat)
                    .filter(|ent| ent.building)
                    .map(|ent| (ent.type_id, ent.complete, ent.build_left))
                    .collect::<Vec<_>>()
            ));
        }
    }
    assert!(
        multi_seed_reached_upgrade,
        "neither fixed seed had a seat reach a gather upgrade from ordinary starting stock: {diagnostics:#?}"
    );
    assert!(
        multi_seed_reached_chemistry,
        "no fixed seed completed Chemistry from ordinary starting stock: {diagnostics:#?}"
    );
}
