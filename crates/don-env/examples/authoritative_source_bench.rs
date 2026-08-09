//! Dependency-free microbenchmark for authoritative scenario source installation/reset/masks.
//!
//! Run explicitly with a release build; this example is not part of the correctness gate.

use don_env::authoritative_backend::{
    AuthoritativeBackend, AuthoritativeScenarioSpec, QueuePosition, ScenarioMovementSource,
    UnitActionRequest, MOVE_TO_VERB_INDEX,
};
use don_env::{ScenarioSpec, ScenarioUnit};
use don_sim::order::OrderIndex;
use don_sim::systems::collision::DOMAIN_LAND;
use don_sim::systems::map_terrain::COORD_PER_TILE;
use don_sim::systems::movement_live::{LiveCollisionGuy, LiveCollisionSource};
use std::hint::black_box;
use std::time::{Duration, Instant};

const UNITS: usize = 64;
const CONSTRUCT_ITERS: u32 = 200;
const RESET_ITERS: u32 = 500;
const MASK_ITERS: u32 = 20_000;

fn scenario() -> AuthoritativeScenarioSpec {
    let mut units = Vec::with_capacity(UNITS);
    let mut movement_sources = Vec::with_capacity(UNITS);
    for unit in 0..UNITS {
        let x = (unit as i32 % 8 + 2) * COORD_PER_TILE;
        let y = (unit as i32 / 8 + 2) * COORD_PER_TILE;
        units.push(ScenarioUnit {
            who: 0,
            type_id: 50,
            x,
            y,
            los_tiles: 4,
        });
        movement_sources.push(ScenarioMovementSource {
            unit,
            source: LiveCollisionSource {
                domain: DOMAIN_LAND,
                block_radius: 1,
                big_radius: 48,
                push_size: 0,
                push_circles: 0,
                unit_flags: 0,
                unit_flags2: 0,
                attack_value: 0,
                spell_id: -1,
                unpacking: false,
                captain: false,
                moving: true,
                searching: false,
                action: OrderIndex::MoveTo as i32,
                invalid_tiles: Vec::new(),
                guys: vec![LiveCollisionGuy {
                    x,
                    y,
                    angle: 0,
                    block_radius: 1,
                }],
            },
        });
    }
    AuthoritativeScenarioSpec {
        episode: ScenarioSpec {
            seed: 0x50ce_bec4,
            map_wcells: 4,
            active_players: vec![0],
            units,
        },
        movement_sources,
    }
}

fn per_operation(elapsed: Duration, iterations: u32) -> f64 {
    elapsed.as_nanos() as f64 / f64::from(iterations)
}

fn main() {
    let setup = scenario();

    let started = Instant::now();
    for _ in 0..CONSTRUCT_ITERS {
        black_box(AuthoritativeBackend::from_authoritative_scenario(
            setup.clone(),
        ))
        .unwrap();
    }
    let construct = started.elapsed();

    let mut backend = AuthoritativeBackend::from_authoritative_scenario(setup).unwrap();
    let actor = backend.sim().world.handle_at_row(0).unwrap();
    let request = UnitActionRequest {
        verb_head: MOVE_TO_VERB_INDEX as u16 + 1,
        actor,
        target_x: 3 * COORD_PER_TILE,
        target_y: 2 * COORD_PER_TILE,
        target_entity: 0,
        queue: QueuePosition::Replace,
        order_flags: 0,
    };

    let started = Instant::now();
    for _ in 0..RESET_ITERS {
        black_box(backend.reset()).unwrap();
    }
    let reset = started.elapsed();

    let started = Instant::now();
    for _ in 0..MASK_ITERS {
        black_box(backend.unit_verb_mask(0, request));
    }
    let mask = started.elapsed();

    println!("authoritative source benchmark: {UNITS} installed units");
    println!(
        "construct+install: {:.1} ns/op ({CONSTRUCT_ITERS} iterations)",
        per_operation(construct, CONSTRUCT_ITERS)
    );
    println!(
        "reset+reinstall:   {:.1} ns/op ({RESET_ITERS} iterations)",
        per_operation(reset, RESET_ITERS)
    );
    println!(
        "conditional mask: {:.1} ns/op ({MASK_ITERS} iterations)",
        per_operation(mask, MASK_ITERS)
    );
}
