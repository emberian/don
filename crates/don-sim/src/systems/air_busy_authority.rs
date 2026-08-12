// SPDX-License-Identifier: GPL-3.0-or-later
//! Exact `UnitData::is_busy` projection used by the AIR launch receivers.
//!
//! Retail `0x0060A370` asks the current CastOrder for its spell id, then calls that spell
//! object's virtual `+0x50` and (only if false) `+0x54`. Either nonzero answer is busy. Every
//! other shape tail-calls `UnitData::is_entering_or_exiting` `0x0060A6F0`, which is true only
//! for the persisted `SpecialAnim::{Enter,Exit}` discriminators.

use crate::order::{Order, OrderIndex};
use crate::systems::economy_order_payload_authority::{CastOrderPayload, EconomyOrderPayload};

pub const UNIT_IS_BUSY_VA: u32 = 0x0060_a370;
pub const UNIT_IS_BUSY_SIZE: u32 = 155;
pub const UNIT_IS_ENTERING_OR_EXITING_VA: u32 = 0x0060_a6f0;

/// Exact answers of one spell object's virtual `+0x50/+0x54` predicates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AirBusySpellAuthority {
    pub spell: i32,
    pub predicate_50: bool,
    pub predicate_54: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AirBusyAuthorityError {
    DuplicateSpell(i32),
    MissingSpell(i32),
    MalformedCastOrder,
    MalformedSpecialAnim,
}

pub fn validate_spell_authority(
    authority: &[AirBusySpellAuthority],
) -> Result<(), AirBusyAuthorityError> {
    for (index, facts) in authority.iter().enumerate() {
        if authority[..index]
            .iter()
            .any(|old| old.spell == facts.spell)
        {
            return Err(AirBusyAuthorityError::DuplicateSpell(facts.spell));
        }
    }
    Ok(())
}

/// Compute the exact retail answer from the current canonical order and installed spell table.
/// No order, ordinary orders, and `SpecialAnim::Unit` are authoritatively not busy and need no
/// external content answer. A reached Cast or malformed SpecialAnim never receives a default.
pub fn exact_air_busy(
    current: Option<&Order>,
    authority: &[AirBusySpellAuthority],
) -> Result<bool, AirBusyAuthorityError> {
    let Some(order) = current else {
        return Ok(false);
    };
    if order.kind == OrderIndex::CastSpell {
        let Some(EconomyOrderPayload::CastSpell(CastOrderPayload { spell, .. })) = order.economy
        else {
            return Err(AirBusyAuthorityError::MalformedCastOrder);
        };
        let facts = authority
            .iter()
            .find(|facts| facts.spell == spell)
            .ok_or(AirBusyAuthorityError::MissingSpell(spell))?;
        return Ok(facts.predicate_50 || facts.predicate_54);
    }
    order
        .is_entering_or_exiting()
        .ok_or(AirBusyAuthorityError::MalformedSpecialAnim)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::order::{SpecialAnimType, ORDER_GROUP};

    fn cast(spell: i32) -> Order {
        Order {
            kind: OrderIndex::CastSpell,
            flags: ORDER_GROUP,
            economy: Some(EconomyOrderPayload::CastSpell(CastOrderPayload {
                paid: 1,
                spell,
            })),
            ..Order::default()
        }
    }

    #[test]
    fn cast_virtual_answers_are_ored_in_retail_order() {
        let spell = 0x292;
        for (predicate_50, predicate_54, expected) in [
            (false, false, false),
            (true, false, true),
            (false, true, true),
            (true, true, true),
        ] {
            assert_eq!(
                exact_air_busy(
                    Some(&cast(spell)),
                    &[AirBusySpellAuthority {
                        spell,
                        predicate_50,
                        predicate_54,
                    }],
                ),
                Ok(expected)
            );
        }
    }

    #[test]
    fn special_anim_uses_the_persisted_discriminator_and_malformed_orders_refuse() {
        assert_eq!(exact_air_busy(None, &[]), Ok(false));
        assert_eq!(
            exact_air_busy(
                Some(&Order::special_anim(SpecialAnimType::Enter, 0, 0)),
                &[],
            ),
            Ok(true)
        );
        assert_eq!(
            exact_air_busy(Some(&Order::special_anim(SpecialAnimType::Exit, 0, 0)), &[],),
            Ok(true)
        );
        assert_eq!(
            exact_air_busy(Some(&Order::special_anim(SpecialAnimType::Unit, 0, 0)), &[],),
            Ok(false)
        );
        assert_eq!(
            exact_air_busy(
                Some(&Order {
                    kind: OrderIndex::SpecialAnim,
                    special_anim: None,
                    ..Order::default()
                }),
                &[],
            ),
            Err(AirBusyAuthorityError::MalformedSpecialAnim)
        );
    }

    #[test]
    fn reached_cast_requires_one_unambiguous_spell_row() {
        let order = cast(77);
        assert_eq!(
            exact_air_busy(Some(&order), &[]),
            Err(AirBusyAuthorityError::MissingSpell(77))
        );
        assert_eq!(
            validate_spell_authority(&[
                AirBusySpellAuthority {
                    spell: 77,
                    predicate_50: false,
                    predicate_54: false,
                },
                AirBusySpellAuthority {
                    spell: 77,
                    predicate_50: true,
                    predicate_54: false,
                },
            ]),
            Err(AirBusyAuthorityError::DuplicateSpell(77))
        );
    }
}
