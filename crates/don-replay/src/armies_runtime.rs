//! Exact, fail-closed runtime walk for the orphan Armies checksum entry point.
//!
//! `CheckSums::check_armies` is not one of the fifteen channels called by
//! `CheckSums::check_all`; it is a thunk to the save-game `Armies::walk_data` path.  This
//! module therefore does not install a replay producer.  It preserves the exact walk as a
//! separate frontier because Army decisions eventually mutate replay-visible Groups and
//! Units, and because the same byte stream verifies save/load state.

use std::fmt;

use don_sim::systems::armies::{Armies, ARMIES_PER_PLAYER, ARMY_WALK_HEAD, ARMY_WALK_TAIL_HI};

/// Orphan `CheckSums::check_armies(CheckSum*)`, a thunk to [`ARMIES_WALK_VA`].
pub const CHECK_ARMIES_VA: u32 = 0x0093_6cf0;
/// `Armies::walk_data(DataWalk*)`, used by save/load verification and GameLog.
pub const ARMIES_WALK_VA: u32 = 0x006f_3700;
/// `Army::walk_data(DataWalk*)`.
pub const ARMY_WALK_VA: u32 = 0x006f_9850;

/// Fixed owner-list count reached by `Armies::walk_data`.
pub const ARMY_OWNER_SLOTS: usize = 8;
/// Bytes emitted for a non-empty pointer-array apart from its pointer-presence pass:
/// length, capacity, increment, masked flags, then capacity and increment again.
pub const PTR_ARRAY_NONEMPTY_FIXED_BYTES: usize = 17;
/// Exact post-`Armies::init` walk size when all 128 Army records are invalid.
pub const INITIAL_ARMIES_WALK_BYTES: usize = ARMY_OWNER_SLOTS
    * (PTR_ARRAY_NONEMPTY_FIXED_BYTES + ARMIES_PER_PLAYER + ARMIES_PER_PLAYER * ARMY_WALK_HEAD);

/// Exact result of one independent Armies walk from a fresh Adler seed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArmiesWalkValue {
    pub checksum: u32,
    pub bytes_walked: u64,
    pub owner_lists: u32,
    pub pointer_slots: u32,
    pub live_armies: u32,
}

/// Refusal to synthesize pointer-array history from a noncanonical container shape.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArmiesRuntimeError {
    OwnerListCount {
        expected: usize,
        actual: usize,
    },
    ArmySlotCount {
        owner: usize,
        expected: usize,
        actual: usize,
    },
}

impl fmt::Display for ArmiesRuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OwnerListCount { expected, actual } => write!(
                f,
                "Armies has {actual} owner lists; post-init authority requires {expected}"
            ),
            Self::ArmySlotCount {
                owner,
                expected,
                actual,
            } => write!(
                f,
                "Armies owner {owner} has {actual} pointer slots; post-init authority requires {expected}"
            ),
        }
    }
}

impl std::error::Error for ArmiesRuntimeError {}

/// Produce the exact `Armies::walk_data` byte stream after canonical `Armies::init`.
///
/// Retail preallocates sixteen non-null `Army` pointers for each of eight owners.  That
/// construction proves the otherwise-erased `PtrArray` history used here: length and
/// capacity are both 16, increment is -1, flags are zero after masking bit `0x40`, and all
/// sixteen pointer-presence bytes are one.  A differently shaped Rust container is refused
/// before any bytes are published.
///
/// Every invalid Army contributes only its two-byte `valid` field.  A live Army contributes
/// its complete 152-byte `ArmyData` image.  Dormant fields in an invalid slot therefore do
/// not affect the result, matching `Army::walk_data`.
pub fn armies_walk_bytes(
    armies: &Armies,
) -> Result<(Vec<u8>, ArmiesWalkValue), ArmiesRuntimeError> {
    if armies.lists.len() != ARMY_OWNER_SLOTS {
        return Err(ArmiesRuntimeError::OwnerListCount {
            expected: ARMY_OWNER_SLOTS,
            actual: armies.lists.len(),
        });
    }
    for (owner, list) in armies.lists.iter().enumerate() {
        if list.len() != ARMIES_PER_PLAYER {
            return Err(ArmiesRuntimeError::ArmySlotCount {
                owner,
                expected: ARMIES_PER_PLAYER,
                actual: list.len(),
            });
        }
    }

    let live_armies = armies
        .lists
        .iter()
        .flatten()
        .filter(|army| army.valid != 0)
        .count();
    let mut out = Vec::with_capacity(INITIAL_ARMIES_WALK_BYTES + live_armies * 150);
    let length = ARMIES_PER_PLAYER as i32;
    let increment = -1i16;

    for list in &armies.lists {
        out.extend_from_slice(&length.to_le_bytes());
        out.extend_from_slice(&length.to_le_bytes());
        out.extend_from_slice(&increment.to_le_bytes());
        out.push(0); // PtrArray flags after persistent `&= 0xbf`.
        out.extend(std::iter::repeat_n(1, ARMIES_PER_PLAYER));
        out.extend_from_slice(&length.to_le_bytes());
        out.extend_from_slice(&increment.to_le_bytes());

        for army in list {
            let image = army.image();
            out.extend_from_slice(&image[..ARMY_WALK_HEAD]);
            if army.valid != 0 {
                out.extend_from_slice(&image[ARMY_WALK_HEAD..ARMY_WALK_TAIL_HI]);
            }
        }
    }

    let value = ArmiesWalkValue {
        checksum: don_sim::checksum::adler32(1, &out),
        bytes_walked: out.len() as u64,
        owner_lists: ARMY_OWNER_SLOTS as u32,
        pointer_slots: (ARMY_OWNER_SLOTS * ARMIES_PER_PLAYER) as u32,
        live_armies: live_armies as u32,
    };
    Ok((out, value))
}
