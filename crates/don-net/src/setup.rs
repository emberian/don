//! Game-setup records: `GameConnectionData`, `GameConnectionDataFull`,
//! `ScenFilePreviewData`, `PlayerConnectionData`.
//!
//! # What these are, and what they are not
//!
//! These are the **in-memory** records `ConnectionData` holds
//! (`ConnectionData::game_data` is a `GameConnectionDataFull` at +0x298,
//! `player_data_server` is a `PlayerConnectionData[8]` at +0). Every offset and
//! size below is `schema/types.json`, i.e. `rise.pdb`'s own TPI. **[measured]**
//!
//! They are **not** what crosses the network in this build. The message ids
//! that used to carry them — `NETMSG_GAMECONNECTIONDATA`(3),
//! `NETMSG_GAMECONNECTIONDATAFULL`(4), `NETMSG_PLAYERCONNECTIONDATA`(1),
//! `NETMSG_ALLPLAYERCONNECTIONDATA`(2) — all land on the *default* arm of
//! `NetDaemon::process`'s jump table and log an error (see [`crate::msg`]).
//! `ConnectionData::send_game` `0x0094E200` and `ConnectionData::send_player`
//! `0x0094EC40` never call `NetSys::send*`; they check `NetSys::is_host`
//! (vtable +0x10) and then publish an `unordered_map<wstring, wstring>` through
//! `MultiplayerManager::Instance()`. The key schema is in [`crate::lobby`].
//!
//! So: encode/decode here is for **our own** transport and for reading the
//! records out of a live process or a save, and the field order is the
//! compiler's memory order. `GameConnectionData` is a pure POD (46 bytes, no
//! padding, no pointers), so its byte image *is* portable.
//! `PlayerConnectionData` is not — two of its members are `std::wstring`, whose
//! 24-byte MSVC representation is a union of an 8-`wchar_t` SSO buffer and a
//! heap pointer. A PlayFab entity id is 16 hex characters, i.e. always past the
//! SSO limit, so a raw 59-byte copy of that struct would ship a *pointer*.
//! [`PlayerConnectionData`] therefore has a length-prefixed portable form and
//! the POD tail is exposed separately.

use crate::msg::{read_narrow, read_wide, write_narrow, write_wide};

/// `GameConnectionData`, `sizeof` 46. **[measured]**
///
/// The 30 leading bytes are a union: 30 named `unsigned char` settings overlaid
/// with `unsigned char data[30]`. We keep the named view; `data()` reproduces
/// the union.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GameConnectionData {
    pub team_style: u8,
    pub map_style: u8,
    pub map_size: u8,
    pub players: u8,
    pub max_observers: u8,
    pub game_speed: u8,
    pub game_rules: u8,
    pub difficulty: u8,
    pub starting_town: u8,
    pub starting_resources: u8,
    pub starting_resources2: u8,
    pub tech_cost: u8,
    pub reveal_map: u8,
    pub pop_limit: u8,
    pub rush_rules: u8,
    pub cannon_times: u8,
    pub starting_technology: u8,
    pub starting_technology2: u8,
    pub ending_technology: u8,
    pub elimination: u8,
    pub victory: u8,
    pub wonderwin: u8,
    pub score_goal: u8,
    pub popwin: u8,
    pub time_limit: u8,
    pub chairs: u8,
    pub econwin: u8,
    pub scenario_type: u8,
    pub script_type: u8,
    pub mods: u8,
    /// +30
    pub lobby_elo: i32,
    /// +34 — the simulation RNG seed every peer must agree on.
    pub seed: u32,
    /// +38
    pub flags: i32,
    /// +42 — how many frames of history the checksum ring keeps.
    pub checksum_window_size: u16,
    /// +44
    pub checksum_deep: u8,
    /// +45
    pub checksum_failure_threshold: u8,
}

impl GameConnectionData {
    pub const WIRE_LEN: usize = 46;

    /// The `unsigned char data[30]` view of the leading settings union.
    pub fn data(&self) -> [u8; 30] {
        [
            self.team_style, self.map_style, self.map_size, self.players,
            self.max_observers, self.game_speed, self.game_rules, self.difficulty,
            self.starting_town, self.starting_resources, self.starting_resources2,
            self.tech_cost, self.reveal_map, self.pop_limit, self.rush_rules,
            self.cannon_times, self.starting_technology, self.starting_technology2,
            self.ending_technology, self.elimination, self.victory, self.wonderwin,
            self.score_goal, self.popwin, self.time_limit, self.chairs, self.econwin,
            self.scenario_type, self.script_type, self.mods,
        ]
    }

    pub fn set_data(&mut self, d: [u8; 30]) {
        self.team_style = d[0];
        self.map_style = d[1];
        self.map_size = d[2];
        self.players = d[3];
        self.max_observers = d[4];
        self.game_speed = d[5];
        self.game_rules = d[6];
        self.difficulty = d[7];
        self.starting_town = d[8];
        self.starting_resources = d[9];
        self.starting_resources2 = d[10];
        self.tech_cost = d[11];
        self.reveal_map = d[12];
        self.pop_limit = d[13];
        self.rush_rules = d[14];
        self.cannon_times = d[15];
        self.starting_technology = d[16];
        self.starting_technology2 = d[17];
        self.ending_technology = d[18];
        self.elimination = d[19];
        self.victory = d[20];
        self.wonderwin = d[21];
        self.score_goal = d[22];
        self.popwin = d[23];
        self.time_limit = d[24];
        self.chairs = d[25];
        self.econwin = d[26];
        self.scenario_type = d[27];
        self.script_type = d[28];
        self.mods = d[29];
    }

    pub fn decode(b: &[u8]) -> Option<Self> {
        if b.len() < Self::WIRE_LEN {
            return None;
        }
        let mut d = [0u8; 30];
        d.copy_from_slice(&b[0..30]);
        let mut g = GameConnectionData::default();
        g.set_data(d);
        g.lobby_elo = i32::from_le_bytes(b[30..34].try_into().unwrap());
        g.seed = u32::from_le_bytes(b[34..38].try_into().unwrap());
        g.flags = i32::from_le_bytes(b[38..42].try_into().unwrap());
        g.checksum_window_size = u16::from_le_bytes([b[42], b[43]]);
        g.checksum_deep = b[44];
        g.checksum_failure_threshold = b[45];
        Some(g)
    }

    pub fn encode(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.data());
        out.extend_from_slice(&self.lobby_elo.to_le_bytes());
        out.extend_from_slice(&self.seed.to_le_bytes());
        out.extend_from_slice(&self.flags.to_le_bytes());
        out.extend_from_slice(&self.checksum_window_size.to_le_bytes());
        out.push(self.checksum_deep);
        out.push(self.checksum_failure_threshold);
    }
}

/// `ScenFilePreviewData`, `sizeof` 45. **[measured]**
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScenFilePreviewData {
    pub num_leaders: u8,
    pub num_human_players: u8,
    pub team_style: u8,
    pub map_size: u8,
    pub scenario_flags: u8,
    pub leader_active: [u8; 8],
    pub leader_tribe: [u8; 8],
    pub leader_team: [u8; 8],
    pub leader_diff: [u8; 8],
    pub player_type: [u8; 8],
}

impl Default for ScenFilePreviewData {
    fn default() -> Self {
        ScenFilePreviewData {
            num_leaders: 0,
            num_human_players: 0,
            team_style: 0,
            map_size: 0,
            scenario_flags: 0,
            leader_active: [0; 8],
            leader_tribe: [0; 8],
            leader_team: [0; 8],
            leader_diff: [0; 8],
            player_type: [0; 8],
        }
    }
}

impl ScenFilePreviewData {
    pub const WIRE_LEN: usize = 45;

    pub fn decode(b: &[u8]) -> Option<Self> {
        if b.len() < Self::WIRE_LEN {
            return None;
        }
        let arr = |o: usize| -> [u8; 8] { b[o..o + 8].try_into().unwrap() };
        Some(ScenFilePreviewData {
            num_leaders: b[0],
            num_human_players: b[1],
            team_style: b[2],
            map_size: b[3],
            scenario_flags: b[4],
            leader_active: arr(5),
            leader_tribe: arr(13),
            leader_team: arr(21),
            leader_diff: arr(29),
            player_type: arr(37),
        })
    }

    pub fn encode(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&[
            self.num_leaders,
            self.num_human_players,
            self.team_style,
            self.map_size,
            self.scenario_flags,
        ]);
        out.extend_from_slice(&self.leader_active);
        out.extend_from_slice(&self.leader_tribe);
        out.extend_from_slice(&self.leader_team);
        out.extend_from_slice(&self.leader_diff);
        out.extend_from_slice(&self.player_type);
    }
}

/// `GameConnectionDataFull`, `sizeof` 2031 (0x7EF). **[measured]**
///
/// `scenario_name` and `script_name` are the *same* 180 bytes at +0x2E — a
/// union, disambiguated by `data.scenario_type` / `data.script_type`. We hold
/// the bytes once, under `scenario_or_script_name`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GameConnectionDataFull {
    pub data: GameConnectionData,
    /// +0x2E, `char[180]`, union of `scenario_name` and `script_name`.
    pub scenario_or_script_name: String,
    /// +0xE2, `wchar_t[260]`
    pub save_name: String,
    /// +0x2EA, `char[512]`
    pub desc: String,
    /// +0x4EA, `char[180]`
    pub mod_name: String,
    /// +0x59E, `char[512]`
    pub mod_desc: String,
    /// +0x79E
    pub mod_size: u32,
    /// +0x7A2
    pub mod_checksum: u32,
    /// +0x7A6
    pub mod_workshop_id: u64,
    /// +0x7AE
    pub scenario_data: ScenFilePreviewData,
    /// +0x7DB
    pub scenario_size: u32,
    /// +0x7DF
    pub scenario_checksum: u32,
    /// +0x7E3
    pub scenario_num_files: u32,
    /// +0x7E7
    pub scenario_workshop_id: u64,
}

/// Field offsets inside `GameConnectionDataFull`, straight from the PDB.
pub mod full_offsets {
    pub const DATA: usize = 0x000;
    pub const SCENARIO_NAME: usize = 0x02E;
    pub const SCRIPT_NAME: usize = 0x02E; // union with SCENARIO_NAME
    pub const SAVE_NAME: usize = 0x0E2;
    pub const DESC: usize = 0x2EA;
    pub const MOD_NAME: usize = 0x4EA;
    pub const MOD_DESC: usize = 0x59E;
    pub const MOD_SIZE: usize = 0x79E;
    pub const MOD_CHECKSUM: usize = 0x7A2;
    pub const MOD_WORKSHOP_ID: usize = 0x7A6;
    pub const SCENARIO_DATA: usize = 0x7AE;
    pub const SCENARIO_SIZE: usize = 0x7DB;
    pub const SCENARIO_CHECKSUM: usize = 0x7DF;
    pub const SCENARIO_NUM_FILES: usize = 0x7E3;
    pub const SCENARIO_WORKSHOP_ID: usize = 0x7E7;

    pub const SCENARIO_NAME_LEN: usize = 180;
    pub const SAVE_NAME_UNITS: usize = 260;
    pub const DESC_LEN: usize = 512;
    pub const MOD_NAME_LEN: usize = 180;
    pub const MOD_DESC_LEN: usize = 512;
}

impl GameConnectionDataFull {
    pub const WIRE_LEN: usize = 0x7EF; // 2031

    pub fn decode(b: &[u8]) -> Option<Self> {
        use full_offsets as O;
        if b.len() < Self::WIRE_LEN {
            return None;
        }
        let u32at = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
        let u64at = |o: usize| u64::from_le_bytes(b[o..o + 8].try_into().unwrap());
        Some(GameConnectionDataFull {
            data: GameConnectionData::decode(&b[O::DATA..])?,
            scenario_or_script_name: read_narrow(b, O::SCENARIO_NAME, O::SCENARIO_NAME_LEN),
            save_name: read_wide(b, O::SAVE_NAME, O::SAVE_NAME_UNITS),
            desc: read_narrow(b, O::DESC, O::DESC_LEN),
            mod_name: read_narrow(b, O::MOD_NAME, O::MOD_NAME_LEN),
            mod_desc: read_narrow(b, O::MOD_DESC, O::MOD_DESC_LEN),
            mod_size: u32at(O::MOD_SIZE),
            mod_checksum: u32at(O::MOD_CHECKSUM),
            mod_workshop_id: u64at(O::MOD_WORKSHOP_ID),
            scenario_data: ScenFilePreviewData::decode(&b[O::SCENARIO_DATA..])?,
            scenario_size: u32at(O::SCENARIO_SIZE),
            scenario_checksum: u32at(O::SCENARIO_CHECKSUM),
            scenario_num_files: u32at(O::SCENARIO_NUM_FILES),
            scenario_workshop_id: u64at(O::SCENARIO_WORKSHOP_ID),
        })
    }

    pub fn encode(&self, out: &mut Vec<u8>) {
        use full_offsets as O;
        let start = out.len();
        self.data.encode(out);
        debug_assert_eq!(out.len() - start, O::SCENARIO_NAME);
        write_narrow(out, &self.scenario_or_script_name, O::SCENARIO_NAME_LEN);
        write_wide(out, &self.save_name, O::SAVE_NAME_UNITS);
        write_narrow(out, &self.desc, O::DESC_LEN);
        write_narrow(out, &self.mod_name, O::MOD_NAME_LEN);
        write_narrow(out, &self.mod_desc, O::MOD_DESC_LEN);
        out.extend_from_slice(&self.mod_size.to_le_bytes());
        out.extend_from_slice(&self.mod_checksum.to_le_bytes());
        out.extend_from_slice(&self.mod_workshop_id.to_le_bytes());
        self.scenario_data.encode(out);
        out.extend_from_slice(&self.scenario_size.to_le_bytes());
        out.extend_from_slice(&self.scenario_checksum.to_le_bytes());
        out.extend_from_slice(&self.scenario_num_files.to_le_bytes());
        out.extend_from_slice(&self.scenario_workshop_id.to_le_bytes());
        debug_assert_eq!(out.len() - start, Self::WIRE_LEN);
    }
}

/// The POD tail of `PlayerConnectionData` — the 11 bytes from +48 to +58 that
/// *are* raw-copyable. **[measured]**
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PlayerSlotPod {
    /// +48
    pub elo: i32,
    /// +52
    pub slot_type: u8,
    /// +53
    pub tribe: u8,
    /// +54
    pub who: u8,
    /// +55
    pub team: u8,
    /// +56
    pub handicap: u8,
    /// +57
    pub diff: u8,
    /// +58 — the lobby-level ready flag, distinct from the netlib's
    /// `CrossplayNetLibPlayer::ready` at +40 driven by `IPT_READYFLAG`.
    pub ready: u8,
}

impl PlayerSlotPod {
    pub const WIRE_LEN: usize = 11;

    pub fn decode(b: &[u8]) -> Option<Self> {
        if b.len() < Self::WIRE_LEN {
            return None;
        }
        Some(PlayerSlotPod {
            elo: i32::from_le_bytes(b[0..4].try_into().unwrap()),
            slot_type: b[4],
            tribe: b[5],
            who: b[6],
            team: b[7],
            handicap: b[8],
            diff: b[9],
            ready: b[10],
        })
    }

    pub fn encode(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.elo.to_le_bytes());
        out.extend_from_slice(&[
            self.slot_type, self.tribe, self.who, self.team, self.handicap, self.diff, self.ready,
        ]);
    }
}

/// `PlayerConnectionData`, `sizeof` 59 in memory. **[measured]**
///
/// The two ids are `std::wstring`. See the module header for why the in-memory
/// image is not a wire format; [`encode_portable`](Self::encode_portable) emits
/// a length-prefixed form instead.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlayerConnectionData {
    /// +0, `std::wstring`
    pub player_id: String,
    /// +24, `std::wstring`
    pub platform_player_id: String,
    pub pod: PlayerSlotPod,
}

impl PlayerConnectionData {
    /// `sizeof` of the in-memory struct. Not a wire length.
    pub const MEMORY_SIZE: usize = 59;

    /// Length-prefixed portable encoding: `u16 len + UTF-16LE` per id, then the
    /// 11-byte POD tail. **This is ours, not the engine's** — the engine never
    /// puts this struct on a socket in this build.
    pub fn encode_portable(&self, out: &mut Vec<u8>) {
        for s in [&self.player_id, &self.platform_player_id] {
            let units: Vec<u16> = s.encode_utf16().collect();
            out.extend_from_slice(&(units.len() as u16).to_le_bytes());
            for u in units {
                out.extend_from_slice(&u.to_le_bytes());
            }
        }
        self.pod.encode(out);
    }

    pub fn decode_portable(b: &[u8]) -> Option<(Self, usize)> {
        let mut o = 0usize;
        let mut ids = [String::new(), String::new()];
        for id in ids.iter_mut() {
            if b.len() < o + 2 {
                return None;
            }
            let n = u16::from_le_bytes([b[o], b[o + 1]]) as usize;
            o += 2;
            if b.len() < o + n * 2 {
                return None;
            }
            let units: Vec<u16> =
                (0..n).map(|i| u16::from_le_bytes([b[o + i * 2], b[o + i * 2 + 1]])).collect();
            *id = String::from_utf16_lossy(&units);
            o += n * 2;
        }
        let pod = PlayerSlotPod::decode(&b[o..])?;
        o += PlayerSlotPod::WIRE_LEN;
        let [player_id, platform_player_id] = ids;
        Some((PlayerConnectionData { player_id, platform_player_id, pod }, o))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn game_connection_data_is_forty_six_bytes_and_round_trips() {
        let mut g = GameConnectionData {
            seed: 0xCAFE_BABE,
            checksum_window_size: 64,
            checksum_deep: 1,
            checksum_failure_threshold: 3,
            lobby_elo: -1200,
            flags: 0x0F0F,
            ..Default::default()
        };
        g.set_data([9u8; 30]);
        let mut b = Vec::new();
        g.encode(&mut b);
        assert_eq!(b.len(), GameConnectionData::WIRE_LEN);
        assert_eq!(GameConnectionData::decode(&b), Some(g));
        // the union really is the first 30 bytes
        assert_eq!(&b[0..30], &[9u8; 30]);
        // seed sits at +34 exactly
        assert_eq!(&b[34..38], &0xCAFE_BABEu32.to_le_bytes());
    }

    #[test]
    fn full_record_is_exactly_2031_bytes_with_the_pdb_offsets() {
        let f = GameConnectionDataFull {
            data: GameConnectionData { seed: 7, ..Default::default() },
            scenario_or_script_name: "alpine".into(),
            save_name: "autosave".into(),
            desc: "a description".into(),
            mod_name: "themod".into(),
            mod_desc: "moddesc".into(),
            mod_size: 1234,
            mod_checksum: 0xdead_beef,
            mod_workshop_id: 0x1122_3344_5566_7788,
            scenario_data: ScenFilePreviewData { num_leaders: 6, ..Default::default() },
            scenario_size: 11,
            scenario_checksum: 22,
            scenario_num_files: 33,
            scenario_workshop_id: 44,
        };
        let mut b = Vec::new();
        f.encode(&mut b);
        assert_eq!(b.len(), 2031);
        assert_eq!(b.len(), GameConnectionDataFull::WIRE_LEN);
        // spot-check that the strings landed on their PDB offsets
        use full_offsets as O;
        assert_eq!(read_narrow(&b, O::SCENARIO_NAME, O::SCENARIO_NAME_LEN), "alpine");
        assert_eq!(read_wide(&b, O::SAVE_NAME, O::SAVE_NAME_UNITS), "autosave");
        assert_eq!(read_narrow(&b, O::MOD_NAME, O::MOD_NAME_LEN), "themod");
        assert_eq!(
            u64::from_le_bytes(b[O::MOD_WORKSHOP_ID..O::MOD_WORKSHOP_ID + 8].try_into().unwrap()),
            0x1122_3344_5566_7788
        );
        assert_eq!(GameConnectionDataFull::decode(&b), Some(f));
    }

    #[test]
    fn scenario_preview_is_forty_five_bytes() {
        let s = ScenFilePreviewData {
            num_leaders: 8,
            num_human_players: 2,
            team_style: 1,
            map_size: 3,
            scenario_flags: 0,
            leader_active: [1, 1, 0, 0, 0, 0, 0, 0],
            leader_tribe: [4, 9, 0, 0, 0, 0, 0, 0],
            leader_team: [1, 2, 0, 0, 0, 0, 0, 0],
            leader_diff: [3; 8],
            player_type: [1, 2, 0, 0, 0, 0, 0, 0],
        };
        let mut b = Vec::new();
        s.encode(&mut b);
        assert_eq!(b.len(), 45);
        assert_eq!(ScenFilePreviewData::decode(&b), Some(s));
    }

    #[test]
    fn player_ids_longer_than_the_sso_buffer_survive_a_portable_round_trip() {
        // 16 hex characters: a PlayFab entity id. The MSVC wstring SSO buffer is
        // eight wchar_t, so this one is heap-allocated in the real struct — the
        // reason a raw 59-byte copy of PlayerConnectionData is not a wire format.
        let p = PlayerConnectionData {
            player_id: "A1B2C3D4E5F60718".into(),
            platform_player_id: "76561198000000000".into(),
            pod: PlayerSlotPod {
                elo: 1500,
                slot_type: 1,
                tribe: 4,
                who: 2,
                team: 1,
                handicap: 0,
                diff: 3,
                ready: 1,
            },
        };
        assert!(p.player_id.len() > 8, "must exceed the SSO limit to be a real test");
        let mut b = Vec::new();
        p.encode_portable(&mut b);
        let (got, n) = PlayerConnectionData::decode_portable(&b).unwrap();
        assert_eq!(n, b.len());
        assert_eq!(got, p);
        assert_ne!(b.len(), PlayerConnectionData::MEMORY_SIZE);
    }
}
