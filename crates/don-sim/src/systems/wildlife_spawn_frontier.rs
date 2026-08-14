// SPDX-License-Identifier: GPL-3.0-or-later
//! Detached exact zero-spawn prefix of `Objects::process_all`'s wildlife block.
//!
//! Retail reaches this block at `0x0065DEC9..0x0065E06B` whenever the pre-increment game frame
//! is divisible by 32. The bounded transaction below owns the quota/count loop, every conditional
//! axis draw, and the WData land-bit probe. It commits the cloned RNG only when every attempted
//! cell rejects. A viable cell stops at the first unowned child,
//! `Objects::init_unit(9, WILDBIRD, ...)` `0x0065E0C0`, without publishing any draw.

use crate::rng::Random;
use crate::systems::map_terrain::{World as TerrainWorld, WorldChecksum};
use crate::systems::sparse_object_bands_authority_frontier::{
    RetailBand, RetailObjectAddress, SparseSlotLifecycle,
};
use crate::world::{Handle, World, WorldObjectIdentity, OBJ_FLAG_ACTIVE};

pub const OBJECTS_PROCESS_ALL_VA: u32 = 0x0065_dce0;
pub const WILDLIFE_BLOCK_FIRST_VA: u32 = 0x0065_dec9;
pub const WILDLIFE_FIRST_X_RANDOM_VA: u32 = 0x0065_dfad;
pub const WILDLIFE_FIRST_Y_RANDOM_VA: u32 = 0x0065_dfd9;
pub const OBJECTS_INIT_UNIT_VA: u32 = 0x0065_e0c0;
pub const UNIT_ADD_AIR_PATROL_ORDER_VA: u32 = 0x005e_4350;
pub const RANDOM_GET_VA: u32 = 0x00a3_9d70;

/// The first invocation reached by the supported golden replay. Later cadence hits are outside
/// this transaction until each has its own canonical capture.
pub const WILDLIFE_FRAME: i32 = 32;
pub const WILDLIFE_PERIOD: i32 = 32;
pub const WILDLIFE_OWNER: u8 = 9;
pub const WILDBIRD_TYPE: i32 = 0x192;
pub const WILDLIFE_MAX: i32 = 10;
pub const WILDLIFE_AREA_DIVISOR: i32 = 100;
pub const WILDLIFE_CELL_FLAG: u16 = 0x20;
pub const RANDOM_LOW: i32 = 0;
pub const RANDOM_HIGH: i32 = 0xffff;
pub const WORLD_CELL_COORD: i32 = 0x300;
pub const WORLD_CELL_CENTER: i32 = 0x180;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WildlifeOwner9UnitFact {
    pub handle: Handle,
    pub o: i16,
    pub uid: u16,
    pub flags: u8,
    pub type_index: i32,
    /// Exact virtual `SubObjectData::is_animal()` answer at vtable `+0x30`.
    pub is_animal: bool,
}

/// External capture authority required by this detached transaction.
///
/// The replay does not serialize any of these dynamic frame-32 owners. A retail oracle capture
/// must bind the complete map checksum, canonical object World, RNG, and every live owner-9 Unit
/// fact together; a caller cannot promote replay metadata or a recorded checksum word into this
/// authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WildlifeFrameAuthority {
    pub revision: u64,
    pub composition_digest: [u8; 32],
    pub frame: i32,
    pub random_state: i32,
    pub map_checksum: WorldChecksum,
    /// Complete byte stream passed to the retail World checksum visitor. This is the exact
    /// stale-map guard; the section Adler ledger alone is deliberately not treated as identity.
    pub map_checksum_image: Vec<u8>,
    pub object_world_digest: u64,
    pub owner9_units: Vec<WildlifeOwner9UnitFact>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WildlifeAxis {
    X,
    Y,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WildlifeRandomDraw {
    pub axis: WildlifeAxis,
    pub state_before: i32,
    pub value: i32,
    pub state_after: i32,
    pub modulus: i32,
    pub coordinate: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WildlifeCandidateReceipt {
    pub attempt: i32,
    pub x: i32,
    pub y: i32,
    pub x_draw: Option<WildlifeRandomDraw>,
    pub y_draw: Option<WildlifeRandomDraw>,
    pub cell_flags: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WildlifeInitUnitRequest {
    pub owner: i32,
    pub type_index: i32,
    pub x: i32,
    pub y: i32,
    pub angle: i32,
    pub captain_o: i32,
    pub captain_who: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WildlifeSpawnRequired {
    pub frame: i32,
    pub map_dimensions: (i32, i32),
    pub quota: i32,
    pub existing_wildbirds: i32,
    pub attempts: i32,
    pub rejected: Vec<WildlifeCandidateReceipt>,
    pub candidate: WildlifeCandidateReceipt,
    pub request: WildlifeInitUnitRequest,
    pub random_state_before: i32,
    /// The state retail reaches immediately before the first unowned child call. It is reported
    /// for composition, but is never published by this transaction.
    pub random_state_at_child: i32,
    pub first_unowned_child_va: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WildlifeZeroSpawnReceipt {
    pub frame: i32,
    pub map_dimensions: (i32, i32),
    pub quota: i32,
    pub existing_wildbirds: i32,
    pub attempts: i32,
    pub candidates: Vec<WildlifeCandidateReceipt>,
    pub random_state_before: i32,
    pub random_state_after: i32,
    pub map_checksum: WorldChecksum,
    pub object_world_digest: u64,
    pub authority_revision: u64,
    pub authority_digest: [u8; 32],
}

impl WildlifeZeroSpawnReceipt {
    pub fn validates(&self) -> bool {
        if self.frame != WILDLIFE_FRAME
            || self.map_dimensions.0 <= 0
            || self.map_dimensions.1 <= 0
            || self.quota != wildlife_quota(self.map_dimensions.0, self.map_dimensions.1)
            || self.attempts != (self.quota - self.existing_wildbirds).max(0)
            || self.candidates.len() != self.attempts as usize
            || self.authority_revision == 0
            || self.authority_digest == [0; 32]
        {
            return false;
        }
        let mut random = Random::new(self.random_state_before);
        for (attempt, candidate) in self.candidates.iter().enumerate() {
            if candidate.attempt != attempt as i32 || candidate.cell_flags & WILDLIFE_CELL_FLAG != 0
            {
                return false;
            }
            let (x, x_draw) = draw_axis(&mut random, self.map_dimensions.0, WildlifeAxis::X);
            let (y, y_draw) = draw_axis(&mut random, self.map_dimensions.1, WildlifeAxis::Y);
            if (candidate.x, candidate.y, candidate.x_draw, candidate.y_draw)
                != (x, y, x_draw, y_draw)
            {
                return false;
            }
        }
        random.state() == self.random_state_after
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WildlifeFrameError {
    MissingAuthorityRevision,
    MissingAuthorityDigest,
    WrongFrame(i32),
    FrameMismatch { world: i32, authority: i32 },
    RandomStateMismatch { world: i32, authority: i32 },
    MapChecksumMismatch,
    MapChecksumImageMismatch,
    ObjectWorldDigestMismatch,
    InvalidMapDimensions { xs: i32, ys: i32, cells: usize },
    Owner9MarkInvalid(i32),
    Owner9ReservedSlot(i32),
    Owner9WrongIdentity(i32),
    Owner9AuthorityMissing(i32),
    Owner9AuthorityExtra,
    Owner9AuthorityMismatch(i32),
    UnitTypeProjectionMismatch(usize),
    SpawnRequired(WildlifeSpawnRequired),
    StaleCanonicalState,
}

#[derive(Clone, Debug)]
pub struct PreparedWildlifeZeroSpawn {
    authority_before: WildlifeFrameAuthority,
    random_after: Random,
    receipt: WildlifeZeroSpawnReceipt,
}

fn wildlife_quota(xs: i32, ys: i32) -> i32 {
    xs.wrapping_mul(ys)
        .wrapping_div(WILDLIFE_AREA_DIVISOR)
        .min(WILDLIFE_MAX)
}

fn draw_axis(
    random: &mut Random,
    modulus: i32,
    axis: WildlifeAxis,
) -> (i32, Option<WildlifeRandomDraw>) {
    // `dimension == 1 || dimension - 1 < 0` at 0x0065DFA0/0x0065DFCA.
    if modulus <= 1 {
        return (0, None);
    }
    let state_before = random.state();
    let value = random.get(RANDOM_LOW, RANDOM_HIGH);
    let state_after = random.state();
    let coordinate = value % modulus;
    (
        coordinate,
        Some(WildlifeRandomDraw {
            axis,
            state_before,
            value,
            state_after,
            modulus,
            coordinate,
        }),
    )
}

fn validate_authoritative_state(
    world: &World,
    map: &TerrainWorld,
    unit_types: &[i32],
    authority: &WildlifeFrameAuthority,
) -> Result<i32, WildlifeFrameError> {
    if authority.revision == 0 {
        return Err(WildlifeFrameError::MissingAuthorityRevision);
    }
    if authority.composition_digest == [0; 32] {
        return Err(WildlifeFrameError::MissingAuthorityDigest);
    }
    if authority.frame != WILDLIFE_FRAME {
        return Err(WildlifeFrameError::WrongFrame(authority.frame));
    }
    if world.frame != authority.frame {
        return Err(WildlifeFrameError::FrameMismatch {
            world: world.frame,
            authority: authority.frame,
        });
    }
    if world.random.state() != authority.random_state {
        return Err(WildlifeFrameError::RandomStateMismatch {
            world: world.random.state(),
            authority: authority.random_state,
        });
    }
    let cells = map
        .xs
        .checked_mul(map.ys)
        .and_then(|n| usize::try_from(n).ok());
    if map.xs <= 0 || map.ys <= 0 || cells != Some(map.wdata.len()) {
        return Err(WildlifeFrameError::InvalidMapDimensions {
            xs: map.xs,
            ys: map.ys,
            cells: map.wdata.len(),
        });
    }
    if map.checksum_sections() != authority.map_checksum {
        return Err(WildlifeFrameError::MapChecksumMismatch);
    }
    if map.checksum_image().0 != authority.map_checksum_image {
        return Err(WildlifeFrameError::MapChecksumImageMismatch);
    }
    if world.digest() != authority.object_world_digest {
        return Err(WildlifeFrameError::ObjectWorldDigestMismatch);
    }

    let registry = world.object_bands();
    let mark = registry
        .mark(WILDLIFE_OWNER as usize, RetailBand::Unit)
        .ok_or(WildlifeFrameError::Owner9MarkInvalid(-1))?;
    if !(RetailBand::Unit.base()..=RetailBand::Unit.limit()).contains(&mark) {
        return Err(WildlifeFrameError::Owner9MarkInvalid(mark));
    }
    let mut facts = authority.owner9_units.iter();
    let mut existing = 0i32;
    for o in RetailBand::Unit.base()..mark {
        let address = RetailObjectAddress::new(WILDLIFE_OWNER, RetailBand::Unit, o);
        let slot = registry
            .slot(address)
            .ok_or(WildlifeFrameError::Owner9WrongIdentity(o))?;
        let identity = match slot.lifecycle {
            SparseSlotLifecycle::Tombstone(_) => continue,
            SparseSlotLifecycle::Reserved { .. } => {
                return Err(WildlifeFrameError::Owner9ReservedSlot(o));
            }
            SparseSlotLifecycle::Live(identity) => identity,
        };
        let WorldObjectIdentity::Unit { id, generation } = identity else {
            return Err(WildlifeFrameError::Owner9WrongIdentity(o));
        };
        let handle = Handle { id, generation };
        let row = world
            .row_of(handle)
            .ok_or(WildlifeFrameError::Owner9WrongIdentity(o))?;
        let fact = facts
            .next()
            .ok_or(WildlifeFrameError::Owner9AuthorityMissing(o))?;
        let projected_type = unit_types
            .get(row)
            .copied()
            .ok_or(WildlifeFrameError::UnitTypeProjectionMismatch(row))?;
        let canonical_type = world
            .unit_type_id(row)
            .ok_or(WildlifeFrameError::UnitTypeProjectionMismatch(row))?;
        if projected_type != canonical_type {
            return Err(WildlifeFrameError::UnitTypeProjectionMismatch(row));
        }
        let flags = world.units.get_flags(row);
        if fact.handle != handle
            || i32::from(fact.o) != o
            || world.units.get_who(row) != WILDLIFE_OWNER
            || i32::from(world.units.o()[row]) != o
            || fact.uid != world.units.get_uid(row)
            || fact.flags != flags
            || fact.type_index != canonical_type
        {
            return Err(WildlifeFrameError::Owner9AuthorityMismatch(o));
        }
        if flags & OBJ_FLAG_ACTIVE != 0 && fact.is_animal && fact.type_index == WILDBIRD_TYPE {
            existing += 1;
        }
    }
    if facts.next().is_some() {
        return Err(WildlifeFrameError::Owner9AuthorityExtra);
    }
    Ok(existing)
}

pub fn prepare_wildlife_zero_spawn(
    world: &World,
    map: &TerrainWorld,
    unit_types: &[i32],
    authority: &WildlifeFrameAuthority,
) -> Result<PreparedWildlifeZeroSpawn, WildlifeFrameError> {
    let existing_wildbirds = validate_authoritative_state(world, map, unit_types, authority)?;
    let quota = wildlife_quota(map.xs, map.ys);
    let attempts = (quota - existing_wildbirds).max(0);
    let random_state_before = world.random.state();
    let mut random_after = world.random;
    let mut candidates = Vec::with_capacity(attempts as usize);
    for attempt in 0..attempts {
        let (x, x_draw) = draw_axis(&mut random_after, map.xs, WildlifeAxis::X);
        let (y, y_draw) = draw_axis(&mut random_after, map.ys, WildlifeAxis::Y);
        let cell = &map.wdata[(y * map.xs + x) as usize];
        let candidate = WildlifeCandidateReceipt {
            attempt,
            x,
            y,
            x_draw,
            y_draw,
            cell_flags: cell.flags,
        };
        if cell.flags & WILDLIFE_CELL_FLAG != 0 {
            return Err(WildlifeFrameError::SpawnRequired(WildlifeSpawnRequired {
                frame: world.frame,
                map_dimensions: (map.xs, map.ys),
                quota,
                existing_wildbirds,
                attempts,
                rejected: candidates,
                candidate,
                request: WildlifeInitUnitRequest {
                    owner: i32::from(WILDLIFE_OWNER),
                    type_index: WILDBIRD_TYPE,
                    x: x.wrapping_mul(WORLD_CELL_COORD) + WORLD_CELL_CENTER,
                    y: y.wrapping_mul(WORLD_CELL_COORD) + WORLD_CELL_CENTER,
                    angle: -1,
                    captain_o: -1,
                    captain_who: -1,
                },
                random_state_before,
                random_state_at_child: random_after.state(),
                first_unowned_child_va: OBJECTS_INIT_UNIT_VA,
            }));
        }
        candidates.push(candidate);
    }
    let receipt = WildlifeZeroSpawnReceipt {
        frame: world.frame,
        map_dimensions: (map.xs, map.ys),
        quota,
        existing_wildbirds,
        attempts,
        candidates,
        random_state_before,
        random_state_after: random_after.state(),
        map_checksum: authority.map_checksum.clone(),
        object_world_digest: authority.object_world_digest,
        authority_revision: authority.revision,
        authority_digest: authority.composition_digest,
    };
    debug_assert!(receipt.validates());
    Ok(PreparedWildlifeZeroSpawn {
        authority_before: authority.clone(),
        random_after,
        receipt,
    })
}

pub fn commit_wildlife_zero_spawn(
    world: &mut World,
    map: &TerrainWorld,
    unit_types: &[i32],
    authority: &WildlifeFrameAuthority,
    prepared: PreparedWildlifeZeroSpawn,
) -> Result<WildlifeZeroSpawnReceipt, WildlifeFrameError> {
    if authority != &prepared.authority_before
        || validate_authoritative_state(world, map, unit_types, authority).is_err()
        || !prepared.receipt.validates()
    {
        return Err(WildlifeFrameError::StaleCanonicalState);
    }
    world.random = prepared.random_after;
    Ok(prepared.receipt)
}
