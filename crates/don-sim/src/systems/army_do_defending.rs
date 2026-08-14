// SPDX-License-Identifier: GPL-3.0-or-later
//! Exact pre-host prefix of retail `Army::do_defending()`.
//!
//! The shipped PE `30478a44…625079` contains the 488-byte body at `0x006F4070`
//! (SHA-256 `5ce7d13e…e6dabd`). Its first operation is
//! `Army::count(2, 0)`; a result below five tail-calls `Army::close` at
//! `0x006F4084`, before `is_engaged`, object lookup, muster search, or RNG. A canonical
//! zero-group Army makes that count exactly zero, and `Army::close` then touches only the
//! four fields below because its group loop is empty.

use super::armies::ArmyData;

pub const RETAIL_VA: u32 = 0x006F_4070;
pub const RETAIL_SIZE: u32 = 488;
pub const RETAIL_SHA256: &str = "5ce7d13ef928ba89360862d7acfeebfbfbe9badcc526d0089f119885c5e6dabd";
pub const RETAIL_CLOSE_JUMP_VA: u32 = 0x006F_4084;
pub const MIN_MOBILE_DEFENDERS: i32 = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmptyDefendingPrefixExit {
    Closed,
    RequiresGroupCount,
}

/// Execute the complete exact transaction when `count(2, 0)` is provably zero.
///
/// Non-empty Armies fail closed before mutation because their Group rows are not owned here.
pub fn do_defending_empty_prefix(army: &mut ArmyData) -> EmptyDefendingPrefixExit {
    if army.num_groups != 0 {
        return EmptyDefendingPrefixExit::RequiresGroupCount;
    }

    // `count(2, 0) == 0 < 5`, followed by the zero-group `Army::close` body.
    army.valid = 0;
    army.status = 0;
    army.human_frame = 0;
    army.num_groups = 0;
    EmptyDefendingPrefixExit::Closed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retail_identity_is_stable() {
        assert_eq!(RETAIL_VA, 0x006F_4070);
        assert_eq!(RETAIL_SIZE, 488);
        assert_eq!(RETAIL_SHA256.len(), 64);
        assert_eq!(RETAIL_CLOSE_JUMP_VA, 0x006F_4084);
        assert_eq!(MIN_MOBILE_DEFENDERS, 5);
    }

    #[test]
    fn zero_group_prefix_closes_without_touching_other_fields() {
        let mut army = ArmyData::default();
        army.valid = 1;
        army.status = 0x20;
        army.human_frame = 7;
        army.city = -1;
        army.x = 0x1234;
        let city = army.city;
        let x = army.x;

        assert_eq!(
            do_defending_empty_prefix(&mut army),
            EmptyDefendingPrefixExit::Closed
        );
        assert_eq!(army.valid, 0);
        assert_eq!(army.status, 0);
        assert_eq!(army.human_frame, 0);
        assert_eq!(army.num_groups, 0);
        assert_eq!(army.city, city);
        assert_eq!(army.x, x);
    }

    #[test]
    fn nonempty_army_stops_before_mutation() {
        let mut army = ArmyData::default();
        army.valid = 1;
        army.status = 0x20;
        army.num_groups = 1;
        let before = army.clone();

        assert_eq!(
            do_defending_empty_prefix(&mut army),
            EmptyDefendingPrefixExit::RequiresGroupCount
        );
        assert_eq!(army, before);
    }
}
