//! What a player can see — the only thing a bot is allowed to decide from.
//!
//! # Why a snapshot and not a `&World`
//!
//! Two reasons, and the second is the load-bearing one.
//!
//! 1. **Fog has to bite.** A bot handed `&World` would read the enemy's build order
//!    through the fog, and every scouting decision it made would be theatre. [`Obs`]
//!    contains only what the player's own units and buildings can see plus what they
//!    remember having seen, which is the same information a human has.
//! 2. **A learned policy gets a tensor, not a pointer.** If the heuristic player decides
//!    from something richer than the observation an RL agent receives, then "the heuristic
//!    is the baseline for self-play" is false — the baseline is playing a different game.
//!    Everything in `Obs` is derivable from `don-env`'s own observation planes, so a
//!    policy trained on those has the same inputs this bot has.
//!
//! The entity list is **stable per observation and indexed from 0**, which is exactly the
//! `TargetEntity` head convention (`slot + 1`, `0` = none). [`Obs::slot`] and
//! [`Obs::unslot`] are the two functions [`crate::arena::cmd::Cmd::to_heads`] wants.

use std::collections::BTreeMap;

use super::cmd::EntId;
use super::map::{Map, Terrain};
use super::types::TypeRow;
use super::world::{Ent, Ids, Job, Score, Sighting, World};
use crate::rules::NRES;

/// One of my own entities, flattened.
#[derive(Clone, Debug)]
pub struct MyEnt {
    pub id: EntId,
    pub type_id: i32,
    pub tx: i32,
    pub ty: i32,
    pub hp: i32,
    pub max_hp: i32,
    pub building: bool,
    pub complete: bool,
    /// Remaining own-site builder frames. Zero for complete objects and units.
    pub build_left: i32,
    pub idle: bool,
    pub job: Job,
    pub workers: i32,
    pub worker_cap: i32,
    pub queue_len: usize,
    pub assigned_to: EntId,
    /// Frame this entity last took damage, or -1.
    pub last_damaged: i64,
}

/// One fog-limited snapshot.
pub struct Obs<'a> {
    pub frame: i64,
    pub who: u8,
    pub pi: usize,
    pub stock: [i32; NRES],
    pub techs: Vec<i32>,
    pub age: usize,
    pub pop: i32,
    pub pop_cap: i32,
    pub mine: Vec<MyEnt>,
    /// Enemy entities I can see or remember, newest sighting per id.
    pub known: Vec<Sighting>,
    /// Read-only handles into the shared, non-secret parts of the world.
    pub map: &'a Map,
    pub world: &'a World,
    slots: Vec<EntId>,
    slot_of: BTreeMap<EntId, u16>,
}

impl<'a> Obs<'a> {
    pub fn of(w: &'a World, pi: usize) -> Obs<'a> {
        let p = &w.players[pi];
        let mut mine = Vec::new();
        for e in w.own_ents(pi) {
            mine.push(MyEnt {
                id: e.id,
                type_id: e.type_id,
                tx: e.tile().0,
                ty: e.tile().1,
                hp: e.hits_left(),
                max_hp: e.hp.myhits,
                building: e.building,
                complete: e.complete,
                build_left: e.build_left,
                idle: e.job == Job::Idle,
                job: e.job,
                workers: e.workers,
                worker_cap: e.worker_cap,
                queue_len: e.queue.len(),
                assigned_to: e.assigned_to,
                last_damaged: e.last_damaged,
            });
        }
        let known: Vec<Sighting> = p.memory.values().copied().collect();
        let mut slots: Vec<EntId> = mine.iter().map(|m| m.id).collect();
        slots.extend(known.iter().map(|s| s.id));
        let slot_of = slots
            .iter()
            .enumerate()
            .map(|(i, &id)| (id, i as u16 + 1))
            .collect();
        Obs {
            frame: w.frame,
            who: pi as u8,
            pi,
            stock: p.stock,
            techs: p.techs.iter().copied().collect(),
            age: w.age_of(pi),
            pop: w.pop(pi),
            pop_cap: w.pop_cap(pi),
            mine,
            known,
            map: &w.map,
            world: w,
            slots,
            slot_of,
        }
    }

    /// `don-env`'s `TargetEntity` head value for an entity: its slot + 1, 0 for none.
    pub fn slot(&self, id: EntId) -> u16 {
        self.slot_of.get(&id).copied().unwrap_or(0)
    }
    /// The inverse.
    pub fn unslot(&self, s: u16) -> EntId {
        if s == 0 {
            EntId::NONE
        } else {
            self.slots
                .get(s as usize - 1)
                .copied()
                .unwrap_or(EntId::NONE)
        }
    }

    pub fn ty(&self, type_id: i32) -> Option<&TypeRow> {
        self.world.types.get(type_id)
    }
    /// Stable type ids a policy needs to name commands.
    ///
    /// This copies the public rules vocabulary into the observation facade; a bot does not
    /// need to reach through the facade to the authoritative world just to name `Citizen`
    /// or `Barracks`.
    pub fn ids(&self) -> Ids {
        self.world.ids
    }
    /// Type ids in this player's static nation roster.
    pub fn roster_type_ids(&self) -> Vec<i32> {
        self.world.roster[self.pi]
            .names()
            .map(|(_, type_id)| type_id)
            .collect()
    }
    /// Shipped balance percentage for two public type ids.
    pub fn balance_percent(&self, attacker: i32, defender: i32) -> i32 {
        self.world.balance.get(attacker, defender).unwrap_or(100)
    }
    pub fn has_tech(&self, t: i32) -> bool {
        self.techs.binary_search(&t).is_ok()
    }
    pub fn can_pay(&self, cost: &[i32; NRES]) -> bool {
        (0..NRES).all(|r| self.stock[r] >= cost[r])
    }
    pub fn count(&self, type_id: i32) -> usize {
        self.mine.iter().filter(|m| m.type_id == type_id).count()
    }
    pub fn count_complete(&self, type_id: i32) -> usize {
        self.mine
            .iter()
            .filter(|m| m.type_id == type_id && m.complete)
            .count()
    }
    /// Built + under construction + queued anywhere. The engine's
    /// `num_type_with_queued`, which is what the shipped script counts with.
    pub fn count_with_queued(&self, type_id: i32) -> usize {
        let q: usize = self
            .world
            .own_ents(self.pi)
            .flat_map(|e| e.queue.iter())
            .filter(|q| q.type_id == type_id)
            .count();
        self.count(type_id) + q
    }
    pub fn find(&self, type_id: i32) -> Option<&MyEnt> {
        self.mine
            .iter()
            .find(|m| m.type_id == type_id && m.complete)
    }
    pub fn all(&self, type_id: i32) -> impl Iterator<Item = &MyEnt> {
        self.mine
            .iter()
            .filter(move |m| m.type_id == type_id && m.complete)
    }
    pub fn terrain(&self, tx: i32, ty: i32) -> Terrain {
        self.map.at(tx, ty)
    }
    pub fn explored(&self, tx: i32, ty: i32) -> bool {
        self.world.explored(self.pi, tx, ty)
    }
    /// Whether a tile is in current line of sight.
    ///
    /// This is intentionally narrower than exposing entity liveness. A remembered enemy
    /// outside current sight may have died or moved; a policy may walk to its last known
    /// location, but must not query the authoritative object table to learn which happened.
    pub fn visible(&self, tx: i32, ty: i32) -> bool {
        if tx < 0 || ty < 0 || tx >= self.map.w || ty >= self.map.h {
            return false;
        }
        self.world.players[self.pi].visible[(ty * self.map.w + tx) as usize]
    }
    pub fn score(&self) -> Score {
        self.world.score(self.pi)
    }
    /// The raw entity, for the few queries a snapshot cannot carry cheaply. Only ever
    /// used for **my own** entities; asking for an enemy's returns `None`.
    pub fn own_ent(&self, id: EntId) -> Option<&Ent> {
        self.world.ent(id).filter(|e| e.who as usize == self.pi)
    }
}
