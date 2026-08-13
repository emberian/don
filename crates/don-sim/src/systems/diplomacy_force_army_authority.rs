// SPDX-License-Identifier: GPL-3.0-or-later
//! Atomic exact early-return arm of diplomacy-forced `Army::process(1)`.
//!
//! `Leader::set_diplo` calls `Army::process(1)` for every valid Army when the owner passes
//! the ordinary Army gates.  The complete general body is not mounted.  One substantive arm
//! is authority-complete: `leader_flags & 0x40` still reaches the entry decrement of
//! `human_frame`, then returns before normalize or any Group, Unit, City, AI, or RNG access.
//!
//! Retail evidence is the shipped PE `30478a44…625079`: `Armies::diplo_change`
//! `0x006F30F0..0x006F3159` (105 bytes, SHA-256 `8191743c…f3eb6`) performs the owner gates,
//! scans valid slots in ascending order, and calls `Army::process(1)`. `Army::process`
//! `0x006F93D0..0x006F9836` (1,138 bytes, SHA-256 `81f684bf…446d3`) decrements the non-zero
//! `human_frame` at `0x006F93DA..0x006F93E5`; the forced path jumps to `0x006F94AA`, tests
//! leader bit `0x40` at `0x006F94B7`, and returns at `0x006F983A` without a deeper call.

use super::armies::{
    Armies, ArmyData, LF2_SKIP_MASK, LF_ACTIVE, LF_ARMIES_OFF, LF_KIND_MASK, LF_KIND_SKIP,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ForceArmyProcessRequest {
    pub owner: usize,
    pub army_slot: usize,
    pub forced: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForceArmyProcessReceipt {
    pub request: ForceArmyProcessRequest,
    pub leader_flags: u32,
    pub leader_flags2: u32,
    pub before: ArmyData,
    pub after: ArmyData,
}

impl ForceArmyProcessReceipt {
    pub fn validates(&self) -> bool {
        if self.request.forced != 1
            || self.leader_flags & LF_ACTIVE == 0
            || self.leader_flags & LF_KIND_MASK == LF_KIND_SKIP
            || self.leader_flags & LF_ARMIES_OFF == 0
            || self.leader_flags2 & LF2_SKIP_MASK != 0
            || self.before.valid == 0
            || usize::try_from(self.before.army).ok() != Some(self.request.army_slot)
            || usize::try_from(self.before.who).ok() != Some(self.request.owner)
        {
            return false;
        }
        let mut expected = self.before.clone();
        if expected.human_frame != 0 {
            expected.human_frame = expected.human_frame.wrapping_sub(1);
        }
        self.after == expected
    }
}

#[derive(Clone, Debug)]
pub struct PreparedForceArmyProcess {
    before: Armies,
    after: Armies,
    leader_flags: [u32; 8],
    leader_flags2: [u32; 8],
    receipts: Vec<ForceArmyProcessReceipt>,
}

impl PreparedForceArmyProcess {
    pub fn validates(&self) -> bool {
        !self.receipts.is_empty()
            && self.receipts.iter().all(ForceArmyProcessReceipt::validates)
            && !self.receipts.iter().enumerate().any(|(index, receipt)| {
                self.receipts[..index].iter().any(|earlier| {
                    earlier.request.owner == receipt.request.owner
                        && earlier.request.army_slot == receipt.request.army_slot
                })
            })
            && self.receipts.iter().all(|receipt| {
                self.leader_flags[receipt.request.owner] == receipt.leader_flags
                    && self.leader_flags2[receipt.request.owner] == receipt.leader_flags2
                    && self.before.lists[receipt.request.owner][receipt.request.army_slot]
                        == receipt.before
                    && self.after.lists[receipt.request.owner][receipt.request.army_slot]
                        == receipt.after
            })
    }

    pub fn is_current(
        &self,
        armies: &Armies,
        leader_flags: &[u32; 8],
        leader_flags2: &[u32; 8],
    ) -> bool {
        self.receipts.iter().all(|receipt| {
            leader_flags[receipt.request.owner] == receipt.leader_flags
                && leader_flags2[receipt.request.owner] == receipt.leader_flags2
                && armies
                    .lists
                    .get(receipt.request.owner)
                    .and_then(|list| list.get(receipt.request.army_slot))
                    == Some(&receipt.before)
        })
    }

    pub fn receipts(&self) -> &[ForceArmyProcessReceipt] {
        &self.receipts
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForceArmyProcessError {
    EmptyRequest,
    OwnerOutOfRange(usize),
    ArmySlotOutOfRange { owner: usize, army_slot: usize },
    UnsupportedForcedArgument(i32),
    OwnerGate { owner: usize },
    RequiresUnresolvedArmyBody { owner: usize, army_slot: usize },
    InvalidArmy { owner: usize, army_slot: usize },
    ArmyIdentity { owner: usize, army_slot: usize },
    DuplicateRequest { owner: usize, army_slot: usize },
    StaleArmy { owner: usize, army_slot: usize },
    StaleLeader { owner: usize },
    InvalidPrepared,
}

pub fn prepare_force_army_process(
    armies: &Armies,
    leader_flags: &[u32; 8],
    leader_flags2: &[u32; 8],
    requests: &[ForceArmyProcessRequest],
) -> Result<PreparedForceArmyProcess, ForceArmyProcessError> {
    if requests.is_empty() {
        return Err(ForceArmyProcessError::EmptyRequest);
    }
    if let Some((index, duplicate)) = requests.iter().enumerate().find(|(index, request)| {
        requests[..*index]
            .iter()
            .any(|earlier| earlier.owner == request.owner && earlier.army_slot == request.army_slot)
    }) {
        let _ = index;
        return Err(ForceArmyProcessError::DuplicateRequest {
            owner: duplicate.owner,
            army_slot: duplicate.army_slot,
        });
    }
    let mut after = armies.clone();
    let mut receipts = Vec::with_capacity(requests.len());
    for request in requests.iter().copied() {
        if request.owner >= leader_flags.len() {
            return Err(ForceArmyProcessError::OwnerOutOfRange(request.owner));
        }
        let Some(list) = armies.lists.get(request.owner) else {
            return Err(ForceArmyProcessError::OwnerOutOfRange(request.owner));
        };
        let Some(before) = list.get(request.army_slot).cloned() else {
            return Err(ForceArmyProcessError::ArmySlotOutOfRange {
                owner: request.owner,
                army_slot: request.army_slot,
            });
        };
        if request.forced != 1 {
            return Err(ForceArmyProcessError::UnsupportedForcedArgument(
                request.forced,
            ));
        }
        let flags = leader_flags[request.owner];
        let flags2 = leader_flags2[request.owner];
        if flags & LF_ACTIVE == 0
            || flags & LF_KIND_MASK == LF_KIND_SKIP
            || flags2 & LF2_SKIP_MASK != 0
        {
            return Err(ForceArmyProcessError::OwnerGate {
                owner: request.owner,
            });
        }
        if flags & LF_ARMIES_OFF == 0 {
            return Err(ForceArmyProcessError::RequiresUnresolvedArmyBody {
                owner: request.owner,
                army_slot: request.army_slot,
            });
        }
        if before.valid == 0 {
            return Err(ForceArmyProcessError::InvalidArmy {
                owner: request.owner,
                army_slot: request.army_slot,
            });
        }
        if usize::try_from(before.army).ok() != Some(request.army_slot)
            || usize::try_from(before.who).ok() != Some(request.owner)
        {
            return Err(ForceArmyProcessError::ArmyIdentity {
                owner: request.owner,
                army_slot: request.army_slot,
            });
        }
        let mut army_after = before.clone();
        if army_after.human_frame != 0 {
            army_after.human_frame = army_after.human_frame.wrapping_sub(1);
        }
        after.lists[request.owner][request.army_slot] = army_after.clone();
        receipts.push(ForceArmyProcessReceipt {
            request,
            leader_flags: flags,
            leader_flags2: flags2,
            before,
            after: army_after,
        });
    }
    let prepared = PreparedForceArmyProcess {
        before: armies.clone(),
        after,
        leader_flags: *leader_flags,
        leader_flags2: *leader_flags2,
        receipts,
    };
    if !prepared.validates() {
        return Err(ForceArmyProcessError::InvalidPrepared);
    }
    Ok(prepared)
}

pub fn commit_force_army_process(
    armies: &mut Armies,
    leader_flags: &[u32; 8],
    leader_flags2: &[u32; 8],
    prepared: PreparedForceArmyProcess,
) -> Result<Vec<ForceArmyProcessReceipt>, ForceArmyProcessError> {
    if !prepared.validates() {
        return Err(ForceArmyProcessError::InvalidPrepared);
    }
    if let Some(stale) = prepared.receipts.iter().find(|receipt| {
        leader_flags[receipt.request.owner] != receipt.leader_flags
            || leader_flags2[receipt.request.owner] != receipt.leader_flags2
    }) {
        return Err(ForceArmyProcessError::StaleLeader {
            owner: stale.request.owner,
        });
    }
    if let Some(stale) = prepared.receipts.iter().find(|receipt| {
        armies
            .lists
            .get(receipt.request.owner)
            .and_then(|list| list.get(receipt.request.army_slot))
            != Some(&receipt.before)
    }) {
        return Err(ForceArmyProcessError::StaleArmy {
            owner: stale.request.owner,
            army_slot: stale.request.army_slot,
        });
    }
    *armies = prepared.after;
    Ok(prepared.receipts)
}
