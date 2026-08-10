// SPDX-License-Identifier: GPL-3.0-or-later
//! Mutation-sensitive source-only proof for the first XML tranche in
//! `Map::place_resources`.

#[path = "../src/place_resources_xml_frontier.rs"]
mod place_resources_xml_frontier;

use don_sim::systems::map_terrain::World;
use place_resources_xml_frontier::{
    execute_place_resources_xml_frontier, BonusXmlRowFact, BonusesSectionSource,
    PlaceResourcesXmlEntryHandoff, PlaceResourcesXmlError, PlaceResourcesXmlEvidence,
    PlaceResourcesXmlFacts, XmlHostHandles, XmlHostRefOperationKind, XmlSectionFact,
    BONUS_GET_ELEMENTS_CALL_VA, CATEGORY_CURRENT_HEAD_RELEASE_CALL_VA,
    CATEGORY_CURRENT_TAIL_RELEASE_CALL_VA, DEFAULT_BONUSES_GET_ELEMENT_CALL_VA,
    DEFAULT_DOCUMENT_HEAD_RELEASE_CALL_VA, DEFAULT_DOCUMENT_TAIL_RELEASE_CALL_VA,
    DEFAULT_STYLE_PATH, FIRST_BONUS_ROW_BODY_VA, PLACE_RESOURCES_POOL_RESIDUAL_VA,
    PLACE_RESOURCES_XML_RESIDUAL_VA, ROW_CURRENT_HEAD_RELEASE_CALL_VA,
    ROW_CURRENT_TAIL_RELEASE_CALL_VA, SELECTED_BONUSES_GET_ELEMENT_CALL_VA,
    SELECTED_DOCUMENT_HEAD_RELEASE_CALL_VA, SELECTED_DOCUMENT_TAIL_RELEASE_CALL_VA,
};

fn entry() -> PlaceResourcesXmlEntryHandoff {
    PlaceResourcesXmlEntryHandoff {
        entry_va: PLACE_RESOURCES_POOL_RESIDUAL_VA,
        player_count_argument: 4,
        random_state: 0x1234_5678,
        world_checksum: World::init_default_rules(4, 4).checksum_sections(),
        sourced_walked_bytes: 52,
        resource_pool_digest: 0x11bb_22aa_33dd_44cc,
    }
}

fn handles(serial: u8) -> XmlHostHandles {
    XmlHostHandles {
        head: serial & 1 != 0,
        tail: serial & 2 != 0,
        inline_tail_word: serial & 4 != 0,
    }
}

fn section(rows: &[u32], section_handles: XmlHostHandles) -> XmlSectionFact {
    XmlSectionFact {
        element_name: "BONUSES".to_owned(),
        handles: section_handles,
        bonus_rows: rows
            .iter()
            .map(|ordinal| BonusXmlRowFact {
                capture_ordinal: *ordinal,
                element_name: "BONUS".to_owned(),
                handles: handles((*ordinal & 7) as u8),
            })
            .collect(),
    }
}

fn facts(entry: &PlaceResourcesXmlEntryHandoff) -> PlaceResourcesXmlFacts {
    PlaceResourcesXmlFacts {
        selected_style_name: "eastwest.xml".to_owned(),
        selected_bonuses: Some(section(&[7, 3, 11], handles(3))),
        default_bonuses: Some(section(&[100, 101], handles(5))),
        evidence: PlaceResourcesXmlEvidence::SyntheticFixture {
            fixture: "place-resources-xml-frontier".to_owned(),
            entry_va: entry.entry_va,
            random_state: entry.random_state,
            world_checksum: entry.world_checksum.clone(),
            sourced_walked_bytes: entry.sourced_walked_bytes,
            resource_pool_digest: entry.resource_pool_digest,
        },
    }
}

#[test]
fn selected_bonuses_preserve_row_order_and_exact_typed_seam() {
    let entry = entry();
    let facts = facts(&entry);
    let mut host = XmlHostHandles::default();
    let out = execute_place_resources_xml_frontier(&mut host, &entry, &facts).unwrap();

    assert_eq!(out.receipt.residual_va, PLACE_RESOURCES_XML_RESIDUAL_VA);
    assert_eq!(out.receipt.selected_style_path, "eastwest.xml");
    assert_eq!(out.receipt.default_style_path, DEFAULT_STYLE_PATH);
    assert_eq!(out.receipt.suffix_delete_call_va, Some(0x0068_f68c));
    assert_eq!(
        out.receipt.selected_get_element_call_va,
        Some(SELECTED_BONUSES_GET_ELEMENT_CALL_VA)
    );
    assert_eq!(out.receipt.default_get_element_call_va, None);
    assert_eq!(out.receipt.get_elements_call_va, BONUS_GET_ELEMENTS_CALL_VA);
    assert_eq!(
        out.receipt.section_source,
        BonusesSectionSource::SelectedStyle
    );
    assert_eq!(out.receipt.row_count, 3);
    assert_eq!(
        out.handoff
            .rows
            .iter()
            .map(|row| row.capture_ordinal)
            .collect::<Vec<_>>(),
        vec![7, 3, 11]
    );
    assert_eq!(out.handoff.resume_va, PLACE_RESOURCES_XML_RESIDUAL_VA);
    assert_eq!(out.handoff.first_row_body_va, FIRST_BONUS_ROW_BODY_VA);
    assert_eq!(out.handoff.player_count_argument, 4);
    assert!(out.handoff.selected_document_live);
    assert!(out.handoff.default_document_live);
    assert_eq!(host, handles(3));
}

#[test]
fn absent_selected_section_falls_back_to_default_and_reassigns_host_refs() {
    let entry = entry();
    let mut facts = facts(&entry);
    facts.selected_bonuses = None;
    let mut host = handles(3);
    let out = execute_place_resources_xml_frontier(&mut host, &entry, &facts).unwrap();

    assert_eq!(
        out.receipt.section_source,
        BonusesSectionSource::DefaultStyle
    );
    assert_eq!(
        out.receipt.default_get_element_call_va,
        Some(DEFAULT_BONUSES_GET_ELEMENT_CALL_VA)
    );
    assert_eq!(
        out.handoff
            .rows
            .iter()
            .map(|row| row.capture_ordinal)
            .collect::<Vec<_>>(),
        vec![100, 101]
    );
    assert_eq!(host, handles(5));
    assert_eq!(
        out.receipt
            .host_ref_operations
            .iter()
            .map(|op| op.kind)
            .collect::<Vec<_>>(),
        vec![
            XmlHostRefOperationKind::ReleaseOldTail,
            XmlHostRefOperationKind::ReleaseOldHead,
            XmlHostRefOperationKind::AcquireNewHead,
        ]
    );
    assert_eq!(
        out.receipt.host_ref_operations[0].assignment_call_va,
        SELECTED_BONUSES_GET_ELEMENT_CALL_VA
    );
    assert_eq!(
        out.receipt.host_ref_operations[2].assignment_call_va,
        DEFAULT_BONUSES_GET_ELEMENT_CALL_VA
    );
}

#[test]
fn valid_empty_selected_section_does_not_fall_back() {
    let entry = entry();
    let mut facts = facts(&entry);
    facts.selected_bonuses = Some(section(&[], handles(4)));
    let mut host = XmlHostHandles::default();
    let out = execute_place_resources_xml_frontier(&mut host, &entry, &facts).unwrap();

    assert_eq!(
        out.receipt.section_source,
        BonusesSectionSource::SelectedStyle
    );
    assert_eq!(out.receipt.row_count, 0);
    assert_eq!(out.receipt.default_get_element_call_va, None);
    assert_eq!(host, handles(4));
}

#[test]
fn empty_style_name_still_initializes_dot_xml_but_selects_default_directly() {
    let entry = entry();
    let mut facts = facts(&entry);
    facts.selected_style_name.clear();
    let mut host = XmlHostHandles::default();
    let out = execute_place_resources_xml_frontier(&mut host, &entry, &facts).unwrap();

    assert_eq!(out.receipt.selected_style_path, ".xml");
    assert_eq!(out.receipt.suffix_delete_call_va, None);
    assert_eq!(out.receipt.selected_get_element_call_va, None);
    assert_eq!(
        out.receipt.default_get_element_call_va,
        Some(DEFAULT_BONUSES_GET_ELEMENT_CALL_VA)
    );
    assert_eq!(
        out.receipt.section_source,
        BonusesSectionSource::DefaultStyle
    );
}

#[test]
fn terminal_dot_is_not_the_native_suffix_search_start() {
    let entry = entry();
    let mut facts = facts(&entry);
    facts.selected_style_name = "eastwest.".to_owned();
    let mut host = XmlHostHandles::default();
    let out = execute_place_resources_xml_frontier(&mut host, &entry, &facts).unwrap();
    assert_eq!(out.receipt.selected_style_path, "eastwest..xml");
    assert_eq!(out.receipt.suffix_delete_call_va, None);
}

#[test]
fn leading_dot_is_seen_when_it_is_not_the_only_code_unit() {
    let entry = entry();
    let mut facts = facts(&entry);
    facts.selected_style_name = ".hidden".to_owned();
    let mut host = XmlHostHandles::default();
    let out = execute_place_resources_xml_frontier(&mut host, &entry, &facts).unwrap();
    assert_eq!(out.receipt.selected_style_path, ".xml");
    assert_eq!(out.receipt.suffix_delete_call_va, Some(0x0068_f68c));
}

#[test]
fn xml_frontier_has_zero_rng_world_or_pool_mutation_and_freezes_pending_tails() {
    let entry = entry();
    let facts = facts(&entry);
    let mut host = XmlHostHandles::default();
    let out = execute_place_resources_xml_frontier(&mut host, &entry, &facts).unwrap();

    assert_eq!(out.receipt.random_draws, 0);
    assert_eq!(out.receipt.random_state_before, entry.random_state);
    assert_eq!(out.receipt.random_state_after, entry.random_state);
    assert_eq!(out.receipt.world_mutations, 0);
    assert_eq!(out.receipt.world_checksum_before, entry.world_checksum);
    assert_eq!(out.receipt.world_checksum_after, entry.world_checksum);
    assert_eq!(out.receipt.sourced_walked_bytes_before, 52);
    assert_eq!(out.receipt.sourced_walked_bytes_after, 52);
    assert_eq!(
        out.receipt.resource_pool_digest_before,
        entry.resource_pool_digest
    );
    assert_eq!(
        out.receipt.resource_pool_digest_after,
        entry.resource_pool_digest
    );
    assert_eq!(
        out.receipt.category_tail_release_call_vas,
        [
            CATEGORY_CURRENT_TAIL_RELEASE_CALL_VA,
            CATEGORY_CURRENT_HEAD_RELEASE_CALL_VA,
            ROW_CURRENT_TAIL_RELEASE_CALL_VA,
            ROW_CURRENT_HEAD_RELEASE_CALL_VA,
        ]
    );
    assert_eq!(
        out.receipt.document_tail_release_call_vas,
        [
            SELECTED_DOCUMENT_TAIL_RELEASE_CALL_VA,
            SELECTED_DOCUMENT_HEAD_RELEASE_CALL_VA,
            DEFAULT_DOCUMENT_TAIL_RELEASE_CALL_VA,
            DEFAULT_DOCUMENT_HEAD_RELEASE_CALL_VA,
        ]
    );
}

#[test]
fn stale_evidence_and_bad_row_are_atomic() {
    let entry = entry();
    let mut stale = facts(&entry);
    if let PlaceResourcesXmlEvidence::SyntheticFixture { random_state, .. } = &mut stale.evidence {
        *random_state ^= 1;
    }
    let original = handles(3);
    let mut host = original;
    assert_eq!(
        execute_place_resources_xml_frontier(&mut host, &entry, &stale),
        Err(PlaceResourcesXmlError::StaleEvidence)
    );
    assert_eq!(host, original);

    let mut bad = facts(&entry);
    bad.selected_bonuses.as_mut().unwrap().bonus_rows[1].element_name = "NOT_BONUS".to_owned();
    assert_eq!(
        execute_place_resources_xml_frontier(&mut host, &entry, &bad),
        Err(PlaceResourcesXmlError::WrongRowName {
            index: 1,
            actual: "NOT_BONUS".to_owned(),
        })
    );
    assert_eq!(host, original);
}
