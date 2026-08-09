//! The derived rules data the simulation runs on, parsed from one binary blob.
//!
//! # Where every number in here comes from
//!
//! Nothing in this file is invented and nothing in it is a derivation either — it is a
//! *reader*. `web/tools/pack-gamedata.mjs` packs three artefacts produced by other lanes:
//!
//! | in the pack | source | what it is |
//! |---|---|---|
//! | 364 unit-type records | `schema/live/live-tables-unit.tsv` | the live `UnitType` list at `0x00C0A264`, ids 50..413 |
//! | 493x493 `i16` | `schema/live/balance-real.bin` | `Balance::final_balance_table` at `0x00C12BF4` |
//! | 16 rules `i32` | `schema/live/rules-block-pid14644.txt` | the live `Constants` singleton at `+0x44..+0xB98` |
//!
//! The rules land in [`don_sim::CombatRules`], whose field comments carry the retail offset
//! each one is read at, so the mapping is checkable field by field.
//!
//! # The fallback, and why it announces itself
//!
//! `schema/live/` is gitignored game content, so a fresh checkout has no pack. Rather than
//! fail to start, [`GameData::synthetic`] builds an obviously fake table — six made-up unit
//! types, a flat balance table, round-number rules — and sets [`GameData::is_real`] to
//! false so the page can say so on screen. A spectator that silently shows plausible
//! numbers from a synthetic table is the exact failure this project is organised against.

use don_sim::CombatRules;

/// `i32` fields per unit record. Must match `UNIT_FIELDS` in the packer.
pub const UNIT_FIELDS: usize = 20;
const MAGIC: &[u8; 8] = b"DONPACK2";

/// One `UnitType`, in the fields this simulation reads.
///
/// Field names are the engine's own column names from the live dump; the comment on each
/// says what is known about its units, and says so when nothing is.
#[derive(Clone, Copy, Debug, Default)]
pub struct UnitTypeRec {
    /// Global type id, 50..413 for units. Also the balance-table row/column.
    pub type_id: i32,
    /// `ATTACK`, stored **x10** — the scale `ObjectData::get_damage` expects.
    pub attack: i32,
    /// `ARMOR`, display scale.
    pub armor: i32,
    /// `HITS`.
    pub hits: i32,
    /// `MOVES`. **Unit not derived.** Used here as subtiles (1/192 tile) per frame, which
    /// is consistent with the rules.xml denominator of 192, and is an assumption.
    pub moves: i32,
    /// `MAX_RANGE`, in tiles. Zero means melee.
    pub max_range: i32,
    /// `MIN_RANGE`, in tiles.
    pub min_range: i32,
    /// `RECHARGE`, in frames between attacks.
    pub recharge: i32,
    /// `TO_HIT`. Not modelled here — no derivation of what it gates.
    pub to_hit: i32,
    /// `DOMAIN`: 0 land, 1 sea, 2 air (inferred, see `don_sim::DamageInput`).
    pub domain: i32,
    /// `MILITARY_LEVEL`, an input to `get_attack` / `get_armor`'s upgrade term.
    pub military_level: i32,
    pub splash_area: i32,
    /// `SPLASH_PERCENT`, read by the damage chain's splash block (never taken here).
    pub splash_percent: i32,
    /// `OBJ_MASK` bit-set, `UnitType[+0x1E4]` — the damage chain reads it in eight places.
    pub obj_masks: i32,
    pub target_size: i32,
    /// `AGE`, 0..7. Used only to compose readable armies.
    pub age: i32,
    pub unit_flags: i32,
    pub los: i32,
    pub role: i32,
    /// Index into the spawn roster, or -1 if this type is not spawned by the spectator.
    pub roster: i32,
}

pub struct GameData {
    pub units: Vec<UnitTypeRec>,
    /// `type_id` -> index into `units`, or `-1`.
    by_id: Vec<i32>,
    /// roster index -> index into `units`.
    pub roster: Vec<u32>,
    pub rules: CombatRules,
    /// `RULES[+0x8B8]`, the upgrade multiplier used by `get_attack` / `get_armor`.
    pub rules_0x8b8: i32,
    balance: Vec<i16>,
    balance_n: i32,
    balance_base: i32,
    /// False when this is the synthetic stand-in rather than the packed game data.
    pub is_real: bool,
}

impl GameData {
    /// Parse the packed blob. Returns `None` on any inconsistency — a truncated or stale
    /// pack must fail loudly, not produce a half-populated table.
    pub fn parse(b: &[u8]) -> Option<GameData> {
        if b.len() < 36 || &b[0..8] != MAGIC {
            return None;
        }
        let u32_at = |o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]) as usize;
        let unit_count = u32_at(8);
        let unit_fields = u32_at(12);
        let rules_count = u32_at(16);
        let balance_n = u32_at(20);
        let balance_base = u32_at(24);
        let roster_count = u32_at(28);
        if unit_fields != UNIT_FIELDS || rules_count < 16 {
            return None;
        }
        let unit_bytes = unit_count * unit_fields * 4;
        let rules_bytes = rules_count * 4;
        let bal_bytes = balance_n * balance_n * 2;
        let head = 36;
        if b.len() < head + unit_bytes + rules_bytes + bal_bytes {
            return None;
        }

        let i32_at = |o: usize| i32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
        let mut units = Vec::with_capacity(unit_count);
        for k in 0..unit_count {
            let o = head + k * unit_fields * 4;
            let f = |j: usize| i32_at(o + j * 4);
            units.push(UnitTypeRec {
                type_id: f(0),
                attack: f(1),
                armor: f(2),
                hits: f(3),
                moves: f(4),
                max_range: f(5),
                min_range: f(6),
                recharge: f(7),
                to_hit: f(8),
                domain: f(9),
                military_level: f(10),
                splash_area: f(11),
                splash_percent: f(12),
                obj_masks: f(13),
                target_size: f(14),
                age: f(15),
                unit_flags: f(16),
                los: f(17),
                role: f(18),
                roster: f(19),
            });
        }

        let ro = head + unit_bytes;
        let r = |j: usize| i32_at(ro + j * 4);
        // Field order is the declaration order of `don_sim::CombatRules`; the packer emits
        // them from the retail offsets named in that struct's own doc comments.
        let rules = CombatRules {
            height_increment: r(0),
            height_bonus: r(1),
            flank_bonus: r(2),
            cavalry_flank_bonus: r(3),
            vehicle_flank_bonus: r(4),
            rocky_modifier: r(5),
            overkill_frames: r(6),
            overkill_damage: r(7),
            entrenchment_modifier: r(8),
            river_modifier: r(9),
            recapture_city_modifier: r(10),
            red_fort_air_defense: r(11),
            rule_0x558: r(12),
            rule_0x76c: r(13),
            rule_0xb98: r(14),
        };
        let rules_0x8b8 = r(15);
        // `height_increment` is a divisor the chain multiplies by 100 and divides by; a
        // zero there is a `#DE` in retail and a trap in wasm. Refuse the pack instead.
        if rules.height_increment == 0 {
            return None;
        }

        let bo = ro + rules_bytes;
        let mut balance = Vec::with_capacity(balance_n * balance_n);
        for k in 0..balance_n * balance_n {
            let o = bo + k * 2;
            balance.push(i16::from_le_bytes([b[o], b[o + 1]]));
        }

        let mut gd = GameData {
            units,
            by_id: Vec::new(),
            roster: Vec::with_capacity(roster_count),
            rules,
            rules_0x8b8,
            balance,
            balance_n: balance_n as i32,
            balance_base: balance_base as i32,
            is_real: true,
        };
        gd.reindex();
        if gd.roster.is_empty() {
            return None;
        }
        Some(gd)
    }

    /// Obviously-fake stand-in for a checkout without the gitignored game data.
    ///
    /// Round numbers on purpose: nobody should be able to mistake a screenshot of this for
    /// a screenshot of the real table.
    pub fn synthetic() -> GameData {
        let mk =
            |type_id, attack, armor, hits, moves, max_range, recharge, age, roster| UnitTypeRec {
                type_id,
                attack,
                armor,
                hits,
                moves,
                max_range,
                min_range: 0,
                recharge,
                to_hit: 0,
                domain: 0,
                military_level: 0,
                splash_area: 0,
                splash_percent: 100,
                obj_masks: 0,
                target_size: 144,
                age,
                unit_flags: 0,
                los: 5,
                role: 0,
                roster,
            };
        let units = vec![
            mk(50, 100, 0, 100, 24, 0, 30, 0, 0),
            mk(51, 200, 2, 200, 20, 0, 40, 1, 1),
            mk(52, 150, 1, 150, 28, 4, 30, 2, 2),
            mk(53, 300, 4, 300, 16, 8, 60, 3, 3),
            mk(54, 250, 3, 250, 32, 2, 30, 4, 4),
            mk(55, 400, 6, 400, 20, 6, 50, 5, 5),
        ];
        let n = 8i32;
        let mut gd = GameData {
            units,
            by_id: Vec::new(),
            roster: Vec::new(),
            // Flat 100 everywhere: no matchup means anything in the synthetic table.
            balance: vec![100; (n * n) as usize],
            balance_n: n,
            balance_base: 50,
            rules: CombatRules {
                height_increment: 100,
                height_bonus: 0,
                flank_bonus: 0,
                cavalry_flank_bonus: 0,
                vehicle_flank_bonus: 0,
                rocky_modifier: 256,
                overkill_frames: 0,
                overkill_damage: 256,
                entrenchment_modifier: 256,
                river_modifier: 256,
                recapture_city_modifier: 256,
                red_fort_air_defense: 0,
                rule_0x558: 0,
                rule_0x76c: 0,
                rule_0xb98: 256,
            },
            rules_0x8b8: 0,
            is_real: false,
        };
        gd.reindex();
        gd
    }

    fn reindex(&mut self) {
        let max_id = self
            .units
            .iter()
            .map(|u| u.type_id)
            .max()
            .unwrap_or(0)
            .max(0) as usize;
        self.by_id = vec![-1; max_id + 1];
        for (k, u) in self.units.iter().enumerate() {
            if u.type_id >= 0 {
                self.by_id[u.type_id as usize] = k as i32;
            }
        }
        let mut roster: Vec<(i32, u32)> = self
            .units
            .iter()
            .enumerate()
            .filter(|(_, u)| u.roster >= 0)
            .map(|(k, u)| (u.roster, k as u32))
            .collect();
        roster.sort_unstable();
        self.roster = roster.into_iter().map(|(_, k)| k).collect();
    }

    #[inline]
    pub fn index_of_type(&self, type_id: i32) -> Option<usize> {
        let k = *self.by_id.get(type_id.max(0) as usize)?;
        if k < 0 {
            None
        } else {
            Some(k as usize)
        }
    }

    /// `Balance::final_balance_table[attacker][defender]`, as a percent.
    ///
    /// The index arithmetic is `don_sim::balance_index`, which is Tier B over the whole
    /// 493x493 domain. The base shift is the one the derivation records: the array at
    /// `0x00C12BF4` has row 0 at type id 50, and the older `0x00C06AFC` figure is that same
    /// array biased back by `2 * (50*493 + 50)` bytes. Out-of-domain pairs return 100, the
    /// table's own neutral value, rather than reading out of bounds.
    #[inline]
    pub fn balance_pct(&self, attacker_type_id: i32, defender_type_id: i32) -> i32 {
        let a = attacker_type_id - self.balance_base;
        let d = defender_type_id - self.balance_base;
        if a < 0 || d < 0 || a >= self.balance_n || d >= self.balance_n {
            return 100;
        }
        let idx = don_sim::balance_index(a, d);
        self.balance.get(idx as usize).copied().unwrap_or(100) as i32
    }

    /// Number of distinct balance values present — a cheap fingerprint of which table is
    /// loaded, reported in the UI so "real data" is a claim the page can back up.
    pub fn balance_distinct(&self) -> u32 {
        let mut seen = [false; 65536];
        let mut n = 0u32;
        for &v in &self.balance {
            let k = v as u16 as usize;
            if !seen[k] {
                seen[k] = true;
                n += 1;
            }
        }
        n
    }
}
