//! The state the checksum walks, and the mapping from `don-sim`'s world onto it.
//!
//! # What this is honest about
//!
//! `don-sim::World` is a placeholder: `pos/vel/hits/armor/attack/cooldown` over
//! anonymous rows, with no leaders, cities, goods, terrain or rule set. It
//! cannot yet produce a `Unit` record in the engine's layout. So this module
//! does **not** pretend to convert it. It offers a state container in the
//! engine's shape — per-channel lists of object *images* in PDB layout — plus
//! an explicit, named bridge for the part `don-sim` really does hold.
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
use crate::walk::{walk_class, WalkOutcome};
use crate::walk_gen::class_index;

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

/// The bridge from `don-sim` to checksum state, and the list of what it cannot
/// carry.
///
/// Kept as data rather than prose so the harness can print it and the report
/// cannot drift from the code.
pub struct SimBridge;

impl SimBridge {
    /// Columns `don_sim::World` holds today, and the engine field each would
    /// have to land in for the `units` channel to mean anything.
    pub const CARRIED: &'static [(&'static str, &'static str)] = &[
        (
            "pos_x",
            "Unit::orders_x.value @ +112 (placeholder units, not WCoord)",
        ),
        (
            "pos_y",
            "Unit::orders_y.value @ +116 (placeholder units, not WCoord)",
        ),
        (
            "owner",
            "Object::leader id (Object::walk_data @ 0x00647830)",
        ),
    ];

    /// What the engine's `Unit` record needs that `don-sim` has no column for.
    /// This is the units-channel worklist.
    pub const MISSING: &'static [&'static str] = &[
        "Unit::collide_frame / damage_frame / angle / dest_angle / trench_angle",
        "Unit::unit_masks / unit_masks2",
        "Unit::los_x / los_y",
        "Unit::group / order stack (Stack<PathData> at +0xb8)",
        "Object identity: id, type, leader, hits in engine layout",
        "Leader records (28,396 B each, essentially the whole player state)",
        "World terrain planes (0x00937040/0x009370a0/0x009370f0 fast paths)",
        "Constants (the rules channel, target 0x12ba3104)",
    ];

    /// Populate a `SimState` from a `don-sim` world.
    ///
    /// Deliberately a no-op that reports what it did: `don-sim::World` holds no
    /// engine-layout record, so converting it would mean *inventing* a layout,
    /// and an invented layout that hashes to a plausible number is precisely
    /// the failure this project is built to avoid. When `don-sim` grows a
    /// `Unit` in PDB layout this becomes a real conversion and the harness
    /// number moves on its own.
    pub fn populate(_world: &don_sim::World, _state: &mut SimState) -> usize {
        0
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
