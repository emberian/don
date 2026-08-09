// SPDX-License-Identifier: GPL-3.0-or-later
#[path = "../src/systems/cast_order_frontier.rs"]
mod cast;

use cast::*;

fn id(o: i32, who: i32, uid: u16) -> ObjectIdentity {
    ObjectIdentity { o, who, uid }
}

fn effects() -> ChannelEffects {
    ChannelEffects {
        anim: 11,
        anim_loop: 0,
        anim_force: 1,
        face_angle: Some(0x1234_5678),
        set_seen_player_bit: Some(2),
        mark_visible_state: true,
        presentation: PresentationCone::GenericEvent,
        set_cloak_bits: false,
        set_bribe_bits: false,
    }
}

fn base_targeted() -> CastFrameFacts {
    let target = id(4, 2, 40);
    CastFrameFacts {
        actor: ActorSnapshot {
            identity: id(9, 1, 90),
            x: 100,
            y: 200,
            spell_time: 2,
            held_o: -1,
            held_who: -1,
            held_uid: u16::MAX,
            state_68: 0,
            state_6c: 0,
            seen_players: 0,
        },
        order: CastOrderState {
            target,
            x: 500,
            y: 600,
            paid: 1,
            spell: SPELL_BRIBE,
            flags: 0,
        },
        spell_is_real: true,
        spell_flags: 0x02,
        pay: PayObservation::AlreadyPaid,
        cone: Some(CastCone::Targeted(TargetChannel {
            resolution: TargetResolution::Original(target),
            valid_target: true,
            bribe_alliance_ok: true,
            coordinates: CoordinateDomain::LiveTarget { x: 500, y: 600 },
            range: RangeCone {
                enabled: true,
                range: 400,
                margin: 0,
                actor_distance: 300,
                nearby: NearbySpotObservation::NotNeeded,
            },
            effects: effects(),
            clock: JobClock {
                before: 2,
                required: 9,
                general_extra_increment: false,
            },
        })),
    }
}

#[test]
fn pdb_extent_layout_and_walk_ranges_are_frozen() {
    assert_eq!(CAST_ORDER_INDEX, 14);
    assert_eq!(UNIT_DO_CAST_VA, 0x005e_bfe0);
    assert_eq!(UNIT_DO_CAST_BYTES, 4_191);
    assert_eq!(UNIT_DO_CAST_END_VA, 0x005e_d03f);
    assert_eq!(CAST_ORDER_SIZE, 48);
    assert_eq!(CAST_ORDER_WALKED_BYTES, 27);
    assert_eq!(offsets::OX, 8);
    assert_eq!(offsets::PAID, 0x1c);
    assert_eq!(offsets::SPELL, 0x20);
    assert_eq!(offsets::FLAGS, 0x2c);
    assert_eq!(CAST_REACHABLE_CFG.first().unwrap().start, UNIT_DO_CAST_VA);
    assert_eq!(CAST_REACHABLE_CFG.last().unwrap().start, 0x005e_d03a);
}

#[test]
fn add_cast_normalizes_only_the_two_generic_pack_deploy_ids() {
    assert_eq!(normalize_pack_deploy(0x28b, true, 0, false), 0x28d);
    assert_eq!(normalize_pack_deploy(0x28b, false, 0x3d, false), 0x28f);
    assert_eq!(normalize_pack_deploy(0x28b, false, 7, true), 0x291);
    assert_eq!(normalize_pack_deploy(0x28c, true, 0, false), 0x28e);
    assert_eq!(normalize_pack_deploy(0x28c, false, 0x190, false), 0x290);
    assert_eq!(normalize_pack_deploy(0x28c, false, 7, true), 0x292);
    assert_eq!(
        normalize_pack_deploy(SPELL_BRIBE, true, 0x3d, true),
        SPELL_BRIBE
    );
}

#[test]
fn cost_rejection_never_reaches_a_cast_domain() {
    let mut f = base_targeted();
    f.order.paid = 0;
    f.pay = PayObservation::Attempted {
        result: PayResult::Rejected,
        local_feedback: true,
        mutation_digest: 0x55,
    };
    f.cone = None;
    let p = plan_cast_frame(&f).unwrap();
    assert_eq!(
        p.steps,
        vec![
            CastStep::PayCosts {
                spell: SPELL_BRIBE,
                mutation_digest: 0x55
            },
            CastStep::CostFeedback,
            CastStep::KillCurrent,
        ]
    );
    assert_eq!(p.terminal, CastTerminal::Returned);
}

#[test]
fn accepted_cost_is_latched_before_target_reads() {
    let mut f = base_targeted();
    f.order.paid = 0;
    f.pay = PayObservation::Attempted {
        result: PayResult::Accepted,
        local_feedback: false,
        mutation_digest: 0x77,
    };
    let p = plan_cast_frame(&f).unwrap();
    assert_eq!(
        p.steps[0],
        CastStep::PayCosts {
            spell: SPELL_BRIBE,
            mutation_digest: 0x77
        }
    );
    assert_eq!(p.steps[1], CastStep::StorePaid(1));
    assert!(matches!(p.steps[2], CastStep::ValidateTarget(_)));
}

#[test]
fn repaired_target_writes_complete_identity_before_validation() {
    let mut f = base_targeted();
    let repaired = id(6, 2, 60);
    if let Some(CastCone::Targeted(ref mut c)) = f.cone {
        c.resolution = TargetResolution::Repaired {
            from: f.order.target,
            to: repaired,
        };
    }
    let p = plan_cast_frame(&f).unwrap();
    assert_eq!(p.steps[0], CastStep::StoreOrderTarget(repaired));
    assert_eq!(p.steps[1], CastStep::ValidateTarget(repaired));
    assert_eq!(p.steps[2], CastStep::StoreHeldTarget(repaired));
}

#[test]
fn invalid_bribe_alliance_kills_after_validation_without_channel_effects() {
    let mut f = base_targeted();
    if let Some(CastCone::Targeted(ref mut c)) = f.cone {
        c.bribe_alliance_ok = false;
    }
    let p = plan_cast_frame(&f).unwrap();
    assert_eq!(
        p.steps,
        vec![
            CastStep::ValidateTarget(f.order.target),
            CastStep::KillCurrent
        ]
    );
}

#[test]
fn out_of_range_found_spot_hands_off_move_and_returns() {
    let mut f = base_targeted();
    if let Some(CastCone::Targeted(ref mut c)) = f.cone {
        c.range.actor_distance = 900;
        c.range.nearby = NearbySpotObservation::Found {
            x: 450,
            y: 550,
            distance_to_target: 350,
        };
    }
    let p = plan_cast_frame(&f).unwrap();
    assert_eq!(p.terminal, CastTerminal::MoveInstalled);
    assert_eq!(
        &p.steps[p.steps.len() - 2..],
        &[
            CastStep::FindNearbySpot {
                target_x: 500,
                target_y: 600
            },
            CastStep::AddMoveOrder { x: 450, y: 550 },
        ]
    );
}

#[test]
fn found_spot_still_out_of_range_must_not_install_move() {
    let mut f = base_targeted();
    if let Some(CastCone::Targeted(ref mut c)) = f.cone {
        c.range.actor_distance = 900;
        c.range.nearby = NearbySpotObservation::Found {
            x: 450,
            y: 550,
            distance_to_target: 401,
        };
    }
    let p = plan_cast_frame(&f).unwrap();
    assert_eq!(p.terminal, CastTerminal::Returned);
    assert_eq!(p.steps.last(), Some(&CastStep::Hold));
    assert!(!p
        .steps
        .iter()
        .any(|s| matches!(s, CastStep::AddMoveOrder { .. })));
}

#[test]
fn targeted_completion_orders_visibility_facing_animation_then_cast_and_retire() {
    let mut f = base_targeted();
    f.actor.spell_time = 8;
    if let Some(CastCone::Targeted(ref mut c)) = f.cone {
        c.clock.before = 8;
    }
    let p = plan_cast_frame(&f).unwrap();
    assert_eq!(p.terminal, CastTerminal::CastAndRetired);
    let seen = p
        .steps
        .iter()
        .position(|s| matches!(s, CastStep::SetSeenPlayerBit(2)))
        .unwrap();
    let face = p
        .steps
        .iter()
        .position(|s| matches!(s, CastStep::SetAngle(_)))
        .unwrap();
    let anim = p
        .steps
        .iter()
        .position(|s| matches!(s, CastStep::SetAnim { .. }))
        .unwrap();
    let cast = p
        .steps
        .iter()
        .position(|s| matches!(s, CastStep::CastTarget { .. }))
        .unwrap();
    let clear = p
        .steps
        .iter()
        .position(|s| matches!(s, CastStep::StorePaidZero))
        .unwrap();
    assert!(seen < face && face < anim && anim < cast && cast < clear);
    assert_eq!(p.steps.last(), Some(&CastStep::KillCurrent));
}

#[test]
fn coordinate_spell_casts_order_point_not_live_target() {
    let mut f = base_targeted();
    f.actor.spell_time = 8;
    f.order.x = 700;
    f.order.y = 800;
    f.spell_flags |= 8;
    if let Some(CastCone::Targeted(ref mut c)) = f.cone {
        c.coordinates = CoordinateDomain::OrderPoint { x: 700, y: 800 };
        c.clock.before = 8;
    }
    let p = plan_cast_frame(&f).unwrap();
    assert!(p.steps.contains(&CastStep::CastPoint {
        spell: SPELL_BRIBE,
        x: 700,
        y: 800
    }));
    assert!(!p
        .steps
        .iter()
        .any(|s| matches!(s, CastStep::CastTarget { .. })));
}

fn base_untargeted(spell: i32) -> CastFrameFacts {
    CastFrameFacts {
        actor: ActorSnapshot {
            identity: id(9, 1, 90),
            x: 100,
            y: 200,
            spell_time: 0,
            held_o: -1,
            held_who: -1,
            held_uid: u16::MAX,
            state_68: 0,
            state_6c: 0,
            seen_players: 0,
        },
        order: CastOrderState {
            target: id(-1, -1, u16::MAX),
            x: 500,
            y: 600,
            paid: 1,
            spell,
            flags: 0,
        },
        spell_is_real: true,
        spell_flags: 0,
        pay: PayObservation::AlreadyPaid,
        cone: Some(CastCone::Untargeted(UntargetedChannel {
            opening: UntargetedOpening::Prepare {
                set_rare_collector_global: false,
                set_merchant_latch: false,
                update_group_piece: false,
                reposition: None,
                face_angle: None,
                anim: 23,
            },
            transport: TransportCone::NotTransport,
            non_spell: NonSpellCone::NotNonSpell,
            clock: JobClock {
                before: 0,
                required: 3,
                general_extra_increment: false,
            },
            cast_is_real_spell: true,
        })),
    }
}

#[test]
fn untargeted_completion_casts_zero_point_then_clears_paid_and_retires() {
    let mut f = base_untargeted(SPELL_RALLY);
    f.actor.spell_time = 2;
    if let Some(CastCone::Untargeted(ref mut c)) = f.cone {
        c.opening = UntargetedOpening::AlreadyOpened;
        c.clock.before = 2;
    }
    let p = plan_cast_frame(&f).unwrap();
    assert_eq!(
        &p.steps[p.steps.len() - 4..],
        &[
            CastStep::StoreSpellTime(0),
            CastStep::CastPoint {
                spell: SPELL_RALLY,
                x: 0,
                y: 0
            },
            CastStep::StorePaidZero,
            CastStep::KillCurrent,
        ]
    );
}

#[test]
fn untargeted_opening_mutations_keep_retail_order() {
    let mut f = base_untargeted(SPELL_RALLY);
    if let Some(CastCone::Untargeted(ref mut c)) = f.cone {
        c.opening = UntargetedOpening::Prepare {
            set_rare_collector_global: true,
            set_merchant_latch: true,
            update_group_piece: true,
            reposition: Some((333, 444)),
            face_angle: Some(0xa000_0000),
            anim: 24,
        };
    }
    let p = plan_cast_frame(&f).unwrap();
    assert_eq!(
        &p.steps[..6],
        &[
            CastStep::SetRareCollectorGlobal,
            CastStep::SetMerchantLatch,
            CastStep::UpdateGroupPiece,
            CastStep::Reposition { x: 333, y: 444 },
            CastStep::SetAngle(0xa000_0000),
            CastStep::SetAnim {
                anim: 24,
                looped: 0,
                force: 1
            },
        ]
    );
}

#[test]
fn untargeted_opening_rejection_kills_with_paid_retained() {
    let mut f = base_untargeted(SPELL_RALLY);
    if let Some(CastCone::Untargeted(ref mut c)) = f.cone {
        c.opening = UntargetedOpening::Rejected {
            local_feedback: true,
        };
    }
    let p = plan_cast_frame(&f).unwrap();
    assert_eq!(
        p.steps,
        vec![CastStep::OpeningFeedback, CastStep::KillCurrent]
    );
    assert_eq!(p.terminal, CastTerminal::KilledPaidRetained);
    assert!(!p.steps.contains(&CastStep::StorePaidZero));
}

#[test]
fn transport_failure_kills_without_clearing_paid_latch() {
    let mut f = base_untargeted(SPELL_TRANSPORT);
    if let Some(CastCone::Untargeted(ref mut c)) = f.cone {
        c.transport = TransportCone::SearchFailed;
    }
    let p = plan_cast_frame(&f).unwrap();
    assert_eq!(p.terminal, CastTerminal::KilledPaidRetained);
    assert_eq!(p.steps.last(), Some(&CastStep::KillCurrent));
    assert!(!p.steps.contains(&CastStep::StorePaidZero));
}

#[test]
fn transport_completion_cast_retains_order_and_paid_state() {
    let mut f = base_untargeted(SPELL_TRANSPORT);
    f.actor.spell_time = 2;
    if let Some(CastCone::Untargeted(ref mut c)) = f.cone {
        c.opening = UntargetedOpening::AlreadyOpened;
        c.transport = TransportCone::CompletionCast;
        c.clock.before = 2;
    }
    let p = plan_cast_frame(&f).unwrap();
    assert_eq!(p.terminal, CastTerminal::TransportRetained);
    assert!(p.steps.contains(&CastStep::CastPoint {
        spell: SPELL_TRANSPORT,
        x: 0,
        y: 0
    }));
    assert!(!p.steps.contains(&CastStep::StorePaidZero));
    assert!(!p.steps.contains(&CastStep::KillCurrent));
}

#[test]
fn non_spell_sentinel_forces_untargeted_domain_and_skips_cast_call() {
    let mut f = base_untargeted(0x900);
    f.spell_is_real = false;
    f.actor.spell_time = 2;
    if let Some(CastCone::Untargeted(ref mut c)) = f.cone {
        c.opening = UntargetedOpening::AlreadyOpened;
        c.clock.before = 2;
        c.cast_is_real_spell = false;
        c.non_spell = NonSpellCone::NoTransfer;
    }
    let p = plan_cast_frame(&f).unwrap();
    assert_eq!(p.effective_spell, NON_SPELL_SENTINEL);
    assert!(!p
        .steps
        .iter()
        .any(|s| matches!(s, CastStep::CastPoint { .. } | CastStep::CastTarget { .. })));
    assert_eq!(p.steps.last(), Some(&CastStep::KillCurrent));
}

#[test]
fn non_spell_transfer_precedes_ordinary_paid_clear_and_retirement() {
    let mut f = base_untargeted(0x900);
    f.spell_is_real = false;
    f.actor.spell_time = 2;
    let building = id(20, 1, 200);
    if let Some(CastCone::Untargeted(ref mut c)) = f.cone {
        c.opening = UntargetedOpening::AlreadyOpened;
        c.clock.before = 2;
        c.cast_is_real_spell = false;
        c.non_spell = NonSpellCone::Transfer {
            building,
            accepts_transport: true,
        };
    }
    let p = plan_cast_frame(&f).unwrap();
    let transfer = p
        .steps
        .iter()
        .position(|s| *s == CastStep::NonSpellTransfer { building })
        .unwrap();
    let clear = p
        .steps
        .iter()
        .position(|s| *s == CastStep::StorePaidZero)
        .unwrap();
    assert!(transfer < clear);
    assert_eq!(p.steps.last(), Some(&CastStep::KillCurrent));
}

#[test]
fn preflight_invalidates_on_any_host_epoch_or_fact_change() {
    let f = base_targeted();
    let p = preflight_cast(1, 2, 3, 4, 5, 6, 7, 8, f).unwrap();
    assert!(preflight_still_valid(&p, &p));
    let mut stale = p.clone();
    stale.object_epoch += 1;
    assert!(!preflight_still_valid(&p, &stale));
    stale = p.clone();
    stale.facts.actor.spell_time += 1;
    assert!(!preflight_still_valid(&p, &stale));
}

#[test]
fn open_tail_inventory_keeps_strict_closure_delta_zero() {
    assert_eq!(CAST_OPEN_TAILS.len(), 13);
    assert!(CAST_OPEN_TAILS.contains(&CastOpenTail::CostTransaction));
    assert!(CAST_OPEN_TAILS.contains(&CastOpenTail::NonSpellBuildingTransfer));
    assert!(CAST_OPEN_TAILS.contains(&CastOpenTail::DispatcherAtomicCommit));
}
