// SPDX-License-Identifier: GPL-3.0-or-later

// Keep the producer compile-checked before its shared registry/caller integration exists.
mod systems {
    pub mod mountain_add_runtime {
        pub use don_sim::systems::mountain_add_runtime::*;
    }
}

mod checksum {
    pub use don_sim::checksum::*;
}

#[path = "../src/systems/mountain_template_producer.rs"]
mod mountain_template_producer;

use mountain_template_producer::{
    derive_mountain_template_from_tga, load_mountain_template_catalog,
    parse_mountain_template_sources, MountainTemplateProducerError, MOUNTAIN_RANGE_INIT_SIZE,
    MOUNTAIN_RANGE_INIT_VA,
};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

fn encode_tga(
    width: usize,
    height: usize,
    canonical_alpha: &[u8],
    image_type: u8,
    descriptor: u8,
) -> Vec<u8> {
    assert_eq!(canonical_alpha.len(), width * height);
    let mut out = vec![0u8; 18];
    out[2] = image_type;
    out[12..14].copy_from_slice(&(width as u16).to_le_bytes());
    out[14..16].copy_from_slice(&(height as u16).to_le_bytes());
    out[16] = 32;
    out[17] = descriptor | 8;

    let mut file_pixels = Vec::with_capacity(width * height);
    let right_origin = descriptor & 0x10 != 0;
    let top_origin = descriptor & 0x20 != 0;
    for file_y in 0..height {
        for file_x in 0..width {
            let x = if right_origin {
                width - 1 - file_x
            } else {
                file_x
            };
            let y = if top_origin {
                file_y
            } else {
                height - 1 - file_y
            };
            file_pixels.push(canonical_alpha[y * width + x]);
        }
    }

    if image_type == 2 {
        for (index, alpha) in file_pixels.into_iter().enumerate() {
            // Non-zero RGB on transparent pixels catches a decoder that reads a color byte.
            out.extend_from_slice(&[0x41, 0x82, (index as u8).wrapping_add(1), alpha]);
        }
    } else {
        let mut cursor = 0usize;
        while cursor < file_pixels.len() {
            let count = (file_pixels.len() - cursor).min(128);
            out.push((count - 1) as u8); // raw RLE packet
            for offset in 0..count {
                let alpha = file_pixels[cursor + offset];
                out.extend_from_slice(&[0x41, 0x82, (cursor + offset + 1) as u8, alpha]);
            }
            cursor += count;
        }
    }
    out
}

fn dense_alpha(width: usize, height: usize) -> Vec<u8> {
    vec![1; width * height]
}

fn dense_rle_tga(width: usize, height: usize, alpha: u8) -> Vec<u8> {
    let mut out = vec![0u8; 18];
    out[2] = 10;
    out[12..14].copy_from_slice(&(width as u16).to_le_bytes());
    out[14..16].copy_from_slice(&(height as u16).to_le_bytes());
    out[16] = 32;
    out[17] = 0x28;
    let mut remaining = width * height;
    while remaining != 0 {
        let count = remaining.min(128);
        out.push(0x80 | (count - 1) as u8);
        out.extend_from_slice(&[0x41, 0x82, 0xc3, alpha]);
        remaining -= count;
    }
    out
}

#[test]
fn dense_non_multiple_surface_pins_center_alignment_and_order() {
    let tga = encode_tga(36, 36, &dense_alpha(36, 36), 2, 0x20);
    let product = derive_mountain_template_from_tga(&tga).unwrap();

    assert_eq!(product.mount_tiles.len(), 64);
    assert_eq!(product.mount_tiles.first().unwrap().x, -4);
    assert_eq!(product.mount_tiles.first().unwrap().y, -4);
    assert_eq!(product.mount_tiles.last().unwrap().x, 3);
    assert_eq!(product.mount_tiles.last().unwrap().y, 3);
    assert_eq!(
        product
            .mount_wcoords
            .iter()
            .map(|offset| (offset.x, offset.y))
            .collect::<Vec<_>>(),
        vec![(-1, -1), (0, -1), (-1, 0), (0, 0)]
    );
    assert_eq!(
        product
            .solid_mount_wcoords
            .iter()
            .map(|offset| (offset.x, offset.y))
            .collect::<Vec<_>>(),
        vec![(-1, -1), (0, -1), (-1, 0), (0, 0)]
    );
}

#[test]
fn alpha_not_color_drives_occupancy_and_any_nonzero_alpha_counts() {
    let mut alpha = vec![0; 36 * 36];
    for y in [2, 6] {
        for x in [2, 6] {
            alpha[y * 36 + x] = 1;
        }
    }
    let product = derive_mountain_template_from_tga(&encode_tga(36, 36, &alpha, 2, 0x20)).unwrap();
    assert_eq!(product.mount_tiles.len(), 1);
    assert_eq!(
        (product.mount_tiles[0].x, product.mount_tiles[0].y),
        (-4, -4)
    );
    assert_eq!(product.mount_wcoords.len(), 1);
    assert!(product.solid_mount_wcoords.is_empty());

    let transparent =
        derive_mountain_template_from_tga(&encode_tga(36, 36, &vec![0; 36 * 36], 2, 0x20)).unwrap();
    assert!(transparent.mount_tiles.is_empty());
    assert!(transparent.mount_wcoords.is_empty());
    assert!(transparent.solid_mount_wcoords.is_empty());
}

#[test]
fn solid_footprint_uses_the_retail_sixteen_of_twenty_five_threshold() {
    let sample_points = [0usize, 4, 8, 12, 16];
    let mut alpha = vec![0; 36 * 36];
    for (ordinal, (dy, dx)) in sample_points
        .into_iter()
        .flat_map(|dy| sample_points.into_iter().map(move |dx| (dy, dx)))
        .enumerate()
    {
        if ordinal < 16 {
            alpha[(2 + dy) * 36 + 2 + dx] = 1;
        }
    }
    let included = derive_mountain_template_from_tga(&encode_tga(36, 36, &alpha, 2, 0x20)).unwrap();
    assert_eq!(included.solid_mount_wcoords.len(), 1);
    assert_eq!(
        (
            included.solid_mount_wcoords[0].x,
            included.solid_mount_wcoords[0].y
        ),
        (-1, -1)
    );

    alpha[2 * 36 + 2] = 0;
    let excluded = derive_mountain_template_from_tga(&encode_tga(36, 36, &alpha, 2, 0x20)).unwrap();
    assert!(excluded.solid_mount_wcoords.is_empty());
}

#[test]
fn tga_rle_and_all_four_origins_normalize_to_one_surface() {
    let mut alpha = vec![0; 36 * 36];
    for y in (2..36).step_by(4) {
        for x in (2..36).step_by(4) {
            if x < 18 || y >= 18 {
                alpha[y * 36 + x] = ((x + y) % 251 + 1) as u8;
            }
        }
    }
    let expected = derive_mountain_template_from_tga(&encode_tga(36, 36, &alpha, 2, 0)).unwrap();
    for descriptor in [0x00, 0x10, 0x20, 0x30] {
        let actual =
            derive_mountain_template_from_tga(&encode_tga(36, 36, &alpha, 10, descriptor)).unwrap();
        assert_eq!(actual, expected, "descriptor {descriptor:#x}");
    }
}

#[test]
fn repeated_rle_packets_may_cross_rows_without_changing_geometry() {
    let expected =
        derive_mountain_template_from_tga(&encode_tga(36, 36, &dense_alpha(36, 36), 2, 0x20))
            .unwrap();
    let actual = derive_mountain_template_from_tga(&dense_rle_tga(36, 36, 1)).unwrap();
    assert_eq!(actual, expected);
}

#[test]
fn malformed_or_overlong_tga_streams_fail_closed() {
    assert_eq!(
        derive_mountain_template_from_tga(&[0; 17]),
        Err(MountainTemplateProducerError::TgaTooShort)
    );

    let mut unsupported = encode_tga(1, 1, &[1], 2, 0x20);
    unsupported[1] = 1;
    assert!(matches!(
        derive_mountain_template_from_tga(&unsupported),
        Err(MountainTemplateProducerError::UnsupportedTga { .. })
    ));

    let mut overflowing_packet = vec![0u8; 18];
    overflowing_packet[2] = 10;
    overflowing_packet[12..14].copy_from_slice(&1u16.to_le_bytes());
    overflowing_packet[14..16].copy_from_slice(&1u16.to_le_bytes());
    overflowing_packet[16] = 32;
    overflowing_packet.extend_from_slice(&[0x81, 0, 0, 0, 0xff]);
    assert_eq!(
        derive_mountain_template_from_tga(&overflowing_packet),
        Err(MountainTemplateProducerError::RlePixelOverflow)
    );
}

fn fixture_xml(rows: &[(&str, &str, &str, &str)]) -> String {
    let mut xml = String::from("<ROOT><MOUNTAINS>");
    for (area, disp, main, ring) in rows {
        xml.push_str(&format!(
            "<MOUNTAIN area=\"{area}\"><TEMPLATE_TEX file=\"{disp}\"/><MAIN_ALPHA_TEX file=\"{main}\"/><RING_ALPHA_TEX file=\"{ring}\"/></MOUNTAIN>"
        ));
    }
    xml.push_str("</MOUNTAINS></ROOT>");
    xml
}

#[test]
fn xml_order_is_template_identity_and_the_three_file_gate_is_strict() {
    assert_eq!(MOUNTAIN_RANGE_INIT_VA, 0x0089_98b0);
    assert_eq!(MOUNTAIN_RANGE_INIT_SIZE, 5_190);
    let xml = fixture_xml(&[
        ("lg", ".\\art\\first.tga", "first-main", "first-ring"),
        ("sm", ".\\art\\second.tga", "second-main", "second-ring"),
    ]);
    let sources = parse_mountain_template_sources(xml.as_bytes()).unwrap();
    assert_eq!(sources.len(), 2);
    assert_eq!(sources[0].index, 0);
    assert_eq!(sources[0].area, "lg");
    assert_eq!(sources[1].index, 1);
    assert_eq!(sources[1].displacement_path, ".\\art\\second.tga");

    let missing = b"<ROOT><MOUNTAINS><MOUNTAIN area=\"lg\"><TEMPLATE_TEX file=\"x\"/><MAIN_ALPHA_TEX file=\"y\"/></MOUNTAIN></MOUNTAINS></ROOT>";
    assert_eq!(
        parse_mountain_template_sources(missing),
        Err(MountainTemplateProducerError::MissingTextureElement {
            mountain: 0,
            element: "RING_ALPHA_TEX"
        })
    );
}

#[test]
fn shipped_xml_census_binds_all_sixteen_template_indices_when_available() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../ron-data/effects_graphics.xml");
    if !path.is_file() {
        return;
    }
    let xml = fs::read(path).unwrap();
    let sources = parse_mountain_template_sources(&xml).unwrap();
    assert_eq!(sources.len(), 16);
    assert_eq!(sources.iter().filter(|row| row.area == "lg").count(), 7);
    assert_eq!(sources.iter().filter(|row| row.area == "med").count(), 8);
    assert_eq!(sources.iter().filter(|row| row.area == "sm").count(), 1);
    assert_eq!(sources.first().unwrap().index, 0);
    assert_eq!(sources.last().unwrap().index, 15);
    assert!(sources
        .iter()
        .all(|row| row.displacement_path.to_ascii_lowercase().ends_with(".tga")));
}

static TEMP_ID: AtomicU64 = AtomicU64::new(0);

fn temp_root() -> PathBuf {
    let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("don-mountain-template-{}-{id}", std::process::id()))
}

#[test]
fn installed_catalog_reads_only_source_named_tgas_and_preserves_index_order() {
    let root = temp_root();
    fs::create_dir_all(root.join("art")).unwrap();
    let xml = fixture_xml(&[
        ("lg", ".\\art\\first.tga", "main-a", "ring-a"),
        ("med", ".\\art\\second.tga", "main-b", "ring-b"),
    ]);
    let xml_path = root.join("effects_graphics.xml");
    fs::write(&xml_path, &xml).unwrap();
    fs::write(
        root.join("art/first.tga"),
        encode_tga(36, 36, &dense_alpha(36, 36), 2, 0x20),
    )
    .unwrap();
    fs::write(
        root.join("art/second.tga"),
        encode_tga(36, 36, &vec![0; 36 * 36], 2, 0x20),
    )
    .unwrap();

    let catalog = load_mountain_template_catalog(&xml_path, &root).unwrap();
    assert_eq!(catalog.effects_graphics_xml.path, xml_path);
    assert_eq!(catalog.effects_graphics_xml.byte_length, xml.len());
    assert_ne!(catalog.effects_graphics_xml.adler32, 1);
    assert_eq!(catalog.sources[0].index, 0);
    assert_eq!(catalog.sources[1].index, 1);
    assert_eq!(catalog.displacement_tgas.len(), 2);
    assert_eq!(
        catalog.displacement_tgas[0].path,
        root.join("art/first.tga")
    );
    assert_ne!(
        catalog.displacement_tgas[0].adler32,
        catalog.displacement_tgas[1].adler32
    );
    assert_eq!(catalog.templates[0].mount_tiles.len(), 64);
    assert!(catalog.templates[1].mount_tiles.is_empty());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn installed_asset_path_cannot_escape_the_content_root() {
    let root = temp_root();
    fs::create_dir_all(&root).unwrap();
    let xml = fixture_xml(&[("lg", "..\\outside.tga", "main", "ring")]);
    let xml_path = root.join("effects_graphics.xml");
    fs::write(&xml_path, xml).unwrap();
    let error = load_mountain_template_catalog(&xml_path, &root).unwrap_err();
    assert_eq!(
        error,
        MountainTemplateProducerError::UnsafeAssetPath {
            mountain: 0,
            path: "..\\outside.tga".to_owned()
        }
    );
    fs::remove_dir_all(root).unwrap();
}
