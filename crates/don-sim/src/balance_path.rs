//! The **load-time** balance path: how `Balance::final_balance_table` gets its contents.
//!
//! # The caller graph, which is the whole finding
//!
//! `docs/mechanics/COVERAGE.md` §6.7 says "the engine reads it through `type_damage` +
//! `compute_modifier` + `return_pack`". That is right about the *derivation* and wrong
//! about the *tense*. Direct-call callers, counted over `.text` [measured,
//! `tools/pdb/callers.py`]:
//!
//! ```text
//! Balance::type_damage      0x0057FB50   1 caller: Balance::compute_modifier
//! Balance::compute_modifier 0x00581CC0   1 caller: Balance::fill_tables
//! Balance::return_pack      0x005821C0   (only compute_modifier)
//! Balance::fill_tables      0x005823F0   1 caller: Balance::init
//! Balance::return_modifier  0x00581CA0   0 callers -- inlined at 0x00644178..0x0064418E
//! ```
//!
//! So the chain runs **once, at rules-load**, and writes its 243,049 results into
//! `final_balance_table`. Combat then reads that array and nothing else. The two paths
//! cannot disagree *at runtime* because only one of them exists at runtime; what they can
//! disagree about is whether our captured table is the array the producer wrote, and
//! whether we index it the way the consumer does.
//!
//! # The index law, measured four independent ways
//!
//! `balance_row = type_index - 50`, over `type_index` in `50..=542`.
//!
//! 1. `fill_tables`' own loop: `for a in 50..0x21F { for b in 50..0x21F { *p++ =
//!    compute_modifier(a, b, mods) } }`, writing from `0x00C12BF4` while
//!    `p < 0xC896C6`. `0xC896C6 - 0xC12BF4 = 0x76AD2 = 493*493*2`.
//! 2. The bias constant: `0x00C12BF4 - 0x00C06AFC = 49,400 = 2 * (50*493 + 50)`, so
//!    retail's own `(a*493 + b)` arithmetic off the folded base lands on
//!    `(a-50)*493 + (b-50)` off the real one.
//! 3. The live type table (`schema/live/live-tables-typeids.tsv`): ids `0..=49` are
//!    `GoodType`, **`50` is the first `UnitType` (Citizen)**, `542` is the last
//!    `BuildType` (Space Program), `543` is the first `ItemType`.
//! 4. `return_pack` stores `pack.unit = type_index - 0x32`, and `lookup_absolute_name`
//!    reads row `i` out of the type array at `Constants+200 + i*4`.
//!
//! [`crate::mechanics::balance_index`] is *correct* — it is retail's arithmetic relative
//! to the **folded** base `0x00C06AFC`. It is only wrong when paired with an array
//! captured at `0x00C12BF4`. [`crate::balance::BalanceTable::get`] is that pairing, and
//! it is where this module's [`table_index`] belongs.
//!
//! # The 399-row modifier space
//!
//! `compute_modifier` folds a second matrix over the base value. It is `short[399][399]`,
//! `malloc(0x4DBC2) = 399*399*2` in `fill_tables`, every cell defaulting to 100 and
//! overridden by name from `ron-data/balance.xml`. Its rows are an *absolute index*
//! decoded by `Balance::lookup_absolute_name` `0x00582910`:
//!
//! | absolute | count | meaning |
//! |---|---:|---|
//! | `0..352` | 352 | `type_index - 50`, i.e. units `50..=401` |
//! | `352..357` | 5 | build class: SIEGE, FORTS, TOWERS, CITIES, OBSPOST |
//! | `357..359` | 2 | BUILDINGS, UNITS |
//! | `359..367` | 8 | AGE_0 .. AGE_7 |
//! | `367..399` | 32 | the object-mask bits, named `Flag_<c>_OBJMASK_<name>` |
//!
//! Two checks that could have failed and did not [measured]:
//!
//! * The 47 category names in the shipped `balance.xml` are exactly 5 + 2 + 8 + 32 in
//!   that order, and the letters run `A..Z` then `1..6` — which is what
//!   `lookup_name`'s `chr(0x41 + bit)` with its `>'Z' → -0x2A` fixup emits.
//! * The four `BuildType` ids `compute_modifier`'s pack probes test are `0x1BB`=443
//!   **Fort**, `0x19E`=414 **Small City**, `0x209`=521 **Lookout**, `0x1B7`=439
//!   **Tower** — matching FORTS / CITIES / OBSPOST / TOWERS one for one.
//!
//! # What is *not* here
//!
//! **`Balance::type_damage` `0x0057FB50` is not ported.** It is 8,524 bytes, over the
//! bulk-decompiler's 8,192-byte limit (`re/decomp-all/MANIFEST.jsonl` records it as
//! `skipped_large`), and it reads two `Type` objects through virtual dispatch, so calling
//! it in the oracle needs a fabricated type array rather than a fabricated pair of
//! objects. This module takes it as an **input** — [`TypeDamage`] — exactly as the oracle
//! takes the 26 damage predicates as inputs. Every value it produces here is therefore
//! conditional on that input, and no `type_damage` number in this repo is measured.

use std::collections::HashMap;

/// First `TypeIndex` with a balance row: `UnitType` Citizen.
pub const FIRST_TYPE: i32 = 50;
/// Last `TypeIndex` with a balance row: `BuildType` Space Program. Inclusive.
pub const LAST_TYPE: i32 = 542;
/// Both dimensions of `final_balance_table`.
pub const DIM: usize = 493;
/// Both dimensions of the modifier matrix `compute_modifier` folds.
pub const MOD_DIM: usize = 399;

/// `0x160` — first absolute row of the five build-class names.
pub const ABS_CLASS_BASE: usize = 0x160;
/// `0x165` — first absolute row of BUILDINGS / UNITS.
pub const ABS_KIND_BASE: usize = 0x165;
/// `0x167` — first absolute row of AGE_0 .. AGE_7.
pub const ABS_AGE_BASE: usize = 0x167;
/// `0x16F` — first absolute row of the 32 object-mask bits.
pub const ABS_FLAG_BASE: usize = 0x16F;

/// First `TypeIndex` rejected by the combat-validity guard (`0x191 < t`).
pub const ANIMAL_FIRST: i32 = 402;
/// Last `TypeIndex` rejected by the combat-validity guard (`t < 0x19E`). Inclusive.
pub const ANIMAL_LAST: i32 = 413;

/// The neutral modifier, and the value `compute_modifier` returns for a rejected type.
pub const NEUTRAL: i32 = 100;

/// `type_index - 50`, or `None` outside `50..=542`.
///
/// The engine has no bounds check here; we refuse rather than read a neighbour's row,
/// because a silently adjacent balance percentage is a divergence nothing would surface.
#[inline]
pub fn balance_row(type_index: i32) -> Option<usize> {
    if (FIRST_TYPE..=LAST_TYPE).contains(&type_index) {
        Some((type_index - FIRST_TYPE) as usize)
    } else {
        None
    }
}

/// Index into a `493x493` array captured at **`0x00C12BF4`**, from raw `TypeIndex`
/// arguments. This is the accessor `ObjectData::get_damage` effectively performs.
#[inline]
pub fn table_index(attacker_type: i32, defender_type: i32) -> Option<usize> {
    Some(balance_row(attacker_type)? * DIM + balance_row(defender_type)?)
}

/// Retail's literal arithmetic relative to the **folded** base `0x00C06AFC`:
/// `attacker*493 + defender`, no bias, no bounds check. Identical to
/// [`crate::mechanics::balance_index`], restated here so the two bases are named side by
/// side and the pairing rule is impossible to miss:
///
/// * `folded_index` goes with `0x00C06AFC`.
/// * [`table_index`] goes with `0x00C12BF4` — which is what
///   `schema/live/balance-real.bin` holds.
#[inline]
pub fn folded_index(attacker_type: i32, defender_type: i32) -> i32 {
    crate::mechanics::balance_index(attacker_type, defender_type)
}

/// Elements between the two bases: `49,400 = 2 * (50*493 + 50)` bytes / 2.
pub const FOLD_BIAS_ELEMENTS: i32 = 50 * DIM as i32 + 50;

/// The `Type::valid_combat` guard both `compute_modifier` and `return_pack` open with —
/// modelled at its *inlined* form, `!(0x191 < t && t < 0x19E)`.
///
/// The general form is a virtual call on the type's vtable slot `+0x18`; the inline
/// arm is taken when that slot is `0x004707A0`, the base implementation. Types
/// `402..=413` are the animals (`WILDBIRD` .. `HERDPEACOCKS`), so this is "animals do not
/// participate in the balance matrix", and `compute_modifier` returns [`NEUTRAL`] for
/// them rather than 0.
#[inline]
pub fn valid_combat(type_index: i32) -> bool {
    !(ANIMAL_FIRST..=ANIMAL_LAST).contains(&type_index)
}

// ---------------------------------------------------------------------------------------
// BalancePack -- Balance::return_pack 0x005821C0
// ---------------------------------------------------------------------------------------

/// The five dwords `Balance::return_pack` fills, in layout order.
///
/// `-1` means "absent", which is how the engine spells it: every slot is initialised to
/// `-1` (and `flags` to `0`) and `compute_modifier` skips any slot that stayed negative.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BalancePack {
    /// `pack[0]` — `type_index - 50`, units only.
    pub unit: i32,
    /// `pack[1]` — build class, `0..=4`; `0` also used for units whose
    /// `UnitType[+0x2B4] & 0x20000` is set.
    pub class: i32,
    /// `pack[2]` — `1` for a unit, `0` for a building.
    pub kind: i32,
    /// `pack[3]` — the age, from vtable slot `+0xEC` (cached at `+0x278`).
    pub age: i32,
    /// `pack[4]` — the 32-bit object mask at `Type+0x1E4`.
    pub flags: u32,
}

impl Default for BalancePack {
    fn default() -> Self {
        BalancePack {
            unit: -1,
            class: -1,
            kind: -1,
            age: -1,
            flags: 0,
        }
    }
}

impl BalancePack {
    /// The absolute indices this pack contributes, in `compute_modifier`'s own push order:
    /// unit row, then class, then kind, then age, then one row per set flag bit from bit 0
    /// upward.
    ///
    /// Order matters. The fold is integer division at every step, so it is **not**
    /// commutative: `(a*(b*100/100)/100)` and `(b*(a*100/100)/100)` differ whenever a
    /// truncation happens.
    /// A `unit` row at or past [`ABS_CLASS_BASE`] is dropped rather than folded. Retail
    /// cannot produce one from the shipped data — the unit predicate admits `50..=413`
    /// and `valid_combat` has already rejected `402..=413`, leaving rows `0..=351` — but
    /// retail also has no bounds check, so a mod adding a 353rd unit type would have it
    /// read past the matrix. We refuse instead of inventing what that read returns.
    pub fn indices(&self) -> Vec<usize> {
        let mut out = Vec::with_capacity(8);
        if self.unit >= 0 && (self.unit as usize) < ABS_CLASS_BASE {
            out.push(self.unit as usize);
        }
        if self.class >= 0 {
            out.push(ABS_CLASS_BASE + self.class as usize);
        }
        if self.kind >= 0 {
            out.push(ABS_KIND_BASE + self.kind as usize);
        }
        if self.age >= 0 {
            out.push(ABS_AGE_BASE + self.age as usize);
        }
        for bit in 0..32 {
            if self.flags & (1u32 << bit) != 0 {
                out.push(ABS_FLAG_BASE + bit);
            }
        }
        out
    }
}

/// What `compute_modifier` needs to know about the type universe. Implemented by whoever
/// owns the loaded rules; kept as a trait so this module never invents a type table.
pub trait TypeUniverse {
    /// `Balance::return_pack`. Returning `BalancePack::default()` is the honest answer for
    /// a type whose attributes are not loaded — it contributes no rows, which is exactly
    /// what retail does for a type that fails the `+0x24` predicate.
    fn pack(&self, type_index: i32) -> BalancePack;
}

/// `Balance::type_damage` `0x0057FB50`, as an **input**. Nothing in this repository
/// implements it; see the module header.
pub trait TypeDamage {
    fn type_damage(&self, attacker_type: i32, defender_type: i32) -> i32;
}

impl<F: Fn(i32, i32) -> i32> TypeDamage for F {
    fn type_damage(&self, a: i32, d: i32) -> i32 {
        self(a, d)
    }
}

// ---------------------------------------------------------------------------------------
// The 399x399 modifier matrix
// ---------------------------------------------------------------------------------------

/// `short[399][399]`, `malloc(0x4DBC2)` in `fill_tables`, every cell 100 until
/// `balance.xml` says otherwise. Row is the **attacker**'s absolute index, column the
/// defender's.
#[derive(Clone)]
pub struct ModifierMatrix {
    data: Vec<i16>,
}

impl Default for ModifierMatrix {
    fn default() -> Self {
        ModifierMatrix::neutral()
    }
}

impl ModifierMatrix {
    /// Every cell 100 — the state `fill_tables` leaves when the XML is absent, and the
    /// state in which `final_balance_table == type_damage` exactly.
    pub fn neutral() -> Self {
        ModifierMatrix {
            data: vec![NEUTRAL as i16; MOD_DIM * MOD_DIM],
        }
    }

    #[inline]
    pub fn get(&self, row: usize, col: usize) -> i16 {
        self.data[row * MOD_DIM + col]
    }

    #[inline]
    pub fn set(&mut self, row: usize, col: usize, v: i16) {
        self.data[row * MOD_DIM + col] = v;
    }

    pub fn raw(&self) -> &[i16] {
        &self.data
    }

    /// How many cells differ from 100. `0` means the fold is the identity everywhere and
    /// `final_balance_table` is `type_damage` verbatim; the shipped file is **not** that.
    pub fn modified_cells(&self) -> usize {
        self.data.iter().filter(|&&v| v != NEUTRAL as i16).count()
    }

    /// Read `ron-data/balance.xml` into the matrix.
    ///
    /// `fill_tables` looks the row element up **by name** and then each column **by
    /// attribute name**, defaulting to 100, so document order is irrelevant and a name the
    /// engine cannot resolve simply never lands. Returns the matrix plus the names that
    /// did not resolve, because silently dropping a row would turn a naming bug into a
    /// balance change nobody could see.
    pub fn from_xml(xml: &str, names: &AbsoluteNames) -> (ModifierMatrix, Vec<String>) {
        let mut m = ModifierMatrix::neutral();
        let mut unresolved = Vec::new();
        for entry in entry_attributes(xml) {
            let Some(row_name) = entry.iter().find(|(k, _)| k == "name").map(|(_, v)| v) else {
                continue;
            };
            let Some(row) = names.index_of(row_name) else {
                unresolved.push(row_name.clone());
                continue;
            };
            for (k, v) in &entry {
                if k == "name" {
                    continue;
                }
                let Some(col) = names.index_of(k) else {
                    unresolved.push(k.clone());
                    continue;
                };
                // The engine's attribute reader is `XMLElement::attr_int(name, 100)`;
                // an unparseable value takes the default rather than erroring.
                let val: i32 = v.parse().unwrap_or(NEUTRAL);
                m.set(row, col, val as i16);
            }
        }
        unresolved.sort();
        unresolved.dedup();
        (m, unresolved)
    }
}

/// Flat `<ENTRY .../>` attribute scan. Deliberately narrow: `balance.xml` is a
/// machine-emitted table of `<ENTRY name="..." Other="123" .../>` with no nesting, no
/// entities and no CDATA, and a general XML parser here would be a dependency bought to
/// read one shape.
fn entry_attributes(xml: &str) -> Vec<Vec<(String, String)>> {
    let mut out = Vec::new();
    let bytes = xml.as_bytes();
    let mut i = 0usize;
    while let Some(p) = xml[i..].find("<ENTRY") {
        let start = i + p + "<ENTRY".len();
        let Some(e) = xml[start..].find('>') else {
            break;
        };
        let end = start + e;
        let mut attrs = Vec::new();
        let mut j = start;
        while j < end {
            // key
            while j < end && (bytes[j] as char).is_whitespace() {
                j += 1;
            }
            let ks = j;
            while j < end && bytes[j] != b'=' && !(bytes[j] as char).is_whitespace() {
                j += 1;
            }
            if ks == j {
                break;
            }
            let key = xml[ks..j].to_string();
            while j < end && (bytes[j] as char).is_whitespace() {
                j += 1;
            }
            if j >= end || bytes[j] != b'=' {
                continue;
            }
            j += 1;
            while j < end && (bytes[j] as char).is_whitespace() {
                j += 1;
            }
            if j >= end || bytes[j] != b'"' {
                continue;
            }
            j += 1;
            let vs = j;
            while j < end && bytes[j] != b'"' {
                j += 1;
            }
            attrs.push((key, xml[vs..j].to_string()));
            j += 1;
        }
        out.push(attrs);
        i = end + 1;
    }
    out
}

// ---------------------------------------------------------------------------------------
// The absolute-index name space -- Balance::lookup_absolute_name 0x00582910
// ---------------------------------------------------------------------------------------

/// The 47 category names, absolute `352..399`, in `lookup_absolute_name` order.
///
/// The first 15 come from three `String` arrays in `.data`
/// (`0x00E37B10`, `0x00E37B74`, `0x00E37BA0`, stride `0x14`) which are BSS-initialised
/// at runtime and therefore not readable from the file image; they are taken from the
/// shipped `balance.xml`, whose 47-name tail matches this layout exactly. The 32 flag
/// names are the composition `"Flag" + "_" + chr(0x41+bit) + "_" + objmask_name[bit]`
/// that `lookup_name` builds, with the `>'Z' → -0x2A` fixup that turns bit 26 into `'1'`.
pub const CATEGORY_NAMES: [&str; 47] = [
    "SIEGE",
    "FORTS",
    "TOWERS",
    "CITIES",
    "OBSPOST",
    "BUILDINGS",
    "UNITS",
    "AGE_0",
    "AGE_1",
    "AGE_2",
    "AGE_3",
    "AGE_4",
    "AGE_5",
    "AGE_6",
    "AGE_7",
    "Flag_A_OBJMASK_ARMORED",
    "Flag_B_OBJMASK_BOMBARD",
    "Flag_C_OBJMASK_CIVILIAN",
    "Flag_D_OBJMASK_MUSKET_INF",
    "Flag_E_OBJMASK_ELEPHANT",
    "Flag_F_OBJMASK_FOOT",
    "Flag_G_OBJMASK_GUN",
    "Flag_H_OBJMASK_HEAVY_INF",
    "Flag_I_OBJMASK_MODERN_INF",
    "Flag_J_OBJMASK_CARRY_AIR",
    "Flag_K_OBJMASK_FOOT_ARCHER",
    "Flag_L_OBJMASK_LARGE",
    "Flag_M_OBJMASK_MOUNTED",
    "Flag_N_OBJMASK_NAVAL",
    "Flag_O_OBJMASK_HORSE_ARCHER",
    "Flag_P_OBJMASK_SPARSE",
    "Flag_Q_OBJMASK_LIGHT_INF",
    "Flag_R_OBJMASK_ARCHERY",
    "Flag_S_OBJMASK_SIEGE",
    "Flag_T_OBJMASK_WAR_MACHINE",
    "Flag_U_OBJMASK_ARMORPIERCE",
    "Flag_V_OBJMASK_VEHICLE",
    "Flag_W_OBJMASK_MELEE",
    "Flag_X_OBJMASK_EXPLOSIVE",
    "Flag_Y_OBJMASK_HEAVY_CAV",
    "Flag_Z_OBJMASK_DETECT",
    "Flag_1_OBJMASK_UNUSED",
    "Flag_2_OBJMASK_MISSILE",
    "Flag_3_OBJMASK_AIR",
    "Flag_4_OBJMASK_LIGHT_CAV",
    "Flag_5_OBJMASK_PIKE",
    "Flag_6_OBJMASK_ANTI_AIR",
];

/// `String::replace(' ', '_')` at `0x00A1D0A0(0x20, 0x5F)`, plus the apostrophe strip at
/// `0x00A16F00(0x27)` that `lookup_name` applies to type names only.
///
/// This is why the shipped file spells Caesar's Legions `Caesars_Legions`.
pub fn normalize_type_name(display: &str) -> String {
    display.replace('\'', "").replace(' ', "_")
}

/// Absolute index `0..399` → the name `balance.xml` is keyed by.
pub struct AbsoluteNames {
    names: Vec<Option<String>>,
    lookup: HashMap<String, usize>,
}

impl AbsoluteNames {
    /// Build from `(type_index, display_name)` pairs. Types outside `50..=401` are
    /// ignored: rows past `0x160` are the category space, and the animals `402..=413`
    /// never reach the matrix because `valid_combat` rejects them first.
    ///
    /// When two types share a display name the **lower** index wins, matching a
    /// forward scan of the engine's type array. 33 such collisions exist in the shipped
    /// data (tribe variants: two Citizens, two Scholars, four Generals ...), and they are
    /// why `balance.xml` has 240 unit rows rather than 352.
    pub fn from_type_names<'a, I>(types: I) -> AbsoluteNames
    where
        I: IntoIterator<Item = (i32, &'a str)>,
    {
        let mut names = vec![None; MOD_DIM];
        for (ti, display) in types {
            if !(FIRST_TYPE..ANIMAL_FIRST).contains(&ti) {
                continue;
            }
            let row = (ti - FIRST_TYPE) as usize;
            if row < ABS_CLASS_BASE {
                names[row] = Some(normalize_type_name(display));
            }
        }
        for (k, name) in CATEGORY_NAMES.iter().enumerate() {
            names[ABS_CLASS_BASE + k] = Some((*name).to_string());
        }
        let mut lookup = HashMap::new();
        for (i, n) in names.iter().enumerate() {
            if let Some(n) = n {
                lookup.entry(n.clone()).or_insert(i);
            }
        }
        AbsoluteNames { names, lookup }
    }

    /// The category rows only. Enough to parse the 47 category rows of `balance.xml`
    /// without the live type table, which is gitignored game content.
    pub fn categories_only() -> AbsoluteNames {
        AbsoluteNames::from_type_names(std::iter::empty::<(i32, &str)>())
    }

    pub fn name(&self, absolute: usize) -> Option<&str> {
        self.names.get(absolute)?.as_deref()
    }

    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.lookup.get(name).copied()
    }

    /// How many of the 399 rows have a name. 399 only if the type table names every one
    /// of `50..=401` distinctly, which the shipped data does not.
    pub fn named(&self) -> usize {
        self.names.iter().filter(|n| n.is_some()).count()
    }
}

/// Parse `schema/live/live-tables-typeids.tsv` — the live-process type dump — into
/// `(type_index, display_name)` pairs. Gitignored game content, so every caller must cope
/// with the file being absent.
pub fn parse_live_type_table(tsv: &str) -> Vec<(i32, String)> {
    let mut out = Vec::new();
    for line in tsv.lines().skip(1) {
        let mut f = line.split('\t');
        let (Some(id), Some(_class), Some(display)) = (f.next(), f.next(), f.next()) else {
            continue;
        };
        if let Ok(id) = id.trim().parse::<i32>() {
            out.push((id, display.to_string()));
        }
    }
    out
}

// ---------------------------------------------------------------------------------------
// compute_modifier / fill_tables
// ---------------------------------------------------------------------------------------

/// The inner fold of `Balance::compute_modifier`, `0x00581E9A..0x00581EE0`.
///
/// ```text
/// acc = 100
/// for row in attacker_rows:              // outer
///     for col in defender_cols:          // inner
///         acc = (mods[row*399 + col] * acc) / 100
/// ```
///
/// The multiply is a 32-bit `imul` and the divide is `cdq; idiv 100`, so this **wraps** on
/// overflow and **truncates toward zero**, not toward negative infinity. Both matter:
/// the shipped matrix contains values up to 700, and a chain of them on a large base can
/// exceed `i32`.
pub fn modifier_fold(
    attacker_rows: &[usize],
    defender_cols: &[usize],
    mods: &ModifierMatrix,
) -> i32 {
    let mut acc: i32 = NEUTRAL;
    for &row in attacker_rows {
        for &col in defender_cols {
            acc = (mods.get(row, col) as i32).wrapping_mul(acc) / 100;
        }
    }
    acc
}

/// `Balance::compute_modifier` `0x00581CC0`, whole.
///
/// The early-out returns [`NEUTRAL`], not zero: a type that fails `valid_combat` gets a
/// flat 100 % rather than being excluded from combat.
pub fn compute_modifier<U: TypeUniverse, D: TypeDamage>(
    attacker_type: i32,
    defender_type: i32,
    types: &U,
    mods: &ModifierMatrix,
    base: &D,
) -> i32 {
    if !valid_combat(attacker_type) || !valid_combat(defender_type) {
        return NEUTRAL;
    }
    let rows = types.pack(attacker_type).indices();
    let cols = types.pack(defender_type).indices();
    let m = modifier_fold(&rows, &cols, mods);
    base.type_damage(attacker_type, defender_type)
        .wrapping_mul(m)
        / 100
}

/// `Balance::fill_tables`' terminal loop `0x00582368..0x00582392`, which is the only thing
/// that ever writes `final_balance_table`.
///
/// Note the store width: `compute_modifier` returns `int` and the loop stores a `short`.
/// The truncation is real, it is retail's, and it is why this returns `Vec<i16>`.
pub fn fill_final_table<U: TypeUniverse, D: TypeDamage>(
    types: &U,
    mods: &ModifierMatrix,
    base: &D,
) -> Vec<i16> {
    let mut out = Vec::with_capacity(DIM * DIM);
    for a in FIRST_TYPE..=LAST_TYPE {
        for d in FIRST_TYPE..=LAST_TYPE {
            out.push(compute_modifier(a, d, types, mods, base) as i16);
        }
    }
    out
}

/// A [`TypeUniverse`] in which types `50..=401` are bare units with no class, age or
/// flags, and everything else contributes nothing — the pack `return_pack` produces when
/// the type's `+0x24` predicate fails.
///
/// Useful for pinning the fold's *shape* without a loaded type table. It is not the
/// shipped universe and must never be used to produce a balance number anyone believes.
pub struct BareUnits;

impl TypeUniverse for BareUnits {
    fn pack(&self, type_index: i32) -> BalancePack {
        let unit = match balance_row(type_index) {
            Some(r) if r < ABS_CLASS_BASE => r as i32,
            _ => -1,
        };
        BalancePack {
            unit,
            ..BalancePack::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn repo(rel: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(rel)
    }

    #[test]
    fn the_index_law_is_minus_fifty() {
        assert_eq!(balance_row(50), Some(0));
        assert_eq!(balance_row(542), Some(DIM - 1));
        assert_eq!(balance_row(49), None);
        assert_eq!(balance_row(543), None);
        assert_eq!(table_index(50, 50), Some(0));
        assert_eq!(table_index(542, 542), Some(DIM * DIM - 1));
    }

    /// The two bases differ by exactly the bias `fill_tables`' loop implies, so retail's
    /// own `a*493+b` off the folded base and our `(a-50)*493+(b-50)` off the real one are
    /// the same cell. If this ever fails, one of the two constants moved.
    #[test]
    fn folded_and_real_bases_agree_after_the_bias() {
        assert_eq!(FOLD_BIAS_ELEMENTS, (0x00C1_2BF4 - 0x00C0_6AFC) / 2);
        for (a, d) in [(50, 50), (50, 542), (542, 50), (542, 542), (293, 414)] {
            assert_eq!(
                folded_index(a, d) - FOLD_BIAS_ELEMENTS,
                table_index(a, d).unwrap() as i32
            );
        }
    }

    /// The exact defect this lane found: `mechanics::balance_index` against a table
    /// captured at `0x00C12BF4` is off by 100 rows.
    #[test]
    fn the_folded_index_is_wrong_for_a_table_captured_at_the_real_base() {
        assert_ne!(folded_index(50, 51), table_index(50, 51).unwrap() as i32);
        assert_eq!(
            folded_index(50, 51) - table_index(50, 51).unwrap() as i32,
            24_700
        );
    }

    #[test]
    fn animals_are_the_rejected_window() {
        assert!(valid_combat(401));
        assert!(!valid_combat(402));
        assert!(!valid_combat(413));
        assert!(valid_combat(414));
    }

    #[test]
    fn pack_indices_follow_the_engines_push_order() {
        let p = BalancePack {
            unit: 7,
            class: 2,
            kind: 1,
            age: 3,
            flags: (1 << 0) | (1 << 31),
        };
        assert_eq!(
            p.indices(),
            vec![
                7,
                ABS_CLASS_BASE + 2,
                ABS_KIND_BASE + 1,
                ABS_AGE_BASE + 3,
                ABS_FLAG_BASE,
                ABS_FLAG_BASE + 31
            ]
        );
        assert_eq!(BalancePack::default().indices(), Vec::<usize>::new());
    }

    #[test]
    fn the_category_block_ends_exactly_at_399() {
        assert_eq!(ABS_CLASS_BASE + CATEGORY_NAMES.len(), MOD_DIM);
        assert_eq!(ABS_KIND_BASE - ABS_CLASS_BASE, 5);
        assert_eq!(ABS_AGE_BASE - ABS_KIND_BASE, 2);
        assert_eq!(ABS_FLAG_BASE - ABS_AGE_BASE, 8);
        assert_eq!(MOD_DIM - ABS_FLAG_BASE, 32);
    }

    #[test]
    fn a_neutral_matrix_makes_compute_modifier_the_identity() {
        let mods = ModifierMatrix::neutral();
        let base = |_a: i32, _d: i32| 137;
        assert_eq!(compute_modifier(50, 51, &BareUnits, &mods, &base), 137);
        // ... and the rejected window still short-circuits to 100.
        assert_eq!(compute_modifier(402, 51, &BareUnits, &mods, &base), NEUTRAL);
        assert_eq!(compute_modifier(51, 402, &BareUnits, &mods, &base), NEUTRAL);
    }

    /// The fold truncates at every step, so it does not commute and it is not the same as
    /// multiplying the modifiers first. Pinning it because "collect the product then
    /// divide once" is the obvious and wrong way to write this.
    #[test]
    fn the_fold_truncates_per_step_and_is_order_sensitive() {
        let mut m = ModifierMatrix::neutral();
        m.set(0, 0, 33);
        m.set(0, 1, 66);
        m.set(1, 0, 66);
        m.set(1, 1, 33);
        assert_eq!(modifier_fold(&[0], &[0, 1], &m), 21);
        // 33 then 33 then 150 diverges from "multiply all three, divide once".
        let mut m3 = ModifierMatrix::neutral();
        m3.set(0, 0, 33);
        m3.set(0, 1, 33);
        m3.set(0, 2, 150);
        assert_eq!(modifier_fold(&[0], &[0, 1, 2], &m3), 15);
        assert_eq!(
            33i32 * 33 * 150 / (100 * 100),
            16,
            "the single-divide answer"
        );
        // And the fold is not commutative in the column order.
        assert_ne!(
            modifier_fold(&[0], &[2, 0, 1], &m3),
            modifier_fold(&[0], &[0, 1, 2], &m3)
        );
    }

    #[test]
    fn fill_final_table_has_the_shape_of_the_capture() {
        let t = fill_final_table(
            &BareUnits,
            &ModifierMatrix::neutral(),
            &|_a: i32, _d: i32| 100,
        );
        assert_eq!(t.len(), DIM * DIM);
        assert!(t.iter().all(|&v| v == 100));
    }

    /// The store is 16-bit; a `compute_modifier` above 32,767 wraps in retail too.
    #[test]
    fn the_store_is_sixteen_bits_wide() {
        let t = fill_final_table(
            &BareUnits,
            &ModifierMatrix::neutral(),
            &|_a: i32, _d: i32| 40_000,
        );
        assert_eq!(t[0], 40_000u32 as i16);
        assert_eq!(t[0], -25_536);
    }

    // -- gates that need the shipped/captured game content -------------------------------

    #[test]
    fn the_shipped_modifier_matrix_is_not_the_identity() {
        let p = repo("ron-data/balance.xml");
        if !p.exists() {
            eprintln!("skipping: {} not present", p.display());
            return;
        }
        let xml = std::fs::read_to_string(&p).unwrap();
        let names = AbsoluteNames::categories_only();
        let (m, _unresolved) = ModifierMatrix::from_xml(&xml, &names);
        // Category rows alone already carry modifiers, so nobody can claim the fold is a
        // no-op without the live type table.
        assert!(
            m.modified_cells() > 0,
            "compute_modifier's fold is load-bearing; if this is 0 the XML did not parse"
        );
    }

    /// The 47 category names in the shipped file appear in exactly the order
    /// `lookup_absolute_name` emits them. Independent of the live type table.
    #[test]
    fn the_shipped_category_tail_matches_the_decoded_layout() {
        let p = repo("ron-data/balance.xml");
        if !p.exists() {
            eprintln!("skipping: {} not present", p.display());
            return;
        }
        let xml = std::fs::read_to_string(&p).unwrap();
        let rows: Vec<String> = entry_attributes(&xml)
            .into_iter()
            .filter_map(|a| a.into_iter().find(|(k, _)| k == "name").map(|(_, v)| v))
            .collect();
        assert_eq!(rows.len(), 291);
        assert_eq!(&rows[rows.len() - 47..], &CATEGORY_NAMES[..]);
    }

    /// With the live type table, every `balance.xml` row resolves to an absolute index and
    /// the unit rows come out in strictly increasing absolute order — which is what a
    /// file emitted by `fill_tables`' own template dump must look like.
    #[test]
    fn every_shipped_row_resolves_against_the_live_type_table() {
        let (xp, tp) = (
            repo("ron-data/balance.xml"),
            repo("schema/live/live-tables-typeids.tsv"),
        );
        if !xp.exists() || !tp.exists() {
            eprintln!("skipping: shipped balance.xml or the live type table is absent");
            return;
        }
        let tsv = std::fs::read_to_string(&tp).unwrap();
        let types = parse_live_type_table(&tsv);
        let names = AbsoluteNames::from_type_names(types.iter().map(|(i, n)| (*i, n.as_str())));
        let xml = std::fs::read_to_string(&xp).unwrap();
        let (m, unresolved) = ModifierMatrix::from_xml(&xml, &names);
        // Four display names in the shipped file have no type in the live capture, so
        // their rows and columns land nowhere — in retail too, since the engine looks the
        // element up by the name it built from the type array.
        assert_eq!(
            unresolved,
            vec!["Marines", "Pathfinder", "Pioneer", "Ranger"],
            "unresolved balance.xml names"
        );
        // 525 non-100 attribute values in the file; 9 of them sit in an unresolved row or
        // column, so 516 reach the matrix. [measured]
        assert_eq!(m.modified_cells(), 516);
    }

    /// The prediction that could have failed: `compute_modifier` returns 100 for the
    /// animal window, so every cell of `final_balance_table` in rows *or* columns
    /// `402..=413` must be exactly 100 — against a 49.7 % base rate for the value 100
    /// across the whole table.
    #[test]
    fn the_captured_table_is_neutral_across_the_animal_window() {
        let p = repo("schema/live/balance-real.bin");
        if !p.exists() {
            eprintln!("skipping: {} not present", p.display());
            return;
        }
        let t = crate::balance::BalanceTable::load(&p).expect("captured table must load");
        let mut checked = 0usize;
        for a in ANIMAL_FIRST..=ANIMAL_LAST {
            for d in FIRST_TYPE..=LAST_TYPE {
                assert_eq!(t.get(a, d), Some(NEUTRAL), "row {a} col {d}");
                assert_eq!(t.get(d, a), Some(NEUTRAL), "row {d} col {a}");
                checked += 2;
            }
        }
        assert_eq!(checked, 12 * 493 * 2);
    }
}
