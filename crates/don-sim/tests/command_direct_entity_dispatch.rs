//! Dispatcher integration pins for command rows 46 through 49.

use don_sim::command::direct_entity_command_integration::plans::{
    DirectEntityCommandRequest, DirectEntityKind, DirectEntityTargetFacts, MarketCommandFacts,
    MarketSide,
};
use don_sim::command::direct_entity_command_integration::unit_action_come_out::{
    preflight_unit_action_come_out, InsideLookupFacts, ObjectIdentity, UnitActionComeOutFacts,
};
use don_sim::command::direct_entity_command_integration::{
    classify_direct_entity_command, execute_market_command,
    preflight_opcode49_unit_action_come_out_command, DirectEntityDisposition,
    DirectEntityFleetReceipt, DirectEntityFleetRequest, DirectEntityOpenTail,
    DirectEntityTransactionStatus, DirectEntityTypeFacts, MarketCounterBinding,
    MarketEconomyBinding, MarketTransactionStatus,
};
use don_sim::command::{Bridge, Fleet, InlineDef, InlinePort, Package};
use don_sim::systems::economy::{
    EconRules, LeaderEcon, MarketPriceGates, MarketState, NUM_RESOURCES, RES_FOOD, RES_TIMBER,
    RES_WEALTH, TRADE_LOT,
};
use don_sim::systems::order_dispatch::OrderQueue;

struct TransactionFleet {
    queue: OrderQueue,
    rules: EconRules,
    market: MarketState,
    econ: LeaderEcon,
    demand: [i32; NUM_RESOURCES],
    supply: [i32; NUM_RESOURCES],
}

impl TransactionFleet {
    fn new() -> Self {
        let mut market = MarketState::default();
        market.base_price[RES_FOOD] = 50;
        market.base_price[RES_TIMBER] = 80;
        let mut econ = LeaderEcon::new();
        econ.stockpile[RES_WEALTH] = 500;
        econ.stockpile[RES_TIMBER] = 2 * TRADE_LOT;
        Self {
            queue: OrderQueue::new(),
            rules: EconRules::shipped(),
            market,
            econ,
            demand: [250; NUM_RESOURCES],
            supply: [350; NUM_RESOURCES],
        }
    }

    fn market_facts(side: MarketSide, frame: i32) -> MarketCommandFacts {
        MarketCommandFacts {
            frame,
            can_buy_sell: (side == MarketSide::Buy).then_some(true),
            has_tribe_bonus_4: (side == MarketSide::Sell).then_some(false),
            has_preq_0x2ad: (side == MarketSide::Sell).then_some(true),
            has_market: (side == MarketSide::Sell).then_some(true),
            nuke_embargo: Some(0),
            selected_leader_who: None,
            display_who: 2,
        }
    }
}

impl Fleet for TransactionFleet {
    fn alive(&self, _who: u8, _o: i16) -> bool {
        false
    }

    fn is_unit(&self, _who: u8, _o: i16) -> bool {
        false
    }

    fn is_building(&self, _who: u8, _o: i16) -> bool {
        false
    }

    fn group_of(&self, _who: u8, _o: i16) -> i16 {
        -1
    }

    fn set_group_of(&mut self, _who: u8, _o: i16, _slot: i16) {}

    fn uid(&self, _who: u8, _o: i16) -> u16 {
        0xffff
    }

    fn pos(&self, _who: u8, _o: i16) -> (i32, i32) {
        (0, 0)
    }

    fn orders(&self, _who: u8, _o: i16) -> Option<&OrderQueue> {
        Some(&self.queue)
    }

    fn orders_mut(&mut self, _who: u8, _o: i16) -> Option<&mut OrderQueue> {
        Some(&mut self.queue)
    }

    fn set_stance(&mut self, _who: u8, _o: i16, _stance: i8) {}

    fn disband(&mut self, _who: u8, _o: i16) {}

    fn apply_direct_entity_command_transaction(
        &mut self,
        envelope: DirectEntityFleetRequest,
    ) -> DirectEntityFleetReceipt {
        match envelope {
            DirectEntityFleetRequest::Market { request, frame } => {
                let resource = usize::try_from(request.good).unwrap();
                let facts = Self::market_facts(request.side, frame);
                let receipt = match request.side {
                    MarketSide::Buy => execute_market_command(
                        request,
                        facts,
                        Some(MarketEconomyBinding::complete(
                            request.who,
                            request.good,
                            &self.rules,
                            &mut self.market,
                            &mut self.econ,
                            MarketCounterBinding::Demand(&mut self.demand[resource]),
                            MarketPriceGates::default(),
                        )),
                    ),
                    MarketSide::Sell => execute_market_command(
                        request,
                        facts,
                        Some(MarketEconomyBinding::complete(
                            request.who,
                            request.good,
                            &self.rules,
                            &mut self.market,
                            &mut self.econ,
                            MarketCounterBinding::Supply(&mut self.supply[resource]),
                            MarketPriceGates::default(),
                        )),
                    ),
                };
                DirectEntityFleetReceipt::Market(receipt)
            }
            DirectEntityFleetRequest::Entity { request, frame } => {
                let target = match request {
                    DirectEntityCommandRequest::Unqueue { uid, .. } => DirectEntityTargetFacts {
                        kind: DirectEntityKind::Unit,
                        active: false,
                        uid: uid as u16,
                    },
                    DirectEntityCommandRequest::ComeOut {
                        object_index, uid, ..
                    } => DirectEntityTargetFacts {
                        kind: DirectEntityKind::Unit,
                        active: true,
                        uid: if object_index == 4 {
                            (uid as u16).wrapping_add(1)
                        } else {
                            uid as u16
                        },
                    },
                };
                let type_facts = match request {
                    DirectEntityCommandRequest::ComeOut {
                        object_index: 5, ..
                    } => Some(DirectEntityTypeFacts {
                        kind: DirectEntityKind::Unit,
                        type_index: 50,
                    }),
                    _ => None,
                };
                let wrapper = match request {
                    DirectEntityCommandRequest::ComeOut {
                        who,
                        object_index: 5,
                        ..
                    } => u8::try_from(who).ok().and_then(|who| {
                        preflight_unit_action_come_out(
                            401,
                            402,
                            403,
                            404,
                            405,
                            UnitActionComeOutFacts {
                                actor: ObjectIdentity::new(who, 5),
                                actor_type: 50,
                                unit_masks: 0x0c00_0042,
                                inside: InsideLookupFacts::default(),
                                actor_first_guy: None,
                                actor_inside_down: None,
                                inside_chain: Vec::new(),
                                leader_flags: None,
                            },
                        )
                        .ok()
                    }),
                    _ => None,
                };
                DirectEntityFleetReceipt::Entity(match wrapper {
                    Some(preflight) => preflight_opcode49_unit_action_come_out_command(
                        request,
                        frame,
                        Some(target),
                        type_facts,
                        preflight,
                    ),
                    None => {
                        classify_direct_entity_command(request, frame, Some(target), type_facts)
                    }
                })
            }
        }
    }
}

fn market_wire(opcode: u8, who: i32, good: i32, flags: i32) -> Vec<u8> {
    let mut wire = vec![opcode];
    wire.extend_from_slice(&who.to_le_bytes());
    wire.extend_from_slice(&good.to_le_bytes());
    wire.extend_from_slice(&flags.to_le_bytes());
    wire
}

fn unqueue_wire(who: i32, object_index: i32, type_index: i32, uid: i16) -> Vec<u8> {
    let mut wire = vec![48];
    wire.extend_from_slice(&who.to_le_bytes());
    wire.extend_from_slice(&object_index.to_le_bytes());
    wire.extend_from_slice(&type_index.to_le_bytes());
    wire.extend_from_slice(&uid.to_le_bytes());
    wire
}

fn come_out_wire(who: i32, object_index: i32, uid: i16) -> Vec<u8> {
    let mut wire = vec![49];
    wire.extend_from_slice(&who.to_le_bytes());
    wire.extend_from_slice(&object_index.to_le_bytes());
    wire.extend_from_slice(&uid.to_le_bytes());
    wire
}

#[test]
fn buy_and_sell_dispatch_complete_atomic_economy_transactions() {
    let mut bridge = Bridge::new();
    bridge.frame = 81;
    let mut package = Package::new(0, 0);
    let mut fleet = TransactionFleet::new();
    let mut payload = market_wire(46, 2, RES_FOOD as i32, 0);
    payload.extend_from_slice(&market_wire(47, 2, RES_TIMBER as i32, 1));

    bridge
        .process_all(&mut package, &payload, &mut fleet)
        .unwrap();

    let receipts = bridge.take_direct_entity_receipts();
    assert_eq!(receipts.len(), 2);
    assert!(receipts.iter().all(|record| record.valid));
    assert!(receipts.iter().all(|record| matches!(
        &record.observed,
        DirectEntityFleetReceipt::Market(receipt)
            if receipt.status == MarketTransactionStatus::Applied
    )));
    assert_eq!(fleet.econ.stockpile[RES_FOOD], TRADE_LOT);
    assert_eq!(fleet.econ.stockpile[RES_TIMBER], 0);
    assert_eq!(bridge.stats.by_opcode[46], 1);
    assert_eq!(bridge.stats.by_opcode[47], 1);
    assert_eq!(InlineDef::find(46).unwrap().port, InlinePort::Complete);
    assert_eq!(InlineDef::find(47).unwrap().port, InlinePort::Complete);
}

#[test]
fn entity_dispatch_carries_a_validated_wrapper_prefix_without_completing_opcode49() {
    let mut bridge = Bridge::new();
    bridge.frame = 144;
    let mut package = Package::new(0, 0);
    let mut fleet = TransactionFleet::new();
    let mut payload = unqueue_wire(3, 9, 52, 70);
    payload.extend_from_slice(&come_out_wire(3, 4, 71));
    payload.extend_from_slice(&come_out_wire(3, 5, 72));

    bridge
        .process_all(&mut package, &payload, &mut fleet)
        .unwrap();

    let receipts = bridge.take_direct_entity_receipts();
    assert_eq!(receipts.len(), 3);
    assert!(receipts.iter().all(|record| record.valid));
    let statuses: Vec<_> = receipts
        .iter()
        .map(|record| match &record.observed {
            DirectEntityFleetReceipt::Entity(receipt) => receipt.status,
            DirectEntityFleetReceipt::Market(_) => unreachable!(),
        })
        .collect();
    assert_eq!(
        statuses.as_slice(),
        &[
            DirectEntityTransactionStatus::Complete,
            DirectEntityTransactionStatus::Complete,
            DirectEntityTransactionStatus::OpenTail,
        ]
    );
    let DirectEntityFleetReceipt::Entity(wrapper) = &receipts[2].observed else {
        unreachable!()
    };
    assert!(wrapper.unit_action_come_out.is_some());
    assert!(matches!(
        wrapper.disposition,
        Some(DirectEntityDisposition::OpenTail(
            DirectEntityOpenTail::GeneralUnitComeOutTransaction { argument: 0, .. }
        ))
    ));
    assert_eq!(InlineDef::find(48).unwrap().port, InlinePort::Complete);
    assert_eq!(InlineDef::find(49).unwrap().port, InlinePort::StateWired);
}
