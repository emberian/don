//! Source-frozen scalar prefix of the shipped `Build::init` call chain.
//!
//! A starting City needs more than the canonical Build registry identity: retail first
//! constructs `BuildData`, writes the Build-specific prelude, and enters
//! `Wall::init` -> `Object::init` -> `SubObject::init`.  The base initializer binds the
//! current type, installs the terrain-derived Z coordinate, and resets the inherited
//! Object body before `Object::add_to_world` mutates the terrain cell/object lists.
//!
//! [`apply_build_init_prefix`] reproduces exactly the scalar writes observed before that
//! first world-mutating barrier.  It deliberately preserves fields which have not yet
//! received a retail write at the boundary (notably `construct_hits`, `ever_seen`,
//! `stance`, and the infiltration bytes).  The returned receipt therefore proves neither
//! complete `Build::init` nor `Build::activate`.

use std::fmt;

use don_sim::objects::BANDED_SLOTS;
use don_sim::systems::production::{self, BuildData, BuildQueue, MiningList};

pub const BUILD_DATA_CTOR_VA: u32 = 0x0062_f370;
pub const MINING_LIST_CTOR_VA: u32 = 0x0047_2260;
pub const BUILD_INIT_VA: u32 = 0x0062_9740;
pub const WALL_INIT_VA: u32 = 0x0063_e9b0;
pub const OBJECT_INIT_VA: u32 = 0x0064_7750;
pub const OBJECT_ADD_TO_WORLD_VA: u32 = 0x0064_d8c0;
pub const SUBOBJECT_INIT_VA: u32 = 0x0066_2300;

/// XOR applied to all three stored coordinates by `SubObject::init`.
pub const SUBOBJECT_COORD_XOR: i32 = 0x0006_3637;
/// XOR applied by the outer `Build::init` prelude to the leader max-age byte.
pub const BUILD_MAX_AGE_XOR: u8 = 0x66;
/// `SubObject::init`'s type-derived flag.
pub const SUBOBJECT_FLAT_FLAG: u8 = 0x20;
/// `Object::init`'s type-derived detector flag.
pub const OBJECT_DETECTOR_FLAG: u8 = 0x40;
/// Capacity installed by `BuildData::BuildData` after its embedded `MiningList` returns.
pub const BUILD_MINING_INITIAL_CAPACITY: i32 = 5;
/// Doubling growth hint installed by `MiningList::MiningList`.
pub const BUILD_MINING_INCREMENT: i16 = -1;

const Z_INTERNAL: usize = 0x0c;
const PTYPE_POINTER: usize = 0x18;
const ON_SCREEN: usize = 0x1c;
const INSIDE_DOWN: usize = 0x28;
const UP: usize = 0x2a;
const DOWN: usize = 0x2c;
const DOWN_WHO: usize = 0x2e;
const HOLD_FRAMES: usize = 0x32;
const NEAR_O: usize = 0x34;
const NEAR_WHO: usize = 0x36;
const HEALING: usize = 0x38;
const INFILTRATED: usize = 0x3a;
const MYLOS: usize = 0x3c;
const TARGETED: usize = 0x3d;
const INSIDE_DOWN_WHO: usize = 0x3e;
const UP_WHO: usize = 0x3f;
const VISIBLE: usize = 0x40;
const LAUNCH_FRAMES: usize = 0x41;
const LAUNCHING_POINTER: usize = 0x44;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BuildTypeInitFacts {
    /// Result of the Type virtual called by `SubObject::init` at vtable slot `+0x64`.
    pub sets_flat_flag: bool,
    /// Result of `Object::init`'s `has_objmask(0x02000000)` query.
    pub sets_detector_flag: bool,
}

/// Inputs read before the first world-owning child call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildInitPrefixRequest {
    pub owner: u8,
    pub object_id: i16,
    pub type_index: i32,
    /// Length of the current ObjectType table used to prove `type_index` is dereferenceable.
    pub type_rows: usize,
    pub snapped_x: i32,
    pub snapped_y: i32,
    /// Return written by `TerrainOut::find_tcoord_z` for the snapped X/Y coordinate.
    pub terrain_z: i32,
    /// Current value of the owner's `ObjectData::uid` counter before retail increments it.
    pub owner_uid_before: u16,
    /// Raw leader byte at the `Build::init` max-age source offset (`leader + 0xdc`).
    pub max_age_source_byte: u8,
    pub type_facts: BuildTypeInitFacts,
}

/// Precise chronological boundary reached by this module.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildLifecycleStage {
    /// `Object::init` has completed its scalar writes and is about to call
    /// `Object::add_to_world`; `Wall::init`, `Build::init`, and activation have not returned.
    BeforeObjectAddToWorld,
}

/// Facts needed by the later Builds walk but not represented by stable pointer bytes in
/// `BuildData`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildInitPointerFacts {
    /// `ObjectData::launching` is null after `Object::init` destroys any prior array.
    pub launching_is_null: bool,
    /// Complete constructor header and tail for `BuildData::gather_from`.
    pub mining_length: i32,
    pub mining_capacity: i32,
    pub mining_increment: i16,
    pub mining_flags: u8,
    pub mining_mtn: i8,
    pub mining_cliff: i8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildInitPrefixReceipt {
    pub stage: BuildLifecycleStage,
    pub owner: u8,
    pub object_id: i16,
    /// Address-independent owner for the ptype binding performed by `SubObject::init`.
    pub current_type: i32,
    pub snapped_position: (i32, i32),
    pub terrain_z: i32,
    pub encoded_position: (i32, i32),
    pub encoded_terrain_z: i32,
    pub flags: u8,
    pub uid: u16,
    pub max_age: u8,
    pub pointer_facts: BuildInitPointerFacts,
}

impl BuildInitPrefixReceipt {
    /// This tranche stops inside `Object::init`, so it can never prove full initialization.
    #[inline]
    pub const fn init_complete(self) -> bool {
        false
    }

    /// Activation is a later call and is never performed by this tranche.
    #[inline]
    pub const fn activation_complete(self) -> bool {
        false
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildInitPrefixError {
    OwnerOutOfRange { owner: u8 },
    TypeIndexOutOfRange { type_index: i32, type_rows: usize },
}

impl fmt::Display for BuildInitPrefixError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OwnerOutOfRange { owner } => write!(
                f,
                "Build owner {owner} is outside retail's playable 0..{BANDED_SLOTS} range"
            ),
            Self::TypeIndexOutOfRange {
                type_index,
                type_rows,
            } => write!(
                f,
                "Build ptype {type_index} is outside the installed {type_rows}-row type table"
            ),
        }
    }
}

impl std::error::Error for BuildInitPrefixError {}

/// Apply the constructor and initializer scalar writes through the instruction immediately
/// before `Object::add_to_world` (`0x0064d8c0`).
///
/// Both refusal paths are preflighted and leave `build` untouched.  On success, fields not
/// yet written at this chronological boundary retain their incoming value.  In particular,
/// callers must not treat this as a complete or checksum-ready Build initializer.
pub fn apply_build_init_prefix(
    build: &mut BuildData,
    request: BuildInitPrefixRequest,
) -> Result<BuildInitPrefixReceipt, BuildInitPrefixError> {
    if request.owner as usize >= BANDED_SLOTS {
        return Err(BuildInitPrefixError::OwnerOutOfRange {
            owner: request.owner,
        });
    }
    let type_row = usize::try_from(request.type_index).ok();
    if type_row.is_none_or(|row| row >= request.type_rows) {
        return Err(BuildInitPrefixError::TypeIndexOutOfRange {
            type_index: request.type_index,
            type_rows: request.type_rows,
        });
    }

    // Object::Object and Wall::Wall constructor-owned typed fields.  Chronologically
    // unwritten fields are intentionally absent from this list.
    build.myhits = 0;
    build.damage = 0;
    build.damage_frac = 0;
    build.job_counter = 0;
    build.job_counter_2 = 0;
    build.constr_time = 0;
    build.gpiece = -1;
    build.frame_started = -1;
    build.build_masks = 0;
    build.helpers = 0;
    build.demolition = 0;

    // BuildData::BuildData constructor-owned fields and empty container shapes.
    build.orig_type = -1;
    build.gather_down = -1;
    build.city = -1;
    build.city_down = -1;
    build.wonder = -1;
    build.dock = -1;
    build.recharging = 0;
    build.attack_ox = -1;
    build.founder = -1;
    build.gather_max = 0;
    build.attack_whom = -1;
    build.max_age = 0;
    build.queue = BuildQueue {
        queued: 0,
        entries: Vec::new(),
    };
    build.gather_from = MiningList {
        tiles: Vec::with_capacity(BUILD_MINING_INITIAL_CAPACITY as usize),
        mtn: -1,
        cliff: -1,
    };
    build.gather = Vec::new();

    // Build::init's outer prelude, before it delegates to Wall::init.
    build.founder = request.owner as i8;
    build.wonder = -1;
    build.max_age = request.max_age_source_byte ^ BUILD_MAX_AGE_XOR;
    build.dock = -1;
    build.orig_type = request.type_index;

    // SubObject::init.  The raw ptype address is process-local; zero it to avoid retaining
    // a stale pointer and return its stable TypeIndex owner in the receipt instead.
    build.who = request.owner;
    build.flags = production::flag::VALID;
    if request.type_facts.sets_flat_flag {
        build.flags |= SUBOBJECT_FLAT_FLAG;
    }
    write_i16(
        &mut build.other,
        production::off::OBJECT_ID,
        request.object_id,
    );
    write_i32(
        &mut build.other,
        Z_INTERNAL,
        request.terrain_z ^ SUBOBJECT_COORD_XOR,
    );
    write_i32(
        &mut build.other,
        production::off::X_INTERNAL,
        request.snapped_x ^ SUBOBJECT_COORD_XOR,
    );
    write_i32(
        &mut build.other,
        production::off::Y_INTERNAL,
        request.snapped_y ^ SUBOBJECT_COORD_XOR,
    );
    write_i32(&mut build.other, PTYPE_POINTER, 0);
    build.other[ON_SCREEN] = 0;

    // Object::init's scalar body, ending immediately before Object::add_to_world.
    write_i16(&mut build.other, INSIDE_DOWN, -1);
    write_i16(&mut build.other, UP, -1);
    write_i16(&mut build.other, DOWN, -1);
    write_i16(&mut build.other, DOWN_WHO, -1);
    build.uid = request.owner_uid_before;
    write_u16(&mut build.other, HOLD_FRAMES, 0);
    write_i16(&mut build.other, NEAR_O, -1);
    write_i16(&mut build.other, NEAR_WHO, -1);
    write_i16(&mut build.other, HEALING, 0);
    build.other[INFILTRATED] = 0;
    build.other[MYLOS] = 0;
    build.other[TARGETED] = 0;
    build.other[INSIDE_DOWN_WHO] = request.owner;
    build.other[UP_WHO] = 0xff;
    build.other[VISIBLE] = 0;
    build.other[LAUNCH_FRAMES] = 0;
    write_i32(&mut build.other, LAUNCHING_POINTER, 0);
    if request.type_facts.sets_detector_flag {
        build.flags |= OBJECT_DETECTOR_FLAG;
    }

    Ok(BuildInitPrefixReceipt {
        stage: BuildLifecycleStage::BeforeObjectAddToWorld,
        owner: build.who,
        object_id: build.object_id(),
        current_type: request.type_index,
        snapped_position: build.position(),
        terrain_z: request.terrain_z,
        encoded_position: (
            read_i32(&build.other, production::off::X_INTERNAL),
            read_i32(&build.other, production::off::Y_INTERNAL),
        ),
        encoded_terrain_z: read_i32(&build.other, Z_INTERNAL),
        flags: build.flags,
        uid: build.uid,
        max_age: build.max_age,
        pointer_facts: BuildInitPointerFacts {
            launching_is_null: true,
            mining_length: 0,
            mining_capacity: BUILD_MINING_INITIAL_CAPACITY,
            mining_increment: BUILD_MINING_INCREMENT,
            mining_flags: 0,
            mining_mtn: -1,
            mining_cliff: -1,
        },
    })
}

fn write_i16(image: &mut [u8], offset: usize, value: i16) {
    image[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn write_u16(image: &mut [u8], offset: usize, value: u16) {
    image[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn write_i32(image: &mut [u8], offset: usize, value: i32) {
    image[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn read_i32(image: &[u8], offset: usize) -> i32 {
    i32::from_le_bytes(
        image[offset..offset + 4]
            .try_into()
            .expect("fixed BuildData scalar window"),
    )
}
