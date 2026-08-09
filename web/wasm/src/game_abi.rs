//! C ABI for the playable browser client.
//!
//! The authoritative state behind this surface is [`don_sim::tick::Sim`].  The arrays
//! exposed to JavaScript are render/query projections rebuilt from that core; they are not
//! a second game world.  Save/load delegates directly to `don_sim`'s deterministic format
//! and atomically swaps the core only after a complete, bounded decode.

use crate::game::{gap, PlayData, FIRST_BUILDING, PLAYERS, QUEUE_MAX, W_CELLS};
use crate::gamedata::GameData;
use crate::wire_gen;
use don_sim::deviations::{Deviation, ModeConfig, Surface};
use don_sim::objects::{Band, BUILD_BAND_BASE, WALL_BAND_BASE};
use don_sim::order::{Order, OrderIndex};
use don_sim::systems::production::runtime::LiveProductionType;
use don_sim::systems::production::{self, BuildData, BuildQueueEntry};
use don_sim::systems::save_load::{load_sim, save_sim};
use don_sim::tick::Sim as CoreSim;
use don_sim::Handle;
use std::cell::UnsafeCell;

const CMD_SCRATCH: usize = 16 * 1024;
const PICK_MAX: usize = 4096;
const SAVE_LIMIT: usize = 256 * 1024 * 1024;
const MAP_TILES: i32 = W_CELLS * don_sim::systems::map_terrain::TILES_PER_WCELL;
const TILE_COORD: i32 = don_sim::world::COORD_PER_TILE;
const CORE_SUBTILE: i32 = don_sim::world::SUBTILE;
const MAP_SPAN: i32 = MAP_TILES * TILE_COORD;
const STARTING_GOODS_RULE: usize = 564;
const BUILD_VIEW_ID_BASE: i32 = 0x4000;
const BUILD_PROJECTION_CAPACITY: usize = (WALL_BAND_BASE - BUILD_BAND_BASE) as usize;
/// `i32`s per player in the block [`game_players_ptr`] exposes.
pub const PLAYER_FIELDS: usize = 34;

pub mod capability {
    pub const CORE_SAVE: u32 = 1 << 0;
    pub const CORE_LOAD: u32 = 1 << 1;
    pub const MOVE: u32 = 1 << 2;
    pub const ATTACK: u32 = 1 << 3;
    pub const HALT: u32 = 1 << 4;
    pub const TRAIN: u32 = 1 << 5;
}

struct Stage(UnsafeCell<Vec<u8>>);
// SAFETY: wasm32-unknown-unknown and the native ABI tests are single-threaded.
unsafe impl Sync for Stage {}
static GAMEDATA: Stage = Stage(UnsafeCell::new(Vec::new()));
static PLAYDATA: Stage = Stage(UnsafeCell::new(Vec::new()));

#[allow(clippy::mut_from_ref)]
fn stage(s: &'static Stage) -> &'static mut Vec<u8> {
    // SAFETY: see `Stage`.
    unsafe { &mut *s.0.get() }
}

/// One core simulation plus fixed browser-facing projections and bounded staging buffers.
pub struct Game {
    core: CoreSim,
    gd: GameData,
    play: PlayData,
    x: Vec<i32>,
    y: Vec<i32>,
    tag: Vec<u32>,
    ids: Vec<i32>,
    view_live: usize,
    cmd: Vec<u8>,
    pick: Vec<i16>,
    info: Vec<i32>,
    players: Vec<i32>,
    products: Vec<i32>,
    gaps: Vec<u32>,
    pending: Vec<u8>,
    selection: [Vec<u32>; PLAYERS],
    commands_seen: u64,
    orders_applied: u64,
    terrain_version: u32,
    save_bytes: Vec<u8>,
    load_bytes: Vec<u8>,
    error: Vec<u8>,
    start_x: [i32; PLAYERS],
    start_y: [i32; PLAYERS],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TrainingRefusal {
    NoProducer,
    QueueFull,
    WrongAge,
    CannotAfford,
    UnsupportedCost,
    CapacityFull,
}

impl TrainingRefusal {
    fn gap(self) -> usize {
        match self {
            TrainingRefusal::NoProducer => gap::NOT_A_PRODUCER,
            TrainingRefusal::QueueFull => gap::QUEUE_FULL,
            TrainingRefusal::WrongAge => gap::WRONG_AGE,
            TrainingRefusal::CannotAfford | TrainingRefusal::UnsupportedCost => gap::CANNOT_AFFORD,
            TrainingRefusal::CapacityFull => gap::CAPACITY_FULL,
        }
    }
}

impl Game {
    fn new(seed: u64) -> Game {
        let gd = GameData::parse(stage(&GAMEDATA)).unwrap_or_else(GameData::synthetic);
        let play = PlayData::parse(stage(&PLAYDATA)).unwrap_or_else(PlayData::empty);
        let mut core = CoreSim::new(seed, W_CELLS as u16);
        core.map.world.seed = seed as i32;
        let mut start_x = [0; PLAYERS];
        let mut start_y = [0; PLAYERS];
        let inset = MAP_TILES / 5;
        let citizen = 50;
        let escort = gd
            .roster
            .first()
            .and_then(|&r| gd.units.get(r as usize))
            .map(|u| u.type_id)
            .unwrap_or(citizen);
        for p in 0..PLAYERS {
            let (tx, ty) = match p {
                0 => (inset, inset),
                1 => (MAP_TILES - inset, MAP_TILES - inset),
                2 => (MAP_TILES - inset, inset),
                _ => (inset, MAP_TILES - inset),
            };
            start_x[p] = tx * TILE_COORD;
            start_y[p] = ty * TILE_COORD;
            // Activate only the core object band, not a leader host the current save
            // tranche cannot yet restore.  Units therefore take part in step 14 while
            // save/load can still roundtrip the supported pre-step state.
            core.world.objects.set_active(p, true);
            for resource in 0..core.leaders[p].econ.stockpile.len() {
                let value = play.rules.at(STARTING_GOODS_RULE + resource * 4);
                core.leaders[p].econ.stockpile[resource] = value;
                core.production_runtime.leaders[p].resources[resource] = value;
            }
            if let Some(building) = play.bld(FIRST_BUILDING) {
                let local_o =
                    BUILD_BAND_BASE + core.world.objects.slot(p).band(Band::Build).len() as u32;
                let mut build = BuildData {
                    flags: production::flag::VALID
                        | production::flag::STARTED
                        | production::flag::ACTIVE,
                    myhits: building.hits.max(1),
                    construct_hits: building.hits.max(1),
                    job_counter: building.job_time.max(1) as u32,
                    constr_time: building.job_time.max(1) as u32,
                    orig_type: FIRST_BUILDING,
                    gather_down: -1,
                    city: -1,
                    city_down: -1,
                    wonder: -1,
                    dock: -1,
                    attack_ox: -1,
                    attack_whom: -1,
                    ..BuildData::default()
                };
                build.other[0x28..0x2a].copy_from_slice(&(-1i16).to_le_bytes());
                build.other[production::off::OBJECT_ID..production::off::OBJECT_ID + 2]
                    .copy_from_slice(&(local_o as i16).to_le_bytes());
                build.other[production::off::X_INTERNAL..production::off::X_INTERNAL + 4]
                    .copy_from_slice(&(start_x[p] ^ 0x63637).to_le_bytes());
                build.other[production::off::Y_INTERNAL..production::off::Y_INTERNAL + 4]
                    .copy_from_slice(&(start_y[p] ^ 0x63637).to_le_bytes());
                core.spawn_build(p, build);
            }
            for k in 0..6 {
                let x = start_x[p] + (k as i32 - 3) * CORE_SUBTILE;
                let y = start_y[p] + 3 * CORE_SUBTILE;
                if let Some(h) = core.spawn_unit(p, citizen, x, y, 4) {
                    let _ = h;
                }
            }
            for k in 0..2 {
                let x = start_x[p] + (k * 2 - 1) * 2 * CORE_SUBTILE;
                let y = start_y[p] - 3 * CORE_SUBTILE;
                let _ = core.spawn_unit(p, escort, x, y, 5);
            }
        }
        let capacity = core.world.capacity() as usize + BUILD_PROJECTION_CAPACITY;
        let mut game = Game {
            core,
            gd,
            play,
            x: vec![0; capacity],
            y: vec![0; capacity],
            tag: vec![0; capacity],
            ids: vec![-1; capacity],
            view_live: 0,
            cmd: vec![0; CMD_SCRATCH],
            pick: vec![0; PICK_MAX],
            info: vec![0; 32],
            players: vec![0; PLAYERS * PLAYER_FIELDS],
            products: vec![0; 512],
            gaps: vec![0; gap::COUNT],
            pending: Vec::with_capacity(4096),
            selection: std::array::from_fn(|_| Vec::with_capacity(128)),
            commands_seen: 0,
            orders_applied: 0,
            terrain_version: 1,
            save_bytes: Vec::new(),
            load_bytes: Vec::new(),
            error: Vec::new(),
            start_x,
            start_y,
        };
        game.install_production_facts();
        game.refresh();
        game
    }

    fn row_for_id(&self, id: u32) -> Option<usize> {
        self.core
            .world
            .handles()
            .iter()
            .position(|&candidate| candidate == id)
    }

    fn build_view_id(row: usize) -> Option<i32> {
        i32::try_from(row)
            .ok()
            .and_then(|row| BUILD_VIEW_ID_BASE.checked_add(row))
    }

    fn build_row_for_id(&self, id: u32) -> Option<usize> {
        let row = i32::try_from(id).ok()?.checked_sub(BUILD_VIEW_ID_BASE)?;
        let row = usize::try_from(row).ok()?;
        self.core
            .builds
            .get(row)
            .is_some_and(BuildData::is_valid)
            .then_some(row)
    }

    fn build_type(&self, row: usize) -> i32 {
        self.core
            .production_runtime
            .build_types
            .get(row)
            .and_then(|value| *value)
            .unwrap_or_else(|| {
                self.core
                    .builds
                    .get(row)
                    .map_or(-1, |build| build.orig_type)
            })
    }

    fn object_owned(&self, id: u32, owner: usize) -> bool {
        self.row_for_id(id)
            .is_some_and(|row| self.owner_at(row) == owner)
            || self
                .build_row_for_id(id)
                .and_then(|row| self.core.builds.get(row))
                .is_some_and(|build| build.who as usize == owner)
    }

    fn owner_at(&self, row: usize) -> usize {
        self.core.world.owner()[row].max(0) as usize
    }

    fn type_at(&self, row: usize) -> i32 {
        self.core.unit_type.get(row).copied().unwrap_or(0)
    }

    fn selected(&self, owner: usize, id: u32) -> bool {
        self.selection
            .get(owner)
            .is_some_and(|ids| ids.iter().any(|&selected| selected == id))
    }

    fn install_production_facts(&mut self) {
        self.core.production_runtime = Default::default();
        if let Some(building) = self.play.bld(FIRST_BUILDING) {
            let mut facts =
                LiveProductionType::in_place_building(FIRST_BUILDING, building.job_time.max(1));
            facts.object_masks = building.obj_masks as u32;
            self.core.production_runtime.install_type(facts);
        }
        for type_id in self.play.products_of(FIRST_BUILDING) {
            let Some(unit) = self.play.unit(type_id) else {
                continue;
            };
            let mut facts =
                LiveProductionType::ordinary_unit(type_id, unit.job_time.max(1), unit.pop.max(0));
            if let Some(record) = self
                .gd
                .index_of_type(type_id)
                .and_then(|index| self.gd.units.get(index))
            {
                if record.domain != 0 {
                    continue;
                }
                facts.unit_flags = record.unit_flags as u32;
                facts.object_masks = record.obj_masks as u32;
            }
            self.core.production_runtime.install_type(facts);
        }
        for (row, build) in self.core.builds.iter().enumerate() {
            if !build.is_valid() {
                continue;
            }
            self.core
                .production_runtime
                .register_build(row, build.orig_type);
            let owner = build.who as usize;
            if owner >= self.core.production_runtime.leaders.len() {
                continue;
            }
            for entry in build.queue.entries.iter().take(build.queue.queued as usize) {
                if let Ok(type_index) = usize::try_from(entry.type_index) {
                    if let Some(count) = self.core.production_runtime.leaders[owner]
                        .queued_counts
                        .get_mut(type_index)
                    {
                        *count = count.wrapping_add(1);
                    }
                }
            }
        }
        for owner in 0..PLAYERS {
            self.core.production_runtime.leaders[owner].resources =
                self.core.leaders[owner].econ.stockpile;
        }
        self.core.production_runtime.world_population = self.core.world.live_count() as i32;
        for row in 0..self.core.world.live_count() as usize {
            let owner = self.owner_at(row);
            let type_id = self.type_at(row);
            if owner >= self.core.production_runtime.leaders.len() {
                continue;
            }
            if let Ok(type_index) = usize::try_from(type_id) {
                if let Some(count) = self.core.production_runtime.leaders[owner]
                    .unit_counts
                    .get_mut(type_index)
                {
                    *count = count.wrapping_add(1);
                }
            }
            if let Some(unit) = self.play.unit(type_id) {
                self.core.production_runtime.leaders[owner].control =
                    self.core.production_runtime.leaders[owner]
                        .control
                        .wrapping_add(unit.pop.max(0));
            }
        }
    }

    fn refresh(&mut self) {
        let live = self.core.world.live_count() as usize;
        let needed = self.core.world.capacity() as usize + BUILD_PROJECTION_CAPACITY;
        if self.x.len() < needed {
            self.x.resize(needed, 0);
            self.y.resize(needed, 0);
            self.tag.resize(needed, 0);
            self.ids.resize(needed, -1);
        }
        self.x[..live].copy_from_slice(self.core.world.pos_x());
        self.y[..live].copy_from_slice(self.core.world.pos_y());
        for row in 0..live {
            let owner = self.owner_at(row);
            let id = self.core.world.handles()[row];
            self.ids[row] = id as i32;
            let type_id = self.type_at(row);
            let hits = self.core.world.hits()[row].max(0);
            let max_hits = self
                .gd
                .index_of_type(type_id)
                .and_then(|i| self.gd.units.get(i))
                .map_or(hits.max(1), |u| u.hits.max(1));
            let hp = ((hits as i64 * 255) / max_hits as i64).clamp(0, 255) as u32;
            self.tag[row] = 0x8000_0000
                | ((self.selected(owner, id) as u32) << 30)
                | (3 << 24)
                | (hp << 16)
                | (((type_id as u32) & 0xff) << 8)
                | (owner as u32 & 0xf);
        }
        let mut view = live;
        for (build_row, build) in self.core.builds.iter().enumerate() {
            if !build.is_valid() || view >= self.x.len() {
                continue;
            }
            let Some(id) = Self::build_view_id(build_row) else {
                continue;
            };
            let owner = build.who as usize;
            let type_id = self.build_type(build_row);
            let facts = self.play.bld(type_id);
            let (x, y) = build.position();
            let hits = (build.myhits - build.damage).max(0);
            let max_hits = facts.map_or(build.construct_hits.max(1), |b| b.hits.max(1));
            let hp = ((hits as i64 * 255) / max_hits as i64).clamp(0, 255) as u32;
            let size = facts.map_or(1, |b| b.x_size.max(b.y_size)).clamp(1, 15) as u32;
            self.x[view] = x;
            self.y[view] = y;
            self.ids[view] = id;
            self.tag[view] = 0x8000_0000
                | ((self.selected(owner, id as u32) as u32) << 30)
                | (1 << 29)
                | ((!build.is_active() as u32) << 28)
                | (size << 24)
                | (hp << 16)
                | (((type_id as u32) & 0xff) << 8)
                | (owner as u32 & 0xf);
            view += 1;
        }
        self.view_live = view;
        self.x[view..].fill(0);
        self.y[view..].fill(0);
        self.tag[view..].fill(0);
        self.ids[view..].fill(-1);
        self.refresh_players();
    }

    fn refresh_players(&mut self) {
        self.players.fill(0);
        let mut pop = [0i32; PLAYERS];
        for row in 0..self.core.world.live_count() as usize {
            let owner = self.owner_at(row);
            if owner < PLAYERS {
                pop[owner] += 1;
            }
        }
        for p in 0..PLAYERS {
            let b = &mut self.players[p * PLAYER_FIELDS..(p + 1) * PLAYER_FIELDS];
            let econ = &self.core.leaders[p].econ;
            b[..6].copy_from_slice(&econ.stockpile);
            b[6..12].copy_from_slice(&econ.displayed);
            b[12..18].copy_from_slice(&econ.commerce_cap);
            b[24..30].copy_from_slice(&econ.gross);
            b[30] = pop[p];
            b[31] = 0; // core population-limit host is not yet exported
            b[32] = econ.age;
            b[33] = 0; // core research progress is not yet exported
        }
    }

    fn set_error(&mut self, message: impl AsRef<str>) {
        self.error.clear();
        self.error.extend_from_slice(message.as_ref().as_bytes());
    }

    fn selected_handles(&self, who: usize) -> Vec<Handle> {
        self.selection
            .get(who)
            .into_iter()
            .flatten()
            .filter_map(|&id| {
                let row = self.row_for_id(id)?;
                (self.owner_at(row) == who)
                    .then(|| self.core.world.handle_at_row(row))
                    .flatten()
            })
            .collect()
    }

    fn selected_build_rows(&self, who: usize) -> Vec<usize> {
        self.selection
            .get(who)
            .into_iter()
            .flatten()
            .filter_map(|&id| {
                let row = self.build_row_for_id(id)?;
                self.core
                    .builds
                    .get(row)
                    .is_some_and(|build| build.who as usize == who)
                    .then_some(row)
            })
            .collect()
    }

    fn packed_cost(cost: &[i32; 6]) -> Result<([i16; 3], [i16; 3]), TrainingRefusal> {
        let mut resources = [-1i16; 3];
        let mut amounts = [0i16; 3];
        let mut slot = 0usize;
        for (resource, &amount) in cost.iter().enumerate() {
            if amount < 0 || amount > i16::MAX as i32 {
                return Err(TrainingRefusal::UnsupportedCost);
            }
            if amount == 0 {
                continue;
            }
            if slot == resources.len() {
                return Err(TrainingRefusal::UnsupportedCost);
            }
            resources[slot] = resource as i16;
            amounts[slot] = amount as i16;
            slot += 1;
        }
        Ok((resources, amounts))
    }

    /// Queue up to `num` units through concrete `BuildData` records.  The returned
    /// refusal, when present, applies to the first item not queued; prior items remain an
    /// ordinary sequential retail transaction and are reported in the first tuple field.
    fn enqueue_training(
        &mut self,
        who: usize,
        type_id: i32,
        num: i32,
    ) -> (usize, Option<TrainingRefusal>) {
        if who >= PLAYERS {
            return (0, Some(TrainingRefusal::NoProducer));
        }
        let Some(unit) = self.play.unit(type_id).copied() else {
            return (0, Some(TrainingRefusal::NoProducer));
        };
        if unit.where_ < 0
            || !self
                .play
                .products_of(unit.where_)
                .any(|product| product == type_id)
        {
            return (0, Some(TrainingRefusal::NoProducer));
        }
        if unit.age > self.core.leaders[who].econ.age {
            return (0, Some(TrainingRefusal::WrongAge));
        }
        let (resources, amounts) = match Self::packed_cost(&unit.cost) {
            Ok(cost) => cost,
            Err(refusal) => return (0, Some(refusal)),
        };
        if type_id < 0 || type_id > i16::MAX as i32 {
            return (0, Some(TrainingRefusal::UnsupportedCost));
        }

        let selected = self.selected_build_rows(who);
        let mut queued = 0usize;
        for _ in 0..num.clamp(1, QUEUE_MAX as i32) {
            let pending_units: usize = self
                .core
                .builds
                .iter()
                .map(|build| build.queue.queued as usize)
                .sum();
            if self.core.world.live_count() as usize + pending_units
                >= self.core.world.capacity() as usize
            {
                return (queued, Some(TrainingRefusal::CapacityFull));
            }
            let matching: Vec<_> = selected
                .iter()
                .copied()
                .filter(|&row| {
                    self.core.builds.get(row).is_some_and(|build| {
                        build.is_active() && self.build_type(row) == unit.where_
                    })
                })
                .collect();
            if matching.is_empty() {
                return (queued, Some(TrainingRefusal::NoProducer));
            }
            let Some(row) = matching.into_iter().find(|&row| {
                (self.core.builds[row].queue.queued as usize) < QUEUE_MAX.min(u8::MAX as usize)
            }) else {
                return (queued, Some(TrainingRefusal::QueueFull));
            };
            if unit
                .cost
                .iter()
                .enumerate()
                .any(|(resource, &amount)| self.core.leaders[who].econ.stockpile[resource] < amount)
            {
                return (queued, Some(TrainingRefusal::CannotAfford));
            }
            for (resource, &amount) in unit.cost.iter().enumerate() {
                self.core.leaders[who].econ.stockpile[resource] -= amount;
            }
            self.core.production_runtime.leaders[who].resources =
                self.core.leaders[who].econ.stockpile;

            let build = &mut self.core.builds[row];
            let slot = build.queue.queued as usize;
            let entry = BuildQueueEntry {
                elapsed: 0,
                type_index: type_id as i16,
                res: resources,
                amt: amounts,
                tail: 0,
            };
            if slot < build.queue.entries.len() {
                build.queue.entries[slot] = entry;
            } else {
                build.queue.entries.push(entry);
            }
            build.queue.queued += 1;
            if let Some(count) = self.core.production_runtime.leaders[who]
                .queued_counts
                .get_mut(type_id as usize)
            {
                *count = count.wrapping_add(1);
            }
            self.core.production_runtime.leaders[who].queue_dirty = true;
            queued += 1;
        }
        (queued, None)
    }

    fn cancel_training(&mut self, who: usize, type_id: i32) -> bool {
        if who >= PLAYERS {
            return false;
        }
        for row in self.selected_build_rows(who) {
            let logical = self.core.builds[row].queue.queued as usize;
            let Some(slot) = (0..logical)
                .rev()
                .find(|&slot| type_id < 0 || self.core.builds[row].queue.type_at(slot) == type_id)
            else {
                continue;
            };
            let entry = self.core.builds[row].queue.entries[slot];
            for (&resource, &amount) in entry.res.iter().zip(&entry.amt) {
                if let Ok(resource) = usize::try_from(resource) {
                    if let Some(stock) = self.core.leaders[who].econ.stockpile.get_mut(resource) {
                        *stock = stock.wrapping_add(amount as i32);
                    }
                }
            }
            let build = &mut self.core.builds[row];
            for at in slot + 1..logical {
                build.queue.entries[at - 1] = build.queue.entries[at];
            }
            build.queue.entries[logical - 1] = BuildQueueEntry {
                type_index: -1,
                res: [-1; 3],
                ..BuildQueueEntry::default()
            };
            build.queue.queued -= 1;
            if let Ok(type_index) = usize::try_from(entry.type_index) {
                if let Some(count) = self.core.production_runtime.leaders[who]
                    .queued_counts
                    .get_mut(type_index)
                {
                    if *count > 0 {
                        *count -= 1;
                    }
                }
            }
            self.core.production_runtime.leaders[who].resources =
                self.core.leaders[who].econ.stockpile;
            self.core.production_runtime.leaders[who].queue_dirty = true;
            return true;
        }
        false
    }

    fn apply_commands(&mut self) {
        let queued = std::mem::take(&mut self.pending);
        let mut offset = 0usize;
        while offset + 8 <= queued.len() {
            let read = |at: usize| {
                u32::from_le_bytes([queued[at], queued[at + 1], queued[at + 2], queued[at + 3]])
            };
            let who = read(offset) as usize;
            let len = read(offset + 4) as usize;
            offset += 8;
            if offset.checked_add(len).is_none_or(|end| end > queued.len()) {
                self.gaps[gap::UNKNOWN_OPCODE] += 1;
                break;
            }
            let bytes = &queued[offset..offset + len];
            offset += len;
            self.commands_seen += 1;
            let Some(&op) = bytes.first() else {
                self.gaps[gap::UNKNOWN_OPCODE] += 1;
                continue;
            };
            match op {
                wire_gen::op::GROUP => {
                    if who >= PLAYERS {
                        self.gaps[gap::NO_SELECTION] += 1;
                        continue;
                    }
                    let n = wire_gen::group::num(bytes);
                    let owner = wire_gen::group::who(bytes);
                    let owner = if owner >= 0 { owner as usize } else { who };
                    let mut ids = Vec::new();
                    for k in 0..n {
                        let Some(id) = wire_gen::group::entry(bytes, k) else {
                            continue;
                        };
                        if id < 0 {
                            continue;
                        }
                        let id = id as u32;
                        if self.object_owned(id, owner) {
                            ids.push(id);
                        }
                    }
                    if let Some(selection) = self.selection.get_mut(owner) {
                        *selection = ids;
                        self.orders_applied += selection.len() as u64;
                    }
                }
                wire_gen::op::MOVE_TO => {
                    let handles = self.selected_handles(who);
                    if handles.is_empty() {
                        self.gaps[gap::NO_SELECTION] += 1;
                    }
                    for handle in handles {
                        if self.core.issue(
                            handle,
                            Order::move_to(
                                wire_gen::move_to::to_x(bytes),
                                wire_gen::move_to::to_y(bytes),
                                CORE_SUBTILE,
                            ),
                        ) {
                            self.orders_applied += 1;
                        }
                    }
                }
                wire_gen::op::ATTACK => {
                    let target = wire_gen::attack::whom(bytes);
                    let target = (target >= 0).then_some(target as u32);
                    let target_fact = target.and_then(|id| {
                        let row = self.row_for_id(id)?;
                        Some((self.owner_at(row) as i8, self.core.world.units.o()[row]))
                    });
                    let handles = self.selected_handles(who);
                    if handles.is_empty() || target_fact.is_none() {
                        self.gaps[gap::NO_SELECTION] += 1;
                    }
                    if let Some((owner, object)) = target_fact {
                        for handle in handles {
                            if self.core.issue(handle, Order::attack(owner, object)) {
                                self.orders_applied += 1;
                            }
                        }
                    }
                }
                wire_gen::op::HALT => {
                    let handles = self.selected_handles(who);
                    if handles.is_empty() {
                        self.gaps[gap::NO_SELECTION] += 1;
                    }
                    for handle in handles {
                        if let Some(row) = self.core.world.row_of(handle) {
                            self.core.world.orders_mut(row).clear();
                            self.orders_applied += 1;
                        }
                    }
                }
                wire_gen::op::GATHER => self.gaps[gap::NOT_GATHERABLE] += 1,
                wire_gen::op::BUILD => self.gaps[gap::PLACEMENT_BLOCKED] += 1,
                wire_gen::op::QUEUE_UP => {
                    let (applied, refusal) = self.enqueue_training(
                        who,
                        wire_gen::queue_up::type_(bytes),
                        wire_gen::queue_up::num(bytes),
                    );
                    self.orders_applied += applied as u64;
                    if let Some(refusal) = refusal {
                        self.gaps[refusal.gap()] += 1;
                    }
                }
                wire_gen::op::UNQUEUE => {
                    if self.cancel_training(who, wire_gen::unqueue::type_(bytes)) {
                        self.orders_applied += 1;
                    } else {
                        self.gaps[gap::NOT_A_PRODUCER] += 1;
                    }
                }
                _ => self.gaps[gap::UNKNOWN_OPCODE] += 1,
            }
        }
        self.pending = queued;
        self.pending.clear();
    }
}

macro_rules! game_ref {
    ($g:expr) => {
        match unsafe { $g.as_mut() } {
            Some(game) => game,
            None => return Default::default(),
        }
    };
}
macro_rules! game_ptr {
    ($g:expr) => {
        match unsafe { $g.as_mut() } {
            Some(game) => game,
            None => return std::ptr::null_mut(),
        }
    };
}

#[no_mangle]
pub extern "C" fn game_gamedata_alloc(len: u32) -> *mut u8 {
    let bytes = stage(&GAMEDATA);
    bytes.clear();
    bytes.resize(len as usize, 0);
    bytes.as_mut_ptr()
}

#[no_mangle]
pub extern "C" fn game_playdata_alloc(len: u32) -> *mut u8 {
    let bytes = stage(&PLAYDATA);
    bytes.clear();
    bytes.resize(len as usize, 0);
    bytes.as_mut_ptr()
}

#[no_mangle]
pub extern "C" fn game_create(seed_lo: u32, seed_hi: u32) -> *mut Game {
    Box::into_raw(Box::new(Game::new(
        ((seed_hi as u64) << 32) | seed_lo as u64,
    )))
}

/// # Safety
/// `g` must be a live handle returned by [`game_create`].
#[no_mangle]
pub unsafe extern "C" fn game_destroy(g: *mut Game) {
    if !g.is_null() {
        drop(unsafe { Box::from_raw(g) });
    }
}

/// Drain exact command packets, advance the authoritative core, and refresh projections.
/// # Safety
/// `g` must be live.
#[no_mangle]
pub unsafe extern "C" fn game_step(g: *mut Game, frames: u32) {
    let game = game_ref!(g);
    for _ in 0..frames {
        game.apply_commands();
        game.core.do_frame();
    }
    game.refresh();
}

#[no_mangle]
pub unsafe extern "C" fn game_x_ptr(g: *mut Game) -> *mut i32 {
    game_ptr!(g).x.as_mut_ptr()
}
#[no_mangle]
pub unsafe extern "C" fn game_y_ptr(g: *mut Game) -> *mut i32 {
    game_ptr!(g).y.as_mut_ptr()
}
#[no_mangle]
pub unsafe extern "C" fn game_tag_ptr(g: *mut Game) -> *mut u32 {
    game_ptr!(g).tag.as_mut_ptr()
}
#[no_mangle]
pub unsafe extern "C" fn game_live(g: *mut Game) -> u32 {
    game_ref!(g).view_live as u32
}
#[no_mangle]
pub unsafe extern "C" fn game_capacity(g: *mut Game) -> u32 {
    game_ref!(g).x.len() as u32
}
#[no_mangle]
pub unsafe extern "C" fn game_id_at_row(g: *mut Game, row: u32) -> i32 {
    game_ref!(g).ids.get(row as usize).copied().unwrap_or(-1)
}
#[no_mangle]
pub unsafe extern "C" fn game_tile_ptr(g: *mut Game) -> *mut u16 {
    game_ptr!(g).core.map.world.tdata.as_mut_ptr()
}
#[no_mangle]
pub unsafe extern "C" fn game_terrain_version(g: *mut Game) -> u32 {
    game_ref!(g).terrain_version
}

#[no_mangle]
pub unsafe extern "C" fn game_cmd_ptr(g: *mut Game) -> *mut u8 {
    game_ptr!(g).cmd.as_mut_ptr()
}
#[no_mangle]
pub extern "C" fn game_cmd_capacity() -> u32 {
    CMD_SCRATCH as u32
}
#[no_mangle]
pub unsafe extern "C" fn game_submit(g: *mut Game, who: u32, len: u32) -> u32 {
    let game = game_ref!(g);
    let n = (len as usize).min(game.cmd.len());
    game.pending.extend_from_slice(&who.to_le_bytes());
    game.pending.extend_from_slice(&(n as u32).to_le_bytes());
    let bytes = game.cmd[..n].to_vec();
    game.pending.extend_from_slice(&bytes);
    game.pending.len() as u32
}

#[no_mangle]
pub unsafe extern "C" fn game_pick_ptr(g: *mut Game) -> *mut i16 {
    game_ptr!(g).pick.as_mut_ptr()
}
#[no_mangle]
pub unsafe extern "C" fn game_pick_box(
    g: *mut Game,
    who: u32,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
) -> u32 {
    let game = game_ref!(g);
    let (lo_x, hi_x) = (x0.min(x1), x0.max(x1));
    let (lo_y, hi_y) = (y0.min(y1), y0.max(y1));
    let mut n = 0usize;
    for row in 0..game.view_live {
        if game.tag[row] & 0xf == who
            && game.x[row] >= lo_x
            && game.x[row] <= hi_x
            && game.y[row] >= lo_y
            && game.y[row] <= hi_y
            && n < game.pick.len()
        {
            game.pick[n] = game.ids[row] as i16;
            n += 1;
        }
    }
    n as u32
}
#[no_mangle]
pub unsafe extern "C" fn game_pick_type(g: *mut Game, who: u32, type_id: i32) -> u32 {
    let game = game_ref!(g);
    let mut n = 0usize;
    for row in 0..game.view_live {
        let id = game.ids[row];
        let projected_type = (id >= 0)
            .then(|| game.row_for_id(id as u32))
            .flatten()
            .map(|unit_row| game.type_at(unit_row))
            .or_else(|| {
                (id >= 0)
                    .then(|| game.build_row_for_id(id as u32))
                    .flatten()
                    .map(|build_row| game.build_type(build_row))
            });
        if game.tag[row] & 0xf == who && projected_type == Some(type_id) && n < game.pick.len() {
            game.pick[n] = id as i16;
            n += 1;
        }
    }
    n as u32
}
#[no_mangle]
pub unsafe extern "C" fn game_pick_at(g: *mut Game, x: i32, y: i32) -> i32 {
    let game = game_ref!(g);
    let mut best = None;
    for row in 0..game.view_live {
        let dx = i64::from(game.x[row] - x);
        let dy = i64::from(game.y[row] - y);
        let distance = dx * dx + dy * dy;
        let building = game.tag[row] & (1 << 29) != 0;
        let extent = if building {
            i32::try_from((game.tag[row] >> 24) & 0xf).unwrap_or(1) * TILE_COORD / 2
        } else {
            CORE_SUBTILE * 2
        };
        if dx.abs() <= i64::from(extent)
            && dy.abs() <= i64::from(extent)
            && best.is_none_or(|(prior, _)| distance < prior)
        {
            best = Some((distance, game.ids[row]));
        }
    }
    best.map_or(-1, |(_, id)| id)
}

#[no_mangle]
pub unsafe extern "C" fn game_info_ptr(g: *mut Game) -> *mut i32 {
    game_ptr!(g).info.as_mut_ptr()
}
#[no_mangle]
pub unsafe extern "C" fn game_object_info(g: *mut Game, id: i32) -> u32 {
    let game = game_ref!(g);
    if let Some(row) = (id >= 0)
        .then(|| game.build_row_for_id(id as u32))
        .flatten()
    {
        game.info.fill(0);
        let build = &game.core.builds[row];
        let type_id = game.build_type(row);
        let facts = game.play.bld(type_id);
        let (x, y) = build.position();
        game.info[0] = id;
        game.info[1] = build.who as i32;
        game.info[2] = type_id;
        game.info[3] = 1;
        game.info[4] = (build.myhits - build.damage).max(0);
        game.info[5] = facts.map_or(build.construct_hits.max(1), |b| b.hits.max(1));
        game.info[6] = facts.map_or(0, |b| b.attack);
        game.info[7] = facts.map_or(0, |b| b.armor);
        game.info[8] = facts.map_or(0, |b| b.max_range);
        game.info[9] = 0;
        game.info[10] = facts.map_or(0, |b| b.recharge);
        game.info[11] = OrderIndex::None as i32;
        game.info[12] = -1;
        game.info[13] = if build.is_active() {
            -1
        } else {
            build.job_counter as i32
        };
        game.info[14] = facts.map_or(build.constr_time as i32, |b| b.job_time.max(1));
        let logical = (build.queue.queued as usize).min(QUEUE_MAX);
        game.info[15] = logical as i32;
        for slot in 0..logical {
            game.info[16 + slot] = build.queue.type_at(slot);
        }
        game.info[24] = 0;
        game.info[25] = -1;
        game.info[26] = x;
        game.info[27] = facts.map_or(1, |b| b.x_size.max(1));
        game.info[28] = facts.map_or(1, |b| b.y_size.max(1));
        game.info[29] = y;
        return 1;
    }
    let Some(row) = (id >= 0).then(|| game.row_for_id(id as u32)).flatten() else {
        return 0;
    };
    game.info.fill(0);
    let type_id = game.type_at(row);
    let unit = game
        .gd
        .index_of_type(type_id)
        .and_then(|i| game.gd.units.get(i));
    let order = game.core.world.orders(row).current();
    game.info[0] = id;
    game.info[1] = game.owner_at(row) as i32;
    game.info[2] = type_id;
    game.info[3] = 0;
    game.info[4] = game.core.world.hits()[row];
    game.info[5] = unit.map_or(game.info[4].max(1), |u| u.hits.max(1));
    game.info[6] = unit.map_or(0, |u| u.attack);
    game.info[7] = unit.map_or(0, |u| u.armor);
    game.info[8] = unit.map_or(0, |u| u.max_range);
    game.info[9] = unit.map_or(0, |u| u.moves);
    game.info[10] = unit.map_or(0, |u| u.recharge);
    game.info[11] = order.map_or(OrderIndex::None as i32, |value| value.kind as i32);
    game.info[12] = order.map_or(-1, |value| value.target_o as i32);
    game.info[13] = -1;
    game.info[14] = 0;
    // Slots 15..23 are the production queue ABI. Core units expose their current order
    // through slot 11; they are not producers, so do not relabel an OrderList as training.
    game.info[15] = 0;
    game.info[24] = 1;
    game.info[25] = -1;
    game.info[26] = game.x[row];
    game.info[27] = 1;
    game.info[28] = 1;
    game.info[29] = game.y[row];
    1
}

#[no_mangle]
pub unsafe extern "C" fn game_players_ptr(g: *mut Game) -> *mut i32 {
    game_ptr!(g).players.as_mut_ptr()
}
#[no_mangle]
pub extern "C" fn game_player_fields() -> u32 {
    PLAYER_FIELDS as u32
}
#[no_mangle]
pub unsafe extern "C" fn game_gaps_ptr(g: *mut Game) -> *mut u32 {
    game_ptr!(g).gaps.as_mut_ptr()
}
#[no_mangle]
pub extern "C" fn game_gap_count() -> u32 {
    gap::COUNT as u32
}
#[no_mangle]
pub unsafe extern "C" fn game_commands_seen(g: *mut Game) -> u32 {
    game_ref!(g).commands_seen as u32
}
#[no_mangle]
pub unsafe extern "C" fn game_orders_applied(g: *mut Game) -> u32 {
    game_ref!(g).orders_applied as u32
}

fn playable_blocker(index: u32) -> Option<Deviation> {
    ModeConfig::improved()
        .readiness_blockers(Surface::PlayableEdition)
        .nth(index as usize)
        .map(|blocker| blocker.deviation())
}
#[no_mangle]
pub extern "C" fn game_playable_blocker_count() -> u32 {
    ModeConfig::improved()
        .readiness_blockers(Surface::PlayableEdition)
        .count() as u32
}
#[no_mangle]
pub extern "C" fn game_playable_blocker_slug_ptr(index: u32) -> *const u8 {
    playable_blocker(index)
        .map(|deviation| deviation.entry().slug.as_ptr())
        .unwrap_or(std::ptr::null())
}
#[no_mangle]
pub extern "C" fn game_playable_blocker_slug_len(index: u32) -> u32 {
    playable_blocker(index).map_or(0, |deviation| deviation.entry().slug.len() as u32)
}
#[no_mangle]
pub extern "C" fn game_playable_blocker_title_ptr(index: u32) -> *const u8 {
    playable_blocker(index)
        .map(|deviation| deviation.entry().title.as_ptr())
        .unwrap_or(std::ptr::null())
}
#[no_mangle]
pub extern "C" fn game_playable_blocker_title_len(index: u32) -> u32 {
    playable_blocker(index).map_or(0, |deviation| deviation.entry().title.len() as u32)
}

#[no_mangle]
pub unsafe extern "C" fn game_placement_grade(
    g: *mut Game,
    who: u32,
    _type_id: i32,
    tx: i32,
    ty: i32,
) -> i32 {
    game_ref!(g)
        .core
        .map
        .world
        .space_at_corner(tx, ty, who as i32, false)
}
#[no_mangle]
pub unsafe extern "C" fn game_check_wcell(g: *mut Game, who: u32, tx: i32, ty: i32) -> i32 {
    game_ref!(g)
        .core
        .map
        .world
        .check_building_wcoord(tx >> 2, ty >> 2, who as i32, 3, 3, 4, false)
}
#[no_mangle]
pub unsafe extern "C" fn game_tile_resource(_g: *mut Game, _tx: i32, _ty: i32) -> i32 {
    -1
}
#[no_mangle]
pub unsafe extern "C" fn game_products(g: *mut Game, producer_type: i32) -> u32 {
    let game = game_ref!(g);
    game.products.fill(-1);
    let products: Vec<_> = game
        .play
        .products_of(producer_type)
        .take(game.products.len())
        .collect();
    game.products[..products.len()].copy_from_slice(&products);
    products.len() as u32
}
#[no_mangle]
pub unsafe extern "C" fn game_products_ptr(g: *mut Game) -> *mut i32 {
    game_ptr!(g).products.as_mut_ptr()
}
#[no_mangle]
pub unsafe extern "C" fn game_has_playdata(g: *mut Game) -> u32 {
    game_ref!(g).play.is_real as u32
}
#[no_mangle]
pub unsafe extern "C" fn game_staged_playdata(g: *mut Game) -> u32 {
    game_ref!(g).play.is_real as u32
}
#[no_mangle]
pub unsafe extern "C" fn game_has_gamedata(g: *mut Game) -> u32 {
    game_ref!(g).gd.is_real as u32
}
#[no_mangle]
pub unsafe extern "C" fn game_frame(g: *mut Game) -> u32 {
    game_ref!(g).core.world.frame as u32
}
#[no_mangle]
pub unsafe extern "C" fn game_digest_lo(g: *mut Game) -> u32 {
    game_ref!(g).core.channel_digest() as u32
}
#[no_mangle]
pub unsafe extern "C" fn game_digest_hi(g: *mut Game) -> u32 {
    (game_ref!(g).core.channel_digest() >> 32) as u32
}
#[no_mangle]
pub unsafe extern "C" fn game_rng_state(g: *mut Game) -> u32 {
    game_ref!(g).core.world.random.state() as u32
}
#[no_mangle]
pub unsafe extern "C" fn game_seed(g: *mut Game) -> u32 {
    game_ref!(g).core.map.world.seed as u32
}
#[no_mangle]
pub unsafe extern "C" fn game_start_x(g: *mut Game, p: u32) -> i32 {
    game_ref!(g).start_x[(p as usize) % PLAYERS]
}
#[no_mangle]
pub unsafe extern "C" fn game_start_y(g: *mut Game, p: u32) -> i32 {
    game_ref!(g).start_y[(p as usize) % PLAYERS]
}
#[no_mangle]
pub unsafe extern "C" fn game_set_pop_setting(_g: *mut Game, _setting: u32) {}
#[no_mangle]
pub unsafe extern "C" fn game_set_income_mode(_g: *mut Game, _mode: u32) {}
#[no_mangle]
pub unsafe extern "C" fn game_debug_spawn(g: *mut Game, owner: u32, type_id: i32, n: u32) -> u32 {
    let game = game_ref!(g);
    let mut made = 0;
    for _ in 0..n {
        if game
            .core
            .spawn_unit(
                owner as usize,
                type_id,
                game.start_x[(owner as usize) % PLAYERS],
                game.start_y[(owner as usize) % PLAYERS],
                4,
            )
            .is_some()
        {
            made += 1;
        }
    }
    game.refresh();
    made
}

/// Serialize the authoritative core. Failure leaves both the live core and the prior
/// successful byte image untouched.
#[no_mangle]
pub unsafe extern "C" fn game_save(g: *mut Game) -> u32 {
    let game = game_ref!(g);
    if !game.pending.is_empty() {
        game.set_error("save refused: browser commands are pending the next core tick");
        return 0;
    }
    match save_sim(&game.core) {
        Ok(bytes) => {
            game.save_bytes = bytes;
            game.error.clear();
            1
        }
        Err(error) => {
            game.set_error(format!("save refused: {error}"));
            0
        }
    }
}
#[no_mangle]
pub unsafe extern "C" fn game_save_ptr(g: *mut Game) -> *const u8 {
    game_ref!(g).save_bytes.as_ptr()
}
#[no_mangle]
pub unsafe extern "C" fn game_save_len(g: *mut Game) -> u32 {
    game_ref!(g).save_bytes.len() as u32
}
#[no_mangle]
pub extern "C" fn game_save_limit() -> u32 {
    SAVE_LIMIT as u32
}
/// Reserve a bounded load staging area. A null return refuses the allocation before the
/// JavaScript copy; the live core is untouched.
#[no_mangle]
pub unsafe extern "C" fn game_load_alloc(g: *mut Game, len: u32) -> *mut u8 {
    let game = game_ptr!(g);
    let len = len as usize;
    if len == 0 || len > SAVE_LIMIT {
        game.set_error(format!(
            "load refused: byte length must be 1..={SAVE_LIMIT}"
        ));
        return std::ptr::null_mut();
    }
    game.load_bytes.clear();
    game.load_bytes.resize(len, 0);
    game.load_bytes.as_mut_ptr()
}
/// Decode into a replacement `Sim` and swap only on complete success.
#[no_mangle]
pub unsafe extern "C" fn game_load_commit(g: *mut Game) -> u32 {
    let game = game_ref!(g);
    match load_sim(&game.load_bytes) {
        Ok(core) if core.map.world.tile_xs == MAP_TILES && core.map.world.tile_ys == MAP_TILES => {
            game.core = core;
            game.install_production_facts();
            game.pending.clear();
            for selection in &mut game.selection {
                selection.clear();
            }
            game.commands_seen = 0;
            game.orders_applied = 0;
            game.terrain_version = game.terrain_version.wrapping_add(1).max(1);
            game.error.clear();
            game.refresh();
            1
        }
        Ok(core) => {
            game.set_error(format!(
                "load refused: map is {}x{} tiles; browser projection requires {MAP_TILES}x{MAP_TILES}",
                core.map.world.tile_xs, core.map.world.tile_ys
            ));
            0
        }
        Err(error) => {
            game.set_error(format!("load refused: {error}"));
            0
        }
    }
}
#[no_mangle]
pub unsafe extern "C" fn game_error_ptr(g: *mut Game) -> *const u8 {
    game_ref!(g).error.as_ptr()
}
#[no_mangle]
pub unsafe extern "C" fn game_error_len(g: *mut Game) -> u32 {
    game_ref!(g).error.len() as u32
}
#[no_mangle]
pub extern "C" fn game_capabilities() -> u32 {
    capability::CORE_SAVE
        | capability::CORE_LOAD
        | capability::MOVE
        | capability::ATTACK
        | capability::HALT
        | capability::TRAIN
}

#[no_mangle]
pub extern "C" fn game_map_tiles() -> i32 {
    MAP_TILES
}
#[no_mangle]
pub extern "C" fn game_map_span() -> i32 {
    MAP_SPAN
}
#[no_mangle]
pub extern "C" fn game_subtile() -> i32 {
    TILE_COORD
}
#[no_mangle]
pub extern "C" fn game_players_count() -> u32 {
    PLAYERS as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static STAGE_LOCK: Mutex<()> = Mutex::new(());

    fn stage_test_playdata() {
        let mut values = vec![0i32; STARTING_GOODS_RULE / 4 + 6];
        values[STARTING_GOODS_RULE / 4..STARTING_GOODS_RULE / 4 + 6].fill(200);
        let mut words = vec![
            1, // units
            crate::game::PLAY_UNIT_FIELDS as i32,
            1, // buildings
            crate::game::PLAY_BLD_FIELDS as i32,
            1, // product edges
            0, // age records
            values.len() as i32,
            0, // reserved header word
        ];
        words.extend_from_slice(&[
            50,
            2,
            0,
            0,
            0,
            0,
            0, // type + cost
            1,
            2,
            FIRST_BUILDING,
            0,
            0,
            4, // pop/time/where/age/masks/los
        ]);
        words.extend_from_slice(&[
            FIRST_BUILDING,
            0,
            0,
            0,
            0,
            0,
            0, // type + cost
            20,
            5,
            5,
            1200,
            2,
            0,
            0,
            1,
            0,
            0,
            6,
        ]);
        words.extend_from_slice(&[FIRST_BUILDING, 50]);
        words.extend(values);
        let play = stage(&PLAYDATA);
        play.clear();
        play.extend_from_slice(b"DONPLAY1");
        for word in words {
            play.extend_from_slice(&word.to_le_bytes());
        }
        stage(&GAMEDATA).clear();
    }

    fn submit_packet(game: &mut Game, who: u32, packet: &[u8]) {
        game.pending.extend_from_slice(&who.to_le_bytes());
        game.pending
            .extend_from_slice(&(packet.len() as u32).to_le_bytes());
        game.pending.extend_from_slice(packet);
    }

    fn select_packet(owner: u8, id: i16) -> [u8; 5] {
        let [lo, hi] = id.to_le_bytes();
        [wire_gen::op::GROUP, 1, owner, lo, hi]
    }

    fn queue_packet(type_id: i32, count: i32) -> [u8; 9] {
        let mut packet = [0u8; 9];
        packet[0] = wire_gen::op::QUEUE_UP;
        packet[1..5].copy_from_slice(&type_id.to_le_bytes());
        packet[5..9].copy_from_slice(&count.to_le_bytes());
        packet
    }

    fn unqueue_packet(type_id: i32) -> [u8; 15] {
        let mut packet = [0u8; 15];
        packet[0] = wire_gen::op::UNQUEUE;
        packet[1..5].copy_from_slice(&0i32.to_le_bytes());
        packet[5..9].copy_from_slice(&BUILD_BAND_BASE.to_le_bytes());
        packet[9..13].copy_from_slice(&type_id.to_le_bytes());
        packet
    }

    #[test]
    fn core_save_load_is_atomic_and_preserves_identity() {
        let _stage = STAGE_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        stage(&PLAYDATA).clear();
        stage(&GAMEDATA).clear();
        let mut game = Game::new(0x1234_5678);
        let before = game.core.channel_digest();
        let rng = game.core.world.random.state();
        let handles: Vec<_> = (0..game.core.world.live_count() as usize)
            .map(|row| game.core.world.handle_at_row(row).unwrap())
            .collect();
        let bytes = save_sim(&game.core).unwrap();
        let replacement = load_sim(&bytes).unwrap();
        game.core = replacement;
        assert_eq!(game.core.channel_digest(), before);
        assert_eq!(game.core.world.random.state(), rng);
        assert_eq!(
            (0..game.core.world.live_count() as usize)
                .map(|row| game.core.world.handle_at_row(row).unwrap())
                .collect::<Vec<_>>(),
            handles
        );

        let stable = game.core.channel_digest();
        game.load_bytes = b"not a save".to_vec();
        assert_eq!(unsafe { game_load_commit(&mut game) }, 0);
        assert!(String::from_utf8_lossy(&game.error).contains("load refused"));
        assert_eq!(game.core.channel_digest(), stable);
    }

    #[test]
    fn city_training_packets_charge_cancel_roundtrip_and_complete_in_core() {
        let _stage = STAGE_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        stage_test_playdata();
        let mut game = Game::new(0x1234_5678);
        assert!(game.play.is_real);
        assert_eq!(game.core.builds.len(), PLAYERS);
        let producer_id = Game::build_view_id(0).unwrap() as i16;
        submit_packet(&mut game, 0, &select_packet(0, producer_id));
        submit_packet(&mut game, 0, &queue_packet(50, 1));
        game.apply_commands();
        assert_eq!(game.core.builds[0].queue.queued, 1);
        assert_eq!(game.core.leaders[0].econ.stockpile[0], 198);
        assert_eq!(game.orders_applied, 2);

        submit_packet(&mut game, 0, &unqueue_packet(50));
        game.apply_commands();
        assert_eq!(game.core.builds[0].queue.queued, 0);
        assert_eq!(game.core.leaders[0].econ.stockpile[0], 200);

        submit_packet(&mut game, 0, &queue_packet(50, 1));
        game.apply_commands();
        let saved = save_sim(&game.core).expect("queued authoritative build must save");
        game.core = load_sim(&saved).expect("queued authoritative build must load");
        game.install_production_facts();
        assert_eq!(game.core.builds[0].queue.queued, 1);
        assert_eq!(game.core.leaders[0].econ.stockpile[0], 198);
        assert_eq!(game.core.production_runtime.leaders[0].queued_counts[50], 1);

        let live_before = game.core.world.live_count();
        let type_count_before = game
            .core
            .unit_type
            .iter()
            .take(live_before as usize)
            .filter(|&&type_id| type_id == 50)
            .count();
        for _ in 0..512 {
            game.core.do_frame();
            if game.core.builds[0].queue.queued == 0 {
                break;
            }
        }
        assert_eq!(game.core.builds[0].queue.queued, 0);
        assert_eq!(game.core.world.live_count(), live_before + 1);
        assert_eq!(
            game.core
                .unit_type
                .iter()
                .take(game.core.world.live_count() as usize)
                .filter(|&&type_id| type_id == 50)
                .count(),
            type_count_before + 1
        );
        assert_eq!(game.core.leaders[0].econ.stockpile[0], 198);
    }

    #[test]
    fn browser_readiness_is_the_compiled_playable_registry() {
        let expected: Vec<_> = ModeConfig::improved()
            .readiness_blockers(Surface::PlayableEdition)
            .map(|blocker| blocker.deviation())
            .collect();
        assert_eq!(game_playable_blocker_count() as usize, expected.len());
        assert!(game_playable_blocker_slug_ptr(u32::MAX).is_null());
        assert_eq!(game_playable_blocker_title_len(u32::MAX), 0);
    }
}
