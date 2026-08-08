//! Differential test of the damage pipeline: `don_sim::damage` vs `FUN_00644130`.
//!
//! Reads as: generate a scenario, write it into the fabricated world
//! (`damage_env.rs`), evaluate the Rust port, call the retail machine code, compare.
//!
//! The Rust side runs **first**, under `catch_unwind`. Two sites in the retail chain use
//! a bare `idiv` with an unchecked divisor; if the port's `idiv_trapping` says this
//! scenario would raise `#DE`, we skip it rather than hand a `SIGFPE` to the child. Those
//! skips are counted and reported — they are a real restriction on the input
//! distribution, not a rounding error.

use crate::damage_env::*;
use don_sim::{damage_traced, CombatRules, DamageInput, DamagePredicates, UnreachedTerms, STEP_NAMES};

/// Call `FUN_00644130`: `__thiscall`, six stack dwords pushed right to left, `ret 0x18`
/// so the callee restores the stack itself.
unsafe fn call_damage(f: u32, this: u32, args: *const u32) -> i32 {
    let ret: i32;
    std::arch::asm!(
        "push dword ptr [{p} + 20]",
        "push dword ptr [{p} + 16]",
        "push dword ptr [{p} + 12]",
        "push dword ptr [{p} + 8]",
        "push dword ptr [{p} + 4]",
        "push dword ptr [{p} + 0]",
        "call {f}",
        p = in(reg) args,
        f = in(reg) f,
        in("ecx") this,
        lateout("eax") ret,
        clobber_abi("C"),
    );
    ret
}

/// One generated world-state. Kept flat and `Copy` so a failing trial can be printed in
/// full and replayed.
#[derive(Clone, Copy, Debug, Default)]
pub struct Scenario {
    pub i: DamageInput,
    pub p: DamagePredicates,
    pub r: CombatRules,
    /// The type ids also decide where in `.data` the balance entry is written.
    pub atk_type_id: i32,
    pub def_type_id: i32,
    /// Raw `build[+0x72]`: negative means "no city", which short-circuits step 30.
    pub city_index: i16,
    /// Raw `city[+0x5F]`, compared against the attacker's player id.
    pub city_owner: i8,
    /// Raw `player[+0x6C5C]`; bit 2 authorises the attacker-mask fixup.
    pub player_byte_6c5c: u8,
}

pub struct Rng(pub u64);
impl Rng {
    pub fn new(seed: u64) -> Rng { Rng(seed) }
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn bit(&mut self) -> bool {
        self.next() & 1 == 1
    }
    /// A value drawn from a mixture: mostly small (where real game numbers live), often
    /// medium, sometimes the full `i32` range so the wrapping multiplies are exercised,
    /// and occasionally a boundary constant.
    fn val(&mut self) -> i32 {
        let r = self.next();
        match r % 8 {
            0..=3 => (r >> 8) as i32 % 200 - 100,
            4..=5 => (r >> 8) as i32 % 20_000 - 10_000,
            6 => (r >> 8) as i32,
            _ => [
                0,
                1,
                -1,
                100,
                -100,
                255,
                256,
                i32::MAX,
                i32::MIN,
                i32::MAX / 2,
                0x0001_0000,
            ][(r >> 8) as usize % 11],
        }
    }
    /// A non-zero divisor. Zero is excluded because retail would `#DE`; `-1` is kept
    /// because it is the other overflow edge and the port must agree on it.
    fn divisor(&mut self) -> i32 {
        loop {
            let v = self.val();
            if v != 0 {
                return v;
            }
        }
    }
    fn masks(&mut self) -> u32 {
        let r = self.next();
        // Bias hard toward the bits the pipeline actually tests, then sprinkle noise:
        // uniform random dwords would leave every interesting guard on ~half the time
        // but never in combination with the rarer ones.
        let interesting: u32 = [
            0x4, 0x8, 0x20, 0x1000, 0x2000, 0x1_0000, 0x100, 0x8, 0x4_0000, 0x20_0000,
            0x800_0000, 0x1000_0000, 0x8000_0000, 0x1_0108,
        ][(r >> 3) as usize % 14];
        let mut m = interesting;
        if r & 1 == 1 {
            m |= (r >> 32) as u32;
        }
        if r & 2 == 2 {
            m |= [0x4u32, 0x8, 0x20, 0x1000, 0x2000, 0x4_0000, 0x20_0000, 0x1000_0000]
                [(r >> 40) as usize % 8];
        }
        m
    }
    /// An angle-ish value: the flank and entrenchment tests compare a biased difference
    /// against sixths of the circle, so uniform noise would almost never land on a
    /// boundary.
    fn angle(&mut self) -> i32 {
        let r = self.next();
        match r % 4 {
            0 => (r >> 8) as i32,
            1 => [
                0i32,
                0x2AAA_AAAAu32 as i32,
                0x2AAA_AAABu32 as i32,
                0x4000_0000,
                0x5555_5555,
                0x6000_0000,
                0x6000_0001,
                0x8000_0000u32 as i32,
                0xAAAA_AAABu32 as i32,
                0xD555_5555u32 as i32,
                0xD555_5556u32 as i32,
            ][(r >> 8) as usize % 11],
            2 => ((r >> 8) as i32) & 0x7FFF_FFFF,
            _ => (r >> 8) as i32 % 512 - 256,
        }
    }
}

pub fn gen_scenario(g: &mut Rng) -> Scenario {
    // Three guards key off literal type ids -- step 6 tests {0x32..0x35}, step 29 tests
    // 0x21D, and the attacker-mask fixup tests {0x1BB, 0x1BC}. Uniform ids in 0..400
    // would never hit any of them, so draw from a mixture. The balance entry for the
    // chosen pair still has to miss the harness's own globals; `balance_collides`
    // rejects the few pairs that do not.
    let atk_type_id = match g.next() % 10 {
        0..=6 => (g.next() % 8) as i32,
        7 | 8 => 0x32 + (g.next() % 4) as i32,
        _ => 0x1BB + (g.next() % 2) as i32,
    };
    let def_type_id = if g.next() % 8 == 0 {
        0x21D
    } else {
        (g.next() % 400) as i32
    };

    let mut s = Scenario {
        atk_type_id,
        def_type_id,
        city_index: if g.bit() { 0 } else { -1 },
        city_owner: if g.bit() { ATK_PLAYER as i8 } else { 7 },
        player_byte_6c5c: if g.bit() { 4 } else { 0 },
        ..Default::default()
    };

    s.i = DamageInput {
        balance_pct: (g.next() as i16) as i32,
        attack: g.val(),
        armor: g.val(),
        attacker_masks: g.masks(),
        defender_masks: g.masks(),
        attack_dir: g.angle(),
        splash_flag: if g.bit() { 1 } else { 0 },
        overkill_gate: if g.bit() { 1 } else { 0 },
        attacker_player: ATK_PLAYER,
        attacker_type_id: atk_type_id,
        attacker_domain: (g.next() % 3) as i32,
        attacker_splash_percent: g.val(),
        attacker_type_0x40: if g.bit() { 0x1AB } else { g.val() },
        attacker_z: g.val(),
        attacker_flag8_bit5: g.bit(),
        defender_type_id: def_type_id,
        defender_domain: (g.next() % 3) as i32,
        defender_type_0x2b8_bit2: g.bit(),
        defender_splash_divisor: g.divisor(),
        defender_flags_0x68: {
            let r = g.next();
            let mut f = 0u32;
            for (bit, sh) in [(1u32, 0), (0x8_0000, 1), (0x40_0000, 2), (0x200_0000, 3)] {
                if (r >> sh) & 1 == 1 {
                    f |= bit;
                }
            }
            f
        },
        defender_flags_0x6c_bit12: g.bit(),
        defender_z: g.val(),
        defender_facing: g.angle(),
        defender_facing_entrench: g.angle(),
        defender_overkill_stamp: if g.bit() { 0 } else { g.val() },
        defender_word_0xa4: (g.next() as i16) as i32,
        attacker_vf_0xe4: if g.bit() { 0 } else { g.val() },
        current_frame: g.val(),
        game_flag_0x821_bit1: g.bit(),
        tile_rocky: g.bit(),
        tile_owner: if g.bit() { ATK_PLAYER as i32 } else { (g.next() % 8) as i32 },
    };

    let atk20 = g.bit();
    s.p = DamagePredicates {
        mask_fixup_authorised: s.player_byte_6c5c & 4 != 0,
        attacker_vf_0x18: g.bit(),
        attacker_vf_0x1c: g.bit(),
        attacker_vf_0x20: atk20,
        attacker_vf_0x130: g.bit(),
        attacker_type_vf_0x10c: g.bit(),
        // The harness resolves both reads through the same object, so they cannot be
        // driven apart here. See docs/derivation/damage-port.md.
        attacker_table_vf_0x20: atk20,
        defender_vf_0x18: g.bit(),
        defender_vf_0x1c: g.bit(),
        defender_vf_0x20: g.bit(),
        defender_vf_0x120: g.bit(),
        defender_vf_0xcc: g.bit(),
        defender_vf_0xd0: g.bit(),
        defender_vf_0xd8: g.bit(),
        defender_type_vf_0x10c: g.bit(),
        defender_build_flag: g.bit(),
        defender_build_0x20: g.bit(),
        recapture_owner_matches: false, // filled in below from city_index / city_owner
        defender_carrier_vf_0x184: g.bit(),
        defender_carrier_vf_0x20: g.bit(),
        defender_mount_attack_is_zero: g.bit(),
        attacker_tech_0x42: g.bit(),
        attacker_tech_0x139: g.bit(),
        attacker_tech_0x83: g.bit(),
        defender_tech_0x216: g.bit(),
        defender_tech_0x143: g.bit(),
        defender_tech_0x109: g.bit(),
        // Structurally unreachable in this harness; see the report.
        step10_bonus_applies: false,
        step11_player_prop_0xf: false,
        step27_player_prop_0xd: false,
        step12_team_differs: false,
        game_mode_is_2: false,
    };
    s.p.recapture_owner_matches =
        s.city_index >= 0 && s.city_owner as i32 == ATK_PLAYER as i32;

    s.r = CombatRules {
        height_increment: g.divisor(),
        height_bonus: g.val(),
        flank_bonus: g.val(),
        cavalry_flank_bonus: g.val(),
        vehicle_flank_bonus: g.val(),
        rocky_modifier: g.val(),
        overkill_frames: g.val(),
        overkill_damage: g.val(),
        entrenchment_modifier: g.val(),
        river_modifier: g.val(),
        recapture_city_modifier: g.val(),
        red_fort_air_defense: g.val(),
        rule_0x558: if g.bit() { 0 } else { g.val() },
        rule_0x76c: g.val(),
        rule_0xb98: g.val(),
    };
    s
}

/// Write a scenario into the fabricated world.
///
/// `wr32` writes into the mapped image (the global pointers and the balance entry); the
/// arena writes go through `a`.
pub fn apply(s: &Scenario, a: &Arena, wr32: &dyn Fn(u32, u32), wr16: &dyn Fn(u32, u16)) {
    // ---- control block: one dword per stubbed virtual --------------------------------
    let b = |v: bool| if v { 1u32 } else { 0u32 };
    a.w32(O_PRED + 4 * C_ATK_18, b(s.p.attacker_vf_0x18));
    a.w32(O_PRED + 4 * C_ATK_1C, b(s.p.attacker_vf_0x1c));
    a.w32(O_PRED + 4 * C_ATK_20, b(s.p.attacker_vf_0x20));
    a.w32(O_PRED + 4 * C_ATK_130, b(s.p.attacker_vf_0x130));
    a.w32(O_PRED + 4 * C_ATK_E4, s.i.attacker_vf_0xe4 as u32);
    a.w32(O_PRED + 4 * C_ATK_D0, 0); // pinned: nonzero sends get_attack into FUN_006DB810
    a.w32(O_PRED + 4 * C_ATK_CC, 0);
    a.w32(O_PRED + 4 * C_ATK_C8, 0);
    a.w32(O_PRED + 4 * C_ATKT_EC, 0);
    a.w32(O_PRED + 4 * C_ATKT_10C, b(s.p.attacker_type_vf_0x10c));
    a.w32(O_PRED + 4 * C_DEF_18, b(s.p.defender_vf_0x18));
    a.w32(O_PRED + 4 * C_DEF_1C, b(s.p.defender_vf_0x1c));
    a.w32(O_PRED + 4 * C_DEF_20, b(s.p.defender_vf_0x20));
    a.w32(O_PRED + 4 * C_DEF_120, b(s.p.defender_vf_0x120));
    a.w32(O_PRED + 4 * C_DEF_CC, b(s.p.defender_vf_0xcc));
    a.w32(O_PRED + 4 * C_DEF_D0, b(s.p.defender_vf_0xd0));
    a.w32(O_PRED + 4 * C_DEF_D8, b(s.p.defender_vf_0xd8));
    a.w32(O_PRED + 4 * C_DEF_C8, 0);
    a.w32(O_PRED + 4 * C_DEF_3C, a.addr(O_BUILD));
    a.w32(O_PRED + 4 * C_DEF_40, a.addr(O_CARRIER));
    a.w32(O_PRED + 4 * C_DEFT_EC, 0);
    a.w32(O_PRED + 4 * C_DEFT_10C, b(s.p.defender_type_vf_0x10c));
    a.w32(O_PRED + 4 * C_CARR_184, b(s.p.defender_carrier_vf_0x184));
    a.w32(O_PRED + 4 * C_CARR_20, b(s.p.defender_carrier_vf_0x20));
    a.w32(O_PRED + 4 * C_AUX_14, 0); // pinned: nonzero reaches FUN_006DB810
    a.w32(O_PRED + 4 * C_ATK_3C, a.addr(O_BUILD));

    // ---- tech tables, indexed by the low byte of the queried tech id -----------------
    for i in 0..256 {
        a.w32(O_TECH_ATK + i * 4, 0);
        a.w32(O_TECH_DEF + i * 4, 0);
    }
    a.w32(O_TECH_ATK + 0x42 * 4, b(s.p.attacker_tech_0x42));
    a.w32(O_TECH_ATK + 0x39 * 4, b(s.p.attacker_tech_0x139));
    a.w32(O_TECH_ATK + 0x83 * 4, b(s.p.attacker_tech_0x83));
    a.w32(O_TECH_DEF + 0x16 * 4, b(s.p.defender_tech_0x216));
    a.w32(O_TECH_DEF + 0x43 * 4, b(s.p.defender_tech_0x143));
    a.w32(O_TECH_DEF + 0x09 * 4, b(s.p.defender_tech_0x109));

    // ---- attacker object -------------------------------------------------------------
    a.w8(O_ATK_OBJ + 8, if s.i.attacker_flag8_bit5 { 0x20 } else { 0 });
    a.w8(O_ATK_OBJ + 9, ATK_PLAYER as u8);
    a.w16(O_ATK_OBJ + 0xA, ATK_INDEX as u16);
    a.w32(O_ATK_OBJ + 0xC, (s.i.attacker_z as u32) ^ 0x63637);

    // ---- defender object -------------------------------------------------------------
    a.w8(O_DEF_OBJ + 9, DEF_PLAYER as u8);
    a.w16(O_DEF_OBJ + 0xA, DEF_INDEX as u16);
    a.w32(O_DEF_OBJ + 0xC, (s.i.defender_z as u32) ^ 0x63637);
    a.w32(O_DEF_OBJ + 0x10, 0x63637); // unmasks to 0 -> tile record 0
    a.w32(O_DEF_OBJ + 0x14, 0x63637);
    a.w32(O_DEF_OBJ + 0x4C, s.i.defender_overkill_stamp as u32);
    a.w32(O_DEF_OBJ + 0x50, s.i.defender_facing as u32);
    a.w32(O_DEF_OBJ + 0x5C, s.i.defender_facing_entrench as u32);
    a.w32(O_DEF_OBJ + 0x68, s.i.defender_flags_0x68);
    a.w32(O_DEF_OBJ + 0x6C, if s.i.defender_flags_0x6c_bit12 { 0x1000 } else { 0 });
    a.w16(O_DEF_OBJ + 0xA4, s.i.defender_word_0xa4 as u16);

    // ---- unit types ------------------------------------------------------------------
    a.w32(O_ATK_TYPE + 4, s.atk_type_id as u32);
    a.w32(O_ATK_TYPE + 0x40, s.i.attacker_type_0x40 as u32);
    a.w32(O_ATK_TYPE + 0x1E4, s.i.attacker_masks);
    a.w32(O_ATK_TYPE + 0x1E8, s.i.attack as u32);
    a.w32(O_ATK_TYPE + 0x204, s.i.attacker_splash_percent as u32);
    a.w32(O_ATK_TYPE + 0x218, s.i.attacker_domain as u32);

    a.w32(O_DEF_TYPE + 4, s.def_type_id as u32);
    a.w32(O_DEF_TYPE + 0x1E4, s.i.defender_masks);
    a.w32(O_DEF_TYPE + 0x214, s.i.armor as u32);
    a.w32(O_DEF_TYPE + 0x218, s.i.defender_domain as u32);
    a.w8(O_DEF_TYPE + 0x2B8, if s.i.defender_type_0x2b8_bit2 { 4 } else { 0 });
    a.w32(O_DEF_TYPE + 0x308, s.i.defender_splash_divisor as u32);

    // ---- build / carrier / mount ------------------------------------------------------
    let build8 = (if s.p.defender_build_flag { 4u8 } else { 0 })
        | (if s.p.defender_build_0x20 { 0x20 } else { 0 });
    a.w8(O_BUILD + 8, build8);
    a.w16(O_BUILD + 0x72, s.city_index as u16);
    a.w32(
        O_MOUNT_TYPE + 0x1E8,
        if s.p.defender_mount_attack_is_zero { 0 } else { 1 },
    );
    a.w8(O_CITY + 0x5F, s.city_owner as u8);

    // ---- world ------------------------------------------------------------------------
    a.w8(O_GAME + 0x20, 4); // pinned: makes FUN_006E1370 return false unconditionally
    a.w8(O_GAME + 0x24, 0); // pinned: keeps FUN_006DA000 out of step 12
    a.w32(O_GAME + 0x550, s.i.current_frame as u32);
    a.w8(O_GAME + 0x821, if s.i.game_flag_0x821_bit1 { 2 } else { 0 });
    a.w8(O_TILES, if s.i.tile_rocky { 8 } else { 0 });
    a.w8(O_TILES + 0xF, s.i.tile_owner as u8);
    a.w16(O_PLAYERS + 0x59DE, 0); // pinned: cuts the step-10 chain before FUN_00646B00
    a.w8(O_PLAYERS + 0x6C5C, s.player_byte_6c5c);

    // ---- rules ------------------------------------------------------------------------
    let r = &s.r;
    for (off, v) in [
        (0x44usize, r.height_increment),
        (0x48, r.height_bonus),
        (0x4C, r.flank_bonus),
        (0x50, r.cavalry_flank_bonus),
        (0x54, r.vehicle_flank_bonus),
        (0x58, r.rocky_modifier),
        (0x5C, r.overkill_frames),
        (0x60, r.overkill_damage),
        (0x64, r.entrenchment_modifier),
        (0x68, r.river_modifier),
        (0x6C, r.recapture_city_modifier),
        (0x4C4, r.red_fort_air_defense),
        (0x558, r.rule_0x558),
        (0x76C, r.rule_0x76c),
        (0xB98, r.rule_0xb98),
        (0x8B8, 0),
        (0xBBC, 0),
    ] {
        a.w32(O_RULES + off, v as u32);
    }

    // ---- balance entry ----------------------------------------------------------------
    let idx = s.atk_type_id.wrapping_mul(493).wrapping_add(s.def_type_id);
    wr16(
        VA_BALANCE.wrapping_add((idx as u32).wrapping_mul(2)),
        s.i.balance_pct as u16,
    );
    let _ = wr32;
}

/// True when the balance entry for these type ids would land on memory the harness owns.
pub fn balance_collides(atk: i32, def: i32) -> bool {
    let idx = atk.wrapping_mul(493).wrapping_add(def);
    let addr = VA_BALANCE.wrapping_add((idx as u32).wrapping_mul(2));
    RESERVED.iter().any(|&(lo, hi)| addr + 2 > lo && addr < hi)
}

/// Write every global pointer the pipeline dereferences.
pub fn install_globals(a: &Arena, wr32: &dyn Fn(u32, u32)) {
    wr32(G_C061D0, a.addr(O_MAP));
    wr32(G_C061D4, a.addr(O_C061D4));
    wr32(G_C061E0, a.addr(O_PLAYERS));
    wr32(G_C061E4, a.addr(O_RULES));
    wr32(G_C061E8, a.addr(O_GAME));
    wr32(G_CAE5FC, a.addr(O_COORD));
    wr32(G_E85DDC, a.addr(O_AUXROOT));
    wr32(G_C0AB84 + ATK_PLAYER * 28, a.addr(O_PTRARR0));
    wr32(G_C0AB84 + DEF_PLAYER * 28, a.addr(O_PTRARR1));
    wr32(G_C0AEC0 + ATK_PLAYER * 28, a.addr(O_PTRARR0));
    wr32(G_C0AEC0 + DEF_PLAYER * 28, a.addr(O_PTRARR1));
}

pub struct Report {
    pub trials: u32,
    pub mismatches: u32,
    pub skipped_de: u32,
    pub skipped_collide: u32,
    pub unexpected_panics: u32,
    pub first_panic: Option<(Scenario, String)>,
    pub coverage: [u32; 30],
    pub first_bad: Option<(Scenario, i32, i32)>,
}

/// Run `trials` randomised scenarios. `damage_fn` is the relocated address of
/// `FUN_00644130`.
pub fn run(a: &Arena, damage_fn: u32, trials: u32, seed: u64, wr32: &dyn Fn(u32, u32), wr16: &dyn Fn(u32, u16)) -> Report {
    let mut g = Rng(seed);
    let mut rep = Report {
        trials: 0,
        mismatches: 0,
        skipped_de: 0,
        skipped_collide: 0,
        unexpected_panics: 0,
        first_panic: None,
        coverage: [0; 30],
        first_bad: None,
    };
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));

    for _ in 0..trials {
        let s = gen_scenario(&mut g);
        if balance_collides(s.atk_type_id, s.def_type_id) {
            rep.skipped_collide += 1;
            continue;
        }
        let u = UnreachedTerms::default();
        let modelled = std::panic::catch_unwind(|| damage_traced(&s.i, &s.p, &s.r, &u));
        let (expect, trace) = match modelled {
            Ok(v) => v,
            Err(e) => {
                // Only an idiv trap is a legitimate skip. Any other panic would mean the
                // port faulted where retail would not, and silently counting it as a
                // skip is exactly how a suite launders a divergence into a pass.
                let msg = e
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_default();
                if msg.starts_with("retail raises #DE") {
                    rep.skipped_de += 1;
                    continue;
                }
                rep.unexpected_panics += 1;
                if rep.first_panic.is_none() {
                    rep.first_panic = Some((s, msg));
                }
                continue;
            }
        };

        if std::env::var_os("DAMAGE_TRACE").is_some() {
            eprintln!("TRIAL {:?}", s);
        }
        apply(&s, a, wr32, wr16);
        a.w32(O_OUTKIND, 0);
        let args: [u32; 6] = [
            DEF_INDEX as u32,
            DEF_PLAYER,
            s.i.attack_dir as u32,
            s.i.splash_flag as u32,
            s.i.overkill_gate as u32,
            a.addr(O_OUTKIND),
        ];
        let got = unsafe { call_damage(damage_fn, a.addr(O_ATK_OBJ), args.as_ptr()) };

        rep.trials += 1;
        for b in 0..30 {
            if trace >> b & 1 == 1 {
                rep.coverage[b] += 1;
            }
        }
        if got != expect {
            rep.mismatches += 1;
            if rep.first_bad.is_none() {
                rep.first_bad = Some((s, expect, got));
            }
        }
    }
    std::panic::set_hook(prev_hook);
    rep
}

pub fn print_report(rep: &Report) {
    println!(
        "  trials {}   mismatches {}   skipped(#DE) {}   skipped(collision) {}   unexpected panics {}",
        rep.trials, rep.mismatches, rep.skipped_de, rep.skipped_collide, rep.unexpected_panics
    );
    if let Some((s, m)) = &rep.first_panic {
        println!("  UNEXPECTED PANIC {m}\n    {:?}", s);
    }
    println!("  step coverage (trials in which the step's arithmetic ran):");
    for (n, name) in STEP_NAMES.iter().enumerate() {
        let c = rep.coverage[n];
        let flag = if c == 0 { "  <-- NEVER TAKEN" } else { "" };
        println!("    {:<16} {:>8}{}", name, c, flag);
    }
    if let Some((s, e, g)) = &rep.first_bad {
        println!("  FIRST MISMATCH expect={e} got={g}");
        println!("    {:?}", s);
    }
}

/// Print retail's answer for a fixed set of scenarios, formatted as Rust assertions.
///
/// This exists so `don-sim`'s unit tests carry **captured** expectations. The ledger
/// already records one case where a hand-computed expectation was simply wrong and the
/// test enshrined the error; capture, do not calculate.
pub fn print_vectors(a: &Arena, damage_fn: u32, wr32: &dyn Fn(u32, u32), wr16: &dyn Fn(u32, u16)) {
    let call = |s: &Scenario| -> i32 {
        apply(s, a, wr32, wr16);
        a.w32(O_OUTKIND, 0);
        let args: [u32; 6] = [
            DEF_INDEX as u32,
            DEF_PLAYER,
            s.i.attack_dir as u32,
            s.i.splash_flag as u32,
            s.i.overkill_gate as u32,
            a.addr(O_OUTKIND),
        ];
        unsafe { call_damage(damage_fn, a.addr(O_ATK_OBJ), args.as_ptr()) }
    };

    // A scenario with every predicate false and every rule zero: the chain collapses to
    // its spine (steps 1, 21, 22, 28). height_increment must stay non-zero or the height
    // step would #DE -- but its guard is closed here anyway.
    let spine = |attack: i32, balance: i32, armor: i32| -> Scenario {
        Scenario {
            atk_type_id: 1,
            def_type_id: 1,
            city_index: -1,
            city_owner: 9,
            player_byte_6c5c: 0,
            i: DamageInput {
                balance_pct: balance,
                attack,
                armor,
                attacker_player: ATK_PLAYER,
                attacker_type_id: 1,
                defender_type_id: 1,
                defender_splash_divisor: 1,
                tile_owner: ATK_PLAYER as i32,
                ..Default::default()
            },
            p: DamagePredicates::default(),
            r: CombatRules { height_increment: 1, ..Default::default() },
        }
    };

    println!("// captured from retail FUN_00644130 via `oracle damage-vectors`");
    for (att, bal, arm) in [
        (100, 100, 0),
        (100, 100, 3),
        (45, 100, 10),
        (0, 0, 0),
        (105, 100, 0),
        (104, 100, 0),
        (1000, 250, 7),
        (-100, 100, 0),
        (i32::MAX, 100, 0),
        (100, -100, 0),
        (100, 100, -5),
        (5, 100, 0),
        (4, 100, 0),
        (10, 100, 100),
    ] {
        let s = spine(att, bal, arm);
        println!("assert_eq!(spine({att}, {bal}, {arm}), {});", call(&s));
    }

    // Land attacker vs sea defender: the floor at 0x00644F13 is skipped.
    let mut s = spine(1, 100, 50);
    s.i.attacker_domain = 0;
    s.i.defender_domain = 1;
    println!("// land attacker (domain 0) vs sea defender (domain 1), attack=1 balance=100 armor=50");
    println!("//   -> {}", call(&s));

    // splash suppresses the floor and adds the divide + splash-percent steps.
    let mut s = spine(10, 100, 100);
    s.i.splash_flag = 1;
    s.i.defender_splash_divisor = 1;
    s.i.attacker_splash_percent = 0;
    println!("// splash_flag=1 splash_percent=0 divisor=1, attack=10 balance=100 armor=100");
    println!("//   -> {}", call(&s));

    // The ×10 internal attack scale, shown against the rescale boundary.
    let mut s = spine(100, 100, 20);
    s.i.overkill_gate = 1;
    s.i.defender_overkill_stamp = 1;
    s.i.current_frame = 2;
    s.i.attacker_vf_0xe4 = 1;
    s.i.defender_word_0xa4 = 2;
    s.p.attacker_vf_0x130 = true;
    s.p.attacker_vf_0x18 = true;
    s.p.defender_vf_0x18 = true;
    s.r.overkill_frames = 30;
    s.r.overkill_damage = 128;
    println!("// armor mid-chain then overkill x128/256: attack=100 balance=100 armor=20");
    println!("//   -> {}", call(&s));
    s.i.splash_flag = 1;
    s.i.attacker_splash_percent = 100;
    println!("// same, splash_flag=1 (floor suppressed), splash_percent=100 divisor=1");
    println!("//   -> {}", call(&s));

    // Attacker-mask fixup: the `or eax,0x40` Ghidra drops. Only observable through a
    // downstream mask test, so pair it with the flank guard on bit 2 (AM & 4).
    let mut s = spine(1000, 100, 0);
    s.i.attacker_flag8_bit5 = true;
    s.player_byte_6c5c = 4;
    s.i.attacker_masks = 0x0002_0000;
    s.p.mask_fixup_authorised = true;
    println!("// mask fixup gate open, AM=0x20000 -> retail clears bit 17 and sets bit 6");
    println!("//   -> {}", call(&s));
}
