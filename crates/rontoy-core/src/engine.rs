use std::collections::{BTreeMap, BTreeSet};

use crate::model::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoachConfig {
    pub max_snapshot_age_ms: u64,
    pub max_field_age_ms: u64,
    pub sustain_ms: u64,
    pub resolve_hysteresis_ms: u64,
    pub advice_ttl_ms: u64,
    pub per_key_cooldown_ms: u64,
    pub update_cooldown_ms: u64,
    pub global_raise_interval_ms: u64,
    pub planning_horizon_ms: u64,
    pub idle_citizen_threshold: u32,
    pub idle_citizen_ms: u64,
    pub idle_production_ms: u64,
    pub pressure_ratio_permille: u32,
    pub economy_exposure_permille: u16,
    pub minimum_confidence: Confidence,
    /// Disabled until a live adapter proves direct visibility semantics.
    pub enable_pressure_advice: bool,
}

impl Default for CoachConfig {
    fn default() -> Self {
        Self {
            max_snapshot_age_ms: 2_000,
            max_field_age_ms: 8_000,
            sustain_ms: 2_000,
            resolve_hysteresis_ms: 3_000,
            advice_ttl_ms: 12_000,
            per_key_cooldown_ms: 30_000,
            update_cooldown_ms: 5_000,
            global_raise_interval_ms: 20_000,
            planning_horizon_ms: 90_000,
            idle_citizen_threshold: 2,
            idle_citizen_ms: 5_000,
            idle_production_ms: 8_000,
            pressure_ratio_permille: 1_250,
            economy_exposure_permille: 650,
            minimum_confidence: Confidence::new(700),
            enable_pressure_advice: false,
        }
    }
}

#[derive(Clone, Debug)]
struct Candidate {
    rule: RuleRef,
    key: AdviceKey,
    category: AdviceCategory,
    severity: Severity,
    confidence: Confidence,
    confidence_breakdown: ConfidenceBreakdown,
    evidence: Vec<Evidence>,
    message: String,
}

#[derive(Clone, Copy, Debug)]
struct Pending {
    first_seen_ms: u64,
}

#[derive(Clone, Copy, Debug)]
struct Missing {
    first_missing_ms: u64,
}

/// Stateful deterministic lifecycle manager.  Its only notion of time is the
/// monotonic millisecond value carried by each input snapshot.
#[derive(Clone, Debug)]
pub struct AdviceEngine {
    config: CoachConfig,
    match_id: Option<u64>,
    last_sequence: Option<u64>,
    last_game_tick: Option<u64>,
    last_analyzed_ms: Option<u64>,
    last_global_raise_ms: Option<u64>,
    pending: BTreeMap<AdviceKey, Pending>,
    missing: BTreeMap<AdviceKey, Missing>,
    active: BTreeMap<AdviceKey, AdviceCard>,
    last_emitted: BTreeMap<AdviceKey, (u64, Severity)>,
}

impl AdviceEngine {
    pub fn new(config: CoachConfig) -> Self {
        Self {
            config,
            match_id: None,
            last_sequence: None,
            last_game_tick: None,
            last_analyzed_ms: None,
            last_global_raise_ms: None,
            pending: BTreeMap::new(),
            missing: BTreeMap::new(),
            active: BTreeMap::new(),
            last_emitted: BTreeMap::new(),
        }
    }

    pub fn config(&self) -> &CoachConfig {
        &self.config
    }

    pub fn reset(&mut self) {
        *self = Self::new(self.config.clone());
    }

    pub fn analyze(&mut self, snapshot: &TelemetrySnapshot) -> AdviceBatch {
        let now = snapshot.meta.analyzed_at_ms;
        let mut events = Vec::new();

        if let Some(old_match) = self.match_id {
            if old_match != snapshot.meta.match_id {
                self.retract_all(RetractionReason::MatchChanged, &mut events);
                self.pending.clear();
                self.missing.clear();
                self.last_emitted.clear();
                self.last_global_raise_ms = None;
                self.last_sequence = None;
                self.last_game_tick = None;
                self.last_analyzed_ms = None;
                self.match_id = Some(snapshot.meta.match_id);
            }
        } else {
            self.match_id = Some(snapshot.meta.match_id);
        }

        if let Some(reason) = self.gate(snapshot) {
            self.retract_all(RetractionReason::DataQuality(reason), &mut events);
            self.pending.clear();
            self.missing.clear();
            self.record_position(snapshot, reason);
            return AdviceBatch {
                active: Vec::new(),
                events,
                suppressed: Some(reason),
            };
        }
        self.record_valid_position(snapshot);

        self.retract_unavailable_optional_sources(snapshot, &mut events);

        let candidates = self.candidates(snapshot);
        let present: BTreeSet<_> = candidates
            .iter()
            .map(|candidate| candidate.key.clone())
            .collect();

        // Resolve absent cards only after hysteresis.  Until then the prior card
        // remains visible but its TTL is not refreshed.
        let active_keys: Vec<_> = self.active.keys().cloned().collect();
        for key in active_keys {
            if present.contains(&key) {
                self.missing.remove(&key);
                continue;
            }
            let missing = self.missing.entry(key.clone()).or_insert(Missing {
                first_missing_ms: now,
            });
            let resolved =
                now.saturating_sub(missing.first_missing_ms) >= self.config.resolve_hysteresis_ms;
            let expired = self
                .active
                .get(&key)
                .is_some_and(|card| now >= card.expires_at_ms);
            if resolved || expired {
                self.active.remove(&key);
                self.missing.remove(&key);
                events.push(AdviceEvent::Retracted {
                    key,
                    reason: if expired {
                        RetractionReason::Expired
                    } else {
                        RetractionReason::Resolved
                    },
                });
            }
        }

        // A condition that vanished before becoming active must sustain again.
        self.pending.retain(|key, _| present.contains(key));

        // Refresh/update active cards first. Escalation is immediate; ordinary
        // updates are rate-limited and never count against the global new-card
        // cadence.
        for candidate in &candidates {
            let Some(old) = self.active.get(&candidate.key).cloned() else {
                continue;
            };
            let changed = old.severity != candidate.severity
                || old.rule != candidate.rule
                || old.confidence != candidate.confidence
                || old.confidence_breakdown != candidate.confidence_breakdown
                || old.message != candidate.message
                || old.evidence != candidate.evidence;
            let escalation = candidate.severity > old.severity;
            let may_update = escalation
                || now.saturating_sub(old.updated_at_ms) >= self.config.update_cooldown_ms;
            let mut card = self.make_card(candidate, old.created_at_ms, now);
            if !changed || !may_update {
                card.rule = old.rule;
                card.message = old.message;
                card.severity = old.severity;
                card.confidence = old.confidence;
                card.confidence_breakdown = old.confidence_breakdown;
                card.evidence = old.evidence;
                card.updated_at_ms = old.updated_at_ms;
            } else {
                events.push(AdviceEvent::Updated(card.clone()));
                self.last_emitted
                    .insert(card.key.clone(), (now, card.severity));
            }
            // Fresh evidence always extends visibility even if presentation did
            // not need an update event.
            card.expires_at_ms = now.saturating_add(self.config.advice_ttl_ms);
            self.active.insert(card.key.clone(), card);
        }

        // Register newly observed candidates and choose at most one eligible
        // raise, ranked deterministically.
        let mut eligible = Vec::new();
        for candidate in candidates {
            if self.active.contains_key(&candidate.key) {
                continue;
            }
            let pending = self
                .pending
                .entry(candidate.key.clone())
                .or_insert(Pending { first_seen_ms: now });
            if now.saturating_sub(pending.first_seen_ms) < self.config.sustain_ms {
                continue;
            }
            if let Some((last_ms, old_severity)) = self.last_emitted.get(&candidate.key) {
                let cooling = now.saturating_sub(*last_ms) < self.config.per_key_cooldown_ms;
                if cooling && candidate.severity <= *old_severity {
                    continue;
                }
            }
            eligible.push(candidate);
        }
        eligible.sort_by(|a, b| {
            b.severity
                .cmp(&a.severity)
                .then_with(|| b.confidence.cmp(&a.confidence))
                .then_with(|| a.key.cmp(&b.key))
        });
        let global_ready = self
            .last_global_raise_ms
            .is_none_or(|last| now.saturating_sub(last) >= self.config.global_raise_interval_ms);
        if global_ready {
            if let Some(candidate) = eligible.into_iter().next() {
                let card = self.make_card(&candidate, now, now);
                self.pending.remove(&candidate.key);
                self.last_emitted
                    .insert(candidate.key.clone(), (now, candidate.severity));
                self.last_global_raise_ms = Some(now);
                self.active.insert(candidate.key.clone(), card.clone());
                events.push(AdviceEvent::Raised(card));
            }
        }

        AdviceBatch {
            active: self.ranked_active(),
            events,
            suppressed: None,
        }
    }

    fn gate(&self, snapshot: &TelemetrySnapshot) -> Option<SuppressionReason> {
        let meta = &snapshot.meta;
        if meta.schema_version != SNAPSHOT_SCHEMA_VERSION {
            return Some(SuppressionReason::UnsupportedSchema);
        }
        if meta.completeness != SnapshotCompleteness::Complete {
            return Some(SuppressionReason::PartialSnapshot);
        }
        if snapshot.economy.is_none() || snapshot.population.is_none() || snapshot.labor.is_none() {
            return Some(SuppressionReason::PartialSnapshot);
        }
        if !meta.coherent {
            return Some(SuppressionReason::IncoherentSnapshot);
        }
        match meta.identity {
            HumanIdentity::Ambiguous => return Some(SuppressionReason::AmbiguousIdentity),
            HumanIdentity::NonHuman => return Some(SuppressionReason::NonHuman),
            HumanIdentity::Confirmed => {}
        }
        match meta.game_mode {
            GameMode::SinglePlayer => {}
            GameMode::Multiplayer => return Some(SuppressionReason::Multiplayer),
            GameMode::Unknown => return Some(SuppressionReason::UnknownGameMode),
        }
        if meta.paused {
            return Some(SuppressionReason::Paused);
        }
        if meta.captured_at_ms > meta.analyzed_at_ms
            || meta.analyzed_at_ms.saturating_sub(meta.captured_at_ms)
                > self.config.max_snapshot_age_ms
        {
            return Some(SuppressionReason::StaleSnapshot);
        }
        if self
            .last_analyzed_ms
            .is_some_and(|last| meta.analyzed_at_ms < last)
        {
            return Some(SuppressionReason::TimeWentBackwards);
        }
        if self
            .last_sequence
            .is_some_and(|last| meta.sample_sequence <= last)
            || self
                .last_game_tick
                .is_some_and(|last| meta.game_tick <= last)
        {
            return Some(SuppressionReason::SampleWentBackwards);
        }
        let transport_delay = meta.analyzed_at_ms.saturating_sub(meta.captured_at_ms);
        let base_fields = [
            snapshot.economy.as_ref().map(|x| x.provenance),
            snapshot.population.as_ref().map(|x| x.provenance),
            snapshot.labor.as_ref().map(|x| x.provenance),
        ];
        if base_fields.into_iter().any(|field| {
            field.is_none_or(|p| {
                p.age_ms.saturating_add(transport_delay) > self.config.max_field_age_ms
            })
        }) {
            return Some(SuppressionReason::StaleSnapshot);
        }
        if !valid_values(snapshot, &self.config) {
            return Some(SuppressionReason::InvalidValues);
        }
        None
    }

    fn record_position(&mut self, snapshot: &TelemetrySnapshot, reason: SuppressionReason) {
        // Data-quality failures should not let a racy/stale sequence poison the
        // last coherent position.  A monotonic pause is safe to remember.
        if reason == SuppressionReason::Paused {
            self.record_valid_position(snapshot);
        }
    }

    fn record_valid_position(&mut self, snapshot: &TelemetrySnapshot) {
        self.last_sequence = Some(snapshot.meta.sample_sequence);
        self.last_game_tick = Some(snapshot.meta.game_tick);
        self.last_analyzed_ms = Some(snapshot.meta.analyzed_at_ms);
    }

    fn retract_unavailable_optional_sources(
        &mut self,
        snapshot: &TelemetrySnapshot,
        events: &mut Vec<AdviceEvent>,
    ) {
        let delay = snapshot
            .meta
            .analyzed_at_ms
            .saturating_sub(snapshot.meta.captured_at_ms);
        let optional = [
            (
                AdviceCategory::Production,
                optional_state(snapshot.production.as_ref(), delay, &self.config),
            ),
            (
                AdviceCategory::Progression,
                optional_state(snapshot.progression.as_ref(), delay, &self.config),
            ),
            (
                AdviceCategory::Pressure,
                if self.config.enable_pressure_advice {
                    optional_state(snapshot.pressure.as_ref(), delay, &self.config)
                } else {
                    OptionalState::Fresh
                },
            ),
        ];
        for (category, state) in optional {
            if state == OptionalState::Fresh {
                continue;
            }
            let keys: Vec<_> = self
                .active
                .iter()
                .filter_map(|(key, card)| (card.category == category).then_some(key.clone()))
                .collect();
            for key in keys {
                self.active.remove(&key);
                self.missing.remove(&key);
                events.push(AdviceEvent::Retracted {
                    key,
                    reason: RetractionReason::DataQuality(match state {
                        OptionalState::Missing => SuppressionReason::PartialSnapshot,
                        OptionalState::Stale => SuppressionReason::StaleSnapshot,
                        OptionalState::Fresh => unreachable!(),
                    }),
                });
            }
        }
    }

    fn candidates(&self, snapshot: &TelemetrySnapshot) -> Vec<Candidate> {
        let mut result = Vec::new();
        if let Some(observed) = snapshot.economy.as_ref() {
            self.economy_candidates(observed, &mut result);
        }
        if let Some(observed) = snapshot.population.as_ref() {
            self.population_candidates(observed, &mut result);
        }
        if let Some(observed) = snapshot.labor.as_ref() {
            self.labor_candidates(observed, &mut result);
        }
        let delay = snapshot
            .meta
            .analyzed_at_ms
            .saturating_sub(snapshot.meta.captured_at_ms);
        if let Some(observed) = fresh_optional(snapshot.production.as_ref(), delay, &self.config) {
            self.production_candidates(observed, &mut result);
        }
        if let (Some(economy), Some(progression)) = (
            snapshot.economy.as_ref(),
            fresh_optional(snapshot.progression.as_ref(), delay, &self.config),
        ) {
            self.progression_candidates(economy, progression, &mut result);
        }
        if self.config.enable_pressure_advice {
            if let Some(observed) = fresh_optional(snapshot.pressure.as_ref(), delay, &self.config)
            {
                self.pressure_candidates(observed, &mut result);
            }
        }
        result.retain(|candidate| candidate.confidence >= self.config.minimum_confidence);
        result
    }

    fn economy_candidates(&self, observed: &Observed<EconomyTelemetry>, out: &mut Vec<Candidate>) {
        let data = &observed.value;
        for resource in Resource::ALL {
            let commerce_status = data.commerce_status.raw(resource);
            if commerce_status != 0 {
                out.push(Candidate {
                    rule: RuleRef { id: "economy.commerce_status", version: 1 },
                    key: AdviceKey::CommerceCap(resource),
                    category: AdviceCategory::Economy,
                    severity: Severity::Notice,
                    confidence: confidence(observed.provenance, 900),
                    confidence_breakdown: confidence_breakdown(observed.provenance, 900),
                    evidence: vec![evidence(
                        "commerce_over_cap",
                        commerce_status as i64,
                        Some(0),
                        "raw-status",
                        observed.provenance,
                    )],
                    message: format!(
                        "{} commerce status is {} (nonzero); additional gatherers may not increase displayed income until Commerce or its cap rises.",
                        title(resource.label()), commerce_status
                    ),
                });
            }

            let stock = data.stock.get(resource);
            let income = data.income_per_minute.get(resource).max(0);
            let projected = stock.saturating_add(
                income.saturating_mul(self.config.planning_horizon_ms as i64) / 60_000,
            );
            let committed = data.planned_unpaid_cost.get(resource);
            if committed > projected {
                let shortfall = committed - projected;
                let severity = if shortfall > committed / 2 {
                    Severity::Warning
                } else {
                    Severity::Notice
                };
                out.push(Candidate {
                    rule: RuleRef { id: "economy.planned_shortfall", version: 1 },
                    key: AdviceKey::ResourceBottleneck(resource),
                    category: AdviceCategory::Economy,
                    severity,
                    confidence: confidence(observed.provenance, 850),
                    confidence_breakdown: confidence_breakdown(observed.provenance, 850),
                    evidence: vec![
                        evidence("stock", stock, None, "milli-resource", observed.provenance),
                        evidence(
                            "income_per_minute",
                            income,
                            None,
                            "milli-resource/min",
                            observed.provenance,
                        ),
                        evidence(
                            "planned_unpaid_cost",
                            committed,
                            Some(projected),
                            "milli-resource",
                            observed.provenance,
                        ),
                    ],
                    message: format!(
                        "Tracked spending has an approximate {} shortfall under fixed income and no other spending.",
                        resource.label()
                    ),
                });
            }
        }
    }

    fn population_candidates(
        &self,
        observed: &Observed<PopulationTelemetry>,
        out: &mut Vec<Candidate>,
    ) {
        let data = &observed.value;
        // used > cap is legal in retail.  It is not itself an error or advice.
        let available = data
            .cap
            .saturating_add(data.incoming_capacity)
            .saturating_sub(data.used);
        let capacity_is_timely = match (data.next_paid_completion_ms, data.incoming_capacity_eta_ms)
        {
            (Some(queue_eta), Some(cap_eta)) => cap_eta <= queue_eta,
            _ => false,
        };
        let projected_block = data.paid_queue_population > available && !capacity_is_timely;
        if data.blocked_paid_population > 0 || projected_block {
            out.push(Candidate {
                rule: RuleRef {
                    id: "population.paid_queue_block",
                    version: 1,
                },
                key: AdviceKey::PopulationBlock,
                category: AdviceCategory::Population,
                severity: if data.blocked_paid_population > 0 {
                    Severity::Warning
                } else {
                    Severity::Notice
                },
                confidence: confidence(observed.provenance, 950),
                confidence_breakdown: confidence_breakdown(observed.provenance, 950),
                evidence: vec![
                    evidence(
                        "population_used",
                        data.used as i64,
                        Some(data.cap as i64),
                        "pop",
                        observed.provenance,
                    ),
                    evidence(
                        "paid_queue_population",
                        data.paid_queue_population as i64,
                        Some(available as i64),
                        "pop",
                        observed.provenance,
                    ),
                    evidence(
                        "blocked_paid_population",
                        data.blocked_paid_population as i64,
                        Some(0),
                        "pop",
                        observed.provenance,
                    ),
                ],
                message:
                    "An already-paid queue is blocked or projected to block on population capacity."
                        .into(),
            });
        }
    }

    fn labor_candidates(&self, observed: &Observed<LaborTelemetry>, out: &mut Vec<Candidate>) {
        let data = &observed.value;
        let total_free: u32 = Resource::ALL
            .into_iter()
            .map(|resource| data.free_gather_slots.get(resource) as u32)
            .sum();
        if data.idle_citizens >= self.config.idle_citizen_threshold
            && data.idle_for_ms >= self.config.idle_citizen_ms
            && total_free > 0
        {
            out.push(Candidate {
                rule: RuleRef {
                    id: "labor.sustained_idle",
                    version: 1,
                },
                key: AdviceKey::IdleCitizens,
                category: AdviceCategory::Labor,
                severity: if data.idle_citizens.saturating_mul(10) >= data.citizens.max(1) {
                    Severity::Warning
                } else {
                    Severity::Notice
                },
                confidence: confidence(observed.provenance, 950),
                confidence_breakdown: confidence_breakdown(observed.provenance, 950),
                evidence: vec![
                    evidence(
                        "idle_citizens",
                        data.idle_citizens as i64,
                        Some(self.config.idle_citizen_threshold as i64),
                        "citizens",
                        observed.provenance,
                    ),
                    evidence(
                        "idle_duration",
                        data.idle_for_ms as i64,
                        Some(self.config.idle_citizen_ms as i64),
                        "ms",
                        observed.provenance,
                    ),
                    evidence(
                        "valid_free_gather_slots",
                        total_free as i64,
                        Some(1),
                        "slots",
                        observed.provenance,
                    ),
                ],
                message: format!(
                    "{} Citizens have been idle; {} validated gather slots are free.",
                    data.idle_citizens, total_free
                ),
            });
        }
        if let Some(rebalance) = &data.rebalance {
            if rebalance.workers > 0
                && rebalance.destination_free_slots >= rebalance.workers
                && rebalance.path_feasible
                && rebalance.marginal_gain_milli_per_minute > 0
                && rebalance.commerce_headroom_milli > 0
            {
                out.push(Candidate {
                    rule: RuleRef { id: "labor.rebalance_model", version: 1 },
                    key: AdviceKey::LaborRebalance(rebalance.from, rebalance.to),
                    category: AdviceCategory::Labor,
                    severity: Severity::Notice,
                    confidence: confidence(observed.provenance, 800),
                    confidence_breakdown: confidence_breakdown(observed.provenance, 800),
                    evidence: vec![
                        evidence("workers", rebalance.workers as i64, None, "citizens", observed.provenance),
                        evidence("destination_free_slots", rebalance.destination_free_slots as i64, Some(rebalance.workers as i64), "slots", observed.provenance),
                        evidence("modeled_marginal_gain", rebalance.marginal_gain_milli_per_minute, Some(1), "milli-resource/min", observed.provenance),
                        evidence("commerce_headroom", rebalance.commerce_headroom_milli, Some(1), "milli-resource", observed.provenance),
                    ],
                    message: format!(
                        "A model estimates a gain from moving {} workers from {} to {}; path and capacity checks passed.",
                        rebalance.workers,
                        rebalance.from.label(),
                        rebalance.to.label()
                    ),
                });
            }
        }
    }

    fn production_candidates(
        &self,
        observed: &Observed<ProductionTelemetry>,
        out: &mut Vec<Candidate>,
    ) {
        for site in &observed.value.sites {
            if site.enabled
                && site.expected_active
                && site.queue_len == 0
                && site.idle_for_ms >= self.config.idle_production_ms
                && site.affordable_option_observed
            {
                out.push(Candidate {
                    rule: RuleRef {
                        id: "production.expected_idle",
                        version: 1,
                    },
                    key: AdviceKey::IdleProduction(site.stable_id),
                    category: AdviceCategory::Production,
                    severity: Severity::Notice,
                    confidence: confidence(observed.provenance, 900),
                    confidence_breakdown: confidence_breakdown(observed.provenance, 900),
                    evidence: vec![
                        evidence("queue_length", 0, Some(1), "orders", observed.provenance),
                        evidence(
                            "idle_duration",
                            site.idle_for_ms as i64,
                            Some(self.config.idle_production_ms as i64),
                            "ms",
                            observed.provenance,
                        ),
                        evidence(
                            "affordable_option_observed",
                            1,
                            Some(1),
                            "bool",
                            observed.provenance,
                        ),
                    ],
                    message: format!(
                        "Tracked {} is idle with an empty queue and an affordable option.",
                        site.class.label()
                    ),
                });
            }
        }
    }

    fn progression_candidates(
        &self,
        economy: &Observed<EconomyTelemetry>,
        progression: &Observed<ProgressionTelemetry>,
        out: &mut Vec<Candidate>,
    ) {
        let transport = economy
            .provenance
            .transport_confidence
            .min(progression.provenance.transport_confidence);
        let semantic = economy
            .provenance
            .semantic_confidence
            .min(progression.provenance.semantic_confidence);
        let breakdown = ConfidenceBreakdown {
            transport,
            semantic,
            inference: Confidence::new(900),
        };
        let confidence = breakdown.overall();
        for opportunity in &progression.value.opportunities {
            if opportunity.tracked_goal
                && !opportunity.already_queued
                && economy.value.stock.can_afford(opportunity.cost)
            {
                out.push(Candidate {
                    rule: RuleRef {
                        id: "progression.tracked_affordable",
                        version: 1,
                    },
                    key: AdviceKey::ProgressionAffordable(opportunity.stable_id),
                    category: AdviceCategory::Progression,
                    severity: Severity::Info,
                    confidence,
                    confidence_breakdown: breakdown,
                    evidence: vec![evidence(
                        "tracked_goal_affordable",
                        1,
                        Some(1),
                        "bool",
                        progression.provenance,
                    )],
                    message: format!("Tracked goal '{}' is now affordable.", opportunity.name),
                });
            }
        }
    }

    fn pressure_candidates(
        &self,
        observed: &Observed<PressureTelemetry>,
        out: &mut Vec<Candidate>,
    ) {
        let data = &observed.value;
        let zero_denominator = if data.observed_enemy_local_strength > 0 {
            u32::MAX
        } else {
            0
        };
        let ratio = data
            .observed_enemy_local_strength
            .saturating_mul(1_000)
            .checked_div(data.own_local_strength)
            .unwrap_or(zero_denominator);
        if data.under_attack
            && data.enemy_visibility == EnemyVisibility::DirectlyVisible
            && data.enemy_observation_age_ms <= self.config.max_field_age_ms
            && ratio >= self.config.pressure_ratio_permille
        {
            out.push(Candidate {
                rule: RuleRef { id: "pressure.visible_local_ratio", version: 1 },
                key: AdviceKey::LocalMilitaryPressure,
                category: AdviceCategory::Pressure,
                severity: if ratio >= 2_000 { Severity::Critical } else { Severity::Warning },
                confidence: confidence(observed.provenance, 800),
                confidence_breakdown: confidence_breakdown(observed.provenance, 800),
                evidence: vec![
                    evidence("under_attack", 1, Some(1), "bool", observed.provenance),
                    evidence("enemy_to_own_strength", ratio as i64, Some(self.config.pressure_ratio_permille as i64), "permille", observed.provenance),
                ],
                message: "Observed local enemy strength exceeds nearby friendly strength during an active attack.".into(),
            });
            if data.recent_economy_spend_permille >= self.config.economy_exposure_permille {
                out.push(Candidate {
                    rule: RuleRef {
                        id: "pressure.economy_exposure",
                        version: 1,
                    },
                    key: AdviceKey::EconomyUnderPressure,
                    category: AdviceCategory::Pressure,
                    severity: Severity::Warning,
                    confidence: confidence(observed.provenance, 750),
                    confidence_breakdown: confidence_breakdown(observed.provenance, 750),
                    evidence: vec![
                        evidence(
                            "recent_economy_spend",
                            data.recent_economy_spend_permille as i64,
                            Some(self.config.economy_exposure_permille as i64),
                            "permille",
                            observed.provenance,
                        ),
                        evidence(
                            "enemy_to_own_strength",
                            ratio as i64,
                            Some(self.config.pressure_ratio_permille as i64),
                            "permille",
                            observed.provenance,
                        ),
                    ],
                    message:
                        "Recent spending is economy-heavy while local military pressure is high."
                            .into(),
                });
            }
        }
    }

    fn make_card(&self, candidate: &Candidate, created_at_ms: u64, now: u64) -> AdviceCard {
        AdviceCard {
            rule: candidate.rule,
            key: candidate.key.clone(),
            category: candidate.category,
            severity: candidate.severity,
            confidence: candidate.confidence,
            confidence_breakdown: candidate.confidence_breakdown,
            evidence: candidate.evidence.clone(),
            message: candidate.message.clone(),
            created_at_ms,
            updated_at_ms: now,
            expires_at_ms: now.saturating_add(self.config.advice_ttl_ms),
            cooldown_ms: self.config.per_key_cooldown_ms,
        }
    }

    fn ranked_active(&self) -> Vec<AdviceCard> {
        let mut cards: Vec<_> = self.active.values().cloned().collect();
        cards.sort_by(|a, b| {
            b.severity
                .cmp(&a.severity)
                .then_with(|| b.confidence.cmp(&a.confidence))
                .then_with(|| a.key.cmp(&b.key))
        });
        cards
    }

    fn retract_all(&mut self, reason: RetractionReason, events: &mut Vec<AdviceEvent>) {
        for key in self.active.keys().cloned().collect::<Vec<_>>() {
            events.push(AdviceEvent::Retracted { key, reason });
        }
        self.active.clear();
    }
}

fn evidence(
    input: &'static str,
    observed: i64,
    threshold: Option<i64>,
    unit: &'static str,
    provenance: FieldProvenance,
) -> Evidence {
    Evidence {
        input,
        observed,
        threshold,
        unit,
        provenance,
    }
}

fn confidence_breakdown(provenance: FieldProvenance, inference: u16) -> ConfidenceBreakdown {
    ConfidenceBreakdown {
        transport: provenance.transport_confidence,
        semantic: provenance.semantic_confidence,
        inference: Confidence::new(inference),
    }
}

fn confidence(provenance: FieldProvenance, inference: u16) -> Confidence {
    confidence_breakdown(provenance, inference).overall()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OptionalState {
    Fresh,
    Missing,
    Stale,
}

fn optional_state<T>(
    value: Option<&Observed<T>>,
    transport_delay_ms: u64,
    config: &CoachConfig,
) -> OptionalState {
    match value {
        None => OptionalState::Missing,
        Some(value)
            if value.provenance.age_ms.saturating_add(transport_delay_ms)
                > config.max_field_age_ms =>
        {
            OptionalState::Stale
        }
        Some(_) => OptionalState::Fresh,
    }
}

fn fresh_optional<'a, T>(
    value: Option<&'a Observed<T>>,
    transport_delay_ms: u64,
    config: &CoachConfig,
) -> Option<&'a Observed<T>> {
    (optional_state(value, transport_delay_ms, config) == OptionalState::Fresh)
        .then_some(value)
        .flatten()
}

fn valid_values(snapshot: &TelemetrySnapshot, config: &CoachConfig) -> bool {
    let Some(economy) = snapshot.economy.as_ref() else {
        return false;
    };
    if Resource::ALL.into_iter().any(|resource| {
        economy.value.stock.get(resource) < 0
            || economy.value.planned_unpaid_cost.get(resource) < 0
            || economy.value.commerce_cap.get(resource) < 0
            || economy.value.commerce_status.raw(resource) > 2
    }) {
        return false;
    }

    let Some(population) = snapshot.population.as_ref() else {
        return false;
    };
    if population.value.blocked_paid_population > population.value.paid_queue_population {
        return false;
    }

    let Some(labor) = snapshot.labor.as_ref() else {
        return false;
    };
    if labor.value.idle_citizens > labor.value.citizens {
        return false;
    }

    if let Some(progression) = snapshot.progression.as_ref() {
        let mut ids = BTreeSet::new();
        if progression.value.opportunities.iter().any(|opportunity| {
            !ids.insert(opportunity.stable_id)
                || Resource::ALL
                    .into_iter()
                    .any(|resource| opportunity.cost.get(resource) < 0)
        }) {
            return false;
        }
    }

    if let Some(production) = snapshot.production.as_ref() {
        let mut ids = BTreeSet::new();
        if production
            .value
            .sites
            .iter()
            .any(|site| !ids.insert(site.stable_id))
        {
            return false;
        }
    }

    if labor
        .value
        .rebalance
        .as_ref()
        .is_some_and(|rebalance| rebalance.from == rebalance.to)
    {
        return false;
    }

    !config.enable_pressure_advice
        || snapshot
            .pressure
            .as_ref()
            .is_none_or(|pressure| pressure.value.recent_economy_spend_permille <= 1_000)
}

fn title(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().chain(chars).collect(),
    }
}
