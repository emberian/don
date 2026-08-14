// SPDX-License-Identifier: GPL-3.0-or-later
//! Exact deterministic body of `Army::do_mustering()` at retail `0x006F4260`.
//!
//! The shipped body is 337 bytes (`army.cpp:3149-3204`). It consumes no RNG. Calls whose
//! owners are outside `ArmyData` are explicit fail-closed host seams; a missing fact stops at
//! the instruction that would have read it and never turns absence into a negative answer.

use super::armies::{ArmyData, ST_DEFENDING, ST_FORMING, ST_MARCHING, ST_TRANSPORTING};

pub const RETAIL_VA: u32 = 0x006F_4260;
pub const RETAIL_SIZE: u32 = 337;
pub const WCOORD_SCALE: i32 = 0x300;
pub const WCOORD_HALF: i32 = 0x180;

/// The two City fields read before `Army::find_muster_spot` is called.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MusteringCity {
    pub flags: u16,
    pub object: i16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MusteringMissingFact {
    Release,
    City { who: usize, city: i32 },
    FindMusterSpot { object: i32, who: i32 },
    Strategy { who: usize, region: i32 },
    Difficulty { who: usize },
    LeaderFlags { who: usize },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MusteringTrace {
    pub release_calls: u32,
    pub city_reads: u32,
    pub find_muster_spot_calls: u32,
    pub strategy_reads: u32,
    pub difficulty_reads: u32,
    pub leader_flag_reads: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MusteringExit {
    /// Retail return 1 after the common city/rally rewrite.
    Dispatched(MusteringTrace),
    /// Retail return 1 from the successful `find_muster_spot` arm. The common rewrite is skipped.
    Forming(MusteringTrace),
    /// Headless-only boundary; retail has no corresponding return value.
    Missing {
        fact: MusteringMissingFact,
        trace: MusteringTrace,
    },
}

impl MusteringExit {
    pub fn trace(self) -> MusteringTrace {
        match self {
            Self::Dispatched(trace) | Self::Forming(trace) | Self::Missing { trace, .. } => trace,
        }
    }

    pub fn retail_return(self) -> Option<i32> {
        match self {
            Self::Dispatched(_) | Self::Forming(_) => Some(1),
            Self::Missing { .. } => None,
        }
    }
}

/// External queries/calls reached by `Army::do_mustering`.
pub trait MusteringHost {
    /// `Army::release_mustering` `0x006F87C0`.
    fn release_mustering(&mut self, army: &ArmyData) -> Option<bool>;
    /// `cities.lists[who][city]`, followed by `CityData::city_flags +0x04` and `o +0x08`.
    fn city(&mut self, who: usize, city: i32) -> Option<MusteringCity>;
    /// `Army::find_muster_spot(city.o, who, 1)` `0x006F5CC0`. The callee may mutate ArmyData
    /// even when it returns zero, so the canonical view is passed by mutable reference.
    fn find_muster_spot(
        &mut self,
        army: &mut ArmyData,
        object: i32,
        who: i32,
        forced: i32,
    ) -> Option<bool>;
    /// `LeaderData::strategy[army.reg]` at Leader `+0xA68`.
    fn strategy(&mut self, who: usize, region: i32) -> Option<u16>;
    /// `LeaderData::get_diff()` `0x006EC000`.
    fn difficulty(&mut self, who: usize) -> Option<i32>;
    /// `LeaderData::leader_flags +0x00`.
    fn leader_flags(&mut self, who: usize) -> Option<u32>;
}

/// Execute all 337 retail bytes in mutation and short-circuit order.
pub fn do_mustering<H: MusteringHost + ?Sized>(army: &mut ArmyData, host: &mut H) -> MusteringExit {
    let mut trace = MusteringTrace::default();

    macro_rules! need {
        ($value:expr, $fact:expr) => {
            match $value {
                Some(value) => value,
                None => return MusteringExit::Missing { fact: $fact, trace },
            }
        };
    }

    trace.release_calls = 1;
    let released = need!(host.release_mustering(army), MusteringMissingFact::Release);
    let who = army.who as usize;

    if released {
        // 0x006F4270: navies skip every Leader strategy/difficulty read.
        if army.navy == 0 {
            trace.strategy_reads = trace.strategy_reads.wrapping_add(1);
            let strategy = need!(
                host.strategy(who, army.reg),
                MusteringMissingFact::Strategy {
                    who,
                    region: army.reg,
                }
            );
            if strategy & 4 != 0 {
                trace.difficulty_reads = trace.difficulty_reads.wrapping_add(1);
                let difficulty = need!(
                    host.difficulty(who),
                    MusteringMissingFact::Difficulty { who }
                );
                if difficulty < 3 {
                    army.status = ST_DEFENDING;
                    finish(army);
                    return MusteringExit::Dispatched(trace);
                }
            }
            if strategy & 8 != 0 {
                trace.leader_flag_reads = trace.leader_flag_reads.wrapping_add(1);
                let flags = need!(
                    host.leader_flags(who),
                    MusteringMissingFact::LeaderFlags { who }
                );
                if flags & 0x300 != 0 {
                    army.status = ST_TRANSPORTING;
                    finish(army);
                    return MusteringExit::Dispatched(trace);
                }
            }
        }
        army.status = ST_MARCHING;
        finish(army);
        return MusteringExit::Dispatched(trace);
    }

    // 0x006F42F1..0x006F4337: a successful muster search is the sole early return and
    // deliberately skips the common city/rally rewrite.
    if army.city >= 0 {
        trace.city_reads = trace.city_reads.wrapping_add(1);
        let city = need!(
            host.city(who, army.city),
            MusteringMissingFact::City {
                who,
                city: army.city,
            }
        );
        if city.flags & 1 != 0 {
            trace.find_muster_spot_calls = trace.find_muster_spot_calls.wrapping_add(1);
            let object = i32::from(city.object);
            if need!(
                host.find_muster_spot(army, object, i32::from(army.who), 1),
                MusteringMissingFact::FindMusterSpot {
                    object,
                    who: i32::from(army.who),
                }
            ) {
                army.status |= ST_FORMING;
                return MusteringExit::Forming(trace);
            }
        }
    }

    trace.strategy_reads = trace.strategy_reads.wrapping_add(1);
    let strategy = need!(
        host.strategy(who, army.reg),
        MusteringMissingFact::Strategy {
            who,
            region: army.reg,
        }
    );
    if strategy & 8 != 0 && army.num_captains > 7 {
        trace.leader_flag_reads = trace.leader_flag_reads.wrapping_add(1);
        let flags = need!(
            host.leader_flags(who),
            MusteringMissingFact::LeaderFlags { who }
        );
        if flags & 0x300 != 0 {
            army.status = ST_TRANSPORTING;
            finish(army);
            return MusteringExit::Dispatched(trace);
        }
    }
    army.status = ST_MARCHING;
    finish(army);
    MusteringExit::Dispatched(trace)
}

#[inline]
fn finish(army: &mut ArmyData) {
    // LEA/SHL/ADD in retail are wrapping 32-bit arithmetic.
    army.city = -1;
    army.x = army
        .muster_x
        .wrapping_mul(WCOORD_SCALE)
        .wrapping_add(WCOORD_HALF);
    army.y = army
        .muster_y
        .wrapping_mul(WCOORD_SCALE)
        .wrapping_add(WCOORD_HALF);
    army.angle = army.muster_angle;
}
