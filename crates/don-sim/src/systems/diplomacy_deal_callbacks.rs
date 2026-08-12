// SPDX-License-Identifier: GPL-3.0-or-later
//! Exact simulation/presentation split for `Leader::consider_tribute` and `notify_deal`.
//!
//! These are the two ordinary-accept callbacks which remained outside the complete
//! `Leader::action_respond(target, 1)` transaction.  Both bodies are source-complete against the
//! shipped PE (`30478a44…625079`) and PDB.  `consider_tribute` owns two persistent frame stamps;
//! `notify_deal` is presentation-only and is therefore retained as an ordered envelope rather
//! than being allowed to block lockstep publication.

use super::leader_set_diplo::{DIPLO_SLOTS, NUM_GOODS};

pub const CONSIDER_TRIBUTE_BEGIN_VA: u32 = 0x006d_1360;
pub const CONSIDER_TRIBUTE_END_VA: u32 = 0x006d_13e4;
pub const NOTIFY_DEAL_BEGIN_VA: u32 = 0x006d_13f0;
pub const NOTIFY_DEAL_END_VA: u32 = 0x006d_14cf;

/// The `LeaderData::econ[6]` flag tested at `+0x450 + good*4`.
pub const ECON_TRIBUTE_RELEVANT: i32 = 4;
/// Encoded resource values are XOR-decoded before the `< 3000` gate.
pub const RESOURCE_XOR: i32 = 0x8221;
pub const RESOURCE_UPPER_EXCLUSIVE: i32 = 3_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DealCallbackLeaderState {
    /// `LeaderData+0x174 + other*4`, PDB `tribute_stamp[8]`.
    pub tribute_stamp: [i32; DIPLO_SLOTS],
    /// `LeaderData+0x194 + other*4`, PDB `gift_stamp[8]`.
    pub gift_stamp: [i32; DIPLO_SLOTS],
}

impl Default for DealCallbackLeaderState {
    fn default() -> Self {
        Self {
            tribute_stamp: [0; DIPLO_SLOTS],
            gift_stamp: [0; DIPLO_SLOTS],
        }
    }
}

/// Canonical mutable fields read/written by the two callbacks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DealCallbackImage {
    pub leaders: [DealCallbackLeaderState; DIPLO_SLOTS],
    /// `Game::frame`, `Game+0x550`.
    pub frame: i32,
    /// `Console+0x298`; `-1` remains a valid no-local-player value.
    pub local_who: i32,
}

impl Default for DealCallbackImage {
    fn default() -> Self {
        Self {
            leaders: std::array::from_fn(|_| DealCallbackLeaderState::default()),
            frame: 0,
            local_who: -1,
        }
    }
}

/// Immutable facts `consider_tribute` reads outside its two retained stamp rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DealCallbackFacts {
    /// `LeaderData+0x00 & 4` for the receiver.
    pub receiver_flag_four: [Option<bool>; DIPLO_SLOTS],
    /// `leaders[sender].blacken` at `LeaderData+0x20c`, multiplied by 100.
    pub sender_blacken: [Option<i32>; DIPLO_SLOTS],
    /// Decoded authoritative resource buckets for the receiver, after accepted movement.
    pub receiver_resources: [[Option<i32>; NUM_GOODS]; DIPLO_SLOTS],
    /// `LeaderData+0x450`: PDB `econ[6]`, an AI bookkeeping/flag row, not the stockpile.
    pub receiver_econ: [[Option<i32>; NUM_GOODS]; DIPLO_SLOTS],
}

impl Default for DealCallbackFacts {
    fn default() -> Self {
        Self {
            receiver_flag_four: [None; DIPLO_SLOTS],
            sender_blacken: [None; DIPLO_SLOTS],
            receiver_resources: [[None; NUM_GOODS]; DIPLO_SLOTS],
            receiver_econ: [[None; NUM_GOODS]; DIPLO_SLOTS],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DealCallbackError {
    LeaderOutOfRange { value: usize },
    GoodOutOfRange { value: usize },
    MissingReceiverFlagFour { receiver: usize },
    MissingSenderBlacken { sender: usize },
    MissingReceiverResource { receiver: usize, good: usize },
    MissingReceiverEcon { receiver: usize, good: usize },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConsiderTributeRequest {
    /// `this`.
    pub receiver: usize,
    /// First stack argument; the remote party whose stamps are written.
    pub sender: usize,
    /// Second stack argument, the raw transferred tribute before scale.
    pub raw: i32,
    /// Third stack argument.
    pub good: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GiftStampDecision {
    SkippedNonPositive,
    ReceiverFlagFour,
    ThresholdAndEconomyGoodTwo,
    ThresholdAndEconomyFlag,
    ThresholdTooSmall,
    ReceiverResourceAtLeastThreeThousand,
    EconomyFlagAbsent,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsiderTributePlan {
    pub request: ConsiderTributeRequest,
    pub before: DealCallbackImage,
    pub after: DealCallbackImage,
    /// Every positive call writes `sender.tribute_stamp[receiver] = frame`, before all gates.
    pub wrote_sender_tribute_stamp: bool,
    pub gift_stamp: GiftStampDecision,
}

fn leader(value: usize) -> Result<(), DealCallbackError> {
    if value < DIPLO_SLOTS {
        Ok(())
    } else {
        Err(DealCallbackError::LeaderOutOfRange { value })
    }
}

/// `Leader::consider_tribute(int who, int value, int good)` `0x006D1360`, all 132 bytes.
///
/// The first write is easy to transpose in a high-level port: the receiver calls the body, but a
/// positive tribute stamps the **sender's** `tribute_stamp[receiver]`.  Only the conditional tail
/// writes the receiver's reciprocal `gift_stamp[sender]`.
pub fn plan_consider_tribute(
    before: &DealCallbackImage,
    facts: &DealCallbackFacts,
    request: ConsiderTributeRequest,
) -> Result<ConsiderTributePlan, DealCallbackError> {
    leader(request.receiver)?;
    leader(request.sender)?;
    if request.good >= NUM_GOODS {
        return Err(DealCallbackError::GoodOutOfRange {
            value: request.good,
        });
    }
    let mut after = before.clone();
    if request.raw <= 0 {
        return Ok(ConsiderTributePlan {
            request,
            before: before.clone(),
            after,
            wrote_sender_tribute_stamp: false,
            gift_stamp: GiftStampDecision::SkippedNonPositive,
        });
    }

    after.leaders[request.sender].tribute_stamp[request.receiver] = before.frame;
    let flag_four = facts.receiver_flag_four[request.receiver].ok_or(
        DealCallbackError::MissingReceiverFlagFour {
            receiver: request.receiver,
        },
    )?;
    let gift_stamp = if flag_four {
        after.leaders[request.receiver].gift_stamp[request.sender] = before.frame;
        GiftStampDecision::ReceiverFlagFour
    } else {
        let blacken = facts.sender_blacken[request.sender].ok_or(
            DealCallbackError::MissingSenderBlacken {
                sender: request.sender,
            },
        )?;
        if request.raw < blacken.wrapping_mul(100) {
            GiftStampDecision::ThresholdTooSmall
        } else {
            let resource = facts.receiver_resources[request.receiver][request.good].ok_or(
                DealCallbackError::MissingReceiverResource {
                    receiver: request.receiver,
                    good: request.good,
                },
            )?;
            if resource >= RESOURCE_UPPER_EXCLUSIVE {
                GiftStampDecision::ReceiverResourceAtLeastThreeThousand
            } else if request.good == 2 {
                after.leaders[request.receiver].gift_stamp[request.sender] = before.frame;
                GiftStampDecision::ThresholdAndEconomyGoodTwo
            } else {
                let econ = facts.receiver_econ[request.receiver][request.good].ok_or(
                    DealCallbackError::MissingReceiverEcon {
                        receiver: request.receiver,
                        good: request.good,
                    },
                )?;
                if econ & ECON_TRIBUTE_RELEVANT != 0 {
                    after.leaders[request.receiver].gift_stamp[request.sender] = before.frame;
                    GiftStampDecision::ThresholdAndEconomyFlag
                } else {
                    GiftStampDecision::EconomyFlagAbsent
                }
            }
        }
    };
    Ok(ConsiderTributePlan {
        request,
        before: before.clone(),
        after,
        wrote_sender_tribute_stamp: true,
        gift_stamp,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotifyDealRequest {
    /// `this`, which gates the whole body against the console local player.
    pub leader: usize,
    /// First argument, appended through `leaders[other].get_name()`.
    pub other: usize,
    /// Second argument: one selects peace text, two alliance text, all others the third text.
    pub treaty: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotifyDealText {
    Peace,
    Alliance,
    Other,
}

/// Presentation-only receipt for the whole 223-byte body. The renderer supplies localized
/// strings and the other leader's name; lockstep retains only the exact semantic selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotifyDealEnvelope {
    pub leader: usize,
    pub other: usize,
    pub text: NotifyDealText,
    /// `Leader::chat_to_local(..., this->who, 0, 0)`.
    pub chat_sender: usize,
    /// `SoundGlobal::play(3)`.
    pub sound_category: i32,
}

/// `Leader::notify_deal(int who, int treaty)` `0x006D13F0`, all 223 bytes.
pub fn plan_notify_deal(
    image: &DealCallbackImage,
    request: NotifyDealRequest,
) -> Result<Option<NotifyDealEnvelope>, DealCallbackError> {
    leader(request.leader)?;
    leader(request.other)?;
    if image.local_who != request.leader as i32 {
        return Ok(None);
    }
    let text = match request.treaty {
        1 => NotifyDealText::Peace,
        2 => NotifyDealText::Alliance,
        _ => NotifyDealText::Other,
    };
    Ok(Some(NotifyDealEnvelope {
        leader: request.leader,
        other: request.other,
        text,
        chat_sender: request.leader,
        sound_category: 3,
    }))
}
