//! Compact identities for the retail Declare/Accept command cohort.
//!
//! The recordings remain external under `ron-data/`.  These constants are derived
//! command counts, field histograms, and normalized-manifest identities only.

pub const DECLARE_OPCODE: u8 = 38;
pub const ACCEPT_OPCODE: u8 = 41;

pub const DECLARE_COMMANDS: usize = 101;
pub const ACCEPT_COMMANDS: usize = 1_415;
pub const DECLARE_WAR_COMMANDS: usize = 94;
pub const DECLARE_PEACE_COMMANDS: usize = 7;
pub const FILES_WITH_DECLARE: usize = 24;
pub const FILES_WITH_ACCEPT: usize = 26;

/// Normalized records are, in corpus/turn/player/command order:
///
/// ```text
/// <sha256 compressed recording><i32 lockstep serial><u32 simulation frame>
/// <i32 play><u16 wire length><complete command bytes>
/// ```
pub const MANIFEST_BYTES: usize = 83_784;
pub const MANIFEST_SHA256: &str =
    "92f255aa5e94bc503cfed8262ed47daeaa74d2819c654deefe5cd683151a97a9";
