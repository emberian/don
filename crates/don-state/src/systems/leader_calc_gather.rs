//! `Leader::calc_gather` 0x006CEEE0 (`re/decomp-all/006ceee0.c`) — the
//! periodic income recompute that `Leader::gather` 0x006CE280 runs on the
//! decrypted `resources[6]` locals. Leaf module: called only from
//! `leaders_process::gather`, which has already evaluated the gate.
//!
//! # Gate (0x006CEEF4..0x006CEF27, 0x006CF788..0x006CF7A4)
//!
//! `leader_flags & 0x2000000` clear: run iff `frame >= gather_stamp + 300
//! && (frame + who*8) % 256 == 0`. Set: run iff `frame == 0 || (who +
//! frame) % 8 == 0`. ([`due`] below.)
//!
//! # Body, in retail order
//!
//! 1. `BitMask<44>::clear` on `rare_owned` (+0x6dac).
//! 2. `known_rares` (+0x6d4) = Σ `reg_known_rares[0..64]` (+0x4d4).
//! 3. `rares_collected[0..44]` (+0x6d8) = 0.
//! 4. `has_wonder(0x215)` → scan the global rare-resource object list
//!    (`DAT_00c0a0e4` count / `DAT_00c0a0f0` list): each live object whose
//!    virtual +0x48(who) and `type_avail(type, 1)` pass and whose tile owner
//!    (`World.wdata[y*xs+x].owner`, +0xf) is `who` sets `rare_owned` bit
//!    `type-6` and clears `rare_owned.flags`. UNTRANSCRIBED (Objects
//!    scan); only reached with the wonder.
//! 5. `resources[g] = Constants[0x24c + 4g] << 4`; `bonus[g] = 0` (stores
//!    the bare key `0x6722`), g in 0..6.
//! 6. `has_tribe_bonus(0x13)` → `resources[0] += (stable_units −
//!    scout_garr − stable_garr − peasants_garr + scouts + peasants) *
//!    Constants[0x848] * 16`.
//! 7. `has_tribe_bonus(0x14)` → `v = (barracks_units − barracks_garr) *
//!    Constants[0x888] * 16`; added to `resources[g]` for g ∈ {0,1,4,2}
//!    with `type_avail(g,1)`.
//! 8. Unless `Game::is_scenario() && !DAT_00cbe329`: the object sums —
//!    `City::calc_gather` 0x00737C60 per city (`city_mark` list),
//!    `BuildData::calc_gather` 0x0062D360 per oil well (`oil_well_mark`
//!    list) and per leader building (indices ≥ 2000) of type 0x1a2/0x1a3
//!    with `+0x72 < 0` and virtual +0x180 == 0, and `Unit::do_gather`
//!    0x005FCE20 per caravan (`+0x15c` list) that is on a trade route.
//!    UNTRANSCRIBED — these walk live Objects and the gather tables
//!    (`FUN_006d5530`, `FUN_00639e40`, `FUN_00609180`).
//! 9. `resources[5] = ((get_buildings(0x1aa, num_buildings[0x1aa]) *
//!    Constants[0x2f8] + 100) * resources[5]) / 100`; `has_preq(0x325)` →
//!    `resources[5] += Constants[0x9c0]*16`; `Constants[0x59c] < 0 &&
//!    has_tribe_bonus(2)` → `resources[2] += resources[4]`.
//! 10. Rares (0x006CF4A6..0x006CF57C): `rare_conquest.flags` (+0x6dc8, a
//!     cached "empty" bit, not serialized) selects: `& 1` → skip; `== 0` →
//!     scan; else recompute from the payload (`OR == 0` → set 1 and skip,
//!     else clear and scan). The scan is suppressed when any other live
//!     leader has `num_units[318] != 0`; otherwise for each set bit `i` of
//!     `rare_conquest`: `rares_collected[i] += 1`, `calc_rare(i+6, tmp, 0)`
//!     0x006E08D0 and `resources += tmp`. `calc_rare` is UNTRANSCRIBED;
//!     with an all-zero `rare_conquest` payload the scan adds nothing, so
//!     the outcome is fully determined by the preceding zeroing.
//! 11. `tmp = rare_owned | rare_conquest; if rare != tmp`: when bit 7 of
//!     `rare.data[2]` differs, `Regions::fix_all_borders` 0x0067F7D0;
//!     `rare = tmp`; `leader_flags |= 0xc000000`; `Leader::calc_pop_cap`
//!     0x006DC490. The two calls are UNTRANSCRIBED (region borders / pop
//!     cap); the assignment and flag are transcribed.
//! 12. `has_rare(0x2c)` → every `resources[g] = (Constants[0x97c]+100) *
//!     resources[g] / 100`.
//! 13. `World+0x78 != 0` → taxation: `t = Constants[0x334 +
//!     4*get_taxation()]`, `has_tribe_bonus(0xb)` → `t = (Constants[0x6ec]
//!     +100)*t/100`, `Game.semaphore.ptr[2] & 2 && has_conquest_bonus(0x20)`
//!     → `t = (Constants[0xb08]+100)*t/100`; `resources[2] += territory *
//!     t * 16 / World+0x78`; `has_tribe_bonus(0x11) && Constants[0x7fc] !=
//!     0` → `resources[0] += (Game.num_nations * territory * 800 /
//!     World+0x78) / Constants[0x7fc]`.
//! 14. `LeaderData::calc_resource_bonuses(resources)` 0x006DB030.
//! 15. `Leaders[who].leader_flags &= ~0x2000000`; `gather_stamp = frame`.
//! 16. For g in 0..6: `!type_avail(g,1) && has_preq(g)` → redirect
//!     `resources[g] * goodtypes[g].+0x2d8 / 256` into good
//!     `goodtypes[g].+0x2c4` (when ≥ 0) and `resources[g] = 0`.
//!
//! # What this module writes
//!
//! Steps 1–3, 5 (`bonus` only), 10 (for an empty `rare_conquest`), 11
//! (assignment + flag) and 15 — every write that does not depend on the
//! object sums — exactly as retail does. `resources[6]` (steps 5–9,
//! 12–14, 16) are left untouched because the object sums in step 8 are
//! UNTRANSCRIBED; the effect log records the gate firing. When
//! `has_wonder(0x215)` is set or `rare_conquest` has bits, nothing is
//! written (steps 4/10 would feed the writes).
//!
//! No `Random::get` on any path.

use crate::sections::Leader;
use crate::tick::StepStatus;

use super::leaders_process::{
    enc_put_i32, ld_i32, ld_put_i32, readable, Ctx, Eval, ENC_BONUS, ENC_PER_GOOD, LD_GATHER_STAMP, LD_WHO,
    NUM_GOODS,
};

/// Non-object writes transcribed; `resources` recompute is not.
pub const STATUS: StepStatus = StepStatus::Partial;

// --- LeaderData image offsets (body index = image offset − 8) ---------------
const LD_REG_KNOWN_RARES: usize = 0x4d4; // int[64]
const LD_KNOWN_RARES: usize = 0x6d4;
const LD_RARES_COLLECTED: usize = 0x6d8; // int[44]
const NUM_RARES: usize = 44;

/// The `calc_gather` gate.
pub fn due(frame: i32, leader_flags: i32, who: i32, gather_stamp: i32) -> bool {
    if leader_flags & 0x2000000 == 0 {
        frame >= gather_stamp.wrapping_add(300) && frame.wrapping_add(who.wrapping_mul(8)) % 256 == 0
    } else {
        frame == 0 || who.wrapping_add(frame) % 8 == 0
    }
}

/// Run the body for `slots[slot]` (gate already passed). Returns true when
/// the non-object writes were applied.
pub(crate) fn run(ctx: &Ctx, slots: &mut [Leader], slot: usize, effects: &mut Vec<String>) -> bool {
    let frame = ctx.frame;
    let who = ld_i32(&slots[slot], LD_WHO);
    let prefix = format!("Leader[{slot}] who {who}: calc_gather 0x006CEEE0 (frame {frame})");

    // Gates that would feed the writes below from unserialized state.
    {
        let ev = Eval { ctx, l: &slots[slot] };
        match ev.has_wonder(0x215) {
            Some(0) => {}
            Some(_) => {
                effects.push(format!("{prefix}: has_wonder(0x215) rare-territory scan needs Objects; nothing written"));
                return false;
            }
            None => {
                effects.push(format!("{prefix}: has_wonder(0x215) undecidable (wonder_mark > 0); nothing written"));
                return false;
            }
        }
        if slots[slot].rare_conquest.data.iter().any(|&b| b != 0) {
            effects.push(format!("{prefix}: rare_conquest non-empty → calc_rare 0x006E08D0 UNTRANSCRIBED; nothing written"));
            return false;
        }
    }
    let who_slot = usize::try_from(who).ok().filter(|&w| w < slots.len() && readable(&slots[w]));

    let l = &mut slots[slot];
    // 1. rare_owned.clear()
    for b in l.rare_owned.data.iter_mut() {
        *b = 0;
    }
    // 2. known_rares = Σ reg_known_rares
    let mut sum: i32 = 0;
    for r in 0..64 {
        sum = sum.wrapping_add(ld_i32(l, LD_REG_KNOWN_RARES + r * 4));
    }
    ld_put_i32(l, LD_KNOWN_RARES, sum);
    // 3. rares_collected = 0
    for i in 0..NUM_RARES {
        ld_put_i32(l, LD_RARES_COLLECTED + i * 4, 0);
    }
    // 5. bonus[g] = 0 (resources[g] base: Constants[0x24c + 4g] << 4 — not stored, see module doc)
    for g in 0..NUM_GOODS {
        enc_put_i32(l, g * ENC_PER_GOOD + ENC_BONUS, 0);
    }
    // 10. empty rare_conquest: scan adds nothing.
    // 11. rare = rare_owned | rare_conquest (both zero here).
    let n = l.rare_owned.data.len();
    let or: Vec<u8> = (0..n)
        .map(|i| l.rare_owned.data[i] | l.rare_conquest.data.get(i).copied().unwrap_or(0))
        .collect();
    let differs = l.rare.data.iter().zip(or.iter().chain(std::iter::repeat(&0))).any(|(a, b)| a != b);
    let mut rare_note = String::new();
    if differs {
        let borders = l.rare.data.get(2).is_some_and(|b| b & 0x80 != 0) != or.get(2).is_some_and(|b| b & 0x80 != 0);
        l.rare.bits = l.rare_owned.bits;
        l.rare.size = n as i32;
        l.rare.data = or;
        l.flags |= 0xc000000;
        rare_note = format!(
            "; rare := rare_owned | rare_conquest, leader_flags |= 0xc000000, calc_pop_cap 0x006DC490 UNTRANSCRIBED{}",
            if borders { ", Regions::fix_all_borders 0x0067F7D0 UNTRANSCRIBED" } else { "" }
        );
    }
    // 15.
    ld_put_i32(l, LD_GATHER_STAMP, frame);
    if let Some(w) = who_slot {
        slots[w].flags &= !0x2000000;
    }
    effects.push(format!(
        "{prefix}: rare_owned cleared, known_rares = {sum}, rares_collected zeroed, bonus[0..6] = 0, \
         gather_stamp = {frame}, Leaders[who].flags &= ~0x2000000{rare_note}; resources[0..6] recompute \
         (city/oil/building/caravan sums) UNTRANSCRIBED — resources untouched"
    ));
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_matches_retail_phasing() {
        // flag clear: needs gather_stamp + 300 and (frame + who*8) % 256 == 0.
        assert!(!due(16, 0x13, 8, 0));
        assert!(!due(256, 0x13, 0, 0), "256 < 0 + 300");
        assert!(due(512, 0x13, 0, 0));
        assert!(due(504, 0x13, 1, 0), "(504 + 8) % 256 == 0");
        assert!(!due(505, 0x13, 1, 0));
        // flag set: frame 0 or (who + frame) % 8 == 0.
        assert!(due(0, 0x2000007, 8, 0));
        assert!(due(16, 0x2000007, 8, 0));
        assert!(!due(17, 0x2000007, 8, 0));
    }
}
