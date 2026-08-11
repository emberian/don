//! Leader-owned tick dispatchers: step 8 `Leaders::process_all` `0x006ED2A0`, step 11
//! `Leaders::strategy_all` `0x006ED430`, step 17 `Leaders::end_process_all` `0x006ED070`,
//! and step 19 `Leader::process_event_frame` `0x006EC180`.
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
//!        leader.pop_issues = 0 ; leader[0x9F4] = 0 ; production failures + counter
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
//!  +- [11] Leaders::strategy_all    0x006ED430   <- [`strategy_all`], this file
//!      for slot in 0..8, flags & 3 == 3:
//!        Leader::check_explore      0x006BC860   -> [`check_explore`]
//!        Leader::plan_strategy      0x006B9620   ; AI body remains explicit
//!        Leader::compute_score(0)   0x006EC560   ; existing victory-score port
//!        Leader::diplomacy          0x006BC950   ; AI body remains explicit
//!      if Game semaphore bit 9:
//!        Game::check_victory        0x005926B0   ; existing victory-score port
//!  +- [17] Leaders::end_process_all 0x006ED070   <- [`end_process_all`], this file
//!      for slot in 0..8, flags & 2:
//!        if pop_issues == 0: clear matching Player warning flags
//!        else if local player: rate-limit and emit population-cap feedback
//!  +- [19] Leader::process_event_frame 0x006EC180 <- [`process_event_frames`], this file
//!      for slot in 0..8, flags & 1:
//!        every 50 frames: fold event counters into rates and 15-second totals
//!        choose local combat music; emit lopsided-battle achievement event
//! ```
//!
//! Everything above is [measured] from a capstone disassembly of `0x006ED2A0`,
//! `0x006CE280`, `0x006CF7C0`, `0x006CF970`, `0x006CDEA0`, `0x006CDCC0`, `0x006B8A20`,
//! `0x006ED430`, `0x006BC860`, `0x006ED070`, and `0x006EC180` against
//! `ron-bin/riseofnations.exe` (sha256
//! `30478a44…625079`), cross-read against `re/decomp-all/`. **Tier C**: structure and
//! constants are read from the binary, nothing here has been executed against retail, and
//! no claim may be promoted without an oracle run.
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
//! * The four virtual slots are now resolved from the retail vtables. `+0x4C` is
//!   `WallData::is_active`, `+0xE8` is `UnitData::is_captain`, and the base implementations
//!   at `+0x15C/+0x160` are `Object::update_hits/update_los`. The building band's complete
//!   `Wall::update_hits/update_los` overrides and `Unit::update_speed` now execute. Unit
//!   speed/armor packages are rebuilt from an explicitly installed, provenance-bound local
//!   post-load type source plus live leader/object state. The retail-derived table is not
//!   compiled into or conveyed by `don-sim`; an absent source and missing type identities remain
//!   explicit misses. `Wall::update_construct_time` `0x0063D560` now executes its complete
//!   545-byte body, with `LeaderData::get_building_speed_upgrade` `0x006DAE90` ported beside
//!   it, whenever its type package is supplied. Wall query population and reached
//!   `Object::eject_contents` remain explicit.
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
use crate::systems::leaders_process_event_frame_step19 as step19;

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
    /// `0x0063D72D` — `LeaderData::city_num`. `Wall::update_construct_time` applies
    /// `CAPITAL_BUILD_TIME` while it is zero.
    pub const CITY_NUM: usize = 0x3F8;
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
    /// `0x006ED213` — current population cap, compared with the selected match limit.
    pub const POP_CAP: usize = 0x7E4;
    /// `0x006ED0AA` — per-frame failed population-production count. Step 8 clears it;
    /// object processing can raise it before step 17 reads it.
    pub const POP_ISSUES: usize = 0x7E8;
    /// `0x006CDCCE` — **f32**, the anti-attrition scale, 256.0 = none.
    pub const ANTI_ATTRITION: usize = 0x7F4;
    /// `0x006BC8B6` / `0x006BC8CB` / `0x006BC91B` — the number of explored region
    /// cells rebuilt by `Leader::check_explore`.
    pub const EXPLORED: usize = 0x9D4;
    /// `0x006CDEA6` — non-zero forces attrition to 0.
    pub const ATTRITION_OFF: usize = 0x7F8;
    /// `0x006CDCC7` — non-zero forces anti-attrition to 0.0.
    pub const ANTI_ATTRITION_OFF: usize = 0x7FC;
    /// `0x006ED2D4` — likewise.
    pub const FRAME_COUNTER_B: usize = 0x9F4;
    /// `0x006EC485` — prior lopsided-battle event frame.
    pub const FRAME_BATTLE: usize = 0xA4C;
    pub const AVERAGE_DEATH_RATE: usize = 0xA50;
    pub const AVERAGE_KILL_RATE: usize = 0xA52;
    pub const AVERAGE_DAMAGE_RATE: usize = 0xA54;
    pub const AVERAGE_HIT_RATE: usize = 0xA56;
    pub const DEATHS_CURRENT_FRAME: usize = 0xA58;
    pub const KILLS_CURRENT_FRAME: usize = 0xA5A;
    pub const HITS_CURRENT_FRAME: usize = 0xA5C;
    pub const DAMAGE_CURRENT_FRAME: usize = 0xA5E;
    pub const DEATHS_FIFTEEN_SECONDS: usize = 0xA60;
    pub const KILLS_FIFTEEN_SECONDS: usize = 0xA62;
    pub const HITS_FIFTEEN_SECONDS: usize = 0xA64;
    pub const DAMAGE_FIFTEEN_SECONDS: usize = 0xA66;
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

    /// Rare bit 13 — tested by `Wall::update_construct_time` as `byte[1] & 0x20` on both
    /// the effective mask (`Leader + 0x6DA5`, `0x0063D614`) and mask B (`+0x6DCD`,
    /// `0x0063D623`). The rule it gates is `TOBACCO_BUILDING_SPEED`.
    pub const TOBACCO: usize = 13;

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
    /// `RULES + 0x04` = 4, `UNIT_MOVE_SPEED`, shipped 1. Master multiplier applied near
    /// the top of `Unit::update_speed`.
    pub unit_move_speed: i32,
    /// `RULES + 0x3C` = 60, `MILITARY_TRANSPORT_BONUS`, shipped 0. Added per Military
    /// library tech to domain-1 types carrying `unit_flags & 0x10`.
    pub military_transport_bonus: i32,
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
    /// `RULES + 0x960` = 2400, `CATTLE_CITIZEN_ARMOR`, shipped 1. Added by
    /// `Unit::update_armor` to type families `0x32/0x33` while rare bit 31 is held.
    pub cattle_citizen_armor: i32,
    /// `RULES + 0x8B8` = 2232, `DUTCH_ATTACK_BONUS`, shipped 1. Despite the historical
    /// name, `ObjectData::armor` also adds it once per age to qualifying Dutch units.
    pub dutch_attack_bonus: i32,
    /// `RULES + 0x4E0` = 1248, `VERSAILLES_UNITS_MOVE`, shipped 25.
    pub versailles_units_move: i32,
    /// `RULES + 0x574` = 1396, PDB member `aztec_move_speed`. The field is absent from
    /// shipped `rules.xml` and from all scalar stores in `Constants::init`, so static-zero
    /// initialization leaves it at 0; captured/modded blocks can still populate it.
    pub aztec_move_speed: i32,
    /// `RULES + 0x5B4` = 1460, `BANTU_UNITS_MOVE`, shipped 25.
    pub bantu_units_move: i32,
    /// `RULES + 0x6BC` = 1724, `FRENCH_SIEGE_MOVE`, shipped 20.
    pub french_siege_move: i32,
    /// `RULES + 0x86C` = 2156, `AMERICANS_MARINE_SPEED_BONUS`, shipped 2.
    pub americans_marine_speed_bonus: i32,
    /// `RULES + 0x938` = 2360, `ALUMINUM_AIR_SPEED`, shipped 25.
    pub aluminum_air_speed: i32,
    /// `RULES + 0x93C` = 2364, `WHALES_SHIPS_MOVE`, shipped 20.
    pub whales_ships_move: i32,
    /// `RULES + 0x154` = 340, `BUILDING_HP_UPGRADE`, shipped 10.
    pub building_hp_upgrade: i32,
    /// The five dwords beginning at `RULES + 0x154` selected by the capital-building arm
    /// of `Wall::update_hits`: `BUILDING_HP_UPGRADE` followed by the first four
    /// `TEMPLE_UPGRADE_HP` entries.
    pub capital_building_hp: [i32; 5],
    /// `RULES + 0x58C` = 1420, `MAYA_BUILDING_HP`, shipped 25.
    pub maya_building_hp: i32,
    /// `RULES + 0x5FC` = 1532, `ROMAN_FORT_HP`, shipped 0.
    pub roman_fort_hp: i32,
    /// `RULES + 0x4F8` = 1272, `TAJ_BUILDING_HP`, shipped 100.
    pub taj_building_hp: i32,
    /// `RULES + 0x4C8` = 1224, `RED_FORT_FORT_HPS`, shipped 33.
    pub red_fort_fort_hps: i32,
    /// `RULES + 0x5CC` = 1484, `NUBIAN_HIT_POINTS`, shipped 50.
    pub nubian_hit_points: i32,
    /// `RULES + 0x4A0` = 1184, `TIKAL_TEMPLE_HP`, shipped 50.
    pub tikal_temple_hp: i32,
    /// `RULES + 0xB08` = 2824, `CTW_MISSIONARIES_BONUS`, shipped 25. Despite the name,
    /// `Wall::update_hits` uses it for the CTW rare-0x20 capital branch.
    pub ctw_missionaries_bonus: i32,
    /// `RULES + 0xC48` = 3144, `SENATE_HP_BONUS`, shipped 35.
    pub senate_hp_bonus: i32,
    /// `RULES + 0x180` = 384, `FORT_UPGRADE_RANGE[5]`, used as signed bytes by
    /// `Wall::update_los`.
    pub fort_upgrade_range: [i32; 5],
    /// `RULES + 0x1A8` = 424, `TOWER_FORT_RANGE[4]`, likewise read as signed bytes.
    pub tower_fort_range: [i32; 4],
    /// `RULES + 0x474` = 1140, `COLOSSEUM_FORT_RANGE`, shipped 0.
    pub colosseum_fort_range: i32,
    /// `RULES + 0x914` = 2324, `FURS_LOS`, shipped 0.
    pub furs_los: i32,

    // -- Wall::update_construct_time 0x0063D560 -------------------------------------------
    // Every one of the seven is a divisor-or-multiplier of construction *time*, so a
    // larger value makes a building go up faster, not slower. The XML descriptions
    // captured beside each stored value say so in words.
    /// `RULES + 0x590` = 1424, `MAYA_BUILDING_SPEED`, shipped 20 ("20% bonus")
    /// [`0x0063D5A1`].
    pub maya_building_speed: i32,
    /// `RULES + 0x4E4` = 1252, `VERSAILLES_BUILDING_SPEED`, shipped 0 ("0% faster")
    /// [`0x0063D5FA`].
    pub versailles_building_speed: i32,
    /// `RULES + 0x90C` = 2316, `TOBACCO_BUILDING_SPEED`, shipped 10 ("10%")
    /// [`0x0063D633`].
    pub tobacco_building_speed: i32,
    /// `RULES + 0x6E4` = 1764, `BRITISH_AA_SPEED`, shipped 33 ("33% faster creation")
    /// [`0x0063D67B`].
    pub british_aa_speed: i32,
    /// `RULES + 0x8B4` = 2228, `DUTCH_FORT_SPEED`, shipped 0 ("0% faster") [`0x0063D6C7`].
    pub dutch_fort_speed: i32,
    /// `RULES + 0x610` = 1552, `ROMAN_FORT_SPEED`, shipped 50 ("50% faster")
    /// [`0x0063D713`].
    pub roman_fort_speed: i32,
    /// `RULES + 0x3BC` = 956, `CAPITAL_BUILD_TIME`, shipped 300 ("300% of normal city
    /// build time (this is for nomad games only)") [`0x0063D73B`]. Unlike the six above it
    /// is a plain `v * RULE / 100` multiplier, and the emitted code applies it to **every**
    /// building of a leader whose `city_num` is zero, with no type gate.
    pub capital_build_time: i32,
}

/// Byte offsets of every [`Step8Rules`] field, so [`Step8Rules::from_block`] and a
/// disassembly listing can be diffed by eye.
pub mod rule_offsets {
    pub const UNIT_MOVE_SPEED: usize = 4;
    pub const MILITARY_TRANSPORT_BONUS: usize = 60;
    pub const TIMER_REFRESH_RATIO: usize = 3328;
    pub const ATTRITION_UPGRADE: usize = 456;
    pub const ATTRITION_IMPROVED: usize = 472;
    pub const COLOSSEUM_ATTRITION: usize = 1136;
    pub const KREMLIN_ATTRITION: usize = 1296;
    pub const LIBERTY_ATTRITION: usize = 1316;
    pub const RUSSIAN_ATTRITION: usize = 1868;
    pub const MONGOL_ATTRITION: usize = 2032;
    pub const DUTCH_ATTACK_BONUS: usize = 2232;
    pub const VERSAILLES_UNITS_MOVE: usize = 1248;
    pub const AZTEC_MOVE_SPEED: usize = 1396;
    pub const BANTU_UNITS_MOVE: usize = 1460;
    pub const FRENCH_SIEGE_MOVE: usize = 1724;
    pub const AMERICANS_MARINE_SPEED_BONUS: usize = 2156;
    pub const ALUMINUM_AIR_SPEED: usize = 2360;
    pub const WHALES_SHIPS_MOVE: usize = 2364;
    pub const BUILDING_HP_UPGRADE: usize = 340;
    pub const CAPITAL_BUILDING_HP: usize = 340;
    pub const FORT_UPGRADE_RANGE: usize = 384;
    pub const TOWER_FORT_RANGE: usize = 424;
    pub const COLOSSEUM_FORT_RANGE: usize = 1140;
    pub const TIKAL_TEMPLE_HP: usize = 1184;
    pub const RED_FORT_FORT_HPS: usize = 1224;
    pub const TAJ_BUILDING_HP: usize = 1272;
    pub const MAYA_BUILDING_HP: usize = 1420;
    pub const NUBIAN_HIT_POINTS: usize = 1484;
    pub const ROMAN_FORT_HP: usize = 1532;
    pub const FURS_LOS: usize = 2324;
    pub const CTW_MISSIONARIES_BONUS: usize = 2824;
    pub const SENATE_HP_BONUS: usize = 3144;
    pub const TITANIUM_ATTRITION: usize = 2396;
    pub const CATTLE_CITIZEN_ARMOR: usize = 2400;
    pub const CTW_ATTRITION: usize = 2572;
    pub const CAPITAL_BUILD_TIME: usize = 956;
    pub const VERSAILLES_BUILDING_SPEED: usize = 1252;
    pub const MAYA_BUILDING_SPEED: usize = 1424;
    pub const ROMAN_FORT_SPEED: usize = 1552;
    pub const BRITISH_AA_SPEED: usize = 1764;
    pub const DUTCH_FORT_SPEED: usize = 2228;
    pub const TOBACCO_BUILDING_SPEED: usize = 2316;
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
            unit_move_speed: 1,
            military_transport_bonus: 0,
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
            cattle_citizen_armor: 1,
            dutch_attack_bonus: 1,
            versailles_units_move: 25,
            aztec_move_speed: 0,
            bantu_units_move: 25,
            french_siege_move: 20,
            americans_marine_speed_bonus: 2,
            aluminum_air_speed: 25,
            whales_ships_move: 20,
            building_hp_upgrade: 10,
            capital_building_hp: [10, 25, 50, 100, 150],
            maya_building_hp: 25,
            roman_fort_hp: 0,
            taj_building_hp: 100,
            red_fort_fort_hps: 33,
            nubian_hit_points: 50,
            tikal_temple_hp: 50,
            ctw_missionaries_bonus: 25,
            senate_hp_bonus: 35,
            fort_upgrade_range: [0, 1, 2, 3, 4],
            tower_fort_range: [0, 1, 2, 3],
            colosseum_fort_range: 0,
            furs_los: 0,
            maya_building_speed: 20,
            versailles_building_speed: 0,
            tobacco_building_speed: 10,
            british_aa_speed: 33,
            dutch_fort_speed: 0,
            roman_fort_speed: 50,
            capital_build_time: 300,
        }
    }

    /// All zeros — a state the engine is never in, since `timer_refresh_ratio` is a
    /// divisor. For tests that want the timer block provably off.
    pub const fn zeroed() -> Step8Rules {
        Step8Rules {
            unit_move_speed: 0,
            military_transport_bonus: 0,
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
            cattle_citizen_armor: 0,
            dutch_attack_bonus: 0,
            versailles_units_move: 0,
            aztec_move_speed: 0,
            bantu_units_move: 0,
            french_siege_move: 0,
            americans_marine_speed_bonus: 0,
            aluminum_air_speed: 0,
            whales_ships_move: 0,
            building_hp_upgrade: 0,
            capital_building_hp: [0; 5],
            maya_building_hp: 0,
            roman_fort_hp: 0,
            taj_building_hp: 0,
            red_fort_fort_hps: 0,
            nubian_hit_points: 0,
            tikal_temple_hp: 0,
            ctw_missionaries_bonus: 0,
            senate_hp_bonus: 0,
            fort_upgrade_range: [0; 5],
            tower_fort_range: [0; 4],
            colosseum_fort_range: 0,
            furs_los: 0,
            maya_building_speed: 0,
            versailles_building_speed: 0,
            tobacco_building_speed: 0,
            british_aa_speed: 0,
            dutch_fort_speed: 0,
            roman_fort_speed: 0,
            capital_build_time: 0,
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
            unit_move_speed: at(rule_offsets::UNIT_MOVE_SPEED),
            military_transport_bonus: at(rule_offsets::MILITARY_TRANSPORT_BONUS),
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
            cattle_citizen_armor: at(rule_offsets::CATTLE_CITIZEN_ARMOR),
            dutch_attack_bonus: at(rule_offsets::DUTCH_ATTACK_BONUS),
            versailles_units_move: at(rule_offsets::VERSAILLES_UNITS_MOVE),
            aztec_move_speed: at(rule_offsets::AZTEC_MOVE_SPEED),
            bantu_units_move: at(rule_offsets::BANTU_UNITS_MOVE),
            french_siege_move: at(rule_offsets::FRENCH_SIEGE_MOVE),
            americans_marine_speed_bonus: at(rule_offsets::AMERICANS_MARINE_SPEED_BONUS),
            aluminum_air_speed: at(rule_offsets::ALUMINUM_AIR_SPEED),
            whales_ships_move: at(rule_offsets::WHALES_SHIPS_MOVE),
            building_hp_upgrade: at(rule_offsets::BUILDING_HP_UPGRADE),
            capital_building_hp: [
                at(rule_offsets::CAPITAL_BUILDING_HP),
                at(rule_offsets::CAPITAL_BUILDING_HP + 4),
                at(rule_offsets::CAPITAL_BUILDING_HP + 8),
                at(rule_offsets::CAPITAL_BUILDING_HP + 12),
                at(rule_offsets::CAPITAL_BUILDING_HP + 16),
            ],
            maya_building_hp: at(rule_offsets::MAYA_BUILDING_HP),
            roman_fort_hp: at(rule_offsets::ROMAN_FORT_HP),
            taj_building_hp: at(rule_offsets::TAJ_BUILDING_HP),
            red_fort_fort_hps: at(rule_offsets::RED_FORT_FORT_HPS),
            nubian_hit_points: at(rule_offsets::NUBIAN_HIT_POINTS),
            tikal_temple_hp: at(rule_offsets::TIKAL_TEMPLE_HP),
            ctw_missionaries_bonus: at(rule_offsets::CTW_MISSIONARIES_BONUS),
            senate_hp_bonus: at(rule_offsets::SENATE_HP_BONUS),
            fort_upgrade_range: [
                at(rule_offsets::FORT_UPGRADE_RANGE),
                at(rule_offsets::FORT_UPGRADE_RANGE + 4),
                at(rule_offsets::FORT_UPGRADE_RANGE + 8),
                at(rule_offsets::FORT_UPGRADE_RANGE + 12),
                at(rule_offsets::FORT_UPGRADE_RANGE + 16),
            ],
            tower_fort_range: [
                at(rule_offsets::TOWER_FORT_RANGE),
                at(rule_offsets::TOWER_FORT_RANGE + 4),
                at(rule_offsets::TOWER_FORT_RANGE + 8),
                at(rule_offsets::TOWER_FORT_RANGE + 12),
            ],
            colosseum_fort_range: at(rule_offsets::COLOSSEUM_FORT_RANGE),
            furs_los: at(rule_offsets::FURS_LOS),
            maya_building_speed: at(rule_offsets::MAYA_BUILDING_SPEED),
            versailles_building_speed: at(rule_offsets::VERSAILLES_BUILDING_SPEED),
            tobacco_building_speed: at(rule_offsets::TOBACCO_BUILDING_SPEED),
            british_aa_speed: at(rule_offsets::BRITISH_AA_SPEED),
            dutch_fort_speed: at(rule_offsets::DUTCH_FORT_SPEED),
            roman_fort_speed: at(rule_offsets::ROMAN_FORT_SPEED),
            capital_build_time: at(rule_offsets::CAPITAL_BUILD_TIME),
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

/// The PDB-named `LeaderData +0xA4C..+0xA66` event-rate block consumed by step 19.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct EventFrameState {
    /// `+0xA4C`.
    pub frame_battle: i32,
    /// `+0xA50/+0xA52/+0xA54/+0xA56`.
    pub average_death_rate: u16,
    pub average_kill_rate: u16,
    pub average_damage_rate: u16,
    pub average_hit_rate: u16,
    /// `+0xA58/+0xA5A/+0xA5C/+0xA5E`.
    pub deaths_current_frame: u16,
    pub kills_current_frame: u16,
    pub hits_current_frame: u16,
    pub damage_current_frame: u16,
    /// `+0xA60/+0xA62/+0xA64/+0xA66`.
    pub deaths_fifteen_seconds: u16,
    pub kills_fifteen_seconds: u16,
    pub hits_fifteen_seconds: u16,
    pub damage_fifteen_seconds: u16,
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
    /// `+0x7E4`.
    pub pop_cap: i32,
    /// `+0x7E8`, named `pop_issues` by the PDB.
    pub pop_issues: i32,
    /// `+0x9F4`.
    pub frame_counter_b: i32,
    /// `+0x7F0`.
    pub attrition: i32,
    /// `+0x7F4`, f32.
    pub anti_attrition: f32,
    /// `+0x7F8` `give_att_disabled`, projected from the canonical LeaderData owner.
    pub attrition_off: i32,
    /// `+0x7FC` `take_att_disabled`, projected from the canonical LeaderData owner.
    pub anti_attrition_off: i32,
    /// `+0x800` `neutral_attrition`, projected from the canonical LeaderData owner.
    pub neutral_attrition: i32,
    /// `+0x804` `disable_building_attrition`, projected from the canonical owner.
    pub building_attrition_off: i32,
    /// `+0x9D4`, rebuilt by step 11's [`check_explore`] on its phase.
    pub explored: i32,
    /// `+0xA4C..+0xA66`, processed at step 19.
    pub event_frame: EventFrameState,
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
    /// Live leader answers used to rebuild Unit speed/armor query packages. These are
    /// port-owned decoded state, like [`Leader::econ`], rather than another recovered
    /// offset block. `Wall::update_construct_time` reads its `has_tribe_bonus` and
    /// Versailles answers too — they are the *same* `LeaderData` queries, and a second
    /// copy of a tribe-bonus word is how one retail predicate becomes two disagreeing ones.
    pub unit_stats: UnitLeaderStatState,
    /// `LeaderData::city_num` (`Leader + 0x3F8`), read by `Wall::update_construct_time`
    /// at `0x0063D72D`.
    pub city_num: i32,
    /// The three `has_preq` answers `LeaderData::get_building_speed_upgrade` `0x006DAE90`
    /// counts, and the one `Wall::update_construct_time` tests directly.
    pub build_stats: BuildLeaderStatState,
}

/// The `LeaderData::has_preq` answers `Wall::update_construct_time` `0x0063D560` needs.
///
/// Retail asks `LeaderData` for each of these at the moment it needs it. Holding them on
/// the leader keeps the construct-time package edge-safe in exactly the way
/// [`UnitLeaderStatState`] keeps the Unit packages edge-safe: a stat pass triggered by a
/// rare-mask edge reads the *current* tech state, never a package built on a prior edge.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct BuildLeaderStatState {
    /// `LeaderData::has_preq(BUILDINGS_CREATED_FASTER)`, `TypeIndex` `0x313` = 787
    /// (`0x0063D5BB`). Its effect is the only non-percentage step in the chain:
    /// `v = (v * 3) >> 2`.
    pub buildings_created_faster: bool,
    /// `LeaderData::has_preq(BUILDINGS_FASTER_1..=3)`, `TypeIndex` `0x2F2..=0x2F4` =
    /// 754..=756, in that order — the three the `0x006DAE90` loop counts.
    pub buildings_faster: [bool; 3],
}

impl BuildLeaderStatState {
    /// `LeaderData::get_building_speed_upgrade` `0x006DAE90` (77 bytes), ported whole.
    ///
    /// The loop runs `t = 0x2F2 ..= 0x2F4` and adds one for each `has_preq(t)` that
    /// answers non-zero. Two details a paraphrase loses. It is **not** a consecutive run
    /// like `Leader::calc_attrition`'s: `0x006DAECA` is `test ecx,ecx; lea eax,[edi+1];
    /// cmove eax,edi`, an unconditional per-iteration accumulate, so a gap in the middle
    /// does not stop the count. And `0x006DAEA0` compares the loop variable against
    /// `0x2AD` (`BUY_SELL`) before consulting `has_tribe_bonus(4)` — a value the
    /// `0x2F2..=0x2F4` range never takes, so that arm is unreachable in this build. It is
    /// the *same* dead `0x2AD` compare `calc_attrition` carries, recorded rather than
    /// silently dropped.
    #[inline]
    pub fn get_building_speed_upgrade(self) -> i32 {
        let mut count = 0i32;
        for present in self.buildings_faster.iter().copied() {
            if present {
                count += 1;
            }
        }
        count
    }
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

/// `GameInfo::Player::flags & 1` — the player record participates in the step-17 scan.
pub const PLAYER_VALID: u16 = 0x0001;
/// `GameInfo::Player::flags & 0x800` — population-production warning is currently set.
pub const PLAYER_POP_CAP_WARNING: u16 = 0x0800;
/// Retail tests `frame - pop_cap_frame > 0x1c1`, so the first due elapsed value is 450.
pub const POP_CAP_WARNING_MIN_ELAPSED: i32 = 450;
/// Category passed to `SoundGlobal::play` when the feedback is emitted.
pub const POP_CAP_WARNING_SOUND: i32 = 0x5B;
/// Offset into retail's localized text table copied before `MessageWin::add_feedback`.
pub const POP_CAP_WARNING_TEXT_OFFSET: u32 = 0xC878;
/// The six `pop_limits` category rows shipped in `rules.xml`, in category order.
pub const SHIPPED_POP_LIMITS: [i32; 6] = [50, 75, 100, 125, 150, 200];

/// The three `GameInfo::Player` fields read or written by step 17.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct EndPlayer {
    /// `GameInfo::Player +0x30`, whose low bit gates the warning-clear scan.
    pub flags: u16,
    /// `GameInfo::Player +0x33`, compared with `Leader +0x08` after zero-extension.
    pub who: u8,
    /// `GameInfo::Player +0x2c`, the frame of the prior population-cap feedback.
    pub pop_cap_frame: i32,
}

/// External `GameInfo` / `Console` state consumed by [`end_process_all`].
///
/// It lives beside the leaders because retail step 17 mutates the player warning bits and
/// feedback stamp. `last_feedback` is the headless presentation boundary: retail
/// constructs one localized string, calls `MessageWin::add_feedback`, then plays sound
/// category `0x5b`; the simulation records that request without importing UI or audio.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EndProcessState {
    pub players: [EndPlayer; NUM_LEADER_SLOTS],
    /// `Console +0x298`; `-1` means the host has not supplied a local player identity.
    pub local_who: i32,
    /// `Console +0x2a0`; an index into `players`.
    pub local_play: i32,
    /// `GameInfo +0x25`; an index into the shipped population-limit category.
    /// `u8::MAX` means the host has not supplied the match option.
    pub pop_limit_index: u8,
    /// Presentation requests emitted by the most recent dispatcher call only.
    pub last_feedback: Vec<PopulationCapFeedback>,
}

impl Default for EndProcessState {
    fn default() -> Self {
        EndProcessState {
            players: std::array::from_fn(|i| EndPlayer {
                who: i as u8,
                ..EndPlayer::default()
            }),
            local_who: -1,
            local_play: -1,
            pop_limit_index: u8::MAX,
            last_feedback: Vec::new(),
        }
    }
}

/// One reached `JukeBox::set_next_mood` presentation call from step 19.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CombatMoodRequest {
    pub leader_index: usize,
    pub mood: i32,
    /// The boolean passed in `ECX`: quiet with no combat, or a >=2000 high-rate transition
    /// out of mood 2.
    pub force: bool,
}

/// The two `Achieve::add_event` kinds emitted by the lopsided-battle detector.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BattleAchievementKind {
    KillsOverDeaths = 0,
    DeathsOverKills = 1,
}

/// One reached `Achieve::add_event(kind, who, EMPTY_STRING)` call from step 19.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BattleAchievementEvent {
    pub leader_index: usize,
    pub who: i32,
    pub kind: BattleAchievementKind,
}

/// Exact ordered product boundary for the most recent step-19 dispatcher call.
///
/// The headless core cannot reproduce JukeBox wall-clock/audio playback, and achievement
/// presentation is not simulation state. It does own delivery of every reached retail call:
/// this outbox retains the original shared sequence number, leader identity, call VA, and
/// arguments. Product adapters consume these receipts; an empty later dispatcher replaces the
/// prior frame so a stale call cannot replay.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EventProductOutbox {
    pub last_calls: Vec<step19::HostTailReceipt>,
}

/// Presentation-owned globals touched by step 19.
///
/// Both moods are zero in the executable image; a product host may refresh
/// `current_music_mood` from its JukeBox before the step. Requests and achievement events
/// are cleared and rebuilt on every dispatcher call, so headless consumers never replay a
/// prior frame's presentation work.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct EventProcessState {
    /// `JukeBox` current mood at `0x00ECBA20`.
    pub current_music_mood: i32,
    /// Requested mood at `0x00ECBA2C`.
    pub next_music_mood: i32,
    /// Installed headless owner for both reached product calls, in exact retail order.
    pub product_outbox: EventProductOutbox,
    /// Convenience projection of JukeBox calls in [`Self::product_outbox`].
    pub last_mood_requests: Vec<CombatMoodRequest>,
    /// Convenience projection of achievement calls in [`Self::product_outbox`].
    pub last_achievement_events: Vec<BattleAchievementEvent>,
}

/// The eight `Leader` slots the loop walks.
#[derive(Clone, Debug)]
pub struct Leaders {
    pub leaders: [Leader; NUM_LEADER_SLOTS],
    pub end: EndProcessState,
    pub event: EventProcessState,
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
            end: EndProcessState::default(),
            event: EventProcessState::default(),
        }
    }

    /// Populate the `GameInfo::Player` identity/valid fields the lightweight simulation
    /// already knows, without disturbing warning bits or timestamps owned by step 17.
    ///
    /// Retail owns `LeaderData` and `GameInfo::Player` separately. The port therefore
    /// performs this synchronization at the dispatcher boundary: it copies the decoded
    /// `who`, toggles only `flags & 1`, and preserves every other Player flag.
    pub fn sync_end_players_from_leaders(&mut self) {
        for i in 0..NUM_LEADER_SLOTS {
            self.end.players[i].who = self.leaders[i].slot as u8;
            if self.leaders[i].flags & flag::IN_GAME != 0 {
                self.end.players[i].flags |= PLAYER_VALID;
            } else {
                self.end.players[i].flags &= !PLAYER_VALID;
            }
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

/// Inputs to `Object::update_hits` `0x00647010` after resolving the object's type-table
/// rows. The three alternate values are only used by the special type families `0x32/0x33`.
///
/// Retail selects type `0x44`, `0x43`, then `0x42` for owner policy bits `0x10`, `0x08`,
/// then `0x04`; with none set it keeps the base type. Carrying all four lookup results here
/// keeps the function exact without making the leader scheduler own the global type table.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ObjectHitInputs {
    pub base_hits: i32,
    pub special_family_32_33: bool,
    pub type_42_hits: i32,
    pub type_43_hits: i32,
    pub type_44_hits: i32,
    pub owner_policy: u8,
}

/// Object/type query answers consumed by `ObjectData::armor` `0x00647DB0`.
///
/// This is the same boundary style as [`AttritionGates`]: the body and branch order are
/// recovered here, while type predicates and tribe membership come from their owning
/// tables rather than being guessed by the scheduler.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct UnitArmorInputs {
    /// `ObjectTypeData::armor` at `type + 0x214`.
    pub type_armor: i32,
    /// `LeaderData::has_tribe_bonus(0x16)`.
    pub dutch: bool,
    /// Vtable `+0x18`, the first Dutch armor eligibility gate.
    pub dutch_armor_eligible: bool,
    /// `this->ptype->type`, compared with `0x3D`, `0x3E`, and decimal `400`.
    pub type_id: i32,
    pub caravan: bool,
    pub supply: bool,
    /// `Game::get_patch_version()`; versions above 8 apply the government-hero exclusion.
    pub patch_version: i32,
    pub gov_hero: bool,
    /// `this->ptype->type` is `0x32` or `0x33`.
    pub special_family_32_33: bool,
}

/// Direct-field and virtual-query answers consumed by `Unit::update_speed` `0x006055C0`.
///
/// The function body is recovered here in full; this package is the honest boundary to the
/// global type table and leader tech/tribe/wonder queries it calls. `military_epoch` is the
/// decoded `LeaderDataEncrypt::epoch[Military]` at encrypted-block `+0xE8` (`^ 0x63187`),
/// not the leader's age.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct UnitSpeedInputs {
    /// `UnitTypeData::moves` at `type + 0x2C0`.
    pub type_moves: i32,
    /// `ObjectTypeData::domain` at `type + 0x218`: 1 water, 2 air in these branches.
    pub domain: i32,
    /// `UnitTypeData::unit_flags` at `type + 0x2B4`.
    pub unit_flags: u32,
    /// `UnitData` flags at unit `+0x6C`; bit `0x200` is the marine-transport bonus gate.
    pub unit_data_flags: u32,
    pub military_epoch: i32,
    /// Result of vtable `+0x148`, `ObjectData::has_objmask(0x2000)`.
    pub has_objmask_2000: bool,
    /// Results of `ObjectData::is(type, 0)` in retail's precedence order.
    pub is_68: bool,
    pub is_66: bool,
    pub is_64: bool,
    pub is_62: bool,
    pub is_42: bool,
    pub is_3a: bool,
    /// The direct type-table dword at `type + 0x40`.
    pub type_line: i32,
    /// `TypeData::type` at `type + 0x04`.
    pub type_id: i32,
    /// `LeaderData::has_tribe_bonus(3)`.
    pub bantu: bool,
    /// `LeaderData::has_tribe_bonus(10)`.
    pub french: bool,
    /// `LeaderData::has_wonder(0x218)`.
    pub versailles: bool,
    /// `LeaderData::get_spy_upgrade()`; only read for `is_3a`.
    pub spy_upgrade: i32,
    /// Result of vtable `+0xC4`, `UnitData::is_hero`, and its upgrade query.
    pub hero: bool,
    pub general_upgrade: i32,
    /// Result of vtable `+0xCC`, `UnitData::is_supply`, and its upgrade query.
    pub supply: bool,
    pub supply_upgrade: i32,
    /// `LeaderData::has_tribe_bonus(0)`.
    pub aztec: bool,
}

/// Decoded leader state queried by `Unit::update_speed` and `ObjectData::armor`.
///
/// Retail obtains these values through `LeaderData` calls. Keeping them together on the
/// leader makes automatic package population edge-safe: every dirty pass reads the current
/// values instead of replaying a package built on a prior rare/tech edge. A new game starts
/// at military epoch zero with no tribe bonuses, Wonders, or unit upgrades. Retail
/// `Game::get_patch_version` returns 8 when the game version is at most the running build;
/// 9 is only its forward-version arm.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct UnitLeaderStatState {
    pub military_epoch: i32,
    /// `LeaderData::has_tribe_bonus(n)`, for the 24 shipped nation slots.
    pub tribe_bonuses: u32,
    /// `LeaderData::has_wonder(0x218)` — Versailles.
    pub versailles: bool,
    pub spy_upgrade: i32,
    pub general_upgrade: i32,
    pub supply_upgrade: i32,
    pub patch_version: i32,
}

impl Default for UnitLeaderStatState {
    fn default() -> Self {
        UnitLeaderStatState {
            military_epoch: 0,
            tribe_bonuses: 0,
            versailles: false,
            spy_upgrade: 0,
            general_upgrade: 0,
            supply_upgrade: 0,
            patch_version: 8,
        }
    }
}

impl UnitLeaderStatState {
    #[inline]
    pub const fn has_tribe_bonus(self, bonus: u32) -> bool {
        bonus < 32 && self.tribe_bonuses & (1u32 << bonus) != 0
    }

    #[inline]
    pub fn set_tribe_bonus(&mut self, bonus: u32, present: bool) {
        if bonus >= 32 {
            return;
        }
        if present {
            self.tribe_bonuses |= 1u32 << bonus;
        } else {
            self.tribe_bonuses &= !(1u32 << bonus);
        }
    }
}

/// Live object facts that select a shipped `UnitTypeData` row.
///
/// `type_id` is the referent behind `UnitData::ptype` (`+0x18`); `unit_masks2` is the
/// object-local dword at `UnitData +0x6C`. Presence of this source opts the object into
/// automatic package population. An unknown type id clears any prior generated package and
/// remains an unresolved call instead of borrowing stale inputs.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct UnitQuerySource {
    pub type_id: i32,
    pub unit_masks2: u32,
}

/// Canonical mutable type fields substituted after the shipped structural query package is
/// rebuilt. BHS owns only these two scalar overrides; lineage, masks, domain, and class gates
/// continue to come from the same instruction-derived type row.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct UnitTypeStatOverride {
    pub type_id: i32,
    pub moves: i32,
    pub armor: i32,
}

/// The supported post-load Unit table covers the 364 consecutive retail Type slots 50..414.
pub const UNIT_TYPE_STAT_FIRST: i32 = 50;
pub const UNIT_TYPE_STAT_END: i32 = 414;
pub const UNIT_TYPE_STAT_ROWS: usize = (UNIT_TYPE_STAT_END - UNIT_TYPE_STAT_FIRST) as usize;

/// SHA-256 of the supported post-load `live-tables-unit.tsv` capture.
///
/// The capture itself is local retail-derived evidence and is deliberately excluded from a
/// source archive. The digest is only an admission identity supplied by the runtime loader; it
/// does not convey the table.
pub const SUPPORTED_UNIT_TYPE_STAT_TSV_SHA256: [u8; 32] = [
    0x59, 0xf5, 0x28, 0x7f, 0x4c, 0x33, 0x87, 0x61, 0x7b, 0xfe, 0x76, 0xab, 0x26, 0x10, 0xc2, 0x42,
    0x23, 0xb2, 0x22, 0x0e, 0xe0, 0xbd, 0xf0, 0xe0, 0x84, 0xaa, 0xe0, 0x28, 0xc5, 0x7a, 0x90, 0xd5,
];

/// Identity attached by the local extractor/loader to one post-load Unit stat source.
///
/// The loader must compute `table_sha256` over the exact TSV bytes before calling
/// [`UnitTypeStatSource::from_live_tsv`]. Requiring both identities keeps an unrelated game
/// generation or a hand-edited table from silently becoming the simulation's shipped truth.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct UnitTypeStatSourceProvenance {
    pub executable_sha256: [u8; 32],
    pub table_sha256: [u8; 32],
}

/// One admitted scalar slice of post-load retail `UnitTypeData`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
struct UnitTypeStatRow {
    type_id: i32,
    from: i32,
    where_type: i32,
    obj_masks: u32,
    armor: i32,
    domain: i32,
    graft: i32,
    unit_flags: u32,
    unit_flags2: u32,
    moves: i32,
}

/// A validated runtime-owned source for the two recovered Unit stat bodies.
///
/// No retail table is compiled into `don-sim`. A product host may read the user's local capture,
/// verify its SHA-256, and install this source. Without one, every automatic type query records a
/// miss and leaves the Unit's prior walked stats untouched.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct UnitTypeStatSource {
    provenance: UnitTypeStatSourceProvenance,
    rows: Box<[Option<UnitTypeStatRow>]>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum UnitTypeStatSourceError {
    UnsupportedExecutable,
    UnsupportedTable,
    MissingHeader,
    MissingColumn(&'static str),
    WrongRowCount {
        got: usize,
    },
    ShortRow {
        line: usize,
    },
    InvalidInteger {
        line: usize,
        column: &'static str,
    },
    TypeIdOutOfRange {
        line: usize,
        type_id: i32,
    },
    DuplicateType {
        type_id: i32,
    },
    MissingType {
        type_id: i32,
    },
    RelationOutOfRange {
        type_id: i32,
        column: &'static str,
        target: i32,
    },
}

impl std::fmt::Display for UnitTypeStatSourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for UnitTypeStatSourceError {}

impl UnitTypeStatSource {
    /// Parse an exact supported local post-load capture after its host computed both identities.
    pub fn from_live_tsv(
        tsv: &str,
        provenance: UnitTypeStatSourceProvenance,
    ) -> Result<Self, UnitTypeStatSourceError> {
        if provenance.executable_sha256 != super::unit_inctime::SUPPORTED_RETAIL_EXE_SHA256 {
            return Err(UnitTypeStatSourceError::UnsupportedExecutable);
        }
        if provenance.table_sha256 != SUPPORTED_UNIT_TYPE_STAT_TSV_SHA256 {
            return Err(UnitTypeStatSourceError::UnsupportedTable);
        }

        let mut lines = tsv.lines();
        let header = lines.next().ok_or(UnitTypeStatSourceError::MissingHeader)?;
        let names: Vec<&str> = header.split('\t').collect();
        let col = |name: &'static str| {
            names
                .iter()
                .position(|candidate| *candidate == name)
                .ok_or(UnitTypeStatSourceError::MissingColumn(name))
        };
        let columns = [
            ("type_id", col("type_id")?),
            ("from", col("from")?),
            ("where", col("where")?),
            ("obj_masks", col("obj_masks")?),
            ("armor", col("armor")?),
            ("domain", col("domain")?),
            ("graft", col("graft")?),
            ("unit_flags", col("unit_flags")?),
            ("unit_flags2", col("unit_flags2")?),
            ("moves", col("moves")?),
        ];
        let max_col = columns.iter().map(|(_, index)| *index).max().unwrap_or(0);
        let data: Vec<&str> = lines.collect();
        if data.len() != UNIT_TYPE_STAT_ROWS {
            return Err(UnitTypeStatSourceError::WrongRowCount { got: data.len() });
        }

        let mut rows = vec![None; UNIT_TYPE_STAT_END as usize];
        for (row_index, line) in data.into_iter().enumerate() {
            let line_number = row_index + 2;
            let cells: Vec<&str> = line.split('\t').collect();
            if cells.len() <= max_col {
                return Err(UnitTypeStatSourceError::ShortRow { line: line_number });
            }
            let parse = |name: &'static str| -> Result<i32, UnitTypeStatSourceError> {
                let column = columns
                    .iter()
                    .find_map(|(candidate, index)| (*candidate == name).then_some(*index))
                    .expect("closed parser column list");
                cells[column]
                    .parse::<i32>()
                    .map_err(|_| UnitTypeStatSourceError::InvalidInteger {
                        line: line_number,
                        column: name,
                    })
            };
            let parse_u32 = |name: &'static str| -> Result<u32, UnitTypeStatSourceError> {
                let column = columns
                    .iter()
                    .find_map(|(candidate, index)| (*candidate == name).then_some(*index))
                    .expect("closed parser column list");
                cells[column]
                    .parse::<u32>()
                    .map_err(|_| UnitTypeStatSourceError::InvalidInteger {
                        line: line_number,
                        column: name,
                    })
            };
            let row = UnitTypeStatRow {
                type_id: parse("type_id")?,
                from: parse("from")?,
                where_type: parse("where")?,
                obj_masks: parse_u32("obj_masks")?,
                armor: parse("armor")?,
                domain: parse("domain")?,
                graft: parse("graft")?,
                unit_flags: parse_u32("unit_flags")?,
                unit_flags2: parse_u32("unit_flags2")?,
                moves: parse("moves")?,
            };
            if !(UNIT_TYPE_STAT_FIRST..UNIT_TYPE_STAT_END).contains(&row.type_id) {
                return Err(UnitTypeStatSourceError::TypeIdOutOfRange {
                    line: line_number,
                    type_id: row.type_id,
                });
            }
            let index = row.type_id as usize;
            if rows[index].replace(row).is_some() {
                return Err(UnitTypeStatSourceError::DuplicateType {
                    type_id: row.type_id,
                });
            }
        }

        for type_id in UNIT_TYPE_STAT_FIRST..UNIT_TYPE_STAT_END {
            let row =
                rows[type_id as usize].ok_or(UnitTypeStatSourceError::MissingType { type_id })?;
            for (column, target) in [("from", row.from), ("graft", row.graft)] {
                if target != -1 && !(UNIT_TYPE_STAT_FIRST..UNIT_TYPE_STAT_END).contains(&target) {
                    return Err(UnitTypeStatSourceError::RelationOutOfRange {
                        type_id,
                        column,
                        target,
                    });
                }
            }
        }

        Ok(Self {
            provenance,
            rows: rows.into_boxed_slice(),
        })
    }

    #[inline]
    fn row(&self, type_id: i32) -> Option<UnitTypeStatRow> {
        let index = usize::try_from(type_id).ok()?;
        self.rows.get(index).copied().flatten()
    }

    pub fn provenance(&self) -> UnitTypeStatSourceProvenance {
        self.provenance
    }
}

/// `ObjectTypeData::is(type, 0)` over the exact load-time source relation.
///
/// `ObjectType::init_is_list` caches this result, but the cache is derived rather than
/// walked source data: identity, then `graft`, then recursive `from`. A malformed cycle or
/// a missing ancestor is an explicit missing fact.
fn shipped_unit_is(source: &UnitTypeStatSource, mut type_id: i32, target: i32) -> Option<bool> {
    let mut remaining = source.rows.len().max(1);
    while remaining != 0 {
        remaining -= 1;
        let row = source.row(type_id)?;
        if row.type_id == target || row.graft == target {
            return Some(true);
        }
        if row.from < 0 {
            return Some(false);
        }
        type_id = row.from;
    }
    None
}

/// Rebuild both global query packages from current shipped type, leader, and object state.
fn derive_unit_query_packages(
    table: &UnitTypeStatSource,
    source: UnitQuerySource,
    leader: &Leader,
) -> Option<(UnitSpeedInputs, UnitArmorInputs)> {
    let row = table.row(source.type_id)?;
    let live = leader.unit_stats;
    let supply = row.unit_flags2 & 0x40 != 0;
    let speed = UnitSpeedInputs {
        type_moves: row.moves,
        domain: row.domain,
        unit_flags: row.unit_flags,
        unit_data_flags: source.unit_masks2,
        military_epoch: live.military_epoch,
        has_objmask_2000: row.obj_masks & 0x2000 != 0,
        is_68: shipped_unit_is(table, source.type_id, 0x68)?,
        is_66: shipped_unit_is(table, source.type_id, 0x66)?,
        is_64: shipped_unit_is(table, source.type_id, 0x64)?,
        is_62: shipped_unit_is(table, source.type_id, 0x62)?,
        is_42: shipped_unit_is(table, source.type_id, 0x42)?,
        is_3a: shipped_unit_is(table, source.type_id, 0x3a)?,
        type_line: row.where_type,
        type_id: row.type_id,
        bantu: live.has_tribe_bonus(3),
        french: live.has_tribe_bonus(10),
        versailles: live.versailles,
        spy_upgrade: live.spy_upgrade,
        hero: row.unit_flags2 & 0x20 != 0,
        general_upgrade: live.general_upgrade,
        supply,
        supply_upgrade: live.supply_upgrade,
        aztec: live.has_tribe_bonus(0),
    };
    let armor = UnitArmorInputs {
        type_armor: row.armor,
        dutch: live.has_tribe_bonus(0x16),
        // UnitData's vtable `+0x18` is the shared true body `0x0041E0E0`.
        dutch_armor_eligible: true,
        type_id: row.type_id,
        caravan: row.unit_flags2 & 8 != 0,
        supply,
        patch_version: live.patch_version,
        gov_hero: row.unit_flags & 0x0400_0000 != 0,
        special_family_32_33: row.type_id == 0x32 || row.type_id == 0x33,
    };
    Some((speed, armor))
}

/// Object- and type-local query answers consumed by `Wall::update_construct_time`
/// `0x0063D560`.
///
/// Everything the function asks *the leader* — four `has_tribe_bonus` calls, one
/// `has_wonder`, two rare-mask bits, `city_num`, and three `has_preq` calls — is read
/// straight off [`Leader`], so this package carries only what lives on the object or in
/// the global type table. Its presence is what distinguishes a resolved call from a
/// charged one.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct WallConstructTimeInputs {
    /// `TypeData::time(who)` `0x00663F20` through `ObjectType` vtable `+0x6C`
    /// (`0x0063D56F`), the seed of the whole chain and an **unsigned** dword.
    ///
    /// The 86-byte body is: `is_spell_type` — either the base test `0x275 <= type < 0x2AC`
    /// (`BASE_SPELLTYPES`..`CANCEL_ALLIANCE7`) when vtable `+0x48` is still
    /// `TypeData::is_spell_type` `0x00470590`, or the override's answer — then
    /// `job_time * 100` for anything that is not a spell type or that
    /// `LeaderData::has_spell` `0x006E0BC0` already answers for, and `res_time * 100`
    /// otherwise. A building type is never in the spell range, so for both bands this is
    /// `BuildTypeData::job_time * 100`. It is a *global type table* read, which is why it
    /// is an input here and not a derivation.
    pub type_time: u32,
    /// Object vtable `+0x2C` (`0x0063D593`, `0x0063D6B9`, `0x0063D705`).
    ///
    /// Band 2000 dispatches to `BuildData::is_wonder` `0x00472320`, which is
    /// `0x20E <= ptype->type < 0x21F` — `BASE_WONDERTYPES`..`SPACEPROGRAM` inclusive.
    /// Band 3000's `WallData` vtable holds the constant-zero body `0x0041BFF0`, so a plain
    /// wall is never a wonder. Same slot, and therefore the same answer, as
    /// [`WallHitInputs::is_wonder`].
    pub is_wonder: bool,
    /// `ObjectData::is(AIRDEFENSE, 0)` — `TypeIndex` `0x20B` = 523 — reached only when the
    /// owner has the British bonus (`0x0063D652`).
    ///
    /// Retail devirtualises it: `0x0063D65D` compares vtable `+0xB8` against
    /// `ObjectData::is` `0x00653790` and, when it matches, calls the type's `+0x60`
    /// (`ObjectTypeData::is` `0x0065F7D0`) directly instead of through the one-jump
    /// trampoline. Every band's `+0xB8` *is* `ObjectData::is`, so the two arms are one
    /// predicate and the fast path is always the taken one.
    pub is_airdefense: bool,
    /// Type vtable `+0xFC` (`0x0063D6AB`, `0x0063D6F7`) — `BuildTypeData::is_fort`
    /// `0x00472BA0`, which is `ObjectTypeData::is(FORTX, 0)`, `TypeIndex` `0x1BB` = 443.
    /// Reached only for a Dutch or Roman owner.
    pub is_fort: bool,
}

/// Global query answers consumed by the percentage chain in `Wall::update_hits`
/// `0x0063F0D0`. Object-local construction fields remain on [`StatObject`] and the base
/// `Object::update_hits` row remains [`StatObject::hit_inputs`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct WallHitInputs {
    pub maya: bool,
    pub roman: bool,
    pub is_city_type: bool,
    pub is_tower_1b7: bool,
    pub is_wonder: bool,
    pub building_hp_upgrade: i32,
    pub taj_mahal: bool,
    pub red_fort: bool,
    pub is_fort_1b4: bool,
    pub nubian: bool,
    /// Vtable `+0x20`, reached only by the `flags & 0x20 == 0` arm.
    pub noncapital_city_gate: bool,
    /// Whether the guard building's `BuildData::city` (`+0x72`) is non-negative.
    pub linked_city: bool,
    /// `City::num_cities()`-like result at `0x00739340`, used as `(count - 1)`.
    pub linked_city_count: i32,
    /// `flags & 0x80` on the linked city object, in the `flags & 0x20 != 0` arm.
    pub linked_city_is_capital: bool,
    pub has_preq_2cd: bool,
    /// `LeaderData::get_building_upgrade()` when `has_preq_2cd` is false.
    pub capital_building_upgrade: i32,
    pub tikal: bool,
    pub ctw_mode: bool,
    pub ctw_rare_20: bool,
    /// Result of `ObjectData::can_carry(DOMAIN_AIR=2)` at the ejection tail.
    pub can_carry_domain_2: bool,
}

/// Global type/leader query answers consumed by `Wall::update_los` `0x0063EEB0`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct WallLosInputs {
    /// Decoded signed byte at `LeaderDataEncrypt + 0xF4` (`^ 0x87`).
    pub science_level: i8,
    /// Signed `ObjectTypeData::science_los` at `type + 0x220`.
    pub type_science_los: i8,
    pub is_wonder: bool,
    pub is_city_type: bool,
    pub is_tower_1b7: bool,
    pub city_range_upgrade: i32,
    pub tower_range_upgrade: i32,
    pub colosseum: bool,
    /// `ObjectTypeData::x_size` at `type + 0x234`; retail adds signed `x_size / 2`.
    pub footprint_x: i32,
}

/// One entry of an `Objects` band, as the two stat passes observe it through the vtable.
///
/// `hit_inputs` and `type_los` are resolved type-table inputs, not invented answers. `None`
/// means the caller has not supplied that global table row; the traversal still executes,
/// but records an unresolved call and leaves walked state untouched.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct StatObject {
    /// `data->byte[8] & 1` — the active bit both band loops test
    /// (`0x006CF7EC`, `0x006CF9A5`).
    pub active: bool,
    /// `WallData::is_active`, vtable `+0x4C`: `flags & 4` (`0x00472350`, 8 bytes).
    pub wall_active: bool,
    /// `UnitData::is_captain`, vtable `+0xE8`: `o_up >> 15` (`0x0046CEB0`, 13 bytes).
    pub captain: bool,
    /// The type-table rows consumed by `Object::update_hits`.
    pub hit_inputs: Option<ObjectHitInputs>,
    /// `ObjectTypeData::los` (`type + 0x21C`) for `Object::update_los`.
    pub type_los: Option<i8>,
    /// Whether the owning leader has its `flags & 1` bit set. An out-of-game owner forces
    /// `Object::update_los` to store zero.
    pub owner_in_game: bool,
    /// `ObjectData::myhits` (`+0x20`), written by `Object::update_hits`.
    pub myhits: i32,
    /// `ObjectData::mylos` (`+0x3C`), written by `Object::update_los`.
    pub mylos: i8,
    /// Type/tribe gate package consumed by the exact `ObjectData::armor()` body.
    pub armor_inputs: Option<UnitArmorInputs>,
    /// Type/tech/tribe gate package consumed by the exact `Unit::update_speed` body.
    pub speed_inputs: Option<UnitSpeedInputs>,
    /// Presence opts this live Unit into automatic speed/armor package population.
    pub unit_query_source: Option<UnitQuerySource>,
    /// Query packages consumed by the building-band Wall override pair.
    pub wall_hit_inputs: Option<WallHitInputs>,
    pub wall_los_inputs: Option<WallLosInputs>,
    /// Query package consumed by `Wall::update_construct_time`. Both bands direct-call the
    /// same body, so both bands carry this.
    pub wall_construct_time_inputs: Option<WallConstructTimeInputs>,
    /// Resolved `UnitData::o_down` (`+0x90`). Retail stores a signed object index and uses
    /// every negative value as the end sentinel; `None` is that sentinel here.
    pub o_down: Option<usize>,
    /// `UnitData::myarmor` (`+0x9C`).
    pub myarmor: i16,
    /// `UnitData::myspeed` (`+0x9A`).
    pub myspeed: i16,
    /// Per-pass write marker, reset by the tick adapter before dispatch. Unlike the call
    /// counters this must not persist, or a clean frame would replay an old derivation.
    pub armor_written: bool,
    /// Same edge-local write marker for `myspeed`.
    pub speed_written: bool,
    /// Building-local fields read/written by the Wall overrides.
    pub wall_started: bool,
    pub wall_city_flag: bool,
    pub job_counter: u32,
    pub constr_time: u32,
    pub construct_hits: i32,
    pub damage: i32,
    pub inside_down: i16,
    pub wall_hits_written: bool,
    pub wall_los_written: bool,
    /// Edge-local write marker for `constr_time`, reset by the tick adapter with the other
    /// two. `Wall::update_construct_time` runs *before* `Wall::update_hits` in the same
    /// loop body, so the value it stores is the one the construction ramp then consumes.
    pub construct_time_written: bool,
    pub eject_contents_requested: bool,
    /// How many times `vtbl + 0x160` was invoked on this object.
    pub v160_calls: u32,
    /// How many times `vtbl + 0x15C` was invoked on this object.
    pub v15c_calls: u32,
    /// How many times `Wall::update_construct_time` `0x0063D560` was invoked.
    pub construct_time_updates: u32,
    /// How many of those resolved their type package and stored `constr_time`.
    pub construct_time_resolved: u32,
    /// `Unit::update_speed` `0x006055C0`.
    pub speed_updates: u32,
    /// `Unit::update_armor` `0x006054C0`.
    pub armor_updates: u32,
    /// Resolved base `Object::update_hits` calls that wrote `myhits`.
    pub object_hits_updates: u32,
    /// Resolved base `Object::update_los` calls that wrote `mylos`.
    pub object_los_updates: u32,
    /// Resolved `Unit::update_armor` calls (one per captain, not per propagated member).
    pub unit_armor_updates: u32,
    /// Resolved `Unit::update_speed` calls (one per captain, not per propagated member).
    pub unit_speed_updates: u32,
    /// Automatic shipped type/leader/object packages rebuilt this pass.
    pub unit_query_populations: u32,
    /// Automatic package requests that stopped at an unknown type/ancestor row.
    pub unit_query_misses: u32,
    pub wall_hits_updates: u32,
    pub wall_los_updates: u32,
}

/// What one stat pass did, so "it ran" is a number instead of an assertion.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct StatPassCounts {
    pub visited: u32,
    pub active: u32,
    pub construct_time_updates: u32,
    /// Of those, the ones whose type package was present and whose `constr_time` was
    /// stored. `construct_time_updates - construct_time_resolved` is exactly what this
    /// pass charged to `Gap::LeaderCalcWallStats` for the construct-time call.
    pub construct_time_resolved: u32,
    pub speed_updates: u32,
    pub armor_updates: u32,
    pub object_hits_updates: u32,
    pub object_los_updates: u32,
    pub unit_armor_updates: u32,
    pub unit_speed_updates: u32,
    pub unit_query_populations: u32,
    pub unit_query_misses: u32,
    pub wall_hits_updates: u32,
    pub wall_los_updates: u32,
    pub eject_contents_calls: u32,
    /// Calls whose body or global type-table input remains unavailable.
    pub unresolved_calls: u32,
}

impl StatPassCounts {
    fn add(&mut self, other: StatPassCounts) {
        self.visited += other.visited;
        self.active += other.active;
        self.construct_time_updates += other.construct_time_updates;
        self.construct_time_resolved += other.construct_time_resolved;
        self.speed_updates += other.speed_updates;
        self.armor_updates += other.armor_updates;
        self.object_hits_updates += other.object_hits_updates;
        self.object_los_updates += other.object_los_updates;
        self.unit_armor_updates += other.unit_armor_updates;
        self.unit_speed_updates += other.unit_speed_updates;
        self.unit_query_populations += other.unit_query_populations;
        self.unit_query_misses += other.unit_query_misses;
        self.wall_hits_updates += other.wall_hits_updates;
        self.wall_los_updates += other.wall_los_updates;
        self.eject_contents_calls += other.eject_contents_calls;
        self.unresolved_calls += other.unresolved_calls;
    }
}

/// `Object::update_hits(int full)` `0x00647010` (98 bytes), the base implementation at
/// vtable `+0x15C` for units and plain walls.
///
/// The `full` argument only selects the return value in the `Wall` override. Base Object
/// always writes and returns `myhits`; retaining it in the signature documents why the
/// dispatcher passes zero without creating a false branch here.
pub fn object_update_hits(o: &mut StatObject, _full: bool) -> Option<i32> {
    let i = o.hit_inputs?;
    let hits = if !i.special_family_32_33 {
        i.base_hits
    } else if i.owner_policy & 0x10 != 0 {
        i.type_44_hits
    } else if i.owner_policy & 0x08 != 0 {
        i.type_43_hits
    } else if i.owner_policy & 0x04 != 0 {
        i.type_42_hits
    } else {
        i.base_hits
    };
    o.myhits = hits;
    o.object_hits_updates = o.object_hits_updates.wrapping_add(1);
    Some(hits)
}

/// `Object::update_los()` `0x00646FE0` (42 bytes), the base implementation at vtable
/// `+0x160`: out-of-game owners store zero; otherwise store `ObjectTypeData::los`.
pub fn object_update_los(o: &mut StatObject) -> Option<i32> {
    let base = o.type_los?;
    o.mylos = if o.owner_in_game { base } else { 0 };
    o.object_los_updates = o.object_los_updates.wrapping_add(1);
    Some(o.mylos as i32)
}

/// `ObjectData::armor()` `0x00647DB0` (215 bytes), including the Dutch merchant/caravan/
/// wagon age bonus. Every early return is retained in retail order.
pub fn object_data_armor(input: &UnitArmorInputs, leader: &Leader, rules: &Step8Rules) -> i32 {
    let base = input.type_armor;
    if !input.dutch || rules.dutch_attack_bonus == 0 || !input.dutch_armor_eligible {
        return base;
    }
    if input.type_id != 0x3d
        && input.type_id != 0x3e
        && input.type_id != 400
        && !input.caravan
        && !input.supply
    {
        return base;
    }
    if input.patch_version > 8 && input.gov_hero {
        return base;
    }
    leader
        .econ
        .age
        .wrapping_mul(rules.dutch_attack_bonus)
        .wrapping_add(base)
}

#[inline]
fn speed_percent(value: i32, bonus: i32) -> i32 {
    bonus
        .wrapping_add(100)
        .wrapping_mul(value)
        .wrapping_div(100)
}

#[inline]
fn speed_quarter_upgrade(value: i32, upgrade: i32) -> i32 {
    let product = upgrade.wrapping_mul(value);
    let delta = product.wrapping_add((product >> 31) & 3) >> 2;
    value.wrapping_add(delta)
}

/// Arithmetic body of `Unit::update_speed` `0x006055C0` after its standalone `o_up`
/// captain climb. Branch order and signed rounding match the retail instructions.
pub fn unit_speed(input: &UnitSpeedInputs, leader: &Leader, rules: &Step8Rules) -> i32 {
    let mut speed = input.type_moves;

    if input.domain == 1 && input.unit_flags & 0x10 != 0 {
        speed = speed.wrapping_add(
            input
                .military_epoch
                .wrapping_mul(rules.military_transport_bonus),
        );
    }
    if input.unit_data_flags & 0x200 != 0 {
        speed = speed.wrapping_add(
            input
                .military_epoch
                .wrapping_mul(rules.americans_marine_speed_bonus),
        );
    }
    speed = rules.unit_move_speed.wrapping_mul(speed);

    if input.has_objmask_2000 && (leader.rare_effective.get(25) || leader.rare_b.get(25)) {
        speed = speed_percent(speed, rules.whales_ships_move);
    }

    // The four ObjectData::is calls are an else-if ladder. The emitted signed division
    // fixups are intentionally retained rather than simplified to positive-only ratios.
    speed = if input.is_68 {
        let product = speed.wrapping_mul(36);
        product.wrapping_add((product >> 31) & 31) >> 5
    } else if input.is_66 {
        let product = speed.wrapping_mul(34);
        product.wrapping_add((product >> 31) & 31) >> 5
    } else if input.is_64 {
        let product = speed.wrapping_shl(5);
        let divided = (product / 27).wrapping_add(product >> 31);
        divided.wrapping_sub(divided >> 31)
    } else if input.is_62 {
        let product = speed.wrapping_mul(30);
        let divided = (product / 24).wrapping_add(product >> 31);
        divided.wrapping_sub(divided >> 31)
    } else {
        speed
    };

    if (input.type_line == 0x1ab || input.type_id == 0x32 || input.type_id == 0x33 || input.is_42)
        && input.bantu
    {
        speed = speed_percent(speed, rules.bantu_units_move);
    }
    if input.type_line == 0x1ae || input.type_line == 0x1af {
        if input.french {
            speed = speed_percent(speed, rules.french_siege_move);
        }
        if input.versailles {
            speed = speed_percent(speed, rules.versailles_units_move);
        }
    }
    if input.domain == 2 && (leader.rare_effective.get(24) || leader.rare_b.get(24)) {
        speed = speed_percent(speed, rules.aluminum_air_speed);
    }
    if input.is_3a {
        speed = speed_quarter_upgrade(speed, input.spy_upgrade);
    }
    if input.hero {
        speed = speed_quarter_upgrade(speed, input.general_upgrade);
    }
    if input.supply {
        speed = speed_quarter_upgrade(speed, input.supply_upgrade);
    }
    if (input.type_id == 0x32 || input.type_id == 0x33) && input.aztec {
        speed = speed_percent(speed, rules.aztec_move_speed);
    }
    speed
}

/// Complete executed suffix of `Unit::update_speed` from the captain selected by
/// `Leader::calc_unit_stats`: derive and store signed `myspeed`, then copy it through the
/// captain's `o_down` chain.
pub fn unit_update_speed(
    units: &mut [StatObject],
    captain: usize,
    leader: &Leader,
    rules: &Step8Rules,
) -> Option<i32> {
    let input = units.get(captain)?.speed_inputs?;
    let stored = unit_speed(&input, leader, rules) as i16;

    let first_down = {
        let u = &mut units[captain];
        u.myspeed = stored;
        u.speed_written = true;
        u.unit_speed_updates = u.unit_speed_updates.wrapping_add(1);
        u.o_down
    };

    let mut next = first_down;
    let mut remaining = units.len();
    while let Some(index) = next {
        if remaining == 0 {
            return None;
        }
        remaining -= 1;
        let u = units.get_mut(index)?;
        u.myspeed = stored;
        u.speed_written = true;
        next = u.o_down;
    }
    Some(stored as i32)
}

/// `Unit::update_armor` `0x006054C0`, as reached by `Leader::calc_unit_stats` after its
/// `UnitData::is_captain` guard.
///
/// The function stores a signed 16-bit armor value on the captain, then follows `o_down`
/// and copies it to every subordinate. Retail's standalone entry first climbs `o_up` until
/// it reaches a captain; the caller here has already performed the exact captain test, so
/// this is the complete executed suffix from `0x00605510` through `0x006055AF`.
pub fn unit_update_armor(
    units: &mut [StatObject],
    captain: usize,
    leader: &Leader,
    rules: &Step8Rules,
) -> Option<i32> {
    let input = units.get(captain)?.armor_inputs?;
    let mut armor = object_data_armor(&input, leader, rules);
    if input.special_family_32_33 && (leader.rare_effective.get(31) || leader.rare_b.get(31)) {
        armor = armor.wrapping_add(rules.cattle_citizen_armor);
    }
    let stored = armor as i16;

    let first_down = {
        let u = &mut units[captain];
        u.myarmor = stored;
        u.armor_written = true;
        u.unit_armor_updates = u.unit_armor_updates.wrapping_add(1);
        u.o_down
    };

    let mut next = first_down;
    let mut remaining = units.len();
    while let Some(index) = next {
        if remaining == 0 {
            return None;
        }
        remaining -= 1;
        let u = units.get_mut(index)?;
        u.myarmor = stored;
        u.armor_written = true;
        next = u.o_down;
    }
    Some(stored as i32)
}

/// The reduction retail spells out seven times inside `Wall::update_construct_time`:
/// `imul eax, ebx, 0x64` / `add ecx, 0x64` / `xor edx, edx` / `div ecx`.
///
/// Every operand is a **u32** and the divide is `div`, not `idiv` — construction time is
/// unsigned everywhere in this function, unlike the signed `pct_scale` chain in
/// `Wall::update_hits`. The multiply is a 32-bit truncating `imul`, so it wraps.
///
/// A rule of exactly `-100` makes the divisor zero, which is a `#DE` fault in retail and
/// not a value the port may invent an answer for. It is refused, and the refusal is what
/// the caller charges.
#[inline]
fn construct_time_pct(v: u32, rule: i32) -> Option<u32> {
    v.wrapping_mul(100)
        .checked_div((rule as u32).wrapping_add(100))
}

/// `Wall::update_construct_time` `0x0063D560` (545 bytes), ported whole.
///
/// Retail calls it directly — not through a vtable — from **both** of
/// `Leader::calc_wall_stats`'s loops (`0x006CF838`, `0x006CF908`), for every active object
/// whose `WallData::is_active` answers zero. That is the under-construction predicate, so
/// this is the function that decides how long a building takes to go up, recomputed on
/// every stat-pass edge for every building still going up.
///
/// The chain, in emitted order. Nothing here is reassociated:
///
/// ```text
/// v = TypeData::time(who)                                        0x0063D56F
/// if maya && !is_wonder             v = v*100 / (MAYA_BUILDING_SPEED       + 100)
/// if has_preq(BUILDINGS_CREATED_FASTER)
///                                   v = (v*3) >> 2                0x0063D5CF
/// if has_wonder(VERSAILLES)         v = v*100 / (VERSAILLES_BUILDING_SPEED + 100)
/// if rare 13 (effective or B)       v = v*100 / (TOBACCO_BUILDING_SPEED    + 100)
/// if british && is_airdefense       v = v*100 / (BRITISH_AA_SPEED          + 100)
/// if dutch && is_fort && !is_wonder v = v*100 / (DUTCH_FORT_SPEED          + 100)
/// if roman && is_fort && !is_wonder v = v*100 / (ROMAN_FORT_SPEED          + 100)
/// if city_num == 0                  v = CAPITAL_BUILD_TIME * v / 100       0x0063D746
/// v = (10 - get_building_speed_upgrade()) * v / 10                         0x0063D767
/// this->constr_time = v                                                    0x0063D76F
/// ```
///
/// Four things a paraphrase gets wrong.
///
/// **The `!is_wonder` guards are three separate calls, not one hoisted flag.** Maya, Dutch
/// and Roman each re-issue vtable `+0x2C`; Versailles, Tobacco and the British do not
/// consult it at all. So a Wonder still gets the Versailles, Tobacco and airdefense
/// reductions.
///
/// **The last two steps divide by literals, not by a rule.** `0x0063D741` and `0x0063D762`
/// load `0x51EB851F` and `0xCCCCCCCD` and take the high half of an unsigned `mul` — the
/// standard unsigned magic divisions by 100 and by 10 — so `CAPITAL_BUILD_TIME` is a
/// multiplier over a fixed 100 and the upgrade step is over a fixed 10, in the opposite
/// direction from the six `RULE + 100` divisors above them.
///
/// **`CAPITAL_BUILD_TIME` has no type gate.** The XML text calls it "300% of normal *city*
/// build time (this is for nomad games only)", but `0x0063D72D` tests only
/// `LeaderData::city_num == 0` and then scales whatever building it was handed. Recorded
/// as measured; the name is the rule's, not the code's.
///
/// **The two rare-mask reads are `& 0x20` at payload byte 1**, `Leader + 0x6DA5` and
/// `+0x6DCD` (`0x0063D614`, `0x0063D623`) — bit 13 of the effective mask or of mask B.
/// They sit one byte below the `& 0x40` pair at `+0x6DA7`/`+0x6DCF` that
/// [`calc_anti_attrition`] reads for Titanium, which is the cross-check that both are
/// indexing the same 12-byte-header `BitMask<44>` payload.
pub fn wall_update_construct_time(
    o: &mut StatObject,
    leader: &Leader,
    rules: &Step8Rules,
) -> Option<u32> {
    let input = o.wall_construct_time_inputs?;
    let live = leader.unit_stats;
    let mut v = input.type_time;

    // has_tribe_bonus(1). The gated rule is named MAYA_BUILDING_SPEED, which is the
    // independent evidence that bonus index 1 is the Maya; the same holds for 6/0xB/0x16
    // below against ROMAN_FORT_SPEED / BRITISH_AA_SPEED / DUTCH_FORT_SPEED.
    if live.has_tribe_bonus(1) && !input.is_wonder {
        v = construct_time_pct(v, rules.maya_building_speed)?;
    }
    if leader.build_stats.buildings_created_faster {
        v = v.wrapping_mul(3) >> 2;
    }
    if live.versailles {
        v = construct_time_pct(v, rules.versailles_building_speed)?;
    }
    if leader.rare_effective.get(RareMask::TOBACCO) || leader.rare_b.get(RareMask::TOBACCO) {
        v = construct_time_pct(v, rules.tobacco_building_speed)?;
    }
    if live.has_tribe_bonus(0x0B) && input.is_airdefense {
        v = construct_time_pct(v, rules.british_aa_speed)?;
    }
    if live.has_tribe_bonus(0x16) && input.is_fort && !input.is_wonder {
        v = construct_time_pct(v, rules.dutch_fort_speed)?;
    }
    if live.has_tribe_bonus(6) && input.is_fort && !input.is_wonder {
        v = construct_time_pct(v, rules.roman_fort_speed)?;
    }
    if leader.city_num == 0 {
        v = (rules.capital_build_time as u32).wrapping_mul(v) / 100;
    }
    let upgrade = leader.build_stats.get_building_speed_upgrade();
    v = (10i32.wrapping_sub(upgrade) as u32).wrapping_mul(v) / 10;

    o.constr_time = v;
    o.construct_time_written = true;
    Some(v)
}

/// Result of the recovered `Wall::update_hits` override. The ejection call is exposed
/// separately because `Object::eject_contents` is its own 2,962-byte transaction.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WallHitOutcome {
    pub returned_hits: i32,
    pub eject_contents: bool,
}

/// `Wall::update_hits(int full)` `0x0063F0D0` (1,509 bytes), including the complete
/// percentage chain, construction ramp, stores, and ejection predicate.
pub fn wall_update_hits(
    o: &mut StatObject,
    rules: &Step8Rules,
    full: bool,
) -> Option<WallHitOutcome> {
    let input = o.wall_hit_inputs?;
    let mut hits = object_update_hits(o, full)?;

    if input.maya {
        hits = crate::systems::production::pct_scale(hits, rules.maya_building_hp);
    }
    if input.roman && (input.is_city_type || input.is_tower_1b7) && !input.is_wonder {
        hits = crate::systems::production::pct_scale(hits, rules.roman_fort_hp);
    }
    hits = crate::systems::production::pct_scale(
        hits,
        rules
            .building_hp_upgrade
            .wrapping_mul(input.building_hp_upgrade),
    );
    if input.taj_mahal {
        hits = crate::systems::production::pct_scale(hits, rules.taj_building_hp);
    }
    if input.red_fort && input.is_city_type && !input.is_wonder {
        hits = crate::systems::production::pct_scale(hits, rules.red_fort_fort_hps);
    }
    if input.is_fort_1b4 && input.nubian && rules.nubian_hit_points != 0 {
        hits = crate::systems::production::pct_scale(hits, rules.nubian_hit_points);
    }

    if o.wall_city_flag {
        if o.wall_active && input.linked_city && input.linked_city_is_capital {
            let upgrade = if input.has_preq_2cd {
                4usize
            } else {
                usize::try_from(input.capital_building_upgrade).ok()?
            };
            let mut bonus = *rules.capital_building_hp.get(upgrade)?;
            if input.tikal {
                bonus = rules
                    .tikal_temple_hp
                    .wrapping_add(100)
                    .wrapping_mul(bonus)
                    .wrapping_add(99)
                    .wrapping_div(100);
            }
            hits = crate::systems::production::pct_scale(hits, bonus);
            if input.ctw_mode && input.ctw_rare_20 {
                hits = crate::systems::production::pct_scale(hits, rules.ctw_missionaries_bonus);
            }
        }
    } else if input.noncapital_city_gate
        && o.wall_active
        && input.linked_city
        && !input.is_city_type
        && !input.is_tower_1b7
    {
        let additive = rules
            .senate_hp_bonus
            .wrapping_mul(input.linked_city_count.wrapping_sub(1))
            .wrapping_mul(hits)
            .wrapping_div(100);
        hits = hits.wrapping_add(additive);
    }

    o.myhits = hits;
    o.construct_hits = crate::systems::production::construct_hits(
        hits,
        o.wall_active,
        input.is_wonder,
        o.job_counter,
        o.constr_time,
    );
    o.wall_hits_written = true;
    o.wall_hits_updates = o.wall_hits_updates.wrapping_add(1);
    let eject_contents =
        o.construct_hits <= o.damage && o.inside_down >= 0 && !input.can_carry_domain_2;
    o.eject_contents_requested = eject_contents;
    Some(WallHitOutcome {
        returned_hits: if full { o.myhits } else { o.construct_hits },
        eject_contents,
    })
}

/// `Wall::update_los()` `0x0063EEB0` (544 bytes). All arithmetic that stores into
/// `ObjectData::mylos` is signed-byte wrapping, including the science product.
pub fn wall_update_los(o: &mut StatObject, leader: &Leader, rules: &Step8Rules) -> Option<i32> {
    let input = o.wall_los_inputs?;
    let mut los: i8;

    if !o.wall_started || (!o.wall_active && !input.is_wonder) {
        los = 0;
    } else {
        if !o.wall_active {
            los = 1;
        } else {
            los = if o.owner_in_game { o.type_los? } else { 0 };
            los = los.wrapping_add(input.science_level.wrapping_mul(input.type_science_los));

            let range_bonus = if input.is_city_type {
                let i = usize::try_from(input.city_range_upgrade).ok()?;
                *rules.fort_upgrade_range.get(i)?
            } else if input.is_tower_1b7 {
                let i = usize::try_from(input.tower_range_upgrade).ok()?;
                *rules.tower_fort_range.get(i)?
            } else {
                0
            };
            los = los.wrapping_add(range_bonus as i8);
            if (input.is_city_type || input.is_tower_1b7) && input.colosseum {
                los = los.wrapping_add(rules.colosseum_fort_range as i8);
            }
            if leader.rare_effective.get(15) || leader.rare_b.get(15) {
                los = los.wrapping_add(rules.furs_los as i8);
            }
        }
        los = los.wrapping_add((input.footprint_x / 2) as i8);
    }

    o.mylos = los;
    o.wall_los_written = true;
    o.wall_los_updates = o.wall_los_updates.wrapping_add(1);
    Some(los as i32)
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
fn build_band_pass(band: &mut [StatObject], leader: &Leader, rules: &Step8Rules) -> StatPassCounts {
    let mut c = StatPassCounts::default();
    for o in band.iter_mut() {
        c.visited += 1;
        if !o.active {
            continue;
        }
        c.active += 1;
        if !o.wall_active {
            // `Wall::update_construct_time` 0x0063D560 — the only non-virtual call in the
            // loop, and the one thing here that is a named retail function rather than a
            // vtable slot. It stores `constr_time` before `Wall::update_hits` below reads
            // it for the construction ramp, so the order of these two is load-bearing.
            o.construct_time_updates += 1;
            c.construct_time_updates += 1;
            if wall_update_construct_time(o, leader, rules).is_some() {
                c.construct_time_resolved += 1;
            } else {
                c.unresolved_calls += 1;
            }
        }
        o.v15c_calls += 1;
        match wall_update_hits(o, rules, false) {
            Some(outcome) => {
                c.wall_hits_updates += 1;
                if outcome.eject_contents {
                    c.eject_contents_calls += 1;
                    // The override and predicate ran; Object::eject_contents is a distinct
                    // 2,962-byte transaction and remains red only when actually reached.
                    c.unresolved_calls += 1;
                }
            }
            None => c.unresolved_calls += 1,
        }
        o.v160_calls += 1;
        if wall_update_los(o, leader, rules).is_some() {
            c.wall_los_updates += 1;
        } else {
            c.unresolved_calls += 1;
        }
    }
    c
}

/// The 3000 band is a plain `WallData` vtable: `+0x15C/+0x160` resolve to the base Object
/// implementations, unlike the building band's `Wall` overrides.
///
/// `Wall::update_construct_time` is **not** one of the differences. `0x006CF908` is a
/// direct `call 0x63d560`, byte-identical in intent to `0x006CF838` in the building loop,
/// so the same body runs here — with the `WallData` vtable's constant-zero `+0x2C`, which
/// is why a plain wall's [`WallConstructTimeInputs::is_wonder`] is always false.
fn wall_band_pass(band: &mut [StatObject], leader: &Leader, rules: &Step8Rules) -> StatPassCounts {
    let mut c = StatPassCounts::default();
    for o in band.iter_mut() {
        c.visited += 1;
        if !o.active {
            continue;
        }
        c.active += 1;
        if !o.wall_active {
            o.construct_time_updates += 1;
            c.construct_time_updates += 1;
            if wall_update_construct_time(o, leader, rules).is_some() {
                c.construct_time_resolved += 1;
            } else {
                c.unresolved_calls += 1;
            }
        }
        o.v15c_calls += 1;
        if object_update_hits(o, false).is_some() {
            c.object_hits_updates += 1;
        } else {
            c.unresolved_calls += 1;
        }
        o.v160_calls += 1;
        if object_update_los(o).is_some() {
            c.object_los_updates += 1;
        } else {
            c.unresolved_calls += 1;
        }
    }
    c
}

/// `Leader::calc_wall_stats` `0x006CF7C0` — re-derive building and wall stats for one owner.
///
/// Two loops with identical bodies over the 2000 and 3000 bands. The one asymmetry is real
/// and is preserved as a comment rather than as behaviour: the first loop fetches its guard
/// object through vtable slot `+0xAC` and the second through `+0xB0`, while both then use
/// `+0xB0` for the work. The distinction still matters after resolving the work object's
/// vtable, so it is recorded here rather than "tidied" away.
pub fn calc_wall_stats(
    leader: &Leader,
    rules: &Step8Rules,
    objs: &mut OwnerObjects,
) -> StatPassCounts {
    let mut c = build_band_pass(&mut objs.band_2000, leader, rules);
    c.add(wall_band_pass(&mut objs.band_3000, leader, rules));
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
    calc_unit_stats_with_source_and_type_overrides(leader, rules, gates, objs, None, &[])
}

/// `Leader::calc_unit_stats` with the canonical BHS-mutated `moves`/`armor` row projection.
/// Structural unit facts still come from the exact shipped query package; only fields whose
/// live `UnitTypeData` owner is mutable are substituted.
pub fn calc_unit_stats_with_type_overrides(
    leader: &mut Leader,
    rules: &Step8Rules,
    gates: &AttritionGates,
    objs: &mut OwnerObjects,
    type_overrides: &[UnitTypeStatOverride],
) -> StatPassCounts {
    calc_unit_stats_with_source_and_type_overrides(leader, rules, gates, objs, None, type_overrides)
}

/// Runtime-source form of [`calc_unit_stats_with_type_overrides`].
///
/// `type_source=None` is an intentional fail-closed state: automatic packages are cleared and
/// counted as misses, while explicitly supplied `speed_inputs`/`armor_inputs` remain usable.
pub fn calc_unit_stats_with_source_and_type_overrides(
    leader: &mut Leader,
    rules: &Step8Rules,
    gates: &AttritionGates,
    objs: &mut OwnerObjects,
    type_source: Option<&UnitTypeStatSource>,
    type_overrides: &[UnitTypeStatOverride],
) -> StatPassCounts {
    calc_attrition(leader, rules, gates);
    calc_anti_attrition(leader, rules, gates);

    let mut c = StatPassCounts::default();
    for i in 0..objs.units.len() {
        let u = &mut objs.units[i];
        c.visited += 1;
        if !u.active {
            continue;
        }
        c.active += 1;
        u.v160_calls += 1;
        if object_update_los(u).is_some() {
            c.object_los_updates += 1;
        } else {
            c.unresolved_calls += 1;
        }
        if u.captain {
            if let Some(source) = u.unit_query_source {
                // Clear first: an unknown replacement type must not replay a package
                // generated on the previous dirty edge.
                u.speed_inputs = None;
                u.armor_inputs = None;
                if let Some((mut speed, mut armor)) =
                    type_source.and_then(|table| derive_unit_query_packages(table, source, leader))
                {
                    if let Some(override_row) = type_overrides
                        .iter()
                        .find(|row| row.type_id == source.type_id)
                    {
                        speed.type_moves = override_row.moves;
                        armor.type_armor = override_row.armor;
                    }
                    u.speed_inputs = Some(speed);
                    u.armor_inputs = Some(armor);
                    u.unit_query_populations = u.unit_query_populations.wrapping_add(1);
                    c.unit_query_populations += 1;
                } else {
                    u.unit_query_misses = u.unit_query_misses.wrapping_add(1);
                    c.unit_query_misses += 1;
                }
            }
            u.v15c_calls += 1;
            if object_update_hits(u, false).is_some() {
                c.object_hits_updates += 1;
            } else {
                c.unresolved_calls += 1;
            }
            u.speed_updates += 1;
            u.armor_updates += 1;
            c.speed_updates += 1;
            c.armor_updates += 1;
            if unit_update_speed(&mut objs.units, i, leader, rules).is_some() {
                c.unit_speed_updates += 1;
            } else {
                c.unresolved_calls += 1;
            }
            if unit_update_armor(&mut objs.units, i, leader, rules).is_some() {
                c.unit_armor_updates += 1;
            } else {
                c.unresolved_calls += 1;
            }
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
    /// Validated local retail-derived input. It is absent in redistributable source builds and
    /// must be installed explicitly by the product host before automatic Unit stat queries run.
    pub(crate) unit_type_stats: Option<UnitTypeStatSource>,
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
    let unit_type_stats = env.unit_type_stats.as_ref();

    for i in 0..NUM_LEADER_SLOTS {
        // 0x006ED2B0 — the outer gate is bit 1, not bit 0.
        if ls.leaders[i].flags & flag::PROCESS == 0 {
            continue;
        }
        trace.processed[i] = true;

        // 0x006ED2B9 / 0x006ED2CA / 0x006ED2D4.
        ls.leaders[i].flags &= !flag::HOSTILE_SEEN;
        ls.leaders[i].pop_issues = 0;
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
            trace.wall_pass[i] = calc_wall_stats(&ls.leaders[i], rules, &mut e.objects);
            trace.wall_stats_ran[i] = true;
        }
        // 0x006ED341.
        if ls.leaders[i].flags & flag::UNIT_STATS_DIRTY != 0 {
            ls.leaders[i].flags &= !flag::UNIT_STATS_DIRTY;
            trace.unit_pass[i] = calc_unit_stats_with_source_and_type_overrides(
                &mut ls.leaders[i],
                rules,
                &e.attrition,
                &mut e.objects,
                unit_type_stats,
                &[],
            );
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
// Leader::check_explore 0x006BC860 / Leaders::strategy_all 0x006ED430
// ===========================================================================================

/// The unnamed `BonusType` queried by `Leader::check_explore` through
/// `LeaderData::has_preq`. Its one captured prerequisite is Electronics (`556`).
pub const EXPLORE_ALL_BONUS: i32 = 0x2B2;
pub const EXPLORE_ALL_PREREQ: usize = 556;
/// `mov eax, 200` at `0x006BC87E`; divided by `GameAccess::ai_speed`.
pub const EXPLORE_PERIOD_BASE: i32 = 200;
/// Each leader's recount phase advances by 25 frames (`imul ..., 0x19`).
pub const EXPLORE_SLOT_PHASE: i32 = 25;
/// The final `+ 12` in the dividend at `0x006BC890`.
pub const EXPLORE_PHASE_BIAS: i32 = 12;

/// The exact WorldData view consumed by `Leader::check_explore`.
///
/// Retail walks region coordinates (`WorldData +0x24/+0x28`) but samples the persistent
/// fog `seen2` plane at `(4*x + 3, 4*y + 3)`. Keeping the dimensions beside the borrowed
/// plane makes an incomplete host adapter explicit instead of indexing invented data.
#[derive(Clone, Copy, Debug)]
pub struct ExploreWorld<'a> {
    pub reg_xs: i32,
    pub reg_ys: i32,
    pub reg_size: i32,
    pub fog_xs: i32,
    pub seen2: &'a [u8],
}

/// What one reached `Leader::check_explore` call did.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ExploreUpdate {
    /// The player-specific phase did not fire; retail leaves `Leader +0x9D4` untouched.
    #[default]
    NotDue,
    /// The Electronics/bonus prerequisite makes the whole region grid explored.
    FullMap(i32),
    /// The sampled bits in `WorldData::seen2` were counted.
    Recounted(i32),
    /// A host supplied a zero/overflowing AI period or an incomplete `seen2` plane.
    /// Retail's World and `ai_speed` invariants exclude this state.
    MissingFacts,
}

/// Inputs read by the 93-byte `Leaders::strategy_all` dispatcher and its recovered first
/// child. `has_explore_preq` is the decoded result of retail's `has_preq(0x2B2)` call; the
/// shipped type row has the single prerequisite Electronics (`556`). `None` keeps an
/// incomplete host adapter red instead of manufacturing a negative answer.
#[derive(Clone, Copy, Debug)]
pub struct StrategyInputs<'a> {
    pub frame: i32,
    pub ai_speed: i32,
    pub world: ExploreWorld<'a>,
    pub has_explore_preq: [Option<bool>; NUM_LEADER_SLOTS],
    /// Game semaphore bit 9 (`game[0x821] & 2`). It gates the tail call independently of
    /// whether any leader passed the loop gate.
    pub check_victory_mode: bool,
}

/// One call boundary in `Leaders::strategy_all`, in exact retail order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StrategyCall {
    CheckExplore { slot: usize, update: ExploreUpdate },
    PlanStrategy(usize),
    ComputeScore { slot: usize, force: i32 },
    Diplomacy(usize),
    CheckVictory,
}

/// Measured execution of step 11. The two giant AI bodies remain represented by reached
/// calls, while the dispatcher, exploration recount, score boundary and victory gate run.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StrategyTrace {
    pub processed: [bool; NUM_LEADER_SLOTS],
    pub explore: [ExploreUpdate; NUM_LEADER_SLOTS],
    pub calls: Vec<StrategyCall>,
}

impl StrategyTrace {
    pub fn leaders_processed(&self) -> usize {
        self.processed.iter().filter(|ran| **ran).count()
    }

    pub fn unresolved_explore(&self) -> usize {
        self.explore
            .iter()
            .filter(|update| **update == ExploreUpdate::MissingFacts)
            .count()
    }
}

/// `Leader::check_explore` `0x006BC860`, the whole 227-byte body.
///
/// Frame zero always recomputes. Later frames use
/// `(leader.slot * 25 + frame + 12) % (200 / ai_speed) == 0`. The world scan samples one
/// `seen2` byte per region cell, specifically fog coordinate `(4*x+3, 4*y+3)`, and tests
/// the low byte of x86's `1 << (slot & 31)` mask.
pub fn check_explore(
    leader: &mut Leader,
    frame: i32,
    ai_speed: i32,
    has_explore_preq: Option<bool>,
    world: ExploreWorld<'_>,
) -> ExploreUpdate {
    if frame != 0 {
        if ai_speed == 0 {
            return ExploreUpdate::MissingFacts;
        }
        let period = EXPLORE_PERIOD_BASE / ai_speed;
        if period == 0 {
            return ExploreUpdate::MissingFacts;
        }
        let phase = leader
            .slot
            .wrapping_mul(EXPLORE_SLOT_PHASE)
            .wrapping_add(frame)
            .wrapping_add(EXPLORE_PHASE_BIAS);
        if phase % period != 0 {
            return ExploreUpdate::NotDue;
        }
    }

    let Some(has_explore_preq) = has_explore_preq else {
        return ExploreUpdate::MissingFacts;
    };
    if has_explore_preq {
        leader.explored = world.reg_size;
        return ExploreUpdate::FullMap(leader.explored);
    }

    let player_mask = (1u32 << ((leader.slot as u32) & 31)) as u8;
    let mut explored = 0i32;
    for y in 0..world.reg_ys.max(0) {
        let fog_y = y.wrapping_mul(4).wrapping_add(3);
        for x in 0..world.reg_xs.max(0) {
            let fog_x = x.wrapping_mul(4).wrapping_add(3);
            let index = world.fog_xs.wrapping_mul(fog_y).wrapping_add(fog_x);
            let Some(&bits) = usize::try_from(index)
                .ok()
                .and_then(|index| world.seen2.get(index))
            else {
                return ExploreUpdate::MissingFacts;
            };
            if bits & player_mask != 0 {
                explored = explored.wrapping_add(1);
            }
        }
    }
    leader.explored = explored;
    ExploreUpdate::Recounted(explored)
}

/// **Step 11 of `Game::do_frame`.** `Leaders::strategy_all` `0x006ED430`, recovered in
/// full at the dispatcher level. A leader enters only when `(flags & 3) == 3`. Calls are
/// emitted in their instruction order so the tick can execute the existing score/victory
/// ports at the exact boundary and charge only the two unresolved AI bodies.
pub fn strategy_all(ls: &mut Leaders, input: StrategyInputs<'_>) -> StrategyTrace {
    let mut trace = StrategyTrace::default();

    for slot in 0..NUM_LEADER_SLOTS {
        if ls.leaders[slot].flags & (flag::IN_GAME | flag::PROCESS)
            != (flag::IN_GAME | flag::PROCESS)
        {
            continue;
        }
        trace.processed[slot] = true;

        let update = check_explore(
            &mut ls.leaders[slot],
            input.frame,
            input.ai_speed,
            input.has_explore_preq[slot],
            input.world,
        );
        trace.explore[slot] = update;
        trace
            .calls
            .push(StrategyCall::CheckExplore { slot, update });
        trace.calls.push(StrategyCall::PlanStrategy(slot));
        trace
            .calls
            .push(StrategyCall::ComputeScore { slot, force: 0 });
        trace.calls.push(StrategyCall::Diplomacy(slot));
    }

    if input.check_victory_mode {
        trace.calls.push(StrategyCall::CheckVictory);
    }
    trace
}

// ===========================================================================================
// Leaders::end_process_all 0x006ED070
// ===========================================================================================

/// One `GameInfo::Player::flags &= 0xf7ff` write from the zero-issues branch.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PopulationWarningClear {
    pub leader_index: usize,
    pub leader_who: i32,
    pub player_index: usize,
}

/// One write to `GameInfo::Player::pop_cap_frame` from the rate-limit branch.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PopulationCapStamp {
    pub leader_index: usize,
    pub player_index: usize,
    pub frame: i32,
}

/// The presentation boundary reached after a due, below-limit population-production
/// failure. Retail adds localized feedback then plays [`POP_CAP_WARNING_SOUND`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PopulationCapFeedback {
    pub leader_index: usize,
    pub leader_who: i32,
    pub player_index: usize,
    pub text_offset: u32,
    pub sound_category: i32,
}

/// Host facts that the retail engine guarantees but a lightweight simulation may omit.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EndProcessMissingFact {
    LocalPlayerIndex(i32),
    PopulationLimitIndex(u8),
}

/// Measured execution of step 17. The dispatcher and every checksum-relevant write run;
/// UI/audio are emitted as inspectable [`PopulationCapFeedback`] events.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EndProcessTrace {
    pub processed: [bool; NUM_LEADER_SLOTS],
    pub warning_clears: Vec<PopulationWarningClear>,
    pub stamp_writes: Vec<PopulationCapStamp>,
    pub feedback: Vec<PopulationCapFeedback>,
    pub missing_facts: Vec<EndProcessMissingFact>,
}

impl EndProcessTrace {
    pub fn leaders_processed(&self) -> usize {
        self.processed.iter().filter(|ran| **ran).count()
    }
}

/// **Step 17 of `Game::do_frame`.** `Leaders::end_process_all` `0x006ED070`, the full
/// 549-byte dispatcher and its inlined per-leader body.
///
/// Processed leaders with no production failures scan all eight Player records and clear
/// warning bit `0x800` on valid records whose byte `who` matches `Leader +0x08`. A failed
/// local leader instead rate-limits feedback: signed wrapping `frame - pop_cap_frame` must
/// be greater than `0x1c1`. Retail stamps the frame *before* checking the selected cap, so
/// this implementation does too, including when the leader has already reached that cap.
pub fn end_process_all(ls: &mut Leaders, frame: i32) -> EndProcessTrace {
    let mut trace = EndProcessTrace::default();
    ls.end.last_feedback.clear();

    for leader_index in 0..NUM_LEADER_SLOTS {
        let leader = &ls.leaders[leader_index];
        if leader.flags & flag::PROCESS == 0 {
            continue;
        }
        trace.processed[leader_index] = true;

        let leader_who = leader.slot;
        let pop_issues = leader.pop_issues;
        let pop_cap = leader.pop_cap;

        if pop_issues == 0 {
            for player_index in 0..NUM_LEADER_SLOTS {
                let player = &mut ls.end.players[player_index];
                if player.flags & PLAYER_VALID != 0 && i32::from(player.who) == leader_who {
                    player.flags &= !PLAYER_POP_CAP_WARNING;
                    trace.warning_clears.push(PopulationWarningClear {
                        leader_index,
                        leader_who,
                        player_index,
                    });
                }
            }
            continue;
        }

        if leader_who != ls.end.local_who {
            continue;
        }
        let Ok(player_index) = usize::try_from(ls.end.local_play) else {
            trace
                .missing_facts
                .push(EndProcessMissingFact::LocalPlayerIndex(ls.end.local_play));
            continue;
        };
        let Some(player) = ls.end.players.get_mut(player_index) else {
            trace
                .missing_facts
                .push(EndProcessMissingFact::LocalPlayerIndex(ls.end.local_play));
            continue;
        };
        if frame.wrapping_sub(player.pop_cap_frame) < POP_CAP_WARNING_MIN_ELAPSED {
            continue;
        }

        // 0x006ED1F0 — this store precedes the population-limit table read/compare.
        player.pop_cap_frame = frame;
        trace.stamp_writes.push(PopulationCapStamp {
            leader_index,
            player_index,
            frame,
        });

        let Some(&pop_limit) = SHIPPED_POP_LIMITS.get(usize::from(ls.end.pop_limit_index)) else {
            trace
                .missing_facts
                .push(EndProcessMissingFact::PopulationLimitIndex(
                    ls.end.pop_limit_index,
                ));
            continue;
        };
        if pop_cap >= pop_limit {
            continue;
        }

        let event = PopulationCapFeedback {
            leader_index,
            leader_who,
            player_index,
            text_offset: POP_CAP_WARNING_TEXT_OFFSET,
            sound_category: POP_CAP_WARNING_SOUND,
        };
        trace.feedback.push(event);
        ls.end.last_feedback.push(event);
    }

    trace
}

// ===========================================================================================
// Leader::process_event_frame 0x006EC180 / Game::do_frame step-19 dispatcher
// ===========================================================================================

pub const EVENT_RATE_PERIOD: i32 = 50;
pub const EVENT_RATE_SCALE: u16 = 100;
pub const COMBAT_MOOD_QUIET_THRESHOLD: u32 = 300;
pub const COMBAT_MOOD_ACTIVE_THRESHOLD: u32 = 600;
pub const COMBAT_MOOD_FORCE_THRESHOLD: u32 = 2000;
pub const COMBAT_SCORE_BIAS: i32 = 200;
pub const BATTLE_EVENT_COOLDOWN: i32 = 1800;
pub const BATTLE_RATE_PER_AGE: i32 = 125;
pub const BATTLE_IMBALANCE_PER_AGE: i32 = 10;
pub const BATTLE_IMBALANCE_BASE: i32 = 20;
pub const BATTLE_RATE_SENTINEL: u16 = 0xFC18;

pub mod combat_mood {
    pub const WINNING: i32 = 0;
    pub const LOSING: i32 = 1;
    pub const QUIET: i32 = 2;
}

/// Exact non-Leader inputs reached by the step-19 body. Ages are indexed by player `who`;
/// team scores are indexed by Leader record because retail calls `get_team_score` on each
/// concrete Leader object. `None` leaves a broken who→age adapter explicit.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EventFrameInputs {
    pub frame: i32,
    pub age_by_who: [Option<i32>; NUM_LEADER_SLOTS],
    /// Exact `LeaderData::get_team_score` answers keyed by Leader record. A missing answer
    /// suppresses the dependent mood mutation and becomes a typed residual.
    pub team_scores: [Option<i32>; NUM_LEADER_SLOTS],
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EventFrameMissingFact {
    AgeForWho(i32),
    LocalLeaderSlot(i32),
    OtherLeaderSlot { leader_index: usize, who: i32 },
    TeamScoreForLeader(usize),
}

/// Measured execution of the complete step-19 dispatcher and deterministic body.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EventFrameTrace {
    /// Passed `Game::do_frame`'s `flags & 1` gate.
    pub dispatched: [bool; NUM_LEADER_SLOTS],
    /// Passed the body's `frame % 50 == 0` gate.
    pub due: [bool; NUM_LEADER_SLOTS],
    pub mood_requests: Vec<CombatMoodRequest>,
    pub achievement_events: Vec<BattleAchievementEvent>,
    pub missing_facts: Vec<EventFrameMissingFact>,
    /// Receipt-complete reconstruction trace. This retains the precise ordering between
    /// deterministic Leader writes, product reads, unresolved host tails and residuals.
    pub exact: step19::Step19Trace,
}

impl EventFrameTrace {
    pub fn leaders_dispatched(&self) -> usize {
        self.dispatched.iter().filter(|ran| **ran).count()
    }

    pub fn leaders_due(&self) -> usize {
        self.due.iter().filter(|ran| **ran).count()
    }
}

/// **Step 19 of `Game::do_frame`.** The exact eight-Leader `flags & 1` dispatcher plus the
/// complete 918-byte `Leader::process_event_frame` body.
///
/// JukeBox and Achieve calls are presentation/product boundaries, emitted as typed events;
/// all LeaderData additions, smoothing, sentinels, cooldown stamps and counter resets run
/// directly in retail order. Because the dispatcher is sequential, a local leader's music
/// scan observes freshly folded hit/damage rates only for earlier Leader slots, exactly as
/// the original loop does.
pub fn process_event_frames(ls: &mut Leaders, input: EventFrameInputs) -> EventFrameTrace {
    ls.event.product_outbox.last_calls.clear();
    ls.event.last_mood_requests.clear();
    ls.event.last_achievement_events.clear();
    let mut exact_state = step19::Step19State {
        leaders: std::array::from_fn(|leader_index| {
            let leader = &ls.leaders[leader_index];
            step19::LeaderSlot {
                flags: leader.flags,
                who: leader.slot,
                diplos: leader.diplo,
                event_queue: step19::EventQueueState {
                    frame_battle: leader.event_frame.frame_battle,
                    average_death_rate: leader.event_frame.average_death_rate,
                    average_kill_rate: leader.event_frame.average_kill_rate,
                    average_damage_rate: leader.event_frame.average_damage_rate,
                    average_hit_rate: leader.event_frame.average_hit_rate,
                    deaths_current_frame: leader.event_frame.deaths_current_frame,
                    kills_current_frame: leader.event_frame.kills_current_frame,
                    hits_current_frame: leader.event_frame.hits_current_frame,
                    damage_current_frame: leader.event_frame.damage_current_frame,
                    deaths_fifteen_seconds: leader.event_frame.deaths_fifteen_seconds,
                    kills_fifteen_seconds: leader.event_frame.kills_fifteen_seconds,
                    hits_fifteen_seconds: leader.event_frame.hits_fifteen_seconds,
                    damage_fifteen_seconds: leader.event_frame.damage_fifteen_seconds,
                },
            }
        }),
        music: step19::MusicState {
            current_mood: ls.event.current_music_mood,
            next_mood: ls.event.next_music_mood,
        },
    };
    let exact = step19::execute_step19(
        &mut exact_state,
        step19::ProductFacts {
            frame: input.frame,
            console_who: ls.end.local_who,
            team_scores: input.team_scores,
            encrypted_ages: input
                .age_by_who
                .map(|age| age.map(|age| (age as u32) ^ step19::AGES_XOR_KEY)),
        },
    );

    // The exact executor owns only the recovered event block. Publish every deterministic
    // mutation back into the canonical Leader records as one adapter transaction.
    for leader_index in 0..NUM_LEADER_SLOTS {
        let queue = exact_state.leaders[leader_index].event_queue;
        ls.leaders[leader_index].event_frame = EventFrameState {
            frame_battle: queue.frame_battle,
            average_death_rate: queue.average_death_rate,
            average_kill_rate: queue.average_kill_rate,
            average_damage_rate: queue.average_damage_rate,
            average_hit_rate: queue.average_hit_rate,
            deaths_current_frame: queue.deaths_current_frame,
            kills_current_frame: queue.kills_current_frame,
            hits_current_frame: queue.hits_current_frame,
            damage_current_frame: queue.damage_current_frame,
            deaths_fifteen_seconds: queue.deaths_fifteen_seconds,
            kills_fifteen_seconds: queue.kills_fifteen_seconds,
            hits_fifteen_seconds: queue.hits_fifteen_seconds,
            damage_fifteen_seconds: queue.damage_fifteen_seconds,
        };
    }
    ls.event.next_music_mood = exact_state.music.next_mood;

    let mut trace = EventFrameTrace::default();
    for visit in &exact.visits {
        match visit.outcome {
            step19::LeaderOutcome::NotVisited | step19::LeaderOutcome::InGameFlagClear => {}
            step19::LeaderOutcome::FrameNotDue => {
                trace.dispatched[visit.leader_index] = true;
            }
            step19::LeaderOutcome::Due { .. } => {
                trace.dispatched[visit.leader_index] = true;
                trace.due[visit.leader_index] = true;
            }
        }
    }
    for tail in &exact.host_tails {
        // Delivery into the installed headless product owner happens in the exact executor's
        // one sequence domain. The decoded vectors below are convenience projections only.
        ls.event.product_outbox.last_calls.push(*tail);
        match tail.host_tail {
            step19::HostTail::JukeBoxSetNextMood {
                requested_mood,
                force,
                ..
            } => {
                let request = CombatMoodRequest {
                    leader_index: tail.leader_index,
                    mood: requested_mood,
                    force,
                };
                trace.mood_requests.push(request);
                ls.event.last_mood_requests.push(request);
            }
            step19::HostTail::AchieveAddEvent { kind, who, .. } => {
                let event = BattleAchievementEvent {
                    leader_index: tail.leader_index,
                    who,
                    kind: match kind {
                        step19::BattleEventKind::KillsOverDeaths => {
                            BattleAchievementKind::KillsOverDeaths
                        }
                        step19::BattleEventKind::DeathsOverKills => {
                            BattleAchievementKind::DeathsOverKills
                        }
                    },
                };
                trace.achievement_events.push(event);
                ls.event.last_achievement_events.push(event);
            }
        }
    }
    trace.missing_facts = exact
        .residuals
        .iter()
        .map(|receipt| match receipt.residual {
            step19::OpenResidual::LeaderWhoOutsideArray { who } => {
                EventFrameMissingFact::LocalLeaderSlot(who)
            }
            step19::OpenResidual::OtherWhoOutsideArray {
                other_leader_index,
                who,
            } => EventFrameMissingFact::OtherLeaderSlot {
                leader_index: other_leader_index,
                who,
            },
            step19::OpenResidual::TeamScoreUnavailable {
                queried_leader_index,
            } => EventFrameMissingFact::TeamScoreForLeader(queried_leader_index),
            step19::OpenResidual::EncryptedAgeUnavailable { who_index } => {
                EventFrameMissingFact::AgeForWho(who_index as i32)
            }
        })
        .collect();
    trace.exact = exact;
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

    fn unit_type_stat_source_fixture() -> UnitTypeStatSource {
        let mut rows = vec![None; UNIT_TYPE_STAT_END as usize];
        let mut insert = |row: UnitTypeStatRow| rows[row.type_id as usize] = Some(row);
        insert(UnitTypeStatRow {
            type_id: 50,
            from: -1,
            where_type: 414,
            moves: 25,
            ..Default::default()
        });
        insert(UnitTypeStatRow {
            type_id: 59,
            from: -1,
            where_type: 427,
            moves: 26,
            ..Default::default()
        });
        insert(UnitTypeStatRow {
            type_id: 100,
            from: -1,
            where_type: 427,
            moves: 32,
            armor: 4,
            ..Default::default()
        });
        insert(UnitTypeStatRow {
            type_id: 103,
            from: 100,
            graft: 102,
            where_type: 427,
            moves: 32,
            armor: 4,
            ..Default::default()
        });
        UnitTypeStatSource {
            provenance: UnitTypeStatSourceProvenance {
                executable_sha256: super::super::unit_inctime::SUPPORTED_RETAIL_EXE_SHA256,
                table_sha256: SUPPORTED_UNIT_TYPE_STAT_TSV_SHA256,
            },
            rows: rows.into_boxed_slice(),
        }
    }

    fn synthetic_live_tsv() -> String {
        let mut tsv = String::from(
            "type_id\tfrom\twhere\tobj_masks\tarmor\tdomain\tgraft\tunit_flags\tunit_flags2\tmoves\n",
        );
        for type_id in UNIT_TYPE_STAT_FIRST..UNIT_TYPE_STAT_END {
            tsv.push_str(&format!("{type_id}\t-1\t414\t0\t0\t0\t-1\t0\t0\t25\n"));
        }
        tsv
    }

    fn supported_source_provenance() -> UnitTypeStatSourceProvenance {
        UnitTypeStatSourceProvenance {
            executable_sha256: super::super::unit_inctime::SUPPORTED_RETAIL_EXE_SHA256,
            table_sha256: SUPPORTED_UNIT_TYPE_STAT_TSV_SHA256,
        }
    }

    #[test]
    fn runtime_unit_type_source_is_strict_and_generation_bound() {
        let tsv = synthetic_live_tsv();
        let source = UnitTypeStatSource::from_live_tsv(&tsv, supported_source_provenance())
            .expect("closed 364-row fixture");
        assert_eq!(source.rows.iter().filter(|row| row.is_some()).count(), 364);
        assert_eq!(source.row(50).unwrap().moves, 25);
        assert!(source.row(49).is_none());

        let mut wrong = supported_source_provenance();
        wrong.table_sha256[0] ^= 1;
        assert_eq!(
            UnitTypeStatSource::from_live_tsv(&tsv, wrong),
            Err(UnitTypeStatSourceError::UnsupportedTable)
        );

        let short = tsv.lines().take(364).collect::<Vec<_>>().join("\n");
        assert_eq!(
            UnitTypeStatSource::from_live_tsv(&short, supported_source_provenance()),
            Err(UnitTypeStatSourceError::WrongRowCount { got: 363 })
        );
    }

    #[test]
    fn local_supported_capture_is_consumed_only_at_runtime() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../schema/live/live-tables-unit.tsv");
        let Ok(tsv) = std::fs::read_to_string(&path) else {
            eprintln!("skipping local retail-derived input: {}", path.display());
            return;
        };
        let source = UnitTypeStatSource::from_live_tsv(&tsv, supported_source_provenance())
            .expect("supported local post-load capture");
        let row = source.row(103).unwrap();
        assert_eq!(
            (row.from, row.graft, row.where_type, row.moves, row.armor),
            (100, 102, 427, 32, 4)
        );
    }

    #[test]
    fn absent_unit_type_source_clears_packages_and_preserves_walked_stats() {
        let mut leader = Leader::new(0);
        let mut objects = OwnerObjects {
            units: vec![StatObject {
                active: true,
                captain: true,
                owner_in_game: true,
                hit_inputs: Some(ObjectHitInputs {
                    base_hits: 100,
                    ..Default::default()
                }),
                type_los: Some(3),
                unit_query_source: Some(UnitQuerySource {
                    type_id: 50,
                    unit_masks2: 0,
                }),
                speed_inputs: Some(UnitSpeedInputs::default()),
                armor_inputs: Some(UnitArmorInputs::default()),
                myspeed: 91,
                myarmor: 92,
                ..Default::default()
            }],
            ..Default::default()
        };

        let counts = calc_unit_stats(
            &mut leader,
            &Step8Rules::shipped(),
            &AttritionGates::default(),
            &mut objects,
        );
        let unit = &objects.units[0];
        assert_eq!((unit.myspeed, unit.myarmor), (91, 92));
        assert!(unit.speed_inputs.is_none() && unit.armor_inputs.is_none());
        assert!(!unit.speed_written && !unit.armor_written);
        assert_eq!(counts.unit_query_populations, 0);
        assert_eq!(counts.unit_query_misses, 1);
        assert_eq!(counts.unresolved_calls, 2);
    }

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

    fn strategy_input<'a>(seen2: &'a [u8]) -> StrategyInputs<'a> {
        StrategyInputs {
            frame: 1,
            ai_speed: 1,
            world: ExploreWorld {
                reg_xs: 2,
                reg_ys: 2,
                reg_size: 4,
                fog_xs: 8,
                seen2,
            },
            has_explore_preq: [Some(false); NUM_LEADER_SLOTS],
            check_victory_mode: false,
        }
    }

    /// `0x006ED440` gates on both low bits and the four calls stay interleaved per slot.
    #[test]
    fn strategy_all_uses_flags_3_and_preserves_exact_call_order() {
        let seen2 = [0u8; 64];
        let mut ls = Leaders::new();
        ls.leaders[0].flags = flag::IN_GAME;
        ls.leaders[1].flags = flag::PROCESS;
        ls.leaders[2].activate();
        ls.leaders[4].activate();

        let mut input = strategy_input(&seen2);
        input.check_victory_mode = true;
        let trace = strategy_all(&mut ls, input);

        assert_eq!(trace.leaders_processed(), 2);
        assert_eq!(
            trace.calls,
            vec![
                StrategyCall::CheckExplore {
                    slot: 2,
                    update: ExploreUpdate::NotDue,
                },
                StrategyCall::PlanStrategy(2),
                StrategyCall::ComputeScore { slot: 2, force: 0 },
                StrategyCall::Diplomacy(2),
                StrategyCall::CheckExplore {
                    slot: 4,
                    update: ExploreUpdate::NotDue,
                },
                StrategyCall::PlanStrategy(4),
                StrategyCall::ComputeScore { slot: 4, force: 0 },
                StrategyCall::Diplomacy(4),
                StrategyCall::CheckVictory,
            ]
        );
    }

    /// The phase is `(slot*25 + frame + 12) % (200/ai_speed)`. For slot 2, frame 138
    /// is due. The scan samples fog coordinates (3,3), (7,3), (3,7), (7,7).
    #[test]
    fn check_explore_recounts_the_exact_seen2_samples_on_its_phase() {
        let mut seen2 = [0u8; 64];
        for index in [27usize, 31, 59] {
            seen2[index] |= 1 << 2;
        }
        seen2[63] |= 1 << 1;
        let world = strategy_input(&seen2).world;
        let mut leader = Leader::new(2);
        leader.explored = 91;

        assert_eq!(
            check_explore(&mut leader, 137, 1, Some(false), world),
            ExploreUpdate::NotDue
        );
        assert_eq!(leader.explored, 91);
        assert_eq!(
            check_explore(&mut leader, 138, 1, Some(false), world),
            ExploreUpdate::Recounted(3)
        );
        assert_eq!(leader.explored, 3);
    }

    /// Frame zero bypasses the period calculation, the prerequisite chooses `reg_size`,
    /// and replacing the fog plane refreshes the value rather than replaying the prior one.
    #[test]
    fn check_explore_frame_zero_full_map_and_refresh_are_mutation_pinned() {
        let mut leader = Leader::new(3);
        let full_seen = [1 << 3; 64];
        let empty_seen = [0u8; 64];

        let full_world = strategy_input(&full_seen).world;
        assert_eq!(
            check_explore(&mut leader, 0, 0, Some(true), full_world),
            ExploreUpdate::FullMap(4),
            "frame zero never divides by ai_speed"
        );
        assert_eq!(leader.explored, 4);

        let empty_world = strategy_input(&empty_seen).world;
        assert_eq!(
            check_explore(&mut leader, 0, 1, Some(false), empty_world),
            ExploreUpdate::Recounted(0)
        );
        assert_eq!(leader.explored, 0, "the old full-map value cannot replay");

        leader.explored = 17;
        let missing_world = ExploreWorld {
            seen2: &empty_seen[..8],
            ..empty_world
        };
        assert_eq!(
            check_explore(&mut leader, 0, 1, Some(false), missing_world),
            ExploreUpdate::MissingFacts
        );
        assert_eq!(leader.explored, 17, "incomplete host state is fail-closed");

        assert_eq!(
            check_explore(&mut leader, 0, 1, None, empty_world),
            ExploreUpdate::MissingFacts,
            "an absent has_preq answer cannot be treated as false"
        );
        assert_eq!(leader.explored, 17);
    }

    /// The tail semaphore is outside the leader loop; it fires even with no active slots.
    #[test]
    fn strategy_all_victory_gate_is_independent_of_active_leaders() {
        let seen2 = [0u8; 64];
        let mut ls = Leaders::new();
        let mut input = strategy_input(&seen2);
        input.check_victory_mode = true;
        let trace = strategy_all(&mut ls, input);
        assert_eq!(trace.leaders_processed(), 0);
        assert_eq!(trace.calls, vec![StrategyCall::CheckVictory]);
    }

    /// The host adapter mirrors only facts already present in Leader state. Warning bits,
    /// timestamps, and unrelated Player flags remain owned by the end-process state.
    #[test]
    fn end_process_player_sync_preserves_warning_state_and_refreshes_identity() {
        let mut ls = Leaders::new();
        ls.leaders[0].slot = 7;
        ls.leaders[0].flags = flag::IN_GAME;
        ls.end.players[0].flags = PLAYER_POP_CAP_WARNING | 0x4000;
        ls.end.players[0].pop_cap_frame = 123;
        ls.end.players[1].flags = PLAYER_VALID | PLAYER_POP_CAP_WARNING;

        ls.sync_end_players_from_leaders();

        assert_eq!(ls.end.players[0].who, 7);
        assert_eq!(
            ls.end.players[0].flags,
            PLAYER_VALID | PLAYER_POP_CAP_WARNING | 0x4000
        );
        assert_eq!(ls.end.players[0].pop_cap_frame, 123);
        assert_eq!(
            ls.end.players[1].flags, PLAYER_POP_CAP_WARNING,
            "an inactive leader clears only Player::VALID"
        );
    }

    /// `0x006ED080` gates on PROCESS, then the zero-issues path scans every Player and
    /// clears `0x800` only when both the valid bit and byte-sized `who` match.
    #[test]
    fn end_process_zero_issues_clears_only_valid_matching_players() {
        let mut ls = Leaders::new();
        ls.leaders[0].flags = flag::IN_GAME;
        ls.leaders[0].slot = 5;
        ls.leaders[2].flags = flag::PROCESS;
        ls.leaders[2].slot = 5;
        ls.end.players[1] = EndPlayer {
            flags: PLAYER_VALID | PLAYER_POP_CAP_WARNING | 0x4000,
            who: 5,
            pop_cap_frame: 0,
        };
        ls.end.players[3] = EndPlayer {
            flags: PLAYER_POP_CAP_WARNING,
            who: 5,
            pop_cap_frame: 0,
        };
        ls.end.players[4] = EndPlayer {
            flags: PLAYER_VALID | PLAYER_POP_CAP_WARNING,
            who: 6,
            pop_cap_frame: 0,
        };

        let trace = end_process_all(&mut ls, 77);

        assert_eq!(trace.leaders_processed(), 1);
        assert!(!trace.processed[0], "IN_GAME alone is not the outer gate");
        assert_eq!(
            trace.warning_clears,
            vec![PopulationWarningClear {
                leader_index: 2,
                leader_who: 5,
                player_index: 1,
            }]
        );
        assert_eq!(ls.end.players[1].flags, PLAYER_VALID | 0x4000);
        assert_eq!(ls.end.players[3].flags, PLAYER_POP_CAP_WARNING);
        assert_eq!(
            ls.end.players[4].flags,
            PLAYER_VALID | PLAYER_POP_CAP_WARNING
        );
    }

    /// The signed elapsed comparison is strictly `> 0x1c1`: 449 is early and 450 is due.
    /// A due, below-limit local failure stamps first and emits the exact sound category.
    #[test]
    fn end_process_feedback_boundary_is_exactly_450_frames() {
        let mut ls = Leaders::new();
        ls.leaders[3].flags = flag::PROCESS;
        ls.leaders[3].slot = 3;
        ls.leaders[3].pop_issues = 1;
        ls.leaders[3].pop_cap = 99;
        ls.end.local_who = 3;
        ls.end.local_play = 2;
        ls.end.pop_limit_index = 2; // 100

        let early = end_process_all(&mut ls, 449);
        assert!(early.stamp_writes.is_empty());
        assert!(early.feedback.is_empty());
        assert_eq!(ls.end.players[2].pop_cap_frame, 0);

        let due = end_process_all(&mut ls, 450);
        assert_eq!(
            due.stamp_writes,
            vec![PopulationCapStamp {
                leader_index: 3,
                player_index: 2,
                frame: 450,
            }]
        );
        let event = PopulationCapFeedback {
            leader_index: 3,
            leader_who: 3,
            player_index: 2,
            text_offset: 0xC878,
            sound_category: 0x5B,
        };
        assert_eq!(due.feedback, vec![event]);
        assert_eq!(ls.end.last_feedback, vec![event]);
        assert_eq!(ls.end.players[2].pop_cap_frame, 450);
    }

    /// The frame write precedes the cap compare, so a leader already at the selected cap
    /// still consumes the notification interval without producing presentation work.
    #[test]
    fn end_process_at_cap_stamps_without_emitting_feedback() {
        let mut ls = Leaders::new();
        ls.leaders[1].flags = flag::PROCESS;
        ls.leaders[1].slot = 6;
        ls.leaders[1].pop_issues = 4;
        ls.leaders[1].pop_cap = 150;
        ls.end.local_who = 6;
        ls.end.local_play = 6;
        ls.end.pop_limit_index = 4; // 150

        let trace = end_process_all(&mut ls, 450);
        assert_eq!(trace.stamp_writes.len(), 1);
        assert!(trace.feedback.is_empty());
        assert_eq!(ls.end.players[6].pop_cap_frame, 450);
    }

    /// Invalid host-only indices stay explicit. Replacing them with current facts causes a
    /// fresh event; a subsequent non-due call clears `last_feedback` rather than replaying it.
    #[test]
    fn end_process_missing_facts_fail_closed_and_feedback_never_replays() {
        let mut ls = Leaders::new();
        ls.leaders[4].flags = flag::PROCESS;
        ls.leaders[4].slot = 4;
        ls.leaders[4].pop_issues = 1;
        ls.leaders[4].pop_cap = 10;
        ls.end.local_who = 4;
        ls.end.local_play = 9;

        let missing_player = end_process_all(&mut ls, 450);
        assert_eq!(
            missing_player.missing_facts,
            vec![EndProcessMissingFact::LocalPlayerIndex(9)]
        );
        assert!(missing_player.stamp_writes.is_empty());

        ls.end.local_play = 4;
        ls.end.pop_limit_index = 99;
        let missing_limit = end_process_all(&mut ls, 450);
        assert_eq!(
            missing_limit.missing_facts,
            vec![EndProcessMissingFact::PopulationLimitIndex(99)]
        );
        assert_eq!(missing_limit.stamp_writes.len(), 1);
        assert!(missing_limit.feedback.is_empty());

        ls.end.pop_limit_index = 0;
        let fresh = end_process_all(&mut ls, 900);
        assert_eq!(fresh.feedback.len(), 1);
        assert_eq!(ls.end.last_feedback.len(), 1);

        let not_due = end_process_all(&mut ls, 901);
        assert!(not_due.feedback.is_empty());
        assert!(ls.end.last_feedback.is_empty(), "no stale event may replay");
    }

    /// A production failure belonging to another player cannot stamp local state or be
    /// mistaken for a missing local-player adapter.
    #[test]
    fn end_process_nonlocal_failure_is_silent() {
        let mut ls = Leaders::new();
        ls.leaders[5].flags = flag::PROCESS;
        ls.leaders[5].slot = 5;
        ls.leaders[5].pop_issues = 1;
        ls.end.local_who = 2;
        ls.end.local_play = -1;

        let trace = end_process_all(&mut ls, 10_000);
        assert!(trace.stamp_writes.is_empty());
        assert!(trace.feedback.is_empty());
        assert!(trace.missing_facts.is_empty());
    }

    fn event_input(frame: i32) -> EventFrameInputs {
        EventFrameInputs {
            frame,
            age_by_who: [Some(0); NUM_LEADER_SLOTS],
            team_scores: [Some(0); NUM_LEADER_SLOTS],
        }
    }

    /// The dispatcher uses IN_GAME, not PROCESS, and the 50-frame body gate returns before
    /// even clearing current counters.
    #[test]
    fn event_dispatch_gate_and_non_due_return_are_exact() {
        let mut ls = Leaders::new();
        ls.leaders[0].flags = flag::PROCESS;
        ls.leaders[1].flags = flag::IN_GAME;
        ls.leaders[1].event_frame.deaths_current_frame = 7;

        let trace = process_event_frames(&mut ls, event_input(49));

        assert!(!trace.dispatched[0]);
        assert!(trace.dispatched[1]);
        assert_eq!(trace.leaders_dispatched(), 1);
        assert_eq!(trace.leaders_due(), 0);
        assert_eq!(ls.leaders[1].event_frame.deaths_current_frame, 7);
    }

    /// Every event counter uses 16-bit wrapping. The scaled current value is selected by
    /// its wrapped zero/nonzero state, then all four currents are zeroed at the tail.
    #[test]
    fn event_rates_fold_wrap_accumulate_and_reset_in_retail_order() {
        let mut ls = Leaders::new();
        ls.leaders[0].flags = flag::IN_GAME;
        ls.leaders[0].event_frame = EventFrameState {
            average_death_rate: 80,
            average_kill_rate: 10,
            average_hit_rate: 60_000,
            average_damage_rate: 5,
            deaths_current_frame: 0,
            kills_current_frame: 2,
            hits_current_frame: 1_000,
            damage_current_frame: 656, // 65,600 wraps to 64 before averaging
            deaths_fifteen_seconds: u16::MAX,
            kills_fifteen_seconds: u16::MAX,
            hits_fifteen_seconds: u16::MAX,
            damage_fifteen_seconds: u16::MAX,
            ..EventFrameState::default()
        };

        let mut input = event_input(50);
        input.age_by_who[0] = Some(7); // keep the achievement arm below its age threshold
        let trace = process_event_frames(&mut ls, input);
        let event = ls.leaders[0].event_frame;

        assert!(trace.due[0]);
        assert_eq!(event.average_death_rate, 70, "zero current decays 7/8");
        assert_eq!(event.average_kill_rate, 105, "(2*100 + 10)/2");
        assert_eq!(event.average_hit_rate, 47_232);
        assert_eq!(event.average_damage_rate, 34, "(64 + 5)/2");
        assert_eq!(event.deaths_fifteen_seconds, u16::MAX);
        assert_eq!(event.kills_fifteen_seconds, 1);
        assert_eq!(event.hits_fifteen_seconds, 999);
        assert_eq!(event.damage_fifteen_seconds, 655);
        assert_eq!(event.deaths_current_frame, 0);
        assert_eq!(event.kills_current_frame, 0);
        assert_eq!(event.hits_current_frame, 0);
        assert_eq!(event.damage_current_frame, 0);
    }

    /// Below 300 combat points mood 2 is requested; the boolean is one only when both
    /// rates are zero. The 300..599 band deliberately leaves the requested mood untouched.
    #[test]
    fn event_music_quiet_and_dead_band_boundaries_are_pinned() {
        let mut ls = Leaders::new();
        ls.leaders[0].flags = flag::IN_GAME;
        ls.end.local_who = 0;
        ls.event.current_music_mood = combat_mood::WINNING;

        let quiet = process_event_frames(&mut ls, event_input(0));
        assert_eq!(ls.event.next_music_mood, combat_mood::QUIET);
        assert_eq!(
            quiet.mood_requests,
            vec![CombatMoodRequest {
                leader_index: 0,
                mood: combat_mood::QUIET,
                force: true,
            }]
        );

        ls.leaders[0].event_frame.hits_current_frame = 2; // average 100
        ls.leaders[0].event_frame.damage_current_frame = 4; // average 200
        ls.event.next_music_mood = 91;
        let dead_band = process_event_frames(&mut ls, event_input(50));
        assert!(dead_band.mood_requests.is_empty());
        assert_eq!(ls.event.next_music_mood, 91);
        assert!(ls.event.last_mood_requests.is_empty());
    }

    /// A weak local score subtracts 200 from the hit-minus-damage balance. This flips a
    /// locally positive combat rate to mood 1 when an active hostile has the stronger score.
    #[test]
    fn event_music_uses_hostile_team_score_bias_and_sequential_rates() {
        let mut ls = Leaders::new();
        ls.leaders[0].flags = flag::IN_GAME;
        ls.leaders[1].flags = flag::IN_GAME;
        ls.end.local_who = 0;
        ls.event.current_music_mood = combat_mood::QUIET;
        // These zero-current averages fold to hit=700 and damage=600: +100 before bias.
        ls.leaders[0].event_frame.average_hit_rate = 800;
        ls.leaders[0].event_frame.average_damage_rate = 686;
        // Slot 0 runs first and sees slot 1's prior-frame combat state, as retail does.
        ls.leaders[1].event_frame.average_hit_rate = 1;
        let mut input = event_input(50);
        input.team_scores[0] = Some(100);
        input.team_scores[1] = Some(1_000);

        let trace = process_event_frames(&mut ls, input);

        assert_eq!(
            trace.mood_requests,
            vec![CombatMoodRequest {
                leader_index: 0,
                mood: combat_mood::LOSING,
                force: false,
            }]
        );
    }

    /// The high-rate transition out of quiet passes a one only at the exact 2000 boundary.
    #[test]
    fn event_music_force_flag_starts_at_two_thousand() {
        let mut ls = Leaders::new();
        ls.leaders[0].flags = flag::IN_GAME;
        ls.end.local_who = 0;
        ls.event.current_music_mood = combat_mood::QUIET;
        ls.leaders[0].event_frame.hits_current_frame = 40; // average = 2000

        let trace = process_event_frames(&mut ls, event_input(50));

        assert_eq!(trace.mood_requests.len(), 1);
        assert_eq!(trace.mood_requests[0].mood, combat_mood::WINNING);
        assert!(trace.mood_requests[0].force);
    }

    /// Achievement type 1 is deaths-over-kills. It stamps the frame and writes both
    /// average fields to 0xFC18; elapsed 1750 is suppressed while 1800 is allowed.
    #[test]
    fn battle_achievement_kind_sentinel_and_cooldown_are_exact() {
        let mut ls = Leaders::new();
        ls.leaders[0].flags = flag::IN_GAME;
        ls.leaders[0].event_frame.average_death_rate = 200;
        ls.leaders[0].event_frame.average_kill_rate = 100;

        let first = process_event_frames(&mut ls, event_input(200));
        let expected = BattleAchievementEvent {
            leader_index: 0,
            who: 0,
            kind: BattleAchievementKind::DeathsOverKills,
        };
        assert_eq!(first.achievement_events, vec![expected]);
        assert_eq!(ls.leaders[0].event_frame.frame_battle, 200);
        assert_eq!(
            ls.leaders[0].event_frame.average_death_rate,
            BATTLE_RATE_SENTINEL
        );
        assert_eq!(
            ls.leaders[0].event_frame.average_kill_rate,
            BATTLE_RATE_SENTINEL
        );

        ls.leaders[0].event_frame.average_death_rate = 200;
        ls.leaders[0].event_frame.average_kill_rate = 100;
        let early = process_event_frames(&mut ls, event_input(1_950));
        assert!(early.achievement_events.is_empty());

        ls.leaders[0].event_frame.average_death_rate = 200;
        ls.leaders[0].event_frame.average_kill_rate = 100;
        let due = process_event_frames(&mut ls, event_input(2_000));
        assert_eq!(due.achievement_events, vec![expected]);
        assert_eq!(ls.leaders[0].event_frame.frame_battle, 2_000);
    }

    /// A missing who→age fact suppresses only the achievement arm. Counter folds/resets
    /// still happen, and presentation lists are refreshed rather than replayed.
    #[test]
    fn event_missing_age_is_explicit_without_stale_replay() {
        let mut ls = Leaders::new();
        ls.leaders[0].flags = flag::IN_GAME;
        ls.leaders[0].slot = 9;
        ls.leaders[0].event_frame.deaths_current_frame = 3;
        ls.event
            .last_achievement_events
            .push(BattleAchievementEvent {
                leader_index: 7,
                who: 7,
                kind: BattleAchievementKind::KillsOverDeaths,
            });

        let trace = process_event_frames(&mut ls, event_input(50));

        assert_eq!(
            trace.missing_facts,
            vec![EventFrameMissingFact::LocalLeaderSlot(9)]
        );
        assert!(trace.achievement_events.is_empty());
        assert!(ls.event.last_achievement_events.is_empty());
        assert_eq!(ls.leaders[0].event_frame.deaths_fifteen_seconds, 3);
        assert_eq!(ls.leaders[0].event_frame.deaths_current_frame, 0);
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

    /// Step 8 resets the PDB-named production-failure count and the counter at `+0x9F4`
    /// for every processed leader, which is what makes step 17 a same-frame post-pass.
    #[test]
    fn pop_issues_and_the_9f4_counter_are_zeroed_every_frame() {
        let mut d = Step8Driver::new();
        d.leaders.leaders[0].activate();
        d.leaders.leaders[0].pop_issues = 77;
        d.leaders.leaders[0].frame_counter_b = -3;
        d.leaders.leaders[1].pop_issues = 77; // not processed
        d.frame();
        assert_eq!(d.leaders.leaders[0].pop_issues, 0);
        assert_eq!(d.leaders.leaders[0].frame_counter_b, 0);
        assert_eq!(d.leaders.leaders[1].pop_issues, 77);
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
            captain: true,
            owner_in_game: true,
            hit_inputs: Some(ObjectHitInputs {
                base_hits: 200,
                ..Default::default()
            }),
            type_los: Some(4),
            speed_inputs: Some(UnitSpeedInputs {
                type_moves: 25,
                ..Default::default()
            }),
            armor_inputs: Some(UnitArmorInputs {
                type_armor: 5,
                special_family_32_33: false,
                ..Default::default()
            }),
            o_down: None,
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
        assert_eq!(t.unit_pass[0].object_hits_updates, 1);
        assert_eq!(t.unit_pass[0].object_los_updates, 1);
        assert_eq!(t.unit_pass[0].unit_speed_updates, 1);
        assert_eq!(t.unit_pass[0].unit_armor_updates, 1);
        assert_eq!(d.env.leaders[0].objects.units[0].myhits, 200);
        assert_eq!(d.env.leaders[0].objects.units[0].mylos, 4);
        assert_eq!(d.env.leaders[0].objects.units[0].myspeed, 25);
        assert_eq!(d.env.leaders[0].objects.units[0].myarmor, 5);

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

    #[test]
    fn object_hit_update_uses_retail_special_family_priority() {
        let mut o = StatObject {
            hit_inputs: Some(ObjectHitInputs {
                base_hits: 100,
                special_family_32_33: true,
                type_42_hits: 420,
                type_43_hits: 430,
                type_44_hits: 440,
                owner_policy: 0x1c,
            }),
            ..Default::default()
        };
        assert_eq!(object_update_hits(&mut o, false), Some(440));
        assert_eq!(o.myhits, 440);

        o.hit_inputs.as_mut().unwrap().owner_policy = 0x0c;
        assert_eq!(object_update_hits(&mut o, false), Some(430));
        o.hit_inputs.as_mut().unwrap().owner_policy = 0x04;
        assert_eq!(object_update_hits(&mut o, false), Some(420));
        o.hit_inputs.as_mut().unwrap().owner_policy = 0;
        assert_eq!(object_update_hits(&mut o, false), Some(100));
    }

    #[test]
    fn object_los_update_zeros_an_out_of_game_owner() {
        let mut o = StatObject {
            type_los: Some(7),
            owner_in_game: false,
            mylos: 99,
            ..Default::default()
        };
        assert_eq!(object_update_los(&mut o), Some(0));
        assert_eq!(o.mylos, 0);
        o.owner_in_game = true;
        assert_eq!(object_update_los(&mut o), Some(7));
        assert_eq!(o.mylos, 7);
    }

    #[test]
    fn wall_hit_override_executes_the_full_modifier_order_and_both_city_arms() {
        let mut rules = Step8Rules::zeroed();
        rules.maya_building_hp = 25;
        rules.roman_fort_hp = 20;
        rules.building_hp_upgrade = 10;
        rules.taj_building_hp = 50;
        rules.red_fort_fort_hps = 10;
        rules.nubian_hit_points = 50;
        rules.capital_building_hp[4] = 40;
        rules.tikal_temple_hp = 50;
        rules.ctw_missionaries_bonus = 20;
        rules.senate_hp_bonus = 35;
        let mut o = StatObject {
            wall_active: true,
            wall_city_flag: true,
            hit_inputs: Some(ObjectHitInputs {
                base_hits: 100,
                ..Default::default()
            }),
            wall_hit_inputs: Some(WallHitInputs {
                maya: true,
                roman: true,
                is_city_type: true,
                building_hp_upgrade: 2,
                taj_mahal: true,
                red_fort: true,
                is_fort_1b4: true,
                nubian: true,
                linked_city: true,
                linked_city_is_capital: true,
                has_preq_2cd: true,
                tikal: true,
                ctw_mode: true,
                ctw_rare_20: true,
                can_carry_domain_2: true,
                ..Default::default()
            }),
            ..Default::default()
        };
        let out = wall_update_hits(&mut o, &rules, false).unwrap();
        assert_eq!(out.returned_hits, 854);
        assert_eq!(o.myhits, 854);
        assert_eq!(o.construct_hits, 854);
        assert!(o.wall_hits_written);
        assert!(!out.eject_contents);

        // Mutation pin for the mutually exclusive non-capital arm: +35% for each city
        // beyond the first is additive, not another pct_scale.
        o.wall_city_flag = false;
        o.hit_inputs.as_mut().unwrap().base_hits = 100;
        o.wall_hit_inputs = Some(WallHitInputs {
            noncapital_city_gate: true,
            linked_city: true,
            linked_city_count: 3,
            can_carry_domain_2: true,
            ..Default::default()
        });
        assert_eq!(
            wall_update_hits(&mut o, &rules, false)
                .unwrap()
                .returned_hits,
            170
        );
    }

    #[test]
    fn wall_hit_override_quantises_progress_and_surfaces_only_reached_ejection() {
        let rules = Step8Rules::shipped();
        let mut o = StatObject {
            wall_active: false,
            job_counter: 500,
            constr_time: 1000,
            damage: 500,
            inside_down: 0,
            hit_inputs: Some(ObjectHitInputs {
                base_hits: 1000,
                ..Default::default()
            }),
            wall_hit_inputs: Some(WallHitInputs::default()),
            ..Default::default()
        };
        let out = wall_update_hits(&mut o, &rules, false).unwrap();
        assert_eq!(o.myhits, 1000);
        assert_eq!(o.construct_hits, 483);
        assert_eq!(out.returned_hits, 483);
        assert!(out.eject_contents);

        o.wall_hit_inputs.as_mut().unwrap().can_carry_domain_2 = true;
        let out = wall_update_hits(&mut o, &rules, true).unwrap();
        assert_eq!(out.returned_hits, 1000);
        assert!(!out.eject_contents);
    }

    #[test]
    fn wall_los_override_keeps_started_wonder_byte_arithmetic_and_rare_order() {
        let mut leader = Leader::new(0);
        leader.rare_b.set(15, true); // Furs
        let mut rules = Step8Rules::shipped();
        rules.colosseum_fort_range = 4;
        rules.furs_los = 5;
        let input = WallLosInputs {
            science_level: 2,
            type_science_los: 3,
            is_city_type: true,
            city_range_upgrade: 2,
            colosseum: true,
            footprint_x: 3,
            ..Default::default()
        };
        let mut o = StatObject {
            wall_started: true,
            wall_active: true,
            owner_in_game: true,
            type_los: Some(5),
            wall_los_inputs: Some(input),
            ..Default::default()
        };
        assert_eq!(wall_update_los(&mut o, &leader, &rules), Some(23));
        assert!(o.wall_los_written);

        // An unstarted object takes the hard-zero return before footprint/type bonuses.
        o.wall_started = false;
        assert_eq!(wall_update_los(&mut o, &leader, &rules), Some(0));

        // A started but incomplete wonder begins at LOS 1, then still takes x_size / 2.
        o.wall_started = true;
        o.wall_active = false;
        o.wall_los_inputs.as_mut().unwrap().is_wonder = true;
        assert_eq!(wall_update_los(&mut o, &leader, &rules), Some(2));
        o.wall_los_inputs.as_mut().unwrap().is_wonder = false;
        assert_eq!(wall_update_los(&mut o, &leader, &rules), Some(0));
    }

    /// `LeaderData::get_building_speed_upgrade` `0x006DAE90` accumulates unconditionally
    /// per iteration (`cmove` at `0x006DAECC`), so a hole in the middle of
    /// `BUILDINGS_FASTER_1..3` still counts the far end. That is the *opposite* of
    /// `Leader::calc_attrition`'s consecutive run, and getting it wrong is silent.
    #[test]
    fn building_speed_upgrade_counts_all_three_preqs_not_a_consecutive_run() {
        let mut s = BuildLeaderStatState::default();
        assert_eq!(s.get_building_speed_upgrade(), 0);
        s.buildings_faster = [true, false, true];
        assert_eq!(s.get_building_speed_upgrade(), 2);
        s.buildings_faster = [false, false, true];
        assert_eq!(s.get_building_speed_upgrade(), 1);
        s.buildings_faster = [true, true, true];
        assert_eq!(s.get_building_speed_upgrade(), 3);
    }

    /// Drives `Wall::update_construct_time` `0x0063D560` one reduction at a time, in
    /// emitted order, against hand-independent expectations: every value below is
    /// recomputed here from the shipped rule and the previous stage, not copied from a run.
    #[test]
    fn construct_time_applies_every_reduction_in_emitted_order() {
        let rules = Step8Rules::shipped();
        let package = WallConstructTimeInputs {
            type_time: 100_000,
            ..Default::default()
        };
        let base = |leader: &Leader, package: WallConstructTimeInputs| -> u32 {
            let mut o = StatObject {
                wall_construct_time_inputs: Some(package),
                ..Default::default()
            };
            let v = wall_update_construct_time(&mut o, leader, &rules).unwrap();
            assert!(o.construct_time_written);
            assert_eq!(o.constr_time, v);
            v
        };

        // A leader with a city, no tribe, no tech: only the final `(10 - 0) * v / 10`
        // runs, and it is the identity.
        let mut leader = Leader::new(0);
        leader.city_num = 1;
        assert_eq!(base(&leader, package), 100_000);

        // Nomad: CAPITAL_BUILD_TIME is a multiplier over a literal 100, so 300 triples.
        leader.city_num = 0;
        assert_eq!(base(&leader, package), 300_000);
        leader.city_num = 1;

        // Maya, MAYA_BUILDING_SPEED = 20: v*100/120.
        leader.unit_stats.set_tribe_bonus(1, true);
        assert_eq!(base(&leader, package), 100_000 * 100 / 120);
        // ... but not for a Wonder, which re-issues vtable +0x2C.
        assert_eq!(
            base(
                &leader,
                WallConstructTimeInputs {
                    is_wonder: true,
                    ..package
                }
            ),
            100_000
        );
        leader.unit_stats.set_tribe_bonus(1, false);

        // BUILDINGS_CREATED_FASTER is the one non-percentage step: (v*3) >> 2.
        leader.build_stats.buildings_created_faster = true;
        assert_eq!(base(&leader, package), 100_000 * 3 / 4);
        leader.build_stats.buildings_created_faster = false;

        // Versailles and Tobacco do NOT consult +0x2C, so a Wonder still gets them.
        leader.unit_stats.versailles = true;
        assert_eq!(
            base(
                &leader,
                WallConstructTimeInputs {
                    is_wonder: true,
                    ..package
                }
            ),
            100_000 * 100 / 100 // VERSAILLES_BUILDING_SPEED ships 0
        );
        leader.unit_stats.versailles = false;

        leader.rare_b.set(RareMask::TOBACCO, true);
        assert_eq!(base(&leader, package), 100_000 * 100 / 110);
        leader.rare_b.set(RareMask::TOBACCO, false);
        leader.rare_effective.set(RareMask::TOBACCO, true);
        assert_eq!(base(&leader, package), 100_000 * 100 / 110);
        leader.rare_effective.set(RareMask::TOBACCO, false);

        // British: only for an AIRDEFENSE type, and with no Wonder gate at all.
        leader.unit_stats.set_tribe_bonus(0x0B, true);
        assert_eq!(base(&leader, package), 100_000);
        let aa = WallConstructTimeInputs {
            is_airdefense: true,
            ..package
        };
        assert_eq!(base(&leader, aa), 100_000 * 100 / 133);
        assert_eq!(
            base(
                &leader,
                WallConstructTimeInputs {
                    is_wonder: true,
                    ..aa
                }
            ),
            100_000 * 100 / 133
        );
        leader.unit_stats.set_tribe_bonus(0x0B, false);

        // Roman: FORTX only, and suppressed on a Wonder. ROMAN_FORT_SPEED ships 50.
        let fort = WallConstructTimeInputs {
            is_fort: true,
            ..package
        };
        leader.unit_stats.set_tribe_bonus(6, true);
        assert_eq!(base(&leader, package), 100_000);
        assert_eq!(base(&leader, fort), 100_000 * 100 / 150);
        assert_eq!(
            base(
                &leader,
                WallConstructTimeInputs {
                    is_wonder: true,
                    ..fort
                }
            ),
            100_000
        );
        leader.unit_stats.set_tribe_bonus(6, false);

        // The upgrade tail divides by a literal 10, applied last.
        leader.build_stats.buildings_faster = [true, true, false];
        assert_eq!(base(&leader, package), (10 - 2) * 100_000 / 10);

        // Order check: Roman then capital then upgrade, each on the previous result.
        leader.city_num = 0;
        leader.unit_stats.set_tribe_bonus(6, true);
        let expect = {
            let v = 100_000u32 * 100 / 150;
            let v = 300 * v / 100;
            (10 - 2) * v / 10
        };
        assert_eq!(base(&leader, fort), expect);
    }

    /// A missing type package is refused, not guessed, and leaves `constr_time` alone —
    /// that refusal is what `Gap::LeaderCalcWallStats` counts.
    #[test]
    fn construct_time_without_a_type_package_is_refused_and_charged_by_both_bands() {
        let leader = Leader::new(0);
        let rules = Step8Rules::shipped();
        let under_construction = StatObject {
            active: true,
            wall_active: false,
            constr_time: 4242,
            ..Default::default()
        };
        let mut objects = OwnerObjects {
            band_2000: vec![under_construction],
            band_3000: vec![under_construction],
            ..Default::default()
        };
        let c = calc_wall_stats(&leader, &rules, &mut objects);
        assert_eq!(c.construct_time_updates, 2);
        assert_eq!(c.construct_time_resolved, 0);
        assert_eq!(objects.band_2000[0].constr_time, 4242);
        assert_eq!(objects.band_3000[0].constr_time, 4242);
        assert!(!objects.band_2000[0].construct_time_written);

        // Supply the package on both bands: the same direct-called body runs in each loop.
        let package = Some(WallConstructTimeInputs {
            type_time: 900,
            ..Default::default()
        });
        objects.band_2000[0].wall_construct_time_inputs = package;
        objects.band_3000[0].wall_construct_time_inputs = package;
        let c = calc_wall_stats(&leader, &rules, &mut objects);
        assert_eq!(c.construct_time_updates, 2);
        assert_eq!(c.construct_time_resolved, 2);
        // city_num is 0 on a fresh leader, so CAPITAL_BUILD_TIME applies: 900 -> 2700.
        assert_eq!(objects.band_2000[0].constr_time, 2700);
        assert_eq!(objects.band_3000[0].constr_time, 2700);

        // An active object never reaches the call at all.
        objects.band_2000[0].wall_active = true;
        objects.band_3000[0].wall_active = true;
        let c = calc_wall_stats(&leader, &rules, &mut objects);
        assert_eq!(c.construct_time_updates, 0);
        assert_eq!(c.construct_time_resolved, 0);
    }

    /// `Wall::update_construct_time` stores `constr_time` *before* `Wall::update_hits`
    /// reads it for the construction ramp (`0x006CF838` precedes `0x006CF85B`). If the two
    /// were reordered the ramp would quantise against last frame's total work.
    #[test]
    fn construct_time_is_stored_before_the_hit_ramp_consumes_it() {
        let mut leader = Leader::new(0);
        leader.city_num = 1;
        let rules = Step8Rules::shipped();
        let mut objects = OwnerObjects {
            band_2000: vec![StatObject {
                active: true,
                wall_active: false,
                owner_in_game: true,
                job_counter: 32,
                // Deliberately stale: retail overwrites this before the ramp runs.
                constr_time: 32,
                hit_inputs: Some(ObjectHitInputs {
                    base_hits: 1000,
                    ..Default::default()
                }),
                wall_hit_inputs: Some(WallHitInputs {
                    can_carry_domain_2: true,
                    ..Default::default()
                }),
                wall_construct_time_inputs: Some(WallConstructTimeInputs {
                    type_time: 128,
                    ..Default::default()
                }),
                ..Default::default()
            }],
            ..Default::default()
        };
        calc_wall_stats(&leader, &rules, &mut objects);
        let o = &objects.band_2000[0];
        assert_eq!(o.constr_time, 128);
        // With ct = 128 the ramp is (32>>5) * 1000 / (128>>5) = 1 * 1000 / 4 = 250. Had the
        // stale ct = 32 survived, `job_counter < constr_time` would have been false and the
        // ramp would have left the full 1000.
        assert_eq!(o.construct_hits, 250);
        assert_eq!(o.myhits, 1000);
    }

    #[test]
    fn build_band_gap_accounting_charges_only_missing_or_reached_nested_bodies() {
        let leader = Leader::new(0);
        let rules = Step8Rules::shipped();
        let resolved = StatObject {
            active: true,
            wall_active: true,
            wall_started: true,
            owner_in_game: true,
            hit_inputs: Some(ObjectHitInputs {
                base_hits: 500,
                ..Default::default()
            }),
            type_los: Some(6),
            wall_hit_inputs: Some(WallHitInputs {
                can_carry_domain_2: true,
                ..Default::default()
            }),
            wall_los_inputs: Some(WallLosInputs::default()),
            ..Default::default()
        };
        let mut objects = OwnerObjects {
            band_2000: vec![resolved],
            ..Default::default()
        };
        let c = calc_wall_stats(&leader, &rules, &mut objects);
        assert_eq!(c.wall_hits_updates, 1);
        assert_eq!(c.wall_los_updates, 1);
        assert_eq!(c.eject_contents_calls, 0);
        assert_eq!(c.unresolved_calls, 0);

        let o = &mut objects.band_2000[0];
        o.damage = 500;
        o.inside_down = 0;
        o.wall_hit_inputs.as_mut().unwrap().can_carry_domain_2 = false;
        let c = calc_wall_stats(&leader, &rules, &mut objects);
        assert_eq!(c.wall_hits_updates, 1);
        assert_eq!(c.wall_los_updates, 1);
        assert_eq!(c.eject_contents_calls, 1);
        assert_eq!(c.unresolved_calls, 1);
    }

    #[test]
    fn unit_armor_applies_cattle_bonus_and_propagates_down_the_captain_chain() {
        let mut leader = Leader::new(0);
        leader.rare_effective.set(31, true);
        let rules = Step8Rules::shipped();
        let mut units = vec![
            StatObject {
                captain: true,
                armor_inputs: Some(UnitArmorInputs {
                    type_armor: 12,
                    special_family_32_33: true,
                    ..Default::default()
                }),
                o_down: Some(1),
                ..Default::default()
            },
            StatObject {
                o_down: Some(2),
                ..Default::default()
            },
            StatObject {
                o_down: None,
                ..Default::default()
            },
        ];

        assert_eq!(unit_update_armor(&mut units, 0, &leader, &rules), Some(13));
        assert_eq!(units.iter().map(|u| u.myarmor).collect::<Vec<_>>(), [13; 3]);
        assert!(units.iter().all(|u| u.armor_written));
        assert_eq!(units[0].unit_armor_updates, 1);
        assert_eq!(units[1].unit_armor_updates, 0);

        // Mutation pin: without rare bit 31, the same resolved ObjectData armor passes
        // through unchanged, while propagation remains identical.
        leader.rare_effective.set(31, false);
        for u in units.iter_mut() {
            u.armor_written = false;
        }
        assert_eq!(unit_update_armor(&mut units, 0, &leader, &rules), Some(12));
        assert_eq!(units.iter().map(|u| u.myarmor).collect::<Vec<_>>(), [12; 3]);
    }

    #[test]
    fn unit_speed_executes_the_retail_modifier_order() {
        let mut leader = Leader::new(0);
        leader.rare_effective.set(25, true); // whales
        let mut rules = Step8Rules::zeroed();
        rules.unit_move_speed = 2;
        rules.military_transport_bonus = 3;
        rules.americans_marine_speed_bonus = 2;
        rules.whales_ships_move = 20;
        rules.bantu_units_move = 25;
        rules.aztec_move_speed = 10;
        let input = UnitSpeedInputs {
            type_moves: 80,
            domain: 1,
            unit_flags: 0x10,
            unit_data_flags: 0x200,
            military_epoch: 2,
            has_objmask_2000: true,
            is_68: true,
            is_3a: true,
            type_id: 0x32,
            bantu: true,
            spy_upgrade: 1,
            hero: true,
            general_upgrade: 2,
            supply: true,
            supply_upgrade: 1,
            aztec: true,
            ..Default::default()
        };

        // ((80 + 2*3 + 2*2)*2) * whales -> type 0x68 -> Bantu -> spy -> general
        // -> supply -> Aztec, truncating at every retail IDIV/shift boundary.
        assert_eq!(unit_speed(&input, &leader, &rules), 778);

        // Mutation pins both rare-mask operands and the has_objmask gate. With neither
        // source carrying Whales, exactly the 20% step disappears.
        leader.rare_effective.set(25, false);
        assert_eq!(unit_speed(&input, &leader, &rules), 649);
        leader.rare_b.set(25, true);
        assert_eq!(unit_speed(&input, &leader, &rules), 778);
        let no_mask = UnitSpeedInputs {
            has_objmask_2000: false,
            ..input
        };
        assert_eq!(unit_speed(&no_mask, &leader, &rules), 649);
    }

    #[test]
    fn unit_speed_pins_type_precedence_signed_rounding_and_siege_air_order() {
        let leader = Leader::new(0);
        let rules = Step8Rules::shipped();
        let all = UnitSpeedInputs {
            type_moves: -32,
            is_68: true,
            is_66: true,
            is_64: true,
            is_62: true,
            ..Default::default()
        };
        assert_eq!(unit_speed(&all, &leader, &rules), -36);
        assert_eq!(
            unit_speed(
                &UnitSpeedInputs {
                    is_68: false,
                    ..all
                },
                &leader,
                &rules
            ),
            -34
        );
        assert_eq!(
            unit_speed(
                &UnitSpeedInputs {
                    is_68: false,
                    is_66: false,
                    ..all
                },
                &leader,
                &rules
            ),
            -37
        );
        assert_eq!(
            unit_speed(
                &UnitSpeedInputs {
                    is_68: false,
                    is_66: false,
                    is_64: false,
                    ..all
                },
                &leader,
                &rules,
            ),
            -40
        );

        let mut air_leader = Leader::new(0);
        air_leader.rare_b.set(24, true); // aluminum
        let siege_air = UnitSpeedInputs {
            type_moves: 100,
            domain: 2,
            type_line: 0x1ae,
            french: true,
            versailles: true,
            ..Default::default()
        };
        // French 120 -> Versailles 150 -> Aluminum 187.
        assert_eq!(unit_speed(&siege_air, &air_leader, &rules), 187);
    }

    #[test]
    fn unit_speed_stores_i16_and_propagates_down_the_captain_chain() {
        let leader = Leader::new(0);
        let rules = Step8Rules::shipped();
        let mut units = vec![
            StatObject {
                speed_inputs: Some(UnitSpeedInputs {
                    type_moves: 40_000,
                    ..Default::default()
                }),
                o_down: Some(1),
                ..Default::default()
            },
            StatObject {
                o_down: Some(2),
                ..Default::default()
            },
            StatObject::default(),
        ];
        let stored = 40_000i32 as i16;
        assert_eq!(
            unit_update_speed(&mut units, 0, &leader, &rules),
            Some(stored as i32)
        );
        assert_eq!(
            units.iter().map(|u| u.myspeed).collect::<Vec<_>>(),
            [stored; 3]
        );
        assert!(units.iter().all(|u| u.speed_written));
        assert_eq!(units[0].unit_speed_updates, 1);
        assert_eq!(units[1].unit_speed_updates, 0);
    }

    #[test]
    fn object_armor_dutch_age_bonus_keeps_all_retail_early_returns() {
        let mut leader = Leader::new(0);
        leader.econ.age = 5;
        let rules = Step8Rules::shipped();
        let mut input = UnitArmorInputs {
            type_armor: 3,
            dutch: true,
            dutch_armor_eligible: true,
            type_id: 0x3d,
            patch_version: 8,
            ..Default::default()
        };
        assert_eq!(object_data_armor(&input, &leader, &rules), 8);

        // Mutation pins the post-patch government-hero exclusion.
        input.patch_version = 9;
        input.gov_hero = true;
        assert_eq!(object_data_armor(&input, &leader, &rules), 3);
        input.gov_hero = false;
        input.type_id = 7;
        assert_eq!(object_data_armor(&input, &leader, &rules), 3);
        input.caravan = true;
        assert_eq!(object_data_armor(&input, &leader, &rules), 8);
    }

    #[test]
    fn shipped_unit_packages_rebuild_type_families_and_live_leader_queries() {
        let table = unit_type_stat_source_fixture();
        let mut leader = Leader::new(0);
        leader.unit_stats.military_epoch = 4;
        leader.unit_stats.set_tribe_bonus(0, true);
        leader.unit_stats.set_tribe_bonus(3, true);
        leader.unit_stats.set_tribe_bonus(10, true);
        leader.unit_stats.set_tribe_bonus(0x16, true);
        leader.unit_stats.versailles = true;
        leader.unit_stats.spy_upgrade = 1;
        leader.unit_stats.general_upgrade = 2;
        leader.unit_stats.supply_upgrade = 3;

        // InfantryGerman (103) has `graft=102` and `from=100` in the post-load retail
        // table. Both relations participate in non-strict ObjectTypeData::is.
        let (speed, armor) = derive_unit_query_packages(
            &table,
            UnitQuerySource {
                type_id: 103,
                unit_masks2: 0x200,
            },
            &leader,
        )
        .unwrap();
        assert_eq!(speed.type_moves, 32);
        assert_eq!(speed.type_line, 427);
        assert_eq!(speed.unit_data_flags, 0x200);
        assert!(speed.is_66, "graft target 102 must match");
        assert!(speed.is_64, "recursive from target 100 must match");
        assert!(!speed.is_68);
        assert!(speed.aztec && speed.bantu && speed.french && speed.versailles);
        assert_eq!(speed.military_epoch, 4);
        assert_eq!(speed.spy_upgrade, 1);
        assert_eq!(speed.general_upgrade, 2);
        assert_eq!(speed.supply_upgrade, 3);
        assert_eq!(armor.type_armor, 4);
        assert!(armor.dutch && armor.dutch_armor_eligible);
        assert_eq!(armor.patch_version, 8);
    }

    #[test]
    fn automatic_unit_packages_refresh_and_missing_types_cannot_replay_stale_stats() {
        let table = unit_type_stat_source_fixture();
        let mut leader = Leader::new(0);
        let rules = Step8Rules::shipped();
        let mut objects = OwnerObjects {
            units: vec![StatObject {
                active: true,
                captain: true,
                owner_in_game: true,
                hit_inputs: Some(ObjectHitInputs {
                    base_hits: 100,
                    ..Default::default()
                }),
                type_los: Some(3),
                unit_query_source: Some(UnitQuerySource {
                    type_id: 0x32,
                    unit_masks2: 0,
                }),
                ..Default::default()
            }],
            ..Default::default()
        };

        let c = calc_unit_stats_with_source_and_type_overrides(
            &mut leader,
            &rules,
            &AttritionGates::default(),
            &mut objects,
            Some(&table),
            &[],
        );
        let unit = &objects.units[0];
        assert_eq!((unit.myspeed, unit.myarmor), (25, 0));
        assert_eq!(c.unit_query_populations, 1);
        assert_eq!(c.unit_query_misses, 0);
        assert_eq!(c.unresolved_calls, 0);

        // Mutating only the live type identity must replace both packages on the next
        // dirty pass. Caravan is shipped type 59: speed 26, armor 0.
        objects.units[0].unit_query_source.as_mut().unwrap().type_id = 59;
        let c = calc_unit_stats_with_source_and_type_overrides(
            &mut leader,
            &rules,
            &AttritionGates::default(),
            &mut objects,
            Some(&table),
            &[],
        );
        assert_eq!(
            (objects.units[0].myspeed, objects.units[0].myarmor),
            (26, 0)
        );
        assert_eq!(c.unit_query_populations, 1);
        assert_eq!(c.unresolved_calls, 0);

        // An unknown replacement row clears the generated packages before lookup. The
        // derived fields remain at the externally-mutated values instead of replaying the
        // Caravan package from the previous edge.
        let unit = &mut objects.units[0];
        unit.unit_query_source.as_mut().unwrap().type_id = 805;
        unit.myspeed = 91;
        unit.myarmor = 92;
        unit.speed_written = false;
        unit.armor_written = false;
        let c = calc_unit_stats_with_source_and_type_overrides(
            &mut leader,
            &rules,
            &AttritionGates::default(),
            &mut objects,
            Some(&table),
            &[],
        );
        let unit = &objects.units[0];
        assert_eq!((unit.myspeed, unit.myarmor), (91, 92));
        assert!(!unit.speed_written && !unit.armor_written);
        assert!(unit.speed_inputs.is_none() && unit.armor_inputs.is_none());
        assert_eq!(c.unit_query_populations, 0);
        assert_eq!(c.unit_query_misses, 1);
        assert_eq!(c.unresolved_calls, 2);
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
        block[rule_offsets::CATTLE_CITIZEN_ARMOR / 4] = 1;
        block[rule_offsets::DUTCH_ATTACK_BONUS / 4] = 1;
        block[rule_offsets::UNIT_MOVE_SPEED / 4] = 1;
        block[rule_offsets::AZTEC_MOVE_SPEED / 4] = 17;
        block[rule_offsets::SENATE_HP_BONUS / 4] = 35;
        block[rule_offsets::FORT_UPGRADE_RANGE / 4 + 4] = 4;
        let r = Step8Rules::from_block(&block);
        assert_eq!(r.timer_refresh_ratio, 5);
        assert_eq!(r.attrition_improved[0], 1);
        assert_eq!(r.attrition_improved[3], 8);
        assert_eq!(r.titanium_attrition, 50);
        assert_eq!(r.cattle_citizen_armor, 1);
        assert_eq!(r.dutch_attack_bonus, 1);
        assert_eq!(r.unit_move_speed, 1);
        assert_eq!(r.aztec_move_speed, 17);
        assert_eq!(r.senate_hp_bonus, 35);
        assert_eq!(r.fort_upgrade_range[4], 4);
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
        block[rule_offsets::CATTLE_CITIZEN_ARMOR / 4] = s.cattle_citizen_armor;
        block[rule_offsets::DUTCH_ATTACK_BONUS / 4] = s.dutch_attack_bonus;
        block[rule_offsets::CTW_ATTRITION / 4] = s.ctw_attrition;
        block[rule_offsets::UNIT_MOVE_SPEED / 4] = s.unit_move_speed;
        block[rule_offsets::MILITARY_TRANSPORT_BONUS / 4] = s.military_transport_bonus;
        block[rule_offsets::VERSAILLES_UNITS_MOVE / 4] = s.versailles_units_move;
        block[rule_offsets::AZTEC_MOVE_SPEED / 4] = s.aztec_move_speed;
        block[rule_offsets::BANTU_UNITS_MOVE / 4] = s.bantu_units_move;
        block[rule_offsets::FRENCH_SIEGE_MOVE / 4] = s.french_siege_move;
        block[rule_offsets::AMERICANS_MARINE_SPEED_BONUS / 4] = s.americans_marine_speed_bonus;
        block[rule_offsets::ALUMINUM_AIR_SPEED / 4] = s.aluminum_air_speed;
        block[rule_offsets::WHALES_SHIPS_MOVE / 4] = s.whales_ships_move;
        block[rule_offsets::BUILDING_HP_UPGRADE / 4] = s.building_hp_upgrade;
        for i in 0..5 {
            block[rule_offsets::CAPITAL_BUILDING_HP / 4 + i] = s.capital_building_hp[i];
            block[rule_offsets::FORT_UPGRADE_RANGE / 4 + i] = s.fort_upgrade_range[i];
        }
        for i in 0..4 {
            block[rule_offsets::TOWER_FORT_RANGE / 4 + i] = s.tower_fort_range[i];
        }
        block[rule_offsets::COLOSSEUM_FORT_RANGE / 4] = s.colosseum_fort_range;
        block[rule_offsets::TIKAL_TEMPLE_HP / 4] = s.tikal_temple_hp;
        block[rule_offsets::RED_FORT_FORT_HPS / 4] = s.red_fort_fort_hps;
        block[rule_offsets::TAJ_BUILDING_HP / 4] = s.taj_building_hp;
        block[rule_offsets::MAYA_BUILDING_HP / 4] = s.maya_building_hp;
        block[rule_offsets::NUBIAN_HIT_POINTS / 4] = s.nubian_hit_points;
        block[rule_offsets::ROMAN_FORT_HP / 4] = s.roman_fort_hp;
        block[rule_offsets::FURS_LOS / 4] = s.furs_los;
        block[rule_offsets::CTW_MISSIONARIES_BONUS / 4] = s.ctw_missionaries_bonus;
        block[rule_offsets::SENATE_HP_BONUS / 4] = s.senate_hp_bonus;
        block[rule_offsets::MAYA_BUILDING_SPEED / 4] = s.maya_building_speed;
        block[rule_offsets::VERSAILLES_BUILDING_SPEED / 4] = s.versailles_building_speed;
        block[rule_offsets::TOBACCO_BUILDING_SPEED / 4] = s.tobacco_building_speed;
        block[rule_offsets::BRITISH_AA_SPEED / 4] = s.british_aa_speed;
        block[rule_offsets::DUTCH_FORT_SPEED / 4] = s.dutch_fort_speed;
        block[rule_offsets::ROMAN_FORT_SPEED / 4] = s.roman_fort_speed;
        block[rule_offsets::CAPITAL_BUILD_TIME / 4] = s.capital_build_time;
        assert_eq!(Step8Rules::from_block(&block), s);
    }
}
