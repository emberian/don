// SPDX-License-Identifier: GPL-3.0-or-later

use don_ai::arena::bots::ai::Ai;
use don_ai::arena::bots::Bot;
use don_ai::arena::gather_upgrades::{
    CARPENTRY_TYPE, CHEMISTRY_TYPE, GRANARY_TYPE, LUMBER_MILL_TYPE, MATHEMATICS_TYPE, SMELTER_TYPE,
};
use don_ai::arena::map::MapParams;
use don_ai::arena::match_run::{load_world, MatchConfig};
use don_ai::arena::obs::Obs;
use don_ai::arena::world::{ConstructionMode, Job, World, FPS};
use don_ai::arena::{Cmd, EntId};
use don_ai::OrderResult;
use don_sim::order::OrderIndex;
use don_sim::systems::combat::RANGE_UNITS_PER_TILE;

fn world(seed: u32) -> World {
    let cfg = MatchConfig {
        map: MapParams {
            seed,
            ..MapParams::default()
        },
        arena: don_ai::arena::world::ArenaParams {
            construction_mode: ConstructionMode::ResearchModel,
            ..Default::default()
        },
        ..MatchConfig::default()
    };
    load_world(&cfg).expect("shipped Arena data is available")
}

fn legal_site_near_builder(world: &World, builder: EntId, type_id: i32) -> (i32, i32) {
    let (cx, cy) = world.ent(builder).expect("founder is live").tile();
    for radius in 1_i32..=18 {
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                if dx.abs().max(dy.abs()) != radius {
                    continue;
                }
                let (tx, ty) = (cx + dx, cy + dy);
                if world.placement_ok(0, type_id, tx, ty).is_ok() {
                    return (tx, ty);
                }
            }
        }
    }
    panic!("generated start has no legal site for type {type_id}")
}

fn place_enhancer(world: &mut World, type_id: i32) -> (EntId, EntId) {
    world.players[0].techs.insert(world.ids.classical_age);
    world.players[0].techs.insert(MATHEMATICS_TYPE);
    world.players[0].techs.insert(CHEMISTRY_TYPE);
    world.players[0].stock = [10_000; 6];
    let founder = world
        .own_ents(0)
        .find(|ent| ent.type_id == world.ids.citizen && ent.job == Job::Idle)
        .map(|ent| ent.id)
        .expect("starting town has an idle founder");
    let (tx, ty) = legal_site_near_builder(world, founder, type_id);
    let site = match world.submit(
        0,
        Cmd::Build {
            worker: founder,
            type_id,
            tx,
            ty,
        },
    ) {
        OrderResult::Ok(raw) => EntId(raw as u32),
        other => panic!("legal paid enhancer command was not accepted: {other:?}"),
    };
    (founder, site)
}

fn put_founder_at(world: &mut World, founder: EntId, x: i32, y: i32) {
    let index = founder.index().expect("founder has a dense Arena handle");
    world.ents[index].x = x;
    world.ents[index].y = y;
    let motion = world.ents[index]
        .motion
        .as_mut()
        .expect("citizen has UnitWork");
    motion.body.x = x;
    motion.body.y = y;
    for guy in world.ents[index].guys.guys.iter_mut().flatten() {
        guy.x = x;
        guy.y = y;
        guy.des_x = x;
        guy.des_y = y;
    }
}

#[test]
fn five_by_five_founder_walks_to_an_uncovered_approach_and_completes_naturally() {
    for type_id in [GRANARY_TYPE, LUMBER_MILL_TYPE, SMELTER_TYPE] {
        let mut world = world(0x5eed_0001 ^ type_id as u32);
        let (founder, site) = place_enhancer(&mut world, type_id);
        let start = world.ent(founder).expect("founder is live").tile();
        let site_tile = world.ent(site).expect("site is live").tile();
        let (x_size, y_size) = world
            .types
            .get(type_id)
            .map(|ty| (ty.x_size, ty.y_size))
            .expect("validated enhancer row");
        assert_eq!((x_size, y_size), (5, 5));

        let motion = world
            .ent(founder)
            .and_then(|ent| ent.motion.as_ref())
            .expect("citizen has UnitWork");
        assert_eq!(
            motion
                .orders
                .iter()
                .map(|order| order.kind)
                .collect::<Vec<_>>(),
            vec![OrderIndex::MoveTo, OrderIndex::BuildAt],
            "the shipped swarm shape is MOVE_TO followed by retained BUILD_AT"
        );
        let build_at = motion.orders.iter().nth(1).expect("BUILD_AT tail");
        let site_identity = world.ent(site).unwrap();
        assert_eq!(
            (build_at.target_who, build_at.target_o, build_at.target_uid,),
            (
                i32::from(site_identity.who),
                i32::from(site_identity.object_o),
                site_identity.object_uid,
            )
        );

        let mut first_progress = None;
        for _ in 0..4_000 {
            world.step();
            let building = world.ent(site).expect("paid enhancer site remains live");
            if building.build_left < 1_000 && first_progress.is_none() {
                first_progress = Some((world.frame, world.ent(founder).unwrap().tile()));
            }
            if building.complete {
                break;
            }
        }

        let (progress_frame, approach) = first_progress.expect("founder never reached the site");
        let half = x_size / 2;
        assert!(
            (approach.0 - site_tile.0).abs() > half
                || (approach.1 - site_tile.1).abs() > half,
            "the founder must not stand inside the non-Farm footprint: {approach:?} vs {site_tile:?}"
        );
        assert_ne!(
            approach, start,
            "the founder must reach the site by movement"
        );
        assert!(
            progress_frame > 0,
            "construction cannot complete in the command frame"
        );
        assert!(world.ent(site).unwrap().complete);
        assert_eq!(world.ent(site).unwrap().build_left, 0);
    }
}

#[test]
fn covered_builder_reswarm_requeues_movement_and_credits_no_construction() {
    let mut world = world(0x5eed_0050);
    let (founder, site) = place_enhancer(&mut world, GRANARY_TYPE);
    let (site_x, site_y) = {
        let site = world.ent(site).expect("site is live");
        (site.x, site.y)
    };
    put_founder_at(&mut world, founder, site_x, site_y);
    {
        let motion = world.ents[founder.index().unwrap()]
            .motion
            .as_mut()
            .unwrap();
        let move_order = motion.orders.front_mut().expect("swarm MOVE_TO prefix");
        assert_eq!(move_order.kind, OrderIndex::MoveTo);
        move_order.x = site_x;
        move_order.y = site_y;
        move_order.dest_x = site_x;
        move_order.dest_y = site_y;
        move_order.tolerance = don_sim::systems::movement::UCELL;
        motion.body.x = site_x;
        motion.body.y = site_y;
    }
    let before = world.ent(site).unwrap().build.as_ref().unwrap().job_counter;

    world.step();

    let building = world.ent(site).expect("reswarm retains the paid site");
    assert_eq!(building.build.as_ref().unwrap().job_counter, before);
    assert!(!building.complete);
    assert_eq!(building.build_left, 1_000);
    let motion = world.ent(founder).unwrap().motion.as_ref().unwrap();
    assert_eq!(
        motion
            .orders
            .iter()
            .map(|order| order.kind)
            .collect::<Vec<_>>(),
        vec![OrderIndex::MoveTo, OrderIndex::BuildAt],
        "reswarm must restore movement ahead of the same construction identity"
    );
    let approach = motion.orders.front().unwrap();
    assert_ne!((approach.x, approach.y), (site_x, site_y));
}

#[test]
fn natural_ai_completes_enhancers_and_unlocks_producer_gated_research_across_seeds_and_seats() {
    let mut diagnostics = Vec::new();
    let mut completed_runs = 0;
    let mut producer_research_runs = 0;
    let mut completed_types = [false; 3];
    let mut completed_seats = [false; 2];
    let mut completed_seeds = [false; 2];
    let mut full_trio_runs = 0;
    let mut chemistry_runs = 0;
    let mut granary_lumber_runs = 0;
    for seed_index in 0_u32..2 {
        let seed = 0x5eed_0001_u32.wrapping_add(seed_index.wrapping_mul(0x9e37_79b9));
        for seat in 0..2 {
            let mut world = world(seed);
            let mut ai = Ai::default();
            let mut commands = Vec::new();
            while world.frame < 30 * 60 * FPS && world.players[seat].alive {
                if world.frame % ai.decide_period().max(1)
                    == seat as i64 % ai.decide_period().max(1)
                {
                    commands.clear();
                    ai.act(&Obs::of(&world, seat), &mut commands);
                    for command in commands.drain(..) {
                        let _ = world.submit(seat as u8, command);
                    }
                }
                world.step();
            }

            let completed = [GRANARY_TYPE, LUMBER_MILL_TYPE, SMELTER_TYPE]
                .map(|type_id| world.count_type(seat, type_id, false));
            let held = [CARPENTRY_TYPE, CHEMISTRY_TYPE]
                .map(|type_id| world.players[seat].techs.contains(&type_id));
            let sites = world
                .own_ents(seat)
                .filter(|ent| matches!(ent.type_id, GRANARY_TYPE | LUMBER_MILL_TYPE | SMELTER_TYPE))
                .map(|ent| {
                    (
                        ent.type_id,
                        ent.complete,
                        ent.build_left,
                        ent.tile(),
                        ent.job,
                    )
                })
                .collect::<Vec<_>>();
            let relevant_buildings = world
                .own_ents(seat)
                .filter(|ent| ent.building)
                .map(|ent| (ent.type_id, ent.complete, ent.build_left, ent.job))
                .collect::<Vec<_>>();
            diagnostics.push(format!(
                "seed={seed:#x} seat={seat} completed={completed:?} held={held:?} sites={sites:?} stock={:?} techs={:?} buildings={relevant_buildings:?}",
                world.players[seat].stock,
                world.players[seat].techs,
            ));
            if completed.iter().any(|&count| count > 0) {
                completed_runs += 1;
                completed_seats[seat] = true;
                completed_seeds[seed_index as usize] = true;
            }
            for (index, count) in completed.into_iter().enumerate() {
                completed_types[index] |= count > 0;
            }
            if completed[0] > 0 && completed[1] > 0 {
                granary_lumber_runs += 1;
            }
            if completed.into_iter().all(|count| count == 1) {
                full_trio_runs += 1;
            }
            chemistry_runs += usize::from(held[1]);
            producer_research_runs += usize::from(
                held[0]
                    || world.players[seat]
                        .techs
                        .contains(&don_ai::arena::gather_upgrades::AGRICULTURE_TYPE),
            );
        }
    }
    eprintln!("natural enhancer cohort: {diagnostics:#?}");
    assert!(
        completed_runs >= 2 && completed_seats.into_iter().all(|value| value),
        "natural policy did not complete enhancers in both seats: {diagnostics:#?}"
    );
    assert!(
        completed_seeds.into_iter().all(|value| value),
        "natural policy did not complete an enhancer on every fixed seed: {diagnostics:#?}"
    );
    assert!(
        granary_lumber_runs == 4,
        "Granary and Lumber Mill did not both complete in every run: {diagnostics:#?}"
    );
    assert_eq!(
        full_trio_runs, 4,
        "every natural run must complete exactly one of each base enhancer: {diagnostics:#?}"
    );
    assert!(completed_types.into_iter().all(|value| value));
    assert_eq!(
        chemistry_runs, 4,
        "natural renewable economy did not complete Chemistry in every run: {diagnostics:#?}"
    );
    assert!(
        producer_research_runs >= 1,
        "completed enhancers did not unlock their paid producer-gated research: {diagnostics:#?}"
    );
}

#[test]
fn construction_approach_is_stable_in_world_units() {
    let mut world = world(0x5eed_0049);
    let (founder, site) = place_enhancer(&mut world, GRANARY_TYPE);
    let motion = world.ent(founder).unwrap().motion.as_ref().unwrap();
    let approach = motion.orders.front().expect("queued MOVE_TO");
    let target = world.ent(site).unwrap();
    assert_eq!(approach.kind, OrderIndex::MoveTo);
    assert_eq!(
        approach.x.rem_euclid(RANGE_UNITS_PER_TILE),
        RANGE_UNITS_PER_TILE / 2
    );
    assert_eq!(
        approach.y.rem_euclid(RANGE_UNITS_PER_TILE),
        RANGE_UNITS_PER_TILE / 2
    );
    assert_ne!((approach.x, approach.y), (target.x, target.y));
}
