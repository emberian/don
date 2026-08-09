//! The players, and the seam a learned policy slots into.
//!
//! Four bots and one adapter:
//!
//! | bot | what it is |
//! |---|---|
//! | [`boom::ShippedOpening`] | the shipped `economic.bhs` purchase order, cases 6→18, as a player |
//! | [`boom::CapFirst`] | the optimiser-derived opening ([`crate::optimum::player::CapFirst`]'s rules) re-expressed against the arena |
//! | [`marshal::Marshal`] | the strong player: map, scouting, counter-composition, army movement, defence, expansion |
//! | [`ai::Ai`] | the improved-edition opponent: observation-only war economy, active scouting, mixed counters, focus fire, and retreat |
//! | [`HeadPolicy`] | **the drop-in**: a `Bot` whose decisions are `don-env` action-head vectors |
//!
//! Everything a bot may read is in [`Obs`]. Everything it may do is a [`Cmd`]. Those two
//! types are the whole contract, and [`HeadPolicy`] exists to prove the contract is the
//! same one an RL agent has.

pub mod ai;
pub mod boom;
pub mod marshal;

use super::cmd::{Cmd, EntId};
use super::map::Terrain;
use super::obs::{MyEnt, Obs};
use super::world::ring;
use crate::rules::NRES;

/// A player.
pub trait Bot {
    fn name(&self) -> String;
    /// Emit commands. Called every [`Bot::decide_period`] frames.
    fn act(&mut self, obs: &Obs, out: &mut Vec<Cmd>);
    /// Frames between decisions. This is a *horizon* knob, not a power knob: a bot that
    /// thinks less often reacts later, it does not gather less.
    fn decide_period(&self) -> i64 {
        15
    }
}

/// A `Bot` driven by `don-env`'s ten action heads.
///
/// This is the drop-in seam made literal. `f` receives the same [`Obs`] every heuristic
/// bot receives and returns `(actor, [i32; 10])` pairs — exactly the shape a policy
/// network emits per controlled entity in `don-env`'s `MultiDiscrete` space. The adapter
/// decodes them with [`Cmd::from_heads`], so a trained model replaces
/// [`marshal::Marshal`] with no other change anywhere.
pub struct HeadPolicy<F> {
    pub label: String,
    pub f: F,
}

impl<F> Bot for HeadPolicy<F>
where
    F: FnMut(&Obs) -> Vec<(EntId, [i32; 10])>,
{
    fn name(&self) -> String {
        self.label.clone()
    }
    fn act(&mut self, obs: &Obs, out: &mut Vec<Cmd>) {
        for (actor, heads) in (self.f)(obs) {
            if let Some(c) = Cmd::from_heads(actor, &heads, |s| obs.unslot(s)) {
                out.push(c);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Shared economic machinery. Both openings and the strong player use it, so the
// head-to-head measures *decisions*, not who wrote a better worker loop.
// ---------------------------------------------------------------------------

/// How many workers on resource `res` still earn anything, given the age's
/// `COMMERCE_CAP`.
///
/// This is the arena's [`crate::optimum::econ::World::useful_slots`]: income is clamped
/// at `COMMERCE_CAP` per resource, cities contribute `CITY_GATHER` for free, and each
/// seated citizen contributes `PEASANT_RATE`. Past the clamp a citizen is 20 food of
/// pure loss — rule 2 of the optimiser-derived player, and it is *true in the arena too*
/// because the arena runs the same derived `resource_tick`.
pub fn useful_slots(obs: &Obs, res: usize) -> i64 {
    let c = &obs.world.types.constants;
    let cap = don_sim::mechanics::commerce_cap(
        obs.age.min(7),
        res.min(NRES - 1),
        &obs.world.econ,
        &don_sim::mechanics::CommerceCapGates::default(),
        0,
    ) as i64;
    let cities = obs.count_complete(obs.world.ids.small_city) as i64;
    let free = c.city_gather[res] as i64 * cities;
    let rate = c.peasant_rate.max(1) as i64;
    ((cap - free) / rate).max(0)
}

/// Total gather seats on a resource, **including buildings still under construction**.
///
/// Counting only finished ones was a real bug and it is worth naming: a bot that decides
/// "seats < workers, place a Farm" every tick places one Farm per tick for the whole
/// 150-frame `JOB_TIME`, because the seat it just bought does not exist yet. The arena
/// sets `worker_cap` at placement, from the terrain, so the pending capacity is knowable.
pub fn seats(obs: &Obs, res: usize) -> i64 {
    obs.mine
        .iter()
        .filter(|m| m.building && m.worker_cap > 0)
        .filter(|m| {
            obs.world
                .ent(m.id)
                .map(|e| e.gather_res == res)
                .unwrap_or(false)
        })
        .map(|m| m.worker_cap as i64)
        .sum()
}

/// Citizens with nothing to do.
///
/// Strictly `Job::Idle`: a citizen walking to a seat (`Gather`), working a site (`Work`)
/// or sent somewhere (`MoveTo`, which is how a scout is expressed) is **not** idle.
/// Treating a walking citizen as idle makes `employ` re-task it every tick, which silently
/// disables scouting — it did, and that is why this is spelled out.
pub fn idle_citizens<'a>(obs: &'a Obs) -> Vec<&'a MyEnt> {
    let cit = obs.world.ids.citizen;
    obs.mine
        .iter()
        .filter(|m| m.type_id == cit && matches!(m.job, super::world::Job::Idle))
        .collect()
}

/// Put every unseated citizen to work: first any unfinished building, then any gather
/// slot with room, nearest first. Returns the number of commands emitted.
pub fn employ(obs: &Obs, out: &mut Vec<Cmd>) -> usize {
    employ_except(obs, &[], out)
}

/// [`employ`], leaving `skip` alone — for a citizen the caller has other plans for.
pub fn employ_except(obs: &Obs, skip: &[EntId], out: &mut Vec<Cmd>) -> usize {
    let cit = obs.world.ids.citizen;
    let mut sites: Vec<(EntId, i32, i32)> = obs
        .mine
        .iter()
        .filter(|m| m.building && !m.complete)
        .map(|m| (m.id, m.tx, m.ty))
        .collect();
    // Free seats, as (id, tx, ty, remaining).
    let mut free: Vec<(EntId, i32, i32, i32)> = obs
        .mine
        .iter()
        .filter(|m| m.building && m.complete && m.worker_cap > m.workers)
        .map(|m| (m.id, m.tx, m.ty, m.worker_cap - m.workers))
        .collect();
    // Account for citizens already walking to a seat so the same slot is not double-booked.
    for m in obs.mine.iter().filter(|m| m.type_id == cit) {
        if let super::world::Job::Gather { target } = m.job {
            if let Some(s) = free.iter_mut().find(|s| s.0 == target) {
                s.3 -= 1;
            }
        }
    }
    free.retain(|s| s.3 > 0);

    let mut n = 0;
    for c in idle_citizens(obs) {
        if skip.contains(&c.id) {
            continue;
        }
        // Builders first: an unfinished Woodcutter's Camp earns nothing.
        if let Some(k) = nearest(&sites, c.tx, c.ty, |s| (s.1, s.2)) {
            out.push(Cmd::Work {
                unit: c.id,
                target: sites[k].0,
            });
            // A site can take several builders; leave it in the list.
            if sites.len() > 3 {
                sites.remove(k);
            }
            n += 1;
            continue;
        }
        if let Some(k) = nearest(&free, c.tx, c.ty, |s| (s.1, s.2)) {
            out.push(Cmd::Gather {
                unit: c.id,
                target: free[k].0,
            });
            free[k].3 -= 1;
            if free[k].3 <= 0 {
                free.remove(k);
            }
            n += 1;
        }
    }
    n
}

fn nearest<T>(v: &[T], tx: i32, ty: i32, pos: impl Fn(&T) -> (i32, i32)) -> Option<usize> {
    v.iter()
        .enumerate()
        .min_by_key(|(_, t)| {
            let (x, y) = pos(t);
            (x - tx).abs().max((y - ty).abs())
        })
        .map(|(i, _)| i)
}

/// A legal site for `type_id`, spiralling out from `(ax, ay)`.
///
/// Returns the first tile [`crate::arena::world::World::placement_ok`] accepts, which is
/// the arena's whole placement rule — the same predicate `submit` will re-check, so a bot
/// cannot place somewhere the world would refuse.
pub fn site_near(obs: &Obs, type_id: i32, ax: i32, ay: i32, max_r: i32) -> Option<(i32, i32)> {
    for r in 2..max_r {
        for (dx, dy) in ring(r) {
            let (x, y) = (ax + dx, ay + dy);
            if obs.world.placement_ok(obs.pi, type_id, x, y).is_ok() {
                return Some((x, y));
            }
        }
    }
    None
}

/// A site for a gatherer that maximises its worker capacity, not merely legality.
///
/// A Woodcutter's Camp with one forest tile beside it holds one citizen; four tiles away
/// there may be a spot holding five. That difference is most of the early economy, and it
/// only exists because the arena has terrain.
pub fn best_gather_site(
    obs: &Obs,
    type_id: i32,
    ax: i32,
    ay: i32,
    max_r: i32,
) -> Option<(i32, i32)> {
    let want = if type_id == obs.world.ids.camp {
        Terrain::Forest
    } else if type_id == obs.world.ids.mine {
        Terrain::Mountain
    } else {
        return site_near(obs, type_id, ax, ay, max_r);
    };
    let mut best: Option<(i64, (i32, i32))> = None;
    for r in 2..max_r {
        for (dx, dy) in ring(r) {
            let (x, y) = (ax + dx, ay + dy);
            if obs.world.placement_ok(obs.pi, type_id, x, y).is_err() {
                continue;
            }
            let cap = obs.map.count_within(x, y, 2, want).min(5) as i64;
            if cap == 0 {
                continue;
            }
            // Capacity first, distance second.
            let score = cap * 1000 - r as i64;
            if best.map_or(true, |(b, _)| score > b) {
                best = Some((score, (x, y)));
            }
        }
        // Stop widening once a full-capacity site is in hand.
        if let Some((s, p)) = best {
            if s / 1000 >= 5 {
                return Some(p);
            }
        }
    }
    best.map(|(_, p)| p)
}

/// My capital: the first complete city, or the first city at all.
pub fn capital<'a, 'w>(obs: &'a Obs<'w>) -> Option<&'a MyEnt> {
    let c = obs.world.ids.small_city;
    obs.mine
        .iter()
        .find(|m| m.type_id == c && m.complete)
        .or_else(|| obs.mine.iter().find(|m| m.type_id == c))
}

/// Whether the player may legally have this type at all: every `PREQ` is a tech it holds,
/// plus the one rule that is not a `PREQ` — a second city needs `City State`.
///
/// Bots must consult this before asking. A `Market` whose `PREQ0` is `Barter` is a
/// perfectly legal *site*, so `placement_ok` says yes and `submit` says `Invalid`; a bot
/// that only checks the site therefore re-requests it every tick forever. That is exactly
/// what happened, and it is what `World::rejects` is for — 166 invalid `BUILD`s in one
/// match, all the same Market.
pub fn legal(obs: &Obs, type_id: i32) -> bool {
    let Some(t) = obs.ty(type_id) else {
        return false;
    };
    if !t.preq.iter().all(|p| obs.has_tech(*p)) {
        return false;
    }
    if type_id == obs.world.ids.small_city
        && !obs.has_tech(obs.world.ids.city_state)
        && obs.count_with_queued(type_id) >= 1
    {
        return false;
    }
    true
}

/// Queue `type_id` at a producer of the right `WHERE`, if one exists and it is affordable.
pub fn queue_at(obs: &Obs, type_id: i32, count: u16, out: &mut Vec<Cmd>) -> bool {
    let Some(t) = obs.ty(type_id) else {
        return false;
    };
    if !legal(obs, type_id) || !obs.can_pay(&t.cost) {
        return false;
    }
    // A tech already held or already in a queue is an `Invalid`, not a retry.
    let is_tech = !t.kind_unit && !t.kind_building;
    if is_tech
        && (obs.has_tech(type_id)
            || obs
                .world
                .own_ents(obs.pi)
                .flat_map(|e| e.queue.iter())
                .any(|q| q.type_id == type_id))
    {
        return false;
    }
    let where_ = t.where_;
    let producer = obs
        .mine
        .iter()
        .filter(|m| m.building && m.complete && (where_ < 0 || m.type_id == where_))
        .min_by_key(|m| m.queue_len);
    match producer {
        Some(p) => {
            out.push(Cmd::Queue {
                producer: p.id,
                type_id,
                count,
            });
            true
        }
        None => false,
    }
}

/// The next unresearched tech in a preference list whose prerequisites are all held.
pub fn next_tech(obs: &Obs, want: &[i32]) -> Option<i32> {
    want.iter().copied().find(|&t| {
        !obs.has_tech(t)
            && obs
                .ty(t)
                .map(|r| r.preq.iter().all(|p| obs.has_tech(*p)))
                .unwrap_or(false)
    })
}
