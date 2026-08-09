// SPDX-License-Identifier: GPL-3.0-or-later
//
// This path import is intentional.  The setup lane owns no shared module-list file; the
// convergence owner can expose `systems::setup_diplomacy` after adapting the authoritative
// leader store.  These tests keep the bounded retail decision tree executable meanwhile.
#[path = "../src/systems/setup_diplomacy.rs"]
mod setup_diplomacy;

use setup_diplomacy::{
    IsTeamArg, LeaderTeamState, PlayerSetup, SetupDiplomacy, TeamQueryError, DIPLO_ALLY,
    SETUP_SLOTS, TEAM_AUTO, TEAM_STYLE_NEUTRAL,
};

fn image() -> SetupDiplomacy {
    let mut image = SetupDiplomacy::default();
    for slot in 0..SETUP_SLOTS {
        image.players[slot] = PlayerSetup {
            flags: 1,
            who: slot as u8,
            team: slot as i8,
        };
        image.leaders[slot] = LeaderTeamState {
            leader_flags: 1,
            who: slot as i32,
            diplos: [0; SETUP_SLOTS],
        };
    }
    image
}

fn declare_alliance(image: &mut SetupDiplomacy, a: usize, b: usize) {
    image.leaders[a].diplos[b] = DIPLO_ALLY;
    image.leaders[b].diplos[a] = DIPLO_ALLY;
}

#[test]
fn get_player_retains_the_last_deferred_match_but_prefers_a_later_clean_match() {
    let mut image = image();
    image.leaders[3].who = 41;
    image.players[0].flags = 0;
    image.players[1] = PlayerSetup {
        flags: 1 | 0x10,
        who: 41,
        team: 1,
    };
    image.players[2] = PlayerSetup {
        flags: 1 | 0x40,
        who: 41,
        team: 2,
    };
    image.players[4] = PlayerSetup {
        flags: 1,
        who: 41,
        team: 3,
    };

    assert_eq!(image.player_index_for_leader(3), Ok(4));
    image.players[4].flags = 0;
    assert_eq!(image.player_index_for_leader(3), Ok(2));
    image.players[2].flags = 0;
    image.players[1].flags = 0;
    assert_eq!(image.player_index_for_leader(3), Ok(0));
}

#[test]
fn get_team_sign_extends_configured_bytes_and_fails_absence_to_raw_auto() {
    let mut image = image();
    image.players[3].team = -2;
    assert_eq!(image.team_of(3), Ok(-2));

    image.players[3].flags = 0;
    image.players[0].flags = 0;
    assert_eq!(image.team_of(3), Ok(i32::from(TEAM_AUTO)));
}

#[test]
fn auto_team_chooses_the_first_runtime_ally_but_the_semaphore_preserves_eight() {
    let mut image = image();
    image.players[5].team = TEAM_AUTO;
    image.frame = 1;
    declare_alliance(&mut image, 5, 2);
    assert_eq!(image.team_of(5), Ok(2), "slot order is first-wins");

    image.semaphore_820 = 0x80;
    assert_eq!(image.team_of(5), Ok(i32::from(TEAM_AUTO)));
}

#[test]
fn auto_team_at_frame_zero_resolves_to_self_before_later_candidates() {
    let mut image = image();
    image.players[3].team = TEAM_AUTO;
    assert_eq!(image.team_of(3), Ok(3));
}

#[test]
fn frame_zero_uses_configured_teams_while_runtime_zero_mode_uses_diplomacy() {
    let mut image = image();
    image.players[1].team = 2;
    image.players[4].team = 2;
    assert_eq!(image.is_team(1, 4, IsTeamArg::Zero), Ok(true));

    image.frame = 1;
    assert_eq!(image.is_team(1, 4, IsTeamArg::Zero), Ok(false));
    image.leaders[1].diplos[4] = DIPLO_ALLY;
    assert_eq!(
        image.is_team(1, 4, IsTeamArg::Zero),
        Ok(false),
        "a unilateral alliance is not a retail alliance"
    );
    image.leaders[4].diplos[1] = DIPLO_ALLY;
    assert_eq!(image.is_team(1, 4, IsTeamArg::Zero), Ok(true));
}

#[test]
fn nonzero_mode_retains_configured_team_except_in_three_alliance_gated_styles() {
    for team_style in [1u8, 2, 3, 4, 5, 6, 7, 9, 10, 12] {
        let mut image = image();
        image.players[1].team = 0;
        image.players[2].team = 0;
        image.frame = 9;
        image.team_style = team_style;
        assert_eq!(
            image.is_team(1, 2, IsTeamArg::NonZero),
            Ok(true),
            "team style {team_style}"
        );
    }

    for team_style in [0u8, 8, 11] {
        let mut image = image();
        image.players[1].team = 0;
        image.players[2].team = 0;
        image.frame = 9;
        image.team_style = team_style;
        assert_eq!(
            image.is_team(1, 2, IsTeamArg::NonZero),
            Ok(false),
            "team style {team_style}"
        );
        declare_alliance(&mut image, 1, 2);
        assert_eq!(image.is_team(1, 2, IsTeamArg::NonZero), Ok(true));
    }
}

#[test]
fn neutral_style_rejects_distinct_auto_players_before_diplomacy() {
    let mut image = image();
    image.team_style = TEAM_STYLE_NEUTRAL;
    image.players[1].team = TEAM_AUTO;
    image.players[2].team = TEAM_AUTO;
    declare_alliance(&mut image, 1, 2);
    assert_eq!(image.is_team(1, 2, IsTeamArg::Zero), Ok(false));
    assert_eq!(image.is_team(1, 1, IsTeamArg::Zero), Ok(true));
}

#[test]
fn configured_team_domain_is_signed_zero_through_three_only() {
    for team in [-128i8, -1, 4, 7, TEAM_AUTO, 127] {
        let mut image = image();
        image.players[1].team = team;
        image.players[2].team = team;
        assert_eq!(
            image.is_team(1, 2, IsTeamArg::NonZero),
            Ok(false),
            "team byte {team}"
        );
    }
}

#[test]
fn invalid_product_indices_fail_closed_instead_of_aliasing_storage() {
    let mut image = image();
    image.leaders[3].who = 99;
    assert_eq!(
        image.is_ally(3, 2),
        Err(TeamQueryError::LeaderWhoOutOfRange { slot: 3, who: 99 })
    );
    assert_eq!(
        image.team_of(SETUP_SLOTS),
        Err(TeamQueryError::LeaderSlotOutOfRange { slot: SETUP_SLOTS })
    );
}
