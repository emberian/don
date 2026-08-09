//! `Build::check_capture` `0x006276A0..0x00627FD5`.
//!
//! The retail routine is a synchronous capture transaction reached through
//! [`super::damage_world`]'s capture-attempt seam.  It first admits the victim and
//! attacker, walks the retail circle-table neighborhood, attributes capture strength,
//! and then either delegates a whole-city transfer to `Cities::capture_city` or performs
//! the generic build `swap_team/activate/close/mask_me` sequence.
//!
//! This module owns the recovered control flow and mutation order.  Large nested retail
//! callees remain explicit, identity-bound world boundaries.  In particular,
//! `Cities::capture_city` still owns its building reassignment, plunder/economy,
//! diplomacy/score, and object-transfer effects.

use super::damage_world::{CaptureCheckRequest, ObjectKey};
use super::vector_dist;

pub const NUM_CAPTURE_PLAYERS: usize = 8;
pub const LAND_DOMAIN: i32 = 0;
pub const SEA_DOMAIN: i32 = 1;
pub const AIR_DOMAIN: i32 = 2;
pub const CAPTURE_FILTER_INDEX: i32 = 8;
pub const CAPTURE_COOLDOWN_FRAMES: i32 = 75;
pub const CAPTURE_STALE_FRAMES: i32 = 900;
pub const CAPTURE_RADIUS_SCALE: i32 = 0xC0;
pub const WORLD_UNITS_PER_TILE: i32 = 0x300;
pub const CITY_CAPTURE_HEALTH_LEVEL: i32 = 6;
pub const OTHER_CAPTURE_HEALTH_LEVEL: i32 = 5;
pub const DEFENDING_BUILD_CAPTURE_BONUS: i32 = 6;
pub const DEFENDING_FORT_CAPTURE_BONUS: i32 = 6;
pub const UNIT_CAPTURE_VETO_MASK: u32 = 0x1;
pub const OBJECT_CAPTURE_VETO_MASK: u32 = 0x0800_0000;

pub const OBJECT_ALIVE_FLAG: u8 = 0x1;
pub const OBJECT_COMPLETE_FLAG: u8 = 0x4;
pub const CITY_CENTER_FLAG: u8 = 0x20;
pub const CONTAINED_BUILD_CAPTURE_MASK: u16 = 0x4000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptureRules {
    /// `Constants +0x18`.
    pub unit_respond_range: i32,
    /// `Constants +0x134`.
    pub city_capture_radius: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptureLeaderSnapshot {
    pub who_field: i32,
    pub leader_flags: u32,
    /// Raw `LeaderData::diplos`, used by the admission gate's mutual-`2` test.
    pub diplos: [i32; NUM_CAPTURE_PLAYERS],
    /// Results of `LeaderData::is_ally` in the direction `self -> index`.
    pub allies: [bool; NUM_CAPTURE_PLAYERS],
    /// Results of `LeaderData::is_enemy` in the direction `self -> index`.
    pub enemies: [bool; NUM_CAPTURE_PLAYERS],
}

impl CaptureLeaderSnapshot {
    #[inline]
    pub const fn is_active(self) -> bool {
        self.leader_flags & 0x2 != 0
    }

    #[inline]
    pub const fn is_capture_candidate(self) -> bool {
        self.leader_flags & 0x3 == 0x3
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptureBuildSnapshot {
    pub key: ObjectKey,
    pub flags: u8,
    /// `ObjectData::inside_down` (`Build +0x28`).  Nonnegative means contained.
    pub inside_down: i16,
    pub city: i16,
    pub x: i32,
    pub y: i32,
    /// Region of the victim's center WData cell.
    pub region: i16,
    /// `BuildTypeData::is_city()` from `BuildData::check_capture_eligible`.
    pub type_is_city: bool,
    /// The virtual `WallData::is_active()` result.  The base implementation is `flags&4`.
    pub is_active: bool,
    /// Virtual `ObjectData::health_level()`.
    pub health_level: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptureCitySnapshot {
    pub key: CityKey,
    pub capture_stamp: i32,
    pub capture_strength: i32,
    pub founder: i8,
    pub race: i8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptureObjectSnapshot {
    pub key: ObjectKey,
    pub flags: u8,
    pub x: i32,
    pub y: i32,
    /// `ObjectTypeData::domain` (`+0x218`).
    pub domain: i32,
    /// Object virtual at `+0x18`.
    pub is_unit: bool,
    /// `UnitData::unit_masks` (`+0x68`); read only when `is_unit` is true.
    pub unit_masks: u32,
    /// `ObjectTypeData::obj_masks` (`+0x1E4`).
    pub object_masks: u32,
    /// Object virtual `attack()` (`+0x120`).
    pub attack: i32,
    /// Object virtual `get_capture_value()` (`+0xEC`).
    pub capture_value: i32,
    /// Object virtual at `+0x20`.
    pub is_build: bool,
    /// `BuildTypeData::is_fort()`; meaningful only for builds.
    pub is_fort: bool,
    /// `ObjectData::num_inside(1)`; meaningful only for defending builds.
    pub garrison_inside_domain_1: i32,
    /// `ObjectData::visible` (`+0x40`).
    pub visible_mask: u8,
    /// Exact result of `Search::valid_filter(key.o,-1,-1,8)` with `ecx=key.who`.
    pub passes_capture_filter: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptureTileSnapshot {
    /// `WorldData::valid` result.
    pub valid: bool,
    /// `WData::region`; a different region suppresses the whole `get_down` chain.
    pub region: i16,
    /// Exact `WData::down/down_who`, then `ObjectData::down/down_who`, traversal order.
    pub down_chain: Vec<CaptureObjectSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptureNeighborhood {
    /// Ring index supplied to the retail `circle_radius/x/y` tables.
    pub radius_tiles: i32,
    pub tiles: Vec<CaptureTileSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildCheckCaptureInput {
    pub request: CaptureCheckRequest,
    pub victim: CaptureBuildSnapshot,
    pub attacker: CaptureObjectSnapshot,
    pub city: CaptureCitySnapshot,
    pub leaders: [CaptureLeaderSnapshot; NUM_CAPTURE_PLAYERS],
    pub frame: i32,
    pub rules: CaptureRules,
    pub neighborhood: CaptureNeighborhood,
    /// `Console +0x298`; `None` means no local player.
    pub console_who: Option<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CityKey {
    pub who: u8,
    pub city: i16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MarkContainedBuildRequest {
    pub build: ObjectKey,
    pub or_mask: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RevealCaptureDefenderRequest {
    pub build: ObjectKey,
    pub viewer: u8,
    pub visible_before: u8,
    pub visible_after: u8,
    /// Retail immediately calls `Wall::update_local_seen()` after setting the bit.
    pub update_local_seen: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptureCensusReceipt {
    pub radius_world: i32,
    pub radius_tiles: i32,
    pub per_owner: [i32; NUM_CAPTURE_PLAYERS],
    pub attacker_side: i32,
    pub defender_side: i32,
    pub reveals: Vec<RevealCaptureDefenderRequest>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureZeroReason {
    IneligibleType,
    IneligibleActiveState,
    IneligibleHealthLevel,
    ContainedBuild,
    FriendlyActiveOwner,
    InactiveAttackerLeader,
    AttackerNotLand,
    AttackerNotUnit,
    AttackerUnitMaskVeto,
    AttackerCannotAttack,
    AttackerObjectMaskVeto,
    CaptureCooldown,
    DefenderHeld,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptureCityBoundaryRequest {
    /// First argument to `Cities::capture_city`.
    pub new_owner: u8,
    /// Second argument.
    pub old_city: CityKey,
    /// Third argument to `Cities::capture_city`.
    pub old_owner: u8,
    /// Strongest active attacker/allied contributor before founder/race re-homing.
    pub strength_winner: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptureObjectBoundaryRequest {
    pub old_build: ObjectKey,
    pub new_owner: u8,
    pub notify_console: bool,
    pub play_lost_sound: bool,
    pub play_captured_sound: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildCheckCaptureOutcomePlan {
    ReturnZero(CaptureZeroReason),
    CaptureCity(CaptureCityBoundaryRequest),
    CaptureObject(CaptureObjectBoundaryRequest),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildCheckCapturePlan {
    pub request: CaptureCheckRequest,
    pub mark_contained: Option<MarkContainedBuildRequest>,
    pub census: Option<CaptureCensusReceipt>,
    pub outcome: BuildCheckCaptureOutcomePlan,
    /// The value cached from the original attacker and written to a newly captured city.
    pub attacker_capture_value: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildCheckCapturePlanError {
    VictimIdentityMismatch,
    AttackerIdentityMismatch,
    InvalidOwner(u8),
    CityIdentityMismatch,
    NeighborhoodRadiusMismatch { expected: i32, actual: i32 },
}

fn player_index(who: u8) -> Result<usize, BuildCheckCapturePlanError> {
    let who_usize = usize::from(who);
    if who_usize < NUM_CAPTURE_PLAYERS {
        Ok(who_usize)
    } else {
        Err(BuildCheckCapturePlanError::InvalidOwner(who))
    }
}

fn capture_radius(victim_flags: u8, rules: CaptureRules) -> (i32, i32) {
    let rule = if victim_flags & CITY_CENTER_FLAG != 0 {
        rules.city_capture_radius
    } else {
        rules.unit_respond_range
    };
    let world = rule.wrapping_mul(CAPTURE_RADIUS_SCALE);
    let tiles = world.wrapping_add(WORLD_UNITS_PER_TILE - 1) / WORLD_UNITS_PER_TILE;
    (world, tiles)
}

fn eligible_health_level(victim: CaptureBuildSnapshot) -> i32 {
    if victim.flags & CITY_CENTER_FLAG != 0 {
        CITY_CAPTURE_HEALTH_LEVEL
    } else {
        OTHER_CAPTURE_HEALTH_LEVEL
    }
}

fn projected_visible_mask(
    projected: &mut Vec<(ObjectKey, u8)>,
    candidate: CaptureObjectSnapshot,
) -> u8 {
    if let Some((_, mask)) = projected.iter().find(|(key, _)| *key == candidate.key) {
        *mask
    } else {
        projected.push((candidate.key, candidate.visible_mask));
        candidate.visible_mask
    }
}

fn set_projected_visible_mask(projected: &mut [(ObjectKey, u8)], key: ObjectKey, value: u8) {
    if let Some((_, mask)) = projected
        .iter_mut()
        .find(|(candidate, _)| *candidate == key)
    {
        *mask = value;
    }
}

fn valid_rehome_owner(
    leaders: &[CaptureLeaderSnapshot; NUM_CAPTURE_PLAYERS],
    winner: u8,
    old_owner: u8,
    candidate: i8,
) -> Option<u8> {
    let candidate = u8::try_from(candidate).ok()?;
    let candidate_index = player_index(candidate).ok()?;
    let winner_index = player_index(winner).ok()?;
    if candidate == old_owner || candidate == winner {
        return None;
    }
    (leaders[winner_index].allies[candidate_index] && leaders[candidate_index].is_active())
        .then_some(candidate)
}

/// Recover the complete deterministic decision and all pre-outcome object mutations.
///
/// `CaptureNeighborhood` is the typed boundary for retail's static circle tables and world
/// linked lists.  Its ring is checked here, and every object predicate, distance test,
/// strength addition, diplomacy attribution, and winner/re-home rule is executable here.
pub fn plan_build_check_capture(
    input: &BuildCheckCaptureInput,
) -> Result<BuildCheckCapturePlan, BuildCheckCapturePlanError> {
    if input.victim.key != input.request.victim {
        return Err(BuildCheckCapturePlanError::VictimIdentityMismatch);
    }
    if input.attacker.key != input.request.attacker {
        return Err(BuildCheckCapturePlanError::AttackerIdentityMismatch);
    }
    let zero = |reason| BuildCheckCapturePlan {
        request: input.request,
        mark_contained: None,
        census: None,
        outcome: BuildCheckCaptureOutcomePlan::ReturnZero(reason),
        attacker_capture_value: input.attacker.capture_value,
    };

    // Inlined body of `BuildData::check_capture_eligible` `0x0062D1D0`.
    if !input.victim.type_is_city {
        return Ok(zero(CaptureZeroReason::IneligibleType));
    }
    if !input.victim.is_active {
        return Ok(zero(CaptureZeroReason::IneligibleActiveState));
    }
    if input.victim.health_level < eligible_health_level(input.victim) {
        return Ok(zero(CaptureZeroReason::IneligibleHealthLevel));
    }

    if input.victim.inside_down >= 0 {
        return Ok(BuildCheckCapturePlan {
            request: input.request,
            mark_contained: Some(MarkContainedBuildRequest {
                build: input.request.victim,
                or_mask: CONTAINED_BUILD_CAPTURE_MASK,
            }),
            census: None,
            outcome: BuildCheckCaptureOutcomePlan::ReturnZero(CaptureZeroReason::ContainedBuild),
            attacker_capture_value: input.attacker.capture_value,
        });
    }

    let victim_owner = player_index(input.request.victim.who)?;
    let attacker_owner = player_index(input.request.attacker.who)?;
    let victim_leader = input.leaders[victim_owner];
    let attacker_leader = input.leaders[attacker_owner];
    let victim_who_field = usize::try_from(victim_leader.who_field).ok();
    let same_or_mutual_team = input.request.attacker.who as i32 == victim_leader.who_field
        || (victim_leader.diplos[attacker_owner] == 2
            && victim_who_field
                .filter(|who| *who < NUM_CAPTURE_PLAYERS)
                .is_some_and(|who| attacker_leader.diplos[who] == 2));
    if same_or_mutual_team && victim_leader.is_active() {
        return Ok(zero(CaptureZeroReason::FriendlyActiveOwner));
    }
    if !attacker_leader.is_active() {
        return Ok(zero(CaptureZeroReason::InactiveAttackerLeader));
    }

    if input.attacker.domain != LAND_DOMAIN {
        return Ok(zero(CaptureZeroReason::AttackerNotLand));
    }
    if !input.attacker.is_unit {
        return Ok(zero(CaptureZeroReason::AttackerNotUnit));
    }
    if input.attacker.unit_masks & UNIT_CAPTURE_VETO_MASK != 0 {
        return Ok(zero(CaptureZeroReason::AttackerUnitMaskVeto));
    }
    if input.attacker.attack == 0 {
        return Ok(zero(CaptureZeroReason::AttackerCannotAttack));
    }
    if input.attacker.object_masks & OBJECT_CAPTURE_VETO_MASK != 0 {
        return Ok(zero(CaptureZeroReason::AttackerObjectMaskVeto));
    }

    if input.city.key
        != (CityKey {
            who: input.request.victim.who,
            city: input.victim.city,
        })
    {
        return Err(BuildCheckCapturePlanError::CityIdentityMismatch);
    }
    if input.frame.wrapping_sub(input.city.capture_stamp) < CAPTURE_COOLDOWN_FRAMES {
        return Ok(zero(CaptureZeroReason::CaptureCooldown));
    }

    let (radius_world, radius_tiles) = capture_radius(input.victim.flags, input.rules);
    if input.neighborhood.radius_tiles != radius_tiles {
        return Err(BuildCheckCapturePlanError::NeighborhoodRadiusMismatch {
            expected: radius_tiles,
            actual: input.neighborhood.radius_tiles,
        });
    }

    let defender_seed = if input.frame.wrapping_sub(input.city.capture_stamp) < CAPTURE_STALE_FRAMES
    {
        input.city.capture_strength
    } else {
        2
    };
    let mut per_owner = [0i32; NUM_CAPTURE_PLAYERS];
    per_owner[attacker_owner] = input.attacker.capture_value;
    per_owner[victim_owner] = defender_seed;
    let mut attacker_side = input.attacker.capture_value;
    let mut defender_side = defender_seed;
    let mut reveals = Vec::new();
    let mut projected_visibility = Vec::new();

    for tile in &input.neighborhood.tiles {
        if !tile.valid || tile.region != input.victim.region {
            continue;
        }
        for candidate in &tile.down_chain {
            let owner = player_index(candidate.key.who)?;
            if candidate.key == input.request.victim || candidate.key == input.request.attacker {
                continue;
            }
            if candidate.flags & OBJECT_ALIVE_FLAG == 0 {
                continue;
            }
            if matches!(candidate.domain, SEA_DOMAIN | AIR_DOMAIN) {
                continue;
            }
            if candidate.is_unit && candidate.unit_masks & UNIT_CAPTURE_VETO_MASK != 0 {
                continue;
            }
            if !candidate.passes_capture_filter {
                continue;
            }
            let dx = candidate.x.wrapping_sub(input.victim.x);
            let dy = candidate.y.wrapping_sub(input.victim.y);
            if vector_dist(dx, dy) > radius_world {
                continue;
            }

            let mut strength = candidate.capture_value;
            if strength != 0 && candidate.is_build && candidate.key.who == input.request.victim.who
            {
                strength = strength.wrapping_add(if candidate.is_fort {
                    DEFENDING_BUILD_CAPTURE_BONUS + DEFENDING_FORT_CAPTURE_BONUS
                } else {
                    DEFENDING_BUILD_CAPTURE_BONUS
                });
                strength = strength.wrapping_add(candidate.garrison_inside_domain_1);

                let viewer_bit = 1u8 << input.request.attacker.who;
                let visible_before = projected_visible_mask(&mut projected_visibility, *candidate);
                if visible_before & viewer_bit == 0 {
                    let visible_after = visible_before | viewer_bit;
                    reveals.push(RevealCaptureDefenderRequest {
                        build: candidate.key,
                        viewer: input.request.attacker.who,
                        visible_before,
                        visible_after,
                        update_local_seen: true,
                    });
                    set_projected_visible_mask(
                        &mut projected_visibility,
                        candidate.key,
                        visible_after,
                    );
                }
            }

            per_owner[owner] = per_owner[owner].wrapping_add(strength);
            if candidate.key.who == input.request.attacker.who
                || (attacker_leader.allies[owner] && victim_leader.enemies[owner])
            {
                attacker_side = attacker_side.wrapping_add(strength);
            } else if candidate.key.who == input.request.victim.who
                || (victim_leader.allies[owner] && attacker_leader.enemies[owner])
            {
                defender_side = defender_side.wrapping_add(strength);
            }
        }
    }

    let census = CaptureCensusReceipt {
        radius_world,
        radius_tiles,
        per_owner,
        attacker_side,
        defender_side,
        reveals,
    };
    if defender_side >= attacker_side {
        return Ok(BuildCheckCapturePlan {
            request: input.request,
            mark_contained: None,
            census: Some(census),
            outcome: BuildCheckCaptureOutcomePlan::ReturnZero(CaptureZeroReason::DefenderHeld),
            attacker_capture_value: input.attacker.capture_value,
        });
    }

    let outcome = if input.victim.flags & CITY_CENTER_FLAG != 0 {
        let mut winner = input.request.attacker.who;
        let mut best = 0;
        for who in 0..NUM_CAPTURE_PLAYERS {
            let leader = input.leaders[who];
            if !leader.is_capture_candidate()
                || (who != attacker_owner && !leader.allies[attacker_owner])
            {
                continue;
            }
            if per_owner[who] > best {
                best = per_owner[who];
                winner = who as u8;
            }
        }

        // Retail prefers an active allied founder, then an active allied race.
        let new_owner = valid_rehome_owner(
            &input.leaders,
            winner,
            input.request.victim.who,
            input.city.founder,
        )
        .or_else(|| {
            valid_rehome_owner(
                &input.leaders,
                winner,
                input.request.victim.who,
                input.city.race,
            )
        })
        .unwrap_or(winner);
        BuildCheckCaptureOutcomePlan::CaptureCity(CaptureCityBoundaryRequest {
            new_owner,
            old_city: input.city.key,
            old_owner: input.request.victim.who,
            strength_winner: winner,
        })
    } else {
        let play_lost_sound = input.console_who == Some(input.request.victim.who);
        let play_captured_sound = input.console_who == Some(input.request.attacker.who);
        BuildCheckCaptureOutcomePlan::CaptureObject(CaptureObjectBoundaryRequest {
            old_build: input.request.victim,
            new_owner: input.request.attacker.who,
            notify_console: play_lost_sound || play_captured_sound,
            play_lost_sound,
            play_captured_sound,
        })
    };

    Ok(BuildCheckCapturePlan {
        request: input.request,
        mark_contained: None,
        census: Some(census),
        outcome,
        attacker_capture_value: input.attacker.capture_value,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CityCaptureBoundaryReceipt {
    pub request: CaptureCityBoundaryRequest,
    pub new_city: CityKey,
    /// `Cities::capture_city` reassigns the city's member buildings.
    pub buildings_reassigned: bool,
    /// Plunder and resource/economy effects completed.
    pub economy_and_plunder_applied: bool,
    /// Diplomacy and score effects completed.
    pub diplomacy_and_score_applied: bool,
    /// The center and dependent object ownership changes completed.
    pub object_transfer_applied: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CityCaptureStrengthRequest {
    pub city: CityKey,
    pub value: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildSwapBoundaryReceipt {
    pub request: CaptureObjectBoundaryRequest,
    /// `None` is retail's negative `Build::swap_team` return.
    pub new_build: Option<ObjectKey>,
    pub ownership_applied: bool,
    pub object_copy_applied: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureSound {
    LostObject0x86,
    CapturedObject0x7c,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptureAnnouncementRequest {
    pub old_build: ObjectKey,
    pub new_build: ObjectKey,
    /// The boundary owns `ObjectData::say_name`, localized concatenation, and the red
    /// `MessageWin::add_message` call at the new object's decoded coordinates.
    pub red_localized_message: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildLifecycleRequest {
    pub build: ObjectKey,
    pub first: i32,
    pub second: i32,
    pub third: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildMaskRequest {
    pub build: ObjectKey,
    pub first: i32,
    pub second: i32,
}

pub trait BuildCheckCaptureWorld {
    fn mark_contained_build(&mut self, request: MarkContainedBuildRequest);
    fn reveal_capture_defender(&mut self, request: RevealCaptureDefenderRequest);

    /// Atomic residual boundary: `Cities::capture_city` `0x00733380`.
    fn capture_city(
        &mut self,
        request: CaptureCityBoundaryRequest,
    ) -> Option<CityCaptureBoundaryReceipt>;
    fn set_city_capture_strength(&mut self, request: CityCaptureStrengthRequest);

    /// Atomic residual boundary: virtual `Build::swap_team(new_owner)`.
    fn swap_build_team(
        &mut self,
        request: CaptureObjectBoundaryRequest,
    ) -> Option<BuildSwapBoundaryReceipt>;
    fn kill_build_after_failed_swap(&mut self, build: ObjectKey);

    fn play_capture_sound(&mut self, sound: CaptureSound);
    fn announce_capture(&mut self, request: CaptureAnnouncementRequest);
    fn activate_captured_build(&mut self, request: BuildLifecycleRequest);
    fn close_old_build(&mut self, request: BuildLifecycleRequest);
    fn mask_captured_build(&mut self, request: BuildMaskRequest);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureCallerEdge {
    /// Return `1`; `Object::do_damage` exits immediately at `0x0064C550..0x0064C558`.
    StopObjectDoDamage,
    /// Return `0`; run `damage_fallthrough`'s `0x0064C558..0x0064C86B` continuation.
    CaptureZeroFallthrough,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BuildCheckCaptureMutation {
    MarkContained(MarkContainedBuildRequest),
    RevealDefender(RevealCaptureDefenderRequest),
    CaptureCity {
        request: CaptureCityBoundaryRequest,
        new_city: CityKey,
    },
    SetCityCaptureStrength(CityCaptureStrengthRequest),
    SwapBuildTeam {
        request: CaptureObjectBoundaryRequest,
        new_build: Option<ObjectKey>,
    },
    KillBuildAfterFailedSwap(ObjectKey),
    PlaySound(CaptureSound),
    Announce(CaptureAnnouncementRequest),
    Activate(BuildLifecycleRequest),
    Close(BuildLifecycleRequest),
    Mask(BuildMaskRequest),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildCheckCaptureReceipt {
    pub request: CaptureCheckRequest,
    pub return_value: i32,
    pub caller_edge: CaptureCallerEdge,
    pub census: Option<CaptureCensusReceipt>,
    pub mutations: Vec<BuildCheckCaptureMutation>,
}

impl BuildCheckCaptureReceipt {
    /// Adapter receipt consumed by `damage_world::apply_capture_attempt`.
    pub const fn capture_attempt_receipt(&self) -> super::damage_world::CaptureCheckReceipt {
        super::damage_world::CaptureCheckReceipt {
            request: self.request,
            returned_nonzero: self.return_value != 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildCheckCaptureApplyError {
    MissingCityCaptureReceipt,
    CityCaptureIdentityMismatch,
    CityCaptureEffectsIncomplete,
    MissingBuildSwapReceipt,
    BuildSwapIdentityMismatch,
    BuildSwapEffectsIncomplete,
}

fn finish_receipt(
    plan: &BuildCheckCapturePlan,
    return_value: i32,
    mutations: Vec<BuildCheckCaptureMutation>,
) -> BuildCheckCaptureReceipt {
    BuildCheckCaptureReceipt {
        request: plan.request,
        return_value,
        caller_edge: if return_value == 0 {
            CaptureCallerEdge::CaptureZeroFallthrough
        } else {
            CaptureCallerEdge::StopObjectDoDamage
        },
        census: plan.census.clone(),
        mutations,
    }
}

/// Apply the recovered transaction in retail instruction order.
pub fn apply_build_check_capture<W: BuildCheckCaptureWorld + ?Sized>(
    plan: &BuildCheckCapturePlan,
    world: &mut W,
) -> Result<BuildCheckCaptureReceipt, BuildCheckCaptureApplyError> {
    let mut mutations = Vec::new();
    if let Some(mark) = plan.mark_contained {
        world.mark_contained_build(mark);
        mutations.push(BuildCheckCaptureMutation::MarkContained(mark));
    }
    if let Some(census) = &plan.census {
        for reveal in &census.reveals {
            world.reveal_capture_defender(*reveal);
            mutations.push(BuildCheckCaptureMutation::RevealDefender(*reveal));
        }
    }

    match plan.outcome {
        BuildCheckCaptureOutcomePlan::ReturnZero(_) => Ok(finish_receipt(plan, 0, mutations)),
        BuildCheckCaptureOutcomePlan::CaptureCity(request) => {
            let receipt = world
                .capture_city(request)
                .ok_or(BuildCheckCaptureApplyError::MissingCityCaptureReceipt)?;
            if receipt.request != request
                || receipt.new_city.who != request.new_owner
                || receipt.new_city.city < 0
            {
                return Err(BuildCheckCaptureApplyError::CityCaptureIdentityMismatch);
            }
            if !receipt.buildings_reassigned
                || !receipt.economy_and_plunder_applied
                || !receipt.diplomacy_and_score_applied
                || !receipt.object_transfer_applied
            {
                return Err(BuildCheckCaptureApplyError::CityCaptureEffectsIncomplete);
            }
            mutations.push(BuildCheckCaptureMutation::CaptureCity {
                request,
                new_city: receipt.new_city,
            });
            let strength = CityCaptureStrengthRequest {
                city: receipt.new_city,
                value: plan.attacker_capture_value,
            };
            world.set_city_capture_strength(strength);
            mutations.push(BuildCheckCaptureMutation::SetCityCaptureStrength(strength));
            Ok(finish_receipt(plan, 1, mutations))
        }
        BuildCheckCaptureOutcomePlan::CaptureObject(request) => {
            let receipt = world
                .swap_build_team(request)
                .ok_or(BuildCheckCaptureApplyError::MissingBuildSwapReceipt)?;
            if receipt.request != request
                || receipt
                    .new_build
                    .is_some_and(|build| build.who != request.new_owner || build.o < 0)
            {
                return Err(BuildCheckCaptureApplyError::BuildSwapIdentityMismatch);
            }
            mutations.push(BuildCheckCaptureMutation::SwapBuildTeam {
                request,
                new_build: receipt.new_build,
            });

            let Some(new_build) = receipt.new_build else {
                world.kill_build_after_failed_swap(request.old_build);
                mutations.push(BuildCheckCaptureMutation::KillBuildAfterFailedSwap(
                    request.old_build,
                ));
                return Ok(finish_receipt(plan, 0, mutations));
            };
            if !receipt.ownership_applied || !receipt.object_copy_applied {
                return Err(BuildCheckCaptureApplyError::BuildSwapEffectsIncomplete);
            }

            if request.play_lost_sound {
                world.play_capture_sound(CaptureSound::LostObject0x86);
                mutations.push(BuildCheckCaptureMutation::PlaySound(
                    CaptureSound::LostObject0x86,
                ));
            }
            if request.play_captured_sound {
                world.play_capture_sound(CaptureSound::CapturedObject0x7c);
                mutations.push(BuildCheckCaptureMutation::PlaySound(
                    CaptureSound::CapturedObject0x7c,
                ));
            }
            if request.notify_console {
                let announcement = CaptureAnnouncementRequest {
                    old_build: request.old_build,
                    new_build,
                    red_localized_message: true,
                };
                world.announce_capture(announcement);
                mutations.push(BuildCheckCaptureMutation::Announce(announcement));
            }

            let activate = BuildLifecycleRequest {
                build: new_build,
                first: 0,
                second: 1,
                third: 0,
            };
            world.activate_captured_build(activate);
            mutations.push(BuildCheckCaptureMutation::Activate(activate));
            let close = BuildLifecycleRequest {
                build: request.old_build,
                first: 0,
                second: -1,
                third: 0,
            };
            world.close_old_build(close);
            mutations.push(BuildCheckCaptureMutation::Close(close));
            let mask = BuildMaskRequest {
                build: new_build,
                first: 1,
                second: 2,
            };
            world.mask_captured_build(mask);
            mutations.push(BuildCheckCaptureMutation::Mask(mask));
            Ok(finish_receipt(plan, 1, mutations))
        }
    }
}
