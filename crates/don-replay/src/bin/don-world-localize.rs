//! Offline localization for the first checksum-channel-12 divergence.
//!
//! This command never treats an Adler-32 value as a byte oracle.  It reports the exact
//! section-local coverage of the existing byte-owner ledger, checks which claimed values
//! still occur in the current model image, and names the earliest *possible* retail/model
//! difference.  An actual differing byte remains unavailable without a checksum-bound
//! retail walk image.

use don_replay::checksum::Channel;
use don_replay::harness::WorldSim;
use don_replay::replay::{corpus, Replay};
use don_replay::world_owner_frontier::{WorldByteSource, WorldOwnerLedger, WorldOwnerSnapshot};
use don_sim::systems::map_terrain::WorldSection;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const CHECKSUM_LIMIT: &str = "checksum-only evidence cannot identify a differing byte";

fn repo_root() -> PathBuf {
    if let Ok(root) = std::env::var("DON_ROOT") {
        return PathBuf::from(root);
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ByteRange {
    start: usize,
    end: usize,
}

impl ByteRange {
    fn len(self) -> usize {
        self.end - self.start
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct OwnedRange {
    range: ByteRange,
    source: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SectionAnalysis {
    section: WorldSection,
    current_bytes: usize,
    ledger_snapshot_bytes: usize,
    supported_bytes: usize,
    unknown_bytes: usize,
    model_delta_bytes: usize,
    owned_ranges: Vec<OwnedRange>,
    unknown_ranges: Vec<ByteRange>,
    model_delta_ranges: Vec<ByteRange>,
}

#[derive(Clone, Copy, Debug, Default)]
struct SectionTotal {
    files: usize,
    current_bytes: usize,
    supported_bytes: usize,
    unknown_bytes: usize,
    model_delta_bytes: usize,
    files_with_model_delta: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CandidateOffset {
    global: usize,
    section: WorldSection,
    section_offset: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct PeerStats {
    comparisons: usize,
    identical: usize,
}

fn source_name(source: &WorldByteSource) -> String {
    match source {
        WorldByteSource::ReplayScalar { field, span, .. } => {
            format!("replay:{field}@{}+{}", span.offset, span.bytes)
        }
        WorldByteSource::DerivedMapSize { selector, span, .. } => {
            format!("derived-map-size:{selector}@{}+{}", span.offset, span.bytes)
        }
        WorldByteSource::ReplayRulesProjection {
            serialized_span, ..
        } => format!(
            "replay-rules@{}+{}",
            serialized_span.offset, serialized_span.bytes
        ),
        WorldByteSource::ExactPortTransition {
            entry_va,
            resume_va,
            ..
        } => format!("exact-port:0x{entry_va:08x}->0x{resume_va:08x}"),
    }
}

fn retained_owner<'a>(
    ledger: &'a WorldOwnerLedger,
    current: &WorldOwnerSnapshot,
    section: WorldSection,
    offset: usize,
) -> Option<&'a WorldByteSource> {
    let prior = ledger.snapshot().section_bytes(section);
    let now = current.section_bytes(section);
    (offset < prior.len() && offset < now.len() && prior[offset] == now[offset])
        .then(|| ledger.owner_at(section, offset))
        .flatten()
}

fn ranges_where(len: usize, mut predicate: impl FnMut(usize) -> bool) -> Vec<ByteRange> {
    let mut ranges = Vec::new();
    let mut offset = 0usize;
    while offset < len {
        if !predicate(offset) {
            offset += 1;
            continue;
        }
        let start = offset;
        offset += 1;
        while offset < len && predicate(offset) {
            offset += 1;
        }
        ranges.push(ByteRange { start, end: offset });
    }
    ranges
}

fn section_analysis(
    ledger: &WorldOwnerLedger,
    current: &WorldOwnerSnapshot,
    section: WorldSection,
) -> SectionAnalysis {
    let prior = ledger.snapshot().section_bytes(section);
    let now = current.section_bytes(section);
    let mut owned_ranges = Vec::new();
    let mut offset = 0usize;
    while offset < now.len() {
        let Some(owner) = retained_owner(ledger, current, section, offset) else {
            offset += 1;
            continue;
        };
        let start = offset;
        offset += 1;
        while offset < now.len() && retained_owner(ledger, current, section, offset) == Some(owner)
        {
            offset += 1;
        }
        owned_ranges.push(OwnedRange {
            range: ByteRange { start, end: offset },
            source: source_name(owner),
        });
    }
    let unknown_ranges = ranges_where(now.len(), |index| {
        retained_owner(ledger, current, section, index).is_none()
    });
    let model_delta_ranges = ranges_where(now.len(), |index| {
        prior.get(index).copied() != now.get(index).copied()
    });
    let supported_bytes = owned_ranges.iter().map(|range| range.range.len()).sum();
    let unknown_bytes = unknown_ranges.iter().map(|range| range.len()).sum();
    let model_delta_bytes = model_delta_ranges.iter().map(|range| range.len()).sum();
    SectionAnalysis {
        section,
        current_bytes: now.len(),
        ledger_snapshot_bytes: prior.len(),
        supported_bytes,
        unknown_bytes,
        model_delta_bytes,
        owned_ranges,
        unknown_ranges,
        model_delta_ranges,
    }
}

fn all_sections(ledger: &WorldOwnerLedger, current: &WorldOwnerSnapshot) -> Vec<SectionAnalysis> {
    WorldSection::all()
        .into_iter()
        .map(|section| section_analysis(ledger, current, section))
        .collect()
}

fn earliest_candidate(
    current: &WorldOwnerSnapshot,
    sections: &[SectionAnalysis],
) -> Option<CandidateOffset> {
    sections.iter().find_map(|analysis| {
        analysis
            .unknown_ranges
            .first()
            .map(|range| CandidateOffset {
                global: current.section(analysis.section).start + range.start,
                section: analysis.section,
                section_offset: range.start,
            })
    })
}

fn world_peer_stats(replay: &Replay) -> PeerStats {
    let mut result = PeerStats::default();
    for turn in &replay.turns {
        let values: Vec<u32> = turn
            .players
            .iter()
            .filter_map(|player| player.checksums)
            .map(|channels| channels.get(Channel::World))
            .collect();
        for value in values.iter().skip(1) {
            result.comparisons += 1;
            result.identical += usize::from(*value == values[0]);
        }
    }
    result
}

fn first_world_checkpoint(replay: &Replay) -> Option<(i32, Vec<(i32, u32)>)> {
    replay.turns.iter().find_map(|turn| {
        let mut values: Vec<(i32, u32)> = turn
            .players
            .iter()
            .filter_map(|player| {
                player
                    .checksums
                    .map(|channels| (player.play, channels.get(Channel::World)))
            })
            .collect();
        if values.is_empty() {
            None
        } else {
            values.sort_by_key(|row| row.0);
            Some((turn.turn, values))
        }
    })
}

fn fmt_ranges(ranges: &[ByteRange], limit: usize) -> String {
    if ranges.is_empty() {
        return "-".into();
    }
    let mut rendered = ranges
        .iter()
        .take(limit)
        .map(|range| format!("{}..{}", range.start, range.end))
        .collect::<Vec<_>>()
        .join(",");
    if ranges.len() > limit {
        rendered.push_str(&format!(",...(+{} ranges)", ranges.len() - limit));
    }
    rendered
}

fn parse_args() -> Result<(Vec<PathBuf>, bool), String> {
    let mut files = Vec::new();
    let mut ranges = false;
    let mut use_corpus = false;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--corpus" => use_corpus = true,
            "--ranges" => ranges = true,
            "--help" | "-h" => {
                println!(
                    "usage: don-world-localize [--corpus] [--ranges] [FILE.rcx ...]\n\
                     Defaults to the repository corpus. --ranges prints exact section-local ranges."
                );
                std::process::exit(0);
            }
            value if value.starts_with('-') => return Err(format!("unknown flag {value}")),
            value => files.push(PathBuf::from(value)),
        }
    }
    if use_corpus || files.is_empty() {
        files = corpus(&repo_root());
    }
    Ok((files, ranges))
}

fn main() {
    let (files, show_ranges) = parse_args().unwrap_or_else(|error| {
        eprintln!("don-world-localize: {error}");
        std::process::exit(2);
    });
    if files.is_empty() {
        eprintln!("NO CORPUS — NOT A PASS");
        std::process::exit(2);
    }

    let mut opened = 0usize;
    let mut checksum_files = 0usize;
    let mut divergent = 0usize;
    let mut first_peer_agreed = 0usize;
    let mut coherent_owner_maps = 0usize;
    let mut peer_total = PeerStats::default();
    let mut totals: BTreeMap<i32, SectionTotal> = BTreeMap::new();
    let mut boundaries: BTreeMap<String, usize> = BTreeMap::new();
    let mut earliest: BTreeMap<(i32, usize), usize> = BTreeMap::new();
    let mut distinct_retail = std::collections::BTreeSet::new();
    let mut distinct_model = std::collections::BTreeSet::new();

    println!("offline World checksum localization (0x39 channel 12)");
    println!("limit: {CHECKSUM_LIMIT}");
    println!(
        "{:<52} {:>5} {:>5} {:>10} {:>10} {:>9} {:>11} {:>9}  {:<38}",
        "recording",
        "turn",
        "peers",
        "retail",
        "model",
        "walked",
        "value-bound",
        "delta",
        "boundary"
    );

    for file in files {
        let replay = match Replay::open(&file) {
            Ok(replay) => replay,
            Err(error) => {
                eprintln!("skip {}: {error}", file.display());
                continue;
            }
        };
        opened += 1;
        let Some((turn, peers)) = first_world_checkpoint(&replay) else {
            continue;
        };
        checksum_files += 1;
        let peer_agreed = peers.len() >= 2 && peers.iter().all(|row| row.1 == peers[0].1);
        first_peer_agreed += usize::from(peer_agreed);
        let retail = peer_agreed.then_some(peers[0].1);
        if let Some(value) = retail {
            distinct_retail.insert(value);
        }
        let stats = world_peer_stats(&replay);
        peer_total.comparisons += stats.comparisons;
        peer_total.identical += stats.identical;

        let sim = WorldSim::from_replay(&replay);
        let map = sim
            .initial_world
            .as_ref()
            .expect("checksum-bearing procedural replay has an initial World");
        let ledger = map
            .ownership
            .as_ref()
            .expect("production replay reconstruction installs a World owner ledger");
        let current = WorldOwnerSnapshot::capture(&map.world)
            .expect("the current World must preserve the canonical walk");
        assert_eq!(
            current.checksum, map.checksum,
            "cached World checksum drift"
        );
        let sections = all_sections(ledger, &current);
        coherent_owner_maps += usize::from(map.ownership_is_coherent());
        let supported: usize = sections.iter().map(|section| section.supported_bytes).sum();
        let delta: usize = sections
            .iter()
            .map(|section| section.model_delta_bytes)
            .sum();
        let model = current.checksum.full;
        distinct_model.insert(model);
        divergent += usize::from(retail.is_some_and(|expected| expected != model));

        let boundary = sim
            .initial_items
            .as_ref()
            .map(|items| items.boundary.name())
            .unwrap_or("no-initial-item-plan");
        *boundaries.entry(boundary.to_string()).or_default() += 1;
        let candidate = earliest_candidate(&current, &sections);
        if let Some(candidate) = candidate {
            *earliest
                .entry((candidate.section as i32, candidate.section_offset))
                .or_default() += 1;
        }
        let name = replay
            .path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        println!(
            "{:<52} {:>5} {:>5} {:>10} 0x{:08x} {:>9} {:>11} {:>9}  {:<38}",
            name,
            turn,
            peers.len(),
            retail.map_or_else(|| "DISAGREE".into(), |v| format!("0x{v:08x}")),
            model,
            current.image.len(),
            supported,
            delta,
            boundary,
        );
        if let Some(candidate) = candidate {
            println!(
                "  earliest lawful candidate: global {} = section {} + {}; actual byte unavailable",
                candidate.global, candidate.section as i32, candidate.section_offset
            );
        }
        if show_ranges {
            println!(
                "  owner ledger: snapshot=0x{:08x}/{} current=0x{:08x}/{} coherent={} ledger-owned={}",
                ledger.snapshot().checksum.full,
                ledger.snapshot().image.len(),
                current.checksum.full,
                current.image.len(),
                map.ownership_is_coherent(),
                ledger.coverage().owned_bytes,
            );
            for section in &sections {
                println!(
                    "  s{:02} current={} snapshot={} supported={} unknown={} model-delta={} unknown-ranges={} delta-ranges={}",
                    section.section as i32,
                    section.current_bytes,
                    section.ledger_snapshot_bytes,
                    section.supported_bytes,
                    section.unknown_bytes,
                    section.model_delta_bytes,
                    fmt_ranges(&section.unknown_ranges, 32),
                    fmt_ranges(&section.model_delta_ranges, 16),
                );
                for owned in &section.owned_ranges {
                    println!(
                        "      owned {}..{} {}",
                        owned.range.start, owned.range.end, owned.source
                    );
                }
            }
        }
        for section in sections {
            let total = totals.entry(section.section as i32).or_default();
            total.files += 1;
            total.current_bytes += section.current_bytes;
            total.supported_bytes += section.supported_bytes;
            total.unknown_bytes += section.unknown_bytes;
            total.model_delta_bytes += section.model_delta_bytes;
            total.files_with_model_delta += usize::from(section.model_delta_bytes != 0);
        }
    }

    println!("\nsummary");
    println!("  recordings opened: {opened}");
    println!("  recordings with 0x39 World checkpoints: {checksum_files}");
    println!(
        "  first checkpoints with >=2 agreeing same-group peers: {first_peer_agreed}/{checksum_files}"
    );
    println!(
        "  current model images with a transitioned/coherent owner ledger: {coherent_owner_maps}/{checksum_files}"
    );
    println!(
        "  all-turn same-group World peer comparisons: {}/{} identical",
        peer_total.identical, peer_total.comparisons
    );
    println!("  first-turn retail/model divergences: {divergent}/{checksum_files}");
    println!(
        "  distinct first-turn values: retail {}, model {}",
        distinct_retail.len(),
        distinct_model.len()
    );
    println!("  current source boundaries:");
    for (boundary, count) in boundaries {
        println!("    {count:>2}  {boundary}");
    }
    println!("  earliest possible difference (not an observed byte difference):");
    for ((section, offset), count) in earliest {
        println!("    {count:>2}  section {section} + {offset}");
    }
    println!("\nsection-local coverage over checksum-bearing recordings");
    println!(
        "  {:>3} {:>12} {:>12} {:>12} {:>12} {:>8}",
        "sec", "walked", "value-bound", "unknown", "model-delta", "files"
    );
    for (section, total) in totals {
        println!(
            "  {section:>3} {:>12} {:>12} {:>12} {:>12} {:>3}/{:<3}",
            total.current_bytes,
            total.supported_bytes,
            total.unknown_bytes,
            total.model_delta_bytes,
            total.files_with_model_delta,
            total.files,
        );
    }
    println!("\nconclusion: {CHECKSUM_LIMIT}; capture-bound byte comparison is still required.");

    if checksum_files == 0 {
        eprintln!("NO 0x39 CHECKPOINTS — NOT A PASS");
        std::process::exit(2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use don_replay::world_owner_frontier::{
        InitialWorldPrefixEvidence, ReplaySpan, WorldOwnerLedger,
    };
    use don_sim::systems::map_terrain::{WCoord, World};

    fn prefix_world() -> (World, WorldOwnerLedger) {
        let mut world = World::init(40, 40, 44, 4, 4);
        world.seed = 17;
        let ledger = WorldOwnerLedger::from_initial_prefix(
            &world,
            InitialWorldPrefixEvidence {
                replay_sha256: [0x5a; 32],
                map_size: 0,
                map_size_span: ReplaySpan::new(19, 1),
                seed: 17,
                seed_span: ReplaySpan::new(31, 4),
                rules: None,
            },
        )
        .unwrap();
        (world, ledger)
    }

    #[test]
    fn exact_current_support_is_section_local_across_a_walk_shape_change() {
        let (mut world, ledger) = prefix_world();
        world.add_starting_location(WCoord(10), WCoord(10));
        let current = WorldOwnerSnapshot::capture(&world).unwrap();
        let sections = all_sections(&ledger, &current);
        assert_eq!(ledger.coverage().owned_bytes, 52);
        assert_eq!(
            sections
                .iter()
                .map(|section| section.supported_bytes)
                .sum::<usize>(),
            52
        );
        assert!(
            sections[WorldSection::StartArrays as usize - 1].current_bytes
                > sections[WorldSection::StartArrays as usize - 1].ledger_snapshot_bytes
        );
        assert_eq!(
            earliest_candidate(&current, &sections),
            Some(CandidateOffset {
                global: 8,
                section: WorldSection::StartArrays,
                section_offset: 0,
            })
        );
    }

    #[test]
    fn changed_model_bytes_are_not_promoted_to_owned() {
        let (mut world, ledger) = prefix_world();
        world.wdata[0].land = 7;
        let current = WorldOwnerSnapshot::capture(&world).unwrap();
        let analysis = section_analysis(&ledger, &current, WorldSection::WData);
        assert_eq!(analysis.supported_bytes, 0);
        assert_eq!(analysis.model_delta_bytes, 1);
        assert_eq!(analysis.unknown_bytes, analysis.current_bytes);
    }

    #[test]
    fn checksum_limit_does_not_claim_a_byte_offset() {
        assert_eq!(
            CHECKSUM_LIMIT,
            "checksum-only evidence cannot identify a differing byte"
        );
    }
}
