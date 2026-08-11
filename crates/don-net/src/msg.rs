//! The game's own message layer: `GenericNetPacket` and every `NetMsg_*`.
//!
//! # Ground truth
//!
//! Every struct size and field offset in this module was read out of
//! `schema/types.json`, which is the TPI stream of `ron-bin/sbl/rise.pdb` — the
//! private PDB whose CodeView GUID matches the shipped `riseofnations.exe`.
//! Dispatch behaviour was read out of the jump table of
//! `NetDaemon::process(int)` at `0x00950f30`. **[measured]**
//!
//! ## Two facts you cannot get from the enum alone
//!
//! 1. **The response flag is masked off before dispatch.** The first instruction
//!    of the dispatcher's switch does `bVar4 = *packet & 0xBF` — that is
//!    `~NETMSG_RESPONSE_FLAG`, i.e. `~64`. So the type byte is a 7-bit id in
//!    bits 0..5 plus bit 6 as a reply marker; the same handler serves both.
//!    `MsgType::from_wire` reproduces this exactly.
//!
//! 2. **Seven of the 31 message ids are dead in this build.** The dispatcher's
//!    jump table at `0x00951280` has 31 entries (`cmp ecx, 0x1e; ja default;
//!    jmp [ecx*4 + 0x951280]`), and seven of them point at the *default* arm
//!    `0x00951201`, which logs an error. Those seven are
//!    `PLAYERCONNECTIONDATA`(1), `ALLPLAYERCONNECTIONDATA`(2),
//!    `GAMECONNECTIONDATA`(3), `GAMECONNECTIONDATAFULL`(4), `PAUSE`(8),
//!    `PLAYER_STATUS_REQUEST`(27) and `PLAYER_STATUS_RESPONSE`(28).
//!
//!    This is a **correction** to `docs/tracks/netcode-symbols.md` §5 Option B
//!    step 5, which planned to negotiate game setup over
//!    `NETMSG_GAMECONNECTIONDATAFULL`(4). That message is not received by this
//!    build at all. Setup and readiness travel as **PlayFab lobby attributes**
//!    instead — see [`crate::lobby`], where the real key schema is recovered.
//!    The structs are still live in memory (`ConnectionData::game_data` is a
//!    `GameConnectionDataFull`), so [`crate::setup`] still encodes them; they
//!    just are not what crosses the wire.

use crate::NetMsgType;

/// Decoded type byte: the 6-bit message id plus the reply marker.
///
/// `NetDaemon::process` computes `type & 0xBF` and switches on that, so the two
/// carry independent meaning and both must survive a decode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MsgType {
    pub id: u8,
    /// `NETMSG_RESPONSE_FLAG` (64) was set on the wire byte.
    pub response: bool,
}

impl MsgType {
    pub const RESPONSE_FLAG: u8 = 64;

    pub fn from_wire(b: u8) -> Self {
        MsgType {
            id: b & !Self::RESPONSE_FLAG,
            response: b & Self::RESPONSE_FLAG != 0,
        }
    }

    pub fn to_wire(self) -> u8 {
        self.id
            | if self.response {
                Self::RESPONSE_FLAG
            } else {
                0
            }
    }

    pub fn is_internal(b: u8) -> bool {
        b >= crate::internal::IPT_BASE
    }
}

/// What `NetDaemon::process` does with a message id, read off the jump table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dispatch {
    /// Has a real handler.
    Handled,
    /// Jump-table entry exists and is a no-op tail (`0x00951271`): the message
    /// is consumed and discarded. Ids 9 (`TAUNT`) and 11 (`DROPSTAMP`).
    Ignored,
    /// Jump-table entry points at the default arm `0x00951201`, which logs
    /// "unexpected message" — the id is dead in this build.
    Dead,
}

/// `NetDaemon::process` jump table at `0x00951280`, 31 entries, verbatim.
/// Index is the masked type byte. **[measured]**
pub const DISPATCH_TARGET: [u32; 31] = [
    0x0095_0fc8,
    0x0095_1201,
    0x0095_1201,
    0x0095_1201,
    0x0095_1201,
    0x0095_116a,
    0x0095_1144,
    0x0095_10e5,
    0x0095_1201,
    0x0095_1271,
    0x0095_11b8,
    0x0095_1271,
    0x0095_11d9,
    0x0095_111e,
    0x0095_1131,
    0x0095_0fdc,
    0x0095_0fef,
    0x0095_1007,
    0x0095_101d,
    0x0095_1033,
    0x0095_1049,
    0x0095_105b,
    0x0095_106f,
    0x0095_10ae,
    0x0095_1086,
    0x0095_109a,
    0x0095_11a4,
    0x0095_1201,
    0x0095_1201,
    0x0095_11ed,
    0x0095_117e,
];

const DEFAULT_ARM: u32 = 0x0095_1201;
const NOOP_ARM: u32 = 0x0095_1271;

/// Classify a message id the way the shipped dispatcher does.
pub fn dispatch(id: u8) -> Dispatch {
    match DISPATCH_TARGET.get(id as usize) {
        None => Dispatch::Dead, // `ja default` for anything above 0x1e
        Some(&DEFAULT_ARM) => Dispatch::Dead,
        Some(&NOOP_ARM) => Dispatch::Ignored,
        Some(_) => Dispatch::Handled,
    }
}

/// `sizeof` for each `NetMsg_*` struct, indexed by message id.
///
/// `None` marks a variable-length message (`COMMANDPACKAGEDATA` carries
/// `data_size` bytes; `SPLINE` carries `len` 8-byte vertices) or an id with no
/// struct in the PDB at all. Values are the compiler's own `sizeof`, so for the
/// two flexible-array messages they count exactly one trailing element.
/// **[measured, `schema/types.json`]**
pub const MSG_SIZE: [Option<u16>; 31] = [
    Some(513), // 0  GENERIC              NetMsg_Generic
    None,      // 1  PLAYERCONNECTIONDATA        (dead; no NetMsg_ struct)
    None,      // 2  ALLPLAYERCONNECTIONDATA     (dead; no NetMsg_ struct)
    None,      // 3  GAMECONNECTIONDATA          (dead; no NetMsg_ struct)
    None,      // 4  GAMECONNECTIONDATAFULL      (dead; no NetMsg_ struct)
    Some(514), // 5  CHAT                 NetMsg_Chat
    Some(9),   // 6  PING                 NetMsg_Ping
    None,      // 7  COMMANDPACKAGEDATA   NetMsg_CommandPackageData (9 + payload)
    Some(10),  // 8  PAUSE                NetMsg_Pause  (struct exists, id dead)
    Some(2),   // 9  TAUNT                NetMsg_Taunt
    Some(5),   // 10 SYNCSIGNAL           NetMsg_SyncSignal
    Some(13),  // 11 DROPSTAMP            NetMsg_DropStamp
    Some(9),   // 12 TIMESYNC             NetMsg_TimeSync
    Some(3),   // 13 DROPVOTE             NetMsg_DropVote
    Some(2),   // 14 DROPDECISION         NetMsg_DropDecision
    Some(2),   // 15 GAMEMODSYNCREQUEST   NetMsg_GameModSyncRequest
    Some(2),   // 16 GAMEMODSYNCRESPONSE  NetMsg_GameModSyncResponse
    Some(525), // 17 SYNCFILEBEGIN        NetMsg_SyncFileBegin
    Some(2),   // 18 SYNCFILERESPONSE     NetMsg_SyncFileResponse
    Some(259), // 19 SYNCFILEDATA         NetMsg_SyncFileData
    Some(1),   // 20 SYNCFILEVERIFY       NetMsg_SyncFileVerify
    Some(521), // 21 SYNCFILEEND          NetMsg_SyncFileEnd
    Some(1),   // 22 SYNCFILEERROR        NetMsg_SyncFileError
    Some(1),   // 23 SYNCDIRERROR         NetMsg_SyncDirError
    Some(5),   // 24 SYNCDIRREQUEST       NetMsg_SyncDirRequest
    Some(529), // 25 SYNCDIRINFO          NetMsg_SyncDirInfo
    Some(69),  // 26 GAMESPYCHALLENGE     NetMsg_GameSpyChallenge
    Some(1),   // 27 PLAYER_STATUS_REQUEST  NetMsg_PlayerStatusRequest
    Some(65),  // 28 PLAYER_STATUS_RESPONSE NetMsg_PlayerStatusResponse
    Some(5),   // 29 DROP_FLAG            NetMsg_DropFlag
    None,      // 30 SPLINE               NetMsg_Spline (6 + 8*len)
];

// ---------------------------------------------------------------------------
// Little-endian primitives. No dependencies, by the crate's own rule.
// ---------------------------------------------------------------------------

#[inline]
fn u16le(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
#[inline]
fn u32le(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
#[inline]
fn i32le(b: &[u8], o: usize) -> i32 {
    u32le(b, o) as i32
}

/// Read a fixed-width UTF-16LE array, stopping at the first NUL.
pub fn read_wide(b: &[u8], off: usize, units: usize) -> String {
    let mut s = String::new();
    for i in 0..units {
        let o = off + i * 2;
        if o + 1 >= b.len() {
            break;
        }
        let c = u16le(b, o);
        if c == 0 {
            break;
        }
        s.push(char::from_u32(c as u32).unwrap_or('\u{fffd}'));
    }
    s
}

/// Write a UTF-16LE string into a fixed-width array, NUL-padded, truncating at
/// `units - 1` so the terminator always survives — which is what the engine's
/// own `wcsncpy`-shaped copies do.
pub fn write_wide(out: &mut Vec<u8>, s: &str, units: usize) {
    let mut n = 0usize;
    for u in s.encode_utf16() {
        if n + 1 >= units {
            break;
        }
        out.extend_from_slice(&u.to_le_bytes());
        n += 1;
    }
    for _ in n..units {
        out.extend_from_slice(&[0, 0]);
    }
}

/// Read a fixed-width NUL-terminated `char` array.
pub fn read_narrow(b: &[u8], off: usize, len: usize) -> String {
    let end = (off + len).min(b.len());
    let slice = &b[off.min(b.len())..end];
    let n = slice.iter().position(|&c| c == 0).unwrap_or(slice.len());
    String::from_utf8_lossy(&slice[..n]).into_owned()
}

/// Write a fixed-width NUL-terminated `char` array.
pub fn write_narrow(out: &mut Vec<u8>, s: &str, len: usize) {
    let b = s.as_bytes();
    let n = b.len().min(len.saturating_sub(1));
    out.extend_from_slice(&b[..n]);
    for _ in n..len {
        out.push(0);
    }
}

// ---------------------------------------------------------------------------
// NetMsg
// ---------------------------------------------------------------------------

/// One decoded game message.
///
/// Variable-length payloads borrow from the input buffer; everything else is
/// owned so a session can queue messages without pinning the receive buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetMsg<'a> {
    Generic {
        buffer: [u8; 512],
    },
    Chat {
        observer_to_all: u8,
        message: String,
    },
    Ping {
        cx: i32,
        cy: i32,
    },
    CommandPackage {
        stamp: u32,
        play: i8,
        payload: &'a [u8],
    },
    Pause {
        play: i32,
        pause_time: u32,
        requested_state: u8,
    },
    Taunt {
        taunt: u8,
    },
    SyncSignal {
        play: i32,
    },
    DropStamp {
        play_from: i32,
        stamp: i32,
        play: i32,
    },
    TimeSync {
        time_stamp_sent: u32,
        time_stamp_received: u32,
    },
    DropVote {
        play: u8,
        vote: i8,
    },
    DropDecision {
        vote: i8,
    },
    DropFlag {
        pid: i32,
    },
    GameModSyncRequest {
        desired: u8,
    },
    GameModSyncResponse {
        required: u8,
    },
    PlayerStatusRequest,
    PlayerStatusResponse {
        player_id: [u32; 8],
        time_since_last_pulse: [u32; 8],
    },
    /// Any id we do not model as a typed variant, kept verbatim so a session can
    /// still relay it (the file-sync family, `SPLINE`, `GAMESPYCHALLENGE`).
    Raw {
        id: u8,
        body: &'a [u8],
    },
}

/// A message plus its reply marker, as it sits on the wire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Framed<'a> {
    pub ty: MsgType,
    pub msg: NetMsg<'a>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MsgError {
    Empty,
    /// Buffer shorter than the message's fixed `sizeof`.
    Short {
        id: u8,
        need: usize,
        have: usize,
    },
    /// `data_size` was negative.
    NegativeSize(i16),
    /// Type byte was >= 128, i.e. an `InternalPacketType`, not a game message.
    IsInternal(u8),
}

impl core::fmt::Display for MsgError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            MsgError::Empty => write!(f, "empty packet"),
            MsgError::Short { id, need, have } => {
                write!(f, "message {id}: need {need} bytes, have {have}")
            }
            MsgError::NegativeSize(n) => write!(f, "negative data_size {n}"),
            MsgError::IsInternal(b) => write!(f, "type {b} is an InternalPacketType"),
        }
    }
}

impl std::error::Error for MsgError {}

impl<'a> NetMsg<'a> {
    /// The message id this variant encodes as.
    pub fn id(&self) -> u8 {
        use NetMsgType as T;
        match self {
            NetMsg::Generic { .. } => T::Generic as u8,
            NetMsg::Chat { .. } => T::Chat as u8,
            NetMsg::Ping { .. } => T::Ping as u8,
            NetMsg::CommandPackage { .. } => T::CommandPackageData as u8,
            NetMsg::Pause { .. } => T::Pause as u8,
            NetMsg::Taunt { .. } => T::Taunt as u8,
            NetMsg::SyncSignal { .. } => T::SyncSignal as u8,
            NetMsg::DropStamp { .. } => T::DropStamp as u8,
            NetMsg::TimeSync { .. } => T::TimeSync as u8,
            NetMsg::DropVote { .. } => T::DropVote as u8,
            NetMsg::DropDecision { .. } => T::DropDecision as u8,
            NetMsg::DropFlag { .. } => T::DropFlag as u8,
            NetMsg::GameModSyncRequest { .. } => T::GameModSyncRequest as u8,
            NetMsg::GameModSyncResponse { .. } => T::GameModSyncResponse as u8,
            NetMsg::PlayerStatusRequest => T::PlayerStatusRequest as u8,
            NetMsg::PlayerStatusResponse { .. } => T::PlayerStatusResponse as u8,
            NetMsg::Raw { id, .. } => *id,
        }
    }

    /// Decode one packet. `buf[0]` is the raw type byte, response flag included.
    pub fn decode(buf: &'a [u8]) -> Result<Framed<'a>, MsgError> {
        if buf.is_empty() {
            return Err(MsgError::Empty);
        }
        if MsgType::is_internal(buf[0]) {
            return Err(MsgError::IsInternal(buf[0]));
        }
        let ty = MsgType::from_wire(buf[0]);
        let need = |n: usize| -> Result<(), MsgError> {
            if buf.len() < n {
                Err(MsgError::Short {
                    id: ty.id,
                    need: n,
                    have: buf.len(),
                })
            } else {
                Ok(())
            }
        };
        use NetMsgType as T;
        let msg = match ty.id {
            x if x == T::Generic as u8 => {
                need(513)?;
                let mut b = [0u8; 512];
                b.copy_from_slice(&buf[1..513]);
                NetMsg::Generic { buffer: b }
            }
            x if x == T::Chat as u8 => {
                need(514)?;
                NetMsg::Chat {
                    observer_to_all: buf[1],
                    message: read_wide(buf, 2, 256),
                }
            }
            x if x == T::Ping as u8 => {
                need(9)?;
                NetMsg::Ping {
                    cx: i32le(buf, 1),
                    cy: i32le(buf, 5),
                }
            }
            x if x == T::CommandPackageData as u8 => {
                // `sizeof(NetMsg_CommandPackageData)` is 9 only because `data`
                // is declared `unsigned char[1]`; the wire record is `8 +
                // data_size`. `CommandPackage::send` `0x0094c1e0` sends exactly
                // `*(short *)(this + 0x10) + 8` bytes (`0094c3fd movsx edx,
                // word ptr [ecx+0x10]` … `0094c407 add edx, 8`) and guards its
                // scramble loop with `if (size != 0)`, so a package with no
                // commands is an 8-byte header alone. Requiring 9 dropped that
                // record silently.
                need(8)?;
                let size = u16le(buf, 6) as i16;
                if size < 0 {
                    return Err(MsgError::NegativeSize(size));
                }
                let end = 8 + size as usize;
                need(end)?;
                NetMsg::CommandPackage {
                    stamp: u32le(buf, 1),
                    play: buf[5] as i8,
                    payload: &buf[8..end],
                }
            }
            x if x == T::Pause as u8 => {
                need(10)?;
                NetMsg::Pause {
                    play: i32le(buf, 1),
                    pause_time: u32le(buf, 5),
                    requested_state: buf[9],
                }
            }
            x if x == T::Taunt as u8 => {
                need(2)?;
                NetMsg::Taunt { taunt: buf[1] }
            }
            x if x == T::SyncSignal as u8 => {
                need(5)?;
                NetMsg::SyncSignal {
                    play: i32le(buf, 1),
                }
            }
            x if x == T::DropStamp as u8 => {
                need(13)?;
                NetMsg::DropStamp {
                    play_from: i32le(buf, 1),
                    stamp: i32le(buf, 5),
                    play: i32le(buf, 9),
                }
            }
            x if x == T::TimeSync as u8 => {
                need(9)?;
                NetMsg::TimeSync {
                    time_stamp_sent: u32le(buf, 1),
                    time_stamp_received: u32le(buf, 5),
                }
            }
            x if x == T::DropVote as u8 => {
                need(3)?;
                NetMsg::DropVote {
                    play: buf[1],
                    vote: buf[2] as i8,
                }
            }
            x if x == T::DropDecision as u8 => {
                need(2)?;
                NetMsg::DropDecision { vote: buf[1] as i8 }
            }
            x if x == T::DropFlag as u8 => {
                need(5)?;
                NetMsg::DropFlag { pid: i32le(buf, 1) }
            }
            x if x == T::GameModSyncRequest as u8 => {
                need(2)?;
                NetMsg::GameModSyncRequest { desired: buf[1] }
            }
            x if x == T::GameModSyncResponse as u8 => {
                need(2)?;
                NetMsg::GameModSyncResponse { required: buf[1] }
            }
            x if x == T::PlayerStatusRequest as u8 => NetMsg::PlayerStatusRequest,
            x if x == T::PlayerStatusResponse as u8 => {
                need(65)?;
                let mut a = [0u32; 8];
                let mut b = [0u32; 8];
                for i in 0..8 {
                    a[i] = u32le(buf, 1 + i * 4);
                    b[i] = u32le(buf, 33 + i * 4);
                }
                NetMsg::PlayerStatusResponse {
                    player_id: a,
                    time_since_last_pulse: b,
                }
            }
            other => NetMsg::Raw {
                id: other,
                body: &buf[1..],
            },
        };
        Ok(Framed { ty, msg })
    }

    /// Encode with the response flag clear.
    pub fn encode(&self, out: &mut Vec<u8>) {
        self.encode_framed(false, out)
    }

    /// Encode, optionally setting `NETMSG_RESPONSE_FLAG`.
    pub fn encode_framed(&self, response: bool, out: &mut Vec<u8>) {
        out.push(
            MsgType {
                id: self.id(),
                response,
            }
            .to_wire(),
        );
        match self {
            NetMsg::Generic { buffer } => out.extend_from_slice(buffer),
            NetMsg::Chat {
                observer_to_all,
                message,
            } => {
                out.push(*observer_to_all);
                write_wide(out, message, 256);
            }
            NetMsg::Ping { cx, cy } => {
                out.extend_from_slice(&cx.to_le_bytes());
                out.extend_from_slice(&cy.to_le_bytes());
            }
            NetMsg::CommandPackage {
                stamp,
                play,
                payload,
            } => {
                out.extend_from_slice(&stamp.to_le_bytes());
                out.push(*play as u8);
                out.extend_from_slice(&(payload.len() as i16).to_le_bytes());
                out.extend_from_slice(payload);
            }
            NetMsg::Pause {
                play,
                pause_time,
                requested_state,
            } => {
                out.extend_from_slice(&play.to_le_bytes());
                out.extend_from_slice(&pause_time.to_le_bytes());
                out.push(*requested_state);
            }
            NetMsg::Taunt { taunt } => out.push(*taunt),
            NetMsg::SyncSignal { play } => out.extend_from_slice(&play.to_le_bytes()),
            NetMsg::DropStamp {
                play_from,
                stamp,
                play,
            } => {
                out.extend_from_slice(&play_from.to_le_bytes());
                out.extend_from_slice(&stamp.to_le_bytes());
                out.extend_from_slice(&play.to_le_bytes());
            }
            NetMsg::TimeSync {
                time_stamp_sent,
                time_stamp_received,
            } => {
                out.extend_from_slice(&time_stamp_sent.to_le_bytes());
                out.extend_from_slice(&time_stamp_received.to_le_bytes());
            }
            NetMsg::DropVote { play, vote } => {
                out.push(*play);
                out.push(*vote as u8);
            }
            NetMsg::DropDecision { vote } => out.push(*vote as u8),
            NetMsg::DropFlag { pid } => out.extend_from_slice(&pid.to_le_bytes()),
            NetMsg::GameModSyncRequest { desired } => out.push(*desired),
            NetMsg::GameModSyncResponse { required } => out.push(*required),
            NetMsg::PlayerStatusRequest => {}
            NetMsg::PlayerStatusResponse {
                player_id,
                time_since_last_pulse,
            } => {
                for v in player_id {
                    out.extend_from_slice(&v.to_le_bytes());
                }
                for v in time_since_last_pulse {
                    out.extend_from_slice(&v.to_le_bytes());
                }
            }
            NetMsg::Raw { body, .. } => out.extend_from_slice(body),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_flag_round_trips_and_matches_the_dispatcher_mask() {
        // NetDaemon::process does `type & 0xBF`; 0xBF == !64.
        assert_eq!(!MsgType::RESPONSE_FLAG, 0xBF);
        for id in 0u8..=63 {
            for response in [false, true] {
                let t = MsgType { id, response };
                assert_eq!(MsgType::from_wire(t.to_wire()), t);
                assert_eq!(t.to_wire() & 0xBF, id);
            }
        }
    }

    #[test]
    fn dead_ids_are_exactly_the_seven_default_arms() {
        let dead: Vec<u8> = (0..31u8)
            .filter(|&i| dispatch(i) == Dispatch::Dead)
            .collect();
        assert_eq!(dead, vec![1, 2, 3, 4, 8, 27, 28]);
        let ignored: Vec<u8> = (0..31u8)
            .filter(|&i| dispatch(i) == Dispatch::Ignored)
            .collect();
        assert_eq!(ignored, vec![9, 11]);
        // Anything past the `cmp ecx, 0x1e` bound is unreachable.
        assert_eq!(dispatch(31), Dispatch::Dead);
        assert_eq!(dispatch(127), Dispatch::Dead);
    }

    #[test]
    fn every_typed_variant_encodes_to_its_pdb_sizeof() {
        let cases: Vec<(NetMsg<'static>, u16)> = vec![
            (NetMsg::Generic { buffer: [7u8; 512] }, 513),
            (
                NetMsg::Chat {
                    observer_to_all: 1,
                    message: "hej".into(),
                },
                514,
            ),
            (NetMsg::Ping { cx: -3, cy: 9 }, 9),
            (
                NetMsg::Pause {
                    play: 2,
                    pause_time: 5,
                    requested_state: 1,
                },
                10,
            ),
            (NetMsg::Taunt { taunt: 3 }, 2),
            (NetMsg::SyncSignal { play: 4 }, 5),
            (
                NetMsg::DropStamp {
                    play_from: 1,
                    stamp: 2,
                    play: 3,
                },
                13,
            ),
            (
                NetMsg::TimeSync {
                    time_stamp_sent: 1,
                    time_stamp_received: 2,
                },
                9,
            ),
            (NetMsg::DropVote { play: 1, vote: -1 }, 3),
            (NetMsg::DropDecision { vote: 1 }, 2),
            (NetMsg::DropFlag { pid: 77 }, 5),
            (NetMsg::GameModSyncRequest { desired: 1 }, 2),
            (NetMsg::GameModSyncResponse { required: 1 }, 2),
            (NetMsg::PlayerStatusRequest, 1),
            (
                NetMsg::PlayerStatusResponse {
                    player_id: [1, 2, 3, 4, 5, 6, 7, 8],
                    time_since_last_pulse: [9; 8],
                },
                65,
            ),
        ];
        for (m, _) in &cases {
            let mut b = Vec::new();
            m.encode(&mut b);
            let expect = MSG_SIZE[m.id() as usize].expect("fixed-size variant");
            assert_eq!(b.len(), expect as usize, "size mismatch for id {}", m.id());
        }
        for (m, want) in &cases {
            let mut b = Vec::new();
            m.encode(&mut b);
            assert_eq!(b.len(), *want as usize);
            let got = NetMsg::decode(&b).unwrap();
            assert!(!got.ty.response);
            assert_eq!(&got.msg, m, "round-trip mismatch for id {}", m.id());
        }
    }

    #[test]
    fn command_package_is_nine_bytes_plus_payload() {
        let payload = [1u8, 2, 3, 4, 5];
        let m = NetMsg::CommandPackage {
            stamp: 0xdead_beef,
            play: 3,
            payload: &payload,
        };
        let mut b = Vec::new();
        m.encode(&mut b);
        assert_eq!(b.len(), 8 + payload.len());
        assert_eq!(b[0], 7);
        let got = NetMsg::decode(&b).unwrap();
        assert_eq!(got.msg, m);
        // and with the response flag set, the same handler must be selected
        let mut b2 = Vec::new();
        m.encode_framed(true, &mut b2);
        assert_eq!(b2[0], 7 | 64);
        let got2 = NetMsg::decode(&b2).unwrap();
        assert!(got2.ty.response);
        assert_eq!(got2.msg, m);
    }

    #[test]
    fn a_command_package_carrying_no_commands_is_eight_bytes_and_decodes() {
        // `sizeof` is 9 because of the `unsigned char[1]` flexible tail, but the
        // wire length is `8 + data_size`, and retail's own send path handles
        // `data_size == 0`. This record must survive a round trip rather than
        // being dropped one byte short.
        let m = NetMsg::CommandPackage {
            stamp: 1,
            play: 1,
            payload: &[],
        };
        let mut b = Vec::new();
        m.encode(&mut b);
        assert_eq!(b.len(), 8);
        assert_eq!(NetMsg::decode(&b).unwrap().msg, m);
        assert_eq!(
            NetMsg::decode(&b[..7]),
            Err(MsgError::Short {
                id: 7,
                need: 8,
                have: 7
            })
        );
    }

    #[test]
    fn internal_packet_types_are_rejected_not_misparsed() {
        assert_eq!(NetMsg::decode(&[128, 0]), Err(MsgError::IsInternal(128)));
        assert_eq!(NetMsg::decode(&[136, 1]), Err(MsgError::IsInternal(136)));
    }

    #[test]
    fn wide_strings_truncate_leaving_the_terminator() {
        let mut out = Vec::new();
        write_wide(&mut out, "abcdef", 4);
        assert_eq!(out.len(), 8);
        assert_eq!(read_wide(&out, 0, 4), "abc");
    }
}
