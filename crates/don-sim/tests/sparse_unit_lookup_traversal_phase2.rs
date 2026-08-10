// SPDX-License-Identifier: GPL-3.0-or-later

use don_sim::systems::sparse_object_bands_authority_frontier::{RetailBand, SparseSlotLifecycle};
use don_sim::systems::{production, walls};
use don_sim::tick::Sim;
use don_sim::world::WorldObjectIdentity;
use don_sim::World;

#[test]
fn exact_unit_lookup_resolves_stable_identity_after_dense_row_compaction() {
    let mut world = World::with_capacity(8, 0x1234);
    let removed = world.spawn_typed(2, 17).unwrap();
    let survivor = world.spawn_typed(3, 19).unwrap();
    let before_row = world.row_of(survivor).unwrap();
    assert_eq!(world.unit_row_at(3, 0), Some(before_row));

    assert!(world.despawn(removed));
    let after_row = world.row_of(survivor).unwrap();
    assert_ne!(after_row, before_row);
    assert_eq!(world.unit_row_at(3, 0), Some(after_row));
    assert_eq!(world.unit_row_at(2, 0), None);
    assert_eq!(world.unit_row_at(-1, 0), None);
    assert_eq!(world.unit_row_at(3, -1), None);
    assert_eq!(world.unit_row_at(3, 2_000), None);
}

#[test]
fn retained_sparse_traversal_drives_unit_rotation_then_build_and_wall() {
    let mut sim = Sim::new(0x2233, 4);
    sim.spawn_unit(0, 17, 768, 768, 4).unwrap();
    sim.spawn_unit(1, 19, 1536, 1536, 4).unwrap();
    let build = sim.spawn_build(1, production::BuildData::default());
    let wall = sim.spawn_wall(1, walls::WallState::default());

    let mut entries = Vec::with_capacity(16);
    sim.world.object_bands().traversal_into(1, &mut entries);
    let capacity = entries.capacity();
    let shape: Vec<(u8, RetailBand, i32)> = entries
        .iter()
        .map(|entry| (entry.address.owner, entry.address.band, entry.address.o))
        .collect();
    assert_eq!(
        shape,
        [
            (1, RetailBand::Unit, 0),
            (0, RetailBand::Unit, 0),
            (1, RetailBand::Build, 2_000),
            (1, RetailBand::Wall, 3_000),
        ]
    );
    assert!(matches!(
        entries[2].lifecycle,
        SparseSlotLifecycle::Live(WorldObjectIdentity::BuildRow(row)) if row == build as u32
    ));
    assert!(matches!(
        entries[3].lifecycle,
        SparseSlotLifecycle::Live(WorldObjectIdentity::WallRow(row)) if row == wall as u32
    ));

    sim.world.object_bands().traversal_into(0, &mut entries);
    assert_eq!(entries.capacity(), capacity);
    assert_eq!(entries[0].address.owner, 0);
    assert_eq!(entries[1].address.owner, 1);
}
