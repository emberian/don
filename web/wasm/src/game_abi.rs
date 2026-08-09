//! C ABI for the playable browser client.
//!
//! The authoritative state behind this surface is [`don_sim::tick::Sim`].  The arrays
//! exposed to JavaScript are render/query projections rebuilt from that core; they are not
//! a second game world.  Save/load delegates directly to `don_sim`'s deterministic format
//! and atomically swaps the core only after a complete, bounded decode.

use crate::game::{gap, PlayData, PLAYERS, W_CELLS};
use crate::gamedata::GameData;
use crate::wire_gen;
use don_sim::deviations::{Deviation, ModeConfig, Surface};
use don_sim::order::{Order, OrderIndex};
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
/// `i32`s per player in the block [`game_players_ptr`] exposes.
pub const PLAYER_FIELDS: usize = 34;

pub mod capability {
    pub const CORE_SAVE: u32 = 1 << 0;
    pub const CORE_LOAD: u32 = 1 << 1;
    pub const MOVE: u32 = 1 << 2;
    pub const ATTACK: u32 = 1 << 3;
    pub const HALT: u32 = 1 << 4;
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
    had_playdata: bool,
    x: Vec<i32>,
    y: Vec<i32>,
    tag: Vec<u32>,
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
        let capacity = core.world.capacity() as usize;
        let mut game = Game {
            core,
            gd,
            had_playdata: play.is_real,
            x: vec![0; capacity],
            y: vec![0; capacity],
            tag: vec![0; capacity],
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

    fn refresh(&mut self) {
        let live = self.core.world.live_count() as usize;
        self.x[..live].copy_from_slice(self.core.world.pos_x());
        self.y[..live].copy_from_slice(self.core.world.pos_y());
        for row in 0..live {
            let owner = self.owner_at(row);
            let id = self.core.world.handles()[row];
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
        self.x[live..].fill(0);
        self.y[live..].fill(0);
        self.tag[live..].fill(0);
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
                        if self
                            .row_for_id(id)
                            .is_some_and(|row| self.owner_at(row) == owner)
                        {
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
                wire_gen::op::QUEUE_UP | wire_gen::op::UNQUEUE => {
                    self.gaps[gap::NOT_A_PRODUCER] += 1
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
    game_ref!(g).core.world.live_count()
}
#[no_mangle]
pub unsafe extern "C" fn game_capacity(g: *mut Game) -> u32 {
    game_ref!(g).core.world.capacity()
}
#[no_mangle]
pub unsafe extern "C" fn game_id_at_row(g: *mut Game, row: u32) -> i32 {
    game_ref!(g)
        .core
        .world
        .handle_at_row(row as usize)
        .map_or(-1, |handle| handle.id as i32)
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
    for row in 0..game.core.world.live_count() as usize {
        if game.owner_at(row) == who as usize
            && game.x[row] >= lo_x
            && game.x[row] <= hi_x
            && game.y[row] >= lo_y
            && game.y[row] <= hi_y
            && n < game.pick.len()
        {
            game.pick[n] = game.core.world.handles()[row] as i16;
            n += 1;
        }
    }
    n as u32
}
#[no_mangle]
pub unsafe extern "C" fn game_pick_type(g: *mut Game, who: u32, type_id: i32) -> u32 {
    let game = game_ref!(g);
    let mut n = 0usize;
    for row in 0..game.core.world.live_count() as usize {
        if game.owner_at(row) == who as usize && game.type_at(row) == type_id && n < game.pick.len()
        {
            game.pick[n] = game.core.world.handles()[row] as i16;
            n += 1;
        }
    }
    n as u32
}
#[no_mangle]
pub unsafe extern "C" fn game_pick_at(g: *mut Game, x: i32, y: i32) -> i32 {
    let game = game_ref!(g);
    let mut best = None;
    for row in 0..game.core.world.live_count() as usize {
        let dx = i64::from(game.x[row] - x);
        let dy = i64::from(game.y[row] - y);
        let distance = dx * dx + dy * dy;
        if distance <= i64::from(CORE_SUBTILE * CORE_SUBTILE * 4)
            && best.is_none_or(|(prior, _)| distance < prior)
        {
            best = Some((distance, game.core.world.handles()[row] as i32));
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
pub unsafe extern "C" fn game_products(_g: *mut Game, _producer_type: i32) -> u32 {
    0
}
#[no_mangle]
pub unsafe extern "C" fn game_products_ptr(g: *mut Game) -> *mut i32 {
    game_ptr!(g).products.as_mut_ptr()
}
#[no_mangle]
pub unsafe extern "C" fn game_has_playdata(g: *mut Game) -> u32 {
    game_ref!(g).had_playdata as u32
}
#[no_mangle]
pub unsafe extern "C" fn game_staged_playdata(g: *mut Game) -> u32 {
    game_ref!(g).had_playdata as u32
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

    #[test]
    fn core_save_load_is_atomic_and_preserves_identity() {
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
