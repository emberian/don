use crate::{PROTOCOL_MAJOR, PROTOCOL_MINOR};

/// A complete transport frame. Sequence numbers are monotonic within `stream_id`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub protocol_major: u16,
    pub protocol_minor: u16,
    pub flags: u32,
    pub stream_id: [u8; 16],
    pub sequence: u64,
    pub sent_unix_ms: i64,
    pub message: Message,
}

impl Frame {
    pub fn new(stream_id: [u8; 16], sequence: u64, sent_unix_ms: i64, message: Message) -> Self {
        Self {
            protocol_major: PROTOCOL_MAJOR,
            protocol_minor: PROTOCOL_MINOR,
            flags: 0,
            stream_id,
            sequence,
            sent_unix_ms,
            message,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(clippy::large_enum_variant)]
pub enum Message {
    Hello(Hello),
    Snapshot(Snapshot),
    Events(EventBatch),
    Heartbeat(Heartbeat),
    /// A future message kind. Its payload is retained exactly for relays.
    Unknown {
        kind: u16,
        payload: Vec<u8>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Hello {
    pub producer: String,
    pub producer_version: String,
    pub capabilities: Vec<String>,
    pub unknown: Vec<UnknownField>,
}

/// Metadata that lets consumers decide whether a snapshot is coherent enough
/// for advice. `game_frame` and `world_revision` are optional because menu and
/// early attach snapshots may not expose them yet.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct SnapshotMeta {
    pub observed_unix_ms: i64,
    pub observed_monotonic_ns: u64,
    pub game_frame: Option<u64>,
    /// Frame/epoch read immediately before and after the snapshot roots. Equal
    /// values are a strong coherence signal without suspending the game.
    pub sampled_frame_start: Option<u64>,
    pub sampled_frame_end: Option<u64>,
    pub sim_time_ms: Option<u64>,
    pub world_revision: Option<u64>,
    pub coherence: Coherence,
    pub partial: bool,
    pub dropped_since_previous: u32,
    pub capture_duration_us: u32,
    pub retry_count: u16,
    pub capability_bits: u64,
    pub component_validity: u64,
    pub read_calls: u32,
    pub bytes_read: u64,
    pub short_reads: u32,
    pub decode_errors: u32,
    pub game_mode: GameMode,
    pub unknown: Vec<UnknownField>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum GameMode {
    SinglePlayer = 1,
    Multiplayer = 2,
    Replay = 3,
    Menu = 4,
    #[default]
    Unknown = 0,
}

impl GameMode {
    pub(crate) fn from_u64(value: u64) -> Option<Self> {
        match value {
            0 => Some(Self::Unknown),
            1 => Some(Self::SinglePlayer),
            2 => Some(Self::Multiplayer),
            3 => Some(Self::Replay),
            4 => Some(Self::Menu),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum Coherence {
    /// Frame/root guards proved that every required read belongs to one epoch.
    Coherent = 1,
    /// The engine was already paused and a coherent read was obtained. The
    /// probe never pauses or resumes the process to manufacture this state.
    Paused = 2,
    /// Guards changed during every bounded retry. This frame is health-only and
    /// must not enter analyzers.
    IncoherentDropped = 3,
    /// Process or required root disappeared during capture.
    SourceLost = 4,
    #[default]
    Unknown = 0,
}

impl Coherence {
    pub(crate) fn from_u64(value: u64) -> Option<Self> {
        match value {
            0 => Some(Self::Unknown),
            1 => Some(Self::Coherent),
            2 => Some(Self::Paused),
            3 => Some(Self::IncoherentDropped),
            4 => Some(Self::SourceLost),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Snapshot {
    pub meta: SnapshotMeta,
    /// Provenance table referenced by `Evidence::source_id` (zero based).
    pub sources: Vec<Source>,
    pub players: Vec<PlayerEconomy>,
    pub entities: Vec<Entity>,
    pub warnings: Vec<Warning>,
    pub advice: Vec<Advice>,
    pub scope: ObservationScope,
    pub local_human: Option<HumanIdentity>,
    pub unknown: Vec<UnknownField>,
}

impl Snapshot {
    /// Whether this snapshot is eligible to enter analyzers. Individual advice
    /// still needs its own evidence and freshness checks.
    pub fn advice_allowed(&self) -> bool {
        let Some(local) = &self.local_human else {
            return false;
        };
        self.meta.coherence == Coherence::Coherent
            && self.meta.game_mode == GameMode::SinglePlayer
            && self.scope == ObservationScope::OwnPlayerOnly
            && local.confirmed_local
            && local.active
            && !self.meta.partial
            && self.meta.game_frame.is_some()
            && self.meta.game_frame == self.meta.sampled_frame_start
            && self.meta.game_frame == self.meta.sampled_frame_end
            && self.meta.component_validity != 0
            && self.meta.read_calls != 0
            && self.meta.bytes_read != 0
            && self.meta.short_reads == 0
            && self.meta.decode_errors == 0
            && self.players.len() == 1
            && self.players[0].player_id == local.player_id
            && self.players[0].gather_stamp.is_some()
            && self.players[0].gather_cache_age_frames.is_some()
            && self.entities.iter().all(|entity| {
                (entity.owner_id.is_none() || entity.owner_id == Some(local.player_id))
                    && matches!(
                        entity.visibility,
                        VisibilityBasis::OwnOrNeutral | VisibilityBasis::CurrentlyVisible
                    )
            })
            && self.sources.iter().any(Source::is_confirmed_process_memory)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum ObservationScope {
    OwnPlayerOnly = 1,
    VisibleWorld = 2,
    FullReplay = 3,
    #[default]
    Unknown = 0,
}

impl ObservationScope {
    pub(crate) fn from_u64(value: u64) -> Option<Self> {
        match value {
            0 => Some(Self::Unknown),
            1 => Some(Self::OwnPlayerOnly),
            2 => Some(Self::VisibleWorld),
            3 => Some(Self::FullReplay),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct HumanIdentity {
    pub player_id: u8,
    pub confirmed_local: bool,
    pub active: bool,
    /// Producer-defined stable reason/key for the identity decision.
    pub identity_key: String,
    pub unknown: Vec<UnknownField>,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Source {
    pub kind: SourceKind,
    /// Stable producer-local identifier, e.g. `econ-block-v3`.
    pub source_key: String,
    /// Retail executable/build fingerprint or replay hash when known.
    pub build_id: String,
    pub process_id: Option<u32>,
    /// OS process creation identity; PID alone may be reused.
    pub process_start_id: Option<u64>,
    pub image_base: Option<u64>,
    pub capture_id: Option<u64>,
    pub detail: String,
    pub unknown: Vec<UnknownField>,
}

impl Source {
    pub fn is_confirmed_process_memory(&self) -> bool {
        self.kind == SourceKind::ProcessMemory
            && !self.source_key.is_empty()
            && !self.build_id.is_empty()
            && self.process_id.is_some_and(|value| value != 0)
            && self.process_start_id.is_some_and(|value| value != 0)
            && self.image_base.is_some_and(|value| value != 0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum SourceKind {
    ProcessMemory = 1,
    Replay = 2,
    Network = 3,
    Derived = 4,
    User = 5,
    #[default]
    Unknown = 0,
}

impl SourceKind {
    pub(crate) fn from_u64(value: u64) -> Option<Self> {
        match value {
            0 => Some(Self::Unknown),
            1 => Some(Self::ProcessMemory),
            2 => Some(Self::Replay),
            3 => Some(Self::Network),
            4 => Some(Self::Derived),
            5 => Some(Self::User),
            _ => None,
        }
    }
}

/// Per-record evidence. Confidence is an integer in 0..=10_000 basis points.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Evidence {
    pub source_id: u16,
    pub confidence_bps: u16,
    /// Versioned calibration or measurement method. Required for non-endpoint
    /// scores so percentages cannot masquerade as measured probability.
    pub calibration_id: String,
    pub freshness_ms: u32,
    /// Producer-defined bitset; unknown bits must be retained.
    pub flags: u32,
    pub unknown: Vec<UnknownField>,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct PlayerEconomy {
    pub player_id: u8,
    pub name: String,
    pub nation_type_id: Option<u32>,
    pub age: Option<u16>,
    pub resources: Vec<ResourceBalance>,
    pub population: Population,
    pub workers: Workforce,
    pub gather_stamp: Option<u64>,
    pub gather_cache_age_frames: Option<u32>,
    pub city_count: Option<u32>,
    pub num_units: Option<u32>,
    pub num_buildings: Option<u32>,
    pub queued_type_counts: Vec<TypeCount>,
    pub evidence: Evidence,
    pub unknown: Vec<UnknownField>,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ResourceBalance {
    pub kind: ResourceKind,
    /// Retail stockpile integer after decrypting the engine field. Display
    /// conversion, if any, is a host concern.
    pub stockpile_raw: Option<i64>,
    /// Retail gather-cache units: sixteenths of a resource per gather period.
    pub income_sixteenths_per_period: Option<i64>,
    /// How the income figure was obtained. Direct engine display rate, observed
    /// stockpile delta, and a model estimate are not interchangeable signals.
    pub income_basis: RateBasis,
    pub gross_sixteenths_per_period: Option<i64>,
    pub support_sixteenths_per_period: Option<i64>,
    pub bonus_sixteenths_per_period: Option<i64>,
    pub leftover_sixteenths_per_period: Option<i64>,
    /// Retail resource-cap entry. `Custom(6)` carries the seventh commerce cap.
    pub resource_cap_raw: Option<i64>,
    /// Raw engine status only. In particular, retail status 2 is a pre-interest
    /// commerce clamp and does not imply displayed income is bounded by cap.
    pub over_cap_status: Option<u8>,
    pub spend_sixteenths_per_period: Option<i64>,
    pub reserved_raw: Option<i64>,
    pub evidence: Evidence,
    pub unknown: Vec<UnknownField>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum RateBasis {
    /// No rate is present or its semantics are unknown.
    #[default]
    Unknown = 0,
    /// Direct engine rate/display block. The source detail must state native
    /// units and whether nation/tech bonuses have already been applied.
    EngineDirect = 1,
    /// Inferred from stockpile deltas and therefore potentially confounded by
    /// construction, purchases, tribute, or refunds.
    ObservedDelta = 2,
    /// Calculated from a known model and observed worker assignments.
    Modeled = 3,
}

impl RateBasis {
    pub(crate) fn from_u64(value: u64) -> Option<Self> {
        match value {
            0 => Some(Self::Unknown),
            1 => Some(Self::EngineDirect),
            2 => Some(Self::ObservedDelta),
            3 => Some(Self::Modeled),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct TypeCount {
    pub type_id: u32,
    pub count: u32,
    pub valid: bool,
    pub unknown: Vec<UnknownField>,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum ResourceKind {
    #[default]
    Food,
    Timber,
    Wealth,
    Knowledge,
    Metal,
    Oil,
    Custom(u16),
}

impl ResourceKind {
    /// Retail array index: food, timber, wealth, knowledge, metal, oil are
    /// exactly 0 through 5.
    pub fn retail_index(&self) -> u16 {
        match self {
            Self::Food => 0,
            Self::Timber => 1,
            Self::Wealth => 2,
            Self::Knowledge => 3,
            Self::Metal => 4,
            Self::Oil => 5,
            Self::Custom(value) => *value,
        }
    }

    pub(crate) fn code(&self) -> u64 {
        u64::from(self.retail_index())
    }

    pub(crate) fn from_code(value: u16) -> Self {
        match value {
            0 => Self::Food,
            1 => Self::Timber,
            2 => Self::Wealth,
            3 => Self::Knowledge,
            4 => Self::Metal,
            5 => Self::Oil,
            n => Self::Custom(n),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Population {
    pub used: u32,
    pub cap: u32,
    pub queued: u32,
    pub citizens: Option<u32>,
    pub military: Option<u32>,
    pub unknown: Vec<UnknownField>,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Workforce {
    pub gathering: u32,
    pub idle: u32,
    pub building: u32,
    pub scouting: u32,
    pub unknown: Vec<UnknownField>,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Entity {
    pub object_id: u64,
    pub type_id: u32,
    pub owner_id: Option<u8>,
    pub kind: EntityKind,
    /// Retail fine-world coordinate units. Display conversion is host-side.
    pub x_fine: i32,
    pub y_fine: i32,
    pub hp_milli: Option<u32>,
    pub hp_max_milli: Option<u32>,
    pub build_progress_ppm: Option<u32>,
    pub state_flags: u64,
    pub queues: Vec<ProductionQueue>,
    pub evidence: Evidence,
    pub visibility: VisibilityBasis,
    pub unknown: Vec<UnknownField>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum VisibilityBasis {
    OwnOrNeutral = 1,
    CurrentlyVisible = 2,
    Remembered = 3,
    Hidden = 4,
    #[default]
    Unknown = 0,
}

impl VisibilityBasis {
    pub(crate) fn from_u64(value: u64) -> Option<Self> {
        match value {
            0 => Some(Self::Unknown),
            1 => Some(Self::OwnOrNeutral),
            2 => Some(Self::CurrentlyVisible),
            3 => Some(Self::Remembered),
            4 => Some(Self::Hidden),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum EntityKind {
    Unit = 1,
    Building = 2,
    Resource = 3,
    Item = 4,
    Projectile = 5,
    #[default]
    Unknown = 0,
}

impl EntityKind {
    pub(crate) fn from_u64(value: u64) -> Option<Self> {
        match value {
            0 => Some(Self::Unknown),
            1 => Some(Self::Unit),
            2 => Some(Self::Building),
            3 => Some(Self::Resource),
            4 => Some(Self::Item),
            5 => Some(Self::Projectile),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ProductionQueue {
    pub queue_id: u16,
    pub capacity: Option<u16>,
    pub items: Vec<QueueItem>,
    pub unknown: Vec<UnknownField>,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct QueueItem {
    pub type_id: u32,
    pub count: u16,
    pub progress_ppm: Option<u32>,
    pub eta_ms: Option<u64>,
    pub paused: bool,
    pub unknown: Vec<UnknownField>,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Warning {
    pub warning_id: u64,
    pub code: String,
    pub severity: Severity,
    pub headline: String,
    pub detail: String,
    pub player_id: Option<u8>,
    pub object_id: Option<u64>,
    pub expires_game_frame: Option<u64>,
    pub evidence: Evidence,
    pub unknown: Vec<UnknownField>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum Severity {
    Info = 1,
    Opportunity = 2,
    Caution = 3,
    Critical = 4,
    #[default]
    Unknown = 0,
}

impl Severity {
    pub(crate) fn from_u64(value: u64) -> Option<Self> {
        match value {
            0 => Some(Self::Unknown),
            1 => Some(Self::Info),
            2 => Some(Self::Opportunity),
            3 => Some(Self::Caution),
            4 => Some(Self::Critical),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Advice {
    pub advice_id: u64,
    pub rule_id: String,
    pub rule_version: String,
    pub lifecycle: AdviceLifecycle,
    /// 0 is least urgent, 10_000 is most urgent.
    pub priority_bps: u16,
    pub headline: String,
    pub rationale: String,
    pub actions: Vec<String>,
    pub related_warning_ids: Vec<u64>,
    pub valid_until_game_frame: Option<u64>,
    pub evidence: Evidence,
    pub unknown: Vec<UnknownField>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum AdviceLifecycle {
    Proposed = 1,
    Active = 2,
    Cleared = 3,
    Superseded = 4,
    #[default]
    Unknown = 0,
}

impl AdviceLifecycle {
    pub(crate) fn from_u64(value: u64) -> Option<Self> {
        match value {
            0 => Some(Self::Unknown),
            1 => Some(Self::Proposed),
            2 => Some(Self::Active),
            3 => Some(Self::Cleared),
            4 => Some(Self::Superseded),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct EventBatch {
    pub base_game_frame: Option<u64>,
    pub events: Vec<Event>,
    pub unknown: Vec<UnknownField>,
}

/// Open event envelope. `kind` is namespaced (e.g. `econ.resource.changed`) and
/// `payload` is producer-defined. Unknown event kinds remain relayable.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Event {
    pub event_id: u64,
    pub game_frame: Option<u64>,
    pub kind: String,
    pub player_id: Option<u8>,
    pub object_id: Option<u64>,
    pub payload: Vec<u8>,
    pub evidence: Evidence,
    pub unknown: Vec<UnknownField>,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Heartbeat {
    pub last_snapshot_sequence: Option<u64>,
    pub producer_state: String,
    pub unknown: Vec<UnknownField>,
}

/// An unrecognized TLV retained exactly, including wire type and flags.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownField {
    pub tag: u16,
    pub wire_type: u8,
    pub flags: u8,
    pub data: Vec<u8>,
}
