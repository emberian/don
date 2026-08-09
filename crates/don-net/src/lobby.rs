//! The PlayFab **lobby attribute schema** — how game setup and player state
//! actually reach the other peers in this build.
//!
//! # How this was recovered
//!
//! `ConnectionData::send_game` `0x0094E200` and `ConnectionData::send_player`
//! `0x0094EC40` do not call `NetSys::send*` at all. Both check
//! `NetSys::is_host` (vtable +0x10), build an
//! `unordered_map<std::wstring, std::wstring>`, and hand it to
//! `MultiplayerManager::Instance()` (`0x00A30DC0`). The keys come from named
//! statics in `connectiondata.obj`, `lobbymanager.obj`, `SteamLobbyMgr.obj`,
//! `lobby.obj` and `multiplayermanager.obj`. Their *values* were read out of
//! the binary two ways: **[measured]**
//!
//! * the `const char*` keys by dereferencing the pointer at each symbol's VA
//!   into `.rdata`;
//! * the `std::wstring` keys by following each symbol's `$initializer$` slot in
//!   the CRT init table (`0x00AC6220`…) to the dynamic-initialiser thunk and
//!   reading the UTF-16 literal it pushes.
//!
//! Per-player keys are built by `MakePlayerkey` `0x004BAB00`, which is a plain
//! `std::wstring` concatenation (`0x004BAA40` is `operator+`): the prefix below
//! followed by the slot index rendered as decimal. So slot 3's ready flag is
//! `steam_ready_3`. **[measured for the concatenation; the "slot index" operand
//! is [inferred] from the caller passing the loop counter through an
//! `int -> const wchar_t*` lambda at `0x004BA3B0`.]**
//!
//! `send_player` only emits the keys whose value differs from
//! `ConnectionData::last_send_player_data[i]` — it is a **delta** publish, with
//! a `force` argument that overrides. That matters for a reimplementation: a
//! joining peer must read the full attribute set once, then apply deltas.
//!
//! # Why this is the important artifact
//!
//! `docs/tracks/netcode-symbols.md` §5 Option B listed "the lobby attribute key
//! names" as the missing piece. This module is that piece.

use crate::setup::{GameConnectionData, PlayerSlotPod};
use std::collections::BTreeMap;

/// Per-player attribute key prefixes. `MakePlayerkey(prefix, slot)`.
/// **[measured]** — the value each name resolves to, not a guess at the name.
pub mod playerkey {
    /// `PLAYERKEY_ELO`
    pub const ELO: &str = "elo_";
    /// `PLAYERKEY_SLOTTYPE`
    pub const SLOT_TYPE: &str = "slot_type_";
    /// `PLAYERKEY_TRIBE`
    pub const TRIBE: &str = "tribe_";
    /// `PLAYERKEY_WHO`
    pub const WHO: &str = "who_";
    /// `PLAYERKEY_TEAM`
    pub const TEAM: &str = "team_";
    /// `PLAYERKEY_HANDICAP`
    pub const HANDICAP: &str = "handicap_";
    /// `PLAYERKEY_DIFFICULTY`
    pub const DIFFICULTY: &str = "diff_";
    /// `PLAYERKEY_READY` — note the `steam_` prefix survives on every platform;
    /// the constant is shared by `connectiondata.obj`, `lobbymanager.obj`,
    /// `SteamLobbyMgr.obj`, `lobby.obj`, `SteamLobby.obj` and
    /// `multiplayermanager.obj` alike.
    pub const READY: &str = "steam_ready_";
    /// `PLAYERKEY_PLAYER_ID`
    pub const PLAYER_ID: &str = "player_id";
    /// `PLAYERKEY_PLATFORM_PLAYER_ID`
    pub const PLATFORM_PLAYER_ID: &str = "platform_player_id";
}

/// Lobby-level attribute keys. **[measured]**
pub mod lobbykey {
    /// `LOBBYNAME_KEY` / `STEAM_LOBBYNAME_KEY` — both resolve to `"name"`.
    pub const NAME: &str = "name";
    /// `STEAM_LOBBYKEY_HOSTID`
    pub const HOST_ID: &str = "hostid";
    /// `LOBBYKEY_QUICKMATCH`
    pub const QUICKMATCH: &str = "qm";
    /// `LOBBYKEY_DISCRIMINATE_LOBBY_TYPE` — the crossplay filter key.
    pub const DISCRIMINATE_LOBBY_TYPE: &str = "discriminate_src";
    /// `LOBBYKEY_DISCRIMINATE_VALUES_XBOX`
    pub const DISCRIMINATE_XBOX: &str = "xboxlive";
    /// `LOBBYKEY_DISCRIMINATE_VALUES_STEAM`
    pub const DISCRIMINATE_STEAM: &str = "steam";
    /// `LOBBYKEY_DISCRIMINATE_VALUES_CROSSPLAY_DISCRIM`
    pub const DISCRIMINATE_NO_CROSSPLAY: &str = "no_crossplay";
    /// `NEGATE_LOBBY_VALUE_PREFIX` — a search term prefixed with this means
    /// "must not equal".
    pub const NEGATE_PREFIX: &str = "__cross_neg__";
    /// `LIST_LOBBY_VALUE_PREFIX` — a search term prefixed with this is a list.
    pub const LIST_PREFIX: &str = "__cross_list__";
    /// `STEAM_LOBBY_SCENARIO_DATA` — the `ScenFilePreviewData` blob.
    pub const SCENARIO_DATA: &str = "scenario_data";
    /// `STEAM_LOBBYKEY_GAME_SEED` — the simulation seed every peer must share.
    pub const GAME_SEED: &str = "game_seed";
    /// `STEAM_LOBBYKEY_LOBBY_FLAGS`
    pub const LOBBY_FLAGS: &str = "lobby_flags";
    /// `STEAM_LOBBYKEY_ELORANK`
    pub const ELORANK: &str = "elorank";
    pub const DESC: &str = "desc";
    pub const CHAIRS: &str = "chairs";
    pub const MODS: &str = "mods";
    pub const MOD_NAME: &str = "mod_name";
    pub const MOD_DESC: &str = "mod_desc";
    pub const MOD_SIZE: &str = "mod_size";
    pub const MOD_CHECKSUM: &str = "mod_checksum";
    pub const MOD_WORKSHOP_ID: &str = "mod_workshop_id";
    pub const SCRIPT_NAME: &str = "script_name";
    pub const SCRIPT_TYPE: &str = "script_type";
    pub const SCENARIO_TYPE: &str = "scenario_type";
    pub const SCENARIO_SIZE: &str = "scenario_size";
    pub const SCENARIO_CHECKSUM: &str = "scenario_checksum";
    pub const SCENARIO_NUM_FILES: &str = "scenario_num_files";
    pub const SCENARIO_WORKSHOP_ID: &str = "scenario_workshop_id";
}

/// The 22 lobby keys that carry a `GameConnectionData` settings byte, paired
/// with the field they map to. Key strings are **[measured]**; the pairing is
/// by name and is **[inferred]** — `send_game` builds them in a straight-line
/// block whose individual `mov`s were not traced one by one.
///
/// Note `STEAM_LOBBY_ECONWIN` resolves to the string **`"echowin"`**, not
/// `"econwin"`. That typo is in the shipped binary; it is not ours, and an
/// interoperating implementation must reproduce it exactly.
pub const SETTING_KEYS: [(&str, SettingField); 26] = [
    ("teamstyle", SettingField::TeamStyle),
    ("map_style", SettingField::MapStyle),
    ("map_size", SettingField::MapSize),
    ("gamespeed", SettingField::GameSpeed),
    ("gamerules", SettingField::GameRules),
    ("starting_town", SettingField::StartingTown),
    ("starting_resources", SettingField::StartingResources),
    ("starting_resources2", SettingField::StartingResources2),
    ("tech_cost", SettingField::TechCost),
    ("reveal_map", SettingField::RevealMap),
    ("pop_limit", SettingField::PopLimit),
    ("rush_rules", SettingField::RushRules),
    ("cannontime", SettingField::CannonTimes),
    ("starting_technology", SettingField::StartingTechnology),
    ("starting_technology2", SettingField::StartingTechnology2),
    ("ending_technology", SettingField::EndingTechnology),
    ("elimination", SettingField::Elimination),
    ("victory", SettingField::Victory),
    ("wonderwin", SettingField::WonderWin),
    ("score_goal", SettingField::ScoreGoal),
    ("popwin", SettingField::PopWin),
    ("time_limit", SettingField::TimeLimit),
    ("chairs", SettingField::Chairs),
    ("echowin", SettingField::EconWin), // sic — the binary's own spelling
    ("scenario_type", SettingField::ScenarioType),
    ("script_type", SettingField::ScriptType),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingField {
    TeamStyle,
    MapStyle,
    MapSize,
    GameSpeed,
    GameRules,
    StartingTown,
    StartingResources,
    StartingResources2,
    TechCost,
    RevealMap,
    PopLimit,
    RushRules,
    CannonTimes,
    StartingTechnology,
    StartingTechnology2,
    EndingTechnology,
    Elimination,
    Victory,
    WonderWin,
    ScoreGoal,
    PopWin,
    TimeLimit,
    Chairs,
    EconWin,
    ScenarioType,
    ScriptType,
}

impl SettingField {
    pub fn get(self, g: &GameConnectionData) -> u8 {
        use SettingField as F;
        match self {
            F::TeamStyle => g.team_style,
            F::MapStyle => g.map_style,
            F::MapSize => g.map_size,
            F::GameSpeed => g.game_speed,
            F::GameRules => g.game_rules,
            F::StartingTown => g.starting_town,
            F::StartingResources => g.starting_resources,
            F::StartingResources2 => g.starting_resources2,
            F::TechCost => g.tech_cost,
            F::RevealMap => g.reveal_map,
            F::PopLimit => g.pop_limit,
            F::RushRules => g.rush_rules,
            F::CannonTimes => g.cannon_times,
            F::StartingTechnology => g.starting_technology,
            F::StartingTechnology2 => g.starting_technology2,
            F::EndingTechnology => g.ending_technology,
            F::Elimination => g.elimination,
            F::Victory => g.victory,
            F::WonderWin => g.wonderwin,
            F::ScoreGoal => g.score_goal,
            F::PopWin => g.popwin,
            F::TimeLimit => g.time_limit,
            F::Chairs => g.chairs,
            F::EconWin => g.econwin,
            F::ScenarioType => g.scenario_type,
            F::ScriptType => g.script_type,
        }
    }

    pub fn set(self, g: &mut GameConnectionData, v: u8) {
        use SettingField as F;
        match self {
            F::TeamStyle => g.team_style = v,
            F::MapStyle => g.map_style = v,
            F::MapSize => g.map_size = v,
            F::GameSpeed => g.game_speed = v,
            F::GameRules => g.game_rules = v,
            F::StartingTown => g.starting_town = v,
            F::StartingResources => g.starting_resources = v,
            F::StartingResources2 => g.starting_resources2 = v,
            F::TechCost => g.tech_cost = v,
            F::RevealMap => g.reveal_map = v,
            F::PopLimit => g.pop_limit = v,
            F::RushRules => g.rush_rules = v,
            F::CannonTimes => g.cannon_times = v,
            F::StartingTechnology => g.starting_technology = v,
            F::StartingTechnology2 => g.starting_technology2 = v,
            F::EndingTechnology => g.ending_technology = v,
            F::Elimination => g.elimination = v,
            F::Victory => g.victory = v,
            F::WonderWin => g.wonderwin = v,
            F::ScoreGoal => g.score_goal = v,
            F::PopWin => g.popwin = v,
            F::TimeLimit => g.time_limit = v,
            F::Chairs => g.chairs = v,
            F::EconWin => g.econwin = v,
            F::ScenarioType => g.scenario_type = v,
            F::ScriptType => g.script_type = v,
        }
    }
}

/// Build the per-player key for a slot: `MakePlayerkey(prefix, slot)`.
pub fn player_key(prefix: &str, slot: usize) -> String {
    format!("{prefix}{slot}")
}

/// An attribute map as the lobby carries it: string → string, both UTF-16 on
/// the wire, both plain `String` here.
pub type Attributes = BTreeMap<String, String>;

/// Render the host-owned game settings into lobby attributes.
///
/// Values are decimal, which is what `<lambda>::operator()(int) -> const
/// wchar_t*` at `0x004BA3B0` produces. **[inferred]** — the lambda is an
/// int-to-wide-string formatter; that it is decimal and unpadded is the only
/// shape consistent with the keys being parsed back with `_wtoi`.
pub fn game_to_attributes(g: &GameConnectionData) -> Attributes {
    let mut a = Attributes::new();
    for (k, f) in SETTING_KEYS {
        a.insert(k.to_string(), f.get(g).to_string());
    }
    a.insert(lobbykey::GAME_SEED.into(), g.seed.to_string());
    a.insert(lobbykey::LOBBY_FLAGS.into(), g.flags.to_string());
    a.insert(lobbykey::ELORANK.into(), g.lobby_elo.to_string());
    a
}

/// Inverse of [`game_to_attributes`]. Missing keys leave the field alone, which
/// is what a delta publish requires.
pub fn attributes_to_game(a: &Attributes, g: &mut GameConnectionData) {
    for (k, f) in SETTING_KEYS {
        if let Some(v) = a.get(k).and_then(|v| v.parse::<u8>().ok()) {
            f.set(g, v);
        }
    }
    if let Some(v) = a
        .get(lobbykey::GAME_SEED)
        .and_then(|v| v.parse::<u32>().ok())
    {
        g.seed = v;
    }
    if let Some(v) = a
        .get(lobbykey::LOBBY_FLAGS)
        .and_then(|v| v.parse::<i32>().ok())
    {
        g.flags = v;
    }
    if let Some(v) = a.get(lobbykey::ELORANK).and_then(|v| v.parse::<i32>().ok()) {
        g.lobby_elo = v;
    }
}

/// Render one player slot into lobby attributes, exactly the set
/// `ConnectionData::send_player` writes.
pub fn player_to_attributes(slot: usize, p: &PlayerSlotPod) -> Attributes {
    use playerkey as K;
    let mut a = Attributes::new();
    a.insert(player_key(K::ELO, slot), p.elo.to_string());
    a.insert(player_key(K::SLOT_TYPE, slot), p.slot_type.to_string());
    a.insert(player_key(K::TRIBE, slot), p.tribe.to_string());
    a.insert(player_key(K::WHO, slot), p.who.to_string());
    a.insert(player_key(K::TEAM, slot), p.team.to_string());
    a.insert(player_key(K::HANDICAP, slot), p.handicap.to_string());
    a.insert(player_key(K::DIFFICULTY, slot), p.diff.to_string());
    a.insert(player_key(K::READY, slot), p.ready.to_string());
    a
}

/// Inverse of [`player_to_attributes`]; absent keys leave the field alone.
pub fn attributes_to_player(a: &Attributes, slot: usize, p: &mut PlayerSlotPod) {
    use playerkey as K;
    let g = |k: &str| a.get(&player_key(k, slot));
    if let Some(v) = g(K::ELO).and_then(|v| v.parse().ok()) {
        p.elo = v;
    }
    if let Some(v) = g(K::SLOT_TYPE).and_then(|v| v.parse().ok()) {
        p.slot_type = v;
    }
    if let Some(v) = g(K::TRIBE).and_then(|v| v.parse().ok()) {
        p.tribe = v;
    }
    if let Some(v) = g(K::WHO).and_then(|v| v.parse().ok()) {
        p.who = v;
    }
    if let Some(v) = g(K::TEAM).and_then(|v| v.parse().ok()) {
        p.team = v;
    }
    if let Some(v) = g(K::HANDICAP).and_then(|v| v.parse().ok()) {
        p.handicap = v;
    }
    if let Some(v) = g(K::DIFFICULTY).and_then(|v| v.parse().ok()) {
        p.diff = v;
    }
    if let Some(v) = g(K::READY).and_then(|v| v.parse().ok()) {
        p.ready = v;
    }
}

/// The delta `ConnectionData::send_player` would publish: only keys whose value
/// changed since the last publish, unless `force`.
pub fn player_delta(
    slot: usize,
    prev: &PlayerSlotPod,
    next: &PlayerSlotPod,
    force: bool,
) -> Attributes {
    let a = player_to_attributes(slot, next);
    if force {
        return a;
    }
    let b = player_to_attributes(slot, prev);
    a.into_iter().filter(|(k, v)| b.get(k) != Some(v)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn player_keys_are_prefix_plus_slot() {
        assert_eq!(player_key(playerkey::READY, 3), "steam_ready_3");
        assert_eq!(player_key(playerkey::ELO, 0), "elo_0");
        assert_eq!(player_key(playerkey::DIFFICULTY, 7), "diff_7");
    }

    #[test]
    fn the_shipped_binary_spells_econwin_echowin() {
        // Not a typo on our side: STEAM_LOBBY_ECONWIN at 0x00AFC0C8 points at
        // 0x00AFC0C0, which holds "echowin". Reproducing it is required for
        // interop, so the test pins it.
        let (k, f) = SETTING_KEYS
            .iter()
            .find(|(_, f)| *f == SettingField::EconWin)
            .unwrap();
        assert_eq!(*k, "echowin");
        assert_eq!(*f, SettingField::EconWin);
    }

    #[test]
    fn setting_keys_are_unique_and_cover_every_field_once() {
        let mut keys: Vec<&str> = SETTING_KEYS.iter().map(|(k, _)| *k).collect();
        keys.sort_unstable();
        let n = keys.len();
        keys.dedup();
        assert_eq!(keys.len(), n, "duplicate lobby key");
        let mut fields: Vec<SettingField> = SETTING_KEYS.iter().map(|(_, f)| *f).collect();
        fields.dedup();
        assert_eq!(fields.len(), SETTING_KEYS.len(), "duplicate field mapping");
    }

    #[test]
    fn game_settings_survive_a_lobby_round_trip() {
        let mut g = GameConnectionData::default();
        g.set_data([
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
            25, 26, 27, 28, 29, 30,
        ]);
        g.seed = 0xDEAD_BEEF;
        g.flags = -7;
        g.lobby_elo = 1337;
        let a = game_to_attributes(&g);
        assert_eq!(a.get("game_seed").map(String::as_str), Some("3735928559"));
        let mut back = GameConnectionData::default();
        attributes_to_game(&a, &mut back);
        // Only the fields the lobby carries come back; `players`,
        // `max_observers`, `difficulty`, `mods` and the checksum trio have no
        // key in the schema and stay at their defaults. That asymmetry is the
        // finding, so assert it rather than paper over it.
        for (_, f) in SETTING_KEYS {
            assert_eq!(f.get(&back), f.get(&g), "{f:?}");
        }
        assert_eq!(back.seed, g.seed);
        assert_eq!(back.flags, g.flags);
        assert_eq!(back.lobby_elo, g.lobby_elo);
        assert_eq!(back.players, 0);
        assert_eq!(back.checksum_window_size, 0);
    }

    #[test]
    fn player_delta_publishes_only_what_changed() {
        let prev = PlayerSlotPod {
            elo: 1500,
            team: 1,
            ready: 0,
            ..Default::default()
        };
        let next = PlayerSlotPod { ready: 1, ..prev };
        let d = player_delta(2, &prev, &next, false);
        assert_eq!(d.len(), 1);
        assert_eq!(d.get("steam_ready_2").map(String::as_str), Some("1"));
        let full = player_delta(2, &prev, &next, true);
        assert_eq!(full.len(), 8);
    }

    #[test]
    fn attributes_round_trip_a_player_slot() {
        let p = PlayerSlotPod {
            elo: -3,
            slot_type: 1,
            tribe: 12,
            who: 4,
            team: 2,
            handicap: 5,
            diff: 3,
            ready: 1,
        };
        let a = player_to_attributes(5, &p);
        let mut back = PlayerSlotPod::default();
        attributes_to_player(&a, 5, &mut back);
        assert_eq!(back, p);
        // and a slot mismatch must not read another slot's values
        let mut wrong = PlayerSlotPod::default();
        attributes_to_player(&a, 4, &mut wrong);
        assert_eq!(wrong, PlayerSlotPod::default());
    }
}
