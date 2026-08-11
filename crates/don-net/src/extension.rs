//! DoN transport extensions — packets that exist only between two copies of
//! **our** replacement `CrossplayNetLib.dll` / owned peer.
//!
//! # Why this namespace exists and why it is not in [`crate::internal`]
//!
//! [`crate::internal`] is the shipped netlib's own control plane, recovered from
//! `CrossplayNetLib.pdb`: nine types at ids 128..=136 with exact `sizeof`s. Not
//! one byte of it is ours to extend, and mixing an invented packet into that
//! enum would quietly destroy the claim that every id there is measured.
//!
//! This module is the opposite claim, stated out loud: **these packets are ours,
//! they are not retail, and no shipped code ever sends or parses one.** They are
//! carried on the same datagram transport, above the shipped internal range, and
//! [`crate::session::Session`] consumes them itself — exactly as it consumes the
//! shipped internal range — so they are never surfaced to the game layer and a
//! `riseofnations.exe` on the far end of our DLL never sees them.
//!
//! # The extensions that exist
//!
//! `CommandPackage::send` `0x0094c1e0` computes the multiplayer payload key
//! inline, six instructions before it calls the `NetSys` send slot:
//!
//! ```text
//! 0094c312  a1ec61c000   mov  eax, dword ptr [0xc061ec]   ; GameAccess::game  (Game*)
//! 0094c31d  8b4010       mov  eax, dword ptr [eax + 0x10]  ; Game::info(+12).seed(+4)
//! 0094c320  c1e808       shr  eax, 8
//! 0094c323  0fb7d8       movzx ebx, ax                     ; key = (u16)(seed >> 8)
//! ...
//! 0094c41c  ff5054       call dword ptr [eax + 0x54]       ; NetSys::send
//! 0094c42e  ff5058       call dword ptr [eax + 0x58]       ; NetSys::send_all
//! ```
//!
//! **[measured]** — instructions from `ron-bin/riseofnations.exe`; the two names
//! from `ron-bin/sbl/rise.pdb` (`public: static class Game &GameAccess::game` at
//! `0x00C061EC`; `Game::info` at offset 12, type `GameInfo`; `GameInfo::seed` at
//! offset 4). See `docs/assembly/multiplayer-command-package-key.md`.
//!
//! A peer joining an in-progress retail match cannot compute that value: the
//! seed is neither in the roster nor in any packet the netlib carries. Our shim
//! *is* inside the process that owns it, so the honest path is for the shim to
//! read it at exactly the moment retail used it and hand it over, rather than
//! for the joining peer to infer a key by ranking ciphertext.

/// First byte value reserved for DoN transport extensions.
///
/// Chosen above the whole shipped `InternalPacketType` range (128..=136) with a
/// deliberate gap, so a future shipped id can never collide with one of ours and
/// [`DonExtension::is_extension`] stays a total, decidable test.
pub const DON_EXT_BASE: u8 = 0xF0;

/// `DonExtension::GameKey`.
pub const DON_EXT_GAMEKEY: u8 = 0xF0;

/// `DonExtension::MatchStart`.
///
/// This is a DoN-owned control transaction, not a recovered retail packet.
/// `CrossplayProxy::ICrossPlayService::StartGame` is an asynchronous service
/// call, while [`crate::session::Session`] previously jumped directly from
/// all-ready to turn zero.  A local match needs an explicit, host-authoritative
/// handoff between those states, so this id carries it without pretending one
/// of retail's nine internal packet ids had room for it.
pub const DON_EXT_MATCH_START: u8 = 0xF1;

/// Exact wire length of a `GameKey` extension: type + `u32` + source.
pub const DON_EXT_GAMEKEY_LEN: usize = 6;

/// Exact wire length of a `MatchStart` extension: type + epoch + seed.
pub const DON_EXT_MATCH_START_LEN: usize = 9;

/// Where the announcing side got the key. This is provenance on the wire: the
/// receiver records which one it acted on instead of treating every key as
/// equally grounded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum GameKeySource {
    /// Read live from `*(u32*)(*(u32*)GameAccess::game + 0x10)` inside
    /// `riseofnations.exe`, after the retail identity gate rebased the address.
    RetailGameInfoSeed = 1,
    /// Supplied to the announcing transport by its operator (`DON_NET_GAME_KEY`).
    /// Used when the announcing process is not the pinned retail executable, so
    /// there is no live `Game` to read.
    Configured = 2,
}

impl GameKeySource {
    pub fn from_wire(value: u8) -> Option<Self> {
        match value {
            1 => Some(GameKeySource::RetailGameInfoSeed),
            2 => Some(GameKeySource::Configured),
            _ => None,
        }
    }

    pub fn to_wire(self) -> u8 {
        self as u8
    }

    pub fn as_str(self) -> &'static str {
        match self {
            GameKeySource::RetailGameInfoSeed => "retail-gameinfo-seed",
            GameKeySource::Configured => "configured",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DonExtension {
    /// The 32-bit `GameInfo::seed` of the match now in progress.
    ///
    /// The whole value is carried, not the derived `u16`, because the same word
    /// seeds both retail transforms: the XOR key is bits 8..23 and the
    /// inter-command pad generator is seeded with the full word (only its low 16
    /// bits can change a pad). See [`crate::obfuscate`].
    GameKey { seed: u32, source: GameKeySource },
    /// Begin one DoN-owned match after the complete roster is ready.
    ///
    /// `epoch` is a caller-owned, non-zero attempt identity. `seed` is carried
    /// in full because it is both simulation setup and the multiplayer command
    /// transform input.  The shape and authorization rules are **[DoN
    /// policy]**; no shipped service packet is claimed here.
    MatchStart { epoch: u32, seed: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtensionError {
    NotExtension(u8),
    UnknownType(u8),
    Short { id: u8, need: usize, have: usize },
    Trailing { id: u8, need: usize, have: usize },
    UnknownGameKeySource(u8),
}

impl core::fmt::Display for ExtensionError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ExtensionError::NotExtension(b) => {
                write!(f, "type {b} is below the DoN extension base {DON_EXT_BASE}")
            }
            ExtensionError::UnknownType(b) => write!(f, "no DoN extension type {b}"),
            ExtensionError::Short { id, need, have } => {
                write!(f, "DoN extension {id}: need {need}, have {have}")
            }
            ExtensionError::Trailing { id, need, have } => {
                write!(f, "DoN extension {id}: exact size {need}, have {have}")
            }
            ExtensionError::UnknownGameKeySource(value) => {
                write!(f, "DoN game-key source {value} is not a defined provenance")
            }
        }
    }
}

impl std::error::Error for ExtensionError {}

impl DonExtension {
    /// Total, cheap test used by the session dispatcher before it decodes.
    pub fn is_extension(first_byte: u8) -> bool {
        first_byte >= DON_EXT_BASE
    }

    pub fn id(&self) -> u8 {
        match self {
            DonExtension::GameKey { .. } => DON_EXT_GAMEKEY,
            DonExtension::MatchStart { .. } => DON_EXT_MATCH_START,
        }
    }

    pub fn wire_len(&self) -> usize {
        match self {
            DonExtension::GameKey { .. } => DON_EXT_GAMEKEY_LEN,
            DonExtension::MatchStart { .. } => DON_EXT_MATCH_START_LEN,
        }
    }

    pub fn decode(buf: &[u8]) -> Result<Self, ExtensionError> {
        let id = *buf.first().ok_or(ExtensionError::NotExtension(0))?;
        if !Self::is_extension(id) {
            return Err(ExtensionError::NotExtension(id));
        }
        let need = match id {
            DON_EXT_GAMEKEY => DON_EXT_GAMEKEY_LEN,
            DON_EXT_MATCH_START => DON_EXT_MATCH_START_LEN,
            other => return Err(ExtensionError::UnknownType(other)),
        };
        if buf.len() < need {
            return Err(ExtensionError::Short {
                id,
                need,
                have: buf.len(),
            });
        }
        if buf.len() > need {
            return Err(ExtensionError::Trailing {
                id,
                need,
                have: buf.len(),
            });
        }
        match id {
            DON_EXT_GAMEKEY => Ok(DonExtension::GameKey {
                seed: u32::from_le_bytes([buf[1], buf[2], buf[3], buf[4]]),
                source: GameKeySource::from_wire(buf[5])
                    .ok_or(ExtensionError::UnknownGameKeySource(buf[5]))?,
            }),
            DON_EXT_MATCH_START => Ok(DonExtension::MatchStart {
                epoch: u32::from_le_bytes([buf[1], buf[2], buf[3], buf[4]]),
                seed: u32::from_le_bytes([buf[5], buf[6], buf[7], buf[8]]),
            }),
            other => Err(ExtensionError::UnknownType(other)),
        }
    }

    pub fn encode(&self, out: &mut Vec<u8>) {
        out.push(self.id());
        match self {
            DonExtension::GameKey { seed, source } => {
                out.extend_from_slice(&seed.to_le_bytes());
                out.push(source.to_wire());
            }
            DonExtension::MatchStart { epoch, seed } => {
                out.extend_from_slice(&epoch.to_le_bytes());
                out.extend_from_slice(&seed.to_le_bytes());
            }
        }
    }
}

/// The bits of a game key that actually reach the wire transform.
///
/// `Obfuscation::xor_key` uses bits 8..23 and the pad generator's output depends
/// only on the low 16 bits of its seed, so two keys agreeing on bits 0..23
/// produce byte-identical packages. Ciphertext recovery can only ever return a
/// representative of this class — it cannot see bits 24..31 — which is why an
/// operator-supplied key is compared against an announced `GameInfo::seed` here
/// rather than by raw equality.
pub const GAME_KEY_WIRE_MASK: u32 = 0x00FF_FFFF;

/// Do these two keys drive the identical package transform?
pub fn game_keys_are_wire_equivalent(a: u32, b: u32) -> bool {
    a & GAME_KEY_WIRE_MASK == b & GAME_KEY_WIRE_MASK
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::internal::{InternalPacket, IPT_BASE, IPT_SIZE};
    use crate::obfuscate::{xor_payload, Obfuscation};

    #[test]
    fn the_extension_range_cannot_collide_with_a_shipped_internal_id() {
        // Every shipped InternalPacketType, and every id the shipped table could
        // grow into, stays below our base.
        assert!(DON_EXT_BASE > IPT_BASE);
        assert!((IPT_BASE as usize) + IPT_SIZE.len() <= DON_EXT_BASE as usize);
        for id in 0..=u8::MAX {
            if DonExtension::is_extension(id) {
                assert!(
                    InternalPacket::decode(&[id; 80]).is_err(),
                    "id {id} decodes as a shipped internal packet"
                );
            }
        }
    }

    #[test]
    fn game_key_round_trips_at_exactly_six_bytes_and_fails_closed() {
        for source in [GameKeySource::RetailGameInfoSeed, GameKeySource::Configured] {
            let packet = DonExtension::GameKey {
                seed: 0xDEAD_BEEF,
                source,
            };
            let mut wire = Vec::new();
            packet.encode(&mut wire);
            assert_eq!(wire.len(), DON_EXT_GAMEKEY_LEN);
            assert_eq!(wire.len(), packet.wire_len());
            assert_eq!(wire[0], DON_EXT_GAMEKEY);
            assert_eq!(DonExtension::decode(&wire), Ok(packet));

            let mut trailing = wire.clone();
            trailing.push(0);
            assert_eq!(
                DonExtension::decode(&trailing),
                Err(ExtensionError::Trailing {
                    id: DON_EXT_GAMEKEY,
                    need: 6,
                    have: 7
                })
            );
            assert_eq!(
                DonExtension::decode(&wire[..5]),
                Err(ExtensionError::Short {
                    id: DON_EXT_GAMEKEY,
                    need: 6,
                    have: 5
                })
            );
            let mut bad_source = wire.clone();
            bad_source[5] = 9;
            assert_eq!(
                DonExtension::decode(&bad_source),
                Err(ExtensionError::UnknownGameKeySource(9))
            );
        }
        assert_eq!(
            DonExtension::decode(&[0xF2, 0, 0, 0, 0, 0]),
            Err(ExtensionError::UnknownType(0xF2))
        );
        assert_eq!(
            DonExtension::decode(&[0x39; 6]),
            Err(ExtensionError::NotExtension(0x39))
        );
    }

    #[test]
    fn match_start_round_trips_at_exactly_nine_bytes_and_fails_closed() {
        let packet = DonExtension::MatchStart {
            epoch: 7,
            seed: 0x0d0a_11ce,
        };
        let mut wire = Vec::new();
        packet.encode(&mut wire);
        assert_eq!(wire.len(), DON_EXT_MATCH_START_LEN);
        assert_eq!(wire.len(), packet.wire_len());
        assert_eq!(wire[0], DON_EXT_MATCH_START);
        assert_eq!(DonExtension::decode(&wire), Ok(packet));

        let mut trailing = wire.clone();
        trailing.push(0);
        assert_eq!(
            DonExtension::decode(&trailing),
            Err(ExtensionError::Trailing {
                id: DON_EXT_MATCH_START,
                need: DON_EXT_MATCH_START_LEN,
                have: DON_EXT_MATCH_START_LEN + 1,
            })
        );
        assert_eq!(
            DonExtension::decode(&wire[..8]),
            Err(ExtensionError::Short {
                id: DON_EXT_MATCH_START,
                need: DON_EXT_MATCH_START_LEN,
                have: DON_EXT_MATCH_START_LEN - 1,
            })
        );
    }

    #[test]
    fn wire_equivalence_is_exactly_the_bits_the_transform_reads() {
        let seed = 0x1234_5678u32;
        for high in 0u32..=255 {
            let other = (seed & GAME_KEY_WIRE_MASK) | (high << 24);
            assert!(game_keys_are_wire_equivalent(seed, other));
            assert_eq!(Obfuscation::xor_key(seed), Obfuscation::xor_key(other));
            let mut a = vec![0u8; 32];
            let mut b = vec![0u8; 32];
            xor_payload(&mut a, Obfuscation::xor_key(seed));
            xor_payload(&mut b, Obfuscation::xor_key(other));
            assert_eq!(a, b);
            let mut ra = Obfuscation::multiplayer(seed);
            let mut rb = Obfuscation::multiplayer(other);
            for _ in 0..64 {
                assert_eq!(ra.next_pad(), rb.next_pad());
            }
        }
        // The mask is exactly bits 0..24: a difference anywhere inside it is not
        // absorbed, at either end of the range.
        assert!(!game_keys_are_wire_equivalent(seed, seed ^ 1));
        assert!(!game_keys_are_wire_equivalent(seed, seed ^ 0x0080_0000));
        // ...and it is not vacuous: the top byte really is invisible.
        assert!(game_keys_are_wire_equivalent(seed, seed ^ 0x0100_0000));
    }
}
