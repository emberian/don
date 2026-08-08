//! `InternalPacketType` — the netlib's own control plane.
//!
//! These are the packets `CrossplayNetLibSys` handles itself and never hands to
//! the game. Every id is `>= 128`, which is why the game's own `NetMsgType`
//! space stops at 32 and the reply flag is 64: the top bit is the discriminator
//! between the two protocols.
//!
//! Layouts are `CrossplayNetLib.pdb` type `0x46AA` plus the struct records
//! beside it; every one is `#pragma pack(1)` — proven by the `int` members
//! sitting at offset +1. **[measured]**
//!
//! Handlers, all `CrossplayNetLibSys::`: `process_playerlist`,
//! `process_drop_request`, `process_cancel_drop_request`, `process_pulse`,
//! `process_create_player_message`, `process_destroy_player_message`,
//! `process_host_migrate_message`, `process_dsync`, `process_ready_flag`.

/// `IPT_BASE`. Any type byte at or above this belongs to the netlib.
pub const IPT_BASE: u8 = 128;

pub const IPT_PLAYERLIST: u8 = 128;
pub const IPT_DROPREQUEST: u8 = 129;
pub const IPT_CANCELDROPREQUEST: u8 = 130;
pub const IPT_PULSEPACKET: u8 = 131;
pub const IPT_ADDPLAYER: u8 = 132;
pub const IPT_DESTROYPLAYER: u8 = 133;
pub const IPT_MIGRATEHOST: u8 = 134;
pub const IPT_DSYNCMSG: u8 = 135;
pub const IPT_READYFLAG: u8 = 136;

/// `sizeof` of the struct for each internal type, indexed by `id - IPT_BASE`.
/// **[measured]**
pub const IPT_SIZE: [u16; 9] = [34, 5, 5, 1, 70, 5, 5, 5, 2];

/// The netlib's `AddPlayerRequest::player_name` is a fixed 64-byte `char` array.
pub const ADD_PLAYER_NAME_LEN: usize = 64;

/// Maximum players the netlib tracks; `HostPlayerList::unique_ids` is `i32[8]`
/// and `NetSys::players` is `NetPlayer*[8]`. **[measured]**
pub const MAX_PLAYERS: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InternalPacket {
    /// `HostPlayerList { u8 type; u8 num_players; i32 unique_ids[8]; }` — 34 B.
    /// The host's authoritative roster; the netlib rebuilds its player array
    /// from it. Note the array is always eight entries on the wire regardless
    /// of `num_players`.
    PlayerList { num_players: u8, unique_ids: [i32; MAX_PLAYERS] },
    /// `DropRequest { u8 type; i32 unique_id; }` — 5 B.
    DropRequest { unique_id: i32 },
    /// `CancelDropRequest` — 5 B, same shape.
    CancelDropRequest { unique_id: i32 },
    /// `GenericNetPacket` alone — 1 B. Liveness; drives `last_pulse`.
    Pulse,
    /// `AddPlayerRequest { u8 type; char player_name[64]; i32 unique_id;
    /// bool is_hosting; }` — 70 B. The join announcement.
    AddPlayer { player_name: String, unique_id: i32, is_hosting: bool },
    /// `DestroyPlayerRequest` — 5 B.
    DestroyPlayer { unique_id: i32 },
    /// `MigrateHostRequest { u8 type; i32 new_host; }` — 5 B.
    MigrateHost { new_host: i32 },
    /// `DsyncNotification { u8 type; i32 frame; }` — 5 B. The out-of-sync
    /// report; `CommandManager::recover_from_oos` `0x0093E9E0` is what acts on
    /// the resulting vote.
    Dsync { frame: i32 },
    /// `ReadyFlagRequest { u8 type; bool ready; }` — 2 B. Emitted by
    /// `send_ready_flag(bool)`, consumed by `process_ready_flag`, lands in
    /// `CrossplayNetLibPlayer::ready` at +40.
    ReadyFlag { ready: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InternalError {
    NotInternal(u8),
    UnknownType(u8),
    Short { id: u8, need: usize, have: usize },
}

impl core::fmt::Display for InternalError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            InternalError::NotInternal(b) => write!(f, "type {b} is below IPT_BASE"),
            InternalError::UnknownType(b) => write!(f, "no InternalPacketType {b}"),
            InternalError::Short { id, need, have } => {
                write!(f, "internal packet {id}: need {need}, have {have}")
            }
        }
    }
}

impl std::error::Error for InternalError {}

fn i32le(b: &[u8], o: usize) -> i32 {
    i32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

impl InternalPacket {
    pub fn id(&self) -> u8 {
        match self {
            InternalPacket::PlayerList { .. } => IPT_PLAYERLIST,
            InternalPacket::DropRequest { .. } => IPT_DROPREQUEST,
            InternalPacket::CancelDropRequest { .. } => IPT_CANCELDROPREQUEST,
            InternalPacket::Pulse => IPT_PULSEPACKET,
            InternalPacket::AddPlayer { .. } => IPT_ADDPLAYER,
            InternalPacket::DestroyPlayer { .. } => IPT_DESTROYPLAYER,
            InternalPacket::MigrateHost { .. } => IPT_MIGRATEHOST,
            InternalPacket::Dsync { .. } => IPT_DSYNCMSG,
            InternalPacket::ReadyFlag { .. } => IPT_READYFLAG,
        }
    }

    /// The engine's `sizeof` for this packet.
    pub fn wire_len(&self) -> usize {
        IPT_SIZE[(self.id() - IPT_BASE) as usize] as usize
    }

    pub fn decode(buf: &[u8]) -> Result<Self, InternalError> {
        let id = *buf.first().ok_or(InternalError::NotInternal(0))?;
        if id < IPT_BASE {
            return Err(InternalError::NotInternal(id));
        }
        let idx = (id - IPT_BASE) as usize;
        let need = *IPT_SIZE.get(idx).ok_or(InternalError::UnknownType(id))? as usize;
        if buf.len() < need {
            return Err(InternalError::Short { id, need, have: buf.len() });
        }
        Ok(match id {
            IPT_PLAYERLIST => {
                let mut ids = [0i32; MAX_PLAYERS];
                for (i, slot) in ids.iter_mut().enumerate() {
                    *slot = i32le(buf, 2 + i * 4);
                }
                InternalPacket::PlayerList { num_players: buf[1], unique_ids: ids }
            }
            IPT_DROPREQUEST => InternalPacket::DropRequest { unique_id: i32le(buf, 1) },
            IPT_CANCELDROPREQUEST => {
                InternalPacket::CancelDropRequest { unique_id: i32le(buf, 1) }
            }
            IPT_PULSEPACKET => InternalPacket::Pulse,
            IPT_ADDPLAYER => InternalPacket::AddPlayer {
                player_name: crate::msg::read_narrow(buf, 1, ADD_PLAYER_NAME_LEN),
                unique_id: i32le(buf, 65),
                is_hosting: buf[69] != 0,
            },
            IPT_DESTROYPLAYER => InternalPacket::DestroyPlayer { unique_id: i32le(buf, 1) },
            IPT_MIGRATEHOST => InternalPacket::MigrateHost { new_host: i32le(buf, 1) },
            IPT_DSYNCMSG => InternalPacket::Dsync { frame: i32le(buf, 1) },
            IPT_READYFLAG => InternalPacket::ReadyFlag { ready: buf[1] != 0 },
            other => return Err(InternalError::UnknownType(other)),
        })
    }

    pub fn encode(&self, out: &mut Vec<u8>) {
        out.push(self.id());
        match self {
            InternalPacket::PlayerList { num_players, unique_ids } => {
                out.push(*num_players);
                for v in unique_ids {
                    out.extend_from_slice(&v.to_le_bytes());
                }
            }
            InternalPacket::DropRequest { unique_id }
            | InternalPacket::CancelDropRequest { unique_id }
            | InternalPacket::DestroyPlayer { unique_id } => {
                out.extend_from_slice(&unique_id.to_le_bytes())
            }
            InternalPacket::Pulse => {}
            InternalPacket::AddPlayer { player_name, unique_id, is_hosting } => {
                crate::msg::write_narrow(out, player_name, ADD_PLAYER_NAME_LEN);
                out.extend_from_slice(&unique_id.to_le_bytes());
                out.push(u8::from(*is_hosting));
            }
            InternalPacket::MigrateHost { new_host } => {
                out.extend_from_slice(&new_host.to_le_bytes())
            }
            InternalPacket::Dsync { frame } => out.extend_from_slice(&frame.to_le_bytes()),
            InternalPacket::ReadyFlag { ready } => out.push(u8::from(*ready)),
        }
    }
}

/// `GenericSessionData { u8 type; u8 validity_number; }` — 2 B. Carried in the
/// session record and checked by `NetMessenger::allow_connection`, which
/// `NetDaemon::allow_connection` `0x00951560` implements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GenericSessionData {
    pub ty: u8,
    pub validity_number: u8,
}

impl GenericSessionData {
    pub const WIRE_LEN: usize = 2;
    pub fn decode(b: &[u8]) -> Option<Self> {
        (b.len() >= 2).then(|| GenericSessionData { ty: b[0], validity_number: b[1] })
    }
    pub fn encode(&self, out: &mut Vec<u8>) {
        out.push(self.ty);
        out.push(self.validity_number);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_internal_packet_encodes_to_its_pdb_sizeof() {
        let cases = [
            InternalPacket::PlayerList { num_players: 3, unique_ids: [1, 2, 3, 0, 0, 0, 0, 0] },
            InternalPacket::DropRequest { unique_id: -5 },
            InternalPacket::CancelDropRequest { unique_id: 7 },
            InternalPacket::Pulse,
            InternalPacket::AddPlayer {
                player_name: "ember".into(),
                unique_id: 0x1234_5678,
                is_hosting: true,
            },
            InternalPacket::DestroyPlayer { unique_id: 9 },
            InternalPacket::MigrateHost { new_host: 2 },
            InternalPacket::Dsync { frame: 4242 },
            InternalPacket::ReadyFlag { ready: true },
        ];
        for c in &cases {
            let mut b = Vec::new();
            c.encode(&mut b);
            assert_eq!(b.len(), c.wire_len(), "sizeof mismatch for {c:?}");
            assert_eq!(&InternalPacket::decode(&b).unwrap(), c);
        }
    }

    #[test]
    fn the_pdb_sizeofs_are_what_pack_one_implies() {
        // pack(1) is what makes these numbers what they are: an i32 at +1.
        assert_eq!(IPT_SIZE[(IPT_DROPREQUEST - IPT_BASE) as usize], 1 + 4);
        assert_eq!(IPT_SIZE[(IPT_PLAYERLIST - IPT_BASE) as usize], 1 + 1 + 4 * 8);
        assert_eq!(IPT_SIZE[(IPT_ADDPLAYER - IPT_BASE) as usize], 1 + 64 + 4 + 1);
        assert_eq!(IPT_SIZE[(IPT_READYFLAG - IPT_BASE) as usize], 1 + 1);
    }

    #[test]
    fn game_and_netlib_id_spaces_do_not_overlap() {
        for id in 0u8..128 {
            assert!(InternalPacket::decode(&[id; 80]).is_err());
        }
        assert!(InternalPacket::decode(&[IPT_PULSEPACKET]).is_ok());
    }
}
