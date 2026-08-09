//! Exact executable prefixes for the still-open command tail rows 73 and 78.
//!
//! These plans intentionally stop at the first unrecovered callee.  A typed handoff is a
//! map for future integration, not evidence that the whole opcode is complete.

pub const LEADER_OPTIONS_OPCODE: u8 = 73;
pub const LEADER_OPTIONS_WIRE_BYTES: usize = 33;
pub const CONSOLE_COMMAND_OPCODE: u8 = 78;
pub const CONSOLE_COMMAND_WIRE_BYTES: usize = 521;
pub const CONSOLE_COMMAND_UNITS: usize = 256;
pub const LEADER_OPTION_ROWS: usize = 10;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AdjacentCommandDecodeError {
    WrongLength { expected: usize, actual: usize },
    WrongOpcode { expected: u8, actual: u8 },
}

fn require_fixed(bytes: &[u8], opcode: u8, len: usize) -> Result<(), AdjacentCommandDecodeError> {
    if bytes.len() != len {
        return Err(AdjacentCommandDecodeError::WrongLength {
            expected: len,
            actual: bytes.len(),
        });
    }
    if bytes[0] != opcode {
        return Err(AdjacentCommandDecodeError::WrongOpcode {
            expected: opcode,
            actual: bytes[0],
        });
    }
    Ok(())
}

fn i32_at(bytes: &[u8], offset: usize) -> i32 {
    i32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("fixed slice"))
}

/// Raw 16-byte `BitMask<32>` representation embedded in `LeaderOptionData`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BitMask32Wire {
    pub bits: i32,
    pub size: i32,
    pub flags: i32,
    pub inline: [u8; 4],
}

/// Safe in-process representation of the same four columns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BitMask32State {
    pub bits: i32,
    pub size: i32,
    pub flags: i32,
    pub inline: [u8; 4],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeaderOptionDataWire {
    pub who: i32,
    pub peasants: i32,
    pub peasants_wait: i32,
    pub buildings: i32,
    pub flags: BitMask32Wire,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeaderOptionsCommand {
    pub data: LeaderOptionDataWire,
}

pub fn decode_leader_options(
    bytes: &[u8],
) -> Result<LeaderOptionsCommand, AdjacentCommandDecodeError> {
    require_fixed(bytes, LEADER_OPTIONS_OPCODE, LEADER_OPTIONS_WIRE_BYTES)?;
    Ok(LeaderOptionsCommand {
        data: LeaderOptionDataWire {
            who: i32_at(bytes, 1),
            peasants: i32_at(bytes, 5),
            peasants_wait: i32_at(bytes, 9),
            buildings: i32_at(bytes, 13),
            flags: BitMask32Wire {
                bits: i32_at(bytes, 17),
                size: i32_at(bytes, 21),
                flags: i32_at(bytes, 25),
                inline: bytes[29..33].try_into().expect("fixed slice"),
            },
        },
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeaderOptionDataState {
    pub who: i32,
    pub peasants: i32,
    pub peasants_wait: i32,
    pub buildings: i32,
    pub flags: BitMask32State,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeaderOptionRowReceipt {
    pub who: i32,
    pub state: LeaderOptionDataState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LeaderOptionsPrefixError {
    UnsafeOwner { who: i32 },
    PreviousRowMismatch,
    UnsafeBitMaskSize { size: i32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeaderOptionChangeSet {
    pub peasants_changed: bool,
    pub buildings_changed: bool,
    pub flag_bit_1_changed: bool,
    pub flag_bit_3_changed: bool,
    pub flag_bit_4_changed: bool,
    pub mirror_to_local_option: bool,
}

impl LeaderOptionChangeSet {
    pub fn needs_open_tail(self) -> bool {
        self.peasants_changed
            || self.buildings_changed
            || self.flag_bit_1_changed
            || self.flag_bit_3_changed
            || self.flag_bit_4_changed
            || self.mirror_to_local_option
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeaderOptionsCascadeRequest {
    pub who: i32,
    pub previous: LeaderOptionDataState,
    pub stored: LeaderOptionDataState,
    pub changes: LeaderOptionChangeSet,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AdjacentPrefixPresentationReceipt {
    LeaderOptionsDiagnostic {
        data: LeaderOptionDataWire,
    },
    ConsoleCommandDiagnostic {
        mouse_x: i32,
        mouse_y: i32,
        command: [u16; CONSOLE_COMMAND_UNITS],
    },
    ConsoleMouseStore {
        mouse_x: i32,
        mouse_y: i32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AdjacentOpenTailRequest {
    LeaderOptionsCascade(LeaderOptionsCascadeRequest),
    ParseConsoleCommand {
        mouse_x: i32,
        mouse_y: i32,
        command: [u16; CONSOLE_COMMAND_UNITS],
        first_mode: i32,
        second_mode: i32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeaderOptionsPrefixPlan {
    pub presentation: AdjacentPrefixPresentationReceipt,
    pub stored: LeaderOptionDataState,
    pub open_tail: Option<AdjacentOpenTailRequest>,
}

/// Execute the state-store prefix at `0x0094422F..0x0094428F`.
///
/// `BitMask<32>::operator=` copies its three scalar headers and then exactly `size` bytes
/// beginning at the inline buffer.  The command carries only four inline bytes, so this
/// planner fails closed when a malformed size would make retail read beyond byte 33.
pub fn plan_leader_options_prefix(
    command: &LeaderOptionsCommand,
    previous: LeaderOptionRowReceipt,
    console_play: i32,
) -> Result<LeaderOptionsPrefixPlan, LeaderOptionsPrefixError> {
    let who = command.data.who;
    if !(0..LEADER_OPTION_ROWS as i32).contains(&who) {
        return Err(LeaderOptionsPrefixError::UnsafeOwner { who });
    }
    if previous.who != who {
        return Err(LeaderOptionsPrefixError::PreviousRowMismatch);
    }
    if !(0..=4).contains(&command.data.flags.size) {
        return Err(LeaderOptionsPrefixError::UnsafeBitMaskSize {
            size: command.data.flags.size,
        });
    }

    let old_flag_byte = previous.state.flags.inline[0];
    let mut stored_flags = BitMask32State {
        bits: command.data.flags.bits,
        size: command.data.flags.size,
        flags: command.data.flags.flags,
        inline: previous.state.flags.inline,
    };
    let copied = command.data.flags.size as usize;
    stored_flags.inline[..copied].copy_from_slice(&command.data.flags.inline[..copied]);

    let stored = LeaderOptionDataState {
        who: command.data.who,
        peasants: command.data.peasants,
        peasants_wait: command.data.peasants_wait,
        buildings: command.data.buildings,
        flags: stored_flags,
    };
    let new_flag_byte = stored.flags.inline[0];
    let changes = LeaderOptionChangeSet {
        peasants_changed: previous.state.peasants != stored.peasants,
        buildings_changed: previous.state.buildings != stored.buildings,
        flag_bit_1_changed: (old_flag_byte ^ new_flag_byte) & 0x02 != 0,
        flag_bit_3_changed: (old_flag_byte ^ new_flag_byte) & 0x08 != 0,
        flag_bit_4_changed: (old_flag_byte ^ new_flag_byte) & 0x10 != 0,
        mirror_to_local_option: who == console_play,
    };
    let open_tail =
        changes
            .needs_open_tail()
            .then_some(AdjacentOpenTailRequest::LeaderOptionsCascade(
                LeaderOptionsCascadeRequest {
                    who,
                    previous: previous.state,
                    stored,
                    changes,
                },
            ));

    Ok(LeaderOptionsPrefixPlan {
        presentation: AdjacentPrefixPresentationReceipt::LeaderOptionsDiagnostic {
            data: command.data,
        },
        stored,
        open_tail,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsoleCommand {
    pub mouse_x: i32,
    pub mouse_y: i32,
    pub command: [u16; CONSOLE_COMMAND_UNITS],
}

pub fn decode_console_command(bytes: &[u8]) -> Result<ConsoleCommand, AdjacentCommandDecodeError> {
    require_fixed(bytes, CONSOLE_COMMAND_OPCODE, CONSOLE_COMMAND_WIRE_BYTES)?;
    let mut command = [0; CONSOLE_COMMAND_UNITS];
    for (unit, pair) in command.iter_mut().zip(bytes[9..521].chunks_exact(2)) {
        *unit = u16::from_le_bytes([pair[0], pair[1]]);
    }
    Ok(ConsoleCommand {
        mouse_x: i32_at(bytes, 1),
        mouse_y: i32_at(bytes, 5),
        command,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsoleCommandPrefixPlan {
    pub presentation: Vec<AdjacentPrefixPresentationReceipt>,
    pub open_tail: Option<AdjacentOpenTailRequest>,
}

/// Recover the exact `ConsoleWin*` null gate, coordinate stores, and parse-call arguments.
/// `ConsoleWin::parse_cmd` remains an open tail because console commands can mutate the
/// simulation; it is not relabelled as presentation merely because its receiver is a UI.
pub fn plan_console_command_prefix(
    command: &ConsoleCommand,
    console_present: bool,
) -> ConsoleCommandPrefixPlan {
    let mut presentation = vec![
        AdjacentPrefixPresentationReceipt::ConsoleCommandDiagnostic {
            mouse_x: command.mouse_x,
            mouse_y: command.mouse_y,
            command: command.command,
        },
    ];
    let open_tail = if console_present {
        presentation.push(AdjacentPrefixPresentationReceipt::ConsoleMouseStore {
            mouse_x: command.mouse_x,
            mouse_y: command.mouse_y,
        });
        Some(AdjacentOpenTailRequest::ParseConsoleCommand {
            mouse_x: command.mouse_x,
            mouse_y: command.mouse_y,
            command: command.command,
            first_mode: 1,
            second_mode: 1,
        })
    } else {
        None
    };
    ConsoleCommandPrefixPlan {
        presentation,
        open_tail,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrozenClosureStatus {
    PrefixExactTailOpen,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdjacentRowClosure {
    pub opcode: u8,
    pub wire_bytes: usize,
    pub status: FrozenClosureStatus,
    pub dispatcher_complete: bool,
    pub whole_row_complete: bool,
    pub integration_owner: &'static str,
}

/// Machine-readable refusal to count exact prefixes as completed dispatcher rows.
pub const ADJACENT_ROW_CLOSURE: [AdjacentRowClosure; 2] = [
    AdjacentRowClosure {
        opcode: LEADER_OPTIONS_OPCODE,
        wire_bytes: LEADER_OPTIONS_WIRE_BYTES,
        status: FrozenClosureStatus::PrefixExactTailOpen,
        dispatcher_complete: false,
        whole_row_complete: false,
        integration_owner: "LeaderOptions object cascades and local option mirror",
    },
    AdjacentRowClosure {
        opcode: CONSOLE_COMMAND_OPCODE,
        wire_bytes: CONSOLE_COMMAND_WIRE_BYTES,
        status: FrozenClosureStatus::PrefixExactTailOpen,
        dispatcher_complete: false,
        whole_row_complete: false,
        integration_owner: "ConsoleWin::parse_cmd",
    },
];
