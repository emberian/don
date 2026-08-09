use std::fmt;

use crate::model::*;

pub const MAGIC: [u8; 4] = *b"DONF";
pub const HEADER_LEN: usize = 56;

const WT_BYTES: u8 = 0;
const WT_U64: u8 = 1;
const WT_I64: u8 = 2;
const WT_TEXT: u8 = 3;
const WT_RECORD: u8 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WireLimits {
    pub max_frame_bytes: usize,
    pub max_record_bytes: usize,
    pub max_fields_per_record: usize,
    pub max_string_bytes: usize,
    pub max_blob_bytes: usize,
}

impl Default for WireLimits {
    fn default() -> Self {
        Self {
            max_frame_bytes: 16 * 1024 * 1024,
            max_record_bytes: 12 * 1024 * 1024,
            max_fields_per_record: 131_072,
            max_string_bytes: 256 * 1024,
            max_blob_bytes: 8 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecodeError {
    FrameTooLarge { actual: usize, limit: usize },
    BadMagic,
    Truncated(&'static str),
    TrailingBytes { expected: usize, actual: usize },
    RecordTooLarge { actual: usize, limit: usize },
    TooManyFields { limit: usize },
    StringTooLarge { actual: usize, limit: usize },
    BlobTooLarge { actual: usize, limit: usize },
    InvalidUtf8,
    IntegerWidth { tag: u16, actual: usize },
    IntegerOutOfRange { tag: u16, target: &'static str },
    UnsupportedMajor { actual: u16, supported: u16 },
    IntegrityMismatch { expected: u32, actual: u32 },
    DuplicateSingularField { tag: u16 },
    UnknownEnumValue { tag: u16, value: u64 },
    MissingRequiredField { tag: u16 },
    WrongWireType { tag: u16, actual: u8 },
    InvalidBoolean { tag: u16, value: u64 },
    ReservedHeader { value: u16 },
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for DecodeError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EncodeError {
    FrameTooLarge { actual: usize, limit: usize },
    RecordTooLarge { actual: usize, limit: usize },
    StringTooLarge { actual: usize, limit: usize },
    BlobTooLarge { actual: usize, limit: usize },
    LengthOverflow,
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for EncodeError {}

#[derive(Clone, Debug)]
struct Field {
    tag: u16,
    wire_type: u8,
    flags: u8,
    data: Vec<u8>,
}

impl From<Field> for UnknownField {
    fn from(value: Field) -> Self {
        Self {
            tag: value.tag,
            wire_type: value.wire_type,
            flags: value.flags,
            data: value.data,
        }
    }
}

fn put_field(
    out: &mut Vec<u8>,
    tag: u16,
    wire_type: u8,
    flags: u8,
    data: &[u8],
) -> Result<(), EncodeError> {
    let len = u32::try_from(data.len()).map_err(|_| EncodeError::LengthOverflow)?;
    out.extend_from_slice(&tag.to_le_bytes());
    out.push(wire_type);
    out.push(flags);
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(data);
    Ok(())
}

fn put_unknown(out: &mut Vec<u8>, field: &UnknownField) -> Result<(), EncodeError> {
    put_field(out, field.tag, field.wire_type, field.flags, &field.data)
}

fn put_u64(out: &mut Vec<u8>, tag: u16, value: u64) -> Result<(), EncodeError> {
    put_field(out, tag, WT_U64, 0, &value.to_le_bytes())
}

fn put_i64(out: &mut Vec<u8>, tag: u16, value: i64) -> Result<(), EncodeError> {
    put_field(out, tag, WT_I64, 0, &value.to_le_bytes())
}

fn put_bool(out: &mut Vec<u8>, tag: u16, value: bool) -> Result<(), EncodeError> {
    put_u64(out, tag, u64::from(value))
}

fn put_text(
    out: &mut Vec<u8>,
    tag: u16,
    value: &str,
    limits: WireLimits,
) -> Result<(), EncodeError> {
    if value.len() > limits.max_string_bytes {
        return Err(EncodeError::StringTooLarge {
            actual: value.len(),
            limit: limits.max_string_bytes,
        });
    }
    put_field(out, tag, WT_TEXT, 0, value.as_bytes())
}

fn put_bytes(
    out: &mut Vec<u8>,
    tag: u16,
    value: &[u8],
    limits: WireLimits,
) -> Result<(), EncodeError> {
    if value.len() > limits.max_blob_bytes {
        return Err(EncodeError::BlobTooLarge {
            actual: value.len(),
            limit: limits.max_blob_bytes,
        });
    }
    put_field(out, tag, WT_BYTES, 0, value)
}

fn put_record<T>(
    out: &mut Vec<u8>,
    tag: u16,
    value: &T,
    limits: WireLimits,
) -> Result<(), EncodeError>
where
    T: RecordCodec,
{
    let data = value.encode_record(limits)?;
    put_field(out, tag, WT_RECORD, 0, &data)
}

fn finish_record(out: Vec<u8>, limits: WireLimits) -> Result<Vec<u8>, EncodeError> {
    if out.len() > limits.max_record_bytes {
        Err(EncodeError::RecordTooLarge {
            actual: out.len(),
            limit: limits.max_record_bytes,
        })
    } else {
        Ok(out)
    }
}

fn parse_fields(
    data: &[u8],
    limits: WireLimits,
    singular_tags: &[u16],
    required_tags: &[u16],
) -> Result<Vec<Field>, DecodeError> {
    if data.len() > limits.max_record_bytes {
        return Err(DecodeError::RecordTooLarge {
            actual: data.len(),
            limit: limits.max_record_bytes,
        });
    }
    let mut cursor = 0usize;
    let mut fields = Vec::new();
    let mut seen_singular = Vec::new();
    while cursor < data.len() {
        if fields.len() >= limits.max_fields_per_record {
            return Err(DecodeError::TooManyFields {
                limit: limits.max_fields_per_record,
            });
        }
        if data.len() - cursor < 8 {
            return Err(DecodeError::Truncated("field header"));
        }
        let tag = u16::from_le_bytes([data[cursor], data[cursor + 1]]);
        if singular_tags.contains(&tag) && !seen_singular.contains(&tag) {
            seen_singular.push(tag);
        } else if singular_tags.contains(&tag) {
            return Err(DecodeError::DuplicateSingularField { tag });
        }
        let wire_type = data[cursor + 2];
        let flags = data[cursor + 3];
        let len = u32::from_le_bytes(data[cursor + 4..cursor + 8].try_into().unwrap()) as usize;
        cursor += 8;
        let end = cursor
            .checked_add(len)
            .ok_or(DecodeError::Truncated("field payload"))?;
        if end > data.len() {
            return Err(DecodeError::Truncated("field payload"));
        }
        fields.push(Field {
            tag,
            wire_type,
            flags,
            data: data[cursor..end].to_vec(),
        });
        cursor = end;
    }
    for &tag in required_tags {
        if !fields.iter().any(|field| field.tag == tag) {
            return Err(DecodeError::MissingRequiredField { tag });
        }
    }
    Ok(fields)
}

fn field_u64(field: &Field) -> Result<Option<u64>, DecodeError> {
    if field.wire_type != WT_U64 {
        return Ok(None);
    }
    if field.data.len() != 8 {
        return Err(DecodeError::IntegerWidth {
            tag: field.tag,
            actual: field.data.len(),
        });
    }
    Ok(Some(u64::from_le_bytes(
        field.data.as_slice().try_into().unwrap(),
    )))
}

fn require_u64_wire(field: &Field, known_tags: &[u16]) -> Result<(), DecodeError> {
    if known_tags.contains(&field.tag) && field.wire_type != WT_U64 {
        Err(DecodeError::WrongWireType {
            tag: field.tag,
            actual: field.wire_type,
        })
    } else {
        Ok(())
    }
}

fn field_i64(field: &Field) -> Result<Option<i64>, DecodeError> {
    if field.wire_type != WT_I64 {
        return Ok(None);
    }
    if field.data.len() != 8 {
        return Err(DecodeError::IntegerWidth {
            tag: field.tag,
            actual: field.data.len(),
        });
    }
    Ok(Some(i64::from_le_bytes(
        field.data.as_slice().try_into().unwrap(),
    )))
}

fn to_u8(value: u64, tag: u16) -> Result<u8, DecodeError> {
    u8::try_from(value).map_err(|_| DecodeError::IntegerOutOfRange { tag, target: "u8" })
}

fn to_u16(value: u64, tag: u16) -> Result<u16, DecodeError> {
    u16::try_from(value).map_err(|_| DecodeError::IntegerOutOfRange { tag, target: "u16" })
}

fn to_u32(value: u64, tag: u16) -> Result<u32, DecodeError> {
    u32::try_from(value).map_err(|_| DecodeError::IntegerOutOfRange { tag, target: "u32" })
}

fn to_bool(value: u64, tag: u16) -> Result<bool, DecodeError> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(DecodeError::InvalidBoolean { tag, value }),
    }
}

fn field_text(field: &Field, limits: WireLimits) -> Result<Option<String>, DecodeError> {
    if field.wire_type != WT_TEXT {
        return Ok(None);
    }
    if field.data.len() > limits.max_string_bytes {
        return Err(DecodeError::StringTooLarge {
            actual: field.data.len(),
            limit: limits.max_string_bytes,
        });
    }
    String::from_utf8(field.data.clone())
        .map(Some)
        .map_err(|_| DecodeError::InvalidUtf8)
}

fn field_bytes(field: &Field, limits: WireLimits) -> Result<Option<Vec<u8>>, DecodeError> {
    if field.wire_type != WT_BYTES {
        return Ok(None);
    }
    if field.data.len() > limits.max_blob_bytes {
        return Err(DecodeError::BlobTooLarge {
            actual: field.data.len(),
            limit: limits.max_blob_bytes,
        });
    }
    Ok(Some(field.data.clone()))
}

fn field_record<T: RecordCodec>(
    field: &Field,
    limits: WireLimits,
) -> Result<Option<T>, DecodeError> {
    if field.wire_type != WT_RECORD {
        return Ok(None);
    }
    T::decode_record(&field.data, limits).map(Some)
}

trait RecordCodec: Sized {
    fn encode_record(&self, limits: WireLimits) -> Result<Vec<u8>, EncodeError>;
    fn decode_record(data: &[u8], limits: WireLimits) -> Result<Self, DecodeError>;
}

/// Encode a frame. The returned vector is safe to prefix with a stream length or
/// send as one WebSocket binary message.
pub fn encode_frame(frame: &Frame, limits: WireLimits) -> Result<Vec<u8>, EncodeError> {
    let (kind, payload) = match &frame.message {
        Message::Hello(value) => (1, value.encode_record(limits)?),
        Message::Snapshot(value) => (2, value.encode_record(limits)?),
        Message::Events(value) => (3, value.encode_record(limits)?),
        Message::Heartbeat(value) => (4, value.encode_record(limits)?),
        Message::Unknown { kind, payload } => (*kind, payload.clone()),
    };
    if payload.len() > limits.max_record_bytes {
        return Err(EncodeError::RecordTooLarge {
            actual: payload.len(),
            limit: limits.max_record_bytes,
        });
    }
    let payload_len = u32::try_from(payload.len()).map_err(|_| EncodeError::LengthOverflow)?;
    let mut out = Vec::with_capacity(HEADER_LEN + payload.len());
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&frame.protocol_major.to_le_bytes());
    out.extend_from_slice(&frame.protocol_minor.to_le_bytes());
    out.extend_from_slice(&kind.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&frame.flags.to_le_bytes());
    out.extend_from_slice(&frame.sequence.to_le_bytes());
    out.extend_from_slice(&frame.sent_unix_ms.to_le_bytes());
    out.extend_from_slice(&frame.stream_id);
    out.extend_from_slice(&payload_len.to_le_bytes());
    let integrity = crc32_parts(&out, &payload);
    out.extend_from_slice(&integrity.to_le_bytes());
    debug_assert_eq!(out.len(), HEADER_LEN);
    out.extend_from_slice(&payload);
    if out.len() > limits.max_frame_bytes {
        return Err(EncodeError::FrameTooLarge {
            actual: out.len(),
            limit: limits.max_frame_bytes,
        });
    }
    Ok(out)
}

/// Decode exactly one complete frame. Unknown message kinds and record fields
/// are retained so protocol-aware relays do not destroy future data.
pub fn decode_frame(data: &[u8], limits: WireLimits) -> Result<Frame, DecodeError> {
    if data.len() > limits.max_frame_bytes {
        return Err(DecodeError::FrameTooLarge {
            actual: data.len(),
            limit: limits.max_frame_bytes,
        });
    }
    if data.len() < HEADER_LEN {
        return Err(DecodeError::Truncated("frame header"));
    }
    if data[..4] != MAGIC {
        return Err(DecodeError::BadMagic);
    }
    let protocol_major = u16::from_le_bytes(data[4..6].try_into().unwrap());
    if protocol_major != crate::PROTOCOL_MAJOR {
        return Err(DecodeError::UnsupportedMajor {
            actual: protocol_major,
            supported: crate::PROTOCOL_MAJOR,
        });
    }
    let protocol_minor = u16::from_le_bytes(data[6..8].try_into().unwrap());
    let kind = u16::from_le_bytes(data[8..10].try_into().unwrap());
    let reserved = u16::from_le_bytes(data[10..12].try_into().unwrap());
    if reserved != 0 {
        return Err(DecodeError::ReservedHeader { value: reserved });
    }
    let flags = u32::from_le_bytes(data[12..16].try_into().unwrap());
    let sequence = u64::from_le_bytes(data[16..24].try_into().unwrap());
    let sent_unix_ms = i64::from_le_bytes(data[24..32].try_into().unwrap());
    let stream_id = data[32..48].try_into().unwrap();
    let payload_len = u32::from_le_bytes(data[48..52].try_into().unwrap()) as usize;
    let expected_crc = u32::from_le_bytes(data[52..56].try_into().unwrap());
    let expected = HEADER_LEN
        .checked_add(payload_len)
        .ok_or(DecodeError::Truncated("payload"))?;
    if data.len() < expected {
        return Err(DecodeError::Truncated("payload"));
    }
    if data.len() != expected {
        return Err(DecodeError::TrailingBytes {
            expected,
            actual: data.len(),
        });
    }
    let payload = &data[HEADER_LEN..];
    let actual_crc = crc32_parts(&data[..52], payload);
    if actual_crc != expected_crc {
        return Err(DecodeError::IntegrityMismatch {
            expected: expected_crc,
            actual: actual_crc,
        });
    }
    let message = match kind {
        1 => Message::Hello(Hello::decode_record(payload, limits)?),
        2 => Message::Snapshot(Snapshot::decode_record(payload, limits)?),
        3 => Message::Events(EventBatch::decode_record(payload, limits)?),
        4 => Message::Heartbeat(Heartbeat::decode_record(payload, limits)?),
        other => Message::Unknown {
            kind: other,
            payload: payload.to_vec(),
        },
    };
    Ok(Frame {
        protocol_major,
        protocol_minor,
        flags,
        stream_id,
        sequence,
        sent_unix_ms,
        message,
    })
}

fn crc32_parts(header: &[u8], payload: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in header.iter().chain(payload) {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = 0u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

macro_rules! unknown_or {
    ($field:ident, $expr:expr, $unknown:expr) => {
        if !$expr {
            let _ = &$unknown;
            return Err(DecodeError::WrongWireType {
                tag: $field.tag,
                actual: $field.wire_type,
            });
        }
    };
}

impl RecordCodec for Hello {
    fn encode_record(&self, l: WireLimits) -> Result<Vec<u8>, EncodeError> {
        let mut o = vec![];
        put_text(&mut o, 1, &self.producer, l)?;
        put_text(&mut o, 2, &self.producer_version, l)?;
        for v in &self.capabilities {
            put_text(&mut o, 3, v, l)?;
        }
        for v in &self.unknown {
            put_unknown(&mut o, v)?;
        }
        finish_record(o, l)
    }
    fn decode_record(d: &[u8], l: WireLimits) -> Result<Self, DecodeError> {
        let mut x = Self::default();
        for f in parse_fields(d, l, &[1, 2], &[1, 2])? {
            match f.tag {
                1 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_text(&f, l)? {
                            x.producer = v;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                2 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_text(&f, l)? {
                            x.producer_version = v;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                3 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_text(&f, l)? {
                            x.capabilities.push(v);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                _ => x.unknown.push(f.into()),
            }
        }
        Ok(x)
    }
}

impl RecordCodec for SnapshotMeta {
    fn encode_record(&self, l: WireLimits) -> Result<Vec<u8>, EncodeError> {
        let mut o = vec![];
        put_i64(&mut o, 1, self.observed_unix_ms)?;
        put_u64(&mut o, 2, self.observed_monotonic_ns)?;
        if let Some(v) = self.game_frame {
            put_u64(&mut o, 3, v)?
        }
        if let Some(v) = self.sim_time_ms {
            put_u64(&mut o, 4, v)?
        }
        if let Some(v) = self.world_revision {
            put_u64(&mut o, 5, v)?
        }
        put_u64(&mut o, 6, self.coherence as u64)?;
        put_bool(&mut o, 7, self.partial)?;
        put_u64(&mut o, 8, self.dropped_since_previous as u64)?;
        if let Some(v) = self.sampled_frame_start {
            put_u64(&mut o, 9, v)?
        }
        if let Some(v) = self.sampled_frame_end {
            put_u64(&mut o, 10, v)?
        }
        put_u64(&mut o, 11, self.capture_duration_us as u64)?;
        put_u64(&mut o, 12, self.retry_count as u64)?;
        put_u64(&mut o, 13, self.capability_bits)?;
        put_u64(&mut o, 14, self.component_validity)?;
        put_u64(&mut o, 15, self.read_calls as u64)?;
        put_u64(&mut o, 16, self.bytes_read)?;
        put_u64(&mut o, 17, self.short_reads as u64)?;
        put_u64(&mut o, 18, self.decode_errors as u64)?;
        put_u64(&mut o, 19, self.game_mode as u64)?;
        for v in &self.unknown {
            put_unknown(&mut o, v)?
        }
        finish_record(o, l)
    }
    fn decode_record(d: &[u8], l: WireLimits) -> Result<Self, DecodeError> {
        let mut x = Self::default();
        for f in parse_fields(
            d,
            l,
            &[
                1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19,
            ],
            &[1, 2, 6, 7, 8, 11, 12, 13, 14, 15, 16, 17, 18, 19],
        )? {
            match f.tag {
                1 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_i64(&f)? {
                            x.observed_unix_ms = v;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                2 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.observed_monotonic_ns = v;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                3 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.game_frame = Some(v);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                4 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.sim_time_ms = Some(v);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                5 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.world_revision = Some(v);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                6 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.coherence =
                                Coherence::from_u64(v).ok_or(DecodeError::UnknownEnumValue {
                                    tag: f.tag,
                                    value: v,
                                })?;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                7 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.partial = to_bool(v, f.tag)?;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                8 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.dropped_since_previous = to_u32(v, f.tag)?;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                9 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.sampled_frame_start = Some(v);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                10 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.sampled_frame_end = Some(v);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                11 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.capture_duration_us = to_u32(v, f.tag)?;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                12 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.retry_count = to_u16(v, f.tag)?;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                13 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.capability_bits = v;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                14 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.component_validity = v;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                15 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.read_calls = to_u32(v, f.tag)?;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                16 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.bytes_read = v;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                17 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.short_reads = to_u32(v, f.tag)?;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                18 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.decode_errors = to_u32(v, f.tag)?;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                19 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.game_mode =
                                GameMode::from_u64(v).ok_or(DecodeError::UnknownEnumValue {
                                    tag: f.tag,
                                    value: v,
                                })?;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                _ => x.unknown.push(f.into()),
            }
        }
        Ok(x)
    }
}

impl RecordCodec for Source {
    fn encode_record(&self, l: WireLimits) -> Result<Vec<u8>, EncodeError> {
        let mut o = vec![];
        put_u64(&mut o, 1, self.kind as u64)?;
        put_text(&mut o, 2, &self.source_key, l)?;
        put_text(&mut o, 3, &self.build_id, l)?;
        if let Some(v) = self.process_id {
            put_u64(&mut o, 4, v as u64)?
        }
        if let Some(v) = self.capture_id {
            put_u64(&mut o, 5, v)?
        }
        put_text(&mut o, 6, &self.detail, l)?;
        if let Some(v) = self.process_start_id {
            put_u64(&mut o, 7, v)?;
        }
        if let Some(v) = self.image_base {
            put_u64(&mut o, 8, v)?;
        }
        for v in &self.unknown {
            put_unknown(&mut o, v)?
        }
        finish_record(o, l)
    }
    fn decode_record(d: &[u8], l: WireLimits) -> Result<Self, DecodeError> {
        let mut x = Self::default();
        for f in parse_fields(d, l, &[1, 2, 3, 4, 5, 6, 7, 8], &[1, 2, 3])? {
            match f.tag {
                1 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.kind =
                                SourceKind::from_u64(v).ok_or(DecodeError::UnknownEnumValue {
                                    tag: f.tag,
                                    value: v,
                                })?;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                2 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_text(&f, l)? {
                            x.source_key = v;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                3 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_text(&f, l)? {
                            x.build_id = v;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                4 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.process_id = Some(to_u32(v, f.tag)?);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                5 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.capture_id = Some(v);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                6 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_text(&f, l)? {
                            x.detail = v;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                7 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.process_start_id = Some(v);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                8 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.image_base = Some(v);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                _ => x.unknown.push(f.into()),
            }
        }
        Ok(x)
    }
}

impl RecordCodec for Evidence {
    fn encode_record(&self, l: WireLimits) -> Result<Vec<u8>, EncodeError> {
        let mut o = vec![];
        put_u64(&mut o, 1, self.source_id as u64)?;
        put_u64(&mut o, 2, self.confidence_bps as u64)?;
        put_u64(&mut o, 3, self.freshness_ms as u64)?;
        put_u64(&mut o, 4, self.flags as u64)?;
        put_text(&mut o, 5, &self.calibration_id, l)?;
        for v in &self.unknown {
            put_unknown(&mut o, v)?
        }
        finish_record(o, l)
    }
    fn decode_record(d: &[u8], l: WireLimits) -> Result<Self, DecodeError> {
        let mut x = Self::default();
        for f in parse_fields(d, l, &[1, 2, 3, 4, 5], &[1, 2, 3, 4, 5])? {
            match f.tag {
                1 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.source_id = to_u16(v, f.tag)?;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                2 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.confidence_bps = to_u16(v, f.tag)?;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                3 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.freshness_ms = to_u32(v, f.tag)?;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                4 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.flags = to_u32(v, f.tag)?;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                5 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_text(&f, l)? {
                            x.calibration_id = v;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                _ => x.unknown.push(f.into()),
            }
        }
        Ok(x)
    }
}

impl RecordCodec for Population {
    fn encode_record(&self, l: WireLimits) -> Result<Vec<u8>, EncodeError> {
        let mut o = vec![];
        put_u64(&mut o, 1, self.used as u64)?;
        put_u64(&mut o, 2, self.cap as u64)?;
        put_u64(&mut o, 3, self.queued as u64)?;
        if let Some(v) = self.citizens {
            put_u64(&mut o, 4, v as u64)?
        }
        if let Some(v) = self.military {
            put_u64(&mut o, 5, v as u64)?
        }
        for v in &self.unknown {
            put_unknown(&mut o, v)?
        }
        finish_record(o, l)
    }
    fn decode_record(d: &[u8], l: WireLimits) -> Result<Self, DecodeError> {
        let mut x = Self::default();
        for f in parse_fields(d, l, &[1, 2, 3, 4, 5], &[1, 2, 3])? {
            require_u64_wire(&f, &[1, 2, 3, 4, 5])?;
            let v = field_u64(&f)?;
            match (f.tag, v) {
                (1, Some(v)) => x.used = to_u32(v, f.tag)?,
                (2, Some(v)) => x.cap = to_u32(v, f.tag)?,
                (3, Some(v)) => x.queued = to_u32(v, f.tag)?,
                (4, Some(v)) => x.citizens = Some(to_u32(v, f.tag)?),
                (5, Some(v)) => x.military = Some(to_u32(v, f.tag)?),
                _ => x.unknown.push(f.into()),
            }
        }
        Ok(x)
    }
}

impl RecordCodec for Workforce {
    fn encode_record(&self, l: WireLimits) -> Result<Vec<u8>, EncodeError> {
        let mut o = vec![];
        put_u64(&mut o, 1, self.gathering as u64)?;
        put_u64(&mut o, 2, self.idle as u64)?;
        put_u64(&mut o, 3, self.building as u64)?;
        put_u64(&mut o, 4, self.scouting as u64)?;
        for v in &self.unknown {
            put_unknown(&mut o, v)?
        }
        finish_record(o, l)
    }
    fn decode_record(d: &[u8], l: WireLimits) -> Result<Self, DecodeError> {
        let mut x = Self::default();
        for f in parse_fields(d, l, &[1, 2, 3, 4], &[1, 2, 3, 4])? {
            require_u64_wire(&f, &[1, 2, 3, 4])?;
            let v = field_u64(&f)?;
            match (f.tag, v) {
                (1, Some(v)) => x.gathering = to_u32(v, f.tag)?,
                (2, Some(v)) => x.idle = to_u32(v, f.tag)?,
                (3, Some(v)) => x.building = to_u32(v, f.tag)?,
                (4, Some(v)) => x.scouting = to_u32(v, f.tag)?,
                _ => x.unknown.push(f.into()),
            }
        }
        Ok(x)
    }
}

impl RecordCodec for TypeCount {
    fn encode_record(&self, l: WireLimits) -> Result<Vec<u8>, EncodeError> {
        let mut o = vec![];
        put_u64(&mut o, 1, self.type_id as u64)?;
        put_u64(&mut o, 2, self.count as u64)?;
        put_bool(&mut o, 3, self.valid)?;
        for v in &self.unknown {
            put_unknown(&mut o, v)?;
        }
        finish_record(o, l)
    }

    fn decode_record(d: &[u8], l: WireLimits) -> Result<Self, DecodeError> {
        let mut x = Self::default();
        for f in parse_fields(d, l, &[1, 2, 3], &[1, 2, 3])? {
            require_u64_wire(&f, &[1, 2, 3])?;
            match (f.tag, field_u64(&f)?) {
                (1, Some(v)) => x.type_id = to_u32(v, f.tag)?,
                (2, Some(v)) => x.count = to_u32(v, f.tag)?,
                (3, Some(v)) => x.valid = to_bool(v, f.tag)?,
                _ => x.unknown.push(f.into()),
            }
        }
        Ok(x)
    }
}

impl RecordCodec for ResourceBalance {
    fn encode_record(&self, l: WireLimits) -> Result<Vec<u8>, EncodeError> {
        let mut o = vec![];
        put_u64(&mut o, 1, self.kind.code())?;
        if let Some(v) = self.stockpile_raw {
            put_i64(&mut o, 2, v)?;
        }
        if let Some(v) = self.income_sixteenths_per_period {
            put_i64(&mut o, 3, v)?
        }
        if let Some(v) = self.spend_sixteenths_per_period {
            put_i64(&mut o, 4, v)?
        }
        if let Some(v) = self.reserved_raw {
            put_i64(&mut o, 5, v)?
        }
        put_record(&mut o, 6, &self.evidence, l)?;
        put_u64(&mut o, 7, self.income_basis as u64)?;
        if let Some(v) = self.gross_sixteenths_per_period {
            put_i64(&mut o, 8, v)?;
        }
        if let Some(v) = self.support_sixteenths_per_period {
            put_i64(&mut o, 9, v)?;
        }
        if let Some(v) = self.bonus_sixteenths_per_period {
            put_i64(&mut o, 10, v)?;
        }
        if let Some(v) = self.leftover_sixteenths_per_period {
            put_i64(&mut o, 11, v)?;
        }
        if let Some(v) = self.resource_cap_raw {
            put_i64(&mut o, 12, v)?;
        }
        if let Some(v) = self.over_cap_status {
            put_u64(&mut o, 13, v as u64)?;
        }
        for v in &self.unknown {
            put_unknown(&mut o, v)?
        }
        finish_record(o, l)
    }
    fn decode_record(d: &[u8], l: WireLimits) -> Result<Self, DecodeError> {
        let mut x = Self::default();
        for f in parse_fields(
            d,
            l,
            &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13],
            &[1, 6, 7],
        )? {
            match f.tag {
                1 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.kind = ResourceKind::from_code(to_u16(v, f.tag)?);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                2 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_i64(&f)? {
                            x.stockpile_raw = Some(v);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                3 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_i64(&f)? {
                            x.income_sixteenths_per_period = Some(v);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                4 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_i64(&f)? {
                            x.spend_sixteenths_per_period = Some(v);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                5 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_i64(&f)? {
                            x.reserved_raw = Some(v);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                6 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_record(&f, l)? {
                            x.evidence = v;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                7 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.income_basis =
                                RateBasis::from_u64(v).ok_or(DecodeError::UnknownEnumValue {
                                    tag: f.tag,
                                    value: v,
                                })?;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                8 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_i64(&f)? {
                            x.gross_sixteenths_per_period = Some(v);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                9 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_i64(&f)? {
                            x.support_sixteenths_per_period = Some(v);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                10 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_i64(&f)? {
                            x.bonus_sixteenths_per_period = Some(v);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                11 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_i64(&f)? {
                            x.leftover_sixteenths_per_period = Some(v);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                12 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_i64(&f)? {
                            x.resource_cap_raw = Some(v);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                13 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.over_cap_status = Some(to_u8(v, f.tag)?);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                _ => x.unknown.push(f.into()),
            }
        }
        Ok(x)
    }
}

impl RecordCodec for PlayerEconomy {
    fn encode_record(&self, l: WireLimits) -> Result<Vec<u8>, EncodeError> {
        let mut o = vec![];
        put_u64(&mut o, 1, self.player_id as u64)?;
        put_text(&mut o, 2, &self.name, l)?;
        if let Some(v) = self.nation_type_id {
            put_u64(&mut o, 3, v as u64)?
        }
        if let Some(v) = self.age {
            put_u64(&mut o, 4, v as u64)?
        }
        for v in &self.resources {
            put_record(&mut o, 5, v, l)?
        }
        put_record(&mut o, 6, &self.population, l)?;
        put_record(&mut o, 7, &self.workers, l)?;
        put_record(&mut o, 8, &self.evidence, l)?;
        if let Some(v) = self.gather_stamp {
            put_u64(&mut o, 9, v)?;
        }
        if let Some(v) = self.gather_cache_age_frames {
            put_u64(&mut o, 10, v as u64)?;
        }
        if let Some(v) = self.city_count {
            put_u64(&mut o, 11, v as u64)?;
        }
        if let Some(v) = self.num_units {
            put_u64(&mut o, 12, v as u64)?;
        }
        if let Some(v) = self.num_buildings {
            put_u64(&mut o, 13, v as u64)?;
        }
        for v in &self.queued_type_counts {
            put_record(&mut o, 14, v, l)?;
        }
        for v in &self.unknown {
            put_unknown(&mut o, v)?
        }
        finish_record(o, l)
    }
    fn decode_record(d: &[u8], l: WireLimits) -> Result<Self, DecodeError> {
        let mut x = Self::default();
        for f in parse_fields(
            d,
            l,
            &[1, 2, 3, 4, 6, 7, 8, 9, 10, 11, 12, 13],
            &[1, 2, 6, 7, 8],
        )? {
            match f.tag {
                1 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.player_id = to_u8(v, f.tag)?;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                2 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_text(&f, l)? {
                            x.name = v;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                3 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.nation_type_id = Some(to_u32(v, f.tag)?);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                4 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.age = Some(to_u16(v, f.tag)?);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                5 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_record(&f, l)? {
                            x.resources.push(v);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                6 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_record(&f, l)? {
                            x.population = v;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                7 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_record(&f, l)? {
                            x.workers = v;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                8 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_record(&f, l)? {
                            x.evidence = v;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                9 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.gather_stamp = Some(v);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                10 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.gather_cache_age_frames = Some(to_u32(v, f.tag)?);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                11 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.city_count = Some(to_u32(v, f.tag)?);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                12 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.num_units = Some(to_u32(v, f.tag)?);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                13 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.num_buildings = Some(to_u32(v, f.tag)?);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                14 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_record(&f, l)? {
                            x.queued_type_counts.push(v);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                _ => x.unknown.push(f.into()),
            }
        }
        Ok(x)
    }
}

impl RecordCodec for QueueItem {
    fn encode_record(&self, l: WireLimits) -> Result<Vec<u8>, EncodeError> {
        let mut o = vec![];
        put_u64(&mut o, 1, self.type_id as u64)?;
        put_u64(&mut o, 2, self.count as u64)?;
        if let Some(v) = self.progress_ppm {
            put_u64(&mut o, 3, v as u64)?
        }
        if let Some(v) = self.eta_ms {
            put_u64(&mut o, 4, v)?
        }
        put_bool(&mut o, 5, self.paused)?;
        for v in &self.unknown {
            put_unknown(&mut o, v)?
        }
        finish_record(o, l)
    }
    fn decode_record(d: &[u8], l: WireLimits) -> Result<Self, DecodeError> {
        let mut x = Self::default();
        for f in parse_fields(d, l, &[1, 2, 3, 4, 5], &[1, 2, 5])? {
            require_u64_wire(&f, &[1, 2, 3, 4, 5])?;
            let v = field_u64(&f)?;
            match (f.tag, v) {
                (1, Some(v)) => x.type_id = to_u32(v, f.tag)?,
                (2, Some(v)) => x.count = to_u16(v, f.tag)?,
                (3, Some(v)) => x.progress_ppm = Some(to_u32(v, f.tag)?),
                (4, Some(v)) => x.eta_ms = Some(v),
                (5, Some(v)) => x.paused = to_bool(v, f.tag)?,
                _ => x.unknown.push(f.into()),
            }
        }
        Ok(x)
    }
}

impl RecordCodec for ProductionQueue {
    fn encode_record(&self, l: WireLimits) -> Result<Vec<u8>, EncodeError> {
        let mut o = vec![];
        put_u64(&mut o, 1, self.queue_id as u64)?;
        if let Some(v) = self.capacity {
            put_u64(&mut o, 2, v as u64)?
        }
        for v in &self.items {
            put_record(&mut o, 3, v, l)?
        }
        for v in &self.unknown {
            put_unknown(&mut o, v)?
        }
        finish_record(o, l)
    }
    fn decode_record(d: &[u8], l: WireLimits) -> Result<Self, DecodeError> {
        let mut x = Self::default();
        for f in parse_fields(d, l, &[1, 2], &[1])? {
            match f.tag {
                1 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.queue_id = to_u16(v, f.tag)?;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                2 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.capacity = Some(to_u16(v, f.tag)?);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                3 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_record(&f, l)? {
                            x.items.push(v);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                _ => x.unknown.push(f.into()),
            }
        }
        Ok(x)
    }
}

impl RecordCodec for Entity {
    fn encode_record(&self, l: WireLimits) -> Result<Vec<u8>, EncodeError> {
        let mut o = vec![];
        put_u64(&mut o, 1, self.object_id)?;
        put_u64(&mut o, 2, self.type_id as u64)?;
        if let Some(v) = self.owner_id {
            put_u64(&mut o, 3, v as u64)?
        }
        put_u64(&mut o, 4, self.kind as u64)?;
        put_i64(&mut o, 5, self.x_fine as i64)?;
        put_i64(&mut o, 6, self.y_fine as i64)?;
        if let Some(v) = self.hp_milli {
            put_u64(&mut o, 7, v as u64)?
        }
        if let Some(v) = self.hp_max_milli {
            put_u64(&mut o, 8, v as u64)?
        }
        if let Some(v) = self.build_progress_ppm {
            put_u64(&mut o, 9, v as u64)?
        }
        put_u64(&mut o, 10, self.state_flags)?;
        for v in &self.queues {
            put_record(&mut o, 11, v, l)?
        }
        put_record(&mut o, 12, &self.evidence, l)?;
        put_u64(&mut o, 13, self.visibility as u64)?;
        for v in &self.unknown {
            put_unknown(&mut o, v)?
        }
        finish_record(o, l)
    }
    fn decode_record(d: &[u8], l: WireLimits) -> Result<Self, DecodeError> {
        let mut x = Self::default();
        for f in parse_fields(
            d,
            l,
            &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 12, 13],
            &[1, 2, 4, 5, 6, 10, 12, 13],
        )? {
            match f.tag {
                1 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.object_id = v;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                2 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.type_id = to_u32(v, f.tag)?;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                3 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.owner_id = Some(to_u8(v, f.tag)?);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                4 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.kind =
                                EntityKind::from_u64(v).ok_or(DecodeError::UnknownEnumValue {
                                    tag: f.tag,
                                    value: v,
                                })?;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                5 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_i64(&f)? {
                            x.x_fine =
                                i32::try_from(v).map_err(|_| DecodeError::IntegerOutOfRange {
                                    tag: f.tag,
                                    target: "i32",
                                })?;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                6 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_i64(&f)? {
                            x.y_fine =
                                i32::try_from(v).map_err(|_| DecodeError::IntegerOutOfRange {
                                    tag: f.tag,
                                    target: "i32",
                                })?;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                7 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.hp_milli = Some(to_u32(v, f.tag)?);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                8 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.hp_max_milli = Some(to_u32(v, f.tag)?);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                9 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.build_progress_ppm = Some(to_u32(v, f.tag)?);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                10 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.state_flags = v;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                11 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_record(&f, l)? {
                            x.queues.push(v);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                12 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_record(&f, l)? {
                            x.evidence = v;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                13 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.visibility = VisibilityBasis::from_u64(v).ok_or(
                                DecodeError::UnknownEnumValue {
                                    tag: f.tag,
                                    value: v,
                                },
                            )?;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                _ => x.unknown.push(f.into()),
            }
        }
        Ok(x)
    }
}

impl RecordCodec for Warning {
    fn encode_record(&self, l: WireLimits) -> Result<Vec<u8>, EncodeError> {
        let mut o = vec![];
        put_u64(&mut o, 1, self.warning_id)?;
        put_text(&mut o, 2, &self.code, l)?;
        put_u64(&mut o, 3, self.severity as u64)?;
        put_text(&mut o, 4, &self.headline, l)?;
        put_text(&mut o, 5, &self.detail, l)?;
        if let Some(v) = self.player_id {
            put_u64(&mut o, 6, v as u64)?
        }
        if let Some(v) = self.object_id {
            put_u64(&mut o, 7, v)?
        }
        if let Some(v) = self.expires_game_frame {
            put_u64(&mut o, 8, v)?
        }
        put_record(&mut o, 9, &self.evidence, l)?;
        for v in &self.unknown {
            put_unknown(&mut o, v)?
        }
        finish_record(o, l)
    }
    fn decode_record(d: &[u8], l: WireLimits) -> Result<Self, DecodeError> {
        let mut x = Self::default();
        for f in parse_fields(d, l, &[1, 2, 3, 4, 5, 6, 7, 8, 9], &[1, 2, 3, 4, 5, 9])? {
            match f.tag {
                1 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.warning_id = v;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                2 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_text(&f, l)? {
                            x.code = v;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                3 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.severity =
                                Severity::from_u64(v).ok_or(DecodeError::UnknownEnumValue {
                                    tag: f.tag,
                                    value: v,
                                })?;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                4 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_text(&f, l)? {
                            x.headline = v;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                5 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_text(&f, l)? {
                            x.detail = v;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                6 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.player_id = Some(to_u8(v, f.tag)?);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                7 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.object_id = Some(v);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                8 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.expires_game_frame = Some(v);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                9 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_record(&f, l)? {
                            x.evidence = v;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                _ => x.unknown.push(f.into()),
            }
        }
        Ok(x)
    }
}

impl RecordCodec for Advice {
    fn encode_record(&self, l: WireLimits) -> Result<Vec<u8>, EncodeError> {
        let mut o = vec![];
        put_u64(&mut o, 1, self.advice_id)?;
        put_u64(&mut o, 2, self.priority_bps as u64)?;
        put_text(&mut o, 3, &self.headline, l)?;
        put_text(&mut o, 4, &self.rationale, l)?;
        for v in &self.actions {
            put_text(&mut o, 5, v, l)?
        }
        for v in &self.related_warning_ids {
            put_u64(&mut o, 6, *v)?
        }
        if let Some(v) = self.valid_until_game_frame {
            put_u64(&mut o, 7, v)?
        }
        put_record(&mut o, 8, &self.evidence, l)?;
        put_text(&mut o, 9, &self.rule_id, l)?;
        put_text(&mut o, 10, &self.rule_version, l)?;
        put_u64(&mut o, 11, self.lifecycle as u64)?;
        for v in &self.unknown {
            put_unknown(&mut o, v)?
        }
        finish_record(o, l)
    }
    fn decode_record(d: &[u8], l: WireLimits) -> Result<Self, DecodeError> {
        let mut x = Self::default();
        for f in parse_fields(
            d,
            l,
            &[1, 2, 3, 4, 7, 8, 9, 10, 11],
            &[1, 2, 3, 4, 8, 9, 10, 11],
        )? {
            match f.tag {
                1 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.advice_id = v;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                2 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.priority_bps = to_u16(v, f.tag)?;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                3 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_text(&f, l)? {
                            x.headline = v;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                4 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_text(&f, l)? {
                            x.rationale = v;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                5 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_text(&f, l)? {
                            x.actions.push(v);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                6 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.related_warning_ids.push(v);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                7 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.valid_until_game_frame = Some(v);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                8 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_record(&f, l)? {
                            x.evidence = v;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                9 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_text(&f, l)? {
                            x.rule_id = v;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                10 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_text(&f, l)? {
                            x.rule_version = v;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                11 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.lifecycle = AdviceLifecycle::from_u64(v).ok_or(
                                DecodeError::UnknownEnumValue {
                                    tag: f.tag,
                                    value: v,
                                },
                            )?;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                _ => x.unknown.push(f.into()),
            }
        }
        Ok(x)
    }
}

impl RecordCodec for HumanIdentity {
    fn encode_record(&self, l: WireLimits) -> Result<Vec<u8>, EncodeError> {
        let mut o = vec![];
        put_u64(&mut o, 1, self.player_id as u64)?;
        put_bool(&mut o, 2, self.confirmed_local)?;
        put_text(&mut o, 3, &self.identity_key, l)?;
        put_bool(&mut o, 4, self.active)?;
        for v in &self.unknown {
            put_unknown(&mut o, v)?;
        }
        finish_record(o, l)
    }

    fn decode_record(d: &[u8], l: WireLimits) -> Result<Self, DecodeError> {
        let mut x = Self::default();
        for f in parse_fields(d, l, &[1, 2, 3, 4], &[1, 2, 3, 4])? {
            match f.tag {
                1 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.player_id = to_u8(v, f.tag)?;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                2 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.confirmed_local = to_bool(v, f.tag)?;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                3 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_text(&f, l)? {
                            x.identity_key = v;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                4 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.active = to_bool(v, f.tag)?;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                _ => x.unknown.push(f.into()),
            }
        }
        Ok(x)
    }
}

impl RecordCodec for Snapshot {
    fn encode_record(&self, l: WireLimits) -> Result<Vec<u8>, EncodeError> {
        let mut o = vec![];
        put_record(&mut o, 1, &self.meta, l)?;
        for v in &self.sources {
            put_record(&mut o, 2, v, l)?
        }
        for v in &self.players {
            put_record(&mut o, 3, v, l)?
        }
        for v in &self.entities {
            put_record(&mut o, 4, v, l)?
        }
        for v in &self.warnings {
            put_record(&mut o, 5, v, l)?
        }
        for v in &self.advice {
            put_record(&mut o, 6, v, l)?
        }
        put_u64(&mut o, 7, self.scope as u64)?;
        if let Some(v) = &self.local_human {
            put_record(&mut o, 8, v, l)?;
        }
        for v in &self.unknown {
            put_unknown(&mut o, v)?
        }
        finish_record(o, l)
    }
    fn decode_record(d: &[u8], l: WireLimits) -> Result<Self, DecodeError> {
        let mut x = Self::default();
        for f in parse_fields(d, l, &[1, 7, 8], &[1, 7])? {
            match f.tag {
                1 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_record(&f, l)? {
                            x.meta = v;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                2 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_record(&f, l)? {
                            x.sources.push(v);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                3 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_record(&f, l)? {
                            x.players.push(v);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                4 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_record(&f, l)? {
                            x.entities.push(v);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                5 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_record(&f, l)? {
                            x.warnings.push(v);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                6 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_record(&f, l)? {
                            x.advice.push(v);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                7 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.scope = ObservationScope::from_u64(v).ok_or(
                                DecodeError::UnknownEnumValue {
                                    tag: f.tag,
                                    value: v,
                                },
                            )?;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                8 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_record(&f, l)? {
                            x.local_human = Some(v);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                _ => x.unknown.push(f.into()),
            }
        }
        Ok(x)
    }
}

impl RecordCodec for Event {
    fn encode_record(&self, l: WireLimits) -> Result<Vec<u8>, EncodeError> {
        let mut o = vec![];
        put_u64(&mut o, 1, self.event_id)?;
        if let Some(v) = self.game_frame {
            put_u64(&mut o, 2, v)?
        }
        put_text(&mut o, 3, &self.kind, l)?;
        if let Some(v) = self.player_id {
            put_u64(&mut o, 4, v as u64)?
        }
        if let Some(v) = self.object_id {
            put_u64(&mut o, 5, v)?
        }
        put_bytes(&mut o, 6, &self.payload, l)?;
        put_record(&mut o, 7, &self.evidence, l)?;
        for v in &self.unknown {
            put_unknown(&mut o, v)?
        }
        finish_record(o, l)
    }
    fn decode_record(d: &[u8], l: WireLimits) -> Result<Self, DecodeError> {
        let mut x = Self::default();
        for f in parse_fields(d, l, &[1, 2, 3, 4, 5, 6, 7], &[1, 3, 6, 7])? {
            match f.tag {
                1 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.event_id = v;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                2 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.game_frame = Some(v);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                3 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_text(&f, l)? {
                            x.kind = v;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                4 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.player_id = Some(to_u8(v, f.tag)?);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                5 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.object_id = Some(v);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                6 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_bytes(&f, l)? {
                            x.payload = v;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                7 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_record(&f, l)? {
                            x.evidence = v;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                _ => x.unknown.push(f.into()),
            }
        }
        Ok(x)
    }
}

impl RecordCodec for EventBatch {
    fn encode_record(&self, l: WireLimits) -> Result<Vec<u8>, EncodeError> {
        let mut o = vec![];
        if let Some(v) = self.base_game_frame {
            put_u64(&mut o, 1, v)?
        }
        for v in &self.events {
            put_record(&mut o, 2, v, l)?
        }
        for v in &self.unknown {
            put_unknown(&mut o, v)?
        }
        finish_record(o, l)
    }
    fn decode_record(d: &[u8], l: WireLimits) -> Result<Self, DecodeError> {
        let mut x = Self::default();
        for f in parse_fields(d, l, &[1], &[])? {
            match f.tag {
                1 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.base_game_frame = Some(v);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                2 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_record(&f, l)? {
                            x.events.push(v);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                _ => x.unknown.push(f.into()),
            }
        }
        Ok(x)
    }
}

impl RecordCodec for Heartbeat {
    fn encode_record(&self, l: WireLimits) -> Result<Vec<u8>, EncodeError> {
        let mut o = vec![];
        if let Some(v) = self.last_snapshot_sequence {
            put_u64(&mut o, 1, v)?
        }
        put_text(&mut o, 2, &self.producer_state, l)?;
        for v in &self.unknown {
            put_unknown(&mut o, v)?
        }
        finish_record(o, l)
    }
    fn decode_record(d: &[u8], l: WireLimits) -> Result<Self, DecodeError> {
        let mut x = Self::default();
        for f in parse_fields(d, l, &[1, 2], &[2])? {
            match f.tag {
                1 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_u64(&f)? {
                            x.last_snapshot_sequence = Some(v);
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                2 => unknown_or!(
                    f,
                    {
                        if let Some(v) = field_text(&f, l)? {
                            x.producer_state = v;
                            true
                        } else {
                            false
                        }
                    },
                    x.unknown
                ),
                _ => x.unknown.push(f.into()),
            }
        }
        Ok(x)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_narrowing_never_wraps() {
        let mut bytes = Vec::new();
        put_u64(&mut bytes, 1, 0).unwrap();
        put_u64(&mut bytes, 2, 65_536).unwrap();
        put_u64(&mut bytes, 3, 0).unwrap();
        put_u64(&mut bytes, 4, 0).unwrap();
        put_text(&mut bytes, 5, "test", WireLimits::default()).unwrap();
        let error = Evidence::decode_record(&bytes, WireLimits::default()).unwrap_err();
        assert_eq!(
            error,
            DecodeError::IntegerOutOfRange {
                tag: 2,
                target: "u16"
            }
        );
    }

    #[test]
    fn future_known_enum_value_is_rejected_not_rewritten() {
        let mut bytes = Vec::new();
        put_i64(&mut bytes, 1, 0).unwrap();
        put_u64(&mut bytes, 2, 0).unwrap();
        put_u64(&mut bytes, 6, 99).unwrap();
        put_bool(&mut bytes, 7, false).unwrap();
        put_u64(&mut bytes, 8, 0).unwrap();
        for tag in 11..=18 {
            put_u64(&mut bytes, tag, 0).unwrap();
        }
        put_u64(&mut bytes, 19, GameMode::SinglePlayer as u64).unwrap();
        let error = SnapshotMeta::decode_record(&bytes, WireLimits::default()).unwrap_err();
        assert_eq!(error, DecodeError::UnknownEnumValue { tag: 6, value: 99 });
    }

    #[test]
    fn omitted_required_and_wrong_wire_types_are_rejected() {
        assert_eq!(
            Hello::decode_record(&[], WireLimits::default()).unwrap_err(),
            DecodeError::MissingRequiredField { tag: 1 }
        );

        let mut wrong = Vec::new();
        put_u64(&mut wrong, 1, 7).unwrap();
        put_text(&mut wrong, 2, "1", WireLimits::default()).unwrap();
        assert_eq!(
            Hello::decode_record(&wrong, WireLimits::default()).unwrap_err(),
            DecodeError::WrongWireType {
                tag: 1,
                actual: WT_U64
            }
        );
    }

    #[test]
    fn booleans_accept_only_zero_or_one() {
        let mut bytes = Vec::new();
        put_u64(&mut bytes, 1, 3).unwrap();
        put_u64(&mut bytes, 2, 4).unwrap();
        put_u64(&mut bytes, 3, 2).unwrap();
        assert_eq!(
            TypeCount::decode_record(&bytes, WireLimits::default()).unwrap_err(),
            DecodeError::InvalidBoolean { tag: 3, value: 2 }
        );
    }
}
