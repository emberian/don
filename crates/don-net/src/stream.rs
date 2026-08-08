//! Locating the command stream inside a decompressed `.rcx` payload.
//!
//! A `.rcx` is a `SaveGame`-serialised header, a state blob, then the command
//! stream to the last byte. Nothing points at the stream, so it is found
//! structurally: the unique longest chain of 18-byte-header records that tiles
//! exactly to EOF. This is the method from `docs/derivation/replay-stream.md`
//! §2, reimplemented here so the Rust corpus tests do not depend on the Python.

use crate::PackageHeader;

/// Result of locating the stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamLocation {
    pub start: usize,
    pub records: usize,
}

/// Find the command stream by backward dynamic programming.
///
/// A record at offset `o` is *good* iff `o + 18 + size` is good, with
/// `play < 8` and `stamp` non-decreasing across the link; `good[len]` is the
/// base case. Returns the start of the longest chain.
///
/// Deliberately **not** constrained on `valid == 0`: doing so silently swallows
/// the first package of a solo stream, which is the only solo package with
/// `valid == 1`.
pub fn find_stream(buf: &[u8]) -> Option<StreamLocation> {
    let n = buf.len();
    if n < PackageHeader::WIRE_LEN {
        return None;
    }
    let u16at = |o: usize| u16::from_le_bytes([buf[o], buf[o + 1]]) as usize;
    let u32at = |o: usize| u32::from_le_bytes(buf[o..o + 4].try_into().unwrap());

    // count[o] = chain length starting at o, 0 = not a valid chain start
    let mut count = vec![0u32; n + 1];
    let mut best = (0u32, usize::MAX);
    let mut o = n - PackageHeader::WIRE_LEN;
    loop {
        'body: {
            if u32at(o + 4) >= 8 {
                break 'body;
            }
            let ln = u16at(o + 16);
            if ln > 4000 {
                break 'body;
            }
            let next = o + PackageHeader::WIRE_LEN + ln;
            if next > n {
                break 'body;
            }
            let c = if next == n {
                1
            } else {
                if count[next] == 0 {
                    break 'body;
                }
                // stamp must not go backwards, and must not leap absurdly
                let (a, b) = (u32at(o), u32at(next));
                if b < a || b - a > 200 {
                    break 'body;
                }
                count[next] + 1
            };
            count[o] = c;
            if c > best.0 {
                best = (c, o);
            }
        }
        if o == 0 {
            break;
        }
        o -= 1;
    }
    if best.1 == usize::MAX {
        None
    } else {
        Some(StreamLocation { start: best.1, records: best.0 as usize })
    }
}
