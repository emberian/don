//! Path-import proof for the exclusive step-12 collision-block cursor store.
//!
//! The production module is compiled here against a tiny instrumented host.  These are adapter
//! tests, not another transcription of the already-covered collision reaper: they fail if the
//! runtime resets its cursor between calls, runs against a copied host, or drops the body's
//! mutation count.

mod systems {
    pub mod map_terrain {
        #[derive(Debug, Default, PartialEq, Eq)]
        pub struct World {
            pub cursors_seen: Vec<i32>,
            pub blocks: u32,
        }
    }

    pub mod collision {
        use super::map_terrain::World;

        pub fn process_coll_blocks(world: &mut World, cursor: &mut i32) -> u32 {
            world.cursors_seen.push(*cursor);
            let freed = u32::from(world.blocks != 0);
            world.blocks = world.blocks.saturating_sub(freed);
            *cursor = (*cursor + 5) % 12;
            freed
        }
    }
}

#[path = "../src/systems/collision_blocks_live.rs"]
mod collision_blocks_live;

use collision_blocks_live::CollisionBlockRuntime;
use systems::map_terrain::World;

#[test]
fn path_import_mutates_the_authoritative_host_and_retains_the_cursor() {
    let mut runtime = CollisionBlockRuntime::new();
    let mut world = World {
        blocks: 2,
        ..World::default()
    };

    let first = runtime.process_step12(&mut world);
    let second = runtime.process_step12(&mut world);

    assert_eq!(
        (first.start_cursor, first.end_cursor, first.freed),
        (0, 5, 1)
    );
    assert_eq!(
        (second.start_cursor, second.end_cursor, second.freed),
        (5, 10, 1)
    );
    assert_eq!(runtime.cursor(), 10);
    assert_eq!(
        world.blocks, 0,
        "the installed world, not a copy, was reaped"
    );
    assert_eq!(
        world.cursors_seen,
        [0, 5],
        "the cursor was not reset per pass"
    );
}

#[test]
fn path_import_preserves_the_raw_cursor_for_save_restore() {
    let mut runtime = CollisionBlockRuntime::from_cursor(-7);
    let mut world = World::default();

    let pass = runtime.process_step12(&mut world);

    assert_eq!(pass.start_cursor, -7);
    assert_eq!(world.cursors_seen, [-7]);
    assert_eq!(runtime.cursor(), -2);
}
