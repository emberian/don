//! Rise of Nations lockstep command wire codec.
//!
//! # What this crate is, and what it is not
//!
//! This is a **codec for the bytes the engine already produces**: the
//! `CommandPackage` command list, the 18-byte record framing used inside a
//! `.rcx` recorded game, and the 8-byte `NetMsg_CommandPackageData` framing used
//! on the network. Every structure size and field offset comes from `rise.pdb`,
//! the PDB Big Huge Games ships in `sbl/` alongside the retail binary, whose
//! CodeView GUID matches `riseofnations.exe` exactly. See
//! `docs/tracks/headless-client.md`.
//!
//! It is **also** a working headless peer. [`session::Session`] over
//! [`transport::TcpTransport`] runs the roster, readiness and lockstep turn
//! protocol between our own processes over the internet — see the
//! `donnet-peer` binary and `tests/tcp_session.rs`.
//!
//! It still **cannot join a retail lobby**. That path is PlayFab Lobby for
//! discovery plus PlayFab Party for data channels, and it is gated on a Steam
//! auth ticket and the PlayFab title id, neither of which we hold. The exact
//! blocking list is in `docs/tracks/headless-net.md` §5.2. Note also that game
//! *setup* never crosses the wire as a `NetMsg` in this build — it is published
//! as lobby attributes, whose recovered key schema is in [`lobby`].
//!
//! # Fidelity
//!
//! Tier C throughout — behaviourally faithful with divergence measured. The
//! measured divergence on the shipped corpus is in `tests/roundtrip.rs`: every
//! packet re-encodes to the exact original bytes, or the test fails.

#![forbid(unsafe_code)]

pub mod evidence;
pub mod extension;
pub mod internal;
pub mod lobby;
pub mod local_match;
pub mod lockstep;
pub mod msg;
pub mod obfuscate;
pub mod opcodes;
pub mod retail;
pub mod session;
pub mod setup;
pub mod stream;
pub mod transport;

pub use evidence::{
    EpochMember, EvidenceError, PersistedLockstepTranscript, ReplayAction, ReplayedLockstep,
    LOCKSTEP_EVIDENCE_MAGIC, LOCKSTEP_EVIDENCE_VERSION, MAX_LOCKSTEP_ACTIONS,
    MAX_LOCKSTEP_EVIDENCE_BYTES, MAX_LOCKSTEP_OUTCOME_BYTES,
};
pub use extension::{
    game_keys_are_wire_equivalent, DonExtension, ExtensionError, GameKeySource, DON_EXT_BASE,
    DON_EXT_GAMEKEY, DON_EXT_MATCH_START, GAME_KEY_WIRE_MASK,
};
pub use internal::{InternalError, InternalPacket};
pub use local_match::{LocalMatch, LocalMatchError, LocalMatchPhase};
pub use lockstep::{
    ChecksumDifference, EpochCause, LockstepError, LockstepEvidence, LockstepRunner,
    LockstepStatus, PackageEvidence, SubmitOutcome, TurnEvidence, TurnTimeoutEvidence,
};
pub use msg::{Framed, MsgType, NetMsg};
pub use obfuscate::{Obfuscation, PadRandom};
pub use opcodes::{COMMAND_NAMES, COMMAND_SIZES, COMMAND_STRUCTS};
pub use retail::{decode_retail_checksum_package, DecodedRetailChecksum, RetailChecksumError};
pub use session::{
    AnnouncedGameKey, AnnouncedMatchStart, Event, MatchStartError, Player, Role, Session,
    SetupRefusal, TurnPackage,
};
pub use setup::{
    GameConnectionData, GameConnectionDataFull, PlayerConnectionData, PlayerSlotPod,
    ScenFilePreviewData,
};
pub use stream::{find_stream, StreamLocation};
pub use transport::{Datagram, Dest, LoopTransport, TcpTransport, Transport};

/// Number of opcodes in the engine's `CommandTypes` enum.
pub const NUM_COMMAND_TYPES: usize = 82;

/// `NetMsgType` — the first byte of every `GenericNetPacket`.
/// Values are the engine's own enum, recovered from `rise.pdb`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum NetMsgType {
    Generic = 0,
    PlayerConnectionData = 1,
    AllPlayerConnectionData = 2,
    GameConnectionData = 3,
    GameConnectionDataFull = 4,
    Chat = 5,
    Ping = 6,
    CommandPackageData = 7,
    Pause = 8,
    Taunt = 9,
    SyncSignal = 10,
    DropStamp = 11,
    TimeSync = 12,
    DropVote = 13,
    DropDecision = 14,
    GameModSyncRequest = 15,
    GameModSyncResponse = 16,
    SyncFileBegin = 17,
    SyncFileResponse = 18,
    SyncFileData = 19,
    SyncFileVerify = 20,
    SyncFileEnd = 21,
    SyncFileError = 22,
    SyncDirError = 23,
    SyncDirRequest = 24,
    SyncDirInfo = 25,
    GameSpyChallenge = 26,
    PlayerStatusRequest = 27,
    PlayerStatusResponse = 28,
    DropFlag = 29,
    Spline = 30,
    GameConnectionScenario = 31,
    GameConnectionModInfo = 32,
}

impl NetMsgType {
    /// Set on a reply. `NETMSG_RESPONSE_FLAG = 64` in the engine enum.
    pub const RESPONSE_FLAG: u8 = 64;
}

/// `InternalPacketType` — transport-level messages handled by
/// `CrossplayNetLibSys`, never by the game. From `CrossplayNetLib.pdb`.
/// All are `>= IPT_BASE`, which is why game message ids stay below 128.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum InternalPacketType {
    PlayerList = 128,
    DropRequest = 129,
    CancelDropRequest = 130,
    PulsePacket = 131,
    AddPlayer = 132,
    DestroyPlayer = 133,
    MigrateHost = 134,
    DsyncMsg = 135,
    ReadyFlag = 136,
}

/// The 16 `check_all` channels carried by `CheckSumsCommand` (opcode 0x39),
/// in the engine's serialisation order.
pub const CHECKSUM_CHANNELS: [&str; 16] = [
    "units",
    "builds",
    "walls",
    "ammo",
    "deaths",
    "groups",
    "guys",
    "leaders",
    "cities",
    "items",
    "goods",
    "world",
    "rules",
    "scenario_data",
    "script_run_time",
    "all",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Opcode outside 0x00..=0x51.
    UnknownOpcode(u8),
    /// The command claims more bytes than the buffer holds.
    Truncated {
        offset: usize,
        need: usize,
        have: usize,
    },
    /// A variable-length command declared an implausible element count.
    BadLength { offset: usize, len: i64 },
    /// The command list did not tile the payload exactly.
    Residual { consumed: usize, len: usize },
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::UnknownOpcode(o) => write!(f, "unknown opcode {o:#04x}"),
            Error::Truncated { offset, need, have } => {
                write!(f, "truncated at {offset}: need {need}, have {have}")
            }
            Error::BadLength { offset, len } => write!(f, "bad length {len} at {offset}"),
            Error::Residual { consumed, len } => {
                write!(f, "consumed {consumed} of {len} payload bytes")
            }
        }
    }
}

impl std::error::Error for Error {}

// ---------------------------------------------------------------------------
// Command
// ---------------------------------------------------------------------------

/// One command as it sits on the wire: an opcode byte plus its body.
///
/// Deliberately untyped. The engine's own dispatcher is a switch that returns a
/// byte length, so length is the only universally meaningful property; typed
/// views are offered for the commands whose layouts we actually use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command<'a> {
    pub opcode: u8,
    /// Opcode byte included: `bytes[0] == opcode`.
    pub bytes: &'a [u8],
}

/// Capacity of retail `CommandPackage::data`.
///
/// This is the payload ceiling shared by the replay record and
/// `NetMsg_CommandPackageData`; it excludes either framing header.
pub const MAX_COMMAND_PACKAGE_PAYLOAD: usize = 512;

impl<'a> Command<'a> {
    pub fn name(&self) -> &'static str {
        COMMAND_NAMES
            .get(self.opcode as usize)
            .copied()
            .unwrap_or("<unknown>")
    }

    pub fn struct_name(&self) -> &'static str {
        COMMAND_STRUCTS
            .get(self.opcode as usize)
            .copied()
            .unwrap_or("<unknown>")
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Byte length of the command starting at `buf[0]`, mirroring the value the
    /// engine's handler returns.
    ///
    /// The three variable-length cases are the only opcodes whose handler
    /// computes a size from the body. Each formula is read directly off the
    /// `lea` that produces the handler's return value — **not** inferred from
    /// `sizeof`, which counts only one element of the trailing array and is
    /// off by a different amount in each case:
    ///
    /// | op | handler | instruction | formula | `sizeof` |
    /// |----|---------|-------------|---------|----------|
    /// | 0x00 | `process_group`  @ `0x0094a6f3` | `lea eax,[eax*2+3]`    | `3 + 2*num`  | 5  |
    /// | 0x33 | `process_spline` @ `0x00945396` | `lea esi,[eax*8+6]`    | `6 + 8*len`  | 14 |
    /// | 0x44 | `process_chat`   @ `0x009458b4` | `lea esi,[eax*2+0x13]` | `19 + 2*len` | 19 |
    ///
    /// `process_chat` is the one that bites: the string is NUL-terminated, so
    /// `len + 1` wide characters are on the wire. Using `17 + 2*len` decodes
    /// 2,423 of 2,425 packages in one recording and then desynchronises —
    /// which is exactly how the corpus test caught it.
    pub fn wire_len(buf: &[u8]) -> Result<usize, Error> {
        let need = |n: usize| -> Result<(), Error> {
            if buf.len() < n {
                Err(Error::Truncated {
                    offset: 0,
                    need: n,
                    have: buf.len(),
                })
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
                let n = u16::from_le_bytes([buf[4], buf[5]]) as usize;
                Ok(6 + 8 * n)
            }
            0x44 => {
                need(17)?;
                let n = i32::from_le_bytes([buf[13], buf[14], buf[15], buf[16]]);
                // The send buffer upstream is 512 bytes; anything outside that
                // cannot be a real chat message.
                if !(0..=512).contains(&n) {
                    return Err(Error::BadLength {
                        offset: 0,
                        len: n as i64,
                    });
                }
                Ok(19 + 2 * n as usize)
            }
            _ => match COMMAND_SIZES.get(op as usize).copied().flatten() {
                Some(n) => Ok(n as usize),
                None => Err(Error::UnknownOpcode(op)),
            },
        }
    }
}

/// Iterate a `CommandPackage` payload, honouring inter-command padding.
///
/// `pad` yields the number of bytes to skip *after* each command. In a solo
/// game the engine takes the `iVar11 = 0` branch, so pass `Obfuscation::none()`.
pub fn decode_commands<'a>(
    payload: &'a [u8],
    obf: &mut Obfuscation,
) -> Result<Vec<Command<'a>>, Error> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < payload.len() {
        let l = Command::wire_len(&payload[i..]).map_err(|e| match e {
            Error::Truncated { need, have, .. } => Error::Truncated {
                offset: i,
                need,
                have,
            },
            Error::BadLength { len, .. } => Error::BadLength { offset: i, len },
            other => other,
        })?;
        if i + l > payload.len() {
            return Err(Error::Truncated {
                offset: i,
                need: l,
                have: payload.len() - i,
            });
        }
        out.push(Command {
            opcode: payload[i],
            bytes: &payload[i..i + l],
        });
        i += l + obf.next_pad();
    }
    if i != payload.len() {
        return Err(Error::Residual {
            consumed: i,
            len: payload.len(),
        });
    }
    Ok(out)
}

/// Re-emit a command list. Round-trips `decode_commands` exactly when the same
/// obfuscation state is used, because padding bytes are reproduced from the
/// same generator.
pub fn encode_commands(cmds: &[Command<'_>], obf: &mut Obfuscation, out: &mut Vec<u8>) {
    for c in cmds {
        out.extend_from_slice(c.bytes);
        for _ in 0..obf.next_pad() {
            out.push(0);
        }
    }
}

// ---------------------------------------------------------------------------
// CommandPackage — the replay record framing
// ---------------------------------------------------------------------------

/// Header of a `CommandPackage` as serialised into a `.rcx`.
///
/// Field names are the engine's own, from `struct CommandPackage` in `rise.pdb`:
/// ```text
/// +0x00 unsigned long stamp     simulation frame
/// +0x04 int           play      player slot that issued the package
/// +0x08 int           valid
/// +0x0c int           group     scratch at runtime; a monotone serial in the file
/// +0x10 short         size      payload length
/// +0x12 unsigned char data[512]
/// ```
/// Only the first 18 bytes plus `size` payload bytes are written; the 512-byte
/// `data` array and the trailing `Random padding` member are in-memory storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackageHeader {
    pub stamp: u32,
    pub play: i32,
    pub valid: i32,
    pub group: i32,
    pub size: u16,
}

impl PackageHeader {
    pub const WIRE_LEN: usize = 18;

    pub fn decode(buf: &[u8]) -> Option<Self> {
        if buf.len() < Self::WIRE_LEN {
            return None;
        }
        let g = |o: usize| u32::from_le_bytes(buf[o..o + 4].try_into().unwrap());
        Some(PackageHeader {
            stamp: g(0),
            play: g(4) as i32,
            valid: g(8) as i32,
            group: g(12) as i32,
            size: u16::from_le_bytes([buf[16], buf[17]]),
        })
    }

    pub fn encode(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.stamp.to_le_bytes());
        out.extend_from_slice(&self.play.to_le_bytes());
        out.extend_from_slice(&self.valid.to_le_bytes());
        out.extend_from_slice(&self.group.to_le_bytes());
        out.extend_from_slice(&self.size.to_le_bytes());
    }
}

/// One framed record from a `.rcx` command stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageRecord<'a> {
    pub header: PackageHeader,
    /// Still obfuscated in a multiplayer file.
    pub payload: &'a [u8],
}

impl<'a> PackageRecord<'a> {
    pub fn decode(buf: &'a [u8]) -> Option<(Self, usize)> {
        let header = PackageHeader::decode(buf)?;
        let n = PackageHeader::WIRE_LEN + header.size as usize;
        if buf.len() < n {
            return None;
        }
        Some((
            PackageRecord {
                header,
                payload: &buf[PackageHeader::WIRE_LEN..n],
            },
            n,
        ))
    }

    pub fn encode(&self, out: &mut Vec<u8>) {
        self.header.encode(out);
        out.extend_from_slice(self.payload);
    }
}

/// Iterator over a `.rcx` command stream starting at `start`.
pub struct PackageStream<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> PackageStream<'a> {
    pub fn new(buf: &'a [u8], start: usize) -> Self {
        PackageStream { buf, pos: start }
    }
    pub fn pos(&self) -> usize {
        self.pos
    }
}

impl<'a> Iterator for PackageStream<'a> {
    type Item = PackageRecord<'a>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.pos >= self.buf.len() {
            return None;
        }
        let (rec, n) = PackageRecord::decode(&self.buf[self.pos..])?;
        self.pos += n;
        Some(rec)
    }
}

// ---------------------------------------------------------------------------
// NetMsg_CommandPackageData — the *network* framing (different from the file)
// ---------------------------------------------------------------------------

/// `NetMsg_CommandPackageData` from `rise.pdb`:
/// ```text
/// +0x00 unsigned char type       = NETMSG_COMMANDPACKAGEDATA (7)
/// +0x01 unsigned long stamp
/// +0x05 char          play
/// +0x06 short         data_size
/// +0x08 unsigned char data[1]    -- data_size bytes follow
/// ```
/// Note this is **8 bytes of header, not 18**: `valid` and `group` are not sent.
/// The receiver is `CommandManager::process_command_package_data`
/// (`?process_command_package_data@CommandManager@@QAEXPAUNetMsg_CommandPackageData@@K@Z`,
/// VA 0x0093fcd0).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetCommandPackage<'a> {
    pub stamp: u32,
    pub play: i8,
    pub payload: &'a [u8],
}

impl<'a> NetCommandPackage<'a> {
    pub const HEADER_LEN: usize = 8;

    pub fn decode(buf: &'a [u8]) -> Option<Self> {
        if buf.len() < Self::HEADER_LEN || buf[0] != NetMsgType::CommandPackageData as u8 {
            return None;
        }
        let stamp = u32::from_le_bytes(buf[1..5].try_into().unwrap());
        let play = buf[5] as i8;
        let size = i16::from_le_bytes([buf[6], buf[7]]);
        if size < 0 {
            return None;
        }
        let end = Self::HEADER_LEN + size as usize;
        if buf.len() < end {
            return None;
        }
        Some(NetCommandPackage {
            stamp,
            play,
            payload: &buf[Self::HEADER_LEN..end],
        })
    }

    pub fn encode(&self, out: &mut Vec<u8>) {
        out.push(NetMsgType::CommandPackageData as u8);
        out.extend_from_slice(&self.stamp.to_le_bytes());
        out.push(self.play as u8);
        out.extend_from_slice(&(self.payload.len() as i16).to_le_bytes());
        out.extend_from_slice(self.payload);
    }

    /// Build the network message the engine would send for a recorded package.
    /// `valid` and `group` are dropped: they are not on the wire.
    pub fn from_record(rec: &PackageRecord<'a>) -> Self {
        NetCommandPackage {
            stamp: rec.header.stamp,
            play: rec.header.play as i8,
            payload: rec.payload,
        }
    }
}

/// Decoded `CheckSumsCommand` (opcode 0x39) — the per-turn lockstep oracle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckSums(pub [u32; 16]);

impl CheckSums {
    pub const WIRE_LEN: usize = 65;

    pub fn decode(cmd: &Command<'_>) -> Option<Self> {
        if cmd.opcode != 0x39 || cmd.bytes.len() != Self::WIRE_LEN {
            return None;
        }
        let mut v = [0u32; 16];
        for (i, s) in v.iter_mut().enumerate() {
            let o = 1 + 4 * i;
            *s = u32::from_le_bytes(cmd.bytes[o..o + 4].try_into().unwrap());
        }
        Some(CheckSums(v))
    }

    pub fn channel(&self, name: &str) -> Option<u32> {
        CHECKSUM_CHANNELS
            .iter()
            .position(|c| *c == name)
            .map(|i| self.0[i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opcode_table_is_complete_and_matches_the_dispatcher_range() {
        assert_eq!(COMMAND_NAMES.len(), NUM_COMMAND_TYPES);
        assert_eq!(COMMAND_SIZES.len(), NUM_COMMAND_TYPES);
        assert_eq!(COMMAND_STRUCTS.len(), NUM_COMMAND_TYPES);
        // exactly three variable-length opcodes
        let var: Vec<usize> = COMMAND_SIZES
            .iter()
            .enumerate()
            .filter(|(_, s)| s.is_none())
            .map(|(i, _)| i)
            .collect();
        assert_eq!(var, vec![0x00, 0x33, 0x44]);
    }

    #[test]
    fn checksums_command_is_sixteen_channels_plus_opcode() {
        assert_eq!(CHECKSUM_CHANNELS.len(), 16);
        assert_eq!(COMMAND_SIZES[0x39], Some(65));
        assert_eq!(1 + 4 * CHECKSUM_CHANNELS.len(), CheckSums::WIRE_LEN);
    }

    #[test]
    fn net_framing_is_ten_bytes_shorter_than_file_framing() {
        assert_eq!(PackageHeader::WIRE_LEN - NetCommandPackage::HEADER_LEN, 10);
    }

    #[test]
    fn variable_length_formulas() {
        // GroupCommand: 3 + 2*num
        assert_eq!(
            Command::wire_len(&[0x00, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0]).unwrap(),
            11
        );
        assert_eq!(Command::wire_len(&[0x00, 0, 0]).unwrap(), 3);
        // SplineCommand: 6 + 8*len
        let mut s = vec![0x33, 1, 2, 3, 2, 0];
        s.extend(std::iter::repeat(0).take(16));
        assert_eq!(Command::wire_len(&s).unwrap(), 22);
        // ChatCommand: 19 + 2*len (the wide string is NUL-terminated)
        let mut c = vec![0x44];
        c.extend_from_slice(&0i32.to_le_bytes());
        c.extend_from_slice(&0i32.to_le_bytes());
        c.extend_from_slice(&0i32.to_le_bytes());
        c.extend_from_slice(&3i32.to_le_bytes());
        c.extend(std::iter::repeat(0).take(8));
        assert_eq!(Command::wire_len(&c).unwrap(), 25);
        // and the base case must equal sizeof(ChatCommand) + 2
        let mut c0 = vec![0x44];
        c0.extend(std::iter::repeat(0).take(16));
        assert_eq!(Command::wire_len(&c0).unwrap(), 19);
    }
}
