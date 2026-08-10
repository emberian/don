// SPDX-License-Identifier: GPL-3.0-or-later

use don_sim::objects::{Band, OWNER_SLOTS};
use don_sim::systems::save_load::{load_sim, save_sim};
use don_sim::systems::sparse_object_bands_authority_frontier::{RetailBand, RetailObjectAddress};
use don_sim::systems::{production, walls};
use don_sim::tick::Sim;
use don_sim::world::WorldObjectIdentity;
use don_sim::{Handle, World};

fn assert_unit_dual_reads(world: &World) {
    assert!(world.object_bands_are_dense_equivalent());
    for owner in 0..OWNER_SLOTS {
        for (offset, &row) in world
            .objects
            .slot(owner)
            .band(Band::Unit)
            .iter()
            .enumerate()
        {
            let handle = world.handle_at_row(row as usize).unwrap();
            let address = RetailObjectAddress::new(owner as u8, RetailBand::Unit, offset as i32);
            assert_eq!(
                world.object_bands().live_identity(address),
                Some(WorldObjectIdentity::Unit {
                    id: handle.id,
                    generation: handle.generation,
                })
            );
            assert_eq!(
                world.object_bands().resolve_dense_row(address, |identity| {
                    let WorldObjectIdentity::Unit { id, generation } = identity else {
                        return None;
                    };
                    world
                        .row_of(Handle { id, generation })
                        .map(|row| row as u32)
                }),
                Some(row)
            );
        }
    }
}

#[test]
fn unit_spawn_and_dense_despawn_keep_the_live_dual_read_exact() {
    let mut world = World::with_capacity(8, 0x1234);
    let removed = world.spawn_typed(2, 17).unwrap();
    let survivor = world.spawn_typed(2, 19).unwrap();
    let other_owner = world.spawn_typed(3, 23).unwrap();
    assert_unit_dual_reads(&world);

    assert!(world.despawn(removed));
    assert_unit_dual_reads(&world);

    let survivor_address = world
        .object_bands()
        .address_of(WorldObjectIdentity::Unit {
            id: survivor.id,
            generation: survivor.generation,
        })
        .unwrap();
    assert_eq!(survivor_address.owner, 2);
    assert_eq!(survivor_address.o, 0);
    let other_address = world
        .object_bands()
        .address_of(WorldObjectIdentity::Unit {
            id: other_owner.id,
            generation: other_owner.generation,
        })
        .unwrap();
    assert_eq!(other_address.owner, 3);
    assert_eq!(other_address.o, 0);
}

#[test]
fn build_and_wall_insertion_dual_write_without_using_sparse_allocation() {
    let mut sim = Sim::new(0x2233, 4);
    let build = sim.spawn_build(1, production::BuildData::default());
    let wall = sim.spawn_wall(1, walls::WallState::default());

    assert!(sim.world.object_bands_are_dense_equivalent());
    assert_eq!(
        sim.world
            .object_bands()
            .live_identity(RetailObjectAddress::new(1, RetailBand::Build, 2000,)),
        Some(WorldObjectIdentity::BuildRow(build as u32))
    );
    assert_eq!(
        sim.world
            .object_bands()
            .live_identity(RetailObjectAddress::new(1, RetailBand::Wall, 3000,)),
        Some(WorldObjectIdentity::WallRow(wall as u32))
    );
}

#[test]
fn owner_activity_is_checksum_visible_and_reversible_in_both_views() {
    let mut world = World::with_capacity(4, 0x3344);
    world.spawn_typed(2, 17).unwrap();
    let active_digest = world.digest();

    assert!(world.set_object_owner_active(2, false));
    assert!(!world.objects.is_active(2));
    assert_eq!(world.object_bands().is_active(2), Some(false));
    assert!(world.object_bands_are_dense_equivalent());
    assert_ne!(world.digest(), active_digest);

    assert!(world.set_object_owner_active(2, true));
    assert_eq!(world.digest(), active_digest);
}

#[test]
fn sparse_owner_roundtrips_in_format_eight_and_changes_save_bytes() {
    let mut sim = Sim::new(0x4455, 4);
    sim.spawn_unit(2, 17, 768, 1536, 4).unwrap();
    let active_bytes = save_sim(&sim).unwrap();
    let active_digest = sim.world.digest();

    assert!(sim.world.set_object_owner_active(2, false));
    let inactive_bytes = save_sim(&sim).unwrap();
    assert_ne!(inactive_bytes, active_bytes);
    assert_ne!(sim.world.digest(), active_digest);

    let loaded = load_sim(&inactive_bytes).unwrap();
    assert!(!loaded.world.objects.is_active(2));
    assert_eq!(loaded.world.object_bands().is_active(2), Some(false));
    assert!(loaded.world.object_bands_are_dense_equivalent());
    assert_eq!(
        loaded.world.object_bands().snapshot().unwrap(),
        sim.world.object_bands().snapshot().unwrap()
    );
    assert_eq!(loaded.world.digest(), sim.world.digest());
    assert_eq!(save_sim(&loaded).unwrap(), inactive_bytes);
}
