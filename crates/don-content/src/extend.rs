//! The extension surface beyond retail: new types, and hooks.
//!
//! # The wall retail hits
//!
//! Rise of Nations' type ids are a **compile-time enum**. `enum TypeIndex` (PDB type stream)
//! runs `0 ..= 805` with hard-coded family bases, and `Balance::final_balance_table`
//! `0x00C12BF4` is a static `short[493][493]` — 486,098 bytes, confirmed byte-for-byte by
//! `schema/live/balance-real.bin`. There is no allocation and no count read from data.
//!
//! Therefore, and this is the sharp claim of this module: **a retail mod cannot add a unit.**
//! It can retune all 352 unit slots, rename them, repoint their art and change what trains
//! them, but the 353rd unit does not exist and cannot be made to. Every "new unit" mod on the
//! Workshop is a reskinned existing slot. The families, all [measured] from `enum TypeIndex`:
//!
//! | family | ids | count |
//! |---|---|---:|
//! | goods (6 common + 44 rare) | `0..50` | 50 |
//! | units | `50..402` | 352 |
//! | gaia | `402..414` | 12 |
//! | buildings (wonders `526..543`) | `414..543` | 129 |
//! | items | `543..544` | 1 |
//! | techs (ages `544..551`, epochs `551..579`, finals `579..583`, govs `623..629`) | `544..629` | 85 |
//! | spells | `629..684` | 55 |
//! | bonuses | `684..806` | 122 |
//!
//! Note the balance matrix is **narrower than the type space**: 493 < 806. It spans ids
//! `0..493`, which stops partway into the building range. That is a shipped fact, not a
//! reading error — it is why a bias-folded base at `0x00C06AFC` (49,400 bytes early) produced
//! the "unexplained negatives" recorded in `CODEX.md`.
//!
//! # What we do instead
//!
//! [`TypeSpace`] keeps `0..806` reserved and identical to retail, and allocates extension ids
//! from `806` upward. That choice is deliberate:
//!
//! * every retail id keeps its numeric value, so a captured table, a `.rcx` command stream
//!   and a disassembly listing all still index correctly;
//! * an extension id is trivially detectable (`id >= RETAIL_NUM_TYPES`), so fidelity mode can
//!   refuse one by construction rather than by convention;
//! * the balance matrix becomes dense-base + sparse-overlay, so the 486,098-byte capture stays
//!   the ground truth and extensions cost only what they use.
//!
//! # Hooks
//!
//! [`HookPoint`] enumerates the places an overlay may attach. The list is deliberately short
//! and every entry names the retail step it sits at, because a hook that does not correspond
//! to a real ordered position in `Game::do_frame` is a hook whose determinism nobody can
//! reason about. `crates/don-sim/src/schedule.rs` owns `DO_FRAME`; this enum is the contract
//! a mod sees, not the schedule itself.

use std::collections::BTreeMap;

use crate::generated::{BALANCE_TABLE_SIDE, RETAIL_NUM_TYPES, TYPE_RANGES};

/// A type id. Values below [`RETAIL_NUM_TYPES`] mean exactly what they mean in retail.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TypeId(pub u16);

impl TypeId {
    pub fn is_retail(self) -> bool {
        self.0 < RETAIL_NUM_TYPES
    }

    /// Which `enum TypeIndex` family this id falls in, if any.
    pub fn family(self) -> Option<&'static str> {
        TYPE_RANGES
            .iter()
            .find(|(_, b, e)| self.0 >= *b && self.0 < *e)
            .map(|(n, _, _)| *n)
    }

    /// Whether `Balance::final_balance_table` has a cell for this id at all.
    pub fn in_balance_matrix(self) -> bool {
        self.0 < BALANCE_TABLE_SIDE
    }
}

#[derive(Clone, Debug)]
pub struct TypeDef {
    pub id: TypeId,
    pub name: String,
    /// Which retail family the new type behaves as. Extensions must pick one so every
    /// existing system knows how to treat them.
    pub family: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtendError {
    /// Fidelity mode forbids any id at or above [`RETAIL_NUM_TYPES`].
    FidelityForbidsExtension,
    /// The family label is not one of `enum TypeIndex`'s.
    UnknownFamily,
}

impl std::fmt::Display for ExtendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExtendError::FidelityForbidsExtension => {
                write!(
                    f,
                    "fidelity mode: type ids must stay below {RETAIL_NUM_TYPES}"
                )
            }
            ExtendError::UnknownFamily => write!(f, "family is not one of enum TypeIndex's"),
        }
    }
}

impl std::error::Error for ExtendError {}

/// Retail's closed id space, plus room above it.
#[derive(Clone, Debug)]
pub struct TypeSpace {
    next: u16,
    extensions: Vec<TypeDef>,
}

impl Default for TypeSpace {
    fn default() -> Self {
        TypeSpace::new()
    }
}

impl TypeSpace {
    pub fn new() -> TypeSpace {
        TypeSpace {
            next: RETAIL_NUM_TYPES,
            extensions: Vec::new(),
        }
    }

    pub fn retail_len(&self) -> u16 {
        RETAIL_NUM_TYPES
    }

    pub fn len(&self) -> usize {
        RETAIL_NUM_TYPES as usize + self.extensions.len()
    }

    pub fn is_empty(&self) -> bool {
        false
    }

    pub fn extensions(&self) -> &[TypeDef] {
        &self.extensions
    }

    /// Allocate a new type id above the retail space.
    pub fn define(
        &mut self,
        name: impl Into<String>,
        family: &'static str,
        fidelity: bool,
    ) -> Result<TypeId, ExtendError> {
        if fidelity {
            return Err(ExtendError::FidelityForbidsExtension);
        }
        if !TYPE_RANGES.iter().any(|(n, _, _)| *n == family) {
            return Err(ExtendError::UnknownFamily);
        }
        let id = TypeId(self.next);
        self.next += 1;
        self.extensions.push(TypeDef {
            id,
            name: name.into(),
            family,
        });
        Ok(id)
    }
}

/// The combat matrix as a dense retail base plus a sparse extension overlay.
///
/// The dense half is `schema/live/balance-real.bin` — 493 x 493 `i16`, captured, never
/// computed. The sparse half only ever holds pairs where at least one id is an extension, or
/// where an overlay deliberately deviates.
#[derive(Clone, Debug)]
pub struct BalanceOverlay {
    sparse: BTreeMap<(u16, u16), i16>,
}

impl Default for BalanceOverlay {
    fn default() -> Self {
        BalanceOverlay::new()
    }
}

impl BalanceOverlay {
    pub fn new() -> BalanceOverlay {
        BalanceOverlay {
            sparse: BTreeMap::new(),
        }
    }

    pub fn set(&mut self, attacker: TypeId, target: TypeId, value: i16) {
        self.sparse.insert((attacker.0, target.0), value);
    }

    pub fn len(&self) -> usize {
        self.sparse.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sparse.is_empty()
    }

    /// Look up, preferring the overlay. `dense` is the captured 493x493 table, row-major.
    pub fn get(&self, dense: &[i16], attacker: TypeId, target: TypeId) -> Option<i16> {
        if let Some(v) = self.sparse.get(&(attacker.0, target.0)) {
            return Some(*v);
        }
        if attacker.in_balance_matrix() && target.in_balance_matrix() {
            let i = attacker.0 as usize * BALANCE_TABLE_SIDE as usize + target.0 as usize;
            dense.get(i).copied()
        } else {
            None
        }
    }
}

/// Where an overlay may attach. Each variant names the retail step it corresponds to so a
/// hook's ordering relative to the tick is a fact rather than an intention.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HookPoint {
    /// After the ruleset is composed and before any world exists. The only point at which a
    /// mod may change constants. Retail's equivalent is `Constants::init` `0x00569A90`.
    RulesComposed,
    /// After the initial world is built, before frame 0.
    WorldInitialised,
    /// `Game::do_frame` step 8, `Leaders::process_all` `0x006ED2A0` — start of the per-player
    /// economy pass.
    PreLeaders,
    /// `Game::do_frame` step 14, `Objects::process_all` `0x0065DCE0`, whose owner rotation is
    /// `(frame + i) % 10`.
    PreObjects,
    /// `Game::do_frame` step 20, immediately after `Game::frame++` `0x005924BF`.
    PostFrame,
    /// A game-over decision, `Game::process_end_game` `0x00591CE0`, step 27.
    EndGame,
}

impl HookPoint {
    /// Whether a hook at this point can change simulation state. `RulesComposed` and
    /// `WorldInitialised` run before the checksum stream starts, so a deviation there is
    /// visible in the `rules` channel and nowhere else; the in-tick points deviate every
    /// channel from the frame they fire.
    pub fn is_in_tick(self) -> bool {
        matches!(
            self,
            HookPoint::PreLeaders
                | HookPoint::PreObjects
                | HookPoint::PostFrame
                | HookPoint::EndGame
        )
    }

    pub fn retail_step(self) -> Option<u8> {
        match self {
            HookPoint::PreLeaders => Some(8),
            HookPoint::PreObjects => Some(14),
            HookPoint::PostFrame => Some(20),
            HookPoint::EndGame => Some(27),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retail_families_partition_the_id_space() {
        // Every retail id belongs to exactly one family, and the families are contiguous.
        let mut covered = vec![0u8; RETAIL_NUM_TYPES as usize];
        for (_, b, e) in TYPE_RANGES.iter() {
            for i in *b..*e {
                covered[i as usize] += 1;
            }
        }
        assert!(
            covered.iter().all(|c| *c == 1),
            "families overlap or leave a gap"
        );
    }

    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn the_balance_matrix_is_narrower_than_the_type_space() {
        assert!(BALANCE_TABLE_SIDE < RETAIL_NUM_TYPES);
        assert_eq!(BALANCE_TABLE_SIDE, 493);
        assert_eq!(RETAIL_NUM_TYPES, 806);
        // and the capture is exactly the matrix
        assert_eq!(
            BALANCE_TABLE_SIDE as usize * BALANCE_TABLE_SIDE as usize * 2,
            486_098
        );
    }

    #[test]
    fn known_ids_land_in_the_families_the_engine_says() {
        assert_eq!(TypeId(0).family(), Some("Good"));
        assert_eq!(TypeId(50).family(), Some("Unit"));
        assert_eq!(TypeId(401).family(), Some("Unit"));
        assert_eq!(TypeId(402).family(), Some("Gaia"));
        assert_eq!(TypeId(414).family(), Some("Build"));
        assert_eq!(TypeId(805).family(), Some("Bonus"));
        assert_eq!(TypeId(806).family(), None);
    }

    #[test]
    fn fidelity_mode_cannot_define_a_new_type() {
        let mut s = TypeSpace::new();
        assert_eq!(
            s.define("Trebuchet", "Unit", true),
            Err(ExtendError::FidelityForbidsExtension)
        );
        assert_eq!(s.len(), RETAIL_NUM_TYPES as usize);
    }

    #[test]
    fn improved_mode_allocates_above_the_retail_space() {
        let mut s = TypeSpace::new();
        let a = s.define("Trebuchet", "Unit", false).unwrap();
        let b = s.define("Aqueduct", "Build", false).unwrap();
        assert_eq!(a, TypeId(RETAIL_NUM_TYPES));
        assert_eq!(b, TypeId(RETAIL_NUM_TYPES + 1));
        assert!(!a.is_retail());
        assert!(TypeId(RETAIL_NUM_TYPES - 1).is_retail());
        assert_eq!(
            s.define("Nonsense", "NotAFamily", false),
            Err(ExtendError::UnknownFamily)
        );
    }

    #[test]
    fn the_balance_overlay_defers_to_the_capture_then_to_itself() {
        let dense = vec![7i16; BALANCE_TABLE_SIDE as usize * BALANCE_TABLE_SIDE as usize];
        let mut o = BalanceOverlay::new();
        assert_eq!(o.get(&dense, TypeId(50), TypeId(60)), Some(7));
        // an extension id has no dense cell
        assert_eq!(o.get(&dense, TypeId(RETAIL_NUM_TYPES), TypeId(60)), None);
        o.set(TypeId(RETAIL_NUM_TYPES), TypeId(60), -3);
        assert_eq!(
            o.get(&dense, TypeId(RETAIL_NUM_TYPES), TypeId(60)),
            Some(-3)
        );
        // and a deliberate deviation shadows the capture
        o.set(TypeId(50), TypeId(60), 99);
        assert_eq!(o.get(&dense, TypeId(50), TypeId(60)), Some(99));
        // an id past the matrix but inside the type space also has no dense cell
        assert_eq!(o.get(&dense, TypeId(600), TypeId(60)), None);
    }

    #[test]
    fn hook_points_name_real_tick_steps() {
        assert_eq!(HookPoint::PreObjects.retail_step(), Some(14));
        assert!(HookPoint::PreObjects.is_in_tick());
        assert!(!HookPoint::RulesComposed.is_in_tick());
        assert_eq!(HookPoint::RulesComposed.retail_step(), None);
    }
}
