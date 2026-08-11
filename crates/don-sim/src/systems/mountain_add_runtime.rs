// SPDX-License-Identifier: GPL-3.0-or-later
//! Map-generation runtime for the map-style-12 path through
//! `Mountains::add_mountain` (`0x0089c2e0`).
//!
//! This module is deliberately host-shaped so it can be source-frozen without
//! editing the shared systems registry.  [`MountainWorld`] is the exact adapter
//! seam for `systems::map_terrain::World`; it does not permit an asserted
//! `Liberr` result.  The supported verification mode is mode 4, the
//! `Mountains::excluding_verify` path reached by Mediterranean/map-style 12
//! from `TerrainGroup::drop_tile`.
//!
//! Fidelity is Tier C (instruction-derived, not oracle-executed).  The complete
//! instruction and PDB ledger, including the deliberately unsupported modes,
//! is in `docs/assembly/mountains-add-mountain-runtime.md`.

/// `Mountains::add_mountain` in the supported executable.
pub const MOUNTAINS_ADD_MOUNTAIN_VA: u32 = 0x0089_c2e0;
/// PDB `S_GPROC32` size.  The body returns with `ret 0x24` (nine stack args).
pub const MOUNTAINS_ADD_MOUNTAIN_SIZE: u32 = 1_936;
/// Map-style 12 reaches the `excluding_verify` switch arm.
pub const EXCLUDING_VERIFY_MODE: i32 = 4;
/// Retail `Liberr` values returned by this body in the supported path.
pub const LIBERR_OK: i32 = 0;
pub const LIBERR_GENERAL: i32 = 1;

const MAX_CIRCLE_RADIUS: i32 = 64;
const CIRCLE_CAP: usize = 0x3248;
const WCOORD_TO_TCOORD: i32 = 4;
const WCOORD_TO_COORD: i32 = 0x300;

/// `WData::flags` bits read and written by the body.
pub mod wflag {
    pub const COAST: u16 = 0x0004;
    pub const ROCKS: u16 = 0x0008;
    pub const MOUNTAINS: u16 = 0x0010;
    pub const FOREST: u16 = 0x0020;
    pub const IMPASSABLE_X: u16 = 0x0040;
    pub const WATERHALF: u16 = 0x0100;
    pub const ORIG_COAST: u16 = 0x0400;
    pub const LAND_CLASS_MASK: u16 = 0x003c;
}

/// `TData` bits used by the common commit tail.
pub mod tflag {
    pub const BLOCKER_MASK: u16 = 0x0003;
    pub const BLOCKER_MOUNTAIN: u16 = 0x0002;
    pub const BEHIND_B: u16 = 0x0008;
}

/// One signed template-relative coordinate.  The same shape is used for the
/// `TCoord` and `WCoord` pairs in PDB `MountainRangeData`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct GridOffset {
    pub x: i32,
    pub y: i32,
}

impl GridOffset {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

/// Runtime product of the shipped displacement-template loader.
///
/// The PDB names the three pairs `mount_tx/mount_ty`, `mount_wx/mount_wy`, and
/// `solid_mount_wx/solid_mount_wy`.  Pairing them here makes a mismatched native
/// X/Y array impossible after the producer boundary.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MountainTemplateRuntime {
    pub mount_tiles: Vec<GridOffset>,
    pub mount_wcoords: Vec<GridOffset>,
    pub solid_mount_wcoords: Vec<GridOffset>,
}

/// Canonical nine-argument call after the native `this` pointer.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct AddMountainCall {
    pub template: i32,
    pub world_x: i32,
    pub world_y: i32,
    pub verification_mode: i32,
    pub mountain_space: i32,
    pub forest_space: i32,
    pub rock_space: i32,
    pub coast_space: i32,
    pub start_min: i32,
}

/// The 21-byte part of one WData row that this function needs is represented by
/// the two scalar fields it actually reads here.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct MountainWorldCell {
    pub flags: u16,
    pub land: i8,
}

/// Exact production adapter boundary.  The canonical implementation maps this
/// onto `systems::map_terrain::World`; tests can implement it on the real World
/// without registering this source-frozen module.
pub trait MountainWorld: Clone {
    fn world_xs(&self) -> i32;
    fn world_ys(&self) -> i32;
    fn tile_xs(&self) -> i32;
    fn tile_ys(&self) -> i32;
    fn world_cell(&self, wx: i32, wy: i32) -> MountainWorldCell;
    fn write_world_flags(&mut self, wx: i32, wy: i32, flags: u16);
    fn tile_mask(&self, tx: i32, ty: i32) -> u16;
    /// Exact `World::set_mountain_at(tx, ty, 1)` transaction: this must include
    /// `set_blocked_at`'s WData counters, road removal, and BAD_PATH ring.
    fn set_mountain_tile(&mut self, tx: i32, ty: i32);
    /// Exact `World::set_behind(tx, ty, 1, 1)` write.
    fn set_behind_b(&mut self, tx: i32, ty: i32);
    /// The native call receives two independent array objects. Keeping their
    /// lengths separate lets the adapter reject malformed retained state before
    /// `start_at` can index either one.
    fn start_x_count(&self) -> usize;
    fn start_y_count(&self) -> usize;
    fn start_at(&self, index: usize) -> (i32, i32);
    fn start_city_reserved(&self, wx: i32, wy: i32) -> bool;
}

/// Bit-exact storage for the `Vert3Array` location row without host float
/// canonicalisation.  Values are the SSE `cvtdq2ps` results written by retail.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct MountainLocationVertex {
    pub x_bits: u32,
    pub y_bits: u32,
    pub z_bits: u32,
}

/// The walked `Array<T>` / `SimpleArray<T>` metadata used by MountainsData.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetailMountainArray<T> {
    pub items: Vec<T>,
    pub capacity: i32,
    pub increment: i16,
    pub flags: u8,
}

impl<T> RetailMountainArray<T> {
    /// State installed by `MountainsData::MountainsData` `0x00433c40`.
    pub const fn empty() -> Self {
        Self {
            items: Vec::new(),
            capacity: 0,
            increment: -1,
            flags: 0,
        }
    }

    fn push_exact(&mut self, value: T) -> Result<(), MountainAddRuntimeError> {
        let length = i32::try_from(self.items.len())
            .map_err(|_| MountainAddRuntimeError::InvalidArrayMetadata)?;
        if length >= self.capacity {
            let increase = if self.increment < 0 {
                if self.capacity == 0 {
                    4
                } else {
                    self.capacity
                }
            } else {
                i32::from(self.increment)
            };
            if increase <= 0 {
                return Err(MountainAddRuntimeError::InvalidArrayMetadata);
            }
            self.capacity = self
                .capacity
                .checked_add(increase)
                .ok_or(MountainAddRuntimeError::InvalidArrayMetadata)?;
        }
        self.items.push(value);
        Ok(())
    }

    fn walk_header(&self, out: &mut Vec<u8>) {
        let length = self.items.len() as i32;
        out.extend_from_slice(&length.to_le_bytes());
        if length != 0 {
            out.extend_from_slice(&self.capacity.to_le_bytes());
            out.extend_from_slice(&self.increment.to_le_bytes());
            out.push(self.flags & !0x40);
        }
    }
}

impl<T> Default for RetailMountainArray<T> {
    fn default() -> Self {
        Self::empty()
    }
}

/// `MountainsData` state mutated or consulted by `add_mountain` mode 4.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MountainAddRuntime {
    /// `PtrArray<MountainRange>` by exact native template index.  `None` is a
    /// null native pointer, distinct from an empty geometry template.
    pub templates: Vec<Option<MountainTemplateRuntime>>,
    /// `MountainsData::verify_bits`, one LSB-first bit per WData cell.
    pub verify_bits: Vec<u8>,
    pub mountain_loc_wcoords_x: RetailMountainArray<i32>,
    pub mountain_loc_wcoords_y: RetailMountainArray<i32>,
    pub mountain_locs: RetailMountainArray<MountainLocationVertex>,
    pub mountain_types: RetailMountainArray<i32>,
}

impl MountainAddRuntime {
    pub fn new(world_cells: usize, templates: Vec<Option<MountainTemplateRuntime>>) -> Self {
        Self {
            templates,
            verify_bits: vec![0; world_cells.saturating_add(7) / 8],
            mountain_loc_wcoords_x: RetailMountainArray::empty(),
            mountain_loc_wcoords_y: RetailMountainArray::empty(),
            mountain_locs: RetailMountainArray::empty(),
            mountain_types: RetailMountainArray::empty(),
        }
    }

    /// Execute the exact map-style-12 path atomically.
    ///
    /// A retail verification rejection is `Ok` with `liberr == 1`; a missing
    /// producer or unsupported verification mode is a typed error.  Both leave
    /// the caller's World and MountainsData state unchanged.  A successful call
    /// returns `liberr == 0` and installs the staged state.  No RNG is read.
    pub fn apply_add_mountain<W: MountainWorld>(
        &mut self,
        world: &mut W,
        call: AddMountainCall,
    ) -> Result<MountainAddReceipt, MountainAddRuntimeError> {
        validate_call(self, world, call)?;

        let mut staged_runtime = self.clone();
        let mut staged_world = world.clone();
        let receipt = staged_runtime.apply_inner(&mut staged_world, call)?;
        if receipt.liberr == LIBERR_OK {
            *self = staged_runtime;
            *world = staged_world;
        }
        Ok(receipt)
    }

    /// Bytes walked by `Mountains::walk_data` `0x0089d320`, excluding the
    /// presentation-only field-name callback.  This includes capacity,
    /// increment, and masked flags whenever an array is non-empty.
    pub fn walked_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        walk_i32_array(&self.mountain_loc_wcoords_x, &mut out);
        walk_i32_array(&self.mountain_loc_wcoords_y, &mut out);
        self.mountain_locs.walk_header(&mut out);
        for v in &self.mountain_locs.items {
            out.extend_from_slice(&v.x_bits.to_le_bytes());
            out.extend_from_slice(&v.y_bits.to_le_bytes());
            out.extend_from_slice(&v.z_bits.to_le_bytes());
        }
        walk_i32_array(&self.mountain_types, &mut out);
        out
    }

    fn apply_inner<W: MountainWorld>(
        &mut self,
        world: &mut W,
        call: AddMountainCall,
    ) -> Result<MountainAddReceipt, MountainAddRuntimeError> {
        let template = self.templates[call.template as usize]
            .as_ref()
            .ok_or(MountainAddRuntimeError::MissingTemplate {
                template: call.template,
            })?
            .clone();
        let circles = CircleTable::build();
        let mut unique_verify_cells = 0usize;

        if let Some(reason) =
            self.excluding_verify(world, &template, call, &mut unique_verify_cells)
        {
            return Ok(MountainAddReceipt::rejected(
                call,
                reason,
                unique_verify_cells,
            ));
        }
        if let Some(reason) = quick_verify_template(world, &template, call, &circles) {
            return Ok(MountainAddReceipt::rejected(
                call,
                reason,
                unique_verify_cells,
            ));
        }
        if let Some(reason) = verify_spacing(world, &template, call, &circles) {
            return Ok(MountainAddReceipt::rejected(
                call,
                reason,
                unique_verify_cells,
            ));
        }

        for offset in &template.solid_mount_wcoords {
            let wx = call.world_x.wrapping_add(offset.x);
            let wy = call.world_y.wrapping_add(offset.y);
            let old = world.world_cell(wx, wy);
            let flags = (old.flags & !wflag::LAND_CLASS_MASK)
                | (old.flags & (wflag::COAST | wflag::ORIG_COAST))
                | wflag::MOUNTAINS;
            world.write_world_flags(wx, wy, flags);
        }

        for offset in &template.mount_tiles {
            let tx = call
                .world_x
                .wrapping_mul(WCOORD_TO_TCOORD)
                .wrapping_add(offset.x);
            let ty = call
                .world_y
                .wrapping_mul(WCOORD_TO_TCOORD)
                .wrapping_add(offset.y);
            world.set_mountain_tile(tx, ty);
        }

        // `set_behind_flags` walks the first three move-table entries: NW, N,
        // NE.  It skips both off-map tiles and tiles already stamped as a
        // mountain blocker.
        const BEHIND_DX: [i32; 3] = [-1, 0, 1];
        const BEHIND_DY: [i32; 3] = [-1, -1, -1];
        let mut behind_tiles_set = 0usize;
        for offset in &template.mount_tiles {
            let base_tx = call
                .world_x
                .wrapping_mul(WCOORD_TO_TCOORD)
                .wrapping_add(offset.x);
            let base_ty = call
                .world_y
                .wrapping_mul(WCOORD_TO_TCOORD)
                .wrapping_add(offset.y);
            for i in 0..3 {
                let tx = base_tx.wrapping_add(BEHIND_DX[i]);
                let ty = base_ty.wrapping_add(BEHIND_DY[i]);
                if valid_tile(world, tx, ty)
                    && world.tile_mask(tx, ty) & tflag::BLOCKER_MASK != tflag::BLOCKER_MOUNTAIN
                {
                    world.set_behind_b(tx, ty);
                    behind_tiles_set += 1;
                }
            }
        }

        self.mountain_loc_wcoords_x.push_exact(call.world_x)?;
        self.mountain_loc_wcoords_y.push_exact(call.world_y)?;
        let x = call.world_x.wrapping_mul(WCOORD_TO_COORD) as f32;
        let y = call.world_y.wrapping_mul(WCOORD_TO_COORD) as f32;
        self.mountain_locs.push_exact(MountainLocationVertex {
            x_bits: x.to_bits(),
            y_bits: y.to_bits(),
            z_bits: 0,
        })?;
        self.mountain_types.push_exact(call.template)?;

        Ok(MountainAddReceipt {
            call,
            liberr: LIBERR_OK,
            rejection: None,
            unique_verify_cells,
            mountain_wcoords_written: template.solid_mount_wcoords.len(),
            mountain_tiles_written: template.mount_tiles.len(),
            behind_tiles_set,
            retained_index: Some(self.mountain_types.items.len() - 1),
            rng_draws: 0,
        })
    }

    fn excluding_verify<W: MountainWorld>(
        &mut self,
        world: &W,
        template: &MountainTemplateRuntime,
        call: AddMountainCall,
        unique_verify_cells: &mut usize,
    ) -> Option<MountainRejection> {
        let mut touched = Vec::new();
        for (mount_index, offset) in template.mount_wcoords.iter().enumerate() {
            let wx = call.world_x.wrapping_add(offset.x);
            let wy = call.world_y.wrapping_add(offset.y);
            if !valid_world(world, wx, wy) {
                continue;
            }
            let index = (wy * world.world_xs() + wx) as usize;
            if bit_is_set(&self.verify_bits, index) {
                continue;
            }
            set_bit(&mut self.verify_bits, index);
            touched.push(index);
            *unique_verify_cells += 1;

            // `validate_call` proved the two independently walked native start
            // arrays have the same length, so this paired access is total.
            for player in 0..world.start_x_count() {
                let (sx, sy) = world.start_at(player);
                let distance = vector_dist(wx.wrapping_sub(sx), wy.wrapping_sub(sy));
                if distance < call.start_min {
                    clear_touched(&mut self.verify_bits, &touched);
                    return Some(MountainRejection::StartDistance {
                        mount_index,
                        player,
                        distance,
                    });
                }
            }
        }
        clear_touched(&mut self.verify_bits, &touched);
        None
    }
}

fn walk_i32_array(array: &RetailMountainArray<i32>, out: &mut Vec<u8>) {
    array.walk_header(out);
    for value in &array.items {
        out.extend_from_slice(&value.to_le_bytes());
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MountainAddReceipt {
    pub call: AddMountainCall,
    pub liberr: i32,
    pub rejection: Option<MountainRejection>,
    pub unique_verify_cells: usize,
    pub mountain_wcoords_written: usize,
    pub mountain_tiles_written: usize,
    pub behind_tiles_set: usize,
    pub retained_index: Option<usize>,
    pub rng_draws: u8,
}

impl MountainAddReceipt {
    fn rejected(
        call: AddMountainCall,
        rejection: MountainRejection,
        unique_verify_cells: usize,
    ) -> Self {
        Self {
            call,
            liberr: LIBERR_GENERAL,
            rejection: Some(rejection),
            unique_verify_cells,
            mountain_wcoords_written: 0,
            mountain_tiles_written: 0,
            behind_tiles_set: 0,
            retained_index: None,
            rng_draws: 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MountainRejection {
    StartDistance {
        mount_index: usize,
        player: usize,
        distance: i32,
    },
    FootprintOutOfBounds {
        mount_index: usize,
        wx: i32,
        wy: i32,
    },
    NotFlat {
        mount_index: usize,
        wx: i32,
        wy: i32,
        flags: u16,
        land: i8,
    },
    StartCity {
        mount_index: usize,
        circle_index: Option<usize>,
        wx: i32,
        wy: i32,
    },
    Spacing {
        mount_index: usize,
        kind: MountainSpacingKind,
        circle_index: usize,
        wx: i32,
        wy: i32,
        flags: u16,
    },
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MountainSpacingKind {
    Mountain,
    Coast,
    Forest,
    Rock,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MountainAddRuntimeError {
    UnsupportedVerificationMode {
        mode: i32,
    },
    MissingTemplate {
        template: i32,
    },
    InvalidWorldShape,
    StartArrayLengthMismatch {
        x_count: usize,
        y_count: usize,
    },
    InvalidVerifyBitShape {
        expected: usize,
        actual: usize,
    },
    InvalidSpacingRadius {
        kind: MountainSpacingKind,
        radius: i32,
    },
    InvalidArrayMetadata,
    TemplateTileOutOfBounds {
        template_index: usize,
        tx: i32,
        ty: i32,
    },
    TemplateSolidCellOutOfBounds {
        template_index: usize,
        wx: i32,
        wy: i32,
    },
}

fn validate_call<W: MountainWorld>(
    runtime: &MountainAddRuntime,
    world: &W,
    call: AddMountainCall,
) -> Result<(), MountainAddRuntimeError> {
    if call.verification_mode != EXCLUDING_VERIFY_MODE {
        return Err(MountainAddRuntimeError::UnsupportedVerificationMode {
            mode: call.verification_mode,
        });
    }
    if call.template < 0 || call.template as usize >= runtime.templates.len() {
        return Err(MountainAddRuntimeError::MissingTemplate {
            template: call.template,
        });
    }
    let Some(template) = runtime.templates[call.template as usize].as_ref() else {
        return Err(MountainAddRuntimeError::MissingTemplate {
            template: call.template,
        });
    };
    if world.world_xs() <= 0
        || world.world_ys() <= 0
        || world.tile_xs() != world.world_xs().wrapping_mul(WCOORD_TO_TCOORD)
        || world.tile_ys() != world.world_ys().wrapping_mul(WCOORD_TO_TCOORD)
    {
        return Err(MountainAddRuntimeError::InvalidWorldShape);
    }
    if world.start_x_count() != world.start_y_count() {
        return Err(MountainAddRuntimeError::StartArrayLengthMismatch {
            x_count: world.start_x_count(),
            y_count: world.start_y_count(),
        });
    }
    let cells = usize::try_from(world.world_xs())
        .ok()
        .and_then(|x| {
            usize::try_from(world.world_ys())
                .ok()
                .and_then(|y| x.checked_mul(y))
        })
        .ok_or(MountainAddRuntimeError::InvalidWorldShape)?;
    let expected_bits = cells.saturating_add(7) / 8;
    if runtime.verify_bits.len() != expected_bits {
        return Err(MountainAddRuntimeError::InvalidVerifyBitShape {
            expected: expected_bits,
            actual: runtime.verify_bits.len(),
        });
    }
    for (kind, radius) in [
        (MountainSpacingKind::Mountain, call.mountain_space),
        (MountainSpacingKind::Coast, call.coast_space),
        (MountainSpacingKind::Forest, call.forest_space),
        (MountainSpacingKind::Rock, call.rock_space),
    ] {
        if !(0..=MAX_CIRCLE_RADIUS).contains(&radius) {
            return Err(MountainAddRuntimeError::InvalidSpacingRadius { kind, radius });
        }
    }
    for (template_index, offset) in template.solid_mount_wcoords.iter().enumerate() {
        let wx = call.world_x.wrapping_add(offset.x);
        let wy = call.world_y.wrapping_add(offset.y);
        if !valid_world(world, wx, wy) {
            return Err(MountainAddRuntimeError::TemplateSolidCellOutOfBounds {
                template_index,
                wx,
                wy,
            });
        }
    }
    for (template_index, offset) in template.mount_tiles.iter().enumerate() {
        let tx = call
            .world_x
            .wrapping_mul(WCOORD_TO_TCOORD)
            .wrapping_add(offset.x);
        let ty = call
            .world_y
            .wrapping_mul(WCOORD_TO_TCOORD)
            .wrapping_add(offset.y);
        if !valid_tile(world, tx, ty) {
            return Err(MountainAddRuntimeError::TemplateTileOutOfBounds {
                template_index,
                tx,
                ty,
            });
        }
    }
    Ok(())
}

fn quick_verify_template<W: MountainWorld>(
    world: &W,
    template: &MountainTemplateRuntime,
    call: AddMountainCall,
    circles: &CircleTable,
) -> Option<MountainRejection> {
    for (mount_index, offset) in template.mount_wcoords.iter().enumerate() {
        let wx = call.world_x.wrapping_add(offset.x);
        let wy = call.world_y.wrapping_add(offset.y);
        if !valid_world(world, wx, wy) {
            return Some(MountainRejection::FootprintOutOfBounds {
                mount_index,
                wx,
                wy,
            });
        }
        let cell = world.world_cell(wx, wy);
        let ocean = cell.flags & wflag::WATERHALF == 0 && matches!(cell.land, 1 | 2);
        if cell.flags & (wflag::MOUNTAINS | wflag::FOREST) != 0
            || ocean
            || cell.flags & (wflag::ROCKS | wflag::IMPASSABLE_X) != 0
        {
            return Some(MountainRejection::NotFlat {
                mount_index,
                wx,
                wy,
                flags: cell.flags,
                land: cell.land,
            });
        }
        if world.start_city_reserved(wx, wy) {
            return Some(MountainRejection::StartCity {
                mount_index,
                circle_index: None,
                wx,
                wy,
            });
        }
        for circle_index in circles.radius[0] as usize..circles.radius[1] as usize {
            let nx = wx.wrapping_add(i32::from(circles.x[circle_index]));
            let ny = wy.wrapping_add(i32::from(circles.y[circle_index]));
            if !valid_world(world, nx, ny) {
                return Some(MountainRejection::FootprintOutOfBounds {
                    mount_index,
                    wx: nx,
                    wy: ny,
                });
            }
            if world.start_city_reserved(nx, ny) {
                return Some(MountainRejection::StartCity {
                    mount_index,
                    circle_index: Some(circle_index),
                    wx: nx,
                    wy: ny,
                });
            }
        }
    }
    None
}

fn verify_spacing<W: MountainWorld>(
    world: &W,
    template: &MountainTemplateRuntime,
    call: AddMountainCall,
    circles: &CircleTable,
) -> Option<MountainRejection> {
    for (mount_index, offset) in template.mount_wcoords.iter().enumerate() {
        let base_x = call.world_x.wrapping_add(offset.x);
        let base_y = call.world_y.wrapping_add(offset.y);
        for (kind, radius, mask) in [
            (
                MountainSpacingKind::Mountain,
                call.mountain_space,
                wflag::MOUNTAINS | wflag::IMPASSABLE_X,
            ),
            (MountainSpacingKind::Coast, call.coast_space, wflag::COAST),
            (
                MountainSpacingKind::Forest,
                call.forest_space,
                wflag::FOREST,
            ),
            (MountainSpacingKind::Rock, call.rock_space, wflag::ROCKS),
        ] {
            for circle_index in circles.radius[0] as usize..circles.radius[radius as usize] as usize
            {
                let wx = base_x.wrapping_add(i32::from(circles.x[circle_index]));
                let wy = base_y.wrapping_add(i32::from(circles.y[circle_index]));
                if !valid_world(world, wx, wy) {
                    continue;
                }
                let flags = world.world_cell(wx, wy).flags;
                if flags & mask != 0 {
                    return Some(MountainRejection::Spacing {
                        mount_index,
                        kind,
                        circle_index,
                        wx,
                        wy,
                        flags,
                    });
                }
            }
        }
    }
    None
}

#[inline]
fn valid_world<W: MountainWorld>(world: &W, wx: i32, wy: i32) -> bool {
    wx >= 0 && wy >= 0 && wx < world.world_xs() && wy < world.world_ys()
}

#[inline]
fn valid_tile<W: MountainWorld>(world: &W, tx: i32, ty: i32) -> bool {
    tx >= 0 && ty >= 0 && tx < world.tile_xs() && ty < world.tile_ys()
}

#[inline]
fn bit_is_set(bits: &[u8], index: usize) -> bool {
    bits[index >> 3] & (1u8 << (index & 7)) != 0
}

#[inline]
fn set_bit(bits: &mut [u8], index: usize) {
    bits[index >> 3] |= 1u8 << (index & 7);
}

#[inline]
fn clear_bit(bits: &mut [u8], index: usize) {
    bits[index >> 3] &= !(1u8 << (index & 7));
}

fn clear_touched(bits: &mut [u8], touched: &[usize]) {
    for &index in touched {
        clear_bit(bits, index);
    }
}

/// `vector_dist` `0x0046cff0`, including the >=60000 overflow guard.
fn vector_dist(dx: i32, dy: i32) -> i32 {
    let a = dx.wrapping_abs();
    let b = dy.wrapping_abs();
    let (hi, lo) = if a > b { (a, b) } else { (b, a) };
    if hi == 0 {
        0
    } else if lo < 60_000 {
        let numerator = (lo as u32).wrapping_mul(lo as u32);
        let denominator = (hi as u32).wrapping_mul(2);
        hi.wrapping_add((numerator / denominator) as i32)
    } else {
        ((lo as u32).wrapping_add((hi as u32).wrapping_mul(2)) >> 1) as i32
    }
}

/// Local regeneration of `circle_x`, `circle_y`, and `circle_radius` from
/// `circle_init` `0x006817f0`.  The body consumes only radii 0..=64.
struct CircleTable {
    x: Vec<i8>,
    y: Vec<i8>,
    radius: [i32; MAX_CIRCLE_RADIUS as usize + 1],
}

impl CircleTable {
    fn build() -> Self {
        let mut x = Vec::with_capacity(13_000);
        let mut y = Vec::with_capacity(13_000);
        let mut radius = [0; MAX_CIRCLE_RADIUS as usize + 1];
        let mut lo = 0i32;
        for r in 0..=MAX_CIRCLE_RADIUS {
            let mut ox = lo;
            while ox <= r {
                let mut oy = lo;
                while oy <= r {
                    if vector_dist(ox, oy) == r {
                        x.push(ox as i8);
                        y.push(oy as i8);
                        if x.len() > CIRCLE_CAP {
                            for slot in radius.iter_mut().skip(r as usize) {
                                *slot = x.len() as i32;
                            }
                            return Self { x, y, radius };
                        }
                    }
                    oy += 1;
                }
                ox += 1;
            }
            radius[r as usize] = x.len() as i32;
            lo -= 1;
        }
        Self { x, y, radius }
    }
}
