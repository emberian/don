//! Canonical Build identity transaction over `don_sim::tick::Sim`.
//!
//! The shipped allocation path is `Objects::init_build` -> `Objects::find_free` ->
//! `Build::init` -> `Wall::init` -> `Object::init` -> `SubObject::init`.  The final leaf
//! binds the owner, allocated object id, ptype and XOR-encoded position.  The current
//! `Sim::spawn_build` already commits the dense and sparse registry append, but intentionally
//! accepts an otherwise opaque `BuildData`; it does not own the ptype or position inputs.
//!
//! [`spawn_canonical_build`] is the narrow replay/setup join.  It proves the pre-existing
//! Build owner is dense-equivalent, stamps only those independently supplied identity facts,
//! delegates the registry append to `Sim::spawn_build`, registers the current ptype, and
//! returns a receipt containing both registry views and the resulting body fields.  It does
//! not claim to execute the remainder of the 1,544-byte `Build::init` body.

use std::fmt;

use don_sim::objects::{Band, BANDED_SLOTS, BUILD_BAND_BASE, OWNER_SLOTS, WALL_BAND_BASE};
use don_sim::systems::production::{self, BuildData};
use don_sim::systems::sparse_object_bands_authority_frontier::{RetailBand, RetailObjectAddress};
use don_sim::tick::Sim;
use don_sim::world::WorldObjectIdentity;

pub const OBJECTS_INIT_BUILD_VA: u32 = 0x0065_d190;
pub const OBJECTS_FIND_FREE_VA: u32 = 0x0065_ad60;
pub const BUILD_INIT_VA: u32 = 0x0062_9740;
pub const WALL_INIT_VA: u32 = 0x0063_e9b0;
pub const OBJECT_INIT_VA: u32 = 0x0064_7750;
pub const SUBOBJECT_INIT_VA: u32 = 0x0066_2300;

/// XOR applied by `SubObject::init` at `0x00662300` to X and Y independently.
pub const SUBOBJECT_COORD_XOR: i32 = 0x0006_3637;

/// Inputs already selected by the caller which owns placement and Build initialization.
///
/// There is deliberately no asserted object id. Retail obtains it from
/// `Objects::find_free`; the canonical dense phase-1 owner obtains the same next id from
/// the Build band and reports it in [`CanonicalBuildSpawnReceipt`].
#[derive(Clone, Debug)]
pub struct CanonicalBuildSpawnRequest {
    pub owner: u8,
    pub type_index: i32,
    pub snapped_x: i32,
    pub snapped_y: i32,
    /// Complete staged Build body apart from the identity fields owned by this transaction.
    /// `flags & 1` must already be set and `city` must be retail's unlinked `-1` sentinel.
    pub build: BuildData,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanonicalBuildSpawnReceipt {
    pub row: usize,
    pub owner: u8,
    pub type_index: i32,
    pub object_id: i16,
    pub snapped_position: (i32, i32),
    pub encoded_position: (i32, i32),
    pub build_mark_before: u32,
    pub build_mark_after: u32,
    pub owner_active_before: bool,
    pub owner_active_after: bool,
    pub dense_registry_row: u32,
    pub sparse_identity: WorldObjectIdentity,
    pub body_owner: u8,
    pub body_object_id: i16,
    pub body_position: (i32, i32),
    pub registered_ptype: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CanonicalBuildSpawnError {
    OwnerOutOfRange {
        owner: u8,
    },
    TypeIndexOutOfRange {
        type_index: i32,
        type_rows: usize,
    },
    InvalidBuildFlags {
        flags: u8,
    },
    IncomingCityAlreadyLinked {
        city: i16,
    },
    RegistryNotDenseEquivalent,
    UnsupportedBuildOwner {
        owner: usize,
        entries: usize,
    },
    RegistryRowOutOfRange {
        owner: usize,
        object_id: u32,
        row: usize,
        build_rows: usize,
    },
    DuplicateRegistryRow {
        row: usize,
        first_owner: usize,
        first_object_id: u32,
        second_owner: usize,
        second_object_id: u32,
    },
    ExistingBuildOwnerMismatch {
        row: usize,
        object_id: u32,
        registry_owner: usize,
        build_owner: u8,
    },
    ExistingBuildObjectIdMismatch {
        row: usize,
        owner: usize,
        registry_object_id: u32,
        build_object_id: i16,
    },
    UnregisteredBuildRow {
        row: usize,
    },
    BuildBandFull {
        owner: usize,
        build_mark: u32,
    },
    PtypeRowAlreadyOwned {
        row: usize,
        type_index: i32,
    },
}

impl fmt::Display for CanonicalBuildSpawnError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OwnerOutOfRange { owner } => write!(
                f,
                "Build owner {owner} is outside retail's playable 0..{BANDED_SLOTS} range"
            ),
            Self::TypeIndexOutOfRange {
                type_index,
                type_rows,
            } => write!(
                f,
                "Build ptype {type_index} is outside the installed {type_rows}-row type table"
            ),
            Self::InvalidBuildFlags { flags } => write!(
                f,
                "staged Build flags {flags:#04x} do not contain SubObjectData::VALID"
            ),
            Self::IncomingCityAlreadyLinked { city } => write!(
                f,
                "staged Build already links City {city}; the atomic City transaction must own that link"
            ),
            Self::RegistryNotDenseEquivalent => write!(
                f,
                "legacy and sparse object registries are not in the supported dense-equivalent phase"
            ),
            Self::UnsupportedBuildOwner { owner, entries } => write!(
                f,
                "owner {owner} has {entries} Build entries, but retail Build bands exist only for owners 0..{BANDED_SLOTS}"
            ),
            Self::RegistryRowOutOfRange {
                owner,
                object_id,
                row,
                build_rows,
            } => write!(
                f,
                "Build registry ({owner},{object_id}) points to row {row}, but Sim owns {build_rows} rows"
            ),
            Self::DuplicateRegistryRow {
                row,
                first_owner,
                first_object_id,
                second_owner,
                second_object_id,
            } => write!(
                f,
                "Build row {row} is registered at both ({first_owner},{first_object_id}) and ({second_owner},{second_object_id})"
            ),
            Self::ExistingBuildOwnerMismatch {
                row,
                object_id,
                registry_owner,
                build_owner,
            } => write!(
                f,
                "existing Build row {row} at ({registry_owner},{object_id}) carries owner {build_owner}"
            ),
            Self::ExistingBuildObjectIdMismatch {
                row,
                owner,
                registry_object_id,
                build_object_id,
            } => write!(
                f,
                "existing Build row {row} at ({owner},{registry_object_id}) carries object id {build_object_id}"
            ),
            Self::UnregisteredBuildRow { row } => {
                write!(f, "existing Build row {row} has no canonical registry address")
            }
            Self::BuildBandFull { owner, build_mark } => write!(
                f,
                "owner {owner}'s Build mark {build_mark} has reached the Wall band"
            ),
            Self::PtypeRowAlreadyOwned { row, type_index } => write!(
                f,
                "future Build row {row} already carries ptype {type_index}"
            ),
        }
    }
}

impl std::error::Error for CanonicalBuildSpawnError {}

/// Atomically append one identity-complete Build after all fallible checks have passed.
///
/// No `Err` path mutates `sim`. After preflight, `Sim::spawn_build` and
/// `LiveProductionRuntime::register_build` are infallible dense appends. The receipt is read
/// back from both registry owners and the committed body rather than echoing the request.
pub fn spawn_canonical_build(
    sim: &mut Sim,
    request: CanonicalBuildSpawnRequest,
) -> Result<CanonicalBuildSpawnReceipt, CanonicalBuildSpawnError> {
    let owner = request.owner as usize;
    if owner >= BANDED_SLOTS {
        return Err(CanonicalBuildSpawnError::OwnerOutOfRange {
            owner: request.owner,
        });
    }
    let type_row = usize::try_from(request.type_index).ok();
    if type_row.is_none_or(|row| row >= sim.production_runtime.types.len()) {
        return Err(CanonicalBuildSpawnError::TypeIndexOutOfRange {
            type_index: request.type_index,
            type_rows: sim.production_runtime.types.len(),
        });
    }
    if !request.build.is_valid() {
        return Err(CanonicalBuildSpawnError::InvalidBuildFlags {
            flags: request.build.flags,
        });
    }
    if request.build.city != -1 {
        return Err(CanonicalBuildSpawnError::IncomingCityAlreadyLinked {
            city: request.build.city,
        });
    }

    validate_existing_build_owner(sim)?;

    let row = sim.builds.len();
    if let Some(Some(type_index)) = sim.production_runtime.build_types.get(row) {
        return Err(CanonicalBuildSpawnError::PtypeRowAlreadyOwned {
            row,
            type_index: *type_index,
        });
    }

    let build_mark_before = sim.world.objects.slot(owner).mark(Band::Build);
    if build_mark_before >= WALL_BAND_BASE {
        return Err(CanonicalBuildSpawnError::BuildBandFull {
            owner,
            build_mark: build_mark_before,
        });
    }
    let object_id = build_mark_before as i16;
    let encoded_x = request.snapped_x ^ SUBOBJECT_COORD_XOR;
    let encoded_y = request.snapped_y ^ SUBOBJECT_COORD_XOR;
    let owner_active_before = sim.world.objects.is_active(owner);

    let mut build = request.build;
    build.who = request.owner;
    build.other[production::off::OBJECT_ID..production::off::OBJECT_ID + 2]
        .copy_from_slice(&object_id.to_le_bytes());
    build.other[production::off::X_INTERNAL..production::off::X_INTERNAL + 4]
        .copy_from_slice(&encoded_x.to_le_bytes());
    build.other[production::off::Y_INTERNAL..production::off::Y_INTERNAL + 4]
        .copy_from_slice(&encoded_y.to_le_bytes());

    let committed_row = sim.spawn_build(owner, build);
    assert_eq!(
        committed_row, row,
        "preflighted dense Build append changed row"
    );
    sim.production_runtime
        .register_build(row, request.type_index);

    let slot = (build_mark_before - BUILD_BAND_BASE) as usize;
    let dense_registry_row = sim.world.objects.slot(owner).band(Band::Build)[slot];
    let address = RetailObjectAddress::new(request.owner, RetailBand::Build, object_id as i32);
    let sparse_identity = sim
        .world
        .object_bands()
        .live_identity(address)
        .expect("Sim::spawn_build must mirror the preflighted dense append");
    let committed = &sim.builds[row];
    let committed_encoded_x = i32::from_le_bytes(
        committed.other[production::off::X_INTERNAL..production::off::X_INTERNAL + 4]
            .try_into()
            .expect("fixed BuildData encoded-X window"),
    );
    let committed_encoded_y = i32::from_le_bytes(
        committed.other[production::off::Y_INTERNAL..production::off::Y_INTERNAL + 4]
            .try_into()
            .expect("fixed BuildData encoded-Y window"),
    );
    let registered_ptype = sim.production_runtime.build_types[row]
        .expect("canonical Build spawn registered current ptype");

    let receipt = CanonicalBuildSpawnReceipt {
        row,
        owner: request.owner,
        type_index: request.type_index,
        object_id,
        snapped_position: (request.snapped_x, request.snapped_y),
        encoded_position: (committed_encoded_x, committed_encoded_y),
        build_mark_before,
        build_mark_after: sim.world.objects.slot(owner).mark(Band::Build),
        owner_active_before,
        owner_active_after: sim.world.objects.is_active(owner),
        dense_registry_row,
        sparse_identity,
        body_owner: committed.who,
        body_object_id: committed.object_id(),
        body_position: committed.position(),
        registered_ptype,
    };

    assert_eq!(receipt.dense_registry_row, row as u32);
    assert_eq!(
        receipt.sparse_identity,
        WorldObjectIdentity::BuildRow(row as u32)
    );
    assert_eq!(receipt.body_owner, request.owner);
    assert_eq!(receipt.body_object_id, object_id);
    assert_eq!(receipt.encoded_position, (encoded_x, encoded_y));
    assert_eq!(receipt.body_position, receipt.snapped_position);
    assert_eq!(receipt.registered_ptype, request.type_index);
    assert_eq!(receipt.build_mark_after, receipt.build_mark_before + 1);
    assert!(receipt.owner_active_after);
    assert!(sim.world.object_bands_are_dense_equivalent());

    Ok(receipt)
}

fn validate_existing_build_owner(sim: &Sim) -> Result<(), CanonicalBuildSpawnError> {
    if !sim.world.object_bands_are_dense_equivalent() {
        return Err(CanonicalBuildSpawnError::RegistryNotDenseEquivalent);
    }
    for owner in BANDED_SLOTS..OWNER_SLOTS {
        let entries = sim.world.objects.slot(owner).band(Band::Build).len();
        if entries != 0 {
            return Err(CanonicalBuildSpawnError::UnsupportedBuildOwner { owner, entries });
        }
    }

    let mut seen: Vec<Option<(usize, u32)>> = vec![None; sim.builds.len()];
    for owner in 0..BANDED_SLOTS {
        for (slot, &row_u32) in sim
            .world
            .objects
            .slot(owner)
            .band(Band::Build)
            .iter()
            .enumerate()
        {
            let object_id = BUILD_BAND_BASE + slot as u32;
            let row = row_u32 as usize;
            let Some(build) = sim.builds.get(row) else {
                return Err(CanonicalBuildSpawnError::RegistryRowOutOfRange {
                    owner,
                    object_id,
                    row,
                    build_rows: sim.builds.len(),
                });
            };
            if let Some((first_owner, first_object_id)) = seen[row] {
                return Err(CanonicalBuildSpawnError::DuplicateRegistryRow {
                    row,
                    first_owner,
                    first_object_id,
                    second_owner: owner,
                    second_object_id: object_id,
                });
            }
            seen[row] = Some((owner, object_id));
            if build.who as usize != owner {
                return Err(CanonicalBuildSpawnError::ExistingBuildOwnerMismatch {
                    row,
                    object_id,
                    registry_owner: owner,
                    build_owner: build.who,
                });
            }
            if i32::from(build.object_id()) != object_id as i32 {
                return Err(CanonicalBuildSpawnError::ExistingBuildObjectIdMismatch {
                    row,
                    owner,
                    registry_object_id: object_id,
                    build_object_id: build.object_id(),
                });
            }
        }
    }
    if let Some(row) = seen.iter().position(Option::is_none) {
        return Err(CanonicalBuildSpawnError::UnregisteredBuildRow { row });
    }
    Ok(())
}
