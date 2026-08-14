// SPDX-License-Identifier: GPL-3.0-or-later
//! Atomic Leader-accounting transaction for the golden frame-zero free Market.
//!
//! Retail reaches these writes through `Build::init`, `Wall::increment_stats`, and the
//! Market/Temple arm of `Build::activate`.  The Build has already been linked and marked
//! ACTIVE at this composition boundary, but no Leader write is published until every
//! mirror, identity, vector shape, and typed region fact has been checked.

#![forbid(unsafe_code)]

use crate::systems::victory_score;

pub const GOLDEN_MARKET_OWNER: usize = 0;
pub const GOLDEN_MARKET_OBJECT_ID: i16 = 2_001;
pub const MARKET_TYPE: i32 = 436;
pub const MARKET_BUILDING_SLOT: usize = (MARKET_TYPE as usize) - victory_score::BUILD_FIRST;
pub const MARKET_GATHER_SLOT: usize = 2;
pub const BUILD_INIT_DIRTY_FLAG: u32 = 0x0200_0000;
pub const BUILD_ACTIVATE_DIRTY_FLAG: u32 = 0x0800_0000;

const _: () = assert!(MARKET_BUILDING_SLOT == 22);
const _: () = assert!(MARKET_GATHER_SLOT < victory_score::NUM_RESOURCES);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MarketLeaderRegionAuthority {
    /// Revision of the complete retail `Leader::produce_building` capture.
    pub capture_revision: u64,
    /// Exact native trace identity retained by the golden starting-Market receipt.
    pub native_trace_sha256: [u8; 32],
    /// DoNSave identity of the captured post-Market City/Build composition before this
    /// missing Leader owner is mounted.
    pub after_sim_sha256: [u8; 32],
    pub owner: usize,
    pub object_id: i16,
    pub type_index: i32,
    /// Exact `WData::region` captured by the World-side constructor authority.
    pub region: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LinkedMarketBuildFacts {
    pub row: usize,
    pub owner: usize,
    pub object_id: i16,
    pub type_index: i32,
    pub city: i16,
    pub active: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MarketLeaderAccountingImage {
    pub flags: u32,
    pub buildings_built: i32,
    pub num_buildings: u16,
    pub regional_buildings: u16,
    pub gather_slots: i32,
    pub gather_slots_high: i32,
    pub high_buildings: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreparedMarketLeaderAccounting {
    pub source: MarketLeaderRegionAuthority,
    pub linked: LinkedMarketBuildFacts,
    pub regional_index: usize,
    pub before: MarketLeaderAccountingImage,
    pub after_init: MarketLeaderAccountingImage,
    pub after_increment_stats: MarketLeaderAccountingImage,
    pub after_market_special: MarketLeaderAccountingImage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MarketLeaderAccountingReceipt {
    pub source: MarketLeaderRegionAuthority,
    pub linked: LinkedMarketBuildFacts,
    pub regional_index: usize,
    pub before: MarketLeaderAccountingImage,
    /// Chronological image after `Build::init`: `buildings_built++`, then dirty `0x02000000`.
    pub after_init: MarketLeaderAccountingImage,
    /// Chronological image after aggregate and regional `Wall::increment_stats` stores.
    pub after_increment_stats: MarketLeaderAccountingImage,
    /// Final image after Market gather-slot/high-water stores and dirty `0x08000000`.
    pub after_market_special: MarketLeaderAccountingImage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarketLeaderAccountingError {
    MissingRevision,
    MissingCompositionDigest,
    UnsupportedSource,
    GoldenSetupBeforeImageDisagreement,
    LinkedBuildDisagreement,
    InactiveOwner { flags: u32 },
    FlagMirrorDisagreement { victory: i32, step8: u32 },
    MissingCanonicalVector { field: &'static str },
    SourceMismatch,
    StaleBeforeImage,
}

impl std::fmt::Display for MarketLeaderAccountingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "golden Market Leader accounting refused: {self:?}")
    }
}

impl std::error::Error for MarketLeaderAccountingError {}

fn validate_source(source: MarketLeaderRegionAuthority) -> Result<(), MarketLeaderAccountingError> {
    if source.capture_revision == 0 {
        return Err(MarketLeaderAccountingError::MissingRevision);
    }
    if source.native_trace_sha256 == [0; 32] || source.after_sim_sha256 == [0; 32] {
        return Err(MarketLeaderAccountingError::MissingCompositionDigest);
    }
    if source.owner != GOLDEN_MARKET_OWNER
        || source.object_id != GOLDEN_MARKET_OBJECT_ID
        || source.type_index != MARKET_TYPE
        || usize::from(source.region) >= victory_score::NUM_BUILD_REGIONS
    {
        return Err(MarketLeaderAccountingError::UnsupportedSource);
    }
    Ok(())
}

fn image(
    source: MarketLeaderRegionAuthority,
    leader: &victory_score::LeaderState,
    step8_flags: u32,
) -> Result<(usize, MarketLeaderAccountingImage), MarketLeaderAccountingError> {
    if leader.leader_flags as u32 != step8_flags {
        return Err(MarketLeaderAccountingError::FlagMirrorDisagreement {
            victory: leader.leader_flags,
            step8: step8_flags,
        });
    }
    let required = (victory_score::leader_flag::VALID | victory_score::leader_flag::ACTIVE) as u32;
    if step8_flags & required != required {
        return Err(MarketLeaderAccountingError::InactiveOwner { flags: step8_flags });
    }
    let regional_index = usize::from(source.region)
        .checked_mul(victory_score::NUM_BUILD_SLOTS)
        .and_then(|begin| begin.checked_add(MARKET_BUILDING_SLOT))
        .ok_or(MarketLeaderAccountingError::MissingCanonicalVector {
            field: "reg_buildings",
        })?;
    let num_buildings = *leader.num_buildings.get(MARKET_BUILDING_SLOT).ok_or(
        MarketLeaderAccountingError::MissingCanonicalVector {
            field: "num_buildings",
        },
    )?;
    let regional_buildings = *leader.reg_buildings.get(regional_index).ok_or(
        MarketLeaderAccountingError::MissingCanonicalVector {
            field: "reg_buildings",
        },
    )?;
    let high_buildings = *leader.high_buildings.get(MARKET_BUILDING_SLOT).ok_or(
        MarketLeaderAccountingError::MissingCanonicalVector {
            field: "high_buildings",
        },
    )?;
    Ok((
        regional_index,
        MarketLeaderAccountingImage {
            flags: step8_flags,
            buildings_built: leader.buildings_built,
            num_buildings,
            regional_buildings,
            gather_slots: leader.gather_slots[MARKET_GATHER_SLOT],
            gather_slots_high: leader.gather_slots_high[MARKET_GATHER_SLOT],
            high_buildings,
        },
    ))
}

pub fn prepare_market_leader_accounting(
    source: MarketLeaderRegionAuthority,
    linked: LinkedMarketBuildFacts,
    leader: &victory_score::LeaderState,
    step8_flags: u32,
) -> Result<PreparedMarketLeaderAccounting, MarketLeaderAccountingError> {
    validate_source(source)?;
    if linked.owner != source.owner
        || linked.object_id != source.object_id
        || linked.type_index != source.type_index
        || linked.city != 0
        || !linked.active
    {
        return Err(MarketLeaderAccountingError::LinkedBuildDisagreement);
    }
    let (regional_index, before) = image(source, leader, step8_flags)?;
    if before.buildings_built != 1
        || before.num_buildings != 0
        || before.regional_buildings != 0
        || before.gather_slots != 0
        || before.gather_slots_high != 0
        || before.high_buildings != 0
    {
        return Err(MarketLeaderAccountingError::GoldenSetupBeforeImageDisagreement);
    }

    let mut after_init = before;
    after_init.buildings_built = after_init.buildings_built.wrapping_add(1);
    after_init.flags |= BUILD_INIT_DIRTY_FLAG;

    let mut after_increment_stats = after_init;
    after_increment_stats.num_buildings = after_increment_stats.num_buildings.wrapping_add(1);
    after_increment_stats.regional_buildings =
        after_increment_stats.regional_buildings.wrapping_add(1);

    let mut after_market_special = after_increment_stats;
    after_market_special.gather_slots = after_market_special.gather_slots.wrapping_add(1);
    after_market_special.gather_slots_high = after_market_special
        .gather_slots_high
        .max(after_market_special.gather_slots);
    after_market_special.high_buildings = after_market_special
        .high_buildings
        .max(after_market_special.num_buildings);
    after_market_special.flags |= BUILD_ACTIVATE_DIRTY_FLAG;

    Ok(PreparedMarketLeaderAccounting {
        source,
        linked,
        regional_index,
        before,
        after_init,
        after_increment_stats,
        after_market_special,
    })
}

pub fn commit_market_leader_accounting(
    source: MarketLeaderRegionAuthority,
    linked: LinkedMarketBuildFacts,
    prepared: PreparedMarketLeaderAccounting,
    leader: &mut victory_score::LeaderState,
    step8_flags: &mut u32,
) -> Result<MarketLeaderAccountingReceipt, MarketLeaderAccountingError> {
    if source != prepared.source {
        return Err(MarketLeaderAccountingError::SourceMismatch);
    }
    let current = match prepare_market_leader_accounting(source, linked, leader, *step8_flags) {
        Err(MarketLeaderAccountingError::GoldenSetupBeforeImageDisagreement) => {
            return Err(MarketLeaderAccountingError::StaleBeforeImage);
        }
        result => result?,
    };
    if current != prepared {
        return Err(MarketLeaderAccountingError::StaleBeforeImage);
    }

    let after = prepared.after_market_special;
    leader.buildings_built = after.buildings_built;
    leader.num_buildings[MARKET_BUILDING_SLOT] = after.num_buildings;
    leader.reg_buildings[prepared.regional_index] = after.regional_buildings;
    leader.gather_slots[MARKET_GATHER_SLOT] = after.gather_slots;
    leader.gather_slots_high[MARKET_GATHER_SLOT] = after.gather_slots_high;
    leader.high_buildings[MARKET_BUILDING_SLOT] = after.high_buildings;
    leader.leader_flags = after.flags as i32;
    *step8_flags = after.flags;

    Ok(MarketLeaderAccountingReceipt {
        source,
        linked,
        regional_index: prepared.regional_index,
        before: prepared.before,
        after_init: prepared.after_init,
        after_increment_stats: prepared.after_increment_stats,
        after_market_special: prepared.after_market_special,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source() -> MarketLeaderRegionAuthority {
        MarketLeaderRegionAuthority {
            capture_revision: 7,
            native_trace_sha256: [0x71; 32],
            after_sim_sha256: [0x72; 32],
            owner: 0,
            object_id: GOLDEN_MARKET_OBJECT_ID,
            type_index: MARKET_TYPE,
            region: 9,
        }
    }

    fn linked() -> LinkedMarketBuildFacts {
        LinkedMarketBuildFacts {
            row: 1,
            owner: 0,
            object_id: GOLDEN_MARKET_OBJECT_ID,
            type_index: MARKET_TYPE,
            city: 0,
            active: true,
        }
    }

    fn leader() -> victory_score::LeaderState {
        victory_score::LeaderState {
            leader_flags: victory_score::leader_flag::VALID | victory_score::leader_flag::ACTIVE,
            buildings_built: 1,
            ..victory_score::LeaderState::default()
        }
    }

    #[test]
    fn market_accounting_preserves_retail_chronology_and_both_flag_mirrors() {
        let mut leader = leader();
        let mut flags = leader.leader_flags as u32;
        let prepared =
            prepare_market_leader_accounting(source(), linked(), &leader, flags).unwrap();
        let receipt =
            commit_market_leader_accounting(source(), linked(), prepared, &mut leader, &mut flags)
                .unwrap();

        assert_eq!(receipt.after_init.buildings_built, 2);
        assert_eq!(receipt.after_increment_stats.num_buildings, 1);
        assert_eq!(receipt.after_increment_stats.regional_buildings, 1);
        assert_eq!(receipt.after_market_special.gather_slots, 1);
        assert_eq!(receipt.after_market_special.gather_slots_high, 1);
        assert_eq!(receipt.after_market_special.high_buildings, 1);
        assert_eq!(leader.leader_flags as u32, flags);
        assert_eq!(
            flags & (BUILD_INIT_DIRTY_FLAG | BUILD_ACTIVATE_DIRTY_FLAG),
            0x0a00_0000
        );
    }

    #[test]
    fn mirror_drift_and_stale_commits_leave_every_owner_unchanged() {
        let mut leader = leader();
        let flags = leader.leader_flags as u32;
        assert!(matches!(
            prepare_market_leader_accounting(source(), linked(), &leader, flags ^ 1),
            Err(MarketLeaderAccountingError::FlagMirrorDisagreement { .. })
        ));

        let prepared =
            prepare_market_leader_accounting(source(), linked(), &leader, flags).unwrap();
        leader.gather_slots[MARKET_GATHER_SLOT] = 3;
        let before = (
            leader.leader_flags,
            leader.buildings_built,
            leader.num_buildings.clone(),
            leader.reg_buildings.clone(),
            leader.gather_slots,
            leader.gather_slots_high,
            leader.high_buildings.clone(),
        );
        let mut step8 = flags;
        assert_eq!(
            commit_market_leader_accounting(source(), linked(), prepared, &mut leader, &mut step8,),
            Err(MarketLeaderAccountingError::StaleBeforeImage)
        );
        assert_eq!(
            (
                leader.leader_flags,
                leader.buildings_built,
                leader.num_buildings.clone(),
                leader.reg_buildings.clone(),
                leader.gather_slots,
                leader.gather_slots_high,
                leader.high_buildings.clone(),
            ),
            before
        );
        assert_eq!(step8, flags);
    }

    #[test]
    fn wrong_identity_city_or_region_refuses() {
        let mut leader = leader();
        let flags = leader.leader_flags as u32;
        let mut wrong_link = linked();
        wrong_link.city = -1;
        assert_eq!(
            prepare_market_leader_accounting(source(), wrong_link, &leader, flags),
            Err(MarketLeaderAccountingError::LinkedBuildDisagreement)
        );
        let mut wrong_source = source();
        wrong_source.region = 64;
        assert_eq!(
            prepare_market_leader_accounting(wrong_source, linked(), &leader, flags),
            Err(MarketLeaderAccountingError::UnsupportedSource)
        );
        leader.buildings_built = 0;
        assert_eq!(
            prepare_market_leader_accounting(source(), linked(), &leader, flags),
            Err(MarketLeaderAccountingError::GoldenSetupBeforeImageDisagreement)
        );
    }
}
