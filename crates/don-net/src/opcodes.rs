// GENERATED from rise.pdb by re/scripts/pdb_types.py -- do not hand-edit.
// Source of truth: `sizeof(<X>Command)` in the shipped PDB for riseofnations.exe
// (GUID 51D4F219-61C6-4F84-9D5B-C3361B0D291F, age 1), which is byte-identical to
// the CodeView record in the retail binary. Opcode -> handler mapping is the
// `CommandPackage::process` switch at VA 0x0094a700.
//
// `None` size = variable-length; see `Command::wire_len`.

/// Engine name of each opcode, indexed by opcode value (`CommandTypes` enum).
pub const COMMAND_NAMES: [&str; 82] = [
    "process_group",
    "process_begin",
    "process_stance",
    "process_form",
    "process_attack",
    "process_siege_attack",
    "process_swarm_around",
    "process_move_to",
    "process_move_near",
    "process_attack_ground",
    "process_patrol",
    "process_launch_patrol",
    "process_halt",
    "process_transport",
    "process_set_transport",
    "process_board_ship",
    "process_repair",
    "process_trade",
    "process_city_gather",
    "process_gather",
    "process_garrison",
    "process_disband",
    "process_gather_point",
    "process_spell",
    "process_queue_up",
    "process_build",
    "process_eject_all",
    "process_alarm",
    "process_flight",
    "process_stop_spell",
    "process_follow",
    "process_guard",
    "process_unitmask",
    "process_buildmask",
    "process_hotkey",
    "process_recall",
    "process_scramble",
    "process_treaty",
    "process_declare",
    "process_clear_tributes",
    "process_clear_all",
    "process_accept",
    "process_reject",
    "process_tribute",
    "process_demand_tribute",
    "process_propose_attack",
    "process_buy",
    "process_sell",
    "process_unqueue",
    "process_come_out",
    "process_ping",
    "process_spline",
    "process_speed_set",
    "process_speed_up",
    "process_speed_down",
    "process_mp_log",
    "process_check_random",
    "process_check_sums",
    "process_next_check_sum",
    "process_cheat_view_all",
    "process_cheat_give_techs",
    "process_cheat_zero_techs",
    "process_cheat_ai_speed_increase",
    "process_cheat_ai_speed_normal",
    "process_cheat_ai_toggle",
    "process_cheat_increase_buckets",
    "process_cheat_zero_buckets",
    "process_cheat_init_unit",
    "process_chat",
    "process_chat_set",
    "process_resign",
    "process_quit",
    "process_camera",
    "process_leader_options",
    "process_turn_data",
    "process_rename_city",
    "process_pause",
    "process_cannon_time",
    "process_console_cmd",
    "process_player_speed",
    "process_ungraceful_player_drop",
    "process_marwan",
];

/// Engine struct name backing each opcode.
pub const COMMAND_STRUCTS: [&str; 82] = [
    "GroupCommand",
    "BeginCommand",
    "StanceCommand",
    "FormCommand",
    "AttackCommand",
    "SiegeAttackCommand",
    "SwarmAroundCommand",
    "MoveToCommand",
    "MoveNearCommand",
    "AttackGroundCommand",
    "PatrolCommand",
    "LaunchPatrolCommand",
    "HaltCommand",
    "TransportCommand",
    "SetTransportCommand",
    "BoardShipCommand",
    "RepairCommand",
    "TradeCommand",
    "CityGatherCommand",
    "GatherCommand",
    "GarrisonCommand",
    "DisbandCommand",
    "GatherPointCommand",
    "SpellCommand",
    "QueueUpCommand",
    "BuildCommand",
    "EjectAllCommand",
    "AlarmCommand",
    "FlightCommand",
    "StopSpellCommand",
    "FollowCommand",
    "GuardCommand",
    "UnitmaskCommand",
    "BuildmaskCommand",
    "HotKeyCommand",
    "RecallCommand",
    "ScrambleCommand",
    "TreatyCommand",
    "DeclareCommand",
    "ClearTributesCommand",
    "ClearAllCommand",
    "AcceptCommand",
    "RejectCommand",
    "TributeCommand",
    "DemandTributeCommand",
    "ProposeAttackCommand",
    "BuyCommand",
    "SellCommand",
    "UnqueueCommand",
    "ComeOutCommand",
    "PingCommand",
    "SplineCommand",
    "SpeedSetCommand",
    "SpeedUpCommand",
    "SpeedDownCommand",
    "MPLogCommand",
    "CheckRandomCommand",
    "CheckSumsCommand",
    "NextCheckSumCommand",
    "CheatViewAllCommand",
    "CheatGiveTechsCommand",
    "CheatZeroTechsCommand",
    "CheatAISpeedIncreaseCommand",
    "CheatAISpeedNormalCommand",
    "CheatAIToggleCommand",
    "CheatIncreaseBucketsCommand",
    "CheatZeroBucketsCommand",
    "CheatInitUnitCommand",
    "ChatCommand",
    "ChatSetCommand",
    "ResignCommand",
    "QuitCommand",
    "CameraCommand",
    "LeaderOptionsCommand",
    "TurnDataCommand",
    "RenameCityCommand",
    "PauseCommand",
    "CannonTimeCommand",
    "ConsoleCmdCommand",
    "PlayerSpeedCommand",
    "UngracefulPlayerDrop",
    "MarwanCommand",
];

/// Fixed wire size in bytes, or `None` when the command is variable-length.
pub const COMMAND_SIZES: [Option<u16>; 82] = [
    None,                 // 00 GroupCommand (variable, base 5)
    Some(1),               // 01 BeginCommand
    Some(5),               // 02 StanceCommand
    Some(13),              // 03 FormCommand
    Some(17),              // 04 AttackCommand
    Some(13),              // 05 SiegeAttackCommand
    Some(17),              // 06 SwarmAroundCommand
    Some(22),              // 07 MoveToCommand
    Some(26),              // 08 MoveNearCommand
    Some(10),              // 09 AttackGroundCommand
    Some(10),              // 0a PatrolCommand
    Some(25),              // 0b LaunchPatrolCommand
    Some(1),               // 0c HaltCommand
    Some(1),               // 0d TransportCommand
    Some(5),               // 0e SetTransportCommand
    Some(9),               // 0f BoardShipCommand
    Some(13),              // 10 RepairCommand
    Some(21),              // 11 TradeCommand
    Some(9),               // 12 CityGatherCommand
    Some(9),               // 13 GatherCommand
    Some(13),              // 14 GarrisonCommand
    Some(5),               // 15 DisbandCommand
    Some(17),              // 16 GatherPointCommand
    Some(21),              // 17 SpellCommand
    Some(9),               // 18 QueueUpCommand
    Some(25),              // 19 BuildCommand
    Some(17),              // 1a EjectAllCommand
    Some(1),               // 1b AlarmCommand
    Some(25),              // 1c FlightCommand
    Some(1),               // 1d StopSpellCommand
    Some(13),              // 1e FollowCommand
    Some(13),              // 1f GuardCommand
    Some(9),               // 20 UnitmaskCommand
    Some(9),               // 21 BuildmaskCommand
    Some(25),              // 22 HotKeyCommand
    Some(1),               // 23 RecallCommand
    Some(1),               // 24 ScrambleCommand
    Some(13),              // 25 TreatyCommand
    Some(13),              // 26 DeclareCommand
    Some(9),               // 27 ClearTributesCommand
    Some(9),               // 28 ClearAllCommand
    Some(9),               // 29 AcceptCommand
    Some(9),               // 2a RejectCommand
    Some(17),              // 2b TributeCommand
    Some(17),              // 2c DemandTributeCommand
    Some(17),              // 2d ProposeAttackCommand
    Some(13),              // 2e BuyCommand
    Some(13),              // 2f SellCommand
    Some(15),              // 30 UnqueueCommand
    Some(11),              // 31 ComeOutCommand
    Some(9),               // 32 PingCommand
    None,                 // 33 SplineCommand (variable, base 14)
    Some(5),               // 34 SpeedSetCommand
    Some(1),               // 35 SpeedUpCommand
    Some(1),               // 36 SpeedDownCommand
    Some(1),               // 37 MPLogCommand
    Some(5),               // 38 CheckRandomCommand
    Some(65),              // 39 CheckSumsCommand
    Some(6),               // 3a NextCheckSumCommand
    Some(5),               // 3b CheatViewAllCommand
    Some(5),               // 3c CheatGiveTechsCommand
    Some(5),               // 3d CheatZeroTechsCommand
    Some(1),               // 3e CheatAISpeedIncreaseCommand
    Some(1),               // 3f CheatAISpeedNormalCommand
    Some(1),               // 40 CheatAIToggleCommand
    Some(5),               // 41 CheatIncreaseBucketsCommand
    Some(5),               // 42 CheatZeroBucketsCommand
    Some(17),              // 43 CheatInitUnitCommand
    None,                 // 44 ChatCommand (variable, base 19)
    Some(9),               // 45 ChatSetCommand
    Some(5),               // 46 ResignCommand
    Some(7),               // 47 QuitCommand
    Some(10),              // 48 CameraCommand
    Some(33),              // 49 LeaderOptionsCommand
    Some(11),              // 4a TurnDataCommand
    Some(53),              // 4b RenameCityCommand
    Some(2),               // 4c PauseCommand
    Some(2),               // 4d CannonTimeCommand
    Some(521),             // 4e ConsoleCmdCommand
    Some(9),               // 4f PlayerSpeedCommand
    Some(3),               // 50 UngracefulPlayerDrop
    Some(2),               // 51 MarwanCommand
];
