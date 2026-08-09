//! Leader-owned tick dispatchers: step 8 `Leaders::process_all` `0x006ED2A0` and step 11
//! `Leaders::strategy_all` `0x006ED430`.
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
//!  +- [11] Leaders::strategy_all    0x006ED430   <- [`strategy_all`], this file
//!      for slot in 0..8, flags & 3 == 3:
//!        Leader::check_explore      0x006BC860   -> [`check_explore`]
//!        Leader::plan_strategy      0x006B9620   ; AI body remains explicit
//!        Leader::compute_score(0)   0x006EC560   ; existing victory-score port
//!        Leader::diplomacy          0x006BC950   ; AI body remains explicit
//!      if Game semaphore bit 9:
//!        Game::check_victory        0x005926B0   ; existing victory-score port
//! ```
//!
//! Everything above is [measured] from a capstone disassembly of `0x006ED2A0`,
//! `0x006CE280`, `0x006CF7C0`, `0x006CF970`, `0x006CDEA0`, `0x006CDCC0`, `0x006B8A20`,
//! `0x006ED430` and `0x006BC860` against `ron-bin/riseofnations.exe` (sha256
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
//!   speed/armor packages are rebuilt from the checked-in shipped type table plus live
//!   leader/object state; missing type identities remain explicit. Wall query population,
//!   `Wall::update_construct_time`, and reached `Object::eject_contents` remain explicit.
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
    /// `0x006BC8B6` / `0x006BC8CB` / `0x006BC91B` — the number of explored region
    /// cells rebuilt by `Leader::check_explore`.
    pub const EXPLORED: usize = 0x9D4;
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
    /// `+0x9D4`, rebuilt by step 11's [`check_explore`] on its phase.
    pub explored: i32,
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
    /// offset block.
    pub unit_stats: UnitLeaderStatState,
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

/// The scalar slice of shipped `UnitTypeData` needed by the two recovered Unit stat bodies.
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

/// Parse the tracked post-load retail table once. The table is the exact source for the
/// load-time-added `unit_flags` bits; reading `unitrules.xml` directly would lose them.
fn shipped_unit_stat_rows() -> &'static [Option<UnitTypeStatRow>] {
    static ROWS: std::sync::OnceLock<Vec<Option<UnitTypeStatRow>>> = std::sync::OnceLock::new();
    ROWS.get_or_init(|| {
        let tsv = include_str!("../../../../schema/live/live-tables-unit.tsv");
        let mut lines = tsv.lines();
        let Some(header) = lines.next() else {
            return Vec::new();
        };
        let names: Vec<&str> = header.split('\t').collect();
        let col = |name: &str| names.iter().position(|candidate| *candidate == name);
        let Some(type_id_col) = col("type_id") else {
            return Vec::new();
        };
        let Some(from_col) = col("from") else {
            return Vec::new();
        };
        let Some(where_col) = col("where") else {
            return Vec::new();
        };
        let Some(obj_masks_col) = col("obj_masks") else {
            return Vec::new();
        };
        let Some(armor_col) = col("armor") else {
            return Vec::new();
        };
        let Some(domain_col) = col("domain") else {
            return Vec::new();
        };
        let Some(graft_col) = col("graft") else {
            return Vec::new();
        };
        let Some(unit_flags_col) = col("unit_flags") else {
            return Vec::new();
        };
        let Some(unit_flags2_col) = col("unit_flags2") else {
            return Vec::new();
        };
        let Some(moves_col) = col("moves") else {
            return Vec::new();
        };
        let max_col = [
            type_id_col,
            from_col,
            where_col,
            obj_masks_col,
            armor_col,
            domain_col,
            graft_col,
            unit_flags_col,
            unit_flags2_col,
            moves_col,
        ]
        .into_iter()
        .max()
        .unwrap_or(0);

        let mut rows = Vec::<Option<UnitTypeStatRow>>::new();
        for line in lines {
            let cells: Vec<&str> = line.split('\t').collect();
            if cells.len() <= max_col {
                continue;
            }
            let parse_i32 = |column: usize| cells[column].parse::<i32>().ok();
            let Some(type_id) = parse_i32(type_id_col) else {
                continue;
            };
            let Ok(index) = usize::try_from(type_id) else {
                continue;
            };
            let Some(row) = (|| {
                Some(UnitTypeStatRow {
                    type_id,
                    from: parse_i32(from_col)?,
                    where_type: parse_i32(where_col)?,
                    obj_masks: parse_i32(obj_masks_col)? as u32,
                    armor: parse_i32(armor_col)?,
                    domain: parse_i32(domain_col)?,
                    graft: parse_i32(graft_col)?,
                    unit_flags: parse_i32(unit_flags_col)? as u32,
                    unit_flags2: parse_i32(unit_flags2_col)? as u32,
                    moves: parse_i32(moves_col)?,
                })
            })() else {
                continue;
            };
            if rows.len() <= index {
                rows.resize(index + 1, None);
            }
            rows[index] = Some(row);
        }
        rows
    })
}

#[inline]
fn shipped_unit_stat_row(type_id: i32) -> Option<UnitTypeStatRow> {
    let index = usize::try_from(type_id).ok()?;
    shipped_unit_stat_rows().get(index).copied().flatten()
}

/// `ObjectTypeData::is(type, 0)` over the exact load-time source relation.
///
/// `ObjectType::init_is_list` caches this result, but the cache is derived rather than
/// walked source data: identity, then `graft`, then recursive `from`. A malformed cycle or
/// a missing ancestor is an explicit missing fact.
fn shipped_unit_is(mut type_id: i32, target: i32) -> Option<bool> {
    let rows = shipped_unit_stat_rows();
    let mut remaining = rows.len().max(1);
    while remaining != 0 {
        remaining -= 1;
        let row = shipped_unit_stat_row(type_id)?;
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
    source: UnitQuerySource,
    leader: &Leader,
) -> Option<(UnitSpeedInputs, UnitArmorInputs)> {
    let row = shipped_unit_stat_row(source.type_id)?;
    let live = leader.unit_stats;
    let supply = row.unit_flags2 & 0x40 != 0;
    let speed = UnitSpeedInputs {
        type_moves: row.moves,
        domain: row.domain,
        unit_flags: row.unit_flags,
        unit_data_flags: source.unit_masks2,
        military_epoch: live.military_epoch,
        has_objmask_2000: row.obj_masks & 0x2000 != 0,
        is_68: shipped_unit_is(source.type_id, 0x68)?,
        is_66: shipped_unit_is(source.type_id, 0x66)?,
        is_64: shipped_unit_is(source.type_id, 0x64)?,
        is_62: shipped_unit_is(source.type_id, 0x62)?,
        is_42: shipped_unit_is(source.type_id, 0x42)?,
        is_3a: shipped_unit_is(source.type_id, 0x3a)?,
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
    pub eject_contents_requested: bool,
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
            // vtable slot.
            o.construct_time_updates += 1;
            c.construct_time_updates += 1;
            c.unresolved_calls += 1;
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
fn wall_band_pass(band: &mut [StatObject]) -> StatPassCounts {
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
            c.unresolved_calls += 1;
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
                if let Some((speed, armor)) = derive_unit_query_packages(source, leader) {
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
            trace.wall_pass[i] = calc_wall_stats(&ls.leaders[i], rules, &mut e.objects);
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

        let c = calc_unit_stats(
            &mut leader,
            &rules,
            &AttritionGates::default(),
            &mut objects,
        );
        let unit = &objects.units[0];
        assert_eq!((unit.myspeed, unit.myarmor), (25, 0));
        assert_eq!(c.unit_query_populations, 1);
        assert_eq!(c.unit_query_misses, 0);
        assert_eq!(c.unresolved_calls, 0);

        // Mutating only the live type identity must replace both packages on the next
        // dirty pass. Caravan is shipped type 59: speed 26, armor 0.
        objects.units[0].unit_query_source.as_mut().unwrap().type_id = 59;
        let c = calc_unit_stats(
            &mut leader,
            &rules,
            &AttritionGates::default(),
            &mut objects,
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
        let c = calc_unit_stats(
            &mut leader,
            &rules,
            &AttritionGates::default(),
            &mut objects,
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
        assert_eq!(Step8Rules::from_block(&block), s);
    }
}
