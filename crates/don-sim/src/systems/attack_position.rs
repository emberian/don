//! Exact positioning transactions inside `Unit::find_attack_pos` (`0x00601280`).
//!
//! This module advances the ordinary held-target seam without pretending that a reflected
//! point or nearest free cell is retail behavior.  The two object arms are materially
//! different:
//!
//! * a unit target calls the complete sixteen-argument `UnitType::find_nearby_spot`
//!   (`0x0061DE70`); that service owns its deterministic rings, terrain, bitmap collision and
//!   ordered collision;
//! * a building target alternates along a rectangle/perimeter state machine, snaps every
//!   probe to a 48-unit cell centre, then calls `invalid_loc`, reads terrain bit `0x4000`,
//!   calls `Objects::find_collision`, calls `Objects::find_ordered_collision`, and only then
//!   draws `game_random(0, 0xffff)` for the candidate's score.
//!
//! The shipped function's perimeter transition block is deliberately a mandatory provider
//! here.  It is not replaced by a circle, rectangle raster, reflected destination, or random
//! free-cell search.  The recovered initial side, linear step and angular step are supplied
//! to that provider, while this module owns the measured acceptance order, strict-low-score
//! tie behavior and adaptive probe budget.  A host that cannot reproduce the perimeter
//! stream gets an error, not a plausible-looking destination.

use super::gathering::{GatherFilterIndex, GatherNearbyPoint, GatherNearbySpotRequest};
use super::held_target::{is_in_range, FindAttackPositionRequest, InRangeResult, IsInRangeInput};
use super::map_terrain::Coord;
use super::movement::{find_angle, ucell_centre, ucell_of, vector_dist};
use super::target::ObjRef;

/// The ordinary actor fields consumed by the recovered positioning arms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AttackPositionActor {
    pub object: ObjRef,
    /// Stable `TypeIndex`; `find_nearby_spot` is a `UnitType` method, not a generic map query.
    pub type_index: i32,
    /// `ObjectTypeData::domain` `+0x218`.
    pub domain: i32,
    /// `ObjectTypeData::big_radius` `+0x244`.
    pub big_radius: i32,
}

/// The common unit-target arm admitted by this tranche.
///
/// `separation` is `find_attack_pos`'s recovered `local_18` after its min/max-range prelude.
/// That prelude includes weapon/melee and special activity branches and is intentionally not
/// reconstructed from an incomplete Arena row.  The caller must provide the measured value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnitTargetPositionFacts {
    pub actor: AttackPositionActor,
    pub target_domain: i32,
    pub target_big_radius: i32,
    pub separation: i32,
    /// Region passed as argument 16 to `find_nearby_spot`; `-1` disables the region gate.
    pub required_region: i32,
    /// The original range input.  On a finder hit the executor replaces only attacker x/y
    /// and the attacker terrain word, then repeats `ObjectData::is_in_range` exactly as retail.
    pub candidate_range: IsInRangeInput,
}

/// Direction number used by the building perimeter block (`1..=8`, clockwise in retail's
/// screen-coordinate convention; odd values are corners and even values are sides).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct BuildingDirection(pub u8);

/// Exact building-perimeter geometry derived before the transition loop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildingPerimeterRequest {
    pub target: ObjRef,
    pub target_x: i32,
    pub target_y: i32,
    pub x_size: i32,
    pub y_size: i32,
    pub source_x: i32,
    pub source_y: i32,
    pub separation: i32,
    pub initial_direction: BuildingDirection,
    /// Linear side-walk step: exactly `0x20`, `0x40`, or `0xC0`.
    pub linear_step: i32,
    /// Corner-arc increment, capped at `0x20000000`.
    pub angular_step: u32,
    /// `0x40000000 / angular_step - 1`, used by the two alternating corner walkers.
    pub corner_last_index: i32,
}

/// Inputs read while deriving the building perimeter's step sizes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildingTargetPositionFacts {
    pub actor: AttackPositionActor,
    pub target_domain: i32,
    pub x_size: i32,
    pub y_size: i32,
    pub separation: i32,
    /// Attacker type field `+0x1FC == 0` takes the fixed `0x20`/`0x20000000` arm.
    pub attacker_range_field_zero: bool,
    /// Attacker virtual `+0x130`. Required and positive when `attacker_range_field_zero`
    /// is false; retail divides by it while selecting the corner increment.
    pub max_range_tiles: Option<i32>,
}

/// Candidate query common to terrain and collision views.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildingCandidateQuery {
    pub point: GatherNearbyPoint,
    pub actor: ObjRef,
}

/// Call site associated with a provider failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttackPositionProviderStage {
    NearbySpot,
    CandidateTerrain,
    BuildingPerimeter,
    InvalidLocation,
    BitmapCollision,
    OrderedCollision,
    GameRandom,
}

/// Mandatory authoritative services used by the recovered arms.
pub trait AttackPositionProvider {
    type Error;

    /// Complete `UnitType::find_nearby_spot`, including deterministic rings, bounds/region,
    /// terrain, bitmap collision and the final ordered-collision query. `Ok(None)` means the
    /// retail finder returned 1 after exhausting its exact stream.
    fn find_nearby_spot(
        &mut self,
        request: GatherNearbySpotRequest,
    ) -> Result<Option<GatherNearbyPoint>, Self::Error>;

    /// Raw probe from the exact building perimeter transition state machine. `sequence_index`
    /// is requested in order from zero and never exceeds 99. The executor performs retail's
    /// UCoord snapping after this call.
    fn building_perimeter_probe(
        &mut self,
        request: BuildingPerimeterRequest,
        sequence_index: u8,
    ) -> Result<GatherNearbyPoint, Self::Error>;

    /// `Unit::invalid_loc` call at `0x00602090`, with its fixed building-search flags
    /// `(1,0,0,0,1)`. `true` rejects the probe before the terrain word is read.
    fn invalid_location(&mut self, query: BuildingCandidateQuery) -> Result<bool, Self::Error>;

    /// Terrain word at the snapped candidate. Bit `0x4000` rejects the building probe.
    /// Unit-target hits also use this method to obtain the terrain word for the repeated
    /// `ObjectData::is_in_range` check.
    fn terrain_word(&mut self, point: GatherNearbyPoint) -> Result<u16, Self::Error>;

    /// `Objects::find_collision(x,y,actor_o,actor_who,0)`. `true` means blocked.
    fn find_bitmap_collision(&mut self, query: BuildingCandidateQuery)
        -> Result<bool, Self::Error>;

    /// `Objects::find_ordered_collision(x,y,actor_o,actor_who)`, called only after the
    /// bitmap is clear. Implementations must preserve WData intrusive-list order.
    fn find_ordered_collision(
        &mut self,
        query: BuildingCandidateQuery,
    ) -> Result<bool, Self::Error>;

    /// Exact `game_random(0, 0xffff)` draw. Values outside the inclusive range fail closed.
    fn game_random_0_ffff(&mut self) -> Result<i32, Self::Error>;
}

/// Source of the returned point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttackPositionSource {
    NearbySpot,
    BuildingPerimeter,
    /// Measured bottom-of-function land/domain fallback, not a host back-off heuristic.
    TargetAnchorFallback,
}

/// Retail boolean result plus the selected output coordinate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttackPositionOutcome {
    Found {
        point: GatherNearbyPoint,
        source: AttackPositionSource,
    },
    NoPosition,
}

/// Auditable building search counts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BuildingSearchTrace {
    pub perimeter_probes: u8,
    pub invalid_location_queries: u8,
    pub terrain_queries: u8,
    pub bitmap_queries: u8,
    pub ordered_queries: u8,
    pub random_draws: u8,
}

/// Result plus the exact call counts of a building search.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildingSearchResult {
    pub outcome: AttackPositionOutcome,
    pub trace: BuildingSearchTrace,
}

/// Fail-closed validation/provider errors.
#[derive(Debug, PartialEq, Eq)]
pub enum AttackPositionError<E> {
    Provider {
        stage: AttackPositionProviderStage,
        source: E,
    },
    ExpectedUnitTarget,
    ExpectedBuildingTarget,
    UnsupportedWrapperParameters {
        param_3: i32,
        param_6: i32,
    },
    ExpectedOutOfRange(InRangeResult),
    ContradictoryInitialRange {
        request: InRangeResult,
        recomputed: InRangeResult,
    },
    ContradictoryRangeCoordinates,
    NegativeRadius(&'static str, i32),
    InvalidIdentity(ObjRef),
    InvalidBuildingSize(i32, i32),
    MissingMaxRange,
    RandomOutOfRange(i32),
}

#[inline]
fn provider_error<E>(stage: AttackPositionProviderStage, source: E) -> AttackPositionError<E> {
    AttackPositionError::Provider { stage, source }
}

#[inline]
fn validate_common<E>(request: FindAttackPositionRequest) -> Result<(), AttackPositionError<E>> {
    if request.wrapper_param_3 != 0 || request.wrapper_param_6 != 0 {
        return Err(AttackPositionError::UnsupportedWrapperParameters {
            param_3: request.wrapper_param_3,
            param_6: request.wrapper_param_6,
        });
    }
    if !request.target.is_some() {
        return Err(AttackPositionError::InvalidIdentity(request.target));
    }
    if matches!(request.range_result, InRangeResult::InRange { .. }) {
        return Err(AttackPositionError::ExpectedOutOfRange(
            request.range_result,
        ));
    }
    Ok(())
}

#[inline]
fn fallback_by_domains(
    request: FindAttackPositionRequest,
    attacker_domain: i32,
    target_domain: i32,
) -> AttackPositionOutcome {
    // 0x00602D7E..0x00602DBD: sea attackers fail; a land attacker also fails against a sea
    // target. Other ordinary domain pairs return true with the target anchor already written.
    if attacker_domain == 1 || (attacker_domain == 0 && target_domain == 1) {
        AttackPositionOutcome::NoPosition
    } else {
        AttackPositionOutcome::Found {
            point: GatherNearbyPoint {
                x: Coord(request.target_x),
                y: Coord(request.target_y),
            },
            source: AttackPositionSource::TargetAnchorFallback,
        }
    }
}

/// Build the exact finder tuple at `0x00602AB9..0x00602B31` for the common unadjusted
/// unit-target arm used by the ordinary six-argument fight wrapper.
pub fn unit_target_nearby_request<E>(
    request: FindAttackPositionRequest,
    facts: UnitTargetPositionFacts,
) -> Result<GatherNearbySpotRequest, AttackPositionError<E>> {
    validate_common(request)?;
    if request.target_is_building {
        return Err(AttackPositionError::ExpectedUnitTarget);
    }
    if !facts.actor.object.is_some() {
        return Err(AttackPositionError::InvalidIdentity(facts.actor.object));
    }
    for (name, value) in [
        ("attacker big_radius", facts.actor.big_radius),
        ("target big_radius", facts.target_big_radius),
        ("separation", facts.separation),
    ] {
        if value < 0 {
            return Err(AttackPositionError::NegativeRadius(name, value));
        }
    }
    let d = facts.candidate_range.distance;
    if d.target_x != request.target_x
        || d.target_y != request.target_y
        || d.attacker_x != request.attacker_x
        || d.attacker_y != request.attacker_y
    {
        return Err(AttackPositionError::ContradictoryRangeCoordinates);
    }
    let recomputed_range = is_in_range(facts.candidate_range);
    if recomputed_range != request.range_result {
        return Err(AttackPositionError::ContradictoryInitialRange {
            request: request.range_result,
            recomputed: recomputed_range,
        });
    }

    let combined = facts
        .target_big_radius
        .wrapping_add(facts.actor.big_radius)
        .wrapping_add(facts.separation);
    let (min_radius, max_radius, radial_step) = if combined > 0x240 {
        (combined.wrapping_sub(0xC0), combined, 0x60)
    } else {
        (combined, 0, 0)
    };
    Ok(GatherNearbySpotRequest {
        centre: GatherNearbyPoint {
            x: Coord(request.target_x),
            y: Coord(request.target_y),
        },
        min_radius,
        max_radius,
        radial_step,
        base_angle: find_angle(
            request.target_x.wrapping_sub(request.attacker_x),
            request.target_y.wrapping_sub(request.attacker_y),
        ) as u32,
        filter: GatherFilterIndex::GATHER,
        worker_type: facts.actor.type_index,
        worker_o: i32::from(facts.actor.object.o),
        worker_owner: i32::from(facts.actor.object.who),
        accept_without_collision: 0,
        expanded: 0,
        overlap_o: -1,
        overlap_owner: 0,
        required_region: facts.required_region,
    })
}

/// Execute the common unit-target positioning arm.
pub fn resolve_unit_target_position<P: AttackPositionProvider>(
    request: FindAttackPositionRequest,
    facts: UnitTargetPositionFacts,
    provider: &mut P,
) -> Result<AttackPositionOutcome, AttackPositionError<P::Error>> {
    let nearby = unit_target_nearby_request(request, facts)?;
    let point = provider
        .find_nearby_spot(nearby)
        .map_err(|e| provider_error(AttackPositionProviderStage::NearbySpot, e))?;
    let Some(point) = point else {
        return Ok(fallback_by_domains(
            request,
            facts.actor.domain,
            facts.target_domain,
        ));
    };

    let terrain = provider
        .terrain_word(point)
        .map_err(|e| provider_error(AttackPositionProviderStage::CandidateTerrain, e))?;
    let mut candidate_range = facts.candidate_range;
    candidate_range.attacker_terrain_word = terrain;
    candidate_range.distance.attacker_x = point.x.0;
    candidate_range.distance.attacker_y = point.y.0;
    if matches!(is_in_range(candidate_range), InRangeResult::InRange { .. }) {
        Ok(AttackPositionOutcome::Found {
            point,
            source: AttackPositionSource::NearbySpot,
        })
    } else {
        Ok(fallback_by_domains(
            request,
            facts.actor.domain,
            facts.target_domain,
        ))
    }
}

/// Exact start direction selected from the source point and rectangular target extents at
/// `0x0060197B..0x00601AE1`.
pub fn building_initial_direction(
    source_x: i32,
    source_y: i32,
    target_x: i32,
    target_y: i32,
    x_size: i32,
    y_size: i32,
) -> BuildingDirection {
    let outside_x = source_x
        .wrapping_sub(target_x)
        .wrapping_abs()
        .wrapping_sub(x_size.wrapping_mul(0x60));
    let outside_y = source_y
        .wrapping_sub(target_y)
        .wrapping_abs()
        .wrapping_sub(y_size.wrapping_mul(0x60));
    let outside_both = outside_x > 0 && outside_y > 0;

    let direction = if outside_y < outside_x {
        if !outside_both {
            if source_x <= target_x {
                8
            } else {
                4
            }
        } else if target_x < source_x {
            if target_y < source_y {
                5
            } else {
                3
            }
        } else if target_y < source_y {
            7
        } else {
            1
        }
    } else if !outside_both {
        if target_y < source_y {
            6
        } else {
            2
        }
    } else if target_y < source_y {
        if source_x <= target_x {
            7
        } else {
            5
        }
    } else if target_x < source_x {
        3
    } else {
        1
    };
    BuildingDirection(direction)
}

/// Derive the measured perimeter request while leaving the 100-probe transition stream to
/// the mandatory provider.
pub fn building_perimeter_request<E>(
    request: FindAttackPositionRequest,
    facts: BuildingTargetPositionFacts,
) -> Result<BuildingPerimeterRequest, AttackPositionError<E>> {
    validate_common(request)?;
    if !request.target_is_building {
        return Err(AttackPositionError::ExpectedBuildingTarget);
    }
    if !facts.actor.object.is_some() {
        return Err(AttackPositionError::InvalidIdentity(facts.actor.object));
    }
    if facts.x_size < 1 || facts.y_size < 1 {
        return Err(AttackPositionError::InvalidBuildingSize(
            facts.x_size,
            facts.y_size,
        ));
    }
    for (name, value) in [
        ("attacker big_radius", facts.actor.big_radius),
        ("separation", facts.separation),
    ] {
        if value < 0 {
            return Err(AttackPositionError::NegativeRadius(name, value));
        }
    }

    let (linear_step, angular_step) = if facts.attacker_range_field_zero {
        (0x20, 0x2000_0000)
    } else {
        let max_range = facts
            .max_range_tiles
            .filter(|v| *v > 0)
            .ok_or(AttackPositionError::MissingMaxRange)?;
        let linear_step =
            if facts.actor.big_radius >= 0x31 && facts.x_size >= 3 && facts.y_size >= 3 {
                0xC0
            } else if facts.actor.big_radius > 0x18 && facts.x_size > 1 && facts.y_size > 1 {
                0x40
            } else {
                0x20
            };
        let range_quarter = 0x4000_0000u32 / max_range as u32;
        let step_divisor = (0xC0 / linear_step) as u32;
        (linear_step, (range_quarter / step_divisor).min(0x2000_0000))
    };
    // Every admitted branch produces a positive divisor; keep the boundary explicit anyway.
    if angular_step == 0 {
        return Err(AttackPositionError::MissingMaxRange);
    }
    let corner_last_index = (0x4000_0000u32 / angular_step) as i32 - 1;
    Ok(BuildingPerimeterRequest {
        target: request.target,
        target_x: request.target_x,
        target_y: request.target_y,
        x_size: facts.x_size,
        y_size: facts.y_size,
        source_x: request.attacker_x,
        source_y: request.attacker_y,
        separation: facts.separation,
        initial_direction: building_initial_direction(
            request.attacker_x,
            request.attacker_y,
            request.target_x,
            request.target_y,
            facts.x_size,
            facts.y_size,
        ),
        linear_step,
        angular_step,
        corner_last_index,
    })
}

#[inline]
fn snap_point(point: GatherNearbyPoint) -> GatherNearbyPoint {
    GatherNearbyPoint {
        x: Coord(ucell_centre(ucell_of(point.x.0))),
        y: Coord(ucell_centre(ucell_of(point.y.0))),
    }
}

/// Execute the recovered building candidate acceptance/scoring loop.
///
/// The provider supplies the exact alternating perimeter stream. This function requests at
/// most 100 entries. After the first accepted candidate, retail shortens the budget to four
/// trailing-counter positions when `separation <= 0x300`, or eleven positions farther out.
pub fn resolve_building_target_position<P: AttackPositionProvider>(
    request: FindAttackPositionRequest,
    facts: BuildingTargetPositionFacts,
    provider: &mut P,
) -> Result<BuildingSearchResult, AttackPositionError<P::Error>> {
    let perimeter = building_perimeter_request(request, facts)?;
    let mut trace = BuildingSearchTrace::default();
    let mut best: Option<(i32, GatherNearbyPoint)> = None;
    let mut limit = 100i32;

    for sequence_index in 0u8..100 {
        let counter = 4 + i32::from(sequence_index);
        trace.perimeter_probes = trace.perimeter_probes.wrapping_add(1);
        let raw = provider
            .building_perimeter_probe(perimeter, sequence_index)
            .map_err(|e| provider_error(AttackPositionProviderStage::BuildingPerimeter, e))?;
        let point = snap_point(raw);
        let query = BuildingCandidateQuery {
            point,
            actor: facts.actor.object,
        };

        trace.invalid_location_queries = trace.invalid_location_queries.wrapping_add(1);
        if provider
            .invalid_location(query)
            .map_err(|e| provider_error(AttackPositionProviderStage::InvalidLocation, e))?
        {
            if counter - 3 >= limit {
                break;
            }
            continue;
        }

        trace.terrain_queries = trace.terrain_queries.wrapping_add(1);
        let terrain = provider
            .terrain_word(point)
            .map_err(|e| provider_error(AttackPositionProviderStage::CandidateTerrain, e))?;
        if terrain & 0x4000 != 0 {
            if counter - 3 >= limit {
                break;
            }
            continue;
        }

        trace.bitmap_queries = trace.bitmap_queries.wrapping_add(1);
        if provider
            .find_bitmap_collision(query)
            .map_err(|e| provider_error(AttackPositionProviderStage::BitmapCollision, e))?
        {
            if counter - 3 >= limit {
                break;
            }
            continue;
        }

        trace.ordered_queries = trace.ordered_queries.wrapping_add(1);
        if provider
            .find_ordered_collision(query)
            .map_err(|e| provider_error(AttackPositionProviderStage::OrderedCollision, e))?
        {
            if counter - 3 >= limit {
                break;
            }
            continue;
        }

        trace.random_draws = trace.random_draws.wrapping_add(1);
        let draw = provider
            .game_random_0_ffff()
            .map_err(|e| provider_error(AttackPositionProviderStage::GameRandom, e))?;
        if !(0..=0xffff).contains(&draw) {
            return Err(AttackPositionError::RandomOutOfRange(draw));
        }
        let score = vector_dist(
            point.x.0.wrapping_sub(request.attacker_x),
            point.y.0.wrapping_sub(request.attacker_y),
        )
        .wrapping_add(draw % 0xC0);
        if best.is_none_or(|(best_score, _)| score < best_score) {
            best = Some((score, point));
            let candidate_limit = if facts.separation <= 0x300 {
                counter
            } else {
                counter.wrapping_add(0x0B)
            };
            limit = limit.min(candidate_limit);
        }

        if counter - 3 >= limit {
            break;
        }
    }

    let outcome = if let Some((_, point)) = best {
        AttackPositionOutcome::Found {
            point,
            source: AttackPositionSource::BuildingPerimeter,
        }
    } else {
        fallback_by_domains(request, facts.actor.domain, facts.target_domain)
    };
    Ok(BuildingSearchResult { outcome, trace })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systems::held_target::{
        AttackDistanceInput, AttackDistanceMode, ObjectFootprint, ReachProfile, TargetLocationGate,
    };

    #[derive(Default)]
    struct MockProvider {
        calls: Vec<&'static str>,
        nearby_result: Option<GatherNearbyPoint>,
        invalid: bool,
        terrain: u16,
        bitmap: bool,
        ordered: bool,
        draw: i32,
        first_raw_x: i32,
        observed_nearby: Option<GatherNearbySpotRequest>,
        observed_perimeter: Option<BuildingPerimeterRequest>,
    }

    impl AttackPositionProvider for MockProvider {
        type Error = &'static str;

        fn find_nearby_spot(
            &mut self,
            request: GatherNearbySpotRequest,
        ) -> Result<Option<GatherNearbyPoint>, Self::Error> {
            self.calls.push("nearby");
            self.observed_nearby = Some(request);
            Ok(self.nearby_result)
        }

        fn building_perimeter_probe(
            &mut self,
            request: BuildingPerimeterRequest,
            sequence_index: u8,
        ) -> Result<GatherNearbyPoint, Self::Error> {
            self.calls.push("perimeter");
            self.observed_perimeter = Some(request);
            Ok(GatherNearbyPoint {
                x: Coord(self.first_raw_x + i32::from(sequence_index) * 48),
                y: Coord(24),
            })
        }

        fn invalid_location(
            &mut self,
            _query: BuildingCandidateQuery,
        ) -> Result<bool, Self::Error> {
            self.calls.push("invalid");
            Ok(self.invalid)
        }

        fn terrain_word(&mut self, _point: GatherNearbyPoint) -> Result<u16, Self::Error> {
            self.calls.push("terrain");
            Ok(self.terrain)
        }

        fn find_bitmap_collision(
            &mut self,
            _query: BuildingCandidateQuery,
        ) -> Result<bool, Self::Error> {
            self.calls.push("bitmap");
            Ok(self.bitmap)
        }

        fn find_ordered_collision(
            &mut self,
            _query: BuildingCandidateQuery,
        ) -> Result<bool, Self::Error> {
            self.calls.push("ordered");
            Ok(self.ordered)
        }

        fn game_random_0_ffff(&mut self) -> Result<i32, Self::Error> {
            self.calls.push("random");
            Ok(self.draw)
        }
    }

    fn request(building: bool) -> FindAttackPositionRequest {
        let range_result = is_in_range(candidate_range());
        FindAttackPositionRequest {
            target: ObjRef::new(8, 2),
            attacker_x: 24,
            attacker_y: 24,
            target_x: 984,
            target_y: 984,
            target_is_building: building,
            range_result,
            wrapper_param_3: 0,
            wrapper_param_6: 0,
        }
    }

    fn actor() -> AttackPositionActor {
        AttackPositionActor {
            object: ObjRef::new(3, 1),
            type_index: 69,
            domain: 0,
            big_radius: 48,
        }
    }

    fn candidate_range() -> IsInRangeInput {
        IsInRangeInput {
            target_active: true,
            target_location: TargetLocationGate::NotRequired,
            attacker_terrain_word: 0,
            distance: AttackDistanceInput {
                attacker_x: 24,
                attacker_y: 24,
                target_x: 984,
                target_y: 984,
                attacker: ObjectFootprint::Unit { block_radius: 48 },
                target: ObjectFootprint::Unit { block_radius: 48 },
                mode: AttackDistanceMode::Footprints,
            },
            reach: ReachProfile::MinMax {
                min_range_tiles: 0,
                max_range_tiles: 3,
                attacker_big_radius: Some(48),
                target_big_radius: Some(48),
                param_6_nonzero: false,
            },
        }
    }

    #[test]
    fn unit_arm_constructs_the_exact_common_finder_tuple() {
        let request = request(false);
        let facts = UnitTargetPositionFacts {
            actor: actor(),
            target_domain: 0,
            target_big_radius: 48,
            separation: 600,
            required_region: -1,
            candidate_range: candidate_range(),
        };
        let nearby = unit_target_nearby_request::<&'static str>(request, facts).unwrap();
        // 48 + 48 + 600 = 696 > 576: min is reduced by 192 and step is 96.
        assert_eq!(
            (nearby.min_radius, nearby.max_radius, nearby.radial_step),
            (504, 696, 96)
        );
        assert_eq!(nearby.centre.x.0, request.target_x);
        assert_eq!(nearby.centre.y.0, request.target_y);
        assert_eq!(nearby.filter, GatherFilterIndex::GATHER);
        assert_eq!((nearby.worker_o, nearby.worker_owner), (3, 1));
        assert_eq!((nearby.accept_without_collision, nearby.expanded), (0, 0));
        assert_eq!((nearby.overlap_o, nearby.overlap_owner), (-1, 0));
        assert_eq!(nearby.base_angle, find_angle(960, 960) as u32);
    }

    #[test]
    fn unit_arm_rejects_a_range_result_from_different_facts() {
        let mut request = request(false);
        request.range_result = InRangeResult::OutOfRange {
            distance: Some(1),
            reason: super::super::held_target::OutOfRangeReason::AboveMaximum,
        };
        let facts = UnitTargetPositionFacts {
            actor: actor(),
            target_domain: 0,
            target_big_radius: 48,
            separation: 600,
            required_region: -1,
            candidate_range: candidate_range(),
        };

        assert!(matches!(
            unit_target_nearby_request::<&'static str>(request, facts),
            Err(AttackPositionError::ContradictoryInitialRange { .. })
        ));
    }

    #[test]
    fn unit_hit_rechecks_range_with_candidate_terrain() {
        let request = request(false);
        let facts = UnitTargetPositionFacts {
            actor: actor(),
            target_domain: 0,
            target_big_radius: 48,
            separation: 600,
            required_region: -1,
            candidate_range: candidate_range(),
        };
        let point = GatherNearbyPoint {
            x: Coord(600),
            y: Coord(600),
        };
        let mut provider = MockProvider {
            nearby_result: Some(point),
            ..Default::default()
        };
        assert_eq!(
            resolve_unit_target_position(request, facts, &mut provider).unwrap(),
            AttackPositionOutcome::Found {
                point,
                source: AttackPositionSource::NearbySpot,
            }
        );
        assert_eq!(provider.calls, ["nearby", "terrain"]);
    }

    #[test]
    fn exhausted_unit_finder_uses_measured_land_target_anchor_fallback() {
        let request = request(false);
        let facts = UnitTargetPositionFacts {
            actor: actor(),
            target_domain: 0,
            target_big_radius: 48,
            separation: 600,
            required_region: -1,
            candidate_range: candidate_range(),
        };
        let mut provider = MockProvider::default();
        assert_eq!(
            resolve_unit_target_position(request, facts, &mut provider).unwrap(),
            AttackPositionOutcome::Found {
                point: GatherNearbyPoint {
                    x: Coord(984),
                    y: Coord(984),
                },
                source: AttackPositionSource::TargetAnchorFallback,
            }
        );
    }

    #[test]
    fn building_direction_matches_all_eight_rectangle_sectors() {
        let t = (1_000, 1_000, 2, 2);
        let samples = [
            ((700, 700), 1),
            ((1_000, 700), 2),
            ((1_300, 700), 3),
            ((1_300, 1_000), 4),
            ((1_300, 1_300), 5),
            ((1_000, 1_300), 6),
            ((700, 1_300), 7),
            ((700, 1_000), 8),
        ];
        for ((x, y), expected) in samples {
            assert_eq!(
                building_initial_direction(x, y, t.0, t.1, t.2, t.3),
                BuildingDirection(expected),
                "source ({x},{y})"
            );
        }
    }

    fn building_facts(separation: i32) -> BuildingTargetPositionFacts {
        BuildingTargetPositionFacts {
            actor: actor(),
            target_domain: 0,
            x_size: 3,
            y_size: 3,
            separation,
            attacker_range_field_zero: false,
            max_range_tiles: Some(8),
        }
    }

    #[test]
    fn building_step_and_corner_increment_follow_retail_thresholds() {
        let r =
            building_perimeter_request::<&'static str>(request(true), building_facts(700)).unwrap();
        assert_eq!(r.linear_step, 0x40, "big_radius 48 is below the 49 cutoff");
        assert_eq!(r.angular_step, 0x02AA_AAAA);
        assert_eq!(r.corner_last_index, 23);

        let mut facts = building_facts(700);
        facts.actor.big_radius = 49;
        let r = building_perimeter_request::<&'static str>(request(true), facts).unwrap();
        assert_eq!(r.linear_step, 0xC0);
        assert_eq!(r.angular_step, 0x0800_0000);
    }

    #[test]
    fn building_gates_are_ordered_and_rng_is_after_both_collision_views() {
        let mut provider = MockProvider {
            first_raw_x: 49, // every point is snapped by the executor
            ordered: true,
            ..Default::default()
        };
        let result =
            resolve_building_target_position(request(true), building_facts(700), &mut provider)
                .unwrap();
        assert_eq!(result.trace.perimeter_probes, 100);
        assert_eq!(result.trace.random_draws, 0);
        assert_eq!(
            &provider.calls[..5],
            ["perimeter", "invalid", "terrain", "bitmap", "ordered"]
        );
        assert!(!provider.calls.contains(&"random"));
    }

    #[test]
    fn near_candidate_shortens_to_four_probes_and_strict_ties_keep_first() {
        let mut provider = MockProvider {
            first_raw_x: 49,
            draw: 0,
            ..Default::default()
        };
        let result =
            resolve_building_target_position(request(true), building_facts(0x300), &mut provider)
                .unwrap();
        assert_eq!(result.trace.perimeter_probes, 4);
        assert_eq!(result.trace.random_draws, 4);
        assert_eq!(
            result.outcome,
            AttackPositionOutcome::Found {
                point: GatherNearbyPoint {
                    x: Coord(72),
                    y: Coord(24),
                },
                source: AttackPositionSource::BuildingPerimeter,
            }
        );
    }

    #[test]
    fn far_candidate_keeps_the_eleven_counter_extension() {
        let mut provider = MockProvider {
            first_raw_x: 49,
            ..Default::default()
        };
        let result =
            resolve_building_target_position(request(true), building_facts(0x301), &mut provider)
                .unwrap();
        assert_eq!(result.trace.perimeter_probes, 15);
        assert_eq!(result.trace.random_draws, 15);
    }

    #[test]
    fn invalid_rng_range_fails_closed_after_world_gates() {
        let mut provider = MockProvider {
            first_raw_x: 49,
            draw: 0x1_0000,
            ..Default::default()
        };
        assert_eq!(
            resolve_building_target_position(request(true), building_facts(0x300), &mut provider),
            Err(AttackPositionError::RandomOutOfRange(0x1_0000))
        );
        assert_eq!(
            provider.calls,
            [
                "perimeter",
                "invalid",
                "terrain",
                "bitmap",
                "ordered",
                "random"
            ]
        );
    }
}
