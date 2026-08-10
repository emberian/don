//! Public reachability contract for ordinary RL episodes over `don_sim::tick::Sim`.

use don_env::{AuthoritativeEpisode, EpisodeError, ScenarioSpec, ScenarioUnit};
use don_sim::systems::map_terrain::COORD_PER_WCELL;

fn duel() -> ScenarioSpec {
    ScenarioSpec {
        seed: 0x51a7_2026,
        map_wcells: 8,
        // Deliberately reversed: activation is canonical player-slot order, while unit
        // allocation below remains declaration order.
        active_players: vec![1, 0],
        units: vec![
            ScenarioUnit {
                who: 1,
                type_id: 51,
                x: 6 * COORD_PER_WCELL,
                y: 6 * COORD_PER_WCELL,
                los_tiles: 4,
            },
            ScenarioUnit {
                who: 0,
                type_id: 50,
                x: COORD_PER_WCELL,
                y: COORD_PER_WCELL,
                los_tiles: 4,
            },
        ],
    }
}

#[test]
fn scenario_validation_fails_before_constructing_a_partial_episode() {
    let mut duplicate = duel();
    duplicate.active_players.push(0);
    assert!(matches!(
        AuthoritativeEpisode::from_spec(duplicate),
        Err(EpisodeError::DuplicatePlayer(0))
    ));

    let mut inactive = duel();
    inactive.active_players.retain(|&who| who != 1);
    assert!(matches!(
        AuthoritativeEpisode::from_spec(inactive),
        Err(EpisodeError::UnitOwnerInactive { unit: 0, who: 1 })
    ));

    let mut off_map = duel();
    off_map.units[0].x = i32::from(off_map.map_wcells) * COORD_PER_WCELL;
    assert!(matches!(
        AuthoritativeEpisode::from_spec(off_map),
        Err(EpisodeError::UnitOffMap { unit: 0, .. })
    ));
}

#[test]
fn reset_replays_seed_allocation_identity_and_state_exactly() {
    let mut episode = AuthoritativeEpisode::from_spec(duel()).unwrap();
    let handles = episode.spawned().to_vec();
    let initial_digest = episode.sim().world.digest();
    let initial_seed = episode.sim().map.world.seed;

    episode.step_frames(7);
    episode.reset().unwrap();

    assert_eq!(episode.spawned(), handles);
    assert_eq!(episode.sim().world.digest(), initial_digest);
    assert_eq!(episode.sim().map.world.seed, initial_seed);
    assert_eq!(episode.sim().world.frame, 0);
}

#[test]
fn identical_scenarios_drive_the_same_retail_ordered_tick_receipts() {
    let mut a = AuthoritativeEpisode::from_spec(duel()).unwrap();
    let mut b = AuthoritativeEpisode::from_spec(duel()).unwrap();

    let ar = a.step_frames(45);
    let br = b.step_frames(45);

    assert_eq!(ar, br);
    assert_eq!(a.sim().world.digest(), b.sim().world.digest());
    assert_eq!(ar.start_frame, 0);
    assert_eq!(ar.end_frame, 45);
    assert_eq!(a.sim().world.seconds, 3);
    // Ordinary frame 33 reaches the exact Step-12 visibility-producer preflight. This bounded
    // scenario supplies Units but no Build/Wall/reveal-fog authority, so the honest result is
    // the named `GameDaemonUpdateAllSeen` gap at top-level stage 12, with zero producer mutation.
    assert!(!ar.top_level_complete());
    assert_eq!(ar.unimplemented[12], 1);
    assert_eq!(ar.unimplemented.iter().sum::<u64>(), 1);
    // Step 14 is Objects::process_all and must actually visit the two declared units.
    assert_eq!(ar.executed[14], 45);
    assert!(ar.work[14] >= 90);
    // Step 20 increments Game::frame once per retail frame.
    assert_eq!(ar.executed[20], 45);
    assert_eq!(ar.work[20], 45);
}

#[test]
fn step_receipt_keeps_zero_frame_calls_observational() {
    let mut episode = AuthoritativeEpisode::from_spec(duel()).unwrap();
    let digest = episode.sim().world.digest();
    let receipt = episode.step_frames(0);

    assert_eq!(receipt.start_frame, 0);
    assert_eq!(receipt.end_frame, 0);
    assert_eq!(receipt.frames, 0);
    assert!(receipt.top_level_complete());
    assert_eq!(episode.sim().world.digest(), digest);
}
