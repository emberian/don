//! The pre-registered 32-bit test for checksum channel 14, `scenario_data`.
//!
//! `docs/assembly/scenario-initial-state.md` derived every checksum-visible field of
//! `ScenarioData` from `ScenarioFuncSet::init` `0x00a03c30`'s instruction stream, and left
//! exactly two free inputs: `internal_strings.xml` ordinals 5958 and 5959, which that
//! initializer indexes by fixed byte offset (`add eax, 0x1d178` / `0x1d18c`, 20-byte
//! `String` stride). §5 of that document registered the test *before* the strings were
//! available:
//!
//! > Compute `scenario_checksum(RetailInitialScenario::new().state(strings))` and compare
//! > against `CORPUS_INITIAL_SCENARIO_CHANNEL = 0x09922b90`. With every other byte already
//! > fixed, that is a single pre-registered 32-bit test. […] If it does not match, the
//! > residual is a *localised* discrepancy in a fully enumerated field set — report it
//! > rather than tuning fields until it agrees.
//!
//! Nothing in the producing path reads a recorded checksum, so this is a prediction the
//! corpus is free to falsify. There are no tunable fields: the two strings are bound
//! positionally, and every other byte is a named store in the initializer.
//!
//! `ron-data/` is gitignored copyrighted game content; without it these tests skip
//! **loudly**, because a skip is not a pass.

use don_replay::checksum::Channel;
use don_replay::harness::{self, Phase, WorldSim};
use don_replay::map_style::ron_data_root_for_replay;
use don_replay::replay::{corpus, Replay};
use don_replay::scenario_channel::{
    InitialScenarioChannel, CORPUS_INITIAL_SCENARIO_CHANNEL, INTERNAL_STRINGS_FILE,
};
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn ron_data() -> Option<PathBuf> {
    let root = repo_root().join("ron-data");
    root.join(INTERNAL_STRINGS_FILE).is_file().then_some(root)
}

fn skip(reason: &str) {
    eprintln!("\n  SKIPPED — NOT A PASS. {reason}\n  Nothing was established.\n");
}

/// Checksum-bearing recordings, selected by evidence rather than by filename.
fn checksummed_replays() -> &'static [Replay] {
    static CACHE: std::sync::OnceLock<Vec<Replay>> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| {
        const SCAN_BUDGET: usize = 14;
        let want: usize = if cfg!(debug_assertions) { 2 } else { 6 };
        let mut files: Vec<PathBuf> = corpus(&repo_root());
        files.sort_by_key(|p| {
            let n = p.file_name().unwrap().to_string_lossy().to_string();
            (!n.starts_with("Playback"), std::cmp::Reverse(n))
        });
        let mut out = Vec::new();
        for f in files.iter().take(SCAN_BUDGET) {
            if out.len() >= want {
                break;
            }
            if let Ok(r) = Replay::open(f) {
                if r.checksum_packets > 0 {
                    out.push(r);
                }
            }
        }
        out
    })
}

/// **The pre-registered test.** One 32-bit comparison, no free parameters.
#[test]
fn the_derived_game_init_scenario_state_is_the_value_retail_carries() {
    let Some(root) = ron_data() else {
        skip("ron-data/internal_strings.xml is not extracted.");
        return;
    };
    let channel = InitialScenarioChannel::load_from_ron_data(&root).expect("shipped string table");

    // The two ordinals came out of the initializer's instruction stream with no reference
    // to this file. Printing them keeps the derivation legible in the test log.
    eprintln!(
        "  int_str_array[5958] = {:?}\n  int_str_array[5959] = {:?}\n  \
         walked {} bytes -> 0x{:08x} (target 0x{:08x})",
        channel.general_powers_script_file,
        channel.temp_save,
        channel.bytes_walked,
        channel.checksum,
        CORPUS_INITIAL_SCENARIO_CHANNEL,
    );

    assert_eq!(
        channel.checksum, CORPUS_INITIAL_SCENARIO_CHANNEL,
        "residual on a fully enumerated field set: do NOT tune fields until it agrees, \
         report the discrepancy"
    );

    // 8,321 bytes of derived fixed state plus the two strings' UTF-16 payloads. This is
    // the number that says the agreement is not empty-state coincidence: an uninstalled
    // channel walks zero bytes and reads 1.
    let string_units = channel.general_powers_script_file.encode_utf16().count()
        + channel.temp_save.encode_utf16().count();
    assert_eq!(channel.bytes_walked, 8_321 + 2 * string_units as u64);
    assert_eq!(channel.bytes_walked, 8_453);
}

/// The target itself, re-measured from the recordings rather than trusted from prose:
/// every checksum-bearing file must carry it on its own first checksummed turn.
#[test]
fn every_recording_starts_channel_fourteen_at_the_same_value() {
    let reps = checksummed_replays();
    if reps.is_empty() {
        skip("no recording carries a CheckSumsCommand.");
        return;
    }
    for rep in reps {
        let first = rep
            .turns
            .iter()
            .find_map(|t| t.any_checksums())
            .expect("a checksum-bearing recording has a first checksummed turn");
        assert_eq!(
            first.1.get(Channel::ScenarioData),
            CORPUS_INITIAL_SCENARIO_CHANNEL,
            "{}: first checksummed turn does not carry the setup-independent initial value",
            rep.path.display()
        );
    }
    eprintln!(
        "  {} recordings, all starting channel 14 at 0x{CORPUS_INITIAL_SCENARIO_CHANNEL:08x}",
        reps.len()
    );
}

/// End to end: the harness installs the derived state, the channel is non-trivial, and it
/// survives until retail's own scenario counters move.
///
/// The producer is deliberately frozen at `Game::init` — no `don-sim` path writes
/// `units_killed`, `builds_destroyed` or `city_lost_to` — so this test asserts survival is
/// positive and *reports* the first divergence rather than demanding there is none.
#[test]
fn the_harness_installs_channel_fourteen_and_survives_a_real_recording() {
    let reps = checksummed_replays();
    if reps.is_empty() {
        skip("no recording carries a CheckSumsCommand.");
        return;
    }
    if ron_data().is_none() {
        skip("ron-data/internal_strings.xml is not extracted.");
        return;
    }
    for rep in reps {
        assert!(
            ron_data_root_for_replay(&rep.path).is_some(),
            "{}: replay is not owned by a ron-data root",
            rep.path.display()
        );
        let mut sim = WorldSim::from_replay(rep);
        assert!(
            sim.initial_scenario.is_some(),
            "{}: channel 14 not installed: {:?}",
            rep.path.display(),
            sim.initial_scenario_error
        );
        let run = harness::run(rep, &mut sim, Phase::BeforeCommands, 0);
        let ch = run.channels[Channel::ScenarioData as usize];
        eprintln!(
            "  {}: survived {} of {} compares ({} non-trivial), first divergence turn {:?} \
             expected 0x{:08x} got 0x{:08x}",
            run.file,
            ch.survived,
            ch.compares,
            ch.nontrivial_compares,
            ch.first_divergence_turn,
            ch.expected,
            ch.got,
        );
        assert!(ch.compares > 0);
        assert_eq!(
            ch.nontrivial_compares, ch.compares,
            "{}: every compare must walk real bytes",
            run.file
        );
        assert_eq!(
            ch.trivial_matches, 0,
            "{}: no empty-state matches",
            run.file
        );
        assert_eq!(
            ch.unmodelled_matches, 0,
            "{}: the channel has an installed producer",
            run.file
        );
        assert!(
            ch.survived > 0,
            "{}: the derived Game::init state did not hold for even one turn",
            run.file
        );
        assert_eq!(
            run.initial_scenario_checksum,
            Some(CORPUS_INITIAL_SCENARIO_CHANNEL)
        );
        assert_eq!(run.initial_scenario_walked_bytes, 8_453);
        assert_eq!(run.initial_scenario_error, None);
    }
}
