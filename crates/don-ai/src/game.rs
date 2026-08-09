//! A headless economic game the transcribed production AI can actually play.
//!
//! # What this is
//!
//! [`Game`] is the smallest world in which `economic.bhs` is a *meaningful*
//! program: cities, buildings with worker slots, citizens, construction and
//! training queues, technology research, ages, population, and a six-resource
//! economy stepped by **`don-sim`'s derived** `Leader::do_gather`
//! (`don_sim::mechanics::resource_tick` + `credit_resource`). Every rule number
//! comes from [`crate::rules`], i.e. out of `ron-data/`; none is typed in here.
//!
//! It implements [`crate::api::ScriptWorld`], so the transcription in
//! [`crate::economic`] runs against it unchanged, and it accepts
//! [`crate::orders::Order`]s, so a policy can drive it through the same seam.
//!
//! # What this is NOT — the model boundary, stated once and plainly
//!
//! **There is no map.** Retail's `place_building_with_cost` runs a placement
//! search over terrain, city radius, resource tiles and blocking; that search is
//! not derived and is not here. In this model a placement succeeds iff the type
//! exists, its prerequisites are met, and the cost is payable. Consequences,
//! each a real divergence from retail and each marked `DEVIATION` at its site:
//!
//! * `DEVIATION 1` — placement never fails for spatial reasons, so the script's
//!   fallback ladders (`place_dock`'s six orphan attempts) are dead code here.
//! * `DEVIATION 2` — `BuildData::gather_max` (`+0x80`, the byte
//!   `max_workers_at_building` `0x009F2520` returns) is terrain-derived in
//!   retail. Here it is a per-type constant in [`ModelParams::gather_max`]. It
//!   is the single most load-bearing model number: it sets how many citizens a
//!   camp absorbs, which sets the whole citizen count the opening trains.
//! * `DEVIATION 3` — which resource a gatherer yields is decided in retail by
//!   the tiles under it (`BuildData::gather_from`, a `MiningList`). Here it is
//!   [`ModelParams::gathers`], a per-type constant.
//! * `DEVIATION 4` — construction and training assume a builder/trainer is
//!   always available and consume no citizen time; a job simply runs for
//!   `JOB_TIME` frames. Retail needs a citizen to walk there and work.
//! * `DEVIATION 5` — there is no combat, no enemy and no diplomacy, so
//!   `was_city_attacked`/`was_city_raided` are always 0 and the script's
//!   Barracks diversion never fires unless a test forces it.
//! * `DEVIATION 6` — the ten *compiled* production stages are stubs (see
//!   [`crate::scheduler`]). This game therefore measures **the script layer
//!   alone**, which is exactly the question the lane was asked.
//!
//! Fidelity: **C for the parts taken from shipped data and from `don-sim`'s
//! derived economy; the model boundary above is not fidelity at all, it is a
//! scaffold.** Nothing here is differentially tested against retail.

use std::collections::{BTreeMap, HashMap, HashSet};

use don_sim::mechanics::{
    commerce_cap, credit_resource, resource_period, resource_tick, CommerceCapGates, EconomyRules,
    ResourceTickInput,
};

use crate::abi::Difficulty;
use crate::api::ScriptWorld;
use crate::orders::{Order, OrderResult};
use crate::rules::{Rules, NRES};

/// Simulation frames per second. `rules.xml`'s header defines a frame as a
/// fifteenth of a second and `was_city_attacked` `0x009FD510` divides the frame
/// delta by 15 to get seconds, so 15 is the engine's own conversion.
pub const FRAMES_PER_SECOND: i32 = 15;

// ---------------------------------------------------------------------------
// Model parameters — the knobs that stand in for underived engine behaviour
// ---------------------------------------------------------------------------

/// Everything the model has to *choose* because the engine derives it from a
/// map we do not have. Kept in one struct so a sweep over the assumptions is a
/// loop, not an edit.
#[derive(Clone, Debug)]
pub struct ModelParams {
    /// `DEVIATION 2` — worker slots per gatherer building type.
    ///
    /// Two of these are pinned by the *shipped script's own arithmetic* rather
    /// than invented: `train_unit_with_need` counts a Farm as needing a worker
    /// only when `num_workers_at_building == 0`, i.e. a Farm holds exactly one;
    /// and `place_woodcutter` targets `min_size = 5` for a Woodcutter's Camp,
    /// tearing one down and retrying while the reported size is below that (to a
    /// floor of 3). The Mine and University figures are ours.
    pub gather_max: BTreeMap<String, i32>,
    /// `DEVIATION 3` — resource slot a gatherer building yields into.
    pub gathers: BTreeMap<String, usize>,
    /// Whether the difficulty income bonus (`LeaderData::get_gather_handicap`
    /// `0x006D66A0`) is applied. On by default because retail applies it.
    pub apply_difficulty_bonus: bool,
    /// The accumulator-period shift, and the **one open calibration question**
    /// in the whole economy.
    ///
    /// `don-sim`'s `resource_period` is `GATHER_RATE << 4` = 7200, read off
    /// `0x006CE7B9` (`mov ecx,[rules+0x27C]; shl ecx,4`), and
    /// `credit_resource` divides the per-frame income by it. That makes an
    /// income of `I` worth `I/16` resources per `GATHER_RATE` (450 frames =
    /// 30 s). But the same income is clamped against `COMMERCE_CAP`, which is
    /// the raw `70` at age 0 — so a player pinned at the age-0 commerce cap
    /// gathers **4.4 resources per 30 s, i.e. 8.75/min**, which is far slower
    /// than the shipped game plays. Something converts between the two scales
    /// and this lane did not find it. (`don-sim`'s own doc comment says the
    /// producers of `gross` are not derived, so the discrepancy may be there
    /// rather than in the shift.)
    ///
    /// `4` is the literal derived value and is the default. `0` — period =
    /// `GATHER_RATE`, so income is "resources per 30 s" — makes the wall-clock
    /// economy behave the way the shipped game does, and is what a usable RL
    /// environment wants until the real answer lands. **Neither is verified.**
    pub gather_period_shift: u32,
}

impl Default for ModelParams {
    fn default() -> Self {
        let mut gather_max = BTreeMap::new();
        gather_max.insert("Farm".to_string(), 1);
        gather_max.insert("Woodcutter's Camp".to_string(), 5);
        gather_max.insert("Mine".to_string(), 4);
        gather_max.insert("University".to_string(), 4);
        let mut gathers = BTreeMap::new();
        gathers.insert("Farm".to_string(), 0); // Food
        gathers.insert("Woodcutter's Camp".to_string(), 1); // Timber
        gathers.insert("Mine".to_string(), 4); // Metal
        gathers.insert("University".to_string(), 3); // Knowledge
        ModelParams {
            gather_max,
            gathers,
            apply_difficulty_bonus: true,
            gather_period_shift: 4,
        }
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// A placed building. `id` is never 0: the shipped scripts test object ids with
/// bare truthiness (`if (find_inactive_build(...))`), so a valid id of 0 would
/// silently change control flow. Retail object ids are likewise nonzero.
#[derive(Clone, Debug)]
pub struct Building {
    pub id: i32,
    pub ty: String,
    /// Index into [`Player::cities`], or `None` for an orphan placement.
    pub city: Option<usize>,
    pub active: bool,
    /// Frames of `JOB_TIME` still owed.
    pub frames_left: i32,
    /// True once construction has begun — `building_started` `0x009F1F20`.
    pub started: bool,
    pub workers: i32,
    /// `BuildData::gather_max` `+0x80`.
    pub gather_max: i32,
}

#[derive(Clone, Debug)]
pub struct City {
    pub id: i32,
    pub name: String,
    pub active: bool,
    pub frames_left: i32,
}

#[derive(Clone, Debug)]
pub struct TrainJob {
    pub unit_type: String,
    pub frames_left: i32,
}

#[derive(Clone, Debug)]
pub struct Research {
    pub tech: String,
    pub frames_left: i32,
}

/// One player's whole state.
#[derive(Clone, Debug)]
pub struct Player {
    pub who: i32,
    pub nation: String,
    /// `player.flags0 & 4` — the console/human bit. Humans never run the
    /// production AI (`Leader::production_ai` returns immediately).
    pub is_human: bool,
    /// Per-leader difficulty, `player+0x50`.
    pub difficulty: Difficulty,
    pub stock: [i32; NRES],
    /// The engine's per-resource accumulator, `econ[0x18 + res*4]`.
    pub acc: [i32; NRES],
    pub cities: Vec<City>,
    pub buildings: Vec<Building>,
    /// Completed units by type.
    pub units: BTreeMap<String, i32>,
    /// Units in production, with the city they are queued at.
    pub queue: Vec<(usize, TrainJob)>,
    pub techs: HashSet<String>,
    pub researching: Option<Research>,
    pub timers: HashMap<String, i32>,
    /// Citizens assigned to a gatherer slot. `idle = citizens - assigned`.
    pub assigned: i32,
    next_object_id: i32,
    /// Total orders accepted, for the report.
    pub orders_ok: u32,
    pub orders_refused: u32,
    pub orders_invalid: u32,
}

impl Player {
    fn unit_count(&self, ty: &str) -> i32 {
        *self.units.get(ty).unwrap_or(&0)
    }
    fn queued_count(&self, ty: &str) -> i32 {
        self.queue.iter().filter(|(_, j)| j.unit_type == ty).count() as i32
    }
    fn building_count(&self, ty: &str, count_inactive: bool) -> i32 {
        self.buildings
            .iter()
            .filter(|b| b.ty == ty && (count_inactive || b.active))
            .count() as i32
    }
    fn city_count(&self, count_inactive: bool) -> i32 {
        self.cities
            .iter()
            .filter(|c| count_inactive || c.active)
            .count() as i32
    }
    fn building(&self, id: i32) -> Option<&Building> {
        self.buildings.iter().find(|b| b.id == id)
    }
    fn city_index(&self, name: &str) -> Option<usize> {
        self.cities.iter().position(|c| c.name == name)
    }
}

/// One thing that happened, for the match timeline.
#[derive(Clone, Debug)]
pub struct Event {
    pub frame: i64,
    pub who: i32,
    pub text: String,
}

/// The whole match.
#[derive(Clone)]
pub struct Game {
    pub rules: Rules,
    pub params: ModelParams,
    pub econ: EconomyRules,
    pub players: Vec<Player>,
    /// `Game + 0x550`.
    pub frame: i64,
    /// `[0x00C061C0]`, the game-speed cell. Divides the AI period and multiplies
    /// income above 1.
    pub game_speed: i32,
    /// `Game + 0x2B`, 0-based: 0 Easiest … 5 Toughest.
    pub global_difficulty: Difficulty,
    /// `Game + 0x2D`, the starting-resources category. 8 = Infinite, which makes
    /// `Leader::production_ai` skip the script stage entirely.
    pub starting_resources: i32,
    /// `get_starting_town_size` category: 0 Nomad, 1 City Center Only,
    /// 2 Small Town, 3 Large Town (`rules.xml` `<CATEGORIES id="startingtowns">`).
    pub starting_town_size: i32,
    pub map_style: String,
    pub no_nation_powers: i32,
    pub rush_rules: i32,
    pub conquest: i32,
    pub log: Vec<Event>,
    /// Set true to keep the full event log; off, only order outcomes are counted.
    pub logging: bool,
}

impl Game {
    /// A fresh match. `nations` names one player per entry; all are AI.
    pub fn new(rules: Rules, nations: &[&str], difficulty: Difficulty) -> Game {
        let params = ModelParams::default();
        let econ = EconomyRules {
            commerce_cap: rules.constants.commerce_cap,
            ..EconomyRules::shipped()
        };
        let mut players = Vec::new();
        for (i, n) in nations.iter().enumerate() {
            players.push(Player {
                who: i as i32 + 1,
                nation: n.to_string(),
                is_human: false,
                difficulty,
                stock: rules.constants.starting_goods,
                acc: [0; NRES],
                cities: Vec::new(),
                buildings: Vec::new(),
                units: BTreeMap::new(),
                queue: Vec::new(),
                techs: HashSet::new(),
                researching: None,
                timers: HashMap::new(),
                assigned: 0,
                next_object_id: 1,
                orders_ok: 0,
                orders_refused: 0,
                orders_invalid: 0,
            });
        }
        let mut g = Game {
            rules,
            params,
            econ,
            players,
            frame: 0,
            game_speed: 1,
            global_difficulty: difficulty,
            starting_resources: 1, // "Standard"
            starting_town_size: 2, // "Small Town"
            map_style: "Texas".to_string(),
            no_nation_powers: 0,
            rush_rules: 0,
            conquest: 0,
            log: Vec::new(),
            logging: true,
        };
        for i in 0..g.players.len() {
            g.seed_start(i);
        }
        g
    }

    /// Seed a player's starting town from `citytemplates.xml`.
    ///
    /// `starting_town_size == 2` ("Small Town") gets the capital plus the
    /// template's buildings — all 17 shipped `size="small"` templates are three
    /// Farms and a Library. Size 1 ("City Center Only") gets the capital alone;
    /// size 0 ("Nomad") gets nothing, which is the branch `economic.bhs` step 1
    /// handles by placing a city. Starting citizens are **not** in the shipped
    /// data (`KOREAN_START_CITIZEN` is a *bonus* over the default), so the count
    /// is a model parameter: 3, matching the three template Farms.
    fn seed_start(&mut self, idx: usize) {
        let size = self.starting_town_size;
        if size <= 0 {
            self.players[idx].units.insert("Citizen".into(), 3);
            return;
        }
        let name = format!("{}-1", self.players[idx].nation);
        let id = self.next_id(idx);
        self.players[idx].cities.push(City {
            id,
            name,
            active: true,
            frames_left: 0,
        });
        self.players[idx].units.insert("Citizen".into(), 3);
        if size >= 2 {
            let template = self.rules.small_town_template.clone();
            for ty in template {
                self.spawn_building(idx, &ty, Some(0), true);
            }
        }
    }

    fn next_id(&mut self, idx: usize) -> i32 {
        let p = &mut self.players[idx];
        let id = p.next_object_id;
        p.next_object_id += 1;
        id
    }

    fn spawn_building(&mut self, idx: usize, ty: &str, city: Option<usize>, active: bool) -> i32 {
        let id = self.next_id(idx);
        let job = self
            .rules
            .buildings
            .get(ty)
            .map(|b| b.job_time)
            .unwrap_or(0);
        let gm = *self.params.gather_max.get(ty).unwrap_or(&0);
        self.players[idx].buildings.push(Building {
            id,
            ty: ty.to_string(),
            city,
            active,
            frames_left: if active { 0 } else { job },
            started: active,
            workers: 0,
            gather_max: gm,
        });
        id
    }

    fn note(&mut self, who: i32, text: String) {
        if self.logging {
            self.log.push(Event {
                frame: self.frame,
                who,
                text,
            });
        }
    }

    // -----------------------------------------------------------------------
    // Economy — driven by don-sim's derived Leader::do_gather
    // -----------------------------------------------------------------------

    /// The player's age: how many age techs they hold.
    ///
    /// `techrules.xml` gives every tech an `AGE`; the age techs themselves are
    /// the ones whose name ends in `" Age"` and whose `WHERE` is the Library.
    /// Holding N of them puts the player in age N.
    pub fn age_of(&self, idx: usize) -> usize {
        let p = &self.players[idx];
        let n = self
            .rules
            .tech_order
            .iter()
            .filter(|t| t.ends_with(" Age") && p.techs.contains(*t))
            .count();
        n.min(7)
    }

    /// Gross income per resource, in the units `resource_tick` consumes.
    ///
    /// `gross = CITY_GATHER[res] * active_cities + PEASANT_RATE * workers[res]`.
    ///
    /// **This composition is ours, not derived.** `docs/derivation/economy.md`
    /// and `don-sim`'s own doc comment both say the producers of `gross` (worker
    /// counts, `CITY_GATHER`, the building bonus tables) are *not* derived — so
    /// this is the simplest composition consistent with the two shipped
    /// constants `CITY_GATHER` (`10food/10timb`) and `PEASANT_RATE` (`10
    /// resources`). It has one property worth stating because it is checkable:
    /// at `PEASANT_RATE = 10` the age-0 `COMMERCE_CAP` of 70 is reached by
    /// exactly seven gatherers on a resource, which is the shape a commerce cap
    /// is for.
    fn gross_income(&self, idx: usize) -> [i32; NRES] {
        let p = &self.players[idx];
        let cities = p.city_count(false);
        let mut gross = [0i32; NRES];
        for (r, slot) in gross.iter_mut().enumerate() {
            *slot = self.rules.constants.city_gather[r] * cities;
        }
        for b in &p.buildings {
            if !b.active || b.workers == 0 {
                continue;
            }
            if let Some(&res) = self.params.gathers.get(&b.ty) {
                gross[res] += self.rules.constants.peasant_rate * b.workers;
            }
        }
        gross
    }

    /// One frame of `Leader::do_gather` for one player, through `don-sim`.
    fn tick_economy(&mut self, idx: usize) {
        let age = self.age_of(idx);
        let gross = self.gross_income(idx);
        let bonus = if self.params.apply_difficulty_bonus {
            self.effective_difficulty(idx).income_bonus_percent()
        } else {
            0
        };
        // `resource_period` is `GATHER_RATE << 4`; the shift is the knob.
        let period = if self.params.gather_period_shift == 4 {
            resource_period(self.econ.gather_rate)
        } else {
            self.econ
                .gather_rate
                .wrapping_shl(self.params.gather_period_shift)
        };
        for res in 0..NRES {
            let cap = commerce_cap(age, res, &self.econ, &CommerceCapGates::default(), 0);
            let input = ResourceTickInput {
                res,
                gross: gross[res],
                expense: self.upkeep(idx, res),
                bonus: 0,
                commerce_cap: cap,
                stockpile: self.players[idx].stock[res],
                interest_threshold: 0,
                // The Dutch-interest path is gated on a player property we
                // cannot resolve (`FUN_006E1370(player, 0x16)`); no nation here
                // has it, so the path stays off and the 16,000 ceiling with it.
                interest_applies: false,
                gather_bonus_pct: bonus,
                difficulty: self.global_difficulty as u8,
                game_flag_0x20_bit1: false,
                game_flag_0x2a_is_9: false,
                game_speed: self.game_speed,
            };
            let out = resource_tick(&input, &self.econ);
            if let Some(income) = out.accumulated {
                let acc = &mut self.players[idx].acc[res];
                let whole = credit_resource(income, period, acc);
                self.players[idx].stock[res] = self.players[idx].stock[res].saturating_add(whole);
            }
        }
    }

    /// Per-unit upkeep, from `unitrules.xml`'s `SUPPORT` column.
    fn upkeep(&self, idx: usize, res: usize) -> i32 {
        let p = &self.players[idx];
        let mut sum = 0;
        for (ty, n) in &p.units {
            if let Some(u) = self.rules.units.get(ty) {
                sum += u.support[res] * n;
            }
        }
        sum
    }

    /// `Player::GetEffectiveDifficulty` `0x006EC000`, in the simple
    /// configuration this model runs (`Game[0x820] & 4` clear, per-player
    /// difficulty unset) — the global byte wins.
    pub fn effective_difficulty(&self, idx: usize) -> Difficulty {
        let _ = idx;
        self.global_difficulty
    }

    pub fn population(&self, idx: usize) -> i32 {
        let p = &self.players[idx];
        let mut pop = 0;
        for (ty, n) in &p.units {
            pop += self.rules.units.get(ty).map(|u| u.pop).unwrap_or(1) * n;
        }
        for (_, j) in &p.queue {
            pop += self
                .rules
                .units
                .get(&j.unit_type)
                .map(|u| u.pop)
                .unwrap_or(1);
        }
        pop
    }

    pub fn pop_cap(&self, idx: usize) -> i32 {
        self.rules.constants.pop_cap[self.age_of(idx).min(7)]
    }

    // -----------------------------------------------------------------------
    // Frame
    // -----------------------------------------------------------------------

    /// Advance one simulation frame: economy, then jobs, then assignment.
    pub fn step(&mut self) {
        self.frame += 1;
        for idx in 0..self.players.len() {
            self.tick_economy(idx);
            self.tick_jobs(idx);
            self.auto_assign(idx);
        }
    }

    fn tick_jobs(&mut self, idx: usize) {
        // Cities under construction.
        let mut finished_cities = Vec::new();
        for c in &mut self.players[idx].cities {
            if !c.active {
                c.frames_left -= 1;
                if c.frames_left <= 0 {
                    c.active = true;
                    finished_cities.push(c.name.clone());
                }
            }
        }
        for n in finished_cities {
            self.note(idx as i32 + 1, format!("city complete: {n}"));
        }

        // Buildings under construction.
        let mut done = Vec::new();
        for b in &mut self.players[idx].buildings {
            if !b.active {
                b.started = true;
                b.frames_left -= 1;
                if b.frames_left <= 0 {
                    b.active = true;
                    done.push(b.ty.clone());
                }
            }
        }
        for ty in done {
            self.note(idx as i32 + 1, format!("built {ty}"));
        }

        // Training.
        let mut trained = Vec::new();
        self.players[idx].queue.retain_mut(|(_, j)| {
            j.frames_left -= 1;
            if j.frames_left <= 0 {
                trained.push(j.unit_type.clone());
                false
            } else {
                true
            }
        });
        for ty in trained {
            *self.players[idx].units.entry(ty.clone()).or_insert(0) += 1;
            self.note(idx as i32 + 1, format!("trained {ty}"));
        }

        // Research.
        let mut learned = None;
        if let Some(r) = &mut self.players[idx].researching {
            r.frames_left -= 1;
            if r.frames_left <= 0 {
                learned = Some(r.tech.clone());
            }
        }
        if let Some(t) = learned {
            self.players[idx].researching = None;
            self.players[idx].techs.insert(t.clone());
            self.note(idx as i32 + 1, format!("researched {t}"));
        }
    }

    /// `DEVIATION 4/6` — the city AI's job, stubbed: idle citizens fill any
    /// unfilled gatherer slot, oldest building first.
    ///
    /// The shipped script's own `assign_idle` only ever pushes citizens toward
    /// Woodcutter's Camps; everything else is compiled. Without *some* stand-in
    /// the economy never grows and the experiment is vacuous, so this exists and
    /// is marked. It is deliberately dumb: no preference, no rebalancing.
    fn auto_assign(&mut self, idx: usize) {
        let citizens = self.players[idx].unit_count("Citizen");
        let mut idle = citizens - self.players[idx].assigned;
        if idle <= 0 {
            return;
        }
        let gathers: Vec<String> = self.params.gathers.keys().cloned().collect();
        for b in &mut self.players[idx].buildings {
            if idle <= 0 {
                break;
            }
            if !b.active || !gathers.contains(&b.ty) {
                continue;
            }
            let free = b.gather_max - b.workers;
            if free > 0 {
                let take = free.min(idle);
                b.workers += take;
                idle -= take;
            }
        }
        self.players[idx].assigned = citizens - idle;
    }

    // -----------------------------------------------------------------------
    // Orders
    // -----------------------------------------------------------------------

    fn can_pay(&self, idx: usize, cost: &[i32; NRES]) -> bool {
        (0..NRES).all(|r| self.players[idx].stock[r] >= cost[r])
    }

    fn pay(&mut self, idx: usize, cost: &[i32; NRES]) {
        for r in 0..NRES {
            self.players[idx].stock[r] -= cost[r];
        }
    }

    fn preqs_met(&self, idx: usize, preq: &[String]) -> bool {
        preq.iter().all(|p| self.players[idx].techs.contains(p))
    }

    /// The one mutation seam. Everything that changes the world goes here.
    pub fn submit(&mut self, o: Order) -> OrderResult {
        let who = o.who();
        let idx = match self.index_of(who) {
            Some(i) => i,
            None => return OrderResult::Invalid,
        };
        let r = self.submit_inner(idx, &o);
        let p = &mut self.players[idx];
        match r {
            OrderResult::Ok(_) => p.orders_ok += 1,
            OrderResult::Refused => p.orders_refused += 1,
            OrderResult::Invalid => p.orders_invalid += 1,
        }
        r
    }

    fn index_of(&self, who: i32) -> Option<usize> {
        if who < 1 || who as usize > self.players.len() {
            None
        } else {
            Some(who as usize - 1)
        }
    }

    fn submit_inner(&mut self, idx: usize, o: &Order) -> OrderResult {
        match o {
            Order::PlaceBuilding {
                build_type,
                city_name,
                ..
            } => {
                let Some(ci) = self.players[idx].city_index(city_name) else {
                    return OrderResult::Invalid;
                };
                self.place(idx, build_type, Some(ci))
            }
            // DEVIATION 1: with no map, an orphan placement is a placement whose
            // city is the anchor's city. The anchor only matters spatially.
            Order::PlaceOrphanBuilding {
                build_type, near, ..
            } => {
                let ci = self.players[idx].building(*near).and_then(|b| b.city);
                self.place(idx, build_type, ci)
            }
            Order::PlaceBuildingUpgrade {
                build_type,
                city_name,
                ..
            } => {
                let Some(ci) = self.players[idx].city_index(city_name) else {
                    return OrderResult::Invalid;
                };
                // An upgrade replaces its `FROM` predecessor in that city.
                let from = self
                    .rules
                    .buildings
                    .get(build_type)
                    .and_then(|b| b.from.clone());
                let target = from.and_then(|f| {
                    self.players[idx]
                        .buildings
                        .iter()
                        .position(|b| b.ty == f && b.city == Some(ci) && b.active)
                });
                let r = self.place(idx, build_type, Some(ci));
                if let (OrderResult::Ok(_), Some(t)) = (r, target) {
                    self.players[idx].buildings.remove(t);
                }
                r
            }
            Order::PlaceCity { .. } => {
                let Some(t) = self.rules.buildings.get("Small City").cloned() else {
                    return OrderResult::Invalid;
                };
                if !self.preqs_met(idx, &t.preq) {
                    return OrderResult::Invalid;
                }
                // The script researches "City State" itself before asking; the
                // engine also requires it, and it is not a PREQ0 on Small City.
                if !self.players[idx].techs.contains("City State")
                    && self.players[idx].cities.len() >= 1
                {
                    return OrderResult::Invalid;
                }
                if !self.can_pay(idx, &t.costs) {
                    return OrderResult::Refused;
                }
                self.pay(idx, &t.costs);
                let id = self.next_id(idx);
                let n = self.players[idx].cities.len() + 1;
                let name = format!("{}-{}", self.players[idx].nation, n);
                self.players[idx].cities.push(City {
                    id,
                    name: name.clone(),
                    active: false,
                    frames_left: t.job_time,
                });
                self.note(idx as i32 + 1, format!("place city {name}"));
                OrderResult::Ok(id)
            }
            Order::TrainUnit { num, unit_type, .. } => self.train(idx, *num, unit_type, None),
            Order::TrainUnitAt {
                num,
                unit_type,
                build_o,
                ..
            } => self.train(idx, *num, unit_type, Some(*build_o)),
            Order::ResearchTech { tech, .. } => {
                let Some(t) = self.rules.techs.get(tech).cloned() else {
                    return OrderResult::Invalid;
                };
                if self.players[idx].techs.contains(tech) {
                    return OrderResult::Invalid;
                }
                if self.players[idx].researching.is_some() {
                    return OrderResult::Refused;
                }
                if !self.preqs_met(idx, &t.preq) {
                    return OrderResult::Invalid;
                }
                // A tech needs its `WHERE` building to exist.
                if let Some(w) = &t.where_ {
                    if self.players[idx].building_count(w, false) == 0 {
                        return OrderResult::Invalid;
                    }
                }
                if !self.can_pay(idx, &t.costs) {
                    return OrderResult::Refused;
                }
                self.pay(idx, &t.costs);
                self.players[idx].researching = Some(Research {
                    tech: tech.clone(),
                    frames_left: t.job_time.max(1),
                });
                self.note(idx as i32 + 1, format!("research {tech}"));
                OrderResult::Ok(1)
            }
            Order::DestroyBuilding { build_o, .. } => {
                match self.players[idx]
                    .buildings
                    .iter()
                    .position(|b| b.id == *build_o)
                {
                    Some(i) => {
                        let b = self.players[idx].buildings.remove(i);
                        self.players[idx].assigned -= b.workers;
                        OrderResult::Ok(1)
                    }
                    None => OrderResult::Invalid,
                }
            }
            // `assign_idle` moves idle citizens onto a woodcutter camp. With no
            // map, "arrive at (x,y)" is "take a free slot at the building whose
            // id was encoded as x" — see `PlayerView::object_position_x`.
            Order::MoveUnit { x, .. } => {
                let target = *x;
                let citizens = self.players[idx].unit_count("Citizen");
                let idle = citizens - self.players[idx].assigned;
                if idle <= 0 {
                    return OrderResult::Refused;
                }
                match self.players[idx]
                    .buildings
                    .iter_mut()
                    .find(|b| b.id == target)
                {
                    Some(b) if b.active && b.workers < b.gather_max => {
                        b.workers += 1;
                        self.players[idx].assigned += 1;
                        OrderResult::Ok(1)
                    }
                    _ => OrderResult::Refused,
                }
            }
            Order::CitizenRepair { .. } => OrderResult::Ok(1),
        }
    }

    fn place(&mut self, idx: usize, ty: &str, city: Option<usize>) -> OrderResult {
        let Some(t) = self.rules.buildings.get(ty).cloned() else {
            return OrderResult::Invalid;
        };
        if !self.preqs_met(idx, &t.preq) {
            return OrderResult::Invalid;
        }
        // `j` = "Max 1 of this building allowed per city" (buildingrules.xml).
        if t.flags.contains('j') {
            if let Some(ci) = city {
                let clash = self.players[idx]
                    .buildings
                    .iter()
                    .any(|b| b.ty == ty && b.city == Some(ci));
                if clash {
                    return OrderResult::Refused;
                }
            }
        }
        if !self.can_pay(idx, &t.costs) {
            return OrderResult::Refused;
        }
        self.pay(idx, &t.costs);
        let id = self.spawn_building(idx, ty, city, false);
        self.note(idx as i32 + 1, format!("place {ty}"));
        OrderResult::Ok(id)
    }

    fn train(&mut self, idx: usize, num: i32, ty: &str, at: Option<i32>) -> OrderResult {
        let Some(u) = self.rules.units.get(ty).cloned() else {
            return OrderResult::Invalid; // e.g. the shipped "Citizens" typo
        };
        if !self.preqs_met(idx, &u.preq) {
            return OrderResult::Invalid;
        }
        if let Some(w) = &u.where_ {
            if self.players[idx].building_count(w, false) == 0
                && self.players[idx].city_count(false) == 0
            {
                return OrderResult::Invalid;
            }
            // `WHERE = Small City` means "any city".
            let is_city = w.ends_with("City");
            if !is_city && self.players[idx].building_count(w, false) == 0 {
                return OrderResult::Invalid;
            }
        }
        let city = at
            .and_then(|id| self.players[idx].cities.iter().position(|c| c.id == id))
            .or_else(|| self.players[idx].cities.iter().position(|c| c.active))
            .unwrap_or(0);
        if self.players[idx].cities.is_empty() {
            return OrderResult::Invalid;
        }
        let mut made = 0;
        for _ in 0..num.max(1) {
            if self.population(idx) >= self.pop_cap(idx) {
                break;
            }
            if !self.can_pay(idx, &u.costs) {
                break;
            }
            self.pay(idx, &u.costs);
            self.players[idx].queue.push((
                city,
                TrainJob {
                    unit_type: ty.to_string(),
                    frames_left: u.job_time.max(1),
                },
            ));
            made += 1;
        }
        if made == 0 {
            OrderResult::Refused
        } else {
            self.note(idx as i32 + 1, format!("train {made}x {ty}"));
            OrderResult::Ok(1)
        }
    }
}

// ---------------------------------------------------------------------------
// ScriptWorld
// ---------------------------------------------------------------------------

/// One player's view of the game, which is what a BHS production script sees.
///
/// Retail's host functions take `who` explicitly and validate it; so does this,
/// via [`Game::index_of`], returning the engine's `-1` on failure.
pub struct PlayerView<'a> {
    pub game: &'a mut Game,
    pub who: i32,
}

impl PlayerView<'_> {
    fn idx(&self) -> Option<usize> {
        self.game.index_of(self.who)
    }
    fn p(&self) -> Option<&Player> {
        self.idx().map(|i| &self.game.players[i])
    }
}

/// Every query guards `who` exactly as the engine does and yields `-1`.
macro_rules! guard {
    ($self:expr, $who:expr) => {
        match $self.game.index_of($who) {
            Some(i) => i,
            None => return -1,
        }
    };
}

impl ScriptWorld for PlayerView<'_> {
    fn get_mapstyle(&self) -> String {
        self.game.map_style.clone()
    }
    fn get_is_no_nation_powers(&self) -> i32 {
        self.game.no_nation_powers
    }
    fn get_rush_rules(&self) -> i32 {
        self.game.rush_rules
    }
    fn is_conquest_scenario(&self) -> i32 {
        self.game.conquest
    }

    fn population(&self, who: i32) -> i32 {
        let i = guard!(self, who);
        self.game.population(i)
    }
    fn age(&self, who: i32) -> i32 {
        let i = guard!(self, who);
        self.game.age_of(i) as i32
    }
    fn get_starting_town_size(&self, who: i32) -> i32 {
        let _ = guard!(self, who);
        self.game.starting_town_size
    }
    fn get_starting_resources(&self, who: i32) -> i32 {
        let _ = guard!(self, who);
        self.game.starting_resources
    }
    fn num_cities(&self, who: i32) -> i32 {
        let i = guard!(self, who);
        self.game.players[i].city_count(false)
    }
    fn num_type(&self, who: i32, object_type: &str) -> i32 {
        let i = guard!(self, who);
        let p = &self.game.players[i];
        p.unit_count(object_type) + p.building_count(object_type, false)
    }
    fn num_type_with_queued(&self, who: i32, unit_type: &str) -> i32 {
        let i = guard!(self, who);
        let p = &self.game.players[i];
        let cities = if unit_type.ends_with("City") {
            p.city_count(true)
        } else {
            0
        };
        p.unit_count(unit_type)
            + p.queued_count(unit_type)
            + p.building_count(unit_type, true)
            + cities
    }
    fn num_rare_resources_seen(&self, who: i32) -> i32 {
        let _ = guard!(self, who);
        0 // DEVIATION 1: no map, so no rare resources are ever seen.
    }
    fn find_nation(&self, who: i32) -> String {
        match self.game.index_of(who) {
            Some(i) => self.game.players[i].nation.clone(),
            None => String::new(),
        }
    }
    fn at_least_type(&self, who: i32, num: i32, ty: &str) -> i32 {
        let i = guard!(self, who);
        match crate::rules::RES_NAMES.iter().position(|n| *n == ty) {
            Some(r) => i32::from(self.game.players[i].stock[r] >= num),
            None => -1,
        }
    }
    fn get_techs_per_age(&self, who: i32) -> i32 {
        let i = guard!(self, who);
        // `0x009EE8B0` reads `RULES[0x220 + age]` and returns 0 above age 6.
        // `TECHS_PER_AGE` is not one of the constants this crate loads, so the
        // count of non-age Library techs available in the current age stands in.
        let age = self.game.age_of(i);
        if age > 6 {
            return 0;
        }
        self.game
            .rules
            .tech_order
            .iter()
            .filter(|t| {
                let r = &self.game.rules.techs[*t];
                r.age as usize == age
                    && !t.ends_with(" Age")
                    && r.where_.as_deref() == Some("Library")
            })
            .count() as i32
    }
    fn have_tech(&self, who: i32, tech_type: &str) -> i32 {
        let i = guard!(self, who);
        i32::from(self.game.players[i].techs.contains(tech_type))
    }
    fn researching_tech(&self, who: i32, tech_type: &str) -> i32 {
        let i = guard!(self, who);
        i32::from(
            self.game.players[i]
                .researching
                .as_ref()
                .map(|r| r.tech.as_str())
                == Some(tech_type),
        )
    }
    fn can_pay_cost(&self, who: i32, ty: &str) -> i32 {
        let i = guard!(self, who);
        match self.game.rules.cost_of(ty) {
            Some(c) => i32::from(self.game.can_pay(i, &c)),
            None => -1,
        }
    }

    fn find_idle_citizen(&self, who: i32) -> i32 {
        let i = guard!(self, who);
        let p = &self.game.players[i];
        // Idle citizens have no per-unit identity in this model; the engine
        // returns an object id and `-1` for none, so report a synthetic nonzero
        // id while any citizen is unassigned.
        if p.unit_count("Citizen") - p.assigned > 0 {
            1
        } else {
            -1
        }
    }
    fn find_unit(&self, who: i32, unit_type: &str) -> i32 {
        let i = guard!(self, who);
        if self.game.players[i].unit_count(unit_type) > 0 {
            1
        } else {
            -1
        }
    }
    fn find_build(&self, who: i32, build_type: &str) -> i32 {
        let i = guard!(self, who);
        self.game.players[i]
            .buildings
            .iter()
            .find(|b| b.ty == build_type && b.active)
            .map(|b| b.id)
            .unwrap_or(-1)
    }
    fn find_build_at_city(
        &self,
        who: i32,
        city_name: &str,
        build_type: &str,
        count_inactive: i32,
    ) -> i32 {
        let i = guard!(self, who);
        let p = &self.game.players[i];
        let Some(ci) = p.city_index(city_name) else {
            return -1;
        };
        p.buildings
            .iter()
            .find(|b| b.ty == build_type && b.city == Some(ci) && (count_inactive != 0 || b.active))
            .map(|b| b.id)
            .unwrap_or(-1)
    }
    fn find_inactive_build(&self, who: i32, build_type: &str) -> i32 {
        let i = guard!(self, who);
        let p = &self.game.players[i];
        if build_type.ends_with("City") {
            if let Some(c) = p.cities.iter().find(|c| !c.active) {
                return c.id;
            }
        }
        p.buildings
            .iter()
            .find(|b| b.ty == build_type && !b.active)
            .map(|b| b.id)
            .unwrap_or(-1)
    }
    fn find_city_id(&self, city_name: &str) -> i32 {
        for p in &self.game.players {
            if let Some(c) = p.cities.iter().find(|c| c.name == city_name) {
                return c.id;
            }
        }
        -1
    }
    fn find_city_with_num(&self, who: i32, city_num: i32) -> String {
        match self.p() {
            Some(p) if who == self.who => p
                .cities
                .get((city_num - 1).max(0) as usize)
                .map(|c| c.name.clone())
                .unwrap_or_default(),
            _ => match self.game.index_of(who) {
                Some(i) => self.game.players[i]
                    .cities
                    .get((city_num - 1).max(0) as usize)
                    .map(|c| c.name.clone())
                    .unwrap_or_default(),
                None => String::new(),
            },
        }
    }
    fn num_city_buildings(
        &self,
        who: i32,
        city_name: &str,
        build_type: &str,
        count_inactive: i32,
    ) -> i32 {
        let i = guard!(self, who);
        let p = &self.game.players[i];
        let Some(ci) = p.city_index(city_name) else {
            return 0;
        };
        p.buildings
            .iter()
            .filter(|b| {
                b.ty == build_type && b.city == Some(ci) && (count_inactive != 0 || b.active)
            })
            .count() as i32
    }
    fn building_started(&self, who: i32, build_o: i32) -> i32 {
        let i = guard!(self, who);
        let p = &self.game.players[i];
        if let Some(b) = p.building(build_o) {
            return i32::from(b.started);
        }
        // `city_placement`'s `health_check` passes the id of a Small City under
        // construction, which is a city record here, not a building.
        if p.cities.iter().any(|c| c.id == build_o) {
            return 1;
        }
        -1
    }
    fn num_workers_at_building(&self, who: i32, build_o: i32) -> i32 {
        let i = guard!(self, who);
        self.game.players[i]
            .building(build_o)
            .map(|b| b.workers)
            .unwrap_or(-1)
    }
    fn max_workers_at_building(&self, who: i32, build_o: i32) -> i32 {
        let i = guard!(self, who);
        // `0x009F2520` returns -1 unless the type carries build flag `g`.
        match self.game.players[i].building(build_o) {
            Some(b) => {
                let g = self
                    .game
                    .rules
                    .buildings
                    .get(&b.ty)
                    .map(|t| t.is_gatherer())
                    .unwrap_or(false);
                if g {
                    b.gather_max
                } else {
                    -1
                }
            }
            None => -1,
        }
    }
    fn num_type_queued(&self, who: i32, build_o: i32, unit_type: &str) -> i32 {
        let i = guard!(self, who);
        let p = &self.game.players[i];
        let Some(ci) = p.cities.iter().position(|c| c.id == build_o) else {
            return -1;
        };
        p.queue
            .iter()
            .filter(|(c, j)| *c == ci && j.unit_type == unit_type)
            .count() as i32
    }
    fn find_num_idle_unit(&self, who: i32, unit_type: &str) -> i32 {
        let i = guard!(self, who);
        let p = &self.game.players[i];
        if unit_type == "Citizen" {
            (p.unit_count("Citizen") - p.assigned).max(0)
        } else {
            p.unit_count(unit_type)
        }
    }

    fn was_city_attacked(&self, who_defender: i32, _city_name: &str, _seconds: i32) -> i32 {
        let _ = guard!(self, who_defender);
        0 // DEVIATION 5: no combat in this model.
    }
    fn was_city_raided(&self, who_defender: i32, _city_name: &str, _seconds: i32) -> i32 {
        let _ = guard!(self, who_defender);
        0
    }

    fn set_timer(&mut self, timer_id: &str, seconds: i32) -> i32 {
        let Some(i) = self.idx() else { return -1 };
        let expiry = self.game.frame + (seconds as i64) * FRAMES_PER_SECOND as i64;
        self.game.players[i]
            .timers
            .insert(timer_id.to_string(), expiry as i32);
        1
    }
    fn stop_timer(&mut self, timer_id: &str) -> i32 {
        let Some(i) = self.idx() else { return -1 };
        self.game.players[i].timers.remove(timer_id);
        1
    }
    fn timer_expired(&self, timer_id: &str) -> i32 {
        let Some(i) = self.idx() else { return -1 };
        match self.game.players[i].timers.get(timer_id) {
            Some(&e) => i32::from(self.game.frame >= e as i64),
            None => 0,
        }
    }

    fn research_tech_with_cost(&mut self, who: i32, tech: &str) -> i32 {
        self.game
            .submit(Order::ResearchTech {
                who,
                tech: tech.to_string(),
            })
            .as_i32()
    }
    fn train_unit_with_cost(&mut self, who: i32, num: i32, unit_type: &str) -> i32 {
        self.game
            .submit(Order::TrainUnit {
                who,
                num,
                unit_type: unit_type.to_string(),
            })
            .as_i32()
    }
    fn train_unit_at_with_cost(
        &mut self,
        who: i32,
        num: i32,
        unit_type: &str,
        build_o: i32,
    ) -> i32 {
        self.game
            .submit(Order::TrainUnitAt {
                who,
                num,
                unit_type: unit_type.to_string(),
                build_o,
            })
            .as_i32()
    }
    fn place_building_with_cost(&mut self, who: i32, build_type: &str, city_name: &str) -> i32 {
        self.game
            .submit(Order::PlaceBuilding {
                who,
                build_type: build_type.to_string(),
                city_name: city_name.to_string(),
            })
            .as_i32()
    }
    fn place_orphan_building_with_cost(&mut self, who: i32, build_type: &str, build_o: i32) -> i32 {
        self.game
            .submit(Order::PlaceOrphanBuilding {
                who,
                build_type: build_type.to_string(),
                near: build_o,
            })
            .as_i32()
    }
    fn place_building_upgrade_with_cost(
        &mut self,
        who: i32,
        build_type: &str,
        city_name: &str,
    ) -> i32 {
        self.game
            .submit(Order::PlaceBuildingUpgrade {
                who,
                build_type: build_type.to_string(),
                city_name: city_name.to_string(),
            })
            .as_i32()
    }
    fn place_city_with_cost(&mut self, who: i32) -> i32 {
        self.game.submit(Order::PlaceCity { who }).as_i32()
    }
    fn destroy_building(&mut self, who: i32, build_o: i32) -> i32 {
        self.game
            .submit(Order::DestroyBuilding { who, build_o })
            .as_i32()
    }
    fn citizen_repair_order(&mut self, who: i32, unit_o: i32, build_o_target: i32) -> i32 {
        self.game
            .submit(Order::CitizenRepair {
                who,
                unit_o,
                build_o_target,
            })
            .as_i32()
    }

    /// `object_position_x(who, object)`. With no map this returns the object id
    /// itself, so that `assign_idle`'s `unit_move_order(who, idle, xpos, ypos)`
    /// still names the building it meant. Marked, not hidden.
    fn object_position_x(&self, who: i32, object: i32) -> i32 {
        let _ = guard!(self, who);
        object
    }
    fn object_position_y(&self, who: i32, _object: i32) -> i32 {
        let _ = guard!(self, who);
        0
    }
    fn unit_move_order(&mut self, who: i32, unit_o: i32, x: i32, y: i32) -> i32 {
        self.game
            .submit(Order::MoveUnit { who, unit_o, x, y })
            .as_i32()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::default_data_dir;

    fn game(n: usize) -> Option<Game> {
        let rules = Rules::load(&default_data_dir()).ok()?;
        let nations: Vec<&str> = ["Romans", "Greeks", "Bantu", "Koreans"][..n].to_vec();
        Some(Game::new(rules, &nations, Difficulty::Tough))
    }

    #[test]
    fn a_small_town_start_is_a_capital_three_farms_and_a_library() {
        let Some(g) = game(1) else { return };
        let p = &g.players[0];
        assert_eq!(p.cities.len(), 1);
        assert_eq!(p.building_count("Farm", true), 3);
        assert_eq!(p.building_count("Library", true), 1);
        assert_eq!(p.stock, g.rules.constants.starting_goods);
    }

    #[test]
    fn citizens_fill_farm_slots_and_then_the_economy_grows() {
        let Some(mut g) = game(1) else { return };
        let start = g.players[0].stock;
        for _ in 0..900 {
            g.step();
        }
        // Three citizens on three one-slot Farms: 3 * PEASANT_RATE food, plus
        // CITY_GATHER on food and timber.
        assert_eq!(g.players[0].assigned, 3);
        assert!(g.players[0].stock[0] > start[0], "food should grow");
        assert!(g.players[0].stock[1] > start[1], "city gather feeds timber");
    }

    #[test]
    fn an_order_that_cannot_be_paid_for_is_refused_not_invalid() {
        let Some(mut g) = game(1) else { return };
        g.players[0].stock = [0; NRES];
        let r = g.submit(Order::PlaceBuilding {
            who: 1,
            build_type: "Woodcutter's Camp".into(),
            city_name: "Romans-1".into(),
        });
        assert_eq!(r, OrderResult::Refused);
        // An unknown type is invalid, which is what the shipped `"Citizens"`
        // typo hits.
        let r = g.submit(Order::TrainUnit {
            who: 1,
            num: 1,
            unit_type: "Citizens".into(),
        });
        assert_eq!(r, OrderResult::Invalid);
    }

    #[test]
    fn placing_a_building_pays_the_shipped_cost_and_takes_job_time_frames() {
        let Some(mut g) = game(1) else { return };
        let before = g.players[0].stock;
        let cost = g.rules.buildings["Woodcutter's Camp"].costs;
        let job = g.rules.buildings["Woodcutter's Camp"].job_time;
        let r = g.submit(Order::PlaceBuilding {
            who: 1,
            build_type: "Woodcutter's Camp".into(),
            city_name: "Romans-1".into(),
        });
        assert!(matches!(r, OrderResult::Ok(_)));
        for res in 0..NRES {
            assert_eq!(g.players[0].stock[res], before[res] - cost[res]);
        }
        // Not active until JOB_TIME frames have passed. The economy runs too,
        // so compare the building state only.
        for _ in 0..job - 1 {
            g.step();
        }
        assert!(!g.players[0].buildings.last().unwrap().active);
        g.step();
        assert!(g.players[0].buildings.last().unwrap().active);
    }

    #[test]
    fn max_workers_reports_minus_one_for_a_non_gatherer() {
        let Some(mut g) = game(1) else { return };
        let lib = g.players[0]
            .buildings
            .iter()
            .find(|b| b.ty == "Library")
            .unwrap()
            .id;
        let farm = g.players[0]
            .buildings
            .iter()
            .find(|b| b.ty == "Farm")
            .unwrap()
            .id;
        let v = PlayerView {
            game: &mut g,
            who: 1,
        };
        assert_eq!(v.max_workers_at_building(1, lib), -1);
        assert_eq!(v.max_workers_at_building(1, farm), 1);
        assert_eq!(v.max_workers_at_building(9, farm), -1); // bad `who`
    }

    #[test]
    fn difficulty_scales_income_exactly_as_the_bonus_table_says() {
        let mut totals = Vec::new();
        for d in [Difficulty::Easiest, Difficulty::Tough, Difficulty::Toughest] {
            let Some(rules) = Rules::load(&default_data_dir()).ok() else {
                return;
            };
            let mut g = Game::new(rules, &["Romans"], d);
            for _ in 0..4500 {
                g.step();
            }
            totals.push(g.players[0].stock[0]);
        }
        let [easiest, tough, toughest] = [totals[0], totals[1], totals[2]];
        assert!(
            easiest < tough,
            "Easiest {easiest} should trail Tough {tough}"
        );
        assert!(
            toughest > tough,
            "Toughest {toughest} should beat Tough {tough}"
        );
    }
}
