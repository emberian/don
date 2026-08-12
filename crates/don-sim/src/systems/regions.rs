//! Deterministic region reconstruction over the world-cell map.
//!
//! Provenance is the retail executable plus its shipped PDB.  The PDB fixes the
//! layouts of `Regions` (60 bytes), `Region` (136 bytes), `WCoordList` (28 bytes),
//! and the three inline bit masks.  The implementation follows these shipped
//! bodies:
//!
//! - `Regions::find_all` `0x0067eff0`--`0x0067f7c8`
//! - `Regions::rebuild_coords` `0x0067f800`--`0x0067fb85`
//! - `Regions::sort_regions` `0x0067fb90`--`0x0067fd6c`
//! - `Regions::set_coastals` `0x0067fd70`--`0x00680051`
//! - `Regions::clear_all` `0x00680060`--`0x00680172`
//! - `Region::finalize_coastals` `0x006805b0`--`0x00680757`
//!
//! In `Map::make` (`0x0068bc90`) the second `clear_all` / `find_all` pair is
//! called immediately after `Map::make_coastlines`.  [`make_coastlines_and_rebuild_regions`]
//! is that complete common construction-chain segment.  Map-style continent
//! generation and the earlier provisional region pass remain upstream.

use super::map_terrain::{wflag, World, NEIGHBOUR_DX, NEIGHBOUR_DY};

pub const REGION_COUNT: usize = 128;
pub const LAND_REGION_COUNT: usize = 64;
pub const SEA_REGION_FIRST: usize = 64;
pub const SEA_REGION_END: usize = 127;

// The two shipped int[40] tables at 0x00adcb14 / 0x00adc424.  The first sixteen
// entries are the radius-two perimeter and the remaining twenty-four are the
// radius-three perimeter.  Their order is retained even though the writes are
// idempotent.
const COASTAL_DX: [i32; 40] = [
    -1, 0, 1, 2, 2, 2, 1, 0, -1, -2, -2, -2, -2, 2, 2, -2, -3, -2, -1, 0, 1, 2, 3, 3, 3, 3, 3, 3,
    3, 2, 1, 0, -1, -2, -3, -3, -3, -3, -3, -3,
];
const COASTAL_DY: [i32; 40] = [
    -2, -2, -2, -1, 0, 1, 2, 2, 2, 1, 0, -1, -2, -2, 2, 2, -3, -3, -3, -3, -3, -3, -3, -2, -1, 0,
    1, 2, 3, 3, 3, 3, 3, 3, 3, 2, 1, 0, -1, -2,
];

/// Inline `BitMask<N>` state, including the flag word retail changes on writes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InlineBitMask<const BYTES: usize> {
    pub bits: i32,
    pub size: i32,
    pub flags: i32,
    pub bytes: [u8; BYTES],
}

impl<const BYTES: usize> InlineBitMask<BYTES> {
    fn new(bits: i32) -> Self {
        Self {
            bits,
            size: BYTES as i32,
            flags: 1,
            bytes: [0; BYTES],
        }
    }

    fn clear(&mut self) {
        self.bytes.fill(0);
        self.flags = 1;
    }

    fn get(&self, bit: usize) -> bool {
        debug_assert!(bit < self.bits as usize);
        self.bytes[bit >> 3] & (1 << (bit & 7)) != 0
    }

    fn set(&mut self, bit: usize) {
        debug_assert!(bit < self.bits as usize);
        self.bytes[bit >> 3] |= 1 << (bit & 7);
        self.flags = 0;
    }
}

/// `WCoordList : Array<WCoordData>` (PDB size 28).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WCoordList {
    pub items: Vec<(i32, i32)>,
    pub capacity: i32,
    pub increment: i16,
    pub flags: u8,
    /// Not walked by `Array<WCoordData>::walk_data`; retained for the PDB layout.
    pub cur_index: i32,
}

impl Default for WCoordList {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            capacity: 0,
            increment: -1,
            flags: 0,
            cur_index: 0,
        }
    }
}

impl WCoordList {
    fn release_preserving_cursor_and_increment(&mut self) {
        self.items.clear();
        self.capacity = 0;
        self.flags = 0;
    }

    fn push_retail(&mut self, coord: (i32, i32)) {
        let length = self.items.len() as i32;
        if length >= self.capacity {
            let increase = if self.increment < 0 {
                self.capacity.max(4)
            } else {
                i32::from(self.increment)
            };
            // Every region list is constructed with increment -1.  Reaching
            // this branch with zero growth therefore means corrupt metadata.
            assert!(increase > 0, "retail WCoordList cannot grow by zero");
            self.capacity = self.capacity.saturating_add(increase);
        }
        self.items.push(coord);
    }
}

/// `Region` (PDB size 136), field-for-field apart from native pointer/vtable bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Region {
    pub common_factor: i32,
    pub goody_factor: i32,
    pub flags: i32,
    pub climate: i32,
    pub region: i32,
    pub size: i32,
    pub rank: i32,
    pub resource: i32,
    pub fertile: i32,
    pub site: i32,
    pub goodies: i32,
    pub borders: i32,
    pub border_id: u8,
    pub scouted: InlineBitMask<1>,
    pub coastal: InlineBitMask<8>,
    pub coast: InlineBitMask<8>,
    pub coords: WCoordList,
}

impl Region {
    fn new(region: i32) -> Self {
        Self {
            common_factor: 0,
            goody_factor: 0,
            flags: 0,
            climate: 0,
            region,
            size: 0,
            rank: 0,
            resource: 0,
            fertile: 0,
            site: 0,
            goodies: 0,
            borders: 0,
            border_id: 0,
            scouted: InlineBitMask::new(8),
            coastal: InlineBitMask::new(64),
            coast: InlineBitMask::new(64),
            coords: WCoordList::default(),
        }
    }

    pub fn is_coastal_with(&self, other_land_region: usize) -> bool {
        self.coastal.get(other_land_region)
    }

    pub fn has_coast(&self, sea_region: usize) -> bool {
        (SEA_REGION_FIRST..128).contains(&sea_region)
            && self.coast.get(sea_region - SEA_REGION_FIRST)
    }
}

/// `Regions` state.  Retail's `ObjectArray<Region>` is fixed to 128 records by
/// `Regions::init` `0x006814c0`, which also writes each record's identity field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Regions {
    pub list: [Region; REGION_COUNT],
    pub sea: i32,
    pub land: i32,
    pub coords: WCoordList,
}

impl Default for Regions {
    fn default() -> Self {
        Self {
            list: std::array::from_fn(|region| Region::new(region as i32)),
            sea: 0,
            land: 0,
            coords: WCoordList::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegionsError {
    InvalidWorldShape {
        xs: i32,
        ys: i32,
        size: i32,
        wdata_len: usize,
    },
    InvalidRegionId {
        x: i32,
        y: i32,
        region: i16,
    },
    RegionSizeMismatch {
        region: usize,
        expected: i32,
        actual: i32,
    },
}

/// Observable completion facts which are not persistent members of `Regions`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct RegionBuildReceipt {
    /// Physical connected land components found before retail's overflow aggregation.
    pub land_components_found: i32,
    /// Physical connected sea components found before retail's overflow aggregation.
    pub sea_components_found: i32,
    /// Uses of land scratch region 62 and aggregate region 63.
    pub land_consolidations: i32,
    /// Uses of sea scratch region 125 and aggregate region 126.
    pub sea_consolidations: i32,
    /// Calls retail makes to `do_all_non_input` (`0x00538810`): one after each
    /// component flood and one at the end of `find_all`.  The function is a host
    /// message pump, not checksum state, so the deterministic port reports it.
    pub non_input_pumps: i32,
}

impl Regions {
    /// Exact `Regions::clear_all` `0x00680060`.
    ///
    /// The conditional release is intentional: coordinates, borders and border id
    /// are cleared only when the old `size` is nonzero.  `region2` is not touched.
    pub fn clear_all(&mut self, world: &mut World) {
        for region in &mut self.list {
            region.flags = 0;
            region.climate = 0;
            region.goodies = 0;
            region.common_factor = 8;
            region.goody_factor = 8;
            if region.size != 0 {
                region.size = 0;
                region.coords.release_preserving_cursor_and_increment();
                region.borders = 0;
                region.border_id = 0;
            }
        }
        for cell in &mut world.wdata {
            cell.region = 0;
        }
        self.land = 0;
        self.sea = 64;
    }

    /// The post-coastline `clear_all` / `find_all` pair from `Map::make`.
    ///
    /// The pair is transactional for structural errors.  Retail's exact 62/63 and
    /// 125/126 consolidation path is retained.  More than 62 physical land
    /// components exposes a shipped inconsistency: relabelling region zero also
    /// captures not-yet-visited cells without increasing region 63's size.  Retail's
    /// later `Region Size mismatch` diagnostic becomes [`RegionsError::RegionSizeMismatch`]
    /// here, and the caller sees none of the partial writes.
    pub fn rebuild_after_coastlines(
        &mut self,
        world: &mut World,
    ) -> Result<RegionBuildReceipt, RegionsError> {
        validate_world_shape(world)?;
        let mut next_regions = self.clone();
        let mut next_world = world.clone();
        next_regions.clear_all(&mut next_world);
        let receipt = next_regions.find_all_cleared(&mut next_world)?;
        *self = next_regions;
        *world = next_world;
        Ok(receipt)
    }

    /// Exact `Regions::find_all(int)` `0x0067eff0` after its caller has already
    /// completed `Regions::clear_all`.
    ///
    /// The native formal stack argument is unread. The operation is
    /// transactional for structural errors so a failed coordinate rebuild does
    /// not expose the native body's partial Region or WData writes.
    pub fn find_all_after_clear(
        &mut self,
        world: &mut World,
    ) -> Result<RegionBuildReceipt, RegionsError> {
        validate_world_shape(world)?;
        let mut next_regions = self.clone();
        let mut next_world = world.clone();
        let receipt = next_regions.find_all_cleared(&mut next_world)?;
        *self = next_regions;
        *world = next_world;
        Ok(receipt)
    }

    pub fn get_num_land(&self) -> i32 {
        self.list[..LAND_REGION_COUNT]
            .iter()
            .filter(|region| region.size != 0)
            .count() as i32
    }

    /// `Regions::get_num_sea` `0x0067eed0` counts records 64 through 126.
    pub fn get_num_sea(&self) -> i32 {
        self.list[SEA_REGION_FIRST..SEA_REGION_END]
            .iter()
            .filter(|region| region.size != 0)
            .count() as i32
    }

    fn find_all_cleared(&mut self, world: &mut World) -> Result<RegionBuildReceipt, RegionsError> {
        self.coords.items.clear();
        self.coords.increment = -1;
        self.coords.flags = 0;
        if self.coords.capacity < world.size {
            self.coords.capacity = world.size;
        }

        let mut receipt = RegionBuildReceipt::default();
        for y in 0..world.ys {
            for x in 0..world.xs {
                if world.wdata(x, y).region != 0 {
                    continue;
                }
                let ocean = world.is_ocean(x, y);

                // Exact pre-existing-neighbour fast path.  It is normally
                // unreachable after clear_all + a complete flood, but is part of
                // the shipped find_all body and matters for corrupted inputs.
                let mut inherited = None;
                for (&dx, &dy) in NEIGHBOUR_DX.iter().zip(NEIGHBOUR_DY.iter()) {
                    let nx = x + dx;
                    let ny = y + dy;
                    if !world.valid_w(nx, ny) || world.is_ocean(nx, ny) != ocean {
                        continue;
                    }
                    let neighbour_region = world.wdata(nx, ny).region;
                    if neighbour_region != 0 && neighbour_region != 127 {
                        inherited = Some(neighbour_region);
                        break;
                    }
                }
                if let Some(region) = inherited {
                    let index = region as usize;
                    world.wdata_mut(x, y).region = region;
                    self.list[index].coords.push_retail((x, y));
                    self.list[index].size += 1;
                    continue;
                }

                let region_id = if ocean {
                    receipt.sea_components_found += 1;
                    self.sea += 1;
                    self.sea.clamp(64, 125) as usize
                } else {
                    receipt.land_components_found += 1;
                    (1..64)
                        .find(|&region| self.list[region].size == 0)
                        .unwrap_or(62)
                };

                self.flood_region(world, x, y, region_id, ocean);
                self.list[region_id].climate = 0;
                receipt.non_input_pumps += 1;

                if !ocean && region_id == 62 {
                    receipt.land_consolidations += 1;
                    self.consolidate_overflow(world, 0..62, 62, 63);
                } else if ocean && region_id == 125 {
                    receipt.sea_consolidations += 1;
                    self.consolidate_overflow(world, 64..125, 125, 126);
                }
            }
        }

        // WATERHALF's water-side region is filled X first, then Y, and the
        // first pure-ocean neighbour in the canonical eight-neighbour ring wins.
        for x in 0..world.xs {
            for y in 0..world.ys {
                let cell = world.wdata(x, y);
                if cell.flags & wflag::WATERHALF == 0 || cell.region2 != 0 {
                    continue;
                }
                for (&dx, &dy) in NEIGHBOUR_DX.iter().zip(NEIGHBOUR_DY.iter()) {
                    let nx = x + dx;
                    let ny = y + dy;
                    if world.valid_w(nx, ny) && world.is_ocean(nx, ny) {
                        let region2 = world.wdata(nx, ny).region;
                        world.wdata_mut(x, y).region2 = region2;
                        break;
                    }
                }
            }
        }

        self.set_coastals(world);
        self.sort_regions();

        // find_all frees the shared flood queue before rebuilding persistent
        // per-region coordinate arrays.
        self.coords.items.clear();
        self.coords.capacity = 0;
        self.coords.flags = 0;
        self.rebuild_coords(world)?;
        receipt.non_input_pumps += 1;
        Ok(receipt)
    }

    fn flood_region(&mut self, world: &mut World, x: i32, y: i32, region: usize, ocean: bool) {
        self.list[region].size = 0;
        let mut queue = Vec::with_capacity(world.size as usize);
        queue.push((x, y));
        world.wdata_mut(x, y).region = region as i16;
        let mut out = 0;
        while out < queue.len() {
            let (cx, cy) = queue[out];
            out += 1;
            self.list[region].size += 1;
            for (&dx, &dy) in NEIGHBOUR_DX.iter().zip(NEIGHBOUR_DY.iter()) {
                let nx = cx + dx;
                let ny = cy + dy;
                if world.valid_w(nx, ny)
                    && world.wdata(nx, ny).region == 0
                    && world.is_ocean(nx, ny) == ocean
                {
                    world.wdata_mut(nx, ny).region = region as i16;
                    queue.push((nx, ny));
                }
            }
        }
    }

    fn consolidate_overflow(
        &mut self,
        world: &mut World,
        reusable: std::ops::Range<usize>,
        scratch: usize,
        aggregate: usize,
    ) {
        let scratch_size = self.list[scratch].size;
        // The executable updates the candidate only on strict `<`, so ties keep
        // the first (lowest-id) region encountered.
        let mut smallest = None;
        let mut smallest_size = scratch_size;
        for region in reusable {
            if self.list[region].size < smallest_size {
                smallest = Some(region);
                smallest_size = self.list[region].size;
            }
        }

        let destination = if let Some(region) = smallest {
            self.list[aggregate].size += self.list[region].size;
            self.list[region].size = 0;
            relabel_world(world, region, aggregate);
            region
        } else {
            aggregate
        };
        self.list[destination].size += scratch_size;
        self.list[scratch].size = 0;
        relabel_world(world, scratch, destination);
    }

    fn set_coastals(&mut self, world: &World) {
        // Retail stops at byte offset 0x4378: region 127 is intentionally untouched.
        for region in &mut self.list[..SEA_REGION_END] {
            region.scouted.clear();
            region.coastal.clear();
            region.coast.clear();
        }

        for y in 0..world.ys {
            for x in 0..world.xs {
                let land_region = world.wdata(x, y).region as usize;
                if land_region >= LAND_REGION_COUNT {
                    continue;
                }
                for (&dx, &dy) in NEIGHBOUR_DX.iter().zip(NEIGHBOUR_DY.iter()) {
                    let nx = x + dx;
                    let ny = y + dy;
                    if !world.valid_w(nx, ny) {
                        continue;
                    }
                    let sea_region = world.wdata(nx, ny).region as usize;
                    if !(SEA_REGION_FIRST..REGION_COUNT).contains(&sea_region) {
                        continue;
                    }
                    self.list[land_region]
                        .coast
                        .set(sea_region - SEA_REGION_FIRST);

                    for (&far_dx, &far_dy) in COASTAL_DX.iter().zip(COASTAL_DY.iter()) {
                        let far_x = x + far_dx;
                        let far_y = y + far_dy;
                        if !world.valid_w(far_x, far_y) {
                            continue;
                        }
                        let other = world.wdata(far_x, far_y).region as usize;
                        if other < LAND_REGION_COUNT && other != land_region {
                            self.list[land_region].coastal.set(other);
                            self.list[other].coastal.set(land_region);
                        }
                    }
                }
            }
        }

        for region in 0..LAND_REGION_COUNT {
            self.finalize_coastals(region);
        }
    }

    fn finalize_coastals(&mut self, index: usize) {
        let self_region = self.list[index].region as usize;
        let mut adjacent = 0;
        while adjacent < LAND_REGION_COUNT {
            let mut rewind = None;
            if adjacent != self_region && self.list[index].coastal.get(adjacent) {
                for third in 0..LAND_REGION_COUNT {
                    if third == self_region
                        || third == adjacent
                        || !self.list[adjacent].coastal.get(third)
                        || self.list[index].coastal.get(third)
                    {
                        continue;
                    }
                    let shares_sea = (0..63).any(|sea| {
                        self.list[index].coast.get(sea)
                            && self.list[adjacent].coast.get(sea)
                            && self.list[third].coast.get(sea)
                    });
                    if shares_sea {
                        self.list[index].coastal.set(third);
                        self.list[third].coastal.set(self_region);
                        if rewind.is_none() && third <= adjacent {
                            rewind = Some(third);
                        }
                    }
                }
            }
            adjacent = rewind.unwrap_or(adjacent + 1);
        }
    }

    fn sort_regions(&mut self) {
        let mut land: Vec<usize> = (0..LAND_REGION_COUNT).collect();
        land.sort_by(|&left, &right| self.list[right].size.cmp(&self.list[left].size));
        for (rank, region) in land.into_iter().enumerate() {
            self.list[region].rank = rank as i32;
        }

        let mut sea: Vec<usize> = (SEA_REGION_FIRST..SEA_REGION_END).collect();
        sea.sort_by(|&left, &right| self.list[right].size.cmp(&self.list[left].size));
        for (rank, region) in sea.into_iter().enumerate() {
            self.list[region].rank = rank as i32;
        }
    }

    fn rebuild_coords(&mut self, world: &World) -> Result<(), RegionsError> {
        for region in &mut self.list {
            if region.size == 0 {
                region.coords.release_preserving_cursor_and_increment();
            } else {
                region.coords.items.clear();
                region.coords.increment = -1;
                region.coords.flags = 0;
                if region.coords.capacity < region.size {
                    region.coords.capacity = region.size;
                }
            }
        }

        let mut actual = [0i32; REGION_COUNT];
        for y in 0..world.ys {
            for x in 0..world.xs {
                let raw = world.wdata(x, y).region;
                let Ok(region) = usize::try_from(raw) else {
                    return Err(RegionsError::InvalidRegionId { x, y, region: raw });
                };
                if region >= REGION_COUNT {
                    return Err(RegionsError::InvalidRegionId { x, y, region: raw });
                }
                actual[region] += 1;
                if actual[region] > self.list[region].size {
                    return Err(RegionsError::RegionSizeMismatch {
                        region,
                        expected: self.list[region].size,
                        actual: actual[region],
                    });
                }
                self.list[region].coords.items.push((x, y));
            }
        }
        for (region, &actual) in actual.iter().enumerate() {
            if actual != self.list[region].size {
                return Err(RegionsError::RegionSizeMismatch {
                    region,
                    expected: self.list[region].size,
                    actual,
                });
            }
        }
        Ok(())
    }
}

/// Exact common `Map::make` segment from `make_coastlines` through the second
/// region rebuild.  The composite is fail-closed for structural Rust errors.
pub fn make_coastlines_and_rebuild_regions(
    world: &mut World,
    regions: &mut Regions,
) -> Result<RegionBuildReceipt, RegionsError> {
    validate_world_shape(world)?;
    let mut next_world = world.clone();
    let mut next_regions = regions.clone();
    next_world.make_coastlines();
    next_regions.clear_all(&mut next_world);
    let receipt = next_regions.find_all_cleared(&mut next_world)?;
    *world = next_world;
    *regions = next_regions;
    Ok(receipt)
}

fn validate_world_shape(world: &World) -> Result<(), RegionsError> {
    let expected = world.xs.checked_mul(world.ys);
    if world.xs < 0
        || world.ys < 0
        || expected != Some(world.size)
        || usize::try_from(world.size).ok() != Some(world.wdata.len())
    {
        return Err(RegionsError::InvalidWorldShape {
            xs: world.xs,
            ys: world.ys,
            size: world.size,
            wdata_len: world.wdata.len(),
        });
    }
    Ok(())
}

fn relabel_world(world: &mut World, from: usize, to: usize) {
    for cell in &mut world.wdata {
        if cell.region == from as i16 {
            cell.region = to as i16;
        }
    }
}
