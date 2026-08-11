// SPDX-License-Identifier: GPL-3.0-or-later
//! Replay-owned, setup-time prefix of checksum channel 8 (`leaders`).
//!
//! This is deliberately a sparse producer. `LeaderData::walk_data` (`0x006D6750`)
//! walks the eight-byte header of every row and, for a valid row, a 26,914-byte
//! fixed body plus several dynamic children. A replay setup does not determine all
//! of that state. This module exposes only bytes written by the shipped setup path
//! whose values are independent of the unresolved team/RNG/call-order tails.
//!
//! In particular, zero-filled internal storage is not a claim. Consumers must use
//! [`InitialLeaderRow::owned_spans`] and [`InitialLeaderRow::owned_slice`] to merge
//! proven slices into a complete channel owner.

#![forbid(unsafe_code)]

use crate::initial::{InitialState, ReplayByteSpan};
use don_sim::systems::leader_init_diplomacy_loop::{
    init_diplomacy_loop, LeaderInitDiplomacyFacts, LeaderInitDiplomacyLoopError,
    LeaderInitDiplomacyLoopImage, LeaderInitDiplomacyLoopRequest, LeaderInitDiplomacyRow,
};
use don_sim::systems::setup_diplomacy::{
    LeaderTeamState, PlayerSetup, SetupDiplomacy, PLAYER_PRESENT, SETUP_SLOTS,
};

pub const LEADER_WALK_DATA_VA: u32 = 0x006d_6750;
pub const LEADERS_CTOR_VA: u32 = 0x006e_d900;
pub const LEADERS_INIT_VA: u32 = 0x006e_d850;
pub const LEADER_INIT_VA: u32 = 0x006e_3930;
pub const GAME_INIT_RULES_AND_TEAMS_VA: u32 = 0x0058_9bb0;
pub const SETUP_BUILD_GAME_VA: u32 = 0x005a_c190;

/// Rows visited by `CheckSums::check_leaders`. Retail constructs ten Leader
/// objects, but the checksum loop visits exactly the first eight.
pub const CHECKSUM_LEADER_SLOTS: usize = SETUP_SLOTS;
/// `LeaderData::walk_data`'s fixed active-row interval is `[8, 0x692a)`.
pub const FIXED_WALK_END: usize = 0x692a;

pub const FLAGS_LOW_BEGIN: usize = 0;
pub const FLAGS_LOW_END: usize = 1;
pub const IDENTITY_BEGIN: usize = 8;
pub const IDENTITY_END: usize = 24;
pub const DIPLOS_BEGIN: usize = 116;
pub const TREATIES_BEGIN: usize = 148;
pub const INVARIANT_OPENING_BEGIN: usize = 180;
pub const INVARIANT_OPENING_END: usize = 500;
pub const AGGRESSION_BEGIN: usize = 528;
pub const INVARIANT_CLOSING_BEGIN: usize = 560;
pub const INVARIANT_CLOSING_END: usize = 916;

/// One inactive row contributes its low flag byte. An active row contributes
/// that byte plus 704 setup-invariant body bytes.
pub const INACTIVE_OWNED_BYTES: usize = 1;
pub const ACTIVE_OWNED_BYTES: usize = 705;

const PLAYER_HUMAN: u16 = 0x0004;
const LEADER_VALID_ACTIVE: u8 = 0x03;
const LEADER_HUMAN: u8 = 0x04;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaderPrefixProvenance {
    /// Low-byte stores in the network/recording arm of
    /// `Game::init_rules_and_teams`.
    InitRulesAndTeamsFlags,
    /// `who`, `tribe`, `defeated_by`, and `gov` stores in `Leader::init`.
    LeaderInitIdentity,
    /// A self cell which is invariant under team assignment and active-row order.
    LeaderInitSelfDiplomacy,
    /// Unconditional reset stores in the recovered eight-target initialization loop.
    LeaderInitDiplomacyReset,
}

/// A checksum-relative owned interval within one `LeaderData` row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LeaderOwnedSpan {
    pub begin: usize,
    pub end: usize,
    pub provenance: LeaderPrefixProvenance,
}

impl LeaderOwnedSpan {
    pub const fn bytes(self) -> usize {
        self.end - self.begin
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LeaderPlayerSource {
    pub flag_gate: ReplayByteSpan,
    pub body: Option<ReplayByteSpan>,
}

/// Replay evidence retained by the sparse producer. The parser does not yet
/// export a byte span for `Game::semaphore`; the complete payload hash binds
/// that observed value without pretending a more local source range exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaderPrefixSourceEvidence {
    pub payload_sha256: [u8; 32],
    pub players: [LeaderPlayerSource; CHECKSUM_LEADER_SLOTS],
    pub semaphore: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitialLeaderRow {
    pub slot: u8,
    pub active: bool,
    pub human: bool,
    pub tribe: Option<u8>,
    pub setup_player_slot: Option<u8>,
    fixed_image: Vec<u8>,
    owned_spans: Vec<LeaderOwnedSpan>,
}

impl InitialLeaderRow {
    pub fn owned_spans(&self) -> &[LeaderOwnedSpan] {
        &self.owned_spans
    }

    /// Return bytes only for a span issued by this row's ownership ledger.
    /// A caller-constructed span cannot expose zero-filled unowned storage.
    pub fn owned_slice(&self, span: LeaderOwnedSpan) -> Option<&[u8]> {
        self.owned_spans
            .contains(&span)
            .then(|| &self.fixed_image[span.begin..span.end])
    }

    pub fn claimed_walked_bytes(&self) -> usize {
        self.owned_spans.iter().map(|span| span.bytes()).sum()
    }
}

/// Exact setup-time bytes, not a complete leaders checksum producer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitialLeaderPrefix {
    pub rows: [InitialLeaderRow; CHECKSUM_LEADER_SLOTS],
    pub active_mask: u8,
    pub human_mask: u8,
    pub nonhuman_mask: u8,
    pub sources: LeaderPrefixSourceEvidence,
}

impl InitialLeaderPrefix {
    /// `8 + 704 * active_leaders` for this ownership revision.
    pub fn claimed_walked_bytes(&self) -> usize {
        self.rows
            .iter()
            .map(InitialLeaderRow::claimed_walked_bytes)
            .sum()
    }

    /// This is only an expiration classifier. `true` means no setup AI can
    /// mutate the frozen image before the first turn; it does not prove that
    /// player commands or an unowned setup body left every claimed byte alone.
    pub const fn is_human_only_first_checksum_candidate(&self) -> bool {
        self.nonhuman_mask == 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InitialLeaderPrefixError {
    InitialFrameNotZero {
        frame: i32,
    },
    MissingRecordingSemaphore,
    NotNetworkOrRecordingPath {
        semaphore_820: u8,
    },
    PlayerCount {
        count: usize,
    },
    PlayerSlotMismatch {
        index: usize,
        slot: u8,
    },
    PlayerPresenceMismatch {
        slot: usize,
        present: bool,
        flags: u16,
    },
    PlayerBodySpanMismatch {
        slot: usize,
        present: bool,
        body: Option<ReplayByteSpan>,
    },
    PlayerWhoOutOfRange {
        player_slot: usize,
        who: u8,
    },
    LeaderInit(LeaderInitDiplomacyLoopError),
}

impl From<LeaderInitDiplomacyLoopError> for InitialLeaderPrefixError {
    fn from(value: LeaderInitDiplomacyLoopError) -> Self {
        Self::LeaderInit(value)
    }
}

impl std::fmt::Display for InitialLeaderPrefixError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InitialFrameNotZero { frame } => {
                write!(f, "replay setup frame is {frame}, expected zero")
            }
            Self::MissingRecordingSemaphore => write!(f, "Game::semaphore has no first byte"),
            Self::NotNetworkOrRecordingPath { semaphore_820 } => write!(
                f,
                "Game+0x820 is 0x{semaphore_820:02x}; bit 0x04 does not select the replay setup arm"
            ),
            Self::PlayerCount { count } => write!(f, "replay has {count} Player rows, expected 8"),
            Self::PlayerSlotMismatch { index, slot } => {
                write!(f, "Player vector index {index} carries slot {slot}")
            }
            Self::PlayerPresenceMismatch {
                slot,
                present,
                flags,
            } => write!(
                f,
                "Player[{slot}] present={present} disagrees with flags 0x{flags:04x}"
            ),
            Self::PlayerBodySpanMismatch {
                slot,
                present,
                body,
            } => write!(
                f,
                "Player[{slot}] present={present} has incompatible body span {body:?}"
            ),
            Self::PlayerWhoOutOfRange { player_slot, who } => {
                write!(f, "Player[{player_slot}].who={who} is outside 0..8")
            }
            Self::LeaderInit(error) => write!(f, "recovered Leader::init loop refused: {error:?}"),
        }
    }
}

impl std::error::Error for InitialLeaderPrefixError {}

#[derive(Debug, Clone, Copy)]
struct SetupPlayerInput {
    flags: u16,
    tribe: u8,
    who: u8,
    team: i8,
}

#[derive(Debug, Clone)]
struct SetupInput {
    players: [SetupPlayerInput; CHECKSUM_LEADER_SLOTS],
    team_style: u8,
    frame: i32,
    semaphore: Vec<u8>,
    facts: LeaderInitDiplomacyFacts,
}

fn put_i32(image: &mut [u8], offset: usize, value: i32) {
    image[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_i32s(image: &mut [u8], offset: usize, values: &[i32]) {
    for (index, value) in values.iter().copied().enumerate() {
        put_i32(image, offset + index * 4, value);
    }
}

fn image_diplomacy_row(
    image: &mut [u8],
    setup: &SetupDiplomacy,
    slot: usize,
    row: &LeaderInitDiplomacyRow,
) {
    put_i32s(image, DIPLOS_BEGIN, &setup.leaders[slot].diplos);
    for (offset, values) in [
        (TREATIES_BEGIN, &row.treaties),
        (180, &row.agendas),
        (212, &row.good_deeds),
        (244, &row.attack_stamp),
        (276, &row.raid_stamp),
        (308, &row.capital_stamp),
        (340, &row.ally_stamp),
        (372, &row.tribute_stamp),
        (404, &row.gift_stamp),
        (436, &row.hire_stamp),
        (468, &row.hire_who),
        (AGGRESSION_BEGIN, &row.aggression),
        (560, &row.strong),
        (592, &row.weak),
        (624, &row.dow),
        (656, &row.invaders),
        (688, &row.broke_alliance),
        (720, &row.made_peace),
    ] {
        put_i32s(image, offset, values);
    }
    put_i32(image, 752, row.got_diplo_message);
    for (offset, values) in [
        (756, &row.last_spoke),
        (788, &row.counteroffer),
        (820, &row.tribute_demanded),
        (852, &row.last_taunt),
        (884, &row.taunt_frame),
    ] {
        put_i32s(image, offset, values);
    }
}

fn derive_model(
    input: &SetupInput,
    active_order: &[usize],
    shared_vision_preq_mask: u8,
) -> Result<([InitialLeaderRow; CHECKSUM_LEADER_SLOTS], u8, u8), InitialLeaderPrefixError> {
    let mut setup = SetupDiplomacy {
        players: std::array::from_fn(|slot| PlayerSetup {
            flags: input.players[slot].flags,
            who: input.players[slot].who,
            team: input.players[slot].team,
        }),
        leaders: std::array::from_fn(|slot| LeaderTeamState {
            leader_flags: 0,
            who: slot as i32,
            diplos: [0; SETUP_SLOTS],
        }),
        team_style: input.team_style,
        frame: input.frame,
        semaphore_820: input.semaphore[0],
    };

    // `Leaders::init` first calls `Leader::init(slot, -1, slot)` for all rows.
    // The negative-tribe arm is independent of the not-yet-active roster.
    let mut diplomacy_rows: [LeaderInitDiplomacyRow; CHECKSUM_LEADER_SLOTS] =
        std::array::from_fn(|_| LeaderInitDiplomacyRow::default());
    for slot in 0..CHECKSUM_LEADER_SLOTS {
        let mut image = LeaderInitDiplomacyLoopImage {
            setup,
            row: diplomacy_rows[slot].clone(),
        };
        init_diplomacy_loop(
            &mut image,
            LeaderInitDiplomacyLoopRequest {
                receiver_slot: slot,
                tribe: -1,
            },
            LeaderInitDiplomacyFacts::default(),
        )?;
        setup = image.setup;
        diplomacy_rows[slot] = image.row;
    }

    // Exact duplicate-who behavior of the network/recording Player scan:
    // the first row activates and supplies tribe; a later non-human duplicate
    // clears HUMAN but does not replace tribe or the owning setup row.
    let mut leader_tribes = [None; CHECKSUM_LEADER_SLOTS];
    let mut setup_player_slots = [None; CHECKSUM_LEADER_SLOTS];
    let mut human_mask = 0u8;
    for (player_slot, player) in input.players.iter().copied().enumerate() {
        if player.flags & PLAYER_PRESENT == 0 {
            continue;
        }
        let who = usize::from(player.who);
        if setup.leaders[who].leader_flags & 1 == 0 {
            setup.leaders[who].leader_flags = 3;
            leader_tribes[who] = Some(player.tribe);
            setup_player_slots[who] = Some(player_slot as u8);
            if player.flags & PLAYER_HUMAN != 0 {
                setup.leaders[who].leader_flags |= i32::from(LEADER_HUMAN);
                human_mask |= 1u8 << who;
            }
        } else if player.flags & PLAYER_HUMAN == 0 {
            setup.leaders[who].leader_flags &= !i32::from(LEADER_HUMAN);
            human_mask &= !(1u8 << who);
        }
    }
    let active_mask = setup
        .leaders
        .iter()
        .enumerate()
        .fold(0u8, |mask, (slot, leader)| {
            mask | (u8::from(leader.leader_flags & 1 != 0) << slot)
        });

    // `Setup::build_game` invokes active `Leader::init` in an RNG-derived order.
    // We execute an explicit order to exercise the recovered body, then publish
    // only fields proven invariant under order and shared-vision prerequisites.
    for &slot in active_order {
        if slot >= CHECKSUM_LEADER_SLOTS || active_mask & (1u8 << slot) == 0 {
            continue;
        }
        let facts = LeaderInitDiplomacyFacts {
            has_shared_vision_preq: shared_vision_preq_mask & (1u8 << slot) != 0,
            ..input.facts
        };
        let mut image = LeaderInitDiplomacyLoopImage {
            setup,
            row: diplomacy_rows[slot].clone(),
        };
        init_diplomacy_loop(
            &mut image,
            LeaderInitDiplomacyLoopRequest {
                receiver_slot: slot,
                tribe: i32::from(leader_tribes[slot].expect("active row has a tribe")),
            },
            facts,
        )?;
        setup = image.setup;
        diplomacy_rows[slot] = image.row;
    }

    let rows = std::array::from_fn(|slot| {
        let active = active_mask & (1u8 << slot) != 0;
        let human = human_mask & (1u8 << slot) != 0;
        let mut fixed_image = vec![0u8; FIXED_WALK_END];
        fixed_image[0] = if active {
            LEADER_VALID_ACTIVE | if human { LEADER_HUMAN } else { 0 }
        } else {
            0
        };
        let mut owned_spans = vec![LeaderOwnedSpan {
            begin: FLAGS_LOW_BEGIN,
            end: FLAGS_LOW_END,
            provenance: LeaderPrefixProvenance::InitRulesAndTeamsFlags,
        }];
        if active {
            put_i32(&mut fixed_image, 8, slot as i32);
            put_i32(
                &mut fixed_image,
                12,
                i32::from(leader_tribes[slot].expect("active row has a tribe")),
            );
            put_i32(&mut fixed_image, 16, -1);
            put_i32(&mut fixed_image, 20, -1);
            image_diplomacy_row(&mut fixed_image, &setup, slot, &diplomacy_rows[slot]);
            let self_cell = slot * 4;
            owned_spans.extend_from_slice(&[
                LeaderOwnedSpan {
                    begin: IDENTITY_BEGIN,
                    end: IDENTITY_END,
                    provenance: LeaderPrefixProvenance::LeaderInitIdentity,
                },
                LeaderOwnedSpan {
                    begin: DIPLOS_BEGIN + self_cell,
                    end: DIPLOS_BEGIN + self_cell + 4,
                    provenance: LeaderPrefixProvenance::LeaderInitSelfDiplomacy,
                },
                LeaderOwnedSpan {
                    begin: TREATIES_BEGIN + self_cell,
                    end: TREATIES_BEGIN + self_cell + 4,
                    provenance: LeaderPrefixProvenance::LeaderInitSelfDiplomacy,
                },
                LeaderOwnedSpan {
                    begin: INVARIANT_OPENING_BEGIN,
                    end: INVARIANT_OPENING_END,
                    provenance: LeaderPrefixProvenance::LeaderInitDiplomacyReset,
                },
                LeaderOwnedSpan {
                    begin: AGGRESSION_BEGIN + self_cell,
                    end: AGGRESSION_BEGIN + self_cell + 4,
                    provenance: LeaderPrefixProvenance::LeaderInitSelfDiplomacy,
                },
                LeaderOwnedSpan {
                    begin: INVARIANT_CLOSING_BEGIN,
                    end: INVARIANT_CLOSING_END,
                    provenance: LeaderPrefixProvenance::LeaderInitDiplomacyReset,
                },
            ]);
            owned_spans.sort_by_key(|span| span.begin);
        }
        InitialLeaderRow {
            slot: slot as u8,
            active,
            human,
            tribe: leader_tribes[slot],
            setup_player_slot: setup_player_slots[slot],
            fixed_image,
            owned_spans,
        }
    });
    Ok((rows, active_mask, human_mask))
}

/// Derive the exact setup-owned Leader prefix from a parsed replay.
pub fn derive(initial: &InitialState) -> Result<InitialLeaderPrefix, InitialLeaderPrefixError> {
    if initial.game.frame != 0 {
        return Err(InitialLeaderPrefixError::InitialFrameNotZero {
            frame: initial.game.frame,
        });
    }
    let Some(&semaphore_820) = initial.game.semaphore.first() else {
        return Err(InitialLeaderPrefixError::MissingRecordingSemaphore);
    };
    if semaphore_820 & 0x04 == 0 {
        return Err(InitialLeaderPrefixError::NotNetworkOrRecordingPath { semaphore_820 });
    }
    if initial.info.players.len() != CHECKSUM_LEADER_SLOTS {
        return Err(InitialLeaderPrefixError::PlayerCount {
            count: initial.info.players.len(),
        });
    }

    for (slot, player) in initial.info.players.iter().enumerate() {
        if usize::from(player.slot) != slot {
            return Err(InitialLeaderPrefixError::PlayerSlotMismatch {
                index: slot,
                slot: player.slot,
            });
        }
        let flags_present = player.flags & PLAYER_PRESENT != 0;
        if player.present != flags_present {
            return Err(InitialLeaderPrefixError::PlayerPresenceMismatch {
                slot,
                present: player.present,
                flags: player.flags,
            });
        }
        let body = initial.worldgen_sources.player_bodies[slot];
        if body.is_some_and(|span| span.bytes != 0x39) || body.is_some() != player.present {
            return Err(InitialLeaderPrefixError::PlayerBodySpanMismatch {
                slot,
                present: player.present,
                body,
            });
        }
        if player.present && usize::from(player.who) >= CHECKSUM_LEADER_SLOTS {
            return Err(InitialLeaderPrefixError::PlayerWhoOutOfRange {
                player_slot: slot,
                who: player.who,
            });
        }
    }

    let players = std::array::from_fn(|slot| {
        let player = &initial.info.players[slot];
        SetupPlayerInput {
            flags: player.flags,
            tribe: player.tribe,
            who: player.who,
            team: player.team as i8,
        }
    });
    let facts = LeaderInitDiplomacyFacts {
        reveal_map: initial.info.settings.reveal_map,
        game_rules: initial.info.settings.game_rules,
        rush_rules: initial.info.settings.rush_rules,
        starting_technology: initial.info.settings.starting_technology,
        starting_technology2: initial.info.settings.starting_technology2,
        ending_technology: initial.info.settings.ending_technology,
        scenario_rules: initial
            .game
            .semaphore
            .get(2)
            .is_some_and(|byte| byte & 0x02 != 0),
        check_victory_mode: initial
            .game
            .semaphore
            .get(1)
            .is_some_and(|byte| byte & 0x02 != 0),
        // This result is not serialized. It affects only ally_mask, which is not owned.
        has_shared_vision_preq: false,
    };
    let input = SetupInput {
        players,
        team_style: initial.info.settings.team_style,
        frame: initial.game.frame,
        semaphore: initial.game.semaphore.clone(),
        facts,
    };
    let order: Vec<usize> = (0..CHECKSUM_LEADER_SLOTS).collect();
    let (rows, active_mask, human_mask) = derive_model(&input, &order, 0)?;
    let sources = LeaderPrefixSourceEvidence {
        payload_sha256: initial.payload_sha256,
        players: std::array::from_fn(|slot| LeaderPlayerSource {
            flag_gate: initial.worldgen_sources.player_flags[slot],
            body: initial.worldgen_sources.player_bodies[slot],
        }),
        semaphore: initial.game.semaphore.clone(),
    };
    Ok(InitialLeaderPrefix {
        rows,
        active_mask,
        human_mask,
        nonhuman_mask: active_mask & !human_mask,
        sources,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> SetupInput {
        let mut players = [SetupPlayerInput {
            flags: 0,
            tribe: 0,
            who: 0,
            team: 8,
        }; CHECKSUM_LEADER_SLOTS];
        for (slot, player) in players.iter_mut().enumerate() {
            player.who = slot as u8;
        }
        players[0] = SetupPlayerInput {
            flags: PLAYER_PRESENT | PLAYER_HUMAN,
            tribe: 3,
            who: 0,
            team: 0,
        };
        players[1] = SetupPlayerInput {
            flags: PLAYER_PRESENT,
            tribe: 7,
            who: 1,
            team: 0,
        };
        SetupInput {
            players,
            team_style: 1,
            frame: 0,
            semaphore: vec![0x04, 0, 0, 0],
            facts: LeaderInitDiplomacyFacts {
                reveal_map: 0,
                game_rules: 0,
                rush_rules: 0,
                starting_technology: 0,
                starting_technology2: 0,
                ending_technology: 7,
                scenario_rules: false,
                check_victory_mode: false,
                has_shared_vision_preq: false,
            },
        }
    }

    fn owned_projection(rows: &[InitialLeaderRow; CHECKSUM_LEADER_SLOTS]) -> Vec<Vec<u8>> {
        rows.iter()
            .map(|row| {
                row.owned_spans()
                    .iter()
                    .flat_map(|span| row.owned_slice(*span).unwrap().iter().copied())
                    .collect()
            })
            .collect()
    }

    #[test]
    fn ownership_is_disjoint_and_has_the_stated_byte_count() {
        let (rows, active, human) = derive_model(&input(), &[0, 1], 0).unwrap();
        assert_eq!(active, 0x03);
        assert_eq!(human, 0x01);
        for row in &rows {
            for pair in row.owned_spans().windows(2) {
                assert!(pair[0].end <= pair[1].begin);
            }
            assert_eq!(row.fixed_image.len(), FIXED_WALK_END);
            assert_eq!(
                row.claimed_walked_bytes(),
                if row.active {
                    ACTIVE_OWNED_BYTES
                } else {
                    INACTIVE_OWNED_BYTES
                }
            );
        }
        assert_eq!(
            rows.iter()
                .map(InitialLeaderRow::claimed_walked_bytes)
                .sum::<usize>(),
            8 + 704 * 2
        );
    }

    #[test]
    fn published_bytes_ignore_unserialized_shared_vision_and_shuffled_active_order() {
        let base = input();
        let (forward, _, _) = derive_model(&base, &[0, 1], 0).unwrap();
        let (reverse, _, _) = derive_model(&base, &[1, 0], 0xff).unwrap();
        assert_eq!(owned_projection(&forward), owned_projection(&reverse));
    }

    #[test]
    fn published_bytes_ignore_unresolved_team_assignment_but_identity_bites() {
        let base = input();
        let (a, _, _) = derive_model(&base, &[0, 1], 0).unwrap();
        let mut alternate = base.clone();
        alternate.team_style = 11;
        alternate.players[0].team = 8;
        alternate.players[1].team = 3;
        alternate.facts.rush_rules = 7;
        let (b, _, _) = derive_model(&alternate, &[1, 0], 0xff).unwrap();
        assert_eq!(owned_projection(&a), owned_projection(&b));

        alternate.players[1].tribe = 9;
        let (c, _, _) = derive_model(&alternate, &[1, 0], 0xff).unwrap();
        assert_ne!(owned_projection(&b), owned_projection(&c));
        assert_eq!(&c[1].fixed_image[12..16], &9i32.to_le_bytes());
    }
}
