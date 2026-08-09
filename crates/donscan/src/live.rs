//! `live` — read the **live entity graph** out of a running `riseofnations.exe`.
//!
//! This is the layer above `vtables`. A vtable census tells you *what classes exist
//! on the heap*; it cannot tell you which pool slots are alive, where anything is, or
//! what type it is. All three of those come from named fields in `ron-bin/sbl/rise.pdb`
//! plus the engine's own code, and they are recorded here with their provenance.
//!
//! # The three facts this module exists to get right
//!
//! **1. The live set is a real engine structure, not a heuristic.** `ObjectsData::lists`
//! is `ObjectsArray[10]` at `Objects+0x04` — ten `PtrArray<Object>`, one per owner slot.
//! `Objects::process_all` (`0x0065DCE0`) walks exactly these, and it walks them in three
//! **fixed index bands**: units `[0, unit_mark[s])`, buildings `[2000, build_mark[s])`,
//! walls `[3000, wall_mark[s])`, where `unit_mark`/`build_mark`/`wall_mark` are
//! `int[10]` at `ObjectsData+348/+388/+428`. So the engine's own iteration order is
//! reproducible and the bands classify an object before its vtable is even read.
//!
//! **2. The validity field is `SubObjectData::flags & 1`.** `SubObjectData::is_active`
//! is `0x0046CDA0`, eight bytes of machine code:
//! `movzx eax,byte [ecx+8]; and eax,1; ret`. `flags` is the `unsigned char` at
//! `SubObjectData+0x08` in the PDB. `Objects::process_all` inlines exactly this test
//! before dispatching `vtable+0x9C`, and decrements `hold_frames` (`+0x32`) otherwise.
//! That is the field, not a heuristic: a pool slot with `flags & 1 == 0` is free.
//! (`WallData::is_active` `0x00472350` is `flags & 4` — a *different* question, "is this
//! wall segment raised"; band membership plus bit 0 is what liveness means.)
//!
//! **3. Coords are XOR-obfuscated in memory.** `SubObjectData` names them
//! `z_internal`/`x_internal`/`y_internal` at `+0x0C/+0x10/+0x14`, and every reader in
//! the binary XORs by `0x00063637` before use (e.g. `0x005F7740`, `0x0063DC70`,
//! `0x00688E10`, `0x008CC700` — `*(uint *)(o + 0x10) ^ 0x63637`). A position read
//! without unmasking is garbage. `GuyData` coords (`GuyData+0x0C/0x10`) are **not**
//! obfuscated; this module reads the `Object` ones.
//!
//! Everything here is Tier **C**: behavioural observation of a live process. The offsets
//! are `[measured]` from the shipped PDB and from disassembly of the functions named
//! above; that a field *means* what its name says is the PDB's claim, not a proof.

use crate::vtables::{VtMap, NONE};

// ---------------------------------------------------------------------------
// Static addresses (preferred image base 0x00400000; add the runtime delta)
// ---------------------------------------------------------------------------

/// `GameAccess::game : Game&` — holds a `Game*`. [PDB + `Objects::process_all`]
pub const VA_GAME: u32 = 0x00C0_61EC;
/// `GameAccess::world : World&` — holds a `World*`.
pub const VA_WORLD: u32 = 0x00C0_6188;
/// `GameAccess::objects : Objects&` — holds an `Objects*`.
pub const VA_OBJECTS: u32 = 0x00C0_618C;
/// `GameAccess::turn_control : TurnControl&` — holds a `TurnControl*`.
pub const VA_TURN_CONTROL: u32 = 0x00C0_6180;
/// `?leaders@@3VLeaders@@A` — the `Leader[8]` array itself, not a pointer.
pub const VA_LEADERS: u32 = 0x00E3_A390;
/// `sizeof(Leader)`, measured: the array runs `0x00E3A390..0x00E71AF0`, 8 × `0x6EEC`.
pub const LEADER_STRIDE: u32 = 0x6EEC;
pub const LEADER_COUNT: u32 = 8;

/// Source-image identity used by both `donscan` and the targeted feed. These fields
/// are unchanged by the Windows loader and identify the supported retail executable.
pub const PREFERRED_IMAGE_BASE: u32 = 0x0040_0000;
pub const SOURCE_ENTRY_RVA: u32 = 0x0015_D699;
pub const SOURCE_IMAGE_SIZE: u32 = 0x00BB_4000;
pub const SOURCE_FILE_SIZE: u64 = 9_925_120;
pub const SOURCE_SHA256: [u8; 32] = [
    0x30, 0x47, 0x8a, 0x44, 0xb5, 0x77, 0xcb, 0x11, 0xeb, 0xcb, 0xbb, 0xf5, 0x3d, 0x3e, 0x93, 0xba,
    0x02, 0xfd, 0x2a, 0xac, 0xf3, 0xbd, 0xef, 0xa6, 0x55, 0x2c, 0x9b, 0x64, 0x49, 0x62, 0x50, 0x79,
];

/// `Game::frame` — incremented at `0x005924BF`, the sim frame counter.
pub const OFF_GAME_FRAME: u32 = 0x550;
/// `Game::seconds` — `frame % 15 == 0` bumps it (`0x005924CF`).
pub const OFF_GAME_SECONDS: u32 = 0x560;
/// First byte of `Game::semaphore.ptr`. Bit 2 is the engine's network-game
/// command-stream flag: it gates multiplayer packet scrambling/checksums at
/// `CommandPackage::process_all` (`0x0094c6a9`) and packet creation
/// (`0x009407a7`). A coherent active match with this bit clear is solo.
pub const OFF_GAME_SEMAPHORE_BYTES: u32 = 0x820;
pub const GAME_NETWORK_FLAG: u32 = 1 << 2;

/// `ScenarioFuncSet::is_paused` (`0x009E5BE0`) is exactly
/// `return (*(u8*)(GameAccess::turn_control + 0x10) & 1)`.
pub const OFF_TURN_CONTROL_FLAGS: u32 = 0x10;
pub const TURN_CONTROL_PAUSED_FLAG: u32 = 1;

// --- Object / SubObjectData / ObjectData -----------------------------------

/// The XOR mask every `Object` coordinate is stored under.
pub const COORD_MASK: u32 = 0x0006_3637;

pub const OFF_OBJ_VPTR: usize = 0x00;
pub const OFF_OBJ_FLAGS: usize = 0x08; // SubObjectData::flags,  bit 0 = is_active
pub const OFF_OBJ_WHO: usize = 0x09; // SubObjectData::who    (owner slot)
pub const OFF_OBJ_O: usize = 0x0A; // SubObjectData::o      (object index, i16)
pub const OFF_OBJ_Z: usize = 0x0C; // z_internal ^ COORD_MASK
pub const OFF_OBJ_X: usize = 0x10; // x_internal ^ COORD_MASK
pub const OFF_OBJ_Y: usize = 0x14; // y_internal ^ COORD_MASK
pub const OFF_OBJ_PTYPE: usize = 0x18; // SubObjectData::ptype : ObjectType*
pub const OFF_OBJ_MYHITS: usize = 0x20; // ObjectData::myhits  (ObjectData::hits() 0x006450C0)
pub const OFF_OBJ_DAMAGE: usize = 0x24; // ObjectData::damage
pub const OFF_OBJ_UID: usize = 0x30; // ObjectData::uid   (u16)
pub const OFF_OBJ_HOLD: usize = 0x32; // ObjectData::hold_frames (u16)
pub const OFF_UNIT_ANGLE: usize = 0x50; // UnitData::angle
pub const OFF_BUILD_CONSTRUCT_HITS: usize = 0x54; // WallData::construct_hits
pub const OFF_BUILD_QUEUED: usize = 0x82;
pub const OFF_BUILD_QUEUE_NUM: usize = 0x88;
pub const OFF_BUILD_QUEUE_LIST: usize = 0x8C;
pub const OFF_UNIT_IDLE_CACHE: usize = 0xB0;

/// Bytes read per object. Covers build construction HP/queue metadata and
/// `UnitData::idle`; it remains far smaller than even the shortest pool stride.
pub const OBJ_READ: usize = 0xB1;

// --- ObjectType / Type ------------------------------------------------------

/// `TypeData::type : TypeIndex` — the global type id (the balance-table row).
pub const OFF_TYPE_INDEX: usize = 0x04;
/// `TypeData::name : String`; the `wchar_t*` sits at `String+0`.
pub const OFF_TYPE_NAME: usize = 0x60;
/// `TypeData::display_name : String`.
pub const OFF_TYPE_DISPLAY: usize = 0x74;
/// `ObjectTypeData::hits` — the type's base hit points.
pub const OFF_TYPE_HITS: usize = 0x210;
/// `ObjectTypeData::armor`.
pub const OFF_TYPE_ARMOR: usize = 0x214;
/// `ObjectTypeData::attack` (stored ×10).
pub const OFF_TYPE_ATTACK: usize = 0x1E8;
/// `ObjectTypeData::x_size` / `y_size` — building footprint in tiles.
pub const OFF_TYPE_XSIZE: usize = 0x234;
pub const OFF_TYPE_YSIZE: usize = 0x238;

// --- ObjectsData ------------------------------------------------------------

/// `ObjectsData::lists : ObjectsArray[10]` begins here.
pub const OFF_OBJECTS_LISTS: usize = 4;
/// `sizeof(ObjectsArray)` = `sizeof(PtrArray<Object>)`.
pub const OBJECTS_ARRAY_STRIDE: usize = 28;
/// Within one `ObjectsArray`: `ArrayBaseMaster::length`.
pub const OFF_ARR_LENGTH: usize = 4;
/// `ArrayBaseMaster::size` (capacity).
pub const OFF_ARR_SIZE: usize = 8;
/// `ArrayBase<Object*>::list`.
pub const OFF_ARR_LIST: usize = 16;
/// `ObjectsData::unit_mark : int[10]`.
pub const OFF_UNIT_MARK: usize = 348;
/// `ObjectsData::build_mark : int[10]`.
pub const OFF_BUILD_MARK: usize = 388;
/// `ObjectsData::wall_mark : int[10]`.
pub const OFF_WALL_MARK: usize = 428;
/// Enough to cover `wall_mark[9]` at 428+36.
pub const OBJECTS_READ: usize = 544;

/// Band start indices, from `Objects::process_all` `0x0065DCE0`.
pub const BAND_UNIT_START: i32 = 0;
pub const BAND_BUILD_START: i32 = 2000;
pub const BAND_WALL_START: i32 = 3000;

pub const NUM_SLOTS: usize = 10;

// --- WorldData --------------------------------------------------------------

pub const OFF_WORLD_XS: usize = 0; // WCoord cells across
pub const OFF_WORLD_YS: usize = 4;
pub const OFF_WORLD_TILE_XS: usize = 24;
pub const OFF_WORLD_TILE_YS: usize = 28;
/// `WorldData::wdata : WData*` — `xs*ys` cells of 28 bytes, indexed `y*xs + x`
/// [measured, `WorldData::get_who` `0x006B4700`].
pub const OFF_WORLD_WDATA: usize = 308;
pub const WDATA_STRIDE: usize = 28;
pub const OFF_WDATA_LAND: usize = 2; // WData::land       (terrain class)
pub const OFF_WDATA_WHO: usize = 15; // WData::who        (territory owner, -1 = none)
pub const OFF_WDATA_WAS_SEEN: usize = 20;

// --- LeaderData -------------------------------------------------------------

pub const OFF_LEADER_FLAGS: usize = 0; // bit 0 = in play (Objects::process_all gates on it)
pub const OFF_LEADER_WHO: usize = 8;
pub const OFF_LEADER_TRIBE: usize = 12;
pub const OFF_LEADER_SCORE: usize = 24;
/// `LeaderData::econ` is AI bookkeeping, not the resource stockpile. The real
/// stockpile is reached through `data_encrypted` below.
pub const OFF_LEADER_AI_ECON: usize = 1104;
pub const OFF_LEADER_GATHER_STAMP: usize = 1964;
/// Exact population. PDB calls this `control`; `ScenarioFuncSet::population`
/// (`0x009E8E70`) returns this field. `LeaderData::pop` at `+0x95C` is an AI
/// class counter and must not be exposed as total population.
pub const OFF_LEADER_POP: usize = 2368;
pub const OFF_LEADER_POP_CAP: usize = 2020;
pub const OFF_LEADER_CITY_NUM: usize = 1016;
pub const OFF_LEADER_GATHER_SLOTS: usize = 2212;
pub const OFF_LEADER_FILLED_GATHER_SLOTS: usize = 2236;
pub const OFF_LEADER_FISHERMEN: usize = 2416;
pub const OFF_LEADER_IDLE_FISHERMEN: usize = 2420;
pub const OFF_LEADER_PEASANTS: usize = 2424;
pub const OFF_LEADER_SCHOLARS: usize = 2428;
pub const OFF_LEADER_FREE_PEASANTS: usize = 2492;
pub const OFF_LEADER_GATHERERS: usize = 2500;
pub const OFF_LEADER_ACTIVE_WARS: usize = 2484;
pub const OFF_LEADER_ATTACKED: usize = 2508;
pub const OFF_LEADER_QUEUE_COUNTS: usize = 2576;
pub const OFF_LEADER_TEAM_COLOR: usize = 26920;
pub const OFF_LEADER_ENCRYPTED: usize = 28344;
/// Through `LeaderData::data_encrypted`, without reading trailing variable arrays.
pub const LEADER_READ: usize = OFF_LEADER_ENCRYPTED + 4;

pub const LEADER_ENCRYPTED_READ: usize = 248;
pub const OFF_ENC_STOCKPILE: usize = 0x00;
pub const OFF_ENC_LEFTOVER: usize = 0x18;
pub const OFF_ENC_RESOURCE_CAP: usize = 0x30;
pub const OFF_ENC_OVER_CAP: usize = 0x4C;
pub const OFF_ENC_GROSS: usize = 0x64;
pub const OFF_ENC_SUPPORT: usize = 0x7C;
pub const OFF_ENC_INCOME: usize = 0x94;
pub const OFF_ENC_RATE: usize = 0xAC;
pub const OFF_ENC_BONUS: usize = 0xC4;
pub const OFF_ENC_AGES: usize = 0xDC;
pub const OFF_ENC_EPOCHS: usize = 0xE0;
pub const OFF_ENC_DISCOVERED: usize = 0xE4;
pub const OFF_ENC_EPOCH: usize = 0xE8;

pub const XOR_STOCKPILE: u32 = 0x8221;
pub const XOR_LEFTOVER: u32 = 0x3421;
pub const XOR_RESOURCE_CAP: u32 = 0x1281;
pub const XOR_OVER_CAP: u32 = 0x8932;
pub const XOR_GROSS: u32 = 0x0872;
pub const XOR_SUPPORT: u32 = 0x26076;
pub const XOR_INCOME: u32 = 0x90236;
pub const XOR_AI_PLANNING_RATE: u32 = 0x73862;
pub const XOR_BONUS: u32 = 0x6722;
pub const XOR_AGES: u32 = 0x62766;
pub const XOR_EPOCHS: u32 = 0x69587;
pub const XOR_DISCOVERED: u32 = 0x13985;
pub const XOR_EPOCH: u32 = 0x63187;

const MAX_ARRAY_SLOTS: i32 = 1 << 16;
const MAX_MAP_CELLS: usize = 1 << 20;
pub const DEFAULT_SNAPSHOT_ATTEMPTS: u8 = 3;

// ---------------------------------------------------------------------------
// Memory access
// ---------------------------------------------------------------------------

/// Anything that can read the target's address space. Implemented over
/// `ReadProcessMemory` in the feed binary and over a byte map in the tests, which
/// is what makes the parsing logic testable on a machine that cannot run the game.
pub trait Mem {
    /// Fills `buf` from `addr`. Returns bytes actually read; a short read is not an
    /// error (a region boundary or a page that went away mid-frame produces one).
    fn read(&self, addr: u64, buf: &mut [u8]) -> usize;
}

#[inline]
pub fn le_u16(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
#[inline]
pub fn le_i16(b: &[u8], o: usize) -> i16 {
    le_u16(b, o) as i16
}
#[inline]
pub fn le_u32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
#[inline]
pub fn le_i32(b: &[u8], o: usize) -> i32 {
    le_u32(b, o) as i32
}

/// Unmask an `Object` coordinate. The engine stores `value ^ 0x00063637`.
#[inline]
pub fn unmask_coord(raw: u32) -> i32 {
    (raw ^ COORD_MASK) as i32
}

// ---------------------------------------------------------------------------
// Snapshot types
// ---------------------------------------------------------------------------

/// Which band of `Objects::lists[s]` an object came out of. This is the engine's own
/// classification and it is available before the vtable is consulted.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[repr(u8)]
pub enum Band {
    Unit = 0,
    Build = 1,
    Wall = 2,
}

/// Finer kind, from the object's vtable via `schema/vtables.json`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Kind {
    Unit = 0,
    Build = 1,
    Wall = 2,
    Animal = 3,
    Ammo = 4,
    Caravan = 5,
    Other = 6,
}

pub fn kind_of(class: Option<&str>, band: Band) -> Kind {
    match class {
        Some("Unit") => Kind::Unit,
        Some("Build") => Kind::Build,
        Some("Wall") => Kind::Wall,
        Some("Animal") => Kind::Animal,
        Some("Ammo") => Kind::Ammo,
        Some("Caravan") => Kind::Caravan,
        _ => match band {
            Band::Unit => Kind::Other,
            Band::Build => Kind::Build,
            Band::Wall => Kind::Wall,
        },
    }
}

#[derive(Clone, Copy, Debug)]
pub struct LiveObject {
    pub addr: u32,
    pub uid: u16,
    pub o: i16,
    /// `TypeData::type` read through `ptype`; `u16::MAX` if `ptype` was unreadable.
    pub type_index: u16,
    pub ptype: u32,
    /// Fine world units (1 tile = 192).
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub angle: i32,
    pub hp: i32,
    pub max_hp: i32,
    pub who: u8,
    pub slot: u8,
    pub band: Band,
    pub kind: Kind,
    pub flags: u8,
    /// Raw PDB field `UnitData::idle`. This is a cache byte, not the result of
    /// retail `UnitData::is_idle`, which walks the order list.
    pub idle_cache: bool,
    /// Which values are authoritative. In particular, razing buildings have an HP
    /// calculation that calls retail code we cannot safely execute out-of-process.
    pub validity: u16,
}

pub const OBJECT_VALID_POSITION: u16 = 1 << 0;
pub const OBJECT_VALID_HP: u16 = 1 << 1;
pub const OBJECT_VALID_TYPE: u16 = 1 << 2;
pub const OBJECT_VALID_IDLE_CACHE: u16 = 1 << 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ObjectIdentity {
    pub slot: u8,
    pub band: Band,
    pub o: i16,
    pub uid: u16,
}

impl LiveObject {
    pub fn identity(&self) -> ObjectIdentity {
        ObjectIdentity {
            slot: self.slot,
            band: self.band,
            o: self.o,
            uid: self.uid,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct LiveLeader {
    pub index: u8,
    pub leader_flags: u32,
    pub who: i32,
    pub tribe: i32,
    pub score: i32,
    pub pop: i32,
    pub pop_cap: i32,
    pub city_num: i32,
    /// Unsigned simulation frame of the last `Leader::calc_gather` cache refresh.
    pub gather_stamp: u32,
    pub team_color: u8,
    pub free_peasants: i32,
    pub gatherers: i32,
    pub fishermen: i32,
    pub idle_fishermen: i32,
    pub peasants: i32,
    pub scholars: i32,
    pub active_wars: i32,
    pub attacked: i32,
    pub gather_slots: [i32; 6],
    pub filled_gather_slots: [i32; 6],
    /// Engine cache of queued attacking-unit class counts: barracks, stable,
    /// factory, combat, dock, air. This is not producer queue depth.
    pub queued_attack_class_cache: [i32; 6],
    /// Decoded `LeaderDataEncrypt` fields.
    pub stockpile: [i32; 6],
    pub leftover: [i32; 6],
    pub resource_cap: [i32; 7],
    pub over_cap: [i32; 6],
    pub gross: [i32; 6],
    pub support: [i32; 6],
    pub income: [i32; 6],
    /// AI planning cache, not live income. It may be stale for a human player.
    pub ai_planning_rate: [i32; 6],
    pub bonus: [i32; 6],
    pub ages: i32,
    pub epochs: i32,
    pub discovered: i32,
    pub epoch: [i32; 4],
    pub validity: u32,
}

pub const LEADER_VALID_CORE: u32 = 1 << 0;
pub const LEADER_VALID_ECON: u32 = 1 << 1;
pub const LEADER_VALID_QUEUED_ATTACK_CACHE: u32 = 1 << 2;

#[derive(Clone, Debug, Default)]
pub struct SlotStat {
    pub band_span: [i32; 3],
    pub band_live: [i32; 3],
    pub array_length: i32,
    pub array_size: i32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum Coherence {
    #[default]
    Unavailable = 0,
    Coherent = 1,
    Torn = 2,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum GameMode {
    #[default]
    Unknown = 0,
    SinglePlayer = 1,
    Multiplayer = 2,
}

fn game_mode_from_semaphore(word: Option<u32>) -> GameMode {
    match word {
        Some(value) if value & GAME_NETWORK_FLAG != 0 => GameMode::Multiplayer,
        Some(_) => GameMode::SinglePlayer,
        None => GameMode::Unknown,
    }
}

pub const COMPONENT_GAME: u32 = 1 << 0;
pub const COMPONENT_WORLD: u32 = 1 << 1;
pub const COMPONENT_OBJECTS: u32 = 1 << 2;
pub const COMPONENT_LEADERS: u32 = 1 << 3;
pub const COMPONENT_ECON: u32 = 1 << 4;

#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub ok: bool,
    pub note: String,
    pub game_frame: u32,
    pub frame_start: u32,
    pub frame_end: u32,
    pub game_seconds: u32,
    /// Current command-stream mode, derived from the guarded network-game bit in
    /// `Game::semaphore`. This is Unknown if that byte could not be read.
    pub game_mode: GameMode,
    /// Exact engine pause predicate, or None if TurnControl was unavailable.
    pub paused: Option<bool>,
    pub world_xs: i32,
    pub world_ys: i32,
    pub tile_xs: i32,
    pub tile_ys: i32,
    pub objects: Vec<LiveObject>,
    pub leaders: Vec<LiveLeader>,
    pub slots: Vec<SlotStat>,
    /// Distinct `ObjectType*` seen this frame, for the type-name cache.
    pub ptypes: Vec<u32>,
    /// How many `ReadProcessMemory` calls the snapshot cost, and how many bytes.
    pub reads: u32,
    pub bytes: u64,
    pub short_reads: u32,
    /// Slots visited but skipped because `flags & 1` was clear — the free pool slots
    /// a vtable census would have counted.
    pub free_slots_skipped: u32,
    pub duplicate_pointers: u32,
    pub duplicate_identities: u32,
    pub invalid_ranges: u32,
    pub coherence: Coherence,
    pub retry_count: u8,
    pub valid_components: u32,
    /// Unique active human/console slot (`leader_flags & 4`), or -1 if missing
    /// or ambiguous. Consumers must not assume a fixed player number.
    pub human_slot: i8,
}

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

struct Reader<'m, M: Mem> {
    mem: &'m M,
    reads: u32,
    bytes: u64,
    short_reads: u32,
}

impl<'m, M: Mem> Reader<'m, M> {
    fn block(&mut self, addr: u64, len: usize) -> Option<Vec<u8>> {
        let mut b = vec![0u8; len];
        let n = self.mem.read(addr, &mut b);
        self.reads += 1;
        self.bytes += n as u64;
        if n < len {
            self.short_reads += 1;
            return None;
        }
        Some(b)
    }
    fn u32(&mut self, addr: u64) -> Option<u32> {
        self.block(addr, 4).map(|b| le_u32(&b, 0))
    }
}

/// Group sorted addresses into as few block reads as possible. Object pools are packed
/// arrays, so a whole slot's units usually collapse into one or two reads; scattered
/// objects cost one each. Without this a 2,000-object frame is 2,000 syscalls.
fn coalesce(addrs: &[u32], obj_len: usize, max_gap: u64, max_block: usize) -> Vec<(u64, usize)> {
    let mut out: Vec<(u64, usize)> = Vec::new();
    let mut i = 0usize;
    while i < addrs.len() {
        let start = addrs[i] as u64;
        let Some(mut end) = start.checked_add(obj_len as u64) else {
            break;
        };
        let mut j = i + 1;
        while j < addrs.len() {
            let a = addrs[j] as u64;
            if a < end {
                // overlapping / duplicate
                let Some(a_end) = a.checked_add(obj_len as u64) else {
                    break;
                };
                end = end.max(a_end);
                j += 1;
                continue;
            }
            if a - end > max_gap {
                break;
            }
            let Some(new_end) = a.checked_add(obj_len as u64) else {
                break;
            };
            let Some(block_len) = new_end.checked_sub(start) else {
                break;
            };
            if block_len > max_block as u64 {
                break;
            }
            end = new_end;
            j += 1;
        }
        if let Some(len) = end.checked_sub(start).and_then(|n| usize::try_from(n).ok()) {
            out.push((start, len));
        }
        i = j;
    }
    out
}

/// A `(start, bytes)` set of blocks plus their contents, with `slice(addr, len)` lookup.
struct Blocks {
    spans: Vec<(u64, Vec<u8>)>,
}

impl Blocks {
    fn get(&self, addr: u64, len: usize) -> Option<&[u8]> {
        // Blocks are built in ascending order, so a binary search on the start finds
        // the only candidate.
        let idx = match self.spans.binary_search_by(|(s, _)| s.cmp(&addr)) {
            Ok(i) => i,
            Err(0) => return None,
            Err(i) => i - 1,
        };
        let (s, data) = &self.spans[idx];
        let off = (addr - s) as usize;
        if off + len <= data.len() {
            Some(&data[off..off + len])
        } else {
            None
        }
    }
}

#[inline]
fn xor_i32(b: &[u8], off: usize, mask: u32) -> i32 {
    (le_u32(b, off) ^ mask) as i32
}

#[inline]
fn retail_hits_left(max_hp: i32, damage: i32) -> i32 {
    let left = max_hp.wrapping_sub(damage);
    if max_hp >= 0 && left >= 0 {
        left.min(max_hp)
    } else {
        0
    }
}

fn finish_metrics<M: Mem>(s: &mut Snapshot, r: &Reader<'_, M>) {
    s.reads = r.reads;
    s.bytes = r.bytes;
    s.short_reads = r.short_reads;
}

fn read_leader<M: Mem>(
    r: &mut Reader<'_, M>,
    base: u64,
    index: u8,
    include_economy: bool,
) -> Option<LiveLeader> {
    let b = r.block(base, LEADER_READ)?;
    let mut l = LiveLeader {
        index,
        leader_flags: le_u32(&b, OFF_LEADER_FLAGS),
        who: le_i32(&b, OFF_LEADER_WHO),
        tribe: le_i32(&b, OFF_LEADER_TRIBE),
        score: le_i32(&b, OFF_LEADER_SCORE),
        pop: le_i32(&b, OFF_LEADER_POP),
        pop_cap: le_i32(&b, OFF_LEADER_POP_CAP),
        city_num: le_i32(&b, OFF_LEADER_CITY_NUM),
        gather_stamp: le_u32(&b, OFF_LEADER_GATHER_STAMP),
        team_color: b[OFF_LEADER_TEAM_COLOR],
        free_peasants: le_i32(&b, OFF_LEADER_FREE_PEASANTS),
        gatherers: le_i32(&b, OFF_LEADER_GATHERERS),
        fishermen: le_i32(&b, OFF_LEADER_FISHERMEN),
        idle_fishermen: le_i32(&b, OFF_LEADER_IDLE_FISHERMEN),
        peasants: le_i32(&b, OFF_LEADER_PEASANTS),
        scholars: le_i32(&b, OFF_LEADER_SCHOLARS),
        active_wars: le_i32(&b, OFF_LEADER_ACTIVE_WARS),
        attacked: le_i32(&b, OFF_LEADER_ATTACKED),
        validity: LEADER_VALID_CORE,
        ..Default::default()
    };
    for k in 0..6 {
        l.gather_slots[k] = le_i32(&b, OFF_LEADER_GATHER_SLOTS + 4 * k);
        l.filled_gather_slots[k] = le_i32(&b, OFF_LEADER_FILLED_GATHER_SLOTS + 4 * k);
        l.queued_attack_class_cache[k] = le_i32(&b, OFF_LEADER_QUEUE_COUNTS + 4 * k);
    }
    if l.queued_attack_class_cache[3]
        == l.queued_attack_class_cache[0].wrapping_add(l.queued_attack_class_cache[1])
    {
        l.validity |= LEADER_VALID_QUEUED_ATTACK_CACHE;
    }
    let encrypted_p = le_u32(&b, OFF_LEADER_ENCRYPTED);
    if include_economy && encrypted_p != 0 {
        if let Some(e) = r.block(encrypted_p as u64, LEADER_ENCRYPTED_READ) {
            for k in 0..6 {
                l.stockpile[k] = xor_i32(&e, OFF_ENC_STOCKPILE + 4 * k, XOR_STOCKPILE);
                l.leftover[k] = xor_i32(&e, OFF_ENC_LEFTOVER + 4 * k, XOR_LEFTOVER);
                l.over_cap[k] = xor_i32(&e, OFF_ENC_OVER_CAP + 4 * k, XOR_OVER_CAP);
                l.gross[k] = xor_i32(&e, OFF_ENC_GROSS + 4 * k, XOR_GROSS);
                l.support[k] = xor_i32(&e, OFF_ENC_SUPPORT + 4 * k, XOR_SUPPORT);
                l.income[k] = xor_i32(&e, OFF_ENC_INCOME + 4 * k, XOR_INCOME);
                l.ai_planning_rate[k] = xor_i32(&e, OFF_ENC_RATE + 4 * k, XOR_AI_PLANNING_RATE);
                l.bonus[k] = xor_i32(&e, OFF_ENC_BONUS + 4 * k, XOR_BONUS);
            }
            for k in 0..7 {
                l.resource_cap[k] = xor_i32(&e, OFF_ENC_RESOURCE_CAP + 4 * k, XOR_RESOURCE_CAP);
            }
            l.ages = xor_i32(&e, OFF_ENC_AGES, XOR_AGES);
            l.epochs = xor_i32(&e, OFF_ENC_EPOCHS, XOR_EPOCHS);
            l.discovered = xor_i32(&e, OFF_ENC_DISCOVERED, XOR_DISCOVERED);
            for k in 0..4 {
                l.epoch[k] = xor_i32(&e, OFF_ENC_EPOCH + 4 * k, XOR_EPOCH);
            }
            l.validity |= LEADER_VALID_ECON;
        }
    }
    Some(l)
}

/// One attempt at the live entity graph. Callers should normally use [`snapshot`],
/// which retries a bounded number of times when this detects a torn frame.
fn snapshot_attempt<M: Mem>(mem: &M, delta: u64, vt: &VtMap) -> Snapshot {
    let mut r = Reader {
        mem,
        reads: 0,
        bytes: 0,
        short_reads: 0,
    };
    let mut s = Snapshot {
        ok: false,
        human_slot: -1,
        ..Default::default()
    };
    let ga = |va: u32| -> u64 { (va as u64).wrapping_add(delta) };

    // Read the simulation clock before anything it guards.
    let game_p = match r.u32(ga(VA_GAME)) {
        Some(p) if p != 0 => p,
        _ => {
            s.note = "GameAccess::game is null or unreadable".into();
            finish_metrics(&mut s, &r);
            return s;
        }
    };
    let Some(frame) = r.u32(game_p as u64 + OFF_GAME_FRAME as u64) else {
        s.note = "Game::frame is unreadable".into();
        finish_metrics(&mut s, &r);
        return s;
    };
    s.frame_start = frame;
    s.game_frame = frame;
    if let Some(seconds) = r.u32(game_p as u64 + OFF_GAME_SECONDS as u64) {
        s.game_seconds = seconds;
    }
    let game_semaphore_before = r.u32(game_p as u64 + OFF_GAME_SEMAPHORE_BYTES as u64);
    s.game_mode = game_mode_from_semaphore(game_semaphore_before);
    let turn_control_before = r.u32(ga(VA_TURN_CONTROL)).filter(|p| *p != 0);
    let pause_flags_before =
        turn_control_before.and_then(|p| r.u32(p as u64 + OFF_TURN_CONTROL_FLAGS as u64));
    s.paused = pause_flags_before.map(|flags| flags & TURN_CONTROL_PAUSED_FLAG != 0);
    s.valid_components |= COMPONENT_GAME;

    // World dimensions are optional for an economy-only observation.
    if let Some(wp) = r.u32(ga(VA_WORLD)) {
        if wp != 0 {
            if let Some(b) = r.block(wp as u64, 64) {
                let xs = le_i32(&b, OFF_WORLD_XS);
                let ys = le_i32(&b, OFF_WORLD_YS);
                if xs > 0 && ys > 0 && xs <= 4096 && ys <= 4096 {
                    s.world_xs = xs;
                    s.world_ys = ys;
                    s.tile_xs = le_i32(&b, OFF_WORLD_TILE_XS);
                    s.tile_ys = le_i32(&b, OFF_WORLD_TILE_YS);
                    s.valid_components |= COMPONENT_WORLD;
                }
            }
        }
    }

    // Leaders must be captured before pointer harvesting because retail gates owner
    // lists on `leader_flags & 1`. Slots 8 and 9 participate in unit processing even
    // though only the first eight are player records emitted by the feed.
    let mut owner_flags = [0u32; NUM_SLOTS];
    let mut owner_flags_valid = [false; NUM_SLOTS];
    let mut human_candidates = Vec::new();
    let mut core_leaders = 0usize;
    let mut econ_leaders = 0usize;
    for i in 0..LEADER_COUNT as usize {
        let Some(base) = ga(VA_LEADERS).checked_add((i as u64) * LEADER_STRIDE as u64) else {
            continue;
        };
        // Full entity diagnostics may include public player aggregates, but never
        // opponents' encrypted economy. The RoNtoy economy path below selects one
        // local human before asking for the encrypted block.
        let Some(l) = read_leader(&mut r, base, i as u8, false) else {
            continue;
        };
        let flags = l.leader_flags;
        owner_flags[i] = flags;
        owner_flags_valid[i] = true;
        if l.validity & LEADER_VALID_ECON != 0 {
            econ_leaders += 1;
        }
        if flags & 7 == 7 {
            human_candidates.push(i as i8);
        }
        core_leaders += 1;
        s.leaders.push(l);
    }
    for (i, valid) in owner_flags_valid
        .iter_mut()
        .enumerate()
        .skip(LEADER_COUNT as usize)
    {
        let base = ga(VA_LEADERS).wrapping_add((i as u64) * LEADER_STRIDE as u64);
        if let Some(flags) = r.u32(base + OFF_LEADER_FLAGS as u64) {
            owner_flags[i] = flags;
            *valid = true;
        }
    }
    if core_leaders > 0 {
        s.valid_components |= COMPONENT_LEADERS;
    }
    if econ_leaders > 0 {
        s.valid_components |= COMPONENT_ECON;
    }
    if human_candidates.len() == 1 {
        s.human_slot = human_candidates[0];
    }

    // Objects header and bounded band pointer harvest.
    let objects_p = match r.u32(ga(VA_OBJECTS)) {
        Some(p) if p != 0 => p,
        _ => {
            s.note = "GameAccess::objects is null or unreadable".into();
            finish_metrics(&mut s, &r);
            return s;
        }
    };
    let ob = match r.block(objects_p as u64, OBJECTS_READ) {
        Some(b) => b,
        None => {
            s.note = format!("could not read Objects at {objects_p:#x}");
            finish_metrics(&mut s, &r);
            return s;
        }
    };
    s.slots = vec![SlotStat::default(); NUM_SLOTS];
    let mut list_meta = [(0u32, 0i32, 0i32); NUM_SLOTS];
    for slot in 0..NUM_SLOTS {
        let arr = OFF_OBJECTS_LISTS + OBJECTS_ARRAY_STRIDE * slot;
        let length = le_i32(&ob, arr + OFF_ARR_LENGTH);
        let cap = le_i32(&ob, arr + OFF_ARR_SIZE);
        let list = le_u32(&ob, arr + OFF_ARR_LIST);
        s.slots[slot].array_length = length;
        s.slots[slot].array_size = cap;
        list_meta[slot] = (list, length, cap);
    }
    let mut want: Vec<(u32, u8, Band)> = Vec::with_capacity(4096);
    let mut harvest = |slot: usize, start: i32, end: i32, band: Band, r: &mut Reader<'_, M>| {
        let (list, length, cap) = list_meta[slot];
        if end <= start {
            return;
        }
        if list == 0
            || length < 0
            || cap < 0
            || length > cap
            || cap > MAX_ARRAY_SLOTS
            || end > length
            || end > cap
        {
            s.invalid_ranges += 1;
            return;
        }
        let Some(n_i32) = end.checked_sub(start) else {
            return;
        };
        let Ok(n) = usize::try_from(n_i32) else {
            return;
        };
        let Some(byte_len) = n.checked_mul(4) else {
            return;
        };
        let Some(addr) = (list as u64).checked_add((start as u64) * 4) else {
            return;
        };
        let Some(ptrs) = r.block(addr, byte_len) else {
            return;
        };
        s.slots[slot].band_span[band as usize] = n_i32;
        for k in 0..n {
            let p = le_u32(&ptrs, k * 4);
            if p != 0 {
                want.push((p, slot as u8, band));
            }
        }
    };
    // Unit owners rotate each frame in retail. Scan in that order, gated across all
    // ten slots. Build and wall processing is fixed 0..7 and leader-gated.
    for i in 0..NUM_SLOTS {
        let slot = (s.frame_start as usize + i) % NUM_SLOTS;
        if owner_flags_valid[slot] && owner_flags[slot] & 1 != 0 {
            harvest(
                slot,
                BAND_UNIT_START,
                le_i32(&ob, OFF_UNIT_MARK + 4 * slot),
                Band::Unit,
                &mut r,
            );
        }
    }
    for slot in 0..LEADER_COUNT as usize {
        if owner_flags_valid[slot] && owner_flags[slot] & 1 != 0 {
            harvest(
                slot,
                BAND_BUILD_START,
                le_i32(&ob, OFF_BUILD_MARK + 4 * slot),
                Band::Build,
                &mut r,
            );
            harvest(
                slot,
                BAND_WALL_START,
                le_i32(&ob, OFF_WALL_MARK + 4 * slot),
                Band::Wall,
                &mut r,
            );
        }
    }

    // Duplicate pointers indicate torn/corrupt lists. Keep one observation, surface
    // the count, and refuse to call the frame authoritative.
    let mut seen_ptrs = std::collections::HashSet::new();
    want.retain(|(p, _, _)| {
        if seen_ptrs.insert(*p) {
            true
        } else {
            s.duplicate_pointers += 1;
            false
        }
    });

    let mut addrs: Vec<u32> = want.iter().map(|(a, _, _)| *a).collect();
    addrs.sort_unstable();
    addrs.dedup();
    let plan = coalesce(&addrs, OBJ_READ, 4096, 1 << 20);
    let mut spans: Vec<(u64, Vec<u8>)> = Vec::with_capacity(plan.len());
    for (a, len) in plan {
        let mut buf = vec![0u8; len];
        let n = mem.read(a, &mut buf);
        r.reads += 1;
        r.bytes += n as u64;
        if n < len {
            r.short_reads += 1;
            buf.truncate(n);
        }
        if !buf.is_empty() {
            spans.push((a, buf));
        }
    }
    let blocks = Blocks { spans };

    let mut ptypes: Vec<u32> = Vec::new();
    let mut identities = std::collections::HashSet::new();
    for (addr, slot, band) in &want {
        let Some(o) = blocks.get(*addr as u64, OBJ_READ) else {
            continue;
        };
        let flags = o[OFF_OBJ_FLAGS];
        if flags & 1 == 0 {
            s.free_slots_skipped += 1;
            continue;
        }
        let ptype = le_u32(o, OFF_OBJ_PTYPE);
        if ptype != 0 {
            ptypes.push(ptype);
        }
        let vt_idx = vt.lookup(le_u32(o, OFF_OBJ_VPTR));
        let class = if vt_idx == NONE {
            None
        } else {
            Some(vt.names[vt.entries[vt_idx as usize].name_idx as usize].as_str())
        };
        let kind = kind_of(class, *band);
        let damage = le_i32(o, OFF_OBJ_DAMAGE);
        let mut validity = OBJECT_VALID_POSITION;
        let mut max_hp = match band {
            Band::Build | Band::Wall => le_i32(o, OFF_BUILD_CONSTRUCT_HITS),
            Band::Unit => le_i32(o, OFF_OBJ_MYHITS),
        };
        // BuildData::hits(0) further scales two razing queue types using train_time.
        // Calling target code would violate this reader's safety boundary, so make
        // that rare value explicitly unavailable instead of publishing base myhits.
        let mut hp_exact = true;
        if *band == Band::Build
            && flags & 4 != 0
            && o[OFF_BUILD_QUEUED] != 0
            && le_i32(o, OFF_BUILD_QUEUE_NUM) > 0
        {
            let q = le_u32(o, OFF_BUILD_QUEUE_LIST);
            if q != 0 {
                if let Some(qb) = r.block(q as u64, 6) {
                    let queued_type = le_i16(&qb, 4);
                    if queued_type == 0x29A || queued_type == 0x286 {
                        hp_exact = false;
                        max_hp = -1;
                    }
                } else {
                    hp_exact = false;
                    max_hp = -1;
                }
            }
        }
        let hp = if hp_exact {
            retail_hits_left(max_hp, damage)
        } else {
            -1
        };
        if hp_exact {
            validity |= OBJECT_VALID_HP;
        }
        if kind == Kind::Unit || kind == Kind::Animal || kind == Kind::Caravan {
            validity |= OBJECT_VALID_IDLE_CACHE;
        }
        let object = LiveObject {
            addr: *addr,
            uid: le_u16(o, OFF_OBJ_UID),
            o: le_i16(o, OFF_OBJ_O),
            type_index: u16::MAX,
            ptype,
            x: unmask_coord(le_u32(o, OFF_OBJ_X)),
            y: unmask_coord(le_u32(o, OFF_OBJ_Y)),
            z: unmask_coord(le_u32(o, OFF_OBJ_Z)),
            angle: if kind == Kind::Unit || kind == Kind::Animal {
                le_i32(o, OFF_UNIT_ANGLE)
            } else {
                0
            },
            hp,
            max_hp,
            who: o[OFF_OBJ_WHO],
            slot: *slot,
            band: *band,
            kind,
            flags,
            idle_cache: o[OFF_UNIT_IDLE_CACHE] != 0,
            validity,
        };
        if !identities.insert(object.identity()) {
            s.duplicate_identities += 1;
            continue;
        }
        s.slots[*slot as usize].band_live[*band as usize] += 1;
        s.objects.push(object);
    }

    ptypes.sort_unstable();
    ptypes.dedup();
    let mut tmap: std::collections::HashMap<u32, u16> = std::collections::HashMap::new();
    for (a, len) in coalesce(&ptypes, 8, 2048, 1 << 16) {
        let mut buf = vec![0u8; len];
        let n = mem.read(a, &mut buf);
        r.reads += 1;
        r.bytes += n as u64;
        if n < len {
            r.short_reads += 1;
        }
        for &p in &ptypes {
            let Some(delta) = (p as u64).checked_sub(a) else {
                continue;
            };
            let Ok(off) = usize::try_from(delta) else {
                continue;
            };
            let Some(end) = off.checked_add(8) else {
                continue;
            };
            if end <= n {
                let ti = le_i32(&buf, off + OFF_TYPE_INDEX);
                if (0..=0xFFFE).contains(&ti) {
                    tmap.insert(p, ti as u16);
                }
            }
        }
    }
    for object in &mut s.objects {
        if let Some(t) = tmap.get(&object.ptype) {
            object.type_index = *t;
            object.validity |= OBJECT_VALID_TYPE;
        }
    }
    s.ptypes = ptypes;
    s.objects
        .sort_by_key(|o| (o.slot, o.band as u8, o.o, o.uid, o.addr));
    s.valid_components |= COMPONENT_OBJECTS;

    // Read both the root pointer and frame at the end. A frame equality without the
    // same Game object is not coherent across loading/menu transitions.
    let end_game_p = r.u32(ga(VA_GAME));
    let game_semaphore_after = end_game_p
        .filter(|p| *p == game_p)
        .and_then(|p| r.u32(p as u64 + OFF_GAME_SEMAPHORE_BYTES as u64));
    let mode_stable = match (game_semaphore_before, game_semaphore_after) {
        (Some(before), Some(after)) => before == after,
        (Some(_), None) => false,
        (None, _) => true,
    };
    let turn_control_after = r.u32(ga(VA_TURN_CONTROL)).filter(|p| *p != 0);
    let pause_flags_after =
        turn_control_after.and_then(|p| r.u32(p as u64 + OFF_TURN_CONTROL_FLAGS as u64));
    let pause_stable = match (turn_control_before, pause_flags_before) {
        (Some(before_ptr), Some(before_flags)) => {
            turn_control_after == Some(before_ptr)
                && pause_flags_after.map(|flags| flags & TURN_CONTROL_PAUSED_FLAG)
                    == Some(before_flags & TURN_CONTROL_PAUSED_FLAG)
        }
        (Some(_), None) => turn_control_after == turn_control_before,
        (None, _) => true,
    };
    let end_frame = end_game_p
        .filter(|p| *p == game_p)
        .and_then(|p| r.u32(p as u64 + OFF_GAME_FRAME as u64));
    match end_frame {
        Some(end) => {
            s.frame_end = end;
            s.game_frame = end;
            if end == s.frame_start && mode_stable && pause_stable {
                s.coherence = Coherence::Coherent;
            } else {
                s.coherence = Coherence::Torn;
                s.note = if end != s.frame_start {
                    format!("frame advanced {} -> {} during capture", s.frame_start, end)
                } else if !mode_stable {
                    "Game::semaphore mode evidence changed during capture".into()
                } else {
                    "TurnControl pause evidence changed during capture".into()
                };
            }
        }
        None => {
            s.coherence = Coherence::Unavailable;
            s.note = "Game pointer/frame changed or became unreadable during capture".into();
        }
    }
    s.ok = s.coherence == Coherence::Coherent
        && s.duplicate_pointers == 0
        && s.duplicate_identities == 0
        && s.invalid_ranges == 0
        && s.valid_components & (COMPONENT_GAME | COMPONENT_OBJECTS)
            == (COMPONENT_GAME | COMPONENT_OBJECTS);
    if !s.ok && s.note.is_empty() {
        s.note = format!(
            "non-authoritative snapshot: {} duplicate pointers, {} duplicate identities, {} invalid ranges",
            s.duplicate_pointers, s.duplicate_identities, s.invalid_ranges
        );
    }
    finish_metrics(&mut s, &r);
    s
}

/// Read a coherent live entity graph, retrying at most `attempts` times. This never
/// suspends or writes the target; retries are independent `ReadProcessMemory` passes.
pub fn snapshot_with_retries<M: Mem>(mem: &M, delta: u64, vt: &VtMap, attempts: u8) -> Snapshot {
    let attempts = attempts.max(1);
    let mut total_reads = 0u32;
    let mut total_bytes = 0u64;
    let mut total_short = 0u32;
    let mut last = None;
    for attempt in 0..attempts {
        let mut s = snapshot_attempt(mem, delta, vt);
        total_reads = total_reads.saturating_add(s.reads);
        total_bytes = total_bytes.saturating_add(s.bytes);
        total_short = total_short.saturating_add(s.short_reads);
        s.reads = total_reads;
        s.bytes = total_bytes;
        s.short_reads = total_short;
        s.retry_count = attempt;
        if s.ok {
            return s;
        }
        let retryable = s.coherence == Coherence::Torn
            || s.duplicate_pointers != 0
            || s.duplicate_identities != 0
            || s.invalid_ranges != 0;
        last = Some(s);
        if !retryable {
            break;
        }
    }
    last.unwrap_or_else(|| Snapshot {
        note: "snapshot attempts exhausted".into(),
        human_slot: -1,
        ..Default::default()
    })
}

/// Read the whole live entity graph with the conservative default retry bound.
pub fn snapshot<M: Mem>(mem: &M, delta: u64, vt: &VtMap) -> Snapshot {
    snapshot_with_retries(mem, delta, vt, DEFAULT_SNAPSHOT_ATTEMPTS)
}

fn economy_snapshot_attempt<M: Mem>(mem: &M, delta: u64) -> Snapshot {
    let mut r = Reader {
        mem,
        reads: 0,
        bytes: 0,
        short_reads: 0,
    };
    let mut s = Snapshot {
        ok: false,
        human_slot: -1,
        ..Default::default()
    };
    let ga = |va: u32| -> u64 { (va as u64).wrapping_add(delta) };
    let game_p = match r.u32(ga(VA_GAME)) {
        Some(p) if p != 0 => p,
        _ => {
            s.note = "GameAccess::game is null or unreadable".into();
            finish_metrics(&mut s, &r);
            return s;
        }
    };
    let Some(frame) = r.u32(game_p as u64 + OFF_GAME_FRAME as u64) else {
        s.note = "Game::frame is unreadable".into();
        finish_metrics(&mut s, &r);
        return s;
    };
    s.frame_start = frame;
    s.game_frame = frame;
    if let Some(seconds) = r.u32(game_p as u64 + OFF_GAME_SECONDS as u64) {
        s.game_seconds = seconds;
    }
    let game_semaphore_before = r.u32(game_p as u64 + OFF_GAME_SEMAPHORE_BYTES as u64);
    s.game_mode = game_mode_from_semaphore(game_semaphore_before);
    let turn_control_before = r.u32(ga(VA_TURN_CONTROL)).filter(|p| *p != 0);
    let pause_flags_before =
        turn_control_before.and_then(|p| r.u32(p as u64 + OFF_TURN_CONTROL_FLAGS as u64));
    s.paused = pause_flags_before.map(|flags| flags & TURN_CONTROL_PAUSED_FLAG != 0);
    s.valid_components |= COMPONENT_GAME;

    // Locate the local player from the console bit. Read only the flags first so
    // the economy fast path pays for one full LeaderData record, never all eight.
    let mut candidates = Vec::new();
    for i in 0..LEADER_COUNT as usize {
        let base = ga(VA_LEADERS).wrapping_add(i as u64 * LEADER_STRIDE as u64);
        if let Some(flags) = r.u32(base + OFF_LEADER_FLAGS as u64) {
            if flags & 7 == 7 {
                candidates.push((i as u8, flags));
            }
        }
    }
    if candidates.len() != 1 {
        s.note = format!(
            "expected one active console leader, found {}",
            candidates.len()
        );
        finish_metrics(&mut s, &r);
        return s;
    }
    let (index, selected_flags) = candidates[0];
    let base = ga(VA_LEADERS).wrapping_add(index as u64 * LEADER_STRIDE as u64);
    let encrypted_before = r.u32(base + OFF_LEADER_ENCRYPTED as u64);
    let Some(leader) = read_leader(&mut r, base, index, true) else {
        s.note = format!("human LeaderData slot {index} is unreadable");
        finish_metrics(&mut s, &r);
        return s;
    };
    if leader.leader_flags != selected_flags || leader.who != index as i32 {
        s.note = format!(
            "console leader slot {index} changed identity (flags={:#x}, who={})",
            leader.leader_flags, leader.who
        );
        finish_metrics(&mut s, &r);
        return s;
    }
    if leader.validity & LEADER_VALID_ECON == 0 {
        s.note = format!("human LeaderDataEncrypt slot {index} is null or unreadable");
        s.leaders.push(leader);
        s.human_slot = index as i8;
        s.valid_components |= COMPONENT_LEADERS;
        finish_metrics(&mut s, &r);
        return s;
    }
    s.human_slot = index as i8;
    s.leaders.push(leader);
    s.valid_components |= COMPONENT_LEADERS | COMPONENT_ECON;

    let end_flags = r.u32(base + OFF_LEADER_FLAGS as u64);
    let end_who = r.u32(base + OFF_LEADER_WHO as u64).map(|who| who as i32);
    let encrypted_after = r.u32(base + OFF_LEADER_ENCRYPTED as u64);
    let roots_stable = end_flags == Some(selected_flags)
        && end_who == Some(index as i32)
        && encrypted_before.is_some()
        && encrypted_before == encrypted_after;
    let end_game_p = r.u32(ga(VA_GAME));
    let game_semaphore_after = end_game_p
        .filter(|p| *p == game_p)
        .and_then(|p| r.u32(p as u64 + OFF_GAME_SEMAPHORE_BYTES as u64));
    let mode_stable = match (game_semaphore_before, game_semaphore_after) {
        (Some(before), Some(after)) => before == after,
        (Some(_), None) => false,
        (None, _) => true,
    };
    let turn_control_after = r.u32(ga(VA_TURN_CONTROL)).filter(|p| *p != 0);
    let pause_flags_after =
        turn_control_after.and_then(|p| r.u32(p as u64 + OFF_TURN_CONTROL_FLAGS as u64));
    let pause_stable = match (turn_control_before, pause_flags_before) {
        (Some(before_ptr), Some(before_flags)) => {
            turn_control_after == Some(before_ptr)
                && pause_flags_after.map(|flags| flags & TURN_CONTROL_PAUSED_FLAG)
                    == Some(before_flags & TURN_CONTROL_PAUSED_FLAG)
        }
        (Some(_), None) => turn_control_after == turn_control_before,
        (None, _) => true,
    };
    let end_frame = end_game_p
        .filter(|p| *p == game_p)
        .and_then(|p| r.u32(p as u64 + OFF_GAME_FRAME as u64));
    match end_frame {
        Some(end) => {
            s.frame_end = end;
            s.game_frame = end;
            if end == s.frame_start && roots_stable && mode_stable && pause_stable {
                s.coherence = Coherence::Coherent;
                s.ok = true;
            } else {
                s.coherence = Coherence::Torn;
                s.note = if end != s.frame_start {
                    format!("frame advanced {} -> {} during capture", s.frame_start, end)
                } else if !roots_stable {
                    "human leader flags/economy root changed during capture".into()
                } else if !mode_stable {
                    "Game::semaphore mode evidence changed during capture".into()
                } else {
                    "TurnControl pause evidence changed during capture".into()
                };
            }
        }
        None => {
            s.coherence = Coherence::Unavailable;
            s.note = "Game pointer/frame changed or became unreadable during capture".into();
        }
    }
    finish_metrics(&mut s, &r);
    s
}

/// Targeted RoNtoy economy capture. Reads eight flag dwords, one human
/// `LeaderData`, and its 248-byte encrypted economy block; World and Objects are
/// intentionally not touched. Default callers should sample this at 1–2 Hz.
pub fn economy_snapshot_with_retries<M: Mem>(mem: &M, delta: u64, attempts: u8) -> Snapshot {
    let attempts = attempts.max(1);
    let mut total_reads = 0u32;
    let mut total_bytes = 0u64;
    let mut total_short = 0u32;
    let mut last = None;
    for attempt in 0..attempts {
        let mut s = economy_snapshot_attempt(mem, delta);
        total_reads = total_reads.saturating_add(s.reads);
        total_bytes = total_bytes.saturating_add(s.bytes);
        total_short = total_short.saturating_add(s.short_reads);
        s.reads = total_reads;
        s.bytes = total_bytes;
        s.short_reads = total_short;
        s.retry_count = attempt;
        if s.ok {
            return s;
        }
        let retryable = s.coherence == Coherence::Torn;
        last = Some(s);
        if !retryable {
            break;
        }
    }
    last.unwrap_or_else(|| Snapshot {
        note: "economy snapshot attempts exhausted".into(),
        human_slot: -1,
        ..Default::default()
    })
}

pub fn economy_snapshot<M: Mem>(mem: &M, delta: u64) -> Snapshot {
    economy_snapshot_with_retries(mem, delta, DEFAULT_SNAPSHOT_ATTEMPTS)
}

/// The territory + terrain grid: one `WData` per WCoord cell, `xs*ys` of them at
/// `World+308`, indexed `y*xs + x` [measured, `WorldData::get_who` `0x006B4700`,
/// `(y * xs + x) * 0x1c + 0xf`].
pub struct MapGrid {
    pub xs: i32,
    pub ys: i32,
    pub tile_xs: i32,
    pub tile_ys: i32,
    /// `WData::land` per cell.
    pub land: Vec<u8>,
    /// `WData::who` per cell, biased by +1 so 0 means "unowned" (`who == -1`).
    pub who: Vec<u8>,
    pub bytes: u64,
}

pub fn read_map<M: Mem>(mem: &M, delta: u64) -> Option<MapGrid> {
    let mut r = Reader {
        mem,
        reads: 0,
        bytes: 0,
        short_reads: 0,
    };
    let wp = r.u32((VA_WORLD as u64).wrapping_add(delta))?;
    if wp == 0 {
        return None;
    }
    let hdr = r.block(wp as u64, 320)?;
    let xs = le_i32(&hdr, OFF_WORLD_XS);
    let ys = le_i32(&hdr, OFF_WORLD_YS);
    if xs <= 0 || ys <= 0 || xs > 4096 || ys > 4096 {
        return None;
    }
    let wdata = le_u32(&hdr, OFF_WORLD_WDATA);
    if wdata == 0 {
        return None;
    }
    let cells = (xs as usize).checked_mul(ys as usize)?;
    if cells > MAX_MAP_CELLS {
        return None;
    }
    let total = cells.checked_mul(WDATA_STRIDE)?;
    let mut raw = vec![0u8; total];
    // Never turn an unreadable tail into invented owner-zero territory.
    let n = mem.read(wdata as u64, &mut raw);
    if n != total {
        return None;
    }
    let mut land = vec![0u8; cells];
    let mut who = vec![0u8; cells];
    for i in 0..cells {
        let o = i * WDATA_STRIDE;
        land[i] = raw[o + OFF_WDATA_LAND];
        let w = raw[o + OFF_WDATA_WHO] as i8;
        who[i] = if w < 0 {
            0
        } else {
            (w as u8).saturating_add(1)
        };
    }
    Some(MapGrid {
        xs,
        ys,
        tile_xs: le_i32(&hdr, OFF_WORLD_TILE_XS),
        tile_ys: le_i32(&hdr, OFF_WORLD_TILE_YS),
        land,
        who,
        bytes: r.bytes + n as u64,
    })
}

/// Read a `String`'s `wchar_t*` payload. `TypeData::name` is a `String` whose first
/// dword is the character pointer [measured, `docs/derivation/live-tables.md` §UnitType].
pub fn read_wstring<M: Mem>(mem: &M, string_addr: u64, max: usize) -> Option<String> {
    let mut p = [0u8; 4];
    if mem.read(string_addr, &mut p) < 4 {
        return None;
    }
    let cp = le_u32(&p, 0);
    if cp == 0 {
        return None;
    }
    let mut buf = vec![0u8; max * 2];
    let n = mem.read(cp as u64, &mut buf);
    let mut out = String::new();
    let mut i = 0;
    while i + 1 < n {
        let c = le_u16(&buf, i);
        if c == 0 {
            break;
        }
        if !(0x20..0x7f).contains(&c) {
            return if out.is_empty() { None } else { Some(out) };
        }
        out.push(c as u8 as char);
        i += 2;
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// Names and a few static stats for one `ObjectType`, read through an object's `ptype`.
#[derive(Clone, Debug, Default)]
pub struct TypeInfo {
    pub type_index: u16,
    pub name: String,
    pub display: String,
    pub hits: i32,
    pub armor: i32,
    pub attack_x10: i32,
    pub x_size: i32,
    pub y_size: i32,
}

pub fn read_type<M: Mem>(mem: &M, ptype: u32) -> Option<TypeInfo> {
    let mut b = vec![0u8; 0x240];
    if mem.read(ptype as u64, &mut b) < 0x240 {
        return None;
    }
    let ti = le_i32(&b, OFF_TYPE_INDEX);
    if !(0..=0xFFFE).contains(&ti) {
        return None;
    }
    Some(TypeInfo {
        type_index: ti as u16,
        name: read_wstring(mem, ptype as u64 + OFF_TYPE_NAME as u64, 64).unwrap_or_default(),
        display: read_wstring(mem, ptype as u64 + OFF_TYPE_DISPLAY as u64, 64).unwrap_or_default(),
        hits: le_i32(&b, OFF_TYPE_HITS),
        armor: le_i32(&b, OFF_TYPE_ARMOR),
        attack_x10: le_i32(&b, OFF_TYPE_ATTACK),
        x_size: le_i32(&b, OFF_TYPE_XSIZE),
        y_size: le_i32(&b, OFF_TYPE_YSIZE),
    })
}

// ---------------------------------------------------------------------------
// Wire format
// ---------------------------------------------------------------------------

pub const WIRE_MAGIC: u32 = 0x4C_4E_4F_44; // "DONL" little-endian
/// Transitional binary snapshot format. The RoNtoy protocol crate owns the eventual
/// public transport envelope; this version is retained for local capture fixtures.
pub const WIRE_VERSION: u16 = 2;
pub const WIRE_HEADER_LEN: usize = 68;
pub const WIRE_OBJ_STRIDE: usize = 48;
pub const WIRE_LEADER_STRIDE: usize = 392;

fn put_u16(v: &mut Vec<u8>, x: u16) {
    v.extend_from_slice(&x.to_le_bytes());
}
fn put_u32(v: &mut Vec<u8>, x: u32) {
    v.extend_from_slice(&x.to_le_bytes());
}
fn put_i32(v: &mut Vec<u8>, x: i32) {
    v.extend_from_slice(&x.to_le_bytes());
}

/// Encode a snapshot as one binary frame. Layout is documented in
/// `web/live/README.md` and decoded by `web/live/app.js`.
pub fn encode_frame(s: &Snapshot, scan_us: u32, pid: u32, image_base: u32) -> Vec<u8> {
    let mut v = Vec::with_capacity(WIRE_HEADER_LEN + s.objects.len() * WIRE_OBJ_STRIDE + 512);
    put_u32(&mut v, WIRE_MAGIC);
    put_u16(&mut v, WIRE_VERSION);
    put_u16(&mut v, WIRE_HEADER_LEN as u16);
    put_u32(&mut v, s.game_frame);
    put_u32(&mut v, s.game_seconds);
    put_u16(&mut v, s.world_xs.clamp(0, 65535) as u16);
    put_u16(&mut v, s.world_ys.clamp(0, 65535) as u16);
    put_u16(&mut v, s.tile_xs.clamp(0, 65535) as u16);
    put_u16(&mut v, s.tile_ys.clamp(0, 65535) as u16);
    put_u32(&mut v, s.objects.len() as u32);
    put_u32(&mut v, WIRE_OBJ_STRIDE as u32);
    put_u32(&mut v, s.leaders.len() as u32);
    put_u32(&mut v, WIRE_LEADER_STRIDE as u32);
    put_u32(&mut v, scan_us);
    put_u32(&mut v, s.reads);
    put_u32(&mut v, s.bytes as u32);
    put_u32(&mut v, s.free_slots_skipped);
    put_u32(&mut v, pid);
    put_u32(&mut v, image_base);
    put_u32(&mut v, if s.ok { 1 } else { 0 });
    debug_assert_eq!(v.len(), WIRE_HEADER_LEN);
    while v.len() < WIRE_HEADER_LEN {
        v.push(0);
    }

    for o in &s.objects {
        put_u32(&mut v, o.addr);
        put_u16(&mut v, o.uid);
        put_u16(&mut v, o.o as u16);
        put_u16(&mut v, o.type_index);
        put_u16(&mut v, o.validity);
        put_u32(&mut v, o.ptype);
        put_i32(&mut v, o.x);
        put_i32(&mut v, o.y);
        put_i32(&mut v, o.z);
        put_i32(&mut v, o.angle);
        put_i32(&mut v, o.hp);
        put_i32(&mut v, o.max_hp);
        v.push(o.who);
        v.push(o.slot);
        v.push(o.band as u8);
        v.push(o.kind as u8);
        v.push(o.flags);
        v.push(u8::from(o.idle_cache));
        put_u16(&mut v, 0);
    }
    for l in &s.leaders {
        v.push(l.index);
        v.push(l.team_color);
        put_u16(&mut v, 0);
        put_u32(&mut v, l.validity);
        put_u32(&mut v, l.leader_flags);
        put_i32(&mut v, l.who);
        put_i32(&mut v, l.tribe);
        put_i32(&mut v, l.score);
        put_i32(&mut v, l.pop);
        put_i32(&mut v, l.pop_cap);
        put_i32(&mut v, l.city_num);
        put_u32(&mut v, l.gather_stamp);
        put_i32(&mut v, l.free_peasants);
        put_i32(&mut v, l.gatherers);
        put_i32(&mut v, l.fishermen);
        put_i32(&mut v, l.idle_fishermen);
        put_i32(&mut v, l.peasants);
        put_i32(&mut v, l.scholars);
        put_i32(&mut v, l.active_wars);
        put_i32(&mut v, l.attacked);
        for values in [
            &l.gather_slots[..],
            &l.filled_gather_slots[..],
            &l.queued_attack_class_cache[..],
            &l.stockpile[..],
            &l.leftover[..],
            &l.resource_cap[..],
            &l.over_cap[..],
            &l.gross[..],
            &l.support[..],
            &l.income[..],
            &l.ai_planning_rate[..],
            &l.bonus[..],
        ] {
            for &value in values {
                put_i32(&mut v, value);
            }
        }
        put_i32(&mut v, l.ages);
        put_i32(&mut v, l.epochs);
        put_i32(&mut v, l.discovered);
        for &value in &l.epoch {
            put_i32(&mut v, value);
        }
    }
    v
}

/// Metadata around one NDJSON observation. This is a deliberately small integration
/// format for the first RoNtoy loop; the dedicated protocol lane owns the eventual
/// binary transport contract.
#[derive(Clone, Copy, Debug, Default)]
pub struct ObservationMeta {
    pub session_id: u64,
    pub capture_seq: u64,
    pub pid: u32,
    pub image_base: u32,
    pub process_started_100ns: u64,
    pub module_size: u64,
    pub module_sha256: [u8; 32],
    pub captured_unix_ms: u64,
    pub monotonic_us: u64,
    pub capture_us: u32,
}

pub fn hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

/// Exact allow-list gate for every offset in this module. PE shape alone is not
/// sufficient: a different build can retain its entry RVA and image size while
/// changing data layouts.
pub fn supported_source_identity(file_size: u64, digest: &[u8; 32]) -> bool {
    file_size == SOURCE_FILE_SIZE && digest == &SOURCE_SHA256
}

/// Dependency-free SHA-256 used to fingerprint the on-disk target image once at
/// attachment. Keeping it here makes the implementation testable off Windows.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h = [
        0x6a09e667u32,
        0xbb67ae85,
        0x3c6ef372,
        0xa54ff53a,
        0x510e527f,
        0x9b05688c,
        0x1f83d9ab,
        0x5be0cd19,
    ];
    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut padded = Vec::with_capacity((data.len() + 72) & !63);
    padded.extend_from_slice(data);
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in padded.chunks_exact(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            let o = i * 4;
            w[i] = u32::from_be_bytes([chunk[o], chunk[o + 1], chunk[o + 2], chunk[o + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (state, value) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *state = state.wrapping_add(value);
        }
    }
    let mut out = [0u8; 32];
    for (i, value) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&value.to_be_bytes());
    }
    out
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn push_i32_array(out: &mut String, values: &[i32]) {
    out.push('[');
    for (i, value) in values.iter().enumerate() {
        if i != 0 {
            out.push(',');
        }
        out.push_str(&value.to_string());
    }
    out.push(']');
}

/// Encode one economy-only observation as a single JSON object (without a trailing
/// newline). Resource arrays are always retail order: food, timber, wealth,
/// knowledge, metal, oil. An invalid capture carries health metadata and `leader:null`.
pub fn encode_economy_ndjson(s: &Snapshot, meta: ObservationMeta) -> String {
    let coherence = match s.coherence {
        Coherence::Unavailable => "unavailable",
        Coherence::Coherent => "coherent",
        Coherence::Torn => "torn",
    };
    let game_mode = match s.game_mode {
        GameMode::Unknown => "unknown",
        GameMode::SinglePlayer => "single_player",
        GameMode::Multiplayer => "multiplayer",
    };
    let paused = match s.paused {
        Some(true) => "true",
        Some(false) => "false",
        None => "null",
    };
    let mut out = String::with_capacity(2048);
    out.push_str("{\"schema\":\"rontoy.observation\",\"version\":{\"major\":1,\"minor\":0}");
    out.push_str(&format!(
        ",\"session_id\":\"{:016x}\",\"capture_seq\":{},\"source\":{{\"pid\":{},\"process_started_100ns\":\"{}\",\"image_base\":{},\"entry_rva\":{},\"image_size\":{},\"module_size\":{},\"module_sha256\":\"{}\"}}",
        meta.session_id,
        meta.capture_seq,
        meta.pid,
        meta.process_started_100ns,
        meta.image_base,
        SOURCE_ENTRY_RVA,
        SOURCE_IMAGE_SIZE,
        meta.module_size,
        hex_bytes(&meta.module_sha256)
    ));
    out.push_str(&format!(
        ",\"game\":{{\"mode\":\"{}\",\"paused\":{},\"seconds\":{}}},\"capture\":{{\"captured_unix_ms\":{},\"monotonic_us\":{},\"duration_us\":{},\"frame_start\":{},\"frame_end\":{},\"coherence\":\"{}\",\"retry_count\":{},\"valid_components\":{},\"reads\":{},\"short_reads\":{},\"bytes\":{}}}",
        game_mode,
        paused,
        s.game_seconds,
        meta.captured_unix_ms,
        meta.monotonic_us,
        meta.capture_us,
        s.frame_start,
        s.frame_end,
        coherence,
        s.retry_count,
        s.valid_components,
        s.reads,
        s.short_reads,
        s.bytes
    ));
    out.push_str(
        ",\"resource_order\":[\"food\",\"timber\",\"wealth\",\"knowledge\",\"metal\",\"oil\"]",
    );
    out.push_str(&format!(
        ",\"ok\":{},\"note\":\"{}\",\"human_slot\":{}",
        s.ok,
        json_escape(&s.note),
        s.human_slot
    ));
    if !s.ok || s.leaders.len() != 1 || s.leaders[0].validity & LEADER_VALID_ECON == 0 {
        out.push_str(",\"leader\":null}");
        return out;
    }
    let l = &s.leaders[0];
    out.push_str(&format!(
        ",\"leader\":{{\"slot\":{},\"who\":{},\"tribe\":{},\"team_color\":{},\"flags\":{},\"validity\":{},\"score\":{},\"population\":{{\"current\":{},\"cap\":{}}},\"city_num\":{},\"gather_stamp\":{},\"gather_cache_age_frames\":{},\"free_peasants\":{},\"gatherers\":{},\"fishermen\":{},\"idle_fishermen\":{},\"peasants\":{},\"scholars\":{},\"active_wars\":{},\"attacked\":{}",
        l.index,
        l.who,
        l.tribe,
        l.team_color,
        l.leader_flags,
        l.validity,
        l.score,
        l.pop,
        l.pop_cap,
        l.city_num,
        l.gather_stamp,
        s.frame_end.wrapping_sub(l.gather_stamp),
        l.free_peasants,
        l.gatherers,
        l.fishermen,
        l.idle_fishermen,
        l.peasants,
        l.scholars,
        l.active_wars,
        l.attacked
    ));
    for (name, values) in [
        ("gather_slots", &l.gather_slots[..]),
        ("filled_gather_slots", &l.filled_gather_slots[..]),
        (
            "queued_attack_class_cache",
            &l.queued_attack_class_cache[..],
        ),
        ("stockpile", &l.stockpile[..]),
        ("leftover", &l.leftover[..]),
        ("resource_cap_x16", &l.resource_cap[..]),
        ("over_cap", &l.over_cap[..]),
        ("gross_x16", &l.gross[..]),
        ("support_x16", &l.support[..]),
        ("income_x16", &l.income[..]),
        ("ai_planning_rate", &l.ai_planning_rate[..]),
        ("bonus", &l.bonus[..]),
        ("epoch", &l.epoch[..]),
    ] {
        out.push_str(",\"");
        out.push_str(name);
        out.push_str("\":");
        push_i32_array(&mut out, values);
    }
    out.push_str(&format!(
        ",\"age\":{},\"epochs\":{},\"discovered\":{}}}}}",
        l.ages, l.epochs, l.discovered
    ));
    out
}

// ---------------------------------------------------------------------------
// Tests — run on the Mac against a synthetic process image
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// A sparse byte map standing in for a target address space.
    #[derive(Default)]
    struct FakeMem {
        pages: std::collections::HashMap<u64, Vec<u8>>, // 4 KiB pages
    }
    impl FakeMem {
        fn put(&mut self, addr: u64, bytes: &[u8]) {
            for (i, b) in bytes.iter().enumerate() {
                let a = addr + i as u64;
                let page = self
                    .pages
                    .entry(a & !0xfff)
                    .or_insert_with(|| vec![0u8; 4096]);
                page[(a & 0xfff) as usize] = *b;
            }
        }
        fn put_u32(&mut self, addr: u64, v: u32) {
            self.put(addr, &v.to_le_bytes());
        }
        fn alloc(&mut self, addr: u64, len: usize) {
            if len != 0 {
                self.put(addr, &vec![0u8; len]);
            }
        }
    }
    impl Mem for FakeMem {
        fn read(&self, addr: u64, buf: &mut [u8]) -> usize {
            for i in 0..buf.len() {
                let a = addr + i as u64;
                match self.pages.get(&(a & !0xfff)) {
                    Some(p) => buf[i] = p[(a & 0xfff) as usize],
                    None => return i,
                }
            }
            buf.len()
        }
    }

    const DELTA: u64 = 0x0096_0000; // the delta measured against the live game

    /// Builds a target image with: a Game, a World, an Objects with one owner slot
    /// holding two units (one live, one free) and one building, and two ObjectTypes.
    fn build() -> FakeMem {
        let mut m = FakeMem::default();
        let game = 0x0500_0000u64;
        let turn_control = 0x0500_1000u64;
        let world = 0x0501_0000u64;
        let objects = 0x0502_0000u64;
        let list = 0x0503_0000u64;
        let unit_live = 0x0510_0000u64;
        let unit_free = 0x0510_1000u64;
        let build = 0x0511_0000u64;
        let utype = 0x0520_0000u64;
        let btype = 0x0520_1000u64;
        let encrypted = 0x0521_0000u64;

        m.alloc(game, OFF_GAME_SEMAPHORE_BYTES as usize + 4);
        m.alloc(turn_control, OFF_TURN_CONTROL_FLAGS as usize + 4);
        m.alloc(world, 320);
        m.alloc(objects, OBJECTS_READ);
        m.alloc(list, 4000 * 4);
        m.alloc(unit_live, OBJ_READ);
        m.alloc(unit_free, OBJ_READ);
        m.alloc(build, OBJ_READ);
        m.alloc(utype, 8);
        m.alloc(btype, 8);
        m.alloc(encrypted, LEADER_ENCRYPTED_READ);
        for k in 0..6 {
            m.put_u32(
                encrypted + (OFF_ENC_STOCKPILE + 4 * k) as u64,
                XOR_STOCKPILE,
            );
            m.put_u32(encrypted + (OFF_ENC_LEFTOVER + 4 * k) as u64, XOR_LEFTOVER);
            m.put_u32(encrypted + (OFF_ENC_OVER_CAP + 4 * k) as u64, XOR_OVER_CAP);
            m.put_u32(encrypted + (OFF_ENC_GROSS + 4 * k) as u64, XOR_GROSS);
            m.put_u32(encrypted + (OFF_ENC_SUPPORT + 4 * k) as u64, XOR_SUPPORT);
            m.put_u32(encrypted + (OFF_ENC_INCOME + 4 * k) as u64, XOR_INCOME);
            m.put_u32(
                encrypted + (OFF_ENC_RATE + 4 * k) as u64,
                XOR_AI_PLANNING_RATE,
            );
            m.put_u32(encrypted + (OFF_ENC_BONUS + 4 * k) as u64, XOR_BONUS);
        }
        for k in 0..7 {
            m.put_u32(
                encrypted + (OFF_ENC_RESOURCE_CAP + 4 * k) as u64,
                XOR_RESOURCE_CAP,
            );
        }
        m.put_u32(encrypted + OFF_ENC_AGES as u64, XOR_AGES);
        m.put_u32(encrypted + OFF_ENC_EPOCHS as u64, XOR_EPOCHS);
        m.put_u32(encrypted + OFF_ENC_DISCOVERED as u64, XOR_DISCOVERED);
        for k in 0..4 {
            m.put_u32(encrypted + (OFF_ENC_EPOCH + 4 * k) as u64, XOR_EPOCH);
        }
        for i in 0..NUM_SLOTS {
            let leader = (VA_LEADERS as u64) + DELTA + i as u64 * LEADER_STRIDE as u64;
            m.alloc(
                leader,
                if i < LEADER_COUNT as usize {
                    LEADER_READ
                } else {
                    4
                },
            );
        }

        m.put_u32((VA_GAME as u64) + DELTA, game as u32);
        m.put_u32((VA_TURN_CONTROL as u64) + DELTA, turn_control as u32);
        m.put_u32((VA_WORLD as u64) + DELTA, world as u32);
        m.put_u32((VA_OBJECTS as u64) + DELTA, objects as u32);
        m.put_u32(game + OFF_GAME_FRAME as u64, 12345);
        m.put_u32(game + OFF_GAME_SECONDS as u64, 823);
        m.put_u32(game + OFF_GAME_SEMAPHORE_BYTES as u64, 0);
        m.put_u32(turn_control + OFF_TURN_CONTROL_FLAGS as u64, 0);
        m.put_u32(world + OFF_WORLD_XS as u64, 40);
        m.put_u32(world + OFF_WORLD_YS as u64, 40);
        m.put_u32(world + OFF_WORLD_TILE_XS as u64, 160);
        m.put_u32(world + OFF_WORLD_TILE_YS as u64, 160);

        // Objects: slot 0 with unit band [0,2) and build band [2000,2001).
        let arr0 = objects + OFF_OBJECTS_LISTS as u64;
        m.put_u32(arr0 + OFF_ARR_LENGTH as u64, 3001);
        m.put_u32(arr0 + OFF_ARR_SIZE as u64, 4000);
        m.put_u32(arr0 + OFF_ARR_LIST as u64, list as u32);
        m.put_u32(objects + OFF_UNIT_MARK as u64, 2);
        m.put_u32(objects + OFF_BUILD_MARK as u64, 2001);
        m.put_u32(objects + OFF_WALL_MARK as u64, 3000);
        m.put_u32(list, unit_live as u32);
        m.put_u32(list + 4, unit_free as u32);
        m.put_u32(list + 2000 * 4, build as u32);

        // Live unit: flags bit 0 set, who 3, coords masked, hp 40/60, type ptr.
        m.put(unit_live + OFF_OBJ_FLAGS as u64, &[0x01, 3]);
        m.put(unit_live + OFF_OBJ_O as u64, &(7i16).to_le_bytes());
        m.put_u32(unit_live + OFF_OBJ_X as u64, (12345i32 as u32) ^ COORD_MASK);
        m.put_u32(unit_live + OFF_OBJ_Y as u64, (67890i32 as u32) ^ COORD_MASK);
        m.put_u32(unit_live + OFF_OBJ_Z as u64, (11i32 as u32) ^ COORD_MASK);
        m.put_u32(unit_live + OFF_OBJ_PTYPE as u64, utype as u32);
        m.put_u32(unit_live + OFF_OBJ_MYHITS as u64, 60);
        m.put_u32(unit_live + OFF_OBJ_DAMAGE as u64, 20);
        m.put(unit_live + OFF_OBJ_UID as u64, &(999u16).to_le_bytes());
        m.put_u32(unit_live + OFF_UNIT_ANGLE as u64, 90);
        m.put_u32(unit_live + OFF_OBJ_VPTR as u64, 0x00b4_17d0 + DELTA as u32); // Unit

        // Free pool slot: identical except flags bit 0 clear.
        m.put(unit_free + OFF_OBJ_FLAGS as u64, &[0x00, 3]);
        m.put_u32(unit_free + OFF_OBJ_X as u64, (999i32 as u32) ^ COORD_MASK);
        m.put_u32(unit_free + OFF_OBJ_PTYPE as u64, utype as u32);
        m.put_u32(unit_free + OFF_OBJ_VPTR as u64, 0x00b4_17d0 + DELTA as u32);

        m.put(build + OFF_OBJ_FLAGS as u64, &[0x05, 1]);
        m.put_u32(build + OFF_OBJ_X as u64, (4000i32 as u32) ^ COORD_MASK);
        m.put_u32(build + OFF_OBJ_Y as u64, (5000i32 as u32) ^ COORD_MASK);
        m.put_u32(build + OFF_OBJ_PTYPE as u64, btype as u32);
        m.put_u32(build + OFF_OBJ_MYHITS as u64, 500);
        m.put_u32(build + OFF_OBJ_DAMAGE as u64, 50);
        m.put_u32(build + OFF_BUILD_CONSTRUCT_HITS as u64, 350);

        m.put_u32(utype + OFF_TYPE_INDEX as u64, 228); // Knight
        m.put_u32(btype + OFF_TYPE_INDEX as u64, 414); // Small City

        // Leaders: leader 0 in play.
        let l0 = (VA_LEADERS as u64) + DELTA;
        m.put_u32(l0 + OFF_LEADER_FLAGS as u64, 7);
        m.put_u32(l0 + OFF_LEADER_WHO as u64, 0);
        m.put_u32(l0 + OFF_LEADER_POP as u64, 29);
        m.put_u32(l0 + OFF_LEADER_ENCRYPTED as u64, encrypted as u32);
        for (k, value) in [1234u32, 2, 3, 4, 5, 6].into_iter().enumerate() {
            m.put_u32(
                encrypted + (OFF_ENC_STOCKPILE + 4 * k) as u64,
                value ^ XOR_STOCKPILE,
            );
        }
        m
    }

    #[test]
    fn free_pool_slots_are_excluded_and_coords_are_unmasked() {
        let m = build();
        let vt = VtMap::build(DELTA).unwrap();
        let s = snapshot(&m, DELTA, &vt);
        assert!(s.ok, "{}", s.note);
        assert_eq!(s.game_frame, 12345);
        assert_eq!(s.world_xs, 40);

        // Three pointers were harvested; one had flags & 1 == 0 and must not appear.
        assert_eq!(s.free_slots_skipped, 1);
        assert_eq!(s.objects.len(), 2);

        let u = s.objects.iter().find(|o| o.band == Band::Unit).unwrap();
        assert_eq!(u.x, 12345, "x_internal was not unmasked with 0x63637");
        assert_eq!(u.y, 67890);
        assert_eq!(u.z, 11);
        assert_eq!(u.who, 3);
        assert_eq!(u.uid, 999);
        assert_eq!(u.hp, 40); // myhits 60 - damage 20
        assert_eq!(u.max_hp, 60);
        assert_eq!(u.angle, 90);
        assert_eq!(u.type_index, 228);
        assert_eq!(u.kind, Kind::Unit, "vtable 0xb417d0 must type as Unit");

        let b = s.objects.iter().find(|o| o.band == Band::Build).unwrap();
        assert_eq!(b.x, 4000);
        assert_eq!(b.type_index, 414);
        assert_eq!(b.kind, Kind::Build);
        assert_eq!(
            b.max_hp, 350,
            "BuildData::hits(0) uses construct_hits, not myhits"
        );
        assert_eq!(b.hp, 300);
    }

    #[test]
    fn a_masked_read_would_be_garbage() {
        // The point of the unmask, stated as a test: the raw dword for x = 0 is the
        // mask itself, which is 407,607 fine units = 2,122 tiles off the map.
        assert_eq!(unmask_coord(COORD_MASK), 0);
        assert_eq!(unmask_coord(0), COORD_MASK as i32);
        assert_eq!(COORD_MASK as i32 / 192, 2120);
    }

    #[test]
    fn sha256_matches_published_vectors() {
        assert_eq!(
            hex_bytes(&sha256(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex_bytes(&sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            hex_bytes(&SOURCE_SHA256),
            "30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079"
        );
        assert!(supported_source_identity(SOURCE_FILE_SIZE, &SOURCE_SHA256));
        let mut wrong = SOURCE_SHA256;
        wrong[0] ^= 1;
        assert!(!supported_source_identity(SOURCE_FILE_SIZE, &wrong));
        assert!(!supported_source_identity(
            SOURCE_FILE_SIZE - 1,
            &SOURCE_SHA256
        ));
    }

    #[test]
    fn leaders_are_read() {
        let m = build();
        let s = economy_snapshot(&m, DELTA);
        assert!(s.ok, "{}", s.note);
        assert_eq!(
            s.leaders.len(),
            1,
            "only the selected human may be transported"
        );
        assert!(
            s.objects.is_empty(),
            "economy capture must not touch object state"
        );
        assert_eq!(s.human_slot, 0);
        assert_eq!(s.game_mode, GameMode::SinglePlayer);
        assert_eq!(s.paused, Some(false));
        assert_eq!(s.leaders[0].leader_flags & 1, 1);
        assert_eq!(s.leaders[0].pop, 29);
        assert_eq!(s.leaders[0].stockpile, [1234, 2, 3, 4, 5, 6]);
        assert_eq!(s.leaders[0].validity & LEADER_VALID_ECON, LEADER_VALID_ECON);
    }

    #[test]
    fn frame_encodes_to_the_declared_stride() {
        let m = build();
        let vt = VtMap::build(DELTA).unwrap();
        let mut s = snapshot(&m, DELTA, &vt);
        s.objects[0].z = -123_456;
        s.objects[0].hp = 70_000;
        s.objects[0].max_hp = 80_000;
        let f = encode_frame(&s, 1234, 42, 0x00d6_0000);
        assert_eq!(le_u32(&f, 0), WIRE_MAGIC);
        assert_eq!(le_u16(&f, 6) as usize, WIRE_HEADER_LEN);
        assert_eq!(
            f.len(),
            WIRE_HEADER_LEN
                + s.objects.len() * WIRE_OBJ_STRIDE
                + s.leaders.len() * WIRE_LEADER_STRIDE
        );
        assert_eq!(le_u32(&f, 8), 12345); // game frame
        assert_eq!(le_u32(&f, 24), 2); // object count
        let first = WIRE_HEADER_LEN;
        assert_eq!(le_i32(&f, first + 24), -123_456, "z must be present");
        assert_eq!(
            le_i32(&f, first + 32),
            70_000,
            "HP must not truncate to u16"
        );
        assert_eq!(le_i32(&f, first + 36), 80_000);
    }

    #[test]
    fn economy_ndjson_is_versioned_ordered_and_human_only() {
        let m = build();
        let s = economy_snapshot(&m, DELTA);
        let line = encode_economy_ndjson(
            &s,
            ObservationMeta {
                session_id: 0x1234,
                capture_seq: 7,
                pid: 42,
                image_base: 0x00d6_0000,
                monotonic_us: 99,
                capture_us: 123,
                ..Default::default()
            },
        );
        assert!(line.contains("\"schema\":\"rontoy.observation\""));
        assert!(line.contains("\"major\":1,\"minor\":0"));
        assert!(line.contains(
            "\"resource_order\":[\"food\",\"timber\",\"wealth\",\"knowledge\",\"metal\",\"oil\"]"
        ));
        assert!(line.contains("\"stockpile\":[1234,2,3,4,5,6]"));
        assert!(line.contains("\"game\":{\"mode\":\"single_player\",\"paused\":false"));
        assert_eq!(line.matches("\"stockpile\"").count(), 1);
        assert!(line.ends_with("}}"));
    }

    #[test]
    fn network_command_stream_is_never_attested_as_single_player() {
        let mut m = build();
        let game = 0x0500_0000u64;
        m.put_u32(game + OFF_GAME_SEMAPHORE_BYTES as u64, GAME_NETWORK_FLAG);
        let s = economy_snapshot(&m, DELTA);
        assert!(s.ok, "{}", s.note);
        assert_eq!(s.game_mode, GameMode::Multiplayer);
        let line = encode_economy_ndjson(&s, ObservationMeta::default());
        assert!(line.contains("\"game\":{\"mode\":\"multiplayer\",\"paused\":false"));
    }

    #[test]
    fn pause_state_uses_the_engines_exact_is_paused_bit() {
        let mut m = build();
        let turn_control = 0x0500_1000u64;
        m.put_u32(
            turn_control + OFF_TURN_CONTROL_FLAGS as u64,
            TURN_CONTROL_PAUSED_FLAG,
        );
        let s = economy_snapshot(&m, DELTA);
        assert!(s.ok, "{}", s.note);
        assert_eq!(s.paused, Some(true));
        let line = encode_economy_ndjson(&s, ObservationMeta::default());
        assert!(line.contains("\"paused\":true"));
    }

    struct TearOnce {
        inner: FakeMem,
        frame_addr: u64,
        frame_reads: std::cell::Cell<u32>,
    }

    impl Mem for TearOnce {
        fn read(&self, addr: u64, buf: &mut [u8]) -> usize {
            let n = self.inner.read(addr, buf);
            if addr == self.frame_addr && buf.len() == 4 && n == 4 {
                let call = self.frame_reads.get();
                self.frame_reads.set(call + 1);
                let frame = if call == 0 { 12345u32 } else { 12346u32 };
                buf.copy_from_slice(&frame.to_le_bytes());
            }
            n
        }
    }

    #[test]
    fn torn_economy_capture_retries_to_one_coherent_frame() {
        let m = TearOnce {
            inner: build(),
            frame_addr: 0x0500_0000 + OFF_GAME_FRAME as u64,
            frame_reads: std::cell::Cell::new(0),
        };
        let s = economy_snapshot_with_retries(&m, DELTA, 3);
        assert!(s.ok, "{}", s.note);
        assert_eq!(s.coherence, Coherence::Coherent);
        assert_eq!((s.frame_start, s.frame_end), (12346, 12346));
        assert_eq!(s.retry_count, 1);
    }

    #[test]
    fn duplicate_pointer_and_identity_are_not_published_twice() {
        let mut m = build();
        let list = 0x0503_0000u64;
        let unit_live = 0x0510_0000u64;
        m.put_u32(list + 4, unit_live as u32);
        let vt = VtMap::build(DELTA).unwrap();
        let s = snapshot_with_retries(&m, DELTA, &vt, 1);
        assert!(!s.ok);
        assert_eq!(s.duplicate_pointers, 1);
        assert_eq!(
            s.objects
                .iter()
                .filter(|o| o.addr == unit_live as u32)
                .count(),
            1
        );
    }

    #[test]
    fn corrupt_array_bounds_are_rejected_without_a_large_read() {
        let mut m = build();
        let objects = 0x0502_0000u64;
        let arr0 = objects + OFF_OBJECTS_LISTS as u64;
        m.put_u32(arr0 + OFF_ARR_SIZE as u64, u32::MAX);
        let vt = VtMap::build(DELTA).unwrap();
        let s = snapshot_with_retries(&m, DELTA, &vt, 1);
        assert!(!s.ok);
        assert!(s.invalid_ranges > 0);
        assert!(
            s.bytes < 1_000_000,
            "invalid capacity triggered an unbounded read"
        );
    }

    #[test]
    fn coalescing_collapses_a_packed_pool_into_one_read() {
        // 600 Units at the measured 0x168 stride: one block, not 600 syscalls.
        let addrs: Vec<u32> = (0..600).map(|i| 0x0b00_0000 + i * 0x168).collect();
        let plan = coalesce(&addrs, OBJ_READ, 4096, 1 << 20);
        assert_eq!(plan.len(), 1);
        // Scattered objects a megabyte apart cannot be merged.
        let far: Vec<u32> = (0..4).map(|i| 0x0b00_0000 + i * 0x0010_0000).collect();
        assert_eq!(coalesce(&far, OBJ_READ, 4096, 1 << 20).len(), 4);
    }

    #[test]
    fn map_grid_is_indexed_row_major_by_the_engines_own_formula() {
        // WorldData::get_who 0x006B4700: (y * xs + x) * 0x1c + 0xf.
        let mut m = build();
        let wdata = 0x0530_0000u64;
        m.put_u32(0x0501_0000 + OFF_WORLD_WDATA as u64, wdata as u32);
        m.alloc(wdata, 40 * 40 * WDATA_STRIDE);
        for i in 0..40 * 40 {
            m.put(
                wdata + (i * WDATA_STRIDE + OFF_WDATA_WHO) as u64,
                &[u8::MAX],
            );
        }
        let xs = 40i64;
        // cell (3, 5) owned by player 2, land class 7
        let idx = (5 * xs + 3) as u64;
        m.put(
            wdata + idx * WDATA_STRIDE as u64 + OFF_WDATA_LAND as u64,
            &[7],
        );
        m.put(
            wdata + idx * WDATA_STRIDE as u64 + OFF_WDATA_WHO as u64,
            &[2],
        );
        let g = read_map(&m, DELTA).unwrap();
        assert_eq!(g.xs, 40);
        assert_eq!(g.who[(5 * 40 + 3) as usize], 3, "who is stored +1 biased");
        assert_eq!(g.land[(5 * 40 + 3) as usize], 7);
        assert_eq!(g.who[0], 0);
    }
}
