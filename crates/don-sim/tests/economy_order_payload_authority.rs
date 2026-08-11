// SPDX-License-Identifier: GPL-3.0-or-later
//! Contract tests for the unregistered economy-order typed authority.

mod order {
    pub use don_sim::order::*;
}
pub use don_sim::Handle;

#[path = "../src/systems/economy_order_payload_authority.rs"]
mod subject;

use don_sim::order::OrderIndex;
use subject::*;

fn handle(id: u32, generation: u32) -> Handle {
    Handle { id, generation }
}

fn target(o: i32, who: i32, uid: u16, id: u32) -> StableTargetIdentity {
    StableTargetIdentity::live(o, who, uid, handle(id, id + 1000))
}

fn header(kind: OrderIndex) -> EconomyOrderHeader {
    EconomyOrderHeader {
        kind,
        flags: 0xa5,
        x: 0x1122_3344,
        y: -0x0102_0304,
        primary: target(0x1020_3040, 3, 0x5060, 71),
    }
}

fn gather_node(metric: u8) -> EconomyOrderNode {
    EconomyOrderNode {
        metric,
        header: header(OrderIndex::Gather),
        payload: EconomyOrderPayload::Gather(GatherOrderPayload {
            tx: 0x0102_0304,
            ty: -2,
            build_type: 0x293,
            wait: 0x1122_3344,
            goto_build: 1,
            non_flat_gather: 2,
            dist_mod: 3,
            been_there: 4,
        }),
    }
}

fn cast_node(metric: u8) -> EconomyOrderNode {
    EconomyOrderNode {
        metric,
        header: header(OrderIndex::CastSpell),
        payload: EconomyOrderPayload::CastSpell(CastOrderPayload {
            paid: 1,
            spell: 0x28a,
        }),
    }
}

fn trade_node(metric: u8) -> EconomyOrderNode {
    EconomyOrderNode {
        metric,
        header: header(OrderIndex::TradeRoute),
        payload: EconomyOrderPayload::TradeRoute(TradeOrderPayload {
            second: target(0x5566_7788, 7, 0x99aa, 93),
            started: 1,
            loaded: 0,
        }),
    }
}

#[test]
fn target_only_authority_covers_board_await_board_and_repair() {
    for kind in [
        OrderIndex::BoardShip,
        OrderIndex::AwaitBoard,
        OrderIndex::Repair,
    ] {
        let node = EconomyOrderNode {
            metric: 0x6d,
            header: header(kind),
            payload: EconomyOrderPayload::TargetOnly,
        };
        assert_eq!(node.retail_walked_bytes(), TARGET_ORDER_WALKED_BYTES);
        let walk = node.retail_walk_image().unwrap();
        assert_eq!(walk.len(), 11);
        assert_eq!(walk[0], 0xa5);
        assert_eq!(&walk[1..5], &0x1020_3040_i32.to_le_bytes());
        assert_eq!(&walk[5..9], &3_i32.to_le_bytes());
        assert_eq!(&walk[9..11], &0x5060_u16.to_le_bytes());
        assert_eq!(
            node.retail_node_image().unwrap().len(),
            TARGET_NODE_WALKED_BYTES
        );

        let leaf = encode_v13_leaf(DON_SAVE_V13, node).unwrap();
        assert_eq!(leaf.metric, 0x6d);
        assert_eq!(leaf.typed_payload, vec![EconomyPayloadTag::None as u8, 0]);
        assert_eq!(
            decode_v13_leaf(DON_SAVE_V13, leaf.metric, node.header, &leaf.typed_payload),
            Ok(node)
        );
    }
}

#[test]
fn gather_walk_and_v13_leaf_preserve_all_twenty_history_bytes() {
    let node = gather_node(0x4e);
    let walk = node.retail_walk_image().unwrap();
    assert_eq!(walk.len(), GATHER_ORDER_WALKED_BYTES);
    assert_eq!(&walk[11..15], &0x0102_0304_i32.to_le_bytes());
    assert_eq!(&walk[15..19], &(-2_i32).to_le_bytes());
    assert_eq!(&walk[19..23], &0x293_i32.to_le_bytes());
    assert_eq!(&walk[23..27], &0x1122_3344_i32.to_le_bytes());
    assert_eq!(&walk[27..31], &[1, 2, 3, 4]);

    let node_image = node.retail_node_image().unwrap();
    assert_eq!(node_image.len(), GATHER_NODE_WALKED_BYTES);
    assert_eq!(&node_image[..4], &(OrderIndex::Gather as i32).to_le_bytes());
    assert_eq!(node_image[4], 0x4e);

    let leaf = encode_v13_leaf(DON_SAVE_V13, node).unwrap();
    assert_eq!(leaf.metric, 0x4e);
    assert_eq!(leaf.typed_payload.len(), 2 + GATHER_SUFFIX_BYTES);
    assert_eq!(&leaf.typed_payload[..2], &[2, 1]);
    assert_eq!(
        decode_v13_leaf(DON_SAVE_V13, leaf.metric, node.header, &leaf.typed_payload),
        Ok(node)
    );
}

#[test]
fn cast_walk_uses_header_coordinates_while_leaf_owns_paid_and_spell() {
    let node = cast_node(0x93);
    let walk = node.retail_walk_image().unwrap();
    assert_eq!(walk.len(), CAST_ORDER_WALKED_BYTES);
    assert_eq!(
        node.retail_node_image().unwrap().len(),
        CAST_NODE_WALKED_BYTES
    );
    assert_eq!(&walk[11..15], &0x1122_3344_i32.to_le_bytes());
    assert_eq!(&walk[15..19], &(-0x0102_0304_i32).to_le_bytes());
    assert_eq!(&walk[19..23], &1_i32.to_le_bytes());
    assert_eq!(&walk[23..27], &0x28a_i32.to_le_bytes());

    let leaf = encode_v13_leaf(DON_SAVE_V13, node).unwrap();
    assert_eq!(leaf.metric, 0x93);
    assert_eq!(leaf.typed_payload.len(), 2 + CAST_SUFFIX_BYTES);
    assert_eq!(&leaf.typed_payload[..2], &[3, 1]);
    assert_eq!(&leaf.typed_payload[2..6], &1_i32.to_le_bytes());
    assert_eq!(&leaf.typed_payload[6..10], &0x28a_i32.to_le_bytes());
    assert_eq!(
        decode_v13_leaf(DON_SAVE_V13, leaf.metric, node.header, &leaf.typed_payload),
        Ok(node)
    );
}

#[test]
fn trade_walk_has_retail_field_order_and_v13_retains_second_handle() {
    let node = trade_node(0x17);
    let walk = node.retail_walk_image().unwrap();
    assert_eq!(walk.len(), TRADE_ORDER_WALKED_BYTES);
    assert_eq!(&walk[11..15], &0x5566_7788_i32.to_le_bytes());
    assert_eq!(&walk[15..19], &7_i32.to_le_bytes());
    assert_eq!(&walk[19..23], &1_i32.to_le_bytes());
    assert_eq!(&walk[23..27], &0_i32.to_le_bytes());
    assert_eq!(&walk[27..29], &0x99aa_u16.to_le_bytes());

    let node_image = node.retail_node_image().unwrap();
    assert_eq!(node_image.len(), TRADE_NODE_WALKED_BYTES);
    assert_eq!(node_image[4], 0x17);

    let leaf = encode_v13_leaf(DON_SAVE_V13, node).unwrap();
    assert_eq!(leaf.metric, 0x17);
    assert_eq!(leaf.typed_payload.len(), 2 + TRADE_SUFFIX_BYTES + 1 + 8);
    assert_eq!(&leaf.typed_payload[..2], &[4, 1]);
    assert_eq!(leaf.typed_payload[2 + TRADE_SUFFIX_BYTES], 1);
    assert_eq!(
        &leaf.typed_payload[2 + TRADE_SUFFIX_BYTES + 1..2 + TRADE_SUFFIX_BYTES + 5],
        &93_u32.to_le_bytes()
    );
    assert_eq!(
        decode_v13_leaf(DON_SAVE_V13, leaf.metric, node.header, &leaf.typed_payload),
        Ok(node)
    );

    // Stable port identity is additive: it never contaminates retail's checksum image.
    let mut changed = node;
    if let EconomyOrderPayload::TradeRoute(ref mut trade) = changed.payload {
        trade.second.handle = Some(handle(93, 9_999));
    }
    assert_eq!(
        changed.retail_node_image().unwrap(),
        node.retail_node_image().unwrap()
    );
    assert_ne!(
        encode_v13_leaf(DON_SAVE_V13, changed).unwrap(),
        encode_v13_leaf(DON_SAVE_V13, node).unwrap()
    );
}

#[test]
fn unresolved_trade_destination_round_trips_only_as_exact_retail_sentinel() {
    let mut node = trade_node(2);
    if let EconomyOrderPayload::TradeRoute(ref mut trade) = node.payload {
        trade.second = StableTargetIdentity::NONE;
    }
    let leaf = encode_v13_leaf(DON_SAVE_V13, node).unwrap();
    assert_eq!(leaf.typed_payload.len(), 2 + TRADE_SUFFIX_BYTES + 1);
    assert_eq!(leaf.typed_payload.last(), Some(&0));
    assert_eq!(
        decode_v13_leaf(DON_SAVE_V13, leaf.metric, node.header, &leaf.typed_payload),
        Ok(node)
    );
}

#[test]
fn every_supported_node_preserves_its_nonzero_metric() {
    let target_node = EconomyOrderNode {
        metric: 0xf1,
        header: header(OrderIndex::Repair),
        payload: EconomyOrderPayload::TargetOnly,
    };
    for node in [
        target_node,
        gather_node(0xf2),
        cast_node(0xf3),
        trade_node(0xf4),
    ] {
        let image = node.retail_node_image().unwrap();
        assert_eq!(image[4], node.metric);
        let leaf = encode_v13_leaf(DON_SAVE_V13, node).unwrap();
        assert_eq!(leaf.metric, node.metric);
        assert_eq!(
            decode_v13_leaf(DON_SAVE_V13, leaf.metric, node.header, &leaf.typed_payload),
            Ok(node)
        );
    }
}

#[test]
fn payload_kind_crosses_are_rejected_before_serialization() {
    let wrong = EconomyOrderNode {
        metric: 0,
        header: header(OrderIndex::Repair),
        payload: EconomyOrderPayload::Gather(GatherOrderPayload::default()),
    };
    assert_eq!(
        encode_v13_leaf(DON_SAVE_V13, wrong),
        Err(EconomyOrderAuthorityError::PayloadDoesNotMatchKind {
            kind: OrderIndex::Repair,
            tag: EconomyPayloadTag::Gather,
        })
    );

    let unsupported = EconomyOrderNode {
        metric: 0,
        header: header(OrderIndex::Attack),
        payload: EconomyOrderPayload::TargetOnly,
    };
    assert_eq!(
        unsupported.retail_node_image(),
        Err(EconomyOrderAuthorityError::UnsupportedOrderKind(
            OrderIndex::Attack
        ))
    );
}

#[test]
fn decoder_fails_closed_on_unknown_tag_version_and_foreign_history() {
    let gather = gather_node(5);
    let mut bytes = encode_v13_leaf(DON_SAVE_V13, gather).unwrap().typed_payload;
    bytes[0] = 0xfe;
    assert_eq!(
        decode_v13_leaf(DON_SAVE_V13, gather.metric, gather.header, &bytes),
        Err(EconomyOrderAuthorityError::UnknownPayloadTag(0xfe))
    );

    let mut bytes = encode_v13_leaf(DON_SAVE_V13, gather).unwrap().typed_payload;
    bytes[1] = 2;
    assert_eq!(
        decode_v13_leaf(DON_SAVE_V13, gather.metric, gather.header, &bytes),
        Err(EconomyOrderAuthorityError::UnknownPayloadVersion {
            tag: EconomyPayloadTag::Gather,
            version: 2,
        })
    );

    let cast = cast_node(5);
    let bytes = encode_v13_leaf(DON_SAVE_V13, cast).unwrap().typed_payload;
    assert_eq!(
        decode_v13_leaf(DON_SAVE_V13, gather.metric, gather.header, &bytes),
        Err(EconomyOrderAuthorityError::ForeignPayloadTag {
            kind: OrderIndex::Gather,
            expected: EconomyPayloadTag::Gather,
            actual: EconomyPayloadTag::CastSpell,
        })
    );
}

#[test]
fn decoder_rejects_every_truncation_and_trailing_byte() {
    for node in [gather_node(9), cast_node(9), trade_node(9)] {
        let bytes = encode_v13_leaf(DON_SAVE_V13, node).unwrap().typed_payload;
        for end in 0..bytes.len() {
            assert!(
                decode_v13_leaf(DON_SAVE_V13, node.metric, node.header, &bytes[..end]).is_err()
            );
        }
        let mut trailing = bytes;
        trailing.push(0);
        assert!(decode_v13_leaf(DON_SAVE_V13, node.metric, node.header, &trailing).is_err());
    }
}

#[test]
fn malformed_or_missing_handles_are_not_normalized() {
    let mut gather = gather_node(1);
    gather.header.primary.handle = None;
    assert_eq!(
        encode_v13_leaf(DON_SAVE_V13, gather),
        Err(EconomyOrderAuthorityError::IncoherentTargetIdentity)
    );

    let mut trade = trade_node(1);
    if let EconomyOrderPayload::TradeRoute(ref mut payload) = trade.payload {
        payload.second.handle = None;
    }
    assert_eq!(
        encode_v13_leaf(DON_SAVE_V13, trade),
        Err(EconomyOrderAuthorityError::IncoherentTargetIdentity)
    );

    let mut cast = cast_node(1);
    cast.header.primary = StableTargetIdentity {
        o: -2,
        who: -1,
        uid: u16::MAX,
        handle: None,
    };
    assert_eq!(
        encode_v13_leaf(DON_SAVE_V13, cast),
        Err(EconomyOrderAuthorityError::IncoherentTargetIdentity)
    );

    let valid = trade_node(1);
    let mut bytes = encode_v13_leaf(DON_SAVE_V13, valid).unwrap().typed_payload;
    bytes[2 + TRADE_SUFFIX_BYTES] = 2;
    assert_eq!(
        decode_v13_leaf(DON_SAVE_V13, valid.metric, valid.header, &bytes),
        Err(EconomyOrderAuthorityError::InvalidHandlePresence(2))
    );
}

#[test]
fn codec_accepts_only_the_proven_v13_envelope() {
    let node = gather_node(0);
    assert_eq!(
        encode_v13_leaf(12, node),
        Err(EconomyOrderAuthorityError::UnsupportedDoNSaveVersion(12))
    );
    assert_eq!(
        decode_v13_leaf(14, node.metric, node.header, &[2, 1]),
        Err(EconomyOrderAuthorityError::UnsupportedDoNSaveVersion(14))
    );
}
