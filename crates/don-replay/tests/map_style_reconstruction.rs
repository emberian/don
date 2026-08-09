// SPDX-License-Identifier: GPL-3.0-or-later
//! Mutation-sensitive tests for replay map-style static-data admission.

use don_replay::map_style::{MapStyleLoadError, MapStyleStaticData, SHIPPED_MAP_STYLE_CATALOG};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let serial = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "don-replay-map-style-{}-{nonce}-{serial}",
            std::process::id(),
        ));
        fs::create_dir_all(root.join("mapstyles")).unwrap();
        let f = Self(root);
        f.write_catalog(None);
        f.write("mapstyles/default.xml", &style_xml("1", false));
        f.write("mapstyles/mediterranean.xml", &style_xml("4 SCALE", false));
        f
    }

    fn write(&self, relative: &str, body: &str) {
        fs::write(self.0.join(relative), body).unwrap();
    }

    fn write_catalog(&self, replacement: Option<(usize, &str)>) {
        let mut xml = String::from("<ROOT><CATEGORIES id=\"mapstyles\" title=\"Map Style:\">");
        for (i, style) in SHIPPED_MAP_STYLE_CATALOG.iter().enumerate() {
            let key = replacement
                .filter(|(index, _)| *index == i)
                .map(|(_, key)| key)
                .unwrap_or(style.key);
            xml.push_str(&format!("<CATEGORY name=\"n{i}\" key=\"{key}\"/>"));
        }
        xml.push_str("</CATEGORIES></ROOT>");
        self.write("rules.xml", &xml);
    }
}

impl AsRef<Path> for Fixture {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn style_xml(numrare: &str, duplicate: bool) -> String {
    let duplicate = if duplicate { " chance=\"99\"" } else { "" };
    format!(
        "<ROOT><MAP><SEA_MAP value=\"1\"/></MAP>\
         <TERRAIN_GROUPS><GROUPENTRY type=\"trees\" chance=\"100\"{duplicate} min_clumps=\"2 SCALE\"/></TERRAIN_GROUPS>\
         <GOODIES><BONUS type=\"Goody\" numrare=\"{numrare}\" spacing=\"8  SCALE\"/></GOODIES></ROOT>"
    )
}

fn style_xml_without_goodies() -> &'static str {
    "<ROOT><MAP><SEA_MAP value=\"1\"/></MAP>\
     <TERRAIN_GROUPS><GROUPENTRY type=\"trees\" chance=\"100\"/></TERRAIN_GROUPS></ROOT>"
}

#[test]
fn exact_catalog_and_raw_placement_expressions_are_admitted() {
    let f = Fixture::new();
    let data = MapStyleStaticData::load_from_ron_data(f.as_ref(), 12).unwrap();
    assert_eq!(data.identity.key, "Mediterranean");
    assert_eq!(
        data.selected_goodies[0].attribute("numrare"),
        Some("4 SCALE")
    );
    assert_eq!(
        data.selected_goodies[0].attribute("spacing"),
        Some("8  SCALE")
    );
    assert_eq!(
        data.selected_terrain_groups[0].attribute("min_clumps"),
        Some("2 SCALE")
    );
}

#[test]
fn changing_one_catalog_key_is_rejected_before_xml_can_be_used() {
    let f = Fixture::new();
    f.write_catalog(Some((12, "Not Mediterranean")));
    assert!(matches!(
        MapStyleStaticData::load_from_ron_data(f.as_ref(), 12),
        Err(MapStyleLoadError::CatalogMismatch { index: 12, .. })
    ));
}

#[test]
fn changing_one_item_expression_changes_the_projection_and_file_evidence() {
    let f = Fixture::new();
    let before = MapStyleStaticData::load_from_ron_data(f.as_ref(), 12).unwrap();
    f.write("mapstyles/mediterranean.xml", &style_xml("5 SCALE", false));
    let after = MapStyleStaticData::load_from_ron_data(f.as_ref(), 12).unwrap();
    assert_ne!(
        before.selected_source.adler32,
        after.selected_source.adler32
    );
    assert_eq!(
        before.selected_goodies[0].attribute("numrare"),
        Some("4 SCALE")
    );
    assert_eq!(
        after.selected_goodies[0].attribute("numrare"),
        Some("5 SCALE")
    );
}

#[test]
fn duplicate_attributes_fail_closed() {
    let f = Fixture::new();
    f.write("mapstyles/mediterranean.xml", &style_xml("4 SCALE", true));
    assert!(matches!(
        MapStyleStaticData::load_from_ron_data(f.as_ref(), 12),
        Err(MapStyleLoadError::Malformed { .. })
    ));
}

#[test]
fn absent_selected_section_uses_default_but_is_not_confused_with_empty() {
    let f = Fixture::new();
    f.write("mapstyles/mediterranean.xml", style_xml_without_goodies());
    let data = MapStyleStaticData::load_from_ron_data(f.as_ref(), 12).unwrap();
    assert!(!data.selected_goodies_section_present);
    assert!(data.selected_goodies.is_empty());
    assert_eq!(data.effective_goodies()[0].attribute("numrare"), Some("1"));

    f.write(
        "mapstyles/mediterranean.xml",
        "<ROOT><MAP></MAP><TERRAIN_GROUPS></TERRAIN_GROUPS><GOODIES></GOODIES></ROOT>",
    );
    let empty = MapStyleStaticData::load_from_ron_data(f.as_ref(), 12).unwrap();
    assert!(empty.selected_goodies_section_present);
    assert!(empty.effective_goodies().is_empty());
}
