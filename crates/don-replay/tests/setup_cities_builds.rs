//! Exact starting City/center-Build producer and replay-harness integration.

use don_replay::checksum::Channel;
use don_replay::harness::{Simulation, WorldSim};
use don_replay::replay::{corpus, Replay};
use don_replay::setup_cities_builds::{
    SetupCitiesError, StartingRegionEvidence, StartingSetupState,
};
use don_replay::wire::{classify, CommandClass, CommandView};
use don_sim::systems::map_terrain::{tflag, Coord, TCoord};
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn two_human_checksum_replays() -> Vec<Replay> {
    corpus(&repo_root())
        .into_iter()
        .filter_map(|path| Replay::open(&path).ok())
        .filter(|rep| {
            rep.turns.iter().any(|turn| turn.any_checksums().is_some())
                && rep.initial.active_players().count() == 2
                && rep
                    .initial
                    .active_players()
                    .all(|player| player.flags & 4 != 0)
        })
        .collect()
}

#[test]
#[ignore = "diagnostic while deriving the setup producer"]
fn print_two_human_setup_inputs() {
    let reps = two_human_checksum_replays();
    assert_eq!(reps.len(), 5);
    for rep in reps {
        let sim = WorldSim::from_replay(&rep);
        let first = rep
            .turns
            .iter()
            .find_map(|turn| turn.any_checksums().map(|(_, sums)| (turn.turn, sums)))
            .unwrap();
        let map = sim.initial_world.as_ref().unwrap();
        eprintln!(
            "{} seed={:08x} settings={:?} players={:?} game_starts={:?}/{:?}/{} starts={:?}/{:?} regions={:?} first={} builds={:08x} cities={:08x} boundary={:?}",
            rep.path.file_name().unwrap().to_string_lossy(),
            rep.initial.info.seed,
            rep.initial.info.settings,
            rep.initial
                .active_players()
                .map(|p| (p.slot, p.who, p.team, p.tribe, p.flags, p.play))
                .collect::<Vec<_>>(),
            rep.initial.game.start_list,
            rep.initial.game.start_index,
            rep.initial.game.num_players,
            map.world.start_x.items,
            map.world.start_y.items,
            map.world
                .start_x
                .items
                .iter()
                .zip(&map.world.start_y.items)
                .map(|(&x, &y)| map.world.wdata(x, y).region)
                .collect::<Vec<_>>(),
            first.0,
            first.1.get(Channel::Builds),
            first.1.get(Channel::Cities),
            sim.initial_items.as_ref().unwrap().boundary,
        );
    }
}

#[test]
#[ignore = "diagnostic for replay-carried setup assignment evidence"]
fn print_setup_command_coordinates() {
    let paths = [
        "Playback___2018.11.17_13_21_42__Sat_.rcx",
        "Playback___2020.02.08_10_49_15__Sat_.rcx",
        "Playback___2020.02.21_09_48_48__Fri_.rcx",
        "Playback___2024.02.23_20_49_35__Fri_.rcx",
        "Playback___2024.03.29_21_52_57__Fri_.rcx",
    ]
    .map(|name| repo_root().join("ron-data/replays/multi").join(name));
    for rep in paths.iter().map(|path| Replay::open(path).unwrap()) {
        eprintln!(
            "\n{} game={:?}/{:?}/{}",
            rep.path.file_name().unwrap().to_string_lossy(),
            rep.initial.game.start_list,
            rep.initial.game.start_index,
            rep.initial.game.num_players
        );
        let mut printed = 0;
        'turns: for turn in &rep.turns {
            for player in &turn.players {
                for command in &player.commands {
                    if classify(command.opcode) != CommandClass::Sim && command.opcode != 0x48 {
                        continue;
                    }
                    let view = CommandView::new(command.opcode, &command.bytes);
                    let fields: Vec<_> = view
                        .fields()
                        .iter()
                        .filter_map(|field| view.read(field).map(|value| (field.name, value)))
                        .collect();
                    if !fields.iter().any(|(name, _)| {
                        matches!(
                            *name,
                            "x" | "y" | "to_x" | "to_y" | "from_x" | "from_y" | "x_loc" | "y_loc"
                        )
                    }) {
                        continue;
                    }
                    eprintln!(
                        "  turn={} play={} {}({:#04x}) {:?}",
                        turn.turn,
                        player.play,
                        view.struct_name(),
                        command.opcode,
                        fields
                    );
                    printed += 1;
                    if printed == 24 {
                        break 'turns;
                    }
                }
            }
        }
    }
}

#[test]
fn all_land_two_human_setups_install_a_frozen_sim_owned_cities_producer() {
    let names = [
        "Playback___2018.11.17_13_21_42__Sat_.rcx",
        "Playback___2020.02.08_10_49_15__Sat_.rcx",
        "Playback___2020.02.21_09_48_48__Fri_.rcx",
    ];
    for name in names {
        let path = repo_root().join("ron-data/replays/multi").join(name);
        let rep = Replay::open(&path).unwrap();
        assert_eq!(rep.initial.game.start_list, [0; 8]);
        assert_eq!(rep.initial.game.start_index, [0; 8]);
        assert_eq!(rep.initial.game.num_players, 0);

        let mut sim = WorldSim::from_replay(&rep);
        if let Some(error) = &sim.initial_setup_error {
            panic!("{name}: setup refused: {error}");
        }
        let setup = sim.initial_setup.as_ref().expect("ordinary setup owner");
        assert_eq!(setup.receipt.active_players, 2);
        assert_eq!(setup.receipt.cities.len(), 2);
        assert_eq!(setup.receipt.world_city_masks.len(), 2);
        assert_eq!(
            setup.receipt.region_evidence,
            StartingRegionEvidence::AllLandSingleComponent
        );
        assert!(setup.receipt.cities.iter().all(|city| {
            city.region == 1
                && city.snapped_position.0 == city.start_wcoord.0 * 0x300 + 0x60
                && city.snapped_position.1 == city.start_wcoord.1 * 0x300 + 0x60
                && city.constructor.city_init_complete
                && !city.constructor.build_init_complete
                && !city.constructor.build_activation_complete
                && city.build.owner == city.owner
                && city.build.object_id == 2000
                && city.build.body_object_id == 2000
                && city.build.type_index == don_sim::systems::tech_cities::ty::VILLAGE
                && city.build.registered_ptype == don_sim::systems::tech_cities::ty::VILLAGE
                && city.build.snapped_position == city.snapped_position
                && city.build.body_position == city.snapped_position
                && city.constructor.object_id == 2000
                && city.constructor.current_type == don_sim::systems::tech_cities::ty::VILLAGE
                && city.constructor.position == city.snapped_position
                && city.constructor.flags_after_activation == 0x27
                && city.constructor.city.city == city.city_slot
                && city.constructor.city.o == 2000
                && city.constructor.city.reg == city.region
                && city.constructor.city.who == city.owner as i8
        }));
        for (city, mask) in setup
            .receipt
            .cities
            .iter()
            .zip(&setup.receipt.world_city_masks)
        {
            let center = (
                TCoord::from_coord(Coord(city.snapped_position.0)).0,
                TCoord::from_coord(Coord(city.snapped_position.1)).0,
            );
            assert_eq!(mask.request.owner, city.owner);
            assert_eq!(mask.request.object_id, city.build.object_id);
            assert_eq!(mask.request.center_tcoord, center);
            assert_eq!(
                mask.request.radius_tiles,
                city.constructor.world_fix.radius_tiles
            );
            let expected_cells = match mask.request.radius_tiles {
                20 => 1_232,
                24 => 1_788,
                radius => panic!("unexpected starting City radius {radius}"),
            };
            assert_eq!(mask.table_endpoint, expected_cells);
            assert_eq!(mask.in_bounds_offsets as usize, expected_cells);
            assert_eq!(mask.out_of_bounds_offsets, 0);
            assert!(mask.city_mask_complete);
            assert!(mask.pre_strategy.schedule_attested);
            assert!(mask.pre_strategy.activation_mask_inputs_joined);
            assert!(!mask.pre_strategy.final_territory_image_joined);
            assert!(!mask.pre_strategy.unit_derived_city_fields_joined);
            assert_ne!(
                setup.sim.map.world.tmask(center.0, center.1) & tflag::CITY,
                0
            );
        }
        assert_eq!(setup.receipt.constructor_cities.cities_walked, 2);
        assert_eq!(setup.receipt.constructor_cities.bytes_walked, 228);
        assert!(!setup.receipt.first_checksum_city_image_ready);
        assert!(!setup.receipt.builds_channel_ready);
        assert!(setup.channels().builds.is_none());
        assert!(setup.channels().cities.is_none());

        let (ours, evidence) = sim.check_all_with_evidence();
        assert_eq!(setup.sim.cities.slots, setup.cities.slots);
        assert_eq!(setup.sim.cities.city_mark, setup.cities.city_mark);
        assert_eq!(
            ours.get(Channel::Cities),
            setup.receipt.constructor_cities.checksum
        );
        assert!(evidence[Channel::Cities as usize].installed);
        assert!(evidence[Channel::Cities as usize].exact_producer);
        assert!(evidence[Channel::Cities as usize].walk_complete);
        assert_eq!(evidence[Channel::Cities as usize].bytes_walked, 228);
        assert_eq!(evidence[Channel::Cities as usize].unsourced_walked, 0);
        assert!(!evidence[Channel::Builds as usize].installed);

        sim.step_turn(0);
        let (_, expired) = sim.check_all_with_evidence();
        assert!(sim.initial_setup.is_none(), "the atomic setup pair expired");
        assert!(
            sim.frozen_initial_cities.is_some(),
            "only the canonical Cities owner survives"
        );
        assert!(expired[Channel::Cities as usize].installed);
        assert!(expired[Channel::Cities as usize].exact_producer);
        assert_eq!(expired[Channel::Cities as usize].bytes_walked, 228);
        assert!(!expired[Channel::Builds as usize].installed);
    }
}

#[test]
fn setup_refuses_unproven_region_extra_town_and_corrupt_camera() {
    let multi = repo_root().join("ron-data/replays/multi");
    let great_lakes =
        Replay::open(&multi.join("Playback___2024.02.23_20_49_35__Fri_.rcx")).unwrap();
    let gl_sim = WorldSim::from_replay(&great_lakes);
    assert!(matches!(
        gl_sim.initial_setup_error,
        Some(SetupCitiesError::UnresolvedStartRegion { map_style: 14 })
    ));

    let town_two = Replay::open(&multi.join("Playback___2024.03.29_21_52_57__Fri_.rcx")).unwrap();
    let town_two_sim = WorldSim::from_replay(&town_two);
    assert!(matches!(
        town_two_sim.initial_setup_error,
        Some(SetupCitiesError::UnsupportedStartingTown { starting_town: 2 })
    ));

    let mut corrupt =
        Replay::open(&multi.join("Playback___2018.11.17_13_21_42__Sat_.rcx")).unwrap();
    let camera = corrupt.turns[0].players[0]
        .commands
        .iter_mut()
        .find(|command| command.opcode == 0x48)
        .unwrap();
    camera.bytes[2..6].copy_from_slice(&0_i32.to_le_bytes());
    let map = corrupt.initial.reconstruct_world().unwrap();
    assert!(matches!(
        StartingSetupState::derive(&corrupt, &map),
        Err(SetupCitiesError::CameraNotVillageCenter { .. })
    ));
}
