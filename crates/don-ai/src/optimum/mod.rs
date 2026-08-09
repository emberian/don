//! **The optimiser-derived opening player**, and the measured Ancient-age economy it
//! plays in.
//!
//! This module is the analytics lane's half of `don-ai`. The rest of the crate
//! ([`crate::economic`], [`crate::library`], [`crate::abi`], [`crate::api`]) is the
//! **transcription of the shipped production AI**; nothing here transcribes anything.
//! What lives here is:
//!
//! * [`econ`] — the opening economy in integers, every constant traced to a live read
//!   of the `Constants` singleton or to a named function in the shipped PDB.
//! * [`player::CapFirst`] — a heuristic opening player whose rules are read off the
//!   search in `analysis/opening.py`.
//! * [`player::ShippedBoom`] — the purchase sequence of `economic.bhs` cases 6–18, as
//!   data, so the two openings can be run head to head inside one economy.
//!
//! **Fidelity: C, and the economy is not the whole game.** No part of this has been
//! executed against retail. It models one player's Ancient-age economy with no map,
//! no opponent, no walking and no combat; the omissions and their signs are listed in
//! `docs/tracks/analytics-v2.md` §4. Do not quote a number out of here as a fact
//! about Rise of Nations; quote it as a fact about this model.

pub mod econ;
pub mod player;

pub use econ::{mmss, Assume, World};
pub use player::{head_to_head, CapFirst, Goal, Run, ShippedBoom, SHIPPED_ORDER};

#[cfg(test)]
mod tests {
    use super::econ::*;
    use super::player::*;

    /// The accumulator is the engine's, not a rate multiplication: over exactly
    /// `gather_rate` frames a player banks `income/16` resources, remainder carried in
    /// `LeaderDataEncrypt::leftover`.
    #[test]
    fn accumulator_matches_the_engine_identity() {
        let mut w = World::new(Assume::default());
        let inc = w.income16();
        let before = w.stock;
        w.advance(GATHER_RATE);
        for r in 0..6 {
            assert_eq!(w.stock[r] - before[r], inc[r] / 16, "resource {r}");
        }
    }

    /// A default start is 5 citizens on 3 farms + 1 camp, i.e. 3 on food and 2 on
    /// timber, giving 30+10 food and 20+10 timber per 30 s. Both are under the
    /// Commerce-0 clamp of 70, so nothing is thrown away yet.
    #[test]
    fn opening_rate_is_under_the_clamp() {
        let w = World::new(Assume::default());
        assert_eq!(w.on[FOOD], 3);
        assert_eq!(w.on[TIMBER], 2);
        assert_eq!(w.rate()[FOOD], 40);
        assert_eq!(w.rate()[TIMBER], 30);
        assert!(w.rate()[FOOD] < COMMERCE_CAP[0]);
    }

    /// The measured clamp, stated as a test: at Commerce 0 with one city the seventh
    /// food gatherer produces nothing at all.
    #[test]
    fn seventh_food_gatherer_is_worth_nothing_at_commerce_zero() {
        let mut w = World::new(Assume::default());
        w.counts[CATALOGUE.iter().position(|i| i.name == "Farm").unwrap()] = 12;
        w.counts[CATALOGUE.iter().position(|i| i.name == "Citizen").unwrap()] = 12;
        w.reseat();
        let mut last = 0;
        let mut deltas = Vec::new();
        for n in 1..=8 {
            w.on = [0; 6];
            w.on[FOOD] = n;
            let r = w.rate()[FOOD];
            deltas.push(r - last);
            last = r;
        }
        // The first step is 20 because the city itself pays 10 food for free; then 10
        // per worker up to the cap of 70, then nothing at all.
        assert_eq!(deltas, vec![20, 10, 10, 10, 10, 10, 0, 0]);
    }

    /// Raising the Commerce level raises the ceiling, which is the whole point of the
    /// optimiser-derived player's first rule.
    #[test]
    fn barter_moves_the_ceiling_by_thirty() {
        let mut w = World::new(Assume::default());
        assert_eq!(w.cap16()[FOOD] / 16, 70);
        w.epoch[COMMERCE] = 1;
        assert_eq!(w.cap16()[FOOD] / 16, 100);
    }

    /// Both players must reach the same objective, and the run must be deterministic.
    #[test]
    fn head_to_head_is_deterministic_and_both_finish() {
        let (a1, b1) = head_to_head(Assume::default(), Goal::default());
        let (a2, b2) = head_to_head(Assume::default(), Goal::default());
        assert_eq!(a1.frames, a2.frames);
        assert_eq!(b1.frames, b2.frames);
        assert!(!a1.stalled, "CapFirst stalled");
        assert!(!b1.stalled, "ShippedBoom stalled");
    }

    /// The headline, as a regression test. If this number moves, the report is stale.
    #[test]
    fn cap_first_beats_the_shipped_order() {
        let (opt, ship) = head_to_head(Assume::default(), Goal::default());
        eprintln!(
            "CapFirst {} ({} frames)  vs  economic.bhs {} ({} frames)",
            mmss(opt.frames),
            opt.frames,
            mmss(ship.frames),
            ship.frames
        );
        assert!(
            opt.frames < ship.frames,
            "optimiser-derived player did not win: {} vs {}",
            opt.frames,
            ship.frames
        );
    }
}
