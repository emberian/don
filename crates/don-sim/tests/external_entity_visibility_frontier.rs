pub use don_sim::Handle;

#[path = "../src/systems/external_entity_visibility_frontier.rs"]
mod external_entity_visibility_frontier;

use external_entity_visibility_frontier::*;

fn viewer(who: u8) -> RetailViewerFacts {
    RetailViewerFacts {
        who,
        vision_mask: 1 << who,
        see_all: false,
        reveal_counter: 0,
        see_own_territory: false,
        allied_territory_mask: 1 << who,
    }
}

fn row(id: u32, who: u8, object_o: i16) -> ExternalUnitFrameRow {
    ExternalUnitFrameRow {
        identity: ExternalEntityIdentity {
            handle: Handle { id, generation: 3 },
            who,
            object_o,
            uid: 100 + id as u16,
        },
        public: ExternalEntityPublicState {
            type_id: 50 + id as i32,
            x: 1000 + id as i32,
            y: 2000 + id as i32,
            hits: 300,
            angle: 400,
            speed: 12,
            recharge: 5,
            order_index: 7,
        },
        visibility: RetailUnitVisibilityFacts {
            active: true,
            unit_masks: 0,
            type_unit_flags: 0,
            unit_masks2: 0,
            has_order: true,
            object_visible_mask: 0,
            cell_seen_mask: 0xff,
            cell_detected_mask: 0,
            territory_owner: -1,
        },
    }
}

fn frame(rows: Vec<ExternalUnitFrameRow>) -> ExternalVisibilityFrame {
    ExternalVisibilityFrame {
        frame: 90,
        fog_option: 0,
        rows,
        viewers: vec![viewer(0), viewer(1), viewer(2)],
    }
}

#[test]
fn shipped_entry_points_offsets_and_cloak_bits_are_frozen() {
    assert_eq!(UNIT_IS_SEEN_VA, 0x0060_7a60);
    assert_eq!(UNIT_IS_DETECTED_VA, 0x0060_a630);
    assert_eq!(UNIT_IS_CLOAKED_VA, 0x0060_a6a0);
    assert_eq!(WORLD_IS_DETECTED_VA, 0x006b_48c0);
    assert_eq!(WORLD_IS_SEEN_VA, 0x006b_55c0);
    assert_eq!(UNIT_MASK_CLOAK, 0x800);
    assert_eq!(UNIT_MASK_DETECTION_BYPASS, 0x1000);
    assert_eq!(TYPE_UNIT_FLAG_CLOAK, 0x4000);
    assert_eq!(TYPE_UNIT_FLAG_CLOAK_WHILE_IDLE, 0x40000);
    assert_eq!(UNIT_MASK2_CLOAK, 0x8000);
}

#[test]
fn all_four_cloak_sources_and_the_idle_order_gate_are_distinct() {
    let mut facts = row(1, 1, 3).visibility;
    facts.unit_masks = UNIT_MASK_CLOAK;
    assert!(facts.is_cloaked());
    facts.unit_masks = 0;
    facts.type_unit_flags = TYPE_UNIT_FLAG_CLOAK;
    assert!(facts.is_cloaked());
    facts.type_unit_flags = 0;
    facts.unit_masks2 = UNIT_MASK2_CLOAK;
    assert!(facts.is_cloaked());
    facts.unit_masks2 = 0;
    facts.type_unit_flags = TYPE_UNIT_FLAG_CLOAK_WHILE_IDLE;
    facts.has_order = false;
    assert!(facts.is_cloaked());
    facts.has_order = true;
    assert!(!facts.is_cloaked());
}

#[test]
fn cloak_requires_the_dedicated_allied_detection_plane_even_with_see_all() {
    let mut target = row(1, 1, 3);
    target.visibility.unit_masks = UNIT_MASK_CLOAK;
    let mut observer = viewer(0);
    observer.see_all = true;
    observer.vision_mask = 0b0000_0101;

    assert_eq!(
        visibility_decision(&target, &observer, 3),
        VisibilityDecision::HiddenUndetectedCloak,
        "neither see-all nor no-fog mode grants detection"
    );
    target.visibility.cell_detected_mask = 0b0000_0100;
    assert_eq!(
        visibility_decision(&target, &observer, 0),
        VisibilityDecision::Visible,
        "detector coverage shared through LeaderData::ally_mask admits the cloak"
    );
    target.visibility.cell_detected_mask = 0;
    target.visibility.unit_masks |= UNIT_MASK_DETECTION_BYPASS;
    assert_eq!(
        visibility_decision(&target, &observer, 0),
        VisibilityDecision::Visible
    );
}

#[test]
fn current_fog_reveal_counter_territory_and_object_override_remain_separate() {
    let mut target = row(1, 1, 3);
    target.visibility.cell_seen_mask = 0;
    let mut observer = viewer(0);
    assert_eq!(
        visibility_decision(&target, &observer, 0),
        VisibilityDecision::HiddenByCurrentFog
    );

    target.visibility.object_visible_mask = 1;
    assert_eq!(
        visibility_decision(&target, &observer, 0),
        VisibilityDecision::Visible
    );
    target.visibility.object_visible_mask = 0;
    observer.reveal_counter = 1;
    assert_eq!(
        visibility_decision(&target, &observer, 0),
        VisibilityDecision::Visible
    );
    observer.reveal_counter = 0;
    observer.see_own_territory = true;
    observer.allied_territory_mask = 0b0000_0011;
    target.visibility.territory_owner = 1;
    assert_eq!(
        visibility_decision(&target, &observer, 0),
        VisibilityDecision::Visible
    );
}

#[test]
fn projection_is_external_only_canonical_and_one_based() {
    let mut hidden = row(7, 2, 4);
    hidden.visibility.cell_seen_mask = 0;
    let own = row(9, 0, 2);
    let low_handle = row(2, 1, 8);
    let high_handle = row(6, 2, 5);
    let mut owner = ExternalEntityVisibilityOwner::default();
    owner
        .install_frame(frame(vec![hidden, own, high_handle, low_handle]))
        .unwrap();

    let projection = owner.project(0).unwrap();
    assert_eq!(
        projection
            .rows
            .iter()
            .map(|entry| (entry.ordinal, entry.identity.handle.id))
            .collect::<Vec<_>>(),
        [(1, 2), (2, 6)]
    );
    assert!(projection.rows.iter().all(|entry| entry.identity.who != 0));
}

#[test]
fn dense_row_compaction_order_cannot_change_ordinals_or_digest() {
    let rows = vec![row(8, 2, 2), row(3, 1, 7), row(5, 2, 9)];
    let mut a = ExternalEntityVisibilityOwner::default();
    let mut b = ExternalEntityVisibilityOwner::default();
    a.install_frame(frame(rows.clone())).unwrap();
    b.install_frame(frame(vec![rows[2], rows[0], rows[1]]))
        .unwrap();

    assert_eq!(a.digest().unwrap(), b.digest().unwrap());
    assert_eq!(a.project(0).unwrap().rows, b.project(0).unwrap().rows);
}

#[test]
fn duplicate_handle_or_retail_identity_is_rejected_atomically() {
    let mut owner = ExternalEntityVisibilityOwner::default();
    owner.install_frame(frame(vec![row(1, 1, 1)])).unwrap();
    let revision = owner.revision();
    let digest = owner.digest().unwrap();

    let first = row(2, 1, 2);
    let mut duplicate_handle = row(2, 2, 3);
    duplicate_handle.identity.uid += 1;
    assert!(matches!(
        owner.install_frame(frame(vec![first, duplicate_handle])),
        Err(VisibilityInstallFault::DuplicateHandle { .. })
    ));
    assert_eq!(owner.revision(), revision);
    assert_eq!(owner.digest().unwrap(), digest);

    let first = row(4, 1, 4);
    let mut duplicate_retail = row(5, 1, 4);
    duplicate_retail.identity.uid = first.identity.uid;
    assert!(matches!(
        owner.install_frame(frame(vec![first, duplicate_retail])),
        Err(VisibilityInstallFault::DuplicateRetailIdentity { .. })
    ));
    assert_eq!(owner.revision(), revision);
    assert_eq!(owner.digest().unwrap(), digest);
}

#[test]
fn target_binding_is_identity_and_revision_bound_across_refresh_and_reset() {
    let mut owner = ExternalEntityVisibilityOwner::default();
    owner.install_frame(frame(vec![row(1, 1, 1)])).unwrap();
    let binding = owner.bind_target(0, 1).unwrap();
    assert_eq!(binding.identity().handle.id, 1);
    assert_eq!(
        owner.revalidate_target(binding).unwrap().identity.handle.id,
        1
    );

    owner.install_frame(frame(vec![row(1, 1, 1)])).unwrap();
    assert!(matches!(
        owner.revalidate_target(binding),
        Err(VisibilityProjectionFault::StaleBinding { .. })
    ));
    let refreshed = owner.bind_target(0, 1).unwrap();
    owner.reset().unwrap();
    assert_eq!(
        owner.project(0),
        Err(VisibilityProjectionFault::Uninstalled)
    );
    assert!(matches!(
        owner.revalidate_target(refreshed),
        Err(VisibilityProjectionFault::StaleBinding { .. })
    ));
    assert_eq!(owner.digest(), Err(VisibilityProjectionFault::Uninstalled));
}

#[test]
fn content_digest_covers_public_cloak_detection_fog_and_viewer_inputs() {
    let baseline = frame(vec![row(1, 1, 1)]);
    let digest = |candidate: ExternalVisibilityFrame| {
        let mut owner = ExternalEntityVisibilityOwner::default();
        owner.install_frame(candidate).unwrap();
        owner.digest().unwrap()
    };
    let expected = digest(baseline.clone());
    let mut variants = Vec::new();

    let mut v = baseline.clone();
    v.frame += 1;
    variants.push(v);
    let mut v = baseline.clone();
    v.rows[0].identity.handle.generation += 1;
    variants.push(v);
    let mut v = baseline.clone();
    v.rows[0].public.hits += 1;
    variants.push(v);
    let mut v = baseline.clone();
    v.rows[0].visibility.unit_masks ^= UNIT_MASK_CLOAK;
    variants.push(v);
    let mut v = baseline.clone();
    v.rows[0].visibility.type_unit_flags ^= TYPE_UNIT_FLAG_CLOAK;
    variants.push(v);
    let mut v = baseline.clone();
    v.rows[0].visibility.unit_masks2 ^= UNIT_MASK2_CLOAK;
    variants.push(v);
    let mut v = baseline.clone();
    v.rows[0].visibility.cell_seen_mask ^= 1;
    variants.push(v);
    let mut v = baseline.clone();
    v.rows[0].visibility.cell_detected_mask ^= 1;
    variants.push(v);
    let mut v = baseline.clone();
    v.viewers[0].see_all = true;
    variants.push(v);
    let mut v = baseline;
    v.viewers[0].vision_mask |= 2;
    variants.push(v);

    assert!(variants
        .into_iter()
        .all(|variant| digest(variant) != expected));
}
