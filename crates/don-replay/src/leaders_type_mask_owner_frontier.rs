// SPDX-License-Identifier: GPL-3.0-or-later
//! Canonical BHS type-owner join for walked Leader `tech` metadata and `obs_flags`.
//!
//! The current `tech` payload is already owned by the production runtime. The canonical
//! [`TypeBuiltinState`] additionally retains the retail bit-count/size header and the complete
//! observation mask mutated by builtins 815..=819. This binder compares its bytes to the exact
//! already-bound dynamic transcript and promotes only the 8-byte `tech` header plus all 109
//! walked `obs_flags` bytes. It remains nested under the frame-zero producer until the canonical
//! type owner is mounted directly on `Sim`, so it cannot survive or install channel 7.

#![forbid(unsafe_code)]

use crate::leader_initial_prefix::CHECKSUM_LEADER_SLOTS;
use crate::leaders_dynamic_children_frontier::{TECH_MASK_BITS, TECH_MASK_BYTES};
use crate::leaders_runtime_frontier::LeadersWalkFrontier;
use crate::leaders_setup_build_registry_frontier::RuntimeLeadersFrameZeroBuildRegistryFrontier;
use don_sim::systems::bhs_type_table::TypeBuiltinState;

pub const TYPE_MASK_HEADER_WALKED_BYTES: usize = 8;
pub const OBS_FLAGS_WALKED_BYTES: usize = TYPE_MASK_HEADER_WALKED_BYTES + TECH_MASK_BYTES;
pub const TYPE_MASK_NEWLY_CANONICAL_WALKED_BYTES: usize =
    TYPE_MASK_HEADER_WALKED_BYTES + OBS_FLAGS_WALKED_BYTES;

const _: () = assert!(OBS_FLAGS_WALKED_BYTES == 109);
const _: () = assert!(TYPE_MASK_NEWLY_CANONICAL_WALKED_BYTES == 117);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypeMaskOwnerSource {
    CanonicalTypeBuiltinState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TypeMaskOwnerClaim {
    pub slot: u8,
    pub source: TypeMaskOwnerSource,
    pub tech_duplicate_payload_bytes: usize,
    pub newly_canonical_walked_bytes: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeLeadersTypeMaskOwnerFrontier {
    inner: Box<RuntimeLeadersFrameZeroBuildRegistryFrontier>,
    claims: Vec<TypeMaskOwnerClaim>,
}

impl RuntimeLeadersTypeMaskOwnerFrontier {
    pub fn inner(&self) -> &RuntimeLeadersFrameZeroBuildRegistryFrontier {
        self.inner.as_ref()
    }

    pub fn claims(&self) -> &[TypeMaskOwnerClaim] {
        &self.claims
    }

    pub fn newly_canonicalized_walked_bytes(&self) -> usize {
        self.claims
            .iter()
            .map(|claim| claim.newly_canonical_walked_bytes)
            .sum()
    }

    pub fn unique_canonical_walked_bytes(&self) -> usize {
        self.inner.unique_canonical_walked_bytes() + self.newly_canonicalized_walked_bytes()
    }

    pub fn remaining_unsourced_walked_bytes(&self) -> u64 {
        self.walk_frontier()
            .bytes_walked
            .saturating_sub(self.unique_canonical_walked_bytes() as u64)
    }

    pub fn walk_frontier(&self) -> LeadersWalkFrontier {
        self.inner.walk_frontier()
    }

    pub fn checksum(&self) -> Result<(u32, u64), LeadersWalkFrontier> {
        Err(self.walk_frontier())
    }

    pub const fn installed_in_scoreboard(&self) -> bool {
        false
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypeMaskOwnerError {
    RosterDisagreement {
        slot: usize,
        transcript_active: bool,
        type_owner_active: bool,
    },
    InvalidOwnerShape {
        slot: usize,
        field: &'static str,
        bits: i32,
        size: i32,
    },
    MissingTranscriptField {
        slot: usize,
        field: &'static str,
    },
    TranscriptDisagreement {
        slot: usize,
        field: &'static str,
        byte: usize,
        owner: u8,
        conditional: u8,
    },
}

impl std::fmt::Display for TypeMaskOwnerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Leader type-mask owner refused: {self:?}")
    }
}

impl std::error::Error for TypeMaskOwnerError {}

fn walked_mask_bytes(bits: i32, size: i32, payload: &[u8; TECH_MASK_BYTES]) -> Vec<u8> {
    let mut out = Vec::with_capacity(TYPE_MASK_HEADER_WALKED_BYTES + TECH_MASK_BYTES);
    out.extend_from_slice(&bits.to_le_bytes());
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(payload);
    out
}

/// Compare the canonical live type owner to the exact transcript already admitted below it.
pub fn bind_type_mask_owner(
    inner: RuntimeLeadersFrameZeroBuildRegistryFrontier,
    types: &TypeBuiltinState,
) -> Result<RuntimeLeadersTypeMaskOwnerFrontier, TypeMaskOwnerError> {
    let dynamic = inner.inner().inner().inner().inner().inner().inner();
    let mut claims = Vec::new();
    for slot in 0..CHECKSUM_LEADER_SLOTS {
        let transcript_active = dynamic.rows()[slot].active;
        let owner = &types.leaders[slot];
        let type_owner_active = owner.leader_flags & 1 != 0;
        if transcript_active != type_owner_active {
            return Err(TypeMaskOwnerError::RosterDisagreement {
                slot,
                transcript_active,
                type_owner_active,
            });
        }
        if !transcript_active {
            continue;
        }
        for (field, mask) in [("tech", &owner.tech), ("obs_flags", &owner.obs_flags)] {
            if mask.bits != TECH_MASK_BITS || mask.size != TECH_MASK_BYTES as i32 {
                return Err(TypeMaskOwnerError::InvalidOwnerShape {
                    slot,
                    field,
                    bits: mask.bits,
                    size: mask.size,
                });
            }
            let owner_bytes = walked_mask_bytes(mask.bits, mask.size, &mask.bytes);
            let conditional = dynamic.rows()[slot]
                .walked_field(field)
                .ok_or(TypeMaskOwnerError::MissingTranscriptField { slot, field })?;
            if let Some(byte) = owner_bytes
                .iter()
                .zip(conditional)
                .position(|(owner, conditional)| owner != conditional)
            {
                return Err(TypeMaskOwnerError::TranscriptDisagreement {
                    slot,
                    field,
                    byte,
                    owner: owner_bytes[byte],
                    conditional: conditional[byte],
                });
            }
        }
        claims.push(TypeMaskOwnerClaim {
            slot: slot as u8,
            source: TypeMaskOwnerSource::CanonicalTypeBuiltinState,
            tech_duplicate_payload_bytes: TECH_MASK_BYTES,
            newly_canonical_walked_bytes: TYPE_MASK_NEWLY_CANONICAL_WALKED_BYTES,
        });
    }
    Ok(RuntimeLeadersTypeMaskOwnerFrontier {
        inner: Box::new(inner),
        claims,
    })
}
