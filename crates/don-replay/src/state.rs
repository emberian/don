//! The state the checksum walks, and the mapping from `don-sim`'s world onto it.
//!
//! # What this is honest about
//!
//! `don-sim::World` now holds PDB-generated `UnitCols` and the engine's own
//! `(who, o)` object registry, so a `Unit` record in the engine's layout *can*
//! be produced — and [`SimBridge::populate`] produces it, in the traversal order
//! `CheckSums::check_units` `0x009371d0` uses. Everything else the fifteen
//! channels walk — builds, walls, ammo, deaths, groups, guys, leaders, cities,
//! items, goods, terrain, rules, scenario, script — has no producer in `don-sim`
//! at all, so those channels stay empty and are labelled `ChannelSource::Absent`
//! rather than scored as agreement.
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
        &mut self.channels[c as usize]
    }

    /// Total objects across all channels — the one-number answer to "does our
    /// state model anything yet".
    pub fn object_count(&self) -> usize {
        self.channels.iter().map(|c| c.objects.len()).sum()
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
            if let Some(cls) = self.element_class[i] {
                for img in &self.channels[i].objects {
                    out.merge(walk_class(cls, img, &mut cs, 8));
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
    pub const PRODUCES: &'static [Channel] = &[Channel::Units];

    /// What the engine's checksum walks that `don-sim` has no producer for.
    /// This is the worklist, and it is the reason fourteen of fifteen channels
    /// still agree with retail only by walking nothing.
    pub const MISSING: &'static [&'static str] = &[
        "BuildData / WallData columns (builds, walls) — World has the bands, not the rows",
        "AmmoData flat list (ammo)",
        "DeathObjData ring, stride 0xa4 (deaths)",
        "Group records + the 32-byte global tail at 0x00e85f4c (groups)",
        "GuyData columns (guys)",
        "LeaderData records, 27,182 walked bytes each (leaders)",
        "City records (cities)",
        "Item / Good flat lists (items, goods)",
        "World terrain planes (world)",
        "Constants + 806 Types + 24 Tribes (rules, target 0x12ba3104)",
        "ScenarioData (scenario_data) — no derived walker either",
        "RunTimeEnv / BHS (script_run_time) — no derived walker either",
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
        rep
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn one_object_moves_exactly_one_channel() {
        let mut s = SimState::new();
        assert!(s.push_default_object(Channel::Units));
        let (ch, _) = s.check_all();
        assert_ne!(ch.get(Channel::Units), 1, "units moved");
        assert_eq!(ch.get(Channel::Walls), 1, "walls did not");
        assert!(ch.total_is_consistent(), "total tracks the change");
    }
}
