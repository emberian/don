//! Source-backed terrain records required by retail gathering.
//!
//! `BuildTypeData::calc_gather` does not derive Woodcutter capacity from a forest count and
//! does not derive Mine capacity from connected components. It reads two installed/runtime
//! sources which must survive map ingestion:
//!
//! * `LandData.make[4]` and `LandData.num_make[4]`, parsed from the ordered `<LANDS>`
//!   section of the installed `rules.xml`; and
//! * the generated `MountainsData::ranges` / `CliffsData::cliff_mining_data` pointer-array
//!   identities, including their separate search coordinates, mining TCoords and mountain
//!   solid-cell arrays.
//!
//! This module retains those sources without manufacturing either from `TData` masks. It
//! also exposes the narrower [`InstalledLandCatalog`]: retail `World::gather_at` mode one
//! does not read the generated object arrays, so a City census need not make a false
//! generation claim merely to consume installed LandData. The full
//! [`MaterializedGatherHost`] implements
//! [`AuthoritativeGatherTerrain`](super::gathering::AuthoritativeGatherTerrain) over the
//! synchronized [`World`](super::map_terrain::World). Missing source records, incoherent
//! provenance, unknown resource tokens and stale world identity all fail closed.

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use super::economy::{RES_FOOD, RES_KNOWLEDGE, RES_METAL, RES_OIL, RES_TIMBER};
use super::gathering::{
    AuthoritativeGatherTerrain, GatherTile, GatherWorldCell, LandGatherData, LandGatherSlot,
    MiningObjectCandidate, MiningObjectKind,
};
use super::map_terrain::{tflag, Coord, World};
use super::movement::vector_dist;

/// SHA-256 of the supported installed `ron-data/rules.xml`.
///
/// As with the graphics materializer, hashing belongs to the installed-data provider; this
/// module compares the producer-supplied identity and validates the independent byte length
/// and XML structure before admitting it.
pub const SUPPORTED_RULES_XML_SHA256: [u8; 32] = [
    0x2c, 0xad, 0x61, 0x56, 0xf2, 0x57, 0xc2, 0xfa, 0xf7, 0x9c, 0x3f, 0xa2, 0xde, 0x29, 0x3a, 0x24,
    0x9f, 0x61, 0xae, 0x24, 0x51, 0x60, 0xb9, 0x2f, 0xb5, 0xa7, 0x6d, 0x0d, 0xbf, 0x3a, 0x99, 0x88,
];
/// Exact byte length of that file.
pub const SUPPORTED_RULES_XML_LEN: usize = 88_632;
/// Ordered LandData rows in the supported file. Their indices are the values returned by
/// `WorldData::get_land(..., 1)`.
pub const SUPPORTED_LAND_NAMES: [&str; 9] = [
    "Land",
    "Sandy",
    "Ocean",
    "Coast",
    "Forest",
    "Mountains",
    "Rocks",
    "Oil",
    "Cliffs",
];

/// Identity tying installed LandData and generated terrain objects to one world.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GatherTerrainSourceStamp {
    pub installed_rules_sha256: [u8; 32],
    pub world_seed: i32,
    /// All object arrays must have been extracted/generated in one completed transaction.
    pub coherent_generation: bool,
}

/// Coordinate domain to which extracted/generated terrain-object records belong.
///
/// This keeps materialization validation independent of a fabricated `World`. The live
/// synchronized WData/TData owner is required later by [`MaterializedGatherHost::new`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GatherTerrainWorldIdentity {
    pub world_xs: i32,
    pub world_ys: i32,
    pub tile_xs: i32,
    pub tile_ys: i32,
    pub world_seed: i32,
}

impl GatherTerrainWorldIdentity {
    pub fn from_world(world: &World) -> Self {
        Self {
            world_xs: world.xs,
            world_ys: world.ys,
            tile_xs: world.tile_xs,
            tile_ys: world.tile_ys,
            world_seed: world.seed,
        }
    }

    fn valid_w(self, wx: i32, wy: i32) -> bool {
        wx >= 0 && wy >= 0 && wx < self.world_xs && wy < self.world_ys
    }

    fn valid_t(self, tx: i32, ty: i32) -> bool {
        tx >= 0 && ty >= 0 && tx < self.tile_xs && ty < self.tile_ys
    }
}

/// One named `LandData` row, kept in installed XML order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaterializedLandData {
    pub name: String,
    pub gather: LandGatherData,
}

/// Installed `LandData[9]` without the unrelated generated Mountain/Cliff arrays.
///
/// `World::gather_at(..., mode=1)` (`0x006B07F0`) consumes only this ordered table,
/// `WorldData::get_land(..., 1)`, and the center tile's `GATHERED` bit. Keeping this
/// narrow owner separate prevents a City terrain census from claiming that the later
/// generated mining-object arrays have been recovered too.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstalledLandCatalog {
    installed_rules_sha256: [u8; 32],
    lands: Vec<MaterializedLandData>,
}

/// Exact read receipt for the mode-one arm of `World::gather_at`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GatherAtModeOneReceipt {
    pub wx: i32,
    pub wy: i32,
    pub center_tx: i32,
    pub center_ty: i32,
    pub land_index: i32,
    pub center_gathered: bool,
    pub slots_visited: u32,
    pub flat_slots_added: u32,
    pub depletable_slots_added: u32,
    pub depletable_slots_skipped: u32,
    pub output: [i32; 6],
}

/// Exact source record for one non-null `MountainsData::ranges[index]` entry.
///
/// `search_wcoords` are the absolute WCoord-like values consumed by
/// `MountainsData::find_nearest` (`0x0089CD30`), kept separately from the absolute TCoords
/// appended by `BuildTypeData::find_gather_tcoords` (`0x0063BDC0`). The binary reads
/// different arrays for those operations; deriving one from the other is unsupported.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaterializedMountainObject {
    pub search_wcoords: Vec<GatherWorldCell>,
    pub mining_tcoords: Vec<GatherTile>,
    pub solid_wcoords: Vec<GatherWorldCell>,
    pub mountain_size: i32,
}

/// Exact source record for one non-null `CliffsData::cliff_mining_data[index]` entry.
/// `CliffMiningData` carries distinct ordered `wcoords` and `tcoords` arrays.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaterializedCliffObject {
    pub search_wcoords: Vec<GatherWorldCell>,
    pub mining_tcoords: Vec<GatherTile>,
}

/// Fail-closed installed/generated terrain errors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GatherTerrainMaterializationError {
    UnsupportedRulesXml,
    WrongRulesXmlLength {
        expected: usize,
        actual: usize,
    },
    IncoherentGeneration,
    Xml(String),
    MissingLands,
    DuplicateLands,
    NestedLand,
    MakeOutsideLand,
    MissingLandName {
        index: usize,
    },
    DuplicateLandName {
        index: usize,
    },
    WrongMakeCount {
        land: String,
        actual: usize,
    },
    MissingMakeAttribute {
        attribute: &'static str,
    },
    DuplicateMakeAttribute {
        attribute: &'static str,
    },
    InvalidMakeAmount(String),
    UnknownMakeResource(String),
    NonZeroNoneAmount(i32),
    WrongLandCount {
        expected: usize,
        actual: usize,
    },
    WrongLandName {
        index: usize,
        expected: &'static str,
        actual: String,
    },
    TooManyMiningObjects {
        kind: MiningObjectKind,
        actual: usize,
    },
    EmptyObjectArray {
        kind: MiningObjectKind,
        index: usize,
        field: &'static str,
    },
    InvalidObjectCoordinate {
        kind: MiningObjectKind,
        index: usize,
        field: &'static str,
        x: i32,
        y: i32,
    },
    InvalidMountainSize {
        index: usize,
        size: i32,
    },
    StaleWorldShape,
    StaleWorldSeed {
        expected: i32,
        actual: i32,
    },
    InvalidLandIndex {
        index: i32,
    },
    WorldCoordinateOutside {
        wx: i32,
        wy: i32,
    },
}

impl InstalledLandCatalog {
    /// Admit the exact shipped `rules.xml` LandData rows, independently of procedural
    /// terrain-object generation. The digest is supplied by the installed-content owner;
    /// byte length, XML structure, row count, order, names and every MAKE token are
    /// validated again here before publication.
    pub fn from_supported_source(
        rules_xml: &[u8],
        installed_rules_sha256: [u8; 32],
    ) -> Result<Self, GatherTerrainMaterializationError> {
        if installed_rules_sha256 != SUPPORTED_RULES_XML_SHA256 {
            return Err(GatherTerrainMaterializationError::UnsupportedRulesXml);
        }
        if rules_xml.len() != SUPPORTED_RULES_XML_LEN {
            return Err(GatherTerrainMaterializationError::WrongRulesXmlLength {
                expected: SUPPORTED_RULES_XML_LEN,
                actual: rules_xml.len(),
            });
        }
        Self::from_admitted_source(rules_xml, installed_rules_sha256)
    }

    #[cfg(test)]
    fn from_fixture(rules_xml: &[u8]) -> Result<Self, GatherTerrainMaterializationError> {
        Self::from_admitted_source(rules_xml, [0; 32])
    }

    fn from_admitted_source(
        rules_xml: &[u8],
        installed_rules_sha256: [u8; 32],
    ) -> Result<Self, GatherTerrainMaterializationError> {
        let lands = parse_lands(rules_xml)?;
        validate_land_rows(&lands)?;
        Ok(Self {
            installed_rules_sha256,
            lands,
        })
    }

    pub fn installed_rules_sha256(&self) -> [u8; 32] {
        self.installed_rules_sha256
    }

    pub fn lands(&self) -> &[MaterializedLandData] {
        &self.lands
    }

    /// Execute the mode-one path of `World::gather_at` at `0x006B07F0`.
    ///
    /// The exact `GoodTypeData::is_flat` body (`0x004780C0`) returns false for
    /// Timber/Metal/Oil (TypeIndexes 1, 4 and 5). Retail adds twice those resources'
    /// `num_make` while the center tile is not `GATHERED`, and skips them once it is.
    /// The other three basic resources always add the ordinary amount. Arithmetic uses
    /// the x86 wrapping behavior.
    pub fn gather_at_mode_one(
        &self,
        world: &World,
        wx: i32,
        wy: i32,
    ) -> Result<GatherAtModeOneReceipt, GatherTerrainMaterializationError> {
        if !world.valid_w(wx, wy) {
            return Err(GatherTerrainMaterializationError::WorldCoordinateOutside { wx, wy });
        }
        let land_index = world.get_land(wx, wy, 1);
        let land = usize::try_from(land_index)
            .ok()
            .and_then(|index| self.lands.get(index))
            .ok_or(GatherTerrainMaterializationError::InvalidLandIndex { index: land_index })?;
        let center_tx = wx.wrapping_mul(4).wrapping_add(2);
        let center_ty = wy.wrapping_mul(4).wrapping_add(2);
        let center_gathered = world.tmask(center_tx, center_ty) & tflag::GATHERED != 0;
        let mut output = [0i32; 6];
        let mut flat_slots_added = 0u32;
        let mut depletable_slots_added = 0u32;
        let mut depletable_slots_skipped = 0u32;

        for slot in land.gather.slots {
            if slot.num_make == 0 || !(0..6).contains(&slot.make) {
                continue;
            }
            let depletable = matches!(slot.make, 1 | 4 | 5);
            let amount = if depletable {
                if center_gathered {
                    depletable_slots_skipped = depletable_slots_skipped.wrapping_add(1);
                    continue;
                }
                depletable_slots_added = depletable_slots_added.wrapping_add(1);
                slot.num_make.wrapping_mul(2)
            } else {
                flat_slots_added = flat_slots_added.wrapping_add(1);
                slot.num_make
            };
            let resource = slot.make as usize;
            output[resource] = output[resource].wrapping_add(amount);
        }

        Ok(GatherAtModeOneReceipt {
            wx,
            wy,
            center_tx,
            center_ty,
            land_index,
            center_gathered,
            slots_visited: 4,
            flat_slots_added,
            depletable_slots_added,
            depletable_slots_skipped,
            output,
        })
    }
}

/// Installed LandData plus generated terrain-object arrays in their retail index order.
///
/// `None` entries deliberately preserve null pointer-array slots. Compacting the vectors
/// would change the signed object id stored in `MiningList::mtn/cliff`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GatherTerrainMaterialization {
    stamp: GatherTerrainSourceStamp,
    world_xs: i32,
    world_ys: i32,
    tile_xs: i32,
    tile_ys: i32,
    lands: Vec<MaterializedLandData>,
    mountains: Vec<Option<MaterializedMountainObject>>,
    cliffs: Vec<Option<MaterializedCliffObject>>,
}

impl GatherTerrainMaterialization {
    /// Parse the supported installed LandData and retain exact generated terrain arrays.
    /// Validation is transactional; no partially admitted catalog is returned.
    pub fn from_supported_sources(
        rules_xml: &[u8],
        stamp: GatherTerrainSourceStamp,
        world: GatherTerrainWorldIdentity,
        mountains: Vec<Option<MaterializedMountainObject>>,
        cliffs: Vec<Option<MaterializedCliffObject>>,
    ) -> Result<Self, GatherTerrainMaterializationError> {
        if stamp.installed_rules_sha256 != SUPPORTED_RULES_XML_SHA256 {
            return Err(GatherTerrainMaterializationError::UnsupportedRulesXml);
        }
        if rules_xml.len() != SUPPORTED_RULES_XML_LEN {
            return Err(GatherTerrainMaterializationError::WrongRulesXmlLength {
                expected: SUPPORTED_RULES_XML_LEN,
                actual: rules_xml.len(),
            });
        }
        Self::from_admitted_sources(rules_xml, stamp, world, mountains, cliffs)
    }

    /// Exercise the XML/materialization contract without presenting synthetic bytes as an
    /// installed retail source. Product callers must use [`Self::from_supported_sources`].
    #[cfg(test)]
    fn from_fixture(
        rules_xml: &[u8],
        stamp: GatherTerrainSourceStamp,
        world: GatherTerrainWorldIdentity,
        mountains: Vec<Option<MaterializedMountainObject>>,
        cliffs: Vec<Option<MaterializedCliffObject>>,
    ) -> Result<Self, GatherTerrainMaterializationError> {
        Self::from_admitted_sources(rules_xml, stamp, world, mountains, cliffs)
    }

    fn from_admitted_sources(
        rules_xml: &[u8],
        stamp: GatherTerrainSourceStamp,
        world: GatherTerrainWorldIdentity,
        mountains: Vec<Option<MaterializedMountainObject>>,
        cliffs: Vec<Option<MaterializedCliffObject>>,
    ) -> Result<Self, GatherTerrainMaterializationError> {
        if !stamp.coherent_generation {
            return Err(GatherTerrainMaterializationError::IncoherentGeneration);
        }
        if stamp.world_seed != world.world_seed {
            return Err(GatherTerrainMaterializationError::StaleWorldSeed {
                expected: stamp.world_seed,
                actual: world.world_seed,
            });
        }

        let lands = parse_lands(rules_xml)?;
        validate_land_rows(&lands)?;
        validate_objects(world, &mountains, &cliffs)?;

        Ok(Self {
            stamp,
            world_xs: world.world_xs,
            world_ys: world.world_ys,
            tile_xs: world.tile_xs,
            tile_ys: world.tile_ys,
            lands,
            mountains,
            cliffs,
        })
    }

    pub fn stamp(&self) -> GatherTerrainSourceStamp {
        self.stamp
    }

    pub fn lands(&self) -> &[MaterializedLandData] {
        &self.lands
    }

    pub fn mountains(&self) -> &[Option<MaterializedMountainObject>] {
        &self.mountains
    }

    pub fn cliffs(&self) -> &[Option<MaterializedCliffObject>] {
        &self.cliffs
    }

    /// Execute the complete read-only `LandData::get_amount(int good, int tindex)` body at
    /// `0x0067E6D0`. Retail linearly scans `make[4]`, returns the paired `num_make` value for
    /// the first match, and otherwise returns zero. The second argument is deliberately
    /// retained by callers for receipt provenance but is not read by the 45-byte callee.
    pub fn land_amount(
        &self,
        world: &World,
        land_index: i32,
        good: i32,
        _tile_linear_index: i32,
    ) -> Result<i32, GatherTerrainMaterializationError> {
        self.validate_world(world)?;
        let index = usize::try_from(land_index)
            .ok()
            .filter(|&index| index < self.lands.len())
            .ok_or(GatherTerrainMaterializationError::InvalidLandIndex { index: land_index })?;
        Ok(self.lands[index]
            .gather
            .slots
            .iter()
            .find(|slot| slot.make == good)
            .map_or(0, |slot| slot.num_make))
    }

    fn validate_world(&self, world: &World) -> Result<(), GatherTerrainMaterializationError> {
        if (world.xs, world.ys, world.tile_xs, world.tile_ys)
            != (self.world_xs, self.world_ys, self.tile_xs, self.tile_ys)
        {
            return Err(GatherTerrainMaterializationError::StaleWorldShape);
        }
        if world.seed != self.stamp.world_seed {
            return Err(GatherTerrainMaterializationError::StaleWorldSeed {
                expected: self.stamp.world_seed,
                actual: world.seed,
            });
        }
        Ok(())
    }
}

#[derive(Default)]
struct LandBuilder {
    name: Option<String>,
    make: Vec<LandGatherSlot>,
}

fn validate_land_rows(
    lands: &[MaterializedLandData],
) -> Result<(), GatherTerrainMaterializationError> {
    if lands.len() != SUPPORTED_LAND_NAMES.len() {
        return Err(GatherTerrainMaterializationError::WrongLandCount {
            expected: SUPPORTED_LAND_NAMES.len(),
            actual: lands.len(),
        });
    }
    for (index, (land, expected)) in lands.iter().zip(SUPPORTED_LAND_NAMES).enumerate() {
        if land.name != expected {
            return Err(GatherTerrainMaterializationError::WrongLandName {
                index,
                expected,
                actual: land.name.clone(),
            });
        }
    }
    Ok(())
}

fn parse_lands(xml: &[u8]) -> Result<Vec<MaterializedLandData>, GatherTerrainMaterializationError> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(true);
    let mut saw_lands = false;
    let mut in_lands = false;
    let mut land = None::<LandBuilder>;
    let mut capture_name = false;
    let mut result = Vec::new();

    loop {
        let event = reader
            .read_event()
            .map_err(|error| GatherTerrainMaterializationError::Xml(error.to_string()))?;
        match event {
            Event::Start(ref element) => match element.name().as_ref() {
                b"LANDS" => {
                    if saw_lands || in_lands {
                        return Err(GatherTerrainMaterializationError::DuplicateLands);
                    }
                    saw_lands = true;
                    in_lands = true;
                }
                b"LAND" if in_lands => {
                    if land.is_some() {
                        return Err(GatherTerrainMaterializationError::NestedLand);
                    }
                    land = Some(LandBuilder::default());
                }
                b"NAME" if land.is_some() => capture_name = true,
                b"MAKE" if in_lands => {
                    let Some(builder) = land.as_mut() else {
                        return Err(GatherTerrainMaterializationError::MakeOutsideLand);
                    };
                    builder.make.push(parse_make(element)?);
                }
                _ => {}
            },
            Event::Empty(ref element) => {
                if element.name().as_ref() == b"MAKE" && in_lands {
                    let Some(builder) = land.as_mut() else {
                        return Err(GatherTerrainMaterializationError::MakeOutsideLand);
                    };
                    builder.make.push(parse_make(element)?);
                }
            }
            Event::Text(ref text) if capture_name => {
                let index = result.len();
                let value = std::str::from_utf8(text.as_ref())
                    .map_err(|error| GatherTerrainMaterializationError::Xml(error.to_string()))?
                    .trim()
                    .to_owned();
                if !value.is_empty() {
                    let builder = land.as_mut().expect("capture_name requires a LAND");
                    if builder.name.replace(value).is_some() {
                        return Err(GatherTerrainMaterializationError::DuplicateLandName { index });
                    }
                }
            }
            Event::End(ref element) => match element.name().as_ref() {
                b"NAME" if capture_name => capture_name = false,
                b"LAND" if in_lands => {
                    let index = result.len();
                    let builder = land
                        .take()
                        .ok_or(GatherTerrainMaterializationError::NestedLand)?;
                    let name = builder
                        .name
                        .ok_or(GatherTerrainMaterializationError::MissingLandName { index })?;
                    if builder.make.len() != 4 {
                        return Err(GatherTerrainMaterializationError::WrongMakeCount {
                            land: name,
                            actual: builder.make.len(),
                        });
                    }
                    result.push(MaterializedLandData {
                        name,
                        gather: LandGatherData {
                            slots: builder.make.try_into().expect("length checked above"),
                        },
                    });
                }
                b"LANDS" if in_lands => in_lands = false,
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }

    if !saw_lands || in_lands || land.is_some() {
        return Err(GatherTerrainMaterializationError::MissingLands);
    }
    Ok(result)
}

fn parse_make(
    element: &BytesStart<'_>,
) -> Result<LandGatherSlot, GatherTerrainMaterializationError> {
    let mut amount = None::<String>;
    let mut resource = None::<String>;
    for attribute in element.attributes() {
        let attribute =
            attribute.map_err(|error| GatherTerrainMaterializationError::Xml(error.to_string()))?;
        let target = match attribute.key.as_ref() {
            b"num" => &mut amount,
            b"type" => &mut resource,
            _ => continue,
        };
        let name = if attribute.key.as_ref() == b"num" {
            "num"
        } else {
            "type"
        };
        if target.is_some() {
            return Err(GatherTerrainMaterializationError::DuplicateMakeAttribute {
                attribute: name,
            });
        }
        *target = Some(
            std::str::from_utf8(attribute.value.as_ref())
                .map_err(|error| GatherTerrainMaterializationError::Xml(error.to_string()))?
                .to_owned(),
        );
    }
    let amount = amount
        .ok_or(GatherTerrainMaterializationError::MissingMakeAttribute { attribute: "num" })?;
    let resource = resource
        .ok_or(GatherTerrainMaterializationError::MissingMakeAttribute { attribute: "type" })?;
    let num_make = amount
        .parse::<i32>()
        .map_err(|_| GatherTerrainMaterializationError::InvalidMakeAmount(amount))?;
    let make = if resource.eq_ignore_ascii_case("none") {
        if num_make != 0 {
            return Err(GatherTerrainMaterializationError::NonZeroNoneAmount(
                num_make,
            ));
        }
        -1
    } else if resource.eq_ignore_ascii_case("Food") {
        RES_FOOD as i32
    } else if resource.eq_ignore_ascii_case("Timber") {
        RES_TIMBER as i32
    } else if resource.eq_ignore_ascii_case("Knowledge") {
        RES_KNOWLEDGE as i32
    } else if resource.eq_ignore_ascii_case("Metal") {
        RES_METAL as i32
    } else if resource.eq_ignore_ascii_case("Oil") {
        RES_OIL as i32
    } else {
        return Err(GatherTerrainMaterializationError::UnknownMakeResource(
            resource,
        ));
    };
    Ok(LandGatherSlot { make, num_make })
}

fn validate_objects(
    world: GatherTerrainWorldIdentity,
    mountains: &[Option<MaterializedMountainObject>],
    cliffs: &[Option<MaterializedCliffObject>],
) -> Result<(), GatherTerrainMaterializationError> {
    if mountains.len() > i8::MAX as usize + 1 {
        return Err(GatherTerrainMaterializationError::TooManyMiningObjects {
            kind: MiningObjectKind::Mountain,
            actual: mountains.len(),
        });
    }
    if cliffs.len() > i8::MAX as usize + 1 {
        return Err(GatherTerrainMaterializationError::TooManyMiningObjects {
            kind: MiningObjectKind::Cliff,
            actual: cliffs.len(),
        });
    }

    for (index, object) in mountains.iter().enumerate() {
        let Some(object) = object else { continue };
        validate_wcoords(
            world,
            MiningObjectKind::Mountain,
            index,
            "search_wcoords",
            &object.search_wcoords,
        )?;
        validate_tcoords(
            world,
            MiningObjectKind::Mountain,
            index,
            "mining_tcoords",
            &object.mining_tcoords,
        )?;
        validate_wcoords(
            world,
            MiningObjectKind::Mountain,
            index,
            "solid_wcoords",
            &object.solid_wcoords,
        )?;
        if object.mountain_size < 0 {
            return Err(GatherTerrainMaterializationError::InvalidMountainSize {
                index,
                size: object.mountain_size,
            });
        }
    }
    for (index, object) in cliffs.iter().enumerate() {
        let Some(object) = object else { continue };
        validate_wcoords(
            world,
            MiningObjectKind::Cliff,
            index,
            "search_wcoords",
            &object.search_wcoords,
        )?;
        validate_tcoords(
            world,
            MiningObjectKind::Cliff,
            index,
            "mining_tcoords",
            &object.mining_tcoords,
        )?;
    }
    Ok(())
}

fn validate_wcoords(
    world: GatherTerrainWorldIdentity,
    kind: MiningObjectKind,
    index: usize,
    field: &'static str,
    coordinates: &[GatherWorldCell],
) -> Result<(), GatherTerrainMaterializationError> {
    if coordinates.is_empty() {
        return Err(GatherTerrainMaterializationError::EmptyObjectArray { kind, index, field });
    }
    for coordinate in coordinates {
        if !world.valid_w(coordinate.wx, coordinate.wy) {
            return Err(GatherTerrainMaterializationError::InvalidObjectCoordinate {
                kind,
                index,
                field,
                x: coordinate.wx,
                y: coordinate.wy,
            });
        }
    }
    Ok(())
}

fn validate_tcoords(
    world: GatherTerrainWorldIdentity,
    kind: MiningObjectKind,
    index: usize,
    field: &'static str,
    coordinates: &[GatherTile],
) -> Result<(), GatherTerrainMaterializationError> {
    if coordinates.is_empty() {
        return Err(GatherTerrainMaterializationError::EmptyObjectArray { kind, index, field });
    }
    for coordinate in coordinates {
        if !world.valid_t(coordinate.tx, coordinate.ty) {
            return Err(GatherTerrainMaterializationError::InvalidObjectCoordinate {
                kind,
                index,
                field,
                x: coordinate.tx,
                y: coordinate.ty,
            });
        }
    }
    Ok(())
}

/// Complete diplomacy source required by territory gates.
pub trait GatherTerrainDiplomacy {
    fn is_allied(&self, site_owner: i32, territory_owner: i32) -> Option<bool>;
}

impl<F> GatherTerrainDiplomacy for F
where
    F: Fn(i32, i32) -> Option<bool>,
{
    fn is_allied(&self, site_owner: i32, territory_owner: i32) -> Option<bool> {
        self(site_owner, territory_owner)
    }
}

/// Borrowed executable host over one synchronized world/materialization pair.
///
/// Terrain edits which rebuild Mountain/Cliff arrays must atomically replace the
/// materialization. Reservation-bit and territory mutations remain live because ordinary
/// tile/WData reads go directly through `world` on every call.
pub struct MaterializedGatherHost<'a, D: ?Sized> {
    world: &'a World,
    materialization: &'a GatherTerrainMaterialization,
    diplomacy: &'a D,
}

impl<'a, D: GatherTerrainDiplomacy + ?Sized> MaterializedGatherHost<'a, D> {
    pub fn new(
        world: &'a World,
        materialization: &'a GatherTerrainMaterialization,
        diplomacy: &'a D,
    ) -> Result<Self, GatherTerrainMaterializationError> {
        materialization.validate_world(world)?;
        Ok(Self {
            world,
            materialization,
            diplomacy,
        })
    }

    fn objects(
        &self,
        kind: MiningObjectKind,
    ) -> Box<dyn Iterator<Item = (usize, &[GatherWorldCell])> + '_> {
        match kind {
            MiningObjectKind::Mountain => Box::new(
                self.materialization
                    .mountains
                    .iter()
                    .enumerate()
                    .filter_map(|(index, object)| {
                        object
                            .as_ref()
                            .map(|object| (index, object.search_wcoords.as_slice()))
                    }),
            ),
            MiningObjectKind::Cliff => {
                Box::new(self.materialization.cliffs.iter().enumerate().filter_map(
                    |(index, object)| {
                        object
                            .as_ref()
                            .map(|object| (index, object.search_wcoords.as_slice()))
                    },
                ))
            }
        }
    }
}

impl<D: GatherTerrainDiplomacy + ?Sized> AuthoritativeGatherTerrain
    for MaterializedGatherHost<'_, D>
{
    fn world_cell_dimensions(&self) -> (i32, i32) {
        (self.world.xs, self.world.ys)
    }

    fn tile_dimensions(&self) -> (i32, i32) {
        (self.world.tile_xs, self.world.tile_ys)
    }

    fn tile_mask(&self, tile: GatherTile) -> Option<u16> {
        self.world
            .valid_t(tile.tx, tile.ty)
            .then(|| self.world.tmask(tile.tx, tile.ty))
    }

    fn territory_owner(&self, wx: i32, wy: i32) -> Option<i32> {
        self.world
            .valid_w(wx, wy)
            .then(|| i32::from(self.world.wdata(wx, wy).who))
    }

    fn world_cell_flags(&self, wx: i32, wy: i32) -> Option<u16> {
        self.world
            .valid_w(wx, wy)
            .then(|| self.world.wdata(wx, wy).flags)
    }

    fn is_allied(&self, site_owner: i32, territory_owner: i32) -> Option<bool> {
        self.diplomacy.is_allied(site_owner, territory_owner)
    }

    fn nearest_mining_object(
        &self,
        kind: MiningObjectKind,
        site_x: Coord,
        site_y: Coord,
        site_region: i16,
    ) -> Option<MiningObjectCandidate> {
        let mut best = None::<MiningObjectCandidate>;
        for (index, points) in self.objects(kind) {
            let first = points[0];
            if site_region >= 0 && self.world.wdata(first.wx, first.wy).region != site_region {
                continue;
            }
            for point in points {
                let point_x = point.wx.wrapping_mul(0x300).wrapping_add(0x180);
                let point_y = point.wy.wrapping_mul(0x300).wrapping_add(0x180);
                let distance = vector_dist(
                    site_x.0.wrapping_sub(point_x),
                    site_y.0.wrapping_sub(point_y),
                );
                if best.is_none_or(|current| distance < current.distance) {
                    best = Some(MiningObjectCandidate {
                        index: index as i32,
                        distance,
                    });
                }
            }
        }
        best
    }

    fn mining_object_tiles(&self, kind: MiningObjectKind, index: i32) -> Option<&[GatherTile]> {
        let index = usize::try_from(index).ok()?;
        match kind {
            MiningObjectKind::Mountain => self
                .materialization
                .mountains
                .get(index)?
                .as_ref()
                .map(|object| object.mining_tcoords.as_slice()),
            MiningObjectKind::Cliff => self
                .materialization
                .cliffs
                .get(index)?
                .as_ref()
                .map(|object| object.mining_tcoords.as_slice()),
        }
    }

    fn mining_object_size(&self, kind: MiningObjectKind, index: i32) -> Option<i32> {
        let index = usize::try_from(index).ok()?;
        match kind {
            MiningObjectKind::Mountain => self
                .materialization
                .mountains
                .get(index)?
                .as_ref()
                .map(|object| object.mountain_size),
            MiningObjectKind::Cliff => self
                .materialization
                .cliffs
                .get(index)?
                .as_ref()
                .map(|object| object.mining_tcoords.len() as i32),
        }
    }

    fn mountain_solid_world_cells(&self, index: i32) -> Option<&[GatherWorldCell]> {
        self.materialization
            .mountains
            .get(usize::try_from(index).ok()?)?
            .as_ref()
            .map(|object| object.solid_wcoords.as_slice())
    }

    fn land_gather_data(&self, wx: i32, wy: i32) -> Option<LandGatherData> {
        if !self.world.valid_w(wx, wy) {
            return None;
        }
        let land_index = usize::try_from(self.world.get_land(wx, wy, 1)).ok()?;
        self.materialization
            .lands
            .get(land_index)
            .map(|land| land.gather)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systems::economy;
    use crate::systems::gathering::{
        woodcutter_gather_capacity, AuthoritativeGatherTerrain, GatherCapacityBonuses,
        GatherCapacityRules, GatherMiningList, WoodGatherCapacityRequest,
    };
    use crate::systems::map_terrain::{tflag, wflag};

    /// Sim-owned parser fixture. The deliberately non-retail amounts prove that this is not a
    /// copy of the installed rules asset. The private fixture constructor bypasses only the
    /// supported file identity/length gates; structural, world and object validation remain on.
    const RULES_XML_FIXTURE: &[u8] = br#"<DON_SIM_TEST_FIXTURE><LANDS>
<LAND><NAME>Land</NAME><MAKE num="11" type="Knowledge"/><MAKE num="12" type="Food"/><MAKE num="0" type="none"/><MAKE num="0" type="none"/></LAND>
<LAND><NAME>Sandy</NAME><MAKE num="21" type="Food"/><MAKE num="0" type="none"/><MAKE num="0" type="none"/><MAKE num="0" type="none"/></LAND>
<LAND><NAME>Ocean</NAME><MAKE num="22" type="Food"/><MAKE num="0" type="none"/><MAKE num="0" type="none"/><MAKE num="0" type="none"/></LAND>
<LAND><NAME>Coast</NAME><MAKE num="23" type="Food"/><MAKE num="0" type="none"/><MAKE num="0" type="none"/><MAKE num="0" type="none"/></LAND>
<LAND><NAME>Forest</NAME><MAKE num="7" type="Timber"/><MAKE num="0" type="none"/><MAKE num="0" type="none"/><MAKE num="0" type="none"/></LAND>
<LAND><NAME>Mountains</NAME><MAKE num="31" type="Metal"/><MAKE num="0" type="none"/><MAKE num="0" type="none"/><MAKE num="0" type="none"/></LAND>
<LAND><NAME>Rocks</NAME><MAKE num="41" type="Knowledge"/><MAKE num="0" type="none"/><MAKE num="0" type="none"/><MAKE num="0" type="none"/></LAND>
<LAND><NAME>Oil</NAME><MAKE num="51" type="Oil"/><MAKE num="0" type="none"/><MAKE num="0" type="none"/><MAKE num="0" type="none"/></LAND>
<LAND><NAME>Cliffs</NAME><MAKE num="61" type="Metal"/><MAKE num="0" type="none"/><MAKE num="0" type="none"/><MAKE num="0" type="none"/></LAND>
</LANDS></DON_SIM_TEST_FIXTURE>"#;

    fn test_world() -> World {
        let mut world = World::init_default_rules(8, 8);
        world.seed = 0x1234_5678;
        for cell in &mut world.wdata {
            cell.land = 0;
            cell.region = 3;
            cell.who = -1;
        }
        world
    }

    fn fixture_stamp(world: &World) -> GatherTerrainSourceStamp {
        GatherTerrainSourceStamp {
            installed_rules_sha256: [0; 32],
            world_seed: world.seed,
            coherent_generation: true,
        }
    }

    fn supported_stamp(world: &World) -> GatherTerrainSourceStamp {
        GatherTerrainSourceStamp {
            installed_rules_sha256: SUPPORTED_RULES_XML_SHA256,
            world_seed: world.seed,
            coherent_generation: true,
        }
    }

    fn mountain(wx: i32, wy: i32) -> MaterializedMountainObject {
        MaterializedMountainObject {
            search_wcoords: vec![GatherWorldCell { wx, wy }],
            mining_tcoords: vec![GatherTile {
                tx: wx * 4 + 2,
                ty: wy * 4 + 2,
            }],
            solid_wcoords: vec![GatherWorldCell { wx, wy }],
            mountain_size: 100,
        }
    }

    fn cliff(wx: i32, wy: i32) -> MaterializedCliffObject {
        MaterializedCliffObject {
            search_wcoords: vec![GatherWorldCell { wx, wy }],
            mining_tcoords: vec![GatherTile {
                tx: wx * 4 + 1,
                ty: wy * 4 + 1,
            }],
        }
    }

    fn materialization(
        world: &World,
        mountains: Vec<Option<MaterializedMountainObject>>,
        cliffs: Vec<Option<MaterializedCliffObject>>,
    ) -> GatherTerrainMaterialization {
        GatherTerrainMaterialization::from_fixture(
            RULES_XML_FIXTURE,
            fixture_stamp(world),
            GatherTerrainWorldIdentity::from_world(world),
            mountains,
            cliffs,
        )
        .unwrap()
    }

    #[test]
    fn sim_owned_fixture_materializes_all_nine_exact_four_slot_lands() {
        let world = test_world();
        let materialization = materialization(&world, Vec::new(), Vec::new());
        assert_eq!(materialization.lands().len(), 9);
        assert_eq!(
            materialization
                .lands()
                .iter()
                .map(|land| land.name.as_str())
                .collect::<Vec<_>>(),
            SUPPORTED_LAND_NAMES
        );
        assert_eq!(
            materialization.lands()[0].gather.slots,
            [
                LandGatherSlot {
                    make: economy::RES_KNOWLEDGE as i32,
                    num_make: 11,
                },
                LandGatherSlot {
                    make: economy::RES_FOOD as i32,
                    num_make: 12,
                },
                LandGatherSlot {
                    make: -1,
                    num_make: 0,
                },
                LandGatherSlot {
                    make: -1,
                    num_make: 0,
                },
            ]
        );
        assert_eq!(
            materialization.lands()[4].gather.slots[0],
            LandGatherSlot {
                make: economy::RES_TIMBER as i32,
                num_make: 7,
            }
        );
        assert_eq!(
            materialization.lands()[5].gather.slots[0].make,
            economy::RES_METAL as i32
        );
        assert_eq!(
            materialization.lands()[8].gather.slots[0].make,
            economy::RES_METAL as i32
        );
    }

    #[test]
    fn land_get_amount_scans_make_slots_in_order_and_ignores_tile_index() {
        let world = test_world();
        let materialization = materialization(&world, Vec::new(), Vec::new());

        assert_eq!(
            materialization
                .land_amount(&world, 0, economy::RES_KNOWLEDGE as i32, 0)
                .unwrap(),
            11
        );
        assert_eq!(
            materialization
                .land_amount(&world, 0, economy::RES_FOOD as i32, i32::MAX)
                .unwrap(),
            12
        );
        assert_eq!(materialization.land_amount(&world, 0, 99, -1).unwrap(), 0);
        assert_eq!(
            materialization.land_amount(&world, 9, 0, 0),
            Err(GatherTerrainMaterializationError::InvalidLandIndex { index: 9 })
        );
    }

    #[test]
    fn local_supported_rules_xml_is_consumed_only_at_runtime() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ron-data/rules.xml");
        let Ok(rules_xml) = std::fs::read(&path) else {
            eprintln!("skipping local retail input: {}", path.display());
            return;
        };
        let world = test_world();
        let materialization = GatherTerrainMaterialization::from_supported_sources(
            &rules_xml,
            supported_stamp(&world),
            GatherTerrainWorldIdentity::from_world(&world),
            Vec::new(),
            Vec::new(),
        )
        .expect("supported local rules.xml");
        assert_eq!(
            materialization.lands()[4].gather.slots[0],
            LandGatherSlot {
                make: economy::RES_TIMBER as i32,
                num_make: 1,
            }
        );
    }

    #[test]
    fn mode_one_gather_uses_only_installed_lands_and_the_center_gathered_bit() {
        let catalog = InstalledLandCatalog::from_fixture(RULES_XML_FIXTURE).unwrap();
        let mut world = test_world();

        let land = catalog.gather_at_mode_one(&world, 3, 4).unwrap();
        assert_eq!(land.land_index, 0);
        assert_eq!((land.center_tx, land.center_ty), (14, 18));
        assert!(!land.center_gathered);
        assert_eq!(land.output, [12, 0, 0, 11, 0, 0]);
        assert_eq!((land.flat_slots_added, land.depletable_slots_added), (2, 0));

        world.wdata_mut(3, 4).flags = wflag::FOREST;
        let forest = catalog.gather_at_mode_one(&world, 3, 4).unwrap();
        assert_eq!(forest.land_index, 4);
        assert_eq!(forest.output, [0, 14, 0, 0, 0, 0]);
        assert_eq!(
            (forest.flat_slots_added, forest.depletable_slots_added),
            (0, 1)
        );

        *world.tmask_mut(forest.center_tx, forest.center_ty) |= tflag::GATHERED;
        let gathered_forest = catalog.gather_at_mode_one(&world, 3, 4).unwrap();
        assert!(gathered_forest.center_gathered);
        assert_eq!(gathered_forest.output, [0; 6]);
        assert_eq!(gathered_forest.depletable_slots_skipped, 1);
    }

    #[test]
    fn mode_one_land_precedence_and_invalid_indices_match_world_get_land() {
        let catalog = InstalledLandCatalog::from_fixture(RULES_XML_FIXTURE).unwrap();
        let mut world = test_world();

        let cell = world.wdata_mut(2, 2);
        cell.land = 99;
        cell.flags = wflag::COAST | wflag::FOREST | wflag::MOUNTAINS | wflag::ROCKS | wflag::OIL;
        assert_eq!(
            catalog.gather_at_mode_one(&world, 2, 2).unwrap().land_index,
            3
        );

        world.wdata_mut(2, 2).flags = wflag::FOREST | wflag::MOUNTAINS | wflag::ROCKS;
        assert_eq!(
            catalog.gather_at_mode_one(&world, 2, 2).unwrap().land_index,
            4
        );
        world.wdata_mut(2, 2).flags = wflag::MOUNTAINS | wflag::ROCKS;
        assert_eq!(
            catalog.gather_at_mode_one(&world, 2, 2).unwrap().land_index,
            5
        );
        world.wdata_mut(2, 2).flags = wflag::ROCKS | wflag::OIL;
        assert_eq!(
            catalog.gather_at_mode_one(&world, 2, 2).unwrap().land_index,
            7
        );

        world.wdata_mut(2, 2).flags = 0;
        assert_eq!(
            catalog.gather_at_mode_one(&world, 2, 2),
            Err(GatherTerrainMaterializationError::InvalidLandIndex { index: 99 })
        );
        assert_eq!(
            catalog.gather_at_mode_one(&world, -1, 2),
            Err(GatherTerrainMaterializationError::WorldCoordinateOutside { wx: -1, wy: 2 })
        );
    }

    #[test]
    fn installed_identity_and_coherent_generation_are_mandatory() {
        let world = test_world();
        assert_eq!(
            InstalledLandCatalog::from_supported_source(RULES_XML_FIXTURE, [0; 32]),
            Err(GatherTerrainMaterializationError::UnsupportedRulesXml)
        );
        assert_eq!(
            InstalledLandCatalog::from_supported_source(
                RULES_XML_FIXTURE,
                SUPPORTED_RULES_XML_SHA256
            ),
            Err(GatherTerrainMaterializationError::WrongRulesXmlLength {
                expected: SUPPORTED_RULES_XML_LEN,
                actual: RULES_XML_FIXTURE.len(),
            })
        );
        let mut wrong = supported_stamp(&world);
        wrong.installed_rules_sha256 = [0; 32];
        assert_eq!(
            GatherTerrainMaterialization::from_supported_sources(
                RULES_XML_FIXTURE,
                wrong,
                GatherTerrainWorldIdentity::from_world(&world),
                Vec::new(),
                Vec::new()
            ),
            Err(GatherTerrainMaterializationError::UnsupportedRulesXml)
        );

        assert_eq!(
            GatherTerrainMaterialization::from_supported_sources(
                RULES_XML_FIXTURE,
                supported_stamp(&world),
                GatherTerrainWorldIdentity::from_world(&world),
                Vec::new(),
                Vec::new()
            ),
            Err(GatherTerrainMaterializationError::WrongRulesXmlLength {
                expected: SUPPORTED_RULES_XML_LEN,
                actual: RULES_XML_FIXTURE.len(),
            })
        );

        let mut incoherent = fixture_stamp(&world);
        incoherent.coherent_generation = false;
        assert_eq!(
            GatherTerrainMaterialization::from_fixture(
                RULES_XML_FIXTURE,
                incoherent,
                GatherTerrainWorldIdentity::from_world(&world),
                Vec::new(),
                Vec::new()
            ),
            Err(GatherTerrainMaterializationError::IncoherentGeneration)
        );
    }

    #[test]
    fn null_pointer_slots_keep_retail_mining_object_indices() {
        let world = test_world();
        let materialization = materialization(
            &world,
            vec![None, Some(mountain(2, 2))],
            vec![None, Some(cliff(5, 5))],
        );
        let diplomacy = |_: i32, _: i32| Some(false);
        let host = MaterializedGatherHost::new(&world, &materialization, &diplomacy).unwrap();
        assert_eq!(
            host.nearest_mining_object(
                MiningObjectKind::Mountain,
                Coord(2 * 0x300 + 0x180),
                Coord(2 * 0x300 + 0x180),
                3,
            ),
            Some(MiningObjectCandidate {
                index: 1,
                distance: 0,
            })
        );
        assert_eq!(
            host.nearest_mining_object(
                MiningObjectKind::Cliff,
                Coord(5 * 0x300 + 0x180),
                Coord(5 * 0x300 + 0x180),
                3,
            )
            .unwrap()
            .index,
            1
        );
    }

    #[test]
    fn nearest_search_is_region_gated_and_strict_ties_keep_first_object() {
        let mut world = test_world();
        world.wdata_mut(1, 1).region = 4;
        let materialization = materialization(
            &world,
            vec![Some(mountain(1, 1)), Some(mountain(3, 1))],
            Vec::new(),
        );
        let diplomacy = |_: i32, _: i32| Some(false);
        let host = MaterializedGatherHost::new(&world, &materialization, &diplomacy).unwrap();
        let midpoint = Coord(2 * 0x300 + 0x180);
        assert_eq!(
            host.nearest_mining_object(MiningObjectKind::Mountain, midpoint, midpoint, -1)
                .unwrap()
                .index,
            0
        );
        assert_eq!(
            host.nearest_mining_object(MiningObjectKind::Mountain, midpoint, midpoint, 3)
                .unwrap()
                .index,
            1
        );
    }

    #[test]
    fn world_land_class_selects_the_source_backed_forest_payload() {
        let mut world = test_world();
        world.wdata_mut(2, 2).flags |= wflag::FOREST;
        *world.tmask_mut(2 * 4 + 2, 2 * 4 + 2) = tflag::GATHER_EDGE;
        let materialization = materialization(&world, Vec::new(), Vec::new());
        let diplomacy = |_: i32, _: i32| Some(false);
        let host = MaterializedGatherHost::new(&world, &materialization, &diplomacy).unwrap();
        let land = host.land_gather_data(2, 2).unwrap();
        assert_eq!(land.slots[0].make, economy::RES_TIMBER as i32);
        assert_eq!(land.slots[0].num_make, 7);
    }

    #[test]
    fn source_backed_host_drives_the_exact_wood_capacity_evaluator() {
        let mut world = test_world();
        let centre = GatherTile { tx: 2, ty: 6 };
        let second_access = GatherTile { tx: 2, ty: 5 };
        world.wdata_mut(0, 1).flags |= wflag::FOREST;
        world.set_gather_edge(centre.tx, centre.ty);
        world.set_gather_edge(second_access.tx, second_access.ty);

        let materialization = materialization(&world, Vec::new(), Vec::new());
        let diplomacy = |_: i32, _: i32| Some(false);
        let host = MaterializedGatherHost::new(&world, &materialization, &diplomacy).unwrap();
        let mut list = GatherMiningList::default();
        list.add(centre);
        list.add(second_access);

        assert_eq!(
            woodcutter_gather_capacity(
                &host,
                &list,
                WoodGatherCapacityRequest {
                    site_tx: 5,
                    site_ty: 5,
                    x_size: 2,
                    y_size: 2,
                    owner: 0,
                    bonuses: GatherCapacityBonuses::default(),
                },
                GatherCapacityRules::shipped(),
            ),
            Ok(4)
        );
    }

    #[test]
    fn stale_world_seed_and_invalid_source_coordinates_fail_closed() {
        let mut world = test_world();
        let materialization = materialization(&world, Vec::new(), Vec::new());
        world.seed ^= 1;
        let diplomacy = |_: i32, _: i32| Some(false);
        assert!(matches!(
            MaterializedGatherHost::new(&world, &materialization, &diplomacy),
            Err(GatherTerrainMaterializationError::StaleWorldSeed { .. })
        ));

        let world = test_world();
        let bad = mountain(world.xs, 0);
        assert!(matches!(
            GatherTerrainMaterialization::from_fixture(
                RULES_XML_FIXTURE,
                fixture_stamp(&world),
                GatherTerrainWorldIdentity::from_world(&world),
                vec![Some(bad)],
                Vec::new(),
            ),
            Err(GatherTerrainMaterializationError::InvalidObjectCoordinate {
                kind: MiningObjectKind::Mountain,
                field: "search_wcoords",
                ..
            })
        ));
    }
}
