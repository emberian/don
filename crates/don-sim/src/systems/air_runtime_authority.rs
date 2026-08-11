// SPDX-License-Identifier: GPL-3.0-or-later
//! Lossless runtime authority required by the canonical Group air-action host.
//!
//! This module is deliberately `std`-only and is not registered in `systems/mod.rs` yet.
//! It freezes three state contracts which the existing shadow bridge cannot provide:
//!
//! * the synchronized type projection read by `Group::action_launch_patrol` and
//!   `Group::action_scramble`;
//! * the scalar and eight ordered `ScenarioData::objects_ignoring_orders` lists consumed
//!   by the action entry prelude; and
//! * the complete dynamic `AirPatrolOrder` payload, including the `SimpleArray<Coord>`
//!   metadata which retail's virtual `walk_data` serializes.
//!
//! No gameplay state is owned here.  These values are intended to become typed fields on
//! the canonical Sim/Order owners.  Keeping an independent sidecar would create the same
//! split-brain state that currently prevents closure of opcodes 11 and 36.

use std::collections::BTreeMap;

pub const DON_SAVE_AIR_PATROL_TAG: u8 = 6;
pub const AIR_PATROL_PAYLOAD_VERSION: u8 = 1;
pub const SCENARIO_IGNORES_PAYLOAD_VERSION: u8 = 1;
pub const SCENARIO_OWNER_SLOTS: usize = 8;

/// A decoder resource limit, not a gameplay cap.  The runtime representation remains
/// dynamically sized and may be constructed with a longer route; untrusted save input is
/// rejected before allocating more than this many coordinates.
pub const MAX_DECODED_PATROL_POINTS: usize = 1 << 20;
pub const MAX_DECODED_IGNORED_OBJECTS_PER_OWNER: usize = 1 << 20;

pub const TYPE_BIPLANE: i32 = 0x11f;
pub const TYPE_BOMBER: i32 = 0x130;
pub const TYPE_HELICOPTER: i32 = 0x136;
pub const AIR_DOMAIN: i32 = 2;
pub const MISSILE_OBJECT_MASK: u32 = 0x0800_0000;
pub const HELICOPTER_UNIT_FLAG: u32 = 0x20;

/// `SimpleArray<Coord>`'s allocator-neutral checksum/save image.
///
/// The PDB layout is 28 bytes, but `SimpleArray::walk_data` `0x00483CC0` does not walk
/// capacity, pointer or cursor.  It walks `length:i32`, `increment:i16`, `flags:u8` (with
/// allocator bit `0x40` cleared), then exactly `length` four-byte elements.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WalkedCoordArray {
    pub increment: i16,
    pub flags: u8,
    pub values: Vec<i32>,
}

impl Default for WalkedCoordArray {
    fn default() -> Self {
        Self {
            // Both arrays are initialized to -1 by PatrolOrder::PatrolOrder 0x00483720.
            increment: -1,
            flags: 0,
            values: Vec::new(),
        }
    }
}

impl WalkedCoordArray {
    fn validate(&self) -> Result<(), AirRuntimeAuthorityError> {
        // Retail clears this allocator/ownership bit before walking it.  A canonical
        // persisted image must already be normalized rather than depend on a mutating save.
        if self.flags & 0x40 != 0 {
            return Err(AirRuntimeAuthorityError::UnnormalizedArrayFlags(self.flags));
        }
        Ok(())
    }

    fn append_walk_bytes(&self, out: &mut Vec<u8>) -> Result<(), AirRuntimeAuthorityError> {
        self.validate()?;
        let len = i32::try_from(self.values.len())
            .map_err(|_| AirRuntimeAuthorityError::PointCountOutOfRange(self.values.len()))?;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&self.increment.to_le_bytes());
        out.push(self.flags);
        for value in &self.values {
            out.extend_from_slice(&value.to_le_bytes());
        }
        Ok(())
    }
}

/// The flat 24-byte range walked by `AirOrder::walk_data` `0x0047F2D0`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AirOrderPayload {
    pub home_o: i32,
    pub home_who: i32,
    pub cruising_alt: i32,
    pub sharp_turn: i32,
    pub old: i32,
    pub returning: i32,
}

impl AirOrderPayload {
    fn append_bytes(self, out: &mut Vec<u8>) {
        for value in [
            self.home_o,
            self.home_who,
            self.cruising_alt,
            self.sharp_turn,
            self.old,
            self.returning,
        ] {
            out.extend_from_slice(&value.to_le_bytes());
        }
    }
}

/// Complete executable/checksum payload of one `AirPatrolOrder`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AirPatrolOrderPayload {
    pub x: WalkedCoordArray,
    pub y: WalkedCoordArray,
    pub waypoint: i32,
    pub air: AirOrderPayload,
}

impl AirPatrolOrderPayload {
    pub fn one(target_x: i32, target_y: i32, home_o: i32, home_who: i32) -> Self {
        Self {
            x: WalkedCoordArray {
                values: vec![target_x],
                ..WalkedCoordArray::default()
            },
            y: WalkedCoordArray {
                values: vec![target_y],
                ..WalkedCoordArray::default()
            },
            waypoint: 0,
            air: AirOrderPayload {
                home_o,
                home_who,
                cruising_alt: 0x640,
                sharp_turn: 0,
                old: 0,
                returning: 0,
            },
        }
    }

    pub fn validate(&self) -> Result<(), AirRuntimeAuthorityError> {
        self.x.validate()?;
        self.y.validate()?;
        if self.x.values.len() != self.y.values.len() {
            return Err(AirRuntimeAuthorityError::WaypointArrayLengthMismatch {
                x: self.x.values.len(),
                y: self.y.values.len(),
            });
        }
        if self.x.values.is_empty() {
            return Err(AirRuntimeAuthorityError::EmptyPatrolRoute);
        }
        i32::try_from(self.x.values.len())
            .map_err(|_| AirRuntimeAuthorityError::PointCountOutOfRange(self.x.values.len()))?;
        match (self.air.home_o >= 0, self.air.home_who >= 0) {
            (true, true) | (false, false) => {}
            _ => return Err(AirRuntimeAuthorityError::IncoherentHomeAddress),
        }
        Ok(())
    }

    /// Bytes submitted by `AirPatrolOrder::walk_data` in retail call order.
    ///
    /// The virtual `UnitOrder::flags` byte is reached twice: once through the primary
    /// `PatrolOrder` base and once through the secondary `AirOrder` base.  The two dynamic
    /// arrays are walked between the primary scalar prefix and the secondary base.
    pub fn retail_walk_bytes(&self, order_flags: u8) -> Result<Vec<u8>, AirRuntimeAuthorityError> {
        self.validate()?;
        let mut out = Vec::with_capacity(2 + 4 + 14 + 8 * self.x.values.len() + 24);
        out.push(order_flags);
        out.extend_from_slice(&self.waypoint.to_le_bytes());
        self.x.append_walk_bytes(&mut out)?;
        self.y.append_walk_bytes(&mut out)?;
        out.push(order_flags);
        self.air.append_bytes(&mut out);
        Ok(out)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TypeRelations {
    pub biplane_nonstrict: bool,
    pub bomber_nonstrict: bool,
    pub helicopter_nonstrict: bool,
}

/// One synchronized `ObjectTypeData` row plus the three non-strict hierarchy answers which
/// cannot be inferred from a raw type id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AirTypeRow {
    pub type_id: i32,
    pub domain: i32,
    pub object_masks: u32,
    pub unit_flags: u32,
    pub relations: TypeRelations,
}

impl AirTypeRow {
    pub const fn launch_common_eligible(self) -> bool {
        self.domain == AIR_DOMAIN && self.object_masks & MISSILE_OBJECT_MASK == 0
    }

    pub const fn is_helicopter_runtime(self) -> bool {
        self.unit_flags & HELICOPTER_UNIT_FLAG != 0
    }

    pub const fn authoritative_relation(self, queried_type: i32) -> Option<bool> {
        match queried_type {
            TYPE_BIPLANE => Some(self.relations.biplane_nonstrict),
            TYPE_BOMBER => Some(self.relations.bomber_nonstrict),
            TYPE_HELICOPTER => Some(self.relations.helicopter_nonstrict),
            _ => None,
        }
    }
}

/// Revision/digest-bound type authority installed from synchronized content.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AirTypeAuthority {
    pub revision: u64,
    pub composition_digest: [u8; 32],
    rows: BTreeMap<i32, AirTypeRow>,
}

impl AirTypeAuthority {
    pub fn new(
        revision: u64,
        composition_digest: [u8; 32],
        rows: impl IntoIterator<Item = AirTypeRow>,
    ) -> Result<Self, AirRuntimeAuthorityError> {
        if composition_digest == [0; 32] {
            return Err(AirRuntimeAuthorityError::MissingCompositionDigest);
        }
        let mut indexed = BTreeMap::new();
        for row in rows {
            if row.type_id < 0 {
                return Err(AirRuntimeAuthorityError::NegativeTypeId(row.type_id));
            }
            if indexed.insert(row.type_id, row).is_some() {
                return Err(AirRuntimeAuthorityError::DuplicateTypeId(row.type_id));
            }
        }
        Ok(Self {
            revision,
            composition_digest,
            rows: indexed,
        })
    }

    pub fn row(&self, type_id: i32) -> Result<AirTypeRow, AirRuntimeAuthorityError> {
        self.rows
            .get(&type_id)
            .copied()
            .ok_or(AirRuntimeAuthorityError::MissingTypeRow(type_id))
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// Canonical scenario state consumed before the launch-patrol body.  List order,
/// duplicates and negative tombstones are intentionally retained.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScenarioIgnoreOrdersAuthority {
    pub revision: u64,
    pub ignore_orders: bool,
    pub ignored_by_owner: [Vec<i32>; SCENARIO_OWNER_SLOTS],
}

impl ScenarioIgnoreOrdersAuthority {
    pub fn validate(&self) -> Result<(), AirRuntimeAuthorityError> {
        for (owner, list) in self.ignored_by_owner.iter().enumerate() {
            for &object in list {
                if i16::try_from(object).is_err() {
                    return Err(AirRuntimeAuthorityError::IgnoredObjectOutOfRange {
                        owner,
                        object,
                    });
                }
            }
        }
        Ok(())
    }

    pub fn ignored_for_owner(&self, owner: usize) -> Result<&[i32], AirRuntimeAuthorityError> {
        self.validate()?;
        self.ignored_by_owner
            .get(owner)
            .map(Vec::as_slice)
            .ok_or(AirRuntimeAuthorityError::ScenarioOwnerOutOfRange(owner))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AirRuntimeAuthorityError {
    WrongPayloadTag(u8),
    WrongPayloadVersion(u8),
    WrongScenarioPayloadVersion(u8),
    InvalidBoolean(u8),
    Truncated,
    TrailingBytes { expected: usize, actual: usize },
    PointCountLimit(usize),
    PointCountOutOfRange(usize),
    WaypointArrayLengthMismatch { x: usize, y: usize },
    EmptyPatrolRoute,
    UnnormalizedArrayFlags(u8),
    IncoherentHomeAddress,
    MissingCompositionDigest,
    NegativeTypeId(i32),
    DuplicateTypeId(i32),
    MissingTypeRow(i32),
    ScenarioOwnerOutOfRange(usize),
    IgnoredObjectLimit { owner: usize, len: usize },
    IgnoredObjectOutOfRange { owner: usize, object: i32 },
}

fn put_i16(out: &mut Vec<u8>, value: i16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_i32(out: &mut Vec<u8>, value: i32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], AirRuntimeAuthorityError> {
        let end = self
            .at
            .checked_add(len)
            .ok_or(AirRuntimeAuthorityError::Truncated)?;
        let result = self
            .bytes
            .get(self.at..end)
            .ok_or(AirRuntimeAuthorityError::Truncated)?;
        self.at = end;
        Ok(result)
    }

    fn u8(&mut self) -> Result<u8, AirRuntimeAuthorityError> {
        Ok(self.take(1)?[0])
    }

    fn i16(&mut self) -> Result<i16, AirRuntimeAuthorityError> {
        Ok(i16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn u32(&mut self) -> Result<u32, AirRuntimeAuthorityError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn i32(&mut self) -> Result<i32, AirRuntimeAuthorityError> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn finish(self) -> Result<(), AirRuntimeAuthorityError> {
        if self.at == self.bytes.len() {
            Ok(())
        } else {
            Err(AirRuntimeAuthorityError::TrailingBytes {
                expected: self.at,
                actual: self.bytes.len(),
            })
        }
    }
}

fn write_coord_array(
    out: &mut Vec<u8>,
    array: &WalkedCoordArray,
) -> Result<(), AirRuntimeAuthorityError> {
    array.validate()?;
    if array.values.len() > MAX_DECODED_PATROL_POINTS {
        return Err(AirRuntimeAuthorityError::PointCountLimit(
            array.values.len(),
        ));
    }
    let len = u32::try_from(array.values.len())
        .map_err(|_| AirRuntimeAuthorityError::PointCountOutOfRange(array.values.len()))?;
    put_u32(out, len);
    put_i16(out, array.increment);
    out.push(array.flags);
    for &value in &array.values {
        put_i32(out, value);
    }
    Ok(())
}

fn read_coord_array(reader: &mut Reader<'_>) -> Result<WalkedCoordArray, AirRuntimeAuthorityError> {
    let len = reader.u32()? as usize;
    if len > MAX_DECODED_PATROL_POINTS {
        return Err(AirRuntimeAuthorityError::PointCountLimit(len));
    }
    let increment = reader.i16()?;
    let flags = reader.u8()?;
    let mut values = Vec::with_capacity(len);
    for _ in 0..len {
        values.push(reader.i32()?);
    }
    let array = WalkedCoordArray {
        increment,
        flags,
        values,
    };
    array.validate()?;
    Ok(array)
}

/// Encode the body inserted at DoNSave's reserved typed order tag 6.
pub fn encode_air_patrol_leaf(
    payload: &AirPatrolOrderPayload,
) -> Result<Vec<u8>, AirRuntimeAuthorityError> {
    payload.validate()?;
    let mut out = Vec::new();
    out.push(DON_SAVE_AIR_PATROL_TAG);
    out.push(AIR_PATROL_PAYLOAD_VERSION);
    write_coord_array(&mut out, &payload.x)?;
    write_coord_array(&mut out, &payload.y)?;
    put_i32(&mut out, payload.waypoint);
    payload.air.append_bytes(&mut out);
    Ok(out)
}

pub fn decode_air_patrol_leaf(
    bytes: &[u8],
) -> Result<AirPatrolOrderPayload, AirRuntimeAuthorityError> {
    let mut reader = Reader::new(bytes);
    let tag = reader.u8()?;
    if tag != DON_SAVE_AIR_PATROL_TAG {
        return Err(AirRuntimeAuthorityError::WrongPayloadTag(tag));
    }
    let version = reader.u8()?;
    if version != AIR_PATROL_PAYLOAD_VERSION {
        return Err(AirRuntimeAuthorityError::WrongPayloadVersion(version));
    }
    let x = read_coord_array(&mut reader)?;
    let y = read_coord_array(&mut reader)?;
    let waypoint = reader.i32()?;
    let air = AirOrderPayload {
        home_o: reader.i32()?,
        home_who: reader.i32()?,
        cruising_alt: reader.i32()?,
        sharp_turn: reader.i32()?,
        old: reader.i32()?,
        returning: reader.i32()?,
    };
    reader.finish()?;
    let result = AirPatrolOrderPayload {
        x,
        y,
        waypoint,
        air,
    };
    result.validate()?;
    Ok(result)
}

/// Persistence image for the scenario scalar/lists.  Revisions are process-local and are
/// deliberately not serialized.
pub fn encode_scenario_ignores(
    authority: &ScenarioIgnoreOrdersAuthority,
) -> Result<Vec<u8>, AirRuntimeAuthorityError> {
    authority.validate()?;
    let mut out = vec![
        SCENARIO_IGNORES_PAYLOAD_VERSION,
        u8::from(authority.ignore_orders),
    ];
    for (owner, list) in authority.ignored_by_owner.iter().enumerate() {
        if list.len() > MAX_DECODED_IGNORED_OBJECTS_PER_OWNER {
            return Err(AirRuntimeAuthorityError::IgnoredObjectLimit {
                owner,
                len: list.len(),
            });
        }
        let len = u32::try_from(list.len()).map_err(|_| {
            AirRuntimeAuthorityError::IgnoredObjectLimit {
                owner,
                len: list.len(),
            }
        })?;
        put_u32(&mut out, len);
        for &object in list {
            put_i32(&mut out, object);
        }
    }
    Ok(out)
}

pub fn decode_scenario_ignores(
    bytes: &[u8],
) -> Result<ScenarioIgnoreOrdersAuthority, AirRuntimeAuthorityError> {
    let mut reader = Reader::new(bytes);
    let version = reader.u8()?;
    if version != SCENARIO_IGNORES_PAYLOAD_VERSION {
        return Err(AirRuntimeAuthorityError::WrongScenarioPayloadVersion(
            version,
        ));
    }
    let ignore_orders = match reader.u8()? {
        0 => false,
        1 => true,
        other => return Err(AirRuntimeAuthorityError::InvalidBoolean(other)),
    };
    let mut ignored_by_owner: [Vec<i32>; SCENARIO_OWNER_SLOTS] =
        std::array::from_fn(|_| Vec::new());
    for (owner, list) in ignored_by_owner.iter_mut().enumerate() {
        let len = reader.u32()? as usize;
        if len > MAX_DECODED_IGNORED_OBJECTS_PER_OWNER {
            return Err(AirRuntimeAuthorityError::IgnoredObjectLimit { owner, len });
        }
        list.reserve(len);
        for _ in 0..len {
            let object = reader.i32()?;
            if i16::try_from(object).is_err() {
                return Err(AirRuntimeAuthorityError::IgnoredObjectOutOfRange { owner, object });
            }
            list.push(object);
        }
    }
    reader.finish()?;
    Ok(ScenarioIgnoreOrdersAuthority {
        revision: 0,
        ignore_orders,
        ignored_by_owner,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload() -> AirPatrolOrderPayload {
        AirPatrolOrderPayload {
            x: WalkedCoordArray {
                increment: -1,
                flags: 8,
                values: vec![1, 2, 3],
            },
            y: WalkedCoordArray {
                increment: 4,
                flags: 0x10,
                values: vec![-4, 5, 6],
            },
            waypoint: 2,
            air: AirOrderPayload {
                home_o: 2_015,
                home_who: 0,
                cruising_alt: 0x640,
                sharp_turn: 1,
                old: 2,
                returning: 0,
            },
        }
    }

    #[test]
    fn typed_leaf_is_lossless_and_deterministic() {
        let before = payload();
        let bytes = encode_air_patrol_leaf(&before).unwrap();
        let after = decode_air_patrol_leaf(&bytes).unwrap();
        assert_eq!(after, before);
        assert_eq!(encode_air_patrol_leaf(&after).unwrap(), bytes);
    }

    #[test]
    fn walk_keeps_array_metadata_and_visits_flags_twice() {
        let bytes = payload().retail_walk_bytes(0xa5).unwrap();
        assert_eq!(bytes.len(), 44 + 8 * 3);
        assert_eq!(bytes[0], 0xa5);
        assert_eq!(bytes[19 + 8 * 3], 0xa5);
    }

    #[test]
    fn malformed_route_and_envelope_refuse() {
        let mut bad = payload();
        bad.y.values.pop();
        assert!(matches!(
            encode_air_patrol_leaf(&bad),
            Err(AirRuntimeAuthorityError::WaypointArrayLengthMismatch { .. })
        ));
        let mut bytes = encode_air_patrol_leaf(&payload()).unwrap();
        bytes.push(0);
        assert!(matches!(
            decode_air_patrol_leaf(&bytes),
            Err(AirRuntimeAuthorityError::TrailingBytes { .. })
        ));
    }

    #[test]
    fn array_ownership_bit_must_be_normalized() {
        let mut bad = payload();
        bad.x.flags |= 0x40;
        assert_eq!(
            encode_air_patrol_leaf(&bad),
            Err(AirRuntimeAuthorityError::UnnormalizedArrayFlags(0x48))
        );
    }

    #[test]
    fn type_projection_requires_digest_and_unique_rows() {
        let row = AirTypeRow {
            type_id: 7,
            domain: AIR_DOMAIN,
            object_masks: 0,
            unit_flags: HELICOPTER_UNIT_FLAG,
            relations: TypeRelations {
                helicopter_nonstrict: true,
                ..TypeRelations::default()
            },
        };
        let authority = AirTypeAuthority::new(5, [9; 32], [row]).unwrap();
        assert!(authority.row(7).unwrap().launch_common_eligible());
        assert!(authority.row(7).unwrap().is_helicopter_runtime());
        assert_eq!(
            AirTypeAuthority::new(5, [0; 32], [row]),
            Err(AirRuntimeAuthorityError::MissingCompositionDigest)
        );
    }

    #[test]
    fn scenario_round_trip_keeps_list_order_duplicates_and_tombstones() {
        let mut before = ScenarioIgnoreOrdersAuthority {
            revision: 99,
            ignore_orders: true,
            ..ScenarioIgnoreOrdersAuthority::default()
        };
        before.ignored_by_owner[1] = vec![2_015, -1, 2_015];
        let bytes = encode_scenario_ignores(&before).unwrap();
        let after = decode_scenario_ignores(&bytes).unwrap();
        assert_eq!(after.revision, 0);
        assert!(after.ignore_orders);
        assert_eq!(after.ignored_by_owner, before.ignored_by_owner);
        assert_eq!(encode_scenario_ignores(&after).unwrap(), bytes);
    }

    #[test]
    fn scenario_boolean_is_canonical() {
        let mut bytes = encode_scenario_ignores(&ScenarioIgnoreOrdersAuthority::default()).unwrap();
        bytes[1] = 2;
        assert_eq!(
            decode_scenario_ignores(&bytes),
            Err(AirRuntimeAuthorityError::InvalidBoolean(2))
        );
    }
}
