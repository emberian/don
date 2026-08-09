//! The two openings, in one world at last.
//!
//! [`ShippedOpening`] is `economic.bhs` cases 6→18 — the designers' purchase order — and
//! [`CapFirst`] is the optimiser-derived player's rules. Both aim at the **same**
//! [`BoomGoal`], which is the state those cases build, so "who booms faster" is one
//! question with one answer instead of two models each grading itself.
//!
//! Neither builds a single soldier. That is not an omission: it is the finding. The
//! shipped opening's entire mutating vocabulary is place/train/research (12 host
//! functions, `docs/tracks/ron-ai-impl.md` §1), and `CapFirst`'s rules are derived from
//! an economic search that has no notion of an enemy. Put either in a match against
//! something that attacks and watch what happens — that is what [`super::marshal`] is for.

use super::*;
use crate::arena::cmd::{Cmd, EntId};
use crate::arena::obs::Obs;
use crate::arena::world::Job;

/// The state both openings are trying to reach — [`crate::optimum::player::Goal`]'s
/// defaults, which are the state `economic.bhs` cases 6→18 build.
#[derive(Clone, Copy, Debug)]
pub struct BoomGoal {
    pub citizens: usize,
    pub farms: usize,
    pub camps: usize,
    pub cities: usize,
    pub markets: usize,
    pub age: usize,
}

impl Default for BoomGoal {
    fn default() -> Self {
        BoomGoal {
            citizens: 14,
            farms: 7,
            camps: 2,
            cities: 2,
            markets: 1,
            age: 1,
        }
    }
}

impl BoomGoal {
    pub fn met(&self, obs: &Obs) -> bool {
        let i = &obs.world.ids;
        obs.age >= self.age
            && obs.count_complete(i.citizen) >= self.citizens
            && obs.count_complete(i.farm) >= self.farms
            && obs.count_complete(i.camp) >= self.camps
            && obs.count_complete(i.small_city) >= self.cities
            && obs.count_complete(i.market) >= self.markets
            && obs.has_tech(i.written_word)
            && obs.has_tech(i.city_state)
            && obs.has_tech(i.barter)
    }
}

/// Choose a citizen to send to a build site: an idle one first, else the one seated at
/// the fullest gatherer (pulling from a five-slot camp costs less than emptying a farm).
pub fn builder_for(obs: &Obs, tx: i32, ty: i32) -> Option<EntId> {
    builder_for_except(obs, tx, ty, &[])
}

/// [`builder_for`], never choosing a citizen in `skip`.
///
/// The exclusion is not decoration. Without it the scout is the cheapest builder on the
/// board (it is walking, not gathering), so every placement recalls it and the bot never
/// explores anything — which is exactly what happened.
pub fn builder_for_except(obs: &Obs, tx: i32, ty: i32, skip: &[EntId]) -> Option<EntId> {
    let cit = obs.world.ids.citizen;
    let mut best: Option<(i64, EntId)> = None;
    for m in obs.mine.iter().filter(|m| m.type_id == cit) {
        if skip.contains(&m.id) {
            continue;
        }
        let d = (m.tx - tx).abs().max((m.ty - ty).abs()) as i64;
        let busy = match m.job {
            Job::Idle => 0,
            Job::Gather { target } => {
                // Prefer taking from a building with spare hands.
                let occupancy = obs
                    .mine
                    .iter()
                    .find(|b| b.id == target)
                    .map(|b| 400 / b.worker_cap.max(1) as i64)
                    .unwrap_or(400);
                200 + occupancy
            }
            Job::Work { .. } => 1200,
            // A citizen already walking somewhere was sent there on purpose.
            _ => 1500,
        };
        let score = busy + d;
        if best.map_or(true, |(b, _)| score < b) {
            best = Some((score, m.id));
        }
    }
    best.map(|(_, id)| id)
}

/// One step of the shipped build order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Want {
    Tech(TechSlot),
    Citizens(usize),
    Farms(usize),
    Camps(usize),
    Cities(usize),
    Markets(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TechSlot {
    WrittenWord,
    CityState,
    Barter,
    ClassicalAge,
}

impl TechSlot {
    fn id(self, obs: &Obs) -> i32 {
        let i = &obs.world.ids;
        match self {
            TechSlot::WrittenWord => i.written_word,
            TechSlot::CityState => i.city_state,
            TechSlot::Barter => i.barter,
            TechSlot::ClassicalAge => i.classical_age,
        }
    }
}

/// `economic.bhs` cases 6→18, generic nation, land map — the shipped opening as a player.
///
/// The list below is the same sequence [`crate::optimum::player::SHIPPED_ORDER`] carries,
/// rewritten as the *absolute counts* each case tests for (`num_type_with_queued(who,
/// "Citizen") < 9` and so on), because that is how the script's own state machine
/// advances. `BLOCK_ON_THIS` is reproduced literally: while a step is unsatisfied the
/// opening issues nothing else.
pub struct ShippedOpening {
    step: usize,
    pub goal: BoomGoal,
    pub done_frame: Option<i64>,
}

impl Default for ShippedOpening {
    fn default() -> Self {
        ShippedOpening {
            step: 0,
            goal: BoomGoal::default(),
            done_frame: None,
        }
    }
}

const SHIPPED_STEPS: &[Want] = &[
    Want::Tech(TechSlot::WrittenWord),
    Want::Tech(TechSlot::CityState),
    Want::Citizens(7),
    Want::Farms(4),
    Want::Cities(2),
    Want::Citizens(9),
    Want::Farms(5),
    Want::Camps(1),
    Want::Tech(TechSlot::Barter),
    Want::Markets(1),
    Want::Citizens(12),
    Want::Farms(7),
    Want::Camps(2),
    Want::Citizens(14),
    Want::Tech(TechSlot::ClassicalAge),
];

impl Bot for ShippedOpening {
    fn name(&self) -> String {
        "ShippedOpening".into()
    }

    fn act(&mut self, obs: &Obs, out: &mut Vec<Cmd>) {
        employ(obs, out);
        if self.done_frame.is_none() && self.goal.met(obs) {
            self.done_frame = Some(obs.frame);
        }
        while self.step < SHIPPED_STEPS.len() && satisfied(obs, SHIPPED_STEPS[self.step]) {
            self.step += 1;
        }
        let Some(&want) = SHIPPED_STEPS.get(self.step) else {
            // Past the book: keep the economy alive rather than freeze, which is what the
            // compiled stages would do once the script latched `SCRIPT_DONE`.
            keep_growing(obs, out);
            return;
        };
        issue(obs, want, out);
    }
}

fn have(obs: &Obs, w: Want) -> usize {
    let i = &obs.world.ids;
    match w {
        Want::Tech(_) => 0,
        Want::Citizens(_) => obs.count_with_queued(i.citizen),
        Want::Farms(_) => obs.count_with_queued(i.farm),
        Want::Camps(_) => obs.count_with_queued(i.camp),
        Want::Cities(_) => obs.count_with_queued(i.small_city),
        Want::Markets(_) => obs.count_with_queued(i.market),
    }
}

fn satisfied(obs: &Obs, w: Want) -> bool {
    match w {
        Want::Tech(t) => obs.has_tech(t.id(obs)),
        Want::Citizens(n)
        | Want::Farms(n)
        | Want::Camps(n)
        | Want::Cities(n)
        | Want::Markets(n) => have(obs, w) >= n,
    }
}

fn issue(obs: &Obs, w: Want, out: &mut Vec<Cmd>) {
    let i = obs.world.ids;
    match w {
        Want::Tech(t) => {
            queue_at(obs, t.id(obs), 1, out);
        }
        Want::Citizens(_) => {
            queue_at(obs, i.citizen, 1, out);
        }
        Want::Farms(_) => {
            place(obs, i.farm, out);
        }
        Want::Camps(_) => {
            place(obs, i.camp, out);
        }
        Want::Markets(_) => {
            place(obs, i.market, out);
        }
        Want::Cities(_) => {
            place(obs, i.small_city, out);
        }
    }
}

/// Place one building of `type_id` near the capital, with the cheapest available citizen.
pub fn place(obs: &Obs, type_id: i32, out: &mut Vec<Cmd>) -> bool {
    place_except(obs, type_id, &[], out)
}

/// [`place`], never conscripting a citizen in `skip`. Returns whether it emitted.
///
/// Callers should place **one** building per decision. Two placements in the same batch
/// pick their sites from the same observation, so the first one to land can block the
/// second, and the world then rejects it — a real race, and the arena counts it (see
/// `World::rejects`), but there is no reason to generate it.
pub fn place_except(obs: &Obs, type_id: i32, skip: &[EntId], out: &mut Vec<Cmd>) -> bool {
    let Some(t) = obs.ty(type_id) else {
        return false;
    };
    if !legal(obs, type_id) || !obs.can_pay(&t.cost) {
        return false;
    }
    let Some(cap) = capital(obs) else {
        return false;
    };
    // Do not stack two orders for the same building on the same tick.
    if obs.mine.iter().any(|m| m.type_id == type_id && !m.complete) && type_id != obs.world.ids.farm
    {
        return false;
    }
    let site = if type_id == obs.world.ids.camp || type_id == obs.world.ids.mine {
        best_gather_site(obs, type_id, cap.tx, cap.ty, 24)
    } else if type_id == obs.world.ids.small_city {
        // A second city goes as far from the first as the build radius allows while
        // staying on our side of the map.
        city_site(obs)
    } else {
        site_near(obs, type_id, cap.tx, cap.ty, 18)
    };
    let Some((x, y)) = site else { return false };
    match builder_for_except(obs, x, y, skip) {
        Some(w) => {
            out.push(Cmd::Build {
                worker: w,
                type_id,
                tx: x,
                ty: y,
            });
            true
        }
        None => false,
    }
}

/// A site for an expansion: legal, as far from the enemy as we can manage, and next to
/// forest so the city is worth having.
pub fn city_site(obs: &Obs) -> Option<(i32, i32)> {
    let cap = capital(obs)?;
    let sep = obs.world.params.min_city_sep;
    // Where the enemy is, as far as we know. With nothing known, "away from the map
    // centre" is the honest default.
    let threat = obs
        .known
        .iter()
        .filter(|s| s.building)
        .map(|s| (s.tx, s.ty))
        .next()
        .unwrap_or((obs.map.w / 2, obs.map.h / 2));
    let mut best: Option<(i64, (i32, i32))> = None;
    for r in sep..sep + 14 {
        for (dx, dy) in crate::arena::world::ring(r) {
            let (x, y) = (cap.tx + dx, cap.ty + dy);
            if obs
                .world
                .placement_ok(obs.pi, obs.world.ids.small_city, x, y)
                .is_err()
            {
                continue;
            }
            let away = (x - threat.0).abs().max((y - threat.1).abs()) as i64;
            let wood = obs
                .map
                .count_within(x, y, 10, crate::arena::map::Terrain::Forest)
                as i64;
            let score = away * 2 + wood;
            if best.map_or(true, |(b, _)| score > b) {
                best = Some((score, (x, y)));
            }
        }
        if best.is_some() {
            break;
        }
    }
    best.map(|(_, p)| p)
}

/// After the book runs out: farms and citizens while they still pay.
fn keep_growing(obs: &Obs, out: &mut Vec<Cmd>) {
    let i = obs.world.ids;
    let cits = obs.count_with_queued(i.citizen) as i64;
    let want = useful_slots(obs, 0) + useful_slots(obs, 1) + 3;
    if cits < want && obs.pop < obs.pop_cap {
        queue_at(obs, i.citizen, 1, out);
        return;
    }
    if (seats(obs, 0) as i64) < useful_slots(obs, 0) {
        place(obs, i.farm, out);
    } else if (seats(obs, 1) as i64) < useful_slots(obs, 1) {
        place(obs, i.camp, out);
    }
    let _ = out.len();
}

// ---------------------------------------------------------------------------
// the optimiser-derived opening
// ---------------------------------------------------------------------------

/// **Cap-First** — [`crate::optimum::player::CapFirst`]'s rules, against the arena.
///
/// The four rules transfer verbatim because the arena runs the same derived economy:
///
/// 1. Raise the ceiling before filling it — the Library's time goes to `City State`, then
///    `Barter`, then the age; `Written Word` moves no limit in the Ancient age and is
///    researched last.
/// 2. Never train a citizen the `COMMERCE_CAP` clamp will not pay for
///    ([`super::useful_slots`]).
/// 3. Keep gather seats one step ahead of citizens.
/// 4. Take the second city the moment `City State` lands.
/// 5. Lock the food bank once the age is the Library's next want.
///
/// Rule 5's trigger is different here and that is stated rather than hidden: the
/// optimiser knew each job's completion time because its world *was* a schedule. The
/// arena has no such oracle, so the lock arms on stock instead — at 60 % of the age's
/// cost, food goes to the age and nothing else.
pub struct CapFirst {
    pub goal: BoomGoal,
    pub done_frame: Option<i64>,
    pub max_builders: i64,
}

impl Default for CapFirst {
    fn default() -> Self {
        CapFirst {
            goal: BoomGoal::default(),
            done_frame: None,
            max_builders: 3,
        }
    }
}

impl CapFirst {
    /// Rule 1's preference list.
    pub fn library_want(&self, obs: &Obs) -> Option<i32> {
        let i = obs.world.ids;
        next_tech(
            obs,
            &[i.city_state, i.barter, i.classical_age, i.written_word],
        )
    }

    /// Rule 2's target.
    pub fn target_citizens(&self, obs: &Obs) -> i64 {
        useful_slots(obs, 0) + useful_slots(obs, 1) + self.max_builders
    }

    /// Rule 5.
    pub fn food_locked(&self, obs: &Obs) -> bool {
        let i = obs.world.ids;
        if obs.has_tech(i.classical_age) || self.library_want(obs) != Some(i.classical_age) {
            return false;
        }
        let cost = obs.ty(i.classical_age).map(|t| t.cost[0]).unwrap_or(0);
        obs.stock[0] * 10 >= cost * 6
    }
}

impl Bot for CapFirst {
    fn name(&self) -> String {
        "CapFirst".into()
    }

    fn act(&mut self, obs: &Obs, out: &mut Vec<Cmd>) {
        employ(obs, out);
        if self.done_frame.is_none() && self.goal.met(obs) {
            self.done_frame = Some(obs.frame);
        }
        let i = obs.world.ids;
        let locked = self.food_locked(obs);

        // Rule 1 — the Library never idles.
        if let Some(t) = self.library_want(obs) {
            queue_at(obs, t, 1, out);
        }

        // One placement per decision, in priority order: rule 4 (the second city the
        // moment City State lands) ahead of rule 3 (seats before workers).
        let food_gap = useful_slots(obs, 0) - seats(obs, 0);
        let wood_gap = useful_slots(obs, 1) - seats(obs, 1);
        let mut wants: Vec<i32> = Vec::new();
        if obs.has_tech(i.city_state)
            && obs.count_with_queued(i.small_city) < self.goal.cities
            && !locked
        {
            wants.push(i.small_city);
        }
        if wood_gap > 0 && obs.count_with_queued(i.camp) < 4 {
            wants.push(i.camp);
        }
        if food_gap > 0 && !locked && obs.count_with_queued(i.farm) < self.goal.farms.max(8) {
            wants.push(i.farm);
        }
        if obs.count_with_queued(i.market) < self.goal.markets && !locked {
            wants.push(i.market);
        }
        for ty in wants {
            if place(obs, ty, out) {
                break;
            }
        }

        // Rule 2 — never a citizen the clamp will not pay for.
        let cits = obs.count_with_queued(i.citizen) as i64;
        if !locked
            && cits < self.target_citizens(obs).max(self.goal.citizens as i64)
            && obs.pop < obs.pop_cap
        {
            queue_at(obs, i.citizen, 1, out);
        }
    }
}
