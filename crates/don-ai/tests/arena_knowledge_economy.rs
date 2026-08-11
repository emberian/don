use don_ai::arena::bots::ai::Ai;
use don_ai::arena::bots::Bot;
use don_ai::arena::cmd::{Cmd, EntId};
use don_ai::arena::map::MapParams;
use don_ai::arena::match_run::{load_world, MatchConfig};
use don_ai::arena::obs::Obs;
use don_ai::arena::world::{World, FPS};
use don_ai::orders::OrderResult;
use don_sim::systems::combat::RANGE_UNITS_PER_TILE;
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

fn university_site(world: &World, pi: usize) -> (i32, i32) {
    for ty in 0..world.map.h {
        for tx in 0..world.map.w {
            if world.placement_ok(pi, world.ids.university, tx, ty).is_ok() {
                return (tx, ty);
            }
        }
    }
    panic!("generated map has no legal University site");
}

fn install_completed_university(world: &mut World, pi: usize) -> EntId {
    world.players[pi].techs.insert(world.ids.classical_age);
    world.players[pi].stock = [10_000; 6];
    let worker = world
        .own_ents(pi)
        .find(|ent| ent.type_id == world.ids.citizen)
        .map(|ent| ent.id)
        .expect("starting Citizen");
    let (tx, ty) = university_site(world, pi);
    assert!(matches!(
        world.submit(
            pi as u8,
            Cmd::Build {
                worker,
                type_id: world.ids.university,
                tx,
                ty,
            },
        ),
        OrderResult::Ok(_)
    ));
    let university = world
        .own_ents(pi)
        .find(|ent| ent.type_id == world.ids.university)
        .map(|ent| ent.id)
        .expect("accepted Build installed the site");

    // This test isolates the knowledge lifecycle from Arena's separately declared
    // ResearchModel construction boundary. It starts at the exact activation product the
    // Scholar queue consumes rather than claiming to test University construction parity.
    let site = &mut world.ents[university.index().unwrap()];
    site.complete = true;
    site.build_left = 0;
    site.build_order = None;
    site.job = don_ai::arena::world::Job::Idle;
    let build = site.build.as_mut().expect("University has BuildData");
    build.flags |= production::flag::STARTED | production::flag::ACTIVE;
    build.build_masks |= 0x1000;
    university
}

#[test]
fn exact_university_scholar_lifecycle_caps_contains_and_credits() {
    let mut world = world(0x5eed_0001);
    let university_type = world.types.get(world.ids.university).unwrap().clone();
    let scholar = world.roster[0].id("Scholar").expect("tribe Scholar row");
    let scholar_type = world.types.get(scholar).unwrap().clone();
    assert_eq!(university_type.cost, [0, 60, 30, 0, 0, 0]);
    assert_eq!(university_type.job_time, 420);
    assert_eq!(scholar_type.cost, [0, 0, 30, 0, 0, 0]);
    assert_eq!(scholar_type.job_time, 75);
    assert_eq!(scholar_type.where_, world.ids.university);
    assert_eq!(scholar_type.support, [2, -1]);
    assert_eq!(scholar_type.support_cost, [2, 0]);

    let university = install_completed_university(&mut world, 0);
    assert_eq!(world.ent(university).unwrap().worker_cap, 7);
    assert_eq!(world.production_cost(0, scholar).unwrap()[2], 30);
    let founder = world
        .own_ents(0)
        .find(|ent| matches!(ent.job, don_ai::arena::world::Job::Work { target } if target == university))
        .map(|ent| ent.id)
        .expect("founding Citizen still addresses the completed site");
    world.step();
    assert_eq!(
        world.ent(founder).unwrap().job,
        don_ai::arena::world::Job::Idle
    );

    let wealth_before = world.players[0].stock[2];
    assert_eq!(
        world.submit(
            0,
            Cmd::Queue {
                producer: university,
                type_id: scholar,
                count: 8,
            },
        ),
        OrderResult::Ok(1)
    );
    // Seven pre-purchase costs: 30, 32, 34, 36, 38, 40, 42.
    assert_eq!(wealth_before - world.players[0].stock[2], 252);
    assert_eq!(world.count_type(0, scholar, true), 7);
    assert_eq!(world.production_cost(0, scholar).unwrap()[2], 44);

    for _ in 0..(7 * scholar_type.job_time) {
        world.step();
    }
    let scholars: Vec<EntId> = world
        .own_ents(0)
        .filter(|ent| ent.type_id == scholar)
        .map(|ent| ent.id)
        .collect();
    assert_eq!(scholars.len(), 7);
    assert_eq!(world.contained_scholars(university), 7);
    assert!(scholars
        .iter()
        .copied()
        .all(|id| world.is_contained_scholar(id)));

    let wealth_at_cap = world.players[0].stock[2];
    assert_eq!(
        world.submit(
            0,
            Cmd::Queue {
                producer: university,
                type_id: scholar,
                count: 1,
            },
        ),
        OrderResult::Refused
    );
    assert_eq!(world.players[0].stock[2], wealth_at_cap);

    // A contained Scholar is still a live/population-counted object but never receives a
    // Unit processing slot.
    world.step();
    assert!(scholars
        .iter()
        .all(|id| !world.last_object_process_order.contains(id)));

    // Move the retained observation coordinates to the tile farthest from every other own
    // object. The contained object must not reveal that tile on the next fog recompute.
    let far = (0..world.map.h)
        .flat_map(|ty| (0..world.map.w).map(move |tx| (tx, ty)))
        .max_by_key(|&(tx, ty)| {
            world
                .own_ents(0)
                .filter(|ent| !world.is_contained_scholar(ent.id))
                .map(|ent| (ent.tile().0 - tx).abs().max((ent.tile().1 - ty).abs()))
                .min()
                .unwrap_or(0)
        })
        .unwrap();
    let moved = scholars[0].index().unwrap();
    world.ents[moved].x = far.0 * RANGE_UNITS_PER_TILE + RANGE_UNITS_PER_TILE / 2;
    world.ents[moved].y = far.1 * RANGE_UNITS_PER_TILE + RANGE_UNITS_PER_TILE / 2;
    world.players[0].visible.fill(false);
    world.players[0].explored.fill(false);
    while world.frame % world.params.fog_period != world.params.fog_period - 1 {
        world.step();
    }
    world.step();
    let far_index = (far.1 * world.map.w + far.0) as usize;
    assert!(!world.players[0].visible[far_index]);

    // Reset only the knowledge accumulator to pin one exact 450-frame period. A city
    // University contributes gross 160 and seven base Scholars 560: 720 * 450 / 7200 = 45.
    world.players[0].stock[3] = 0;
    world.players[0].acc[3] = 0;
    world.players[0].gathered[3] = 0;
    // Even corrupted legacy worker bookkeeping must not turn Citizens into University
    // PEASANT_RATE income; only literacy + contained Scholars own this contribution.
    world.ents[university.index().unwrap()].workers = 7;
    for _ in 0..450 {
        world.step();
    }
    assert_eq!(world.players[0].stock[3], 45);
    assert_eq!(world.players[0].gathered[3], 45);
}

fn evaluation_seed(index: u32) -> u32 {
    0x5EED_0001u32.wrapping_add(index.wrapping_mul(0x9E37_79B9))
}

fn accepted_knowledge(seed: u32, seat: usize) -> (usize, usize) {
    let mut world = world(seed);
    let mut ai = Ai::default();
    let mut commands = Vec::new();
    let mut universities = 0;
    let mut scholars = 0;
    let limit = 12 * 60 * FPS;
    while world.frame < limit && world.players[seat].alive {
        if world.frame % ai.decide_period().max(1) == seat as i64 % ai.decide_period().max(1) {
            commands.clear();
            ai.act(&Obs::of(&world, seat), &mut commands);
            for command in commands.drain(..) {
                let type_id = match command {
                    Cmd::Queue { type_id, .. } | Cmd::Build { type_id, .. } => Some(type_id),
                    _ => None,
                };
                let result = world.submit(seat as u8, command);
                if matches!(result, OrderResult::Ok(_)) {
                    if type_id == Some(world.ids.university) {
                        universities += 1;
                    } else if type_id.is_some_and(|type_id| {
                        world
                            .types
                            .get(type_id)
                            .is_some_and(|row| row.name == "Scholar")
                    }) {
                        scholars += 1;
                    }
                }
            }
        }
        world.step();
    }
    eprintln!(
        "knowledge trace seed={seed:#x} seat={seat} accepted=({universities},{scholars}) sites={:?} stock={:?} rejects={:?}",
        world
            .own_ents(seat)
            .filter(|ent| ent.type_id == world.ids.university)
            .map(|ent| (ent.complete, ent.build_left, ent.construction_refusal))
            .collect::<Vec<_>>(),
        world.players[seat].stock,
        world.rejects
    );
    (universities, scholars)
}

#[test]
fn ai_accepts_knowledge_decisions_across_fixed_seeds_and_seats() {
    let mut scholar_runs = 0;
    let mut scholar_decisions = 0;
    for seed_index in 0..2 {
        for seat in 0..2 {
            let (universities, scholars) = accepted_knowledge(evaluation_seed(seed_index), seat);
            assert!(
                universities > 0,
                "seed {seed_index} seat {seat}: no University"
            );
            scholar_runs += usize::from(scholars > 0);
            scholar_decisions += scholars;
        }
    }
    assert!(
        scholar_runs >= 2,
        "Scholar coverage reached only {scholar_runs}/4 runs"
    );
    assert!(
        scholar_decisions >= 2,
        "only {scholar_decisions} accepted Scholar decisions"
    );
}
