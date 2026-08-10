// SPDX-License-Identifier: GPL-3.0-or-later
//! Source-only authoritative owner for policy-visible external Unit identities.
//!
//! `don-env` currently refuses ATTACK at `TargetIdentityVisibilityUnavailable`.  This module
//! freezes the smallest owner that can close that boundary without exposing an omniscient dense
//! World row: a complete frame image is canonicalized by stable [`Handle`], projected through
//! retail's cloak/detection/fog predicate, and bound to a revisioned one-based policy ordinal.
//! It is intentionally not wired into `Sim` or `don-env` yet.

#![allow(dead_code)]

use crate::Handle;

pub const VIEWER_SLOTS: usize = 8;
pub const OBJECT_OWNER_SLOTS: usize = 10;

pub const UNIT_IS_SEEN_VA: u32 = 0x0060_7a60;
pub const UNIT_IS_DETECTED_VA: u32 = 0x0060_a630;
pub const UNIT_IS_CLOAKED_VA: u32 = 0x0060_a6a0;
pub const WORLD_IS_DETECTED_VA: u32 = 0x006b_48c0;
pub const WORLD_IS_SEEN_VA: u32 = 0x006b_55c0;

pub const UNIT_MASK_CLOAK: u32 = 0x0000_0800;
/// `UnitData::is_seen` skips its detection query when this instance bit is set.
pub const UNIT_MASK_DETECTION_BYPASS: u32 = 0x0000_1000;
pub const TYPE_UNIT_FLAG_CLOAK: u32 = 0x0000_4000;
pub const TYPE_UNIT_FLAG_CLOAK_WHILE_IDLE: u32 = 0x0004_0000;
pub const UNIT_MASK2_CLOAK: u32 = 0x0000_8000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExternalEntityIdentity {
    /// Stable across dense-row compaction; generation rejects a recycled Sim id.
    pub handle: Handle,
    /// Retail `SubObjectData::who +0x09`.
    pub who: u8,
    /// Retail owner-array slot `SubObjectData::o +0x0A`.
    pub object_o: i16,
    /// Retail incarnation guard `ObjectData::uid +0x30`.
    pub uid: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExternalEntityPublicState {
    pub type_id: i32,
    pub x: i32,
    pub y: i32,
    pub hits: i32,
    pub angle: i32,
    pub speed: i16,
    pub recharge: u8,
    pub order_index: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetailUnitVisibilityFacts {
    /// `SubObjectData::flags +0x08 & 1`; false rows are rejected, never retained.
    pub active: bool,
    /// Instance `UnitData::unit_masks +0x68`.
    pub unit_masks: u32,
    /// Canonical type-owner `UnitTypeData::unit_flags +0x2B4`.
    pub type_unit_flags: u32,
    /// Instance `UnitData::unit_masks2 +0x6C`.
    pub unit_masks2: u32,
    /// Exact `UnitData::get_order() != nullptr` result for idle-cloak evaluation.
    pub has_order: bool,
    /// Walked `ObjectData::visible +0x40`; direct viewer bit, after fog misses.
    pub object_visible_mask: u8,
    /// Current World `seen +0x15C` byte at the object's exact fog cell.
    pub cell_seen_mask: u8,
    /// Current World `seen3 +0x164` byte at the same fog cell.
    pub cell_detected_mask: u8,
    /// `WData::who` at the enclosing WCoord. Retail admits `-1` and `-2` sentinels.
    pub territory_owner: i8,
}

impl RetailUnitVisibilityFacts {
    pub const fn is_cloaked(self) -> bool {
        self.unit_masks & UNIT_MASK_CLOAK != 0
            || self.type_unit_flags & TYPE_UNIT_FLAG_CLOAK != 0
            || self.unit_masks2 & UNIT_MASK2_CLOAK != 0
            || (self.type_unit_flags & TYPE_UNIT_FLAG_CLOAK_WHILE_IDLE != 0 && !self.has_order)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExternalUnitFrameRow {
    pub identity: ExternalEntityIdentity,
    pub public: ExternalEntityPublicState,
    pub visibility: RetailUnitVisibilityFacts,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetailViewerFacts {
    pub who: u8,
    /// Exact `LeaderData::ally_mask +0x6929` byte used by both World plane queries.
    pub vision_mask: u8,
    pub see_all: bool,
    pub reveal_counter: i16,
    pub see_own_territory: bool,
    /// Bit `n` is set iff retail `LeaderData::is_ally(n)` is true for this viewer.
    pub allied_territory_mask: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalVisibilityFrame {
    pub frame: i32,
    /// `GameData +0x30`; value 3 makes current fog visible, but never grants detection.
    pub fog_option: u8,
    /// Dense source order is deliberately irrelevant.
    pub rows: Vec<ExternalUnitFrameRow>,
    pub viewers: Vec<RetailViewerFacts>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VisibilityDecision {
    Visible,
    HiddenUndetectedCloak,
    HiddenByCurrentFog,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VisibilityInstallFault {
    InvalidFogOption(u8),
    TooManyRows(usize),
    InvalidViewer(u8),
    DuplicateViewer(u8),
    ViewerMaskExcludesSelf {
        viewer: u8,
        mask: u8,
    },
    InvalidOwner {
        row: usize,
        who: u8,
    },
    InvalidObjectSlot {
        row: usize,
        object_o: i16,
    },
    InvalidType {
        row: usize,
        type_id: i32,
    },
    InactiveRow {
        row: usize,
    },
    InvalidTerritoryOwner {
        row: usize,
        who: i8,
    },
    DuplicateHandle {
        first: usize,
        second: usize,
        handle: Handle,
    },
    DuplicateRetailIdentity {
        first: usize,
        second: usize,
        who: u8,
        object_o: i16,
        uid: u16,
    },
    RevisionExhausted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VisibilityProjectionFault {
    Uninstalled,
    InvalidViewer(u8),
    ViewerUnavailable(u8),
    MissingTargetEntity,
    TargetOrdinalUnavailable { ordinal: u16, visible: usize },
    StaleBinding { expected: u64, observed: u64 },
    BindingIdentityChanged,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VisibleExternalEntity {
    pub ordinal: u16,
    pub identity: ExternalEntityIdentity,
    pub public: ExternalEntityPublicState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalVisibilityProjection {
    pub owner_revision: u64,
    pub frame: i32,
    pub viewer: u8,
    pub rows: Vec<VisibleExternalEntity>,
}

/// Opaque prepare token for ATTACK mask/apply parity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VisibleTargetBinding {
    owner_revision: u64,
    frame: i32,
    viewer: u8,
    ordinal: u16,
    identity: ExternalEntityIdentity,
}

impl VisibleTargetBinding {
    pub const fn owner_revision(self) -> u64 {
        self.owner_revision
    }

    pub const fn frame(self) -> i32 {
        self.frame
    }

    pub const fn viewer(self) -> u8 {
        self.viewer
    }

    pub const fn ordinal(self) -> u16 {
        self.ordinal
    }

    pub const fn identity(self) -> ExternalEntityIdentity {
        self.identity
    }
}

#[derive(Clone, Debug)]
struct InstalledFrame {
    frame: i32,
    fog_option: u8,
    rows: Vec<ExternalUnitFrameRow>,
    viewers: Vec<RetailViewerFacts>,
}

#[derive(Clone, Debug, Default)]
pub struct ExternalEntityVisibilityOwner {
    revision: u64,
    installed: Option<InstalledFrame>,
}

impl ExternalEntityVisibilityOwner {
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    pub fn install_frame(
        &mut self,
        mut frame: ExternalVisibilityFrame,
    ) -> Result<u64, VisibilityInstallFault> {
        validate_frame(&frame)?;
        let next = self
            .revision
            .checked_add(1)
            .ok_or(VisibilityInstallFault::RevisionExhausted)?;
        frame.rows.sort_by_key(row_key);
        frame.viewers.sort_by_key(|viewer| viewer.who);
        self.installed = Some(InstalledFrame {
            frame: frame.frame,
            fog_option: frame.fog_option,
            rows: frame.rows,
            viewers: frame.viewers,
        });
        self.revision = next;
        Ok(next)
    }

    /// A Sim reset invalidates every outstanding ordinal. The next observation must install a
    /// complete new frame even when deterministic setup recreates identical handles.
    pub fn reset(&mut self) -> Result<u64, VisibilityInstallFault> {
        let next = self
            .revision
            .checked_add(1)
            .ok_or(VisibilityInstallFault::RevisionExhausted)?;
        self.installed = None;
        self.revision = next;
        Ok(next)
    }

    pub fn project(
        &self,
        viewer: u8,
    ) -> Result<ExternalVisibilityProjection, VisibilityProjectionFault> {
        if usize::from(viewer) >= VIEWER_SLOTS {
            return Err(VisibilityProjectionFault::InvalidViewer(viewer));
        }
        let frame = self
            .installed
            .as_ref()
            .ok_or(VisibilityProjectionFault::Uninstalled)?;
        let viewer_facts = frame
            .viewers
            .iter()
            .find(|facts| facts.who == viewer)
            .ok_or(VisibilityProjectionFault::ViewerUnavailable(viewer))?;
        let mut rows = Vec::new();
        for row in &frame.rows {
            if row.identity.who == viewer
                || visibility_decision(row, viewer_facts, frame.fog_option)
                    != VisibilityDecision::Visible
            {
                continue;
            }
            let ordinal = u16::try_from(rows.len() + 1)
                .expect("validated frame cannot exceed the u16 policy ordinal range");
            rows.push(VisibleExternalEntity {
                ordinal,
                identity: row.identity,
                public: row.public,
            });
        }
        Ok(ExternalVisibilityProjection {
            owner_revision: self.revision,
            frame: frame.frame,
            viewer,
            rows,
        })
    }

    pub fn bind_target(
        &self,
        viewer: u8,
        ordinal: u16,
    ) -> Result<VisibleTargetBinding, VisibilityProjectionFault> {
        if ordinal == 0 {
            return Err(VisibilityProjectionFault::MissingTargetEntity);
        }
        let projection = self.project(viewer)?;
        let target = projection.rows.get(usize::from(ordinal - 1)).ok_or(
            VisibilityProjectionFault::TargetOrdinalUnavailable {
                ordinal,
                visible: projection.rows.len(),
            },
        )?;
        Ok(VisibleTargetBinding {
            owner_revision: projection.owner_revision,
            frame: projection.frame,
            viewer,
            ordinal,
            identity: target.identity,
        })
    }

    pub fn revalidate_target(
        &self,
        binding: VisibleTargetBinding,
    ) -> Result<VisibleExternalEntity, VisibilityProjectionFault> {
        if binding.owner_revision != self.revision {
            return Err(VisibilityProjectionFault::StaleBinding {
                expected: binding.owner_revision,
                observed: self.revision,
            });
        }
        let projection = self.project(binding.viewer)?;
        let target = projection
            .rows
            .get(usize::from(binding.ordinal - 1))
            .copied()
            .ok_or(VisibilityProjectionFault::BindingIdentityChanged)?;
        if target.identity != binding.identity || projection.frame != binding.frame {
            return Err(VisibilityProjectionFault::BindingIdentityChanged);
        }
        Ok(target)
    }

    /// Canonical owner-content digest. Revision and dense source order are excluded; every
    /// identity, public field, cloak/fog input, viewer policy, and the frame are included.
    /// This is a determinism/staleness diagnostic, not a retail `DataWalk` checksum channel.
    pub fn digest(&self) -> Result<u64, VisibilityProjectionFault> {
        let frame = self
            .installed
            .as_ref()
            .ok_or(VisibilityProjectionFault::Uninstalled)?;
        let mut digest = 0xcbf2_9ce4_8422_2325u64;
        mix(&mut digest, frame.frame as u32 as u64);
        mix(&mut digest, u64::from(frame.fog_option));
        mix(&mut digest, frame.rows.len() as u64);
        for row in &frame.rows {
            mix(&mut digest, u64::from(row.identity.handle.id));
            mix(&mut digest, u64::from(row.identity.handle.generation));
            mix(&mut digest, u64::from(row.identity.who));
            mix(&mut digest, row.identity.object_o as u16 as u64);
            mix(&mut digest, u64::from(row.identity.uid));
            mix(&mut digest, row.public.type_id as u32 as u64);
            mix(&mut digest, row.public.x as u32 as u64);
            mix(&mut digest, row.public.y as u32 as u64);
            mix(&mut digest, row.public.hits as u32 as u64);
            mix(&mut digest, row.public.angle as u32 as u64);
            mix(&mut digest, row.public.speed as u16 as u64);
            mix(&mut digest, u64::from(row.public.recharge));
            mix(&mut digest, u64::from(row.public.order_index));
            mix(&mut digest, row.visibility.active as u64);
            mix(&mut digest, u64::from(row.visibility.unit_masks));
            mix(&mut digest, u64::from(row.visibility.type_unit_flags));
            mix(&mut digest, u64::from(row.visibility.unit_masks2));
            mix(&mut digest, row.visibility.has_order as u64);
            mix(&mut digest, u64::from(row.visibility.object_visible_mask));
            mix(&mut digest, u64::from(row.visibility.cell_seen_mask));
            mix(&mut digest, u64::from(row.visibility.cell_detected_mask));
            mix(&mut digest, row.visibility.territory_owner as u8 as u64);
        }
        mix(&mut digest, frame.viewers.len() as u64);
        for viewer in &frame.viewers {
            mix(&mut digest, u64::from(viewer.who));
            mix(&mut digest, u64::from(viewer.vision_mask));
            mix(&mut digest, viewer.see_all as u64);
            mix(&mut digest, viewer.reveal_counter as u16 as u64);
            mix(&mut digest, viewer.see_own_territory as u64);
            mix(&mut digest, u64::from(viewer.allied_territory_mask));
        }
        Ok(digest)
    }
}

pub fn visibility_decision(
    row: &ExternalUnitFrameRow,
    viewer: &RetailViewerFacts,
    fog_option: u8,
) -> VisibilityDecision {
    let facts = row.visibility;
    if facts.is_cloaked()
        && facts.unit_masks & UNIT_MASK_DETECTION_BYPASS == 0
        && row.identity.who != viewer.who
        && facts.cell_detected_mask & viewer.vision_mask == 0
    {
        return VisibilityDecision::HiddenUndetectedCloak;
    }
    if row.identity.who == viewer.who
        || fog_option == 3
        || viewer.see_all
        || viewer.reveal_counter != 0
        || territory_is_visible(facts.territory_owner, viewer)
        || facts.cell_seen_mask & viewer.vision_mask != 0
        || facts.object_visible_mask & (1u8 << viewer.who) != 0
    {
        VisibilityDecision::Visible
    } else {
        VisibilityDecision::HiddenByCurrentFog
    }
}

fn territory_is_visible(owner: i8, viewer: &RetailViewerFacts) -> bool {
    viewer.see_own_territory
        && owner >= 0
        && viewer.allied_territory_mask & (1u8 << owner as u8) != 0
}

fn validate_frame(frame: &ExternalVisibilityFrame) -> Result<(), VisibilityInstallFault> {
    if frame.fog_option > 3 {
        return Err(VisibilityInstallFault::InvalidFogOption(frame.fog_option));
    }
    if frame.rows.len() > usize::from(u16::MAX) {
        return Err(VisibilityInstallFault::TooManyRows(frame.rows.len()));
    }
    for (index, viewer) in frame.viewers.iter().enumerate() {
        if usize::from(viewer.who) >= VIEWER_SLOTS {
            return Err(VisibilityInstallFault::InvalidViewer(viewer.who));
        }
        if viewer.vision_mask & (1u8 << viewer.who) == 0 {
            return Err(VisibilityInstallFault::ViewerMaskExcludesSelf {
                viewer: viewer.who,
                mask: viewer.vision_mask,
            });
        }
        if frame.viewers[..index]
            .iter()
            .any(|earlier| earlier.who == viewer.who)
        {
            return Err(VisibilityInstallFault::DuplicateViewer(viewer.who));
        }
    }
    for (index, row) in frame.rows.iter().enumerate() {
        if usize::from(row.identity.who) >= OBJECT_OWNER_SLOTS {
            return Err(VisibilityInstallFault::InvalidOwner {
                row: index,
                who: row.identity.who,
            });
        }
        if row.identity.object_o < 0 {
            return Err(VisibilityInstallFault::InvalidObjectSlot {
                row: index,
                object_o: row.identity.object_o,
            });
        }
        if row.public.type_id < 0 {
            return Err(VisibilityInstallFault::InvalidType {
                row: index,
                type_id: row.public.type_id,
            });
        }
        if !row.visibility.active {
            return Err(VisibilityInstallFault::InactiveRow { row: index });
        }
        if !(-2..=7).contains(&row.visibility.territory_owner) {
            return Err(VisibilityInstallFault::InvalidTerritoryOwner {
                row: index,
                who: row.visibility.territory_owner,
            });
        }
        for (earlier_index, earlier) in frame.rows[..index].iter().enumerate() {
            if earlier.identity.handle == row.identity.handle {
                return Err(VisibilityInstallFault::DuplicateHandle {
                    first: earlier_index,
                    second: index,
                    handle: row.identity.handle,
                });
            }
            if earlier.identity.who == row.identity.who
                && earlier.identity.object_o == row.identity.object_o
                && earlier.identity.uid == row.identity.uid
            {
                return Err(VisibilityInstallFault::DuplicateRetailIdentity {
                    first: earlier_index,
                    second: index,
                    who: row.identity.who,
                    object_o: row.identity.object_o,
                    uid: row.identity.uid,
                });
            }
        }
    }
    Ok(())
}

fn row_key(row: &ExternalUnitFrameRow) -> (u32, u32, u8, i16, u16) {
    (
        row.identity.handle.id,
        row.identity.handle.generation,
        row.identity.who,
        row.identity.object_o,
        row.identity.uid,
    )
}

fn mix(digest: &mut u64, value: u64) {
    *digest ^= value;
    *digest = digest.wrapping_mul(0x0000_0100_0000_01b3);
}
