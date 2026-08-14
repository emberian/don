//! # Walls — the `walls` checksum channel, and the building construction base layer
//!
//! A port of the `Wall` subsystem of *Rise of Nations: Extended Edition*, derived from
//! `ron-bin/riseofnations.exe` + `ron-bin/sbl/rise.pdb`. Everything is `[measured]`
//! against those two artifacts unless a comment says `[inference]` or `UNDERIVED`.
//!
//! ## The headline, and it is not what the lane brief expected
//!
//! **`BuildData` derives from `Wall`.** From the PDB type stream:
//!
//! ```text
//! SubObjectData -> SubObjectOut -> SubObject -> ObjectData -> ObjectOut -> Object
//!   -> WallData -> WallOut -> Wall -> BuildData -> BuildOut -> Build
//! ```
//!
//! Every building in the game *is* a `Wall`. `Wall` is not a fence you drag across the
//! map — it is the **construction/health/fog base class** that `Build` inherits. That is
//! why `Wall::process` turns up in the production lane as "the thing that zeroes builder
//! helper counts every frame": it is the base-class `process` that `Build::process` calls
//! on every building, every tick.
//!
//! Confirmation, by exhaustive reverse call graph over the 22,701 PDB procedures:
//! every `Wall::*` method has exactly one caller and it is the matching `Build::*`
//! (`docs/mechanics/walls.md` §"What Wall means in this engine" summarizes the result).
//! `Wall::inc_time` (`0x0063FB60`) has
//! *zero* direct callers because it is not overridden — it sits at `+0xA0` in **both**
//! the `Wall` and the `Build` vtables, so it is the per-building `inc_time`.
//!
//! ## The `walls` checksum channel is structurally empty in this build
//!
//! `CheckSums::check_walls` (`0x00937360`) walks `objects.lists[who][o]` for
//! `o in obj_base[2] .. wall_mark[who]`. `Objects::init` (`0x0065EA80`) sets
//! `obj_base = {0, 2000, 3000}` and `obj_end = {2000, 3000, 3000}` — the wall band is
//! `[3000, 3000)`, **zero capacity by construction**. `Objects::clear` (`0x0065D740`)
//! sets `wall_mark[i] = 3000`; an exhaustive scan of `.text` finds no other writer, and
//! `Objects::init_wall` (`0x00658C50`) has **zero callers**. There is no wall type in
//! `ron-data/buildingrules.xml` or `typenames.xml` either.
//!
//! So the channel walks nothing and contributes **exactly `1`** (adler-32 seeded to 1
//! over zero bytes) to the sixteen-dword checksum command, on every ordinary retail turn. See
//! [`WallChannel::retail_default`](crate::systems::walls::WallChannel::retail_default) and
//! the test that asserts it. The walker is still
//! implemented here because it is also the *shape* of the walked block that
//! `BuildData::walk_data` emits inside the **`builds`** channel.
//!
//! ## The walked byte stream — 88 bytes per live object
//!
//! `WallData::walk_data` (`0x00642510`) → `Object::walk_data` (`0x00647830`) →
//! `SubObject::walk_data` (`0x006621D0`). `CheckSum::walk_test` (`0x0041BFE0`) is an
//! empty COMDAT stub, so the field-name strings contribute **zero bytes**;
//! `CheckSum::walk_function` (`0x00936FF0`) is `accum = adler32(accum, begin, end-begin)`
//! plus `size += end-begin`.
//!
//! | # | bytes | engine range | contents |
//! |--:|------:|--------------|----------|
//! | 1 | 1  | `[+0x08, +0x09)` | `flags` |
//! | 2 | 1  | stack byte | `SubObject` `must_walk` |
//! | 3 | 15 | `[+0x09, +0x18)` | `who`, `o`, `z`, `x`, `y` — **only if `must_walk`** |
//! | 4 | 4  | stack dword | `ptype ? ptype->type : 0` — only if `must_walk` |
//! | 5 | 1  | stack byte | `Object` `must_walk` |
//! | 6 | 34 | `[+0x20, +0x42)` | `myhits` … `launch_frames` — only if `must_walk` |
//! | 7 | 1  | stack byte | `launching != nullptr` |
//! | 8 | —  |            | `SimpleArray<int>::walk_data` if that byte is set |
//! | 9 | 1  | stack byte | `WallData` `must_walk` |
//! |10 | 30 | `[+0x48, +0x66)` | `job_counter` … `demolition` — only if `must_walk` |
//!
//! `must_walk` is `Object::must_walk` (`0x00647930`), emitted **three times** (once per
//! level of the hierarchy) with the same value:
//! `(dw->input == 0) && (ptype != 0 || (flags & 1) || hold_frames != 0)`. For a `CheckSum`
//! walk `input` is always 0 (`0x0045DEC0`, the ctor), and the channel only visits
//! objects with `flags & 1`, so it is always 1 there — but the load path needs the general
//! form and it is implemented in full.
//!
//! **The walked window stops at `+0x66`, one field short of `WallOut::render_gpiece`
//! (`+0x68`)** — but `WallData::gpiece` at `+0x58` is *inside* it. Another instance of
//! "presentation classes are not always safe to skip": the art-piece index a building
//! picked is lockstep state, the render cache is not.
//!
//! **No float is stored anywhere in the walked block.** All 30 WallData bytes are
//! integers. `BuildData::hits` (`0x0062E740`) does contain an `f32` divide, but it is a
//! *query* that derives a value from walked integers during razing — see
//! [`razing_hits`](crate::systems::walls::razing_hits). Nothing float-valued is ever hashed
//! here.
//!
//! ## What is NOT here
//!
//! * The multiplier chain in `Wall::update_hits` (`0x0063F0D0`, 1,509 bytes) — ~12
//!   `(K + 100) * v / 100` percent bonuses off `Constants` fields whose *offsets* are
//!   recorded in the report but whose rule names are not derived.
//!   [`progress_hits`](crate::systems::walls::progress_hits)
//!   implements the part that is fully derived: the construction-progress ramp.
//! * `Wall::activate` (`0x0063E4B0`, 852 B), `Wall::init` (744 B), `Wall::close` (843 B),
//!   `Wall::inc_time` (2,273 B), `Wall::swap_team` (406 B) — read for structure, not
//!   ported. They are large and mostly call out to `World`/`Objects`/`Leader`.
//! * `Wall::mask_me` / `mask_city` / `kill_competing_buildings` / `kill_at_tile` — the
//!   terrain-mask and site-clearing layer. Owned by the map/terrain lane's data.
//! * `WallData::armor` (`0x0063FA60`) — only its final, fully-derived step is ported
//!   ([`armor_inactive_halved`](crate::systems::walls::armor_inactive_halved)).

use crate::container::EngineArray;

/// The lockstep checksum primitive. Taken straight from [`crate::checksum`] rather than
/// hopping through `systems::ammo`, and re-exported so `walls::adler32` resolves for
/// callers that hash [`WallData::walk_bytes`].
pub use crate::checksum::adler32;

// ============================================================================
// Object banding — `Objects::init` 0x0065EA80 [measured]
// ============================================================================

/// `obj_base[0]` — first index of the **unit** band inside `Objects::lists[who]`.
pub const UNIT_BAND_BASE: i32 = 0;
/// `obj_end[0]` — one past the last unit slot.
pub const UNIT_BAND_END: i32 = 2000;
/// `obj_base[1]` — first index of the **building** band.
pub const BUILD_BAND_BASE: i32 = 2000;
/// `obj_end[1]` — one past the last building slot.
pub const BUILD_BAND_END: i32 = 3000;
/// `obj_base[2]` — first index of the **wall** band.
pub const WALL_BAND_BASE: i32 = 3000;
/// `obj_end[2]` — one past the last wall slot. **Equal to [`WALL_BAND_BASE`]**: the band
/// has zero capacity, so no wall object can ever exist.
pub const WALL_BAND_END: i32 = 3000;

/// `Objects::lists` is `ObjectsArray[10]` — ten owner slots.
pub const OWNER_SLOTS: usize = 10;

/// `CheckSums::check_walls` and `check_builds` stop at leader `0x00E71AF0`, i.e. after
/// **8** leaders. `check_units` stops at `0x00E789DC`, i.e. after **9**. `Leaders::list`
/// is `Leader[10]`. The asymmetry is real and measured at the instruction level; owner
/// slots 8 and 9 are never walked by the wall or build channels.
pub const CHECK_WALLS_LEADERS: usize = 8;
/// See [`CHECK_WALLS_LEADERS`]. `check_units` covers one more leader slot.
pub const CHECK_UNITS_LEADERS: usize = 9;

/// `Leader` stride in the `Leaders::list` array, bytes. Only used to document the loop
/// bounds that produced [`CHECK_WALLS_LEADERS`].
pub const LEADER_STRIDE: u32 = 0x6EEC;
/// `?leaders@@3VLeaders@@A`.
pub const LEADERS_BASE_VA: u32 = 0x00E3A390;

// ============================================================================
// Flag bits — `SubObjectData::flags` at +0x08 [measured, from the predicate sites]
// ============================================================================

/// `flags & 1` — the object slot is live. `CheckSums::check_walls` gates on this, and it
/// is one of the three disjuncts of `Object::must_walk`.
pub const FLAG_ALIVE: u8 = 0x01;
/// `flags & 2` — `WallData::is_started` (`0x00472360`): the construction site exists on
/// the map (a citizen has laid the foundation).
pub const FLAG_STARTED: u8 = 0x02;
/// `flags & 4` — `WallData::is_active` (`0x00472350`): construction is **complete** and
/// the building is operating.
pub const FLAG_ACTIVE: u8 = 0x04;
/// `flags & 0x20` — selects the "belongs to a city" arm of `Wall::update_hits` and
/// suppresses the completion chime in `Wall::do_construct`. UNDERIVED name.
pub const FLAG_IN_CITY: u8 = 0x20;

// ============================================================================
// `WallData::build_masks` bits at +0x60 [measured, from Wall::process / do_construct]
// ============================================================================

/// Half of a two-phase "seen recently" toggle cleared by `Wall::process` every 32 frames.
pub const MASK_SEEN_A: i16 = 0x0010;
/// The other half. `Wall::process`: `if (m & 0x10) m &= !0x10; else m &= !0x20;`
pub const MASK_SEEN_B: i16 = 0x0020;
/// Latched by `Wall::process` when `helpers != 0` at the moment it resets the counter —
/// i.e. "at least one citizen worked on me last frame".
pub const MASK_HAD_HELPERS: i16 = 0x0400;
/// Set by the *first* `Wall::do_construct` call in a frame, cleared unconditionally by
/// `Wall::process`. Guards the once-per-frame `BuildData::recharging` bump.
pub const MASK_WORKED_THIS_FRAME: i16 = 0x0800;

/// `Object` `Coord` obfuscation key. `SubObjectData::x_internal` / `y_internal` are stored
/// XORed with this; `z_internal` is not. The checksum hashes the **obfuscated** words, so
/// a port that stores plain coordinates desyncs on identical logical state.
pub const COORD_XOR: u32 = 0x0006_3637;

/// One tile, in world/fine units.
pub const WORLD_UNITS_PER_TILE: i32 = 192;

// ============================================================================
// The walked state
// ============================================================================

/// The simulation state of one `Wall` — equivalently, the `Wall` sub-object of every
/// building. Field offsets are the engine's, from `schema/pdb-types.json`.
///
/// Only fields inside a walked range are modelled. `ptype` is stored as the resolved
/// `ObjectType::type` (`ObjectType + 4`) because that is the only thing the walker reads
/// out of the type pointer, plus a presence flag for the null case.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct WallState {
    // ---- SubObjectData -------------------------------------------------
    /// `+0x08` `flags`
    pub flags: u8,
    /// `+0x09` `who` — owner slot, 0..=9.
    pub who: u8,
    /// `+0x0A` `o` — index within the owner's object list.
    pub o: i16,
    /// `+0x0C` `z_internal` — a `Coord`, **not** XOR-obfuscated.
    pub z: i32,
    /// `+0x10` `x_internal` — a `Coord`, stored XORed with [`COORD_XOR`].
    pub x_obf: u32,
    /// `+0x14` `y_internal` — a `Coord`, stored XORed with [`COORD_XOR`].
    pub y_obf: u32,
    /// `+0x18` `ptype`. `None` models the null pointer; `Some(t)` carries
    /// `ObjectType::type`, the only field the walker dereferences.
    pub ptype: Option<i32>,

    // ---- ObjectData, the +0x20..+0x42 block ----------------------------
    /// `+0x20` `myhits` — full hit points after every multiplier. Written by
    /// `Wall::update_hits`.
    pub myhits: i32,
    /// `+0x24` `damage` — accumulated damage. Health is `construct_hits - damage`.
    pub damage: i32,
    /// `+0x28`
    pub inside_down: i16,
    /// `+0x2A`
    pub up: i16,
    /// `+0x2C`
    pub down: i16,
    /// `+0x2E`
    pub down_who: i16,
    /// `+0x30`
    pub uid: u16,
    /// `+0x32` — third disjunct of `Object::must_walk`.
    pub hold_frames: u16,
    /// `+0x34`
    pub near_o: i16,
    /// `+0x36`
    pub near_who: i16,
    /// `+0x38`
    pub healing: i16,
    /// `+0x3A`
    pub infiltrated: i8,
    /// `+0x3B`
    pub damage_frac: i8,
    /// `+0x3C` `mylos`
    pub mylos: i8,
    /// `+0x3D` `targeted` — decays by integer /4 every 8 frames in `Wall::process`.
    pub targeted: i8,
    /// `+0x3E`
    pub inside_down_who: i8,
    /// `+0x3F`
    pub up_who: i8,
    /// `+0x40`
    pub visible: i8,
    /// `+0x41`
    pub launch_frames: i8,
    /// `+0x44` `launching : SimpleArray<int>*`. Walked as a presence byte followed by
    /// `SimpleArray<int>::walk_data` (`0x00473120`) when non-null.
    pub launching: Option<EngineArray<i32>>,

    // ---- WallData, the +0x48..+0x66 block -------------------------------
    /// `+0x48` `job_counter` — construction work done, compared against `constr_time`.
    pub job_counter: u32,
    /// `+0x4C` `job_counter_2` — a second accumulator advanced by the same amount.
    pub job_counter_2: u32,
    /// `+0x50` `constr_time` — total work required. Written by
    /// `Wall::update_construct_time` (`0x0063D560`).
    pub constr_time: u32,
    /// `+0x54` `construct_hits` — the **effective** maximum hit points, ramped by
    /// construction progress. See [`progress_hits`].
    pub construct_hits: i32,
    /// `+0x58` `gpiece` — the chosen art piece. Presentation-flavoured, **checksummed**.
    pub gpiece: i32,
    /// `+0x5C` `frame_started`
    pub frame_started: i32,
    /// `+0x60` `build_masks` — see the `MASK_*` constants.
    pub build_masks: i16,
    /// `+0x62` `ever_seen` — a **per-player bitmask**, `1 << owner`, of who has laid eyes
    /// on this object (`Wall::process` sets a bit then calls `Wall::check_ever_seen(1)`).
    pub ever_seen: u8,
    /// `+0x63` `ever_seen_completed`
    pub ever_seen_completed: u8,
    /// `+0x64` `helpers` — citizens that contributed **this frame**. Incremented by
    /// `Wall::do_construct`, reset to 0 by `Wall::process`. See [`WallState::process`].
    pub helpers: u8,
    /// `+0x65` `demolition`
    pub demolition: u8,
}

impl WallState {
    /// Deobfuscated `x` coordinate, in world/fine units.
    #[inline]
    pub fn x(&self) -> i32 {
        (self.x_obf ^ COORD_XOR) as i32
    }
    /// Deobfuscated `y` coordinate, in world/fine units.
    #[inline]
    pub fn y(&self) -> i32 {
        (self.y_obf ^ COORD_XOR) as i32
    }
    /// Store a plain `x`, obfuscating it the way the engine does.
    #[inline]
    pub fn set_x(&mut self, x: i32) {
        self.x_obf = (x as u32) ^ COORD_XOR;
    }
    /// Store a plain `y`, obfuscating it the way the engine does.
    #[inline]
    pub fn set_y(&mut self, y: i32) {
        self.y_obf = (y as u32) ^ COORD_XOR;
    }

    /// `WallData::is_started` `0x00472360`.
    #[inline]
    pub fn is_started(&self) -> bool {
        self.flags & FLAG_STARTED != 0
    }
    /// `WallData::is_active` `0x00472350` — construction complete.
    #[inline]
    pub fn is_active(&self) -> bool {
        self.flags & FLAG_ACTIVE != 0
    }
    /// `flags & 1`, the gate `CheckSums::check_walls` applies before walking a slot.
    #[inline]
    pub fn is_alive(&self) -> bool {
        self.flags & FLAG_ALIVE != 0
    }

    /// `Object::must_walk` `0x00647930` \[measured\].
    ///
    /// ```text
    /// (dw->input == 0) && (ptype != 0 || (flags & 1) || hold_frames != 0)
    /// ```
    ///
    /// `input` is 0 for a `CheckSum` walk and for a save; it is non-zero only when
    /// *loading*. The byte this returns is itself emitted into the stream by every one of
    /// the three `walk_data` levels.
    #[inline]
    pub fn must_walk(&self, input_mode: bool) -> bool {
        !input_mode && (self.ptype.is_some() || self.is_alive() || self.hold_frames != 0)
    }

    /// `WallData::hits(int cur)` `0x00642BB0` — `cur ? myhits : construct_hits`.
    #[inline]
    pub fn hits(&self, cur: bool) -> i32 {
        if cur {
            self.myhits
        } else {
            self.construct_hits
        }
    }

    /// Health remaining. `Wall::update_hits` destroys the object when this reaches 0.
    #[inline]
    pub fn hits_left(&self) -> i32 {
        self.construct_hits - self.damage
    }

    /// The destruction test at the tail of `Wall::update_hits` `0x0063F0D0` \[measured\]:
    /// `construct_hits <= damage && (short)o >= 0`.
    #[inline]
    pub fn is_destroyed(&self) -> bool {
        self.construct_hits <= self.damage && self.o >= 0
    }

    /// The exact byte stream `WallData::walk_data` hands to `DataWalk::walk_function`,
    /// in order. For a live object with no `launching` array this is **88 bytes**.
    ///
    /// Chain: `WallData::walk_data` `0x00642510` → `Object::walk_data` `0x00647830` →
    /// `SubObject::walk_data` `0x006621D0`. `walk_test` emits nothing for a `CheckSum`.
    pub fn walk_bytes(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(88);
        let must = self.must_walk(false);

        // ---- SubObject::walk_data 0x006621D0 ----------------------------
        // walk_test(name)                                   -> 0 bytes
        b.push(self.flags); //  walk(+0x08, +0x09)
        b.push(must as u8); //  must_walk emits its own result byte
        if must {
            // walk(+0x09, +0x18) -- 15 bytes
            b.push(self.who);
            b.extend_from_slice(&self.o.to_le_bytes());
            b.extend_from_slice(&self.z.to_le_bytes());
            b.extend_from_slice(&self.x_obf.to_le_bytes());
            b.extend_from_slice(&self.y_obf.to_le_bytes());
            // `t = ptype ? *(int*)(ptype + 4) : 0` then walk(&t, &t + 4)
            b.extend_from_slice(&self.ptype.unwrap_or(0).to_le_bytes());
        }

        // ---- Object::walk_data 0x00647830 -------------------------------
        // walk_test(name)                                   -> 0 bytes
        b.push(must as u8); //  vt[+0x78] = Object::must_walk
        if must {
            // walk(+0x20, +0x42) -- 34 bytes
            b.extend_from_slice(&self.myhits.to_le_bytes());
            b.extend_from_slice(&self.damage.to_le_bytes());
            b.extend_from_slice(&self.inside_down.to_le_bytes());
            b.extend_from_slice(&self.up.to_le_bytes());
            b.extend_from_slice(&self.down.to_le_bytes());
            b.extend_from_slice(&self.down_who.to_le_bytes());
            b.extend_from_slice(&self.uid.to_le_bytes());
            b.extend_from_slice(&self.hold_frames.to_le_bytes());
            b.extend_from_slice(&self.near_o.to_le_bytes());
            b.extend_from_slice(&self.near_who.to_le_bytes());
            b.extend_from_slice(&self.healing.to_le_bytes());
            b.push(self.infiltrated as u8);
            b.push(self.damage_frac as u8);
            b.push(self.mylos as u8);
            b.push(self.targeted as u8);
            b.push(self.inside_down_who as u8);
            b.push(self.up_who as u8);
            b.push(self.visible as u8);
            b.push(self.launch_frames as u8);
        }
        // `if (dw->input == 0) { b = (launching != 0); walk(&b, &b + 1); }`
        b.push(self.launching.is_some() as u8);
        if let Some(l) = &self.launching {
            // SimpleArray<int>::walk_data 0x00473120. Engine-faithful `Array<T>` walks
            // emit length/capacity/grow/flags before the elements, so this field uses
            // `crate::container::EngineArray` rather than a plain Vec.
            b.extend_from_slice(&simple_array_walk_bytes(l));
        }

        // ---- WallData::walk_data 0x00642510 -----------------------------
        // walk_test(name)                                   -> 0 bytes
        b.push(must as u8); //  guarded-devirtualised Object::must_walk
        if must {
            // walk(+0x48, +0x66) -- 30 bytes
            b.extend_from_slice(&self.job_counter.to_le_bytes());
            b.extend_from_slice(&self.job_counter_2.to_le_bytes());
            b.extend_from_slice(&self.constr_time.to_le_bytes());
            b.extend_from_slice(&self.construct_hits.to_le_bytes());
            b.extend_from_slice(&self.gpiece.to_le_bytes());
            b.extend_from_slice(&self.frame_started.to_le_bytes());
            b.extend_from_slice(&self.build_masks.to_le_bytes());
            b.push(self.ever_seen);
            b.push(self.ever_seen_completed);
            b.push(self.helpers);
            b.push(self.demolition);
        }
        b
    }
}

/// Byte length of one `WallData::walk_data` emission for a live object with no
/// `launching` array: `2 + 19` (SubObject) `+ 1 + 34 + 1` (Object) `+ 1 + 30` (WallData).
pub const WALK_BYTES_LIVE: usize = 88;
/// Byte length when `must_walk` is false: three result bytes plus `flags` plus the
/// `launching` presence byte.
pub const WALK_BYTES_DEAD: usize = 5;

/// `SimpleArray<int>::walk_data` `0x00473120` on the checksum/write path.
///
/// The length is always emitted. For a non-empty array it is followed by capacity,
/// growth hint, `flags & !0x40`, and the elements. Capacity and growth hint are part of
/// lockstep state, which is why [`WallState::launching`] stores an [`EngineArray`].
fn simple_array_walk_bytes(elems: &EngineArray<i32>) -> Vec<u8> {
    let (len, size, increment, flags) = elems.checksum_header();
    let mut b = Vec::with_capacity(4 + if len == 0 { 0 } else { 7 + elems.len() * 4 });
    b.extend_from_slice(&len.to_le_bytes());
    if len == 0 {
        return b;
    }
    b.extend_from_slice(&size.to_le_bytes());
    b.extend_from_slice(&increment.to_le_bytes());
    b.push(flags & !0x40);
    for e in elems.as_slice() {
        b.extend_from_slice(&e.to_le_bytes());
    }
    b
}

// ============================================================================
// The channel
// ============================================================================

/// One owner's wall band, plus the high-water mark the channel loop reads.
#[derive(Clone, Debug, Default)]
pub struct OwnerBand {
    /// `Objects::lists[who].list` restricted to the wall band. Index `i` here is engine
    /// index `WALL_BAND_BASE + i`.
    pub slots: Vec<WallState>,
    /// `Objects::wall_mark[who]`. `Objects::clear` sets it to [`WALL_BAND_BASE`] and
    /// nothing in `.text` ever advances it.
    pub wall_mark: i32,
}

impl OwnerBand {
    /// The band as `Objects::clear` leaves it: empty, mark parked at the base.
    pub fn empty() -> Self {
        OwnerBand {
            slots: Vec::new(),
            wall_mark: WALL_BAND_BASE,
        }
    }
}

/// Everything `CheckSums::check_walls` reads.
#[derive(Clone, Debug)]
pub struct WallChannel {
    /// `Objects::valid` at `Objects + 0x1F4`. The whole channel is skipped when zero.
    pub objects_valid: bool,
    /// `leaders.list[i].leader_flags & 1` for each of the ten slots.
    pub leader_active: [bool; OWNER_SLOTS],
    /// Per-owner wall band.
    pub bands: [OwnerBand; OWNER_SLOTS],
}

impl Default for WallChannel {
    fn default() -> Self {
        Self::retail_default()
    }
}

impl WallChannel {
    /// A canonical empty channel fixture: valid, every leader gate enabled, and every wall
    /// band empty because band 2 has zero capacity. Enabling the leader gates makes
    /// synthetic bands observable in tests; it is not a claim that every retail leader
    /// slot is active. [`WallChannel::checksum`] over this returns `1`.
    pub fn retail_default() -> Self {
        WallChannel {
            objects_valid: true,
            leader_active: [true; OWNER_SLOTS],
            bands: std::array::from_fn(|_| OwnerBand::empty()),
        }
    }

    /// `CheckSums::check_walls` `0x00937360`, reproduced instruction for instruction
    /// \[measured\].
    ///
    /// ```text
    /// if (objects.valid == 0) return;
    /// esi = obj_base[2];                              // 3000
    /// for (i = 0; i < 8; i++) {                       // NOT 10, NOT 9
    ///     if (!(leaders.list[i].leader_flags & 1)) continue;
    ///     for (o = obj_base[2]; o < objects.wall_mark[i]; o++) {
    ///         w = objects.lists[i].list[o]->vt[+0xB0]();     // get_wallc, "return this"
    ///         if (w->flags & 1) w->vt[+0x7C](checksum);      // WallData::walk_data
    ///     }
    /// }
    /// ```
    ///
    /// `check_all` (`0x00936560`) seeds `accum = 1` and zeroes `size` immediately before
    /// the call, then adds the resulting `accum` into a running diagnostic sum. Each of
    /// the 15 channels therefore restarts adler-32 at 1; the command path serializes all
    /// 15 values plus their total (see `docs/mechanics/COVERAGE.md` §1.1).
    pub fn checksum(&self) -> u32 {
        let mut a: u32 = 1;
        if !self.objects_valid {
            return a;
        }
        for i in 0..CHECK_WALLS_LEADERS {
            if !self.leader_active[i] {
                continue;
            }
            let band = &self.bands[i];
            let mut o = WALL_BAND_BASE;
            while o < band.wall_mark {
                let idx = (o - WALL_BAND_BASE) as usize;
                if let Some(w) = band.slots.get(idx) {
                    if w.is_alive() {
                        a = adler32(a, &w.walk_bytes());
                    }
                }
                o += 1;
            }
        }
        a
    }

    /// Total bytes the channel would feed to adler-32 — `CheckSum::size` at `+0x14`,
    /// which `check_all` also zeroes per channel. Useful as a cheap divergence probe.
    pub fn walked_size(&self) -> u32 {
        let mut n = 0u32;
        if !self.objects_valid {
            return 0;
        }
        for i in 0..CHECK_WALLS_LEADERS {
            if !self.leader_active[i] {
                continue;
            }
            let band = &self.bands[i];
            for o in WALL_BAND_BASE..band.wall_mark {
                if let Some(w) = band.slots.get((o - WALL_BAND_BASE) as usize) {
                    if w.is_alive() {
                        n += w.walk_bytes().len() as u32;
                    }
                }
            }
        }
        n
    }
}

// ============================================================================
// Construction — `Wall::do_construct` 0x006434D0, called from `Unit::do_build`
// ============================================================================

/// What one citizen's construction tick did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConstructOutcome {
    /// `!is_started()` and the site is unbuildable: the engine calls `Object::die(1)`.
    SiteRejected,
    /// Work was credited; construction continues. Returns 0 in the engine.
    Progressed { credited: u32 },
    /// `job_counter >= constr_time`: `Wall::activate(0, 1, 1)` runs. Returns 1.
    Completed { credited: u32 },
    /// The object is already `is_active()`; `do_construct` does nothing and returns 0.
    AlreadyActive,
}

/// Site-validity codes accepted by `Wall::do_construct` from
/// `BuildTypeData::blocked_site` (`0x00636A50`) \[measured, the literal comparisons\].
///
/// `0x2A` is accepted only conditionally — see [`do_construct`].
pub const BLOCKED_SITE_OK: &[i32] = &[0x00, 0x27, 0x28, 0x29, 0x2B];
/// The conditional acceptance code: allowed when the object already belongs to a city
/// (`BuildData::city >= 0`) and
/// `CityData::num_wonders(1) <= 1 + LeaderData::has_tribe_bonus(7)`.
pub const BLOCKED_SITE_CONDITIONAL: i32 = 0x2A;

/// `Wall::do_construct(int amount)` `0x006434D0` \[measured\], the work half.
///
/// The engine, verbatim in structure:
///
/// ```text
/// if (ai_speed > 1)       amount *= ai_speed;          // GameAccess::ai_speed [0x00C061C0]
/// if (!is_started())      { site check; start and continue, or die and return 0; }
/// if (is_active())        return 0;
/// amount /= (helpers + 1);                             // <-- diminishing returns
/// if (!(build_masks & 0x800)) { recharging++; build_masks |= 0x800; }
/// helpers++;
/// if (amount < 1) amount = 1;
/// job_counter_2 += amount;
/// job_counter   += amount;
/// if (job_counter >= construct_time(0)) { activate(0,1,1); return 1; }
/// return 0;
/// ```
///
/// **The diminishing-returns divisor reads `helpers` *before* incrementing it**, and
/// `Wall::process` zeroes `helpers` at the end of every frame. So within one frame the
/// first citizen to reach the site contributes `amount / 1`, the second `amount / 2`, the
/// third `amount / 3`, … each flooring at 1. That is the whole multi-builder rule, and it
/// is per-frame, not per-building-lifetime.
///
/// `ai_speed` is the `GameAccess::ai_speed` cheat/debug multiplier; pass 1 for normal play.
pub fn do_construct(
    st: &mut WallState,
    amount: i32,
    ai_speed: i32,
    site_code: Option<i32>,
    site_conditional_ok: bool,
) -> ConstructOutcome {
    let mut amount = amount;
    if ai_speed > 1 {
        amount = ai_speed.wrapping_mul(amount);
    }

    if !st.is_started() {
        let code = site_code.unwrap_or(-1);
        let ok = if code == BLOCKED_SITE_CONDITIONAL {
            site_conditional_ok
        } else {
            BLOCKED_SITE_OK.contains(&code)
        };
        if ok {
            st.flags |= FLAG_STARTED;
            // Build::start(1) falls through into the inactive progress body.
        } else {
            return ConstructOutcome::SiteRejected;
        }
    }

    if st.is_active() {
        return ConstructOutcome::AlreadyActive;
    }

    amount /= i32::from(st.helpers) + 1;
    if st.build_masks & MASK_WORKED_THIS_FRAME == 0 {
        // `BuildData::recharging` (+0x7A) bump lives in the build lane; the mask latch is
        // the part this layer owns.
        st.build_masks |= MASK_WORKED_THIS_FRAME;
    }
    st.helpers = st.helpers.wrapping_add(1);
    if amount < 1 {
        amount = 1;
    }
    let credited = amount as u32;
    st.job_counter_2 = st.job_counter_2.wrapping_add(credited);
    st.job_counter = st.job_counter.wrapping_add(credited);

    if st.job_counter >= st.constr_time {
        st.flags |= FLAG_ACTIVE;
        ConstructOutcome::Completed { credited }
    } else {
        ConstructOutcome::Progressed { credited }
    }
}

/// The UI "nearly finished" threshold in `Wall::do_construct` \[measured\]: the engine
/// compares `(f32)job_counter / (f32)constr_time` against `0.84f` before and after the
/// increment and sets a global flag on the upward crossing. Presentation only, but it is
/// the one float on the construction path and it is recorded so nobody re-derives it.
pub const NEARLY_DONE_FRACTION: f32 = 0.84;

/// Did this tick cross the [`NEARLY_DONE_FRACTION`] boundary? Reproduces the engine's
/// `f32` comparison exactly (both operands are `i32`-to-`f32` conversions).
pub fn crossed_nearly_done(before: u32, after: u32, total: u32) -> bool {
    if total == 0 {
        return false;
    }
    let t = total as i32 as f32;
    let a = after as i32 as f32 / t;
    let b = before as i32 as f32 / t;
    a > NEARLY_DONE_FRACTION && b <= NEARLY_DONE_FRACTION
}

// ============================================================================
// Hit points — `Wall::update_hits` 0x0063F0D0
// ============================================================================

/// The construction-progress hit-point ramp at the tail of `Wall::update_hits`
/// `0x0063F0D0` \[measured\]. **Pure integer** — there is no float anywhere in it.
///
/// ```text
/// if (!is_active() && job_counter < construct_time(0)) {
///     a = max(job_counter      >> 5, 1);
///     b = max(construct_time() >> 5, 1);
///     if (!is_wonder())  hp = max(full * a / b, 1);
///     else               hp = (full + 1)/2 + max((full/2) * a / b, 1);
/// }
/// ```
///
/// The `>> 5` prescale is not cosmetic: it is what keeps `full * a` from overflowing
/// `int` on a 5,000-HP city with a 600-frame job time, and it quantises the ramp into
/// 32-work-unit steps. Reproduce the shifts, not an equivalent rational.
///
/// Wonders (`BuildData::is_wonder`, `Build` vtable `+0x2C` → `0x00472320`) start at half
/// health and ramp the other half — a plain `Wall` returns 0 from that slot, so the
/// `is_wonder` arm is unreachable for a true wall.
pub fn progress_hits(full_hits: i32, job_counter: u32, constr_time: u32, is_wonder: bool) -> i32 {
    if job_counter >= constr_time {
        return full_hits;
    }
    let a = core::cmp::max((job_counter >> 5) as i32, 1);
    let b = core::cmp::max((constr_time >> 5) as i32, 1);
    if !is_wonder {
        core::cmp::max(full_hits.wrapping_mul(a) / b, 1)
    } else {
        let half = core::cmp::max((full_hits / 2).wrapping_mul(a) / b, 1);
        (full_hits + 1) / 2 + half
    }
}

/// Apply `Wall::update_hits`'s two stores \[measured\]: `myhits` gets the fully multiplied
/// value, `construct_hits` gets the progress-ramped one, and the object dies when
/// `construct_hits <= damage`.
///
/// `full_hits` is the output of the ~12-step percent-bonus chain that this module does
/// **not** derive (see the module header); pass the value your rules layer computed.
/// Returns `true` if the object should be destroyed this call.
pub fn update_hits(st: &mut WallState, full_hits: i32, is_wonder: bool) -> bool {
    st.myhits = full_hits;
    let hp = if st.is_active() {
        full_hits
    } else {
        progress_hits(full_hits, st.job_counter, st.constr_time, is_wonder)
    };
    st.construct_hits = hp;
    st.is_destroyed()
}

/// `BuildData::hits(int)` `0x0062E740` — **the one float in the whole cone** \[measured\].
///
/// While a building is razing itself (its queue head is `DISBAND` `0x29A` or
/// `DEPOPULATE` `0x286`), its effective hit points interpolate down:
///
/// ```text
/// h = (int)(((float)(total - done) * (float)h) / (float)total);
/// if (h < 1) h = 1;
/// ```
///
/// Three `i32 -> f32` conversions, one multiply, one divide, one truncating convert back.
/// The inputs are all walked integers and the result is **not stored**, so no float enters
/// the checksum — but the truncation is observable through subsequent damage arithmetic,
/// so it is reproduced here in `f32` rather than rationalised into integers.
pub fn razing_hits(hits: i32, done: i32, total: i32) -> i32 {
    if total == 0 {
        return hits;
    }
    let v = (((total - done) as f32) * (hits as f32)) / (total as f32);
    let v = v as i32;
    if v < 1 {
        1
    } else {
        v
    }
}

/// Tail of `WallData::armor` `0x0063FA60` \[measured\]: whatever the accumulated armor is,
/// **an object that is not yet `is_active()` has half of it**, integer-divided.
///
/// The rest of that function adds `Constants + 0xC78` when a friendly `THESENATOR`
/// (`0x161`) is in range, which is a rules lookup this module does not own.
#[inline]
pub fn armor_inactive_halved(armor: i32, is_active: bool) -> i32 {
    if is_active {
        armor
    } else {
        armor / 2
    }
}

// ============================================================================
// Footprint — `WallData::tile_corner` 0x00643440 / `covers_tile` 0x006439B0
// ============================================================================

/// World/fine units to tile index.
///
/// The engine does `tile_table[(coord ^ COORD_XOR) >> 6]`, where `tile_table`
/// (`[0x00CAE5FC]`) is a runtime lookup built by the world layer. `>> 6` yields
/// thirds-of-a-tile (192 / 64 = 3), so the table is a divide-by-three. Modelled here as
/// floor division by 192 \[inference\]; the round trip `tile(t * 192) == t` and
/// `tile(t * 192 + 96) == t` both hold, which is what `tile_corner` relies on.
#[inline]
pub fn world_to_tile(coord: i32) -> i32 {
    coord.div_euclid(WORLD_UNITS_PER_TILE)
}

/// `WallData::tile_corner(TCoord*, TCoord*)` `0x00643440` \[measured\].
///
/// ```text
/// sx = tile(x) * 192;  if (x_size & 1) sx += 96;
/// sy = tile(y) * 192;  if (y_size & 1) sy += 96;
/// *corner_x = tile(sx) - (x_size >> 1);
/// *corner_y = tile(sy) - (y_size >> 1);
/// ```
///
/// Note the half-tile nudge is a no-op under a floor table (`tile(192t + 96) == t`); it is
/// reproduced anyway because the real table is a runtime artifact and this lane did not
/// read it. `x_size`/`y_size` come from `ObjectTypeData + 0x234` / `+ 0x238`.
pub fn tile_corner(x: i32, y: i32, x_size: i32, y_size: i32) -> (i32, i32) {
    let mut sx = world_to_tile(x) * WORLD_UNITS_PER_TILE;
    if x_size & 1 != 0 {
        sx += WORLD_UNITS_PER_TILE / 2;
    }
    let mut sy = world_to_tile(y) * WORLD_UNITS_PER_TILE;
    if y_size & 1 != 0 {
        sy += WORLD_UNITS_PER_TILE / 2;
    }
    (
        world_to_tile(sx) - (x_size >> 1),
        world_to_tile(sy) - (y_size >> 1),
    )
}

/// `WallData::covers_tile(TCoord, TCoord)` `0x006439B0` \[measured\] — the footprint test
/// that answers "does this object block that tile", i.e. how a wall or building occludes
/// pathing and projectile lanes at tile granularity.
pub fn covers_tile(x: i32, y: i32, x_size: i32, y_size: i32, tx: i32, ty: i32) -> bool {
    let (cx, cy) = tile_corner(x, y, x_size, y_size);
    tx >= cx && tx < cx + x_size && ty >= cy && ty < cy + y_size
}

// ============================================================================
// The per-frame tick — `Wall::process` 0x00640450
// ============================================================================

/// Side effects `Wall::process` requests from layers this module does not own.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProcessEffects {
    /// `Wall::check_ever_seen(0)` fired (the every-8-frames visibility refresh).
    pub check_ever_seen_periodic: bool,
    /// The `(frame + o) % 32 == 0` slot came up — the AI helper-demand scan and the
    /// unstarted-oil-platform check run here. Not ported; surfaced as a flag.
    pub slow_slot_32: bool,
    /// The `(frame + o) % 16 == 0` slot came up — the unfriendly-territory check runs
    /// here, which can destroy an unstarted building. Not ported; surfaced as a flag.
    pub territory_slot_16: bool,
}

/// The checksummed `WallData` scalars that the Build-band `Wall::process` prefix owns.
///
/// A [`BuildData`](crate::systems::production::BuildData) carries these exact fields, but
/// keeping this small value separate prevents the empty retail Wall band from becoming a
/// second, divergent owner for a building's base-class state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildWallPrefixState {
    /// `ObjectData::targeted` at `+0x3D`.
    pub targeted: i8,
    /// `WallData::ever_seen` at `+0x62`.
    pub ever_seen: u8,
    /// `WallData::ever_seen_completed` at `+0x63`.
    pub ever_seen_completed: u8,
    /// `WallData::build_masks` at `+0x60`.
    pub build_masks: u16,
    /// `WallData::helpers` at `+0x64`.
    pub helpers: u8,
}

/// The first source-dependent child reached by the Build-band `Wall::process` prefix.
///
/// `Complete` means the base-class call returned and `Build::process` itself is next. The
/// other variants are hard ordering boundaries: callers must not publish later helper or
/// Build writes until that child has an authoritative host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildWallPrefixBoundary {
    /// `Wall::check_ever_seen(0)` at the owner-phased eight-frame gate.
    CheckEverSeenPeriodic,
    /// The inactive-only helper-demand/oil-platform cone in the 32-frame slot.
    InactiveSlowSlot,
    /// The 16-frame unfriendly-territory cone.
    TerritorySlot,
    /// `BuildTypeData::is_dock()` at `0x00640A3A`.  The receipt carries the exact
    /// decoded position and `WData::who` read which precede the call.
    TerritoryIsDock,
    /// The dock-only `WallData::tile_corner` / `BuildTypeData::check_enemy_adjacent`
    /// branch.  Golden Village and Market type authority proves this branch unreachable.
    TerritoryDockFootprint,
    /// `LeaderData::is_ally(territory_owner)` at `0x00640AB2`.
    TerritoryIsAlly,
    /// `Game::war_allowed()` at `0x006408AE`, reached only when rush rules are non-zero.
    TerritoryWarAllowed,
    /// `Wall::process` returned without reaching an unresolved child.
    Complete,
}

/// One immutable golden Build identity admitted to the frame-8/16/24 periodic no-op lane.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BuildWallPeriodicIdentity {
    pub row: usize,
    pub who: u8,
    pub o: i16,
    pub uid: u16,
    pub type_index: i32,
}

/// Owner-0 visibility slots reached before the supported frame-31 golden checkpoint.
pub const GOLDEN_BUILD_WALL_PERIODIC_FRAMES: [i32; 3] = [8, 16, 24];

/// Territory slots reached by the two golden starting Builds before frame 32.
pub const GOLDEN_BUILD_WALL_TERRITORY_FRAMES: [i32; 3] = [15, 16, 31];

/// Reinstalled Game::everyone_mask authority for the two supported golden starting Builds.
///
/// The scalar is written once by the retail loop at `0x0058A2DB..0x0058A301`, from the
/// eight `LeaderData::leader_flags & 1` bytes. It is not part of the compact Sim save, so
/// replay setup must re-bind this sidecar after load. A missing or tampered authority simply
/// leaves `check_ever_seen` at its existing fail-closed boundary.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BuildWallPeriodicAuthority {
    pub revision: u64,
    pub composition_digest: u32,
    pub everyone_mask: u8,
    /// The only owner-0 periodic slots admitted before the frame-31 golden checkpoint.
    pub frames: [i32; 3],
    pub builds: [BuildWallPeriodicIdentity; 2],
}

/// Stable structural digest verified at every authority lookup.
pub fn build_wall_periodic_authority_digest(authority: &BuildWallPeriodicAuthority) -> u32 {
    let mut image = b"don-build-wall-periodic-authority-v1".to_vec();
    image.extend_from_slice(&authority.revision.to_le_bytes());
    image.push(authority.everyone_mask);
    for frame in authority.frames {
        image.extend_from_slice(&frame.to_le_bytes());
    }
    for build in authority.builds {
        image.extend_from_slice(&(build.row as u64).to_le_bytes());
        image.push(build.who);
        image.extend_from_slice(&build.o.to_le_bytes());
        image.extend_from_slice(&build.uid.to_le_bytes());
        image.extend_from_slice(&build.type_index.to_le_bytes());
    }
    adler32(1, &image)
}

impl BuildWallPeriodicAuthority {
    /// Return the exact Game mask only for an identity-complete, still-started bound Build.
    pub fn everyone_mask_for(
        &self,
        frame: i32,
        row: usize,
        build: &crate::systems::production::BuildData,
        type_index: Option<i32>,
    ) -> Option<u8> {
        if self.revision == 0
            || self.composition_digest == 0
            || self.everyone_mask == 0
            || self.frames != GOLDEN_BUILD_WALL_PERIODIC_FRAMES
            || !self.frames.contains(&frame)
            || self.composition_digest != build_wall_periodic_authority_digest(self)
            || !build.is_started()
            || !build.is_active()
        {
            return None;
        }
        self.builds
            .iter()
            .find(|identity| {
                identity.row == row
                    && identity.who == build.who
                    && identity.o == build.object_id()
                    && identity.uid == build.uid
                    && Some(identity.type_index) == type_index
            })
            .map(|_| self.everyone_mask)
    }
}

/// Immutable content identity admitted to the golden territory lane.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BuildWallTerritoryIdentity {
    pub row: usize,
    pub who: u8,
    pub o: i16,
    pub uid: u16,
    pub type_index: i32,
    /// Source-evaluated `BuildTypeData::is_dock()`.
    pub is_dock: bool,
}

/// Replay/content authority for the exact golden territory calls.
///
/// The mutable inputs are deliberately absent. `disable_building_attrition`, object
/// coordinates and `WData::who` are read from their canonical live Sim owners on the frame
/// where retail reads them. This sidecar admits only the replay-carried `rush_rules` scalar,
/// the two stable Build identities, and the source-evaluated type child.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BuildWallTerritoryAuthority {
    pub revision: u64,
    pub composition_digest: u32,
    pub rush_rules: u8,
    pub frames: [i32; 3],
    pub builds: [BuildWallTerritoryIdentity; 2],
}

/// Stable structural digest verified before exposing any territory fact.
pub fn build_wall_territory_authority_digest(authority: &BuildWallTerritoryAuthority) -> u32 {
    let mut image = b"don-build-wall-territory-authority-v1".to_vec();
    image.extend_from_slice(&authority.revision.to_le_bytes());
    image.push(authority.rush_rules);
    for frame in authority.frames {
        image.extend_from_slice(&frame.to_le_bytes());
    }
    for build in authority.builds {
        image.extend_from_slice(&(build.row as u64).to_le_bytes());
        image.push(build.who);
        image.extend_from_slice(&build.o.to_le_bytes());
        image.extend_from_slice(&build.uid.to_le_bytes());
        image.extend_from_slice(&build.type_index.to_le_bytes());
        image.push(u8::from(build.is_dock));
    }
    adler32(1, &image)
}

/// Source-bound static facts for one reached territory slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildWallTerritoryStaticFacts {
    pub type_index: i32,
    pub rush_rules: u8,
    pub is_dock: bool,
}

impl BuildWallTerritoryAuthority {
    /// Admit facts only for the exact live golden identity and one of its native slots.
    pub fn facts_for(
        &self,
        frame: i32,
        row: usize,
        build: &crate::systems::production::BuildData,
        type_index: Option<i32>,
        live_rush_rules: u8,
    ) -> Option<BuildWallTerritoryStaticFacts> {
        if self.revision == 0
            || self.composition_digest == 0
            || self.frames != GOLDEN_BUILD_WALL_TERRITORY_FRAMES
            || !self.frames.contains(&frame)
            || self.rush_rules != live_rush_rules
            || self.composition_digest != build_wall_territory_authority_digest(self)
            || !build.is_started()
            || !build.is_active()
        {
            return None;
        }
        self.builds
            .iter()
            .find(|identity| {
                identity.row == row
                    && identity.who == build.who
                    && identity.o == build.object_id()
                    && identity.uid == build.uid
                    && Some(identity.type_index) == type_index
            })
            .map(|identity| BuildWallTerritoryStaticFacts {
                type_index: identity.type_index,
                rush_rules: self.rush_rules,
                is_dock: identity.is_dock,
            })
    }
}

/// Live, instruction-ordered inputs to the golden territory cone.
///
/// `fine_position`, `wcoord`, and `territory_owner` are `None` on Market frame 31 because
/// retail returns at the second phase gate before reading any of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildWallTerritoryInput {
    pub static_facts: BuildWallTerritoryStaticFacts,
    pub building_attrition_disabled: i32,
    pub fine_position: Option<(i32, i32)>,
    pub wcoord: Option<(i32, i32)>,
    pub territory_owner: Option<i32>,
}

/// Why the source-bound territory cone returned or stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildWallTerritoryExit {
    BuildingAttritionDisabled,
    Phase32AlternateNoop,
    IsDockBoundary,
    DockFootprintBoundary,
    UnownedTerritory,
    FriendlyTerritory,
    IsAllyBoundary,
    WarAllowedBoundary,
}

/// Exact read-only envelope consumed after the helper latch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildWallTerritoryReceipt {
    pub building_attrition_disabled: i32,
    pub rush_rules: u8,
    pub phase32_due: bool,
    pub type_index: i32,
    pub fine_position: Option<(i32, i32)>,
    pub wcoord: Option<(i32, i32)>,
    pub territory_owner: Option<i32>,
    pub is_dock: Option<bool>,
    pub exit: BuildWallTerritoryExit,
}

/// Auditable result of the canonical Build-band `Wall::process` prefix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildWallPrefixReceipt {
    pub frame: i32,
    pub who: u8,
    pub o: i16,
    pub phase: i32,
    /// Cadence facts are reported even when an earlier child prevents reaching them.
    pub periodic_due: bool,
    pub slow_slot_32_due: bool,
    pub territory_slot_16_due: bool,
    pub active: bool,
    /// The periodic child returned at its exact pre-footprint no-op gate.
    pub periodic_child_completed_noop: bool,
    /// Present only after a source-bound territory authority was consumed.
    pub territory: Option<BuildWallTerritoryReceipt>,
    pub before: BuildWallPrefixState,
    pub after: BuildWallPrefixState,
    pub boundary: BuildWallPrefixBoundary,
}

/// Execute the maximal locally owned prefix of `Wall::process` for one canonical Build.
///
/// This differs deliberately from [`WallState::process`], whose detached `ProcessEffects`
/// API models every local bookkeeping write at once. Retail interleaves source-dependent
/// children between those writes. This function preserves that order and stops at the first
/// child, so a periodic visibility refresh cannot be silently stepped over to clear helpers,
/// and an inactive slow-slot child cannot be stepped over to the territory cone.
pub fn process_build_wall_prefix(
    state: &mut BuildWallPrefixState,
    frame: i32,
    who: u8,
    o: i16,
    active: bool,
    periodic_everyone_mask: Option<u8>,
) -> BuildWallPrefixReceipt {
    process_build_wall_prefix_with_territory(
        state,
        frame,
        who,
        o,
        active,
        periodic_everyone_mask,
        None,
    )
}

/// Execute the same native prefix with an optional source-bound territory envelope.
pub fn process_build_wall_prefix_with_territory(
    state: &mut BuildWallPrefixState,
    frame: i32,
    who: u8,
    o: i16,
    active: bool,
    periodic_everyone_mask: Option<u8>,
    territory_input: Option<BuildWallTerritoryInput>,
) -> BuildWallPrefixReceipt {
    let before = *state;
    let phase = frame.wrapping_add(i32::from(o));
    let periodic_due = frame != 0 && ((frame as u32 & 7) as u8) == who;
    let slow_slot_32_due = phase.rem_euclid(32) == 0;
    let territory_slot_16_due = phase.rem_euclid(16) == 0;

    let mut periodic_child_completed_noop = false;
    let boundary = if periodic_due {
        state.targeted = div4_toward_zero(state.targeted);
        // `Wall::check_ever_seen(0)` 0x0063CE70 returns at 0x0063CFD4..0x0063CFE1
        // before `get_corner`, World reads, Leader notifications, and update_local_seen when
        // both bytes already contain Game::everyone_mask. The optional mask is a
        // revision-bound Game-field authority, never a value inferred from the Build.
        periodic_child_completed_noop = periodic_everyone_mask.is_some_and(|mask| {
            state.ever_seen & mask == mask && state.ever_seen_completed & mask == mask
        });
        if !periodic_child_completed_noop {
            return BuildWallPrefixReceipt {
                frame,
                who,
                o,
                phase,
                periodic_due,
                slow_slot_32_due,
                territory_slot_16_due,
                active,
                periodic_child_completed_noop,
                territory: None,
                before,
                after: *state,
                boundary: BuildWallPrefixBoundary::CheckEverSeenPeriodic,
            };
        }
        // Exact no-op return: resume at Wall::process 0x00640499, before the phase32 gate.
        if slow_slot_32_due {
            if state.build_masks & MASK_SEEN_A as u16 != 0 {
                state.build_masks &= !(MASK_SEEN_A as u16);
            } else {
                state.build_masks &= !(MASK_SEEN_B as u16);
            }
            if !active {
                return BuildWallPrefixReceipt {
                    frame,
                    who,
                    o,
                    phase,
                    periodic_due,
                    slow_slot_32_due,
                    territory_slot_16_due,
                    active,
                    periodic_child_completed_noop,
                    territory: None,
                    before,
                    after: *state,
                    boundary: BuildWallPrefixBoundary::InactiveSlowSlot,
                };
            }
        }

        if state.helpers == 0 {
            state.build_masks &= !(MASK_HAD_HELPERS as u16);
        } else {
            state.helpers = 0;
            state.build_masks |= MASK_HAD_HELPERS as u16;
        }
        state.build_masks &= !(MASK_WORKED_THIS_FRAME as u16);

        if territory_slot_16_due {
            BuildWallPrefixBoundary::TerritorySlot
        } else {
            BuildWallPrefixBoundary::Complete
        }
    } else {
        if slow_slot_32_due {
            if state.build_masks & MASK_SEEN_A as u16 != 0 {
                state.build_masks &= !(MASK_SEEN_A as u16);
            } else {
                state.build_masks &= !(MASK_SEEN_B as u16);
            }
            // The exact Build vtable resolves `is_active` to `flags & 4`. Active Village
            // and Market objects jump directly to the helper latch; only inactive objects
            // enter the source-dependent helper-demand/oil cone.
            if !active {
                return BuildWallPrefixReceipt {
                    frame,
                    who,
                    o,
                    phase,
                    periodic_due,
                    slow_slot_32_due,
                    territory_slot_16_due,
                    active,
                    periodic_child_completed_noop,
                    territory: None,
                    before,
                    after: *state,
                    boundary: BuildWallPrefixBoundary::InactiveSlowSlot,
                };
            }
        }

        if state.helpers == 0 {
            state.build_masks &= !(MASK_HAD_HELPERS as u16);
        } else {
            state.helpers = 0;
            state.build_masks |= MASK_HAD_HELPERS as u16;
        }
        state.build_masks &= !(MASK_WORKED_THIS_FRAME as u16);

        if territory_slot_16_due {
            BuildWallPrefixBoundary::TerritorySlot
        } else {
            BuildWallPrefixBoundary::Complete
        }
    };

    let (boundary, territory) = if boundary == BuildWallPrefixBoundary::TerritorySlot {
        resolve_build_wall_territory(phase, who, territory_input)
    } else {
        (boundary, None)
    };

    BuildWallPrefixReceipt {
        frame,
        who,
        o,
        phase,
        periodic_due,
        slow_slot_32_due,
        territory_slot_16_due,
        active,
        periodic_child_completed_noop,
        territory,
        before,
        after: *state,
        boundary,
    }
}

fn resolve_build_wall_territory(
    phase: i32,
    who: u8,
    input: Option<BuildWallTerritoryInput>,
) -> (BuildWallPrefixBoundary, Option<BuildWallTerritoryReceipt>) {
    let Some(input) = input else {
        return (BuildWallPrefixBoundary::TerritorySlot, None);
    };
    let facts = input.static_facts;
    let phase32_due = phase.rem_euclid(32) == 0;
    let finish = |boundary, exit, is_dock| {
        (
            boundary,
            Some(BuildWallTerritoryReceipt {
                building_attrition_disabled: input.building_attrition_disabled,
                rush_rules: facts.rush_rules,
                phase32_due,
                type_index: facts.type_index,
                fine_position: input.fine_position,
                wcoord: input.wcoord,
                territory_owner: input.territory_owner,
                is_dock,
                exit,
            }),
        )
    };

    // 0x0064088B..0x0064089C: scenario authority can disable this entire cone.
    if input.building_attrition_disabled != 0 {
        return finish(
            BuildWallPrefixBoundary::Complete,
            BuildWallTerritoryExit::BuildingAttritionDisabled,
            None,
        );
    }
    // 0x006408A2..0x006408B5: non-zero rush rules call Game::war_allowed first.
    if facts.rush_rules != 0 {
        return finish(
            BuildWallPrefixBoundary::TerritoryWarAllowed,
            BuildWallTerritoryExit::WarAllowedBoundary,
            None,
        );
    }
    // 0x006409E3..0x006409F0: standard rules inspect unfriendly territory only on the
    // alternating 32-frame occurrence. Market frame 31 closes here without later reads.
    if !phase32_due {
        return finish(
            BuildWallPrefixBoundary::Complete,
            BuildWallTerritoryExit::Phase32AlternateNoop,
            None,
        );
    }

    // Retail reads both coordinates and WData::who before the is_dock virtual call.
    if input.fine_position.is_none() || input.wcoord.is_none() || input.territory_owner.is_none() {
        return finish(
            BuildWallPrefixBoundary::TerritoryIsDock,
            BuildWallTerritoryExit::IsDockBoundary,
            None,
        );
    }
    if facts.is_dock {
        return finish(
            BuildWallPrefixBoundary::TerritoryDockFootprint,
            BuildWallTerritoryExit::DockFootprintBoundary,
            Some(true),
        );
    }
    let owner = input
        .territory_owner
        .expect("the complete live territory probe was checked above");
    if owner < 0 {
        return finish(
            BuildWallPrefixBoundary::Complete,
            BuildWallTerritoryExit::UnownedTerritory,
            Some(false),
        );
    }
    if owner == i32::from(who) {
        return finish(
            BuildWallPrefixBoundary::Complete,
            BuildWallTerritoryExit::FriendlyTerritory,
            Some(false),
        );
    }
    finish(
        BuildWallPrefixBoundary::TerritoryIsAlly,
        BuildWallTerritoryExit::IsAllyBoundary,
        Some(false),
    )
}

/// Resume exactly at `0x00640862` after the helper-latch stores have been published.
/// A receipt which did not reach that boundary is returned unchanged.
pub fn resume_build_wall_territory(
    mut receipt: BuildWallPrefixReceipt,
    input: BuildWallTerritoryInput,
) -> BuildWallPrefixReceipt {
    if receipt.boundary != BuildWallPrefixBoundary::TerritorySlot
        || !receipt.territory_slot_16_due
        || receipt.territory.is_some()
    {
        return receipt;
    }
    let (boundary, territory) =
        resolve_build_wall_territory(receipt.phase, receipt.who, Some(input));
    receipt.boundary = boundary;
    receipt.territory = territory;
    receipt
}

impl WallState {
    /// `Wall::process` `0x00640450` \[measured\], the deterministic bookkeeping half.
    ///
    /// Frame phasing, exactly as the engine computes it:
    ///
    /// | gate | condition | what it drives |
    /// |---|---|---|
    /// | targeted decay | `frame != 0 && (frame & 7) == who` | `targeted /= 4`, `check_ever_seen(0)` |
    /// | slow slot | `(frame + o) % 32 == 0` | seen-toggle clear, AI helper demand |
    /// | oil-platform check | `(frame + o) % 128 == 0` | destroy an unsited `OILPLATFORM` |
    /// | territory check | `(frame + o) % 16 == 0` | enemy-territory destruction / warning |
    ///
    /// Two consequences worth carrying into a scheduler. First, **the decay phase is
    /// keyed on `who`, not on `o`** — and `frame & 7` only ever reaches 7, so owner slots
    /// 8 and 9 never decay `targeted` at all. Second, the helper reset is *unconditional*
    /// and runs after every gate:
    ///
    /// ```text
    /// if (helpers == 0) build_masks &= ~0x400;
    /// else            { helpers = 0; build_masks |= 0x400; }
    /// build_masks &= ~0x800;
    /// ```
    ///
    /// So `helpers` is a **single-frame accumulator**: `Wall::do_construct` increments it
    /// as citizens arrive, `Wall::process` consumes it into the `MASK_HAD_HELPERS` latch
    /// and clears it. Any port that treats `helpers` as a persistent builder count will
    /// get the diminishing-returns divisor wrong from frame two onward.
    pub fn process(&mut self, frame: i32) -> ProcessEffects {
        let mut fx = ProcessEffects::default();

        // `if (frame != 0 && (u8)frame & 7 == who)`
        if frame != 0 && ((frame as u32 & 7) as u8) == self.who {
            self.targeted = div4_toward_zero(self.targeted);
            fx.check_ever_seen_periodic = true;
        }

        let phase = frame.wrapping_add(i32::from(self.o));
        if phase.rem_euclid(32) == 0 {
            fx.slow_slot_32 = true;
            // Two-phase "seen recently" toggle.
            if self.build_masks & MASK_SEEN_A != 0 {
                self.build_masks &= !MASK_SEEN_A;
            } else {
                self.build_masks &= !MASK_SEEN_B;
            }
        }

        // Unconditional helper latch + reset.
        if self.helpers == 0 {
            self.build_masks &= !MASK_HAD_HELPERS;
        } else {
            self.helpers = 0;
            self.build_masks |= MASK_HAD_HELPERS;
        }
        self.build_masks &= !MASK_WORKED_THIS_FRAME;

        if phase.rem_euclid(16) == 0 {
            fx.territory_slot_16 = true;
        }
        fx
    }

    /// `Wall::process`'s per-player sighting latch \[measured\]:
    /// `if (!(ever_seen & (1 << owner)) && (leaders[owner].leader_flags & 0x2000))`
    /// then set the bit and call `Wall::check_ever_seen(1)`.
    ///
    /// Returns `true` when the bit was newly set, i.e. when the engine would fire the
    /// expensive refresh.
    pub fn mark_ever_seen_by(&mut self, owner: u8, owner_leader_flag_2000: bool) -> bool {
        // Retail shifts a 32-bit 1 by `(owner & 31)`, then truncates the result to the
        // byte field at +0x62. Slots 8..31 therefore produce zero; they do not alias bit 7.
        let bit = (1u32 << (owner & 0x1F)) as u8;
        if self.ever_seen & bit == 0 && owner_leader_flag_2000 {
            self.ever_seen |= bit;
            bit != 0
        } else {
            false
        }
    }
}

/// C's `x / 4` for a signed `char`, as MSVC emits it:
/// `(x + ((x >> 31) & 3)) >> 2` — round toward zero, not floor.
#[inline]
pub fn div4_toward_zero(x: i8) -> i8 {
    let v = x as i32;
    (((v + ((v >> 31) & 3)) >> 2) & 0xFF) as u8 as i8
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn live() -> WallState {
        WallState {
            flags: FLAG_ALIVE | FLAG_STARTED,
            who: 3,
            o: 3007,
            z: 0,
            ptype: Some(417),
            myhits: 800,
            construct_hits: 400,
            constr_time: 600,
            job_counter: 300,
            ..Default::default()
        }
    }

    // ---- banding ---------------------------------------------------------

    #[test]
    fn wall_band_has_zero_capacity() {
        // Objects::init 0x0065EA80: obj_base = {0,2000,3000}, obj_end = {2000,3000,3000}.
        assert_eq!(WALL_BAND_BASE, WALL_BAND_END);
        assert_eq!(BUILD_BAND_END, WALL_BAND_BASE);
        assert_eq!(UNIT_BAND_END, BUILD_BAND_BASE);
    }

    #[test]
    fn leader_loop_bounds_match_the_disassembly() {
        // check_walls / check_builds: `cmp edx, 0xE71AF0`  -> 8 leaders.
        // check_units:                `cmp ecx, 0xE789DC`  -> 9 leaders.
        assert_eq!(
            LEADERS_BASE_VA + CHECK_WALLS_LEADERS as u32 * LEADER_STRIDE,
            0x00E7_1AF0
        );
        assert_eq!(
            LEADERS_BASE_VA + CHECK_UNITS_LEADERS as u32 * LEADER_STRIDE,
            0x00E7_89DC
        );
        assert!(CHECK_WALLS_LEADERS < OWNER_SLOTS);
    }

    // ---- the channel -----------------------------------------------------

    #[test]
    fn retail_walls_channel_is_exactly_one() {
        // adler32 seeded to 1 over zero bytes. This is the value `check_all` adds for
        // the walls channel in every real game.
        assert_eq!(WallChannel::retail_default().checksum(), 1);
        assert_eq!(WallChannel::retail_default().walked_size(), 0);
    }

    #[test]
    fn invalid_objects_short_circuits() {
        let mut c = WallChannel::retail_default();
        c.objects_valid = false;
        c.bands[0].slots.push(live());
        c.bands[0].wall_mark = WALL_BAND_BASE + 1;
        assert_eq!(c.checksum(), 1);
    }

    #[test]
    fn walk_is_88_bytes_for_a_live_object() {
        assert_eq!(live().walk_bytes().len(), WALK_BYTES_LIVE);
        assert_eq!(2 + 19 + 1 + 34 + 1 + 1 + 30, WALK_BYTES_LIVE);
    }

    #[test]
    fn walk_is_5_bytes_when_must_walk_is_false() {
        let mut w = WallState::default();
        w.ptype = None;
        w.flags = 0;
        w.hold_frames = 0;
        assert!(!w.must_walk(false));
        assert_eq!(w.walk_bytes().len(), WALK_BYTES_DEAD);
        // flags, must, must, launching-present, must
        assert_eq!(w.walk_bytes(), vec![0, 0, 0, 0, 0]);
    }

    #[test]
    fn launching_array_walks_its_engine_header_and_elements() {
        let mut empty = live();
        empty.launching = Some(EngineArray::new());
        let empty_bytes = empty.walk_bytes();
        assert_eq!(
            empty_bytes.len(),
            WALK_BYTES_LIVE + 4,
            "an empty array still walks length"
        );

        let mut launching = EngineArray::new();
        launching.set_flags(0x43);
        launching.add(0x1122_3344);
        launching.add(-7);
        let mut w = live();
        w.launching = Some(launching);
        let bytes = w.walk_bytes();
        // The array follows the presence byte, which is byte 56 for a live object.
        let start = 57;
        assert_eq!(&bytes[start..start + 4], &2i32.to_le_bytes());
        assert_eq!(&bytes[start + 4..start + 8], &5i32.to_le_bytes());
        assert_eq!(&bytes[start + 8..start + 10], &(-1i16).to_le_bytes());
        assert_eq!(
            bytes[start + 10],
            0x03,
            "SimpleArray clears transient flag 0x40"
        );
        assert_eq!(
            &bytes[start + 11..start + 15],
            &0x1122_3344i32.to_le_bytes()
        );
        assert_eq!(&bytes[start + 15..start + 19], &(-7i32).to_le_bytes());
        assert_eq!(bytes.len(), WALK_BYTES_LIVE + 19);
    }

    #[test]
    fn must_walk_is_the_three_way_disjunction() {
        let mut w = WallState::default();
        assert!(!w.must_walk(false));
        w.ptype = Some(1);
        assert!(w.must_walk(false));
        w.ptype = None;
        w.flags = FLAG_ALIVE;
        assert!(w.must_walk(false));
        w.flags = 0;
        w.hold_frames = 1;
        assert!(w.must_walk(false));
        // `input != 0` (the load path) forces it false regardless.
        assert!(!w.must_walk(true));
    }

    #[test]
    fn walk_emits_obfuscated_coordinates() {
        let mut w = live();
        w.set_x(12_345);
        let b = w.walk_bytes();
        // flags(1) + must(1) + who(1) + o(2) + z(4) = offset 9 for x.
        let x = u32::from_le_bytes([b[9], b[10], b[11], b[12]]);
        assert_eq!(x, 12_345u32 ^ COORD_XOR);
        assert_ne!(x, 12_345);
        assert_eq!(w.x(), 12_345);
    }

    #[test]
    fn channel_only_walks_slots_below_the_mark() {
        let mut c = WallChannel::retail_default();
        c.bands[0].slots = vec![live(), live(), live()];
        c.bands[0].wall_mark = WALL_BAND_BASE; // as Objects::clear leaves it
        assert_eq!(c.checksum(), 1, "mark parked at base -> nothing walked");
        c.bands[0].wall_mark = WALL_BAND_BASE + 2;
        assert_eq!(c.walked_size(), 2 * WALK_BYTES_LIVE as u32);
    }

    #[test]
    fn channel_skips_dead_slots_and_inactive_leaders() {
        let mut c = WallChannel::retail_default();
        let mut dead = live();
        dead.flags = 0;
        dead.ptype = None;
        c.bands[0].slots = vec![dead, live()];
        c.bands[0].wall_mark = WALL_BAND_BASE + 2;
        assert_eq!(c.walked_size(), WALK_BYTES_LIVE as u32);

        c.leader_active[0] = false;
        assert_eq!(c.checksum(), 1);
    }

    #[test]
    fn channel_ignores_owner_slots_8_and_9() {
        let mut c = WallChannel::retail_default();
        for i in 8..OWNER_SLOTS {
            c.bands[i].slots = vec![live()];
            c.bands[i].wall_mark = WALL_BAND_BASE + 1;
        }
        assert_eq!(c.checksum(), 1, "slots 8 and 9 are past the loop bound");
        c.bands[7].slots = vec![live()];
        c.bands[7].wall_mark = WALL_BAND_BASE + 1;
        assert_ne!(c.checksum(), 1);
    }

    #[test]
    fn every_walked_field_moves_the_checksum() {
        let base = live();
        let b0 = adler32(1, &base.walk_bytes());
        let mut probes: Vec<WallState> = Vec::new();
        let mut m = |f: fn(&mut WallState)| {
            let mut w = base.clone();
            f(&mut w);
            probes.push(w);
        };
        m(|w| w.job_counter ^= 1);
        m(|w| w.job_counter_2 ^= 1);
        m(|w| w.constr_time ^= 1);
        m(|w| w.construct_hits ^= 1);
        m(|w| w.gpiece ^= 1);
        m(|w| w.frame_started ^= 1);
        m(|w| w.build_masks ^= 1);
        m(|w| w.ever_seen ^= 1);
        m(|w| w.ever_seen_completed ^= 1);
        m(|w| w.helpers ^= 1);
        m(|w| w.demolition ^= 1);
        m(|w| w.myhits ^= 1);
        m(|w| w.damage ^= 1);
        m(|w| w.targeted ^= 1);
        m(|w| w.x_obf ^= 1);
        m(|w| w.ptype = Some(999));
        for (i, p) in probes.iter().enumerate() {
            assert_ne!(adler32(1, &p.walk_bytes()), b0, "probe {i} did not perturb");
        }
    }

    #[test]
    fn gpiece_is_inside_the_checksum_but_render_gpiece_is_not() {
        // WallData::gpiece is +0x58, inside [+0x48, +0x66). WallOut::render_gpiece is
        // +0x68, outside. The struct has no render_gpiece field at all, by design.
        let mut a = live();
        let b = a.clone();
        a.gpiece = 7;
        assert_ne!(a.walk_bytes(), b.walk_bytes());
    }

    // ---- construction ----------------------------------------------------

    #[test]
    fn helpers_divide_the_construction_rate_within_one_frame() {
        let mut w = live();
        w.constr_time = 1_000_000;
        let mut credits = Vec::new();
        for _ in 0..4 {
            match do_construct(&mut w, 120, 1, None, false) {
                ConstructOutcome::Progressed { credited } => credits.push(credited),
                other => panic!("{other:?}"),
            }
        }
        // helpers is read before the increment: 120/1, 120/2, 120/3, 120/4.
        assert_eq!(credits, vec![120, 60, 40, 30]);
        assert_eq!(w.helpers, 4);
        assert_eq!(w.job_counter, 300 + 120 + 60 + 40 + 30);
        assert_eq!(w.job_counter_2, 120 + 60 + 40 + 30);
    }

    #[test]
    fn credited_work_floors_at_one() {
        let mut w = live();
        w.constr_time = 1_000_000;
        w.helpers = 200;
        match do_construct(&mut w, 3, 1, None, false) {
            ConstructOutcome::Progressed { credited } => assert_eq!(credited, 1),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn process_resets_helpers_so_the_divisor_restarts_each_frame() {
        let mut w = live();
        w.constr_time = 1_000_000;
        do_construct(&mut w, 100, 1, None, false);
        do_construct(&mut w, 100, 1, None, false);
        assert_eq!(w.helpers, 2);
        assert!(w.build_masks & MASK_WORKED_THIS_FRAME != 0);

        w.process(64); // any frame
        assert_eq!(w.helpers, 0);
        assert!(w.build_masks & MASK_HAD_HELPERS != 0);
        assert!(w.build_masks & MASK_WORKED_THIS_FRAME == 0);

        // Next frame the first builder is back to full rate.
        match do_construct(&mut w, 100, 1, None, false) {
            ConstructOutcome::Progressed { credited } => assert_eq!(credited, 100),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn had_helpers_latch_clears_on_an_idle_frame() {
        let mut w = live();
        w.build_masks = MASK_HAD_HELPERS;
        w.helpers = 0;
        w.process(1);
        assert!(w.build_masks & MASK_HAD_HELPERS == 0);
    }

    #[test]
    fn ai_speed_multiplies_before_the_helper_divide() {
        let mut w = live();
        w.constr_time = 1_000_000;
        w.helpers = 1;
        match do_construct(&mut w, 10, 4, None, false) {
            // (10 * 4) / (1 + 1) == 20
            ConstructOutcome::Progressed { credited } => assert_eq!(credited, 20),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn construction_completes_and_sets_active() {
        let mut w = live();
        w.constr_time = 310;
        assert!(matches!(
            do_construct(&mut w, 20, 1, None, false),
            ConstructOutcome::Completed { credited: 20 }
        ));
        assert!(w.is_active());
        assert!(matches!(
            do_construct(&mut w, 20, 1, None, false),
            ConstructOutcome::AlreadyActive
        ));
    }

    #[test]
    fn unstarted_site_codes() {
        for &c in BLOCKED_SITE_OK {
            let mut w = live();
            w.flags = FLAG_ALIVE;
            assert_eq!(
                do_construct(&mut w, 10, 1, Some(c), false),
                ConstructOutcome::Progressed { credited: 10 }
            );
            assert!(w.is_started());
            assert_eq!(w.job_counter, 310, "starting also credits first work");
        }
        let mut w = live();
        w.flags = FLAG_ALIVE;
        assert_eq!(
            do_construct(&mut w, 10, 1, Some(BLOCKED_SITE_CONDITIONAL), false),
            ConstructOutcome::SiteRejected
        );
        let mut w = live();
        w.flags = FLAG_ALIVE;
        assert_eq!(
            do_construct(&mut w, 10, 1, Some(BLOCKED_SITE_CONDITIONAL), true),
            ConstructOutcome::Progressed { credited: 10 }
        );
        let mut w = live();
        w.flags = FLAG_ALIVE;
        assert_eq!(
            do_construct(&mut w, 10, 1, Some(0x99), false),
            ConstructOutcome::SiteRejected
        );
    }

    #[test]
    fn nearly_done_crossing_is_edge_triggered() {
        assert!(crossed_nearly_done(830, 850, 1000));
        assert!(!crossed_nearly_done(850, 860, 1000));
        assert!(!crossed_nearly_done(100, 200, 1000));
        assert!(!crossed_nearly_done(0, 1, 0));
    }

    // ---- hit points ------------------------------------------------------

    #[test]
    fn progress_hits_ramps_with_the_shift_quantisation() {
        // full=1000, constr_time=600 -> b = max(600>>5,1) = 18
        // job=300 -> a = 300>>5 = 9 -> 1000*9/18 = 500
        assert_eq!(progress_hits(1000, 300, 600, false), 500);
        // job=31 -> a = 0 -> clamped to 1 -> 1000*1/18 = 55
        assert_eq!(progress_hits(1000, 31, 600, false), 55);
        // complete -> full
        assert_eq!(progress_hits(1000, 600, 600, false), 1000);
        assert_eq!(progress_hits(1000, 700, 600, false), 1000);
    }

    #[test]
    fn progress_hits_never_returns_zero() {
        assert_eq!(progress_hits(1, 1, 1_000_000, false), 1);
        assert_eq!(progress_hits(0, 1, 1_000_000, false), 1);
    }

    #[test]
    fn wonders_start_at_half_health() {
        // full=1000, job=0(->a=1), constr=600(->b=18): (1000+1)/2 + max(500*1/18,1)
        assert_eq!(progress_hits(1000, 0, 600, true), 500 + 27);
        // and a wonder is always at least half.
        assert!(progress_hits(1000, 0, 1_000_000, true) >= 500);
    }

    #[test]
    fn tiny_job_times_do_not_divide_by_zero() {
        assert_eq!(progress_hits(100, 0, 4, false), 100 / 1); // b clamps to 1
        assert_eq!(progress_hits(100, 0, 0, false), 100); // job >= constr -> full
    }

    #[test]
    fn update_hits_writes_both_fields_and_reports_death() {
        let mut w = live();
        w.flags = FLAG_ALIVE | FLAG_STARTED;
        w.constr_time = 600;
        w.job_counter = 300;
        w.damage = 0;
        assert!(!update_hits(&mut w, 1000, false));
        assert_eq!(w.myhits, 1000);
        assert_eq!(w.construct_hits, 500);
        assert_eq!(w.hits_left(), 500);
        assert_eq!(w.hits(true), 1000);
        assert_eq!(w.hits(false), 500);

        w.damage = 500;
        assert!(update_hits(&mut w, 1000, false), "damage >= construct_hits");
    }

    #[test]
    fn a_negative_object_index_cannot_die() {
        let mut w = live();
        w.o = -1;
        w.construct_hits = 0;
        w.damage = 10;
        assert!(!w.is_destroyed());
    }

    #[test]
    fn active_buildings_use_full_hits() {
        let mut w = live();
        w.flags |= FLAG_ACTIVE;
        w.job_counter = 0;
        update_hits(&mut w, 1234, false);
        assert_eq!(w.construct_hits, 1234);
    }

    #[test]
    fn razing_interpolation_matches_the_float_shape() {
        assert_eq!(razing_hits(1000, 0, 100), 1000);
        assert_eq!(razing_hits(1000, 50, 100), 500);
        assert_eq!(razing_hits(1000, 100, 100), 1, "floors at 1, never 0");
        assert_eq!(razing_hits(1000, 200, 100), 1);
        assert_eq!(razing_hits(7, 3, 7), 4); // (4*7)/7 = 4.0 -> 4
        assert_eq!(razing_hits(5, 0, 0), 5);
    }

    #[test]
    fn armor_is_halved_before_completion() {
        assert_eq!(armor_inactive_halved(7, true), 7);
        assert_eq!(armor_inactive_halved(7, false), 3);
        assert_eq!(armor_inactive_halved(1, false), 0);
    }

    // ---- footprint -------------------------------------------------------

    #[test]
    fn footprint_covers_exactly_the_size_box() {
        // A 3x3 building centred in tile (10, 10).
        let (x, y) = (
            10 * WORLD_UNITS_PER_TILE + 96,
            10 * WORLD_UNITS_PER_TILE + 96,
        );
        assert_eq!(tile_corner(x, y, 3, 3), (9, 9));
        for tx in 9..12 {
            for ty in 9..12 {
                assert!(covers_tile(x, y, 3, 3, tx, ty), "({tx},{ty})");
            }
        }
        assert!(!covers_tile(x, y, 3, 3, 8, 10));
        assert!(!covers_tile(x, y, 3, 3, 12, 10));
        assert!(!covers_tile(x, y, 3, 3, 10, 12));
    }

    #[test]
    fn even_sized_footprints_anchor_differently() {
        let x = 10 * WORLD_UNITS_PER_TILE;
        assert_eq!(tile_corner(x, x, 2, 2), (9, 9));
        assert_eq!(tile_corner(x, x, 4, 4), (8, 8));
        assert_eq!(tile_corner(x, x, 1, 1), (10, 10));
    }

    #[test]
    fn world_to_tile_floors_toward_negative_infinity() {
        assert_eq!(world_to_tile(0), 0);
        assert_eq!(world_to_tile(191), 0);
        assert_eq!(world_to_tile(192), 1);
        assert_eq!(world_to_tile(-1), -1);
        for t in 0..64 {
            assert_eq!(world_to_tile(t * WORLD_UNITS_PER_TILE), t);
            assert_eq!(world_to_tile(t * WORLD_UNITS_PER_TILE + 96), t);
        }
    }

    // ---- per-frame -------------------------------------------------------

    #[test]
    fn targeted_decays_only_on_the_owners_phase_frame() {
        let mut w = live(); // who = 3
        w.targeted = 100;
        for f in 1..=7i32 {
            if (f as u32 & 7) as u8 != 3 {
                w.process(f);
                assert_eq!(w.targeted, 100, "frame {f} should not decay");
            }
        }
        let fx = w.process(3);
        assert!(fx.check_ever_seen_periodic);
        assert_eq!(w.targeted, 25);
        w.process(11);
        assert_eq!(w.targeted, 6);
    }

    #[test]
    fn owner_slots_8_and_9_never_decay_targeted() {
        for who in 8..10u8 {
            let mut w = live();
            w.who = who;
            w.targeted = 100;
            for f in 1..200 {
                w.process(f);
            }
            assert_eq!(w.targeted, 100, "who={who}: frame & 7 never reaches {who}");
        }
    }

    #[test]
    fn targeted_decay_rounds_toward_zero() {
        assert_eq!(div4_toward_zero(7), 1);
        assert_eq!(div4_toward_zero(-7), -1); // C truncation, not floor(-1.75) = -2
        assert_eq!(div4_toward_zero(-1), 0);
        assert_eq!(div4_toward_zero(-4), -1);
        assert_eq!(div4_toward_zero(0), 0);
    }

    #[test]
    fn seen_toggle_alternates_on_the_32_frame_slot() {
        let mut w = live();
        w.o = 0;
        w.build_masks = MASK_SEEN_A | MASK_SEEN_B;
        let fx = w.process(32);
        assert!(fx.slow_slot_32);
        assert_eq!(w.build_masks & (MASK_SEEN_A | MASK_SEEN_B), MASK_SEEN_B);
        w.process(64);
        assert_eq!(w.build_masks & (MASK_SEEN_A | MASK_SEEN_B), 0);
    }

    #[test]
    fn frame_phase_is_offset_by_object_index() {
        let mut a = live();
        a.o = 0;
        let mut b = live();
        b.o = 5;
        assert!(a.process(32).slow_slot_32);
        assert!(!b.process(32).slow_slot_32);
        assert!(b.process(27).slow_slot_32);
    }

    #[test]
    fn territory_slot_fires_every_16_frames() {
        let mut w = live();
        w.o = 0;
        let hits: Vec<i32> = (0..48)
            .filter(|f| w.clone().process(*f).territory_slot_16)
            .collect();
        assert_eq!(hits, vec![0, 16, 32]);
    }

    #[test]
    fn build_prefix_periodic_child_stops_before_same_frame_slow_and_helper_writes() {
        let mut state = BuildWallPrefixState {
            targeted: -7,
            ever_seen: 0,
            ever_seen_completed: 0,
            build_masks: MASK_SEEN_A as u16 | MASK_SEEN_B as u16 | MASK_WORKED_THIS_FRAME as u16,
            helpers: 3,
        };

        // Village o2000: frame 16 is simultaneously owner0-periodic, slow32 and territory16.
        let receipt = process_build_wall_prefix(&mut state, 16, 0, 2000, true, None);

        assert!(receipt.periodic_due);
        assert!(receipt.slow_slot_32_due);
        assert!(receipt.territory_slot_16_due);
        assert_eq!(
            receipt.boundary,
            BuildWallPrefixBoundary::CheckEverSeenPeriodic
        );
        assert_eq!(state.targeted, -1);
        assert_eq!(
            state.helpers, 3,
            "helper reset is after the unresolved child"
        );
        assert_eq!(
            state.build_masks, receipt.before.build_masks,
            "slow toggle and helper masks are after the unresolved child"
        );
    }

    #[test]
    fn covered_periodic_child_returns_noop_and_resumes_the_wall_prefix() {
        let mut state = BuildWallPrefixState {
            targeted: 64,
            ever_seen: 1,
            ever_seen_completed: 1,
            build_masks: MASK_WORKED_THIS_FRAME as u16,
            helpers: 2,
        };

        // At frame 8 neither golden o2000 nor o2001 has a phase16/32 slot. Once the exact
        // everyone-mask gate returns, the helper latch is therefore the next write.
        let receipt = process_build_wall_prefix(&mut state, 8, 0, 2000, true, Some(1));

        assert!(receipt.periodic_due);
        assert!(receipt.periodic_child_completed_noop);
        assert_eq!(receipt.boundary, BuildWallPrefixBoundary::Complete);
        assert_eq!(state.targeted, 16);
        assert_eq!(state.helpers, 0);
        assert_ne!(state.build_masks & MASK_HAD_HELPERS as u16, 0);
        assert_eq!(state.build_masks & MASK_WORKED_THIS_FRAME as u16, 0);
    }

    #[test]
    fn village_frame16_noop_periodic_child_exposes_the_later_territory_boundary() {
        let mut state = BuildWallPrefixState {
            targeted: 0,
            ever_seen: 1,
            ever_seen_completed: 1,
            build_masks: MASK_SEEN_A as u16 | MASK_SEEN_B as u16,
            helpers: 0,
        };

        let receipt = process_build_wall_prefix(&mut state, 16, 0, 2000, true, Some(1));

        assert!(receipt.periodic_child_completed_noop);
        assert!(receipt.slow_slot_32_due);
        assert!(receipt.territory_slot_16_due);
        assert_eq!(receipt.boundary, BuildWallPrefixBoundary::TerritorySlot);
        assert_eq!(state.build_masks & MASK_SEEN_A as u16, 0);
        assert_ne!(state.build_masks & MASK_SEEN_B as u16, 0);
    }

    #[test]
    fn periodic_authority_is_identity_complete_and_self_hashing() {
        let mut build = crate::systems::production::BuildData {
            flags: FLAG_ALIVE | FLAG_STARTED | FLAG_ACTIVE,
            who: 0,
            uid: 9,
            ..Default::default()
        };
        build.set_object_id(2000);
        let mut authority = BuildWallPeriodicAuthority {
            revision: 7,
            composition_digest: 0,
            everyone_mask: 1,
            frames: [8, 16, 24],
            builds: [
                BuildWallPeriodicIdentity {
                    row: 0,
                    who: 0,
                    o: 2000,
                    uid: 9,
                    type_index: 414,
                },
                BuildWallPeriodicIdentity {
                    row: 1,
                    who: 0,
                    o: 2001,
                    uid: 10,
                    type_index: 436,
                },
            ],
        };
        authority.composition_digest = build_wall_periodic_authority_digest(&authority);
        assert_eq!(
            authority.everyone_mask_for(8, 0, &build, Some(414)),
            Some(1)
        );
        assert_eq!(authority.everyone_mask_for(32, 0, &build, Some(414)), None);

        authority.everyone_mask = 3;
        assert_eq!(authority.everyone_mask_for(8, 0, &build, Some(414)), None);
    }

    #[test]
    fn active_market_slow_slot_reaches_helper_latch_then_territory_boundary() {
        let mut state = BuildWallPrefixState {
            targeted: 64,
            ever_seen: 0,
            ever_seen_completed: 0,
            build_masks: MASK_SEEN_A as u16 | MASK_SEEN_B as u16 | MASK_WORKED_THIS_FRAME as u16,
            helpers: 2,
        };

        // Dutch Market o2001: frame 15 is phase 2016, divisible by both 32 and 16.
        let receipt = process_build_wall_prefix(&mut state, 15, 0, 2001, true, None);

        assert!(!receipt.periodic_due);
        assert!(receipt.slow_slot_32_due);
        assert!(receipt.territory_slot_16_due);
        assert_eq!(receipt.boundary, BuildWallPrefixBoundary::TerritorySlot);
        assert_eq!(state.targeted, 64);
        assert_eq!(state.helpers, 0);
        assert_eq!(state.build_masks & MASK_SEEN_A as u16, 0);
        assert_ne!(state.build_masks & MASK_SEEN_B as u16, 0);
        assert_ne!(state.build_masks & MASK_HAD_HELPERS as u16, 0);
        assert_eq!(state.build_masks & MASK_WORKED_THIS_FRAME as u16, 0);
    }

    #[test]
    fn inactive_slow_slot_stops_after_seen_toggle_before_helper_latch() {
        let mut state = BuildWallPrefixState {
            targeted: 0,
            ever_seen: 0,
            ever_seen_completed: 0,
            build_masks: MASK_SEEN_A as u16 | MASK_WORKED_THIS_FRAME as u16,
            helpers: 1,
        };

        let receipt = process_build_wall_prefix(&mut state, 15, 0, 2001, false, None);

        assert_eq!(receipt.boundary, BuildWallPrefixBoundary::InactiveSlowSlot);
        assert_eq!(state.build_masks & MASK_SEEN_A as u16, 0);
        assert_ne!(state.build_masks & MASK_WORKED_THIS_FRAME as u16, 0);
        assert_eq!(state.helpers, 1);
    }

    #[test]
    fn market_territory_only_slot_applies_helper_latch_before_stopping() {
        let mut state = BuildWallPrefixState {
            targeted: 0,
            ever_seen: 0,
            ever_seen_completed: 0,
            build_masks: MASK_SEEN_A as u16 | MASK_WORKED_THIS_FRAME as u16,
            helpers: 0,
        };

        // Market o2001: frame 31 is phase 2032, divisible by 16 but not by 32.
        let receipt = process_build_wall_prefix(&mut state, 31, 0, 2001, true, None);

        assert!(!receipt.slow_slot_32_due);
        assert!(receipt.territory_slot_16_due);
        assert_eq!(receipt.boundary, BuildWallPrefixBoundary::TerritorySlot);
        assert_ne!(state.build_masks & MASK_SEEN_A as u16, 0);
        assert_eq!(state.build_masks & MASK_WORKED_THIS_FRAME as u16, 0);
        assert_eq!(state.build_masks & MASK_HAD_HELPERS as u16, 0);
    }

    fn golden_territory_input(
        type_index: i32,
        building_attrition_disabled: i32,
        fine_position: Option<(i32, i32)>,
        wcoord: Option<(i32, i32)>,
        territory_owner: Option<i32>,
        is_dock: bool,
    ) -> BuildWallTerritoryInput {
        BuildWallTerritoryInput {
            static_facts: BuildWallTerritoryStaticFacts {
                type_index,
                rush_rules: 0,
                is_dock,
            },
            building_attrition_disabled,
            fine_position,
            wcoord,
            territory_owner,
        }
    }

    #[test]
    fn market_frame31_territory_returns_before_position_world_or_type_reads() {
        let mut state = BuildWallPrefixState {
            targeted: 0,
            ever_seen: 1,
            ever_seen_completed: 1,
            build_masks: MASK_WORKED_THIS_FRAME as u16,
            helpers: 0,
        };
        let receipt = process_build_wall_prefix_with_territory(
            &mut state,
            31,
            0,
            2001,
            true,
            None,
            Some(golden_territory_input(436, 0, None, None, None, false)),
        );

        assert_eq!(receipt.boundary, BuildWallPrefixBoundary::Complete);
        let territory = receipt.territory.expect("source-bound territory receipt");
        assert_eq!(territory.exit, BuildWallTerritoryExit::Phase32AlternateNoop);
        assert!(!territory.phase32_due);
        assert_eq!(territory.fine_position, None);
        assert_eq!(territory.wcoord, None);
        assert_eq!(territory.territory_owner, None);
        assert_eq!(territory.is_dock, None);
    }

    #[test]
    fn phase32_territory_stops_typed_at_is_dock_when_live_probe_is_missing() {
        let mut state = BuildWallPrefixState {
            targeted: 0,
            ever_seen: 1,
            ever_seen_completed: 1,
            build_masks: 0,
            helpers: 0,
        };
        let receipt = process_build_wall_prefix_with_territory(
            &mut state,
            15,
            0,
            2001,
            true,
            None,
            Some(golden_territory_input(436, 0, None, None, None, false)),
        );

        assert_eq!(receipt.boundary, BuildWallPrefixBoundary::TerritoryIsDock);
        assert_eq!(
            receipt.territory.expect("typed boundary").exit,
            BuildWallTerritoryExit::IsDockBoundary
        );
    }

    #[test]
    fn source_bound_non_dock_on_friendly_wdata_returns_wall_process() {
        let mut state = BuildWallPrefixState {
            targeted: 0,
            ever_seen: 1,
            ever_seen_completed: 1,
            build_masks: 0,
            helpers: 0,
        };
        let receipt = process_build_wall_prefix_with_territory(
            &mut state,
            16,
            0,
            2000,
            true,
            Some(1),
            Some(golden_territory_input(
                414,
                0,
                Some((12_288, 12_288)),
                Some((16, 16)),
                Some(0),
                false,
            )),
        );

        assert_eq!(receipt.boundary, BuildWallPrefixBoundary::Complete);
        let territory = receipt.territory.expect("source-bound territory receipt");
        assert_eq!(territory.exit, BuildWallTerritoryExit::FriendlyTerritory);
        assert_eq!(territory.is_dock, Some(false));
        assert_eq!(territory.territory_owner, Some(0));
    }

    #[test]
    fn territory_authority_is_self_hashed_and_identity_complete() {
        let mut build = crate::systems::production::BuildData {
            flags: crate::systems::production::flag::VALID
                | crate::systems::production::flag::STARTED
                | crate::systems::production::flag::ACTIVE,
            who: 0,
            uid: 9,
            ..Default::default()
        };
        build.set_object_id(2000);
        let mut authority = BuildWallTerritoryAuthority {
            revision: 4,
            composition_digest: 0,
            rush_rules: 0,
            frames: GOLDEN_BUILD_WALL_TERRITORY_FRAMES,
            builds: [
                BuildWallTerritoryIdentity {
                    row: 3,
                    who: 0,
                    o: 2000,
                    uid: 9,
                    type_index: 414,
                    is_dock: false,
                },
                BuildWallTerritoryIdentity {
                    row: 4,
                    who: 0,
                    o: 2001,
                    uid: 10,
                    type_index: 436,
                    is_dock: false,
                },
            ],
        };
        authority.composition_digest = build_wall_territory_authority_digest(&authority);
        assert_eq!(
            authority.facts_for(16, 3, &build, Some(414), 0),
            Some(BuildWallTerritoryStaticFacts {
                type_index: 414,
                rush_rules: 0,
                is_dock: false,
            })
        );
        authority.builds[0].uid ^= 1;
        assert_eq!(authority.facts_for(16, 3, &build, Some(414), 0), None);
    }

    #[test]
    fn ever_seen_is_a_per_player_bitmask() {
        let mut w = live();
        assert!(w.mark_ever_seen_by(2, true));
        assert_eq!(w.ever_seen, 0b100);
        assert!(!w.mark_ever_seen_by(2, true), "already set");
        assert!(!w.mark_ever_seen_by(3, false), "leader flag 0x2000 clear");
        assert_eq!(w.ever_seen, 0b100);
        assert!(w.mark_ever_seen_by(3, true));
        assert_eq!(w.ever_seen, 0b1100);
    }

    #[test]
    fn ever_seen_shift_truncates_slots_above_seven() {
        let mut w = live();
        assert!(!w.mark_ever_seen_by(8, true));
        assert!(!w.mark_ever_seen_by(31, true));
        assert_eq!(
            w.ever_seen, 0,
            "the 32-bit shift truncates to an 8-bit field"
        );
        assert!(
            w.mark_ever_seen_by(34, true),
            "the engine masks the shift count to five bits"
        );
        assert_eq!(w.ever_seen, 0b100);
    }

    // ---- adler-32 shared with the ammo channel ---------------------------

    #[test]
    fn adler_seed_is_one_per_channel() {
        assert_eq!(adler32(1, b""), 1);
        assert_eq!(adler32(1, b"abc"), 0x024D_0127);
    }
}
