//! Reading a `.rcx` into turns: recorded checksums plus per-player commands.
//!
//! Everything structural here is already derived and is used, not re-derived:
//! the container (`docs/derivation/replay-stream.md`), the 18-byte
//! `CommandPackage` framing and the multiplayer XOR + pad model (`don-net`),
//! and the 65-byte `CheckSumsCommand` (`docs/derivation/replay-checksum.md`).
//! This module's job is to turn all of that into a turn-indexed structure the
//! harness can step.

use crate::checksum::{Channels, NUM_CHANNELS};
use crate::initial::{parse_initial_state, InitialState};
use don_net::obfuscate::{rank_xor_keys, xor_payload};
use don_net::{
    decode_commands, find_stream, CheckSums, Command, Obfuscation, PackageHeader, PackageRecord,
    PackageStream,
};
use std::path::{Path, PathBuf};

/// One command as the harness sees it: owned bytes, so a turn can outlive the
/// de-obfuscated buffer it came from.
#[derive(Debug, Clone)]
pub struct OwnedCommand {
    pub opcode: u8,
    pub bytes: Vec<u8>,
}

/// One player's contribution to one turn.
#[derive(Debug, Clone)]
pub struct PlayerTurn {
    pub play: i32,
    /// `CommandPackage::stamp` — the simulation frame the package was built on.
    pub stamp: u32,
    pub commands: Vec<OwnedCommand>,
    /// The recorded 16-channel tuple, if this package carried one.
    pub checksums: Option<Channels>,
}

/// One turn of the recording.
#[derive(Debug, Clone, Default)]
pub struct Turn {
    /// `CommandPackage::group` — the monotone per-turn serial.
    pub turn: i32,
    pub players: Vec<PlayerTurn>,
}

impl Turn {
    pub fn checksums_for(&self, play: i32) -> Option<Channels> {
        self.players
            .iter()
            .find(|p| p.play == play)
            .and_then(|p| p.checksums)
    }
    /// Any player's recorded tuple, lowest player index first. With no desync
    /// every reporting player has the same tuple; when they disagree the
    /// harness reports it separately rather than silently picking one.
    pub fn any_checksums(&self) -> Option<(i32, Channels)> {
        let mut v: Vec<&PlayerTurn> = self
            .players
            .iter()
            .filter(|p| p.checksums.is_some())
            .collect();
        v.sort_by_key(|p| p.play);
        v.first().map(|p| (p.play, p.checksums.unwrap()))
    }
}

/// A decoded recording.
#[derive(Debug, Clone)]
pub struct Replay {
    pub path: PathBuf,
    /// Build string from the payload's leading UTF-16 version field, e.g.
    /// `00.2024.06.20`.
    pub version: Option<String>,
    /// The authoritative `Game::walk_data` / `GameInfo::walk_data` setup that
    /// precedes both the opaque initial-state block and the first command.
    pub initial: InitialState,
    pub stream_start: usize,
    pub payload_len: usize,
    pub packages: usize,
    /// Packages whose command list tiled the payload exactly.
    pub packages_decoded: usize,
    pub xor_key: u16,
    pub pad_seed: Option<u32>,
    pub players: Vec<i32>,
    pub turns: Vec<Turn>,
    /// Packages carrying a `CheckSumsCommand`.
    pub checksum_packets: usize,
    /// Packets whose 16th word equalled the wrapping sum of the first 15.
    pub checksum_total_ok: usize,
    /// Packets whose fifteen walked values were all adler-32-shaped.
    pub checksum_shape_ok: usize,
    pub anomalies: Vec<String>,
}

#[derive(Debug)]
pub enum LoadError {
    Unreadable(String),
    Initial(String),
    NoStream,
    FramingResidue(usize),
    NoKey,
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Unreadable(s) => write!(f, "unreadable: {s}"),
            LoadError::Initial(s) => write!(f, "initial setup: {s}"),
            LoadError::NoStream => write!(f, "no command-package chain"),
            LoadError::FramingResidue(n) => write!(f, "framing residue: {n} bytes"),
            LoadError::NoKey => write!(f, "no viable obfuscation key"),
        }
    }
}

/// Decompress a `.rcx`. Most are a single gzip member from offset 0; three in
/// the shipped corpus are stored raw. Shells out to the system `gzip` so this
/// crate needs no inflate dependency, exactly as `don-net`'s corpus tests do.
pub fn load_payload(p: &Path) -> Result<Vec<u8>, LoadError> {
    let raw = std::fs::read(p).map_err(|e| LoadError::Unreadable(e.to_string()))?;
    if raw.len() < 2 {
        return Err(LoadError::Unreadable("too short".into()));
    }
    if raw[0] != 0x1F || raw[1] != 0x8B {
        return Ok(raw);
    }
    let out = std::process::Command::new("gzip")
        .arg("-dc")
        .arg(p)
        .output()
        .map_err(|e| LoadError::Unreadable(format!("gzip: {e}")))?;
    if out.stdout.is_empty() {
        Err(LoadError::Unreadable("gzip produced nothing".into()))
    } else {
        Ok(out.stdout)
    }
}

/// Recover `(xor key, pad seed)` by decoding evidence, then decode every
/// package. Mirrors the search `don-net`'s corpus test uses: only the low 16
/// bits of the pad seed can change a draw and bits 8..15 are pinned by the XOR
/// key, so 256 candidates remain per key.
fn recover_obfuscation(
    recs: &[PackageRecord<'_>],
) -> Option<(u16, Option<u32>, Vec<Vec<u8>>, usize)> {
    const PROBE: usize = 48;
    let candidates = rank_xor_keys(recs.iter().map(|r| r.payload), 12);
    let mut best: Option<(usize, u16, Option<u32>, Vec<Vec<u8>>)> = None;
    for key in candidates {
        let plains: Vec<Vec<u8>> = recs
            .iter()
            .map(|r| {
                let mut b = r.payload.to_vec();
                xor_payload(&mut b, key);
                b
            })
            .collect();
        let score = |seed: Option<u32>, limit: usize| -> usize {
            plains
                .iter()
                .take(limit)
                .filter(|p| {
                    let mut o = match seed {
                        None => Obfuscation::none(),
                        Some(s) => Obfuscation::with_seed(s),
                    };
                    decode_commands(p, &mut o).is_ok()
                })
                .count()
        };
        let probe = PROBE.min(plains.len());
        let mut shortlist: Vec<Option<u32>> = vec![None];
        let mut probe_best = score(None, probe);
        for lo in 0u32..256 {
            let s = Some((((key & 0xFF) as u32) << 8) | lo);
            let n = score(s, probe);
            if n > probe_best {
                probe_best = n;
                shortlist = vec![s];
            } else if n == probe_best && n > 0 {
                shortlist.push(s);
            }
        }
        let mut winner: Option<u32> = None;
        let mut best_n = 0usize;
        for cand in &shortlist {
            let n = score(*cand, usize::MAX);
            if n > best_n {
                best_n = n;
                winner = *cand;
            }
        }
        if best.as_ref().is_none_or(|b| best_n > b.0) {
            best = Some((best_n, key, winner, plains));
        }
        if best_n == recs.len() {
            break;
        }
    }
    best.map(|(n, k, s, p)| (k, s, p, n))
}

impl Replay {
    pub fn open(path: &Path) -> Result<Replay, LoadError> {
        let payload = load_payload(path)?;
        let initial =
            parse_initial_state(&payload).map_err(|e| LoadError::Initial(e.to_string()))?;
        let loc = find_stream(&payload).ok_or(LoadError::NoStream)?;
        let recs: Vec<PackageRecord> = PackageStream::new(&payload, loc.start).collect();
        if recs.is_empty() {
            return Err(LoadError::NoStream);
        }
        let consumed: usize = recs
            .iter()
            .map(|r| PackageHeader::WIRE_LEN + r.payload.len())
            .sum();
        if loc.start + consumed != payload.len() {
            return Err(LoadError::FramingResidue(
                payload.len() - loc.start - consumed,
            ));
        }

        let (xor_key, pad_seed, plains, decoded) =
            recover_obfuscation(&recs).ok_or(LoadError::NoKey)?;

        let mut rep = Replay {
            path: path.to_path_buf(),
            version: Some(
                initial
                    .info
                    .version_string
                    .trim_start_matches('(')
                    .to_string(),
            ),
            initial,
            stream_start: loc.start,
            payload_len: payload.len(),
            packages: recs.len(),
            packages_decoded: decoded,
            xor_key,
            pad_seed,
            players: Vec::new(),
            turns: Vec::new(),
            checksum_packets: 0,
            checksum_total_ok: 0,
            checksum_shape_ok: 0,
            anomalies: Vec::new(),
        };

        // group -> index into turns, kept sorted by construction because the
        // stream is written in turn order.
        let mut by_turn: Vec<Turn> = Vec::new();
        let mut turn_index: std::collections::HashMap<i32, usize> = Default::default();

        for (r, plain) in recs.iter().zip(plains.iter()) {
            let mut obf = match pad_seed {
                None => Obfuscation::none(),
                Some(s) => Obfuscation::with_seed(s),
            };
            let cmds: Vec<Command> = match decode_commands(plain, &mut obf) {
                Ok(c) => c,
                Err(e) => {
                    if rep.anomalies.len() < 8 {
                        rep.anomalies.push(format!(
                            "turn {} play {}: {e}",
                            r.header.group, r.header.play
                        ));
                    }
                    continue;
                }
            };

            let mut pt = PlayerTurn {
                play: r.header.play,
                stamp: r.header.stamp,
                commands: Vec::with_capacity(cmds.len()),
                checksums: None,
            };
            for c in &cmds {
                if let Some(cs) = CheckSums::decode(c) {
                    let ch = Channels::from_recorded(cs.0);
                    rep.checksum_packets += 1;
                    if ch.total_is_consistent() {
                        rep.checksum_total_ok += 1;
                    }
                    if ch.adler_shaped() {
                        rep.checksum_shape_ok += 1;
                    }
                    pt.checksums = Some(ch);
                }
                pt.commands.push(OwnedCommand {
                    opcode: c.opcode,
                    bytes: c.bytes.to_vec(),
                });
            }
            if !rep.players.contains(&pt.play) {
                rep.players.push(pt.play);
            }
            let g = r.header.group;
            let idx = *turn_index.entry(g).or_insert_with(|| {
                by_turn.push(Turn {
                    turn: g,
                    players: Vec::new(),
                });
                by_turn.len() - 1
            });
            by_turn[idx].players.push(pt);
        }
        by_turn.sort_by_key(|t| t.turn);
        rep.players.sort_unstable();
        rep.turns = by_turn;
        Ok(rep)
    }

    /// Turns that carry at least one recorded checksum tuple.
    pub fn checksummed_turns(&self) -> usize {
        self.turns
            .iter()
            .filter(|t| t.any_checksums().is_some())
            .count()
    }

    /// Median simulation frames per turn, measured from `stamp` deltas across
    /// consecutive turns of one player. The lockstep turn is **not** the
    /// simulation frame, and the ratio is a property of the recording (turn
    /// length adapts to latency), so it is measured per file rather than
    /// assumed.
    pub fn frames_per_turn(&self) -> Option<f64> {
        let p = *self.players.first()?;
        let mut samples: Vec<i64> = Vec::new();
        let mut prev: Option<(i32, u32)> = None;
        for t in &self.turns {
            if let Some(pt) = t.players.iter().find(|x| x.play == p) {
                if let Some((pg, ps)) = prev {
                    let dg = (t.turn - pg) as i64;
                    let ds = pt.stamp as i64 - ps as i64;
                    if dg > 0 && ds >= 0 {
                        samples.push(ds * 1000 / dg);
                    }
                }
                prev = Some((t.turn, pt.stamp));
            }
        }
        if samples.is_empty() {
            return None;
        }
        samples.sort_unstable();
        Some(samples[samples.len() / 2] as f64 / 1000.0)
    }

    /// Cross-player agreement, aligning players by `CommandPackage::group` —
    /// the monotone per-turn serial. Returns
    /// `(comparisons, identical, per-channel disagreement counts)`.
    ///
    /// This is the harness's control experiment. Retail's own two clients
    /// disagreeing puts a floor under what our sim can be asked to match, and a
    /// comparator that cannot find a real disagreement cannot find ours either.
    pub fn crossplay(&self) -> (usize, usize, [usize; NUM_CHANNELS]) {
        let mut comparisons = 0usize;
        let mut identical = 0usize;
        let mut per = [0usize; NUM_CHANNELS];
        for t in &self.turns {
            let tuples: Vec<Channels> = t.players.iter().filter_map(|p| p.checksums).collect();
            for i in 1..tuples.len() {
                comparisons += 1;
                if tuples[i] == tuples[0] {
                    identical += 1;
                } else {
                    for c in 0..NUM_CHANNELS {
                        if tuples[i].0[c] != tuples[0].0[c] {
                            per[c] += 1;
                        }
                    }
                }
            }
        }
        (comparisons, identical, per)
    }

    /// The same comparison aligned by `CommandPackage::stamp` (the simulation
    /// frame) instead of by `group`.
    ///
    /// Both keys are in the header and both look like "the same moment", so
    /// which one is the right join is a real question, and the answer is
    /// measurable: a wrong join manufactures disagreements out of two
    /// perfectly-agreeing clients. `don-net`'s corpus test joins on `stamp`;
    /// the harness reports both so the difference is visible rather than
    /// inherited.
    pub fn crossplay_by_stamp(&self) -> (usize, usize, [usize; NUM_CHANNELS]) {
        let (c, i, p, _, _) = self.crossplay_by_stamp_diag();
        (c, i, p)
    }

    /// As [`Replay::crossplay_by_stamp`], plus the diagnosis: of the
    /// disagreeing pairs, how many were **not the same turn**, and how many
    /// stamp buckets mix turns at all.
    ///
    /// This is the whole question. If a disagreement is between two packages
    /// with different `group` values, it is a join error, not a desync — the
    /// comparison never had ground truth in it.
    pub fn crossplay_by_stamp_diag(&self) -> (usize, usize, [usize; NUM_CHANNELS], usize, usize) {
        // (stamp) -> [(play, group, tuple)]
        let mut by_stamp: std::collections::BTreeMap<u32, Vec<(i32, i32, Channels)>> =
            Default::default();
        for t in &self.turns {
            for p in &t.players {
                if let Some(c) = p.checksums {
                    by_stamp
                        .entry(p.stamp)
                        .or_default()
                        .push((p.play, t.turn, c));
                }
            }
        }
        let mut comparisons = 0usize;
        let mut identical = 0usize;
        let mut per = [0usize; NUM_CHANNELS];
        let mut disagree_diff_turn = 0usize;
        let mut mixed_buckets = 0usize;
        for v in by_stamp.values() {
            if v.iter().any(|e| e.1 != v[0].1) {
                mixed_buckets += 1;
            }
            for w in v.windows(2) {
                comparisons += 1;
                if w[0].2 == w[1].2 {
                    identical += 1;
                } else {
                    if w[0].1 != w[1].1 {
                        disagree_diff_turn += 1;
                    }
                    for c in 0..NUM_CHANNELS {
                        if (w[0].2).0[c] != (w[1].2).0[c] {
                            per[c] += 1;
                        }
                    }
                }
            }
        }
        (
            comparisons,
            identical,
            per,
            disagree_diff_turn,
            mixed_buckets,
        )
    }
}

/// Every `.rcx` under `ron-data/replays/` and `ron-data/replays/multi/`, sorted.
pub fn corpus(root: &Path) -> Vec<PathBuf> {
    let mut v = Vec::new();
    for dir in [
        root.join("ron-data/replays"),
        root.join("ron-data/replays/multi"),
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
    v
}
