//! Exact fresh-Unit continuation through `Object::update_seen(0)` and
//! `World::reveal_fog`.
//!
//! This source is intentionally not registered as a replay-channel producer.  It owns the
//! synchronized fog, Good-ever-seen, Item-ever-seen, `LeaderData::new_rares`, and
//! `LeaderData::oil_patches` mutations reached by the normal-land `Unit::init` setup call.
//! `Unit::get_goody_box` is the first external owner: it constructs a scratch `Group`, pushes
//! it into `Groups`, and issues `Group::action_move_to`, so this module emits an exact request
//! instead of mutating an unjoined Groups projection.

use crate::leader_initial_prefix::CHECKSUM_LEADER_SLOTS;
use crate::leaders_dynamic_children_frontier::{DynamicLeadersAuthority, RetailArray};
use crate::unit_init_collision_tail_deep_re::{
    UnitInitCollisionTailReceipt, UnitInitTailExternalResidual, VisibilityUpdateRequest,
    OBJECT_UPDATE_SEEN_CALL_VA, OBJECT_UPDATE_SEEN_VA,
};
use don_sim::container::increase_by;
use don_sim::systems::borders_fog::{update_seen, CircleTable, Fog, SeeingObject};
use don_sim::systems::items::{Items, DOWN_ITEM, WFLAG_ITEM, WFLAG_OVERRIDE_LAND};
use don_sim::systems::map_terrain::{land, tflag, wflag, World};
use don_sim::systems::movement::{cosx, sinx};
use don_sim::systems::step12_visibility_producer_frontier::{
    resolve_unit_los, UnitLosFacts, UnitLosFault,
};
use don_sim::systems::world_oil_goods::{OilGoodRuntime, OIL_GOOD_TYPE};
use std::collections::HashSet;
use std::fmt;

pub const WORLD_REVEAL_FOG_CALL_VA: u32 = 0x0065_1ef3;
pub const WORLD_REVEAL_FOG_VA: u32 = 0x006b_3d30;
pub const OBJECTS_FIND_GOOD_AT_CALL_VA: u32 = 0x006b_3dbe;
pub const OBJECTS_FIND_GOOD_AT_VA: u32 = 0x0065_bec0;
pub const LEADER_NEW_RARE_CALL_VA: u32 = 0x006b_3dfb;
pub const LEADER_NEW_RARE_VA: u32 = 0x006d_9e70;
pub const LEADER_TYPE_AVAIL_CALL_VA: u32 = 0x006d_9f27;
pub const LEADER_TYPE_AVAIL_VA: u32 = 0x006e_33a0;
pub const OBJECT_DATA_IS_VA: u32 = 0x0065_3790;
pub const OBJECTS_FIND_GOODY_AT_CALL_VA: u32 = 0x006b_40ee;
pub const OBJECTS_FIND_GOODY_AT_VA: u32 = 0x0065_c040;
pub const UNIT_GET_GOODY_BOX_CALL_VA: u32 = 0x006b_4163;
pub const UNIT_GET_GOODY_BOX_VA: u32 = 0x005f_7690;
pub const GROUP_CLEAR_VA: u32 = 0x0071_3e80;
pub const GROUP_ADD_VA: u32 = 0x0071_4350;
pub const GROUPS_PUSH_GROUP_VA: u32 = 0x0070_f9e0;
pub const GROUP_ACTION_MOVE_TO_VA: u32 = 0x0070_fba0;
pub const UNIT_DATA_LOS_VA: u32 = 0x0061_00c0;
pub const UNIT_DATA_IS_WONDER_VA: u32 = 0x0041_bff0;
pub const UNIT_UPDATE_LOCAL_SEEN_VA: u32 = 0x0060_e410;
pub const WORLD_SET_SEEN2_VA: u32 = 0x006b_4bb0;
pub const PROJECT_VA: u32 = 0x0092_cf40;
pub const ARRAY_BASE_ADD_VA: u32 = 0x0042_daf0;

pub const DOWN_GOOD: i16 = -2;
pub const UNIT_AUTO_EXPLORE: u32 = 0x0000_0100;
pub const GOOD_WCELL_FOOTPRINT: u16 = 0x0001;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RevealObjectLink {
    pub who: i16,
    pub o: i16,
    pub next: i16,
    pub next_who: i16,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RevealLeaderFacts {
    pub leader_flags: i32,
    /// `LeaderData::diplos[8]` at `+0x74`.
    pub diplos: [i32; CHECKSUM_LEADER_SLOTS],
}

/// Type/catalog facts read by `Leader::new_rare` after `type_avail` succeeds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RareGoodClassAuthority {
    pub good_slot: i32,
    pub type_index: i32,
    pub is_type_6: bool,
    pub is_type_31: bool,
}

/// One actually reached `LeaderData::type_avail(type, 1)` call, in native chronology.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeaderTypeAvailReceipt {
    pub call_va: u32,
    pub body_va: u32,
    pub leader: u8,
    pub type_index: i32,
    pub strict: i32,
    pub available: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetupUnitVisibilityAuthority {
    pub los: UnitLosFacts,
    pub leaders: [RevealLeaderFacts; CHECKSUM_LEADER_SLOTS],
    pub object_links: Vec<RevealObjectLink>,
    pub rare_goods: Vec<RareGoodClassAuthority>,
    pub type_avail_calls: Vec<LeaderTypeAvailReceipt>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NewRareDecision {
    Inactive,
    NotMutualAlly,
    HumanCandidateRejected,
    AlreadyKnown,
    TypeUnavailable,
    ExcludedType6,
    ExcludedType31,
    Added,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NewRareCandidateReceipt {
    pub leader: u8,
    pub decision: NewRareDecision,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewRareReceipt {
    pub call_va: u32,
    pub body_va: u32,
    pub source: u8,
    pub good_slot: i32,
    pub source_human_early_return: bool,
    pub candidates: Vec<NewRareCandidateReceipt>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetailArrayAddReceipt {
    pub body_va: u32,
    pub leader: u8,
    pub field: LeaderArrayField,
    pub value: i32,
    pub index: i32,
    pub length_before: i32,
    pub length_after: i32,
    pub capacity_before: i32,
    pub capacity_after: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeaderArrayField {
    NewRares,
    OilPatches,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RareRevealReceipt {
    pub current_resource_tile: bool,
    pub canonical_resource_tile: bool,
    pub find_good_call_va: Option<u32>,
    pub find_good_body_va: Option<u32>,
    pub good_slot: Option<i32>,
    pub new_rare: Option<NewRareReceipt>,
    pub ever_seen_before: Option<u8>,
    pub ever_seen_after: Option<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OilRevealReceipt {
    pub oil_cell: bool,
    pub good_slot: Option<i32>,
    pub already_known: bool,
    pub add: Option<RetailArrayAddReceipt>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemRevealReceipt {
    pub item_cell: bool,
    pub canonical_land_gate: bool,
    pub find_goody_call_va: Option<u32>,
    pub find_goody_body_va: Option<u32>,
    pub item_slot: Option<i32>,
    pub ever_seen_before: Option<u8>,
    pub ever_seen_after: Option<u8>,
}

/// Exact call arguments of `Unit::get_goody_box(wx, wy)` and its four child entries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AutoExploreGoodyRequest {
    pub ordinal: u16,
    pub reveal_ordinal: u16,
    pub call_va: u32,
    pub body_va: u32,
    pub who: u8,
    pub o: i16,
    pub world_x: i32,
    pub world_y: i32,
    pub target_fine_x: i32,
    pub target_fine_y: i32,
    pub group_clear_va: u32,
    pub group_add_va: u32,
    pub groups_push_group_va: u32,
    pub group_action_move_to_va: u32,
    pub queue_pos: i32,
    pub order_index: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RevealFogReceipt {
    pub ordinal: u16,
    pub call_va: u32,
    pub body_va: u32,
    pub fog_x: i32,
    pub fog_y: i32,
    pub world_x: i32,
    pub world_y: i32,
    pub rare: RareRevealReceipt,
    pub oil: OilRevealReceipt,
    pub item: ItemRevealReceipt,
    pub auto_explore: Option<AutoExploreGoodyRequest>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SetupVisibilityExternalResidual {
    None,
    GroupsAutoExplore {
        first_call_va: u32,
        first_body_va: u32,
        requests: Vec<AutoExploreGoodyRequest>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetupUnitVisibilityReceipt {
    pub object_update_seen_call_va: u32,
    pub object_update_seen_body_va: u32,
    pub unit_los_body_va: u32,
    pub unit_is_wonder_body_va: u32,
    pub resolved_los_tiles: i32,
    pub projected_small_los: bool,
    pub project_body_va: Option<u32>,
    pub stamp_fine_x: i32,
    pub stamp_fine_y: i32,
    pub object_visible_before: i8,
    pub object_visible_after: i8,
    pub update_local_seen_skipped: bool,
    pub update_local_seen_body_va: u32,
    pub set_seen2_body_va: u32,
    pub newly_explored: Vec<(i32, i32)>,
    pub reveals: Vec<RevealFogReceipt>,
    pub leader_array_adds: Vec<RetailArrayAddReceipt>,
    pub good_ever_seen_mutations: usize,
    pub item_ever_seen_mutations: usize,
    pub rng_before: i32,
    pub rng_after: i32,
    pub next_external_residual: SetupVisibilityExternalResidual,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SetupUnitVisibilityError {
    WrongTailResidual,
    InvalidFreshUnitSeam,
    InvalidWorld,
    InvalidCanonicalWorld,
    InvalidGoods,
    InvalidLeaderArray { leader: u8, field: LeaderArrayField },
    DuplicateObjectLink { who: i16, o: i16 },
    DuplicateRareAuthority { slot: i32 },
    MissingObjectLink { who: i16, o: i16 },
    ObjectChainCycle { who: i16, o: i16 },
    MissingRareAuthority { slot: i32 },
    StaleRareAuthority { slot: i32 },
    MissingTypeAvail { ordinal: usize },
    StaleTypeAvail { ordinal: usize },
    UnusedTypeAvail { first_unused: usize },
    Los(UnitLosFault),
    LosOutOfRange { los: i32 },
    ArrayCannotGrow { leader: u8, field: LeaderArrayField },
}

impl fmt::Display for SetupUnitVisibilityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "setup Unit visibility continuation refused: {self:?}")
    }
}

impl std::error::Error for SetupUnitVisibilityError {}

impl From<UnitLosFault> for SetupUnitVisibilityError {
    fn from(value: UnitLosFault) -> Self {
        Self::Los(value)
    }
}

#[derive(Clone, Copy)]
struct FreshVisibilitySeam {
    request: VisibilityUpdateRequest,
    leader_flags_after: u32,
    rng: i32,
}

fn bind_seam(
    tail: &UnitInitCollisionTailReceipt,
) -> Result<FreshVisibilitySeam, SetupUnitVisibilityError> {
    if tail.next_external_residual
        != UnitInitTailExternalResidual::ObjectUpdateSeen(tail.visibility)
    {
        return Err(SetupUnitVisibilityError::WrongTailResidual);
    }
    let request = tail.visibility;
    if request.call_va != OBJECT_UPDATE_SEEN_CALL_VA
        || request.body_va != OBJECT_UPDATE_SEEN_VA
        || request.incremental != 0
        || request.type_domain != 0
        || request.infiltrated != 0
        || request.owner as usize >= CHECKSUM_LEADER_SLOTS
        || request.o < 0
        || request.object_flags & 1 == 0
        || i32::from(request.owner) != tail.identity.owner
        || i32::from(request.o) != tail.identity.o
        || tail.rng_before != tail.rng_after
    {
        return Err(SetupUnitVisibilityError::InvalidFreshUnitSeam);
    }
    Ok(FreshVisibilitySeam {
        request,
        leader_flags_after: tail.leader_flags_after,
        rng: tail.rng_after,
    })
}

fn validate_world(world: &World) -> bool {
    world.xs > 0
        && world.ys > 0
        && world.fog_xs == world.xs * 2
        && world.fog_ys == world.ys * 2
        && world.tile_xs == world.xs * 4
        && world.tile_ys == world.ys * 4
        && world.wdata.len() == (world.xs * world.ys) as usize
        && world.tdata.len() == (world.tile_xs * world.tile_ys) as usize
        && world.seen.len() == (world.fog_xs * world.fog_ys) as usize
        && world.seen2.len() == world.seen.len()
        && world.seen3.len() == world.seen.len()
        && world.wcoord_seen.len() == world.wdata.len()
}

fn validate_array(array: &RetailArray<i32>) -> bool {
    array.length >= 0
        && array.capacity >= array.length
        && array.elements.len() == array.length as usize
        && array.flags == 0
        && array.increment != 0
}

fn array_contains(array: &RetailArray<i32>, value: i32) -> bool {
    array
        .elements
        .iter()
        .take(array.length as usize)
        .any(|&v| v == value)
}

fn array_add(
    array: &mut RetailArray<i32>,
    leader: u8,
    field: LeaderArrayField,
    value: i32,
) -> Result<RetailArrayAddReceipt, SetupUnitVisibilityError> {
    let length_before = array.length;
    let capacity_before = array.capacity;
    if array.length >= array.capacity {
        let by = increase_by(array.increment, array.capacity);
        let next = array.capacity.wrapping_add(by);
        if by <= 0 || next <= array.length {
            return Err(SetupUnitVisibilityError::ArrayCannotGrow { leader, field });
        }
        array.capacity = next;
    }
    let index = array.length;
    array.elements.push(value);
    array.length = array.length.wrapping_add(1);
    Ok(RetailArrayAddReceipt {
        body_va: ARRAY_BASE_ADD_VA,
        leader,
        field,
        value,
        index,
        length_before,
        length_after: array.length,
        capacity_before,
        capacity_after: array.capacity,
    })
}

fn link_for(links: &[RevealObjectLink], who: i16, o: i16) -> Option<RevealObjectLink> {
    links
        .iter()
        .copied()
        .find(|link| link.who == who && link.o == o)
}

fn terminal_from_cell(
    down: i16,
    down_who: i16,
    links: &[RevealObjectLink],
) -> Result<(i16, i16), SetupUnitVisibilityError> {
    let mut o = down;
    let mut who = down_who;
    let mut seen = HashSet::new();
    while o >= 0 {
        if !seen.insert((who, o)) {
            return Err(SetupUnitVisibilityError::ObjectChainCycle { who, o });
        }
        let link = link_for(links, who, o)
            .ok_or(SetupUnitVisibilityError::MissingObjectLink { who, o })?;
        o = link.next;
        who = link.next_who;
    }
    Ok((o, who))
}

fn fine_to_wcoord(fine: i32) -> i32 {
    don_sim::systems::map_terrain::div_3(fine >> 8)
}

fn good_at_cell(
    worldc: &World,
    goods: &OilGoodRuntime,
    links: &[RevealObjectLink],
    wx: i32,
    wy: i32,
) -> Result<Option<i32>, SetupUnitVisibilityError> {
    let cell = &worldc.wdata[worldc.w_index(wx, wy)];
    let (terminal, slot) = terminal_from_cell(cell.down, cell.down_who, links)?;
    if terminal != DOWN_GOOD || slot < 0 {
        return Ok(None);
    }
    let Some(good) = goods.slots.get(slot as usize) else {
        return Ok(None);
    };
    if !good.active() || !good.ptype_present || good.node.type_index == OIL_GOOD_TYPE {
        return Ok(None);
    }
    Ok(Some(i32::from(slot)))
}

fn item_at_cell(
    worldc: &World,
    items: &Items,
    links: &[RevealObjectLink],
    wx: i32,
    wy: i32,
) -> Result<Option<i32>, SetupUnitVisibilityError> {
    let cell = &worldc.wdata[worldc.w_index(wx, wy)];
    let (terminal, slot) = terminal_from_cell(cell.down, cell.down_who, links)?;
    if terminal != DOWN_ITEM || slot < 0 {
        return Ok(None);
    }
    Ok(match items.get(slot as usize) {
        Some(item) if item.is_valid() => Some(i32::from(slot)),
        _ => None,
    })
}

fn first_good_at_wcoord(goods: &OilGoodRuntime, wx: i32, wy: i32) -> Option<i32> {
    let end = goods.good_mark.max(0).min(goods.slots.len() as i32) as usize;
    goods.slots[..end]
        .iter()
        .enumerate()
        .find(|(_, good)| {
            good.active()
                && fine_to_wcoord(good.coord_x()) == wx
                && fine_to_wcoord(good.coord_y()) == wy
        })
        .map(|(slot, _)| slot as i32)
}

fn class_authority(
    authorities: &[RareGoodClassAuthority],
    slot: i32,
) -> Option<RareGoodClassAuthority> {
    authorities.iter().copied().find(|a| a.good_slot == slot)
}

struct TypeAvailCursor<'a> {
    calls: &'a [LeaderTypeAvailReceipt],
    at: usize,
}

impl<'a> TypeAvailCursor<'a> {
    fn take(&mut self, leader: u8, type_index: i32) -> Result<bool, SetupUnitVisibilityError> {
        let ordinal = self.at;
        let call = self
            .calls
            .get(ordinal)
            .copied()
            .ok_or(SetupUnitVisibilityError::MissingTypeAvail { ordinal })?;
        if call.call_va != LEADER_TYPE_AVAIL_CALL_VA
            || call.body_va != LEADER_TYPE_AVAIL_VA
            || call.leader != leader
            || call.type_index != type_index
            || call.strict != 1
        {
            return Err(SetupUnitVisibilityError::StaleTypeAvail { ordinal });
        }
        self.at += 1;
        Ok(call.available)
    }
}

fn apply_new_rare(
    source: u8,
    good_slot: i32,
    goods: &OilGoodRuntime,
    facts: &[RevealLeaderFacts; CHECKSUM_LEADER_SLOTS],
    dynamic: &mut DynamicLeadersAuthority,
    classes: &[RareGoodClassAuthority],
    type_avail: &mut TypeAvailCursor<'_>,
    adds: &mut Vec<RetailArrayAddReceipt>,
) -> Result<NewRareReceipt, SetupUnitVisibilityError> {
    let source_flags = facts[source as usize].leader_flags;
    if source_flags & 0x0c == 0x04 {
        return Ok(NewRareReceipt {
            call_va: LEADER_NEW_RARE_CALL_VA,
            body_va: LEADER_NEW_RARE_VA,
            source,
            good_slot,
            source_human_early_return: true,
            candidates: Vec::new(),
        });
    }
    let good = goods
        .slots
        .get(good_slot as usize)
        .ok_or(SetupUnitVisibilityError::StaleRareAuthority { slot: good_slot })?;
    let class = class_authority(classes, good_slot)
        .ok_or(SetupUnitVisibilityError::MissingRareAuthority { slot: good_slot })?;
    if !good.active()
        || !good.ptype_present
        || class.type_index != good.node.type_index
        || class.good_slot != good_slot
    {
        return Err(SetupUnitVisibilityError::StaleRareAuthority { slot: good_slot });
    }

    let mut candidates = Vec::with_capacity(CHECKSUM_LEADER_SLOTS);
    for candidate in 0..CHECKSUM_LEADER_SLOTS {
        let candidate_u8 = candidate as u8;
        let candidate_flags = facts[candidate].leader_flags;
        let decision = if candidate_flags & 1 == 0 {
            NewRareDecision::Inactive
        } else if candidate != source as usize
            && (facts[candidate].diplos[source as usize] != 2
                || facts[source as usize].diplos[candidate] != 2)
        {
            NewRareDecision::NotMutualAlly
        } else if candidate_flags & 4 != 0 && source_flags & 8 == 0 {
            NewRareDecision::HumanCandidateRejected
        } else if array_contains(&dynamic.rows[candidate].new_rares, good_slot) {
            NewRareDecision::AlreadyKnown
        } else if !type_avail.take(candidate_u8, class.type_index)? {
            NewRareDecision::TypeUnavailable
        } else if class.is_type_6 {
            NewRareDecision::ExcludedType6
        } else if class.is_type_31 {
            NewRareDecision::ExcludedType31
        } else {
            let add = array_add(
                &mut dynamic.rows[candidate].new_rares,
                candidate_u8,
                LeaderArrayField::NewRares,
                good_slot,
            )?;
            adds.push(add);
            NewRareDecision::Added
        };
        candidates.push(NewRareCandidateReceipt {
            leader: candidate_u8,
            decision,
        });
    }
    Ok(NewRareReceipt {
        call_va: LEADER_NEW_RARE_CALL_VA,
        body_va: LEADER_NEW_RARE_VA,
        source,
        good_slot,
        source_human_early_return: false,
        candidates,
    })
}

fn mark_bit(byte: &mut u8, who: u8) -> (u8, u8) {
    let before = *byte;
    *byte |= (1u32.wrapping_shl(u32::from(who) & 0x1f)) as u8;
    (before, *byte)
}

#[allow(clippy::too_many_arguments)]
fn reveal_one(
    ordinal: u16,
    fx: i32,
    fy: i32,
    request: VisibilityUpdateRequest,
    world: &World,
    worldc: &World,
    goods: &mut OilGoodRuntime,
    items: &mut Items,
    dynamic: &mut DynamicLeadersAuthority,
    authority: &SetupUnitVisibilityAuthority,
    type_avail: &mut TypeAvailCursor<'_>,
    adds: &mut Vec<RetailArrayAddReceipt>,
    auto: &mut Vec<AutoExploreGoodyRequest>,
) -> Result<RevealFogReceipt, SetupUnitVisibilityError> {
    let who = request.owner;
    let wx = fx >> 1;
    let wy = fy >> 1;
    let tx = fx.wrapping_mul(2).wrapping_add(1);
    let ty = fy.wrapping_mul(2).wrapping_add(1);
    let current_resource_tile = world.tdata[world.t_index(tx, ty)] & tflag::RESOURCE != 0;
    let canonical_resource_tile = worldc.tdata[worldc.t_index(tx, ty)] & tflag::RESOURCE != 0;

    let mut rare = RareRevealReceipt {
        current_resource_tile,
        canonical_resource_tile,
        find_good_call_va: None,
        find_good_body_va: None,
        good_slot: None,
        new_rare: None,
        ever_seen_before: None,
        ever_seen_after: None,
    };
    if current_resource_tile && canonical_resource_tile {
        rare.find_good_call_va = Some(OBJECTS_FIND_GOOD_AT_CALL_VA);
        rare.find_good_body_va = Some(OBJECTS_FIND_GOOD_AT_VA);
        if let Some(slot) = good_at_cell(worldc, goods, &authority.object_links, wx, wy)? {
            rare.good_slot = Some(slot);
            rare.new_rare = Some(apply_new_rare(
                who,
                slot,
                goods,
                &authority.leaders,
                dynamic,
                &authority.rare_goods,
                type_avail,
                adds,
            )?);
            let good = &mut goods.slots[slot as usize];
            let (before, after) = mark_bit(&mut good.node.ever_seen, who);
            rare.ever_seen_before = Some(before);
            rare.ever_seen_after = Some(after);
        }
    }

    let oil_cell = world.wdata[world.w_index(wx, wy)].flags & wflag::OIL != 0;
    let mut oil = OilRevealReceipt {
        oil_cell,
        good_slot: None,
        already_known: false,
        add: None,
    };
    if oil_cell {
        if let Some(slot) = first_good_at_wcoord(goods, wx, wy) {
            oil.good_slot = Some(slot);
            let row = &mut dynamic.rows[who as usize].oil_patches;
            oil.already_known = array_contains(row, slot);
            if !oil.already_known {
                let add = array_add(row, who, LeaderArrayField::OilPatches, slot)?;
                oil.add = Some(add);
                adds.push(add);
            }
        }
    }

    let current = &world.wdata[world.w_index(wx, wy)];
    let canonical = &worldc.wdata[worldc.w_index(wx, wy)];
    let item_cell = current.flags & WFLAG_ITEM != 0;
    let canonical_land_gate = canonical.flags & WFLAG_OVERRIDE_LAND != 0
        || (canonical.land != land::COASTAL && canonical.land != land::OCEAN);
    let mut item = ItemRevealReceipt {
        item_cell,
        canonical_land_gate,
        find_goody_call_va: None,
        find_goody_body_va: None,
        item_slot: None,
        ever_seen_before: None,
        ever_seen_after: None,
    };
    let mut auto_explore = None;
    if item_cell {
        if canonical_land_gate {
            item.find_goody_call_va = Some(OBJECTS_FIND_GOODY_AT_CALL_VA);
            item.find_goody_body_va = Some(OBJECTS_FIND_GOODY_AT_VA);
            if let Some(slot) = item_at_cell(worldc, items, &authority.object_links, wx, wy)? {
                item.item_slot = Some(slot);
                let target = items.get_mut(slot as usize).expect("validated item slot");
                let before = target.ever_seen;
                target.mark_seen(who);
                item.ever_seen_before = Some(before);
                item.ever_seen_after = Some(target.ever_seen);
            }
        }
        if request.unit_masks & UNIT_AUTO_EXPLORE != 0 {
            let req = AutoExploreGoodyRequest {
                ordinal: auto.len() as u16,
                reveal_ordinal: ordinal,
                call_va: UNIT_GET_GOODY_BOX_CALL_VA,
                body_va: UNIT_GET_GOODY_BOX_VA,
                who,
                o: request.o,
                world_x: wx,
                world_y: wy,
                target_fine_x: wx.wrapping_mul(0x300).wrapping_add(0x180),
                target_fine_y: wy.wrapping_mul(0x300).wrapping_add(0x180),
                group_clear_va: GROUP_CLEAR_VA,
                group_add_va: GROUP_ADD_VA,
                groups_push_group_va: GROUPS_PUSH_GROUP_VA,
                group_action_move_to_va: GROUP_ACTION_MOVE_TO_VA,
                queue_pos: 0,
                order_index: 3,
            };
            auto.push(req);
            auto_explore = Some(req);
        }
    }

    Ok(RevealFogReceipt {
        ordinal,
        call_va: WORLD_REVEAL_FOG_CALL_VA,
        body_va: WORLD_REVEAL_FOG_VA,
        fog_x: fx,
        fog_y: fy,
        world_x: wx,
        world_y: wy,
        rare,
        oil,
        item,
        auto_explore,
    })
}

fn project(x: i32, y: i32, angle: i32, distance: i32) -> (i32, i32) {
    (
        x.wrapping_add(sinx(angle, distance)),
        y.wrapping_sub(cosx(angle, distance)),
    )
}

#[allow(clippy::too_many_arguments)]
fn produce_from_seam(
    world: &mut World,
    worldc: &World,
    goods: &mut OilGoodRuntime,
    items: &mut Items,
    dynamic: &mut DynamicLeadersAuthority,
    seam: FreshVisibilitySeam,
    authority: SetupUnitVisibilityAuthority,
) -> Result<SetupUnitVisibilityReceipt, SetupUnitVisibilityError> {
    if !validate_world(world) {
        return Err(SetupUnitVisibilityError::InvalidWorld);
    }
    if !validate_world(worldc) {
        return Err(SetupUnitVisibilityError::InvalidCanonicalWorld);
    }
    if (
        worldc.xs,
        worldc.ys,
        worldc.fog_xs,
        worldc.fog_ys,
        worldc.tile_xs,
        worldc.tile_ys,
    ) != (
        world.xs,
        world.ys,
        world.fog_xs,
        world.fog_ys,
        world.tile_xs,
        world.tile_ys,
    ) {
        return Err(SetupUnitVisibilityError::InvalidCanonicalWorld);
    }
    if goods.good_mark < 0 || goods.good_mark as usize > goods.slots.len() {
        return Err(SetupUnitVisibilityError::InvalidGoods);
    }
    if authority.leaders[seam.request.owner as usize].leader_flags as u32 != seam.leader_flags_after
        || authority.los.mylos != seam.request.source_mylos
    {
        return Err(SetupUnitVisibilityError::InvalidFreshUnitSeam);
    }
    for (leader, row) in dynamic.rows.iter().enumerate() {
        for (field, array) in [
            (LeaderArrayField::NewRares, &row.new_rares),
            (LeaderArrayField::OilPatches, &row.oil_patches),
        ] {
            if !validate_array(array) {
                return Err(SetupUnitVisibilityError::InvalidLeaderArray {
                    leader: leader as u8,
                    field,
                });
            }
        }
    }
    let mut link_keys = HashSet::new();
    for link in &authority.object_links {
        if link.who < 0 || link.o < 0 || !link_keys.insert((link.who, link.o)) {
            return Err(SetupUnitVisibilityError::DuplicateObjectLink {
                who: link.who,
                o: link.o,
            });
        }
    }
    let mut rare_slots = HashSet::new();
    for class in &authority.rare_goods {
        if class.good_slot < 0 || !rare_slots.insert(class.good_slot) {
            return Err(SetupUnitVisibilityError::DuplicateRareAuthority {
                slot: class.good_slot,
            });
        }
    }

    let los = resolve_unit_los(authority.los)?;
    if los < 0 || los > i32::MAX / 0xc0 {
        return Err(SetupUnitVisibilityError::LosOutOfRange { los });
    }
    let radius = (los * 0xc0) / 0x180;
    let projected_small_los = los > 0
        && radius <= 3
        && seam.request.type_domain == 0
        && seam.request.type_unit_flags2 & 4 == 0
        && seam.request.unit_masks & 1 == 0;
    let (stamp_x, stamp_y) = if projected_small_los {
        project(seam.request.x, seam.request.y, seam.request.angle, 0x180)
    } else {
        (seam.request.x, seam.request.y)
    };

    // Every synchronized owner is staged together.  `Object::init` at 0x006477A1 wrote
    // `visible=0, launch_frames=0`, UnitData::is_wonder is the folded false stub, and the
    // request is `incremental=0`; therefore the vtable +0x164 `Unit::update_local_seen`
    // call at 0x00651DFD is provably skipped for this fresh cohort.
    let mut next_world = world.clone();
    let mut next_goods = goods.clone();
    let mut next_items = items.clone();
    let mut next_dynamic = dynamic.clone();

    let circle = CircleTable::build();
    let fog = Fog::new();
    let seeing = SeeingObject {
        fine_x: stamp_x,
        fine_y: stamp_y,
        owner: seam.request.owner,
        los_tiles: los,
        detector: seam.request.object_flags & 0x40 != 0,
        grant_seen2_to: 0,
    };
    let mut newly_explored = Vec::new();
    update_seen(&fog, &mut next_world, &circle, &seeing, &mut newly_explored);

    let mut type_avail = TypeAvailCursor {
        calls: &authority.type_avail_calls,
        at: 0,
    };
    let mut reveals = Vec::with_capacity(newly_explored.len());
    let mut adds = Vec::new();
    let mut auto = Vec::new();
    for (ordinal, &(fx, fy)) in newly_explored.iter().enumerate() {
        reveals.push(reveal_one(
            ordinal as u16,
            fx,
            fy,
            seam.request,
            &next_world,
            worldc,
            &mut next_goods,
            &mut next_items,
            &mut next_dynamic,
            &authority,
            &mut type_avail,
            &mut adds,
            &mut auto,
        )?);
    }
    if type_avail.at != authority.type_avail_calls.len() {
        return Err(SetupUnitVisibilityError::UnusedTypeAvail {
            first_unused: type_avail.at,
        });
    }

    let good_ever_seen_mutations = reveals
        .iter()
        .filter(|r| r.rare.ever_seen_before != r.rare.ever_seen_after)
        .count();
    let item_ever_seen_mutations = reveals
        .iter()
        .filter(|r| r.item.ever_seen_before != r.item.ever_seen_after)
        .count();
    let next_external_residual = if auto.is_empty() {
        SetupVisibilityExternalResidual::None
    } else {
        SetupVisibilityExternalResidual::GroupsAutoExplore {
            first_call_va: UNIT_GET_GOODY_BOX_CALL_VA,
            first_body_va: UNIT_GET_GOODY_BOX_VA,
            requests: auto,
        }
    };

    *world = next_world;
    *goods = next_goods;
    *items = next_items;
    *dynamic = next_dynamic;

    Ok(SetupUnitVisibilityReceipt {
        object_update_seen_call_va: seam.request.call_va,
        object_update_seen_body_va: seam.request.body_va,
        unit_los_body_va: UNIT_DATA_LOS_VA,
        unit_is_wonder_body_va: UNIT_DATA_IS_WONDER_VA,
        resolved_los_tiles: los,
        projected_small_los,
        project_body_va: projected_small_los.then_some(PROJECT_VA),
        stamp_fine_x: stamp_x,
        stamp_fine_y: stamp_y,
        object_visible_before: 0,
        object_visible_after: 0,
        update_local_seen_skipped: true,
        update_local_seen_body_va: UNIT_UPDATE_LOCAL_SEEN_VA,
        set_seen2_body_va: WORLD_SET_SEEN2_VA,
        newly_explored,
        reveals,
        leader_array_adds: adds,
        good_ever_seen_mutations,
        item_ever_seen_mutations,
        rng_before: seam.rng,
        rng_after: seam.rng,
        next_external_residual,
    })
}

/// Apply the complete fresh normal-land setup visibility transaction atomically.
#[allow(clippy::too_many_arguments)]
pub fn produce_setup_unit_visibility(
    world: &mut World,
    worldc: &World,
    goods: &mut OilGoodRuntime,
    items: &mut Items,
    dynamic: &mut DynamicLeadersAuthority,
    tail: &UnitInitCollisionTailReceipt,
    authority: SetupUnitVisibilityAuthority,
) -> Result<SetupUnitVisibilityReceipt, SetupUnitVisibilityError> {
    produce_from_seam(
        world,
        worldc,
        goods,
        items,
        dynamic,
        bind_seam(tail)?,
        authority,
    )
}

#[cfg(test)]
pub fn produce_setup_unit_visibility_test_seam(
    world: &mut World,
    worldc: &World,
    goods: &mut OilGoodRuntime,
    items: &mut Items,
    dynamic: &mut DynamicLeadersAuthority,
    request: VisibilityUpdateRequest,
    leader_flags_after: u32,
    rng: i32,
    authority: SetupUnitVisibilityAuthority,
) -> Result<SetupUnitVisibilityReceipt, SetupUnitVisibilityError> {
    produce_from_seam(
        world,
        worldc,
        goods,
        items,
        dynamic,
        FreshVisibilitySeam {
            request,
            leader_flags_after,
            rng,
        },
        authority,
    )
}

/// Source-bound executable/PDB identity.  No replay wire checksum is an input.
pub const RETAIL_EXE_SHA256: &str =
    "30478a44bb1f5697c9c294367344d2231ae35db8c35edddc03204ec0f625079";
pub const RETAIL_PDB_SHA256: &str =
    "334a3e976dc6d33feb83f41c4afcbb250a8c9d20fe5c5938645100998d9bff5";
