//! Exact pattern-0 mountain-template retry transaction in `TerrainGroups::place_all`.
//!
//! The shipped PDB identifies the enclosing function at `0x006a70d0`; the
//! retail instruction stream at `0x006a8b95`--`0x006a8d20` fixes three cursor
//! cycles around `TerrainGroup::place_player_group` (`0x006a4190`).  Each call
//! is preceded by `NetDaemon::process_all` and followed by `Mountains::get_range`
//! even when it succeeds.  Exhausting the primary cycle randomizes all three
//! mountain lists.  The one-step-smaller cycle always randomizes afterward,
//! while the special size-two tail runs a size-one cycle only for side effects
//! and preserves the successful result of the preceding cycle.

use super::map_terrain::World;
use super::mountains::{MountainRandomizeReceipt, Mountains};
use super::terrain_groups::TerrainGroup;
use super::terrain_player_group::{
    PlacePlayerGroupCall, PlacePlayerGroupError, PlacePlayerGroupOutcome, PlacePlayerGroupReceipt,
    PlayerGroupExternalRequest, PlayerGroupExternalResolution, PlayerMountainResolver,
    RecordedPlayerMountainResolver,
};
use crate::rng::Random;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MountainTemplateRetryStage {
    Primary,
    OneStepSmaller,
    SmallestSideEffect,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MountainTemplateRetryAttempt {
    pub stage: MountainTemplateRetryStage,
    pub range_size: i32,
    pub template: i32,
    /// Retail fetches the next cursor value after every resolved call,
    /// including a successful call. It is absent only when a typed host effect
    /// is unresolved and the native return value is therefore still unknown.
    pub next_template: Option<i32>,
    pub placement: PlacePlayerGroupReceipt,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerMountainTemplateRetryReceipt {
    /// Template used by the failed call immediately preceding this transaction.
    pub initial_template: i32,
    pub attempts: Vec<MountainTemplateRetryAttempt>,
    /// Ordered calls to `Mountains::randomize_mountains` at `0x006a8c08` and,
    /// when reached, `0x006a8c90`.
    pub randomizations: Vec<MountainRandomizeReceipt>,
    pub external_resolutions_consumed: usize,
    pub outcome: PlacePlayerGroupOutcome,
    pub rng_state_after: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlayerMountainTemplateRetryError {
    UnsupportedGroupType { group_type: i32 },
    UnsupportedTargetTiles { target_tiles: i32 },
    UnexpectedGrowthOutcome,
    InvalidPlayerGroup(PlacePlayerGroupError),
}

enum PhaseOutcome {
    Returned(i32),
    ExternalResolutionRequired(PlayerGroupExternalRequest),
}

impl TerrainGroup {
    /// Execute `place_all`'s type-5 continuation after the first mountain
    /// `place_player_group` call returned zero.
    ///
    /// `pump` models the simulation-external daemon call immediately before
    /// each retry. `call.land_subtype` is ignored; `initial_template` is the
    /// exact value used by the failed call at `0x006a8b86`.
    #[allow(clippy::too_many_arguments)]
    pub fn apply_player_mountain_template_retry(
        &mut self,
        world: &mut World,
        random: &mut Random,
        mountains: &mut Mountains,
        call: PlacePlayerGroupCall,
        initial_template: i32,
        formation_x: &mut Vec<i32>,
        formation_y: &mut Vec<i32>,
        externals: &[PlayerGroupExternalResolution],
        mut pump: impl FnMut(),
    ) -> Result<PlayerMountainTemplateRetryReceipt, PlayerMountainTemplateRetryError> {
        let mut resolver = RecordedPlayerMountainResolver::new(externals);
        self.apply_player_mountain_template_retry_with_resolver(
            world,
            random,
            mountains,
            call,
            initial_template,
            formation_x,
            formation_y,
            externals,
            &mut resolver,
            &mut pump,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn apply_player_mountain_template_retry_with_resolver(
        &mut self,
        world: &mut World,
        random: &mut Random,
        mountains: &mut Mountains,
        call: PlacePlayerGroupCall,
        initial_template: i32,
        formation_x: &mut Vec<i32>,
        formation_y: &mut Vec<i32>,
        externals: &[PlayerGroupExternalResolution],
        mountain_resolver: &mut impl PlayerMountainResolver,
        pump: &mut impl FnMut(),
    ) -> Result<PlayerMountainTemplateRetryReceipt, PlayerMountainTemplateRetryError> {
        if self.group_type != 5 {
            return Err(PlayerMountainTemplateRetryError::UnsupportedGroupType {
                group_type: self.group_type,
            });
        }
        // `place_all` caps type-5 primary sizes at three and validates a
        // positive inclusive size span before reaching this block.
        if !(1..=3).contains(&call.target_tiles) {
            return Err(PlayerMountainTemplateRetryError::UnsupportedTargetTiles {
                target_tiles: call.target_tiles,
            });
        }

        let mut receipt = PlayerMountainTemplateRetryReceipt {
            initial_template,
            attempts: Vec::new(),
            randomizations: Vec::new(),
            external_resolutions_consumed: 0,
            outcome: PlacePlayerGroupOutcome::Returned(0),
            rng_state_after: random.state(),
        };

        let primary = run_phase(
            self,
            world,
            random,
            mountains,
            call,
            initial_template,
            call.target_tiles,
            MountainTemplateRetryStage::Primary,
            formation_x,
            formation_y,
            externals,
            mountain_resolver,
            &mut receipt,
            pump,
        )?;
        let primary_result = match primary {
            PhaseOutcome::Returned(value) => value,
            PhaseOutcome::ExternalResolutionRequired(request) => {
                return Ok(finish(receipt, random, request));
            }
        };
        if primary_result != 0 {
            receipt.outcome = PlacePlayerGroupOutcome::Returned(primary_result);
            receipt.rng_state_after = random.state();
            return Ok(receipt);
        }

        receipt
            .randomizations
            .push(mountains.randomize_mountains(random));
        if call.target_tiles <= 1 {
            receipt.rng_state_after = random.state();
            return Ok(receipt);
        }

        let smaller_size = call.target_tiles - 1;
        let smaller_template = mountains.get_range_raw(smaller_size);
        let smaller = run_phase(
            self,
            world,
            random,
            mountains,
            call,
            smaller_template,
            smaller_size,
            MountainTemplateRetryStage::OneStepSmaller,
            formation_x,
            formation_y,
            externals,
            mountain_resolver,
            &mut receipt,
            pump,
        )?;
        let smaller_result = match smaller {
            PhaseOutcome::Returned(value) => value,
            PhaseOutcome::ExternalResolutionRequired(request) => {
                return Ok(finish(receipt, random, request));
            }
        };

        // This call is unconditional after a resolved one-step-smaller cycle,
        // including both its success and exhausted-zero exits.
        receipt
            .randomizations
            .push(mountains.randomize_mountains(random));
        if smaller_result == 0 || call.target_tiles != 2 {
            receipt.outcome = PlacePlayerGroupOutcome::Returned(smaller_result);
            receipt.rng_state_after = random.state();
            return Ok(receipt);
        }

        let smallest_template = mountains.get_range_raw(smaller_size);
        let smallest = run_phase(
            self,
            world,
            random,
            mountains,
            call,
            smallest_template,
            smaller_size,
            MountainTemplateRetryStage::SmallestSideEffect,
            formation_x,
            formation_y,
            externals,
            mountain_resolver,
            &mut receipt,
            pump,
        )?;
        if let PhaseOutcome::ExternalResolutionRequired(request) = smallest {
            return Ok(finish(receipt, random, request));
        }

        // `0x006a8d1d` restores the saved result of the one-step-smaller call;
        // the size-one cycle's return value is deliberately discarded.
        receipt.outcome = PlacePlayerGroupOutcome::Returned(smaller_result);
        receipt.rng_state_after = random.state();
        Ok(receipt)
    }
}

#[allow(clippy::too_many_arguments)]
fn run_phase(
    group: &mut TerrainGroup,
    world: &mut World,
    random: &mut Random,
    mountains: &mut Mountains,
    base_call: PlacePlayerGroupCall,
    initial_template: i32,
    range_size: i32,
    stage: MountainTemplateRetryStage,
    formation_x: &mut Vec<i32>,
    formation_y: &mut Vec<i32>,
    externals: &[PlayerGroupExternalResolution],
    mountain_resolver: &mut impl PlayerMountainResolver,
    receipt: &mut PlayerMountainTemplateRetryReceipt,
    pump: &mut impl FnMut(),
) -> Result<PhaseOutcome, PlayerMountainTemplateRetryError> {
    let baseline = initial_template;
    let mut template = initial_template;
    loop {
        pump();
        let placement = group
            .apply_place_player_group_with_mountain_resolver(
                world,
                random,
                PlacePlayerGroupCall {
                    land_subtype: template,
                    strict_type_four: false,
                    ..base_call
                },
                formation_x,
                formation_y,
                &externals[receipt.external_resolutions_consumed..],
                mountain_resolver,
            )
            .map_err(PlayerMountainTemplateRetryError::InvalidPlayerGroup)?;
        receipt.external_resolutions_consumed += placement.external_resolutions_consumed;

        match placement.outcome.clone() {
            PlacePlayerGroupOutcome::ExternalResolutionRequired { request } => {
                receipt.attempts.push(MountainTemplateRetryAttempt {
                    stage,
                    range_size,
                    template,
                    next_template: None,
                    placement,
                });
                return Ok(PhaseOutcome::ExternalResolutionRequired(request));
            }
            PlacePlayerGroupOutcome::GrowthKernel { .. } => {
                return Err(PlayerMountainTemplateRetryError::UnexpectedGrowthOutcome);
            }
            PlacePlayerGroupOutcome::Returned(return_value) => {
                // Native advances the mountain cursor before testing either the
                // placement result or the cycle sentinel.
                let next_template = mountains.get_range_raw(range_size);
                receipt.attempts.push(MountainTemplateRetryAttempt {
                    stage,
                    range_size,
                    template,
                    next_template: Some(next_template),
                    placement,
                });
                if return_value != 0 || next_template == baseline {
                    return Ok(PhaseOutcome::Returned(return_value));
                }
                template = next_template;
            }
        }
    }
}

fn finish(
    mut receipt: PlayerMountainTemplateRetryReceipt,
    random: &Random,
    request: PlayerGroupExternalRequest,
) -> PlayerMountainTemplateRetryReceipt {
    receipt.outcome = PlacePlayerGroupOutcome::ExternalResolutionRequired { request };
    receipt.rng_state_after = random.state();
    receipt
}
