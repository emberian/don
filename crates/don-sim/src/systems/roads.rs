//! Retail step 22: `Roads::scan_and_kill_stray_roads` `0x008956A0`.
//!
//! The scheduled body is recovered in full from the shipped executable/PDB.  It advances
//! the PDB `Roads::curscan_x/y` pair, examines `WorldData::size / 500` WCoord cells, builds
//! four persistent nine-entry caches for each of their sixteen tiles, and runs
//! `scan_and_kill_bad_tcoord` followed by `scan_and_kill_straggled_tcoord`.
//!
//! One input does not live in `WorldData`: `Terrain::CoordInfo::roads_in_wcoord` points at
//! sixteen `RoadElementCandidate` records maintained by the product road renderer.  This
//! module represents the six fields the scheduled body actually reads or mutates.  A current
//! road tile without that fact fails closed and is reported; neighbour candidates are not
//! inputs because retail reuses the current record's direction bits for all nine cache slots.
//! Tiles without the `SURFACE_ROAD` bit establish that no candidate is needed, so an
//! ordinary road-free headless map still executes the exact scanner without synthetic data.

use std::collections::BTreeMap;

use super::map_terrain::{tflag, World};

/// `RoadElementCandidate`, PDB size 16, reduced to fields reached by step 22 and its direct
/// `TerrainOut::road_changed` child.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoadCandidate {
    /// `+0x00`.
    pub element_num: i32,
    /// `+0x04`; the eight road-connect directions occupy bits 31..24.
    pub flags: u32,
    /// `+0x08`; ten is the empty-candidate sentinel.
    pub rotation: u8,
    /// `+0x0A`, decremented by the direct `road_changed(..., add=0)` child.
    pub ref_count: u8,
    /// `+0x0B`, consumed before `ref_count` when nonzero.
    pub pending_camel_steps: u8,
    /// `+0x0F`.
    pub is_terrain_creation: bool,
}

impl Default for RoadCandidate {
    fn default() -> Self {
        Self {
            element_num: 0,
            flags: 0,
            rotation: 10,
            ref_count: 0,
            pending_camel_steps: 0,
            is_terrain_creation: false,
        }
    }
}

/// Whether the renderer-owned candidate fact is known for one tile.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CandidateFact {
    /// The headless adapter has not supplied a candidate for a live road tile.
    #[default]
    Missing,
    /// `CoordInfo::roads_in_wcoord == nullptr` or the selected slot is empty.
    Absent,
    /// A live 16-byte candidate.
    Present(RoadCandidate),
}

/// Persistent PDB Roads fields reached by the scheduled pass, plus the sparse candidate
/// view supplied by the product terrain/road layer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoadScanState {
    /// `Roads +0x5FC/+0x600`; `RoadsOut::init` initializes both to zero.
    pub curscan_x: i32,
    pub curscan_y: i32,
    /// `RoadsPieces +0x9C`, the parsed `C4` support element index.
    pub support_element: i32,
    /// `Roads +0x128`; the out-of-bounds arm accidentally clears this old cache instead
    /// of `road_cache2`. It is retained to reproduce that store.
    pub legacy_road_cache: [i32; 9],
    /// `Roads +0x604/+0x628/+0x64C/+0x670`.
    pub road_cache2: [i32; 9],
    pub build_cache: [i32; 9],
    pub points_cache: [i32; 9],
    pub aqua_cache: [i32; 9],
    candidates: BTreeMap<(i32, i32), CandidateFact>,
    /// Typed form of the reached `Roads::road_cleared` presentation/renderer queue edge.
    /// Cleared at the start of every call so events never replay.
    pub last_cleared: Vec<(i32, i32)>,
}

impl Default for RoadScanState {
    fn default() -> Self {
        Self {
            curscan_x: 0,
            curscan_y: 0,
            support_element: -1,
            legacy_road_cache: [0; 9],
            road_cache2: [0; 9],
            build_cache: [0; 9],
            points_cache: [0; 9],
            aqua_cache: [0; 9],
            candidates: BTreeMap::new(),
            last_cleared: Vec::new(),
        }
    }
}

impl RoadScanState {
    /// Supply an exact renderer candidate fact for a tile.
    pub fn set_candidate(&mut self, tx: i32, ty: i32, fact: CandidateFact) {
        self.candidates.insert((tx, ty), fact);
    }

    pub fn candidate(&self, tx: i32, ty: i32) -> CandidateFact {
        self.candidates
            .get(&(tx, ty))
            .copied()
            .unwrap_or(CandidateFact::Missing)
    }
}

/// Per-call evidence from the retail scanner.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RoadScanTrace {
    /// `WorldData::size / 500` WCoord cells visited.
    pub cells_scanned: u32,
    /// Sixteen per visited cell.
    pub tiles_scanned: u32,
    /// Distinct terrain road bits cleared. A tile reached twice in the two direct children
    /// is counted once, matching the `World::set_road_at` transition.
    pub roads_cleared: u32,
    /// Tile evaluations suppressed because a live road's renderer candidate was absent
    /// from the headless input.
    pub missing_candidate_tiles: u32,
    /// Direct `TerrainOut::road_changed(..., add=0)` calls, including calls after a prior
    /// child already removed the candidate.
    pub road_changed_calls: u32,
}

// PDB/global tables at 0x00ADCAF0, 0x00ADC400 and 0x00AF2570. Index zero is the tile itself;
// the remaining order is NW, N, NE, E, SE, S, SW, W.
const DX: [i32; 9] = [0, -1, 0, 1, 1, 1, 0, -1, -1];
const DY: [i32; 9] = [0, -1, -1, -1, 0, 1, 1, 1, 0];
const ROAD_DIRECTION_MASK: [u32; 9] = [
    0,
    0x8000_0000,
    0x4000_0000,
    0x2000_0000,
    0x1000_0000,
    0x0800_0000,
    0x0400_0000,
    0x0200_0000,
    0x0100_0000,
];

#[inline]
fn is_road(world: &World, tx: i32, ty: i32) -> bool {
    world.tmask(tx, ty) & tflag::SURFACE_MASK == tflag::SURFACE_ROAD
}

/// Populate the four caches exactly as `0x00895770..0x00895899` does. `candidate` is the
/// record for the tile being scanned, not a record selected from each neighbouring tile:
/// retail computes that pointer once at `0x00895849..0x00895858` and reuses its `flags`
/// dword for all nine direction masks. Returns whether the current live road made that
/// renderer-owned record unknowable.
fn populate_caches(
    state: &mut RoadScanState,
    world: &World,
    tx: i32,
    ty: i32,
    candidate: CandidateFact,
) -> bool {
    let missing = is_road(world, tx, ty) && candidate == CandidateFact::Missing;
    for i in 0..9 {
        let nx = tx + DX[i];
        let ny = ty + DY[i];
        if !world.valid_t(nx, ny) {
            // Retail typo/quirk: the store is `[Roads+0x128+i*4]`, not road_cache2 at
            // +0x604. Consequently road_cache2 retains its preceding value off-map.
            state.legacy_road_cache[i] = 0;
            state.build_cache[i] = 0;
            state.aqua_cache[i] = 0;
            state.points_cache[i] = 0;
            continue;
        }

        let mask = world.tmask(nx, ny);
        let road = (mask & tflag::SURFACE_MASK == tflag::SURFACE_ROAD) as i32;
        state.road_cache2[i] = road;
        state.build_cache[i] = ((mask & tflag::BLOCKER_MASK) == tflag::BLOCKER_BUILDING) as i32;
        state.aqua_cache[i] = ((mask & tflag::RIVER != 0)
            || (mask & tflag::SURFACE_MASK == tflag::SURFACE_WATER))
            as i32;
        state.points_cache[i] = 0;
        if road == 0 {
            continue;
        }
        match candidate {
            CandidateFact::Present(current) if current.rotation != 10 => {
                state.points_cache[i] = (current.flags & ROAD_DIRECTION_MASK[i] != 0) as i32;
            }
            CandidateFact::Present(_) => {}
            CandidateFact::Absent | CandidateFact::Missing => {}
        }
    }
    missing
}

/// Direct child `TerrainOut::road_changed(tx,ty,0,0,1)` reached before most removals.
fn road_changed_remove(state: &mut RoadScanState, tx: i32, ty: i32) {
    let Some(fact) = state.candidates.get_mut(&(tx, ty)) else {
        return;
    };
    let CandidateFact::Present(candidate) = fact else {
        return;
    };
    if candidate.rotation == 10 {
        return;
    }
    if candidate.pending_camel_steps == 0 {
        candidate.ref_count = candidate.ref_count.wrapping_sub(1);
    } else {
        candidate.pending_camel_steps = candidate.pending_camel_steps.wrapping_sub(1);
    }
    if candidate.ref_count == 0 && candidate.pending_camel_steps == 0 {
        *fact = CandidateFact::Absent;
    }
}

fn clear_road(
    state: &mut RoadScanState,
    world: &mut World,
    tx: i32,
    ty: i32,
    notify_candidate: bool,
    trace: &mut RoadScanTrace,
) {
    if notify_candidate {
        trace.road_changed_calls = trace.road_changed_calls.wrapping_add(1);
        road_changed_remove(state, tx, ty);
    }
    let was_road = world.valid_t(tx, ty) && is_road(world, tx, ty);
    world.set_road_at(tx, ty, false, 0, false);
    if was_road {
        trace.roads_cleared = trace.roads_cleared.wrapping_add(1);
        state.last_cleared.push((tx, ty));
    }
}

/// `Roads::scan_and_kill_bad_tcoord` `0x0088E100`.
fn scan_bad_tcoord(
    state: &mut RoadScanState,
    world: &mut World,
    tx: i32,
    ty: i32,
    candidate: CandidateFact,
    trace: &mut RoadScanTrace,
) {
    let CandidateFact::Present(candidate) = candidate else {
        if state.road_cache2[0] != 0 {
            // This branch does not call TerrainOut::road_changed.
            clear_road(state, world, tx, ty, false, trace);
        }
        return;
    };
    if candidate.rotation == 10 {
        if state.road_cache2[0] != 0 {
            clear_road(state, world, tx, ty, false, trace);
        }
        return;
    }

    if candidate.is_terrain_creation || candidate.element_num == state.support_element {
        if state.road_cache2.iter().any(|&v| v != 0) {
            return;
        }
        clear_road(state, world, tx, ty, true, trace);
        return;
    }

    if state.road_cache2[0] == 0 {
        clear_road(state, world, tx, ty, true, trace);
        return;
    }

    let mut connections = 0;
    for i in 1..9 {
        if state.points_cache[i] == 0 {
            continue;
        }
        connections += 1;
        let nx = tx + DX[i];
        let ny = ty + DY[i];
        if !world.valid_t(nx, ny) || state.road_cache2[i] == 0 {
            clear_road(state, world, tx, ty, true, trace);
            return;
        }
    }
    if connections == 0 {
        clear_road(state, world, tx, ty, true, trace);
    }
}

/// `Roads::scan_and_kill_straggled_tcoord` `0x0088E050`.
fn scan_straggled_tcoord(
    state: &mut RoadScanState,
    world: &mut World,
    tx: i32,
    ty: i32,
    trace: &mut RoadScanTrace,
) {
    let mut connected = 0;
    let mut adjacent_aqua = 0;
    let mut adjacent_build = 0;
    for i in 1..9 {
        if state.points_cache[i] != 0 && state.road_cache2[i] == 0 {
            clear_road(state, world, tx, ty, true, trace);
            return;
        }
        connected += (state.points_cache[i] != 0) as i32;
        adjacent_aqua += (state.aqua_cache[i] != 0) as i32;
        adjacent_build += (state.build_cache[i] != 0) as i32;
    }
    if connected == 1 && adjacent_build == 0 && adjacent_aqua == 0 {
        clear_road(state, world, tx, ty, true, trace);
    }
}

/// Complete scheduled body at `0x008956A0`.
pub fn scan_and_kill_stray_roads(state: &mut RoadScanState, world: &mut World) -> RoadScanTrace {
    let mut trace = RoadScanTrace::default();
    state.last_cleared.clear();
    let passes = world.size / 500;
    if passes <= 0 {
        return trace;
    }

    for _ in 0..passes {
        state.curscan_x = state.curscan_x.wrapping_add(1);
        if state.curscan_x >= world.xs {
            state.curscan_x = 0;
            state.curscan_y = state.curscan_y.wrapping_add(1);
            if state.curscan_y >= world.ys {
                state.curscan_y = 0;
            }
        }
        trace.cells_scanned = trace.cells_scanned.wrapping_add(1);

        for tile in 0..16 {
            let tx = state.curscan_x * 4 + (tile & 3);
            let ty = state.curscan_y * 4 + (tile >> 2);
            trace.tiles_scanned = trace.tiles_scanned.wrapping_add(1);
            let candidate = if is_road(world, tx, ty) {
                state.candidate(tx, ty)
            } else {
                // Renderer candidates are allocated by road_changed. With no road bit and
                // no explicit supplied record, the sparse headless view proves absence.
                state
                    .candidates
                    .get(&(tx, ty))
                    .copied()
                    .unwrap_or(CandidateFact::Absent)
            };
            let missing = populate_caches(state, world, tx, ty, candidate);
            if missing {
                trace.missing_candidate_tiles = trace.missing_candidate_tiles.wrapping_add(1);
                continue;
            }

            scan_bad_tcoord(state, world, tx, ty, candidate, &mut trace);
            // Retail tests the cache populated before scan_bad, not the possibly-cleared
            // terrain bit. This can call the second child after the first removed the road.
            if state.road_cache2[0] != 0 {
                scan_straggled_tcoord(state, world, tx, ty, &mut trace);
            }
        }
    }
    trace
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world(xs: i32, ys: i32) -> World {
        World::init_default_rules(xs, ys)
    }

    fn candidate(flags: u32) -> CandidateFact {
        CandidateFact::Present(RoadCandidate {
            element_num: 7,
            flags,
            rotation: 0,
            ref_count: 1,
            ..RoadCandidate::default()
        })
    }

    #[test]
    fn retail_scans_size_div_five_hundred_and_advances_before_use() {
        let mut below = world(499, 1);
        let mut state = RoadScanState::default();
        let trace = scan_and_kill_stray_roads(&mut state, &mut below);
        assert_eq!(trace, RoadScanTrace::default());
        assert_eq!((state.curscan_x, state.curscan_y), (0, 0));

        let mut exact = world(25, 20);
        let trace = scan_and_kill_stray_roads(&mut state, &mut exact);
        assert_eq!(trace.cells_scanned, 1);
        assert_eq!(trace.tiles_scanned, 16);
        assert_eq!((state.curscan_x, state.curscan_y), (1, 0));
    }

    #[test]
    fn known_absent_candidate_clears_the_live_road_without_renderer_callback() {
        let mut world = world(25, 20);
        world.set_road_at(4, 0, true, 0, true);
        let mut state = RoadScanState::default();
        state.set_candidate(4, 0, CandidateFact::Absent);
        let trace = scan_and_kill_stray_roads(&mut state, &mut world);
        assert!(!is_road(&world, 4, 0));
        assert_eq!(trace.roads_cleared, 1);
        assert_eq!(trace.road_changed_calls, 0);
        assert_eq!(state.last_cleared, [(4, 0)]);
    }

    #[test]
    fn missing_candidate_suppresses_mutation_and_is_counted() {
        let mut world = world(25, 20);
        world.set_road_at(4, 0, true, 0, true);
        let mut state = RoadScanState::default();
        let trace = scan_and_kill_stray_roads(&mut state, &mut world);
        assert!(is_road(&world, 4, 0));
        assert_eq!(trace.roads_cleared, 0);
        assert!(trace.missing_candidate_tiles >= 1);
    }

    #[test]
    fn ordinary_candidate_without_connections_is_removed_and_released() {
        let mut world = world(25, 20);
        world.set_road_at(4, 0, true, 0, true);
        let mut state = RoadScanState::default();
        state.set_candidate(4, 0, candidate(0));
        let trace = scan_and_kill_stray_roads(&mut state, &mut world);
        assert!(!is_road(&world, 4, 0));
        assert_eq!(trace.roads_cleared, 1);
        assert_eq!(trace.road_changed_calls, 1);
        assert_eq!(state.candidate(4, 0), CandidateFact::Absent);
    }

    #[test]
    fn one_connected_road_is_not_straggled_when_a_building_supports_it() {
        let mut world = world(25, 20);
        world.set_road_at(4, 0, true, 0, true);
        world.set_road_at(5, 0, true, 0, true);
        world.set_building_at(4, 1, true);
        let mut state = RoadScanState::default();
        // Candidate flags belong to their own tile: (4,0) points EAST (index 4) and
        // (5,0) points WEST (index 8). The former fixture inverted these masks because
        // the old port incorrectly read each direction from the neighbouring candidate.
        state.set_candidate(4, 0, candidate(ROAD_DIRECTION_MASK[4]));
        state.set_candidate(5, 0, candidate(ROAD_DIRECTION_MASK[8]));
        let trace = scan_and_kill_stray_roads(&mut state, &mut world);
        assert!(is_road(&world, 4, 0));
        assert_eq!(trace.roads_cleared, 0);
    }

    #[test]
    fn lone_connection_without_build_or_water_is_straggled() {
        let mut world = world(25, 20);
        world.set_road_at(4, 0, true, 0, true);
        world.set_road_at(5, 0, true, 0, true);
        let mut state = RoadScanState::default();
        state.set_candidate(4, 0, candidate(ROAD_DIRECTION_MASK[4]));
        state.set_candidate(5, 0, candidate(ROAD_DIRECTION_MASK[4]));
        let trace = scan_and_kill_stray_roads(&mut state, &mut world);
        assert!(!is_road(&world, 4, 0));
        assert!(trace.roads_cleared >= 1);
    }

    #[test]
    fn out_of_bounds_arm_preserves_road_cache2_but_clears_the_legacy_slot() {
        let world = world(25, 20);
        let mut state = RoadScanState::default();
        state.road_cache2[1] = 9;
        state.legacy_road_cache[1] = 7;
        assert!(!populate_caches(
            &mut state,
            &world,
            0,
            0,
            CandidateFact::Absent,
        ));
        assert_eq!(state.road_cache2[1], 9);
        assert_eq!(state.legacy_road_cache[1], 0);
        assert_eq!(state.build_cache[1], 0);
        assert_eq!(state.points_cache[1], 0);
        assert_eq!(state.aqua_cache[1], 0);
    }

    #[test]
    fn all_direction_masks_belong_to_the_current_tile_candidate() {
        let mut world = world(25, 20);
        world.set_road_at(4, 0, true, 0, true);
        world.set_road_at(5, 0, true, 0, true);
        let current = candidate(ROAD_DIRECTION_MASK[4]);
        let mut state = RoadScanState::default();

        // The east neighbour's renderer record is deliberately missing. Retail still
        // obtains the east connection from the current (4,0) candidate's bit 28.
        assert!(!populate_caches(&mut state, &world, 4, 0, current));
        assert_eq!(state.points_cache[4], 1);

        // Supplying and then mutating the neighbour candidate cannot affect this cache.
        state.set_candidate(5, 0, candidate(0));
        assert!(!populate_caches(&mut state, &world, 4, 0, current));
        assert_eq!(state.points_cache[4], 1);
        state.set_candidate(5, 0, candidate(ROAD_DIRECTION_MASK[8]));
        assert!(!populate_caches(&mut state, &world, 4, 0, current));
        assert_eq!(state.points_cache[4], 1);
    }

    #[test]
    fn east_west_candidate_ownership_is_mutation_sensitive() {
        fn run(current_flags: u32, east_flags: u32) -> bool {
            let mut world = world(25, 20);
            world.set_road_at(4, 0, true, 0, true);
            world.set_road_at(5, 0, true, 0, true);
            // Prevent the exact one-ended straggler rule from obscuring the bad-road
            // connection decision under test.
            world.set_building_at(4, 1, true);
            let mut state = RoadScanState::default();
            state.set_candidate(4, 0, candidate(current_flags));
            state.set_candidate(5, 0, candidate(east_flags));
            scan_and_kill_stray_roads(&mut state, &mut world);
            is_road(&world, 4, 0)
        }

        // Current points east; the east candidate advertises nothing. Retail keeps center.
        assert!(run(ROAD_DIRECTION_MASK[4], 0));
        // Current points west into an empty tile; the east candidate's east bit cannot save it.
        assert!(!run(ROAD_DIRECTION_MASK[8], ROAD_DIRECTION_MASK[4]));
    }

    #[test]
    fn events_clear_per_call_and_camel_steps_are_consumed_before_refcount() {
        let mut state = RoadScanState::default();
        state.set_candidate(
            4,
            0,
            CandidateFact::Present(RoadCandidate {
                rotation: 0,
                ref_count: 1,
                pending_camel_steps: 1,
                ..RoadCandidate::default()
            }),
        );
        road_changed_remove(&mut state, 4, 0);
        let CandidateFact::Present(after_one) = state.candidate(4, 0) else {
            panic!("candidate released too early")
        };
        assert_eq!((after_one.ref_count, after_one.pending_camel_steps), (1, 0));
        road_changed_remove(&mut state, 4, 0);
        assert_eq!(state.candidate(4, 0), CandidateFact::Absent);

        state.last_cleared.push((1, 2));
        let mut too_small = world(10, 10);
        let trace = scan_and_kill_stray_roads(&mut state, &mut too_small);
        assert_eq!(trace.tiles_scanned, 0);
        assert!(state.last_cleared.is_empty());
    }
}
