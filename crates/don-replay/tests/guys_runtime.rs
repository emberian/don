#[path = "../src/guys_runtime.rs"]
mod guys_runtime;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use don_replay::checksum::Channel;
use don_replay::replay::{corpus, Replay};
use don_sim::systems::groups_guys::{CheckSum, GuyData, UnitGuys, GUY_WALK_LEN};
use don_sim::world::World;
use guys_runtime::{
    check_world_guys, guy_array_walk_bytes, GuyArrayWalkError, GuyWalkFacts, GuysDriverFacts,
    GuysRuntimeError, GuysWalkAuthority, CHECK_GUYS_VA, GUYS_CHANNEL_OWNER_SLOTS,
    GUY_ARRAY_WALK_VA, GUY_DATA_WALK_VA, NONEMPTY_GUY_ARRAY_FIXED_BYTES,
};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn one_guy(who: i8, o: i16, tag: i32) -> UnitGuys {
    let guy = GuyData {
        ty: tag,
        x: tag.wrapping_mul(17),
        who,
        o,
        guy_num: 0,
        ..GuyData::default()
    };
    UnitGuys {
        guys: vec![Some(guy)],
        size: 1,
        increment: -1,
        flags: 0,
        guy_mark: 1,
    }
}

fn spawn(world: &mut World, owner: u8, ty: i32) -> (don_sim::Handle, usize, i16) {
    let handle = world.spawn_typed(owner, ty).unwrap();
    let row = world.row_of(handle).unwrap();
    let o = world.units.o()[row];
    (handle, row, o)
}

fn install(
    authority: &mut GuysWalkAuthority,
    world: &mut World,
    handle: don_sim::Handle,
    row: usize,
    guys: UnitGuys,
) {
    world.units.guy_mark_mut()[row] = guys.guy_mark;
    assert!(authority.install(handle, GuyWalkFacts::new(guys)).is_none());
}

#[test]
fn pointer_array_stream_preserves_presence_then_repeats_history_then_recurses() {
    assert_eq!(CHECK_GUYS_VA, 0x0093_7430);
    assert_eq!(GUY_ARRAY_WALK_VA, 0x0046_df30);
    assert_eq!(GUY_DATA_WALK_VA, 0x005e_0210);
    let mut live = GuyData {
        ty: 0x1122_3344,
        x: 0x0102_0304,
        who: 3,
        o: 19,
        guy_num: 0,
        ..GuyData::default()
    };
    live.guy_flags = 0x5566;
    let guys = UnitGuys {
        guys: vec![Some(live), None],
        size: 7,
        increment: -3,
        flags: 0x63,
        guy_mark: 1,
    };

    let (bytes, value) = guy_array_walk_bytes(&guys, 3, 19).unwrap();
    assert_eq!(
        bytes.len(),
        NONEMPTY_GUY_ARRAY_FIXED_BYTES + 2 + GUY_WALK_LEN
    );
    assert_eq!(&bytes[0..4], &2i32.to_le_bytes());
    assert_eq!(&bytes[4..8], &7i32.to_le_bytes());
    assert_eq!(&bytes[8..10], &(-3i16).to_le_bytes());
    assert_eq!(bytes[10], 0x23, "bit 0x40 is cleared before hashing");
    assert_eq!(&bytes[11..13], &[1, 0], "pointer presence pass");
    assert_eq!(&bytes[13..17], &7i32.to_le_bytes());
    assert_eq!(&bytes[17..19], &(-3i16).to_le_bytes());
    assert_eq!(&bytes[19..], &live.walk_bytes());
    assert_eq!(value.guys_walked, 1);
    assert_eq!(value.null_slots, 1);
    assert_eq!(value.flags_after, 0x23);

    let mut existing = CheckSum::default();
    guys.walk(&mut existing);
    assert_eq!(value.checksum, existing.value);
}

#[test]
fn the_zero_length_arm_reads_no_dormant_array_history() {
    let a = UnitGuys {
        size: -999,
        increment: i16::MIN,
        flags: 0xff,
        guy_mark: -7,
        ..UnitGuys::default()
    };
    let b = UnitGuys::default();
    let (a_bytes, a_value) = guy_array_walk_bytes(&a, 8, 1999).unwrap();
    let (b_bytes, b_value) = guy_array_walk_bytes(&b, 0, 0).unwrap();
    assert_eq!(a_bytes, 0i32.to_le_bytes());
    assert_eq!(a_bytes, b_bytes);
    assert_eq!(a_value.checksum, b_value.checksum);
    assert_eq!(a_value.bytes_walked, 4);
    assert_eq!(a_value.flags_after, 0xff, "empty arrays return before masking");
}

#[test]
fn impossible_dynamic_shapes_and_slot_identity_fail_closed() {
    let too_small = UnitGuys {
        guys: vec![None],
        size: 0,
        guy_mark: 0,
        ..UnitGuys::default()
    };
    assert_eq!(
        guy_array_walk_bytes(&too_small, 0, 0),
        Err(GuyArrayWalkError::LengthExceedsCapacity { length: 1, size: 0 })
    );

    let missing_prefix = UnitGuys {
        guys: vec![None],
        size: 1,
        guy_mark: 1,
        ..UnitGuys::default()
    };
    assert_eq!(
        guy_array_walk_bytes(&missing_prefix, 0, 0),
        Err(GuyArrayWalkError::MissingSquadPrefix {
            slot: 0,
            guy_mark: 1,
        })
    );

    let wrong = one_guy(2, 17, 8);
    assert!(matches!(
        guy_array_walk_bytes(&wrong, 2, 18),
        Err(GuyArrayWalkError::GuyIdentityMismatch {
            slot: 0,
            expected_o: 18,
            actual_o: 17,
            ..
        })
    ));
}

#[test]
fn channel_is_owner_major_reaches_ninth_owner_and_excludes_tenth() {
    assert_eq!(GUYS_CHANNEL_OWNER_SLOTS, 9);
    let mut world = World::with_capacity(8, 17);
    // Dense order is deliberately the reverse of checksum owner order.
    let (h1, r1, o1) = spawn(&mut world, 1, 101);
    let (h0, r0, o0) = spawn(&mut world, 0, 202);
    let (h8, r8, o8) = spawn(&mut world, 8, 303);
    let (h9, r9, o9) = spawn(&mut world, 9, 404);
    let mut authority = GuysWalkAuthority::default();
    assert!(authority.is_empty());
    install(&mut authority, &mut world, h1, r1, one_guy(1, o1, 101));
    install(&mut authority, &mut world, h0, r0, one_guy(0, o0, 202));
    install(&mut authority, &mut world, h8, r8, one_guy(8, o8, 303));
    install(&mut authority, &mut world, h9, r9, one_guy(9, o9, 404));
    assert_eq!(authority.len(), 4);

    let b0 = guy_array_walk_bytes(&authority.get(h0).unwrap().guys, 0, o0)
        .unwrap()
        .0;
    let b1 = guy_array_walk_bytes(&authority.get(h1).unwrap().guys, 1, o1)
        .unwrap()
        .0;
    let b8 = guy_array_walk_bytes(&authority.get(h8).unwrap().guys, 8, o8)
        .unwrap()
        .0;
    let mut expected = 1;
    for bytes in [&b0, &b1, &b8] {
        expected = don_sim::checksum::adler32(expected, bytes);
    }
    let dense = [&b1, &b0, &b8]
        .into_iter()
        .fold(1, |sum, bytes| don_sim::checksum::adler32(sum, bytes));

    let result = check_world_guys(&world, GuysDriverFacts::all_active(), &mut authority).unwrap();
    assert_eq!(result.checksum, expected);
    assert_ne!(result.checksum, dense);
    assert_eq!(result.units_walked, 3);
    assert_eq!(result.guys_walked, 3);
    assert_eq!(result.skipped_outside_walk, 1);
    assert_eq!(result.registry_entries, 4);

    authority.get_mut(h8).unwrap().guys.guys[0]
        .as_mut()
        .unwrap()
        .x += 1;
    let owner_eight_mutated =
        check_world_guys(&world, GuysDriverFacts::all_active(), &mut authority).unwrap();
    assert_ne!(
        owner_eight_mutated.checksum, result.checksum,
        "owner-8 Animal Guys are inside the checksum walk"
    );

    authority.get_mut(h9).unwrap().guys.guys[0]
        .as_mut()
        .unwrap()
        .x += 1;
    let owner_nine_mutated =
        check_world_guys(&world, GuysDriverFacts::all_active(), &mut authority).unwrap();
    assert_eq!(
        owner_nine_mutated.checksum, owner_eight_mutated.checksum,
        "owner-9 Animal Guys are outside the checksum walk"
    );
}

#[test]
fn stable_identity_survives_dense_compaction_and_stale_authority_is_refused() {
    let mut world = World::with_capacity(4, 23);
    let (removed, removed_row, removed_o) = spawn(&mut world, 0, 11);
    let (survivor, survivor_row, survivor_o) = spawn(&mut world, 1, 22);
    assert_eq!(survivor_row, 1);
    let mut authority = GuysWalkAuthority::default();
    install(
        &mut authority,
        &mut world,
        removed,
        removed_row,
        one_guy(0, removed_o, 11),
    );
    install(
        &mut authority,
        &mut world,
        survivor,
        survivor_row,
        one_guy(1, survivor_o, 22),
    );

    assert!(world.despawn(removed));
    assert_eq!(world.row_of(survivor), Some(0));
    assert_eq!(
        check_world_guys(&world, GuysDriverFacts::all_active(), &mut authority),
        Err(GuysRuntimeError::AuthorityForDeadUnit {
            id: removed.id,
            generation: removed.generation,
        })
    );

    authority.remove(removed);
    let result = check_world_guys(&world, GuysDriverFacts::all_active(), &mut authority).unwrap();
    assert_eq!(result.units_walked, 1);
    assert_eq!(result.guys_walked, 1);
}

#[test]
fn inactive_gates_do_not_demand_authority() {
    let mut world = World::with_capacity(3, 29);
    let (_inactive_leader, _row0, _o0) = spawn(&mut world, 0, 1);
    let (_inactive_unit, row1, _o1) = spawn(&mut world, 1, 2);
    world.units.set_flags(row1, 0);
    let driver = GuysDriverFacts {
        objects_valid: true,
        leader_active: [false; GUYS_CHANNEL_OWNER_SLOTS],
    };
    let result = check_world_guys(&world, driver, &mut GuysWalkAuthority::default()).unwrap();
    assert_eq!(result.checksum, 1);
    assert_eq!(result.skipped_inactive_leader, 1);
    assert_eq!(result.skipped_inactive_unit, 1);

    let invalid = GuysDriverFacts {
        objects_valid: false,
        leader_active: [true; GUYS_CHANNEL_OWNER_SLOTS],
    };
    let result = check_world_guys(&world, invalid, &mut GuysWalkAuthority::default()).unwrap();
    assert_eq!(result.checksum, 1);
    assert_eq!(
        result.registry_entries, 0,
        "retail returns before reading bands"
    );
}

#[test]
fn ptrarray_flag_commit_is_atomic_across_the_channel() {
    let mut world = World::with_capacity(3, 31);
    let (first, first_row, first_o) = spawn(&mut world, 0, 10);
    let (second, second_row, second_o) = spawn(&mut world, 0, 20);
    let mut first_guys = one_guy(0, first_o, 10);
    first_guys.flags = 0x40;
    let mut invalid_second = one_guy(0, second_o, 20);
    invalid_second.size = 0;
    let mut authority = GuysWalkAuthority::default();
    install(&mut authority, &mut world, first, first_row, first_guys);
    install(
        &mut authority,
        &mut world,
        second,
        second_row,
        invalid_second,
    );

    assert!(matches!(
        check_world_guys(
            &world,
            GuysDriverFacts::all_active(),
            &mut authority
        ),
        Err(GuysRuntimeError::GuyArrayWalk {
            row,
            source: GuyArrayWalkError::LengthExceedsCapacity { .. },
            ..
        }) if row == second_row
    ));
    assert_eq!(
        authority.get(first).unwrap().guys.flags,
        0x40,
        "later preflight failure must not partially commit an earlier flags clear"
    );

    authority.get_mut(second).unwrap().guys.size = 1;
    let result = check_world_guys(&world, GuysDriverFacts::all_active(), &mut authority).unwrap();
    assert_eq!(result.arrays_masked, 1);
    assert_eq!(authority.get(first).unwrap().guys.flags, 0);
}

#[test]
fn a_normal_world_unit_exposes_the_missing_guy_owner_instead_of_hashing_zero() {
    let mut world = World::with_capacity(1, 37);
    let (handle, row, _o) = spawn(&mut world, 0, 77);
    world.units.guy_mark_mut()[row] = 1;
    assert_eq!(
        check_world_guys(
            &world,
            GuysDriverFacts::all_active(),
            &mut GuysWalkAuthority::default()
        ),
        Err(GuysRuntimeError::MissingWalkAuthority {
            row,
            owner: 0,
            o: 0,
            handle,
        })
    );
}

#[test]
fn local_corpus_measures_the_first_turn_deadline_without_fitting_a_guy_image() {
    let files = corpus(&repo_root());
    if files.is_empty() {
        eprintln!(
            "\n  SKIPPED — NOT A PASS. No local replay corpus; no Guys target was measured.\n"
        );
        return;
    }

    let mut decoded = 0usize;
    let mut decode_failures = 0usize;
    let mut checksummed = 0usize;
    let mut checksum_turns = 0usize;
    let mut first_values = BTreeSet::new();
    for path in &files {
        let Ok(replay) = Replay::open(path) else {
            decode_failures += 1;
            continue;
        };
        decoded += 1;
        let mut first = None;
        for turn in &replay.turns {
            if let Some((_, sums)) = turn.any_checksums() {
                checksum_turns += 1;
                first.get_or_insert(sums.get(Channel::Guys));
            }
        }
        if let Some(first) = first {
            checksummed += 1;
            assert_ne!(
                first,
                1,
                "{} carries a vacuous first Guys value",
                path.display()
            );
            first_values.insert(first);
        }
    }
    eprintln!(
        "  Guys corpus: {} files, {decoded} decoded, {decode_failures} legacy decode failures, {checksummed} checksum-bearing, {checksum_turns} checksum turns, {} distinct first values",
        files.len(),
        first_values.len()
    );
    assert!(decoded > 0);
    assert!(checksummed > 0);
    if decoded == 61 {
        assert_eq!(
            checksummed, 21,
            "the frozen 61-recording decoded corpus changed"
        );
        assert_eq!(
            checksum_turns, 222_938,
            "the frozen Guys comparison count changed"
        );
        assert_eq!(
            first_values.len(),
            21,
            "first Guys state stopped being recording-specific"
        );
    }
}
