//! `Leader::process_taunt` `0x006B8CC0` through the real tick.
//!
//! Every test here drives `Sim::do_frame`, so what it observes is the dispatcher at
//! `0x006ED3CE` calling the recovered body at its exact place inside step 8 — not the body
//! called directly. The unit tests in `systems/leader_process_taunt.rs` cover the arms; these
//! cover the wiring, the frame semantics, and the two claims that motivated the lane:
//! that this function writes checksum-visible simulation state, and that its presentation
//! leaves through a typed boundary instead of the sim path.
//!
//! Tier C throughout. Nothing here has been executed against retail.

use don_sim::systems::leader_process_taunt::{
    taunt, BuildMods, TauntStop, TauntUnresolved, DIPLO_ALLIED,
};
use don_sim::systems::leaders;
use don_sim::tick::{Gap, Sim};

/// Two mutually allied, active, non-human leaders with every host answer supplied, wound
/// forward to frame 1 so the dispatcher's `Game::frame != 0` gate (`0x006ED3CE`) is open.
fn allied_sim() -> Sim {
    let mut sim = Sim::new(0x7a11, 16);
    sim.activate(0);
    sim.activate(1);
    sim.step8.leaders[0].diplo[1] = DIPLO_ALLIED;
    sim.step8.leaders[1].diplo[0] = DIPLO_ALLIED;

    let env = &mut sim.step8_env.taunt;
    env.local_who = Some(-1);
    env.team_style = Some(0);
    for i in 0..8 {
        env.is_neutral[i] = Some(false);
        for r in 0..6 {
            // `LeaderData::type_avail(res, 1)` `0x006E33A0`; the tribute arms test `== 4`.
            env.type_avail[i][r] = Some(4);
        }
    }

    // Frame 0 cannot dispatch: `0x006ED3CE` is `cmp dword [ecx + 0x550], 0 ; je`.
    sim.do_frame();
    assert_eq!(sim.world.frame, 1);
    sim
}

/// Stamp one of the eight `LeaderData::incoming_taunt*` triples for the coming frame.
fn arm(sim: &mut Sim, slot: usize, entry: usize, kind: i32, who: i32) {
    let frame = sim.world.frame;
    sim.step8.leaders[slot].taunt_kind[entry] = kind;
    sim.step8.leaders[slot].taunt_arg[entry] = who;
    sim.step8.leaders[slot].taunt_frame[entry] = frame;
}

#[test]
fn the_real_tick_stages_a_two_sided_tribute_ledger() {
    let mut sim = allied_sim();
    // The façade is what `Sim::sync_step8_inputs` copies into the step-8 leader.
    sim.leaders[0].econ.stockpile[0] = 900;
    arm(&mut sim, 0, 3, taunt::FOOD, 1);
    sim.do_frame();

    let stock = sim.step8.leaders[0].econ.stockpile[0];
    // `Leader::action_offer` `0x006D1780` writes both sides, and the second is the exact
    // negation of the first. Neither side's stockpile moves here: that is
    // `Leader::action_respond` `0x006D03C0`, which is this port's named boundary.
    assert_eq!(sim.step8.leaders[0].taunt.dip[1].offers[0], stock / 3);
    assert_eq!(sim.step8.leaders[1].taunt.dip[0].offers[0], -(stock / 3));
    assert_eq!(sim.step8.leaders[1].econ.stockpile[0], 0);

    // The two arrays the dispatcher never touches, at `+0x354` and `+0x374`.
    assert_eq!(sim.step8.leaders[0].taunt.last_taunt[1], taunt::FOOD);
    assert_eq!(sim.step8.leaders[0].taunt.last_taunt_frame[1], 1);
    // `team_style == 0` is not "teams locked", so the cooldown arm ran and stamped
    // `gift_stamp[1]` at `0x006B8ECD`.
    assert_eq!(sim.step8.leaders[0].taunt.gift_stamp[1], 1);

    // The step-8 economy transaction still reaches the shared façade afterwards.
    assert_eq!(sim.leaders[0].econ.stockpile[0], stock);

    // The dispatch happened once and the only retail call still absent on its path is
    // `Leader::action_respond`.
    assert_eq!(sim.cover.leader_taunt_dispatches, 1);
}

#[test]
fn the_gift_cooldown_blocks_the_second_tribute_for_4500_frames() {
    let mut sim = allied_sim();
    sim.leaders[0].econ.stockpile[0] = 900;
    arm(&mut sim, 0, 0, taunt::FOOD, 1);
    sim.do_frame();
    let first = sim.step8.leaders[0].taunt.dip[1].offers[0];
    assert!(first > 0);

    // Same frame budget, a second ask well inside `0x1194` frames.
    arm(&mut sim, 0, 0, taunt::FOOD, 1);
    sim.do_frame();
    assert_eq!(sim.step8.leaders[0].taunt.dip[1].offers[0], first);
    // The stamp did not move either — retail returns before `0x006B8ECD`.
    assert_eq!(sim.step8.leaders[0].taunt.gift_stamp[1], 1);
}

#[test]
fn a_build_taunt_rewrites_and_clamps_the_ai_scalars_inside_the_tick() {
    let mut sim = allied_sim();
    sim.step8.leaders[0].taunt.mods = BuildMods {
        wonder: 0,
        ground: 0x100,
        air: 0x100,
        sea: 0x100,
        infra: 0x100,
        defense: 0x100,
    };
    arm(&mut sim, 0, 5, taunt::BUILD_SEA, 1);
    sim.do_frame();

    let m = sim.step8.leaders[0].taunt.mods;
    // `0x006B8F9E`: ground/16, sea<<5, air/16, infra/4 — then the five clamps at
    // `0x006B94A4`, which is why infra lands on its 0x80 floor rather than on 0x40.
    assert_eq!(m.ground, 0x10);
    assert_eq!(m.sea, 0x2000);
    assert_eq!(m.air, 0x10);
    assert_eq!(m.infra, 0x80);
    assert_eq!(m.defense, 0x100);
    assert_eq!(m.wonder, 0);
}

#[test]
fn rush_writes_personality_raid_through_the_tick() {
    let mut sim = allied_sim();
    arm(&mut sim, 0, 1, taunt::RUSH, 1);
    sim.do_frame();
    assert_eq!(sim.step8.leaders[0].taunt.personality_raid, 1);
    assert_eq!(sim.step8.leaders[0].taunt.mods.ground, 0x200);
    assert_eq!(sim.step8.leaders[0].taunt.mods.defense, 0x80);

    arm(&mut sim, 0, 1, taunt::BOOM, 1);
    sim.do_frame();
    assert_eq!(sim.step8.leaders[0].taunt.personality_raid, -2);
    assert_eq!(sim.step8.leaders[0].taunt.mods.ground, 4);
    assert_eq!(sim.step8.leaders[0].taunt.mods.infra, 0x200);
}

#[test]
fn a_human_leader_is_the_first_gate_and_writes_nothing() {
    let mut sim = allied_sim();
    sim.leaders[0].econ.stockpile[0] = 900;
    // `LeaderData::leader_flags & 4` at `0x006B8CEF` — `victory_score::leader_flag::HUMAN`.
    sim.step8.leaders[0].flags |= 4;
    arm(&mut sim, 0, 2, taunt::FOOD, 1);
    sim.do_frame();

    assert_eq!(sim.step8.leaders[0].taunt.dip[1].offers[0], 0);
    assert_eq!(sim.step8.leaders[0].taunt.last_taunt[1], 0);
    assert_eq!(sim.step8.leaders[0].taunt.gift_stamp[1], 0);
    // The scan still dispatched; the body returned at its first instruction group.
    assert_eq!(sim.cover.leader_taunt_dispatches, 1);
}

#[test]
fn a_tribute_between_non_allies_is_refused_by_both_diplomacy_reads() {
    let mut sim = allied_sim();
    sim.leaders[0].econ.stockpile[0] = 900;
    // `0x006B8D0E` reads the asked leader's own record …
    sim.step8.leaders[0].diplo[1] = 0;
    arm(&mut sim, 0, 0, taunt::FOOD, 1);
    sim.do_frame();
    assert_eq!(sim.step8.leaders[0].taunt.dip[1].offers[0], 0);

    // … and `0x006B8D29` reads the *other* leader's, indexed by `leaders[me].who`.
    sim.step8.leaders[0].diplo[1] = DIPLO_ALLIED;
    sim.step8.leaders[1].diplo[0] = 0;
    arm(&mut sim, 0, 0, taunt::FOOD, 1);
    sim.do_frame();
    assert_eq!(sim.step8.leaders[0].taunt.dip[1].offers[0], 0);

    // Both directions restored: the same setup now stages an offer.
    sim.step8.leaders[1].diplo[0] = DIPLO_ALLIED;
    arm(&mut sim, 0, 0, taunt::FOOD, 1);
    sim.do_frame();
    assert!(sim.step8.leaders[0].taunt.dip[1].offers[0] > 0);
}

#[test]
fn a_withheld_host_answer_refuses_the_dispatch_and_stays_charged() {
    let mut sim = allied_sim();
    sim.leaders[0].econ.stockpile[0] = 900;
    // `LeaderData::type_avail` `0x006E33A0` has no port; nothing is guessed in its place.
    sim.step8_env.taunt.type_avail[1][0] = None;
    let before = sim.cover.gaps[Gap::LeaderProcessTaunt.index()];
    arm(&mut sim, 0, 0, taunt::FOOD, 1);
    sim.do_frame();

    assert_eq!(sim.step8.leaders[0].taunt.dip[1].offers[0], 0);
    assert_eq!(sim.cover.gaps[Gap::LeaderProcessTaunt.index()], before + 1);
}

#[test]
fn the_dispatcher_reads_the_pre_increment_frame_for_both_the_scan_and_the_stamp() {
    let mut sim = allied_sim();
    // Stamped for a frame that is not the one about to run.
    sim.step8.leaders[0].taunt_kind[7] = taunt::RUSH;
    sim.step8.leaders[0].taunt_arg[7] = 1;
    sim.step8.leaders[0].taunt_frame[7] = 99;
    sim.do_frame();
    assert_eq!(sim.cover.leader_taunt_dispatches, 0);
    assert_eq!(sim.step8.leaders[0].taunt.personality_raid, 0);

    // Wind to the stamped frame; step 8 runs before step 20 moves `Game::frame`.
    while sim.world.frame < 99 {
        sim.do_frame();
    }
    assert_eq!(sim.world.frame, 99);
    sim.do_frame();
    assert_eq!(sim.cover.leader_taunt_dispatches, 1);
    assert_eq!(sim.step8.leaders[0].taunt.personality_raid, 1);
    assert_eq!(sim.step8.leaders[0].taunt.last_taunt_frame[1], 99);
}

#[test]
fn all_eight_table_entries_fire_in_order_on_one_frame() {
    let mut sim = allied_sim();
    sim.leaders[0].econ.stockpile = [900, 900, 900, 900, 900, 900];
    let frame = sim.world.frame;
    // Every entry aims at the same ally; the last one written wins `last_taunt[1]`.
    for k in 0..8 {
        sim.step8.leaders[0].taunt_kind[k] = taunt::BUILD_INFRA;
        sim.step8.leaders[0].taunt_arg[k] = 1;
        sim.step8.leaders[0].taunt_frame[k] = frame;
    }
    sim.step8.leaders[0].taunt.mods.infra = 1;
    sim.do_frame();

    assert_eq!(sim.cover.leader_taunt_dispatches, 8);
    // 1 clamps to 0x80, then seven more `<< 2` with a clamp after each: 0x80 << 2 = 0x200,
    // 0x800, 0x2000, 0x8000, then the 0x8000 ceiling holds for the rest.
    assert_eq!(sim.step8.leaders[0].taunt.mods.infra, 0x8000);
    assert_eq!(sim.step8.leaders[0].taunt.mods.ground, 1);
}

#[test]
fn presentation_never_reaches_the_simulation_path() {
    // `TAUNT_NEED` returns before any store unless `who == Console::who`, and the flavour
    // die is `internal_random` `0x00EB697C`, not `game_random` `0x00C06184`. Two frames that
    // differ only in the local player's identity and the presentation die must leave the
    // simulation bit-identical.
    let mut quiet = allied_sim();
    let mut loud = allied_sim();
    loud.step8_env.taunt.local_who = Some(1);
    loud.step8_env.taunt.taunt_audio = true;
    loud.step8_env.taunt.flavour_rolls.extend([0, 4, 8]);

    for sim in [&mut quiet, &mut loud] {
        sim.leaders[0].econ.stockpile = [700, 700, 700, 700, 700, 700];
        arm(sim, 0, 0, taunt::NEED, 1);
        arm(sim, 0, 1, taunt::ATTACK, 1);
        sim.do_frame();
    }

    assert_eq!(
        quiet.step8.leaders[0].taunt, loud.step8.leaders[0].taunt,
        "presentation state leaked into the leader"
    );
    assert_eq!(
        quiet.step8.leaders[0].econ, loud.step8.leaders[0].econ,
        "presentation state leaked into the economy"
    );
    // The strongest form of the claim: `game_random`'s state is untouched by the
    // presentation die, so a lockstep peer that sees a different local player stays in sync.
    assert_eq!(quiet.world.random.state(), loud.world.random.state());
    // …and the scalar write that is *not* presentation happened in both.
    assert_eq!(quiet.step8.leaders[0].taunt.mods.defense, 0x40);
}

#[test]
fn an_unknown_taunt_code_still_normalises_the_scalars() {
    // `0x006B9264` is the jump table's `ja` arm: it skips every store and the whole
    // presentation block, but falls into the five clamps at `0x006B94A4`.
    let mut sim = allied_sim();
    sim.step8.leaders[0].taunt.mods = BuildMods::default();
    arm(&mut sim, 0, 0, taunt::COUNT, 1);
    sim.do_frame();

    let m = sim.step8.leaders[0].taunt.mods;
    assert_eq!((m.ground, m.sea, m.air), (1, 1, 1));
    assert_eq!(m.infra, 0x80);
    assert_eq!(m.defense, 0x10);
    // The two pre-switch stamps still landed.
    assert_eq!(sim.step8.leaders[0].taunt.last_taunt[1], taunt::COUNT);
}

#[test]
fn step_eight_stays_a_stub_and_the_step_run_is_unchanged() {
    // This lane closes one charged child. Nothing here licenses flipping step 8's status,
    // and `Gap::LeaderProcessTaunt` is still charged for the boundary the port refuses.
    let mut sim = allied_sim();
    sim.leaders[0].econ.stockpile[0] = 900;
    let before = sim.cover.gaps[Gap::LeaderProcessTaunt.index()];
    arm(&mut sim, 0, 0, taunt::FOOD, 1);
    sim.do_frame();
    assert_eq!(
        sim.cover.gaps[Gap::LeaderProcessTaunt.index()],
        before + 1,
        "reaching Leader::action_respond 0x006D03C0 must still charge the gap"
    );
}

/// The named stops and boundaries are part of the module's contract, so a rename or a
/// dropped variant is a compile error here rather than a silent behaviour change.
#[test]
fn the_named_boundary_is_action_respond() {
    let boundaries = [
        TauntUnresolved::ActionRespond,
        TauntUnresolved::TypeAvail,
        TauntUnresolved::TeamsLocked,
        TauntUnresolved::IsNeutral,
        TauntUnresolved::SlotOutOfRange,
    ];
    assert!(boundaries.iter().all(|b| b.is_simulation()));
    assert!(!TauntUnresolved::FlavourRoll.is_simulation());
    assert!(!TauntUnresolved::LocalWho.is_simulation());
    assert_ne!(TauntStop::GiftCooldown, TauntStop::NotEnoughStockpile);
    assert_eq!(leaders::NUM_LEADER_SLOTS, 8);
}
