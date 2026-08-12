//! Authoritative tick-side object store for the recovered movement collision transaction.
//!
//! The generated [`UnitCols`](crate::generated::state::UnitCols) already contains every
//! checksum-visible collision counter and identity link.  This module adapts those columns,
//! the engine-visible [`ObjectRegistry`](crate::objects::ObjectRegistry), current orders,
//! installed live Guy bodies, and leader diplomacy to [`collision::CollUnits`].  It also owns
//! initial WData linking/footprint stamps and the spatial [`ActorCommit`] transaction required
//! when the resolver snaps a unit to a new UCoord centre.
//!
//! No collision fact is defaulted.  A unit only participates after
//! [`LiveCollisionRuntime::install`] receives its resolved type/body facts.  Before movement,
//! [`LiveCollisionRuntime::preflight`] proves that every active unit has a source, the Guy count
//! matches `UnitData::guy_mark`, and every on-map anchor is present in the expected WData chain.
//! Missing facts therefore stop movement rather than becoming an invisible "empty map" answer.

use std::cell::Cell;

use crate::objects::Band;
use crate::order::OrderIndex;
use crate::systems::collision::{
    self, BoatQuery, CollGuy, CollUnits, UnitRow, DOMAIN_AIR, DOMAIN_LAND,
};
use crate::systems::leaders;
use crate::systems::map_terrain::World as TerrainWorld;
use crate::systems::movement::{self, PathStack};
use crate::systems::movement_driver::{ActorCommit, ActorRef};
use crate::systems::victory_score;
use crate::world::{Handle, World, OBJ_FLAG_ACTIVE};

/// One live, non-null squad Guy and the resolved type radius its collision stamp uses.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LiveCollisionGuy {
    pub x: i32,
    pub y: i32,
    pub angle: i32,
    pub block_radius: i32,
}

impl LiveCollisionGuy {
    fn coll(self) -> CollGuy {
        CollGuy {
            x: self.x,
            y: self.y,
            block_radius: self.block_radius,
        }
    }
}

/// Exact non-column facts read by `detect_unit_collision` / `resolve_unit_collision`.
///
/// The ordinary tick currently drives the one-live-Guy movement shape.  Multi-Guy sources are
/// still installed and block/corner-test exactly, but [`LiveCollisionRuntime::actor_ready`]
/// refuses to move them until the full formation-producing `Unit::set_new_location` body is
/// attached.  This is a fail-closed capability boundary, not a one-Guy approximation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveCollisionSource {
    pub domain: i32,
    pub block_radius: i32,
    pub big_radius: i32,
    pub push_size: i32,
    pub push_circles: i32,
    pub unit_flags: u32,
    pub unit_flags2: u32,
    pub attack_value: i32,
    pub spell_id: i32,
    pub unpacking: bool,
    pub captain: bool,
    pub moving: bool,
    pub searching: bool,
    /// `UnitData::action_type`, which is not necessarily the current order type.
    pub action: i32,
    /// Complete answer set for this unit's `UnitData::invalid_loc(tile_x, tile_y)` predicate.
    /// Absence from the vector means valid; callers install this source only when the vector is
    /// a complete representation for the live map/rules state.
    pub invalid_tiles: Vec<(i32, i32)>,
    /// Live, non-null squad guys in pointer-array order. Crew must not be included.
    pub guys: Vec<LiveCollisionGuy>,
}

impl LiveCollisionSource {
    /// Shipped ordinary-land gate which bypasses `detect_boat_collision`.
    pub fn ordinary_land(&self) -> bool {
        self.domain == DOMAIN_LAND
            && self.unit_flags & 0x0002_0000 == 0
            && self.unit_flags2 & (0x20 | 0x40) == 0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct InstalledSource {
    pub(crate) actor: Handle,
    pub(crate) state_revision: u64,
    pub(crate) facts: LiveCollisionSource,
    pub(crate) linked: bool,
}

/// Identity- and revision-bound image of the current action fields consumed by movement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MovementSourceState {
    pub actor: Handle,
    pub row: usize,
    pub revision: u64,
    pub moving: bool,
    pub action: i32,
}

/// Complete result of one atomic installed-source state transition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MovementSourceStateReceipt {
    pub before: MovementSourceState,
    pub after: MovementSourceState,
}

/// Canonical before/after image of the spatial part of one contained `Unit::come_out`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ComeOutRelocationState {
    pub source: MovementSourceState,
    pub point: (i32, i32),
    pub angle: i32,
    pub inside: (i8, i16),
    pub guy: LiveCollisionGuy,
    pub linked: bool,
}

/// Complete result of publishing the collision/World/Guy half of a contained release.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ComeOutRelocationReceipt {
    pub before: ComeOutRelocationState,
    pub after: ComeOutRelocationState,
}

/// Per-current-order collision fields not represented by the compact generic `Order` union.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CollisionOrderState {
    pub step_dest: Option<(i32, i32)>,
    pub detour: Option<(i32, i32)>,
    pub wait: i32,
    pub retry: i32,
}

/// A missing or inconsistent live fact.  All variants are deterministic and fail movement
/// closed; none is converted into "no collision".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveCollisionFault {
    RowOutOfRange(usize),
    StaleActor(Handle),
    MissingSource(usize),
    MissingPath(usize),
    MissingOrderState(usize),
    SourceAlreadyInstalled(usize),
    NonEmptyRehydrationRuntime,
    IncompleteRehydration {
        active: usize,
        supplied: usize,
    },
    DuplicateRehydratedRow(usize),
    SavedGuyActorMismatch {
        row: usize,
        guy: usize,
    },
    ForeignSource {
        row: usize,
        requested: Handle,
        installed: Handle,
    },
    StaleSourceRevision {
        row: usize,
        expected: u64,
        observed: u64,
    },
    InactiveActor(usize),
    OffMapActor(usize),
    InvalidDomain(i32),
    InvalidBlockRadius(i32),
    InvalidGuyCount {
        column: i32,
        supplied: usize,
    },
    InvalidGuyLocation(usize),
    InvalidActorLocation,
    UnlinkedAnchor(usize),
    BrokenWorldChain {
        who: i32,
        o: i32,
    },
    UnsupportedBoatSolver(usize),
    UnsupportedMovingFormation {
        row: usize,
        guys: usize,
    },
    UnsupportedAttackSlack(usize),
    MissingRepathHost(usize),
    StaleActorCommit(usize),
    SpatialWriteOutsideCommit(usize),
    InvalidLeader(i32),
}

/// Persistent movement-collision state owned by one tick simulation.
#[derive(Debug, Default)]
pub struct LiveCollisionRuntime {
    pub(crate) sources: Vec<Option<InstalledSource>>,
    pub order_state: Vec<CollisionOrderState>,
    pub check: collision::CollCheck,
    pub repath_budget: [i32; 10],
    budget_frame: i32,
    pub(crate) path_top_flags: Vec<u8>,
}

impl LiveCollisionRuntime {
    pub fn new() -> Self {
        Self {
            budget_frame: i32::MIN,
            ..Self::default()
        }
    }

    /// Keep pointer-free sidecars row-aligned with the generated SoA.
    pub fn ensure_rows(&mut self, rows: usize) {
        self.sources.resize_with(rows, || None);
        self.order_state
            .resize(rows, CollisionOrderState::default());
        self.path_top_flags.resize(rows, 0);
    }

    pub fn snapshot_paths(&mut self, paths: &[PathStack], rows: usize) {
        self.ensure_rows(rows);
        for row in 0..rows {
            self.path_top_flags[row] = paths
                .get(row)
                .and_then(PathStack::peek)
                .map_or(0, |p| p.flags as u8);
        }
    }

    pub fn begin_frame(&mut self, frame: i32) {
        if self.budget_frame != frame {
            self.repath_budget = [0; 10];
            self.budget_frame = frame;
        }
    }

    pub fn source(&self, row: usize) -> Option<&LiveCollisionSource> {
        self.sources
            .get(row)
            .and_then(Option::as_ref)
            .map(|s| &s.facts)
    }

    /// Read the exact installed action-state image for one live actor.
    pub fn source_state(
        &self,
        world: &World,
        actor: Handle,
    ) -> Result<MovementSourceState, LiveCollisionFault> {
        let row = world
            .row_of(actor)
            .ok_or(LiveCollisionFault::StaleActor(actor))?;
        if world.units.get_flags(row) & OBJ_FLAG_ACTIVE == 0 {
            return Err(LiveCollisionFault::InactiveActor(row));
        }
        let installed = self
            .sources
            .get(row)
            .and_then(Option::as_ref)
            .ok_or(LiveCollisionFault::MissingSource(row))?;
        if installed.actor != actor {
            return Err(LiveCollisionFault::ForeignSource {
                row,
                requested: actor,
                installed: installed.actor,
            });
        }
        Ok(installed.state(row))
    }

    /// Compare-and-set the action fields consumed by the authoritative movement adapter.
    ///
    /// Handle resolution, activity, installed identity and revision are all checked before
    /// either field changes. The revision advances even when the requested values equal the
    /// current image, so replaying a successfully consumed request is observably stale.
    pub fn compare_exchange_source_state(
        &mut self,
        world: &World,
        actor: Handle,
        expected_revision: u64,
        moving: bool,
        action: OrderIndex,
    ) -> Result<MovementSourceStateReceipt, LiveCollisionFault> {
        let before = self.source_state(world, actor)?;
        if before.revision != expected_revision {
            return Err(LiveCollisionFault::StaleSourceRevision {
                row: before.row,
                expected: expected_revision,
                observed: before.revision,
            });
        }

        let installed = self.sources[before.row]
            .as_mut()
            .expect("source_state proved installed source");
        installed.facts.moving = moving;
        installed.facts.action = action as i32;
        installed.state_revision = installed.state_revision.wrapping_add(1);
        let after = installed.state(before.row);
        Ok(MovementSourceStateReceipt { before, after })
    }

    /// Atomically attach an authoritative collision source, world anchor and Guy stamps.
    pub fn install(
        &mut self,
        world: &mut World,
        terrain: &mut TerrainWorld,
        handle: Handle,
        source: LiveCollisionSource,
    ) -> Result<usize, LiveCollisionFault> {
        let row = world
            .row_of(handle)
            .ok_or(LiveCollisionFault::RowOutOfRange(handle.id as usize))?;
        self.ensure_rows(world.live_count() as usize);
        if self.sources[row].is_some() {
            return Err(LiveCollisionFault::SourceAlreadyInstalled(row));
        }
        validate_source(world, terrain, row, &source)?;

        // Every validation is complete before the first mutation.  Link at the WData head in
        // the same stack order as Object::add_to_world, then install each live Guy footprint.
        link_anchor(world, terrain, row)?;
        // Normal Unit construction initializes the "no blocker" sentinels.  The compact
        // World allocator zeroes every generated plane, so the collision installation is the
        // first authoritative owner of these three fields.
        world.units.collide_o_mut()[row] = -1;
        world.units.collide_who_mut()[row] = -1;
        world.units.collide_guy_mut()[row] = 0;
        for (guy_num, guy) in source.guys.iter().enumerate() {
            collision::guy_set_new_location(
                terrain,
                (-1, -1),
                (guy.x, guy.y),
                source.domain,
                guy_num as i32,
                source.guys.len() as i32,
                guy.block_radius,
            );
        }
        self.sources[row] = Some(InstalledSource {
            actor: handle,
            state_revision: 0,
            facts: source,
            linked: true,
        });
        Ok(row)
    }

    /// Rebuild the pointer-free collision sidecar around a canonical loaded World/terrain
    /// image without relinking anchors or restamping Guy footprints.
    ///
    /// DoNSave owns the intrusive WData lists and collision bitmap but deliberately carries no
    /// external type/Guy source. A product adapter may reconstruct those immutable facts after
    /// load. This transaction accepts exactly one generation-bound source for every active Unit,
    /// validates every source, saved anchor, and one-Guy actor image before the first sidecar
    /// write, then replaces the empty runtime in one assignment. It cannot hide a malformed save
    /// by repairing the checksum-owned spatial channels.
    pub fn rehydrate_saved_sources(
        &mut self,
        world: &World,
        terrain: &TerrainWorld,
        sources: Vec<(Handle, LiveCollisionSource)>,
    ) -> Result<usize, LiveCollisionFault> {
        if self.sources.iter().any(Option::is_some) {
            return Err(LiveCollisionFault::NonEmptyRehydrationRuntime);
        }
        let rows = world.live_count() as usize;
        let active = (0..rows)
            .filter(|&row| world.units.get_flags(row) & OBJ_FLAG_ACTIVE != 0)
            .count();
        if sources.len() != active {
            return Err(LiveCollisionFault::IncompleteRehydration {
                active,
                supplied: sources.len(),
            });
        }

        let mut planned = vec![None; rows];
        for (actor, source) in sources {
            let row = world
                .row_of(actor)
                .ok_or(LiveCollisionFault::StaleActor(actor))?;
            if planned[row].is_some() {
                return Err(LiveCollisionFault::DuplicateRehydratedRow(row));
            }
            validate_source(world, terrain, row, &source)?;
            if !anchor_is_linked(world, terrain, row) {
                return Err(LiveCollisionFault::UnlinkedAnchor(row));
            }
            if let [guy] = source.guys.as_slice() {
                if (guy.x, guy.y, guy.angle)
                    != (
                        world.units.x_internal()[row],
                        world.units.y_internal()[row],
                        world.units.angle()[row],
                    )
                {
                    return Err(LiveCollisionFault::SavedGuyActorMismatch { row, guy: 0 });
                }
            }
            planned[row] = Some(InstalledSource {
                actor,
                state_revision: 0,
                facts: source,
                linked: true,
            });
        }
        for row in 0..rows {
            if world.units.get_flags(row) & OBJ_FLAG_ACTIVE != 0 && planned[row].is_none() {
                return Err(LiveCollisionFault::MissingSource(row));
            }
        }

        let mut replacement = Self::new();
        replacement.ensure_rows(rows);
        replacement.sources = planned;
        *self = replacement;
        Ok(active)
    }

    /// Attach an exact collision/Guy source for a Unit which is currently inside an object.
    ///
    /// Contained Units own neither a WData anchor nor collision footprints, so this variant
    /// records the same source facts as [`Self::install`] without publishing either spatial
    /// structure.  [`Self::release_contained_for_come_out`] is the only transition which turns
    /// such an installation into a linked, stamped on-map source.
    pub fn install_contained(
        &mut self,
        world: &World,
        terrain: &TerrainWorld,
        handle: Handle,
        source: LiveCollisionSource,
    ) -> Result<usize, LiveCollisionFault> {
        let row = world
            .row_of(handle)
            .ok_or(LiveCollisionFault::RowOutOfRange(handle.id as usize))?;
        self.ensure_rows(world.live_count() as usize);
        if self.sources[row].is_some() {
            return Err(LiveCollisionFault::SourceAlreadyInstalled(row));
        }
        if world.units.get_flags(row) & OBJ_FLAG_ACTIVE == 0 {
            return Err(LiveCollisionFault::InactiveActor(row));
        }
        if world.units.inside_up()[row] < 0 {
            return Err(LiveCollisionFault::OffMapActor(row));
        }
        if !(0..=DOMAIN_AIR).contains(&source.domain) {
            return Err(LiveCollisionFault::InvalidDomain(source.domain));
        }
        if !(0..=10).contains(&source.block_radius) {
            return Err(LiveCollisionFault::InvalidBlockRadius(source.block_radius));
        }
        let mark = world.units.guy_mark()[row] as i32;
        if mark < 0 || mark as usize != source.guys.len() {
            return Err(LiveCollisionFault::InvalidGuyCount {
                column: mark,
                supplied: source.guys.len(),
            });
        }
        for (index, guy) in source.guys.iter().enumerate() {
            if !(0..=10).contains(&guy.block_radius) || !terrain.valid_coord(guy.x, guy.y) {
                return Err(LiveCollisionFault::InvalidGuyLocation(index));
            }
        }
        self.sources[row] = Some(InstalledSource {
            actor: handle,
            state_revision: 0,
            facts: source,
            linked: false,
        });
        Ok(row)
    }

    /// Atomically publish the canonical World anchor and one-Guy collision footprint for a
    /// contained release.
    ///
    /// Every fallible identity, revision, containment, formation, and destination check runs
    /// before the first write.  The successful commit clears `inside_up`, links the actor at
    /// the WData head, stamps its Guy, installs the new point/facing, clears movement/action,
    /// and advances the source revision as one receipt-bearing transition.
    #[allow(clippy::too_many_arguments)]
    pub fn release_contained_for_come_out(
        &mut self,
        world: &mut World,
        terrain: &mut TerrainWorld,
        actor: Handle,
        expected_revision: u64,
        expected_container: (i8, i16),
        point: (i32, i32),
        angle: i32,
    ) -> Result<ComeOutRelocationReceipt, LiveCollisionFault> {
        let row = world
            .row_of(actor)
            .ok_or(LiveCollisionFault::StaleActor(actor))?;
        if world.units.get_flags(row) & OBJ_FLAG_ACTIVE == 0 {
            return Err(LiveCollisionFault::InactiveActor(row));
        }
        let installed = self
            .sources
            .get(row)
            .and_then(Option::as_ref)
            .ok_or(LiveCollisionFault::MissingSource(row))?;
        if installed.actor != actor {
            return Err(LiveCollisionFault::ForeignSource {
                row,
                requested: actor,
                installed: installed.actor,
            });
        }
        if installed.state_revision != expected_revision {
            return Err(LiveCollisionFault::StaleSourceRevision {
                row,
                expected: expected_revision,
                observed: installed.state_revision,
            });
        }
        let observed_container = (
            world.units.inside_up_who()[row],
            world.units.inside_up()[row],
        );
        if installed.linked
            || observed_container != expected_container
            || anchor_is_linked(world, terrain, row)
        {
            return Err(LiveCollisionFault::UnlinkedAnchor(row));
        }
        if installed.facts.guys.len() != 1 {
            return Err(LiveCollisionFault::UnsupportedMovingFormation {
                row,
                guys: installed.facts.guys.len(),
            });
        }
        if !terrain.valid_coord(point.0, point.1)
            || installed
                .facts
                .invalid_tiles
                .contains(&(movement::tile_of(point.0), movement::tile_of(point.1)))
        {
            return Err(LiveCollisionFault::InvalidActorLocation);
        }

        let before = ComeOutRelocationState {
            source: installed.state(row),
            point: (world.units.x_internal()[row], world.units.y_internal()[row]),
            angle: world.units.angle()[row],
            inside: observed_container,
            guy: installed.facts.guys[0],
            linked: false,
        };

        world.units.x_internal_mut()[row] = point.0;
        world.units.y_internal_mut()[row] = point.1;
        world.units.angle_mut()[row] = angle;
        world.units.inside_up_mut()[row] = -1;
        world.units.inside_up_who_mut()[row] = -1;
        link_anchor(world, terrain, row)?;

        let installed = self.sources[row]
            .as_mut()
            .expect("contained source was preflighted");
        let guy = &mut installed.facts.guys[0];
        collision::guy_set_new_location(
            terrain,
            (-1, -1),
            point,
            installed.facts.domain,
            0,
            1,
            guy.block_radius,
        );
        guy.x = point.0;
        guy.y = point.1;
        guy.angle = angle;
        installed.facts.moving = false;
        installed.facts.action = OrderIndex::None as i32;
        installed.linked = true;
        installed.state_revision = installed.state_revision.wrapping_add(1);
        let guy_after = *guy;
        let source_after = installed.state(row);

        let after = ComeOutRelocationState {
            source: source_after,
            point,
            angle,
            inside: (-1, -1),
            guy: guy_after,
            linked: true,
        };
        Ok(ComeOutRelocationReceipt { before, after })
    }

    /// Prove the object store is complete before a collision callback may run.
    pub fn preflight(
        &self,
        world: &World,
        terrain: &TerrainWorld,
        paths: &[PathStack],
    ) -> Result<(), LiveCollisionFault> {
        let rows = world.live_count() as usize;
        if self.sources.len() < rows || self.order_state.len() < rows {
            return Err(LiveCollisionFault::MissingSource(self.sources.len()));
        }
        for row in 0..rows {
            if world.units.get_flags(row) & OBJ_FLAG_ACTIVE == 0 {
                continue;
            }
            let installed = self.sources[row]
                .as_ref()
                .ok_or(LiveCollisionFault::MissingSource(row))?;
            let actor = world
                .handle_at_row(row)
                .ok_or(LiveCollisionFault::RowOutOfRange(row))?;
            if installed.actor != actor {
                return Err(LiveCollisionFault::ForeignSource {
                    row,
                    requested: actor,
                    installed: installed.actor,
                });
            }
            if paths.get(row).is_none() {
                return Err(LiveCollisionFault::MissingPath(row));
            }
            let mark = world.units.guy_mark()[row] as i32;
            if mark < 0 || mark as usize != installed.facts.guys.len() {
                return Err(LiveCollisionFault::InvalidGuyCount {
                    column: mark,
                    supplied: installed.facts.guys.len(),
                });
            }
            if world.units.inside_up()[row] < 0 {
                if !installed.linked || !anchor_is_linked(world, terrain, row) {
                    return Err(LiveCollisionFault::UnlinkedAnchor(row));
                }
            }
        }
        Ok(())
    }

    /// Whether this installed row can execute the currently recovered spatial commit exactly.
    pub fn actor_ready(&self, world: &World, row: usize) -> Result<(), LiveCollisionFault> {
        if row >= world.live_count() as usize {
            return Err(LiveCollisionFault::RowOutOfRange(row));
        }
        if world.units.get_flags(row) & OBJ_FLAG_ACTIVE == 0 {
            return Err(LiveCollisionFault::InactiveActor(row));
        }
        if world.units.inside_up()[row] >= 0 {
            return Err(LiveCollisionFault::OffMapActor(row));
        }
        let source = self
            .source(row)
            .ok_or(LiveCollisionFault::MissingSource(row))?;
        if source.domain != DOMAIN_AIR && !source.ordinary_land() {
            return Err(LiveCollisionFault::UnsupportedBoatSolver(row));
        }
        if source.guys.len() != 1 {
            return Err(LiveCollisionFault::UnsupportedMovingFormation {
                row,
                guys: source.guys.len(),
            });
        }
        Ok(())
    }
}

impl InstalledSource {
    fn state(&self, row: usize) -> MovementSourceState {
        MovementSourceState {
            actor: self.actor,
            row,
            revision: self.state_revision,
            moving: self.facts.moving,
            action: self.facts.action,
        }
    }
}

fn validate_source(
    world: &World,
    terrain: &TerrainWorld,
    row: usize,
    source: &LiveCollisionSource,
) -> Result<(), LiveCollisionFault> {
    if world.units.get_flags(row) & OBJ_FLAG_ACTIVE == 0 {
        return Err(LiveCollisionFault::InactiveActor(row));
    }
    if world.units.inside_up()[row] >= 0 {
        return Err(LiveCollisionFault::OffMapActor(row));
    }
    if !(0..=DOMAIN_AIR).contains(&source.domain) {
        return Err(LiveCollisionFault::InvalidDomain(source.domain));
    }
    if !(0..=10).contains(&source.block_radius) {
        return Err(LiveCollisionFault::InvalidBlockRadius(source.block_radius));
    }
    let mark = world.units.guy_mark()[row] as i32;
    if mark < 0 || mark as usize != source.guys.len() {
        return Err(LiveCollisionFault::InvalidGuyCount {
            column: mark,
            supplied: source.guys.len(),
        });
    }
    let x = world.units.x_internal()[row];
    let y = world.units.y_internal()[row];
    if !terrain.valid_coord(x, y) {
        return Err(LiveCollisionFault::InvalidActorLocation);
    }
    for (i, guy) in source.guys.iter().enumerate() {
        if !(0..=10).contains(&guy.block_radius) || !terrain.valid_coord(guy.x, guy.y) {
            return Err(LiveCollisionFault::InvalidGuyLocation(i));
        }
    }
    Ok(())
}

fn object_row(world: &World, who: i32, o: i32) -> Option<usize> {
    if who < 0 || o < 0 || who as usize >= crate::objects::OWNER_SLOTS {
        return None;
    }
    world
        .objects
        .slot(who as usize)
        .band(Band::Unit)
        .get(o as usize)
        .copied()
        .map(|r| r as usize)
        .filter(|&r| r < world.live_count() as usize)
}

fn link_anchor(
    world: &mut World,
    terrain: &mut TerrainWorld,
    row: usize,
) -> Result<(), LiveCollisionFault> {
    let x = world.units.x_internal()[row];
    let y = world.units.y_internal()[row];
    let wx = movement::wcell_of(x);
    let wy = movement::wcell_of(y);
    if !terrain.valid_w(wx, wy) {
        return Err(LiveCollisionFault::InvalidActorLocation);
    }
    let head = terrain.wdata(wx, wy);
    world.units.down_mut()[row] = head.down;
    world.units.down_who_mut()[row] = head.down_who;
    terrain.set_down(
        wx,
        wy,
        world.units.o()[row],
        world.units.get_who(row) as i16,
    );
    Ok(())
}

fn anchor_is_linked(world: &World, terrain: &TerrainWorld, row: usize) -> bool {
    let who = world.units.get_who(row) as i32;
    let o = world.units.o()[row] as i32;
    let wx = movement::wcell_of(world.units.x_internal()[row]);
    let wy = movement::wcell_of(world.units.y_internal()[row]);
    if !terrain.valid_w(wx, wy) {
        return false;
    }
    let head = terrain.wdata(wx, wy);
    let mut cur = (head.down_who as i32, head.down as i32);
    for _ in 0..=world.live_count() {
        if cur == (who, o) {
            return true;
        }
        let Some(next_row) = object_row(world, cur.0, cur.1) else {
            return false;
        };
        cur = (
            world.units.down_who()[next_row] as i32,
            world.units.down()[next_row] as i32,
        );
    }
    false
}

/// The concrete `CollUnits` view over generated columns and installed sources.
pub struct LiveCollisionStore<'a> {
    pub(crate) world: &'a mut World,
    pub(crate) sources: &'a mut [Option<InstalledSource>],
    pub(crate) order_state: &'a mut [CollisionOrderState],
    pub(crate) path_top_flags: &'a [u8],
    pub(crate) leaders: &'a leaders::Leaders,
    pub(crate) victory: &'a victory_score::Leaders,
    pub(crate) repath_budget: &'a mut [i32; 10],
    fault: Cell<Option<LiveCollisionFault>>,
}

impl<'a> LiveCollisionStore<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        world: &'a mut World,
        sources: &'a mut [Option<InstalledSource>],
        order_state: &'a mut [CollisionOrderState],
        path_top_flags: &'a [u8],
        leaders: &'a leaders::Leaders,
        victory: &'a victory_score::Leaders,
        repath_budget: &'a mut [i32; 10],
    ) -> Self {
        Self {
            world,
            sources,
            order_state,
            path_top_flags,
            leaders,
            victory,
            repath_budget,
            fault: Cell::new(None),
        }
    }

    fn fail(&self, fault: LiveCollisionFault) {
        if self.fault.get().is_none() {
            self.fault.set(Some(fault));
        }
    }

    pub fn take_fault(&self) -> Option<LiveCollisionFault> {
        self.fault.take()
    }

    pub fn row_index(&self, who: i32, o: i32) -> Option<usize> {
        object_row(self.world, who, o)
    }

    fn source_at(&self, row: usize) -> Option<&LiveCollisionSource> {
        self.sources
            .get(row)
            .and_then(Option::as_ref)
            .map(|s| &s.facts)
    }

    fn write_scalars(&mut self, row: usize, value: &UnitRow) {
        self.world.units.collide_mut()[row] = value.collide;
        self.world.units.collide_o_mut()[row] = value.collide_o;
        self.world.units.collide_guy_mut()[row] = value.collide_guy;
        self.world.units.collide_frame_mut()[row] = value.collide_frame;
        self.world.units.safe_mut()[row] = value.safe;
        self.world.units.collide_who_mut()[row] = value.collide_who;
        self.world.units.set_unit_masks(row, value.unit_masks);
    }
}

impl CollUnits for LiveCollisionStore<'_> {
    fn row(&self, who: i32, o: i32) -> Option<UnitRow> {
        let row = self.row_index(who, o)?;
        let Some(source) = self.source_at(row) else {
            self.fail(LiveCollisionFault::MissingSource(row));
            return None;
        };
        let current = self.world.orders(row).current();
        Some(UnitRow {
            who,
            o,
            x: self.world.units.x_internal()[row],
            y: self.world.units.y_internal()[row],
            down: self.world.units.down()[row],
            down_who: self.world.units.down_who()[row],
            domain: source.domain,
            block_radius: source.block_radius,
            big_radius: source.big_radius,
            push_size: source.push_size,
            push_circles: source.push_circles,
            angle: self.world.units.angle()[row],
            first_guy_angle: source
                .guys
                .first()
                .map_or(self.world.units.angle()[row], |g| g.angle),
            group: self.world.units.group()[row],
            collide: self.world.units.collide()[row],
            collide_o: self.world.units.collide_o()[row],
            collide_who: self.world.units.collide_who()[row],
            collide_guy: self.world.units.collide_guy()[row],
            collide_frame: self.world.units.collide_frame()[row],
            safe: self.world.units.safe()[row],
            unit_masks: self.world.units.get_unit_masks(row),
            on_map: self.world.units.inside_up()[row] < 0,
            active: self.world.units.get_flags(row) & OBJ_FLAG_ACTIVE != 0,
            moving: source.moving,
            action: source.action,
            order: current.map_or(OrderIndex::None as i32, |o| o.kind as i32),
            has_orders: current.is_some(),
            searching: source.searching,
            path_top_flags: self.path_top_flags.get(row).copied().unwrap_or_else(|| {
                self.fail(LiveCollisionFault::MissingPath(row));
                8 // collision-suppressing, fail-closed while the outer adapter rejects fault
            }),
            unit_flags: source.unit_flags,
            unit_flags2: source.unit_flags2,
            attack_value: source.attack_value,
            spell_id: source.spell_id,
            unpacking: source.unpacking,
            captain: source.captain,
        })
    }

    fn unit_corner(&self, who: i32, o: i32, cx: i32, cy: i32) -> i32 {
        let Some(row) = self.row_index(who, o) else {
            return 0;
        };
        let Some(source) = self.source_at(row) else {
            self.fail(LiveCollisionFault::MissingSource(row));
            return 0;
        };
        collision::unit_corner(
            cx,
            cy,
            source.guys.iter().copied().map(LiveCollisionGuy::coll),
        )
    }

    fn find_boat_units(&mut self, _query: BoatQuery) -> Vec<(i32, i32)> {
        self.fail(LiveCollisionFault::UnsupportedBoatSolver(usize::MAX));
        Vec::new()
    }

    fn effective_owner(&self, who: i32) -> i32 {
        let Some(leader) = self.leaders.leaders.get(who as usize) else {
            self.fail(LiveCollisionFault::InvalidLeader(who));
            return who;
        };
        leader.slot
    }

    fn diplomacy(&self, who: i32, other: i32) -> i32 {
        let Some(leader) = self.leaders.leaders.get(who as usize) else {
            self.fail(LiveCollisionFault::InvalidLeader(who));
            return 0;
        };
        leader.diplo_toward(other)
    }

    fn boat_invalid_loc(&self, who: i32, o: i32, _tx: i32, _ty: i32) -> bool {
        let row = self.row_index(who, o).unwrap_or(usize::MAX);
        self.fail(LiveCollisionFault::UnsupportedBoatSolver(row));
        true
    }

    fn set_boat_location(&mut self, who: i32, o: i32, _x: i32, _y: i32) {
        let row = self.row_index(who, o).unwrap_or(usize::MAX);
        self.fail(LiveCollisionFault::UnsupportedBoatSolver(row));
    }

    fn face_pushed_idle_unit(
        &mut self,
        who: i32,
        o: i32,
        _angle: i32,
        _pusher_who: i32,
        _pusher_o: i32,
    ) {
        let row = self.row_index(who, o).unwrap_or(usize::MAX);
        self.fail(LiveCollisionFault::UnsupportedBoatSolver(row));
    }

    fn write(&mut self, who: i32, o: i32, value: &UnitRow) {
        let Some(row) = self.row_index(who, o) else {
            return;
        };
        if (
            self.world.units.x_internal()[row],
            self.world.units.y_internal()[row],
        ) != (value.x, value.y)
        {
            self.fail(LiveCollisionFault::SpatialWriteOutsideCommit(row));
            return;
        }
        self.write_scalars(row, value);
    }

    fn is_enemy(&self, me: i32, them: i32) -> bool {
        if me < 0
            || them < 0
            || me as usize >= victory_score::NUM_LEADERS
            || them as usize >= victory_score::NUM_LEADERS
        {
            self.fail(LiveCollisionFault::InvalidLeader(me.min(them)));
            return true;
        }
        self.victory.is_enemy(me as usize, them as usize)
    }

    fn order_dest(&self, who: i32, o: i32) -> Option<(i32, i32)> {
        let row = self.row_index(who, o)?;
        self.order_state.get(row).and_then(|s| s.step_dest)
    }

    fn set_order_dest(&mut self, who: i32, o: i32, x: i32, y: i32) {
        let Some(row) = self.row_index(who, o) else {
            return;
        };
        let Some(state) = self.order_state.get_mut(row) else {
            self.fail(LiveCollisionFault::MissingOrderState(row));
            return;
        };
        state.step_dest = Some((x, y));
    }

    fn set_order_detour(&mut self, who: i32, o: i32, x: i32, y: i32) {
        let Some(row) = self.row_index(who, o) else {
            return;
        };
        let Some(state) = self.order_state.get_mut(row) else {
            self.fail(LiveCollisionFault::MissingOrderState(row));
            return;
        };
        state.detour = Some((x, y));
    }

    fn set_order_wait(&mut self, who: i32, o: i32, ticks: i32) {
        let Some(row) = self.row_index(who, o) else {
            return;
        };
        let Some(state) = self.order_state.get_mut(row) else {
            self.fail(LiveCollisionFault::MissingOrderState(row));
            return;
        };
        state.wait = ticks;
    }

    fn clear_order_retry(&mut self, who: i32, o: i32) {
        let Some(row) = self.row_index(who, o) else {
            return;
        };
        let Some(state) = self.order_state.get_mut(row) else {
            self.fail(LiveCollisionFault::MissingOrderState(row));
            return;
        };
        state.retry = 0;
    }

    fn order_targets(&self, who: i32, o: i32, target_who: i32, target_o: i32) -> bool {
        let Some(row) = self.row_index(who, o) else {
            return false;
        };
        self.world.orders(row).current().is_some_and(|ord| {
            ord.target_who as i32 == target_who && ord.target_o as i32 == target_o
        })
    }

    fn attack_slack(&self, who: i32, o: i32, _nx: i32, _ny: i32) -> i32 {
        let row = self.row_index(who, o).unwrap_or(usize::MAX);
        self.fail(LiveCollisionFault::UnsupportedAttackSlack(row));
        0
    }

    fn repath_budget(&self, who: i32) -> i32 {
        self.repath_budget
            .get(who as usize)
            .copied()
            .unwrap_or_else(|| {
                self.fail(LiveCollisionFault::InvalidLeader(who));
                16 // retail's hard throttle: no repath
            })
    }

    fn bump_repath_budget(&mut self, who: i32) {
        let Some(value) = self.repath_budget.get_mut(who as usize) else {
            self.fail(LiveCollisionFault::InvalidLeader(who));
            return;
        };
        *value += 1;
    }

    fn frame(&self) -> i32 {
        self.world.frame
    }
}

/// Commit detector/resolver row mutations into generated columns and, when needed, perform the
/// exact one-live-Guy spatial relocation owned by this tranche.
pub fn commit_actor(
    terrain: &mut TerrainWorld,
    store: &mut LiveCollisionStore<'_>,
    commit: ActorCommit,
) {
    let Some(row) = store.row_index(commit.actor.who, commit.actor.o) else {
        store.fail(LiveCollisionFault::RowOutOfRange(commit.actor.o as usize));
        return;
    };
    if store.row(commit.actor.who, commit.actor.o) != Some(commit.before) {
        store.fail(LiveCollisionFault::StaleActorCommit(row));
        return;
    }

    if (commit.before.x, commit.before.y) != (commit.after.x, commit.after.y) {
        let guys = store
            .sources
            .get(row)
            .and_then(Option::as_ref)
            .map_or(0, |s| s.facts.guys.len());
        if guys != 1 {
            store.fail(LiveCollisionFault::UnsupportedMovingFormation { row, guys });
            return;
        }
        if !terrain.valid_coord(commit.after.x, commit.after.y) {
            store.fail(LiveCollisionFault::InvalidActorLocation);
            return;
        }
        if relocate_anchor(store.world, terrain, row, commit.after.x, commit.after.y).is_err() {
            store.fail(LiveCollisionFault::BrokenWorldChain {
                who: commit.actor.who,
                o: commit.actor.o,
            });
            return;
        }
        let installed = store.sources[row].as_mut().unwrap();
        let guy = &mut installed.facts.guys[0];
        collision::guy_set_new_location(
            terrain,
            (guy.x, guy.y),
            (commit.after.x, commit.after.y),
            installed.facts.domain,
            0,
            1,
            guy.block_radius,
        );
        guy.x = commit.after.x;
        guy.y = commit.after.y;
    }
    store.write_scalars(row, &commit.after);
}

/// Commit the integrator's ordinary translation/facing result after the collision callback
/// returned clear.  Collision child state was already committed by [`commit_actor`]; this is
/// the enclosing `Unit::move_step`/`Unit::set_new_location` write which makes a successful step
/// visible to the next unit in object traversal order.
pub fn commit_body(
    terrain: &mut TerrainWorld,
    store: &mut LiveCollisionStore<'_>,
    actor: ActorRef,
    body: &movement::Body,
) {
    let Some(row) = store.row_index(actor.who, actor.o) else {
        store.fail(LiveCollisionFault::RowOutOfRange(actor.o as usize));
        return;
    };
    let current = (
        store.world.units.x_internal()[row],
        store.world.units.y_internal()[row],
    );
    if current != (body.x, body.y) {
        let guys = store.sources[row]
            .as_ref()
            .map_or(0, |s| s.facts.guys.len());
        if guys != 1 {
            store.fail(LiveCollisionFault::UnsupportedMovingFormation { row, guys });
            return;
        }
        if relocate_anchor(store.world, terrain, row, body.x, body.y).is_err() {
            store.fail(LiveCollisionFault::BrokenWorldChain {
                who: actor.who,
                o: actor.o,
            });
            return;
        }
        let installed = store.sources[row].as_mut().unwrap();
        let guy = &mut installed.facts.guys[0];
        collision::guy_set_new_location(
            terrain,
            (guy.x, guy.y),
            (body.x, body.y),
            installed.facts.domain,
            0,
            1,
            guy.block_radius,
        );
        guy.x = body.x;
        guy.y = body.y;
    }
    store.world.units.angle_mut()[row] = body.angle;
    if let Some(guy) = store.sources[row]
        .as_mut()
        .and_then(|s| s.facts.guys.first_mut())
    {
        guy.angle = body.angle;
    }
}

fn relocate_anchor(
    world: &mut World,
    terrain: &mut TerrainWorld,
    row: usize,
    x: i32,
    y: i32,
) -> Result<(), ()> {
    let who = world.units.get_who(row) as i32;
    let o = world.units.o()[row] as i32;
    let old = (
        movement::wcell_of(world.units.x_internal()[row]),
        movement::wcell_of(world.units.y_internal()[row]),
    );
    let new = (movement::wcell_of(x), movement::wcell_of(y));
    if !terrain.valid_w(old.0, old.1) || !terrain.valid_w(new.0, new.1) {
        return Err(());
    }
    if old == new {
        world.units.x_internal_mut()[row] = x;
        world.units.y_internal_mut()[row] = y;
        return Ok(());
    }

    // Resolve the old splice without mutation first.
    let head = terrain.wdata(old.0, old.1);
    let predecessor = if (head.down_who as i32, head.down as i32) == (who, o) {
        None
    } else {
        let mut cur = (head.down_who as i32, head.down as i32);
        let mut found = None;
        for _ in 0..=world.live_count() {
            let Some(cur_row) = object_row(world, cur.0, cur.1) else {
                break;
            };
            let next = (
                world.units.down_who()[cur_row] as i32,
                world.units.down()[cur_row] as i32,
            );
            if next == (who, o) {
                found = Some(cur_row);
                break;
            }
            cur = next;
        }
        Some(found.ok_or(())?)
    };

    let actor_next = (world.units.down()[row], world.units.down_who()[row]);
    if let Some(pred) = predecessor {
        world.units.down_mut()[pred] = actor_next.0;
        world.units.down_who_mut()[pred] = actor_next.1;
    } else {
        terrain.set_down(old.0, old.1, actor_next.0, actor_next.1);
    }
    world.units.down_mut()[row] = -1;
    world.units.down_who_mut()[row] = -1;
    world.units.x_internal_mut()[row] = x;
    world.units.y_internal_mut()[row] = y;

    let new_head = terrain.wdata(new.0, new.1);
    world.units.down_mut()[row] = new_head.down;
    world.units.down_who_mut()[row] = new_head.down_who;
    terrain.set_down(new.0, new.1, o as i16, who as i16);
    Ok(())
}

/// Actor-specific invalid-location host.  Repath remains an explicit fail-closed boundary until
/// the tick parks/restores the retail PathFinder containers per unit.
pub struct InstalledPathHost<'a> {
    pub row: usize,
    pub invalid_tiles: &'a [(i32, i32)],
    fault: Option<LiveCollisionFault>,
}

impl<'a> InstalledPathHost<'a> {
    pub fn new(row: usize, invalid_tiles: &'a [(i32, i32)]) -> Self {
        Self {
            row,
            invalid_tiles,
            fault: None,
        }
    }

    pub fn take_fault(&mut self) -> Option<LiveCollisionFault> {
        self.fault.take()
    }
}

impl crate::systems::movement_driver::CollisionPathHost for InstalledPathHost<'_> {
    fn invalid_loc(&self, tile_x: i32, tile_y: i32) -> bool {
        self.invalid_tiles.contains(&(tile_x, tile_y))
    }

    fn find_upath(&mut self, _path: &mut PathStack, _quick: bool) -> bool {
        self.fault = Some(LiveCollisionFault::MissingRepathHost(self.row));
        false
    }
}

/// Movement-only `UnitWorld` view whose invalid-location set was supplied with the installed
/// actor. Collision itself must enter through the typed callback; the legacy boolean query
/// panics if reached so it cannot silently regain authority.
pub struct InstalledMoveWorld<'a> {
    pub tiles_w: i32,
    pub tiles_h: i32,
    pub wcells_w: i32,
    pub invalid_tiles: &'a [(i32, i32)],
}

impl movement::UnitWorld for InstalledMoveWorld<'_> {
    fn tiles_w(&self) -> i32 {
        self.tiles_w
    }
    fn tiles_h(&self) -> i32 {
        self.tiles_h
    }
    fn wcells_w(&self) -> i32 {
        self.wcells_w
    }
    fn invalid_loc(&self, tile_x: i32, tile_y: i32) -> bool {
        self.invalid_tiles.contains(&(tile_x, tile_y))
    }
    fn unit_collides(&self, _x: i32, _y: i32) -> bool {
        panic!("live movement collision must use CollisionSession")
    }
    fn needs_transport(&self, _fx: i32, _fy: i32, _tx: i32, _ty: i32) -> i32 {
        0
    }
    fn tregion(&self, _tile_x: i32, _tile_y: i32) -> i32 {
        -1
    }
}

/// Pull one actor's installed facts without exposing the internal linked marker.
pub fn actor_ref(world: &World, row: usize) -> ActorRef {
    ActorRef {
        who: world.units.get_who(row) as i32,
        o: world.units.o()[row] as i32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systems::collision::{block_get, Resolve};
    use crate::systems::movement::ucell_of;
    use crate::systems::movement_driver::ActorCommitKind;

    fn source(x: i32, y: i32) -> LiveCollisionSource {
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
        }
    }

    fn leaders() -> (leaders::Leaders, victory_score::Leaders) {
        let mut ls = leaders::Leaders::new();
        ls.leaders[0].activate();
        let types =
            victory_score::TypeTable::with_default_kinds(victory_score::ScoreConstants::default());
        let mut vs = victory_score::Leaders::new(types);
        vs.slots[0].leader_flags = victory_score::leader_flag::VALID;
        (ls, vs)
    }

    #[test]
    fn movement_collision_install_links_anchor_and_stamps_supplied_live_guy() {
        let mut world = World::new(1);
        let mut terrain = TerrainWorld::init_default_rules(3, 3);
        let h1 = world.allocate_typed_at(0, 1, 360, 504).unwrap();
        let h2 = world.allocate_typed_at(0, 1, 600, 504).unwrap();
        world.units.guy_mark_mut()[0] = 1;
        world.units.guy_mark_mut()[1] = 1;
        let mut rt = LiveCollisionRuntime::new();
        rt.install(&mut world, &mut terrain, h1, source(360, 504))
            .unwrap();
        rt.install(&mut world, &mut terrain, h2, source(600, 504))
            .unwrap();

        let cell = terrain.wdata(movement::wcell_of(600), movement::wcell_of(504));
        assert_eq!((cell.down_who, cell.down), (0, 1));
        assert_eq!((world.units.down_who()[1], world.units.down()[1]), (0, 0));
        let (ux, uy) = (ucell_of(360), ucell_of(504));
        let block = terrain.wdata(ux >> 4, uy >> 4).block.as_deref().unwrap();
        assert!(block_get(block, ux, uy));
        let paths = vec![PathStack::new(), PathStack::new()];
        assert_eq!(rt.preflight(&world, &terrain, &paths), Ok(()));
    }

    #[test]
    fn loaded_collision_sources_rehydrate_without_relinking_or_restamping() {
        let mut world = World::new(31);
        let mut terrain = TerrainWorld::init_default_rules(3, 3);
        let h1 = world.allocate_typed_at(0, 1, 360, 504).unwrap();
        let h2 = world.allocate_typed_at(0, 1, 600, 504).unwrap();
        world.units.guy_mark_mut()[0] = 1;
        world.units.guy_mark_mut()[1] = 1;
        let source1 = source(360, 504);
        let source2 = source(600, 504);
        let mut installed = LiveCollisionRuntime::new();
        installed
            .install(&mut world, &mut terrain, h1, source1.clone())
            .unwrap();
        installed
            .install(&mut world, &mut terrain, h2, source2.clone())
            .unwrap();
        let head_before = {
            let head = terrain.wdata(movement::wcell_of(600), movement::wcell_of(504));
            (head.down_who, head.down)
        };
        let block_before = terrain
            .wdata(ucell_of(360) >> 4, ucell_of(504) >> 4)
            .block
            .clone();

        let mut loaded = LiveCollisionRuntime::new();
        assert_eq!(
            loaded.rehydrate_saved_sources(
                &world,
                &terrain,
                vec![(h1, source1.clone()), (h2, source2.clone())],
            ),
            Ok(2)
        );
        assert_eq!(loaded.source(0), Some(&source1));
        assert_eq!(loaded.source(1), Some(&source2));
        assert_eq!(
            {
                let head = terrain.wdata(movement::wcell_of(600), movement::wcell_of(504));
                (head.down_who, head.down)
            },
            head_before
        );
        assert_eq!(
            terrain.wdata(ucell_of(360) >> 4, ucell_of(504) >> 4).block,
            block_before
        );
        assert_eq!(
            loaded.preflight(&world, &terrain, &[PathStack::new(), PathStack::new()]),
            Ok(())
        );
    }

    #[test]
    fn loaded_collision_rehydration_rejects_one_altered_body_atomically() {
        let mut world = World::new(32);
        let mut terrain = TerrainWorld::init_default_rules(3, 3);
        let h1 = world.allocate_typed_at(0, 1, 360, 504).unwrap();
        let h2 = world.allocate_typed_at(0, 1, 600, 504).unwrap();
        world.units.guy_mark_mut()[0] = 1;
        world.units.guy_mark_mut()[1] = 1;
        let source1 = source(360, 504);
        let source2 = source(600, 504);
        let mut installed = LiveCollisionRuntime::new();
        installed
            .install(&mut world, &mut terrain, h1, source1.clone())
            .unwrap();
        installed
            .install(&mut world, &mut terrain, h2, source2.clone())
            .unwrap();

        let mut altered = source2;
        altered.guys[0].x = 984;
        let mut loaded = LiveCollisionRuntime::new();
        assert_eq!(
            loaded.rehydrate_saved_sources(&world, &terrain, vec![(h1, source1), (h2, altered)],),
            Err(LiveCollisionFault::SavedGuyActorMismatch { row: 1, guy: 0 })
        );
        assert!(loaded.sources.is_empty());
        assert!(loaded.order_state.is_empty());
    }

    #[test]
    fn movement_collision_missing_source_preflight_fails_closed() {
        let mut world = World::new(2);
        let terrain = TerrainWorld::init_default_rules(3, 3);
        let _ = world.allocate_typed_at(0, 1, 360, 504).unwrap();
        world.units.guy_mark_mut()[0] = 1;
        let mut rt = LiveCollisionRuntime::new();
        rt.ensure_rows(1);
        assert_eq!(
            rt.preflight(&world, &terrain, &[PathStack::new()]),
            Err(LiveCollisionFault::MissingSource(0))
        );
    }

    #[test]
    fn movement_collision_rejected_install_is_atomic() {
        let mut world = World::new(22);
        let mut terrain = TerrainWorld::init_default_rules(3, 3);
        let h = world.allocate_typed_at(0, 1, 360, 504).unwrap();
        world.units.guy_mark_mut()[0] = 2;
        let mut rt = LiveCollisionRuntime::new();
        let anchor = (movement::wcell_of(360), movement::wcell_of(504));
        let baseline_head = {
            let head = terrain.wdata(anchor.0, anchor.1);
            (head.down_who, head.down)
        };

        assert_eq!(
            rt.install(&mut world, &mut terrain, h, source(360, 504)),
            Err(LiveCollisionFault::InvalidGuyCount {
                column: 2,
                supplied: 1,
            })
        );
        let head = terrain.wdata(anchor.0, anchor.1);
        assert_eq!((head.down_who, head.down), baseline_head);
        assert!(rt.source(0).is_none());
        assert!(terrain
            .wdata(ucell_of(360) >> 4, ucell_of(504) >> 4)
            .block
            .is_none());
    }

    #[test]
    fn movement_collision_actor_commit_relocates_world_link_guy_stamp_and_generated_counters() {
        let mut world = World::new(3);
        let mut terrain = TerrainWorld::init_default_rules(3, 3);
        let h = world.allocate_typed_at(0, 1, 360, 504).unwrap();
        world.units.guy_mark_mut()[0] = 1;
        let mut rt = LiveCollisionRuntime::new();
        rt.install(&mut world, &mut terrain, h, source(360, 504))
            .unwrap();
        rt.snapshot_paths(&[PathStack::new()], 1);
        let (ls, vs) = leaders();
        let mut store = LiveCollisionStore::new(
            &mut world,
            &mut rt.sources,
            &mut rt.order_state,
            &rt.path_top_flags,
            &ls,
            &vs,
            &mut rt.repath_budget,
        );
        let before = store.row(0, 0).unwrap();
        let mut after = before;
        after.x = 900;
        after.y = 504;
        after.collide = 7;
        after.collide_o = 4;
        after.collide_who = 1;
        commit_actor(
            &mut terrain,
            &mut store,
            ActorCommit {
                actor: ActorRef { who: 0, o: 0 },
                before,
                after,
                kind: ActorCommitKind::Resolve(Resolve::Repath {
                    wait_drawn: None,
                    found: false,
                }),
            },
        );
        assert_eq!(store.take_fault(), None);
        assert_eq!(
            (
                store.world.units.x_internal()[0],
                store.world.units.y_internal()[0]
            ),
            (900, 504)
        );
        assert_eq!(store.world.units.collide()[0], 7);
        assert_eq!(
            (terrain.wdata(1, 0).down_who, terrain.wdata(1, 0).down),
            (0, 0)
        );
        let installed = store.sources[0].as_ref().unwrap();
        assert_eq!(
            (installed.facts.guys[0].x, installed.facts.guys[0].y),
            (900, 504)
        );
    }

    #[test]
    fn movement_collision_stale_actor_commit_has_no_spatial_or_counter_effect() {
        let mut world = World::new(23);
        let mut terrain = TerrainWorld::init_default_rules(3, 3);
        let h = world.allocate_typed_at(0, 1, 360, 504).unwrap();
        world.units.guy_mark_mut()[0] = 1;
        let mut rt = LiveCollisionRuntime::new();
        rt.install(&mut world, &mut terrain, h, source(360, 504))
            .unwrap();
        rt.snapshot_paths(&[PathStack::new()], 1);
        let (ls, vs) = leaders();
        let mut store = LiveCollisionStore::new(
            &mut world,
            &mut rt.sources,
            &mut rt.order_state,
            &rt.path_top_flags,
            &ls,
            &vs,
            &mut rt.repath_budget,
        );
        let mut stale = store.row(0, 0).unwrap();
        stale.collide = 9;
        let mut after = stale;
        after.x = 900;
        after.collide = 10;
        commit_actor(
            &mut terrain,
            &mut store,
            ActorCommit {
                actor: ActorRef { who: 0, o: 0 },
                before: stale,
                after,
                kind: ActorCommitKind::Resolve(Resolve::Wait),
            },
        );

        assert_eq!(
            store.take_fault(),
            Some(LiveCollisionFault::StaleActorCommit(0))
        );
        assert_eq!(store.world.units.x_internal()[0], 360);
        assert_eq!(store.world.units.collide()[0], 0);
        assert_eq!(store.sources[0].as_ref().unwrap().facts.guys[0].x, 360);
    }

    #[test]
    fn movement_collision_store_persists_detector_destination_and_reads_generated_identity() {
        let mut world = World::new(4);
        let mut terrain = TerrainWorld::init_default_rules(3, 3);
        let h = world.allocate_typed_at(0, 1, 360, 504).unwrap();
        world.units.guy_mark_mut()[0] = 1;
        let mut rt = LiveCollisionRuntime::new();
        rt.install(&mut world, &mut terrain, h, source(360, 504))
            .unwrap();
        rt.snapshot_paths(&[PathStack::new()], 1);
        let (ls, vs) = leaders();
        let mut store = LiveCollisionStore::new(
            &mut world,
            &mut rt.sources,
            &mut rt.order_state,
            &rt.path_top_flags,
            &ls,
            &vs,
            &mut rt.repath_budget,
        );
        store.set_order_dest(0, 0, 500, 700);
        assert_eq!(store.order_dest(0, 0), Some((500, 700)));
        let row = store.row(0, 0).unwrap();
        assert_eq!((row.who, row.o, row.x, row.y), (0, 0, 360, 504));
        assert_eq!(
            row.collide_o, -1,
            "installed no-blocker sentinel is authoritative"
        );
    }
}
