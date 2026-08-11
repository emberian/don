// SPDX-License-Identifier: GPL-3.0-or-later
//! `Group::action_hotkey(int)` `0x006FA7A0`, 64 bytes.
//!
//! The whole receiver is four instructions of work.  [measured, capstone over
//! `ron-bin/riseofnations.exe` sha256 `30478a44…625079`, named from `ron-bin/sbl/rise.pdb`]:
//!
//! ```text
//! 006fa7a4  imul esi, dword ptr [ebp + 8], 0x9fc   ; slot * sizeof(HotKeyGroup)
//! 006fa7ab  mov  edx, ecx                          ; edx <- this (the source Group)
//! 006fa7ad  mov  ecx, dword ptr [0xc0afe0]         ; ecx <- hot_key_groups
//! 006fa7b3  lea  ecx, [esi + ecx]
//! 006fa7b6  call 0x715120                          ; HotKeyGroups::copy_group
//! 006fa7bb  mov  ecx, dword ptr [0xc0afe0]
//! 006fa7c1  push 1
//! 006fa7c3  lea  ecx, [esi + ecx]
//! 006fa7c6  call 0x7152f0                          ; HotKeyGroupOut::update_name
//! 006fa7cb  mov  eax, dword ptr [0xc0afe0]
//! 006fa7d0  mov  dword ptr [esi + eax + 0x9d8], 0  ; camera valid <- 0
//! 006fa7dd  ret  4
//! ```
//!
//! `copy_group` receives the destination `HotKeyGroup` in `ECX` and the source `Group` in
//! `EDX`; it takes no stack argument and cleans no stack (`ret` at `0x00715224`).  The PDB
//! declares it `void HotKeyGroups::copy_group(Group*, const …)`; the emitted body is the
//! authority and disagrees about where the arguments live.
//!
//! # Why this is the same state transition opcode 34 already writes
//!
//! `CommandPackage::process_hotkey` `0x009474D0` does not call this receiver — it *inlines*
//! it.  Its `clear == 0` arm at `0x009475B8..0x009475F2` is instruction-for-instruction the
//! body above, with the group taken from `groups.list[package.group]`.  So the recovered
//! `copy_hotkey_group` and the `+0x9D8` camera-valid store already carried by the opcode-34
//! row *are* this action; this module only names the receiver-shaped entry point, which
//! retail reaches from `Console::on_key_down` `0x007CC80B` / `0x007CC8BA` (its only two
//! call sites — measured over every named procedure in `.text`).
//!
//! # What is deliberately not simulation state
//!
//! `HotKeyGroupOut::update_name(1)` `0x007152F0` (3,404 bytes) writes exactly two words of
//! the destination and nothing else: the `String` at `+0x9E0` and the icon selector at
//! `+0x9F4` [measured — those are the only store destinations in the whole body, and the
//! only calls it makes are 31 `String::operator+=`, two `String::operator=`, `String::String`,
//! `String::close`, `memset`, and the read-only `ObjectData::is_worker` / `can_carry`].  Its
//! first branch compares the group owner against the local display player
//! (`[[0xc06210]+0x298]` at `0x0071533E`) and returns early for anybody else, so it is not a
//! deterministic function of shared state at all.  It is the display name, and it stays
//! outside the headless bridge — exactly as it already does for opcode 34.

/// `sizeof(HotKeyGroup)`, the stride `action_hotkey` multiplies its argument by
/// [measured, `imul esi, dword ptr [ebp + 8], 0x9fc`].
pub const SIZEOF_HOTKEY_GROUP: usize = 0x9fc;

/// `HotKeyGroup::camera_valid`, the dword this receiver zeroes [measured,
/// `mov dword ptr [esi + eax + 0x9d8], 0`]. `process_hotkey`'s `clear != 0` arm writes the
/// same field, plus `+0x9D0`/`+0x9D4`/`+0x9DC`, which is what identifies it as the camera
/// validity latch.
pub const HOTKEY_CAMERA_VALID_OFFSET: usize = 0x9d8;

pub const GROUP_ACTION_HOTKEY_VA: u32 = 0x006f_a7a0;
pub const GROUP_ACTION_HOTKEY_BYTES: usize = 64;
pub const HOTKEY_GROUPS_COPY_GROUP_VA: u32 = 0x0071_5120;
pub const HOTKEY_GROUP_OUT_UPDATE_NAME_VA: u32 = 0x0071_52f0;
pub const CONSOLE_ON_KEY_DOWN_CALL_SITES: [u32; 2] = [0x007c_c80b, 0x007c_c8ba];

/// One ordered operation of `Group::action_hotkey`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HotKeyActionStep {
    /// `HotKeyGroups::copy_group(&hot_key_groups[slot], this)` `0x00715120`.
    CopyGroup { slot: usize, stamp: i32 },
    /// `HotKeyGroupOut::update_name(1)` `0x007152F0` — display name and icon only.
    UpdateName { slot: usize, arg: i32 },
    /// `hot_key_groups[slot].camera_valid = 0`.
    ClearCamera { slot: usize },
}

/// The plan for one `Group::action_hotkey` call. Every reached branch of the receiver is
/// represented; there are no others.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HotKeyActionPlan {
    pub slot: usize,
    pub steps: Vec<HotKeyActionStep>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HotKeyActionPlanError {
    /// Retail indexes `hot_key_groups` with no bound check at all; a port must refuse
    /// rather than reproduce the out-of-bounds write.
    SlotOutOfRange { slot: i32, slots: usize },
}

/// Recover the complete ordered body of `Group::action_hotkey(slot)`.
///
/// `stamp` is `Game::frame`, read by `copy_group` from `[[0x00C061EC] + 0x550]` at
/// `0x00715158` and stored into the destination's `+0x14`.
pub fn plan_action_hotkey(
    slot: i32,
    stamp: i32,
    slots: usize,
) -> Result<HotKeyActionPlan, HotKeyActionPlanError> {
    let Ok(index) = usize::try_from(slot) else {
        return Err(HotKeyActionPlanError::SlotOutOfRange { slot, slots });
    };
    if index >= slots {
        return Err(HotKeyActionPlanError::SlotOutOfRange { slot, slots });
    }
    Ok(HotKeyActionPlan {
        slot: index,
        steps: vec![
            HotKeyActionStep::CopyGroup { slot: index, stamp },
            HotKeyActionStep::UpdateName {
                slot: index,
                arg: 1,
            },
            HotKeyActionStep::ClearCamera { slot: index },
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_body_is_exactly_copy_then_name_then_camera_clear() {
        let plan = plan_action_hotkey(7, 4242, 162).expect("in-range slot");
        assert_eq!(
            plan,
            HotKeyActionPlan {
                slot: 7,
                steps: vec![
                    HotKeyActionStep::CopyGroup {
                        slot: 7,
                        stamp: 4242
                    },
                    HotKeyActionStep::UpdateName { slot: 7, arg: 1 },
                    HotKeyActionStep::ClearCamera { slot: 7 },
                ],
            }
        );
    }

    #[test]
    fn an_out_of_range_slot_refuses_instead_of_writing_past_the_array() {
        assert_eq!(
            plan_action_hotkey(162, 0, 162),
            Err(HotKeyActionPlanError::SlotOutOfRange {
                slot: 162,
                slots: 162
            })
        );
        assert_eq!(
            plan_action_hotkey(-1, 0, 162),
            Err(HotKeyActionPlanError::SlotOutOfRange {
                slot: -1,
                slots: 162
            })
        );
    }
}
