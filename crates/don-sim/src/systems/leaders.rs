//! Step 8 of `Game::do_frame`: `Leaders::process_all` `0x006ED2A0` and its second level.
//!
//! # What this file is
//!
//! `economy.rs` ports the *contents* of the per-player economy — `calc_gather`,
//! `calc_resource_caps`, `do_gather`, the market, taxes, tribute. It does not port the
//! thing that **calls** them. This file is that caller: the 387-byte loop at
//! `0x006ED2A0` that `Game::do_frame` reaches as step 8 of 29, before anything moves.
//!
//! ```text
//! Game::do_frame                    0x00591EF0
//!  +- [8] Leaders::process_all      0x006ED2A0   <- [`process_all`], this file
//!      for slot in 0..8, flags & 2:
//!        flags &= ~0x40000                       ; the hostile-contact bit, recomputed
//!        leader[0x7E8] = 0 ; leader[0x9F4] = 0    ; two per-frame counters
//!        for other in 0..8, flags & 1:            ; the diplomacy scan
//!            ...                                  ; -> [`Leaders::scan_hostiles`]
//!        Leader::gather             0x006CE280   -> [`leader_gather`]
//!          +- Leader::calc_gather        0x006CEEE0  economy::calc_gather
//!          +- BitMask<44> union          0x006CE35F  <- NEW HERE: sets 0x0C000000
//!          +- Leader::calc_resource_caps 0x006CE900  economy::calc_resource_caps
//!          +- Leader::do_gather          0x006CE450  economy::do_gather
//!        if flags & 0x8000000: clear, Leader::calc_wall_stats 0x006CF7C0 -> [`calc_wall_stats`]
//!        if flags & 0x4000000: clear, Leader::calc_unit_stats 0x006CF970 -> [`calc_unit_stats`]
//!          +- Leader::calc_attrition      0x006CDEA0 -> [`calc_attrition`]
//!          +- Leader::calc_anti_attrition 0x006CDCC0 -> [`calc_anti_attrition`]
//!        Leader::process_elimination 0x006B8A20      ; ported in `victory_score.rs`, see below
//!        if RULES.timer_refresh_ratio && frame % it == 0: three grace timers creep
//!        if frame != 0: scan 8 taunt slots for one stamped this frame
//!        flags &= ~0x80000
//! ```
//!
//! Everything above is [measured] from a capstone disassembly of `0x006ED2A0`,
//! `0x006CE280`, `0x006CF7C0`, `0x006CF970`, `0x006CDEA0`, `0x006CDCC0` and `0x006B8A20`
//! against `ron-bin/riseofnations.exe` (sha256 `30478a44…625079`), cross-read against
//! `re/decomp-all/`. **Tier C**: structure and constants are read from the binary, nothing
//! here has been executed against retail, and no claim may be promoted without an oracle
//! run.
//!
//! # Five things the earlier rendition of step 8 did not have
//!
//! 1. **The outer gate is `flags & 2`, not `flags & 1`.** `0x006ED2B0` tests bit 1;
//!    bit 0 is the gate on the *inner* diplomacy scan (`0x006ED2E2`) and on step 19's
//!    `Leader::process_event_frame` loop (`0x005924A5`). They are different bits and a
//!    port that uses one `active` boolean for both cannot be right in both places.
//! 2. **`calc_wall_stats` and `calc_unit_stats` are edge-triggered**, on `0x8000000` and
//!    `0x4000000`. Nothing in step 8 sets those except `Leader::gather` itself, at
//!    `0x006CE3D0`, when the leader's effective rare mask changes. Without that union the
//!    two functions never run at all, so "they have no port" understated the gap: the
//!    *protocol* that decides when they run was also missing.
//! 3. **Three grace timers creep toward zero** every `TIMER_REFRESH_RATIO` frames while
//!    their companion flag is clear (`0x006ED381`..`0x006ED3CE`).
//! 4. **The taunt table is eight parallel triples** at `+0x394` / `+0x3B4` / `+0x3D4`, and
//!    the scan is gated on `Game::frame != 0` — with a zeroed table that gate is the only
//!    thing stopping all eight firing on frame 0.
//! 5. `Leaders::process_all` reads `Game::frame` at `Game + 0x550`, and step 8 runs
//!    **before** the increment at step 20 (`0x005924BF`). Step 8 therefore sees the
//!    pre-increment frame, and so does `economy::calc_gather_due`'s `% 256` phase.
//!
//! # What is deliberately not here
//!
//! * **`Leader::process_elimination` `0x006B8A20`** is already ported, correctly, as
//!   `victory_score::Leaders::process_elimination`, over `Game::retake_capital`
//!   `0x00594530`. Duplicating it would give the tick two disagreeing copies of one
//!   retail function, which is the `adler32`-implemented-nine-times failure. [`Step8Trace`]
//!   records the slots at which retail calls it, in retail's order, so the scheduler can
//!   drive the existing port from here.
//! * **`Leader::process_taunt` `0x006B8CC0`** (2,340 B) is AI chat. Dispatches are recorded
//!   in [`Step8Trace::taunts`] and the body is not ported.
//! * The bodies behind the four unresolved virtual slots that `calc_wall_stats` /
//!   `calc_unit_stats` call (`+0x4C`, `+0xE8`, `+0x15C`, `+0x160` on the object's data) are
//!   the object graph's, not the leader's. They are represented by [`StatObject`]'s
//!   observable booleans and counted, never invented.
//!
//! # Two facts about `Leader::calc_anti_attrition` worth stating out loud
//!
//! It is **floating point in the simulation**: `0x006CDCC0` is SSE binary32 throughout
//! (`movss`/`mulss`/`divss`/`cvtdq2ps`), storing an f32 at `leader + 0x7F4` whose "no
//! resistance" value is `0x43800000` = 256.0. `README-LLM.md`'s float note is about
//! `get_damage` and the road A\*, and does not extend here. The chain is `x = x * 100.0f /
//! (float)(100 - rule)` per source, evaluated in that order; reassociating it changes the
//! result, so [`calc_anti_attrition`] does not reassociate it.
//!
//! And its multiplier sources are read from **two** rare masks — the effective mask's
//! payload byte at `leader + 0x6DA7` and mask B's at `leader + 0x6DCF`, both bit `0x40`,
//! i.e. rare bit 30 (`0x006CDE47`/`0x006CDE50`). That is the same bit-30 in the two
//! `BitMask<44>` payloads the union at `0x006CE35F` relates, which is how the mask layout
//! below was cross-checked.

#![allow(clippy::needless_range_loop)]

use crate::systems::economy::{
    self, pct, CapGates, DoGatherContext, EconRules, GatherInputs, LeaderEcon, Payout,
    NUM_RESOURCES,
};

// ===========================================================================================
// The leaders array
// ===========================================================================================

/// `Leaders::process_all`'s loop runs `[0x00E3A390, 0x00E71AF0)` in strides of `0x6EEC`,
/// which is exactly eight (`0x00E71AF0 - 0x00E3A390 == 8 * 0x6EEC`) [measured,
/// `0x006ED2A3`/`0x006ED40D`/`0x006ED413`]. The same three constants drive step 19.
pub const NUM_LEADER_SLOTS: usize = 8;
/// `sizeof(Leader)` as the loop strides it.
pub const LEADER_STRIDE: u32 = 0x6EEC;
/// `Leaders leaders` — the static array `Leaders::process_all` walks.
pub const LEADERS_BASE_VA: u32 = 0x00E3A390;
/// One past the last leader.
pub const LEADERS_END_VA: u32 = 0x00E71AF0;

/// The diplomacy value that means "allied". `0x006ED2F1` and `0x006ED300` both compare the
/// stored `diplo` dword against `2`, and the hostile bit is only suppressed when **both**
/// directions read 2.
pub const DIPLO_ALLIED: i32 = 2;

/// Bits of `Leader + 0x00` that step 8 reads or writes.
///
/// Only the bits this function touches are named. A bit not listed here is not "unused",
/// it is unread by `0x006ED2A0`.
pub mod flag {
    /// `& 1` — gate on the **inner** diplomacy scan (`0x006ED2E2`) and on step 19's
    /// `Leader::process_event_frame` loop (`0x005924A5`).
    pub const IN_GAME: u32 = 0x0000_0001;
    /// `& 2` — gate on the **outer** loop (`0x006ED2B0`). This is the one that decides
    /// whether a leader's economy runs at all.
    pub const PROCESS: u32 = 0x0000_0002;
    /// Read on the *other* leader in the diplomacy scan (`0x006ED307`). A leader without
    /// it never makes anyone hostile, whatever the diplomacy says.
    pub const COUNTS_AS_HOSTILE: u32 = 0x0002_0000;
    /// Cleared at `0x006ED2B9` and recomputed by the scan (`0x006ED30F`): "somebody I am
    /// not mutually allied with is live".
    pub const HOSTILE_SEEN: u32 = 0x0004_0000;
    /// Cleared at the tail of every processed leader (`0x006ED407`). Nothing in step 8
    /// sets it, so it is an inbound request bit owned by some other subsystem.
    pub const PENDING: u32 = 0x0008_0000;
    /// `& 0x4000000` — request `Leader::calc_unit_stats` (`0x006ED343`).
    pub const UNIT_STATS_DIRTY: u32 = 0x0400_0000;
    /// `& 0x8000000` — request `Leader::calc_wall_stats` (`0x006ED32C`).
    pub const WALL_STATS_DIRTY: u32 = 0x0800_0000;
    /// Both, as `Leader::gather` sets them in one `or` at `0x006ED3D0`.
    pub const STATS_DIRTY: u32 = UNIT_STATS_DIRTY | WALL_STATS_DIRTY;
}

/// Byte offsets inside `Leader`, each recovered from the instruction that reads it.
pub mod offsets {
    /// `0x006ED2B0` — the flag word.
    pub const FLAGS: usize = 0x000;
    /// `0x006ED2E7` — the player slot. Note the diplomacy scan indexes the leaders array
    /// by **this**, not by loop position (`0x006ED2F8`: `imul eax, eax, 0x1BBB`).
    pub const SLOT: usize = 0x008;
    /// `0x006ED2F1` — 8 dwords, `diplo[other_slot]`.
    pub const DIPLO: usize = 0x074;
    /// `0x006ED3F1` (`[edi - 0x20]`) — 8 dwords, first argument of `Leader::process_taunt`.
    pub const TAUNT_KIND: usize = 0x394;
    /// `0x006ED3ED` (`[edi]`) — 8 dwords, second argument.
    pub const TAUNT_ARG: usize = 0x3B4;
    /// `0x006ED3E2` (`[edi + 0x20]`) — 8 dwords, the frame the taunt is stamped for.
    pub const TAUNT_FRAME: usize = 0x3D4;
    /// `0x006ED381` — grace timer 0's value. Also `Leader::process_elimination`'s
    /// capital-loss stamp (`0x006B8A4F`, via `0x00E3A7A4 = 0x00E3A390 + 0x414`).
    pub const TIMER0_VALUE: usize = 0x414;
    /// `0x006ED38B` — grace timer 0's freeze flag; also `process_elimination`'s gate
    /// (`0x006B8A36`).
    pub const TIMER0_FROZEN: usize = 0x418;
    /// `0x00594587` — the 8.8 per-leader scale `Game::retake_capital` multiplies by.
    pub const RETAKE_SCALE: usize = 0x41C;
    /// `0x006ED39A` / `0x006ED3A4`.
    pub const TIMER1_VALUE: usize = 0x440;
    pub const TIMER1_FROZEN: usize = 0x444;
    /// `0x006ED3B4` / `0x006ED3BE`.
    pub const TIMER2_VALUE: usize = 0x448;
    pub const TIMER2_FROZEN: usize = 0x44C;
    /// `0x006CDFDC` — the attrition this leader inflicts.
    pub const ATTRITION: usize = 0x7F0;
    /// `0x006CDCCE` — **f32**, the anti-attrition scale, 256.0 = none.
    pub const ANTI_ATTRITION: usize = 0x7F4;
    /// `0x006CDEA6` — non-zero forces attrition to 0.
    pub const ATTRITION_OFF: usize = 0x7F8;
    /// `0x006CDCC7` — non-zero forces anti-attrition to 0.0.
    pub const ANTI_ATTRITION_OFF: usize = 0x7FC;
    /// `0x006ED2CA` — zeroed for every processed leader, every frame.
    pub const FRAME_COUNTER_A: usize = 0x7E8;
    /// `0x006ED2D4` — likewise.
    pub const FRAME_COUNTER_B: usize = 0x9F4;
    /// `0x006CDF6E` — the CTW gate byte `calc_attrition` tests.
    pub const CONQUEST_BYTE: usize = 0x6900;
    /// `BitMask<44>` object; payload at `+0xC` = `0x6DA4`. The effective mask.
    pub const RARE_MASK_EFFECTIVE: usize = 0x6D98;
    /// `BitMask<44>` object; payload `0x6DB8`. Left operand of the union.
    pub const RARE_MASK_A: usize = 0x6DAC;
    /// `BitMask<44>` object; payload `0x6DCC` — the mask `economy::GatherInputs::rares`
    /// mirrors. Right operand of the union.
    pub const RARE_MASK_B: usize = 0x6DC0;
    /// `0x006CE28B` — pointer to the economy block `economy::LeaderEcon` models.
    pub const ECON_PTR: usize = 0x6EB8;
}

// ===========================================================================================
// BitMask<44>
// ===========================================================================================

/// Payload bytes of a `BitMask<44>`.
///
/// The class is 20 bytes: three header dwords then the payload at `+0xC`, which is why the
/// three consecutive objects sit at `0x6D98`, `0x6DAC`, `0x6DC0` and their payloads at
/// `0x6DA4`, `0x6DB8`, `0x6DCC`. `Leader::gather`'s copy at `0x006CE382` moves the three
/// header dwords, then `memcpy`s `len & ~3` payload bytes and finishes the tail a byte at a
/// time — a length of 6 for 44 bits is exactly what makes that four-plus-two split
/// necessary, which is the cross-check on this layout.
pub const RARE_MASK_BYTES: usize = 6;

/// A `BitMask<44>` payload.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub struct RareMask {
    pub bytes: [u8; RARE_MASK_BYTES],
}

impl RareMask {
    /// Rare bit 30 — the one `calc_anti_attrition` tests as `byte[3] & 0x40`
    /// (`0x006CDE47`), whose rule is `TITANIUM_ATTRITION`.
    pub const TITANIUM: usize = 30;

    /// Bit count. `economy::NUM_RARES` is the same 44.
    pub const BITS: usize = 44;

    pub const fn empty() -> RareMask {
        RareMask {
            bytes: [0; RARE_MASK_BYTES],
        }
    }

    /// Build from bit indices. Out-of-range indices are ignored.
    pub fn from_bits(bits: &[usize]) -> RareMask {
        let mut m = RareMask::empty();
        for &b in bits {
            m.set(b, true);
        }
        m
    }

    #[inline]
    pub fn get(&self, bit: usize) -> bool {
        if bit >= Self::BITS {
            return false;
        }
        self.bytes[bit / 8] & (1u8 << (bit % 8)) != 0
    }

    #[inline]
    pub fn set(&mut self, bit: usize, on: bool) {
        if bit >= Self::BITS {
            return;
        }
        let m = 1u8 << (bit % 8);
        if on {
            self.bytes[bit / 8] |= m;
        } else {
            self.bytes[bit / 8] &= !m;
        }
    }

    /// `BitMask<44>::operator|` `0x0047CE20`.
    #[inline]
    pub fn union(&self, other: &RareMask) -> RareMask {
        let mut out = RareMask::empty();
        for i in 0..RARE_MASK_BYTES {
            out.bytes[i] = self.bytes[i] | other.bytes[i];
        }
        out
    }
}

// ===========================================================================================
// The rules step 8 reads
// ===========================================================================================

/// The `Constants` slots step 8 and its second level read, by byte offset into the same
/// value block `economy::EconRules` addresses.
///
/// These are kept apart from `EconRules` on purpose: `EconRules::shipped()` carries only
/// the economy subset, so reading `timer_refresh_ratio` through it would silently return 0
/// and quietly disable the grace timers. Values below are the loader's, from
/// `docs/derivation/rules-constants.json` — captured from `Constants::init`, never
/// hand-computed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Step8Rules {
    /// `RULES + 0xD00` = 3328, `TIMER_REFRESH_RATIO`, shipped 5. Divisor of `Game::frame`
    /// at `0x006ED37B`; **zero disables the whole timer block** (`0x006ED370`).
    pub timer_refresh_ratio: i32,
    /// `RULES + 0x1D8` = 472, `ATTRITION_IMPROVED[4]`, shipped `1, 2, 4, 8`. Indexed
    /// `count - 1` by `0x006CDEEC`'s `[eax + edi*4 + 0x1D4]`.
    pub attrition_improved: [i32; 4],
    /// `RULES + 0x1C8` = 456, `ATTRITION_UPGRADE[4]`, shipped `25, 50, 75, 100` — percent
    /// *reduction*, so a value of 100 collapses anti-attrition to 0.
    pub attrition_upgrade: [i32; 4],
    /// `RULES + 0x470` = 1136, `COLOSSEUM_ATTRITION`, shipped 50.
    pub colosseum_attrition: i32,
    /// `RULES + 0x74C` = 1868, `RUSSIAN_ATTRITION`, shipped 100.
    pub russian_attrition: i32,
    /// `RULES + 0xA0C` = 2572, `CTW_ATTRITION`, shipped 50.
    pub ctw_attrition: i32,
    /// `RULES + 0x510` = 1296, `KREMLIN_ATTRITION`, shipped 100.
    pub kremlin_attrition: i32,
    /// `RULES + 0x524` = 1316, `LIBERTY_ATTRITION`, shipped 100 — which means the Statue of
    /// Liberty branch at `0x006CDDB7` takes the `>= 100` exit and zeroes the scale outright.
    pub liberty_attrition: i32,
    /// `RULES + 0x7F0` = 2032, `MONGOL_ATTRITION`, shipped 50.
    pub mongol_attrition: i32,
    /// `RULES + 0x95C` = 2396, `TITANIUM_ATTRITION`, shipped 50.
    pub titanium_attrition: i32,
}

/// Byte offsets of every [`Step8Rules`] field, so [`Step8Rules::from_block`] and a
/// disassembly listing can be diffed by eye.
pub mod rule_offsets {
    pub const TIMER_REFRESH_RATIO: usize = 3328;
    pub const ATTRITION_UPGRADE: usize = 456;
    pub const ATTRITION_IMPROVED: usize = 472;
    pub const COLOSSEUM_ATTRITION: usize = 1136;
    pub const KREMLIN_ATTRITION: usize = 1296;
    pub const LIBERTY_ATTRITION: usize = 1316;
    pub const RUSSIAN_ATTRITION: usize = 1868;
    pub const MONGOL_ATTRITION: usize = 2032;
    pub const TITANIUM_ATTRITION: usize = 2396;
    pub const CTW_ATTRITION: usize = 2572;
}

impl Default for Step8Rules {
    fn default() -> Self {
        Step8Rules::shipped()
    }
}

impl Step8Rules {
    /// The shipped `rules.xml` values.
    pub const fn shipped() -> Step8Rules {
        Step8Rules {
            timer_refresh_ratio: 5,
            attrition_improved: [1, 2, 4, 8],
            attrition_upgrade: [25, 50, 75, 100],
            colosseum_attrition: 50,
            russian_attrition: 100,
            ctw_attrition: 50,
            kremlin_attrition: 100,
            liberty_attrition: 100,
            mongol_attrition: 50,
            titanium_attrition: 50,
        }
    }

    /// All zeros — a state the engine is never in, since `timer_refresh_ratio` is a
    /// divisor. For tests that want the timer block provably off.
    pub const fn zeroed() -> Step8Rules {
        Step8Rules {
            timer_refresh_ratio: 0,
            attrition_improved: [0; 4],
            attrition_upgrade: [0; 4],
            colosseum_attrition: 0,
            russian_attrition: 0,
            ctw_attrition: 0,
            kremlin_attrition: 0,
            liberty_attrition: 0,
            mongol_attrition: 0,
            titanium_attrition: 0,
        }
    }

    /// Read out of a full `Constants` value block — `don_rules::Rules::raw`, or a live
    /// capture of `[[0x00C061F0]]`. Slots past the end of `block` read as zero.
    pub fn from_block(block: &[i32]) -> Step8Rules {
        let at = |byte_off: usize| -> i32 { block.get(byte_off / 4).copied().unwrap_or(0) };
        let arr4 = |byte_off: usize| -> [i32; 4] {
            [
                at(byte_off),
                at(byte_off + 4),
                at(byte_off + 8),
                at(byte_off + 12),
            ]
        };
        Step8Rules {
            timer_refresh_ratio: at(rule_offsets::TIMER_REFRESH_RATIO),
            attrition_improved: arr4(rule_offsets::ATTRITION_IMPROVED),
            attrition_upgrade: arr4(rule_offsets::ATTRITION_UPGRADE),
            colosseum_attrition: at(rule_offsets::COLOSSEUM_ATTRITION),
            russian_attrition: at(rule_offsets::RUSSIAN_ATTRITION),
            ctw_attrition: at(rule_offsets::CTW_ATTRITION),
            kremlin_attrition: at(rule_offsets::KREMLIN_ATTRITION),
            liberty_attrition: at(rule_offsets::LIBERTY_ATTRITION),
            mongol_attrition: at(rule_offsets::MONGOL_ATTRITION),
            titanium_attrition: at(rule_offsets::TITANIUM_ATTRITION),
        }
    }
}

// ===========================================================================================
// Leader state
// ===========================================================================================

/// One of the three `(value, frozen)` pairs the timer block at `0x006ED381` creeps.
///
/// The pair semantics come from `Leader::process_elimination`, which reads the same two
/// dwords at `+0x414`/`+0x418`: `frozen != 0` means the clock is *running against you*
/// (`0x006B8A36`), and only while it is clear does `value` creep back toward zero. So a
/// negative `value` is a debt of frames and this block is the refund.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct GraceTimer {
    /// Negative while a debt is outstanding; the block increments it toward 0.
    pub value: i32,
    /// Non-zero freezes the creep. `Leader::process_elimination` requires it non-zero.
    pub frozen: i32,
}

/// The slice of `Leader` that step 8 touches.
///
/// Field-for-field with [`offsets`]; nothing here is a convenience aggregate except the
/// two `economy` scheduling fields, which live in the engine's `Leader::calc_gather`
/// prologue rather than at a byte offset we have pinned.
#[derive(Clone, Debug, Default)]
pub struct Leader {
    /// `+0x00`.
    pub flags: u32,
    /// `+0x08`.
    pub slot: i32,
    /// `+0x74`, indexed by the *other* leader's slot.
    pub diplo: [i32; NUM_LEADER_SLOTS],
    /// `+0x394`.
    pub taunt_kind: [i32; NUM_LEADER_SLOTS],
    /// `+0x3B4`.
    pub taunt_arg: [i32; NUM_LEADER_SLOTS],
    /// `+0x3D4`.
    pub taunt_frame: [i32; NUM_LEADER_SLOTS],
    /// `+0x414/+0x418`, `+0x440/+0x444`, `+0x448/+0x44C`, in that order.
    pub timers: [GraceTimer; 3],
    /// `+0x41C`.
    pub retake_scale: i32,
    /// `+0x7E8`.
    pub frame_counter_a: i32,
    /// `+0x9F4`.
    pub frame_counter_b: i32,
    /// `+0x7F0`.
    pub attrition: i32,
    /// `+0x7F4`, f32.
    pub anti_attrition: f32,
    /// `+0x7F8`.
    pub attrition_off: i32,
    /// `+0x7FC`.
    pub anti_attrition_off: i32,
    /// `+0x6900`, the CTW gate byte.
    pub conquest_byte: u8,
    /// `+0x6D98` payload.
    pub rare_effective: RareMask,
    /// `+0x6DAC` payload.
    pub rare_a: RareMask,
    /// `+0x6DC0` payload — the mask `economy::GatherInputs::rares` mirrors.
    pub rare_b: RareMask,
    /// `*(Leader + 0x6EB8)`.
    pub econ: LeaderEcon,
    /// `Leader::calc_gather`'s recompute stamp.
    pub last_calc_frame: i32,
    /// `Leader::calc_gather`'s `0x2000000` economy-dirty bit, held apart from [`flags`]
    /// because `economy::calc_gather_due` already owns its meaning.
    pub econ_dirty: bool,
}

impl Leader {
    /// A leader in no game at all: every flag clear, so [`process_all`] skips it.
    pub fn new(slot: i32) -> Leader {
        Leader {
            slot,
            anti_attrition: NO_ANTI_ATTRITION,
            ..Default::default()
        }
    }

    /// The two bits that put a leader in the outer loop *and* in everybody else's
    /// diplomacy scan.
    pub fn activate(&mut self) -> &mut Leader {
        self.flags |= flag::IN_GAME | flag::PROCESS;
        self
    }

    #[inline]
    pub fn diplo_toward(&self, other_slot: i32) -> i32 {
        if (0..NUM_LEADER_SLOTS as i32).contains(&other_slot) {
            self.diplo[other_slot as usize]
        } else {
            // A port guard, not a game state: retail's index is a slot the engine
            // guarantees in range by construction.
            0
        }
    }
}

/// `leader + 0x7F4` when nothing resists attrition: `0x43800000` = 256.0f
/// (`0x006CDCCE`, a `mov` of the bit pattern, not a load).
pub const NO_ANTI_ATTRITION: f32 = 256.0;

/// The eight `Leader` slots the loop walks.
#[derive(Clone, Debug)]
pub struct Leaders {
    pub leaders: [Leader; NUM_LEADER_SLOTS],
}

impl Default for Leaders {
    fn default() -> Self {
        Leaders::new()
    }
}

impl Leaders {
    /// Eight inactive leaders whose `slot` matches their array index, which is what the
    /// engine's own initialisation produces and what the diplomacy scan's
    /// `leaders[a.slot]` indexing assumes.
    pub fn new() -> Leaders {
        Leaders {
            leaders: std::array::from_fn(|i| Leader::new(i as i32)),
        }
    }

    /// `leaders_base + slot * 0x6EEC` — the scan's own addressing (`0x006ED2F8`).
    #[inline]
    pub fn by_slot(&self, slot: i32) -> Option<&Leader> {
        if (0..NUM_LEADER_SLOTS as i32).contains(&slot) {
            Some(&self.leaders[slot as usize])
        } else {
            None
        }
    }

    /// The inner loop of `0x006ED2E0`..`0x006ED321`, hoisted so the borrow of the leader
    /// being written does not overlap the reads.
    ///
    /// Retail asks, for every other leader `b` that is `IN_GAME` and on a different slot:
    /// *are `a` and `b` mutually allied?* If not, and `b` `COUNTS_AS_HOSTILE`, `a` gets
    /// `HOSTILE_SEEN`. Note the two diplomacy reads use different objects — `b.diplo[a]`
    /// comes off the leader being scanned, `a.diplo[b]` off `leaders[a.slot]` — and the
    /// short-circuit means `a.diplo[b]` is not even read when `b.diplo[a] != 2`.
    pub fn scan_hostiles(&self, a_slot: i32) -> bool {
        for b in self.leaders.iter() {
            if b.flags & flag::IN_GAME == 0 {
                continue;
            }
            let b_slot = b.slot;
            if a_slot == b_slot {
                continue;
            }
            let mutual_ally = b.diplo_toward(a_slot) == DIPLO_ALLIED
                && self
                    .by_slot(a_slot)
                    .map(|a| a.diplo_toward(b_slot) == DIPLO_ALLIED)
                    .unwrap_or(false);
            if !mutual_ally && (b.flags & flag::COUNTS_AS_HOSTILE != 0) {
                return true;
            }
        }
        false
    }

    /// adler-32 over the eight economy blocks, as `economy::leaders_channel` builds it.
    ///
    /// This is the **modelled subset** of channel 8, not channel 8: `LeaderData::walk_data`
    /// `0x006D6750` walks 27,182 bytes and `economy::LeaderEcon::image` emits 244. It is
    /// comparable between two runs of this code and is not yet comparable to retail.
    pub fn econ_channel(&self) -> u32 {
        let econs: [LeaderEcon; NUM_LEADER_SLOTS] = std::array::from_fn(|i| self.leaders[i].econ);
        economy::leaders_channel(&econs)
    }
}

// ===========================================================================================
// Leader::calc_attrition  0x006CDEA0
// ===========================================================================================

/// The tech / tribe / wonder answers `calc_attrition` and `calc_anti_attrition` obtain by
/// querying the leader, one field per retail call.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct AttritionGates {
    /// `LeaderData::has_preq` `0x006DB810` for type indices `0x2DD..=0x2E0`, in order.
    /// The loop at `0x006CDEB8` **stops at the first miss**, so these are consumed as a
    /// consecutive run, not a popcount.
    pub attrition_preqs: [bool; 4],
    /// `LeaderData::has_wonder(0x212)` `0x006EBC10` — the Colosseum.
    pub colosseum: bool,
    /// `LeaderData::has_tribe_bonus(0x0D)` `0x006E1370` — the Russians.
    pub russian: bool,
    /// `gamec[0x822] & 2` — Conquer-the-World. Combined with `Leader + 0x6900`.
    pub conquest_mode: bool,
    /// `LeaderData::has_wonder(0x21A)` — the Kremlin.
    pub kremlin: bool,
    /// `has_preq(0x2FE)`, `has_preq(0x2FF)`, `has_preq(0x300)` — the anti-attrition
    /// upgrades, tested **highest first** (`0x006CDCF1`), so only the best one applies.
    pub anti_preqs: [bool; 3],
    /// `has_wonder(0x219)` — the Statue of Liberty.
    pub liberty: bool,
    /// `has_tribe_bonus(0x11)` — the Mongols.
    pub mongol: bool,
}

/// `Leader::calc_attrition` `0x006CDEA0` — how much attrition this leader inflicts.
///
/// ```text
/// if (leader[0x7F8]) { leader[0x7F0] = 0; return; }
/// n = number of consecutive has_preq(0x2DD..0x2E0), stopping at the first miss
/// v = n ? ATTRITION_IMPROVED[n - 1] : 0
/// each of Colosseum / Russians / CTW / Kremlin:  v = v * (RULE + 100) / 100, and if that
///                                                lands on 0 it is forced to 1
/// leader[0x7F0] = v
/// ```
///
/// The floor is a `cmove` on the flags of the `add` that finishes the `/100` idiom
/// (`0x006CDF2B`), i.e. it fires exactly when the product truncates to zero — not when the
/// input was zero. With `v = 0` going in, every multiplier therefore *raises* it to 1.
///
/// One dead branch, recorded because a future reader will hit it: `0x006CDEB8` compares the
/// loop variable against `0x2AD` and only then consults `has_tribe_bonus(4)`. The loop runs
/// `0x2DD..=0x2E0`, so that compare is never true and the tribe-bonus call is unreachable in
/// this build.
pub fn calc_attrition(leader: &mut Leader, rules: &Step8Rules, gates: &AttritionGates) {
    if leader.attrition_off != 0 {
        leader.attrition = 0;
        return;
    }
    let mut n = 0usize;
    for i in 0..4 {
        if !gates.attrition_preqs[i] {
            break;
        }
        n += 1;
    }
    let mut v = if n != 0 {
        rules.attrition_improved[n - 1]
    } else {
        0
    };

    // `0x006CDF09` .. `0x006CDFD8`, in retail's order. Each is skipped whole when its
    // gate is false, so a zero rule is not the same as an absent wonder.
    let scale = |v: &mut i32, rule: i32| {
        *v = pct(*v, rule.wrapping_add(100));
        if *v == 0 {
            *v = 1;
        }
    };
    if gates.colosseum {
        scale(&mut v, rules.colosseum_attrition);
    }
    if gates.russian {
        scale(&mut v, rules.russian_attrition);
    }
    if gates.conquest_mode && leader.conquest_byte != 0 {
        scale(&mut v, rules.ctw_attrition);
    }
    if gates.kremlin {
        scale(&mut v, rules.kremlin_attrition);
    }
    leader.attrition = v;
}

/// `Leader::calc_anti_attrition` `0x006CDCC0` — the leader's attrition resistance, as an
/// **f32** scale where 256.0 is "no resistance".
///
/// Four independent reduction sources, each `x = x * 100.0f / (float)(100 - rule)`, and each
/// of which short-circuits the whole function to `0.0` when its rule is `>= 100`
/// (`0x006CDD5C`, `0x006CDDBD`, `0x006CDE1D`, `0x006CDE5F`). With shipped data
/// `LIBERTY_ATTRITION` is 100, so the Statue of Liberty is total immunity by that exit and
/// not by arithmetic.
///
/// The final source is not a query but a bit: rare 30 in **either** the effective mask or
/// mask B.
pub fn calc_anti_attrition(leader: &mut Leader, rules: &Step8Rules, gates: &AttritionGates) {
    leader.anti_attrition = NO_ANTI_ATTRITION;
    if leader.anti_attrition_off != 0 {
        leader.anti_attrition = 0.0;
        return;
    }

    // `0x006CDCF1`: 0x300 first, then 0x2FF, then 0x2FE; the index into ATTRITION_UPGRADE
    // is 2 / 1 / 0 respectively (`lea ecx,[edi-0x62]` with edi = 100 is the literal 2).
    let upgrade = if gates.anti_preqs[2] {
        Some(2usize)
    } else if gates.anti_preqs[1] {
        Some(1)
    } else if gates.anti_preqs[0] {
        Some(0)
    } else {
        None
    };

    // Returns false when the caller must stop: the rule zeroed the scale outright.
    fn reduce(x: &mut f32, rule: i32) -> bool {
        if rule >= 100 {
            *x = 0.0;
            return false;
        }
        *x = (*x * 100.0) / ((100 - rule) as f32);
        true
    }

    if let Some(k) = upgrade {
        if !reduce(&mut leader.anti_attrition, rules.attrition_upgrade[k]) {
            return;
        }
    }
    if gates.liberty && !reduce(&mut leader.anti_attrition, rules.liberty_attrition) {
        return;
    }
    if gates.mongol && !reduce(&mut leader.anti_attrition, rules.mongol_attrition) {
        return;
    }
    if leader.rare_effective.get(RareMask::TITANIUM) || leader.rare_b.get(RareMask::TITANIUM) {
        reduce(&mut leader.anti_attrition, rules.titanium_attrition);
    }
}

// ===========================================================================================
// Leader::calc_wall_stats 0x006CF7C0 / Leader::calc_unit_stats 0x006CF970
// ===========================================================================================

/// One entry of an `Objects` band, as the two stat passes observe it through the vtable.
///
/// Both functions are pure object-graph traversal — every decision they make is a virtual
/// call on the object's data — so this type carries the *observable answers* rather than
/// inventing bodies for slots we have not resolved. The counters make the pass measurable:
/// a caller can prove the loop ran and how far it got, which is the whole point of wiring
/// it in at all.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct StatObject {
    /// `data->byte[8] & 1` — the active bit both band loops test
    /// (`0x006CF7EC`, `0x006CF9A5`).
    pub active: bool,
    /// `vtbl + 0x4C` on the wall data. Zero routes through `Wall::update_construct_time`
    /// `0x0063D560` (`0x006CF80B`).
    pub construct_time_resolved: bool,
    /// `vtbl + 0xE8` on the unit data — the guard `calc_unit_stats` tests before
    /// re-deriving speed and armor (`0x006CF9CD`).
    pub stats_stale: bool,
    /// How many times `vtbl + 0x160` was invoked on this object.
    pub v160_calls: u32,
    /// How many times `vtbl + 0x15C` was invoked on this object.
    pub v15c_calls: u32,
    /// How many times `Wall::update_construct_time` `0x0063D560` was invoked.
    pub construct_time_updates: u32,
    /// `Unit::update_speed` `0x006055C0`.
    pub speed_updates: u32,
    /// `Unit::update_armor` `0x006054C0`.
    pub armor_updates: u32,
}

/// What one stat pass did, so "it ran" is a number instead of an assertion.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct StatPassCounts {
    pub visited: u32,
    pub active: u32,
    pub construct_time_updates: u32,
    pub speed_updates: u32,
    pub armor_updates: u32,
}

impl StatPassCounts {
    fn add(&mut self, other: StatPassCounts) {
        self.visited += other.visited;
        self.active += other.active;
        self.construct_time_updates += other.construct_time_updates;
        self.speed_updates += other.speed_updates;
        self.armor_updates += other.armor_updates;
    }
}

/// The two `Objects` bands `Leader::calc_wall_stats` walks, and the unit band
/// `Leader::calc_unit_stats` walks.
///
/// The band bases are retail's: the building band starts at index **2000** and runs to
/// `Objects + slot*4 + 0x184`; the wall band starts at **3000** and runs to
/// `Objects + slot*4 + 0x1AC`; the unit band starts at **0** and runs to
/// `Objects + slot*4 + 0x15C`. Those are the same 2000 / 3000 bands `crate::objects`
/// already knows, which is the cross-check that this is the right traversal.
#[derive(Clone, Debug, Default)]
pub struct OwnerObjects {
    /// Band 2000 — buildings.
    pub band_2000: Vec<StatObject>,
    /// Band 3000 — walls.
    pub band_3000: Vec<StatObject>,
    /// Band 0 — units.
    pub units: Vec<StatObject>,
}

/// The body both of `calc_wall_stats`'s loops share (`0x006CF7D8` and `0x006CF88E`).
fn wall_band_pass(band: &mut [StatObject]) -> StatPassCounts {
    let mut c = StatPassCounts::default();
    for o in band.iter_mut() {
        c.visited += 1;
        if !o.active {
            continue;
        }
        c.active += 1;
        if !o.construct_time_resolved {
            // `Wall::update_construct_time` 0x0063D560 — the only non-virtual call in the
            // loop, and the one thing here that is a named retail function rather than a
            // vtable slot.
            o.construct_time_updates += 1;
            c.construct_time_updates += 1;
        }
        o.v15c_calls += 1;
        o.v160_calls += 1;
    }
    c
}

/// `Leader::calc_wall_stats` `0x006CF7C0` — re-derive building and wall stats for one owner.
///
/// Two loops with identical bodies over the 2000 and 3000 bands. The one asymmetry is real
/// and is preserved as a comment rather than as behaviour: the first loop fetches its guard
/// object through vtable slot `+0xAC` and the second through `+0xB0`, while both then use
/// `+0xB0` for the work. With the slot bodies unresolved that distinction has no observable
/// effect here, and it is flagged so nobody "tidies" it later.
pub fn calc_wall_stats(objs: &mut OwnerObjects) -> StatPassCounts {
    let mut c = wall_band_pass(&mut objs.band_2000);
    c.add(wall_band_pass(&mut objs.band_3000));
    c
}

/// `Leader::calc_unit_stats` `0x006CF970` — attrition, then every unit's speed and armor.
///
/// Retail order, and the order matters: both attrition values are recomputed **before** the
/// unit loop, because `Unit::update_speed` `0x006055C0` and `Unit::update_armor`
/// `0x006054C0` are what consume them.
pub fn calc_unit_stats(
    leader: &mut Leader,
    rules: &Step8Rules,
    gates: &AttritionGates,
    objs: &mut OwnerObjects,
) -> StatPassCounts {
    calc_attrition(leader, rules, gates);
    calc_anti_attrition(leader, rules, gates);

    let mut c = StatPassCounts::default();
    for u in objs.units.iter_mut() {
        c.visited += 1;
        if !u.active {
            continue;
        }
        c.active += 1;
        u.v160_calls += 1;
        if u.stats_stale {
            u.v15c_calls += 1;
            u.speed_updates += 1;
            u.armor_updates += 1;
            c.speed_updates += 1;
            c.armor_updates += 1;
        }
    }
    c
}

// ===========================================================================================
// Leader::gather  0x006CE280
// ===========================================================================================

/// What one `Leader::gather` did.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct GatherOutcome {
    /// `economy::calc_gather_due` fired and gross income was recomposed.
    pub gross_recomputed: bool,
    /// The `BitMask<44>` union changed the effective mask, so `0x0C000000` was set.
    pub rare_mask_changed: bool,
    /// Per-resource result of `Leader::do_gather`.
    pub payouts: [Payout; NUM_RESOURCES],
}

/// `Leader::gather` `0x006CE280` — the per-frame entry point, in retail's order.
///
/// `economy::leader_gather` already composes three of the four calls, but it cannot carry
/// the second one, so this drives the four pieces directly:
///
/// 1. `Leader::calc_gather` `0x006CEEE0`, behind `economy::calc_gather_due`.
/// 2. **The `BitMask<44>` union at `0x006CE35F`** — `effective != (A | B)` copies the union
///    in and sets `flags |= 0x0C000000`, the only producer of the two stat-dirty bits step 8
///    consumes. This is the piece that was missing.
/// 3. Zero the six expense slots (`0x006CE3DE`).
/// 4. `Leader::calc_resource_caps` `0x006CE900`, then `Leader::do_gather` `0x006CE450`.
///
/// The prologue and epilogue that read the six gross slots out and XOR them back
/// (`0x006CE291`..`0x006CE352`) are a round trip through `^ 0x872`; `economy::LeaderEcon`
/// holds plain integers and applies the masks in `image()`, so that round trip is the
/// identity here and is deliberately absent rather than forgotten.
pub fn leader_gather(
    leader: &mut Leader,
    rules: &EconRules,
    frame: i32,
    gather: &GatherInputs,
    caps: &CapGates,
    ctx: &DoGatherContext,
) -> GatherOutcome {
    let mut out = GatherOutcome::default();

    // 1.
    if economy::calc_gather_due(
        frame,
        leader.slot,
        leader.last_calc_frame,
        leader.econ_dirty,
    ) {
        let g = economy::calc_gather(rules, gather);
        leader.econ.gross = g.gross;
        leader.econ.breakdown = [0; NUM_RESOURCES];
        leader.econ_dirty = false;
        leader.last_calc_frame = frame;
        out.gross_recomputed = true;
    }

    // 2.
    let union = leader.rare_a.union(&leader.rare_b);
    if leader.rare_effective != union {
        leader.rare_effective = union;
        leader.flags |= flag::STATS_DIRTY;
        out.rare_mask_changed = true;
    }

    // 3.
    leader.econ.expense = [0; NUM_RESOURCES];

    // 4.
    leader.econ.commerce_cap = economy::calc_resource_caps(rules, leader.econ.age, caps);
    out.payouts = economy::do_gather(rules, &mut leader.econ, ctx);
    out
}

// ===========================================================================================
// Leaders::process_all  0x006ED2A0
// ===========================================================================================

/// Everything one leader's step 8 needs that lives outside `Leader`.
#[derive(Clone, Debug, Default)]
pub struct LeaderEnv {
    pub gather: GatherInputs,
    pub caps: CapGates,
    pub payout: DoGatherContext,
    pub attrition: AttritionGates,
    pub objects: OwnerObjects,
}

/// The eight per-leader environments, indexed by slot.
#[derive(Clone, Debug, Default)]
pub struct Step8Env {
    pub leaders: [LeaderEnv; NUM_LEADER_SLOTS],
}

/// One `Leader::process_taunt` `0x006B8CC0` call retail would have made.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TauntDispatch {
    pub slot: usize,
    /// `Leader + 0x394 + k*4`, pushed last, so it is the first parameter.
    pub kind: i32,
    /// `Leader + 0x3B4 + k*4`.
    pub arg: i32,
    /// Which of the eight table entries fired.
    pub entry: usize,
}

/// What step 8 did this frame. Every field is a count or a record of something that
/// happened, so a caller measures the step instead of trusting it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Step8Trace {
    /// Passed the `flags & 2` gate.
    pub processed: [bool; NUM_LEADER_SLOTS],
    /// Ended the frame with `HOSTILE_SEEN` set.
    pub hostile_seen: [bool; NUM_LEADER_SLOTS],
    /// `economy::calc_gather` actually recomposed gross income.
    pub gross_recomputed: [bool; NUM_LEADER_SLOTS],
    /// The rare-mask union changed, so the two stat-dirty bits were set.
    pub rare_mask_changed: [bool; NUM_LEADER_SLOTS],
    /// `Leader::calc_wall_stats` was entered.
    pub wall_stats_ran: [bool; NUM_LEADER_SLOTS],
    /// `Leader::calc_unit_stats` was entered.
    pub unit_stats_ran: [bool; NUM_LEADER_SLOTS],
    pub wall_pass: [StatPassCounts; NUM_LEADER_SLOTS],
    pub unit_pass: [StatPassCounts; NUM_LEADER_SLOTS],
    /// How many of the three grace timers crept.
    pub timers_crept: [u8; NUM_LEADER_SLOTS],
    /// Slots at which retail calls `Leader::process_elimination` `0x006B8A20`, in retail's
    /// order. The port lives in `victory_score::Leaders::process_elimination`; a scheduler
    /// driving this module must invoke it for exactly these, here.
    pub elimination_calls: Vec<usize>,
    /// `Leader::process_taunt` dispatches, in retail's order.
    pub taunts: Vec<TauntDispatch>,
    pub payouts: [[Payout; NUM_RESOURCES]; NUM_LEADER_SLOTS],
}

impl Step8Trace {
    /// How many leaders passed the outer gate.
    pub fn leaders_processed(&self) -> usize {
        self.processed.iter().filter(|b| **b).count()
    }
}

/// **Step 8 of `Game::do_frame`.** `Leaders::process_all` `0x006ED2A0`.
///
/// `frame` is `Game + 0x550` **before** step 20's increment; passing the post-increment
/// value shifts every `% 256` gather phase and every taunt stamp by one frame.
pub fn process_all(
    ls: &mut Leaders,
    frame: i32,
    rules: &Step8Rules,
    econ_rules: &EconRules,
    env: &mut Step8Env,
) -> Step8Trace {
    let mut trace = Step8Trace::default();

    for i in 0..NUM_LEADER_SLOTS {
        // 0x006ED2B0 — the outer gate is bit 1, not bit 0.
        if ls.leaders[i].flags & flag::PROCESS == 0 {
            continue;
        }
        trace.processed[i] = true;

        // 0x006ED2B9 / 0x006ED2CA / 0x006ED2D4.
        ls.leaders[i].flags &= !flag::HOSTILE_SEEN;
        ls.leaders[i].frame_counter_a = 0;
        ls.leaders[i].frame_counter_b = 0;

        // 0x006ED2E0..0x006ED321 — the diplomacy scan, read-only over the whole array.
        let a_slot = ls.leaders[i].slot;
        if ls.scan_hostiles(a_slot) {
            ls.leaders[i].flags |= flag::HOSTILE_SEEN;
        }
        trace.hostile_seen[i] = ls.leaders[i].flags & flag::HOSTILE_SEEN != 0;

        // 0x006ED325 — Leader::gather.
        let e = &mut env.leaders[i];
        let g = leader_gather(
            &mut ls.leaders[i],
            econ_rules,
            frame,
            &e.gather,
            &e.caps,
            &e.payout,
        );
        trace.gross_recomputed[i] = g.gross_recomputed;
        trace.rare_mask_changed[i] = g.rare_mask_changed;
        trace.payouts[i] = g.payouts;

        // 0x006ED32A — edge-triggered on the bits Leader::gather may have just set. The
        // clear happens *before* the call, so a stat pass that re-dirties runs again next
        // frame rather than being swallowed.
        if ls.leaders[i].flags & flag::WALL_STATS_DIRTY != 0 {
            ls.leaders[i].flags &= !flag::WALL_STATS_DIRTY;
            trace.wall_pass[i] = calc_wall_stats(&mut e.objects);
            trace.wall_stats_ran[i] = true;
        }
        // 0x006ED341.
        if ls.leaders[i].flags & flag::UNIT_STATS_DIRTY != 0 {
            ls.leaders[i].flags &= !flag::UNIT_STATS_DIRTY;
            trace.unit_pass[i] =
                calc_unit_stats(&mut ls.leaders[i], rules, &e.attrition, &mut e.objects);
            trace.unit_stats_ran[i] = true;
        }

        // 0x006ED35A — Leader::process_elimination, ported in victory_score.rs.
        trace.elimination_calls.push(i);

        // 0x006ED35F..0x006ED3CE — the grace timers.
        if rules.timer_refresh_ratio != 0 && frame % rules.timer_refresh_ratio == 0 {
            let mut crept = 0u8;
            for t in ls.leaders[i].timers.iter_mut() {
                if t.value < 0 && t.frozen == 0 {
                    t.value = t.value.wrapping_add(1);
                    crept += 1;
                }
            }
            trace.timers_crept[i] = crept;
        }

        // 0x006ED3CE — the taunt scan. `frame != 0` is what keeps a zeroed table quiet on
        // the first frame.
        if frame != 0 {
            for k in 0..NUM_LEADER_SLOTS {
                if ls.leaders[i].taunt_frame[k] == frame {
                    trace.taunts.push(TauntDispatch {
                        slot: i,
                        kind: ls.leaders[i].taunt_kind[k],
                        arg: ls.leaders[i].taunt_arg[k],
                        entry: k,
                    });
                }
            }
        }

        // 0x006ED407.
        ls.leaders[i].flags &= !flag::PENDING;
    }

    trace
}

// ===========================================================================================
// A driver, so this is a thing that runs rather than a thing that compiles
// ===========================================================================================

/// Step 8 plus the two clock steps that bracket it, run for real.
///
/// `Game::do_frame` increments `Game::frame` at step 20 and `Game::seconds` at step 23,
/// both *after* step 8 (`0x005924BF` / `0x005924CF`). [`Step8Driver::frame`] therefore runs
/// step 8 against the pre-increment frame and then advances the clock, which is the whole
/// of the tick that step 8 can see. It is not `Game::do_frame` — 27 other steps are absent —
/// and it exists so the ported code above has an execution path that is not a unit test.
#[derive(Clone, Debug)]
pub struct Step8Driver {
    pub leaders: Leaders,
    pub env: Step8Env,
    pub rules: Step8Rules,
    pub econ_rules: EconRules,
    /// `Game + 0x550`.
    pub frame: i32,
    /// `Game + 0x560`.
    pub seconds: i32,
    /// Totals across every frame run, so the driver is measurable.
    pub totals: DriverTotals,
}

/// Everything [`Step8Driver`] has done since it was built.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct DriverTotals {
    pub frames: u64,
    pub leader_frames: u64,
    pub gathers: u64,
    pub gross_recomputes: u64,
    pub wall_stat_passes: u64,
    pub unit_stat_passes: u64,
    pub elimination_calls: u64,
    pub taunts: u64,
    pub timer_creeps: u64,
    pub whole_resources_credited: [i64; NUM_RESOURCES],
}

impl Default for Step8Driver {
    fn default() -> Self {
        Step8Driver::new()
    }
}

impl Step8Driver {
    pub fn new() -> Step8Driver {
        Step8Driver {
            leaders: Leaders::new(),
            env: Step8Env::default(),
            rules: Step8Rules::shipped(),
            econ_rules: EconRules::shipped(),
            frame: 0,
            seconds: 0,
            totals: DriverTotals::default(),
        }
    }

    /// One tick's worth of step 8, then steps 20 and 23.
    pub fn frame(&mut self) -> Step8Trace {
        let t = process_all(
            &mut self.leaders,
            self.frame,
            &self.rules,
            &self.econ_rules,
            &mut self.env,
        );

        self.totals.frames += 1;
        self.totals.leader_frames += t.leaders_processed() as u64;
        self.totals.gathers += t.leaders_processed() as u64;
        self.totals.gross_recomputes += t.gross_recomputed.iter().filter(|b| **b).count() as u64;
        self.totals.wall_stat_passes += t.wall_stats_ran.iter().filter(|b| **b).count() as u64;
        self.totals.unit_stat_passes += t.unit_stats_ran.iter().filter(|b| **b).count() as u64;
        self.totals.elimination_calls += t.elimination_calls.len() as u64;
        self.totals.taunts += t.taunts.len() as u64;
        self.totals.timer_creeps += t.timers_crept.iter().map(|c| *c as u64).sum::<u64>();
        for p in t.payouts.iter() {
            for r in 0..NUM_RESOURCES {
                self.totals.whole_resources_credited[r] += p[r].whole as i64;
            }
        }

        // Step 20 `0x005924BF`, then step 23 `0x005924CF`.
        self.frame = self.frame.wrapping_add(1);
        if self.frame % crate::schedule::FRAMES_PER_SECOND == 0 {
            self.seconds = self.seconds.wrapping_add(1);
        }
        t
    }

    /// Run `n` frames and return the last trace.
    pub fn run(&mut self, n: usize) -> Step8Trace {
        let mut last = Step8Trace::default();
        for _ in 0..n {
            last = self.frame();
        }
        last
    }

    /// The modelled leaders channel over the eight economy blocks.
    pub fn econ_channel(&self) -> u32 {
        self.leaders.econ_channel()
    }
}

// ===========================================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// The loop bounds are self-consistent: eight leaders, exactly.
    #[test]
    fn the_array_is_eight_leaders_of_stride_0x6eec() {
        assert_eq!(
            (LEADERS_END_VA - LEADERS_BASE_VA) / LEADER_STRIDE,
            NUM_LEADER_SLOTS as u32
        );
        assert_eq!(
            LEADERS_BASE_VA + LEADER_STRIDE * NUM_LEADER_SLOTS as u32,
            LEADERS_END_VA
        );
        // 0x00E3A7A4 is the address `Leader::process_elimination` reads for slot 0's
        // capital stamp; it must be the base plus TIMER0_VALUE.
        assert_eq!(LEADERS_BASE_VA + offsets::TIMER0_VALUE as u32, 0x00E3A7A4);
        // and 0x00E3A7AC is Game::retake_capital's per-leader scale.
        assert_eq!(LEADERS_BASE_VA + offsets::RETAKE_SCALE as u32, 0x00E3A7AC);
    }

    /// The taunt table is three arrays at `-0x20 / 0 / +0x20` off one cursor.
    #[test]
    fn taunt_table_offsets_match_the_cursor_arithmetic() {
        assert_eq!(offsets::TAUNT_ARG - offsets::TAUNT_KIND, 0x20);
        assert_eq!(offsets::TAUNT_FRAME - offsets::TAUNT_ARG, 0x20);
        assert_eq!(0x20 / 4, NUM_LEADER_SLOTS);
    }

    /// `NO_ANTI_ATTRITION` is the literal the engine `mov`s, read as f32.
    #[test]
    fn no_anti_attrition_is_the_0x43800000_literal() {
        assert_eq!(NO_ANTI_ATTRITION.to_bits(), 0x4380_0000);
    }

    /// The outer gate is bit 1. A leader with only bit 0 is scanned by everyone else's
    /// diplomacy loop and never runs its own economy.
    #[test]
    fn outer_gate_is_bit_1_and_inner_gate_is_bit_0() {
        let mut d = Step8Driver::new();
        d.leaders.leaders[0].flags |= flag::IN_GAME; // in game, not processed
        d.leaders.leaders[1].flags |= flag::PROCESS; // processed, not in game
        let t = d.frame();
        assert!(!t.processed[0]);
        assert!(t.processed[1]);
        assert_eq!(t.leaders_processed(), 1);
    }

    /// Mutual allies do not raise the hostile bit; one-sided alliance does.
    #[test]
    fn hostile_scan_needs_both_directions_to_be_allied() {
        let mut ls = Leaders::new();
        ls.leaders[0].activate();
        ls.leaders[1].activate();
        ls.leaders[1].flags |= flag::COUNTS_AS_HOSTILE;

        // Neither allied.
        assert!(ls.scan_hostiles(0));

        // Both allied -> suppressed.
        ls.leaders[0].diplo[1] = DIPLO_ALLIED;
        ls.leaders[1].diplo[0] = DIPLO_ALLIED;
        assert!(!ls.scan_hostiles(0));

        // One-sided -> back on.
        ls.leaders[0].diplo[1] = 0;
        assert!(ls.scan_hostiles(0));

        // Mutually allied again, but 1 no longer counts as hostile at all: still quiet.
        ls.leaders[0].diplo[1] = DIPLO_ALLIED;
        ls.leaders[1].flags &= !flag::COUNTS_AS_HOSTILE;
        assert!(!ls.scan_hostiles(0));
    }

    /// A leader without `COUNTS_AS_HOSTILE` never makes anyone hostile, however hostile the
    /// diplomacy is. This is the `test edx, 0x20000` at `0x006ED307`.
    #[test]
    fn hostile_scan_requires_the_0x20000_bit_on_the_other_leader() {
        let mut ls = Leaders::new();
        ls.leaders[0].activate();
        ls.leaders[1].activate();
        assert!(!ls.scan_hostiles(0));
    }

    /// `HOSTILE_SEEN` is recomputed every frame, not latched.
    #[test]
    fn hostile_bit_is_cleared_before_it_is_recomputed() {
        let mut d = Step8Driver::new();
        d.leaders.leaders[0].activate();
        d.leaders.leaders[1].activate();
        d.leaders.leaders[1].flags |= flag::COUNTS_AS_HOSTILE;
        let t = d.frame();
        assert!(t.hostile_seen[0]);

        d.leaders.leaders[1].flags &= !flag::COUNTS_AS_HOSTILE;
        let t = d.frame();
        assert!(!t.hostile_seen[0]);
        assert_eq!(d.leaders.leaders[0].flags & flag::HOSTILE_SEEN, 0);
    }

    /// The two per-frame counters are zeroed for every processed leader.
    #[test]
    fn the_two_frame_counters_are_zeroed_every_frame() {
        let mut d = Step8Driver::new();
        d.leaders.leaders[0].activate();
        d.leaders.leaders[0].frame_counter_a = 77;
        d.leaders.leaders[0].frame_counter_b = -3;
        d.leaders.leaders[1].frame_counter_a = 77; // not processed
        d.frame();
        assert_eq!(d.leaders.leaders[0].frame_counter_a, 0);
        assert_eq!(d.leaders.leaders[0].frame_counter_b, 0);
        assert_eq!(d.leaders.leaders[1].frame_counter_a, 77);
    }

    /// `PENDING` is cleared at the tail, and only for processed leaders.
    #[test]
    fn pending_bit_is_cleared_at_the_tail() {
        let mut d = Step8Driver::new();
        d.leaders.leaders[0].activate();
        d.leaders.leaders[0].flags |= flag::PENDING;
        d.leaders.leaders[1].flags |= flag::PENDING;
        d.frame();
        assert_eq!(d.leaders.leaders[0].flags & flag::PENDING, 0);
        assert_ne!(d.leaders.leaders[1].flags & flag::PENDING, 0);
    }

    // -- the piece that was missing -------------------------------------------------------

    /// The rare-mask union is the only producer of the two stat-dirty bits, and the stat
    /// passes are edge-triggered off them. This is the whole protocol in one test.
    #[test]
    fn rare_mask_change_is_what_makes_the_stat_passes_run() {
        let mut d = Step8Driver::new();
        d.leaders.leaders[0].activate();
        d.env.leaders[0].objects.band_2000 = vec![StatObject {
            active: true,
            ..Default::default()
        }];
        d.env.leaders[0].objects.units = vec![StatObject {
            active: true,
            stats_stale: true,
            ..Default::default()
        }];

        // Frame 0: masks are all empty, the union changes nothing, nothing runs.
        let t = d.frame();
        assert!(!t.rare_mask_changed[0]);
        assert!(!t.wall_stats_ran[0]);
        assert!(!t.unit_stats_ran[0]);

        // Acquire a rare: the union changes, both bits set, both passes run this frame.
        d.leaders.leaders[0].rare_b = RareMask::from_bits(&[3]);
        let t = d.frame();
        assert!(t.rare_mask_changed[0]);
        assert!(t.wall_stats_ran[0]);
        assert!(t.unit_stats_ran[0]);
        assert_eq!(t.wall_pass[0].active, 1);
        assert_eq!(t.unit_pass[0].speed_updates, 1);
        assert_eq!(t.unit_pass[0].armor_updates, 1);

        // Steady state: no change, no passes. Edge-triggered, not level-triggered.
        let t = d.frame();
        assert!(!t.rare_mask_changed[0]);
        assert!(!t.wall_stats_ran[0]);
        assert!(!t.unit_stats_ran[0]);

        // The effective mask is the union of both operands.
        d.leaders.leaders[0].rare_a = RareMask::from_bits(&[9]);
        d.frame();
        assert!(d.leaders.leaders[0].rare_effective.get(3));
        assert!(d.leaders.leaders[0].rare_effective.get(9));
    }

    /// An externally-set dirty bit is honoured and consumed, which is how any other
    /// subsystem asks for a stat pass.
    #[test]
    fn externally_set_dirty_bits_are_consumed_once() {
        let mut d = Step8Driver::new();
        d.leaders.leaders[0].activate();
        d.leaders.leaders[0].flags |= flag::STATS_DIRTY;
        let t = d.frame();
        assert!(t.wall_stats_ran[0] && t.unit_stats_ran[0]);
        assert_eq!(d.leaders.leaders[0].flags & flag::STATS_DIRTY, 0);
        let t = d.frame();
        assert!(!t.wall_stats_ran[0] && !t.unit_stats_ran[0]);
    }

    // -- the timers -----------------------------------------------------------------------

    /// Negative timers creep toward zero on the refresh beat, frozen ones do not, and a
    /// zero ratio disables the block entirely.
    #[test]
    fn grace_timers_creep_only_on_the_refresh_beat() {
        let mut d = Step8Driver::new();
        d.leaders.leaders[0].activate();
        d.leaders.leaders[0].timers[0] = GraceTimer {
            value: -10,
            frozen: 0,
        };
        d.leaders.leaders[0].timers[1] = GraceTimer {
            value: -10,
            frozen: 1,
        };
        d.leaders.leaders[0].timers[2] = GraceTimer {
            value: 4,
            frozen: 0,
        };

        // 10 frames at ratio 5 -> frames 0 and 5 are on the beat.
        d.run(10);
        assert_eq!(d.leaders.leaders[0].timers[0].value, -8);
        assert_eq!(d.leaders.leaders[0].timers[1].value, -10, "frozen");
        assert_eq!(d.leaders.leaders[0].timers[2].value, 4, "non-negative");
        assert_eq!(d.totals.timer_creeps, 2);

        // Ratio 0 is the `je` at 0x006ED372: the block never runs.
        let mut d = Step8Driver::new();
        d.rules.timer_refresh_ratio = 0;
        d.leaders.leaders[0].activate();
        d.leaders.leaders[0].timers[0] = GraceTimer {
            value: -10,
            frozen: 0,
        };
        d.run(100);
        assert_eq!(d.leaders.leaders[0].timers[0].value, -10);
        assert_eq!(d.totals.timer_creeps, 0);
    }

    /// A timer never overshoots zero.
    #[test]
    fn grace_timer_stops_at_zero() {
        let mut d = Step8Driver::new();
        d.leaders.leaders[0].activate();
        d.leaders.leaders[0].timers[0] = GraceTimer {
            value: -2,
            frozen: 0,
        };
        d.run(600);
        assert_eq!(d.leaders.leaders[0].timers[0].value, 0);
    }

    // -- taunts ---------------------------------------------------------------------------

    /// The scan is gated on `frame != 0`, so a zeroed table is quiet on frame 0 and fires
    /// exactly once when a stamp matches.
    #[test]
    fn taunt_scan_skips_frame_zero_and_fires_on_the_stamp() {
        let mut d = Step8Driver::new();
        d.leaders.leaders[0].activate();
        // A zeroed table would otherwise match frame 0 in all eight entries.
        let t = d.frame();
        assert!(t.taunts.is_empty());

        d.leaders.leaders[0].taunt_frame[2] = 4;
        d.leaders.leaders[0].taunt_kind[2] = 11;
        d.leaders.leaders[0].taunt_arg[2] = 22;
        let mut fired = Vec::new();
        for _ in 0..10 {
            fired.extend(d.frame().taunts);
        }
        assert_eq!(fired.len(), 1);
        assert_eq!(
            fired[0],
            TauntDispatch {
                slot: 0,
                kind: 11,
                arg: 22,
                entry: 2
            }
        );
    }

    /// All eight entries are scanned, in table order.
    #[test]
    fn every_taunt_entry_is_scanned() {
        let mut d = Step8Driver::new();
        d.leaders.leaders[0].activate();
        for k in 0..NUM_LEADER_SLOTS {
            d.leaders.leaders[0].taunt_frame[k] = 3;
            d.leaders.leaders[0].taunt_kind[k] = k as i32;
        }
        d.run(3);
        let t = d.frame(); // frame 3
        assert_eq!(t.taunts.len(), NUM_LEADER_SLOTS);
        for (k, td) in t.taunts.iter().enumerate() {
            assert_eq!(td.entry, k);
            assert_eq!(td.kind, k as i32);
        }
    }

    // -- attrition ------------------------------------------------------------------------

    /// With no multiplier the value is a straight `ATTRITION_IMPROVED` table read, so these
    /// are the shipped constants and not arithmetic.
    #[test]
    fn attrition_is_a_table_read_of_the_consecutive_preq_run() {
        let r = Step8Rules::shipped();
        let mut l = Leader::new(0);
        let mut g = AttritionGates::default();

        calc_attrition(&mut l, &r, &g);
        assert_eq!(l.attrition, 0, "no preqs, no attrition");

        for (n, want) in [(1usize, 1), (2, 2), (3, 4), (4, 8)] {
            g.attrition_preqs = [false; 4];
            for i in 0..n {
                g.attrition_preqs[i] = true;
            }
            calc_attrition(&mut l, &r, &g);
            assert_eq!(l.attrition, want, "run of {n}");
        }

        // The run stops at the first miss: 1, _, 1, 1 counts as one.
        g.attrition_preqs = [true, false, true, true];
        calc_attrition(&mut l, &r, &g);
        assert_eq!(l.attrition, r.attrition_improved[0]);
    }

    /// `leader + 0x7F8` forces zero regardless of everything else.
    #[test]
    fn attrition_off_wins() {
        let r = Step8Rules::shipped();
        let mut l = Leader::new(0);
        l.attrition_off = 1;
        let g = AttritionGates {
            attrition_preqs: [true; 4],
            colosseum: true,
            kremlin: true,
            ..Default::default()
        };
        calc_attrition(&mut l, &r, &g);
        assert_eq!(l.attrition, 0);
    }

    /// The `cmove` floor fires on the *product*, so a multiplier applied to zero raises it
    /// to one. That is the behaviour at `0x006CDF2B` and it is easy to lose.
    #[test]
    fn a_multiplier_over_zero_attrition_floors_to_one() {
        let r = Step8Rules::shipped();
        let mut l = Leader::new(0);
        let g = AttritionGates {
            colosseum: true,
            ..Default::default()
        };
        calc_attrition(&mut l, &r, &g);
        assert_eq!(l.attrition, 1);
    }

    /// Each gate is skipped whole when false, so multipliers compose monotonically.
    #[test]
    fn attrition_multipliers_compose_and_never_shrink() {
        let r = Step8Rules::shipped();
        let base_gates = AttritionGates {
            attrition_preqs: [true, true, true, true],
            ..Default::default()
        };
        let mut l = Leader::new(0);
        calc_attrition(&mut l, &r, &base_gates);
        let base = l.attrition;

        for g in [
            AttritionGates {
                colosseum: true,
                ..base_gates
            },
            AttritionGates {
                russian: true,
                ..base_gates
            },
            AttritionGates {
                kremlin: true,
                ..base_gates
            },
        ] {
            let mut l = Leader::new(0);
            calc_attrition(&mut l, &r, &g);
            assert!(l.attrition >= base, "a positive rule must not shrink it");
        }

        // CTW needs both the mode and the leader byte.
        let g = AttritionGates {
            conquest_mode: true,
            ..base_gates
        };
        let mut l = Leader::new(0);
        calc_attrition(&mut l, &r, &g);
        assert_eq!(l.attrition, base, "conquest byte clear -> no bonus");
        let mut l = Leader::new(0);
        l.conquest_byte = 1;
        calc_attrition(&mut l, &r, &g);
        assert!(l.attrition > base);
    }

    /// Anti-attrition starts at 256.0 and each source strictly raises it, until a rule of
    /// 100 collapses it to zero.
    #[test]
    fn anti_attrition_is_f32_and_liberty_is_a_hard_zero() {
        let r = Step8Rules::shipped();
        let mut l = Leader::new(0);
        let mut g = AttritionGates::default();

        calc_anti_attrition(&mut l, &r, &g);
        assert_eq!(l.anti_attrition, NO_ANTI_ATTRITION);

        // The best upgrade wins: 0x300 -> ATTRITION_UPGRADE[2] = 75.
        g.anti_preqs = [true, true, true];
        calc_anti_attrition(&mut l, &r, &g);
        let best = l.anti_attrition;
        g.anti_preqs = [true, false, false];
        calc_anti_attrition(&mut l, &r, &g);
        assert!(best > l.anti_attrition, "0x300 must beat 0x2FE");

        // LIBERTY_ATTRITION ships at 100, so it takes the >= 100 exit.
        g = AttritionGates {
            liberty: true,
            ..Default::default()
        };
        calc_anti_attrition(&mut l, &r, &g);
        assert_eq!(l.anti_attrition, 0.0);

        // `leader + 0x7FC` is the same hard zero.
        l.anti_attrition_off = 1;
        calc_anti_attrition(&mut l, &r, &AttritionGates::default());
        assert_eq!(l.anti_attrition, 0.0);
    }

    /// The titanium source is a bit in either mask, not a query.
    #[test]
    fn titanium_rare_reduces_attrition_from_either_mask() {
        let r = Step8Rules::shipped();
        let g = AttritionGates::default();

        let mut l = Leader::new(0);
        l.rare_b = RareMask::from_bits(&[RareMask::TITANIUM]);
        calc_anti_attrition(&mut l, &r, &g);
        let from_b = l.anti_attrition;

        let mut l = Leader::new(0);
        l.rare_effective = RareMask::from_bits(&[RareMask::TITANIUM]);
        calc_anti_attrition(&mut l, &r, &g);
        assert_eq!(from_b, l.anti_attrition);
        assert!(from_b > NO_ANTI_ATTRITION);
    }

    /// Bit 30 is byte 3 mask 0x40 — the literal `test byte [.. + 3], 0x40` the engine emits.
    #[test]
    fn titanium_bit_is_byte_three_mask_0x40() {
        let m = RareMask::from_bits(&[RareMask::TITANIUM]);
        assert_eq!(m.bytes[3], 0x40);
        assert_eq!(RARE_MASK_BYTES * 8 - 4, RareMask::BITS);
    }

    // -- the economy actually running -----------------------------------------------------

    /// The one that matters: eight leaders, three thousand frames, real income landing in
    /// real stockpiles through the real `economy` chain. Numbers here are *observed*, never
    /// hand-computed; the assertions are on structure and on the engine's own schedule.
    #[test]
    fn eight_leaders_run_three_thousand_frames_and_income_lands() {
        let mut d = Step8Driver::new();
        for i in 0..NUM_LEADER_SLOTS {
            d.leaders.leaders[i].activate();
            // A town's worth of income, in the sixteenths-per-GATHER_RATE units
            // `calc_gather` produces. Supplied as `object_income`, which is exactly what
            // the four unported object-graph loops would have summed.
            d.env.leaders[i].gather.object_income = [4000, 3000, 2000, 1000, 500, 0];
            d.env.leaders[i].caps.wonder_additive = [0; NUM_RESOURCES];
        }

        d.run(3000);

        assert_eq!(d.totals.frames, 3000);
        assert_eq!(d.totals.leader_frames, 3000 * 8);
        assert_eq!(d.totals.gathers, 3000 * 8);
        assert_eq!(d.frame, 3000);
        assert_eq!(d.seconds, 200, "15 sim frames is one game second");

        // Income only exists after the first gross recomposition, which is phase-offset
        // per leader: `(frame + slot*8) % 256 == 0` with a 300-frame floor.
        assert!(d.totals.gross_recomputes > 0);
        for i in 0..NUM_LEADER_SLOTS {
            let e = &d.leaders.leaders[i].econ;
            assert!(e.stockpile[0] > 0, "leader {i} gathered no food");
            assert!(e.stockpile[4] > 0, "leader {i} gathered no metal");
            assert_eq!(e.stockpile[5], 0, "no oil income was supplied");
        }
        // Knowledge has the hardcoded 999 cap, so it is not clamped by commerce_cap.
        assert!(d.totals.whole_resources_credited[3] > 0);

        // Every leader saw the same inputs and the same schedule offset by slot*8, so the
        // eight stockpiles must be close but not necessarily equal.
        let food: Vec<i32> = (0..NUM_LEADER_SLOTS)
            .map(|i| d.leaders.leaders[i].econ.stockpile[0])
            .collect();
        let lo = *food.iter().min().unwrap();
        let hi = *food.iter().max().unwrap();
        assert!(
            hi - lo <= hi / 4,
            "phase offset should be a small effect: {food:?}"
        );
    }

    /// **Measured, by running it:** the clean-path gross recomputation lands every
    /// **512** frames, not every 256.
    ///
    /// `economy::calc_gather_due`'s clean path needs both `frame >= last + 300` and
    /// `(frame + slot*8) % 256 == 0`. The beats are 256 apart and the floor is 300, so the
    /// beat immediately after a recompute is always suppressed and the next one always
    /// fires. The period is therefore 2×256, and the phase is `-slot*8`. At 15 frames per
    /// game second that is one recomputation per leader per **34.1 s**, which is the real
    /// bound on how stale a leader's gross income can be — twice the 256 the lane brief
    /// carried.
    #[test]
    fn gross_income_actually_lags_512_frames_not_256() {
        for slot in 0..NUM_LEADER_SLOTS {
            let mut d = Step8Driver::new();
            d.leaders.leaders[slot].activate();
            d.env.leaders[slot].gather.object_income = [16, 0, 0, 0, 0, 0];

            let mut fired = Vec::new();
            for _ in 0..4096 {
                let f = d.frame;
                if d.frame().gross_recomputed[slot] {
                    fired.push(f);
                }
            }
            assert!(fired.len() > 4, "slot {slot} barely recomputed: {fired:?}");
            let gaps: Vec<i32> = fired.windows(2).map(|w| w[1] - w[0]).collect();
            assert!(
                gaps.iter().all(|g| *g == 512),
                "slot {slot} steady-state period was not 512: {gaps:?}"
            );
            // The phase is the engine's own `-slot*8`.
            assert_eq!(
                (fired[1] + slot as i32 * 8) % 256,
                0,
                "slot {slot} fired off its phase"
            );
        }
    }

    /// The dirty path is the fast one: every 8 frames, phase `-slot`.
    #[test]
    fn a_dirty_economy_recomputes_every_eight_frames() {
        let mut d = Step8Driver::new();
        d.leaders.leaders[0].activate();
        d.env.leaders[0].gather.object_income = [16, 0, 0, 0, 0, 0];
        let mut fired = Vec::new();
        for _ in 0..200 {
            // Something changed again this frame; retail re-arms 0x2000000 the same way.
            d.leaders.leaders[0].econ_dirty = true;
            let f = d.frame;
            if d.frame().gross_recomputed[0] {
                fired.push(f);
            }
        }
        let gaps: Vec<i32> = fired.windows(2).map(|w| w[1] - w[0]).collect();
        assert!(gaps.iter().all(|g| *g == 8), "{gaps:?}");
    }

    /// Determinism: the same inputs produce the same channel, twice.
    #[test]
    fn the_run_is_deterministic() {
        fn build() -> Step8Driver {
            let mut d = Step8Driver::new();
            for i in 0..NUM_LEADER_SLOTS {
                d.leaders.leaders[i].activate();
                d.leaders.leaders[i].econ.age = (i % 4) as i32;
                d.env.leaders[i].gather.object_income =
                    [1000 + 100 * i as i32, 900, 800, 700, 600, 500];
                d.leaders.leaders[i].taunt_frame[i] = 100 + i as i32;
            }
            d
        }
        let mut a = build();
        let mut b = build();
        a.run(1200);
        b.run(1200);
        assert_eq!(a.econ_channel(), b.econ_channel());
        assert_eq!(a.totals, b.totals);
        assert_eq!(a.leaders.leaders[3].econ, b.leaders.leaders[3].econ);
    }

    /// The channel moves as the economy moves. A checksum that never changes is the
    /// trivially-passing failure mode this project keeps finding.
    #[test]
    fn the_modelled_channel_is_not_constant() {
        let mut d = Step8Driver::new();
        for i in 0..NUM_LEADER_SLOTS {
            d.leaders.leaders[i].activate();
            d.env.leaders[i].gather.object_income = [2000, 0, 0, 0, 0, 0];
        }
        let start = d.econ_channel();
        d.run(1000);
        let end = d.econ_channel();
        assert_ne!(start, end, "1000 frames of income must move the channel");
    }

    /// Step 8 must see the *pre-increment* frame. If it saw the post-increment one every
    /// gather phase and every taunt stamp would be off by one, so pin it.
    #[test]
    fn step_eight_sees_the_pre_increment_frame() {
        let mut d = Step8Driver::new();
        d.leaders.leaders[0].activate();
        d.leaders.leaders[0].taunt_frame[0] = 0;
        // frame 0: the `frame != 0` gate suppresses it.
        assert!(d.frame().taunts.is_empty());
        assert_eq!(d.frame, 1);
        // A stamp of 1 fires on the second call, when `frame` is still 1.
        d.leaders.leaders[0].taunt_frame[0] = 1;
        assert_eq!(d.frame().taunts.len(), 1);
        assert_eq!(d.frame, 2);
    }

    /// Elimination is recorded, in order, for exactly the processed leaders — the contract
    /// a scheduler needs to drive `victory_score::Leaders::process_elimination` from here.
    #[test]
    fn elimination_calls_are_recorded_in_retail_order() {
        let mut d = Step8Driver::new();
        d.leaders.leaders[1].activate();
        d.leaders.leaders[5].activate();
        d.leaders.leaders[6].flags |= flag::IN_GAME;
        let t = d.frame();
        assert_eq!(t.elimination_calls, vec![1, 5]);
    }

    /// `Step8Rules::from_block` reads the same offsets the disassembly does.
    #[test]
    fn rules_load_from_a_value_block_by_offset() {
        let mut block = [0i32; 848];
        block[rule_offsets::TIMER_REFRESH_RATIO / 4] = 5;
        block[rule_offsets::ATTRITION_IMPROVED / 4] = 1;
        block[rule_offsets::ATTRITION_IMPROVED / 4 + 3] = 8;
        block[rule_offsets::TITANIUM_ATTRITION / 4] = 50;
        let r = Step8Rules::from_block(&block);
        assert_eq!(r.timer_refresh_ratio, 5);
        assert_eq!(r.attrition_improved[0], 1);
        assert_eq!(r.attrition_improved[3], 8);
        assert_eq!(r.titanium_attrition, 50);
        // A short block zero-extends rather than panicking.
        assert_eq!(Step8Rules::from_block(&[]).timer_refresh_ratio, 0);
    }

    /// The shipped table and the loader's own values agree — this is the guard against a
    /// typo in `Step8Rules::shipped`.
    #[test]
    fn shipped_rules_round_trip_through_a_block() {
        let s = Step8Rules::shipped();
        let mut block = [0i32; 848];
        block[rule_offsets::TIMER_REFRESH_RATIO / 4] = s.timer_refresh_ratio;
        for i in 0..4 {
            block[rule_offsets::ATTRITION_IMPROVED / 4 + i] = s.attrition_improved[i];
            block[rule_offsets::ATTRITION_UPGRADE / 4 + i] = s.attrition_upgrade[i];
        }
        block[rule_offsets::COLOSSEUM_ATTRITION / 4] = s.colosseum_attrition;
        block[rule_offsets::KREMLIN_ATTRITION / 4] = s.kremlin_attrition;
        block[rule_offsets::LIBERTY_ATTRITION / 4] = s.liberty_attrition;
        block[rule_offsets::RUSSIAN_ATTRITION / 4] = s.russian_attrition;
        block[rule_offsets::MONGOL_ATTRITION / 4] = s.mongol_attrition;
        block[rule_offsets::TITANIUM_ATTRITION / 4] = s.titanium_attrition;
        block[rule_offsets::CTW_ATTRITION / 4] = s.ctw_attrition;
        assert_eq!(Step8Rules::from_block(&block), s);
    }
}
