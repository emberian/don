//! Exact, fail-closed Guys checksum adapter over the canonical sparse Unit owner.
//!
//! `CheckSums::check_guys` does not walk a flat Guy pool.  It visits active Unit objects
//! in fixed `(owner, object-index)` order and invokes the `PtrArray<Guy>` embedded at
//! `UnitData+0xe4`.  The array walk hashes allocation history and null-pointer topology
//! before recursively hashing each non-null Guy's flat 155-byte synchronized image.
//!
//! [`don_sim::systems::groups_guys::UnitGuys`] already carries that complete byte state,
//! but normal [`don_sim::world::World`] rows do not own one.  This module binds UnitGuys to
//! stable generational Unit identity, validates every dynamic/container and identity edge,
//! and executes the shipped nine-owner traversal.  It does not synthesize initial Guys or
//! install itself as a replay channel producer.

use std::collections::BTreeMap;
use std::fmt;

use don_sim::systems::groups_guys::{GuyData, UnitGuys, GUY_WALK_LEN};
use don_sim::systems::sparse_object_bands_authority_frontier::{
    ObjectStorageClass, RetailBand, RetailObjectAddress, SparseSlotLifecycle,
};
use don_sim::world::{Handle, World, WorldObjectIdentity, OBJ_FLAG_ACTIVE};

/// `CheckSums::check_guys(CheckSum*)`.
pub const CHECK_GUYS_VA: u32 = 0x0093_7430;
/// `PtrArray<Guy>::walk_data(DataWalk*)`.
pub const GUY_ARRAY_WALK_VA: u32 = 0x0046_df30;
/// `GuyData::walk_data(DataWalk*)`.
pub const GUY_DATA_WALK_VA: u32 = 0x005e_0210;

/// Owner records reached by `check_guys`: `(0xe789dc-0xe3a390)/0x6eec == 9`.
///
/// This deliberately differs from the eight playable Group owners and from the ten owner
/// records used by `Objects::process_all`.
pub const GUYS_CHANNEL_OWNER_SLOTS: usize = 9;

/// Bytes emitted by a positive-length `PtrArray<Guy>` before recursive Guy images:
/// length, capacity, increment, masked flags, one presence byte per slot, then capacity
/// and increment a second time.
pub const NONEMPTY_GUY_ARRAY_FIXED_BYTES: usize = 17;

/// Exact driver-owned gates which are not fields of a Unit or Guy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuysDriverFacts {
    /// `Objects+0x1f4`; false makes the retail function return without reading a band.
    pub objects_valid: bool,
    /// Low bit of the nine leader records reached by the driver, in address order.
    pub leader_active: [bool; GUYS_CHANNEL_OWNER_SLOTS],
}

impl GuysDriverFacts {
    pub const fn all_active() -> Self {
        Self {
            objects_valid: true,
            leader_active: [true; GUYS_CHANNEL_OWNER_SLOTS],
        }
    }
}

impl Default for GuysDriverFacts {
    fn default() -> Self {
        Self {
            objects_valid: true,
            leader_active: [false; GUYS_CHANNEL_OWNER_SLOTS],
        }
    }
}

/// Exact Guy pointer-array state for one stable Unit identity.
///
/// The wrapper is intentionally small: the canonical Sim type already owns every walked
/// byte and the array metadata.  A separate type would create two byte layouts to keep in
/// sync.
#[derive(Clone, Debug, PartialEq)]
pub struct GuyWalkFacts {
    pub guys: UnitGuys,
}

impl GuyWalkFacts {
    pub fn new(guys: UnitGuys) -> Self {
        Self { guys }
    }
}

/// Explicit Guy state keyed by generational identity rather than compactable dense row.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GuysWalkAuthority {
    rows: BTreeMap<(u32, u32), GuyWalkFacts>,
}

impl GuysWalkAuthority {
    pub fn install(&mut self, handle: Handle, facts: GuyWalkFacts) -> Option<GuyWalkFacts> {
        self.rows.insert((handle.id, handle.generation), facts)
    }

    pub fn remove(&mut self, handle: Handle) -> Option<GuyWalkFacts> {
        self.rows.remove(&(handle.id, handle.generation))
    }

    pub fn get(&self, handle: Handle) -> Option<&GuyWalkFacts> {
        self.rows.get(&(handle.id, handle.generation))
    }

    pub fn get_mut(&mut self, handle: Handle) -> Option<&mut GuyWalkFacts> {
        self.rows.get_mut(&(handle.id, handle.generation))
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// One exact `PtrArray<Guy>` contribution from a fresh Adler seed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuyArrayWalkValue {
    pub checksum: u32,
    pub bytes_walked: u64,
    pub guys_walked: u32,
    pub null_slots: u32,
    /// Persistent value of `PtrArray+0x14` after the walk clears bit `0x40`.
    pub flags_after: u8,
}

/// Exact isolated Guys-channel value from a fresh Adler seed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuysChannelValue {
    pub checksum: u32,
    pub bytes_walked: u64,
    pub units_walked: u32,
    pub guys_walked: u32,
    pub null_slots: u32,
    /// Unit-band slots below their owner high-water marks, including tombstones.
    pub registry_entries: u32,
    pub skipped_inactive_leader: u32,
    pub skipped_inactive_unit: u32,
    /// Live active Units in owner slot 9, which the shipped loop does not reach.
    pub skipped_outside_walk: u32,
    /// Reached positive-length arrays whose persistent `0x40` flag was cleared after
    /// full preflight. Empty arrays return before the retail clear.
    pub arrays_masked: u32,
}

impl GuysChannelValue {
    fn empty() -> Self {
        Self {
            checksum: 1,
            bytes_walked: 0,
            units_walked: 0,
            guys_walked: 0,
            null_slots: 0,
            registry_entries: 0,
            skipped_inactive_leader: 0,
            skipped_inactive_unit: 0,
            skipped_outside_walk: 0,
            arrays_masked: 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GuyArrayWalkError {
    LengthOverflow {
        length: usize,
    },
    NegativeCapacity {
        size: i32,
    },
    LengthExceedsCapacity {
        length: usize,
        size: i32,
    },
    NegativeGuyMark {
        guy_mark: i8,
    },
    GuyMarkExceedsLength {
        guy_mark: usize,
        length: usize,
    },
    MissingSquadPrefix {
        slot: usize,
        guy_mark: usize,
    },
    GuyNumberOverflow {
        slot: usize,
    },
    GuyIdentityMismatch {
        slot: usize,
        expected_who: i8,
        actual_who: i8,
        expected_o: i16,
        actual_o: i16,
        expected_guy_num: i8,
        actual_guy_num: i8,
    },
}

impl fmt::Display for GuyArrayWalkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LengthOverflow { length } => {
                write!(f, "Guy pointer-array length {length} exceeds signed 32-bit")
            }
            Self::NegativeCapacity { size } => {
                write!(f, "non-empty Guy pointer array has negative capacity {size}")
            }
            Self::LengthExceedsCapacity { length, size } => write!(
                f,
                "Guy pointer-array length {length} exceeds engine capacity {size}"
            ),
            Self::NegativeGuyMark { guy_mark } => {
                write!(f, "UnitData::guy_mark is negative ({guy_mark})")
            }
            Self::GuyMarkExceedsLength { guy_mark, length } => write!(
                f,
                "UnitData::guy_mark {guy_mark} exceeds Guy pointer-array length {length}"
            ),
            Self::MissingSquadPrefix { slot, guy_mark } => write!(
                f,
                "Guy slot {slot} is null inside the live squad prefix 0..{guy_mark}"
            ),
            Self::GuyNumberOverflow { slot } => {
                write!(f, "non-null Guy slot {slot} cannot fit GuyData::guy_num")
            }
            Self::GuyIdentityMismatch {
                slot,
                expected_who,
                actual_who,
                expected_o,
                actual_o,
                expected_guy_num,
                actual_guy_num,
            } => write!(
                f,
                "Guy slot {slot} identity ({actual_who},{actual_o},{actual_guy_num}) does not match owning Unit ({expected_who},{expected_o},{expected_guy_num})"
            ),
        }
    }
}

impl std::error::Error for GuyArrayWalkError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GuysRuntimeError {
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
    ActiveTombstone {
        owner: usize,
        o: i32,
        flags: u8,
    },
    ReservedSparseSlot {
        owner: usize,
        o: i32,
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
    MissingWalkAuthority {
        row: usize,
        owner: usize,
        o: i32,
        handle: Handle,
    },
    GuyMarkMismatch {
        row: usize,
        columns: i8,
        authority: i8,
    },
    GuyArrayWalk {
        row: usize,
        owner: usize,
        o: i32,
        source: GuyArrayWalkError,
    },
    AuthorityForDeadUnit {
        id: u32,
        generation: u32,
    },
    CountOverflow,
}

impl fmt::Display for GuysRuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnitLengthMismatch { columns, live } => write!(
                f,
                "generated Unit columns have {columns} rows but World reports {live} live Units"
            ),
            Self::InvalidOwnerMark { owner, mark } => {
                write!(f, "owner {owner} has invalid Unit high-water mark {mark}")
            }
            Self::MissingSparseSlot { owner, o, mark } => write!(
                f,
                "owner {owner} Unit slot {o} is missing below high-water mark {mark}"
            ),
            Self::WrongUnitStorage { owner, o } => {
                write!(f, "owner {owner} Unit slot {o} lacks Unit parallel storage")
            }
            Self::ActiveTombstone { owner, o, flags } => write!(
                f,
                "owner {owner} Unit tombstone {o} retains active flags 0x{flags:02x}"
            ),
            Self::ReservedSparseSlot { owner, o } => {
                write!(f, "owner {owner} Unit slot {o} is reserved during checksum")
            }
            Self::LiveBeyondMark { owner, o, mark } => write!(
                f,
                "owner {owner} live Unit {o} lies at or beyond high-water mark {mark}"
            ),
            Self::WrongSparseIdentity { owner, o, identity } => write!(
                f,
                "owner {owner} Unit slot {o} carries non-Unit identity {identity:?}"
            ),
            Self::SparseIdentityUnresolved { owner, o, id, generation } => write!(
                f,
                "owner {owner} Unit slot {o} identity ({id},{generation}) does not resolve"
            ),
            Self::DenseIdentityMismatch { owner, o, row, expected, actual } => write!(
                f,
                "owner {owner} Unit slot {o} resolved row {row}, expected {expected:?}, found {actual:?}"
            ),
            Self::DuplicateDenseRow {
                row,
                first_owner,
                first_o,
                second_owner,
                second_o,
            } => write!(
                f,
                "dense Unit row {row} is aliased by ({first_owner},{first_o}) and ({second_owner},{second_o})"
            ),
            Self::UnitOwnerMismatch { row, sparse_owner, unit_owner } => write!(
                f,
                "Unit row {row} says owner {unit_owner}, sparse owner is {sparse_owner}"
            ),
            Self::UnitObjectIndexMismatch { row, owner, sparse_o, unit_o } => write!(
                f,
                "Unit row {row} owner {owner} says object {unit_o}, sparse object is {sparse_o}"
            ),
            Self::MissingWalkAuthority { row, owner, o, handle } => write!(
                f,
                "active Unit row {row} at ({owner},{o}) identity {handle:?} lacks Guy authority"
            ),
            Self::GuyMarkMismatch { row, columns, authority } => write!(
                f,
                "Unit row {row} column guy_mark {columns} differs from Guy authority {authority}"
            ),
            Self::GuyArrayWalk { row, owner, o, source } => write!(
                f,
                "active Unit row {row} at ({owner},{o}) cannot walk Guys: {source}"
            ),
            Self::AuthorityForDeadUnit { id, generation } => write!(
                f,
                "Guy authority exists for dead Unit identity ({id},{generation})"
            ),
            Self::CountOverflow => write!(f, "Guys channel count or byte count overflowed"),
        }
    }
}

impl std::error::Error for GuysRuntimeError {}

fn validate_guy_identity(
    slot: usize,
    guy: &GuyData,
    expected_who: i8,
    expected_o: i16,
) -> Result<(), GuyArrayWalkError> {
    let expected_guy_num =
        i8::try_from(slot).map_err(|_| GuyArrayWalkError::GuyNumberOverflow { slot })?;
    if guy.who != expected_who || guy.o != expected_o || guy.guy_num != expected_guy_num {
        return Err(GuyArrayWalkError::GuyIdentityMismatch {
            slot,
            expected_who,
            actual_who: guy.who,
            expected_o,
            actual_o: guy.o,
            expected_guy_num,
            actual_guy_num: guy.guy_num,
        });
    }
    Ok(())
}

/// Produce the exact checksum byte stream for one `PtrArray<Guy>`.
///
/// Empty arrays hash only a signed zero length; their dormant capacity/flags are not read.
/// Positive arrays hash capacity and increment twice, preserve null slots in the presence
/// pass, and append non-null Guys in slot order.  `flags_after` records the retail walk's
/// persistent clear of bit `0x40`; this pure helper does not mutate its input.
pub fn guy_array_walk_bytes(
    guys: &UnitGuys,
    expected_who: i8,
    expected_o: i16,
) -> Result<(Vec<u8>, GuyArrayWalkValue), GuyArrayWalkError> {
    let length = guys.len();
    let signed_length =
        i32::try_from(length).map_err(|_| GuyArrayWalkError::LengthOverflow { length })?;
    let mut out = Vec::new();
    out.extend_from_slice(&signed_length.to_le_bytes());
    if length == 0 {
        let value = GuyArrayWalkValue {
            checksum: don_sim::checksum::adler32(1, &out),
            bytes_walked: 4,
            guys_walked: 0,
            null_slots: 0,
            flags_after: guys.flags,
        };
        return Ok((out, value));
    }

    if guys.size < 0 {
        return Err(GuyArrayWalkError::NegativeCapacity { size: guys.size });
    }
    if length > guys.size as usize {
        return Err(GuyArrayWalkError::LengthExceedsCapacity {
            length,
            size: guys.size,
        });
    }
    if guys.guy_mark < 0 {
        return Err(GuyArrayWalkError::NegativeGuyMark {
            guy_mark: guys.guy_mark,
        });
    }
    let guy_mark = guys.guy_mark as usize;
    if guy_mark > length {
        return Err(GuyArrayWalkError::GuyMarkExceedsLength { guy_mark, length });
    }
    for slot in 0..guy_mark {
        if guys.guys[slot].is_none() {
            return Err(GuyArrayWalkError::MissingSquadPrefix { slot, guy_mark });
        }
    }
    for (slot, guy) in guys.guys.iter().enumerate() {
        if let Some(guy) = guy {
            validate_guy_identity(slot, guy, expected_who, expected_o)?;
        }
    }

    out.reserve(NONEMPTY_GUY_ARRAY_FIXED_BYTES + length + length * GUY_WALK_LEN);
    out.extend_from_slice(&guys.size.to_le_bytes());
    out.extend_from_slice(&guys.increment.to_le_bytes());
    let flags_after = guys.flags & !0x40;
    out.push(flags_after);
    for slot in &guys.guys {
        out.push(u8::from(slot.is_some()));
    }
    out.extend_from_slice(&guys.size.to_le_bytes());
    out.extend_from_slice(&guys.increment.to_le_bytes());

    let mut guys_walked = 0u32;
    let mut null_slots = 0u32;
    for slot in &guys.guys {
        match slot {
            Some(guy) => {
                out.extend_from_slice(&guy.walk_bytes());
                guys_walked = guys_walked
                    .checked_add(1)
                    .ok_or(GuyArrayWalkError::LengthOverflow { length })?;
            }
            None => {
                null_slots = null_slots
                    .checked_add(1)
                    .ok_or(GuyArrayWalkError::LengthOverflow { length })?;
            }
        }
    }
    let value = GuyArrayWalkValue {
        checksum: don_sim::checksum::adler32(1, &out),
        bytes_walked: out.len() as u64,
        guys_walked,
        null_slots,
        flags_after,
    };
    Ok((out, value))
}

#[derive(Clone)]
struct ReachedUnit {
    owner: usize,
    o: i32,
    row: usize,
    handle: Handle,
}

#[derive(Clone)]
struct PreparedUnit {
    reached: ReachedUnit,
    bytes: Vec<u8>,
    guys_walked: u32,
    null_slots: u32,
    flags_after: u8,
}

/// Execute `CheckSums::check_guys` over World's canonical sparse Unit bands.
///
/// Validation and byte preparation finish for every reached Unit before checksum-visible
/// authority is mutated.  On success the commit clears `PtrArray+0x14 & 0x40` exactly as
/// retail does.  On any error every authority row is unchanged.
pub fn check_world_guys(
    world: &World,
    driver: GuysDriverFacts,
    authority: &mut GuysWalkAuthority,
) -> Result<GuysChannelValue, GuysRuntimeError> {
    if !driver.objects_valid {
        return Ok(GuysChannelValue::empty());
    }
    if world.units.len() != world.live_count() as usize {
        return Err(GuysRuntimeError::UnitLengthMismatch {
            columns: world.units.len(),
            live: world.live_count(),
        });
    }

    let bands = world.object_bands();
    let mut seen: Vec<Option<(usize, i32)>> = vec![None; world.units.len()];
    let mut reached = Vec::new();
    let mut registry_entries = 0u32;
    let mut skipped_inactive_leader = 0u32;
    let mut skipped_inactive_unit = 0u32;
    let mut skipped_outside_walk = 0u32;

    for owner in 0..don_sim::objects::OWNER_SLOTS {
        let mark = bands
            .mark(owner, RetailBand::Unit)
            .ok_or(GuysRuntimeError::InvalidOwnerMark { owner, mark: -1 })?;
        if !(RetailBand::Unit.base()..=RetailBand::Unit.limit()).contains(&mark) {
            return Err(GuysRuntimeError::InvalidOwnerMark { owner, mark });
        }
        let retained = bands
            .retained_slots(owner, RetailBand::Unit)
            .ok_or(GuysRuntimeError::InvalidOwnerMark { owner, mark })?;

        for offset in 0..retained {
            let o = RetailBand::Unit.base() + offset as i32;
            let address = RetailObjectAddress::new(owner as u8, RetailBand::Unit, o);
            let slot = bands
                .slot(address)
                .ok_or(GuysRuntimeError::MissingSparseSlot { owner, o, mark })?;
            // Objects::find_free constructs Unit for playable owners and Animal for the
            // two nature owners; both share UnitData and the parallel Units projection.
            let expected_class = if owner < 8 {
                ObjectStorageClass::Unit
            } else {
                ObjectStorageClass::Animal
            };
            if slot.storage.class != expected_class || slot.storage.unit.is_none() {
                return Err(GuysRuntimeError::WrongUnitStorage { owner, o });
            }
            if o < mark {
                registry_entries = registry_entries
                    .checked_add(1)
                    .ok_or(GuysRuntimeError::CountOverflow)?;
            }

            match slot.lifecycle {
                SparseSlotLifecycle::Tombstone(tombstone) => {
                    if o < mark && tombstone.flags & OBJ_FLAG_ACTIVE != 0 {
                        return Err(GuysRuntimeError::ActiveTombstone {
                            owner,
                            o,
                            flags: tombstone.flags,
                        });
                    }
                }
                SparseSlotLifecycle::Reserved { .. } => {
                    return Err(GuysRuntimeError::ReservedSparseSlot { owner, o });
                }
                SparseSlotLifecycle::Live(identity) => {
                    if o >= mark {
                        return Err(GuysRuntimeError::LiveBeyondMark { owner, o, mark });
                    }
                    let WorldObjectIdentity::Unit { id, generation } = identity else {
                        return Err(GuysRuntimeError::WrongSparseIdentity { owner, o, identity });
                    };
                    let row = world.unit_row_at(owner as i32, o).ok_or(
                        GuysRuntimeError::SparseIdentityUnresolved {
                            owner,
                            o,
                            id,
                            generation,
                        },
                    )?;
                    let handle = Handle { id, generation };
                    let actual = world.handle_at_row(row);
                    if actual != Some(handle) {
                        return Err(GuysRuntimeError::DenseIdentityMismatch {
                            owner,
                            o,
                            row,
                            expected: handle,
                            actual,
                        });
                    }
                    if let Some((first_owner, first_o)) = seen[row] {
                        return Err(GuysRuntimeError::DuplicateDenseRow {
                            row,
                            first_owner,
                            first_o,
                            second_owner: owner,
                            second_o: o,
                        });
                    }
                    seen[row] = Some((owner, o));
                    let unit_owner = world.units.get_who(row);
                    if unit_owner as usize != owner {
                        return Err(GuysRuntimeError::UnitOwnerMismatch {
                            row,
                            sparse_owner: owner,
                            unit_owner,
                        });
                    }
                    let unit_o = world.units.o()[row];
                    if i32::from(unit_o) != o {
                        return Err(GuysRuntimeError::UnitObjectIndexMismatch {
                            row,
                            owner,
                            sparse_o: o,
                            unit_o,
                        });
                    }
                    if world.units.get_flags(row) & OBJ_FLAG_ACTIVE == 0 {
                        skipped_inactive_unit = skipped_inactive_unit
                            .checked_add(1)
                            .ok_or(GuysRuntimeError::CountOverflow)?;
                        continue;
                    }
                    if owner >= GUYS_CHANNEL_OWNER_SLOTS {
                        skipped_outside_walk = skipped_outside_walk
                            .checked_add(1)
                            .ok_or(GuysRuntimeError::CountOverflow)?;
                        continue;
                    }
                    if !driver.leader_active[owner] {
                        skipped_inactive_leader = skipped_inactive_leader
                            .checked_add(1)
                            .ok_or(GuysRuntimeError::CountOverflow)?;
                        continue;
                    }
                    reached.push(ReachedUnit {
                        owner,
                        o,
                        row,
                        handle,
                    });
                }
            }
        }
    }

    for (&(id, generation), _) in &authority.rows {
        if world.row_of(Handle { id, generation }).is_none() {
            return Err(GuysRuntimeError::AuthorityForDeadUnit { id, generation });
        }
    }

    let mut prepared = Vec::with_capacity(reached.len());
    for reached in reached {
        let facts =
            authority
                .get(reached.handle)
                .ok_or(GuysRuntimeError::MissingWalkAuthority {
                    row: reached.row,
                    owner: reached.owner,
                    o: reached.o,
                    handle: reached.handle,
                })?;
        let column_mark = world.units.guy_mark()[reached.row];
        if column_mark != facts.guys.guy_mark {
            return Err(GuysRuntimeError::GuyMarkMismatch {
                row: reached.row,
                columns: column_mark,
                authority: facts.guys.guy_mark,
            });
        }
        let (bytes, value) =
            guy_array_walk_bytes(&facts.guys, reached.owner as i8, reached.o as i16).map_err(
                |source| GuysRuntimeError::GuyArrayWalk {
                    row: reached.row,
                    owner: reached.owner,
                    o: reached.o,
                    source,
                },
            )?;
        prepared.push(PreparedUnit {
            reached,
            bytes,
            guys_walked: value.guys_walked,
            null_slots: value.null_slots,
            flags_after: value.flags_after,
        });
    }

    let mut result = GuysChannelValue {
        registry_entries,
        skipped_inactive_leader,
        skipped_inactive_unit,
        skipped_outside_walk,
        ..GuysChannelValue::empty()
    };
    for unit in &prepared {
        result.checksum = don_sim::checksum::adler32(result.checksum, &unit.bytes);
        result.bytes_walked = result
            .bytes_walked
            .checked_add(unit.bytes.len() as u64)
            .ok_or(GuysRuntimeError::CountOverflow)?;
        result.units_walked = result
            .units_walked
            .checked_add(1)
            .ok_or(GuysRuntimeError::CountOverflow)?;
        result.guys_walked = result
            .guys_walked
            .checked_add(unit.guys_walked)
            .ok_or(GuysRuntimeError::CountOverflow)?;
        result.null_slots = result
            .null_slots
            .checked_add(unit.null_slots)
            .ok_or(GuysRuntimeError::CountOverflow)?;
    }

    // All fallible work is complete. Commit the one persistent mutation performed by
    // PtrArray<Guy>::walk_data.
    for unit in &prepared {
        let facts = authority
            .get_mut(unit.reached.handle)
            .expect("prepared authority remains installed until commit");
        if facts.guys.flags != unit.flags_after {
            facts.guys.flags = unit.flags_after;
            result.arrays_masked += 1;
        }
    }
    Ok(result)
}
