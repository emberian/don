//! The Ancient-age economy of *Rise of Nations*, in integers, from measured
//! quantities only.
//!
//! Every constant below is either a live read of the `Constants` singleton out of a
//! running `riseofnations.exe` (`schema/live/rules-block-pid14644.txt`, decoded with
//! the PDB field layout in `schema/types.json`) or an instruction-level read of the
//! named function in `ron-bin/riseofnations.exe`. The PDB names are the shipped ones.
//!
//! # Units
//!
//! The engine carries income in **sixteenths of a resource per `gather_rate` frames**.
//! `Leader::do_gather` @ `0x006CE450`:
//!
//! ```text
//! 006ce7b9  mov  ecx, [constants + 0x27c]   ; gather_rate = 450
//! 006ce7c1  shl  ecx, 4                     ; period = 7200
//! 006ce7c5  idiv ecx                        ; whole = income / period, rem in edx
//! 006ce7de  add  ecx, edx                   ; LeaderDataEncrypt::leftover[r] += rem
//! 006ce810  ...  while (leftover >= period) { leftover -= period; whole++ }
//! 006ce855  add  eax, whole                 ; the resource bucket += whole
//! ```
//!
//! and `ScenarioFuncSet::gather_rate` @ `0x009E9136` returns `income >> 4`, i.e. the
//! number the AI scripts and the HUD see is resources per 30 s.
//!
//! # The commerce clamp — the load-bearing measurement
//!
//! `Leader::calc_resource_caps` @ `0x006CE900` writes `commerce_cap[epoch[2]]` into
//! `LeaderDataEncrypt::resource_cap[r]` and then, at the very end of the per-resource
//! loop, scales it into income units:
//!
//! ```text
//! 006cee70  mov  eax, [ecx + 0x30]   ; resource_cap[r], encrypted
//! 006cee73  xor  eax, 0x1281         ; decrypt
//! 006cee78  shl  eax, 4              ; x16  <-- the clamp is in INCOME units
//! 006cee85  mov  [ecx + 0x30], eax   ; store back
//! 006cee8b  cmp  ebx, 7 ; jl         ; r = 0..6, matching resource_cap[7]
//! ```
//!
//! so the Ancient-age ceiling is **70 resources per 30 s per resource**, not 70/16.
//! Prior analysis could not settle this and carried it as the model's largest
//! assumption; it is measured.

/// Resource slot order, from `Constants::starting_goods` prose and the three
/// independent witnesses in `docs/tracks/build-order-analytics.md` §2.3.
pub const FOOD: usize = 0;
pub const TIMBER: usize = 1;
pub const WEALTH: usize = 2;
pub const KNOWLEDGE: usize = 3;
pub const METAL: usize = 4;
pub const OIL: usize = 5;

/// `Constants::gather_rate` @ `+0x27C` — live value 450 frames.
pub const GATHER_RATE: i64 = 450;
/// `gather_rate << 4` — the accumulator period, `0x006CE7C1`.
pub const PERIOD: i64 = GATHER_RATE * 16;
/// `Constants::peasant_rate` @ `+0x280` — live 2560 = 10.0 in 8.8 fixed point.
pub const PEASANT_RATE: i64 = 2560;
/// One worked gather slot, in income units: `16 * peasant_rate / 256`.
pub const SLOT16: i64 = 16 * PEASANT_RATE / 256; // 160 = 10 resources / period
/// `Constants::city_gather` @ `+0x264` — live `[10,10,0,0,0,0]`, added `<<4` per city.
pub const CITY_GATHER: [i64; 6] = [10, 10, 0, 0, 0, 0];
/// `Constants::basic_gather` @ `+0x24C` — live all zero.
pub const BASIC_GATHER: [i64; 6] = [0; 6];
/// `Constants::starting_goods` @ `+0x234`.
pub const STARTING_GOODS: [i64; 6] = [200, 200, 100, 100, 100, 100];
/// `Constants::commerce_cap` @ `+0x400`, indexed by `LeaderDataEncrypt::epoch[2]`.
pub const COMMERCE_CAP: [i64; 8] = [70, 100, 150, 200, 260, 320, 400, 500];
/// `Constants::pop_cap` @ `+0x3C4`, indexed by `epoch[0]`.
pub const POP_CAP: [i64; 8] = [25, 50, 75, 100, 125, 150, 175, 200];
/// Hardcoded knowledge cap: `0x1166 ^ 0x1281 = 999` at `0x006CE92C`.
pub const KNOWLEDGE_CAP: i64 = 999;
/// `mov eax, 0x3e70` at `0x006CE706` — 16000 income units = 1000 resources / period.
pub const GLOBAL_CEILING: i64 = 16000;
/// `Constants::market_taxes` @ `+0x32C` — `CityData::get_taxes` @ `0x00737B50`.
pub const MARKET_TAXES: i64 = 10;
/// `Constants::university_literacy` @ `+0x34C` — `CityData::get_literacy` @ `0x00737C00`.
pub const UNIVERSITY_LITERACY: i64 = 10;
/// `Constants::farms_per_city_base` @ `+0x2A0`.
pub const FARMS_PER_CITY: i64 = 5;
/// `Constants::unit_cost_factor` / `build_cost_factor` / `tech_cost_factor`, all 10.
pub const COST_FACTOR: i64 = 10;
/// `Constants::unit_worker_ramp_max` @ `+0x398` — 500 %.
pub const WORKER_RAMP: i64 = 500;
/// The engine's own frames→seconds conversion, `idiv 15` at `0x005924CF`.
/// (`TurnControl::timings` puts a Normal tick at 67 ms, i.e. 14.9 Hz; the two agree
/// to within 0.5 % and 450 frames is 30 s either way.)
pub const FPS: i64 = 15;

/// The four library columns. A tech's `cat` field in the shipped rules is its column,
/// and `LeaderDataEncrypt::epoch[cat]` is the level counter the engine indexes the
/// column's limit with: `epoch[0]`→`pop_cap`, `epoch[1]+1`→city limit
/// (`LeaderData::get_city_limit` @ `0x006D6130`), `epoch[2]`→`commerce_cap`
/// (read at `0x006CE911` from `data_encrypted + 0xF0` = `epoch[2]`).
pub const MILITARY: usize = 0;
pub const CIVIC: usize = 1;
pub const COMMERCE: usize = 2;
pub const SCIENCE: usize = 3;

/// What a purchasable thing costs and how long it takes.
///
/// `cost` is the raw `COST` field from the live type tables; the engine multiplies it
/// by the cost factor in `TypeData::get_cost` @ `0x006645B4`. `support`/`support_cost`
/// are the two `(resource, amount)` ramp pairs read at `0x0066541C`.
#[derive(Clone, Copy, Debug)]
pub struct Item {
    pub name: &'static str,
    pub kind: Kind,
    pub cost: [i64; 6],
    pub job_time: i64,
    pub support: [i8; 2],
    pub support_cost: [i64; 2],
    /// Library column for techs; ignored otherwise.
    pub cat: usize,
    /// True for the age advances, which move `age` rather than a column level.
    pub is_age: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Unit,
    Building,
    Tech,
}

macro_rules! item {
    ($n:expr, $k:expr, $c:expr, $jt:expr, $s:expr, $sc:expr, $cat:expr, $age:expr) => {
        Item {
            name: $n,
            kind: $k,
            cost: $c,
            job_time: $jt,
            support: $s,
            support_cost: $sc,
            cat: $cat,
            is_age: $age,
        }
    };
}

/// The Ancient-age opening catalogue, from `schema/live/{unit,building,tech}-attributes.txt`
/// (read out of a running process) via `analysis/derived.json`.
pub const CATALOGUE: &[Item] = &[
    item!(
        "Citizen",
        Kind::Unit,
        [2, 0, 0, 0, 0, 0],
        50,
        [0, -1],
        [1, 0],
        0,
        false
    ),
    item!(
        "Farm",
        Kind::Building,
        [0, 4, 0, 0, 0, 0],
        150,
        [1, -1],
        [4, 0],
        0,
        false
    ),
    item!(
        "Woodcutter's Camp",
        Kind::Building,
        [5, 0, 0, 0, 0, 0],
        150,
        [0, -1],
        [20, 0],
        0,
        false
    ),
    item!(
        "Mine",
        Kind::Building,
        [0, 5, 0, 0, 0, 0],
        150,
        [1, -1],
        [20, 0],
        0,
        false
    ),
    item!(
        "Small City",
        Kind::Building,
        [1, 1, 0, 0, 0, 0],
        600,
        [0, 1],
        [50, 50],
        0,
        false
    ),
    item!(
        "Market",
        Kind::Building,
        [0, 8, 0, 0, 0, 0],
        420,
        [1, 0],
        [30, 0],
        0,
        false
    ),
    item!(
        "Library",
        Kind::Building,
        [0, 6, 0, 0, 0, 0],
        420,
        [2, 1],
        [0, 20],
        0,
        false
    ),
    item!(
        "University",
        Kind::Building,
        [0, 6, 3, 0, 0, 0],
        420,
        [1, 2],
        [20, 20],
        0,
        false
    ),
    item!(
        "Barracks",
        Kind::Building,
        [0, 12, 0, 0, 0, 0],
        420,
        [1, -1],
        [25, 0],
        0,
        false
    ),
    item!(
        "Barter",
        Kind::Tech,
        [6, 6, 0, 0, 0, 0],
        200,
        [-1, -1],
        [0, 0],
        COMMERCE,
        false
    ),
    item!(
        "City State",
        Kind::Tech,
        [12, 0, 0, 0, 0, 0],
        200,
        [-1, -1],
        [0, 0],
        CIVIC,
        false
    ),
    item!(
        "Written Word",
        Kind::Tech,
        [0, 12, 5, 0, 0, 0],
        200,
        [-1, -1],
        [0, 0],
        SCIENCE,
        false
    ),
    item!(
        "The Art of War",
        Kind::Tech,
        [12, 0, 0, 0, 0, 0],
        200,
        [-1, -1],
        [0, 0],
        MILITARY,
        false
    ),
    item!(
        "Classical Age",
        Kind::Tech,
        [25, 0, 0, 0, 0, 0],
        400,
        [-1, -1],
        [0, 0],
        SCIENCE,
        true
    ),
];

pub fn item(name: &str) -> &'static Item {
    CATALOGUE
        .iter()
        .find(|i| i.name == name)
        .unwrap_or_else(|| panic!("unknown item {name}"))
}

/// Quantities the binary did **not** tell us. Every field has a sweep in
/// `docs/tracks/analytics-v2.md`.
#[derive(Clone, Copy, Debug)]
pub struct Assume {
    /// Gather slots per Farm. `farms_per_city_base = 5` and the shipped AI's
    /// one-farm-one-worker bookkeeping force 1.
    pub slots_farm: i64,
    /// Gather slots per Woodcutter's Camp. `aibestbuildlibrary.bhs::place_woodcutter`
    /// destroys and re-places a camp whose `max_workers_at_building` is below
    /// `min_size = 5`, so 5 is the designers' own target.
    pub slots_wood: i64,
    pub slots_mine: i64,
    /// `buildingrules.xml` documents `JOB_TIME` as the time for **one** citizen; the
    /// k-citizen speedup shape is not derived. Linear is the natural reading.
    pub max_builders: i64,
    /// Cost-ramp ceiling class for buildings. `TypeData::get_cost` picks it by unit
    /// class and a zero ceiling means NO ceiling (`test esi,esi` @ `0x00665452`);
    /// which class a building falls into is not derived.
    pub building_ramp_pct: i64,
    pub start_citizens: i64,
    pub start_farms: i64,
    pub start_woodcamps: i64,
    pub walk_frames: i64,
}

impl Default for Assume {
    fn default() -> Self {
        Assume {
            slots_farm: 1,
            slots_wood: 5,
            slots_mine: 4,
            max_builders: 4,
            building_ramp_pct: 0,
            start_citizens: 5,
            start_farms: 3,
            start_woodcamps: 1,
            walk_frames: 0,
        }
    }
}

/// `TypeData::get_cost` @ `0x00664090`, reduced to the shipped-data case.
///
/// `cost[r] = COST[r]*factor + min(SUPPORTVALUE*count, COST[r]*factor*ramp/100)`,
/// where `count` is owned **plus queued** (`0x00665966`) and a ceiling of zero means
/// no ceiling (`0x00665452`).
pub fn cost_of(it: &Item, count: i64, ramp_pct: i64) -> [i64; 6] {
    let mut c = [0i64; 6];
    for r in 0..6 {
        c[r] = it.cost[r] * COST_FACTOR;
    }
    for k in 0..2 {
        let r = it.support[k];
        if r < 0 {
            continue;
        }
        let r = r as usize;
        let mut v = it.support_cost[k] * count;
        if ramp_pct != 0 {
            let lim = c[r] * ramp_pct / 100;
            if lim != 0 && lim < v {
                v = lim;
            }
        }
        c[r] += v;
    }
    c
}

/// One queued item awaiting completion.
#[derive(Clone, Copy, Debug)]
pub struct Pending {
    pub done: i64,
    pub idx: usize,
}

/// The counters the model tracks. Each holds **owned + queued**, which is what the
/// engine charges cost ramps against and what the AI's `num_type_with_queued` reads;
/// income and gather slots use `built()`, which subtracts the pending queue.
#[derive(Clone, Debug)]
pub struct World {
    pub a: Assume,
    pub frame: i64,
    pub stock: [i64; 6],
    pub leftover: [i64; 6],
    pub counts: [i64; 16],
    pub on: [i64; 6],
    pub epoch: [i64; 4],
    pub age: i64,
    pub techs: u32,
    pub t_free: i64,
    pub b_free: i64,
    pub l_free: i64,
    pub builders: i64,
    pub pend: Vec<Pending>,
    pub log: Vec<(i64, i64, usize)>,
}

fn idx_of(name: &str) -> usize {
    CATALOGUE.iter().position(|i| i.name == name).unwrap()
}

impl World {
    pub fn new(a: Assume) -> World {
        let mut w = World {
            a,
            frame: 0,
            stock: STARTING_GOODS,
            leftover: [0; 6],
            counts: [0; 16],
            on: [0; 6],
            epoch: [0; 4],
            age: 0,
            techs: 0,
            t_free: 0,
            b_free: 0,
            l_free: 0,
            builders: 0,
            pend: Vec::new(),
            log: Vec::new(),
        };
        w.counts[idx_of("Citizen")] = a.start_citizens;
        w.counts[idx_of("Farm")] = a.start_farms;
        w.counts[idx_of("Woodcutter's Camp")] = a.start_woodcamps;
        // The starting settlement IS a Small City (type 414, where Citizens are
        // trained), so the second city already pays one step of the 50/50 cost ramp.
        w.counts[idx_of("Small City")] = 1;
        w.reseat();
        w
    }

    pub fn count(&self, name: &str) -> i64 {
        self.counts[idx_of(name)]
    }

    pub fn npend(&self, i: usize) -> i64 {
        self.pend.iter().filter(|p| p.idx == i).count() as i64
    }

    pub fn built(&self, name: &str) -> i64 {
        let i = idx_of(name);
        self.counts[i] - self.npend(i)
    }

    pub fn has_tech(&self, name: &str) -> bool {
        self.techs & (1 << idx_of(name)) != 0
    }

    pub fn tech_landed(&self, name: &str) -> bool {
        let i = idx_of(name);
        self.has_tech(name) && self.npend(i) == 0
    }

    pub fn pop_cap(&self) -> i64 {
        POP_CAP[self.epoch[MILITARY].min(7) as usize]
    }

    /// `LeaderData::get_city_limit` @ `0x006D6130`: `epoch[1] + 1`.
    pub fn city_limit(&self) -> i64 {
        self.epoch[CIVIC] + 1
    }

    /// `Leader::calc_resource_caps`: `commerce_cap[epoch[2]] * 16`, knowledge 999*16.
    pub fn cap16(&self) -> [i64; 6] {
        let c = COMMERCE_CAP[self.epoch[COMMERCE].min(7) as usize] * 16;
        let mut out = [c; 6];
        out[KNOWLEDGE] = KNOWLEDGE_CAP * 16;
        out
    }

    fn slots(&self) -> [i64; 6] {
        let mut s = [0i64; 6];
        s[FOOD] = self.built("Farm") * self.a.slots_farm;
        s[TIMBER] = self.built("Woodcutter's Camp") * self.a.slots_wood;
        s[METAL] = self.built("Mine") * self.a.slots_mine;
        s[KNOWLEDGE] = self.built("University");
        s
    }

    /// Income that costs no worker: cities, markets, universities.
    pub fn free_income16(&self, r: usize) -> i64 {
        let mut v = BASIC_GATHER[r] * 16 + CITY_GATHER[r] * 16 * self.built("Small City");
        if r == WEALTH {
            v += MARKET_TAXES * 16 * self.built("Market");
        }
        if r == KNOWLEDGE {
            v += UNIVERSITY_LITERACY * 16 * self.built("University");
        }
        v
    }

    /// How many more workers on `r` still produce anything before the clamp bites.
    pub fn useful_slots(&self, r: usize) -> i64 {
        let base = self.free_income16(r);
        let cap = self.cap16()[r];
        if base >= cap {
            0
        } else {
            (cap - base + SLOT16 - 1) / SLOT16
        }
    }

    pub fn reseat(&mut self) {
        let sl = self.slots();
        let mut free = self.built("Citizen") - self.builders;
        self.on = [0; 6];
        for &r in &[FOOD, TIMBER, KNOWLEDGE, METAL] {
            let take = sl[r].min(free).min(self.useful_slots(r)).max(0);
            self.on[r] = take;
            free -= take;
        }
        for &r in &[FOOD, TIMBER, METAL] {
            let room = sl[r] - self.on[r];
            let take = room.min(free).max(0);
            self.on[r] += take;
            free -= take;
        }
    }

    /// `Leader::calc_gather` → `Leader::do_gather`, in 1/16 resource per 450 frames.
    pub fn income16(&self) -> [i64; 6] {
        let mut out = [0i64; 6];
        for r in 0..6 {
            let v = 16 * self.on[r] * PEASANT_RATE / 256;
            out[r] = v.max(0) + self.free_income16(r);
        }
        let cap = self.cap16();
        for r in 0..6 {
            if out[r] > cap[r] {
                out[r] = cap[r]; // 0x006CE512 / 0x006CE61C
            }
            if out[r] > GLOBAL_CEILING {
                out[r] = GLOBAL_CEILING; // 0x006CE706
            }
        }
        out
    }

    /// Resources per 30 s — exactly what `ScenarioFuncSet::gather_rate` returns.
    pub fn rate(&self) -> [i64; 6] {
        let i = self.income16();
        let mut o = [0i64; 6];
        for r in 0..6 {
            o[r] = i[r] / 16;
        }
        o
    }

    fn tick(&mut self, frames: i64) {
        if frames <= 0 {
            self.frame += frames.max(0);
            return;
        }
        let inc = self.income16();
        for r in 0..6 {
            if inc[r] <= 0 {
                continue;
            }
            let tot = inc[r] * frames + self.leftover[r];
            self.stock[r] += tot / PERIOD;
            self.leftover[r] = tot % PERIOD;
        }
        self.frame += frames;
    }

    fn flush(&mut self) {
        let landed: Vec<Pending> = self
            .pend
            .iter()
            .copied()
            .filter(|p| p.done <= self.frame)
            .collect();
        let crew = self.builders > 0 && self.b_free <= self.frame;
        if landed.is_empty() && !crew {
            return;
        }
        for p in &landed {
            let it = &CATALOGUE[p.idx];
            if it.kind == Kind::Tech {
                if it.is_age {
                    self.age += 1;
                } else {
                    self.epoch[it.cat] += 1;
                }
            }
        }
        self.pend.retain(|p| p.done > self.frame);
        if crew {
            self.builders = 0;
        }
        self.reseat();
    }

    fn next_event(&self, target: i64) -> Option<i64> {
        let mut best: Option<i64> = None;
        if self.builders > 0 && self.frame < self.b_free && self.b_free < target {
            best = Some(self.b_free);
        }
        for p in &self.pend {
            if self.frame < p.done && p.done < target && best.map_or(true, |b| p.done < b) {
                best = Some(p.done);
            }
        }
        best
    }

    /// The next frame before `before` at which something completes and the wish list
    /// could change. Used by the player to re-plan instead of committing early.
    pub fn next_change(&self, before: i64) -> Option<i64> {
        self.next_event(before)
    }

    pub fn advance(&mut self, frames: i64) {
        let target = self.frame + frames.max(0);
        self.flush();
        while self.frame < target {
            match self.next_event(target) {
                None => {
                    let d = target - self.frame;
                    self.tick(d);
                    break;
                }
                Some(ev) => {
                    let d = ev - self.frame;
                    self.tick(d);
                    self.flush();
                }
            }
        }
        self.flush();
    }

    pub fn advance_to(&mut self, frame: i64) {
        if frame > self.frame {
            self.advance(frame - self.frame);
        } else {
            self.flush();
        }
    }

    /// Frames until `cost` is affordable, honouring every income step in between.
    pub fn frames_to_afford(&self, cost: &[i64; 6], limit: i64) -> Option<i64> {
        let mut probe = self.clone();
        let mut step = 0i64;
        for _ in 0..256 {
            probe.flush();
            let mut need = [0i64; 6];
            let mut any = false;
            for r in 0..6 {
                need[r] = (cost[r] - probe.stock[r]).max(0);
                if need[r] > 0 {
                    any = true;
                }
            }
            if !any {
                return Some(step);
            }
            let inc = probe.income16();
            let mut worst: Option<i64> = Some(0);
            for r in 0..6 {
                if need[r] == 0 {
                    continue;
                }
                if inc[r] <= 0 {
                    worst = None;
                    break;
                }
                let f = (need[r] * PERIOD - probe.leftover[r] + inc[r] - 1) / inc[r];
                worst = Some(worst.unwrap().max(f.max(0)));
            }
            let probe_target = probe.frame + worst.unwrap_or(limit) + 1;
            if let Some(ev) = probe.next_event(probe_target) {
                let jump = ev - probe.frame;
                probe.advance(jump);
                step += jump;
                if step > limit {
                    return None;
                }
                continue;
            }
            let w = worst?;
            step += w;
            return if step <= limit { Some(step) } else { None };
        }
        None
    }

    pub fn horizon(&self) -> i64 {
        let mut h = self
            .frame
            .max(self.t_free)
            .max(self.b_free)
            .max(self.l_free);
        for p in &self.pend {
            h = h.max(p.done);
        }
        h
    }

    pub fn settle(&self) -> World {
        let mut w = self.clone();
        let h = w.horizon();
        w.advance_to(h);
        w
    }

    pub fn legal(&self, name: &str) -> bool {
        let it = item(name);
        match name {
            "Citizen" => self.count("Citizen") < self.pop_cap(),
            "Small City" => self.count("Small City") < self.city_limit(),
            "Farm" => self.count("Farm") < FARMS_PER_CITY * self.count("Small City"),
            _ => {
                if it.kind == Kind::Tech {
                    if self.has_tech(name) {
                        return false;
                    }
                    // Ancient-age prerequisites inside this catalogue: the Market needs
                    // Barter; nothing else in the opening set has one that binds.
                    return true;
                }
                if name == "Market" {
                    return self.tech_landed("Barter");
                }
                true
            }
        }
    }

    fn action_time(&self, name: &str) -> i64 {
        let it = item(name);
        match it.kind {
            Kind::Unit => it.job_time,
            Kind::Tech => it.job_time,
            Kind::Building => {
                let k = self
                    .a
                    .max_builders
                    .min((self.count("Citizen") - self.builders).max(1));
                it.job_time / k + self.a.walk_frames
            }
        }
    }

    fn action_cost(&self, name: &str) -> [i64; 6] {
        let it = item(name);
        match it.kind {
            Kind::Unit => cost_of(it, self.count("Citizen"), WORKER_RAMP),
            Kind::Building => cost_of(it, self.count(name), self.a.building_ramp_pct),
            Kind::Tech => cost_of(it, 0, 0),
        }
    }

    /// The frame at which `name` could be queued: its server free, and paid for.
    /// `None` if it can never be paid for from here.
    pub fn ready_at(&self, name: &str) -> Option<i64> {
        let it = item(name);
        let free = match it.kind {
            Kind::Unit => self.t_free,
            Kind::Building => self.b_free,
            Kind::Tech => self.l_free,
        };
        let mut probe = self.clone();
        probe.advance_to(free.max(probe.frame));
        let cost = probe.action_cost(name);
        probe
            .frames_to_afford(&cost, 90_000)
            .map(|w| probe.frame + w)
    }

    /// Queue `name` at the earliest frame its server is free **and** it is affordable.
    /// Returns false (leaving the world untouched) if it can never be paid for.
    pub fn buy(&mut self, name: &str) -> bool {
        let it = item(name);
        let i = idx_of(name);
        let server = match it.kind {
            Kind::Unit => 0,
            Kind::Building => 1,
            Kind::Tech => 2,
        };
        let free = [self.t_free, self.b_free, self.l_free][server];
        let dur = self.action_time(name);
        let mut probe = self.clone();
        probe.advance_to(free.max(probe.frame));
        let cost = probe.action_cost(name);
        let wait = match probe.frames_to_afford(&cost, 90_000) {
            Some(w) => w,
            None => return false,
        };
        probe.advance(wait);
        for r in 0..6 {
            probe.stock[r] -= cost[r];
        }
        let done = probe.frame + dur;
        match server {
            0 => probe.t_free = done,
            1 => {
                probe.b_free = done;
                probe.builders = probe
                    .a
                    .max_builders
                    .min((probe.built("Citizen") - 1).max(0));
            }
            _ => probe.l_free = done,
        }
        probe.log.push((probe.frame, done, i));
        probe.counts[i] += 1;
        if it.kind == Kind::Tech {
            probe.techs |= 1 << i;
        }
        probe.pend.push(Pending { done, idx: i });
        probe.reseat();
        *self = probe;
        true
    }
}

pub fn mmss(f: i64) -> String {
    let t = f / FPS;
    format!("{}:{:02}", t / 60, t % 60)
}
