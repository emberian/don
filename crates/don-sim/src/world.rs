//! The simulation world: PDB-derived state columns, the object registry, and the tick.
//!
//! # What changed, and why it matters
//!
//! This used to be a hand-named placeholder (`pos_x`, `vel_x`, `armor`, `attack`) with an
//! invented integrator. It is now driven by [`crate::generated::state`], which is
//! generated from the shipped PDB's own type stream: every column below is a field the
//! MSVC compiler emitted for this exact binary, at its real offset, with its real width,
//! flagged with whether `walk_data` — i.e. the checksum — visits it.
//!
//! The consequence is that "which state exists" is no longer a design decision. It is
//! read out of `schema/pdb-types.json`, and adding a field means regenerating, not
//! editing.
//!
//! # Storage: dense rows, stable handles, engine-visible identity
//!
//! Live units occupy rows `0..live` of every column with no holes, so a tick system is a
//! straight-line pass over a contiguous prefix. Densifying costs index stability, so
//! identity is carried by a generational [`Handle`].
//!
//! *Separately*, every unit also carries the engine's own address: `who` (owner slot) and
//! `o` (index inside that owner's banded object list), maintained by
//! [`crate::objects::ObjectRegistry`]. Rows are ours; `(who, o)` is the engine's, and the
//! two are kept in step through every spawn and despawn.
//!
//! # The tick
//!
//! [`World::step`] is `Game::do_frame` `0x00591EF0`, run as the ordered schedule in
//! [`crate::schedule::DO_FRAME`]. Coverage is counted per subsystem and per order type
//! rather than estimated: see [`World::coverage`].

use crate::generated::state::{unit, UnitCols};
use crate::objects::{Band, ObjectRegistry, HERD_PERIOD, OWNER_SLOTS, WILDLIFE_PERIOD};
use crate::order::{ArmStatus, Order, OrderCoverage, OrderIndex, OrderList};
use crate::rng::Random;
use crate::schedule::{ScheduleCoverage, DO_FRAME, FRAMES_PER_SECOND, SPEED_NORMAL, TIMINGS_MS};
use crate::simd;
use crate::systems::sparse_object_bands_authority_frontier::{
    DenseRegistryEntry, RetailBand, RetailObjectAddress, SnapshotLifecycle, SparseObjectBands,
    SparseRegistryError, SparseRegistrySnapshot, SparseSlotLifecycle, TraversalEntry,
};
use crate::trig::{cosx, find_angle, sinx};

/// Sim frames per game **second** — `Game::do_frame`'s own `idiv 15` at `0x005924CF`
/// [measured]. This is the unit every `"450 frames"` rule value is denominated in.
///
/// It is **not** a wall-clock rate. Pacing comes from `TurnControl::timings`
/// `0x00AFC4A4` = `{200, 125, 67, 50, 1}` ms/frame, so Normal is 67 ms — 14.925 Hz.
/// Both numbers are real and they are not the same number; see [`TICK_MS_NORMAL`].
/// (The old sourcing of this constant to a `rules.xml` header comment is superseded: the
/// comment is right about the denomination and says nothing about pacing.)
pub const TICK_HZ: u32 = FRAMES_PER_SECOND as u32;

/// Milliseconds of wall clock per sim frame at Normal speed — `TurnControl::timings[2]`
/// `0x00AFC4A4` [measured]. A "30 second" 450-frame rule really takes 30.15 s.
pub const TICK_MS_NORMAL: i32 = TIMINGS_MS[SPEED_NORMAL];

/// World Coord units per map tile.
///
/// [measured] from `Objects::process_all`'s wildlife spawn, which turns a tile index into
/// a position with `tile * 0x300 + 0x180` — stride 768, centred at 384. The pathfinder's
/// 192-unit step is therefore a quarter tile.
pub const COORD_PER_TILE: i32 = 0x300;

/// The pathfinder's quarter-tile step. Kept under the old name because callers spell
/// movement granularity this way; it is a quarter tile, not a tile.
pub const SUBTILE: i32 = COORD_PER_TILE / 4;

/// Hard per-world unit capacity.
///
/// A provisioning decision, not a claim about the game: the engine's own per-owner unit
/// band runs `[0, 2000)` (see [`crate::objects`]), so 4096 is comfortably above one
/// owner's ceiling and below anything that would make a row index awkward.
pub const MAX_UNITS: usize = 4096;

/// Default map extent in Coord units: a 256-tile square.
pub const MAP_SPAN: i32 = 256 * COORD_PER_TILE;

const NO_ROW: u32 = u32::MAX;

/// `SubObjectData::flags` bit 0 at +8 — the "active, run `::process`" bit that
/// `Objects::process_all` tests at `0x0065DD0D` [measured].
pub const OBJ_FLAG_ACTIVE: u8 = 1;

/// Stable identity for a unit, valid across compaction.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Handle {
    pub id: u32,
    pub generation: u32,
}

/// Stable identity stored behind a retail `(owner, band, o)` address.
///
/// Unit rows already have a generational identity. Build and Wall pools currently expose only
/// their own dense row ids, so those variants state that narrower authority explicitly instead
/// of pretending a Unit handle owns all three bands.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum WorldObjectIdentity {
    Unit { id: u32, generation: u32 },
    BuildRow(u32),
    WallRow(u32),
}

/// What the tick actually executed, counted rather than estimated.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Coverage {
    pub schedule: ScheduleCoverage,
    pub orders: OrderCoverage,
    /// `Unit::process` entries.
    pub unit_process: u64,
    /// `Unit::work` entries (vtable +0x188).
    pub unit_work: u64,
    /// `Guy::process` -> `Guy::move` integrations.
    pub guy_move: u64,
    /// Inactive objects whose `hold_frames` was decremented instead of processed.
    pub hold_decrements: u64,
    /// `Build::process` dispatches. The executor is not ported.
    pub build_process: u64,
    /// `Wall::process` dispatches. The executor is not ported.
    pub wall_process: u64,
    /// Frames on which retail would have drawn `game_random` for the wildlife spawn and
    /// we did not. **Every one is a divergence in the RNG stream** — see the note on
    /// `objects_process_all`.
    pub wildlife_draws_skipped: u64,
    /// `do_attack` calls that produced a number through the derived damage pipeline.
    pub damage_applied: u64,
    /// `do_attack` calls that could not run for want of a balance or type table.
    pub damage_skipped_no_tables: u64,
}

impl Coverage {
    pub fn merge(&mut self, other: &Coverage) {
        self.schedule.merge(&other.schedule);
        self.orders.merge(&other.orders);
        self.unit_process += other.unit_process;
        self.unit_work += other.unit_work;
        self.guy_move += other.guy_move;
        self.hold_decrements += other.hold_decrements;
        self.build_process += other.build_process;
        self.wall_process += other.wall_process;
        self.wildlife_draws_skipped += other.wildlife_draws_skipped;
        self.damage_applied += other.damage_applied;
        self.damage_skipped_no_tables += other.damage_skipped_no_tables;
    }
}

/// The handful of `UnitTypeData` fields the ported combat path reads.
///
/// Offsets are the PDB's [measured]; the full table is generated in
/// [`crate::generated::state::unit_type`] and this is the projection the tick needs.
/// `attack` is carried **x10** by the loader (`0x0061B01B`), exactly as retail stores it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UnitTypeStats {
    /// `UnitTypeData::type` at +4 — the global type id, and the balance-table row.
    pub type_id: i32,
    /// `+488 attack`, x10.
    pub attack: i32,
    /// `+532 armor`, display scale.
    pub armor: i32,
    /// `+528 hits`.
    pub hits: i32,
    /// `+500 recharge`, frames between attacks.
    pub recharge: i32,
    /// `+508 max_range`, Coord units.
    pub max_range: i32,
    /// `+504 min_range`.
    pub min_range: i32,
}

/// Static rules the world reads, shared across a batch rather than copied per world.
///
/// Type tables and the balance matrix are *global* game data. Holding them behind a
/// shared handle is what keeps a batch of 4096 worlds from paying 486 KB of balance table
/// 4096 times.
#[derive(Clone, Default)]
pub struct SharedRules {
    pub balance: Option<std::sync::Arc<crate::balance::BalanceTable>>,
    /// Per-type combat stats, keyed by the engine's global type id.
    pub unit_stats: std::sync::Arc<Vec<UnitTypeStats>>,
    pub combat: crate::mechanics::CombatRules,
}

/// One simulation world.
#[derive(Clone)]
pub struct World {
    /// `UnitData` columns, generated from the PDB.
    pub units: UnitCols,
    /// `UnitData::orderlist` at +200, one per unit row.
    unit_orders: Vec<OrderList>,
    /// The type id behind `UnitData::ptype` (`ObjectType*` at +24).
    ///
    /// A pointer cannot be a column, so the *referent's* identity is carried instead.
    /// This is a port-level substitution and is named as one.
    unit_type_id: Vec<i32>,
    /// Per-frame movement step, `sinx(angle, speed)` / `-cosx(angle, speed)`.
    ///
    /// Derived scratch, not a PDB field: retail recomputes it inside `Unit::move_step`
    /// every frame from `angle` and the speed. Caching it makes the integration an
    /// element-wise kernel, which is what [`World::step_hot`] measures.
    move_step_x: Vec<i32>,
    move_step_y: Vec<i32>,

    /// `Objects` — ten owner slots, three index bands, and the rotation.
    pub objects: ObjectRegistry,
    /// Stable retail object addresses joined to row-independent identities.
    ///
    /// This is live canonical save/checksum and traversal state, but allocation deliberately
    /// remains on [`ObjectRegistry`] during the dual-read phase. Dense mutations mirror their
    /// committed result here. Unit tombstone holds may tick; sparse allocation/reuse remains
    /// disabled until every dense-address consumer migrates.
    object_bands: SparseObjectBands<WorldObjectIdentity>,
    /// Reusable buffer for the per-frame traversal order. Not state; allocating it every
    /// frame was measurably the largest single cost in the object pass.
    traversal_buf: Vec<TraversalEntry<WorldObjectIdentity>>,

    // ---- identity ----
    handle_of_row: Vec<u32>,
    row_of_handle: Vec<u32>,
    generation: Vec<u32>,
    live: u32,
    capacity: u32,

    /// `Game::frame` at `Game+0x550`. Incremented at step 20, *after*
    /// `Objects::process_all`, which is why the owner rotation uses the pre-increment
    /// value.
    pub frame: i32,
    /// `Game::seconds` at `Game+0x560`. One per 15 frames.
    pub seconds: i32,
    /// `GameAccess::game_random` `0x00E37A8C` — the main simulation stream.
    pub random: Random,
    /// Checksum-visible `GameAccess::items` registry attached to the terrain world's
    /// existing WCoord occupancy plane.
    ///
    /// `None` is deliberately different from an initialized-but-empty registry:
    /// callers must not report an Adler value of one as a modelled channel until
    /// map setup has supplied the grid dimensions.
    pub item_runtime: Option<crate::item_runtime::ItemRuntime>,
    pub rules: SharedRules,
    coverage: Coverage,
}

/// The pointer-free, checksum-relevant portion of [`World`] owned by save/load.
///
/// `row_of_handle`, the dense object registry, traversal scratch, and coverage are deliberately
/// absent. They are derived state. The sparse object-band snapshot is authoritative persisted
/// state; import rebuilds the dense compatibility view and requires the two representations to
/// agree before replacing the live world.
#[derive(Clone)]
pub(crate) struct WorldSaveState {
    pub units: UnitCols,
    pub unit_orders: Vec<OrderList>,
    pub unit_type_id: Vec<i32>,
    pub move_step_x: Vec<i32>,
    pub move_step_y: Vec<i32>,
    pub handle_of_row: Vec<u32>,
    pub generation: Vec<u32>,
    pub active_slots: [bool; OWNER_SLOTS],
    /// Exact row traversal for each owner's retail band-2000 object range.
    pub build_rows: [Vec<u32>; OWNER_SLOTS],
    /// `None` exists only for conversion of legacy format-7 dense saves. Every newly exported
    /// state carries the canonical sparse owner and validates it against the dense projection.
    pub object_bands: Option<SparseRegistrySnapshot<WorldObjectIdentity>>,
    pub live: u32,
    pub capacity: u32,
    pub frame: i32,
    pub seconds: i32,
    pub random_state: i32,
}

/// A structural load failure. No variant is recoverable by filling in guessed state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum WorldSaveError {
    Capacity,
    Length(&'static str),
    HandlePermutation,
    OrderTargetIdentity {
        row: usize,
        order: usize,
        handle: Handle,
    },
    Owner {
        row: usize,
        owner: u8,
    },
    ObjectIndex {
        row: usize,
        index: i16,
    },
    DuplicateObjectIndex {
        owner: usize,
        index: usize,
    },
    RegistryMismatch,
    UnsupportedObjectBand,
}

impl std::fmt::Display for WorldSaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Capacity => f.write_str("world capacity/live count is invalid"),
            Self::Length(name) => write!(f, "world save length mismatch: {name}"),
            Self::HandlePermutation => f.write_str("world handle ids are not a permutation"),
            Self::OrderTargetIdentity { row, order, handle } => write!(
                f,
                "unit row {row} order {order} has invalid exact target identity {handle:?}"
            ),
            Self::Owner { row, owner } => write!(f, "unit row {row} has invalid owner {owner}"),
            Self::ObjectIndex { row, index } => {
                write!(f, "unit row {row} has invalid object index {index}")
            }
            Self::DuplicateObjectIndex { owner, index } => {
                write!(f, "owner {owner} has duplicate object index {index}")
            }
            Self::RegistryMismatch => f.write_str("world object registry disagrees with unit rows"),
            Self::UnsupportedObjectBand => f.write_str(
                "world save adapter owns unit/build bands only, with builds in player slots 0..7",
            ),
        }
    }
}

impl WorldSaveState {
    fn validate_and_rebuild(
        &self,
    ) -> Result<
        (
            ObjectRegistry,
            SparseObjectBands<WorldObjectIdentity>,
            Vec<u32>,
        ),
        WorldSaveError,
    > {
        let cap = self.capacity as usize;
        let live = self.live as usize;
        if cap > MAX_UNITS || live > cap {
            return Err(WorldSaveError::Capacity);
        }
        if self.units.capacity() != cap || self.units.len() != live {
            return Err(WorldSaveError::Length("unit columns"));
        }
        for (name, len, expected) in [
            ("order lists", self.unit_orders.len(), live),
            ("unit type ids", self.unit_type_id.len(), live),
            ("move step x", self.move_step_x.len(), live),
            ("move step y", self.move_step_y.len(), live),
            ("handle permutation", self.handle_of_row.len(), cap),
            ("handle generations", self.generation.len(), cap),
        ] {
            if len != expected {
                return Err(WorldSaveError::Length(name));
            }
        }

        let mut seen_handles = vec![false; cap];
        for &id in &self.handle_of_row {
            let id = id as usize;
            if id >= cap || std::mem::replace(&mut seen_handles[id], true) {
                return Err(WorldSaveError::HandlePermutation);
            }
        }

        for (row, orders) in self.unit_orders.iter().enumerate() {
            for (order, value) in orders.iter().enumerate() {
                let Some(handle) = value.target_handle else {
                    continue;
                };
                if handle.id as usize >= cap || value.target_who < 0 || value.target_o < 0 {
                    return Err(WorldSaveError::OrderTargetIdentity { row, order, handle });
                }
            }
        }

        let mut rows_by_owner: [Vec<Option<u32>>; OWNER_SLOTS] =
            std::array::from_fn(|_| Vec::new());
        for row in 0..live {
            let owner = self.units.get_who(row);
            if owner as usize >= OWNER_SLOTS {
                return Err(WorldSaveError::Owner { row, owner });
            }
            let object_index = self.units.o()[row];
            if object_index < 0 {
                return Err(WorldSaveError::ObjectIndex {
                    row,
                    index: object_index,
                });
            }
            let object_index = object_index as usize;
            let entries = &mut rows_by_owner[owner as usize];
            if object_index >= live {
                return Err(WorldSaveError::ObjectIndex {
                    row,
                    index: self.units.o()[row],
                });
            }
            if entries.len() <= object_index {
                entries.resize(object_index + 1, None);
            }
            if entries[object_index].replace(row as u32).is_some() {
                return Err(WorldSaveError::DuplicateObjectIndex {
                    owner: owner as usize,
                    index: object_index,
                });
            }
        }

        let mut objects = ObjectRegistry::new();
        let mut object_entries = Vec::with_capacity(live);
        for (owner, entries) in rows_by_owner.iter().enumerate() {
            if entries.iter().any(Option::is_none) {
                return Err(WorldSaveError::RegistryMismatch);
            }
            for (index, row) in entries.iter().enumerate() {
                let row = row.unwrap();
                let inserted = objects.insert(owner, Band::Unit, row);
                if inserted as usize != index {
                    return Err(WorldSaveError::RegistryMismatch);
                }
                let id = self.handle_of_row[row as usize];
                object_entries.push(DenseRegistryEntry {
                    address: RetailObjectAddress::new(owner as u8, RetailBand::Unit, index as i32),
                    identity: WorldObjectIdentity::Unit {
                        id,
                        generation: self.generation[id as usize],
                    },
                });
            }
        }

        let build_count: usize = self.build_rows.iter().map(Vec::len).sum();
        let mut seen_build_rows = vec![false; build_count];
        for (owner, rows) in self.build_rows.iter().enumerate() {
            if owner >= crate::objects::BANDED_SLOTS && !rows.is_empty() {
                return Err(WorldSaveError::UnsupportedObjectBand);
            }
            for (index, &row) in rows.iter().enumerate() {
                let row_index = row as usize;
                if row_index >= build_count
                    || std::mem::replace(&mut seen_build_rows[row_index], true)
                {
                    return Err(WorldSaveError::RegistryMismatch);
                }
                let inserted = objects.insert(owner, Band::Build, row);
                if inserted != crate::objects::BUILD_BAND_BASE + index as u32 {
                    return Err(WorldSaveError::RegistryMismatch);
                }
                object_entries.push(DenseRegistryEntry {
                    address: RetailObjectAddress::new(
                        owner as u8,
                        RetailBand::Build,
                        crate::objects::BUILD_BAND_BASE as i32 + index as i32,
                    ),
                    identity: WorldObjectIdentity::BuildRow(row),
                });
            }
        }
        for (owner, active) in self.active_slots.iter().copied().enumerate() {
            objects.set_active(owner, active);
        }

        let mut row_of_handle = vec![NO_ROW; cap];
        for (row, &id) in self.handle_of_row.iter().take(live).enumerate() {
            row_of_handle[id as usize] = row as u32;
        }

        let (derived_bands, _) =
            SparseObjectBands::from_dense_entries(self.active_slots, object_entries)
                .map_err(|_| WorldSaveError::RegistryMismatch)?;
        let object_bands = if let Some(snapshot) = &self.object_bands {
            let restored = SparseObjectBands::from_snapshot(snapshot.clone())
                .map_err(|_| WorldSaveError::RegistryMismatch)?;
            if restored
                .snapshot()
                .map_err(|_| WorldSaveError::RegistryMismatch)?
                != derived_bands
                    .snapshot()
                    .map_err(|_| WorldSaveError::RegistryMismatch)?
            {
                // During the dual-read phase the old dense consumers cannot represent a sparse
                // gap. Persist the new owner, but admit only the exact dense-equivalent subset
                // until lookup/traversal and allocation have migrated together.
                return Err(WorldSaveError::RegistryMismatch);
            }
            restored
        } else {
            // Format 7 carried only the dense rows. Its representable state has no tombstones,
            // so the conversion is exact and becomes format-8 canonical state on resave.
            derived_bands
        };
        Ok((objects, object_bands, row_of_handle))
    }
}

impl World {
    /// A world provisioned to [`MAX_UNITS`].
    pub fn new(seed: u64) -> World {
        World::with_capacity(MAX_UNITS, seed)
    }

    /// A world provisioned for at most `capacity` units (clamped to [`MAX_UNITS`]).
    pub fn with_capacity(capacity: usize, seed: u64) -> World {
        let n = capacity.min(MAX_UNITS);
        World {
            units: UnitCols::with_capacity(n),
            unit_orders: Vec::with_capacity(n),
            unit_type_id: Vec::with_capacity(n),
            move_step_x: Vec::with_capacity(n),
            move_step_y: Vec::with_capacity(n),
            objects: ObjectRegistry::new(),
            object_bands: SparseObjectBands::new(),
            traversal_buf: Vec::with_capacity(n + 16),
            handle_of_row: (0..n as u32).collect(),
            row_of_handle: vec![NO_ROW; n],
            generation: vec![0; n],
            live: 0,
            capacity: n as u32,
            frame: 0,
            seconds: 0,
            // The engine seeds `game_random` from the match setup; the low 32 bits of the
            // caller's seed stand in for that and keep every world distinct.
            random: Random::new(seed as i32),
            item_runtime: None,
            rules: SharedRules::default(),
            coverage: Coverage::default(),
        }
    }

    #[inline]
    pub fn live_count(&self) -> u32 {
        self.live
    }

    #[inline]
    pub fn capacity(&self) -> u32 {
        self.capacity
    }

    /// Read-only access to the canonical sparse retail-address owner.
    ///
    /// Mutation stays behind World's dual-write methods until sparse allocation and every dense
    /// lookup consumer migrate as one transaction.
    #[inline]
    pub fn object_bands(&self) -> &SparseObjectBands<WorldObjectIdentity> {
        &self.object_bands
    }

    /// Resolve an exact retail Unit address through its stable generational identity.
    /// Tombstones, wrong-band identities, stale handles, and malformed coordinates are absent.
    pub fn unit_row_at(&self, who: i32, o: i32) -> Option<usize> {
        if who < 0 || who as usize >= OWNER_SLOTS || !RetailBand::Unit.contains(o) {
            return None;
        }
        let identity = self.object_bands.live_identity(RetailObjectAddress::new(
            who as u8,
            RetailBand::Unit,
            o,
        ))?;
        let WorldObjectIdentity::Unit { id, generation } = identity else {
            return None;
        };
        self.row_of(Handle { id, generation })
    }

    /// Exclusive Unit-band high-water mark for one owner.
    pub fn unit_mark(&self, owner: usize) -> Option<i32> {
        self.object_bands.mark(owner, RetailBand::Unit)
    }

    pub(crate) fn tick_unit_tombstone_hold(
        &mut self,
        address: RetailObjectAddress,
    ) -> Result<u16, SparseRegistryError> {
        if address.band != RetailBand::Unit {
            return Err(SparseRegistryError::InvalidObjectIndex);
        }
        self.object_bands.tick_tombstone_hold(address)
    }

    /// Whether every current dense address resolves to the same stable identity and all marks,
    /// activity bits, and retained slots are still in the gap-free phase-1 subset.
    pub fn object_bands_are_dense_equivalent(&self) -> bool {
        let Ok(derived) = self.derive_object_bands_from_dense() else {
            return false;
        };
        self.object_bands.snapshot().ok() == derived.snapshot().ok()
    }

    /// Activate/deactivate one Objects owner in both live representations.
    pub fn set_object_owner_active(&mut self, owner: usize, active: bool) -> bool {
        if owner >= OWNER_SLOTS {
            return false;
        }
        self.objects.set_active(owner, active);
        self.object_bands
            .set_active(owner, active)
            .expect("owner was range-checked");
        true
    }

    fn derive_object_bands_from_dense(
        &self,
    ) -> Result<SparseObjectBands<WorldObjectIdentity>, WorldSaveError> {
        let active = std::array::from_fn(|owner| self.objects.is_active(owner));
        let mut entries = Vec::with_capacity(self.objects.total_objects());
        for owner in 0..OWNER_SLOTS {
            for (band, retail_band) in [
                (Band::Unit, RetailBand::Unit),
                (Band::Build, RetailBand::Build),
                (Band::Wall, RetailBand::Wall),
            ] {
                for (offset, &row) in self.objects.slot(owner).band(band).iter().enumerate() {
                    let o = band.base() + offset as u32;
                    let identity = match band {
                        Band::Unit => {
                            let row_index = row as usize;
                            if row_index >= self.live as usize
                                || self.units.get_who(row_index) as usize != owner
                                || self.units.o()[row_index] as i32 != o as i32
                            {
                                return Err(WorldSaveError::RegistryMismatch);
                            }
                            let id = self.handle_of_row[row_index];
                            WorldObjectIdentity::Unit {
                                id,
                                generation: self.generation[id as usize],
                            }
                        }
                        Band::Build => WorldObjectIdentity::BuildRow(row),
                        Band::Wall => WorldObjectIdentity::WallRow(row),
                    };
                    entries.push(DenseRegistryEntry {
                        address: RetailObjectAddress::new(owner as u8, retail_band, o as i32),
                        identity,
                    });
                }
            }
        }
        SparseObjectBands::from_dense_entries(active, entries)
            .map(|(bands, _)| bands)
            .map_err(|_| WorldSaveError::RegistryMismatch)
    }

    pub(crate) fn mirror_dense_non_unit_append(
        &mut self,
        owner: usize,
        band: Band,
        row: u32,
        expected_o: u32,
    ) -> Result<(), WorldSaveError> {
        let (retail_band, identity) = match band {
            Band::Build => (RetailBand::Build, WorldObjectIdentity::BuildRow(row)),
            Band::Wall => (RetailBand::Wall, WorldObjectIdentity::WallRow(row)),
            Band::Unit => return Err(WorldSaveError::RegistryMismatch),
        };
        let receipt = self
            .object_bands
            .mirror_dense_append(owner as u8, retail_band, identity)
            .map_err(|_| WorldSaveError::RegistryMismatch)?;
        if receipt.address.o != expected_o as i32 {
            return Err(WorldSaveError::RegistryMismatch);
        }
        Ok(())
    }

    /// Export the save-owned state after proving that all private/derived stores agree.
    ///
    /// Building bodies are owned by [`crate::tick::Sim`]; this adapter carries only their
    /// stable band traversal rows. The caller must validate and serialize the bodies in
    /// the same transaction. Wall bodies remain outside this save tranche.
    pub(crate) fn export_save_state(&self) -> Result<WorldSaveState, WorldSaveError> {
        for owner in 0..OWNER_SLOTS {
            if !self.objects.slot(owner).band(Band::Wall).is_empty()
                || (owner >= crate::objects::BANDED_SLOTS
                    && !self.objects.slot(owner).band(Band::Build).is_empty())
            {
                return Err(WorldSaveError::UnsupportedObjectBand);
            }
        }
        let state = WorldSaveState {
            units: self.units.clone(),
            unit_orders: self.unit_orders.clone(),
            unit_type_id: self.unit_type_id.clone(),
            move_step_x: self.move_step_x.clone(),
            move_step_y: self.move_step_y.clone(),
            handle_of_row: self.handle_of_row.clone(),
            generation: self.generation.clone(),
            active_slots: std::array::from_fn(|i| self.objects.is_active(i)),
            build_rows: std::array::from_fn(|i| self.objects.slot(i).band(Band::Build).to_vec()),
            object_bands: Some(
                self.object_bands
                    .snapshot()
                    .map_err(|_| WorldSaveError::RegistryMismatch)?,
            ),
            live: self.live,
            capacity: self.capacity,
            frame: self.frame,
            seconds: self.seconds,
            random_state: self.random.state(),
        };
        let (rebuilt, rebuilt_bands, _) = state.validate_and_rebuild()?;
        for owner in 0..OWNER_SLOTS {
            if rebuilt.is_active(owner) != self.objects.is_active(owner)
                || rebuilt.slot(owner).band(Band::Unit) != self.objects.slot(owner).band(Band::Unit)
                || rebuilt.slot(owner).band(Band::Build)
                    != self.objects.slot(owner).band(Band::Build)
            {
                return Err(WorldSaveError::RegistryMismatch);
            }
        }
        if rebuilt_bands
            .snapshot()
            .map_err(|_| WorldSaveError::RegistryMismatch)?
            != self
                .object_bands
                .snapshot()
                .map_err(|_| WorldSaveError::RegistryMismatch)?
        {
            return Err(WorldSaveError::RegistryMismatch);
        }
        let build_count: usize = state.build_rows.iter().map(Vec::len).sum();
        if self.objects.total_objects() != self.live as usize + build_count {
            return Err(WorldSaveError::RegistryMismatch);
        }
        Ok(state)
    }

    /// Atomically replace the save-owned state.
    ///
    /// Validation and all allocations occur before `self` is touched. On error the
    /// original world therefore remains byte-for-byte usable by the caller.
    pub(crate) fn import_save_state(
        &mut self,
        state: WorldSaveState,
    ) -> Result<(), WorldSaveError> {
        let (objects, object_bands, row_of_handle) = state.validate_and_rebuild()?;
        let replacement = World {
            units: state.units,
            unit_orders: state.unit_orders,
            unit_type_id: state.unit_type_id,
            move_step_x: state.move_step_x,
            move_step_y: state.move_step_y,
            objects,
            object_bands,
            traversal_buf: Vec::with_capacity(state.live as usize + 16),
            handle_of_row: state.handle_of_row,
            row_of_handle,
            generation: state.generation,
            live: state.live,
            capacity: state.capacity,
            frame: state.frame,
            seconds: state.seconds,
            random: Random::new(state.random_state),
            item_runtime: None,
            rules: self.rules.clone(),
            coverage: Coverage::default(),
        };
        *self = replacement;
        Ok(())
    }

    #[inline]
    pub fn coverage(&self) -> &Coverage {
        &self.coverage
    }

    /// Bytes of column storage this world reserves, whatever the population.
    pub fn bytes_reserved(&self) -> usize {
        let c = self.capacity as usize;
        self.units.bytes_reserved()
            + c * (4 * 3    // unit_type_id, move_step_x, move_step_y
                + 4 * 3) // handle_of_row, row_of_handle, generation
    }

    // ---- spawning ------------------------------------------------------------------

    /// Append a unit owned by `owner`, registered in that owner's unit band.
    ///
    /// Position, facing and speed are drawn from `game_random`. That is **scenario
    /// setup, not a mechanic**: it exists so a benchmark or a determinism test has a
    /// populated world, and it consumes the real LCG so the draw order is at least
    /// reproducible. Nothing about it claims to match how the engine places units.
    pub fn spawn(&mut self, owner: u8) -> Option<Handle> {
        self.spawn_typed(owner, 0)
    }

    /// [`World::spawn`] with an explicit type id, so the combat path has something to
    /// look up.
    pub fn spawn_typed(&mut self, owner: u8, type_id: i32) -> Option<Handle> {
        if self.live >= self.capacity || owner as usize >= OWNER_SLOTS {
            return None;
        }
        let row = self.units.push_zeroed()?;
        let id = self.handle_of_row[row];

        let o = self.objects.insert(owner as usize, Band::Unit, row as u32);

        // Four draws, in a fixed order, so the stream is reproducible.
        let x = self.random.get(0, MAP_SPAN);
        let y = self.random.get(0, MAP_SPAN);
        let angle = self.random.get(0, 0xFFFF) << 16;
        let speed = self.random.get(8, 64);

        let stats = self.type_stats(type_id).copied();
        self.units.x_internal_mut()[row] = x;
        self.units.y_internal_mut()[row] = y;
        self.units.angle_mut()[row] = angle;
        self.units.myspeed_mut()[row] = speed as i16;
        self.units.set_flags(row, OBJ_FLAG_ACTIVE);
        self.units.set_who(row, owner);
        self.units.o_mut()[row] = o as i16;
        self.units.set_uid(row, (id & 0xFFFF) as u16);
        self.units.o_up_mut()[row] = -1;
        self.units.inside_up_mut()[row] = -1;
        self.units.inside_up_who_mut()[row] = -1;
        self.units.tolerance_mut()[row] = SUBTILE;
        self.units.myhits_mut()[row] = stats.map_or(100, |s| s.hits.max(1));
        self.units.myarmor_mut()[row] = stats.map_or(0, |s| {
            s.armor.clamp(i16::MIN as i32, i16::MAX as i32) as i16
        });

        self.unit_orders.push(OrderList::new());
        self.unit_type_id.push(type_id);
        self.move_step_x.push(0);
        self.move_step_y.push(0);

        self.row_of_handle[id as usize] = row as u32;
        self.live += 1;
        let handle = Handle {
            id,
            generation: self.generation[id as usize],
        };
        let mirror = self
            .object_bands
            .mirror_dense_append(
                owner,
                RetailBand::Unit,
                WorldObjectIdentity::Unit {
                    id: handle.id,
                    generation: handle.generation,
                },
            )
            .expect("spawn committed a gap-free dense Unit append");
        assert_eq!(mirror.address.o, o as i32);
        Some(handle)
    }

    /// Allocate one live runtime unit at an exact caller-supplied position without
    /// consuming `game_random`.
    ///
    /// Unlike [`World::spawn_typed`], this is not a scenario-population helper: it is the
    /// storage boundary behind retail `Objects::init_unit` callers that already resolved
    /// their coordinates. Motion starts at rest; later placement/order transactions own
    /// stance, containment, launch, and movement initialization.
    pub fn allocate_typed_at(&mut self, owner: u8, type_id: i32, x: i32, y: i32) -> Option<Handle> {
        if self.live >= self.capacity || owner as usize >= OWNER_SLOTS {
            return None;
        }
        let row = self.units.push_zeroed()?;
        let id = self.handle_of_row[row];
        let o = self.objects.insert(owner as usize, Band::Unit, row as u32);
        let stats = self.type_stats(type_id).copied();

        self.units.x_internal_mut()[row] = x;
        self.units.y_internal_mut()[row] = y;
        self.units.angle_mut()[row] = 0;
        self.units.myspeed_mut()[row] = 0;
        self.units.set_flags(row, OBJ_FLAG_ACTIVE);
        self.units.set_who(row, owner);
        self.units.o_mut()[row] = o as i16;
        self.units.set_uid(row, (id & 0xffff) as u16);
        self.units.o_up_mut()[row] = -1;
        self.units.inside_up_mut()[row] = -1;
        self.units.inside_up_who_mut()[row] = -1;
        self.units.tolerance_mut()[row] = SUBTILE;
        self.units.myhits_mut()[row] = stats.map_or(100, |s| s.hits.max(1));
        self.units.myarmor_mut()[row] = stats.map_or(0, |s| {
            s.armor.clamp(i16::MIN as i32, i16::MAX as i32) as i16
        });

        self.unit_orders.push(OrderList::new());
        self.unit_type_id.push(type_id);
        self.move_step_x.push(0);
        self.move_step_y.push(0);
        self.row_of_handle[id as usize] = row as u32;
        self.live += 1;
        let handle = Handle {
            id,
            generation: self.generation[id as usize],
        };
        let mirror = self
            .object_bands
            .mirror_dense_append(
                owner,
                RetailBand::Unit,
                WorldObjectIdentity::Unit {
                    id: handle.id,
                    generation: handle.generation,
                },
            )
            .expect("allocation committed a gap-free dense Unit append");
        assert_eq!(mirror.address.o, o as i32);
        Some(handle)
    }

    fn type_stats(&self, type_id: i32) -> Option<&UnitTypeStats> {
        if type_id <= 0 {
            return None;
        }
        self.rules.unit_stats.iter().find(|s| s.type_id == type_id)
    }

    /// Row currently holding `h`, or `None` if the handle is stale.
    ///
    /// Two independent checks. The generation catches an id that was freed and reissued;
    /// the row/id round trip catches an id that is simply dead, without a liveness flag.
    #[inline]
    pub fn row_of(&self, h: Handle) -> Option<usize> {
        let id = h.id as usize;
        if id >= self.capacity as usize || self.generation[id] != h.generation {
            return None;
        }
        let row = self.row_of_handle[id] as usize;
        if row >= self.live as usize || self.handle_of_row[row] != h.id {
            return None;
        }
        Some(row)
    }

    #[inline]
    pub fn is_alive(&self, h: Handle) -> bool {
        self.row_of(h).is_some()
    }

    /// Remove a unit, swapping the last live row into its place.
    ///
    /// Three structures move together and all three are load-bearing: the generated
    /// columns, the side vectors, and the owner's object band. A miss in any one leaves a
    /// unit addressable by `(who, o)` that no longer exists.
    pub fn despawn(&mut self, h: Handle) -> bool {
        let Some(row) = self.row_of(h) else {
            return false;
        };
        let last = self.live as usize - 1;

        // 1. Unregister from the owner's band. If another entry moved into the hole, its
        //    engine-visible `o` changed and its column must say so.
        let who = self.units.get_who(row) as usize;
        let o = self.units.o()[row] as u32;
        let dense_moved = self.objects.remove(who, Band::Unit, o);
        if let Some((moved_row, new_o)) = dense_moved {
            self.units.o_mut()[moved_row as usize] = new_o as i16;
        }
        let sparse_removed = self
            .object_bands
            .mirror_dense_swap_remove(
                RetailObjectAddress::new(who as u8, RetailBand::Unit, o as i32),
                WorldObjectIdentity::Unit {
                    id: h.id,
                    generation: h.generation,
                },
            )
            .expect("despawn mirrored one gap-free dense Unit removal");
        match (dense_moved, sparse_removed.moved) {
            (None, None) => {}
            (Some((moved_row, new_o)), Some((moved_identity, moved_address))) => {
                let moved_id = self.handle_of_row[moved_row as usize];
                assert_eq!(
                    moved_identity,
                    WorldObjectIdentity::Unit {
                        id: moved_id,
                        generation: self.generation[moved_id as usize],
                    }
                );
                assert_eq!(moved_address.o, new_o as i32);
            }
            _ => panic!("dense and sparse Unit swap-remove receipts diverged"),
        }

        // 2. Compact the columns.
        if row != last {
            self.units.copy_row(row, last);
            self.unit_orders.swap_remove(row);
            self.unit_type_id.swap_remove(row);
            self.move_step_x.swap_remove(row);
            self.move_step_y.swap_remove(row);
            // The unit that moved into `row` is still registered against `last`.
            let mwho = self.units.get_who(row) as usize;
            let mo = self.units.o()[row] as u32;
            self.objects.repoint(mwho, Band::Unit, mo, row as u32);
            let moved = self.handle_of_row[last];
            self.handle_of_row[row] = moved;
            self.row_of_handle[moved as usize] = row as u32;
        } else {
            self.unit_orders.pop();
            self.unit_type_id.pop();
            self.move_step_x.pop();
            self.move_step_y.pop();
        }
        self.units.pop();

        self.handle_of_row[last] = h.id;
        self.live -= 1;
        let id = h.id as usize;
        self.generation[id] = self.generation[id].wrapping_add(1);
        true
    }

    // ---- orders --------------------------------------------------------------------

    #[inline]
    pub fn orders(&self, row: usize) -> &OrderList {
        &self.unit_orders[row]
    }

    #[inline]
    pub fn orders_mut(&mut self, row: usize) -> &mut OrderList {
        &mut self.unit_orders[row]
    }

    /// Install an order, replacing whatever was queued — what an un-shifted command does.
    pub fn issue(&mut self, h: Handle, o: Order) -> bool {
        let Some(row) = self.row_of(h) else {
            return false;
        };
        self.unit_orders[row].replace(o);
        true
    }

    // ---- column views ---------------------------------------------------------------
    //
    // Named for the PDB fields they are, with the old placeholder spellings kept where a
    // real field corresponds. `vel_x`/`vel_y` are gone: `UnitData` stores a facing and a
    // speed, not a velocity.

    #[inline]
    pub fn pos_x(&self) -> &[i32] {
        self.units.x_internal()
    }
    #[inline]
    pub fn pos_y(&self) -> &[i32] {
        self.units.y_internal()
    }
    /// `UnitData::angle` at +80, a binary angle (2^32 = one turn).
    #[inline]
    pub fn angle(&self) -> &[i32] {
        self.units.angle()
    }
    /// `UnitData::myspeed` at +154.
    #[inline]
    pub fn myspeed(&self) -> &[i16] {
        self.units.myspeed()
    }
    /// `ObjectData::myhits` at +32.
    #[inline]
    pub fn hits(&self) -> &[i32] {
        self.units.myhits()
    }
    #[inline]
    pub fn hits_mut(&mut self) -> &mut [i32] {
        self.units.myhits_mut()
    }
    /// `ObjectData::hold_frames` at +50 — the counter `Objects::process_all` decrements
    /// for inactive objects.
    #[inline]
    pub fn cooldown(&self) -> &[i16] {
        self.units.hold_frames()
    }
    #[inline]
    pub fn cooldown_mut(&mut self) -> &mut [i16] {
        self.units.hold_frames_mut()
    }
    /// The derived per-frame movement step. Not a PDB field; see the field docs.
    #[inline]
    pub fn move_step_x(&self) -> &[i32] {
        &self.move_step_x[..self.live as usize]
    }
    #[inline]
    pub fn move_step_y(&self) -> &[i32] {
        &self.move_step_y[..self.live as usize]
    }
    /// `SubObjectData::who` at +9 — the owner slot.
    #[inline]
    pub fn owner(&self) -> &[i8] {
        self.units.who()
    }
    #[inline]
    pub fn handles(&self) -> &[u32] {
        &self.handle_of_row[..self.live as usize]
    }
    /// Stable generational identity for a live dense row.
    ///
    /// Product adapters use this after loading an opaque save: the public row/id columns
    /// alone cannot safely reconstruct the generation component of a [`Handle`].
    #[inline]
    pub fn handle_at_row(&self, row: usize) -> Option<Handle> {
        if row >= self.live as usize {
            return None;
        }
        let id = self.handle_of_row[row];
        Some(Handle {
            id,
            generation: self.generation[id as usize],
        })
    }
    /// The whole id permutation, live region followed by the free pool.
    #[inline]
    pub fn all_handle_ids(&self) -> &[u32] {
        &self.handle_of_row
    }

    #[inline]
    pub fn set_pos(&mut self, row: usize, x: i32, y: i32) {
        debug_assert!(row < self.live as usize);
        self.units.x_internal_mut()[row] = x;
        self.units.y_internal_mut()[row] = y;
    }

    /// Set the cached movement step directly, for tests and the lane-major experiment.
    pub fn set_move_step(&mut self, row: usize, sx: i32, sy: i32) {
        self.move_step_x[row] = sx;
        self.move_step_y[row] = sy;
    }

    pub fn advance_frames(&mut self, n: i32) {
        self.frame = self.frame.wrapping_add(n);
    }

    // ---- the tick -------------------------------------------------------------------

    /// `Game::do_frame` `0x00591EF0`, in retail's own call order.
    ///
    /// Every entry of [`DO_FRAME`] is visited and counted. Steps whose bodies are not
    /// ported still increment their counter, so [`World::coverage`] reports what a run
    /// actually exercised instead of what it was hoped to exercise.
    pub fn step(&mut self) {
        // 0..7: autosave, logging, the debug-lag draw, speed commands, scripts, CTW,
        // tutorial, Steam. None have simulation bodies we model.
        // 8..13: Leaders::process_all, NetDaemon, diplomacy chat, Leaders::strategy_all,
        // GameDaemon::process_all, Armies::process_all — all stubs for now.
        for s in 0..14 {
            self.coverage.schedule.enter(s);
        }

        // 14 — the one that matters.
        self.coverage.schedule.enter(14);
        self.objects_process_all();

        // 15 Objects::inc_time, 16 GraphicEvents, 17 end_process_all, 18 Achieve,
        // 19 process_event_frame.
        for s in 15..20 {
            self.coverage.schedule.enter(s);
        }

        // 20 — Game::frame++ happens HERE, after the object pass.
        self.coverage.schedule.enter(20);
        self.frame = self.frame.wrapping_add(1);

        // 21 OrdersMemManager::cycle, 22 stray roads.
        self.coverage.schedule.enter(21);
        self.coverage.schedule.enter(22);

        // 23 — 15 frames is one game second.
        self.coverage.schedule.enter(23);
        if self.frame % FRAMES_PER_SECOND == 0 {
            self.seconds = self.seconds.wrapping_add(1);
        }

        // 24 cannon time, 25 autosave, 26 log, 27 end game, 28 capture.
        for s in 24..DO_FRAME.len() {
            self.coverage.schedule.enter(s);
        }
    }

    /// `Objects::process_all` `0x0065DCE0`.
    ///
    /// The traversal order comes from the canonical sparse owner: `(frame + i) % 10` for
    /// Units, then fixed eight-slot Build/Wall order. Unit tombstones remain in the walk so
    /// their inactive hold counter advances without inventing a dense row.
    ///
    /// # A recorded divergence
    ///
    /// Retail spawns wildlife every 32 frames and every 64 frames steps one herd, and the
    /// wildlife spawn **draws `game_random`** — up to two draws per placement attempt,
    /// gated on a terrain test we have no map for. Drawing the wrong number of values
    /// would be worse than drawing none, so we draw none and count the frames in
    /// [`Coverage::wildlife_draws_skipped`]. Every one of those frames is a point where
    /// our RNG stream leaves retail's. This is the single largest known obstacle to a
    /// stream-faithful tick and it belongs to the worldgen/terrain lane.
    fn objects_process_all(&mut self) {
        let frame = self.frame;
        // Move the buffer out so the loop can mutate `self` while walking it; it goes
        // straight back at the end, so the allocation survives the frame.
        let mut order = std::mem::take(&mut self.traversal_buf);
        self.object_bands.traversal_into(frame, &mut order);
        for entry in order.iter().copied() {
            match (entry.address.band, entry.lifecycle) {
                (
                    RetailBand::Unit,
                    SparseSlotLifecycle::Live(WorldObjectIdentity::Unit { id, generation }),
                ) => {
                    let Some(row) = self.row_of(Handle { id, generation }) else {
                        continue;
                    };
                    if self.units.get_flags(row) & OBJ_FLAG_ACTIVE != 0 {
                        self.unit_process(row);
                    } else {
                        let hf = self.units.get_hold_frames(row);
                        if hf != 0 {
                            self.units.set_hold_frames(row, hf - 1);
                            self.coverage.hold_decrements += 1;
                        }
                    }
                }
                (RetailBand::Unit, SparseSlotLifecycle::Tombstone(facts)) => {
                    if facts.flags & OBJ_FLAG_ACTIVE == 0 && facts.hold_frames != 0 {
                        self.tick_unit_tombstone_hold(entry.address)
                            .expect("traversal entry remains the same Unit tombstone");
                        self.coverage.hold_decrements += 1;
                    }
                }
                (
                    RetailBand::Build,
                    SparseSlotLifecycle::Live(WorldObjectIdentity::BuildRow(_)),
                ) => self.coverage.build_process += 1,
                (RetailBand::Wall, SparseSlotLifecycle::Live(WorldObjectIdentity::WallRow(_))) => {
                    self.coverage.wall_process += 1
                }
                (_, SparseSlotLifecycle::Reserved { .. }) => {
                    panic!("live World traversal observed an outstanding object reservation")
                }
                // Phase 2 owns Unit tombstones only. Build/Wall tombstones and wrong-band live
                // identities remain unadmitted by save/import and have no runtime body here.
                _ => {}
            }
        }
        self.traversal_buf = order;
        if frame % WILDLIFE_PERIOD == 0 {
            self.coverage.wildlife_draws_skipped += 1;
        }
        let _ = HERD_PERIOD;
    }

    /// `Unit::process` `0x00610BC0`, vtable slot 39 (+0x9C).
    ///
    /// Retail's body is attrition, spells, healing, cloak, supply, then `Unit::work`
    /// (slot 98, +0x188) and finally `Guy::process`. Only the last two carry ported
    /// behaviour; the five preludes are absent and the coverage report says so.
    fn unit_process(&mut self, row: usize) {
        self.coverage.unit_process += 1;
        self.unit_work(row);
        self.guy_process(row);
    }

    /// `Unit::work` `0x0060D180` -> `Unit::do_job` `0x00617A10`.
    ///
    /// The dispatch is the 28-entry jump table at `0x00617B94`, indexed directly by the
    /// head order's type. Arms without a ported body still record the dispatch.
    fn unit_work(&mut self, row: usize) {
        self.coverage.unit_work += 1;
        let kind = self.unit_orders[row].order_type();
        let status = self.coverage.orders.record(kind);
        if status == ArmStatus::Unimplemented {
            return;
        }
        match kind {
            // Arm 0: the virtual at [unit+0x184]. An idle unit holds position.
            OrderIndex::None => {
                self.move_step_x[row] = 0;
                self.move_step_y[row] = 0;
            }
            // Arms 1 and 4 are the same executor, `Unit::do_move` 0x005F7B30 [measured].
            OrderIndex::MoveTo | OrderIndex::FleeTo => self.do_move(row),
            OrderIndex::Attack => self.do_attack(row),
            // Arm 5 falls to the default arm and does nothing. Faithfully empty.
            OrderIndex::Patrol => {}
            _ => {}
        }
    }

    /// `Unit::do_move` `0x005F7B30` — the funnel every locomotion order reaches.
    ///
    /// # Fidelity, stated precisely
    ///
    /// The *primitives* are [measured] ports: `find_angle` `0x0092D130` and `sinx`/`cosx`
    /// `0x0092D100`/`0x0092D0C0`. The *logic around them* is not: retail's `do_move` is
    /// 4,582 bytes over a `Stack<PathData>`, `vector_dist`, `is_in_range`, collision
    /// resolution and four pathfinder entry points. What is here is a straight-line walk
    /// toward the order's destination that stops inside `tolerance`. Treat it as a
    /// movement *placeholder wearing derived trig*; [`crate::systems::movement`] holds
    /// the real pathfinder and is what this should be rewired to.
    fn do_move(&mut self, row: usize) {
        let Some(ord) = self.unit_orders[row].current().cloned() else {
            self.move_step_x[row] = 0;
            self.move_step_y[row] = 0;
            return;
        };
        let x = self.units.x_internal()[row];
        let y = self.units.y_internal()[row];
        let dx = ord.x.wrapping_sub(x);
        let dy = ord.y.wrapping_sub(y);
        let tol = if ord.tolerance > 0 {
            ord.tolerance
        } else {
            self.units.tolerance()[row]
        };
        let d2 = (dx as i64) * (dx as i64) + (dy as i64) * (dy as i64);
        if d2 <= (tol as i64) * (tol as i64) {
            self.unit_orders[row].kill_current();
            self.move_step_x[row] = 0;
            self.move_step_y[row] = 0;
            self.units.set_idle(row, 1);
            return;
        }
        let angle = find_angle(dx, dy);
        self.units.angle_mut()[row] = angle;
        self.units.set_idle(row, 0);
        let speed = self.units.myspeed()[row] as i32;
        self.move_step_x[row] = sinx(angle, speed);
        // Angle 0 points at -y, so the y component is the negated cosine.
        self.move_step_y[row] = -cosx(angle, speed);
    }

    /// `Unit::do_attack` `0x005F1B80` — range gate, recharge, and the damage pipeline.
    ///
    /// # Fidelity, stated precisely
    ///
    /// The **arithmetic** is [`crate::mechanics::damage`], the port of
    /// `ObjectData::get_damage` `0x00644130`, fed with the real balance-table entry from
    /// `Balance::final_balance_table` `0x00C12BF4`. The **predicates** are all default
    /// (false), because resolving them means walking an object graph this world does not
    /// model — so no modifier branch fires and the number is the spine of the chain only.
    /// The **range and recharge gate** around it is ours, not derived.
    ///
    /// Real formula, real table, no modifiers, invented gating. Tier C as a whole.
    fn do_attack(&mut self, row: usize) {
        let Some(ord) = self.unit_orders[row].current().cloned() else {
            return;
        };
        if ord.target_who < 0 || ord.target_o < 0 {
            self.unit_orders[row].kill_current();
            return;
        }
        let recharging = self.units.get_recharging(row);
        if recharging > 0 {
            self.units.set_recharging(row, recharging - 1);
            return;
        }
        let Some(balance) = self.rules.balance.clone() else {
            self.coverage.damage_skipped_no_tables += 1;
            return;
        };
        let Some(trow) = self.unit_row_at(i32::from(ord.target_who), i32::from(ord.target_o))
        else {
            self.unit_orders[row].kill_current();
            return;
        };
        if trow >= self.live as usize || trow == row {
            self.unit_orders[row].kill_current();
            return;
        }

        let atk_type = self.unit_type_id[row];
        let def_type = self.unit_type_id[trow];
        let (Some(atk), Some(def)) = (
            self.type_stats(atk_type).copied(),
            self.type_stats(def_type).copied(),
        ) else {
            self.coverage.damage_skipped_no_tables += 1;
            return;
        };

        let dx = self.units.x_internal()[trow].wrapping_sub(self.units.x_internal()[row]);
        let dy = self.units.y_internal()[trow].wrapping_sub(self.units.y_internal()[row]);
        let d2 = (dx as i64) * (dx as i64) + (dy as i64) * (dy as i64);
        let r = atk.max_range as i64;
        if d2 > r * r {
            // Out of range: close. Retail reaches `do_move` through `check_target_path`;
            // this is the same *shape*, not the same code.
            let angle = find_angle(dx, dy);
            self.units.angle_mut()[row] = angle;
            let speed = self.units.myspeed()[row] as i32;
            self.move_step_x[row] = sinx(angle, speed);
            self.move_step_y[row] = -cosx(angle, speed);
            return;
        }
        self.move_step_x[row] = 0;
        self.move_step_y[row] = 0;

        let Some(balance_pct) = balance.get(atk.type_id, def.type_id) else {
            self.coverage.damage_skipped_no_tables += 1;
            return;
        };
        let input = crate::mechanics::DamageInput {
            balance_pct,
            attack: crate::mechanics::get_attack(atk.attack, false, 0, 0),
            armor: crate::mechanics::get_armor(def.armor, false, 0, 0),
            attack_dir: find_angle(dx, dy),
            attacker_player: self.units.get_who(row) as u32,
            attacker_type_id: atk.type_id,
            defender_type_id: def.type_id,
            defender_facing: self.units.angle()[trow],
            defender_facing_entrench: self.units.angle()[trow],
            current_frame: self.frame,
            ..Default::default()
        };
        let d = crate::mechanics::damage(
            &input,
            &crate::mechanics::DamagePredicates::default(),
            &self.rules.combat,
            &crate::mechanics::UnreachedTerms::default(),
        );
        let hp = self.units.myhits()[trow].wrapping_sub(d);
        self.units.myhits_mut()[trow] = hp;
        self.units
            .set_recharging(row, atk.recharge.clamp(0, 255) as u8);
        self.coverage.damage_applied += 1;
        if hp <= 0 {
            // Clearing the active bit is what stops `Objects::process_all` ticking it;
            // actual removal is `Objects::kill_object`, which is not ported.
            let f = self.units.get_flags(trow);
            self.units.set_flags(trow, f & !OBJ_FLAG_ACTIVE);
        }
    }

    /// `Guy::process` `0x005E0230` -> `Guy::move` `0x005D9240`, the position integrator.
    #[inline]
    fn guy_process(&mut self, row: usize) {
        self.coverage.guy_move += 1;
        let x = self.units.x_internal()[row].wrapping_add(self.move_step_x[row]);
        let y = self.units.y_internal()[row].wrapping_add(self.move_step_y[row]);
        self.units.x_internal_mut()[row] = x.rem_euclid(MAP_SPAN);
        self.units.y_internal_mut()[row] = y.rem_euclid(MAP_SPAN);
    }

    // ---- the element-wise subset, for layout measurement ---------------------------

    /// The tick's **element-wise subset**: integrate every live unit's position by its
    /// cached step, then decrement every `hold_frames`.
    ///
    /// This is **not** `Game::do_frame` and does not claim to be. It exists because the
    /// batch-layout question — does lane-major beat world-dense — is only meaningful on
    /// arithmetic with no per-unit control flow, and this is that arithmetic. It is what
    /// [`crate::LaneBatch`] mirrors, so comparing the two compares layouts, not
    /// simulations.
    pub fn step_hot(&mut self) {
        let n = self.live as usize;
        simd::integrate_wrap(
            self.units.x_internal_mut(),
            &self.move_step_x[..n],
            MAP_SPAN,
        );
        simd::integrate_wrap(
            self.units.y_internal_mut(),
            &self.move_step_y[..n],
            MAP_SPAN,
        );
        simd::tick_down(self.units.hold_frames_mut());
        self.frame = self.frame.wrapping_add(1);
    }

    /// [`World::step_hot`] forced through the scalar reference kernels.
    pub fn step_scalar_reference(&mut self) {
        let n = self.live as usize;
        simd::integrate_wrap_scalar(
            self.units.x_internal_mut(),
            &self.move_step_x[..n],
            MAP_SPAN,
        );
        simd::integrate_wrap_scalar(
            self.units.y_internal_mut(),
            &self.move_step_y[..n],
            MAP_SPAN,
        );
        simd::tick_down_scalar(self.units.hold_frames_mut());
        self.frame = self.frame.wrapping_add(1);
    }

    /// [`World::step_hot`] through the explicitly vectorised kernels.
    pub fn step_hand_simd(&mut self) {
        let n = self.live as usize;
        simd::hand::integrate_wrap(
            self.units.x_internal_mut(),
            &self.move_step_x[..n],
            MAP_SPAN,
        );
        simd::hand::integrate_wrap(
            self.units.y_internal_mut(),
            &self.move_step_y[..n],
            MAP_SPAN,
        );
        simd::hand::tick_down(self.units.hold_frames_mut());
        self.frame = self.frame.wrapping_add(1);
    }

    // ---- checksum ------------------------------------------------------------------

    /// Digest of sim-critical state, **defined by the engine's own walker**.
    ///
    /// The field set is not chosen here: it is every `UnitData` field that
    /// `Unit::walk_data` `0x0060CF40` visits and that this crate materialises, read out of
    /// the generated descriptor table at run time. That is the same set `CheckSum`,
    /// `SaveGame` and `LoadGame` walk, because they are the only `DataWalk`
    /// implementations.
    ///
    /// Per-unit hashes are combined commutatively and each is seeded with the unit's
    /// handle, so the digest is invariant under row compaction — a unit changing row is
    /// not a state change — while still separating two units that swapped attributes.
    ///
    /// It is **not** `adler32` over the engine's byte image, so it is not comparable with
    /// a retail checksum. Making it comparable needs the byte-exact walk order, which the
    /// generated `WALK_OPS` now carries but nothing consumes yet.
    pub fn digest(&self) -> u64 {
        let n = self.live as usize;
        let mut acc: u64 = 0;
        for row in 0..n {
            let mut h: u64 = 0xcbf2_9ce4_8422_2325;
            macro_rules! mix {
                ($v:expr) => {{
                    h ^= $v as u64;
                    h = h.wrapping_mul(0x0000_0100_0000_01B3);
                }};
            }
            mix!(self.handle_of_row[row]);
            for f in unit::FIELDS.iter() {
                if f.alias_of.is_some() || f.walked != Some(true) {
                    continue;
                }
                let p = f.plane as usize;
                let k = f.count as usize;
                match f.pool {
                    crate::generated::state::Pool::W4 => {
                        if k == 1 {
                            mix!(self.units.w4_slice(p)[row] as u32);
                        } else {
                            for &v in self.units.w4_arr(p, row, k) {
                                mix!(v as u32);
                            }
                        }
                    }
                    crate::generated::state::Pool::W2 => {
                        if k == 1 {
                            mix!(self.units.w2_slice(p)[row] as u16);
                        } else {
                            for &v in self.units.w2_arr(p, row, k) {
                                mix!(v as u16);
                            }
                        }
                    }
                    crate::generated::state::Pool::W1 => {
                        if k == 1 {
                            mix!(self.units.w1_slice(p)[row] as u8);
                        } else {
                            for &v in self.units.w1_arr(p, row, k) {
                                mix!(v as u8);
                            }
                        }
                    }
                    _ => {}
                }
            }
            // The order list is walked state too (`UnitData::orderlist` at +200).
            mix!(self.unit_orders[row].len() as u32);
            if let Some(o) = self.unit_orders[row].current() {
                mix!(o.kind.index() as u32);
                mix!(o.x as u32);
                mix!(o.y as u32);
                mix!(o.flags);
            }
            acc = acc.wrapping_add(h);
        }
        let mut out = acc ^ self.object_bands_digest().rotate_left(23);
        out ^= self.frame as u32 as u64;
        out = out.wrapping_mul(0x0000_0100_0000_01B3);
        out ^ (self.live as u64)
    }

    fn object_bands_digest(&self) -> u64 {
        #[inline]
        fn mix(hash: &mut u64, value: u64) {
            *hash ^= value;
            *hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
        }

        let snapshot = self
            .object_bands
            .snapshot()
            .expect("live World never exposes an outstanding sparse reservation");
        let mut hash = 0xcbf2_9ce4_8422_2325;
        mix(&mut hash, 0x4f42_4a53_5041_5253); // "OBJSPARS" domain separator.
        for (owner, active) in snapshot.active.into_iter().enumerate() {
            mix(&mut hash, owner as u64);
            mix(&mut hash, u64::from(active));
        }
        for (owner, owner_state) in snapshot.owners.into_iter().enumerate() {
            for (band, band_state) in owner_state.bands.into_iter().enumerate() {
                mix(&mut hash, owner as u64);
                mix(&mut hash, band as u64);
                mix(&mut hash, band_state.mark as u32 as u64);
                mix(&mut hash, band_state.slots.len() as u64);
                for lifecycle in band_state.slots {
                    match lifecycle {
                        SnapshotLifecycle::Tombstone(facts) => {
                            mix(&mut hash, 0);
                            mix(&mut hash, facts.flags as u64);
                            mix(&mut hash, facts.hold_frames as u64);
                            mix(&mut hash, u64::from(facts.is_unit));
                            mix(&mut hash, facts.o_up as u16 as u64);
                        }
                        SnapshotLifecycle::Live(WorldObjectIdentity::Unit { id, generation }) => {
                            mix(&mut hash, 1);
                            mix(&mut hash, id as u64);
                            mix(&mut hash, generation as u64);
                        }
                        SnapshotLifecycle::Live(WorldObjectIdentity::BuildRow(row)) => {
                            mix(&mut hash, 2);
                            mix(&mut hash, row as u64);
                        }
                        SnapshotLifecycle::Live(WorldObjectIdentity::WallRow(row)) => {
                            mix(&mut hash, 3);
                            mix(&mut hash, row as u64);
                        }
                    }
                }
            }
        }
        hash
    }

    /// `(materialised walked fields, walked fields, materialised walked bytes, walked
    /// bytes)` for `UnitData`.
    ///
    /// Reported so "the checksum covers sim-critical state" is a number rather than a
    /// claim.
    pub fn digest_field_coverage() -> (usize, usize, u32, u32) {
        let mut fields = 0usize;
        let mut bytes = 0u32;
        let mut walked_fields = 0usize;
        let mut walked_bytes = 0u32;
        for f in unit::FIELDS.iter() {
            if f.alias_of.is_some() || f.walked != Some(true) {
                continue;
            }
            walked_fields += 1;
            walked_bytes += f.size;
            if f.repr.materialised() {
                fields += 1;
                bytes += f.size;
            }
        }
        (fields, walked_fields, bytes, walked_bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_and_despawn_track_occupancy() {
        let mut w = World::new(7);
        assert_eq!(w.live_count(), 0);
        let a = w.spawn(0).unwrap();
        let b = w.spawn(1).unwrap();
        assert_eq!(w.live_count(), 2);
        assert!(w.despawn(a));
        assert_eq!(w.live_count(), 1);
        assert!(!w.despawn(a), "double despawn must be a no-op");
        assert!(w.despawn(b));
        assert_eq!(w.live_count(), 0);
    }

    #[test]
    fn capacity_is_bounded_and_handles_recycle() {
        let mut w = World::with_capacity(256, 1);
        let mut hs = Vec::new();
        for _ in 0..256 {
            hs.push(w.spawn(0).expect("within capacity"));
        }
        assert!(w.spawn(0).is_none(), "must not exceed fixed capacity");
        assert!(w.despawn(hs[0]));
        assert!(w.spawn(0).is_some(), "freed handle must be reusable");
    }

    #[test]
    fn stale_handles_are_rejected_after_reuse() {
        let mut w = World::with_capacity(8, 3);
        let a = w.spawn(0).unwrap();
        w.spawn(1).unwrap();
        assert!(w.despawn(a));
        let c = w.spawn(2).unwrap();
        assert_eq!(c.id, a.id, "id is recycled");
        assert_ne!(c.generation, a.generation, "generation must move");
        assert!(!w.is_alive(a));
        assert!(!w.despawn(a));
        assert!(w.is_alive(c));
    }

    #[test]
    fn live_rows_expose_their_exact_generational_handle() {
        let mut w = World::with_capacity(4, 9);
        let first = w.spawn(0).unwrap();
        let second = w.spawn(1).unwrap();
        assert_eq!(w.handle_at_row(0), Some(first));
        assert_eq!(w.handle_at_row(1), Some(second));
        assert_eq!(w.handle_at_row(2), None);

        assert!(w.despawn(first));
        assert_eq!(
            w.handle_at_row(0),
            Some(second),
            "compacted rows keep identity"
        );
        let recycled = w.spawn(2).unwrap();
        assert_eq!(w.handle_at_row(1), Some(recycled));
        assert_ne!(recycled.generation, first.generation);
    }

    /// The engine-visible `(who, o)` address must survive every despawn, for every
    /// survivor. This is the invariant a swap-remove is most likely to break, and losing
    /// it means an order can name an object that no longer exists.
    #[test]
    fn the_engine_object_address_stays_consistent_under_churn() {
        let mut w = World::with_capacity(96, 0xBEE5);
        let mut alive: Vec<Handle> = Vec::new();
        let mut rng: u64 = 1;
        for round in 0..500 {
            rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
            let r = (rng >> 33) as usize;
            if alive.len() < 96 && (alive.is_empty() || (r & 3) != 0) {
                if let Some(h) = w.spawn((round % 4) as u8) {
                    alive.push(h);
                }
            } else if !alive.is_empty() {
                let h = alive.swap_remove(r % alive.len());
                assert!(w.despawn(h));
            }
            assert_eq!(w.live_count() as usize, alive.len());
            for row in 0..w.live_count() as usize {
                let who = w.units.get_who(row) as usize;
                let o = w.units.o()[row] as u32;
                assert_eq!(
                    w.objects
                        .slot(who)
                        .band(Band::Unit)
                        .get(o as usize)
                        .copied(),
                    Some(row as u32),
                    "round {round}: row {row} says it is ({who}, {o}) but the band disagrees"
                );
                assert_eq!(
                    w.unit_row_at(who as i32, o as i32),
                    Some(row),
                    "round {round}: sparse lookup disagrees for ({who}, {o})"
                );
            }
            let total: usize = (0..OWNER_SLOTS)
                .map(|s| w.objects.band_len(s, Band::Unit))
                .sum();
            assert_eq!(
                total,
                alive.len(),
                "round {round}: registry and world disagree"
            );
        }
    }

    #[test]
    fn handle_ids_remain_a_permutation_under_churn() {
        let cap = 64;
        let mut w = World::with_capacity(cap, 0xF00D);
        let mut alive: Vec<Handle> = Vec::new();
        let mut rng: u64 = 9;
        for round in 0..300 {
            rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
            let r = (rng >> 33) as usize;
            if alive.len() < cap && (alive.is_empty() || (r & 3) != 0) {
                if let Some(h) = w.spawn((round % 4) as u8) {
                    alive.push(h);
                }
            } else if !alive.is_empty() {
                let h = alive.swap_remove(r % alive.len());
                assert!(w.despawn(h));
            }
            let mut ids = w.all_handle_ids().to_vec();
            ids.sort_unstable();
            assert!(
                ids.iter().copied().eq(0..cap as u32),
                "id permutation broken at {round}"
            );
        }
    }

    #[test]
    fn stepping_is_deterministic_for_a_given_seed() {
        let run = || {
            let mut w = World::with_capacity(64, 0xDEAD_BEEF);
            for _ in 0..64 {
                w.spawn(0);
            }
            for _ in 0..500 {
                w.step();
            }
            w.digest()
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn different_seeds_diverge() {
        let run = |s| {
            let mut w = World::with_capacity(64, s);
            for _ in 0..64 {
                w.spawn(0);
            }
            for _ in 0..100 {
                w.step();
            }
            w.digest()
        };
        assert_ne!(run(1), run(2));
    }

    /// The frame counter increments at step 20, so the rotation `Objects::process_all`
    /// uses is the *pre-increment* frame. Getting this backwards puts every owner slot
    /// one frame out of phase with retail.
    #[test]
    fn the_owner_rotation_uses_the_pre_increment_frame() {
        let mut w = World::with_capacity(16, 5);
        for s in 0..4u8 {
            w.spawn(s);
        }
        assert_eq!(w.objects.traversal(w.frame)[0].0, 0);
        w.step();
        assert_eq!(w.frame, 1);
        assert_eq!(
            w.objects.traversal(w.frame)[0].0,
            1,
            "rotation must advance by one"
        );
    }

    /// Every schedule entry must be visited every frame, or the coverage report is a lie.
    #[test]
    fn every_do_frame_step_is_entered_once_per_tick() {
        let mut w = World::with_capacity(4, 1);
        w.spawn(0);
        for _ in 0..10 {
            w.step();
        }
        for (i, s) in DO_FRAME.iter().enumerate() {
            assert_eq!(
                w.coverage().schedule.entered[i],
                10,
                "step {i} ({}) miscounted",
                s.name
            );
        }
    }

    /// A move order takes the unit toward its destination and then clears itself.
    #[test]
    fn a_move_order_converges_and_then_clears_itself() {
        let mut w = World::with_capacity(4, 42);
        let h = w.spawn(0).unwrap();
        let row = w.row_of(h).unwrap();
        w.set_pos(row, 1000, 1000);
        w.units.myspeed_mut()[row] = 50;
        assert!(w.issue(h, Order::move_to(5000, 1000, SUBTILE)));
        let mut steps = 0;
        while !w.orders(row).is_empty() && steps < 500 {
            w.step();
            steps += 1;
        }
        assert!(steps < 500, "the unit never arrived");
        let dx = (w.pos_x()[row] - 5000).abs();
        let dy = (w.pos_y()[row] - 1000).abs();
        assert!(
            dx <= SUBTILE && dy <= SUBTILE,
            "stopped {dx},{dy} from the target"
        );
        assert_eq!(
            w.coverage().orders.dispatches[OrderIndex::MoveTo.index()],
            steps as u64
        );
    }

    /// `PATROL` is faithfully empty and `FLEE_TO` shares `MOVE_TO`'s arm. Both are
    /// [measured] properties of the jump table and both are easy to "fix" by accident.
    #[test]
    fn patrol_does_nothing_and_flee_to_moves() {
        let mut w = World::with_capacity(4, 7);
        let h = w.spawn(0).unwrap();
        let row = w.row_of(h).unwrap();
        w.set_pos(row, 0, 0);
        w.units.myspeed_mut()[row] = 40;
        w.issue(
            h,
            Order {
                kind: OrderIndex::Patrol,
                x: 9000,
                y: 0,
                ..Order::default()
            },
        );
        w.step();
        assert_eq!(
            (w.pos_x()[row], w.pos_y()[row]),
            (0, 0),
            "PATROL must not move a unit"
        );
        assert_eq!(
            w.coverage().orders.dispatches[OrderIndex::Patrol.index()],
            1
        );

        w.issue(
            h,
            Order {
                kind: OrderIndex::FleeTo,
                x: 9000,
                y: 0,
                tolerance: SUBTILE,
                ..Order::default()
            },
        );
        w.step();
        assert!(
            w.pos_x()[row] > 0,
            "FLEE_TO shares MOVE_TO's executor and must move"
        );
    }

    /// Unimplemented arms are dispatched and counted, never silently dropped.
    #[test]
    fn unimplemented_order_arms_are_counted() {
        let mut w = World::with_capacity(4, 11);
        let h = w.spawn(0).unwrap();
        w.issue(
            h,
            Order {
                kind: OrderIndex::Gather,
                ..Order::default()
            },
        );
        for _ in 0..7 {
            w.step();
        }
        let c = w.coverage().orders;
        assert_eq!(c.dispatches[OrderIndex::Gather.index()], 7);
        assert_eq!(c.unimplemented, 7);
        assert!(c.covered_fraction() < 1.0);
    }

    /// The digest is defined by the walker, and it must actually cover the walked set.
    #[test]
    fn the_digest_covers_the_walked_field_set() {
        let (fields, walked_fields, bytes, walked_bytes) = World::digest_field_coverage();
        assert!(fields > 0);
        assert_eq!(
            fields, walked_fields,
            "every walked UnitData field is materialised"
        );
        assert_eq!(bytes, walked_bytes);
        // Unit::walk_data walks 111 bytes of UnitData [measured, state-schema.json].
        assert_eq!(walked_bytes, 111);
    }

    #[test]
    fn tombstone_hold_ticks_and_moves_digest_but_export_remains_gap_fail_closed() {
        use crate::systems::sparse_object_bands_authority_frontier::TombstoneFacts;

        let mut world = World::with_capacity(4, 0x4455);
        let handle = world.spawn_typed(2, 17).unwrap();
        let address = world
            .object_bands
            .address_of(WorldObjectIdentity::Unit {
                id: handle.id,
                generation: handle.generation,
            })
            .unwrap();
        let before = world.digest();
        world
            .object_bands
            .retire(
                address,
                WorldObjectIdentity::Unit {
                    id: handle.id,
                    generation: handle.generation,
                },
                TombstoneFacts {
                    flags: 0,
                    hold_frames: 3,
                    is_unit: true,
                    o_up: -1,
                },
            )
            .unwrap();
        let retired = world.digest();
        assert_ne!(retired, before);
        assert_eq!(world.unit_row_at(2, address.o), None);
        world.step();
        assert_eq!(world.coverage().hold_decrements, 1);
        assert!(matches!(
            world.object_bands.slot(address).unwrap().lifecycle,
            SparseSlotLifecycle::Tombstone(facts) if facts.hold_frames == 2
        ));
        assert_ne!(world.digest(), retired);
        assert!(!world.object_bands_are_dense_equivalent());
        assert!(matches!(
            world.export_save_state(),
            Err(WorldSaveError::RegistryMismatch)
        ));
    }

    /// Compaction must not be observable in the digest: only state is.
    #[test]
    fn digest_is_independent_of_row_order() {
        fn swap_dense_rows(world: &mut World, a: usize, b: usize) {
            for plane in 0..unit::W4_PLANES {
                world.units.w4_plane_mut(plane).swap(a, b);
            }
            for plane in 0..unit::W2_PLANES {
                world.units.w2_plane_mut(plane).swap(a, b);
            }
            for plane in 0..unit::W1_PLANES {
                world.units.w1_plane_mut(plane).swap(a, b);
            }
            for plane in 0..unit::WF_PLANES {
                world.units.wf_slice_mut(plane).swap(a, b);
            }
            world.unit_orders.swap(a, b);
            world.unit_type_id.swap(a, b);
            world.move_step_x.swap(a, b);
            world.move_step_y.swap(a, b);
            world.handle_of_row.swap(a, b);
            world.row_of_handle[world.handle_of_row[a] as usize] = a as u32;
            world.row_of_handle[world.handle_of_row[b] as usize] = b as u32;
            for row in [a, b] {
                let owner = world.units.get_who(row) as usize;
                let o = world.units.o()[row] as u32;
                world.objects.repoint(owner, Band::Unit, o, row as u32);
            }
        }

        let mut a = World::with_capacity(64, 5);
        for k in 0..64 {
            a.spawn((k % 4) as u8).unwrap();
        }
        let mut b = a.clone();
        for (left, right) in [(3, 17), (40, 41), (5, 63)] {
            swap_dense_rows(&mut b, left, right);
        }
        assert_ne!(
            a.handles(),
            b.handles(),
            "vacuous unless the row orders differ"
        );
        assert!(a.object_bands_are_dense_equivalent());
        assert!(b.object_bands_are_dense_equivalent());
        assert_eq!(a.digest(), b.digest());
    }

    /// The vector, scalar and hand-written kernel paths must agree bit for bit.
    #[test]
    fn every_kernel_path_steps_a_world_identically() {
        for &units in &[
            0usize, 1, 3, 4, 5, 7, 8, 9, 15, 16, 17, 31, 33, 64, 257, 1000,
        ] {
            let build = || {
                let mut w = World::with_capacity(units.max(1), 0x1234_5678 ^ units as u64);
                for k in 0..units {
                    let h = w.spawn((k % 5) as u8).unwrap();
                    let row = w.row_of(h).unwrap();
                    w.cooldown_mut()[row] = ((k as i32 % 7) - 2) as i16;
                    w.set_move_step(row, (k as i32 % 11) - 5, (k as i32 % 13) - 6);
                }
                w
            };
            let mut vecw = build();
            let mut scaw = build();
            let mut porw = build();
            for _ in 0..97 {
                vecw.step_hot();
                scaw.step_scalar_reference();
                porw.step_hand_simd();
            }
            assert_eq!(
                vecw.digest(),
                scaw.digest(),
                "digest diverged at {units} units"
            );
            assert_eq!(vecw.pos_x(), scaw.pos_x(), "pos_x diverged at {units}");
            assert_eq!(vecw.pos_y(), scaw.pos_y(), "pos_y diverged at {units}");
            assert_eq!(
                vecw.cooldown(),
                scaw.cooldown(),
                "cooldown diverged at {units}"
            );
            assert_eq!(
                vecw.digest(),
                porw.digest(),
                "hand simd diverged at {units}"
            );
        }
    }

    #[test]
    fn positions_stay_in_bounds() {
        let mut w = World::with_capacity(256, 99);
        for _ in 0..256 {
            w.spawn(0);
        }
        for row in 0..w.live_count() as usize {
            w.set_move_step(row, 977, -1231);
        }
        for _ in 0..2000 {
            w.step_hot();
        }
        for row in 0..w.live_count() as usize {
            assert!((0..MAP_SPAN).contains(&w.pos_x()[row]));
            assert!((0..MAP_SPAN).contains(&w.pos_y()[row]));
        }
    }

    /// The tick rate constants are the measured ones, and they are two different facts.
    #[test]
    fn the_clock_is_the_measured_one() {
        assert_eq!(
            TICK_HZ, 15,
            "15 sim frames is one game second (idiv 15 at 0x005924CF)"
        );
        assert_eq!(
            TICK_MS_NORMAL, 67,
            "Normal pacing is 67 ms (TurnControl::timings[2])"
        );
        assert_eq!(COORD_PER_TILE, 768);
    }

    #[test]
    fn runtime_allocation_uses_exact_coordinates_without_rng_or_setup_motion() {
        let mut world = World::with_capacity(4, 0x1357_2468);
        let rng_before = world.random.state();

        let handle = world.allocate_typed_at(2, 77, 12_345, 54_321).unwrap();
        let row = world.row_of(handle).unwrap();

        assert_eq!(world.random.state(), rng_before);
        assert_eq!(world.live_count(), 1);
        assert_eq!(world.units.x_internal()[row], 12_345);
        assert_eq!(world.units.y_internal()[row], 54_321);
        assert_eq!(world.units.angle()[row], 0);
        assert_eq!(world.units.myspeed()[row], 0);
        assert_eq!(
            world.units.get_flags(row) & OBJ_FLAG_ACTIVE,
            OBJ_FLAG_ACTIVE
        );
        assert_eq!(world.units.get_who(row), 2);
        assert_eq!(world.units.o()[row], 0);
        assert_eq!(world.units.o_up()[row], -1);
        assert_eq!(world.units.inside_up()[row], -1);
        assert_eq!(world.units.inside_up_who()[row], -1);
        assert_eq!(world.units.tolerance()[row], SUBTILE);
        assert!(world.orders(row).is_empty());
        assert_eq!(world.unit_type_id[row], 77);
        assert_eq!(world.move_step_x[row], 0);
        assert_eq!(world.move_step_y[row], 0);
        assert_eq!(world.objects.slot(2).band(Band::Unit), &[row as u32]);
    }

    #[test]
    fn failed_runtime_allocation_is_atomic_and_rng_free() {
        let mut world = World::with_capacity(1, 0x2468_1357);
        world.allocate_typed_at(0, 50, 100, 200).unwrap();
        let rng_before = world.random.state();
        let digest_before = world.digest();
        let objects_before = world.objects.total_objects();

        assert_eq!(world.allocate_typed_at(0, 51, 300, 400), None);
        assert_eq!(
            world.allocate_typed_at(OWNER_SLOTS as u8, 51, 300, 400),
            None
        );

        assert_eq!(world.random.state(), rng_before);
        assert_eq!(world.digest(), digest_before);
        assert_eq!(world.live_count(), 1);
        assert_eq!(world.objects.total_objects(), objects_before);
        assert_eq!(world.unit_orders.len(), 1);
        assert_eq!(world.unit_type_id.len(), 1);

        let mut invalid_owner = World::with_capacity(1, 0x1122_3344);
        let rng_before = invalid_owner.random.state();
        let digest_before = invalid_owner.digest();
        assert_eq!(
            invalid_owner.allocate_typed_at(OWNER_SLOTS as u8, 51, 300, 400),
            None
        );
        assert_eq!(invalid_owner.random.state(), rng_before);
        assert_eq!(invalid_owner.digest(), digest_before);
        assert_eq!(invalid_owner.live_count(), 0);
        assert_eq!(invalid_owner.objects.total_objects(), 0);
    }

    /// Damage runs the derived pipeline against the real balance table when both are
    /// loaded, and refuses rather than inventing a number when they are not.
    #[test]
    fn attacks_use_the_derived_pipeline_or_refuse() {
        let mut w = World::with_capacity(8, 3);
        let a = w.spawn_typed(0, 50).unwrap();
        let d = w.spawn_typed(1, 51).unwrap();
        let (arow, drow) = (w.row_of(a).unwrap(), w.row_of(d).unwrap());
        w.set_pos(arow, 1000, 1000);
        w.set_pos(drow, 1100, 1000);
        w.issue(a, Order::attack(1, 0));

        // No tables: counted as skipped, never guessed.
        w.step();
        assert_eq!(w.coverage().damage_skipped_no_tables, 1);
        assert_eq!(w.coverage().damage_applied, 0);

        let Ok(bal) = crate::balance::BalanceTable::load_default() else {
            eprintln!("skipping the applied half: captured balance table not present");
            return;
        };
        w.rules.balance = Some(std::sync::Arc::new(bal));
        w.rules.unit_stats = std::sync::Arc::new(vec![
            UnitTypeStats {
                type_id: 50,
                attack: 120,
                armor: 1,
                hits: 100,
                recharge: 10,
                max_range: 2000,
                min_range: 0,
            },
            UnitTypeStats {
                type_id: 51,
                attack: 90,
                armor: 3,
                hits: 100,
                recharge: 12,
                max_range: 2000,
                min_range: 0,
            },
        ]);
        // Positions are reset because the first step moved nothing (no tables) but the
        // order survived.
        w.set_pos(arow, 1000, 1000);
        w.set_pos(drow, 1100, 1000);
        let before = w.hits()[drow];
        w.step();
        assert_eq!(w.coverage().damage_applied, 1);
        assert!(
            w.hits()[drow] < before,
            "the defender must have taken damage"
        );
        // The attacker is now recharging, so the next frame applies nothing.
        w.step();
        assert_eq!(w.coverage().damage_applied, 1);
    }

    #[test]
    fn malformed_save_import_is_rejected_without_mutating_the_world() {
        let mut w = World::with_capacity(8, 0x1234);
        let a = w.spawn_typed(2, 17).unwrap();
        let b = w.spawn_typed(2, 19).unwrap();
        w.issue(a, Order::move_to(100, 200, 9));
        w.issue(b, Order::attack(2, 0));
        let before_digest = w.digest();
        let before_rng = w.random.state();
        let before_handles = w.all_handle_ids().to_vec();

        let mut corrupt = w.export_save_state().unwrap();
        corrupt.handle_of_row[1] = corrupt.handle_of_row[0];
        assert_eq!(
            w.import_save_state(corrupt),
            Err(WorldSaveError::HandlePermutation)
        );
        assert_eq!(w.digest(), before_digest);
        assert_eq!(w.random.state(), before_rng);
        assert_eq!(w.all_handle_ids(), before_handles);

        let mut corrupt = w.export_save_state().unwrap();
        corrupt.units.o_mut()[1] = 0;
        assert!(matches!(
            w.import_save_state(corrupt),
            Err(WorldSaveError::DuplicateObjectIndex { .. })
        ));
        assert_eq!(w.digest(), before_digest);
        assert_eq!(w.random.state(), before_rng);
        assert_eq!(w.all_handle_ids(), before_handles);
    }

    #[test]
    fn save_adapter_preserves_build_band_rows_and_rejects_non_permutations() {
        let mut w = World::with_capacity(4, 0x2233);
        let first = w.objects.insert(2, Band::Build, 1);
        w.mirror_dense_non_unit_append(2, Band::Build, 1, first)
            .unwrap();
        let second = w.objects.insert(2, Band::Build, 0);
        w.mirror_dense_non_unit_append(2, Band::Build, 0, second)
            .unwrap();
        let state = w.export_save_state().unwrap();
        assert_eq!(state.build_rows[2], vec![1, 0]);

        let mut loaded = World::with_capacity(1, 0);
        loaded.import_save_state(state.clone()).unwrap();
        assert_eq!(loaded.objects.slot(2).band(Band::Build), &[1, 0]);

        let before = loaded.objects.slot(2).band(Band::Build).to_vec();
        let mut corrupt = state;
        corrupt.build_rows[2][1] = 1;
        assert_eq!(
            loaded.import_save_state(corrupt),
            Err(WorldSaveError::RegistryMismatch)
        );
        assert_eq!(loaded.objects.slot(2).band(Band::Build), before);
    }
}
