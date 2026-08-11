// SPDX-License-Identifier: GPL-3.0-or-later
//! `arena-diplomacy-model`: the Arena's declaration command and its side-effect channel.
//!
//! The pure tests need no shipped data — they drive
//! `don_ai::arena::diplomacy_runtime` directly over an explicit image. The live-world
//! tests go through `load_world` and skip when `ron-data/` is absent, matching the other
//! arena test files.

use don_ai::arena::diplomacy_runtime::{
    apply_declaration, plan_declaration, ArenaDeclarationInputs, ArenaDeclarationRefusal,
    ArenaDiplomacy, ArenaDiplomacyObject, ArenaLeaderDiplomacyRow, ArenaLeaderRuntime, PlayerCmd,
    ALLY_LOS_TYPE, EJECT_MY_SHIT_VA, LEADER_VICTORY_VA,
};
use don_ai::arena::match_run::{load_world, MatchConfig};
use don_ai::arena::retail_systems::DiplomacyState;
use don_ai::arena::world::World;
use don_ai::OrderResult;
use don_sim::systems::leader_init_diplomacy_loop::{
    plan_leader_init_diplomacy_loop, LeaderInitDiplomacyFacts, LeaderInitDiplomacyLoopImage,
    LeaderInitDiplomacyLoopRequest, SharedVisionDecision,
};
use don_sim::systems::leader_set_diplo::{SetDiploMutation, SetDiploStep};
use don_sim::systems::setup_diplomacy::{PlayerSetup, SetupDiplomacy, PLAYER_PRESENT};
use don_sim::systems::victory_score::{leader_flag, Diplo, NUM_LEADERS};

// ---------------------------------------------------------------------------
// A pure two/three-leader fixture.
// ---------------------------------------------------------------------------

struct Fixture {
    diplomacy: DiplomacyState,
    rows: Vec<ArenaLeaderDiplomacyRow>,
    leaders: Vec<ArenaLeaderRuntime>,
    objects: Vec<ArenaDiplomacyObject>,
    console_who: usize,
}

impl Fixture {
    fn new(players: usize) -> Self {
        Self {
            diplomacy: DiplomacyState::at_war(),
            rows: (0..players).map(ArenaLeaderDiplomacyRow::opening).collect(),
            leaders: vec![
                ArenaLeaderRuntime {
                    alive: true,
                    leader_ai: true,
                };
                players
            ],
            objects: Vec::new(),
            console_who: 0,
        }
    }

    fn inputs(&self) -> ArenaDeclarationInputs<'_> {
        ArenaDeclarationInputs {
            diplomacy: &self.diplomacy,
            rows: &self.rows,
            leaders: &self.leaders,
            objects: &self.objects,
            console_who: self.console_who,
            reveal_map: 0,
            victory_mask: 0,
        }
    }

    fn declare(
        &mut self,
        actor: usize,
        target: usize,
        state: Diplo,
    ) -> Result<Vec<(usize, usize, Diplo)>, ArenaDeclarationRefusal> {
        let planned = plan_declaration(&self.inputs(), actor, target, state)?;
        let outcome = apply_declaration(&mut self.diplomacy, &mut self.rows, &planned)?;
        Ok(outcome.declarations)
    }
}

// ---------------------------------------------------------------------------
// The opening rows are `Leader::init`'s, not this crate's.
// ---------------------------------------------------------------------------

/// `ArenaLeaderDiplomacyRow::opening` must be exactly what the registered
/// `Leader::init` eight-target loop (`0x006E3BF9..0x006E3D93`) produces for the Arena's
/// declared options: `ally_mask = 1 << who`, every `treaties` cell clear, and every
/// non-self declaration at war.
#[test]
fn the_opening_row_is_the_leader_init_loop_output() {
    for who in 0..2usize {
        let mut setup = SetupDiplomacy::default();
        // Two present, team-less leaders. Team bytes outside `0..4` are not configured
        // teams, so `is_team` is false for every distinct pair.
        for slot in 0..2usize {
            setup.players[slot] = PlayerSetup {
                flags: PLAYER_PRESENT,
                who: slot as u8,
                team: 8,
            };
            setup.leaders[slot].leader_flags = 1;
            setup.leaders[slot].who = slot as i32;
        }
        // `teams_locked` is `!matches!(team_style, 0 | 8 | 11)`; a locked style is what
        // makes the non-team opening WAR rather than PEACE. The Arena declares no
        // `GameInfo::team_style` of its own — see the residual in the doc.
        setup.team_style = 1;

        let facts = LeaderInitDiplomacyFacts {
            reveal_map: 0,
            game_rules: 0,
            rush_rules: 0,
            starting_technology: 0,
            starting_technology2: 0,
            ending_technology: 0,
            scenario_rules: false,
            check_victory_mode: false,
            has_shared_vision_preq: false,
        };
        let before = LeaderInitDiplomacyLoopImage {
            setup,
            row: Default::default(),
        };
        let plan = plan_leader_init_diplomacy_loop(
            &before,
            LeaderInitDiplomacyLoopRequest {
                receiver_slot: who,
                tribe: 0,
            },
            facts,
        )
        .expect("the recovered Leader::init loop plans for a present two-leader setup");

        let arena = ArenaLeaderDiplomacyRow::opening(who);
        assert_eq!(
            plan.receipt().ally_mask,
            arena.ally_mask,
            "ally_mask for leader {who}"
        );
        assert_eq!(
            plan.next_state().row.treaties,
            arena.treaties,
            "treaties for leader {who}"
        );
        // The self cell is the one nonzero treaty: `is_team(who, who, 0)` is true at
        // `0x006EBD39` before any team byte is read, and the loop ORs bit 0 for a team.
        let mut expected = [0i32; NUM_LEADERS];
        expected[who] = 1;
        assert_eq!(arena.treaties, expected);
        for target in plan.receipt().targets.iter() {
            let expected = if target.target == who {
                Diplo::Ally as i32
            } else {
                Diplo::War as i32
            };
            // Self stays at the ally cell retail's own `is_ally` reports; every other
            // pair opens at war, which is `DiplomacyState::at_war()`.
            if target.target != who {
                assert_eq!(
                    plan.next_state().setup.leaders[who].diplos[target.target],
                    expected,
                    "opening declaration {who} -> {}",
                    target.target
                );
            }
            assert!(
                matches!(
                    target.shared_vision,
                    SharedVisionDecision::NotAllied | SharedVisionDecision::Unavailable
                ),
                "no shared vision is granted at the Arena's opening"
            );
        }
    }
}

/// The Arena's opening `ally_mask` is self-only, which is exactly what the previous
/// `is_ally`-derived `FogLeader::player_mask` produced. Wiring the fog to `ally_mask`
/// therefore changes no at-war behaviour.
#[test]
fn opening_ally_mask_is_self_only() {
    let d = ArenaDiplomacy::opening(3, 0);
    for who in 0..3usize {
        assert_eq!(d.ally_mask(who), 1u8 << who);
    }
}

// ---------------------------------------------------------------------------
// The transaction the Arena can execute.
// ---------------------------------------------------------------------------

#[test]
fn peace_declaration_writes_both_directional_cells_in_retail_order() {
    let mut f = Fixture::new(2);
    let writes = f
        .declare(0, 1, Diplo::Peace)
        .expect("war -> peace reaches no unhosted authority");
    // `0x006EC94E` then `0x006EC95B`: the actor's own `diplos` cell first, then the
    // global target row. They are two distinct writes, not one symmetric update.
    assert_eq!(
        writes,
        vec![(0, 1, Diplo::Peace), (1, 0, Diplo::Peace)],
        "both directional cells, actor's first"
    );
    assert_eq!(f.diplomacy.declarations()[0][1], Diplo::Peace as i32);
    assert_eq!(f.diplomacy.declarations()[1][0], Diplo::Peace as i32);
    assert_eq!(
        f.diplomacy.relation(0, 1).expect("valid pair"),
        Diplo::Peace
    );
}

#[test]
fn redeclaring_the_current_state_is_a_no_op_transaction() {
    let mut f = Fixture::new(2);
    let writes = f
        .declare(0, 1, Diplo::War)
        .expect("a war-on-war declaration plans");
    // `0x006EC6BB`: equal raw declarations return before every side effect, including
    // the interface-dirty write.
    assert!(writes.is_empty(), "no declaration cell is rewritten");
}

#[test]
fn peace_then_war_returns_the_pair_to_the_opening() {
    let mut f = Fixture::new(2);
    f.declare(0, 1, Diplo::Peace).expect("peace commits");
    let writes = f.declare(1, 0, Diplo::War).expect("war commits");
    assert_eq!(writes, vec![(1, 0, Diplo::War), (0, 1, Diplo::War)]);
    assert_eq!(f.diplomacy.relation(0, 1).expect("valid pair"), Diplo::War);
}

/// A declaration issued by a leader the Arena does not have is retail's `-1`, and it
/// must not be reported as a hostable refusal.
#[test]
fn a_slot_outside_the_match_is_rejected_before_planning() {
    let f = Fixture::new(2);
    assert_eq!(
        plan_declaration(&f.inputs(), 0, 5, Diplo::Peace).unwrap_err(),
        ArenaDeclarationRefusal::UnknownLeader { slot: 5 }
    );
}

// ---------------------------------------------------------------------------
// The transaction the Arena refuses, by name.
// ---------------------------------------------------------------------------

/// In a two-player match, an alliance leaves no independent active leader, so
/// `Leader::set_diplo` calls `Leader::victory(0, 0)` at `0x006EC9B0` and sets game
/// semaphore bit 22. The Arena hosts no `Leaders`/`Match`, so it refuses — and, because
/// the refusal is raised at plan time, the world is untouched.
#[test]
fn a_two_player_alliance_refuses_on_leader_victory_and_changes_nothing() {
    let mut f = Fixture::new(2);
    let before = f.diplomacy.clone();
    let before_rows = f.rows.clone();
    let err = f
        .declare(0, 1, Diplo::Ally)
        .expect_err("a two-player alliance is an immediate shared victory in retail");
    assert_eq!(err, ArenaDeclarationRefusal::UnhostedVictory { winner: 0 });
    assert_eq!(err.retail_va(), Some(LEADER_VICTORY_VA));
    assert_eq!(f.diplomacy, before, "no declaration cell moved");
    assert_eq!(f.rows, before_rows, "no ally_mask bit moved");
}

/// With a third independent active leader the same alliance is fully hostable: the
/// victory scan finds one and emits no authority call at all.
#[test]
fn a_three_player_alliance_commits_when_an_independent_leader_remains() {
    let mut f = Fixture::new(3);
    let writes = f
        .declare(0, 1, Diplo::Ally)
        .expect("leader 2 is active, independent and at war with both");
    assert_eq!(writes, vec![(0, 1, Diplo::Ally), (1, 0, Diplo::Ally)]);
    assert_eq!(f.diplomacy.relation(0, 1).expect("valid pair"), Diplo::Ally);
}

/// A defeated third leader is not independent-and-active: `leader_flags & 3 == 3` fails
/// once `Leader::defeat` has cleared `ACTIVE`, so the alliance becomes a victory again.
#[test]
fn a_defeated_third_leader_does_not_keep_the_alliance_hostable() {
    let mut f = Fixture::new(3);
    f.leaders[2].alive = false;
    let err = f
        .declare(0, 1, Diplo::Ally)
        .expect_err("an inactive leader fails the & 3 == 3 scan predicate");
    assert_eq!(err, ArenaDeclarationRefusal::UnhostedVictory { winner: 0 });
}

/// Shared vision is a `has_preq(ALLY_LOS)` / `reveal_map >= 1` decision, not a
/// consequence of being allied. With neither, an alliance grants no vision.
#[test]
fn an_alliance_without_ally_los_grants_no_shared_vision() {
    let mut f = Fixture::new(3);
    f.declare(0, 1, Diplo::Ally).expect("alliance commits");
    assert_eq!(
        f.rows[0].ally_mask, 0b001,
        "leader 0 still sees only itself"
    );
    assert_eq!(
        f.rows[1].ally_mask, 0b010,
        "leader 1 still sees only itself"
    );
}

#[test]
fn ally_los_grants_shared_vision_and_breaking_the_alliance_revokes_it() {
    let mut f = Fixture::new(3);
    f.rows[0].ally_los = true;
    f.rows[1].ally_los = true;
    f.declare(0, 1, Diplo::Ally).expect("alliance commits");
    assert_eq!(f.rows[0].ally_mask, 0b011);
    assert_eq!(f.rows[1].ally_mask, 0b011);

    // `0x006EC6D2`: the revocation clears both bytes *before* the declaration is
    // rewritten, so the ordering is observable in the emitted steps.
    let planned =
        plan_declaration(&f.inputs(), 0, 1, Diplo::Peace).expect("breaking the alliance plans");
    let mut order = planned
        .plan()
        .steps
        .iter()
        .filter_map(|step| match step {
            SetDiploStep::Mutation(SetDiploMutation::ClearSharedVision { viewer, .. }) => {
                Some(("clear", *viewer))
            }
            SetDiploStep::Mutation(SetDiploMutation::WriteDeclaration { from, .. }) => {
                Some(("write", *from))
            }
            _ => None,
        })
        .collect::<Vec<_>>()
        .into_iter();
    assert_eq!(order.next(), Some(("clear", 0)));
    assert_eq!(order.next(), Some(("clear", 1)));
    assert_eq!(order.next(), Some(("write", 0)));
    assert_eq!(order.next(), Some(("write", 1)));

    apply_declaration(&mut f.diplomacy, &mut f.rows, &planned).expect("break commits");
    assert_eq!(f.rows[0].ally_mask, 0b001);
    assert_eq!(f.rows[1].ally_mask, 0b010);
}

/// The containment sweep is refused rather than skipped. Nothing in the Arena sets
/// `carrier_who` today; this pins that the refusal is structural, so a future transport
/// runtime cannot silently inherit "no ejection happens".
#[test]
fn a_contained_object_refuses_the_ejection_sweep() {
    let mut f = Fixture::new(3);
    f.rows[0].ally_los = true;
    f.rows[1].ally_los = true;
    f.declare(0, 1, Diplo::Ally).expect("alliance commits");
    f.objects.push(ArenaDiplomacyObject {
        owner: 0,
        object_o: 17,
        is_unit: true,
        carrier_who: Some(1),
    });
    let err = plan_declaration(&f.inputs(), 0, 1, Diplo::War)
        .expect_err("revoking an alliance with a contained unit reaches 0x006D0220");
    assert_eq!(
        err,
        ArenaDeclarationRefusal::UnhostedEjection {
            owner: 0,
            object_o: 17
        }
    );
    assert_eq!(err.retail_va(), Some(EJECT_MY_SHIT_VA));
}

/// Uncontained objects are walked and select nothing — the sweep runs, it just has no
/// members. This is why the ordinary Arena reaches no ejection refusal.
#[test]
fn uncontained_objects_do_not_reach_the_ejection_refusal() {
    let mut f = Fixture::new(3);
    f.rows[0].ally_los = true;
    f.rows[1].ally_los = true;
    f.objects = (0..8)
        .map(|o| ArenaDiplomacyObject {
            owner: usize::from(o % 2 != 0),
            object_o: i32::from(o),
            is_unit: true,
            carrier_who: None,
        })
        .collect();
    f.declare(0, 1, Diplo::Ally).expect("alliance commits");
    f.declare(0, 1, Diplo::War)
        .expect("the revocation sweep selects no contained unit");
}

// ---------------------------------------------------------------------------
// The command itself.
// ---------------------------------------------------------------------------

#[test]
fn declare_is_the_shipped_player_verb() {
    let c = PlayerCmd::Declare {
        target: 1,
        state: Diplo::Peace,
    };
    assert_eq!(c.verb_name(), "DECLARE");
    assert_eq!(c.opcode(), 38, "DeclareCommand");
}

#[test]
fn ally_los_is_the_shipped_type_index() {
    // `schema/types.json` `TypeIndex`: 0x2b0 is ALLY_LOS, the prerequisite
    // `Leader::set_diplo` and `Leader::init` both query for shared vision.
    assert_eq!(ALLY_LOS_TYPE, 0x2b0);
}

// ---------------------------------------------------------------------------
// Live world.
// ---------------------------------------------------------------------------

fn world(players: usize) -> Option<World> {
    let mut cfg = MatchConfig::default();
    cfg.tribes = vec![6; players];
    load_world(&cfg).ok()
}

#[test]
fn a_live_two_player_arena_hosts_the_shared_victory_and_retains_it() {
    let Some(mut w) = world(2) else { return };
    assert_eq!(
        w.submit_player(
            0,
            PlayerCmd::Declare {
                target: 1,
                state: Diplo::Ally,
            }
        ),
        OrderResult::Ok(1),
        "the live Arena owns the same Leaders/Match transaction as Sim"
    );
    assert_eq!(w.players[0].orders_refused, 0);
    assert_eq!(w.diplomacy.relation(0, 1).expect("valid pair"), Diplo::Ally);
    let host = w.arena_diplomacy.leader_match();
    assert!(host.leaders().slots[0].flag(leader_flag::WON));
    assert!(host.leaders().slots[1].flag(leader_flag::WON));
    assert!(host
        .game()
        .sem(don_sim::systems::victory_score::game_sem::VICTORY_RESOLVED));

    assert_eq!(
        w.submit_player(
            0,
            PlayerCmd::Declare {
                target: 1,
                state: Diplo::Peace,
            }
        ),
        OrderResult::Ok(1),
        "a later declaration still runs against the retained terminal owner"
    );
    assert_eq!(
        w.diplomacy.relation(0, 1).expect("valid pair"),
        Diplo::Peace
    );
    assert_eq!(w.arena_diplomacy.log().len(), 2);
}

/// The fog planes read `ally_mask`. Without `ALLY_LOS` a peace or alliance grants no
/// vision, so a live match's visibility is unchanged by a declaration — the previous
/// `is_ally`-derived mask would have opened leader 1's map to leader 0 on the alliance.
#[test]
fn a_live_declaration_does_not_move_the_fog_without_ally_los() {
    let Some(mut w) = world(3) else { return };
    let before: Vec<bool> = w.players[0].visible.clone();
    assert_eq!(
        w.submit_player(
            0,
            PlayerCmd::Declare {
                target: 1,
                state: Diplo::Ally,
            }
        ),
        OrderResult::Ok(1)
    );
    assert_eq!(w.arena_diplomacy.ally_mask(0), 0b001);
    assert_eq!(w.players[0].visible, before);
}

#[test]
fn every_arena_slot_opens_with_the_retail_ally_mask() {
    let Some(w) = world(3) else { return };
    for who in 0..3usize {
        assert_eq!(w.arena_diplomacy.ally_mask(who), 1u8 << who);
        let mut treaties = [0i32; NUM_LEADERS];
        treaties[who] = 1;
        assert_eq!(w.arena_diplomacy.rows()[who].treaties, treaties);
    }
    assert_eq!(w.arena_diplomacy.console_who(), 0);
    assert_eq!(w.arena_diplomacy.reveal_map(), 0);
}
