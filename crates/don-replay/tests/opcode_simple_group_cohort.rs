#[path = "../src/opcode_simple_group_cohort.rs"]
mod cohort;

use cohort::{
    classify_action, row, CanonicalSurface, IntegrationState, PackageRelation, ACTION_OPCODES,
    CANONICAL_SURFACES, COHORT, COHORT_INTEGRATION,
};
use don_replay::replay::{corpus, Replay};
use don_sim::command::{ActionDef, InlineDef, InlinePort, Port, Receiver, WireLen, OPCODES};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap()
        .to_path_buf()
}

#[test]
fn pdb_freezes_every_handler_and_action_boundary() {
    let tsv = std::fs::read_to_string(root().join("schema/rise-procs.tsv")).unwrap();
    for evidence in COHORT {
        let handler = format!(
            "{:08x}\t{}\tCommandPackage::{}\t",
            evidence.handler_va, evidence.handler_size, evidence.handler
        );
        assert!(
            tsv.lines().any(|line| line.starts_with(&handler)),
            "missing handler boundary for opcode {}: {handler}",
            evidence.opcode
        );
        if let (Some(action), Some(va), Some(size)) =
            (evidence.action, evidence.action_va, evidence.action_size)
        {
            let action = format!("{:08x}\t{}\t{}\t", va, size, action);
            assert!(
                tsv.lines().any(|line| line.starts_with(&action)),
                "missing action boundary for opcode {}: {action}",
                evidence.opcode
            );
        }
    }
}

#[test]
fn command_table_freezes_the_exact_cohort_identity_receiver_and_wire_width() {
    for evidence in COHORT {
        let row = &OPCODES[usize::from(evidence.opcode)];
        assert_eq!(row.op, evidence.opcode);
        assert_eq!(row.name, evidence.command);
        assert_eq!(row.method, evidence.handler);
        assert_eq!(row.method_va, evidence.handler_va);
        assert_eq!(
            row.receiver,
            if evidence.opcode == 0 {
                Receiver::None
            } else {
                Receiver::Group
            }
        );
        match (row.wire, evidence.wire_len) {
            (WireLen::Variable, None) | (WireLen::Fixed(_), Some(_)) => {}
            pair => panic!("opcode {} wire mismatch: {pair:?}", evidence.opcode),
        }
        if let (WireLen::Fixed(actual), Some(expected)) = (row.wire, evidence.wire_len) {
            assert_eq!(usize::from(actual), expected, "opcode {}", evidence.opcode);
        }
        assert_eq!(
            row.action,
            evidence
                .action
                .map(|symbol| symbol.strip_prefix("Group::action_").unwrap())
        );
    }
}

#[test]
fn source_bound_validation_artifact_matches_the_frozen_counts() {
    let json = std::fs::read_to_string(root().join("schema/replay-validation.json")).unwrap();
    for evidence in COHORT {
        let key = format!("\"0x{:02x}\"", evidence.opcode);
        match json.find(&key) {
            Some(start) => {
                let tail = &json[start..json.len().min(start + 256)];
                let count = format!("\"count\": {}", evidence.validation_count);
                assert!(tail.contains(&count), "{key} missing {count}");
            }
            None => assert_eq!(
                evidence.validation_count, 0,
                "nonzero frozen count but no validation row for {key}"
            ),
        }
    }
}

#[test]
fn executable_and_pdb_identities_are_the_shipped_pair() {
    let output = Command::new("shasum")
        .args([
            "-a",
            "256",
            "ron-bin/riseofnations.exe",
            "ron-bin/sbl/rise.pdb",
        ])
        .current_dir(root())
        .output()
        .unwrap();
    assert!(output.status.success());
    let hashes = String::from_utf8(output.stdout).unwrap();
    assert!(hashes.contains("30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079"));
    assert!(hashes.contains("334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5"));
}

fn u16_at(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap())
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn pe_file_offset(image: &[u8], va: u32) -> usize {
    let pe = u32_at(image, 0x3c) as usize;
    assert_eq!(&image[pe..pe + 4], b"PE\0\0");
    let sections = u16_at(image, pe + 6) as usize;
    let optional_size = u16_at(image, pe + 20) as usize;
    let optional = pe + 24;
    assert_eq!(
        u16_at(image, optional),
        0x10b,
        "expected PE32 optional header"
    );
    let image_base = u32_at(image, optional + 28);
    let rva = va.checked_sub(image_base).unwrap();
    let table = optional + optional_size;
    for index in 0..sections {
        let section = table + index * 40;
        let virtual_size = u32_at(image, section + 8);
        let virtual_address = u32_at(image, section + 12);
        let raw_size = u32_at(image, section + 16);
        let raw = u32_at(image, section + 20);
        let extent = virtual_size.max(raw_size);
        if (virtual_address..virtual_address + extent).contains(&rva) {
            return (raw + rva - virtual_address) as usize;
        }
    }
    panic!("VA {va:#010x} is outside every PE section");
}

#[test]
fn executable_call_instructions_reach_the_frozen_actions() {
    let image = std::fs::read(root().join("ron-bin/riseofnations.exe")).unwrap();
    for evidence in COHORT.iter().filter(|row| row.action.is_some()) {
        let offset = pe_file_offset(&image, evidence.action_call_va);
        if evidence.opcode == 1 {
            assert_eq!(&image[offset..offset + 3], &[0xff, 0x50, 0x14]);
            continue;
        }
        assert_eq!(image[offset], 0xe8, "opcode {}", evidence.opcode);
        let displacement = i32::from_le_bytes(image[offset + 1..offset + 5].try_into().unwrap());
        let target = evidence
            .action_call_va
            .wrapping_add(5)
            .wrapping_add(displacement as u32);
        assert_eq!(
            target,
            evidence.action_va.unwrap(),
            "opcode {}",
            evidence.opcode
        );
    }
}

#[test]
fn closure_table_claim_is_receiver_complete_not_canonical_sim_mounted() {
    for action in [
        "begin",
        "stance",
        "halt",
        "set_transport",
        "disband",
        "stop_spell",
        "follow",
        "unitmask",
        "buildmask",
    ] {
        assert_eq!(
            ActionDef::find(action).unwrap().port,
            Port::Complete,
            "{action}"
        );
    }
    // The source authority deliberately has no `Sim` import or shared module export.  Its
    // presence must not be mistaken for a production mount or a new Complete promotion.
    assert!(
        !std::fs::read_to_string(root().join("crates/don-replay/src/lib.rs"))
            .unwrap()
            .contains("pub mod opcode_simple_group_cohort")
    );
    assert!(CANONICAL_SURFACES.contains(&CanonicalSurface::Groups));
    assert!(CANONICAL_SURFACES.contains(&CanonicalSurface::Orders));
    assert!(CANONICAL_SURFACES.contains(&CanonicalSurface::PlayerMap));
    assert!(CANONICAL_SURFACES.contains(&CanonicalSurface::GameClockRng));
    assert!(CANONICAL_SURFACES.contains(&CanonicalSurface::BuildProductionQueue));
    assert!(CANONICAL_SURFACES.contains(&CanonicalSurface::LeaderState));
    assert!(CANONICAL_SURFACES.contains(&CanonicalSurface::ActionFactAuthority));
    assert_eq!(
        COHORT_INTEGRATION.last(),
        Some(&IntegrationState::CanonicalAdapterMissing)
    );
}

#[test]
fn decimal_opcode_49_is_the_separate_come_out_inline_row() {
    assert!(row(49).is_none());
    let come_out = InlineDef::find(49).unwrap();
    assert_eq!(come_out.name, "come_out");
    assert_eq!(come_out.port, InlinePort::StateWired);
}

/// This is deliberately ignored in the default test profile: it decodes every local retail
/// recording, including user-added files, and can take minutes.  Run it explicitly before
/// changing the cohort package shell or the corpus receipts in its derivation document.
#[test]
#[ignore = "full retail replay corpus"]
fn full_local_corpus_reports_the_exact_package_topology() {
    let paths = corpus(&root());
    assert!(!paths.is_empty(), "retail corpus absent");

    let mut decoded_files = 0usize;
    let mut counts = BTreeMap::<u8, u64>::new();
    let mut relations = BTreeMap::<u8, [u64; 3]>::new();
    let mut packages = 0u64;
    let mut package_records = 0u64;
    let mut packages_decoded = 0u64;
    let mut incomplete = Vec::new();
    let mut load_failures = Vec::new();
    for path in &paths {
        let replay = match Replay::open(path) {
            Ok(replay) => replay,
            Err(error) => {
                load_failures.push((path.clone(), error.to_string()));
                continue;
            }
        };
        decoded_files += 1;
        package_records += replay.packages as u64;
        packages_decoded += replay.packages_decoded as u64;
        if replay.packages != replay.packages_decoded {
            incomplete.push((path.clone(), replay.packages, replay.packages_decoded));
        }
        for turn in &replay.turns {
            for player in &turn.players {
                packages += 1;
                let opcodes = player
                    .commands
                    .iter()
                    .map(|command| command.opcode)
                    .collect::<Vec<_>>();
                for (index, &opcode) in opcodes.iter().enumerate() {
                    if row(opcode).is_some() {
                        *counts.entry(opcode).or_default() += 1;
                    }
                    if !ACTION_OPCODES.contains(&opcode) {
                        continue;
                    }
                    let relation = classify_action(&opcodes, index).unwrap();
                    let buckets = relations.entry(opcode).or_default();
                    buckets[match relation {
                        PackageRelation::AdjacentGroup => 0,
                        PackageRelation::EarlierGroup => 1,
                        PackageRelation::CachedGroup => 2,
                    }] += 1;
                }
            }
        }
    }
    assert_eq!(decoded_files + load_failures.len(), paths.len());
    assert_ne!(decoded_files, 0);
    assert_eq!(packages, packages_decoded);
    eprintln!(
        "decoded_files={decoded_files} package_records={package_records} \
         packages_decoded={packages_decoded} incomplete_files={} load_failures={}",
        incomplete.len(),
        load_failures.len()
    );
    for (path, error) in &load_failures {
        eprintln!("load_failure={} error={error}", path.display());
    }
    for (path, records, decoded) in &incomplete {
        eprintln!(
            "incomplete={} package_records={records} packages_decoded={decoded}",
            path.display()
        );
    }
    for evidence in COHORT {
        eprintln!(
            "opcode={:#04x} count={} relations={:?}",
            evidence.opcode,
            counts.get(&evidence.opcode).copied().unwrap_or(0),
            relations.get(&evidence.opcode).copied().unwrap_or_default()
        );
    }
}
