//! Parsers for Rise of Nations rule data.
//!
//! Everything here is derived from the shipped data files or from the binary, per
//! `docs/CHARTER.md`. Community documentation is never a source.

pub mod offsets;
pub mod rules;
pub mod value;

pub use rules::{Parser, RuleField, RuleSlot, Rules, FIELDS, RULES_DWORDS, SHIPPED, SLOTS};
pub use value::{apply_parser, as_int, as_scaled, wtoi, Number, RuleValue};

#[cfg(test)]
mod ground_truth {
    //! Anchors for the generated offset table.
    //!
    //! These three values were read out of Ghidra's decompiled C for two loaders that the
    //! decompiler *can* handle, independently of the instruction-level extractor that
    //! produced `offsets.rs`. They are the only bindings for which we have two independent
    //! derivations, so they are the regression anchor: if the extractor or the generator
    //! ever drifts, these break first.
    use crate::offsets;

    #[test]
    fn extractor_agrees_with_decompiled_c() {
        assert_eq!(offsets::fun_0065fc00::RECHARGE, 500);
        assert_eq!(offsets::fun_0061c490::CREW_SIZE, 780);
        assert_eq!(offsets::fun_0061c490::BASE_FORM, 784);
    }

    /// The rules.xml constant block is contiguous on a 4-byte stride from offset 0.
    /// This is what let us recognise it as a table at all, so it is worth pinning.
    #[test]
    fn rules_constant_block_is_contiguous() {
        use offsets::fun_00570170 as r;
        assert_eq!(r::UNIT_FORMATION_SPACING, 0);
        assert_eq!(r::UNIT_MOVE_SPEED, 4);
        assert_eq!(r::UNIT_TURN_SPEED, 8);
        assert_eq!(r::UNIT_PACK_TURN_BONUS, 12);
        assert_eq!(r::UNIT_BLOCK_RADIUS, 16);
        assert_eq!(r::UNIT_GUY_SPACING, 20);
    }

    /// Binding is by name, not by position: `ship_defensive_respond_range` follows
    /// `unit_defensive_respond_range` in the XML but lands well after it in memory.
    #[test]
    fn binding_is_by_name_not_position() {
        use offsets::fun_00570170 as r;
        assert_eq!(r::UNIT_DEFENSIVE_RESPOND_RANGE, 28);
        assert_eq!(r::SHIP_DEFENSIVE_RESPOND_RANGE, 52);
        assert!(r::SHIP_DEFENSIVE_RESPOND_RANGE > r::UNIT_GATHER_RESPOND_RANGE);
    }
}
