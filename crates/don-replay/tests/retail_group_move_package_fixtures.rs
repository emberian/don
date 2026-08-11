//! Retail-backed fixtures for the future canonical `Sim::process_command_package` host.
//!
//! This test deliberately never instantiates `command::Bridge`: the `.rcx` supplies the
//! wire/chronology evidence and the `.SVX` supplies the canonical `groups_guys::Groups`
//! image.  If the local retail artifacts are absent, the artifact-backed tests say so and
//! establish nothing; the five compact extracted packages still guard exact wire decoding.

#[path = "fixtures/retail_group_move.rs"]
mod fixture;

use don_net::{decode_commands, Obfuscation};
use don_replay::replay::Replay;
use don_replay::wire::CommandView;
use don_replay::world_owner_frontier::sha256;
use fixture::{LiveGroup, PackageFixture};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        write!(out, "{b:02x}").unwrap();
    }
    out
}

fn decode_hex(s: &str) -> Vec<u8> {
    assert_eq!(s.len() % 2, 0);
    s.as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn i16_at(bytes: &[u8], at: usize) -> Option<i16> {
    Some(i16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn i32_at(bytes: &[u8], at: usize) -> Option<i32> {
    Some(i32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn fields(command: &[u8]) -> [i64; 9] {
    let view = CommandView::new(7, command);
    std::array::from_fn(|i| {
        view.get(
            [
                "to_x",
                "to_y",
                "set_angle",
                "angle",
                "orders",
                "queued",
                "form",
                "width",
                "disembark",
            ][i],
        )
        .unwrap()
    })
}

fn decoded_fixture(f: PackageFixture) -> (Vec<u8>, Vec<Vec<u8>>) {
    let payload = decode_hex(f.payload_hex);
    assert_eq!(hex(&sha256(&payload)), f.payload_sha256);
    let mut obfuscation = Obfuscation::none();
    let commands = decode_commands(&payload, &mut obfuscation).unwrap();
    let owned = commands
        .iter()
        .map(|c| c.bytes.to_vec())
        .collect::<Vec<_>>();
    assert_eq!(
        commands.iter().map(|c| c.opcode).collect::<Vec<_>>(),
        f.opcodes
    );
    (payload, owned)
}

#[test]
fn compact_packages_cover_prefix_cache_special_tail_and_multiselect_shapes() {
    let decoded = fixture::PACKAGE_FIXTURES.map(|f| (f, decoded_fixture(f).1));

    let (_, prefix) = &decoded[0];
    assert_eq!(
        prefix.iter().map(|c| c[0]).collect::<Vec<_>>(),
        [72, 79, 0, 7]
    );
    assert_eq!(&prefix[2], &decode_hex("0001000200"));
    assert_eq!(fields(&prefix[3]), [9_741, 47_438, 0, 0, 1, 2, 0, 50, 0]);

    let (_, cached) = &decoded[1];
    assert_eq!(&cached[0], &[0, 0, 0]);
    assert_eq!(fields(&cached[1]), [15_152, 49_625, 0, 0, 1, 2, 0, 50, 0]);

    let (_, special) = &decoded[2];
    assert_eq!(&special[0], &[0, 0, 0]);
    assert_eq!(fields(&special[1]), [8_930, 29_901, 0, 0, 2, 1, -1, -1, 0]);

    let (_, facing) = &decoded[3];
    assert_eq!(facing[0][1], 26);
    assert_eq!(facing[0][2] as i8, 0);
    assert_eq!(facing[0].len(), 3 + 2 * 26);
    assert_eq!(
        fields(&facing[1]),
        [8_459, 19_871, 1, 0x00ee_0000, 1, 2, 0, 50, 0]
    );

    let (_, singleton) = &decoded[4];
    assert_eq!(&singleton[0], &decode_hex("0001000100"));
    assert_eq!(
        fields(&singleton[1]),
        [47_435, 47_486, 0, 0, 1, 2, 0, 50, 0]
    );
}

#[test]
fn finished_replay_binds_all_fifty_three_group_move_packages() {
    let path = repo_root().join(fixture::REPLAY_RELATIVE_PATH);
    if !path.exists() {
        eprintln!(
            "\n  SKIPPED — NOT A PASS. {} is absent; the retail cohort was not exercised.\n",
            path.display()
        );
        return;
    }

    let compressed = std::fs::read(&path).unwrap();
    assert_eq!(hex(&sha256(&compressed)), fixture::REPLAY_FILE_SHA256);
    let replay = Replay::open(&path).unwrap();
    assert_eq!(
        hex(&replay.initial.payload_sha256),
        fixture::REPLAY_PLAIN_SHA256
    );
    assert_eq!(replay.initial.info.seed, fixture::REPLAY_SEED);
    assert_eq!(replay.stream_start, fixture::REPLAY_STREAM_OFFSET);
    assert_eq!(replay.packages, fixture::REPLAY_PACKAGES);
    assert_eq!(replay.packages_decoded, fixture::REPLAY_PACKAGES);
    assert_eq!(replay.xor_key, 0);
    assert_eq!(replay.pad_seed, None);
    assert_eq!(replay.turns.len(), fixture::REPLAY_PACKAGES);
    assert_eq!(
        replay
            .initial
            .active_players()
            .map(|p| (p.slot, p.who, p.play))
            .collect::<Vec<_>>(),
        [(0, 0, 0), (1, 1, 1), (2, 2, 2), (3, 3, 3)]
    );
    for expected in fixture::PACKAGE_FIXTURES {
        let turn = &replay.turns[expected.package_index0 as usize];
        assert_eq!(turn.turn, expected.serial);
        assert_eq!(turn.players[0].stamp, expected.frame);
        let payload = turn.players[0]
            .commands
            .iter()
            .flat_map(|command| command.bytes.iter().copied())
            .collect::<Vec<_>>();
        assert_eq!(hex(&payload), expected.payload_hex);
    }

    let mut commands_total = 0usize;
    let mut group_total = 0usize;
    let mut move_total = 0usize;
    let mut pair_total = 0usize;
    let mut pair_manifest = Vec::new();
    let mut package_manifest = Vec::new();
    let mut shapes: BTreeMap<Vec<u8>, usize> = BTreeMap::new();
    let mut group_sizes: BTreeMap<u8, usize> = BTreeMap::new();
    let mut move_tails: BTreeMap<[i64; 7], usize> = BTreeMap::new();
    let mut explicit_occurrences = 0usize;
    let mut unique_members = BTreeSet::new();
    let mut empty_history = Vec::new();
    let mut last_explicit: [Option<(u32, Vec<i16>)>; 8] = Default::default();
    let mut min_x = i64::MAX;
    let mut max_x = i64::MIN;
    let mut min_y = i64::MAX;
    let mut max_y = i64::MIN;

    for (package_index0, turn) in replay.turns.iter().enumerate() {
        assert_eq!(turn.turn, package_index0 as i32 + 1);
        assert_eq!(turn.players.len(), 1);
        let player = &turn.players[0];
        commands_total += player.commands.len();
        group_total += player.commands.iter().filter(|c| c.opcode == 0).count();
        let moves = player
            .commands
            .iter()
            .enumerate()
            .filter(|(_, c)| c.opcode == 7)
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        move_total += moves.len();
        if moves.is_empty() {
            // Empty re-selection caches persist across every action kind, not merely moves.
            for command in &player.commands {
                if command.opcode == 0 && command.bytes[1] != 0 {
                    let who = command.bytes[2] as i8;
                    assert!((0..8).contains(&who));
                    let members = (0..command.bytes[1] as usize)
                        .map(|i| i16_at(&command.bytes, 3 + 2 * i).unwrap())
                        .collect();
                    last_explicit[who as usize] = Some((package_index0 as u32, members));
                }
            }
            continue;
        }

        assert_eq!(moves.len(), 1);
        let move_at = moves[0];
        assert_eq!(
            move_at + 1,
            player.commands.len(),
            "MoveTo is package-terminal"
        );
        assert!(move_at > 0);
        let group_at = move_at - 1;
        assert_eq!(player.commands[group_at].opcode, 0);
        pair_total += 1;

        // Commands before Group are real package members and must not reset its later scratch.
        let shape = player.commands.iter().map(|c| c.opcode).collect::<Vec<_>>();
        *shapes.entry(shape).or_default() += 1;
        assert_eq!(player.play, 0);

        let group = &player.commands[group_at].bytes;
        let mv = &player.commands[move_at].bytes;
        let num = group[1];
        let who = group[2] as i8;
        assert_eq!(who, 0);
        assert_eq!(group.len(), 3 + 2 * num as usize);
        *group_sizes.entry(num).or_default() += 1;
        let members = (0..num as usize)
            .map(|i| i16_at(group, 3 + i * 2).unwrap())
            .collect::<Vec<_>>();
        assert!(members.iter().all(|&o| o >= 0));
        assert_eq!(
            members.iter().copied().collect::<HashSet<_>>().len(),
            members.len()
        );
        explicit_occurrences += members.len();
        unique_members.extend(members.iter().copied());

        if num == 0 {
            let (source, cached) = last_explicit[who as usize].as_ref().unwrap();
            empty_history.push((package_index0 as u32, *source, cached.len()));
        } else {
            last_explicit[who as usize] = Some((package_index0 as u32, members));
        }

        let values = fields(mv);
        min_x = min_x.min(values[0]);
        max_x = max_x.max(values[0]);
        min_y = min_y.min(values[1]);
        max_y = max_y.max(values[1]);
        *move_tails
            .entry(values[2..].try_into().unwrap())
            .or_default() += 1;

        pair_manifest.extend_from_slice(&(package_index0 as u32).to_le_bytes());
        pair_manifest.extend_from_slice(&player.stamp.to_le_bytes());
        pair_manifest.extend_from_slice(&turn.turn.to_le_bytes());
        pair_manifest.extend_from_slice(&(group.len() as u16).to_le_bytes());
        pair_manifest.extend_from_slice(group);
        pair_manifest.extend_from_slice(&(mv.len() as u16).to_le_bytes());
        pair_manifest.extend_from_slice(mv);

        let payload = player
            .commands
            .iter()
            .flat_map(|command| command.bytes.iter().copied())
            .collect::<Vec<_>>();
        package_manifest.extend_from_slice(&(payload.len() as u16).to_le_bytes());
        package_manifest.extend_from_slice(&payload);
    }

    assert_eq!(commands_total, fixture::REPLAY_COMMANDS);
    assert_eq!(group_total, fixture::GROUP_COMMANDS);
    assert_eq!(move_total, fixture::MOVE_TO_COMMANDS);
    assert_eq!(pair_total, fixture::MOVE_TO_COMMANDS);
    assert_eq!(
        shapes,
        BTreeMap::from([
            (vec![0, 7], 41),
            (vec![72, 0, 7], 11),
            (vec![72, 79, 0, 7], 1)
        ])
    );
    assert_eq!(
        group_sizes,
        BTreeMap::from([
            (0, 10),
            (1, 32),
            (2, 1),
            (4, 2),
            (5, 1),
            (6, 1),
            (26, 1),
            (29, 1),
            (37, 1),
            (38, 1),
            (52, 1),
            (55, 1),
        ])
    );
    assert_eq!(explicit_occurrences, 290);
    assert_eq!(unique_members.len(), 129);
    assert_eq!(unique_members.first(), Some(&0));
    assert_eq!(unique_members.last(), Some(&324));
    assert_eq!((min_x, max_x, min_y, max_y), (1_175, 47_435, 3_181, 51_244));
    assert_eq!(
        move_tails,
        BTreeMap::from([
            ([0, 0, 1, 2, 0, 50, 0], 47),
            ([0, 0, 2, 1, -1, -1, 0], 1),
            ([1, 0x00ee_0000, 1, 2, 0, 50, 0], 1),
            ([1, 0x0351_0000, 1, 2, 0, 50, 0], 1),
            ([1, 0x067c_0000, 1, 2, 0, 50, 0], 1),
            ([1, 0x08b1_0000, 1, 2, 0, 50, 0], 1),
            ([1, 0x761f_0000, 1, 2, 0, 50, 0], 1),
        ])
    );
    assert_eq!(
        empty_history,
        [
            (6_145, 6_123, 1),
            (14_153, 14_138, 1),
            (38_933, 38_912, 4),
            (40_451, 40_439, 4),
            (41_925, 41_918, 2),
            (42_169, 42_147, 1),
            (42_188, 42_147, 1),
            (47_024, 47_008, 67),
            (48_621, 48_604, 5),
            (50_138, 50_044, 13),
        ]
    );
    assert_eq!(pair_manifest.len(), fixture::PAIR_MANIFEST_BYTES);
    assert_eq!(hex(&sha256(&pair_manifest)), fixture::PAIR_MANIFEST_SHA256);
    assert_eq!(package_manifest.len(), fixture::PACKAGE_MANIFEST_BYTES);
    assert_eq!(
        hex(&sha256(&package_manifest)),
        fixture::PACKAGE_MANIFEST_SHA256
    );
}

#[derive(Debug)]
struct ParsedGroups {
    records_end: usize,
    end: usize,
    live: Vec<LiveGroup>,
    live_vectors_zero: bool,
    last_group: [i32; 8],
    proc_group: i32,
}

fn parse_groups_at(bytes: &[u8], off: usize) -> Option<ParsedGroups> {
    if u32_at(bytes, off)? != 512
        || u32_at(bytes, off + 4)? != 512
        || i16_at(bytes, off + 8)? != -1
        || *bytes.get(off + 10)? != 0
    {
        return None;
    }
    let mut p = off + 11;
    let mut live = Vec::new();
    let mut live_vectors_zero = true;
    for slot in 0..512i32 {
        let header = bytes.get(p..p + 72)?;
        let word = |i: usize| i32_at(header, i * 4);
        let id = word(0)?;
        let num = word(2)?;
        let form = word(3)?;
        let stamp = word(4)?;
        let order_num = word(10)?;
        let role = word(12)?;
        let form_num = word(16)?;
        let buildings = header[69];
        let who = header[70];
        if id != slot
            || !(0..=128).contains(&num)
            || !(0..=128).contains(&form_num)
            || who >= 8
            || buildings > 1
        {
            return None;
        }
        p += 72;
        let n = num as usize;
        let members = bytes.get(p..p + 2 * n)?;
        p += 2 * n;
        let vectors = bytes.get(p..p + 16 * n)?;
        p += 16 * n;
        let angles = bytes.get(p..p + n)?;
        p += n;
        if n != 0 {
            if who as i32 != slot / 64 || n != 1 {
                return None;
            }
            live_vectors_zero &= vectors.iter().all(|&b| b == 0) && angles[0] == 0;
            live.push(LiveGroup {
                slot,
                member: i16::from_le_bytes(members.try_into().ok()?),
                form,
                stamp,
                order_num,
                role,
                form_num,
                buildings,
                who,
            });
        }
    }
    let records_end = p;
    if *bytes.get(p)? != 0xef {
        return None;
    }
    p += 1;
    let mut last_group = [0; 8];
    for value in &mut last_group {
        *value = i32_at(bytes, p)?;
        p += 4;
    }
    let proc_group = i32_at(bytes, p)?;
    p += 4;
    if !(0..64).contains(&proc_group) {
        return None;
    }
    Some(ParsedGroups {
        records_end,
        end: p,
        live,
        live_vectors_zero,
        last_group,
        proc_group,
    })
}

fn read_wstr(bytes: &[u8], p: &mut usize) -> Option<()> {
    let n = u32_at(bytes, *p)? as usize;
    *p = p.checked_add(4 + 2 * n)?;
    bytes.get(..*p)?;
    Some(())
}

fn gameinfo_identity(bytes: &[u8]) -> Option<(u32, Vec<(u8, u8, u8)>)> {
    let mut p = 0x16;
    if *bytes.get(p)? != 0x42 {
        return None;
    }
    p += 1;
    read_wstr(bytes, &mut p)?;
    p += 4; // version
    let seed = u32_at(bytes, p)?;
    p += 4 + 12 + 4 + 30; // seed, checksum controls, flags, settings
    let mut mapping = Vec::new();
    for slot in 0..8u8 {
        if *bytes.get(p)? != 0x50 {
            return None;
        }
        p += 1;
        let flags = u16_at(bytes, p)?;
        p += 2;
        if flags & 1 == 0 {
            continue;
        }
        let body = bytes.get(p..p + 57)?;
        p += 57;
        mapping.push((slot, body[0x33], body[0x36]));
        read_wstr(bytes, &mut p)?;
    }
    Some((seed, mapping))
}

#[test]
fn fresh_v16_save_binds_commands_to_the_canonical_player_major_group_pool() {
    let path = repo_root().join(fixture::SAVE_RELATIVE_PATH);
    if !path.exists() {
        eprintln!(
            "\n  SKIPPED — NOT A PASS. {} is absent; the retail Groups image was not exercised.\n",
            path.display()
        );
        return;
    }

    let compressed = std::fs::read(&path).unwrap();
    assert_eq!(hex(&sha256(&compressed)), fixture::SAVE_FILE_SHA256);
    let output = std::process::Command::new("gzip")
        .arg("-dc")
        .arg(&path)
        .output()
        .unwrap();
    assert!(output.status.success());
    let plain = output.stdout;
    assert_eq!(plain.len(), fixture::SAVE_PLAIN_BYTES);
    assert_eq!(hex(&sha256(&plain)), fixture::SAVE_PLAIN_SHA256);

    let candidates = plain
        .windows(4)
        .enumerate()
        .filter_map(|(off, raw)| {
            (u32::from_le_bytes(raw.try_into().unwrap()) == 512
                && parse_groups_at(&plain, off).is_some())
            .then_some(off)
        })
        .collect::<Vec<_>>();
    assert_eq!(candidates, [fixture::GROUPS_OFFSET]);
    let parsed = parse_groups_at(&plain, fixture::GROUPS_OFFSET).unwrap();
    assert_eq!(fixture::GROUPS_ELEMENTS_OFFSET, fixture::GROUPS_OFFSET + 11);
    assert_eq!(parsed.records_end, fixture::GROUPS_RECORDS_END);
    assert_eq!(parsed.end, fixture::GROUPS_END);
    assert_eq!(fixture::GROUPS_LAST_GROUP_OFFSET, parsed.records_end + 1);
    assert_eq!(
        fixture::GROUPS_PROC_GROUP_OFFSET,
        parsed.records_end + 1 + 32
    );
    assert!(parsed.live_vectors_zero);
    assert_eq!(parsed.live, fixture::LIVE_GROUPS);
    assert_eq!(parsed.last_group, fixture::LAST_GROUP);
    assert_eq!(parsed.proc_group, fixture::PROC_GROUP);
    assert_eq!(
        hex(&sha256(
            &plain[fixture::GROUPS_ELEMENTS_OFFSET..fixture::GROUPS_RECORDS_END]
        )),
        fixture::GROUPS_RECORDS_SHA256
    );
    assert_eq!(
        hex(&sha256(&plain[fixture::GROUPS_OFFSET..fixture::GROUPS_END])),
        fixture::GROUPS_SECTION_SHA256
    );

    let frame = u32_at(&plain, fixture::SAVE_FRAME_OFFSET).unwrap();
    assert_eq!(frame, fixture::SAVE_FRAME);
    assert_eq!(frame % 64, parsed.proc_group as u32);
    let (seed, mapping) = gameinfo_identity(&plain).unwrap();
    assert_eq!(seed, fixture::SAVE_SEED);
    assert_ne!(
        seed,
        fixture::REPLAY_SEED,
        "the RCX and SVX are different matches"
    );
    assert_eq!(mapping, [(0, 0, 0), (1, 1, 1), (2, 2, 2), (3, 3, 3)]);

    // These are owner-local object indices inside canonical player-major groups, not
    // package serials or global object handles. `last_group` remains the one allocation
    // owner that command processing and save/resume must mutate together.
    assert!(parsed.live.iter().all(|g| g.slot / 64 == i32::from(g.who)));
    assert!(parsed.live.iter().any(|g| g.member >= 2_000));
    for (who, &last) in parsed.last_group.iter().enumerate() {
        assert!((who as i32 * 64..who as i32 * 64 + 64).contains(&last));
    }
}
