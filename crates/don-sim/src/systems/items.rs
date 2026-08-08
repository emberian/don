//! Items — the `CheckSums::check_items` channel, and what an "item" actually is.
//!
//! # The answer to the basic question
//!
//! **An item is a goody box.** That is the whole class.
//!
//! `ron-data/itemrules.xml` declares exactly one `<ITEM>`, `Goody Box`, and the PDB's
//! `TypeIndex` enum agrees at the type level: `BASE_ITEMTYPES = 543`, `GOODY = 543`,
//! `END_ITEMTYPES = 544`, **`NUM_ITEMTYPES = 1`** [measured, `schema/pdb-types.json`
//! `enums.TypeIndex`; `ron-data/itemrules.xml`]. Every `Objects::init_item` call site in
//! the binary passes the literal `0x21F` (543).
//!
//! So `GameAccess::items : PtrArray<Item>` (`items` at `0x00C0A100`, `sizeof 28`) is the
//! map's **ruins / goody-box registry**, and `check_items` is the channel that keeps it in
//! lockstep. It is not treasure-in-transit, not tribute, not rare resources — rare
//! resources are `Good`/`GoodType` (`goods` at `0x00C0A0E0`, the *separate* `check_goods`
//! channel) and scenery is `Doober`.
//!
//! # Is the channel vestigial?
//!
//! No, but it is *small*. `Item` is 44 bytes, has **no `process`** (vtable slot 39
//! `+0x9C` on the `Item` vtable `0x00B44F34` is the folded empty stub `0x0041BFF0`), and
//! `Objects::process_all` never iterates `items`. Items are inert between the events that
//! touch them: a unit walking onto one, fog revealing one, or terrain repair relocating
//! one. Their whole mutable state is a position, a validity flag, and an 8-bit per-player
//! "ever seen" mask.
//!
//! What is *not* vestigial is the payout: `Unit::explore_goody` `0x005F9780` draws from
//! `GameAccess::game_random` — the **main simulation stream** — once per candidate
//! resource. Get the draw count or the candidate order wrong and the RNG stream desyncs
//! for everything downstream. That, plus the position bytes, is why items are a channel.
//!
//! # Provenance
//!
//! Everything below is `[measured]` against `ron-bin/riseofnations.exe`
//! (sha256 `30478a44…625079`) and `ron-bin/sbl/rise.pdb` on this Mac, by disassembly
//! (capstone) cross-checked against `re/decomp-all/`. **Nothing here has been executed on
//! the oracle**, so every behavioural claim in this module is **Tier C**: transcribed and
//! self-tested, never differentially tested against retail. Do not promote it.
//!
//! | thing | VA | source |
//! |---|---|---|
//! | `CheckSums::check_items` | `0x00937790` | checksums.cpp:0x47 |
//! | `Item::walk_data` | `0x00677150` | item.cpp:178 |
//! | `SubObject::walk_data` | `0x006621D0` | subobject.cpp |
//! | `SubObject::must_walk` | `0x006623A0` | subobject.cpp |
//! | `CheckSum::walk_function` | `0x00936FF0` | checksums.cpp:109 |
//! | `CheckSum::walk_test` | `0x0041BFE0` | **`ret 4` — a no-op** |
//! | `adler32` | `0x00A46830` | misc.cpp:464 |
//! | `Item::init` | `0x006770E0` | item.cpp:196 |
//! | `Item::close` | `0x00677050` | item.cpp:205 |
//! | `ItemData::is_seen` | `0x00677850` | item.cpp:15 |
//! | `ItemTypeData::snap_center` | `0x00677DF0` | itemtype.cpp:6 |
//! | `Objects::init_item` | `0x00653E00` | objects.cpp:4841 |
//! | `Objects::remove_item` | `0x0065B850` | objects.cpp:4866 |
//! | `ObjectsData::find_goody_at` (tile) | `0x0065C040` | objects.cpp:1984 |
//! | `ObjectsData::find_goody_at` (coord) | `0x0065B7C0` | objects.cpp:2015 |
//! | `World::clear_down` | `0x006B3AD0` | world.cpp:2727 |
//! | `Unit::explore_goody` | `0x005F9780` | unit.cpp:15116 |
//! | `Unit::find_goody_box` | `0x005F2540` | unit.cpp:19979 |
//! | `Terrain::move_goody` | `0x0084A3D0` | terrain.cpp:3719 |
//!
//! # The coordinate ladder, and `div_3_table`
//!
//! Every item address in the engine goes through the global `int *div_3_table`
//! (`0x00CAE5FC`). Its contents are `t[i] = i / 3` — **unsigned** division — filled at
//! `0x00681DB0` (`*(uint*)(base + i*4) = i / 3`). Combined with the shift at each call
//! site that gives two exact conversions:
//!
//! * `div_3_table[c >> 6] == c / 192` — the **TCoord** (tile), 192 fine units.
//! * `div_3_table[c >> 8] == c / 768` — the **WCoord** cell, 4 tiles.
//!
//! Items live on the WCoord grid: `ItemTypeData::snap_center` is literally
//! `x = wx * 0x300 + 0x180`, the centre of a 768-unit cell. The 28-byte `WData` array at
//! `World + 0x134` is **WCoord-indexed** (`wy * World::xs + wx`), not tile-indexed —
//! `World::xs`/`ys` are WCoord dimensions.
//!
//! The buffer behind `div_3_table` is `malloc(n*0x30*4)` with the published pointer
//! biased `+n*0x18` ints into it, so negative indices are *addressable* but **never
//! initialised**. Negative coordinates therefore read uninitialised heap in retail; we
//! treat them as out of contract.

use crate::rng::Random;

// ---------------------------------------------------------------------------
// Type-level constants
// ---------------------------------------------------------------------------

/// `TypeIndex::GOODY` / `BASE_ITEMTYPES` [measured, PDB `TypeIndex` enum].
pub const TYPE_GOODY: i32 = 543;
/// `TypeIndex::END_ITEMTYPES` [measured].
pub const END_ITEMTYPES: i32 = 544;
/// `TypeIndex::NUM_ITEMTYPES` — there is exactly one item type in the shipped game.
pub const NUM_ITEMTYPES: usize = 1;

/// XOR key applied to every `Object`/`SubObject` `Coord` **in memory**
/// (`SubObject::init` `0x00662300`, `Item::close` `0x00677050`).
///
/// The checksum walks raw memory, so the *obfuscated* words are what gets hashed. We
/// store them obfuscated for exactly that reason.
pub const COORD_XOR: i32 = 0x0006_3637;

/// Fine units per WCoord cell: 4 tiles x 192 [measured, `ItemTypeData::snap_center`].
pub const WCELL_SPAN: i32 = 0x300;
/// Half a WCoord cell — the snap offset an item is placed at.
pub const WCELL_CENTER: i32 = 0x180;
/// Fine units per tile.
pub const TILE_SPAN: i32 = 192;

// ---------------------------------------------------------------------------
// WData bits and sentinels that items care about
// ---------------------------------------------------------------------------

/// `WData::flags` bit set by `Item::init` and cleared by `Item::close`: "an item stands
/// in this cell". This is the bit `Unit::set_new_location` and `Unit::find_goody_box`
/// actually test — the `down` list is bookkeeping, this bit is the gate.
pub const WFLAG_ITEM: u16 = 0x8000;

/// `WData::flags` bit `0x100`. Tested by every goody lookup as
/// `if !(flags & 0x100) && (land == 1 || land == 2) { reject }`. Named only by its value:
/// its meaning (bridge? ice? shallows?) is **not derived** and belongs to the
/// `map_terrain` lane.
pub const WFLAG_OVERRIDE_LAND: u16 = 0x0100;

/// `WData::land` values that make a cell reject a goody lookup unless
/// [`WFLAG_OVERRIDE_LAND`] is set. Water, on the evidence, but not derived by name.
pub const LAND_REJECT_A: i8 = 1;
/// See [`LAND_REJECT_A`].
pub const LAND_REJECT_B: i8 = 2;

/// `WData::down` sentinel meaning "the occupant of this cell is an item, and
/// `down_who` is its slot in `items`" [measured, `Objects::init_item` writes
/// `mov word [cell+8], 0xFFFD`].
pub const DOWN_ITEM: i16 = -3;
/// `WData::down` "empty" value written by `World::clear_down`.
pub const DOWN_NONE: i16 = -1;

// ---------------------------------------------------------------------------
// Resource indices, straight out of the PDB TypeIndex enum
// ---------------------------------------------------------------------------

pub const RES_FOOD: usize = 0;
pub const RES_TIMBER: usize = 1;
pub const RES_WEALTH: usize = 2;
pub const RES_KNOWLEDGE: usize = 3;
pub const RES_METAL: usize = 4;
pub const RES_OIL: usize = 5;
/// The six "bucket" resources a goody box can be scored against.
pub const NUM_BUCKET_RESOURCES: usize = 6;

/// The resource a goody box will **never** award: index 3, `KNOWLEDGE`
/// [measured, `cmp esi, 3; je skip` at `0x005F99CC`].
pub const GOODY_EXCLUDED_RESOURCE: usize = RES_KNOWLEDGE;

/// The resource awarded when *no* candidate qualified: index 2, `WEALTH`
/// [measured, `mov ecx, 2; cmovns ecx, eax` at `0x005F9A6F`].
pub const GOODY_FALLBACK_RESOURCE: usize = RES_WEALTH;

/// Modulus applied to the RNG draw before it is added to the bucket level
/// [measured, `mov ecx, 0x19; idiv ecx` at `0x005F99FD`].
pub const GOODY_JITTER_MOD: i32 = 25;

/// Sentinel the "lowest bucket" search starts from [measured, `0x05F5E0FF`].
pub const GOODY_SCORE_SENTINEL: i32 = 0x05F5_E0FF;

/// `LeaderData::has_tribe_bonus(9)` — the Spanish ruins bonus, the only tribe bonus that
/// touches goody boxes [measured, `push 9; call LeaderData::has_tribe_bonus` at
/// `0x005F9A22`].
pub const TRIBE_BONUS_SPANISH_RUINS: i32 = 9;

/// `LeaderData::get_epoch(3)` — the Library epoch track the payout scales with.
///
/// Index 3 is `BASE_SCIENCETYPES = 551 = 0x227` [measured: `LeaderData::compute_epoch`
/// `0x006D6F80` maps cat 3 -> `0x227`, and the PDB `TypeIndex` enum names `0x227`
/// `WRITTEN_WORD` / `BASE_SCIENCETYPES`]. The other three are cat 2 -> `0x22E`
/// `BASE_COMMERCETYPES`, cat 1 -> `0x235` `BASE_CIVICTYPES`, cat 0 -> `0x23C`
/// `BASE_MILITARYTYPES`.
///
/// This is worth stating loudly because the `rules.xml` comment says
/// `GOODY_BOX_AGE value="25 resources / age"` and the machine code does **not** read
/// `LeaderData::get_age` (`enc + 0xDC`, XOR `0x62766`). It reads `enc + 0xF4`, XOR
/// `0x63187`, which is `epoch[3]` — the **Science** library track, 0..7.
pub const GOODY_EPOCH_TRACK: usize = 3;

// ---------------------------------------------------------------------------
// The four rules.xml constants the payout uses
// ---------------------------------------------------------------------------

/// The `Constants` fields `Unit::explore_goody` reads, with their `Constants` offsets and
/// their retail values from `ron-data/rules.xml`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GoodyRules {
    /// `Constants + 0xC24`, `<GOODY_BOX value="25 resources"/>`.
    pub goody_box: i32,
    /// `Constants + 0xC28`, `<GOODY_BOX_AGE value="25 resources / age"/>`.
    pub goody_box_age: i32,
    /// `Constants + 0x688`, `<SPANISH_RUINS_BASE value="30"/>`.
    pub spanish_ruins_base: i32,
    /// `Constants + 0x68C`, `<SPANISH_RUINS value="26"/>`.
    pub spanish_ruins: i32,
}

impl GoodyRules {
    /// The shipped values [measured, `docs/derivation/rules-constants.json`, parser
    /// `wtoi`, no scaling].
    pub const RETAIL: GoodyRules = GoodyRules {
        goody_box: 25,
        goody_box_age: 25,
        spanish_ruins_base: 30,
        spanish_ruins: 26,
    };
}

impl Default for GoodyRules {
    fn default() -> Self {
        GoodyRules::RETAIL
    }
}

/// `amount = epoch_science * per_epoch + base`, with the Spanish pair swapped in whole.
///
/// ```text
/// 005f9a2a  call LeaderData::has_tribe_bonus(9)
/// 005f9a37  mov  ebx, [enc + 0xF4]        ; epoch[3]
/// 005f9a42  je   normal
/// 005f9a44  xor  ebx, 0x63187
/// 005f9a4a  imul ebx, [constants + 0x68C] ; SPANISH_RUINS      = 26
/// 005f9a51  add  ebx, [constants + 0x688] ; SPANISH_RUINS_BASE = 30
/// normal:
/// 005f9a59  xor  ebx, 0x63187
/// 005f9a5f  imul ebx, [constants + 0xC28] ; GOODY_BOX_AGE      = 25
/// 005f9a66  add  ebx, [constants + 0xC24] ; GOODY_BOX          = 25
/// ```
#[inline]
pub fn goody_amount(rules: &GoodyRules, epoch_science: i32, spanish_ruins_bonus: bool) -> i32 {
    let (per, base) = if spanish_ruins_bonus {
        (rules.spanish_ruins, rules.spanish_ruins_base)
    } else {
        (rules.goody_box_age, rules.goody_box)
    };
    epoch_science.wrapping_mul(per).wrapping_add(base)
}

// ---------------------------------------------------------------------------
// Item
// ---------------------------------------------------------------------------

/// One slot of `PtrArray<Item>`, holding exactly the fields the checksum walks.
///
/// Layout mirrors the retail object so the walked byte ranges are transcribable
/// one-for-one (`sizeof(Item) == 44`):
///
/// | off | field | note |
/// |---:|---|---|
/// | `+0x08` | `flags: u8` | bit 0 = valid. `SubObjectData::flags` |
/// | `+0x09` | `who: u8` | always `0xFF` for an item — `Item::init` is called with `-1` |
/// | `+0x0A` | `o: i16` | the item's own slot index in `items` |
/// | `+0x0C` | `z_internal: i32` | `Coord ^ COORD_XOR` |
/// | `+0x10` | `x_internal: i32` | `Coord ^ COORD_XOR` |
/// | `+0x14` | `y_internal: i32` | `Coord ^ COORD_XOR` |
/// | `+0x18` | `ptype: ObjectType*` | walked as its `TypeData::type` (`Type + 4`) |
/// | `+0x1C` | `on_screen: u8` | presentation, **not** walked |
/// | `+0x20` | `ever_seen: u8` | per-player bitmask, `1 << who` |
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Item {
    /// `SubObjectData::flags` `+0x08`. `Item::init` sets it to exactly `1`; the
    /// `|= 0x20` branch in `SubObject::init` is gated on `Type` vtable slot 25, which for
    /// `ItemType` is the folded `xor eax,eax; ret` stub, so it never fires for items.
    pub flags: u8,
    /// `SubObjectData::who` `+0x09`.
    pub who: u8,
    /// `SubObjectData::o` `+0x0A` — the slot index, echoed into the object.
    pub o: i16,
    /// `SubObjectData::z_internal` `+0x0C`, XOR-obfuscated.
    pub z_internal: i32,
    /// `SubObjectData::x_internal` `+0x10`, XOR-obfuscated.
    pub x_internal: i32,
    /// `SubObjectData::y_internal` `+0x14`, XOR-obfuscated.
    pub y_internal: i32,
    /// `TypeData::type` of `SubObjectData::ptype`, or `0` when `ptype` is null.
    pub type_index: i32,
    /// True when `ptype != nullptr`. Separate from `type_index` because
    /// `SubObject::must_walk` tests the *pointer*, and a null pointer walks a `0`.
    pub has_type: bool,
    /// `ItemData::ever_seen` `+0x20` — bit `1 << who` per player.
    pub ever_seen: u8,
}

impl Item {
    /// `ItemData::is_valid_item` `0x0046CDA0` — `flags & 1`.
    #[inline]
    pub fn is_valid(&self) -> bool {
        self.flags & 1 != 0
    }

    /// De-obfuscated `x`.
    #[inline]
    pub fn x(&self) -> i32 {
        self.x_internal ^ COORD_XOR
    }
    /// De-obfuscated `y`.
    #[inline]
    pub fn y(&self) -> i32 {
        self.y_internal ^ COORD_XOR
    }
    /// De-obfuscated `z`.
    #[inline]
    pub fn z(&self) -> i32 {
        self.z_internal ^ COORD_XOR
    }

    /// `SubObject::must_walk` `0x006623A0`, on the **save/checksum** path
    /// (`DataWalk::input == 0`): `ptype != null || (flags & 1)`.
    ///
    /// The retail function also *emits this byte through the walker*, which is easy to
    /// miss: it stashes the bool in the high byte of its own argument slot
    /// (`mov byte [ebp+0xB], 0/1`) and walks `[ebp+0xB]..[ebp+0xC]`. One byte, always.
    #[inline]
    pub fn must_walk(&self) -> bool {
        self.has_type || self.is_valid()
    }

    /// `ItemData::is_seen(who, ...)` `0x00677850`, first clause only:
    /// `ever_seen & vision_mask` where the mask is the byte at `Leader + 0x6929` — a
    /// team/shared-vision mask, **not** simply `1 << who`. The rest of that function
    /// (the `who < 0` arm) is not transcribed here.
    #[inline]
    pub fn is_seen_by_mask(&self, vision_mask: u8) -> bool {
        self.ever_seen & vision_mask != 0
    }

    /// `World::reveal_fog` `0x006B3D30`: `item->ever_seen |= 1 << (who & 0x1F)`.
    ///
    /// The shift is performed in a 32-bit register and only then truncated into the
    /// 8-bit field. Consequently `who` 8..31 writes zero; it does **not** wrap back to
    /// bit 0. Player slots are 0..7, but preserving the out-of-contract behavior keeps
    /// malformed input from aliasing another player.
    #[inline]
    pub fn mark_seen(&mut self, who: u8) {
        self.ever_seen |= (1u32.wrapping_shl(u32::from(who) & 0x1F)) as u8;
    }

    /// Full `ItemData::is_seen(int who, int unused)` `0x00677850`.
    ///
    /// Retail tests three sources, in this order:
    ///
    /// 1. the item's `ever_seen` byte against the leader's shared-vision mask;
    /// 2. for a non-negative player with tribe bonus 9, when the item's W-cell owner is
    ///    that player, `WorldData::was_seen` at the item's fog coordinate;
    /// 3. otherwise `WorldData::is_seen` at that fog coordinate.
    ///
    /// The world owns the two visibility maps, so they are injected as callbacks. The
    /// second retail argument is unused by the shipped function and is intentionally not
    /// represented here. `vision_mask` and `spanish_ruins_bonus` are leader state that a
    /// future World integration must supply for `who`.
    pub fn is_seen<FWas, FIs>(
        &self,
        grid: &WGrid,
        who: i32,
        vision_mask: u8,
        spanish_ruins_bonus: bool,
        mut was_seen: FWas,
        mut is_seen: FIs,
    ) -> bool
    where
        FWas: FnMut(i32, i32, i32) -> bool,
        FIs: FnMut(i32, i32, i32) -> bool,
    {
        if self.is_seen_by_mask(vision_mask) {
            return true;
        }

        let (wx, wy) = (wcoord_of(self.x()), wcoord_of(self.y()));
        let (fx, fy) = (fcoord_of(self.x()), fcoord_of(self.y()));
        if who >= 0
            && spanish_ruins_bonus
            && grid.in_bounds(wx, wy)
            && i32::from(grid.cell(wx, wy).who) == who
        {
            return was_seen(fx, fy, who);
        }
        is_seen(fx, fy, who)
    }
}

/// `WCoord -> Coord` cell centre. `ItemTypeData::snap_center` `0x00677DF0`, verbatim:
/// `*x = wx * 0x300 + 0x180`.
#[inline]
pub fn snap_center(wx: i32, wy: i32) -> (i32, i32) {
    (
        wx.wrapping_mul(WCELL_SPAN).wrapping_add(WCELL_CENTER),
        wy.wrapping_mul(WCELL_SPAN).wrapping_add(WCELL_CENTER),
    )
}

/// `div_3_table[c >> 8]` — fine `Coord` to `WCoord`.
///
/// Reproduces the table exactly: arithmetic shift, then **unsigned** divide by 3
/// (`*(uint*)(base + i*4) = i / 3` at `0x00681DB0`). Negative inputs index the
/// uninitialised half of the retail table and are out of contract; we return the
/// mathematically consistent value rather than pretending to model garbage.
#[inline]
pub fn wcoord_of(c: i32) -> i32 {
    ((c >> 8) as u32 / 3) as i32
}

/// `div_3_table[c >> 6]` — fine `Coord` to `TCoord` (tile). Same caveat as [`wcoord_of`].
#[inline]
pub fn tcoord_of(c: i32) -> i32 {
    ((c >> 6) as u32 / 3) as i32
}

/// `div_3_table[c >> 7]` — fine `Coord` to the 384-unit fog grid used by
/// `ItemData::is_seen` [measured, `0x006778E8..0x00677953`].
#[inline]
pub fn fcoord_of(c: i32) -> i32 {
    ((c >> 7) as u32 / 3) as i32
}

// ---------------------------------------------------------------------------
// The WCoord cell grid, restricted to what items touch
// ---------------------------------------------------------------------------

/// The fields of the 28-byte `WData` cell that the item subsystem reads or writes.
///
/// This is deliberately *not* the whole `WData` (`flags, land, land_sub, region, region2,
/// down, down_who, val, goods, light, who, who2, blocked, bad, solid, was_seen, block`) —
/// the full cell belongs to the world/map lanes. Fold this in when they land one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ItemCell {
    /// `WData::flags` `+0x00`.
    pub flags: u16,
    /// `WData::land` `+0x02`.
    pub land: i8,
    /// `WData::region` `+0x04`.
    pub region: i16,
    /// `WData::down` `+0x08`.
    pub down: i16,
    /// `WData::down_who` `+0x0A`.
    pub down_who: i16,
    /// `WData::who` `+0x0F`, used by the Spanish-ruins branch of
    /// `ItemData::is_seen`.
    pub who: i8,
}

impl Default for ItemCell {
    fn default() -> Self {
        ItemCell {
            flags: 0,
            land: 0,
            region: 0,
            down: DOWN_NONE,
            down_who: DOWN_NONE,
            who: -1,
        }
    }
}

/// A WCoord-indexed grid of [`ItemCell`], `wy * xs + wx`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct WGrid {
    xs: i32,
    ys: i32,
    cells: Vec<ItemCell>,
}

/// What `World::clear_down` did, so a caller that owns `Objects` can finish the job.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClearDown {
    /// The cell head was a sentinel (`< 0`); `down`/`down_who` are now `-1`. Complete.
    HeadCleared,
    /// The cell head is a real object index. Retail walks the object chain to its tail
    /// and, if the tail's `next` (`Object + 0x2C`) is `< -1`, sets it to `-1`. We do not
    /// own `Objects`, so we report the entry point instead of guessing.
    NeedsObjectChain { down: i16, down_who: i16 },
}

/// The object-chain access that item bookkeeping needs but does not own.
///
/// Retail stores a cell's heterogeneous occupancy list as `(down, down_who)` followed by
/// `(ObjectData::next, ObjectData::next_who)` pairs. `down` is an index into the owner
/// selected by `down_who`; terminal negative values are sentinels (`-3` is an item).
/// The main object store can implement this trait without leaking its layout into this
/// subsystem.
pub trait ItemObjectChain {
    /// Return `(next, next_who)` for object `(index, who)`, or `None` if the reference is
    /// invalid. Retail would dereference an invalid reference; this port reports it.
    fn next_link(&self, index: i16, who: i16) -> Option<(i16, i16)>;

    /// Replace only `ObjectData::next`, leaving `next_who` untouched, as retail does when
    /// it unlinks a terminal item marker. Returns false if the reference is invalid.
    fn set_next(&mut self, index: i16, who: i16, next: i16) -> bool;
}

/// A corrupt or unavailable object chain prevented exact item bookkeeping.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ItemChainError {
    MissingObject { index: i16, who: i16 },
    Cycle { index: i16, who: i16 },
}

impl WGrid {
    pub fn new(xs: i32, ys: i32) -> WGrid {
        assert!(xs > 0 && ys > 0);
        WGrid {
            xs,
            ys,
            cells: vec![ItemCell::default(); (xs as usize) * (ys as usize)],
        }
    }

    #[inline]
    pub fn xs(&self) -> i32 {
        self.xs
    }
    #[inline]
    pub fn ys(&self) -> i32 {
        self.ys
    }

    #[inline]
    pub fn in_bounds(&self, wx: i32, wy: i32) -> bool {
        wx >= 0 && wy >= 0 && wx < self.xs && wy < self.ys
    }

    #[inline]
    fn idx(&self, wx: i32, wy: i32) -> usize {
        debug_assert!(self.in_bounds(wx, wy));
        (wy as usize) * (self.xs as usize) + (wx as usize)
    }

    #[inline]
    pub fn cell(&self, wx: i32, wy: i32) -> &ItemCell {
        &self.cells[self.idx(wx, wy)]
    }

    #[inline]
    pub fn cell_mut(&mut self, wx: i32, wy: i32) -> &mut ItemCell {
        let i = self.idx(wx, wy);
        &mut self.cells[i]
    }

    /// `(flags & WFLAG_ITEM) != 0`. `Unit::set_new_location` tests this as
    /// `(short)flags < 0`, which is the same bit.
    #[inline]
    pub fn has_item_bit(&self, wx: i32, wy: i32) -> bool {
        self.cell(wx, wy).flags & WFLAG_ITEM != 0
    }

    /// The land gate shared by every goody lookup
    /// (`ObjectsData::find_goody_at` `0x0065B7C0`, `World::reveal_fog`,
    /// `Unit::find_goody_box`, `Terrain::move_goody`):
    ///
    /// ```text
    /// if ((cell.flags & 0x100) == 0 && (cell.land == 1 || cell.land == 2)) reject;
    /// ```
    #[inline]
    pub fn goody_lookup_allowed(&self, wx: i32, wy: i32) -> bool {
        let c = self.cell(wx, wy);
        if c.flags & WFLAG_OVERRIDE_LAND == 0
            && (c.land == LAND_REJECT_A || c.land == LAND_REJECT_B)
        {
            return false;
        }
        true
    }

    /// `World::clear_down` `0x006B3AD0`.
    pub fn clear_down(&mut self, wx: i32, wy: i32) -> ClearDown {
        let c = *self.cell(wx, wy);
        if c.down >= 0 {
            return ClearDown::NeedsObjectChain {
                down: c.down,
                down_who: c.down_who,
            };
        }
        let m = self.cell_mut(wx, wy);
        m.down = DOWN_NONE;
        m.down_who = DOWN_NONE;
        ClearDown::HeadCleared
    }

    /// Complete `World::clear_down` `0x006B3AD0`, including a live object head.
    ///
    /// If the terminal link is any sentinel below `-1`, retail replaces that object's
    /// `next` with `-1`. The cell head itself is left in place. Cycle detection is a
    /// safety boundary for corrupt input; valid retail chains are acyclic.
    pub fn clear_down_with_objects<C: ItemObjectChain>(
        &mut self,
        wx: i32,
        wy: i32,
        objects: &mut C,
    ) -> Result<(), ItemChainError> {
        let head = *self.cell(wx, wy);
        if head.down < 0 {
            let c = self.cell_mut(wx, wy);
            c.down = DOWN_NONE;
            c.down_who = DOWN_NONE;
            return Ok(());
        }

        let mut index = head.down;
        let mut who = head.down_who;
        let mut visited = std::collections::HashSet::new();
        loop {
            if !visited.insert((index, who)) {
                return Err(ItemChainError::Cycle { index, who });
            }
            let (next, next_who) = objects
                .next_link(index, who)
                .ok_or(ItemChainError::MissingObject { index, who })?;
            if next < 0 {
                if next < DOWN_NONE && !objects.set_next(index, who, DOWN_NONE) {
                    return Err(ItemChainError::MissingObject { index, who });
                }
                return Ok(());
            }
            index = next;
            who = next_who;
        }
    }
}

// ---------------------------------------------------------------------------
// The registry
// ---------------------------------------------------------------------------

/// `GameAccess::items : PtrArray<Item>&` — the global goody-box registry.
///
/// Slot indices are **stable identity**: a removed item leaves its slot in place with
/// `flags == 0`, and `Objects::init_item` reuses the *first* such slot before it grows
/// the array. Reproduce that and only that; a `Vec::swap_remove` reorders live items and
/// changes the checksum.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Items {
    slots: Vec<Item>,
}

impl Items {
    pub fn new() -> Items {
        Items { slots: Vec::new() }
    }

    /// The `PtrArray` length — dead slots included, exactly as `items.length`.
    #[inline]
    pub fn len(&self) -> usize {
        self.slots.len()
    }
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }
    #[inline]
    pub fn get(&self, slot: usize) -> Option<&Item> {
        self.slots.get(slot)
    }
    #[inline]
    pub fn get_mut(&mut self, slot: usize) -> Option<&mut Item> {
        self.slots.get_mut(slot)
    }
    /// Live items, in array order — the order `check_items` hashes them in.
    pub fn iter_valid(&self) -> impl Iterator<Item = (usize, &Item)> {
        self.slots
            .iter()
            .enumerate()
            .filter(|(_, it)| it.is_valid())
    }
    /// Number of live items.
    pub fn count_valid(&self) -> usize {
        self.slots.iter().filter(|it| it.is_valid()).count()
    }

    /// `Objects::init_item(TypeIndex, Coord x, Coord y)` `0x00653E00`.
    ///
    /// ```text
    /// i = first index with (items[i]->flags & 1) == 0, else items.length (grow)
    /// items[i]->init(-1, type, i, x, y)          ; vtable +0x8C -> Item::init
    /// cell(x, y).down     = -3
    /// cell(x, y).down_who = i
    /// ```
    ///
    /// and `Item::init` `0x006770E0` in turn is
    /// `SubObject::init(-1, type, i, x, y); ever_seen = 0; cell(x, y).flags |= 0x8000`.
    ///
    /// `z` is `TerrainOut::find_tcoord_z(tcoord_of(x), tcoord_of(y), 1)`, which we cannot
    /// evaluate here — pass it in. The map/terrain lane owns that height query.
    pub fn init_item(
        &mut self,
        grid: &mut WGrid,
        type_index: i32,
        x: i32,
        y: i32,
        z: i32,
    ) -> usize {
        let slot = self
            .slots
            .iter()
            .position(|it| !it.is_valid())
            .unwrap_or_else(|| {
                self.slots.push(Item::default());
                self.slots.len() - 1
            });

        let it = &mut self.slots[slot];
        // SubObject::init 0x00662300
        it.who = 0xFF; // (u8)(-1)
        it.flags = 1;
        it.o = slot as i16;
        it.type_index = type_index;
        it.has_type = true;
        it.x_internal = x ^ COORD_XOR;
        it.y_internal = y ^ COORD_XOR;
        it.z_internal = z ^ COORD_XOR;
        // Item::init 0x006770E0
        it.ever_seen = 0;

        let (wx, wy) = (wcoord_of(x), wcoord_of(y));
        grid.cell_mut(wx, wy).flags |= WFLAG_ITEM;
        let c = grid.cell_mut(wx, wy);
        c.down = DOWN_ITEM;
        c.down_who = slot as i16;
        slot
    }

    /// Place a goody box at a WCoord cell centre — the map-generation entry point.
    /// `Map::place_region_resource` `0x00690480` and `Map::place_player_resource`
    /// `0x00691F70` both do `init_item(0x21F, wx*0x300 + 0x180, wy*0x300 + 0x180)`.
    pub fn place_goody(&mut self, grid: &mut WGrid, wx: i32, wy: i32, z: i32) -> usize {
        let (x, y) = snap_center(wx, wy);
        self.init_item(grid, TYPE_GOODY, x, y, z)
    }

    /// `Item::close` `0x00677050`, reached through `Objects::remove_item` `0x0065B850`
    /// (vtable `+0x90`):
    ///
    /// ```text
    /// cell(x, y).flags &= ~0x8000
    /// World::clear_down(wcoord_of(x), wcoord_of(y))
    /// this->flags = 0
    /// ```
    ///
    /// The slot is **not** freed — it stays in the array for `init_item` to reuse.
    pub fn close(&mut self, grid: &mut WGrid, slot: usize) -> Option<ClearDown> {
        let it = self.slots.get(slot)?;
        let (wx, wy) = (wcoord_of(it.x()), wcoord_of(it.y()));
        if !grid.in_bounds(wx, wy) {
            self.slots[slot].flags = 0;
            return None;
        }
        grid.cell_mut(wx, wy).flags &= !WFLAG_ITEM;
        let cleared = grid.clear_down(wx, wy);
        self.slots[slot].flags = 0;
        Some(cleared)
    }

    /// Complete `Item::close`, including `World::clear_down` when a live object heads
    /// the cell's occupancy chain.
    pub fn close_with_objects<C: ItemObjectChain>(
        &mut self,
        grid: &mut WGrid,
        slot: usize,
        objects: &mut C,
    ) -> Result<bool, ItemChainError> {
        let Some(it) = self.slots.get(slot) else {
            return Ok(false);
        };
        let (wx, wy) = (wcoord_of(it.x()), wcoord_of(it.y()));
        if !grid.in_bounds(wx, wy) {
            self.slots[slot].flags = 0;
            return Ok(true);
        }

        grid.cell_mut(wx, wy).flags &= !WFLAG_ITEM;
        grid.clear_down_with_objects(wx, wy, objects)?;
        self.slots[slot].flags = 0;
        Ok(true)
    }

    /// `ObjectsData::find_goody_at(TCoord&, TCoord&)` `0x0065C040` — the WCoord-cell
    /// overload, despite the PDB's parameter names.
    ///
    /// ```text
    /// d = cell.down; w = cell.down_who
    /// if (d >= 0) { walk the object chain: d = obj.next, w = obj.next_who }
    /// if (d == -3 && items[w] is valid) return w
    /// return -1
    /// ```
    ///
    /// The chain walk needs `Objects`, which this module does not own. This compatibility
    /// entry point is exact only when the head is already a sentinel. When the head is a
    /// live object index it returns `Err(head)` rather than inventing a chain; integration
    /// code must use [`Items::find_goody_at_with_objects`].
    pub fn find_goody_at(&self, grid: &WGrid, wx: i32, wy: i32) -> Result<i32, (i16, i16)> {
        if !grid.in_bounds(wx, wy) {
            return Ok(-1);
        }
        let c = grid.cell(wx, wy);
        if c.down >= 0 {
            return Err((c.down, c.down_who));
        }
        if c.down != DOWN_ITEM {
            return Ok(-1);
        }
        let slot = c.down_who as i32;
        match self.get(slot as usize) {
            Some(it) if it.is_valid() => Ok(slot),
            _ => Ok(-1),
        }
    }

    /// Complete `ObjectsData::find_goody_at` `0x0065C040`, including traversal through
    /// a live object head. Prefer this integration API once the owning World exposes an
    /// [`ItemObjectChain`]; [`Items::find_goody_at`] is deliberately sentinel-only.
    pub fn find_goody_at_with_objects<C: ItemObjectChain>(
        &self,
        grid: &WGrid,
        wx: i32,
        wy: i32,
        objects: &C,
    ) -> Result<i32, ItemChainError> {
        if !grid.in_bounds(wx, wy) {
            return Ok(-1);
        }
        let c = grid.cell(wx, wy);
        let mut down = c.down;
        let mut who = c.down_who;
        let mut visited = std::collections::HashSet::new();
        while down >= 0 {
            if !visited.insert((down, who)) {
                return Err(ItemChainError::Cycle { index: down, who });
            }
            let (next, next_who) = objects
                .next_link(down, who)
                .ok_or(ItemChainError::MissingObject { index: down, who })?;
            down = next;
            who = next_who;
        }
        if down != DOWN_ITEM || who < 0 {
            return Ok(-1);
        }
        Ok(match self.get(who as usize) {
            Some(it) if it.is_valid() => i32::from(who),
            _ => -1,
        })
    }

    /// `ObjectsData::find_goody_at(Coord, Coord, int)` `0x0065B7C0` — the fine-coordinate
    /// overload. Applies the land gate first, then defers to the cell overload.
    pub fn find_goody_at_coord(&self, grid: &WGrid, x: i32, y: i32) -> Result<i32, (i16, i16)> {
        let (wx, wy) = (wcoord_of(x), wcoord_of(y));
        if !grid.in_bounds(wx, wy) {
            return Ok(-1);
        }
        if !grid.goody_lookup_allowed(wx, wy) {
            return Ok(-1);
        }
        self.find_goody_at(grid, wx, wy)
    }

    /// Fine-coordinate counterpart to [`Items::find_goody_at_with_objects`].
    pub fn find_goody_at_coord_with_objects<C: ItemObjectChain>(
        &self,
        grid: &WGrid,
        x: i32,
        y: i32,
        objects: &C,
    ) -> Result<i32, ItemChainError> {
        let (wx, wy) = (wcoord_of(x), wcoord_of(y));
        if !grid.in_bounds(wx, wy) || !grid.goody_lookup_allowed(wx, wy) {
            return Ok(-1);
        }
        self.find_goody_at_with_objects(grid, wx, wy, objects)
    }

    /// `World::reveal_fog` `0x006B3D30`, item clause for a sentinel-headed cell.
    /// Use [`Items::reveal_with_objects`] when the cell can have a live object head.
    pub fn reveal(&mut self, grid: &WGrid, wx: i32, wy: i32, who: u8) {
        if !grid.in_bounds(wx, wy) || !grid.goody_lookup_allowed(wx, wy) {
            return;
        }
        if let Ok(slot) = self.find_goody_at(grid, wx, wy) {
            if slot >= 0 {
                if let Some(it) = self.get_mut(slot as usize) {
                    it.mark_seen(who);
                }
            }
        }
    }

    /// Complete `World::reveal_fog` item lookup through a heterogeneous object chain.
    pub fn reveal_with_objects<C: ItemObjectChain>(
        &mut self,
        grid: &WGrid,
        objects: &C,
        wx: i32,
        wy: i32,
        who: u8,
    ) -> Result<(), ItemChainError> {
        if !grid.in_bounds(wx, wy) || !grid.goody_lookup_allowed(wx, wy) {
            return Ok(());
        }
        let slot = self.find_goody_at_with_objects(grid, wx, wy, objects)?;
        if slot >= 0 {
            if let Some(it) = self.get_mut(slot as usize) {
                it.mark_seen(who);
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// The channel
// ---------------------------------------------------------------------------

/// `adler32` `0x00A46830` (`misc.cpp:464`) — the stock zlib routine, transcribed.
///
/// `NMAX = 0x15B0 = 5552`, `BASE = 65521` (the `0x80078071` / `0xFFFF000F` pair is the
/// reciprocal-multiply form of `% 65521`). A null buffer returns `1`; here that is the
/// empty slice.
pub fn adler32(adler: u32, buf: &[u8]) -> u32 {
    const BASE: u32 = 65521;
    const NMAX: usize = 0x15B0;

    let mut s1 = adler & 0xFFFF;
    let mut s2 = (adler >> 16) & 0xFFFF;

    if buf.is_empty() {
        // `test edi, edi; jne ...; lea eax, [edx+1]` -- with a null pointer the retail
        // routine returns 1. An empty (non-null) buffer falls through the loop and
        // returns `adler` unchanged; both agree when `adler == 1`, which is the only
        // value `check_all` ever starts a channel from.
        return adler;
    }

    let mut i = 0usize;
    while i < buf.len() {
        let n = core::cmp::min(NMAX, buf.len() - i);
        for &b in &buf[i..i + n] {
            s1 = s1.wrapping_add(u32::from(b));
            s2 = s2.wrapping_add(s1);
        }
        s1 %= BASE;
        s2 %= BASE;
        i += n;
    }
    (s2 << 16) | s1
}

/// Push one `Item`'s walked bytes, in emission order.
///
/// `Item::walk_data` `0x00677150` then `SubObject::walk_data` `0x006621D0`:
///
/// ```text
/// walker->walk_test(StringTable[..] + 0x165D0)   ; CheckSum::walk_test is `ret 4` -> 0 bytes
/// walker->walk(this+0x20, this+0x21)             ; ever_seen                       -> 1 byte
/// SubObject::walk_data(walker):
///   walker->walk_test(StringTable[..] + 0x1E938) ;                                 -> 0 bytes
///   walker->walk(this+8, this+9)                 ; flags                           -> 1 byte
///   b = this->must_walk(walker)                  ; emits 1 byte through the walker -> 1 byte
///   if (b) {
///     walker->walk(this+9, this+0x18)            ; who,o,z,x,y                     -> 15 bytes
///     t = ptype ? ptype->type : 0
///     walker->walk(&t, &t+4)                     ; TypeIndex                       -> 4 bytes
///   }
/// ```
///
/// **22 bytes for a live item.** The tag strings contribute nothing to the checksum —
/// they are there for `SaveGame`/`LoadGame`, whose `walk_test` is not a stub. That is the
/// single most likely place to over-count when porting from the save format.
pub fn walk_item(it: &Item, out: &mut Vec<u8>) {
    out.push(it.ever_seen);
    out.push(it.flags);
    let mw = it.must_walk();
    out.push(u8::from(mw));
    if mw {
        out.push(it.who);
        out.extend_from_slice(&it.o.to_le_bytes());
        out.extend_from_slice(&it.z_internal.to_le_bytes());
        out.extend_from_slice(&it.x_internal.to_le_bytes());
        out.extend_from_slice(&it.y_internal.to_le_bytes());
        let t = if it.has_type { it.type_index } else { 0 };
        out.extend_from_slice(&t.to_le_bytes());
    }
}

/// Bytes for a live item: `ever_seen`, `flags`, `must_walk`, 15 body bytes, 4 type bytes.
pub const ITEM_WALK_BYTES: usize = 22;

/// The whole `check_items` byte stream, in `items` array order.
///
/// `CheckSums::check_items` `0x00937790` iterates `items[0 .. items.length]`, **skips any
/// slot whose `flags & 1` is clear**, and calls `walk_data` on the rest. It does *not*
/// call `PtrArray<Item>::walk_data`, so — unlike a generic `Array<T>` channel — the
/// length, capacity and growth hint are **not** hashed. Only the live elements are.
pub fn channel_bytes(items: &Items) -> Vec<u8> {
    let mut out = Vec::with_capacity(items.count_valid() * ITEM_WALK_BYTES);
    for (_, it) in items.iter_valid() {
        walk_item(it, &mut out);
    }
    out
}

/// The `items` checksum channel value.
///
/// `check_all` `0x00936560` resets `CheckSum::accum` to `1` before every channel and sums
/// the fifteen results (`units, builds, walls, ammo, deaths, groups, guys, leaders,
/// cities, items, goods, world, rules, scenario_data, script_run_time`), so this returns
/// the channel's own accumulator, not a running total.
pub fn checksum_items(items: &Items) -> u32 {
    let mut accum = 1u32;
    let mut buf = Vec::with_capacity(ITEM_WALK_BYTES);
    for (_, it) in items.iter_valid() {
        buf.clear();
        walk_item(it, &mut buf);
        accum = adler32(accum, &buf);
    }
    accum
}

/// The byte count `CheckSum::size` (`+0x14`) accumulates for this channel.
pub fn channel_size(items: &Items) -> u32 {
    items
        .iter_valid()
        .map(|(_, it)| if it.must_walk() { 22u32 } else { 3 })
        .sum()
}

// ---------------------------------------------------------------------------
// The payout
// ---------------------------------------------------------------------------

/// What `Unit::explore_goody` did to a leader.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GoodyAward {
    /// Which `items` slot was consumed.
    pub slot: i32,
    /// Bucket index 0..5 that was credited.
    pub resource: usize,
    /// Amount credited to `bucket[resource]` *and* to `LeaderData::goody_box_resources`.
    pub amount: i32,
    /// How many `Random::get(0, 0xFFFF)` draws the collection consumed. This is the
    /// lockstep-relevant number: it is `count(i in 0..6 : type_avail(i) && i != 3)`.
    pub draws: u32,
}

/// The leader state `Unit::explore_goody` reads and writes.
///
/// `bucket` is `LeaderDataEncrypt::bucket[6]` at `Leader + 0x6EB8 -> +0x00`, stored
/// XOR `0x8221` in retail (`LeaderData::bucket_get` `0x0046F200`,
/// `LeaderData::bucket_add` `0x0043ED10`). We hold it decrypted: the encryption is
/// anti-tamper, is not walked by any checksum channel, and reproducing it would only add
/// a way to get it wrong.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LeaderGoody {
    /// The six resource stockpiles, decrypted.
    pub bucket: [i32; NUM_BUCKET_RESOURCES],
    /// `LeaderData::goody_box_resources`, `Leader + 0x86C` — a running total, named by
    /// the PDB, that this is the only writer of.
    pub goody_box_resources: i32,
    /// `LeaderData::type_avail(i, 1)` for `i` in `0..6` — is that resource type
    /// researched/available to this player yet.
    pub type_avail: [bool; NUM_BUCKET_RESOURCES],
    /// `LeaderData::get_epoch(3)` — the Science library track, 0..7. See
    /// [`GOODY_EPOCH_TRACK`].
    pub epoch_science: i32,
    /// `LeaderData::has_tribe_bonus(9)`.
    pub spanish_ruins_bonus: bool,
}

impl Default for LeaderGoody {
    fn default() -> Self {
        LeaderGoody {
            bucket: [0; NUM_BUCKET_RESOURCES],
            goody_box_resources: 0,
            type_avail: [true, true, true, true, true, true],
            epoch_science: 0,
            spanish_ruins_bonus: false,
        }
    }
}

/// The resource-selection loop of `Unit::explore_goody`, `0x005F99B0..0x005F9A16`.
///
/// ```text
/// best_res = -1; best = 0x05F5E0FF
/// for i in 0..6:
///     if !type_avail(i, 1): continue          ; no RNG draw
///     if i == 3:            continue          ; KNOWLEDGE, no RNG draw
///     b     = bucket[i]
///     score = Random::get(0, 0xFFFF) % 25 + b ; ONE draw, from game_random
///     if score < best: best_res = i; best = score
/// return best_res >= 0 ? best_res : 2         ; WEALTH
/// ```
///
/// Read that as "the resource you are poorest in, jittered by 0..24". The jitter is small
/// relative to any real stockpile, so it only decides near-ties — but it is a genuine
/// draw from the simulation stream every time, and skipping it desyncs.
///
/// Returns `(resource, draws)`.
pub fn pick_goody_resource(leader: &LeaderGoody, rng: &mut Random) -> (usize, u32) {
    let mut best_res: i32 = -1;
    let mut best: i32 = GOODY_SCORE_SENTINEL;
    let mut draws = 0u32;

    for i in 0..NUM_BUCKET_RESOURCES {
        // Order matters for faithfulness even though neither test draws: retail
        // evaluates type_avail first and only then rejects index 3.
        if !leader.type_avail[i] {
            continue;
        }
        if i == GOODY_EXCLUDED_RESOURCE {
            continue;
        }
        let b = leader.bucket[i];
        let r = rng.get(0, 0xFFFF);
        draws += 1;
        let score = (r % GOODY_JITTER_MOD).wrapping_add(b);
        if score < best {
            best_res = i as i32;
            best = score;
        }
    }

    let res = if best_res >= 0 {
        best_res as usize
    } else {
        GOODY_FALLBACK_RESOURCE
    };
    (res, draws)
}

/// `Unit::explore_goody` `0x005F9780`, simulation half.
///
/// Call order, transcribed:
///
/// 1. `slot = ObjectsData::find_goody_at(unit.x, unit.y, unit.who)`.
/// 2. if `slot >= 0`: remember the item's coords, then `item->close()`.
/// 3. detach the `-3` marker from the cell's object chain (see [`detach_item_marker`]).
/// 4. `cell(unit.x, unit.y).flags &= ~0x8000`.
/// 5. **`if (Game::frame == 0) return;`** — during setup a unit spawned on a goody box
///    deletes it and gets nothing.
/// 6. pick the resource (RNG), compute the amount, credit `bucket` and
///    `goody_box_resources`.
///
/// Everything between those steps in retail — `SoundGlobal::play(0x33)`, the message
/// window, `STAT_RUINS_RESOURCES` Steam stats — is presentation and is not modelled.
///
/// The caller is `Unit::set_new_location` `0x005F8D20`, gated at `0x005F9033` on:
/// the unit actually changed cell, `vt[0x30]() == 0`, `(unit + 0x68) & 1 == 0`,
/// **`UnitType::domain == 0`** (`ObjectTypeData::domain` at `+0x218` — land units only;
/// ships and aircraft never collect), and the destination cell carrying `0x8000`.
pub fn explore_goody(
    items: &mut Items,
    grid: &mut WGrid,
    leader: &mut LeaderGoody,
    rules: &GoodyRules,
    rng: &mut Random,
    unit_x: i32,
    unit_y: i32,
    game_frame: u32,
) -> Option<GoodyAward> {
    let (wx, wy) = (wcoord_of(unit_x), wcoord_of(unit_y));
    if !grid.in_bounds(wx, wy) {
        return None;
    }

    let slot = match items.find_goody_at_coord(grid, unit_x, unit_y) {
        Ok(s) => s,
        // Head of the cell's down list is a live object; the chain walk is Objects' job.
        Err(_) => return None,
    };
    if slot >= 0 {
        items.close(grid, slot as usize);
    }

    detach_item_marker(grid, wx, wy);
    grid.cell_mut(wx, wy).flags &= !WFLAG_ITEM;

    finish_explore_goody(leader, rules, rng, slot, game_frame)
}

/// Full `Unit::explore_goody` simulation half with heterogeneous object traversal and
/// unlink. The caller still owns the `Unit::set_new_location` gates documented on
/// [`explore_goody`].
#[allow(clippy::too_many_arguments)]
pub fn explore_goody_with_objects<C: ItemObjectChain>(
    items: &mut Items,
    grid: &mut WGrid,
    objects: &mut C,
    leader: &mut LeaderGoody,
    rules: &GoodyRules,
    rng: &mut Random,
    unit_x: i32,
    unit_y: i32,
    game_frame: u32,
) -> Result<Option<GoodyAward>, ItemChainError> {
    let (wx, wy) = (wcoord_of(unit_x), wcoord_of(unit_y));
    if !grid.in_bounds(wx, wy) {
        return Ok(None);
    }

    let slot = items.find_goody_at_coord_with_objects(grid, unit_x, unit_y, objects)?;
    if slot >= 0 {
        items.close_with_objects(grid, slot as usize, objects)?;
    }

    detach_item_marker_with_objects(grid, wx, wy, objects)?;
    grid.cell_mut(wx, wy).flags &= !WFLAG_ITEM;
    Ok(finish_explore_goody(leader, rules, rng, slot, game_frame))
}

fn finish_explore_goody(
    leader: &mut LeaderGoody,
    rules: &GoodyRules,
    rng: &mut Random,
    slot: i32,
    game_frame: u32,
) -> Option<GoodyAward> {
    // 0x005F9975: `cmp dword [Game + 0x550], 0; je return` -- Game::frame.
    if game_frame == 0 {
        return None;
    }

    let (resource, draws) = pick_goody_resource(leader, rng);
    let amount = goody_amount(rules, leader.epoch_science, leader.spanish_ruins_bonus);

    leader.bucket[resource] = leader.bucket[resource].wrapping_add(amount);
    leader.goody_box_resources = leader.goody_box_resources.wrapping_add(amount);

    Some(GoodyAward {
        slot,
        resource,
        amount,
        draws,
    })
}

/// Step 3 of [`explore_goody`], `0x005F987E..0x005F98CA`.
///
/// When the cell head is already a sentinel the retail code writes `down = -3` and
/// `down_who = <the value it just read>` — a **genuine no-op** at the instruction level
/// (`mov edx, 0xFFFFFFFD` / `mov [cell+8], dx`). It is dead in practice because
/// `Item::close` ran `World::clear_down` first, which set the head to `-1`, so the
/// `== -3` test fails. Transcribed as the no-op it is rather than "fixed", because a
/// tidier version would diverge if the head ever *were* `-3`.
///
/// The `down >= 0` arm walks the object chain clearing a `-3` `next` pointer; use
/// [`detach_item_marker_with_objects`] for that complete path.
pub fn detach_item_marker(grid: &mut WGrid, wx: i32, wy: i32) {
    let c = *grid.cell(wx, wy);
    if c.down >= 0 {
        // Object chain: caller territory. See ClearDown::NeedsObjectChain.
        return;
    }
    if c.down == DOWN_ITEM {
        let m = grid.cell_mut(wx, wy);
        m.down = DOWN_ITEM;
        m.down_who = c.down_who;
    }
}

/// Complete the object-chain arm of [`detach_item_marker`].
///
/// This transcribes `Unit::explore_goody` `0x005F982F..0x005F987C`: walk from the cell
/// head, and when an object's `next` is `-3`, replace that `next` with `-1`. In the usual
/// path `Item::close` has already done the same work through `World::clear_down`; the
/// second pass is retained because it exists in retail.
pub fn detach_item_marker_with_objects<C: ItemObjectChain>(
    grid: &mut WGrid,
    wx: i32,
    wy: i32,
    objects: &mut C,
) -> Result<(), ItemChainError> {
    let c = *grid.cell(wx, wy);
    if c.down < 0 {
        detach_item_marker(grid, wx, wy);
        return Ok(());
    }

    let mut index = c.down;
    let mut who = c.down_who;
    let mut visited = std::collections::HashSet::new();
    loop {
        if !visited.insert((index, who)) {
            return Err(ItemChainError::Cycle { index, who });
        }
        let (next, next_who) = objects
            .next_link(index, who)
            .ok_or(ItemChainError::MissingObject { index, who })?;
        if next == DOWN_ITEM && !objects.set_next(index, who, DOWN_NONE) {
            return Err(ItemChainError::MissingObject { index, who });
        }
        if next < 0 {
            return Ok(());
        }
        index = next;
        who = next_who;
    }
}

// `Terrain::move_goody` `0x0084A3D0` is intentionally not exposed as an item API yet.
// It is a 2,798-byte terrain-repair transaction over items, goods, forests, terrain
// visibility, occupancy, cell flags, and height. The item arm scans retail spiral entries
// 9..24, but candidate eligibility depends on state this module does not own (including a
// 49-cell pre-change land snapshot). An item-only approximation would silently move ruins
// to cells retail rejects. The integration boundary and exact missing predicates are
// recorded in `docs/mechanics/items.md`.

// ---------------------------------------------------------------------------
// The scout search
// ---------------------------------------------------------------------------

/// `radius[11]` at `0x00ADD1E0` — `(2r+1)^2`, the prefix length of the ring tables.
pub const RADIUS: [i32; 11] = [1, 9, 25, 49, 81, 121, 169, 225, 289, 361, 441];

/// `move_x[0..49]` at `0x00ADCAF0` — the first four rings of the 441-entry neighbourhood
/// spiral, in the order the engine visits them [measured, dumped from `.rdata`].
pub const MOVE_X: [i32; 49] = [
    0, -1, 0, 1, 1, 1, 0, -1, -1, -1, 0, 1, 2, 2, 2, 1, 0, -1, -2, -2, -2, -2, 2, 2, -2, -3, -2,
    -1, 0, 1, 2, 3, 3, 3, 3, 3, 3, 3, 2, 1, 0, -1, -2, -3, -3, -3, -3, -3, -3,
];

/// `move_y[0..49]` at `0x00ADC400`. See [`MOVE_X`].
pub const MOVE_Y: [i32; 49] = [
    0, -1, -1, -1, 0, 1, 1, 1, 0, -2, -2, -2, -1, 0, 1, 2, 2, 2, 1, 0, -1, -2, -2, 2, 2, -3, -3,
    -3, -3, -3, -3, -3, -2, -1, 0, 1, 2, 3, 3, 3, 3, 3, 3, 3, 2, 1, 0, -1, -2,
];

/// `Unit::find_goody_box` scans `radius[3] == 49` offsets — Chebyshev radius 3 in WCoord
/// cells, i.e. a 7x7 block, 12 tiles across [measured, `cmp local, 0xC4` with a stride
/// of 4 bytes].
pub const GOODY_SEARCH_CELLS: usize = 49;

/// `Unit::find_goody_box` `0x005F2540`, search half.
///
/// Retail preconditions, in order:
///
/// ```text
/// if (unit->ptype->domain != 0) return 0;          ; land units only
/// home_region = cell(unit).region
/// for k in 0..49:                                   ; ring order, MOVE_X/MOVE_Y
///     wx = ux + move_x[k]; wy = uy + move_y[k]
///     if out of bounds: continue
///     if cell(wx,wy).region != home_region: continue
///     if (short)cell(wx,wy).flags >= 0: continue     ; needs the 0x8000 item bit
///     if any of the four fog cells (wx*2+{0,1}, wy*2+{0,1}) is seen by unit->who:
///         -> accept
///     else if the land gate passes and find_goody_at() finds a slot whose
///          ItemData::is_seen(unit->who) is true:
///         -> accept
///     ; accept: if the cell is already the unit's order target, return 0,
///     ;         else issue a move there and return 1
/// ```
///
/// Fog is not this module's state, so both retail visibility predicates are injected:
/// `was_seen(fx, fy, who)` and `is_seen(fx, fy, who)`. The four-cell discovery gate calls
/// `was_seen` with **fog-cell** coordinates `WCoord * 2 + {0,1}`. The item fallback calls
/// full [`Item::is_seen`], including shared vision, the Spanish owned-cell special case,
/// and current visibility.
///
/// Returns the accepted `(wx, wy)`. The caller must enforce the domain precondition and
/// suppress an order when this is already the unit's target; issuing the move is the
/// movement lane's job. This entry point cannot traverse a live object head and skips
/// such a cell; use [`find_goody_box_with_objects`] for World integration.
pub fn find_goody_box<FWas, FIs>(
    items: &Items,
    grid: &WGrid,
    unit_x: i32,
    unit_y: i32,
    who: u8,
    vision_mask: u8,
    spanish_ruins_bonus: bool,
    was_seen: FWas,
    is_seen: FIs,
) -> Option<(i32, i32)>
where
    FWas: FnMut(i32, i32, i32) -> bool,
    FIs: FnMut(i32, i32, i32) -> bool,
{
    find_goody_box_inner(
        items,
        grid,
        unit_x,
        unit_y,
        who,
        vision_mask,
        spanish_ruins_bonus,
        was_seen,
        is_seen,
        |wx, wy| {
            Ok::<_, core::convert::Infallible>(items.find_goody_at(grid, wx, wy).unwrap_or(-1))
        },
    )
    .expect("infallible sentinel-only lookup")
}

/// Full [`find_goody_box`] search with heterogeneous object-chain traversal.
pub fn find_goody_box_with_objects<C, FWas, FIs>(
    items: &Items,
    grid: &WGrid,
    objects: &C,
    unit_x: i32,
    unit_y: i32,
    who: u8,
    vision_mask: u8,
    spanish_ruins_bonus: bool,
    was_seen: FWas,
    is_seen: FIs,
) -> Result<Option<(i32, i32)>, ItemChainError>
where
    C: ItemObjectChain,
    FWas: FnMut(i32, i32, i32) -> bool,
    FIs: FnMut(i32, i32, i32) -> bool,
{
    find_goody_box_inner(
        items,
        grid,
        unit_x,
        unit_y,
        who,
        vision_mask,
        spanish_ruins_bonus,
        was_seen,
        is_seen,
        |wx, wy| items.find_goody_at_with_objects(grid, wx, wy, objects),
    )
}

#[allow(clippy::too_many_arguments)]
fn find_goody_box_inner<E, FWas, FIs, FFind>(
    items: &Items,
    grid: &WGrid,
    unit_x: i32,
    unit_y: i32,
    who: u8,
    vision_mask: u8,
    spanish_ruins_bonus: bool,
    mut was_seen: FWas,
    mut is_seen: FIs,
    mut find_goody: FFind,
) -> Result<Option<(i32, i32)>, E>
where
    FWas: FnMut(i32, i32, i32) -> bool,
    FIs: FnMut(i32, i32, i32) -> bool,
    FFind: FnMut(i32, i32) -> Result<i32, E>,
{
    let (ux, uy) = (wcoord_of(unit_x), wcoord_of(unit_y));
    if !grid.in_bounds(ux, uy) {
        return Ok(None);
    }
    let home_region = grid.cell(ux, uy).region;

    for k in 0..GOODY_SEARCH_CELLS {
        let wx = ux + MOVE_X[k];
        let wy = uy + MOVE_Y[k];
        if !grid.in_bounds(wx, wy) {
            continue;
        }
        let c = grid.cell(wx, wy);
        if c.region != home_region {
            continue;
        }
        if c.flags & WFLAG_ITEM == 0 {
            continue;
        }

        let discovered = was_seen(wx * 2 + 1, wy * 2 + 1, i32::from(who))
            || was_seen(wx * 2, wy * 2 + 1, i32::from(who))
            || was_seen(wx * 2 + 1, wy * 2, i32::from(who))
            || was_seen(wx * 2, wy * 2, i32::from(who));
        if discovered {
            return Ok(Some((wx, wy)));
        }

        if !grid.goody_lookup_allowed(wx, wy) {
            continue;
        }
        match find_goody(wx, wy)? {
            slot if slot >= 0 => {
                if let Some(it) = items.get(slot as usize) {
                    if it.is_seen(
                        grid,
                        i32::from(who),
                        vision_mask,
                        spanish_ruins_bonus,
                        &mut was_seen,
                        &mut is_seen,
                    ) {
                        return Ok(Some((wx, wy)));
                    }
                }
            }
            _ => {}
        }
    }
    Ok(None)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct TestObjects {
        links: std::collections::HashMap<(i16, i16), (i16, i16)>,
    }

    impl ItemObjectChain for TestObjects {
        fn next_link(&self, index: i16, who: i16) -> Option<(i16, i16)> {
            self.links.get(&(index, who)).copied()
        }

        fn set_next(&mut self, index: i16, who: i16, next: i16) -> bool {
            let Some(link) = self.links.get_mut(&(index, who)) else {
                return false;
            };
            link.0 = next;
            true
        }
    }

    fn grid() -> WGrid {
        WGrid::new(16, 16)
    }

    // -- type-level facts ---------------------------------------------------

    #[test]
    fn there_is_exactly_one_item_type() {
        assert_eq!(TYPE_GOODY, 543);
        assert_eq!(END_ITEMTYPES - TYPE_GOODY, NUM_ITEMTYPES as i32);
    }

    #[test]
    fn resource_indices_match_the_pdb_enum() {
        assert_eq!(RES_KNOWLEDGE, GOODY_EXCLUDED_RESOURCE);
        assert_eq!(RES_WEALTH, GOODY_FALLBACK_RESOURCE);
        assert_eq!((RES_FOOD, RES_TIMBER, RES_WEALTH), (0, 1, 2));
        assert_eq!((RES_KNOWLEDGE, RES_METAL, RES_OIL), (3, 4, 5));
    }

    // -- coordinate ladder --------------------------------------------------

    #[test]
    fn div3_table_reproduces_the_two_conversions() {
        // div_3_table[i] = i/3, so [c>>8] = c/768 and [c>>6] = c/192.
        for c in 0..(768 * 40) {
            assert_eq!(wcoord_of(c), c / 768, "wcoord_of({c})");
            assert_eq!(tcoord_of(c), c / 192, "tcoord_of({c})");
        }
    }

    #[test]
    fn snap_center_is_the_cell_middle() {
        assert_eq!(snap_center(0, 0), (384, 384));
        assert_eq!(snap_center(3, 5), (3 * 768 + 384, 5 * 768 + 384));
        for w in 0..64 {
            let (x, _) = snap_center(w, 0);
            assert_eq!(
                wcoord_of(x),
                w,
                "a snapped item must land back in its own cell"
            );
        }
    }

    #[test]
    fn wcell_is_four_tiles() {
        assert_eq!(WCELL_SPAN, 4 * TILE_SPAN);
        assert_eq!(WCELL_CENTER * 2, WCELL_SPAN);
        assert_eq!(fcoord_of(WCELL_CENTER), 1);
    }

    // -- the obfuscation ----------------------------------------------------

    #[test]
    fn coords_round_trip_through_the_xor() {
        let mut g = grid();
        let mut items = Items::new();
        let (x, y) = snap_center(4, 7);
        let s = items.init_item(&mut g, TYPE_GOODY, x, y, 1234);
        let it = items.get(s).unwrap();
        assert_eq!((it.x(), it.y(), it.z()), (x, y, 1234));
        assert_ne!(it.x_internal, x, "the stored word must be obfuscated");
    }

    /// The ctor's -1 sentinel, `0xFFF9C9C8`, is exactly `-1 ^ COORD_XOR`. That is the
    /// cheapest possible check that the key is the right one.
    #[test]
    fn ctor_sentinel_is_minus_one_obfuscated() {
        assert_eq!((-1i32) ^ COORD_XOR, 0xFFF9_C9C8u32 as i32);
    }

    // -- registry lifecycle -------------------------------------------------

    #[test]
    fn init_item_marks_the_cell_and_the_down_list() {
        let mut g = grid();
        let mut items = Items::new();
        let (x, y) = snap_center(2, 3);
        let s = items.init_item(&mut g, TYPE_GOODY, x, y, 0);
        assert_eq!(s, 0);
        assert!(g.has_item_bit(2, 3));
        assert_eq!(g.cell(2, 3).down, DOWN_ITEM);
        assert_eq!(g.cell(2, 3).down_who, 0);
        let it = items.get(0).unwrap();
        assert_eq!(
            it.flags, 1,
            "SubObject::init writes exactly 1; the |0x20 arm is a stub for ItemType"
        );
        assert_eq!(it.who, 0xFF, "Item::init passes who = -1");
        assert_eq!(it.o, 0);
        assert_eq!(it.ever_seen, 0);
    }

    #[test]
    fn close_clears_the_bit_and_frees_the_slot_in_place() {
        let mut g = grid();
        let mut items = Items::new();
        let (x, y) = snap_center(2, 3);
        let s = items.init_item(&mut g, TYPE_GOODY, x, y, 0);
        assert_eq!(items.close(&mut g, s), Some(ClearDown::HeadCleared));
        assert!(!g.has_item_bit(2, 3));
        assert_eq!(g.cell(2, 3).down, DOWN_NONE);
        assert_eq!(items.len(), 1, "the slot stays in the array");
        assert!(!items.get(0).unwrap().is_valid());
        assert_eq!(items.count_valid(), 0);
    }

    #[test]
    fn init_item_reuses_the_lowest_free_slot() {
        let mut g = grid();
        let mut items = Items::new();
        let a = items.place_goody(&mut g, 1, 1, 0);
        let b = items.place_goody(&mut g, 2, 2, 0);
        let c = items.place_goody(&mut g, 3, 3, 0);
        assert_eq!((a, b, c), (0, 1, 2));
        items.close(&mut g, a);
        items.close(&mut g, b);
        let d = items.place_goody(&mut g, 4, 4, 0);
        assert_eq!(d, 0, "first free slot, not the end");
        let e = items.place_goody(&mut g, 5, 5, 0);
        assert_eq!(e, 1);
        assert_eq!(items.len(), 3, "no growth while free slots remain");
    }

    #[test]
    fn o_field_tracks_the_slot_on_reuse() {
        let mut g = grid();
        let mut items = Items::new();
        items.place_goody(&mut g, 1, 1, 0);
        let b = items.place_goody(&mut g, 2, 2, 0);
        items.close(&mut g, b);
        let c = items.place_goody(&mut g, 6, 6, 0);
        assert_eq!(c, b);
        assert_eq!(items.get(c).unwrap().o, c as i16);
    }

    // -- lookup -------------------------------------------------------------

    #[test]
    fn find_goody_at_respects_the_land_gate() {
        let mut g = grid();
        let mut items = Items::new();
        let (x, y) = snap_center(5, 5);
        items.place_goody(&mut g, 5, 5, 0);
        assert_eq!(items.find_goody_at_coord(&g, x, y), Ok(0));

        g.cell_mut(5, 5).land = LAND_REJECT_A;
        assert_eq!(items.find_goody_at_coord(&g, x, y), Ok(-1), "water rejects");

        g.cell_mut(5, 5).flags |= WFLAG_OVERRIDE_LAND;
        assert_eq!(
            items.find_goody_at_coord(&g, x, y),
            Ok(0),
            "0x100 overrides"
        );
    }

    #[test]
    fn a_stale_down_marker_is_not_a_goody() {
        let mut g = grid();
        let mut items = Items::new();
        items.place_goody(&mut g, 5, 5, 0);
        // Hand-forge the situation retail leaves when the marker survives close().
        items.get_mut(0).unwrap().flags = 0;
        g.cell_mut(5, 5).down = DOWN_ITEM;
        g.cell_mut(5, 5).down_who = 0;
        assert_eq!(items.find_goody_at(&g, 5, 5), Ok(-1));
    }

    #[test]
    fn a_live_object_head_is_reported_not_guessed() {
        let mut g = grid();
        let items = Items::new();
        g.cell_mut(4, 4).down = 7;
        g.cell_mut(4, 4).down_who = 2;
        assert_eq!(items.find_goody_at(&g, 4, 4), Err((7, 2)));
    }

    #[test]
    fn full_lookup_walks_and_unlinks_a_live_object_head() {
        let mut g = grid();
        let mut items = Items::new();
        let slot = items.place_goody(&mut g, 4, 4, 0);
        g.cell_mut(4, 4).down = 7;
        g.cell_mut(4, 4).down_who = 2;
        let mut objects = TestObjects::default();
        objects.links.insert((7, 2), (DOWN_ITEM, slot as i16));

        assert_eq!(
            items.find_goody_at_with_objects(&g, 4, 4, &objects),
            Ok(slot as i32)
        );
        assert_eq!(
            items.close_with_objects(&mut g, slot, &mut objects),
            Ok(true)
        );
        assert_eq!(objects.links[&(7, 2)].0, DOWN_NONE);
        assert_eq!(g.cell(4, 4).down, 7, "retail retains the live cell head");
        assert!(!items.get(slot).unwrap().is_valid());
    }

    #[test]
    fn reveal_with_objects_marks_an_item_behind_a_live_head() {
        let mut g = grid();
        let mut items = Items::new();
        let slot = items.place_goody(&mut g, 4, 4, 0);
        g.cell_mut(4, 4).down = 7;
        g.cell_mut(4, 4).down_who = 2;
        let mut objects = TestObjects::default();
        objects.links.insert((7, 2), (DOWN_ITEM, slot as i16));

        items.reveal_with_objects(&g, &objects, 4, 4, 3).unwrap();
        assert_eq!(items.get(slot).unwrap().ever_seen, 0b0000_1000);
    }

    #[test]
    fn corrupt_object_cycles_are_reported() {
        let mut g = grid();
        let items = Items::new();
        g.cell_mut(4, 4).down = 7;
        g.cell_mut(4, 4).down_who = 2;
        let mut objects = TestObjects::default();
        objects.links.insert((7, 2), (8, 2));
        objects.links.insert((8, 2), (7, 2));
        assert_eq!(
            items.find_goody_at_with_objects(&g, 4, 4, &objects),
            Err(ItemChainError::Cycle { index: 7, who: 2 })
        );
    }

    // -- ever_seen ----------------------------------------------------------

    #[test]
    fn reveal_sets_one_bit_per_player() {
        let mut g = grid();
        let mut items = Items::new();
        items.place_goody(&mut g, 6, 6, 0);
        items.reveal(&g, 6, 6, 0);
        items.reveal(&g, 6, 6, 3);
        assert_eq!(items.get(0).unwrap().ever_seen, 0b0000_1001);
        assert!(items.get(0).unwrap().is_seen_by_mask(0b0000_0001));
        assert!(!items.get(0).unwrap().is_seen_by_mask(0b0000_0110));
    }

    #[test]
    fn reveal_shift_truncates_instead_of_wrapping_for_who_ge_eight() {
        let mut item = Item::default();
        item.mark_seen(8);
        item.mark_seen(31);
        assert_eq!(item.ever_seen, 0);
        item.mark_seen(32);
        assert_eq!(
            item.ever_seen, 1,
            "x86 masks a 32-bit shift count to five bits"
        );
    }

    #[test]
    fn full_is_seen_uses_shared_owned_history_then_current_visibility() {
        let mut g = grid();
        let mut items = Items::new();
        let slot = items.place_goody(&mut g, 6, 6, 0);
        let item = items.get_mut(slot).unwrap();
        item.ever_seen = 0b0000_0100;

        assert!(item.is_seen(
            &g,
            0,
            0b0000_0100,
            false,
            |_, _, _| panic!("shared-vision hit must short-circuit"),
            |_, _, _| panic!("shared-vision hit must short-circuit"),
        ));

        item.ever_seen = 0;
        g.cell_mut(6, 6).who = 0;
        assert!(item.is_seen(
            &g,
            0,
            0,
            true,
            |fx, fy, who| (fx, fy, who) == (13, 13, 0),
            |_, _, _| false,
        ));

        g.cell_mut(6, 6).who = 1;
        assert!(item.is_seen(
            &g,
            0,
            0,
            true,
            |_, _, _| false,
            |fx, fy, who| (fx, fy, who) == (13, 13, 0),
        ));
    }

    #[test]
    fn reveal_is_blocked_by_the_land_gate() {
        let mut g = grid();
        let mut items = Items::new();
        items.place_goody(&mut g, 6, 6, 0);
        g.cell_mut(6, 6).land = LAND_REJECT_B;
        items.reveal(&g, 6, 6, 0);
        assert_eq!(items.get(0).unwrap().ever_seen, 0);
    }

    // -- adler32 ------------------------------------------------------------

    /// zlib's own published vectors. If these fail nothing downstream is meaningful.
    #[test]
    fn adler32_matches_the_reference() {
        assert_eq!(adler32(1, b""), 1);
        assert_eq!(adler32(1, b"a"), 0x0062_0062);
        assert_eq!(adler32(1, b"abc"), 0x024D_0127);
        assert_eq!(adler32(1, b"Wikipedia"), 0x11E6_0398);
    }

    /// The `NMAX = 5552` block structure has to be transparent: chaining a call must be
    /// identical to hashing the concatenation, or the per-item chaining in
    /// `checksum_items` would not equal `adler32` over `channel_bytes`.
    #[test]
    fn adler32_chains_like_one_pass() {
        let big: Vec<u8> = (0..20_000u32).map(|i| (i * 37 + 11) as u8).collect();
        let one = adler32(1, &big);
        let mut acc = 1u32;
        for chunk in big.chunks(97) {
            acc = adler32(acc, chunk);
        }
        assert_eq!(one, acc);
        // and across the NMAX boundary specifically
        let mut acc2 = adler32(1, &big[..5552]);
        acc2 = adler32(acc2, &big[5552..]);
        assert_eq!(one, acc2);
    }

    // -- the channel --------------------------------------------------------

    #[test]
    fn a_live_item_walks_exactly_twenty_two_bytes() {
        let mut g = grid();
        let mut items = Items::new();
        items.place_goody(&mut g, 1, 2, 99);
        let b = channel_bytes(&items);
        assert_eq!(b.len(), ITEM_WALK_BYTES);
        assert_eq!(channel_size(&items), 22);
    }

    /// The exact byte layout, spelled out. If someone "helpfully" reorders `ever_seen`
    /// after `flags`, or drops the `must_walk` byte, this fails.
    #[test]
    fn walked_byte_order_is_ever_seen_flags_mustwalk_body_type() {
        let mut g = grid();
        let mut items = Items::new();
        let (x, y) = snap_center(1, 2);
        items.place_goody(&mut g, 1, 2, 99);
        items.get_mut(0).unwrap().ever_seen = 0x0A;

        let b = channel_bytes(&items);
        assert_eq!(
            b[0], 0x0A,
            "ever_seen first -- Item::walk_data walks +0x20 before the base"
        );
        assert_eq!(b[1], 1, "flags");
        assert_eq!(b[2], 1, "must_walk emits its own byte");
        assert_eq!(b[3], 0xFF, "who = (u8)-1");
        assert_eq!(i16::from_le_bytes([b[4], b[5]]), 0, "o = slot 0");
        assert_eq!(i32::from_le_bytes([b[6], b[7], b[8], b[9]]), 99 ^ COORD_XOR);
        assert_eq!(
            i32::from_le_bytes([b[10], b[11], b[12], b[13]]),
            x ^ COORD_XOR
        );
        assert_eq!(
            i32::from_le_bytes([b[14], b[15], b[16], b[17]]),
            y ^ COORD_XOR
        );
        assert_eq!(
            i32::from_le_bytes([b[18], b[19], b[20], b[21]]),
            TYPE_GOODY,
            "TypeData::type, not the pointer"
        );
    }

    #[test]
    fn dead_slots_contribute_nothing() {
        let mut g = grid();
        let mut a = Items::new();
        a.place_goody(&mut g, 1, 1, 0);
        let dead = a.place_goody(&mut g, 2, 2, 0);
        a.place_goody(&mut g, 3, 3, 0);
        a.close(&mut g, dead);

        let mut g2 = grid();
        let mut b = Items::new();
        b.place_goody(&mut g2, 1, 1, 0);
        b.place_goody(&mut g2, 3, 3, 0);
        // b's second item is at slot 1, a's is at slot 2 -- different `o`, so the
        // checksums differ. That is the point: `o` is walked.
        assert_ne!(checksum_items(&a), checksum_items(&b));
        assert_eq!(channel_size(&a), channel_size(&b));
    }

    #[test]
    fn checksum_starts_at_one_and_is_order_sensitive() {
        let empty = Items::new();
        assert_eq!(
            checksum_items(&empty),
            1,
            "an empty channel is adler32's seed"
        );

        let mut g = grid();
        let mut a = Items::new();
        a.place_goody(&mut g, 1, 1, 0);
        a.place_goody(&mut g, 9, 9, 0);

        let mut g2 = grid();
        let mut b = Items::new();
        b.place_goody(&mut g2, 9, 9, 0);
        b.place_goody(&mut g2, 1, 1, 0);

        assert_ne!(checksum_items(&a), checksum_items(&b));
    }

    #[test]
    fn chaining_per_item_equals_hashing_the_whole_stream() {
        let mut g = grid();
        let mut items = Items::new();
        for i in 0..12 {
            items.place_goody(&mut g, i % 4, i / 4, i * 7);
        }
        items.get_mut(5).unwrap().ever_seen = 0xC3;
        assert_eq!(checksum_items(&items), adler32(1, &channel_bytes(&items)));
    }

    #[test]
    fn ever_seen_is_checksummed() {
        let mut g = grid();
        let mut items = Items::new();
        items.place_goody(&mut g, 4, 4, 0);
        let before = checksum_items(&items);
        items.reveal(&g, 4, 4, 2);
        assert_ne!(
            checksum_items(&items),
            before,
            "fog reveal mutates lockstep state -- this is why items are a channel"
        );
    }

    #[test]
    fn a_null_ptype_item_walks_only_three_bytes() {
        // must_walk is `ptype != null || (flags & 1)`. check_items filters on flags&1, so
        // a walked item always has must_walk == true; the false arm only exists on the
        // save path. Exercise the encoder anyway so the guard cannot rot.
        let it = Item::default();
        assert!(!it.must_walk());
        let mut v = Vec::new();
        walk_item(&it, &mut v);
        assert_eq!(v, vec![0, 0, 0]);
    }

    // -- the payout ---------------------------------------------------------

    #[test]
    fn goody_amount_is_epoch_times_rate_plus_base() {
        let r = GoodyRules::RETAIL;
        assert_eq!(goody_amount(&r, 0, false), 25);
        assert_eq!(goody_amount(&r, 1, false), 50);
        assert_eq!(goody_amount(&r, 7, false), 200);
        assert_eq!(goody_amount(&r, 0, true), 30);
        assert_eq!(goody_amount(&r, 7, true), 212);
    }

    #[test]
    fn retail_constants_are_the_shipped_ones() {
        let r = GoodyRules::RETAIL;
        assert_eq!((r.goody_box, r.goody_box_age), (25, 25));
        assert_eq!((r.spanish_ruins_base, r.spanish_ruins), (30, 26));
    }

    #[test]
    fn knowledge_is_never_awarded_and_never_draws() {
        let mut leader = LeaderGoody::default();
        // Make KNOWLEDGE overwhelmingly the poorest, so a naive min would pick it.
        leader.bucket = [10_000, 10_000, 10_000, 0, 10_000, 10_000];
        let mut rng = Random::new(0x1234_5678);
        for _ in 0..200 {
            let (res, draws) = pick_goody_resource(&leader, &mut rng);
            assert_ne!(res, RES_KNOWLEDGE);
            assert_eq!(draws, 5, "six resources minus KNOWLEDGE");
        }
    }

    #[test]
    fn unavailable_resources_consume_no_draw() {
        let mut leader = LeaderGoody::default();
        leader.type_avail = [true, true, true, true, false, false]; // no metal, no oil
        let mut rng = Random::new(99);
        let (_, draws) = pick_goody_resource(&leader, &mut rng);
        assert_eq!(
            draws, 3,
            "FOOD, TIMBER, WEALTH -- KNOWLEDGE skipped, METAL/OIL absent"
        );
    }

    #[test]
    fn no_candidate_falls_back_to_wealth_with_no_draws() {
        let mut leader = LeaderGoody::default();
        leader.type_avail = [false, false, false, true, false, false];
        let mut rng = Random::new(7);
        let before = rng.state();
        let (res, draws) = pick_goody_resource(&leader, &mut rng);
        assert_eq!(res, GOODY_FALLBACK_RESOURCE);
        assert_eq!(draws, 0);
        assert_eq!(
            rng.state(),
            before,
            "the RNG must not move when nothing qualifies"
        );
    }

    #[test]
    fn the_poorest_bucket_wins_when_the_gap_exceeds_the_jitter() {
        // Jitter is 0..24, so a 100-wide gap is decisive whatever the stream does.
        let mut leader = LeaderGoody::default();
        leader.bucket = [500, 500, 500, 0, 100, 500];
        let mut rng = Random::new(0xDEAD_BEEFu32 as i32);
        for _ in 0..500 {
            let (res, _) = pick_goody_resource(&leader, &mut rng);
            assert_eq!(res, RES_METAL);
        }
    }

    #[test]
    fn ties_are_broken_by_the_draw_and_favour_the_earlier_index() {
        // All equal: `score < best` is strict, so index order wins every tie in the
        // jitter. Over many trials every candidate must still appear.
        let leader = LeaderGoody::default();
        let mut rng = Random::new(4242);
        let mut seen = [0usize; NUM_BUCKET_RESOURCES];
        for _ in 0..5_000 {
            let (res, _) = pick_goody_resource(&leader, &mut rng);
            seen[res] += 1;
        }
        assert_eq!(seen[RES_KNOWLEDGE], 0);
        for i in [RES_FOOD, RES_TIMBER, RES_WEALTH, RES_METAL, RES_OIL] {
            assert!(
                seen[i] > 0,
                "resource {i} never won a tie -- the loop is not running"
            );
        }
    }

    #[test]
    fn explore_goody_credits_bucket_and_running_total() {
        let mut g = grid();
        let mut items = Items::new();
        let (x, y) = snap_center(3, 3);
        items.place_goody(&mut g, 3, 3, 0);

        let mut leader = LeaderGoody {
            bucket: [900, 900, 900, 900, 0, 900],
            ..LeaderGoody::default()
        };
        leader.epoch_science = 2;
        let mut rng = Random::new(1);

        let award = explore_goody(
            &mut items,
            &mut g,
            &mut leader,
            &GoodyRules::RETAIL,
            &mut rng,
            x,
            y,
            10,
        )
        .expect("a goody box was there");

        assert_eq!(award.resource, RES_METAL);
        assert_eq!(award.amount, 2 * 25 + 25);
        assert_eq!(award.draws, 5);
        assert_eq!(leader.bucket[RES_METAL], 75);
        assert_eq!(leader.goody_box_resources, 75);
        assert!(!g.has_item_bit(3, 3));
        assert_eq!(items.count_valid(), 0);
        assert_eq!(checksum_items(&items), 1, "channel is empty again");
    }

    #[test]
    fn explore_goody_with_objects_collects_behind_a_live_head() {
        let mut g = grid();
        let mut items = Items::new();
        let (x, y) = snap_center(3, 3);
        let slot = items.place_goody(&mut g, 3, 3, 0);
        g.cell_mut(3, 3).down = 7;
        g.cell_mut(3, 3).down_who = 2;
        let mut objects = TestObjects::default();
        objects.links.insert((7, 2), (DOWN_ITEM, slot as i16));
        let mut leader = LeaderGoody::default();
        let mut rng = Random::new(1);

        let award = explore_goody_with_objects(
            &mut items,
            &mut g,
            &mut objects,
            &mut leader,
            &GoodyRules::RETAIL,
            &mut rng,
            x,
            y,
            10,
        )
        .unwrap()
        .unwrap();

        assert_eq!(award.slot, slot as i32);
        assert_eq!(objects.links[&(7, 2)].0, DOWN_NONE);
        assert_eq!(items.count_valid(), 0);
        assert!(!g.has_item_bit(3, 3));
    }

    #[test]
    fn frame_zero_deletes_the_box_and_pays_nothing() {
        let mut g = grid();
        let mut items = Items::new();
        let (x, y) = snap_center(3, 3);
        items.place_goody(&mut g, 3, 3, 0);
        let mut leader = LeaderGoody::default();
        let mut rng = Random::new(1);
        let before = rng.state();

        let award = explore_goody(
            &mut items,
            &mut g,
            &mut leader,
            &GoodyRules::RETAIL,
            &mut rng,
            x,
            y,
            0,
        );
        assert!(award.is_none());
        assert_eq!(items.count_valid(), 0, "the box is still consumed");
        assert_eq!(leader.bucket, [0; 6]);
        assert_eq!(rng.state(), before, "no draws before the frame check");
    }

    #[test]
    fn direct_explore_preserves_the_retail_caller_precondition() {
        let mut g = grid();
        let mut items = Items::new();
        let mut leader = LeaderGoody::default();
        let mut rng = Random::new(1);
        let before = rng.state();
        let (x, y) = snap_center(3, 3);
        let award = explore_goody(
            &mut items,
            &mut g,
            &mut leader,
            &GoodyRules::RETAIL,
            &mut rng,
            x,
            y,
            10,
        )
        .expect("retail's direct function does not re-check WFLAG_ITEM");

        // `Unit::set_new_location` is what gates this call on WFLAG_ITEM. Calling the
        // callee directly without that precondition still consumes the normal draws and
        // awards resources; adding an early return here would diverge from retail.
        assert_eq!(award.slot, -1);
        assert_eq!(award.draws, 5);
        assert_ne!(rng.state(), before);
        assert_eq!(leader.goody_box_resources, GoodyRules::RETAIL.goody_box);
    }

    /// The lockstep property that matters: two players collecting boxes in the same
    /// order must consume the same RNG stream.
    #[test]
    fn draw_count_is_a_pure_function_of_availability() {
        for mask in 0u8..64 {
            let mut leader = LeaderGoody::default();
            for i in 0..6 {
                leader.type_avail[i] = mask & (1 << i) != 0;
            }
            let expected = (0..6)
                .filter(|&i| leader.type_avail[i] && i != GOODY_EXCLUDED_RESOURCE)
                .count() as u32;
            let mut rng = Random::new(mask as i32 + 1);
            let start = rng.state();
            let (_, draws) = pick_goody_resource(&leader, &mut rng);
            assert_eq!(draws, expected);
            let mut check = Random::new(start);
            for _ in 0..draws {
                check.get(0, 0xFFFF);
            }
            assert_eq!(rng.state(), check.state());
        }
    }

    // -- the scout search ---------------------------------------------------

    #[test]
    fn ring_tables_are_a_complete_chebyshev_disc() {
        assert_eq!(RADIUS[3] as usize, GOODY_SEARCH_CELLS);
        let mut seen = std::collections::HashSet::new();
        for k in 0..GOODY_SEARCH_CELLS {
            assert!(MOVE_X[k].abs() <= 3 && MOVE_Y[k].abs() <= 3);
            assert!(
                seen.insert((MOVE_X[k], MOVE_Y[k])),
                "duplicate offset at {k}"
            );
        }
        assert_eq!(seen.len(), 49, "7x7 block, every cell exactly once");
        // Rings are contiguous prefixes: radius[r] entries cover Chebyshev <= r.
        for (r, &n) in RADIUS.iter().take(4).enumerate() {
            for k in 0..n as usize {
                assert!(MOVE_X[k].abs().max(MOVE_Y[k].abs()) <= r as i32);
            }
        }
    }

    #[test]
    fn find_goody_box_scans_in_ring_order() {
        let mut g = grid();
        let mut items = Items::new();
        // Two boxes: one at Chebyshev 3, one at Chebyshev 1. Ring order must find the
        // near one even though the far one is at a lower array index.
        items.place_goody(&mut g, 8 + 3, 8 + 3, 0);
        items.place_goody(&mut g, 8 + 1, 8, 0);
        let (ux, uy) = snap_center(8, 8);
        let hit = find_goody_box(
            &items,
            &g,
            ux,
            uy,
            0,
            1,
            false,
            |_, _, _| true,
            |_, _, _| false,
        );
        assert_eq!(hit, Some((9, 8)));
    }

    #[test]
    fn full_scout_search_finds_an_item_behind_a_live_head() {
        let mut g = grid();
        let mut items = Items::new();
        let slot = items.place_goody(&mut g, 9, 8, 0);
        g.cell_mut(9, 8).down = 7;
        g.cell_mut(9, 8).down_who = 2;
        let mut objects = TestObjects::default();
        objects.links.insert((7, 2), (DOWN_ITEM, slot as i16));
        let (ux, uy) = snap_center(8, 8);

        let hit = find_goody_box_with_objects(
            &items,
            &g,
            &objects,
            ux,
            uy,
            0,
            0,
            false,
            |_, _, _| false,
            |_, _, _| true,
        )
        .unwrap();
        assert_eq!(hit, Some((9, 8)));
    }

    #[test]
    fn find_goody_box_requires_the_same_region() {
        let mut g = grid();
        let mut items = Items::new();
        items.place_goody(&mut g, 9, 8, 0);
        g.cell_mut(9, 8).region = 5; // unit's home region is 0
        let (ux, uy) = snap_center(8, 8);
        assert_eq!(
            find_goody_box(
                &items,
                &g,
                ux,
                uy,
                0,
                1,
                false,
                |_, _, _| true,
                |_, _, _| false,
            ),
            None
        );
    }

    #[test]
    fn find_goody_box_falls_back_to_ever_seen_when_fogged() {
        let mut g = grid();
        let mut items = Items::new();
        let s = items.place_goody(&mut g, 9, 8, 0);
        let (ux, uy) = snap_center(8, 8);

        // Fully fogged and never seen -> nothing.
        assert_eq!(
            find_goody_box(
                &items,
                &g,
                ux,
                uy,
                0,
                1,
                false,
                |_, _, _| false,
                |_, _, _| false,
            ),
            None
        );
        // Fogged but remembered -> found.
        items.get_mut(s).unwrap().mark_seen(0);
        assert_eq!(
            find_goody_box(
                &items,
                &g,
                ux,
                uy,
                0,
                1,
                false,
                |_, _, _| false,
                |_, _, _| false,
            ),
            Some((9, 8))
        );
        // Remembered by a different player only -> not for us.
        items.get_mut(s).unwrap().ever_seen = 0b0000_0100;
        assert_eq!(
            find_goody_box(
                &items,
                &g,
                ux,
                uy,
                0,
                1,
                false,
                |_, _, _| false,
                |_, _, _| false,
            ),
            None
        );
    }

    #[test]
    fn find_goody_box_asks_fog_at_fog_resolution() {
        let mut g = grid();
        let mut items = Items::new();
        items.place_goody(&mut g, 9, 8, 0);
        let (ux, uy) = snap_center(8, 8);
        let mut asked: Vec<(i32, i32)> = Vec::new();
        let _ = find_goody_box(
            &items,
            &g,
            ux,
            uy,
            0,
            1,
            false,
            |fx, fy, _| {
                asked.push((fx, fy));
                false
            },
            |_, _, _| false,
        );
        // WCoord 9,8 -> fog cells 18..19 x 16..17.
        assert!(asked.contains(&(19, 17)));
        assert!(asked.contains(&(18, 16)));
        assert!(asked
            .iter()
            .all(|&(fx, fy)| (18..=19).contains(&fx) && (16..=17).contains(&fy)));
    }

    #[test]
    fn find_goody_box_ignores_cells_without_the_item_bit() {
        let mut g = grid();
        let mut items = Items::new();
        items.place_goody(&mut g, 9, 8, 0);
        g.cell_mut(9, 8).flags &= !WFLAG_ITEM;
        let (ux, uy) = snap_center(8, 8);
        assert_eq!(
            find_goody_box(
                &items,
                &g,
                ux,
                uy,
                0,
                1,
                false,
                |_, _, _| true,
                |_, _, _| false,
            ),
            None
        );
    }
}
