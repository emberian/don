// SPDX-License-Identifier: GPL-3.0-or-later
//! Installed-content producer for retail `MountainRangeData` displacement footprints.
//!
//! `MountainRange::init` (`0x0089_98b0`) does not copy a compiled geometry catalog.
//! It loads the `TEMPLATE_TEX` named by `effects_graphics.xml`, normalizes the TGA into a
//! top-left row-major 32-bit surface, and derives the three coordinate pairs consumed by
//! `Mountains::add_mountain`.  This module reproduces that source-to-runtime boundary; it
//! deliberately contains no shipped displacement pixels or precomputed footprint rows.
//!
//! The replay owner installs this producer only from an explicit user-owned content provider.
//! Missing or incomplete art remains a typed boundary; no default geometry is synthesized.

use crate::checksum::adler32;
use crate::systems::mountain_add_runtime::{GridOffset, MountainTemplateRuntime};
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};

/// PDB `MountainRange::init` and matching code size.
pub const MOUNTAIN_RANGE_INIT_VA: u32 = 0x0089_98b0;
pub const MOUNTAIN_RANGE_INIT_SIZE: u32 = 5_190;
/// Exact supported-PE body identity for `0x008998b0..0x0089acf6`.
pub const MOUNTAIN_RANGE_INIT_SHA256: &str =
    "1b7ac0662a6c23c7a74a23d301255763e636ef952b2e0a90dfd8929478d6f819";
/// Shipped `MountainsData::ranges` capacity established by `Mountains::add_range`.
pub const MOUNTAIN_TEMPLATE_CAPACITY: usize = 16;

/// One installed `<MOUNTAIN>` row, retained in document/template-index order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MountainTemplateSource {
    pub index: usize,
    pub area: String,
    /// Exact IEEE-754 word parsed from the required `<MOUNTAIN height>` attribute.
    pub height_bits: u32,
    pub displacement_path: String,
    pub main_alpha_path: String,
    pub ring_alpha_path: String,
}

/// Evidence for one installed file read by this producer.
///
/// The digest and geometry are computed from the same in-memory byte buffer,
/// so the receipt cannot name one file version while deriving another. The
/// installed bytes themselves remain user-owned and are not retained.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MountainTemplateFileEvidence {
    pub path: PathBuf,
    pub byte_length: usize,
    pub adler32: u32,
}

/// Geometry products and their installed-source bindings in one atomic catalog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MountainTemplateCatalog {
    pub effects_graphics_xml: MountainTemplateFileEvidence,
    pub sources: Vec<MountainTemplateSource>,
    pub displacement_tgas: Vec<MountainTemplateFileEvidence>,
    pub templates: Vec<MountainTemplateRuntime>,
    /// Ordered `MountainRangeOut::tcoord_verts` rows from the same decoded images.
    pub tcoord_vertices: Vec<Vec<MountainTcoordVertex>>,
    // Prevent downstream code from asserting a catalog without executing the
    // installed-content loader above. The public rows remain immutable evidence.
    source_boundary: (),
}

/// One exact `Vert3` produced by the first, four-pixel `MountainRange::init` pass.
///
/// The height-writing `fill_mountain_data(..., arg6=0, arg7=0)` call consumes this
/// array, not the later eight-pixel face mesh. Raw float words prevent host-side
/// canonicalisation of the source-derived coordinates and height.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct MountainTcoordVertex {
    pub x_bits: u32,
    pub y_bits: u32,
    pub z_bits: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MountainTemplateProducerError {
    Read {
        path: String,
        message: String,
    },
    Xml(String),
    MissingMountainsSection,
    DuplicateMountainsSection,
    MountainOutsideSection,
    NestedMountain,
    MissingTextureElement {
        mountain: usize,
        element: &'static str,
    },
    DuplicateTextureElement {
        mountain: usize,
        element: &'static str,
    },
    MissingAttribute {
        mountain: usize,
        element: &'static str,
        attribute: &'static str,
    },
    DuplicateAttribute {
        mountain: usize,
        element: &'static str,
        attribute: &'static str,
    },
    EmptyAttribute {
        mountain: usize,
        element: &'static str,
        attribute: &'static str,
    },
    InvalidFloatAttribute {
        mountain: usize,
        element: &'static str,
        attribute: &'static str,
        value: String,
    },
    TooManyTemplates {
        actual: usize,
    },
    UnsafeAssetPath {
        mountain: usize,
        path: String,
    },
    TgaTooShort,
    UnsupportedTga {
        image_type: u8,
        color_map_type: u8,
        pixel_depth: u8,
        x_origin: u16,
        y_origin: u16,
    },
    EmptyTga,
    NonSquareTga {
        width: usize,
        height: usize,
    },
    TruncatedTga,
    TgaPixelOverflow,
    RlePixelOverflow,
}

impl fmt::Display for MountainTemplateProducerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, message } => write!(f, "could not read {path}: {message}"),
            Self::Xml(message) => write!(f, "malformed effects_graphics.xml: {message}"),
            Self::MissingMountainsSection => write!(f, "missing MOUNTAINS section"),
            Self::DuplicateMountainsSection => write!(f, "duplicate MOUNTAINS section"),
            Self::MountainOutsideSection => write!(f, "MOUNTAIN outside MOUNTAINS"),
            Self::NestedMountain => write!(f, "nested MOUNTAIN element"),
            Self::MissingTextureElement { mountain, element } => {
                write!(f, "mountain {mountain} is missing {element}")
            }
            Self::DuplicateTextureElement { mountain, element } => {
                write!(f, "mountain {mountain} repeats {element}")
            }
            Self::MissingAttribute {
                mountain,
                element,
                attribute,
            } => write!(f, "mountain {mountain} {element} is missing {attribute}"),
            Self::DuplicateAttribute {
                mountain,
                element,
                attribute,
            } => write!(f, "mountain {mountain} {element} repeats {attribute}"),
            Self::EmptyAttribute {
                mountain,
                element,
                attribute,
            } => write!(f, "mountain {mountain} {element} has empty {attribute}"),
            Self::InvalidFloatAttribute {
                mountain,
                element,
                attribute,
                value,
            } => write!(
                f,
                "mountain {mountain} {element} has invalid {attribute} float {value}"
            ),
            Self::TooManyTemplates { actual } => write!(
                f,
                "{actual} mountain templates exceed the retail capacity of {MOUNTAIN_TEMPLATE_CAPACITY}"
            ),
            Self::UnsafeAssetPath { mountain, path } => {
                write!(f, "mountain {mountain} has unsafe installed path {path}")
            }
            Self::TgaTooShort => write!(f, "TGA is shorter than its 18-byte header"),
            Self::UnsupportedTga {
                image_type,
                color_map_type,
                pixel_depth,
                x_origin,
                y_origin,
            } => write!(
                f,
                "unsupported TGA type={image_type} cmap={color_map_type} depth={pixel_depth} origin=({x_origin},{y_origin})"
            ),
            Self::EmptyTga => write!(f, "TGA has a zero dimension"),
            Self::NonSquareTga { width, height } => write!(
                f,
                "TGA {width}x{height} is outside the exact square MountainRange domain"
            ),
            Self::TruncatedTga => write!(f, "TGA pixel stream is truncated"),
            Self::TgaPixelOverflow => write!(f, "TGA dimensions overflow the host"),
            Self::RlePixelOverflow => write!(f, "TGA RLE packet exceeds the declared image"),
        }
    }
}

impl std::error::Error for MountainTemplateProducerError {}

#[derive(Default)]
struct MountainSourceBuilder {
    index: usize,
    area: String,
    height_bits: u32,
    displacement_path: Option<String>,
    main_alpha_path: Option<String>,
    ring_alpha_path: Option<String>,
}

/// Load the installed `effects_graphics.xml` and every referenced displacement TGA.
///
/// Asset paths are confined beneath `content_root`. Retail strings use Windows separators,
/// so `.` and `\` are normalized before that confinement check. Main/ring alpha names are
/// required because `Mountains::init` admits a row only when all three XML `file` strings
/// are non-empty; only `TEMPLATE_TEX` contributes to checksum-relevant geometry.
pub fn load_mountain_template_catalog(
    effects_graphics_xml: &Path,
    content_root: &Path,
) -> Result<MountainTemplateCatalog, MountainTemplateProducerError> {
    let xml =
        fs::read(effects_graphics_xml).map_err(|error| MountainTemplateProducerError::Read {
            path: effects_graphics_xml.display().to_string(),
            message: error.to_string(),
        })?;
    let sources = parse_mountain_template_sources(&xml)?;
    let effects_graphics_xml = file_evidence(effects_graphics_xml, &xml);
    let mut displacement_tgas = Vec::with_capacity(sources.len());
    let mut templates = Vec::with_capacity(sources.len());
    let mut tcoord_vertices = Vec::with_capacity(sources.len());
    for source in &sources {
        let path = confined_asset_path(content_root, source.index, &source.displacement_path)?;
        let bytes = fs::read(&path).map_err(|error| MountainTemplateProducerError::Read {
            path: path.display().to_string(),
            message: error.to_string(),
        })?;
        displacement_tgas.push(file_evidence(&path, &bytes));
        let geometry = derive_mountain_template_geometry_from_tga(&bytes, source.height_bits)?;
        templates.push(geometry.runtime);
        tcoord_vertices.push(geometry.tcoord_vertices);
    }
    Ok(MountainTemplateCatalog {
        effects_graphics_xml,
        sources,
        displacement_tgas,
        templates,
        tcoord_vertices,
        source_boundary: (),
    })
}

fn file_evidence(path: &Path, bytes: &[u8]) -> MountainTemplateFileEvidence {
    MountainTemplateFileEvidence {
        path: path.to_path_buf(),
        byte_length: bytes.len(),
        adler32: adler32(1, bytes),
    }
}

/// Parse the `MOUNTAINS` catalog without opening its proprietary image inputs.
pub fn parse_mountain_template_sources(
    xml: &[u8],
) -> Result<Vec<MountainTemplateSource>, MountainTemplateProducerError> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(true);
    let mut saw_mountains = false;
    let mut in_mountains = false;
    let mut current = None::<MountainSourceBuilder>;
    let mut result = Vec::new();

    loop {
        let event = reader
            .read_event()
            .map_err(|error| MountainTemplateProducerError::Xml(error.to_string()))?;
        match event {
            Event::Start(ref element) => match element.name().as_ref() {
                b"MOUNTAINS" => {
                    if saw_mountains || in_mountains {
                        return Err(MountainTemplateProducerError::DuplicateMountainsSection);
                    }
                    saw_mountains = true;
                    in_mountains = true;
                }
                b"MOUNTAIN" => {
                    if !in_mountains {
                        return Err(MountainTemplateProducerError::MountainOutsideSection);
                    }
                    if current.is_some() {
                        return Err(MountainTemplateProducerError::NestedMountain);
                    }
                    let index = result.len();
                    let area = required_attr(element, index, "MOUNTAIN", b"area", "area")?;
                    let height = required_attr(element, index, "MOUNTAIN", b"height", "height")?;
                    let parsed_height = height.parse::<f32>().map_err(|_| {
                        MountainTemplateProducerError::InvalidFloatAttribute {
                            mountain: index,
                            element: "MOUNTAIN",
                            attribute: "height",
                            value: height.clone(),
                        }
                    })?;
                    if !parsed_height.is_finite() {
                        return Err(MountainTemplateProducerError::InvalidFloatAttribute {
                            mountain: index,
                            element: "MOUNTAIN",
                            attribute: "height",
                            value: height,
                        });
                    }
                    let height_bits = parsed_height.to_bits();
                    current = Some(MountainSourceBuilder {
                        index,
                        area,
                        height_bits,
                        ..MountainSourceBuilder::default()
                    });
                }
                b"TEMPLATE_TEX" | b"MAIN_ALPHA_TEX" | b"RING_ALPHA_TEX" => {
                    set_texture_path(&mut current, element)?;
                }
                _ => {}
            },
            Event::Empty(ref element) => match element.name().as_ref() {
                b"TEMPLATE_TEX" | b"MAIN_ALPHA_TEX" | b"RING_ALPHA_TEX" => {
                    set_texture_path(&mut current, element)?;
                }
                b"MOUNTAIN" => {
                    if !in_mountains {
                        return Err(MountainTemplateProducerError::MountainOutsideSection);
                    }
                    return Err(MountainTemplateProducerError::MissingTextureElement {
                        mountain: result.len(),
                        element: "TEMPLATE_TEX",
                    });
                }
                _ => {}
            },
            Event::End(ref element) => match element.name().as_ref() {
                b"MOUNTAIN" => {
                    let builder = current
                        .take()
                        .ok_or(MountainTemplateProducerError::MountainOutsideSection)?;
                    result.push(finish_source(builder)?);
                    if result.len() > MOUNTAIN_TEMPLATE_CAPACITY {
                        return Err(MountainTemplateProducerError::TooManyTemplates {
                            actual: result.len(),
                        });
                    }
                }
                b"MOUNTAINS" => {
                    if !in_mountains || current.is_some() {
                        return Err(MountainTemplateProducerError::NestedMountain);
                    }
                    in_mountains = false;
                }
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }

    if !saw_mountains {
        return Err(MountainTemplateProducerError::MissingMountainsSection);
    }
    if in_mountains || current.is_some() {
        return Err(MountainTemplateProducerError::Xml(
            "unterminated MOUNTAINS/MOUNTAIN element".to_owned(),
        ));
    }
    Ok(result)
}

fn required_attr(
    element: &BytesStart<'_>,
    mountain: usize,
    element_name: &'static str,
    key: &[u8],
    attribute_name: &'static str,
) -> Result<String, MountainTemplateProducerError> {
    let mut found = None;
    for attribute in element.attributes() {
        let attribute =
            attribute.map_err(|error| MountainTemplateProducerError::Xml(error.to_string()))?;
        if attribute.key.as_ref() != key {
            continue;
        }
        if found.is_some() {
            return Err(MountainTemplateProducerError::DuplicateAttribute {
                mountain,
                element: element_name,
                attribute: attribute_name,
            });
        }
        let value = std::str::from_utf8(attribute.value.as_ref())
            .map_err(|error| MountainTemplateProducerError::Xml(error.to_string()))?
            .to_owned();
        if value.is_empty() {
            return Err(MountainTemplateProducerError::EmptyAttribute {
                mountain,
                element: element_name,
                attribute: attribute_name,
            });
        }
        found = Some(value);
    }
    found.ok_or(MountainTemplateProducerError::MissingAttribute {
        mountain,
        element: element_name,
        attribute: attribute_name,
    })
}

fn set_texture_path(
    current: &mut Option<MountainSourceBuilder>,
    element: &BytesStart<'_>,
) -> Result<(), MountainTemplateProducerError> {
    let builder = current
        .as_mut()
        .ok_or(MountainTemplateProducerError::MountainOutsideSection)?;
    let (name, slot) = match element.name().as_ref() {
        b"TEMPLATE_TEX" => ("TEMPLATE_TEX", &mut builder.displacement_path),
        b"MAIN_ALPHA_TEX" => ("MAIN_ALPHA_TEX", &mut builder.main_alpha_path),
        b"RING_ALPHA_TEX" => ("RING_ALPHA_TEX", &mut builder.ring_alpha_path),
        _ => unreachable!("caller filters texture elements"),
    };
    if slot.is_some() {
        return Err(MountainTemplateProducerError::DuplicateTextureElement {
            mountain: builder.index,
            element: name,
        });
    }
    *slot = Some(required_attr(
        element,
        builder.index,
        name,
        b"file",
        "file",
    )?);
    Ok(())
}

fn finish_source(
    builder: MountainSourceBuilder,
) -> Result<MountainTemplateSource, MountainTemplateProducerError> {
    let require = |value: Option<String>, element: &'static str| {
        value.ok_or(MountainTemplateProducerError::MissingTextureElement {
            mountain: builder.index,
            element,
        })
    };
    Ok(MountainTemplateSource {
        index: builder.index,
        area: builder.area,
        height_bits: builder.height_bits,
        displacement_path: require(builder.displacement_path, "TEMPLATE_TEX")?,
        main_alpha_path: require(builder.main_alpha_path, "MAIN_ALPHA_TEX")?,
        ring_alpha_path: require(builder.ring_alpha_path, "RING_ALPHA_TEX")?,
    })
}

fn confined_asset_path(
    root: &Path,
    mountain: usize,
    retail_path: &str,
) -> Result<PathBuf, MountainTemplateProducerError> {
    let normalized = retail_path.replace('\\', "/");
    let mut relative = PathBuf::new();
    for component in Path::new(&normalized).components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => relative.push(part),
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(MountainTemplateProducerError::UnsafeAssetPath {
                    mountain,
                    path: retail_path.to_owned(),
                });
            }
        }
    }
    if relative.as_os_str().is_empty() {
        return Err(MountainTemplateProducerError::UnsafeAssetPath {
            mountain,
            path: retail_path.to_owned(),
        });
    }
    Ok(root.join(relative))
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MountainImage {
    width: usize,
    height: usize,
    /// Top-left row-major red and alpha, matching `ImageIO::decode_tga`'s normalized
    /// in-memory R,G,B,A texture surface.
    red: Vec<u8>,
    alpha: Vec<u8>,
}

impl MountainImage {
    #[inline]
    fn present(&self, x: usize, y: usize) -> bool {
        x < self.width && y < self.height && self.alpha[y * self.width + x] != 0
    }
}

/// Decode a retail-supported true-color TGA and derive the checksum-relevant footprint.
pub fn derive_mountain_template_from_tga(
    tga: &[u8],
) -> Result<MountainTemplateRuntime, MountainTemplateProducerError> {
    Ok(derive_from_image(decode_tga(tga)?)?.runtime)
}

/// Atomic source product used by mountain placement and the later terrain-height pass.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MountainTemplateGeometry {
    pub runtime: MountainTemplateRuntime,
    pub tcoord_vertices: Vec<MountainTcoordVertex>,
}

/// Decode one installed displacement TGA once and derive both native consumers.
pub fn derive_mountain_template_geometry_from_tga(
    tga: &[u8],
    height_bits: u32,
) -> Result<MountainTemplateGeometry, MountainTemplateProducerError> {
    derive_geometry(decode_tga(tga)?, height_bits)
}

#[derive(Copy, Clone)]
struct MountainPixel {
    red: u8,
    alpha: u8,
}

fn decode_tga(tga: &[u8]) -> Result<MountainImage, MountainTemplateProducerError> {
    if tga.len() < 18 {
        return Err(MountainTemplateProducerError::TgaTooShort);
    }
    let id_len = usize::from(tga[0]);
    let color_map_type = tga[1];
    let image_type = tga[2];
    let color_map_origin = u16::from_le_bytes([tga[3], tga[4]]);
    let color_map_length = u16::from_le_bytes([tga[5], tga[6]]);
    let color_map_depth = tga[7];
    let x_origin = u16::from_le_bytes([tga[8], tga[9]]);
    let y_origin = u16::from_le_bytes([tga[10], tga[11]]);
    let width = usize::from(u16::from_le_bytes([tga[12], tga[13]]));
    let height = usize::from(u16::from_le_bytes([tga[14], tga[15]]));
    let pixel_depth = tga[16];
    let descriptor = tga[17];

    if color_map_type != 0
        || !matches!(image_type, 2 | 10)
        || !matches!(pixel_depth, 24 | 32)
        || color_map_origin != 0
        || color_map_length != 0
        || color_map_depth != 0
        || x_origin != 0
        || y_origin != 0
    {
        return Err(MountainTemplateProducerError::UnsupportedTga {
            image_type,
            color_map_type,
            pixel_depth,
            x_origin,
            y_origin,
        });
    }
    if width == 0 || height == 0 {
        return Err(MountainTemplateProducerError::EmptyTga);
    }
    // Retail's first vertex and face loops compare y with image width and x with
    // image height while indexing y*width+x. Shipped displacement maps are square;
    // a rectangular input would make the native body walk out of bounds.
    if width != height {
        return Err(MountainTemplateProducerError::NonSquareTga { width, height });
    }
    let pixel_count = width
        .checked_mul(height)
        .ok_or(MountainTemplateProducerError::TgaPixelOverflow)?;
    let pixel_bytes = usize::from(pixel_depth / 8);
    let mut cursor = 18usize
        .checked_add(id_len)
        .ok_or(MountainTemplateProducerError::TgaPixelOverflow)?;
    if cursor > tga.len() {
        return Err(MountainTemplateProducerError::TruncatedTga);
    }

    let mut file_pixels = Vec::with_capacity(pixel_count);
    if image_type == 2 {
        while file_pixels.len() < pixel_count {
            file_pixels.push(read_tga_pixel(tga, &mut cursor, pixel_bytes)?);
        }
    } else {
        while file_pixels.len() < pixel_count {
            let packet = *tga
                .get(cursor)
                .ok_or(MountainTemplateProducerError::TruncatedTga)?;
            cursor += 1;
            let count = usize::from(packet & 0x7f) + 1;
            if file_pixels.len().saturating_add(count) > pixel_count {
                return Err(MountainTemplateProducerError::RlePixelOverflow);
            }
            if packet & 0x80 != 0 {
                let pixel = read_tga_pixel(tga, &mut cursor, pixel_bytes)?;
                file_pixels.extend(std::iter::repeat_n(pixel, count));
            } else {
                for _ in 0..count {
                    file_pixels.push(read_tga_pixel(tga, &mut cursor, pixel_bytes)?);
                }
            }
        }
    }

    // ImageIO::decode_tga normalizes descriptor bits 4/5 into a top-left, left-to-right
    // surface before MountainRange::init asks the Texture for mip-0 pixels.
    let right_origin = descriptor & 0x10 != 0;
    let top_origin = descriptor & 0x20 != 0;
    let mut red = vec![0; pixel_count];
    let mut alpha = vec![0; pixel_count];
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
            let pixel = file_pixels[file_y * width + file_x];
            red[y * width + x] = pixel.red;
            alpha[y * width + x] = pixel.alpha;
        }
    }
    Ok(MountainImage {
        width,
        height,
        red,
        alpha,
    })
}

fn read_tga_pixel(
    tga: &[u8],
    cursor: &mut usize,
    pixel_bytes: usize,
) -> Result<MountainPixel, MountainTemplateProducerError> {
    let end = cursor
        .checked_add(pixel_bytes)
        .ok_or(MountainTemplateProducerError::TgaPixelOverflow)?;
    let pixel = tga
        .get(*cursor..end)
        .ok_or(MountainTemplateProducerError::TruncatedTga)?;
    *cursor = end;
    // TGA is B,G,R,(A). ImageIO stores R,G,B,A. MountainRange::init reads byte 0
    // for displacement Z and independently tests byte 3 for vertex presence.
    Ok(MountainPixel {
        red: pixel[2],
        alpha: if pixel_bytes == 4 { pixel[3] } else { 0xff },
    })
}

fn derive_from_image(
    image: MountainImage,
) -> Result<MountainTemplateGeometry, MountainTemplateProducerError> {
    derive_geometry(image, 0)
}

fn derive_geometry(
    image: MountainImage,
    height_bits: u32,
) -> Result<MountainTemplateGeometry, MountainTemplateProducerError> {
    let width =
        i32::try_from(image.width).map_err(|_| MountainTemplateProducerError::TgaPixelOverflow)?;
    let height =
        i32::try_from(image.height).map_err(|_| MountainTemplateProducerError::TgaPixelOverflow)?;
    let start_x = (width / 2).rem_euclid(16);
    let start_y = (height / 2).rem_euclid(16);
    let center_x4 = (width / 2 - start_x) / 4;
    let center_y4 = (height / 2 - start_y) / 4;
    let center_x16 = (width / 2 - start_x) / 16;
    let center_y16 = (height / 2 - start_y) / 16;

    // `0x00899c47..0x00899ed9`: y-major then x-major, both stepping by four.
    // Presence is alpha; displacement height is the normalized surface's red byte.
    let mut tcoord_vertices = Vec::new();
    let height3 = f32::from_bits(height_bits) * 3.0f32;
    let mut y = start_y;
    while y < width {
        let mut x = start_x;
        while x < height {
            let index = y as usize * image.width + x as usize;
            if image.alpha[index] != 0 {
                let vertex_x = ((x - width / 2) as f32 * 192.0f32) * 0.25f32;
                let vertex_y = ((y - height / 2) as f32 * 192.0f32) * 0.25f32;
                let vertex_z = (f32::from(image.red[index]) * height3) / 255.0f32;
                tcoord_vertices.push(MountainTcoordVertex {
                    x_bits: vertex_x.to_bits(),
                    y_bits: vertex_y.to_bits(),
                    z_bits: vertex_z.to_bits(),
                });
            }
            x += 4;
        }
        y += 4;
    }

    let mut mount_tiles = Vec::new();
    let mut mount_wcoords = Vec::new();
    let mut seen_wcoords = BTreeSet::new();
    let mut y = start_y;
    while y < height {
        let mut x = start_x;
        while x < width {
            if image.present(x as usize, y as usize)
                && image.present((x + 4) as usize, y as usize)
                && image.present(x as usize, (y + 4) as usize)
                && image.present((x + 4) as usize, (y + 4) as usize)
            {
                mount_tiles.push(GridOffset::new(x / 4 - center_x4, y / 4 - center_y4));
                let world = GridOffset::new(x / 16 - center_x16, y / 16 - center_y16);
                if seen_wcoords.insert((world.x, world.y)) {
                    mount_wcoords.push(world);
                }
            }
            x += 4;
        }
        y += 4;
    }

    let mut solid_mount_wcoords = Vec::new();
    let mut y = start_y;
    while y + 16 < height {
        let mut x = start_x;
        while x + 16 < width {
            let mut occupied = 0u8;
            let mut all = true;
            for dy in [0, 4, 8, 12, 16] {
                for dx in [0, 4, 8, 12, 16] {
                    if image.present((x + dx) as usize, (y + dy) as usize) {
                        occupied += 1;
                    } else {
                        all = false;
                    }
                }
            }
            if all || occupied >= 16 {
                solid_mount_wcoords.push(GridOffset::new(x / 16 - center_x16, y / 16 - center_y16));
            }
            x += 16;
        }
        y += 16;
    }

    Ok(MountainTemplateGeometry {
        runtime: MountainTemplateRuntime {
            mount_tiles,
            mount_wcoords,
            solid_mount_wcoords,
        },
        tcoord_vertices,
    })
}
