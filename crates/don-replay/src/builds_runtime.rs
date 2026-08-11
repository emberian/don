//! Exact, fail-closed adapter from the canonical simulation build owner to the retail
//! Builds checksum walk.
//!
//! `don_sim::tick::Sim` already owns both sides of `CheckSums::check_builds`' outer loop:
//! `Sim::builds` stores the `BuildData` rows and `World::objects` stores each leader's
//! dense band-2000 traversal.  This module joins them without flattening owner order.
//!
//! `BuildData` intentionally does not own every pointer target reached by the inherited
//! `SubObject`/`Object` walk.  In particular, the current build type, the optional
//! `ObjectData::launching` `SimpleArray<int>`, and the full engine-shaped mining array
//! live outside its 220-byte image.  [`BuildsWalkAuthority`] makes those omissions an
//! explicit input.  A live build without a complete row of authority is refused; a null
//! pointer must be asserted as [`BuildWalkFacts::launching`] = `None`, not inferred from
//! absence.
//!
//! This is a producer prerequisite, not a claim that replay setup already creates the
//! initial city/build state present in retail recordings.

use std::fmt;

use don_sim::container::EngineArray;
use don_sim::objects::{Band, BANDED_SLOTS, BUILD_BAND_BASE, OWNER_SLOTS};
use don_sim::systems::gathering::GatherMiningList;
use don_sim::systems::production::{self, BuildData, BuildQueueEntry};
use don_sim::tick::Sim;

/// `CheckSums::check_builds(CheckSum*, int)` (checksums.cpp:646).
pub const CHECK_BUILDS_VA: u32 = 0x0093_7290;
/// `BuildData::walk_data(DataWalk*)` (build.cpp:35).
pub const BUILD_DATA_WALK_VA: u32 = 0x0062_f270;
/// `WallData::walk_data(DataWalk*)`.
pub const WALL_DATA_WALK_VA: u32 = 0x0064_2510;
/// `Object::walk_data(DataWalk*)`.
pub const OBJECT_WALK_VA: u32 = 0x0064_7830;
/// `Object::must_walk(DataWalk*)`.
pub const OBJECT_MUST_WALK_VA: u32 = 0x0064_7930;
/// `SubObject::walk_data(DataWalk*)`.
pub const SUBOBJECT_WALK_VA: u32 = 0x0066_21d0;
/// `BuildQueue::walk_data(DataWalk*)`.
pub const BUILD_QUEUE_WALK_VA: u32 = 0x0063_05f0;
/// `Array<TCoordData>::walk_data(DataWalk*)`.
pub const MINING_ARRAY_WALK_VA: u32 = 0x0047_1c30;
/// `PtrLinkListAbstract<GatherPoint>::walk_data(DataWalk*)`.
pub const GATHER_LIST_WALK_VA: u32 = 0x0047_08a0;

/// Bytes walked by one valid Build with null `launching`, empty build/mining/gather
/// containers, and `Object::must_walk == true`.
///
/// This includes all four emitted `must_walk` result bytes: one each in `SubObject`,
/// `Object`, `WallData`, and `BuildData`.
pub const EMPTY_LIVE_BUILD_WALK_BYTES: u64 = 131;

/// Pointer/container state reached by one Build walk but not owned by `BuildData`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildWalkFacts {
    /// `ObjectData::launching` at `BuildData + 0x44`.
    ///
    /// `None` is an authoritative null pointer. `Some(empty)` is a present array whose
    /// four-byte zero length still reaches the checksum.
    pub launching: Option<EngineArray<i32>>,
    /// `BuildData::gather_from` at `+0x98`, including the engine array's capacity,
    /// increment and flags.  A plain `Vec` cannot reproduce this walk.
    pub mining: GatherMiningList,
}

impl Default for BuildWalkFacts {
    fn default() -> Self {
        Self {
            launching: None,
            mining: GatherMiningList::default(),
        }
    }
}

/// Explicit per-`Sim::builds`-row authority.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BuildsWalkAuthority {
    rows: Vec<Option<BuildWalkFacts>>,
}

impl BuildsWalkAuthority {
    /// Install or replace authority for one `Sim::builds` row.
    pub fn install(&mut self, row: usize, facts: BuildWalkFacts) {
        if self.rows.len() <= row {
            self.rows.resize_with(row + 1, || None);
        }
        self.rows[row] = Some(facts);
    }

    /// Remove authority for one row.  A later live walk of that row will fail closed.
    pub fn remove(&mut self, row: usize) -> Option<BuildWalkFacts> {
        self.rows.get_mut(row).and_then(Option::take)
    }

    /// Borrow the installed row, if any.
    pub fn get(&self, row: usize) -> Option<&BuildWalkFacts> {
        self.rows.get(row).and_then(Option::as_ref)
    }

    fn first_fact_beyond(&self, rows: usize) -> Option<usize> {
        self.rows
            .iter()
            .enumerate()
            .skip(rows)
            .find_map(|(row, facts)| facts.as_ref().map(|_| row))
    }
}

/// Result of walking one authorized Build record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildWalkValue {
    pub checksum: u32,
    pub bytes_walked: u64,
}

/// Isolated Builds-channel value produced from a canonical `Sim` owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildsChannelValue {
    pub checksum: u32,
    pub bytes_walked: u64,
    /// Valid Build rows which reached `BuildData::walk_data`.
    pub builds_walked: u32,
    /// Registry entries whose row/owner/object identity was validated, including invalid
    /// rows skipped by the retail `flags & 1` gate.
    pub registry_entries: u32,
}

/// Refusal while constructing one Build's exact byte stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BuildWalkError {
    /// `production::BuildData` still carries a legacy packed mining vector which cannot
    /// be proven identical to the authoritative 8-byte `TCoordData` array.  Empty is the
    /// only non-conflicting state until the Sim owner migrates to `GatherMiningList`.
    LegacyMiningPayloadPresent { length: usize },
    /// The authoritative mining sidecar disagrees with the two tail bytes retained by
    /// `production::BuildData`.
    MiningTailMismatch {
        build_mtn: i8,
        build_cliff: i8,
        authority_mtn: i8,
        authority_cliff: i8,
    },
    ContainerLengthOverflow {
        container: &'static str,
        length: usize,
    },
}

impl fmt::Display for BuildWalkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BuildWalkError::LegacyMiningPayloadPresent { length } => write!(
                f,
                "BuildData carries {length} legacy packed mining entries alongside the authoritative TCoordData array"
            ),
            BuildWalkError::MiningTailMismatch {
                build_mtn,
                build_cliff,
                authority_mtn,
                authority_cliff,
            } => write!(
                f,
                "BuildData mining tail ({build_mtn},{build_cliff}) disagrees with authority ({authority_mtn},{authority_cliff})"
            ),
            BuildWalkError::ContainerLengthOverflow { container, length } => write!(
                f,
                "Build {container} length {length} does not fit retail's signed 32-bit field"
            ),
        }
    }
}

impl std::error::Error for BuildWalkError {}

/// Fail-closed owner/adapter errors.  None of these is repaired by synthesizing bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BuildsRuntimeError {
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
    UnregisteredBuildRow {
        row: usize,
    },
    BuildOwnerMismatch {
        row: usize,
        object_id: u32,
        registry_owner: usize,
        build_owner: u8,
    },
    BuildObjectIdMismatch {
        row: usize,
        owner: usize,
        registry_object_id: u32,
        build_object_id: i16,
    },
    AuthorityRowOutOfRange {
        row: usize,
        build_rows: usize,
    },
    MissingWalkAuthority {
        row: usize,
        owner: usize,
        object_id: u32,
    },
    MissingBuildType {
        row: usize,
        owner: usize,
        object_id: u32,
    },
    BuildWalk {
        row: usize,
        owner: usize,
        object_id: u32,
        source: BuildWalkError,
    },
    CountOverflow,
}

impl fmt::Display for BuildsRuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedBuildOwner { owner, entries } => write!(
                f,
                "owner slot {owner} has {entries} Build entries, but retail walks Build bands only for slots 0..{BANDED_SLOTS}"
            ),
            Self::RegistryRowOutOfRange {
                owner,
                object_id,
                row,
                build_rows,
            } => write!(
                f,
                "Build registry ({owner},{object_id}) points at row {row}, but Sim owns {build_rows} rows"
            ),
            Self::DuplicateRegistryRow {
                row,
                first_owner,
                first_object_id,
                second_owner,
                second_object_id,
            } => write!(
                f,
                "Build row {row} is registered twice: ({first_owner},{first_object_id}) and ({second_owner},{second_object_id})"
            ),
            Self::UnregisteredBuildRow { row } => {
                write!(f, "Sim Build row {row} has no band-2000 registry owner")
            }
            Self::BuildOwnerMismatch {
                row,
                object_id,
                registry_owner,
                build_owner,
            } => write!(
                f,
                "Build row {row} at ({registry_owner},{object_id}) carries owner {build_owner}"
            ),
            Self::BuildObjectIdMismatch {
                row,
                owner,
                registry_object_id,
                build_object_id,
            } => write!(
                f,
                "Build row {row} at ({owner},{registry_object_id}) carries object id {build_object_id}"
            ),
            Self::AuthorityRowOutOfRange { row, build_rows } => write!(
                f,
                "Build walk authority exists for row {row}, but Sim owns {build_rows} rows"
            ),
            Self::MissingWalkAuthority {
                row,
                owner,
                object_id,
            } => write!(
                f,
                "live Build row {row} at ({owner},{object_id}) lacks launching/mining authority"
            ),
            Self::MissingBuildType {
                row,
                owner,
                object_id,
            } => write!(
                f,
                "live Build row {row} at ({owner},{object_id}) lacks current ptype authority"
            ),
            Self::BuildWalk {
                row,
                owner,
                object_id,
                source,
            } => write!(
                f,
                "live Build row {row} at ({owner},{object_id}) cannot be walked: {source}"
            ),
            Self::CountOverflow => write!(f, "Build channel count exceeds u32"),
        }
    }
}

impl std::error::Error for BuildsRuntimeError {}

/// Produce the exact byte stream handed to `CheckSum::walk_function` by one valid Build.
///
/// The caller supplies `ptype_index` because `SubObject::walk_data` hashes
/// `build->ptype->index`, not the pointer stored in the 220-byte record.  This function is
/// intentionally independent of registry iteration so the inherited call order can be
/// tested byte-for-byte.
pub fn build_walk_bytes(
    build: &BuildData,
    ptype_index: i32,
    facts: &BuildWalkFacts,
) -> Result<Vec<u8>, BuildWalkError> {
    if !build.gather_from.tiles.is_empty() {
        return Err(BuildWalkError::LegacyMiningPayloadPresent {
            length: build.gather_from.tiles.len(),
        });
    }
    if build.gather_from.mtn != facts.mining.mtn || build.gather_from.cliff != facts.mining.cliff {
        return Err(BuildWalkError::MiningTailMismatch {
            build_mtn: build.gather_from.mtn,
            build_cliff: build.gather_from.cliff,
            authority_mtn: facts.mining.mtn,
            authority_cliff: facts.mining.cliff,
        });
    }

    let image = build.image();
    let must_walk = build.is_valid();
    let mut out = Vec::with_capacity(EMPTY_LIVE_BUILD_WALK_BYTES as usize);

    // BuildData::walk_data 0x0062f270 begins with two derived-record bytes.
    out.extend_from_slice(&image[production::off::FOUNDER..production::off::FOUNDER + 1]);
    out.extend_from_slice(&image[production::off::MAX_AGE..production::off::MAX_AGE + 1]);

    // WallData::walk_data -> Object::walk_data -> SubObject::walk_data.
    out.push(image[0x08]);
    out.push(must_walk as u8);
    if must_walk {
        out.extend_from_slice(&image[0x09..0x18]);
        out.extend_from_slice(&ptype_index.to_le_bytes());
    }

    out.push(must_walk as u8);
    if must_walk {
        out.extend_from_slice(&image[0x20..0x42]);
    }
    out.push(facts.launching.is_some() as u8);
    if let Some(launching) = &facts.launching {
        append_simple_array_i32(&mut out, launching);
    }

    out.push(must_walk as u8);
    if must_walk {
        out.extend_from_slice(&image[0x48..0x66]);
    }

    // BuildData's own Object::must_walk call emits a fourth result byte.
    out.push(must_walk as u8);
    if !must_walk {
        return Ok(out);
    }

    out.extend_from_slice(&image[0x70..0x86]);

    // BuildQueue::walk_data 0x006305f0.
    let queue_len = i32::try_from(build.queue.entries.len()).map_err(|_| {
        BuildWalkError::ContainerLengthOverflow {
            container: "queue",
            length: build.queue.entries.len(),
        }
    })?;
    out.extend_from_slice(&queue_len.to_le_bytes());
    for entry in &build.queue.entries {
        out.extend_from_slice(&entry.image()[..BuildQueueEntry::WALKED_BYTES]);
    }

    // MiningList tail, then Array<TCoordData>::walk_data 0x00471c30.  `walked_image`
    // retains length/capacity/increment/flags and emits every coordinate as two i32s.
    out.extend_from_slice(&facts.mining.walked_image());

    // PtrLinkListAbstract<GatherPoint>::walk_data 0x004708a0.
    let gather_len =
        i32::try_from(build.gather.len()).map_err(|_| BuildWalkError::ContainerLengthOverflow {
            container: "gather list",
            length: build.gather.len(),
        })?;
    out.extend_from_slice(&gather_len.to_le_bytes());
    for point in &build.gather {
        // The list walker emits a null four-byte class/factory token before every node.
        out.extend_from_slice(&0u32.to_le_bytes());
        out.push(point.node_tag);
        out.extend_from_slice(&point.x.to_le_bytes());
        out.extend_from_slice(&point.y.to_le_bytes());
        out.push(point.action);
    }

    out.extend_from_slice(&image[production::off::ORIG_TYPE..production::off::ORIG_TYPE + 4]);
    Ok(out)
}

/// Isolated checksum for one authorized Build record.
pub fn build_walk_value(
    build: &BuildData,
    ptype_index: i32,
    facts: &BuildWalkFacts,
) -> Result<BuildWalkValue, BuildWalkError> {
    let bytes = build_walk_bytes(build, ptype_index, facts)?;
    Ok(BuildWalkValue {
        checksum: don_sim::checksum::adler32(1, &bytes),
        bytes_walked: bytes.len() as u64,
    })
}

/// Execute `CheckSums::check_builds` over `Sim`'s canonical owner and explicit dynamic
/// authority.
///
/// The returned checksum is isolated (seed 1 at the start of Builds).  `check_all` carries
/// a fresh `CheckSum` per channel and later sums the fifteen channel values; recorded bytes
/// are never accepted as producer input here.
pub fn check_sim_builds(
    sim: &Sim,
    authority: &BuildsWalkAuthority,
) -> Result<BuildsChannelValue, BuildsRuntimeError> {
    if let Some(row) = authority.first_fact_beyond(sim.builds.len()) {
        return Err(BuildsRuntimeError::AuthorityRowOutOfRange {
            row,
            build_rows: sim.builds.len(),
        });
    }

    // Build/Wall bands do not exist for nature/unowned slots 8 and 9 in the shipped loop.
    for owner in BANDED_SLOTS..OWNER_SLOTS {
        let entries = sim.world.objects.slot(owner).band(Band::Build).len();
        if entries != 0 {
            return Err(BuildsRuntimeError::UnsupportedBuildOwner { owner, entries });
        }
    }

    // Validate the ownership join for all rows before hashing any bytes.  This keeps a
    // malformed registry from yielding an attractive but meaningless checksum.
    let mut seen: Vec<Option<(usize, u32)>> = vec![None; sim.builds.len()];
    let mut registry_entries = 0u32;
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
                return Err(BuildsRuntimeError::RegistryRowOutOfRange {
                    owner,
                    object_id,
                    row,
                    build_rows: sim.builds.len(),
                });
            };
            if let Some((first_owner, first_object_id)) = seen[row] {
                return Err(BuildsRuntimeError::DuplicateRegistryRow {
                    row,
                    first_owner,
                    first_object_id,
                    second_owner: owner,
                    second_object_id: object_id,
                });
            }
            seen[row] = Some((owner, object_id));

            if build.who as usize != owner {
                return Err(BuildsRuntimeError::BuildOwnerMismatch {
                    row,
                    object_id,
                    registry_owner: owner,
                    build_owner: build.who,
                });
            }
            if i32::from(build.object_id()) != object_id as i32 {
                return Err(BuildsRuntimeError::BuildObjectIdMismatch {
                    row,
                    owner,
                    registry_object_id: object_id,
                    build_object_id: build.object_id(),
                });
            }
            registry_entries = registry_entries
                .checked_add(1)
                .ok_or(BuildsRuntimeError::CountOverflow)?;
        }
    }
    if let Some(row) = seen.iter().position(Option::is_none) {
        return Err(BuildsRuntimeError::UnregisteredBuildRow { row });
    }

    let mut checksum = 1u32;
    let mut bytes_walked = 0u64;
    let mut builds_walked = 0u32;

    // Exact fixed outer-owner order and dense band order from 0x00937290.
    for owner in 0..BANDED_SLOTS {
        if !sim.world.objects.is_active(owner) {
            continue;
        }
        for (slot, &row_u32) in sim
            .world
            .objects
            .slot(owner)
            .band(Band::Build)
            .iter()
            .enumerate()
        {
            let row = row_u32 as usize;
            let build = &sim.builds[row];
            if !build.is_valid() {
                continue;
            }
            let object_id = BUILD_BAND_BASE + slot as u32;
            let facts = authority
                .get(row)
                .ok_or(BuildsRuntimeError::MissingWalkAuthority {
                    row,
                    owner,
                    object_id,
                })?;
            let ptype_index = sim
                .production_runtime
                .build_types
                .get(row)
                .copied()
                .flatten()
                .ok_or(BuildsRuntimeError::MissingBuildType {
                    row,
                    owner,
                    object_id,
                })?;
            let bytes = build_walk_bytes(build, ptype_index, facts).map_err(|source| {
                BuildsRuntimeError::BuildWalk {
                    row,
                    owner,
                    object_id,
                    source,
                }
            })?;
            checksum = don_sim::checksum::adler32(checksum, &bytes);
            bytes_walked = bytes_walked
                .checked_add(bytes.len() as u64)
                .ok_or(BuildsRuntimeError::CountOverflow)?;
            builds_walked = builds_walked
                .checked_add(1)
                .ok_or(BuildsRuntimeError::CountOverflow)?;
        }
    }

    Ok(BuildsChannelValue {
        checksum,
        bytes_walked,
        builds_walked,
        registry_entries,
    })
}

fn append_simple_array_i32(out: &mut Vec<u8>, array: &EngineArray<i32>) {
    let (length, size, increment, flags) = array.checksum_header();
    out.extend_from_slice(&length.to_le_bytes());
    if length == 0 {
        return;
    }
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&increment.to_le_bytes());
    out.push(flags & !0x40);
    for &element in array.as_slice() {
        out.extend_from_slice(&element.to_le_bytes());
    }
}
