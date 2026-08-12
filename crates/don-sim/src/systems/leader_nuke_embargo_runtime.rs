// SPDX-License-Identifier: GPL-3.0-or-later

//! Canonical owner and exact retail children for the nuclear embargo consulted by
//! `Leader::market_speculation`.
//!
//! This closes `LeaderData::get_my_nuke_embargo` `0x006D5350` (98 bytes) and
//! `LeaderData::get_nuke_embargo` `0x006D52C0` (129 bytes). The two mutable PDB inputs are
//! `LeaderData::nuke_stamp` `+0x7B4` and `LeaderData::nukes_used` `+0x7BC`; both are plain i32
//! values and neither currently has a canonical save owner.

use std::fmt;

pub const LEADER_COUNT: usize = 8;
pub const GET_MY_NUKE_EMBARGO_VA: u32 = 0x006d_5350;
pub const GET_NUKE_EMBARGO_VA: u32 = 0x006d_52c0;
pub const HAS_WONDER_VA: u32 = 0x006e_bc10;
pub const IS_ALLY_VA: u32 = 0x006e_db50;
pub const ANTI_NUKE_WONDER: i32 = 0x021e;
pub const NUKE_EMBARGO_VALUES_PER_LEADER: usize = 2;
pub const NUKE_EMBARGO_BYTES_PER_LEADER: usize = NUKE_EMBARGO_VALUES_PER_LEADER * 4;
pub const NUKE_EMBARGO_TABLE_BYTES: usize = LEADER_COUNT * NUKE_EMBARGO_BYTES_PER_LEADER;

/// Canonical plain fields read by `get_my_nuke_embargo`, in PDB order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CanonicalNukeEmbargo {
    /// `LeaderData +0x7B4`. Zero disables the personal timer before the wonder query.
    pub nuke_stamp: i32,
    /// `LeaderData +0x7BC`. Multiplied by `NUKE_EMBARGO_NATION`.
    pub nukes_used: i32,
}

impl CanonicalNukeEmbargo {
    pub const fn extension_values(self) -> [i32; NUKE_EMBARGO_VALUES_PER_LEADER] {
        [self.nuke_stamp, self.nukes_used]
    }

    pub const fn from_extension_values(values: [i32; NUKE_EMBARGO_VALUES_PER_LEADER]) -> Self {
        Self {
            nuke_stamp: values[0],
            nukes_used: values[1],
        }
    }

    pub fn extension_bytes(self) -> [u8; NUKE_EMBARGO_BYTES_PER_LEADER] {
        let mut bytes = [0; NUKE_EMBARGO_BYTES_PER_LEADER];
        for (index, value) in self.extension_values().into_iter().enumerate() {
            bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    pub fn from_extension_bytes(bytes: &[u8]) -> Result<Self, NukeEmbargoCodecError> {
        if bytes.len() != NUKE_EMBARGO_BYTES_PER_LEADER {
            return Err(NukeEmbargoCodecError::Length {
                expected: NUKE_EMBARGO_BYTES_PER_LEADER,
                actual: bytes.len(),
            });
        }
        Ok(Self {
            nuke_stamp: i32::from_le_bytes(bytes[0..4].try_into().expect("row length checked")),
            nukes_used: i32::from_le_bytes(bytes[4..8].try_into().expect("row length checked")),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NukeEmbargoCodecError {
    Length { expected: usize, actual: usize },
}

impl fmt::Display for NukeEmbargoCodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Length { expected, actual } => {
                write!(f, "nuke-embargo row is {actual} bytes; expected {expected}")
            }
        }
    }
}

impl std::error::Error for NukeEmbargoCodecError {}

pub fn table_extension_bytes(
    rows: &[CanonicalNukeEmbargo; LEADER_COUNT],
) -> [u8; NUKE_EMBARGO_TABLE_BYTES] {
    let mut bytes = [0; NUKE_EMBARGO_TABLE_BYTES];
    for (slot, row) in rows.iter().copied().enumerate() {
        let start = slot * NUKE_EMBARGO_BYTES_PER_LEADER;
        bytes[start..start + NUKE_EMBARGO_BYTES_PER_LEADER].copy_from_slice(&row.extension_bytes());
    }
    bytes
}

pub fn table_from_extension_bytes(
    bytes: &[u8],
) -> Result<[CanonicalNukeEmbargo; LEADER_COUNT], NukeEmbargoCodecError> {
    if bytes.len() != NUKE_EMBARGO_TABLE_BYTES {
        return Err(NukeEmbargoCodecError::Length {
            expected: NUKE_EMBARGO_TABLE_BYTES,
            actual: bytes.len(),
        });
    }
    let mut rows = [CanonicalNukeEmbargo::default(); LEADER_COUNT];
    for (slot, row) in rows.iter_mut().enumerate() {
        let start = slot * NUKE_EMBARGO_BYTES_PER_LEADER;
        *row = CanonicalNukeEmbargo::from_extension_bytes(
            &bytes[start..start + NUKE_EMBARGO_BYTES_PER_LEADER],
        )?;
    }
    Ok(rows)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NukeEmbargoRules {
    /// Rules `+0xD20`, shipped 900 frames.
    pub base: i32,
    /// Rules `+0xD24`, shipped 900 frames per nuke used by this nation.
    pub nation: i32,
    /// Rules `+0xD28`, shipped 0 frames per world nuke.
    pub world: i32,
}

impl NukeEmbargoRules {
    pub const fn shipped() -> Self {
        Self {
            base: 900,
            nation: 900,
            world: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MyNukeEmbargoInputs {
    pub state: CanonicalNukeEmbargo,
    /// Exact `has_wonder(0x21E)` answer for this row.
    pub has_anti_nuke_wonder: bool,
    /// `Game +0x6E0` global nuke count.
    pub world_nukes: i32,
    /// `Game +0x550` current frame.
    pub frame: i32,
    pub rules: NukeEmbargoRules,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MyNukeEmbargoReceipt {
    /// Retail does not call `has_wonder` when `nuke_stamp == 0`.
    pub read_wonder: bool,
    /// Wrapped x86 arithmetic before the signed clamp at zero. None on either early return.
    pub raw_timer: Option<i32>,
}

/// Complete `LeaderData::get_my_nuke_embargo` `0x006D5350`.
pub fn get_my_nuke_embargo(inputs: MyNukeEmbargoInputs) -> (i32, MyNukeEmbargoReceipt) {
    if inputs.state.nuke_stamp == 0 {
        return (0, MyNukeEmbargoReceipt::default());
    }
    if inputs.has_anti_nuke_wonder {
        return (
            0,
            MyNukeEmbargoReceipt {
                read_wonder: true,
                raw_timer: None,
            },
        );
    }

    let timer = inputs
        .rules
        .world
        .wrapping_mul(inputs.world_nukes)
        .wrapping_add(inputs.rules.nation.wrapping_mul(inputs.state.nukes_used))
        .wrapping_sub(inputs.frame)
        .wrapping_add(inputs.rules.base)
        .wrapping_add(inputs.state.nuke_stamp);
    (
        timer.max(0),
        MyNukeEmbargoReceipt {
            read_wonder: true,
            raw_timer: Some(timer),
        },
    )
}

/// Canonical existing match/relationship facts consumed by `get_nuke_embargo`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NukeEmbargoInputs {
    pub who: i32,
    pub active: [bool; LEADER_COUNT],
    /// Directional `LeaderData +0x74 + other*4` relation values. Retail requires 2 both ways.
    pub relations: [[i32; LEADER_COUNT]; LEADER_COUNT],
    pub has_anti_nuke_wonder: [bool; LEADER_COUNT],
    pub owners: [CanonicalNukeEmbargo; LEADER_COUNT],
    pub world_nukes: i32,
    pub frame: i32,
    pub rules: NukeEmbargoRules,
}

impl Default for NukeEmbargoInputs {
    fn default() -> Self {
        Self {
            who: 0,
            active: [false; LEADER_COUNT],
            relations: [[0; LEADER_COUNT]; LEADER_COUNT],
            has_anti_nuke_wonder: [false; LEADER_COUNT],
            owners: [CanonicalNukeEmbargo::default(); LEADER_COUNT],
            world_nukes: 0,
            frame: 0,
            rules: NukeEmbargoRules::shipped(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NukeEmbargoCall {
    OuterWonder {
        slot: usize,
        present: bool,
    },
    IsAlly {
        slot: usize,
        allied: bool,
    },
    MyEmbargo {
        slot: usize,
        value: i32,
        receipt: MyNukeEmbargoReceipt,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NukeEmbargoReceipt {
    pub visited_mask: u8,
    pub admitted_mask: u8,
    pub max_slot: Option<usize>,
    pub calls: Vec<NukeEmbargoCall>,
}

/// Complete `LeaderData::get_nuke_embargo` `0x006D52C0`.
///
/// A detached `who` outside retail's eight-row table is rejected before indexing instead of
/// turning an impossible pointer walk into gameplay.
pub fn get_nuke_embargo(inputs: &NukeEmbargoInputs) -> Option<(i32, NukeEmbargoReceipt)> {
    let who = usize::try_from(inputs.who)
        .ok()
        .filter(|&slot| slot < LEADER_COUNT)?;
    let mut receipt = NukeEmbargoReceipt::default();
    receipt.calls.push(NukeEmbargoCall::OuterWonder {
        slot: who,
        present: inputs.has_anti_nuke_wonder[who],
    });
    if inputs.has_anti_nuke_wonder[who] {
        return Some((0, receipt));
    }

    let mut maximum = 0_i32;
    for slot in 0..LEADER_COUNT {
        receipt.visited_mask |= 1 << slot;
        if !inputs.active[slot] {
            continue;
        }
        let admitted = if slot == who {
            true
        } else {
            let allied = inputs.relations[who][slot] == 2 && inputs.relations[slot][who] == 2;
            receipt.calls.push(NukeEmbargoCall::IsAlly { slot, allied });
            allied
        };
        if !admitted {
            continue;
        }
        receipt.admitted_mask |= 1 << slot;
        let (value, child) = get_my_nuke_embargo(MyNukeEmbargoInputs {
            state: inputs.owners[slot],
            has_anti_nuke_wonder: inputs.has_anti_nuke_wonder[slot],
            world_nukes: inputs.world_nukes,
            frame: inputs.frame,
            rules: inputs.rules,
        });
        receipt.calls.push(NukeEmbargoCall::MyEmbargo {
            slot,
            value,
            receipt: child,
        });
        if value > maximum {
            maximum = value;
            receipt.max_slot = Some(slot);
        }
    }
    Some((maximum, receipt))
}
