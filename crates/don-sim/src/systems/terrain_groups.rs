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

/// First unresolved deterministic dependency in `TerrainGroups::place_all`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PlaceAllError {
    /// Callsite `0x006a7330`, target `Mountains::randomize_mountains`
    /// `0x0089ca70`.  The callback advances the main RNG zero to three times based
    /// on three external linked lists, then mutates their cursors.
    MountainsRandomizerUnavailable,
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
    /// Before the first terrain-group selection draw, retail calls
    /// `Mountains::randomize_mountains` at `0x006a7330`.  That function consumes
    /// zero to three draws from the main RNG according to external mountain-range
    /// lists and rotates three shared list cursors.  Those lists are not yet part
    /// of the supported simulation state, so executing later group chance, clump,
    /// player, or region placement logic would start from an unproven RNG state.
    /// This boundary therefore returns before touching the caller's world, group
    /// state, or RNG.
    pub fn place_all(
        &mut self,
        _world: &mut World,
        _random: &mut Random,
        _progress: i32,
        _place_players: i32,
    ) -> Result<i32, PlaceAllError> {
        Err(PlaceAllError::MountainsRandomizerUnavailable)
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
