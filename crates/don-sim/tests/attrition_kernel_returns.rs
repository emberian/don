// SPDX-License-Identifier: GPL-3.0-or-later
//! `UnitData::get_attrition` `0x00608FD0`, `Leader::calc_attrition` `0x006CDEA0`, and the
//! out-of-bounds read `Unit::process_attrition` `0x005E11A0` performs on an unowned tile.
//!
//! These are Rust tests over the don-sim kernels. They are **not** retail differential
//! evidence and they do not move the module past fidelity tier C; what they pin is that
//! the transcription still says what the instruction stream says. Each test names the
//! specific regression it catches.

use don_sim::systems::borders_fog::{
    attrition_period, calc_attrition, get_attrition, special_attrition_period,
    unowned_territory_fallthrough, AttritionInput, AttritionRules, UnownedTerritoryFallthrough,
    ANTI_ATT_BASE, LEADERS_VA, LEADER_STRIDE, WINDOW_KEY_STATES_LEN, WINDOW_KEY_STATES_VA,
};

/// A unit that takes the shipped baseline period of 48 frames, so any test below that
/// expects `0` is expecting a *return*, not an arithmetic coincidence.
fn baseline() -> AttritionInput {
    AttritionInput {
        attacker_attrition: 1,
        victim_anti_att: ANTI_ATT_BASE,
        siege_class: false,
        militia: false,
        type_id: 0,
        domain: 0,
        age_diff: -1,
        owner_has_preq_0x2fe: false,
        unit_is_idle: false,
    }
}

// ---------------------------------------------------------------------------
// 1. The missing return: 0x00609152 / 0x00609161
// ---------------------------------------------------------------------------

/// Catches a `get_attrition` that is missing the `has_preq(0x2FE) && is_idle` return
/// entirely — the state this port was in before 2026-08-10, where an idle Forage-era unit
/// was assigned the ordinary 48-frame period instead of none at all.
#[test]
fn an_idle_unit_whose_owner_holds_the_forage_bonus_takes_no_attrition() {
    let c = AttritionRules::default();

    // The control: same unit, neither condition. 48 frames.
    let plain = get_attrition(&baseline(), &c);
    assert_eq!(plain, 256);
    assert_eq!(attrition_period(plain, &c), Some(48));

    let exempt = AttritionInput {
        owner_has_preq_0x2fe: true,
        unit_is_idle: true,
        ..baseline()
    };
    assert_eq!(
        get_attrition(&exempt, &c),
        0,
        "0x00609168 falls through to the `xor eax, eax` at 0x0060916E"
    );
    assert_eq!(attrition_period(get_attrition(&exempt, &c), &c), None);
}

/// Catches the same arm implemented as an OR, or with one of the two operands dropped.
/// Retail needs *both*: `0x00609159` returns to the divisor path when `has_preq` is zero,
/// `0x00609168` returns to it when `is_idle` is zero.
#[test]
fn the_idle_exemption_needs_both_operands() {
    let c = AttritionRules::default();
    let only_bonus = AttritionInput {
        owner_has_preq_0x2fe: true,
        ..baseline()
    };
    let only_idle = AttritionInput {
        unit_is_idle: true,
        ..baseline()
    };
    assert_eq!(
        get_attrition(&only_bonus, &c),
        256,
        "a moving unit is not exempt"
    );
    assert_eq!(
        get_attrition(&only_idle, &c),
        256,
        "idling without the bonus is not exempt"
    );
}

/// Catches the arm being hoisted above the `militia` split. Retail's militia branch is
/// reached by `jne 0x609177` / `je 0x609122`: it computes `(scale * 100) / (militia + 100)`
/// at `0x00609069` and jumps to `0x0060907F`, never touching either call. A militia unit
/// of a Forage nation standing still therefore still takes attrition — at the *increased*
/// militia rate.
#[test]
fn the_idle_exemption_is_the_non_militia_branch_only() {
    let c = AttritionRules::default();
    let militia = AttritionInput {
        militia: true,
        owner_has_preq_0x2fe: true,
        unit_is_idle: true,
        ..baseline()
    };
    // (256 * 100) / (300 + 100) = 64 -> a 12-frame period, four times faster than baseline.
    assert_eq!(get_attrition(&militia, &c), 64);
    assert_eq!(attrition_period(get_attrition(&militia, &c), &c), Some(12));
}

/// Catches the exemption being placed after the merchant/air divisors *and* implemented as
/// a divide rather than a return: a merchant air unit halves twice, so a "divide by a big
/// number" mutation would still leave a non-zero rate here, while retail returns zero.
#[test]
fn the_idle_exemption_beats_every_later_divisor() {
    let c = AttritionRules::default();
    let merchant_air = AttritionInput {
        type_id: 0x3D,
        domain: 2,
        ..baseline()
    };
    assert_eq!(get_attrition(&merchant_air, &c), 64, "256 / 2 / 2");
    let exempt = AttritionInput {
        owner_has_preq_0x2fe: true,
        unit_is_idle: true,
        ..merchant_air
    };
    assert_eq!(get_attrition(&exempt, &c), 0);
}

// ---------------------------------------------------------------------------
// 2. `domain` is `ObjectTypeData::domain +0x218`, not an unnamed class
// ---------------------------------------------------------------------------

/// Catches the old "1 suppresses attrition" reading being implemented inside the kernel.
/// `0x0060909F` compares `+0x218` against `2` and nothing else; the sea return lives in
/// `Unit::process_attrition` at `0x005E1456`. A kernel that returns 0 for `domain == 1`
/// would be putting the caller's control flow in the callee — and it would then disagree
/// with the caller for any host that had already applied the sea return.
#[test]
fn only_the_air_domain_is_read_by_the_rate_kernel() {
    let c = AttritionRules::default();
    let land = get_attrition(
        &AttritionInput {
            domain: 0,
            ..baseline()
        },
        &c,
    );
    let sea = get_attrition(
        &AttritionInput {
            domain: 1,
            ..baseline()
        },
        &c,
    );
    let air = get_attrition(
        &AttritionInput {
            domain: 2,
            ..baseline()
        },
        &c,
    );
    assert_eq!(land, 256);
    assert_eq!(
        sea, 256,
        "0x0060909F tests `== 2`, so 1 changes nothing here"
    );
    assert_eq!(air, 128, "air halves the rate");
}

/// Catches the same misreading in the trespass-period path. `special_attrition_period`
/// halves for the air domain only; the shipped `peace_attrition` and `assassin_attrition`
/// are both 8 frames.
#[test]
fn the_trespass_periods_halve_for_air_only() {
    let c = AttritionRules::default();
    assert_eq!(special_attrition_period(c.peace_attrition, 0), 8);
    assert_eq!(special_attrition_period(c.peace_attrition, 1), 8);
    assert_eq!(special_attrition_period(c.peace_attrition, 2), 4);
    assert_eq!(special_attrition_period(c.assassin_attrition, 2), 4);
}

// ---------------------------------------------------------------------------
// 3. The unowned-territory fall-through at 0x005E12CD
// ---------------------------------------------------------------------------

/// Catches anyone "fixing" the fall-through by asserting retail returns for every negative
/// territory index. It returns for `-1` *because of what happens to sit there*, and the
/// only reason we can say so is that the address resolves inside `Window::key_states`.
#[test]
fn an_unowned_tile_reads_the_win32_keyboard_state_buffer() {
    let f = unowned_territory_fallthrough(-1);
    let UnownedTerritoryFallthrough::KeyboardState { va, vkey } = f else {
        panic!("leaders.list[-1] must resolve into Window::key_states, got {f:?}");
    };
    // leaders 0x00E3A390 - sizeof(Leader) 0x6EEC = 0x00E334A4.
    assert_eq!(va, 0x00E3_34A4);
    assert_eq!(va, LEADERS_VA - LEADER_STRIDE);
    assert_eq!(vkey, 0xE4, "virtual-key index inside the 256-byte buffer");
    assert!((WINDOW_KEY_STATES_VA..WINDOW_KEY_STATES_VA + WINDOW_KEY_STATES_LEN).contains(&va));
    assert_eq!(
        f.admits_attrition(),
        Some(false),
        "GetKeyboardState never sets bit 0x02, so `test al, 2` at 0x005E12E1 cannot pass"
    );
}

/// Catches the contested marker being folded into the `-1` answer. `World::compute_reg_
/// territory` writes `-2` (`mov ecx, 0xFFFFFFFE` at `0x006B14E8`/`0x006B1515`, stored
/// through `0x006B1731`), and `leaders.list[-2]` lands in unnamed zero-initialised `.data`
/// where the shipped PDB has no symbol at all. Claiming an answer there would be invention.
#[test]
fn the_contested_marker_lands_somewhere_we_have_not_derived() {
    let f = unowned_territory_fallthrough(-2);
    let UnownedTerritoryFallthrough::UnnamedStatic { va } = f else {
        panic!("leaders.list[-2] is outside key_states, got {f:?}");
    };
    assert_eq!(va, 0x00E2_C5B8);
    assert_eq!(va, LEADERS_VA - 2 * LEADER_STRIDE);
    assert_eq!(
        f.admits_attrition(),
        None,
        "underived must stay underived; Some(false) here would be a fabricated continuation"
    );
}

/// Catches the alias machinery being applied to real owners — `0x005E1294`'s `jns` sends
/// every non-negative index down the ordinary owned chain and no aliasing happens.
#[test]
fn an_owned_tile_is_not_a_fallthrough_at_all() {
    for slot in 0..10 {
        assert_eq!(
            unowned_territory_fallthrough(slot),
            UnownedTerritoryFallthrough::Owned { slot }
        );
        assert_eq!(unowned_territory_fallthrough(slot).admits_attrition(), None);
    }
}

// ---------------------------------------------------------------------------
// 4. `Leader::calc_attrition` 0x006CDEA0 — the rate that gates everything
// ---------------------------------------------------------------------------

/// Catches an off-by-one in `[Constants + n*4 + 0x1D4]`. The loop counter `n` is the number
/// of satisfied prerequisites, and the load is indexed by `n` against a base of `0x1D4`,
/// i.e. `attrition_improved[n - 1]` against the real `+0x1D8` array `{1, 2, 4, 8}`.
#[test]
fn the_research_ladder_indexes_attrition_improved_from_the_level_count() {
    let c = AttritionRules::default();
    let ladder = |n: usize| {
        let mut chain = [false; 4];
        for slot in chain.iter_mut().take(n) {
            *slot = true;
        }
        calc_attrition(false, chain, false, false, false, false, &c)
    };
    assert_eq!(
        [ladder(0), ladder(1), ladder(2), ladder(3), ladder(4)],
        [0, 1, 2, 4, 8]
    );
}

/// Catches the chain being counted as a popcount instead of a prefix. `0x006CDED7`'s `je`
/// leaves the loop the first time `has_preq` answers zero, so a gap truncates the count.
#[test]
fn a_gap_in_the_prerequisite_chain_stops_the_count() {
    let c = AttritionRules::default();
    assert_eq!(
        calc_attrition(
            false,
            [true, false, true, true],
            false,
            false,
            false,
            false,
            &c
        ),
        1,
        "three of four held, but 0x2DE broke the chain at 0x006CDED7"
    );
    assert_eq!(
        calc_attrition(
            false,
            [false, true, true, true],
            false,
            false,
            false,
            false,
            &c
        ),
        0
    );
}

/// Catches dropping `give_att_disabled`. `0x006CDEA6` jumps straight to the store, so the
/// leader's whole bonus set is skipped — a wonder cannot resurrect a disabled rate.
#[test]
fn give_att_disabled_zeroes_the_rate_before_any_bonus() {
    let c = AttritionRules::default();
    assert_eq!(
        calc_attrition(true, [true; 4], true, true, true, true, &c),
        0
    );
}

/// The load-bearing one. Catches dropping the `cmove` floor at `0x006CDF2B` and its three
/// siblings: `((50 + 100) * 0) / 100` is zero, and retail replaces that zero with **one**.
/// So the Colosseum, the Russian tribe bonus, the Conquer-the-World bonus or the Kremlin
/// each make a nation with *no* attrition research deal attrition at all. Without the floor
/// every one of these is 0 and `get_attrition` returns 0 forever.
#[test]
fn a_single_bonus_lifts_a_researchless_nation_off_zero() {
    let c = AttritionRules::default();
    let none = [false; 4];
    assert_eq!(
        calc_attrition(false, none, true, false, false, false, &c),
        1
    );
    assert_eq!(
        calc_attrition(false, none, false, true, false, false, &c),
        1
    );
    assert_eq!(
        calc_attrition(false, none, false, false, true, false, &c),
        1
    );
    assert_eq!(
        calc_attrition(false, none, false, false, false, true, &c),
        1
    );
    assert_eq!(
        calc_attrition(false, none, false, false, false, false, &c),
        0,
        "with no bonus at all the rate really is zero"
    );
    // And a non-zero rate is exactly what `get_attrition` needs to produce a period.
    let armed = AttritionInput {
        attacker_attrition: calc_attrition(false, none, true, false, false, false, &c),
        ..baseline()
    };
    assert_eq!(attrition_period(get_attrition(&armed, &c), &c), Some(48));
}

/// Catches the four multipliers being reordered or folded into one product. The `/100` is
/// MSVC's truncating signed divide (`imul 0x51EB851F; sar edx, 5`), so order is observable:
/// the shipped order 0x212 -> 0xD -> CTW -> 0x21A gives 6 at level one, and the same four
/// percentages applied Kremlin-first give 9.
#[test]
fn the_four_multipliers_compose_in_the_shipped_order() {
    let c = AttritionRules::default();
    let one = [true, false, false, false];
    assert_eq!(
        calc_attrition(false, one, true, true, true, true, &c),
        6,
        "1 -> (150*1)/100=1 -> (200*1)/100=2 -> (150*2)/100=3 -> (200*3)/100=6"
    );

    // The mutation this pins: the same set applied Kremlin-first.
    let mut mutated = 1i32;
    for pct in [
        c.kremlin_attrition,
        c.colosseum_attrition,
        c.russian_attrition,
        c.ctw_attrition,
    ] {
        mutated = ((pct + 100) * mutated) / 100;
        if mutated == 0 {
            mutated = 1;
        }
    }
    assert_eq!(mutated, 9);
    assert_ne!(
        calc_attrition(false, one, true, true, true, true, &c),
        mutated
    );

    // Fully researched, all four bonuses: 8 -> 12 -> 24 -> 36 -> 72.
    assert_eq!(
        calc_attrition(false, [true; 4], true, true, true, true, &c),
        72
    );
}

/// Catches a constant swap between the two 50%s and the two 100%s. Each percentage is
/// keyed by the `Constants` byte offset the instruction stream loads, and the shipped
/// values are asymmetric, so a swap moves the answer.
#[test]
fn each_bonus_uses_its_own_shipped_percentage() {
    let c = AttritionRules::default();
    assert_eq!(
        (
            c.colosseum_attrition,
            c.russian_attrition,
            c.ctw_attrition,
            c.kremlin_attrition
        ),
        (50, 100, 50, 100)
    );
    let four = [true; 4]; // att = 8
    assert_eq!(
        calc_attrition(false, four, true, false, false, false, &c),
        12
    );
    assert_eq!(
        calc_attrition(false, four, false, true, false, false, &c),
        16
    );
    assert_eq!(
        calc_attrition(false, four, false, false, true, false, &c),
        12
    );
    assert_eq!(
        calc_attrition(false, four, false, false, false, true, &c),
        16
    );
}
