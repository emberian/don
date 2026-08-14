// SPDX-License-Identifier: GPL-3.0-or-later
//! Installed-content admission and canonical place-all owner integration.
//!
//! Every displacement image below is a synthetic TGA made by the test. No
//! shipped pixels or precomputed mountain footprint is embedded here.

use don_replay::harness::WorldSim;
use don_replay::initial::replay_place_all_owners::{
    ReplayMountainContentProvider, ReplayPlaceAllMountainOwnerBoundary,
    ReplayPlaceAllOwnerInitialization, ReplayPlaceAllOwnerInitializationError,
    MOUNTAIN_TEMPLATE_PRODUCER_VA,
};
use don_replay::place_all_advance::{
    advance_place_all_boundary_owned, initial_region_helping_state, resolve_mountain_ranges,
    OilGoodPolicy, PlaceAllAdvanceFacts, PlaceAllStop,
};
use don_replay::replay::Replay;
use don_sim::systems::mountain_template_producer::MOUNTAIN_TEMPLATE_CAPACITY;
use don_sim::systems::terrain_groups::PlaceAllOwnerSource;
use don_sim::systems::terrain_region_continuation::PlaceRegionGroupOwnerReceipt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct InstalledFixture {
    root: PathBuf,
    rows: usize,
}

impl InstalledFixture {
    fn new(rows: usize) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let serial = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "don-replay-installed-mountains-{}-{nonce}-{serial}",
            std::process::id()
        ));
        fs::create_dir_all(root.join("art")).unwrap();
        let fixture = Self { root, rows };
        for index in 0..rows {
            fixture.write_template(index, &vec![0; 36 * 36]);
        }
        fs::write(fixture.root.join("effects_graphics.xml"), fixture.xml()).unwrap();
        fixture
    }

    fn provider(&self) -> ReplayMountainContentProvider {
        ReplayMountainContentProvider::from_content_root(&self.root)
    }

    fn effects_graphics_xml(&self) -> PathBuf {
        self.root.join("effects_graphics.xml")
    }

    fn write_template(&self, index: usize, alpha: &[u8]) {
        fs::write(
            self.root.join(format!("art/template-{index}.tga")),
            encode_tga(36, 36, alpha),
        )
        .unwrap();
    }

    fn xml(&self) -> String {
        let mut xml = String::from("<ROOT><MOUNTAINS>");
        for index in 0..self.rows {
            // Preserve the shipped 7 large / 8 medium / 1 small list shape.
            let area = match index {
                0..=6 => "lg",
                7..=14 => "med",
                _ => "sm",
            };
            xml.push_str(&format!(
                "<MOUNTAIN area=\"{area}\" height=\"450\"><TEMPLATE_TEX file=\".\\art\\template-{index}.tga\"/><MAIN_ALPHA_TEX file=\"synthetic-main-{index}\"/><RING_ALPHA_TEX file=\"synthetic-ring-{index}\"/></MOUNTAIN>"
            ));
        }
        xml.push_str("</MOUNTAINS></ROOT>");
        xml
    }
}

impl Drop for InstalledFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn encode_tga(width: usize, height: usize, alpha: &[u8]) -> Vec<u8> {
    assert_eq!(alpha.len(), width * height);
    let mut out = vec![0; 18];
    out[2] = 2; // uncompressed true-color
    out[12..14].copy_from_slice(&(width as u16).to_le_bytes());
    out[14..16].copy_from_slice(&(height as u16).to_le_bytes());
    out[16] = 32;
    out[17] = 0x28; // top-left origin, eight alpha bits
    for (index, &alpha) in alpha.iter().enumerate() {
        // Non-zero color on every pixel catches any accidental RGB occupancy.
        out.extend_from_slice(&[0x41, 0x82, index.wrapping_add(1) as u8, alpha]);
    }
    out
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn open_replay(name: &str) -> Option<Replay> {
    let path = repo_root().join("ron-data/replays/multi").join(name);
    if !path.is_file() {
        eprintln!("\n  SKIPPED — NOT A PASS. {name} is absent; nothing was exercised.\n");
        return None;
    }
    Some(Replay::open(&path).expect("retail replay must decode"))
}

#[test]
fn complete_user_content_retains_all_sources_and_derived_templates_atomically() {
    let fixture = InstalledFixture::new(MOUNTAIN_TEMPLATE_CAPACITY);
    let initialization =
        ReplayPlaceAllOwnerInitialization::from_installed_content(fixture.provider()).unwrap();
    assert_eq!(
        initialization.mountain_boundary(),
        ReplayPlaceAllMountainOwnerBoundary::InstalledDisplacementTemplates {
            producer_va: MOUNTAIN_TEMPLATE_PRODUCER_VA,
            template_count: MOUNTAIN_TEMPLATE_CAPACITY,
        }
    );

    let installed = initialization.installed_mountain_receipt().unwrap();
    assert_eq!(
        installed.catalog().sources.len(),
        MOUNTAIN_TEMPLATE_CAPACITY
    );
    assert_eq!(
        installed.catalog().templates.len(),
        MOUNTAIN_TEMPLATE_CAPACITY
    );
    assert_eq!(
        installed.catalog().displacement_tgas.len(),
        MOUNTAIN_TEMPLATE_CAPACITY
    );
    assert_eq!(
        installed.catalog().effects_graphics_xml.path,
        fixture.effects_graphics_xml()
    );
    for (index, source) in installed.catalog().sources.iter().enumerate() {
        assert_eq!(source.index, index);
        assert_eq!(
            source.displacement_path,
            format!(".\\art\\template-{index}.tga")
        );
    }

    let owners = initialization.entry_owners(257);
    let mountains = owners.mountains.expect("installed mountain runtime");
    assert_eq!(mountains.templates.len(), MOUNTAIN_TEMPLATE_CAPACITY);
    assert!(mountains.templates.iter().all(Option::is_some));
    assert_eq!(mountains.verify_bits.len(), 33);
}

#[test]
fn incomplete_catalog_cannot_yield_an_owner_and_the_exact_sixteen_gate_is_mutation_sensitive() {
    let fixture = InstalledFixture::new(MOUNTAIN_TEMPLATE_CAPACITY - 1);
    assert_eq!(
        ReplayPlaceAllOwnerInitialization::from_installed_content(fixture.provider()),
        Err(
            ReplayPlaceAllOwnerInitializationError::IncompleteMountainTemplateCatalog {
                expected: MOUNTAIN_TEMPLATE_CAPACITY,
                sources: MOUNTAIN_TEMPLATE_CAPACITY - 1,
                displacement_tgas: MOUNTAIN_TEMPLATE_CAPACITY - 1,
                templates: MOUNTAIN_TEMPLATE_CAPACITY - 1,
            }
        )
    );
}

#[test]
fn changing_one_synthetic_tga_changes_the_retained_runtime_not_just_source_text() {
    let fixture = InstalledFixture::new(MOUNTAIN_TEMPLATE_CAPACITY);
    let before =
        ReplayPlaceAllOwnerInitialization::from_installed_content(fixture.provider()).unwrap();

    let mut alpha = vec![0; 36 * 36];
    for (x, y) in [(2usize, 2usize), (6, 2), (2, 6), (6, 6)] {
        alpha[y * 36 + x] = 1;
    }
    fixture.write_template(0, &alpha);
    let after =
        ReplayPlaceAllOwnerInitialization::from_installed_content(fixture.provider()).unwrap();

    assert_eq!(
        before
            .installed_mountain_receipt()
            .unwrap()
            .catalog()
            .sources,
        after
            .installed_mountain_receipt()
            .unwrap()
            .catalog()
            .sources,
        "the provider/XML binding did not change"
    );
    assert_ne!(
        before
            .installed_mountain_receipt()
            .unwrap()
            .catalog()
            .templates[0],
        after
            .installed_mountain_receipt()
            .unwrap()
            .catalog()
            .templates[0],
        "the TGA alpha mutation must reach the derived owner runtime"
    );
    assert_ne!(
        before
            .installed_mountain_receipt()
            .unwrap()
            .catalog()
            .displacement_tgas[0]
            .adler32,
        after
            .installed_mountain_receipt()
            .unwrap()
            .catalog()
            .displacement_tgas[0]
            .adler32,
        "the exact TGA bytes must be bound into installed-source evidence"
    );
    assert_eq!(
        after
            .installed_mountain_receipt()
            .unwrap()
            .catalog()
            .templates[0]
            .mount_tiles
            .len(),
        1
    );
}

#[test]
fn mediterranean_region_owner_consumes_the_installed_mode_four_runtime() {
    let Some(replay) = open_replay("Playback___2025.02.10_21_26_50__Mon_.rcx") else {
        return;
    };
    let fixture = InstalledFixture::new(MOUNTAIN_TEMPLATE_CAPACITY);
    let initialization =
        ReplayPlaceAllOwnerInitialization::from_installed_content(fixture.provider()).unwrap();
    let sim = WorldSim::from_replay(&replay);
    let plan = sim.initial_items.as_ref().expect("initial item plan");
    let map = sim.initial_world.as_ref().expect("prefix World");
    let continent = sim.initial_continent.as_ref().expect("continent receipt");
    let facts = PlaceAllAdvanceFacts {
        mountains: Some(resolve_mountain_ranges(&fixture.effects_graphics_xml()).unwrap()),
        helping: Some(initial_region_helping_state(&map.world)),
        oil_good_policy: OilGoodPolicy::Stop,
        ..PlaceAllAdvanceFacts::default()
    };
    let owners = initialization.entry_owners(map.world.wdata.len());
    let advance = advance_place_all_boundary_owned(plan, map, continent, &facts, &owners).unwrap();

    let mountain = advance
        .owner_receipts
        .iter()
        .find(|receipt| {
            matches!(receipt.source, PlaceAllOwnerSource::Region { .. })
                && matches!(receipt.execution, PlaceRegionGroupOwnerReceipt::Mountain(_))
        })
        .expect("Mediterranean group zero must execute the installed mountain owner");
    let PlaceRegionGroupOwnerReceipt::Mountain(execution) = &mountain.execution else {
        unreachable!();
    };
    assert_eq!(execution.execution.call.verification_mode, 4);
    assert!(execution.mountain_walk_bytes_after > execution.mountain_walk_bytes_before);
    assert!(
        !matches!(
            advance.stop,
            PlaceAllStop::MountainsAddMountain { group_index: 0, .. }
        ),
        "the installed mode-4 owner must move Mediterranean past its former group-zero stop"
    );
    assert!(
        owners
            .mountains
            .as_ref()
            .unwrap()
            .mountain_locs
            .items
            .is_empty(),
        "the read-only survey must not mutate its entry owner"
    );
}

#[test]
fn great_lakes_player_mode_five_consumes_the_installed_runtime() {
    let Some(replay) = open_replay("Playback___2024.02.24_21_25_53__Sat_.rcx") else {
        return;
    };
    let fixture = InstalledFixture::new(MOUNTAIN_TEMPLATE_CAPACITY);
    // One native sampled TCoord and its enclosing WCoord give every range an
    // explicit, small installed footprint. The fixture remains synthetic;
    // this test pins owner plumbing, not shipped geometry.
    let mut alpha = vec![0; 36 * 36];
    for y in [2, 6] {
        for x in [2, 6] {
            alpha[y * 36 + x] = 1;
        }
    }
    for index in 0..MOUNTAIN_TEMPLATE_CAPACITY {
        fixture.write_template(index, &alpha);
    }
    let initialization =
        ReplayPlaceAllOwnerInitialization::from_installed_content(fixture.provider()).unwrap();
    let sim = WorldSim::from_replay(&replay);
    let plan = sim.initial_items.as_ref().expect("initial item plan");
    let map = sim.initial_world.as_ref().expect("prefix World");
    let continent = sim.initial_continent.as_ref().expect("continent receipt");
    let facts = PlaceAllAdvanceFacts {
        mountains: Some(resolve_mountain_ranges(&fixture.effects_graphics_xml()).unwrap()),
        helping: Some(initial_region_helping_state(&map.world)),
        doober_rules: Some(
            plan.fertility
                .as_ref()
                .expect("installed tileset fertility facts")
                .doober_rules,
        ),
        oil_good_policy: OilGoodPolicy::Stop,
        ..PlaceAllAdvanceFacts::default()
    };
    let owners = initialization.entry_owners(map.world.wdata.len());
    assert!(owners.mountains.is_some(), "the catalog is installed");
    let advance = advance_place_all_boundary_owned(plan, map, continent, &facts, &owners).unwrap();

    let player_mountains = advance
        .owner_receipts
        .iter()
        .filter(|receipt| {
            matches!(receipt.source, PlaceAllOwnerSource::Player { .. })
                && matches!(receipt.execution, PlaceRegionGroupOwnerReceipt::Mountain(_))
        })
        .collect::<Vec<_>>();
    assert!(
        !player_mountains.is_empty(),
        "Great Lakes group two must execute the installed player mountain owner"
    );
    for receipt in player_mountains {
        let PlaceRegionGroupOwnerReceipt::Mountain(execution) = &receipt.execution else {
            unreachable!();
        };
        assert_eq!(execution.execution.call.verification_mode, 5);
        assert_eq!(execution.execution.rng_draws, 0);
    }
    assert!(advance.completed_groups.contains(&2));
    assert_eq!(advance.stop, PlaceAllStop::PostPlacementReporting);
    let authority = advance
        .post_placement_authority
        .as_ref()
        .expect("reporting-only boundary must retain final map authority");
    assert!(authority.checksum_is_coherent());
    assert_eq!(
        authority.world_checksum,
        authority.world.checksum_sections()
    );
    let final_owners = authority
        .owners
        .as_ref()
        .expect("owned survey must retain its final subsystem owners");
    assert!(!final_owners
        .mountains
        .as_ref()
        .expect("installed runtime survives through treeification")
        .mountain_types
        .items
        .is_empty());
    assert!(
        !matches!(
            advance.stop,
            PlaceAllStop::MountainsAddMountain { group_index: 2, .. }
        ),
        "the installed mode-5 owner must move Great Lakes past its former group-two stop"
    );
}
