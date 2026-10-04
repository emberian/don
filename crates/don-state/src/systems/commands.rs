//! `CommandPackage::process_all` 0x0094c500 / `CommandPackage::process`
//! 0x0094a700 — applying one lockstep command package onto the canonical
//! `Save`. Leaf module: exposes free functions for the owning traversal to
//! call; never edits sibling modules.
//!
//! Retail shape (`re/decomp-all/0094c500.c`): a package is `stamp, play,
//! valid, group, size, data[size]`. `process_all` walks `data` command by
//! command; each `process_<op>` returns the command's byte length, and in a
//! network game (`Game+0x820 & 4`) the payload was XOR-obfuscated with the
//! seed's high half and every command is followed by `Random::get(0, 2)` pad
//! bytes. Solo recordings (every capture we have) take neither branch, so
//! applying a package draws **no** `game_random` steps of its own.
//!
//! `CommandPackage::group` (+0xc) is runtime scratch: `process_group`
//! (opcode 0) resolves the selection into a `Groups` slot and stores its
//! index there; every Sim-class opcode after it in the same package acts on
//! `Groups.list[group]` (`0x00949945: imul ecx,[ebx+0xc],0x9d4; add
//! ecx,[0xe85f20]`), gated on `group >= 0`.
//!
//! Status per opcode is reported in [`CommandRecord::status`]:
//! `Ported` bodies are transcribed from the disassembly, `Partial` bodies
//! perform the unconditional writes and name what is still missing, `Stub`
//! bodies only decode their arguments. The whole-module `STATUS` is
//! `Partial` until MoveTo/Gather/Build/QueueUp land (they need the
//! pathfinder, `Objects::add` and the production queue, which are stubs in
//! this crate).
//!
//! Layout references (`re/scripts/pdb_layout.py`):
//!   Group        +0x04 id, +0x08 army, +0x0c num, +0x10 form, +0x14 stamp,
//!                +0x18 ox, +0x1c oy, +0x20 o_dist, +0x24 o_angle,
//!                +0x28 disband, +0x2c order_num, +0x30 priority, +0x34 role,
//!                +0x38 think_frame, +0x3c new_speed, +0x40 speed,
//!                +0x44 form_num, +0x48 facing, +0x49 buildings, +0x4a who,
//!                +0x4b march, +0x4c off_x[128], +0x24c off_y[128],
//!                +0x44c curr_x[128], +0x64c curr_y[128], +0x84c angles[128],
//!                +0x8cc list[128]. `Group::walk_data` 0x00708400 walks
//!                +0x04..+0x4c then the first `num` entries of each array —
//!                `sections::Group.hdr` is image +0x04..+0x4c.
//!   UnitData     +0x08 flags, +0x09 who, +0x0a o, +0x10/+0x14 x/y (^0x63637),
//!                +0x18 ptype, +0x30 uid, +0x68 unit_masks, +0x70 orders_x,
//!                +0x74 orders_y, +0x58 dest_angle, +0x50 angle, +0x80 group,
//!                +0x82 inside_up, +0x8e o_up, +0x90 o_down, +0x9a myspeed,
//!                +0xb1 stance, +0xb6 play. `sections::Unit.body` is
//!                image +0x48..+0xb7, `ObjBase.mid` is +0x20..+0x42.
//!   Player       stride 0x8c inside Game (+0x44 + play*0x8c): +0x30 flags,
//!                +0x33 who. `PlayerInfo.body` is Player +0x00..+0x39.

#![allow(dead_code)]

use crate::sections::{Group, Obj, Save, Unit};
use crate::tick::StepStatus;

pub const STATUS: StepStatus = StepStatus::Partial;

// ---------------------------------------------------------------------------
// Wire format: `CommandPackage::process` 0x0094a700 switch. Every handler
// returns the command's byte length; three are variable.
// ---------------------------------------------------------------------------

/// `sizeof(<X>Command)` per opcode (`CommandTypes` 0x00..0x51), `None` for
/// the three variable-length commands. Same table `don-net` carries; copied
/// so this crate stays dependency-free.
pub const COMMAND_SIZES: [Option<u16>; 82] = [
    None,      // 00 GroupCommand: 3 + 2*num
    Some(1),   // 01 BeginCommand
    Some(5),   // 02 StanceCommand
    Some(13),  // 03 FormCommand
    Some(17),  // 04 AttackCommand
    Some(13),  // 05 SiegeAttackCommand
    Some(17),  // 06 SwarmAroundCommand
    Some(22),  // 07 MoveToCommand
    Some(26),  // 08 MoveNearCommand
    Some(10),  // 09 AttackGroundCommand
    Some(10),  // 0a PatrolCommand
    Some(25),  // 0b LaunchPatrolCommand
    Some(1),   // 0c HaltCommand
    Some(1),   // 0d TransportCommand
    Some(5),   // 0e SetTransportCommand
    Some(9),   // 0f BoardShipCommand
    Some(13),  // 10 RepairCommand
    Some(21),  // 11 TradeCommand
    Some(9),   // 12 CityGatherCommand
    Some(9),   // 13 GatherCommand
    Some(13),  // 14 GarrisonCommand
    Some(5),   // 15 DisbandCommand
    Some(17),  // 16 GatherPointCommand
    Some(21),  // 17 SpellCommand
    Some(9),   // 18 QueueUpCommand
    Some(25),  // 19 BuildCommand
    Some(17),  // 1a EjectAllCommand
    Some(1),   // 1b AlarmCommand
    Some(25),  // 1c FlightCommand
    Some(1),   // 1d StopSpellCommand
    Some(13),  // 1e FollowCommand
    Some(13),  // 1f GuardCommand
    Some(9),   // 20 UnitmaskCommand
    Some(9),   // 21 BuildmaskCommand
    Some(25),  // 22 HotKeyCommand
    Some(1),   // 23 RecallCommand
    Some(1),   // 24 ScrambleCommand
    Some(13),  // 25 TreatyCommand
    Some(13),  // 26 DeclareCommand
    Some(9),   // 27 ClearTributesCommand
    Some(9),   // 28 ClearAllCommand
    Some(9),   // 29 AcceptCommand
    Some(9),   // 2a RejectCommand
    Some(17),  // 2b TributeCommand
    Some(17),  // 2c DemandTributeCommand
    Some(17),  // 2d ProposeAttackCommand
    Some(13),  // 2e BuyCommand
    Some(13),  // 2f SellCommand
    Some(15),  // 30 UnqueueCommand
    Some(11),  // 31 ComeOutCommand
    Some(9),   // 32 PingCommand
    None,      // 33 SplineCommand: 6 + 8*len
    Some(5),   // 34 SpeedSetCommand
    Some(1),   // 35 SpeedUpCommand
    Some(1),   // 36 SpeedDownCommand
    Some(1),   // 37 MPLogCommand
    Some(5),   // 38 CheckRandomCommand
    Some(65),  // 39 CheckSumsCommand
    Some(6),   // 3a NextCheckSumCommand
    Some(5),   // 3b CheatViewAllCommand
    Some(5),   // 3c CheatGiveTechsCommand
    Some(5),   // 3d CheatZeroTechsCommand
    Some(1),   // 3e CheatAISpeedIncreaseCommand
    Some(1),   // 3f CheatAISpeedNormalCommand
    Some(1),   // 40 CheatAIToggleCommand
    Some(5),   // 41 CheatIncreaseBucketsCommand
    Some(5),   // 42 CheatZeroBucketsCommand
    Some(17),  // 43 CheatInitUnitCommand
    None,      // 44 ChatCommand: 19 + 2*len
    Some(9),   // 45 ChatSetCommand
    Some(5),   // 46 ResignCommand
    Some(7),   // 47 QuitCommand
    Some(10),  // 48 CameraCommand
    Some(33),  // 49 LeaderOptionsCommand
    Some(11),  // 4a TurnDataCommand
    Some(53),  // 4b RenameCityCommand
    Some(2),   // 4c PauseCommand
    Some(2),   // 4d CannonTimeCommand
    Some(521), // 4e ConsoleCmdCommand
    Some(9),   // 4f PlayerSpeedCommand
    Some(3),   // 50 UngracefulPlayerDrop
    Some(2),   // 51 MarwanCommand
];

/// `CommandPackage::process_*` method names by opcode (PDB).
pub const COMMAND_NAMES: [&str; 82] = [
    "group", "begin", "stance", "form", "attack", "siege_attack", "swarm_around", "move_to",
    "move_near", "attack_ground", "patrol", "launch_patrol", "halt", "transport", "set_transport",
    "board_ship", "repair", "trade", "city_gather", "gather", "garrison", "disband", "gather_point",
    "spell", "queue_up", "build", "eject_all", "alarm", "flight", "stop_spell", "follow", "guard",
    "unitmask", "buildmask", "hotkey", "recall", "scramble", "treaty", "declare", "clear_tributes",
    "clear_all", "accept", "reject", "tribute", "demand_tribute", "propose_attack", "buy", "sell",
    "unqueue", "come_out", "ping", "spline", "speed_set", "speed_up", "speed_down", "mp_log",
    "check_random", "check_sums", "next_check_sum", "cheat_view_all", "cheat_give_techs",
    "cheat_zero_techs", "cheat_ai_speed_increase", "cheat_ai_speed_normal", "cheat_ai_toggle",
    "cheat_increase_buckets", "cheat_zero_buckets", "cheat_init_unit", "chat", "chat_set",
    "resign", "quit", "camera", "leader_options", "turn_data", "rename_city", "pause",
    "cannon_time", "console_cmd", "player_speed", "ungraceful_player_drop", "marwan",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyError {
    /// Fewer bytes than the opcode's handler would read.
    Truncated { offset: usize, need: usize, have: usize },
    /// Opcode outside the 0x00..0x51 switch.
    UnknownOpcode { offset: usize, opcode: u8 },
    /// `ChatCommand` length outside the 512-byte send buffer.
    BadLength { offset: usize, len: i32 },
    /// `play` outside 0..8.
    BadPlayer(i32),
    /// A state index the save does not have (lets a caller tell "the
    /// transcription is wrong" from "the save is not what retail had").
    Shape(String),
}

impl std::fmt::Display for ApplyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApplyError::Truncated { offset, need, have } => {
                write!(f, "command at +{offset:#x} truncated: need {need}, have {have}")
            }
            ApplyError::UnknownOpcode { offset, opcode } => {
                write!(f, "command at +{offset:#x}: unknown opcode {opcode:#04x}")
            }
            ApplyError::BadLength { offset, len } => {
                write!(f, "command at +{offset:#x}: chat length {len}")
            }
            ApplyError::BadPlayer(p) => write!(f, "play {p} outside 0..8"),
            ApplyError::Shape(s) => write!(f, "save shape: {s}"),
        }
    }
}

impl std::error::Error for ApplyError {}

/// Byte length of the command at `buf[0]` — the value the retail handler
/// returns (`lea eax,[eax*2+3]` at 0x0094a6f3 for GroupCommand,
/// `lea esi,[eax*8+6]` at 0x00945396 for SplineCommand,
/// `lea esi,[eax*2+0x13]` at 0x009458b4 for ChatCommand).
pub fn wire_len(buf: &[u8]) -> Result<usize, ApplyError> {
    let need = |n: usize| -> Result<(), ApplyError> {
        if buf.len() < n {
            Err(ApplyError::Truncated { offset: 0, need: n, have: buf.len() })
        } else {
            Ok(())
        }
    };
    need(1)?;
    let op = buf[0];
    match op {
        0x00 => {
            need(2)?;
            Ok(3 + 2 * buf[1] as usize)
        }
        0x33 => {
            need(6)?;
            Ok(6 + 8 * u16::from_le_bytes([buf[4], buf[5]]) as usize)
        }
        0x44 => {
            need(17)?;
            let n = i32::from_le_bytes([buf[13], buf[14], buf[15], buf[16]]);
            if !(0..=512).contains(&n) {
                return Err(ApplyError::BadLength { offset: 0, len: n });
            }
            Ok(19 + 2 * n as usize)
        }
        _ => match COMMAND_SIZES.get(op as usize).copied().flatten() {
            Some(n) => Ok(n as usize),
            None => Err(ApplyError::UnknownOpcode { offset: 0, opcode: op }),
        },
    }
}

/// Split a solo package payload into its commands (`process_all` loop with
/// the `Game+0x820 & 4` network branch off: no XOR, no `Random::get(0,2)`
/// inter-command pad).
pub fn split_commands(payload: &[u8]) -> Result<Vec<&[u8]>, ApplyError> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < payload.len() {
        let l = wire_len(&payload[i..]).map_err(|e| match e {
            ApplyError::Truncated { need, have, .. } => ApplyError::Truncated { offset: i, need, have },
            ApplyError::UnknownOpcode { opcode, .. } => ApplyError::UnknownOpcode { offset: i, opcode },
            ApplyError::BadLength { len, .. } => ApplyError::BadLength { offset: i, len },
            other => other,
        })?;
        if i + l > payload.len() {
            return Err(ApplyError::Truncated { offset: i, need: l, have: payload.len() - i });
        }
        out.push(&payload[i..i + l]);
        i += l;
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Runtime context that is not walked into the save.
// ---------------------------------------------------------------------------

/// `process_group`'s per-player "last selection" cache: `DAT_00cbee88[play]`
/// (count), `DAT_00cbeeb0[play*128 + i]` (object index) and
/// `DAT_00cbf6b0[play*128 + i]` (uid at selection time). A `GroupCommand`
/// with `num == 0` re-selects from here, re-validating each uid.
#[derive(Clone)]
pub struct SelectionCache {
    pub num: u8,
    pub o: [i16; 128],
    pub uid: [i16; 128],
}

impl Default for SelectionCache {
    fn default() -> Self {
        SelectionCache { num: 0, o: [0; 128], uid: [0; 128] }
    }
}

/// Non-walked state the command layer carries between packages.
#[derive(Clone, Default)]
pub struct CommandContext {
    pub last_selection: [SelectionCache; 8],
}

// ---------------------------------------------------------------------------
// Reports
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct CommandRecord {
    pub opcode: u8,
    pub name: &'static str,
    pub len: usize,
    pub status: StepStatus,
    pub effects: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct PackageReport {
    /// `CommandPackage::group` after the package: the `Groups.list` index
    /// the package's Sim-class commands acted on, `-1` when none.
    pub group: i32,
    pub commands: Vec<CommandRecord>,
}

/// Apply one package of commands issued by player slot `play` with a fresh
/// (empty) selection cache. See [`apply_package_with`].
pub fn apply_package(save: &mut Save, play: usize, bytes: &[u8]) -> Result<PackageReport, ApplyError> {
    let mut ctx = CommandContext::default();
    apply_package_with(save, &mut ctx, play, bytes)
}

/// `CommandPackage::process_all` 0x0094c500 for a solo package: run every
/// command in `bytes` in order against `save`, carrying `group` (+0xc)
/// across them. `play` is `CommandPackage::play`.
pub fn apply_package_with(
    save: &mut Save,
    ctx: &mut CommandContext,
    play: usize,
    bytes: &[u8],
) -> Result<PackageReport, ApplyError> {
    if play >= 8 {
        return Err(ApplyError::BadPlayer(play as i32));
    }
    let mut pkg = Package { play, group: -1 };
    let mut report = PackageReport { group: -1, commands: Vec::new() };
    for cmd in split_commands(bytes)? {
        let mut rec = CommandRecord {
            opcode: cmd[0],
            name: COMMAND_NAMES.get(cmd[0] as usize).copied().unwrap_or("?"),
            len: cmd.len(),
            status: StepStatus::Stub,
            effects: Vec::new(),
        };
        rec.status = process(save, ctx, &mut pkg, cmd, &mut rec.effects)?;
        report.commands.push(rec);
    }
    report.group = pkg.group;
    Ok(report)
}

/// The two `CommandPackage` fields the handlers read/write.
struct Package {
    play: usize,
    group: i32,
}

/// `CommandPackage::process` 0x0094a700: the opcode switch. The
/// `in_ECX[1] == *(Game+0x2a0)` (`play == local player`) → `UI+0x90 = 1`
/// branches are interface refresh flags, not simulation state.
fn process(
    save: &mut Save,
    ctx: &mut CommandContext,
    pkg: &mut Package,
    cmd: &[u8],
    fx: &mut Vec<String>,
) -> Result<StepStatus, ApplyError> {
    let i32_at = |o: usize| i32::from_le_bytes(cmd[o..o + 4].try_into().unwrap());
    let i8_at = |o: usize| cmd[o] as i8 as i32;
    Ok(match cmd[0] {
        0x00 => {
            process_group(save, ctx, pkg, cmd, fx)?;
            StepStatus::Ported
        }
        0x01 => {
            // process_begin 0x00949fd0 -> Group::action_begin 0x00714100:
            // `*(this+0x28) = 0` (disband).
            if pkg.group >= 0 {
                with_group(save, pkg.group, |g| g.set_i32(G_DISBAND, 0))?;
                fx.push(format!("Groups.list[{}].disband = 0", pkg.group));
            }
            StepStatus::Ported
        }
        0x02 => {
            let stance = i32_at(1);
            fx.push(format!("stance={stance}"));
            if pkg.group >= 0 {
                action_stance(save, pkg.group, stance, fx)?
            } else {
                StepStatus::Ported
            }
        }
        0x07 => {
            fx.push(format!(
                "move_to to=({},{}) set_angle={} angle={} orders={} queued={} form={} width={} disembark={}",
                i32_at(1), i32_at(5), i32_at(9), i32_at(13), i8_at(0x11), i8_at(0x12), i8_at(0x13), i8_at(0x14), i8_at(0x15)
            ));
            if pkg.group >= 0 {
                // Group::action_move_to 0x0070fba0 -> Group::action_move_near
                // 0x00704990 (9,205 B): compute_form, PathFinder::find_wpath
                // for the group path, UnitType::find_nearby_spot per member,
                // then Unit::add_group_move_order / add_move_facing_order.
                // Needs the pathfinder (systems/pathfinder.rs is a Stub) and
                // the Form tables; not transcribed.
                fx.push("Group::action_move_near 0x00704990 not transcribed (pathfinder/forms)".into());
            }
            StepStatus::Stub
        }
        0x0c => {
            if pkg.group >= 0 {
                action_halt(save, pkg.group, 0, fx)?
            } else {
                StepStatus::Ported
            }
        }
        0x13 => {
            fx.push(format!("gather ox={} queued={}", i32_at(1), i32_at(5)));
            if pkg.group >= 0 {
                // Group::action_gather 0x00700b90 (3,052 B): per citizen
                // Unit::add_gather_order after a reachable-site search through
                // the pathfinder; GatherPointList bookkeeping on the Build.
                fx.push("Group::action_gather 0x00700b90 not transcribed (pathfinder)".into());
            }
            StepStatus::Stub
        }
        0x15 => {
            let all = i32_at(1);
            fx.push(format!("disband all={all}"));
            if pkg.group >= 0 {
                action_disband(save, pkg.group, all, fx)?
            } else {
                StepStatus::Ported
            }
        }
        0x18 => {
            fx.push(format!("queue_up type={} num={}", i32_at(1), i32_at(5)));
            if pkg.group >= 0 {
                // Group::action_queue_up 0x006fdbb0 (1,516 B): per Build
                // member, LeaderData cost check + BuildQueue append (18 B
                // rows) + resource deduction. Production side is
                // systems/production_train.rs (Stub); not transcribed.
                fx.push("Group::action_queue_up 0x006fdbb0 not transcribed (production queue)".into());
            }
            StepStatus::Stub
        }
        0x19 => {
            fx.push(format!(
                "build at=({},{}) to=({},{}) type={} queued={}",
                i32_at(1), i32_at(5), i32_at(9), i32_at(13), i32_at(0x11), i32_at(0x15)
            ));
            if pkg.group >= 0 {
                // Group::action_build 0x00707510 (1,256 B): Objects::add of a
                // new Build (slot allocation, uid counter, World footprint),
                // then Unit::add_build_order per citizen via the pathfinder.
                fx.push("Group::action_build 0x00707510 not transcribed (Objects::add + pathfinder)".into());
            }
            StepStatus::Stub
        }
        0x39 | 0x3a | 0x38 | 0x37 => {
            // check_sums / next_check_sum / check_random / mp_log: desync
            // instrumentation; compares, never writes sim state.
            StepStatus::Ported
        }
        0x48 | 0x4a | 0x4f | 0x4c | 0x4d => {
            // camera / turn_data / player_speed / pause / cannon_time:
            // TurnControl + Camera section state; not Sim-class.
            StepStatus::Stub
        }
        _ => StepStatus::Stub,
    })
}

// ---------------------------------------------------------------------------
// Group in-memory model (full 128-slot arrays, as retail holds it)
// ---------------------------------------------------------------------------

// `Group.hdr` offsets (image offset - 4).
const G_ID: usize = 0x00;
const G_ARMY: usize = 0x04;
const G_NUM: usize = 0x08;
const G_FORM: usize = 0x0c;
const G_STAMP: usize = 0x10;
const G_OX: usize = 0x14;
const G_OY: usize = 0x18;
const G_O_DIST: usize = 0x1c;
const G_O_ANGLE: usize = 0x20;
const G_DISBAND: usize = 0x24;
const G_ORDER_NUM: usize = 0x28;
const G_PRIORITY: usize = 0x2c;
const G_ROLE: usize = 0x30;
const G_THINK_FRAME: usize = 0x34;
const G_NEW_SPEED: usize = 0x38;
const G_SPEED: usize = 0x3c;
const G_FORM_NUM: usize = 0x40;
const G_FACING: usize = 0x44;
const G_BUILDINGS: usize = 0x45;
const G_WHO: usize = 0x46;
const G_MARCH: usize = 0x47;

/// Retail `Group` as it sits in memory: 72-byte scalar block plus the six
/// 128-entry arrays. `sections::Group` only persists the first `num` entries
/// of each array, so edits happen here and are stored back truncated.
#[derive(Clone)]
pub struct GroupMem {
    pub hdr: [u8; 72],
    pub list: [i16; 128],
    pub off_x: [i32; 128],
    pub off_y: [i32; 128],
    pub curr_x: [i32; 128],
    pub curr_y: [i32; 128],
    pub angles: [i8; 128],
}

impl Default for GroupMem {
    fn default() -> Self {
        GroupMem {
            hdr: [0; 72],
            list: [0; 128],
            off_x: [0; 128],
            off_y: [0; 128],
            curr_x: [0; 128],
            curr_y: [0; 128],
            angles: [0; 128],
        }
    }
}

impl GroupMem {
    /// `Group::Group` 0x007140c0 + `Group::clear(-1)`: a stack temporary
    /// with `id == -1` (the `local_a48 = 0xffffffff` in `process_group`).
    fn temp(frame: i32) -> Self {
        let mut g = GroupMem::default();
        g.set_i32(G_ID, -1);
        g.clear(-1, frame);
        g
    }

    fn from_saved(s: &Group) -> Result<Self, ApplyError> {
        if s.hdr.len() != 72 {
            return Err(ApplyError::Shape(format!("Group.hdr is {} bytes", s.hdr.len())));
        }
        let mut g = GroupMem::default();
        g.hdr.copy_from_slice(&s.hdr);
        let n = g.num().clamp(0, 128) as usize;
        for i in 0..n {
            g.list[i] = i16::from_le_bytes([s.list[i * 2], s.list[i * 2 + 1]]);
            g.off_x[i] = i32::from_le_bytes(s.off_x[i * 4..i * 4 + 4].try_into().unwrap());
            g.off_y[i] = i32::from_le_bytes(s.off_y[i * 4..i * 4 + 4].try_into().unwrap());
            g.curr_x[i] = i32::from_le_bytes(s.curr_x[i * 4..i * 4 + 4].try_into().unwrap());
            g.curr_y[i] = i32::from_le_bytes(s.curr_y[i * 4..i * 4 + 4].try_into().unwrap());
            g.angles[i] = s.angles[i] as i8;
        }
        Ok(g)
    }

    fn store(&self, s: &mut Group) {
        s.hdr.clear();
        s.hdr.extend_from_slice(&self.hdr);
        let n = self.num().clamp(0, 128) as usize;
        s.list = self.list[..n].iter().flat_map(|v| v.to_le_bytes()).collect();
        s.off_x = self.off_x[..n].iter().flat_map(|v| v.to_le_bytes()).collect();
        s.off_y = self.off_y[..n].iter().flat_map(|v| v.to_le_bytes()).collect();
        s.curr_x = self.curr_x[..n].iter().flat_map(|v| v.to_le_bytes()).collect();
        s.curr_y = self.curr_y[..n].iter().flat_map(|v| v.to_le_bytes()).collect();
        s.angles = self.angles[..n].iter().map(|&v| v as u8).collect();
    }

    fn i32(&self, off: usize) -> i32 {
        i32::from_le_bytes(self.hdr[off..off + 4].try_into().unwrap())
    }
    fn set_i32(&mut self, off: usize, v: i32) {
        self.hdr[off..off + 4].copy_from_slice(&v.to_le_bytes());
    }
    fn num(&self) -> i32 {
        self.i32(G_NUM)
    }
    fn set_num(&mut self, n: i32) {
        self.set_i32(G_NUM, n);
    }
    fn who(&self) -> usize {
        self.hdr[G_WHO] as usize
    }
    fn id(&self) -> i32 {
        self.i32(G_ID)
    }
    fn buildings(&self) -> u8 {
        self.hdr[G_BUILDINGS]
    }

    /// `Group::clear` 0x00713e80.
    fn clear(&mut self, id: i32, frame: i32) {
        if id >= 0 {
            self.set_i32(G_ID, id);
        }
        self.hdr[G_WHO] = 0;
        self.set_i32(G_ARMY, -1);
        self.set_num(0);
        self.set_i32(G_FORM, -1);
        self.set_i32(G_STAMP, frame);
        self.set_i32(G_OX, 0);
        self.set_i32(G_OY, 0);
        self.set_i32(G_O_DIST, 0);
        self.set_i32(G_O_ANGLE, 0);
        self.hdr[G_FACING] = 0;
        self.hdr[G_BUILDINGS] = 0;
        self.set_i32(G_DISBAND, 0);
        self.set_i32(G_ORDER_NUM, 0);
        self.set_i32(G_PRIORITY, 0);
        self.set_i32(G_ROLE, 0);
        self.set_i32(G_NEW_SPEED, 0);
        self.set_i32(G_SPEED, 0);
        self.set_i32(G_FORM_NUM, 0);
        self.set_i32(G_THINK_FRAME, 0);
        self.hdr[G_MARCH] = 0;
    }

    /// Remove slot `i`, shifting the six arrays down (the shared loop body
    /// of `get_num` / `get_num_cap` / `normalize` / `kill`).
    fn remove_slot(&mut self, i: usize) {
        let n = self.num() as usize;
        for k in i..n.saturating_sub(1) {
            self.list[k] = self.list[k + 1];
            self.angles[k] = self.angles[k + 1];
            self.off_x[k] = self.off_x[k + 1];
            self.off_y[k] = self.off_y[k + 1];
            self.curr_x[k] = self.curr_x[k + 1];
            self.curr_y[k] = self.curr_y[k + 1];
        }
        self.set_num(self.num() - 1);
    }

    /// `Group::find` 0x0070f8f0: 1-based position of `o` in `list`, 0 when
    /// absent, owner mismatch, or (`check != 0`) the object is not active.
    fn find(&self, save: &Save, o: i32, who: usize, check: bool) -> i32 {
        if who != self.who() {
            return 0;
        }
        if check && obj_flags(save, who, o) & 1 == 0 {
            return 0;
        }
        let n = self.num().max(0) as usize;
        for i in 0..n {
            if self.list[i] as i32 == o {
                return i as i32 + 1;
            }
        }
        0
    }

    /// `Group::get_num` 0x00714700 (vtable +4): compacts inactive members
    /// out of the list; small groups with a real id go through `normalize`.
    fn get_num(&mut self, save: &Save, frame: i32) -> i32 {
        let n = self.num();
        if n < 1 {
            self.set_num(0);
            self.set_i32(G_OY, 0);
            self.set_i32(G_OX, 0);
            return 0;
        }
        if n < 4 && self.id() >= 0 {
            self.normalize(save, frame);
            return self.num();
        }
        let who = self.who();
        let mut i = n - 1;
        while i >= 0 {
            if obj_flags(save, who, self.list[i as usize] as i32) & 1 == 0 {
                self.remove_slot(i as usize);
            }
            i -= 1;
        }
        self.num()
    }

    /// `Group::get_num_cap` 0x007145c0 (vtable +8): like `get_num`, returns
    /// the number of captains among the active members.
    fn get_num_cap(&mut self, save: &Save, frame: i32) -> i32 {
        if self.num() <= 0 {
            self.set_num(0);
            return 0;
        }
        if self.num() < 4 && self.id() >= 0 {
            self.normalize(save, frame);
        }
        let who = self.who();
        let mut caps = 0;
        let mut i = self.num() - 1;
        while i >= 0 {
            let o = self.list[i as usize] as i32;
            if obj_flags(save, who, o) & 1 == 0 {
                self.remove_slot(i as usize);
            } else if is_captain(save, who, o) {
                caps += 1;
            }
            i -= 1;
        }
        caps
    }

    /// `Group::normalize` 0x00711540: drop members that are dead, negative,
    /// (for a priority-0 group with an id) units that no longer point back
    /// at this group, or non-wall objects; then `find_role` + the
    /// `compute_speed` tail.
    fn normalize(&mut self, save: &Save, frame: i32) {
        let who = self.who();
        let mut i = self.num() - 1;
        while i >= 0 {
            let o = self.list[i as usize] as i32;
            let drop = if o < 0 {
                true
            } else {
                let dead = obj_flags(save, who, o) & 1 == 0;
                let strayed = self.i32(G_PRIORITY) == 0
                    && self.id() >= 0
                    && (!obj_is_unit(save, who, o)
                        || unit_i16(save, who, o, 0x80).map(|g| g as i32) != Some(self.id()));
                dead || (strayed && !obj_is_wall(save, who, o))
            };
            if drop {
                self.remove_slot(i as usize);
            }
            i -= 1;
        }
        self.find_role(save);
        self.compute_speed(save);
        let _ = frame;
    }

    /// `Group::find_role` 0x007081f0: `role = OR of UnitType+0x2c8` over
    /// active unit members (buildings groups get 0).
    fn find_role(&mut self, save: &Save) {
        self.set_i32(G_ROLE, 0);
        if self.buildings() != 0 {
            return;
        }
        let who = self.who();
        let mut role = 0i32;
        for i in 0..self.num().max(0) as usize {
            let o = self.list[i] as i32;
            if obj_is_valid_unit(save, who, o) {
                role |= type_i32(save, unit_ptype(save, who, o), 0x2c8).unwrap_or(0);
            }
        }
        self.set_i32(G_ROLE, role);
    }

    /// `GroupData::find_leader` 0x0070ccb0: the active captain with the
    /// lowest `FormData::type_cat`, preferring on-map members (pass 0) and
    /// falling back to any (pass 1). Returns the object index or -1.
    fn find_leader(&self, save: &Save) -> i32 {
        let who = self.who();
        for pass in 0..2 {
            let mut best_cat = 0x12;
            let mut best = -1;
            for i in 0..self.num().max(0) as usize {
                let o = self.list[i] as i32;
                if !obj_is_valid_unit(save, who, o) || !is_captain(save, who, o) {
                    continue;
                }
                if pass == 0 && !unit_is_on_map(save, who, o) {
                    continue;
                }
                let cat = type_cat(save, unit_ptype(save, who, o), who);
                if best < 0 || cat < best_cat {
                    best_cat = cat;
                    best = o;
                }
            }
            if best >= 0 {
                return best;
            }
        }
        -1
    }

    /// `Group::compute_speed` 0x00707f80: `speed = new_speed =
    /// UnitData::speed(leader)` for a non-building group with a leader,
    /// else 0.
    fn compute_speed(&mut self, save: &Save) {
        let mut v = 0;
        if self.buildings() == 0 && self.num() > 0 {
            let l = self.find_leader(save);
            if l >= 0 {
                v = unit_speed(save, self.who(), l);
            }
        }
        self.set_i32(G_SPEED, v);
        self.set_i32(G_NEW_SPEED, v);
    }

    /// `Group::add` 0x00714350 (vtable +0xc).
    fn add(&mut self, save: &mut Save, o: i32, who: usize, sub: bool, force: bool, frame: i32) {
        let n = if force { self.get_num_cap(save, frame) } else { self.get_num(save, frame) };
        if n == 0 {
            self.hdr[G_WHO] = who as u8;
        }
        if !(self.num() == 0 || who == self.who()) {
            return;
        }
        if obj_is_unit(save, who, o) {
            if !sub {
                if !is_captain(save, who, o) {
                    // A carried/attached unit selects its captain instead.
                    let up = unit_i16(save, who, o, 0x8e).unwrap_or(-1) as i32;
                    self.add(save, up, who, false, force, frame);
                    return;
                }
            } else if is_captain(save, who, o) {
                self.kill(save, o, who, false, false, frame);
            }
        }
        self.set_i32(G_DISBAND, 0);
        let is_b = obj_is_build(save, who, o) as u8;
        let n = self.num();
        if !(n == 0 || self.buildings() == is_b) {
            return;
        }
        if self.find(save, o, who, true) != 0 || n >= 0x80 {
            return;
        }
        if n == 0 {
            self.hdr[G_WHO] = who as u8;
        }
        let i = n as usize;
        self.off_x[i] = 0;
        self.off_y[i] = 0;
        self.curr_x[i] = 0;
        self.curr_y[i] = 0;
        self.angles[i] = 0;
        self.list[i] = o as i16;
        self.set_num(n + 1);
        self.hdr[G_BUILDINGS] = is_b;
        self.set_i32(G_STAMP, frame);
        if obj_is_valid_unit(save, who, o) {
            let role = self.i32(G_ROLE) | type_i32(save, unit_ptype(save, who, o), 0x2c8).unwrap_or(0);
            self.set_i32(G_ROLE, role);
            let down = unit_i16(save, who, o, 0x90).unwrap_or(-1) as i32;
            if down >= 0 && obj_flags(save, who, down) & 1 != 0 {
                self.add(save, down, who, true, force, frame);
            }
        }
        if self.id() >= 0 {
            self.compute_speed(save);
        }
    }

    /// `Group::kill` 0x00714110 (vtable +0x10).
    fn kill(&mut self, save: &mut Save, o: i32, who: usize, sub: bool, force: bool, frame: i32) {
        if obj_is_unit(save, who, o) {
            if !sub && !is_captain(save, who, o) {
                let up = unit_i16(save, who, o, 0x8e).unwrap_or(-1) as i32;
                self.kill(save, up, who, false, force, frame);
                return;
            }
            let down = unit_i16(save, who, o, 0x90).unwrap_or(-1) as i32;
            if down >= 0 && (force || obj_flags(save, who, down) & 1 != 0) {
                self.kill(save, down, who, true, force, frame);
            }
        }
        if who != self.who() {
            return;
        }
        let n = self.num().max(0) as usize;
        let Some(idx) = (0..n).find(|&i| self.list[i] as i32 == o) else { return };
        if obj_is_unit(save, who, o) && unit_i16(save, who, o, 0x80).map(|g| g as i32) == Some(self.id()) {
            set_unit_i16(save, who, o, 0x80, -1);
        }
        self.set_i32(G_DISBAND, 0);
        self.remove_slot(idx);
        if self.num() == 0 {
            self.clear(-1, frame);
        } else {
            self.set_i32(G_STAMP, frame);
        }
        self.compute_speed(save);
    }

    /// `Group::equals_group` 0x00708000: same owner, (after normalizing both
    /// sides that have ids) same `get_num`, same `buildings`, same `list`.
    fn equals(&mut self, other: &mut GroupMem, save: &Save, frame: i32) -> bool {
        if self.who() != other.who() {
            return false;
        }
        if self.id() != -1 {
            self.normalize(save, frame);
        }
        if other.id() != -1 {
            other.normalize(save, frame);
        }
        let a = self.get_num(save, frame);
        let b = other.get_num(save, frame);
        if a != b || self.buildings() != other.buildings() {
            return false;
        }
        (0..self.num().max(0) as usize).all(|i| self.list[i] == other.list[i])
    }

    /// `Groups::copy_group` 0x006fa690 (`this` = slot, `src` = temp).
    fn copy_from(&mut self, src: &GroupMem, frame: i32) {
        self.hdr[G_WHO] = src.hdr[G_WHO];
        self.set_num(src.num());
        for off in [G_OX, G_OY, G_O_DIST, G_O_ANGLE, G_SPEED] {
            self.set_i32(off, src.i32(off));
        }
        self.hdr[G_BUILDINGS] = src.hdr[G_BUILDINGS];
        self.set_i32(G_STAMP, frame);
        let n = src.num().clamp(0, 128) as usize;
        self.list[..n].copy_from_slice(&src.list[..n]);
        self.angles[..n].copy_from_slice(&src.angles[..n]);
        self.off_x[..n].copy_from_slice(&src.off_x[..n]);
        self.off_y[..n].copy_from_slice(&src.off_y[..n]);
        self.curr_x[..n].copy_from_slice(&src.curr_x[..n]);
        self.curr_y[..n].copy_from_slice(&src.curr_y[..n]);
    }
}

/// Edit `Groups.list[idx]` in place through the in-memory model.
fn with_group<T>(save: &mut Save, idx: i32, f: impl FnOnce(&mut GroupMem) -> T) -> Result<T, ApplyError> {
    let slot = usize::try_from(idx)
        .ok()
        .filter(|&i| i < save.groups.list.elems.len())
        .ok_or_else(|| ApplyError::Shape(format!("Groups.list[{idx}] (len {})", save.groups.list.elems.len())))?;
    let mut g = GroupMem::from_saved(&save.groups.list.elems[slot])?;
    let r = f(&mut g);
    g.store(&mut save.groups.list.elems[slot]);
    Ok(r)
}

fn take_group(save: &Save, idx: i32) -> Result<GroupMem, ApplyError> {
    let slot = usize::try_from(idx)
        .ok()
        .filter(|&i| i < save.groups.list.elems.len())
        .ok_or_else(|| ApplyError::Shape(format!("Groups.list[{idx}] (len {})", save.groups.list.elems.len())))?;
    GroupMem::from_saved(&save.groups.list.elems[slot])
}

fn put_group(save: &mut Save, idx: i32, g: &GroupMem) {
    g.store(&mut save.groups.list.elems[idx as usize]);
}

/// `Groups.last_group[who]` (`DAT_00e85f2c + who*4`).
fn last_group(save: &Save, who: usize) -> i32 {
    save.groups
        .last_group
        .get(who * 4..who * 4 + 4)
        .map(|b| i32::from_le_bytes(b.try_into().unwrap()))
        .unwrap_or(-1)
}

fn set_last_group(save: &mut Save, who: usize, v: i32) {
    if let Some(b) = save.groups.last_group.get_mut(who * 4..who * 4 + 4) {
        b.copy_from_slice(&v.to_le_bytes());
    }
}

// ---------------------------------------------------------------------------
// Groups::push_group / get_open_slot
// ---------------------------------------------------------------------------

/// `Groups::push_group` 0x0070f9e0 `(who, &temp, force)`: reuse
/// `last_group[who]` when it equals the temp, else take an open slot and
/// copy into it; point every unit member's `group` at the slot, removing it
/// from its previous group first.
fn push_group(save: &mut Save, who: usize, temp: &mut GroupMem, force: bool, fx: &mut Vec<String>) -> Result<i32, ApplyError> {
    let frame = frame(save);
    if !force && temp.num() < 2 {
        // Singletons are not pushed: the unit just leaves its group.
        for i in 0..temp.num().max(0) as usize {
            let o = temp.list[i] as i32;
            if obj_is_valid_unit(save, who, o) {
                set_unit_i16(save, who, o, 0x80, -1);
            }
        }
        return Ok(-1);
    }
    let mut idx = last_group(save, who);
    let same = if idx >= 0 {
        let mut last = take_group(save, idx)?;
        let same = last.equals(temp, save, frame);
        put_group(save, idx, &last); // normalize/get_num may have compacted it
        same
    } else {
        false
    };
    if !same {
        idx = get_open_slot(save, who, fx)?;
        let mut slot = take_group(save, idx)?;
        slot.copy_from(temp, frame);
        put_group(save, idx, &slot);
        set_last_group(save, who, idx);
        fx.push(format!("Groups.list[{idx}] <- selection ({} members), Groups.last_group[{who}] = {idx}", temp.num()));
    } else {
        fx.push(format!("Groups.list[{idx}] reused (equals_group), Groups.last_group[{who}] unchanged"));
    }
    for i in 0..temp.num().max(0) as usize {
        let o = temp.list[i] as i32;
        if !obj_is_valid_unit(save, who, o) {
            continue;
        }
        let prev = unit_i16(save, who, o, 0x80).unwrap_or(-1) as i32;
        if prev >= 0 && prev != idx {
            let uo = unit_i16(save, who, o, 0x0a).unwrap_or(o as i16) as i32;
            let uwho = unit_u8(save, who, o, 0x09).unwrap_or(who as u8) as usize;
            let mut pg = take_group(save, prev)?;
            pg.kill(save, uo, uwho, false, false, frame);
            put_group(save, prev, &pg);
        }
        set_unit_i16(save, who, o, 0x80, idx as i16);
    }
    Ok(idx)
}

/// `Groups::get_open_slot` 0x006fa460 `(who, &temp)`: scan `who*64 ..
/// who*64+46`; the first empty (or buildings) slot that is not
/// `last_group[who]` wins immediately, else the stalest singular-captain
/// group (`get_num_cap() == 1`, oldest `stamp`), else (after the "NEED MORE
/// GROUPS" complaint) the stalest by stamp. Units still pointing at the
/// chosen non-building slot are detached.
fn get_open_slot(save: &mut Save, who: usize, fx: &mut Vec<String>) -> Result<i32, ApplyError> {
    let fr = frame(save);
    let base = (who * 0x40) as i32;
    let end = base + 0x2e;
    if save.groups.list.elems.len() < end as usize {
        return Err(ApplyError::Shape(format!("Groups.list has {} slots, need {}", save.groups.list.elems.len(), end)));
    }
    let last = last_group(save, who);
    let mut oldest_stamp = fr;
    let mut best = -1;
    let mut chosen = -1;
    for i in base..end {
        let mut g = take_group(save, i)?;
        let n = g.get_num(save, fr);
        let empty_or_build = n == 0 || g.buildings() != 0;
        if empty_or_build && i != last {
            put_group(save, i, &g);
            chosen = i;
            break;
        }
        if g.i32(G_STAMP) <= oldest_stamp && g.get_num_cap(save, fr) == 1 && i != last {
            oldest_stamp = g.i32(G_STAMP);
            best = i;
        }
        put_group(save, i, &g);
        chosen = best;
    }
    if chosen < 0 {
        fx.push("Groups::get_open_slot: 'UH OH, NEED MORE GROUPS!' fallback (stalest stamp)".into());
        for i in base..end {
            let g = take_group(save, i)?;
            if g.i32(G_STAMP) <= fr && i != last {
                chosen = i;
            }
        }
    }
    if chosen < 0 {
        return Err(ApplyError::Shape(format!("no group slot for who {who}")));
    }
    let g = take_group(save, chosen)?;
    if g.buildings() == 0 {
        // Detach units that still reference this slot: Objects.lists[who]
        // from the first unit index (`*PTR_DAT_00c06198`, 0) to
        // `obj_mark[0][who]` == `unit_mark[who]`, the unit plane bound.
        let n = unit_mark(save, who);
        for o in 0..n {
            if obj_is_unit(save, who, o) && unit_i16(save, who, o, 0x80) == Some(chosen as i16) {
                set_unit_i16(save, who, o, 0x80, -1);
            }
        }
    }
    Ok(chosen)
}

// ---------------------------------------------------------------------------
// CommandPackage::process_group 0x0094a0c0
// ---------------------------------------------------------------------------

/// `GroupCommand { u8 num; i8 who; i16 list[num] }`. Builds a stack `Group`
/// from the selection (or the player's cached last selection when `num ==
/// 0`), stamps `Unit::play` on every selected unit and its cargo chain,
/// then `Groups::push_group(who, &temp, 1)` into `package.group`.
fn process_group(
    save: &mut Save,
    ctx: &mut CommandContext,
    pkg: &mut Package,
    cmd: &[u8],
    fx: &mut Vec<String>,
) -> Result<(), ApplyError> {
    let num = cmd[1] as usize;
    let who_i = cmd[2] as i8 as i32;
    let play = pkg.play;
    let fr = frame(save);
    let mut temp = GroupMem::temp(fr);
    // `Game + play*0x8c + 0x77` = Player[play].who.
    let player_who = save.game.info.players.get(play).and_then(|p| p.body.get(0x33)).copied().map(i32::from);
    if player_who != Some(who_i) {
        // LeaderData::is_team(who, 0) — an observer/teammate selecting
        // another slot's objects; retail only logs. `package.group` is left
        // as it was.
        fx.push(format!("GroupCommand who {who_i} != Player[{play}].who {player_who:?}: ignored"));
        return Ok(());
    }
    let who = who_i as usize;
    if who >= 8 {
        return Err(ApplyError::Shape(format!("GroupCommand who {who}")));
    }
    let mut count = 0usize;
    if num == 0 {
        let sel = ctx.last_selection[play].clone();
        if sel.num == 0 {
            pkg.group = -1;
            fx.push("GroupCommand num=0 with empty last selection: group = -1".into());
            return Ok(());
        }
        for i in 0..sel.num as usize {
            let o = sel.o[i] as i32;
            if obj_flags(save, who, o) & 1 != 0 && object_uid(save, who, o) == Some(sel.uid[i]) {
                temp.add(save, o, who, false, false, fr);
                stamp_play_chain(save, who, o, play, &mut temp, fr);
                count = 1;
            }
        }
        fx.push(format!("GroupCommand num=0: re-selected {} cached objects for who {who}", sel.num));
    } else {
        let net = game_flags(save) & 0x10 != 0;
        let sel = &mut ctx.last_selection[play];
        sel.num = num as u8;
        let umark = unit_mark(save, who);
        for i in 0..num {
            let o = i16::from_le_bytes([cmd[3 + i * 2], cmd[4 + i * 2]]) as i32;
            if net && !obj_present(save, who, o) {
                fx.push(format!("GroupCommand: object {who}/{o} absent -- broken replay"));
                continue;
            }
            if obj_flags(save, who, o) & 1 == 0 {
                continue;
            }
            if obj_is_unit(save, who, o) && o >= umark {
                fx.push(format!("GroupCommand: o {o} >= objects.unit_mark[{who}] {umark} (retail asserts, skips)"));
                continue;
            }
            temp.add(save, o, who, false, false, fr);
            if count < 128 {
                ctx.last_selection[play].o[count] = o as i16;
                ctx.last_selection[play].uid[count] = object_uid(save, who, o).unwrap_or(0);
            }
            count += 1;
            stamp_play_chain(save, who, o, play, &mut temp, fr);
        }
        ctx.last_selection[play].num = count.min(255) as u8;
        fx.push(format!("GroupCommand: {count}/{num} objects selected for who {who}; temp group num={}", temp.num()));
    }
    if count != 0 {
        pkg.group = push_group(save, who, &mut temp, true, fx)?;
    } else {
        pkg.group = -1;
    }
    fx.push(format!("package.group = {}", pkg.group));
    Ok(())
}

/// The `is_unit` arm shared by both `process_group` loops: `Unit::play =
/// play` on the object, then walk `o_down` (cargo chain) stamping `play`
/// and adding each to the temp group.
fn stamp_play_chain(save: &mut Save, who: usize, o: i32, play: usize, temp: &mut GroupMem, fr: i32) {
    if !obj_is_unit(save, who, o) {
        return;
    }
    set_unit_u8(save, who, o, 0xb6, play as u8);
    let mut d = unit_i16(save, who, o, 0x90).unwrap_or(-1) as i32;
    let mut guard = 0;
    while d >= 0 && guard < 128 {
        set_unit_u8(save, who, d, 0xb6, play as u8);
        temp.add(save, d, who, false, false, fr);
        d = unit_i16(save, who, d, 0x90).unwrap_or(-1) as i32;
        guard += 1;
    }
}

// ---------------------------------------------------------------------------
// Group::action_halt 0x0070d0c0 (Partial)
// ---------------------------------------------------------------------------

/// `Group::action_halt(flags)`: `action_begin` (disband = 0); for a
/// non-building group `army = -1` and every active on-map member that is
/// not an airborne plane and not entering/exiting a building gets
/// `Unit::clear_orders` 0x005e3860: `unit_masks &= ~0x0400_0000`,
/// `path.length = 0`, `close_orders` (pop orders from the tail while the
/// tail's OrderIndex != 0 — i.e. every walked order), `clear_partial_path`
/// (pathfinder scratch, not walked), `update_action` (`orders_x/y = x/y`,
/// `dest_angle = angle`), then `unit_masks &= ~0x100`.
///
/// Partial: `Unit::kill_current_order` 0x005e2cb0 (1,312 B) also releases
/// the order's target bookkeeping (targeted counts, gather sites, build
/// helpers) on other objects; those writes are not transcribed. The
/// `DAT_00cc02f8` branch (editor selection) is off in play.
fn action_halt(save: &mut Save, gidx: i32, flags: u32, fx: &mut Vec<String>) -> Result<StepStatus, ApplyError> {
    let fr = frame(save);
    let mut g = take_group(save, gidx)?;
    g.set_i32(G_DISBAND, 0);
    if g.buildings() == 0 {
        g.set_i32(G_ARMY, -1);
        let who = g.who();
        for i in 0..g.num().max(0) as usize {
            let o = g.list[i] as i32;
            if !obj_is_valid_unit(save, who, o) || !unit_is_on_map(save, who, o) {
                continue;
            }
            let pt = unit_ptype(save, who, o);
            // UnitData::is_plane: kind == 2 && !(flags & 0x20).
            let plane = type_i32(save, pt, 0x218) == Some(2) && type_i32(save, pt, 0x2b4).unwrap_or(0) & 0x20 == 0;
            if plane {
                // is_plane() != 0 -> skip (airborne planes keep their orders)
                continue;
            }
            if unit_is_entering_or_exiting(save, who, o) {
                continue;
            }
            if flags & 4 != 0 {
                // ptype->vtable+0x10c (UnitType::is_... ) gate: not modelled.
            }
            if flags & 2 != 0 && type_i32(save, pt, 0x2b8).unwrap_or(0) & 0x10 != 0 {
                continue;
            }
            if flags & 1 != 0 {
                // ObjectData::is(0x3a, 0) gate: not modelled.
            }
            unit_clear_orders(save, who, o, fx);
            if let Some(m) = unit_u32(save, who, o, 0x68) {
                set_unit_u32(save, who, o, 0x68, m & !0x100);
            }
        }
    }
    put_group(save, gidx, &g);
    let _ = fr;
    Ok(StepStatus::Partial)
}

/// `Unit::clear_orders` 0x005e3860 + the order-list pop of `close_orders`.
fn unit_clear_orders(save: &mut Save, who: usize, o: i32, fx: &mut Vec<String>) {
    if let Some(m) = unit_u32(save, who, o, 0x68) {
        set_unit_u32(save, who, o, 0x68, m & !0x0400_0000);
    }
    let Some(u) = unit_mut(save, who, o) else { return };
    u.path.data.clear();
    u.path.len = 0;
    let dropped = u.orders.orders.len();
    // close_orders: while tail.order_index() != 0 { kill_current_order }.
    // Every OrderIndex the walker persists (1..4, 6, 7, 14) is non-zero.
    while u.orders.orders.last().map(|od| od.ty != 0).unwrap_or(false) {
        u.orders.orders.pop();
    }
    u.orders.count = u.orders.orders.len() as i32;
    // update_action: orders_x/y = x/y (both stored ^0x63637 so the copy is
    // byte-for-byte), dest_angle = angle; with no orders left that is final.
    let x = u.base.sub.body.get(7..11).map(|b| b.to_vec());
    let y = u.base.sub.body.get(11..15).map(|b| b.to_vec());
    if let (Some(x), Some(y)) = (x, y) {
        u.body[0x70 - 0x48..0x74 - 0x48].copy_from_slice(&x);
        u.body[0x74 - 0x48..0x78 - 0x48].copy_from_slice(&y);
    }
    let angle = u.body[0x50 - 0x48..0x54 - 0x48].to_vec();
    u.body[0x58 - 0x48..0x5c - 0x48].copy_from_slice(&angle);
    fx.push(format!("Unit {who}/{o}: clear_orders (dropped {dropped} orders, path.length=0, orders_xy=xy, dest_angle=angle)"));
}

// ---------------------------------------------------------------------------
// Group::action_disband 0x0070e260 (Partial) / action_stance 0x0070d440 (Partial)
// ---------------------------------------------------------------------------

/// `Group::action_disband(all)`: not yet transcribed beyond the argument;
/// the body sells the group's units back (`Leader` resource credits,
/// `Objects::kill` of each member) — both outside this crate's ported set.
fn action_disband(_save: &mut Save, gidx: i32, all: i32, fx: &mut Vec<String>) -> Result<StepStatus, ApplyError> {
    fx.push(format!("Group::action_disband 0x0070e260 on Groups.list[{gidx}] (all={all}) not transcribed (Objects::kill + resource refund)"));
    Ok(StepStatus::Stub)
}

/// `Group::action_stance(stance)`: for an on-map group whose stance type
/// (leader's `ObjectType::get_stance_type`, vtable +0x138) is 0..3, every
/// member whose type reports the same stance type and is not a plane gets
/// `Unit::stance = stance` (+0xb1) and `flags |= 0x10` (+0x08); buildings
/// get `Build+0x7e = stance`. Negative `stance` cycles (-1 next, -2 prev)
/// through 6/4/2 values by stance type; combat stances 0/3/4 then
/// `Unit::clear_orders`-lite (0x005e3890: reset `TargetOrder+0x1c` on
/// Attack(10) orders), stance 1/2/5 may re-target.
///
/// Partial: `ObjectType::get_stance_type` is a virtual on the type class
/// whose per-class body is not in `TypeRec`'s decoded range, so the type
/// filter is approximated by "unit types" and the cycle/re-target branches
/// are reported, not applied.
fn action_stance(save: &mut Save, gidx: i32, stance: i32, fx: &mut Vec<String>) -> Result<StepStatus, ApplyError> {
    let g = take_group(save, gidx)?;
    if g.num() == 0 || stance < 0 {
        fx.push(format!("action_stance: group num={} stance={stance} -> cycle/no-op not applied", g.num()));
        return Ok(StepStatus::Stub);
    }
    let who = g.who();
    // GroupData::is_on_map: any active captain that is on the map.
    let on_map = g.buildings() != 0
        || (0..g.num() as usize).any(|i| {
            let o = g.list[i] as i32;
            obj_is_valid_unit(save, who, o) && is_captain(save, who, o) && unit_is_on_map(save, who, o)
        });
    if !on_map {
        return Ok(StepStatus::Partial);
    }
    with_group(save, gidx, |g| g.set_i32(G_DISBAND, 0))?;
    let mut n = 0;
    for i in 0..g.num() as usize {
        let o = g.list[i] as i32;
        if obj_flags(save, who, o) & 1 == 0 || !obj_is_unit(save, who, o) {
            continue;
        }
        let pt = unit_ptype(save, who, o);
        let plane = type_i32(save, pt, 0x218) == Some(2) && type_i32(save, pt, 0x2b4).unwrap_or(0) & 0x20 == 0;
        if plane {
            continue;
        }
        set_unit_u8(save, who, o, 0xb1, stance as u8);
        if let Some(u) = unit_mut(save, who, o) {
            u.base.sub.flags |= 0x10;
        }
        n += 1;
    }
    fx.push(format!("action_stance: Unit.stance = {stance}, flags |= 0x10 on {n} members of Groups.list[{gidx}] (type filter approximated)"));
    Ok(StepStatus::Partial)
}

// ---------------------------------------------------------------------------
// FormData::type_cat 0x0072dfc0 and UnitData::speed 0x0060aae0
// ---------------------------------------------------------------------------

/// `FormData::type_cat(type, who, who)`: a 0..10 category from the type's
/// flag words (+0x1e4 unit flags, +0x1e8, +0x1fc, +0x2b8 flags2). The
/// `UnitType::vftable` fast paths read the fields directly; other type
/// classes answer the same flags through `has_flag`, which this follows.
fn type_cat(save: &Save, ptype: i32, who: usize) -> i32 {
    let f = |off: usize| type_i32(save, ptype, off).unwrap_or(0) as u32;
    let f1e4 = f(0x1e4);
    let f2b8 = f(0x2b8);
    if f(0x1e8) != 0 && f2b8 & 0x40 == 0 && f2b8 & 8 == 0 {
        let idx = type_i32(save, ptype, 0x04).unwrap_or(-1);
        if idx != 0x3d && idx != 0x3e && idx != 400 {
            if f1e4 & 4 != 0 {
                return 10;
            }
            if f1e4 & 0x20 == 0 {
                if f2b8 & 4 != 0 || f1e4 & 0x8000_0000 != 0 {
                    return 6;
                }
                if f1e4 & 0x1000 == 0 {
                    return if f1e4 & 0x20_0000 != 0 { 0 } else { 6 };
                }
                let leader_flags = save.leaders.slots.get(who).map(|l| l.flags).unwrap_or(0);
                if f(0x1fc) != 0 && leader_flags & 4 != 0 {
                    return 5;
                }
                return 2;
            }
            return 3 + (f(0x1fc) != 0) as i32;
        }
    }
    if f2b8 & 0x60 != 0 {
        return 6;
    }
    8 + 2 * (f1e4 & 4 != 0) as i32
}

/// `UnitData::speed` 0x0060aae0: `myspeed` (+0x9a) plus terrain/road,
/// tribe-bonus, general-aura and tech multipliers. Only the base term is
/// transcribed here; the modifiers read `LeaderData` tribe-bonus shorts
/// (+0x59cc..+0x59e8), `Constants` (+0x838..+0x844, +0xb4c..+0xbd4,
/// +0xc50), the terrain region table and `ObjectData::has_general`.
fn unit_speed(save: &Save, who: usize, o: i32) -> i32 {
    unit_i16(save, who, o, 0x9a).unwrap_or(0) as i32
}

// ---------------------------------------------------------------------------
// Object / Unit / type accessors over the Save
// ---------------------------------------------------------------------------

fn frame(save: &Save) -> i32 {
    i32::from_le_bytes(save.game.scalars[0..4].try_into().unwrap())
}

/// `Game+0x820` flag byte (network/replay flags). Lives in `Game.scalars`
/// at 0x820-0x550 when the walked block covers it (it does: 404 bytes).
fn game_flags(save: &Save) -> u8 {
    save.game.scalars.get(0x820 - 0x550).copied().unwrap_or(0)
}

/// `Objects.scalars`: valid, ammo_index, good_mark, rare_mark, then
/// unit_mark[9] at byte 16.
fn unit_mark(save: &Save, who: usize) -> i32 {
    let o = 16 + who * 4;
    save.objects
        .scalars
        .get(o..o + 4)
        .map(|b| i32::from_le_bytes(b.try_into().unwrap()))
        .unwrap_or(0)
}

fn obj<'a>(save: &'a Save, who: usize, o: i32) -> Option<&'a Obj> {
    save.objects.lists.get(who)?.elems.get(usize::try_from(o).ok()?)?.as_ref()
}

fn obj_present(save: &Save, who: usize, o: i32) -> bool {
    obj(save, who, o).is_some()
}

/// `SubObject+0x08` flags.
fn obj_flags(save: &Save, who: usize, o: i32) -> u8 {
    obj(save, who, o).map(Obj::obj_flags).unwrap_or(0)
}

/// Object vtable +0x18 (`is_unit`): 1 for Unit/Animal, 0 for Build.
fn obj_is_unit(save: &Save, who: usize, o: i32) -> bool {
    matches!(obj(save, who, o), Some(Obj::Unit(_)) | Some(Obj::Animal(_)))
}

/// Object vtable +0x1c (`is_build`).
fn obj_is_build(save: &Save, who: usize, o: i32) -> bool {
    matches!(obj(save, who, o), Some(Obj::Build(_)))
}

/// Object vtable +0x20 (`is_wall`): Build/Wall arms return 1.
fn obj_is_wall(save: &Save, who: usize, o: i32) -> bool {
    obj_is_build(save, who, o)
}

/// Object vtable +0x08 (`is_valid_unit`): Unit/Animal `flags & 1`, Build 0.
fn obj_is_valid_unit(save: &Save, who: usize, o: i32) -> bool {
    obj_is_unit(save, who, o) && obj_flags(save, who, o) & 1 != 0
}

fn unit_ref(save: &Save, who: usize, o: i32) -> Option<&Unit> {
    match obj(save, who, o)? {
        Obj::Unit(u) => Some(u),
        Obj::Animal(a) => Some(&a.unit),
        Obj::Build(_) => None,
    }
}

fn unit_mut(save: &mut Save, who: usize, o: i32) -> Option<&mut Unit> {
    match save.objects.lists.get_mut(who)?.elems.get_mut(usize::try_from(o).ok()?)?.as_mut()? {
        Obj::Unit(u) => Some(u),
        Obj::Animal(a) => Some(&mut a.unit),
        Obj::Build(_) => None,
    }
}

/// Read a UnitData field by image offset across the three walked ranges
/// (`sub.body` +0x09..+0x1c, `mid` +0x20..+0x42, `body` +0x48..+0xb7).
fn unit_bytes(u: &Unit, off: usize, n: usize) -> Option<&[u8]> {
    match off {
        0x09..=0x1b => u.base.sub.body.get(off - 0x09..off - 0x09 + n),
        0x20..=0x41 => u.base.mid.get(off - 0x20..off - 0x20 + n),
        0x48..=0xb6 => u.body.get(off - 0x48..off - 0x48 + n),
        _ => None,
    }
}

fn unit_bytes_mut(u: &mut Unit, off: usize, n: usize) -> Option<&mut [u8]> {
    match off {
        0x09..=0x1b => u.base.sub.body.get_mut(off - 0x09..off - 0x09 + n),
        0x20..=0x41 => u.base.mid.get_mut(off - 0x20..off - 0x20 + n),
        0x48..=0xb6 => u.body.get_mut(off - 0x48..off - 0x48 + n),
        _ => None,
    }
}

fn unit_u8(save: &Save, who: usize, o: i32, off: usize) -> Option<u8> {
    unit_bytes(unit_ref(save, who, o)?, off, 1).map(|b| b[0])
}

fn unit_i16(save: &Save, who: usize, o: i32, off: usize) -> Option<i16> {
    unit_bytes(unit_ref(save, who, o)?, off, 2).map(|b| i16::from_le_bytes([b[0], b[1]]))
}

fn unit_u32(save: &Save, who: usize, o: i32, off: usize) -> Option<u32> {
    unit_bytes(unit_ref(save, who, o)?, off, 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()))
}

fn set_unit_u8(save: &mut Save, who: usize, o: i32, off: usize, v: u8) {
    if let Some(b) = unit_mut(save, who, o).and_then(|u| unit_bytes_mut(u, off, 1)) {
        b[0] = v;
    }
}

fn set_unit_i16(save: &mut Save, who: usize, o: i32, off: usize, v: i16) {
    if let Some(b) = unit_mut(save, who, o).and_then(|u| unit_bytes_mut(u, off, 2)) {
        b.copy_from_slice(&v.to_le_bytes());
    }
}

fn set_unit_u32(save: &mut Save, who: usize, o: i32, off: usize, v: u32) {
    if let Some(b) = unit_mut(save, who, o).and_then(|u| unit_bytes_mut(u, off, 4)) {
        b.copy_from_slice(&v.to_le_bytes());
    }
}

/// `SubObject+0x18` ptype, serialized as the type index.
fn unit_ptype(save: &Save, who: usize, o: i32) -> i32 {
    unit_ref(save, who, o)
        .and_then(|u| u.base.sub.body.get(15..19))
        .map(|b| i32::from_le_bytes(b.try_into().unwrap()))
        .unwrap_or(-1)
}

/// `UnitData::is_captain` 0x0046ceb0: `o_up < 0`.
fn is_captain(save: &Save, who: usize, o: i32) -> bool {
    unit_i16(save, who, o, 0x8e).map(|v| v < 0).unwrap_or(false)
}

/// `UnitData::is_on_map` 0x0046ce30: `inside_up < 0`.
fn unit_is_on_map(save: &Save, who: usize, o: i32) -> bool {
    unit_i16(save, who, o, 0x82).map(|v| v < 0).unwrap_or(false)
}

/// `UnitData::is_entering_or_exiting` 0x0060a6f0: the head order's
/// `get_target` (+0x11c) object with state 0/1. Only Garrison/Board-class
/// orders carry a target; none of the OrderIndex values the walker
/// persists (1..4, 6, 7, 14) do, so this is false for every walked save.
fn unit_is_entering_or_exiting(save: &Save, who: usize, o: i32) -> bool {
    let _ = (save, who, o);
    false
}

/// `Object+0x30` uid.
fn object_uid(save: &Save, who: usize, o: i32) -> Option<i16> {
    let mid = match obj(save, who, o)? {
        Obj::Unit(u) => &u.base.mid,
        Obj::Animal(a) => &a.unit.base.mid,
        Obj::Build(b) => &b.base.mid,
    };
    mid.get(0x10..0x12).map(|v| i16::from_le_bytes([v[0], v[1]]))
}

/// `Rules.types[idx]` i32 at image offset `off` (same ranges as
/// `objects_process::TypeImg`: head 0x04..0x5e, obj_mid 0x1e4..0x27c, ext
/// 0x2b4..0x2cc then 0x2d4.. shifted by the 8-byte gap).
fn type_i32(save: &Save, idx: i32, off: usize) -> Option<i32> {
    let t = save.rules_tail.rules.types.get(usize::try_from(idx).ok()?)?;
    let (v, i) = match off {
        0x04..=0x5d => (&t.head, off - 4),
        0x1e4..=0x27b => (&t.obj_mid, off - 0x1e4),
        0x2b4..=0x2cb => (&t.ext, off - 0x2b4),
        0x2d4.. => (&t.ext, off - 0x2b4 - 8),
        _ => return None,
    };
    v.get(i..i + 4).map(|b| i32::from_le_bytes(b.try_into().unwrap()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_len_matches_retail_handlers() {
        assert_eq!(wire_len(&[0x00, 3, 0, 1, 0, 2, 0, 3, 0]).unwrap(), 9);
        assert_eq!(wire_len(&[0x07; 22]).unwrap(), 22);
        assert_eq!(wire_len(&[0x0c]).unwrap(), 1);
        assert_eq!(wire_len(&[0x33, 0, 0, 0, 2, 0]).unwrap(), 6 + 16);
        let mut chat = vec![0x44u8; 17];
        chat[13..17].copy_from_slice(&3i32.to_le_bytes());
        assert_eq!(wire_len(&chat).unwrap(), 19 + 6);
        assert!(matches!(wire_len(&[0x52]), Err(ApplyError::UnknownOpcode { .. })));
        assert!(matches!(wire_len(&[0x00]), Err(ApplyError::Truncated { .. })));
    }

    #[test]
    fn split_commands_tiles_payload() {
        let mut p = vec![0x00, 1, 0, 5, 0]; // GroupCommand num=1 who=0 o=5
        p.extend_from_slice(&[0x0c]); // Halt
        p.extend_from_slice(&[0x02, 1, 0, 0, 0]); // Stance 1
        let cmds = split_commands(&p).unwrap();
        assert_eq!(cmds.iter().map(|c| c[0]).collect::<Vec<_>>(), vec![0x00, 0x0c, 0x02]);
        assert!(matches!(split_commands(&[0x02, 1]), Err(ApplyError::Truncated { offset: 0, .. })));
    }

    #[test]
    fn group_mem_roundtrips_saved_group() {
        let mut g = GroupMem::temp(77);
        g.set_i32(G_ID, 12);
        g.hdr[G_WHO] = 3;
        g.list[0] = 40;
        g.list[1] = 41;
        g.off_x[1] = -5;
        g.angles[1] = -3;
        g.set_num(2);
        let mut s = Group::default();
        g.store(&mut s);
        assert_eq!(s.hdr.len(), 72);
        assert_eq!(s.list.len(), 4);
        assert_eq!(s.off_x.len(), 8);
        assert_eq!(s.angles.len(), 2);
        let back = GroupMem::from_saved(&s).unwrap();
        assert_eq!(back.num(), 2);
        assert_eq!(back.list[1], 41);
        assert_eq!(back.off_x[1], -5);
        assert_eq!(back.angles[1], -3);
        assert_eq!(back.i32(G_STAMP), 77);
        assert_eq!(back.i32(G_FORM), -1);
        assert_eq!(back.i32(G_ARMY), -1);
    }

    #[test]
    fn remove_slot_shifts_all_planes() {
        let mut g = GroupMem::temp(0);
        for i in 0..3 {
            g.list[i] = i as i16 + 10;
            g.curr_y[i] = i as i32 * 100;
        }
        g.set_num(3);
        g.remove_slot(0);
        assert_eq!(g.num(), 2);
        assert_eq!(&g.list[..2], &[11, 12]);
        assert_eq!(&g.curr_y[..2], &[100, 200]);
    }

    #[test]
    fn apply_package_rejects_bad_player() {
        let mut s = Save::default();
        assert!(matches!(apply_package(&mut s, 8, &[0x0c]), Err(ApplyError::BadPlayer(8))));
    }

    /// First stride-1 capture save under `schema/live/frame-pairs/`, if the
    /// proprietary captures are present.
    fn live_save() -> Option<(std::path::PathBuf, Save)> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().ok()?;
        let mut dirs: Vec<_> = std::fs::read_dir(root.join("schema/live/frame-pairs")).ok()?.flatten().map(|e| e.path()).collect();
        dirs.sort();
        for d in dirs {
            let mut svx: Vec<_> = std::fs::read_dir(&d)
                .ok()?
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("svx"))
                .collect();
            svx.sort();
            if let Some(p) = svx.into_iter().next() {
                let raw = crate::container::load_svx(&p).ok()?;
                let img = crate::load(&raw).ok()?;
                return Some((p, img.state));
            }
        }
        None
    }

    /// Layout facts the transcription leans on, checked against a live save:
    /// 8 owners x 64 Group slots, `Groups.last_group` indices inside the
    /// owner's block (or -1), `Player.body[0x33]` (`who`) a slot index, and
    /// every unit's `group` either -1 or a slot whose `list` contains it.
    #[test]
    fn live_save_layout_facts() {
        let Some((path, save)) = live_save() else {
            eprintln!("no live captures; skipping");
            return;
        };
        eprintln!("probing {}", path.display());
        assert_eq!(save.groups.list.elems.len(), 512, "Groups.list is 8 x 64 slots");
        assert_eq!(save.groups.last_group.len(), 32);
        for who in 0..8 {
            let lg = last_group(&save, who);
            assert!(lg == -1 || (who as i32 * 64..who as i32 * 64 + 64).contains(&lg), "last_group[{who}] = {lg}");
            if let Some(p) = save.game.info.players.get(who) {
                if let Some(&w) = p.body.get(0x33) {
                    assert!(w < 8, "Player[{who}].who = {w}");
                }
            }
        }
        for (slot, g) in save.groups.list.elems.iter().enumerate() {
            let gm = GroupMem::from_saved(g).unwrap();
            assert_eq!(gm.id(), slot as i32, "Groups.list[{slot}].id");
            if gm.num() > 0 {
                assert_eq!(gm.who(), slot / 64, "Groups.list[{slot}].who vs owner block");
            }
        }
        let mut units = 0;
        let mut grouped = 0;
        for who in 0..8 {
            let Some(l) = save.objects.lists.get(who) else { continue };
            for o in 0..l.elems.len() as i32 {
                if !obj_is_valid_unit(&save, who, o) {
                    continue;
                }
                units += 1;
                let g = unit_i16(&save, who, o, 0x80).unwrap() as i32;
                if g >= 0 {
                    grouped += 1;
                    let gm = take_group(&save, g).unwrap();
                    assert!(
                        (0..gm.num().max(0) as usize).any(|i| gm.list[i] as i32 == o),
                        "unit {who}/{o} says group {g} but Groups.list[{g}].list lacks it"
                    );
                }
            }
        }
        eprintln!("{units} active units, {grouped} in groups");
    }

    /// Synthetic GroupCommand on a live save: select the first two active
    /// captains of the human slot and check the writes `process_group`
    /// performs are mutually consistent and the save still round-trips.
    #[test]
    fn live_save_group_command_consistency() {
        let Some((_, mut save)) = live_save() else {
            eprintln!("no live captures; skipping");
            return;
        };
        let play = 0usize;
        let who = save.game.info.players[play].body.get(0x33).copied().unwrap_or(0) as usize;
        let mut picks = Vec::new();
        for o in 0..unit_mark(&save, who) {
            if obj_is_valid_unit(&save, who, o) && is_captain(&save, who, o) && unit_is_on_map(&save, who, o) {
                picks.push(o as i16);
                if picks.len() == 2 {
                    break;
                }
            }
        }
        assert_eq!(picks.len(), 2, "need two captains for who {who}");
        let mut cmd = vec![0x00u8, 2, who as u8];
        for p in &picks {
            cmd.extend_from_slice(&p.to_le_bytes());
        }
        cmd.push(0x01); // BeginCommand on the new group
        let before = save.clone();
        let rep = apply_package(&mut save, play, &cmd).unwrap();
        assert_eq!(rep.commands.len(), 2);
        assert_eq!(rep.commands[0].status, StepStatus::Ported);
        let g = rep.group;
        assert!(g >= 0, "GroupCommand must resolve a group: {:?}", rep.commands[0].effects);
        assert_eq!(last_group(&save, who), g);
        let gm = take_group(&save, g).unwrap();
        assert_eq!(gm.who(), who);
        assert_eq!(gm.buildings(), 0);
        assert_eq!(gm.i32(G_DISBAND), 0);
        assert_eq!(gm.i32(G_STAMP), frame(&save));
        let members: Vec<i16> = (0..gm.num() as usize).map(|i| gm.list[i]).collect();
        for p in &picks {
            assert!(members.contains(p), "pick {p} missing from {members:?}");
            assert_eq!(unit_i16(&save, who, *p as i32, 0x80), Some(g as i16));
            assert_eq!(unit_u8(&save, who, *p as i32, 0xb6), Some(play as u8));
        }
        // Every other group byte-for-byte unchanged except ones that lost a
        // member to the new group and the previous `last_group[who]`, which
        // `equals_group` normalizes (role / speed recomputed).
        let prev_last = last_group(&before, who);
        for (slot, (a, b)) in before.groups.list.elems.iter().zip(save.groups.list.elems.iter()).enumerate() {
            if slot as i32 == g || slot as i32 == prev_last {
                continue;
            }
            let ga = GroupMem::from_saved(a).unwrap();
            let lost = (0..ga.num().max(0) as usize).any(|i| picks.contains(&ga.list[i]));
            if !lost {
                assert_eq!(a.hdr, b.hdr, "Groups.list[{slot}].hdr changed");
                assert_eq!(a.list, b.list, "Groups.list[{slot}].list changed");
            }
        }
        // The edited state still walks.
        let bytes = crate::save(&mut save).expect("save after GroupCommand");
        crate::load(&bytes).expect("reload after GroupCommand");
        // Re-issuing the same selection reuses the slot (equals_group).
        let rep2 = apply_package(&mut save, play, &cmd[..cmd.len() - 1]).unwrap();
        assert_eq!(rep2.group, g, "{:?}", rep2.commands[0].effects);
    }
}
