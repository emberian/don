//! Common terrain-group world-generation stages.
//!
//! The shipped PDB fixes `TerrainGroups` at 280 bytes, `TerrainGroup` at 156,
//! `Fractal` at 120, and identifies the two common calls made by `Map::make`:
//! `TerrainGroups::fill_fertile` (`0x006a6f90`) followed by
//! `TerrainGroups::place_all` (`0x006a70d0`).  Behavior here is checked against
//! Ghidra and the retail instruction stream, not inferred from terrain names.
//!
//! `fill_fertile` is complete for a supplied `Fractal::frac` byte plane and its
//! supplied partition thresholds.  The plane is produced upstream by
//! `TerrainGroups::init_tileset_data`; this module deliberately does not invent
//! a fractal generator or tileset frequencies.

use super::map_terrain::World;
use super::mountains::{MountainRandomizeReceipt, Mountains};
use super::regions::WCoordList;
use crate::rng::Random;

/// `TerrainGroup` (PDB size 156), excluding native vbase/pointer representation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TerrainGroup {
    /// PDB field `type` at `+0x04`.
    pub group_type: i32,
    pub chance: i32,
    pub grouping: i32,
    pub min_clumps: i32,
    pub max_clumps: i32,
    pub pattern: i32,
    pub min_size: i32,
    pub max_size: i32,
    pub start_min: i32,
    pub start_max: i32,
    pub forest_space: i32,
    pub mount_space: i32,
    pub rock_space: i32,
    pub coast_space: i32,
    pub cent_min: i32,
    pub cent_max: i32,
    pub edge_min: i32,
    pub edge_max: i32,
    pub corner_min: i32,
    pub corner_max: i32,
    pub min_oil: i32,
    pub max_oil: i32,
    pub cliff_face: i32,
    pub touching_mountain: i32,
    pub placed: Vec<i32>,
    pub tiles: WCoordList,
}

/// The `Fractal::frac : ObjectArray<Array<unsigned char>>` values consumed by
/// `fill_fertile`.  The outer index is X and each inner array is indexed by Y.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FertilityFractal {
    pub columns: Vec<Vec<u8>>,
}

/// Deterministic inputs owned by the retail `TerrainGroups` singleton.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TerrainGroups {
    pub groups: Vec<TerrainGroup>,
    pub subtype_freqs: [Vec<i32>; 3],
    pub console_info: i32,
    pub fractal: FertilityFractal,
    pub partitions: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FillFertileError {
    InvalidWorldShape {
        xs: i32,
        ys: i32,
        size: i32,
        wdata_len: usize,
    },
    MissingFractalColumn {
        x: i32,
        required_columns: i32,
        actual_columns: usize,
    },
    ShortFractalColumn {
        x: i32,
        required_rows: i32,
        actual_rows: usize,
    },
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct FillFertileReceipt {
    /// Number of `WData.land == 0` cells written and logged by retail.
    pub fertile_cells: i32,
}

/// One row of the two zero-filled native temporary arrays used by `place_all`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct TerrainGroupSelection {
    pub selected: bool,
    /// Zero for rejected groups; otherwise the fixed or randomly selected clumps.
    pub clumps: i32,
}

/// Deterministic result through `0x006a7615`, immediately before the placement
/// pass resets its group index and reaches `NetDaemon::process_all`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerrainGroupSelectionReceipt {
    pub groups: Vec<TerrainGroupSelection>,
    /// Accumulators at `0x00cbe460` before retail normalizes them in place.
    pub raw_clumps_by_type: [i32; 5],
    /// The same five globals after `0x006a75c0`--`0x006a75e9`.
    pub normalized_clumps_by_type: [i32; 5],
    pub chance_draws: u32,
    pub clump_draws: u32,
    pub rng_state_after: i32,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TerrainGroupSelectionError {
    /// Native indexes a five-element table with `TerrainGroup::type - 4`.
    UnsupportedGroupType { group_index: usize, group_type: i32 },
    /// Native's inclusive `(max - min) + 1` divisor must fit positive `i32`.
    InvalidClumpSpan {
        group_index: usize,
        min_clumps: i32,
        max_clumps: i32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaceAllPreviewReceipt {
    pub mountain_randomization: MountainRandomizeReceipt,
    pub group_selection: TerrainGroupSelectionReceipt,
}

/// First unresolved dependency in `TerrainGroups::place_all`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlaceAllError {
    InvalidTerrainGroupSelection(TerrainGroupSelectionError),
    /// The exact randomization, selection, and five-type normalization prefix
    /// completed on preview state.  Retail next pumps the network daemon at
    /// callsite `0x006a7645`, before it starts placing the first group.
    NetDaemonProcessAllUnavailable {
        preview: PlaceAllPreviewReceipt,
    },
}

impl TerrainGroups {
    /// Complete `TerrainGroups::fill_fertile` `0x006a6f90`--`0x006a70cd`.
    ///
    /// Retail scans X outside / Y inside.  For every cell whose `land` byte is
    /// exactly zero, it finds the first partition strictly greater than the
    /// fractal byte.  Equality advances to the next bucket.  It logs that integer,
    /// narrows it into `land_sub`, and redundantly writes `land = 0`; all other
    /// `WData` bytes are preserved.  Non-fertile land classes are skipped.
    pub fn fill_fertile(&self, world: &mut World) -> Result<FillFertileReceipt, FillFertileError> {
        self.fill_fertile_with_log(world, |_| {})
    }

    /// Same shipped body with the `Log::say` integer argument exposed so callers
    /// can preserve or test its observable X-major order.  Logging itself does not
    /// mutate deterministic simulation state.
    pub fn fill_fertile_with_log(
        &self,
        world: &mut World,
        mut log_bucket: impl FnMut(i32),
    ) -> Result<FillFertileReceipt, FillFertileError> {
        validate_world(world)?;
        self.validate_fractal_accesses(world)?;

        let mut receipt = FillFertileReceipt::default();
        for x in 0..world.xs {
            for y in 0..world.ys {
                if world.wdata(x, y).land != 0 {
                    continue;
                }

                let value = self.fractal.columns[x as usize][y as usize];
                let mut bucket = 0usize;
                while bucket < self.partitions.len() && value >= self.partitions[bucket] {
                    bucket += 1;
                }

                log_bucket(bucket as i32);
                let cell = world.wdata_mut(x, y);
                cell.land_sub = bucket as u8;
                cell.land = 0;
                receipt.fertile_cells += 1;
            }
        }
        Ok(receipt)
    }

    /// Fail-closed prefix of `TerrainGroups::place_all` `0x006a70d0`.
    ///
    /// Before the first terrain-group selection draw, retail randomizes mountains
    /// at `0x006a7330`, then executes [`Self::select_groups`] and normalizes five
    /// type accumulators.  The prefix is executed on preview clones, proving its
    /// composed RNG order while retaining the fail-closed contract: until the
    /// network/placement pass is recovered, no partial world, group, mountain-list,
    /// or RNG mutation escapes.
    pub fn place_all(
        &mut self,
        _world: &mut World,
        random: &mut Random,
        mountains: &mut Mountains,
        _progress: i32,
        _place_players: i32,
    ) -> Result<i32, PlaceAllError> {
        let mut preview_random = *random;
        let mut preview_mountains = mountains.clone();
        let mountain_randomization = preview_mountains.randomize_mountains(&mut preview_random);
        let group_selection = self
            .select_groups(&mut preview_random)
            .map_err(PlaceAllError::InvalidTerrainGroupSelection)?;
        Err(PlaceAllError::NetDaemonProcessAllUnavailable {
            preview: PlaceAllPreviewReceipt {
                mountain_randomization,
                group_selection,
            },
        })
    }

    /// Exact terrain-group chance/clump-selection transaction from
    /// `0x006a7445` through the five-type normalization ending at `0x006a7615`.
    ///
    /// A zero `grouping` always starts a new percentile draw.  Adjacent groups
    /// with the same nonzero `grouping` carry the signed remainder and the prior
    /// selected flag.  The short-circuit when that remainder is already negative
    /// is observable: it can reject without subtracting the current chance.
    /// Selected groups draw an inclusive clump count only when `min < max`.
    /// Structural validation precedes the first draw, so errors are transactional.
    pub fn select_groups(
        &self,
        random: &mut Random,
    ) -> Result<TerrainGroupSelectionReceipt, TerrainGroupSelectionError> {
        self.validate_selection_inputs()?;

        let mut selections = vec![TerrainGroupSelection::default(); self.groups.len()];
        let mut raw_clumps_by_type = [0i32; 5];
        let mut last_grouping = -10i32;
        let mut remaining_percentile = 0i32;
        let mut previous_selected = false;
        let mut chance_draws = 0u32;
        let mut clump_draws = 0u32;

        for (index, group) in self.groups.iter().enumerate() {
            if group.grouping == 0 || group.grouping != last_grouping {
                remaining_percentile = random.get(0, 0xffff) % 100;
                chance_draws += 1;
                last_grouping = group.grouping;
                previous_selected = false;
            }

            let reject_without_subtraction =
                remaining_percentile < 0 && (group.chance != 0 || !previous_selected);
            let rejected = if reject_without_subtraction {
                true
            } else {
                remaining_percentile = remaining_percentile.wrapping_sub(group.chance);
                remaining_percentile >= 0
            };

            if rejected {
                previous_selected = false;
                continue;
            }

            previous_selected = true;
            let mut clumps = group.min_clumps;
            if group.min_clumps < group.max_clumps {
                let span = group
                    .max_clumps
                    .wrapping_sub(group.min_clumps)
                    .wrapping_add(1);
                clumps = (random.get(0, 0xffff) % span).wrapping_add(group.min_clumps);
                clump_draws += 1;
            }
            selections[index] = TerrainGroupSelection {
                selected: true,
                clumps,
            };
            let type_index = (group.group_type - 4) as usize;
            raw_clumps_by_type[type_index] = raw_clumps_by_type[type_index].wrapping_add(clumps);
        }

        let normalized_clumps_by_type = raw_clumps_by_type.map(|total| {
            if total < 2 {
                i32::MAX
            } else {
                let half = total >> 1;
                if half & 1 != 0 {
                    half - 1
                } else {
                    half
                }
            }
        });

        Ok(TerrainGroupSelectionReceipt {
            groups: selections,
            raw_clumps_by_type,
            normalized_clumps_by_type,
            chance_draws,
            clump_draws,
            rng_state_after: random.state(),
        })
    }

    fn validate_selection_inputs(&self) -> Result<(), TerrainGroupSelectionError> {
        for (group_index, group) in self.groups.iter().enumerate() {
            if !(4..=8).contains(&group.group_type) {
                return Err(TerrainGroupSelectionError::UnsupportedGroupType {
                    group_index,
                    group_type: group.group_type,
                });
            }
            if group.min_clumps < group.max_clumps {
                let span = i64::from(group.max_clumps) - i64::from(group.min_clumps) + 1;
                if span > i64::from(i32::MAX) {
                    return Err(TerrainGroupSelectionError::InvalidClumpSpan {
                        group_index,
                        min_clumps: group.min_clumps,
                        max_clumps: group.max_clumps,
                    });
                }
            }
        }
        Ok(())
    }

    fn validate_fractal_accesses(&self, world: &World) -> Result<(), FillFertileError> {
        for x in 0..world.xs {
            for y in 0..world.ys {
                if world.wdata(x, y).land != 0 {
                    continue;
                }
                let Some(column) = self.fractal.columns.get(x as usize) else {
                    return Err(FillFertileError::MissingFractalColumn {
                        x,
                        required_columns: x + 1,
                        actual_columns: self.fractal.columns.len(),
                    });
                };
                if column.len() <= y as usize {
                    return Err(FillFertileError::ShortFractalColumn {
                        x,
                        required_rows: y + 1,
                        actual_rows: column.len(),
                    });
                }
            }
        }
        Ok(())
    }
}

fn validate_world(world: &World) -> Result<(), FillFertileError> {
    let expected = world.xs.checked_mul(world.ys);
    if world.xs < 0
        || world.ys < 0
        || expected != Some(world.size)
        || usize::try_from(world.size).ok() != Some(world.wdata.len())
    {
        return Err(FillFertileError::InvalidWorldShape {
            xs: world.xs,
            ys: world.ys,
            size: world.size,
            wdata_len: world.wdata.len(),
        });
    }
    Ok(())
}
