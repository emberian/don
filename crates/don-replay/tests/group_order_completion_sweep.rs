//! Evidence gate for the complete fourteen-row `orders_partial` cohort.

#[allow(dead_code)]
#[path = "fixtures/retail_group_move.rs"]
mod fixture;
#[path = "../src/group_order_completion_sweep.rs"]
mod sweep;

use don_replay::replay::{corpus, Replay};
use don_replay::wire::{COMMAND_METHOD, COMMAND_SIZEOF, COMMAND_STRUCT};
use don_replay::world_owner_frontier::sha256;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use sweep::{
    action_def, decode_group_order, DecodeError, DecodedGroupOrder, OpenDependency, ACTIONS,
    CAPSTONE_INSTRUCTION_COUNTS, SOURCE_BOUND_COUNTS,
};

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

fn i32_at(bytes: &[u8], at: usize) -> Option<i32> {
    Some(i32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn i16_at(bytes: &[u8], at: usize) -> Option<i16> {
    Some(i16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn put_i32(bytes: &mut Vec<u8>, value: i32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn pe_u16(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap())
}

fn pe_u32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

fn pe_file_offset(image: &[u8], va: u32) -> usize {
    let pe = pe_u32(image, 0x3c) as usize;
    assert_eq!(&image[pe..pe + 4], b"PE\0\0");
    let sections = pe_u16(image, pe + 6) as usize;
    let optional_size = pe_u16(image, pe + 20) as usize;
    let optional = pe + 24;
    assert_eq!(pe_u16(image, optional), 0x10b);
    let rva = va - pe_u32(image, optional + 28);
    let table = optional + optional_size;
    for index in 0..sections {
        let section = table + index * 40;
        let virtual_size = pe_u32(image, section + 8);
        let virtual_address = pe_u32(image, section + 12);
        let raw_size = pe_u32(image, section + 16);
        let raw = pe_u32(image, section + 20);
        if (virtual_address..virtual_address + virtual_size.max(raw_size)).contains(&rva) {
            return (raw + rva - virtual_address) as usize;
        }
    }
    panic!("VA {va:#010x} is outside the PE image")
}

fn synthetic(def: &sweep::ActionDef) -> Vec<u8> {
    let mut bytes = vec![def.opcode];
    while bytes.len() < def.wire_len {
        let value = (def.opcode as i32)
            .wrapping_mul(0x0101_0101)
            .wrapping_add(bytes.len() as i32);
        if def.wire_len - bytes.len() >= 4 {
            put_i32(&mut bytes, value);
        } else {
            bytes.push(value as u8);
        }
    }
    bytes
}

#[test]
fn frozen_rows_are_exactly_the_current_fourteen_orders_partial_actions() {
    assert_eq!(ACTIONS.len(), 14);
    let closure =
        std::fs::read_to_string(repo_root().join("schema/simulation-closure.json")).unwrap();
    assert_eq!(
        closure.matches("\"status\": \"orders_partial\"").count(),
        14
    );
    for row in ACTIONS {
        let name = format!("\"name\": \"{}\"", row.name);
        let at = closure
            .find(&name)
            .unwrap_or_else(|| panic!("missing {}", row.name));
        let tail = &closure[at..closure.len().min(at + 700)];
        assert!(
            tail.contains("\"status\": \"orders_partial\""),
            "{}",
            row.name
        );
        assert!(row.payload_authority);
        assert!(!row.open.is_empty(), "{} was silently promoted", row.name);
        assert_eq!(
            row.canonical_package_route,
            !row.open.contains(&OpenDependency::CanonicalPackageRoute),
            "{} package readiness disagrees with its DAG",
            row.name
        );
        assert_eq!(
            row.executors_complete,
            !row.open.contains(&OpenDependency::ConcreteExecutor),
            "{} executor readiness disagrees with its DAG",
            row.name
        );
        assert_eq!(
            row.save_reload_complete,
            !row.open.contains(&OpenDependency::SaveReload),
            "{} save readiness disagrees with its DAG",
            row.name
        );
    }
    assert_eq!(ACTIONS.iter().map(|r| r.action_size).sum::<u32>(), 29_143);
}

#[test]
fn pdb_spans_and_executable_calls_freeze_the_actual_retail_bodies() {
    let root = repo_root();
    let procs = std::fs::read_to_string(root.join("schema/rise-procs.tsv")).unwrap();
    let image = std::fs::read(root.join("ron-bin/riseofnations.exe")).unwrap();
    let pdb = std::fs::read(root.join("ron-bin/sbl/rise.pdb")).unwrap();
    assert_eq!(
        hex(&sha256(&image)),
        "30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079"
    );
    assert_eq!(
        hex(&sha256(&pdb)),
        "334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5"
    );
    let mut manifest = Vec::new();
    for row in ACTIONS {
        let process = format!("{:08x}\t{}\t", row.process_va, row.process_size);
        let action = format!(
            "{:08x}\t{}\tGroup::action_{}\t",
            row.action_va, row.action_size, row.name
        );
        assert!(
            procs.lines().any(|line| line.starts_with(&process)),
            "{}",
            row.name
        );
        assert!(
            procs.lines().any(|line| line.starts_with(&action)),
            "{}",
            row.name
        );

        let call = pe_file_offset(&image, row.action_call_va);
        assert_eq!(image[call], 0xe8, "{}", row.name);
        let displacement = i32::from_le_bytes(image[call + 1..call + 5].try_into().unwrap());
        assert_eq!(
            row.action_call_va
                .wrapping_add(5)
                .wrapping_add(displacement as u32),
            row.action_va,
            "{}",
            row.name
        );

        let process_at = pe_file_offset(&image, row.process_va);
        let action_at = pe_file_offset(&image, row.action_va);
        let process_bytes = &image[process_at..process_at + row.process_size as usize];
        let action_bytes = &image[action_at..action_at + row.action_size as usize];
        manifest.push(row.opcode);
        manifest.extend_from_slice(&row.process_va.to_le_bytes());
        manifest.extend_from_slice(&row.process_size.to_le_bytes());
        manifest.extend_from_slice(&sha256(process_bytes));
        manifest.extend_from_slice(&row.action_va.to_le_bytes());
        manifest.extend_from_slice(&row.action_size.to_le_bytes());
        manifest.extend_from_slice(&sha256(action_bytes));
    }
    assert_eq!(manifest.len(), 1_134);
    assert_eq!(
        hex(&sha256(&manifest)),
        "0bb0e83a596ba0b54ddd59f828e1184dd47ff215633f77717840a11ef6356af1"
    );
    assert_eq!(
        CAPSTONE_INSTRUCTION_COUNTS
            .iter()
            .map(|(process, _)| process)
            .sum::<usize>(),
        1_503
    );
    assert_eq!(
        CAPSTONE_INSTRUCTION_COUNTS
            .iter()
            .map(|(_, action)| action)
            .sum::<usize>(),
        8_105
    );
}

#[test]
fn wire_rows_agree_with_both_generated_tables_and_strict_decoder() {
    for row in ACTIONS {
        let op = row.opcode as usize;
        assert_eq!(
            don_net::COMMAND_SIZES[op],
            Some(row.wire_len as u16),
            "{}",
            row.name
        );
        assert_eq!(COMMAND_SIZEOF[op], row.wire_len as u16, "{}", row.name);
        assert_eq!(COMMAND_STRUCT[op], row.command, "{}", row.name);
        assert_eq!(
            COMMAND_METHOD[op],
            format!("process_{}", row.name),
            "{}",
            row.name
        );

        let bytes = synthetic(&row);
        assert!(decode_group_order(&bytes).is_ok(), "{}", row.name);
        let truncated_error = if row.wire_len == 1 {
            DecodeError::UnsupportedOpcode(u8::MAX)
        } else {
            DecodeError::WrongLength {
                opcode: row.opcode,
                expected: row.wire_len,
                actual: row.wire_len - 1,
            }
        };
        assert_eq!(
            decode_group_order(&bytes[..bytes.len() - 1]),
            Err(truncated_error)
        );
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert_eq!(
            decode_group_order(&trailing),
            Err(DecodeError::WrongLength {
                opcode: row.opcode,
                expected: row.wire_len,
                actual: row.wire_len + 1,
            })
        );
    }
    assert_eq!(
        decode_group_order(&[12]),
        Err(DecodeError::UnsupportedOpcode(12))
    );
    assert_eq!(
        decode_group_order(&[]),
        Err(DecodeError::UnsupportedOpcode(u8::MAX))
    );
}

#[test]
fn packed_fields_are_not_normalized_or_defaulted() {
    let mut move_to = vec![7];
    put_i32(&mut move_to, -123);
    put_i32(&mut move_to, 456);
    put_i32(&mut move_to, -7);
    put_i32(&mut move_to, i32::MIN);
    move_to.extend_from_slice(&[0xfe, 0x81, 0x80, 0x7f, 0xff]);
    assert_eq!(
        decode_group_order(&move_to).unwrap(),
        DecodedGroupOrder::MoveTo {
            to_x: -123,
            to_y: 456,
            set_angle: -7,
            angle: i32::MIN,
            orders: -2,
            queued: -127,
            form: -128,
            width: 127,
            disembark: -1,
        }
    );

    let mut trade = vec![17];
    for field in [-1, 2, i32::MIN, i32::MAX, 0x1020_3040] {
        put_i32(&mut trade, field);
    }
    assert_eq!(
        decode_group_order(&trade).unwrap(),
        DecodedGroupOrder::Trade {
            ox: -1,
            whom: 2,
            oxx: i32::MIN,
            whose: i32::MAX,
            queued: 0x1020_3040,
        }
    );
}

#[test]
fn source_bound_replay_counts_cover_eleven_rows_and_freeze_three_gaps() {
    let validation =
        std::fs::read_to_string(repo_root().join("schema/replay-validation.json")).unwrap();
    assert_eq!(SOURCE_BOUND_COUNTS.iter().sum::<usize>(), 16_745);
    for (row, expected) in ACTIONS.into_iter().zip(SOURCE_BOUND_COUNTS) {
        let key = format!("\"0x{:02x}\":", row.opcode);
        let actual = validation.find(&key).map_or(0, |at| {
            let tail = &validation[at..validation.len().min(at + 180)];
            let marker = "\"count\":";
            let count_at = tail.find(marker).unwrap() + marker.len();
            tail[count_at..]
                .trim_start()
                .split(|ch: char| !ch.is_ascii_digit())
                .next()
                .unwrap()
                .parse::<usize>()
                .unwrap()
        });
        assert_eq!(actual, expected, "{}", row.name);
    }
    assert_eq!(
        ACTIONS
            .into_iter()
            .zip(SOURCE_BOUND_COUNTS)
            .filter_map(|(row, count)| (count == 0).then_some(row.opcode))
            .collect::<Vec<_>>(),
        vec![8, 15, 16]
    );
}

#[test]
#[ignore = "full retail replay corpus"]
fn every_shipped_corpus_record_in_the_cohort_decodes_exactly() {
    let root = repo_root();
    let paths = corpus(&root);
    assert!(
        paths.len() >= 61,
        "retail replay corpus is incomplete: {}",
        paths.len()
    );
    let mut counts = BTreeMap::<u8, usize>::new();
    let mut manifest = Vec::new();
    let mut decoded_replays = 0usize;
    for path in paths {
        let Ok(replay) = Replay::open(&path) else {
            continue;
        };
        decoded_replays += 1;
        for turn in replay.turns {
            for player in turn.players {
                for command in player.commands {
                    if action_def(command.opcode).is_none() {
                        continue;
                    }
                    decode_group_order(&command.bytes).unwrap_or_else(|error| {
                        panic!("{} opcode {}: {error:?}", path.display(), command.opcode)
                    });
                    *counts.entry(command.opcode).or_default() += 1;
                    manifest.push(command.opcode);
                    manifest.extend_from_slice(&(command.bytes.len() as u16).to_le_bytes());
                    manifest.extend_from_slice(&command.bytes);
                }
            }
        }
    }
    assert!(
        decoded_replays >= 60,
        "only {decoded_replays} replay decoders admitted"
    );
    for (row, source_bound) in ACTIONS.into_iter().zip(SOURCE_BOUND_COUNTS) {
        assert!(
            counts.get(&row.opcode).copied().unwrap_or(0) >= source_bound,
            "{} dropped below the frozen source-bound corpus count",
            row.name
        );
    }
    assert!(counts.values().sum::<usize>() >= 16_745);
    eprintln!("group-order cohort counts={counts:?}");
    eprintln!(
        "group-order manifest bytes={} sha256={}",
        manifest.len(),
        hex(&sha256(&manifest))
    );
}

#[derive(Debug)]
struct ParsedGroups {
    live: usize,
    end: usize,
    last_group: [i32; 8],
    proc_group: i32,
}

fn parse_groups_at(bytes: &[u8], off: usize) -> Option<ParsedGroups> {
    if i32_at(bytes, off)? != 512
        || i32_at(bytes, off + 4)? != 512
        || i16_at(bytes, off + 8)? != -1
        || *bytes.get(off + 10)? != 0
    {
        return None;
    }
    let mut p = off + 11;
    let mut live = 0usize;
    for slot in 0..512i32 {
        let header = bytes.get(p..p + 72)?;
        let id = i32_at(header, 0)?;
        let num = i32_at(header, 8)?;
        let form_num = i32_at(header, 64)?;
        let who = header[70];
        if id != slot || !(0..=128).contains(&num) || !(0..=128).contains(&form_num) || who >= 8 {
            return None;
        }
        p += 72;
        let n = num as usize;
        bytes.get(p..p + 19 * n)?;
        p += 19 * n;
        live += usize::from(n != 0);
    }
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
    Some(ParsedGroups {
        live,
        end: p,
        last_group,
        proc_group,
    })
}

#[test]
fn fresh_svx_proves_groups_but_not_yet_orderlist_offsets() {
    let path = repo_root().join(fixture::SAVE_RELATIVE_PATH);
    assert!(path.exists(), "fresh retail SVX is absent");
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
    let groups = parse_groups_at(&plain, fixture::GROUPS_OFFSET).unwrap();
    assert_eq!(groups.end, fixture::GROUPS_END);
    assert_eq!(groups.live, fixture::LIVE_GROUPS.len());
    assert_eq!(groups.last_group, fixture::LAST_GROUP);
    assert_eq!(groups.proc_group, fixture::PROC_GROUP);

    // This is deliberately not an OrderList assertion. The current retail parser has not
    // localized the per-Unit `OrderList::walk_data` substreams in this SVX, so the artifact
    // supplies canonical Groups evidence and zero save-closure credit for order payloads.
}
