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
use crate::item_runtime::{
    validate_absent_items_map, ItemRuntime, ItemRuntimeSaveError, ItemRuntimeSaveState,
};
use crate::order::{
    FollowOrderPayload, FormOrderState, Order, OrderIndex, OrderList, SpecialAnimOrderState,
    SpecialAnimType,
};
use crate::systems::{
    borders_fog, economy, game_daemon_step12, groups_guys, items::Item, map_terrain, movement,
    production,
};
use crate::tick::{LeaderSlot, Sim, NUM_LEADERS};
use crate::world::{WorldSaveError, WorldSaveState, MAX_UNITS};

mod step8_views;

const MAGIC: &[u8; 8] = b"DoNSave\0";
const FORMAT_VERSION: u32 = 6;
const MAX_SAVE_BYTES: usize = 256 * 1024 * 1024;
const MAX_ORDERS_PER_UNIT: usize = 1024;
const MAX_PATH_RECORDS: usize = 1 << 20;
const MAX_ITEM_SLOTS: usize = i16::MAX as usize + 1;
const MAX_BUILDS: usize = crate::objects::BANDED_SLOTS * production::BUILD_POOL_SLOTS;
const MAX_BUILD_QUEUE_ENTRIES: usize = 4096;
const MAX_BUILD_MINING_TILES: usize = 1 << 20;
const MAX_BUILD_GATHER_POINTS: usize = 1 << 16;

const ROOT: u16 = 0x444e;
const CORE: u16 = 0x0001;
const MAP: u16 = 0x0002;
const OBJECTS: u16 = 0x0003;
const LEADERS: u16 = 0x0004;
const PATHS: u16 = 0x0005;
const ITEMS: u16 = 0x0006;
const BUILDS: u16 = 0x0007;
const REQUIRED: [u16; 7] = [CORE, MAP, OBJECTS, LEADERS, PATHS, ITEMS, BUILDS];

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
    Items(String),
    Builds(String),
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
            Self::Items(s) => write!(f, "invalid item state: {s}"),
            Self::Builds(s) => write!(f, "invalid construction/production state: {s}"),
        }
    }
}

impl std::error::Error for SaveError {}

impl From<WorldSaveError> for SaveError {
    fn from(value: WorldSaveError) -> Self {
        Self::World(value.to_string())
    }
}

impl From<ItemRuntimeSaveError> for SaveError {
    fn from(value: ItemRuntimeSaveError) -> Self {
        Self::Items(value.to_string())
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
    w.bool(o.special_anim.is_some());
    if let Some(special) = o.special_anim {
        w.i32(special.special_type as i32);
        w.i32(special.started);
        w.i32(special.frames);
        w.i32(special.data1);
        w.i32(special.data2);
        w.i32(special.data3);
        w.i32(special.data4);
        w.i32(special.ox);
        w.i32(special.whom);
    }
    w.bool(o.form_order.is_some());
    if let Some(form) = o.form_order {
        w.i32(form.angle);
        w.i32(form.new_form);
        w.i32(form.delay);
    }
    w.bool(o.follow.is_some());
    if let Some(follow) = o.follow {
        w.i32(follow.ox);
        w.i32(follow.whom);
        w.u16(follow.uid);
        w.i32(follow.oxx);
        w.i32(follow.whose);
        w.u16(follow.uid2);
    }
}

fn read_order(r: &mut Reader<'_>) -> Result<Order, SaveError> {
    let kind = OrderIndex::from_index(r.u8()? as usize)
        .ok_or(SaveError::Invalid("unknown unit order index"))?;
    let order = Order {
        kind,
        flags: r.u8()?,
        x: r.i32()?,
        y: r.i32()?,
        target_who: r.i8()?,
        target_o: r.i16()?,
        tolerance: r.i32()?,
        special_anim: None,
        follow: None,
        form_order: None,
    };
    let special_anim = if r.bool()? {
        let special_type = SpecialAnimType::from_raw(r.i32()?)
            .ok_or(SaveError::Invalid("unknown special animation type"))?;
        Some(SpecialAnimOrderState {
            special_type,
            started: r.i32()?,
            frames: r.i32()?,
            data1: r.i32()?,
            data2: r.i32()?,
            data3: r.i32()?,
            data4: r.i32()?,
            ox: r.i32()?,
            whom: r.i32()?,
        })
    } else {
        None
    };
    let form_order = if r.bool()? {
        Some(FormOrderState {
            angle: r.i32()?,
            new_form: r.i32()?,
            delay: r.i32()?,
        })
    } else {
        None
    };
    let follow = if r.bool()? {
        Some(FollowOrderPayload {
            ox: r.i32()?,
            whom: r.i32()?,
            uid: r.u16()?,
            oxx: r.i32()?,
            whose: r.i32()?,
            uid2: r.u16()?,
        })
    } else {
        None
    };
    Ok(Order {
        special_anim,
        form_order,
        follow,
        ..order
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
    for rows in &state.build_rows {
        w.len(rows.len(), "build registry rows")?;
        for &row in rows {
            w.u32(row);
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
    let mut build_rows: [Vec<u32>; crate::objects::OWNER_SLOTS] =
        std::array::from_fn(|_| Vec::new());
    for rows in &mut build_rows {
        let count = r.len(
            crate::objects::BANDED_SLOTS * production::BUILD_POOL_SLOTS,
            "build registry rows",
        )?;
        rows.reserve(count);
        for _ in 0..count {
            rows.push(r.u32()?);
        }
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
        build_rows,
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

fn validate_build_state(
    builds: &[production::BuildData],
    world: &WorldSaveState,
) -> Result<(), SaveError> {
    if builds.len() > MAX_BUILDS {
        return Err(SaveError::Limit("build records"));
    }
    let registry_count: usize = world.build_rows.iter().map(Vec::len).sum();
    if registry_count != builds.len() {
        return Err(SaveError::Builds(
            "BuildData rows do not cover the build registry exactly".into(),
        ));
    }

    let mut seen = vec![false; builds.len()];
    for (owner, rows) in world.build_rows.iter().enumerate() {
        if owner >= crate::objects::BANDED_SLOTS && !rows.is_empty() {
            return Err(SaveError::Builds(
                "build object exists outside retail player slots 0..7".into(),
            ));
        }
        if rows.len() > production::BUILD_POOL_SLOTS {
            return Err(SaveError::Limit("per-player build band"));
        }
        for (slot, &row) in rows.iter().enumerate() {
            let row = row as usize;
            let Some(build) = builds.get(row) else {
                return Err(SaveError::Builds(
                    "build registry row is outside the BuildData vector".into(),
                ));
            };
            if std::mem::replace(&mut seen[row], true) {
                return Err(SaveError::Builds(
                    "BuildData row appears more than once in the registry".into(),
                ));
            }
            if build.who as usize != owner
                || build.object_id() as i32 != crate::objects::BUILD_BAND_BASE as i32 + slot as i32
            {
                return Err(SaveError::Builds(
                    "BuildData (who,o) identity disagrees with the registry".into(),
                ));
            }
            validate_supported_build(build)?;
        }
    }
    if seen.iter().any(|seen| !seen) {
        return Err(SaveError::Builds(
            "BuildData vector contains an unregistered row".into(),
        ));
    }
    Ok(())
}

fn validate_supported_build(build: &production::BuildData) -> Result<(), SaveError> {
    if !build.is_valid() {
        return Err(SaveError::Builds(
            "closed/tombstone building slots are not yet reconstructible".into(),
        ));
    }
    if build.queue.num() > MAX_BUILD_QUEUE_ENTRIES {
        return Err(SaveError::Limit("build queue allocation"));
    }
    if build.queue.queued as usize > build.queue.num() {
        return Err(SaveError::Builds(
            "logical build queue exceeds its allocated records".into(),
        ));
    }
    if build.queue.queued != 0 && !build.is_active() {
        return Err(SaveError::Builds(
            "an unfinished construction site owns a live production queue".into(),
        ));
    }
    if build.queue.queued == 0 && build.build_masks & production::mask::REPEAT_QUEUE != 0 {
        return Err(SaveError::Builds(
            "repeat-production latch is set on an empty queue".into(),
        ));
    }
    for entry in build.queue.entries.iter().take(build.queue.queued as usize) {
        if entry.type_index < 0 || entry.elapsed < 0 {
            return Err(SaveError::Builds(
                "live build queue entry has invalid type/progress".into(),
            ));
        }
        if matches!(entry.type_index as i32, 0x286 | 0x29a) {
            return Err(SaveError::Builds(
                "razing/special building queue state is not owned".into(),
            ));
        }
    }
    if build.gather_from.tiles.len() > MAX_BUILD_MINING_TILES {
        return Err(SaveError::Limit("building mining tiles"));
    }
    if build.gather.len() > MAX_BUILD_GATHER_POINTS {
        return Err(SaveError::Limit("building gather points"));
    }

    let inside_down = i16::from_le_bytes([build.other[0x28], build.other[0x29]]);
    if build.flags & production::flag::CAPTURED != 0
        || build.build_masks & (production::mask::EJECTING | production::mask::OWNERSHIP_LATCH) != 0
        || build.demolition != 0
        || build.gather_down >= 0
        || build.city >= 0
        || build.city_down >= 0
        || build.wonder >= 0
        || build.dock >= 0
        || build.attack_ox >= 0
        || build.attack_whom >= 0
        || build.gather_max != 0
        || build.infiltrate != 0
        || build.infiltrate2 != 0
        || build.gather_from.mtn != 0
        || build.gather_from.cliff != 0
        || !build.gather_from.tiles.is_empty()
        || !build.gather.is_empty()
        || inside_down >= 0
    {
        return Err(SaveError::Builds(
            "captured/wonder/gather/garrison/special-family building state is not owned".into(),
        ));
    }
    Ok(())
}

fn write_builds(
    builds: &[production::BuildData],
    world: &WorldSaveState,
) -> Result<Vec<u8>, SaveError> {
    validate_build_state(builds, world)?;
    let mut w = Writer::default();
    w.len(builds.len(), "build records")?;
    for build in builds {
        w.bytes(&build.other);
        w.u8(build.flags);
        w.u8(build.who);
        w.i32(build.myhits);
        w.i32(build.damage);
        w.u16(build.uid);
        w.i8(build.damage_frac);
        w.u32(build.job_counter);
        w.u32(build.job_counter_2);
        w.u32(build.constr_time);
        w.i32(build.construct_hits);
        w.i32(build.gpiece);
        w.i32(build.frame_started);
        w.u16(build.build_masks);
        w.u8(build.ever_seen);
        w.u8(build.ever_seen_completed);
        w.u8(build.helpers);
        w.u8(build.demolition);
        w.i32(build.orig_type);
        w.i16(build.gather_down);
        w.i16(build.city);
        w.i16(build.city_down);
        w.i16(build.wonder);
        w.i16(build.dock);
        w.i16(build.recharging);
        w.i16(build.attack_ox);
        w.i8(build.stance);
        w.i8(build.founder);
        w.i8(build.gather_max);
        w.i8(build.attack_whom);
        w.u8(build.max_age);
        w.u8(build.infiltrate);
        w.u8(build.infiltrate2);

        w.u8(build.queue.queued);
        w.len(build.queue.entries.len(), "build queue allocation")?;
        for entry in &build.queue.entries {
            w.i32(entry.elapsed);
            w.i16(entry.type_index);
            for &resource in &entry.res {
                w.i16(resource);
            }
            for &amount in &entry.amt {
                w.i16(amount);
            }
            w.i16(entry.tail);
        }

        w.i8(build.gather_from.mtn);
        w.i8(build.gather_from.cliff);
        w.len(build.gather_from.tiles.len(), "building mining tiles")?;
        for &tile in &build.gather_from.tiles {
            w.u32(tile);
        }
        w.len(build.gather.len(), "building gather points")?;
        for point in &build.gather {
            w.i32(point.x);
            w.i32(point.y);
            w.u8(point.action);
            w.u8(point.node_tag);
        }
    }
    Ok(w.0)
}

fn read_builds(
    data: &[u8],
    world: &WorldSaveState,
) -> Result<Vec<production::BuildData>, SaveError> {
    let mut r = Reader::new(data);
    let count = r.len(MAX_BUILDS, "build records")?;
    let mut builds = Vec::with_capacity(count);
    for _ in 0..count {
        let mut build = production::BuildData::default();
        build
            .other
            .copy_from_slice(r.take(production::BUILDDATA_SIZE)?);
        build.flags = r.u8()?;
        build.who = r.u8()?;
        build.myhits = r.i32()?;
        build.damage = r.i32()?;
        build.uid = r.u16()?;
        build.damage_frac = r.i8()?;
        build.job_counter = r.u32()?;
        build.job_counter_2 = r.u32()?;
        build.constr_time = r.u32()?;
        build.construct_hits = r.i32()?;
        build.gpiece = r.i32()?;
        build.frame_started = r.i32()?;
        build.build_masks = r.u16()?;
        build.ever_seen = r.u8()?;
        build.ever_seen_completed = r.u8()?;
        build.helpers = r.u8()?;
        build.demolition = r.u8()?;
        build.orig_type = r.i32()?;
        build.gather_down = r.i16()?;
        build.city = r.i16()?;
        build.city_down = r.i16()?;
        build.wonder = r.i16()?;
        build.dock = r.i16()?;
        build.recharging = r.i16()?;
        build.attack_ox = r.i16()?;
        build.stance = r.i8()?;
        build.founder = r.i8()?;
        build.gather_max = r.i8()?;
        build.attack_whom = r.i8()?;
        build.max_age = r.u8()?;
        build.infiltrate = r.u8()?;
        build.infiltrate2 = r.u8()?;

        build.queue.queued = r.u8()?;
        let queue_count = r.len(MAX_BUILD_QUEUE_ENTRIES, "build queue allocation")?;
        build.queue.entries.reserve(queue_count);
        for _ in 0..queue_count {
            let elapsed = r.i32()?;
            let type_index = r.i16()?;
            let mut res = [0; 3];
            for resource in &mut res {
                *resource = r.i16()?;
            }
            let mut amt = [0; 3];
            for amount in &mut amt {
                *amount = r.i16()?;
            }
            build.queue.entries.push(production::BuildQueueEntry {
                elapsed,
                type_index,
                res,
                amt,
                tail: r.i16()?,
            });
        }

        build.gather_from.mtn = r.i8()?;
        build.gather_from.cliff = r.i8()?;
        let mining_count = r.len(MAX_BUILD_MINING_TILES, "building mining tiles")?;
        build.gather_from.tiles.reserve(mining_count);
        for _ in 0..mining_count {
            build.gather_from.tiles.push(r.u32()?);
        }
        let gather_count = r.len(MAX_BUILD_GATHER_POINTS, "building gather points")?;
        build.gather.reserve(gather_count);
        for _ in 0..gather_count {
            build.gather.push(production::GatherPoint {
                x: r.i32()?,
                y: r.i32()?,
                action: r.u8()?,
                node_tag: r.u8()?,
            });
        }
        builds.push(build);
    }
    r.finish()?;
    validate_build_state(&builds, world)?;
    Ok(builds)
}

fn write_items(sim: &Sim) -> Result<Vec<u8>, SaveError> {
    let mut w = Writer::default();
    let Some(runtime) = sim.world.item_runtime.as_ref() else {
        // Absence is a distinct producer state, not shorthand for an empty registry.
        // It is only coherent when channel 12 has no item sentinels either.
        validate_absent_items_map(&sim.map.world)?;
        w.u8(0);
        return Ok(w.0);
    };

    let state = runtime.export_save_state(&sim.map.world)?;
    w.u8(1);
    w.i32(state.map_xs);
    w.i32(state.map_ys);
    if state.slots.len() > MAX_ITEM_SLOTS {
        return Err(SaveError::Limit("item stable slots"));
    }
    w.len(state.slots.len(), "item stable slots")?;
    for item in state.slots {
        w.u8(item.flags);
        w.u8(item.who);
        w.i16(item.o);
        w.i32(item.z_internal);
        w.i32(item.x_internal);
        w.i32(item.y_internal);
        w.i32(item.type_index);
        w.bool(item.has_type);
        w.u8(item.ever_seen);
    }
    Ok(w.0)
}

fn read_items(data: &[u8], map: &map_terrain::World) -> Result<Option<ItemRuntime>, SaveError> {
    let mut r = Reader::new(data);
    match r.u8()? {
        0 => {
            r.finish()?;
            validate_absent_items_map(map)?;
            Ok(None)
        }
        1 => {
            let map_xs = r.i32()?;
            let map_ys = r.i32()?;
            let count = r.len(MAX_ITEM_SLOTS, "item stable slots")?;
            let mut slots = Vec::with_capacity(count);
            for _ in 0..count {
                slots.push(Item {
                    flags: r.u8()?,
                    who: r.u8()?,
                    o: r.i16()?,
                    z_internal: r.i32()?,
                    x_internal: r.i32()?,
                    y_internal: r.i32()?,
                    type_index: r.i32()?,
                    has_type: r.bool()?,
                    ever_seen: r.u8()?,
                });
            }
            r.finish()?;
            Ok(Some(ItemRuntime::import_save_state(
                ItemRuntimeSaveState {
                    map_xs,
                    map_ys,
                    slots,
                },
                map,
            )?))
        }
        _ => Err(SaveError::Invalid("unknown item producer state")),
    }
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

fn step8_state_is_pristine(sim: &Sim) -> bool {
    step8_views::is_supported_derived_snapshot(sim)
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CoreState {
    seed: i32,
    frame: i32,
    seconds: i32,
    random_state: i32,
    /// Exact PDB-owned record. Its `repaths`, `borders`, and `busy` fields are independent;
    /// `empty_colls` is the one persisted representation of the collision runtime's cursor.
    game_daemon: game_daemon_step12::GameDaemonState,
    /// Independent `GroupsData` walk cursor. Group slots and `last_group` remain outside
    /// this narrow tranche and are still refused unless pristine.
    groups_proc_group: i32,
}

fn write_core(sim: &Sim) -> Vec<u8> {
    let mut w = Writer::default();
    w.u32(FORMAT_VERSION);
    w.i32(sim.map.world.seed);
    w.i32(sim.world.frame);
    w.i32(sim.world.seconds);
    w.i32(sim.world.random.state());
    for &repath in &sim.game_daemon.repaths {
        w.i32(repath);
    }
    // The live runtime exclusively advances this scalar; reject_unsupported has already
    // proved the PDB-shaped GameDaemon mirror agrees with it.
    w.i32(sim.collision_blocks.cursor());
    w.i32(sim.game_daemon.borders);
    w.i32(sim.game_daemon.busy);
    w.i32(sim.groups.proc_group);
    w.0
}

fn read_core(data: &[u8]) -> Result<CoreState, SaveError> {
    let mut r = Reader::new(data);
    if r.u32()? != FORMAT_VERSION {
        return Err(SaveError::Invalid("unsupported save format version"));
    }
    let seed = r.i32()?;
    let frame = r.i32()?;
    let seconds = r.i32()?;
    let random_state = r.i32()?;
    let mut repaths = [0; game_daemon_step12::LEADER_SLOTS];
    for repath in &mut repaths {
        *repath = r.i32()?;
    }
    let game_daemon = game_daemon_step12::GameDaemonState {
        repaths,
        empty_colls: r.i32()?,
        borders: r.i32()?,
        busy: r.i32()?,
    };
    let groups_proc_group = r.i32()?;
    if !(0..groups_guys::GROUPS_PER_PLAYER as i32).contains(&groups_proc_group) {
        return Err(SaveError::Invalid("groups proc_group"));
    }
    r.finish()?;
    Ok(CoreState {
        seed,
        frame,
        seconds,
        random_state,
        game_daemon,
        groups_proc_group,
    })
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
    if sim.groups.list != default_groups.list || sim.groups.last_group != default_groups.last_group
    {
        return Err(SaveError::Unsupported("groups"));
    }
    if !(0..groups_guys::GROUPS_PER_PLAYER as i32).contains(&sim.groups.proc_group) {
        return Err(SaveError::Invalid("groups proc_group"));
    }
    if sim.game_daemon.empty_colls != sim.collision_blocks.cursor() {
        // The runtime is an execution adapter for the same GameDaemon+0x20 scalar, not a
        // second persistent owner. Refuse divergent public state instead of choosing one.
        return Err(SaveError::Invalid("GameDaemon collision cursor mirror"));
    }
    if sim.world.rules.balance.is_some()
        || !sim.world.rules.unit_stats.is_empty()
        || sim.world.rules.combat != crate::mechanics::CombatRules::default()
    {
        return Err(SaveError::Unsupported("installed World rules"));
    }
    if sim.econ_rules != economy::EconRules::shipped() {
        return Err(SaveError::Unsupported("modified economy rules"));
    }
    if production::SHIPPED_PRODUCTION_RULES
        .iter()
        .any(|&(offset, _, value)| sim.prod_rules.get(offset) != value)
    {
        return Err(SaveError::Unsupported("modified production rules"));
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
    let builds = write_builds(&sim.builds, &state)?;
    let map = write_map(sim)?;
    // Validate the producer through the same bounded decoder used for untrusted input.
    // This catches an internally inconsistent public MapState before bytes escape.
    let _ = read_map(&map)?;
    let items = write_items(sim)?;
    let root = Chunk::branch(
        ROOT,
        vec![
            Chunk::leaf(CORE, write_core(sim)),
            Chunk::leaf(MAP, map),
            Chunk::leaf(OBJECTS, write_world_state(&state)?),
            Chunk::leaf(LEADERS, write_leaders(sim)?),
            Chunk::leaf(PATHS, write_paths(sim)?),
            Chunk::leaf(ITEMS, items),
            Chunk::leaf(BUILDS, builds),
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
    let [core, map, objects, leaders, paths, items, builds] = sections.map(Option::unwrap);
    let core = read_core(core)?;
    let map = read_map(map)?;
    if map.world.seed != core.seed {
        return Err(SaveError::Invalid("core/map seed mismatch"));
    }
    let world_state = read_world_state(objects, core.frame, core.seconds, core.random_state)?;
    let builds = read_builds(builds, &world_state)?;
    let expected_types = world_state.unit_type_id.clone();
    let (leaders, market) = read_leaders(leaders)?;
    let (unit_type, paths, path_unit) = read_paths(paths, &expected_types)?;
    let item_runtime = read_items(items, &map.world)?;

    let mut sim = Sim::new(core.seed as u32 as u64, map.world.xs as u16);
    sim.map = map;
    sim.world.import_save_state(world_state)?;
    sim.world.item_runtime = item_runtime;
    sim.builds = builds;
    sim.leaders = leaders;
    sim.market = market;
    sim.unit_type = unit_type;
    sim.paths = paths;
    sim.path_unit = path_unit;
    sim.crash_units = vec![None; sim.world.live_count() as usize];
    sim.game_daemon = core.game_daemon;
    sim.collision_blocks =
        crate::systems::collision_blocks_live::CollisionBlockRuntime::from_cursor(
            core.game_daemon.empty_colls,
        );
    sim.groups.proc_group = core.groups_proc_group;
    // A loaded state must itself be saveable. This catches accidental constructor state
    // that would otherwise make the first post-load save fail or silently differ.
    reject_unsupported(&sim)?;
    Ok(sim)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::order::{OrderIndex, SpecialAnimType};
    use crate::systems::items::{self, GoodyRules, LeaderGoody, DOWN_ITEM, WFLAG_ITEM};

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
            special_anim: None,
            ..Order::default()
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
        sim.world
            .orders_mut(br)
            .push(Order::special_anim(SpecialAnimType::Exit, 41, 43));
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

    fn sim_with_stable_items() -> Sim {
        let mut sim = supported_sim();
        for (wx, wy) in [(0, 0), (1, 1), (2, 2)] {
            sim.map.world.wdata_mut(wx, wy).land = 0;
        }
        sim.world.configure_items(&sim.map.world);
        assert_eq!(sim.world.place_goody(&mut sim.map.world, 0, 0, 5), Ok(0));
        assert_eq!(sim.world.place_goody(&mut sim.map.world, 1, 1, 9), Ok(1));
        assert_eq!(sim.world.place_goody(&mut sim.map.world, 2, 2, 13), Ok(2));
        sim.world.reveal_goody(&sim.map.world, 2, 2, 3).unwrap();

        let mut leader = LeaderGoody::default();
        let (x, y) = items::snap_center(1, 1);
        assert!(sim
            .world
            .collect_goody(
                &mut sim.map.world,
                &mut leader,
                &GoodyRules::default(),
                x,
                y,
            )
            .unwrap()
            .is_some());
        let runtime = sim.world.item_runtime.as_ref().unwrap();
        assert_eq!(runtime.items().len(), 3);
        assert!(runtime.items().get(0).unwrap().is_valid());
        assert!(!runtime.items().get(1).unwrap().is_valid());
        assert!(runtime.items().get(2).unwrap().is_valid());
        sim
    }

    fn ordinary_build(active: bool) -> production::BuildData {
        let mut build = production::BuildData {
            flags: production::flag::VALID | production::flag::STARTED,
            myhits: 640,
            damage: 17,
            uid: 0x3456,
            job_counter: 321,
            job_counter_2: 287,
            constr_time: 20_000,
            construct_hits: 37,
            frame_started: 5,
            build_masks: production::mask::WORKED_LAST_FRAME,
            ever_seen: 4,
            ever_seen_completed: 2,
            helpers: 0,
            recharging: 9,
            stance: 3,
            founder: 2,
            max_age: 4,
            ..Default::default()
        };
        if active {
            build.flags |= production::flag::ACTIVE;
        }
        // These are real object/list links, not nullable Rust options. The supported
        // ordinary family must carry retail's negative sentinel rather than Default's
        // zero-filled test image.
        build.gather_down = -1;
        build.city = -1;
        build.city_down = -1;
        build.wonder = -1;
        build.dock = -1;
        build.attack_ox = -1;
        build.attack_whom = -1;
        build.other[0x28..0x2a].copy_from_slice(&(-1i16).to_le_bytes());
        build
    }

    fn install_build(sim: &mut Sim, owner: usize, build: production::BuildData) -> usize {
        let row = sim.spawn_build(owner, build);
        let slot = sim
            .world
            .objects
            .slot(owner)
            .band(crate::objects::Band::Build)
            .len()
            - 1;
        let object_id = crate::objects::BUILD_BAND_BASE as i16 + slot as i16;
        sim.builds[row].other[production::off::OBJECT_ID..production::off::OBJECT_ID + 2]
            .copy_from_slice(&object_id.to_le_bytes());
        row
    }

    fn assert_build_eq(actual: &production::BuildData, expected: &production::BuildData) {
        assert_eq!(actual.image(), expected.image());
        assert_eq!(actual.other, expected.other);
        assert_eq!(actual.queue.queued, expected.queue.queued);
        assert_eq!(actual.queue.entries, expected.queue.entries);
        assert_eq!(actual.gather_from.tiles, expected.gather_from.tiles);
        assert_eq!(actual.gather_from.mtn, expected.gather_from.mtn);
        assert_eq!(actual.gather_from.cliff, expected.gather_from.cliff);
        assert_eq!(actual.gather.len(), expected.gather.len());
        for (actual, expected) in actual.gather.iter().zip(&expected.gather) {
            assert_eq!(actual.x, expected.x);
            assert_eq!(actual.y, expected.y);
            assert_eq!(actual.action, expected.action);
            assert_eq!(actual.node_tag, expected.node_tag);
        }
    }

    fn sim_with_construction_and_queue() -> (Sim, usize, usize) {
        let mut sim = supported_sim();
        let site = install_build(&mut sim, 2, ordinary_build(false));

        let mut producer = ordinary_build(true);
        producer.job_counter = 0;
        producer.job_counter_2 = 0;
        producer.construct_hits = producer.myhits;
        producer.build_masks |= production::mask::REPEAT_QUEUE;
        producer.queue.queued = 2;
        producer.queue.entries = vec![
            production::BuildQueueEntry {
                elapsed: 700,
                type_index: 50,
                res: [0, 1, -1],
                amt: [30, 20, 0],
                tail: 0x1234,
            },
            production::BuildQueueEntry {
                elapsed: 900,
                type_index: 551,
                res: [3, -1, -1],
                amt: [60, 0, 0],
                tail: -7,
            },
            // Allocated stale tail is checksum-visible even though queued == 2.
            production::BuildQueueEntry {
                elapsed: 888,
                type_index: 777,
                res: [5, 4, 3],
                amt: [1, 2, 3],
                tail: 99,
            },
        ];
        let producer = install_build(&mut sim, 2, producer);

        let mut order = Order::default();
        order.kind = OrderIndex::BuildAt;
        order.target_who = 2;
        order.target_o = site as i16;
        sim.world.orders_mut(0).replace(order);
        (sim, site, producer)
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
    fn step12_owned_state_round_trips_and_resumes_without_serializing_its_cursor_mirror_twice() {
        let mut original = supported_sim();
        original.game_daemon = game_daemon_step12::GameDaemonState {
            repaths: [3, 4, 5, 6, 7, 8, i32::MIN, i32::MAX],
            empty_colls: 37,
            borders: -91,
            busy: i32::MIN,
        };
        original.collision_blocks =
            crate::systems::collision_blocks_live::CollisionBlockRuntime::from_cursor(37);
        original.groups.proc_group = 63;

        let bytes = save_sim(&original).unwrap();
        let mut loaded = load_sim(&bytes).unwrap();
        assert_eq!(loaded.game_daemon, original.game_daemon);
        assert_eq!(loaded.collision_blocks.cursor(), 37);
        assert_eq!(loaded.groups.proc_group, 63);
        assert_eq!(save_sim(&loaded).unwrap(), bytes);

        original.do_frame();
        loaded.do_frame();
        assert_eq!(loaded.game_daemon, original.game_daemon);
        assert_eq!(
            loaded.collision_blocks.cursor(),
            original.collision_blocks.cursor()
        );
        assert_eq!(loaded.groups.proc_group, original.groups.proc_group);
    }

    #[test]
    fn one_real_step12_pass_can_be_saved_with_default_group_slots() {
        let mut sim = Sim::new(0x3456_789a, 4);
        sim.do_frame();
        assert_eq!(sim.groups.proc_group, 1);
        assert_eq!(sim.game_daemon.empty_colls, sim.collision_blocks.cursor());

        let bytes = save_sim(&sim).unwrap();
        let loaded = load_sim(&bytes).unwrap();
        assert_eq!(loaded.game_daemon, sim.game_daemon);
        assert_eq!(loaded.groups.proc_group, 1);
        assert_eq!(save_sim(&loaded).unwrap(), bytes);
    }

    #[test]
    fn divergent_step12_cursor_views_and_live_group_slots_still_fail_closed() {
        let mut divergent = supported_sim();
        divergent.game_daemon.empty_colls = 7;
        divergent.collision_blocks =
            crate::systems::collision_blocks_live::CollisionBlockRuntime::from_cursor(9);
        assert_eq!(
            save_sim(&divergent),
            Err(SaveError::Invalid("GameDaemon collision cursor mirror"))
        );

        let mut live_group = supported_sim();
        live_group.groups.list[0].form = 4;
        assert_eq!(save_sim(&live_group), Err(SaveError::Unsupported("groups")));

        let mut invalid_cursor = supported_sim();
        invalid_cursor.groups.proc_group = groups_guys::GROUPS_PER_PLAYER as i32;
        assert_eq!(
            save_sim(&invalid_cursor),
            Err(SaveError::Invalid("groups proc_group"))
        );
    }

    #[test]
    fn item_producer_absence_and_initialized_empty_are_distinct() {
        let absent = supported_sim();
        let absent_bytes = save_sim(&absent).unwrap();
        let absent_loaded = load_sim(&absent_bytes).unwrap();
        assert!(absent_loaded.world.item_runtime.is_none());
        assert!(absent_loaded.world.items_channel().is_err());

        let mut empty = supported_sim();
        empty.world.configure_items(&empty.map.world);
        let empty_report = empty.world.items_channel().unwrap();
        assert_eq!(empty_report.checksum, 1);
        assert_eq!(empty_report.elements, 0);
        assert_eq!(empty_report.bytes_walked, 0);
        let empty_bytes = save_sim(&empty).unwrap();
        assert_ne!(empty_bytes, absent_bytes);

        let empty_loaded = load_sim(&empty_bytes).unwrap();
        assert!(empty_loaded.world.item_runtime.is_some());
        assert_eq!(empty_loaded.world.items_channel().unwrap(), empty_report);
        assert_eq!(save_sim(&empty_loaded).unwrap(), empty_bytes);
    }

    #[test]
    fn item_slots_and_channel_10_12_coupling_roundtrip_exactly() {
        let mut original = sim_with_stable_items();
        let item_report = original.world.items_channel().unwrap();
        let map_checksum = original.map.world.checksum();
        assert_eq!(item_report.elements, 2);
        assert_eq!(item_report.bytes_walked, 44);

        let bytes = save_sim(&original).unwrap();
        let mut loaded = load_sim(&bytes).unwrap();
        assert_eq!(loaded.world.items_channel().unwrap(), item_report);
        assert_eq!(loaded.map.world.checksum(), map_checksum);
        assert_eq!(
            loaded.world.item_runtime.as_ref().unwrap().items(),
            original.world.item_runtime.as_ref().unwrap().items()
        );
        assert_eq!(save_sim(&loaded).unwrap(), bytes);

        // The exact dead slot is authoritative state: both timelines must reuse slot 1
        // before growing and must change channels 10 and 12 identically.
        assert_eq!(
            original
                .world
                .place_goody(&mut original.map.world, 1, 2, 17),
            Ok(1)
        );
        assert_eq!(
            loaded.world.place_goody(&mut loaded.map.world, 1, 2, 17),
            Ok(1)
        );
        assert_eq!(loaded.world.items_channel(), original.world.items_channel());
        assert_eq!(loaded.map.world.checksum(), original.map.world.checksum());
    }

    #[test]
    fn item_map_mismatch_and_heterogeneous_occupancy_fail_closed() {
        let mut absent = supported_sim();
        let cell = absent.map.world.wdata_mut(0, 0);
        cell.flags |= WFLAG_ITEM;
        cell.down = DOWN_ITEM;
        cell.down_who = 0;
        assert!(matches!(save_sim(&absent), Err(SaveError::Items(_))));

        let mut heterogeneous = supported_sim();
        heterogeneous
            .world
            .configure_items(&heterogeneous.map.world);
        heterogeneous
            .world
            .place_goody(&mut heterogeneous.map.world, 0, 0, 5)
            .unwrap();
        heterogeneous.map.world.wdata_mut(0, 0).down = 0;
        let error = save_sim(&heterogeneous).unwrap_err();
        assert!(matches!(error, SaveError::Items(ref s) if s.contains("heterogeneous")));
    }

    #[test]
    fn corrupt_item_slot_identity_is_rejected_on_load() {
        let bytes = save_sim(&sim_with_stable_items()).unwrap();
        let items = section_offset(&bytes, ITEMS);
        // payload: present (1), map shape (8), slot count (4), then flags/who/o.
        let first_o = items + 8 + 1 + 8 + 4 + 2;
        let mut corrupt = bytes.clone();
        corrupt[first_o..first_o + 2].copy_from_slice(&7i16.to_le_bytes());
        assert!(matches!(load_error(&corrupt), SaveError::Items(_)));
    }

    #[test]
    fn item_walked_byte_mutation_changes_only_channel_10() {
        let original = sim_with_stable_items();
        let original_items = original.world.items_channel().unwrap();
        let original_map = original.map.world.checksum();
        let mut bytes = save_sim(&original).unwrap();
        let items = section_offset(&bytes, ITEMS);
        // payload: present/shape/count (13), then a 22-byte record whose last byte is
        // ItemData::ever_seen. This field is walked by channel 10 but not channel 12.
        let first_ever_seen = items + 8 + 13 + 21;
        assert_eq!(bytes[first_ever_seen], 0);
        bytes[first_ever_seen] = 1;

        let loaded = load_sim(&bytes).unwrap();
        assert_ne!(loaded.world.items_channel().unwrap(), original_items);
        assert_eq!(loaded.map.world.checksum(), original_map);
        assert_eq!(
            loaded
                .world
                .item_runtime
                .as_ref()
                .unwrap()
                .items()
                .get(0)
                .unwrap()
                .ever_seen,
            1
        );
        assert_eq!(save_sim(&loaded).unwrap(), bytes);
    }

    #[test]
    fn construction_queue_identity_and_resume_roundtrip() {
        let (mut original, site, producer) = sim_with_construction_and_queue();
        let bytes = save_sim(&original).unwrap();
        let mut loaded = load_sim(&bytes).unwrap();

        assert_eq!(loaded.builds.len(), 2);
        assert_build_eq(&loaded.builds[site], &original.builds[site]);
        assert_build_eq(&loaded.builds[producer], &original.builds[producer]);
        assert_eq!(
            loaded
                .world
                .objects
                .slot(2)
                .band(crate::objects::Band::Build),
            &[site as u32, producer as u32]
        );
        assert_eq!(loaded.builds[site].object_id(), 2000);
        assert_eq!(loaded.builds[producer].object_id(), 2001);
        assert_eq!(loaded.builds[producer].queue.queued, 2);
        assert_eq!(loaded.builds[producer].queue.entries[0].elapsed, 700);
        assert_ne!(
            loaded.builds[producer].build_masks & production::mask::REPEAT_QUEUE,
            0
        );
        assert_eq!(save_sim(&loaded).unwrap(), bytes);

        // Resume the live construction timeline through Sim's object scheduler.
        original.do_frame();
        loaded.do_frame();
        assert_eq!(loaded.channel_digest(), original.channel_digest());
        assert_eq!(
            loaded.builds[site].job_counter,
            original.builds[site].job_counter
        );
        assert_build_eq(&loaded.builds[site], &original.builds[site]);

        // Queue completion needs mandatory external hosts, but the owned progress kernel
        // can resume immediately from both a unit and a research record.
        for (slot, kind, total) in [
            (0, production::QueueKind::Unit, 2_000),
            (1, production::QueueKind::Research, 3_000),
        ] {
            let original_step = production::queue_step(
                original.builds[producer].queue.queued,
                slot,
                original.builds[producer].queue.num(),
                original.builds[producer].queue.entries[slot].elapsed,
                total,
                kind,
                1,
                &original.prod_rules,
            )
            .unwrap();
            let loaded_step = production::queue_step(
                loaded.builds[producer].queue.queued,
                slot,
                loaded.builds[producer].queue.num(),
                loaded.builds[producer].queue.entries[slot].elapsed,
                total,
                kind,
                1,
                &loaded.prod_rules,
            )
            .unwrap();
            assert_eq!(loaded_step, original_step);
            original.builds[producer].queue.entries[slot].elapsed = original_step.elapsed;
            loaded.builds[producer].queue.entries[slot].elapsed = loaded_step.elapsed;
        }
        assert_build_eq(&loaded.builds[producer], &original.builds[producer]);
    }

    #[test]
    fn build_registry_corruption_and_special_families_fail_closed() {
        let (sim, _, _) = sim_with_construction_and_queue();
        let bytes = save_sim(&sim).unwrap();
        let builds = section_offset(&bytes, BUILDS);
        // payload: build count (4), first BuildData.other starts immediately afterward,
        // and SubObjectData::o is the short at +0x0a.
        let first_object_id = builds + 8 + 4 + production::off::OBJECT_ID;
        let mut corrupt = bytes.clone();
        corrupt[first_object_id..first_object_id + 2].copy_from_slice(&2001i16.to_le_bytes());
        assert!(matches!(load_error(&corrupt), SaveError::Builds(_)));

        let mut special = sim;
        special.builds[0].wonder = 0;
        assert!(matches!(save_sim(&special), Err(SaveError::Builds(_))));

        let (mut malformed, _, producer) = sim_with_construction_and_queue();
        malformed.builds[producer].queue.queued = 4;
        assert!(matches!(save_sim(&malformed), Err(SaveError::Builds(_))));
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

    #[test]
    fn unknown_special_animation_discriminators_are_rejected() {
        let mut bytes = vec![OrderIndex::SpecialAnim as u8, 0];
        bytes.extend_from_slice(&[0; 4 + 4 + 1 + 2 + 4]);
        bytes.push(1);
        bytes.extend_from_slice(&3i32.to_le_bytes());
        let mut reader = Reader::new(&bytes);
        assert_eq!(
            read_order(&mut reader),
            Err(SaveError::Invalid("unknown special animation type"))
        );
    }

    #[test]
    fn change_form_payload_round_trips_without_losing_the_walked_delay() {
        let order = Order::change_form(0x1020_3040, -7, 1234);
        let mut writer = Writer::default();
        write_order(&mut writer, &order);
        let mut reader = Reader::new(&writer.0);
        assert_eq!(read_order(&mut reader).unwrap(), order);
        reader.finish().unwrap();
    }

    #[test]
    fn follow_payload_round_trips_both_full_width_identities_and_uid_snapshots() {
        let order = Order::follow(FollowOrderPayload {
            ox: 70_000,
            whom: 300,
            uid: 0x1234,
            oxx: -40_000,
            whose: -500,
            uid2: 0xabcd,
        });
        let mut writer = Writer::default();
        write_order(&mut writer, &order);
        let mut reader = Reader::new(&writer.0);
        assert_eq!(read_order(&mut reader).unwrap(), order);
        reader.finish().unwrap();
    }
}
