//! The arena map: terrain, start positions, and the spatial rules constants.
//!
//! # What is real and what is ours
//!
//! **Real**: the five placement radii, read out of `ron-data/rules.xml` by tag
//! ([`Spatial::load`]) — `CITY_CENTER_RADIUS`, `CITY_CENTER_POP_RADIUS`,
//! `CITY_CAPTURE_RADIUS`, `WOODCUTTER_RADIUS`, `MINE_RADIUS`. They are the reason a
//! Woodcutter's Camp cannot be dropped anywhere and a second city has to be walked to.
//!
//! **Ours**: the map *generator*. Retail's is `RandomMap::*` and is not derived. This one
//! exists to make placement, scouting and distance real, and it is built to be **exactly
//! fair**: the terrain has 4-fold rotational symmetry about the map centre and the start
//! positions sit on the orbit of one point under that rotation, so with 2 or 4 players
//! every player's neighbourhood is the *same* neighbourhood, rotated. A test asserts it.
//! An unfair map would make every head-to-head number in this lane meaningless.
//! Consequently its forest and mountain circle stamps are not presented as retail
//! `LandData`, `MountainRangeData`, or `CliffMiningData`. [`Map::generate`] marks those
//! gathering sources unavailable. A map ingester which actually has the installed rules
//! bytes and generated object arrays can retain them transactionally through
//! [`Map::retain_gather_terrain_sources`].
//!
//! There is **no water and no naval layer**. `economic.bhs` branches on
//! `get_mapstyle()`; the arena reports a land style so those branches take the land path.

use std::path::Path;

#[path = "map_gather.rs"]
mod map_gather;
pub use map_gather::{
    GatherTerrainRetentionError, GatherTerrainSourceState, GatherTerrainUnavailable,
    RetainedGatherTerrainSources,
};

/// One tile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Terrain {
    Grass,
    Forest,
    Mountain,
}

impl Terrain {
    /// Buildings and units cannot occupy mountain or forest.
    pub fn buildable(self) -> bool {
        self == Terrain::Grass
    }
    pub fn passable(self) -> bool {
        self != Terrain::Mountain
    }
}

/// The placement radii, read from `rules.xml` rather than chosen.
///
/// Each ships as `<TAG value="N tiles"/>`; the leading integer is what
/// `String::fraction`'s `_wtoi` would take, so the parse is the engine's.
#[derive(Clone, Copy, Debug)]
pub struct Spatial {
    /// `CITY_CENTER_RADIUS` — 20 tiles. How far from its city a building may stand.
    pub city_center_radius: i32,
    /// `CITY_CENTER_POP_RADIUS` — 4 tiles.
    pub city_center_pop_radius: i32,
    /// `CITY_CAPTURE_RADIUS` — 10 tiles.
    pub city_capture_radius: i32,
    /// `WOODCUTTER_RADIUS` — 8 tiles.
    pub woodcutter_radius: i32,
    /// `MINE_RADIUS` — 6 tiles.
    pub mine_radius: i32,
}

impl Spatial {
    pub fn load(rules_xml: &Path) -> Result<Spatial, String> {
        let text = std::fs::read_to_string(rules_xml)
            .map_err(|e| format!("{}: {e}", rules_xml.display()))?;
        let pick = |tag: &str| -> Result<i32, String> {
            let needle = format!("<{tag} value=\"");
            let i = text
                .find(&needle)
                .ok_or_else(|| format!("rules.xml has no <{tag}>"))?;
            let rest = &text[i + needle.len()..];
            let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            digits
                .parse()
                .map_err(|_| format!("<{tag}> value is not a leading integer"))
        };
        Ok(Spatial {
            city_center_radius: pick("CITY_CENTER_RADIUS")?,
            city_center_pop_radius: pick("CITY_CENTER_POP_RADIUS")?,
            city_capture_radius: pick("CITY_CAPTURE_RADIUS")?,
            woodcutter_radius: pick("WOODCUTTER_RADIUS")?,
            mine_radius: pick("MINE_RADIUS")?,
        })
    }
}

/// The LCG the engine uses — `s <- s*1664525 + 1013904223`, measured in `Random::get`.
///
/// This is a **separate stream from the simulation's**: it seeds the arena's own map
/// generator, which is not retail's. Sharing the recurrence keeps one integer generator
/// in the project; it does not make this retail's map.
#[derive(Clone, Copy, Debug)]
pub struct Lcg(pub u32);

impl Lcg {
    pub fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
    /// Uniform on `[lo, hi]`, via the high bits (the low bits of an LCG are weak).
    pub fn range(&mut self, lo: i32, hi: i32) -> i32 {
        if hi <= lo {
            return lo;
        }
        let span = (hi - lo + 1) as u32;
        lo + ((self.next() >> 8) % span) as i32
    }
}

/// The map.
#[derive(Clone, Debug)]
pub struct Map {
    pub w: i32,
    pub h: i32,
    /// The arena generator seed.  Map generation has its own stream, while the arena's
    /// retail simulation RNG starts from this same explicit seed and then advances only
    /// at retail-derived call sites.
    pub seed: u32,
    tiles: Vec<Terrain>,
    /// One start tile per player, in player order.
    pub starts: Vec<(i32, i32)>,
    pub spatial: Spatial,
    /// Retail gathering data is a separate source plane. The Arena generator cannot
    /// derive it from its final Terrain bitmap.
    gather_terrain: GatherTerrainSourceState,
}

/// Tuning for [`Map::generate`]. Every field changes the *shape* of a fair map, never
/// one player's share of it.
#[derive(Clone, Copy, Debug)]
pub struct MapParams {
    pub size: i32,
    pub players: usize,
    pub seed: u32,
    /// Distance from map centre to each start, in tiles.
    pub start_radius: i32,
    /// Forest clusters stamped per player quadrant.
    pub forest_clusters: i32,
    /// Mountain clusters stamped per player quadrant.
    pub mountain_clusters: i32,
}

impl Default for MapParams {
    fn default() -> Self {
        MapParams {
            size: 96,
            players: 2,
            seed: 0x5EED_0001,
            start_radius: 30,
            forest_clusters: 7,
            mountain_clusters: 3,
        }
    }
}

impl Map {
    pub fn at(&self, x: i32, y: i32) -> Terrain {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            return Terrain::Mountain; // off-map is impassable, not grass
        }
        self.tiles[(y * self.w + x) as usize]
    }

    fn set(&mut self, x: i32, y: i32, t: Terrain) {
        if x >= 0 && y >= 0 && x < self.w && y < self.h {
            let i = (y * self.w + x) as usize;
            self.tiles[i] = t;
        }
    }

    #[cfg(test)]
    pub(crate) fn test_set(&mut self, x: i32, y: i32, t: Terrain) {
        self.set(x, y, t);
    }

    /// Rotate a point 90° × `k` about the map centre. Exact on an integer grid, which is
    /// why the symmetry claim is a claim and not an approximation.
    fn rot(&self, x: i32, y: i32, k: usize) -> (i32, i32) {
        let (cx, cy) = (self.w - 1, self.h - 1);
        match k & 3 {
            0 => (x, y),
            1 => (cy - y, x),
            2 => (cx - x, cy - y),
            _ => (y, cx - x),
        }
    }

    /// Count tiles of a kind within Chebyshev radius `r`.
    pub fn count_within(&self, x: i32, y: i32, r: i32, t: Terrain) -> i32 {
        let mut n = 0;
        for dy in -r..=r {
            for dx in -r..=r {
                if self.at(x + dx, y + dy) == t {
                    n += 1;
                }
            }
        }
        n
    }

    /// A fair map. See the module docs for what "fair" means here.
    pub fn generate(p: MapParams, spatial: Spatial) -> Map {
        assert!(
            (2..=4).contains(&p.players),
            "the arena's symmetry is 4-fold; 2..=4 players"
        );
        let mut m = Map {
            w: p.size,
            h: p.size,
            seed: p.seed,
            tiles: vec![Terrain::Grass; (p.size * p.size) as usize],
            starts: Vec::new(),
            spatial,
            gather_terrain: GatherTerrainSourceState::Unavailable(
                GatherTerrainUnavailable::NonRetailGenerator,
            ),
        };
        let mut rng = Lcg(p.seed);

        // One quadrant's worth of features, then stamped at all four rotations. Two
        // players use rotations 0 and 2 for their starts but still receive all four
        // stamps of the terrain, so the map is symmetric under 180° as well.
        let cx = (m.w - 1) / 2;
        let cy = (m.h - 1) / 2;
        let sx = cx - p.start_radius;
        let sy = cy;

        let mut blueprint: Vec<(i32, i32, Terrain, i32)> = Vec::new();
        // Forest: the resource a Woodcutter's Camp needs. Placed in a ring around the
        // start so the first camp is a short walk and the later ones are not.
        for i in 0..p.forest_clusters {
            let ring = 6 + (i % 3) * 5;
            let ang = rng.range(0, 359);
            let (dx, dy) = polar(ring, ang);
            blueprint.push((sx + dx, sy + dy, Terrain::Forest, rng.range(2, 3)));
        }
        for i in 0..p.mountain_clusters {
            let ring = 10 + (i % 2) * 7;
            let ang = rng.range(0, 359);
            let (dx, dy) = polar(ring, ang);
            blueprint.push((sx + dx, sy + dy, Terrain::Mountain, rng.range(1, 2)));
        }
        // Neutral middle: contested forest, worth expanding toward.
        for i in 0..3 {
            let ang = rng.range(0, 359);
            let (dx, dy) = polar(6 + i * 3, ang);
            blueprint.push((cx + dx, cy + dy, Terrain::Forest, 2));
        }

        for k in 0..4 {
            for &(bx, by, t, r) in &blueprint {
                let (x, y) = m.rot(bx, by, k);
                for dy in -r..=r {
                    for dx in -r..=r {
                        if dx * dx + dy * dy <= r * r {
                            m.set(x + dx, y + dy, t);
                        }
                    }
                }
            }
        }

        // Starts last, so the town square is guaranteed clear. The square is cleared at
        // **all four** rotations even in a two-player game, or clearing it would itself
        // break the symmetry the fairness claim rests on.
        for k in 0..4 {
            let (x, y) = m.rot(sx, sy, k);
            for dy in -4..=4 {
                for dx in -4..=4 {
                    m.set(x + dx, y + dy, Terrain::Grass);
                }
            }
        }
        for i in 0..p.players {
            let k = if p.players == 2 { i * 2 } else { i };
            m.starts.push(m.rot(sx, sy, k));
        }
        m
    }

    /// Chebyshev distance in tiles — the metric an 8-connected grid actually uses.
    pub fn tile_dist(a: (i32, i32), b: (i32, i32)) -> i32 {
        (a.0 - b.0).abs().max((a.1 - b.1).abs())
    }
}

/// Integer polar offset, degrees. All-integer so map generation is bit-identical on
/// every platform — a float `sin` here would make two machines disagree about the map
/// and therefore about every match result run on them.
pub fn polar(r: i32, deg: i32) -> (i32, i32) {
    let d = deg.rem_euclid(360);
    ((r * cos_deg(d)) / 4096, (r * sin_deg(d)) / 4096)
}

/// `round(sin(d degrees) * 4096)`, quarter table mirrored into the full turn.
fn sin_deg(d: i32) -> i32 {
    const Q: [i32; 91] = [
        0, 71, 143, 214, 286, 357, 428, 499, 570, 641, 711, 782, 852, 921, 991, 1060, 1129, 1198,
        1266, 1334, 1401, 1468, 1534, 1600, 1666, 1731, 1796, 1860, 1923, 1986, 2048, 2110, 2171,
        2231, 2290, 2349, 2408, 2465, 2522, 2578, 2633, 2687, 2741, 2793, 2845, 2896, 2946, 2996,
        3044, 3091, 3138, 3183, 3228, 3271, 3314, 3355, 3396, 3435, 3474, 3511, 3547, 3582, 3617,
        3650, 3681, 3712, 3742, 3770, 3798, 3824, 3849, 3873, 3896, 3917, 3937, 3956, 3974, 3991,
        4006, 4021, 4034, 4046, 4056, 4065, 4074, 4080, 4086, 4090, 4094, 4095, 4096,
    ];
    let d = d.rem_euclid(360);
    match d / 90 {
        0 => Q[d as usize],
        1 => Q[(180 - d) as usize],
        2 => -Q[(d - 180) as usize],
        _ => -Q[(360 - d) as usize],
    }
}

fn cos_deg(d: i32) -> i32 {
    sin_deg(d + 90)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spatial() -> Option<Spatial> {
        Spatial::load(&crate::rules::default_data_dir().join("rules.xml")).ok()
    }

    #[test]
    fn the_radii_come_from_rules_xml() {
        let Some(s) = spatial() else { return };
        assert_eq!(s.city_center_radius, 20);
        assert_eq!(s.city_center_pop_radius, 4);
        assert_eq!(s.city_capture_radius, 10);
        assert_eq!(s.woodcutter_radius, 8);
        assert_eq!(s.mine_radius, 6);
    }

    #[test]
    fn the_map_is_exactly_symmetric_under_a_quarter_turn() {
        let Some(s) = spatial() else { return };
        let m = Map::generate(MapParams::default(), s);
        for y in 0..m.h {
            for x in 0..m.w {
                let (rx, ry) = m.rot(x, y, 1);
                assert_eq!(
                    m.at(x, y),
                    m.at(rx, ry),
                    "({x},{y}) != rot90 ({rx},{ry}) -- the map is not fair"
                );
            }
        }
    }

    #[test]
    fn both_starts_see_the_same_amount_of_forest_and_mountain() {
        let Some(s) = spatial() else { return };
        let m = Map::generate(MapParams::default(), s);
        let f: Vec<i32> = m
            .starts
            .iter()
            .map(|&(x, y)| m.count_within(x, y, 20, Terrain::Forest))
            .collect();
        let mt: Vec<i32> = m
            .starts
            .iter()
            .map(|&(x, y)| m.count_within(x, y, 20, Terrain::Mountain))
            .collect();
        assert_eq!(f[0], f[1], "forest within 20 tiles differs: {f:?}");
        assert_eq!(mt[0], mt[1], "mountain within 20 tiles differs: {mt:?}");
        assert!(f[0] > 0, "a start with no forest cannot build a camp");
    }

    #[test]
    fn generation_is_deterministic() {
        let Some(s) = spatial() else { return };
        let a = Map::generate(MapParams::default(), s);
        let b = Map::generate(MapParams::default(), s);
        assert_eq!(a.tiles, b.tiles);
        assert_eq!(a.starts, b.starts);
    }
}
