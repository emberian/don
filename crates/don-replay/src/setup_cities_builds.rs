//! Atomic reconstruction of the ordinary starting City centers.
//!
//! The replay header precedes `Setup::build_game`: its serialized `Game::start_list`,
//! `start_index`, and `num_players` bytes are still zero and are therefore not assignment
//! evidence.  The first command group supplies a stronger source. `Game::zoom_to_first_unit`
//! centers each local camera on that Leader's first valid object (the center Build at object
//! id 2000), and the player's first `CameraCommand` carries the resulting exact x/y pair.
//! This owner joins that replay-carried position to the unique initial `Player::play`/Leader
//! identity. No recorded checksum participates in assignment.
//!
//! The transaction creates the center Build identity required by `City::walk_data`, links
//! the exact fresh `CityRecord`, and validates the constructor-time Cities walk against the
//! canonical Sim/registry owners. It deliberately does not promote either replay channel:
//! the Build body still stops before the inherited initializer is complete, and retail's
//! first `Game::do_frame` calls `Leader::plan_strategy`, which recomputes walked City bytes
//! `+0x62..+0x71` from WData before the first recorded checksum. Keeping the constructor
//! value separate from the installable pair makes that temporal boundary fail closed.

#![forbid(unsafe_code)]

use std::fmt;

use don_sim::systems::production::{self, BuildData};
use don_sim::systems::tech_cities::{self, CityPool};
use don_sim::tick::Sim;

use crate::build_spawn_runtime::{
    spawn_canonical_build, CanonicalBuildSpawnError, CanonicalBuildSpawnReceipt,
    CanonicalBuildSpawnRequest,
};
use crate::builds_runtime::{BuildWalkFacts, BuildsChannelValue, BuildsWalkAuthority};
use crate::cities_runtime::{check_sim_cities, CitiesChannelValue, CitiesRuntimeError};
use crate::city_build_constructor_runtime::{
    apply_fresh_starting_village_projection, FreshStartingVillageReceipt,
    FreshStartingVillageRequest, StartingCityConstructorError,
};
use crate::initial::{InitialState, InitialWorld};
use crate::replay::Replay;
use crate::wire::CommandView;

pub const SETUP_BUILD_GAME_VA: u32 = 0x005a_c190;
pub const SETUP_BUILD_EMPIRE_VA: u32 = 0x005a_bb80;
pub const SETUP_BUILD_CITIES_VA: u32 = 0x005a_b910;
pub const BUILD_SNAP_CENTER_VA: u32 = 0x0063_6190;
pub const CITY_INIT_VA: u32 = 0x0073_7050;
pub const BUILD_ACTIVATE_VA: u32 = 0x0062_3e20;
pub const GAME_ZOOM_TO_FIRST_UNIT_VA: u32 = 0x0058_db30;
pub const CAMERA_SET_LOC_VA: u32 = 0x0084_4a60;
pub const CAMERA_COMMAND_OPCODE: u8 = 0x48;
pub const REGIONS_FIND_ALL_VA: u32 = 0x006a_0780;
pub const LEADER_PLAN_STRATEGY_VA: u32 = 0x006b_9620;
pub const LEADERS_STRATEGY_ALL_VA: u32 = 0x006e_d430;
pub const GAME_DO_FRAME_VA: u32 = 0x0059_1ef0;

pub const PLAYER_FLAG_PRESENT: u16 = 0x0001;
pub const PLAYER_FLAG_HUMAN: u16 = 0x0004;
pub const PLAYER_FLAG_OBSERVER: u16 = 0x0080;
pub const UNTEAMED: u8 = 8;
pub const CITY_CENTER_TYPE: i32 = tech_cities::ty::VILLAGE;
pub const WORLD_TO_COORD: i32 = 0x300;
/// Village is 7x7 tiles, so `BuildTypeData::snap_center` adds half a tile on each axis.
pub const VILLAGE_CENTER_OFFSET: i32 = 0x60;

/// Source for the exact position installed in the center Build and City.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartingPositionEvidence {
    /// First command group, one `CameraCommand` for the exact `Player::play` identity.
    FirstCameraOnObject2000,
}

/// Source for the City region byte read by `City::init` from WData.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartingRegionEvidence {
    /// Old World and Himalayas invert the wiped world to one all-land component. The
    /// common `Regions::find_all` pass assigns that sole land component id 1.
    AllLandSingleComponent,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartingCityReceipt {
    pub owner: u8,
    pub replay_player_slot: u8,
    pub replay_play: i32,
    pub camera_turn: i32,
    pub camera_stamp: u32,
    pub start_wcoord: (i32, i32),
    pub snapped_position: (i32, i32),
    pub region: i16,
    pub city_slot: i16,
    pub build: CanonicalBuildSpawnReceipt,
    pub constructor: FreshStartingVillageReceipt,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartingSetupReceipt {
    pub position_evidence: StartingPositionEvidence,
    pub region_evidence: StartingRegionEvidence,
    pub active_players: usize,
    pub city_flags: u16,
    pub cities: Vec<StartingCityReceipt>,
    /// Exact constructor-time walk, before frame-zero strategy recomputes the City census.
    pub constructor_cities: CitiesChannelValue,
    pub first_checksum_city_image_ready: bool,
    pub builds_channel_ready: bool,
}

/// Exact channel values admitted by the completed transaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StartingSetupChannels {
    /// These are an inseparable pair. State integration installs neither unless both are
    /// present after the complete setup and frame-zero census transaction.
    pub cities: Option<CitiesChannelValue>,
    pub builds: Option<BuildsChannelValue>,
}

/// Canonical fresh-constructor state retained until the first simulation mutation.
pub struct StartingSetupState {
    pub sim: Sim,
    pub cities: CityPool,
    pub build_walk: BuildsWalkAuthority,
    pub receipt: StartingSetupReceipt,
    channels: StartingSetupChannels,
}

impl StartingSetupState {
    pub fn channels(&self) -> StartingSetupChannels {
        self.channels
    }

    /// Derive one ordinary, all-human, unteamed setup as a local transaction.
    ///
    /// Every fallible operation targets local staged owners. Returning `Err` therefore
    /// publishes neither a partial CityPool nor a Build/registry append.
    pub fn derive(replay: &Replay, map: &InitialWorld) -> Result<Self, SetupCitiesError> {
        let initial = &replay.initial;
        validate_top_level(initial, map)?;
        let (region, region_evidence) = start_region(initial.info.settings.map_style)?;
        let assignments = derive_assignments(replay, map, region)?;

        let wcells = u16::try_from(map.world.xs)
            .map_err(|_| SetupCitiesError::MapDimensionOutOfRange { xs: map.world.xs })?;
        if i32::from(wcells) != map.world.xs || map.world.xs != map.world.ys {
            return Err(SetupCitiesError::MapDimensionOutOfRange { xs: map.world.xs });
        }
        let mut sim = Sim::new(u64::from(initial.info.seed), wcells);
        sim.map.world = map.world.clone();
        let mut cities = CityPool::new();
        let mut build_walk = BuildsWalkAuthority::default();
        let mut receipts = Vec::with_capacity(assignments.len());

        for assignment in assignments {
            let owner = usize::from(assignment.owner);
            sim.activate(owner);

            let mut staged_build = BuildData {
                // `SubObject::init` owns VALID and the BuildType city predicate owns
                // 0x20. `apply_fresh_starting_village_projection` performs the later
                // Wall/Build activation writes only after canonical identity exists.
                flags: production::flag::VALID | 0x20,
                orig_type: CITY_CENTER_TYPE,
                city: -1,
                city_down: -1,
                wonder: -1,
                dock: -1,
                attack_ox: -1,
                attack_whom: -1,
                founder: assignment.owner as i8,
                ..BuildData::default()
            };
            // The opaque body remains explicitly non-authoritative for Builds. Keeping
            // the ordinary sentinels here prevents the City join from concealing a linked
            // or invalid center identity.
            staged_build.queue.queued = 0;
            let build = spawn_canonical_build(
                &mut sim,
                CanonicalBuildSpawnRequest {
                    owner: assignment.owner,
                    type_index: CITY_CENTER_TYPE,
                    snapped_x: assignment.snapped_position.0,
                    snapped_y: assignment.snapped_position.1,
                    build: staged_build,
                },
            )
            .map_err(SetupCitiesError::BuildSpawn)?;

            let city_slot = cities.alloc_slot(owner);
            let city_slot_i16 = i16::try_from(city_slot)
                .map_err(|_| SetupCitiesError::CitySlotOverflow { owner, city_slot })?;
            let constructor = apply_fresh_starting_village_projection(
                &mut sim.builds[build.row],
                &mut sim.map.world,
                FreshStartingVillageRequest {
                    owner: assignment.owner,
                    city_slot: city_slot_i16,
                    current_type: CITY_CENTER_TYPE,
                    // Names are save-only: City::walk_data skips both strings for a
                    // CheckSum visitor. Content resolution remains outside this owner.
                    city_name: String::new(),
                    city_id: String::new(),
                    indian_radius_bonus: assignment.tribe == 21,
                },
            )
            .map_err(SetupCitiesError::CityConstructor)?;
            if constructor.region != assignment.region {
                return Err(SetupCitiesError::CenterRegionMismatch {
                    owner: assignment.owner,
                    expected: assignment.region,
                    actual: constructor.region,
                });
            }
            build_walk.install(
                build.row,
                BuildWalkFacts {
                    launching: None,
                    mining: Default::default(),
                },
            );

            cities.slots[owner][city_slot] = constructor.city.clone();
            receipts.push(StartingCityReceipt {
                owner: assignment.owner,
                replay_player_slot: assignment.replay_player_slot,
                replay_play: assignment.replay_play,
                camera_turn: assignment.camera_turn,
                camera_stamp: assignment.camera_stamp,
                start_wcoord: assignment.start_wcoord,
                snapped_position: assignment.snapped_position,
                region: assignment.region,
                city_slot: city_slot_i16,
                build,
                constructor,
            });
        }

        let cities_channel = check_sim_cities(&sim, &cities).map_err(SetupCitiesError::Cities)?;
        let expected_bytes = u64::try_from(receipts.len())
            .ok()
            .and_then(|count| {
                count.checked_mul(crate::cities_runtime::EMPTY_CARAVAN_CITY_WALK_BYTES)
            })
            .ok_or(SetupCitiesError::CityWalkCountOverflow)?;
        if cities_channel.cities_walked as usize != receipts.len()
            || cities_channel.bytes_walked != expected_bytes
        {
            return Err(SetupCitiesError::IncompleteCitiesChannel {
                expected_cities: receipts.len(),
                actual_cities: cities_channel.cities_walked,
                expected_bytes,
                actual_bytes: cities_channel.bytes_walked,
            });
        }

        Ok(Self {
            sim,
            cities,
            build_walk,
            receipt: StartingSetupReceipt {
                position_evidence: StartingPositionEvidence::FirstCameraOnObject2000,
                region_evidence,
                active_players: receipts.len(),
                city_flags: 0x4011,
                cities: receipts,
                constructor_cities: cities_channel,
                first_checksum_city_image_ready: false,
                builds_channel_ready: false,
            },
            channels: StartingSetupChannels {
                cities: None,
                builds: None,
            },
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Assignment {
    owner: u8,
    tribe: u8,
    replay_player_slot: u8,
    replay_play: i32,
    camera_turn: i32,
    camera_stamp: u32,
    start_wcoord: (i32, i32),
    snapped_position: (i32, i32),
    region: i16,
}

fn validate_top_level(initial: &InitialState, map: &InitialWorld) -> Result<(), SetupCitiesError> {
    if initial.info.settings.scenario_type != 0 {
        return Err(SetupCitiesError::UnsupportedScenario {
            scenario_type: initial.info.settings.scenario_type,
        });
    }
    // Starting Town 2/3 creates additional Builds after the center and can update the City
    // through Build::set_city. Refuse until that complete transaction is owned.
    if initial.info.settings.starting_town != 1 {
        return Err(SetupCitiesError::UnsupportedStartingTown {
            starting_town: initial.info.settings.starting_town,
        });
    }
    if map.world.xs <= 0 || map.world.ys <= 0 || map.world.xs != map.world.ys {
        return Err(SetupCitiesError::MapDimensionOutOfRange { xs: map.world.xs });
    }
    Ok(())
}

fn start_region(map_style: u8) -> Result<(i16, StartingRegionEvidence), SetupCitiesError> {
    match map_style {
        6 | 9 => Ok((1, StartingRegionEvidence::AllLandSingleComponent)),
        _ => Err(SetupCitiesError::UnresolvedStartRegion { map_style }),
    }
}

fn derive_assignments(
    replay: &Replay,
    map: &InitialWorld,
    region: i16,
) -> Result<Vec<Assignment>, SetupCitiesError> {
    let initial = &replay.initial;
    let active: Vec<_> = initial.active_players().collect();
    if active.is_empty() || active.len() > tech_cities::NUM_PLAYERS {
        return Err(SetupCitiesError::ActivePlayerCount {
            active: active.len(),
        });
    }

    let mut by_owner = [None; tech_cities::NUM_PLAYERS];
    let mut by_play: Vec<(i32, u8, u8, u8)> = Vec::with_capacity(active.len());
    for player in active {
        if player.flags & PLAYER_FLAG_PRESENT == 0
            || player.flags & PLAYER_FLAG_HUMAN == 0
            || player.flags & PLAYER_FLAG_OBSERVER != 0
        {
            return Err(SetupCitiesError::UnsupportedPlayerFlags {
                slot: player.slot,
                flags: player.flags,
            });
        }
        if player.team != UNTEAMED {
            return Err(SetupCitiesError::UnsupportedTeam {
                slot: player.slot,
                team: player.team,
            });
        }
        let owner = usize::from(player.who);
        if owner >= tech_cities::NUM_PLAYERS {
            return Err(SetupCitiesError::OwnerOutOfRange {
                slot: player.slot,
                owner: player.who,
            });
        }
        if let Some(first_slot) = by_owner[owner].replace(player.slot) {
            return Err(SetupCitiesError::DuplicateOwner {
                owner: player.who,
                first_slot,
                second_slot: player.slot,
            });
        }
        let play = i32::from(player.play);
        if let Some((_, first_slot, _, _)) = by_play.iter().find(|(seen, _, _, _)| *seen == play) {
            return Err(SetupCitiesError::DuplicatePlay {
                play,
                first_slot: *first_slot,
                second_slot: player.slot,
            });
        }
        by_play.push((play, player.slot, player.who, player.tribe));
    }

    let first_turn = replay
        .turns
        .first()
        .ok_or(SetupCitiesError::MissingFirstCommandGroup)?;
    if replay
        .turns
        .windows(2)
        .any(|turns| turns[0].turn >= turns[1].turn)
    {
        return Err(SetupCitiesError::NonMonotoneReplayTurns);
    }

    let mut seen_plays = Vec::with_capacity(by_play.len());
    let mut out = Vec::with_capacity(by_play.len());
    for player_turn in &first_turn.players {
        let Some(&(_, replay_player_slot, owner, tribe)) = by_play
            .iter()
            .find(|(play, _, _, _)| *play == player_turn.play)
        else {
            return Err(SetupCitiesError::UnknownFirstTurnPlay {
                play: player_turn.play,
            });
        };
        if seen_plays.contains(&player_turn.play) {
            return Err(SetupCitiesError::DuplicateFirstTurnPlay {
                play: player_turn.play,
            });
        }
        seen_plays.push(player_turn.play);
        let mut cameras = player_turn
            .commands
            .iter()
            .filter(|command| command.opcode == CAMERA_COMMAND_OPCODE);
        let camera = cameras.next().ok_or(SetupCitiesError::MissingFirstCamera {
            play: player_turn.play,
            turn: first_turn.turn,
        })?;
        if cameras.next().is_some() {
            return Err(SetupCitiesError::DuplicateFirstCamera {
                play: player_turn.play,
                turn: first_turn.turn,
            });
        }
        let view = CommandView::new(camera.opcode, &camera.bytes);
        let x = view
            .get("x_loc")
            .and_then(|value| i32::try_from(value).ok())
            .ok_or(SetupCitiesError::MalformedFirstCamera {
                play: player_turn.play,
                field: "x_loc",
            })?;
        let y = view
            .get("y_loc")
            .and_then(|value| i32::try_from(value).ok())
            .ok_or(SetupCitiesError::MalformedFirstCamera {
                play: player_turn.play,
                field: "y_loc",
            })?;
        if x.rem_euclid(WORLD_TO_COORD) != VILLAGE_CENTER_OFFSET
            || y.rem_euclid(WORLD_TO_COORD) != VILLAGE_CENTER_OFFSET
        {
            return Err(SetupCitiesError::CameraNotVillageCenter {
                play: player_turn.play,
                x,
                y,
            });
        }
        let start_x = (x - VILLAGE_CENTER_OFFSET) / WORLD_TO_COORD;
        let start_y = (y - VILLAGE_CENTER_OFFSET) / WORLD_TO_COORD;
        if start_x < 0 || start_y < 0 || start_x >= map.world.xs || start_y >= map.world.ys {
            return Err(SetupCitiesError::StartOutsideMap {
                play: player_turn.play,
                x: start_x,
                y: start_y,
                xs: map.world.xs,
                ys: map.world.ys,
            });
        }
        out.push(Assignment {
            owner,
            tribe,
            replay_player_slot,
            replay_play: player_turn.play,
            camera_turn: first_turn.turn,
            camera_stamp: player_turn.stamp,
            start_wcoord: (start_x, start_y),
            snapped_position: (x, y),
            region,
        });
    }
    for &(play, _, _, _) in &by_play {
        if !seen_plays.contains(&play) {
            return Err(SetupCitiesError::MissingFirstTurnPlay {
                play,
                turn: first_turn.turn,
            });
        }
    }
    out.sort_by_key(|assignment| assignment.owner);
    Ok(out)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SetupCitiesError {
    UnsupportedScenario {
        scenario_type: u8,
    },
    UnsupportedStartingTown {
        starting_town: u8,
    },
    UnresolvedStartRegion {
        map_style: u8,
    },
    MapDimensionOutOfRange {
        xs: i32,
    },
    ActivePlayerCount {
        active: usize,
    },
    UnsupportedPlayerFlags {
        slot: u8,
        flags: u16,
    },
    UnsupportedTeam {
        slot: u8,
        team: u8,
    },
    OwnerOutOfRange {
        slot: u8,
        owner: u8,
    },
    DuplicateOwner {
        owner: u8,
        first_slot: u8,
        second_slot: u8,
    },
    DuplicatePlay {
        play: i32,
        first_slot: u8,
        second_slot: u8,
    },
    MissingFirstCommandGroup,
    NonMonotoneReplayTurns,
    UnknownFirstTurnPlay {
        play: i32,
    },
    DuplicateFirstTurnPlay {
        play: i32,
    },
    MissingFirstTurnPlay {
        play: i32,
        turn: i32,
    },
    MissingFirstCamera {
        play: i32,
        turn: i32,
    },
    DuplicateFirstCamera {
        play: i32,
        turn: i32,
    },
    MalformedFirstCamera {
        play: i32,
        field: &'static str,
    },
    CameraNotVillageCenter {
        play: i32,
        x: i32,
        y: i32,
    },
    StartOutsideMap {
        play: i32,
        x: i32,
        y: i32,
        xs: i32,
        ys: i32,
    },
    BuildSpawn(CanonicalBuildSpawnError),
    CityConstructor(StartingCityConstructorError),
    CenterRegionMismatch {
        owner: u8,
        expected: i16,
        actual: i16,
    },
    CitySlotOverflow {
        owner: usize,
        city_slot: usize,
    },
    Cities(CitiesRuntimeError),
    CityWalkCountOverflow,
    IncompleteCitiesChannel {
        expected_cities: usize,
        actual_cities: u32,
        expected_bytes: u64,
        actual_bytes: u64,
    },
}

impl fmt::Display for SetupCitiesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "starting Cities setup refused: {self:?}")
    }
}

impl std::error::Error for SetupCitiesError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_source_proven_single_landmass_styles_admit_region_one() {
        for map_style in [6, 9] {
            assert_eq!(
                start_region(map_style),
                Ok((1, StartingRegionEvidence::AllLandSingleComponent))
            );
        }
        assert_eq!(
            start_region(14),
            Err(SetupCitiesError::UnresolvedStartRegion { map_style: 14 })
        );
    }

    #[test]
    fn installable_channels_stay_an_atomic_pair() {
        let channels = StartingSetupChannels {
            cities: None,
            builds: None,
        };
        assert_eq!(channels.cities, None);
        assert_eq!(channels.builds, None);
    }
}
