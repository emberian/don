// SPDX-License-Identifier: GPL-3.0-or-later
//! Atomic dual-mirror owner for the Citizen `Unit::think` Leader request bit.
//!
//! Retail type 50/51 Units clear their instance masks and then OR `0x0008_0000` into
//! `LeaderData::leader_flags` at `0x005F6FA3`. DoN deliberately mirrors that one retail
//! dword in the step-8 and victory-score owners; the Leaders checksum producer refuses when
//! they disagree. This module therefore prepares and commits both stores as one stale-checked
//! transaction. The later `leader_flags2 & 2` AI gate does not suppress this earlier OR.

#![forbid(unsafe_code)]

use crate::systems::{leaders, victory_score};

pub const UNIT_THINK_VA: u32 = 0x005f_6e40;
pub const UNIT_THINK_CITIZEN_TYPE_GATE_VA: u32 = 0x005f_6f83;
pub const UNIT_THINK_CITIZEN_MASK_CLEAR_VA: u32 = 0x005f_6f91;
pub const UNIT_THINK_LEADER_PENDING_OR_VA: u32 = 0x005f_6fa3;
pub const UNIT_THINK_FLAGS2_GATE_VA: u32 = 0x005f_6fde;
pub const UNIT_THINK_PENDING_FLAG: u32 = leaders::flag::PENDING;

const _: () = assert!(UNIT_THINK_PENDING_FLAG == 0x0008_0000);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnitThinkLeaderPendingSource {
    pub revision: u64,
    pub composition_digest: [u8; 32],
    pub owner: usize,
    pub unit_o: i32,
    pub unit_type: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreparedUnitThinkLeaderPending {
    pub source: UnitThinkLeaderPendingSource,
    pub victory_flags_before: i32,
    pub step8_flags_before: u32,
    pub victory_flags2_before: i32,
    pub step8_flags2_before: u32,
    pub flags_after: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnitThinkLeaderPendingReceipt {
    pub source: UnitThinkLeaderPendingSource,
    pub flags_before: u32,
    pub flags_after: u32,
    pub leader_flags2: u32,
    pub unit_ai_disabled: bool,
    pub changed: bool,
    pub retail_store_va: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitThinkLeaderPendingError {
    MissingRevision,
    MissingCompositionDigest,
    OwnerOutOfRange {
        owner: usize,
    },
    UnsupportedActor {
        unit_o: i32,
        unit_type: i32,
    },
    MirrorDisagreement {
        victory_flags: i32,
        step8_flags: u32,
    },
    Flags2MirrorDisagreement {
        victory_flags2: i32,
        step8_flags2: u32,
    },
    InactiveOwner {
        flags: u32,
    },
    SourceMismatch {
        prepared: UnitThinkLeaderPendingSource,
        current: UnitThinkLeaderPendingSource,
    },
    StaleBeforeImage {
        expected_victory_flags: i32,
        actual_victory_flags: i32,
        expected_step8_flags: u32,
        actual_step8_flags: u32,
        expected_victory_flags2: i32,
        actual_victory_flags2: i32,
        expected_step8_flags2: u32,
        actual_step8_flags2: u32,
    },
}

impl std::fmt::Display for UnitThinkLeaderPendingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Unit::think Leader pending transaction refused: {self:?}"
        )
    }
}

impl std::error::Error for UnitThinkLeaderPendingError {}

fn validate_source(
    source: UnitThinkLeaderPendingSource,
    leader_slots: usize,
) -> Result<(), UnitThinkLeaderPendingError> {
    if source.revision == 0 {
        return Err(UnitThinkLeaderPendingError::MissingRevision);
    }
    if source.composition_digest == [0; 32] {
        return Err(UnitThinkLeaderPendingError::MissingCompositionDigest);
    }
    if source.owner >= leader_slots {
        return Err(UnitThinkLeaderPendingError::OwnerOutOfRange {
            owner: source.owner,
        });
    }
    if source.unit_o < 0 || !matches!(source.unit_type, 50 | 51) {
        return Err(UnitThinkLeaderPendingError::UnsupportedActor {
            unit_o: source.unit_o,
            unit_type: source.unit_type,
        });
    }
    Ok(())
}

pub fn prepare_unit_think_leader_pending(
    source: UnitThinkLeaderPendingSource,
    leader_slots: usize,
    victory_flags: i32,
    step8_flags: u32,
    victory_flags2: i32,
    step8_flags2: u32,
) -> Result<PreparedUnitThinkLeaderPending, UnitThinkLeaderPendingError> {
    validate_source(source, leader_slots)?;
    if victory_flags as u32 != step8_flags {
        return Err(UnitThinkLeaderPendingError::MirrorDisagreement {
            victory_flags,
            step8_flags,
        });
    }
    if victory_flags2 as u32 != step8_flags2 {
        return Err(UnitThinkLeaderPendingError::Flags2MirrorDisagreement {
            victory_flags2,
            step8_flags2,
        });
    }
    let required = (victory_score::leader_flag::VALID | victory_score::leader_flag::ACTIVE) as u32;
    if step8_flags & required != required {
        return Err(UnitThinkLeaderPendingError::InactiveOwner { flags: step8_flags });
    }
    Ok(PreparedUnitThinkLeaderPending {
        source,
        victory_flags_before: victory_flags,
        step8_flags_before: step8_flags,
        victory_flags2_before: victory_flags2,
        step8_flags2_before: step8_flags2,
        flags_after: step8_flags | UNIT_THINK_PENDING_FLAG,
    })
}

pub fn commit_unit_think_leader_pending(
    source: UnitThinkLeaderPendingSource,
    prepared: PreparedUnitThinkLeaderPending,
    victory_flags: &mut i32,
    step8_flags: &mut u32,
    victory_flags2: i32,
    step8_flags2: u32,
) -> Result<UnitThinkLeaderPendingReceipt, UnitThinkLeaderPendingError> {
    if source != prepared.source {
        return Err(UnitThinkLeaderPendingError::SourceMismatch {
            prepared: prepared.source,
            current: source,
        });
    }
    if *victory_flags != prepared.victory_flags_before
        || *step8_flags != prepared.step8_flags_before
        || victory_flags2 != prepared.victory_flags2_before
        || step8_flags2 != prepared.step8_flags2_before
    {
        return Err(UnitThinkLeaderPendingError::StaleBeforeImage {
            expected_victory_flags: prepared.victory_flags_before,
            actual_victory_flags: *victory_flags,
            expected_step8_flags: prepared.step8_flags_before,
            actual_step8_flags: *step8_flags,
            expected_victory_flags2: prepared.victory_flags2_before,
            actual_victory_flags2: victory_flags2,
            expected_step8_flags2: prepared.step8_flags2_before,
            actual_step8_flags2: step8_flags2,
        });
    }

    // No fallible work follows the first store: either both mirrors commit or neither moves.
    *victory_flags = prepared.flags_after as i32;
    *step8_flags = prepared.flags_after;
    Ok(UnitThinkLeaderPendingReceipt {
        source,
        flags_before: prepared.step8_flags_before,
        flags_after: prepared.flags_after,
        leader_flags2: prepared.step8_flags2_before,
        unit_ai_disabled: prepared.step8_flags2_before
            & victory_score::leader_flag2::UNIT_AI_OFF as u32
            != 0,
        changed: prepared.step8_flags_before != prepared.flags_after,
        retail_store_va: UNIT_THINK_LEADER_PENDING_OR_VA,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tick::Sim;

    fn source() -> UnitThinkLeaderPendingSource {
        UnitThinkLeaderPendingSource {
            revision: 7,
            composition_digest: [0xa5; 32],
            owner: 0,
            unit_o: 3,
            unit_type: 50,
        }
    }

    #[test]
    fn exact_or_commits_both_mirrors_and_is_idempotent() {
        let mut victory = 7i32;
        let mut step8 = 7u32;
        let prepared =
            prepare_unit_think_leader_pending(source(), 8, victory, step8, 0, 0).unwrap();
        let receipt =
            commit_unit_think_leader_pending(source(), prepared, &mut victory, &mut step8, 0, 0)
                .unwrap();
        assert_eq!(receipt.flags_before, 7);
        assert_eq!(receipt.flags_after, 0x0008_0007);
        assert!(receipt.changed);
        assert_eq!(victory as u32, receipt.flags_after);
        assert_eq!(step8, receipt.flags_after);

        let prepared =
            prepare_unit_think_leader_pending(source(), 8, victory, step8, 2, 2).unwrap();
        let receipt =
            commit_unit_think_leader_pending(source(), prepared, &mut victory, &mut step8, 2, 2)
                .unwrap();
        assert!(!receipt.changed);
        assert!(receipt.unit_ai_disabled);
    }

    #[test]
    fn source_and_before_image_mutations_refuse_without_a_partial_store() {
        let mut victory = 7i32;
        let mut step8 = 7u32;
        let prepared =
            prepare_unit_think_leader_pending(source(), 8, victory, step8, 0, 0).unwrap();

        let mut changed_source = source();
        changed_source.revision += 1;
        assert!(matches!(
            commit_unit_think_leader_pending(
                changed_source,
                prepared,
                &mut victory,
                &mut step8,
                0,
                0,
            ),
            Err(UnitThinkLeaderPendingError::SourceMismatch { .. })
        ));
        assert_eq!((victory, step8), (7, 7));

        victory |= 0x100;
        assert!(matches!(
            commit_unit_think_leader_pending(source(), prepared, &mut victory, &mut step8, 0, 0),
            Err(UnitThinkLeaderPendingError::StaleBeforeImage { .. })
        ));
        assert_eq!(step8, 7);

        victory = 7;
        step8 |= 0x200;
        assert!(matches!(
            commit_unit_think_leader_pending(source(), prepared, &mut victory, &mut step8, 0, 0),
            Err(UnitThinkLeaderPendingError::StaleBeforeImage { .. })
        ));
        assert_eq!(victory, 7);
    }

    #[test]
    fn malformed_authority_and_mirror_drift_fail_closed() {
        let mut invalid = source();
        invalid.revision = 0;
        assert_eq!(
            prepare_unit_think_leader_pending(invalid, 8, 7, 7, 0, 0),
            Err(UnitThinkLeaderPendingError::MissingRevision)
        );
        invalid = source();
        invalid.composition_digest = [0; 32];
        assert_eq!(
            prepare_unit_think_leader_pending(invalid, 8, 7, 7, 0, 0),
            Err(UnitThinkLeaderPendingError::MissingCompositionDigest)
        );
        invalid = source();
        invalid.owner = 8;
        assert_eq!(
            prepare_unit_think_leader_pending(invalid, 8, 7, 7, 0, 0),
            Err(UnitThinkLeaderPendingError::OwnerOutOfRange { owner: 8 })
        );
        invalid = source();
        invalid.unit_type = 52;
        assert_eq!(
            prepare_unit_think_leader_pending(invalid, 8, 7, 7, 0, 0),
            Err(UnitThinkLeaderPendingError::UnsupportedActor {
                unit_o: 3,
                unit_type: 52,
            })
        );
        assert!(matches!(
            prepare_unit_think_leader_pending(source(), 8, 7, 3, 0, 0),
            Err(UnitThinkLeaderPendingError::MirrorDisagreement { .. })
        ));
        assert!(matches!(
            prepare_unit_think_leader_pending(source(), 8, 7, 7, 0, 2),
            Err(UnitThinkLeaderPendingError::Flags2MirrorDisagreement { .. })
        ));
        assert_eq!(
            prepare_unit_think_leader_pending(source(), 8, 1, 1, 0, 0),
            Err(UnitThinkLeaderPendingError::InactiveOwner { flags: 1 })
        );
    }

    #[test]
    fn sim_mount_updates_the_checksum_mirrors_and_bites_drift() {
        let mut sim = Sim::new(11, 16);
        sim.activate(0);
        let prepared = sim.prepare_unit_think_leader_pending(source()).unwrap();
        let receipt = sim
            .commit_unit_think_leader_pending(source(), prepared)
            .unwrap();
        assert_eq!(receipt.flags_after, 0x0008_0003);
        assert_eq!(
            sim.vic_leaders.slots[0].leader_flags as u32,
            sim.step8.leaders[0].flags
        );

        let prepared = sim.prepare_unit_think_leader_pending(source()).unwrap();
        sim.step8.leaders[0].flags ^= 0x100;
        assert!(matches!(
            sim.commit_unit_think_leader_pending(source(), prepared),
            Err(UnitThinkLeaderPendingError::StaleBeforeImage { .. })
        ));
        assert_eq!(sim.vic_leaders.slots[0].leader_flags as u32, 0x0008_0003);

        sim.step8.leaders[0].flags ^= 0x100;
        let prepared = sim.prepare_unit_think_leader_pending(source()).unwrap();
        sim.step8.leaders[0].ai.flags2 |= victory_score::leader_flag2::UNIT_AI_OFF as u32;
        assert!(matches!(
            sim.commit_unit_think_leader_pending(source(), prepared),
            Err(UnitThinkLeaderPendingError::StaleBeforeImage { .. })
        ));
        assert_eq!(sim.vic_leaders.slots[0].leader_flags as u32, 0x0008_0003);
    }
}
