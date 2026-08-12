// SPDX-License-Identifier: GPL-3.0-or-later
//! Exclusive typed authority for the economy/containment order cohort.
//!
//! This is the lossless order-node and DoNSave-v13 leaf boundary used by the canonical
//! Board/Repair/Trade package host. It does not invent executable state for an unknown concrete
//! order: kind/payload mismatches, unknown tags and unknown versions all fail closed.

use crate::order::OrderIndex;
use crate::Handle;

pub const DON_SAVE_V13: u32 = 13;
pub const ECONOMY_PAYLOAD_VERSION: u8 = 1;
pub const RETAIL_OWNER_SLOTS: i32 = 10;

pub const TARGET_ORDER_WALKED_BYTES: usize = 11;
pub const GATHER_ORDER_WALKED_BYTES: usize = 31;
pub const CAST_ORDER_WALKED_BYTES: usize = 27;
pub const TRADE_ORDER_WALKED_BYTES: usize = 29;
pub const ORDER_LIST_NODE_PREFIX_BYTES: usize = 5;

pub const TARGET_NODE_WALKED_BYTES: usize =
    ORDER_LIST_NODE_PREFIX_BYTES + TARGET_ORDER_WALKED_BYTES;
pub const GATHER_NODE_WALKED_BYTES: usize =
    ORDER_LIST_NODE_PREFIX_BYTES + GATHER_ORDER_WALKED_BYTES;
pub const CAST_NODE_WALKED_BYTES: usize = ORDER_LIST_NODE_PREFIX_BYTES + CAST_ORDER_WALKED_BYTES;
pub const TRADE_NODE_WALKED_BYTES: usize = ORDER_LIST_NODE_PREFIX_BYTES + TRADE_ORDER_WALKED_BYTES;

pub const GATHER_SUFFIX_BYTES: usize = 20;
pub const CAST_SUFFIX_BYTES: usize = 8;
pub const TRADE_SUFFIX_BYTES: usize = 18;

/// The tags already reserved by the shared v12+ typed order envelope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum EconomyPayloadTag {
    None = 0,
    Gather = 2,
    CastSpell = 3,
    TradeRoute = 4,
}

impl EconomyPayloadTag {
    pub const fn from_raw(raw: u8) -> Option<Self> {
        match raw {
            0 => Some(Self::None),
            2 => Some(Self::Gather),
            3 => Some(Self::CastSpell),
            4 => Some(Self::TradeRoute),
            _ => None,
        }
    }

    pub const fn version(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Gather | Self::CastSpell | Self::TradeRoute => ECONOMY_PAYLOAD_VERSION,
        }
    }
}

/// Retail scalar identity plus the port's compaction-stable authority.
///
/// Unit-band addresses require a `Handle`. Build/Wall addresses are already bound by the
/// save-owned sparse object registry to `BuildRow`/`WallRow`, so they retain their exact retail
/// address and UID with no invented Unit handle. The exact no-target sentinel is also retained
/// because coordinate casts and not-yet-selected trade destinations legitimately carry it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StableTargetIdentity {
    pub o: i32,
    pub who: i32,
    pub uid: u16,
    pub handle: Option<Handle>,
}

impl StableTargetIdentity {
    pub const NONE: Self = Self {
        o: -1,
        who: -1,
        uid: u16::MAX,
        handle: None,
    };

    pub const fn live(o: i32, who: i32, uid: u16, handle: Handle) -> Self {
        Self {
            o,
            who,
            uid,
            handle: Some(handle),
        }
    }

    /// One canonical Build/Wall-band target. The sparse object registry, not a Unit Handle,
    /// owns its stable row identity.
    pub const fn banded(o: i32, who: i32, uid: u16) -> Self {
        Self {
            o,
            who,
            uid,
            handle: None,
        }
    }

    pub const fn is_live(self) -> bool {
        self.o >= 0 && self.who >= 0
    }

    fn validate(self) -> Result<(), EconomyOrderAuthorityError> {
        match (self.o >= 0, self.who >= 0, self.handle) {
            (true, true, Some(_)) => Ok(()),
            (true, true, None)
                if (2_000..=i16::MAX as i32).contains(&self.o)
                    && (0..RETAIL_OWNER_SLOTS).contains(&self.who) =>
            {
                Ok(())
            }
            (false, false, None) if self.o == -1 && self.who == -1 && self.uid == u16::MAX => {
                Ok(())
            }
            _ => Err(EconomyOrderAuthorityError::IncoherentTargetIdentity),
        }
    }
}

/// Exact twenty-byte suffix walked by `GatherOrder::walk_data` after `TargetOrder`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GatherOrderPayload {
    pub tx: i32,
    pub ty: i32,
    pub build_type: i32,
    pub wait: i32,
    pub goto_build: u8,
    pub non_flat_gather: u8,
    pub dist_mod: u8,
    pub been_there: u8,
}

/// Exact eight-byte suffix not already present in the flattened generic order header.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CastOrderPayload {
    pub paid: i32,
    pub spell: i32,
}

/// Exact second endpoint and mutable route history of `TradeOrder`.
///
/// `second.handle` is additive port identity and is deliberately saved even though it is not
/// part of retail's 29-byte checksum walk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TradeOrderPayload {
    pub second: StableTargetIdentity,
    pub started: i32,
    pub loaded: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EconomyOrderPayload {
    /// `BoardOrder`, `AwaitBoardOrder`, and `RepairOrder` are complete `TargetOrder`s.
    TargetOnly,
    Gather(GatherOrderPayload),
    CastSpell(CastOrderPayload),
    TradeRoute(TradeOrderPayload),
}

impl EconomyOrderPayload {
    pub const fn tag(self) -> EconomyPayloadTag {
        match self {
            Self::TargetOnly => EconomyPayloadTag::None,
            Self::Gather(_) => EconomyPayloadTag::Gather,
            Self::CastSpell(_) => EconomyPayloadTag::CastSpell,
            Self::TradeRoute(_) => EconomyPayloadTag::TradeRoute,
        }
    }
}

/// The part of a production `Order` which precedes the typed payload envelope.
///
/// The retail scalar widths stay `i32` here.  The current flattened order narrows target owner
/// and object indices; a future adapter must prove the conversion instead of silently truncating.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EconomyOrderHeader {
    pub kind: OrderIndex,
    pub flags: u8,
    pub x: i32,
    pub y: i32,
    pub primary: StableTargetIdentity,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EconomyOrderNode {
    /// The byte stored in `RecycledOrderNode::metric` and walked after the four-byte type.
    pub metric: u8,
    pub header: EconomyOrderHeader,
    pub payload: EconomyOrderPayload,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EconomyOrderAuthorityError {
    UnsupportedDoNSaveVersion(u32),
    UnsupportedOrderKind(OrderIndex),
    PayloadDoesNotMatchKind {
        kind: OrderIndex,
        tag: EconomyPayloadTag,
    },
    IncoherentTargetIdentity,
    HeaderTargetOutOfRange {
        o: i32,
        who: i32,
    },
    MissingRequiredPrimaryTarget(OrderIndex),
    UnknownPayloadTag(u8),
    UnknownPayloadVersion {
        tag: EconomyPayloadTag,
        version: u8,
    },
    ForeignPayloadTag {
        kind: OrderIndex,
        expected: EconomyPayloadTag,
        actual: EconomyPayloadTag,
    },
    InvalidHandlePresence(u8),
    WrongLeafSize {
        expected: usize,
        actual: usize,
    },
}

fn expected_payload_tag(kind: OrderIndex) -> Option<EconomyPayloadTag> {
    match kind {
        OrderIndex::BoardShip | OrderIndex::AwaitBoard | OrderIndex::Repair => {
            Some(EconomyPayloadTag::None)
        }
        OrderIndex::Gather => Some(EconomyPayloadTag::Gather),
        OrderIndex::CastSpell => Some(EconomyPayloadTag::CastSpell),
        OrderIndex::TradeRoute => Some(EconomyPayloadTag::TradeRoute),
        _ => None,
    }
}

impl EconomyOrderNode {
    pub fn validate(self) -> Result<(), EconomyOrderAuthorityError> {
        let Some(expected) = expected_payload_tag(self.header.kind) else {
            return Err(EconomyOrderAuthorityError::UnsupportedOrderKind(
                self.header.kind,
            ));
        };
        let actual = self.payload.tag();
        if actual != expected {
            return Err(EconomyOrderAuthorityError::PayloadDoesNotMatchKind {
                kind: self.header.kind,
                tag: actual,
            });
        }
        self.header.primary.validate()?;
        if matches!(
            self.header.kind,
            OrderIndex::BoardShip
                | OrderIndex::AwaitBoard
                | OrderIndex::Repair
                | OrderIndex::Gather
                | OrderIndex::TradeRoute
        ) && !self.header.primary.is_live()
        {
            return Err(EconomyOrderAuthorityError::MissingRequiredPrimaryTarget(
                self.header.kind,
            ));
        }
        if let EconomyOrderPayload::TradeRoute(payload) = self.payload {
            payload.second.validate()?;
        }
        Ok(())
    }

    pub const fn retail_walked_bytes(self) -> usize {
        match self.payload {
            EconomyOrderPayload::TargetOnly => TARGET_ORDER_WALKED_BYTES,
            EconomyOrderPayload::Gather(_) => GATHER_ORDER_WALKED_BYTES,
            EconomyOrderPayload::CastSpell(_) => CAST_ORDER_WALKED_BYTES,
            EconomyOrderPayload::TradeRoute(_) => TRADE_ORDER_WALKED_BYTES,
        }
    }

    /// Exact concrete virtual walk, excluding the surrounding order type and node metric.
    pub fn retail_walk_image(self) -> Result<Vec<u8>, EconomyOrderAuthorityError> {
        self.validate()?;
        let mut out = Vec::with_capacity(self.retail_walked_bytes());
        out.push(self.header.flags);
        push_primary_identity(&mut out, self.header.primary);
        match self.payload {
            EconomyOrderPayload::TargetOnly => {}
            EconomyOrderPayload::Gather(payload) => push_gather_suffix(&mut out, payload),
            EconomyOrderPayload::CastSpell(payload) => {
                out.extend_from_slice(&self.header.x.to_le_bytes());
                out.extend_from_slice(&self.header.y.to_le_bytes());
                out.extend_from_slice(&payload.paid.to_le_bytes());
                out.extend_from_slice(&payload.spell.to_le_bytes());
            }
            EconomyOrderPayload::TradeRoute(payload) => {
                push_trade_suffix(&mut out, payload);
            }
        }
        debug_assert_eq!(out.len(), self.retail_walked_bytes());
        Ok(out)
    }

    /// Exact `OrderList::walk_data` node image: `type:i32`, `metric:u8`, virtual payload.
    pub fn retail_node_image(self) -> Result<Vec<u8>, EconomyOrderAuthorityError> {
        let walk = self.retail_walk_image()?;
        let mut out = Vec::with_capacity(ORDER_LIST_NODE_PREFIX_BYTES + walk.len());
        out.extend_from_slice(&(self.header.kind as i32).to_le_bytes());
        out.push(self.metric);
        out.extend_from_slice(&walk);
        Ok(out)
    }
}

fn push_primary_identity(out: &mut Vec<u8>, identity: StableTargetIdentity) {
    out.extend_from_slice(&identity.o.to_le_bytes());
    out.extend_from_slice(&identity.who.to_le_bytes());
    out.extend_from_slice(&identity.uid.to_le_bytes());
}

fn push_gather_suffix(out: &mut Vec<u8>, payload: GatherOrderPayload) {
    out.extend_from_slice(&payload.tx.to_le_bytes());
    out.extend_from_slice(&payload.ty.to_le_bytes());
    out.extend_from_slice(&payload.build_type.to_le_bytes());
    out.extend_from_slice(&payload.wait.to_le_bytes());
    out.extend_from_slice(&[
        payload.goto_build,
        payload.non_flat_gather,
        payload.dist_mod,
        payload.been_there,
    ]);
}

fn push_trade_suffix(out: &mut Vec<u8>, payload: TradeOrderPayload) {
    out.extend_from_slice(&payload.second.o.to_le_bytes());
    out.extend_from_slice(&payload.second.who.to_le_bytes());
    out.extend_from_slice(&payload.started.to_le_bytes());
    out.extend_from_slice(&payload.loaded.to_le_bytes());
    out.extend_from_slice(&payload.second.uid.to_le_bytes());
}

fn require_v13(format_version: u32) -> Result<(), EconomyOrderAuthorityError> {
    // v13 introduced this leaf layout. Later additive root formats retain it byte-for-byte;
    // rejecting v14 here would make every existing Gather/Cast/Trade order unsaveable merely
    // because an unrelated top-level section was appended.
    if format_version >= DON_SAVE_V13 {
        Ok(())
    } else {
        Err(EconomyOrderAuthorityError::UnsupportedDoNSaveVersion(
            format_version,
        ))
    }
}

/// The two separated pieces one order node contributes to DoNSave v13.
///
/// `metric` must be written before the existing generic order record. `typed_payload` follows
/// that generic record at the already-reserved v12+ typed-envelope anchor. Keeping the pieces
/// separate avoids changing the placement of any v12 field while reserving metric ownership in
/// v13.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodedEconomyOrderLeaf {
    pub metric: u8,
    pub typed_payload: Vec<u8>,
}

/// Encode the merge-ready DoNSave-v13 node pieces.
///
/// Typed payload layout is `tag:u8, payload_version:u8, payload...`. Gather and Cast retain
/// their exact retail suffix. Trade appends a one-byte Handle presence marker and optional
/// eight-byte `Handle` after its exact 18-byte retail suffix; those additive bytes are not
/// submitted to the retail checksum walk.
pub fn encode_v13_leaf(
    format_version: u32,
    node: EconomyOrderNode,
) -> Result<EncodedEconomyOrderLeaf, EconomyOrderAuthorityError> {
    require_v13(format_version)?;
    node.validate()?;
    let tag = node.payload.tag();
    let mut out = vec![tag as u8, tag.version()];
    match node.payload {
        EconomyOrderPayload::TargetOnly => {}
        EconomyOrderPayload::Gather(payload) => push_gather_suffix(&mut out, payload),
        EconomyOrderPayload::CastSpell(payload) => {
            out.extend_from_slice(&payload.paid.to_le_bytes());
            out.extend_from_slice(&payload.spell.to_le_bytes());
        }
        EconomyOrderPayload::TradeRoute(payload) => {
            push_trade_suffix(&mut out, payload);
            match payload.second.handle {
                None => out.push(0),
                Some(handle) => {
                    out.push(1);
                    out.extend_from_slice(&handle.id.to_le_bytes());
                    out.extend_from_slice(&handle.generation.to_le_bytes());
                }
            }
        }
    }
    Ok(EncodedEconomyOrderLeaf {
        metric: node.metric,
        typed_payload: out,
    })
}

fn read_i32(bytes: &[u8], at: usize) -> i32 {
    i32::from_le_bytes(bytes[at..at + 4].try_into().expect("validated leaf size"))
}

fn read_u16(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(bytes[at..at + 2].try_into().expect("validated leaf size"))
}

fn read_u32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("validated leaf size"))
}

fn require_size(bytes: &[u8], expected: usize) -> Result<(), EconomyOrderAuthorityError> {
    if bytes.len() == expected {
        Ok(())
    } else {
        Err(EconomyOrderAuthorityError::WrongLeafSize {
            expected,
            actual: bytes.len(),
        })
    }
}

/// Decode one v13 leaf against the already-decoded generic order header.
///
/// `header.kind` is authoritative: a well-formed Gather body attached to a Repair order is
/// rejected instead of being retained as an opaque history or silently discarded.
pub fn decode_v13_leaf(
    format_version: u32,
    metric: u8,
    header: EconomyOrderHeader,
    bytes: &[u8],
) -> Result<EconomyOrderNode, EconomyOrderAuthorityError> {
    require_v13(format_version)?;
    if bytes.len() < 2 {
        return Err(EconomyOrderAuthorityError::WrongLeafSize {
            expected: 2,
            actual: bytes.len(),
        });
    }
    let tag = EconomyPayloadTag::from_raw(bytes[0])
        .ok_or(EconomyOrderAuthorityError::UnknownPayloadTag(bytes[0]))?;
    let Some(expected) = expected_payload_tag(header.kind) else {
        return Err(EconomyOrderAuthorityError::UnsupportedOrderKind(
            header.kind,
        ));
    };
    if tag != expected {
        return Err(EconomyOrderAuthorityError::ForeignPayloadTag {
            kind: header.kind,
            expected,
            actual: tag,
        });
    }
    if bytes[1] != tag.version() {
        return Err(EconomyOrderAuthorityError::UnknownPayloadVersion {
            tag,
            version: bytes[1],
        });
    }

    let payload = match tag {
        EconomyPayloadTag::None => {
            require_size(bytes, 2)?;
            EconomyOrderPayload::TargetOnly
        }
        EconomyPayloadTag::Gather => {
            require_size(bytes, 2 + GATHER_SUFFIX_BYTES)?;
            EconomyOrderPayload::Gather(GatherOrderPayload {
                tx: read_i32(bytes, 2),
                ty: read_i32(bytes, 6),
                build_type: read_i32(bytes, 10),
                wait: read_i32(bytes, 14),
                goto_build: bytes[18],
                non_flat_gather: bytes[19],
                dist_mod: bytes[20],
                been_there: bytes[21],
            })
        }
        EconomyPayloadTag::CastSpell => {
            require_size(bytes, 2 + CAST_SUFFIX_BYTES)?;
            EconomyOrderPayload::CastSpell(CastOrderPayload {
                paid: read_i32(bytes, 2),
                spell: read_i32(bytes, 6),
            })
        }
        EconomyPayloadTag::TradeRoute => {
            const FIXED: usize = 2 + TRADE_SUFFIX_BYTES + 1;
            if bytes.len() < FIXED {
                return Err(EconomyOrderAuthorityError::WrongLeafSize {
                    expected: FIXED,
                    actual: bytes.len(),
                });
            }
            let present = bytes[2 + TRADE_SUFFIX_BYTES];
            let (expected_size, handle) = match present {
                0 => (FIXED, None),
                1 => {
                    let expected = FIXED + 8;
                    if bytes.len() < expected {
                        return Err(EconomyOrderAuthorityError::WrongLeafSize {
                            expected,
                            actual: bytes.len(),
                        });
                    }
                    (
                        expected,
                        Some(Handle {
                            id: read_u32(bytes, FIXED),
                            generation: read_u32(bytes, FIXED + 4),
                        }),
                    )
                }
                raw => return Err(EconomyOrderAuthorityError::InvalidHandlePresence(raw)),
            };
            require_size(bytes, expected_size)?;
            EconomyOrderPayload::TradeRoute(TradeOrderPayload {
                second: StableTargetIdentity {
                    o: read_i32(bytes, 2),
                    who: read_i32(bytes, 6),
                    uid: read_u16(bytes, 18),
                    handle,
                },
                started: read_i32(bytes, 10),
                loaded: read_i32(bytes, 14),
            })
        }
    };
    let node = EconomyOrderNode {
        metric,
        header,
        payload,
    };
    node.validate()?;
    Ok(node)
}
