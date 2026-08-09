//! `Cities::capture_city` transaction prefix, `0x00733380..0x00733749`.
//!
//! The full retail procedure is 7,998 bytes. This module owns its first coherent
//! 969-byte transaction: diplomacy cleanup, capital/Persian bookkeeping, capture/loss
//! counters, the guarded platform achievement, leader capture flags, radius state, the
//! city-center team swap, immediate activation, initial plunder, and the two achievement
//! events. The result is a typed continuation at the first branch on the swap result.

use super::build_check_capture::{CaptureCityBoundaryRequest, CityKey};
use super::damage_world::ObjectKey;

pub const CITIES_CAPTURE_CITY_START: u32 = 0x0073_3380;
pub const CITIES_CAPTURE_PREFIX_END: u32 = 0x0073_3749;
pub const CITIES_CAPTURE_CITY_END: u32 = 0x0073_52BE;
pub const CITIES_CAPTURE_PREFIX_SIZE: u32 = CITIES_CAPTURE_PREFIX_END - CITIES_CAPTURE_CITY_START;
pub const CITIES_CAPTURE_CITY_SIZE: u32 = CITIES_CAPTURE_CITY_END - CITIES_CAPTURE_CITY_START;

pub const NUM_CAPTURE_PLAYERS: usize = 8;
pub const CAPITAL_CITY_FLAG: u16 = 0x10;
pub const CAPTURE_TOUCHED_LEADER_FLAG: u32 = 0x0200_0000;
pub const PERSIAN_TRIBE_BONUS_INDEX: i32 = 0x17;
pub const CONQUEROR_ACHIEVEMENT_ID: i32 = 0x0B;
pub const CITY_NAME_FIELD_OFFSET: u16 = 0x90;
pub const MAX_CAPTURE_RADIUS: i32 = 0x40;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PersianCapitalLookup {
    /// First output of `LeaderData::find_capital`; retail only tests it for nonnegative.
    pub found_city: i32,
    /// Second output; equality with the old owner is retained for the later body.
    pub found_owner: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CityCapturePlatformGate {
    /// `Game +0x77`; `None` represents a sentinel/non-player value.
    pub active_player: Option<u8>,
    /// `Game +0x820`. Bit `0x10` suppresses the unlock.
    pub game_flags_0x820: u32,
    /// `Game +0x69C`; value `1` suppresses the unlock.
    pub mode_0x69c: i32,
    /// `Game +0x6A8`; value `1` suppresses the unlock.
    pub mode_0x6a8: i32,
    /// Selected setup-player record byte at `+0xCB`; nonzero suppresses the unlock.
    pub setup_player_flag_0xcb: u8,
}

impl CityCapturePlatformGate {
    #[inline]
    pub const fn unlocks_for(self, new_owner: u8) -> bool {
        matches!(self.active_player, Some(who) if who == new_owner)
            && self.game_flags_0x820 & 0x10 == 0
            && self.mode_0x69c != 1
            && self.mode_0x6a8 != 1
            && self.setup_player_flag_0xcb == 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CitiesCapturePrefixInput {
    /// Identity and argument order inherited from `Build::check_capture`.
    pub request: CaptureCityBoundaryRequest,
    /// `CityData::city_flags` of the old record.
    pub old_city_flags: u16,
    /// The old center object indexed by `CityData::o`, resolved under `old_owner`.
    pub old_center: ObjectKey,
    /// `LeaderData::dip[new_owner].agree` for the old owner.
    pub old_owner_diplomacy_agree: i32,
    /// `LeaderData::has_tribe_bonus(0x17)` (Persians) for the old owner.
    pub old_owner_has_persian_bonus: bool,
    /// Mandatory exactly when the city is a capital and the Persian bonus is present.
    pub persian_capital_lookup: Option<PersianCapitalLookup>,
    /// `old_owner.get_radius(old_center_type)`, before retail's high clamp.
    pub old_center_radius: i32,
    /// `Constants::CITY_PLUNDER_PER_LEVEL` (`+0x304`).
    pub city_plunder_per_level: i32,
    pub platform: CityCapturePlatformGate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CitiesCapturePrefixPlanError {
    InvalidOwner(u8),
    OldCityOwnerMismatch,
    OldCenterOwnerMismatch,
    InvalidOldCityIndex,
    InvalidOldCenterIndex,
    MissingPersianCapitalLookup,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClearCaptureDiplomacyRequest {
    pub old_owner: u8,
    pub new_owner: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureCounter {
    CitiesCaptured,
    CitiesLost,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IncrementCaptureCounterRequest {
    pub who: u8,
    pub counter: CaptureCounter,
    pub amount: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlatformAchievement {
    Conqueror = CONQUEROR_ACHIEVEMENT_ID as isize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnlockCityCaptureAchievementRequest {
    /// `StatsAndAchievements::Achievement` index 11, `ACH_CONQUEROR`.
    pub achievement: PlatformAchievement,
    /// Second argument to `UnlockAchievement`; retail passes true.
    pub unlock: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MarkCaptureTouchedLeaderRequest {
    pub who: u8,
    pub or_mask: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SwapCityCenterRequest {
    pub old_center: ObjectKey,
    pub new_owner: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActivateCapturedCenterRequest {
    pub new_center: ObjectKey,
    pub first: i32,
    pub second: i32,
    pub third: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureAchievementEvent {
    CityCaptured = 6,
    CityLost = 7,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptureAchievementEventRequest {
    pub event: CaptureAchievementEvent,
    pub who: u8,
    /// Both events receive the old `CityData::name` String at offset `0x90`.
    pub city_name_source: CityKey,
    pub city_name_field_offset: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CitiesCapturePrefixPlan {
    pub request: CaptureCityBoundaryRequest,
    pub clear_diplomacy_actions: bool,
    pub old_city_was_capital: bool,
    /// Persian-only fact retained for the downstream capital/recapture phase.
    pub captured_own_capital: bool,
    pub unlock_achievement_11: bool,
    pub old_center: ObjectKey,
    pub capture_radius: i32,
    pub capture_radius_ring: i32,
    pub city_plunder_per_level: i32,
}

fn valid_owner(who: u8) -> Result<(), CitiesCapturePrefixPlanError> {
    if usize::from(who) < NUM_CAPTURE_PLAYERS {
        Ok(())
    } else {
        Err(CitiesCapturePrefixPlanError::InvalidOwner(who))
    }
}

/// Exact signed `(+3)/4` sequence at `0x007335F4..0x00733600`.
#[inline]
pub const fn capture_radius_ring(radius: i32) -> i32 {
    let clamped = if radius > MAX_CAPTURE_RADIUS {
        MAX_CAPTURE_RADIUS
    } else {
        radius
    };
    let plus_three = clamped.wrapping_add(3);
    plus_three.wrapping_add((plus_three >> 31) & 3) >> 2
}

/// Resolve every infallible retail-global read needed before prefix mutations begin.
pub fn plan_cities_capture_prefix(
    input: CitiesCapturePrefixInput,
) -> Result<CitiesCapturePrefixPlan, CitiesCapturePrefixPlanError> {
    valid_owner(input.request.new_owner)?;
    valid_owner(input.request.old_owner)?;
    if input.request.old_city.who != input.request.old_owner {
        return Err(CitiesCapturePrefixPlanError::OldCityOwnerMismatch);
    }
    if input.old_center.who != input.request.old_owner {
        return Err(CitiesCapturePrefixPlanError::OldCenterOwnerMismatch);
    }
    if input.request.old_city.city < 0 {
        return Err(CitiesCapturePrefixPlanError::InvalidOldCityIndex);
    }
    if input.old_center.o < 0 {
        return Err(CitiesCapturePrefixPlanError::InvalidOldCenterIndex);
    }

    let old_city_was_capital = input.old_city_flags & CAPITAL_CITY_FLAG != 0;
    let captured_own_capital = if input.old_owner_has_persian_bonus && old_city_was_capital {
        let lookup = input
            .persian_capital_lookup
            .ok_or(CitiesCapturePrefixPlanError::MissingPersianCapitalLookup)?;
        lookup.found_city >= 0
            && lookup.found_owner >= 0
            && lookup.found_owner == i32::from(input.request.old_owner)
    } else {
        false
    };
    let capture_radius = input.old_center_radius.min(MAX_CAPTURE_RADIUS);

    Ok(CitiesCapturePrefixPlan {
        request: input.request,
        clear_diplomacy_actions: input.old_owner_diplomacy_agree == 1,
        old_city_was_capital,
        captured_own_capital,
        unlock_achievement_11: input.platform.unlocks_for(input.request.new_owner),
        old_center: input.old_center,
        capture_radius,
        capture_radius_ring: capture_radius_ring(input.old_center_radius),
        city_plunder_per_level: input.city_plunder_per_level,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapturedCenterState {
    pub new_center: ObjectKey,
    /// `BuildData::city` read from the new center after activation.
    pub new_city: CityKey,
    /// `CityData::get_level()` on the new city.
    pub new_city_level: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwapCityCenterResult {
    Failed,
    Succeeded(CapturedCenterState),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SwapCityCenterReceipt {
    pub request: SwapCityCenterRequest,
    pub result: SwapCityCenterResult,
    /// Mandatory on success; the boundary may not silently return an index only.
    pub ownership_applied: bool,
    pub object_copy_applied: bool,
}

pub trait CitiesCapturePrefixWorld {
    fn clear_capture_diplomacy(&mut self, request: ClearCaptureDiplomacyRequest);
    fn increment_capture_counter(&mut self, request: IncrementCaptureCounterRequest);
    fn unlock_city_capture_achievement(&mut self, request: UnlockCityCaptureAchievementRequest);
    fn mark_capture_touched_leader(&mut self, request: MarkCaptureTouchedLeaderRequest);
    fn swap_city_center(&mut self, request: SwapCityCenterRequest)
        -> Option<SwapCityCenterReceipt>;
    fn activate_captured_center(&mut self, request: ActivateCapturedCenterRequest);
    fn add_capture_achievement_event(&mut self, request: CaptureAchievementEventRequest);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CitiesCapturePrefixContinuation {
    /// `local_14 == -1` jumps to the old member census at `0x00733CE2`.
    CenterSwapFailed0x00733ce2,
    /// A valid new city enters `City::capture` and member reassignment at `0x00733755`.
    CenterSwapSucceeded0x00733755,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CitiesCapturePrefixMutation {
    ClearDiplomacy(ClearCaptureDiplomacyRequest),
    IncrementCounter(IncrementCaptureCounterRequest),
    UnlockAchievement(UnlockCityCaptureAchievementRequest),
    MarkLeader(MarkCaptureTouchedLeaderRequest),
    SwapCenter(SwapCityCenterReceipt),
    ActivateCenter(ActivateCapturedCenterRequest),
    AchievementEvent(CaptureAchievementEventRequest),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CitiesCapturePrefixReceipt {
    pub request: CaptureCityBoundaryRequest,
    pub old_city_was_capital: bool,
    pub captured_own_capital: bool,
    pub capture_radius: i32,
    pub capture_radius_ring: i32,
    /// The first SimpleArray always contains the old city center at this boundary.
    pub old_city_objects: Vec<ObjectKey>,
    /// Empty on swap failure; otherwise contains the new center at this boundary.
    pub new_city_objects: Vec<ObjectKey>,
    pub new_city: Option<CityKey>,
    /// Zero on failure, otherwise `CITY_PLUNDER_PER_LEVEL * (level - 1)`.
    pub plunder_accumulator: i32,
    pub continuation: CitiesCapturePrefixContinuation,
    pub mutations: Vec<CitiesCapturePrefixMutation>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CitiesCapturePrefixApplyError {
    MissingSwapReceipt,
    SwapRequestMismatch,
    SuccessfulSwapIdentityMismatch,
    SuccessfulSwapEffectsIncomplete,
}

fn achievement_event(
    request: CaptureCityBoundaryRequest,
    event: CaptureAchievementEvent,
) -> CaptureAchievementEventRequest {
    CaptureAchievementEventRequest {
        event,
        who: match event {
            CaptureAchievementEvent::CityCaptured => request.new_owner,
            CaptureAchievementEvent::CityLost => request.old_owner,
        },
        city_name_source: request.old_city,
        city_name_field_offset: CITY_NAME_FIELD_OFFSET,
    }
}

/// Apply the recovered prefix in retail instruction order.
pub fn apply_cities_capture_prefix<W: CitiesCapturePrefixWorld + ?Sized>(
    plan: CitiesCapturePrefixPlan,
    world: &mut W,
) -> Result<CitiesCapturePrefixReceipt, CitiesCapturePrefixApplyError> {
    let mut mutations = Vec::new();
    if plan.clear_diplomacy_actions {
        let request = ClearCaptureDiplomacyRequest {
            old_owner: plan.request.old_owner,
            new_owner: plan.request.new_owner,
        };
        world.clear_capture_diplomacy(request);
        mutations.push(CitiesCapturePrefixMutation::ClearDiplomacy(request));
    }

    let lost = IncrementCaptureCounterRequest {
        who: plan.request.old_owner,
        counter: CaptureCounter::CitiesLost,
        amount: 1,
    };
    world.increment_capture_counter(lost);
    mutations.push(CitiesCapturePrefixMutation::IncrementCounter(lost));
    let captured = IncrementCaptureCounterRequest {
        who: plan.request.new_owner,
        counter: CaptureCounter::CitiesCaptured,
        amount: 1,
    };
    world.increment_capture_counter(captured);
    mutations.push(CitiesCapturePrefixMutation::IncrementCounter(captured));

    if plan.unlock_achievement_11 {
        let unlock = UnlockCityCaptureAchievementRequest {
            achievement: PlatformAchievement::Conqueror,
            unlock: true,
        };
        world.unlock_city_capture_achievement(unlock);
        mutations.push(CitiesCapturePrefixMutation::UnlockAchievement(unlock));
    }

    // Retail marks the new owner first, then the old owner.
    for who in [plan.request.new_owner, plan.request.old_owner] {
        let request = MarkCaptureTouchedLeaderRequest {
            who,
            or_mask: CAPTURE_TOUCHED_LEADER_FLAG,
        };
        world.mark_capture_touched_leader(request);
        mutations.push(CitiesCapturePrefixMutation::MarkLeader(request));
    }

    let swap_request = SwapCityCenterRequest {
        old_center: plan.old_center,
        new_owner: plan.request.new_owner,
    };
    let swap = world
        .swap_city_center(swap_request)
        .ok_or(CitiesCapturePrefixApplyError::MissingSwapReceipt)?;
    if swap.request != swap_request {
        return Err(CitiesCapturePrefixApplyError::SwapRequestMismatch);
    }
    mutations.push(CitiesCapturePrefixMutation::SwapCenter(swap));

    let (new_city_objects, new_city, plunder_accumulator, continuation) = match swap.result {
        SwapCityCenterResult::Failed => (
            Vec::new(),
            None,
            0,
            CitiesCapturePrefixContinuation::CenterSwapFailed0x00733ce2,
        ),
        SwapCityCenterResult::Succeeded(state) => {
            if state.new_center.who != plan.request.new_owner
                || state.new_center.o < 0
                || state.new_city.who != plan.request.new_owner
                || state.new_city.city < 0
            {
                return Err(CitiesCapturePrefixApplyError::SuccessfulSwapIdentityMismatch);
            }
            if !swap.ownership_applied || !swap.object_copy_applied {
                return Err(CitiesCapturePrefixApplyError::SuccessfulSwapEffectsIncomplete);
            }
            let activate = ActivateCapturedCenterRequest {
                new_center: state.new_center,
                first: 1,
                second: 1,
                third: 0,
            };
            world.activate_captured_center(activate);
            mutations.push(CitiesCapturePrefixMutation::ActivateCenter(activate));
            (
                vec![state.new_center],
                Some(state.new_city),
                plan.city_plunder_per_level
                    .wrapping_mul(state.new_city_level.wrapping_sub(1)),
                CitiesCapturePrefixContinuation::CenterSwapSucceeded0x00733755,
            )
        }
    };

    for event in [
        CaptureAchievementEvent::CityCaptured,
        CaptureAchievementEvent::CityLost,
    ] {
        let request = achievement_event(plan.request, event);
        world.add_capture_achievement_event(request);
        mutations.push(CitiesCapturePrefixMutation::AchievementEvent(request));
    }

    Ok(CitiesCapturePrefixReceipt {
        request: plan.request,
        old_city_was_capital: plan.old_city_was_capital,
        captured_own_capital: plan.captured_own_capital,
        capture_radius: plan.capture_radius,
        capture_radius_ring: plan.capture_radius_ring,
        old_city_objects: vec![plan.old_center],
        new_city_objects,
        new_city,
        plunder_accumulator,
        continuation,
        mutations,
    })
}
