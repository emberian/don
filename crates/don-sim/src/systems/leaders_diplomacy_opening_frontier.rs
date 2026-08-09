// SPDX-License-Identifier: GPL-3.0-or-later

//! Source-only reconstruction of the opening cone of tick step 11's
//! `Leader::diplomacy` (`0x006BC950`).
//!
//! The full PDB procedure is 20,348 bytes. This frontier owns the retail opening blocks from
//! `0x006BC96E` through the first unowned instruction at `0x006BCB2C`, plus the physically
//! late target-loop continuation at `0x006BE98A`: owner/mode gates, the eight-record ally and
//! score scan, target eligibility, the exact power-of-two cadence, and the first local
//! agenda-bit mutation.
//! The next instruction enters resource, city, map, chat, and command policy. Reaching it is
//! therefore a typed residual rather than permission to invent those product-owned facts.

pub const LEADER_DIPLOMACY_VA: u32 = 0x006b_c950;
pub const LEADER_DIPLOMACY_PDB_SIZE: u32 = 20_348;
pub const OPENING_BEGIN_VA: u32 = 0x006b_c96e;
pub const OPENING_END_VA: u32 = 0x006b_cb2c;
pub const DOWNSTREAM_POLICY_VA: u32 = OPENING_END_VA;
pub const TARGET_LOOP_CONTINUE_VA: u32 = 0x006b_e98a;
pub const FUNCTION_RETURN_VA: u32 = 0x006b_e99a;
pub const LEADER_IS_ALLY_VA: u32 = 0x006e_db50;
pub const LEADERS_BASE_VA: u32 = 0x00e3_a390;
pub const LEADER_STRIDE: u32 = 0x6eec;
pub const RETAIL_LEADER_SLOTS: usize = 8;

pub const LEADER_FLAGS_OFFSET: u32 = 0x000;
pub const LEADER_WHO_OFFSET: u32 = 0x008;
pub const LEADER_SCORE_OFFSET: u32 = 0x018;
pub const LEADER_DIPLOS_OFFSET: u32 = 0x074;
pub const LEADER_TREATIES_OFFSET: u32 = 0x094;
pub const LEADER_AGENDAS_OFFSET: u32 = 0x0b4;
pub const LEADER_COUNTEROFFER_OFFSET: u32 = 0x314;
pub const LEADER_TRIBUTE_DEMANDED_OFFSET: u32 = 0x334;
pub const LEADER_DIP_OFFSET: u32 = 0x692c;
pub const DIPLOMACY_STRIDE: u32 = 0x5c;
pub const DIPLOMACY_AGREE_OFFSET: u32 = 0;
pub const GAME_FRAME_OFFSET: u32 = 0x550;

pub const LEADER_VALID: u32 = 0x1;
pub const LEADER_ACTIVE: u32 = 0x2;
pub const LEADER_HUMAN: u32 = 0x4;
pub const LEADER_PLAYING: u32 = LEADER_VALID | LEADER_ACTIVE;
pub const DIPLO_ALLIED: i32 = 2;
pub const TREATY_ELIGIBLE: i32 = 0x1;
pub const TREATY_BLOCKED: i32 = 0x4;
pub const AGENDA_PENDING: i32 = 0x4;
pub const AGENDA_SLOW_CADENCE: i32 = 0x8;

pub const NORMAL_PERIOD: u32 = 0x800;
pub const TRIBUTE_PERIOD: u32 = 0x2000;

#[inline]
pub const fn leader_va(index: usize) -> u32 {
    LEADERS_BASE_VA + index as u32 * LEADER_STRIDE
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GlobalLeaderSlot {
    pub flags: u32,
    pub who: i32,
    pub score: i32,
    pub diplos: [i32; RETAIL_LEADER_SLOTS],
}

/// State read through the `this` pointer. Retail deliberately obtains the initial human
/// gate and owner score through `leaders[this->who]`, not through this record.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DiplomacyOwnerState {
    pub who: i32,
    pub treaties: [i32; RETAIL_LEADER_SLOTS],
    pub agendas: [i32; RETAIL_LEADER_SLOTS],
    pub counteroffer: [i32; RETAIL_LEADER_SLOTS],
    pub tribute_demanded: [i32; RETAIL_LEADER_SLOTS],
    pub dip_agree: [i32; RETAIL_LEADER_SLOTS],
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DiplomacyOpeningState {
    pub owner: DiplomacyOwnerState,
    pub leaders: [GlobalLeaderSlot; RETAIL_LEADER_SLOTS],
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GameFacts {
    pub frame: u32,
    /// `game[0x821] & 2`, semaphore bit 9.
    pub check_victory_mode: bool,
    /// `*GameAccess::ai_off` at preferred VA `0x00C061C4`.
    pub ai_off: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GateOutcome {
    HumanOwner,
    CheckVictoryMode,
    AiDisabled,
    Entered,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AllyReason {
    SameWho,
    MutualAllied {
        forward: i32,
        reciprocal: i32,
    },
    NotMutual {
        forward: i32,
        reciprocal: Option<i32>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AllyReadReceipt {
    pub sequence: u8,
    pub call_va: u32,
    pub subject_index: usize,
    pub subject_who: i32,
    pub other_who: i32,
    pub result: bool,
    pub reason: AllyReason,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanReceipt {
    pub ally_reads: Vec<AllyReadReceipt>,
    pub non_allies: u8,
    pub stronger_than_owner: u8,
    /// Retail initializes the maximum to zero and replaces on `score >= maximum`, so the
    /// later record wins a non-negative tie and all-negative sets retain slot `-1`.
    pub strongest_score: i32,
    pub strongest_slot: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetSkip {
    ProcessFlagClear,
    OwnerSlot,
    TreatyInactive,
    TreatyBlocked,
    CadenceMiss { period: u32, phase_word: u32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolicyTrigger {
    PendingAgenda,
    CadenceDue { period: u32, phase_word: u32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetOutcome {
    Skipped(TargetSkip),
    ReachedPolicy {
        trigger: PolicyTrigger,
        agenda_before: i32,
        agenda_after: i32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TargetVisit {
    pub target_index: usize,
    pub target_va: u32,
    pub outcome: TargetOutcome,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AgendaMutationReceipt {
    pub instruction_va: u32,
    pub target_index: usize,
    pub before: i32,
    pub after: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenResidual {
    /// The opening mutation is complete, but continuing requires the unrecovered remainder
    /// of the 20,348-byte AI policy body.
    DownstreamPolicy {
        target_index: usize,
        first_unowned_va: u32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiplomacyOpeningTrace {
    pub gate: GateOutcome,
    pub scan: Option<ScanReceipt>,
    pub targets: Vec<TargetVisit>,
    pub local_mutations: Vec<AgendaMutationReceipt>,
    pub residual: Option<OpenResidual>,
}

impl DiplomacyOpeningTrace {
    fn gated(gate: GateOutcome) -> Self {
        Self {
            gate,
            scan: None,
            targets: Vec::new(),
            local_mutations: Vec::new(),
            residual: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpeningError {
    OwnerWhoOutsideArray { who: i32 },
    ActiveLeaderWhoOutsideArray { leader_index: usize, who: i32 },
}

fn array_index(who: i32) -> Option<usize> {
    usize::try_from(who)
        .ok()
        .filter(|index| *index < RETAIL_LEADER_SLOTS)
}

fn read_is_ally(
    state: &DiplomacyOpeningState,
    subject_index: usize,
    other_who: i32,
    sequence: u8,
) -> Result<AllyReadReceipt, OpeningError> {
    let subject = state.leaders[subject_index];
    if subject.who == other_who {
        return Ok(AllyReadReceipt {
            sequence,
            call_va: LEADER_IS_ALLY_VA,
            subject_index,
            subject_who: subject.who,
            other_who,
            result: true,
            reason: AllyReason::SameWho,
        });
    }

    let other_index =
        array_index(other_who).ok_or(OpeningError::OwnerWhoOutsideArray { who: other_who })?;
    let forward = subject.diplos[other_index];
    let reciprocal = if forward == DIPLO_ALLIED {
        // `0x006EDB66` short-circuits before the reciprocal array index. Preserve that
        // ownership: an impossible subject identity is only a missing fact when retail
        // would actually use it.
        let subject_who =
            array_index(subject.who).ok_or(OpeningError::ActiveLeaderWhoOutsideArray {
                leader_index: subject_index,
                who: subject.who,
            })?;
        Some(state.leaders[other_index].diplos[subject_who])
    } else {
        None
    };
    let result = forward == DIPLO_ALLIED && reciprocal == Some(DIPLO_ALLIED);
    let reason = if result {
        AllyReason::MutualAllied {
            forward,
            reciprocal: reciprocal.unwrap(),
        }
    } else {
        AllyReason::NotMutual {
            forward,
            reciprocal,
        }
    };
    Ok(AllyReadReceipt {
        sequence,
        call_va: LEADER_IS_ALLY_VA,
        subject_index,
        subject_who: subject.who,
        other_who,
        result,
        reason,
    })
}

fn scan_leaders(
    state: &DiplomacyOpeningState,
    owner_index: usize,
) -> Result<ScanReceipt, OpeningError> {
    let owner_who = state.owner.who;
    let owner_score = state.leaders[owner_index].score;
    let mut receipt = ScanReceipt {
        ally_reads: Vec::new(),
        non_allies: 0,
        stronger_than_owner: 0,
        strongest_score: 0,
        strongest_slot: -1,
    };

    for subject_index in 0..RETAIL_LEADER_SLOTS {
        let subject = state.leaders[subject_index];
        if subject.flags & LEADER_PLAYING != LEADER_PLAYING {
            continue;
        }
        let ally = read_is_ally(
            state,
            subject_index,
            owner_who,
            receipt.ally_reads.len() as u8,
        )?;
        if !ally.result {
            receipt.non_allies = receipt.non_allies.wrapping_add(1);
        }
        receipt.ally_reads.push(ally);
        if subject.score > owner_score {
            receipt.stronger_than_owner = receipt.stronger_than_owner.wrapping_add(1);
        }
        if subject.score >= receipt.strongest_score {
            receipt.strongest_score = subject.score;
            receipt.strongest_slot = subject_index as i32;
        }
    }
    Ok(receipt)
}

/// The exact period selection at `0x006BCAB7..0x006BCAF0`.
pub fn target_period(agree: i32, tribute_demanded: i32, counteroffer: i32, agenda: i32) -> u32 {
    let mut period = NORMAL_PERIOD;
    if agree != 1 {
        if tribute_demanded != 0 {
            period = TRIBUTE_PERIOD;
        }
        if counteroffer != 0 {
            period = period.wrapping_mul(2);
        }
    }
    if agenda & AGENDA_SLOW_CADENCE == 0 {
        period >>= 2;
    }
    period
}

/// Retail adds with wrapping dword arithmetic, then masks by `period - 1`. Every reachable
/// period is a power of two, so no division or signed remainder occurs here.
pub fn target_phase_word(frame: u32, owner_who: usize, target_index: usize, period: u32) -> u32 {
    let pair_index = target_index as u32 + 8 * owner_who as u32;
    frame.wrapping_add(pair_index.wrapping_mul(period >> 6))
}

/// Execute only the recovered opening cone. All fallible owner/index reads complete before
/// the first possible store, so an [`OpeningError`] leaves `state` byte-for-byte unchanged.
pub fn execute_diplomacy_opening(
    state: &mut DiplomacyOpeningState,
    game: GameFacts,
) -> Result<DiplomacyOpeningTrace, OpeningError> {
    let owner_index = array_index(state.owner.who).ok_or(OpeningError::OwnerWhoOutsideArray {
        who: state.owner.who,
    })?;

    // `0x006BC99A` reads `leaders[this->who].leader_flags`, not `this->leader_flags`.
    if state.leaders[owner_index].flags & LEADER_HUMAN != 0 {
        return Ok(DiplomacyOpeningTrace::gated(GateOutcome::HumanOwner));
    }
    if game.check_victory_mode {
        return Ok(DiplomacyOpeningTrace::gated(GateOutcome::CheckVictoryMode));
    }
    if game.ai_off {
        return Ok(DiplomacyOpeningTrace::gated(GateOutcome::AiDisabled));
    }

    let scan = scan_leaders(state, owner_index)?;
    let mut trace = DiplomacyOpeningTrace {
        gate: GateOutcome::Entered,
        scan: Some(scan),
        targets: Vec::new(),
        local_mutations: Vec::new(),
        residual: None,
    };

    for target_index in 0..RETAIL_LEADER_SLOTS {
        let skip = if state.leaders[target_index].flags & LEADER_ACTIVE == 0 {
            Some(TargetSkip::ProcessFlagClear)
        } else if target_index == owner_index {
            Some(TargetSkip::OwnerSlot)
        } else if state.owner.treaties[target_index] & TREATY_ELIGIBLE == 0 {
            Some(TargetSkip::TreatyInactive)
        } else if state.owner.treaties[target_index] & TREATY_BLOCKED != 0 {
            Some(TargetSkip::TreatyBlocked)
        } else {
            None
        };
        if let Some(skip) = skip {
            trace.targets.push(TargetVisit {
                target_index,
                target_va: leader_va(target_index),
                outcome: TargetOutcome::Skipped(skip),
            });
            continue;
        }

        let agenda_before = state.owner.agendas[target_index];
        let trigger = if agenda_before & AGENDA_PENDING != 0 {
            PolicyTrigger::PendingAgenda
        } else {
            let period = target_period(
                state.owner.dip_agree[target_index],
                state.owner.tribute_demanded[target_index],
                state.owner.counteroffer[target_index],
                agenda_before,
            );
            let phase_word = target_phase_word(game.frame, owner_index, target_index, period);
            if phase_word & period.wrapping_sub(1) != 0 {
                trace.targets.push(TargetVisit {
                    target_index,
                    target_va: leader_va(target_index),
                    outcome: TargetOutcome::Skipped(TargetSkip::CadenceMiss { period, phase_word }),
                });
                continue;
            }
            PolicyTrigger::CadenceDue { period, phase_word }
        };

        let agenda_after = agenda_before & !AGENDA_PENDING;
        state.owner.agendas[target_index] = agenda_after;
        trace.targets.push(TargetVisit {
            target_index,
            target_va: leader_va(target_index),
            outcome: TargetOutcome::ReachedPolicy {
                trigger,
                agenda_before,
                agenda_after,
            },
        });
        trace.local_mutations.push(AgendaMutationReceipt {
            instruction_va: 0x006b_cb2a,
            target_index,
            before: agenda_before,
            after: agenda_after,
        });
        trace.residual = Some(OpenResidual::DownstreamPolicy {
            target_index,
            first_unowned_va: DOWNSTREAM_POLICY_VA,
        });
        break;
    }

    Ok(trace)
}
