// SPDX-License-Identifier: GPL-3.0-or-later
//! Exact logical mutation owner for the three `ResourceDivvyPool` selectors.
//!
//! The shipped implementations at `0x0068a4a0`, `0x0068a570`, and `0x0068a640`
//! select an unused entry, mark its bit before inspecting the good ID, and clear the
//! complete lane mask after every logical entry has been visited.

use crate::place_resources_pool_frontier::{
    resource_divvy_pool_digest, ResourceDivvyPoolState, ResourcePoolBitMask,
};
use don_sim::rng::Random;

pub const RESOURCE_POOL_GET_WATER_VA: u32 = 0x0068_a4a0;
pub const RESOURCE_POOL_GET_WATER_RANDOM_CALL_VA: u32 = 0x0068_a4cd;
pub const RESOURCE_POOL_GET_WATER_RETURN_VA: u32 = 0x0068_a565;
pub const RESOURCE_POOL_GET_LATE_VA: u32 = 0x0068_a570;
pub const RESOURCE_POOL_GET_LATE_RANDOM_CALL_VA: u32 = 0x0068_a59d;
pub const RESOURCE_POOL_GET_LATE_RETURN_VA: u32 = 0x0068_a635;
pub const RESOURCE_POOL_GET_EARLY_VA: u32 = 0x0068_a640;
pub const RESOURCE_POOL_GET_EARLY_RANDOM_CALL_VA: u32 = 0x0068_a66c;
pub const RESOURCE_POOL_GET_EARLY_RETURN_VA: u32 = 0x0068_a6f6;
pub const RANDOM_GET_VA: u32 = 0x00a3_9d70;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourcePoolLane {
    Water,
    Late,
    Early,
}

impl ResourcePoolLane {
    pub const fn entry_va(self) -> u32 {
        match self {
            Self::Water => RESOURCE_POOL_GET_WATER_VA,
            Self::Late => RESOURCE_POOL_GET_LATE_VA,
            Self::Early => RESOURCE_POOL_GET_EARLY_VA,
        }
    }

    pub const fn random_call_va(self) -> u32 {
        match self {
            Self::Water => RESOURCE_POOL_GET_WATER_RANDOM_CALL_VA,
            Self::Late => RESOURCE_POOL_GET_LATE_RANDOM_CALL_VA,
            Self::Early => RESOURCE_POOL_GET_EARLY_RANDOM_CALL_VA,
        }
    }

    pub const fn return_va(self) -> u32 {
        match self {
            Self::Water => RESOURCE_POOL_GET_WATER_RETURN_VA,
            Self::Late => RESOURCE_POOL_GET_LATE_RETURN_VA,
            Self::Early => RESOURCE_POOL_GET_EARLY_RETURN_VA,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourcePoolSelectionDraw {
    pub call_va: u32,
    pub random_get_va: u32,
    pub low: i32,
    pub high: i32,
    pub state_before: i32,
    pub raw: i32,
    pub selected_index: usize,
    pub state_after: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourcePoolSelectionReceipt {
    pub lane: ResourcePoolLane,
    pub entry_va: u32,
    pub return_va: u32,
    pub random_state_before: i32,
    pub random_draws: Vec<ResourcePoolSelectionDraw>,
    pub random_state_after: i32,
    pub selected_index: usize,
    pub selected_good: i32,
    pub cleared_after_exhaustion: bool,
    pub pool_before: ResourceDivvyPoolState,
    pub pool_after: ResourceDivvyPoolState,
    pub pool_digest_before: u64,
    pub pool_digest_after: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResourcePoolSelectionError {
    InvalidProjection { lane: ResourcePoolLane },
    NoSelectableGood { lane: ResourcePoolLane },
}

fn lane<'a>(
    pool: &'a ResourceDivvyPoolState,
    lane: ResourcePoolLane,
) -> (&'a ResourcePoolBitMask, &'a [i32]) {
    match lane {
        ResourcePoolLane::Water => (&pool.water_bits, &pool.water_goods),
        ResourcePoolLane::Late => (&pool.late_bits, &pool.late_goods),
        ResourcePoolLane::Early => (&pool.early_bits, &pool.early_goods),
    }
}

fn lane_mut<'a>(
    pool: &'a mut ResourceDivvyPoolState,
    lane: ResourcePoolLane,
) -> (&'a mut ResourcePoolBitMask, &'a [i32]) {
    match lane {
        ResourcePoolLane::Water => (&mut pool.water_bits, &pool.water_goods),
        ResourcePoolLane::Late => (&mut pool.late_bits, &pool.late_goods),
        ResourcePoolLane::Early => (&mut pool.early_bits, &pool.early_goods),
    }
}

fn bit_is_set(mask: &ResourcePoolBitMask, index: usize) -> bool {
    mask.bytes[index >> 3] & (1u8 << (index & 7)) != 0
}

fn valid_projection(mask: &ResourcePoolBitMask, goods: &[i32]) -> bool {
    let expected_bytes = goods.len().saturating_add(7) / 8;
    if mask.bits != goods.len() as i32 || mask.bytes.len() != expected_bytes || goods.is_empty() {
        return false;
    }
    let used_bits = goods.len() & 7;
    used_bits == 0
        || mask
            .bytes
            .last()
            .is_some_and(|byte| byte & !((1u8 << used_bits) - 1) == 0)
}

/// Execute one exact selector transaction against the logical six-field pool projection.
///
/// The mutation is staged. Invalid projections and native non-terminating states (no unused,
/// non-`-1` entry) leave both the pool and RNG state untouched.
pub fn execute_resource_pool_selection(
    pool: &mut ResourceDivvyPoolState,
    lane_id: ResourcePoolLane,
    random: &mut Random,
) -> Result<ResourcePoolSelectionReceipt, ResourcePoolSelectionError> {
    let pool_before = pool.clone();
    let random_state_before = random.state();
    let (before_mask, before_goods) = lane(&pool_before, lane_id);
    if !valid_projection(before_mask, before_goods) {
        return Err(ResourcePoolSelectionError::InvalidProjection { lane: lane_id });
    }
    if !before_goods
        .iter()
        .enumerate()
        .any(|(index, good)| *good != -1 && !bit_is_set(before_mask, index))
    {
        return Err(ResourcePoolSelectionError::NoSelectableGood { lane: lane_id });
    }

    let mut staged_pool = pool_before.clone();
    let mut staged_random = random.clone();
    let mut random_draws = Vec::new();
    let selected_index;
    let selected_good;
    loop {
        let (_, goods) = lane(&staged_pool, lane_id);
        let index = if goods.len() <= 1 {
            0
        } else {
            let state_before = staged_random.state();
            let raw = staged_random.get(0, 0xffff);
            let selected_index = (raw % goods.len() as i32) as usize;
            random_draws.push(ResourcePoolSelectionDraw {
                call_va: lane_id.random_call_va(),
                random_get_va: RANDOM_GET_VA,
                low: 0,
                high: 0xffff,
                state_before,
                raw,
                selected_index,
                state_after: staged_random.state(),
            });
            selected_index
        };

        let (mask, goods) = lane_mut(&mut staged_pool, lane_id);
        if bit_is_set(mask, index) {
            continue;
        }
        mask.bytes[index >> 3] |= 1u8 << (index & 7);
        let good = goods[index];
        if good == -1 {
            continue;
        }
        selected_index = index;
        selected_good = good;
        break;
    }

    let (mask, goods) = lane_mut(&mut staged_pool, lane_id);
    let cleared_after_exhaustion = (0..goods.len()).all(|index| bit_is_set(mask, index));
    if cleared_after_exhaustion {
        mask.bytes.fill(0);
    }

    let receipt = ResourcePoolSelectionReceipt {
        lane: lane_id,
        entry_va: lane_id.entry_va(),
        return_va: lane_id.return_va(),
        random_state_before,
        random_draws,
        random_state_after: staged_random.state(),
        selected_index,
        selected_good,
        pool_digest_before: resource_divvy_pool_digest(&pool_before),
        pool_digest_after: resource_divvy_pool_digest(&staged_pool),
        pool_before,
        pool_after: staged_pool.clone(),
        cleared_after_exhaustion,
    };
    *pool = staged_pool;
    *random = staged_random;
    Ok(receipt)
}

/// Re-execute and compare a selector receipt, including its complete concrete pool states.
pub fn validate_resource_pool_selection(receipt: &ResourcePoolSelectionReceipt) -> bool {
    let mut pool = receipt.pool_before.clone();
    let mut random = Random::new(receipt.random_state_before);
    execute_resource_pool_selection(&mut pool, receipt.lane, &mut random)
        .is_ok_and(|expected| expected == *receipt)
}
