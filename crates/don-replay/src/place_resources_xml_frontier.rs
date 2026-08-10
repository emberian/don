// SPDX-License-Identifier: GPL-3.0-or-later
//! Source-only replay frontier for the XML bootstrap immediately after the
//! deterministic `Map::place_resources` divvy-pool prefix.
//!
//! The exact shipped span is `0x0068f597..0x0068fb9d`. It normalizes the selected
//! map-style filename, opens the selected and default XML documents, selects the first
//! (`BONUSES`) section with retail's present-section fallback rule, and captures its
//! ordered `BONUS` children. This span owns only host XML references. It consumes no RNG
//! and mutates neither `World` nor `Map::resource_pool`.

use don_sim::systems::map_terrain::WorldChecksum;

pub const SHIPPED_EXE_SHA256: &str =
    "30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079";
pub const SHIPPED_PDB_SHA256: &str =
    "334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5";

pub const PLACE_RESOURCES_POOL_RESIDUAL_VA: u32 = 0x0068_f597;
pub const STYLE_SUFFIX_DELETE_CALL_VA: u32 = 0x0068_f68c;
pub const STYLE_COPY_CALL_VA: u32 = 0x0068_f69b;
pub const STYLE_EXTENSION_APPEND_CALL_VA: u32 = 0x0068_f6b4;
pub const SELECTED_XML_INIT_CALL_VA: u32 = 0x0068_f6d1;
pub const DEFAULT_XML_INIT_CALL_VA: u32 = 0x0068_f6f9;
pub const FIRST_CATEGORY_DISPATCH_VA: u32 = 0x0068_f754;
pub const SELECTED_BONUSES_GET_ELEMENT_CALL_VA: u32 = 0x0068_fa51;
pub const DEFAULT_BONUSES_GET_ELEMENT_CALL_VA: u32 = 0x0068_fb0b;
pub const FIRST_CATEGORY_CONVERGENCE_VA: u32 = 0x0068_fb7f;
pub const BONUS_GET_ELEMENTS_CALL_VA: u32 = 0x0068_fb98;
pub const PLACE_RESOURCES_XML_RESIDUAL_VA: u32 = 0x0068_fb9d;
pub const FIRST_BONUS_ROW_BODY_VA: u32 = 0x0068_fbb3;

pub const STRING_OPERATOR_ASSIGN_VA: u32 = 0x00a1_eeb0;
pub const STRING_OPERATOR_APPEND_VA: u32 = 0x00a1_d440;
pub const STRING_DELETE_VA: u32 = 0x00a1_6120;
pub const XML_INIT_VA: u32 = 0x00a2_79e0;
pub const XML_NODE_GET_ELEMENT_VA: u32 = 0x00a2_8090;
pub const XML_NODE_GET_ELEMENTS_VA: u32 = 0x00a2_7720;
pub const RANDOM_GET_VA: u32 = 0x00a3_9d70;

pub const INTERNAL_STRING_TABLE_VA: u32 = 0x00c0_6378;
pub const DEFAULT_STYLE_STRING_OFFSET: u32 = 0x0001_73b8;
pub const XML_EXTENSION_STRING_OFFSET: u32 = 0x0001_73f4;
pub const BONUSES_STRING_OFFSET: u32 = 0x0001_7a98;
pub const BONUS_STRING_OFFSET: u32 = 0x0001_7ae8;

pub const DEFAULT_STYLE_PATH: &str = "mapstyles/default.xml";
pub const XML_EXTENSION: &str = ".xml";
pub const FIRST_CATEGORY_NAME: &str = "BONUSES";
pub const BONUS_ROW_NAME: &str = "BONUS";

/// Per-category host-reference cleanup performed after all rows in the category.
pub const CATEGORY_CURRENT_TAIL_RELEASE_CALL_VA: u32 = 0x0069_0232;
pub const CATEGORY_CURRENT_HEAD_RELEASE_CALL_VA: u32 = 0x0069_024c;
pub const ROW_CURRENT_TAIL_RELEASE_CALL_VA: u32 = 0x0069_0266;
pub const ROW_CURRENT_HEAD_RELEASE_CALL_VA: u32 = 0x0069_0280;
/// Document host-reference cleanup performed after all three resource categories.
pub const SELECTED_DOCUMENT_TAIL_RELEASE_CALL_VA: u32 = 0x0069_02ad;
pub const SELECTED_DOCUMENT_HEAD_RELEASE_CALL_VA: u32 = 0x0069_02c7;
pub const DEFAULT_DOCUMENT_TAIL_RELEASE_CALL_VA: u32 = 0x0069_02e1;
pub const DEFAULT_DOCUMENT_HEAD_RELEASE_CALL_VA: u32 = 0x0069_02fb;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaceResourcesXmlEntryHandoff {
    pub entry_va: u32,
    pub player_count_argument: i32,
    pub random_state: i32,
    pub world_checksum: WorldChecksum,
    pub sourced_walked_bytes: u64,
    /// Stable digest of the complete six-field `Map::resource_pool` projection.
    pub resource_pool_digest: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct XmlHostHandles {
    /// `XMLElement + 0x04`, mirrored in `global_cur_element + 0x04`.
    pub head: bool,
    /// `XMLElement + 0x20`, mirrored in `global_cur_element + 0x20`.
    pub tail: bool,
    /// The adjacent native word checked at `global_cur_element + 0x24` when the tail
    /// pointer is null. This distinguishes an absent node from a valid inline node.
    pub inline_tail_word: bool,
}

impl XmlHostHandles {
    pub fn valid(self) -> bool {
        self.tail || self.inline_tail_word
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BonusXmlRowFact {
    /// Capture-local identity used only to freeze native row order.
    pub capture_ordinal: u32,
    pub element_name: String,
    pub handles: XmlHostHandles,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XmlSectionFact {
    pub element_name: String,
    pub handles: XmlHostHandles,
    /// Exact ordered result of `XMLNode::get_elements("BONUS", ..., 0)`.
    pub bonus_rows: Vec<BonusXmlRowFact>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlaceResourcesXmlEvidence {
    RetailCapture {
        executable_sha256: String,
        pdb_sha256: String,
        capture_sha256: [u8; 32],
        internal_string_table_va: u32,
        default_style_string_offset: u32,
        xml_extension_string_offset: u32,
        bonuses_string_offset: u32,
        bonus_string_offset: u32,
        entry_va: u32,
        random_state: i32,
        world_checksum: WorldChecksum,
        sourced_walked_bytes: u64,
        resource_pool_digest: u64,
    },
    SyntheticFixture {
        fixture: String,
        entry_va: u32,
        random_state: i32,
        world_checksum: WorldChecksum,
        sourced_walked_bytes: u64,
        resource_pool_digest: u64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaceResourcesXmlFacts {
    /// Native `Map::map_filename` (`Map + 0x114`) before suffix normalization.
    pub selected_style_name: String,
    /// The selected root's `BONUSES` node. `Some(empty)` remains a present, empty
    /// section and must not fall back to the default document.
    pub selected_bonuses: Option<XmlSectionFact>,
    pub default_bonuses: Option<XmlSectionFact>,
    pub evidence: PlaceResourcesXmlEvidence,
}

impl PlaceResourcesXmlFacts {
    fn evidence_matches(&self, entry: &PlaceResourcesXmlEntryHandoff) -> bool {
        match &self.evidence {
            PlaceResourcesXmlEvidence::RetailCapture {
                executable_sha256,
                pdb_sha256,
                capture_sha256,
                internal_string_table_va,
                default_style_string_offset,
                xml_extension_string_offset,
                bonuses_string_offset,
                bonus_string_offset,
                entry_va,
                random_state,
                world_checksum,
                sourced_walked_bytes,
                resource_pool_digest,
            } => {
                executable_sha256 == SHIPPED_EXE_SHA256
                    && pdb_sha256 == SHIPPED_PDB_SHA256
                    && capture_sha256.iter().any(|byte| *byte != 0)
                    && *internal_string_table_va == INTERNAL_STRING_TABLE_VA
                    && *default_style_string_offset == DEFAULT_STYLE_STRING_OFFSET
                    && *xml_extension_string_offset == XML_EXTENSION_STRING_OFFSET
                    && *bonuses_string_offset == BONUSES_STRING_OFFSET
                    && *bonus_string_offset == BONUS_STRING_OFFSET
                    && *entry_va == entry.entry_va
                    && *random_state == entry.random_state
                    && world_checksum == &entry.world_checksum
                    && *sourced_walked_bytes == entry.sourced_walked_bytes
                    && *resource_pool_digest == entry.resource_pool_digest
            }
            PlaceResourcesXmlEvidence::SyntheticFixture {
                fixture,
                entry_va,
                random_state,
                world_checksum,
                sourced_walked_bytes,
                resource_pool_digest,
            } => {
                cfg!(test)
                    && !fixture.is_empty()
                    && *entry_va == entry.entry_va
                    && *random_state == entry.random_state
                    && world_checksum == &entry.world_checksum
                    && *sourced_walked_bytes == entry.sourced_walked_bytes
                    && *resource_pool_digest == entry.resource_pool_digest
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BonusesSectionSource {
    SelectedStyle,
    DefaultStyle,
    MissingDefault,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XmlHostRefOperationKind {
    ReleaseOldTail,
    ReleaseOldHead,
    AcquireNewHead,
    AcquireNewTail,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct XmlHostRefOperation {
    pub assignment_call_va: u32,
    pub kind: XmlHostRefOperationKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaceResourcesXmlReceipt {
    pub entry_va: u32,
    pub residual_va: u32,
    pub selected_style_name: String,
    pub selected_style_path: String,
    pub default_style_path: String,
    pub suffix_delete_call_va: Option<u32>,
    pub selected_xml_init_call_va: u32,
    pub default_xml_init_call_va: u32,
    pub first_category_dispatch_va: u32,
    pub selected_get_element_call_va: Option<u32>,
    pub default_get_element_call_va: Option<u32>,
    pub category_convergence_va: u32,
    pub get_elements_call_va: u32,
    pub category_name: &'static str,
    pub row_name: &'static str,
    pub section_source: BonusesSectionSource,
    pub row_count: usize,
    pub host_ref_operations: Vec<XmlHostRefOperation>,
    pub category_tail_release_call_vas: [u32; 4],
    pub document_tail_release_call_vas: [u32; 4],
    pub random_state_before: i32,
    pub random_state_after: i32,
    pub random_draws: usize,
    pub random_get_va: u32,
    pub world_checksum_before: WorldChecksum,
    pub world_checksum_after: WorldChecksum,
    pub sourced_walked_bytes_before: u64,
    pub sourced_walked_bytes_after: u64,
    pub resource_pool_digest_before: u64,
    pub resource_pool_digest_after: u64,
    pub world_mutations: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaceResourcesBonusRowsHandoff {
    pub resume_va: u32,
    pub first_row_body_va: u32,
    pub player_count_argument: i32,
    pub section_source: BonusesSectionSource,
    pub current_category_handles: XmlHostHandles,
    pub rows: Vec<BonusXmlRowFact>,
    pub selected_document_live: bool,
    pub default_document_live: bool,
    pub random_state: i32,
    pub world_checksum: WorldChecksum,
    pub sourced_walked_bytes: u64,
    pub resource_pool_digest: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaceResourcesXmlOutcome {
    pub receipt: PlaceResourcesXmlReceipt,
    pub handoff: PlaceResourcesBonusRowsHandoff,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlaceResourcesXmlError {
    WrongEntry {
        expected: u32,
        actual: u32,
    },
    StaleEvidence,
    WrongSectionName {
        source: BonusesSectionSource,
        actual: String,
    },
    WrongRowName {
        index: usize,
        actual: String,
    },
}

fn normalize_selected_style_path(name: &str) -> (String, bool) {
    let mut units: Vec<u16> = name.encode_utf16().collect();
    let delete_at = if units.len() > 1 {
        (0..units.len().saturating_sub(1))
            .rev()
            .find(|index| units[*index] == u16::from(b'.'))
    } else {
        None
    };
    if let Some(index) = delete_at {
        units.truncate(index);
    }
    units.extend(XML_EXTENSION.encode_utf16());
    (
        String::from_utf16(&units).expect("truncating at ASCII dot preserves UTF-16"),
        delete_at.is_some(),
    )
}

fn assign_host_handles(
    current: &mut XmlHostHandles,
    next: XmlHostHandles,
    assignment_call_va: u32,
    operations: &mut Vec<XmlHostRefOperation>,
) {
    if current.tail {
        operations.push(XmlHostRefOperation {
            assignment_call_va,
            kind: XmlHostRefOperationKind::ReleaseOldTail,
        });
    }
    if current.head {
        operations.push(XmlHostRefOperation {
            assignment_call_va,
            kind: XmlHostRefOperationKind::ReleaseOldHead,
        });
    }
    if next.head {
        operations.push(XmlHostRefOperation {
            assignment_call_va,
            kind: XmlHostRefOperationKind::AcquireNewHead,
        });
    }
    if next.tail {
        operations.push(XmlHostRefOperation {
            assignment_call_va,
            kind: XmlHostRefOperationKind::AcquireNewTail,
        });
    }
    *current = next;
}

pub fn execute_place_resources_xml_frontier(
    host_current: &mut XmlHostHandles,
    entry: &PlaceResourcesXmlEntryHandoff,
    facts: &PlaceResourcesXmlFacts,
) -> Result<PlaceResourcesXmlOutcome, PlaceResourcesXmlError> {
    if entry.entry_va != PLACE_RESOURCES_POOL_RESIDUAL_VA {
        return Err(PlaceResourcesXmlError::WrongEntry {
            expected: PLACE_RESOURCES_POOL_RESIDUAL_VA,
            actual: entry.entry_va,
        });
    }
    if !facts.evidence_matches(entry) {
        return Err(PlaceResourcesXmlError::StaleEvidence);
    }

    let (selected_style_path, deleted_suffix) =
        normalize_selected_style_path(&facts.selected_style_name);
    let mut staged_host = *host_current;
    let mut host_ref_operations = Vec::new();
    let mut selected_get_element_call_va = None;
    let mut default_get_element_call_va = None;

    let (section, source) = if facts.selected_style_name.is_empty() {
        default_get_element_call_va = Some(DEFAULT_BONUSES_GET_ELEMENT_CALL_VA);
        (
            facts.default_bonuses.as_ref(),
            BonusesSectionSource::DefaultStyle,
        )
    } else {
        selected_get_element_call_va = Some(SELECTED_BONUSES_GET_ELEMENT_CALL_VA);
        if let Some(selected) = facts.selected_bonuses.as_ref() {
            if selected.element_name != FIRST_CATEGORY_NAME {
                return Err(PlaceResourcesXmlError::WrongSectionName {
                    source: BonusesSectionSource::SelectedStyle,
                    actual: selected.element_name.clone(),
                });
            }
            assign_host_handles(
                &mut staged_host,
                selected.handles,
                SELECTED_BONUSES_GET_ELEMENT_CALL_VA,
                &mut host_ref_operations,
            );
            if selected.handles.valid() {
                (Some(selected), BonusesSectionSource::SelectedStyle)
            } else {
                default_get_element_call_va = Some(DEFAULT_BONUSES_GET_ELEMENT_CALL_VA);
                (
                    facts.default_bonuses.as_ref(),
                    BonusesSectionSource::DefaultStyle,
                )
            }
        } else {
            // `get_element` still assigns its invalid result to the global host before
            // retail tests validity and falls back.
            assign_host_handles(
                &mut staged_host,
                XmlHostHandles::default(),
                SELECTED_BONUSES_GET_ELEMENT_CALL_VA,
                &mut host_ref_operations,
            );
            default_get_element_call_va = Some(DEFAULT_BONUSES_GET_ELEMENT_CALL_VA);
            (
                facts.default_bonuses.as_ref(),
                BonusesSectionSource::DefaultStyle,
            )
        }
    };

    let (rows, final_source, final_handles) = if let Some(section) = section {
        let source = if source == BonusesSectionSource::DefaultStyle {
            if section.element_name != FIRST_CATEGORY_NAME {
                return Err(PlaceResourcesXmlError::WrongSectionName {
                    source,
                    actual: section.element_name.clone(),
                });
            }
            assign_host_handles(
                &mut staged_host,
                section.handles,
                DEFAULT_BONUSES_GET_ELEMENT_CALL_VA,
                &mut host_ref_operations,
            );
            if section.handles.valid() {
                BonusesSectionSource::DefaultStyle
            } else {
                BonusesSectionSource::MissingDefault
            }
        } else {
            source
        };
        for (index, row) in section.bonus_rows.iter().enumerate() {
            if row.element_name != BONUS_ROW_NAME {
                return Err(PlaceResourcesXmlError::WrongRowName {
                    index,
                    actual: row.element_name.clone(),
                });
            }
        }
        let rows = if source == BonusesSectionSource::MissingDefault {
            Vec::new()
        } else {
            section.bonus_rows.clone()
        };
        (rows, source, staged_host)
    } else {
        assign_host_handles(
            &mut staged_host,
            XmlHostHandles::default(),
            DEFAULT_BONUSES_GET_ELEMENT_CALL_VA,
            &mut host_ref_operations,
        );
        (
            Vec::new(),
            BonusesSectionSource::MissingDefault,
            staged_host,
        )
    };

    let receipt = PlaceResourcesXmlReceipt {
        entry_va: entry.entry_va,
        residual_va: PLACE_RESOURCES_XML_RESIDUAL_VA,
        selected_style_name: facts.selected_style_name.clone(),
        selected_style_path,
        default_style_path: DEFAULT_STYLE_PATH.to_owned(),
        suffix_delete_call_va: deleted_suffix.then_some(STYLE_SUFFIX_DELETE_CALL_VA),
        selected_xml_init_call_va: SELECTED_XML_INIT_CALL_VA,
        default_xml_init_call_va: DEFAULT_XML_INIT_CALL_VA,
        first_category_dispatch_va: FIRST_CATEGORY_DISPATCH_VA,
        selected_get_element_call_va,
        default_get_element_call_va,
        category_convergence_va: FIRST_CATEGORY_CONVERGENCE_VA,
        get_elements_call_va: BONUS_GET_ELEMENTS_CALL_VA,
        category_name: FIRST_CATEGORY_NAME,
        row_name: BONUS_ROW_NAME,
        section_source: final_source,
        row_count: rows.len(),
        host_ref_operations,
        category_tail_release_call_vas: [
            CATEGORY_CURRENT_TAIL_RELEASE_CALL_VA,
            CATEGORY_CURRENT_HEAD_RELEASE_CALL_VA,
            ROW_CURRENT_TAIL_RELEASE_CALL_VA,
            ROW_CURRENT_HEAD_RELEASE_CALL_VA,
        ],
        document_tail_release_call_vas: [
            SELECTED_DOCUMENT_TAIL_RELEASE_CALL_VA,
            SELECTED_DOCUMENT_HEAD_RELEASE_CALL_VA,
            DEFAULT_DOCUMENT_TAIL_RELEASE_CALL_VA,
            DEFAULT_DOCUMENT_HEAD_RELEASE_CALL_VA,
        ],
        random_state_before: entry.random_state,
        random_state_after: entry.random_state,
        random_draws: 0,
        random_get_va: RANDOM_GET_VA,
        world_checksum_before: entry.world_checksum.clone(),
        world_checksum_after: entry.world_checksum.clone(),
        sourced_walked_bytes_before: entry.sourced_walked_bytes,
        sourced_walked_bytes_after: entry.sourced_walked_bytes,
        resource_pool_digest_before: entry.resource_pool_digest,
        resource_pool_digest_after: entry.resource_pool_digest,
        world_mutations: 0,
    };
    let handoff = PlaceResourcesBonusRowsHandoff {
        resume_va: PLACE_RESOURCES_XML_RESIDUAL_VA,
        first_row_body_va: FIRST_BONUS_ROW_BODY_VA,
        player_count_argument: entry.player_count_argument,
        section_source: final_source,
        current_category_handles: final_handles,
        rows,
        selected_document_live: true,
        default_document_live: true,
        random_state: entry.random_state,
        world_checksum: entry.world_checksum.clone(),
        sourced_walked_bytes: entry.sourced_walked_bytes,
        resource_pool_digest: entry.resource_pool_digest,
    };
    *host_current = staged_host;
    Ok(PlaceResourcesXmlOutcome { receipt, handoff })
}
