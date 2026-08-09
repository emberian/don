//! Shared telemetry contract for the rontoy Windows probe, host, UI, and advisor.
//!
//! The wire format is deliberately dependency-free and lossless for unknown fields.
//! See `docs/protocol.md` for compatibility rules and units.

mod model;
mod validate;
mod wire;

pub use model::*;
pub use validate::{Validate, ValidationError, ValidationLimits};
pub use wire::{
    decode_frame, encode_frame, DecodeError, EncodeError, WireLimits, HEADER_LEN, MAGIC,
};

/// Current protocol major. Readers must reject a larger major until they know
/// its compatibility rules; additions within this major use new TLV tags.
pub const PROTOCOL_MAJOR: u16 = 1;
pub const PROTOCOL_MINOR: u16 = 0;
