// SPDX-License-Identifier: GPL-3.0-or-later

use don_replay::place_resources_pool_frontier::{
    resource_divvy_pool_digest, ResourceDivvyPoolState, ResourcePoolBitMask,
};
use don_replay::resource_divvy_pool_selection_frontier::{
    execute_resource_pool_selection, validate_resource_pool_selection, ResourcePoolLane,
    ResourcePoolSelectionError, RESOURCE_POOL_GET_EARLY_RANDOM_CALL_VA,
};
use don_sim::rng::Random;

fn mask(bits: usize, bytes: &[u8]) -> ResourcePoolBitMask {
    ResourcePoolBitMask {
        bits: bits as i32,
        bytes: bytes.to_vec(),
    }
}

fn pool() -> ResourceDivvyPoolState {
    ResourceDivvyPoolState {
        early_bits: mask(3, &[0]),
        early_goods: vec![6, 7, 8],
        late_bits: mask(2, &[0]),
        late_goods: vec![20, 21],
        water_bits: mask(1, &[0]),
        water_goods: vec![30],
    }
}

#[test]
fn singleton_selects_without_rng_and_clears_the_exhausted_mask() {
    let mut pool = pool();
    let before = pool.clone();
    let mut random = Random::new(0x1234_5678);

    let receipt =
        execute_resource_pool_selection(&mut pool, ResourcePoolLane::Water, &mut random).unwrap();

    assert_eq!(receipt.selected_index, 0);
    assert_eq!(receipt.selected_good, 30);
    assert!(receipt.random_draws.is_empty());
    assert!(receipt.cleared_after_exhaustion);
    assert_eq!(pool, before);
    assert_eq!(random.state(), 0x1234_5678);
    assert!(validate_resource_pool_selection(&receipt));
}

#[test]
fn multi_entry_lane_records_exact_rng_index_and_concrete_digest() {
    let mut pool = pool();
    let mut random = Random::new(0x1234_5678);

    let receipt =
        execute_resource_pool_selection(&mut pool, ResourcePoolLane::Early, &mut random).unwrap();

    assert_eq!(receipt.random_draws.len(), 1);
    assert_eq!(
        receipt.random_draws[0].call_va,
        RESOURCE_POOL_GET_EARLY_RANDOM_CALL_VA
    );
    assert_eq!(
        receipt.random_draws[0].selected_index,
        receipt.selected_index
    );
    assert_eq!(pool, receipt.pool_after);
    assert_eq!(
        receipt.pool_digest_after,
        resource_divvy_pool_digest(&receipt.pool_after)
    );
    assert!(validate_resource_pool_selection(&receipt));
}

#[test]
fn used_and_minus_one_entries_retry_after_marking_the_invalid_entry() {
    let mut pool = pool();
    pool.early_goods = vec![-1, 7, 8];
    pool.early_bits.bytes[0] = 0b0000_0010;
    // Seed zero visits used index 1, sentinel index 0, used index 1, then valid index 2.
    let mut random = Random::new(0);

    let receipt =
        execute_resource_pool_selection(&mut pool, ResourcePoolLane::Early, &mut random).unwrap();

    assert!(receipt.random_draws.len() >= 2);
    assert_ne!(receipt.selected_good, -1);
    assert_ne!(receipt.selected_good, 7);
    assert!(receipt.cleared_after_exhaustion);
    assert_eq!(receipt.pool_after.early_bits.bytes, [0]);
}

#[test]
fn malformed_projection_and_nonterminating_native_state_are_atomic_errors() {
    let mut malformed = pool();
    malformed.early_bits.bits = 4;
    let malformed_before = malformed.clone();
    let mut random = Random::new(9);
    assert_eq!(
        execute_resource_pool_selection(&mut malformed, ResourcePoolLane::Early, &mut random),
        Err(ResourcePoolSelectionError::InvalidProjection {
            lane: ResourcePoolLane::Early
        })
    );
    assert_eq!(malformed, malformed_before);
    assert_eq!(random.state(), 9);

    let mut exhausted = pool();
    exhausted.early_bits.bytes[0] = 0b0000_0111;
    let exhausted_before = exhausted.clone();
    assert_eq!(
        execute_resource_pool_selection(&mut exhausted, ResourcePoolLane::Early, &mut random),
        Err(ResourcePoolSelectionError::NoSelectableGood {
            lane: ResourcePoolLane::Early
        })
    );
    assert_eq!(exhausted, exhausted_before);
    assert_eq!(random.state(), 9);
}

#[test]
fn substituted_post_pool_state_does_not_validate() {
    let mut pool = pool();
    let mut random = Random::new(0x1234_5678);
    let mut receipt =
        execute_resource_pool_selection(&mut pool, ResourcePoolLane::Late, &mut random).unwrap();
    receipt.pool_after.late_bits.bytes[0] ^= 1;

    assert!(!validate_resource_pool_selection(&receipt));
}
