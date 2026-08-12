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
    FollowOrderPayload, FormOrderState, MoveOrderState, Order, OrderIndex, OrderList,
    SpecialAnimOrderState, SpecialAnimType,
};
use crate::script_runtime::ScriptRuntime;
use crate::systems::{
    air_runtime_authority::{
        self, AirOrderPayload, AirPatrolOrderPayload, WalkedCoordArray, MAX_DECODED_PATROL_POINTS,
    },
    bhs_type_runtime::TypeBuiltinBoundaryError,
    borders_fog,
    canonical_diplomacy_host::{
        decode_diplomacy_for_save, encode_diplomacy_payload, DIPLOMACY_SAVE_FORMAT_VERSION,
        PRE_DIPLOMACY_SAVE_FORMAT_VERSION,
    },
    economy,
    economy_order_payload_authority::{
        self as economy_payload, EconomyOrderHeader, EconomyOrderNode, EconomyOrderPayload,
        StableTargetIdentity,
    },
    game_daemon_step12, groups_guys,
    items::Item,
    map_terrain, movement,
    player_setup::ManualPlayerSetup,
    production,
    setup_diplomacy::SETUP_SLOTS,
    sparse_object_bands_authority_frontier::{
        RetailBand, SnapshotLifecycle, SparseBandSnapshot, SparseOwnerSnapshot,
        SparseRegistrySnapshot, TombstoneFacts,
    },
    strafe_runtime_authority::{self, StrafeTargetIdentity, STRAFE_LEAF_BYTES},
    victory_score::MatchOptions,
};
use crate::tick::{LeaderSlot, Sim, NUM_LEADERS};
use crate::world::{WorldObjectIdentity, WorldSaveError, WorldSaveState, MAX_UNITS};

mod armies;
mod command_package_state;
mod groups;
mod leader_match;
mod step8_views;

const MAGIC: &[u8; 8] = b"DoNSave\0";
/// First version persisting the scenario ignore-orders scalar and eight ordered lists.
const SCENARIO_IGNORES_FORMAT_VERSION: u32 = 15;
/// First version persisting the checksum-owned global FarmStruct array.
const FARMS_FORMAT_VERSION: u32 = 16;
/// First version persisting the exact canonical eight-by-sixteen Armies owner.
const ARMIES_FORMAT_VERSION: u32 = 17;
const FORMAT_VERSION: u32 = ARMIES_FORMAT_VERSION;
/// First version reserving the retail `RecycledOrderNode::metric` byte per order-list node.
const ORDER_NODE_METRIC_FORMAT_VERSION: u32 = 13;
/// First version carrying the typed, extension-safe per-order payload envelope.
const TYPED_ORDER_FORMAT_VERSION: u32 = 12;
/// The last version whose order stream omitted executable MoveOrder/GroupOrder scalars.
const LEGACY_ORDER_FORMAT_VERSION: u32 = 11;
/// The last version whose root ended at [`GROUPS`], before [`LEADER_MATCH`] was added.
const GROUPS_FORMAT_VERSION: u32 = 10;
/// The last version whose root ended at [`PLAYER_SETUP`], before [`GROUPS`] was added.
const PLAYER_SETUP_FORMAT_VERSION: u32 = 9;
const SPARSE_OBJECTS_FORMAT_VERSION: u32 = 8;
const LEGACY_DENSE_OBJECTS_FORMAT_VERSION: u32 = 7;
const MAX_SAVE_BYTES: usize = 256 * 1024 * 1024;
const MAX_ORDERS_PER_UNIT: usize = 1024;
const MAX_PATH_RECORDS: usize = 1 << 20;
const MAX_ITEM_SLOTS: usize = i16::MAX as usize + 1;
const MAX_BUILDS: usize = crate::objects::BANDED_SLOTS * production::BUILD_POOL_SLOTS;
const MAX_BUILD_QUEUE_ENTRIES: usize = 4096;
const MAX_BUILD_MINING_TILES: usize = 1 << 20;
const MAX_BUILD_GATHER_POINTS: usize = 1 << 16;

/// DoNSave v12's extensible per-order payload tag table.
///
/// Every v12 order carries `tag:u8, payload_version:u8, payload...` after the legacy order
/// image.  Tags 2--10 are reserved by independently recovered concrete-order audits,
/// but are intentionally not writable/readable until [`Order`] owns their typed payloads:
/// reserving a number is not permission to serialize opaque placeholder bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum DoNSaveOrderPayloadTag {
    None = 0,
    Move = 1,
    Gather = 2,
    CastSpell = 3,
    TradeRoute = 4,
    Guard = 5,
    AirPatrol = 6,
    GroupPatrol = 7,
    Strafe = 8,
    AttackGround = 9,
    AirAttackGround = 10,
}

impl DoNSaveOrderPayloadTag {
    pub const fn from_raw(raw: u8) -> Option<Self> {
        match raw {
            0 => Some(Self::None),
            1 => Some(Self::Move),
            2 => Some(Self::Gather),
            3 => Some(Self::CastSpell),
            4 => Some(Self::TradeRoute),
            5 => Some(Self::Guard),
            6 => Some(Self::AirPatrol),
            7 => Some(Self::GroupPatrol),
            8 => Some(Self::Strafe),
            9 => Some(Self::AttackGround),
            10 => Some(Self::AirAttackGround),
            _ => None,
        }
    }

    /// Version of the exact field contract reserved for this tag. `None` has no body;
    /// every concrete payload starts at version 1 and evolves independently of DoNSave.
    pub const fn wire_version(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Move
            | Self::Gather
            | Self::CastSpell
            | Self::TradeRoute
            | Self::Guard
            | Self::AirPatrol
            | Self::GroupPatrol
            | Self::Strafe
            | Self::AttackGround
            | Self::AirAttackGround => 1,
        }
    }
}

const ROOT: u16 = 0x444e;
const CORE: u16 = 0x0001;
const MAP: u16 = 0x0002;
const OBJECTS: u16 = 0x0003;
const LEADERS: u16 = 0x0004;
const PATHS: u16 = 0x0005;
const ITEMS: u16 = 0x0006;
const BUILDS: u16 = 0x0007;
const PLAYER_SETUP: u16 = 0x0008;
const GROUPS: u16 = 0x0009;
const LEADER_MATCH: u16 = 0x000a;
const COMMAND_PACKAGE_STATE: u16 = 0x000b;
const DIPLOMACY: u16 = 0x000c;
const SCENARIO_IGNORES: u16 = 0x000d;
const FARMS: u16 = 0x000e;
const ARMIES: u16 = 0x000f;
const LEGACY_REQUIRED: [u16; 7] = [CORE, MAP, OBJECTS, LEADERS, PATHS, ITEMS, BUILDS];
const REQUIRED: [u16; 15] = [
    CORE,
    MAP,
    OBJECTS,
    LEADERS,
    PATHS,
    ITEMS,
    BUILDS,
    PLAYER_SETUP,
    GROUPS,
    LEADER_MATCH,
    COMMAND_PACKAGE_STATE,
    DIPLOMACY,
    SCENARIO_IGNORES,
    FARMS,
    ARMIES,
];
/// The root sections a stream of `version` must carry, in order.
fn required_sections(version: u32) -> &'static [u16] {
    match version {
        ARMIES_FORMAT_VERSION => &REQUIRED,
        FARMS_FORMAT_VERSION => &REQUIRED[..14],
        SCENARIO_IGNORES_FORMAT_VERSION => &REQUIRED[..13],
        DIPLOMACY_SAVE_FORMAT_VERSION => &REQUIRED[..12],
        PRE_DIPLOMACY_SAVE_FORMAT_VERSION => &REQUIRED[..11],
        LEGACY_ORDER_FORMAT_VERSION..=TYPED_ORDER_FORMAT_VERSION => &REQUIRED[..10],
        GROUPS_FORMAT_VERSION => &REQUIRED[..9],
        PLAYER_SETUP_FORMAT_VERSION => &REQUIRED[..8],
        _ => &LEGACY_REQUIRED,
    }
}

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
    BhsTypes(TypeBuiltinBoundaryError),
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
            Self::BhsTypes(error) => write!(f, "unsupported BHS type state: {error:?}"),
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

impl From<TypeBuiltinBoundaryError> for SaveError {
    fn from(value: TypeBuiltinBoundaryError) -> Self {
        Self::BhsTypes(value)
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

#[inline]
const fn order_kind_carries_move_state(kind: OrderIndex) -> bool {
    matches!(
        kind,
        OrderIndex::MoveTo
            | OrderIndex::AttackTo
            | OrderIndex::ExploreTo
            | OrderIndex::FleeTo
            | OrderIndex::ChangeForm
            | OrderIndex::GroupMove
            | OrderIndex::GroupAttackTo
    )
}

#[inline]
const fn order_kind_carries_economy_payload(kind: OrderIndex) -> bool {
    matches!(
        kind,
        OrderIndex::Gather
            | OrderIndex::BoardShip
            | OrderIndex::AwaitBoard
            | OrderIndex::Repair
            | OrderIndex::CastSpell
            | OrderIndex::TradeRoute
    )
}

fn economy_node(order: &Order) -> Result<Option<EconomyOrderNode>, SaveError> {
    let Some(payload) = order.economy else {
        if order_kind_carries_economy_payload(order.kind) {
            return Err(SaveError::Invalid("missing economy order payload"));
        }
        return Ok(None);
    };
    if !order_kind_carries_economy_payload(order.kind) {
        return Err(SaveError::Invalid("economy payload on foreign order kind"));
    }
    let node = EconomyOrderNode {
        metric: order.node_metric,
        header: EconomyOrderHeader {
            kind: order.kind,
            flags: order.flags,
            x: order.x,
            y: order.y,
            primary: StableTargetIdentity {
                o: i32::from(order.target_o),
                who: i32::from(order.target_who),
                uid: order.target_uid,
                handle: order.target_handle,
            },
        },
        payload,
    };
    node.validate().map_err(map_economy_payload_save_error)?;
    Ok(Some(node))
}

fn write_order(w: &mut Writer, o: &Order, format_version: u32) -> Result<(), SaveError> {
    // Validate the typed envelope before appending any bytes. `save_sim` writes into a local
    // buffer too, so an invalid public order cannot leak either a partial stream or a
    // partially normalized replacement order to its caller.
    if format_version < ORDER_NODE_METRIC_FORMAT_VERSION && o.air_patrol.is_some() {
        return Err(SaveError::Unsupported(
            "AIR_PATROL payload before DoNSave v13",
        ));
    }
    if format_version < ORDER_NODE_METRIC_FORMAT_VERSION && o.strafe.is_some() {
        return Err(SaveError::Unsupported("STRAFE payload before DoNSave v13"));
    }
    if format_version < ORDER_NODE_METRIC_FORMAT_VERSION && o.guard.is_some() {
        return Err(SaveError::Unsupported("GUARD payload before DoNSave v13"));
    }
    if format_version >= TYPED_ORDER_FORMAT_VERSION {
        if o.move_state.is_some() && !order_kind_carries_move_state(o.kind) {
            return Err(SaveError::Invalid("movement payload on foreign order kind"));
        }
        if o.move_state.is_none() && order_kind_carries_move_state(o.kind) {
            return Err(SaveError::Invalid("missing movement payload"));
        }
        if let (Some(move_state), Some(form)) = (o.move_state, o.form_order) {
            if o.kind == OrderIndex::ChangeForm && move_state.angle != form.angle {
                return Err(SaveError::Invalid("change-form movement angle mismatch"));
            }
        }
        if format_version >= ORDER_NODE_METRIC_FORMAT_VERSION {
            if o.air_patrol.is_some() && o.kind != OrderIndex::AirPatrol {
                return Err(SaveError::Invalid(
                    "AIR_PATROL payload on foreign order kind",
                ));
            }
            if o.air_patrol.is_none() && o.kind == OrderIndex::AirPatrol {
                return Err(SaveError::Invalid("missing AIR_PATROL payload"));
            }
            if o.strafe.is_some() && o.kind != OrderIndex::Strafe {
                return Err(SaveError::Invalid("STRAFE payload on foreign order kind"));
            }
            if o.strafe.is_none() && o.kind == OrderIndex::Strafe {
                return Err(SaveError::Invalid("missing STRAFE payload"));
            }
            if o.strafe.as_ref().is_some_and(|strafe| {
                (i32::from(o.target_o), i32::from(o.target_who), o.target_uid)
                    != (strafe.target_o, strafe.target_who, strafe.target_uid)
            }) {
                return Err(SaveError::Invalid("invalid STRAFE payload"));
            }
            if o.guard.is_some() && o.kind != OrderIndex::Guard {
                return Err(SaveError::Invalid("GUARD payload on foreign order kind"));
            }
            if o.guard.is_some_and(|guard| {
                (i32::from(o.target_o), i32::from(o.target_who), o.target_uid)
                    != (guard.target.o, guard.target.who, guard.target.uid)
            }) {
                return Err(SaveError::Invalid("invalid GUARD payload"));
            }
        }
        let typed_payloads = usize::from(o.move_state.is_some())
            + usize::from(o.air_patrol.is_some())
            + usize::from(o.strafe.is_some())
            + usize::from(o.guard.is_some());
        if typed_payloads > 1 {
            return Err(SaveError::Invalid("multiple typed order payloads"));
        }
    }
    // Economy payload ownership starts at v13.  Older writers must retain the exact v7--v12
    // generic image for legacy Board/AwaitBoard/Repair/Gather/Cast/Trade orders instead of
    // retroactively requiring a payload their format could not carry.
    let economy_node = if format_version >= economy_payload::DON_SAVE_V13 {
        economy_node(o)?
    } else if o.economy.is_some() {
        return Err(SaveError::Unsupported("economy payload before DoNSave v13"));
    } else {
        None
    };
    if economy_node.is_some()
        && (o.move_state.is_some()
            || o.air_patrol.is_some()
            || o.strafe.is_some()
            || o.guard.is_some())
    {
        return Err(SaveError::Invalid("multiple typed order payloads"));
    }
    if o.node_metric != 0 && format_version < ORDER_NODE_METRIC_FORMAT_VERSION {
        return Err(SaveError::Invalid("order node metric before v13"));
    }
    let economy_leaf = if let Some(node) = economy_node {
        if format_version < economy_payload::DON_SAVE_V13 {
            return Err(SaveError::Unsupported("economy payload before DoNSave v13"));
        }
        Some(
            economy_payload::encode_v13_leaf(format_version, node)
                .map_err(map_economy_payload_save_error)?,
        )
    } else {
        None
    };
    let air_patrol_leaf = if format_version >= ORDER_NODE_METRIC_FORMAT_VERSION {
        o.air_patrol
            .as_ref()
            .map(air_runtime_authority::encode_air_patrol_leaf)
            .transpose()
            .map_err(map_air_patrol_save_error)?
    } else {
        None
    };
    let strafe_leaf = if format_version >= ORDER_NODE_METRIC_FORMAT_VERSION {
        o.strafe
            .as_ref()
            .map(strafe_runtime_authority::encode_strafe_leaf)
            .transpose()
            .map_err(map_strafe_save_error)?
    } else {
        None
    };
    w.u8(o.kind as u8);
    w.u8(o.flags);
    w.i32(o.x);
    w.i32(o.y);
    w.i8(o.target_who);
    w.i16(o.target_o);
    w.u16(o.target_uid);
    w.bool(o.target_handle.is_some());
    if let Some(handle) = o.target_handle {
        w.u32(handle.id);
        w.u32(handle.generation);
    }
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
    if format_version >= TYPED_ORDER_FORMAT_VERSION {
        let tag = if o.move_state.is_some() {
            DoNSaveOrderPayloadTag::Move
        } else if let Some(leaf) = &economy_leaf {
            DoNSaveOrderPayloadTag::from_raw(leaf.typed_payload[0]).expect("validated economy tag")
        } else if air_patrol_leaf.is_some() {
            DoNSaveOrderPayloadTag::AirPatrol
        } else if strafe_leaf.is_some() {
            DoNSaveOrderPayloadTag::Strafe
        } else if o.guard.is_some() {
            DoNSaveOrderPayloadTag::Guard
        } else {
            DoNSaveOrderPayloadTag::None
        };
        if let Some(leaf) = economy_leaf {
            debug_assert_eq!(leaf.metric, o.node_metric);
            debug_assert_eq!(leaf.typed_payload[0], tag as u8);
            debug_assert_eq!(leaf.typed_payload[1], tag.wire_version());
            w.bytes(&leaf.typed_payload);
        } else if let Some(leaf) = air_patrol_leaf {
            // The canonical encoder includes tag/version so the independently tested leaf
            // is byte-for-byte the stream body, rather than being re-spelled here.
            debug_assert_eq!(leaf[0], tag as u8);
            debug_assert_eq!(leaf[1], tag.wire_version());
            w.bytes(&leaf);
        } else if let Some(leaf) = strafe_leaf {
            debug_assert_eq!(leaf[0], tag as u8);
            debug_assert_eq!(leaf[1], tag.wire_version());
            w.bytes(&leaf);
        } else if let Some(guard) = o.guard {
            w.u8(tag as u8);
            w.u8(tag.wire_version());
            w.i32(guard.target.o);
            w.i32(guard.target.who);
            w.u16(guard.target.uid);
            w.i32(guard.dx);
            w.i32(guard.dy);
            w.i32(guard.guard_x);
            w.i32(guard.guard_y);
            w.i32(guard.idle);
            w.i32(guard.retry);
        } else {
            w.u8(tag as u8);
            w.u8(tag.wire_version());
        }
        if let Some(state) = o.move_state {
            w.i32(state.angle);
            w.i32(state.dest);
            w.i32(state.pause);
            w.i32(state.retry);
            w.i32(state.attempts);
            w.i32(state.timer);
            w.i32(state.facing);
            w.i32(state.dest_x);
            w.i32(state.dest_y);
            w.i32(state.last_x);
            w.i32(state.last_y);
            w.i32(state.coll_x);
            w.i32(state.coll_y);
            w.i32(state.orig_x);
            w.i32(state.orig_y);
            w.i16(state.off_x);
            w.i16(state.off_y);
            w.i32(state.group_oxx);
            w.i32(state.group_whose);
            w.i32(state.group_id);
            w.i32(state.group_form_id);
            w.i32(state.group_angle);
            w.i32(state.in_group);
        }
    }
    Ok(())
}

fn read_order(r: &mut Reader<'_>, format_version: u32) -> Result<Order, SaveError> {
    let kind = OrderIndex::from_index(r.u8()? as usize)
        .ok_or(SaveError::Invalid("unknown unit order index"))?;
    let order = Order {
        node_metric: 0,
        kind,
        flags: r.u8()?,
        x: r.i32()?,
        y: r.i32()?,
        target_who: r.i8()?,
        target_o: r.i16()?,
        target_uid: r.u16()?,
        target_handle: if r.bool()? {
            Some(crate::Handle {
                id: r.u32()?,
                generation: r.u32()?,
            })
        } else {
            None
        },
        tolerance: r.i32()?,
        move_state: None,
        special_anim: None,
        follow: None,
        form_order: None,
        air_patrol: None,
        strafe: None,
        guard: None,
        economy: None,
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
    let (move_state, air_patrol, strafe, guard, economy) = if format_version
        >= TYPED_ORDER_FORMAT_VERSION
    {
        let tag = DoNSaveOrderPayloadTag::from_raw(r.u8()?).ok_or(SaveError::Invalid(
            "unknown order payload discriminator/version",
        ))?;
        let version = r.u8()?;
        match (tag, version) {
            (DoNSaveOrderPayloadTag::None, 0) => {
                if order_kind_carries_move_state(kind) {
                    return Err(SaveError::Invalid("missing movement payload"));
                }
                if format_version >= ORDER_NODE_METRIC_FORMAT_VERSION
                    && kind == OrderIndex::AirPatrol
                {
                    return Err(SaveError::Invalid("missing AIR_PATROL payload"));
                }
                if format_version >= ORDER_NODE_METRIC_FORMAT_VERSION && kind == OrderIndex::Strafe
                {
                    return Err(SaveError::Invalid("missing STRAFE payload"));
                }
                let economy = if format_version >= economy_payload::DON_SAVE_V13
                    && order_kind_carries_economy_payload(kind)
                {
                    Some(read_economy_payload(
                        r,
                        kind,
                        &order,
                        DoNSaveOrderPayloadTag::None,
                        0,
                    )?)
                } else {
                    None
                };
                (None, None, None, None, economy)
            }
            (DoNSaveOrderPayloadTag::Move, 1) => {
                if !order_kind_carries_move_state(kind) {
                    return Err(SaveError::Invalid("movement payload on foreign order kind"));
                }
                (
                    Some(MoveOrderState {
                        angle: r.i32()?,
                        dest: r.i32()?,
                        pause: r.i32()?,
                        retry: r.i32()?,
                        attempts: r.i32()?,
                        timer: r.i32()?,
                        facing: r.i32()?,
                        dest_x: r.i32()?,
                        dest_y: r.i32()?,
                        last_x: r.i32()?,
                        last_y: r.i32()?,
                        coll_x: r.i32()?,
                        coll_y: r.i32()?,
                        orig_x: r.i32()?,
                        orig_y: r.i32()?,
                        off_x: r.i16()?,
                        off_y: r.i16()?,
                        group_oxx: r.i32()?,
                        group_whose: r.i32()?,
                        group_id: r.i32()?,
                        group_form_id: r.i32()?,
                        group_angle: r.i32()?,
                        in_group: r.i32()?,
                    }),
                    None,
                    None,
                    None,
                    None,
                )
            }
            (
                tag @ (DoNSaveOrderPayloadTag::Gather
                | DoNSaveOrderPayloadTag::CastSpell
                | DoNSaveOrderPayloadTag::TradeRoute),
                1,
            ) if format_version >= economy_payload::DON_SAVE_V13 => (
                None,
                None,
                None,
                None,
                Some(read_economy_payload(r, kind, &order, tag, 1)?),
            ),
            (DoNSaveOrderPayloadTag::AirPatrol, 1)
                if format_version >= ORDER_NODE_METRIC_FORMAT_VERSION =>
            {
                if kind != OrderIndex::AirPatrol {
                    return Err(SaveError::Invalid(
                        "AIR_PATROL payload on foreign order kind",
                    ));
                }
                let x = read_air_patrol_coord_array(r)?;
                let y = read_air_patrol_coord_array(r)?;
                let payload = AirPatrolOrderPayload {
                    x,
                    y,
                    waypoint: r.i32()?,
                    air: AirOrderPayload {
                        home_o: r.i32()?,
                        home_who: r.i32()?,
                        cruising_alt: r.i32()?,
                        sharp_turn: r.i32()?,
                        old: r.i32()?,
                        returning: r.i32()?,
                    },
                };
                payload.validate().map_err(map_air_patrol_save_error)?;
                (None, Some(payload), None, None, None)
            }
            (DoNSaveOrderPayloadTag::AirPatrol, _)
                if format_version >= ORDER_NODE_METRIC_FORMAT_VERSION =>
            {
                return Err(SaveError::Invalid(
                    "unknown order payload discriminator/version",
                ));
            }
            (DoNSaveOrderPayloadTag::Strafe, 1)
                if format_version >= ORDER_NODE_METRIC_FORMAT_VERSION =>
            {
                if kind != OrderIndex::Strafe {
                    return Err(SaveError::Invalid("STRAFE payload on foreign order kind"));
                }
                let mut bytes = Vec::with_capacity(STRAFE_LEAF_BYTES);
                bytes.extend_from_slice(&[
                    DoNSaveOrderPayloadTag::Strafe as u8,
                    DoNSaveOrderPayloadTag::Strafe.wire_version(),
                ]);
                bytes.extend_from_slice(r.take(STRAFE_LEAF_BYTES - 2)?);
                let payload = strafe_runtime_authority::decode_strafe_leaf(
                    StrafeTargetIdentity {
                        o: i32::from(order.target_o),
                        who: i32::from(order.target_who),
                        uid: order.target_uid,
                    },
                    &bytes,
                )
                .map_err(map_strafe_save_error)?;
                (None, None, Some(payload), None, None)
            }
            (DoNSaveOrderPayloadTag::Strafe, _)
                if format_version >= ORDER_NODE_METRIC_FORMAT_VERSION =>
            {
                return Err(SaveError::Invalid(
                    "unknown order payload discriminator/version",
                ));
            }
            (DoNSaveOrderPayloadTag::Guard, 1)
                if format_version >= ORDER_NODE_METRIC_FORMAT_VERSION =>
            {
                if kind != OrderIndex::Guard {
                    return Err(SaveError::Invalid("GUARD payload on foreign order kind"));
                }
                let payload = crate::systems::guard_order::GuardOrderState {
                    target: crate::systems::guard_order::GuardIdentity {
                        o: r.i32()?,
                        who: r.i32()?,
                        uid: r.u16()?,
                    },
                    dx: r.i32()?,
                    dy: r.i32()?,
                    guard_x: r.i32()?,
                    guard_y: r.i32()?,
                    idle: r.i32()?,
                    retry: r.i32()?,
                };
                if (payload.target.o, payload.target.who, payload.target.uid)
                    != (
                        i32::from(order.target_o),
                        i32::from(order.target_who),
                        order.target_uid,
                    )
                {
                    return Err(SaveError::Invalid("invalid GUARD payload"));
                }
                (None, None, None, Some(payload), None)
            }
            (DoNSaveOrderPayloadTag::Guard, _)
                if format_version >= ORDER_NODE_METRIC_FORMAT_VERSION =>
            {
                return Err(SaveError::Invalid(
                    "unknown order payload discriminator/version",
                ));
            }
            (
                DoNSaveOrderPayloadTag::Gather
                | DoNSaveOrderPayloadTag::CastSpell
                | DoNSaveOrderPayloadTag::TradeRoute
                | DoNSaveOrderPayloadTag::Guard
                | DoNSaveOrderPayloadTag::AirPatrol
                | DoNSaveOrderPayloadTag::GroupPatrol
                | DoNSaveOrderPayloadTag::Strafe
                | DoNSaveOrderPayloadTag::AttackGround
                | DoNSaveOrderPayloadTag::AirAttackGround,
                _,
            ) => return Err(SaveError::Unsupported("reserved typed order payload")),
            _ => {
                return Err(SaveError::Invalid(
                    "unknown order payload discriminator/version",
                ))
            }
        }
    } else {
        (None, None, None, None, None)
    };
    if let (Some(move_state), Some(form)) = (move_state, form_order) {
        if kind == OrderIndex::ChangeForm && move_state.angle != form.angle {
            return Err(SaveError::Invalid("change-form movement angle mismatch"));
        }
    }
    Ok(Order {
        special_anim,
        form_order,
        follow,
        move_state,
        air_patrol,
        strafe,
        guard,
        economy,
        ..order
    })
}

fn map_air_patrol_save_error(error: air_runtime_authority::AirRuntimeAuthorityError) -> SaveError {
    match error {
        air_runtime_authority::AirRuntimeAuthorityError::PointCountLimit(_)
        | air_runtime_authority::AirRuntimeAuthorityError::PointCountOutOfRange(_) => {
            SaveError::Limit("AIR_PATROL waypoint count")
        }
        _ => SaveError::Invalid("invalid AIR_PATROL payload"),
    }
}

fn map_strafe_save_error(_error: strafe_runtime_authority::StrafeAuthorityError) -> SaveError {
    SaveError::Invalid("invalid STRAFE payload")
}

fn map_economy_payload_save_error(error: economy_payload::EconomyOrderAuthorityError) -> SaveError {
    match error {
        economy_payload::EconomyOrderAuthorityError::UnsupportedDoNSaveVersion(_) => {
            SaveError::Unsupported("economy payload before DoNSave v13")
        }
        _ => SaveError::Invalid("invalid economy order payload"),
    }
}

fn read_economy_payload(
    r: &mut Reader<'_>,
    kind: OrderIndex,
    order: &Order,
    tag: DoNSaveOrderPayloadTag,
    version: u8,
) -> Result<EconomyOrderPayload, SaveError> {
    let mut bytes = vec![tag as u8, version];
    match tag {
        DoNSaveOrderPayloadTag::None => {}
        DoNSaveOrderPayloadTag::Gather => {
            bytes.extend_from_slice(r.take(economy_payload::GATHER_SUFFIX_BYTES)?)
        }
        DoNSaveOrderPayloadTag::CastSpell => {
            bytes.extend_from_slice(r.take(economy_payload::CAST_SUFFIX_BYTES)?)
        }
        DoNSaveOrderPayloadTag::TradeRoute => {
            bytes.extend_from_slice(r.take(economy_payload::TRADE_SUFFIX_BYTES)?);
            let present = r.u8()?;
            bytes.push(present);
            match present {
                0 => {}
                1 => bytes.extend_from_slice(r.take(8)?),
                _ => return Err(SaveError::Invalid("invalid economy order payload")),
            }
        }
        _ => return Err(SaveError::Invalid("invalid economy order payload")),
    }
    let header = EconomyOrderHeader {
        kind,
        flags: order.flags,
        x: order.x,
        y: order.y,
        primary: StableTargetIdentity {
            o: i32::from(order.target_o),
            who: i32::from(order.target_who),
            uid: order.target_uid,
            handle: order.target_handle,
        },
    };
    economy_payload::decode_v13_leaf(economy_payload::DON_SAVE_V13, 0, header, &bytes)
        .map(|node| node.payload)
        .map_err(map_economy_payload_save_error)
}

fn read_air_patrol_coord_array(r: &mut Reader<'_>) -> Result<WalkedCoordArray, SaveError> {
    let len = r.len(MAX_DECODED_PATROL_POINTS, "AIR_PATROL waypoint count")?;
    let increment = r.i16()?;
    let flags = r.u8()?;
    let mut values = Vec::with_capacity(len);
    for _ in 0..len {
        values.push(r.i32()?);
    }
    Ok(WalkedCoordArray {
        increment,
        flags,
        values,
    })
}

fn write_order_node(w: &mut Writer, order: &Order, format_version: u32) -> Result<(), SaveError> {
    if format_version >= ORDER_NODE_METRIC_FORMAT_VERSION {
        w.u8(order.node_metric);
    }
    write_order(w, order, format_version)
}

fn read_order_node(r: &mut Reader<'_>, format_version: u32) -> Result<Order, SaveError> {
    let metric = if format_version >= ORDER_NODE_METRIC_FORMAT_VERSION {
        r.u8()?
    } else {
        0
    };
    let mut order = read_order(r, format_version)?;
    order.node_metric = metric;
    Ok(order)
}

fn write_sparse_object_bands(
    w: &mut Writer,
    snapshot: &SparseRegistrySnapshot<WorldObjectIdentity>,
) -> Result<(), SaveError> {
    if snapshot.owners.len() != crate::objects::OWNER_SLOTS {
        return Err(SaveError::Invalid("sparse object owner count"));
    }
    for active in snapshot.active {
        w.bool(active);
    }
    for owner in &snapshot.owners {
        if owner.bands.len() != RetailBand::ALL.len() {
            return Err(SaveError::Invalid("sparse object band count"));
        }
        for (band_index, band_state) in owner.bands.iter().enumerate() {
            let band = RetailBand::ALL[band_index];
            let capacity = (band.limit() - band.base()) as usize;
            if band_state.mark < band.base()
                || band_state.mark > band.limit()
                || band_state.slots.len() > capacity
            {
                return Err(SaveError::Invalid("sparse object band bounds"));
            }
            w.i32(band_state.mark);
            w.len(band_state.slots.len(), "sparse object slots")?;
            for lifecycle in &band_state.slots {
                match lifecycle {
                    SnapshotLifecycle::Tombstone(facts) => {
                        w.u8(0);
                        w.u8(facts.flags);
                        w.u16(facts.hold_frames);
                        w.bool(facts.is_unit);
                        w.i16(facts.o_up);
                    }
                    SnapshotLifecycle::Live(WorldObjectIdentity::Unit { id, generation })
                        if band == RetailBand::Unit =>
                    {
                        w.u8(1);
                        w.u32(*id);
                        w.u32(*generation);
                    }
                    SnapshotLifecycle::Live(WorldObjectIdentity::BuildRow(row))
                        if band == RetailBand::Build =>
                    {
                        w.u8(2);
                        w.u32(*row);
                    }
                    SnapshotLifecycle::Live(WorldObjectIdentity::WallRow(row))
                        if band == RetailBand::Wall =>
                    {
                        w.u8(3);
                        w.u32(*row);
                    }
                    SnapshotLifecycle::Live(_) => {
                        return Err(SaveError::Invalid("sparse object identity band"));
                    }
                }
            }
        }
    }
    Ok(())
}

fn read_sparse_object_bands(
    r: &mut Reader<'_>,
) -> Result<SparseRegistrySnapshot<WorldObjectIdentity>, SaveError> {
    let mut active = [false; crate::objects::OWNER_SLOTS];
    for value in &mut active {
        *value = r.bool()?;
    }
    let mut owners = Vec::with_capacity(crate::objects::OWNER_SLOTS);
    for _owner in 0..crate::objects::OWNER_SLOTS {
        let mut bands = Vec::with_capacity(RetailBand::ALL.len());
        for band in RetailBand::ALL {
            let mark = r.i32()?;
            if mark < band.base() || mark > band.limit() {
                return Err(SaveError::Invalid("sparse object band mark"));
            }
            let capacity = (band.limit() - band.base()) as usize;
            let count = r.len(capacity, "sparse object slots")?;
            if (mark - band.base()) as usize > count {
                return Err(SaveError::Invalid("sparse object mark exceeds storage"));
            }
            let mut slots = Vec::with_capacity(count);
            for _ in 0..count {
                let lifecycle = match r.u8()? {
                    0 => SnapshotLifecycle::Tombstone(TombstoneFacts {
                        flags: r.u8()?,
                        hold_frames: r.u16()?,
                        is_unit: r.bool()?,
                        o_up: r.i16()?,
                    }),
                    1 if band == RetailBand::Unit => {
                        SnapshotLifecycle::Live(WorldObjectIdentity::Unit {
                            id: r.u32()?,
                            generation: r.u32()?,
                        })
                    }
                    2 if band == RetailBand::Build => {
                        SnapshotLifecycle::Live(WorldObjectIdentity::BuildRow(r.u32()?))
                    }
                    3 if band == RetailBand::Wall => {
                        SnapshotLifecycle::Live(WorldObjectIdentity::WallRow(r.u32()?))
                    }
                    _ => return Err(SaveError::Invalid("sparse object lifecycle tag")),
                };
                slots.push(lifecycle);
            }
            bands.push(SparseBandSnapshot { mark, slots });
        }
        owners.push(SparseOwnerSnapshot { bands });
    }
    Ok(SparseRegistrySnapshot { active, owners })
}

fn write_world_state_for_version(
    state: &WorldSaveState,
    format_version: u32,
) -> Result<Vec<u8>, SaveError> {
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
            write_order_node(&mut w, order, format_version)?;
        }
    }
    for rows in &state.build_rows {
        w.len(rows.len(), "build registry rows")?;
        for &row in rows {
            w.u32(row);
        }
    }
    match format_version {
        SPARSE_OBJECTS_FORMAT_VERSION..=FORMAT_VERSION => {
            let object_bands = state
                .object_bands
                .as_ref()
                .ok_or(SaveError::Invalid("missing sparse object bands"))?;
            write_sparse_object_bands(&mut w, object_bands)?;
        }
        LEGACY_DENSE_OBJECTS_FORMAT_VERSION => {}
        _ => return Err(SaveError::Invalid("unsupported save format version")),
    }
    Ok(w.0)
}

fn write_world_state(state: &WorldSaveState) -> Result<Vec<u8>, SaveError> {
    write_world_state_for_version(state, FORMAT_VERSION)
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
    format_version: u32,
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
            list.push(read_order_node(&mut r, format_version)?);
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
    let object_bands = match format_version {
        SPARSE_OBJECTS_FORMAT_VERSION..=FORMAT_VERSION => Some(read_sparse_object_bands(&mut r)?),
        LEGACY_DENSE_OBJECTS_FORMAT_VERSION => None,
        _ => return Err(SaveError::Invalid("unsupported save format version")),
    };
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
        object_bands,
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
    validate_build_unit_containment(builds, world)?;
    validate_build_city_links(builds, world)?;
    Ok(())
}

fn saved_unit_row(world: &WorldSaveState, who: u8, o: i16) -> Option<usize> {
    (0..world.live as usize)
        .find(|&row| world.units.get_who(row) == who && world.units.o()[row] == o)
}

/// Admit only the exact Build-garrison shape needed by AIR packages: a Build head followed by an
/// acyclic Unit `inside_down` chain whose every child has the reciprocal Build `inside_up` link.
/// The bytes were already carried by the Build and Unit save images; this closes validation
/// without silently admitting nested buildings, wrong owners, duplicate children, or tombstones.
fn validate_build_unit_containment(
    builds: &[production::BuildData],
    world: &WorldSaveState,
) -> Result<(), SaveError> {
    let mut claimed = vec![false; world.live as usize];
    for build in builds {
        let mut next_o = i16::from_le_bytes([build.other[0x28], build.other[0x29]]);
        let mut next_who = build.other[0x3e] as i8;
        let mut walked = 0usize;
        while next_o >= 0 {
            let who = u8::try_from(next_who).map_err(|_| {
                SaveError::Builds("Build containment has a negative child owner".into())
            })?;
            let row = saved_unit_row(world, who, next_o).ok_or_else(|| {
                SaveError::Builds("Build containment points outside the live Unit band".into())
            })?;
            if world.units.get_flags(row) & crate::world::OBJ_FLAG_ACTIVE == 0 {
                return Err(SaveError::Builds(
                    "Build containment points at an inactive Unit".into(),
                ));
            }
            if std::mem::replace(&mut claimed[row], true) {
                return Err(SaveError::Builds(
                    "Build containment repeats a Unit or contains a cycle".into(),
                ));
            }
            if world.units.inside_up()[row] != build.object_id()
                || world.units.inside_up_who()[row] != build.who as i8
            {
                return Err(SaveError::Builds(
                    "Build containment child lacks the reciprocal inside_up link".into(),
                ));
            }
            walked += 1;
            if walked > world.live as usize {
                return Err(SaveError::Builds(
                    "Build containment chain exceeds the live Unit pool".into(),
                ));
            }
            next_o = world.units.inside_down()[row];
            next_who = world.units.inside_down_who()[row];
        }
    }
    Ok(())
}

fn build_row_for_object(world: &WorldSaveState, owner: usize, o: i16) -> Option<usize> {
    let slot = i32::from(o).checked_sub(crate::objects::BUILD_BAND_BASE as i32)?;
    let slot = usize::try_from(slot).ok()?;
    world
        .build_rows
        .get(owner)?
        .get(slot)
        .map(|&row| row as usize)
}

/// Validate the Build-owned half of `BuildData::city_down` before a CityPool is available.
/// Every edge must stay inside the same owner's Build band and the same City slot; every node
/// has at most one predecessor and every chain is acyclic.
fn validate_build_city_links(
    builds: &[production::BuildData],
    world: &WorldSaveState,
) -> Result<(), SaveError> {
    let mut predecessors = vec![0u8; builds.len()];
    for build in builds {
        if build.city < 0 && build.city_down >= 0 {
            return Err(SaveError::Builds(
                "unlinked Build has a city_down successor".into(),
            ));
        }
        if build.city_down < 0 {
            continue;
        }
        let row = build_row_for_object(world, build.who as usize, build.city_down).ok_or(
            SaveError::Builds("city_down points outside the owner's Build band".into()),
        )?;
        let successor = &builds[row];
        if successor.who != build.who || successor.city != build.city {
            return Err(SaveError::Builds(
                "city_down crosses an owner or City boundary".into(),
            ));
        }
        predecessors[row] = predecessors[row].saturating_add(1);
        if predecessors[row] > 1 {
            return Err(SaveError::Builds(
                "Build has more than one city_down predecessor".into(),
            ));
        }
    }

    for start in 0..builds.len() {
        let mut seen = vec![false; builds.len()];
        let mut row = start;
        loop {
            if std::mem::replace(&mut seen[row], true) {
                return Err(SaveError::Builds("city_down chain contains a cycle".into()));
            }
            let next = builds[row].city_down;
            if next < 0 {
                break;
            }
            row = build_row_for_object(world, builds[row].who as usize, next).ok_or(
                SaveError::Builds("city_down points outside the owner's Build band".into()),
            )?;
        }
    }
    Ok(())
}

/// Bind every nonnegative Build City link to the checksum-owned CityPool and prove that every
/// chain is rooted at that City's center Build. This closes the save/load boundary opened by
/// canonical opcode-25 construction without admitting arbitrary serialized links.
fn validate_build_city_pool(
    builds: &[production::BuildData],
    world: &WorldSaveState,
    cities: &crate::systems::tech_cities::CityPool,
) -> Result<(), SaveError> {
    validate_build_city_links(builds, world)?;
    let mut reached = vec![false; builds.len()];
    for owner in 0..crate::objects::BANDED_SLOTS {
        let mark = usize::try_from(cities.city_mark[owner])
            .map_err(|_| SaveError::Builds("negative City high-water mark".into()))?;
        if mark > cities.slots[owner].len() {
            return Err(SaveError::Builds(
                "City high-water mark exceeds its canonical pool".into(),
            ));
        }
        for (slot, city) in cities.slots[owner][..mark].iter().enumerate() {
            if !city.active() {
                continue;
            }
            if city.who != owner as i8 || city.city != slot as i16 {
                return Err(SaveError::Builds(
                    "active City identity disagrees with its canonical pool slot".into(),
                ));
            }
            // A retained City record may outlive every Build in its old chain (for example the
            // former-capital record used by defeat/capture history). It has no Build-owned edge
            // to validate. Once any Build names this City, however, the saved center must root
            // the whole exact chain below.
            if !builds
                .iter()
                .any(|build| build.who as usize == owner && build.city == slot as i16)
            {
                continue;
            }
            let mut row = build_row_for_object(world, owner, city.o).ok_or(SaveError::Builds(
                "active City center is missing from its owner's Build band".into(),
            ))?;
            loop {
                let build = &builds[row];
                if build.who as usize != owner || build.city != slot as i16 {
                    return Err(SaveError::Builds(
                        "City chain Build disagrees with its root City".into(),
                    ));
                }
                if std::mem::replace(&mut reached[row], true) {
                    return Err(SaveError::Builds(
                        "Build is reachable from more than one active City".into(),
                    ));
                }
                if build.city_down < 0 {
                    break;
                }
                row = build_row_for_object(world, owner, build.city_down).ok_or(
                    SaveError::Builds("City chain leaves its owner's Build band".into()),
                )?;
            }
        }
    }
    if builds
        .iter()
        .enumerate()
        .any(|(row, build)| build.city >= 0 && !reached[row])
    {
        return Err(SaveError::Builds(
            "City-linked Build is not reachable from an active City center".into(),
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

    // The CAPTURED/converted flag is already an explicit byte in this section and has no
    // subordinate allocation to reconstruct. Fresh retail construction sites carry it, so
    // rejecting the value made an otherwise byte-owned BUILD_AT state unloadable.
    // `BuildData::dock/farm/fort/oil_well` is one serialized i16 union.  A completed Farm
    // (`orig_type == 0x1A1`) uses it only as the `Farms::farm_data` index.  The canonical
    // Gather-Farm adapter binds that index to the canonical v16 FarmStruct owner plus a
    // revision/digest Guy/content authority after load. Preserve this already-owned scalar;
    // keep every other union interpretation closed.
    let unsupported_special_index = build.dock >= 0 && build.orig_type != 0x1a1;
    if build.build_masks & (production::mask::EJECTING | production::mask::OWNERSHIP_LATCH) != 0
        || build.demolition != 0
        || build.gather_down >= 0
        || build.wonder >= 0
        || unsupported_special_index
        || build.attack_ox >= 0
        || build.attack_whom >= 0
        || build.gather_max != 0
        || build.infiltrate != 0
        || build.infiltrate2 != 0
        || build.gather_from.mtn != 0
        || build.gather_from.cliff != 0
        || !build.gather_from.tiles.is_empty()
        || !build.gather.is_empty()
    {
        return Err(SaveError::Builds(
            "wonder/gather/special-family building state is not owned".into(),
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

fn write_farms(farms: &crate::systems::canonical_gather_work::Farms) -> Result<Vec<u8>, SaveError> {
    let (length, capacity, increment, flags) = farms.header();
    if length < 0 || capacity < length || capacity > i16::MAX as i32 || flags != 0 {
        return Err(SaveError::Invalid("Farms array header"));
    }
    let mut w = Writer::default();
    w.i32(length);
    w.i32(capacity);
    w.i16(increment);
    w.u8(flags);
    for farm in farms.records() {
        w.bytes(&farm.image());
    }
    Ok(w.0)
}

fn read_farms(data: &[u8]) -> Result<crate::systems::canonical_gather_work::Farms, SaveError> {
    use crate::systems::canonical_gather_work::{FarmStruct, Farms};

    let mut r = Reader::new(data);
    let length = r.i32()?;
    let capacity = r.i32()?;
    let increment = r.i16()?;
    let flags = r.u8()?;
    if length < 0 || capacity < length || capacity > i16::MAX as i32 || flags != 0 {
        return Err(SaveError::Invalid("Farms array header"));
    }
    let mut farms = Farms::with_header(capacity, increment, flags);
    for _ in 0..length {
        let image = r.take(FarmStruct::WALKED_BYTES)?;
        let mut farm = FarmStruct {
            who: i32::from_le_bytes(image[0..4].try_into().unwrap()),
            o: i32::from_le_bytes(image[4..8].try_into().unwrap()),
            valid: image[188],
            farm_type: image[189],
            ..FarmStruct::default()
        };
        for index in 0..16 {
            farm.percent[index] =
                u32::from_le_bytes(image[8 + index * 4..12 + index * 4].try_into().unwrap());
        }
        for index in 0..25 {
            farm.terrain_height[index] =
                u32::from_le_bytes(image[72 + index * 4..76 + index * 4].try_into().unwrap());
        }
        farm.status.copy_from_slice(&image[172..188]);
        farms
            .push(farm)
            .map_err(|_| SaveError::Invalid("Farms array capacity"))?;
    }
    r.finish()?;
    Ok(farms)
}

fn validate_farm_bindings(
    builds: &[production::BuildData],
    farms: &crate::systems::canonical_gather_work::Farms,
) -> Result<(), SaveError> {
    for build in builds {
        if build.orig_type == crate::systems::canonical_gather_work::FARM_PROPERTY
            && build.dock >= 0
        {
            let farm = farms
                .get(build.dock as usize)
                .ok_or(SaveError::Invalid("Farm Build index"))?;
            if (farm.valid, farm.who, farm.o)
                != (1, i32::from(build.who), i32::from(build.object_id()))
            {
                return Err(SaveError::Invalid("Farm Build/record identity"));
            }
        }
    }
    for (index, farm) in farms
        .records()
        .iter()
        .enumerate()
        .filter(|(_, farm)| farm.valid != 0)
    {
        let matches = builds
            .iter()
            .filter(|build| {
                build.orig_type == crate::systems::canonical_gather_work::FARM_PROPERTY
                    && build.dock == index as i16
                    && i32::from(build.who) == farm.who
                    && i32::from(build.object_id()) == farm.o
            })
            .count();
        if matches != 1 {
            return Err(SaveError::Invalid("orphan or duplicate Farm record"));
        }
    }
    Ok(())
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

fn leader_is_supported(l: &LeaderSlot, expected_active: bool) -> bool {
    let mut border = l.border;
    let border_active = border.active;
    border.active = false;
    l.active == expected_active
        && border_active == expected_active
        && gather_inputs_are_default(&l.gather_inputs)
        && l.cap_gates == economy::CapGates::default()
        && l.gather_ctx == economy::DoGatherContext::default()
        && border_input_is_default(&border)
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
    let configured = sim.vic_leaders.setup_owner.configured_mask();
    if sim
        .leaders
        .iter()
        .enumerate()
        .any(|(who, l)| !leader_is_supported(l, configured & (1u8 << who) != 0))
    {
        return Err(SaveError::Unsupported(
            "non-canonical leader activation or non-default leader input hosts",
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

/// Minimal reconstructive owner for the frame-zero PlayerSetup transaction. All large
/// diplomacy/treaty rows are deterministic outputs and are deliberately rebuilt by the
/// canonical transaction on load rather than serialized a second time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PlayerSetupSaveState {
    request: ManualPlayerSetup,
    options: MatchOptions,
    semaphore: u32,
}

fn canonical_player_setup_snapshot(sim: &Sim) -> Result<Option<PlayerSetupSaveState>, SaveError> {
    let Some(applied) = sim.vic_leaders.setup_owner.applied() else {
        return Ok(None);
    };

    // Reconstruct only the setup-time semaphore facts the transaction actually read.
    // Runtime bits (game over, victory resolved, quit/drop state) belong to LEADER_MATCH;
    // feeding them back into frame zero would turn current state into invented setup input.
    let mut setup_semaphore = 0u32;
    if applied.state.setup.semaphore_820 & 0x04 != 0 {
        setup_semaphore |= 1u32 << crate::systems::victory_score::game_sem::NET_OR_RECORDING;
    }
    if applied.diplomacy.facts[0].scenario_rules {
        setup_semaphore |= 1u32 << crate::systems::victory_score::game_sem::SCENARIO_RULES;
    }
    if applied.diplomacy.facts[0].check_victory_mode {
        setup_semaphore |= 1u32 << crate::systems::victory_score::game_sem::CHECK_VICTORY_MODE;
    }

    // `Player::resign` may change MatchOptions::team_style after setup.  The setup
    // recipe must retain the input that produced its immutable receipt; LEADER_MATCH
    // separately owns the current runtime option byte.
    let mut setup_options = sim.vic_match.options;
    setup_options.team_style = applied.request.team_style;
    let snapshot = PlayerSetupSaveState {
        request: applied.request,
        options: setup_options,
        semaphore: setup_semaphore,
    };
    let mut expected = Sim::new(0, 1);
    expected.vic_match.options = snapshot.options;
    expected.vic_match.semaphore = snapshot.semaphore;
    expected
        .start_manual_player_setup(snapshot.request)
        .map_err(|_| SaveError::Invalid("player setup reconstruction"))?;

    let expected_applied = expected.vic_leaders.setup_owner.applied().unwrap();
    if applied != expected_applied {
        return Err(SaveError::Invalid("divergent player setup owner"));
    }

    for who in 0..SETUP_SLOTS {
        let active = snapshot.request.active_mask & (1u8 << who) != 0;
        let actual = &sim.vic_leaders.slots[who];
        if (actual.leader_flags & crate::systems::victory_score::leader_flag::VALID != 0) != active
            || sim.leaders[who].active != active
            || sim.leaders[who].border.active != active
            || sim.world.objects.is_active(who) != active
            || (active && sim.map.fog.leaders[who].player_mask & (1u8 << who) == 0)
            || (!active && sim.map.fog.leaders[who].player_mask != 0)
        {
            return Err(SaveError::Invalid("divergent player setup projection"));
        }
    }
    Ok(Some(snapshot))
}

fn write_match_options(w: &mut Writer, options: MatchOptions) {
    for byte in [
        options.team_style,
        options.game_rules,
        options.starting_resources,
        options.reveal_map,
        options.rush_rules,
        options.starting_technology,
        options.starting_technology2,
        options.ending_technology,
        options.elimination,
        options.victory,
        options.wonderwin,
        options.score_goal,
        options.popwin,
        options.time_limit,
        options.chairs,
        options.econwin,
    ] {
        w.u8(byte);
    }
}

fn read_match_options(r: &mut Reader<'_>) -> Result<MatchOptions, SaveError> {
    Ok(MatchOptions {
        team_style: r.u8()?,
        game_rules: r.u8()?,
        starting_resources: r.u8()?,
        reveal_map: r.u8()?,
        rush_rules: r.u8()?,
        starting_technology: r.u8()?,
        starting_technology2: r.u8()?,
        ending_technology: r.u8()?,
        elimination: r.u8()?,
        victory: r.u8()?,
        wonderwin: r.u8()?,
        score_goal: r.u8()?,
        popwin: r.u8()?,
        time_limit: r.u8()?,
        chairs: r.u8()?,
        econwin: r.u8()?,
    })
}

fn write_player_setup(sim: &Sim) -> Result<Vec<u8>, SaveError> {
    let mut w = Writer::default();
    let Some(snapshot) = canonical_player_setup_snapshot(sim)? else {
        w.bool(false);
        return Ok(w.0);
    };
    w.bool(true);
    w.u8(snapshot.request.active_mask);
    for team in snapshot.request.teams {
        w.i8(team);
    }
    w.u8(snapshot.request.team_style);
    w.u8(u8::try_from(snapshot.request.local_player_setup_slot)
        .map_err(|_| SaveError::Invalid("player setup local slot"))?);
    w.bool(snapshot.request.ranked);
    w.u8(snapshot.request.shared_vision_preq_mask);
    write_match_options(&mut w, snapshot.options);
    w.u32(snapshot.semaphore);
    Ok(w.0)
}

fn read_player_setup(data: &[u8]) -> Result<Option<PlayerSetupSaveState>, SaveError> {
    let mut r = Reader::new(data);
    if !r.bool()? {
        r.finish()?;
        return Ok(None);
    }
    let active_mask = r.u8()?;
    let mut teams = [0i8; SETUP_SLOTS];
    for team in &mut teams {
        *team = r.i8()?;
    }
    let request = ManualPlayerSetup {
        active_mask,
        teams,
        team_style: r.u8()?,
        local_player_setup_slot: r.u8()? as usize,
        ranked: r.bool()?,
        shared_vision_preq_mask: r.u8()?,
    };
    let options = read_match_options(&mut r)?;
    let semaphore = r.u32()?;
    r.finish()?;
    Ok(Some(PlayerSetupSaveState {
        request,
        options,
        semaphore,
    }))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CoreState {
    format_version: u32,
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
    let format_version = r.u32()?;
    if !(LEGACY_DENSE_OBJECTS_FORMAT_VERSION..=FORMAT_VERSION).contains(&format_version) {
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
        format_version,
        seed,
        frame,
        seconds,
        random_state,
        game_daemon,
        groups_proc_group,
    })
}

fn reject_unsupported(sim: &Sim) -> Result<(), SaveError> {
    if !sim.command_package_state.is_empty() && sim.players.is_none() {
        return Err(SaveError::Unsupported(
            "command selection cache without player mapping",
        ));
    }
    if sim.step12_visibility
        != crate::systems::step12_visibility_runtime::Step12VisibilityAuthority::default()
    {
        // Detector-init provenance, exact leader/HeroesData joins, and synchronized type
        // composition are mandatory after load. Refuse this tranche until it owns those bytes;
        // reconstructing `detector:false` would corrupt checksum channel 12 on a later refresh.
        return Err(SaveError::Unsupported("step-12 visibility authority"));
    }
    canonical_player_setup_snapshot(sim)?;
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
    groups::validate(&sim.groups)?;
    armies::validate(&sim.armies, &sim.groups)?;
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
    validate_build_city_pool(&sim.builds, &state, &sim.cities)?;
    validate_farm_bindings(&sim.builds, &sim.farms)?;
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
            Chunk::leaf(PLAYER_SETUP, write_player_setup(sim)?),
            Chunk::leaf(GROUPS, groups::write(&sim.groups)?),
            Chunk::leaf(LEADER_MATCH, leader_match::write(sim)?),
            Chunk::leaf(
                COMMAND_PACKAGE_STATE,
                command_package_state::write(&sim.command_package_state)?,
            ),
            Chunk::leaf(DIPLOMACY, encode_diplomacy_payload(&sim.diplomacy)),
            Chunk::leaf(
                SCENARIO_IGNORES,
                air_runtime_authority::encode_scenario_ignores(&sim.scenario_ignore_orders)
                    .map_err(|_| SaveError::Invalid("scenario ignore-orders state"))?,
            ),
            Chunk::leaf(FARMS, write_farms(&sim.farms)?),
            Chunk::leaf(ARMIES, armies::write(&sim.armies, &sim.groups)?),
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

/// Serialize a simulation that has an external persistent script runtime.
///
/// The ordinary `save_sim` entry point predates script ownership and cannot inspect a runtime
/// supplied separately to `Sim::do_frame_with_scripts`.  Script-bearing callers must use this
/// combined boundary: it rejects every installed type owner until DoNSave can restore that owner
/// and its synchronized rules/mod provenance, then delegates to the current deterministic writer.
pub fn save_sim_with_scripts(sim: &Sim, scripts: &ScriptRuntime) -> Result<Vec<u8>, SaveError> {
    scripts.admit_type_state_for_save()?;
    save_sim(sim)
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
    if root.header.id != ROOT
        || !(LEGACY_REQUIRED.len()..=REQUIRED.len()).contains(&(root.header.num_chunks as usize))
    {
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
    let core = read_core(sections[0].ok_or(SaveError::MissingChunk(CORE))?)?;
    let required = required_sections(core.format_version);
    if root.children.len() != required.len() {
        return Err(SaveError::InvalidChunk(
            "root child count does not match format version",
        ));
    }
    for (index, &id) in REQUIRED.iter().enumerate() {
        match (required.contains(&id), sections[index].is_some()) {
            (true, false) => return Err(SaveError::MissingChunk(id)),
            (false, true) => return Err(SaveError::UnknownChunk(id)),
            _ => {}
        }
    }
    let map = sections[1].unwrap();
    let objects = sections[2].unwrap();
    let leaders = sections[3].unwrap();
    let paths = sections[4].unwrap();
    let items = sections[5].unwrap();
    let builds = sections[6].unwrap();
    let map = read_map(map)?;
    if map.world.seed != core.seed {
        return Err(SaveError::Invalid("core/map seed mismatch"));
    }
    let world_state = read_world_state(
        objects,
        core.format_version,
        core.frame,
        core.seconds,
        core.random_state,
    )?;
    let builds = read_builds(builds, &world_state)?;
    let expected_types = world_state.unit_type_id.clone();
    let (leaders, market) = read_leaders(leaders)?;
    let (unit_type, paths, path_unit) = read_paths(paths, &expected_types)?;
    let item_runtime = read_items(items, &map.world)?;
    let player_setup = match sections[7] {
        Some(data) => read_player_setup(data)?,
        None => None,
    };
    // A stream older than `GROUPS` carried the section's whole reachable state in its
    // refusal: `reject_unsupported` would not write one unless the pool was constructor
    // state, so the default pool is a faithful decode of what those bytes meant.
    let group_pool = match sections[8] {
        Some(data) => groups::read(data, core.groups_proc_group)?,
        None => groups_guys::Groups {
            proc_group: core.groups_proc_group,
            ..groups_guys::Groups::default()
        },
    };
    let leader_match = match sections[9] {
        Some(data) => Some(leader_match::read(data, core.format_version)?),
        None => None,
    };
    let command_state = match sections[10] {
        Some(data) => command_package_state::read(data)?,
        None => crate::systems::canonical_group_move_host::CommandPackageState::default(),
    };
    let diplomacy = decode_diplomacy_for_save(core.format_version, sections[11])
        .map_err(|_| SaveError::Invalid("diplomacy payload"))?;
    let scenario_ignore_orders = match sections[12] {
        Some(data) if core.format_version >= SCENARIO_IGNORES_FORMAT_VERSION => {
            air_runtime_authority::decode_scenario_ignores(data)
                .map_err(|_| SaveError::Invalid("scenario ignore-orders payload"))?
        }
        None if core.format_version < SCENARIO_IGNORES_FORMAT_VERSION => {
            air_runtime_authority::ScenarioIgnoreOrdersAuthority::default()
        }
        _ => return Err(SaveError::Invalid("scenario ignore-orders section version")),
    };
    let farms = match sections[13] {
        Some(data) if core.format_version >= FARMS_FORMAT_VERSION => read_farms(data)?,
        None if core.format_version < FARMS_FORMAT_VERSION => {
            crate::systems::canonical_gather_work::Farms::default()
        }
        _ => return Err(SaveError::Invalid("Farms section version")),
    };
    let armies = match sections[14] {
        Some(data) if core.format_version >= ARMIES_FORMAT_VERSION => {
            armies::read(data, &group_pool)?
        }
        None if core.format_version < ARMIES_FORMAT_VERSION => {
            crate::systems::armies::Armies::new()
        }
        _ => return Err(SaveError::Invalid("Armies section version")),
    };
    validate_farm_bindings(&builds, &farms)?;
    if let Some(setup) = player_setup {
        if leader_match.is_none() && core.frame != 0 {
            return Err(SaveError::Invalid("player setup outside frame zero"));
        }
        for who in 0..SETUP_SLOTS {
            let active = setup.request.active_mask & (1u8 << who) != 0;
            if world_state.active_slots[who] != active
                || world_state
                    .object_bands
                    .as_ref()
                    .is_none_or(|bands| bands.active[who] != active)
                || (active && map.fog.leaders[who].player_mask & (1u8 << who) == 0)
                || (!active && map.fog.leaders[who].player_mask != 0)
            {
                return Err(SaveError::Invalid(
                    "player setup activation projection mismatch",
                ));
            }
        }
    }

    let mut sim = Sim::new(core.seed as u32 as u64, map.world.xs as u16);
    if let Some(setup) = player_setup {
        // Rebuild the immutable setup receipt at its real frame-zero boundary. Saved
        // world/map owners are installed afterwards, then LEADER_MATCH restores the
        // mutable runtime rows and clocks over this one canonical setup owner.
        sim.vic_match.options = setup.options;
        sim.vic_match.semaphore = setup.semaphore;
        sim.start_manual_player_setup(setup.request)
            .map_err(|_| SaveError::Invalid("player setup reconstruction"))?;
        let setup_sem_mask = (1u32 << crate::systems::victory_score::game_sem::NET_OR_RECORDING)
            | (1u32 << crate::systems::victory_score::game_sem::SCENARIO_RULES)
            | (1u32 << crate::systems::victory_score::game_sem::CHECK_VICTORY_MODE);
        let setup_mismatch = if leader_match.is_some() {
            setup.semaphore & !setup_sem_mask != 0
                || sim.vic_match.options != setup.options
                || sim.vic_match.semaphore & setup_sem_mask != setup.semaphore
        } else {
            sim.vic_match.options != setup.options || sim.vic_match.semaphore != setup.semaphore
        };
        if setup_mismatch {
            return Err(SaveError::Invalid(
                "player setup derived options/semaphore mismatch",
            ));
        }
    }
    sim.map = map;
    sim.world.import_save_state(world_state)?;
    sim.world.item_runtime = item_runtime;
    sim.builds = builds;
    sim.leaders = leaders;
    if let Some(setup) = player_setup {
        for who in 0..SETUP_SLOTS {
            let active = setup.request.active_mask & (1u8 << who) != 0;
            sim.leaders[who].active = active;
            sim.leaders[who].border.active = active;
        }
    }
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
    sim.groups = group_pool;
    sim.armies = armies;
    sim.command_package_state = command_state;
    sim.diplomacy = diplomacy;
    sim.scenario_ignore_orders = scenario_ignore_orders;
    sim.farms = farms;
    if let Some(state) = leader_match {
        leader_match::restore(&mut sim, state)?;
    } else {
        // Formats 7-10 did not carry the Match owner.  The CORE clock is nevertheless
        // authoritative, so install its exact projection when upgrading the stream rather
        // than manufacturing a frame-zero Match beside a later World.
        sim.vic_match.frame = sim.world.frame;
        sim.vic_match.tick = sim.world.seconds;
    }
    let restored_world = sim.world.export_save_state()?;
    validate_build_city_pool(&sim.builds, &restored_world, &sim.cities)?;
    if !sim.command_package_state.is_empty() && sim.players.is_none() {
        return Err(SaveError::Invalid(
            "command selection cache without player mapping",
        ));
    }
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
        sim.vic_match.frame = 7;
        sim.vic_match.tick = 2;
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

    /// Re-encode version-sensitive leaves and retain exactly that version's root shape.
    fn prior_format_stream(sim: &Sim, format_version: u32) -> Vec<u8> {
        let state = sim.world.export_save_state().unwrap();
        let current = save_sim(sim).unwrap();
        let parsed = parse_chunk(&current[MAGIC.len()..]).unwrap();
        let children = parsed
            .children
            .iter()
            .filter(|child| required_sections(format_version).contains(&child.header.id))
            .map(|child| {
                let mut data = child.data.to_vec();
                if child.header.id == CORE {
                    data[..4].copy_from_slice(&format_version.to_le_bytes());
                } else if child.header.id == OBJECTS {
                    data = write_world_state_for_version(&state, format_version).unwrap();
                } else if child.header.id == LEADER_MATCH {
                    data = leader_match::write_for_version(sim, format_version).unwrap();
                }
                Chunk::leaf(child.header.id, data)
            })
            .collect();
        let root = Chunk::branch(ROOT, children).encode().unwrap();
        let mut legacy = MAGIC.to_vec();
        legacy.extend_from_slice(&root);
        legacy
    }

    fn format_eleven_stream(sim: &Sim) -> Vec<u8> {
        prior_format_stream(sim, LEGACY_ORDER_FORMAT_VERSION)
    }

    fn encoded_root(children: Vec<Chunk>) -> Vec<u8> {
        let root = Chunk::branch(ROOT, children).encode().unwrap();
        let mut bytes = MAGIC.to_vec();
        bytes.extend_from_slice(&root);
        bytes
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

    fn sim_with_city_build_chain() -> (Sim, usize, usize) {
        let mut sim = supported_sim();
        let center = install_build(&mut sim, 2, ordinary_build(true));
        let farm = install_build(&mut sim, 2, ordinary_build(false));
        let center_o = sim.builds[center].object_id();
        let farm_o = sim.builds[farm].object_id();
        sim.builds[center].city = 0;
        sim.builds[center].city_down = farm_o;
        sim.builds[farm].city = 0;
        sim.builds[farm].city_down = -1;
        let city = &mut sim.cities.slots[2][0];
        city.city_flags = 1;
        city.city = 0;
        city.o = center_o;
        city.who = 2;
        sim.cities.city_mark[2] = 1;
        (sim, center, farm)
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
    fn pre_target_identity_v5_and_v6_streams_are_rejected() {
        let bytes = save_sim(&supported_sim()).unwrap();
        let core = section_offset(&bytes, CORE);
        for version in [5u32, 6] {
            let mut legacy = bytes.clone();
            legacy[core + 8..core + 12].copy_from_slice(&version.to_le_bytes());
            assert_eq!(
                load_error(&legacy),
                SaveError::Invalid("unsupported save format version")
            );
        }
    }

    #[test]
    fn legacy_format_seven_dense_objects_convert_to_canonical_sparse_state() {
        let original = supported_sim();
        let state = original.world.export_save_state().unwrap();
        let payload =
            write_world_state_for_version(&state, LEGACY_DENSE_OBJECTS_FORMAT_VERSION).unwrap();
        let decoded = read_world_state(
            &payload,
            LEGACY_DENSE_OBJECTS_FORMAT_VERSION,
            original.world.frame,
            original.world.seconds,
            original.world.random.state(),
        )
        .unwrap();
        assert!(decoded.object_bands.is_none());

        let mut converted = crate::World::with_capacity(1, 0);
        converted.import_save_state(decoded).unwrap();
        assert!(converted.object_bands_are_dense_equivalent());
        assert!(converted
            .export_save_state()
            .unwrap()
            .object_bands
            .is_some());
    }

    #[test]
    fn format_eleven_nonmove_orders_upgrade_but_an_active_move_cannot_be_laundered() {
        let ordinary = supported_sim();
        let ordinary_loaded = load_sim(&format_eleven_stream(&ordinary)).unwrap();
        let upgraded = save_sim(&ordinary_loaded).unwrap();
        assert_eq!(
            read_core(&parse_chunk(&upgraded[MAGIC.len()..]).unwrap().children[0].data)
                .unwrap()
                .format_version,
            FORMAT_VERSION
        );

        let mut active_move = supported_sim();
        active_move
            .world
            .orders_mut(0)
            .replace(Order::move_to(4_000, 5_000, 17));
        let legacy = format_eleven_stream(&active_move);
        let loaded = load_sim(&legacy).unwrap();
        let decoded = loaded.world.orders(0).current().unwrap().clone();
        assert_eq!(decoded.kind, OrderIndex::MoveTo);
        assert_eq!(decoded.move_state, None);
        let before = loaded.world.orders(0).clone();
        assert_eq!(
            save_sim(&loaded),
            Err(SaveError::Invalid("missing movement payload"))
        );
        assert_eq!(loaded.world.orders(0), &before);
    }

    #[test]
    fn v13_root_and_leader_rows_roundtrip_exactly_with_only_new_v14_state_defaulted() {
        let mut original = supported_sim();
        original.vic_leaders.slots[2].leader_flags2 = 0x1357_2468;

        let v13 = prior_format_stream(&original, PRE_DIPLOMACY_SAVE_FORMAT_VERSION);
        let parsed = parse_chunk(&v13[MAGIC.len()..]).unwrap();
        assert_eq!(parsed.children.len(), 11);
        assert!(parsed
            .children
            .iter()
            .all(|child| child.header.id != DIPLOMACY));

        let loaded = load_sim(&v13).unwrap();
        let row = &loaded.vic_leaders.slots[2];
        assert_eq!(row.leader_flags2, 0x1357_2468);
        assert_eq!(
            [
                row.production_step,
                row.prod_script_run,
                row.script_step,
                row.control,
                row.effective_pop,
            ],
            [0; 5]
        );
        assert_eq!(
            loaded.diplomacy,
            crate::systems::canonical_diplomacy_host::DiplomacyPersistentState::default()
        );
        assert_eq!(
            prior_format_stream(&loaded, PRE_DIPLOMACY_SAVE_FORMAT_VERSION),
            v13
        );

        let mut unrepresentable = loaded;
        unrepresentable.vic_leaders.slots[2].production_step = 1;
        assert_eq!(
            leader_match::write_for_version(&unrepresentable, PRE_DIPLOMACY_SAVE_FORMAT_VERSION),
            Err(SaveError::Unsupported("production AI Leader extension"))
        );
    }

    #[test]
    fn rooted_city_build_chain_roundtrips_exactly_in_v13_and_v14() {
        let (original, center, farm) = sim_with_city_build_chain();

        let v14 = save_sim(&original).unwrap();
        let loaded_v14 = load_sim(&v14).unwrap();
        assert_eq!(loaded_v14.builds[center].city, 0);
        assert_eq!(loaded_v14.builds[center].city_down, 2001);
        assert_eq!(loaded_v14.builds[farm].city, 0);
        assert_eq!(loaded_v14.builds[farm].city_down, -1);
        assert_eq!(save_sim(&loaded_v14).unwrap(), v14);

        let v13 = prior_format_stream(&original, PRE_DIPLOMACY_SAVE_FORMAT_VERSION);
        let loaded_v13 = load_sim(&v13).unwrap();
        assert_eq!(loaded_v13.builds[center].city_down, 2001);
        assert_eq!(loaded_v13.builds[farm].city, 0);
        assert_eq!(
            prior_format_stream(&loaded_v13, PRE_DIPLOMACY_SAVE_FORMAT_VERSION),
            v13
        );
    }

    #[test]
    fn malformed_city_build_links_refuse_without_loosening_other_build_boundaries() {
        let (valid, center, _farm) = sim_with_city_build_chain();

        let mut dangling = valid;
        dangling.builds[center].city_down = 2_777;
        assert!(matches!(save_sim(&dangling), Err(SaveError::Builds(_))));

        let (mut cycle, center, farm) = sim_with_city_build_chain();
        cycle.builds[farm].city_down = cycle.builds[center].object_id();
        assert!(matches!(save_sim(&cycle), Err(SaveError::Builds(_))));

        let (mut cross_city, _, farm) = sim_with_city_build_chain();
        cross_city.builds[farm].city = 1;
        assert!(matches!(save_sim(&cross_city), Err(SaveError::Builds(_))));

        let (mut wrong_owner, _, _) = sim_with_city_build_chain();
        wrong_owner.cities.slots[2][0].who = 3;
        assert!(matches!(save_sim(&wrong_owner), Err(SaveError::Builds(_))));

        let (mut duplicate, _, farm) = sim_with_city_build_chain();
        let fork = install_build(&mut duplicate, 2, ordinary_build(false));
        duplicate.builds[fork].city = 0;
        duplicate.builds[fork].city_down = duplicate.builds[farm].object_id();
        assert!(matches!(save_sim(&duplicate), Err(SaveError::Builds(_))));

        let (mut still_unsupported, _, _) = sim_with_city_build_chain();
        still_unsupported.builds[center].wonder = 0;
        assert!(matches!(
            save_sim(&still_unsupported),
            Err(SaveError::Builds(_))
        ));
    }

    #[test]
    fn v14_diplomacy_leaf_resumes_and_malformed_root_variants_fail_closed() {
        let mut original = supported_sim();
        let retained = &mut original.diplomacy.leaders[3];
        retained.proposals[5].agreement_pending = 1;
        retained.proposals[5].proposal_open = -7;
        retained.proposals[5].treaty = 2;
        retained.proposals[5].offers = [11, -12, 13, -14, 15, -16];
        retained.proposals[5].declaration_costs = [21, 22, 23, 24, 25, 26];
        retained.proposals[5].attacks[1] = 91;
        retained.reserved_resources = [31, 32, 33, 34, 35, 36];
        retained.repeated_targets = 41;
        retained.sent_raw = 42;
        retained.received_scaled = 43;

        let bytes = save_sim(&original).unwrap();
        let loaded = load_sim(&bytes).unwrap();
        assert_eq!(loaded.diplomacy, original.diplomacy);
        assert_eq!(save_sim(&loaded).unwrap(), bytes);

        let parsed = parse_chunk(&bytes[MAGIC.len()..]).unwrap();
        let without = parsed
            .children
            .iter()
            .filter(|child| child.header.id != DIPLOMACY)
            .map(|child| Chunk::leaf(child.header.id, child.data.to_vec()))
            .collect();
        assert_eq!(
            load_error(&encoded_root(without)),
            SaveError::InvalidChunk("root child count does not match format version")
        );

        let trailing = parsed
            .children
            .iter()
            .map(|child| {
                let mut data = child.data.to_vec();
                if child.header.id == DIPLOMACY {
                    data.push(0);
                }
                Chunk::leaf(child.header.id, data)
            })
            .collect();
        assert_eq!(
            load_error(&encoded_root(trailing)),
            SaveError::Invalid("diplomacy payload")
        );

        let mut duplicate: Vec<Chunk> = parsed
            .children
            .iter()
            .map(|child| Chunk::leaf(child.header.id, child.data.to_vec()))
            .collect();
        duplicate.push(Chunk::leaf(
            DIPLOMACY,
            encode_diplomacy_payload(&original.diplomacy),
        ));
        assert_eq!(
            load_error(&encoded_root(duplicate)),
            SaveError::InvalidChunk("root id/child count")
        );
    }

    #[test]
    fn v15_scenario_ignore_lists_roundtrip_and_v14_defaults_byte_exactly() {
        let mut original = supported_sim();
        original.scenario_ignore_orders.ignore_orders = true;
        original.scenario_ignore_orders.ignored_by_owner[0] = vec![17, -1, 17];
        original.scenario_ignore_orders.ignored_by_owner[7] = vec![i16::MAX as i32];

        let bytes = save_sim(&original).unwrap();
        let loaded = load_sim(&bytes).unwrap();
        assert_eq!(loaded.scenario_ignore_orders.ignore_orders, true);
        assert_eq!(
            loaded.scenario_ignore_orders.ignored_by_owner,
            original.scenario_ignore_orders.ignored_by_owner
        );
        assert_eq!(loaded.scenario_ignore_orders.revision, 0);
        assert_eq!(save_sim(&loaded).unwrap(), bytes);

        let mut wrong_payload_version = bytes.clone();
        let scenario = section_offset(&wrong_payload_version, SCENARIO_IGNORES);
        wrong_payload_version[scenario + 8] = 2;
        assert_eq!(
            load_error(&wrong_payload_version),
            SaveError::Invalid("scenario ignore-orders payload")
        );

        let pristine = supported_sim();
        let v14 = prior_format_stream(&pristine, DIPLOMACY_SAVE_FORMAT_VERSION);
        let upgraded = load_sim(&v14).unwrap();
        assert_eq!(
            upgraded.scenario_ignore_orders,
            air_runtime_authority::ScenarioIgnoreOrdersAuthority::default()
        );
        assert_eq!(
            prior_format_stream(&upgraded, DIPLOMACY_SAVE_FORMAT_VERSION),
            v14
        );
    }

    #[test]
    fn v17_armies_roundtrip_legacy_defaults_and_malformed_links_fail_closed() {
        let mut original = supported_sim();
        let group_id = groups_guys::Groups::index(2, 3);
        let mut group = groups_guys::GroupData {
            id: group_id as i32,
            army: 3,
            who: 2,
            form: 4,
            ..groups_guys::GroupData::default()
        };
        assert!(group.add(0, 2, false, 0x44, 7));
        original.groups.list[group_id] = group;
        let army = &mut original.armies.lists[2][3];
        army.valid = 1;
        army.army = 3;
        army.who = 2;
        army.status = 0x12;
        army.reg = 9;
        army.role = 0x44;
        army.num_units = 1;
        army.num_captains = 2;
        army.num_standard = 3;
        army.num_decoys = 4;
        army.city = 5;
        army.navy = 1;
        army.human_frame = 6;
        army.hurry = 7;
        army.target_o = 8;
        army.target_who = 3;
        army.x = 900;
        army.y = 901;
        army.angle = 48;
        army.rally_dist = 77;
        army.muster_x = 12;
        army.muster_y = 13;
        army.muster_angle = 14;
        army.num_groups = 1;
        army.list[0] = group_id as i32;
        let expected_image = army.image();

        let bytes = save_sim(&original).unwrap();
        assert_eq!(
            u32::from_le_bytes(bytes[24..28].try_into().unwrap()),
            ARMIES_FORMAT_VERSION
        );
        let loaded = load_sim(&bytes).unwrap();
        assert_eq!(loaded.armies.lists[2][3].image(), expected_image);
        assert_eq!(
            loaded.armies.find_dist,
            crate::systems::armies::FIND_DIST_SEED
        );
        assert_eq!(save_sim(&loaded).unwrap(), bytes);

        let pristine = supported_sim();
        let v16 = prior_format_stream(&pristine, FARMS_FORMAT_VERSION);
        let upgraded = load_sim(&v16).unwrap();
        assert!(upgraded
            .armies
            .lists
            .iter()
            .flatten()
            .all(|army| army.valid == 0));
        assert_eq!(prior_format_stream(&upgraded, FARMS_FORMAT_VERSION), v16);

        let mut bad_presence = bytes.clone();
        let section = section_offset(&bad_presence, ARMIES);
        bad_presence[section + 8 + 11] = 0;
        assert_eq!(
            load_error(&bad_presence),
            SaveError::Invalid("Army pointer presence")
        );

        let mut bad_identity = bytes.clone();
        let section = section_offset(&bad_identity, ARMIES);
        let valid = section + 8 + 2 * 65 + 33 + 3 * 2;
        bad_identity[valid + 2..valid + 4].copy_from_slice(&4i16.to_le_bytes());
        assert_eq!(
            load_error(&bad_identity),
            SaveError::Invalid("live Army identity/shape")
        );

        let mut duplicate = original;
        let second = &mut duplicate.armies.lists[2][4];
        second.valid = 1;
        second.army = 4;
        second.who = 2;
        second.num_groups = 1;
        second.list[0] = group_id as i32;
        assert_eq!(
            save_sim(&duplicate),
            Err(SaveError::Invalid("Army group duplicate/cycle"))
        );

        let mut bad_backlink = loaded;
        bad_backlink.groups.list[group_id].army = 4;
        assert_eq!(
            save_sim(&bad_backlink),
            Err(SaveError::Invalid("Army/Group backlink"))
        );
    }

    #[test]
    fn format_twelve_typed_orders_load_with_empty_cache_and_upgrade_to_v14() {
        let mut original = supported_sim();
        let row = 0;
        original
            .world
            .orders_mut(row)
            .replace(Order::move_to(4_321, 7_654, 0));
        let legacy = prior_format_stream(&original, TYPED_ORDER_FORMAT_VERSION);
        let loaded = load_sim(&legacy).unwrap();
        assert!(loaded.command_package_state.is_empty());
        assert_eq!(loaded.world.orders(row), original.world.orders(row));
        let upgraded = save_sim(&loaded).unwrap();
        let parsed = parse_chunk(&upgraded[MAGIC.len()..]).unwrap();
        assert_eq!(
            read_core(parsed.children[0].data).unwrap().format_version,
            FORMAT_VERSION
        );
        assert!(parsed
            .children
            .iter()
            .any(|child| child.header.id == COMMAND_PACKAGE_STATE));
    }

    #[test]
    fn v13_order_node_metric_is_reused_exactly_by_v14_and_round_trips() {
        let order = Order {
            kind: OrderIndex::Think,
            flags: 0x5a,
            x: 0x1122_3344,
            y: -77,
            ..Order::default()
        };
        let mut v12 = Writer::default();
        write_order_node(&mut v12, &order, TYPED_ORDER_FORMAT_VERSION).unwrap();
        let mut v13 = Writer::default();
        write_order_node(&mut v13, &order, PRE_DIPLOMACY_SAVE_FORMAT_VERSION).unwrap();
        let mut v14 = Writer::default();
        write_order_node(&mut v14, &order, FORMAT_VERSION).unwrap();

        assert_eq!(v13.0.first(), Some(&0));
        assert_eq!(&v13.0[1..], v12.0.as_slice());
        assert_eq!(v14.0, v13.0);

        let mut v12_reader = Reader::new(&v12.0);
        assert_eq!(
            read_order_node(&mut v12_reader, TYPED_ORDER_FORMAT_VERSION).unwrap(),
            order
        );
        v12_reader.finish().unwrap();
        let mut v13_reader = Reader::new(&v13.0);
        assert_eq!(
            read_order_node(&mut v13_reader, FORMAT_VERSION).unwrap(),
            order
        );
        v13_reader.finish().unwrap();

        let mut nonzero = v13.0.clone();
        nonzero[0] = 0xa7;
        let mut expected_nonzero = order.clone();
        expected_nonzero.node_metric = 0xa7;
        let mut nonzero_reader = Reader::new(&nonzero);
        assert_eq!(
            read_order_node(&mut nonzero_reader, FORMAT_VERSION).unwrap(),
            expected_nonzero
        );
        nonzero_reader.finish().unwrap();
        assert_eq!(
            read_order_node(&mut Reader::new(&[]), FORMAT_VERSION),
            Err(SaveError::Invalid("truncated payload"))
        );
    }

    #[test]
    fn v13_economy_order_tags_and_metrics_are_byte_identical_in_v14() {
        use crate::systems::economy_order_payload_authority::{
            CastOrderPayload, EconomyOrderHeader, EconomyOrderNode, EconomyOrderPayload,
            GatherOrderPayload, StableTargetIdentity, TradeOrderPayload,
        };

        let primary = StableTargetIdentity::live(
            17,
            2,
            0x1234,
            crate::Handle {
                id: 91,
                generation: 7,
            },
        );
        let header = |kind| EconomyOrderHeader {
            kind,
            flags: 0x53,
            x: -12_345,
            y: 98_765,
            primary,
        };
        let cases = [
            EconomyOrderNode {
                metric: 0x11,
                header: header(OrderIndex::BoardShip),
                payload: EconomyOrderPayload::TargetOnly,
            },
            EconomyOrderNode {
                metric: 0x22,
                header: header(OrderIndex::Gather),
                payload: EconomyOrderPayload::Gather(GatherOrderPayload {
                    tx: -1,
                    ty: 2,
                    build_type: 3,
                    wait: -4,
                    goto_build: 5,
                    non_flat_gather: 6,
                    dist_mod: 7,
                    been_there: 8,
                }),
            },
            EconomyOrderNode {
                metric: 0x33,
                header: EconomyOrderHeader {
                    primary: StableTargetIdentity::NONE,
                    ..header(OrderIndex::CastSpell)
                },
                payload: EconomyOrderPayload::CastSpell(CastOrderPayload {
                    paid: -99,
                    spell: 101,
                }),
            },
            EconomyOrderNode {
                metric: 0x44,
                header: header(OrderIndex::TradeRoute),
                payload: EconomyOrderPayload::TradeRoute(TradeOrderPayload {
                    second: StableTargetIdentity::banded(2_007, 3, 0xabcd),
                    started: -77,
                    loaded: 88,
                }),
            },
        ];

        for node in cases {
            let order = Order::economy(node).unwrap();
            let mut v13 = Writer::default();
            write_order_node(&mut v13, &order, PRE_DIPLOMACY_SAVE_FORMAT_VERSION).unwrap();
            let mut writer = Writer::default();
            write_order_node(&mut writer, &order, FORMAT_VERSION).unwrap();
            assert_eq!(writer.0, v13.0);
            assert_eq!(writer.0[0], node.metric);

            let mut reader = Reader::new(&writer.0);
            let decoded = read_order_node(&mut reader, FORMAT_VERSION).unwrap();
            reader.finish().unwrap();
            assert_eq!(decoded, order);

            let mut resaved = Writer::default();
            write_order_node(&mut resaved, &decoded, FORMAT_VERSION).unwrap();
            assert_eq!(resaved.0, writer.0);
        }
    }

    #[test]
    fn legacy_economy_kinds_keep_their_payload_free_v7_through_v12_images() {
        let legacy = Order {
            kind: OrderIndex::BoardShip,
            flags: 0x41,
            target_who: 2,
            target_o: 17,
            target_uid: 0x1234,
            ..Order::default()
        };

        for version in LEGACY_DENSE_OBJECTS_FORMAT_VERSION..=TYPED_ORDER_FORMAT_VERSION {
            let mut writer = Writer::default();
            write_order_node(&mut writer, &legacy, version).unwrap();
            if version == TYPED_ORDER_FORMAT_VERSION {
                assert_eq!(
                    &writer.0[writer.0.len() - 2..],
                    &[DoNSaveOrderPayloadTag::None as u8, 0]
                );
            }
            let mut reader = Reader::new(&writer.0);
            assert_eq!(read_order_node(&mut reader, version).unwrap(), legacy);
            reader.finish().unwrap();
        }

        assert_eq!(
            write_order_node(&mut Writer::default(), &legacy, FORMAT_VERSION),
            Err(SaveError::Invalid("missing economy order payload"))
        );
    }

    #[test]
    fn format_eight_sparse_stream_remains_loadable_without_player_setup_chunk() {
        let original = supported_sim();
        let state = original.world.export_save_state().unwrap();
        let bytes = save_sim(&original).unwrap();
        let parsed = parse_chunk(&bytes[MAGIC.len()..]).unwrap();
        let children = parsed
            .children
            .iter()
            .filter(|child| {
                !matches!(
                    child.header.id,
                    PLAYER_SETUP
                        | GROUPS
                        | LEADER_MATCH
                        | COMMAND_PACKAGE_STATE
                        | DIPLOMACY
                        | SCENARIO_IGNORES
                        | FARMS
                        | ARMIES
                )
            })
            .map(|child| {
                let mut data = child.data.to_vec();
                if child.header.id == CORE {
                    data[..4].copy_from_slice(&SPARSE_OBJECTS_FORMAT_VERSION.to_le_bytes());
                } else if child.header.id == OBJECTS {
                    data = write_world_state_for_version(&state, SPARSE_OBJECTS_FORMAT_VERSION)
                        .unwrap();
                }
                Chunk::leaf(child.header.id, data)
            })
            .collect();
        let root = Chunk::branch(ROOT, children).encode().unwrap();
        let mut legacy = MAGIC.to_vec();
        legacy.extend_from_slice(&root);

        let loaded = load_sim(&legacy).unwrap();
        assert!(loaded.vic_leaders.setup_owner.applied().is_none());
        // Resave intentionally upgrades the stream to the current format.
        let upgraded = save_sim(&loaded).unwrap();
        assert_eq!(
            read_core(&parse_chunk(&upgraded[MAGIC.len()..]).unwrap().children[0].data)
                .unwrap()
                .format_version,
            FORMAT_VERSION
        );
    }

    #[test]
    fn format_ten_groups_stream_remains_loadable_without_leader_match_chunk() {
        let mut original = supported_sim();
        // v10 did not own Match clocks; its representable shape reconstructs defaults.
        original.world.frame = 0;
        original.world.seconds = 0;
        original.vic_match.frame = 0;
        original.vic_match.tick = 0;
        let state = original.world.export_save_state().unwrap();
        let bytes = save_sim(&original).unwrap();
        let parsed = parse_chunk(&bytes[MAGIC.len()..]).unwrap();
        let children = parsed
            .children
            .iter()
            .filter(|child| {
                !matches!(
                    child.header.id,
                    LEADER_MATCH
                        | COMMAND_PACKAGE_STATE
                        | DIPLOMACY
                        | SCENARIO_IGNORES
                        | FARMS
                        | ARMIES
                )
            })
            .map(|child| {
                let mut data = child.data.to_vec();
                if child.header.id == CORE {
                    data[..4].copy_from_slice(&GROUPS_FORMAT_VERSION.to_le_bytes());
                } else if child.header.id == OBJECTS {
                    data = write_world_state_for_version(&state, GROUPS_FORMAT_VERSION).unwrap();
                }
                Chunk::leaf(child.header.id, data)
            })
            .collect();
        let root = Chunk::branch(ROOT, children).encode().unwrap();
        let mut legacy = MAGIC.to_vec();
        legacy.extend_from_slice(&root);

        let loaded = load_sim(&legacy).unwrap();
        assert!(loaded.vic_leaders.setup_owner.applied().is_none());
        assert_eq!(loaded.world.frame, 0);
        assert_eq!(
            read_core(
                &parse_chunk(&save_sim(&loaded).unwrap()[MAGIC.len()..])
                    .unwrap()
                    .children[0]
                    .data
            )
            .unwrap()
            .format_version,
            FORMAT_VERSION
        );
    }

    #[test]
    fn format_eight_sparse_gap_is_preserved_by_codec_and_rejected_by_dense_phase() {
        let original = supported_sim();
        let mut state = original.world.export_save_state().unwrap();
        let snapshot = state.object_bands.as_mut().unwrap();
        let unit_band = &mut snapshot.owners[2].bands[0];
        unit_band.slots[0] = SnapshotLifecycle::Tombstone(TombstoneFacts {
            flags: 0,
            hold_frames: 3,
            is_unit: true,
            o_up: -1,
        });

        let payload = write_world_state(&state).unwrap();
        let decoded = read_world_state(
            &payload,
            FORMAT_VERSION,
            original.world.frame,
            original.world.seconds,
            original.world.random.state(),
        )
        .unwrap();
        assert_eq!(decoded.object_bands, state.object_bands);

        let mut target = crate::World::with_capacity(1, 0);
        assert_eq!(
            target.import_save_state(decoded),
            Err(WorldSaveError::RegistryMismatch)
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
    fn divergent_step12_cursor_views_and_impossible_group_slots_still_fail_closed() {
        let mut divergent = supported_sim();
        divergent.game_daemon.empty_colls = 7;
        divergent.collision_blocks =
            crate::systems::collision_blocks_live::CollisionBlockRuntime::from_cursor(9);
        assert_eq!(
            save_sim(&divergent),
            Err(SaveError::Invalid("GameDaemon collision cursor mirror"))
        );

        // A live group slot is no longer a refusal — the `GROUPS` section owns it. What
        // still fails closed is a slot no retail transition can produce: `Group::add`
        // `0x00714350` returns early on `num >= 0x80`, so `num` above `GROUP_MAX_MEMBERS`
        // is not a state, it is corruption, and it would index past the walked arrays.
        let mut live_group = supported_sim();
        live_group.groups.list[0].form = 4;
        let live_bytes = save_sim(&live_group).unwrap();
        assert_eq!(load_sim(&live_bytes).unwrap().groups.list[0].form, 4);

        let mut impossible = supported_sim();
        impossible.groups.list[0].num = groups_guys::GROUP_MAX_MEMBERS as i32 + 1;
        assert_eq!(
            save_sim(&impossible),
            Err(SaveError::Invalid("group slot shape"))
        );

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
        assert!(original.world.set_object_owner_active(2, false));
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
        sim.replace_step12_visibility_type_source(
            1,
            0x12_51_51_51,
            crate::systems::step12_visibility_runtime::VisibilityConstants {
                ptolemy_los_bonus: 2,
                the_ceo_unit_los: 2,
            },
            Vec::new(),
        )
        .unwrap();
        assert_eq!(
            save_sim(&sim),
            Err(SaveError::Unsupported("step-12 visibility authority"))
        );

        let mut sim = supported_sim();
        sim.leaders[0].active = true;
        assert!(matches!(
            save_sim(&sim),
            Err(SaveError::Unsupported(
                "non-canonical leader activation or non-default leader input hosts"
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
    fn player_setup_chunk_cannot_disagree_with_world_activation() {
        let mut sim = Sim::new(0x91, 4);
        let mut request = ManualPlayerSetup {
            active_mask: 0x03,
            team_style: 1,
            local_player_setup_slot: 0,
            ..ManualPlayerSetup::default()
        };
        request.teams[0] = 0;
        request.teams[1] = 1;
        sim.start_manual_player_setup(request).unwrap();
        let original = save_sim(&sim).unwrap();
        let mut bytes = original.clone();
        let setup = section_offset(&bytes, PLAYER_SETUP);
        // leaf header, present bool, then active mask
        bytes[setup + 9] = 0x01;
        assert_eq!(
            load_error(&bytes),
            SaveError::Invalid("player setup activation projection mismatch")
        );

        let mut bytes = original;
        // The retained MatchOptions::team_style must equal the request-derived output;
        // load may not silently normalize a changed byte.
        bytes[setup + 22] = 2;
        assert_eq!(
            load_error(&bytes),
            SaveError::Invalid("player setup derived options/semaphore mismatch")
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
            read_order(&mut reader, FORMAT_VERSION),
            Err(SaveError::Invalid("unknown unit order index"))
        );
    }

    #[test]
    fn unknown_special_animation_discriminators_are_rejected() {
        let mut writer = Writer::default();
        write_order(
            &mut writer,
            &Order::special_anim(SpecialAnimType::Unit, 0, 0),
            FORMAT_VERSION,
        )
        .unwrap();
        let mut bytes = writer.0;
        // Fixed v7 prefix: kind/flags, x/y, who/o/uid, absent Handle, tolerance, then the
        // present SPECIAL_ANIM byte. Its discriminator immediately follows at byte 21.
        bytes[21..25].copy_from_slice(&3i32.to_le_bytes());
        let mut reader = Reader::new(&bytes);
        assert_eq!(
            read_order(&mut reader, FORMAT_VERSION),
            Err(SaveError::Invalid("unknown special animation type"))
        );
    }

    #[test]
    fn change_form_payload_round_trips_without_losing_the_walked_delay() {
        let order = Order::change_form(0x1020_3040, -7, 1234);
        let mut writer = Writer::default();
        write_order(&mut writer, &order, FORMAT_VERSION).unwrap();
        let mut reader = Reader::new(&writer.0);
        assert_eq!(read_order(&mut reader, FORMAT_VERSION).unwrap(), order);
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
        write_order(&mut writer, &order, FORMAT_VERSION).unwrap();
        let mut reader = Reader::new(&writer.0);
        assert_eq!(read_order(&mut reader, FORMAT_VERSION).unwrap(), order);
        reader.finish().unwrap();
    }

    fn exact_air_patrol_order() -> Order {
        Order::air_patrol(
            AirPatrolOrderPayload {
                x: WalkedCoordArray {
                    increment: -7,
                    flags: 0x08,
                    values: vec![1, 2, 3],
                },
                y: WalkedCoordArray {
                    increment: 9,
                    flags: 0x10,
                    values: vec![4, 5, 6],
                },
                waypoint: 2,
                air: AirOrderPayload {
                    home_o: 2_015,
                    home_who: 0,
                    cruising_alt: 0x640,
                    sharp_turn: 1,
                    old: 2,
                    returning: 0,
                },
            },
            true,
        )
        .unwrap()
    }

    #[test]
    fn air_patrol_tag_six_round_trips_every_dynamic_field() {
        let order = exact_air_patrol_order();
        let mut writer = Writer::default();
        write_order(&mut writer, &order, FORMAT_VERSION).unwrap();
        assert_eq!(writer.0[23], DoNSaveOrderPayloadTag::AirPatrol as u8);
        assert_eq!(writer.0[24], 1);
        let mut reader = Reader::new(&writer.0);
        assert_eq!(read_order(&mut reader, FORMAT_VERSION).unwrap(), order);
        reader.finish().unwrap();
    }

    #[test]
    fn air_patrol_tag_six_is_v13_only_and_v7_through_v12_stay_exact() {
        let legacy = Order {
            kind: OrderIndex::AirPatrol,
            flags: 0x5a,
            x: 0x1122_3344,
            y: -77,
            ..Order::default()
        };
        let mut writer = Writer::default();
        write_order(&mut writer, &legacy, TYPED_ORDER_FORMAT_VERSION).unwrap();
        assert_eq!(writer.0[23], DoNSaveOrderPayloadTag::None as u8);
        assert_eq!(writer.0[24], 0);
        let mut reader = Reader::new(&writer.0);
        assert_eq!(
            read_order(&mut reader, TYPED_ORDER_FORMAT_VERSION).unwrap(),
            legacy
        );
        reader.finish().unwrap();

        let exact = exact_air_patrol_order();
        for version in LEGACY_DENSE_OBJECTS_FORMAT_VERSION..=TYPED_ORDER_FORMAT_VERSION {
            assert_eq!(
                write_order(&mut Writer::default(), &exact, version),
                Err(SaveError::Unsupported(
                    "AIR_PATROL payload before DoNSave v13"
                )),
                "v{version} silently discarded the typed AIR_PATROL payload"
            );
        }
        assert_eq!(
            write_order(&mut Writer::default(), &legacy, FORMAT_VERSION),
            Err(SaveError::Invalid("missing AIR_PATROL payload"))
        );

        let mut reserved = writer.0;
        reserved[23] = DoNSaveOrderPayloadTag::AirPatrol as u8;
        reserved[24] = 1;
        assert_eq!(
            read_order(&mut Reader::new(&reserved), TYPED_ORDER_FORMAT_VERSION),
            Err(SaveError::Unsupported("reserved typed order payload"))
        );
    }

    #[test]
    fn air_patrol_tag_version_count_flags_and_truncation_mutations_refuse() {
        let order = exact_air_patrol_order();
        let mut writer = Writer::default();
        write_order(&mut writer, &order, FORMAT_VERSION).unwrap();
        let valid = writer.0;

        let mut wrong_version = valid.clone();
        wrong_version[24] = 2;
        assert_eq!(
            read_order(&mut Reader::new(&wrong_version), FORMAT_VERSION),
            Err(SaveError::Invalid(
                "unknown order payload discriminator/version"
            ))
        );

        let mut excessive = valid.clone();
        excessive[25..29].copy_from_slice(&((MAX_DECODED_PATROL_POINTS as u32) + 1).to_le_bytes());
        assert_eq!(
            read_order(&mut Reader::new(&excessive), FORMAT_VERSION),
            Err(SaveError::Limit("AIR_PATROL waypoint count"))
        );

        let mut allocator_flag = valid.clone();
        // prefix 23, tag/version 2, x count 4, x increment 2, then x flags.
        allocator_flag[31] |= 0x40;
        assert_eq!(
            read_order(&mut Reader::new(&allocator_flag), FORMAT_VERSION),
            Err(SaveError::Invalid("invalid AIR_PATROL payload"))
        );

        let mut truncated = valid;
        truncated.pop();
        assert_eq!(
            read_order(&mut Reader::new(&truncated), FORMAT_VERSION),
            Err(SaveError::Invalid("truncated payload"))
        );
    }

    fn exact_strafe_order() -> Order {
        Order::strafe(
            crate::systems::patrol::StrafeOrder {
                target_o: 12,
                target_who: 3,
                target_uid: 0x4567,
                def_x: -101,
                def_y: 202,
                mandatory: 1,
                defensive: 0,
                in_range: 1,
                ever_in_range: 1,
                new_ord: 0,
                air: crate::systems::air::AirOrderWalk {
                    oxx: 7,
                    whose: 2,
                    cruising_alt: 0x640,
                    sharp_turn: -1,
                    old: 4,
                    returning: 0,
                },
                xx: 900,
                yy: 901,
            },
            false,
        )
        .unwrap()
    }

    #[test]
    fn strafe_tag_eight_round_trips_and_v7_through_v12_remain_payload_free() {
        let order = exact_strafe_order();
        let mut writer = Writer::default();
        write_order(&mut writer, &order, FORMAT_VERSION).unwrap();
        assert_eq!(writer.0[23], DoNSaveOrderPayloadTag::Strafe as u8);
        assert_eq!(writer.0[24], 1);
        assert_eq!(writer.0.len(), 23 + STRAFE_LEAF_BYTES);
        let mut reader = Reader::new(&writer.0);
        assert_eq!(read_order(&mut reader, FORMAT_VERSION).unwrap(), order);
        reader.finish().unwrap();

        for version in LEGACY_DENSE_OBJECTS_FORMAT_VERSION..=TYPED_ORDER_FORMAT_VERSION {
            assert_eq!(
                write_order(&mut Writer::default(), &order, version),
                Err(SaveError::Unsupported("STRAFE payload before DoNSave v13")),
                "v{version} silently discarded the typed STRAFE payload"
            );
        }

        let legacy = Order {
            kind: OrderIndex::Strafe,
            target_who: 3,
            target_o: 12,
            target_uid: 0x4567,
            ..Order::default()
        };
        let mut legacy_writer = Writer::default();
        write_order(&mut legacy_writer, &legacy, TYPED_ORDER_FORMAT_VERSION).unwrap();
        assert_eq!(legacy_writer.0[23], DoNSaveOrderPayloadTag::None as u8);
        assert_eq!(legacy_writer.0[24], 0);
        let mut legacy_reader = Reader::new(&legacy_writer.0);
        assert_eq!(
            read_order(&mut legacy_reader, TYPED_ORDER_FORMAT_VERSION).unwrap(),
            legacy
        );
        legacy_reader.finish().unwrap();
    }

    #[test]
    fn versions_seven_through_eleven_keep_the_exact_legacy_order_image() {
        let order = Order::move_to(0x1020_3040, -0x1020_304, 77);
        let mut expected = order.clone();
        expected.move_state = None;
        let mut reference = None;
        for version in LEGACY_DENSE_OBJECTS_FORMAT_VERSION..=LEGACY_ORDER_FORMAT_VERSION {
            let mut writer = Writer::default();
            write_order(&mut writer, &order, version).unwrap();
            if let Some(reference) = &reference {
                assert_eq!(
                    &writer.0, reference,
                    "legacy order image changed in v{version}"
                );
            } else {
                reference = Some(writer.0.clone());
            }
            let mut reader = Reader::new(&writer.0);
            assert_eq!(read_order(&mut reader, version).unwrap(), expected);
            reader.finish().unwrap();
        }
    }

    #[test]
    fn every_move_payload_field_is_independently_serialized_and_restored() {
        macro_rules! movement_field_mutations {
            ($($field:ident => $value:expr),+ $(,)?) => {{
                vec![$({
                    let mut state = MoveOrderState::default();
                    state.$field = $value;
                    (stringify!($field), state)
                }),+]
            }};
        }

        let variants = movement_field_mutations![
            angle => 11,
            dest => 12,
            pause => 13,
            retry => 14,
            attempts => 15,
            timer => 16,
            facing => 17,
            dest_x => 18,
            dest_y => 19,
            last_x => 20,
            last_y => 21,
            coll_x => 22,
            coll_y => 23,
            orig_x => 24,
            orig_y => 25,
            off_x => 26,
            off_y => 27,
            group_oxx => 28,
            group_whose => 29,
            group_id => 30,
            group_form_id => 31,
            group_angle => 32,
            in_group => 33,
        ];
        let baseline_order = Order {
            kind: OrderIndex::MoveTo,
            move_state: Some(MoveOrderState::default()),
            ..Order::default()
        };
        let mut baseline_writer = Writer::default();
        write_order(&mut baseline_writer, &baseline_order, FORMAT_VERSION).unwrap();
        let mut images = std::collections::BTreeSet::new();

        for (field, state) in variants {
            let order = Order {
                kind: OrderIndex::MoveTo,
                move_state: Some(state),
                ..Order::default()
            };
            let mut writer = Writer::default();
            write_order(&mut writer, &order, FORMAT_VERSION).unwrap();
            assert_ne!(writer.0, baseline_writer.0, "{field} was not serialized");
            assert!(
                images.insert(writer.0.clone()),
                "{field} aliased another field"
            );
            let mut reader = Reader::new(&writer.0);
            assert_eq!(read_order(&mut reader, FORMAT_VERSION).unwrap(), order);
            reader.finish().unwrap();
        }
        assert_eq!(images.len(), 23);
    }

    #[test]
    fn v12_typed_order_envelope_rejects_foreign_unknown_and_truncated_payloads() {
        let missing = Order {
            kind: OrderIndex::MoveTo,
            ..Order::default()
        };
        let mut writer = Writer::default();
        assert_eq!(
            write_order(&mut writer, &missing, FORMAT_VERSION),
            Err(SaveError::Invalid("missing movement payload"))
        );
        assert!(writer.0.is_empty(), "validation must precede every write");

        let foreign = Order {
            kind: OrderIndex::Attack,
            move_state: Some(MoveOrderState::default()),
            ..Order::default()
        };
        let mut writer = Writer::default();
        assert_eq!(
            write_order(&mut writer, &foreign, FORMAT_VERSION),
            Err(SaveError::Invalid("movement payload on foreign order kind"))
        );
        assert!(writer.0.is_empty(), "validation must precede every write");

        let move_order = Order::move_to(100, 200, 3);
        let mut writer = Writer::default();
        write_order(&mut writer, &move_order, FORMAT_VERSION).unwrap();
        let valid = writer.0;

        let mut foreign_bytes = valid.clone();
        foreign_bytes[0] = OrderIndex::Attack as u8;
        assert_eq!(
            read_order(&mut Reader::new(&foreign_bytes), FORMAT_VERSION),
            Err(SaveError::Invalid("movement payload on foreign order kind"))
        );

        let mut missing_bytes = valid.clone();
        missing_bytes[23] = DoNSaveOrderPayloadTag::None as u8;
        missing_bytes[24] = DoNSaveOrderPayloadTag::None.wire_version();
        assert_eq!(
            read_order(&mut Reader::new(&missing_bytes), FORMAT_VERSION),
            Err(SaveError::Invalid("missing movement payload"))
        );

        // No optional legacy payloads are present, so the v12 tag/version are bytes 23/24.
        let mut unknown = valid.clone();
        unknown[23] = 0xff;
        assert_eq!(
            read_order(&mut Reader::new(&unknown), FORMAT_VERSION),
            Err(SaveError::Invalid(
                "unknown order payload discriminator/version"
            ))
        );
        let mut wrong_version = valid.clone();
        wrong_version[24] = 7;
        assert_eq!(
            read_order(&mut Reader::new(&wrong_version), FORMAT_VERSION),
            Err(SaveError::Invalid(
                "unknown order payload discriminator/version"
            ))
        );
        let mut truncated = valid;
        truncated.pop();
        assert_eq!(
            read_order(&mut Reader::new(&truncated), FORMAT_VERSION),
            Err(SaveError::Invalid("truncated payload"))
        );
    }

    #[test]
    fn save_refuses_a_foreign_move_payload_without_mutating_the_sim() {
        let mut sim = supported_sim();
        let malformed = Order {
            kind: OrderIndex::Attack,
            move_state: Some(MoveOrderState::fresh(123, 456)),
            ..Order::default()
        };
        sim.world.orders_mut(0).replace(malformed);
        let before = sim.world.orders(0).clone();
        let frame = sim.world.frame;
        let random = sim.world.random.state();
        assert_eq!(
            save_sim(&sim),
            Err(SaveError::Invalid("movement payload on foreign order kind"))
        );
        assert_eq!(sim.world.orders(0), &before);
        assert_eq!(sim.world.frame, frame);
        assert_eq!(sim.world.random.state(), random);
    }
}
