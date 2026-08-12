//! Canonical `Armies::walk_data` owner for the additive DoNSave v17 section.
//!
//! Retail walks eight fixed `PtrArray<Army>` containers. Each contains sixteen present
//! pointers; every Army contributes its `valid:i16`, and only a valid Army contributes the
//! remaining 150 bytes of `ArmyData`. The Sim erases the pointer-array allocation history, but
//! `Armies::init` fixes it exactly: length/capacity 16, increment -1, flags zero, and sixteen
//! present pointers. This codec emits and validates that measured image rather than inventing a
//! compact field order.

use crate::systems::{
    armies::{Armies, ArmyData, ARMIES_PER_PLAYER, ARMY_MAX_GROUPS},
    groups_guys::{Groups, NUM_GROUPS},
};
use crate::tick::NUM_LEADERS;

use super::{Reader, SaveError, Writer};

const ARMY_IMAGE_BYTES: usize = 152;
const LIVE_TAIL_BYTES: usize = ARMY_IMAGE_BYTES - 2;
const PTR_INCREMENT: i16 = -1;

fn validate_cross_links(armies: &Armies, groups: &Groups) -> Result<(), SaveError> {
    if armies.lists.len() != NUM_LEADERS {
        return Err(SaveError::Invalid("Armies owner-list cardinality"));
    }
    let mut predecessor = vec![None; NUM_GROUPS];
    for (owner, list) in armies.lists.iter().enumerate() {
        if list.len() != ARMIES_PER_PLAYER {
            return Err(SaveError::Invalid("Army slot cardinality"));
        }
        for (slot, army) in list.iter().enumerate() {
            if army.valid == 0 {
                continue;
            }
            if army.valid != 1
                || army.who != owner as i16
                || army.army != slot as i16
                || !(0..=ARMY_MAX_GROUPS as i16).contains(&army.num_groups)
            {
                return Err(SaveError::Invalid("live Army identity/shape"));
            }
            for &group_id in &army.list[..army.num_groups as usize] {
                if group_id < 0 {
                    continue;
                }
                let group_id = usize::try_from(group_id)
                    .ok()
                    .filter(|group_id| *group_id < groups.list.len())
                    .ok_or(SaveError::Invalid("Army group reference"))?;
                if predecessor[group_id].replace((owner, slot)).is_some() {
                    return Err(SaveError::Invalid("Army group duplicate/cycle"));
                }
                let group = &groups.list[group_id];
                if group.num != 0 && (group.who as usize != owner || group.army != slot as i32) {
                    return Err(SaveError::Invalid("Army/Group backlink"));
                }
            }
        }
    }
    Ok(())
}

pub(super) fn validate(armies: &Armies, groups: &Groups) -> Result<(), SaveError> {
    validate_cross_links(armies, groups)
}

pub(super) fn write(armies: &Armies, groups: &Groups) -> Result<Vec<u8>, SaveError> {
    validate_cross_links(armies, groups)?;
    let mut w = Writer::default();
    for list in &armies.lists {
        w.i32(ARMIES_PER_PLAYER as i32);
        w.i32(ARMIES_PER_PLAYER as i32);
        w.i16(PTR_INCREMENT);
        w.u8(0);
        for _ in 0..ARMIES_PER_PLAYER {
            w.u8(1);
        }
        w.i32(ARMIES_PER_PLAYER as i32);
        w.i16(PTR_INCREMENT);
        for army in list {
            let image = army.image();
            w.bytes(&image[..2]);
            if army.valid != 0 {
                w.bytes(&image[2..]);
            }
        }
    }
    Ok(w.0)
}

pub(super) fn read(data: &[u8], groups: &Groups) -> Result<Armies, SaveError> {
    let mut r = Reader::new(data);
    let mut armies = Armies::new();
    for owner in 0..NUM_LEADERS {
        if r.i32()? != ARMIES_PER_PLAYER as i32
            || r.i32()? != ARMIES_PER_PLAYER as i32
            || r.i16()? != PTR_INCREMENT
            || r.u8()? != 0
        {
            return Err(SaveError::Invalid("Armies pointer-array header"));
        }
        for _ in 0..ARMIES_PER_PLAYER {
            if r.u8()? != 1 {
                return Err(SaveError::Invalid("Army pointer presence"));
            }
        }
        if r.i32()? != ARMIES_PER_PLAYER as i32 || r.i16()? != PTR_INCREMENT {
            return Err(SaveError::Invalid("Armies repeated allocation history"));
        }
        for slot in 0..ARMIES_PER_PLAYER {
            let valid = r.i16()?;
            if valid == 0 {
                continue;
            }
            let tail = r.take(LIVE_TAIL_BYTES)?;
            armies.lists[owner][slot] = decode_live_army(valid, tail);
        }
    }
    r.finish()?;
    validate_cross_links(&armies, groups)?;
    Ok(armies)
}

fn decode_live_army(valid: i16, tail: &[u8]) -> ArmyData {
    debug_assert_eq!(tail.len(), LIVE_TAIL_BYTES);
    let i16_at = |offset: usize| i16::from_le_bytes(tail[offset..offset + 2].try_into().unwrap());
    let i32_at = |offset: usize| i32::from_le_bytes(tail[offset..offset + 4].try_into().unwrap());
    let mut army = ArmyData {
        valid,
        army: i16_at(0),
        status: i32_at(2),
        reg: i32_at(6),
        role: i32_at(10),
        num_units: i32_at(14),
        num_captains: i32_at(18),
        num_standard: i32_at(22),
        num_decoys: i32_at(26),
        city: i32_at(30),
        navy: i32_at(34),
        human_frame: i32_at(38),
        hurry: i32_at(42),
        target_o: i32_at(46),
        target_who: i32_at(50),
        x: i32_at(54),
        y: i32_at(58),
        angle: i32_at(62),
        rally_dist: i32_at(66),
        muster_x: i32_at(70),
        muster_y: i32_at(74),
        muster_angle: i32_at(78),
        who: i16_at(146),
        num_groups: i16_at(148),
        ..ArmyData::default()
    };
    for (index, group) in army.list.iter_mut().enumerate() {
        *group = i32_at(82 + index * 4);
    }
    debug_assert_eq!(&army.image()[2..], tail);
    army
}
