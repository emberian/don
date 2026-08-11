// SPDX-License-Identifier: GPL-3.0-or-later
//! The `Groups` save owner, driven through the real tick.
//!
//! `Groups::walk_data` `0x00713E30` is the retail save stream's group section. Until this
//! landed, `save_load::reject_unsupported` refused any `Sim` whose pool differed from
//! `Groups::default()`, which is every `Sim` that has ever commanded a group.

use don_sim::objects::Band;
use don_sim::systems::groups_guys::{
    CheckSum, GroupData, Groups, MemberState, GROUPS_PER_PLAYER, GROUP_MAX_MEMBERS, NUM_GROUPS,
};
use don_sim::systems::player_setup::ManualPlayerSetup;
use don_sim::systems::save_load::{load_sim, save_sim, SaveError};
use don_sim::tick::Sim;

/// `CheckSums::check_groups` `0x00937530` over the live pool — the channel a desync would
/// show up in, and a different traversal from the save section.
fn groups_channel(sim: &Sim) -> u32 {
    let mut cs = CheckSum::default();
    sim.groups.check_groups(&mut cs);
    cs.value
}

/// The `DoNSave` container, re-walked here rather than borrowed from the writer, so the
/// section-size assertions below are an independent measurement.
fn section_range(bytes: &[u8], want: u16) -> std::ops::Range<usize> {
    const MAGIC: usize = 8;
    let root = &bytes[MAGIC..];
    assert_eq!(
        u32::from_le_bytes(root[0..4].try_into().unwrap()) as usize,
        root.len()
    );
    let children = u16::from_le_bytes(root[6..8].try_into().unwrap());
    let mut at = 8usize;
    for _ in 0..children {
        let size = u32::from_le_bytes(root[at..at + 4].try_into().unwrap()) as usize;
        let id = u16::from_le_bytes(root[at + 4..at + 6].try_into().unwrap());
        if id == want {
            return MAGIC + at + 8..MAGIC + at + size;
        }
        at += size;
    }
    panic!("section {want:#06x} absent");
}

fn section(bytes: &[u8], want: u16) -> &[u8] {
    &bytes[section_range(bytes, want)]
}

/// The `GROUPS` chunk id.
const GROUPS_SECTION: u16 = 0x0009;

/// `load_sim`'s error, without requiring `Sim: Debug`.
fn load_error(bytes: &[u8]) -> SaveError {
    match load_sim(bytes) {
        Ok(_) => panic!("expected a rejected load"),
        Err(error) => error,
    }
}

/// Two active players, real spawned units, and a real `Group` per player assembled by
/// `Group::add` `0x00714350` from that owner's own Unit-band offsets — which is exactly
/// the `o` retail stores in `GroupData::list`.
fn commanded_sim() -> Sim {
    let mut sim = Sim::new(0x51a7_2026, 8);
    let mut request = ManualPlayerSetup {
        active_mask: 0x03,
        team_style: 1,
        local_player_setup_slot: 0,
        ..ManualPlayerSetup::default()
    };
    request.teams[0] = 0;
    request.teams[1] = 1;
    sim.start_manual_player_setup(request).unwrap();
    assert!(sim.leaders[0].active && sim.leaders[1].active);

    for who in 0..2usize {
        for i in 0..4i32 {
            sim.spawn_unit(who, 17 + i, 768 + i * 384, 1536 + who as i32 * 768, 4)
                .unwrap();
        }
    }

    for who in 0..2usize {
        let members = sim.world.objects.slot(who).band(Band::Unit).len();
        assert_eq!(members, 4);
        let frame = sim.world.frame;
        // Slot 5 of this owner's 64-stride window; `Groups::process` reaches slot 5 on the
        // sixth frame, so a 70-frame resume visits it more than once.
        let slot = Groups::index(who, 5);
        let g = &mut sim.groups.list[slot];
        for o in 0..members {
            assert!(g.add(o as i16, who as u8, false, 0x11 << who, frame));
        }
        g.form = 2;
        g.form_num = g.num;
        g.army = 3 + who as i32;
        g.priority = 1;
        g.o_dist = 640;
        g.o_angle = 0x2000_0000;
        // A non-zero speed is what `Group::compute_speed` will overwrite on the first
        // `Groups::process` pass that reaches this slot; without it a 70-frame resume
        // would not observably touch the pool.
        g.speed = 900 + who as i32;
        // Deliberately not equal to `speed`: `Group::compute_speed` will collapse them, so
        // an unequal pair is what proves the two adjacent header words are decoded as
        // themselves and not as each other.
        g.new_speed = g.speed + 7;
        for i in 0..g.num as usize {
            g.off_x[i] = i as i32 - 1;
            g.off_y[i] = 2 * i as i32;
            g.angles[i] = (10 + i) as i8;
        }
        g.update_positions(0x2000_0000);
        sim.groups.last_group[who] = slot as i32;
    }
    sim
}

#[test]
fn a_live_group_pool_round_trips_and_resumes_through_seventy_real_frames() {
    let mut original = commanded_sim();
    let before = groups_channel(&original);
    assert_ne!(
        before,
        groups_channel(&Sim::new(0x51a7_2026, 8)),
        "the fixture pool is indistinguishable from a fresh one"
    );

    let bytes = save_sim(&original).unwrap();
    let mut loaded = load_sim(&bytes).unwrap();
    assert_eq!(loaded.groups.list, original.groups.list);
    assert_eq!(loaded.groups.last_group, original.groups.last_group);
    assert_eq!(loaded.groups.proc_group, original.groups.proc_group);
    assert_eq!(groups_channel(&loaded), before);
    // Loading and immediately resaving must reproduce the stream byte for byte.
    assert_eq!(save_sim(&loaded).unwrap(), bytes);

    // 70 > 64, so `Groups::process` `0x006FA210` reaches every slot of both active
    // players: `Group::normalize` and `Group::compute_speed` run on the restored pool.
    for frame in 0..70 {
        original.do_frame();
        loaded.do_frame();
        assert_eq!(
            groups_channel(&loaded),
            groups_channel(&original),
            "groups channel diverged at resumed frame {frame}"
        );
        assert_eq!(loaded.channel_digest(), original.channel_digest());
    }
    assert_eq!(loaded.groups.list, original.groups.list);
    assert_eq!(loaded.groups.proc_group, original.groups.proc_group);
    assert_ne!(
        groups_channel(&original),
        before,
        "70 frames of Groups::process changed no group byte, so the resume proves nothing"
    );
}

#[test]
fn the_section_carries_exactly_the_bytes_group_walk_data_covers() {
    // A fresh pool: 512 slots, every one `num == 0`, so `Group::walk_data`'s guarded
    // member walks are all skipped and only the 72-byte headers are emitted.
    let empty = save_sim(&Sim::new(0x91, 4)).unwrap();
    let empty_section = section(&empty, GROUPS_SECTION);
    assert_eq!(NUM_GROUPS, 512);
    assert_eq!(empty_section.len(), 4 + NUM_GROUPS * 72 + 32);
    // 512*72 + 32 == 36,896 is the independently derived initial size of the `groups`
    // checksum channel; the save section is that plus the `Array<Group>` length word.
    assert_eq!(NUM_GROUPS * 72 + 32, 36_896);

    // Eight members across two groups: each costs 2 (list) + 4*4 (off/curr) + 1 (angles).
    let live = save_sim(&commanded_sim()).unwrap();
    let live_section = section(&live, GROUPS_SECTION);
    assert_eq!(live_section.len(), empty_section.len() + 8 * (2 + 16 + 1));

    // The header the section emits per group is the retail `Group::walk_data` image.
    let sim = commanded_sim();
    let slot = Groups::index(0, 5);
    // Every earlier slot is empty, so the headers up to `slot` are contiguous.
    assert!(sim.groups.list[..slot].iter().all(|g| g.num == 0));
    let header = &live_section[4 + slot * 72..4 + slot * 72 + 72];
    assert_eq!(header, sim.groups.list[slot].header_bytes());
}

#[test]
fn member_slots_at_or_past_num_are_not_state() {
    // Produce `form_num > num` the way the engine does: `Group::normalize` `0x00711540`
    // compacts the member arrays and decrements `num` without touching `form_num`.
    let mut clean = commanded_sim();
    let mut dirty = commanded_sim();
    let slot = Groups::index(0, 5);
    let victim = clean.groups.list[slot].list[3];
    let drop_victim = |o: i16| {
        if o == victim {
            MemberState::Dead
        } else {
            MemberState::Keep
        }
    };
    for sim in [&mut clean, &mut dirty] {
        sim.groups.list[slot].normalize(&drop_victim);
    }
    let n = clean.groups.list[slot].num as usize;
    let form_num = clean.groups.list[slot].form_num as usize;
    assert_eq!((n, form_num), (3, 4));

    // Fill the one region retail's own walk does not cover.
    for i in n..GROUP_MAX_MEMBERS {
        let g = &mut dirty.groups.list[slot];
        g.off_x[i] = 0x7f00 + i as i32;
        g.off_y[i] = -(i as i32) - 1;
        g.curr_x[i] = i32::MIN + i as i32;
        g.curr_y[i] = i32::MAX - i as i32;
        g.angles[i] = -(i as i8) - 1;
        g.list[i] = 0x2000 + i as i16;
    }

    assert_eq!(groups_channel(&dirty), groups_channel(&clean));
    assert_eq!(save_sim(&dirty).unwrap(), save_sim(&clean).unwrap());

    // `Group::update_positions` `0x007138A0` is the only shipped read past `num`: its loop
    // bound is `form_num`. Its excursion writes only into the same uncovered region.
    for sim in [&mut clean, &mut dirty] {
        sim.groups.list[slot].update_positions(0x2000_0000);
    }
    assert_eq!(
        clean.groups.list[slot].curr_x[..n],
        dirty.groups.list[slot].curr_x[..n]
    );
    assert_ne!(
        clean.groups.list[slot].curr_x[n], dirty.groups.list[slot].curr_x[n],
        "the excursion past num is not reachable here, so this test proves nothing"
    );
    assert_eq!(groups_channel(&dirty), groups_channel(&clean));

    // And the next `Group::add` resets the slot before it can ever be observed.
    let frame = dirty.world.frame;
    assert!(dirty.groups.list[slot].add(victim, 0, false, 0, frame));
    let g = &dirty.groups.list[slot];
    assert_eq!(
        (
            g.off_x[n],
            g.off_y[n],
            g.curr_x[n],
            g.curr_y[n],
            g.angles[n]
        ),
        (0, 0, 0, 0, 0)
    );
}

#[test]
fn a_pool_without_a_setup_owner_round_trips_across_forty_resumed_frames() {
    // No `PlayerSetup` transaction, so no active leader and no frame-zero restriction:
    // this isolates the `GROUPS` section from the setup owner's own boundary.
    let mut original = Sim::new(0x3456_789a, 4);
    original.spawn_unit(2, 17, 768, 1536, 4).unwrap();
    original.spawn_unit(2, 19, 1536, 1536, 4).unwrap();
    let slot = Groups::index(2, 11);
    let frame = original.world.frame;
    let g = &mut original.groups.list[slot];
    assert!(g.add(0, 2, false, 0x40, frame));
    assert!(g.add(1, 2, true, 0x80, frame));
    g.march = 1;
    g.facing = 1;
    original.groups.last_group[2] = slot as i32;

    for _ in 0..40 {
        original.do_frame();
    }
    let bytes = save_sim(&original).unwrap();
    let mut loaded = load_sim(&bytes).unwrap();
    assert_eq!(loaded.groups.list, original.groups.list);
    assert_eq!(loaded.groups.last_group, original.groups.last_group);
    assert_eq!(loaded.groups.proc_group, original.groups.proc_group);
    assert_eq!(save_sim(&loaded).unwrap(), bytes);

    for _ in 0..40 {
        original.do_frame();
        loaded.do_frame();
        assert_eq!(groups_channel(&loaded), groups_channel(&original));
        assert_eq!(loaded.channel_digest(), original.channel_digest());
    }
    assert_eq!(loaded.groups.list[slot], original.groups.list[slot]);
}

#[test]
fn structurally_impossible_pools_fail_closed_on_both_sides() {
    let mut over_capacity = commanded_sim();
    over_capacity.groups.list[0].num = GROUP_MAX_MEMBERS as i32 + 1;
    assert_eq!(
        save_sim(&over_capacity),
        Err(SaveError::Invalid("group slot shape"))
    );

    let mut bad_owner = commanded_sim();
    bad_owner.groups.list[7].who = 8;
    assert_eq!(
        save_sim(&bad_owner),
        Err(SaveError::Invalid("group slot shape"))
    );

    let mut bad_form = commanded_sim();
    bad_form.groups.list[3].form_num = -1;
    assert_eq!(
        save_sim(&bad_form),
        Err(SaveError::Invalid("group slot shape"))
    );

    let mut bad_cursor = commanded_sim();
    bad_cursor.groups.proc_group = GROUPS_PER_PLAYER as i32;
    assert_eq!(
        save_sim(&bad_cursor),
        Err(SaveError::Invalid("groups proc_group"))
    );

    // A pool whose cardinality is not the retail 8 x 64.
    let mut short_pool = commanded_sim();
    short_pool.groups.list.truncate(NUM_GROUPS - 1);
    assert_eq!(
        save_sim(&short_pool),
        Err(SaveError::Invalid("group pool cardinality"))
    );
}

#[test]
fn a_corrupt_groups_section_is_rejected_rather_than_absorbed() {
    let bytes = save_sim(&commanded_sim()).unwrap();
    let range = section_range(&bytes, GROUPS_SECTION);
    let start = range.start;

    // `num` of slot 0, at header word 2 of the first group, past the length word.
    let mut bad_num = bytes.clone();
    let num_at = start + 4 + 8;
    bad_num[num_at..num_at + 4].copy_from_slice(&(GROUP_MAX_MEMBERS as i32 + 1).to_le_bytes());
    assert_eq!(load_error(&bad_num), SaveError::Invalid("group slot shape"));

    // A member count that no longer matches the payload the section actually carries.
    let mut short_num = bytes.clone();
    short_num[num_at..num_at + 4].copy_from_slice(&1i32.to_le_bytes());
    assert!(matches!(
        load_error(&short_num),
        SaveError::Invalid("trailing payload bytes") | SaveError::Invalid("group slot shape")
    ));

    // The `Array<Group>` length word is pinned to the retail cardinality.
    let mut bad_len = bytes.clone();
    bad_len[start..start + 4].copy_from_slice(&(NUM_GROUPS as u32 - 1).to_le_bytes());
    assert_eq!(
        load_error(&bad_len),
        SaveError::Invalid("group pool cardinality")
    );

    // `last_group`, the 32 bytes `check_groups` reaches through `Groups+0x3C`.
    let mut bad_last = bytes.clone();
    let last_at = range.end - 32;
    bad_last[last_at..last_at + 4].copy_from_slice(&0x0bad_f00du32.to_le_bytes());
    let loaded = load_sim(&bad_last).unwrap();
    assert_eq!(loaded.groups.last_group[0], 0x0bad_f00du32 as i32);
    assert_ne!(
        groups_channel(&loaded),
        groups_channel(&commanded_sim()),
        "last_group is not reaching the channel, so the section is not carrying it"
    );
}

#[test]
fn a_set_up_match_and_live_group_pool_save_past_frame_zero() {
    // LEADER_MATCH owns the mutable rows over the immutable setup receipt, so advancing
    // the match no longer turns PlayerSetup into an unreconstructible approximation.
    let mut sim = commanded_sim();
    save_sim(&sim).unwrap();
    // Advance the authoritative clocks without running unrelated step-8 AI hosts, whose
    // dynamic save owner is a separate named boundary.
    sim.world.frame = 1;
    sim.vic_match.frame = 1;
    let bytes = save_sim(&sim).expect("set-up mid-match state is now owned");
    let loaded = load_sim(&bytes).expect("mid-match state reloads");
    assert_eq!(loaded.channel_digest(), sim.channel_digest());
    assert_eq!(save_sim(&loaded).unwrap(), bytes);
    assert!(!sim.groups.list[Groups::index(0, 5)].list[..4]
        .iter()
        .any(|&o| o < 0));
}

#[test]
fn a_group_data_header_round_trips_through_its_retail_image() {
    // The section writes `GroupData::header_bytes` verbatim; this pins the decoder to the
    // same field order, so the two cannot drift apart silently.
    let mut sim = Sim::new(0x11, 4);
    let g = GroupData {
        id: -3,
        army: i32::MIN,
        num: 0,
        form: 5,
        stamp: 12_345,
        ox: -1,
        oy: 2,
        o_dist: 3,
        o_angle: i32::MAX,
        disband: -4,
        order_num: 5,
        priority: -6,
        role: 0x7f7f_7f7f,
        think_frame: 8,
        new_speed: -9,
        speed: 10,
        form_num: 0,
        facing: 1,
        buildings: 1,
        who: 7,
        march: 1,
        ..GroupData::default()
    };
    sim.groups.list[100] = g.clone();
    let bytes = save_sim(&sim).unwrap();
    let loaded = load_sim(&bytes).unwrap();
    assert_eq!(loaded.groups.list[100], g);
    assert_eq!(loaded.groups.list[100].header_bytes(), g.header_bytes());
}
