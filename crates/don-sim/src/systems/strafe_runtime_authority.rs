// SPDX-License-Identifier: GPL-3.0-or-later
//! Lossless source-only authority for the concrete `StrafeOrder` payload.
//!
//! The canonical runtime representation remains [`crate::systems::patrol::StrafeOrder`].
//! This module only freezes its retail walk image and the independently reserved DoNSave
//! v13 tag-8 leaf. It is intentionally not registered or connected to `Order`, dispatch,
//! tick, or save/load until those shared owners can publish one atomic STRAFE transaction.

use crate::systems::patrol::StrafeOrder;

pub const DON_SAVE_STRAFE_TAG: u8 = 8;
pub const STRAFE_PAYLOAD_VERSION: u8 = 1;
pub const STRAFE_LEAF_BYTES: usize = 47;
pub const STRAFE_RETAIL_WALK_BYTES: usize = 57;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StrafeTargetIdentity {
    pub o: i32,
    pub who: i32,
    pub uid: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StrafeAuthorityError {
    WrongPayloadTag(u8),
    WrongPayloadVersion(u8),
    Truncated,
    TrailingBytes { expected: usize, actual: usize },
    IncoherentTargetAddress { o: i32, who: i32 },
    IncoherentHomeAddress { o: i32, who: i32 },
    NonCanonicalFlag { field: &'static str, value: u8 },
}

#[inline]
pub const fn target_identity(order: &StrafeOrder) -> StrafeTargetIdentity {
    StrafeTargetIdentity {
        o: order.target_o,
        who: order.target_who,
        uid: order.target_uid,
    }
}

fn validate_pair(o: i32, who: i32) -> bool {
    matches!((o >= 0, who >= 0), (true, true) | (false, false))
}

fn validate_flag(field: &'static str, value: u8) -> Result<(), StrafeAuthorityError> {
    if value <= 1 {
        Ok(())
    } else {
        Err(StrafeAuthorityError::NonCanonicalFlag { field, value })
    }
}

pub fn validate_strafe_order(order: &StrafeOrder) -> Result<(), StrafeAuthorityError> {
    if !validate_pair(order.target_o, order.target_who) {
        return Err(StrafeAuthorityError::IncoherentTargetAddress {
            o: order.target_o,
            who: order.target_who,
        });
    }
    if !validate_pair(order.air.oxx, order.air.whose) {
        return Err(StrafeAuthorityError::IncoherentHomeAddress {
            o: order.air.oxx,
            who: order.air.whose,
        });
    }
    validate_flag("mandatory", order.mandatory)?;
    validate_flag("defensive", order.defensive)?;
    validate_flag("in_range", order.in_range)?;
    validate_flag("ever_in_range", order.ever_in_range)?;
    validate_flag("new_ord", order.new_ord)?;
    Ok(())
}

fn put_i32(out: &mut Vec<u8>, value: i32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn append_attack_and_air(out: &mut Vec<u8>, order: &StrafeOrder) {
    put_i32(out, order.def_x);
    put_i32(out, order.def_y);
    out.extend_from_slice(&[
        order.mandatory,
        order.defensive,
        order.in_range,
        order.ever_in_range,
        order.new_ord,
    ]);
    for value in [
        order.air.oxx,
        order.air.whose,
        order.air.cruising_alt,
        order.air.sharp_turn,
        order.air.old,
        order.air.returning,
        order.xx,
        order.yy,
    ] {
        put_i32(out, value);
    }
}

/// Exact 57-byte image submitted by `StrafeOrder::walk_data` in retail call order.
pub fn retail_walk_bytes(
    order_flags: u8,
    order: &StrafeOrder,
) -> Result<Vec<u8>, StrafeAuthorityError> {
    validate_strafe_order(order)?;
    let mut out = Vec::with_capacity(STRAFE_RETAIL_WALK_BYTES);
    out.push(order_flags);
    put_i32(&mut out, order.target_o);
    put_i32(&mut out, order.target_who);
    put_u16(&mut out, order.target_uid);
    put_i32(&mut out, order.def_x);
    put_i32(&mut out, order.def_y);
    out.extend_from_slice(&[
        order.mandatory,
        order.defensive,
        order.in_range,
        order.ever_in_range,
        order.new_ord,
    ]);
    // The shared virtual UnitOrder flags byte is visited again through the AirOrder base.
    out.push(order_flags);
    for value in [
        order.air.oxx,
        order.air.whose,
        order.air.cruising_alt,
        order.air.sharp_turn,
        order.air.old,
        order.air.returning,
        order.xx,
        order.yy,
    ] {
        put_i32(&mut out, value);
    }
    debug_assert_eq!(out.len(), STRAFE_RETAIL_WALK_BYTES);
    Ok(out)
}

/// Fixed-width body for DoNSave v13's reserved typed-order tag 8.
///
/// Target identity and common order flags stay in the existing generic order header. The
/// leaf owns every remaining walked byte; decoding receives the header identity explicitly
/// so no foreign target can be invented by the payload codec.
pub fn encode_strafe_leaf(order: &StrafeOrder) -> Result<Vec<u8>, StrafeAuthorityError> {
    validate_strafe_order(order)?;
    let mut out = Vec::with_capacity(STRAFE_LEAF_BYTES);
    out.extend_from_slice(&[DON_SAVE_STRAFE_TAG, STRAFE_PAYLOAD_VERSION]);
    append_attack_and_air(&mut out, order);
    debug_assert_eq!(out.len(), STRAFE_LEAF_BYTES);
    Ok(out)
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], StrafeAuthorityError> {
        let end = self
            .at
            .checked_add(count)
            .ok_or(StrafeAuthorityError::Truncated)?;
        let bytes = self
            .bytes
            .get(self.at..end)
            .ok_or(StrafeAuthorityError::Truncated)?;
        self.at = end;
        Ok(bytes)
    }

    fn u8(&mut self) -> Result<u8, StrafeAuthorityError> {
        Ok(self.take(1)?[0])
    }

    fn i32(&mut self) -> Result<i32, StrafeAuthorityError> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn finish(self) -> Result<(), StrafeAuthorityError> {
        if self.at == self.bytes.len() {
            Ok(())
        } else {
            Err(StrafeAuthorityError::TrailingBytes {
                expected: self.at,
                actual: self.bytes.len(),
            })
        }
    }
}

pub fn decode_strafe_leaf(
    target: StrafeTargetIdentity,
    bytes: &[u8],
) -> Result<StrafeOrder, StrafeAuthorityError> {
    let mut reader = Reader { bytes, at: 0 };
    let tag = reader.u8()?;
    if tag != DON_SAVE_STRAFE_TAG {
        return Err(StrafeAuthorityError::WrongPayloadTag(tag));
    }
    let version = reader.u8()?;
    if version != STRAFE_PAYLOAD_VERSION {
        return Err(StrafeAuthorityError::WrongPayloadVersion(version));
    }
    let def_x = reader.i32()?;
    let def_y = reader.i32()?;
    let mandatory = reader.u8()?;
    let defensive = reader.u8()?;
    let in_range = reader.u8()?;
    let ever_in_range = reader.u8()?;
    let new_ord = reader.u8()?;
    let order = StrafeOrder {
        target_o: target.o,
        target_who: target.who,
        target_uid: target.uid,
        def_x,
        def_y,
        mandatory,
        defensive,
        in_range,
        ever_in_range,
        new_ord,
        air: crate::systems::air::AirOrderWalk {
            oxx: reader.i32()?,
            whose: reader.i32()?,
            cruising_alt: reader.i32()?,
            sharp_turn: reader.i32()?,
            old: reader.i32()?,
            returning: reader.i32()?,
        },
        xx: reader.i32()?,
        yy: reader.i32()?,
    };
    reader.finish()?;
    validate_strafe_order(&order)?;
    Ok(order)
}
