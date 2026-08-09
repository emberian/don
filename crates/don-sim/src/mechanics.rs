//! Mechanics derived from the binary.
//!
//! Nothing enters this module without a `docs/provenance-ledger.md` entry naming the
//! function it came from and the evidence behind it. Placeholder logic lives in
//! `world.rs` and is marked as such; this module is for the real thing.

/// Map two integers into the inclusive range spanned by `lo` and `hi`.
///
/// Derived from `riseofnations.exe` at VA `0x00846450` (`__stdcall`, four dword args,
/// `ret 0x10`). The retail body is:
///
/// ```text
/// eax = hi - lo ; cdq ; ecx = |eax| + 1        ; branchless abs via xor/sub with sign
/// eax = a ; imul eax, eax ; imul eax, b        ; signed 32-bit, wrapping
/// cdq ; idiv ecx                               ; edx = truncated remainder
/// eax = edx + lo
/// ```
///
/// Semantics worth stating because they are easy to get wrong and were confirmed, not
/// assumed:
///
/// * All multiplication wraps (signed 32-bit overflow is not saturating).
/// * `idiv` truncates toward zero, so the remainder takes the sign of the **dividend**.
///   `a*a*b` can be negative — `a*a` alone can wrap negative — so the result can fall
///   *below* `lo`. This is faithful behaviour, not a bug in this port.
/// * `hi` is not required to exceed `lo`; the divisor uses `|hi - lo|`.
///
/// # Fidelity
///
/// **Tier B** — differentially tested against the retail code under
/// `crates/oracle`: 500,008 inputs (8 hand-chosen edge cases covering `i32::MIN`,
/// `i32::MAX`, zero ranges and inverted ranges, plus 500,000 pseudo-random), zero
/// mismatches. Testing, not proof: this is not a claim about all 2^128 inputs.
///
/// The name is descriptive of the computation. What the engine *uses* it for is not yet
/// established, so it deliberately does not claim to be "the RNG" or "the damage roll".
#[inline]
pub fn hash_into_range(a: i32, b: i32, lo: i32, hi: i32) -> i32 {
    let divisor = hi.wrapping_sub(lo).wrapping_abs().wrapping_add(1);
    a.wrapping_mul(a)
        .wrapping_mul(b)
        .wrapping_rem(divisor)
        .wrapping_add(lo)
}

// ============================================================================
// The damage pipeline — a port of `FUN_00644130` in `ron-bin/riseofnations.exe`.
// ============================================================================
//
// # Provenance
//
// Everything in this module comes from the machine code at VA `0x00644130`
// (`__thiscall`, six stack dwords, `ret 0x18`) plus its two operand suppliers
// `Object::get_attack` at `0x006469F0` and `Object::get_armor` at `0x00647DB0`, and the
// flank classifier at `0x0092CFE0`. The structural walk-through is
// `docs/derivation/combat.md`; the port and its differential evidence are
// `docs/derivation/damage-port.md`. Every arithmetic step below carries the VA of the
// instruction that performs it, so a reader can re-disassemble and check the line.
//
// Ghidra's decompilation of `0x00644130` was used only for control-flow shape. It is
// **wrong in at least one place** — it drops the `or eax, 0x40` at `0x006442AB` from the
// attacker-mask fixup — which is exactly why the charter says decompiled C is a
// hypothesis. Values here were transcribed from capstone output, then checked against
// retail execution.
//
// # Fidelity
//
// Mixed, and deliberately not averaged into one number:
//
// * The **arithmetic chain** as driven through retail machine code in the oracle is
//   **Tier B** — see `docs/derivation/damage-port.md` for the sample count, the input
//   distribution, and the exact list of steps the harness could and could not reach.
// * Steps that the harness could not reach (10, 11, 27, and the upgrade branches of
//   `get_attack`/`get_armor`) are implemented from the disassembly and are **unverified**
//   — no execution has ever compared them. They are marked `UNVERIFIED` inline.
// * The **predicates** are not implemented at all. They are inputs
//   ([`DamagePredicates`]), because resolving them means walking the engine's object
//   graph, which this crate does not have yet. Nothing here claims to reproduce *which*
//   modifiers fire in a real game, only what the arithmetic does once they have.
//
// Nothing here is Tier A. Nothing here is verified in the proof-assistant sense.
//
// # The shape of the computation
//
// Pure 32-bit signed integer arithmetic — there is no `xmm` operand and no `f*` mnemonic
// anywhere in `0x00644130..0x00645060`, so the IEEE hazard surface documented in
// `docs/binary-ground-truth.md` does not touch this subsystem at all.
//
// Two properties that a formula written in display units silently loses:
//
// * Attack is carried **×10** (the unit parser multiplies `ATTACK` by 10 at
//   `0x0061B01B`), and the chain rescales exactly once, with `(D + 5) / 10` at
//   `0x00644B7D` — round-half-up for non-negative `D`.
// * Armor is subtracted at `0x00644B91`, which is step 22 of 31. Eight further
//   modifiers apply *after* it, so an intermediate can be negative and then be scaled.
//
// # Overflow
//
// Every multiply in the retail chain is a 32-bit `imul` whose low half is kept, so this
// port wraps rather than promoting to 64-bit. Two sites use a real `idiv` with a divisor
// the engine never checks (`0x006448B9`, `0x00644D78`); a zero or overflowing divisor
// there raises `#DE` in retail, and [`damage`] panics with the site's address instead of
// inventing a value.

/// Signed division that traps exactly where retail's `idiv` traps.
///
/// `idiv` raises `#DE` both on a zero divisor and on `i32::MIN / -1`. The engine checks
/// neither at `0x006448B9` or `0x00644D78`; it relies on the shipped data never producing
/// one. Panicking is the closest faithful analogue — inventing a result would be a
/// silent divergence from a state the real process cannot survive.
#[inline]
#[track_caller]
fn idiv_trapping(num: i32, den: i32, site: &str) -> i32 {
    if den == 0 || (num == i32::MIN && den == -1) {
        panic!("retail raises #DE here: idiv {num} / {den} at {site}");
    }
    num / den
}

/// `x / 100`, truncating, from a wrapped 32-bit product.
///
/// Compiler idiom at `0x006442B1` and eight other sites: `imul` the operands as 32 bits,
/// then `mov eax, 0x51EB851F; imul; sar edx, 5; +signbit`. That magic sequence computes
/// exactly `trunc(x / 100)` over the whole `i32` range, so the port may use `/` — but the
/// **product must wrap first**, which is why callers pass an already-wrapped `x`.
#[inline]
fn div100(x: i32) -> i32 {
    x / 100
}

/// `x / 256`, truncating. Idiom at `0x006449C7` and five other sites:
/// `cdq; and edx, 0xFF; lea r, [edx+eax]; sar r, 8` — i.e. bias-then-shift, which is
/// truncation toward zero, *not* an arithmetic shift.
#[inline]
fn div256(x: i32) -> i32 {
    x / 256
}

/// The rules.xml constants the damage chain reads out of the `RULES` singleton.
///
/// The singleton is reached through `[0x00C061E4]`; `[0x00C061F0]` aliases the same
/// object (confirmed by a live-process read, see `docs/provenance-ledger.md`). Field
/// names come from `FUN_00570170`'s bindings where they are known; offsets are given for
/// the ones that are not, because naming them from folklore would be exactly the drift
/// the charter forbids.
///
/// The divisor each field is applied with tells you its stored format: `/100` fields hold
/// plain integer percents, `/256` fields hold 8.8 fixed point.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CombatRules {
    /// `+0x44` `HEIGHT_INCREMENT`. Denominator, multiplied by 100 at `0x00644D70`.
    /// Zero raises `#DE`.
    pub height_increment: i32,
    /// `+0x48` `HEIGHT_BONUS`. Integer percent per increment.
    pub height_bonus: i32,
    /// `+0x4C` `FLANK_BONUS`. Integer percent per flank level.
    pub flank_bonus: i32,
    /// `+0x50` `CAVALRY_FLANK_BONUS`. 8.8 fixed point, scales `flank_bonus`.
    pub cavalry_flank_bonus: i32,
    /// `+0x54` `VEHICLE_FLANK_BONUS`. 8.8 fixed point, scales `flank_bonus`.
    pub vehicle_flank_bonus: i32,
    /// `+0x58` `ROCKY_MODIFIER`. 8.8 fixed point.
    pub rocky_modifier: i32,
    /// `+0x5C` `OVERKILL_FRAMES`. A frame count, compared against a frame delta.
    pub overkill_frames: i32,
    /// `+0x60` `OVERKILL_DAMAGE`. 8.8 fixed point.
    pub overkill_damage: i32,
    /// `+0x64` `ENTRENCHMENT_MODIFIER`. 8.8 fixed point.
    pub entrenchment_modifier: i32,
    /// `+0x68` `RIVER_MODIFIER`. 8.8 fixed point.
    pub river_modifier: i32,
    /// `+0x6C` (decimal 108) `RECAPTURE_CITY_MODIFIER`. 8.8 fixed point. Shipped
    /// `"2/1"` through `String::fraction(256)` → **512**, i.e. exactly double.
    ///
    /// Step 30 is the last thing `get_damage` does and it *replaces* the accumulated
    /// damage with `d * 512 / 256`. It fires when the defender is a building whose
    /// `[+8] & 0x20` is set (a city centre) and the city's race matches the attacker's
    /// player — recapturing your own razed city hits twice as hard.
    /// `ObjectData::get_damage` is its only reader in the whole corpus.
    pub recapture_city_modifier: i32,
    /// `+0x4C4` `RED_FORT_AIR_DEFENSE`. Integer percent, applied as `(100 - v)/100`.
    pub red_fort_air_defense: i32,
    /// `+0x558`. Gate for the step-29 zeroing. Name not established.
    pub rule_0x558: i32,
    /// `+0x76C`. Integer percent, applied as `(100 + v)/100`. Name not established.
    pub rule_0x76c: i32,
    /// `+0xB98`. 8.8 fixed point, applied inside the entrenchment branch. Name not
    /// established.
    pub rule_0xb98: i32,
}

/// Everything the chain reads that is not a rules constant and not a predicate.
///
/// Field comments give the retail source of each value. `attack` and `armor` are the
/// *returns* of the two virtual getters, not raw type fields, because both getters add an
/// upgrade term — see [`get_attack`] and [`get_armor`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DamageInput {
    // ---- operands read before the chain starts ----
    /// `0x0064418E`: `(i32)(i16) balance[atk_type_id * 493 + def_type_id]`.
    pub balance_pct: i32,
    /// `0x006441BE`: `attacker->vtbl[0x120]()`, i.e. [`get_attack`]. Carried ×10.
    pub attack: i32,
    /// `0x006441B1`: `defender->vtbl[0x124]()`, i.e. [`get_armor`]. Display scale.
    pub armor: i32,
    /// `0x006441D3`: attacker `UnitType[+0x1E4]`, the `OBJ_MASK` bit-set.
    pub attacker_masks: u32,
    /// `0x006441DF`: defender `UnitType[+0x1E4]`.
    pub defender_masks: u32,

    // ---- the six stack arguments, minus the two that name the defender ----
    /// `ebp+0x10`. An angle, differenced against the defender's facing in steps 20 and 26.
    pub attack_dir: i32,
    /// `ebp+0x14`. Non-zero selects the splash path (steps 13/14/15/16) and suppresses
    /// the floor of 1.
    pub splash_flag: i32,
    /// `ebp+0x18`. Gates the whole overkill block (step 23).
    pub overkill_gate: i32,

    // ---- attacker object / type ----
    /// `[attacker+9]`, the owning player id.
    pub attacker_player: u32,
    /// `attacker UnitType[+4]`, the global type id (also the balance-table row).
    pub attacker_type_id: i32,
    /// `attacker UnitType[+0x218]`. Land = 0, Sea = 1, Air = 2 (inferred from the
    /// domain tests at `0x0064459B` and `0x00644F3E`, not from a shipped name).
    pub attacker_domain: i32,
    /// `attacker UnitType[+0x204]`, `SPLASH_PERCENT`.
    pub attacker_splash_percent: i32,
    /// `attacker UnitType[+0x40]`.
    pub attacker_type_0x40: i32,
    /// `(attacker[+0xC] ^ 0x63637)`, the XOR-masked height. Unmask before passing.
    pub attacker_z: i32,
    /// `[attacker+8] & 0x20`, one input to the mask-fixup gate at `0x00644218`.
    pub attacker_flag8_bit5: bool,

    // ---- defender object / type ----
    /// `defender UnitType[+4]`, the global type id (also the balance-table column).
    pub defender_type_id: i32,
    /// `defender UnitType[+0x218]`.
    pub defender_domain: i32,
    /// `defender UnitType[+0x2B8] & 4`.
    pub defender_type_0x2b8_bit2: bool,
    /// `defender UnitType[+0x308]`, the splash divisor. A real `idiv` at `0x006448B9`
    /// with no zero check.
    pub defender_splash_divisor: i32,
    /// `defender[+0x68]`, an object flag word. Bits used: 0x1, 0x80000, 0x400000,
    /// 0x2000000.
    pub defender_flags_0x68: u32,
    /// `defender[+0x6C] & 0x1000`.
    pub defender_flags_0x6c_bit12: bool,
    /// `(defender[+0xC] ^ 0x63637)`, the XOR-masked height. Unmask before passing.
    pub defender_z: i32,
    /// `defender[+0x50]`, differenced against `attack_dir` for the flank test.
    pub defender_facing: i32,
    /// `defender[+0x5C]`, differenced against `attack_dir` for the entrenchment test.
    pub defender_facing_entrench: i32,
    /// `defender[+0x4C]`, the frame the last overkill window opened. Zero disables.
    pub defender_overkill_stamp: i32,
    /// `(i32)(i16) defender[+0xA4]`, compared against `attacker->vtbl[0xE4]()`.
    pub defender_word_0xa4: i32,
    /// `attacker->vtbl[0xE4]()` at `0x00644C0D`.
    pub attacker_vf_0xe4: i32,

    // ---- world ----
    /// `[[0x00C061E8] + 0x550]`, the current frame.
    pub current_frame: i32,
    /// `[[0x00C061E8] + 0x821] & 2`.
    pub game_flag_0x821_bit1: bool,
    /// Tile record byte 0 bit 3 at `0x00644CD7` — rocky terrain under the defender.
    pub tile_rocky: bool,
    /// Tile record byte `+0xF` at `0x00644509` — the tile owner's player id.
    pub tile_owner: i32,
}

/// The guards the chain evaluates by walking the object graph or calling a virtual.
///
/// These are **inputs, not derivations**. Each is named for the retail test that produces
/// it and carries the VA of the call or compare. Filling them in correctly is a separate
/// (unsolved) problem: it needs the `Object` vtable semantics and the world state, which
/// this crate does not model yet. Naming them after game concepts we have not derived
/// would be folklore, so they are named after their slots.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DamagePredicates {
    /// `0x00644243..0x006442A1` — the whole aux-object chain that authorises the
    /// attacker-mask fixup. Only consulted when its outer gate at `0x00644218` opens.
    pub mask_fixup_authorised: bool,

    /// `attacker->vtbl[0x18]()` — "alive"-shaped; `0x00644591`, `0x00644BB4`, `0x00644AA4`.
    pub attacker_vf_0x18: bool,
    /// `attacker->vtbl[0x1C]()` — `0x00644672`, damage-kind only.
    pub attacker_vf_0x1c: bool,
    /// `attacker->vtbl[0x20]()` — `0x0064435F`, `0x00644424`, `0x00644803`.
    pub attacker_vf_0x20: bool,
    /// `attacker->vtbl[0x130]()` — `0x00644BA2`, gates overkill.
    pub attacker_vf_0x130: bool,
    /// `attacker UnitType->vtbl[0x10C]()` — `0x00644D50` (height), `0x00644C72` (overkill).
    pub attacker_type_vf_0x10c: bool,
    /// The attacker re-fetched from the object table; its `vtbl[0x20]()` at `0x00644307`.
    /// Distinct from [`Self::attacker_vf_0x20`] only because retail reads it through a
    /// different path — in a consistent world they agree.
    pub attacker_table_vf_0x20: bool,

    /// `defender->vtbl[0x18]()` — the most-tested guard in the function, twelve sites.
    pub defender_vf_0x18: bool,
    /// `defender->vtbl[0x1C]()` — `0x00644A22`.
    pub defender_vf_0x1c: bool,
    /// `defender->vtbl[0x20]()` — `0x00644439`, `0x0064447B`, `0x00644553`, `0x00644F9F`.
    pub defender_vf_0x20: bool,
    /// `defender->vtbl[0x120]()` — `0x00644391`.
    pub defender_vf_0x120: bool,
    /// `defender->vtbl[0xCC]()` — `0x006443F5`.
    pub defender_vf_0xcc: bool,
    /// `defender->vtbl[0xD0]()` — `0x0064440D`.
    pub defender_vf_0xd0: bool,
    /// `defender->vtbl[0xD8]()` — `0x006443AD`.
    pub defender_vf_0xd8: bool,
    /// `defender UnitType->vtbl[0x10C]()` — `0x006443DD`, `0x00644923`.
    pub defender_type_vf_0x10c: bool,

    /// `defender->vtbl[0x3C]()` then the `Build::vftable` (`0x00B42174`) test at
    /// `0x00644453`/`0x0064456D`: true when the resulting flag is **non-zero**.
    /// Steps 5 and 7 fire when it is *zero*; step 30 requires it non-zero.
    pub defender_build_flag: bool,
    /// `(defender->vtbl[0x3C]())[+8] & 0x20` — `0x00644FE5`.
    pub defender_build_0x20: bool,
    /// The city-owner comparison that closes step 30 — `0x00645018`.
    pub recapture_owner_matches: bool,
    /// `(defender->vtbl[0x40]())->vtbl[0x184]()` — `0x00644A44`.
    pub defender_carrier_vf_0x184: bool,
    /// `(defender->vtbl[0x40]())->vtbl[0x20]()` — `0x00644A74`.
    pub defender_carrier_vf_0x20: bool,
    /// `(defender->vtbl[0x3C]())->type[+0x1E8] == 0` — `0x00644A8F`.
    pub defender_mount_attack_is_zero: bool,

    /// Tech `0x42` on the attacker's type — `0x006444B9`.
    pub attacker_tech_0x42: bool,
    /// Tech `0x139` on the attacker's type — `0x0064453E`.
    pub attacker_tech_0x139: bool,
    /// Tech `0x83` on the attacker's type — `0x00644DE8`, suppresses entrenchment.
    pub attacker_tech_0x83: bool,
    /// Tech `0x216` on the defender's type — `0x006445DB`, the Red Fort gate.
    pub defender_tech_0x216: bool,
    /// Tech `0x143` on the defender's type — `0x00644979`.
    pub defender_tech_0x143: bool,
    /// Tech `0x109` on the defender's type — `0x00644C66`.
    pub defender_tech_0x109: bool,

    /// **UNVERIFIED (step 10, `0x006446BE`).** The whole `FUN_00646B00` chain.
    pub step10_bonus_applies: bool,
    /// **UNVERIFIED (step 11, `0x00644743`).** `FUN_006E1370(0xF)` on the attacker's
    /// player.
    pub step11_player_prop_0xf: bool,
    /// **UNVERIFIED (step 27, `0x00644E7E`).** `FUN_006E1370(0xD)` on the attacker's
    /// player.
    pub step27_player_prop_0xd: bool,
    /// **UNVERIFIED (step 12, `0x00644879`).** `FUN_006DA000(def_player) != attacker
    /// player`. Only consulted on the non-`0x400000` route.
    pub step12_team_differs: bool,
    /// `[[0x00C061E8] + 0x24] == 2` — `0x00644866`.
    pub game_mode_is_2: bool,
}

/// Additive terms for the two steps the oracle harness could not reach.
///
/// **UNVERIFIED.** Steps 10 and 11 read `RULES[+0xBBC]` and `RULES[+0x794]`, and step 11's
/// multiplier also needs the attacker's player military/age levels (`player[+0xDC] ^
/// 0x62766` and `player[+0xE8] ^ 0x63187`). None of it has ever been executed against
/// retail, so rather than smuggle it into [`CombatRules`] as if it were on the same
/// footing, it lives here and defaults to inert.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UnreachedTerms {
    /// `RULES[+0xBBC]`, added at `0x0064473D` (step 10).
    pub rule_0xbbc: i32,
    /// `RULES[+0x794]`, the step-11 percent at `0x006447B4`. Negative means
    /// "per level of `min(military, age)`".
    pub rule_0x794: i32,
    /// `min(player[+0xDC] ^ 0x62766, player[+0xE8] ^ 0x63187)` for step 11.
    pub step11_player_level: i32,
}

/// Flank tier from a wrapped angle delta — `0x0092CFE0`, eleven instructions, ISLAND.
///
/// The retail body is `cmp ecx, 0xD5555555; jbe .. ; xor eax,eax; ret` then
/// `lea eax,[ecx-0x60000000]; mov ecx,0x40000000; cmp ecx,eax; sbb eax,eax; neg eax;
/// inc eax; ret`.
///
/// It clobbers `EAX` and `ECX` and **preserves `EDX`** — load-bearing, because the caller
/// at `0x00644B3A` tests the defender's mask bits out of `EDX` immediately after the
/// call.
///
/// **Tier B** — 500,017 inputs (17 boundary values plus a 500,000-point stride-8191 sweep
/// of the full `u32` domain), 0 mismatches. Testing, not proof.
#[inline]
pub fn flank_level(delta: u32) -> u32 {
    if delta > 0xD555_5555 {
        0
    } else if 0x4000_0000u32 < delta.wrapping_sub(0x6000_0000) {
        2
    } else {
        1
    }
}

/// The 0/1/2 direction tier used by the entrenchment step — inlined at `0x00644E0E`.
///
/// A *different* classifier from [`flank_level`], despite the shared 0/1/2 shape and the
/// shared `0x40000000` split: the reject test is `(x - 0x2AAAAAAA) > 0xAAAAAAAB` rather
/// than `x > 0xD5555555`. They disagree on, for instance, `x = 0`. Worth stating plainly
/// because assuming one function serves both would be an easy and invisible error.
#[inline]
pub fn entrench_dir_level(delta: u32) -> u32 {
    if delta.wrapping_sub(0x2AAA_AAAA) > 0xAAAA_AAAB {
        0
    } else if 0x4000_0000u32 < delta.wrapping_sub(0x6000_0000) {
        2
    } else {
        1
    }
}

/// Flat index into the runtime balance table — `0x00581CA0`, inlined at
/// `0x00644178..0x0064418E`.
///
/// `493 = 364 unit types + 129 building types`, the combined type space; the **attacker
/// is the row**. The lookup itself is `(i32)(i16) *(i16*)(0x00C06AFC + 2*index)`.
///
/// **Tier B for the address arithmetic only** — exhaustive over the 493×493 domain plus
/// 4,000 out-of-domain rows, 0 mismatches. The table *contents* are loaded from
/// `balance.xml` at runtime and were not tested; `docs/derivation/combat.md` §5 records an
/// unresolved question about the table's storage extent.
#[inline]
pub fn balance_index(attacker_type_id: i32, defender_type_id: i32) -> i32 {
    attacker_type_id
        .wrapping_mul(493)
        .wrapping_add(defender_type_id)
}

/// `Object::get_attack` — `0x006469F0`, vtable slot `+0x120`.
///
/// Returns the ×10 attack. The upgrade term is
/// `10 * ((player[+0xDC] ^ 0x62766) * RULES[+0x8B8])` at `0x00646AE2`
/// (`lea eax,[ecx+ecx*4]; lea eax,[edi+eax*2]`).
///
/// **The base path is Tier B** (it is what the damage harness drives, 0 mismatches);
/// **the upgrade path is UNVERIFIED** — reaching it needs `FUN_006E1370(0x16)` to return
/// true, which the harness deliberately disables, plus four more virtual predicates.
/// `upgrade_applies` is therefore an input, not a derivation.
#[inline]
pub fn get_attack(
    type_attack: i32,
    upgrade_applies: bool,
    military_level: i32,
    rules_0x8b8: i32,
) -> i32 {
    if upgrade_applies {
        type_attack.wrapping_add(military_level.wrapping_mul(rules_0x8b8).wrapping_mul(10))
    } else {
        type_attack
    }
}

/// `Object::get_armor` — `0x00647DB0`, vtable slot `+0x124`.
///
/// Same shape as [`get_attack`] but the upgrade term is `1 ×`, not `10 ×`
/// (`0x00647E6F: lea eax,[ecx+esi]`) — armor lives on the display scale, attack on the
/// ×10 scale. Same fidelity split: base path Tier B, upgrade path UNVERIFIED.
#[inline]
pub fn get_armor(
    type_armor: i32,
    upgrade_applies: bool,
    military_level: i32,
    rules_0x8b8: i32,
) -> i32 {
    if upgrade_applies {
        type_armor.wrapping_add(military_level.wrapping_mul(rules_0x8b8))
    } else {
        type_armor
    }
}

/// The full pipeline — `FUN_00644130`.
///
/// Step numbers match the table in `docs/derivation/combat.md` §3 and the VA on each line
/// is the instruction that performs the arithmetic. Reading this next to a disassembly of
/// `0x00644130..0x00645060` should be a line-for-line exercise; that was the point.
///
/// # Panics
///
/// Where retail raises `#DE`: `defender_splash_divisor == 0` on the splash path
/// (`0x006448B9`), or `height_increment * 100 == 0` on the height path (`0x00644D78`).
pub fn damage(i: &DamageInput, p: &DamagePredicates, r: &CombatRules, u: &UnreachedTerms) -> i32 {
    damage_traced(i, p, r, u).0
}

/// Names of the trace bits [`damage_traced`] returns, indexed by bit position.
///
/// Numbers follow the step table in `docs/derivation/combat.md` §3, so bit 0 is "step 1
/// ran" and so on. The trace exists because a differential test that never takes a branch
/// has not tested it, and "0 mismatches" over a corpus that only ever ran the spine would
/// be a green suite that secretly tests nothing.
pub const STEP_NAMES: [&str; 30] = [
    "0-maskfix",
    "2-armor133",
    "3-div3",
    "4a-mul3div4",
    "4b-div2",
    "5-mul4",
    "6-div2",
    "7-div2",
    "8-redfort",
    "9a-mul2",
    "9b-armor+1",
    "10-add",
    "11-pct",
    "12-mul2",
    "13-idiv",
    "14-splashpct",
    "15-mul3",
    "16-mul25",
    "17-river",
    "18-x1000",
    "19-mul4",
    "20-flank",
    "23-overkill",
    "23b-div2",
    "24-rocky",
    "25-height",
    "26-entrench",
    "28-floor1",
    "29-zero",
    "30-recapture",
];

/// [`damage`] plus a bitmask of which guarded steps actually executed.
///
/// Bit `n` corresponds to `STEP_NAMES[n]`. The bits are set at the site that performs the
/// arithmetic, in the same branch, so a step cannot be reported as covered without having
/// run. Steps 21, 22 and 31 are unconditional and have no bit; 28, 29 and 30 are reported
/// through the return value's shape rather than a bit.
// The nesting below mirrors the retail branch structure one-for-one: `0x00644218` gates
// `0x00644243`, and `0x0064480B` gates `0x0064483F`. Collapsing those `if`s would read
// better and correspond worse, and correspondence is the whole point of this file.
#[allow(clippy::collapsible_if)]
#[allow(clippy::manual_range_patterns)]
pub fn damage_traced(
    i: &DamageInput,
    p: &DamagePredicates,
    r: &CombatRules,
    u: &UnreachedTerms,
) -> (i32, u64) {
    let mut t: u64 = 0;
    // ---- operands, 0x0064418E..0x006441DF -------------------------------------------
    let b = i.balance_pct;
    let mut arm = i.armor;
    let mut am = i.attacker_masks;
    let dm = i.defender_masks;

    // Attacker-mask fixup, 0x00644204..0x006442AE. Ghidra reports only the AND; the
    // machine code at 0x006442AB also ORs in bit 6.
    if i.attacker_type_id == 0x1BC || i.attacker_type_id == 0x1BB || i.attacker_flag8_bit5 {
        if p.mask_fixup_authorised {
            am = (am & 0xFFFD_FFFF) | 0x40;
            t |= 1 << 0;
        }
    }

    // 1 — 0x006442B1. The product wraps before the divide; do not promote to i64.
    let mut d = div100(i.attack.wrapping_mul(b));

    // 2 — 0x006442C9. Armor, not damage: 0x85 = 133.
    if am & 0x8 != 0 {
        arm = div100(arm.wrapping_mul(133));
        t |= 1 << 1;
    }

    // 3 — 0x00644307
    if p.attacker_table_vf_0x20
        && dm & 0x0004_0000 != 0
        && p.defender_vf_0x18
        && i.defender_type_0x2b8_bit2
        && i.defender_flags_0x68 & 0x0008_0000 == 0
    {
        d /= 3; // 0x00644352
        t |= 1 << 2;
    }

    // 4 — 0x0064435B
    if p.attacker_vf_0x20 && p.defender_vf_0x18 && p.defender_vf_0x120 && p.defender_vf_0xd8 {
        if dm & 0x20 != 0 {
            d = d.wrapping_mul(3) / 4; // 4a — 0x006443BD
            t |= 1 << 3;
        } else if p.defender_type_vf_0x10c || p.defender_vf_0xcc || p.defender_vf_0xd0 {
            d /= 2; // 4b — 0x0064441E
            t |= 1 << 4;
        }
    }

    // 5 — 0x00644420
    if p.attacker_vf_0x20 && p.defender_vf_0x20 && !p.defender_build_flag {
        d = d.wrapping_mul(4); // 0x0064446A, shl edi,2
        t |= 1 << 5;
    }

    // 6 — 0x0064446D
    if p.defender_vf_0x20 {
        let id_hit = matches!(i.attacker_type_id, 0x32 | 0x33 | 0x34 | 0x35);
        if (id_hit || p.attacker_tech_0x42) && i.tile_owner != i.attacker_player as i32 {
            d /= 2; // 0x0064451D
            t |= 1 << 6;
        }
    }

    // 7 — 0x0064451F
    if p.attacker_tech_0x139 && p.defender_vf_0x20 && !p.defender_build_flag {
        d /= 2; // 0x0064458B
        t |= 1 << 7;
    }

    // 8 — 0x0064458D. Note the mask re-read at 0x006445A4 is the *unmutated* type field,
    // which is why this tests `i.attacker_masks` and not the local `am`.
    if p.attacker_vf_0x18
        && i.attacker_domain == 2
        && i.attacker_masks & 0x0800_0000 == 0
        && p.defender_tech_0x216
    {
        d = div100(d.wrapping_mul(100i32.wrapping_sub(r.red_fort_air_defense))); // 0x006445F7
        t |= 1 << 8;
    }

    // 9 — 0x00644606
    if p.defender_vf_0x18 {
        let c = i.defender_flags_0x68 & 0x0008_0000 != 0;
        if c {
            d = d.wrapping_add(d); // 0x00644632
            t |= 1 << 9;
        }
        if i.defender_type_0x2b8_bit2 && !c {
            arm = arm.wrapping_add(1); // 0x00644644
            t |= 1 << 10;
        }
    }

    // 10 — 0x006446BE. UNVERIFIED: never executed against retail.
    if p.step10_bonus_applies {
        d = d.wrapping_add(u.rule_0xbbc); // 0x0064473D
        t |= 1 << 11;
    }

    // 11 — 0x00644743. UNVERIFIED: never executed against retail.
    if p.step11_player_prop_0xf && i.attacker_type_0x40 == 0x1AB {
        let mut m = u.rule_0x794;
        if m < 0 {
            m = m.wrapping_mul(u.step11_player_level).wrapping_neg(); // 0x006447E3
        }
        d = div100(m.wrapping_add(100).wrapping_mul(d)); // 0x006447F0
        t |= 1 << 12;
    }

    // 12 — 0x006447FF
    if p.attacker_vf_0x20 || (i.attacker_domain == 0 && i.defender_domain != 0) {
        if p.defender_vf_0x18 {
            let by_flag = i.defender_flags_0x68 & 0x0040_0000 != 0;
            let by_team = i.defender_domain == 1 && p.game_mode_is_2 && p.step12_team_differs;
            if by_flag || by_team {
                d = d.wrapping_add(d); // 0x00644889
                t |= 1 << 13;
            }
        }
    }

    // 13/14/15/16 — the splash block, 0x0064488B
    if i.splash_flag != 0 {
        if p.defender_vf_0x18 {
            // 13 — 0x006448B9, a real idiv with no zero check.
            d = idiv_trapping(d, i.defender_splash_divisor, "0x006448B9");
            t |= 1 << 14;
        }
        // 14 — 0x006448EA
        d = div100(i.attacker_splash_percent.wrapping_mul(d));
        t |= 1 << 15;
        if p.defender_vf_0x18 {
            // 15 — 0x0064494E
            if p.defender_type_vf_0x10c
                && i.defender_type_0x2b8_bit2
                && i.defender_flags_0x68 & 0x0008_0000 == 0
            {
                d = d.wrapping_mul(3);
                t |= 1 << 16;
            }
            // 16 — 0x00644980
            if p.defender_tech_0x143 {
                d = div100(d.wrapping_mul(25));
                t |= 1 << 17;
            }
        }
    }

    // 17 — 0x00644994. The guard is the sign of the *unmasked* height word.
    if p.defender_vf_0x18 && i.defender_z < 0 {
        d = div256(r.river_modifier.wrapping_mul(d)); // 0x006449C7
        t |= 1 << 18;
    }

    // 18 — 0x006449D7. Armor is zeroed and the result is forced to the ×1000 scale.
    if p.defender_vf_0x18 && i.defender_flags_0x68 & 1 != 0 {
        arm = 0; // 0x006449FD
                 // 0x00644A04 calls 0x006469F0 directly, not through the vtable.
        d = d.max(i.attack).wrapping_mul(1000); // 0x00644A11
        t |= 1 << 19;
    }

    // 19 — 0x00644A17
    if p.defender_vf_0x1c && p.defender_carrier_vf_0x184 && !i.game_flag_0x821_bit1 {
        arm = 0; // 0x00644A61
        if !p.defender_carrier_vf_0x20 || p.defender_mount_attack_is_zero {
            d = d.wrapping_mul(4); // 0x00644A98
            t |= 1 << 20;
        }
    }

    // 20 — flank, 0x00644A9B..0x00644B7B
    if p.attacker_vf_0x18
        && p.defender_vf_0x18
        && am & 4 == 0
        && dm & 4 == 0
        && am & 0x1000_0000 == 0
        && dm & 0x1000_0000 == 0
        && am & 0x2000 == dm & 0x2000
        && dm & 0x2000 == 0
    {
        // 0x00644B12: subtract, then bias by 0x80000000 (which is also its own negation).
        let delta = (i.defender_facing as u32)
            .wrapping_sub(i.attack_dir as u32)
            .wrapping_sub(0x8000_0000);
        if delta >= 0x2AAA_AAAA {
            let lvl = flank_level(delta) as i32;
            if lvl != 0 {
                let mut pct = r.flank_bonus;
                if dm & 0x0020_0000 != 0 {
                    pct = div256(r.vehicle_flank_bonus.wrapping_mul(pct)); // 0x00644B42
                } else if dm & 0x1000 != 0 {
                    pct = div256(r.cavalry_flank_bonus.wrapping_mul(pct)); // 0x00644B4F
                }
                // 0x00644B62: pct*lvl, +100, *D, /100 — all 32-bit wrapping.
                d = div100(pct.wrapping_mul(lvl).wrapping_add(100).wrapping_mul(d));
                t |= 1 << 21;
            }
        }
    }

    // 21 — 0x00644B7D. The one rescale off the ×10 attack scale; round-half-up for D >= 0.
    d = d.wrapping_add(5) / 10;

    // 22 — 0x00644B91. Armor lands here, not at the end.
    d = d.wrapping_sub(arm);

    // 23 — overkill, 0x00644B94
    if i.overkill_gate != 0 && p.attacker_vf_0x130 && p.attacker_vf_0x18 && p.defender_vf_0x18 {
        let stamp = i.defender_overkill_stamp;
        if stamp != 0
            && i.current_frame.wrapping_sub(stamp) < r.overkill_frames
            && i.attacker_vf_0xe4 != i.defender_word_0xa4
        {
            d = div256(r.overkill_damage.wrapping_mul(d)); // 0x00644C35
            t |= 1 << 22;
            if p.defender_tech_0x109 && !p.attacker_type_vf_0x10c {
                d /= 2; // 23b — 0x00644C83
                t |= 1 << 23;
            }
        }
    }

    // 24 — rocky, 0x00644C85
    if dm & 0x0001_0108 != 0 && i.tile_rocky {
        d = div256(r.rocky_modifier.wrapping_mul(d)); // 0x00644CE5
        t |= 1 << 24;
    }

    // 25 — height, 0x00644CF5
    if i.defender_domain != 2
        && i.attacker_domain != 2
        && !p.attacker_type_vf_0x10c
        && i.attacker_z > i.defender_z
    {
        let dz = i.attacker_z.wrapping_sub(i.defender_z); // 0x00644D64
        let num = dz.wrapping_mul(r.height_bonus).wrapping_mul(d); // 0x00644D6C, 0x00644D74
        let den = r.height_increment.wrapping_mul(100); // 0x00644D70
        d = d.wrapping_add(idiv_trapping(num, den, "0x00644D78"));
        t |= 1 << 25;
    }

    // 26 — entrenchment, 0x00644D7F
    if p.defender_vf_0x18 && i.defender_flags_0x68 & 0x0200_0000 != 0 && !p.attacker_tech_0x83 {
        let raw = (i.defender_facing_entrench as u32)
            .wrapping_sub(i.attack_dir as u32)
            .wrapping_sub(0x8000_0000);
        let dir = entrench_dir_level(raw);
        if i.splash_flag != 0 || dir == 0 {
            d = div256(r.entrenchment_modifier.wrapping_mul(d)); // 0x00644E44
            t |= 1 << 26;
            if i.defender_flags_0x6c_bit12 {
                d = div256(r.rule_0xb98.wrapping_mul(d)); // 26b — 0x00644E66
            }
        }
    }

    // 27 — 0x00644E7E. UNVERIFIED: never executed against retail.
    if r.rule_0x76c != 0
        && p.step27_player_prop_0xd
        && p.attacker_vf_0x18
        && i.attacker_type_0x40 == 0x1AC
        && (p.defender_vf_0xcc || p.defender_type_vf_0x10c)
    {
        d = div100(r.rule_0x76c.wrapping_add(100).wrapping_mul(d)); // 0x00644F04
    }

    // 28 — the conditional floor, 0x00644F13. Three tests, not one.
    if d < 1 {
        let masks_agree = (dm & 0x1000_0000) == ((am >> 3) & 0x1000_0000);
        let land_vs_sea = i.attacker_domain == 0 && i.defender_domain == 1;
        if !land_vs_sea && masks_agree && i.splash_flag == 0 {
            d = 1; // 0x00644F61
            t |= 1 << 27;
        }
    }

    // 29 — 0x00644F69
    if i.defender_type_id == 0x21D && i.attacker_domain == 2 && r.rule_0x558 != 0 {
        d = 0; // 0x00644F9A
        t |= 1 << 28;
    }

    // 30 — city recapture, 0x00644F9D
    if p.defender_vf_0x20
        && p.defender_build_flag
        && p.defender_build_0x20
        && p.recapture_owner_matches
    {
        t |= 1 << 29;
        return (div256(r.recapture_city_modifier.wrapping_mul(d)), t); // 0x00645024
    }

    // 31 — 0x0064503C
    (d, t)
}

// ============================================================================
// The economy — resource tick, commerce caps, the build-rate ramp, attrition timing.
// ============================================================================
//
// # Provenance
//
// Ports of four retail routines, each cited on the function that carries it:
//
// * `Player::TickResource` `0x006CE450` — the per-frame income pipeline and the
//   per-resource accumulator.
// * `Player::UpdateCommerceCaps` `0x006CE900` — the age-indexed cap.
// * the build-rate ramp at `0x00650AB5` inside `FUN_006508C0`.
// * the attrition interval, `FUN_00608FD0` + its consumer sites `0x005E193D`,
//   `0x006117C8` and `0x006115EA`.
//
// The structural walk-through is `docs/derivation/economy.md`; this lane's disassembly
// pass, its corrections to that document, and the list of what stayed unimplemented are
// `docs/derivation/sim-economy.md`.
//
// # Fidelity
//
// **Tier C at best, and mostly "transcribed from the disassembly and never executed".**
// Unlike the damage chain, *none* of this has been run against retail: there is no oracle
// harness for `0x006CE450` (it walks the player array, the game object and six obfuscated
// econ slots). What is [measured] is the instruction sequence — every line below was read
// out of capstone output at the address in its comment, not out of Ghidra, and the four
// places where Ghidra's C and the machine code disagree about control flow are called out.
// What is **not** established is anything about the values that flow in.
//
// Nothing here is Tier A or Tier B. Do not promote it by citing the tokenizer's Tier B.
//
// # What is deliberately missing
//
// The composition of `income` from worker counts, `CITY_GATHER`, `BASIC_GATHER`,
// `SCHOLAR_RATE` and the building-bonus tables is **not derived**, so it is not here — the
// pipeline takes the already-composed gross and expense as inputs. Likewise the
// SUPPORT/PROGRESSION cost curve: `FUN_00664090` is 12 KB and was not reduced, so only its
// ramp ceiling appears. See `docs/derivation/sim-economy.md` §"Not implemented".

/// `x / 200`, truncating. Idiom at `0x00650B7D`: the same `0x51EB851F` multiply as
/// [`div100`] but with `sar edx, 6` instead of `5`.
#[inline]
fn div200(x: i32) -> i32 {
    x / 200
}

/// `cvttss2si` — SSE truncate-toward-zero float→int, as the attrition path uses it at
/// `0x0060914E`.
///
/// Rust's `as i32` **saturates** on out-of-range and maps NaN to 0; `cvttss2si` returns the
/// "integer indefinite" value `0x80000000` for NaN, infinities and anything that does not
/// fit. Faithful here matters: the multiplier at `player + 0x7F4` is engine-written float
/// we do not control.
#[inline]
fn cvttss2si(x: f32) -> i32 {
    if x.is_nan() || x >= 2_147_483_648.0 || x < -2_147_483_648.0 {
        i32::MIN
    } else {
        x as i32
    }
}

/// The rules.xml constants the economy reads, with the offset each is read at.
///
/// Names and offsets are from `docs/derivation/rules-constants.json`; shipped values are in
/// the doc comments so a divergence is visible without opening the JSON. Build one with
/// [`EconomyRules::shipped`] or fill it from a live `RULES` capture — do **not** hand-write
/// numbers into it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EconomyRules {
    /// `+0x27C` `GATHER_RATE`, frames. Shipped `"450 frames"` → 450. Read at `0x006CE7B9`.
    pub gather_rate: i32,
    /// `+0x8A8` `DUTCH_INTEREST`, percent. Shipped 5. Read at `0x006CE6C2`.
    pub dutch_interest: i32,
    /// `+0x8AC` `DUTCH_INTEREST_CAP`. Shipped 50. Read at `0x006CE6F6`, shifted `<< 4`.
    pub dutch_interest_cap: i32,
    /// `+0x400` `COMMERCE_CAP[8]`, indexed by **age**. Shipped 70/100/150/200/260/320/400/500.
    /// Read at `0x006CE940`.
    pub commerce_cap: [i32; 8],
    /// `+0x654` `EGYPTIAN_FOOD_COMMERCE`, percent. Shipped 10. `0x006CEA5C`, food only.
    pub egyptian_food_commerce: i32,
    /// `+0x6CC` `FRENCH_TIMBER_COMMERCE`, percent. Shipped 10. `0x006CEA04`, timber only.
    pub french_timber_commerce: i32,
    /// `+0x598` `INCA_WEALTH_CAP`, percent. Shipped 33. `0x006CE9A9`, wealth only.
    pub inca_wealth_cap: i32,
    /// `+0x6D0` `BRITISH_COMMERCE`, percent. Shipped 25. `0x006CE971`, every resource.
    pub british_commerce: i32,
    /// `+0x220` `UNIT_RATE_BASE`, percent (`AsScaled(100)`). Shipped `"6/5"` → 120.
    /// Read at `0x00650AF6`.
    pub unit_rate_base: i32,
    /// `+0x224` `UNIT_RATE_PROGRESSION`, percent (`AsScaled(100)`). Shipped `"3/4"` → 75.
    /// Read at `0x00650B31`.
    pub unit_rate_progression: i32,
    /// `+0x1E8` `SIEGE_ATTRITION`, percent reduction. Shipped 50. `0x00609021`.
    pub siege_attrition: i32,
    /// `+0x1EC` `MILITIA_ATTRITION`, percent increase. Shipped 300. `0x0060906E`.
    pub militia_attrition: i32,
    /// `+0x1F0` `ATTRITION_AGED_UP`, percent per age of advantage. Shipped 25. `0x006090F3`.
    pub attrition_aged_up: i32,
    /// `+0xD3C` `ATTRITION`, **frames**. Shipped `"48 frames"` → 48. Master switch at
    /// `0x00608FFE`; the interval numerator at `0x005E1942`.
    pub attrition: i32,
    /// `+0xD38` `ASSASSIN_ATTRITION`, frames. Shipped 8. `0x005E15DD`.
    pub assassin_attrition: i32,
    /// `+0xD34` `PEACE_ATTRITION`, frames. Shipped 8. Border-violation attrition,
    /// `0x00822D6D`.
    pub peace_attrition: i32,
}

impl EconomyRules {
    /// The shipped values, from `docs/derivation/rules-constants.json` — the same table
    /// `don-rules` generates its typed accessors from, 828 of whose 834 constants were read
    /// back out of a running match. Captured, not calculated.
    pub const fn shipped() -> EconomyRules {
        EconomyRules {
            gather_rate: 450,
            dutch_interest: 5,
            dutch_interest_cap: 50,
            commerce_cap: [70, 100, 150, 200, 260, 320, 400, 500],
            egyptian_food_commerce: 10,
            french_timber_commerce: 10,
            inca_wealth_cap: 33,
            british_commerce: 25,
            unit_rate_base: 120,
            unit_rate_progression: 75,
            siege_attrition: 50,
            militia_attrition: 300,
            attrition_aged_up: 25,
            attrition: 48,
            assassin_attrition: 8,
            peace_attrition: 8,
        }
    }
}

/// Resource slot indices, as the engine's `for res in 0..6` loop numbers them.
///
/// Only two are pinned by code: **3 is knowledge** (it is the slot with the hardcoded 999
/// cap at `0x006CE92C`, the difficulty penalty at `0x006CE755` and no interest term) and
/// **2 is wealth** (`INCA_WEALTH_CAP` applies to it at `0x006CE9A9`). 1 is timber
/// (`FRENCH_TIMBER_COMMERCE`) and 0 is food (`EGYPTIAN_FOOD_COMMERCE`), from the same
/// per-resource branches. Slots 4 and 5 have no per-resource branch and are **not named
/// here** — the wiki's ordering is not evidence.
pub const RES_FOOD: usize = 0;
/// See [`RES_FOOD`].
pub const RES_TIMBER: usize = 1;
/// See [`RES_FOOD`].
pub const RES_WEALTH: usize = 2;
/// See [`RES_FOOD`].
pub const RES_KNOWLEDGE: usize = 3;

/// The accumulator period, in the same units as `income`: `GATHER_RATE * 16`.
///
/// `0x006CE7B9`: `mov ecx,[rules+0x27C]; shl ecx,4`. With the shipped 450 that is **7200**,
/// and since [`resource_tick`] runs once per frame (below), an income of `I` credits
/// `I / 7200` whole resources per frame — i.e. `I` is "sixteenths of a resource per
/// `GATHER_RATE` frames", and `GATHER_RATE` frames is 30 s at the engine's 15 fps.
///
/// The shift is a 32-bit `shl`, so it wraps rather than saturating.
#[inline]
pub fn resource_period(gather_rate: i32) -> i32 {
    gather_rate.wrapping_shl(4)
}

/// Everything `Player::TickResource` reads for one resource that is not a rules constant.
///
/// The XOR masks are the engine's anti-tamper obfuscation; pass the **unmasked** values.
/// Each field names the slot it comes from so a live capture can be wired up mechanically.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ResourceTickInput {
    /// Resource slot, 0..6. Drives three branches; see [`RES_KNOWLEDGE`].
    pub res: usize,
    /// `econ[0x64 + res*4] ^ 0x872`, the gross income. **Its producers are not derived** —
    /// worker counts, `CITY_GATHER`, the building bonus tables. `0x006CE4C8`.
    pub gross: i32,
    /// `econ[0x7C + res*4] ^ 0x26076`, the expense term. `0x006CE4CC`.
    pub expense: i32,
    /// `player[0x4B0 + res*4]`, an additive per-resource bonus. `0x006CE4DD`.
    pub bonus: i32,
    /// `econ[0x30 + res*4] ^ 0x1281`, this resource's commerce cap — see [`commerce_cap`].
    /// `0x006CE509`.
    pub commerce_cap: i32,
    /// The player's current stockpile, `[0x00E41248 + player*0x6EEC][res] ^ 0x8221`. Only
    /// read on the interest path. `0x006CE6B4`.
    pub stockpile: i32,
    /// The interest threshold the stockpile is measured against, built at
    /// `0x006CE666`/`0x006CE684`/`0x006CE69A` from a game-config table and, on one branch,
    /// `RULES[+0xA60]`. Composition **not derived**; supplied whole.
    pub interest_threshold: i32,
    /// `res != 3 && FUN_006E1370(player, 0x16)` — the gate at `0x006CE62F`/`0x006CE643`.
    /// Also gates the 16,000 clamp; see [`resource_tick`].
    pub interest_applies: bool,
    /// `FUN_006D66A0(player)`, the gather-rate bonus percent. `0x006CE72C`.
    pub gather_bonus_pct: i32,
    /// `*(0x00C061EC) + 0x2F`, the difficulty byte. Only read for knowledge. `0x006CE75A`.
    pub difficulty: u8,
    /// `game[0x20] & 2`. `0x006CE783`.
    pub game_flag_0x20_bit1: bool,
    /// `game[0x2A] == 9`. `0x006CE789`.
    pub game_flag_0x2a_is_9: bool,
    /// `**(0x00C061C0)`, the game speed. Only multiplies when `> 1`. `0x006CE79C`.
    pub game_speed: i32,
}

/// What one resource's tick produced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourceTick {
    /// The income the engine caches for the UI at `econ[0x94 + res*4] ^ 0x90236`.
    /// **Not** the value that reaches the accumulator: it is cached at `0x006CE723`, three
    /// steps before the last multiplier.
    pub displayed: i32,
    /// The income that reaches the accumulator, or `None` on the negative-income early-out
    /// at `0x006CE4E7`, where retail stores the displayed value and skips the resource
    /// entirely — no accumulation, no stockpile write.
    pub accumulated: Option<i32>,
}

/// `Player::TickResource`, one resource — `0x006CE450`, `__thiscall`, no args.
///
/// Runs **once per frame per player**: `FUN_00591EF0` — the frame update, which increments
/// the frame counter at `game + 0x550` (`0x005924BF`) and bumps a seconds counter every 15
/// of them (`0x005924CF`, `idiv 15`) — calls `FUN_006ED2A0` at `0x0059224F`, which loops
/// over players and calls `FUN_006CE280`, which calls `0x006CE900` then `0x006CE450`. No
/// modulo gates that chain — the periodicity lives entirely in the accumulator.
///
/// The order below is the retail order, which is **not** the order
/// `docs/derivation/economy.md` §3.2 gives. Three differences, each read off capstone
/// output rather than Ghidra:
///
/// 1. A negative income **returns early** (`jns` at `0x006CE4E7`). Nothing accumulates.
/// 2. The commerce cap clamps the income **before** the interest term (`0x006CE512`), not
///    only inside it.
/// 3. The 16,000 clamp is **not unconditional**. `0x006CE62F` and `0x006CE643` jump over it
///    to `0x006CE715`, so knowledge — and any player without property `0x16` — has no such
///    ceiling. Ghidra's C agrees; the ordering in the derivation document does not.
///
/// Also corrected: the interest clamp is `(DUTCH_INTEREST_CAP << 4) + commerce_cap`
/// (`0x006CE6F6`: only the rule constant is shifted), not `(cap + rule) << 4`.
///
/// # Panics
///
/// Never — every divide here is by a literal 100/200/2/4.
pub fn resource_tick(i: &ResourceTickInput, r: &EconomyRules) -> ResourceTick {
    // 0x006CE4C8..0x006CE4DD
    let mut income = i.gross.wrapping_sub(i.expense).wrapping_add(i.bonus);

    // 0x006CE4E7: `jns` — negative income is displayed and then abandoned.
    if income < 0 {
        return ResourceTick {
            displayed: income,
            accumulated: None,
        };
    }

    // 0x006CE512: clamp to the commerce cap. `jle` keeps the income when equal.
    if income > i.commerce_cap {
        income = i.commerce_cap; // 0x006CE61C
    }

    if i.interest_applies {
        // 0x006CE6BC: surplus over the threshold earns interest, in sixteenths.
        let surplus = i.stockpile.wrapping_sub(i.interest_threshold);
        if surplus > 0 {
            let interest = div100(r.dutch_interest.wrapping_mul(surplus)).wrapping_shl(4); // 0x006CE6C8..0x006CE6DC
            income = income.wrapping_add(interest); // 0x006CE6DF
            let limit = r
                .dutch_interest_cap
                .wrapping_shl(4)
                .wrapping_add(i.commerce_cap); // 0x006CE6FC
            if income > limit {
                income = limit; // 0x006CE703, cmovg
            }
        }
        // 0x006CE706: the global ceiling, reachable only on this path.
        if income > 0x3E70 {
            income = 0x3E70;
        }
    }

    // 0x006CE723: the UI cache is taken here, before the last three multipliers.
    let displayed = income;

    // 0x006CE72C: gather-rate bonus percent.
    if i.gather_bonus_pct != 0 {
        income = div100(i.gather_bonus_pct.wrapping_add(100).wrapping_mul(income));
        // 0x006CE73D
    }

    // 0x006CE755: knowledge only, and only above difficulty 4.
    if i.res == RES_KNOWLEDGE && i.difficulty > 1 && i.difficulty > 4 {
        if i.difficulty < 7 {
            income = income.wrapping_mul(3) / 4; // 0x006CE769, bias-then-shift = trunc
        } else {
            income /= 2; // 0x006CE777
        }
    }

    // 0x006CE783
    if i.game_flag_0x20_bit1 || i.game_flag_0x2a_is_9 {
        income = income.wrapping_mul(3) / 2; // 0x006CE78F
    }

    // 0x006CE7A3: only speeds above 1 multiply.
    if i.game_speed > 1 {
        income = income.wrapping_mul(i.game_speed); // 0x006CE7A8
    }

    ResourceTick {
        displayed,
        accumulated: Some(income),
    }
}

/// Credit a resource from this frame's income — `0x006CE7B9`..`0x006CE833`.
///
/// `acc` is the engine's per-resource accumulator at `econ[0x18 + res*4]` (stored
/// `^ 0x3421`; pass it unmasked). Returns the whole resources to add to the stockpile.
///
/// The carry loop is retail's, not a simplification: the division supplies
/// `income / period` and a remainder, the remainder is *added* to whatever the accumulator
/// already held, and then the loop drains it. Writing this as one division would be wrong
/// whenever the accumulator was already above the period.
///
/// # Panics
///
/// When `period == 0` — retail's `idiv` at `0x006CE7C5` raises `#DE`. `period` is
/// `GATHER_RATE * 16`, so this is a `GATHER_RATE` of 0 (or a multiple of 2^28).
pub fn credit_resource(income: i32, period: i32, acc: &mut i32) -> i32 {
    let mut whole = idiv_trapping(income, period, "0x006CE7C5"); // 0x006CE7C5
    let rem = income.wrapping_rem(period);
    *acc = acc.wrapping_add(rem); // 0x006CE7DE
    while *acc >= period {
        // 0x006CE810: subtract a whole period, credit one resource.
        *acc = acc.wrapping_sub(period);
        whole = whole.wrapping_add(1);
    }
    whole
}

/// The two civ-percent gates `UpdateCommerceCaps` evaluates. Both are
/// `FUN_006E1370(player, prop)` calls, i.e. player-property queries this crate cannot
/// resolve; they are inputs, like the damage chain's predicates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CommerceCapGates {
    /// `FUN_006E1370(player, 0xB)` at `0x006CE959`. Applies to every resource.
    pub british: bool,
    /// The per-resource query at `0x006CE9BD` (wealth, prop 2), `0x006CEA19` (timber,
    /// prop 0xA) or `0x006CEA6D` (food, prop 7). One bool because the branches are
    /// mutually exclusive.
    pub resource_civ: bool,
}

/// `Player::UpdateCommerceCaps`, one resource — `0x006CE900`, `__thiscall`.
///
/// Two hard facts, both [measured] off the instructions:
///
/// * The cap is **indexed by the player's age** (`econ[0xF0] ^ 0x63187`), reading
///   `RULES[0x400 + age*4]` at `0x006CE940`. It is not a per-resource table.
/// * **Knowledge's cap is hardcoded 999** and never touches `rules.xml`: `0x006CE92C`
///   stores the literal `0x1166`, and `0x1166 ^ 0x1281 = 0x3E7`. The `jmp` after it skips
///   every bonus, so no civ or wonder term can raise it.
///
/// `wonder_bonus` is the sum of the additive terms from `0x006CEAA6` onward (Pyramids,
/// Colossus, Taj, Eiffel, Kremlin, Tikal, Angkor, the Republic trio, Diamonds), each gated
/// by its own wonder/tech check. **Those gates are not derived**, so the sum is an input;
/// passing 0 models "no wonders".
///
/// # Panics
///
/// On an out-of-range `age`. Retail indexes `RULES[0x400 + age*4]` with no bound check and
/// would read the next field; refusing is better than silently reading `TERRITORY_BASE`.
pub fn commerce_cap(
    age: usize,
    res: usize,
    r: &EconomyRules,
    g: &CommerceCapGates,
    wonder_bonus: i32,
) -> i32 {
    if res == RES_KNOWLEDGE {
        return 999; // 0x006CE92C
    }
    assert!(
        age < 8,
        "COMMERCE_CAP has 8 entries; age {age} would read past it"
    );
    let mut cap = r.commerce_cap[age]; // 0x006CE940

    if g.british {
        cap = div100(r.british_commerce.wrapping_add(100).wrapping_mul(cap)); // 0x006CE982
    }

    // 0x006CE9A4 / 0x006CE9FF / 0x006CEA58 — one branch per resource, each skipped when
    // its rule is 0 (`test eax,eax; je`), so a zero rule never even queries the property.
    let civ_pct = match res {
        RES_WEALTH => r.inca_wealth_cap,
        RES_TIMBER => r.french_timber_commerce,
        RES_FOOD => r.egyptian_food_commerce,
        _ => 0,
    };
    if civ_pct != 0 && g.resource_civ {
        cap = div100(civ_pct.wrapping_add(100).wrapping_mul(cap)); // 0x006CE9DE
    }

    cap.wrapping_add(wonder_bonus)
}

/// The per-item rate ramp — `0x00650AB5`..`0x00650B4D`, inside `FUN_006508C0`.
///
/// ```text
/// base  = x * UNIT_RATE_BASE / 100                       ; 0x00650AF6, /100 magic
/// ramp  = count * JOB_EXTRA_TIME * UNIT_RATE_PROGRESSION  ; 0x00650B27, 0x00650B31
/// out   = clamp(base + ramp, 0, 3 * base)                 ; 0x00650B3D..0x00650B4D
/// ```
///
/// `count` is the player's live count of this type, a `u16` at
/// `players + 0x56FE + (player*0x3776 + type)*2` (`0x00650B17`) — note the **per-player
/// stride**, which `docs/derivation/economy.md` §4.2 omits. `job_extra_time` is
/// `UnitType + 0x2E8`, independently confirmed as `JOB_EXTRA_TIME` by
/// `schema/bindings.json`.
///
/// The clamp is exactly as written: if either `base + ramp` **or** `3 * base` is negative
/// the result is 0, so a wrapped product collapses rather than propagating.
///
/// **What `x` is has not been established.** It comes from `UnitType` vtable `+0x6C` or
/// `+0x70` (`0x00650907` / `0x00650AA8`) and this crate has no `UnitType`. So this is the
/// ramp arithmetic, not "train time" — do not wire it to a build queue and call it derived.
/// `docs/derivation/economy.md` §4.2 flags the same gap.
///
/// # Superseded
///
/// The caveat above is **closed**: the enclosing function is `ObjectData::train_time`
/// `0x006508C0`, `x` is the type's base job time, and the ceiling is 3x the
/// `unit_rate_base`-scaled base. [`crate::systems::production::train_time_ramp`] carries
/// that derivation with the surrounding train-time pipeline; this function computes the
/// same integers with none of the context and is kept only so existing callers still
/// build.
#[deprecated(
    note = "use crate::systems::production::train_time_ramp, which has the enclosing derivation"
)]
#[inline]
pub fn ramped_rate(x: i32, count: u16, job_extra_time: i32, r: &EconomyRules) -> i32 {
    let base = div100(x.wrapping_mul(r.unit_rate_base)); // 0x00650AF6..0x00650B10
    let ramp = (count as i32)
        .wrapping_mul(job_extra_time) // 0x00650B27
        .wrapping_mul(r.unit_rate_progression); // 0x00650B31
    let ceiling = base.wrapping_mul(3); // 0x00650B2E, lea eax,[ecx+ecx*2]
    let v = ramp.wrapping_add(base); // 0x00650B38
    if v < 0 || ceiling < 0 {
        0 // 0x00650B4B
    } else if v > ceiling {
        ceiling // 0x00650B47
    } else {
        v
    }
}

/// The "no-rush" style reduction applied after the ramp — `0x00650B67`..`0x00650B85`.
///
/// Gated by `game[0x820] & 4` (`0x00650B55`); `pct` is `FUN_006DA740(player)`, whose
/// meaning is not derived. `v * (200 - pct) / 200`, and the `/200` is the `0x51EB851F`
/// magic with `sar edx, 6` — half the usual shift, which is easy to misread as `/100`.
#[inline]
pub fn rate_after_game_option(v: i32, pct: i32) -> i32 {
    div200(200i32.wrapping_sub(pct).wrapping_mul(v))
}

/// The unit-class ramp ceiling for a cost — `0x006656AA`..`0x006656CD` in `FUN_00664090`.
///
/// `ramp_max_pct` is one of four rules selected by the unit's class:
/// `UNIT_SCHOLAR_RAMP_MAX` (+0x394, 2000), `UNIT_WORKER_RAMP_MAX` (+0x398, 500),
/// `UNIT_OTHER_CIVILIAN_RAMP_MAX` (+0x39C, 200), `UNIT_MILITARY_RAMP_MAX` (+0x3A0, 125),
/// at `0x006656AF` / `0x0066569F` / `0x006653FF` / `0x0066568F`. **Which class a unit is
/// in is not derived** here.
///
/// # Superseded
///
/// The four-way class selection **is** now measured, in
/// [`crate::systems::production`], along with `TypeData::get_cost`'s surrounding ramp.
/// Use that; this is the bare multiply.
#[deprecated(note = "use crate::systems::production, which carries the measured class selection")]
#[inline]
pub fn cost_ramp_ceiling(base_cost: i32, ramp_max_pct: i32) -> i32 {
    div100(ramp_max_pct.wrapping_mul(base_cost))
}

/// Apply that ceiling to one resource's ramped cost — `0x00665452`, `cmovl`.
///
/// A **zero ceiling means no ceiling** (`test esi,esi; je`), which is the opposite of the
/// natural reading and is why this is a named function rather than a `min`.
#[inline]
pub fn clamp_cost_to_ramp_ceiling(cost: i32, ceiling: i32) -> i32 {
    if ceiling != 0 && ceiling < cost {
        ceiling
    } else {
        cost
    }
}

/// Object-graph guards for [`attrition_interval_scale`], resolved by walking the engine's
/// object graph. Inputs, exactly like [`DamagePredicates`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AttritionPredicates {
    /// `unitType->vtbl[0x10C]()` at `0x00609012` — the siege-class query.
    pub siege_class: bool,
    /// `unit->vtbl[0xB8](0x42, 0)` at `0x00609047`, which the compiler devirtualises to
    /// `unitType->vtbl[0x60](0x42, 0)` at `0x0060905E`. Selects the **integer** branch;
    /// false takes the float branch.
    pub has_ability_0x42: bool,
    /// `FUN_006DB810(owner_player, 0x2FE)` at `0x00609152`. Float branch only.
    pub owner_prop_0x2fe: bool,
    /// `FUN_0046FA40(unit)` at `0x00609161`. Float branch only; with the one above it,
    /// makes the unit immune.
    pub unit_immune: bool,
}

/// Non-rule inputs to [`attrition_interval_scale`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AttritionInput {
    /// `players[attacker].attrition_level`, `player + 0x7F0`. Zero disables attrition
    /// outright (`0x00608FED`). Its construction is **not derived**;
    /// `ATTRITION_IMPROVED[] = 1,2,4,8` is the obvious candidate and stays a hypothesis.
    pub attrition_level: i32,
    /// `unitType[+4]`, the global type id. Three ids halve the interval (`0x00609085`).
    pub unit_type_id: i32,
    /// `unitType[+0x218]`, the domain. 2 (air) halves it again (`0x0060909F`).
    pub unit_domain: i32,
    /// `players[unit_owner] + 0x7F4`, a `f32` multiplier built at `0x006CDD20` from
    /// `ATTRITION_UPGRADE[]`, `LIBERTY_ATTRITION`, `COLOSSEUM_ATTRITION` and
    /// `KREMLIN_ATTRITION`. Float branch only.
    pub owner_attrition_mult: f32,
    /// `players[attacker].econ[0xDC] ^ 0x62766` — the attacker's age. `0x006090DE`.
    pub attacker_age: i32,
    /// The same field for the unit's own owner. `0x006090C8`.
    pub unit_owner_age: i32,
}

/// `FUN_00608FD0` — `__thiscall(Unit*, int attacker_player)`, `ret 4`.
///
/// # What this actually returns, which is not what its shape suggests
///
/// It is **not** a damage multiplier. It is an interval scale in 8.8 fixed point: the one
/// caller that matters multiplies it by `ATTRITION` — a value `rules.xml` documents as
/// `"48 frames"` — shifts off the 8.8, and stores the result in `unit + 0x9E`, which
/// `0x006117C8` uses as `(frame + phase) % period == 0`. So a **larger** return means a
/// **longer** gap between attrition ticks, which is why the final step divides by the
/// attrition level rather than multiplying: a higher level ticks *more often*.
///
/// `docs/derivation/economy.md` §5.2 flagged that inversion as unresolved and warned not to
/// implement it. It is resolved here [measured]: the decisive witness is `0x005E15DD`, the
/// assassin path, which stores `ASSASSIN_ATTRITION` (`"8 frames"`) into the very same word
/// with no scaling at all — a frame count, plainly.
///
/// # The chain
///
/// ```text
/// 0x00608FED  level == 0                      -> 0
/// 0x00608FFE  RULES.ATTRITION == 0            -> 0        (master switch)
/// 0x0060900B  v = 0x100                                    (1.0 in 8.8)
/// 0x00609021  siege: SIEGE_ATTRITION >= 100   -> 0
/// 0x0060903D          else v = 25600 / (100 - SIEGE_ATTRITION)
/// 0x0060906E  ability 0x42: v = v*100 / (100 + MILITIA_ATTRITION)
/// 0x0060913E  else:         v = (i32)((f32)v * owner_mult * 0.00390625f)
/// 0x00609096  type id in {0x3D, 0x3E, 0x190}: v /= 2
/// 0x006090AF  domain == 2:                    v /= 2
/// 0x006090F9  age advantage >= 0: level = (level*(100 + AGED_UP*diff) + 99) / 100
/// 0x0060911B  return v / level
/// ```
///
/// The `+ 99` before the `/100` is a ceiling, and it is on the *level*, not the interval.
///
/// # Panics
///
/// Where retail's `idiv` raises `#DE`: `MILITIA_ATTRITION == -100`, or an age-adjusted
/// level of 0 (reachable when the ceiling term drives a positive level to zero, e.g. a
/// large negative `ATTRITION_AGED_UP`).
pub fn attrition_interval_scale(
    i: &AttritionInput,
    p: &AttritionPredicates,
    r: &EconomyRules,
) -> i32 {
    let mut level = i.attrition_level;
    if level == 0 {
        return 0; // 0x00608FED
    }
    if r.attrition == 0 {
        return 0; // 0x00608FFE
    }
    let mut v: i32 = 0x100; // 0x0060900B

    if p.siege_class {
        if r.siege_attrition >= 100 {
            return 0; // 0x00609027
        }
        v = 25600 / 100i32.wrapping_sub(r.siege_attrition); // 0x0060903D
    }

    if p.has_ability_0x42 {
        v = idiv_trapping(
            v.wrapping_mul(100),
            r.militia_attrition.wrapping_add(100),
            "0x0060907B",
        );
    } else {
        // 0x0060912C: int -> f32, two SSE multiplies in this order, then cvttss2si.
        v = cvttss2si((v as f32) * i.owner_attrition_mult * 0.003_906_25);
        if p.owner_prop_0x2fe && p.unit_immune {
            return 0; // 0x0060916E
        }
    }

    if matches!(i.unit_type_id, 0x3D | 0x3E | 0x190) {
        v /= 2; // 0x00609096
    }
    if i.unit_domain == 2 {
        v /= 2; // 0x006090A8
    }

    let age_diff = i.attacker_age.wrapping_sub(i.unit_owner_age); // 0x006090E9
    if age_diff >= 0 {
        level = div100(
            r.attrition_aged_up
                .wrapping_mul(age_diff)
                .wrapping_add(100)
                .wrapping_mul(level)
                .wrapping_add(99),
        ); // 0x006090F9..0x00609114
    }

    idiv_trapping(v, level, "0x0060911B")
}

/// Turn an interval scale into the frame period stored at `unit + 0x9E` — `0x005E193D`.
///
/// `max(1, ATTRITION * scale / 256)`, the `/256` being the bias-then-shift idiom at
/// `0x005E1951` (truncation toward zero, not an arithmetic shift) and the floor being a
/// `cmovg` against a preloaded 1. A scale of 0 means "no attrition" and the caller skips
/// the store entirely (`0x005E1937`), so this is only called with a non-zero scale.
#[inline]
pub fn attrition_period_frames(attrition_rule: i32, interval_scale: i32) -> i32 {
    let t = div256(attrition_rule.wrapping_mul(interval_scale));
    if t > 1 {
        t
    } else {
        1
    }
}

/// Merge a new attrition period into the one already on the unit — `0x005E1962`.
///
/// Retail keeps the **smallest** non-zero period, i.e. the fastest attrition source wins,
/// and the store is a 16-bit truncation (`mov word ptr [ebx+0x9E], cx`).
#[inline]
pub fn merge_attrition_period(existing: i16, candidate: i32) -> i16 {
    if existing == 0 || candidate < existing as i32 {
        candidate as i16
    } else {
        existing
    }
}

/// Does attrition tick this frame? — `0x006117C8`, in `FUN_00610BC0`'s per-unit update.
///
/// ```text
/// period = (i16)unit[0x9E];  if (period == 0) no
/// (frame + (i16)unit[0x0A]) % period == 0
/// ```
///
/// `unit + 0x0A` is a per-unit `i16` phase, which staggers units across frames instead of
/// having every unit tick together. `frame` is `[*(0x00C061EC) + 0x550]`.
///
/// This closes the "attrition timing" gap that `docs/derivation/economy.md` §5.3 left open
/// and `docs/provenance-ledger.md` lists as not derived.
///
/// # Panics
///
/// On `period == -1` with a frame sum of `i32::MIN`, where retail's `idiv` raises `#DE`.
#[inline]
pub fn attrition_fires(frame: i32, phase: i16, period: i16) -> bool {
    if period == 0 {
        return false; // 0x006117CF
    }
    idiv_rem_trapping(
        frame.wrapping_add(phase as i32),
        period as i32,
        "0x006117E9",
    ) == 0
}

/// Is the unit's attrition state recomputed this frame? — `0x006115EA`.
///
/// `(frame + (i16)unit[0x0A]) % 32 == 0`, written by the compiler as
/// `and eax, 0x8000001F` plus the negative fixup, which is signed `% 32`. The body it
/// gates is `FUN_005E11A0`, which is where [`attrition_interval_scale`] is called from
/// (`0x005E192E`). So the *period* is refreshed every 32 frames per unit, on the same
/// stagger as [`attrition_fires`], while the *ticks* run at the period itself.
#[inline]
pub fn attrition_recompute_due(frame: i32, phase: i16) -> bool {
    frame.wrapping_add(phase as i32) % 32 == 0
}

/// Signed remainder that traps exactly where retail's `idiv` traps. See [`idiv_trapping`].
#[inline]
#[track_caller]
fn idiv_rem_trapping(num: i32, den: i32, site: &str) -> i32 {
    if den == 0 || (num == i32::MIN && den == -1) {
        panic!("retail raises #DE here: idiv {num} / {den} at {site}");
    }
    num % den
}

#[cfg(test)]
#[allow(deprecated)] // these tests exist to pin the superseded functions until callers move
mod economy_tests {
    use super::*;

    /// `GATHER_RATE * 16`. The shipped 450 gives 7200, and at the engine's 15 fps that is
    /// one payout per 30 s — corroborated by the UI at `0x0072A5B0`, which divides
    /// `GATHER_RATE` by 15 and prints seconds.
    #[test]
    fn accumulator_period_is_gather_rate_times_sixteen() {
        let r = EconomyRules::shipped();
        assert_eq!(r.gather_rate, 450);
        assert_eq!(resource_period(r.gather_rate), 7200);
        assert_eq!(resource_period(r.gather_rate) / 15, 480); // frames*16 / fps
    }

    /// The carry loop is not `income / period`: a remainder left over from earlier frames
    /// can push a later frame over the line. Drive it a frame at a time and count.
    #[test]
    fn accumulator_carries_remainders_across_frames() {
        let period = 7200;
        let mut acc = 0;
        let mut total = 0;
        // 100 sixteenths per frame: 7200/100 = one whole resource every 72 frames.
        for _ in 0..720 {
            total += credit_resource(100, period, &mut acc);
        }
        assert_eq!(total, 10);
        assert_eq!(acc, 0);
        // A single frame's income above the period credits whole units immediately.
        let mut acc2 = 0;
        assert_eq!(credit_resource(7200 * 3 + 1, period, &mut acc2), 3);
        assert_eq!(acc2, 1);
        // And a pre-loaded accumulator drains through the loop, not through the divide.
        let mut acc3 = period - 1;
        assert_eq!(credit_resource(1, period, &mut acc3), 1);
        assert_eq!(acc3, 0);
    }

    #[test]
    #[should_panic(expected = "0x006CE7C5")]
    fn zero_gather_rate_traps_like_retail() {
        let mut acc = 0;
        credit_resource(1, resource_period(0), &mut acc);
    }

    /// Negative income abandons the resource: no accumulation at all, only a display value.
    /// This is the early-out at `0x006CE4E7` that the derivation document does not mention.
    #[test]
    fn negative_income_returns_early() {
        let r = EconomyRules::shipped();
        let i = ResourceTickInput {
            gross: 10,
            expense: 40,
            commerce_cap: 500,
            ..Default::default()
        };
        let t = resource_tick(&i, &r);
        assert_eq!(t.displayed, -30);
        assert_eq!(t.accumulated, None);
    }

    /// The commerce cap clamps before anything else, and the 16,000 ceiling only exists on
    /// the interest path — so an uncapped, interest-less player can exceed it.
    #[test]
    fn commerce_cap_clamps_first_and_the_global_ceiling_is_conditional() {
        let r = EconomyRules::shipped();
        let base = ResourceTickInput {
            gross: 100_000,
            commerce_cap: 500,
            ..Default::default()
        };
        assert_eq!(resource_tick(&base, &r).accumulated, Some(500));

        // A cap above 16,000 with no interest term: the ceiling never runs.
        let no_interest = ResourceTickInput {
            commerce_cap: 20_000,
            ..base
        };
        assert_eq!(resource_tick(&no_interest, &r).accumulated, Some(20_000));

        // Same input, interest path taken: now 0x006CE706 applies.
        let interest = ResourceTickInput {
            interest_applies: true,
            ..no_interest
        };
        assert_eq!(resource_tick(&interest, &r).accumulated, Some(0x3E70));
    }

    /// The displayed income is cached three steps early, so it is not the income that is
    /// actually credited. Anything mirroring the UI number must not use it as the truth.
    #[test]
    fn displayed_income_is_taken_before_the_last_multipliers() {
        let r = EconomyRules::shipped();
        let i = ResourceTickInput {
            gross: 1_000,
            commerce_cap: 100_000,
            gather_bonus_pct: 50,
            game_speed: 2,
            ..Default::default()
        };
        let t = resource_tick(&i, &r);
        assert_eq!(t.displayed, 1_000);
        assert_eq!(t.accumulated, Some(3_000)); // 1000 * 150/100, then * 2
    }

    /// Knowledge is the odd slot: hardcoded 999 cap, difficulty penalty, no interest.
    #[test]
    fn knowledge_is_special_cased() {
        let r = EconomyRules::shipped();
        let g = CommerceCapGates {
            british: true,
            resource_civ: true,
        };
        assert_eq!(commerce_cap(0, RES_KNOWLEDGE, &r, &g, 500), 999);
        assert_eq!(commerce_cap(7, RES_KNOWLEDGE, &r, &g, 0), 999);

        let hard = ResourceTickInput {
            res: RES_KNOWLEDGE,
            gross: 1_000,
            commerce_cap: 100_000,
            difficulty: 5,
            ..Default::default()
        };
        assert_eq!(resource_tick(&hard, &r).accumulated, Some(750));
        let hardest = ResourceTickInput {
            difficulty: 7,
            ..hard
        };
        assert_eq!(resource_tick(&hardest, &r).accumulated, Some(500));
        // Difficulty 4 and below is untouched, and the penalty is knowledge-only.
        let easy = ResourceTickInput {
            difficulty: 4,
            ..hard
        };
        assert_eq!(resource_tick(&easy, &r).accumulated, Some(1_000));
        let food = ResourceTickInput {
            res: RES_FOOD,
            ..hard
        };
        assert_eq!(resource_tick(&food, &r).accumulated, Some(1_000));
    }

    #[test]
    fn commerce_cap_is_indexed_by_age() {
        let r = EconomyRules::shipped();
        let none = CommerceCapGates::default();
        assert_eq!(commerce_cap(0, RES_FOOD, &r, &none, 0), 70);
        assert_eq!(commerce_cap(7, RES_FOOD, &r, &none, 0), 500);
        // British is a percent on top; the per-resource civ percent stacks after it.
        let brit = CommerceCapGates {
            british: true,
            resource_civ: false,
        };
        assert_eq!(commerce_cap(0, RES_FOOD, &r, &brit, 0), 87); // 70 * 125/100
        let both = CommerceCapGates {
            british: true,
            resource_civ: true,
        };
        assert_eq!(commerce_cap(0, RES_WEALTH, &r, &both, 0), 115); // 87 * 133/100
        assert_eq!(commerce_cap(0, RES_WEALTH, &r, &both, 25), 140);
    }

    #[test]
    #[should_panic(expected = "COMMERCE_CAP has 8 entries")]
    fn age_past_the_table_is_refused() {
        commerce_cap(
            8,
            RES_FOOD,
            &EconomyRules::shipped(),
            &CommerceCapGates::default(),
            0,
        );
    }

    /// The ramp saturates at 3x base, and a negative intermediate collapses to zero rather
    /// than propagating.
    #[test]
    fn rate_ramp_clamps_to_three_times_base() {
        let r = EconomyRules::shipped();
        assert_eq!(ramped_rate(100, 0, 1, &r), 120); // 100 * 120/100
        assert_eq!(ramped_rate(100, 1, 1, &r), 195); // + 1*1*75
        assert_eq!(ramped_rate(100, 3, 1, &r), 345);
        assert_eq!(ramped_rate(100, 4, 1, &r), 360); // clamped: 3 * 120
        assert_eq!(ramped_rate(100, 60_000, 1, &r), 360);
        assert_eq!(ramped_rate(-100, 0, 1, &r), 0); // base < 0 -> 0
    }

    /// A zero ramp ceiling means *no* ceiling.
    #[test]
    fn cost_ramp_ceiling_of_zero_does_not_clamp() {
        assert_eq!(cost_ramp_ceiling(200, 125), 250);
        assert_eq!(clamp_cost_to_ramp_ceiling(400, 250), 250);
        assert_eq!(clamp_cost_to_ramp_ceiling(100, 250), 100);
        assert_eq!(clamp_cost_to_ramp_ceiling(400, 0), 400);
    }

    /// An unmodified interval scale of `0x100` gives back `ATTRITION` frames exactly — the
    /// "baseline level for regular attrition" the shipped comment describes, which is what
    /// pins `0x100` as 1.0 in 8.8 rather than as an arbitrary constant.
    #[test]
    fn baseline_attrition_period_is_the_attrition_rule_itself() {
        let r = EconomyRules::shipped();
        assert_eq!(attrition_period_frames(r.attrition, 0x100), 48);
        assert_eq!(r.attrition, 48);

        // Shipped MILITIA_ATTRITION is 300, so the ability branch quarters the interval:
        // 256*100/400 = 64, and 48*64/256 = 12 frames.
        let p = AttritionPredicates {
            has_ability_0x42: true,
            ..Default::default()
        };
        let i = AttritionInput {
            attrition_level: 1,
            ..Default::default()
        };
        assert_eq!(attrition_interval_scale(&i, &p, &r), 64);
        assert_eq!(attrition_period_frames(r.attrition, 64), 12);

        // The float branch multiplies by `player[+0x7F4]` and then by 1/256, so 256.0 is
        // its identity. **The real scale of that field is not derived** (it is built at
        // 0x006CDD20 by repeated `f = f*100/(100-reduction)` steps whose base we did not
        // recover), so this pins the arithmetic, not the game's baseline.
        let p2 = AttritionPredicates::default();
        let i2 = AttritionInput {
            owner_attrition_mult: 256.0,
            ..i
        };
        assert_eq!(attrition_interval_scale(&i2, &p2, &r), 256);
    }

    /// The inversion that `docs/derivation/economy.md` §5.2 flagged: a higher attrition
    /// level shortens the period, which is why the last step divides by it.
    #[test]
    fn higher_attrition_level_shortens_the_period() {
        let r = EconomyRules::shipped();
        let p = AttritionPredicates {
            has_ability_0x42: true,
            ..Default::default()
        };
        let mut last = i32::MAX;
        for level in [1, 2, 4, 8] {
            let i = AttritionInput {
                attrition_level: level,
                ..Default::default()
            };
            let period = attrition_period_frames(r.attrition, attrition_interval_scale(&i, &p, &r));
            assert!(
                period < last,
                "level {level} gave period {period}, not shorter than {last}"
            );
            last = period;
        }
        assert_eq!(last, 1); // level 8: 64/8 = 8 -> 48*8/256 = 1, the floor
    }

    /// The master switch and the zero level both return 0, which the call site treats as
    /// "no attrition at all" — it skips the store entirely.
    #[test]
    fn attrition_master_switch_and_zero_level() {
        let r = EconomyRules::shipped();
        let p = AttritionPredicates {
            has_ability_0x42: true,
            ..Default::default()
        };
        let i = AttritionInput {
            attrition_level: 0,
            ..Default::default()
        };
        assert_eq!(attrition_interval_scale(&i, &p, &r), 0);

        let off = EconomyRules { attrition: 0, ..r };
        let i2 = AttritionInput {
            attrition_level: 4,
            ..Default::default()
        };
        assert_eq!(attrition_interval_scale(&i2, &p, &off), 0);
    }

    /// Siege units at 50% reduction get double the interval, and a 100% reduction is
    /// immunity rather than a divide by zero.
    #[test]
    fn siege_reduction_lengthens_the_interval_and_saturates_at_immunity() {
        let r = EconomyRules::shipped();
        let p = AttritionPredicates {
            siege_class: true,
            has_ability_0x42: true,
            ..Default::default()
        };
        let i = AttritionInput {
            attrition_level: 1,
            ..Default::default()
        };
        // 25600/(100-50) = 512, then the militia branch: 512*100/400.
        assert_eq!(attrition_interval_scale(&i, &p, &r), 128);
        let immune = EconomyRules {
            siege_attrition: 100,
            ..r
        };
        assert_eq!(attrition_interval_scale(&i, &p, &immune), 0);
    }

    /// The float branch must truncate the way `cvttss2si` does, including its out-of-range
    /// result — `as i32` alone would saturate and silently diverge.
    #[test]
    fn float_branch_truncates_toward_zero_like_cvttss2si() {
        assert_eq!(cvttss2si(1.9), 1);
        assert_eq!(cvttss2si(-1.9), -1);
        assert_eq!(cvttss2si(f32::NAN), i32::MIN);
        assert_eq!(cvttss2si(f32::INFINITY), i32::MIN);
        assert_eq!(cvttss2si(1e30), i32::MIN);
        assert_ne!(cvttss2si(1e30), i32::MAX); // what `as i32` would give
    }

    /// The period gate and the 32-frame recompute gate share the per-unit phase, so two
    /// units with different phases never tick on the same frames.
    #[test]
    fn attrition_timing_is_staggered_by_the_unit_phase() {
        assert!(attrition_fires(0, 0, 48));
        assert!(!attrition_fires(1, 0, 48));
        assert!(attrition_fires(48, 0, 48));
        assert!(attrition_fires(47, 1, 48));
        assert!(!attrition_fires(5, 0, 0)); // period 0 = no attrition

        assert!(attrition_recompute_due(0, 0));
        assert!(attrition_recompute_due(32, 0));
        assert!(!attrition_recompute_due(31, 0));
        assert!(attrition_recompute_due(31, 1));
    }

    /// The fastest source wins, and the store is 16-bit.
    #[test]
    fn attrition_periods_merge_by_minimum() {
        assert_eq!(merge_attrition_period(0, 48), 48);
        assert_eq!(merge_attrition_period(48, 8), 8);
        assert_eq!(merge_attrition_period(8, 48), 8);
        assert_eq!(merge_attrition_period(0, 0x1_0008), 8); // truncated to a word
    }
}

#[cfg(test)]
mod damage_tests {
    use super::*;

    /// A world where every predicate is false and every rule constant is zero: the chain
    /// collapses to its spine, steps 1, 21, 22 and 28.
    fn spine(attack: i32, balance: i32, armor: i32) -> i32 {
        let i = DamageInput {
            balance_pct: balance,
            attack,
            armor,
            ..Default::default()
        };
        damage(
            &i,
            &DamagePredicates::default(),
            &CombatRules::default(),
            &UnreachedTerms::default(),
        )
    }

    #[test]
    fn spine_matches_retail_on_captured_vectors() {
        // Every value here is **captured** from retail `FUN_00644130` via
        // `oracle damage-vectors` on hbox, not hand-computed. The ledger already records
        // one case where a hand-computed expectation was wrong and the test enshrined the
        // error; capture, do not calculate.
        assert_eq!(spine(100, 100, 0), 10);
        assert_eq!(spine(100, 100, 3), 7);
        assert_eq!(spine(45, 100, 10), 1);
        assert_eq!(spine(0, 0, 0), 1);
        assert_eq!(spine(105, 100, 0), 11);
        assert_eq!(spine(104, 100, 0), 10);
        assert_eq!(spine(1000, 250, 7), 243);
        assert_eq!(spine(-100, 100, 0), 1);
        assert_eq!(spine(2147483647, 100, 0), 1);
        assert_eq!(spine(100, -100, 0), 1);
        assert_eq!(spine(100, 100, -5), 15);
        assert_eq!(spine(5, 100, 0), 1);
        assert_eq!(spine(4, 100, 0), 1);
        assert_eq!(spine(10, 100, 100), 1);
    }

    #[test]
    fn attack_is_carried_times_ten_and_rounds_half_up() {
        // ATTACK=10 in unitrules.xml is stored as 100; against a 100% balance entry with
        // no armor that is 10 damage, not 100. Captured, as above.
        assert_eq!(spine(100, 100, 0), 10);
        assert_eq!(spine(105, 100, 0), 11); // (105+5)/10
        assert_eq!(spine(104, 100, 0), 10);
    }

    #[test]
    fn floor_is_conditional_on_splash() {
        // splash_flag != 0 suppresses the floor entirely (0x00644F2D) *and* opens the
        // splash block. Both retail outputs captured via `oracle damage-vectors`.
        let mut i = DamageInput {
            attack: 10,
            balance_pct: 100,
            armor: 100,
            defender_splash_divisor: 1,
            ..Default::default()
        };
        let p = DamagePredicates::default();
        let r = CombatRules {
            height_increment: 1,
            ..Default::default()
        };
        let u = UnreachedTerms::default();
        assert_eq!(damage(&i, &p, &r, &u), 1);
        i.splash_flag = 1;
        assert_eq!(damage(&i, &p, &r, &u), -100);
    }

    #[test]
    fn floor_is_conditional_on_land_versus_sea() {
        // Captured: attack=1 balance=100 armor=50, land attacker vs sea defender -> -50.
        // A land-vs-sea pairing skips the floor at 0x00644F5D, so the negative survives.
        let i = DamageInput {
            attack: 1,
            balance_pct: 100,
            armor: 50,
            attacker_domain: 0,
            defender_domain: 1,
            defender_splash_divisor: 1,
            ..Default::default()
        };
        let r = CombatRules {
            height_increment: 1,
            ..Default::default()
        };
        assert_eq!(
            damage(
                &i,
                &DamagePredicates::default(),
                &r,
                &UnreachedTerms::default()
            ),
            -50
        );
    }

    #[test]
    fn armor_lands_mid_chain_so_later_modifiers_scale_a_negative() {
        // The derivation's headline claim. Armor is step 22 of 31, so an overkill
        // modifier (step 23) scales an already-negative intermediate. Both values
        // captured from retail.
        //
        // This is not a stylistic point: moving the subtraction to the end of the chain
        // — the community formula — diverges from retail on 50,164 of 199,629 trials.
        let mut i = DamageInput {
            attack: 100,
            balance_pct: 100,
            armor: 20,
            overkill_gate: 1,
            defender_overkill_stamp: 1,
            current_frame: 2,
            attacker_vf_0xe4: 1,
            defender_word_0xa4: 2,
            defender_splash_divisor: 1,
            ..Default::default()
        };
        let p = DamagePredicates {
            attacker_vf_0x130: true,
            attacker_vf_0x18: true,
            defender_vf_0x18: true,
            ..Default::default()
        };
        let r = CombatRules {
            height_increment: 1,
            overkill_frames: 30,
            overkill_damage: 128, // 0.5 in 8.8 fixed point
            ..Default::default()
        };
        let u = UnreachedTerms::default();
        assert_eq!(damage(&i, &p, &r, &u), 1);

        // Same scenario with splash set: the floor is suppressed and the negative shows.
        i.splash_flag = 1;
        i.attacker_splash_percent = 100;
        assert_eq!(damage(&i, &p, &r, &u), -5);
    }

    #[test]
    fn attacker_mask_fixup_cannot_change_the_result() {
        // 0x006442A3 clears mask bit 17 and sets bit 6. Neither bit is tested anywhere
        // downstream in FUN_00644130, so the fixup is structurally unobservable through
        // the return value — confirmed by mutation: deleting it entirely produced 0
        // mismatches over 199,629 differential trials. Retail returns 100 here.
        let i = DamageInput {
            attack: 1000,
            balance_pct: 100,
            attacker_masks: 0x0002_0000,
            attacker_flag8_bit5: true,
            defender_splash_divisor: 1,
            ..Default::default()
        };
        let mut p = DamagePredicates {
            mask_fixup_authorised: true,
            ..Default::default()
        };
        let r = CombatRules {
            height_increment: 1,
            ..Default::default()
        };
        let u = UnreachedTerms::default();
        assert_eq!(damage(&i, &p, &r, &u), 100);
        p.mask_fixup_authorised = false;
        assert_eq!(damage(&i, &p, &r, &u), 100);
    }

    #[test]
    fn flank_level_boundaries() {
        assert_eq!(flank_level(0xD555_5556), 0);
        assert_eq!(flank_level(0xD555_5555), 2);
        assert_eq!(flank_level(0), 2);
        assert_eq!(flank_level(0x6000_0000), 1);
        assert_eq!(flank_level(0xA000_0000), 1);
        assert_eq!(flank_level(0xA000_0001), 2);
    }

    #[test]
    fn entrench_dir_is_not_flank_level() {
        // The two classifiers disagree; assuming one function served both would be an
        // invisible error, so pin the disagreement.
        assert_ne!(flank_level(0), entrench_dir_level(0));
        assert_eq!(entrench_dir_level(0), 0);
        assert_eq!(flank_level(0), 2);
    }

    #[test]
    fn balance_index_uses_493_stride_with_attacker_as_row() {
        assert_eq!(balance_index(0, 0), 0);
        assert_eq!(balance_index(1, 0), 493);
        assert_eq!(balance_index(0, 1), 1);
        assert_eq!(balance_index(492, 492), 492 * 493 + 492);
    }

    #[test]
    #[should_panic(expected = "0x006448B9")]
    fn zero_splash_divisor_traps_like_retail() {
        let i = DamageInput {
            attack: 100,
            balance_pct: 100,
            splash_flag: 1,
            defender_splash_divisor: 0,
            ..Default::default()
        };
        let p = DamagePredicates {
            defender_vf_0x18: true,
            ..Default::default()
        };
        damage(&i, &p, &CombatRules::default(), &UnreachedTerms::default());
    }
}

#[cfg(test)]
#[allow(deprecated)] // these tests exist to pin the superseded functions until callers move
mod tests {
    use super::*;

    /// Values **captured from the retail code** via `oracle vectors`, not hand-computed.
    /// An earlier version of this test had `(7, -3, -100, 100)` as -47 from my own
    /// arithmetic; the binary says -247. Capture, do not calculate.
    #[test]
    fn matches_retail_on_captured_vectors() {
        assert_eq!(hash_into_range(0, 0, 0, 0), 0);
        assert_eq!(hash_into_range(1, 1, 0, 1), 1);
        assert_eq!(hash_into_range(-1, -1, -5, 5), -6);
        assert_eq!(hash_into_range(7, -3, -100, 100), -247);
        assert_eq!(hash_into_range(123456, 789, 0, 0), 0);
        assert_eq!(hash_into_range(5, 5, 10, -10), 30);
        assert_eq!(hash_into_range(100, 3, 1, 6), 1);
        assert_eq!(hash_into_range(-9, 4, 0, 100), 21);
    }

    #[test]
    fn wrapping_multiplication_does_not_panic_on_extremes() {
        // i32::MIN * i32::MIN wraps; a naive implementation panics in debug builds.
        let _ = hash_into_range(i32::MIN, 1, 0, 10);
        let _ = hash_into_range(i32::MAX, i32::MAX, i32::MIN, i32::MAX);
        let _ = hash_into_range(i32::MIN, i32::MIN, i32::MAX, i32::MIN);
    }

    #[test]
    fn result_may_fall_below_lo_because_idiv_truncates() {
        // Faithful to the binary: the remainder carries the dividend's sign.
        let r = hash_into_range(-1, -1, -5, 5);
        assert!(
            r < -5,
            "expected sub-lo result from a negative dividend, got {r}"
        );
    }

    #[test]
    fn inverted_range_is_accepted() {
        // hi < lo uses |hi - lo|; the retail code does not order them.
        let a = hash_into_range(5, 5, 10, -10);
        let b = hash_into_range(5, 5, 10, -10);
        assert_eq!(a, b);
    }
}
