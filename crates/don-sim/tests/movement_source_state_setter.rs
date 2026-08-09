//! Identity, revision and atomicity contract for the Sim-owned movement action state.

use don_sim::order::OrderIndex;
use don_sim::systems::collision::DOMAIN_LAND;
use don_sim::systems::movement::PathStack;
use don_sim::systems::movement_live::{
    LiveCollisionFault, LiveCollisionGuy, LiveCollisionSource, MovementSourceState,
};
use don_sim::tick::Sim;

fn source(x: i32, y: i32, moving: bool, action: OrderIndex) -> LiveCollisionSource {
    LiveCollisionSource {
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
        moving,
        searching: false,
        action: action as i32,
        invalid_tiles: Vec::new(),
        guys: vec![LiveCollisionGuy {
            x,
            y,
            angle: 0,
            block_radius: 1,
        }],
    }
}

fn installed() -> (Sim, don_sim::world::Handle) {
    let mut sim = Sim::new(0x51a7_e001, 2);
    let actor = sim.spawn_unit(0, 50, 360, 504, 4).unwrap();
    sim.install_movement_collision_source(actor, source(360, 504, false, OrderIndex::None))
        .unwrap();
    (sim, actor)
}

#[test]
fn cas_transition_publishes_one_coherent_revision_bound_receipt() {
    let (mut sim, actor) = installed();
    let digest_before = sim.world.digest();
    let initial = sim.movement_source_state(actor).unwrap();
    assert_eq!(
        initial,
        MovementSourceState {
            actor,
            row: 0,
            revision: 0,
            moving: false,
            action: OrderIndex::None as i32,
        }
    );

    let receipt = sim
        .compare_exchange_movement_source_state(actor, initial.revision, true, OrderIndex::MoveTo)
        .unwrap();
    assert_eq!(receipt.before, initial);
    assert_eq!(receipt.after.actor, actor);
    assert_eq!(receipt.after.row, initial.row);
    assert_eq!(receipt.after.revision, 1);
    assert!(receipt.after.moving);
    assert_eq!(receipt.after.action, OrderIndex::MoveTo as i32);
    assert_eq!(sim.movement_source_state(actor), Ok(receipt.after));
    assert_eq!(sim.world.digest(), digest_before);
}

#[test]
fn stale_revision_refuses_without_changing_either_action_field() {
    let (mut sim, actor) = installed();
    let first = sim
        .compare_exchange_movement_source_state(actor, 0, true, OrderIndex::MoveTo)
        .unwrap();
    let digest_before = sim.world.digest();

    assert_eq!(
        sim.compare_exchange_movement_source_state(actor, 0, false, OrderIndex::None),
        Err(LiveCollisionFault::StaleSourceRevision {
            row: 0,
            expected: 0,
            observed: 1,
        })
    );
    assert_eq!(sim.movement_source_state(actor), Ok(first.after));
    assert_eq!(sim.world.digest(), digest_before);
}

#[test]
fn stale_and_foreign_handles_refuse_atomically_after_row_compaction() {
    let (mut sim, installed_actor) = installed();
    let moved_actor = sim.spawn_unit(0, 51, 600, 504, 4).unwrap();
    let installed_before = sim.movement_source_state(installed_actor).unwrap();
    let digest_before = sim.world.digest();
    assert!(sim.world.despawn(installed_actor));
    let digest_after_despawn = sim.world.digest();
    assert_ne!(digest_after_despawn, digest_before);

    assert_eq!(
        sim.compare_exchange_movement_source_state(
            installed_actor,
            installed_before.revision,
            true,
            OrderIndex::MoveTo,
        ),
        Err(LiveCollisionFault::StaleActor(installed_actor))
    );
    assert_eq!(
        sim.compare_exchange_movement_source_state(moved_actor, 0, true, OrderIndex::MoveTo),
        Err(LiveCollisionFault::ForeignSource {
            row: 0,
            requested: moved_actor,
            installed: installed_actor,
        })
    );
    assert_eq!(
        sim.movement_collision
            .preflight(&sim.world, &sim.map.world, &[PathStack::new()]),
        Err(LiveCollisionFault::ForeignSource {
            row: 0,
            requested: moved_actor,
            installed: installed_actor,
        })
    );
    assert_eq!(sim.world.digest(), digest_after_despawn);
    assert_eq!(
        sim.movement_collision
            .source(0)
            .map(|source| (source.moving, source.action)),
        Some((installed_before.moving, installed_before.action))
    );
}

#[test]
fn compatibility_wrapper_uses_the_current_revision_and_returns_the_resolved_row() {
    let (mut sim, actor) = installed();
    assert_eq!(
        sim.set_movement_source_state(actor, true, OrderIndex::FleeTo),
        Ok(0)
    );
    assert_eq!(
        sim.movement_source_state(actor).unwrap(),
        MovementSourceState {
            actor,
            row: 0,
            revision: 1,
            moving: true,
            action: OrderIndex::FleeTo as i32,
        }
    );
    assert_eq!(
        sim.set_movement_source_state(actor, true, OrderIndex::FleeTo),
        Ok(0)
    );
    assert_eq!(sim.movement_source_state(actor).unwrap().revision, 2);
}
