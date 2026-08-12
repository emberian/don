// SPDX-License-Identifier: GPL-3.0-or-later

#[path = "../src/systems/diplomacy_deal_callbacks.rs"]
mod diplomacy_deal_callbacks;
#[path = "../src/systems/leader_set_diplo.rs"]
mod leader_set_diplo;

use diplomacy_deal_callbacks::{
    plan_consider_tribute, plan_notify_deal, ConsiderTributeRequest, DealCallbackFacts,
    DealCallbackImage, GiftStampDecision, NotifyDealRequest, NotifyDealText,
};

fn facts() -> DealCallbackFacts {
    DealCallbackFacts {
        receiver_flag_four: [Some(false); 8],
        sender_blacken: [Some(1); 8],
        receiver_resources: [[Some(100); 6]; 8],
        receiver_econ: [[Some(4); 6]; 8],
    }
}

#[test]
fn zero_raw_is_a_complete_noop_without_querying_any_fact() {
    let before = DealCallbackImage {
        frame: 77,
        local_who: 2,
        ..Default::default()
    };
    let plan = plan_consider_tribute(
        &before,
        &DealCallbackFacts::default(),
        ConsiderTributeRequest {
            receiver: 2,
            sender: 3,
            raw: 0,
            good: 5,
        },
    )
    .unwrap();
    assert_eq!(plan.after, before);
    assert!(!plan.wrote_sender_tribute_stamp);
    assert_eq!(plan.gift_stamp, GiftStampDecision::SkippedNonPositive);
}

#[test]
fn positive_call_stamps_sender_first_and_receiver_when_threshold_gates_pass() {
    let before = DealCallbackImage {
        frame: 700,
        local_who: 2,
        ..Default::default()
    };
    let plan = plan_consider_tribute(
        &before,
        &facts(),
        ConsiderTributeRequest {
            receiver: 3,
            sender: 2,
            raw: 125,
            good: 0,
        },
    )
    .unwrap();
    assert_eq!(plan.after.leaders[2].tribute_stamp[3], 700);
    assert_eq!(plan.after.leaders[3].gift_stamp[2], 700);
    assert_eq!(plan.gift_stamp, GiftStampDecision::ThresholdAndEconomyFlag);
}

#[test]
fn flag_four_bypasses_blacken_resource_and_econ_queries() {
    let mut facts = DealCallbackFacts::default();
    facts.receiver_flag_four[4] = Some(true);
    let before = DealCallbackImage {
        frame: 91,
        ..Default::default()
    };
    let plan = plan_consider_tribute(
        &before,
        &facts,
        ConsiderTributeRequest {
            receiver: 4,
            sender: 1,
            raw: 1,
            good: 5,
        },
    )
    .unwrap();
    assert_eq!(plan.after.leaders[1].tribute_stamp[4], 91);
    assert_eq!(plan.after.leaders[4].gift_stamp[1], 91);
    assert_eq!(plan.gift_stamp, GiftStampDecision::ReceiverFlagFour);
}

#[test]
fn good_two_bypasses_econ_flag_but_not_threshold_or_resource_cap() {
    let before = DealCallbackImage {
        frame: 55,
        ..Default::default()
    };
    let mut facts = facts();
    facts.receiver_econ[0][2] = None;
    let plan = plan_consider_tribute(
        &before,
        &facts,
        ConsiderTributeRequest {
            receiver: 0,
            sender: 1,
            raw: 100,
            good: 2,
        },
    )
    .unwrap();
    assert_eq!(plan.after.leaders[0].gift_stamp[1], 55);
    assert_eq!(
        plan.gift_stamp,
        GiftStampDecision::ThresholdAndEconomyGoodTwo
    );

    facts.receiver_resources[0][2] = Some(3_000);
    let capped = plan_consider_tribute(
        &before,
        &facts,
        ConsiderTributeRequest {
            receiver: 0,
            sender: 1,
            raw: 100,
            good: 2,
        },
    )
    .unwrap();
    assert_eq!(capped.after.leaders[0].gift_stamp[1], 0);
    assert_eq!(
        capped.gift_stamp,
        GiftStampDecision::ReceiverResourceAtLeastThreeThousand
    );
}

#[test]
fn blacken_multiplier_uses_x86_wrapping_and_positive_calls_always_stamp_sender() {
    let before = DealCallbackImage {
        frame: 123,
        ..Default::default()
    };
    let mut facts = facts();
    facts.sender_blacken[6] = Some(i32::MAX);
    facts.receiver_econ[5][1] = Some(0);
    let plan = plan_consider_tribute(
        &before,
        &facts,
        ConsiderTributeRequest {
            receiver: 5,
            sender: 6,
            raw: 1,
            good: 1,
        },
    )
    .unwrap();
    assert_eq!(plan.after.leaders[6].tribute_stamp[5], 123);
    assert_eq!(plan.after.leaders[5].gift_stamp[6], 0);
    assert_eq!(plan.gift_stamp, GiftStampDecision::EconomyFlagAbsent);
}

#[test]
fn notify_deal_is_local_only_and_selects_all_three_text_arms() {
    let image = DealCallbackImage {
        local_who: 3,
        ..Default::default()
    };
    assert_eq!(
        plan_notify_deal(
            &image,
            NotifyDealRequest {
                leader: 2,
                other: 3,
                treaty: 1,
            }
        )
        .unwrap(),
        None
    );
    for (treaty, text) in [
        (1, NotifyDealText::Peace),
        (2, NotifyDealText::Alliance),
        (-1, NotifyDealText::Other),
    ] {
        let envelope = plan_notify_deal(
            &image,
            NotifyDealRequest {
                leader: 3,
                other: 2,
                treaty,
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(envelope.text, text);
        assert_eq!(envelope.chat_sender, 3);
        assert_eq!(envelope.sound_category, 3);
    }
}
