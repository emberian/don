//! The loop: step a simulation turn by turn against a real command stream and
//! compare our sixteen checksum channels against the recorded ones.
//!
//! # The metric
//!
//! For each channel independently: **how many consecutive turns, from the first
//! turn the recording checksums, do we agree?** That number — `survived` — is
//! the headline. It is per channel because the channels are independent walks,
//! so `walls` agreeing while `units` does not is information, not noise.
//!
//! Two things keep the number honest:
//!
//! - A channel we agree on **only because both sides are empty** is reported as
//!   `trivial`, separately from one where we walked bytes. Both are real
//!   comparisons; only the second is evidence about our mechanics.
//! - A turn where retail's own two clients disagree is excluded from `survived`
//!   and counted as `retail_disagreed`. 21 such comparisons exist in this
//!   corpus (`docs/tracks/headless-client.md`), and treating one as our bug
//!   would be reading noise as signal.

use crate::checksum::{Channel, Channels, CHANNELS, CHANNEL_NAMES, NUM_CHANNELS, NUM_WALKED};
use crate::replay::Replay;
use crate::state::SimState;
use crate::wire::{classify, CommandClass, CommandView, Order};
use std::collections::BTreeMap;

/// When in the turn the recorded tuple is taken, relative to that turn's
/// commands.
///
/// **Unresolved, and it matters.** The sender builds the package during
/// `PROCESS_TURN` and appends its checksum to the same package that carries the
/// player's new commands, but lockstep executes a turn's commands some number of
/// turns *later*. Until that latency is measured, the phase is a parameter of
/// the harness rather than a claim, and the run records which one it used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Checksum reflects state before this turn's commands are applied.
    BeforeCommands,
    /// Checksum reflects state after this turn's commands and this turn's step.
    AfterStep,
}

impl Phase {
    pub fn name(self) -> &'static str {
        match self {
            Phase::BeforeCommands => "before_commands",
            Phase::AfterStep => "after_step",
        }
    }
}

/// Per-channel outcome over one recording.
#[derive(Debug, Clone, Copy, Default)]
pub struct ChannelResult {
    /// Consecutive agreeing turns from the first checksummed turn.
    pub survived: u32,
    /// Turn number of the first disagreement, if any.
    pub first_divergence_turn: Option<i32>,
    /// Recorded value at the first disagreement.
    pub expected: u32,
    /// Our value at the first disagreement.
    pub got: u32,
    pub compares: u32,
    pub matches: u32,
    /// Compares where our walker touched zero bytes (both sides empty).
    pub trivial_matches: u32,
    /// Compares excluded because retail's own clients disagreed on this channel.
    pub retail_disagreed: u32,
}

/// Whole-run outcome.
#[derive(Debug, Clone)]
pub struct RunResult {
    pub file: String,
    pub version: Option<String>,
    pub phase: Phase,
    pub latency: u32,
    pub turns_total: usize,
    pub turns_checksummed: usize,
    pub first_turn: i32,
    pub last_turn: i32,
    pub players: Vec<i32>,
    pub frames_per_turn: Option<f64>,
    pub packages: usize,
    pub packages_decoded: usize,
    pub checksum_packets: usize,
    pub checksum_total_ok: usize,
    pub checksum_shape_ok: usize,
    pub rules_constant: Option<u32>,
    pub crossplay_comparisons: usize,
    pub crossplay_identical: usize,
    pub crossplay_per_channel: [usize; NUM_CHANNELS],
    /// The same control experiment joined on `stamp` instead of `group`, and
    /// how many of its disagreements compare packages from different turns.
    pub crossplay_stamp_comparisons: usize,
    pub crossplay_stamp_identical: usize,
    pub crossplay_stamp_wrong_turn: usize,
    pub channels: [ChannelResult; NUM_CHANNELS],
    /// Command counts by opcode over the whole recording.
    pub opcode_counts: BTreeMap<u8, usize>,
    pub sim_commands: usize,
    pub lockstep_commands: usize,
    pub presentation_commands: usize,
    /// Orders the harness decoded into a typed `Order` (not `Raw`).
    pub typed_orders: usize,
    /// Orders applied to the simulation. Zero until `don-sim` can act on one.
    pub orders_applied: usize,
    pub anomalies: Vec<String>,
}

impl RunResult {
    /// The headline: the longest per-channel survival, and which channel.
    pub fn best_channel(&self) -> (Channel, u32) {
        let mut best = (Channel::Units, 0u32);
        for (i, c) in CHANNELS.iter().enumerate() {
            if self.channels[i].survived > best.1 {
                best = (*c, self.channels[i].survived);
            }
        }
        best
    }
}

/// A simulation the harness can drive. `don-sim` does not implement this yet —
/// that is the point of the measurement — so the default implementation is the
/// explicit null model, and swapping in a real one changes nothing about the
/// harness.
pub trait Simulation {
    /// Apply one decoded order issued by `play`. Return true if the simulation
    /// actually acted on it.
    fn apply(&mut self, _play: i32, _order: &Order) -> bool {
        false
    }
    /// Advance one lockstep turn by `frames` simulation frames.
    fn step_turn(&mut self, _frames: u32) {}
    /// Produce the sixteen channels for the current state.
    fn check_all(&self) -> (Channels, [u64; NUM_WALKED]);
}

/// The null simulation: correct empty state, no mechanics.
///
/// It is not a stub for its own sake. An empty world is the *correct* model of a
/// game before anything is spawned, so every turn it matches is a turn our
/// walker and the engine's agree on — and the turn it stops matching is exactly
/// the turn retail acquires an object we do not have.
pub struct NullSim {
    pub state: SimState,
    pub world: don_sim::World,
    pub turns: u64,
}

impl Default for NullSim {
    fn default() -> Self {
        NullSim::new()
    }
}

impl NullSim {
    pub fn new() -> NullSim {
        NullSim {
            state: SimState::new(),
            world: don_sim::World::with_capacity(64, 1),
            turns: 0,
        }
    }
}

impl Simulation for NullSim {
    fn step_turn(&mut self, _frames: u32) {
        self.turns += 1;
        crate::state::SimBridge::populate(&self.world, &mut self.state);
    }
    fn check_all(&self) -> (Channels, [u64; NUM_WALKED]) {
        let (ch, outs) = self.state.check_all();
        let mut bytes = [0u64; NUM_WALKED];
        for i in 0..NUM_WALKED {
            bytes[i] = outs[i].bytes_walked;
        }
        (ch, bytes)
    }
}

/// Run one recording through a simulation and produce the divergence profile.
pub fn run<S: Simulation>(rep: &Replay, sim: &mut S, phase: Phase, latency: u32) -> RunResult {
    let mut res = RunResult {
        file: rep
            .path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default(),
        version: rep.version.clone(),
        phase,
        latency,
        turns_total: rep.turns.len(),
        turns_checksummed: rep.checksummed_turns(),
        first_turn: rep.turns.first().map(|t| t.turn).unwrap_or(0),
        last_turn: rep.turns.last().map(|t| t.turn).unwrap_or(0),
        players: rep.players.clone(),
        frames_per_turn: rep.frames_per_turn(),
        packages: rep.packages,
        packages_decoded: rep.packages_decoded,
        checksum_packets: rep.checksum_packets,
        checksum_total_ok: rep.checksum_total_ok,
        checksum_shape_ok: rep.checksum_shape_ok,
        rules_constant: None,
        crossplay_comparisons: 0,
        crossplay_identical: 0,
        crossplay_per_channel: [0; NUM_CHANNELS],
        crossplay_stamp_comparisons: 0,
        crossplay_stamp_identical: 0,
        crossplay_stamp_wrong_turn: 0,
        channels: [ChannelResult::default(); NUM_CHANNELS],
        opcode_counts: BTreeMap::new(),
        sim_commands: 0,
        lockstep_commands: 0,
        presentation_commands: 0,
        typed_orders: 0,
        orders_applied: 0,
        anomalies: rep.anomalies.clone(),
    };

    let (cc, ci, cper) = rep.crossplay();
    res.crossplay_comparisons = cc;
    res.crossplay_identical = ci;
    res.crossplay_per_channel = cper;
    let (sc, si, _, dt, _) = rep.crossplay_by_stamp_diag();
    res.crossplay_stamp_comparisons = sc;
    res.crossplay_stamp_identical = si;
    res.crossplay_stamp_wrong_turn = dt;

    // `rules` is static data; if it is constant across the recording, report it,
    // since it is the one channel with an exact published target.
    {
        let mut vals = rep
            .turns
            .iter()
            .filter_map(|t| t.any_checksums().map(|(_, c)| c.get(Channel::Rules)));
        if let Some(first) = vals.next() {
            if vals.all(|v| v == first) {
                res.rules_constant = Some(first);
            }
        }
    }

    // Per-channel "still agreeing" flags; a channel stops accumulating
    // `survived` at its own first disagreement, independently of the others.
    let mut alive = [true; NUM_CHANNELS];
    let frames = res.frames_per_turn.unwrap_or(1.0).round().max(1.0) as u32;

    // Commands issued at turn T execute at turn T + latency.
    let mut pending: BTreeMap<i32, Vec<(i32, Order)>> = BTreeMap::new();

    for t in &rep.turns {
        // ---- decode this turn's commands ----
        for p in &t.players {
            for c in &p.commands {
                *res.opcode_counts.entry(c.opcode).or_insert(0) += 1;
                match classify(c.opcode) {
                    CommandClass::Sim => res.sim_commands += 1,
                    CommandClass::Lockstep => res.lockstep_commands += 1,
                    CommandClass::Presentation => res.presentation_commands += 1,
                }
                if classify(c.opcode) != CommandClass::Sim {
                    continue;
                }
                let view = CommandView::new(c.opcode, &c.bytes);
                let order = Order::decode(&view);
                if !matches!(order, Order::Raw { .. }) {
                    res.typed_orders += 1;
                }
                pending
                    .entry(t.turn + latency as i32)
                    .or_default()
                    .push((p.play, order));
            }
        }

        let recorded = t.any_checksums();

        // ---- compare, phase = before commands ----
        if phase == Phase::BeforeCommands {
            if let Some((_, rec)) = recorded {
                compare(&mut res, &mut alive, t.turn, rec, sim, rep);
            }
        }

        // ---- apply this turn's due orders, then step ----
        if let Some(due) = pending.remove(&t.turn) {
            for (play, o) in due {
                if sim.apply(play, &o) {
                    res.orders_applied += 1;
                }
            }
        }
        sim.step_turn(frames);

        // ---- compare, phase = after step ----
        if phase == Phase::AfterStep {
            if let Some((_, rec)) = recorded {
                compare(&mut res, &mut alive, t.turn, rec, sim, rep);
            }
        }
    }
    res
}

fn compare<S: Simulation>(
    res: &mut RunResult,
    alive: &mut [bool; NUM_CHANNELS],
    turn: i32,
    rec: Channels,
    sim: &S,
    rep: &Replay,
) {
    let (ours, bytes) = sim.check_all();

    // Which channels do retail's own clients disagree on this turn? Those
    // carry no ground truth and are excluded.
    let tuples: Vec<Channels> = rep
        .turns
        .binary_search_by_key(&turn, |t| t.turn)
        .ok()
        .map(|i| {
            rep.turns[i]
                .players
                .iter()
                .filter_map(|p| p.checksums)
                .collect()
        })
        .unwrap_or_default();
    let mut contested = [false; NUM_CHANNELS];
    for tu in tuples.iter().skip(1) {
        for c in 0..NUM_CHANNELS {
            if tu.0[c] != tuples[0].0[c] {
                contested[c] = true;
            }
        }
    }

    for c in 0..NUM_CHANNELS {
        let r = &mut res.channels[c];
        if contested[c] {
            r.retail_disagreed += 1;
            continue;
        }
        r.compares += 1;
        let agree = ours.0[c] == rec.0[c];
        if agree {
            r.matches += 1;
            if c < NUM_WALKED && bytes[c] == 0 {
                r.trivial_matches += 1;
            }
            if alive[c] {
                r.survived += 1;
            }
        } else {
            if alive[c] {
                alive[c] = false;
                r.first_divergence_turn = Some(turn);
                r.expected = rec.0[c];
                r.got = ours.0[c];
            }
        }
    }
}

/// Render a run as a compact table.
pub fn format_table(r: &RunResult) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "{}\n  version {:<16} turns {} ({} checksummed, {}..{})  players {:?}  frames/turn {}\n",
        r.file,
        r.version.clone().unwrap_or_else(|| "?".into()),
        r.turns_total,
        r.turns_checksummed,
        r.first_turn,
        r.last_turn,
        r.players,
        r.frames_per_turn
            .map(|f| format!("{f:.2}"))
            .unwrap_or_else(|| "?".into()),
    ));
    s.push_str(&format!(
        "  packages {}/{} decoded   checksum packets {} (total-ok {}, adler-shaped {})\n",
        r.packages_decoded,
        r.packages,
        r.checksum_packets,
        r.checksum_total_ok,
        r.checksum_shape_ok
    ));
    if r.crossplay_comparisons > 0 {
        s.push_str(&format!(
            "  cross-player {}/{} identical\n",
            r.crossplay_identical, r.crossplay_comparisons
        ));
    }
    s.push_str(
        "  channel            survived  first-div      expected        got  compares  trivial\n",
    );
    for (i, name) in CHANNEL_NAMES.iter().enumerate() {
        let c = &r.channels[i];
        s.push_str(&format!(
            "  {name:<16} {:9}  {:>9}  {:>10}  {:>9}  {:8}  {:7}\n",
            c.survived,
            c.first_divergence_turn
                .map(|t| t.to_string())
                .unwrap_or_else(|| "-".into()),
            format!("{:08x}", c.expected),
            format!("{:08x}", c.got),
            c.compares,
            c.trivial_matches,
        ));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_sim_reports_the_empty_tuple() {
        let s = NullSim::new();
        let (ch, bytes) = s.check_all();
        assert_eq!(ch, Channels::empty_state());
        assert!(bytes.iter().all(|&b| b == 0));
    }
}
