use std::collections::BTreeSet;
use std::fmt;

use crate::model::*;
use crate::PROTOCOL_MAJOR;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ValidationLimits {
    pub max_sources: usize,
    pub max_players: usize,
    pub max_resources_per_player: usize,
    pub max_type_counts_per_player: usize,
    pub max_entities: usize,
    pub max_queues_per_entity: usize,
    pub max_items_per_queue: usize,
    pub max_warnings: usize,
    pub max_advice: usize,
    pub max_events: usize,
    pub max_actions_per_advice: usize,
    pub max_unknown_fields_per_record: usize,
    pub max_unknown_bytes_per_record: usize,
    pub max_text_bytes: usize,
    pub max_event_payload_bytes: usize,
    pub max_gather_cache_age_frames: u32,
}

impl Default for ValidationLimits {
    fn default() -> Self {
        Self {
            max_sources: 64,
            max_players: 16,
            max_resources_per_player: 64,
            max_type_counts_per_player: 1_024,
            max_entities: 100_000,
            max_queues_per_entity: 16,
            max_items_per_queue: 256,
            max_warnings: 4_096,
            max_advice: 1_024,
            max_events: 65_536,
            max_actions_per_advice: 32,
            max_unknown_fields_per_record: 256,
            max_unknown_bytes_per_record: 1024 * 1024,
            max_text_bytes: 64 * 1024,
            max_event_payload_bytes: 1024 * 1024,
            max_gather_cache_age_frames: 1_000_000,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationError {
    pub path: String,
    pub message: String,
}

impl ValidationError {
    fn new(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path, self.message)
    }
}

impl std::error::Error for ValidationError {}

pub trait Validate {
    fn validate(&self, limits: ValidationLimits) -> Result<(), ValidationError>;
}

impl Validate for Frame {
    fn validate(&self, limits: ValidationLimits) -> Result<(), ValidationError> {
        if self.protocol_major != PROTOCOL_MAJOR {
            return Err(ValidationError::new(
                "protocol_major",
                format!(
                    "unsupported major {}; expected {}",
                    self.protocol_major, PROTOCOL_MAJOR
                ),
            ));
        }
        match &self.message {
            Message::Hello(v) => {
                text("message.hello.producer", &v.producer, limits)?;
                text(
                    "message.hello.producer_version",
                    &v.producer_version,
                    limits,
                )?;
                for (i, value) in v.capabilities.iter().enumerate() {
                    text(&format!("message.hello.capabilities[{i}]"), value, limits)?;
                }
                unknown("message.hello.unknown", &v.unknown, limits)
            }
            Message::Snapshot(v) => validate_snapshot(v, limits),
            Message::Events(v) => validate_events(v, limits),
            Message::Heartbeat(v) => {
                text(
                    "message.heartbeat.producer_state",
                    &v.producer_state,
                    limits,
                )?;
                unknown("message.heartbeat.unknown", &v.unknown, limits)
            }
            Message::Unknown { payload, .. } => {
                if payload.len() > limits.max_event_payload_bytes {
                    Err(ValidationError::new(
                        "message.unknown.payload",
                        "payload exceeds configured limit",
                    ))
                } else {
                    Ok(())
                }
            }
        }
    }
}

fn validate_snapshot(s: &Snapshot, l: ValidationLimits) -> Result<(), ValidationError> {
    bounded("snapshot.sources", s.sources.len(), l.max_sources)?;
    bounded("snapshot.players", s.players.len(), l.max_players)?;
    bounded("snapshot.entities", s.entities.len(), l.max_entities)?;
    bounded("snapshot.warnings", s.warnings.len(), l.max_warnings)?;
    bounded("snapshot.advice", s.advice.len(), l.max_advice)?;
    unknown("snapshot.unknown", &s.unknown, l)?;
    unknown("snapshot.meta.unknown", &s.meta.unknown, l)?;

    if let Some(local) = &s.local_human {
        text("snapshot.local_human.identity_key", &local.identity_key, l)?;
        unknown("snapshot.local_human.unknown", &local.unknown, l)?;
        if local.confirmed_local && local.identity_key.is_empty() {
            return Err(ValidationError::new(
                "snapshot.local_human.identity_key",
                "confirmed local human requires a nonempty identity key",
            ));
        }
    }

    if matches!(s.meta.coherence, Coherence::Coherent | Coherence::Paused) {
        match (s.meta.sampled_frame_start, s.meta.sampled_frame_end) {
            (Some(a), Some(b)) if a == b => {}
            _ => return Err(ValidationError::new(
                "snapshot.meta.coherence",
                "exact/stable snapshot requires equal sampled_frame_start and sampled_frame_end",
            )),
        }
    }
    if !s.advice_allowed() && !s.advice.is_empty() {
        return Err(ValidationError::new(
            "snapshot.advice",
            "advice requires coherent single-player capture, confirmed local human, own-player scope, and a complete process-memory source",
        ));
    }

    if s.scope == ObservationScope::OwnPlayerOnly {
        let local = s.local_human.as_ref().ok_or_else(|| {
            ValidationError::new(
                "snapshot.local_human",
                "own-player scope requires local human identity",
            )
        })?;
        if s.players
            .iter()
            .any(|player| player.player_id != local.player_id)
        {
            return Err(ValidationError::new(
                "snapshot.players",
                "own-player scope contains an opponent economy",
            ));
        }
        if s.entities.iter().any(|entity| {
            entity
                .owner_id
                .is_some_and(|owner| owner != local.player_id)
                || matches!(
                    entity.visibility,
                    VisibilityBasis::Hidden
                        | VisibilityBasis::Remembered
                        | VisibilityBasis::Unknown
                )
        }) {
            return Err(ValidationError::new(
                "snapshot.entities",
                "own-player scope contains an opponent or non-currently-visible entity",
            ));
        }
        if s.warnings
            .iter()
            .any(|warning| warning.player_id.is_some_and(|id| id != local.player_id))
        {
            return Err(ValidationError::new(
                "snapshot.warnings",
                "own-player scope warning references another player",
            ));
        }
    }

    for (i, source) in s.sources.iter().enumerate() {
        text(
            &format!("snapshot.sources[{i}].source_key"),
            &source.source_key,
            l,
        )?;
        text(
            &format!("snapshot.sources[{i}].build_id"),
            &source.build_id,
            l,
        )?;
        text(&format!("snapshot.sources[{i}].detail"), &source.detail, l)?;
        unknown(
            &format!("snapshot.sources[{i}].unknown"),
            &source.unknown,
            l,
        )?;
    }

    let mut player_ids = BTreeSet::new();
    for (i, player) in s.players.iter().enumerate() {
        let base = format!("snapshot.players[{i}]");
        if !player_ids.insert(player.player_id) {
            return Err(ValidationError::new(
                format!("{base}.player_id"),
                "duplicate player id",
            ));
        }
        text(&format!("{base}.name"), &player.name, l)?;
        bounded(
            &format!("{base}.resources"),
            player.resources.len(),
            l.max_resources_per_player,
        )?;
        bounded(
            &format!("{base}.queued_type_counts"),
            player.queued_type_counts.len(),
            l.max_type_counts_per_player,
        )?;
        validate_evidence(
            &format!("{base}.evidence"),
            &player.evidence,
            s.sources.len(),
            l,
        )?;
        unknown(&format!("{base}.unknown"), &player.unknown, l)?;
        unknown(
            &format!("{base}.population.unknown"),
            &player.population.unknown,
            l,
        )?;
        unknown(
            &format!("{base}.workers.unknown"),
            &player.workers.unknown,
            l,
        )?;
        match (player.gather_stamp, player.gather_cache_age_frames) {
            (Some(stamp), Some(age)) => {
                if age > l.max_gather_cache_age_frames {
                    return Err(ValidationError::new(
                        format!("{base}.gather_cache_age_frames"),
                        "gather cache age exceeds configured bound",
                    ));
                }
                if let Some(frame) = s.meta.game_frame {
                    if frame.wrapping_sub(stamp) != u64::from(age) {
                        return Err(ValidationError::new(
                            format!("{base}.gather_cache_age_frames"),
                            "does not match wrapping game_frame - gather_stamp",
                        ));
                    }
                }
            }
            (None, None) => {}
            _ => {
                return Err(ValidationError::new(
                    format!("{base}.gather_stamp"),
                    "gather stamp and cache age must be present together",
                ));
            }
        }
        let mut queued_types = BTreeSet::new();
        for (j, count) in player.queued_type_counts.iter().enumerate() {
            let path = format!("{base}.queued_type_counts[{j}]");
            if !queued_types.insert(count.type_id) {
                return Err(ValidationError::new(
                    format!("{path}.type_id"),
                    "duplicate queued type id",
                ));
            }
            unknown(&format!("{path}.unknown"), &count.unknown, l)?;
        }
        let mut kinds = BTreeSet::new();
        for (j, resource) in player.resources.iter().enumerate() {
            let rb = format!("{base}.resources[{j}]");
            if matches!(&resource.kind, ResourceKind::Custom(value) if *value < 6) {
                return Err(ValidationError::new(
                    format!("{rb}.kind"),
                    "custom resource indices 0..=5 collide with retail resources",
                ));
            }
            if !kinds.insert(resource.kind.code()) {
                return Err(ValidationError::new(
                    format!("{rb}.kind"),
                    "duplicate resource kind",
                ));
            }
            if resource.income_sixteenths_per_period.is_some()
                && resource.income_basis == RateBasis::Unknown
            {
                return Err(ValidationError::new(format!("{rb}.income_basis"), "a present income rate must declare EngineDirect, ObservedDelta, or Modeled basis"));
            }
            if resource.income_sixteenths_per_period.is_none()
                && resource.income_basis != RateBasis::Unknown
            {
                return Err(ValidationError::new(
                    format!("{rb}.income_basis"),
                    "rate basis is set but income rate is absent",
                ));
            }
            if resource.over_cap_status.is_some_and(|status| status > 2) {
                return Err(ValidationError::new(
                    format!("{rb}.over_cap_status"),
                    "known retail status must be in 0..=2",
                ));
            }
            validate_evidence(
                &format!("{rb}.evidence"),
                &resource.evidence,
                s.sources.len(),
                l,
            )?;
            unknown(&format!("{rb}.unknown"), &resource.unknown, l)?;
        }
    }

    let mut object_ids = BTreeSet::new();
    for (i, entity) in s.entities.iter().enumerate() {
        let base = format!("snapshot.entities[{i}]");
        if !object_ids.insert(entity.object_id) {
            return Err(ValidationError::new(
                format!("{base}.object_id"),
                "duplicate object id",
            ));
        }
        if entity.build_progress_ppm.is_some_and(|v| v > 1_000_000) {
            return Err(ValidationError::new(
                format!("{base}.build_progress_ppm"),
                "must be <= 1,000,000",
            ));
        }
        bounded(
            &format!("{base}.queues"),
            entity.queues.len(),
            l.max_queues_per_entity,
        )?;
        validate_evidence(
            &format!("{base}.evidence"),
            &entity.evidence,
            s.sources.len(),
            l,
        )?;
        unknown(&format!("{base}.unknown"), &entity.unknown, l)?;
        for (j, queue) in entity.queues.iter().enumerate() {
            let qb = format!("{base}.queues[{j}]");
            bounded(
                &format!("{qb}.items"),
                queue.items.len(),
                l.max_items_per_queue,
            )?;
            unknown(&format!("{qb}.unknown"), &queue.unknown, l)?;
            for (k, item) in queue.items.iter().enumerate() {
                if item.progress_ppm.is_some_and(|v| v > 1_000_000) {
                    return Err(ValidationError::new(
                        format!("{qb}.items[{k}].progress_ppm"),
                        "must be <= 1,000,000",
                    ));
                }
                unknown(&format!("{qb}.items[{k}].unknown"), &item.unknown, l)?;
            }
        }
    }

    for (i, warning) in s.warnings.iter().enumerate() {
        let base = format!("snapshot.warnings[{i}]");
        text(&format!("{base}.code"), &warning.code, l)?;
        text(&format!("{base}.headline"), &warning.headline, l)?;
        text(&format!("{base}.detail"), &warning.detail, l)?;
        validate_evidence(
            &format!("{base}.evidence"),
            &warning.evidence,
            s.sources.len(),
            l,
        )?;
        unknown(&format!("{base}.unknown"), &warning.unknown, l)?;
    }
    for (i, advice) in s.advice.iter().enumerate() {
        let base = format!("snapshot.advice[{i}]");
        if advice.priority_bps > 10_000 {
            return Err(ValidationError::new(
                format!("{base}.priority_bps"),
                "must be <= 10,000",
            ));
        }
        text(&format!("{base}.headline"), &advice.headline, l)?;
        text(&format!("{base}.rationale"), &advice.rationale, l)?;
        text(&format!("{base}.rule_id"), &advice.rule_id, l)?;
        text(&format!("{base}.rule_version"), &advice.rule_version, l)?;
        if advice.rule_id.is_empty()
            || advice.rule_version.is_empty()
            || advice.lifecycle == AdviceLifecycle::Unknown
        {
            return Err(ValidationError::new(
                base,
                "advice requires rule_id, rule_version, and a known lifecycle",
            ));
        }
        bounded(
            &format!("{base}.actions"),
            advice.actions.len(),
            l.max_actions_per_advice,
        )?;
        for (j, action) in advice.actions.iter().enumerate() {
            text(&format!("{base}.actions[{j}]"), action, l)?;
        }
        validate_evidence(
            &format!("{base}.evidence"),
            &advice.evidence,
            s.sources.len(),
            l,
        )?;
        unknown(&format!("{base}.unknown"), &advice.unknown, l)?;
    }
    Ok(())
}

fn validate_events(batch: &EventBatch, l: ValidationLimits) -> Result<(), ValidationError> {
    bounded("events.events", batch.events.len(), l.max_events)?;
    unknown("events.unknown", &batch.unknown, l)?;
    for (i, event) in batch.events.iter().enumerate() {
        let base = format!("events.events[{i}]");
        text(&format!("{base}.kind"), &event.kind, l)?;
        if event.payload.len() > l.max_event_payload_bytes {
            return Err(ValidationError::new(
                format!("{base}.payload"),
                "payload exceeds configured limit",
            ));
        }
        // Event batches may reference a source table established by the latest
        // snapshot, so only the confidence range can be checked here.
        if event.evidence.confidence_bps > 10_000 {
            return Err(ValidationError::new(
                format!("{base}.evidence.confidence_bps"),
                "must be <= 10,000",
            ));
        }
        text(
            &format!("{base}.evidence.calibration_id"),
            &event.evidence.calibration_id,
            l,
        )?;
        if event.evidence.confidence_bps != 0
            && event.evidence.confidence_bps != 10_000
            && event.evidence.calibration_id.is_empty()
        {
            return Err(ValidationError::new(
                format!("{base}.evidence.calibration_id"),
                "non-endpoint confidence requires a versioned calibration method",
            ));
        }
        unknown(
            &format!("{base}.evidence.unknown"),
            &event.evidence.unknown,
            l,
        )?;
        unknown(&format!("{base}.unknown"), &event.unknown, l)?;
    }
    Ok(())
}

fn validate_evidence(
    path: &str,
    value: &Evidence,
    sources: usize,
    l: ValidationLimits,
) -> Result<(), ValidationError> {
    if usize::from(value.source_id) >= sources {
        return Err(ValidationError::new(
            format!("{path}.source_id"),
            "source index is outside snapshot source table",
        ));
    }
    if value.confidence_bps > 10_000 {
        return Err(ValidationError::new(
            format!("{path}.confidence_bps"),
            "must be <= 10,000",
        ));
    }
    text(&format!("{path}.calibration_id"), &value.calibration_id, l)?;
    if value.confidence_bps != 0
        && value.confidence_bps != 10_000
        && value.calibration_id.is_empty()
    {
        return Err(ValidationError::new(
            format!("{path}.calibration_id"),
            "non-endpoint confidence requires a versioned calibration method",
        ));
    }
    unknown(&format!("{path}.unknown"), &value.unknown, l)
}

fn text(path: &str, value: &str, l: ValidationLimits) -> Result<(), ValidationError> {
    if value.len() > l.max_text_bytes {
        Err(ValidationError::new(path, "text exceeds configured limit"))
    } else {
        Ok(())
    }
}

fn bounded(path: &str, actual: usize, limit: usize) -> Result<(), ValidationError> {
    if actual > limit {
        Err(ValidationError::new(
            path,
            format!("contains {actual} items; limit is {limit}"),
        ))
    } else {
        Ok(())
    }
}

fn unknown(
    path: &str,
    fields: &[UnknownField],
    l: ValidationLimits,
) -> Result<(), ValidationError> {
    bounded(path, fields.len(), l.max_unknown_fields_per_record)?;
    let bytes = fields
        .iter()
        .try_fold(0usize, |sum, f| sum.checked_add(f.data.len()))
        .ok_or_else(|| ValidationError::new(path, "unknown field byte count overflow"))?;
    if bytes > l.max_unknown_bytes_per_record {
        Err(ValidationError::new(
            path,
            "unknown fields exceed configured byte limit",
        ))
    } else {
        Ok(())
    }
}
