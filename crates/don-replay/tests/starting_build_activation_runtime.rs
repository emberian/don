pub mod build_init_prefix {
    pub use don_replay::build_init_prefix::*;
}
pub mod builds_runtime {
    pub use don_replay::builds_runtime::*;
}
pub mod city_build_constructor_runtime {
    pub use don_replay::city_build_constructor_runtime::*;
}

#[path = "../src/starting_build_activation_runtime.rs"]
mod subject;

use build_init_prefix::{BuildInitPrefixRequest, BuildTypeInitFacts};
use builds_runtime::{check_sim_builds, BuildsWalkAuthority};
use city_build_constructor_runtime::{
    apply_fresh_starting_village_projection, FreshStartingVillageReceipt,
    FreshStartingVillageRequest,
};
use don_replay::build_spawn_runtime::{spawn_canonical_build, CanonicalBuildSpawnRequest};
use don_sim::objects::BUILD_BAND_BASE;
use don_sim::systems::map_terrain::{tflag, COORD_PER_WCELL};
use don_sim::systems::production::{self, BuildData, BuildQueueEntry};
use don_sim::tick::Sim;
use subject::{
    complete_starting_village_build, StartingBuildStage, StartingVillageBuildActivationError,
    StartingVillageBuildSourceFacts, BUILD_ACTIVATE_VA, BUILD_DATA_WALK_VA, BUILD_INIT_VA,
    BUILD_PROCESS_VA, BUILD_QUEUE_INIT_VA, BUILD_TYPE_MASK_ME_VA, LEADER_PROCESS_ALL_VA,
    OBJECT_ADD_TO_WORLD_VA, OBJECT_UPDATE_SEEN_ALLY_VA, OBJECT_UPDATE_SEEN_VA,
    STARTING_BUILD_STAGE_ORDER, STARTING_VILLAGE_BUILD_MASK, STARTING_VILLAGE_FINAL_FLAGS,
    STARTING_VILLAGE_FOOTPRINT, STARTING_VILLAGE_NATIVE_MASK_CITY_FLAGS,
    STARTING_VILLAGE_QUEUE_ROWS, STARTING_VILLAGE_TYPE, STARTING_VILLAGE_WALK_BYTES,
    WALL_ACTIVATE_VA, WALL_INIT_VA, WALL_MASK_CITY_VA, WALL_MASK_ME_VA, WALL_START_VA,
    WALL_UPDATE_HITS_VA, WALL_UPDATE_LOS_VA, WDATA_BUILD_MASK, WORLD_RESIDUALS,
};

const OWNER: u8 = 2;
const CITY_SLOT: i16 = 0;
const POSITION: (i32, i32) = (30 * COORD_PER_WCELL + 96, 31 * COORD_PER_WCELL + 96);

fn staged_center() -> BuildData {
    BuildData {
        flags: production::flag::VALID | 0x20,
        orig_type: STARTING_VILLAGE_TYPE,
        city: -1,
        city_down: -1,
        wonder: -1,
        dock: -1,
        attack_ox: -1,
        attack_whom: -1,
        founder: OWNER as i8,
        ..BuildData::default()
    }
}

fn setup_constructor() -> (Sim, FreshStartingVillageReceipt) {
    let mut sim = Sim::new(0x51de, 64);
    let center = sim.map.world.w_index(30, 31);
    sim.map.world.wdata[center].region = 1;
    let spawn = spawn_canonical_build(
        &mut sim,
        CanonicalBuildSpawnRequest {
            owner: OWNER,
            type_index: STARTING_VILLAGE_TYPE,
            snapped_x: POSITION.0,
            snapped_y: POSITION.1,
            build: staged_center(),
        },
    )
    .unwrap();
    assert_eq!((spawn.row, spawn.object_id), (0, BUILD_BAND_BASE as i16));
    let receipt = apply_fresh_starting_village_projection(
        &mut sim.builds[spawn.row],
        &mut sim.map.world,
        FreshStartingVillageRequest {
            owner: OWNER,
            city_slot: CITY_SLOT,
            current_type: STARTING_VILLAGE_TYPE,
            city_name: String::new(),
            city_id: String::new(),
            indian_radius_bonus: false,
        },
    )
    .unwrap();
    (sim, receipt)
}

fn prefix_request(sim: &Sim) -> BuildInitPrefixRequest {
    BuildInitPrefixRequest {
        owner: OWNER,
        object_id: BUILD_BAND_BASE as i16,
        type_index: STARTING_VILLAGE_TYPE,
        type_rows: sim.production_runtime.types.len(),
        snapped_x: POSITION.0,
        snapped_y: POSITION.1,
        terrain_z: 0x126,
        owner_uid_before: 17,
        max_age_source_byte: 0x66,
        type_facts: BuildTypeInitFacts {
            sets_flat_flag: true,
            sets_detector_flag: false,
        },
    }
}

fn source_facts() -> StartingVillageBuildSourceFacts {
    let owner_bit = 1 << OWNER;
    StartingVillageBuildSourceFacts {
        wall_init_full_hits: 1_200,
        first_checkpoint_full_hits: 1_200,
        construct_time: 1_800,
        first_checkpoint_los: 12,
        stance: 0,
        wall_init_ever_seen: 0,
        wall_init_ever_seen_completed: 0,
        first_checkpoint_ever_seen: owner_bit,
        first_checkpoint_ever_seen_completed: owner_bit,
        setup_game_frame: 0,
        active_build_process_frames: 1,
        can_carry_air: false,
        gathers_from_terrain: false,
        indian_radius_bonus: false,
    }
}

#[test]
fn shipped_call_chain_and_temporal_order_are_pinned() {
    assert_eq!(BUILD_INIT_VA, 0x0062_9740);
    assert_eq!(OBJECT_ADD_TO_WORLD_VA, 0x0064_d8c0);
    assert_eq!(WALL_INIT_VA, 0x0063_e9b0);
    assert_eq!(BUILD_QUEUE_INIT_VA, 0x0063_07d0);
    assert_eq!(WALL_START_VA, 0x0063_e810);
    assert_eq!(WALL_MASK_ME_VA, 0x0064_2fc0);
    assert_eq!(BUILD_TYPE_MASK_ME_VA, 0x0063_12a0);
    assert_eq!(WALL_MASK_CITY_VA, 0x0063_e310);
    assert_eq!(WALL_ACTIVATE_VA, 0x0063_e4b0);
    assert_eq!(BUILD_ACTIVATE_VA, 0x0062_3e20);
    assert_eq!(OBJECT_UPDATE_SEEN_VA, 0x0065_1b80);
    assert_eq!(OBJECT_UPDATE_SEEN_ALLY_VA, 0x0065_37a0);
    assert_eq!(LEADER_PROCESS_ALL_VA, 0x006e_d2a0);
    assert_eq!(WALL_UPDATE_HITS_VA, 0x0063_f0d0);
    assert_eq!(WALL_UPDATE_LOS_VA, 0x0063_eeb0);
    assert_eq!(BUILD_PROCESS_VA, 0x0061_edf0);
    assert_eq!(BUILD_DATA_WALK_VA, 0x0062_f270);
    assert_eq!(
        STARTING_BUILD_STAGE_ORDER[0],
        StartingBuildStage::CanonicalOwnerValidated
    );
    assert_eq!(
        STARTING_BUILD_STAGE_ORDER.last(),
        Some(&StartingBuildStage::BuildWalkComplete)
    );
}

#[test]
fn complete_row_owns_first_checkpoint_build_walk_and_installs_authority() {
    let (mut sim, constructor) = setup_constructor();
    let prefix = prefix_request(&sim);
    let mut authority = BuildsWalkAuthority::default();
    let receipt = complete_starting_village_build(
        &mut sim,
        &mut authority,
        0,
        &constructor,
        prefix,
        source_facts(),
    )
    .unwrap();

    assert_eq!(receipt.row, 0);
    assert_eq!(receipt.owner, OWNER);
    assert_eq!(receipt.object_id, BUILD_BAND_BASE as i16);
    assert_eq!(receipt.current_type, STARTING_VILLAGE_TYPE);
    assert_eq!(receipt.city_slot, CITY_SLOT);
    assert_eq!(receipt.stages, STARTING_BUILD_STAGE_ORDER);
    assert!(receipt.build_init_complete);
    assert!(receipt.build_activation_complete);
    assert!(receipt.first_checkpoint_build_image_ready);
    assert!(receipt.build_walk_authority_installed);
    assert!(!receipt.world_channel_ready);
    assert_eq!(receipt.world_residuals, WORLD_RESIDUALS);

    let build = &sim.builds[0];
    assert_eq!(build.flags, STARTING_VILLAGE_FINAL_FLAGS);
    assert_eq!(build.build_masks, STARTING_VILLAGE_BUILD_MASK);
    assert_eq!(build.city, CITY_SLOT);
    assert_eq!(build.founder, OWNER as i8);
    assert_eq!(build.orig_type, STARTING_VILLAGE_TYPE);
    assert_eq!(build.myhits, 1_200);
    assert_eq!(build.construct_hits, 1_200);
    assert_eq!(build.constr_time, 1_800);
    assert_eq!(build.frame_started, 0);
    assert_eq!(build.queue.queued, 0);
    assert_eq!(build.queue.entries.len(), STARTING_VILLAGE_QUEUE_ROWS);
    assert_eq!(build.queue.entries[0].type_index, -1);
    assert!(build.queue.entries[1..]
        .iter()
        .all(|entry| *entry == BuildQueueEntry::default()));
    assert!(build.gather_from.tiles.is_empty());
    assert_eq!((build.gather_from.mtn, build.gather_from.cliff), (-1, -1));
    assert!(build.gather.is_empty());
    assert_eq!(receipt.wall_init_construct_hits, 21);
    assert_eq!(receipt.first_checkpoint_construct_hits, 1_200);
    assert_eq!(receipt.queue.walked_bytes, 20 * 18);
    assert_eq!(receipt.walk.bytes_walked, STARTING_VILLAGE_WALK_BYTES);

    let channel = check_sim_builds(&sim, &authority).unwrap();
    assert_eq!(channel.registry_entries, 1);
    assert_eq!(channel.builds_walked, 1);
    assert_eq!(channel.bytes_walked, STARTING_VILLAGE_WALK_BYTES);
    assert_eq!(channel.checksum, receipt.walk.checksum);
}

#[test]
fn object_link_and_blockers_land_while_city_disc_stays_a_typed_request() {
    let (mut sim, constructor) = setup_constructor();
    let prefix = prefix_request(&sim);
    let mut authority = BuildsWalkAuthority::default();
    let city_bits_before = sim
        .map
        .world
        .tdata
        .iter()
        .filter(|mask| **mask & tflag::CITY != 0)
        .count();
    let receipt = complete_starting_village_build(
        &mut sim,
        &mut authority,
        0,
        &constructor,
        prefix,
        source_facts(),
    )
    .unwrap();

    let center = sim.map.world.w_index(30, 31);
    assert_eq!(
        (
            sim.map.world.wdata[center].down,
            sim.map.world.wdata[center].down_who
        ),
        (BUILD_BAND_BASE as i16, OWNER as i16)
    );
    assert_ne!(sim.map.world.wdata[center].flags & WDATA_BUILD_MASK, 0);
    assert_eq!(receipt.object_link.wdata_head_before, (-1, 0));
    assert_eq!(
        receipt.object_link.wdata_head_after,
        (BUILD_BAND_BASE as i16, OWNER as i16)
    );
    assert_eq!(
        (
            receipt.object_link.object_up,
            receipt.object_link.object_down,
            receipt.object_link.object_down_who
        ),
        (-1, -1, 0)
    );

    let (corner_x, corner_y) = receipt.footprint.corner_tcoord;
    let blockers = STARTING_VILLAGE_FOOTPRINT
        .tiles(corner_x, corner_y)
        .into_iter()
        .filter(|&(tx, ty)| {
            sim.map.world.tdata[sim.map.world.t_index(tx, ty)] & tflag::BLOCKER_MASK
                == tflag::BLOCKER_BUILDING
        })
        .count();
    assert_eq!(blockers, 49);
    assert_eq!(receipt.footprint.footprint_tiles, 49);
    assert_eq!(
        receipt.wall_mask_city.center_tcoord,
        (
            production::tile_of(POSITION.0),
            production::tile_of(POSITION.1)
        )
    );
    assert_eq!(receipt.wall_mask_city.radius_tiles, 20);
    assert_eq!(receipt.wall_mask_city.on, 1);
    assert_eq!(receipt.wall_mask_city.owner, OWNER);
    assert_eq!(receipt.wall_mask_city.object_id, BUILD_BAND_BASE as i16);
    assert_eq!(
        receipt.wall_mask_city.native_call_flags,
        STARTING_VILLAGE_NATIVE_MASK_CITY_FLAGS
    );
    assert_eq!(receipt.wall_mask_city.native_call_city, -1);
    assert_eq!(
        receipt.wall_mask_city.post_activation_flags,
        STARTING_VILLAGE_FINAL_FLAGS
    );
    assert!(receipt.wall_mask_city.city_flag_set());
    assert_eq!(
        sim.map
            .world
            .tdata
            .iter()
            .filter(|mask| **mask & tflag::CITY != 0)
            .count(),
        city_bits_before
    );
    assert!(receipt.footprint.blocker_kind_complete);
    assert!(!receipt.footprint.world_channel_ready);
}

#[test]
fn source_and_world_refusals_publish_neither_row_world_nor_authority() {
    let (mut sim, constructor) = setup_constructor();
    let prefix = prefix_request(&sim);
    let before_build = sim.builds[0].image();
    let before_world = sim.map.world.checksum_sections();
    let mut authority = BuildsWalkAuthority::default();
    let mut bad = source_facts();
    bad.can_carry_air = true;
    let error =
        complete_starting_village_build(&mut sim, &mut authority, 0, &constructor, prefix, bad)
            .unwrap_err();
    assert_eq!(
        error,
        StartingVillageBuildActivationError::InvalidSourceFacts
    );
    assert_eq!(sim.builds[0].image(), before_build);
    assert_eq!(sim.map.world.checksum_sections(), before_world);
    assert!(authority.get(0).is_none());

    let center = sim.map.world.w_index(30, 31);
    sim.map.world.wdata[center].down = 77;
    sim.map.world.wdata[center].down_who = 4;
    let before_linked_build = sim.builds[0].image();
    let before_linked_world = sim.map.world.checksum_sections();
    let error = complete_starting_village_build(
        &mut sim,
        &mut authority,
        0,
        &constructor,
        prefix,
        source_facts(),
    )
    .unwrap_err();
    assert_eq!(
        error,
        StartingVillageBuildActivationError::CenterWorldCellAlreadyLinked {
            down: 77,
            down_who: 4
        }
    );
    assert_eq!(sim.builds[0].image(), before_linked_build);
    assert_eq!(sim.map.world.checksum_sections(), before_linked_world);
    assert!(authority.get(0).is_none());
}

#[test]
fn visibility_must_include_the_owner_after_activation() {
    let (mut sim, constructor) = setup_constructor();
    let prefix = prefix_request(&sim);
    let mut facts = source_facts();
    facts.first_checkpoint_ever_seen = 0;
    facts.first_checkpoint_ever_seen_completed = 0;
    let mut authority = BuildsWalkAuthority::default();
    let error =
        complete_starting_village_build(&mut sim, &mut authority, 0, &constructor, prefix, facts)
            .unwrap_err();
    assert!(matches!(
        error,
        StartingVillageBuildActivationError::MissingOwnerVisibility { .. }
    ));
    assert!(authority.get(0).is_none());
}
