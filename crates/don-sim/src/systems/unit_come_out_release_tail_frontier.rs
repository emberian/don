// SPDX-License-Identifier: GPL-3.0-or-later
//! Fourth and final tranche of retail `Unit::come_out(int)` `0x00617C10`.
//!
//! The first three tranches
//! ([`crate::systems::unit_come_out_full_frontier`],
//! [`crate::systems::unit_come_out_common_release_frontier`],
//! [`crate::systems::unit_come_out_gather_selection_frontier`])
//! stop at `0x006191A5`, immediately before retail consumes the selected/fallback point.
//! This planner owns everything after it: the sequential interval
//! `0x006191A5..0x0061A206` and its own ten compiler-outlined virtual-call islands in
//! `0x0061A281..0x0061A2D5` — 4,193 + 84 = **4,277 bytes**. It also *maps* the seven islands
//! in `0x0061A206..0x0061A24E`, which resume inside the first tranche and which no lane had
//! accounted for; 4,277 + 72 = **4,349**, exactly the residual
//! [`crate::systems::unit_come_out_gather_selection_frontier::RESIDUAL_BYTES_AFTER_TRANCHE`]
//! reported. With this tranche the 9,925-byte body is tiled with no gap and no overlap;
//! [`crate::systems::unit_come_out_body_map`] asserts that byte by byte.
//!
//! # What the tail actually is
//!
//! It is the **order-installation dispatcher**. Once the unit has a location, retail decides
//! what the freshly released unit should *do*, in a fixed cascade:
//!
//! ```text
//!   0x006191A5  face the container                     Unit::set_angle
//!   0x006191FC  nothing was selected            -> tail
//!   0x0061922D  a building under the point?     ObjectsData::find_any_building_at
//!   0x0061923E  reposition beside it            find_angle / find_nearby_spot / set_new_location
//!   0x00619386  specialist dispatch             add_build_order / add_repair_order / add_gather_order
//!   0x0061960F  trade                           add_trade_order
//!   0x006196C7  garrison                        add_garrison_order       (+ the o_down chain)
//!   0x00619854  enemy building                  add_attack_order         (+ the o_down chain)
//!   0x00619AA3  a unit under the point?         ObjectsData::find_unit_with_radius
//!   0x00619D74  movement fallback               add_move_facing_order / Group::action_move_to
//!   0x00619FE2  SPECIAL_ANIM exit order         add_spec_anim_order / do_spec_anim
//!   0x0061A0B6  options.rebuild = 1
//!   0x0061A0C5  terminal army gate              Random::get -> Unit::add_to_army
//! ```
//!
//! Three consequences are load-bearing and are asserted by this module's tests.
//!
//! 1. **`Unit::come_out` consumes a canonical `game_random` draw.** [measured] the two
//!    `call 0x00A39D70` sites at `0x0061A1BB` and `0x0061A1D5` load `ECX` from
//!    `[0x00C06184]` = `GameAccess::game_random` — the main simulation stream, not a private
//!    generator. They are mutually exclusive, so the body's whole-function draw budget on
//!    this path is exactly **one**, and it is taken only when
//!    `UnitData::unit_masks & 0x40000` is set *and* the unit is off map. Any host that
//!    budgets zero desyncs.
//! 2. **The army gate is a frame-parity test.** `Game+0x550` is `frame` [PDB
//!    `types.json`], so the terminal predicate is `(game.frame & r) != 0` where `r` is
//!    `draw % 3` or `draw & 1` depending on a type probe. It is not a probability.
//! 3. **Every order the tail installs is replicated across the actor's `o_down` chain.**
//!    `UnitData+0x90` is `o_down` [PDB]. Garrison and both attack routes walk it to
//!    termination (`o_down < 0`), issuing the identical order to each link. A host that
//!    orders only the released unit under-installs.
//!
//! # Fidelity
//!
//! Tier **C**: derived from the `ron-bin/riseofnations.exe` instruction stream (capstone
//! PE32) cross-read with a full Ghidra decompilation of the body and with `rise.pdb` types
//! and signatures. Exercised only against this port. **Not** differentially tested against
//! retail, and nothing here is verified in the proof-assistant sense.
//!
//! The Ghidra decompilation is *lossy* in two places this planner corrects from the
//! instruction stream, and both change behaviour:
//!
//! * at `0x00619C54` Ghidra renders both arms of the unit-attack branch as the same
//!   `add_attack_order(u, who, 1)`. The instruction stream pushes `1,1,1` on the
//!   `action != 0` arm (`0x00619C6D`) and `0,0,1` on the `action == 0` arm (`0x00619CF4`) —
//!   two different orders;
//! * at `0x00619DA2`/`0x00619DBA`/`0x00619DD2` Ghidra drops the arguments of the three
//!   `where`-type probes. They are `is(0x1AB, false)`, `is(0x1AC, false)` and
//!   `is(0x1B0, false)`.

// ---------------------------------------------------------------------------
// Extent
// ---------------------------------------------------------------------------

/// First byte owned by this tranche; the resume point the gather-selection tranche ends on.
pub const RELEASE_TAIL_START_VA: u32 = 0x0061_91a5;
/// One past the last sequential byte; the first compiler-outlined island follows.
pub const OUTLINED_REGION_START_VA: u32 = 0x0061_a206;
/// One past the last byte of `Unit::come_out`.
pub const UNIT_COME_OUT_END_VA: u32 = 0x0061_a2d5;
/// `0x006191A5..0x0061A206`.
pub const SEQUENTIAL_BYTES: u32 = OUTLINED_REGION_START_VA - RELEASE_TAIL_START_VA;
/// This tranche's own outlined islands, `0x0061A281..0x0061A2D5`.
pub const OWN_OUTLINED_BYTES: u32 = UNIT_COME_OUT_END_VA - 0x0061_a281;
/// 4,193 + 84. This tranche's **own** behaviour.
pub const LOGICAL_TRANCHE_BYTES: u32 = SEQUENTIAL_BYTES + OWN_OUTLINED_BYTES;
/// The seven islands `0x0061A206..0x0061A24E`. They sit in this tranche's address
/// neighbourhood but every one of them resumes inside the **first** tranche, so they are its
/// behaviour, not this one's. No lane had counted them: the first tranche published a flat
/// `PREFIX_BYTES = 0x006186B4 - 0x00617C10` with no island accounting at all. This lane
/// identified and mapped them; [`crate::systems::unit_come_out_body_map`] attributes them to
/// [`crate::systems::unit_come_out_body_map::Tranche::Prefix`], which is why the prefix's
/// real total is 2,796 rather than the 2,724 it published.
pub const REATTRIBUTED_PREFIX_ISLAND_BYTES: u32 = 0x0061_a24e - 0x0061_a206;
/// The residual the gather-selection tranche left: 4,277 of this tranche's own plus the 72
/// reattributed island bytes. This tranche closes it to zero.
pub const PRIOR_RESIDUAL_BYTES: u32 = 4_349;
pub const RESIDUAL_BYTES_AFTER_TRANCHE: u32 =
    PRIOR_RESIDUAL_BYTES - LOGICAL_TRANCHE_BYTES - REATTRIBUTED_PREFIX_ISLAND_BYTES;

/// The two mutually exclusive `Random::get(0, 0xFFFF)` sites, both on `game_random`.
pub const RANDOM_GET_CALL_VAS: [u32; 2] = [0x0061_a1bb, 0x0061_a1d5];
/// `GameAccess::game_random` [PDB public].
pub const GAME_RANDOM_VA: u32 = 0x00c0_6184;
/// `GameAccess::objects` [PDB public].
pub const OBJECTS_VA: u32 = 0x00c0_618c;
/// `GameAccess::constants` [PDB public].
pub const CONSTANTS_VA: u32 = 0x00c0_61f0;
/// `GameAccess::game` [PDB public]; `+0x550` is `Game::frame`.
pub const GAME_VA: u32 = 0x00c0_61ec;
/// `MiscAccess::options` [PDB public]; `+0x90` is `Options::rebuild`.
pub const OPTIONS_VA: u32 = 0x00c0_6204;
/// `Game::frame` [PDB `types.json`].
pub const GAME_FRAME_OFFSET: u32 = 0x550;
/// `Options::rebuild` [PDB `types.json`], written unconditionally at `0x0061A0B6`.
pub const OPTIONS_REBUILD_OFFSET: u32 = 0x90;
/// `const Build::'vftable'` — the fast path of the `get_garrison_limit` devirtualisation.
pub const BUILD_VFTABLE_VA: u32 = 0x00b4_2174;
/// `ObjectData::is` `0x00653790`, the 12-byte type-query forwarder every `is(...)` site
/// devirtualises against.
pub const OBJECT_DATA_IS_VA: u32 = 0x0065_3790;

// ---------------------------------------------------------------------------
// Shipped constants the dispatch reads
// ---------------------------------------------------------------------------

/// `SubObjectData::x_internal` / `y_internal` are stored XOR'd with this [measured, every
/// coordinate read in the body].
pub const COORD_OBFUSCATION: u32 = 0x0006_3637;
/// `FilterIndex` passed to both spatial finders.
pub const SEARCH_FILTER: i32 = 0x11;
/// The worker pair, which repairs/builds/gathers at a **non**-University building.
pub const WORKER_TYPES: [i32; 2] = [0x32, 0x33];
/// The Scholar pair, which gathers only at a University.
pub const SCHOLAR_TYPES: [i32; 2] = [0x34, 0x35];
/// `TypeIndex::UNIVERSITY`, the polarity pivot between the two specialist arms.
pub const UNIVERSITY_TYPE: i32 = 0x1a4;
/// The three `TypeData::where` probes that promote the movement fallback to mode 2.
pub const MOVE_MODE_PROMOTING_WHERE_TYPES: [i32; 3] = [0x1ab, 0x1ac, 0x1b0];
/// Types that return before the army gate is even considered.
pub const ARMY_EXEMPT_TYPES: [i32; 3] = [0x3d, 0x3e, 0x190];
/// `UnitData::unit_masks` bit that opens the terminal army gate.
pub const ARMY_CANDIDATE_MASK: u32 = 0x0004_0000;
/// `ObjectTypeData::unit_flags2` bit tested at `0x0061A134`.
pub const UNIT_FLAGS2_ARMY_BIT: u32 = 0x10;
/// The `is()` probe that selects between the two RNG residues.
pub const ARMY_RESIDUE_PROBE_TYPE: i32 = 0x143;
/// The `is()` probe used on the no-RNG branch.
pub const ARMY_NO_RNG_PROBE_TYPE: i32 = 0x3a;
/// `Random::get` bounds at both sites.
pub const RANDOM_MIN: i32 = 0;
pub const RANDOM_MAX: i32 = 0xffff;
/// `(x_size + y_size) * 0x30` — the per-type half of the search band.
pub const SEARCH_BAND_TYPE_SCALE: i32 = 0x30;
/// `Unit::find_nearby_spot`'s fixed angle on the uber-size movement route.
pub const UBER_PROBE_ANGLE: u32 = 0x5555_5555;
/// `Unit::find_nearby_spot`'s fixed filter on the uber-size movement route.
pub const UBER_PROBE_FILTER: i32 = 3;
/// `SpecialAnimKind::Exit`, matching [`crate::systems::special_anim_executor`].
pub const SPEC_ANIM_EXIT: i32 = 1;

// ---------------------------------------------------------------------------
// Identities and scalars
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ObjectIdentity {
    pub owner: i8,
    pub object: i16,
}

impl ObjectIdentity {
    pub const fn new(owner: i8, object: i16) -> Self {
        Self { owner, object }
    }

    pub const fn valid(self) -> bool {
        self.owner >= 0 && self.object >= 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RngStamp {
    pub seed: u32,
    pub draws: u64,
}

/// `QueuePos`, as pushed. Retail only ever passes these three here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueuePos {
    /// `0`.
    Append,
    /// `1`.
    Replace,
    /// `2`.
    Front,
}

impl QueuePos {
    pub const fn raw(self) -> i32 {
        match self {
            QueuePos::Append => 0,
            QueuePos::Replace => 1,
            QueuePos::Front => 2,
        }
    }
}

// ---------------------------------------------------------------------------
// Host facts
// ---------------------------------------------------------------------------

/// Everything about the released unit the tail reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActorFacts {
    pub identity: ObjectIdentity,
    /// `TypeData::type` at `type +0x04`.
    pub type_index: i32,
    /// `TypeData::where` at `type +0x40`. Negative means "no producer type".
    pub where_type: i32,
    /// `ObjectTypeData::attack` at `type +0x1E8`.
    pub attack: i32,
    /// `ObjectTypeData::x_size` / `y_size` at `type +0x234` / `+0x238`.
    pub x_size: i32,
    pub y_size: i32,
    /// `ObjectTypeData::uber_size` at `type +0x308`.
    pub uber_size: i32,
    /// `ObjectTypeData::unit_flags2` at `type +0x2B8`.
    pub unit_flags2: u32,
    /// `UnitData::unit_masks` at `+0x68`.
    pub unit_masks: u32,
    /// `UnitData::is_on_map`, the devirtualised `+0xD0` slot.
    pub on_map: bool,
    /// The `+0xF4` virtual the movement branches compare against `5`.
    pub domain_query: i32,
    /// The `type +0x10C` virtual the movement branch tests for zero.
    pub type_movement_query: i32,
    /// `UnitData::o_down` at `+0x90`, then each link's own `o_down`, in walk order,
    /// terminated when retail's `o_down < 0`. Empty means the actor is the only link.
    pub o_down_chain: Vec<i16>,
}

/// The container the unit just left. `None` when the earlier tranches left `container_o < 0`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContainerFacts {
    pub identity: ObjectIdentity,
    /// The `+0x08` virtual guarded before `Unit::set_angle` at `0x006191F7`.
    pub active: bool,
    /// `UnitData::angle` at `+0x50`, passed verbatim to `Unit::set_angle`.
    pub angle: i32,
    /// `SubObjectData::x_internal` / `y_internal` **already de-obfuscated**.
    pub position: Point,
}

/// What the gather-selection tranche handed over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectionFacts {
    /// The `[esp+0x20]` flag. `false` means the loop exhausted with no candidate.
    pub selected: bool,
    /// The selected point, already the raw GatherPoint coordinates.
    pub point: Point,
    /// `GatherPoint::action` at `+0x0C`.
    pub action: u8,
    /// The scratch group index from `Groups::push_group`, or negative.
    pub scratch_group: i32,
    /// The first tranche's `container_gpiece` local. Retail uses this single value as **both**
    /// coordinates on the exhaust fallback and as `add_spec_anim_order`'s second argument.
    pub container_gpiece: i32,
    /// The first tranche's `local_9ec`, initialised `-1` at function entry.
    pub spec_anim: i32,
}

/// `GameAccess::constants` fields the search band needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConstantsFacts {
    /// `Constants::unit_train_distance` at `+0x84`.
    pub unit_train_distance: i32,
    /// `Constants::unit_train_max_distance` at `+0x88`.
    pub unit_train_max_distance: i32,
}

/// Everything about a building the spatial finder returned.
///
/// Modelled as one struct rather than as ~15 individual vtable receipts, matching the
/// sibling gather-selection tranche's `BuildingDirectFacts`. Every field is a distinct retail
/// probe and is named with the slot it comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildingFacts {
    /// `ObjectsData::find_who` at `objects +0x200`, filled in by the finder. **Not** the gaia
    /// owner — it is the owner of the object the finder located.
    pub find_who: i8,
    /// `SubObjectData::x_internal` / `y_internal`, de-obfuscated.
    pub position: Point,
    /// `SubObjectData::flags & 1`.
    pub flags_alive: bool,
    /// `ObjectData::damage` at `+0x24`.
    pub damage: i32,
    /// The `+0x0C` virtual.
    pub slot_0c: i32,
    /// The `+0x1C` virtual, the specialist-dispatch gate.
    pub slot_1c: i32,
    /// The `+0x20` virtual of `objects[find_who][index]`.
    pub slot_20: i32,
    /// The `+0x24` virtual of the object's `ObjectData` (`+0xAC`).
    pub data_slot_24: i32,
    /// The `+0x4C` virtual.
    pub slot_4c: i32,
    /// `object->[+0xB0]()->[+0x18]->[+0x90]()`, the type-side gate shared by both specialist
    /// arms.
    pub type_slot_90: i32,
    /// `object->[+0xB0]()->is(0x1A4, false)`. Non-zero means the building is a University.
    pub is_university: bool,
    /// `BuildTypeData::get_garrison_limit(build_type, actor.who)`.
    pub garrison_limit: i32,
    /// `UnitTypeData::can_garrison(actor_type, building_type)`.
    pub can_garrison: i32,
    /// `LeaderData::is_enemy(actor.who, find_who)`.
    pub is_enemy: bool,
    /// `LeaderData::is_ally(actor.who, find_who)`.
    pub is_ally: bool,
}

/// The complete fact set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleaseTailFacts {
    pub actor: ActorFacts,
    pub container: Option<ContainerFacts>,
    pub selection: SelectionFacts,
    pub constants: ConstantsFacts,
    /// `Game::frame`, the army gate's left operand.
    pub game_frame: u32,
    /// `types[actor.where_type]->is(t, false)` for the three
    /// [`MOVE_MODE_PROMOTING_WHERE_TYPES`], in probe order. Required only when the movement
    /// fallback is reached with `attack != 0`, `domain_query != 5` and `where_type >= 0`.
    pub where_probes: Option<[bool; 3]>,
    /// `this->is(0x143, false)` and `this->is(0x3A, false)`. Required only on the army path.
    pub army_probes: Option<ArmyProbeFacts>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArmyProbeFacts {
    /// `is(0x143, false)`.
    pub is_residue_type: bool,
    /// `is(0x3A, false)`. Read only on the `unit_flags2 & 0x10 == 0` branch.
    pub is_no_rng_type: bool,
}

// ---------------------------------------------------------------------------
// Host calls
// ---------------------------------------------------------------------------

/// A host call the planner must have a receipt for, in retail order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostCallKind {
    /// `ObjectsData::find_any_building_at(tx, ty, search, who, 0x11, who, search)`
    /// `0x0061922D`. Returns the building index, negative when nothing is there.
    FindAnyBuildingAt { tile: Point, who: i8 },
    /// `ObjectsData::find_unit_with_radius(x, y, .., who, 1, .., 0x11, ..)` `0x00619AC2`.
    FindUnitWithRadius { point: Point, who: i8 },
    /// `find_angle(dx, dy)` `0x0092D130`. `__fastcall`: `dx` in ECX, `dy` in EDX.
    FindAngle { dx: i32, dy: i32 },
    /// `UnitType::find_nearby_spot(cx, cy, &nx, &ny, lo, hi, 0, angle, 0, o, who, 0, 0, -1, 0, -1)`.
    FindNearbySpot {
        centre: Point,
        band: (i32, i32),
        angle: i32,
        filter: i32,
    },
    /// `Unit::find_nearby_spot(x, y, &nx, &ny, 0, -1, 0, 0x55555555, 3, 0, 1, -1)`
    /// `0x00619E5C`. Its return code is discarded by retail.
    UnitFindNearbySpot { point: Point },
    /// `Random::get(0, 0xFFFF)` on `game_random`, at exactly one of
    /// [`RANDOM_GET_CALL_VAS`].
    RandomGet { va: u32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostReturn {
    /// A finder index. Negative is "nothing found".
    Index(i32),
    /// An angle.
    Angle(i32),
    /// `find_nearby_spot`'s `(code, point)`. `code == 0` accepts.
    Spot { code: i32, point: Point },
    /// A `Random::get` result.
    Draw(i32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostCallReceipt {
    pub call: HostCallKind,
    pub ret: HostReturn,
    /// The RNG stamp **after** the call. Only [`HostCallKind::RandomGet`] may advance it.
    pub rng: RngStamp,
    /// The `BuildingFacts` the finder resolved, for the two finder kinds.
    pub found: Option<BuildingFacts>,
}

// ---------------------------------------------------------------------------
// Plan
// ---------------------------------------------------------------------------

/// One order retail installs, with its measured argument list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstalledOrder {
    /// `Unit::add_build_order(o, who, QueuePos, int)` `0x006194F5`, pushed `edi, esi, 0, 1, 0`
    /// — so `(b, find_who, Replace, 0)`.
    Build {
        o: i32,
        who: i8,
        queue: QueuePos,
        flag: i32,
    },
    /// `Unit::add_repair_order(o, who, QueuePos, int)` `0x00619542`.
    Repair {
        o: i32,
        who: i8,
        queue: QueuePos,
        flag: i32,
    },
    /// `Unit::add_gather_order(o, QueuePos, int)` `0x006194A8` (Scholar) / `0x006195F8`
    /// (worker). No owner argument — the PDB signature has three parameters.
    Gather { o: i32, queue: QueuePos, flag: i32 },
    /// `Unit::add_trade_order(o, who, oxx, whose, QueuePos, int)` `0x006196B5`, pushed
    /// `edi, esi, -1, -1, 1, 0`.
    Trade {
        o: i32,
        who: i8,
        oxx: i32,
        whose: i32,
        queue: QueuePos,
        flag: i32,
    },
    /// `Unit::add_garrison_order(o, who, int, QueuePos, int)` `0x006197D4` / `0x0061981B`,
    /// pushed `edi, esi, 0, 1, 0`.
    Garrison {
        o: i32,
        who: i8,
        arg3: i32,
        queue: QueuePos,
        flag: i32,
    },
    /// `Unit::add_attack_order(o, who, QueuePos, int, int)` `0x005E5410`. The last two
    /// arguments are the ones Ghidra dropped; they differ per call site.
    Attack {
        o: i32,
        who: i8,
        queue: QueuePos,
        arg4: i32,
        arg5: i32,
    },
    /// `Unit::add_move_order(x, y, int, int, QueuePos, int, int, Coord, Coord)` `0x00619984`
    /// / `0x006199D6`.
    Move {
        point: Point,
        arg3: i32,
        queue: QueuePos,
    },
    /// `Unit::add_move_facing_order(...)` `0x00619FDD`, the ordinary movement fallback.
    MoveFacing { tile: Point, angle: i32, mode: i32 },
    /// `Group::action_move_to(x, y, QueuePos, int, int, OrderIndex, ...)` — the uber-size and
    /// scratch-group routes at `0x006199EA`, `0x00619A04` and `0x00619E98`. `group` is the
    /// `groups.list` index (stride `0x9D4` from `[0x00E85F20]`).
    GroupMoveTo {
        point: Point,
        group: i32,
        mode: i32,
        angle: Option<i32>,
    },
}

/// A published effect, in retail order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReleaseTailStep {
    /// `Unit::set_angle(container.angle)` `0x006191F7`.
    SetAngle { angle: i32 },
    /// `Unit::set_new_location(x, y, 1, 1)`.
    SetNewLocation { point: Point },
    /// One order, on one object. The dispatcher issues the identical order to the actor and
    /// then to every link of its `o_down` chain.
    InstallOrder {
        on: ObjectIdentity,
        order: InstalledOrder,
    },
    /// `Unit::add_spec_anim_order(1, container_gpiece, 0, <unresolved>)` `0x0061A00D`,
    /// then the four `Unit::update_order` writes and `Unit::do_spec_anim`.
    SpecialAnimExit {
        gpiece: i32,
        /// `SpecialAnim +0x1C`, the container's de-obfuscated `x_internal`.
        data3: i32,
        /// `SpecialAnim +0x20`, the container's de-obfuscated `y_internal`.
        data4: i32,
        /// `SpecialAnim +0x24`.
        ox: i32,
        /// `SpecialAnim +0x28`.
        whom: i32,
    },
    /// `options.rebuild = 1` `0x0061A0B6`. Unconditional; every path reaches it.
    SetOptionsRebuild,
    /// `Unit::add_to_army()` `0x0061A1F6`.
    AddToArmy,
}

/// Where the tail stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReleaseTailExit {
    /// The tail always returns `0`. `Unit::come_out` returns non-zero only through the
    /// outlined island at `0x0061A216`, which belongs to the first tranche.
    Returned(i32),
}

/// A decision the tail reached but whose retail argument this lane could not derive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReleaseTailBoundary {
    /// `0x0061A002 push ecx` supplies `add_spec_anim_order`'s fourth (`QueuePos`) argument
    /// from a register that is not established on every path into `0x00619FE2`. The other
    /// three arguments are measured; this one is not derivable from the body alone.
    SpecAnimQueuePosUnestablished { push_va: u32 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleaseTailPlan {
    pub steps: Vec<ReleaseTailStep>,
    pub exit: ReleaseTailExit,
    pub boundaries: Vec<ReleaseTailBoundary>,
    /// The stamp after the last receipt. `draws` advances by at most one across the tail.
    pub rng: RngStamp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReleaseTailError {
    /// The receipt stream ran out where retail makes a call.
    MissingReceipt { expected: HostCallKind },
    /// The next receipt is for a different call than retail makes here.
    ReceiptMismatch {
        expected: HostCallKind,
        got: HostCallKind,
    },
    /// A receipt carried the wrong return shape.
    ReceiptReturnMismatch { call: HostCallKind },
    /// A finder receipt returned a non-negative index with no `BuildingFacts`.
    MissingFoundFacts { call: HostCallKind },
    /// Receipts were supplied that retail never makes.
    UnconsumedReceipts { remaining: usize },
    /// A non-`RandomGet` receipt advanced the RNG, or a `RandomGet` did not advance it by
    /// exactly one.
    RngDiscontinuity { at: HostCallKind },
    /// `RandomGet` was recorded at a VA that is not one of [`RANDOM_GET_CALL_VAS`].
    RandomGetAtWrongSite { va: u32 },
    /// A conditionally required fact was not supplied.
    MissingFact(MissingFact),
    /// The actor identity is not addressable.
    ActorOutOfRange { identity: ObjectIdentity },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MissingFact {
    /// `where_probes` were needed by the movement fallback.
    WhereProbes,
    /// `army_probes` were needed by the terminal gate.
    ArmyProbes,
}

// ---------------------------------------------------------------------------
// Receipt cursor
// ---------------------------------------------------------------------------

struct Cursor<'a> {
    receipts: &'a [HostCallReceipt],
    next: usize,
    rng: RngStamp,
}

impl<'a> Cursor<'a> {
    fn new(receipts: &'a [HostCallReceipt], rng: RngStamp) -> Self {
        Self {
            receipts,
            next: 0,
            rng,
        }
    }

    fn take(&mut self, expected: HostCallKind) -> Result<HostCallReceipt, ReleaseTailError> {
        let Some(receipt) = self.receipts.get(self.next).copied() else {
            return Err(ReleaseTailError::MissingReceipt { expected });
        };
        if receipt.call != expected {
            return Err(ReleaseTailError::ReceiptMismatch {
                expected,
                got: receipt.call,
            });
        }
        self.next += 1;

        let is_draw = matches!(receipt.call, HostCallKind::RandomGet { .. });
        let want_draws = if is_draw {
            self.rng.draws + 1
        } else {
            self.rng.draws
        };
        if receipt.rng.draws != want_draws {
            return Err(ReleaseTailError::RngDiscontinuity { at: receipt.call });
        }
        if !is_draw && receipt.rng.seed != self.rng.seed {
            return Err(ReleaseTailError::RngDiscontinuity { at: receipt.call });
        }
        if let HostCallKind::RandomGet { va } = receipt.call {
            if !RANDOM_GET_CALL_VAS.contains(&va) {
                return Err(ReleaseTailError::RandomGetAtWrongSite { va });
            }
        }
        self.rng = receipt.rng;
        Ok(receipt)
    }

    fn index(
        &mut self,
        expected: HostCallKind,
    ) -> Result<(i32, Option<BuildingFacts>), ReleaseTailError> {
        let receipt = self.take(expected)?;
        let HostReturn::Index(index) = receipt.ret else {
            return Err(ReleaseTailError::ReceiptReturnMismatch { call: expected });
        };
        if index >= 0 && receipt.found.is_none() {
            return Err(ReleaseTailError::MissingFoundFacts { call: expected });
        }
        Ok((index, receipt.found))
    }

    fn angle(&mut self, expected: HostCallKind) -> Result<i32, ReleaseTailError> {
        let receipt = self.take(expected)?;
        let HostReturn::Angle(angle) = receipt.ret else {
            return Err(ReleaseTailError::ReceiptReturnMismatch { call: expected });
        };
        Ok(angle)
    }

    fn spot(&mut self, expected: HostCallKind) -> Result<(i32, Point), ReleaseTailError> {
        let receipt = self.take(expected)?;
        let HostReturn::Spot { code, point } = receipt.ret else {
            return Err(ReleaseTailError::ReceiptReturnMismatch { call: expected });
        };
        Ok((code, point))
    }

    fn draw(&mut self, va: u32) -> Result<i32, ReleaseTailError> {
        let receipt = self.take(HostCallKind::RandomGet { va })?;
        let HostReturn::Draw(draw) = receipt.ret else {
            return Err(ReleaseTailError::ReceiptReturnMismatch {
                call: HostCallKind::RandomGet { va },
            });
        };
        Ok(draw)
    }

    fn finish(self) -> Result<RngStamp, ReleaseTailError> {
        if self.next != self.receipts.len() {
            return Err(ReleaseTailError::UnconsumedReceipts {
                remaining: self.receipts.len() - self.next,
            });
        }
        Ok(self.rng)
    }
}

// ---------------------------------------------------------------------------
// The planner
// ---------------------------------------------------------------------------

/// The search band both repositioning sites compute:
/// `lo = (x_size + y_size) * 0x30 + constants.unit_train_distance`,
/// `hi = lo + (unit_train_max_distance - unit_train_distance)`.
///
/// [measured, `0x00619279..0x0061929B` and `0x00619B0B..0x00619B30`] both sites compute it
/// with the identical instruction sequence, including the `lea ecx, [eax + eax*2]; shl ecx, 4`
/// encoding of the `* 0x30`.
pub fn search_band(actor: &ActorFacts, constants: &ConstantsFacts) -> (i32, i32) {
    let lo = (actor.x_size + actor.y_size)
        .wrapping_mul(SEARCH_BAND_TYPE_SCALE)
        .wrapping_add(constants.unit_train_distance);
    let hi = lo.wrapping_add(
        constants
            .unit_train_max_distance
            .wrapping_sub(constants.unit_train_distance),
    );
    (lo, hi)
}

/// The terminal army predicate.
///
/// `r` is `draw % 3` when `is(0x143)` holds and `draw & 1` (sign-corrected the way retail
/// does at `0x0061A1DF..0x0061A1E5`) otherwise, and the unit joins an army when
/// `(game.frame & r) != 0`.
pub fn army_residue(is_residue_type: bool, draw: i32) -> i32 {
    if is_residue_type {
        draw % 3
    } else {
        // and eax, 0x80000001 / jns / dec / or 0xfffffffe / inc — the MSVC idiom for a
        // sign-preserving `% 2`.
        let masked = (draw as u32 & 0x8000_0001) as i32;
        if masked < 0 {
            ((masked - 1) | -2i32).wrapping_add(1)
        } else {
            masked
        }
    }
}

/// Plan the release tail.
///
/// Receipts must be supplied in retail call order. The planner never mutates anything; it
/// returns an ordered [`ReleaseTailPlan`] a host may apply atomically, or an error.
pub fn plan_unit_come_out_release_tail(
    facts: &ReleaseTailFacts,
    receipts: &[HostCallReceipt],
    rng: RngStamp,
) -> Result<ReleaseTailPlan, ReleaseTailError> {
    if !facts.actor.identity.valid() {
        return Err(ReleaseTailError::ActorOutOfRange {
            identity: facts.actor.identity,
        });
    }
    let mut cursor = Cursor::new(receipts, rng);
    let mut steps: Vec<ReleaseTailStep> = Vec::new();
    let actor = &facts.actor;
    let who = actor.identity.owner;
    let sel = &facts.selection;

    // --- A. 0x006191A5: face the container -------------------------------------------
    if let Some(container) = facts.container {
        if container.identity.object >= 0 && container.active {
            steps.push(ReleaseTailStep::SetAngle {
                angle: container.angle,
            });
        }
    }

    // `selected` gates the entire dispatcher. 0x006191FC.
    let mut selected = sel.selected;

    if selected {
        // --- C. 0x0061922D: is a building under the point? ---------------------------
        let tile = Point {
            x: sel.point.x >> 6,
            y: sel.point.y >> 6,
        };
        let (building_index, building) =
            cursor.index(HostCallKind::FindAnyBuildingAt { tile, who })?;

        if building_index >= 0 {
            let b = building.expect("index() enforces facts for a non-negative index");
            let band = search_band(actor, &facts.constants);
            let centre = facts
                .container
                .map(|c| c.position)
                .unwrap_or(Point { x: 0, y: 0 });

            // --- D. reposition beside the building -----------------------------------
            let angle = cursor.angle(HostCallKind::FindAngle {
                dx: b.position.x.wrapping_sub(centre.x),
                dy: b.position.y.wrapping_sub(centre.y),
            })?;
            let (code, spot) = cursor.spot(HostCallKind::FindNearbySpot {
                centre,
                band,
                angle,
                filter: 0,
            })?;
            if code == 0 {
                steps.push(ReleaseTailStep::SetNewLocation { point: spot });
            }

            // --- E. 0x00619386: specialist dispatch ----------------------------------
            // Both arms share the same three-call preamble and differ only in the polarity
            // of the University probe. That polarity flip is the whole content of the arm.
            let same_owner = b.find_who == who;
            let specialist_gate = b.slot_1c != 0;
            if WORKER_TYPES.contains(&actor.type_index) && specialist_gate {
                if b.slot_4c == 0 && same_owner {
                    steps.push(ReleaseTailStep::InstallOrder {
                        on: actor.identity,
                        order: InstalledOrder::Build {
                            o: building_index,
                            who: b.find_who,
                            queue: QueuePos::Replace,
                            flag: 0,
                        },
                    });
                    selected = false;
                } else if b.damage != 0 {
                    steps.push(ReleaseTailStep::InstallOrder {
                        on: actor.identity,
                        order: InstalledOrder::Repair {
                            o: building_index,
                            who: b.find_who,
                            queue: QueuePos::Replace,
                            flag: 0,
                        },
                    });
                    selected = false;
                } else if sel.action != 0
                    && b.slot_0c != 0
                    && b.type_slot_90 != 0
                    && !b.is_university
                    && same_owner
                {
                    steps.push(ReleaseTailStep::InstallOrder {
                        on: actor.identity,
                        order: InstalledOrder::Gather {
                            o: building_index,
                            queue: QueuePos::Replace,
                            flag: 1,
                        },
                    });
                    selected = false;
                }
            } else if SCHOLAR_TYPES.contains(&actor.type_index)
                && specialist_gate
                && sel.action != 0
                && b.slot_0c != 0
                && b.type_slot_90 != 0
                && b.is_university
                && same_owner
            {
                steps.push(ReleaseTailStep::InstallOrder {
                    on: actor.identity,
                    order: InstalledOrder::Gather {
                        o: building_index,
                        queue: QueuePos::Replace,
                        flag: 1,
                    },
                });
                selected = false;
            }

            // --- F. 0x0061960F: trade --------------------------------------------------
            let traded = actor.on_map && b.slot_20 != 0 && b.data_slot_24 != 0 && !b.is_enemy;
            if traded {
                steps.push(ReleaseTailStep::InstallOrder {
                    on: actor.identity,
                    order: InstalledOrder::Trade {
                        o: building_index,
                        who: b.find_who,
                        oxx: -1,
                        whose: -1,
                        queue: QueuePos::Replace,
                        flag: 0,
                    },
                });
                selected = false;
            } else {
                // --- G. 0x006196C7: garrison -------------------------------------------
                if selected
                    && b.flags_alive
                    && b.slot_20 != 0
                    && b.slot_4c != 0
                    && b.is_ally
                    && b.garrison_limit != 0
                    && b.can_garrison != 0
                    && sel.action != 0
                {
                    let order = InstalledOrder::Garrison {
                        o: building_index,
                        who: b.find_who,
                        arg3: 0,
                        queue: QueuePos::Replace,
                        flag: 0,
                    };
                    push_across_chain(&mut steps, actor, order);
                    selected = false;
                }
            }

            // --- H. 0x00619854: enemy building -----------------------------------------
            if actor.attack != 0 && b.is_enemy {
                if sel.action != 0 {
                    push_across_chain(
                        &mut steps,
                        actor,
                        InstalledOrder::Attack {
                            o: building_index,
                            who: b.find_who,
                            queue: QueuePos::Replace,
                            arg4: 1,
                            arg5: 1,
                        },
                    );
                } else if actor.domain_query == 5 {
                    if sel.scratch_group >= 0 {
                        steps.push(ReleaseTailStep::InstallOrder {
                            on: actor.identity,
                            order: InstalledOrder::GroupMoveTo {
                                point: sel.point,
                                group: sel.scratch_group,
                                mode: 1,
                                angle: None,
                            },
                        });
                    } else {
                        steps.push(ReleaseTailStep::InstallOrder {
                            on: actor.identity,
                            order: InstalledOrder::Move {
                                point: sel.point,
                                arg3: 1,
                                queue: QueuePos::Front,
                            },
                        });
                    }
                } else if actor.type_movement_query == 0 && sel.scratch_group >= 0 {
                    steps.push(ReleaseTailStep::InstallOrder {
                        on: actor.identity,
                        order: InstalledOrder::GroupMoveTo {
                            point: sel.point,
                            group: sel.scratch_group,
                            mode: 2,
                            angle: None,
                        },
                    });
                } else if actor.type_movement_query == 0 && actor.uber_size <= 1 {
                    steps.push(ReleaseTailStep::InstallOrder {
                        on: actor.identity,
                        order: InstalledOrder::Move {
                            point: sel.point,
                            arg3: 2,
                            queue: QueuePos::Front,
                        },
                    });
                } else {
                    push_across_chain(
                        &mut steps,
                        actor,
                        InstalledOrder::Attack {
                            o: building_index,
                            who: b.find_who,
                            queue: QueuePos::Replace,
                            arg4: 1,
                            arg5: 0,
                        },
                    );
                }
                return finish(cursor, steps, facts);
            }
        }

        // --- U. 0x00619AA3: is a unit under the point? --------------------------------
        if selected {
            let (unit_index, found) = cursor.index(HostCallKind::FindUnitWithRadius {
                point: sel.point,
                who,
            })?;
            if unit_index >= 0 {
                let u = found.expect("index() enforces facts for a non-negative index");
                let band = search_band(actor, &facts.constants);
                let centre = facts
                    .container
                    .map(|c| c.position)
                    .unwrap_or(Point { x: 0, y: 0 });
                let angle = cursor.angle(HostCallKind::FindAngle {
                    dx: u.position.x.wrapping_sub(centre.x),
                    dy: u.position.y.wrapping_sub(centre.y),
                })?;
                let (code, spot) = cursor.spot(HostCallKind::FindNearbySpot {
                    centre,
                    band,
                    angle,
                    filter: 0,
                })?;
                if code == 0 {
                    steps.push(ReleaseTailStep::SetNewLocation { point: spot });
                }
                if actor.attack != 0 && u.is_enemy {
                    // The two arms differ. Ghidra renders them identically; the instruction
                    // stream does not.
                    let (arg4, arg5) = if sel.action != 0 { (1, 1) } else { (0, 0) };
                    push_across_chain(
                        &mut steps,
                        actor,
                        InstalledOrder::Attack {
                            o: unit_index,
                            who: u.find_who,
                            queue: QueuePos::Replace,
                            arg4,
                            arg5,
                        },
                    );
                    return finish(cursor, steps, facts);
                }
            }

            // --- M. 0x00619D74: movement fallback --------------------------------------
            let mut mode = 1;
            if actor.attack != 0 && actor.domain_query != 5 && actor.where_type >= 0 {
                let probes = facts
                    .where_probes
                    .ok_or(ReleaseTailError::MissingFact(MissingFact::WhereProbes))?;
                if probes.iter().any(|hit| *hit) {
                    mode = 2;
                }
            }
            if actor.on_map {
                if actor.uber_size > 1 && sel.scratch_group >= 0 {
                    let (_code, spot) =
                        cursor.spot(HostCallKind::UnitFindNearbySpot { point: sel.point })?;
                    let angle = cursor.angle(HostCallKind::FindAngle {
                        dx: spot.y.wrapping_sub(sel.point.y),
                        dy: spot.x.wrapping_sub(sel.point.x),
                    })?;
                    steps.push(ReleaseTailStep::InstallOrder {
                        on: actor.identity,
                        order: InstalledOrder::GroupMoveTo {
                            point: spot,
                            group: sel.scratch_group,
                            mode,
                            angle: Some(angle),
                        },
                    });
                } else {
                    let band = search_band(actor, &facts.constants);
                    let centre = facts
                        .container
                        .map(|c| c.position)
                        .unwrap_or(Point { x: 0, y: 0 });
                    let angle = cursor.angle(HostCallKind::FindAngle {
                        dx: sel.point.x.wrapping_sub(centre.x),
                        dy: sel.point.y.wrapping_sub(centre.y),
                    })?;
                    let (code, spot) = cursor.spot(HostCallKind::FindNearbySpot {
                        centre,
                        band,
                        angle,
                        filter: 0,
                    })?;
                    if code == 0 {
                        steps.push(ReleaseTailStep::SetNewLocation { point: spot });
                    }
                    let facing = cursor.angle(HostCallKind::FindAngle {
                        dx: spot.x.wrapping_sub(sel.point.x),
                        dy: spot.y.wrapping_sub(sel.point.y),
                    })?;
                    steps.push(ReleaseTailStep::InstallOrder {
                        on: actor.identity,
                        order: InstalledOrder::MoveFacing {
                            tile: Point {
                                x: spot.x >> 4,
                                y: spot.y >> 4,
                            },
                            angle: facing,
                            mode,
                        },
                    });
                }
            }
        }
    }

    finish(cursor, steps, facts)
}

/// Issue `order` to the actor and then to every link of its `o_down` chain.
///
/// [measured, `0x006197D9..0x00619852` (garrison), `0x006198B9..0x00619932` and
/// `0x00619A1D..0x00619A92` (attack), `0x00619C7A..0x00619CED` and `0x00619D01..0x00619D6D`
/// (unit attack)] every one of those loops has the identical shape: read `UnitData::o_down`
/// (`+0x90`, signed short), stop when negative, resolve `objects[actor.who][o_down]`, take its
/// `+0xA8` virtual to get the `Unit`, issue the same order, then re-read that link's own
/// `o_down`.
fn push_across_chain(steps: &mut Vec<ReleaseTailStep>, actor: &ActorFacts, order: InstalledOrder) {
    steps.push(ReleaseTailStep::InstallOrder {
        on: actor.identity,
        order,
    });
    for link in &actor.o_down_chain {
        if *link < 0 {
            break;
        }
        steps.push(ReleaseTailStep::InstallOrder {
            on: ObjectIdentity::new(actor.identity.owner, *link),
            order,
        });
    }
}

/// The common tail every path funnels through: `0x00619FE2` onwards.
fn finish(
    mut cursor: Cursor<'_>,
    mut steps: Vec<ReleaseTailStep>,
    facts: &ReleaseTailFacts,
) -> Result<ReleaseTailPlan, ReleaseTailError> {
    let mut boundaries: Vec<ReleaseTailBoundary> = Vec::new();
    let actor = &facts.actor;
    let sel = &facts.selection;

    // 0x00619FE2: the SPECIAL_ANIM exit order.
    if sel.spec_anim != 0 && facts.game_frame != 0 {
        let container = facts.container;
        steps.push(ReleaseTailStep::SpecialAnimExit {
            gpiece: sel.container_gpiece,
            data3: container.map(|c| c.position.x).unwrap_or(0),
            data4: container.map(|c| c.position.y).unwrap_or(0),
            ox: container.map(|c| c.identity.object as i32).unwrap_or(-1),
            whom: container.map(|c| c.identity.owner as i32).unwrap_or(-1),
        });
        boundaries.push(ReleaseTailBoundary::SpecAnimQueuePosUnestablished {
            push_va: 0x0061_a002,
        });
    }

    // 0x0061A0B6: unconditional.
    steps.push(ReleaseTailStep::SetOptionsRebuild);

    // 0x0061A0C5: the terminal army gate.
    let army_candidate = actor.unit_masks & ARMY_CANDIDATE_MASK != 0
        && !actor.on_map
        && !ARMY_EXEMPT_TYPES.contains(&actor.type_index);
    if army_candidate {
        let probes = facts
            .army_probes
            .ok_or(ReleaseTailError::MissingFact(MissingFact::ArmyProbes))?;
        let joins = if actor.unit_flags2 & UNIT_FLAGS2_ARMY_BIT == 0 && !probes.is_residue_type {
            // 0x0061A13B: no draw at all on this branch.
            !probes.is_no_rng_type
        } else {
            let va = if probes.is_residue_type {
                RANDOM_GET_CALL_VAS[0]
            } else {
                RANDOM_GET_CALL_VAS[1]
            };
            let draw = cursor.draw(va)?;
            let residue = army_residue(probes.is_residue_type, draw);
            (facts.game_frame as i32 & residue) != 0
        };
        if joins {
            steps.push(ReleaseTailStep::AddToArmy);
        }
    }

    let rng = cursor.finish()?;
    Ok(ReleaseTailPlan {
        steps,
        exit: ReleaseTailExit::Returned(0),
        boundaries,
        rng,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn stamp(draws: u64) -> RngStamp {
        RngStamp {
            seed: 0x1234_5678,
            draws,
        }
    }

    fn actor() -> ActorFacts {
        ActorFacts {
            identity: ObjectIdentity::new(1, 7),
            type_index: 0x32,
            where_type: -1,
            attack: 0,
            x_size: 1,
            y_size: 1,
            uber_size: 1,
            unit_flags2: 0,
            unit_masks: 0,
            on_map: true,
            domain_query: 0,
            type_movement_query: 0,
            o_down_chain: Vec::new(),
        }
    }

    fn container() -> ContainerFacts {
        ContainerFacts {
            identity: ObjectIdentity::new(1, 3),
            active: true,
            angle: 0x4000,
            position: Point { x: 1_000, y: 2_000 },
        }
    }

    fn selection() -> SelectionFacts {
        SelectionFacts {
            selected: true,
            point: Point { x: 1_280, y: 2_560 },
            action: 1,
            scratch_group: -1,
            container_gpiece: 0x99,
            spec_anim: 0,
        }
    }

    fn constants() -> ConstantsFacts {
        ConstantsFacts {
            unit_train_distance: 100,
            unit_train_max_distance: 400,
        }
    }

    fn facts() -> ReleaseTailFacts {
        ReleaseTailFacts {
            actor: actor(),
            container: Some(container()),
            selection: selection(),
            constants: constants(),
            game_frame: 0,
            where_probes: None,
            army_probes: None,
        }
    }

    fn building(find_who: i8) -> BuildingFacts {
        BuildingFacts {
            find_who,
            position: Point { x: 1_300, y: 2_600 },
            flags_alive: true,
            damage: 0,
            slot_0c: 1,
            slot_1c: 1,
            slot_20: 0,
            data_slot_24: 0,
            slot_4c: 0,
            type_slot_90: 1,
            is_university: false,
            garrison_limit: 0,
            can_garrison: 0,
            is_enemy: false,
            is_ally: false,
        }
    }

    /// Receipts for "a building was found, repositioning succeeded".
    fn building_receipts(
        index: i32,
        b: BuildingFacts,
        tile: Point,
        who: i8,
    ) -> Vec<HostCallReceipt> {
        let band = search_band(&actor(), &constants());
        let centre = container().position;
        vec![
            HostCallReceipt {
                call: HostCallKind::FindAnyBuildingAt { tile, who },
                ret: HostReturn::Index(index),
                rng: stamp(0),
                found: Some(b),
            },
            HostCallReceipt {
                call: HostCallKind::FindAngle {
                    dx: b.position.x - centre.x,
                    dy: b.position.y - centre.y,
                },
                ret: HostReturn::Angle(0x2000),
                rng: stamp(0),
                found: None,
            },
            HostCallReceipt {
                call: HostCallKind::FindNearbySpot {
                    centre,
                    band,
                    angle: 0x2000,
                    filter: 0,
                },
                ret: HostReturn::Spot {
                    code: 0,
                    point: Point { x: 1_290, y: 2_580 },
                },
                rng: stamp(0),
                found: None,
            },
        ]
    }

    fn tile_of(p: Point) -> Point {
        Point {
            x: p.x >> 6,
            y: p.y >> 6,
        }
    }

    #[test]
    fn the_tranche_closes_the_body() {
        assert_eq!(SEQUENTIAL_BYTES, 4_193);
        assert_eq!(OWN_OUTLINED_BYTES, 84);
        assert_eq!(REATTRIBUTED_PREFIX_ISLAND_BYTES, 72);
        assert_eq!(LOGICAL_TRANCHE_BYTES, 4_277);
        assert_eq!(RESIDUAL_BYTES_AFTER_TRANCHE, 0);
    }

    #[test]
    fn a_damaged_own_building_takes_the_repair_arm_and_an_undamaged_one_does_not() {
        // The pivot is ObjectData::damage at +0x24, measured at 0x00619517.
        let mut f = facts();
        f.actor.type_index = WORKER_TYPES[0];
        let tile = tile_of(f.selection.point);

        let mut b = building(1);
        b.slot_4c = 1; // not the build arm
        b.damage = 5;
        let plan = plan_unit_come_out_release_tail(&f, &building_receipts(4, b, tile, 1), stamp(0))
            .expect("repair plan");
        assert!(plan.steps.iter().any(|s| matches!(
            s,
            ReleaseTailStep::InstallOrder {
                order: InstalledOrder::Repair { o: 4, .. },
                ..
            }
        )));

        b.damage = 0;
        let plan = plan_unit_come_out_release_tail(&f, &building_receipts(4, b, tile, 1), stamp(0))
            .expect("gather plan");
        assert!(!plan.steps.iter().any(|s| matches!(
            s,
            ReleaseTailStep::InstallOrder {
                order: InstalledOrder::Repair { .. },
                ..
            }
        )));
        assert!(plan.steps.iter().any(|s| matches!(
            s,
            ReleaseTailStep::InstallOrder {
                order: InstalledOrder::Gather { o: 4, .. },
                ..
            }
        )));
    }

    #[test]
    fn the_university_probe_polarity_is_opposite_for_the_two_specialist_pairs() {
        // Worker pair: gathers only at a NON-University (0x006195D5 `jne skip`).
        // Scholar pair: gathers only AT a University (0x0061947B `je skip`).
        let tile = tile_of(selection().point);
        for (type_index, university, want_gather) in [
            (WORKER_TYPES[0], false, true),
            (WORKER_TYPES[0], true, false),
            (SCHOLAR_TYPES[0], true, true),
            (SCHOLAR_TYPES[0], false, false),
        ] {
            let mut f = facts();
            f.actor.type_index = type_index;
            // Off the map, so neither the trade gate nor the movement fallback fires and the
            // only difference between the four cases is the University probe.
            f.actor.on_map = false;
            let mut b = building(1);
            b.slot_4c = 1;
            b.is_university = university;
            let mut receipts = building_receipts(9, b, tile, 1);
            if !want_gather {
                // No specialist order installed, so `selected` survives and retail runs the
                // unit probe at 0x00619AA3.
                receipts.push(HostCallReceipt {
                    call: HostCallKind::FindUnitWithRadius {
                        point: f.selection.point,
                        who: 1,
                    },
                    ret: HostReturn::Index(-1),
                    rng: stamp(0),
                    found: None,
                });
            }
            let plan = plan_unit_come_out_release_tail(&f, &receipts, stamp(0)).expect("plan");
            let gathered = plan.steps.iter().any(|s| {
                matches!(
                    s,
                    ReleaseTailStep::InstallOrder {
                        order: InstalledOrder::Gather { .. },
                        ..
                    }
                )
            });
            assert_eq!(
                gathered, want_gather,
                "type {type_index:#x} university={university}"
            );
        }
    }

    #[test]
    fn a_garrison_order_is_replicated_across_the_whole_o_down_chain() {
        let mut f = facts();
        f.actor.type_index = 0x40; // neither specialist pair
        f.actor.o_down_chain = vec![11, 12, 13];
        let mut b = building(1);
        b.flags_alive = true;
        b.slot_20 = 1;
        b.data_slot_24 = 0; // fails the trade gate
        b.slot_4c = 1;
        b.is_ally = true;
        b.garrison_limit = 4;
        b.can_garrison = 1;
        let tile = tile_of(f.selection.point);
        let plan = plan_unit_come_out_release_tail(&f, &building_receipts(6, b, tile, 1), stamp(0))
            .expect("garrison plan");
        let garrisoned: Vec<_> = plan
            .steps
            .iter()
            .filter_map(|s| match s {
                ReleaseTailStep::InstallOrder {
                    on,
                    order: InstalledOrder::Garrison { .. },
                } => Some(*on),
                _ => None,
            })
            .collect();
        assert_eq!(
            garrisoned,
            vec![
                ObjectIdentity::new(1, 7),
                ObjectIdentity::new(1, 11),
                ObjectIdentity::new(1, 12),
                ObjectIdentity::new(1, 13),
            ]
        );
    }

    #[test]
    fn the_two_unit_attack_arms_carry_different_arguments() {
        // Ghidra renders both as add_attack_order(u, who, 1). The instruction stream pushes
        // 1,1,1 at 0x00619C6D and 0,0,1 at 0x00619CF4.
        let tile = tile_of(selection().point);
        let mut collected = Vec::new();
        for action in [1u8, 0u8] {
            let mut f = facts();
            f.actor.type_index = 0x40;
            f.actor.attack = 1;
            f.selection.action = action;
            let mut u = building(2);
            u.is_enemy = true;
            let band = search_band(&f.actor, &f.constants);
            let centre = container().position;
            let receipts = vec![
                HostCallReceipt {
                    call: HostCallKind::FindAnyBuildingAt { tile, who: 1 },
                    ret: HostReturn::Index(-1),
                    rng: stamp(0),
                    found: None,
                },
                HostCallReceipt {
                    call: HostCallKind::FindUnitWithRadius {
                        point: f.selection.point,
                        who: 1,
                    },
                    ret: HostReturn::Index(21),
                    rng: stamp(0),
                    found: Some(u),
                },
                HostCallReceipt {
                    call: HostCallKind::FindAngle {
                        dx: u.position.x - centre.x,
                        dy: u.position.y - centre.y,
                    },
                    ret: HostReturn::Angle(1),
                    rng: stamp(0),
                    found: None,
                },
                HostCallReceipt {
                    call: HostCallKind::FindNearbySpot {
                        centre,
                        band,
                        angle: 1,
                        filter: 0,
                    },
                    ret: HostReturn::Spot {
                        code: 1,
                        point: Point { x: 0, y: 0 },
                    },
                    rng: stamp(0),
                    found: None,
                },
            ];
            let plan =
                plan_unit_come_out_release_tail(&f, &receipts, stamp(0)).expect("attack plan");
            let order = plan
                .steps
                .iter()
                .find_map(|s| match s {
                    ReleaseTailStep::InstallOrder {
                        order: InstalledOrder::Attack { arg4, arg5, .. },
                        ..
                    } => Some((*arg4, *arg5)),
                    _ => None,
                })
                .expect("an attack order");
            collected.push(order);
        }
        assert_eq!(collected, vec![(1, 1), (0, 0)]);
    }

    #[test]
    fn the_army_gate_draws_exactly_once_and_only_when_it_is_reached() {
        let mut f = facts();
        f.actor.unit_masks = ARMY_CANDIDATE_MASK;
        f.actor.on_map = false;
        f.actor.unit_flags2 = UNIT_FLAGS2_ARMY_BIT;
        f.selection.selected = false;
        f.game_frame = 3;
        f.army_probes = Some(ArmyProbeFacts {
            is_residue_type: true,
            is_no_rng_type: false,
        });
        let receipts = vec![HostCallReceipt {
            call: HostCallKind::RandomGet {
                va: RANDOM_GET_CALL_VAS[0],
            },
            ret: HostReturn::Draw(7),
            rng: stamp(1),
            found: None,
        }];
        let plan = plan_unit_come_out_release_tail(&f, &receipts, stamp(0)).expect("army plan");
        assert_eq!(plan.rng.draws, 1);
        // draw 7 % 3 == 1; frame 3 & 1 == 1 -> joins.
        assert!(plan.steps.contains(&ReleaseTailStep::AddToArmy));

        // The same unit on the map never reaches the gate and never draws.
        f.actor.on_map = true;
        let plan = plan_unit_come_out_release_tail(&f, &[], stamp(0)).expect("no army plan");
        assert_eq!(plan.rng.draws, 0);
        assert!(!plan.steps.contains(&ReleaseTailStep::AddToArmy));
    }

    #[test]
    fn the_army_gate_is_a_frame_parity_test_not_a_probability() {
        // Game+0x550 is `frame`. With frame 0 the AND is always zero, so no unit ever
        // joins an army on frame 0 however the draw falls.
        for draw in [0, 1, 2, 3, 4, 5, 0xfffe] {
            let mut f = facts();
            f.actor.unit_masks = ARMY_CANDIDATE_MASK;
            f.actor.on_map = false;
            f.actor.unit_flags2 = UNIT_FLAGS2_ARMY_BIT;
            f.selection.selected = false;
            f.game_frame = 0;
            f.army_probes = Some(ArmyProbeFacts {
                is_residue_type: true,
                is_no_rng_type: false,
            });
            let receipts = vec![HostCallReceipt {
                call: HostCallKind::RandomGet {
                    va: RANDOM_GET_CALL_VAS[0],
                },
                ret: HostReturn::Draw(draw),
                rng: stamp(1),
                found: None,
            }];
            let plan = plan_unit_come_out_release_tail(&f, &receipts, stamp(0)).expect("plan");
            assert!(
                !plan.steps.contains(&ReleaseTailStep::AddToArmy),
                "draw {draw} joined on frame 0"
            );
        }
    }

    #[test]
    fn the_no_rng_army_branch_consumes_no_draw() {
        let mut f = facts();
        f.actor.unit_masks = ARMY_CANDIDATE_MASK;
        f.actor.on_map = false;
        f.actor.unit_flags2 = 0; // the 0x10 bit is clear
        f.selection.selected = false;
        f.game_frame = 7;
        f.army_probes = Some(ArmyProbeFacts {
            is_residue_type: false,
            is_no_rng_type: false,
        });
        let plan = plan_unit_come_out_release_tail(&f, &[], stamp(0)).expect("plan");
        assert_eq!(plan.rng.draws, 0);
        assert!(plan.steps.contains(&ReleaseTailStep::AddToArmy));
    }

    #[test]
    fn the_army_exempt_types_return_before_the_gate() {
        for t in ARMY_EXEMPT_TYPES {
            let mut f = facts();
            f.actor.type_index = t;
            f.actor.unit_masks = ARMY_CANDIDATE_MASK;
            f.actor.on_map = false;
            f.selection.selected = false;
            // No army_probes supplied: reaching the gate would be a MissingFact error.
            let plan = plan_unit_come_out_release_tail(&f, &[], stamp(0)).expect("plan");
            assert!(!plan.steps.contains(&ReleaseTailStep::AddToArmy));
        }
    }

    #[test]
    fn options_rebuild_is_written_on_every_path() {
        let mut f = facts();
        f.selection.selected = false;
        let plan = plan_unit_come_out_release_tail(&f, &[], stamp(0)).expect("plan");
        assert!(plan.steps.contains(&ReleaseTailStep::SetOptionsRebuild));
        assert_eq!(plan.exit, ReleaseTailExit::Returned(0));
    }

    #[test]
    fn an_inactive_container_does_not_get_faced() {
        let mut f = facts();
        f.selection.selected = false;
        f.container = Some(ContainerFacts {
            active: false,
            ..container()
        });
        let plan = plan_unit_come_out_release_tail(&f, &[], stamp(0)).expect("plan");
        assert!(!plan
            .steps
            .iter()
            .any(|s| matches!(s, ReleaseTailStep::SetAngle { .. })));

        f.container = Some(container());
        let plan = plan_unit_come_out_release_tail(&f, &[], stamp(0)).expect("plan");
        assert_eq!(plan.steps[0], ReleaseTailStep::SetAngle { angle: 0x4000 });
    }

    #[test]
    fn the_spec_anim_order_records_its_unresolved_queue_argument() {
        let mut f = facts();
        f.selection.selected = false;
        f.selection.spec_anim = -1;
        f.game_frame = 12;
        let plan = plan_unit_come_out_release_tail(&f, &[], stamp(0)).expect("plan");
        assert!(plan.steps.iter().any(|s| matches!(
            s,
            ReleaseTailStep::SpecialAnimExit {
                gpiece: 0x99,
                data3: 1_000,
                data4: 2_000,
                ox: 3,
                whom: 1
            }
        )));
        assert_eq!(
            plan.boundaries,
            vec![ReleaseTailBoundary::SpecAnimQueuePosUnestablished {
                push_va: 0x0061_a002
            }]
        );
    }

    #[test]
    fn a_receipt_stream_that_does_not_match_the_call_order_is_refused() {
        let mut f = facts();
        f.selection.selected = true;
        let receipts = vec![HostCallReceipt {
            call: HostCallKind::FindUnitWithRadius {
                point: f.selection.point,
                who: 1,
            },
            ret: HostReturn::Index(-1),
            rng: stamp(0),
            found: None,
        }];
        let err = plan_unit_come_out_release_tail(&f, &receipts, stamp(0)).unwrap_err();
        assert!(matches!(err, ReleaseTailError::ReceiptMismatch { .. }));
    }

    #[test]
    fn a_random_get_recorded_at_the_wrong_site_is_refused() {
        let mut f = facts();
        f.actor.unit_masks = ARMY_CANDIDATE_MASK;
        f.actor.on_map = false;
        f.actor.unit_flags2 = UNIT_FLAGS2_ARMY_BIT;
        f.selection.selected = false;
        f.army_probes = Some(ArmyProbeFacts {
            is_residue_type: true,
            is_no_rng_type: false,
        });
        let receipts = vec![HostCallReceipt {
            call: HostCallKind::RandomGet { va: 0x0061_9999 },
            ret: HostReturn::Draw(1),
            rng: stamp(1),
            found: None,
        }];
        let err = plan_unit_come_out_release_tail(&f, &receipts, stamp(0)).unwrap_err();
        assert!(matches!(err, ReleaseTailError::ReceiptMismatch { .. }));
    }

    #[test]
    fn a_receipt_that_advances_the_rng_without_drawing_is_refused() {
        let mut f = facts();
        f.selection.selected = true;
        let tile = tile_of(f.selection.point);
        let receipts = vec![HostCallReceipt {
            call: HostCallKind::FindAnyBuildingAt { tile, who: 1 },
            ret: HostReturn::Index(-1),
            rng: stamp(1),
            found: None,
        }];
        let err = plan_unit_come_out_release_tail(&f, &receipts, stamp(0)).unwrap_err();
        assert!(matches!(err, ReleaseTailError::RngDiscontinuity { .. }));
    }

    #[test]
    fn unconsumed_receipts_are_refused() {
        let mut f = facts();
        f.selection.selected = false;
        let receipts = vec![HostCallReceipt {
            call: HostCallKind::FindAnyBuildingAt {
                tile: Point { x: 0, y: 0 },
                who: 1,
            },
            ret: HostReturn::Index(-1),
            rng: stamp(0),
            found: None,
        }];
        let err = plan_unit_come_out_release_tail(&f, &receipts, stamp(0)).unwrap_err();
        assert_eq!(err, ReleaseTailError::UnconsumedReceipts { remaining: 1 });
    }

    #[test]
    fn a_finder_hit_without_facts_is_refused() {
        let mut f = facts();
        f.selection.selected = true;
        let tile = tile_of(f.selection.point);
        let receipts = vec![HostCallReceipt {
            call: HostCallKind::FindAnyBuildingAt { tile, who: 1 },
            ret: HostReturn::Index(3),
            rng: stamp(0),
            found: None,
        }];
        let err = plan_unit_come_out_release_tail(&f, &receipts, stamp(0)).unwrap_err();
        assert!(matches!(err, ReleaseTailError::MissingFoundFacts { .. }));
    }

    #[test]
    fn the_movement_fallback_needs_its_where_probes() {
        let mut f = facts();
        f.actor.attack = 1;
        f.actor.where_type = 5;
        f.actor.type_index = 0x40;
        let tile = tile_of(f.selection.point);
        let receipts = vec![
            HostCallReceipt {
                call: HostCallKind::FindAnyBuildingAt { tile, who: 1 },
                ret: HostReturn::Index(-1),
                rng: stamp(0),
                found: None,
            },
            HostCallReceipt {
                call: HostCallKind::FindUnitWithRadius {
                    point: f.selection.point,
                    who: 1,
                },
                ret: HostReturn::Index(-1),
                rng: stamp(0),
                found: None,
            },
        ];
        let err = plan_unit_come_out_release_tail(&f, &receipts, stamp(0)).unwrap_err();
        assert_eq!(err, ReleaseTailError::MissingFact(MissingFact::WhereProbes));
    }

    #[test]
    fn the_search_band_is_the_measured_expression() {
        let a = ActorFacts {
            x_size: 3,
            y_size: 5,
            ..actor()
        };
        let c = ConstantsFacts {
            unit_train_distance: 64,
            unit_train_max_distance: 320,
        };
        // (3 + 5) * 0x30 + 64 = 448; hi = 448 + (320 - 64) = 704.
        assert_eq!(search_band(&a, &c), (448, 704));
    }

    #[test]
    fn the_signed_residue_matches_the_msvc_idiom() {
        // `and eax, 0x80000001` then the dec/or/inc sign fix is a sign-preserving % 2.
        assert_eq!(army_residue(false, 0), 0);
        assert_eq!(army_residue(false, 1), 1);
        assert_eq!(army_residue(false, 2), 0);
        assert_eq!(army_residue(false, 0xffff), 1);
        assert_eq!(army_residue(true, 7), 1);
        assert_eq!(army_residue(true, 9), 0);
    }

    #[test]
    fn an_unaddressable_actor_is_refused_before_any_receipt_is_read() {
        let mut f = facts();
        f.actor.identity = ObjectIdentity::new(-1, 7);
        let err = plan_unit_come_out_release_tail(&f, &[], stamp(0)).unwrap_err();
        assert!(matches!(err, ReleaseTailError::ActorOutOfRange { .. }));
    }
}
