//! Compact derived metadata for one retail Group -> MoveTo cohort.
//!
//! This is intentionally not a recording: the `.rcx` and `.SVX` artifacts stay under
//! `ron-data/`.  The constants below are content identities, normalized-cohort digests,
//! and five small decoded package fixtures selected to cover distinct wire shapes.

pub const REPLAY_RELATIVE_PATH: &str = "ron-data/replays/Playback - 2026.08.11 11'44'38 (Tue).rcx";
pub const REPLAY_FILE_SHA256: &str =
    "558e0cd53dbed4f820e8757c0327a58384d433e5beb8e9eeef5d64df4d67bd54";
pub const REPLAY_PLAIN_SHA256: &str =
    "5dbef00c8283ba213e929bd01df6f9cffafcf6b8d3beaa0fbff90f74348b2927";
pub const REPLAY_SEED: u32 = 0x007f_93e0;
pub const REPLAY_STREAM_OFFSET: usize = 1_025_163;
pub const REPLAY_PACKAGES: usize = 60_402;
pub const REPLAY_COMMANDS: usize = 68_811;
pub const GROUP_COMMANDS: usize = 675;
pub const MOVE_TO_COMMANDS: usize = 53;

/// All 53 records, in chronological order:
///
/// ```text
/// <u32 package_index0><u32 frame><i32 package_serial>
/// <u16 group_len><Group bytes><u16 move_len><MoveTo bytes>
/// ```
pub const PAIR_MANIFEST_BYTES: usize = 2_753;
pub const PAIR_MANIFEST_SHA256: &str =
    "a6da97b0cfa65677e46520b7be7c67152dcef93e05504e87da606bfce812b48f";

/// The same 53 packages as `<u16 payload_len><complete decoded payload>`, in order.
pub const PACKAGE_MANIFEST_BYTES: usize = 2_140;
pub const PACKAGE_MANIFEST_SHA256: &str =
    "f14a3c09b04b4df3f375989fe6f0746d153cc73131c3fd8f34bf38b48de0765c";

#[derive(Clone, Copy, Debug)]
pub struct PackageFixture {
    pub package_index0: u32,
    pub frame: u32,
    pub serial: i32,
    pub opcodes: &'static [u8],
    pub payload_hex: &'static str,
    pub payload_sha256: &'static str,
}

pub const PACKAGE_FIXTURES: [PackageFixture; 5] = [
    // A Camera and PlayerSpeed prefix must not clear later Group scratch.
    PackageFixture {
        package_index0: 6_123,
        frame: 6_105,
        serial: 6_124,
        opcodes: &[72, 79, 0, 7],
        payload_hex: "48040436000061c100004f00080000000000000001000200070d2600004eb9000000000000000000000102003200",
        payload_sha256: "00d499703ec9eadf40ded67a7eea2047d71badded59fb6d410b456f378aa720e",
    },
    // Empty Group means UID-guarded reuse of the last explicit selection.
    PackageFixture {
        package_index0: 6_145,
        frame: 6_126,
        serial: 6_146,
        opcodes: &[0, 7],
        payload_hex: "00000007303b0000d9c1000000000000000000000102003200",
        payload_sha256: "a2f972475cef6445fcb4e6657ffefb0fbbb66fbf248bbef5bb56b201b2d70f01",
    },
    // The only non-default orders/queue/form/width row in this recording.
    PackageFixture {
        package_index0: 38_933,
        frame: 38_744,
        serial: 38_934,
        opcodes: &[0, 7],
        payload_hex: "00000007e2220000cd74000000000000000000000201ffff00",
        payload_sha256: "f5c70a69a41f2804d9435468d27036e066b824956b740ffcd85cc498fde7f970",
    },
    // A 26-member owner-local selection with an explicit facing angle.
    PackageFixture {
        package_index0: 41_790,
        frame: 41_527,
        serial: 41_791,
        opcodes: &[0, 7],
        payload_hex: "001a000a0050000e0040006100960055007800880091009900a300a800900095003c0054005c00830085008e009c00a00086008f009400070b2100009f4d0000010000000000ee000102003200",
        payload_sha256: "09c80767e94eb428f30c8665dc6aab108a557817106be4dd8e40c71b3014dbe6",
    },
    // Minimal explicit singleton package: the direct Web/canonical-host acceptance case.
    PackageFixture {
        package_index0: 6_283,
        frame: 6_263,
        serial: 6_284,
        opcodes: &[0, 7],
        payload_hex: "0001000100074bb900007eb9000000000000000000000102003200",
        payload_sha256: "052db053c3ddb72e341dff12f01c185e7eacb80b6e51fa1463eda2482d036600",
    },
];

pub const SAVE_RELATIVE_PATH: &str =
    "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX";
pub const SAVE_FILE_SHA256: &str =
    "161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7";
pub const SAVE_PLAIN_SHA256: &str =
    "fa31f89f9bf435e28847e9e54d62cfe712e6bafc6cedb9ab98c8ab6730adb7c8";
pub const SAVE_PLAIN_BYTES: usize = 2_923_161;
pub const SAVE_SEED: u32 = 0x0148_10ac;
pub const SAVE_FRAME: u32 = 1_199;
pub const SAVE_FRAME_OFFSET: usize = 0x4d3;

pub const GROUPS_OFFSET: usize = 0x4610b;
pub const GROUPS_ELEMENTS_OFFSET: usize = 0x46116;
pub const GROUPS_RECORDS_END: usize = 0x4f1fa;
pub const GROUPS_LAST_GROUP_OFFSET: usize = 0x4f1fb;
pub const GROUPS_PROC_GROUP_OFFSET: usize = 0x4f21b;
pub const GROUPS_END: usize = 0x4f21f;
pub const GROUPS_RECORDS_SHA256: &str =
    "5555f94c193c9df5838e1daf2a125e3fe2d4b1ad5acbba721ed2d54182c40926";
pub const GROUPS_SECTION_SHA256: &str =
    "401ae136297c2028c0a21b72cd7a26b768a0910c556e6958eb41cc32782faf17";
pub const LAST_GROUP: [i32; 8] = [4, 65, 130, 192, 256, 320, 384, 448];
pub const PROC_GROUP: i32 = 47;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LiveGroup {
    pub slot: i32,
    pub member: i16,
    pub form: i32,
    pub stamp: i32,
    pub order_num: i32,
    pub role: i32,
    pub form_num: i32,
    pub buildings: u8,
    pub who: u8,
}

pub const LIVE_GROUPS: [LiveGroup; 12] = [
    LiveGroup {
        slot: 0,
        member: 1,
        form: 0,
        stamp: 0,
        order_num: 1,
        role: 262_400,
        form_num: 1,
        buildings: 0,
        who: 0,
    },
    LiveGroup {
        slot: 1,
        member: 16,
        form: 0,
        stamp: 1_055,
        order_num: 1,
        role: 524_544,
        form_num: 1,
        buildings: 0,
        who: 0,
    },
    LiveGroup {
        slot: 2,
        member: 0,
        form: 0,
        stamp: 1_087,
        order_num: 1,
        role: 262_416,
        form_num: 1,
        buildings: 0,
        who: 0,
    },
    LiveGroup {
        slot: 4,
        member: 14,
        form: 0,
        stamp: 1_145,
        order_num: 1,
        role: 599_056,
        form_num: 1,
        buildings: 0,
        who: 0,
    },
    LiveGroup {
        slot: 6,
        member: 2,
        form: 0,
        stamp: 1_030,
        order_num: 1,
        role: 262_400,
        form_num: 1,
        buildings: 0,
        who: 0,
    },
    LiveGroup {
        slot: 64,
        member: 0,
        form: 0,
        stamp: 1_110,
        order_num: 1,
        role: 262_416,
        form_num: 1,
        buildings: 0,
        who: 1,
    },
    LiveGroup {
        slot: 65,
        member: 1,
        form: -1,
        stamp: 1_124,
        order_num: 1,
        role: 262_912,
        form_num: 1,
        buildings: 0,
        who: 1,
    },
    LiveGroup {
        slot: 128,
        member: 0,
        form: 0,
        stamp: 137,
        order_num: 7,
        role: 262_416,
        form_num: 1,
        buildings: 0,
        who: 2,
    },
    LiveGroup {
        slot: 129,
        member: 2_000,
        form: -1,
        stamp: 951,
        order_num: 0,
        role: 0,
        form_num: 0,
        buildings: 1,
        who: 2,
    },
    LiveGroup {
        slot: 130,
        member: 2_005,
        form: -1,
        stamp: 1_151,
        order_num: 0,
        role: 0,
        form_num: 0,
        buildings: 1,
        who: 2,
    },
    LiveGroup {
        slot: 192,
        member: 2_000,
        form: -1,
        stamp: 926,
        order_num: 0,
        role: 0,
        form_num: 0,
        buildings: 1,
        who: 3,
    },
    LiveGroup {
        slot: 194,
        member: 0,
        form: 0,
        stamp: 756,
        order_num: 1,
        role: 262_416,
        form_num: 1,
        buildings: 0,
        who: 3,
    },
];
