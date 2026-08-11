//! Entry-owner provenance for the automatic replay place-all survey.

use don_replay::initial::replay_place_all_owners::{
    ReplayPlaceAllMountainOwnerBoundary, ReplayPlaceAllOwnerInitialization,
    MOUNTAIN_TEMPLATE_PRODUCER_VA,
};
use don_replay::replay_goods_initial::{
    InitialGoodsStorageSource, OBJECTS_CLEAR_VA, OBJECTS_INIT_VA,
};

#[test]
fn automatic_entry_owns_cold_goods_but_requires_explicit_installed_mountain_templates() {
    let initialization = ReplayPlaceAllOwnerInitialization::cold_process();
    let goods = initialization.goods_initialization();

    assert_eq!(goods.objects_init_va, OBJECTS_INIT_VA);
    assert_eq!(goods.objects_clear_va, OBJECTS_CLEAR_VA);
    assert_eq!(goods.storage_source, InitialGoodsStorageSource::ColdPeImage);
    assert_eq!(goods.state.array.length, 0);
    assert_eq!(goods.state.array.capacity, 0);
    assert_eq!(goods.state.good_mark, 0);
    assert_eq!(goods.state.active_count, 0);
    assert_eq!(goods.state.goods_checksum, 1);
    assert_eq!(
        initialization.mountain_boundary(),
        ReplayPlaceAllMountainOwnerBoundary::MissingInstalledDisplacementTemplates {
            producer_va: MOUNTAIN_TEMPLATE_PRODUCER_VA,
            required_source: "user-owned effects_graphics.xml and referenced displacement TGAs",
        }
    );

    let mut entry = initialization.entry_owners(0);
    assert!(entry.mountains.is_none());
    let entry_goods = entry.oil_goods.as_mut().expect("exact Goods owner");
    entry_goods.capacity = 32;

    // The read-only survey receives a clone. Its speculative owner execution
    // cannot rewrite the initialization evidence retained by this carrier.
    assert_eq!(
        initialization.goods_runtime().goods().capacity,
        0,
        "entry owner must be an isolated snapshot"
    );
}
