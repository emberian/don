//! Tier-C caster, stealth, and Gaia/animal primitives.
//!
//! This is a bounded recovery of Claude lane `aaa5073c047dfb20c` from session
//! `62b78482-846c-4ffd-a44c-2199d3744a8e`.  It deliberately stops at the seams the
//! interrupted lane had actually derived: active-spell expiry, mana capacity, cloak and
//! detection predicates, the first sixteen ability records, herd scheduling/migration,
//! herd spawn counts/angle draws, and farm-animal spawn plans.
//!
//! Every behavioural function below is Tier C: transcribed from the shipped executable
//! and PDB, then tested locally, but not yet compared against a live retail oracle or a
//! replay checksum.  See `docs/mechanics/casters-animals.md` for the evidence table and
//! the intentionally unimplemented boundaries.

// Standalone `rustc --test` loads the crate's real RNG implementation for an integration
// check without making this not-yet-declared module depend on crate-root wiring.
#[cfg(test)]
#[path = "../rng.rs"]
mod recovered_test_rng;

// -----------------------------------------------------------------------------
// Spell type IDs and static ability facts
// -----------------------------------------------------------------------------

pub const TYPE_BRIBE: i32 = 629;
pub const TYPE_PILFER: i32 = 630;
pub const TYPE_COUNTERINTEL: i32 = 631;
pub const TYPE_ASSIMILATE: i32 = 632;
pub const TYPE_RALLY: i32 = 633;
pub const TYPE_CREATE_DECOY: i32 = 634;
pub const TYPE_AMBUSH: i32 = 635;
pub const TYPE_FORCED_MARCH: i32 = 636;
pub const TYPE_ENTRENCH: i32 = 637;
pub const TYPE_RESTOCK_SUPPLIES: i32 = 638;
pub const TYPE_DOUBLE_AGENT: i32 = 639;
pub const TYPE_SABOTAGE: i32 = 640;
pub const TYPE_SNIPER: i32 = 641;
pub const TYPE_BLOW_TREAD: i32 = 642;
pub const TYPE_JAM_RADAR: i32 = 643;
pub const TYPE_PARADROP: i32 = 644;

/// A literal row from the first sixteen `<CRAFT>` entries in shipped
/// `ron-data/craftrules.xml`.
///
/// `research_cost` and `cast_cost` intentionally remain raw strings.  The XML itself says
/// `COST` is the research cost and `COST2` is the cast cost; treating the common
/// `20g/20w` research placeholder as a live cast payment would be wrong.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AbilityFact {
    pub type_id: i32,
    /// PDB `TypeIndex` spelling.  Type 639 is `DOUBLE_AGENT` although the XML display name
    /// is "Informer".
    pub type_name: &'static str,
    pub name: &'static str,
    pub flags: &'static str,
    pub research_cost: &'static str,
    pub cast_cost: &'static str,
    /// XML unit: 1/15 second.
    pub job_time: i32,
    /// Frames, as demonstrated by `cast_ambush`/`cast_march` adding it to `Game::frame`.
    pub duration: i32,
    pub duration_per_upgrade: i32,
    /// "Craft" points in the shipped XML terminology.
    pub mana: i32,
    /// Tile coordinates (`TCoord`) per the shipped XML comment.
    pub range: i32,
    pub from: &'static str,
    pub from2: &'static str,
    /// False means the shipped `<PREQ0>` is `Disable`/`disable`.
    pub enabled: bool,
}

/// The active spy/general/commando ability block, TypeIndex 629..=644.
///
/// These values are data, not a claim that all sixteen pass the runtime
/// `SpellTypeData::is_castable` gate.  In particular, six rows are disabled in the
/// shipped XML.
pub const CORE_ABILITIES: [AbilityFact; 16] = [
    AbilityFact {
        type_id: TYPE_BRIBE,
        type_name: "BRIBE",
        name: "Bribe",
        flags: "fcbhm",
        research_cost: "",
        cast_cost: "",
        job_time: 100,
        duration: 0,
        duration_per_upgrade: 0,
        mana: 1000,
        range: 6,
        from: "Spy",
        from2: "The Senator",
        enabled: true,
    },
    AbilityFact {
        type_id: TYPE_PILFER,
        type_name: "PILFER",
        name: "Pilfer Resources",
        flags: "fcm",
        research_cost: "20g/20w",
        cast_cost: "",
        job_time: 100,
        duration: 0,
        duration_per_upgrade: 0,
        mana: 1000,
        range: 1,
        from: "None",
        from2: "None",
        enabled: false,
    },
    AbilityFact {
        type_id: TYPE_COUNTERINTEL,
        type_name: "COUNTERINTEL",
        name: "Counterintelligence",
        flags: "febchm",
        research_cost: "",
        cast_cost: "",
        job_time: 38,
        duration: 0,
        duration_per_upgrade: 0,
        mana: 500,
        range: 10,
        from: "Spy",
        from2: "Scout",
        enabled: true,
    },
    AbilityFact {
        type_id: TYPE_ASSIMILATE,
        type_name: "ASSIMILATE",
        name: "Assimilate",
        flags: "ecfh",
        research_cost: "",
        cast_cost: "",
        job_time: 100,
        duration: 0,
        duration_per_upgrade: 0,
        mana: 2000,
        range: 1,
        from: "None",
        from2: "None",
        enabled: false,
    },
    AbilityFact {
        type_id: TYPE_RALLY,
        type_name: "RALLY",
        name: "Rally!",
        flags: "glm",
        research_cost: "",
        cast_cost: "",
        job_time: 0,
        duration: 15,
        duration_per_upgrade: 0,
        mana: 1000,
        range: 0,
        from: "None",
        from2: "None",
        enabled: false,
    },
    AbilityFact {
        type_id: TYPE_CREATE_DECOY,
        type_name: "CREATE_DECOY",
        name: "Create Decoys",
        flags: "lm",
        research_cost: "20g/20w",
        cast_cost: "",
        job_time: 100,
        duration: 0,
        duration_per_upgrade: 0,
        mana: 1000,
        range: 0,
        from: "General",
        from2: "None",
        enabled: true,
    },
    AbilityFact {
        type_id: TYPE_AMBUSH,
        type_name: "AMBUSH",
        name: "Ambush",
        flags: "glm",
        research_cost: "20g/20w",
        cast_cost: "",
        job_time: 0,
        duration: 30,
        duration_per_upgrade: 15,
        mana: 1000,
        range: 0,
        from: "General",
        from2: "None",
        enabled: true,
    },
    AbilityFact {
        type_id: TYPE_FORCED_MARCH,
        type_name: "FORCED_MARCH",
        name: "Forced March",
        flags: "glm",
        research_cost: "20g/20w",
        cast_cost: "",
        job_time: 0,
        duration: 10,
        duration_per_upgrade: 5,
        mana: 1000,
        range: 0,
        from: "General",
        from2: "None",
        enabled: true,
    },
    AbilityFact {
        type_id: TYPE_ENTRENCH,
        type_name: "ENTRENCH",
        name: "Entrench",
        flags: "lm",
        research_cost: "20g/20w",
        cast_cost: "",
        job_time: 120,
        duration: 0,
        duration_per_upgrade: 0,
        mana: 1000,
        range: 0,
        from: "General",
        from2: "None",
        enabled: true,
    },
    AbilityFact {
        type_id: TYPE_RESTOCK_SUPPLIES,
        type_name: "RESTOCK_SUPPLIES",
        name: "Restock Supplies",
        flags: "",
        research_cost: "",
        cast_cost: "2w/2m/2c/2o/2f",
        job_time: 200,
        duration: 0,
        duration_per_upgrade: 0,
        mana: 0,
        range: 0,
        from: "None",
        from2: "None",
        enabled: false,
    },
    AbilityFact {
        type_id: TYPE_DOUBLE_AGENT,
        type_name: "DOUBLE_AGENT",
        name: "Informer",
        flags: "fbcml",
        research_cost: "20g/20w",
        cast_cost: "",
        job_time: 40,
        duration: 0,
        duration_per_upgrade: 0,
        mana: 500,
        range: 10,
        from: "Spy",
        from2: "None",
        enabled: true,
    },
    AbilityFact {
        type_id: TYPE_SABOTAGE,
        type_name: "SABOTAGE",
        name: "Sabotage",
        flags: "fcm",
        research_cost: "",
        cast_cost: "",
        job_time: 150,
        duration: 0,
        duration_per_upgrade: 0,
        mana: 1000,
        range: 1,
        from: "Commando",
        from2: "None",
        enabled: true,
    },
    AbilityFact {
        type_id: TYPE_SNIPER,
        type_name: "SNIPER",
        name: "Sniper",
        flags: "fbm",
        research_cost: "",
        cast_cost: "",
        job_time: 60,
        duration: 0,
        duration_per_upgrade: 0,
        mana: 1000,
        range: 12,
        from: "Commando",
        from2: "None",
        enabled: true,
    },
    AbilityFact {
        type_id: TYPE_BLOW_TREAD,
        type_name: "BLOW_TREAD",
        name: "Blow Tread",
        flags: "fbim",
        research_cost: "",
        cast_cost: "",
        job_time: 25,
        duration: 0,
        duration_per_upgrade: 0,
        mana: 500,
        range: 6,
        from: "none",
        from2: "None",
        enabled: false,
    },
    AbilityFact {
        type_id: TYPE_JAM_RADAR,
        type_name: "JAM_RADAR",
        name: "Jam Radar",
        flags: "gm",
        research_cost: "",
        cast_cost: "",
        job_time: 0,
        duration: 15,
        duration_per_upgrade: 0,
        mana: 1000,
        range: 12,
        from: "None",
        from2: "None",
        enabled: true,
    },
    AbilityFact {
        type_id: TYPE_PARADROP,
        type_name: "PARADROP",
        name: "Paradrop",
        flags: "dlm",
        research_cost: "",
        cast_cost: "",
        job_time: 25,
        duration: 0,
        duration_per_upgrade: 0,
        mana: 500,
        range: 0,
        from: "None",
        from2: "None",
        enabled: false,
    },
];

#[inline]
pub fn ability_fact(type_id: i32) -> Option<&'static AbilityFact> {
    let index = type_id.checked_sub(TYPE_BRIBE)? as usize;
    CORE_ABILITIES.get(index)
}

// -----------------------------------------------------------------------------
// Mana and stealth predicates
// -----------------------------------------------------------------------------

/// Inputs to `UnitData::mana` (`0x00609A50`) after its virtual/type queries have been
/// resolved by the object/leader layers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ManaCapacityInput {
    pub base_mana: i32,
    pub is_air: bool,
    pub has_space_program: bool,
    pub space_air_percent: i32,
    pub is_supply: bool,
    pub supply_upgrade: i32,
    pub has_special_craft_bonus: bool,
    pub is_general: bool,
    pub special_craft_percent: i32,
}

/// Pure arithmetic tail of `UnitData::mana` (`0x00609A50`).
///
/// The branch order matters: air units only see the Space Program percentage; ground
/// supply units first multiply by `supply_upgrade + 1`, and the special-craft percentage
/// applies only to generals.
pub fn mana_capacity(input: ManaCapacityInput) -> i32 {
    let mut mana = input.base_mana;
    if mana == 0 {
        return 0;
    }
    if input.is_air {
        if input.has_space_program {
            mana = input.space_air_percent.wrapping_add(100).wrapping_mul(mana) / 100;
        }
        return mana;
    }
    if input.is_supply {
        mana = mana.wrapping_mul(input.supply_upgrade.wrapping_add(1));
    }
    if input.has_special_craft_bonus && input.is_general {
        mana = input
            .special_craft_percent
            .wrapping_add(100)
            .wrapping_mul(mana)
            / 100;
    }
    mana
}

/// `UnitData::mana_left` (`0x00609A30`): capacity minus the signed `UnitData+0x96`
/// expenditure counter, clamped only at zero.
#[inline]
pub fn mana_left(capacity: i32, spent: i16) -> i32 {
    capacity.wrapping_sub(spent as i32).max(0)
}

/// The `short` update performed by `SpellType::pay_cast_costs` (`0x00676C40`) after a
/// successful cost transaction on a unit caster.
#[inline]
pub fn spend_mana(spent: i16, cost: i32) -> i16 {
    spent.wrapping_add(cost as i16)
}

pub const CLOAK_OBJECT_FLAG: u32 = 0x0000_0800;
pub const CLOAK_TYPE_FLAG: u32 = 0x0000_4000;
pub const CLOAK_WHILE_IDLE_TYPE_FLAG: u32 = 0x0004_0000;
pub const CLOAK_SECONDARY_OBJECT_FLAG: u32 = 0x0000_8000;
/// Uppercase `Z` in the shipped `OBJ_MASK` alphabet (`1 << ('Z' - 'A')`).
pub const OBJECT_MASK_DETECT: u32 = 0x0200_0000;

#[inline]
pub const fn has_detector_mask(object_masks: u32) -> bool {
    object_masks & OBJECT_MASK_DETECT != 0
}

/// `UnitData::is_cloaked` (`0x0060A6A0`).
///
/// `has_order` is the resolved result of `UnitData::get_order() != nullptr`.  The
/// conditional type bit means "cloaked while idle", not unconditional stealth.
#[inline]
pub fn is_cloaked(
    object_flags: u32,
    type_flags: u32,
    secondary_object_flags: u32,
    has_order: bool,
) -> bool {
    object_flags & CLOAK_OBJECT_FLAG != 0
        || type_flags & CLOAK_TYPE_FLAG != 0
        || secondary_object_flags & CLOAK_SECONDARY_OBJECT_FLAG != 0
        || (type_flags & CLOAK_WHILE_IDLE_TYPE_FLAG != 0 && !has_order)
}

/// `UnitData::is_detected` (`0x0060A630`) after the object's fog-cell lookup.
///
/// An owner always detects its own unit.  Other players require their bit in the fog
/// cell's dedicated `detected` byte intersecting `LeaderData::ally_mask` (`+0x6929`) for
/// the viewer.  The mask means allied detector coverage is shared.  There is intentionally
/// no `see_all` input because retail does not consult it in this function.
#[inline]
pub fn is_detected(owner: u8, viewer: u8, viewer_ally_mask: u8, cell_detected_mask: u8) -> bool {
    viewer == owner || viewer_ally_mask & cell_detected_mask != 0
}

// -----------------------------------------------------------------------------
// CasterData::active_spells
// -----------------------------------------------------------------------------

pub const AMBUSH_OBJECT_FLAGS: u32 = 0x0000_2800;
pub const FORCED_MARCH_OBJECT_FLAG: u32 = 0x0000_8000;

/// `ActiveSpell`, PDB size 12.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ActiveSpell {
    pub type_id: i32,
    pub start_frame: i32,
    pub end_frame: i32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CasterProcessResult {
    /// Removed in retail's reverse scan order.
    pub removed: Vec<ActiveSpell>,
    /// Presentation-only Jam Radar pulses due this frame.
    pub jam_radar_pulses: u32,
    /// Retail calls `Leader::verify_spell_flags` once if Ambush or Forced March ended.
    pub verify_spell_flags: bool,
    /// Retail also sets its global visibility/cache dirty word in the same case.
    pub invalidate_visibility: bool,
}

/// Logical body of `Caster::process_spells` (`0x00739AD0`).
///
/// The retail `Array<ActiveSpell>::remove` searches for the first equal triplet rather
/// than removing by index.  This implementation preserves that oddity, including when
/// duplicate triplets exist.  The caller remains responsible for the engine array's
/// checksummed capacity/growth metadata; this function does not pretend a Rust `Vec`
/// represents the whole walked container.
pub fn process_active_spells(
    spells: &mut Vec<ActiveSpell>,
    current_frame: i32,
    force: bool,
    object_flags: &mut u32,
) -> CasterProcessResult {
    let mut out = CasterProcessResult::default();
    let mut index = spells.len();
    while index != 0 {
        index -= 1;
        let spell = spells[index];
        if !force && spell.end_frame >= current_frame {
            if spell.type_id == TYPE_JAM_RADAR
                && (spell
                    .start_frame
                    .wrapping_sub(current_frame)
                    .wrapping_add(1) as u32
                    & 0x1f)
                    == 0
            {
                out.jam_radar_pulses += 1;
            }
            continue;
        }

        match spell.type_id {
            TYPE_AMBUSH => {
                *object_flags &= !AMBUSH_OBJECT_FLAGS;
                out.verify_spell_flags = true;
            }
            TYPE_FORCED_MARCH => {
                *object_flags &= !FORCED_MARCH_OBJECT_FLAG;
                out.verify_spell_flags = true;
            }
            _ => {}
        }

        // Array<ActiveSpell>::remove(ActiveSpell), 0x0048A960: first equal value.
        if let Some(first_equal) = spells.iter().position(|candidate| *candidate == spell) {
            spells.remove(first_equal);
            out.removed.push(spell);
        }
    }
    out.invalidate_visibility = out.verify_spell_flags;
    out
}

// -----------------------------------------------------------------------------
// Gaia type block and herds
// -----------------------------------------------------------------------------

pub const BASE_GAIA_TYPES: i32 = 402;
pub const NUM_GAIA_TYPES: usize = 12;

#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GaiaType {
    Bird = 402,
    FlockBird = 403,
    GullBird = 404,
    FarmPig = 405,
    FarmChicken = 406,
    HerdHorse = 407,
    HerdSheep = 408,
    HerdBison = 409,
    HerdBear = 410,
    HerdFish = 411,
    HerdWhale = 412,
    HerdPeacock = 413,
}

pub const GAIA_TYPES: [GaiaType; NUM_GAIA_TYPES] = [
    GaiaType::Bird,
    GaiaType::FlockBird,
    GaiaType::GullBird,
    GaiaType::FarmPig,
    GaiaType::FarmChicken,
    GaiaType::HerdHorse,
    GaiaType::HerdSheep,
    GaiaType::HerdBison,
    GaiaType::HerdBear,
    GaiaType::HerdFish,
    GaiaType::HerdWhale,
    GaiaType::HerdPeacock,
];

pub const HERD_BLOCKED_CELL_FLAGS: u16 = 0x0070;
pub const PLAYABLE_OWNER_SLOTS: u8 = 8;
pub const HERD_ACTIVE_FLAG: u8 = 0x01;
/// Owner passed to `Objects::init_unit` by `Herd::create_units`.
pub const HERD_OBJECT_OWNER: u8 = 8;
/// Owner passed to `Objects::init_unit` by `Farms::add_animals`.
pub const FARM_ANIMAL_OBJECT_OWNER: u8 = 9;

/// `HerdData`, PDB size 28.  The final `herd_flags` byte is represented explicitly; the
/// compiler's tail padding is not a claim about the engine's serializer.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HerdData {
    pub cx: i32,
    pub cy: i32,
    pub wx: i32,
    pub wy: i32,
    pub type_id: i32,
    pub good_object: i32,
    pub herd_id: i16,
    pub herd_flags: u8,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HerdCell {
    pub flags: u16,
    pub owner: u8,
}

/// Which herd array slot `Herds::process` (`0x00741CC0`) selects this frame, before its
/// `herd_flags & 1` gate.
///
/// Retail does work only every 64 frames, divides `frame / 64` by
/// `max(herd_count, 5)`, and uses the remainder.  The five-slot floor means a world with
/// fewer than five herds deliberately has empty scheduler turns.
pub fn scheduled_herd_index(frame: u32, herd_count: usize) -> Option<usize> {
    if frame & 0x3f != 0 {
        return None;
    }
    let denominator = herd_count.max(5);
    let index = (frame as usize / 64) % denominator;
    (index < herd_count).then_some(index)
}

/// Complete scheduler gate from `Herds::process`: cadence, five-slot floor, bounds, and
/// the selected herd's active bit.
pub fn scheduled_active_herd(frame: u32, herds: &[HerdData]) -> Option<usize> {
    scheduled_herd_index(frame, herds.len())
        .filter(|&index| herds[index].herd_flags & HERD_ACTIVE_FLAG != 0)
}

/// `Herd::process` (`0x00741760`).
///
/// `draw` must implement retail `Random::get`; the closure shape keeps this isolated
/// module independent while allowing callers to pass `|lo, hi| game_random.get(lo, hi)`.
/// Exactly **two** draws occur before any bounds/cell test, including on rejection.
pub fn process_herd(
    herd: &mut HerdData,
    world_xs: i32,
    world_ys: i32,
    mut draw: impl FnMut(i32, i32) -> i32,
    mut cell_at: impl FnMut(i32, i32) -> HerdCell,
) -> bool {
    let candidate_x = herd.cx.wrapping_sub(1).wrapping_add(draw(0, 0xffff) % 3);
    let candidate_y = herd.cy.wrapping_sub(1).wrapping_add(draw(0, 0xffff) % 3);
    if candidate_x < 0 || candidate_y < 0 || candidate_x >= world_xs || candidate_y >= world_ys {
        return false;
    }
    let cell = cell_at(candidate_x, candidate_y);
    if cell.flags & HERD_BLOCKED_CELL_FLAGS != 0 || cell.owner >= PLAYABLE_OWNER_SLOTS {
        return false;
    }
    herd.wx = candidate_x;
    herd.wy = candidate_y;
    true
}

/// Unit count selected at the head of `Herd::create_units` (`0x007417F0`).
#[inline]
pub const fn herd_spawn_count(type_id: i32) -> usize {
    match type_id {
        x if x == GaiaType::HerdWhale as i32 => 1,
        x if x == GaiaType::HerdFish as i32 => 3,
        _ => 4,
    }
}

/// Input to retail's final `Unit::set_angle` call in `Herd::create_units`.
///
/// Fish consume no draw and pass zero; whales consume no draw and pass `0x80000000`.
/// Other herd types consume one draw, reduce it modulo 360, and pass the degrees through
/// `degrees_to_angle(degrees, 1)`.  The conversion itself belongs to the shared angle
/// layer and is therefore kept explicit rather than guessed here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HerdAngleSeed {
    Raw(u32),
    Degrees(i32),
}

pub fn herd_angle_seed(type_id: i32, mut draw: impl FnMut(i32, i32) -> i32) -> HerdAngleSeed {
    match type_id {
        x if x == GaiaType::HerdFish as i32 => HerdAngleSeed::Raw(0),
        x if x == GaiaType::HerdWhale as i32 => HerdAngleSeed::Raw(0x8000_0000),
        _ => HerdAngleSeed::Degrees(draw(0, 0xffff) % 360),
    }
}

// -----------------------------------------------------------------------------
// Farm animals
// -----------------------------------------------------------------------------

pub const FARM_ANIMALS_PER_FARM: usize = 5;
pub const FARM_ANIMAL_JITTER_SPAN: i32 = 0x180; // 384 Coord
pub const FARM_ANIMAL_JITTER_BIAS: i32 = 0x0c0; // 192 Coord

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FarmAnimalSpawn {
    pub type_id: i32,
    pub object_owner: u8,
    pub x: i32,
    pub y: i32,
    /// `AnimalData+0x150`: farm object index.
    pub farm_object: i16,
    /// `AnimalData+0x152`: farm owner slot.
    pub farm_owner: i16,
    /// `AnimalData+0x154`: 0..4 within this farm's spawn batch.
    pub animal_id: i8,
}

/// The three gates visible across the two `Farms::add_animals` overloads
/// (`0x008D92D0`, `0x008D8F30`).
#[inline]
pub const fn farm_is_spawn_eligible(
    farm_outer_flag_nonzero: bool,
    farm_state_is_one: bool,
    build_is_complete: bool,
) -> bool {
    farm_outer_flag_nonzero && farm_state_is_one && build_is_complete
}

/// Build the exact five records spawned by `Farms::add_animals(int)` (`0x008D8F30`) for
/// one already-eligible farm.
///
/// Draw order per animal is load-bearing:
///
/// 1. low bit selects chicken (even) or pig (odd),
/// 2. Y jitter,
/// 3. X jitter.
///
/// Consequently an eligible farm consumes exactly 15 main-simulation RNG draws.
pub fn plan_farm_animals(
    farm_x: i32,
    farm_y: i32,
    farm_owner: i16,
    farm_object: i16,
    mut draw: impl FnMut(i32, i32) -> i32,
) -> [FarmAnimalSpawn; FARM_ANIMALS_PER_FARM] {
    std::array::from_fn(|animal_id| {
        let selector = draw(0, 0xffff);
        let y_jitter = draw(0, 0xffff) % FARM_ANIMAL_JITTER_SPAN - FARM_ANIMAL_JITTER_BIAS;
        let x_jitter = draw(0, 0xffff) % FARM_ANIMAL_JITTER_SPAN - FARM_ANIMAL_JITTER_BIAS;
        FarmAnimalSpawn {
            type_id: if selector & 1 == 0 {
                GaiaType::FarmChicken as i32
            } else {
                GaiaType::FarmPig as i32
            },
            object_owner: FARM_ANIMAL_OBJECT_OWNER,
            x: farm_x.wrapping_add(x_jitter),
            y: farm_y.wrapping_add(y_jitter),
            farm_object,
            farm_owner,
            animal_id: animal_id as i8,
        }
    })
}

/// The exact 128-frame gate at the head of `Animal::think_farm_animal`
/// (`0x005D7700`).  No RNG is consumed when this returns false.
#[inline]
pub fn farm_animal_wander_due(frame: i32, animal_object: i16, animal_id: i8) -> bool {
    frame.wrapping_add((animal_object as i32).wrapping_mul(animal_id as i32 + 1)) & 0x7f == 0
}

#[cfg(test)]
mod tests {
    use super::recovered_test_rng::Random;
    use super::*;

    #[derive(Debug)]
    struct ScriptedRng {
        values: Vec<i32>,
        index: usize,
        calls: Vec<(i32, i32)>,
    }

    impl ScriptedRng {
        fn new(values: &[i32]) -> Self {
            Self {
                values: values.to_vec(),
                index: 0,
                calls: Vec::new(),
            }
        }

        fn get(&mut self, lo: i32, hi: i32) -> i32 {
            self.calls.push((lo, hi));
            let value = self.values[self.index];
            self.index += 1;
            assert!((lo..hi).contains(&value));
            value
        }
    }

    #[test]
    fn ability_block_is_dense_and_freezes_disabled_rows() {
        for (index, ability) in CORE_ABILITIES.iter().enumerate() {
            assert_eq!(ability.type_id, TYPE_BRIBE + index as i32);
            assert_eq!(ability_fact(ability.type_id), Some(ability));
        }
        let disabled: Vec<i32> = CORE_ABILITIES
            .iter()
            .filter(|ability| !ability.enabled)
            .map(|ability| ability.type_id)
            .collect();
        assert_eq!(disabled, [630, 632, 633, 638, 642, 644]);
        assert_eq!(ability_fact(628), None);
        assert_eq!(ability_fact(645), None);
    }

    #[test]
    fn recovered_pod_layouts_match_the_pdb_sizes() {
        assert_eq!(std::mem::size_of::<ActiveSpell>(), 12);
        assert_eq!(std::mem::size_of::<HerdData>(), 28);
    }

    #[test]
    fn costs_keep_research_and_cast_columns_distinct() {
        let ambush = ability_fact(TYPE_AMBUSH).unwrap();
        assert_eq!(ambush.research_cost, "20g/20w");
        assert_eq!(ambush.cast_cost, "");
        let restock = ability_fact(TYPE_RESTOCK_SUPPLIES).unwrap();
        assert_eq!(restock.research_cost, "");
        assert_eq!(restock.cast_cost, "2w/2m/2c/2o/2f");
    }

    #[test]
    fn mana_capacity_preserves_retail_branch_order() {
        let common = ManaCapacityInput {
            base_mana: 1000,
            is_air: false,
            has_space_program: false,
            space_air_percent: 90,
            is_supply: true,
            supply_upgrade: 2,
            has_special_craft_bonus: true,
            is_general: true,
            special_craft_percent: 20,
        };
        assert_eq!(mana_capacity(common), 3600);
        assert_eq!(
            mana_capacity(ManaCapacityInput {
                is_air: true,
                has_space_program: true,
                ..common
            }),
            1900
        );
        assert_eq!(mana_left(1000, 125), 875);
        assert_eq!(mana_left(1000, 1200), 0);
        assert_eq!(spend_mana(32_760, 10), -32_766);
    }

    #[test]
    fn idle_cloak_and_detection_are_separate_planes() {
        assert!(is_cloaked(0, CLOAK_WHILE_IDLE_TYPE_FLAG, 0, false));
        assert!(!is_cloaked(0, CLOAK_WHILE_IDLE_TYPE_FLAG, 0, true));
        assert!(is_cloaked(CLOAK_OBJECT_FLAG, 0, 0, true));
        assert!(has_detector_mask(OBJECT_MASK_DETECT));
        assert!(!has_detector_mask(OBJECT_MASK_DETECT >> 1));
        assert!(is_detected(2, 2, 0, 0));
        assert!(!is_detected(2, 3, 1 << 3, 0));
        assert!(is_detected(2, 3, 1 << 3, 1 << 3));
        assert!(
            is_detected(2, 3, 1 << 4, 1 << 4),
            "allied detection is shared"
        );
    }

    #[test]
    fn spell_end_frame_is_inclusive() {
        let mut spells = vec![ActiveSpell {
            type_id: TYPE_AMBUSH,
            start_frame: 10,
            end_frame: 20,
        }];
        let mut flags = AMBUSH_OBJECT_FLAGS;
        let at_end = process_active_spells(&mut spells, 20, false, &mut flags);
        assert!(at_end.removed.is_empty());
        assert_eq!(flags, AMBUSH_OBJECT_FLAGS);

        let after_end = process_active_spells(&mut spells, 21, false, &mut flags);
        assert_eq!(after_end.removed.len(), 1);
        assert_eq!(flags & AMBUSH_OBJECT_FLAGS, 0);
        assert!(after_end.verify_spell_flags);
        assert!(after_end.invalidate_visibility);
    }

    #[test]
    fn force_removes_every_spell_and_clears_both_effect_bits() {
        let mut spells = vec![
            ActiveSpell {
                type_id: TYPE_AMBUSH,
                start_frame: 1,
                end_frame: 999,
            },
            ActiveSpell {
                type_id: TYPE_FORCED_MARCH,
                start_frame: 1,
                end_frame: 999,
            },
            ActiveSpell {
                type_id: TYPE_JAM_RADAR,
                start_frame: 1,
                end_frame: 999,
            },
        ];
        let mut flags = AMBUSH_OBJECT_FLAGS | FORCED_MARCH_OBJECT_FLAG | 0x10;
        let out = process_active_spells(&mut spells, 2, true, &mut flags);
        assert!(spells.is_empty());
        assert_eq!(
            out.removed.iter().map(|s| s.type_id).collect::<Vec<_>>(),
            [643, 636, 635]
        );
        assert_eq!(flags, 0x10);
        assert_eq!(out.jam_radar_pulses, 0);
    }

    #[test]
    fn duplicate_spell_removal_uses_first_equal_value() {
        let duplicate = ActiveSpell {
            type_id: TYPE_SNIPER,
            start_frame: 1,
            end_frame: 1,
        };
        let mut spells = vec![
            duplicate,
            ActiveSpell {
                type_id: TYPE_BRIBE,
                start_frame: 2,
                end_frame: 50,
            },
            duplicate,
        ];
        let mut flags = 0;
        let out = process_active_spells(&mut spells, 2, false, &mut flags);
        assert_eq!(out.removed, [duplicate, duplicate]);
        assert_eq!(
            spells,
            [ActiveSpell {
                type_id: TYPE_BRIBE,
                start_frame: 2,
                end_frame: 50
            }]
        );
    }

    #[test]
    fn jam_radar_pulse_is_every_32_frames_relative_to_start() {
        let spell = ActiveSpell {
            type_id: TYPE_JAM_RADAR,
            start_frame: 100,
            end_frame: 200,
        };
        let mut flags = 0;
        let mut spells = vec![spell];
        assert_eq!(
            process_active_spells(&mut spells, 101, false, &mut flags).jam_radar_pulses,
            1
        );
        assert_eq!(
            process_active_spells(&mut spells, 102, false, &mut flags).jam_radar_pulses,
            0
        );
        assert_eq!(
            process_active_spells(&mut spells, 133, false, &mut flags).jam_radar_pulses,
            1
        );
    }

    #[test]
    fn gaia_type_block_is_exactly_twelve_dense_ids() {
        assert_eq!(GAIA_TYPES.len(), NUM_GAIA_TYPES);
        for (index, kind) in GAIA_TYPES.iter().enumerate() {
            assert_eq!(*kind as i32, BASE_GAIA_TYPES + index as i32);
        }
    }

    #[test]
    fn herd_scheduler_has_five_slot_floor() {
        assert_eq!(scheduled_herd_index(0, 2), Some(0));
        assert_eq!(scheduled_herd_index(64, 2), Some(1));
        assert_eq!(scheduled_herd_index(128, 2), None);
        assert_eq!(scheduled_herd_index(320, 2), Some(0));
        assert_eq!(scheduled_herd_index(1, 20), None);
        assert_eq!(scheduled_herd_index(64 * 19, 20), Some(19));

        let mut herds = vec![HerdData::default(); 2];
        herds[0].herd_flags = HERD_ACTIVE_FLAG;
        assert_eq!(scheduled_active_herd(0, &herds), Some(0));
        assert_eq!(scheduled_active_herd(64, &herds), None);
        herds[1].herd_flags = HERD_ACTIVE_FLAG;
        assert_eq!(scheduled_active_herd(64, &herds), Some(1));
    }

    #[test]
    fn herd_process_draws_twice_before_rejecting_bounds() {
        let mut herd = HerdData {
            cx: 0,
            cy: 0,
            wx: 7,
            wy: 8,
            ..HerdData::default()
        };
        let mut rng = ScriptedRng::new(&[0, 2]); // candidate (-1, 1)
        let accepted = process_herd(
            &mut herd,
            10,
            10,
            |lo, hi| rng.get(lo, hi),
            |_, _| panic!("out-of-bounds cell must not be read"),
        );
        assert!(!accepted);
        assert_eq!(rng.calls, [(0, 0xffff), (0, 0xffff)]);
        assert_eq!((herd.wx, herd.wy), (7, 8));
    }

    #[test]
    fn herd_process_accepts_only_open_playable_cells() {
        let base = HerdData {
            cx: 4,
            cy: 4,
            wx: 0,
            wy: 0,
            ..HerdData::default()
        };
        let mut open = base;
        let mut rng = ScriptedRng::new(&[2, 0]); // candidate (5, 3)
        assert!(process_herd(
            &mut open,
            10,
            10,
            |lo, hi| rng.get(lo, hi),
            |x, y| {
                assert_eq!((x, y), (5, 3));
                HerdCell { flags: 0, owner: 7 }
            }
        ));
        assert_eq!((open.wx, open.wy), (5, 3));

        let mut blocked = base;
        let mut rng = ScriptedRng::new(&[2, 0]);
        assert!(!process_herd(
            &mut blocked,
            10,
            10,
            |lo, hi| rng.get(lo, hi),
            |_, _| HerdCell {
                flags: 0x10,
                owner: 0
            }
        ));
        assert_eq!((blocked.wx, blocked.wy), (0, 0));
    }

    #[test]
    fn herd_creation_count_and_angle_draws_match_type_branches() {
        assert_eq!(herd_spawn_count(GaiaType::HerdWhale as i32), 1);
        assert_eq!(herd_spawn_count(GaiaType::HerdFish as i32), 3);
        assert_eq!(herd_spawn_count(GaiaType::HerdBison as i32), 4);

        let mut fish_calls = 0;
        assert_eq!(
            herd_angle_seed(GaiaType::HerdFish as i32, |_, _| {
                fish_calls += 1;
                0
            }),
            HerdAngleSeed::Raw(0)
        );
        assert_eq!(fish_calls, 0);
        let mut whale_calls = 0;
        assert_eq!(
            herd_angle_seed(GaiaType::HerdWhale as i32, |_, _| {
                whale_calls += 1;
                0
            }),
            HerdAngleSeed::Raw(0x8000_0000)
        );
        assert_eq!(whale_calls, 0);
        let mut rng = ScriptedRng::new(&[721]);
        assert_eq!(
            herd_angle_seed(GaiaType::HerdBison as i32, |lo, hi| rng.get(lo, hi)),
            HerdAngleSeed::Degrees(1)
        );
        assert_eq!(rng.calls, [(0, 0xffff)]);
    }

    #[test]
    fn eligible_farm_plan_consumes_fifteen_draws_in_selector_y_x_order() {
        let values: Vec<i32> = (0..15).collect();
        let mut rng = ScriptedRng::new(&values);
        let spawns = plan_farm_animals(1000, 2000, 3, 77, |lo, hi| rng.get(lo, hi));
        assert_eq!(rng.calls, vec![(0, 0xffff); 15]);
        assert_eq!(
            spawns[0],
            FarmAnimalSpawn {
                type_id: GaiaType::FarmChicken as i32,
                object_owner: FARM_ANIMAL_OBJECT_OWNER,
                x: 810,
                y: 1809,
                farm_object: 77,
                farm_owner: 3,
                animal_id: 0
            }
        );
        assert_eq!(
            spawns[1],
            FarmAnimalSpawn {
                type_id: GaiaType::FarmPig as i32,
                object_owner: FARM_ANIMAL_OBJECT_OWNER,
                x: 813,
                y: 1812,
                farm_object: 77,
                farm_owner: 3,
                animal_id: 1
            }
        );
        assert_eq!(spawns[4].animal_id, 4);
    }

    #[test]
    fn farm_and_herd_paths_advance_the_real_sim_rng_exactly() {
        let seed = 0x1234_5678;
        let mut farm_rng = Random::new(seed);
        let mut farm_reference = Random::new(seed);
        let _ = plan_farm_animals(0, 0, 0, 0, |lo, hi| farm_rng.get(lo, hi));
        for _ in 0..15 {
            farm_reference.advance();
        }
        assert_eq!(farm_rng.state(), farm_reference.state());

        let mut herd_rng = Random::new(seed);
        let mut herd_reference = Random::new(seed);
        let mut herd = HerdData {
            cx: 0,
            cy: 0,
            ..HerdData::default()
        };
        let _ = process_herd(
            &mut herd,
            0,
            0,
            |lo, hi| herd_rng.get(lo, hi),
            |_, _| unreachable!(),
        );
        herd_reference.advance();
        herd_reference.advance();
        assert_eq!(herd_rng.state(), herd_reference.state());
    }

    #[test]
    fn farm_gates_and_wander_cadence_are_pure() {
        assert!(farm_is_spawn_eligible(true, true, true));
        assert!(!farm_is_spawn_eligible(false, true, true));
        assert!(!farm_is_spawn_eligible(true, false, true));
        assert!(!farm_is_spawn_eligible(true, true, false));
        assert!(farm_animal_wander_due(122, 3, 1)); // 122 + 3*(1+1) == 128
        assert!(!farm_animal_wander_due(121, 3, 1));
    }
}
