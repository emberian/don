//! Defeated-player Unit-band transaction from `Leader::defeat`.
//!
//! Retail does not raze every owned object here. After the Build-band
//! `Build::clean_queue(0)` sweep, `0x006ECC1C..0x006ECCAF` walks the owner's Unit band:
//!
//! ```text
//! for unit in owner.units:
//!     if !(unit.flags & VALID): continue
//!     if unit.is_plane(): unit.die(0, -1, 0.0)
//!     else:               unit.clear_orders()
//!     unit.unit_masks &= ~0x0004_0000
//! ```
//!
//! The two virtual slots are resolved against the shipped `Unit` vtable `0x00B417D0`:
//! `+0xC0 = UnitData::is_plane` `0x0046CE40`, and `+0x158 = Unit::die`
//! `0x0060EDA0`. [`plan_defeated_unit`] freezes that dispatch independently of any one
//! live store; the tick adapter owns the concrete order/path/death mutations.

/// `UnitData::unit_masks` bit cleared after either branch at `0x006ECCA6`.
pub const DEFEAT_UNIT_MASK: u32 = 0x0004_0000;

/// The one branch selected for a valid Unit in the defeated owner's band.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefeatedUnitAction {
    /// `Unit::die(0, -1, 0.0)` for exact `UnitData::is_plane()` hits.
    DiePlane,
    /// `Unit::clear_orders()` for every other valid Unit.
    CloseOrders,
}

/// Resolve the retail branch. Invalid/stale object slots are skipped without mutation.
#[inline]
pub const fn plan_defeated_unit(valid: bool, is_plane: bool) -> Option<DefeatedUnitAction> {
    if !valid {
        None
    } else if is_plane {
        Some(DefeatedUnitAction::DiePlane)
    } else {
        Some(DefeatedUnitAction::CloseOrders)
    }
}

/// Mutation counts returned by the concrete live adapter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DefeatCleanupReceipt {
    pub owner: usize,
    pub slots_visited: usize,
    pub invalid_skipped: usize,
    pub planes_killed: usize,
    pub orders_closed: usize,
    pub unit_masks_cleared: usize,
}

/// Missing live facts that prevent an owner sweep from being classified before mutation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefeatCleanupError {
    MissingUnitType {
        owner: usize,
        object_id: usize,
    },
    UnsupportedUnitType {
        owner: usize,
        object_id: usize,
        type_index: i32,
    },
    MissingPathState {
        owner: usize,
        object_id: usize,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_valid_true_planes_take_the_die_branch() {
        assert_eq!(plan_defeated_unit(false, false), None);
        assert_eq!(plan_defeated_unit(false, true), None);
        assert_eq!(
            plan_defeated_unit(true, false),
            Some(DefeatedUnitAction::CloseOrders)
        );
        assert_eq!(
            plan_defeated_unit(true, true),
            Some(DefeatedUnitAction::DiePlane)
        );
    }

    #[test]
    fn defeated_mask_is_the_post_branch_army_leash_bit() {
        let before = 0x55a5_ffffu32;
        assert_eq!(before & !DEFEAT_UNIT_MASK, 0x55a1_ffff);
    }
}
