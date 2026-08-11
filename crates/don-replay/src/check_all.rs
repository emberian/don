//! `CheckSums::check_all` `0x00936560` — the whole-world checksum, composed.
//!
//! Fifteen channels ran in this crate before this module existed, but nothing
//! *composed* them: there was no function you could hand a world and get the
//! tuple a retail client would have put on the wire. That is what this is.
//!
//! # What retail actually does, and the correction it forces
//!
//! `check_all` builds one `CheckSum` visitor on its stack and calls the fifteen
//! channel walkers in address order. Traced over the whole 1,614-byte body
//! [measured, `docs/mechanics/COVERAGE.md` §1.1]:
//!
//! * the accumulator is the visitor's `+0x10` field, and it is
//!   `mov dword ptr [ebp-0x18], 1` **before every channel** — sixteen resets,
//!   one pre-loop init plus fifteen channels. So each channel is an independent
//!   adler-32 starting at 1, not one rolling hash;
//! * `edi` accumulates the running sum (`mov edi,[ebp-0x18]` after channel 1 at
//!   `0x00936658`, then `add edi,[ebp-0x18]` fourteen times, last at
//!   `0x00936B49`);
//! * but the epilogue stores `edi` only into `[ebp-0x10]` to feed
//!   `SyncLogger::logToMemory`, and returns `[ebp-0x18]`:
//!
//! ```text
//! 00936b53  mov  dword ptr [ebp - 0x10], edi   ; sum -> sync log only
//! 00936b8c  mov  eax, dword ptr [ebp - 0x18]   ; RETURN = channel 15
//! 00936bad  ret
//! ```
//!
//! **`check_all`'s return value is the fifteenth channel** (`RunTimeEnv`), not
//! the sum. The brief that seeded this lane, and `README-LLM.md`, both say
//! "returns the SUM"; they are wrong in the same way, and the sum is real but
//! comes from somewhere else. [`CheckAll::returns`] reproduces the return value
//! and [`CheckAll::total`] the sum, so neither can be quietly substituted for
//! the other.
//!
//! The sum reaches the wire from `CommandManager::issue_check_sums`
//! `0x00940770`, which runs its own `CheckSum` over all fifteen channels and
//! packs a 65-byte record: opcode `0x39`, fifteen `u32` at `+1, +5, … +57`, and
//! the total at `+61` (`add eax, esi` at `0x009409F0`). `1 + 16*4 = 65`, exactly
//! the recorded wire size, and `CommandPackage::process_check_sums` `0x009459D0`
//! reads it back at `[edi+1]`, `[edi+5]`, … . [`CheckSumsRecord`] is that
//! layout.
//!
//! # What a channel value means when it is 1
//!
//! Adler-32 of nothing is 1, so a channel reads 1 both when the game genuinely
//! has no walls and when our simulation has no *concept* of walls. Those are
//! completely different claims and the scoreboard used to score them the same.
//! [`ChannelSource`] separates them, per channel, as data.

use crate::checksum::{Channel, Channels, CHANNEL_NAMES, NUM_CHANNELS, NUM_WALKED};
use crate::state::{SimBridge, SimState};
use crate::walk::WalkOutcome;

/// `CheckSums::check_all`.
pub const CHECK_ALL_VA: u32 = 0x0093_6560;
/// `CommandManager::issue_check_sums` — builds the 65-byte wire record.
pub const ISSUE_CHECK_SUMS_VA: u32 = 0x0094_0770;
/// `CommandPackage::process_check_sums` — reads it back on the far side.
pub const PROCESS_CHECK_SUMS_VA: u32 = 0x0094_59d0;
/// `CheckSumsCommand`.
pub const CHECK_SUMS_OPCODE: u8 = 0x39;
/// `1 + 16 * 4`.
pub const CHECK_SUMS_WIRE_LEN: usize = 65;

/// How a channel walker reaches its elements. Taken from the walkers themselves
/// (`re/decomp-all/<va>.c`, re-read for this lane), because the traversal
/// *order* is part of the checksum: adler-32 is order-sensitive, so a channel
/// with the right elements in the wrong order is a wrong channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Traversal {
    /// `for who in 0..slots { if leader.flags & 1 { for o in band { if
    /// obj.flags & mask { walk } } } }`. The leader loop is **not** rotated —
    /// the `(frame + i) % 10` rotation is `Objects::process_all`'s, and applying
    /// it here would reorder the hash every frame.
    PerOwnerBand { slots: u8, flag_mask: u8 },
    /// One flat array on the `Game` singleton: `for i in 0..count { if gate {
    /// walk } }`.
    FlatList { flag_mask: u8 },
    /// Walked inline by `check_all` itself rather than by a `CheckSums::*`
    /// helper.
    Inline,
}

/// One channel's traversal, with the address it was read from.
#[derive(Debug, Clone, Copy)]
pub struct ChannelWalker {
    pub name: &'static str,
    pub va: u32,
    pub traversal: Traversal,
    /// The PDB class whose `walk_data` runs per element, or `None` where the
    /// walker inlines the walk.
    pub element: Option<&'static str>,
}

/// The fifteen walkers, in `check_all` call order.
///
/// The two owner-loop bounds are [measured] from the loop guards: `check_units` and
/// `check_guys` run `&DAT_00e3a390 .. 0xe789dc`, while `check_builds`/`check_walls`/
/// `check_cities` run `.. 0xe71af0`, stride `0x6eec`, which is
/// **9** leader slots for `units`/`guys` and **8** for the rest. That the two
/// differ is not a transcription slip; it is in the binary.
pub const WALKERS: [ChannelWalker; NUM_WALKED] = [
    ChannelWalker {
        name: "units",
        va: 0x0093_71d0,
        traversal: Traversal::PerOwnerBand {
            slots: 9,
            flag_mask: 1,
        },
        element: Some("Unit"),
    },
    ChannelWalker {
        name: "builds",
        va: 0x0093_7290,
        traversal: Traversal::PerOwnerBand {
            slots: 8,
            flag_mask: 1,
        },
        element: Some("BuildData"),
    },
    ChannelWalker {
        name: "walls",
        va: 0x0093_7360,
        traversal: Traversal::PerOwnerBand {
            slots: 8,
            flag_mask: 1,
        },
        element: Some("WallData"),
    },
    ChannelWalker {
        // `if ((*(byte *)(puVar1 + 1) & 3) != 0)` — ammo gates on flags & 3,
        // not & 1 like the object channels.
        name: "ammo",
        va: 0x0093_74e0,
        traversal: Traversal::FlatList { flag_mask: 3 },
        element: Some("AmmoData"),
    },
    ChannelWalker {
        // Stride 0xa4 over `Game+0x14c`, count `Game+0x140`; walks [0,4) then,
        // if that dword is non-zero, [4,0x4b). No vtable dispatch.
        name: "deaths",
        va: 0x0093_6bb0,
        traversal: Traversal::FlatList { flag_mask: 0 },
        element: Some("DeathObjData"),
    },
    ChannelWalker {
        name: "groups",
        va: 0x0093_7530,
        traversal: Traversal::FlatList { flag_mask: 0 },
        element: Some("Group"),
    },
    ChannelWalker {
        name: "guys",
        va: 0x0093_7430,
        traversal: Traversal::PerOwnerBand {
            slots: 9,
            flag_mask: 1,
        },
        element: Some("GuyData"),
    },
    ChannelWalker {
        name: "leaders",
        va: 0x0093_75a0,
        traversal: Traversal::Inline,
        element: Some("LeaderData"),
    },
    ChannelWalker {
        name: "cities",
        va: 0x0093_7600,
        traversal: Traversal::PerOwnerBand {
            slots: 8,
            flag_mask: 1,
        },
        element: Some("City"),
    },
    ChannelWalker {
        name: "items",
        va: 0x0093_7790,
        traversal: Traversal::FlatList { flag_mask: 1 },
        element: Some("Item"),
    },
    ChannelWalker {
        name: "goods",
        va: 0x0093_7710,
        traversal: Traversal::FlatList { flag_mask: 1 },
        element: Some("Good"),
    },
    ChannelWalker {
        name: "world",
        va: 0x006b_5cf0,
        traversal: Traversal::Inline,
        element: Some("World"),
    },
    ChannelWalker {
        name: "rules",
        va: 0x0058_9550,
        traversal: Traversal::Inline,
        element: Some("Constants"),
    },
    ChannelWalker {
        name: "scenario_data",
        va: 0x0099_7ad0,
        traversal: Traversal::Inline,
        element: None,
    },
    ChannelWalker {
        name: "script_run_time",
        va: 0x009c_41a0,
        traversal: Traversal::Inline,
        element: None,
    },
];

/// Whether our simulation has any producer for a channel's elements.
///
/// The distinction the scoreboard needs: agreeing with retail on an *absent*
/// channel is not evidence about our mechanics, it is evidence that we have not
/// written any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelSource {
    /// `don-sim` holds this channel's elements and the bridge images them, so
    /// "empty" is a claim that can be wrong.
    Modelled,
    /// A producer exists, but a particular world may not have initialized it. The
    /// per-state `installed` bit decides whether an empty agreement is substantive.
    Conditional,
    /// Nothing produces this channel's elements yet. It reads 1 because there is
    /// nothing to walk, and any agreement is vacuous.
    Absent,
}

/// Per channel, whether the `don-sim` → engine-layout bridge can produce it.
/// Kept beside the bridge it describes: `populate` supplies units and conditional
/// items, while `populate_with_map` also supplies world; the test at the bottom
/// exercises all three and fails if the capability table disagrees.
pub const CHANNEL_SOURCE: [ChannelSource; NUM_WALKED] = [
    ChannelSource::Modelled,    // units   — World::units + the unit band
    ChannelSource::Absent,      // builds  — no BuildData columns in World
    ChannelSource::Absent,      // walls
    ChannelSource::Absent,      // ammo
    ChannelSource::Absent,      // deaths
    ChannelSource::Absent,      // groups
    ChannelSource::Absent,      // guys
    ChannelSource::Absent,      // leaders
    ChannelSource::Absent,      // cities
    ChannelSource::Conditional, // items — optional World::item_runtime
    ChannelSource::Absent,      // goods
    ChannelSource::Modelled,    // world   — exact dynamic World::walk_data bridge
    ChannelSource::Absent,      // rules
    ChannelSource::Absent,      // scenario_data
    // script_run_time — the bridge produces the empty `ScriptFile::script_files` count
    // that `RunTimeEnv::walk_data` 0x009c41a0 always hashes, so "no scripts loaded" is a
    // claim that can be (and usually is) wrong rather than a vacuous 1.
    ChannelSource::Modelled,
];

/// One channel's result, with everything needed to judge it.
#[derive(Debug, Clone, Copy, Default)]
pub struct ChannelReport {
    pub value: u32,
    /// Elements the traversal visited.
    pub elements: u32,
    /// Bytes handed to the visitor.
    pub bytes: u64,
    /// Bytes the elements' `walk_data` visits that no column materialises. The
    /// ceiling on this channel's fidelity: every one is a byte retail hashes and
    /// we hash a zero for.
    pub unsourced_walked_bytes: u64,
    /// A producer was installed even if it currently walked zero bytes. Exact ownership
    /// is a separate gate below.
    pub installed: bool,
    /// The producer owns the retail traversal exactly. Generic object-image walkers are
    /// coverage probes and leave this false even when they happen to execute every op in
    /// an empty state.
    pub exact_producer: bool,
    pub outcome: WalkOutcome,
}

impl ChannelReport {
    /// True when the walk executed every op of every element. An incomplete walk
    /// can still produce a number; it just cannot be believed.
    pub fn complete(&self) -> bool {
        self.outcome.is_complete()
    }

    /// Whether an equality on this channel is substantive compatibility evidence.
    /// Every gate is load-bearing: installation admits the owner, non-empty bytes rule
    /// out adler-of-nothing, zero unsourced bytes rules out placeholders, a complete walk
    /// rules out skipped operations, and `exact_producer` excludes the generic bridge.
    pub fn substantive(&self) -> bool {
        self.installed
            && self.bytes > 0
            && self.unsourced_walked_bytes == 0
            && self.complete()
            && self.exact_producer
    }
}

/// The result of one `check_all`: the sixteen wire words plus the evidence.
#[derive(Debug, Clone)]
pub struct CheckAll {
    pub channels: Channels,
    pub per: [ChannelReport; NUM_WALKED],
}

impl CheckAll {
    /// Compute the fifteen channels over a state already in engine layout.
    ///
    /// Each channel gets a **fresh** visitor (`+0x10` reset to 1 before every
    /// channel), and `all` is the wrapping sum of the fifteen.
    pub fn of_state(state: &SimState) -> CheckAll {
        let (channels, outcomes) = state.check_all();
        let mut per = [ChannelReport::default(); NUM_WALKED];
        for i in 0..NUM_WALKED {
            per[i] = ChannelReport {
                value: channels.0[i],
                elements: state.channel_element_count(i),
                bytes: outcomes[i].bytes_walked,
                unsourced_walked_bytes: state.unsourced_walked_bytes(i),
                installed: state.channel_is_installed(i),
                exact_producer: state.channel_has_exact_producer(i),
                outcome: outcomes[i],
            };
        }
        CheckAll { channels, per }
    }

    /// Compute the fifteen channels over a `don-sim` world: bridge, then walk.
    /// This is the function the whole lane exists to provide.
    pub fn of_world(world: &don_sim::World) -> CheckAll {
        let mut st = SimState::new();
        SimBridge::populate(world, &mut st);
        CheckAll::of_state(&st)
    }

    /// The value retail's `check_all` **returns**: the fifteenth channel.
    pub fn returns(&self) -> u32 {
        self.channels.get(Channel::ScriptRunTime)
    }

    /// The value retail *logs* and puts on the wire as the sixteenth dword: the
    /// wrapping sum of the fifteen.
    pub fn total(&self) -> u32 {
        self.channels.computed_total()
    }

    /// Total bytes this world handed to the visitor across all fifteen channels.
    /// Zero means every channel agreed with retail only by walking nothing.
    pub fn bytes_walked(&self) -> u64 {
        self.per.iter().map(|c| c.bytes).sum()
    }

    /// Channels that walked at least one byte — the ones whose agreement or
    /// disagreement is about our mechanics rather than about our absence.
    pub fn non_trivial(&self) -> Vec<&'static str> {
        (0..NUM_WALKED)
            .filter(|&i| self.per[i].bytes > 0)
            .map(|i| CHANNEL_NAMES[i])
            .collect()
    }

    /// The 65-byte `CheckSumsCommand` a retail client would send.
    pub fn record(&self) -> CheckSumsRecord {
        CheckSumsRecord::from_channels(self.channels)
    }

    /// A compact per-channel table.
    pub fn format(&self) -> String {
        let mut s = String::new();
        s.push_str(
            "  channel            value     elements      bytes  unsourced  source     walk       exact  substantive\n",
        );
        for i in 0..NUM_WALKED {
            let c = &self.per[i];
            s.push_str(&format!(
                "  {:<16} {:08x} {:12} {:10} {:10}  {:<9} {:<10} {:<5}  {}\n",
                CHANNEL_NAMES[i],
                c.value,
                c.elements,
                c.bytes,
                c.unsourced_walked_bytes,
                match (CHANNEL_SOURCE[i], c.installed) {
                    (ChannelSource::Modelled, true) => "modelled",
                    (ChannelSource::Modelled, false) => "MISSING",
                    (ChannelSource::Conditional, true) => "modelled",
                    (ChannelSource::Conditional, false) => "MISSING",
                    // `rules` and `scenario_data` have no `don-sim` producer, but a
                    // replay/shipped-data producer can be installed on the state. Printing
                    // ABSENT for those was wrong once the bytes were real.
                    (ChannelSource::Absent, true) => "installed",
                    (ChannelSource::Absent, false) => "ABSENT",
                },
                if c.complete() {
                    "complete".to_string()
                } else {
                    format!("{} ops missed", c.outcome.ops_missed())
                },
                if c.exact_producer { "yes" } else { "no" },
                if c.substantive() { "yes" } else { "no" },
            ));
        }
        s.push_str(&format!(
            "  {:<16} {:08x}  (wrapping sum)      returns {:08x}  (channel 15)\n",
            "all",
            self.total(),
            self.returns()
        ));
        s
    }
}

/// `check_all` over a `don-sim` world.
pub fn check_all(world: &don_sim::World) -> CheckAll {
    CheckAll::of_world(world)
}

/// The 65-byte `CheckSumsCommand` record, exactly as
/// `CommandManager::issue_check_sums` `0x00940770` packs it.
///
/// | offset | field |
/// |---|---|
/// | `+0` | `u8` opcode `0x39` |
/// | `+1, +5, … +57` | `u32` × 15, one per channel, in `check_all` order |
/// | `+61` | `u32` total — the wrapping sum |
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckSumsRecord {
    pub channels: Channels,
}

impl CheckSumsRecord {
    pub fn from_channels(mut channels: Channels) -> CheckSumsRecord {
        let t = channels.computed_total();
        channels.set(Channel::All, t);
        CheckSumsRecord { channels }
    }

    pub fn encode(&self) -> [u8; CHECK_SUMS_WIRE_LEN] {
        let mut b = [0u8; CHECK_SUMS_WIRE_LEN];
        b[0] = CHECK_SUMS_OPCODE;
        for i in 0..NUM_CHANNELS {
            let o = 1 + 4 * i;
            b[o..o + 4].copy_from_slice(&self.channels.0[i].to_le_bytes());
        }
        b
    }

    /// Decode a wire record. Rejects the wrong opcode or length; does **not**
    /// reject an inconsistent total, because a desynced client really does send
    /// one and swallowing it would hide the thing we are looking for.
    pub fn decode(bytes: &[u8]) -> Option<CheckSumsRecord> {
        if bytes.len() != CHECK_SUMS_WIRE_LEN || bytes[0] != CHECK_SUMS_OPCODE {
            return None;
        }
        let mut v = [0u32; NUM_CHANNELS];
        for (i, s) in v.iter_mut().enumerate() {
            let o = 1 + 4 * i;
            *s = u32::from_le_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]);
        }
        Some(CheckSumsRecord {
            channels: Channels(v),
        })
    }

    /// The engine's own internal relation, and the reason a `CheckSumsCommand`
    /// is findable in an unframed byte stream.
    pub fn total_is_consistent(&self) -> bool {
        self.channels.total_is_consistent()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checksum::CHANNELS;

    /// The wire record is 65 bytes at the derived offsets and survives a round
    /// trip through both this decoder and `don-net`'s independent one.
    #[test]
    fn the_wire_record_round_trips_through_two_decoders() {
        let mut ch = Channels([0; NUM_CHANNELS]);
        for (i, c) in CHANNELS[..NUM_WALKED].iter().enumerate() {
            ch.set(*c, 0x1000_0001u32.wrapping_mul(i as u32 + 1));
        }
        let rec = CheckSumsRecord::from_channels(ch);
        let b = rec.encode();
        assert_eq!(b.len(), 65);
        assert_eq!(b[0], 0x39);
        assert!(rec.total_is_consistent());

        let back = CheckSumsRecord::decode(&b).expect("decodes");
        assert_eq!(back, rec);

        // don-net's decoder reads the same bytes into the same words.
        let cmd = don_net::Command {
            opcode: 0x39,
            bytes: &b,
        };
        let ns = don_net::CheckSums::decode(&cmd).expect("don-net decodes");
        assert_eq!(ns.0, rec.channels.0, "two decoders, one layout");
    }

    #[test]
    fn a_short_or_wrong_opcode_record_is_refused() {
        let b = [0u8; 65];
        assert!(
            CheckSumsRecord::decode(&b).is_none(),
            "opcode 0 is not 0x39"
        );
        let mut b = [0u8; 64];
        b[0] = 0x39;
        assert!(CheckSumsRecord::decode(&b).is_none(), "64 bytes is not 65");
    }

    /// The §1.1 correction, as an executable statement: the returned value is
    /// channel 15, and it is *not* the sum.
    #[test]
    fn check_all_returns_channel_fifteen_not_the_sum() {
        let mut ch = Channels([0; NUM_CHANNELS]);
        for (i, c) in CHANNELS[..NUM_WALKED].iter().enumerate() {
            ch.set(*c, i as u32 + 1);
        }
        let ca = CheckAll {
            channels: ch,
            per: [ChannelReport::default(); NUM_WALKED],
        };
        assert_eq!(ca.returns(), 15, "channel 15 is script_run_time");
        assert_eq!(ca.total(), (1..=15).sum::<u32>());
        assert_ne!(ca.returns(), ca.total());
    }

    /// An empty world walks nothing on the fourteen object/state channels and says so
    /// honestly. Channel 15 is the one exception and deliberately so: retail's
    /// `RunTimeEnv::walk_data` `0x009c41a0` always hashes the four-byte
    /// `ScriptFile::script_files` count, so an empty script registry is
    /// `adler32(1, [0;4])`, not the adler-of-nothing 1. This entry point has no replay
    /// map; the map-aware bridge is exercised separately.
    #[test]
    fn an_empty_world_is_all_ones_except_the_empty_script_file_count() {
        let w = don_sim::World::with_capacity(8, 1);
        let ca = check_all(&w);
        let script = Channel::ScriptRunTime as usize;
        for i in 0..NUM_WALKED {
            if i == script {
                continue;
            }
            assert_eq!(ca.channels.0[i], 1, "{} walked something", CHANNEL_NAMES[i]);
        }
        assert_eq!(
            ca.channels.get(Channel::ScriptRunTime),
            crate::script_channel::EMPTY_RUNTIME_CHANNEL
        );
        assert_eq!(ca.bytes_walked(), 4);
        assert_eq!(ca.non_trivial(), vec!["script_run_time"]);
        assert_eq!(
            ca.total(),
            14 + crate::script_channel::EMPTY_RUNTIME_CHANNEL
        );
        assert!(ca.record().total_is_consistent());
    }

    /// One unit in the world moves exactly the `units` channel, makes it
    /// non-trivial, and moves the total by the same amount.
    #[test]
    fn a_populated_world_makes_the_units_channel_non_trivial() {
        let mut w = don_sim::World::with_capacity(8, 99);
        w.spawn(0).expect("spawn");
        let ca = check_all(&w);
        assert_eq!(ca.per[Channel::Units as usize].elements, 1);
        assert!(
            ca.per[Channel::Units as usize].bytes >= 145,
            "Unit 111 + Object 34: {:?}",
            ca.per[Channel::Units as usize]
        );
        assert_ne!(ca.channels.get(Channel::Units), 1);
        assert_eq!(ca.channels.get(Channel::Walls), 1, "walls untouched");
        assert_eq!(ca.non_trivial(), vec!["units", "script_run_time"]);
        assert!(ca.record().total_is_consistent());
    }

    /// Two worlds differing in one unit field produce different `units`
    /// channels. Without this the bridge could be writing nothing at all.
    #[test]
    fn a_one_field_difference_reaches_the_channel() {
        let mut a = don_sim::World::with_capacity(8, 5);
        let h = a.spawn(2).expect("spawn");
        let mut b = a.clone();
        let row = b.row_of(h).expect("row");
        // `stance` at +177 is one byte inside Unit::walk_data's [72,183).
        b.units.stance_mut()[row] ^= 1;
        assert_ne!(
            check_all(&a).channels.get(Channel::Units),
            check_all(&b).channels.get(Channel::Units),
            "a walked byte changed and the channel did not"
        );
    }

    /// A field the walker does **not** visit must not move the channel. This is
    /// the other half of the bite test: a walker that hashed the whole image
    /// would pass the test above and fail this one.
    ///
    /// `on_screen` at `+28` is the field to use, and finding that out corrected
    /// a wrong belief on the way: `x_internal` at `+16` *looks* unwalked —
    /// `don-sim`'s generated `walked` flag says so — but `SubObject::walk_data`
    /// `0x006621d0` walks `[9,24)`, so the coordinates are hashed on every unit
    /// on every turn through the base class.
    #[test]
    fn an_unwalked_field_does_not_reach_the_channel() {
        let mut a = don_sim::World::with_capacity(8, 5);
        let h = a.spawn(2).expect("spawn");
        let mut b = a.clone();
        let row = b.row_of(h).expect("row");
        b.units.set_on_screen(row, 1);
        assert_eq!(
            check_all(&a).channels.get(Channel::Units),
            check_all(&b).channels.get(Channel::Units),
            "an unwalked field reached the checksum"
        );
        // ...and the same edit to a walked base-class field does move it.
        let mut c = a.clone();
        c.units.x_internal_mut()[row] ^= 0x7f;
        assert_ne!(
            check_all(&a).channels.get(Channel::Units),
            check_all(&c).channels.get(Channel::Units),
            "SubObject::walk_data walks [9,24), which contains x_internal"
        );
    }

    /// The source table and the bridge must agree about which channels are
    /// produced at all, or the scoreboard's honesty annotation is a lie.
    #[test]
    fn the_source_table_matches_what_the_bridge_fills() {
        let mut w = don_sim::World::with_capacity(8, 3);
        for who in 0..3u8 {
            w.spawn(who).expect("spawn");
        }
        let mut st = SimState::new();
        let map = don_sim::systems::map_terrain::World::init_default_rules(40, 40);
        w.configure_items(&map);
        SimBridge::populate_with_map(&w, &map, 0, &mut st);
        for i in 0..NUM_WALKED {
            let filled = st.channel_is_installed(i);
            match CHANNEL_SOURCE[i] {
                ChannelSource::Modelled => assert!(
                    filled,
                    "{} is declared modelled but the bridge produced nothing",
                    CHANNEL_NAMES[i]
                ),
                ChannelSource::Conditional => assert!(
                    filled,
                    "{} was initialized but its conditional bridge produced nothing",
                    CHANNEL_NAMES[i]
                ),
                ChannelSource::Absent => assert!(
                    !filled,
                    "{} is declared absent but the bridge produced elements",
                    CHANNEL_NAMES[i]
                ),
            }
        }
    }

    /// The walker table is the fifteen channels in wire order, and every VA is
    /// the one the channel table already carries.
    #[test]
    fn the_walker_table_is_the_channel_table() {
        assert_eq!(WALKERS.len(), NUM_WALKED);
        for i in 0..NUM_WALKED {
            assert_eq!(WALKERS[i].name, CHANNEL_NAMES[i]);
            assert_eq!(WALKERS[i].va, crate::checksum::CHANNEL_WALKER_VA[i]);
        }
    }

    /// The checksum primitive must be singular. `don-sim` owns the derived
    /// `adler32`; this crate's copy has to agree with it byte for byte or one of
    /// them is a different function.
    #[test]
    fn the_two_adler_implementations_agree() {
        let mut s: u32 = 0x1234_5678;
        let mut buf = Vec::new();
        for n in [0usize, 1, 2, 15, 16, 17, 31, 5551, 5552, 5553, 11105] {
            buf.clear();
            for i in 0..n {
                s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                buf.push((s >> 16) as u8 ^ i as u8);
            }
            for seed in [1u32, 0, 0xffff_ffff, 0x0007_0007] {
                assert_eq!(
                    crate::checksum::adler32(seed, &buf),
                    don_sim::checksum::adler32(seed, &buf),
                    "len {n} seed {seed:08x}"
                );
            }
        }
    }
}
