// SPDX-License-Identifier: GPL-3.0-or-later
//! Exact caller-null-list runtime for `Map::fill_cont` (`0x0068a960`).
//!
//! The East Meets West style passes two eight-word output arrays and a null
//! `SimpleArray<int>*`. Retail therefore constructs the unused-team list
//! locally, assigns each active team (or each unteamed leader) to a continent,
//! then randomly exchanges equal-sized continent labels. The function mutates
//! only stack/local storage and the main RNG; it does not write the World walk.

use don_sim::rng::Random;

pub const MAP_FILL_CONT_VA: u32 = 0x0068_a960;
pub const MAP_FILL_CONT_RET_VA: u32 = 0x0068_abf1;
pub const MAP_FILL_CONT_END_VA: u32 = 0x0068_abf4;
pub const MAP_FILL_CONT_EQUAL_SIZE_RNG_VA: u32 = 0x0068_ab5f;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TeamContinentPartitionError {
    NoActiveLeaders,
    InvalidTeam { slot: u8, team: u8 },
    MissingUnusedTeam { slot: u8, ordinal: usize },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TeamContinentRandomDraw {
    pub call_va: u32,
    pub left_continent: u8,
    pub right_continent: u8,
    pub state_before: i32,
    pub raw: i32,
    pub exchanged: bool,
    pub state_after: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TeamContinentPartitionReceipt {
    pub entry_va: u32,
    pub return_va: u32,
    /// Fixed leader-slot order (`0..8`), preserving holes.
    pub active_slots: Vec<u8>,
    /// Missing ordinary team ids appended in ascending order by the null-list
    /// caller path at `0x0068a9e5..0x0068aa36`.
    pub unused_teams: Vec<u8>,
    /// `param_1[8]`: player counts by continent id. Retail never exchanges
    /// this array; only the team-to-continent labels move.
    pub continent_counts: [i32; 8],
    /// `param_2[8]`: ordinary team id to continent id. The equal-size exchange
    /// loop deliberately rewrites `-1` entries as well.
    pub team_to_continent: [i32; 8],
    pub continent_count: u8,
    pub random_state_before: i32,
    pub random_state_after: i32,
    pub random_draws: Vec<TeamContinentRandomDraw>,
    /// The leaf has no World pointer and changes no checksum-walked byte.
    pub world_walked_bytes_changed: u64,
}

/// Execute the exact `MapEastMeetsWest::make_continents` call domain.
///
/// `leaders[slot]` is `None` for an inactive retail leader and `Some(team)` for
/// an active one. Team `8` is retail's unteamed sentinel. This entry models the
/// actual caller's null optional-list argument, not arbitrary external
/// `SimpleArray` allocator states.
pub fn execute_team_continent_partition(
    leaders: &[Option<u8>; 8],
    rng: &mut Random,
) -> Result<TeamContinentPartitionReceipt, TeamContinentPartitionError> {
    let mut active_slots = Vec::new();
    for (slot, team) in leaders.iter().copied().enumerate() {
        let Some(team) = team else { continue };
        if team > 8 {
            return Err(TeamContinentPartitionError::InvalidTeam {
                slot: slot as u8,
                team,
            });
        }
        active_slots.push(slot as u8);
    }
    if active_slots.is_empty() {
        return Err(TeamContinentPartitionError::NoActiveLeaders);
    }

    // The first native loop scans candidate team ids 0..7 and appends one only
    // when no active Leader reports it from LeaderData::get_team 0x006ec040.
    let mut unused_teams = Vec::new();
    for candidate in 0u8..8 {
        if !leaders.iter().flatten().any(|team| *team == candidate) {
            unused_teams.push(candidate);
        }
    }

    let mut continent_counts = [0i32; 8];
    let mut team_to_continent = [-1i32; 8];
    let mut continent_count = 0usize;
    let mut unteamed_ordinal = 0usize;
    for (slot, team) in leaders.iter().copied().enumerate() {
        let Some(team) = team else { continue };
        if team == 8 {
            let Some(&borrowed_team) = unused_teams.get(unteamed_ordinal) else {
                return Err(TeamContinentPartitionError::MissingUnusedTeam {
                    slot: slot as u8,
                    ordinal: unteamed_ordinal,
                });
            };
            team_to_continent[borrowed_team as usize] = continent_count as i32;
            continent_counts[continent_count] = 1;
            continent_count += 1;
            unteamed_ordinal += 1;
            continue;
        }

        let team_index = team as usize;
        if team_to_continent[team_index] == -1 {
            team_to_continent[team_index] = continent_count as i32;
            continent_count += 1;
        }
        let continent = team_to_continent[team_index] as usize;
        continent_counts[continent] = continent_counts[continent].wrapping_add(1);
    }

    let random_state_before = rng.state();
    let mut random_draws = Vec::new();
    for left in 0usize..8 {
        if continent_counts[left] == 0 {
            continue;
        }
        for right in 0usize..8 {
            if left == right || continent_counts[right] != continent_counts[left] {
                continue;
            }
            let state_before = rng.state();
            let raw = rng.get(0, 0xffff);
            let exchanged = raw as u32 & 0x8000_0001 != 0;
            if exchanged {
                // Native loads each old word once. For -1 both conditions are
                // true and the second store wins, leaving `left`.
                for mapping in &mut team_to_continent {
                    let old = *mapping;
                    if old == left as i32 || old == -1 {
                        *mapping = right as i32;
                    }
                    if old == right as i32 || old == -1 {
                        *mapping = left as i32;
                    }
                }
            }
            random_draws.push(TeamContinentRandomDraw {
                call_va: MAP_FILL_CONT_EQUAL_SIZE_RNG_VA,
                left_continent: left as u8,
                right_continent: right as u8,
                state_before,
                raw,
                exchanged,
                state_after: rng.state(),
            });
        }
    }

    Ok(TeamContinentPartitionReceipt {
        entry_va: MAP_FILL_CONT_VA,
        return_va: MAP_FILL_CONT_RET_VA,
        active_slots,
        unused_teams,
        continent_counts,
        team_to_continent,
        continent_count: continent_count as u8,
        random_state_before,
        random_state_after: rng.state(),
        random_draws,
        world_walked_bytes_changed: 0,
    })
}
