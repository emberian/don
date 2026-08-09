//! Typed validation for the retail multiplayer checksum package carried by a
//! [`TurnPackage`](crate::session::TurnPackage).
//!
//! The session layer deliberately preserves command payloads opaquely. This
//! module is the shared boundary that applies retail's per-package XOR/padding
//! transform and extracts the exact 65-byte opcode-`0x39` checksum oracle.

use crate::obfuscate::xor_payload;
use crate::session::TurnPackage;
use crate::{decode_commands, CheckSums, Error, Obfuscation};

pub const CHECKSUM_OPCODE: u8 = 0x39;
pub const COMMAND_PACKAGE_HEADER_LEN: usize = 8;
pub const CHECKSUM_ONLY_PACKET_LEN: usize = COMMAND_PACKAGE_HEADER_LEN + CheckSums::WIRE_LEN;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedRetailChecksum {
    pub stamp: u32,
    pub play: i8,
    pub command_count: usize,
    pub opcodes: Vec<u8>,
    pub checksums: CheckSums,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetailChecksumError {
    PayloadTooLarge(usize),
    CommandDecode(Error),
    MissingChecksum,
    MultipleChecksums,
    InvalidChecksumTotal { expected: u32, actual: u32 },
}

impl core::fmt::Display for RetailChecksumError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::PayloadTooLarge(len) => {
                write!(f, "retail command payload is {len} bytes; maximum is 512")
            }
            Self::CommandDecode(error) => write!(f, "retail command decode: {error}"),
            Self::MissingChecksum => write!(f, "retail package has no checksum command"),
            Self::MultipleChecksums => {
                write!(f, "retail package has more than one checksum command")
            }
            Self::InvalidChecksumTotal { expected, actual } => write!(
                f,
                "retail checksum total is {actual:#010x}; expected {expected:#010x}"
            ),
        }
    }
}

impl std::error::Error for RetailChecksumError {}

/// Decode one package exactly as `CommandPackage::process_all` does in a
/// multiplayer game. The obfuscation state restarts from `game_key` for every
/// package; checksum absence or duplication is refused rather than inferred.
pub fn decode_retail_checksum_package(
    package: &TurnPackage,
    game_key: u32,
) -> Result<DecodedRetailChecksum, RetailChecksumError> {
    if package.payload.len() > 512 {
        return Err(RetailChecksumError::PayloadTooLarge(package.payload.len()));
    }
    let mut plain = package.payload.clone();
    xor_payload(&mut plain, Obfuscation::xor_key(game_key));
    let mut obfuscation = Obfuscation::multiplayer(game_key);
    let commands =
        decode_commands(&plain, &mut obfuscation).map_err(RetailChecksumError::CommandDecode)?;
    let mut checksums = None;
    let opcodes = commands.iter().map(|command| command.opcode).collect();
    for command in &commands {
        if command.opcode != CHECKSUM_OPCODE {
            continue;
        }
        let decoded = CheckSums::decode(command).ok_or(RetailChecksumError::MissingChecksum)?;
        let expected = decoded.0[..15]
            .iter()
            .fold(0u32, |sum, value| sum.wrapping_add(*value));
        if decoded.0[15] != expected {
            return Err(RetailChecksumError::InvalidChecksumTotal {
                expected,
                actual: decoded.0[15],
            });
        }
        if checksums.replace(decoded).is_some() {
            return Err(RetailChecksumError::MultipleChecksums);
        }
    }
    Ok(DecodedRetailChecksum {
        stamp: package.stamp,
        play: package.play,
        command_count: commands.len(),
        opcodes,
        checksums: checksums.ok_or(RetailChecksumError::MissingChecksum)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{encode_commands, Command};

    fn package(words: [u32; 16], game_key: u32) -> TurnPackage {
        let mut checksum = vec![CHECKSUM_OPCODE];
        for word in words {
            checksum.extend_from_slice(&word.to_le_bytes());
        }
        let command = Command {
            opcode: CHECKSUM_OPCODE,
            bytes: &checksum,
        };
        let mut payload = Vec::new();
        encode_commands(
            &[command],
            &mut Obfuscation::multiplayer(game_key),
            &mut payload,
        );
        xor_payload(&mut payload, Obfuscation::xor_key(game_key));
        TurnPackage {
            stamp: 1,
            play: 0,
            payload,
        }
    }

    #[test]
    fn invalid_checksum_total_is_refused() {
        let game_key = 0x005a_c33d;
        let bad = package([1; 16], game_key);
        assert_eq!(
            decode_retail_checksum_package(&bad, game_key),
            Err(RetailChecksumError::InvalidChecksumTotal {
                expected: 15,
                actual: 1,
            })
        );
    }

    #[test]
    fn payload_over_retail_command_storage_is_refused() {
        let oversized = TurnPackage {
            stamp: 1,
            play: 0,
            payload: vec![0; 513],
        };
        assert_eq!(
            decode_retail_checksum_package(&oversized, 0),
            Err(RetailChecksumError::PayloadTooLarge(513))
        );
    }
}
