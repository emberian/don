//! Authoritative initial-game setup from the `.rcx` prefix.
//!
//! A recording begins with `Game::walk_data` (`0x00589600`), not an ad-hoc
//! replay header.  The first child is `GameInfo::walk_data` (`0x005d6570`), so
//! the seed, map selectors and all eight `Player` setup records are available
//! before the first command package. This module consumes that prefix in the
//! exact retail walk order. Dynamic initial-world state after
//! `game.info.save_name` is still not presented as a save snapshot. The static
//! Rules section at the end of that span is now independently bounded and
//! projected through its exact checksum-only traversal.

use crate::checksum::adler32;
use crate::rules_channel::{
    BALANCE_BYTES, RETAIL_AFTER_BALANCE, RETAIL_AFTER_CONSTANTS, RETAIL_AFTER_TRIBES,
    RETAIL_AFTER_TYPES, RETAIL_WALKED_BYTES, RULES_BLOCK_BYTES, RULES_DUPLICATE_OFFSET,
    SHIPPED_RULES_CHANNEL, TRIBE_COUNT, TRIBE_SIZE, TYPE_SLOTS,
};
use don_sim::systems::map_terrain::{World, WorldChecksum};

pub const TAG_GAME: u8 = 0x16;
pub const TAG_GAME_INFO: u8 = 0x42;
pub const TAG_PLAYER: u8 = 0x50;
/// `Game::walk_rules_data`'s section tag in every supported-corpus recording.
///
/// The byte is written by `SaveGame::walk_tag` before `Types::walk_rules_data`
/// (`0x00589550`); `CheckSum::walk_tag` is a no-op, so it is parsed but never
/// handed to the checksum primitive.
pub const TAG_RULES: u8 = 0x92;
/// The tag emitted before each of the 24 `Tribe::walk_rules_data` bodies.
pub const TAG_TRIBE: u8 = 0x8f;

/// Exact serialized length of the unmodded shipped Rules section.
///
/// Measured independently in the 2024.06.20 solo and multiplayer recordings
/// and in the 2017.11.29 corpus: the section begins at its `0x92` tag and ends
/// exactly at the first command-package byte in all specimens.
pub const SHIPPED_RULES_SERIALIZED_BYTES: usize = 1_024_221;
/// Serialized `Types::walk_rules_data` body, excluding the outer Rules tag.
/// The difference from `RETAIL_TYPE_WALKED_BYTES` is exactly the save-only
/// Type-name and Tech string encodings.
pub const SHIPPED_TYPES_SERIALIZED_BYTES: usize = 500_334;

/// Search boundary after the already-parsed `Game` prefix. The only
/// intervening writers are one `String::walk_data` and at most 44 bytes of
/// per-player extras (`0x009534e0`). The deliberately generous bound keeps a
/// corrupt length from turning parsing into an unbounded command-stream scan;
/// failure simply leaves the channel absent.
const RULES_SEARCH_BYTES: usize = 256 * 1024;

/// The seven shipped `MapSizeData::data[0]` world-cell edges, in list order.
///
/// The list is `[40, 50, 60, 70, 80, 90, 100]`; `GameInfo::map_size` is its
/// index.  `Game::wonder_timer` independently reads index 3 as the 70-cell
/// Standard-map reference (`0x005944f0`), pinning both the unit and the order.
pub const MAP_SIZE_WORLD_EDGES: [i32; 7] = [40, 50, 60, 70, 80, 90, 100];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameSettings {
    pub team_style: u8,
    pub map_style: u8,
    pub map_size: u8,
    pub players: u8,
    pub max_observers: u8,
    pub game_speed: u8,
    pub game_rules: u8,
    pub difficulty: u8,
    pub starting_town: u8,
    pub starting_resources: u8,
    pub starting_resources2: u8,
    pub tech_cost: u8,
    pub reveal_map: u8,
    pub pop_limit: u8,
    pub rush_rules: u8,
    pub cannon_times: u8,
    pub starting_technology: u8,
    pub starting_technology2: u8,
    pub ending_technology: u8,
    pub elimination: u8,
    pub victory: u8,
    pub wonderwin: u8,
    pub score_goal: u8,
    pub popwin: u8,
    pub time_limit: u8,
    pub chairs: u8,
    pub econwin: u8,
    pub scenario_type: u8,
    pub script_type: u8,
    pub mods: u8,
}

impl GameSettings {
    fn from_bytes(b: [u8; 30]) -> Self {
        Self {
            team_style: b[0],
            map_style: b[1],
            map_size: b[2],
            players: b[3],
            max_observers: b[4],
            game_speed: b[5],
            game_rules: b[6],
            difficulty: b[7],
            starting_town: b[8],
            starting_resources: b[9],
            starting_resources2: b[10],
            tech_cost: b[11],
            reveal_map: b[12],
            pop_limit: b[13],
            rush_rules: b[14],
            cannon_times: b[15],
            starting_technology: b[16],
            starting_technology2: b[17],
            ending_technology: b[18],
            elimination: b[19],
            victory: b[20],
            wonderwin: b[21],
            score_goal: b[22],
            popwin: b[23],
            time_limit: b[24],
            chairs: b[25],
            econwin: b[26],
            scenario_type: b[27],
            script_type: b[28],
            mods: b[29],
        }
    }

    pub fn map_edge_world_cells(self) -> Option<i32> {
        MAP_SIZE_WORLD_EDGES.get(self.map_size as usize).copied()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitialPlayer {
    pub slot: u8,
    pub present: bool,
    /// The two-byte `Player::flags` gate written before the conditional body.
    pub flags: u16,
    /// Ten synchronized input counters, then `caravan_frame` and `pop_cap_frame`.
    pub counters_and_frames: [u32; 12],
    pub tribe: u8,
    pub who: u8,
    pub team: u8,
    pub handicap: u8,
    pub play: u8,
    pub pauses: u8,
    pub difficulty: u8,
    pub name: String,
}

impl InitialPlayer {
    fn absent(slot: u8, flags: u16) -> Self {
        Self {
            slot,
            present: false,
            flags,
            counters_and_frames: [0; 12],
            tribe: 0,
            who: 0,
            team: 0,
            handicap: 0,
            play: 0,
            pauses: 0,
            difficulty: 0,
            name: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModBlock {
    V16 {
        checksum: u32,
        total_size: u32,
        scenario_script: String,
        scenario_path: String,
        mod_name: String,
        checksum2: u32,
        total_size2: u32,
        mod_name2: String,
    },
    V15 {
        scenario_script: String,
        scenario_path: String,
        scenario_dir: String,
        mod_name: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitialGameInfo {
    pub version_string: String,
    pub version: u32,
    pub seed: u32,
    pub checksum_deep: i32,
    pub checksum_window_size: i32,
    pub checksum_failure_threshold: i32,
    pub flags: u32,
    pub settings: GameSettings,
    pub players: Vec<InitialPlayer>,
    pub mods: ModBlock,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitialGame {
    pub frame: i32,
    pub frame_to_break: i32,
    pub playing: i32,
    pub loading: i32,
    pub tick: i32,
    pub market_tick: i32,
    pub market: [i32; 6],
    pub world_cities: i32,
    pub world_villages: i32,
    pub total_units: i32,
    pub everyone_mask: i32,
    pub armageddon: i32,
    pub semaphore_bits: i32,
    pub semaphore: Vec<u8>,
    pub graphic_tick: i32,
}

/// Parsed setup and the exact number of prefix bytes consumed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitialState {
    /// `sGameSaveVersion`, inferred as retail does not put the global version
    /// word in a recording.  The v15/v16 tail is selected only when the
    /// following `Game::semaphore` satisfies its PDB layout invariant.
    pub save_format: u32,
    pub info: InitialGameInfo,
    pub game: InitialGame,
    pub save_name: String,
    pub bytes_walked: usize,
    /// Static rules recovered from the replay's own SaveGame section.
    ///
    /// `None` is fail-closed: conquest/custom recordings may omit this section,
    /// and a structurally valid section is still rejected unless every
    /// independently captured cumulative checkpoint agrees with retail.
    pub rules: Option<InitialRules>,
}

/// Checksum-visible projection of the replay-carried static Rules section.
///
/// This is not a copied wire checksum. The parser independently replays the
/// exact `Game::walk_rules_data` ordering over the bytes written by the shared
/// `SaveGame` visitor, skipping only section tags and save-only strings exactly
/// where the shipped walkers gate on `DataWalk::is_checksum`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InitialRules {
    pub serialized_offset: usize,
    pub serialized_bytes: usize,
    pub walked_bytes: u64,
    pub checksum: u32,
    pub after_types: u32,
    pub after_constants: u32,
    pub after_balance: u32,
}

/// The largest world reconstruction justified by the prefix alone.
///
/// `world` follows the shipped map-size table and exact `World::init` dimension
/// arithmetic, then installs the replay seed through the oracle-backed
/// `Map::make` entry semantics.  Terrain, resource placement and starting
/// coordinates remain zero and every byte they contribute is counted as
/// unsourced; this object is a divergence-producing checksum slice, not a
/// fabricated complete save.
#[derive(Clone, Debug)]
pub struct InitialWorld {
    pub world: World,
    pub checksum: WorldChecksum,
    pub sourced_walked_bytes: u64,
}

impl InitialWorld {
    pub fn unsourced_walked_bytes(&self) -> u64 {
        self.checksum
            .bytes
            .saturating_sub(self.sourced_walked_bytes)
    }
}

impl InitialState {
    pub fn active_players(&self) -> impl Iterator<Item = &InitialPlayer> {
        self.info.players.iter().filter(|p| p.present)
    }

    /// Apply only prefix-proven setup to the sim's exact world checksum owner.
    pub fn reconstruct_world(&self) -> Option<InitialWorld> {
        // Non-zero scenario types may carry custom dimensions in the still-
        // opaque state block. `map_size` is then UI/setup metadata, not proof
        // that `World::xs/ys` equal the shipped procedural size entry.
        if self.info.settings.scenario_type != 0 {
            return None;
        }
        let edge = self.info.settings.map_edge_world_cells()?;
        let mut world = World::init_default_rules(edge, edge);
        // `GameInfo::seed` is unsigned, but Map::make's argument is signed and
        // its exact prefix preserves prior state for negative values.
        let seed_installed = world.seed_map_generation(self.info.seed as i32).is_some();
        let checksum = world.checksum_sections();

        // Prefix-proven bytes in World::walk_data:
        //   §1: xs/ys (8)
        //   §4: ten dimensions/derived sizes (40), the six default territory
        //       limits (24, for the unmodded shipped rule load), and seed (4).
        // Nothing in the prefix proves the remaining arrays or generated map.
        let unmodded = self.info.settings.mods == 0
            && match &self.info.mods {
                ModBlock::V16 {
                    checksum,
                    total_size,
                    checksum2,
                    total_size2,
                    mod_name,
                    mod_name2,
                    ..
                } => {
                    *checksum == 0
                        && *total_size == 0
                        && *checksum2 == 0
                        && *total_size2 == 0
                        && mod_name.is_empty()
                        && mod_name2.is_empty()
                }
                ModBlock::V15 { mod_name, .. } => mod_name.is_empty(),
            };
        let sourced_walked_bytes =
            8 + 40 + if seed_installed { 4 } else { 0 } + if unmodded { 24 } else { 0 };
        Some(InitialWorld {
            world,
            checksum,
            sourced_walked_bytes,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub offset: usize,
    pub message: String,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "initial prefix at {:#x}: {}", self.offset, self.message)
    }
}

struct Reader<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> Reader<'a> {
    fn new(b: &'a [u8]) -> Self {
        Self { b, p: 0 }
    }

    fn err(&self, message: impl Into<String>) -> ParseError {
        ParseError {
            offset: self.p,
            message: message.into(),
        }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], ParseError> {
        let end = self
            .p
            .checked_add(n)
            .ok_or_else(|| self.err("offset overflow"))?;
        if end > self.b.len() {
            return Err(self.err(format!(
                "need {n} bytes, only {} remain",
                self.b.len() - self.p
            )));
        }
        let out = &self.b[self.p..end];
        self.p = end;
        Ok(out)
    }

    fn u8(&mut self) -> Result<u8, ParseError> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, ParseError> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }
    fn u32(&mut self) -> Result<u32, ParseError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn i32(&mut self) -> Result<i32, ParseError> {
        Ok(self.u32()? as i32)
    }
    fn string(&mut self) -> Result<String, ParseError> {
        let n = self.u32()? as usize;
        if n > 32_768 {
            return Err(self.err(format!("absurd UTF-16 string length {n}")));
        }
        let raw = self.take(
            n.checked_mul(2)
                .ok_or_else(|| self.err("string overflow"))?,
        )?;
        let words = raw
            .chunks_exact(2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]));
        String::from_utf16(&words.collect::<Vec<_>>())
            .map_err(|_| self.err("invalid UTF-16 string"))
    }
    fn tag(&mut self, want: u8, name: &str) -> Result<(), ParseError> {
        let got = self.u8()?;
        if got == want {
            Ok(())
        } else {
            Err(self.err(format!("{name} tag {got:#04x}, expected {want:#04x}")))
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct RulesAdler {
    checksum: u32,
    bytes: u64,
}

impl RulesAdler {
    fn new() -> Self {
        Self {
            checksum: 1,
            bytes: 0,
        }
    }

    fn update(&mut self, bytes: &[u8]) {
        self.checksum = adler32(self.checksum, bytes);
        self.bytes += bytes.len() as u64;
    }
}

fn rules_walk(r: &mut Reader<'_>, adler: &mut RulesAdler, n: usize) -> Result<(), ParseError> {
    let bytes = r.take(n)?;
    adler.update(bytes);
    Ok(())
}

fn rules_skip_string(r: &mut Reader<'_>) -> Result<(), ParseError> {
    let n = r.u32()? as usize;
    if n > 32_768 {
        return Err(r.err(format!("absurd Rules UTF-16 string length {n}")));
    }
    let raw = r.take(
        n.checked_mul(2)
            .ok_or_else(|| r.err("Rules string overflow"))?,
    )?;
    let words = raw
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]));
    if std::char::decode_utf16(words).any(|c| c.is_err()) {
        return Err(r.err("invalid Rules UTF-16 string"));
    }
    Ok(())
}

fn rules_walk_u16_array(r: &mut Reader<'_>, adler: &mut RulesAdler) -> Result<usize, ParseError> {
    let raw_count = r.take(4)?;
    let count = i32::from_le_bytes(raw_count.try_into().unwrap());
    adler.update(raw_count);
    if !(0..=TYPE_SLOTS as i32).contains(&count) {
        return Err(r.err(format!(
            "Rules u16 array count {count} outside 0..={TYPE_SLOTS}"
        )));
    }
    if count == 0 {
        return Ok(0);
    }

    // `SimpleArray<unsigned short>::walk_data` (`0x00476610`) writes these
    // seven bytes only for a non-empty array: capacity, grow, flags & 0xbf.
    let metadata = r.take(7)?;
    let capacity = i32::from_le_bytes(metadata[..4].try_into().unwrap());
    if capacity < count {
        return Err(r.err(format!(
            "Rules u16 array capacity {capacity} below count {count}"
        )));
    }
    if metadata[6] & 0x40 != 0 {
        return Err(r.err("Rules u16 array retained masked flag 0x40"));
    }
    adler.update(metadata);
    rules_walk(r, adler, count as usize * 2)?;
    Ok(count as usize)
}

/// Parse one candidate `Game::walk_rules_data` SaveGame section and project it
/// through the checksum-only traversal.
///
/// The four checkpoint comparisons are admission gates, not values substituted
/// into the result. A one-byte mutation in Types, Constants, Balance, or Tribes
/// changes the independently computed accumulator and makes this function
/// refuse the section.
pub fn parse_serialized_rules_at(
    payload: &[u8],
    offset: usize,
) -> Result<InitialRules, ParseError> {
    let section = payload.get(offset..).ok_or(ParseError {
        offset,
        message: "Rules offset outside payload".into(),
    })?;
    let result = (|| {
        let mut r = Reader::new(section);
        let mut adler = RulesAdler::new();
        r.tag(TAG_RULES, "Rules")?;

        // `Types::walk_rules_data` (`0x00669800`) dispatches 806 records in
        // global TypeIndex order. The dynamic kind intervals are fixed by the
        // shipped registries and independently checked by the live Type capture.
        let mut array_elements = 0usize;
        for slot in 0..TYPE_SLOTS {
            let kind = match slot {
                0..=49 => 0,    // GoodType
                50..=413 => 1,  // UnitType
                414..=542 => 2, // BuildType
                543 => 3,       // ItemType -> ObjectType walker
                544..=628 => 4, // TechType
                629..=683 => 5, // SpellType
                _ => 6,         // BonusType -> Type walker
            };

            // Type::walk_rules_data (`0x00663190`): [this+4,this+0x5e),
            // followed in SaveGame only by `name` String::walk_data.
            rules_walk(&mut r, &mut adler, 90)?;
            rules_skip_string(&mut r)?;

            if kind <= 3 {
                // ObjectType::walk_rules_data (`0x0065fba0`).
                rules_walk(&mut r, &mut adler, 152)?;
                array_elements += rules_walk_u16_array(&mut r, &mut adler)?;
                array_elements += rules_walk_u16_array(&mut r, &mut adler)?;
            }

            match kind {
                0 => rules_walk(&mut r, &mut adler, 68)?,
                // Unit's four consecutive calls total 24 + 8 + 4 + 756.
                1 => rules_walk(&mut r, &mut adler, 792)?,
                2 => rules_walk(&mut r, &mut adler, 49)?,
                4 => {
                    rules_walk(&mut r, &mut adler, 27)?;
                    // TechType saves eight additional strings, all gated out
                    // of CheckSum by `DataWalk+0x08`.
                    for _ in 0..8 {
                        rules_skip_string(&mut r)?;
                    }
                }
                5 => rules_walk(&mut r, &mut adler, 48)?,
                3 | 6 => {}
                _ => unreachable!(),
            }
        }
        if array_elements != 2_363 {
            return Err(r.err(format!(
                "Rules Type arrays contain {array_elements} elements, expected 2363"
            )));
        }
        if r.p != 1 + SHIPPED_TYPES_SERIALIZED_BYTES {
            return Err(r.err(format!(
                "Rules Types serialized width {}, expected {}",
                r.p - 1,
                SHIPPED_TYPES_SERIALIZED_BYTES
            )));
        }
        let after_types = adler.checksum;
        if after_types != RETAIL_AFTER_TYPES {
            return Err(r.err(format!(
                "Rules Types checkpoint {after_types:#010x}, expected {RETAIL_AFTER_TYPES:#010x}"
            )));
        }

        let constants_at = r.p;
        rules_walk(&mut r, &mut adler, RULES_BLOCK_BYTES)?;
        let duplicate: [u8; 4] = r.take(4)?.try_into().unwrap();
        let source: [u8; 4] = section
            [constants_at + RULES_DUPLICATE_OFFSET..constants_at + RULES_DUPLICATE_OFFSET + 4]
            .try_into()
            .unwrap();
        if duplicate != source {
            return Err(r.err("Rules Constants duplicate visit does not repeat +0x804"));
        }
        adler.update(&duplicate);
        let after_constants = adler.checksum;
        if after_constants != RETAIL_AFTER_CONSTANTS {
            return Err(r.err(format!(
                "Rules Constants checkpoint {after_constants:#010x}, expected {RETAIL_AFTER_CONSTANTS:#010x}"
            )));
        }

        rules_walk(&mut r, &mut adler, BALANCE_BYTES)?;
        let after_balance = adler.checksum;
        if after_balance != RETAIL_AFTER_BALANCE {
            return Err(r.err(format!(
                "Rules Balance checkpoint {after_balance:#010x}, expected {RETAIL_AFTER_BALANCE:#010x}"
            )));
        }

        for tribe in 0..TRIBE_COUNT {
            r.tag(TAG_TRIBE, &format!("Tribe[{tribe}]"))?;
            rules_walk(&mut r, &mut adler, 0x18)?;
            rules_walk(&mut r, &mut adler, TRIBE_SIZE - 0x70)?;
        }

        if r.p != SHIPPED_RULES_SERIALIZED_BYTES {
            return Err(r.err(format!(
                "Rules serialized width {}, expected {SHIPPED_RULES_SERIALIZED_BYTES}",
                r.p
            )));
        }
        if adler.bytes != RETAIL_WALKED_BYTES {
            return Err(r.err(format!(
                "Rules walked {} bytes, expected {RETAIL_WALKED_BYTES}",
                adler.bytes
            )));
        }
        if adler.checksum != RETAIL_AFTER_TRIBES || adler.checksum != SHIPPED_RULES_CHANNEL {
            return Err(r.err(format!(
                "Rules final checkpoint {:#010x}, expected {SHIPPED_RULES_CHANNEL:#010x}",
                adler.checksum
            )));
        }

        Ok(InitialRules {
            serialized_offset: offset,
            serialized_bytes: r.p,
            walked_bytes: adler.bytes,
            checksum: adler.checksum,
            after_types,
            after_constants,
            after_balance,
        })
    })();
    result.map_err(|mut e: ParseError| {
        e.offset = e.offset.saturating_add(offset);
        e
    })
}

fn find_serialized_rules(
    payload: &[u8],
    search_from: usize,
) -> Result<Option<InitialRules>, ParseError> {
    let available_end = payload
        .len()
        .checked_sub(SHIPPED_RULES_SERIALIZED_BYTES)
        .map(|v| v + 1)
        .unwrap_or(0);
    let scan_end = search_from
        .saturating_add(RULES_SEARCH_BYTES)
        .min(available_end);
    let mut found = None;
    for offset in search_from.min(scan_end)..scan_end {
        if payload[offset] != TAG_RULES {
            continue;
        }
        let Ok(candidate) = parse_serialized_rules_at(payload, offset) else {
            continue;
        };
        if found.is_some() {
            return Err(ParseError {
                offset,
                message: "ambiguous duplicate shipped Rules sections".into(),
            });
        }
        found = Some(candidate);
    }
    Ok(found)
}

fn at_i32(body: &[u8], at: usize) -> i32 {
    i32::from_le_bytes([body[at], body[at + 1], body[at + 2], body[at + 3]])
}

fn parse_mod_block(r: &mut Reader<'_>, format: u32) -> Result<ModBlock, ParseError> {
    if format >= 16 {
        Ok(ModBlock::V16 {
            checksum: r.u32()?,
            total_size: r.u32()?,
            scenario_script: r.string()?,
            scenario_path: r.string()?,
            mod_name: r.string()?,
            checksum2: r.u32()?,
            total_size2: r.u32()?,
            mod_name2: r.string()?,
        })
    } else {
        Ok(ModBlock::V15 {
            scenario_script: r.string()?,
            scenario_path: r.string()?,
            scenario_dir: r.string()?,
            mod_name: r.string()?,
        })
    }
}

fn parse_candidate(payload: &[u8], format: u32) -> Result<InitialState, ParseError> {
    let mut r = Reader::new(payload);
    r.tag(TAG_GAME, "Game")?;
    r.tag(TAG_GAME_INFO, "GameInfo")?;
    let version_string = r.string()?;
    let version = r.u32()?;
    let seed = r.u32()?;
    let checksum_deep = r.i32()?;
    let checksum_window_size = r.i32()?;
    let checksum_failure_threshold = r.i32()?;
    let flags = r.u32()?;
    let settings = GameSettings::from_bytes(
        r.take(30)?
            .try_into()
            .map_err(|_| r.err("GameInfo settings width"))?,
    );

    let mut players = Vec::with_capacity(8);
    for slot in 0..8u8 {
        r.tag(TAG_PLAYER, "Player")?;
        let flags = r.u16()?;
        if flags & 1 == 0 {
            players.push(InitialPlayer::absent(slot, flags));
            continue;
        }
        let body = r.take(0x39)?;
        let mut counters_and_frames = [0u32; 12];
        for (i, v) in counters_and_frames.iter_mut().enumerate() {
            let at = i * 4;
            *v = u32::from_le_bytes([body[at], body[at + 1], body[at + 2], body[at + 3]]);
        }
        let body_flags = u16::from_le_bytes([body[0x30], body[0x31]]);
        if body_flags != flags {
            return Err(r.err(format!(
                "Player[{slot}] flags disagree: gate={flags:#06x}, body={body_flags:#06x}"
            )));
        }
        players.push(InitialPlayer {
            slot,
            present: true,
            flags,
            counters_and_frames,
            tribe: body[0x32],
            who: body[0x33],
            team: body[0x34],
            handicap: body[0x35],
            play: body[0x36],
            pauses: body[0x37],
            difficulty: body[0x38],
            name: r.string()?,
        });
    }
    let mods = parse_mod_block(&mut r, format)?;

    let body = r.take(404)?;
    let mut market = [0i32; 6];
    for (i, v) in market.iter_mut().enumerate() {
        *v = at_i32(body, 0x18 + i * 4);
    }
    let frame = at_i32(body, 0);
    let frame_to_break = at_i32(body, 4);
    let playing = at_i32(body, 8);
    let loading = at_i32(body, 12);
    let tick = at_i32(body, 16);
    let market_tick = at_i32(body, 20);
    let world_cities = at_i32(body, 0x180);
    let world_villages = at_i32(body, 0x184);
    let total_units = at_i32(body, 0x188);
    let everyone_mask = at_i32(body, 0x18c);
    let armageddon = at_i32(body, 0x190);

    let semaphore_bits = r.i32()?;
    let semaphore_size = r.i32()?;
    if !(0..=32).contains(&semaphore_size) || semaphore_bits != semaphore_size * 8 {
        return Err(r.err(format!(
            "invalid Game semaphore bits={semaphore_bits}, size={semaphore_size}"
        )));
    }
    let semaphore = r.take(semaphore_size as usize)?.to_vec();
    let graphic_tick = r.i32()?;
    let save_name = r.string()?;
    let bytes_walked = r.p;
    let rules = find_serialized_rules(payload, bytes_walked)?;

    Ok(InitialState {
        save_format: format,
        info: InitialGameInfo {
            version_string,
            version,
            seed,
            checksum_deep,
            checksum_window_size,
            checksum_failure_threshold,
            flags,
            settings,
            players,
            mods,
        },
        game: InitialGame {
            frame,
            frame_to_break,
            playing,
            loading,
            tick,
            market_tick,
            market,
            world_cities,
            world_villages,
            total_units,
            everyone_mask,
            armageddon,
            semaphore_bits,
            semaphore,
            graphic_tick,
        },
        save_name,
        bytes_walked,
        rules,
    })
}

/// Parse the retail `Game`/`GameInfo` prefix and infer its v15/v16 tail.
pub fn parse_initial_state(payload: &[u8]) -> Result<InitialState, ParseError> {
    let mut last = None;
    for format in [16, 15] {
        match parse_candidate(payload, format) {
            Ok(s) => return Ok(s),
            Err(e) => last = Some(e),
        }
    }
    Err(last.unwrap_or(ParseError {
        offset: 0,
        message: "no supported GameInfo format".into(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wstr(out: &mut Vec<u8>, s: &str) {
        let w: Vec<u16> = s.encode_utf16().collect();
        out.extend_from_slice(&(w.len() as u32).to_le_bytes());
        for c in w {
            out.extend_from_slice(&c.to_le_bytes());
        }
    }

    fn fixture(format: u32) -> Vec<u8> {
        let mut b = vec![TAG_GAME, TAG_GAME_INFO];
        wstr(&mut b, "(Version: test)");
        b.extend_from_slice(&7u32.to_le_bytes());
        b.extend_from_slice(&0x1234_5678u32.to_le_bytes());
        b.extend_from_slice(&(-2i32).to_le_bytes());
        b.extend_from_slice(&3i32.to_le_bytes());
        b.extend_from_slice(&4i32.to_le_bytes());
        b.extend_from_slice(&0x60u32.to_le_bytes());
        let mut settings = [0u8; 30];
        settings[1] = 12;
        settings[2] = 3;
        settings[3] = 2;
        b.extend_from_slice(&settings);
        for slot in 0..8u8 {
            b.push(TAG_PLAYER);
            let present = slot < 2;
            let flags = if present { 1u16 } else { 0 };
            b.extend_from_slice(&flags.to_le_bytes());
            if present {
                let mut body = [0u8; 0x39];
                body[0x30..0x32].copy_from_slice(&flags.to_le_bytes());
                body[0x32] = 20 + slot;
                body[0x33] = slot;
                body[0x34] = slot + 1;
                body[0x36] = slot;
                b.extend_from_slice(&body);
                wstr(&mut b, if slot == 0 { "one" } else { "two" });
            }
        }
        if format >= 16 {
            b.extend_from_slice(&0u32.to_le_bytes());
            b.extend_from_slice(&0u32.to_le_bytes());
            wstr(&mut b, "");
            wstr(&mut b, "");
            wstr(&mut b, "");
            b.extend_from_slice(&0u32.to_le_bytes());
            b.extend_from_slice(&0u32.to_le_bytes());
            wstr(&mut b, "");
        } else {
            for _ in 0..4 {
                wstr(&mut b, "");
            }
        }
        let mut game = [0u8; 404];
        game[4..8].copy_from_slice(&(-1i32).to_le_bytes());
        b.extend_from_slice(&game);
        b.extend_from_slice(&16i32.to_le_bytes());
        b.extend_from_slice(&2i32.to_le_bytes());
        b.extend_from_slice(&[0xaa, 0x55]);
        b.extend_from_slice(&9i32.to_le_bytes());
        wstr(&mut b, "fixture");
        b
    }

    #[test]
    fn parses_both_retail_tail_formats_and_player_setup() {
        for format in [15, 16] {
            let b = fixture(format);
            let s = parse_initial_state(&b).unwrap();
            assert_eq!(s.save_format, format);
            assert_eq!(s.bytes_walked, b.len());
            assert_eq!(s.info.seed, 0x1234_5678);
            assert_eq!(s.info.settings.map_size, 3);
            assert_eq!(s.info.settings.map_edge_world_cells(), Some(70));
            let p: Vec<_> = s.active_players().collect();
            assert_eq!(p.len(), 2);
            assert_eq!((p[1].who, p[1].team, p[1].name.as_str()), (1, 2, "two"));
            assert_eq!(s.game.semaphore, [0xaa, 0x55]);
        }
    }

    #[test]
    fn reconstruction_is_nonempty_and_marks_generated_bytes_unsourced() {
        let s = parse_initial_state(&fixture(16)).unwrap();
        let w = s.reconstruct_world().unwrap();
        assert_eq!((w.world.xs, w.world.ys), (70, 70));
        assert_eq!(w.world.seed, 0x1234_5678);
        assert!(w.checksum.bytes > w.sourced_walked_bytes);
        assert!(w.unsourced_walked_bytes() > 100_000);
        assert_ne!(w.checksum.full, 1);
    }

    #[test]
    fn custom_scenario_does_not_invent_dimensions_from_the_ui_map_size() {
        let mut b = fixture(16);
        // GameInfo settings begin after two tags, the String, and 24 scalar
        // bytes. Locate the known 30-byte fixture block by its map style/size.
        let at = b.windows(3).position(|w| w == [0, 12, 3]).unwrap();
        b[at + 27] = 5;
        let s = parse_initial_state(&b).unwrap();
        assert_eq!(s.info.settings.scenario_type, 5);
        assert!(s.reconstruct_world().is_none());
    }
}
