//! Two opening players over the measured economy in [`super::econ`].
//!
//! * [`CapFirst`] — **the optimiser-derived player**. Its rules are read off the
//!   search in `analysis/opening.py`, not off the shipped scripts. It exists to be
//!   run against the designers' order and to lose honestly if it does.
//! * [`ShippedBoom`] — the boom order of `ron-data/ai-scripts/economic.bhs`
//!   (cases 6→18, generic nation, land map), as *data*. This is deliberately **not**
//!   the script transcription: the `economic` and `library` modules of this crate own
//!   that, and this is only the purchase sequence those cases emit, so that the two
//!   openings can be run head to head inside one economy.
//!
//! Both drive the same [`World`] and are fully deterministic.

use super::econ::*;

/// What an opening is trying to reach. The default is exactly the state
/// `economic.bhs` cases 6–18 build, so both players answer the same question.
#[derive(Clone, Copy, Debug)]
pub struct Goal {
    pub age: i64,
    pub citizens: i64,
    pub farms: i64,
    pub woodcamps: i64,
    pub cities: i64,
    pub markets: i64,
    pub want_written_word: bool,
    pub want_city_state: bool,
    pub want_barter: bool,
}

impl Default for Goal {
    fn default() -> Self {
        Goal {
            age: 1,
            citizens: 14,
            farms: 7,
            woodcamps: 2,
            cities: 2,
            markets: 1,
            want_written_word: true,
            want_city_state: true,
            want_barter: true,
        }
    }
}

impl Goal {
    pub fn met(&self, w: &World) -> bool {
        w.age >= self.age
            && w.built("Citizen") >= self.citizens
            && w.built("Farm") >= self.farms
            && w.built("Woodcutter's Camp") >= self.woodcamps
            && w.built("Small City") >= self.cities
            && w.built("Market") >= self.markets
            && (!self.want_written_word || w.tech_landed("Written Word"))
            && (!self.want_city_state || w.tech_landed("City State"))
            && (!self.want_barter || w.tech_landed("Barter"))
    }
}

/// A finished run.
#[derive(Clone, Debug)]
pub struct Run {
    pub frames: i64,
    pub world: World,
    pub stalled: bool,
}

impl Run {
    pub fn order(&self) -> Vec<(i64, i64, &'static str)> {
        self.world
            .log
            .iter()
            .map(|&(s, d, i)| (s, d, CATALOGUE[i].name))
            .collect()
    }
}

/// How much later a Citizen has to look before the crew loses the bank to it.
/// Read off the search: the optimum never trains a citizen that would stand idle.
const CITIZEN_IDLE_PENALTY: i64 = 450;

/// How close the Classical Age has to be before the food bank is reserved for it.
const AGE_RESERVE_WINDOW: i64 = 1500;

/// Wait until `name` becomes legal (the shipped script's `BLOCK_ON_THIS`), then buy.
fn buy_blocking(w: &mut World, name: &str, limit: i64) -> bool {
    let mut guard = 0;
    while !w.legal(name) {
        let h = w.horizon();
        if h > w.frame {
            w.advance_to(h.min(w.frame + 600));
        } else {
            w.advance(60);
        }
        guard += 1;
        if w.frame > limit || guard > 400 {
            return false;
        }
    }
    w.buy(name)
}

// ---------------------------------------------------------------------------
// the optimiser-derived player
// ---------------------------------------------------------------------------

/// **Cap-First** — the player the optimisation says to be.
///
/// Four rules, each of which is a thing the search does that the shipped order does
/// not:
///
/// 1. **Raise the ceiling before filling it.** The Library's time goes to `City State`
///    then `Barter` — the two techs that move a *limit* (`epoch[1]+1` cities, and
///    `commerce_cap` 70→100 per resource per 30 s). `Written Word` moves no limit in
///    the Ancient age and is researched last, only because the target asks for it.
/// 2. **Never train a citizen the clamp will not pay for.** A new worker is worth
///    `SLOT16` only while gross income is under `commerce_cap*16`; past that it is
///    worth nothing and costs food. `World::useful_slots` is the test.
/// 3. **Keep gather slots one step ahead of citizens**, since a citizen with no slot
///    earns nothing, and farms are capped at `farms_per_city_base * cities`.
/// 4. **Take the second city the moment `City State` lands** — not for its 10+10 free
///    income (which the clamp partly eats) but because it doubles the farm cap.
#[derive(Clone, Copy, Debug, Default)]
pub struct CapFirst;

impl CapFirst {
    fn want_library(&self, w: &World, g: &Goal) -> Option<&'static str> {
        if g.want_city_state && !w.has_tech("City State") {
            return Some("City State");
        }
        if g.want_barter && !w.has_tech("Barter") {
            return Some("Barter");
        }
        // Age before Written Word: the age is 250 food and gates everything after it,
        // Written Word is 120 timber + 50 wealth and gates nothing we want.
        if w.age < g.age && !w.has_tech("Classical Age") {
            return Some("Classical Age");
        }
        if g.want_written_word && !w.has_tech("Written Word") {
            return Some("Written Word");
        }
        None
    }

    fn want_train(&self, w: &World, g: &Goal) -> Option<&'static str> {
        if w.count("Citizen") >= w.pop_cap() {
            return None;
        }
        // rule 2: never train a citizen the clamp will not pay for, and never train
        // past what the objective asks for -- an unemployed citizen is 20+ food of
        // pure loss, and the search never buys one.
        let want = g.citizens.min(self.target_citizens(w).max(g.citizens));
        if w.count("Citizen") >= want {
            return None;
        }
        Some("Citizen")
    }

    /// How many citizens the clamp can still pay for, in total. `useful_slots`
    /// already answers "how many workers on this resource earn anything", so the two
    /// resources simply add; the `+max_builders` keeps a crew available.
    fn target_citizens(&self, w: &World) -> i64 {
        w.useful_slots(FOOD) + w.useful_slots(TIMBER) + w.a.max_builders
    }

    /// The build crew's wish list, most valuable first.  A list rather than a single
    /// choice because a want can be temporarily unavailable (rule 5 locks the food
    /// bank for the age) and an idle crew is pure loss.
    fn want_build(&self, w: &World, g: &Goal) -> Vec<&'static str> {
        let mut v = Vec::new();
        let cities = w.count("Small City");
        // rule 4: the second city as soon as the Civic level allows it
        if cities < g.cities && w.legal("Small City") {
            v.push("Small City");
        }
        // rule 3: a slot for every citizen the clamp will pay for
        let farm_cap = FARMS_PER_CITY * cities;
        let want_farms = w.useful_slots(FOOD).max(g.farms).min(farm_cap);
        if w.count("Farm") < want_farms && w.legal("Farm") {
            v.push("Farm");
        }
        if w.count("Market") < g.markets && w.legal("Market") {
            v.push("Market");
        }
        let camps = w.count("Woodcutter's Camp");
        let want_camps =
            ((w.useful_slots(TIMBER) + w.a.slots_wood - 1) / w.a.slots_wood).max(g.woodcamps);
        if camps < want_camps.min(4) {
            v.push("Woodcutter's Camp");
        }
        v
    }

    /// rule 5: once the Classical Age is the next thing the Library wants and it is
    /// within about a minute, **stop spending food on anything else**.  The age is
    /// 250 food against a food ceiling of 100 per 30 s, so it is 75 s of the entire
    /// food economy; a 25-food citizen or a 70-food woodcutter's camp taken during
    /// that window pushes the age back by its own cost in full.  The search does the
    /// same thing by construction -- in its plan the second camp lands *after* the
    /// age, not before.
    fn food_locked(&self, w: &World, g: &Goal) -> bool {
        if w.has_tech("Classical Age") || self.want_library(w, g) != Some("Classical Age") {
            return false;
        }
        match w.ready_at("Classical Age") {
            Some(t) => t - w.frame < AGE_RESERVE_WINDOW,
            None => false,
        }
    }

    pub fn run(&self, a: Assume, g: Goal, limit: i64) -> Run {
        let mut w = World::new(a);
        for _ in 0..240 {
            if g.met(&w.settle()) {
                break;
            }
            let mut wants: Vec<(usize, &'static str)> = Vec::new();
            if let Some(n) = self.want_train(&w, &g) {
                wants.push((0, n));
            }
            for n in self.want_build(&w, &g) {
                wants.push((1, n));
            }
            if let Some(n) = self.want_library(&w, &g) {
                wants.push((2, n));
            }
            // Issue whichever wanted job can START first.  The model's clock is
            // global and monotone, so committing early to something you cannot yet
            // afford idles the other two servers -- exactly the mistake that makes a
            // naive priority list lose to the search.
            // rule 6: when a citizen would stand idle, a gather slot is worth more
            // than another citizen, so the build crew gets the bank first.  Without
            // this the trainer wins every tie (a Citizen is the cheapest thing on the
            // board) and eats the food the city and the age need.
            let seated: i64 = w.on.iter().sum();
            let idle = w.built("Citizen") - w.builders - seated;
            let locked = self.food_locked(&w, &g);
            let mut pick: Option<(&'static str, i64)> = None;
            for (s, want) in wants.into_iter() {
                if locked && want != "Classical Age" && item(want).cost[FOOD] > 0 {
                    continue;
                }
                let t = match w.ready_at(want) {
                    Some(t) => t,
                    None => continue,
                };
                let penalty = if s == 0 && idle > 0 {
                    CITIZEN_IDLE_PENALTY
                } else {
                    0
                };
                let score = t + penalty;
                if pick.map_or(true, |(_, bt)| score < bt) {
                    pick = Some((want, score));
                }
            }
            let (name, when) = match pick {
                Some(p) => p,
                None => break,
            };
            // rule 7: do not commit to something you cannot pay for yet if anything is
            // still in flight -- a tech landing changes the ceiling, and the ceiling is
            // what the whole plan is about.  Advance to the next completion and re-plan.
            if when > w.frame {
                if let Some(ev) = w.next_change(when) {
                    w.advance_to(ev);
                    continue;
                }
            }
            if !buy_blocking(&mut w, name, limit) {
                break;
            }
            if w.frame > limit {
                break;
            }
        }
        let settled = w.settle();
        Run {
            frames: w.horizon(),
            stalled: !g.met(&settled),
            world: settled,
        }
    }
}

// ---------------------------------------------------------------------------
// the designers' order
// ---------------------------------------------------------------------------

/// `economic.bhs` cases 6→18, generic nation, land map, map size ≥ 2.
///
/// | case | action | case | action |
/// |---|---|---|---|
/// | 6 | Science I — Written Word | 13 | Woodcutter's Camp #2 |
/// | 7 | Civic I — City State | 14 | Commerce I — Barter |
/// | 8 | citizens → 9 | 15 | Market #1 |
/// | 9 | Farm | 16 | citizens → 14 |
/// | 10 | City #2 | 17 | farms → 7 |
/// | 11 | citizens → 11 | 18 | Classical Age |
/// | 12 | Farm | | |
pub const SHIPPED_ORDER: &[&str] = &[
    "Written Word",
    "City State",
    "Citizen",
    "Citizen",
    "Citizen",
    "Citizen",
    "Farm",
    "Small City",
    "Citizen",
    "Citizen",
    "Farm",
    "Woodcutter's Camp",
    "Barter",
    "Market",
    "Citizen",
    "Citizen",
    "Citizen",
    "Farm",
    "Farm",
    "Classical Age",
];

#[derive(Clone, Copy, Debug, Default)]
pub struct ShippedBoom;

impl ShippedBoom {
    pub fn run(&self, a: Assume, g: Goal, limit: i64) -> Run {
        run_order(SHIPPED_ORDER, a, g, limit)
    }
}

/// `economic.bhs` with **exactly one edit**: case 6 and case 14 swapped, so the
/// Library's first job is Commerce I (Barter) instead of Science I (Written Word) and
/// everything else about the designers' order is untouched.
///
/// This isolates the single decision the optimiser disagrees with. It is also a
/// testable prediction about the retail game: run two AI matches with those two cases
/// swapped and compare resource totals at 10:00.
pub const SHIPPED_ORDER_SWAPPED: &[&str] = &[
    "Barter",
    "City State",
    "Citizen",
    "Citizen",
    "Citizen",
    "Citizen",
    "Farm",
    "Small City",
    "Citizen",
    "Citizen",
    "Farm",
    "Woodcutter's Camp",
    "Written Word",
    "Market",
    "Citizen",
    "Citizen",
    "Citizen",
    "Farm",
    "Farm",
    "Classical Age",
];

/// Run any fixed purchase order with the shipped script's `BLOCK_ON_THIS` semantics.
pub fn run_order(order: &[&str], a: Assume, g: Goal, limit: i64) -> Run {
    let mut w = World::new(a);
    for name in order {
        if !buy_blocking(&mut w, name, limit) {
            let settled = w.settle();
            return Run {
                frames: w.horizon(),
                stalled: true,
                world: settled,
            };
        }
    }
    let settled = w.settle();
    Run {
        frames: w.horizon(),
        stalled: !g.met(&settled),
        world: settled,
    }
}

/// Run both players under identical assumptions and report the gap in frames.
pub fn head_to_head(a: Assume, g: Goal) -> (Run, Run) {
    (CapFirst.run(a, g, 90_000), ShippedBoom.run(a, g, 90_000))
}
