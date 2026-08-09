//! Shapes: the factored action space, the mask layout, and the observation layout.
//!
//! Everything downstream (mask writer, observation writer, the Python spaces) is derived
//! from [`EnvConfig`] through this module, so a shape can only be wrong in one place.

use crate::generated as g;

/// Provisioning and semantics of one vectorised environment.
///
/// Sizes that the engine fixes (`NUM_TYPES`, `NUM_PLAYERS`, the tick period) are
/// constants in [`crate::generated`] and are *not* configurable. Sizes the engine does not
/// fix for us yet — map extent, how many entities a policy may address — are here.
#[derive(Clone, Debug)]
pub struct EnvConfig {
    /// Observation grid width in tiles. The real map extents are underived; this is the
    /// resolution the spatial planes are rasterised at, not a claim about map size.
    pub grid_w: usize,
    pub grid_h: usize,
    /// Rows of the entity list an observation exposes, and therefore the size of the
    /// `TargetEntity` head minus one.
    pub max_entities: usize,
    /// Entities a single agent may command per step. Actions beyond this are dropped.
    pub max_controlled: usize,
    /// Agents (player slots) that are policy-controlled. `1..=8`.
    pub num_agents: usize,
    /// Simulation frames advanced per `step`. 1 = one 67 ms tick at Normal.
    pub frames_per_step: u32,
    /// Episode cap in steps; 0 disables truncation.
    pub max_steps: u32,
    /// Units spawned per player at reset. Placeholder scenario: the real start-position
    /// generator is underived.
    pub start_units: usize,
    /// Whether observations are filtered by line of sight. The fog model itself is not
    /// derived; when true the env applies a LOS-radius reveal from `TypeCap::los` and
    /// says so in [`crate::env::VecEnv::provenance`].
    pub fog: bool,
    pub seed: u64,
}

impl Default for EnvConfig {
    fn default() -> Self {
        EnvConfig {
            grid_w: 64,
            grid_h: 64,
            max_entities: 64,
            max_controlled: 32,
            num_agents: 2,
            frames_per_step: 1,
            max_steps: 4096,
            start_units: 16,
            fog: false,
            seed: 0x5EED,
        }
    }
}

impl EnvConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.num_agents == 0 || self.num_agents > g::NUM_PLAYERS {
            return Err(format!("num_agents must be 1..={}", g::NUM_PLAYERS));
        }
        if self.grid_w == 0 || self.grid_h == 0 {
            return Err("grid must be non-empty".into());
        }
        if self.max_controlled > self.max_entities {
            return Err("max_controlled must be <= max_entities".into());
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Action heads
// ---------------------------------------------------------------------------

/// Sizes of the ten unit-action heads, in `UnitHead` order.
pub fn unit_head_sizes(c: &EnvConfig) -> [usize; g::N_UNIT_HEADS] {
    [
        g::N_UNIT_VERBS + 1, // 0 Verb, index 0 = NOOP
        c.grid_w,            // 1 TargetX
        c.grid_h,            // 2 TargetY
        c.max_entities + 1,  // 3 TargetEntity, index 0 = none
        g::NUM_TYPES,        // 4 Type
        3,                   // 5 QueuePos   (QUEUE_FIRST/LAST/NEW)
        4,                   // 6 Stance     (StanceTypes, 4 real values)
        g::FORMS.len(),      // 7 Form       (FormIndex)
        8,                   // 8 OrderMods  (3 player-settable ORDER_* bits)
        5,                   // 9 Count      {1,2,3,5,10}
    ]
}

/// Sizes of the five player-action heads, in `PlayerHead` order.
pub fn player_head_sizes(_c: &EnvConfig) -> [usize; g::N_PLAYER_HEADS] {
    [
        g::N_PLAYER_VERBS + 1, // 0 Verb
        g::NUM_PLAYERS,        // 1 TargetPlayer
        g::NUM_COMMON,         // 2 Good
        AMOUNT_BUCKETS.len(),  // 3 Amount
        3,                     // 4 Treaty (WAR/PEACE/ALLY)
    ]
}

/// Tribute/market quantities the `Amount` head selects. The engine's field is a raw int;
/// these buckets are an interface decision, marked as such.
pub const AMOUNT_BUCKETS: [i32; 8] = [10, 25, 50, 100, 250, 500, 1000, 5000];
/// Values the `Count` head selects for `QueueUp::num`.
pub const COUNT_BUCKETS: [i32; 5] = [1, 2, 3, 5, 10];

/// Byte offset of each head inside one entity's mask record, plus the record size.
///
/// Masks are **bit-packed**, LSB first within each byte, and each head starts on a byte
/// boundary. Bit packing is not a micro-optimisation here: the `Type` head alone is 806
/// wide, so a `bool` mask would cost 806 stores per entity per step where the packed form
/// costs a 101-byte `AND` of two precomputed bitsets.
#[derive(Clone, Debug)]
pub struct MaskLayout {
    pub offsets: Vec<usize>,
    pub sizes: Vec<usize>,
    pub record_bytes: usize,
}

impl MaskLayout {
    pub fn new(sizes: &[usize]) -> MaskLayout {
        let mut offsets = Vec::with_capacity(sizes.len());
        let mut at = 0usize;
        for s in sizes {
            offsets.push(at);
            at += s.div_ceil(8);
        }
        MaskLayout {
            offsets,
            sizes: sizes.to_vec(),
            record_bytes: at,
        }
    }
    #[inline]
    pub fn head<'a>(&self, rec: &'a mut [u8], h: usize) -> &'a mut [u8] {
        let o = self.offsets[h];
        let n = self.sizes[h].div_ceil(8);
        &mut rec[o..o + n]
    }
}

#[inline]
pub fn set_bit(buf: &mut [u8], i: usize) {
    buf[i / 8] |= 1 << (i % 8);
}
#[inline]
pub fn get_bit(buf: &[u8], i: usize) -> bool {
    buf[i / 8] & (1 << (i % 8)) != 0
}
/// Set bits `0..n`, clearing the unused tail of the final byte.
#[inline]
pub fn fill_bits(buf: &mut [u8], n: usize) {
    let full = n / 8;
    buf[..full].fill(0xFF);
    if full < buf.len() {
        buf[full] = if n % 8 == 0 {
            0
        } else {
            (1u16 << (n % 8)) as u8 - 1
        };
        buf[full + 1..].fill(0);
    }
}

// ---------------------------------------------------------------------------
// Observation layout
// ---------------------------------------------------------------------------

/// Spatial feature planes, `f32`, shape `(planes, grid_h, grid_w)`.
///
/// Plane choice is taken from what the engine itself treats as sim-critical: the
/// `FLAG_*` terrain coordinate flags and the per-owner object occupancy that
/// `Objects::process_all` walks. Planes whose source is not modelled yet are listed in
/// [`crate::env::VecEnv::provenance`] rather than quietly emitting zeros.
pub const SPATIAL_PLANES: [&str; 12] = [
    "own_units", // count of the observing agent's entities in the tile
    "ally_units",
    "enemy_units",
    "own_buildings",
    "enemy_buildings",
    "hp_fraction",   // mean hits/max over entities in the tile
    "cooldown",      // mean recharge counter, normalised
    "resource_node", // FLAG_RESOURCE / FLAG_GOODY
    "blocking",      // FLAG_MOUNTAIN | FLAG_CLIFF | FLAG_ROCK | FLAG_NEAR_BLOCKING
    "water",         // FLAG_COAST | FLAG_RIVER
    "visible",       // in line of sight this frame
    "explored",      // ever seen
];

/// Per-entity feature columns, `f32`, shape `(max_entities, ENTITY_FEATURES.len())`.
///
/// Names are the engine's own field names where one exists: `Object::myhits`,
/// `Unit::stance`, `Unit::myarmor`, `Unit::myspeed`, `Unit::recharging`, `Unit::form`,
/// `GuyData::{x,y,angle}`, `Unit::orders_x/orders_y` (see `schema/state-schema.json`).
pub const ENTITY_FEATURES: [&str; 16] = [
    "alive",
    "relation", // 0 self, 1 ally, 2 enemy, 3 neutral
    "x",        // tile, normalised to [0,1)
    "y",
    "type_index", // TypeIndex / NUM_TYPES
    "category",   // TypeCap::category / 4
    "myhits",     // Object::myhits, normalised by type HITS
    "myarmor",
    "myspeed",
    "recharging", // Unit::recharging, normalised by type RECHARGE
    "stance",     // Unit::stance
    "form",       // Unit::form
    "order",      // OrderIndex currently executing
    "orders_x",   // Unit::orders_x, tile-normalised
    "orders_y",
    "controllable", // 1 if this agent may address it this step
];

/// Per-agent global vector. Every entry is a `LeaderData` field or a direct function of
/// one; offsets are from `schema/state-schema.json`.
pub const GLOBAL_FEATURES: [&str; 24] = [
    "econ_food",
    "econ_timber",
    "econ_wealth",
    "econ_knowledge",
    "econ_metal",
    "econ_oil",
    "base_rate_food",
    "base_rate_timber",
    "base_rate_wealth",
    "base_rate_knowledge",
    "base_rate_metal",
    "base_rate_oil",
    "pop",
    "pop_cap",
    "city_num",
    "units_built",
    "units_killed",
    "units_lost",
    "score",
    "territory",
    "explored",
    "frame",
    "step_frac",
    "num_alive_players",
];
