//! The state the checksum walks, and the mapping from `don-sim`'s world onto it.
//!
//! # What this is honest about
//!
//! `don-sim::World` now holds PDB-generated `UnitCols` and the engine's own
//! `(who, o)` object registry, so a `Unit` record in the engine's layout *can*
//! be produced — and [`SimBridge::populate`] produces it, in the traversal order
//! `CheckSums::check_units` `0x009371d0` uses. The `world` channel has a second,
//! exact producer: `don-sim`'s derived `World::walk_data` implementation, fed
//! from the authoritative `.rcx` initial setup. Everything else the fifteen
//! channels walk — builds, walls, ammo, deaths, groups, guys, leaders, cities,
//! goods and scenario — has no producer in `don-sim` at all. Items are projected from
//! the optional authoritative `World::item_runtime`; unavailable and initialized-empty
//! are distinct bridge states. Channel 15 has two producers: every `populate` installs
//! the exact traversal `RunTimeEnv::walk_data` `0x009c41a0` performs over an empty
//! `ScriptFile::script_files` array — four bytes of a zero count, not zero bytes — and
//! `populate_script_runtime` replaces it from `don-sim::script_runtime::ScriptRuntime`
//! when the compiled program carries a complete retail walk sidecar, failing closed
//! otherwise.
//! Static rules are the one replay-specific exception: an admitted recording
//! can install its complete checksum-visible SaveGame projection, while an
//! independently constructed `don-sim` world still has no rules producer.
//!
//! A channel with no objects walks nothing, and adler-32 keeps its initial
//! value of 1. That is not a fudge: `walls` reads exactly `0x00000001` for all
//! 11,321 turns of the 2025 multiplayer recording because that game had no
//! walls, and `deaths`/`ammo` start at 1 and leave it the moment the first
//! projectile or corpse exists. Matching a channel while both sides are empty
//! is a real comparison with a real failure mode — it stops the instant retail
//! has an object and we do not — and it is exactly the "we survive N turns on
//! channel X" number the harness reports.

use crate::checksum::{Channel, Channels, CheckSum, CHANNELS, NUM_WALKED};
use crate::image::{class_coverage, image_row};
use crate::walk::{walk_class, WalkOutcome};
use crate::walk_gen::class_index;
use don_sim::objects::Band;

/// The `walk_data` each channel's walker invokes per element, by PDB symbol.
/// Resolved from the channel walker VAs in `docs/derivation/checksum.md` §4 via
/// `tools/pdb/lookup.py`.
pub const CHANNEL_WALKER_SYMBOL: [&str; NUM_WALKED] = [
    "CheckSums::check_units",
    "CheckSums::check_builds",
    "CheckSums::check_walls",
    "CheckSums::check_ammo",
    "CheckSums::check_deaths",
    "CheckSums::check_groups",
    "CheckSums::check_guys",
    "LeaderData::walk_data (inlined 8x by check_all)",
    "CheckSums::check_cities",
    "CheckSums::check_items",
    "CheckSums::check_goods",
    "World::walk_data",
    "Game::walk_rules_data",
    "ScenarioData::walk_data",
    "RunTimeEnv::walk_data",
];

/// The engine class whose `walk_data` each channel's walker invokes per element,
/// as a key into the generated table from `schema/state-schema.json`.
///
/// `None` means **no derived walker exists yet** for that channel's element, so
/// the harness must refuse to produce a value the moment the channel is
/// non-empty. Two channels are `None` today: `ScenarioData::walk_data`
/// (`0x00997ad0`) is `static __cdecl`, so the extractor's `this`-taint never
/// fires on it, and `RunTimeEnv::walk_data` (`0x009c41a0`) is likewise absent
/// from the 278 classes the schema resolved.
///
/// `leaders` is the odd one out among the resolved ones: `check_all` inlines an
/// 8-iteration loop over `LeaderData::walk_data` (`0x006d6750`) at base
/// `0x00e3a390`, stride `0x6eec`, while the object channels iterate **9**
/// leader slots. Both numbers are measured; the 9th slot's own record is not
/// checksummed.
pub const CHANNEL_ELEMENT_CLASS: [Option<&str>; NUM_WALKED] = [
    Some("Unit"),         // units
    Some("BuildData"),    // builds
    Some("WallData"),     // walls
    Some("AmmoData"),     // ammo
    Some("DeathObjData"), // deaths
    Some("Group"),        // groups
    Some("GuyData"),      // guys
    Some("LeaderData"),   // leaders
    Some("City"),         // cities
    Some("Item"),         // items
    Some("Good"),         // goods
    Some("World"),        // world
    Some("Constants"),    // rules   (Game::walk_rules_data -> Constants)
    None,                 // scenario_data
    None,                 // script_run_time
];

/// Number of leader slots the object channels iterate.
pub const OBJECT_LEADER_SLOTS: usize = 9;
/// Number of leader records the `leaders` channel itself walks.
pub const CHECKSUMMED_LEADERS: usize = 8;

/// One channel's contents: object images in PDB layout, in traversal order.
#[derive(Debug, Clone, Default)]
pub struct ChannelState {
    /// Flat images, one per object. Empty means the walker is never called.
    pub objects: Vec<Vec<u8>>,
}

/// A channel already traversed by a shipped exact walker rather than by the
/// generic fixed-image table. `World::walk_data` follows dynamic arrays and
/// pointer-owned planes, so flattening its 372-byte owner image cannot execute
/// it; `don-sim::systems::map_terrain::World::walk` can and does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DirectChannel {
    checksum: u32,
    bytes_walked: u64,
    elements: u32,
}

impl ChannelState {
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }
    pub fn len(&self) -> usize {
        self.objects.len()
    }
}

/// The full sixteen-channel state, in the engine's shape.
#[derive(Debug, Clone, Default)]
pub struct SimState {
    pub channels: [ChannelState; NUM_WALKED],
    /// Class index into the generated walk table, resolved once.
    element_class: [Option<usize>; NUM_WALKED],
    resolved: bool,
    /// Per channel, walked bytes the bridge could not source from a column and
    /// therefore left zero. Written by [`SimBridge::populate`].
    unsourced_walked: [u64; NUM_WALKED],
    direct: [Option<DirectChannel>; NUM_WALKED],
    /// A producer can be installed while walking zero elements. This distinguishes a
    /// modelled empty registry from a missing subsystem.
    installed: [bool; NUM_WALKED],
}

impl SimState {
    pub fn new() -> SimState {
        let mut s = SimState::default();
        s.resolve();
        s
    }

    fn resolve(&mut self) {
        if self.resolved {
            return;
        }
        for i in 0..NUM_WALKED {
            self.element_class[i] = CHANNEL_ELEMENT_CLASS[i].and_then(class_index);
        }
        self.resolved = true;
    }

    pub fn channel_mut(&mut self, c: Channel) -> &mut ChannelState {
        let i = c as usize;
        self.direct[i] = None;
        self.unsourced_walked[i] = 0;
        self.installed[i] = true;
        &mut self.channels[i]
    }

    /// Total objects across all channels — the one-number answer to "does our
    /// state model anything yet".
    pub fn object_count(&self) -> usize {
        self.channels.iter().map(|c| c.objects.len()).sum::<usize>()
            + self
                .direct
                .iter()
                .filter_map(|d| d.map(|d| d.elements as usize))
                .sum::<usize>()
    }

    pub fn channel_element_count(&self, i: usize) -> u32 {
        self.channels.get(i).map_or(0, |c| c.len() as u32)
            + self
                .direct
                .get(i)
                .and_then(|d| *d)
                .map_or(0, |d| d.elements)
    }

    /// True when this channel has an authoritative producer, including an initialized
    /// producer whose current traversal is empty.
    pub fn channel_is_installed(&self, i: usize) -> bool {
        self.installed.get(i).copied().unwrap_or(false)
    }

    /// Installation state for the fifteen walked channels.
    pub fn installed_channels(&self) -> [bool; NUM_WALKED] {
        self.installed
    }

    fn set_direct_channel(
        &mut self,
        c: Channel,
        checksum: u32,
        bytes_walked: u64,
        unsourced_walked: u64,
    ) {
        self.set_direct_channel_elements(c, checksum, bytes_walked, unsourced_walked, 1);
    }

    fn set_direct_channel_elements(
        &mut self,
        c: Channel,
        checksum: u32,
        bytes_walked: u64,
        unsourced_walked: u64,
        elements: u32,
    ) {
        let i = c as usize;
        self.channels[i].objects.clear();
        self.direct[i] = Some(DirectChannel {
            checksum,
            bytes_walked,
            elements,
        });
        self.unsourced_walked[i] = unsourced_walked;
        self.installed[i] = true;
    }

    fn clear_channel(&mut self, c: Channel) {
        let i = c as usize;
        self.channels[i].objects.clear();
        self.direct[i] = None;
        self.unsourced_walked[i] = 0;
        self.installed[i] = false;
    }

    /// Walked bytes on channel `i` that the bridge left zero because no column
    /// materialises them. `0` on an empty channel means nothing; on a populated
    /// one it is the fidelity ceiling.
    pub fn unsourced_walked_bytes(&self, i: usize) -> u64 {
        self.unsourced_walked.get(i).copied().unwrap_or(0)
    }

    /// Compute the sixteen values `check_all` would produce over this state.
    ///
    /// Each channel gets a **fresh** `CheckSum` (`+0x10` reset to 1 before each
    /// channel, per `check_all`), the fifteen are summed with wrapping addition
    /// into `all`.
    pub fn check_all(&self) -> (Channels, [WalkOutcome; NUM_WALKED]) {
        let mut ch = Channels([0; 16]);
        let mut outcomes = [WalkOutcome::default(); NUM_WALKED];
        for i in 0..NUM_WALKED {
            let mut cs = CheckSum::new();
            let mut out = WalkOutcome::default();
            if let Some(direct) = self.direct[i] {
                cs.checksum = direct.checksum;
                cs.bytes = direct.bytes_walked;
                out.bytes_walked = direct.bytes_walked;
                out.ops_executed = 1;
            } else if let Some(cls) = self.element_class[i] {
                for img in &self.channels[i].objects {
                    out.merge(walk_class(cls, img, &mut cs, 8));
                }
                if self.installed[i] && self.channels[i].objects.is_empty() {
                    out.ops_executed = 1;
                }
            } else if !self.channels[i].objects.is_empty() {
                // We have objects but no derived walker for their class: refuse
                // to produce a number that looks like agreement.
                out.ops_unresolved += 1;
            }
            outcomes[i] = out;
            ch.set(CHANNELS[i], cs.checksum);
        }
        ch.set(Channel::All, ch.computed_total());
        (ch, outcomes)
    }

    /// Push an object image onto a channel. The image length is the PDB
    /// `sizeof` of the channel's element class, so a walk can never read past
    /// it.
    pub fn push_default_object(&mut self, c: Channel) -> bool {
        self.resolve();
        let i = c as usize;
        if i >= NUM_WALKED {
            return false;
        }
        match self.element_class[i].map(|k| crate::walk_gen::SPECS[k].sizeof as usize) {
            Some(n) if n > 0 => {
                self.direct[i] = None;
                self.unsourced_walked[i] = 0;
                self.installed[i] = true;
                self.channels[i].objects.push(vec![0u8; n]);
                true
            }
            _ => false,
        }
    }
}

/// Owner slots `CheckSums::check_units` `0x009371d0` and `check_guys`
/// `0x00937430` iterate: `&DAT_00e3a390 .. 0xe789dc` at stride `0x6eec` is
/// exactly **9** [measured].
///
/// `Objects::process_all` rotates over **ten**, so a unit parked in slot 9 is
/// processed by the tick and *not* seen by the units checksum. The bridge counts
/// those rather than quietly imaging them.
pub const UNITS_CHANNEL_OWNER_SLOTS: usize = 9;

/// Owner slots `check_builds` `0x00937290`, `check_walls` `0x00937360` and
/// `check_cities` `0x00937600` iterate: `.. 0xe71af0` is **8** [measured].
pub const BUILDS_CHANNEL_OWNER_SLOTS: usize = 8;

/// What one `populate` did, as counts rather than prose.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BridgeReport {
    /// Elements written, per channel.
    pub elements: [u32; NUM_WALKED],
    /// Walked bytes left zero because no column materialises them, per channel.
    pub unsourced_walked: [u64; NUM_WALKED],
    /// Units skipped because their owner slot is inactive — retail's
    /// `leader.flags & 1` gate.
    pub skipped_inactive_owner: u32,
    /// Units skipped because `SubObjectData::flags & 1` is clear.
    pub skipped_inactive_object: u32,
    /// Units in owner slots `>= 9`, which `check_units` never reaches.
    pub skipped_outside_walk: u32,
    /// Conditional channel-10 projection outcome.
    pub items: ItemProjection,
}

/// Whether the optional authoritative item registry was admitted into channel 10.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ItemProjection {
    /// The world has no initialized item registry; this is absent, not empty.
    #[default]
    Unavailable,
    /// A real initialized registry walked zero live elements.
    Empty,
    /// A real initialized registry walked at least one live element.
    Live,
    /// The registry was attached to different terrain dimensions and was refused.
    MapMismatch {
        item_xs: i32,
        item_ys: i32,
        map_xs: i32,
        map_ys: i32,
    },
}

impl BridgeReport {
    pub fn total_elements(&self) -> u32 {
        self.elements.iter().sum()
    }
}

/// The bridge from `don-sim` to checksum state.
///
/// Kept as data rather than prose so the harness can print it and the report
/// cannot drift from the code.
pub struct SimBridge;

impl SimBridge {
    /// Channels this bridge produces. Everything else is
    /// `ChannelSource::Absent`: not "we think it is empty", but "nothing in
    /// `don-sim` can say".
    pub const PRODUCES: &'static [Channel] = &[
        Channel::Units,
        Channel::Items,
        Channel::World,
        Channel::ScriptRunTime,
    ];

    /// What the engine's checksum walks that `don-sim` has no producer for.
    /// This is the worklist, and it is the reason eleven of fifteen channels
    /// still agree with retail only by walking nothing.
    pub const MISSING: &'static [&'static str] = &[
        "BuildData / WallData columns (builds, walls) — World has the bands, not the rows",
        "AmmoData flat list (ammo)",
        "DeathObjData ring, stride 0xa4 (deaths)",
        "Group records + the 32-byte global tail at 0x00e85f4c (groups)",
        "GuyData columns (guys)",
        "LeaderData records, 27,182 walked bytes each (leaders)",
        "City records (cities)",
        "Good flat list (goods)",
        "Constants + 806 Types + 24 Tribes (rules, target 0x12ba3104)",
        "ScenarioData mutation (scenario_data) — the ScenarioFuncSet::init 0x00a03c30 initial state is produced, but nothing writes units_killed/builds_destroyed/city_lost_to, so the channel is frozen at Game::init and expires at the recording's first kill; see docs/assembly/scenario-initial-state.md",
        "ScriptFile records (script_run_time) — only the empty ScriptFile::script_files count is produced; shipped programs still need their retail container/value walk metadata",
    ];

    /// Populate a `SimState` from a `don-sim` world.
    ///
    /// Only the channels in [`SimBridge::PRODUCES`] are touched; the rest are
    /// left exactly as they were, so a caller that has staged state of its own
    /// (a test, or a future lane's producer) does not have it silently erased.
    ///
    /// The `units` traversal is `check_units` `0x009371d0`, read for this lane:
    ///
    /// ```text
    /// for who in 0..9:                       ; 9 leader slots, NOT rotated
    ///     if leader[who].flags & 1:
    ///         for o in unit_band[who]:       ; band base 0, dense, in `o` order
    ///             if obj.flags & 1:
    ///                 obj->vt[0x7c](checksum)   ; Unit::walk_data
    /// ```
    ///
    /// Two details that are easy to get wrong and are not: the leader loop is
    /// **unrotated** (the `(frame + i) % 10` rotation belongs to
    /// `Objects::process_all`, and applying it here would change the hash every
    /// frame for the same state), and the object gate is `flags & 1` on
    /// `SubObjectData::flags` at `+8`, the same bit the tick uses.
    pub fn populate(world: &don_sim::World, state: &mut SimState) -> BridgeReport {
        state.resolve();
        let mut rep = BridgeReport::default();
        let ui = Channel::Units as usize;

        // Per-element cost of everything the checksum visits that no column
        // materialises. Static: it depends on the class, not the row.
        let per_unit_unsourced =
            class_coverage::<don_sim::generated::state::UnitCols>().unsourced_walked as u64;

        state.installed[ui] = true;
        let ch = &mut state.channels[ui];
        ch.objects.clear();
        for who in 0..don_sim::objects::OWNER_SLOTS {
            let band = world.objects.slot(who).band(Band::Unit);
            if who >= UNITS_CHANNEL_OWNER_SLOTS {
                rep.skipped_outside_walk += band.len() as u32;
                continue;
            }
            if !world.objects.is_active(who) {
                rep.skipped_inactive_owner += band.len() as u32;
                continue;
            }
            for &row in band {
                let row = row as usize;
                if world.units.get_flags(row) & don_sim::world::OBJ_FLAG_ACTIVE == 0 {
                    rep.skipped_inactive_object += 1;
                    continue;
                }
                ch.objects.push(image_row(&world.units, row));
            }
        }
        rep.elements[ui] = ch.objects.len() as u32;
        rep.unsourced_walked[ui] = rep.elements[ui] as u64 * per_unit_unsourced;
        state.unsourced_walked[ui] = rep.unsourced_walked[ui];
        Self::populate_items(world, state, &mut rep);
        Self::populate_empty_script_runtime(state, &mut rep);
        rep
    }

    /// Install channel 15 for a world holding **no** BHS program.
    ///
    /// `RunTimeEnv::walk_data` `0x009c41a0` walks the signed 32-bit element count of the
    /// global `ScriptFile::script_files` (`0x00c8cba0`) unconditionally, then one
    /// `ScriptFile::walk_data` per entry. A `don_sim::World` on its own owns no
    /// `ScriptFile` registry, so its count is zero and the traversal is complete at four
    /// bytes — sourced, not padded.
    ///
    /// This is a real and frequently wrong claim: any recording whose engine had loaded
    /// script files disagrees immediately. [`SimBridge::populate_script_runtime`]
    /// replaces it whenever an authoritative `ScriptRuntime` exists, and must therefore
    /// be called after `populate`.
    fn populate_empty_script_runtime(state: &mut SimState, rep: &mut BridgeReport) {
        let empty = crate::script_channel::checksum_empty_runtime();
        state.set_direct_channel_elements(
            Channel::ScriptRunTime,
            empty.checksum,
            empty.bytes_walked,
            0,
            0,
        );
        rep.elements[Channel::ScriptRunTime as usize] = 0;
    }

    fn populate_items(world: &don_sim::World, state: &mut SimState, rep: &mut BridgeReport) {
        let ii = Channel::Items as usize;
        match world.items_channel() {
            Ok(items) => {
                state.set_direct_channel_elements(
                    Channel::Items,
                    items.checksum,
                    u64::from(items.bytes_walked),
                    0,
                    items.elements,
                );
                rep.elements[ii] = items.elements;
                rep.items = if items.elements == 0 {
                    ItemProjection::Empty
                } else {
                    ItemProjection::Live
                };
            }
            Err(don_sim::item_runtime::ItemRuntimeError::Unavailable) => {
                state.clear_channel(Channel::Items);
                rep.items = ItemProjection::Unavailable;
            }
            Err(_) => unreachable!("items_channel has no map-dependent failure mode"),
        }
    }

    fn validate_item_map_shape(
        world: &don_sim::World,
        map_shape: (i32, i32),
        state: &mut SimState,
        rep: &mut BridgeReport,
    ) {
        let Some(runtime) = world.item_runtime.as_ref() else {
            return;
        };
        let item_shape = runtime.map_shape();
        if item_shape == map_shape {
            return;
        }
        state.clear_channel(Channel::Items);
        rep.elements[Channel::Items as usize] = 0;
        rep.items = ItemProjection::MapMismatch {
            item_xs: item_shape.0,
            item_ys: item_shape.1,
            map_xs: map_shape.0,
            map_ys: map_shape.1,
        };
    }

    /// Populate both the object-backed channels and the dynamic `world`
    /// channel. The latter uses `World::checksum_sections`, which is the same
    /// exact traversal as `World::walk_data(-1)` and reports its byte count.
    pub fn populate_with_map(
        world: &don_sim::World,
        map: &don_sim::systems::map_terrain::World,
        map_unsourced_walked: u64,
        state: &mut SimState,
    ) -> BridgeReport {
        let checksum = map.checksum_sections();
        Self::populate_with_map_checksum(
            world,
            &checksum,
            (map.xs, map.ys),
            map_unsourced_walked,
            state,
        )
    }

    /// As [`SimBridge::populate_with_map`], using a cached walk result. Initial
    /// replay terrain is immutable until the world-generation port can produce
    /// it, so re-walking hundreds of thousands of bytes on every command turn
    /// would add cost without observing any state change.
    pub fn populate_with_map_checksum(
        world: &don_sim::World,
        checksum: &don_sim::systems::map_terrain::WorldChecksum,
        map_shape: (i32, i32),
        map_unsourced_walked: u64,
        state: &mut SimState,
    ) -> BridgeReport {
        let mut rep = Self::populate(world, state);
        Self::validate_item_map_shape(world, map_shape, state, &mut rep);
        let wi = Channel::World as usize;
        state.set_direct_channel(
            Channel::World,
            checksum.full,
            checksum.bytes,
            map_unsourced_walked.min(checksum.bytes),
        );
        rep.elements[wi] = 1;
        rep.unsourced_walked[wi] = map_unsourced_walked.min(checksum.bytes);
        rep
    }

    /// Install the exact static `rules` projection carried by a replay.
    ///
    /// This is intentionally separate from [`SimBridge::PRODUCES`]: the bytes
    /// come from the replay's SaveGame section, not from a complete don-sim
    /// rules owner. [`crate::initial::parse_serialized_rules_at`] admits the
    /// value only after independently replaying the shipped traversal and
    /// matching every cumulative retail checkpoint, so there are no unsourced
    /// bytes and no recorded wire checksum is copied into state.
    pub fn populate_replay_rules(rules: &crate::initial::InitialRules, state: &mut SimState) {
        state.set_direct_channel(Channel::Rules, rules.checksum, rules.walked_bytes, 0);
    }

    /// Install channel 14 with the state a retail `Game::init` leaves in `ScenarioData`.
    ///
    /// Like [`SimBridge::populate_replay_rules`] this is outside [`SimBridge::PRODUCES`],
    /// because its last two inputs come from shipped `internal_strings.xml` rather than
    /// from a `don_sim::World`. Every byte is sourced: the image is
    /// `ScenarioFuncSet::init` `0x00a03c30` read field by field, plus the two ordinals
    /// that initializer indexes, walked through `ScenarioData::walk_data` `0x00997ad0`.
    /// The recorded wire value is never copied in.
    ///
    /// **This is a frozen producer, and it is wrong on purpose after a point.**
    /// `ScenarioData` is mutable: `Object::take_damage` `0x00652020`,
    /// `Object::disband` `0x006455c0` and `City::capture` `0x00736c40` write
    /// `units_killed` / `builds_destroyed` / `city_lost_to`, and nothing in `don-sim`
    /// drives them. So the claim this makes is "no scenario counter has moved yet", which
    /// is true from `Game::init` and false from the recording's first kill onward. The
    /// turn it stops matching is the measurement.
    pub fn populate_scenario_initial(
        scenario: &crate::scenario_channel::InitialScenarioChannel,
        state: &mut SimState,
    ) {
        state.set_direct_channel(
            Channel::ScenarioData,
            scenario.checksum,
            scenario.bytes_walked,
            0,
        );
    }

    /// Install channel 15 from the authoritative BHS runtime's persistent program state.
    ///
    /// `RunTimeEnv::close` removes transient interpreter frames before walking; DON's VM
    /// likewise constructs each frame locally, so [`ScriptRuntime::program`] is exactly
    /// the reachable persistent boundary: compiled files, statics, and trigger bits.
    /// Container headers and `ScriptType` ownership fields erased by Rust are required
    /// through `Program::walk_meta`; missing or stale metadata returns an error and leaves
    /// the existing channel untouched.
    pub fn populate_script_runtime(
        runtime: &don_sim::script_runtime::ScriptRuntime,
        state: &mut SimState,
    ) -> Result<
        crate::script_channel::ScriptChannelChecksum,
        crate::script_channel::ScriptChannelError,
    > {
        let checksum = crate::script_channel::checksum_program(runtime.program())?;
        state.set_direct_channel(
            Channel::ScriptRunTime,
            checksum.checksum,
            checksum.bytes_walked,
            0,
        );
        Ok(checksum)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    use don_bhs::host::NullHost;
    use don_bhs::program::{Program, ScriptFile, ValueWalkMeta};
    use don_bhs::value::Value;
    use don_bhs::vm::Vm;
    use don_bhs_cc::sema::{self, Severity};

    /// Exactly the two channels documented as unresolved may be unresolved. If
    /// a third goes missing, the schema regressed; if one is recovered, this
    /// test says so instead of letting the gap quietly persist.
    #[test]
    fn channel_walker_resolution_is_thirteen_of_fifteen() {
        let s = SimState::new();
        let missing: Vec<&str> = (0..NUM_WALKED)
            .filter(|&i| s.element_class[i].is_none())
            .map(|i| crate::checksum::CHANNEL_NAMES[i])
            .collect();
        assert_eq!(
            missing,
            vec!["scenario_data", "script_run_time"],
            "{missing:?}"
        );
    }

    #[test]
    fn empty_state_reproduces_the_all_ones_tuple() {
        let s = SimState::new();
        let (ch, outs) = s.check_all();
        assert_eq!(ch, Channels::empty_state());
        assert!(ch.total_is_consistent());
        assert!(outs.iter().all(|o| o.bytes_walked == 0));
    }

    #[test]
    fn authoritative_script_runtime_installs_a_non_vacuous_channel_fifteen() {
        let source =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../don-bhs/oracle/fixtures/static_int.bhs");
        let includes = sema::IncludePath::with_roots([source.parent().unwrap().to_path_buf()]);
        let unit = sema::analyze(&source, &includes).unwrap();
        let (mut program, diags, _) = don_bhs_cc::codegen::compile(&unit);
        let errors: Vec<String> = unit
            .diags
            .iter()
            .chain(&diags)
            .filter(|diag| diag.severity == Severity::Error)
            .map(ToString::to_string)
            .collect();
        assert!(errors.is_empty(), "{}", errors.join("\n"));

        // Exercise the normal compiler-produced metadata through the measured
        // OP_INIT_COPY static growth path before handing ownership to ScriptRuntime.
        let mut host = NullHost;
        let outcome = Vm::new(&mut program, &mut host)
            .run_script(0, "static_int")
            .unwrap();
        assert_eq!(outcome.returned, Some(Value::Int(0)));
        assert_eq!(
            program.walk_meta().unwrap().files[0].script_meta[0].statics,
            [Some(ValueWalkMeta::scalar(3, 0))]
        );

        let runtime = don_sim::script_runtime::ScriptRuntime::new(program, None, None).unwrap();
        let mut state = SimState::new();
        let script = SimBridge::populate_script_runtime(&runtime, &mut state).unwrap();
        let report = crate::check_all::CheckAll::of_state(&state);

        assert!(script.bytes_walked > 4, "a named Script body was walked");
        assert_eq!(
            state.channel_element_count(Channel::ScriptRunTime as usize),
            1
        );
        assert_eq!(report.returns(), script.checksum);
        assert_eq!(
            report.per[Channel::ScriptRunTime as usize].bytes,
            script.bytes_walked
        );
        assert!(report.per[Channel::ScriptRunTime as usize].complete());
    }

    #[test]
    fn missing_script_walk_metadata_leaves_channel_fifteen_uninstalled() {
        let runtime = don_sim::script_runtime::ScriptRuntime::new(
            Program::single(ScriptFile::default()),
            None,
            None,
        )
        .unwrap();
        let mut state = SimState::new();
        assert_eq!(
            SimBridge::populate_script_runtime(&runtime, &mut state),
            Err(crate::script_channel::ScriptChannelError::MissingProgramWalkMetadata)
        );
        let report = crate::check_all::CheckAll::of_state(&state);
        assert_eq!(report.returns(), 1);
        assert_eq!(report.per[Channel::ScriptRunTime as usize].bytes, 0);
        assert_eq!(
            state.channel_element_count(Channel::ScriptRunTime as usize),
            0
        );
    }

    #[test]
    fn one_object_moves_exactly_one_channel() {
        let mut s = SimState::new();
        assert!(s.push_default_object(Channel::Units));
        let (ch, _) = s.check_all();
        assert_ne!(ch.get(Channel::Units), 1, "units moved");
        assert_eq!(ch.get(Channel::Walls), 1, "walls did not");
        assert!(ch.total_is_consistent(), "total tracks the change");
    }
}
