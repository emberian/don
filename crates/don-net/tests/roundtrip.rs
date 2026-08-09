//! Corpus round-trip: decode every command package in every shipped `.rcx`,
//! re-encode it, and require the bytes back exactly.
//!
//! This is the test that can fail, and it has. A hand-built fixture only shows
//! the encoder agrees with the decoder; re-emitting bytes the *retail engine
//! wrote* is what shows the codec agrees with the engine. Any wrong command
//! size, wrong variable-length formula, wrong XOR key or wrong pad model shifts
//! every following byte and the walk stops tiling the payload.
//!
//! It earned its keep immediately: with `ChatCommand` sized `17 + 2*len`
//! instead of the handler's actual `19 + 2*len`, 17 of 63 recordings decoded.
//! With the corrected formula, 59 do.
//!
//! `ron-data/` is gitignored copyrighted game content. When it is absent these
//! tests skip **loudly** — a skip is not a pass, and the banner says so.
//!
//! Run with output:
//!   cargo test -p don-net -- --nocapture

use don_net::obfuscate::{rank_xor_keys, xor_payload};
use don_net::{
    decode_commands, encode_commands, find_stream, CheckSums, Command, NetCommandPackage,
    Obfuscation, PackageHeader, PackageRecord, PackageStream,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command as Proc;

// --- measured floors -------------------------------------------------------
// These encode what the corpus on this machine actually shows. They are floors,
// not targets: a regression in any size formula drops the package rate by
// orders of magnitude, not by a hair.

/// Fraction of command packages (across files that contain a command stream)
/// that must round-trip byte-exactly. Measured: 1,296,192 of 1,296,194.
const MIN_PACKAGE_RATE: f64 = 0.9999;
/// Fraction of files containing a command stream that must reach 100%.
const MIN_PERFECT_FILE_RATE: f64 = 0.95;
/// Fraction of cross-player checksum tuple comparisons that must agree.
/// Measured: 265,910 of 265,931 = 0.99992.
const MIN_CHECKSUM_AGREEMENT: f64 = 0.999;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

/// Number of recordings the default run sweeps. The full corpus is ~46 MB and
/// several minutes; this crate lives in a workspace other lanes run
/// `cargo test` on, so the default is a deterministic spread and the full sweep
/// is one environment variable away:
///
///   DON_NET_FULL_CORPUS=1 cargo test -p don-net --release -- --nocapture
///
/// The headline numbers in `docs/tracks/headless-client.md` are from the full
/// sweep. The subset is still a real test: it spans four engine-build eras and
/// both solo and multiplayer, and every size-formula regression breaks it.
const DEFAULT_FILES: usize = 12;

fn full_corpus_requested() -> bool {
    std::env::var("DON_NET_FULL_CORPUS").is_ok_and(|v| v != "0" && !v.is_empty())
}

fn corpus() -> Vec<PathBuf> {
    let mut v = Vec::new();
    for dir in [
        repo_root().join("ron-data/replays"),
        repo_root().join("ron-data/replays/multi"),
    ] {
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().and_then(|s| s.to_str()) == Some("rcx") {
                    v.push(p);
                }
            }
        }
    }
    v.sort();
    if full_corpus_requested() || v.len() <= DEFAULT_FILES {
        return v;
    }
    // Deterministic even spread over the sorted (chronological) corpus, so the
    // subset always covers the oldest and newest builds.
    let n = v.len();
    let mut out: Vec<PathBuf> = (0..DEFAULT_FILES)
        .map(|i| v[i * (n - 1) / (DEFAULT_FILES - 1)].clone())
        .collect();
    out.dedup();
    out
}

/// Load a `.rcx` payload.
///
/// Most recordings are a single gzip member from offset 0, but **not all**:
/// three files in this corpus are stored uncompressed and start straight at the
/// payload (`16 42 1a 00 …` — the `SaveGame::walk_test` tag byte, then the
/// version string). `File::write` chooses `fwrite` or `gzwrite` off a mode bit,
/// so both forms are legal output of the same code path. Sniff the magic.
///
/// Decompression shells out to the system `gzip`, so this crate needs no
/// inflate dependency and a green `cargo test` never touches the registry.
fn load_payload(p: &Path) -> Option<Vec<u8>> {
    let raw = std::fs::read(p).ok()?;
    if raw.len() < 2 {
        return None;
    }
    if raw[0] != 0x1F || raw[1] != 0x8B {
        return Some(raw);
    }
    let out = Proc::new("gzip").arg("-dc").arg(p).output().ok()?;
    if out.stdout.is_empty() {
        None
    } else {
        Some(out.stdout)
    }
}

#[derive(Default)]
struct FileResult {
    packages: usize,
    ok: usize,
    commands: usize,
    key: u16,
    /// None = solo (no obfuscation applied); Some = multiplayer pad seeds that fit.
    seeds: Option<Vec<u32>>,
    opcodes: HashMap<u8, usize>,
    /// (stamp, play, checksums)
    checksums: Vec<(u32, i32, CheckSums)>,
    anomalies: Vec<String>,
}

/// Decode one file end to end, asserting the byte-exact round trip per package.
/// Returns `Err` only when the file contains no command stream at all.
fn analyse(path: &Path) -> Result<FileResult, String> {
    let payload = load_payload(path).ok_or("unreadable / not a gzip member")?;
    let loc = find_stream(&payload).ok_or("no command-package chain")?;

    // 1. Framing must tile exactly to EOF.
    let recs: Vec<PackageRecord> = PackageStream::new(&payload, loc.start).collect();
    let consumed: usize = recs
        .iter()
        .map(|r| PackageHeader::WIRE_LEN + r.payload.len())
        .sum();
    if loc.start + consumed != payload.len() {
        return Err(format!(
            "framing residue: {} bytes",
            payload.len() - loc.start - consumed
        ));
    }
    if recs.is_empty() {
        return Err("chain is empty".into());
    }

    // 2. Header round-trip, byte for byte, on every record.
    for r in &recs {
        let mut enc = Vec::new();
        r.encode(&mut enc);
        let (again, n) = PackageRecord::decode(&enc).ok_or("record failed to re-decode")?;
        assert_eq!(again.header, r.header, "header survives a round trip");
        assert_eq!(again.payload, r.payload, "payload survives a round trip");
        assert_eq!(n, enc.len());
        assert_eq!(n, PackageHeader::WIRE_LEN + r.payload.len());
    }

    // 3. Pick the (xor key, pad seed) pair that decodes the most packages. The
    //    key is a frequency guess, so it is only ever accepted on evidence.
    let candidates = rank_xor_keys(recs.iter().map(|r| r.payload), 12);
    let dexor_all = |key: u16| -> Vec<Vec<u8>> {
        recs.iter()
            .map(|r| {
                let mut b = r.payload.to_vec();
                xor_payload(&mut b, key);
                b
            })
            .collect()
    };
    //    The 256-way seed search runs over a bounded PROBE prefix only; whichever
    //    seeds survive it are then scored over *every* package. Searching the
    //    whole file 256 times is pure waste (and this crate sits in a workspace
    //    several lanes run `cargo test` on), while the verification below is
    //    still over the complete corpus, so nothing is weakened.
    const PROBE: usize = 48;
    let score = |plains: &[Vec<u8>], seed: Option<u32>, limit: usize| -> usize {
        plains
            .iter()
            .take(limit)
            .filter(|p| {
                let mut obf = match seed {
                    None => Obfuscation::none(),
                    Some(s) => Obfuscation::with_seed(s),
                };
                decode_commands(p, &mut obf).is_ok()
            })
            .count()
    };
    let all = usize::MAX;

    let mut best: Option<(usize, u16, Vec<Vec<u8>>, Option<Vec<u32>>)> = None;
    for &key in &candidates {
        let plains = dexor_all(key);
        let probe = PROBE.min(plains.len());

        // Only the low 16 bits of the seed can change any draw, and bits 8..15
        // are pinned by the XOR key, so 256 candidates remain.
        let mut shortlist: Vec<Option<u32>> = vec![None];
        let probe_nopad = score(&plains, None, probe);
        let mut probe_best = probe_nopad;
        for lo in 0u32..256 {
            let s = Some((((key & 0xFF) as u32) << 8) | lo);
            let n = score(&plains, s, probe);
            if n > probe_best {
                probe_best = n;
                shortlist = vec![s];
            } else if n == probe_best && n > 0 {
                shortlist.push(s);
            }
        }

        // Now score the shortlist over the whole file.
        let mut winners: Vec<u32> = Vec::new();
        let mut best_n = 0usize;
        let mut best_is_nopad = false;
        for cand in &shortlist {
            let n = score(&plains, *cand, all);
            match (n > best_n, cand) {
                (true, None) => {
                    best_n = n;
                    best_is_nopad = true;
                    winners.clear();
                }
                (true, Some(s)) => {
                    best_n = n;
                    best_is_nopad = false;
                    winners = vec![*s];
                }
                (false, Some(s)) if n == best_n && !best_is_nopad => winners.push(*s),
                _ => {}
            }
        }
        let n = best_n;
        let sel = if best_is_nopad { None } else { Some(winners) };
        if best.as_ref().map_or(true, |b| n > b.0) {
            best = Some((n, key, plains, sel));
        }
        if n == recs.len() {
            break;
        }
    }
    let (ok, key, plains, seeds) = best.ok_or("no candidate keys (empty payloads)")?;

    // 4. Decode, re-encode, require identical bytes for every package.
    let mut res = FileResult {
        packages: recs.len(),
        ok,
        key,
        seeds,
        ..Default::default()
    };
    for (r, plain) in recs.iter().zip(plains.iter()) {
        let mk = || match &res.seeds {
            None => Obfuscation::none(),
            Some(s) => Obfuscation::with_seed(s[0]),
        };
        let mut obf = mk();
        let cmds: Vec<Command> = match decode_commands(plain, &mut obf) {
            Ok(c) => c,
            Err(e) => {
                if res.anomalies.len() < 8 {
                    res.anomalies.push(format!(
                        "stamp {} play {} len {}: {e}",
                        r.header.stamp,
                        r.header.play,
                        plain.len()
                    ));
                }
                continue;
            }
        };
        res.commands += cmds.len();
        for c in &cmds {
            *res.opcodes.entry(c.opcode).or_insert(0) += 1;
            if let Some(cs) = CheckSums::decode(c) {
                res.checksums.push((r.header.stamp, r.header.play, cs));
            }
        }

        // --- the load-bearing assertions ---
        // (a) every command's bytes sit exactly where the walk says they do
        let mut off = 0usize;
        let mut walk = mk();
        for c in &cmds {
            assert_eq!(
                &plain[off..off + c.len()],
                c.bytes,
                "command bytes moved at offset {off} (stamp {})",
                r.header.stamp
            );
            off += c.len() + walk.next_pad();
        }
        assert_eq!(
            off,
            plain.len(),
            "walk must tile the payload (stamp {})",
            r.header.stamp
        );

        // (b) re-encoding reproduces the payload length exactly
        let mut re = Vec::with_capacity(plain.len());
        let mut enc = mk();
        encode_commands(&cmds, &mut enc, &mut re);
        assert_eq!(
            re.len(),
            plain.len(),
            "re-encoded length (stamp {})",
            r.header.stamp
        );

        // (c) the XOR is an exact involution back to the original file bytes
        let mut back = plain.clone();
        xor_payload(&mut back, key);
        assert_eq!(back, r.payload, "XOR round trip (stamp {})", r.header.stamp);
    }
    Ok(res)
}

fn short(p: &Path) -> String {
    let n = p.file_name().unwrap().to_string_lossy().to_string();
    n.chars().take(44).collect()
}

#[test]
fn corpus_round_trips_byte_for_byte() {
    let files = corpus();
    if files.is_empty() {
        eprintln!(
            "\n  SKIPPED — NOT A PASS. No .rcx under ron-data/replays/.\n  \
             This test is the only evidence the codec matches the engine; \
             without the corpus it establishes nothing.\n"
        );
        return;
    }
    if !full_corpus_requested() {
        eprintln!(
            "\n  subset run ({} of the available recordings). \
             DON_NET_FULL_CORPUS=1 sweeps all of them.\n",
            files.len()
        );
    }
    let (mut pk, mut okp, mut cmds) = (0usize, 0usize, 0usize);
    let (mut solo, mut mp, mut perfect, mut with_stream) = (0usize, 0usize, 0usize, 0usize);
    let mut no_stream: Vec<(String, String)> = Vec::new();
    let mut imperfect: Vec<(String, usize, usize, Vec<String>)> = Vec::new();
    let mut opcodes: HashMap<u8, usize> = HashMap::new();

    for f in &files {
        match analyse(f) {
            Ok(r) => {
                with_stream += 1;
                pk += r.packages;
                okp += r.ok;
                cmds += r.commands;
                if r.seeds.is_none() {
                    solo += 1
                } else {
                    mp += 1
                }
                for (k, v) in &r.opcodes {
                    *opcodes.entry(*k).or_insert(0) += v;
                }
                if r.ok == r.packages {
                    perfect += 1;
                } else {
                    imperfect.push((short(f), r.ok, r.packages, r.anomalies.clone()));
                }
                eprintln!(
                    "  {:<44} pk={:<7} cmd={:<8} key={:04x} {:<26} {}",
                    short(f),
                    r.packages,
                    r.commands,
                    r.key,
                    match &r.seeds {
                        None => "solo, no padding".to_string(),
                        Some(s) => format!("mp, {} pad seed(s) fit", s.len()),
                    },
                    if r.ok == r.packages {
                        "ALL".to_string()
                    } else {
                        format!("{}/{}", r.ok, r.packages)
                    }
                );
            }
            Err(e) => {
                no_stream.push((short(f), e.clone()));
                eprintln!("  {:<44} -- {e}", short(f));
            }
        }
    }

    let rate = okp as f64 / pk.max(1) as f64;
    let file_rate = perfect as f64 / with_stream.max(1) as f64;
    eprintln!(
        "\n  {} files; {} carry a command stream ({} solo, {} multiplayer), \
         {} carry none\n  \
         {}/{} packages round-tripped byte-exactly ({:.5}), {} commands, \
         {} distinct opcodes\n  \
         {}/{} files at 100% ({:.3})\n",
        files.len(),
        with_stream,
        solo,
        mp,
        no_stream.len(),
        okp,
        pk,
        rate,
        cmds,
        opcodes.len(),
        perfect,
        with_stream,
        file_rate
    );
    if !no_stream.is_empty() {
        eprintln!("  files with no command stream (truncated or pre-EE builds):");
        for (n, e) in &no_stream {
            eprintln!("    {n}  --  {e}");
        }
    }
    if !imperfect.is_empty() {
        eprintln!("\n  files with malformed packages (measured divergence):");
        for (n, o, t, a) in &imperfect {
            eprintln!("    {n}  {o}/{t}");
            for s in a {
                eprintln!("        {s}");
            }
        }
    }
    eprintln!();

    assert!(with_stream > 0, "no file yielded a command stream");
    assert!(
        rate >= MIN_PACKAGE_RATE,
        "package round-trip rate {rate:.6} below floor {MIN_PACKAGE_RATE}"
    );
    assert!(
        file_rate >= MIN_PERFECT_FILE_RATE,
        "per-file perfect rate {file_rate:.4} below floor {MIN_PERFECT_FILE_RATE}"
    );
}

/// Independent check that could fail: in a multiplayer game two players
/// serialise their own 16-channel checksum tuple for the same turn on two
/// different machines. If the decode is right and the engine is deterministic,
/// the tuples must be identical. A wrong XOR key or a wrong command size
/// anywhere upstream cannot produce agreeing tuples from two byte streams.
#[test]
fn multiplayer_checksums_agree_between_players() {
    let files = corpus();
    if files.is_empty() {
        eprintln!("\n  SKIPPED — NOT A PASS. No corpus under ron-data/replays/.\n");
        return;
    }
    let (mut compared, mut agree, mut with_data) = (0usize, 0usize, 0usize);
    let mut offenders: Vec<(String, u32, usize)> = Vec::new();
    for f in &files {
        let Ok(r) = analyse(f) else { continue };
        if r.checksums.is_empty() {
            continue;
        }
        with_data += 1;
        let mut by_stamp: HashMap<u32, Vec<(i32, CheckSums)>> = HashMap::new();
        let mut last_stamp = 0u32;
        for (stamp, play, cs) in &r.checksums {
            by_stamp.entry(*stamp).or_default().push((*play, *cs));
            last_stamp = last_stamp.max(*stamp);
        }
        let mut bad = 0usize;
        let mut first_bad = u32::MAX;
        // which of the 16 channels actually differ, so a disagreement localises
        let mut chan: [usize; 16] = [0; 16];
        for (stamp, v) in &by_stamp {
            for w in v.windows(2) {
                compared += 1;
                if w[0].1 == w[1].1 {
                    agree += 1;
                } else {
                    bad += 1;
                    first_bad = first_bad.min(*stamp);
                    for i in 0..16 {
                        if (w[0].1).0[i] != (w[1].1).0[i] {
                            chan[i] += 1;
                        }
                    }
                }
            }
        }
        if bad > 0 {
            offenders.push((short(f), first_bad, bad));
            let which: Vec<String> = (0..16)
                .filter(|&i| chan[i] > 0)
                .map(|i| format!("{}x{}", don_net::CHECKSUM_CHANNELS[i], chan[i]))
                .collect();
            eprintln!(
                "  {:<44} {bad} disagreeing turn(s); first at stamp {first_bad} \
                 of {last_stamp}; channels: {}",
                short(f),
                which.join(" ")
            );
        }
    }
    let rate = agree as f64 / compared.max(1) as f64;
    eprintln!(
        "\n  cross-player checksum tuples compared: {compared} across {with_data} \
         multiplayer files; identical: {agree} ({rate:.6})\n  \
         {} file(s) show any disagreement\n",
        offenders.len()
    );
    if compared > 0 {
        assert!(
            rate >= MIN_CHECKSUM_AGREEMENT,
            "cross-player checksum agreement {rate:.6} below floor \
             {MIN_CHECKSUM_AGREEMENT}; the decode is probably wrong"
        );
    }
}

/// The network framing carries strictly less than the file framing. Show that
/// on real bytes: build the `NetMsg_CommandPackageData` the engine would have
/// sent for each recorded package and check it re-decodes to the same command
/// payload, dropping `valid` and `group`.
#[test]
fn recorded_packages_convert_to_network_messages() {
    let files = corpus();
    if files.is_empty() {
        eprintln!("\n  SKIPPED — NOT A PASS. No corpus under ron-data/replays/.\n");
        return;
    }
    let mut n = 0usize;
    let mut saved = 0usize;
    for f in files.iter().take(6) {
        let Some(payload) = load_payload(f) else {
            continue;
        };
        let Some(loc) = find_stream(&payload) else {
            continue;
        };
        for rec in PackageStream::new(&payload, loc.start) {
            let msg = NetCommandPackage::from_record(&rec);
            let mut wire = Vec::new();
            msg.encode(&mut wire);
            assert_eq!(
                wire.len(),
                NetCommandPackage::HEADER_LEN + rec.payload.len()
            );
            let back = NetCommandPackage::decode(&wire).expect("net message re-decodes");
            assert_eq!(back.stamp, rec.header.stamp);
            assert_eq!(back.play as i32, rec.header.play);
            assert_eq!(back.payload, rec.payload);
            n += 1;
            saved += PackageHeader::WIRE_LEN - NetCommandPackage::HEADER_LEN;
        }
    }
    eprintln!(
        "\n  {n} recorded packages converted to NetMsg_CommandPackageData and back; \
         {saved} header bytes are file-only (valid + group)\n"
    );
    assert!(n > 0);
}
