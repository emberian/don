//! Exact, fail-closed Units checksum adapter over the canonical sparse Unit owner.
//!
//! The older [`crate::state::SimBridge`] path images a `UnitCols` row and asks the
//! generated generic walker to do what it can.  That remains a useful coverage probe, but
//! it is not the retail walk: the generated schema deliberately leaves computed
//! `must_walk` bytes, ptype dereferences, pointer-owned arrays, concrete order nodes, and
//! non-empty `Guy` recursion unresolved.
//!
//! This module takes the opposite contract.  It emits the exact inherited
//! `SubObject -> Object -> Unit` checksum byte order for the bounded state it supports and
//! refuses the rest.  Paths retain the engine's `Stack<PathData>` capacity/increment
//! history, launching retains `SimpleArray<int>` history, and traversal comes from
//! `World::object_bands`, not the dense compatibility registry.  Non-empty `OrderList`
//! and `PtrArray<Guy>` state remain typed stops until their complete concrete owners exist.
//!
//! This is an executable producer prerequisite, not a replay setup producer and not a
//! claim that any recorded Units checksum matches.

use std::collections::BTreeMap;
use std::fmt;

use don_sim::container::{EngineArray, EngineStack};
use don_sim::generated::state::{unit, Pool, Repr, UnitCols};
use don_sim::order::OrderList;
use don_sim::systems::movement::PathData;
use don_sim::systems::sparse_object_bands_authority_frontier::{
    ObjectStorageClass, RetailBand, RetailObjectAddress, SparseSlotLifecycle,
};
use don_sim::world::{Handle, World, WorldObjectIdentity, OBJ_FLAG_ACTIVE};

/// `CheckSums::check_units(CheckSum*, int)` (checksums.cpp:502).
pub const CHECK_UNITS_VA: u32 = 0x0093_71d0;
/// `Unit::walk_data(DataWalk*)`.
pub const UNIT_WALK_VA: u32 = 0x0060_cf40;
/// `Unit::must_walk(DataWalk*)`.
pub const UNIT_MUST_WALK_VA: u32 = 0x0060_d040;
/// `Object::walk_data(DataWalk*)`.
pub const OBJECT_WALK_VA: u32 = 0x0064_7830;
/// `Object::must_walk(DataWalk*)`.
pub const OBJECT_MUST_WALK_VA: u32 = 0x0064_7930;
/// `SubObject::walk_data(DataWalk*)`.
pub const SUBOBJECT_WALK_VA: u32 = 0x0066_21d0;
/// `Stack<PathData>::walk_data(DataWalk*)`.
pub const PATH_STACK_WALK_VA: u32 = 0x0046_d8b0;
/// `OrderList::walk_data(DataWalk*)`.
pub const ORDER_LIST_WALK_VA: u32 = 0x0073_0270;
/// `PtrArray<Guy>::walk_data(DataWalk*, int)`.
pub const GUY_ARRAY_WALK_VA: u32 = 0x0046_df30;

/// Owner slots reached by the shipped Units checksum loop: `0..9`, not all ten object
/// owners and not the eight player-only slots used by Builds/Walls/Cities.
pub const UNITS_CHANNEL_OWNER_SLOTS: usize = 9;

/// Exact checksum bytes for one active Unit with null launching, an initialized-empty
/// path stack (`size=10`, `increment=-1`), no orders, and an empty Guy pointer array.
pub const EMPTY_LIVE_UNIT_WALK_BYTES: u64 = 186;

/// Object coordinates are stored XOR-obfuscated in retail object images.
const OBJECT_COORD_XOR: i32 = 0x0006_3637;

/// Current authority for the Unit-owned `PtrArray<Guy>` at `UnitData+0xe4`.
///
/// The zero-length arm is complete: retail hashes only the signed zero count.  A positive
/// length additionally hashes capacity/increment/flags, per-slot presence, and recursively
/// calls each concrete `Guy::walk_data`; neither `World` nor this adapter owns those rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuyArrayWalkFacts {
    InitializedEmpty,
    NonEmptyUnsupported { length: usize },
}

/// Pointer/container state reached by one Unit walk but not completely owned by `World`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnitWalkFacts {
    /// Identity behind `UnitData::ptype` at `+0x18`. `None` is an authoritative null
    /// pointer and hashes a zero type id; it is not inferred from a missing sidecar row.
    pub ptype_index: Option<i32>,
    /// `ObjectData::launching` at `+0x44`. `None` is authoritative null; `Some(empty)`
    /// hashes a presence byte and a zero length and therefore differs.
    pub launching: Option<EngineArray<i32>>,
    /// Engine-shaped `Stack<PathData>` including `size`, `length`, and byte increment.
    pub path: EngineStack<PathData>,
    /// Bounded authority for `UnitData::guys` at `+0xe4`.
    pub guys: GuyArrayWalkFacts,
}

impl UnitWalkFacts {
    /// Explicit first-allocation shape used by focused tests and future setup producers.
    /// This constructor does not assert that retail setup has actually installed it.
    pub fn initialized_empty(ptype_index: Option<i32>) -> Self {
        Self {
            ptype_index,
            launching: None,
            path: EngineStack::new(-1),
            guys: GuyArrayWalkFacts::InitializedEmpty,
        }
    }
}

/// Explicit facts keyed by stable generational identity rather than dense row.
/// Compaction therefore cannot silently attach one Unit's ptype/path state to another.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UnitsWalkAuthority {
    rows: BTreeMap<(u32, u32), UnitWalkFacts>,
}

impl UnitsWalkAuthority {
    pub fn install(&mut self, handle: Handle, facts: UnitWalkFacts) -> Option<UnitWalkFacts> {
        self.rows.insert((handle.id, handle.generation), facts)
    }

    pub fn remove(&mut self, handle: Handle) -> Option<UnitWalkFacts> {
        self.rows.remove(&(handle.id, handle.generation))
    }

    pub fn get(&self, handle: Handle) -> Option<&UnitWalkFacts> {
        self.rows.get(&(handle.id, handle.generation))
    }
}

/// One exact Unit contribution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnitWalkValue {
    pub checksum: u32,
    pub bytes_walked: u64,
}

/// Exact isolated Units-channel value (fresh Adler seed `1`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnitsChannelValue {
    pub checksum: u32,
    pub bytes_walked: u64,
    pub units_walked: u32,
    /// Sparse Unit slots below their owner high-water marks, including tombstones and
    /// inactive live rows which retail inspects before applying its `flags & 1` gate.
    pub registry_entries: u32,
    /// Live active rows in owner slot 9, which the shipped checksum loop does not reach.
    pub skipped_outside_walk: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnitWalkError {
    RowOutOfRange {
        row: usize,
        rows: usize,
    },
    InactiveUnit {
        row: usize,
        flags: u8,
    },
    NonEmptyOrders {
        row: usize,
        length: usize,
    },
    NonEmptyGuyArrayUnsupported {
        row: usize,
        length: usize,
    },
    ContainerLengthOverflow {
        container: &'static str,
        length: usize,
    },
    ContainerCapacity {
        container: &'static str,
        length: usize,
        size: i32,
    },
}

impl fmt::Display for UnitWalkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RowOutOfRange { row, rows } => {
                write!(f, "Unit row {row} is outside {rows} generated rows")
            }
            Self::InactiveUnit { row, flags } => write!(
                f,
                "Unit row {row} has flags 0x{flags:02x}; check_units would not call walk_data"
            ),
            Self::NonEmptyOrders { row, length } => write!(
                f,
                "Unit row {row} has {length} flattened orders without complete concrete walk payloads"
            ),
            Self::NonEmptyGuyArrayUnsupported { row, length } => write!(
                f,
                "Unit row {row} has {length} Guy pointers without recursive Guy walk authority"
            ),
            Self::ContainerLengthOverflow { container, length } => write!(
                f,
                "Unit {container} length {length} does not fit retail's signed 32-bit field"
            ),
            Self::ContainerCapacity {
                container,
                length,
                size,
            } => write!(
                f,
                "Unit {container} length {length} exceeds engine capacity {size}"
            ),
        }
    }
}

impl std::error::Error for UnitWalkError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnitsRuntimeError {
    UnitLengthMismatch {
        columns: usize,
        live: u32,
    },
    InvalidOwnerMark {
        owner: usize,
        mark: i32,
    },
    MissingSparseSlot {
        owner: usize,
        o: i32,
        mark: i32,
    },
    WrongUnitStorage {
        owner: usize,
        o: i32,
    },
    ReservedSparseSlot {
        owner: usize,
        o: i32,
    },
    ActiveTombstone {
        owner: usize,
        o: i32,
        flags: u8,
    },
    LiveBeyondMark {
        owner: usize,
        o: i32,
        mark: i32,
    },
    WrongSparseIdentity {
        owner: usize,
        o: i32,
        identity: WorldObjectIdentity,
    },
    SparseIdentityUnresolved {
        owner: usize,
        o: i32,
        id: u32,
        generation: u32,
    },
    DenseIdentityMismatch {
        owner: usize,
        o: i32,
        row: usize,
        expected: Handle,
        actual: Option<Handle>,
    },
    DuplicateDenseRow {
        row: usize,
        first_owner: usize,
        first_o: i32,
        second_owner: usize,
        second_o: i32,
    },
    UnitOwnerMismatch {
        row: usize,
        sparse_owner: usize,
        unit_owner: u8,
    },
    UnitObjectIndexMismatch {
        row: usize,
        owner: usize,
        sparse_o: i32,
        unit_o: i16,
    },
    UnregisteredUnitRow {
        row: usize,
    },
    AuthorityForDeadUnit {
        id: u32,
        generation: u32,
    },
    MissingWalkAuthority {
        row: usize,
        owner: usize,
        o: i32,
        handle: Handle,
    },
    UnitWalk {
        row: usize,
        owner: usize,
        o: i32,
        source: UnitWalkError,
    },
    CountOverflow,
}

impl fmt::Display for UnitsRuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnitLengthMismatch { columns, live } => {
                write!(f, "World has {columns} Unit rows but live_count is {live}")
            }
            Self::InvalidOwnerMark { owner, mark } => {
                write!(f, "Unit owner {owner} has invalid exclusive mark {mark}")
            }
            Self::MissingSparseSlot { owner, o, mark } => write!(
                f,
                "Unit owner {owner} is missing retained slot {o} below mark {mark}"
            ),
            Self::WrongUnitStorage { owner, o } => write!(
                f,
                "sparse Unit address ({owner},{o}) lacks parallel Unit storage"
            ),
            Self::ReservedSparseSlot { owner, o } => {
                write!(f, "sparse Unit address ({owner},{o}) is still reserved")
            }
            Self::ActiveTombstone { owner, o, flags } => write!(
                f,
                "sparse Unit tombstone ({owner},{o}) has active flags 0x{flags:02x} but no walkable row"
            ),
            Self::LiveBeyondMark { owner, o, mark } => write!(
                f,
                "live sparse Unit ({owner},{o}) lies beyond exclusive mark {mark}"
            ),
            Self::WrongSparseIdentity { owner, o, identity } => write!(
                f,
                "sparse Unit address ({owner},{o}) carries non-Unit identity {identity:?}"
            ),
            Self::SparseIdentityUnresolved {
                owner,
                o,
                id,
                generation,
            } => write!(
                f,
                "sparse Unit ({owner},{o}) identity ({id},{generation}) has no live dense row"
            ),
            Self::DenseIdentityMismatch {
                owner,
                o,
                row,
                expected,
                actual,
            } => write!(
                f,
                "sparse Unit ({owner},{o}) resolved row {row}, expected {expected:?}, found {actual:?}"
            ),
            Self::DuplicateDenseRow {
                row,
                first_owner,
                first_o,
                second_owner,
                second_o,
            } => write!(
                f,
                "Unit row {row} is registered twice at ({first_owner},{first_o}) and ({second_owner},{second_o})"
            ),
            Self::UnitOwnerMismatch {
                row,
                sparse_owner,
                unit_owner,
            } => write!(
                f,
                "Unit row {row} sparse owner {sparse_owner} disagrees with UnitData.who {unit_owner}"
            ),
            Self::UnitObjectIndexMismatch {
                row,
                owner,
                sparse_o,
                unit_o,
            } => write!(
                f,
                "Unit row {row} at owner {owner} sparse o {sparse_o} disagrees with UnitData.o {unit_o}"
            ),
            Self::UnregisteredUnitRow { row } => {
                write!(f, "live Unit row {row} has no canonical sparse address")
            }
            Self::AuthorityForDeadUnit { id, generation } => write!(
                f,
                "Unit walk authority exists for dead identity ({id},{generation})"
            ),
            Self::MissingWalkAuthority {
                row,
                owner,
                o,
                handle,
            } => write!(
                f,
                "active Unit row {row} at ({owner},{o}) identity {handle:?} lacks walk authority"
            ),
            Self::UnitWalk {
                row,
                owner,
                o,
                source,
            } => write!(
                f,
                "active Unit row {row} at ({owner},{o}) cannot be walked: {source}"
            ),
            Self::CountOverflow => write!(f, "Units channel count or byte count overflowed"),
        }
    }
}

impl std::error::Error for UnitsRuntimeError {}

/// Produce the exact bytes passed to `CheckSum::walk_function` by one active Unit in the
/// supported bounded domain.
///
/// Tags are absent because `CheckSum::walk_test` is the shipped no-op at `0x0041bfe0`.
/// The three literal `1` bytes are the concrete Unit's `must_walk` result emitted once by
/// SubObject, once by Object, and once by Unit.  They are part of the checksum stream.
pub fn unit_walk_bytes(
    units: &UnitCols,
    row: usize,
    orders: &OrderList,
    facts: &UnitWalkFacts,
) -> Result<Vec<u8>, UnitWalkError> {
    if row >= units.len() {
        return Err(UnitWalkError::RowOutOfRange {
            row,
            rows: units.len(),
        });
    }
    let flags = units.get_flags(row);
    if flags & OBJ_FLAG_ACTIVE == 0 {
        return Err(UnitWalkError::InactiveUnit { row, flags });
    }
    if !orders.is_empty() {
        return Err(UnitWalkError::NonEmptyOrders {
            row,
            length: orders.len(),
        });
    }
    if let GuyArrayWalkFacts::NonEmptyUnsupported { length } = facts.guys {
        return Err(UnitWalkError::NonEmptyGuyArrayUnsupported { row, length });
    }

    let image = unit_image(units, row);
    let mut out = Vec::with_capacity(EMPTY_LIVE_UNIT_WALK_BYTES as usize);

    // SubObject::walk_data 0x006621d0.
    out.push(image[0x08]);
    out.push(1); // concrete Unit::must_walk; active flag makes the result true
    out.extend_from_slice(&image[0x09..0x18]);
    out.extend_from_slice(&facts.ptype_index.unwrap_or(0).to_le_bytes());

    // Object::walk_data 0x00647830.
    out.push(1);
    out.extend_from_slice(&image[0x20..0x42]);
    out.push(facts.launching.is_some() as u8);
    if let Some(launching) = &facts.launching {
        append_simple_array_i32(&mut out, launching)?;
    }

    // Unit::walk_data 0x0060cf40, with check_units' section mask = -1.
    out.push(1);
    out.extend_from_slice(&image[0x48..0xb7]);
    append_path_stack(&mut out, &facts.path)?;

    // OrderList::walk_data: the signed count. Positive counts are refused above because
    // every node then needs type, node tag, and concrete virtual walk bytes.
    out.extend_from_slice(&0i32.to_le_bytes());

    // PtrArray<Guy>::walk_data: zero length returns immediately and hashes no array
    // history. Positive lengths recurse into concrete Guys and are refused above.
    out.extend_from_slice(&0i32.to_le_bytes());
    Ok(out)
}

pub fn unit_walk_value(
    units: &UnitCols,
    row: usize,
    orders: &OrderList,
    facts: &UnitWalkFacts,
) -> Result<UnitWalkValue, UnitWalkError> {
    let bytes = unit_walk_bytes(units, row, orders, facts)?;
    Ok(UnitWalkValue {
        checksum: don_sim::checksum::adler32(1, &bytes),
        bytes_walked: bytes.len() as u64,
    })
}

#[derive(Clone, Copy)]
struct ReachedUnit {
    owner: usize,
    o: i32,
    row: usize,
    handle: Handle,
}

/// Execute the exact fixed-owner `CheckSums::check_units` traversal over `World`'s
/// canonical sparse owner.
///
/// The owner loop is `0..9` and unrotated.  Sparse retained slots are visited in ascending
/// `o`; tombstones are skipped only after their stored flags prove retail's `flags & 1`
/// gate false.  Stable identity is resolved back to a dense Unit row and cross-checked
/// against the row's `{who,o}` fields before any byte is hashed.
pub fn check_world_units(
    world: &World,
    authority: &UnitsWalkAuthority,
) -> Result<UnitsChannelValue, UnitsRuntimeError> {
    if world.units.len() != world.live_count() as usize {
        return Err(UnitsRuntimeError::UnitLengthMismatch {
            columns: world.units.len(),
            live: world.live_count(),
        });
    }

    let owner = world.object_bands();
    let mut seen: Vec<Option<(usize, i32)>> = vec![None; world.units.len()];
    let mut reached = Vec::new();
    let mut registry_entries = 0u32;
    let mut skipped_outside_walk = 0u32;

    for who in 0..don_sim::objects::OWNER_SLOTS {
        let mark =
            owner
                .mark(who, RetailBand::Unit)
                .ok_or(UnitsRuntimeError::InvalidOwnerMark {
                    owner: who,
                    mark: -1,
                })?;
        if !(RetailBand::Unit.base()..=RetailBand::Unit.limit()).contains(&mark) {
            return Err(UnitsRuntimeError::InvalidOwnerMark { owner: who, mark });
        }
        let retained = owner
            .retained_slots(who, RetailBand::Unit)
            .ok_or(UnitsRuntimeError::InvalidOwnerMark { owner: who, mark })?;
        let active_owner = owner
            .is_active(who)
            .ok_or(UnitsRuntimeError::InvalidOwnerMark { owner: who, mark })?;

        for offset in 0..retained {
            let o = RetailBand::Unit.base() + offset as i32;
            let address = RetailObjectAddress::new(who as u8, RetailBand::Unit, o);
            let slot = owner
                .slot(address)
                .ok_or(UnitsRuntimeError::MissingSparseSlot {
                    owner: who,
                    o,
                    mark,
                })?;
            if !matches!(
                slot.storage.class,
                ObjectStorageClass::Unit | ObjectStorageClass::Animal
            ) || slot.storage.unit.is_none()
            {
                return Err(UnitsRuntimeError::WrongUnitStorage { owner: who, o });
            }
            if o < mark {
                registry_entries = registry_entries
                    .checked_add(1)
                    .ok_or(UnitsRuntimeError::CountOverflow)?;
            }

            match slot.lifecycle {
                SparseSlotLifecycle::Tombstone(tombstone) => {
                    if o < mark && tombstone.flags & OBJ_FLAG_ACTIVE != 0 {
                        return Err(UnitsRuntimeError::ActiveTombstone {
                            owner: who,
                            o,
                            flags: tombstone.flags,
                        });
                    }
                }
                SparseSlotLifecycle::Reserved { .. } => {
                    return Err(UnitsRuntimeError::ReservedSparseSlot { owner: who, o });
                }
                SparseSlotLifecycle::Live(identity) => {
                    if o >= mark {
                        return Err(UnitsRuntimeError::LiveBeyondMark {
                            owner: who,
                            o,
                            mark,
                        });
                    }
                    let WorldObjectIdentity::Unit { id, generation } = identity else {
                        return Err(UnitsRuntimeError::WrongSparseIdentity {
                            owner: who,
                            o,
                            identity,
                        });
                    };
                    let row = world.unit_row_at(who as i32, o).ok_or(
                        UnitsRuntimeError::SparseIdentityUnresolved {
                            owner: who,
                            o,
                            id,
                            generation,
                        },
                    )?;
                    let handle = Handle { id, generation };
                    let actual = world.handle_at_row(row);
                    if actual != Some(handle) {
                        return Err(UnitsRuntimeError::DenseIdentityMismatch {
                            owner: who,
                            o,
                            row,
                            expected: handle,
                            actual,
                        });
                    }
                    if let Some((first_owner, first_o)) = seen[row] {
                        return Err(UnitsRuntimeError::DuplicateDenseRow {
                            row,
                            first_owner,
                            first_o,
                            second_owner: who,
                            second_o: o,
                        });
                    }
                    seen[row] = Some((who, o));
                    let unit_owner = world.units.get_who(row);
                    if unit_owner as usize != who {
                        return Err(UnitsRuntimeError::UnitOwnerMismatch {
                            row,
                            sparse_owner: who,
                            unit_owner,
                        });
                    }
                    let unit_o = world.units.o()[row];
                    if i32::from(unit_o) != o {
                        return Err(UnitsRuntimeError::UnitObjectIndexMismatch {
                            row,
                            owner: who,
                            sparse_o: o,
                            unit_o,
                        });
                    }
                    if world.units.get_flags(row) & OBJ_FLAG_ACTIVE == 0 || !active_owner {
                        continue;
                    }
                    if who >= UNITS_CHANNEL_OWNER_SLOTS {
                        skipped_outside_walk = skipped_outside_walk
                            .checked_add(1)
                            .ok_or(UnitsRuntimeError::CountOverflow)?;
                        continue;
                    }
                    reached.push(ReachedUnit {
                        owner: who,
                        o,
                        row,
                        handle,
                    });
                }
            }
        }

        // A mark cannot name storage which is absent. This loop is separate from retained
        // iteration so a truncated sparse array fails before being mistaken for tombstones.
        if retained < mark as usize {
            let o = RetailBand::Unit.base() + retained as i32;
            return Err(UnitsRuntimeError::MissingSparseSlot {
                owner: who,
                o,
                mark,
            });
        }
    }

    if let Some(row) = seen.iter().position(Option::is_none) {
        return Err(UnitsRuntimeError::UnregisteredUnitRow { row });
    }
    for &(id, generation) in authority.rows.keys() {
        if world.row_of(Handle { id, generation }).is_none() {
            return Err(UnitsRuntimeError::AuthorityForDeadUnit { id, generation });
        }
    }

    let mut checksum = 1u32;
    let mut bytes_walked = 0u64;
    let mut units_walked = 0u32;
    for reached in reached {
        let facts =
            authority
                .get(reached.handle)
                .ok_or(UnitsRuntimeError::MissingWalkAuthority {
                    row: reached.row,
                    owner: reached.owner,
                    o: reached.o,
                    handle: reached.handle,
                })?;
        let bytes = unit_walk_bytes(&world.units, reached.row, world.orders(reached.row), facts)
            .map_err(|source| UnitsRuntimeError::UnitWalk {
                row: reached.row,
                owner: reached.owner,
                o: reached.o,
                source,
            })?;
        checksum = don_sim::checksum::adler32(checksum, &bytes);
        bytes_walked = bytes_walked
            .checked_add(bytes.len() as u64)
            .ok_or(UnitsRuntimeError::CountOverflow)?;
        units_walked = units_walked
            .checked_add(1)
            .ok_or(UnitsRuntimeError::CountOverflow)?;
    }

    Ok(UnitsChannelValue {
        checksum,
        bytes_walked,
        units_walked,
        registry_entries,
        skipped_outside_walk,
    })
}

fn append_path_stack(out: &mut Vec<u8>, path: &EngineStack<PathData>) -> Result<(), UnitWalkError> {
    let (size, length, increment) = path.checksum_header();
    let length_usize =
        usize::try_from(length).map_err(|_| UnitWalkError::ContainerLengthOverflow {
            container: "path",
            length: path.len(),
        })?;
    if size < length || size < 0 {
        return Err(UnitWalkError::ContainerCapacity {
            container: "path",
            length: length_usize,
            size,
        });
    }
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&length.to_le_bytes());
    out.push(increment as u8);
    for record in path.as_slice() {
        out.extend_from_slice(&record.to_x.to_le_bytes());
        out.extend_from_slice(&record.to_y.to_le_bytes());
        out.extend_from_slice(&record.tolerance.to_le_bytes());
        out.extend_from_slice(&record.flags.to_le_bytes());
    }
    Ok(())
}

fn append_simple_array_i32(
    out: &mut Vec<u8>,
    array: &EngineArray<i32>,
) -> Result<(), UnitWalkError> {
    let (length, size, increment, flags) = array.checksum_header();
    let length_usize =
        usize::try_from(length).map_err(|_| UnitWalkError::ContainerLengthOverflow {
            container: "launching",
            length: array.len(),
        })?;
    out.extend_from_slice(&length.to_le_bytes());
    if length == 0 {
        return Ok(());
    }
    if size < length || size < 0 {
        return Err(UnitWalkError::ContainerCapacity {
            container: "launching",
            length: length_usize,
            size,
        });
    }
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&increment.to_le_bytes());
    out.push(flags & !0x40);
    for &element in array.as_slice() {
        out.extend_from_slice(&element.to_le_bytes());
    }
    Ok(())
}

/// Table-driven scalar image only. The byte windows selected from it are exact PE-derived
/// windows above; no generic walk specification is consulted here.
fn unit_image(units: &UnitCols, row: usize) -> Vec<u8> {
    let mut image = vec![0u8; unit::DESC.sizeof as usize];
    for field in unit::FIELDS.iter() {
        if field.alias_of.is_some() || !field.repr.materialised() || field.count == 0 {
            continue;
        }
        let elements = field.count as usize;
        let element_size = (field.size / field.count) as usize;
        let masked = matches!(field.name, "z_internal" | "x_internal" | "y_internal");
        for index in 0..elements {
            let plane = field.plane as usize + index;
            let offset = field.offset as usize + index * element_size;
            let destination = &mut image[offset..offset + element_size];
            match field.pool {
                Pool::W4 => {
                    let value = units.w4_plane(plane)[row];
                    let value = if masked {
                        value ^ OBJECT_COORD_XOR
                    } else {
                        value
                    };
                    let bytes = match field.repr {
                        Repr::U32 => (value as u32).to_le_bytes(),
                        _ => value.to_le_bytes(),
                    };
                    destination.copy_from_slice(&bytes[..element_size]);
                }
                Pool::W2 => destination
                    .copy_from_slice(&units.w2_plane(plane)[row].to_le_bytes()[..element_size]),
                Pool::W1 => destination[0] = units.w1_plane(plane)[row] as u8,
                Pool::WF => destination
                    .copy_from_slice(&units.wf_slice(plane)[row].to_le_bytes()[..element_size]),
                Pool::None => unreachable!("materialised Unit field has no storage pool"),
            }
        }
    }
    image
}
