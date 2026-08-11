//! The corpus test for checksum channel 5, `groups`.
//!
//! `docs/assembly/groups-initial-state.md` derives every checksum-visible byte of the
//! `Groups` state a retail `Game::init` leaves behind, from two initializers and nothing
//! else: `Groups::clear` `0x00713f20` (512 slots, the `last_group` literals, the two
//! post-clear stores) and `Group::clear` `0x00713e80` (the per-slot field writes). There
//! is no shipped-data input, no replay input and no free parameter — which is exactly why
//! the value is the same in every game, and why the seven zero-AI recordings all carry it
//! on their first checksummed turn while the fourteen AI recordings do not.
//!
//! The in-crate unit test makes the single 32-bit statement. These tests are about the
//! *corpus*: that the derived value is a value retail actually records, that installing it
//! makes every compare walk 36,896 real bytes rather than agreeing by walking none, and
//! that survival is exactly the recordings whose first checksummed turn carries it.
//!
//! `ron-data/` is gitignored copyrighted game content; without it these tests skip
//! **loudly**, because a skip is not a pass.

use don_replay::checksum::Channel;
use don_replay::groups_channel::{
    InitialGroupsChannel, CORPUS_INITIAL_GROUPS_CHANNEL, GROUP_SLOTS, INITIAL_WALKED_BYTES,
};
use don_replay::harness::{self, Phase, WorldSim};
use don_replay::replay::{corpus, Replay};
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn skip(reason: &str) {
    eprintln!("\n  SKIPPED — NOT A PASS. {reason}\n  Nothing was established.\n");
}

/// Checksum-bearing recordings, selected by evidence rather than by filename. The scan
/// budget is wider than the sibling channel tests' because this channel's interesting
/// split is *between* recordings: a window that saw only AI games would establish half of
/// the claim.
fn checksummed_replays() -> &'static [Replay] {
    static CACHE: std::sync::OnceLock<Vec<Replay>> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| {
        const SCAN_BUDGET: usize = 22;
        // Same window in debug and release. This was `if cfg!(debug_assertions) { 3 } else
        // { 12 }`, which made the test's CONCLUSION depend on the build profile: only 21 of
        // 61 recordings carry checksums and only 7 of those start channel 5 at the derived
        // value, so a 3-file window can contain no instance of the split this test exists
        // to establish. It passed under `--release` and failed under a debug umbrella run.
        // A cheaper test that can assert something its own sample cannot contain is not
        // cheaper.
        let want: usize = 12;
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

fn first_recorded_groups(rep: &Replay) -> u32 {
    rep.turns
        .iter()
        .find_map(|t| t.any_checksums())
        .expect("a checksum-bearing recording has a first checksummed turn")
        .1
        .get(Channel::Groups)
}

/// The derived state, and the size that says an agreement here is not empty-state.
#[test]
fn the_derived_game_init_groups_state_is_thirty_six_thousand_real_bytes() {
    let ch = InitialGroupsChannel::derive();
    eprintln!(
        "  {} slots, walked {} bytes -> 0x{:08x} (target 0x{:08x})",
        ch.slots, ch.bytes_walked, ch.checksum, CORPUS_INITIAL_GROUPS_CHANNEL
    );
    assert_eq!(ch.slots, GROUP_SLOTS as u32);
    assert_eq!(ch.bytes_walked, INITIAL_WALKED_BYTES);
    assert_eq!(ch.bytes_walked, 36_896);
    assert_eq!(
        ch.checksum, CORPUS_INITIAL_GROUPS_CHANNEL,
        "residual on a fully enumerated field set: do NOT tune fields until it agrees, \
         report the discrepancy"
    );
}

/// The target itself, re-measured from the recordings rather than trusted from prose. The
/// value must occur, and the corpus must **not** be unanimous — a channel every recording
/// starts at the same value would be evidence the harness was reading a constant, and this
/// one genuinely splits.
#[test]
fn the_derived_value_occurs_in_the_corpus_and_the_corpus_is_not_unanimous() {
    let reps = checksummed_replays();
    if reps.is_empty() {
        skip("no recording carries a CheckSumsCommand.");
        return;
    }
    if reps.len() < 3 {
        skip("fewer than three checksum-bearing recordings are available locally.");
        return;
    }
    let mut carrying = 0usize;
    for rep in reps {
        let first = first_recorded_groups(rep);
        eprintln!(
            "  {:52} first checksummed turn 0x{:08x}",
            rep.path.file_name().unwrap().to_string_lossy(),
            first
        );
        if first == CORPUS_INITIAL_GROUPS_CHANNEL {
            carrying += 1;
        }
    }
    assert!(
        carrying > 0,
        "no scanned recording starts channel 5 at the derived Game::init value"
    );
    assert!(
        carrying < reps.len(),
        "every scanned recording starts channel 5 at the same value — the split this \
         channel is claimed to have is not visible in this window, so the claim is \
         untested here"
    );
    eprintln!("  {carrying} of {} recordings carry it", reps.len());
}

/// End to end. The harness installs the derived state on every recording, so every compare
/// walks 36,896 bytes and none of the agreement is empty-state; and survival is exactly the
/// recordings whose first checksummed turn carries the derived value.
///
/// The producer is deliberately frozen at `Game::init` — nothing in `don-sim` drives
/// `Groups::push_group` `0x0070f9e0` or any `Group::action_*` — so this reports the first
/// divergence rather than demanding there is none.
#[test]
fn the_harness_installs_channel_five_and_never_agrees_by_walking_nothing() {
    let reps = checksummed_replays();
    if reps.is_empty() {
        skip("no recording carries a CheckSumsCommand.");
        return;
    }
    let g = Channel::Groups as usize;
    let mut survived_any = false;
    for rep in reps {
        let first = first_recorded_groups(rep);
        let mut sim = WorldSim::from_replay(rep);
        assert_eq!(
            sim.initial_groups.checksum, CORPUS_INITIAL_GROUPS_CHANNEL,
            "the harness must install the derived value, not a recorded one"
        );
        let run = harness::run(rep, &mut sim, Phase::BeforeCommands, 0);
        let ch = run.channels[g];
        eprintln!(
            "  {}: survived {} of {} compares ({} non-trivial, {} bytes/compare), first \
             divergence turn {:?} expected 0x{:08x} got 0x{:08x}",
            run.file,
            ch.survived,
            ch.compares,
            ch.nontrivial_compares,
            ch.our_bytes_walked,
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
            ch.our_bytes_walked, INITIAL_WALKED_BYTES,
            "{}: the installed producer must be the 36,896-byte one",
            run.file
        );
        assert_eq!(
            ch.our_unsourced_walked, 0,
            "{}: every walked byte comes from an initializer store",
            run.file
        );
        assert_eq!(
            ch.trivial_matches, 0,
            "{}: an installed 36,896-byte producer cannot match trivially",
            run.file
        );
        assert_eq!(
            ch.unmodelled_matches, 0,
            "{}: the channel has an installed producer",
            run.file
        );
        assert_eq!(
            ch.retail_empty_compares, 0,
            "{}: retail never reads 1 on this channel — it always has 512 slots",
            run.file
        );

        // The biconditional: this producer is exactly the `Game::init` state, so it
        // survives a recording if and only if that recording's first checksummed turn is
        // still at `Game::init`.
        if first == CORPUS_INITIAL_GROUPS_CHANNEL {
            assert!(
                ch.survived > 0,
                "{}: retail carries the derived value on turn 1 and we did not agree",
                run.file
            );
            survived_any = true;
        } else {
            assert_eq!(
                ch.survived, 0,
                "{}: retail had already moved off Game::init and we agreed anyway",
                run.file
            );
            assert_eq!(ch.expected, first);
        }
    }
    assert!(
        survived_any,
        "no scanned recording exercised the agreeing branch"
    );
}

/// The comparator bites on this channel: a state one slot away from the derived one must
/// stop agreeing. Without this, `survived` could be measuring a producer that installs the
/// recorded value.
#[test]
fn a_one_slot_perturbation_loses_the_agreement() {
    use don_replay::groups_channel::{groups_checksum, GroupRecord, LAST_GROUP_INIT};

    let owner = don_replay::groups_channel::RetailInitialGroups::new();
    let base = groups_checksum(owner.slots(), owner.last_group()).unwrap();
    assert_eq!(base.checksum, CORPUS_INITIAL_GROUPS_CHANNEL);

    let mut perturbed: Vec<GroupRecord<'static>> = owner.slots().to_vec();
    perturbed[GROUP_SLOTS - 1].window.who = 1;
    let moved = groups_checksum(&perturbed, &LAST_GROUP_INIT).unwrap();
    assert_eq!(moved.bytes_walked, base.bytes_walked);
    assert_ne!(
        moved.checksum, base.checksum,
        "one byte of one of 512 slots did not reach the channel"
    );
}
