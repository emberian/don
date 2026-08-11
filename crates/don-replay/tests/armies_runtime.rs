#[path = "../src/armies_runtime.rs"]
mod armies_runtime;

use armies_runtime::{
    armies_walk_bytes, ArmiesRuntimeError, ARMIES_WALK_VA, ARMY_OWNER_SLOTS, ARMY_WALK_VA,
    CHECK_ARMIES_VA, INITIAL_ARMIES_WALK_BYTES, PTR_ARRAY_NONEMPTY_FIXED_BYTES,
};
use don_replay::check_all::WALKERS;
use don_sim::systems::armies::{Armies, ARMIES_PER_PLAYER, ARMY_WALK_HEAD};

#[test]
fn the_orphan_entry_point_is_not_a_replay_checksum_channel() {
    assert_eq!(CHECK_ARMIES_VA, 0x0093_6cf0);
    assert_eq!(ARMIES_WALK_VA, 0x006f_3700);
    assert_eq!(ARMY_WALK_VA, 0x006f_9850);
    assert!(WALKERS.iter().all(|walker| walker.name != "armies"));
}

#[test]
fn canonical_init_emits_both_container_history_passes_and_pointer_presence() {
    let armies = Armies::new();
    let (bytes, value) = armies_walk_bytes(&armies).unwrap();
    assert_eq!(ARMY_OWNER_SLOTS, 8);
    assert_eq!(ARMIES_PER_PLAYER, 16);
    assert_eq!(INITIAL_ARMIES_WALK_BYTES, 520);
    assert_eq!(bytes.len(), INITIAL_ARMIES_WALK_BYTES);
    assert_eq!(value.bytes_walked, 520);
    assert_eq!(value.owner_lists, 8);
    assert_eq!(value.pointer_slots, 128);
    assert_eq!(value.live_armies, 0);

    let owner_bytes =
        PTR_ARRAY_NONEMPTY_FIXED_BYTES + ARMIES_PER_PLAYER + ARMIES_PER_PLAYER * ARMY_WALK_HEAD;
    assert_eq!(owner_bytes, 65);
    for owner in 0..ARMY_OWNER_SLOTS {
        let base = owner * owner_bytes;
        assert_eq!(&bytes[base..base + 4], &16i32.to_le_bytes());
        assert_eq!(&bytes[base + 4..base + 8], &16i32.to_le_bytes());
        assert_eq!(&bytes[base + 8..base + 10], &(-1i16).to_le_bytes());
        assert_eq!(bytes[base + 10], 0);
        assert_eq!(&bytes[base + 11..base + 27], &[1u8; 16]);
        assert_eq!(&bytes[base + 27..base + 31], &16i32.to_le_bytes());
        assert_eq!(&bytes[base + 31..base + 33], &(-1i16).to_le_bytes());
        assert_eq!(&bytes[base + 33..base + 65], &[0u8; 32]);
    }
}

#[test]
fn a_live_army_hashes_its_full_image_while_invalid_tail_history_is_dormant() {
    let mut armies = Armies::new();
    let live = &mut armies.lists[3][4];
    live.valid = 1;
    live.army = 4;
    live.who = 3;
    live.status = 0x1122_3344;
    live.list[0] = 197;
    live.num_groups = 1;

    let (before_bytes, before) = armies_walk_bytes(&armies).unwrap();
    assert_eq!(
        before.bytes_walked,
        (INITIAL_ARMIES_WALK_BYTES + 150) as u64
    );
    assert_eq!(before.live_armies, 1);

    // Three initial 65-byte owner streams, then this owner's 33-byte container history
    // and four invalid two-byte Army heads.
    let live_offset = 3 * 65 + 33 + 4 * 2;
    assert_eq!(
        &before_bytes[live_offset..live_offset + 152],
        &armies.lists[3][4].image()
    );

    armies.lists[7][15].status ^= 0x55aa_55aa;
    let (dormant_bytes, dormant) = armies_walk_bytes(&armies).unwrap();
    assert_eq!(dormant_bytes, before_bytes);
    assert_eq!(dormant.checksum, before.checksum);

    armies.lists[3][4].status ^= 1;
    let (mutated_bytes, mutated) = armies_walk_bytes(&armies).unwrap();
    assert_ne!(mutated_bytes, before_bytes);
    assert_ne!(mutated.checksum, before.checksum);
}

#[test]
fn erased_container_history_is_admitted_only_at_the_canonical_post_init_shape() {
    let mut missing_owner = Armies::new();
    missing_owner.lists.pop();
    assert_eq!(
        armies_walk_bytes(&missing_owner),
        Err(ArmiesRuntimeError::OwnerListCount {
            expected: 8,
            actual: 7,
        })
    );

    let mut short_list = Armies::new();
    short_list.lists[5].pop();
    assert_eq!(
        armies_walk_bytes(&short_list),
        Err(ArmiesRuntimeError::ArmySlotCount {
            owner: 5,
            expected: 16,
            actual: 15,
        })
    );
}
