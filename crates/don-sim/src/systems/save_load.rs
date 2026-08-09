//! Deterministic save/load for the authoritative headless simulation tranche.
//!
//! This is intentionally **not** a Rise of Nations `.svx` writer. Retail's top-level
//! `SaveGame` stream walks 91 object-graph sections that `don-sim` does not yet own.
//! Claiming compatibility while omitting one would turn a load into silent state loss.
//!
//! The container primitive is retail-exact, however: every chunk starts with the shipped
//! eight-byte `ChunkHeader { u32 size, u16 id, u16 num_chunks }`; `size` includes the
//! header and nested bytes, and a parent's size includes every child. Unknown, duplicate,
//! missing, truncated, trailing, or unsupported sections are rejected.

use std::fmt;

use crate::generated::state::unit::{UnitCols, W1_PLANES, W2_PLANES, W4_PLANES, WF_PLANES};
use crate::order::{Order, OrderIndex, OrderList};
use crate::systems::{borders_fog, economy, leaders as step8, map_terrain, movement};
use crate::tick::{LeaderSlot, Sim, NUM_LEADERS};
use crate::world::{WorldSaveError, WorldSaveState, MAX_UNITS};

const MAGIC: &[u8; 8] = b"DoNSave\0";
const FORMAT_VERSION: u32 = 1;
const MAX_SAVE_BYTES: usize = 256 * 1024 * 1024;
const MAX_ORDERS_PER_UNIT: usize = 1024;
const MAX_PATH_RECORDS: usize = 1 << 20;

const ROOT: u16 = 0x444e;
const CORE: u16 = 0x0001;
const MAP: u16 = 0x0002;
const OBJECTS: u16 = 0x0003;
const LEADERS: u16 = 0x0004;
const PATHS: u16 = 0x0005;
const REQUIRED: [u16; 5] = [CORE, MAP, OBJECTS, LEADERS, PATHS];

/// A bounded, fail-closed save/load failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SaveError {
    Unsupported(&'static str),
    Invalid(&'static str),
    InvalidChunk(&'static str),
    MissingChunk(u16),
    DuplicateChunk(u16),
    UnknownChunk(u16),
    Limit(&'static str),
    World(String),
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(s) => write!(f, "unsupported save section: {s}"),
            Self::Invalid(s) => write!(f, "invalid save state: {s}"),
            Self::InvalidChunk(s) => write!(f, "invalid chunk container: {s}"),
            Self::MissingChunk(id) => write!(f, "missing required chunk 0x{id:04x}"),
            Self::DuplicateChunk(id) => write!(f, "duplicate chunk 0x{id:04x}"),
            Self::UnknownChunk(id) => write!(f, "unknown chunk 0x{id:04x}"),
            Self::Limit(s) => write!(f, "save limit exceeded: {s}"),
            Self::World(s) => write!(f, "invalid world state: {s}"),
        }
    }
}

impl std::error::Error for SaveError {}

impl From<WorldSaveError> for SaveError {
    fn from(value: WorldSaveError) -> Self {
        Self::World(value.to_string())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ChunkHeader {
    size: u32,
    id: u16,
    num_chunks: u16,
}

struct Chunk {
    id: u16,
    children: Vec<Chunk>,
    data: Vec<u8>,
}

impl Chunk {
    fn leaf(id: u16, data: Vec<u8>) -> Self {
        Self {
            id,
            children: Vec::new(),
            data,
        }
    }

    fn branch(id: u16, children: Vec<Chunk>) -> Self {
        Self {
            id,
            children,
            data: Vec::new(),
        }
    }

    fn encode(&self) -> Result<Vec<u8>, SaveError> {
        if !self.children.is_empty() && !self.data.is_empty() {
            return Err(SaveError::InvalidChunk("mixed child and raw payload"));
        }
        let mut body = Vec::new();
        if self.children.is_empty() {
            body.extend_from_slice(&self.data);
        } else {
            for child in &self.children {
                body.extend_from_slice(&child.encode()?);
            }
        }
        let size = body
            .len()
            .checked_add(8)
            .and_then(|n| u32::try_from(n).ok())
            .ok_or(SaveError::Limit("chunk size"))?;
        let num_chunks = u16::try_from(self.children.len())
            .map_err(|_| SaveError::Limit("chunk child count"))?;
        let mut out = Vec::with_capacity(size as usize);
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&self.id.to_le_bytes());
        out.extend_from_slice(&num_chunks.to_le_bytes());
        out.extend_from_slice(&body);
        Ok(out)
    }
}

struct ParsedChunk<'a> {
    header: ChunkHeader,
    data: &'a [u8],
    children: Vec<ParsedChunk<'a>>,
}

fn parse_chunk(input: &[u8]) -> Result<ParsedChunk<'_>, SaveError> {
    if input.len() < 8 {
        return Err(SaveError::InvalidChunk("truncated header"));
    }
    let header = ChunkHeader {
        size: u32::from_le_bytes(input[0..4].try_into().unwrap()),
        id: u16::from_le_bytes(input[4..6].try_into().unwrap()),
        num_chunks: u16::from_le_bytes(input[6..8].try_into().unwrap()),
    };
    let size = header.size as usize;
    if size < 8 || size != input.len() {
        return Err(SaveError::InvalidChunk(
            "size does not bound the chunk exactly",
        ));
    }
    let payload = &input[8..];
    if header.num_chunks == 0 {
        return Ok(ParsedChunk {
            header,
            data: payload,
            children: Vec::new(),
        });
    }
    let mut children = Vec::with_capacity(header.num_chunks as usize);
    let mut at = 0usize;
    for _ in 0..header.num_chunks {
        if payload.len().saturating_sub(at) < 8 {
            return Err(SaveError::InvalidChunk("truncated child header"));
        }
        let child_size = u32::from_le_bytes(payload[at..at + 4].try_into().unwrap()) as usize;
        if child_size < 8 || child_size > payload.len() - at {
            return Err(SaveError::InvalidChunk("child escapes parent"));
        }
        children.push(parse_chunk(&payload[at..at + child_size])?);
        at += child_size;
    }
    if at != payload.len() {
        return Err(SaveError::InvalidChunk(
            "parent has unclaimed trailing payload",
        ));
    }
    Ok(ParsedChunk {
        header,
        data: &[],
        children,
    })
}

#[derive(Default)]
struct Writer(Vec<u8>);

impl Writer {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn i8(&mut self, v: i8) {
        self.u8(v as u8);
    }
    fn bool(&mut self, v: bool) {
        self.u8(u8::from(v));
    }
    fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn i16(&mut self, v: i16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn i32(&mut self, v: i32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn bytes(&mut self, v: &[u8]) {
        self.0.extend_from_slice(v);
    }
    fn len(&mut self, n: usize, what: &'static str) -> Result<(), SaveError> {
        self.u32(u32::try_from(n).map_err(|_| SaveError::Limit(what))?);
        Ok(())
    }
}

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, at: 0 }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], SaveError> {
        let end = self
            .at
            .checked_add(n)
            .filter(|&end| end <= self.data.len())
            .ok_or(SaveError::Invalid("truncated payload"))?;
        let out = &self.data[self.at..end];
        self.at = end;
        Ok(out)
    }
    fn u8(&mut self) -> Result<u8, SaveError> {
        Ok(self.take(1)?[0])
    }
    fn i8(&mut self) -> Result<i8, SaveError> {
        Ok(self.u8()? as i8)
    }
    fn bool(&mut self) -> Result<bool, SaveError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(SaveError::Invalid("non-canonical bool")),
        }
    }
    fn u16(&mut self) -> Result<u16, SaveError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn i16(&mut self) -> Result<i16, SaveError> {
        Ok(i16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32, SaveError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn i32(&mut self) -> Result<i32, SaveError> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn len(&mut self, max: usize, what: &'static str) -> Result<usize, SaveError> {
        let n = self.u32()? as usize;
        if n > max {
            return Err(SaveError::Limit(what));
        }
        Ok(n)
    }
    fn finish(self) -> Result<(), SaveError> {
        if self.at == self.data.len() {
            Ok(())
        } else {
            Err(SaveError::Invalid("trailing payload bytes"))
        }
    }
}

fn write_order(w: &mut Writer, o: &Order) {
    w.u8(o.kind as u8);
    w.u8(o.flags);
    w.i32(o.x);
    w.i32(o.y);
    w.i8(o.target_who);
    w.i16(o.target_o);
    w.i32(o.tolerance);
}

fn read_order(r: &mut Reader<'_>) -> Result<Order, SaveError> {
    let kind = OrderIndex::from_index(r.u8()? as usize)
        .ok_or(SaveError::Invalid("unknown unit order index"))?;
    Ok(Order {
        kind,
        flags: r.u8()?,
        x: r.i32()?,
        y: r.i32()?,
        target_who: r.i8()?,
        target_o: r.i16()?,
        tolerance: r.i32()?,
    })
}

fn write_world_state(state: &WorldSaveState) -> Result<Vec<u8>, SaveError> {
    let mut w = Writer::default();
    w.u32(state.capacity);
    w.u32(state.live);
    for active in state.active_slots {
        w.bool(active);
    }
    w.len(state.handle_of_row.len(), "handle permutation")?;
    for &id in &state.handle_of_row {
        w.u32(id);
    }
    w.len(state.generation.len(), "handle generations")?;
    for &generation in &state.generation {
        w.u32(generation);
    }

    // Plane ids are generated from the PDB schema and frozen in the stream so a schema
    // regeneration cannot reinterpret old bytes as a different UnitData layout.
    w.u16(W4_PLANES as u16);
    w.u16(W2_PLANES as u16);
    w.u16(W1_PLANES as u16);
    w.u16(WF_PLANES as u16);
    for p in 0..W4_PLANES {
        for &v in state.units.w4_plane(p) {
            w.i32(v);
        }
    }
    for p in 0..W2_PLANES {
        for &v in state.units.w2_plane(p) {
            w.i16(v);
        }
    }
    for p in 0..W1_PLANES {
        for &v in state.units.w1_plane(p) {
            w.i8(v);
        }
    }
    for p in 0..WF_PLANES {
        for &v in state.units.wf_slice(p) {
            w.u32(v.to_bits());
        }
    }

    for values in [&state.unit_type_id, &state.move_step_x, &state.move_step_y] {
        w.len(values.len(), "unit side vector")?;
        for &v in values {
            w.i32(v);
        }
    }
    w.len(state.unit_orders.len(), "unit order lists")?;
    for list in &state.unit_orders {
        if list.len() > MAX_ORDERS_PER_UNIT {
            return Err(SaveError::Limit("orders per unit"));
        }
        w.len(list.len(), "orders per unit")?;
        for order in list.iter() {
            write_order(&mut w, order);
        }
    }
    Ok(w.0)
}

fn read_i32_vec(
    r: &mut Reader<'_>,
    expected: usize,
    what: &'static str,
) -> Result<Vec<i32>, SaveError> {
    if r.len(MAX_UNITS, what)? != expected {
        return Err(SaveError::Invalid(what));
    }
    (0..expected).map(|_| r.i32()).collect()
}

fn read_world_state(
    data: &[u8],
    frame: i32,
    seconds: i32,
    random_state: i32,
) -> Result<WorldSaveState, SaveError> {
    let mut r = Reader::new(data);
    let capacity = r.u32()?;
    let live = r.u32()?;
    let cap = capacity as usize;
    let n = live as usize;
    if cap > MAX_UNITS || n > cap {
        return Err(SaveError::Limit("world units"));
    }
    let mut active_slots = [false; crate::objects::OWNER_SLOTS];
    for active in &mut active_slots {
        *active = r.bool()?;
    }
    if r.len(MAX_UNITS, "handle permutation")? != cap {
        return Err(SaveError::Invalid("handle permutation length"));
    }
    let handle_of_row = (0..cap).map(|_| r.u32()).collect::<Result<Vec<_>, _>>()?;
    if r.len(MAX_UNITS, "handle generations")? != cap {
        return Err(SaveError::Invalid("handle generation length"));
    }
    let generation = (0..cap).map(|_| r.u32()).collect::<Result<Vec<_>, _>>()?;

    if r.u16()? as usize != W4_PLANES
        || r.u16()? as usize != W2_PLANES
        || r.u16()? as usize != W1_PLANES
        || r.u16()? as usize != WF_PLANES
    {
        return Err(SaveError::Invalid("unit column schema mismatch"));
    }
    let mut units = UnitCols::with_capacity(cap);
    for _ in 0..n {
        units
            .push_zeroed()
            .ok_or(SaveError::Invalid("unit column capacity"))?;
    }
    for p in 0..W4_PLANES {
        for v in units.w4_plane_mut(p) {
            *v = r.i32()?;
        }
    }
    for p in 0..W2_PLANES {
        for v in units.w2_plane_mut(p) {
            *v = r.i16()?;
        }
    }
    for p in 0..W1_PLANES {
        for v in units.w1_plane_mut(p) {
            *v = r.i8()?;
        }
    }
    for p in 0..WF_PLANES {
        for v in units.wf_slice_mut(p) {
            *v = f32::from_bits(r.u32()?);
        }
    }

    let unit_type_id = read_i32_vec(&mut r, n, "unit type vector length")?;
    let move_step_x = read_i32_vec(&mut r, n, "move-step-x vector length")?;
    let move_step_y = read_i32_vec(&mut r, n, "move-step-y vector length")?;
    if r.len(MAX_UNITS, "unit order lists")? != n {
        return Err(SaveError::Invalid("unit order-list length"));
    }
    let mut unit_orders = Vec::with_capacity(n);
    for _ in 0..n {
        let count = r.len(MAX_ORDERS_PER_UNIT, "orders per unit")?;
        let mut list = OrderList::new();
        for _ in 0..count {
            list.push(read_order(&mut r)?);
        }
        unit_orders.push(list);
    }
    r.finish()?;
    Ok(WorldSaveState {
        units,
        unit_orders,
        unit_type_id,
        move_step_x,
        move_step_y,
        handle_of_row,
        generation,
        active_slots,
        live,
        capacity,
        frame,
        seconds,
        random_state,
    })
}

fn write_paths(sim: &Sim) -> Result<Vec<u8>, SaveError> {
    let live = sim.world.live_count() as usize;
    if sim.unit_type.len() != live
        || sim.paths.len() != live
        || sim.path_unit.len() != live
        || sim.crash_units.len() != live
    {
        return Err(SaveError::Invalid("unit side-store lengths"));
    }
    if sim.crash_units.iter().any(Option::is_some) {
        return Err(SaveError::Unsupported("crash unit sources"));
    }
    let state = sim.world.export_save_state()?;
    if sim.unit_type != state.unit_type_id {
        return Err(SaveError::Invalid("Sim and World unit type ids disagree"));
    }
    let mut w = Writer::default();
    w.len(live, "path unit count")?;
    for row in 0..live {
        w.i32(sim.unit_type[row]);
        let unit = sim.path_unit[row];
        w.i32(unit.type_size);
        w.bool(unit.can_board_transport);
        w.bool(unit.small_footprint);
        w.bool(unit.can_transport);
        if sim.paths[row].records.len() > MAX_PATH_RECORDS {
            return Err(SaveError::Limit("path records"));
        }
        w.len(sim.paths[row].records.len(), "path records")?;
        for p in &sim.paths[row].records {
            w.i32(p.to_x);
            w.i32(p.to_y);
            w.i32(p.tolerance);
            w.i32(p.flags);
        }
    }
    Ok(w.0)
}

fn read_paths(
    data: &[u8],
    expected_types: &[i32],
) -> Result<(Vec<i32>, Vec<movement::PathStack>, Vec<movement::PathUnit>), SaveError> {
    let mut r = Reader::new(data);
    let n = r.len(MAX_UNITS, "path unit count")?;
    if n != expected_types.len() {
        return Err(SaveError::Invalid("path unit count"));
    }
    let mut types = Vec::with_capacity(n);
    let mut paths = Vec::with_capacity(n);
    let mut path_units = Vec::with_capacity(n);
    for expected in expected_types {
        let type_id = r.i32()?;
        if type_id != *expected {
            return Err(SaveError::Invalid(
                "path type id disagrees with object chunk",
            ));
        }
        types.push(type_id);
        path_units.push(movement::PathUnit {
            type_size: r.i32()?,
            can_board_transport: r.bool()?,
            small_footprint: r.bool()?,
            can_transport: r.bool()?,
        });
        let count = r.len(MAX_PATH_RECORDS, "path records")?;
        let mut path = movement::PathStack::new();
        path.records.reserve(count);
        for _ in 0..count {
            path.push(movement::PathData {
                to_x: r.i32()?,
                to_y: r.i32()?,
                tolerance: r.i32()?,
                flags: r.i32()?,
            });
        }
        paths.push(path);
    }
    r.finish()?;
    Ok((types, paths, path_units))
}

fn write_walked_i32(w: &mut Writer, a: &map_terrain::WalkedArray<i32>) -> Result<(), SaveError> {
    if a.capacity < 0 || (a.capacity as usize) < a.items.len() {
        return Err(SaveError::Invalid("WalkedArray<i32> metadata"));
    }
    w.len(a.items.len(), "walked i32 array")?;
    w.i32(a.capacity);
    w.i16(a.increment);
    w.u8(a.flags);
    for &v in &a.items {
        w.i32(v);
    }
    Ok(())
}

fn read_walked_i32(
    r: &mut Reader<'_>,
    max: usize,
) -> Result<map_terrain::WalkedArray<i32>, SaveError> {
    let n = r.len(max, "walked i32 array")?;
    let capacity = r.i32()?;
    let increment = r.i16()?;
    let flags = r.u8()?;
    if capacity < 0 || (capacity as usize) < n {
        return Err(SaveError::Invalid("WalkedArray<i32> metadata"));
    }
    let items = (0..n).map(|_| r.i32()).collect::<Result<Vec<_>, _>>()?;
    Ok(map_terrain::WalkedArray {
        items,
        capacity,
        increment,
        flags,
    })
}

fn write_walked_coords(
    w: &mut Writer,
    a: &map_terrain::WalkedArray<(i32, i32)>,
) -> Result<(), SaveError> {
    if a.capacity < 0 || (a.capacity as usize) < a.items.len() {
        return Err(SaveError::Invalid("WalkedArray<WCoord> metadata"));
    }
    w.len(a.items.len(), "walked coordinate array")?;
    w.i32(a.capacity);
    w.i16(a.increment);
    w.u8(a.flags);
    for &(x, y) in &a.items {
        w.i32(x);
        w.i32(y);
    }
    Ok(())
}

fn read_walked_coords(
    r: &mut Reader<'_>,
    max: usize,
) -> Result<map_terrain::WalkedArray<(i32, i32)>, SaveError> {
    let n = r.len(max, "walked coordinate array")?;
    let capacity = r.i32()?;
    let increment = r.i16()?;
    let flags = r.u8()?;
    if capacity < 0 || (capacity as usize) < n {
        return Err(SaveError::Invalid("WalkedArray<WCoord> metadata"));
    }
    let items = (0..n)
        .map(|_| Ok((r.i32()?, r.i32()?)))
        .collect::<Result<Vec<_>, SaveError>>()?;
    Ok(map_terrain::WalkedArray {
        items,
        capacity,
        increment,
        flags,
    })
}

fn write_wdata(w: &mut Writer, cell: &map_terrain::WData) -> Result<(), SaveError> {
    w.u16(cell.flags);
    w.i8(cell.land);
    w.u8(cell.land_sub);
    w.i16(cell.region);
    w.i16(cell.region2);
    w.i16(cell.down);
    w.i16(cell.down_who);
    w.u8(cell.val);
    w.u8(cell.goods);
    w.u8(cell.light);
    w.i8(cell.who);
    w.i8(cell.who2);
    w.u8(cell.blocked);
    w.u8(cell.bad);
    w.i8(cell.solid);
    w.u8(cell.was_seen);
    w.bool(cell.block.is_some());
    if let Some(block) = &cell.block {
        if block.bits != 768 || block.size != 96 {
            return Err(SaveError::Invalid("collision block geometry"));
        }
        w.i32(block.bits);
        w.i32(block.size);
        w.i32(block.flags);
        w.bytes(&block.ptr);
    }
    Ok(())
}

fn read_wdata(r: &mut Reader<'_>) -> Result<map_terrain::WData, SaveError> {
    let mut cell = map_terrain::WData {
        flags: r.u16()?,
        land: r.i8()?,
        land_sub: r.u8()?,
        region: r.i16()?,
        region2: r.i16()?,
        down: r.i16()?,
        down_who: r.i16()?,
        val: r.u8()?,
        goods: r.u8()?,
        light: r.u8()?,
        who: r.i8()?,
        who2: r.i8()?,
        blocked: r.u8()?,
        bad: r.u8()?,
        solid: r.i8()?,
        was_seen: r.u8()?,
        block: None,
    };
    if r.bool()? {
        let bits = r.i32()?;
        let size = r.i32()?;
        let flags = r.i32()?;
        if bits != 768 || size != 96 {
            return Err(SaveError::Invalid("collision block geometry"));
        }
        let mut ptr = [0u8; 96];
        ptr.copy_from_slice(r.take(96)?);
        cell.block = Some(Box::new(map_terrain::CollBlock {
            bits,
            size,
            flags,
            ptr,
        }));
    }
    Ok(cell)
}

fn territory_is_shipped(t: &borders_fog::TerritoryRules) -> bool {
    let d = borders_fog::TerritoryRules::default();
    t.mode == d.mode
        && t.fort_upgrade_terr == d.fort_upgrade_terr
        && t.temple_upgrade_terr == d.temple_upgrade_terr
        && t.civic_upgrade_terr == d.civic_upgrade_terr
        && t.capital_territory_bonus == d.capital_territory_bonus
        && t.city_level_territory_bonus == d.city_level_territory_bonus
        && t.fort_territory_multiplier == d.fort_territory_multiplier
        && t.city_territory_multiplier == d.city_territory_multiplier
        && t.territory_base == d.territory_base
        && t.territory_limit_base == d.territory_limit_base
        && t.territory_limit_civic == d.territory_limit_civic
        && t.territory_limit_city == d.territory_limit_city
        && t.territory_den == d.territory_den
        && t.territory_num == d.territory_num
        && t.colosseum_territory_bonus == d.colosseum_territory_bonus
        && t.colosseum_fort_borders == d.colosseum_fort_borders
        && t.tikal_temple_borders_pct == d.tikal_temple_borders_pct
        && t.tikal_temple_hp_pct == d.tikal_temple_hp_pct
        && t.eiffel_tower_territory_bonus == d.eiffel_tower_territory_bonus
        && t.roman_fort_borders == d.roman_fort_borders
        && t.gems_territory_bonus == d.gems_territory_bonus
        && t.russian_borders == d.russian_borders
        && t.russian_borders_per_age == d.russian_borders_per_age
        && t.ctw_missionaries_bonus == d.ctw_missionaries_bonus
}

fn write_map(sim: &Sim) -> Result<Vec<u8>, SaveError> {
    let map = &sim.map;
    let world = &map.world;
    let canonical_circle = borders_fog::CircleTable::build();
    if map.circle.x != canonical_circle.x
        || map.circle.y != canonical_circle.y
        || map.circle.radius != canonical_circle.radius
    {
        return Err(SaveError::Unsupported("modified circle table"));
    }
    if !territory_is_shipped(&map.territory) {
        return Err(SaveError::Unsupported("modified territory rules"));
    }
    if world.xs <= 0 || world.xs != world.ys || world.xs > u16::MAX as i32 {
        return Err(SaveError::Unsupported(
            "non-square or unbounded terrain world",
        ));
    }
    let mut w = Writer::default();
    for v in [
        world.xs,
        world.ys,
        world.size,
        world.fog_xs,
        world.fog_ys,
        world.fog_size,
        world.tile_xs,
        world.tile_ys,
        world.tile_size,
        world.reg_xs,
        world.reg_ys,
        world.reg_size,
        world.map,
        world.sea_map,
        world.player_territory_limit,
        world.player_territory_limit_civic,
        world.player_territory_limit_city,
        world.colonized_territory_limit,
        world.colonized_territory_limit_civic,
        world.colonized_territory_limit_city,
        world.player_reg,
        world.resource_reg,
        world.forest_size,
        world.mountain_size,
        world.rock_size,
        world.total_metal,
        world.total_oil,
        world.goodies,
        world.land_resources,
        world.sea_resources,
        world.land_size,
        world.seed,
    ] {
        w.i32(v);
    }
    for a in [
        &world.start_x,
        &world.start_y,
        &world.start_city_x,
        &world.start_city_y,
        &world.oil_x,
        &world.oil_y,
    ] {
        write_walked_i32(&mut w, a)?;
    }
    w.len(world.start_city_locs.len(), "start city mask")?;
    w.bytes(&world.start_city_locs);
    w.len(world.wdata.len(), "world cells")?;
    for cell in &world.wdata {
        write_wdata(&mut w, cell)?;
    }
    w.len(world.tdata.len(), "terrain tiles")?;
    for &v in &world.tdata {
        w.u16(v);
    }
    for danger in &world.danger {
        w.len(danger.len(), "danger plane")?;
        for &v in danger {
            w.i32(v);
        }
    }
    for plane in [&world.seen, &world.seen2, &world.seen3, &world.wcoord_seen] {
        w.len(plane.len(), "fog plane")?;
        w.bytes(plane);
    }
    write_walked_coords(&mut w, &world.terrain_sync.halfland_locs)?;
    write_walked_i32(&mut w, &world.terrain_sync.halfland_types)?;
    write_walked_i32(&mut w, &world.terrain_sync.halfland_subtypes)?;
    write_walked_i32(&mut w, &world.terrain_sync.nuke_hits)?;

    for leader in &map.fog.leaders {
        w.bool(leader.see_all);
        w.bool(leader.explored_all);
        w.bool(leader.see_own_territory);
        w.i16(leader.reveal_counter);
        w.u8(leader.player_mask);
    }
    w.u8(map.fog.option.0);
    w.len(map.regions.len(), "border regions")?;
    for region in &map.regions {
        if region.size < 0
            || region.size as usize != region.coords.len()
            || region.borders < 0
            || region.borders > region.size
        {
            return Err(SaveError::Invalid("border region cursor"));
        }
        w.u32(region.flags);
        w.i32(region.size);
        w.i32(region.borders);
        w.len(region.coords.len(), "border region coordinates")?;
        for &(x, y) in &region.coords {
            w.i32(x);
            w.i32(y);
        }
    }
    for (slot, sources) in map.border_sources.iter().enumerate() {
        w.len(sources.len(), "border sources")?;
        for source in sources {
            if source.slot != slot as i32 {
                return Err(SaveError::Invalid("border source slot"));
            }
            match source.kind {
                borders_fog::BorderSourceKind::City {
                    level,
                    has_temple,
                    capital,
                } => {
                    w.u8(0);
                    w.i32(level);
                    w.bool(has_temple);
                    w.bool(capital);
                }
                borders_fog::BorderSourceKind::Fort { upgraded } => {
                    w.u8(1);
                    w.bool(upgraded);
                }
            }
            w.i32(source.slot);
            w.i32(source.who);
            w.i32(source.tile_x);
            w.i32(source.tile_y);
            w.bool(source.alive);
        }
    }
    Ok(w.0)
}

const MAX_MAP_CELLS: usize = 1 << 20;
const MAX_TILE_CELLS: usize = MAX_MAP_CELLS * 16;
const MAX_FOG_CELLS: usize = MAX_MAP_CELLS * 4;

fn read_exact_bytes(
    r: &mut Reader<'_>,
    expected: usize,
    max: usize,
    what: &'static str,
) -> Result<Vec<u8>, SaveError> {
    if r.len(max, what)? != expected {
        return Err(SaveError::Invalid(what));
    }
    Ok(r.take(expected)?.to_vec())
}

fn read_map(data: &[u8]) -> Result<crate::tick::MapState, SaveError> {
    let mut r = Reader::new(data);
    let mut scalar = [0i32; 32];
    for v in &mut scalar {
        *v = r.i32()?;
    }
    let [xs, ys, size, fog_xs, fog_ys, fog_size, tile_xs, tile_ys, tile_size, reg_xs, reg_ys, reg_size, map, sea_map, player_territory_limit, player_territory_limit_civic, player_territory_limit_city, colonized_territory_limit, colonized_territory_limit_civic, colonized_territory_limit_city, player_reg, resource_reg, forest_size, mountain_size, rock_size, total_metal, total_oil, goodies, land_resources, sea_resources, land_size, seed] =
        scalar;
    if xs <= 0 || xs != ys || xs > u16::MAX as i32 {
        return Err(SaveError::Invalid("terrain dimensions"));
    }
    let derived_size = xs
        .checked_mul(ys)
        .ok_or(SaveError::Invalid("terrain dimension overflow"))?;
    let derived_tile_xs = xs
        .checked_mul(4)
        .ok_or(SaveError::Invalid("terrain dimension overflow"))?;
    let derived_tile_ys = ys
        .checked_mul(4)
        .ok_or(SaveError::Invalid("terrain dimension overflow"))?;
    let derived_tile_size = derived_tile_xs
        .checked_mul(derived_tile_ys)
        .ok_or(SaveError::Invalid("terrain dimension overflow"))?;
    let derived_fog_xs = derived_tile_xs
        .checked_mul(2)
        .ok_or(SaveError::Invalid("terrain dimension overflow"))?
        / 4;
    let derived_fog_ys = derived_tile_ys
        .checked_mul(2)
        .ok_or(SaveError::Invalid("terrain dimension overflow"))?
        / 4;
    let derived_fog_size = derived_fog_xs
        .checked_mul(derived_fog_ys)
        .ok_or(SaveError::Invalid("terrain dimension overflow"))?;
    let derived_reg_xs = derived_tile_xs / 8;
    let derived_reg_ys = derived_tile_ys / 8;
    let derived_reg_size = derived_reg_xs
        .checked_mul(derived_reg_ys)
        .ok_or(SaveError::Invalid("terrain dimension overflow"))?;
    if [
        size, fog_xs, fog_ys, fog_size, tile_xs, tile_ys, tile_size, reg_xs, reg_ys, reg_size,
    ] != [
        derived_size,
        derived_fog_xs,
        derived_fog_ys,
        derived_fog_size,
        derived_tile_xs,
        derived_tile_ys,
        derived_tile_size,
        derived_reg_xs,
        derived_reg_ys,
        derived_reg_size,
    ] {
        return Err(SaveError::Invalid("derived terrain dimensions"));
    }
    let cells = usize::try_from(size).map_err(|_| SaveError::Invalid("terrain size"))?;
    let tiles = usize::try_from(tile_size).map_err(|_| SaveError::Invalid("tile size"))?;
    let fog_cells = usize::try_from(fog_size).map_err(|_| SaveError::Invalid("fog size"))?;
    let reg_cells = usize::try_from(reg_size).map_err(|_| SaveError::Invalid("region size"))?;
    if cells > MAX_MAP_CELLS || tiles > MAX_TILE_CELLS || fog_cells > MAX_FOG_CELLS {
        return Err(SaveError::Limit("terrain dimensions"));
    }
    let expected = map_terrain::World::init(
        xs,
        ys,
        player_territory_limit,
        player_territory_limit_civic,
        player_territory_limit_city,
    );
    debug_assert_eq!(expected.size, size);

    let start_x = read_walked_i32(&mut r, cells)?;
    let start_y = read_walked_i32(&mut r, cells)?;
    let start_city_x = read_walked_i32(&mut r, cells)?;
    let start_city_y = read_walked_i32(&mut r, cells)?;
    let oil_x = read_walked_i32(&mut r, cells)?;
    let oil_y = read_walked_i32(&mut r, cells)?;
    for (a, b) in [
        (&start_x, &start_y),
        (&start_city_x, &start_city_y),
        (&oil_x, &oil_y),
    ] {
        if a.items.len() != b.items.len() {
            return Err(SaveError::Invalid("coordinate plane length"));
        }
    }
    let start_city_locs = read_exact_bytes(
        &mut r,
        cells.saturating_add(7) / 8,
        MAX_MAP_CELLS / 8 + 1,
        "start city mask length",
    )?;
    if r.len(MAX_MAP_CELLS, "world cell count")? != cells {
        return Err(SaveError::Invalid("world cell count"));
    }
    let wdata = (0..cells)
        .map(|_| read_wdata(&mut r))
        .collect::<Result<Vec<_>, _>>()?;
    if r.len(MAX_TILE_CELLS, "terrain tile count")? != tiles {
        return Err(SaveError::Invalid("terrain tile count"));
    }
    let tdata = (0..tiles).map(|_| r.u16()).collect::<Result<Vec<_>, _>>()?;
    let mut danger: [Vec<i32>; 8] = std::array::from_fn(|_| Vec::new());
    for plane in &mut danger {
        if r.len(MAX_MAP_CELLS, "danger plane length")? != reg_cells {
            return Err(SaveError::Invalid("danger plane length"));
        }
        *plane = (0..reg_cells)
            .map(|_| r.i32())
            .collect::<Result<Vec<_>, _>>()?;
    }
    let seen = read_exact_bytes(&mut r, fog_cells, MAX_FOG_CELLS, "seen plane length")?;
    let seen2 = read_exact_bytes(&mut r, fog_cells, MAX_FOG_CELLS, "seen2 plane length")?;
    let seen3 = read_exact_bytes(&mut r, fog_cells, MAX_FOG_CELLS, "seen3 plane length")?;
    let wcoord_seen = read_exact_bytes(&mut r, cells, MAX_MAP_CELLS, "wcoord_seen plane length")?;
    let terrain_sync = map_terrain::TerrainSync {
        halfland_locs: read_walked_coords(&mut r, MAX_TILE_CELLS)?,
        halfland_types: read_walked_i32(&mut r, MAX_TILE_CELLS)?,
        halfland_subtypes: read_walked_i32(&mut r, MAX_TILE_CELLS)?,
        nuke_hits: read_walked_i32(&mut r, MAX_TILE_CELLS)?,
    };

    let mut fog = borders_fog::Fog::new();
    for leader in &mut fog.leaders {
        leader.see_all = r.bool()?;
        leader.explored_all = r.bool()?;
        leader.see_own_territory = r.bool()?;
        leader.reveal_counter = r.i16()?;
        leader.player_mask = r.u8()?;
    }
    fog.option = borders_fog::FogOption(r.u8()?);
    let region_count = r.len(MAX_MAP_CELLS, "border region count")?;
    let mut regions = Vec::with_capacity(region_count);
    for _ in 0..region_count {
        let flags = r.u32()?;
        let region_size = r.i32()?;
        let borders = r.i32()?;
        if region_size < 0 || borders < 0 || borders > region_size {
            return Err(SaveError::Invalid("border region cursor"));
        }
        let coord_count = r.len(MAX_MAP_CELLS, "border region coordinate count")?;
        if coord_count != region_size as usize {
            return Err(SaveError::Invalid("border region size"));
        }
        let mut coords = Vec::with_capacity(coord_count);
        for _ in 0..coord_count {
            let x = r.i32()?;
            let y = r.i32()?;
            if x < 0 || y < 0 || x >= xs || y >= ys {
                return Err(SaveError::Invalid("border region coordinate"));
            }
            coords.push((x, y));
        }
        regions.push(borders_fog::RegionBorderState {
            flags,
            size: region_size,
            borders,
            coords,
        });
    }
    let mut border_sources: [Vec<borders_fog::BorderSource>; NUM_LEADERS] =
        std::array::from_fn(|_| Vec::new());
    for (slot, sources) in border_sources.iter_mut().enumerate() {
        let count = r.len(MAX_MAP_CELLS, "border source count")?;
        sources.reserve(count);
        for _ in 0..count {
            let kind = match r.u8()? {
                0 => borders_fog::BorderSourceKind::City {
                    level: r.i32()?,
                    has_temple: r.bool()?,
                    capital: r.bool()?,
                },
                1 => borders_fog::BorderSourceKind::Fort {
                    upgraded: r.bool()?,
                },
                _ => return Err(SaveError::Invalid("border source kind")),
            };
            let source_slot = r.i32()?;
            let who = r.i32()?;
            let tile_x = r.i32()?;
            let tile_y = r.i32()?;
            let alive = r.bool()?;
            if source_slot != slot as i32
                || tile_x < 0
                || tile_y < 0
                || tile_x >= tile_xs
                || tile_y >= tile_ys
            {
                return Err(SaveError::Invalid("border source coordinates/slot"));
            }
            sources.push(borders_fog::BorderSource {
                kind,
                slot: source_slot,
                who,
                tile_x,
                tile_y,
                alive,
            });
        }
    }
    r.finish()?;

    let world = map_terrain::World {
        xs,
        ys,
        size,
        fog_xs,
        fog_ys,
        fog_size,
        tile_xs,
        tile_ys,
        tile_size,
        reg_xs,
        reg_ys,
        reg_size,
        map,
        sea_map,
        player_territory_limit,
        player_territory_limit_civic,
        player_territory_limit_city,
        colonized_territory_limit,
        colonized_territory_limit_civic,
        colonized_territory_limit_city,
        player_reg,
        resource_reg,
        forest_size,
        mountain_size,
        rock_size,
        total_metal,
        total_oil,
        goodies,
        land_resources,
        sea_resources,
        land_size,
        seed,
        start_x,
        start_y,
        start_city_x,
        start_city_y,
        oil_x,
        oil_y,
        start_city_locs,
        wdata,
        tdata,
        danger,
        seen,
        seen2,
        seen3,
        wcoord_seen,
        terrain_sync,
    };
    Ok(crate::tick::MapState {
        world,
        fog,
        circle: borders_fog::CircleTable::build(),
        territory: borders_fog::TerritoryRules::default(),
        regions,
        border_sources,
    })
}

fn resource_bonus_is_default(g: &economy::ResourceBonusGates) -> bool {
    let d = economy::ResourceBonusGates::default();
    g.russian == d.russian
        && g.pyramids == d.pyramids
        && g.colossus == d.colossus
        && g.hanging_gardens == d.hanging_gardens
        && g.angkor == d.angkor
        && g.taj == d.taj
        && g.eiffel == d.eiffel
        && g.tikal == d.tikal
        && g.global_prosperity == d.global_prosperity
        && g.ctw_stacks == d.ctw_stacks
}

fn gather_inputs_are_default(g: &economy::GatherInputs) -> bool {
    let d = economy::GatherInputs::default();
    g.object_income == d.object_income
        && g.lakota_units == d.lakota_units
        && g.americans_buildings == d.americans_buildings
        && g.refineries == d.refineries
        && g.capitalism == d.capitalism
        && g.inca == d.inca
        && g.rares == d.rares
        && g.rare_yields == d.rare_yields
        && g.rare_ctx == d.rare_ctx
        && g.coffee == d.coffee
        && g.taxation_level == d.taxation_level
        && g.british == d.british
        && g.ctw_missionaries == d.ctw_missionaries
        && g.territory_tiles == d.territory_tiles
        && g.total_land_tiles == d.total_land_tiles
        && g.mongol == d.mongol
        && g.mongol_game_term == d.mongol_game_term
        && resource_bonus_is_default(&g.bonus_gates)
        && g.type_avail == d.type_avail
        && g.has_preq == d.has_preq
        && g.substitution == d.substitution
}

fn border_input_is_default(b: &borders_fog::LeaderBorderInput) -> bool {
    let d = borders_fog::LeaderBorderInput::default();
    b.active == d.active
        && b.temple_upgrade == d.temple_upgrade
        && b.fort_upgrade == d.fort_upgrade
        && b.gov_level == d.gov_level
        && b.age == d.age
        && b.rare_gems == d.rare_gems
        && b.wonder_colosseum == d.wonder_colosseum
        && b.wonder_tikal == d.wonder_tikal
        && b.wonder_eiffel == d.wonder_eiffel
        && b.tribe_roman == d.tribe_roman
        && b.tribe_russian == d.tribe_russian
        && b.ctw_missionaries == d.ctw_missionaries
        && b.ctw_raw_bonus == d.ctw_raw_bonus
}

fn leader_is_supported(l: &LeaderSlot) -> bool {
    !l.active
        && gather_inputs_are_default(&l.gather_inputs)
        && l.cap_gates == economy::CapGates::default()
        && l.gather_ctx == economy::DoGatherContext::default()
        && border_input_is_default(&l.border)
}

fn step8_leader_is_new(actual: &step8::Leader, expected: &step8::Leader) -> bool {
    actual.flags == expected.flags
        && actual.slot == expected.slot
        && actual.diplo == expected.diplo
        && actual.taunt_kind == expected.taunt_kind
        && actual.taunt_arg == expected.taunt_arg
        && actual.taunt_frame == expected.taunt_frame
        && actual.timers == expected.timers
        && actual.retake_scale == expected.retake_scale
        && actual.pop_cap == expected.pop_cap
        && actual.pop_issues == expected.pop_issues
        && actual.frame_counter_b == expected.frame_counter_b
        && actual.attrition == expected.attrition
        && actual.anti_attrition.to_bits() == expected.anti_attrition.to_bits()
        && actual.attrition_off == expected.attrition_off
        && actual.anti_attrition_off == expected.anti_attrition_off
        && actual.explored == expected.explored
        && actual.conquest_byte == expected.conquest_byte
        && actual.rare_effective == expected.rare_effective
        && actual.rare_a == expected.rare_a
        && actual.rare_b == expected.rare_b
        && actual.econ == expected.econ
        && actual.last_calc_frame == expected.last_calc_frame
        && actual.econ_dirty == expected.econ_dirty
        && actual.unit_stats == expected.unit_stats
}

fn step8_state_is_pristine(sim: &Sim) -> bool {
    let expected = step8::Leaders::new();
    if !sim
        .step8
        .leaders
        .iter()
        .zip(expected.leaders.iter())
        .all(|(actual, expected)| step8_leader_is_new(actual, expected))
    {
        return false;
    }
    if sim.step8_rules != step8::Step8Rules::shipped() {
        return false;
    }
    sim.step8_env.leaders.iter().all(|env| {
        gather_inputs_are_default(&env.gather)
            && env.caps == economy::CapGates::default()
            && env.payout == economy::DoGatherContext::default()
            && env.attrition == step8::AttritionGates::default()
            && env.objects.band_2000.is_empty()
            && env.objects.band_3000.is_empty()
            && env.objects.units.is_empty()
    })
}

fn write_econ(w: &mut Writer, e: &economy::LeaderEcon) {
    for values in [
        &e.stockpile,
        &e.accumulator,
        &e.commerce_cap,
        &e.capped_flag,
        &e.gross,
        &e.expense,
        &e.displayed,
        &e.breakdown,
    ] {
        for &v in values {
            w.i32(v);
        }
    }
    w.i32(e.age_alt);
    w.i32(e.age);
}

fn read_econ(r: &mut Reader<'_>) -> Result<economy::LeaderEcon, SaveError> {
    let mut arrays = [[0i32; economy::NUM_RESOURCES]; 8];
    for values in &mut arrays {
        for v in values {
            *v = r.i32()?;
        }
    }
    Ok(economy::LeaderEcon {
        stockpile: arrays[0],
        accumulator: arrays[1],
        commerce_cap: arrays[2],
        capped_flag: arrays[3],
        gross: arrays[4],
        expense: arrays[5],
        displayed: arrays[6],
        breakdown: arrays[7],
        age_alt: r.i32()?,
        age: r.i32()?,
    })
}

fn write_leaders(sim: &Sim) -> Result<Vec<u8>, SaveError> {
    if sim.leaders.iter().any(|l| !leader_is_supported(l)) {
        return Err(SaveError::Unsupported(
            "active leaders or non-default leader input hosts",
        ));
    }
    let mut w = Writer::default();
    w.u8(NUM_LEADERS as u8);
    for leader in &sim.leaders {
        write_econ(&mut w, &leader.econ);
        w.i32(leader.last_calc_frame);
        w.bool(leader.dirty);
    }
    w.i32(sim.market.cycle);
    for values in [
        &sim.market.base_price,
        &sim.market.spread,
        &sim.market.trend_target,
        &sim.market.trend_step,
        &sim.market.trend_countdown,
    ] {
        for &v in values {
            w.i32(v);
        }
    }
    Ok(w.0)
}

fn read_leaders(
    data: &[u8],
) -> Result<([LeaderSlot; NUM_LEADERS], economy::MarketState), SaveError> {
    let mut r = Reader::new(data);
    if r.u8()? as usize != NUM_LEADERS {
        return Err(SaveError::Invalid("leader slot count"));
    }
    let mut leaders: [LeaderSlot; NUM_LEADERS] = Default::default();
    for leader in &mut leaders {
        leader.econ = read_econ(&mut r)?;
        leader.last_calc_frame = r.i32()?;
        leader.dirty = r.bool()?;
    }
    let mut market = economy::MarketState {
        cycle: r.i32()?,
        ..Default::default()
    };
    for values in [
        &mut market.base_price,
        &mut market.spread,
        &mut market.trend_target,
        &mut market.trend_step,
        &mut market.trend_countdown,
    ] {
        for v in values {
            *v = r.i32()?;
        }
    }
    r.finish()?;
    Ok((leaders, market))
}

fn write_core(sim: &Sim) -> Vec<u8> {
    let mut w = Writer::default();
    w.u32(FORMAT_VERSION);
    w.i32(sim.map.world.seed);
    w.i32(sim.world.frame);
    w.i32(sim.world.seconds);
    w.i32(sim.world.random.state());
    w.0
}

fn read_core(data: &[u8]) -> Result<(i32, i32, i32, i32), SaveError> {
    let mut r = Reader::new(data);
    if r.u32()? != FORMAT_VERSION {
        return Err(SaveError::Invalid("unsupported save format version"));
    }
    let out = (r.i32()?, r.i32()?, r.i32()?, r.i32()?);
    r.finish()?;
    Ok(out)
}

fn reject_unsupported(sim: &Sim) -> Result<(), SaveError> {
    if !step8_state_is_pristine(sim) {
        return Err(SaveError::Unsupported("step-8 leader state/hosts"));
    }
    if sim.cannon_time != crate::tick::CannonTimeState::default() {
        return Err(SaveError::Unsupported("cannon-time state"));
    }
    if sim.combat_rules != crate::systems::combat::CombatConstants::shipped() {
        return Err(SaveError::Unsupported("modified combat rules"));
    }
    if !sim.builds.is_empty() {
        return Err(SaveError::Unsupported("buildings"));
    }
    if !sim.walls.is_empty() {
        return Err(SaveError::Unsupported("walls"));
    }
    if !sim.herds.is_empty() {
        return Err(SaveError::Unsupported("herds"));
    }
    if !sim.shooter_rules.is_empty() {
        return Err(SaveError::Unsupported("projectile type rules"));
    }
    if sim.ammo.ammo_index != 0 || sim.ammo.slots.iter().any(|a| a.occupied()) {
        return Err(SaveError::Unsupported("live or consumed projectile pool"));
    }
    if sim.deaths.slots.iter().any(|d| d.valid != 0) {
        return Err(SaveError::Unsupported("live death records"));
    }
    if !sim.crash_type_rules.is_empty() || sim.crash_env.is_some() {
        return Err(SaveError::Unsupported("aircraft crash hosts"));
    }
    if sim.wonder_world.is_some() || sim.wonder_error.is_some() {
        return Err(SaveError::Unsupported("Wonder external world/error"));
    }
    for who in 0..NUM_LEADERS {
        if sim.wonders.wonder_mark(who) != 0
            || sim.wonders.wonders_built(who) != 0
            || sim.wonders.wonders_held(who) != 0
            || !sim.wonders.unbuilt(who).is_empty()
        {
            return Err(SaveError::Unsupported("Wonder lifecycle"));
        }
    }
    let default_groups = crate::systems::groups_guys::Groups::default();
    if sim.groups.list != default_groups.list
        || sim.groups.last_group != default_groups.last_group
        || sim.groups.proc_group != default_groups.proc_group
    {
        return Err(SaveError::Unsupported("groups"));
    }
    if sim.world.item_runtime.is_some()
        || sim.world.rules.balance.is_some()
        || !sim.world.rules.unit_stats.is_empty()
        || sim.world.rules.combat != crate::mechanics::CombatRules::default()
    {
        return Err(SaveError::Unsupported("installed World rules/items"));
    }
    if sim.econ_rules != economy::EconRules::shipped() {
        return Err(SaveError::Unsupported("modified economy rules"));
    }
    Ok(())
}

/// Serialize the currently supported authoritative Sim tranche.
///
/// Unsupported live subsystems return an error before any bytes are returned. The output
/// is deterministic: saving the same state twice, or loading and immediately resaving it,
/// yields identical bytes including every chunk size/count and dynamic-array metadata.
pub fn save_sim(sim: &Sim) -> Result<Vec<u8>, SaveError> {
    reject_unsupported(sim)?;
    let state = sim.world.export_save_state()?;
    let map = write_map(sim)?;
    // Validate the producer through the same bounded decoder used for untrusted input.
    // This catches an internally inconsistent public MapState before bytes escape.
    let _ = read_map(&map)?;
    let root = Chunk::branch(
        ROOT,
        vec![
            Chunk::leaf(CORE, write_core(sim)),
            Chunk::leaf(MAP, map),
            Chunk::leaf(OBJECTS, write_world_state(&state)?),
            Chunk::leaf(LEADERS, write_leaders(sim)?),
            Chunk::leaf(PATHS, write_paths(sim)?),
        ],
    )
    .encode()?;
    let total = MAGIC
        .len()
        .checked_add(root.len())
        .ok_or(SaveError::Limit("save size"))?;
    if total > MAX_SAVE_BYTES {
        return Err(SaveError::Limit("save size"));
    }
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&root);
    Ok(out)
}

/// Load a supported Sim save. No caller-owned state is mutated on failure.
pub fn load_sim(bytes: &[u8]) -> Result<Sim, SaveError> {
    if bytes.len() > MAX_SAVE_BYTES {
        return Err(SaveError::Limit("save size"));
    }
    if bytes.len() < MAGIC.len() + 8 || &bytes[..MAGIC.len()] != MAGIC {
        return Err(SaveError::Invalid("save magic"));
    }
    let root = parse_chunk(&bytes[MAGIC.len()..])?;
    if root.header.id != ROOT || root.header.num_chunks as usize != REQUIRED.len() {
        return Err(SaveError::InvalidChunk("root id/child count"));
    }
    let mut sections: [Option<&[u8]>; REQUIRED.len()] = [None; REQUIRED.len()];
    for child in &root.children {
        if child.header.num_chunks != 0 {
            return Err(SaveError::InvalidChunk("top-level section is not a leaf"));
        }
        let Some(index) = REQUIRED.iter().position(|&id| id == child.header.id) else {
            return Err(SaveError::UnknownChunk(child.header.id));
        };
        if sections[index].replace(child.data).is_some() {
            return Err(SaveError::DuplicateChunk(child.header.id));
        }
    }
    for (index, section) in sections.iter().enumerate() {
        if section.is_none() {
            return Err(SaveError::MissingChunk(REQUIRED[index]));
        }
    }
    let [core, map, objects, leaders, paths] = sections.map(Option::unwrap);
    let (seed, frame, seconds, random_state) = read_core(core)?;
    let map = read_map(map)?;
    if map.world.seed != seed {
        return Err(SaveError::Invalid("core/map seed mismatch"));
    }
    let world_state = read_world_state(objects, frame, seconds, random_state)?;
    let expected_types = world_state.unit_type_id.clone();
    let (leaders, market) = read_leaders(leaders)?;
    let (unit_type, paths, path_unit) = read_paths(paths, &expected_types)?;

    let mut sim = Sim::new(seed as u32 as u64, map.world.xs as u16);
    sim.map = map;
    sim.world.import_save_state(world_state)?;
    sim.leaders = leaders;
    sim.market = market;
    sim.unit_type = unit_type;
    sim.paths = paths;
    sim.path_unit = path_unit;
    sim.crash_units = vec![None; sim.world.live_count() as usize];
    // A loaded state must itself be saveable. This catches accidental constructor state
    // that would otherwise make the first post-load save fail or silently differ.
    reject_unsupported(&sim)?;
    Ok(sim)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::order::OrderIndex;

    fn supported_sim() -> Sim {
        let mut sim = Sim::new(0x1234_5678, 4);
        sim.map.world.seed = 0x1234_5678;
        sim.world.frame = 7;
        sim.world.seconds = 2;
        sim.world.random.reseed(0x7654_3210);

        let a = sim.spawn_unit(2, 17, 768, 1536, 4).unwrap();
        let b = sim.spawn_unit(3, 19, 2304, 1536, 5).unwrap();
        let ar = sim.world.row_of(a).unwrap();
        let br = sim.world.row_of(b).unwrap();
        sim.world.orders_mut(ar).push(Order {
            kind: OrderIndex::Think,
            flags: 0xa5,
            x: 1001,
            y: -202,
            target_who: 3,
            target_o: 0,
            tolerance: 77,
        });
        sim.world.orders_mut(ar).push(Order {
            kind: OrderIndex::Patrol,
            ..Order::default()
        });
        sim.world.orders_mut(br).push(Order {
            kind: OrderIndex::Guard,
            target_who: 2,
            target_o: 0,
            ..Order::default()
        });
        sim.paths[ar].push(movement::PathData {
            to_x: 1500,
            to_y: 1600,
            tolerance: 11,
            flags: movement::PathData::FLAG_WAYPOINT,
        });
        sim.path_unit[ar] = movement::PathUnit {
            type_size: 3,
            can_board_transport: true,
            small_footprint: false,
            can_transport: false,
        };

        sim.leaders[0].econ.stockpile = [11, 22, 33, 44, 55, 66];
        sim.leaders[0].econ.accumulator = [-7, 8, -9, 10, -11, 12];
        sim.leaders[0].last_calc_frame = -23;
        sim.leaders[0].dirty = true;
        sim.market.cycle = 9;
        sim.market.base_price = [1, 2, 3, 4, 5, 6];
        sim.market.spread = [-1, -2, -3, -4, -5, -6];

        sim.map.world.start_x = map_terrain::WalkedArray {
            items: vec![1, 3],
            capacity: 5,
            increment: -1,
            flags: 0x51,
        };
        sim.map.world.start_y = map_terrain::WalkedArray {
            items: vec![2, 4],
            capacity: 7,
            increment: 3,
            flags: 0x82,
        };
        sim.map.world.wdata[3].goods = 17;
        sim.map.world.wdata[3].who = 2;
        let mut block = map_terrain::CollBlock::default();
        block.flags = 2;
        block.ptr[11] = 0x80;
        sim.map.world.wdata[3].block = Some(Box::new(block));
        sim.map.world.tdata[5] = 0x8042;
        sim.map.world.seen[1] = 4;
        sim.map.world.seen2[1] = 12;
        sim.map.world.wcoord_seen[0] = 4;
        sim.map.world.terrain_sync.nuke_hits = map_terrain::WalkedArray {
            items: vec![99, -1],
            capacity: 4,
            increment: 2,
            flags: 3,
        };
        sim.map.fog.leaders[2].player_mask = 4;
        sim.map.fog.leaders[2].reveal_counter = 3;
        sim
    }

    fn section_offset(bytes: &[u8], id: u16) -> usize {
        let root = MAGIC.len();
        let count = u16::from_le_bytes(bytes[root + 6..root + 8].try_into().unwrap());
        let mut at = root + 8;
        for _ in 0..count {
            let size = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
            let got = u16::from_le_bytes(bytes[at + 4..at + 6].try_into().unwrap());
            if got == id {
                return at;
            }
            at += size;
        }
        panic!("section 0x{id:04x} not found")
    }

    fn load_error(bytes: &[u8]) -> SaveError {
        match load_sim(bytes) {
            Ok(_) => panic!("corrupt save unexpectedly loaded"),
            Err(error) => error,
        }
    }

    #[test]
    fn chunk_headers_use_retail_size_and_child_count_semantics() {
        let bytes = save_sim(&supported_sim()).unwrap();
        let root = MAGIC.len();
        assert_eq!(
            u32::from_le_bytes(bytes[root..root + 4].try_into().unwrap()) as usize,
            bytes.len() - MAGIC.len()
        );
        assert_eq!(
            u16::from_le_bytes(bytes[root + 4..root + 6].try_into().unwrap()),
            ROOT
        );
        assert_eq!(
            u16::from_le_bytes(bytes[root + 6..root + 8].try_into().unwrap()),
            REQUIRED.len() as u16
        );
        let parsed = parse_chunk(&bytes[MAGIC.len()..]).unwrap();
        assert_eq!(parsed.children.len(), REQUIRED.len());
        assert_eq!(
            parsed
                .children
                .iter()
                .map(|c| c.header.size as usize)
                .sum::<usize>()
                + 8,
            parsed.header.size as usize
        );
    }

    #[test]
    fn save_load_resave_and_resume_are_deterministic() {
        let mut original = supported_sim();
        let before_digest = original.channel_digest();
        let bytes = save_sim(&original).unwrap();
        let mut loaded = load_sim(&bytes).unwrap();
        assert_eq!(loaded.channel_digest(), before_digest);
        assert_eq!(save_sim(&loaded).unwrap(), bytes);
        assert_eq!(loaded.world.random.state(), original.world.random.state());
        assert_eq!(loaded.map.world.checksum(), original.map.world.checksum());

        original.do_frame();
        loaded.do_frame();
        assert_eq!(loaded.channel_digest(), original.channel_digest());
        assert_eq!(loaded.world.random.state(), original.world.random.state());
        assert_eq!(loaded.world.frame, original.world.frame);
        for row in 0..original.world.live_count() as usize {
            assert_eq!(loaded.world.orders(row), original.world.orders(row));
        }
    }

    #[test]
    fn walked_array_metadata_survives_byte_for_byte() {
        let original = supported_sim();
        let bytes = save_sim(&original).unwrap();
        let loaded = load_sim(&bytes).unwrap();
        assert_eq!(loaded.map.world.start_x, original.map.world.start_x);
        assert_eq!(loaded.map.world.start_y, original.map.world.start_y);
        assert_eq!(
            loaded.map.world.terrain_sync.nuke_hits,
            original.map.world.terrain_sync.nuke_hits
        );
        assert_eq!(save_sim(&loaded).unwrap(), bytes);
    }

    #[test]
    fn inactive_owner_with_retained_units_roundtrips_without_reactivation() {
        let mut original = supported_sim();
        original.world.objects.set_active(2, false);
        let bytes = save_sim(&original).unwrap();
        let loaded = load_sim(&bytes).unwrap();
        assert!(!loaded.world.objects.is_active(2));
        assert_eq!(
            loaded
                .world
                .objects
                .slot(2)
                .band(crate::objects::Band::Unit),
            original
                .world
                .objects
                .slot(2)
                .band(crate::objects::Band::Unit)
        );
        assert_eq!(save_sim(&loaded).unwrap(), bytes);
    }

    #[test]
    fn unsupported_live_sections_are_refused_before_serialization() {
        let mut sim = supported_sim();
        sim.leaders[0].active = true;
        assert!(matches!(
            save_sim(&sim),
            Err(SaveError::Unsupported(
                "active leaders or non-default leader input hosts"
            ))
        ));

        let mut sim = supported_sim();
        sim.step8.leaders[0].activate();
        assert_eq!(
            save_sim(&sim),
            Err(SaveError::Unsupported("step-8 leader state/hosts"))
        );

        let mut sim = supported_sim();
        sim.map.world.start_x.capacity = 1;
        assert_eq!(
            save_sim(&sim),
            Err(SaveError::Invalid("WalkedArray<i32> metadata"))
        );
    }

    #[test]
    fn unknown_duplicate_trailing_and_escaping_chunks_are_rejected() {
        let bytes = save_sim(&supported_sim()).unwrap();
        let first = MAGIC.len() + 8;

        let mut unknown = bytes.clone();
        unknown[first + 4..first + 6].copy_from_slice(&0x7fffu16.to_le_bytes());
        assert_eq!(load_error(&unknown), SaveError::UnknownChunk(0x7fff));

        let mut duplicate = bytes.clone();
        let second = section_offset(&duplicate, MAP);
        duplicate[second + 4..second + 6].copy_from_slice(&CORE.to_le_bytes());
        assert_eq!(load_error(&duplicate), SaveError::DuplicateChunk(CORE));

        let mut trailing = bytes.clone();
        trailing.push(0);
        assert!(matches!(
            load_error(&trailing),
            SaveError::InvalidChunk("size does not bound the chunk exactly")
        ));

        let mut escapes = bytes.clone();
        escapes[first..first + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(
            load_error(&escapes),
            SaveError::InvalidChunk("child escapes parent")
        );
    }

    #[test]
    fn corrupt_handle_permutation_fails_closed() {
        let bytes = save_sim(&supported_sim()).unwrap();
        let objects = section_offset(&bytes, OBJECTS);
        // payload: capacity/live (8), active[10], handle length (4), then ids.
        let first_handle = objects + 8 + 8 + crate::objects::OWNER_SLOTS + 4;
        let mut duplicate = bytes.clone();
        let id = duplicate[first_handle..first_handle + 4].to_vec();
        duplicate[first_handle + 4..first_handle + 8].copy_from_slice(&id);
        assert!(matches!(load_error(&duplicate), SaveError::World(_)));

        // Mutating arbitrary payload must not affect the caller-owned source buffer, and a
        // rejected load cannot produce a partially initialized Sim.
        assert_eq!(save_sim(&supported_sim()).unwrap(), bytes);
    }

    #[test]
    fn unknown_order_tags_are_rejected() {
        let mut bytes = vec![0xff, 0];
        bytes.extend_from_slice(&[0; 4 + 4 + 1 + 2 + 4]);
        let mut reader = Reader::new(&bytes);
        assert_eq!(
            read_order(&mut reader),
            Err(SaveError::Invalid("unknown unit order index"))
        );
    }
}
