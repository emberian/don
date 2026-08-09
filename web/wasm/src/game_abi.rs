//! C ABI for the **playable** client, alongside `lib.rs`'s spectator ABI.
//!
//! Same discipline as the spectator surface: the only things that cross the JS boundary are
//! `u32` scalars, byte offsets into wasm linear memory, and raw engine wire bytes. The
//! renderer binds `game_x_ptr` / `game_y_ptr` / `game_tag_ptr` directly as vertex buffers,
//! so there is no per-entity CPU pass and no JSON anywhere in the frame loop.
//!
//! Two staging buffers are filled before [`game_create`] and never again, because an
//! allocation after that point could grow linear memory and detach every JS view.

use crate::game::{gap, GameWorld, PlayData, MAP_SPAN, MAP_TILES, PLAYERS};
use crate::gamedata::GameData;
use crate::real::SUBTILE;
use crate::wire_gen;
use don_sim::deviations::{Deviation, ModeConfig, Surface};
use don_sim::systems::economy as econ;
use std::cell::UnsafeCell;

const CMD_SCRATCH: usize = 16 * 1024;
const PICK_MAX: usize = 4096;
/// `i32`s per player in the block [`game_players_ptr`] exposes.
pub const PLAYER_FIELDS: usize = 34;

struct Stage(UnsafeCell<Vec<u8>>);
// SAFETY: wasm32-unknown-unknown is single-threaded and every access happens between JS
// calls on one thread; the native twin is single-threaded too.
unsafe impl Sync for Stage {}
static GAMEDATA: Stage = Stage(UnsafeCell::new(Vec::new()));
static PLAYDATA: Stage = Stage(UnsafeCell::new(Vec::new()));

#[allow(clippy::mut_from_ref)]
fn stage(s: &'static Stage) -> &'static mut Vec<u8> {
    // SAFETY: single-threaded by construction; see `Stage`.
    unsafe { &mut *s.0.get() }
}

/// One playable game plus the scratch every query writes into.
pub struct Game {
    pub gd: GameData,
    pub pd: PlayData,
    pub world: GameWorld,
    cmd: Vec<u8>,
    pick: Vec<i16>,
    info: Vec<i32>,
    players: Vec<i32>,
    products: Vec<i32>,
    pending: Vec<u8>,
}

impl Game {
    fn new(seed: u64) -> Game {
        let gd = GameData::parse(stage(&GAMEDATA)).unwrap_or_else(GameData::synthetic);
        let pd = PlayData::parse(stage(&PLAYDATA)).unwrap_or_else(PlayData::empty);
        let world = GameWorld::new(&gd, &pd, seed);
        Game {
            gd,
            pd,
            world,
            cmd: vec![0; CMD_SCRATCH],
            pick: vec![0; PICK_MAX],
            info: vec![0; 32],
            players: vec![0; PLAYERS * PLAYER_FIELDS],
            products: vec![0; 512],
            pending: Vec::with_capacity(4096),
        }
    }

    /// Drain the queued commands into world state at a tick boundary, in arrival order —
    /// the lockstep discipline, and the reason a human clicking and a policy acting are the
    /// same code path.
    fn apply_commands(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        let q = std::mem::take(&mut self.pending);
        let mut o = 0usize;
        while o + 8 <= q.len() {
            let rd = |k: usize| u32::from_le_bytes([q[k], q[k + 1], q[k + 2], q[k + 3]]);
            let who = rd(o) as u8;
            let len = rd(o + 4) as usize;
            o += 8;
            if o + len > q.len() {
                break;
            }
            let b = &q[o..o + len];
            o += len;
            if b.is_empty() {
                continue;
            }
            self.world.commands_seen += 1;
            let w = &mut self.world;
            let n = match b[0] {
                wire_gen::op::GROUP => {
                    let num = wire_gen::group::num(b);
                    let field = wire_gen::group::who(b);
                    let player = if field >= 0 { field as u8 } else { who };
                    let ids: Vec<i16> =
                        (0..num).filter_map(|k| wire_gen::group::entry(b, k)).collect();
                    w.cmd_group(player, ids.into_iter())
                }
                wire_gen::op::MOVE_TO => {
                    w.cmd_move_to(who, wire_gen::move_to::to_x(b), wire_gen::move_to::to_y(b))
                }
                wire_gen::op::ATTACK => w.cmd_attack(who, wire_gen::attack::whom(b)),
                wire_gen::op::HALT => w.cmd_halt(who),
                wire_gen::op::GATHER => w.cmd_gather(who, wire_gen::gather::ox(b)),
                wire_gen::op::BUILD => w.cmd_build(
                    &self.pd,
                    who,
                    wire_gen::build::x(b),
                    wire_gen::build::y(b),
                    wire_gen::build::type_(b),
                ),
                wire_gen::op::QUEUE_UP => w.cmd_queue_up(
                    &self.pd,
                    who,
                    wire_gen::queue_up::type_(b),
                    wire_gen::queue_up::num(b),
                ),
                wire_gen::op::UNQUEUE => {
                    w.cmd_unqueue(&self.pd, who, wire_gen::unqueue::type_(b))
                }
                _ => {
                    w.gaps[gap::UNKNOWN_OPCODE] += 1;
                    0
                }
            };
            self.world.orders_applied += n as u64;
        }
        self.pending = q;
        self.pending.clear();
    }

    fn refresh_players(&mut self) {
        for p in 0..PLAYERS {
            let pl = &self.world.players[p];
            let b = &mut self.players[p * PLAYER_FIELDS..(p + 1) * PLAYER_FIELDS];
            for i in 0..econ::NUM_RESOURCES {
                b[i] = pl.econ.stockpile[i];
                b[6 + i] = pl.income[i];
                b[12 + i] = pl.econ.commerce_cap[i];
                b[18 + i] = pl.workers[i];
                b[24 + i] = pl.gross[i];
            }
            b[30] = pl.pop;
            b[31] = pl.pop_cap;
            b[32] = pl.econ.age;
            b[33] = if pl.researching >= 0 { pl.research_prog.max(1) } else { 0 };
        }
    }
}

macro_rules! game_ref {
    ($g:expr) => {
        // SAFETY: every entry point documents that `g` must be a live handle from
        // `game_create`; the null check keeps a zero handle from dereferencing.
        match unsafe { $g.as_mut() } {
            Some(r) => r,
            None => return Default::default(),
        }
    };
}
macro_rules! game_ptr {
    ($g:expr) => {
        match unsafe { $g.as_mut() } {
            Some(r) => r,
            None => return std::ptr::null_mut(),
        }
    };
}

// ---- staging ---------------------------------------------------------------------------

/// Reserve `len` bytes for `gamedata.bin` and return the offset. Call before
/// [`game_create`].
#[no_mangle]
pub extern "C" fn game_gamedata_alloc(len: u32) -> *mut u8 {
    let s = stage(&GAMEDATA);
    s.clear();
    s.resize(len as usize, 0);
    s.as_mut_ptr()
}

/// Reserve `len` bytes for `playdata.bin` and return the offset.
#[no_mangle]
pub extern "C" fn game_playdata_alloc(len: u32) -> *mut u8 {
    let s = stage(&PLAYDATA);
    s.clear();
    s.resize(len as usize, 0);
    s.as_mut_ptr()
}

// ---- lifecycle -------------------------------------------------------------------------

/// Create a game. The seed is split because a `u64` across the wasm C ABI becomes a BigInt
/// in JS.
#[no_mangle]
pub extern "C" fn game_create(seed_lo: u32, seed_hi: u32) -> *mut Game {
    let seed = ((seed_hi as u64) << 32) | seed_lo as u64;
    Box::into_raw(Box::new(Game::new(seed)))
}

/// # Safety
/// `g` must be a handle from [`game_create`] that has not already been destroyed.
#[no_mangle]
pub unsafe extern "C" fn game_destroy(g: *mut Game) {
    if !g.is_null() {
        // SAFETY: caller guarantees `g` came from `Box::into_raw` in `game_create`.
        drop(unsafe { Box::from_raw(g) });
    }
}

/// Advance `frames` simulation frames, draining the command queue at each tick boundary.
/// # Safety
/// `g` must be a live handle from [`game_create`].
#[no_mangle]
pub unsafe extern "C" fn game_step(g: *mut Game, frames: u32) {
    let game = match unsafe { g.as_mut() } {
        Some(r) => r,
        None => return,
    };
    for _ in 0..frames {
        game.apply_commands();
        let (gd, pd) = (&game.gd, &game.pd);
        game.world.step(gd, pd);
    }
    game.refresh_players();
}

// ---- the zero-copy render surface --------------------------------------------------------

/// # Safety
/// `g` must be a live handle from [`game_create`].
#[no_mangle]
pub unsafe extern "C" fn game_x_ptr(g: *mut Game) -> *mut i32 {
    game_ptr!(g).world.pos_x.as_mut_ptr()
}
/// # Safety
/// `g` must be a live handle from [`game_create`].
#[no_mangle]
pub unsafe extern "C" fn game_y_ptr(g: *mut Game) -> *mut i32 {
    game_ptr!(g).world.pos_y.as_mut_ptr()
}
/// # Safety
/// `g` must be a live handle from [`game_create`].
#[no_mangle]
pub unsafe extern "C" fn game_tag_ptr(g: *mut Game) -> *mut u32 {
    game_ptr!(g).world.tag.as_mut_ptr()
}
/// # Safety
/// `g` must be a live handle from [`game_create`].
#[no_mangle]
pub unsafe extern "C" fn game_live(g: *mut Game) -> u32 {
    game_ref!(g).world.live_count()
}
/// # Safety
/// `g` must be a live handle from [`game_create`].
#[no_mangle]
pub unsafe extern "C" fn game_capacity(g: *mut Game) -> u32 {
    game_ref!(g).world.capacity()
}

/// Stable object id for a live dense row, or `-1` when `row` is not live. The render
/// columns are row-indexed, while commands are id-indexed; exposing this bridge keeps
/// selection exact even when several objects overlap on screen.
/// # Safety
/// `g` must be a live handle from [`game_create`].
#[no_mangle]
pub unsafe extern "C" fn game_id_at_row(g: *mut Game, row: u32) -> i32 {
    let world = &game_ref!(g).world;
    if row >= world.live_count() {
        -1
    } else {
        world.id_of_row(row as usize)
    }
}

/// The tile mask array — `MAP_TILES * MAP_TILES` `u16`s, `TData::mask` verbatim. The
/// renderer turns this into its terrain texture; the bit meanings are
/// `don_sim::systems::map_terrain::tflag`.
/// # Safety
/// `g` must be a live handle from [`game_create`].
#[no_mangle]
pub unsafe extern "C" fn game_tile_ptr(g: *mut Game) -> *mut u16 {
    game_ptr!(g).world.terrain.tdata.as_mut_ptr()
}

/// Bumped on every tile-mask change, so the renderer re-uploads only when it must.
/// # Safety
/// `g` must be a live handle from [`game_create`].
#[no_mangle]
pub unsafe extern "C" fn game_terrain_version(g: *mut Game) -> u32 {
    game_ref!(g).world.terrain_version
}

// ---- commands ---------------------------------------------------------------------------

/// # Safety
/// `g` must be a live handle from [`game_create`].
#[no_mangle]
pub unsafe extern "C" fn game_cmd_ptr(g: *mut Game) -> *mut u8 {
    game_ptr!(g).cmd.as_mut_ptr()
}

#[no_mangle]
pub extern "C" fn game_cmd_capacity() -> u32 {
    CMD_SCRATCH as u32
}

/// Queue the first `len` bytes of the command scratch as a command from player `who`.
/// Returns the queue length in bytes.
/// # Safety
/// `g` must be a live handle from [`game_create`].
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

// ---- queries ------------------------------------------------------------------------------

/// # Safety
/// `g` must be a live handle from [`game_create`].
#[no_mangle]
pub unsafe extern "C" fn game_pick_ptr(g: *mut Game) -> *mut i16 {
    game_ptr!(g).pick.as_mut_ptr()
}

/// Ids of `who`'s objects inside a subtile box. Units win over buildings.
/// # Safety
/// `g` must be a live handle from [`game_create`].
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
    game.world.pick_box(who as u8, x0, y0, x1, y1, &mut game.pick) as u32
}

/// Ids of every object of `who` with this type id — "select all of type".
/// # Safety
/// `g` must be a live handle from [`game_create`].
#[no_mangle]
pub unsafe extern "C" fn game_pick_type(g: *mut Game, who: u32, type_id: i32) -> u32 {
    let game = game_ref!(g);
    game.world.pick_type(who as u8, type_id, &mut game.pick) as u32
}

/// Object id under a subtile point, any owner, or `-1`.
/// # Safety
/// `g` must be a live handle from [`game_create`].
#[no_mangle]
pub unsafe extern "C" fn game_pick_at(g: *mut Game, x: i32, y: i32) -> i32 {
    game_ref!(g).world.pick_at(x, y)
}

/// # Safety
/// `g` must be a live handle from [`game_create`].
#[no_mangle]
pub unsafe extern "C" fn game_info_ptr(g: *mut Game) -> *mut i32 {
    game_ptr!(g).info.as_mut_ptr()
}

/// Fill the info block for one object. Returns 1 when the id resolves.
///
/// Layout: `[id, owner, type_id, is_building, hits, max_hits, attack_x10, armor, range,
/// moves, recharge, order, order_arg, build_progress, job_time, queue_n, q0..q7, pop,
/// gather_res, pos_x, pos_y, footprint_x, footprint_y]`.
/// # Safety
/// `g` must be a live handle from [`game_create`].
#[no_mangle]
pub unsafe extern "C" fn game_object_info(g: *mut Game, id: i32) -> u32 {
    let game = game_ref!(g);
    let Some(row) = game.world.row_of_id(id) else { return 0 };
    let a = game.world.aux[row];
    let i = &mut game.info;
    for v in i.iter_mut() {
        *v = 0;
    }
    i[0] = id;
    i[1] = a.owner as i32;
    i[2] = a.type_id;
    i[3] = a.is_building as i32;
    i[4] = a.hits;
    i[5] = a.max_hits;
    if a.is_building {
        let b = game.pd.blds.get(a.tidx as usize).copied().unwrap_or_default();
        i[6] = b.attack;
        i[7] = b.armor;
        i[8] = b.max_range;
        i[9] = 0;
        i[10] = b.recharge;
        i[14] = b.job_time;
        i[24] = 0;
        i[27] = b.x_size;
        i[28] = b.y_size;
    } else {
        let u = game.gd.units[a.tidx as usize];
        i[6] = u.attack;
        i[7] = u.armor;
        i[8] = u.max_range;
        i[9] = u.moves;
        i[10] = u.recharge;
        i[14] = game.pd.unit(a.type_id).map(|p| p.job_time).unwrap_or(0);
        i[24] = game.pd.unit(a.type_id).map(|p| p.pop).unwrap_or(-1);
        i[27] = 1;
        i[28] = 1;
    }
    i[11] = a.order as i32;
    i[12] = a.order_a;
    i[13] = a.build_progress;
    i[15] = a.queue_n as i32;
    for k in 0..crate::game::QUEUE_MAX {
        i[16 + k] = a.queue[k];
    }
    i[25] = a.gather_res as i32;
    i[26] = game.world.pos_x[row];
    i[29] = game.world.pos_y[row];
    1
}

/// Per-player block: `PLAYERS * PLAYER_FIELDS` `i32`s.
///
/// Fields per player: `stockpile[6], income[6], cap[6], workers[6], gross[6], pop,
/// pop_cap, age, research_progress`.
/// # Safety
/// `g` must be a live handle from [`game_create`].
#[no_mangle]
pub unsafe extern "C" fn game_players_ptr(g: *mut Game) -> *mut i32 {
    game_ptr!(g).players.as_mut_ptr()
}

#[no_mangle]
pub extern "C" fn game_player_fields() -> u32 {
    PLAYER_FIELDS as u32
}

/// Issued-but-unexecuted counters, `gap::COUNT` `u32`s. The client shows them live.
/// # Safety
/// `g` must be a live handle from [`game_create`].
#[no_mangle]
pub unsafe extern "C" fn game_gaps_ptr(g: *mut Game) -> *mut u32 {
    game_ptr!(g).world.gaps.as_mut_ptr()
}

#[no_mangle]
pub extern "C" fn game_gap_count() -> u32 {
    gap::COUNT as u32
}

/// Number of exact wire packets drained at tick boundaries. This counts packets, including
/// ones that reached a fail-closed gap; [`game_orders_applied`] counts affected objects.
/// Together with the JS submit counter this makes queued -> drained -> applied visible.
/// # Safety
/// `g` must be a live handle from [`game_create`].
#[no_mangle]
pub unsafe extern "C" fn game_commands_seen(g: *mut Game) -> u32 {
    game_ref!(g).world.commands_seen as u32
}

/// Number of object-order mutations returned by command handlers.
/// # Safety
/// `g` must be a live handle from [`game_create`].
#[no_mangle]
pub unsafe extern "C" fn game_orders_applied(g: *mut Game) -> u32 {
    game_ref!(g).world.orders_applied as u32
}

fn playable_blocker(index: u32) -> Option<Deviation> {
    ModeConfig::improved()
        .readiness_blockers(Surface::PlayableEdition)
        .nth(index as usize)
        .map(|b| b.deviation())
}

/// Current repository-level blockers for the playable product surface, taken directly
/// from `don_sim::deviations`. These are not a claim that this standalone `GameWorld` is
/// the Arena; the browser labels that separate runtime identity explicitly.
#[no_mangle]
pub extern "C" fn game_playable_blocker_count() -> u32 {
    ModeConfig::improved()
        .readiness_blockers(Surface::PlayableEdition)
        .count() as u32
}

#[no_mangle]
pub extern "C" fn game_playable_blocker_slug_ptr(index: u32) -> *const u8 {
    playable_blocker(index)
        .map(|d| d.entry().slug.as_ptr())
        .unwrap_or(std::ptr::null())
}

#[no_mangle]
pub extern "C" fn game_playable_blocker_slug_len(index: u32) -> u32 {
    playable_blocker(index)
        .map(|d| d.entry().slug.len() as u32)
        .unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn game_playable_blocker_title_ptr(index: u32) -> *const u8 {
    playable_blocker(index)
        .map(|d| d.entry().title.as_ptr())
        .unwrap_or(std::ptr::null())
}

#[no_mangle]
pub extern "C" fn game_playable_blocker_title_len(index: u32) -> u32 {
    playable_blocker(index)
        .map(|d| d.entry().title.len() as u32)
        .unwrap_or(0)
}

/// `WorldData::space_at_corner`'s grade for a footprint at a tile: 0 blocked, 2 partial,
/// 3 approach clear, 4 fully clear. **This is the engine's own predicate** and it is what
/// tints the placement preview.
/// # Safety
/// `g` must be a live handle from [`game_create`].
#[no_mangle]
pub unsafe extern "C" fn game_placement_grade(
    g: *mut Game,
    who: u32,
    type_id: i32,
    tx: i32,
    ty: i32,
) -> i32 {
    let game = game_ref!(g);
    game.world.grade_placement(&game.pd, who as i32, type_id, tx, ty)
}

/// `WorldData::check_building_wcoord`'s grade for the W cell containing a tile.
/// # Safety
/// `g` must be a live handle from [`game_create`].
#[no_mangle]
pub unsafe extern "C" fn game_check_wcell(g: *mut Game, who: u32, tx: i32, ty: i32) -> i32 {
    let game = game_ref!(g);
    game.world.check_wcell(who as i32, tx >> 2, ty >> 2, 3, 3, 4)
}

/// Resource slot a tile yields (0..5), or `-1`. The gate is `has_gather_access`.
/// # Safety
/// `g` must be a live handle from [`game_create`].
#[no_mangle]
pub unsafe extern "C" fn game_tile_resource(g: *mut Game, tx: i32, ty: i32) -> i32 {
    game_ref!(g).world.tile_resource(tx, ty).map(|r| r as i32).unwrap_or(-1)
}

/// Fill [`game_products_ptr`] with the product type ids of a producer and return the count.
/// This is the `WHERE` join, so a menu can never offer something the tables do not.
/// # Safety
/// `g` must be a live handle from [`game_create`].
#[no_mangle]
pub unsafe extern "C" fn game_products(g: *mut Game, producer_type: i32) -> u32 {
    let game = game_ref!(g);
    let ids: Vec<i32> = game.pd.products_of(producer_type).collect();
    let n = ids.len().min(game.products.len());
    game.products[..n].copy_from_slice(&ids[..n]);
    n as u32
}

/// # Safety
/// `g` must be a live handle from [`game_create`].
#[no_mangle]
pub unsafe extern "C" fn game_products_ptr(g: *mut Game) -> *mut i32 {
    game_ptr!(g).products.as_mut_ptr()
}

/// 1 when the packed play data (costs, footprints, producer edges) is present.
/// # Safety
/// `g` must be a live handle from [`game_create`].
#[no_mangle]
pub unsafe extern "C" fn game_has_playdata(g: *mut Game) -> u32 {
    game_ref!(g).pd.is_real as u32
}

/// 1 when the packed unit/balance tables are present.
/// # Safety
/// `g` must be a live handle from [`game_create`].
#[no_mangle]
pub unsafe extern "C" fn game_has_gamedata(g: *mut Game) -> u32 {
    game_ref!(g).gd.is_real as u32
}

/// # Safety
/// `g` must be a live handle from [`game_create`].
#[no_mangle]
pub unsafe extern "C" fn game_frame(g: *mut Game) -> u32 {
    game_ref!(g).world.frame as u32
}

/// # Safety
/// `g` must be a live handle from [`game_create`].
#[no_mangle]
pub unsafe extern "C" fn game_digest_lo(g: *mut Game) -> u32 {
    game_ref!(g).world.digest() as u32
}
/// # Safety
/// `g` must be a live handle from [`game_create`].
#[no_mangle]
pub unsafe extern "C" fn game_digest_hi(g: *mut Game) -> u32 {
    (game_ref!(g).world.digest() >> 32) as u32
}

/// Start position of a player, in subtiles — what "jump to my base" needs.
/// # Safety
/// `g` must be a live handle from [`game_create`].
#[no_mangle]
pub unsafe extern "C" fn game_start_x(g: *mut Game, p: u32) -> i32 {
    game_ref!(g).world.start_x[(p as usize) % PLAYERS]
}
/// # Safety
/// `g` must be a live handle from [`game_create`].
#[no_mangle]
pub unsafe extern "C" fn game_start_y(g: *mut Game, p: u32) -> i32 {
    game_ref!(g).world.start_y[(p as usize) % PLAYERS]
}

/// Set the population-limit game option, `0..7` indexing the derived `POP_CAP` array
/// `{25,50,75,100,125,150,175,200}`.
/// # Safety
/// `g` must be a live handle from [`game_create`].
#[no_mangle]
pub unsafe extern "C" fn game_set_pop_setting(g: *mut Game, k: u32) {
    let game = match unsafe { g.as_mut() } {
        Some(r) => r,
        None => return,
    };
    game.world.pop_cap_setting = (k as usize).min(7);
}

/// Income experiment: 0 runs retail's `Leader::do_gather` commerce-cap step; 1 lifts
/// only that clamp. This switch does not label the larger playable world Fidelity mode.
/// # Safety
/// `g` must be a live handle from [`game_create`].
#[no_mangle]
pub unsafe extern "C" fn game_set_income_mode(g: *mut Game, mode: u32) {
    let game = match unsafe { g.as_mut() } {
        Some(r) => r,
        None => return,
    };
    game.world.income_mode = mode.min(1);
}

/// **Benchmark hook** — see `GameWorld::debug_spawn`. Returns how many were made.
/// # Safety
/// `g` must be a live handle from [`game_create`].
#[no_mangle]
pub unsafe extern "C" fn game_debug_spawn(g: *mut Game, owner: u32, type_id: i32, n: u32) -> u32 {
    let game = game_ref!(g);
    let gd = &game.gd;
    game.world.debug_spawn(gd, owner as u8, type_id, n)
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
    SUBTILE
}
#[no_mangle]
pub extern "C" fn game_players_count() -> u32 {
    PLAYERS as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_readiness_is_the_compiled_playable_registry() {
        let expected: Vec<_> = ModeConfig::improved()
            .readiness_blockers(Surface::PlayableEdition)
            .map(|b| b.deviation())
            .collect();
        assert!(!expected.is_empty(), "the page must not advertise product readiness");
        assert_eq!(game_playable_blocker_count() as usize, expected.len());
        for (i, d) in expected.into_iter().enumerate() {
            assert_eq!(game_playable_blocker_slug_len(i as u32), d.entry().slug.len() as u32);
            assert_eq!(game_playable_blocker_title_len(i as u32), d.entry().title.len() as u32);
        }
        assert!(game_playable_blocker_slug_ptr(u32::MAX).is_null());
        assert_eq!(game_playable_blocker_title_len(u32::MAX), 0);
    }
}
