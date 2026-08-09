//! The type roster the arena plays with, read from the **live-process type tables**.
//!
//! # Why not the XML
//!
//! [`crate::rules`] already parses `ron-data/*.xml` and is the right source for the
//! *rules constants* (`GATHER_RATE`, `PEASANT_RATE`, `CITY_GATHER`, `POP_CAP`,
//! `COMMERCE_CAP`, the three cost factors). It is the wrong source for **types**, for
//! one reason that matters: the XML carries no `TypeIndex`, and the balance matrix is
//! indexed by `TypeIndex`. `schema/live/live-tables-{unit,building,tech}.tsv` are dumps
//! of the running game's own `UnitType` / `BuildType` / `TechType` arrays — they carry
//! the id **and** every combat field, so a `Hoplites`-vs-`Bowmen` lookup is the id the
//! engine itself would use rather than a name match we invented.
//!
//! Both files are ground truth under `README-LLM.md`'s rule: one is shipped data, the
//! other is a live read. Neither is folklore, and no number in this module is typed in.
//!
//! # What is *not* here
//!
//! Nation (`tribe`) modelling is one integer deep: [`Roster::for_tribe`] keeps, for each
//! display name, the lowest `TypeIndex` whose `TRIBE_MASK` bit is set for that tribe.
//! That reproduces "Romans get Hoplites, Koreans get Hwarang" and nothing else — no
//! nation powers, no unique bonuses. `tribe` is a parameter so the gap is visible.
//!
//! Fidelity: **C**. The values are live reads; their *composition into a game* is ours.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::rules::{Constants, Rules, NRES};

/// Which engine table a row came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Unit,
    Building,
    Tech,
}

/// One row of `UnitType` / `BuildType` / `TechType`, reduced to what the arena reads.
///
/// Field names are the TSV column names, which are the PDB field names.
#[derive(Clone, Debug, Default)]
pub struct TypeRow {
    pub kind_unit: bool,
    pub kind_building: bool,
    pub id: i32,
    pub name: String,
    pub internal: String,
    /// `TypeData::cat`. 5 = civilian, 0 = infantry, 1 = cavalry, 3 = siege, 6 = naval.
    pub cat: i32,
    /// `TRIBE_MASK`, one bit per nation.
    pub tribe_mask: u32,
    /// Already multiplied by the matching `*_COST_FACTOR` from `rules.xml`.
    pub cost: [i32; NRES],
    /// `JOB_TIME`, frames of citizen-work.
    pub job_time: i32,
    /// `PREQ0..2` as `TypeIndex`, `-1` dropped.
    pub preq: Vec<i32>,
    /// `WHERE` — the `TypeIndex` of the building this is produced at, or `-1`.
    pub where_: i32,
    /// `FROM` — the `TypeIndex` this upgrades from, or `-1`.
    pub from: i32,
    pub age: i32,
    // ---- combat ----
    /// `ATTACK`, carried ×10 exactly as `ObjectData::get_attack` returns it.
    pub attack: i32,
    pub armor: i32,
    pub hits: i32,
    /// `RECHARGE`, in frames.
    pub recharge: i32,
    pub min_range: i32,
    pub max_range: i32,
    pub los: i32,
    /// `MOVES`. `rules.xml`: `UNIT_MOVE_SPEED = 1/192 tile`, so this is world units per
    /// frame in the same space as [`don_sim::systems::combat::RANGE_UNITS_PER_TILE`].
    /// **That reading is unverified**; it is the only interpretation consistent with the
    /// shipped comment and with `TARGET_RADIUS`'s 1/2-tile scale.
    pub moves: i32,
    /// `UnitTypeData::turn_speed` at `+0x2C4`, already stored by the live process as a
    /// 32-bit binary angle (`0x20000000` = 45 degrees).  Movement must use this value;
    /// substituting an arena-wide angular rate changes both the path traversed and the
    /// frame on which an order completes.
    pub turn_speed: i32,
    /// Retail's unit-path A* reads `UnitTypeData::new_block_radius` at `+0x248` as the
    /// footprint size.  The live table carries it in 48-world-unit cells.
    pub new_block_radius: i32,
    pub big_radius: i32,
    /// Per-guy formation/collision fields from the live `UnitTypeData`.  They are retained
    /// together because retail stamps collision occupancy for every squad guy, not once
    /// for the parent unit.
    pub guy_spacing: i32,
    pub x_spacing: i32,
    pub y_spacing: i32,
    pub guy_radius: i32,
    pub squad_size: i32,
    pub crew_size: i32,
    pub role: i32,
    pub base_form: i32,
    pub push_size: i32,
    pub push_circles: i32,
    pub unit_flags: u32,
    pub unit_flags2: u32,
    /// `OBJ_MASK`, the `UnitType[+0x1E4]` bit-set `get_damage` reads.
    pub obj_masks: u32,
    /// `ObjectTypeData::fly_high` / `fly_low` at `+0x250/+0x254`. These percentages are
    /// consumed by retail's anti-air dud gate; zero is meaningful and is not defaulted.
    pub fly_high: i32,
    pub fly_low: i32,
    /// `UnitTypeData::mana` at `+0x2EC`; for aircraft this is the sortie fuel budget.
    pub mana: i32,
    pub uber_size: i32,
    pub ammo_per_att: i32,
    pub splash_percent: i32,
    pub domain: i32,
    /// `POP` — the TSV's `control_cost`.
    pub pop: i32,
    pub military_level: i32,
    /// Buildings only: `BUILD_FLAGS`. Bit `0x40` = "resource gatherer".
    pub build_flags: u32,
    pub x_size: i32,
    pub y_size: i32,
}

impl TypeRow {
    pub fn is_gatherer(&self) -> bool {
        self.kind_building && self.build_flags & 0x40 != 0
    }
    /// Anything that can shoot. Cities and Towers can, which is why this is not
    /// "is a unit with `cat != 5`".
    pub fn can_attack(&self) -> bool {
        self.attack > 0
    }
    /// A unit that exists to fight: has an attack and is not a civilian category.
    pub fn is_military(&self) -> bool {
        self.kind_unit && self.attack > 0 && self.cat != 5 && self.cat != 8
    }
    pub fn is_civilian(&self) -> bool {
        self.kind_unit && self.cat == 5
    }

    /// Exact static fields consumed by the recovered retail air primitives. Keeping this
    /// conversion on the live-table row prevents an arena adapter from inventing flight
    /// bands or fuel values when aircraft are eventually admitted to the map.
    pub fn air_type_data(&self) -> don_sim::systems::air::AirTypeData {
        don_sim::systems::air::AirTypeData {
            obj_masks: self.obj_masks,
            domain: self.domain,
            los: self.los,
            fly_high: self.fly_high,
            fly_low: self.fly_low,
            unit_flags: self.unit_flags,
            mana: self.mana,
        }
    }
}

/// Every type the engine knows, plus the rules constants.
#[derive(Clone, Debug)]
pub struct Types {
    pub rows: BTreeMap<i32, TypeRow>,
    pub constants: Constants,
    /// Per-unit `SUPPORT` upkeep, by `TypeIndex`.
    ///
    /// The live TSV's four `support*` columns are an id/amount encoding this lane did not
    /// settle, so upkeep comes from `unitrules.xml`'s `<SUPPORT>` string, which
    /// [`crate::rules::parse_cost`] already reads (`"1f support"` -> one food) and which
    /// is covered by that module's tests. Matched by display name; a unit with no XML
    /// record carries zero upkeep, and that is a *gap*, not a claim.
    pub upkeep: BTreeMap<i32, [i32; NRES]>,
}

/// The subset of [`Types`] one nation can actually build, resolved by display name.
#[derive(Clone, Debug)]
pub struct Roster {
    pub tribe: u8,
    /// display name -> `TypeIndex`
    by_name: BTreeMap<String, i32>,
}

impl Roster {
    pub fn id(&self, name: &str) -> Option<i32> {
        self.by_name.get(name).copied()
    }
    pub fn names(&self) -> impl Iterator<Item = (&str, i32)> {
        self.by_name.iter().map(|(k, v)| (k.as_str(), *v))
    }
}

/// `schema/live`, relative to this crate.
pub fn default_live_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schema/live")
}

fn col(hdr: &[String], name: &str) -> Option<usize> {
    hdr.iter().position(|h| h == name)
}

struct Tsv {
    hdr: Vec<String>,
    rows: Vec<Vec<String>>,
}

fn read_tsv(p: &Path) -> Result<Tsv, String> {
    let text = std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?;
    let mut lines = text.lines();
    let hdr: Vec<String> = lines
        .next()
        .ok_or_else(|| format!("{}: empty", p.display()))?
        .split('\t')
        .map(|s| s.to_string())
        .collect();
    let rows = lines
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.split('\t').map(|s| s.to_string()).collect())
        .collect();
    Ok(Tsv { hdr, rows })
}

/// Reads a named column as `i64`, defaulting to 0 when the column or the cell is absent.
/// Absence is *reported* by [`Types::load`] for the columns it cannot do without.
fn num(hdr: &[String], row: &[String], name: &str) -> i64 {
    match col(hdr, name).and_then(|i| row.get(i)) {
        Some(v) => v.trim().parse::<i64>().unwrap_or(0),
        None => 0,
    }
}

fn text(hdr: &[String], row: &[String], name: &str) -> String {
    col(hdr, name)
        .and_then(|i| row.get(i))
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

fn costs(hdr: &[String], row: &[String], factor: i32) -> [i32; NRES] {
    let mut c = [0i32; NRES];
    for (r, slot) in c.iter_mut().enumerate() {
        *slot = num(hdr, row, &format!("cost{r}")) as i32 * factor;
    }
    c
}

fn preqs(hdr: &[String], row: &[String]) -> Vec<i32> {
    (0..3)
        .map(|i| num(hdr, row, &format!("preq{i}")) as i32)
        .filter(|&v| v >= 0)
        .collect()
}

impl Types {
    /// Load `ron-data/` constants plus the three live type tables.
    ///
    /// Returns `Err` with the offending path rather than a partial roster: an arena with
    /// half a type table is a match whose result means nothing.
    pub fn load(data_dir: &Path, live_dir: &Path) -> Result<Types, String> {
        let rules = Rules::load(data_dir)?;
        let mut t = Types::load_with_constants(rules.constants, live_dir)?;
        let up: Vec<(i32, [i32; NRES])> = t
            .rows
            .values()
            .filter(|r| r.kind_unit)
            .filter_map(|r| rules.units.get(&r.name).map(|x| (r.id, x.support)))
            .filter(|(_, s)| s.iter().any(|&v| v != 0))
            .collect();
        t.upkeep.extend(up);
        Ok(t)
    }

    pub fn load_with_constants(constants: Constants, live_dir: &Path) -> Result<Types, String> {
        let mut rows = BTreeMap::new();
        let unit = read_tsv(&live_dir.join("live-tables-unit.tsv"))?;
        let build = read_tsv(&live_dir.join("live-tables-building.tsv"))?;
        let tech = read_tsv(&live_dir.join("live-tables-tech.tsv"))?;
        for want in [
            "type_id",
            "name_display",
            "attack",
            "hits",
            "moves",
            "turn_speed",
            "new_block_radius",
            "fly_high",
            "fly_low",
            "mana",
            "squad_size",
            "role",
            "base_form",
            "push_size",
            "push_circles",
        ] {
            if col(&unit.hdr, want).is_none() {
                return Err(format!("live-tables-unit.tsv has no `{want}` column"));
            }
        }

        let mut push = |t: &Tsv, kind: Kind, factor: i32| {
            for r in &t.rows {
                let h = &t.hdr;
                let id = num(h, r, "type_id") as i32;
                let row = TypeRow {
                    kind_unit: kind == Kind::Unit,
                    kind_building: kind == Kind::Building,
                    id,
                    name: text(h, r, "name_display"),
                    internal: text(h, r, "name_internal"),
                    cat: num(h, r, "cat") as i32,
                    tribe_mask: num(h, r, "tribe_mask") as u32,
                    cost: costs(h, r, factor),
                    job_time: num(h, r, "job_time") as i32,
                    preq: preqs(h, r),
                    where_: num(h, r, "where") as i32,
                    from: num(h, r, "from") as i32,
                    age: num(h, r, "age") as i32,
                    attack: num(h, r, "attack") as i32,
                    armor: num(h, r, "armor") as i32,
                    hits: num(h, r, "hits") as i32,
                    recharge: num(h, r, "recharge") as i32,
                    min_range: num(h, r, "min_range") as i32,
                    max_range: num(h, r, "max_range") as i32,
                    los: num(h, r, "los") as i32,
                    moves: num(h, r, "moves") as i32,
                    turn_speed: num(h, r, "turn_speed") as i32,
                    new_block_radius: num(h, r, "new_block_radius") as i32,
                    big_radius: num(h, r, "big_radius") as i32,
                    guy_spacing: num(h, r, "guy_spacing") as i32,
                    x_spacing: num(h, r, "x_spacing") as i32,
                    y_spacing: num(h, r, "y_spacing") as i32,
                    guy_radius: num(h, r, "guy_radius") as i32,
                    squad_size: num(h, r, "squad_size") as i32,
                    crew_size: num(h, r, "crew_size") as i32,
                    role: num(h, r, "role") as i32,
                    base_form: num(h, r, "base_form") as i32,
                    push_size: num(h, r, "push_size") as i32,
                    push_circles: num(h, r, "push_circles") as i32,
                    unit_flags: num(h, r, "unit_flags") as u32,
                    unit_flags2: num(h, r, "unit_flags2") as u32,
                    obj_masks: num(h, r, "obj_masks") as u32,
                    fly_high: num(h, r, "fly_high") as i32,
                    fly_low: num(h, r, "fly_low") as i32,
                    mana: num(h, r, "mana") as i32,
                    uber_size: num(h, r, "uber_size").max(1) as i32,
                    ammo_per_att: num(h, r, "ammo_per_att").max(1) as i32,
                    splash_percent: num(h, r, "splash_percent") as i32,
                    domain: num(h, r, "domain") as i32,
                    pop: num(h, r, "control_cost") as i32,
                    military_level: num(h, r, "military_level") as i32,
                    build_flags: num(h, r, "build_flags") as u32,
                    x_size: num(h, r, "x_size").max(1) as i32,
                    y_size: num(h, r, "y_size").max(1) as i32,
                };
                rows.insert(id, row);
            }
        };
        push(&unit, Kind::Unit, constants.unit_cost_factor);
        push(&build, Kind::Building, constants.build_cost_factor);
        push(&tech, Kind::Tech, constants.tech_cost_factor);

        Ok(Types {
            rows,
            constants,
            upkeep: BTreeMap::new(),
        })
    }

    /// Load from the repo's own directories, or `None` when the gitignored game data is
    /// absent. Tests use this and skip; a binary reports the error.
    pub fn load_default() -> Result<Types, String> {
        Types::load(&crate::rules::default_data_dir(), &default_live_dir())
    }

    pub fn get(&self, id: i32) -> Option<&TypeRow> {
        self.rows.get(&id)
    }

    /// The lowest-id type of each display name available to `tribe`.
    ///
    /// Lowest id is the generic variant: the engine lists `CITIZENS` (50) before
    /// `CITIZENSKOREAN` (51), `HOPLITES` before every national reskin.
    pub fn for_tribe(&self, tribe: u8) -> Roster {
        let bit = 1u32 << (tribe as u32 & 31);
        let mut by_name: BTreeMap<String, i32> = BTreeMap::new();
        for (&id, r) in &self.rows {
            if r.name.is_empty() || r.tribe_mask & bit == 0 {
                continue;
            }
            by_name.entry(r.name.clone()).or_insert(id);
        }
        Roster { tribe, by_name }
    }

    /// The age techs, in age order — the ones whose `cat` is the tech category and whose
    /// display name ends in `" Age"`. Holding N of them puts a player in age N.
    pub fn age_techs(&self) -> Vec<i32> {
        let mut v: Vec<(i32, i32)> = self
            .rows
            .values()
            .filter(|r| !r.kind_unit && !r.kind_building && r.name.ends_with(" Age"))
            .map(|r| (r.age, r.id))
            .collect();
        v.sort();
        v.into_iter().map(|(_, id)| id).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn types() -> Option<Types> {
        Types::load_default().ok()
    }

    #[test]
    fn the_live_tables_carry_the_type_ids_the_balance_matrix_is_indexed_by() {
        let Some(t) = types() else { return };
        // These four ids are printed in `schema/live/type-names.txt`; the point of the
        // assertion is that the loader keys on the engine's own id, not on a name match.
        assert_eq!(t.get(50).unwrap().name, "Citizen");
        assert_eq!(t.get(132).unwrap().name, "Hoplites");
        assert_eq!(t.get(170).unwrap().name, "Bowmen");
        assert_eq!(t.get(414).unwrap().name, "Small City");
        assert!(t.get(414).unwrap().kind_building);
        assert!(t.get(170).unwrap().kind_unit);
    }

    #[test]
    fn attack_is_carried_times_ten_as_get_damage_expects() {
        let Some(t) = types() else { return };
        // `unitrules.xml` says `<ATTACK>4</ATTACK>` for a Citizen; the live table says 40.
        assert_eq!(t.get(50).unwrap().attack, 40);
        assert_eq!(t.get(50).unwrap().hits, 40);
    }

    #[test]
    fn movement_shape_and_turn_rate_are_live_fields_not_arena_defaults() {
        let Some(t) = types() else { return };
        let citizen = t.get(50).unwrap();
        assert_eq!(citizen.turn_speed, 0x2000_0000);
        assert_eq!(citizen.new_block_radius, 1);
        assert_eq!(citizen.squad_size, 1);
        assert_eq!(citizen.crew_size, 0);
        assert_eq!(citizen.role, 262_912);
        assert_eq!(citizen.base_form, 0);
        assert_eq!(citizen.push_size, 48);
        assert_eq!(citizen.push_circles, 1);
    }

    #[test]
    fn air_adapter_uses_live_flight_and_fuel_fields() {
        let Some(t) = types() else { return };
        let fighter = t.get(295).unwrap().air_type_data();
        assert_eq!(fighter.domain, don_sim::systems::air::DOMAIN_AIR);
        assert_eq!(fighter.fly_high, 0);
        assert_eq!(fighter.fly_low, 10);
        assert_eq!(fighter.mana, 500);
        assert!(fighter.is_plane());
        assert!(!fighter.is_helicopter());
    }

    #[test]
    fn cost_is_multiplied_by_the_shipped_cost_factor() {
        let Some(t) = types() else { return };
        // Citizen `<COST>2f</COST>` x UNIT_COST_FACTOR 10.
        assert_eq!(t.get(50).unwrap().cost[0], 20);
        assert_eq!(t.constants.unit_cost_factor, 10);
    }

    #[test]
    fn the_tribe_filter_gives_romans_the_ancient_triangle() {
        let Some(t) = types() else { return };
        let r = t.for_tribe(6);
        assert_eq!(r.id("Hoplites"), Some(132));
        assert_eq!(r.id("Bowmen"), Some(170));
        assert_eq!(r.id("Slingers"), Some(82));
        // Nation rosters really are different sets: tribe 16 has the Korean archer line,
        // tribe 6 does not, and neither has the other's Atl-Atls.
        let k = t.for_tribe(16);
        assert_eq!(k.id("Hwarang"), Some(180));
        assert_eq!(r.id("Hwarang"), None);
        assert_eq!(r.id("Atl-Atls"), None);
        assert_eq!(t.for_tribe(0).id("Atl-Atls"), Some(85));
    }

    #[test]
    fn age_techs_are_ordered_and_start_with_classical() {
        let Some(t) = types() else { return };
        let ages = t.age_techs();
        assert_eq!(ages.first().copied(), Some(544));
        assert_eq!(t.get(544).unwrap().name, "Classical Age");
        assert!(ages.len() >= 7, "{} age techs", ages.len());
    }
}
