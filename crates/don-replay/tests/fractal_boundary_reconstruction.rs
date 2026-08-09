// SPDX-License-Identifier: GPL-3.0-or-later
//! Mutation-sensitive proof pack for the retail fertility-fractal boundary.

#[path = "../src/fractal_boundary.rs"]
mod fractal_boundary;

use fractal_boundary::{
    build_partitions, generate_retail_fractal, resolve_fertility_boundary_xml,
    FractalBoundaryError, FractalReplayInputs,
};

const DEFAULT_XML: &str = r#"
<ROOT>
  <MAP>
    <TILESET>
      <TILECHANCE type="Dirty" chance="18"/>
      <TILECHANCE type="Snowy" chance="82"/>
    </TILESET>
  </MAP>
  <TILESET_DATA>
    <DIRTY>
      <LANDKEY name="baseland" frequency_0="45" frequency_1="25" frequency_2="20" frequency_3="10"/>
    </DIRTY>
    <SNOWY>
      <LANDKEY name="baseland" frequency_0="50" frequency_1="0" frequency_2="0" frequency_3="50"/>
    </SNOWY>
  </TILESET_DATA>
</ROOT>
"#;

const SELECTED_WITHOUT_TILESET: &str = r#"
<ROOT>
  <MAP><SEA_MAP value="0"/></MAP>
</ROOT>
"#;

const TILESETS_XML: &str = r#"
<TILESETS>
  <TILESET name="Dirty">
    <TERRAINGROUP><CLUMP_FACTOR value="2"/></TERRAINGROUP>
    <BASELAND><BASE/><BASE/><BASE/><BASE/></BASELAND>
    <COASTAL><BASE/><BASE/></COASTAL>
  </TILESET>
  <TILESET name="Snowy">
    <TERRAINGROUP><CLUMP_FACTOR value="3"/></TERRAINGROUP>
    <BASELAND><BASE/><BASE/><BASE/><BASE/></BASELAND>
  </TILESET>
</TILESETS>
"#;

fn inputs() -> FractalReplayInputs {
    FractalReplayInputs {
        seed: 108,
        map_style: 12,
        scenario_type: 0,
        world_edge: Some(8),
    }
}

#[test]
fn fractal_init_freezes_every_interior_byte_and_both_guard_edges() {
    let plane = generate_retail_fractal(8, 8, 2, 1).unwrap();
    let expected_x_major = vec![
        89, 69, 101, 92, 133, 87, 98, 61, 119, 98, 60, 86, 143, 84, 54, 47, 90, 75, 53, 86, 146,
        99, 63, 52, 72, 58, 31, 93, 123, 57, 46, 58, 1, 37, 37, 21, 58, 59, 85, 38, 18, 2, 61, 61,
        124, 81, 95, 44, 41, 13, 42, 98, 150, 103, 75, 32, 72, 73, 55, 99, 123, 120, 69, 25,
    ];

    assert_eq!(plane.flat_x_major(), expected_x_major);
    assert_eq!(plane.columns.len(), 9);
    assert!(plane.columns.iter().all(|column| column.len() == 9));
    assert_eq!(
        plane.columns[8], plane.columns[0],
        "flags=0 wraps the X guard"
    );
    assert!(
        plane.columns.iter().all(|column| column[8] == 0),
        "the unused Y guard remains zero"
    );
    assert_eq!(plane.random_draws, 64);
    assert_eq!(plane.random_state_after, 0x9c69_3a41_u32 as i32);
}

#[test]
fn partitions_freeze_binary32_truncation_narrowing_and_cumulative_addition() {
    assert_eq!(build_partitions(&[45, 25, 20, 10]), vec![114, 177, 228]);
    assert_eq!(build_partitions(&[200, 200, 0]), vec![254, 252]);
    assert!(build_partitions(&[100]).is_empty());
}

#[test]
fn resolver_closes_tile_selection_static_inputs_and_fill_fertile_adapter() {
    let resolved = resolve_fertility_boundary_xml(
        inputs(),
        DEFAULT_XML,
        SELECTED_WITHOUT_TILESET,
        TILESETS_XML,
    )
    .unwrap();

    // Random::get returns 218 for seed 108. Retail then takes `% 100` and uses
    // `jle`, so the boundary value 18 remains in Dirty's chance-18 bucket.
    assert_eq!(resolved.tile_selection_draw, Some(218));
    assert_eq!(resolved.tile_selection_bucket, 18);
    assert_eq!(resolved.main_random_state_after_tileset, 1_193_672_923);
    assert_eq!(resolved.tileset, "Dirty");
    assert_eq!(resolved.clump_factor, 2);
    assert_eq!(resolved.baseland_frequencies, vec![45, 25, 20, 10]);
    assert_eq!(resolved.partitions, vec![114, 177, 228]);

    let terrain_groups = resolved.terrain_groups_input();
    assert_eq!(terrain_groups.fractal.columns, resolved.plane.columns);
    assert_eq!(terrain_groups.partitions, resolved.partitions);
}

#[test]
fn selected_subtrees_override_default_and_one_weight_consumes_no_draw() {
    let selected = r#"
    <ROOT>
      <MAP><TILESET><TILECHANCE type="Snowy" chance="1"/></TILESET></MAP>
      <TILESET_DATA>
        <SNOWY>
          <LANDKEY name="baseland" frequency_0="25" frequency_1="25" frequency_2="25" frequency_3="25"/>
        </SNOWY>
      </TILESET_DATA>
    </ROOT>
    "#;
    let resolved =
        resolve_fertility_boundary_xml(inputs(), DEFAULT_XML, selected, TILESETS_XML).unwrap();

    assert_eq!(resolved.tileset, "Snowy");
    assert_eq!(resolved.tile_selection_draw, None);
    assert_eq!(resolved.main_random_state_after_tileset, 108);
    assert_eq!(resolved.clump_factor, 3);
    assert_eq!(resolved.baseland_frequencies, vec![25, 25, 25, 25]);
    assert_eq!(resolved.partitions, vec![63, 126, 189]);
}

#[test]
fn unresolved_installed_content_and_incomplete_frequency_rows_fail_closed() {
    let missing_tileset = DEFAULT_XML.replace("type=\"Dirty\"", "type=\"Bogus\"");
    assert!(matches!(
        resolve_fertility_boundary_xml(
            inputs(),
            &missing_tileset,
            SELECTED_WITHOUT_TILESET,
            TILESETS_XML,
        ),
        Err(FractalBoundaryError::TilesetNotInstalled { tileset }) if tileset == "Bogus"
    ));

    let incomplete = DEFAULT_XML.replace(" frequency_3=\"10\"", "");
    assert!(matches!(
        resolve_fertility_boundary_xml(
            inputs(),
            &incomplete,
            SELECTED_WITHOUT_TILESET,
            TILESETS_XML,
        ),
        Err(FractalBoundaryError::MissingBaselandFrequency {
            tileset,
            index: 3,
        }) if tileset == "Dirty"
    ));
}
