use std::fmt;

/// Version of the in-process telemetry contract consumed by this crate.
pub const SNAPSHOT_SCHEMA_VERSION: u16 = 1;

/// Retail resource order.  Do not reorder: it is part of the telemetry ABI.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u8)]
pub enum Resource {
    Food = 0,
    Timber = 1,
    Wealth = 2,
    Knowledge = 3,
    Metal = 4,
    Oil = 5,
}

impl Resource {
    pub const ALL: [Self; 6] = [
        Self::Food,
        Self::Timber,
        Self::Wealth,
        Self::Knowledge,
        Self::Metal,
        Self::Oil,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Food => "food",
            Self::Timber => "timber",
            Self::Wealth => "wealth",
            Self::Knowledge => "knowledge",
            Self::Metal => "metal",
            Self::Oil => "oil",
        }
    }
}

/// Fixed-point resource values in thousandths of a displayed resource unit.
/// This avoids floating-point variation in replayed advice.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ResourceVector(pub [i64; 6]);

impl ResourceVector {
    pub const ZERO: Self = Self([0; 6]);

    pub const fn from_milli(values: [i64; 6]) -> Self {
        Self(values)
    }

    pub fn from_whole(values: [i64; 6]) -> Self {
        Self(values.map(|value| value.saturating_mul(1_000)))
    }

    pub const fn get(self, resource: Resource) -> i64 {
        self.0[resource as usize]
    }

    pub fn set(&mut self, resource: Resource, value: i64) {
        self.0[resource as usize] = value;
    }

    pub fn can_afford(self, cost: Self) -> bool {
        Resource::ALL
            .into_iter()
            .all(|resource| self.get(resource) >= cost.get(resource))
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ResourceWorkers(pub [u16; 6]);

impl ResourceWorkers {
    pub const fn get(self, resource: Resource) -> u16 {
        self.0[resource as usize]
    }
}

/// Conservative ordinal confidence score (0..=1000), not a calibrated
/// probability. Cards also expose the transport/semantic/inference facets
/// from which this minimum is derived.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct Confidence(u16);

impl Confidence {
    pub const NONE: Self = Self(0);
    pub const MAX: Self = Self(1_000);

    pub const fn new(permille: u16) -> Self {
        Self(if permille > 1_000 { 1_000 } else { permille })
    }

    pub const fn permille(self) -> u16 {
        self.0
    }

    pub const fn min(self, other: Self) -> Self {
        if self.0 <= other.0 {
            self
        } else {
            other
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TelemetrySource {
    ProcessMemory,
    Replay,
    Simulation,
    Manual,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FieldProvenance {
    pub source: TelemetrySource,
    /// Confidence that bytes were transported coherently from this source.
    pub transport_confidence: Confidence,
    /// Confidence that the adapter assigned the correct game meaning to them.
    pub semantic_confidence: Confidence,
    /// Age of this particular input when the snapshot was assembled.
    pub age_ms: u64,
}

impl FieldProvenance {
    pub const fn direct(source: TelemetrySource) -> Self {
        Self {
            source,
            transport_confidence: Confidence::MAX,
            // A direct read can still be attached to the wrong object/field.
            semantic_confidence: Confidence::new(900),
            age_ms: 0,
        }
    }

    pub const fn overall(self) -> Confidence {
        self.transport_confidence.min(self.semantic_confidence)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Observed<T> {
    pub value: T,
    pub provenance: FieldProvenance,
}

impl<T> Observed<T> {
    pub const fn new(value: T, provenance: FieldProvenance) -> Self {
        Self { value, provenance }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SnapshotCompleteness {
    /// Satisfies the adapter's declared complete snapshot contract.
    Complete,
    /// A diagnostic/bring-up sample.  Never produces advice.
    Partial,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HumanIdentity {
    Confirmed,
    Ambiguous,
    NonHuman,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GameMode {
    SinglePlayer,
    Multiplayer,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotMeta {
    pub schema_version: u16,
    pub match_id: u64,
    pub sample_sequence: u64,
    pub game_tick: u64,
    /// Monotonic time at which the adapter completed this coherent sample.
    pub captured_at_ms: u64,
    /// Monotonic time at which this analysis is requested.
    pub analyzed_at_ms: u64,
    pub paused: bool,
    pub coherent: bool,
    pub completeness: SnapshotCompleteness,
    pub identity: HumanIdentity,
    pub game_mode: GameMode,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TelemetrySnapshot {
    pub meta: SnapshotMeta,
    /// Required R1 leader-level groups.
    pub economy: Option<Observed<EconomyTelemetry>>,
    pub population: Option<Observed<PopulationTelemetry>>,
    pub labor: Option<Observed<LaborTelemetry>>,
    /// Optional capabilities. Their absence suppresses only dependent rules.
    pub production: Option<Observed<ProductionTelemetry>>,
    pub progression: Option<Observed<ProgressionTelemetry>>,
    pub pressure: Option<Observed<PressureTelemetry>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EconomyTelemetry {
    pub stock: ResourceVector,
    /// Current displayed/derived income rate, in milli-resources per minute.
    pub income_per_minute: ResourceVector,
    /// Explicit, unpaid future spending selected by a player/plan. Paid retail
    /// queues must not be included again here.
    pub planned_unpaid_cost: ResourceVector,
    pub commerce_cap: ResourceVector,
    /// Raw retail per-resource status (currently observed as 0/1/2).  Keeping
    /// the raw value avoids inventing semantics for status 2. This is read
    /// directly, never inferred from noisy stock deltas.
    pub commerce_status: CommerceStatus,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CommerceStatus(pub [u8; 6]);

impl CommerceStatus {
    pub const fn raw(self, resource: Resource) -> u8 {
        self.0[resource as usize]
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PopulationTelemetry {
    pub used: u32,
    pub cap: u32,
    /// Population cost in live, already-paid queues.
    pub paid_queue_population: u32,
    /// Already-paid queued population explicitly observed blocked on cap.
    pub blocked_paid_population: u32,
    pub next_paid_completion_ms: Option<u64>,
    pub incoming_capacity: u32,
    pub incoming_capacity_eta_ms: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LaborTelemetry {
    pub citizens: u32,
    pub idle_citizens: u32,
    pub idle_for_ms: u64,
    pub assigned: ResourceWorkers,
    /// Currently usable gather slots, confirmed by the adapter.
    pub free_gather_slots: ResourceWorkers,
    /// Optional adapter/model result.  Advice is emitted only when every
    /// feasibility guard is explicitly true.
    pub rebalance: Option<RebalanceOpportunity>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RebalanceOpportunity {
    pub from: Resource,
    pub to: Resource,
    pub workers: u16,
    pub destination_free_slots: u16,
    pub path_feasible: bool,
    pub marginal_gain_milli_per_minute: i64,
    pub commerce_headroom_milli: i64,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ProductionClass {
    Citizen,
    LandMilitary,
    Air,
    Naval,
    Research,
    Other,
}

impl ProductionClass {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Citizen => "Citizen production",
            Self::LandMilitary => "land production",
            Self::Air => "air production",
            Self::Naval => "naval production",
            Self::Research => "research",
            Self::Other => "production",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProductionTelemetry {
    pub sites: Vec<ProductionSite>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProductionSite {
    pub stable_id: u32,
    pub class: ProductionClass,
    pub enabled: bool,
    /// Explicit user/plan intent; an empty queue alone is not a problem.
    pub expected_active: bool,
    pub queue_len: u16,
    pub idle_for_ms: u64,
    pub affordable_option_observed: bool,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum OpportunityKind {
    Age,
    EconomyTech,
    MilitaryTech,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgressionTelemetry {
    pub current_age: u8,
    pub opportunities: Vec<ProgressionOpportunity>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgressionOpportunity {
    pub stable_id: u32,
    pub name: String,
    pub kind: OpportunityKind,
    pub cost: ResourceVector,
    /// True only when the opportunity belongs to a player-selected/tracked goal.
    pub tracked_goal: bool,
    pub already_queued: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PressureTelemetry {
    pub under_attack: bool,
    pub own_local_strength: u32,
    pub observed_enemy_local_strength: u32,
    pub enemy_observation_age_ms: u64,
    pub enemy_visibility: EnemyVisibility,
    /// Share of recent spending committed to economy, in permille.
    pub recent_economy_spend_permille: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EnemyVisibility {
    DirectlyVisible,
    Remembered,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum AdviceCategory {
    Economy,
    Population,
    Labor,
    Production,
    Progression,
    Pressure,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Severity {
    Info,
    Notice,
    Warning,
    Critical,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum AdviceKey {
    ResourceBottleneck(Resource),
    CommerceCap(Resource),
    PopulationBlock,
    IdleCitizens,
    LaborRebalance(Resource, Resource),
    IdleProduction(u32),
    ProgressionAffordable(u32),
    LocalMilitaryPressure,
    EconomyUnderPressure,
}

impl fmt::Display for AdviceKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ResourceBottleneck(r) => write!(f, "economy.bottleneck.{}", r.label()),
            Self::CommerceCap(r) => write!(f, "economy.commerce-cap.{}", r.label()),
            Self::PopulationBlock => f.write_str("population.paid-queue-block"),
            Self::IdleCitizens => f.write_str("labor.idle-citizens"),
            Self::LaborRebalance(from, to) => {
                write!(f, "labor.rebalance.{}-to-{}", from.label(), to.label())
            }
            Self::IdleProduction(id) => write!(f, "production.idle.{id}"),
            Self::ProgressionAffordable(id) => write!(f, "progression.affordable.{id}"),
            Self::LocalMilitaryPressure => f.write_str("pressure.local-military"),
            Self::EconomyUnderPressure => f.write_str("pressure.economy-exposure"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Evidence {
    pub input: &'static str,
    pub observed: i64,
    pub threshold: Option<i64>,
    pub unit: &'static str,
    pub provenance: FieldProvenance,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuleRef {
    pub id: &'static str,
    pub version: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConfidenceBreakdown {
    pub transport: Confidence,
    pub semantic: Confidence,
    pub inference: Confidence,
}

impl ConfidenceBreakdown {
    pub const fn overall(self) -> Confidence {
        self.transport.min(self.semantic).min(self.inference)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdviceCard {
    pub rule: RuleRef,
    pub key: AdviceKey,
    pub category: AdviceCategory,
    pub severity: Severity,
    pub confidence: Confidence,
    pub confidence_breakdown: ConfidenceBreakdown,
    pub evidence: Vec<Evidence>,
    pub message: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub expires_at_ms: u64,
    /// Minimum time before this key may be raised again after resolution.
    pub cooldown_ms: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SuppressionReason {
    UnsupportedSchema,
    PartialSnapshot,
    IncoherentSnapshot,
    AmbiguousIdentity,
    NonHuman,
    UnknownGameMode,
    Multiplayer,
    Paused,
    StaleSnapshot,
    InvalidValues,
    TimeWentBackwards,
    SampleWentBackwards,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RetractionReason {
    Resolved,
    Expired,
    DataQuality(SuppressionReason),
    MatchChanged,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AdviceEvent {
    Raised(AdviceCard),
    Updated(AdviceCard),
    Retracted {
        key: AdviceKey,
        reason: RetractionReason,
    },
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AdviceBatch {
    /// Current ranked cards after applying hysteresis and lifecycle rules.
    pub active: Vec<AdviceCard>,
    /// Changes a UI/speech adapter may consume.  `active` remains authoritative.
    pub events: Vec<AdviceEvent>,
    pub suppressed: Option<SuppressionReason>,
}
