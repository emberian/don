//! Typed byte ownership for checksum channel 12 (`World::walk_data`).
//!
//! The replay bridge currently carries one scalar `sourced_walked_bytes`.  That is enough
//! to report a ceiling, but it cannot answer the questions a checksum investigation needs:
//! which exact bytes are sourced, which producer last wrote them, and whether a generator
//! receipt changed anything outside its proved output sections.  This isolated frontier
//! keeps those answers without changing the shared replay bridge or schedule.
//!
//! A recorded checksum is deliberately not an ownership source.  It can compare a complete
//! model image, and a captured retail walk can localise a difference, but neither may turn
//! unknown model bytes into owned bytes.

#![forbid(unsafe_code)]

use don_sim::checksum::{adler32, ByteSink, DataWalk};
use don_sim::systems::map_terrain::{World, WorldChecksum, WorldSection};

pub const WORLD_WALKER_VA: u32 = 0x006b_5cf0;
pub const MAP_MAKE_VA: u32 = 0x0068_bc90;
pub const SHIPPED_RULES_CHANNEL: u32 = 0x12ba_3104;
pub const RETAIL_AFTER_CONSTANTS: u32 = 0x5062_5668;
pub const SHIPPED_RULES_SERIALIZED_BYTES: usize = 1_024_221;
pub const MAP_SIZE_WORLD_EDGES: [i32; 7] = [40, 50, 60, 70, 80, 90, 100];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReplaySpan {
    pub offset: usize,
    pub bytes: usize,
}

impl ReplaySpan {
    pub const fn new(offset: usize, bytes: usize) -> Self {
        Self { offset, bytes }
    }

    pub fn end(self) -> Option<usize> {
        self.offset.checked_add(self.bytes)
    }
}

/// Evidence for the replay-carried Rules projection which supplies the six initial
/// territory-limit words in `World` section 4.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RulesWorldEvidence {
    pub serialized_span: ReplaySpan,
    pub serialized_sha256: [u8; 32],
    pub checksum: u32,
    pub after_constants: u32,
    pub player_base: i32,
    pub player_civic: i32,
    pub player_city: i32,
}

/// The only replay/static facts that the initial prefix can lawfully project into the
/// channel-12 byte stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InitialWorldPrefixEvidence {
    pub replay_sha256: [u8; 32],
    pub map_size: u8,
    pub map_size_span: ReplaySpan,
    pub seed: u32,
    pub seed_span: ReplaySpan,
    pub rules: Option<RulesWorldEvidence>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorldByteSource {
    /// A word copied bit-for-bit from the decompressed replay prefix.
    ReplayScalar {
        replay_sha256: [u8; 32],
        span: ReplaySpan,
        field: &'static str,
    },
    /// Dimensions deterministically derived through the shipped seven-row map-size table.
    DerivedMapSize {
        replay_sha256: [u8; 32],
        span: ReplaySpan,
        selector: u8,
        edge: i32,
        table_va_proof: &'static str,
    },
    /// Constants admitted by independently replaying the serialized Rules traversal.
    ReplayRulesProjection {
        serialized_span: ReplaySpan,
        serialized_sha256: [u8; 32],
        checksum: u32,
        after_constants: u32,
    },
    /// Bytes observably changed by an exact port between two bound World snapshots.
    ExactPortTransition {
        entry_va: u32,
        resume_va: u32,
        implementation_sha256: [u8; 32],
        receipt_sha256: [u8; 32],
        proof_document: &'static str,
        input_checksum: u32,
        output_checksum: u32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SectionWindow {
    pub section: WorldSection,
    pub start: usize,
    pub end: usize,
    pub checksum: u32,
}

impl SectionWindow {
    pub fn len(self) -> usize {
        self.end - self.start
    }

    pub fn contains(self, global_offset: usize) -> bool {
        self.start <= global_offset && global_offset < self.end
    }
}

/// One canonical image of the exact retail walker ordering.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorldOwnerSnapshot {
    pub checksum: WorldChecksum,
    pub image: Vec<u8>,
    pub sections: [SectionWindow; WorldSection::COUNT],
}

impl WorldOwnerSnapshot {
    pub fn capture(world: &World) -> Result<Self, WorldOwnerError> {
        let checksum = world.checksum_sections();
        let mut image = Vec::with_capacity(checksum.bytes as usize);
        let mut windows = [SectionWindow {
            section: WorldSection::Dims,
            start: 0,
            end: 0,
            checksum: 1,
        }; WorldSection::COUNT];

        for (index, section) in WorldSection::all().into_iter().enumerate() {
            let mut sink = ByteSink::new();
            world.walk_section(&mut sink, section as i32);
            let start = image.len();
            image.extend_from_slice(&sink.0);
            let end = image.len();
            windows[index] = SectionWindow {
                section,
                start,
                end,
                checksum: sink.checksum(),
            };
            let recorded = checksum.section(section);
            if recorded.bytes != sink.0.len() as u64 || recorded.adler != sink.checksum() {
                return Err(WorldOwnerError::SectionTraversalDrift { section });
            }
        }

        let whole = world.checksum_image();
        if whole.0 != image
            || checksum.bytes != image.len() as u64
            || checksum.full != adler32(1, &image)
        {
            return Err(WorldOwnerError::WholeTraversalDrift);
        }
        Ok(Self {
            checksum,
            image,
            sections: windows,
        })
    }

    pub fn section(&self, section: WorldSection) -> SectionWindow {
        self.sections[section as usize - 1]
    }

    pub fn section_bytes(&self, section: WorldSection) -> &[u8] {
        let window = self.section(section);
        &self.image[window.start..window.end]
    }

    pub fn locate(&self, global_offset: usize) -> Option<(WorldSection, usize)> {
        self.sections
            .iter()
            .find(|window| window.contains(global_offset))
            .map(|window| (window.section, global_offset - window.start))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WorldSectionMask(u16);

impl WorldSectionMask {
    pub const NONE: Self = Self(0);

    pub const fn only(section: WorldSection) -> Self {
        Self(1 << (section as u16 - 1))
    }

    pub fn with(mut self, section: WorldSection) -> Self {
        self.0 |= 1 << (section as u16 - 1);
        self
    }

    pub const fn contains(self, section: WorldSection) -> bool {
        self.0 & (1 << (section as u16 - 1)) != 0
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExactPortTransitionProof {
    pub entry_va: u32,
    pub resume_va: u32,
    pub implementation_sha256: [u8; 32],
    pub receipt_sha256: [u8; 32],
    pub proof_document: &'static str,
    pub input_checksum: u32,
    pub output_checksum: u32,
    pub allowed_sections: WorldSectionMask,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChangedRange {
    pub section: WorldSection,
    pub offset: usize,
    pub bytes: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransitionReceipt {
    pub entry_va: u32,
    pub resume_va: u32,
    pub input_checksum: u32,
    pub output_checksum: u32,
    pub changed_bytes: usize,
    pub changed_ranges: Vec<ChangedRange>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OwnershipCoverage {
    pub walked_bytes: usize,
    pub owned_bytes: usize,
    pub unknown_bytes: usize,
}

/// A byte-level owner map over one `World::walk_data(-1)` image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorldOwnerLedger {
    snapshot: WorldOwnerSnapshot,
    /// Index into `sources`. `None` is explicitly unknown, never an implicit zero owner.
    owners: Vec<Option<usize>>,
    sources: Vec<WorldByteSource>,
}

impl WorldOwnerLedger {
    /// Admit exactly the prefix-proven bytes. With a complete shipped Rules projection this
    /// is 76 bytes: section 1 dimensions (8), ten derived size words (40), six territory
    /// words (24), and the replay seed (4).
    pub fn from_initial_prefix(
        world: &World,
        evidence: InitialWorldPrefixEvidence,
    ) -> Result<Self, WorldOwnerError> {
        validate_digest(evidence.replay_sha256, "replay_sha256")?;
        if evidence.map_size_span.bytes != 1 || evidence.map_size_span.end().is_none() {
            return Err(WorldOwnerError::InvalidReplaySpan {
                field: "map_size",
                span: evidence.map_size_span,
            });
        }
        if evidence.seed_span.bytes != 4 || evidence.seed_span.end().is_none() {
            return Err(WorldOwnerError::InvalidReplaySpan {
                field: "seed",
                span: evidence.seed_span,
            });
        }
        if spans_overlap(evidence.map_size_span, evidence.seed_span) {
            return Err(WorldOwnerError::OverlappingReplaySources {
                left: evidence.map_size_span,
                right: evidence.seed_span,
            });
        }
        let edge = MAP_SIZE_WORLD_EDGES
            .get(evidence.map_size as usize)
            .copied()
            .ok_or(WorldOwnerError::UnknownMapSize(evidence.map_size))?;
        let snapshot = WorldOwnerSnapshot::capture(world)?;
        let mut ledger = Self {
            owners: vec![None; snapshot.image.len()],
            snapshot,
            sources: Vec::new(),
        };

        let dims = [edge.to_le_bytes(), edge.to_le_bytes()].concat();
        ledger.claim(
            WorldSection::Dims,
            0,
            &dims,
            WorldByteSource::DerivedMapSize {
                replay_sha256: evidence.replay_sha256,
                span: evidence.map_size_span,
                selector: evidence.map_size,
                edge,
                table_va_proof: "MapSizeData::data[0] list [40,50,60,70,80,90,100]",
            },
        )?;

        let tile_xs = edge
            .checked_mul(4)
            .ok_or(WorldOwnerError::DimensionOverflow)?;
        let tile_ys = tile_xs;
        let fog_xs = tile_xs
            .checked_mul(2)
            .ok_or(WorldOwnerError::DimensionOverflow)?
            / 4;
        let fog_ys = fog_xs;
        let reg_xs = tile_xs / 8;
        let reg_ys = reg_xs;
        let derived = [
            edge.checked_mul(edge)
                .ok_or(WorldOwnerError::DimensionOverflow)?,
            fog_xs,
            fog_ys,
            fog_xs
                .checked_mul(fog_ys)
                .ok_or(WorldOwnerError::DimensionOverflow)?,
            tile_xs,
            tile_ys,
            tile_xs
                .checked_mul(tile_ys)
                .ok_or(WorldOwnerError::DimensionOverflow)?,
            reg_xs,
            reg_ys,
            reg_xs
                .checked_mul(reg_ys)
                .ok_or(WorldOwnerError::DimensionOverflow)?,
        ];
        let derived_bytes: Vec<u8> = derived.iter().flat_map(|word| word.to_le_bytes()).collect();
        ledger.claim(
            WorldSection::Scalars,
            0,
            &derived_bytes,
            WorldByteSource::DerivedMapSize {
                replay_sha256: evidence.replay_sha256,
                span: evidence.map_size_span,
                selector: evidence.map_size,
                edge,
                table_va_proof: "World::init 0x006b76f0 dimension arithmetic",
            },
        )?;

        if let Some(rules) = evidence.rules {
            validate_digest(rules.serialized_sha256, "serialized_rules_sha256")?;
            if rules.serialized_span.bytes != SHIPPED_RULES_SERIALIZED_BYTES
                || rules.serialized_span.end().is_none()
            {
                return Err(WorldOwnerError::InvalidReplaySpan {
                    field: "serialized_rules",
                    span: rules.serialized_span,
                });
            }
            if rules.checksum != SHIPPED_RULES_CHANNEL
                || rules.after_constants != RETAIL_AFTER_CONSTANTS
            {
                return Err(WorldOwnerError::RulesCheckpointMismatch {
                    checksum: rules.checksum,
                    after_constants: rules.after_constants,
                });
            }
            if (rules.player_base, rules.player_civic, rules.player_city) != (44, 4, 4) {
                return Err(WorldOwnerError::RulesTerritoryMismatch {
                    player_base: rules.player_base,
                    player_civic: rules.player_civic,
                    player_city: rules.player_city,
                });
            }
            if spans_overlap(evidence.map_size_span, rules.serialized_span) {
                return Err(WorldOwnerError::OverlappingReplaySources {
                    left: rules.serialized_span,
                    right: evidence.map_size_span,
                });
            }
            if spans_overlap(evidence.seed_span, rules.serialized_span) {
                return Err(WorldOwnerError::OverlappingReplaySources {
                    left: rules.serialized_span,
                    right: evidence.seed_span,
                });
            }
            let territory = [
                rules.player_base,
                rules.player_civic,
                rules.player_city,
                rules.player_base,
                rules.player_civic,
                rules.player_city,
            ];
            let territory_bytes: Vec<u8> = territory
                .iter()
                .flat_map(|word| word.to_le_bytes())
                .collect();
            ledger.claim(
                WorldSection::Scalars,
                48,
                &territory_bytes,
                WorldByteSource::ReplayRulesProjection {
                    serialized_span: rules.serialized_span,
                    serialized_sha256: rules.serialized_sha256,
                    checksum: rules.checksum,
                    after_constants: rules.after_constants,
                },
            )?;
        }

        ledger.claim(
            WorldSection::Scalars,
            116,
            &evidence.seed.to_le_bytes(),
            WorldByteSource::ReplayScalar {
                replay_sha256: evidence.replay_sha256,
                span: evidence.seed_span,
                field: "GameInfo::seed -> World::seed",
            },
        )?;
        Ok(ledger)
    }

    pub fn snapshot(&self) -> &WorldOwnerSnapshot {
        &self.snapshot
    }

    pub fn sources(&self) -> &[WorldByteSource] {
        &self.sources
    }

    pub fn coverage(&self) -> OwnershipCoverage {
        let owned_bytes = self.owners.iter().filter(|owner| owner.is_some()).count();
        OwnershipCoverage {
            walked_bytes: self.owners.len(),
            owned_bytes,
            unknown_bytes: self.owners.len() - owned_bytes,
        }
    }

    pub fn owner_at(&self, section: WorldSection, offset: usize) -> Option<&WorldByteSource> {
        let window = self.snapshot.section(section);
        if offset >= window.len() {
            return None;
        }
        self.owners[window.start + offset].map(|source| &self.sources[source])
    }

    /// Advance ownership through one receipt-bound exact-port transition. Only bytes which
    /// actually changed are assigned to the new producer. Unchanged zeroes do not become
    /// "known" merely because a routine ran.
    pub fn advance_exact_port(
        &mut self,
        world_after: &World,
        proof: ExactPortTransitionProof,
    ) -> Result<TransitionReceipt, WorldOwnerError> {
        validate_transition_proof(&proof)?;
        if self.snapshot.checksum.full != proof.input_checksum {
            return Err(WorldOwnerError::TransitionInputMismatch {
                expected: proof.input_checksum,
                actual: self.snapshot.checksum.full,
            });
        }
        let after = WorldOwnerSnapshot::capture(world_after)?;
        if after.checksum.full != proof.output_checksum {
            return Err(WorldOwnerError::TransitionOutputMismatch {
                expected: proof.output_checksum,
                actual: after.checksum.full,
            });
        }
        if self.snapshot.sections.map(|window| window.len())
            != after.sections.map(|window| window.len())
        {
            return Err(WorldOwnerError::WalkShapeChanged);
        }

        let mut changed = Vec::new();
        for section in WorldSection::all() {
            let before = self.snapshot.section_bytes(section);
            let next = after.section_bytes(section);
            let mut cursor = 0usize;
            while cursor < before.len() {
                if before[cursor] == next[cursor] {
                    cursor += 1;
                    continue;
                }
                if !proof.allowed_sections.contains(section) {
                    return Err(WorldOwnerError::ForbiddenSectionMutation {
                        section,
                        offset: cursor,
                    });
                }
                let start = cursor;
                while cursor < before.len() && before[cursor] != next[cursor] {
                    cursor += 1;
                }
                changed.push(ChangedRange {
                    section,
                    offset: start,
                    bytes: cursor - start,
                });
            }
        }

        let source = self.sources.len();
        self.sources.push(WorldByteSource::ExactPortTransition {
            entry_va: proof.entry_va,
            resume_va: proof.resume_va,
            implementation_sha256: proof.implementation_sha256,
            receipt_sha256: proof.receipt_sha256,
            proof_document: proof.proof_document,
            input_checksum: proof.input_checksum,
            output_checksum: proof.output_checksum,
        });
        for range in &changed {
            let window = after.section(range.section);
            for owner in &mut self.owners
                [window.start + range.offset..window.start + range.offset + range.bytes]
            {
                *owner = Some(source);
            }
        }
        self.snapshot = after;
        Ok(TransitionReceipt {
            entry_va: proof.entry_va,
            resume_va: proof.resume_va,
            input_checksum: proof.input_checksum,
            output_checksum: proof.output_checksum,
            changed_bytes: changed.iter().map(|range| range.bytes).sum(),
            changed_ranges: changed,
        })
    }

    /// Compare against a peer-agreed recorded checksum. This cannot localise a byte and
    /// cannot alter ownership.
    pub fn compare_checkpoint(
        &self,
        checkpoint: &RetailWorldCheckpoint,
    ) -> Result<WorldCheckpointComparison, WorldOwnerError> {
        let expected = checkpoint.validate()?;
        Ok(WorldCheckpointComparison {
            expected,
            actual: self.snapshot.checksum.full,
            matches: expected == self.snapshot.checksum.full,
            first_difference: None,
        })
    }

    /// Compare against an actual retail `World::walk_data` byte capture. The capture is
    /// first bound to the peer-agreed checkpoint by the canonical adler primitive.
    pub fn compare_retail_walk(
        &self,
        capture: &RetailWorldWalkCapture,
    ) -> Result<WorldCheckpointComparison, WorldOwnerError> {
        let expected = capture.checkpoint.validate()?;
        validate_digest(capture.executable_sha256, "retail_executable_sha256")?;
        validate_digest(capture.capture_sha256, "retail_capture_sha256")?;
        let capture_checksum = adler32(1, &capture.image);
        if capture_checksum != expected {
            return Err(WorldOwnerError::RetailCaptureChecksumMismatch {
                expected,
                actual: capture_checksum,
            });
        }
        let first_global = first_difference(&self.snapshot.image, &capture.image);
        let first_difference = first_global.map(|global_offset| RetailWalkDifference {
            global_offset,
            location: self.snapshot.locate(global_offset).map_or(
                RetailDifferenceLocation::BeyondModelWalk,
                |(section, offset)| RetailDifferenceLocation::Section { section, offset },
            ),
        });
        Ok(WorldCheckpointComparison {
            expected,
            actual: self.snapshot.checksum.full,
            matches: first_difference.is_none(),
            first_difference,
        })
    }

    fn claim(
        &mut self,
        section: WorldSection,
        offset: usize,
        expected: &[u8],
        source: WorldByteSource,
    ) -> Result<(), WorldOwnerError> {
        let window = self.snapshot.section(section);
        let end = offset
            .checked_add(expected.len())
            .ok_or(WorldOwnerError::ClaimOutOfBounds { section, offset })?;
        if end > window.len() {
            return Err(WorldOwnerError::ClaimOutOfBounds { section, offset });
        }
        let range = window.start + offset..window.start + end;
        if let Some(overlap) = self.owners[range.clone()].iter().position(Option::is_some) {
            return Err(WorldOwnerError::OverlappingClaim {
                section,
                offset: offset + overlap,
            });
        }
        let actual = &self.snapshot.image[range.clone()];
        if actual != expected {
            let mismatch = first_difference(actual, expected).unwrap_or(0);
            return Err(WorldOwnerError::ClaimValueMismatch {
                section,
                offset: offset + mismatch,
                expected: expected.get(mismatch).copied(),
                actual: actual.get(mismatch).copied(),
            });
        }
        let source_index = self.sources.len();
        self.sources.push(source);
        self.owners[range].fill(Some(source_index));
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetailWorldCheckpoint {
    pub replay_sha256: [u8; 32],
    pub turn: i32,
    /// Values recorded by distinct players for the same lockstep group.
    pub peer_checksums: Vec<u32>,
}

impl RetailWorldCheckpoint {
    fn validate(&self) -> Result<u32, WorldOwnerError> {
        validate_digest(self.replay_sha256, "checkpoint_replay_sha256")?;
        if self.turn < 0 {
            return Err(WorldOwnerError::InvalidRetailTurn(self.turn));
        }
        let Some(&first) = self.peer_checksums.first() else {
            return Err(WorldOwnerError::MissingPeerChecksums);
        };
        if self.peer_checksums.len() < 2 {
            return Err(WorldOwnerError::MissingPeerChecksums);
        }
        if self.peer_checksums.iter().any(|&value| value != first) {
            return Err(WorldOwnerError::RetailPeersDisagree);
        }
        Ok(first)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetailWorldWalkCapture {
    pub checkpoint: RetailWorldCheckpoint,
    pub executable_sha256: [u8; 32],
    pub capture_sha256: [u8; 32],
    pub image: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetailDifferenceLocation {
    Section {
        section: WorldSection,
        offset: usize,
    },
    BeyondModelWalk,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetailWalkDifference {
    pub global_offset: usize,
    pub location: RetailDifferenceLocation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorldCheckpointComparison {
    pub expected: u32,
    pub actual: u32,
    pub matches: bool,
    /// Always `None` for checksum-only evidence. Populated only by a bound byte capture.
    pub first_difference: Option<RetailWalkDifference>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorldOwnerError {
    MissingDigest(&'static str),
    InvalidReplaySpan {
        field: &'static str,
        span: ReplaySpan,
    },
    UnknownMapSize(u8),
    DimensionOverflow,
    RulesCheckpointMismatch {
        checksum: u32,
        after_constants: u32,
    },
    RulesTerritoryMismatch {
        player_base: i32,
        player_civic: i32,
        player_city: i32,
    },
    OverlappingReplaySources {
        left: ReplaySpan,
        right: ReplaySpan,
    },
    SectionTraversalDrift {
        section: WorldSection,
    },
    WholeTraversalDrift,
    ClaimOutOfBounds {
        section: WorldSection,
        offset: usize,
    },
    OverlappingClaim {
        section: WorldSection,
        offset: usize,
    },
    ClaimValueMismatch {
        section: WorldSection,
        offset: usize,
        expected: Option<u8>,
        actual: Option<u8>,
    },
    InvalidTransitionProof,
    TransitionInputMismatch {
        expected: u32,
        actual: u32,
    },
    TransitionOutputMismatch {
        expected: u32,
        actual: u32,
    },
    WalkShapeChanged,
    ForbiddenSectionMutation {
        section: WorldSection,
        offset: usize,
    },
    InvalidRetailTurn(i32),
    MissingPeerChecksums,
    RetailPeersDisagree,
    RetailCaptureChecksumMismatch {
        expected: u32,
        actual: u32,
    },
}

fn validate_digest(digest: [u8; 32], field: &'static str) -> Result<(), WorldOwnerError> {
    if digest.iter().all(|&byte| byte == 0) {
        return Err(WorldOwnerError::MissingDigest(field));
    }
    Ok(())
}

fn spans_overlap(left: ReplaySpan, right: ReplaySpan) -> bool {
    match (left.end(), right.end()) {
        (Some(left_end), Some(right_end)) => left.offset < right_end && right.offset < left_end,
        _ => true,
    }
}

fn validate_transition_proof(proof: &ExactPortTransitionProof) -> Result<(), WorldOwnerError> {
    if proof.entry_va == 0
        || proof.resume_va == 0
        || proof.proof_document.is_empty()
        || proof.allowed_sections.is_empty()
        || validate_digest(proof.implementation_sha256, "implementation_sha256").is_err()
        || validate_digest(proof.receipt_sha256, "receipt_sha256").is_err()
    {
        return Err(WorldOwnerError::InvalidTransitionProof);
    }
    Ok(())
}

fn first_difference(left: &[u8], right: &[u8]) -> Option<usize> {
    let shared = left.len().min(right.len());
    (0..shared)
        .find(|&index| left[index] != right[index])
        .or((left.len() != right.len()).then_some(shared))
}
