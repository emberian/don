//! Observation encoders. Every writer takes a `&mut [f32]` slice of a buffer the env owns
//! for its whole lifetime, so the Python side is a `numpy` view over Rust memory and no
//! observation is ever serialised or copied.

use crate::generated as g;
use crate::spec::{EnvConfig, ENTITY_FEATURES, GLOBAL_FEATURES, SPATIAL_PLANES};
use crate::state::EnvWorld;
use crate::typecaps::F_BUILDING;
use don_sim::world::SUBTILE;

pub const N_PLANES: usize = SPATIAL_PLANES.len();
pub const N_ENTITY_FEATURES: usize = ENTITY_FEATURES.len();
pub const N_GLOBAL_FEATURES: usize = GLOBAL_FEATURES.len();

/// Planes whose source exists in this build. The rest are emitted as zeros and named in
/// `VecEnv::provenance`, because a silently-zero plane is indistinguishable from a real
/// one to a network and would be discovered only as an unexplained plateau.
pub const LIVE_PLANES: [&str; 7] = [
    "own_units", "ally_units", "enemy_units", "own_buildings", "enemy_buildings",
    "hp_fraction", "cooldown",
];

/// Pick the entity rows an agent observes, nearest-first around its own centroid so the
/// truncation to `max_entities` is a locality choice rather than an arbitrary one.
pub fn select_entities(w: &EnvWorld, cfg: &EnvConfig, who: u8, out: &mut Vec<usize>) {
    out.clear();
    let n = w.sim.live_count() as usize;
    // Own entities first, in row order.
    for row in 0..n {
        if w.sim.owner()[row] == who as i8 {
            out.push(row);
            if out.len() == cfg.max_entities {
                return;
            }
        }
    }
    if out.is_empty() {
        for row in 0..n {
            out.push(row);
            if out.len() == cfg.max_entities {
                return;
            }
        }
        return;
    }
    // Then everyone else, by squared distance to the owner's centroid.
    let (mut cx, mut cy) = (0i64, 0i64);
    for &r in out.iter() {
        cx += w.sim.pos_x()[r] as i64;
        cy += w.sim.pos_y()[r] as i64;
    }
    cx /= out.len() as i64;
    cy /= out.len() as i64;
    let mut others: Vec<(i64, usize)> = (0..n)
        .filter(|&r| w.sim.owner()[r] != who as i8)
        .map(|r| {
            let dx = w.sim.pos_x()[r] as i64 - cx;
            let dy = w.sim.pos_y()[r] as i64 - cy;
            (dx * dx + dy * dy, r)
        })
        .collect();
    others.sort_unstable();
    for (_, r) in others {
        if out.len() == cfg.max_entities {
            break;
        }
        out.push(r);
    }
}

/// `(N_PLANES, grid_h, grid_w)` f32, agent-relative.
pub fn write_spatial(w: &EnvWorld, cfg: &EnvConfig, who: u8, out: &mut [f32]) {
    out.fill(0.0);
    let (gw, gh) = (cfg.grid_w, cfg.grid_h);
    let plane = gw * gh;
    let n = w.sim.live_count() as usize;
    let mut occupancy = vec![0f32; plane];
    for row in 0..n {
        let (tx, ty) = w.tile_of(row);
        let (tx, ty) = (tx.clamp(0, gw as i32 - 1) as usize, ty.clamp(0, gh as i32 - 1) as usize);
        let idx = ty * gw + tx;
        let owner = w.sim.owner()[row];
        let building = w.rules.caps.get(w.type_index[row]).has(F_BUILDING);
        let rel = w.relation(who, owner);
        let p = match (building, rel) {
            (false, 0) => 0,
            (false, 1) => 1,
            (false, _) => 2,
            (true, 0) | (true, 1) => 3,
            (true, _) => 4,
        };
        out[p * plane + idx] += 1.0;
        out[5 * plane + idx] += w.sim.hits()[row] as f32 / w.max_hits[row].max(1) as f32;
        out[6 * plane + idx] += w.sim.cooldown()[row] as f32 / 64.0;
        occupancy[idx] += 1.0;
        if cfg.fog {
            // LOS reveal from the type's own `LOS` (TCoords; 4 TCoords = 1 tile).
            if rel == 0 || rel == 1 {
                let los = (w.rules.caps.get(w.type_index[row]).los as i32 / 4).max(1);
                for dy in -los..=los {
                    for dx in -los..=los {
                        if dx * dx + dy * dy > los * los {
                            continue;
                        }
                        let (x, y) = (tx as i32 + dx, ty as i32 + dy);
                        if x < 0 || y < 0 || x >= gw as i32 || y >= gh as i32 {
                            continue;
                        }
                        let j = y as usize * gw + x as usize;
                        out[10 * plane + j] = 1.0;
                        out[11 * plane + j] = 1.0;
                    }
                }
            }
        }
    }
    // Turn the two accumulators into means.
    for i in 0..plane {
        if occupancy[i] > 0.0 {
            out[5 * plane + i] /= occupancy[i];
            out[6 * plane + i] /= occupancy[i];
        }
    }
    if !cfg.fog {
        out[10 * plane..12 * plane].fill(1.0);
    } else {
        // Anything not revealed this frame is masked out of the object planes.
        for i in 0..plane {
            if out[10 * plane + i] == 0.0 {
                for p in [2usize, 4] {
                    out[p * plane + i] = 0.0;
                }
            }
        }
    }
}

/// `(max_entities, N_ENTITY_FEATURES)` f32.
pub fn write_entities(
    w: &EnvWorld,
    cfg: &EnvConfig,
    who: u8,
    rows: &[usize],
    controlled: usize,
    out: &mut [f32],
) {
    out.fill(0.0);
    let (gwx, ghy) = ((cfg.grid_w * SUBTILE as usize) as f32, (cfg.grid_h * SUBTILE as usize) as f32);
    for (slot, &row) in rows.iter().enumerate().take(cfg.max_entities) {
        let f = &mut out[slot * N_ENTITY_FEATURES..(slot + 1) * N_ENTITY_FEATURES];
        let c = w.rules.caps.get(w.type_index[row]);
        f[0] = 1.0;
        f[1] = w.relation(who, w.sim.owner()[row]) as f32;
        f[2] = w.sim.pos_x()[row] as f32 / gwx;
        f[3] = w.sim.pos_y()[row] as f32 / ghy;
        f[4] = w.type_index[row] as f32 / g::NUM_TYPES as f32;
        f[5] = c.category as f32 / 4.0;
        f[6] = w.sim.hits()[row] as f32 / w.max_hits[row].max(1) as f32;
        f[7] = w.armor[row] as f32 / 16.0;
        f[8] = w.speed[row] as f32 / 128.0;
        f[9] = w.sim.cooldown()[row] as f32 / c.recharge.max(1) as f32;
        f[10] = w.stance[row] as f32;
        f[11] = w.form[row] as f32;
        f[12] = w.order[row] as f32 / 28.0;
        f[13] = w.dest_x[row] as f32 / gwx;
        f[14] = w.dest_y[row] as f32 / ghy;
        f[15] = if slot < controlled && w.sim.owner()[row] == who as i8 { 1.0 } else { 0.0 };
    }
}

/// `(N_GLOBAL_FEATURES,)` f32.
pub fn write_global(w: &EnvWorld, cfg: &EnvConfig, who: u8, out: &mut [f32]) {
    let p = &w.players[who as usize];
    for k in 0..g::NUM_COMMON {
        out[k] = p.econ[k] as f32 / 1000.0;
        out[6 + k] = p.base_rate[k] as f32 / 100.0;
    }
    out[12] = p.pop as f32 / 200.0;
    out[13] = p.pop_cap as f32 / 200.0;
    out[14] = p.city_num as f32 / 16.0;
    out[15] = p.units_built as f32 / 100.0;
    out[16] = p.units_killed as f32 / 100.0;
    out[17] = p.units_lost as f32 / 100.0;
    out[18] = p.score.total() as f32 / 1000.0;
    out[19] = p.territory as f32 / 1000.0;
    out[20] = p.explored as f32 / 1000.0;
    out[21] = w.sim.frame as f32 / 10_000.0;
    out[22] = if cfg.max_steps > 0 { w.step_index as f32 / cfg.max_steps as f32 } else { 0.0 };
    out[23] = w.players.iter().filter(|q| q.alive).count() as f32 / g::NUM_PLAYERS as f32;
}
