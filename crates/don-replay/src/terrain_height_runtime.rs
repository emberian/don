//! Exact read-only owner for `TerrainOut::find_tcoord_z`.
//!
//! Terrain height is deliberately absent from `don_sim::map_terrain::World`: the shipped
//! `TerrainOut::master_land_heights` float array is render/world-generation state and is
//! not walked by the World checksum.  It is nevertheless an input to every
//! `SubObject::init`, so a replay setup producer cannot flatten it without changing the
//! Builds and Units checksum images.
//!
//! This module accepts a completed height plane as explicit source authority and separately
//! assembles the exact pre-mountain plane from bounded completed-worldgen intermediates. The
//! latter path executes the shipped `generate_land` height/color slice, including vertex-code
//! reads, coast-distance blending, and the native smoothing passes. It accepts neither height
//! floats nor a desired Z, and it is intentionally unable to answer queries until the later
//! `adjust_for_mountains` pass is installed. Only a complete authority joins the canonical
//! World's dimensions and TData to the 196-byte retail query body.

use std::fmt;

use don_sim::systems::map_terrain::{tflag, World};
use don_sim::systems::mountain_add_runtime::MountainAddRuntime;
use don_sim::systems::mountain_template_producer::{
    MountainTemplateCatalog, MOUNTAIN_RANGE_INIT_SHA256, MOUNTAIN_RANGE_INIT_VA,
};

use crate::fractal_boundary::{generate_retail_fractal, RetailFractalPlane};
use crate::initial::InitialState;
use crate::world_owner_frontier::sha256;

/// `TerrainOut::find_tcoord_z(TCoord,TCoord,int)`.
pub const TERRAIN_FIND_TCOORD_Z_VA: u32 = 0x0085_44a0;
pub const TERRAIN_FIND_TCOORD_Z_BYTES: u32 = 196;
/// `GameAccessConst::find_tcoord_z(Coord,Coord,int)`, the public Coord wrapper.
pub const GAME_ACCESS_FIND_TCOORD_Z_VA: u32 = 0x0058_34b0;
/// Coord-overload wrapper on `TerrainOut`.
pub const TERRAIN_FIND_COORD_Z_VA: u32 = 0x0086_6710;

/// Height-plane owner and exact supporting leaves used by normal `Terrain::init`.
pub const TERRAIN_GENERATE_LAND_VA: u32 = 0x0085_f8d0;
pub const TERRAIN_GENERATE_LAND_BYTES: u32 = 2_789;
pub const TERRAIN_GET_VERT_CODES_VA: u32 = 0x0086_bf70;
pub const TERRAIN_GET_VERT_CODES_BYTES: u32 = 609;
pub const TERRAIN_DETERMINE_LAND_HEIGHT_COLOR_VA: u32 = 0x0086_c1e0;
pub const TERRAIN_DETERMINE_LAND_HEIGHT_COLOR_BYTES: u32 = 434;
pub const TERRAIN_SMOOTH_TCOORD_VA: u32 = 0x0086_c3a0;
pub const TERRAIN_SMOOTH_TCOORD_BYTES: u32 = 410;
pub const TERRAIN_FIND_CLOSEST_COORDINFO_VA: u32 = 0x0086_a260;
pub const TERRAIN_FIND_CLOSEST_COORDINFO_BYTES: u32 = 664;
pub const TERRAIN_ADJUST_FOR_MOUNTAINS_VA: u32 = 0x0087_03c0;
pub const TERRAIN_ADJUST_FOR_MOUNTAINS_BYTES: u32 = 3_305;
pub const TERRAIN_FILL_MOUNTAIN_DATA_VA: u32 = 0x0086_9380;
pub const TERRAIN_FILL_MOUNTAIN_DATA_BYTES: u32 = 1_030;
pub const FRACTAL_GET_HEIGHT_VA: u32 = 0x006a_a870;
pub const FRACTAL_GET_HEIGHT_BYTES: u32 = 360;
/// `TerrainOut::refresh_data`, which initializes the two height Fractals.
pub const TERRAIN_REFRESH_DATA_VA: u32 = 0x0087_0050;
pub const TERRAIN_REFRESH_DATA_BYTES: u32 = 867;
pub const TERRAIN_HEIGHT_FRACTAL_INIT_CALL_VA: u32 = 0x0087_0275;
pub const TERRAIN_HEIGHT_DETAIL_FRACTAL_INIT_CALL_VA: u32 = 0x0087_02a8;
pub const FRACTAL_INIT_VA: u32 = 0x006a_a2d0;
pub const FRACTAL_INIT_BYTES: u32 = 1_428;

/// Exact supported-PE body identity for `0x008544a0..0x00854564`.
pub const TERRAIN_FIND_TCOORD_Z_SHA256: &str =
    "f9f2e5c9f818c640dd38e8ba9a055ead1eb03d029ac42a62139366c4c9dd94ef";
pub const TERRAIN_GENERATE_LAND_SHA256: &str =
    "5cc1f8e243fcf7556492ca1215df4785a5bd810c3550c7a5a0cb27b79e38445a";
pub const TERRAIN_GET_VERT_CODES_SHA256: &str =
    "8eee1044d69671f26222f6801813dbd9e72547a6f9dca4a7b9c4cfbfedaf6695";
pub const TERRAIN_DETERMINE_LAND_HEIGHT_COLOR_SHA256: &str =
    "720e687b621b9fb72acb922e6097956462adb0fe31b59d4afd589cae57a1eb68";
pub const TERRAIN_SMOOTH_TCOORD_SHA256: &str =
    "7f8898b51371d97cd937b44c8a2a40c3b192e6d739f3f7faf935bb117852b8b9";
pub const TERRAIN_FIND_CLOSEST_COORDINFO_SHA256: &str =
    "7968498a530c49764f8b0a2b6c2453a2384d807e1cde2b2090182add5fba75b2";
pub const TERRAIN_ADJUST_FOR_MOUNTAINS_SHA256: &str =
    "4aea3b9267fa1a3bc09b3bcd49cd52e9e4b326f6cf6edc42ae754fdfe5edda5e";
pub const TERRAIN_FILL_MOUNTAIN_DATA_SHA256: &str =
    "8a85b4f8a9beeff18e562523fbfec5d626c1035e5541c225cad9972732a71cd0";
pub const FRACTAL_GET_HEIGHT_SHA256: &str =
    "93057be843aa676b22710c7b79d22861e052c889fbcc2647aaea061dde4dbe02";
pub const TERRAIN_REFRESH_DATA_SHA256: &str =
    "6b97b38457dfc025efe5f050cc37b4123be39fd4fe25d9ccf56a6bceb02d7ec5";
pub const FRACTAL_INIT_SHA256: &str =
    "44e3a9c196de3c8be8291398dd6608976285fdffb3937180bf697b16ba380546";

/// Retail normal terrain is four render vertices per WCoord.  The height query hardcodes
/// that same factor at `0x0085450e`/`0x00854517`.
pub const TERRAIN_TESSELATION_LEVEL: i32 = 4;

/// Exact initialized state read by `Fractal::get_height(int,int)`.
///
/// `frac_columns` is the native outer-X/inner-Y `(xs+1)*(ys+1)` byte storage. It is upstream
/// of all render-resolution samples: callers cannot supply the sample grids consumed by
/// `generate_land`, let alone height words or Z values. Reconstructing this initialized state
/// from the map RNG and `Fractal::init` is the remaining upstream fractal residual.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerrainFractalAuthority {
    pub frac_columns: Vec<u8>,
    pub xs: i32,
    pub ys: i32,
    pub flags: i32,
    pub partitions: [i32; 16],
    pub random_seed: u32,
    pub x_inc_bits: u64,
    pub y_inc_bits: u64,
    pub initialized_source_digest: [u8; 32],
}

/// Exact completed-worldgen inputs consumed by the height-only `generate_land` slice.
///
/// Both initialized Fractals are sampled by the shipped body. `coord_info_flags` is the
/// WCoord-resolution `CoordInfo::flags` grid created by `generate_land_lists`. Reconstructing
/// the Fractal states and those flags from map seed and canonical World is the explicit
/// upstream residual; this boundary never admits sampled grids, SVX Z, or height-plane words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerrainHeightWorldgenInputs {
    pub height_fractal: TerrainFractalAuthority,
    pub height_fractal_detail: TerrainFractalAuthority,
    pub coord_info_flags: Vec<u16>,
    /// Exact IEEE-754 global `land_height` word (`0x00cbe54c`).
    pub land_height_bits: u32,
    /// Exact IEEE-754 `TerrainOut+0x5630` mountain-height word.
    pub mountain_height_bits: u32,
    /// Exact IEEE-754 global height scale word (`0x00c0629c`).
    pub height_scale_bits: u32,
    /// Nonzero identity of the `generate_land_lists` CoordInfo production boundary.
    pub coord_info_source_digest: [u8; 32],
}

/// Non-Fractal completed-worldgen sources retained while `CoordInfo` and installed
/// height scalars remain separate exact producers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerrainHeightNonFractalInputs {
    pub coord_info_flags: Vec<u16>,
    pub land_height_bits: u32,
    pub mountain_height_bits: u32,
    pub height_scale_bits: u32,
    pub coord_info_source_digest: [u8; 32],
}

/// Evidence for the two `Fractal::init` calls in `TerrainOut::refresh_data`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerrainHeightRefreshFractalReceipt {
    pub refresh_data_va: u32,
    pub fractal_init_va: u32,
    pub height_call_va: u32,
    pub detail_call_va: u32,
    pub replay_payload_sha256: [u8; 32],
    pub world_seed: i32,
    pub xs: i32,
    pub ys: i32,
    pub semaphore_821: u8,
    pub check_victory_mode: bool,
    pub height_requested_smooth: i32,
    pub detail_requested_smooth: i32,
    pub height_seed: u32,
    pub detail_seed: u32,
    pub height_random_draws: u32,
    pub detail_random_draws: u32,
    pub height_random_state_after: i32,
    pub detail_random_state_after: i32,
    pub height_authority_digest: [u8; 32],
    pub detail_authority_digest: [u8; 32],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerrainHeightWorldgenReceipt {
    pub generate_land_va: u32,
    pub fractal_get_height_va: u32,
    pub get_vert_codes_va: u32,
    pub determine_land_height_color_va: u32,
    pub find_closest_coordinfo_va: u32,
    pub smooth_tcoord_va: u32,
    pub completed_worldgen_digest: [u8; 32],
    pub height_fractal_digest: [u8; 32],
    pub height_fractal_detail_digest: [u8; 32],
    pub derived_plane_digest: [u8; 32],
    pub vertices: usize,
    pub locked_zero_vertices: usize,
    pub coordinfo_zero_vertices: usize,
    pub mountain_vertices: usize,
    pub fixed_height_vertices: usize,
    pub fractal_vertices: usize,
    pub smoothing_vertices: usize,
    pub smoothing_passes: usize,
    /// Normal `Terrain::init` still has a later height-writing mountain pass.
    pub final_query_authority: bool,
    pub remaining_adjust_for_mountains_va: u32,
    pub remaining_fill_mountain_data_va: u32,
}

/// Exact output of `generate_land`'s height slice before normal `Terrain::init` calls
/// `adjust_for_mountains`. This is deliberately not a [`TerrainHeightAuthority`]:
/// `fill_mountain_data` can still add to height words at `0x008695f9`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerrainHeightPreMountainPlane {
    pub master_land_height_bits: Vec<u32>,
    pub land_height_bits: u32,
    pub source_digest: [u8; 32],
}

/// Evidence for the new-map `adjust_for_mountains(arg7=0)` height transaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerrainMountainHeightReceipt {
    pub adjust_for_mountains_va: u32,
    pub fill_mountain_data_va: u32,
    pub mountain_range_init_va: u32,
    pub mountain_range_init_sha256: &'static str,
    pub pre_mountain_digest: [u8; 32],
    pub catalog_digest: [u8; 32],
    pub placement_digest: [u8; 32],
    pub final_plane_digest: [u8; 32],
    pub placements: usize,
    pub source_vertices: usize,
    pub matched_vertices: usize,
    pub unmatched_vertices: usize,
    /// This producer is exclusively the new-map height-adding call (`arg7=0`).
    pub load_rebuild_mode: bool,
    pub final_query_authority: bool,
}

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
        find_tcoord_z_from_authority(self, world, tx, ty, zero_negative)
    }
}

impl TerrainFractalAuthority {
    /// Execute `Fractal::get_height(int,int)` against an initialized, source-identified grid.
    pub fn get_height(&self, x: i32, y: i32) -> Result<u8, TerrainHeightError> {
        validate_fractal_authority(self)?;
        fractal_get_height(self, x, y)
    }
}

impl TerrainHeightWorldgenInputs {
    /// Reconstruct both initialized height Fractals from the replay-carried seed and
    /// complete Game semaphore captured before setup.
    ///
    /// Retail reseeds each Fractal's private RNG from `World::seed`; this does not
    /// consume or depend on the map-generation RNG handoff. The only call-shape branch
    /// is Game semaphore bit 9, read from `Game+0x821 & 2`.
    pub fn from_refresh_data(
        world: &World,
        initial: &InitialState,
        remaining: TerrainHeightNonFractalInputs,
    ) -> Result<(Self, TerrainHeightRefreshFractalReceipt), TerrainHeightError> {
        validate_world_shape(world)?;
        if initial.payload_sha256 == [0; 32] {
            return Err(TerrainHeightError::MissingSourceIdentity);
        }
        if initial.info.seed as i32 != world.seed {
            return Err(TerrainHeightError::ReplayWorldSeedMismatch {
                replay_seed: initial.info.seed,
                world_seed: world.seed,
            });
        }
        let semaphore_821 =
            *initial
                .game
                .semaphore
                .get(1)
                .ok_or(TerrainHeightError::GameSemaphoreTooShort {
                    expected: 2,
                    actual: initial.game.semaphore.len(),
                })?;
        derive_refresh_worldgen_inputs(world, initial.payload_sha256, semaphore_821, remaining)
    }
}

impl TerrainHeightPreMountainPlane {
    /// Execute the exact normal-init height-producing slice of `generate_land`.
    ///
    /// The result cannot answer height queries until `adjust_for_mountains` is executed.
    pub fn from_completed_worldgen(
        world: &World,
        inputs: &TerrainHeightWorldgenInputs,
    ) -> Result<(Self, TerrainHeightWorldgenReceipt), TerrainHeightError> {
        validate_world_shape(world)?;
        if inputs.coord_info_source_digest == [0; 32] {
            return Err(TerrainHeightError::MissingSourceIdentity);
        }
        let expected_vertices = grid_len(world.tile_xs, world.tile_ys)?;
        if inputs.coord_info_flags.len() != world.size as usize {
            return Err(TerrainHeightError::CoordInfoFlagsLengthMismatch {
                expected: world.size as usize,
                actual: inputs.coord_info_flags.len(),
            });
        }

        let height_fractal_digest = validate_fractal_authority(&inputs.height_fractal)?;
        let height_fractal_detail_digest =
            validate_fractal_authority(&inputs.height_fractal_detail)?;
        let width = world.tile_xs as usize + 1;
        let height = world.tile_ys as usize + 1;
        let mut height_fractal_samples = Vec::with_capacity(expected_vertices);
        let mut height_fractal_detail_samples = Vec::with_capacity(expected_vertices);
        for y in 0..height {
            for x in 0..width {
                height_fractal_samples.push(fractal_get_height(
                    &inputs.height_fractal,
                    x as i32,
                    y as i32,
                )?);
                height_fractal_detail_samples.push(fractal_get_height(
                    &inputs.height_fractal_detail,
                    x as i32,
                    y as i32,
                )?);
            }
        }
        let completed_worldgen_digest = completed_worldgen_digest(
            world,
            inputs,
            height_fractal_digest,
            height_fractal_detail_digest,
        );

        let land_height = f32::from_bits(inputs.land_height_bits);
        let mountain_height = f32::from_bits(inputs.mountain_height_bits);
        let height_scale = f32::from_bits(inputs.height_scale_bits);
        let mut heights = vec![inputs.land_height_bits; expected_vertices];
        let mut smoothing_coords = Vec::new();
        let mut locked_zero_vertices = 0;
        let mut coordinfo_zero_vertices = 0;
        let mut mountain_vertices = 0;
        let mut fixed_height_vertices = 0;
        let mut fractal_vertices = 0;

        for y in 0..height {
            for x in 0..width {
                let index = y * width + x;
                let tx = x as i32;
                let ty = y as i32;
                if get_tdata_vertex_codes(world, tx, ty) & 0x10 != 0 {
                    heights[index] = 0;
                    locked_zero_vertices += 1;
                    continue;
                }

                let codes = get_coordinfo_vertex_codes(world, &inputs.coord_info_flags, tx, ty);
                if codes & 0x1000 != 0 {
                    heights[index] = ((mountain_height + land_height) * height_scale).to_bits();
                    mountain_vertices += 1;
                } else if codes & 0x4 != 0 {
                    heights[index] = 0;
                    coordinfo_zero_vertices += 1;
                } else if codes & 0x2 != 0 {
                    heights[index] = 100.0f32.to_bits();
                    fixed_height_vertices += 1;
                } else {
                    // 0x0086c297..0x0086c38f. Keep the binary32 operation order explicit.
                    let coarse = height_fractal_samples[index] as f32;
                    let detail = height_fractal_detail_samples[index] as f32;
                    let coarse_height = coarse * 7.5;
                    let base_height = coarse_height + land_height + detail * 1.875;
                    let coast = find_closest_coordinfo(world, &inputs.coord_info_flags, tx, ty);
                    let result = if coast >= 0.0 {
                        let coast_height = coast * 0.05 * 7.5 * 192.0 + land_height;
                        let coast_weight = 4.0 - coast;
                        let blended_coast = coast_height * coast_weight * 0.25;
                        let blended_base = base_height * coast * 0.25;
                        // 0x0086c360..0x0086c366 compares both coordinates with the same
                        // X-derived `tesselation * world_xs` bound. Preserve that shipped
                        // rectangular-map quirk rather than substituting `tile_ys` here.
                        if tx < world.tile_xs && ty < world.tile_xs {
                            smoothing_coords.push((x, y));
                        }
                        blended_base + blended_coast
                    } else {
                        base_height
                    };
                    heights[index] = result.to_bits();
                    fractal_vertices += 1;
                }
            }
        }

        let mut smoothing_passes = 0;
        for &(x, y) in &smoothing_coords {
            smooth_tcoord(&mut heights, width, height, x, y);
            smoothing_passes += 1;
            if (x | y) & 1 != 0 {
                smooth_tcoord(&mut heights, width, height, x, y);
                smoothing_passes += 1;
            }
        }

        let derived_plane_digest = worldgen_plane_digest(
            world,
            inputs,
            completed_worldgen_digest,
            &height_fractal_samples,
            &height_fractal_detail_samples,
            &heights,
        );
        let plane = TerrainHeightPreMountainPlane {
            master_land_height_bits: heights,
            land_height_bits: inputs.land_height_bits,
            source_digest: derived_plane_digest,
        };
        let receipt = TerrainHeightWorldgenReceipt {
            generate_land_va: TERRAIN_GENERATE_LAND_VA,
            fractal_get_height_va: FRACTAL_GET_HEIGHT_VA,
            get_vert_codes_va: TERRAIN_GET_VERT_CODES_VA,
            determine_land_height_color_va: TERRAIN_DETERMINE_LAND_HEIGHT_COLOR_VA,
            find_closest_coordinfo_va: TERRAIN_FIND_CLOSEST_COORDINFO_VA,
            smooth_tcoord_va: TERRAIN_SMOOTH_TCOORD_VA,
            completed_worldgen_digest,
            height_fractal_digest,
            height_fractal_detail_digest,
            derived_plane_digest,
            vertices: expected_vertices,
            locked_zero_vertices,
            coordinfo_zero_vertices,
            mountain_vertices,
            fixed_height_vertices,
            fractal_vertices,
            smoothing_vertices: smoothing_coords.len(),
            smoothing_passes,
            final_query_authority: false,
            remaining_adjust_for_mountains_va: TERRAIN_ADJUST_FOR_MOUNTAINS_VA,
            remaining_fill_mountain_data_va: TERRAIN_FILL_MOUNTAIN_DATA_VA,
        };
        Ok((plane, receipt))
    }

    /// Execute the height-writing half of new-map `adjust_for_mountains`.
    ///
    /// The plane is consumed so the source-derived displacement cannot be applied
    /// twice through this API. Installed template vertices and retained placement
    /// rows are joined by retail template index and placement ordinal; no replay Z
    /// value or desired output height is accepted.
    pub fn finish_new_map_mountains(
        self,
        world: &World,
        catalog: &MountainTemplateCatalog,
        mountains: &MountainAddRuntime,
    ) -> Result<(TerrainHeightAuthority, TerrainMountainHeightReceipt), TerrainHeightError> {
        validate_world_shape(world)?;
        if self.source_digest == [0; 32] {
            return Err(TerrainHeightError::MissingSourceIdentity);
        }
        let expected_heights = grid_len(world.tile_xs, world.tile_ys)?;
        if self.master_land_height_bits.len() != expected_heights {
            return Err(TerrainHeightError::HeightPlaneLengthMismatch {
                expected: expected_heights,
                actual: self.master_land_height_bits.len(),
            });
        }

        let template_count = catalog.sources.len();
        if template_count == 0
            || catalog.displacement_tgas.len() != template_count
            || catalog.templates.len() != template_count
            || catalog.tcoord_vertices.len() != template_count
        {
            return Err(TerrainHeightError::MountainCatalogShapeMismatch {
                sources: template_count,
                displacement_tgas: catalog.displacement_tgas.len(),
                templates: catalog.templates.len(),
                tcoord_vertices: catalog.tcoord_vertices.len(),
            });
        }
        if mountains.templates.len() != template_count
            || mountains
                .templates
                .iter()
                .zip(&catalog.templates)
                .any(|(installed, source)| installed.as_ref() != Some(source))
        {
            return Err(TerrainHeightError::MountainRuntimeCatalogMismatch);
        }

        let placements = mountains.mountain_locs.items.len();
        let array_lengths = [
            mountains.mountain_loc_wcoords_x.items.len(),
            mountains.mountain_loc_wcoords_y.items.len(),
            mountains.mountain_types.items.len(),
        ];
        if array_lengths.into_iter().any(|length| length != placements) {
            return Err(TerrainHeightError::MountainPlacementLengthMismatch {
                locations: placements,
                world_x: array_lengths[0],
                world_y: array_lengths[1],
                types: array_lengths[2],
            });
        }

        let catalog_digest = mountain_catalog_digest(catalog);
        let placement_digest = sha256(&mountains.walked_bytes());
        let mut heights = self.master_land_height_bits;
        let stride = world.tile_xs as usize + 1;
        let mut source_vertices = 0usize;
        let mut matched_vertices = 0usize;

        for placement in 0..placements {
            let template = mountains.mountain_types.items[placement];
            let template_index = usize::try_from(template)
                .ok()
                .filter(|&index| index < template_count)
                .ok_or(TerrainHeightError::MountainTemplateIndexOutsideCatalog {
                    placement,
                    template,
                    templates: template_count,
                })?;
            let world_x = mountains.mountain_loc_wcoords_x.items[placement];
            let world_y = mountains.mountain_loc_wcoords_y.items[placement];
            let location = mountains.mountain_locs.items[placement];
            let expected_x = world_x.wrapping_mul(0x300) as f32;
            let expected_y = world_y.wrapping_mul(0x300) as f32;
            if location.x_bits != expected_x.to_bits()
                || location.y_bits != expected_y.to_bits()
                || location.z_bits != 0
            {
                return Err(TerrainHeightError::MountainPlacementLocationMismatch {
                    placement,
                    world_x,
                    world_y,
                    location: [location.x_bits, location.y_bits, location.z_bits],
                });
            }

            for vertex in &catalog.tcoord_vertices[template_index] {
                source_vertices += 1;
                // `fill_mountain_data` adds retained placement X/Y with scalar
                // `addss`, then compares each component against the 192-unit
                // terrain lattice with a strict `abs(delta) < 0.01f` gate.
                let translated_x = f32::from_bits(vertex.x_bits) + f32::from_bits(location.x_bits);
                let translated_y = f32::from_bits(vertex.y_bits) + f32::from_bits(location.y_bits);
                let candidate_x = cvttss2si(translated_x / 192.0f32);
                let candidate_y = cvttss2si(translated_y / 192.0f32);
                let lattice_x = candidate_x as f32 * 192.0f32;
                let lattice_y = candidate_y as f32 * 192.0f32;
                if (translated_x - lattice_x).abs() >= 0.01f32
                    || (translated_y - lattice_y).abs() >= 0.01f32
                    || candidate_x < 0
                    || candidate_y < 0
                    || candidate_x > world.tile_xs
                    || candidate_y > world.tile_ys
                {
                    continue;
                }
                let index = candidate_y as usize * stride + candidate_x as usize;
                let adjusted = f32::from_bits(heights[index]) + f32::from_bits(vertex.z_bits);
                heights[index] = adjusted.to_bits();
                matched_vertices += 1;
            }
        }

        let final_plane_digest = mountain_height_plane_digest(
            self.source_digest,
            catalog_digest,
            placement_digest,
            world,
            &heights,
        );
        let authority = TerrainHeightAuthority {
            master_land_height_bits: heights,
            land_height_bits: self.land_height_bits,
            source: TerrainHeightSource::CompletedWorldgen,
            source_digest: final_plane_digest,
        };
        let receipt = TerrainMountainHeightReceipt {
            adjust_for_mountains_va: TERRAIN_ADJUST_FOR_MOUNTAINS_VA,
            fill_mountain_data_va: TERRAIN_FILL_MOUNTAIN_DATA_VA,
            mountain_range_init_va: MOUNTAIN_RANGE_INIT_VA,
            mountain_range_init_sha256: MOUNTAIN_RANGE_INIT_SHA256,
            pre_mountain_digest: self.source_digest,
            catalog_digest,
            placement_digest,
            final_plane_digest,
            placements,
            source_vertices,
            matched_vertices,
            unmatched_vertices: source_vertices - matched_vertices,
            load_rebuild_mode: false,
            final_query_authority: true,
        };
        Ok((authority, receipt))
    }
}

fn find_tcoord_z_from_authority(
    authority: &TerrainHeightAuthority,
    world: &World,
    tx: i32,
    ty: i32,
    zero_negative: i32,
) -> Result<TerrainTcoordZReceipt, TerrainHeightError> {
    let self_ = authority;
    if self_.source_digest == [0; 32] {
        return Err(TerrainHeightError::MissingSourceIdentity);
    }

    if !self_.is_initialized() {
        let source_bits = [self_.land_height_bits, self_.land_height_bits];
        let raw_z = cvttss2si(f32::from_bits(self_.land_height_bits));
        return Ok(TerrainTcoordZReceipt {
            body_va: TERRAIN_FIND_TCOORD_Z_VA,
            source: self_.source,
            source_digest: self_.source_digest,
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
    if self_.master_land_height_bits.len() != expected_heights {
        return Err(TerrainHeightError::HeightPlaneLengthMismatch {
            expected: expected_heights,
            actual: self_.master_land_height_bits.len(),
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
            source: self_.source,
            source_digest: self_.source_digest,
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
        self_.master_land_height_bits[first],
        self_.master_land_height_bits[second],
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
        source: self_.source,
        source_digest: self_.source_digest,
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
    ReplayWorldSeedMismatch {
        replay_seed: u32,
        world_seed: i32,
    },
    GameSemaphoreTooShort {
        expected: usize,
        actual: usize,
    },
    RefreshFractalInitRejected {
        xs: i32,
        ys: i32,
        smooth: i32,
    },
    InvalidWorldShape,
    ShapeOverflow,
    HeightPlaneLengthMismatch {
        expected: usize,
        actual: usize,
    },
    InvalidFractalShape {
        xs: i32,
        ys: i32,
        expected: usize,
        actual: usize,
    },
    InvalidFractalIncrement {
        x_inc_bits: u64,
        y_inc_bits: u64,
    },
    FractalSampleOutsideInitializedGrid {
        x: i32,
        y: i32,
        column: i32,
        row: i32,
        xs: i32,
        ys: i32,
    },
    CoordInfoFlagsLengthMismatch {
        expected: usize,
        actual: usize,
    },
    MountainCatalogShapeMismatch {
        sources: usize,
        displacement_tgas: usize,
        templates: usize,
        tcoord_vertices: usize,
    },
    MountainRuntimeCatalogMismatch,
    MountainPlacementLengthMismatch {
        locations: usize,
        world_x: usize,
        world_y: usize,
        types: usize,
    },
    MountainTemplateIndexOutsideCatalog {
        placement: usize,
        template: i32,
        templates: usize,
    },
    MountainPlacementLocationMismatch {
        placement: usize,
        world_x: i32,
        world_y: i32,
        location: [u32; 3],
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
        write!(f, "terrain height authority refused: {self:?}")
    }
}

impl std::error::Error for TerrainHeightError {}

fn derive_refresh_worldgen_inputs(
    world: &World,
    replay_payload_sha256: [u8; 32],
    semaphore_821: u8,
    remaining: TerrainHeightNonFractalInputs,
) -> Result<
    (
        TerrainHeightWorldgenInputs,
        TerrainHeightRefreshFractalReceipt,
    ),
    TerrainHeightError,
> {
    let xs = world
        .tile_xs
        .checked_add(1)
        .ok_or(TerrainHeightError::ShapeOverflow)?;
    let ys = world
        .tile_ys
        .checked_add(1)
        .ok_or(TerrainHeightError::ShapeOverflow)?;
    let check_victory_mode = semaphore_821 & 2 != 0;
    let height_requested_smooth = if check_victory_mode { 0 } else { 5 };
    let detail_requested_smooth = height_requested_smooth - 2;
    let height_seed = world.seed as u32;
    let detail_seed = height_seed.wrapping_mul(2);
    let height_plane = generate_retail_fractal(xs, ys, height_requested_smooth, height_seed)
        .map_err(|_| TerrainHeightError::RefreshFractalInitRejected {
            xs,
            ys,
            smooth: height_requested_smooth,
        })?;
    let detail_plane = generate_retail_fractal(xs, ys, detail_requested_smooth, detail_seed)
        .map_err(|_| TerrainHeightError::RefreshFractalInitRejected {
            xs,
            ys,
            smooth: detail_requested_smooth,
        })?;
    let height_fractal = refresh_fractal_authority(
        replay_payload_sha256,
        TERRAIN_HEIGHT_FRACTAL_INIT_CALL_VA,
        height_requested_smooth,
        height_seed,
        &height_plane,
    );
    let height_fractal_detail = refresh_fractal_authority(
        replay_payload_sha256,
        TERRAIN_HEIGHT_DETAIL_FRACTAL_INIT_CALL_VA,
        detail_requested_smooth,
        detail_seed,
        &detail_plane,
    );
    let height_authority_digest = validate_fractal_authority(&height_fractal)?;
    let detail_authority_digest = validate_fractal_authority(&height_fractal_detail)?;
    let receipt = TerrainHeightRefreshFractalReceipt {
        refresh_data_va: TERRAIN_REFRESH_DATA_VA,
        fractal_init_va: FRACTAL_INIT_VA,
        height_call_va: TERRAIN_HEIGHT_FRACTAL_INIT_CALL_VA,
        detail_call_va: TERRAIN_HEIGHT_DETAIL_FRACTAL_INIT_CALL_VA,
        replay_payload_sha256,
        world_seed: world.seed,
        xs,
        ys,
        semaphore_821,
        check_victory_mode,
        height_requested_smooth,
        detail_requested_smooth,
        height_seed,
        detail_seed,
        height_random_draws: height_plane.random_draws,
        detail_random_draws: detail_plane.random_draws,
        height_random_state_after: height_plane.random_state_after,
        detail_random_state_after: detail_plane.random_state_after,
        height_authority_digest,
        detail_authority_digest,
    };
    Ok((
        TerrainHeightWorldgenInputs {
            height_fractal,
            height_fractal_detail,
            coord_info_flags: remaining.coord_info_flags,
            land_height_bits: remaining.land_height_bits,
            mountain_height_bits: remaining.mountain_height_bits,
            height_scale_bits: remaining.height_scale_bits,
            coord_info_source_digest: remaining.coord_info_source_digest,
        },
        receipt,
    ))
}

fn refresh_fractal_authority(
    replay_payload_sha256: [u8; 32],
    call_va: u32,
    requested_smooth: i32,
    seed: u32,
    plane: &RetailFractalPlane,
) -> TerrainFractalAuthority {
    let frac_columns = plane
        .columns
        .iter()
        .flat_map(|column| column.iter().copied())
        .collect::<Vec<_>>();
    let flags = 2;
    let x_inc_bits = (plane.xs as f64 / (plane.xs + 1) as f64).to_bits();
    let y_inc_bits = 1.0f64.to_bits();
    let mut source = Vec::with_capacity(128 + frac_columns.len());
    source.extend_from_slice(b"don-terrain-refresh-fractal-init-v1\0");
    source.extend_from_slice(TERRAIN_REFRESH_DATA_SHA256.as_bytes());
    source.extend_from_slice(FRACTAL_INIT_SHA256.as_bytes());
    source.extend_from_slice(&replay_payload_sha256);
    source.extend_from_slice(&call_va.to_le_bytes());
    for value in [plane.xs, plane.ys, requested_smooth, plane.smooth, flags] {
        source.extend_from_slice(&value.to_le_bytes());
    }
    source.extend_from_slice(&seed.to_le_bytes());
    source.extend_from_slice(&plane.random_draws.to_le_bytes());
    source.extend_from_slice(&plane.random_state_after.to_le_bytes());
    source.extend_from_slice(&x_inc_bits.to_le_bytes());
    source.extend_from_slice(&y_inc_bits.to_le_bytes());
    source.extend_from_slice(&frac_columns);
    TerrainFractalAuthority {
        frac_columns,
        xs: plane.xs,
        ys: plane.ys,
        flags,
        partitions: [-1; 16],
        random_seed: plane.random_state_after as u32,
        x_inc_bits,
        y_inc_bits,
        initialized_source_digest: sha256(&source),
    }
}

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

fn validate_fractal_authority(
    fractal: &TerrainFractalAuthority,
) -> Result<[u8; 32], TerrainHeightError> {
    if fractal.initialized_source_digest == [0; 32] {
        return Err(TerrainHeightError::MissingSourceIdentity);
    }
    let expected = if fractal.xs >= 2 && fractal.ys >= 2 {
        (fractal.xs as usize + 1)
            .checked_mul(fractal.ys as usize + 1)
            .ok_or(TerrainHeightError::ShapeOverflow)?
    } else {
        0
    };
    if expected == 0 || fractal.frac_columns.len() != expected {
        return Err(TerrainHeightError::InvalidFractalShape {
            xs: fractal.xs,
            ys: fractal.ys,
            expected,
            actual: fractal.frac_columns.len(),
        });
    }

    // `Fractal::init` 0x006aa4e7..0x006aa518. Bit zero selects wrapping in X;
    // Y's initialized ratio is `ys/ys` and therefore exactly 1.0.
    let wrap_x = fractal.flags & 1;
    let expected_x_inc = fractal.xs as f64 / (fractal.xs - wrap_x + 1) as f64;
    if fractal.x_inc_bits != expected_x_inc.to_bits() || fractal.y_inc_bits != 1.0f64.to_bits() {
        return Err(TerrainHeightError::InvalidFractalIncrement {
            x_inc_bits: fractal.x_inc_bits,
            y_inc_bits: fractal.y_inc_bits,
        });
    }

    let mut bytes = Vec::with_capacity(160 + fractal.frac_columns.len());
    bytes.extend_from_slice(b"don-initialized-fractal-v1\0");
    bytes.extend_from_slice(&fractal.initialized_source_digest);
    for value in [fractal.xs, fractal.ys, fractal.flags] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for value in fractal.partitions {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&fractal.random_seed.to_le_bytes());
    bytes.extend_from_slice(&fractal.x_inc_bits.to_le_bytes());
    bytes.extend_from_slice(&fractal.y_inc_bits.to_le_bytes());
    bytes.extend_from_slice(&fractal.frac_columns);
    Ok(sha256(&bytes))
}

fn fractal_get_height(
    fractal: &TerrainFractalAuthority,
    x: i32,
    y: i32,
) -> Result<u8, TerrainHeightError> {
    let scaled_x = (x as f64 + 0.5) * f64::from_bits(fractal.x_inc_bits);
    let scaled_y = (y as f64 + 0.5) * f64::from_bits(fractal.y_inc_bits);
    let column = cvttsd2si(scaled_x);
    let row = cvttsd2si(scaled_y);
    if column < 0 || row < 0 || column >= fractal.xs || row >= fractal.ys {
        return Err(TerrainHeightError::FractalSampleOutsideInitializedGrid {
            x,
            y,
            column,
            row,
            xs: fractal.xs,
            ys: fractal.ys,
        });
    }

    let dx = scaled_x - column as f64;
    let dy = scaled_y - row as f64;
    let one_minus_dx = 1.0 - dx;
    let one_minus_dy = 1.0 - dy;
    let stride = fractal.ys as usize + 1;
    let at = |cx: i32, cy: i32| -> f64 {
        fractal.frac_columns[cx as usize * stride + cy as usize] as f64
    };
    let p00 = at(column, row);
    let p10 = at(column + 1, row);
    let p01 = at(column, row + 1);
    let p11 = at(column + 1, row + 1);

    // Preserve the scalar-double multiply/add order at 0x006aa8eb..0x006aa960.
    let w00 = one_minus_dx * one_minus_dy;
    let w10 = one_minus_dy * dx;
    let w01 = one_minus_dx * dy;
    let w11 = dy * dx;
    let interpolated = (((p00 * w00 + 0.0) + p10 * w10) + p01 * w01) + p11 * w11;
    let clamped = cvttsd2si(interpolated).clamp(0, 255) as u8;

    if fractal.partitions[0] != -1 {
        let mut bucket = 0u8;
        for &threshold in &fractal.partitions {
            if threshold == -1 || i32::from(clamped) < threshold {
                break;
            }
            bucket = bucket.wrapping_add(1);
        }
        Ok(bucket)
    } else if fractal.flags & 2 != 0 {
        Ok(((u16::from(clamped) * 100) >> 8) as u8)
    } else {
        Ok(clamped)
    }
}

fn completed_worldgen_digest(
    world: &World,
    inputs: &TerrainHeightWorldgenInputs,
    height_fractal_digest: [u8; 32],
    height_fractal_detail_digest: [u8; 32],
) -> [u8; 32] {
    let mut bytes = Vec::with_capacity(128 + inputs.coord_info_flags.len() * 2);
    bytes.extend_from_slice(b"don-terrain-height-completed-worldgen-v2\0");
    bytes.extend_from_slice(&height_fractal_digest);
    bytes.extend_from_slice(&height_fractal_detail_digest);
    bytes.extend_from_slice(&inputs.coord_info_source_digest);
    for value in [world.xs, world.ys, world.tile_xs, world.tile_ys] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for &value in &inputs.coord_info_flags {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    sha256(&bytes)
}

fn get_coordinfo_vertex_codes(world: &World, flags: &[u16], x: i32, y: i32) -> u16 {
    let cell_x = x / TERRAIN_TESSELATION_LEVEL;
    let cell_y = y / TERRAIN_TESSELATION_LEVEL;
    let rem_x = x % TERRAIN_TESSELATION_LEVEL;
    let rem_y = y % TERRAIN_TESSELATION_LEVEL;
    let at = |wx: i32, wy: i32| -> u16 {
        if wx < 0 || wy < 0 || wx >= world.xs || wy >= world.ys {
            0
        } else {
            flags[(wy * world.xs + wx) as usize]
        }
    };
    let southeast = at(cell_x, cell_y);
    match (rem_x == 0, rem_y == 0) {
        (false, false) => southeast,
        (true, false) => southeast | at(cell_x - 1, cell_y),
        (false, true) => southeast | at(cell_x, cell_y - 1),
        (true, true) => {
            southeast | at(cell_x - 1, cell_y) | at(cell_x, cell_y - 1) | at(cell_x - 1, cell_y - 1)
        }
    }
}

fn get_tdata_vertex_codes(world: &World, x: i32, y: i32) -> u16 {
    let at = |tx: i32, ty: i32| -> u16 {
        if tx < 0 || ty < 0 || tx >= world.tile_xs || ty >= world.tile_ys {
            0
        } else if world.tdata[(ty * world.tile_xs + tx) as usize] & tflag::RIVER != 0 {
            0x10
        } else {
            0
        }
    };
    at(x, y) | at(x - 1, y) | at(x, y - 1) | at(x - 1, y - 1)
}

/// `find_closest_coordinfo(..., 4)` searches the four shipped square shells (radius 0..3).
/// A flagged CoordInfo contributes its `(tesselation_level + 1)^2` render vertices.
fn find_closest_coordinfo(world: &World, flags: &[u16], x: i32, y: i32) -> f32 {
    let center_x = x >> 2;
    let center_y = y >> 2;
    let mut min_squared = 1.0e20f32;
    let mut found = false;
    for radius in 0..4 {
        let mut found_this_shell = false;
        for wy in center_y - radius..=center_y + radius {
            for wx in center_x - radius..=center_x + radius {
                if radius != 0
                    && wx != center_x - radius
                    && wx != center_x + radius
                    && wy != center_y - radius
                    && wy != center_y + radius
                {
                    continue;
                }
                if wx < 0 || wy < 0 || wx >= world.xs || wy >= world.ys {
                    continue;
                }
                if flags[(wy * world.xs + wx) as usize] & 0x20 == 0 {
                    continue;
                }
                found = true;
                found_this_shell = true;
                for local_y in 0..=TERRAIN_TESSELATION_LEVEL {
                    let dy = ((wy * TERRAIN_TESSELATION_LEVEL + local_y - y) * 192) as f32;
                    let dy_squared = dy * dy;
                    for local_x in 0..=TERRAIN_TESSELATION_LEVEL {
                        let dx = ((wx * TERRAIN_TESSELATION_LEVEL + local_x - x) * 192) as f32;
                        let squared = dx * dx + dy_squared;
                        if squared < min_squared {
                            min_squared = squared;
                        }
                    }
                }
            }
        }
        if found_this_shell {
            break;
        }
    }
    if found {
        min_squared.sqrt() * (1.0 / 768.0)
    } else {
        -1.0
    }
}

fn smooth_tcoord(heights: &mut [u32], width: usize, height: usize, x: usize, y: usize) {
    let mut neighborhood = [[0.0f32; 4]; 4];
    for (local_y, row) in neighborhood.iter_mut().enumerate() {
        for (local_x, value) in row.iter_mut().enumerate() {
            let sample_x = x as isize - 1 + local_x as isize;
            let sample_y = y as isize - 1 + local_y as isize;
            if sample_x >= 0
                && sample_y >= 0
                && sample_x < width as isize - 1
                && sample_y < height as isize - 1
            {
                *value = f32::from_bits(heights[sample_y as usize * width + sample_x as usize]);
            }
        }
    }
    for center_y in 1..=2 {
        for center_x in 1..=2 {
            let mut sum = 0.0f32;
            let mut count = 0;
            for row in &neighborhood[center_y - 1..=center_y + 1] {
                for &sample in &row[center_x - 1..=center_x + 1] {
                    if sample != 0.0 {
                        count += 1;
                        sum += sample;
                    }
                }
            }
            let output_x = x + center_x - 1;
            let output_y = y + center_y - 1;
            let output_index = output_y * width + output_x;
            if count != 0 && f32::from_bits(heights[output_index]) > 0.0 {
                heights[output_index] = (sum / count as f32).to_bits();
            }
        }
    }
}

fn worldgen_plane_digest(
    world: &World,
    inputs: &TerrainHeightWorldgenInputs,
    completed_worldgen_digest: [u8; 32],
    height_fractal_samples: &[u8],
    height_fractal_detail_samples: &[u8],
    heights: &[u32],
) -> [u8; 32] {
    let mut bytes = Vec::with_capacity(
        64 + height_fractal_samples.len() * 2
            + inputs.coord_info_flags.len() * 2
            + world.tdata.len() * 2
            + heights.len() * 4,
    );
    bytes.extend_from_slice(b"don-terrain-height-worldgen-v2\0");
    bytes.extend_from_slice(&completed_worldgen_digest);
    for value in [
        world.xs,
        world.ys,
        world.tile_xs,
        world.tile_ys,
        TERRAIN_TESSELATION_LEVEL,
    ] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for value in [
        inputs.land_height_bits,
        inputs.mountain_height_bits,
        inputs.height_scale_bits,
    ] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(height_fractal_samples);
    bytes.extend_from_slice(height_fractal_detail_samples);
    for &value in &inputs.coord_info_flags {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for &value in &world.tdata {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for &value in heights {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    sha256(&bytes)
}

fn mountain_catalog_digest(catalog: &MountainTemplateCatalog) -> [u8; 32] {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"don-mountain-height-catalog-v1\0");
    bytes.extend_from_slice(MOUNTAIN_RANGE_INIT_SHA256.as_bytes());
    bytes.extend_from_slice(&(catalog.sources.len() as u64).to_le_bytes());
    bytes.extend_from_slice(&(catalog.effects_graphics_xml.byte_length as u64).to_le_bytes());
    bytes.extend_from_slice(&catalog.effects_graphics_xml.adler32.to_le_bytes());
    for (((source, evidence), runtime), vertices) in catalog
        .sources
        .iter()
        .zip(&catalog.displacement_tgas)
        .zip(&catalog.templates)
        .zip(&catalog.tcoord_vertices)
    {
        bytes.extend_from_slice(&(source.index as u64).to_le_bytes());
        digest_string(&mut bytes, &source.area);
        bytes.extend_from_slice(&source.height_bits.to_le_bytes());
        digest_string(&mut bytes, &source.displacement_path);
        digest_string(&mut bytes, &source.main_alpha_path);
        digest_string(&mut bytes, &source.ring_alpha_path);
        bytes.extend_from_slice(&(evidence.byte_length as u64).to_le_bytes());
        bytes.extend_from_slice(&evidence.adler32.to_le_bytes());
        for offsets in [
            &runtime.mount_tiles,
            &runtime.mount_wcoords,
            &runtime.solid_mount_wcoords,
        ] {
            bytes.extend_from_slice(&(offsets.len() as u64).to_le_bytes());
            for offset in offsets {
                bytes.extend_from_slice(&offset.x.to_le_bytes());
                bytes.extend_from_slice(&offset.y.to_le_bytes());
            }
        }
        bytes.extend_from_slice(&(vertices.len() as u64).to_le_bytes());
        for vertex in vertices {
            bytes.extend_from_slice(&vertex.x_bits.to_le_bytes());
            bytes.extend_from_slice(&vertex.y_bits.to_le_bytes());
            bytes.extend_from_slice(&vertex.z_bits.to_le_bytes());
        }
    }
    sha256(&bytes)
}

fn digest_string(bytes: &mut Vec<u8>, value: &str) {
    bytes.extend_from_slice(&(value.len() as u64).to_le_bytes());
    bytes.extend_from_slice(value.as_bytes());
}

fn mountain_height_plane_digest(
    pre_mountain_digest: [u8; 32],
    catalog_digest: [u8; 32],
    placement_digest: [u8; 32],
    world: &World,
    heights: &[u32],
) -> [u8; 32] {
    let mut bytes = Vec::with_capacity(128 + heights.len() * 4);
    bytes.extend_from_slice(b"don-terrain-height-post-mountain-v1\0");
    bytes.extend_from_slice(&pre_mountain_digest);
    bytes.extend_from_slice(&catalog_digest);
    bytes.extend_from_slice(&placement_digest);
    for value in [world.xs, world.ys, world.tile_xs, world.tile_ys] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for &height in heights {
        bytes.extend_from_slice(&height.to_le_bytes());
    }
    sha256(&bytes)
}

/// SSE `cvttsd2si`: truncate toward zero, returning `0x80000000` on NaN/overflow.
fn cvttsd2si(value: f64) -> i32 {
    if !value.is_finite() || !(-2_147_483_648.0..2_147_483_648.0).contains(&value) {
        i32::MIN
    } else {
        value.trunc() as i32
    }
}

/// SSE `cvttss2si`: truncate toward zero, returning `0x80000000` on NaN/overflow.
fn cvttss2si(value: f32) -> i32 {
    if !value.is_finite() || !(-2_147_483_648.0..2_147_483_648.0).contains(&value) {
        i32::MIN
    } else {
        value.trunc() as i32
    }
}

#[cfg(test)]
mod refresh_fractal_tests {
    use super::*;

    fn remaining(world: &World) -> TerrainHeightNonFractalInputs {
        TerrainHeightNonFractalInputs {
            coord_info_flags: vec![0; world.size as usize],
            land_height_bits: 30.0f32.to_bits(),
            mountain_height_bits: (-303.0f32).to_bits(),
            height_scale_bits: 1.0f32.to_bits(),
            coord_info_source_digest: [0xa5; 32],
        }
    }

    #[test]
    fn refresh_calls_derive_both_guarded_fractals_from_world_seed() {
        let mut world = World::init_default_rules(8, 8);
        world.seed = 0x1234_5678;
        let (inputs, receipt) =
            derive_refresh_worldgen_inputs(&world, [0x91; 32], 0, remaining(&world)).unwrap();

        assert_eq!((receipt.xs, receipt.ys), (33, 33));
        assert_eq!(
            (
                receipt.height_requested_smooth,
                receipt.detail_requested_smooth,
                receipt.height_seed,
                receipt.detail_seed,
            ),
            (5, 3, 0x1234_5678, 0x2468_acf0)
        );
        assert_eq!(inputs.height_fractal.flags, 2);
        assert_eq!(inputs.height_fractal_detail.flags, 2);
        assert_eq!(inputs.height_fractal.partitions, [-1; 16]);
        assert_eq!(inputs.height_fractal.frac_columns.len(), 34 * 34);
        assert_eq!(
            inputs.height_fractal.x_inc_bits,
            (33.0f64 / 34.0f64).to_bits()
        );
        assert_eq!(inputs.height_fractal.y_inc_bits, 1.0f64.to_bits());
        assert_eq!(
            inputs.height_fractal.random_seed,
            receipt.height_random_state_after as u32
        );
        assert_ne!(receipt.height_authority_digest, [0; 32]);
        assert_ne!(receipt.detail_authority_digest, [0; 32]);
    }

    #[test]
    fn victory_bit_reproduces_zero_and_negative_two_requested_smooths() {
        let mut world = World::init_default_rules(8, 8);
        world.seed = -7;
        let (inputs, receipt) =
            derive_refresh_worldgen_inputs(&world, [0x92; 32], 2, remaining(&world)).unwrap();

        assert!(receipt.check_victory_mode);
        assert_eq!(receipt.semaphore_821, 2);
        assert_eq!(receipt.height_requested_smooth, 0);
        assert_eq!(receipt.detail_requested_smooth, -2);
        assert_eq!(receipt.height_seed, (-7i32) as u32);
        assert_eq!(receipt.detail_seed, ((-7i32) as u32).wrapping_mul(2));
        assert_eq!(inputs.height_fractal.frac_columns.len(), 34 * 34);
        assert_eq!(inputs.height_fractal_detail.frac_columns.len(), 34 * 34);
        assert_ne!(
            inputs.height_fractal.initialized_source_digest,
            inputs.height_fractal_detail.initialized_source_digest
        );
    }
}
