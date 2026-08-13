//! Exact read-only owner for `TerrainOut::find_tcoord_z`.
//!
//! Terrain height is deliberately absent from `don_sim::map_terrain::World`: the shipped
//! `TerrainOut::master_land_heights` float array is render/world-generation state and is
//! not walked by the World checksum.  It is nevertheless an input to every
//! `SubObject::init`, so a replay setup producer cannot flatten it without changing the
//! Builds and Units checksum images.
//!
//! This module accepts the completed height plane as explicit source authority, joins it
//! to the canonical World's dimensions and TData surface bits, and executes the complete
//! initialized-domain semantics of the 196-byte retail body.  It does not generate the
//! plane and never accepts a desired Z as input.

use std::fmt;

use don_sim::systems::map_terrain::{tflag, World};

/// `TerrainOut::find_tcoord_z(TCoord,TCoord,int)`.
pub const TERRAIN_FIND_TCOORD_Z_VA: u32 = 0x0085_44a0;
pub const TERRAIN_FIND_TCOORD_Z_BYTES: u32 = 196;
/// `GameAccessConst::find_tcoord_z(Coord,Coord,int)`, the public Coord wrapper.
pub const GAME_ACCESS_FIND_TCOORD_Z_VA: u32 = 0x0058_34b0;
/// Coord-overload wrapper on `TerrainOut`.
pub const TERRAIN_FIND_COORD_Z_VA: u32 = 0x0086_6710;

/// Exact supported-PE body identity for `0x008544a0..0x00854564`.
pub const TERRAIN_FIND_TCOORD_Z_SHA256: &str =
    "f9f2e5c9f818c640dd38e8ba9a055ead1eb03d029ac42a62139366c4c9dd94ef";

/// Where the height bytes came from.  None of these variants reconstructs worldgen; each
/// denotes a complete source-owned plane captured at a coherent runtime boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerrainHeightSource {
    CompletedWorldgen,
    RetailLiveSnapshot,
}

/// Explicit source authority for the non-checksummed `master_land_heights` array.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerrainHeightAuthority {
    /// Exact IEEE-754 words. Empty reproduces retail's uninitialized-array fallback.
    pub master_land_height_bits: Vec<u32>,
    /// Exact IEEE-754 word at global `land_height` `0x00cbe54c`.
    pub land_height_bits: u32,
    pub source: TerrainHeightSource,
    /// Identity of the complete worldgen or live snapshot which supplied the plane.
    pub source_digest: [u8; 32],
}

impl TerrainHeightAuthority {
    #[inline]
    pub fn is_initialized(&self) -> bool {
        !self.master_land_height_bits.is_empty()
    }

    /// Execute the exact TCoord overload against the canonical World surface owner.
    pub fn find_tcoord_z(
        &self,
        world: &World,
        tx: i32,
        ty: i32,
        zero_negative: i32,
    ) -> Result<TerrainTcoordZReceipt, TerrainHeightError> {
        if self.source_digest == [0; 32] {
            return Err(TerrainHeightError::MissingSourceIdentity);
        }

        if !self.is_initialized() {
            let source_bits = [self.land_height_bits, self.land_height_bits];
            let raw_z = cvttss2si(f32::from_bits(self.land_height_bits));
            return Ok(TerrainTcoordZReceipt {
                body_va: TERRAIN_FIND_TCOORD_Z_VA,
                source: self.source,
                source_digest: self.source_digest,
                tcoord: (tx, ty),
                zero_negative,
                height_indices: None,
                height_bits: Some(source_bits),
                water: false,
                uninitialized_fallback: true,
                raw_z: Some(raw_z),
                returned_z: raw_z,
            });
        }

        validate_world_shape(world)?;
        let expected_heights = grid_len(world.tile_xs, world.tile_ys)?;
        if self.master_land_height_bits.len() != expected_heights {
            return Err(TerrainHeightError::HeightPlaneLengthMismatch {
                expected: expected_heights,
                actual: self.master_land_height_bits.len(),
            });
        }
        if tx < 0 || ty < 0 || tx >= world.tile_xs || ty >= world.tile_ys {
            return Err(TerrainHeightError::TcoordOutsideHeightPlane {
                tx,
                ty,
                tile_xs: world.tile_xs,
                tile_ys: world.tile_ys,
            });
        }

        let tile_index = (ty as usize)
            .checked_mul(world.tile_xs as usize)
            .and_then(|row| row.checked_add(tx as usize))
            .ok_or(TerrainHeightError::ShapeOverflow)?;
        let water = world.tdata[tile_index] & tflag::SURFACE_MASK == tflag::SURFACE_WATER;
        // Native order matters: the surface branch at 0x008544f7 returns before either
        // height vertex is read.  Preserve that read set in the receipt as well as the
        // returned value.
        if water {
            return Ok(TerrainTcoordZReceipt {
                body_va: TERRAIN_FIND_TCOORD_Z_VA,
                source: self.source,
                source_digest: self.source_digest,
                tcoord: (tx, ty),
                zero_negative,
                height_indices: None,
                height_bits: None,
                water: true,
                uninitialized_fallback: false,
                raw_z: None,
                returned_z: 0,
            });
        }

        let stride = world.tile_xs as usize + 1;
        // Exact operands at 0x00854525 and 0x0085452a:
        //   h[(ty + 1) * (tile_xs + 1) + tx]
        // + h[ty * (tile_xs + 1) + tx + 1]
        let first = (ty as usize + 1)
            .checked_mul(stride)
            .and_then(|row| row.checked_add(tx as usize))
            .ok_or(TerrainHeightError::ShapeOverflow)?;
        let second = (ty as usize)
            .checked_mul(stride)
            .and_then(|row| row.checked_add(tx as usize + 1))
            .ok_or(TerrainHeightError::ShapeOverflow)?;
        let height_bits = [
            self.master_land_height_bits[first],
            self.master_land_height_bits[second],
        ];
        let average = (f32::from_bits(height_bits[0]) + f32::from_bits(height_bits[1])) * 0.5;
        let raw_z = cvttss2si(average);
        let returned_z = if raw_z < 0 && zero_negative == 1 {
            0
        } else {
            raw_z
        };

        Ok(TerrainTcoordZReceipt {
            body_va: TERRAIN_FIND_TCOORD_Z_VA,
            source: self.source,
            source_digest: self.source_digest,
            tcoord: (tx, ty),
            zero_negative,
            height_indices: Some((first, second)),
            height_bits: Some(height_bits),
            water,
            uninitialized_fallback: false,
            raw_z: Some(raw_z),
            returned_z,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerrainTcoordZReceipt {
    pub body_va: u32,
    pub source: TerrainHeightSource,
    pub source_digest: [u8; 32],
    pub tcoord: (i32, i32),
    pub zero_negative: i32,
    /// The exact diagonal height vertices read, or `None` when water returns first.
    pub height_indices: Option<(usize, usize)>,
    /// The exact source words read, including duplicated `land_height` in the fallback.
    pub height_bits: Option<[u32; 2]>,
    pub water: bool,
    pub uninitialized_fallback: bool,
    /// Result of SSE `cvttss2si`; absent when the water branch returns before conversion.
    pub raw_z: Option<i32>,
    pub returned_z: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerrainHeightError {
    MissingSourceIdentity,
    InvalidWorldShape,
    ShapeOverflow,
    HeightPlaneLengthMismatch {
        expected: usize,
        actual: usize,
    },
    TcoordOutsideHeightPlane {
        tx: i32,
        ty: i32,
        tile_xs: i32,
        tile_ys: i32,
    },
}

impl fmt::Display for TerrainHeightError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TerrainOut::find_tcoord_z refused: {self:?}")
    }
}

impl std::error::Error for TerrainHeightError {}

fn validate_world_shape(world: &World) -> Result<(), TerrainHeightError> {
    let tile_xs = world
        .xs
        .checked_mul(4)
        .ok_or(TerrainHeightError::ShapeOverflow)?;
    let tile_ys = world
        .ys
        .checked_mul(4)
        .ok_or(TerrainHeightError::ShapeOverflow)?;
    let tile_size = tile_xs
        .checked_mul(tile_ys)
        .ok_or(TerrainHeightError::ShapeOverflow)?;
    if world.xs <= 0
        || world.ys <= 0
        || world.tile_xs != tile_xs
        || world.tile_ys != tile_ys
        || world.tile_size != tile_size
        || world.tdata.len() != tile_size as usize
    {
        return Err(TerrainHeightError::InvalidWorldShape);
    }
    Ok(())
}

fn grid_len(tile_xs: i32, tile_ys: i32) -> Result<usize, TerrainHeightError> {
    let xs = usize::try_from(tile_xs).map_err(|_| TerrainHeightError::InvalidWorldShape)?;
    let ys = usize::try_from(tile_ys).map_err(|_| TerrainHeightError::InvalidWorldShape)?;
    xs.checked_add(1)
        .and_then(|x| ys.checked_add(1).and_then(|y| x.checked_mul(y)))
        .ok_or(TerrainHeightError::ShapeOverflow)
}

/// SSE `cvttss2si`: truncate toward zero, returning `0x80000000` on NaN/overflow.
fn cvttss2si(value: f32) -> i32 {
    if !value.is_finite() || !(-2_147_483_648.0..2_147_483_648.0).contains(&value) {
        i32::MIN
    } else {
        value.trunc() as i32
    }
}
