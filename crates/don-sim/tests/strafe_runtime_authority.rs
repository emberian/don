// SPDX-License-Identifier: GPL-3.0-or-later

mod systems {
    pub mod air {
        pub use don_sim::systems::air::AirOrderWalk;
    }
    pub mod patrol {
        pub use don_sim::systems::patrol::StrafeOrder;
    }
}

#[path = "../src/systems/strafe_runtime_authority.rs"]
mod authority;

use authority::*;
use systems::{air::AirOrderWalk, patrol::StrafeOrder};

fn order() -> StrafeOrder {
    StrafeOrder {
        target_o: 2_017,
        target_who: 3,
        target_uid: 0xabcd,
        def_x: -101,
        def_y: 202,
        mandatory: 1,
        defensive: 0,
        in_range: 1,
        ever_in_range: 1,
        new_ord: 0,
        air: AirOrderWalk {
            oxx: 2_009,
            whose: 3,
            cruising_alt: 0x640,
            sharp_turn: 1,
            old: -7,
            returning: 0,
        },
        xx: 30_001,
        yy: -40_002,
    }
}

#[test]
fn retail_walk_is_exactly_57_bytes_and_visits_flags_twice() {
    let order = order();
    let bytes = retail_walk_bytes(0xa5, &order).unwrap();
    assert_eq!(bytes.len(), STRAFE_RETAIL_WALK_BYTES);
    assert_eq!(bytes[0], 0xa5);
    assert_eq!(&bytes[1..5], &order.target_o.to_le_bytes());
    assert_eq!(&bytes[5..9], &order.target_who.to_le_bytes());
    assert_eq!(&bytes[9..11], &order.target_uid.to_le_bytes());
    assert_eq!(bytes[24], 0xa5);
    assert_eq!(&bytes[25..29], &order.air.oxx.to_le_bytes());
    assert_eq!(&bytes[49..53], &order.xx.to_le_bytes());
    assert_eq!(&bytes[53..57], &order.yy.to_le_bytes());
}

#[test]
fn v13_tag_eight_leaf_round_trips_every_non_header_field() {
    let before = order();
    let bytes = encode_strafe_leaf(&before).unwrap();
    assert_eq!(bytes.len(), STRAFE_LEAF_BYTES);
    assert_eq!(bytes[0], DON_SAVE_STRAFE_TAG);
    assert_eq!(bytes[1], STRAFE_PAYLOAD_VERSION);
    let after = decode_strafe_leaf(target_identity(&before), &bytes).unwrap();
    assert_eq!(after, before);
    assert_eq!(encode_strafe_leaf(&after).unwrap(), bytes);
}

#[test]
fn targetless_return_preserves_the_retail_uid_word() {
    let mut before = order();
    before.target_o = -1;
    before.target_who = -1;
    before.target_uid = 0xbeef;
    before.air.returning = 1;
    let bytes = encode_strafe_leaf(&before).unwrap();
    assert_eq!(
        decode_strafe_leaf(target_identity(&before), &bytes).unwrap(),
        before
    );
}

#[test]
fn all_five_boolean_bytes_are_independently_validated() {
    let mutations: [(&str, fn(&mut StrafeOrder)); 5] = [
        ("mandatory", |o| o.mandatory = 2),
        ("defensive", |o| o.defensive = 2),
        ("in_range", |o| o.in_range = 2),
        ("ever_in_range", |o| o.ever_in_range = 2),
        ("new_ord", |o| o.new_ord = 2),
    ];
    for (field, mutate) in mutations {
        let mut bad = order();
        mutate(&mut bad);
        assert_eq!(
            encode_strafe_leaf(&bad),
            Err(StrafeAuthorityError::NonCanonicalFlag { field, value: 2 })
        );
    }
}

#[test]
fn target_and_home_addresses_fail_closed_without_normalization() {
    let mut bad_target = order();
    bad_target.target_o = -1;
    assert_eq!(
        encode_strafe_leaf(&bad_target),
        Err(StrafeAuthorityError::IncoherentTargetAddress { o: -1, who: 3 })
    );

    let mut bad_home = order();
    bad_home.air.whose = -1;
    assert_eq!(
        encode_strafe_leaf(&bad_home),
        Err(StrafeAuthorityError::IncoherentHomeAddress { o: 2_009, who: -1 })
    );
}

#[test]
fn envelope_tag_version_truncation_and_trailing_mutations_refuse() {
    let order = order();
    let valid = encode_strafe_leaf(&order).unwrap();

    let mut wrong_tag = valid.clone();
    wrong_tag[0] = 7;
    assert_eq!(
        decode_strafe_leaf(target_identity(&order), &wrong_tag),
        Err(StrafeAuthorityError::WrongPayloadTag(7))
    );

    let mut future = valid.clone();
    future[1] = 2;
    assert_eq!(
        decode_strafe_leaf(target_identity(&order), &future),
        Err(StrafeAuthorityError::WrongPayloadVersion(2))
    );

    assert_eq!(
        decode_strafe_leaf(target_identity(&order), &valid[..valid.len() - 1]),
        Err(StrafeAuthorityError::Truncated)
    );

    let mut trailing = valid;
    trailing.push(0);
    assert_eq!(
        decode_strafe_leaf(target_identity(&order), &trailing),
        Err(StrafeAuthorityError::TrailingBytes {
            expected: STRAFE_LEAF_BYTES,
            actual: STRAFE_LEAF_BYTES + 1,
        })
    );
}

#[test]
fn every_fixed_leaf_field_owns_distinct_bytes() {
    let baseline = order();
    let base = encode_strafe_leaf(&baseline).unwrap();
    let mutations: [fn(&mut StrafeOrder); 15] = [
        |o| o.def_x ^= 1,
        |o| o.def_y ^= 1,
        |o| o.mandatory ^= 1,
        |o| o.defensive ^= 1,
        |o| o.in_range ^= 1,
        |o| o.ever_in_range ^= 1,
        |o| o.new_ord ^= 1,
        |o| o.air.oxx ^= 1,
        |o| o.air.whose ^= 1,
        |o| o.air.cruising_alt ^= 1,
        |o| o.air.sharp_turn ^= 1,
        |o| o.air.old ^= 1,
        |o| o.air.returning ^= 1,
        |o| o.xx ^= 1,
        |o| o.yy ^= 1,
    ];
    let mut images = std::collections::BTreeSet::new();
    for mutate in mutations {
        let mut changed = baseline.clone();
        mutate(&mut changed);
        let image = encode_strafe_leaf(&changed).unwrap();
        assert_ne!(image, base);
        assert!(images.insert(image));
    }
    assert_eq!(images.len(), mutations.len());
}
