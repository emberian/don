// SPDX-License-Identifier: GPL-3.0-or-later
//! Complete detached `LeaderData::get_target` runtime.
//!
//! Retail `LeaderData::get_target` is the 90-byte leaf at
//! `0x006DA000..0x006DA059`. It reads `LeaderData::who` (`+0x08`),
//! `Game::start_index[who]` (`Game +0x67C`), `Game::start_list[8]`
//! (`Game +0x65C`), and the candidate `LeaderData::leader_flags` dword at the
//! head of each `0x6EEC`-byte Leader row. It has no calls, writes, or RNG.
//!
//! The scan begins at `(start_index[who] + 1) % 8`, visits exactly eight
//! positions using signed 32-bit C remainder, and returns the first candidate
//! whose `(leader_flags & 3) == 3`. Eight misses return the calling leader's
//! `who`; `start_index` is not advanced. Detached indices which would make
//! retail read outside either fixed array are refused instead of reproducing
//! unchecked pointer arithmetic.

#![forbid(unsafe_code)]

pub const LEADER_GET_TARGET_VA: u32 = 0x006d_a000;
pub const LEADER_GET_TARGET_END_VA: u32 = 0x006d_a05a;
pub const GAME_START_LIST_OFFSET: u32 = 0x065c;
pub const GAME_START_INDEX_OFFSET: u32 = 0x067c;
pub const LEADER_WHO_OFFSET: u32 = 0x0008;
pub const LEADER_FLAGS_OFFSET: u32 = 0x0000;
pub const LEADER_STRIDE: u32 = 0x6eec;
pub const LEADER_COUNT: usize = 8;
pub const ACTIVE_LEADER_MASK: i32 = 0x0003;

/// The two `LeaderData` fields touched by this leaf across the fixed Leader table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeaderTargetRow {
    pub leader_flags: i32,
    pub who: i32,
}

/// The exact two fixed Game arrays read by `LeaderData::get_target`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeaderTargetGame {
    pub start_list: [i32; LEADER_COUNT],
    pub start_index: [i32; LEADER_COUNT],
}

/// Complete detached input image for the leaf.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeaderTargetContext {
    pub leaders: [LeaderTargetRow; LEADER_COUNT],
    pub game: LeaderTargetGame,
}

/// One candidate read in retail order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeaderTargetProbe {
    pub probe: u8,
    pub start_list_index: u8,
    pub candidate: u8,
    pub candidate_flags: i32,
    pub active: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeaderTargetExit {
    ActiveCandidate,
    CallingLeaderFallback,
}

/// Exact read receipt. Unvisited rows remain `None` after the first active candidate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeaderTargetReceipt {
    pub function_va: u32,
    pub leader_slot: u8,
    pub leader_who: i32,
    pub start_index: i32,
    pub probes: [Option<LeaderTargetProbe>; LEADER_COUNT],
    pub probe_count: u8,
    pub target: i32,
    pub exit: LeaderTargetExit,
    pub writes: u8,
    pub rng_draws: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeaderTargetError {
    InvalidLeaderSlot(usize),
    InvalidLeaderWho {
        leader_slot: usize,
        who: i32,
    },
    InvalidStartListIndex {
        leader_slot: usize,
        who: i32,
        probe: u8,
        index: i32,
    },
    InvalidCandidate {
        leader_slot: usize,
        probe: u8,
        candidate: i32,
    },
}

/// Execute the complete source-owned `LeaderData::get_target` leaf.
pub fn get_target(
    context: &LeaderTargetContext,
    leader_slot: usize,
) -> Result<LeaderTargetReceipt, LeaderTargetError> {
    let leader = context
        .leaders
        .get(leader_slot)
        .ok_or(LeaderTargetError::InvalidLeaderSlot(leader_slot))?;
    let who_index = usize::try_from(leader.who)
        .ok()
        .filter(|&who| who < LEADER_COUNT)
        .ok_or(LeaderTargetError::InvalidLeaderWho {
            leader_slot,
            who: leader.who,
        })?;
    let start_index = context.game.start_index[who_index];
    let mut probes = [None; LEADER_COUNT];

    for probe in 0..LEADER_COUNT {
        // `inc edi; lea eax,[edi+edx]` are wrapping i32 operations. The following
        // mask/fixup sequence implements signed C remainder by eight.
        let rotated = start_index.wrapping_add(1).wrapping_add(probe as i32) % LEADER_COUNT as i32;
        let start_list_index = usize::try_from(rotated)
            .ok()
            .filter(|&index| index < LEADER_COUNT)
            .ok_or(LeaderTargetError::InvalidStartListIndex {
                leader_slot,
                who: leader.who,
                probe: probe as u8,
                index: rotated,
            })?;
        let candidate = context.game.start_list[start_list_index];
        let candidate_index = usize::try_from(candidate)
            .ok()
            .filter(|&index| index < LEADER_COUNT)
            .ok_or(LeaderTargetError::InvalidCandidate {
                leader_slot,
                probe: probe as u8,
                candidate,
            })?;
        let candidate_flags = context.leaders[candidate_index].leader_flags;
        let active = candidate_flags & ACTIVE_LEADER_MASK == ACTIVE_LEADER_MASK;
        probes[probe] = Some(LeaderTargetProbe {
            probe: probe as u8,
            start_list_index: start_list_index as u8,
            candidate: candidate_index as u8,
            candidate_flags,
            active,
        });
        if active {
            return Ok(LeaderTargetReceipt {
                function_va: LEADER_GET_TARGET_VA,
                leader_slot: leader_slot as u8,
                leader_who: leader.who,
                start_index,
                probes,
                probe_count: probe as u8 + 1,
                target: candidate,
                exit: LeaderTargetExit::ActiveCandidate,
                writes: 0,
                rng_draws: 0,
            });
        }
    }

    Ok(LeaderTargetReceipt {
        function_va: LEADER_GET_TARGET_VA,
        leader_slot: leader_slot as u8,
        leader_who: leader.who,
        start_index,
        probes,
        probe_count: LEADER_COUNT as u8,
        target: leader.who,
        exit: LeaderTargetExit::CallingLeaderFallback,
        writes: 0,
        rng_draws: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> LeaderTargetContext {
        LeaderTargetContext {
            leaders: std::array::from_fn(|slot| LeaderTargetRow {
                leader_flags: 0,
                who: slot as i32,
            }),
            game: LeaderTargetGame {
                start_list: [0, 1, 2, 3, 4, 5, 6, 7],
                start_index: [0; LEADER_COUNT],
            },
        }
    }

    #[test]
    fn rotated_scan_returns_first_active_candidate_without_writes_or_rng() {
        let mut input = context();
        input.game.start_index[3] = 5;
        input.leaders[7].leader_flags = ACTIVE_LEADER_MASK;
        input.leaders[0].leader_flags = ACTIVE_LEADER_MASK;

        let receipt = get_target(&input, 3).unwrap();
        assert_eq!(receipt.target, 7);
        assert_eq!(receipt.exit, LeaderTargetExit::ActiveCandidate);
        assert_eq!(receipt.probe_count, 2);
        assert_eq!(receipt.probes[0].unwrap().candidate, 6);
        assert_eq!(receipt.probes[1].unwrap().candidate, 7);
        assert_eq!((receipt.writes, receipt.rng_draws), (0, 0));
        assert!(receipt.probes[2..].iter().all(Option::is_none));
    }

    #[test]
    fn eight_misses_return_the_calling_rows_who_without_advancing_start_index() {
        let mut input = context();
        input.game.start_index[5] = 6;
        let before = input.game.start_index;

        let receipt = get_target(&input, 5).unwrap();
        assert_eq!(receipt.target, 5);
        assert_eq!(receipt.exit, LeaderTargetExit::CallingLeaderFallback);
        assert_eq!(receipt.probe_count, 8);
        assert_eq!(input.game.start_index, before);
        assert_eq!(
            receipt
                .probes
                .map(|probe| probe.expect("all probes executed").candidate),
            [7, 0, 1, 2, 3, 4, 5, 6]
        );
    }

    #[test]
    fn unchecked_retail_array_domains_fail_closed() {
        let mut input = context();
        input.leaders[2].who = -1;
        assert_eq!(
            get_target(&input, 2),
            Err(LeaderTargetError::InvalidLeaderWho {
                leader_slot: 2,
                who: -1,
            })
        );

        input.leaders[2].who = 2;
        input.game.start_index[2] = -2;
        assert_eq!(
            get_target(&input, 2),
            Err(LeaderTargetError::InvalidStartListIndex {
                leader_slot: 2,
                who: 2,
                probe: 0,
                index: -1,
            })
        );

        input.game.start_index[2] = 0;
        input.game.start_list[1] = 8;
        assert_eq!(
            get_target(&input, 2),
            Err(LeaderTargetError::InvalidCandidate {
                leader_slot: 2,
                probe: 0,
                candidate: 8,
            })
        );
    }
}
