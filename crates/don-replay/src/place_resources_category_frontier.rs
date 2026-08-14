// SPDX-License-Identifier: GPL-3.0-or-later
//! Source-only owner for the resource-category tail and the first exact prefixes of
//! `Map::place_region_resource` / `Map::place_player_resource`.
//!
//! This source is intentionally not registered yet.  It starts at the existing
//! `BONUSES` residual `0x00690225`, preserves the chance carry across `FISH` and
//! `GOODIES`, and leaves the large candidate-filter bodies as typed residuals.

use don_sim::rng::Random;
use don_sim::systems::map_terrain::WorldChecksum;

pub const SHIPPED_EXE_SHA256: &str =
    "30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079";
pub const SHIPPED_PDB_SHA256: &str =
    "334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5";

pub const CATEGORY_TAIL_VA: u32 = 0x0069_0225;
pub const CATEGORY_INCREMENT_VA: u32 = 0x0069_0290;
pub const CATEGORY_DISPATCH_VA: u32 = 0x0068_f754;
pub const ROW_ENUMERATE_CALL_VA: u32 = 0x0068_fb98;
pub const ROW_COUNT_TEST_VA: u32 = 0x0068_fba8;
pub const ROW_BODY_VA: u32 = 0x0068_fbb3;
pub const FUNCTION_RETURN_VA: u32 = 0x0069_0476;

pub const CATEGORY_RELEASE_CALL_VAS: [u32; 4] =
    [0x0069_0232, 0x0069_024c, 0x0069_0266, 0x0069_0280];
pub const DOCUMENT_RELEASE_CALL_VAS: [u32; 4] =
    [0x0069_02ad, 0x0069_02c7, 0x0069_02e1, 0x0069_02fb];

pub const SELECTED_FISH_GET_ELEMENT_CALL_VA: u32 = 0x0068_f8f6;
pub const DEFAULT_FISH_GET_ELEMENT_CALL_VA: u32 = 0x0068_f9b0;
pub const SELECTED_GOODIES_GET_ELEMENT_CALL_VA: u32 = 0x0068_f79b;
pub const DEFAULT_GOODIES_GET_ELEMENT_CALL_VA: u32 = 0x0068_f855;

pub const BONUSES_STRING_OFFSET: u32 = 0x0001_7a98;
pub const SELECTED_FISH_STRING_OFFSET: u32 = 0x0001_7aac;
pub const DEFAULT_FISH_STRING_OFFSET: u32 = 0x0001_7ac0;
pub const GOODIES_STRING_OFFSET: u32 = 0x0001_7ad4;
pub const BONUS_ROW_STRING_OFFSET: u32 = 0x0001_7ae8;

pub const MAP_PLACE_REGION_RESOURCE_VA: u32 = 0x0069_0480;
pub const MAP_FIND_AVAIL_REGIONS_CALL_VA: u32 = 0x0069_0622;
pub const MAP_FIND_AVAIL_REGIONS_VA: u32 = 0x0068_f380;
pub const FIND_AVAIL_RANDOM_GET_CALL_VA: u32 = 0x0068_f498;
pub const REGION_POOL_SELECTION_VA: u32 = 0x0069_06ba;
pub const REGION_RANDOM_GET_CALL_VA: u32 = 0x0069_071a;
pub const REGION_CANDIDATE_SCAN_VA: u32 = 0x0069_0770;
pub const REGION_EMPTY_RETURN_PATH_VA: u32 = 0x0069_0631;

pub const MAP_PLACE_PLAYER_RESOURCE_VA: u32 = 0x0069_1f70;
pub const PLAYER_POOL_SELECTION_VA: u32 = 0x0069_20a0;
pub const PLAYER_RANDOM_GET_CALL_VA: u32 = 0x0069_2114;
pub const PLAYER_CANDIDATE_SCAN_VA: u32 = 0x0069_2160;
pub const PLAYER_INVALID_START_PATH_VA: u32 = 0x0069_2e0d;
pub const RANDOM_GET_VA: u32 = 0x00a3_9d70;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HostHandles {
    pub head: bool,
    pub tail: bool,
    pub inline_tail_word: bool,
}

impl HostHandles {
    pub fn valid(self) -> bool {
        self.tail || self.inline_tail_word
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeterministicResourceState {
    pub random_state: i32,
    pub world_checksum: WorldChecksum,
    pub sourced_walked_bytes: u64,
    pub resource_pool_digest: u64,
    pub allocated_resources: i32,
    pub requested_resources: i32,
    /// Native `[ebp-0x90]`, initialized once before the three-category loop.
    pub last_chance_group: i32,
    /// Native `[ebp-0x2c]`, initialized once before the three-category loop.
    pub signed_chance_budget: i32,
    /// Native `[ebp-0x28]`, initialized once before the three-category loop.
    pub winner_seen: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceCategory {
    Bonuses,
    Fish,
    Goodies,
}

impl ResourceCategory {
    pub fn ordinal(self) -> u8 {
        match self {
            Self::Bonuses => 0,
            Self::Fish => 1,
            Self::Goodies => 2,
        }
    }

    fn next(self) -> Option<Self> {
        match self {
            Self::Bonuses => Some(Self::Fish),
            Self::Fish => Some(Self::Goodies),
            Self::Goodies => None,
        }
    }

    fn element_name(self) -> &'static str {
        match self {
            Self::Bonuses => "BONUSES",
            Self::Fish => "FISH",
            Self::Goodies => "GOODIES",
        }
    }

    fn selected_lookup(self) -> (&'static str, u32, u32) {
        match self {
            Self::Fish => (
                "FISH ",
                SELECTED_FISH_STRING_OFFSET,
                SELECTED_FISH_GET_ELEMENT_CALL_VA,
            ),
            Self::Goodies => (
                "GOODIES",
                GOODIES_STRING_OFFSET,
                SELECTED_GOODIES_GET_ELEMENT_CALL_VA,
            ),
            Self::Bonuses => unreachable!("the owner starts after BONUSES"),
        }
    }

    fn default_lookup(self) -> (&'static str, u32, u32) {
        match self {
            Self::Fish => (
                "FISH",
                DEFAULT_FISH_STRING_OFFSET,
                DEFAULT_FISH_GET_ELEMENT_CALL_VA,
            ),
            Self::Goodies => (
                "GOODIES",
                GOODIES_STRING_OFFSET,
                DEFAULT_GOODIES_GET_ELEMENT_CALL_VA,
            ),
            Self::Bonuses => unreachable!("the owner starts after BONUSES"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CategoryRowFact {
    pub capture_ordinal: u32,
    pub element_name: String,
    pub handles: HostHandles,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CategorySectionFact {
    /// The exact token passed to `XMLNode::get_element`, including the shipped
    /// trailing space in the selected-document FISH lookup.
    pub lookup_token: String,
    /// Name reported by the returned element. This remains `FISH`; it is not
    /// conflated with the selected lookup's `"FISH "` XPath token.
    pub returned_element_name: String,
    pub handles: HostHandles,
    pub rows: Vec<CategoryRowFact>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResourceFrontierEvidence {
    RetailCapture {
        executable_sha256: String,
        pdb_sha256: String,
        capture_sha256: [u8; 32],
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

impl ResourceFrontierEvidence {
    fn admissible(&self, entry_va: u32, state: &DeterministicResourceState) -> bool {
        let fields = |actual_entry: u32,
                      random_state: i32,
                      world_checksum: &WorldChecksum,
                      sourced_walked_bytes: u64,
                      resource_pool_digest: u64| {
            actual_entry == entry_va
                && random_state == state.random_state
                && world_checksum == &state.world_checksum
                && sourced_walked_bytes == state.sourced_walked_bytes
                && resource_pool_digest == state.resource_pool_digest
        };
        match self {
            Self::RetailCapture {
                executable_sha256,
                pdb_sha256,
                capture_sha256,
                entry_va: actual_entry,
                random_state,
                world_checksum,
                sourced_walked_bytes,
                resource_pool_digest,
            } => {
                executable_sha256 == SHIPPED_EXE_SHA256
                    && pdb_sha256 == SHIPPED_PDB_SHA256
                    && capture_sha256.iter().any(|byte| *byte != 0)
                    && fields(
                        *actual_entry,
                        *random_state,
                        world_checksum,
                        *sourced_walked_bytes,
                        *resource_pool_digest,
                    )
            }
            Self::SyntheticFixture {
                fixture,
                entry_va: actual_entry,
                random_state,
                world_checksum,
                sourced_walked_bytes,
                resource_pool_digest,
            } => {
                cfg!(test)
                    && !fixture.is_empty()
                    && fields(
                        *actual_entry,
                        *random_state,
                        world_checksum,
                        *sourced_walked_bytes,
                        *resource_pool_digest,
                    )
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CategoryAdvanceFacts {
    pub selected_style_name_nonempty: bool,
    pub selected_section: Option<CategorySectionFact>,
    pub default_section: Option<CategorySectionFact>,
    pub evidence: ResourceFrontierEvidence,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CategoryLoopState {
    pub category: ResourceCategory,
    /// A caller may advance only after the category's row count reaches zero.
    pub rows_remaining: usize,
    pub category_handles: HostHandles,
    pub row_handles: HostHandles,
    pub selected_document_handles: HostHandles,
    pub default_document_handles: HostHandles,
    pub deterministic: DeterministicResourceState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostRefKind {
    ReleaseCategoryTail,
    ReleaseCategoryHead,
    ReleaseRowTail,
    ReleaseRowHead,
    AcquireCategoryHead,
    AcquireCategoryTail,
    ReleaseSelectedDocumentTail,
    ReleaseSelectedDocumentHead,
    ReleaseDefaultDocumentTail,
    ReleaseDefaultDocumentHead,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostRefOperation {
    pub call_va: u32,
    pub kind: HostRefKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SectionSource {
    Selected,
    Default,
    MissingDefault,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CategoryAdvanceDisposition {
    NextCategory {
        category: ResourceCategory,
        source: SectionSource,
        rows: Vec<CategoryRowFact>,
        residual_va: u32,
    },
    Returned {
        return_count: i32,
        return_va: u32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CategoryAdvanceReceipt {
    pub entry_va: u32,
    pub completed_category: ResourceCategory,
    pub completed_ordinal: u8,
    pub cleanup_operations: Vec<HostRefOperation>,
    pub category_increment_va: u32,
    pub category_dispatch_va: Option<u32>,
    pub selected_lookup_call_va: Option<u32>,
    pub selected_lookup_string_offset: Option<u32>,
    pub default_lookup_call_va: Option<u32>,
    pub default_lookup_string_offset: Option<u32>,
    pub row_enumerate_call_va: Option<u32>,
    pub row_count_test_va: Option<u32>,
    pub deterministic_before: DeterministicResourceState,
    pub deterministic_after: DeterministicResourceState,
    pub disposition: CategoryAdvanceDisposition,
}

/// No-authority prefix of category cleanup, stopping before the first XML
/// lookup for the next category. This is useful when row/category handles and
/// the selected-style-name branch are already authenticated but the returned
/// section is not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CategoryCleanupToLookupReceipt {
    pub entry_va: u32,
    pub completed_category: ResourceCategory,
    pub cleanup_operations: Vec<HostRefOperation>,
    pub category_increment_va: u32,
    pub category_dispatch_va: u32,
    pub next_category: ResourceCategory,
    pub selected_style_name_nonempty: bool,
    pub lookup_call_va: u32,
    pub lookup_string_offset: u32,
    pub lookup_token: &'static str,
    pub residual_va: u32,
    pub deterministic_before: DeterministicResourceState,
    pub deterministic_after: DeterministicResourceState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CategoryCleanupToLookupError {
    RowsRemain,
    UnsupportedCompletedCategory,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CategoryAdvanceError {
    RowsRemain,
    StaleEvidence,
    WrongLookupToken {
        expected: &'static str,
        actual: String,
    },
    WrongSectionName {
        expected: &'static str,
        actual: String,
    },
    WrongRowName {
        index: usize,
        actual: String,
    },
}

fn release_if(out: &mut Vec<HostRefOperation>, condition: bool, call_va: u32, kind: HostRefKind) {
    if condition {
        out.push(HostRefOperation { call_va, kind });
    }
}

/// Execute FISH category cleanup through the exact GOODIES dispatch, stopping
/// before the selected/default XML lookup. RNG, World, pool, counters, chance
/// locals, and document handles are carried unchanged.
pub fn execute_fish_cleanup_to_goodies_lookup(
    state: &mut CategoryLoopState,
    selected_style_name_nonempty: bool,
) -> Result<CategoryCleanupToLookupReceipt, CategoryCleanupToLookupError> {
    if state.rows_remaining != 0 {
        return Err(CategoryCleanupToLookupError::RowsRemain);
    }
    if state.category != ResourceCategory::Fish {
        return Err(CategoryCleanupToLookupError::UnsupportedCompletedCategory);
    }

    let mut staged = state.clone();
    let deterministic_before = staged.deterministic.clone();
    let mut operations = Vec::new();
    release_if(
        &mut operations,
        staged.category_handles.tail,
        CATEGORY_RELEASE_CALL_VAS[0],
        HostRefKind::ReleaseCategoryTail,
    );
    release_if(
        &mut operations,
        staged.category_handles.head,
        CATEGORY_RELEASE_CALL_VAS[1],
        HostRefKind::ReleaseCategoryHead,
    );
    release_if(
        &mut operations,
        staged.row_handles.tail,
        CATEGORY_RELEASE_CALL_VAS[2],
        HostRefKind::ReleaseRowTail,
    );
    release_if(
        &mut operations,
        staged.row_handles.head,
        CATEGORY_RELEASE_CALL_VAS[3],
        HostRefKind::ReleaseRowHead,
    );
    staged.category_handles = HostHandles::default();
    staged.row_handles = HostHandles::default();
    staged.category = ResourceCategory::Goodies;

    let (lookup_call_va, lookup_string_offset, lookup_token) = if selected_style_name_nonempty {
        (
            SELECTED_GOODIES_GET_ELEMENT_CALL_VA,
            GOODIES_STRING_OFFSET,
            "GOODIES",
        )
    } else {
        (
            DEFAULT_GOODIES_GET_ELEMENT_CALL_VA,
            GOODIES_STRING_OFFSET,
            "GOODIES",
        )
    };
    let receipt = CategoryCleanupToLookupReceipt {
        entry_va: CATEGORY_TAIL_VA,
        completed_category: ResourceCategory::Fish,
        cleanup_operations: operations,
        category_increment_va: CATEGORY_INCREMENT_VA,
        category_dispatch_va: CATEGORY_DISPATCH_VA,
        next_category: ResourceCategory::Goodies,
        selected_style_name_nonempty,
        lookup_call_va,
        lookup_string_offset,
        lookup_token,
        residual_va: lookup_call_va,
        deterministic_before,
        deterministic_after: staged.deterministic.clone(),
    };
    *state = staged;
    Ok(receipt)
}

fn validate_section(
    section: &CategorySectionFact,
    expected_lookup: &'static str,
    expected_element: &'static str,
) -> Result<(), CategoryAdvanceError> {
    if section.lookup_token != expected_lookup {
        return Err(CategoryAdvanceError::WrongLookupToken {
            expected: expected_lookup,
            actual: section.lookup_token.clone(),
        });
    }
    if section.returned_element_name != expected_element {
        return Err(CategoryAdvanceError::WrongSectionName {
            expected: expected_element,
            actual: section.returned_element_name.clone(),
        });
    }
    for (index, row) in section.rows.iter().enumerate() {
        if row.element_name != "BONUS" {
            return Err(CategoryAdvanceError::WrongRowName {
                index,
                actual: row.element_name.clone(),
            });
        }
    }
    Ok(())
}

/// Advance one completed category.  RNG, World, pool, return counter, requested
/// counter, and all three chance locals are carried byte-for-byte.
pub fn advance_completed_category(
    state: &mut CategoryLoopState,
    facts: &CategoryAdvanceFacts,
) -> Result<CategoryAdvanceReceipt, CategoryAdvanceError> {
    if state.rows_remaining != 0 {
        return Err(CategoryAdvanceError::RowsRemain);
    }
    if !facts
        .evidence
        .admissible(CATEGORY_TAIL_VA, &state.deterministic)
    {
        return Err(CategoryAdvanceError::StaleEvidence);
    }
    let mut staged = state.clone();
    let mut operations = Vec::new();
    release_if(
        &mut operations,
        staged.category_handles.tail,
        CATEGORY_RELEASE_CALL_VAS[0],
        HostRefKind::ReleaseCategoryTail,
    );
    release_if(
        &mut operations,
        staged.category_handles.head,
        CATEGORY_RELEASE_CALL_VAS[1],
        HostRefKind::ReleaseCategoryHead,
    );
    release_if(
        &mut operations,
        staged.row_handles.tail,
        CATEGORY_RELEASE_CALL_VAS[2],
        HostRefKind::ReleaseRowTail,
    );
    release_if(
        &mut operations,
        staged.row_handles.head,
        CATEGORY_RELEASE_CALL_VAS[3],
        HostRefKind::ReleaseRowHead,
    );
    staged.category_handles = HostHandles::default();
    staged.row_handles = HostHandles::default();

    let completed = staged.category;
    let deterministic_before = staged.deterministic.clone();
    let Some(next) = completed.next() else {
        release_if(
            &mut operations,
            staged.selected_document_handles.tail,
            DOCUMENT_RELEASE_CALL_VAS[0],
            HostRefKind::ReleaseSelectedDocumentTail,
        );
        release_if(
            &mut operations,
            staged.selected_document_handles.head,
            DOCUMENT_RELEASE_CALL_VAS[1],
            HostRefKind::ReleaseSelectedDocumentHead,
        );
        release_if(
            &mut operations,
            staged.default_document_handles.tail,
            DOCUMENT_RELEASE_CALL_VAS[2],
            HostRefKind::ReleaseDefaultDocumentTail,
        );
        release_if(
            &mut operations,
            staged.default_document_handles.head,
            DOCUMENT_RELEASE_CALL_VAS[3],
            HostRefKind::ReleaseDefaultDocumentHead,
        );
        staged.selected_document_handles = HostHandles::default();
        staged.default_document_handles = HostHandles::default();
        let receipt = CategoryAdvanceReceipt {
            entry_va: CATEGORY_TAIL_VA,
            completed_category: completed,
            completed_ordinal: completed.ordinal(),
            cleanup_operations: operations,
            category_increment_va: CATEGORY_INCREMENT_VA,
            category_dispatch_va: None,
            selected_lookup_call_va: None,
            selected_lookup_string_offset: None,
            default_lookup_call_va: None,
            default_lookup_string_offset: None,
            row_enumerate_call_va: None,
            row_count_test_va: None,
            deterministic_before,
            deterministic_after: staged.deterministic.clone(),
            disposition: CategoryAdvanceDisposition::Returned {
                return_count: staged.deterministic.allocated_resources,
                return_va: FUNCTION_RETURN_VA,
            },
        };
        *state = staged;
        return Ok(receipt);
    };

    let (selected_name, selected_offset, selected_call) = next.selected_lookup();
    let (default_name, default_offset, default_call) = next.default_lookup();
    let mut selected_lookup_call_va = None;
    let mut default_lookup_call_va = None;
    let selected = if facts.selected_style_name_nonempty {
        selected_lookup_call_va = Some(selected_call);
        if let Some(section) = &facts.selected_section {
            validate_section(section, selected_name, next.element_name())?;
        }
        facts
            .selected_section
            .as_ref()
            .filter(|section| section.handles.valid())
    } else {
        None
    };
    let (source, chosen) = if let Some(section) = selected {
        (SectionSource::Selected, Some(section))
    } else {
        default_lookup_call_va = Some(default_call);
        if let Some(section) = &facts.default_section {
            validate_section(section, default_name, next.element_name())?;
        }
        match facts
            .default_section
            .as_ref()
            .filter(|section| section.handles.valid())
        {
            Some(section) => (SectionSource::Default, Some(section)),
            None => (SectionSource::MissingDefault, None),
        }
    };

    if let Some(section) = chosen {
        if section.handles.head {
            operations.push(HostRefOperation {
                call_va: if source == SectionSource::Selected {
                    selected_call
                } else {
                    default_call
                },
                kind: HostRefKind::AcquireCategoryHead,
            });
        }
        if section.handles.tail {
            operations.push(HostRefOperation {
                call_va: if source == SectionSource::Selected {
                    selected_call
                } else {
                    default_call
                },
                kind: HostRefKind::AcquireCategoryTail,
            });
        }
        staged.category_handles = section.handles;
        staged.rows_remaining = section.rows.len();
    } else {
        staged.rows_remaining = 0;
    }
    staged.category = next;
    let rows = chosen
        .map(|section| section.rows.clone())
        .unwrap_or_default();
    let residual_va = if rows.is_empty() {
        CATEGORY_TAIL_VA
    } else {
        ROW_BODY_VA
    };
    let receipt = CategoryAdvanceReceipt {
        entry_va: CATEGORY_TAIL_VA,
        completed_category: completed,
        completed_ordinal: completed.ordinal(),
        cleanup_operations: operations,
        category_increment_va: CATEGORY_INCREMENT_VA,
        category_dispatch_va: Some(CATEGORY_DISPATCH_VA),
        selected_lookup_call_va,
        selected_lookup_string_offset: selected_lookup_call_va.map(|_| selected_offset),
        default_lookup_call_va,
        default_lookup_string_offset: default_lookup_call_va.map(|_| default_offset),
        row_enumerate_call_va: Some(ROW_ENUMERATE_CALL_VA),
        row_count_test_va: Some(ROW_COUNT_TEST_VA),
        deterministic_before,
        deterministic_after: staged.deterministic.clone(),
        disposition: CategoryAdvanceDisposition::NextCategory {
            category: next,
            source,
            rows,
            residual_va,
        },
    };
    *state = staged;
    Ok(receipt)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExactRandomDraw {
    pub call_va: u32,
    pub random_get_va: u32,
    pub low: i32,
    pub high: i32,
    pub state_before: i32,
    pub raw: i32,
    pub modulus: i32,
    pub remainder: i32,
    pub state_after: i32,
}

fn draw_mod(random: &mut Random, call_va: u32, modulus: i32) -> ExactRandomDraw {
    let state_before = random.state();
    let raw = random.get(0, 0xffff);
    ExactRandomDraw {
        call_va,
        random_get_va: RANDOM_GET_VA,
        low: 0,
        high: 0xffff,
        state_before,
        raw,
        modulus,
        remainder: raw % modulus,
        state_after: random.state(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegionPrefixFact {
    pub region_id: i32,
    pub land_cell_count: i32,
    pub contains_player_start: bool,
    pub candidate_point_count: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegionPlacementPrefixFacts {
    pub ocean_rare_good_ids: Vec<i32>,
    /// Exactly regions 1..=126 in ascending ID order.
    pub regions: Vec<RegionPrefixFact>,
    pub evidence: ResourceFrontierEvidence,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegionPlacementPrefixRequest {
    pub good_id: i32,
    pub pattern: i32,
    pub saturate: i32,
    pub num_rare: i32,
    pub selector: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegionPrefixDisposition {
    NoAvailableRegions {
        return_path_va: u32,
    },
    NoRequestedResources {
        residual_va: u32,
    },
    PoolSelectionBoundary {
        call_va: u32,
        selected_region: i32,
    },
    CandidateScan {
        residual_va: u32,
        selected_region: i32,
        point_index: i32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegionPlacementPrefixReceipt {
    pub entry_va: u32,
    pub find_avail_regions_call_va: u32,
    pub find_avail_regions_va: u32,
    pub water_good: bool,
    /// Exact linked-list traversal order. `LinkList::add` prepends, so retail's
    /// ascending region scan is observed here in descending ID order.
    pub available_regions: Vec<i32>,
    pub find_avail_draw: Option<ExactRandomDraw>,
    pub first_region_index: Option<usize>,
    pub region_pass_count: i32,
    pub per_region_limit: i32,
    pub point_draw: Option<ExactRandomDraw>,
    pub random_state_before: i32,
    pub random_state_after: i32,
    pub world_checksum: WorldChecksum,
    pub disposition: RegionPrefixDisposition,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlacementPrefixError {
    StaleEvidence,
    IncompleteRegionCatalog,
    InvalidRegionPointCount { region_id: i32 },
    InvalidPlayerStarts,
    InvalidSpiralOffsets,
}

/// Own the exact region-placement prefix through the first point chosen for the
/// candidate scan.  Selector rows stop before their divvy-pool callback.
pub fn execute_region_placement_prefix(
    state: &DeterministicResourceState,
    request: RegionPlacementPrefixRequest,
    facts: &RegionPlacementPrefixFacts,
) -> Result<RegionPlacementPrefixReceipt, PlacementPrefixError> {
    if !facts
        .evidence
        .admissible(MAP_PLACE_REGION_RESOURCE_VA, state)
    {
        return Err(PlacementPrefixError::StaleEvidence);
    }
    if facts.regions.len() != 126
        || facts
            .regions
            .iter()
            .enumerate()
            .any(|(index, region)| region.region_id != index as i32 + 1)
    {
        return Err(PlacementPrefixError::IncompleteRegionCatalog);
    }
    if let Some(region) = facts
        .regions
        .iter()
        .find(|region| region.candidate_point_count < 0)
    {
        return Err(PlacementPrefixError::InvalidRegionPointCount {
            region_id: region.region_id,
        });
    }

    let water_good = request.good_id >= 0
        && request.good_id != 0x21f
        && facts.ocean_rare_good_ids.contains(&request.good_id);
    let range = if water_good { 64..=126 } else { 1..=63 };
    let mut available_regions: Vec<i32> = range
        .filter(|region_id| {
            let region = &facts.regions[*region_id as usize - 1];
            region.land_cell_count > 5 && !(request.pattern == 3 && region.contains_player_start)
        })
        .collect();
    available_regions.reverse();

    let mut random = Random::new(state.random_state);
    let find_avail_draw = if available_regions.len() > 1 {
        Some(draw_mod(
            &mut random,
            FIND_AVAIL_RANDOM_GET_CALL_VA,
            available_regions.len() as i32,
        ))
    } else {
        None
    };
    let first_region_index = (!available_regions.is_empty()).then(|| {
        find_avail_draw
            .as_ref()
            .map(|draw| draw.remainder as usize)
            .unwrap_or(0)
    });
    let region_pass_count = match request.pattern {
        1 => available_regions.len() as i32,
        4 => 4,
        _ => 1,
    };
    let per_region_limit = if request.num_rare > 2
        && request.saturate == 0
        && ((request.pattern != 1 && request.pattern != 3) || available_regions.len() == 1)
    {
        request.num_rare >> 1
    } else {
        i32::MAX
    };

    let Some(first_index) = first_region_index else {
        return Ok(RegionPlacementPrefixReceipt {
            entry_va: MAP_PLACE_REGION_RESOURCE_VA,
            find_avail_regions_call_va: MAP_FIND_AVAIL_REGIONS_CALL_VA,
            find_avail_regions_va: MAP_FIND_AVAIL_REGIONS_VA,
            water_good,
            available_regions,
            find_avail_draw,
            first_region_index: None,
            region_pass_count,
            per_region_limit,
            point_draw: None,
            random_state_before: state.random_state,
            random_state_after: random.state(),
            world_checksum: state.world_checksum.clone(),
            disposition: RegionPrefixDisposition::NoAvailableRegions {
                return_path_va: REGION_EMPTY_RETURN_PATH_VA,
            },
        });
    };
    let selected_region = available_regions[first_index];
    if request.num_rare <= 0 {
        return Ok(RegionPlacementPrefixReceipt {
            entry_va: MAP_PLACE_REGION_RESOURCE_VA,
            find_avail_regions_call_va: MAP_FIND_AVAIL_REGIONS_CALL_VA,
            find_avail_regions_va: MAP_FIND_AVAIL_REGIONS_VA,
            water_good,
            available_regions,
            find_avail_draw,
            first_region_index: Some(first_index),
            region_pass_count,
            per_region_limit,
            point_draw: None,
            random_state_before: state.random_state,
            random_state_after: random.state(),
            world_checksum: state.world_checksum.clone(),
            disposition: RegionPrefixDisposition::NoRequestedResources {
                residual_va: 0x0069_1ba9,
            },
        });
    }
    if request.selector != 0 {
        return Ok(RegionPlacementPrefixReceipt {
            entry_va: MAP_PLACE_REGION_RESOURCE_VA,
            find_avail_regions_call_va: MAP_FIND_AVAIL_REGIONS_CALL_VA,
            find_avail_regions_va: MAP_FIND_AVAIL_REGIONS_VA,
            water_good,
            available_regions,
            find_avail_draw,
            first_region_index: Some(first_index),
            region_pass_count,
            per_region_limit,
            point_draw: None,
            random_state_before: state.random_state,
            random_state_after: random.state(),
            world_checksum: state.world_checksum.clone(),
            disposition: RegionPrefixDisposition::PoolSelectionBoundary {
                call_va: REGION_POOL_SELECTION_VA,
                selected_region,
            },
        });
    }

    let point_count = facts.regions[selected_region as usize - 1].candidate_point_count;
    let point_draw = if point_count > 1 {
        Some(draw_mod(
            &mut random,
            REGION_RANDOM_GET_CALL_VA,
            point_count,
        ))
    } else {
        None
    };
    let point_index = point_draw.as_ref().map(|draw| draw.remainder).unwrap_or(0);
    Ok(RegionPlacementPrefixReceipt {
        entry_va: MAP_PLACE_REGION_RESOURCE_VA,
        find_avail_regions_call_va: MAP_FIND_AVAIL_REGIONS_CALL_VA,
        find_avail_regions_va: MAP_FIND_AVAIL_REGIONS_VA,
        water_good,
        available_regions,
        find_avail_draw,
        first_region_index: Some(first_index),
        region_pass_count,
        per_region_limit,
        point_draw,
        random_state_before: state.random_state,
        random_state_after: random.state(),
        world_checksum: state.world_checksum.clone(),
        disposition: RegionPrefixDisposition::CandidateScan {
            residual_va: REGION_CANDIDATE_SCAN_VA,
            selected_region,
            point_index,
        },
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlayerStartFact {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerPlacementPrefixFacts {
    pub world_width: i32,
    pub world_height: i32,
    pub starts: Vec<PlayerStartFact>,
    /// Exact 65-entry table at `0x00cbe330`, indexed by a clamped radius 0..=64.
    pub spiral_ring_ends: Vec<i32>,
    pub evidence: ResourceFrontierEvidence,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlayerPlacementPrefixRequest {
    pub player_count: i32,
    pub good_id: i32,
    pub num_rare: i32,
    pub spacing: i32,
    pub group_spacing: i32,
    pub selector: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlayerPrefixDisposition {
    InvalidFirstStart {
        path_va: u32,
    },
    NoRequestedResources {
        path_va: u32,
    },
    PoolSelectionBoundary {
        call_va: u32,
    },
    EmptyCandidateSpan {
        residual_va: u32,
    },
    CandidateScan {
        residual_va: u32,
        first_candidate_table_index: i32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerPlacementPrefixReceipt {
    pub entry_va: u32,
    pub water_good: bool,
    pub spacing_after_clamp: i32,
    pub group_spacing_after_clamp: i32,
    pub candidate_span: i32,
    pub point_draw: Option<ExactRandomDraw>,
    pub random_state_before: i32,
    pub random_state_after: i32,
    pub world_checksum: WorldChecksum,
    pub disposition: PlayerPrefixDisposition,
}

/// Own the exact first-player prefix through the first spiral-table index.  The
/// large terrain/distance scan beginning at `0x00692160` remains external.
pub fn execute_player_placement_prefix(
    state: &DeterministicResourceState,
    request: PlayerPlacementPrefixRequest,
    facts: &PlayerPlacementPrefixFacts,
) -> Result<PlayerPlacementPrefixReceipt, PlacementPrefixError> {
    if !facts
        .evidence
        .admissible(MAP_PLACE_PLAYER_RESOURCE_VA, state)
    {
        return Err(PlacementPrefixError::StaleEvidence);
    }
    if facts.spiral_ring_ends.len() != 65
        || facts
            .spiral_ring_ends
            .windows(2)
            .any(|pair| pair[0] > pair[1])
    {
        return Err(PlacementPrefixError::InvalidSpiralOffsets);
    }
    if request.player_count <= 0 {
        return Ok(PlayerPlacementPrefixReceipt {
            entry_va: MAP_PLACE_PLAYER_RESOURCE_VA,
            water_good: request.good_id == 6 || request.good_id == 31,
            spacing_after_clamp: request.spacing,
            group_spacing_after_clamp: request.group_spacing,
            candidate_span: 0,
            point_draw: None,
            random_state_before: state.random_state,
            random_state_after: state.random_state,
            world_checksum: state.world_checksum.clone(),
            disposition: PlayerPrefixDisposition::NoRequestedResources {
                path_va: 0x0069_2e19,
            },
        });
    }
    let Some(first) = facts.starts.first().copied() else {
        return Err(PlacementPrefixError::InvalidPlayerStarts);
    };
    if first.x < 0 || first.y < 0 || first.x >= facts.world_width || first.y >= facts.world_height {
        return Ok(PlayerPlacementPrefixReceipt {
            entry_va: MAP_PLACE_PLAYER_RESOURCE_VA,
            water_good: request.good_id == 6 || request.good_id == 31,
            spacing_after_clamp: request.spacing,
            group_spacing_after_clamp: request.group_spacing,
            candidate_span: 0,
            point_draw: None,
            random_state_before: state.random_state,
            random_state_after: state.random_state,
            world_checksum: state.world_checksum.clone(),
            disposition: PlayerPrefixDisposition::InvalidFirstStart {
                path_va: PLAYER_INVALID_START_PATH_VA,
            },
        });
    }
    if request.num_rare <= 0 {
        return Ok(PlayerPlacementPrefixReceipt {
            entry_va: MAP_PLACE_PLAYER_RESOURCE_VA,
            water_good: request.good_id == 6 || request.good_id == 31,
            spacing_after_clamp: request.spacing,
            group_spacing_after_clamp: request.group_spacing,
            candidate_span: 0,
            point_draw: None,
            random_state_before: state.random_state,
            random_state_after: state.random_state,
            world_checksum: state.world_checksum.clone(),
            disposition: PlayerPrefixDisposition::NoRequestedResources {
                path_va: PLAYER_INVALID_START_PATH_VA,
            },
        });
    }
    if request.selector != 0 {
        return Ok(PlayerPlacementPrefixReceipt {
            entry_va: MAP_PLACE_PLAYER_RESOURCE_VA,
            water_good: request.good_id == 6 || request.good_id == 31,
            spacing_after_clamp: request.spacing,
            group_spacing_after_clamp: request.group_spacing,
            candidate_span: 0,
            point_draw: None,
            random_state_before: state.random_state,
            random_state_after: state.random_state,
            world_checksum: state.world_checksum.clone(),
            disposition: PlayerPrefixDisposition::PoolSelectionBoundary {
                call_va: PLAYER_POOL_SELECTION_VA,
            },
        });
    }

    let group_spacing = if request.group_spacing > 64 || request.group_spacing == 0 {
        64
    } else {
        request.group_spacing
    };
    if group_spacing < 0 {
        return Err(PlacementPrefixError::InvalidSpiralOffsets);
    }
    let spacing = request.spacing.min(group_spacing);
    if spacing < 0 {
        return Err(PlacementPrefixError::InvalidSpiralOffsets);
    }
    let span =
        facts.spiral_ring_ends[group_spacing as usize] - facts.spiral_ring_ends[spacing as usize];
    if span < 0 {
        return Err(PlacementPrefixError::InvalidSpiralOffsets);
    }
    let mut random = Random::new(state.random_state);
    let point_draw = if span > 0 {
        // Retail deliberately divides by `span + 1`; the subsequent scan reduces
        // the result modulo `span`, giving table index zero two ways.
        Some(draw_mod(&mut random, PLAYER_RANDOM_GET_CALL_VA, span + 1))
    } else {
        None
    };
    let disposition = if span == 0 {
        PlayerPrefixDisposition::EmptyCandidateSpan {
            residual_va: 0x0069_212b,
        }
    } else {
        let start = point_draw.as_ref().expect("positive span draws").remainder;
        PlayerPrefixDisposition::CandidateScan {
            residual_va: PLAYER_CANDIDATE_SCAN_VA,
            first_candidate_table_index: start % span + facts.spiral_ring_ends[spacing as usize],
        }
    };
    Ok(PlayerPlacementPrefixReceipt {
        entry_va: MAP_PLACE_PLAYER_RESOURCE_VA,
        water_good: request.good_id == 6 || request.good_id == 31,
        spacing_after_clamp: spacing,
        group_spacing_after_clamp: group_spacing,
        candidate_span: span,
        point_draw,
        random_state_before: state.random_state,
        random_state_after: random.state(),
        world_checksum: state.world_checksum.clone(),
        disposition,
    })
}
