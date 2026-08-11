//! `Ai`: the improved-edition, non-cheating arena player.
//!
//! The policy is not a difficulty multiplier over Marshal. It receives no income, vision, map,
//! production, or combat advantage. Its edge has to come from decisions made through
//! [`Obs`]: a clamp-aware economy, active scouting, an observed counter mix,
//! concentrated attacks, and retreats based on surviving army value and local opposition.
//!
//! The source-level boundary is intentional: this module never reaches through `Obs` to
//! the arena `World`. Static rules are read with `Obs::ids` / `Obs::ty`, own state comes
//! from `Obs::mine`, and enemy state comes only from current or remembered `Obs::known`
//! sightings. A remembered target outside current visibility is a location to investigate,
//! not proof that an entity is still alive.

use std::collections::BTreeMap;

use super::boom::place_except;
use super::marshal::{buildable_military, counter_pick, enemy_army};
use super::{capital, employ_except, next_tech, queue_at, seats, useful_slots, Bot};
use crate::arena::cmd::{Cmd, EntId};
use crate::arena::gather_upgrades;
use crate::arena::knowledge_economy;
use crate::arena::obs::{MyEnt, Obs};
use crate::arena::world::{Job, FPS};
use don_sim::systems::production::ProdRules;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Massing,
    Pushing,
    Retreating,
    Defending,
}

/// Improved-edition strategy state. Every belief here was derived from an earlier `Obs`.
pub struct Ai {
    mode: Mode,
    scout: Option<EntId>,
    scout_goal: Option<(i32, i32)>,
    scout_leg: usize,
    enemy_base: Option<(i32, i32)>,
    threat: Option<(i32, i32)>,
    mode_until: i64,
    push_start_value: i64,
    push_start_health: i64,
    pushes: u32,
    composition_turn: usize,
    /// Highest observed count per enemy type. This is memory, not hidden live state.
    observed_mix: BTreeMap<i32, i64>,
    builders: Vec<EntId>,
    /// Public owner-local founder/tile reserved while an early Market is placed.
    reserved_university_site: Option<(EntId, (i32, i32))>,
}

impl Default for Ai {
    fn default() -> Self {
        Ai {
            mode: Mode::Massing,
            scout: None,
            scout_goal: None,
            scout_leg: 0,
            enemy_base: None,
            threat: None,
            mode_until: 0,
            push_start_value: 0,
            push_start_health: 0,
            pushes: 0,
            composition_turn: 0,
            observed_mix: BTreeMap::new(),
            builders: Vec::new(),
            reserved_university_site: None,
        }
    }
}

impl Bot for Ai {
    fn name(&self) -> String {
        "Ai".into()
    }

    fn decide_period(&self) -> i64 {
        FPS // one decision per game-second, the same horizon as Marshal
    }

    fn act(&mut self, obs: &Obs, out: &mut Vec<Cmd>) {
        self.reserve_builders(obs);
        self.update_beliefs(obs);
        // Establish the scout reservation before any placement can choose from the same
        // idle snapshot.
        self.scout(obs, out);
        self.economy(obs, out);
        self.staff_critical_site(obs, out);
        self.train_army(obs, out);
        self.command_army(obs, out);
        let mut skip = self.builders.clone();
        skip.extend(self.scout);
        // Commands issued from this observation are not reflected in `m.job` yet. Protect
        // every explicit actor from generic employment, or a same-tick Gather/Work could
        // overwrite a new Build, scout waypoint, or civilian retreat.
        skip.extend(out.iter().map(Cmd::actor));
        employ_except(obs, &skip, out);
    }
}

impl Ai {
    fn reserve_builders(&mut self, obs: &Obs) {
        let citizen = obs.ids().citizen;
        self.builders.retain(|id| {
            obs.mine.iter().any(|m| {
                m.id == *id && m.type_id == citizen && !matches!(m.job, Job::Gather { .. })
            })
        });
        while self.builders.len() < 1 {
            let next = obs
                .mine
                .iter()
                .filter(|m| m.type_id == citizen)
                .filter(|m| !matches!(m.job, Job::Gather { .. }))
                .filter(|m| !self.builders.contains(&m.id) && Some(m.id) != self.scout)
                .min_by_key(|m| (!m.idle, m.id));
            let Some(next) = next else { break };
            self.builders.push(next.id);
        }
    }

    fn update_beliefs(&mut self, obs: &Obs) {
        let ids = obs.ids();
        if let Some(city) = obs
            .known
            .iter()
            .filter(|s| s.building && s.type_id == ids.small_city)
            .max_by_key(|s| s.frame)
            .or_else(|| {
                obs.known
                    .iter()
                    .filter(|s| s.building)
                    .max_by_key(|s| s.frame)
            })
        {
            self.enemy_base = Some((city.tx, city.ty));
        }

        let current = enemy_army(obs);
        for (type_id, count) in current {
            self.observed_mix
                .entry(type_id)
                .and_modify(|old| *old = (*old).max(count))
                .or_insert(count);
        }

        // Damage to our own snapshot is the only attack alarm. Prefer the most recently
        // hit low-HP object so the response goes to the part of the economy in danger.
        if let Some(hurt) = obs
            .mine
            .iter()
            // Front-line units taking damage is expected contact, not a reason to abort a
            // push. Only attacks on the economy/base trigger the defence state.
            .filter(|m| m.building || m.type_id == ids.citizen)
            .filter(|m| m.last_damaged >= 0 && obs.frame - m.last_damaged <= 5 * FPS)
            .min_by_key(|m| (m.hp * 100 / m.max_hp.max(1), -m.last_damaged))
        {
            self.threat = Some((hurt.tx, hurt.ty));
            self.mode = Mode::Defending;
            self.mode_until = obs.frame + 16 * FPS;
        } else if self.mode == Mode::Defending && obs.frame >= self.mode_until {
            self.mode = Mode::Massing;
        }
    }

    /// Military readiness is brought forward without abandoning the commerce clamp.
    fn economy(&mut self, obs: &Obs, out: &mut Vec<Cmd>) {
        let ids = obs.ids();
        let skip: Vec<EntId> = self.scout.into_iter().collect();

        // City State and Written Word retain the prior opening. Barter has no prerequisite,
        // so one Market can establish the only base renewable wealth source while the
        // Classical food bank refills. Optional construction remains reserved through the
        // already represented University site. Mathematics then follows from ordinary
        // starting stock plus tax income, independently of University/Scholar income.
        let barter_before_age =
            obs.ty(ids.barter)
                .zip(obs.ty(ids.university))
                .is_some_and(|(barter, university)| {
                    let timber = don_sim::systems::economy::RES_TIMBER;
                    obs.stock[timber] >= barter.cost[timber] + university.cost[timber]
                });
        let tech_order = if barter_before_age {
            [
                ids.city_state,
                ids.written_word,
                ids.barter,
                ids.classical_age,
                gather_upgrades::MATHEMATICS_TYPE,
                ids.art_of_war,
                gather_upgrades::CHEMISTRY_TYPE,
                gather_upgrades::CARPENTRY_TYPE,
                gather_upgrades::AGRICULTURE_TYPE,
            ]
        } else {
            [
                ids.city_state,
                ids.written_word,
                ids.classical_age,
                ids.barter,
                gather_upgrades::MATHEMATICS_TYPE,
                ids.art_of_war,
                gather_upgrades::CHEMISTRY_TYPE,
                gather_upgrades::CARPENTRY_TYPE,
                gather_upgrades::AGRICULTURE_TYPE,
            ]
        };
        if let Some(tech) = next_tech(obs, &tech_order) {
            // Preserve the already represented knowledge tranche before Mathematics can
            // spend its wealth bank. This is purchase sequencing only: Mathematics still
            // consumes starting knowledge and Market tax, never Scholar income.
            let university_site_pending = tech == gather_upgrades::MATHEMATICS_TYPE
                || (tech == ids.barter && obs.has_tech(ids.classical_age));
            let university_site_pending =
                university_site_pending && obs.count_with_queued(ids.university) < 1;
            if !university_site_pending {
                queue_at(obs, tech, 1, out);
            }
        }

        let age_pending = !obs.has_tech(ids.classical_age)
            && (obs.count_with_queued(ids.market) > 0
                || (next_tech(obs, &[ids.classical_age]) == Some(ids.classical_age)
                    && obs
                        .ty(ids.classical_age)
                        .map(|t| obs.stock[0] * 10 >= t.cost[0] * 6)
                        .unwrap_or(false)));
        // Once City State lands, grow a minimum viable gather base, then stop spending
        // the food bank until the Barracks prerequisite completes. Locking immediately
        // with only the opening workers saved nominal stock but took four minutes to
        // gather the tech; never locking let repeated citizen queues starve it entirely.
        let war_pending = !obs.has_tech(ids.art_of_war)
            && next_tech(obs, &[ids.art_of_war]) == Some(ids.art_of_war);
        let critical_economy_pending =
            obs.count_with_queued(ids.market) < 1 || obs.count_with_queued(ids.university) < 1;
        let food_gap = useful_slots(obs, 0) - seats(obs, 0);
        let wood_gap = useful_slots(obs, 1) - seats(obs, 1);
        let barracks_goal = if obs.age >= 1 || self.mode == Mode::Defending {
            2
        } else {
            1
        };
        let barracks_incomplete = obs
            .mine
            .iter()
            .any(|m| m.type_id == ids.barracks && !m.complete);
        let placed_market = !barracks_incomplete
            && obs.has_tech(ids.barter)
            && obs.count_with_queued(ids.market) < 1
            && self.place_adjacent_city_building(obs, ids.market, out);
        let placed_university = !placed_market
            && !barracks_incomplete
            && obs.has_tech(ids.classical_age)
            && obs.count_with_queued(ids.university) < 1
            && self.place_adjacent_city_building(obs, ids.university, out);

        let mut wants = Vec::new();
        if obs.has_tech(ids.art_of_war) && obs.count_with_queued(ids.barracks) < barracks_goal {
            wants.push(ids.barracks);
        }
        // The next measured zero-coverage surface is the base gather-enhancer trio.
        // Their exact identities, costs and gates were validated when World was created;
        // affordability and placement still come solely from this public observation.
        if obs.has_tech(gather_upgrades::MATHEMATICS_TYPE) {
            if obs.count_with_queued(gather_upgrades::GRANARY_TYPE) < 1 {
                wants.push(gather_upgrades::GRANARY_TYPE);
            }
            if obs.count_with_queued(gather_upgrades::LUMBER_MILL_TYPE) < 1 {
                wants.push(gather_upgrades::LUMBER_MILL_TYPE);
            }
        }
        if obs.has_tech(gather_upgrades::CHEMISTRY_TYPE)
            && obs.count_with_queued(gather_upgrades::SMELTER_TYPE) < 1
        {
            wants.push(gather_upgrades::SMELTER_TYPE);
        }
        if !barracks_incomplete
            && self.mode == Mode::Defending
            && obs.count_with_queued(ids.tower) < 2
        {
            wants.push(ids.tower);
        }
        if !barracks_incomplete
            && obs.has_tech(ids.city_state)
            && self.mode != Mode::Defending
            && obs.count_with_queued(ids.small_city) < 2
            && obs.count_complete(ids.barracks) > 0
            && !age_pending
        {
            wants.push(ids.small_city);
        }
        if !barracks_incomplete
            && !critical_economy_pending
            && wood_gap > 0
            && obs.count_with_queued(ids.camp) < 4
        {
            wants.push(ids.camp);
        }
        if !barracks_incomplete
            && !critical_economy_pending
            && food_gap > 0
            && obs.count_with_queued(ids.farm) < 10
            && !age_pending
        {
            wants.push(ids.farm);
        }
        if !barracks_incomplete
            && !critical_economy_pending
            && obs.has_tech(ids.classical_age)
            && useful_slots(obs, 4) > seats(obs, 4)
            && obs.count_with_queued(ids.mine) < 3
        {
            wants.push(ids.mine);
        }
        if !placed_market && !placed_university {
            for type_id in wants {
                if place_except(obs, type_id, &skip, out) {
                    break;
                }
            }
        }

        // Scholars are tribe-roster Units held inside the completed University. Resolve
        // the grafted Korean row from the observation instead of hardcoding generic 52.
        let scholar = obs.roster_type_ids().into_iter().find(|&type_id| {
            obs.ty(type_id)
                .is_some_and(|t| t.kind_unit && t.name == "Scholar" && t.where_ == ids.university)
        });
        if let Some(scholar) = scholar {
            let count = obs.count_with_queued(scholar) as i32;
            // Preserve one early knowledge decision per completed University, then let
            // Market tax fund Mathematics and Chemistry before additional Scholar ramp
            // costs repeatedly consume the same wealth source. This is ordering only;
            // every Scholar and research still pays its exact loaded queue cost.
            let scholar_budget_open = count < 1 || obs.has_tech(gather_upgrades::CHEMISTRY_TYPE);
            if scholar_budget_open && count < don_sim::systems::gathering::MAX_KNOWLEDGE_GATHERERS {
                let t = obs
                    .ty(scholar)
                    .expect("roster Scholar remains in the live table");
                let cost = knowledge_economy::scholar_cost(
                    t.cost,
                    t.support,
                    t.support_cost,
                    t.progression,
                    count,
                    &ProdRules::shipped(),
                );
                if obs.can_pay(&cost) {
                    queue_at(obs, scholar, 1, out);
                }
            }
        }

        // Grow to useful seats, plus dedicated builders/scout. During an emergency the
        // replacement floor prevents a raid from permanently collapsing the economy.
        let useful = useful_slots(obs, 0) + useful_slots(obs, 1) + useful_slots(obs, 4);
        // Each visible unfinished site may need a fresh capital-side helper when its
        // founding builder is path-blocked. The bounded buffer pays for real productive
        // work and disappears as sites finish; it is not a permanent income bonus.
        let active_sites = obs
            .mine
            .iter()
            .filter(|m| m.building && !m.complete)
            .count() as i64;
        let target = useful.clamp(12, 20) + 3 + active_sites.min(3);
        let citizens = obs.count_with_queued(ids.citizen) as i64;
        // Establish a contestable field army before resuming full growth. Afterwards one
        // in three economy decisions may buy a citizen; the other two preserve resources
        // for continuous production. This prevents the old six-unit plateau without
        // permanently abandoning economic replacement.
        let second = obs.frame / FPS;
        let military_growth = obs.count_complete(ids.barracks) > 0
            && (army(obs).len() < 12 || second.rem_euclid(3) != 0);
        if !age_pending
            && (!war_pending || citizens < 6)
            && !military_growth
            && citizens < target
            && obs.pop < obs.pop_cap
        {
            queue_at(obs, ids.citizen, 1, out);
        }
    }

    /// Put a critical city building's nearest footprint edge one tile from its founder.
    /// Generic capital-first placement is legal but, on some generated layouts, strands a
    /// construction worker behind the site. This searches the same public placement
    /// predicate from the worker outward and changes no movement or construction rule.
    fn place_adjacent_city_building(
        &mut self,
        obs: &Obs,
        type_id: i32,
        out: &mut Vec<Cmd>,
    ) -> bool {
        let ids = obs.ids();
        let Some(t) = obs.ty(type_id) else {
            return false;
        };
        if !obs.can_pay(&t.cost) || !t.preq.iter().all(|tech| obs.has_tech(*tech)) {
            return false;
        }
        if type_id == ids.university {
            if let Some((worker, site)) = self.reserved_university_site {
                let worker = obs
                    .mine
                    .iter()
                    .find(|ent| {
                        ent.id == worker && matches!(ent.job, Job::Idle | Job::Gather { .. })
                    })
                    .or_else(|| {
                        obs.mine.iter().find(|ent| {
                            ent.type_id == ids.citizen
                                && Some(ent.id) != self.scout
                                && matches!(ent.job, Job::Idle | Job::Gather { .. })
                        })
                    });
                if obs.placement_ok(type_id, site.0, site.1) {
                    let Some(worker) = worker else {
                        return false;
                    };
                    out.push(Cmd::Build {
                        worker: worker.id,
                        type_id,
                        tx: site.0,
                        ty: site.1,
                    });
                    self.reserved_university_site = None;
                    return true;
                }
                self.reserved_university_site = None;
            }
        }
        let founder = self
            .builders
            .iter()
            .filter_map(|id| obs.mine.iter().find(|ent| ent.id == *id))
            .find(|ent| matches!(ent.job, Job::Idle | Job::Gather { .. }))
            .or_else(|| {
                obs.mine.iter().find(|ent| {
                    ent.type_id == ids.citizen
                        && Some(ent.id) != self.scout
                        && matches!(ent.job, Job::Idle | Job::Gather { .. })
                })
            });
        let Some(founder) = founder else {
            return false;
        };
        // An early Market must not occupy the only founder-local University footprint.
        // Both sites come from the same public command-admission boolean; the reservation
        // is policy geometry, not a hidden object or occupancy query.
        let reserved_university = if type_id == ids.market {
            founder_local_site(obs, ids.university, founder.tx, founder.ty, None)
        } else {
            None
        };
        let avoid = if type_id == ids.market {
            let market = obs.ty(ids.market).expect("Market row remains public");
            let university = obs
                .ty(ids.university)
                .expect("University row remains public");
            reserved_university.map(|site| {
                let sep = ((market.x_size + university.x_size) / 2)
                    .max((market.y_size + university.y_size) / 2);
                (site, sep)
            })
        } else {
            None
        };
        let Some((tx, ty)) = founder_local_site(obs, type_id, founder.tx, founder.ty, avoid) else {
            return false;
        };
        out.push(Cmd::Build {
            worker: founder.id,
            type_id,
            tx,
            ty,
        });
        if let Some(site) = reserved_university {
            self.reserved_university_site = Some((founder.id, site));
        }
        true
    }

    /// One citizen follows a deterministic wide circuit until it observes an enemy base.
    fn scout(&mut self, obs: &Obs, out: &mut Vec<Cmd>) {
        let ids = obs.ids();
        if self.enemy_base.is_some() || obs.frame >= 8 * 60 * FPS {
            if let Some(scout) = self.scout.take() {
                if obs.mine.iter().any(|m| m.id == scout) {
                    out.push(Cmd::Halt { unit: scout });
                }
            }
            return;
        }

        let scout = match self.scout.filter(|id| obs.mine.iter().any(|m| m.id == *id)) {
            Some(id) => id,
            None => {
                let Some(home) = capital(obs) else { return };
                let Some(unit) = obs
                    .mine
                    .iter()
                    .filter(|m| m.type_id == ids.citizen)
                    .filter(|m| !self.builders.contains(&m.id))
                    .max_by_key(|m| (m.tx - home.tx).abs().max((m.ty - home.ty).abs()))
                else {
                    return;
                };
                self.scout = Some(unit.id);
                unit.id
            }
        };
        let Some(unit) = obs.mine.iter().find(|m| m.id == scout) else {
            return;
        };
        let arrived = self
            .scout_goal
            .map(|goal| distance((unit.tx, unit.ty), goal) <= 3)
            .unwrap_or(true);
        if arrived || unit.job == Job::Idle {
            self.scout_leg = self.scout_leg.wrapping_add(1);
            // On a symmetric competitive start, the point opposite our capital is the
            // highest-probability enemy start. This is a hypothesis from public map
            // geometry, not a read of the hidden start list; the circuit remains the
            // deterministic fallback when the hypothesis is wrong.
            let first_hypothesis = (self.scout_leg == 1).then(|| {
                capital(obs).map(|home| (obs.map.w - 1 - home.tx, obs.map.h - 1 - home.ty))
            });
            let goal = first_hypothesis
                .flatten()
                .or_else(|| scout_waypoint(obs, unit.tx, unit.ty, self.scout_leg));
            if let Some(goal) = goal {
                self.scout_goal = Some(goal);
                out.push(Cmd::Move {
                    unit: scout,
                    tx: goal.0,
                    ty: goal.1,
                });
            }
        }
    }

    fn train_army(&mut self, obs: &Obs, out: &mut Vec<Cmd>) {
        let ids = obs.ids();
        // Preserve the food bank once the defensive prerequisite is established, and the
        // timber/wealth bank after aging, until the first University decision is accepted.
        // These are observation-derived reservations, not an income or cost modifier.
        if (obs.has_tech(ids.art_of_war) && !obs.has_tech(ids.classical_age))
            || (obs.has_tech(ids.classical_age) && obs.count_with_queued(ids.university) == 0)
        {
            return;
        }
        let candidates = buildable_military(obs);
        if candidates.is_empty() || obs.pop >= obs.pop_cap {
            return;
        }
        let remembered: Vec<(i32, i64)> = self
            .observed_mix
            .iter()
            .map(|(&type_id, &count)| (type_id, count))
            .collect();
        let counter = counter_pick(obs, &candidates, &remembered);
        let ranged = candidates
            .iter()
            .copied()
            .filter(|&id| obs.ty(id).map(|t| t.max_range > 1).unwrap_or(false))
            .min_by_key(|&id| type_value(obs, id));
        let durable = candidates
            .iter()
            .copied()
            .max_by_key(|&id| obs.ty(id).map(|t| t.hits).unwrap_or(0));

        // Counter units dominate, but every fourth/sixth choice adds range/frontline.
        // A monoculture is too easy to counter after the next sighting update.
        self.composition_turn = self.composition_turn.wrapping_add(1);
        let pick = if self.composition_turn % 6 == 0 {
            durable.or(counter)
        } else if self.composition_turn % 4 == 0 {
            ranged.or(counter)
        } else {
            counter
        };
        if let Some(type_id) = pick {
            queue_at(obs, type_id, 1, out);
        }
    }

    /// Put newly idle citizens onto the first University or Barracks before generic site
    /// staffing.
    /// This is deliberately bounded: construction gets at most two bodies and the
    /// normal employment pass handles everything else.
    fn staff_critical_site(&self, obs: &Obs, out: &mut Vec<Cmd>) {
        let ids = obs.ids();
        let Some(site) = obs
            .mine
            .iter()
            .find(|m| m.type_id == ids.market && !m.complete)
            .or_else(|| {
                obs.mine
                    .iter()
                    .find(|m| m.type_id == ids.university && !m.complete)
            })
            .or_else(|| {
                obs.mine
                    .iter()
                    .find(|m| m.type_id == ids.barracks && !m.complete)
            })
        else {
            return;
        };
        let assigned = obs
            .mine
            .iter()
            .filter(|m| matches!(m.job, Job::Work { target } if target == site.id))
            .count();
        // Retail movement/collision is body-based: sending every newly spawned citizen
        // down the same line can create a self-blocking column. One founder plus one
        // helper is enough to accelerate the site without manufacturing a crowd.
        for worker in obs
            .mine
            .iter()
            .filter(|m| m.type_id == ids.citizen && m.idle && Some(m.id) != self.scout)
            .take(2usize.saturating_sub(assigned))
        {
            out.push(Cmd::Work {
                unit: worker.id,
                target: site.id,
            });
        }
    }

    fn command_army(&mut self, obs: &Obs, out: &mut Vec<Cmd>) {
        let army = army(obs);
        if army.is_empty() {
            return;
        }
        let Some(home_ent) = capital(obs) else { return };
        let home = (home_ent.tx, home_ent.ty);
        let value = army_value(obs, &army);
        let health = army_health_value(obs, &army);
        let centre = centroid(&army);
        let visible_enemy_value = visible_enemy_value(obs, centre, 14);
        let enemy_known_value = self
            .observed_mix
            .iter()
            .map(|(&id, &n)| type_value(obs, id) * n)
            .sum::<i64>();
        // If a civilian scout has not confirmed a base by three minutes, the army probes
        // the same public-geometry hypothesis itself. Refusing to move without confirmed
        // information was safe but lost every seat-one game before contact.
        let objective = self.enemy_base.or_else(|| {
            (obs.frame >= 3 * 60 * FPS).then_some((obs.map.w - 1 - home.0, obs.map.h - 1 - home.1))
        });
        let rally = objective
            .map(|enemy| midpoint(home, enemy))
            .unwrap_or((home.0 + 4, home.1));

        if self.mode == Mode::Pushing {
            let attrited = value * 100 < self.push_start_value * 58
                || health * 100 < self.push_start_health * 52;
            let outmatched = visible_enemy_value > 0 && visible_enemy_value * 5 > health * 6;
            if attrited || outmatched {
                self.mode = Mode::Retreating;
                self.mode_until = obs.frame + 35 * FPS;
            }
        }
        if self.mode == Mode::Retreating
            && obs.frame >= self.mode_until
            && value * 4 >= self.push_start_value.max(1) * 3
        {
            self.mode = Mode::Massing;
        }

        match self.mode {
            Mode::Defending => {
                let threat = self.threat.unwrap_or(home);
                if let Some(target) = best_visible_target(obs, centre, false) {
                    focus_attack(&army, target, out);
                } else {
                    move_army(obs, &army, threat, out);
                }
                // Pull civilians out of the immediate contact area without reading the
                // attacker's hidden path or target.
                let ids = obs.ids();
                for unit in obs
                    .mine
                    .iter()
                    .filter(|m| m.type_id == ids.citizen && distance((m.tx, m.ty), threat) <= 6)
                {
                    if needs_move(unit, home) {
                        out.push(Cmd::Move {
                            unit: unit.id,
                            tx: home.0,
                            ty: home.1,
                        });
                    }
                }
            }
            Mode::Retreating => move_army(obs, &army, home, out),
            Mode::Massing => {
                move_army(obs, &army, rally, out);
                // Sightings are memory: a unit seen five minutes ago may no longer exist,
                // so remembered composition is suitable for counter selection but cannot
                // be treated as a perfect live army census. Bound its effect on the push
                // threshold and let current visibility drive the retreat decision.
                let required = (enemy_known_value * 3 / 4).clamp(240, 560);
                if objective.is_some() && health >= required {
                    self.mode = Mode::Pushing;
                    self.push_start_value = value.max(1);
                    self.push_start_health = health.max(1);
                    self.pushes += 1;
                }
            }
            Mode::Pushing => {
                if let Some(target) = best_visible_target(obs, centre, true) {
                    focus_attack(&army, target, out);
                } else if let Some(base) = objective {
                    move_army(obs, &army, base, out);
                } else {
                    self.mode = Mode::Massing;
                }
            }
        }
    }
}

fn army<'a>(obs: &'a Obs) -> Vec<&'a MyEnt> {
    obs.mine
        .iter()
        .filter(|m| !m.building && obs.ty(m.type_id).map(|t| t.is_military()).unwrap_or(false))
        .collect()
}

fn type_value(obs: &Obs, type_id: i32) -> i64 {
    obs.ty(type_id)
        .map(|t| t.cost.iter().map(|&cost| i64::from(cost)).sum())
        .unwrap_or(0)
}

fn army_value(obs: &Obs, army: &[&MyEnt]) -> i64 {
    army.iter().map(|unit| type_value(obs, unit.type_id)).sum()
}

fn army_health_value(obs: &Obs, army: &[&MyEnt]) -> i64 {
    army.iter()
        .map(|unit| {
            type_value(obs, unit.type_id) * i64::from(unit.hp) / i64::from(unit.max_hp.max(1))
        })
        .sum()
}

fn visible_enemy_value(obs: &Obs, centre: (i32, i32), radius: i32) -> i64 {
    obs.known
        .iter()
        .filter(|s| {
            !s.building && obs.visible(s.tx, s.ty) && distance((s.tx, s.ty), centre) <= radius
        })
        .map(|s| type_value(obs, s.type_id))
        .sum()
}

fn best_visible_target(obs: &Obs, from: (i32, i32), pushing: bool) -> Option<EntId> {
    let ids = obs.ids();
    obs.known
        .iter()
        // A scout can reveal a worker on the far side of the map. That is useful intel,
        // not a reason for the whole army to abandon its local fight and path to it.
        .filter(|s| obs.visible(s.tx, s.ty) && distance((s.tx, s.ty), from) <= 18)
        .min_by_key(|s| {
            let class = obs.ty(s.type_id).map_or(5, |ty| {
                if ty.is_civilian() {
                    if pushing {
                        0
                    } else {
                        1
                    }
                } else if ty.is_military() {
                    if pushing {
                        1
                    } else {
                        0
                    }
                } else if s.type_id == ids.small_city {
                    if pushing {
                        4
                    } else {
                        5
                    }
                } else if s.building {
                    2
                } else {
                    3
                }
            });
            (class, distance((s.tx, s.ty), from), s.id)
        })
        .map(|s| s.id)
}

fn focus_attack(army: &[&MyEnt], target: EntId, out: &mut Vec<Cmd>) {
    for unit in army {
        if unit.job != (Job::Attack { target }) {
            out.push(Cmd::Attack {
                unit: unit.id,
                target,
            });
        }
    }
}

fn move_army(obs: &Obs, army: &[&MyEnt], target: (i32, i32), out: &mut Vec<Cmd>) {
    // A single destination makes every body compete for one collision cell and can leave
    // an otherwise healthy army parked in a column. Preserve cohesion with a small,
    // deterministic 3x3 formation around the strategic objective.
    const FORMATION: [(i32, i32); 9] = [
        (0, 0),
        (-3, -3),
        (0, -3),
        (3, -3),
        (-3, 0),
        (3, 0),
        (-3, 3),
        (0, 3),
        (3, 3),
    ];
    for (index, unit) in army.iter().enumerate() {
        let offset = FORMATION[index % FORMATION.len()];
        let destination = (
            (target.0 + offset.0).clamp(1, obs.map.w - 2),
            (target.1 + offset.1).clamp(1, obs.map.h - 2),
        );
        if distance((unit.tx, unit.ty), destination) > 3 && needs_move(unit, destination) {
            out.push(Cmd::Move {
                unit: unit.id,
                tx: destination.0,
                ty: destination.1,
            });
        }
    }
}

fn needs_move(unit: &MyEnt, target: (i32, i32)) -> bool {
    match unit.job {
        Job::MoveTo { x, y } => {
            let tile = don_sim::systems::combat::RANGE_UNITS_PER_TILE;
            (x / tile, y / tile) != target
        }
        _ => true,
    }
}

fn centroid(units: &[&MyEnt]) -> (i32, i32) {
    let n = units.len().max(1) as i32;
    (
        units.iter().map(|unit| unit.tx).sum::<i32>() / n,
        units.iter().map(|unit| unit.ty).sum::<i32>() / n,
    )
}

fn midpoint(a: (i32, i32), b: (i32, i32)) -> (i32, i32) {
    (a.0 + (b.0 - a.0) / 2, a.1 + (b.1 - a.1) / 2)
}

fn distance(a: (i32, i32), b: (i32, i32)) -> i32 {
    (a.0 - b.0).abs().max((a.1 - b.1).abs())
}

fn founder_local_site(
    obs: &Obs,
    type_id: i32,
    founder_tx: i32,
    founder_ty: i32,
    avoid: Option<((i32, i32), i32)>,
) -> Option<(i32, i32)> {
    for radius in 3..18 {
        // Prefer a straight cardinal approach. The generic ring begins at its north-west
        // corner; on dense starts that diagonal can be legal for the footprint yet leave
        // the founding Citizen behind neighbouring bodies.
        let cardinal = [(0, -radius), (radius, 0), (0, radius), (-radius, 0)];
        for (dx, dy) in cardinal
            .into_iter()
            .chain(crate::arena::world::ring(radius))
        {
            let site = (founder_tx + dx, founder_ty + dy);
            if avoid.is_some_and(|(other, sep)| distance(site, other) < sep) {
                continue;
            }
            if obs.placement_ok(type_id, site.0, site.1) {
                return Some(site);
            }
        }
    }
    None
}

fn scout_waypoint(obs: &Obs, sx: i32, sy: i32, leg: usize) -> Option<(i32, i32)> {
    let (cx, cy) = (obs.map.w / 2, obs.map.h / 2);
    let radius = obs.map.w.min(obs.map.h) * 3 / 8;
    // Twelve legs is the shortest circuit that stayed reliable across the arena's fair
    // map seeds; sixteen delayed first contact enough to lose an entire production cycle.
    const LEGS: i32 = 12;
    let start = (0..LEGS)
        .min_by_key(|&k| {
            let (px, py) = crate::arena::map::polar(radius, k * 360 / LEGS);
            distance((px, py), (sx - cx, sy - cy))
        })
        .unwrap_or(0);
    for extra in 0..LEGS {
        let k = (start + leg as i32 + extra) % LEGS;
        let (px, py) = crate::arena::map::polar(radius, k * 360 / LEGS);
        let target = (
            (cx + px).clamp(1, obs.map.w - 2),
            (cy + py).clamp(1, obs.map.h - 2),
        );
        if obs.terrain(target.0, target.1).passable() {
            return Some(target);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    #[test]
    fn ai_has_no_direct_world_escape_hatch() {
        let source = include_str!("ai.rs");
        let forbidden = ["obs", ".world"].concat();
        assert!(!source.contains(&forbidden));
    }
}
