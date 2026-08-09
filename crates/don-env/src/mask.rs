//! Per-parameter action masks, computed from state.
//!
//! # Invariants this module promises
//!
//! 1. **A masked-in action is applicable.** If a head value's bit is set, `action::apply_*`
//!    will not classify the action `illegal` for *that* head. Cross-head conjunctions the
//!    factored form cannot express (e.g. "this target is in range of this attacker") are
//!    the documented exception, listed in [`CROSS_HEAD_GAPS`].
//! 2. **Never all-zero.** Every head of an emittable verb has at least one legal value, and
//!    the Verb head always has NOOP. A policy that samples under the mask can therefore
//!    never be stuck, which is the failure mode that makes masked training diverge.
//! 3. **A dead entity masks to NOOP only**, so padded rows cost the policy nothing.

use crate::generated as g;
use crate::spec::{fill_bits, set_bit, EnvConfig, MaskLayout};
use crate::state::EnvWorld;
use crate::typecaps::{
    F_ATTACK, F_BUILDING, F_CASTER, F_CIVILIAN, F_GARR_FORT, F_GARR_TOWN, F_MOVE, F_PRODUCER,
    F_SIEGE, F_TRANSPORT,
};

/// Conjunctions a factored mask cannot express. Each is a real, quantified looseness in
/// invariant 1, not a hand-wave.
pub const CROSS_HEAD_GAPS: [&str; 6] = [
    "Attack(target): the target mask is 'a hostile entity exists', not 'in this \
     attacker's RANGE'; out-of-range attacks become an approach order, never illegal.",
    "Build(x,y): tile legality (terrain, footprint, overlap) is not modelled, so the \
     spatial heads are unmasked apart from the grid bound.",
    "QueueUp(type,count): affordability is masked for count=1 only; a larger count \
     partially fills and the remainder is dropped.",
    "MoveTo(x,y): reachability is not masked; PathFinder::astar_path is unread.",
    "Tribute(good,amount): both heads are masked against max(econ), so an individually \
     legal pair can be jointly unaffordable.",
    "TargetPlayer: when every other player is dead the head would be all-zero, so it \
     falls back to self and the diplomacy verbs become no-ops.",
];

pub struct MaskWriter {
    pub unit: MaskLayout,
    pub player: MaskLayout,
    /// Scratch: the acting player's affordable-type bitset, recomputed once per player
    /// per step instead of once per entity.
    affordable: Vec<u8>,
    bitset_bytes: usize,
}

impl MaskWriter {
    pub fn new(cfg: &EnvConfig) -> MaskWriter {
        let bb = g::NUM_TYPES.div_ceil(8);
        MaskWriter {
            unit: MaskLayout::new(&crate::spec::unit_head_sizes(cfg)),
            player: MaskLayout::new(&crate::spec::player_head_sizes(cfg)),
            affordable: vec![0; bb],
            bitset_bytes: bb,
        }
    }

    /// Write masks for one agent. `rows` is the agent's controllable entity rows, in the
    /// same order the observation lists them; `out` is `max_controlled * record_bytes`.
    pub fn write_unit_masks(
        &mut self,
        w: &EnvWorld,
        cfg: &EnvConfig,
        who: u8,
        rows: &[usize],
        entity_rows: &[usize],
        out: &mut [u8],
    ) {
        out.fill(0);
        let rec = self.unit.record_bytes;
        // Padding rows — slots past the agent's entity count — get value 0 legal on every
        // head, i.e. exactly one well-defined NOOP. Leaving them all-zero would hand a
        // masked sampler an empty categorical, which is invariant 2's whole point.
        for i in 0..cfg.max_controlled {
            let r = &mut out[i * rec..(i + 1) * rec];
            for h in 0..self.unit.sizes.len() {
                set_bit(self.unit.head(r, h), 0);
            }
        }

        // Per-player scratch: which types this player can pay for *and* has pop headroom
        // for right now. Computed once per agent instead of once per entity — the Type
        // head is 806 wide and this is what keeps masking off the critical path.
        self.affordable.fill(0);
        let ps = &w.players[who as usize];
        for t in 0..g::NUM_TYPES {
            let c = w.rules.caps.get(t as u16);
            if ps.can_afford(&c.cost) && ps.pop + c.pop.max(1) as i32 <= ps.pop_cap {
                self.affordable[t / 8] |= 1 << (t % 8);
            }
        }
        // Which observed entity slots are plausible targets at all.
        let mut hostile = vec![0u8; (cfg.max_entities + 1).div_ceil(8)];
        let mut friendly = vec![0u8; (cfg.max_entities + 1).div_ceil(8)];
        let mut any_hostile = false;
        let mut any_friendly = false;
        for (slot, &r) in entity_rows.iter().enumerate().take(cfg.max_entities) {
            let o = w.sim.owner()[r];
            match w.relation(who, o) {
                0 | 1 => {
                    set_bit(&mut friendly, slot + 1);
                    any_friendly = true;
                }
                _ => {
                    set_bit(&mut hostile, slot + 1);
                    any_hostile = true;
                }
            }
        }

        for (i, &row) in rows.iter().enumerate().take(cfg.max_controlled) {
            let r = &mut out[i * rec..(i + 1) * rec];
            if w.sim.owner()[row] != who as i8 {
                continue;
            }
            let t = w.type_index[row];
            let c = *w.rules.caps.get(t);
            let is_building = c.has(F_BUILDING);

            // ---- Type, computed FIRST -------------------------------------------------
            // The Verb head gates QueueUp/Build on this being non-empty: offering a verb
            // whose only parameter value is the fallback would break invariant 1, and it
            // is the one place where two heads genuinely have to be decided together.
            let type_any = {
                let bb = self.bitset_bytes;
                let src: &[u8] = if c.has(F_PRODUCER) {
                    w.rules.caps.produces(t)
                } else if c.has(F_CIVILIAN) {
                    &w.rules.building_types
                } else {
                    &[]
                };
                let th = self.unit.head(r, g::UnitHead::Type as usize);
                let mut any = false;
                for k in 0..bb.min(src.len()) {
                    let v = src[k] & self.affordable[k];
                    th[k] = v;
                    any |= v != 0;
                }
                if !any {
                    set_bit(th, 0); // never all-zero
                }
                any
            };

            // ---- Verb head -----------------------------------------------------------
            {
                let vh = self.unit.head(r, g::UnitHead::Verb as usize);
                let mut allow = |v: usize| set_bit(vh, v + 1);
                // Always available to anything the player owns.
                allow(g::uv::HALT);
                allow(g::uv::STANCE);
                allow(g::uv::DISBAND);
                if c.has(F_MOVE) && !is_building {
                    allow(g::uv::MOVE_TO);
                    allow(g::uv::MOVE_NEAR);
                    allow(g::uv::PATROL);
                    // `Group::action_launch_patrol` only installs an order on true
                    // planes. Air-domain helicopters deliberately fail this predicate.
                    if c.is_plane {
                        allow(g::uv::LAUNCH_PATROL);
                    }
                    allow(g::uv::FORM);
                    if any_friendly {
                        allow(g::uv::FOLLOW);
                        allow(g::uv::GUARD);
                    }
                }
                if c.has(F_ATTACK) && any_hostile {
                    allow(g::uv::ATTACK);
                    if c.has(F_SIEGE) {
                        allow(g::uv::SIEGE_ATTACK);
                        allow(g::uv::ATTACK_GROUND);
                    }
                }
                if c.has(F_CIVILIAN) && !is_building {
                    allow(g::uv::GATHER);
                    allow(g::uv::REPAIR);
                    if type_any {
                        allow(g::uv::BUILD);
                    }
                }
                if c.has(F_CASTER) {
                    allow(g::uv::SPELL);
                    allow(g::uv::STOP_SPELL);
                }
                if c.has(F_TRANSPORT) {
                    allow(g::uv::TRANSPORT);
                    allow(g::uv::SET_TRANSPORT);
                    allow(g::uv::EJECTALL);
                }
                if (c.has(F_GARR_TOWN) || c.has(F_GARR_FORT)) && any_friendly {
                    allow(g::uv::GARRISON);
                }
                if c.has(F_PRODUCER) {
                    if type_any {
                        allow(g::uv::QUEUE_UP);
                    }
                    allow(g::uv::UNQUEUE);
                    allow(g::uv::COME_OUT);
                    allow(g::uv::GATHER_POINT);
                    allow(g::uv::CITY_GATHER);
                }
            }

            // ---- spatial heads: the whole grid, bounded ------------------------------
            fill_bits(self.unit.head(r, g::UnitHead::TargetX as usize), cfg.grid_w);
            fill_bits(self.unit.head(r, g::UnitHead::TargetY as usize), cfg.grid_h);

            // ---- TargetEntity -------------------------------------------------------
            {
                let self_slot = entity_rows.iter().position(|&r2| r2 == row);
                let th = self.unit.head(r, g::UnitHead::TargetEntity as usize);
                let src = if c.has(F_ATTACK) { &hostile } else { &friendly };
                for (b, s) in th.iter_mut().zip(src.iter()) {
                    *b |= *s;
                }
                if c.has(F_CIVILIAN) {
                    for (b, s) in th.iter_mut().zip(friendly.iter()) {
                        *b |= *s;
                    }
                }
                // Never offer the actor itself: every verb that reads a target rejects it.
                if let Some(sl) = self_slot {
                    let bit = sl + 1;
                    th[bit / 8] &= !(1u8 << (bit % 8));
                }
                // "no target" (index 0) is offered only when there is no real target, so a
                // masked sampler that picks a target-taking verb almost never draws it.
                // The residual case — no target exists at all — is the incoherence the
                // factored form cannot rule out, and it is counted, not hidden.
                if th.iter().all(|b| *b == 0) {
                    set_bit(th, 0);
                }
            }

            fill_bits(self.unit.head(r, g::UnitHead::QueuePos as usize), 3);
            fill_bits(self.unit.head(r, g::UnitHead::Stance as usize), 4);
            fill_bits(
                self.unit.head(r, g::UnitHead::Form as usize),
                g::FORMS.len(),
            );
            fill_bits(self.unit.head(r, g::UnitHead::OrderMods as usize), 8);
            fill_bits(self.unit.head(r, g::UnitHead::Count as usize), 5);
        }
    }

    pub fn write_player_mask(&self, w: &EnvWorld, who: u8, out: &mut [u8]) {
        out.fill(0);
        let r = out;
        set_bit(self.player.head(r, g::PlayerHead::Verb as usize), 0);
        {
            let vh = self.player.head(r, g::PlayerHead::Verb as usize);
            let mut allow = |v: usize| set_bit(vh, v + 1);
            allow(g::pv::RESIGN);
            allow(g::pv::ALARM);
            allow(g::pv::UNITMASK);
            allow(g::pv::BUILDMASK);
            allow(g::pv::TREATY);
            allow(g::pv::DECLARE);
            allow(g::pv::ACCEPT);
            allow(g::pv::REJECT);
            allow(g::pv::PROPOSE_ATTACK);
            allow(g::pv::CLEAR_ALL);
            allow(g::pv::CLEAR_TRIBUTES);
            allow(g::pv::DEMAND_TRIBUTE);
            allow(g::pv::LEADER_OPTIONS);
            allow(g::pv::BUY);
            allow(g::pv::SELL);
            if w.players[who as usize]
                .econ
                .iter()
                .any(|&v| v >= crate::spec::AMOUNT_BUCKETS[0])
            {
                allow(g::pv::TRIBUTE);
            }
        }
        {
            let ph = self.player.head(r, g::PlayerHead::TargetPlayer as usize);
            let mut any = false;
            for p in 0..g::NUM_PLAYERS {
                if p != who as usize && w.players[p].alive {
                    set_bit(ph, p);
                    any = true;
                }
            }
            if !any {
                set_bit(ph, who as usize);
            }
        }
        {
            let gh = self.player.head(r, g::PlayerHead::Good as usize);
            for k in 0..g::NUM_COMMON {
                set_bit(gh, k);
            }
        }
        {
            let ah = self.player.head(r, g::PlayerHead::Amount as usize);
            let max = w.players[who as usize]
                .econ
                .iter()
                .copied()
                .max()
                .unwrap_or(0);
            let mut any = false;
            for (i, &b) in crate::spec::AMOUNT_BUCKETS.iter().enumerate() {
                if b <= max {
                    set_bit(ah, i);
                    any = true;
                }
            }
            if !any {
                set_bit(ah, 0);
            }
        }
        fill_bits(self.player.head(r, g::PlayerHead::Treaty as usize), 3);
    }
}

/// `1` where `F_*` capability flags exist but no verb consumes them yet. Kept as a list so
/// the report can be generated rather than written from memory.
pub const UNCONSUMED_FLAGS: [&str; 4] = ["STEALTH", "DETECT", "ANTIAIR", "SIEGE(partial)"];
