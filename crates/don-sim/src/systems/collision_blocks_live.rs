//! Live step-12 adapter for `GameDaemon::process_coll_blocks` `0x00731F90`.
//!
//! [`collision::process_coll_blocks`] already contains the recovered 204-byte reaper,
//! including its per-frame budget and collision-block flag transitions.  What the executable
//! tick was missing is the one piece of `GameDaemon` state the body receives through `this`:
//! the persistent scan cursor at `GameDaemon + 0x20`.
//!
//! This type is deliberately the cursor's exclusive owner.  Callers can snapshot its scalar
//! value for save/load, but each pass must mutate the same runtime and the authoritative
//! [`World`].  That prevents a convenient temporary cursor or cloned terrain plane from making
//! every frame rescan the first cells while the checksum-visible collision blocks never change.

use crate::systems::{collision, map_terrain::World};

/// Persistent `GameDaemon + 0x20` state for the collision-block reaper.
///
/// The cursor is not clamped here.  Retail's body clamps it against the current world size at
/// the point of use and wraps it after every inspected cell; preserving the raw scalar also
/// makes a save/load round trip lossless if the map store has not yet been restored.
#[derive(Debug, PartialEq, Eq)]
pub struct CollisionBlockRuntime {
    cursor: i32,
}

impl Default for CollisionBlockRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl CollisionBlockRuntime {
    /// Retail begins a new `GameDaemon` with its collision-block cursor at zero.
    pub const fn new() -> Self {
        Self { cursor: 0 }
    }

    /// Restore the exact scalar saved from `GameDaemon + 0x20`.
    pub const fn from_cursor(cursor: i32) -> Self {
        Self { cursor }
    }

    /// Scalar snapshot for the eventual save/load adapter.
    pub const fn cursor(&self) -> i32 {
        self.cursor
    }

    /// Run the recovered step-12 body against the authoritative terrain store.
    ///
    /// The returned cursors make integration tests sensitive to resetting or copying the
    /// runtime, while `freed` proves that mutations reached the supplied world rather than a
    /// compatibility view.
    pub fn process_step12(&mut self, world: &mut World) -> CollisionBlockPass {
        let start_cursor = self.cursor;
        let freed = collision::process_coll_blocks(world, &mut self.cursor);
        CollisionBlockPass {
            start_cursor,
            end_cursor: self.cursor,
            freed,
        }
    }
}

/// Observable result of one collision-block maintenance pass.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[must_use]
pub struct CollisionBlockPass {
    pub start_cursor: i32,
    pub end_cursor: i32,
    pub freed: u32,
}
