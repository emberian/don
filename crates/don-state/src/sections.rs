//! Top-level `WalkDataGame::walk_data` (0x005a2360) section chain, as an
//! owned byte-image state tree.
//!
//! Each struct owns the bytes its retail `walk_data` produces: direct ranges
//! are `Vec<u8>` fields, counts/capacities/flags are scalar fields, children
//! are `Vec`s of child structs, `String`s are `Vec<u16>`. Every `walk` is a
//! literal transcription of the retail op sequence and is driven identically
//! by `Loader`, `Saver` and `CheckSum` — the only direction branch is
//! `w.is_loading()`, where stream counts allocate storage before the element
//! loop.
//!
//! walk_test tag bytes are preserved from the stream rather than asserted
//! against fresh-save constants, because populated saves carry different tag
//! values than an empty specimen. Where retail reads a runtime value from a
//! global rather than the stream (GraphicEvents' slot count), the value is an
//! explicit constant with its provenance recorded; unknown bodies fail closed
//! with class/VA/offset.

use crate::prim::{self, Arr, BitMask, Body, PtrVec, Row, SimpleVec};
use crate::walk::{DataWalk, Loader, WalkError};

/// `GraphicEvents::init` (re/decomp-all/008e5390.c) sizes the event root
/// array as `ammo_names.length + GraphicPieces.first_ammo_piece` — driven by
/// the installation's `effects_graphics.xml` (233 AMMO rows), not by the save
/// or the map. The installed fresh game yields 15,368; it is a named constant
/// because retail never serializes it.
pub const GRAPHIC_EVENT_SLOTS: usize = 15_368;

const CLS: &str = "WalkDataGame";

type R = Result<(), WalkError>;

fn tag(w: &mut dyn DataWalk, path: &str, t: &mut u8) -> R {
    w.walk_tag(path, t)
}

// ---------------------------------------------------------------------------
// GameInfo::walk_data 0x005d6570
// ---------------------------------------------------------------------------

#[derive(Default, Clone)]
pub struct PlayerInfo {
    pub tag: u8,
    pub flags: u16,
    pub body: Vec<u8>,
    pub name: Vec<u16>,
}

#[derive(Default, Clone)]
pub struct GameInfo {
    pub tag: u8,
    pub version_string: Vec<u16>,
    /// version + seed + checksum_deep + window_size + failure_threshold + flags
    pub head: Vec<u8>,
    /// gi+0x18..+0x36: 29 single-byte settings + mods byte
    pub settings: Vec<u8>,
    pub players: Vec<PlayerInfo>, // always 8
    // mod block (save_version >= 0x10): 8B + 3 strings + 8B + 1 string;
    // older saves serialize 4 strings and no head/tail.
    pub mod_head: Vec<u8>,
    pub scenario_script: Vec<u16>,
    pub scenario_path: Vec<u16>,
    pub scenario_dir: Vec<u16>, // pre-0x10 only
    pub mod_name: Vec<u16>,
    pub mod_tail: Vec<u8>,
    pub mod2_name: Vec<u16>,
}

impl GameInfo {
    pub(crate) fn walk(&mut self, w: &mut dyn DataWalk, save_version: u32) -> R {
        const VA: u32 = 0x005d6570;
        const C: &str = "GameInfo";
        tag(w, "GameInfo.tag", &mut self.tag)?;
        prim::wstr(w, "GameInfo.version_string", &mut self.version_string, C, VA)?;
        prim::take(w, "GameInfo.head", &mut self.head, 24, C, VA)?;
        prim::take(w, "GameInfo.settings", &mut self.settings, 30, C, VA)?;
        if w.is_loading() && self.players.is_empty() {
            self.players.resize_with(8, PlayerInfo::default);
        }
        for i in 0..8 {
            let p = format!("GameInfo.player[{i}]");
            let pl = &mut self.players[i];
            tag(w, &p, &mut pl.tag)?;
            prim::w_u16(w, &p, VA, &mut pl.flags)?;
            if pl.flags & 1 != 0 {
                prim::take(w, &p, &mut pl.body, 0x39, C, VA)?;
                prim::wstr(w, &p, &mut pl.name, C, VA)?;
            }
        }
        if save_version >= 0x10 {
            prim::take(w, "GameInfo.mod_block", &mut self.mod_head, 8, C, VA)?;
            prim::wstr(w, "GameInfo.mod_block.scenario_script", &mut self.scenario_script, C, VA)?;
            prim::wstr(w, "GameInfo.mod_block.scenario_path", &mut self.scenario_path, C, VA)?;
            prim::wstr(w, "GameInfo.mod_block.mod_name", &mut self.mod_name, C, VA)?;
            prim::take(w, "GameInfo.mod_block.tail", &mut self.mod_tail, 8, C, VA)?;
            prim::wstr(w, "GameInfo.mod_block.mod2_name", &mut self.mod2_name, C, VA)?;
        } else {
            prim::wstr(w, "GameInfo.mod_block.scenario_script", &mut self.scenario_script, C, VA)?;
            prim::wstr(w, "GameInfo.mod_block.scenario_path", &mut self.scenario_path, C, VA)?;
            prim::wstr(w, "GameInfo.mod_block.scenario_dir", &mut self.scenario_dir, C, VA)?;
            prim::wstr(w, "GameInfo.mod_block.mod_name", &mut self.mod_name, C, VA)?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Game::walk_data 0x00589600
// ---------------------------------------------------------------------------

#[derive(Default, Clone)]
pub struct Game {
    pub tag: u8,
    pub info: GameInfo,
    /// game+0x550..+0x6e4 scalars.
    pub scalars: Vec<u8>,
    pub sem_bits: i32,
    pub sem_size: i32,
    pub sem_ptr: Vec<u8>,
    pub graphic_tick: i32,
}

impl Game {
    pub(crate) fn walk(&mut self, w: &mut dyn DataWalk, save_version: u32) -> R {
        const VA: u32 = 0x00589600;
        const C: &str = "Game";
        tag(w, "Game.tag", &mut self.tag)?;
        self.info.walk(w, save_version)?;
        prim::take(w, "Game.scalars", &mut self.scalars, 404, C, VA)?;
        prim::w_i32(w, "Game.semaphore.bits", VA, &mut self.sem_bits)?;
        prim::w_i32(w, "Game.semaphore.size", VA, &mut self.sem_size)?;
        if w.is_loading() && !(0..=32).contains(&self.sem_size) {
            return Err(w.fail(C, VA, format!("semaphore.size {}", self.sem_size)));
        }
        let n = self.sem_size as usize;
        prim::take(w, "Game.semaphore.ptr", &mut self.sem_ptr, n, C, VA)?;
        prim::w_i32(w, "Game.graphic_tick", VA, &mut self.graphic_tick)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// ObjectArray<Tribe>::walk_data 0x0047e230
// ---------------------------------------------------------------------------

#[derive(Default, Clone)]
pub struct Tribe {
    pub tag: u8,
    /// +0x54..+0x70 scalars.
    pub scalars: Vec<u8>,
    /// +0x70..+0x5f0 graft.
    pub graft: Vec<u8>,
    pub names: Vec<Vec<u16>>, // 4
}

impl Body for Tribe {
    fn walk(&mut self, path: &str, w: &mut dyn DataWalk) -> R {
        const VA: u32 = 0x0047e230;
        const C: &str = "Tribe";
        tag(w, path, &mut self.tag)?;
        prim::take(w, path, &mut self.scalars, 7 * 4, C, VA)?;
        prim::take(w, path, &mut self.graft, 352 * 4, C, VA)?;
        if w.is_loading() && self.names.is_empty() {
            self.names.resize_with(4, Vec::new);
        }
        for i in 0..4 {
            prim::wstr(w, path, &mut self.names[i], C, VA)?;
        }
        Ok(())
    }
}

#[derive(Default, Clone)]
pub struct Tribes {
    pub tag: u8,
    pub list: Arr<Tribe>,
}

impl Tribes {
    pub(crate) fn walk(&mut self, w: &mut dyn DataWalk) -> R {
        const VA: u32 = 0x0047e230;
        tag(w, "Tribes.tag", &mut self.tag)?;
        self.list.walk(w, "Tribes", "Tribes", VA)
    }
}

// ---------------------------------------------------------------------------
// Leaders::walk_data 0x006e38e0 — NINE LeaderData slots at stride 0x6eec
// (loop bound is the end address 0x00e789dc; eight was the fresh-save error)
// ---------------------------------------------------------------------------

#[derive(Default, Clone)]
pub struct Leader {
    pub tag: u8,
    pub flags: i32,
    pub flags2: i32,
    /// LeaderData +0x08..+0x692a.
    pub body: Vec<u8>,
    /// diplomacy[8].
    pub diplomacy: Vec<u8>,
    /// personality.
    pub personality: Vec<u8>,
    pub tech: BitMask,
    pub tech_at_start: BitMask,
    pub obs_flags: BitMask,
    pub conquest_wonders: BitMask,
    pub conquest_wonders_in_game: BitMask,
    pub conquest_racial_powers: BitMask,
    pub sites: Arr<Row<0x18>>,
    pub make_list: Arr<Row<0x28>>,
    pub mil_trainers: SimpleVec,
    pub new_rares: SimpleVec,
    pub oil_patches: SimpleVec,
    pub prod_script: Vec<u16>,
    pub rare: BitMask,
    pub rare_owned: BitMask,
    pub rare_conquest: BitMask,
    pub data_encrypted: Vec<u8>,
}

#[derive(Default, Clone)]
pub struct Leaders {
    pub tag: u8,
    pub prod_script_path: Vec<u16>,
    pub slots: Vec<Leader>, // 9
}

impl Leaders {
    pub(crate) fn walk(&mut self, w: &mut dyn DataWalk) -> R {
        const VA: u32 = 0x006e38e0;
        const C: &str = "Leaders";
        tag(w, "Leaders.tag", &mut self.tag)?;
        prim::wstr(w, "Leaders.prod_script_path", &mut self.prod_script_path, C, VA)?;
        if w.is_loading() && self.slots.is_empty() {
            self.slots.resize_with(9, Leader::default);
        }
        for i in 0..9 {
            let p = format!("Leader[{i}]");
            self.slots[i].walk(&p, w)?;
        }
        Ok(())
    }
}

impl Leader {
    /// `LeaderData::walk_data` 0x006d6750 — also driven per-record by the
    /// `leaders` checksum channel (slots 0..8 at 0x00e3a390, stride 0x6eec).
    pub(crate) fn walk(&mut self, path: &str, w: &mut dyn DataWalk) -> R {
        const VA: u32 = 0x006d6750;
        const C: &str = "LeaderData";
        let p = path;
        tag(w, p, &mut self.tag)?;
        prim::w_i32(w, p, VA, &mut self.flags)?;
        prim::w_i32(w, p, VA, &mut self.flags2)?;
        if self.flags & 1 == 0 {
            return Ok(());
        }
        prim::take(w, p, &mut self.body, 0x6922, C, VA)?;
        prim::take(w, p, &mut self.diplomacy, 8 * 0x5c, C, VA)?;
        prim::take(w, p, &mut self.personality, 0x60, C, VA)?;
        for (bm, name) in [
            (&mut self.tech, "tech"),
            (&mut self.tech_at_start, "tech_at_start"),
            (&mut self.obs_flags, "obs_flags"),
            (&mut self.conquest_wonders, "conquest_wonders"),
            (&mut self.conquest_wonders_in_game, "conquest_wonders_in_game"),
            (&mut self.conquest_racial_powers, "conquest_racial_powers"),
        ] {
            bm.walk(w, &format!("{p}.{name}"), C, VA)?;
        }
        self.sites.walk(w, &format!("{p}.sites"), C, VA)?;
        self.make_list.walk(w, &format!("{p}.make_list"), C, VA)?;
        for (sv, name) in [
            (&mut self.mil_trainers, "mil_trainers"),
            (&mut self.new_rares, "new_rares"),
            (&mut self.oil_patches, "oil_patches"),
        ] {
            sv.walk(w, &format!("{p}.{name}"), C, VA, 4)?;
        }
        prim::wstr(w, &format!("{p}.prod_script"), &mut self.prod_script, C, VA)?;
        for (bm, name) in [
            (&mut self.rare, "rare"),
            (&mut self.rare_owned, "rare_owned"),
            (&mut self.rare_conquest, "rare_conquest"),
        ] {
            bm.walk(w, &format!("{p}.{name}"), C, VA)?;
        }
        prim::take(w, &format!("{p}.data_encrypted"), &mut self.data_encrypted, 62 * 4, C, VA)
    }
}

// ---------------------------------------------------------------------------
// small fixed sections
// ---------------------------------------------------------------------------

#[derive(Default, Clone)]
pub struct TileSet {
    pub tag: u8,
    pub name: Vec<u16>,
}

impl TileSet {
    pub(crate) fn walk(&mut self, w: &mut dyn DataWalk) -> R {
        const VA: u32 = 0x0087b290;
        tag(w, "TileSet.tag", &mut self.tag)?;
        prim::wstr(w, "TileSet.name", &mut self.name, "TileSet", VA)
    }
}

#[derive(Default, Clone)]
pub struct Mountains {
    pub tag: u8,
    pub loc_wx: SimpleVec,
    pub loc_wy: SimpleVec,
    pub locs: SimpleVec,
    pub types: SimpleVec,
}

impl Mountains {
    pub(crate) fn walk(&mut self, w: &mut dyn DataWalk) -> R {
        const VA: u32 = 0x0089d320;
        const C: &str = "Mountains";
        tag(w, "Mountains.tag", &mut self.tag)?;
        self.loc_wx.walk(w, "Mountains.loc_wx", C, VA, 4)?;
        self.loc_wy.walk(w, "Mountains.loc_wy", C, VA, 4)?;
        self.locs.walk(w, "Mountains.locs", C, VA, 12)?;
        self.types.walk(w, "Mountains.types", C, VA, 4)
    }
}

// ---------------------------------------------------------------------------
// owner-array sections: tag + 8 presence-planed PtrArrays.
// ---------------------------------------------------------------------------

#[derive(Default, Clone)]
pub struct OwnerLists<T> {
    pub tag: u8,
    pub lists: Vec<PtrVec<T>>, // 8
}

impl<T: Default + Body + Clone> OwnerLists<T> {
    pub(crate) fn walk(&mut self, w: &mut dyn DataWalk, class: &'static str, va: u32) -> R {
        tag(w, &format!("{class}.tag"), &mut self.tag)?;
        if w.is_loading() && self.lists.is_empty() {
            self.lists.resize_with(8, PtrVec::default);
        }
        for i in 0..8 {
            self.lists[i].walk(w, &format!("{class}.lists[{i}]"), class, va)?;
        }
        Ok(())
    }
}

#[derive(Default, Clone)]
pub struct Army {
    pub tag: u8,
    pub valid: i16,
    /// Army +0x02..+0x98.
    pub body: Vec<u8>,
}

impl Body for Army {
    fn walk(&mut self, path: &str, w: &mut dyn DataWalk) -> R {
        const VA: u32 = 0x006f3700;
        tag(w, path, &mut self.tag)?;
        prim::w_i16(w, "Army.valid", VA, &mut self.valid)?;
        if self.valid != 0 {
            prim::take(w, "Army.body", &mut self.body, 150, "Army", VA)?;
        }
        Ok(())
    }
}

#[derive(Default, Clone)]
pub struct City {
    pub flags: u16,
    /// CityData +6..+114.
    pub pod: Vec<u8>,
    pub name: Vec<u16>,
    pub id: Vec<u16>,
    pub vans: Arr<Row<8>>,
}

impl Body for City {
    fn walk(&mut self, _path: &str, w: &mut dyn DataWalk) -> R {
        const VA: u32 = 0x00735410;
        prim::w_u16(w, "City.flags", VA, &mut self.flags)?;
        if self.flags & 1 != 0 {
            prim::take(w, "City.pod", &mut self.pod, 108, "City", VA)?;
            // City::walk_data gates the name/id strings on !is_checksum
            // (re/decomp-all/00937600.c — `param_1[2] == 0` branch).
            if !w.is_checksum() {
                prim::wstr(w, "City.name", &mut self.name, "City", VA)?;
                prim::wstr(w, "City.id", &mut self.id, "City", VA)?;
            }
            self.vans.walk(w, "City.vans", "City", VA)?;
        }
        Ok(())
    }
}

#[derive(Default, Clone)]
pub struct Form {
    pub tag: u8,
    pub name: Vec<u16>,
    pub desc: Vec<u16>,
    /// +0x28..+0xe90.
    pub data: Vec<u8>,
}

impl Body for Form {
    fn walk(&mut self, path: &str, w: &mut dyn DataWalk) -> R {
        const VA: u32 = 0x00481190;
        tag(w, path, &mut self.tag)?;
        prim::wstr(w, "Form.name", &mut self.name, "Form", VA)?;
        prim::wstr(w, "Form.desc", &mut self.desc, "Form", VA)?;
        prim::take(w, "Form.data", &mut self.data, 0xe90 - 0x28, "Form", VA)
    }
}

#[derive(Default, Clone)]
pub struct Forms {
    pub tag: u8,
    pub list: Arr<Form>,
}

impl Forms {
    pub(crate) fn walk(&mut self, w: &mut dyn DataWalk) -> R {
        const VA: u32 = 0x00481190;
        tag(w, "Forms.tag", &mut self.tag)?;
        self.list.walk(w, "Forms", "Forms", VA)
    }
}

/// Goods / Items share the body grammar:
/// tag + ever_seen u8 + SubObject tag + flags u8 + must_walk u8;
/// if must_walk: who u8 + o i16 + z,x,y i32 + type_index i32.
#[derive(Default, Clone)]
pub struct GoodItem {
    pub tag: u8,
    pub ever_seen: u8,
    pub sub_tag: u8,
    pub flags: u8,
    pub must_walk: u8,
    /// SubObject gated body: who,o,z,x,y,type_index.
    pub body: Vec<u8>,
}

impl Body for GoodItem {
    fn walk(&mut self, path: &str, w: &mut dyn DataWalk) -> R {
        tag(w, path, &mut self.tag)?;
        prim::w_u8(w, "ever_seen", 0, &mut self.ever_seen)?;
        tag(w, "SubObject.tag", &mut self.sub_tag)?;
        prim::w_u8(w, "flags", 0, &mut self.flags)?;
        prim::w_u8(w, "must_walk", 0, &mut self.must_walk)?;
        if self.must_walk > 1 {
            return Err(w.fail("GoodItem", 0, "must_walk not boolean".to_string()));
        }
        if self.must_walk == 1 {
            prim::take(w, "SubObject.body", &mut self.body, 19, "GoodItem", 0)?;
        }
        Ok(())
    }
}

/// Hero/Special row: active_spells SimpleArray(12B) then 6B slot scalars.
#[derive(Default, Clone)]
pub struct HeroLike {
    pub active_spells: SimpleVec,
    pub slot: Vec<u8>,
}

impl Body for HeroLike {
    fn walk(&mut self, path: &str, w: &mut dyn DataWalk) -> R {
        self.active_spells.walk(w, &format!("{path}.active_spells"), "Hero", 0, 12)?;
        prim::take(w, path, &mut self.slot, 6, "Hero", 0)
    }
}

#[derive(Default, Clone)]
pub struct HerdLike {
    /// cx,cy,wx,wy,type,good i32 + herd i16 + flags i8.
    pub slot: Vec<u8>,
}

impl Body for HerdLike {
    fn walk(&mut self, path: &str, w: &mut dyn DataWalk) -> R {
        prim::take(w, path, &mut self.slot, 27, "Herd", 0)
    }
}

#[derive(Default, Clone)]
pub struct Caravan {
    pub tag: u8,
    /// city2..o i16s + flags,who i8.
    pub head: Vec<u8>,
    /// making_road, reset_road.
    pub roads: Vec<u8>,
    /// Stack<PathData>: i32 capacity, i32 length, i8 increment, len*16.
    pub cap: i32,
    pub len: i32,
    pub inc: u8,
    pub road: Vec<u8>,
}

impl Body for Caravan {
    fn walk(&mut self, path: &str, w: &mut dyn DataWalk) -> R {
        const C: &str = "Caravan";
        tag(w, path, &mut self.tag)?;
        prim::take(w, "Caravan.head", &mut self.head, 14, C, 0)?;
        prim::take(w, "Caravan.roads", &mut self.roads, 8, C, 0)?;
        if !w.is_loading() {
            self.len = (self.road.len() / 16) as i32;
        }
        prim::w_i32(w, "Caravan.road.cap", 0, &mut self.cap)?;
        prim::w_i32(w, "Caravan.road.len", 0, &mut self.len)?;
        prim::w_u8(w, "Caravan.road.inc", 0, &mut self.inc)?;
        if w.is_loading()
            && (self.len < 0 || self.cap < 0 || self.len > self.cap || self.cap > 1 << 20)
        {
            return Err(w.fail(C, 0, format!("road stack cap={} len={}", self.cap, self.len)));
        }
        let n = self.len as usize * 16;
        prim::take(w, "Caravan.road.data", &mut self.road, n, C, 0)
    }
}

#[derive(Default, Clone)]
pub struct Land {
    pub tag: u8,
    pub pod: Vec<u8>,
    pub name: Vec<u16>,
    pub key: Vec<u16>,
}

impl Body for Land {
    fn walk(&mut self, path: &str, w: &mut dyn DataWalk) -> R {
        tag(w, path, &mut self.tag)?;
        prim::take(w, "Land.pod", &mut self.pod, 264, "Land", 0)?;
        prim::wstr(w, "Land.name", &mut self.name, "Land", 0)?;
        prim::wstr(w, "Land.key", &mut self.key, "Land", 0)
    }
}

#[derive(Default, Clone)]
pub struct Lands {
    pub tag: u8,
    pub list: Arr<Land>,
}

impl Lands {
    pub(crate) fn walk(&mut self, w: &mut dyn DataWalk) -> R {
        tag(w, "Lands.tag", &mut self.tag)?;
        self.list.walk(w, "Lands.list", "Lands", 0)
    }
}

// ---------------------------------------------------------------------------
// LeaderOptions (10 slots) / OptionInfo (331 rows)
// ---------------------------------------------------------------------------

#[derive(Default, Clone)]
pub struct LeaderOption {
    pub tag: u8,
    /// who, peasants, peasants_wait, buildings.
    pub head: Vec<u8>,
    /// BitMask<32>.
    pub bits: i32,
    pub size: i32,
    pub flags: Vec<u8>,
}

#[derive(Default, Clone)]
pub struct LeaderOptions {
    pub tag: u8,
    pub slots: Vec<LeaderOption>, // 10
}

impl LeaderOptions {
    pub(crate) fn walk(&mut self, w: &mut dyn DataWalk) -> R {
        const C: &str = "LeaderOptions";
        tag(w, "LeaderOptions.tag", &mut self.tag)?;
        if w.is_loading() && self.slots.is_empty() {
            self.slots.resize_with(10, LeaderOption::default);
        }
        for i in 0..10 {
            let s = &mut self.slots[i];
            let p = format!("LeaderOption[{i}]");
            tag(w, &p, &mut s.tag)?;
            prim::take(w, &p, &mut s.head, 16, C, 0)?;
            prim::w_i32(w, &p, 0, &mut s.bits)?;
            prim::w_i32(w, &p, 0, &mut s.size)?;
            if w.is_loading() && (s.bits < 0 || s.bits > 32 || s.size != (s.bits + 7) / 8) {
                return Err(w.fail(
                    C,
                    0,
                    format!("flags BitMask bits={} size={}", s.bits, s.size),
                ));
            }
            let n = s.size as usize;
            prim::take(w, &p, &mut s.flags, n, C, 0)?;
        }
        Ok(())
    }
}

#[derive(Default, Clone)]
pub struct OptionRow {
    pub tag: u8,
    pub a: Vec<u16>,
    pub b: Vec<u16>,
}

#[derive(Default, Clone)]
pub struct OptionInfo {
    pub tag: u8,
    pub rows: Vec<OptionRow>, // 331
}

impl OptionInfo {
    pub(crate) fn walk(&mut self, w: &mut dyn DataWalk) -> R {
        const VA: u32 = 0x0072c1e0;
        const C: &str = "OptionInfo";
        tag(w, "OptionInfo.tag", &mut self.tag)?;
        if w.is_loading() && self.rows.is_empty() {
            self.rows.resize_with(331, OptionRow::default);
        }
        for i in 0..331 {
            let r = &mut self.rows[i];
            let p = format!("OptionData[{i}]");
            tag(w, &p, &mut r.tag)?;
            prim::wstr(w, &p, &mut r.a, C, VA)?;
            prim::wstr(w, &p, &mut r.b, C, VA)?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Pathfinder direct block + specialized Array<Group> (0x0047ea30) + tail
// ---------------------------------------------------------------------------

/// Group::walk_data 0x00708400: 72-byte header then num-dependent rows.
#[derive(Default, Clone)]
pub struct Group {
    pub hdr: Vec<u8>, // 72
    pub list: Vec<u8>,
    pub off_x: Vec<u8>,
    pub off_y: Vec<u8>,
    pub curr_x: Vec<u8>,
    pub curr_y: Vec<u8>,
    pub angles: Vec<u8>,
}

impl Group {
    fn num(&self) -> i32 {
        i32::from_le_bytes(self.hdr[8..12].try_into().unwrap_or([0; 4]))
    }

    pub(crate) fn walk(&mut self, path: &str, w: &mut dyn DataWalk, class: &'static str) -> R {
        const VA: u32 = 0x00708400;
        prim::take(w, path, &mut self.hdr, 72, class, VA)?;
        let num = self.num();
        if !(0..=128).contains(&num) {
            return Err(w.fail(class, VA, format!("Group.num {num}")));
        }
        let n = num as usize;
        prim::take(w, path, &mut self.list, n * 2, class, VA)?; // list i16[num]
        for v in [&mut self.off_x, &mut self.off_y, &mut self.curr_x, &mut self.curr_y] {
            prim::take(w, path, v, n * 4, class, VA)?; // off_x..curr_y i32[num]
        }
        prim::take(w, path, &mut self.angles, n, class, VA) // angles i8[num]
    }
}

impl Body for Group {
    fn walk(&mut self, path: &str, w: &mut dyn DataWalk) -> R {
        self.walk(path, w, "Group")
    }
}

#[derive(Default, Clone)]
pub struct Groups {
    pub list: Arr<Group>,
    pub tail_tag: u8,
    pub last_group: Vec<u8>,
    pub proc_group: i32,
}

impl Groups {
    pub(crate) fn walk(&mut self, w: &mut dyn DataWalk) -> R {
        const VA: u32 = 0x0047ea30;
        const C: &str = "Groups";
        self.list.walk(w, "Groups.list", C, VA)?;
        tag(w, "Groups.tail.tag", &mut self.tail_tag)?;
        prim::take(w, "Groups.last_group", &mut self.last_group, 8 * 4, C, VA)?;
        prim::w_i32(w, "Groups.proc_group", VA, &mut self.proc_group)
    }
}

// ---------------------------------------------------------------------------
// Objects::walk_data — nine MultiPtrArray<Object> lists, Ammo ptr array,
// DeathObj ObjectArray. Object bodies dispatch on the serialized concrete
// type code; unknown types fail closed with the exact offset.
// ---------------------------------------------------------------------------

#[derive(Default, Clone)]
pub struct SubObj {
    pub tag: u8,
    pub flags: u8,
    pub gate: u8,
    /// who,o,z,x,y,ptype_index.
    pub body: Vec<u8>,
}

impl SubObj {
    pub(crate) fn walk(&mut self, path: &str, w: &mut dyn DataWalk, class: &'static str) -> R {
        let p = format!("{path}.SubObject");
        tag(w, &p, &mut self.tag)?;
        prim::w_u8(w, &p, 0, &mut self.flags)?;
        prim::w_u8(w, &p, 0, &mut self.gate)?;
        if self.gate > 1 {
            return Err(w.fail(class, 0, "SubObject must_walk not boolean".to_string()));
        }
        if self.gate == 1 {
            prim::take(w, &p, &mut self.body, 19, class, 0)?;
        }
        Ok(())
    }
}

#[derive(Default, Clone)]
pub struct ObjBase {
    pub sub: SubObj,
    pub tag: u8,
    pub gate: u8,
    /// ObjectData +0x20..+0x42.
    pub mid: Vec<u8>,
    pub launch: u8,
    pub launching: SimpleVec,
}

impl ObjBase {
    pub(crate) fn walk(&mut self, path: &str, w: &mut dyn DataWalk, class: &'static str) -> R {
        self.sub.walk(path, w, class)?;
        let p = format!("{path}.Object");
        tag(w, &p, &mut self.tag)?;
        prim::w_u8(w, &p, 0, &mut self.gate)?;
        if self.gate > 1 {
            return Err(w.fail(class, 0, "Object must_walk not boolean".to_string()));
        }
        if self.gate == 1 {
            prim::take(w, &p, &mut self.mid, 34, class, 0)?;
        }
        prim::w_u8(w, &p, 0, &mut self.launch)?;
        if self.launch > 1 {
            return Err(w.fail(class, 0, "Object launching flag not boolean".to_string()));
        }
        if self.launch == 1 {
            self.launching.walk(w, &format!("{p}.launching"), class, 0, 4)?;
        }
        Ok(())
    }
}

/// Stack<PathData>: i32 capacity, i32 length, i8 increment, 16B records.
#[derive(Default, Clone)]
pub struct PathStack {
    pub cap: i32,
    pub len: i32,
    pub inc: u8,
    pub data: Vec<u8>,
}

impl PathStack {
    pub(crate) fn walk(&mut self, path: &str, w: &mut dyn DataWalk, class: &'static str) -> R {
        if !w.is_loading() {
            self.len = (self.data.len() / 16) as i32;
        }
        prim::w_i32(w, path, 0, &mut self.cap)?;
        prim::w_i32(w, path, 0, &mut self.len)?;
        prim::w_u8(w, path, 0, &mut self.inc)?;
        if w.is_loading()
            && (self.len < 0 || self.cap < 0 || self.len > self.cap || self.cap > 1 << 20)
        {
            return Err(w.fail(class, 0, format!("PathData cap={} len={}", self.cap, self.len)));
        }
        let n = self.len as usize * 16;
        prim::take(w, path, &mut self.data, n, class, 0)
    }
}

#[derive(Default, Clone)]
pub struct Order {
    /// Concrete OrderIndex.
    pub ty: i32,
    /// RecycledOrderNode.metric.
    pub metric: u8,
    /// Concrete payload including the inherited UnitOrder::flags byte.
    pub payload: Vec<u8>,
}

#[derive(Default, Clone)]
pub struct OrderList {
    pub count: i32,
    pub orders: Vec<Order>,
}

impl OrderList {
    pub(crate) fn walk(&mut self, path: &str, w: &mut dyn DataWalk, class: &'static str) -> R {
        if !w.is_loading() {
            self.count = self.orders.len() as i32;
        }
        prim::w_i32(w, path, 0, &mut self.count)?;
        if !(0..=65_536).contains(&self.count) {
            return Err(w.fail(class, 0, format!("OrderList count {}", self.count)));
        }
        if w.is_loading() {
            self.orders.clear();
            self.orders.resize_with(self.count as usize, Order::default);
        }
        for i in 0..self.orders.len() {
            let o = &mut self.orders[i];
            let p = format!("{path}[{i}]");
            prim::w_i32(w, &p, 0, &mut o.ty)?;
            prim::w_u8(w, &p, 0, &mut o.metric)?;
            // Payload sizes from savegame-unit-orderlist-census.md (include
            // the inherited flags byte): MoveOrder family, TargetOrder,
            // GatherOrder (Target+20), CastOrder (Target+16).
            let size = match o.ty {
                1..=4 => 77,
                6 => 11,
                7 => 31,
                14 => 27,
                _ => {
                    return Err(w.fail(
                        class,
                        0,
                        format!("unknown OrderIndex {} — concrete order body has no decoder", o.ty),
                    ))
                }
            };
            prim::take(w, &p, &mut o.payload, size, class, 0)?;
        }
        Ok(())
    }
}

#[derive(Default, Clone)]
pub struct Unit {
    pub base: ObjBase,
    pub tag: u8,
    pub gate: u8,
    /// UnitData +0x48..+0xb7.
    pub body: Vec<u8>,
    pub path: PathStack,
    pub orders: OrderList,
    /// PtrArray<Guy>: 155-byte GuyData rows.
    pub guys: PtrVec<Row<155>>,
}

impl Unit {
    pub(crate) fn walk(&mut self, path: &str, w: &mut dyn DataWalk, class: &'static str) -> R {
        self.base.walk(path, w, class)?;
        let p = format!("{path}.Unit");
        tag(w, &p, &mut self.tag)?;
        prim::w_u8(w, &p, 0, &mut self.gate)?;
        if self.gate > 1 {
            return Err(w.fail(class, 0, "Unit must_walk not boolean".to_string()));
        }
        if self.base.sub.gate == 0 || self.gate == 0 {
            return Ok(());
        }
        prim::take(w, &p, &mut self.body, 111, class, 0)?;
        self.path.walk(&format!("{p}.path"), w, class)?;
        self.orders.walk(&format!("{p}.orders"), w, class)?;
        self.guys.walk(w, &format!("{p}.guys"), class, 0)
    }
}

#[derive(Default, Clone)]
pub struct GatherPt {
    /// list discriminator.
    pub disc: i32,
    /// LLNode metric.
    pub metric: u8,
    pub body: Vec<u8>,
}

#[derive(Default, Clone)]
pub struct Build {
    /// founder, max_age — emitted before the Wall base.
    pub head: Vec<u8>,
    pub base: ObjBase,
    pub wall_tag: u8,
    pub wall_gate: u8,
    /// WallData gated body.
    pub wall_body: Vec<u8>,
    pub build_tag: u8,
    pub build_gate: u8,
    /// BuildData gated head.
    pub body: Vec<u8>,
    pub queue_tag: u8,
    pub queue_len: i32,
    /// 18B BuildQueue rows.
    pub queue: Vec<u8>,
    /// MiningList::mtn, cliff.
    pub mining_head: Vec<u8>,
    pub mlen: i32,
    pub mcap: i32,
    pub minc: i16,
    pub mflags: u8,
    /// 8B mining rows.
    pub mining: Vec<u8>,
    pub gather_len: i32,
    pub gathers: Vec<GatherPt>,
    pub orig_type: i32,
}

impl Build {
    pub(crate) fn walk(&mut self, path: &str, w: &mut dyn DataWalk, class: &'static str) -> R {
        prim::take(w, path, &mut self.head, 2, class, 0)?;
        self.base.walk(path, w, class)?;
        let wp = format!("{path}.Wall");
        tag(w, &wp, &mut self.wall_tag)?;
        prim::w_u8(w, &wp, 0, &mut self.wall_gate)?;
        if self.wall_gate > 1 {
            return Err(w.fail(class, 0, "Wall must_walk not boolean".to_string()));
        }
        if self.wall_gate == 1 {
            prim::take(w, &wp, &mut self.wall_body, 30, class, 0)?;
        }
        let p = format!("{path}.Build");
        tag(w, &p, &mut self.build_tag)?;
        prim::w_u8(w, &p, 0, &mut self.build_gate)?;
        if self.build_gate > 1 {
            return Err(w.fail(class, 0, "Build must_walk not boolean".to_string()));
        }
        if self.base.sub.gate == 0 || self.build_gate == 0 {
            return Ok(());
        }
        prim::take(w, &p, &mut self.body, 22, class, 0)?;
        let q = format!("{p}.queue");
        tag(w, &q, &mut self.queue_tag)?;
        if !w.is_loading() {
            self.queue_len = (self.queue.len() / 18) as i32;
        }
        prim::w_i32(w, &q, 0, &mut self.queue_len)?;
        if !(0..=65_536).contains(&self.queue_len) {
            return Err(w.fail(class, 0, format!("BuildQueue size {}", self.queue_len)));
        }
        let n = self.queue_len as usize * 18;
        prim::take(w, &q, &mut self.queue, n, class, 0)?;
        prim::take(w, &q, &mut self.mining_head, 2, class, 0)?; // mtn, cliff
        let mp = format!("{p}.mining");
        if !w.is_loading() {
            self.mlen = (self.mining.len() / 8) as i32;
        }
        prim::w_i32(w, &mp, 0, &mut self.mlen)?;
        if !(0..=65_536).contains(&self.mlen) {
            return Err(w.fail(class, 0, format!("MiningList len {}", self.mlen)));
        }
        if self.mlen != 0 {
            prim::w_i32(w, &mp, 0, &mut self.mcap)?;
            prim::w_i16(w, &mp, 0, &mut self.minc)?;
            prim::w_u8(w, &mp, 0, &mut self.mflags)?;
            if w.is_loading() && self.mcap < self.mlen {
                return Err(w.fail(
                    class,
                    0,
                    format!("MiningList cap {} < len {}", self.mcap, self.mlen),
                ));
            }
            let n = self.mlen as usize * 8;
            prim::take(w, &mp, &mut self.mining, n, class, 0)?;
        }
        let gp = format!("{p}.gather");
        if !w.is_loading() {
            self.gather_len = self.gathers.len() as i32;
        }
        prim::w_i32(w, &gp, 0, &mut self.gather_len)?;
        if !(0..=65_536).contains(&self.gather_len) {
            return Err(w.fail(class, 0, format!("GatherPointList {}", self.gather_len)));
        }
        if w.is_loading() {
            self.gathers.clear();
            self.gathers.resize_with(self.gather_len as usize, GatherPt::default);
        }
        for g in self.gathers.iter_mut() {
            prim::w_i32(w, &gp, 0, &mut g.disc)?;
            prim::w_u8(w, &gp, 0, &mut g.metric)?;
            prim::take(w, &gp, &mut g.body, 9, class, 0)?;
        }
        prim::w_i32(w, &format!("{p}.orig_type"), 0, &mut self.orig_type)
    }
}

/// Animal::walk_data 0x005d7ea0: inherited Unit body (FUN_0060cf40), then a
/// walk_test tag, a computed/persisted must_walk gate, and — when the gate
/// holds — the direct range `[this+0x150, this+0x155)`. The decomp's
/// `in_ECX` is `int*`, so `in_ECX + 0x54` is byte offset +0x150: exactly the
/// five AnimalData bytes (ox, whom, aid) observed in the saves.
#[derive(Default, Clone)]
pub struct Animal {
    pub unit: Unit,
    pub tag: u8,
    pub gate: u8,
    /// AnimalData +0x150..+0x155: ox i16, whom i16, aid i8.
    pub tail: Vec<u8>,
}

impl Animal {
    pub(crate) fn walk(&mut self, path: &str, w: &mut dyn DataWalk, class: &'static str) -> R {
        self.unit.walk(path, w, class)?;
        let p = format!("{path}.Animal");
        tag(w, &p, &mut self.tag)?;
        prim::w_u8(w, &p, 0, &mut self.gate)?;
        if self.gate > 1 {
            return Err(w.fail(class, 0, "Animal must_walk not boolean".to_string()));
        }
        if self.gate == 1 {
            prim::take(w, &p, &mut self.tail, 0x155 - 0x150, class, 0)?;
        }
        Ok(())
    }
}

/// A serialized Object body, dispatched on the concrete type plane.
#[derive(Clone)]
pub enum Obj {
    Unit(Box<Unit>),
    Build(Box<Build>),
    Animal(Box<Animal>),
}

impl Obj {
    pub fn ty(&self) -> i32 {
        match self {
            Obj::Unit(_) => 0,
            Obj::Build(_) => 1,
            Obj::Animal(_) => 3,
        }
    }
    /// SubObject flags byte — retail tests `*(byte*)(this + 8) & 1` on the
    /// object for the units/guys channels.
    pub fn obj_flags(&self) -> u8 {
        match self {
            Obj::Unit(u) => u.base.sub.flags,
            Obj::Build(b) => b.base.sub.flags,
            Obj::Animal(a) => a.unit.base.sub.flags,
        }
    }
    pub(crate) fn walk(&mut self, path: &str, w: &mut dyn DataWalk, class: &'static str) -> R {
        match self {
            Obj::Unit(u) => u.walk(path, w, class),
            Obj::Build(b) => b.walk(path, w, class),
            Obj::Animal(a) => a.walk(path, w, class),
        }
    }
}

/// `MultiPtrArray<Object>`: header, presence plane, `i32 concrete_type` per
/// present slot, repeated history, then virtual bodies.
#[derive(Default, Clone)]
pub struct ObjList {
    pub len: i32,
    pub cap: i32,
    pub inc: i16,
    pub flags: u8,
    pub present: Vec<u8>,
    /// Concrete type per present slot, in stream order.
    pub types: Vec<i32>,
    pub cap2: i32,
    pub inc2: i16,
    /// Indexed by slot; absent slots stay `None`.
    pub elems: Vec<Option<Obj>>,
}

impl ObjList {
    pub(crate) fn walk(&mut self, path: &str, w: &mut dyn DataWalk, class: &'static str) -> R {
        if !w.is_loading() {
            self.len = self.elems.len() as i32;
        }
        prim::w_i32(w, path, 0, &mut self.len)?;
        if w.is_loading() && !(0..=4_000_000).contains(&self.len) {
            return Err(w.fail(class, 0, format!("MultiPtrArray length {}", self.len)));
        }
        if self.len == 0 {
            return Ok(());
        }
        prim::w_i32(w, path, 0, &mut self.cap)?;
        prim::w_i16(w, path, 0, &mut self.inc)?;
        prim::w_u8(w, path, 0, &mut self.flags)?;
        if w.is_loading() && self.cap < self.len {
            return Err(w.fail(
                class,
                0,
                format!("MultiPtrArray capacity {} < length {}", self.cap, self.len),
            ));
        }
        let n = self.len as usize;
        prim::take(w, path, &mut self.present, n, class, 0)?;
        if self.present.iter().any(|&v| v > 1) {
            return Err(w.fail(class, 0, "non-boolean presence byte".to_string()));
        }
        if w.is_loading() {
            let np = self.present.iter().filter(|&&v| v != 0).count();
            self.types.clear();
            self.types.resize(np, 0);
        }
        for t in self.types.iter_mut() {
            prim::w_i32(w, path, 0, t)?;
        }
        prim::w_i32(w, path, 0, &mut self.cap2)?;
        prim::w_i16(w, path, 0, &mut self.inc2)?;
        if self.cap2 != self.cap || self.inc2 != self.inc {
            return Err(w.fail(
                class,
                0,
                format!(
                    "repeated history ({},{}) != header ({},{})",
                    self.cap2, self.inc2, self.cap, self.inc
                ),
            ));
        }
        if w.is_loading() {
            self.elems.clear();
            self.elems.resize_with(self.len as usize, || None);
        }
        let mut tys = self.types.iter().copied();
        for (i, &p) in self.present.clone().iter().enumerate() {
            if p == 0 {
                continue;
            }
            let bp = format!("{path}[{i}]");
            let ty = tys.next().unwrap_or_else(|| self.elems[i].as_ref().map(Obj::ty).unwrap_or(-1));
            if w.is_loading() {
                self.elems[i] = Some(match ty {
                    0 => Obj::Unit(Box::new(Unit::default())),
                    1 => Obj::Build(Box::new(Build::default())),
                    3 => Obj::Animal(Box::new(Animal::default())),
                    _ => {
                        return Err(w.fail(
                            class,
                            0,
                            format!("slot {i} concrete type {ty} has no walk_data decoder"),
                        ))
                    }
                });
            }
            self.elems[i].as_mut().unwrap().walk(&bp, w, class)?;
        }
        Ok(())
    }
}

#[derive(Default, Clone)]
pub struct Spline {
    pub tag: u8,
    /// SplineData +64..+100.
    pub head: Vec<u8>,
    pub control_verts: Arr<Row<12>>,
    pub knots: SimpleVec,
    pub spline_knots: SimpleVec,
    pub weights: SimpleVec,
    pub spline_verts: Arr<Row<12>>,
    pub spline_normals: Arr<Row<12>>,
}

impl Spline {
    pub(crate) fn walk(&mut self, path: &str, w: &mut dyn DataWalk, class: &'static str) -> R {
        let p = format!("{path}.Spline");
        tag(w, &p, &mut self.tag)?;
        prim::take(w, &p, &mut self.head, 36, class, 0)?;
        self.control_verts.walk(w, &format!("{p}.control_verts"), class, 0)?;
        self.knots.walk(w, &format!("{p}.knots"), class, 0, 4)?;
        self.spline_knots.walk(w, &format!("{p}.spline_knots"), class, 0, 4)?;
        self.weights.walk(w, &format!("{p}.weights"), class, 0, 4)?;
        self.spline_verts.walk(w, &format!("{p}.spline_verts"), class, 0)?;
        self.spline_normals.walk(w, &format!("{p}.spline_normals"), class, 0)
    }
}

#[derive(Default, Clone)]
pub struct Ammo {
    pub tag: u8,
    pub flags: u8,
    /// AmmoData +5..+104, walked when flags & 3.
    pub body: Vec<u8>,
    pub has_path: u8,
    pub spline: Option<Box<Spline>>,
}

impl Body for Ammo {
    fn walk(&mut self, path: &str, w: &mut dyn DataWalk) -> R {
        const C: &str = "Ammo";
        let p = format!("{path}.Ammo");
        tag(w, &p, &mut self.tag)?;
        prim::w_u8(w, &p, 0, &mut self.flags)?;
        if self.flags & 3 != 0 {
            prim::take(w, &p, &mut self.body, 99, C, 0)?;
        }
        prim::w_u8(w, &p, 0, &mut self.has_path)?;
        if self.has_path > 1 {
            return Err(w.fail(C, 0, "Ammo ammo_path flag not boolean".to_string()));
        }
        if self.has_path == 1 {
            if w.is_loading() && self.spline.is_none() {
                self.spline = Some(Box::default());
            }
            self.spline.as_mut().unwrap().walk(&p, w, C)?;
        }
        Ok(())
    }
}

#[derive(Default, Clone)]
pub struct DeathRow {
    pub tag: u8,
    pub valid: i32,
    /// DeathObjData +4..+75.
    pub body: Vec<u8>,
}

impl Body for DeathRow {
    fn walk(&mut self, path: &str, w: &mut dyn DataWalk) -> R {
        let p = format!("{path}.DeathObj");
        tag(w, &p, &mut self.tag)?;
        prim::w_i32(w, &p, 0, &mut self.valid)?;
        if self.valid != 0 {
            prim::take(w, &p, &mut self.body, 71, "DeathObj", 0)?;
        }
        Ok(())
    }
}

#[derive(Default, Clone)]
pub struct Objects {
    pub tag: u8,
    /// valid, ammo_index, good_mark, rare_mark, unit/build/wall_mark[9],
    /// obj_ctr[9]u16.
    pub scalars: Vec<u8>,
    pub lists: Vec<ObjList>, // 9
    pub ammo: PtrVec<Ammo>,
    pub deaths: Arr<DeathRow>,
}

impl Objects {
    pub(crate) fn walk(&mut self, w: &mut dyn DataWalk) -> R {
        const C: &str = "Objects";
        tag(w, "Objects.tag", &mut self.tag)?;
        prim::take(w, "Objects.scalars", &mut self.scalars, 4 * (2 + 2 + 9 + 9 + 9) + 9 * 2, C, 0)?;
        if w.is_loading() && self.lists.is_empty() {
            self.lists.resize_with(9, ObjList::default);
        }
        for owner in 0..9 {
            self.lists[owner].walk(&format!("Objects.lists[{owner}]"), w, C)?;
        }
        self.ammo.walk(w, "Objects.ammo", C, 0)?;
        self.deaths.walk(w, "Objects.deaths", C, 0)
    }
}

// ---------------------------------------------------------------------------
// HotKeyGroups: tag + Array<HotKeyGroup> — Group projection plus row tag,
// loc_x/loc_y IEEE bits and valid.
// ---------------------------------------------------------------------------

#[derive(Default, Clone)]
pub struct HotKey {
    pub group: Group,
    pub tag: u8,
    /// loc_x, loc_y.
    pub loc: Vec<u8>,
    pub valid: i32,
}

impl Body for HotKey {
    fn walk(&mut self, path: &str, w: &mut dyn DataWalk) -> R {
        self.group.walk(path, w, "HotKeyGroup")?;
        tag(w, &format!("{path}.tag"), &mut self.tag)?;
        prim::take(w, &format!("{path}.loc"), &mut self.loc, 8, "HotKeyGroup", 0)?;
        prim::w_i32(w, &format!("{path}.valid"), 0, &mut self.valid)
    }
}

#[derive(Default, Clone)]
pub struct HotKeyGroups {
    pub tag: u8,
    pub list: Arr<HotKey>,
}

impl HotKeyGroups {
    pub(crate) fn walk(&mut self, w: &mut dyn DataWalk) -> R {
        tag(w, "HotKeyGroups.tag", &mut self.tag)?;
        self.list.walk(w, "HotKeyGroups.list", "HotKeyGroups", 0)
    }
}

// ---------------------------------------------------------------------------
// World::walk_data(visitor, -1) — all phases.
// ---------------------------------------------------------------------------

#[derive(Default, Clone)]
pub struct CollBlock {
    pub present: i32,
    pub bits: i32,
    pub size: i32,
    pub data: Vec<u8>,
}

#[derive(Default, Clone)]
pub struct World {
    pub tag: u8,
    pub xs: i32,
    pub ys: i32,
    pub start_x: SimpleVec,
    pub start_y: SimpleVec,
    pub start_city_x: SimpleVec,
    pub start_city_y: SimpleVec,
    pub oil_x: SimpleVec,
    pub oil_y: SimpleVec,
    pub direct: Vec<i32>, // 30
    pub wdata: Vec<u8>,
    pub tdata: Vec<u8>,
    pub seen: Vec<u8>,
    pub seen2: Vec<u8>,
    pub seen3: Vec<u8>,
    pub wcoord_seen: Vec<u8>,
    pub danger: Vec<u8>,
    pub blocks: Vec<CollBlock>,
    pub halfland_locs: Arr<Row<8>>,
    pub halfland_types: SimpleVec,
    pub halfland_subtypes: SimpleVec,
    pub nuke_hits: SimpleVec,
}

impl World {
    pub(crate) fn walk(&mut self, w: &mut dyn DataWalk) -> R {
        const VA: u32 = 0x006b5cf0;
        const C: &str = "World";
        tag(w, "World.tag", &mut self.tag)?;
        prim::w_i32(w, "World.xs", VA, &mut self.xs)?;
        prim::w_i32(w, "World.ys", VA, &mut self.ys)?;
        for (sv, name) in [
            (&mut self.start_x, "start_x"),
            (&mut self.start_y, "start_y"),
            (&mut self.start_city_x, "start_city_x"),
            (&mut self.start_city_y, "start_city_y"),
            (&mut self.oil_x, "oil_x"),
            (&mut self.oil_y, "oil_y"),
        ] {
            sv.walk(w, &format!("World.{name}"), C, VA, 4)?;
        }
        if w.is_loading() && self.direct.is_empty() {
            self.direct.resize(30, 0);
        }
        for i in 0..30 {
            prim::w_i32(w, &format!("World.direct[{i}]"), VA, &mut self.direct[i])?;
        }
        let (size, fog_size, tile_size, reg_size) =
            (self.direct[0], self.direct[3], self.direct[6], self.direct[9]);
        if w.is_loading() {
            for (n, v) in [
                ("size", size),
                ("fog_size", fog_size),
                ("tile_size", tile_size),
                ("reg_size", reg_size),
            ] {
                if !(0..=1 << 24).contains(&v) {
                    return Err(w.fail(C, VA, format!("World.{n} {v}")));
                }
            }
        }
        prim::take(w, "World.wdata", &mut self.wdata, size as usize * 21, C, VA)?;
        prim::take(w, "World.tdata", &mut self.tdata, tile_size as usize * 2, C, VA)?;
        prim::take(w, "World.seen", &mut self.seen, fog_size as usize, C, VA)?;
        prim::take(w, "World.seen2", &mut self.seen2, fog_size as usize, C, VA)?;
        prim::take(w, "World.seen3", &mut self.seen3, fog_size as usize, C, VA)?;
        prim::take(w, "World.wcoord_seen", &mut self.wcoord_seen, size as usize, C, VA)?;
        prim::take(w, "World.danger", &mut self.danger, 8 * 4 * reg_size as usize, C, VA)?;
        if w.is_loading() {
            self.blocks.clear();
            self.blocks.resize_with(size as usize, CollBlock::default);
        }
        for i in 0..self.blocks.len() {
            let b = &mut self.blocks[i];
            let p = format!("World.block[{i}]");
            prim::w_i32(w, &p, VA, &mut b.present)?;
            if !(0..=1).contains(&b.present) {
                return Err(w.fail(C, VA, format!("CollBlock[{i}] presence {}", b.present)));
            }
            if b.present == 1 {
                prim::w_i32(w, &p, VA, &mut b.bits)?;
                prim::w_i32(w, &p, VA, &mut b.size)?;
                if w.is_loading()
                    && (b.bits < 0 || b.size < 0 || b.size > 96 || b.bits > b.size * 8)
                {
                    return Err(w.fail(
                        C,
                        VA,
                        format!("CollBlock[{i}] bits={} size={}", b.bits, b.size),
                    ));
                }
                let n = b.size as usize;
                prim::take(w, &p, &mut b.data, n, C, VA)?;
            }
        }
        self.halfland_locs.walk(w, "Terrain.halfland_locs", C, VA)?;
        self.halfland_types.walk(w, "Terrain.halfland_types", C, VA, 4)?;
        self.halfland_subtypes.walk(w, "Terrain.halfland_subtypes", C, VA, 4)?;
        self.nuke_hits.walk(w, "Terrain.nuke_hits", C, VA, 4)
    }
}

// ---------------------------------------------------------------------------
// GraphicEvents::walk_data 0x008e4d70 — interleaved per-slot roots.
// ---------------------------------------------------------------------------

#[derive(Default, Clone)]
pub struct GraphicEvent {
    pub ty: i32,
    /// +8..+27 (19B) for type 8 else +8..+36 (28B).
    pub body: Vec<u8>,
}

impl Body for GraphicEvent {
    fn walk(&mut self, path: &str, w: &mut dyn DataWalk) -> R {
        const VA: u32 = 0x00919780;
        prim::w_i32(w, &format!("{path}.type"), VA, &mut self.ty)?;
        let n = if self.ty == 8 { 19 } else { 28 };
        prim::take(w, &format!("{path}.body"), &mut self.body, n, "GraphicEvent", VA)
    }
}

/// One linked EventGroup node: 38 PtrArray<GraphicEvent> + civ/age i8s +
/// a next-present chain marker.
#[derive(Default, Clone)]
pub struct EventGroup {
    pub events: Vec<PtrVec<GraphicEvent>>, // 38
    /// civ, age.
    pub civ_age: Vec<u8>,
    pub next: u8,
}

/// One root slot: a presence byte then zero or more EventGroup nodes.
#[derive(Default, Clone)]
pub struct EventRoot {
    pub present: u8,
    pub groups: Vec<EventGroup>,
}

#[derive(Default, Clone)]
pub struct GraphicEvents {
    pub tag: u8,
    /// GRAPHIC_EVENT_SLOTS roots — runtime-sized, not serialized.
    pub roots: Vec<EventRoot>,
    pub entrench_who: SimpleVec,
    pub entrench_o: SimpleVec,
    pub entrench_angle: SimpleVec,
    pub ambience: Arr<Row<24>>,
    /// missile_offset_{x,y,z}.
    pub missiles: Vec<u8>,
}

impl GraphicEvents {
    pub(crate) fn walk(&mut self, w: &mut dyn DataWalk) -> R {
        const VA: u32 = 0x008e4d70;
        const C: &str = "GraphicEvents";
        tag(w, "GraphicEvents.tag", &mut self.tag)?;
        let slots = if w.is_loading() {
            self.roots.clear();
            self.roots.resize_with(GRAPHIC_EVENT_SLOTS, EventRoot::default);
            GRAPHIC_EVENT_SLOTS
        } else {
            self.roots.len()
        };
        for slot in 0..slots {
            let r = &mut self.roots[slot];
            let p = format!("GraphicEvents.slot[{slot}]");
            if !w.is_loading() {
                r.present = (!r.groups.is_empty()) as u8;
            }
            prim::w_u8(w, &p, VA, &mut r.present)?;
            if r.present > 1 {
                return Err(w.fail(C, VA, format!("root[{slot}] marker {}", r.present)));
            }
            if w.is_loading() {
                r.groups.clear();
            }
            let mut node = 0usize;
            while r.present == 1 {
                if w.is_loading() {
                    r.groups.push(EventGroup::default());
                } else if node == r.groups.len() {
                    break;
                }
                let g = &mut r.groups[node];
                if g.events.is_empty() {
                    g.events.resize_with(38, PtrVec::default);
                }
                for kind in 0..38 {
                    g.events[kind].walk(
                        w,
                        &format!("{p}.group[{node}].events[{kind}]"),
                        C,
                        VA,
                    )?;
                }
                prim::take(w, &p, &mut g.civ_age, 2, C, VA)?;
                prim::w_u8(w, &p, VA, &mut g.next)?;
                if g.next > 1 {
                    return Err(w.fail(C, VA, format!("group[{node}] next marker {}", g.next)));
                }
                let next = g.next;
                node += 1;
                if w.is_loading() {
                    r.present = next;
                } else if node == r.groups.len() && next != 0 {
                    return Err(w.fail(C, VA, format!("group[{node}] dangling next marker")));
                }
            }
        }
        self.entrench_who.walk(w, "GraphicEvents.entrench_who", C, VA, 2)?;
        self.entrench_o.walk(w, "GraphicEvents.entrench_o", C, VA, 4)?;
        self.entrench_angle.walk(w, "GraphicEvents.entrench_angle", C, VA, 4)?;
        self.ambience.walk(w, "GraphicEvents.ambience", C, VA)?;
        prim::take(w, "GraphicEvents.missiles", &mut self.missiles, 12, C, VA)
    }
}

// ---------------------------------------------------------------------------
// Scene::walk_data 0x008c0f70
// ---------------------------------------------------------------------------

#[derive(Default, Clone)]
pub struct Scene {
    pub tag: u8,
    pub flags: BitMask,
    pub draw_flags: BitMask,
    pub ping_x: SimpleVec,
    pub ping_y: SimpleVec,
    pub ping_who: SimpleVec,
    pub ping_timer: SimpleVec,
    pub ping_finish_frame: SimpleVec,
    pub last_time: i32,
}

impl Scene {
    pub(crate) fn walk(&mut self, w: &mut dyn DataWalk) -> R {
        const VA: u32 = 0x008c0f70;
        const C: &str = "Scene";
        tag(w, "Scene.tag", &mut self.tag)?;
        self.flags.walk(w, "Scene.flags", C, VA)?;
        self.draw_flags.walk(w, "Scene.draw_flags", C, VA)?;
        for (sv, name, esz) in [
            (&mut self.ping_x, "ping_x", 4usize),
            (&mut self.ping_y, "ping_y", 4),
            (&mut self.ping_who, "ping_who", 1),
            (&mut self.ping_timer, "ping_timer", 1),
            (&mut self.ping_finish_frame, "ping_finish_frame", 4),
        ] {
            sv.walk(w, &format!("Scene.{name}"), C, VA, esz)?;
        }
        prim::w_i32(w, "Scene.last_time", VA, &mut self.last_time)
    }
}

// ---------------------------------------------------------------------------
// WalkDataGame::walk_data 0x005a2360 — the top-level state tree.
// ---------------------------------------------------------------------------

#[derive(Default, Clone)]
pub struct Save {
    /// Save magic string (String grammar).
    pub magic: Vec<u16>,
    pub version: u32,
    /// The top-level GameInfo (walked again inside `game`).
    pub info: GameInfo,
    /// game+0x298..+0x338 Console block, in three walked ranges.
    pub console1: Vec<u8>,
    pub console2: Vec<u8>,
    pub console3: Vec<u8>,
    /// Objects band scalars (24B).
    pub bands: Vec<u8>,
    pub game: Game,
    pub tribes: Tribes,
    pub leaders: Leaders,
    /// TechType::leader_off bytes for indices 0x880/4..0x9D4/4: 85 bytes.
    pub types: Vec<u8>,
    pub tileset: TileSet,
    pub mountains: Mountains,
    /// Constants +0..+0xd40 then the caller's direct scalar block.
    pub constants: Vec<u8>,
    pub direct_scalars: Vec<u8>,
    pub armies: OwnerLists<Army>,
    pub cities: OwnerLists<City>,
    pub forms: Forms,
    pub goods: PtrVec<GoodItem>,
    pub items: PtrVec<GoodItem>,
    pub heroes: OwnerLists<HeroLike>,
    pub herds: Herds,
    pub specials: OwnerLists<HeroLike>,
    pub wonders: OwnerLists<Row<14>>,
    pub forts: OwnerLists<Row<8>>,
    pub docks: OwnerLists<Row<10>>,
    pub oil_wells: OwnerLists<Row<8>>,
    pub supplies: OwnerLists<Row<6>>,
    pub caravans: OwnerLists<Caravan>,
    pub lands: Lands,
    pub leader_options: LeaderOptions,
    pub option_info: OptionInfo,
    /// Pathfinder direct block (27 i32).
    pub pathfinder: Vec<u8>,
    pub groups: Groups,
    pub objects: Objects,
    pub hotkey_groups: HotKeyGroups,
    pub world: World,
    /// 44-byte direct block between World and GraphicEvents.
    pub post_world: Vec<u8>,
    pub graphic_events: GraphicEvents,
    pub scene: Scene,
    // --- post-Scene tail (WalkDataGame order, re/decomp-all/005a2360.c) ---
    pub farms: Farms,
    pub unbuilt_wonders: Unbuilt<3, true>,
    pub unbuilt_cities: Unbuilt<4, false>,
    pub unbuilt_forts: Unbuilt<4, true>,
    pub conquest: ConquestGame,
    /// Direct globals block [0x00c06200]+0x28c..+0x370 (228B).
    pub post_conq_a: Vec<u8>,
    /// Tag walked between the two direct-globals blocks.
    pub post_conq_tag: u8,
    /// Direct globals block [0x00c06200]+0xb4..+0x288 (468B).
    pub post_conq_b: Vec<u8>,
    pub select_groups: SelectGroups,
    pub options: Options,
    pub command_manager: CommandManager,
    pub rivers: PtrVec<River>,
    /// Terrain::walk_coord_data emits 2B per runtime coord-list node; the
    /// node census is not serialized. Currently walks nothing — if the stream
    /// carries node bytes here, MessageWin will misparse and report the stop.
    pub message_win: MessageWin,
    pub terrain_roads: TerrainRoads,
    pub cliffs: Cliffs,
    pub doober: Doober,
    /// Direct globals [0x00c061b8]+0x18..+0x20 (8B).
    pub post_doober: Vec<u8>,
    pub regions: Arr<Region>,
    pub wcoords: Arr<Row<8>>,
    pub terrain: Terrain,
    /// Three direct i32 globals (0x00cab380, 0x00cb4bac, 0x00cab3a4).
    pub post_terrain: Vec<u8>,
    pub achieve: Achieve,
    pub scenario: ScenarioData,
    pub run_time_env: RunTimeEnv,
    pub final_globals: FinalGlobals,
    /// The tail after final_globals: RunTimeEnv record serialization, the
    /// conditional object, the Rules section (Game::walk_rules_data) and any
    /// trailing sections. Kept opaque — the boundary between the runtime
    /// serialization and the rules block is not yet resolved (see Rules below
    /// for the decoded rules grammar).
    pub rules_tail: RulesTail,
}

#[derive(Default, Clone)]
pub struct Herds {
    pub tag: u8,
    pub list: PtrVec<HerdLike>,
}

impl Herds {
    pub(crate) fn walk(&mut self, w: &mut dyn DataWalk) -> R {
        tag(w, "Herds.tag", &mut self.tag)?;
        self.list.walk(w, "Herds", "Herds", 0)
    }
}

impl Save {
    /// The literal top-level op sequence.
    pub fn walk_data(&mut self, w: &mut dyn DataWalk) -> R {
        const VA: u32 = 0x005a8220;
        prim::wstr(w, "magic", &mut self.magic, CLS, VA)?;
        prim::w_u32(w, "sGameSaveVersion", VA, &mut self.version)?;
        if w.is_loading() && !(8..=0x20).contains(&self.version) {
            return Err(w.fail(CLS, VA, format!("bad save version {}", self.version)));
        }
        let version = self.version;
        self.info.walk(w, version)?;
        prim::take(w, "Console", &mut self.console1, 0x2b8 - 0x298, CLS, 0x005a2360)?;
        prim::take(w, "Console", &mut self.console2, 0x2c2 - 0x2b8, CLS, 0x005a2360)?;
        prim::take(w, "Console", &mut self.console3, 0x338 - 0x2c4, CLS, 0x005a2360)?;
        prim::take(w, "Objects.bands", &mut self.bands, 24, CLS, 0x005a2926)?;
        self.game.walk(w, version)?;
        self.tribes.walk(w)?;
        self.leaders.walk(w)?;
        prim::take(w, "Types", &mut self.types, 85, "Types", 0)?;
        self.tileset.walk(w)?;
        self.mountains.walk(w)?;
        prim::take(w, "Constants", &mut self.constants, 0xd40, "Constants", 0x005a2a68)?;
        prim::take(w, "DirectScalars", &mut self.direct_scalars, 12, "Constants", 0x005a2a68)?;
        self.armies.walk(w, "Armies", 0x006f3700)?;
        self.cities.walk(w, "Cities", 0x00735410)?;
        self.forms.walk(w)?;
        self.goods.walk(w, "Goods", "Goods", 0)?;
        self.items.walk(w, "Items", "Items", 0)?;
        self.heroes.walk(w, "Heroes", 0)?;
        self.herds.walk(w)?;
        self.specials.walk(w, "Specials", 0)?;
        self.wonders.walk(w, "Wonders", 0)?;
        self.forts.walk(w, "Forts", 0)?;
        self.docks.walk(w, "Docks", 0)?;
        self.oil_wells.walk(w, "OilWells", 0)?;
        self.supplies.walk(w, "Supplies", 0)?;
        self.caravans.walk(w, "Caravans", 0)?;
        self.lands.walk(w)?;
        self.leader_options.walk(w)?;
        self.option_info.walk(w)?;
        prim::take(w, "Pathfinder.direct", &mut self.pathfinder, 27 * 4, "Pathfinder", 0)?;
        self.groups.walk(w)?;
        self.objects.walk(w)?;
        self.hotkey_groups.walk(w)?;
        self.world.walk(w)?;
        prim::take(w, "WalkDataGame.post_world", &mut self.post_world, 44, CLS, 0x005a2360)?;
        self.graphic_events.walk(w)?;
        self.scene.walk(w)?;
        self.farms.walk(w)?;
        self.unbuilt_wonders.walk(w, "UnbuiltWonders", 0x0073c290)?;
        self.unbuilt_cities.walk(w, "UnbuiltCities", 0x00460dc0)?;
        self.unbuilt_forts.walk(w, "UnbuiltForts", 0x0073bcc0)?;
        self.conquest.walk(w)?;
        prim::take(w, "WalkDataGame.post_conq_a", &mut self.post_conq_a, 228, CLS, VA)?;
        tag(w, "WalkDataGame.post_conq.tag", &mut self.post_conq_tag)?;
        prim::take(w, "WalkDataGame.post_conq_b", &mut self.post_conq_b, 468, CLS, VA)?;
        self.select_groups.walk(w)?;
        self.options.walk(w)?;
        self.command_manager.walk(w)?;
        self.rivers.walk(w, "Rivers", "PtrArray<River>", 0x004a2f80)?;
        // Terrain::walk_coord_data (0x00850ef0): runtime-list walk, emits
        // nothing for empty coord lists.
        self.message_win.walk(w)?;
        self.terrain_roads.walk(w)?;
        self.cliffs.walk(w)?;
        self.doober.walk(w)?;
        prim::take(w, "WalkDataGame.post_doober", &mut self.post_doober, 8, CLS, VA)?;
        self.regions.walk(w, "Regions", "ObjectArray<Region>", 0x00478ed0)?;
        self.wcoords.walk(w, "Regions.wcoords", "Array<WCoordData>", 0x00478990)?;
        let (xs, ys) = (self.world.xs.max(0) as usize, self.world.ys.max(0) as usize);
        self.terrain.walk(w, xs, ys)?;
        prim::take(w, "WalkDataGame.post_terrain", &mut self.post_terrain, 12, CLS, VA)?;
        self.achieve.walk(w)?;
        self.scenario.walk(w)?;
        self.run_time_env.walk(w)?;
        self.final_globals.walk(w)?;
        self.rules_tail.walk(w)
    }
}

/// Load the decompressed SaveGame stream into a fresh state tree. On success
/// returns `(state, loader)`; a `WalkError` reports the class/VA/offset where
/// an undecoded or malformed body stopped the walk.
pub fn load_save(buf: &[u8]) -> Result<(Save, Loader<'_>), WalkError> {
    let mut l = Loader::new(buf);
    let mut s = Save::default();
    s.walk_data(&mut l)?;
    Ok((s, l))
}

/// Debug variant: always returns the loader (with its span trace) plus any
/// error, so tests can attribute the stopping offset to a field path.
pub fn load_save_dbg(buf: &[u8]) -> (Loader<'_>, Option<WalkError>) {
    let mut l = Loader::new(buf);
    let mut s = Save::default();
    let r = s.walk_data(&mut l);
    (l, r.err())
}

// ---------------------------------------------------------------------------
// Post-Scene tail — WalkDataGame::walk_data order from re/decomp-all/005a2360.c
// (the doc TOC ordering for Terrain::walk_data/walk_roads was superseded by the
// disassembly: Regions -> WCoordData -> Terrain::walk_data, and the roads walk
// FUN_00852a60 sits between MessageWin and CliffsData).
// ---------------------------------------------------------------------------

/// A serialized String as a walked container element.
#[derive(Default, Clone)]
pub struct WStr {
    pub s: Vec<u16>,
}

impl Body for WStr {
    fn walk(&mut self, path: &str, w: &mut dyn DataWalk) -> R {
        prim::wstr(w, path, &mut self.s, "String", 0x00a1d2d0)
    }
}

/// `SimpleArray<int>` as a walked container element.
#[derive(Default, Clone)]
pub struct SArr {
    pub v: SimpleVec,
}

impl Body for SArr {
    fn walk(&mut self, path: &str, w: &mut dyn DataWalk) -> R {
        self.v.walk(w, path, "SimpleArray<int>", 0x00473120, 4)
    }
}

/// An array whose element grammar is not yet decoded: the header is walked
/// and a nonzero length fails closed at the element offset.
#[derive(Clone)]
pub struct UnknownArr {
    pub len: i32,
    pub cap: i32,
    pub inc: i16,
    pub flags: u8,
    pub class: &'static str,
}

impl Default for UnknownArr {
    fn default() -> Self {
        UnknownArr { len: 0, cap: 0, inc: 0, flags: 0, class: "UnknownArr" }
    }
}

impl UnknownArr {
    pub fn named(class: &'static str) -> Self {
        UnknownArr { class, ..Default::default() }
    }
}

impl Body for UnknownArr {
    fn walk(&mut self, path: &str, w: &mut dyn DataWalk) -> R {
        if !w.is_loading() {
            if self.len != 0 {
                return Err(w.fail(
                    self.class,
                    0,
                    format!("{path}: cannot emit {} undecoded elements", self.len),
                ));
            }
        }
        prim::w_i32(w, path, 0, &mut self.len)?;
        if self.len == 0 {
            return Ok(());
        }
        prim::w_i32(w, path, 0, &mut self.cap)?;
        prim::w_i16(w, path, 0, &mut self.inc)?;
        prim::w_u8(w, path, 0, &mut self.flags)?;
        Err(w.fail(
            self.class,
            0,
            format!("{path}: {} elements of undecoded row grammar", self.len),
        ))
    }
}

/// A counted link list: i32 count; when nonzero, `n` bodies.
#[derive(Default, Clone)]
pub struct Cnt<T> {
    pub len: i32,
    pub elems: Vec<T>,
}

impl<T: Default + Body> Cnt<T> {
    pub fn walk(&mut self, w: &mut dyn DataWalk, path: &str, class: &'static str, va: u32) -> R {
        if !w.is_loading() {
            self.len = self.elems.len() as i32;
        }
        prim::w_i32(w, path, va, &mut self.len)?;
        if w.is_loading() && !(0..=4_000_000).contains(&self.len) {
            return Err(w.fail(class, va, format!("{path} count {}", self.len)));
        }
        if w.is_loading() {
            self.elems.clear();
            self.elems.resize_with(self.len.max(0) as usize, T::default);
        }
        for (i, e) in self.elems.iter_mut().enumerate() {
            e.walk(&format!("{path}[{i}]"), w)?;
        }
        Ok(())
    }
}

/// Counted list whose element grammar is undecoded; fails closed on nonzero.
#[derive(Clone)]
pub struct UnknownCnt {
    pub len: i32,
    pub class: &'static str,
}

impl Default for UnknownCnt {
    fn default() -> Self {
        UnknownCnt { len: 0, class: "UnknownCnt" }
    }
}

impl UnknownCnt {
    pub fn named(class: &'static str) -> Self {
        UnknownCnt { class, ..Default::default() }
    }
    pub fn walk(&mut self, w: &mut dyn DataWalk, path: &str, va: u32) -> R {
        prim::w_i32(w, path, va, &mut self.len)?;
        if self.len != 0 {
            return Err(w.fail(
                self.class,
                va,
                format!("{path}: {} elements of undecoded node grammar", self.len),
            ));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Farms tranche: tag + 10B start_color + 8B heights + Array<FarmStruct>
// (190B rows). docs/derivation/savegame-farm-structs.md.
// ---------------------------------------------------------------------------

#[derive(Default, Clone)]
pub struct Farms {
    pub tag: u8,
    pub start_color: Vec<u8>,
    pub heights: Vec<u8>,
    pub farm_data: Arr<Row<190>>,
}

impl Farms {
    pub(crate) fn walk(&mut self, w: &mut dyn DataWalk) -> R {
        const VA: u32 = 0x005a2e20;
        tag(w, "Farms.tag", &mut self.tag)?;
        prim::take(w, "Farms.start_color", &mut self.start_color, 10, "Farms", VA)?;
        prim::take(w, "Farms.heights", &mut self.heights, 8, "Farms", VA)?;
        self.farm_data.walk(w, "Farms.data", "Array<FarmStruct>", 0x004a8db0)
    }
}

/// The three Unbuilt* owners: Wonders (tag + 8 lists of 3B rows),
/// Cities (no tag, 4B rows), Forts (tag + 4B rows).
#[derive(Clone)]
pub struct Unbuilt<const ROW: usize, const TAGGED: bool> {
    pub tag: u8,
    pub lists: Vec<Arr<Row<ROW>>>,
}

impl<const ROW: usize, const TAGGED: bool> Default for Unbuilt<ROW, TAGGED> {
    fn default() -> Self {
        Unbuilt { tag: 0, lists: (0..8).map(|_| Arr::default()).collect() }
    }
}

impl<const ROW: usize, const TAGGED: bool> Unbuilt<ROW, TAGGED> {
    pub(crate) fn walk(&mut self, w: &mut dyn DataWalk, name: &str, va: u32) -> R {
        if TAGGED {
            tag(w, &format!("{name}.tag"), &mut self.tag)?;
        }
        for i in 0..8 {
            self.lists[i].walk(w, &format!("{name}[{i}]"), "Unbuilt", va)?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// ConquestGame::walk_data 0x00798410 — docs/derivation/savegame-conquest-game.md
// ---------------------------------------------------------------------------

#[derive(Default, Clone)]
pub struct ConquestLeader {
    pub tag: u8,
    pub head: Vec<u8>,      // +0..+843
    pub bonus: PtrVec<UnknownRow>,
    pub a0: SArr,
    pub a1: SArr,
    pub a2: SArr,
    pub a3: SArr,
    pub a4: SArr,
    pub name: Vec<u16>,
    pub f0: Vec<u8>,        // +1008..1016
    pub v0: Vec<u8>,        // +1020, variable i32 count + data
    pub f1: Vec<u8>,        // +1024..1032
    pub v1: Vec<u8>,        // +1036 variable
    pub f2: Vec<u8>,        // +1060..1068
    pub v2: Vec<u8>,        // +1068 variable (thisload)
    pub a5: SArr,
    pub a6: SArr,
    pub a7: SArr,
}

/// Element of undecoded fixed grammar inside a presence-planed array.
#[derive(Default, Clone)]
pub struct UnknownRow;

impl Body for UnknownRow {
    fn walk(&mut self, path: &str, w: &mut dyn DataWalk) -> R {
        Err(w.fail(
            "UnknownRow",
            0,
            format!("{path}: undecoded element grammar"),
        ))
    }
}

impl Body for ConquestLeader {
    fn walk(&mut self, path: &str, w: &mut dyn DataWalk) -> R {
        const VA: u32 = 0x0079b6d0;
        const C: &str = "ConquestLeader";
        tag(w, path, &mut self.tag)?;
        prim::take(w, path, &mut self.head, 843, C, VA)?;
        self.bonus.walk(w, &format!("{path}.bonus"), "ObjectArray<ConquestBonusCard>", 0x00492d80)?;
        for (i, a) in [&mut self.a0, &mut self.a1, &mut self.a2, &mut self.a3, &mut self.a4]
            .iter_mut()
            .enumerate()
        {
            a.walk(&format!("{path}.a{i}"), w)?;
        }
        prim::wstr(w, path, &mut self.name, C, VA)?;
        prim::take(w, path, &mut self.f0, 8, C, VA)?;
        // Variable runs: i32 count + bytes (ranges reported variable by schema).
        var_run(w, &format!("{path}.v0"), &mut self.v0, C, VA)?;
        prim::take(w, path, &mut self.f1, 8, C, VA)?;
        var_run(w, &format!("{path}.v1"), &mut self.v1, C, VA)?;
        prim::take(w, path, &mut self.f2, 8, C, VA)?;
        var_run(w, &format!("{path}.v2"), &mut self.v2, C, VA)?;
        for (i, a) in [&mut self.a5, &mut self.a6, &mut self.a7].iter_mut().enumerate() {
            a.walk(&format!("{path}.b{i}"), w)?;
        }
        Ok(())
    }
}

/// `i32 n` then `n` bytes (the variable `[begin, begin + *(count)]` ranges).
pub fn var_run(
    w: &mut dyn DataWalk,
    path: &str,
    data: &mut Vec<u8>,
    class: &'static str,
    va: u32,
) -> R {
    let mut n = if w.is_loading() { 0i32 } else { data.len() as i32 };
    prim::w_i32(w, path, va, &mut n)?;
    if w.is_loading() && !(0..=1 << 24).contains(&n) {
        return Err(w.fail(class, va, format!("{path} count {n}")));
    }
    prim::take(w, path, data, n.max(0) as usize, class, va)
}

#[derive(Default, Clone)]
pub struct ConquestNode {
    pub tag: u8,
    pub a: Vec<u8>, // 68
    pub b: Vec<u8>, // 12
    pub s0: Vec<u16>,
    pub s1: Vec<u16>,
    pub s2: Vec<u16>,
    pub links: Arr<ConquestLink>,
}

impl Body for ConquestNode {
    fn walk(&mut self, path: &str, w: &mut dyn DataWalk) -> R {
        const VA: u32 = 0x007a5520;
        const C: &str = "ConquestNode";
        tag(w, path, &mut self.tag)?;
        prim::take(w, path, &mut self.a, 68, C, VA)?;
        prim::take(w, path, &mut self.b, 12, C, VA)?;
        prim::wstr(w, path, &mut self.s0, C, VA)?;
        prim::wstr(w, path, &mut self.s1, C, VA)?;
        prim::wstr(w, path, &mut self.s2, C, VA)?;
        self.links.walk(w, &format!("{path}.links"), "ObjectArray<ConquestLink>", 0x00493840)
    }
}

#[derive(Default, Clone)]
pub struct ConquestLink {
    pub head: Vec<u8>, // 24
    pub a0: SArr,
    pub a1: SArr,
    pub a2: SArr,
    pub a3: SArr,
    pub a4: SArr,
}

impl Body for ConquestLink {
    fn walk(&mut self, path: &str, w: &mut dyn DataWalk) -> R {
        const VA: u32 = 0x007a5860;
        const C: &str = "ConquestLink";
        prim::take(w, path, &mut self.head, 24, C, VA)?;
        for (i, a) in [&mut self.a0, &mut self.a1, &mut self.a2, &mut self.a3, &mut self.a4]
            .iter_mut()
            .enumerate()
        {
            a.walk(&format!("{path}.a{i}"), w)?;
        }
        Ok(())
    }
}

#[derive(Default, Clone)]
pub struct ConquestColony {
    pub head: Vec<u8>, // 16
    pub a: SArr,
    pub s0: Vec<u16>,
    pub s1: Vec<u16>,
}

impl Body for ConquestColony {
    fn walk(&mut self, path: &str, w: &mut dyn DataWalk) -> R {
        const VA: u32 = 0x00789400;
        const C: &str = "ConquestColony";
        prim::take(w, path, &mut self.head, 16, C, VA)?;
        self.a.walk(&format!("{path}.a"), w)?;
        prim::wstr(w, path, &mut self.s0, C, VA)?;
        prim::wstr(w, path, &mut self.s1, C, VA)
    }
}

#[derive(Default, Clone)]
pub struct ConquestStyle {
    pub tag: u8,
    pub head: Vec<u8>, // 140
    pub names: Vec<Vec<u16>>, // 10
    pub tail: Vec<u8>,       // 4
    pub entries: PtrVec<UnknownRow>,
}

impl Body for ConquestStyle {
    fn walk(&mut self, path: &str, w: &mut dyn DataWalk) -> R {
        const VA: u32 = 0x007a9170;
        const C: &str = "ConquestStyle";
        tag(w, path, &mut self.tag)?;
        prim::take(w, path, &mut self.head, 140, C, VA)?;
        if w.is_loading() && self.names.is_empty() {
            self.names.resize_with(10, Vec::new);
        }
        for i in 0..10 {
            prim::wstr(w, path, &mut self.names[i], C, VA)?;
        }
        prim::take(w, path, &mut self.tail, 4, C, VA)?;
        self.entries.walk(w, &format!("{path}.entries"), "PtrArray<StringListEntry>", 0x004940d0)
    }
}

/// ObjectArray<ConquestStyle> as a value element (game_styles rows).
#[derive(Default, Clone)]
pub struct StyleArr {
    pub v: Arr<ConquestStyle>,
}

impl Body for StyleArr {
    fn walk(&mut self, path: &str, w: &mut dyn DataWalk) -> R {
        self.v.walk(w, path, "ObjectArray<ConquestStyle>", 0x004923b0)
    }
}

/// NamedSimpleArray<int>: array header, n i32 values, then n String names.
#[derive(Default, Clone)]
pub struct NamedInts {
    pub len: i32,
    pub cap: i32,
    pub inc: i16,
    pub flags: u8,
    pub vals: Vec<u8>,
    pub names: Vec<Vec<u16>>,
}

impl NamedInts {
    pub(crate) fn walk(&mut self, w: &mut dyn DataWalk, path: &str, class: &'static str, va: u32) -> R {
        if !w.is_loading() {
            self.len = self.names.len() as i32;
        }
        prim::w_i32(w, path, va, &mut self.len)?;
        if w.is_loading() && !(0..=4_000_000).contains(&self.len) {
            return Err(w.fail(class, va, format!("{path} length {}", self.len)));
        }
        if self.len == 0 {
            return Ok(());
        }
        prim::w_i32(w, path, va, &mut self.cap)?;
        prim::w_i16(w, path, va, &mut self.inc)?;
        prim::w_u8(w, path, va, &mut self.flags)?;
        let n = self.len as usize;
        prim::take(w, path, &mut self.vals, n * 4, class, va)?;
        if w.is_loading() {
            self.names.clear();
            self.names.resize_with(n, Vec::new);
        }
        for i in 0..n.min(self.names.len()) {
            prim::wstr(w, &format!("{path}[{i}].name"), &mut self.names[i], class, va)?;
        }
        Ok(())
    }
}

/// NamedObjectArray<String>: array header, n value Strings, then n name
/// Strings (docs/derivation/savegame-conquest-game.md "named String arrays
/// write the value Strings followed by the name Strings").
#[derive(Default, Clone)]
pub struct NamedStrs {
    pub len: i32,
    pub cap: i32,
    pub inc: i16,
    pub flags: u8,
    pub vals: Vec<Vec<u16>>,
    pub names: Vec<Vec<u16>>,
}

impl NamedStrs {
    pub(crate) fn walk(&mut self, w: &mut dyn DataWalk, path: &str, class: &'static str, va: u32) -> R {
        if !w.is_loading() {
            self.len = self.vals.len() as i32;
        }
        prim::w_i32(w, path, va, &mut self.len)?;
        if w.is_loading() && !(0..=4_000_000).contains(&self.len) {
            return Err(w.fail(class, va, format!("{path} length {}", self.len)));
        }
        if self.len == 0 {
            return Ok(());
        }
        prim::w_i32(w, path, va, &mut self.cap)?;
        prim::w_i16(w, path, va, &mut self.inc)?;
        prim::w_u8(w, path, va, &mut self.flags)?;
        let n = self.len as usize;
        if w.is_loading() {
            self.vals.clear();
            self.vals.resize_with(n, Vec::new);
            self.names.clear();
            self.names.resize_with(n, Vec::new);
        }
        for i in 0..n {
            prim::wstr(w, &format!("{path}[{i}]"), &mut self.vals[i], class, va)?;
        }
        for i in 0..n {
            prim::wstr(w, &format!("{path}[{i}].name"), &mut self.names[i], class, va)?;
        }
        Ok(())
    }
}

/// LinkList<int,short> 0x00491a90: i32 count, per node i16 then i32.
#[derive(Default, Clone)]
pub struct LinkList {
    pub len: i32,
    pub nodes: Vec<u8>, // 6 bytes per node (i16 metric + i32 value)
}

impl LinkList {
    pub(crate) fn walk(&mut self, w: &mut dyn DataWalk, path: &str, class: &'static str, va: u32) -> R {
        if !w.is_loading() {
            self.len = (self.nodes.len() / 6) as i32;
        }
        prim::w_i32(w, path, va, &mut self.len)?;
        if w.is_loading() && !(0..=4_000_000).contains(&self.len) {
            return Err(w.fail(class, va, format!("{path} count {}", self.len)));
        }
        prim::take(w, path, &mut self.nodes, self.len.max(0) as usize * 6, class, va)
    }
}

/// ConquestPieces 0x007accb0: twenty-four PtrArray<ConquestPiece> slots,
/// each a full presence-planed pointer array of 44-byte bodies.
#[derive(Clone)]
pub struct ConquestPieces {
    pub slots: Vec<PtrVec<Row<44>>>,
}

impl Default for ConquestPieces {
    fn default() -> Self {
        ConquestPieces { slots: (0..24).map(|_| PtrVec::default()).collect() }
    }
}

impl ConquestPieces {
    pub(crate) fn walk(&mut self, w: &mut dyn DataWalk) -> R {
        for i in 0..24 {
            self.slots[i].walk(w, &format!("ConquestPieces[{i}]"), "PtrArray<ConquestPiece>", 0x007accb0)?;
        }
        Ok(())
    }
}

#[derive(Default, Clone)]
pub struct ConquestGame {
    pub tag: u8,
    pub head: Vec<u8>, // +4..+392
    pub colors: Arr<Row<10>>,
    pub leaders_tag: u8,
    pub leaders: Arr<ConquestLeader>,
    pub nodes_tag: u8,
    pub nodes: Arr<ConquestNode>,
    pub colonies: Arr<ConquestColony>,
    pub continents: Arr<WStr>,
    pub barbarian_files: Arr<WStr>,
    pub names: Vec<Vec<u16>>, // 10
    pub map_size_scale: SimpleVec,
    pub pieces: ConquestPieces,
    pub reinforcement_armies: UnknownArr,
    pub f1500: Vec<u8>, // +1500..1508
    pub news_strings: Arr<WStr>,
    pub news_items: Arr<Row<24>>,
    pub news_link: LinkList,
    pub overrun_armies: SimpleVec,
    pub continents_captured: SimpleVec,
    pub bonus_card_deck: Arr<WStr>,
    pub game_styles: Arr<StyleArr>,
    pub stored_ints: NamedInts,
    pub stored_strs: NamedStrs,
    pub diplo_deals: NamedInts,
    pub tribes_tag: u8,
    pub tribes: Arr<Tribe>,
    pub tribe_names: Vec<Vec<u16>>, // 8
    pub allied_punks: SimpleVec,
}

impl ConquestGame {
    pub(crate) fn walk(&mut self, w: &mut dyn DataWalk) -> R {
        const VA: u32 = 0x00798410;
        const C: &str = "ConquestGame";
        tag(w, "ConquestGame.tag", &mut self.tag)?;
        prim::take(w, "ConquestGame.head", &mut self.head, 388, C, VA)?;
        self.colors.walk(w, "ConquestGame.colors", "Array<Color>", 0x004912e0)?;
        tag(w, "ConquestGame.leaders.tag", &mut self.leaders_tag)?;
        self.leaders.walk(w, "ConquestGame.leaders", "ObjectArray<ConquestLeader>", 0x0079b6d0)?;
        tag(w, "ConquestGame.nodes.tag", &mut self.nodes_tag)?;
        self.nodes.walk(w, "ConquestGame.nodes", "ObjectArray<ConquestNode>", 0x007a5520)?;
        self.colonies.walk(w, "ConquestGame.colonies", "ObjectArray<ConquestColony>", 0x00789400)?;
        self.continents.walk(w, "ConquestGame.continents", "ObjectArray<String>", 0x00490fb0)?;
        self.barbarian_files.walk(w, "ConquestGame.barbarian_files", "ObjectArray<String>", 0x00490fb0)?;
        if w.is_loading() && self.names.is_empty() {
            self.names.resize_with(10, Vec::new);
        }
        for i in 0..10 {
            prim::wstr(w, &format!("ConquestGame.names[{i}]"), &mut self.names[i], C, VA)?;
        }
        self.map_size_scale.walk(w, "ConquestGame.map_size_scale", C, VA, 4)?;
        self.pieces.walk(w)?;
        self.reinforcement_armies.walk("ConquestGame.reinforcement_armies", w)?;
        prim::take(w, "ConquestGame.f1500", &mut self.f1500, 8, C, VA)?;
        self.news_strings.walk(w, "ConquestGame.news_strings", "ObjectArray<String>", 0x00490fb0)?;
        self.news_items.walk(w, "ConquestGame.news_items", "Array<ConquestNewsItem>", 0x004934c0)?;
        self.news_link.walk(w, "ConquestGame.news_link", C, 0x00491a90)?;
        self.overrun_armies.walk(w, "ConquestGame.overrun_armies", C, VA, 4)?;
        self.continents_captured.walk(w, "ConquestGame.continents_captured", C, VA, 4)?;
        self.bonus_card_deck.walk(w, "ConquestGame.bonus_card_deck", "ObjectArray<String>", 0x00490fb0)?;
        self.game_styles.walk(w, "ConquestGame.game_styles", "ObjectArray<ObjectArray<ConquestStyle>>", 0x004916f0)?;
        self.stored_ints.walk(w, "ConquestGame.stored_ints", C, 0x00490960)?;
        self.stored_strs.walk(w, "ConquestGame.stored_strs", C, 0x00491ca0)?;
        self.diplo_deals.walk(w, "ConquestGame.diplo_deals", C, 0x00490960)?;
        tag(w, "ConquestGame.tribes.tag", &mut self.tribes_tag)?;
        self.tribes.walk(w, "ConquestGame.tribes", "ObjectArray<Tribe>", 0x0047e230)?;
        if w.is_loading() && self.tribe_names.is_empty() {
            self.tribe_names.resize_with(8, Vec::new);
        }
        for i in 0..8 {
            prim::wstr(w, &format!("ConquestGame.tribe_names[{i}]"), &mut self.tribe_names[i], C, VA)?;
        }
        self.allied_punks.walk(w, "ConquestGame.allied_punks", C, VA, 4)
    }
}

// ---------------------------------------------------------------------------
// SelectGroups: tag + two Array<SelectGroup>; SelectGroup = Group + tag + 24B.
// docs/derivation/savegame-select-groups.md
// ---------------------------------------------------------------------------

#[derive(Default, Clone)]
pub struct SelectGroup {
    pub group: Group,
    pub tag: u8,
    pub tail: Vec<u8>, // 6 i32: whose,flashing,named,item_ox,good_ox,flash_frame
}

impl Body for SelectGroup {
    fn walk(&mut self, path: &str, w: &mut dyn DataWalk) -> R {
        self.group.walk(path, w, "SelectGroup")?;
        tag(w, path, &mut self.tag)?;
        prim::take(w, path, &mut self.tail, 24, "SelectGroup", 0x00480900)
    }
}

#[derive(Default, Clone)]
pub struct SelectGroups {
    pub tag: u8,
    pub a: Arr<SelectGroup>,
    pub b: Arr<SelectGroup>,
}

impl SelectGroups {
    pub(crate) fn walk(&mut self, w: &mut dyn DataWalk) -> R {
        tag(w, "SelectGroups.tag", &mut self.tag)?;
        self.a.walk(w, "SelectGroups.a", "Array<SelectGroup>", 0x00480900)?;
        self.b.walk(w, "SelectGroups.b", "Array<SelectGroup>", 0x00480900)
    }
}

/// Options: inherited untagged Array<Option> (18B rows), then Options tag +
/// +0x20..+0x7c (92B) + +0x7c..+0x8e (18B).
/// docs/derivation/savegame-options.md
#[derive(Default, Clone)]
pub struct Options {
    pub list: Arr<Row<18>>,
    pub tag: u8,
    pub a: Vec<u8>, // 92
    pub b: Vec<u8>, // 18
}

impl Options {
    pub(crate) fn walk(&mut self, w: &mut dyn DataWalk) -> R {
        const C: &str = "Options";
        self.list.walk(w, "Options.list", "Array<Option>", 0x00480cc0)?;
        tag(w, "Options.tag", &mut self.tag)?;
        prim::take(w, "Options.a", &mut self.a, 92, C, 0x00480cc0)?;
        prim::take(w, "Options.b", &mut self.b, 18, C, 0x00480cc0)
    }
}

// ---------------------------------------------------------------------------
// CommandManager::walk_data 0x00942d30 —
// docs/derivation/savegame-command-manager.md
// ---------------------------------------------------------------------------

#[derive(Default, Clone)]
pub struct CommandPackage {
    pub tag: u8,
    pub stamp: u32,
    pub play: i32,
    pub valid: i32,
    pub group: i32,
    pub size: i16,
    pub data: Vec<u8>,
}

impl CommandPackage {
    pub(crate) fn walk(&mut self, path: &str, w: &mut dyn DataWalk) -> R {
        const VA: u32 = 0x00952500;
        const C: &str = "CommandPackage";
        prim::w_u32(w, path, VA, &mut self.stamp)?;
        prim::w_i32(w, path, VA, &mut self.play)?;
        prim::w_i32(w, path, VA, &mut self.valid)?;
        prim::w_i32(w, path, VA, &mut self.group)?;
        if !w.is_loading() {
            self.size = self.data.len() as i16;
        }
        prim::w_i16(w, path, VA, &mut self.size)?;
        if w.is_loading() && !(0..=512).contains(&self.size) {
            return Err(w.fail(C, VA, format!("{path} size {}", self.size)));
        }
        prim::take(w, path, &mut self.data, self.size.max(0) as usize, C, VA)
    }
}

#[derive(Clone)]
pub struct PackageFifo {
    pub tag: u8,
    pub head: Vec<u8>, // front,front_local,length,length_local (16B)
    pub packages: Vec<CommandPackage>, // 20 with per-package tag
}

impl Default for PackageFifo {
    fn default() -> Self {
        PackageFifo {
            tag: 0,
            head: Vec::new(),
            packages: (0..20).map(|_| CommandPackage::default()).collect(),
        }
    }
}

impl PackageFifo {
    pub(crate) fn walk(&mut self, path: &str, w: &mut dyn DataWalk) -> R {
        const C: &str = "PackageFifo";
        tag(w, &format!("{path}.tag"), &mut self.tag)?;
        prim::take(w, path, &mut self.head, 16, C, 0x00952500)?;
        for i in 0..20 {
            tag(w, &format!("{path}.packages[{i}].tag"), &mut self.packages[i].tag)?;
            self.packages[i].walk(&format!("{path}.packages[{i}]"), w)?;
        }
        Ok(())
    }
}

/// CommandManager: tag, tag, local CommandPackage, then eight FIFOs.
#[derive(Clone)]
pub struct CommandManager {
    pub tag: u8,
    pub local_tag: u8,
    pub local_package: CommandPackage,
    pub fifos: Vec<PackageFifo>,
}

impl Default for CommandManager {
    fn default() -> Self {
        CommandManager {
            tag: 0,
            local_tag: 0,
            local_package: CommandPackage::default(),
            fifos: (0..8).map(|_| PackageFifo::default()).collect(),
        }
    }
}

impl CommandManager {
    pub(crate) fn walk(&mut self, w: &mut dyn DataWalk) -> R {
        const VA: u32 = 0x00942d30;
        tag(w, "CommandManager.tag", &mut self.tag)?;
        tag(w, "CommandManager.local.tag", &mut self.local_tag)?;
        self.local_package.walk("CommandManager.local_package", w)?;
        for i in 0..8 {
            self.fifos[i].walk(&format!("CommandManager.fifos[{i}]"), w)?;
        }
        let _ = VA;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// PtrArray<River> 0x004a2f80 — River = i32 creation_spline_present + Spline.
// docs/derivation/savegame-rivers.md
// ---------------------------------------------------------------------------

#[derive(Default, Clone)]
pub struct River {
    pub present: i32,
    pub spline: Option<Box<Spline>>,
}

impl Body for River {
    fn walk(&mut self, path: &str, w: &mut dyn DataWalk) -> R {
        const C: &str = "River";
        if !w.is_loading() {
            self.present = i32::from(self.spline.is_some());
        }
        prim::w_i32(w, path, 0x009132b0, &mut self.present)?;
        if !(0..=1).contains(&self.present) {
            return Err(w.fail(C, 0, format!("{path} spline present {}", self.present)));
        }
        if self.present != 0 {
            self.spline
                .get_or_insert_with(|| Box::new(Spline::default()))
                .walk(path, w, C)?;
        }
        Ok(())
    }
}

/// Terrain::walk_coord_data 0x00850ef0: per coordinate slot, two bytes for
/// each node of the runtime coord list. Node counts are rebuilt at load time
/// from already-loaded state and are not serialized; an empty list emits
/// nothing. Preserved as a flat run of 2B node records whose length is
/// determined on load by the runtime list census — unknown here, so a
/// nonzero stream length surfaces as a stop via the next section.
/// MessageWin::walk_data 0x007e9d40.
#[derive(Clone)]
pub struct MessageWin {
    pub lists: Vec<Cnt<MsgNode>>, // 4
    pub a: Vec<u8>,              // 10
    pub tag: u8,
    pub b: Vec<u8>, // 8
    pub s0: Vec<u16>,
    pub s1: Vec<u16>,
    pub c: Vec<u8>, // 8
    pub d: Vec<u8>, // 10
    pub s2: Vec<u16>,
    pub e: Vec<u8>, // 8
    pub f: Vec<u8>, // 10
}

impl Default for MessageWin {
    fn default() -> Self {
        MessageWin {
            lists: (0..4).map(|_| Cnt::default()).collect(),
            a: Vec::new(),
            tag: 0,
            b: Vec::new(),
            s0: Vec::new(),
            s1: Vec::new(),
            c: Vec::new(),
            d: Vec::new(),
            s2: Vec::new(),
            e: Vec::new(),
            f: Vec::new(),
        }
    }
}

/// LLNode<MessageWinEntry*,int> 0x00498210: i32 metric, tag, entry body
/// (36B + 1B + 10B + 2 Strings).
#[derive(Default, Clone)]
pub struct MsgNode {
    pub key: i32,
    pub metric: i32,
    pub tag: u8,
    pub a: Vec<u8>, // 36
    pub b: Vec<u8>, // 1
    pub c: Vec<u8>, // 10
    pub s0: Vec<u16>,
    pub s1: Vec<u16>,
}

impl Body for MsgNode {
    fn walk(&mut self, path: &str, w: &mut dyn DataWalk) -> R {
        const C: &str = "MessageWinEntry";
        const VA: u32 = 0x00498210;
        prim::w_i32(w, path, VA, &mut self.key)?;
        prim::w_i32(w, path, VA, &mut self.metric)?;
        tag(w, path, &mut self.tag)?;
        prim::take(w, path, &mut self.a, 36, C, VA)?;
        prim::take(w, path, &mut self.b, 1, C, VA)?;
        prim::take(w, path, &mut self.c, 10, C, VA)?;
        prim::wstr(w, path, &mut self.s0, C, VA)?;
        prim::wstr(w, path, &mut self.s1, C, VA)
    }
}

impl MessageWin {
    pub(crate) fn walk(&mut self, w: &mut dyn DataWalk) -> R {
        const VA: u32 = 0x007e9d40;
        const C: &str = "MessageWin";
        for i in 0..4 {
            self.lists[i].walk(w, &format!("MessageWin.lists[{i}]"), C, 0x00497cf0)?;
        }
        prim::take(w, "MessageWin.a", &mut self.a, 10, C, VA)?;
        tag(w, "MessageWin.tag", &mut self.tag)?;
        prim::take(w, "MessageWin.b", &mut self.b, 8, C, VA)?;
        prim::wstr(w, "MessageWin.s0", &mut self.s0, C, VA)?;
        prim::wstr(w, "MessageWin.s1", &mut self.s1, C, VA)?;
        prim::take(w, "MessageWin.c", &mut self.c, 8, C, VA)?;
        prim::take(w, "MessageWin.d", &mut self.d, 10, C, VA)?;
        prim::wstr(w, "MessageWin.s2", &mut self.s2, C, VA)?;
        prim::take(w, "MessageWin.e", &mut self.e, 8, C, VA)?;
        prim::take(w, "MessageWin.f", &mut self.f, 10, C, VA)
    }
}

/// Terrain::walk_roads 0x00852a60: SimpleArray<int> (FUN_00490b10) then an
/// ObjectArray<SimpleArray<int>> (FUN_0049b770).
#[derive(Default, Clone)]
pub struct TerrainRoads {
    pub a: SimpleVec,
    pub lists: Arr<SArr>,
}

impl TerrainRoads {
    pub(crate) fn walk(&mut self, w: &mut dyn DataWalk) -> R {
        self.a.walk(w, "Terrain.roads.a", "SimpleArray<int>", 0x00490b10, 4)?;
        self.lists.walk(w, "Terrain.roads.lists", "ObjectArray<SimpleArray<int>>", 0x0049b770)
    }
}

/// CliffsData 0x008a9480: PtrArray<Cliff>(4B rows), 8B, 8B, SimpleArray<int>,
/// PtrArray<CliffMiningData>.
#[derive(Default, Clone)]
pub struct Cliffs {
    pub cliffs: PtrVec<Row<4>>,
    pub a: Vec<u8>, // 8
    pub b: Vec<u8>, // 8
    pub list: SimpleVec,
    pub mining: PtrVec<CliffMining>,
}

#[derive(Default, Clone)]
pub struct CliffMining {
    pub w: Arr<Row<8>>, // Array<WCoordData>
    pub t: Arr<Row<8>>, // Array<TCoordData>
    pub head: Vec<u8>,  // 16
}

impl Body for CliffMining {
    fn walk(&mut self, path: &str, w: &mut dyn DataWalk) -> R {
        const VA: u32 = 0x008a6d70;
        self.w.walk(w, &format!("{path}.w"), "Array<WCoordData>", 0x00478990)?;
        self.t.walk(w, &format!("{path}.t"), "Array<TCoordData>", 0x00471c30)?;
        prim::take(w, path, &mut self.head, 16, "CliffMiningData", VA)
    }
}

impl Cliffs {
    pub(crate) fn walk(&mut self, w: &mut dyn DataWalk) -> R {
        const VA: u32 = 0x008a9480;
        const C: &str = "CliffsData";
        self.cliffs.walk(w, "CliffsData.cliffs", "PtrArray<Cliff>", 0x004a7100)?;
        prim::take(w, "CliffsData.a", &mut self.a, 8, C, VA)?;
        prim::take(w, "CliffsData.b", &mut self.b, 8, C, VA)?;
        self.list.walk(w, "CliffsData.list", C, VA, 4)?;
        self.mining.walk(w, "CliffsData.mining", "PtrArray<CliffMiningData>", 0x004a7440)
    }
}

/// Doober 0x00846ff0.
#[derive(Default, Clone)]
pub struct Doober {
    pub tag: u8,
    pub a: SimpleVec,
    pub uv0: Arr<Row<8>>,
    pub b: SimpleVec,
    pub c: SimpleVec,
    pub uv1: Arr<Row<8>>,
    pub d: SimpleVec, // SimpleArray<u8> element — 1B rows
    pub e: SimpleVec,
    pub rects: SimpleVec, // RectTemplate<short> — 8B rows
    pub f: SimpleVec,
    pub g: SimpleVec,
    pub h: SimpleVec,
}

impl Doober {
    pub(crate) fn walk(&mut self, w: &mut dyn DataWalk) -> R {
        const VA: u32 = 0x00846ff0;
        const C: &str = "Doober";
        tag(w, "Doober.tag", &mut self.tag)?;
        self.a.walk(w, "Doober.a", C, VA, 4)?;
        self.uv0.walk(w, "Doober.uv0", "Array<UVPair>", 0x00499e90)?;
        self.b.walk(w, "Doober.b", C, VA, 4)?;
        self.c.walk(w, "Doober.c", C, VA, 4)?;
        self.uv1.walk(w, "Doober.uv1", "Array<UVPair>", 0x00499e90)?;
        self.d.walk(w, "Doober.d", C, VA, 1)?;
        self.e.walk(w, "Doober.e", C, VA, 4)?;
        self.rects.walk(w, "Doober.rects", C, VA, 8)?;
        self.f.walk(w, "Doober.f", C, VA, 4)?;
        self.g.walk(w, "Doober.g", C, VA, 4)?;
        self.h.walk(w, "Doober.h", C, VA, 4)
    }
}

/// Region::walk_data 0x00680e20: tag + 44B + three (i32,i32 n, u8 data[n])
/// count-runs + Array<WCoordData> + i32 flag (skipped by CheckSum).
#[derive(Default, Clone)]
pub struct Region {
    pub tag: u8,
    pub head: Vec<u8>, // 44
    pub r0: Vec<u8>,   // 8 (i32 + count)
    pub d0: Vec<u8>,
    pub r1: Vec<u8>,
    pub d1: Vec<u8>,
    pub r2: Vec<u8>,
    pub d2: Vec<u8>,
    pub coords: Arr<Row<8>>,
    pub flag: i32,
}

fn count_run(
    w: &mut dyn DataWalk,
    path: &str,
    head: &mut Vec<u8>,
    data: &mut Vec<u8>,
    class: &'static str,
    va: u32,
) -> R {
    if !w.is_loading() {
        let n = data.len() as i32;
        head[4..8].copy_from_slice(&n.to_le_bytes());
    }
    prim::take(w, path, head, 8, class, va)?;
    let n = i32::from_le_bytes(head[4..8].try_into().unwrap());
    if !(0..=1 << 24).contains(&n) {
        return Err(w.fail(class, va, format!("{path} count {n}")));
    }
    prim::take(w, path, data, n as usize, class, va)
}

impl Body for Region {
    fn walk(&mut self, path: &str, w: &mut dyn DataWalk) -> R {
        const VA: u32 = 0x00680e20;
        const C: &str = "Region";
        tag(w, path, &mut self.tag)?;
        prim::take(w, path, &mut self.head, 44, C, VA)?;
        count_run(w, &format!("{path}.d0"), &mut self.r0, &mut self.d0, C, VA)?;
        count_run(w, &format!("{path}.d1"), &mut self.r1, &mut self.d1, C, VA)?;
        count_run(w, &format!("{path}.d2"), &mut self.r2, &mut self.d2, C, VA)?;
        self.coords.walk(w, &format!("{path}.coords"), "Array<WCoordData>", 0x00478990)?;
        if !w.is_checksum() {
            prim::w_i32(w, path, VA, &mut self.flag)?;
        }
        Ok(())
    }
}

/// Terrain::walk_data 0x00852b00: per map tile, a flag byte; a nonzero flag
/// is followed by 16 rows of 16 bytes (PDB tile scratch block).
#[derive(Default, Clone)]
pub struct Terrain {
    /// flags[xs*ys] then for each set flag a 256B block, concatenated.
    pub flags: Vec<u8>,
    pub blocks: Vec<Vec<u8>>,
}

impl Terrain {
    pub(crate) fn walk(&mut self, w: &mut dyn DataWalk, xs: usize, ys: usize) -> R {
        const VA: u32 = 0x00852b00;
        const C: &str = "Terrain";
        let total = xs.checked_mul(ys).unwrap_or(0);
        if total > 1 << 24 {
            return Err(w.fail(C, VA, format!("Terrain dims {xs}x{ys}")));
        }
        if w.is_loading() {
            self.flags.clear();
            self.flags.resize(total, 0);
        }
        let mut bi = 0usize;
        for i in 0..total {
            let mut f = [self.flags.get(i).copied().unwrap_or(0)];
            w.walk_bytes(&format!("Terrain.tile[{i}].flag"), &mut f)?;
            if w.is_loading() {
                self.flags[i] = f[0];
            }
            if f[0] != 0 {
                if w.is_loading() {
                    self.blocks.push(vec![0; 256]);
                }
                if bi >= self.blocks.len() {
                    return Err(w.fail(C, VA, format!("Terrain.tile[{i}] missing block")));
                }
                w.walk_bytes(&format!("Terrain.tile[{i}].block"), &mut self.blocks[bi])?;
                bi += 1;
            }
        }
        if !w.is_loading() && bi != self.blocks.len() {
            return Err(w.fail(C, VA, format!("Terrain blocks {} != flags", self.blocks.len())));
        }
        Ok(())
    }
}

/// Achieve 0x007af790: tag + 4B + AchieveData + SimpleArray<int> +
/// ObjectArray<AchieveEvent>.
#[derive(Default, Clone)]
pub struct Achieve {
    pub tag: u8,
    pub head: i32,
    /// 6 fixed records at DAT_00e87d60 stride 0x140 (FUN_007af790).
    pub data: Vec<AchieveData>,
    pub list: SimpleVec,
    /// 8 fixed ObjectArrays at DAT_00e884e0 stride 0x18 (FUN_00495130 each).
    pub events: Vec<Arr<AchieveEvent>>,
}

/// FUN_007af0b0 — one fixed player-history record (object stride 0x140):
/// tag + image[0x138..0x140) (8B) + 8 x SimpleArray<int> +
/// image[0xf8..0x118) (32B) + image[0x118..0x138) (32B) + String.
#[derive(Default, Clone)]
pub struct AchieveData {
    pub tag: u8,
    pub a: Vec<u8>, // 8
    pub l: Vec<SimpleVec>,
    pub b: Vec<u8>, // 32
    pub c: Vec<u8>, // 32
    pub s: Vec<u16>,
}

impl AchieveData {
    pub(crate) fn walk(&mut self, w: &mut dyn DataWalk) -> R {
        const VA: u32 = 0x007af0b0;
        const C: &str = "AchieveData";
        tag(w, "AchieveData.tag", &mut self.tag)?;
        prim::take(w, "AchieveData.a", &mut self.a, 8, C, VA)?;
        if w.is_loading() && self.l.is_empty() {
            self.l.resize_with(8, SimpleVec::default);
        }
        for i in 0..self.l.len() {
            self.l[i].walk(w, "AchieveData.l", C, VA, 4)?;
        }
        prim::take(w, "AchieveData.b", &mut self.b, 32, C, VA)?;
        prim::take(w, "AchieveData.c", &mut self.c, 32, C, VA)?;
        prim::wstr(w, "AchieveData.s", &mut self.s, C, VA)
    }
}

/// Element of the 8 nested ObjectArrays (FUN_00495130): 8B head + a
/// checksum-gated String (`param_1[2]==0` → serialized, skipped by CheckSum).
#[derive(Default, Clone)]
pub struct AchieveEvent {
    pub head: Vec<u8>, // 8
    pub s: Vec<u16>,
}

impl Body for AchieveEvent {
    fn walk(&mut self, path: &str, w: &mut dyn DataWalk) -> R {
        const C: &str = "AchieveEvent";
        prim::take(w, path, &mut self.head, 8, C, 0x00495130)?;
        if !w.is_checksum() {
            prim::wstr(w, path, &mut self.s, C, 0x00495130)?;
        }
        Ok(())
    }
}

impl Achieve {
    pub(crate) fn walk(&mut self, w: &mut dyn DataWalk) -> R {
        const VA: u32 = 0x007af790;
        const C: &str = "Achieve";
        tag(w, "Achieve.tag", &mut self.tag)?;
        prim::w_i32(w, "Achieve.head", VA, &mut self.head)?;
        if w.is_loading() {
            self.data.resize_with(6, AchieveData::default);
        }
        for i in 0..self.data.len() {
            self.data[i].walk(w)?;
        }
        self.list.walk(w, "Achieve.list", C, VA, 4)?;
        if w.is_loading() {
            self.events.resize_with(8, Arr::default);
        }
        for i in 0..self.events.len() {
            self.events[i].walk(w, "Achieve.events", "ObjectArray<AchieveEvent>", 0x00495130)?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// ScenarioData 0x00997ad0 — the globals dump before the variable subsections.
// Fixed ranges total 10,183 bytes of direct walked scalars (computed from the
// decompiled range list) plus 6 Strings, three 10B blocks, and the subwalks.
// ---------------------------------------------------------------------------

/// ScenarioObjective link list node (PtrLinkListAbstract<ScenarioObjective,
/// int> 0x004c8420): i32 key + node metric i32 + tag + entry
/// (8B + String + String + tag + String + 10B).
#[derive(Default, Clone)]
pub struct ScenarioObjective {
    pub key: i32,
    pub metric: i32,
    pub tag: u8,
    pub a: Vec<u8>, // 8
    pub s0: Vec<u16>,
    pub s1: Vec<u16>,
    pub tag2: u8,
    pub s2: Vec<u16>,
    pub b: Vec<u8>, // 10
}

impl Body for ScenarioObjective {
    fn walk(&mut self, path: &str, w: &mut dyn DataWalk) -> R {
        const VA: u32 = 0x00999640;
        const C: &str = "ScenarioObjective";
        prim::w_i32(w, path, VA, &mut self.key)?;
        prim::w_i32(w, path, VA, &mut self.metric)?;
        tag(w, path, &mut self.tag)?;
        prim::take(w, path, &mut self.a, 8, C, VA)?;
        prim::wstr(w, path, &mut self.s0, C, VA)?;
        prim::wstr(w, path, &mut self.s1, C, VA)?;
        tag(w, path, &mut self.tag2)?;
        prim::wstr(w, path, &mut self.s2, C, VA)?;
        prim::take(w, path, &mut self.b, 10, C, VA)
    }
}

/// The ScenarioData globals block: fixed direct-byte ranges and small
/// structures in exact walk order. Element grammars for the counted
/// sub-collections fail closed when nonzero.
#[derive(Default, Clone)]
pub struct ScenarioData {
    pub g0: Vec<u8>,  // 32 (8 x i32)
    pub g1: Vec<u8>,  // 96 (8 slots x 3 i32)
    pub g2: Vec<u8>,  // 4
    pub g3: Vec<u8>,  // 124 (cc2210..cc228c)
    pub g4: Vec<u8>,  // 32 (cc2190..cc21b0)
    pub g5: Vec<u8>,  // 16 (cc0320..cc0330 u16 x8)
    pub g6: Vec<u8>,  // 5632 (cc0330..cc1930 u16 x8 blocks of 0x160)
    pub g7: Vec<u8>,  // 2064 (cc1970..cc2180 u16 x8 blocks of 0x81)
    pub g8: Vec<u8>,  // 8 (cc2180..88)
    pub g9: Vec<u8>,  // 64 (cc22a0..cc22e0, 8x8)
    pub g10: Vec<u8>, // 8 (cc21b8..c0)
    pub g11: Vec<u8>, // 8 (cc21e0..e8)
    pub g12: Vec<u8>, // 14 (scattered cb/cbe singles)
    pub strs: Vec<Vec<u16>>, // 6
    pub g13: Vec<u8>, // 10 + 10 + 10
    /// FUN_004c8070: i32 count + per entry (i32 + String).
    pub components: Vec<(i32, Vec<u16>)>,
    /// FUN_004c7060: ObjectArray of virtual-walked entries — undecoded.
    pub obj_array: UnknownArr,
    /// FUN_004c7270: count + per entry (u8 + virtual walk) — undecoded.
    pub obj_list: UnknownCnt,
    /// Checksum-skipped pair: 8B + count + data (mask==0 only).
    pub cs_skip_head: Vec<u8>,
    pub cs_skip_data: Vec<u8>,
    pub wcoords: Arr<Row<8>>,
    pub g14: Vec<u8>, // 8 (c8cb60..68)
    pub g15: Vec<u8>, // count-prefixed data (c8cb6c..)
    /// 8 x PtrLinkListAbstract<ScenarioObjective,int>.
    pub objectives: Vec<Cnt<ScenarioObjective>>,
    /// FUN_004c7db0 NamedObjectArray<ScenarioGroup> — undecoded rows.
    pub groups: UnknownArr,
    /// FUN_004c75e0 counted list — undecoded.
    pub reveal: UnknownCnt,
    /// 2 x 8 FUN_004c78b0 ObjectArrays (ed63f0..ed6570) — undecoded rows.
    pub lists0: Vec<UnknownArr>,
    /// 8 x SimpleArray<int>.
    pub simple_lists: Vec<SimpleVec>,
    pub g16: i32,
}

impl ScenarioData {
    pub(crate) fn walk(&mut self, w: &mut dyn DataWalk) -> R {
        const VA: u32 = 0x00997ad0;
        const C: &str = "ScenarioData";
        prim::take(w, "ScenarioData.g0", &mut self.g0, 32, C, VA)?;
        prim::take(w, "ScenarioData.g1", &mut self.g1, 96, C, VA)?;
        prim::take(w, "ScenarioData.g2", &mut self.g2, 4, C, VA)?;
        prim::take(w, "ScenarioData.g3", &mut self.g3, 124, C, VA)?;
        prim::take(w, "ScenarioData.g4", &mut self.g4, 32, C, VA)?;
        prim::take(w, "ScenarioData.g5", &mut self.g5, 16, C, VA)?;
        prim::take(w, "ScenarioData.g6", &mut self.g6, 5632, C, VA)?;
        prim::take(w, "ScenarioData.g7", &mut self.g7, 2064, C, VA)?;
        prim::take(w, "ScenarioData.g8", &mut self.g8, 8, C, VA)?;
        prim::take(w, "ScenarioData.g9", &mut self.g9, 64, C, VA)?;
        prim::take(w, "ScenarioData.g10", &mut self.g10, 8, C, VA)?;
        prim::take(w, "ScenarioData.g11", &mut self.g11, 8, C, VA)?;
        prim::take(w, "ScenarioData.g12", &mut self.g12, 14, C, VA)?;
        if w.is_loading() && self.strs.is_empty() {
            self.strs.resize_with(6, Vec::new);
            self.objectives.resize_with(8, Cnt::default);
            self.lists0.resize_with(16, || UnknownArr::named("ScenarioData.lists0"));
            self.simple_lists.resize_with(8, SimpleVec::default);
        }
        for i in 0..6 {
            prim::wstr(w, &format!("ScenarioData.strs[{i}]"), &mut self.strs[i], C, VA)?;
        }
        prim::take(w, "ScenarioData.g13", &mut self.g13, 30, C, VA)?;
        // FUN_004c8070: counted (u8 + String) component list.
        let mut n = if w.is_loading() { 0 } else { self.components.len() as i32 };
        prim::w_i32(w, "ScenarioData.components", VA, &mut n)?;
        if w.is_loading() {
            if !(0..=1 << 20).contains(&n) {
                return Err(w.fail(C, VA, format!("components count {n}")));
            }
            self.components.clear();
            self.components.resize(n as usize, (0, Vec::new()));
        }
        for i in 0..self.components.len() {
            let (u, s) = &mut self.components[i];
            prim::w_i32(w, "ScenarioData.components[]", VA, u)?;
            prim::wstr(w, "ScenarioData.components[]", s, C, VA)?;
        }
        self.obj_array.walk("ScenarioData.obj_array", w)?;
        self.obj_list.walk(w, "ScenarioData.obj_list", VA)?;
        if !w.is_checksum() {
            count_run(w, "ScenarioData.cs_skip", &mut self.cs_skip_head, &mut self.cs_skip_data, C, VA)?;
        }
        self.wcoords.walk(w, "ScenarioData.wcoords", "Array<WCoordData>", 0x00478990)?;
        // Second count-run pair walks unconditionally (8B head then data).
        count_run(w, "ScenarioData.g15", &mut self.g14, &mut self.g15, C, VA)?;
        for i in 0..8 {
            self.objectives[i].walk(w, &format!("ScenarioData.objectives[{i}]"), C, 0x004c8420)?;
        }
        self.groups.walk("ScenarioData.groups", w)?;
        self.reveal.walk(w, "ScenarioData.reveal", VA)?;
        for i in 0..16 {
            self.lists0[i].walk(&format!("ScenarioData.lists0[{i}]"), w)?;
        }
        for i in 0..8 {
            self.simple_lists[i].walk(w, &format!("ScenarioData.simple_lists[{i}]"), C, VA, 4)?;
        }
        prim::w_i32(w, "ScenarioData.g16", VA, &mut self.g16)
    }
}

/// RunTimeEnv 0x009c41a0: tag + i32 record count + per-record bodies
/// (FUN_009c63b0). FUN_009c40a0 takes no visitor — load-side reset only.
#[derive(Default, Clone)]
pub struct RunTimeEnv {
    pub tag: u8,
    pub count: i32,
    pub records: Vec<EnvRecord>,
}

/// Element of a record's FUN_004cccd0 pointer array (FUN_009c5f30, object
/// size 0xcc): tag + i32/n pair + count-prefixed data + SimpleArray<int> +
/// SimpleArray<u8> + 3 × ObjectArray<String> + String + 12B.
#[derive(Default, Clone)]
pub struct EnvSub {
    pub tag: u8,
    pub head: Vec<u8>, // 8: i32 + data count
    pub data: Vec<u8>,
    pub ints: SArr,
    pub bytes: SimpleVec,
    pub s0: Arr<WStr>,
    pub s1: Arr<WStr>,
    pub s2: Arr<WStr>,
    pub name: Vec<u16>,
    pub tail: Vec<u8>, // 12
}

impl Body for EnvSub {
    fn walk(&mut self, path: &str, w: &mut dyn DataWalk) -> R {
        const VA: u32 = 0x009c5f30;
        const C: &str = "RunTimeEnv.Sub";
        tag(w, path, &mut self.tag)?;
        if !w.is_loading() {
            let n = self.data.len() as i32;
            self.head[4..8].copy_from_slice(&n.to_le_bytes());
        }
        prim::take(w, path, &mut self.head, 8, C, VA)?;
        let n = i32::from_le_bytes(self.head[4..8].try_into().unwrap());
        if !(0..=1 << 24).contains(&n) {
            return Err(w.fail(C, VA, format!("{path} data count {n}")));
        }
        if n != 0 {
            prim::take(w, path, &mut self.data, n as usize, C, VA)?;
        }
        self.ints.walk(&format!("{path}.ints"), w)?;
        self.bytes.walk(w, &format!("{path}.bytes"), C, VA, 1)?;
        self.s0.walk(w, &format!("{path}.s0"), "ObjectArray<String>", 0x00490fb0)?;
        self.s1.walk(w, &format!("{path}.s1"), "ObjectArray<String>", 0x00490fb0)?;
        self.s2.walk(w, &format!("{path}.s2"), "ObjectArray<String>", 0x00490fb0)?;
        prim::wstr(w, path, &mut self.name, C, VA)?;
        prim::take(w, path, &mut self.tail, 12, C, VA)
    }
}

/// FUN_004cd230 counted list: i32 count + n × 7B nodes (i16 + 5B).
#[derive(Default, Clone)]
pub struct EnvList7 {
    pub len: i32,
    pub nodes: Vec<u8>,
}

impl EnvList7 {
    pub(crate) fn walk(&mut self, w: &mut dyn DataWalk, path: &str, va: u32) -> R {
        if !w.is_loading() {
            self.len = (self.nodes.len() / 7) as i32;
        }
        prim::w_i32(w, path, va, &mut self.len)?;
        if w.is_loading() && !(0..=4_000_000).contains(&self.len) {
            return Err(w.fail("EnvList7", va, format!("{path} count {}", self.len)));
        }
        prim::take(w, path, &mut self.nodes, self.len.max(0) as usize * 7, "EnvList7", va)
    }
}

#[derive(Default, Clone)]
pub struct EnvRecord {
    pub tag: u8,
    pub a: SimpleVec, // SimpleArray<u8>
    pub subs: PtrVec<EnvSub>, // FUN_004cccd0
    pub c: SimpleVec, // SimpleArray<int>
    // Everything below is gated `param_1[2] == 0` — walked on save/load,
    // skipped by CheckSum.
    pub d: Arr<WStr>, // ObjectArray<String>
    pub s: Vec<u16>,
    pub e: SimpleVec,
    pub f: EnvList7,  // FUN_004cd230
    pub tail: Vec<u8>, // +0xd4..+0xdd 9B
}

impl Body for EnvRecord {
    fn walk(&mut self, path: &str, w: &mut dyn DataWalk) -> R {
        const VA: u32 = 0x009c63b0;
        const C: &str = "RunTimeEnv.Record";
        tag(w, path, &mut self.tag)?;
        self.a.walk(w, &format!("{path}.a"), C, 0x0049a090, 1)?;
        self.subs.walk(w, &format!("{path}.subs"), C, 0x004cccd0)?;
        self.c.walk(w, &format!("{path}.c"), C, VA, 4)?;
        if !w.is_checksum() {
            self.d.walk(w, &format!("{path}.d"), "ObjectArray<String>", 0x00490fb0)?;
            prim::wstr(w, path, &mut self.s, C, VA)?;
            self.e.walk(w, &format!("{path}.e"), C, VA, 4)?;
            self.f.walk(w, &format!("{path}.f"), 0x004cd230)?;
            prim::take(w, path, &mut self.tail, 9, C, VA)?;
        }
        Ok(())
    }
}

impl RunTimeEnv {
    pub(crate) fn walk(&mut self, w: &mut dyn DataWalk) -> R {
        const VA: u32 = 0x009c41a0;
        const C: &str = "RunTimeEnv";
        tag(w, "RunTimeEnv.tag", &mut self.tag)?;
        if !w.is_loading() {
            self.count = self.records.len() as i32;
        }
        prim::w_i32(w, "RunTimeEnv.count", VA, &mut self.count)?;
        if w.is_loading() {
            if !(0..=1 << 20).contains(&self.count) {
                return Err(w.fail(C, VA, format!("record count {}", self.count)));
            }
            self.records.clear();
            self.records.resize_with(self.count as usize, EnvRecord::default);
        }
        for i in 0..self.records.len() {
            self.records[i].walk(&format!("RunTimeEnv.records[{i}]"), w)?;
        }
        Ok(())
    }
}

/// Final direct globals after RunTimeEnv: tag + 8B + count-prefixed data +
/// 341B (0x00c06180 block, decomp 005a2360 lines 1044–1050).
#[derive(Default, Clone)]
pub struct FinalGlobals {
    pub tag: u8,
    pub head: Vec<u8>, // 8 (i32 + count at +8)
    pub data: Vec<u8>, // count bytes at +0x10
    pub tail: Vec<u8>, // 341 (+0x14..+0x169)
}

impl FinalGlobals {
    pub(crate) fn walk(&mut self, w: &mut dyn DataWalk) -> R {
        const C: &str = "WalkDataGame.final";
        tag(w, "WalkDataGame.final.tag", &mut self.tag)?;
        count_run(w, "WalkDataGame.final.data", &mut self.head, &mut self.data, C, 0)?;
        prim::take(w, "WalkDataGame.final.tail", &mut self.tail, 341, C, 0)
    }
}

// ---------------------------------------------------------------------------
// Rules tail — Game::walk_rules_data 0x00589550. The same function serializes
// the section and drives the `rules` checksum channel: walk_test tags are
// no-ops for CheckSum and the checksum-gated `String::walk_data` calls
// (`param_1[2] != 0` skips them) emit only in load/save mode.
//
// Types::walk_rules_data (0x00669800) makes 806 virtual dispatches through
// vtable slot +0xc4 in global TypeIndex order. The shipped slot bands
// (docs/mechanics/bhs-type-channel13-frontier.md):
//   0..50 GoodType, 50..414 UnitType, 414..543 BuildType, 543 ObjectType,
//   544..629 TechType, 629..684 SpellType, 684..806 Type (BonusType).
// ---------------------------------------------------------------------------

/// Serialized width of one shipped `Game::walk_rules_data` section
/// (`0x00589550`) for this install: tag + 806 type records + Constants
/// (0xd40) + duplicated dword + Balance (493*493*2) + 24 tribes. The string
/// data inside type records makes it install-specific, but every shipped
/// capture agrees on this width (matches don-replay's
/// `SHIPPED_RULES_SERIALIZED_BYTES`).
pub const RULES_SERIALIZED_BYTES: usize = 1_024_221;

/// Tail grammar after `final_globals` (`WalkDataGame::walk_data` 0x005a2360
/// ends with `Game::walk_rules_data` 0x00589550):
///   [opaque scenario/RunTimeEnv-serialized bytes] + [Rules] + [opaque trailer].
/// The leading span is bounded by the self-authenticating Rules scan (every
/// type record serializes its slot index at image[0..4), so a false tag
/// cannot survive the first two records); the trailing span is bounded by
/// EOF. Both opaque spans are reported stop points: the leading one is
/// script-VM serialization (bytecode, const pools, editor paths) whose
/// interior counts live in globals we have not yet resolved, and the trailer
/// (~840 KB, i32-grid-like) has no known writer in the walk — no section
/// between `final_globals` and EOF accounts for it.
#[derive(Default, Clone)]
pub struct RulesTail {
    pub pre_rules: Vec<u8>,
    pub rules: Rules,
    pub post_rules: Vec<u8>,
}

/// Scan `buf[start..]` for the Rules section: a `0x92` tag followed by a
/// zero type_index, then a full typed `Rules::walk` that must consume
/// exactly `RULES_SERIALIZED_BYTES`. Returns the absolute offset.
pub fn find_rules_boundary(buf: &[u8], start: usize) -> Option<usize> {
    let scan_end = buf.len().checked_sub(RULES_SERIALIZED_BYTES)? + 1;
    for o in start.min(scan_end)..scan_end {
        if buf[o] != 0x92 {
            continue;
        }
        if buf[o + 1..o + 5] != [0, 0, 0, 0] {
            continue;
        }
        let mut probe = Loader::new(&buf[o..o + RULES_SERIALIZED_BYTES]);
        let mut rules = Rules::default();
        if rules.walk(&mut probe).is_ok() && probe.pos == RULES_SERIALIZED_BYTES {
            return Some(o);
        }
    }
    None
}

impl RulesTail {
    pub(crate) fn walk(&mut self, w: &mut dyn DataWalk) -> R {
        const C: &str = "RulesTail";
        const VA: u32 = 0x00589550;
        if w.is_loading() {
            let off = w.rules_boundary().ok_or_else(|| {
                w.fail(C, VA, "no self-validating Rules section found in tail".into())
            })?;
            self.pre_rules.clear();
            self.pre_rules.resize(off, 0);
        }
        w.walk_bytes("RulesTail.pre_rules", &mut self.pre_rules)?;
        self.rules.walk(w)?;
        if w.is_loading() {
            let n = w.remaining();
            self.post_rules.clear();
            self.post_rules.resize(n, 0);
        }
        w.walk_bytes("RulesTail.post_rules", &mut self.post_rules)
    }
}

/// Per-slot walker for one serialized type record. The serialized image
/// begins with the slot's `type_index` (i32 == slot), which the load path
/// validates — it is the self-authentication the boundary scan relies on.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeRuleKind {
    Good,
    Unit,
    Build,
    Object,
    Tech,
    Spell,
    Type,
}

impl TypeRuleKind {
    pub fn for_slot(slot: usize) -> Self {
        match slot {
            0..=49 => Self::Good,
            50..=413 => Self::Unit,
            414..=542 => Self::Build,
            543 => Self::Object,
            544..=628 => Self::Tech,
            629..=683 => Self::Spell,
            _ => Self::Type,
        }
    }
}

/// One serialized type record (Type::walk_rules_data 0x00663190 base +
/// ObjectType 0x0065fba0 + per-kind tails):
///   base      image[4..94) (90B) + checksum-gated String
///   object    image[0x1e4..0x27c) (152B) + 2 x SimpleArray<u16>
///   unit tail image[0x2b4..0x2cc)(24) + [0x2d4..0x2dc)(8) + [0x2dc..0x2e0)(4)
///             + [0x2e0..0x5d4)(756)
///   build     image[0x2b4..0x2e5) (49B)
///   good      image[0x2b4..0x2f8) (68B)
///   tech      image[0x1c8..0x1e3) (27B) + 8 checksum-gated Strings
///   spell     image[0x1c8..0x1f8) (48B)
#[derive(Default, Clone)]
pub struct TypeRec {
    pub head: Vec<u8>,
    pub name: Vec<u16>,
    pub obj_mid: Vec<u8>,
    pub arr0: SimpleVec,
    pub arr1: SimpleVec,
    pub ext: Vec<u8>,
    pub tech_strs: Vec<Vec<u16>>,
}

impl TypeRec {
    #[allow(dead_code)]
    fn walk(&mut self, slot: usize, w: &mut dyn DataWalk) -> R {
        const C: &str = "TypeRec";
        const VA: u32 = 0x00663190;
        let kind = TypeRuleKind::for_slot(slot);
        let path = format!("Types[{slot}]");
        prim::take(w, &path, &mut self.head, 90, C, VA)?;
        if w.is_loading()
            && i32::from_le_bytes(self.head[..4].try_into().unwrap()) != slot as i32
        {
            return Err(w.fail(
                C,
                VA,
                format!("{path}: serialized type_index != slot"),
            ));
        }
        if !w.is_checksum() {
            prim::wstr(w, &path, &mut self.name, C, VA)?;
        }
        match kind {
            TypeRuleKind::Tech => {
                prim::take(w, &path, &mut self.ext, 27, C, 0x0066d5c0)?;
                if w.is_loading() {
                    self.tech_strs.resize_with(8, Vec::new);
                }
                if !w.is_checksum() {
                    for i in 0..self.tech_strs.len() {
                        prim::wstr(w, &path, &mut self.tech_strs[i], C, 0x0066d5c0)?;
                    }
                }
            }
            TypeRuleKind::Spell => {
                prim::take(w, &path, &mut self.ext, 48, C, 0x00675400)?;
            }
            TypeRuleKind::Type => {}
            _ => {
                prim::take(w, &path, &mut self.obj_mid, 152, C, 0x0065fba0)?;
                self.arr0.walk(w, &path, C, 0x00476610, 2)?;
                self.arr1.walk(w, &path, C, 0x00476610, 2)?;
                let n = match kind {
                    TypeRuleKind::Unit => 792,   // 24 + 8 + 4 + 756
                    TypeRuleKind::Build => 49,
                    TypeRuleKind::Good => 68,
                    TypeRuleKind::Object => 0,
                    _ => unreachable!(),
                };
                prim::take(w, &path, &mut self.ext, n, C, 0x0061d190)?;
            }
        }
        Ok(())
    }
}

/// Tribe record tail walk: 24 records, each `tag + image[0x54..0x6c) (24B) +
/// image[0x70..0x5f0) (1408B)`.
#[derive(Default, Clone)]
pub struct TribeRec {
    pub tag: u8,
    pub a: Vec<u8>,
    pub b: Vec<u8>,
}

impl TribeRec {
    #[allow(dead_code)]
    fn walk(&mut self, slot: usize, w: &mut dyn DataWalk) -> R {
        let path = format!("Tribes[{slot}]");
        tag(w, &path, &mut self.tag)?;
        prim::take(w, &path, &mut self.a, 24, "Tribe", 0x00589550)?;
        prim::take(w, &path, &mut self.b, 1408, "Tribe", 0x00589550)
    }
}

/// Game::walk_rules_data 0x00589550: tag + 806 typed records + Constants
/// 0xd40 + duplicated dword at +0x804 + Balance (493*493*2) + 24 tribes.
#[derive(Default, Clone)]
pub struct Rules {
    pub tag: u8,
    pub types: Vec<TypeRec>,
    pub constants: Vec<u8>,
    pub const_dup: Vec<u8>,
    pub balance: Vec<u8>,
    pub tribes: Vec<TribeRec>,
}

impl Rules {
    #[allow(dead_code)]
    pub(crate) fn walk(&mut self, w: &mut dyn DataWalk) -> R {
        const C: &str = "Rules";
        const VA: u32 = 0x00589550;
        tag(w, "Rules.tag", &mut self.tag)?;
        if w.is_loading() {
            self.types.resize_with(806, TypeRec::default);
            self.tribes.resize_with(24, TribeRec::default);
        }
        for i in 0..self.types.len() {
            self.types[i].walk(i, w)?;
        }
        prim::take(w, "Rules.constants", &mut self.constants, 0xd40, C, VA)?;
        prim::take(w, "Rules.const_dup", &mut self.const_dup, 4, C, VA)?;
        prim::take(w, "Rules.balance", &mut self.balance, 493 * 493 * 2, C, 0x00582cc0)?;
        for i in 0..self.tribes.len() {
            self.tribes[i].walk(i, w)?;
        }
        Ok(())
    }
}
