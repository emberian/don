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
//!   and counted as `retail_disagreed`. The current corpus has zero such
//!   disagreements when joined on the actual turn serial; retaining the gate
//!   prevents a future desynced recording from being attributed to our sim.

use crate::checksum::{Channel, Channels, CHANNELS, CHANNEL_NAMES, NUM_CHANNELS, NUM_WALKED};
use crate::map_style::MapStyleStaticData;
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
    /// Matches on a channel this world has **no installed producer for**
    /// (`ChannelSource::Absent`, or an uninitialized conditional producer). A subset of
    /// `trivial_matches`, and the honest name for it: not "our empty model was right",
    /// but "this world did not produce this channel". `walls` surviving 25,442 turns is
    /// entirely this.
    pub unmodelled_matches: u32,
    /// Compares where our walker touched at least one byte. The only compares
    /// that are evidence about our mechanics.
    pub nontrivial_compares: u32,
    /// Bytes our walker handed the visitor on the last compare.
    pub our_bytes_walked: u64,
    /// Of those bytes, the count present only as explicit zero placeholders
    /// because the current reconstruction has no authoritative source.
    pub our_unsourced_walked: u64,
    /// Compares where *retail's* value was 1 — the engine walked nothing either.
    /// A channel we "survive" for its whole recording while this equals
    /// `compares` was never tested at all.
    pub retail_empty_compares: u32,
    /// First turn on which retail's value left 1. Our deadline: this is the turn
    /// by which we must have a producer, measured from the recording itself.
    pub retail_first_nonempty_turn: Option<i32>,
    /// Compares excluded because retail's own clients disagreed on this channel.
    pub retail_disagreed: u32,
}

/// Whole-run outcome.
#[derive(Debug, Clone)]
pub struct RunResult {
    pub file: String,
    pub version: Option<String>,
    pub initial_prefix_bytes: usize,
    pub initial_seed: u32,
    pub initial_map_style: u8,
    pub initial_map_size: u8,
    pub initial_map_edge: Option<i32>,
    pub initial_active_players: usize,
    pub initial_teams: Vec<u8>,
    /// First boundary reached by the executable initial-item reconstruction.
    pub initial_item_boundary: crate::initial::InitialItemBoundary,
    /// Catalog-validated concrete style identity, when the lawful local static
    /// data was present beside the replay corpus.
    pub initial_item_style_key: Option<String>,
    pub initial_item_style_filename: Option<String>,
    pub initial_item_default_terrain_groups: usize,
    pub initial_item_selected_terrain_groups: usize,
    pub initial_item_selected_terrain_groups_present: bool,
    pub initial_item_effective_terrain_groups: usize,
    pub initial_item_default_goodies: usize,
    pub initial_item_selected_goodies: usize,
    pub initial_item_selected_goodies_present: bool,
    pub initial_item_effective_goodies: usize,
    /// Proven top-level direct RNG call sites. This is not the dynamic draw
    /// count; indirect callees and retry loops remain the next generator work.
    pub initial_item_known_direct_rng_sites: Vec<u32>,
    /// Executed style prefix and exact RNG state at its first unavailable
    /// primitive. Unlike the static site inventory, this is a dynamic draw
    /// schedule for the replay's concrete branch.
    pub initial_continent: Option<crate::continent::ContinentReceipt>,
    pub initial_item_style_error: Option<String>,
    pub initial_tile_selection: Option<crate::fractal_boundary::TileSelectionBoundary>,
    pub initial_fertility_error: Option<String>,
    pub initial_fill_fertile_cells: Option<i32>,
    /// Executed `TerrainGroups::place_all` `0x006a70d0` survey. It commits no
    /// world state; it names the first primitive inside the call that the
    /// reconstruction cannot run and how far the group dispatcher got.
    pub initial_place_all: Option<crate::place_all_advance::PlaceAllAdvanceReceipt>,
    pub initial_place_all_error: Option<String>,
    /// Distinct `.rcx` bytes carrying the known scalar worldgen tuple.
    pub initial_item_scalar_source_bytes: usize,
    pub initial_rules_offset: Option<usize>,
    pub initial_rules_serialized_bytes: usize,
    pub initial_rules_walked_bytes: u64,
    pub initial_rules_checksum: Option<u32>,
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
    /// Per-channel subset of `check_all().1` not yet sourced by simulation
    /// state. Defaults to zero for external implementations.
    fn unsourced_walked(&self) -> [u64; NUM_WALKED] {
        [0; NUM_WALKED]
    }
    /// Per-world producer admission. Conditional channels such as `items` are absent
    /// until their runtime owner is initialized, even though the bridge supports them.
    fn installed_channels(&self) -> [bool; NUM_WALKED] {
        std::array::from_fn(|i| {
            crate::check_all::CHANNEL_SOURCE[i] == crate::check_all::ChannelSource::Modelled
        })
    }
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
    fn unsourced_walked(&self) -> [u64; NUM_WALKED] {
        std::array::from_fn(|i| self.state.unsourced_walked_bytes(i))
    }
    fn installed_channels(&self) -> [bool; NUM_WALKED] {
        let dynamic = self.state.installed_channels();
        std::array::from_fn(|i| {
            dynamic[i]
                || crate::check_all::CHANNEL_SOURCE[i] == crate::check_all::ChannelSource::Modelled
        })
    }
}

/// A simulation that really is a `don_sim::World`: it steps the tick, images
/// its rows into engine layout through [`crate::state::SimBridge`], and reports
/// the composed [`crate::check_all::CheckAll`].
///
/// The difference from [`NullSim`] is that this one *runs* — `World::step` per
/// simulation frame, `population` carried across turns — so the moment `don-sim`
/// can spawn from a command, the `units` channel on the scoreboard stops being a
/// comparison of nothing against something.
///
/// [`WorldSim::seeded`] exists for one purpose and is labelled for it: to show
/// the whole path end to end against a real recording before any producer
/// exists. A seeded population is **not** derived from the recording and cannot
/// match retail; what it demonstrates is that the bytes now flow, and the run
/// reports it as `seed_units`, never as fidelity.
pub struct WorldSim {
    pub world: don_sim::World,
    pub state: SimState,
    pub turns: u64,
    pub frames: u64,
    pub seed_units: u32,
    /// Prefix-derived map slice. The executable continent prefix populates
    /// exact continuation state, but provisional geometry is not promoted to
    /// final-checksum source coverage until its downstream generator completes;
    /// the remaining initial-world bytes stay explicitly unsourced.
    pub initial_world: Option<crate::initial::InitialWorld>,
    /// Exact replay-carried prefix and first absent input for initial goodies.
    pub initial_items: Option<crate::initial::InitialItemReconstruction>,
    /// Executed continent-prefix receipt, including exact RNG state and the
    /// first unported geometry call.
    pub initial_continent: Option<crate::continent::ContinentReceipt>,
    /// Result of executing that prefix against `initial_world`. A blocked plan
    /// must leave the item channel uninstalled.
    pub initial_item_error: Option<crate::initial::InitialItemReconstructionError>,
    /// Why local static content could not advance the plan, if it could not.
    pub initial_item_style_error: Option<String>,
    /// Exact checksum-visible static state projected from the replay's own
    /// SaveGame Rules section. Unlike `initial_world`, this slice is complete:
    /// all 997,846 visited bytes are present and independently checkpointed.
    pub initial_rules: Option<crate::initial::InitialRules>,
}

impl Default for WorldSim {
    fn default() -> Self {
        WorldSim::new()
    }
}

impl WorldSim {
    pub fn new() -> WorldSim {
        WorldSim {
            world: don_sim::World::with_capacity(4096, 1),
            state: SimState::new(),
            turns: 0,
            frames: 0,
            seed_units: 0,
            initial_world: None,
            initial_items: None,
            initial_continent: None,
            initial_item_error: None,
            initial_item_style_error: None,
            initial_rules: None,
        }
    }

    /// Build the largest simulation state justified by the recording before
    /// its first command: map dimensions, default rule limits, and map seed.
    pub fn from_replay(rep: &Replay) -> WorldSim {
        let mut s = WorldSim::new();
        s.initial_world = rep.initial.reconstruct_world();
        let (items, style_error, continent, execution_error) =
            initial_items_for_replay(rep, s.initial_world.as_mut());
        s.initial_items = Some(items);
        s.initial_continent = continent;
        s.initial_item_style_error = style_error;
        s.initial_rules = rep.initial.rules;
        s.initial_item_error = execution_error;
        if s.initial_item_error.is_none() {
            if let (Some(items), Some(map)) = (&s.initial_items, &mut s.initial_world) {
                s.initial_item_error = items.apply(&mut s.world, &mut map.world).err();
            }
        }
        s.populate_state();
        s
    }

    fn populate_state(&mut self) {
        if let Some(map) = &self.initial_world {
            crate::state::SimBridge::populate_with_map_checksum(
                &self.world,
                &map.checksum,
                (map.world.xs, map.world.ys),
                map.unsourced_walked_bytes(),
                &mut self.state,
            );
        } else {
            crate::state::SimBridge::populate(&self.world, &mut self.state);
        }
        if let Some(rules) = &self.initial_rules {
            crate::state::SimBridge::populate_replay_rules(rules, &mut self.state);
        }
    }

    /// A world pre-populated with `per_owner` units in each of `owners` owner
    /// slots. Declared, not derived — see the type docs.
    pub fn seeded(owners: u8, per_owner: u32) -> WorldSim {
        let mut s = WorldSim::new();
        for who in 0..owners {
            for _ in 0..per_owner {
                if s.world.spawn(who).is_some() {
                    s.seed_units += 1;
                }
            }
        }
        s.populate_state();
        s
    }
}

impl Simulation for WorldSim {
    fn step_turn(&mut self, frames: u32) {
        self.turns += 1;
        for _ in 0..frames {
            self.world.step();
            self.frames += 1;
        }
        self.populate_state();
    }
    fn check_all(&self) -> (Channels, [u64; NUM_WALKED]) {
        let (ch, outs) = self.state.check_all();
        let mut bytes = [0u64; NUM_WALKED];
        for i in 0..NUM_WALKED {
            bytes[i] = outs[i].bytes_walked;
        }
        (ch, bytes)
    }
    fn unsourced_walked(&self) -> [u64; NUM_WALKED] {
        std::array::from_fn(|i| self.state.unsourced_walked_bytes(i))
    }
    fn installed_channels(&self) -> [bool; NUM_WALKED] {
        self.state.installed_channels()
    }
}

/// Run one recording through a simulation and produce the divergence profile.
pub fn run<S: Simulation>(rep: &Replay, sim: &mut S, phase: Phase, latency: u32) -> RunResult {
    let mut report_world = rep.initial.reconstruct_world();
    let (initial_items, initial_item_style_error, initial_continent, _execution_error) =
        initial_items_for_replay(rep, report_world.as_mut());
    let style = initial_items.style.as_ref();
    let mut res = RunResult {
        file: rep
            .path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default(),
        version: rep.version.clone(),
        initial_prefix_bytes: rep.initial.bytes_walked,
        initial_seed: rep.initial.info.seed,
        initial_map_style: rep.initial.info.settings.map_style,
        initial_map_size: rep.initial.info.settings.map_size,
        initial_map_edge: rep.initial.info.settings.map_edge_world_cells(),
        initial_active_players: rep.initial.active_players().count(),
        initial_teams: rep.initial.active_players().map(|p| p.team).collect(),
        initial_item_boundary: initial_items.boundary,
        initial_item_style_key: style.map(|s| s.identity.key.to_owned()),
        initial_item_style_filename: style.and_then(|s| s.identity.filename).map(str::to_owned),
        initial_item_default_terrain_groups: style.map_or(0, |s| s.default_terrain_groups.len()),
        initial_item_selected_terrain_groups: style.map_or(0, |s| s.selected_terrain_groups.len()),
        initial_item_selected_terrain_groups_present: style
            .is_some_and(|s| s.selected_terrain_groups_section_present),
        initial_item_effective_terrain_groups: style
            .map_or(0, |s| s.effective_terrain_groups().len()),
        initial_item_default_goodies: style.map_or(0, |s| s.default_goodies.len()),
        initial_item_selected_goodies: style.map_or(0, |s| s.selected_goodies.len()),
        initial_item_selected_goodies_present: style
            .is_some_and(|s| s.selected_goodies_section_present),
        initial_item_effective_goodies: style.map_or(0, |s| s.effective_goodies().len()),
        initial_item_known_direct_rng_sites: style
            .map(MapStyleStaticData::known_direct_rng_sites)
            .unwrap_or_default(),
        initial_continent,
        initial_item_style_error,
        initial_tile_selection: initial_items.tile_selection.clone(),
        initial_fertility_error: initial_items
            .fertility_error
            .as_ref()
            .map(ToString::to_string),
        initial_fill_fertile_cells: initial_items
            .fill_fertile
            .map(|receipt| receipt.fertile_cells),
        initial_place_all: initial_items.place_all_advance.clone(),
        initial_place_all_error: initial_items
            .place_all_advance_error
            .as_ref()
            .map(|error| format!("{error:?}")),
        initial_item_scalar_source_bytes: initial_items.scalar_source_bytes(),
        initial_rules_offset: rep.initial.rules.map(|r| r.serialized_offset),
        initial_rules_serialized_bytes: rep.initial.rules.map_or(0, |r| r.serialized_bytes),
        initial_rules_walked_bytes: rep.initial.rules.map_or(0, |r| r.walked_bytes),
        initial_rules_checksum: rep.initial.rules.map(|r| r.checksum),
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

fn initial_items_for_replay(
    rep: &Replay,
    mut map: Option<&mut crate::initial::InitialWorld>,
) -> (
    crate::initial::InitialItemReconstruction,
    Option<String>,
    Option<crate::continent::ContinentReceipt>,
    Option<crate::initial::InitialItemReconstructionError>,
) {
    use crate::map_style::{ron_data_root_for_replay, MapStyleStaticData};

    let base = rep.initial.reconstruct_items();
    if !matches!(
        base.boundary,
        crate::initial::InitialItemBoundary::MapStyleContentUnavailable { .. }
    ) {
        return (base, None, None, None);
    }
    let Some(root) = ron_data_root_for_replay(&rep.path) else {
        return (
            base,
            Some("replay path has no owning ron-data/rules.xml ancestor".into()),
            None,
            None,
        );
    };
    match MapStyleStaticData::load_from_ron_data(&root, base.inputs.map_style) {
        Ok(style) => match rep.initial.reconstruct_items_with_style(style) {
            Ok(mut plan) => {
                let mut receipt = None;
                let mut execution_error = None;
                if let Some(map) = map.as_deref_mut() {
                    let tilesets_xml = root.join("tilesets.xml");
                    match plan.advance_continent_prefix_with_tilesets(map, &tilesets_xml) {
                        Ok(done) => {
                            map.checksum = map.world.checksum_sections();
                            receipt = Some(done);
                        }
                        Err(error) => execution_error = Some(error),
                    }
                }
                (plan, None, receipt, execution_error)
            }
            Err(e) => (
                base,
                Some(format!("static-style admission failed: {e:?}")),
                None,
                None,
            ),
        },
        Err(e) => (base, Some(e.to_string()), None, None),
    }
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
    let unsourced = sim.unsourced_walked();
    let installed = sim.installed_channels();

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
        // Retail's own emptiness, measured from the recording: adler-32 of
        // nothing is 1, so `rec == 1` is the engine saying this channel had no
        // elements on this turn. Without this the survival numbers cannot be
        // read at all.
        if c < NUM_WALKED {
            if rec.0[c] == 1 {
                r.retail_empty_compares += 1;
            } else if r.retail_first_nonempty_turn.is_none() {
                r.retail_first_nonempty_turn = Some(turn);
            }
            r.our_bytes_walked = bytes[c];
            r.our_unsourced_walked = unsourced[c];
            if bytes[c] > 0 {
                r.nontrivial_compares += 1;
            }
        }
        let agree = ours.0[c] == rec.0[c];
        if agree {
            r.matches += 1;
            if c < NUM_WALKED && bytes[c] == 0 {
                r.trivial_matches += 1;
                if !installed[c] {
                    r.unmodelled_matches += 1;
                }
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
        "  initial Game/GameInfo {} bytes  seed {:08x}  map style {} size {} edge {}  active {} teams {:?}\n",
        r.initial_prefix_bytes,
        r.initial_seed,
        r.initial_map_style,
        r.initial_map_size,
        r.initial_map_edge
            .map(|x| x.to_string())
            .unwrap_or_else(|| "unresolved".into()),
        r.initial_active_players,
        r.initial_teams,
    ));
    s.push_str(&format!(
        "  initial items blocked at {}  ({} exact scalar source bytes; replay bytes for style/tables/candidates/post-RNG: 0/0/0/0)\n",
        r.initial_item_boundary.name(),
        r.initial_item_scalar_source_bytes,
    ));
    if let Some(key) = &r.initial_item_style_key {
        s.push_str(&format!(
            "  map style {key} ({}) admitted: terrain groups default/selected/effective {}/{}/{}, goodies {}/{}/{}, selected sections present {}/{}, {} known direct RNG sites\n",
            r.initial_item_style_filename.as_deref().unwrap_or("unresolved"),
            r.initial_item_default_terrain_groups,
            r.initial_item_selected_terrain_groups,
            r.initial_item_effective_terrain_groups,
            r.initial_item_default_goodies,
            r.initial_item_selected_goodies,
            r.initial_item_effective_goodies,
            r.initial_item_selected_terrain_groups_present,
            r.initial_item_selected_goodies_present,
            r.initial_item_known_direct_rng_sites.len(),
        ));
    } else if let Some(error) = &r.initial_item_style_error {
        s.push_str(&format!("  map style static data unavailable: {error}\n"));
    }
    if let Some(selection) = &r.initial_tile_selection {
        s.push_str(&format!(
            "  tileset {} selected at bucket {} from {:?} after {} map-style pass(es); main RNG handoff {:08x}\n",
            selection.tileset,
            selection.bucket,
            selection.draw,
            selection.passes.len(),
            selection.main_random_state_after as u32,
        ));
    }
    if let Some(cells) = r.initial_fill_fertile_cells {
        s.push_str(&format!(
            "  fill_fertile completed for {cells} fertile cells; next TerrainGroups::place_all\n"
        ));
    } else if let Some(error) = &r.initial_fertility_error {
        s.push_str(&format!("  fertility static input unavailable: {error}\n"));
    }
    if let Some(checksum) = r.initial_rules_checksum {
        s.push_str(&format!(
            "  replay Rules @ {:#x}: {} serialized bytes -> {} checksum-visible bytes, {:08x}\n",
            r.initial_rules_offset.unwrap_or(0),
            r.initial_rules_serialized_bytes,
            r.initial_rules_walked_bytes,
            checksum,
        ));
    }
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
        "  channel          survived  first-div    expected       got  compares  trivial  unmodelled  our-bytes  unsourced  retail-empty  retail-1st\n",
    );
    for (i, name) in CHANNEL_NAMES.iter().enumerate() {
        let c = &r.channels[i];
        let expected = c
            .first_divergence_turn
            .map(|_| format!("{:08x}", c.expected))
            .unwrap_or_else(|| "-".into());
        let got = c
            .first_divergence_turn
            .map(|_| format!("{:08x}", c.got))
            .unwrap_or_else(|| "-".into());
        s.push_str(&format!(
            "  {name:<16} {:7}  {:>9}  {:>10}  {:>9}  {:8}  {:7}  {:10}  {:9}  {:9}  {:12}  {:>10}\n",
            c.survived,
            c.first_divergence_turn
                .map(|t| t.to_string())
                .unwrap_or_else(|| "-".into()),
            expected,
            got,
            c.compares,
            c.trivial_matches,
            c.unmodelled_matches,
            c.our_bytes_walked,
            c.our_unsourced_walked,
            c.retail_empty_compares,
            c.retail_first_nonempty_turn
                .map(|t| t.to_string())
                .unwrap_or_else(|| "never".into()),
        ));
    }
    s.push_str(
        "  `unmodelled` = agreements on a channel don-sim has no producer for; those are not evidence.\n  \
         `retail-empty` = compares where the ENGINE also walked nothing; `retail-1st` = the turn it stopped.\n",
    );
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
