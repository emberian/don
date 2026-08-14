// SPDX-License-Identifier: GPL-3.0-or-later
//! Atomic exact bounded arms of diplomacy-forced `Army::process(1)`.
//!
//! `Leader::set_diplo` calls `Army::process(1)` for every valid Army when the owner passes
//! the ordinary Army gates. The complete general body is not mounted. The bounded arms below are
//! authority-complete: `leader_flags & 0x40` still reaches the entry decrement of `human_frame`,
//! then returns before normalization; an active empty non-mustering Army with zero cities
//! normalizes its five derived fields and takes the exact retirement/`close` path without any
//! Group, Unit, terrain, AI, or RNG access; an empty mustering Army with a still-live human-order
//! countdown normalizes, clamps its rally point through `send_here`, and returns before dispatch;
//! and an empty naval muster whose canonical founding City is foreign or inactive follows the
//! exact countdown/normalize/merge/retarget/dispatch tail, releases, enters `do_marching`, then
//! closes at its exact zero-mobile prefix before target selection or RNG.
//! The same released empty land-muster transaction is complete when its saved
//! `LeaderData::strategy[reg]` cannot select transporting. Strategy bit 4 is also complete
//! on the replay/network `get_diff` arm: the match semaphore forces the saved per-Leader
//! `multi_diff`; values below three enter defending and the empty Army closes at the exact
//! zero-count prefix before any host read.
//!
//! Retail evidence is the shipped PE `30478a44…625079`: `Armies::diplo_change`
//! `0x006F30F0..0x006F3159` (105 bytes, SHA-256 `8191743c…f3eb6`) performs the owner gates,
//! scans valid slots in ascending order, and calls `Army::process(1)`. `Army::process`
//! `0x006F93D0..0x006F9836` (1,138 bytes, SHA-256 `81f684bf…446d3`) decrements the non-zero
//! `human_frame` at `0x006F93DA..0x006F93E5`; the forced path jumps to `0x006F94AA`, tests
//! leader bit `0x40` at `0x006F94B7`, and returns at `0x006F983A` without a deeper call. With
//! that bit clear the same body calls `Army::normalize` (`0x006F9B50`, 657 bytes); its empty
//! input reads no Group, and the zero-standard retirement calls `Army::close` (`0x006F8EA0`,
//! 118 bytes), whose zero-group input likewise reaches no external host. The human-order branch
//! calls `Army::send_here` (`0x006F98A0`, 422 bytes); with zero groups its only external reads are
//! the World width and height used by the retail coordinate clamp. `Army::do_mustering`
//! (`0x006F4260`, 337 bytes) is recovered whole. Its naval release arm reads no strategy or
//! difficulty state; `Army::release_mustering` returns 1 immediately when the City owner differs
//! or, for the owning player, when the low flags byte is inactive.

use super::armies::{
    div3_shift8, Armies, ArmyData, LF2_SKIP_MASK, LF_ACTIVE, LF_ARMIES_OFF, LF_KIND_MASK,
    LF_KIND_SKIP, ST_DEFENDING, ST_MARCHING, ST_MUSTERING,
};
use super::army_do_defending::{do_defending_empty_prefix, EmptyDefendingPrefixExit};
use super::army_do_mustering::{
    do_mustering, MusteringCity, MusteringExit, MusteringHost, MUSTER_STRATEGY_REGIONS,
};
use super::leader_tribe_bonus_runtime::{get_diff, GetDiffExit, GetDiffInputs};
use super::tech_cities::CityPool;
use crate::trig::find_angle;

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
    /// Read only by the empty-retirement branch; the other bounded arms leave this `None`.
    pub leader_city_num: Option<i32>,
    /// Read only when an empty Army obeys its outstanding human rally order.
    pub world_size: Option<(i32, i32)>,
    /// Exact short-circuit City fields read by `Army::release_mustering`.
    pub muster_city: Option<ForceArmyMusterCityFact>,
    /// Exact `LeaderData::strategy[ArmyData::reg]` row read by a released land muster.
    pub muster_strategy: Option<ForceArmyMusterStrategyFact>,
    /// Exact short-circuit inputs read by `LeaderData::get_diff` on strategy bit 4.
    pub muster_difficulty: Option<ForceArmyMusterDifficultyFact>,
    pub outcome: ForceArmyProcessOutcome,
    pub before: ArmyData,
    pub after: ArmyData,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForceArmyProcessOutcome {
    ArmiesOff,
    RetiredEmpty,
    MovedEmptyHumanOrder,
    ClosedEmptyNavalMuster,
    ClosedEmptyLandMuster,
    ClosedEmptyDefendingMuster,
}

/// The smallest canonical City witness that makes `Army::release_mustering` return 1.
/// Retail reads `who` first; a foreign owner therefore does not read `city_flags`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForceArmyMusterCityFact {
    ForeignOwner { who: i8 },
    InactiveOwner { who: i8, flags_low: u8 },
}

/// The persistent LeaderData word selected by `ArmyData::reg`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ForceArmyMusterStrategyFact {
    pub region: usize,
    pub value: u16,
}

/// The two values read by the forced-per-Leader arm of `LeaderData::get_diff`.
///
/// `match_flags_820 & 4` corresponds to `game_sem::NET_OR_RECORDING`; the set bit
/// short-circuits before the other Game flag bytes and global difficulty are read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ForceArmyMusterDifficultyFact {
    pub match_flags_820: u8,
    pub multi_diff: i32,
}

impl ForceArmyMusterDifficultyFact {
    fn value(self) -> Option<i32> {
        let (value, receipt) = get_diff(GetDiffInputs {
            match_flags_820: self.match_flags_820,
            multi_diff: self.multi_diff,
            ..GetDiffInputs::default()
        });
        (receipt.exit == GetDiffExit::ForcedLeaderDifficulty && receipt.read_multi_diff)
            .then_some(value as i32)
    }
}

impl ForceArmyMusterCityFact {
    fn validates(self, before: &ArmyData) -> bool {
        if before.city < 0 {
            return false;
        }
        let owner = before.who as i8;
        match self {
            Self::ForeignOwner { who } => who != owner,
            Self::InactiveOwner { who, flags_low } => who == owner && flags_low & 1 == 0,
        }
    }
}

struct ReleasedEmptyMusterHost {
    strategy: Option<ForceArmyMusterStrategyFact>,
    difficulty: Option<ForceArmyMusterDifficultyFact>,
    leader_flags: u32,
}

impl MusteringHost for ReleasedEmptyMusterHost {
    fn release_mustering(&mut self, _army: &ArmyData) -> Option<bool> {
        Some(true)
    }

    fn city(&mut self, _who: usize, _city: i32) -> Option<MusteringCity> {
        None
    }

    fn find_muster_spot(
        &mut self,
        _army: &mut ArmyData,
        _object: i32,
        _who: i32,
        _forced: i32,
    ) -> Option<bool> {
        None
    }

    fn strategy(&mut self, _who: usize, region: i32) -> Option<u16> {
        self.strategy
            .filter(|fact| usize::try_from(region).ok() == Some(fact.region))
            .map(|fact| fact.value)
    }

    fn difficulty(&mut self, _who: usize) -> Option<i32> {
        self.difficulty
            .and_then(ForceArmyMusterDifficultyFact::value)
    }

    fn leader_flags(&mut self, _who: usize) -> Option<u32> {
        Some(self.leader_flags)
    }
}

fn observe_muster_city(
    cities: &CityPool,
    owner: usize,
    city: i32,
) -> Option<ForceArmyMusterCityFact> {
    let city = cities.slots.get(owner)?.get(usize::try_from(city).ok()?)?;
    if city.who != owner as i8 {
        Some(ForceArmyMusterCityFact::ForeignOwner { who: city.who })
    } else {
        let flags_low = city.city_flags as u8;
        (flags_low & 1 == 0).then_some(ForceArmyMusterCityFact::InactiveOwner {
            who: city.who,
            flags_low,
        })
    }
}

fn muster_city_is_current(cities: &CityPool, receipt: &ForceArmyProcessReceipt) -> bool {
    let Some(expected) = receipt.muster_city else {
        return true;
    };
    observe_muster_city(cities, receipt.request.owner, receipt.before.city) == Some(expected)
}

fn muster_strategy_is_current(
    strategy: &[[u16; MUSTER_STRATEGY_REGIONS]; 8],
    receipt: &ForceArmyProcessReceipt,
) -> bool {
    let Some(expected) = receipt.muster_strategy else {
        return true;
    };
    strategy
        .get(receipt.request.owner)
        .and_then(|rows| rows.get(expected.region))
        .copied()
        == Some(expected.value)
}

fn muster_difficulty_is_current(
    match_semaphore: u32,
    leader_multi_diff: &[i32; 8],
    receipt: &ForceArmyProcessReceipt,
) -> bool {
    let Some(expected) = receipt.muster_difficulty else {
        return true;
    };
    // Only the tested bit influences this short-circuit arm; unrelated semaphore bits may be
    // staged earlier in the same retail transaction (notably GAME_OVER during Victory).
    (expected.match_flags_820 ^ match_semaphore as u8) & 4 == 0
        && leader_multi_diff[receipt.request.owner] == expected.multi_diff
}

impl ForceArmyProcessReceipt {
    pub fn muster_city_is_current(&self, cities: &CityPool) -> bool {
        muster_city_is_current(cities, self)
    }

    pub fn muster_strategy_is_current(
        &self,
        strategy: &[[u16; MUSTER_STRATEGY_REGIONS]; 8],
    ) -> bool {
        muster_strategy_is_current(strategy, self)
    }

    pub fn muster_difficulty_is_current(
        &self,
        match_semaphore: u32,
        leader_multi_diff: &[i32; 8],
    ) -> bool {
        muster_difficulty_is_current(match_semaphore, leader_multi_diff, self)
    }

    pub fn validates(&self) -> bool {
        if self.request.forced != 1
            || self.leader_flags & LF_ACTIVE == 0
            || self.leader_flags & LF_KIND_MASK == LF_KIND_SKIP
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
        match self.outcome {
            ForceArmyProcessOutcome::ArmiesOff => {
                if self.leader_flags & LF_ARMIES_OFF == 0
                    || self.leader_city_num.is_some()
                    || self.world_size.is_some()
                    || self.muster_city.is_some()
                    || self.muster_strategy.is_some()
                    || self.muster_difficulty.is_some()
                {
                    return false;
                }
            }
            ForceArmyProcessOutcome::RetiredEmpty => {
                if self.leader_flags & LF_ARMIES_OFF != 0
                    || self.leader_city_num != Some(0)
                    || self.before.num_groups != 0
                    || self.before.status & ST_MUSTERING != 0
                    || self.world_size.is_some()
                    || self.muster_city.is_some()
                    || self.muster_strategy.is_some()
                    || self.muster_difficulty.is_some()
                {
                    return false;
                }
                // `normalize`, the zero-city scan, then `Army::close`. `close` retains
                // city-independent targeting/rally fields and the unused list tail.
                expected.role = 0;
                expected.num_units = 0;
                expected.num_captains = 0;
                expected.num_standard = 0;
                expected.num_decoys = 0;
                expected.city = 0;
                expected.valid = 0;
                expected.status = 0;
                expected.human_frame = 0;
                expected.num_groups = 0;
            }
            ForceArmyProcessOutcome::MovedEmptyHumanOrder => {
                let Some((width, height)) = self.world_size else {
                    return false;
                };
                let Some(max_x) = width.checked_mul(3 * 256) else {
                    return false;
                };
                let Some(max_y) = height.checked_mul(3 * 256) else {
                    return false;
                };
                if self.leader_flags & LF_ARMIES_OFF != 0
                    || self.leader_city_num.is_some()
                    || self.before.num_groups != 0
                    || self.before.status & ST_MUSTERING == 0
                    || self.before.human_frame <= 1
                    || max_x <= 0
                    || max_y <= 0
                    || self.muster_city.is_some()
                    || self.muster_strategy.is_some()
                    || self.muster_difficulty.is_some()
                {
                    return false;
                }
                expected.role = 0;
                expected.num_units = 0;
                expected.num_captains = 0;
                expected.num_standard = 0;
                expected.num_decoys = 0;
                let old_x = expected.x;
                let old_y = expected.y;
                let mut x = old_x.max(0);
                let mut y = old_y.max(0);
                if x >= max_x {
                    x = max_x - 1;
                }
                if y >= max_y {
                    y = max_y - 1;
                }
                if x != old_x && y != old_y {
                    expected.muster_angle =
                        find_angle(x.wrapping_sub(old_x), y.wrapping_sub(old_y));
                }
                expected.x = x;
                expected.y = y;
                expected.muster_x = div3_shift8(x);
                expected.muster_y = div3_shift8(y);
            }
            ForceArmyProcessOutcome::ClosedEmptyNavalMuster => {
                if self.leader_flags & LF_ARMIES_OFF != 0
                    || self.leader_city_num.is_some()
                    || self.world_size.is_some()
                    || self.before.num_groups != 0
                    || self.before.status & ST_MUSTERING == 0
                    || !matches!(self.before.human_frame, 0 | 1)
                    || self.before.navy == 0
                    || (self.before.target_o >= 0 && self.before.target_who >= 0)
                    || !self
                        .muster_city
                        .is_some_and(|fact| fact.validates(&self.before))
                    || self.muster_strategy.is_some()
                    || self.muster_difficulty.is_some()
                {
                    return false;
                }
                expected.role = 0;
                expected.num_units = 0;
                expected.num_captains = 0;
                expected.num_standard = 0;
                expected.num_decoys = 0;
                let mut host = ReleasedEmptyMusterHost {
                    strategy: None,
                    difficulty: None,
                    leader_flags: self.leader_flags,
                };
                if !matches!(
                    do_mustering(&mut expected, &mut host),
                    MusteringExit::Dispatched(_)
                ) || expected.status != ST_MARCHING
                {
                    return false;
                }
                // Retail re-reads the new status, enters do_marching, and count(2, 0) is
                // exactly zero for a zero-group Army. Its first arm calls Army::close before
                // target validation/find_target, so no target host or RNG is reached.
                expected.valid = 0;
                expected.status = 0;
                expected.human_frame = 0;
                expected.num_groups = 0;
                // The later non-forming is_engaged call repeats empty normalization.
                expected.role = 0;
                expected.num_units = 0;
                expected.num_captains = 0;
                expected.num_standard = 0;
                expected.num_decoys = 0;
            }
            ForceArmyProcessOutcome::ClosedEmptyLandMuster => {
                let Some(strategy) = self.muster_strategy else {
                    return false;
                };
                if self.leader_flags & LF_ARMIES_OFF != 0
                    || self.leader_city_num.is_some()
                    || self.world_size.is_some()
                    || self.before.num_groups != 0
                    || self.before.status & ST_MUSTERING == 0
                    || !matches!(self.before.human_frame, 0 | 1)
                    || self.before.navy != 0
                    || (self.before.target_o >= 0 && self.before.target_who >= 0)
                    || !self
                        .muster_city
                        .is_some_and(|fact| fact.validates(&self.before))
                    || usize::try_from(self.before.reg).ok() != Some(strategy.region)
                    || strategy.region >= MUSTER_STRATEGY_REGIONS
                    // This outcome is the marching arm, so bit 4 must not enter defending.
                    // Bit 8 may read leader_flags, which is already receipt-bound, but it must
                    // not select do_transporting.
                    || strategy.value & 4 != 0
                    || (strategy.value & 8 != 0 && self.leader_flags & 0x300 != 0)
                    || self.muster_difficulty.is_some()
                {
                    return false;
                }
                expected.role = 0;
                expected.num_units = 0;
                expected.num_captains = 0;
                expected.num_standard = 0;
                expected.num_decoys = 0;
                let mut host = ReleasedEmptyMusterHost {
                    strategy: Some(strategy),
                    difficulty: None,
                    leader_flags: self.leader_flags,
                };
                if !matches!(
                    do_mustering(&mut expected, &mut host),
                    MusteringExit::Dispatched(_)
                ) || expected.status != ST_MARCHING
                {
                    return false;
                }
                // Retail re-reads marching status and closes at the zero-mobile prefix.
                expected.valid = 0;
                expected.status = 0;
                expected.human_frame = 0;
                expected.num_groups = 0;
                expected.role = 0;
                expected.num_units = 0;
                expected.num_captains = 0;
                expected.num_standard = 0;
                expected.num_decoys = 0;
            }
            ForceArmyProcessOutcome::ClosedEmptyDefendingMuster => {
                let (Some(strategy), Some(difficulty)) =
                    (self.muster_strategy, self.muster_difficulty)
                else {
                    return false;
                };
                if self.leader_flags & LF_ARMIES_OFF != 0
                    || self.leader_city_num.is_some()
                    || self.world_size.is_some()
                    || self.before.num_groups != 0
                    || self.before.status & ST_MUSTERING == 0
                    || !matches!(self.before.human_frame, 0 | 1)
                    || self.before.navy != 0
                    || (self.before.target_o >= 0 && self.before.target_who >= 0)
                    || !self
                        .muster_city
                        .is_some_and(|fact| fact.validates(&self.before))
                    || usize::try_from(self.before.reg).ok() != Some(strategy.region)
                    || strategy.region >= MUSTER_STRATEGY_REGIONS
                    || strategy.value & 4 == 0
                    || difficulty.value().is_none_or(|value| value >= 3)
                {
                    return false;
                }
                expected.role = 0;
                expected.num_units = 0;
                expected.num_captains = 0;
                expected.num_standard = 0;
                expected.num_decoys = 0;
                let mut host = ReleasedEmptyMusterHost {
                    strategy: Some(strategy),
                    difficulty: Some(difficulty),
                    leader_flags: self.leader_flags,
                };
                if !matches!(
                    do_mustering(&mut expected, &mut host),
                    MusteringExit::Dispatched(_)
                ) || expected.status != ST_DEFENDING
                    || do_defending_empty_prefix(&mut expected) != EmptyDefendingPrefixExit::Closed
                {
                    return false;
                }
                // The later non-forming `is_engaged` call repeats empty normalization.
                expected.role = 0;
                expected.num_units = 0;
                expected.num_captains = 0;
                expected.num_standard = 0;
                expected.num_decoys = 0;
            }
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
    leader_city_num: [i32; 8],
    world_size: (i32, i32),
    match_flags_820: Option<u8>,
    leader_multi_diff: Option<[i32; 8]>,
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
                    && receipt.leader_city_num.is_none_or(|city_num| {
                        self.leader_city_num[receipt.request.owner] == city_num
                    })
                    && receipt
                        .world_size
                        .is_none_or(|world_size| self.world_size == world_size)
                    && receipt.muster_difficulty.is_none_or(|difficulty| {
                        self.match_flags_820 == Some(difficulty.match_flags_820)
                            && self.leader_multi_diff.is_some_and(|multi_diff| {
                                multi_diff[receipt.request.owner] == difficulty.multi_diff
                            })
                    })
                    && self.before.lists[receipt.request.owner][receipt.request.army_slot]
                        == receipt.before
                    && self.after.lists[receipt.request.owner][receipt.request.army_slot]
                        == receipt.after
            })
    }

    pub fn is_current(
        &self,
        armies: &Armies,
        cities: &CityPool,
        leader_flags: &[u32; 8],
        leader_flags2: &[u32; 8],
        leader_city_num: &[i32; 8],
        world_size: (i32, i32),
    ) -> bool {
        self.is_current_impl(
            armies,
            cities,
            leader_flags,
            leader_flags2,
            leader_city_num,
            world_size,
            None,
            None,
        )
    }

    pub fn is_current_with_strategy(
        &self,
        armies: &Armies,
        cities: &CityPool,
        leader_flags: &[u32; 8],
        leader_flags2: &[u32; 8],
        leader_city_num: &[i32; 8],
        world_size: (i32, i32),
        leader_strategy: &[[u16; MUSTER_STRATEGY_REGIONS]; 8],
    ) -> bool {
        self.is_current_impl(
            armies,
            cities,
            leader_flags,
            leader_flags2,
            leader_city_num,
            world_size,
            Some(leader_strategy),
            None,
        )
    }

    pub fn is_current_with_strategy_and_difficulty(
        &self,
        armies: &Armies,
        cities: &CityPool,
        leader_flags: &[u32; 8],
        leader_flags2: &[u32; 8],
        leader_city_num: &[i32; 8],
        world_size: (i32, i32),
        leader_strategy: &[[u16; MUSTER_STRATEGY_REGIONS]; 8],
        match_semaphore: u32,
        leader_multi_diff: &[i32; 8],
    ) -> bool {
        self.is_current_impl(
            armies,
            cities,
            leader_flags,
            leader_flags2,
            leader_city_num,
            world_size,
            Some(leader_strategy),
            Some((match_semaphore, leader_multi_diff)),
        )
    }

    fn is_current_impl(
        &self,
        armies: &Armies,
        cities: &CityPool,
        leader_flags: &[u32; 8],
        leader_flags2: &[u32; 8],
        leader_city_num: &[i32; 8],
        world_size: (i32, i32),
        leader_strategy: Option<&[[u16; MUSTER_STRATEGY_REGIONS]; 8]>,
        difficulty_authority: Option<(u32, &[i32; 8])>,
    ) -> bool {
        self.receipts.iter().all(|receipt| {
            leader_flags[receipt.request.owner] == receipt.leader_flags
                && leader_flags2[receipt.request.owner] == receipt.leader_flags2
                && receipt
                    .leader_city_num
                    .is_none_or(|city_num| leader_city_num[receipt.request.owner] == city_num)
                && receipt
                    .world_size
                    .is_none_or(|expected| world_size == expected)
                && muster_city_is_current(cities, receipt)
                && receipt.muster_strategy.is_none_or(|_| {
                    leader_strategy
                        .is_some_and(|strategy| muster_strategy_is_current(strategy, receipt))
                })
                && receipt.muster_difficulty.is_none_or(|_| {
                    difficulty_authority.is_some_and(|(match_semaphore, leader_multi_diff)| {
                        muster_difficulty_is_current(match_semaphore, leader_multi_diff, receipt)
                    })
                })
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
    StaleWorld,
    StaleCity { owner: usize, city: i32 },
    StaleStrategy { owner: usize, region: usize },
    StaleDifficulty { owner: usize },
    InvalidPrepared,
}

pub fn prepare_force_army_process(
    armies: &Armies,
    cities: &CityPool,
    leader_flags: &[u32; 8],
    leader_flags2: &[u32; 8],
    leader_city_num: &[i32; 8],
    world_size: (i32, i32),
    requests: &[ForceArmyProcessRequest],
) -> Result<PreparedForceArmyProcess, ForceArmyProcessError> {
    prepare_force_army_process_impl(
        armies,
        cities,
        leader_flags,
        leader_flags2,
        leader_city_num,
        world_size,
        None,
        None,
        requests,
    )
}

pub fn prepare_force_army_process_with_strategy(
    armies: &Armies,
    cities: &CityPool,
    leader_flags: &[u32; 8],
    leader_flags2: &[u32; 8],
    leader_city_num: &[i32; 8],
    world_size: (i32, i32),
    leader_strategy: &[[u16; MUSTER_STRATEGY_REGIONS]; 8],
    requests: &[ForceArmyProcessRequest],
) -> Result<PreparedForceArmyProcess, ForceArmyProcessError> {
    prepare_force_army_process_impl(
        armies,
        cities,
        leader_flags,
        leader_flags2,
        leader_city_num,
        world_size,
        Some(leader_strategy),
        None,
        requests,
    )
}

pub fn prepare_force_army_process_with_strategy_and_difficulty(
    armies: &Armies,
    cities: &CityPool,
    leader_flags: &[u32; 8],
    leader_flags2: &[u32; 8],
    leader_city_num: &[i32; 8],
    world_size: (i32, i32),
    leader_strategy: &[[u16; MUSTER_STRATEGY_REGIONS]; 8],
    match_semaphore: u32,
    leader_multi_diff: &[i32; 8],
    requests: &[ForceArmyProcessRequest],
) -> Result<PreparedForceArmyProcess, ForceArmyProcessError> {
    prepare_force_army_process_impl(
        armies,
        cities,
        leader_flags,
        leader_flags2,
        leader_city_num,
        world_size,
        Some(leader_strategy),
        Some((match_semaphore, leader_multi_diff)),
        requests,
    )
}

fn prepare_force_army_process_impl(
    armies: &Armies,
    cities: &CityPool,
    leader_flags: &[u32; 8],
    leader_flags2: &[u32; 8],
    leader_city_num: &[i32; 8],
    world_size: (i32, i32),
    leader_strategy: Option<&[[u16; MUSTER_STRATEGY_REGIONS]; 8]>,
    difficulty_authority: Option<(u32, &[i32; 8])>,
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
        let (
            outcome,
            city_num,
            receipt_world_size,
            muster_city,
            muster_strategy,
            muster_difficulty,
        ) = if flags & LF_ARMIES_OFF != 0 {
            (
                ForceArmyProcessOutcome::ArmiesOff,
                None,
                None,
                None,
                None,
                None,
            )
        } else if before.num_groups == 0
            && before.status & ST_MUSTERING != 0
            && before.human_frame > 1
            && world_size
                .0
                .checked_mul(3 * 256)
                .is_some_and(|limit| limit > 0)
            && world_size
                .1
                .checked_mul(3 * 256)
                .is_some_and(|limit| limit > 0)
        {
            (
                ForceArmyProcessOutcome::MovedEmptyHumanOrder,
                None,
                Some(world_size),
                None,
                None,
                None,
            )
        } else if before.num_groups == 0
            && before.status & ST_MUSTERING != 0
            && matches!(before.human_frame, 0 | 1)
            && before.navy != 0
            && (before.target_o < 0 || before.target_who < 0)
        {
            let Some(muster_city) = observe_muster_city(cities, request.owner, before.city) else {
                return Err(ForceArmyProcessError::RequiresUnresolvedArmyBody {
                    owner: request.owner,
                    army_slot: request.army_slot,
                });
            };
            (
                ForceArmyProcessOutcome::ClosedEmptyNavalMuster,
                None,
                None,
                Some(muster_city),
                None,
                None,
            )
        } else if before.num_groups == 0
            && before.status & ST_MUSTERING != 0
            && matches!(before.human_frame, 0 | 1)
            && before.navy == 0
            && (before.target_o < 0 || before.target_who < 0)
        {
            let Some(muster_city) = observe_muster_city(cities, request.owner, before.city) else {
                return Err(ForceArmyProcessError::RequiresUnresolvedArmyBody {
                    owner: request.owner,
                    army_slot: request.army_slot,
                });
            };
            let Some(region) = usize::try_from(before.reg)
                .ok()
                .filter(|region| *region < MUSTER_STRATEGY_REGIONS)
            else {
                return Err(ForceArmyProcessError::RequiresUnresolvedArmyBody {
                    owner: request.owner,
                    army_slot: request.army_slot,
                });
            };
            let Some(value) = leader_strategy.map(|strategy| strategy[request.owner][region])
            else {
                return Err(ForceArmyProcessError::RequiresUnresolvedArmyBody {
                    owner: request.owner,
                    army_slot: request.army_slot,
                });
            };
            let muster_difficulty = if value & 4 != 0 {
                let Some((match_semaphore, leader_multi_diff)) = difficulty_authority else {
                    return Err(ForceArmyProcessError::RequiresUnresolvedArmyBody {
                        owner: request.owner,
                        army_slot: request.army_slot,
                    });
                };
                let fact = ForceArmyMusterDifficultyFact {
                    match_flags_820: match_semaphore as u8,
                    multi_diff: leader_multi_diff[request.owner],
                };
                if fact.value().is_none() {
                    return Err(ForceArmyProcessError::RequiresUnresolvedArmyBody {
                        owner: request.owner,
                        army_slot: request.army_slot,
                    });
                }
                Some(fact)
            } else {
                None
            };
            let defending = muster_difficulty
                .and_then(ForceArmyMusterDifficultyFact::value)
                .is_some_and(|value| value < 3);
            if (value & 4 != 0 && !defending)
                || (!defending && value & 8 != 0 && flags & 0x300 != 0)
            {
                return Err(ForceArmyProcessError::RequiresUnresolvedArmyBody {
                    owner: request.owner,
                    army_slot: request.army_slot,
                });
            }
            (
                if defending {
                    ForceArmyProcessOutcome::ClosedEmptyDefendingMuster
                } else {
                    ForceArmyProcessOutcome::ClosedEmptyLandMuster
                },
                None,
                None,
                Some(muster_city),
                Some(ForceArmyMusterStrategyFact { region, value }),
                muster_difficulty,
            )
        } else {
            let city_num = leader_city_num[request.owner];
            if before.num_groups == 0 && before.status & ST_MUSTERING == 0 && city_num == 0 {
                (
                    ForceArmyProcessOutcome::RetiredEmpty,
                    Some(city_num),
                    None,
                    None,
                    None,
                    None,
                )
            } else {
                return Err(ForceArmyProcessError::RequiresUnresolvedArmyBody {
                    owner: request.owner,
                    army_slot: request.army_slot,
                });
            }
        };
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
        match outcome {
            ForceArmyProcessOutcome::ArmiesOff => {}
            ForceArmyProcessOutcome::RetiredEmpty => {
                army_after.role = 0;
                army_after.num_units = 0;
                army_after.num_captains = 0;
                army_after.num_standard = 0;
                army_after.num_decoys = 0;
                army_after.city = 0;
                army_after.valid = 0;
                army_after.status = 0;
                army_after.human_frame = 0;
                army_after.num_groups = 0;
            }
            ForceArmyProcessOutcome::MovedEmptyHumanOrder => {
                army_after.role = 0;
                army_after.num_units = 0;
                army_after.num_captains = 0;
                army_after.num_standard = 0;
                army_after.num_decoys = 0;
                let (width, height) = world_size;
                let max_x = width * 3 * 256;
                let max_y = height * 3 * 256;
                let old_x = army_after.x;
                let old_y = army_after.y;
                let mut x = old_x.max(0);
                let mut y = old_y.max(0);
                if x >= max_x {
                    x = max_x - 1;
                }
                if y >= max_y {
                    y = max_y - 1;
                }
                if x != old_x && y != old_y {
                    army_after.muster_angle =
                        find_angle(x.wrapping_sub(old_x), y.wrapping_sub(old_y));
                }
                army_after.x = x;
                army_after.y = y;
                army_after.muster_x = div3_shift8(x);
                army_after.muster_y = div3_shift8(y);
            }
            ForceArmyProcessOutcome::ClosedEmptyNavalMuster => {
                army_after.role = 0;
                army_after.num_units = 0;
                army_after.num_captains = 0;
                army_after.num_standard = 0;
                army_after.num_decoys = 0;
                let mut host = ReleasedEmptyMusterHost {
                    strategy: None,
                    difficulty: None,
                    leader_flags: flags,
                };
                let out = do_mustering(&mut army_after, &mut host);
                debug_assert!(matches!(out, MusteringExit::Dispatched(_)));
                debug_assert_eq!(army_after.status, ST_MARCHING);
                // Army::do_marching begins with count(2, 0); an empty Army obtains zero and
                // calls close before any target query or find_target/RNG work.
                army_after.valid = 0;
                army_after.status = 0;
                army_after.human_frame = 0;
                army_after.num_groups = 0;
            }
            ForceArmyProcessOutcome::ClosedEmptyLandMuster => {
                army_after.role = 0;
                army_after.num_units = 0;
                army_after.num_captains = 0;
                army_after.num_standard = 0;
                army_after.num_decoys = 0;
                let mut host = ReleasedEmptyMusterHost {
                    strategy: muster_strategy,
                    difficulty: None,
                    leader_flags: flags,
                };
                let out = do_mustering(&mut army_after, &mut host);
                debug_assert!(matches!(out, MusteringExit::Dispatched(_)));
                debug_assert_eq!(army_after.status, ST_MARCHING);
                army_after.valid = 0;
                army_after.status = 0;
                army_after.human_frame = 0;
                army_after.num_groups = 0;
            }
            ForceArmyProcessOutcome::ClosedEmptyDefendingMuster => {
                army_after.role = 0;
                army_after.num_units = 0;
                army_after.num_captains = 0;
                army_after.num_standard = 0;
                army_after.num_decoys = 0;
                let mut host = ReleasedEmptyMusterHost {
                    strategy: muster_strategy,
                    difficulty: muster_difficulty,
                    leader_flags: flags,
                };
                let out = do_mustering(&mut army_after, &mut host);
                debug_assert!(matches!(out, MusteringExit::Dispatched(_)));
                debug_assert_eq!(army_after.status, ST_DEFENDING);
                debug_assert_eq!(
                    do_defending_empty_prefix(&mut army_after),
                    EmptyDefendingPrefixExit::Closed
                );
            }
        }
        after.lists[request.owner][request.army_slot] = army_after.clone();
        receipts.push(ForceArmyProcessReceipt {
            request,
            leader_flags: flags,
            leader_flags2: flags2,
            leader_city_num: city_num,
            world_size: receipt_world_size,
            muster_city,
            muster_strategy,
            muster_difficulty,
            outcome,
            before,
            after: army_after,
        });
    }
    let prepared = PreparedForceArmyProcess {
        before: armies.clone(),
        after,
        leader_flags: *leader_flags,
        leader_flags2: *leader_flags2,
        leader_city_num: *leader_city_num,
        world_size,
        match_flags_820: difficulty_authority.map(|(semaphore, _)| semaphore as u8),
        leader_multi_diff: difficulty_authority.map(|(_, multi_diff)| *multi_diff),
        receipts,
    };
    if !prepared.validates() {
        return Err(ForceArmyProcessError::InvalidPrepared);
    }
    Ok(prepared)
}

pub fn commit_force_army_process(
    armies: &mut Armies,
    cities: &CityPool,
    leader_flags: &[u32; 8],
    leader_flags2: &[u32; 8],
    leader_city_num: &[i32; 8],
    world_size: (i32, i32),
    prepared: PreparedForceArmyProcess,
) -> Result<Vec<ForceArmyProcessReceipt>, ForceArmyProcessError> {
    commit_force_army_process_impl(
        armies,
        cities,
        leader_flags,
        leader_flags2,
        leader_city_num,
        world_size,
        None,
        None,
        prepared,
    )
}

pub fn commit_force_army_process_with_strategy(
    armies: &mut Armies,
    cities: &CityPool,
    leader_flags: &[u32; 8],
    leader_flags2: &[u32; 8],
    leader_city_num: &[i32; 8],
    world_size: (i32, i32),
    leader_strategy: &[[u16; MUSTER_STRATEGY_REGIONS]; 8],
    prepared: PreparedForceArmyProcess,
) -> Result<Vec<ForceArmyProcessReceipt>, ForceArmyProcessError> {
    commit_force_army_process_impl(
        armies,
        cities,
        leader_flags,
        leader_flags2,
        leader_city_num,
        world_size,
        Some(leader_strategy),
        None,
        prepared,
    )
}

pub fn commit_force_army_process_with_strategy_and_difficulty(
    armies: &mut Armies,
    cities: &CityPool,
    leader_flags: &[u32; 8],
    leader_flags2: &[u32; 8],
    leader_city_num: &[i32; 8],
    world_size: (i32, i32),
    leader_strategy: &[[u16; MUSTER_STRATEGY_REGIONS]; 8],
    match_semaphore: u32,
    leader_multi_diff: &[i32; 8],
    prepared: PreparedForceArmyProcess,
) -> Result<Vec<ForceArmyProcessReceipt>, ForceArmyProcessError> {
    commit_force_army_process_impl(
        armies,
        cities,
        leader_flags,
        leader_flags2,
        leader_city_num,
        world_size,
        Some(leader_strategy),
        Some((match_semaphore, leader_multi_diff)),
        prepared,
    )
}

fn commit_force_army_process_impl(
    armies: &mut Armies,
    cities: &CityPool,
    leader_flags: &[u32; 8],
    leader_flags2: &[u32; 8],
    leader_city_num: &[i32; 8],
    world_size: (i32, i32),
    leader_strategy: Option<&[[u16; MUSTER_STRATEGY_REGIONS]; 8]>,
    difficulty_authority: Option<(u32, &[i32; 8])>,
    prepared: PreparedForceArmyProcess,
) -> Result<Vec<ForceArmyProcessReceipt>, ForceArmyProcessError> {
    if !prepared.validates() {
        return Err(ForceArmyProcessError::InvalidPrepared);
    }
    if let Some(stale) = prepared.receipts.iter().find(|receipt| {
        leader_flags[receipt.request.owner] != receipt.leader_flags
            || leader_flags2[receipt.request.owner] != receipt.leader_flags2
            || receipt
                .leader_city_num
                .is_some_and(|city_num| leader_city_num[receipt.request.owner] != city_num)
    }) {
        return Err(ForceArmyProcessError::StaleLeader {
            owner: stale.request.owner,
        });
    }
    if prepared.receipts.iter().any(|receipt| {
        receipt
            .world_size
            .is_some_and(|expected| expected != world_size)
    }) {
        return Err(ForceArmyProcessError::StaleWorld);
    }
    if let Some(stale) = prepared
        .receipts
        .iter()
        .find(|receipt| !muster_city_is_current(cities, receipt))
    {
        return Err(ForceArmyProcessError::StaleCity {
            owner: stale.request.owner,
            city: stale.before.city,
        });
    }
    if let Some(stale) = prepared.receipts.iter().find(|receipt| {
        receipt.muster_strategy.is_some()
            && leader_strategy.is_none_or(|strategy| !muster_strategy_is_current(strategy, receipt))
    }) {
        return Err(ForceArmyProcessError::StaleStrategy {
            owner: stale.request.owner,
            region: stale.muster_strategy.expect("checked some").region,
        });
    }
    if let Some(stale) = prepared.receipts.iter().find(|receipt| {
        receipt.muster_difficulty.is_some()
            && difficulty_authority.is_none_or(|(match_semaphore, leader_multi_diff)| {
                !muster_difficulty_is_current(match_semaphore, leader_multi_diff, receipt)
            })
    }) {
        return Err(ForceArmyProcessError::StaleDifficulty {
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
