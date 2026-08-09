//! Decoding a factored action into engine commands, and applying it.
//!
//! # Why factored, and why masked per parameter
//!
//! The engine enumerates its own action space twice: 82 `CommandTypes` opcodes on the
//! wire, and 28 `OrderIndex` order kinds inside `UnitOrder`. Flattening that into one
//! categorical is not merely large, it is *wrong shaped*: `Attack(target=17)` and
//! `Attack(target=18)` share everything except one parameter, and a flat head cannot
//! generalise across them. So the action is a `MultiDiscrete` over ten heads, and each
//! head gets its own mask.
//!
//! The masking is load-bearing rather than decorative. The microRTS ablation is the
//! reference point: with full parameter-level invalid-action masking the agent reaches a
//! 0.82 win rate against the built-in AI; with masking removed it reaches 0.00. So a head
//! whose mask is not derivable from state is a head that should not exist yet — which is
//! why the unsupplied command fields are enumerated in `generated.rs` instead of being
//! given arbitrary heads.

use crate::generated as g;
use crate::spec::{EnvConfig, AMOUNT_BUCKETS, COUNT_BUCKETS};
use crate::state::{EnvWorld, GatherHost, GatherHostError};
use crate::typecaps::{F_BUILDING, F_CIVILIAN, F_PRODUCER};
use don_sim::command::QueuePos;
use don_sim::systems::order_dispatch::OrderRec;
use don_sim::world::SUBTILE;

/// One entity's action, as the ten head values.
#[derive(Clone, Copy, Debug, Default)]
pub struct UnitAction {
    pub verb: u16,
    pub target_x: u16,
    pub target_y: u16,
    pub target_entity: u16,
    pub type_index: u16,
    pub queue_pos: u16,
    pub stance: u16,
    pub form: u16,
    pub order_mods: u16,
    pub count: u16,
}

impl UnitAction {
    pub fn from_slice(v: &[i32]) -> UnitAction {
        let g_ = |i: usize| v.get(i).copied().unwrap_or(0).max(0) as u16;
        UnitAction {
            verb: g_(0),
            target_x: g_(1),
            target_y: g_(2),
            target_entity: g_(3),
            type_index: g_(4),
            queue_pos: g_(5),
            stance: g_(6),
            form: g_(7),
            order_mods: g_(8),
            count: g_(9),
        }
    }
}

/// One player's global action, as the five head values.
#[derive(Clone, Copy, Debug, Default)]
pub struct PlayerAction {
    pub verb: u16,
    pub target_player: u16,
    pub good: u16,
    pub amount: u16,
    pub treaty: u16,
}

impl PlayerAction {
    pub fn from_slice(v: &[i32]) -> PlayerAction {
        let g_ = |i: usize| v.get(i).copied().unwrap_or(0).max(0) as u16;
        PlayerAction {
            verb: g_(0),
            target_player: g_(1),
            good: g_(2),
            amount: g_(3),
            treaty: g_(4),
        }
    }
}

/// What happened to one applied action. Counted, so a run can report the rate at which a
/// policy is emitting actions the env silently drops.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ApplyStats {
    pub noop: u32,
    pub applied: u32,
    /// The acting or target entity died earlier in the same step. Not a policy error: the
    /// action was legal when the observation was taken.
    pub stale: u32,
    /// Verb accepted, but this build has no dynamics for it.
    pub accepted_no_effect: u32,
    /// Every head value was individually legal but their conjunction is not satisfiable —
    /// the exact residue the factored form cannot express (see `mask::CROSS_HEAD_GAPS`).
    /// Treated as a no-op. Counted separately so the size of that residue is measurable
    /// rather than assumed small.
    pub incoherent: u32,
    /// Rejected: the action violated its own mask (a policy bug, or an unmasked sampler).
    pub illegal: u32,
}

/// Return the order retail installs for either patrol wire command.
///
/// `Group::action_patrol` `0x007030C0` calls `Unit::add_patrol_order` for ordinary
/// units, but delegates true planes to `Group::action_air_patrol` `0x007029D0`.
/// `Group::action_launch_patrol` `0x00703580` only reaches
/// `Unit::add_air_patrol_order` for true planes.  The two allocation sites request
/// orders 22 (`GROUP_PATROL`) and 17 (`AIR_PATROL`) respectively.  In particular,
/// neither path constructs the dead `OrderIndex::Patrol` arm (5).
pub fn patrol_order_for_opcode(opcode: u8, is_plane: bool) -> Option<g::OrderIndex> {
    match (opcode, is_plane) {
        (10, false) => Some(g::OrderIndex::GroupPatrol),
        (10 | 11, true) => Some(g::OrderIndex::AirPatrol),
        _ => None,
    }
}

impl ApplyStats {
    pub fn add(&mut self, o: &ApplyStats) {
        self.noop += o.noop;
        self.applied += o.applied;
        self.stale += o.stale;
        self.incoherent += o.incoherent;
        self.accepted_no_effect += o.accepted_no_effect;
        self.illegal += o.illegal;
    }
}

/// Apply one entity action.
///
/// `actor` and the `TargetEntity` head are resolved through `don-sim` handles taken when
/// the observation was written, so an entity that died earlier in the same step yields
/// `stale` rather than silently addressing whichever entity was compacted into its row.
pub fn apply_unit(
    w: &mut EnvWorld,
    cfg: &EnvConfig,
    who: u8,
    actor: don_sim::Handle,
    a: UnitAction,
    st: &mut ApplyStats,
) {
    if a.verb == 0 {
        st.noop += 1;
        return;
    }
    let Some(row) = w.sim.row_of(actor) else {
        st.stale += 1;
        return;
    };
    let vi = (a.verb - 1) as usize;
    if vi >= g::N_UNIT_VERBS || w.sim.owner()[row] != who as i8 {
        st.illegal += 1;
        return;
    }
    let tx = (a.target_x as i32).min(cfg.grid_w as i32 - 1) * SUBTILE + SUBTILE / 2;
    let ty = (a.target_y as i32).min(cfg.grid_h as i32 - 1) * SUBTILE + SUBTILE / 2;
    let target_row = if a.target_entity == 0 {
        None
    } else {
        w.obs_ents[who as usize]
            .get((a.target_entity - 1) as usize)
            .and_then(|h| w.sim.row_of(*h))
    };
    let queue = QueuePos::from_i64(a.queue_pos as i64);

    match vi {
        g::uv::MOVE_TO | g::uv::MOVE_NEAR => {
            if w.speed[row] <= 0 {
                st.illegal += 1;
                return;
            }
            w.dest_x[row] = tx;
            w.dest_y[row] = ty;
            w.install_order(row, OrderRec::move_to(tx, ty, 0), queue);
            st.applied += 1;
        }
        g::uv::PATROL | g::uv::LAUNCH_PATROL => {
            if w.speed[row] <= 0 {
                st.illegal += 1;
                return;
            }
            // The permissive fallback has no UnitData::is_plane evidence. Guessing here
            // would silently turn the same command into different retail order classes,
            // so leave it visibly unimplemented until the derived type table is present.
            if w.rules.caps.is_permissive() {
                w.unimplemented.unit[vi] += 1;
                st.accepted_no_effect += 1;
                return;
            }
            let opcode = g::UNIT_VERBS[vi].opcode;
            let Some(order) = patrol_order_for_opcode(opcode, w.cap(w.type_index[row]).is_plane)
            else {
                // Retail's launch-patrol group walk ignores non-plane members. The mask
                // never offers this conjunction; classify an unmasked request as illegal
                // instead of manufacturing a ground order.
                st.illegal += 1;
                return;
            };
            match order {
                g::OrderIndex::GroupPatrol => {
                    w.install_group_patrol_order(row, tx, ty, queue);
                }
                g::OrderIndex::AirPatrol => {
                    w.install_air_patrol_order(row, tx, ty, queue);
                }
                _ => unreachable!("patrol router only returns executable patrol classes"),
            }
            st.applied += 1;
        }
        g::uv::ATTACK | g::uv::SIEGE_ATTACK | g::uv::SWARM_AROUND => {
            let Some(tr) = target_row else {
                // No target selected (the head's only legal value when the world holds no
                // hostile), or a masked-in target that has since died.
                if a.target_entity == 0 {
                    st.incoherent += 1
                } else {
                    st.stale += 1
                }
                return;
            };
            if w.attack[row] <= 0 || tr == row {
                st.illegal += 1;
                return;
            }
            w.target[row] = w.handle_at(tr);
            let target_who = w.sim.owner()[tr] as i32;
            let target_o = w.sim.units.o()[tr] as i32;
            let target_uid = w.sim.units.uid()[tr] as u16;
            w.install_order(
                row,
                OrderRec::attack(target_who, target_o, target_uid),
                queue,
            );
            st.applied += 1;
        }
        g::uv::HALT => {
            w.clear_orders(row);
            let (px, py) = (w.sim.pos_x()[row], w.sim.pos_y()[row]);
            w.dest_x[row] = px;
            w.dest_y[row] = py;
            st.applied += 1;
        }
        g::uv::STANCE => {
            w.stance[row] = a.stance.min(3) as u8;
            st.applied += 1;
        }
        g::uv::FORM => {
            w.form[row] = g::FORMS[(a.form as usize).min(g::FORMS.len() - 1)].1;
            st.applied += 1;
        }
        g::uv::DISBAND => {
            let h = w.handle_at(row);
            w.despawn(h);
            st.applied += 1;
        }
        g::uv::QUEUE_UP => {
            // Producers instantiate immediately: the real build queue (`BuildQueue`,
            // `Unit::queue_time`, ramping via `JOB_EXTRA_TIME`) is not derived, so a
            // timed queue here would be invented. Cost and pop are enforced, because both
            // come from the shipped tables.
            let t = a.type_index;
            let pt = w.type_index[row];
            if !w.cap(pt).has(F_PRODUCER) || !bit(w.rules.caps.produces(pt), t as usize) {
                st.illegal += 1;
                return;
            }
            let n = COUNT_BUCKETS[(a.count as usize).min(COUNT_BUCKETS.len() - 1)];
            let c = *w.cap(t);
            let p = who as usize;
            let mut made = 0;
            for _ in 0..n {
                if !w.players[p].can_afford(&c.cost)
                    || w.players[p].pop + c.pop.max(1) as i32 > w.players[p].pop_cap
                {
                    break;
                }
                let cost = c.cost;
                w.players[p].pay_public(&cost);
                let (px, py) = (w.sim.pos_x()[row], w.sim.pos_y()[row]);
                if w.spawn(who, t, px + SUBTILE, py).is_some() {
                    w.players[p].units_built += 1;
                    made += 1;
                }
            }
            if made > 0 {
                st.applied += 1
            } else {
                // Producible and affordable when the mask was written; another entity of
                // the same player spent the resources first. A race, not a policy error.
                st.stale += 1
            }
        }
        g::uv::BUILD => {
            let t = a.type_index;
            let c = *w.cap(t);
            let p = who as usize;
            if !c.has(F_BUILDING) || !w.cap(w.type_index[row]).has(F_CIVILIAN) {
                st.illegal += 1;
                return;
            }
            if !w.players[p].can_afford(&c.cost) {
                st.stale += 1;
                return;
            }
            let cost = c.cost;
            w.players[p].pay_public(&cost);
            // Instant completion: `Build::construct_hits` / `WallData::construct_time`
            // exist in the walked state but their progression is underived.
            if w.spawn(who, t, tx, ty).is_some() {
                w.players[p].buildings_built += 1;
                st.applied += 1;
            } else {
                st.stale += 1; // world at entity capacity
            }
        }
        _ => {
            w.unimplemented.unit[vi] += 1;
            st.accepted_no_effect += 1;
        }
    }
}

/// Apply one action through the explicit Farm gathering provider when its verb is GATHER.
///
/// The ordinary [`apply_unit`] path remains unchanged and Gather stays masked there. This
/// entrypoint exists so an environment with authoritative target/update/evaluator/leader
/// hosts can admit the recovered transaction without installing a guessed default provider.
pub fn apply_unit_with_gather_host(
    w: &mut EnvWorld,
    cfg: &EnvConfig,
    who: u8,
    actor: don_sim::Handle,
    a: UnitAction,
    host: &mut dyn GatherHost,
    st: &mut ApplyStats,
) -> Result<(), GatherHostError> {
    let gather_verb = (g::uv::GATHER + 1) as u16;
    if a.verb != gather_verb {
        apply_unit(w, cfg, who, actor, a, st);
        return Ok(());
    }
    let Some(row) = w.sim.row_of(actor) else {
        st.stale += 1;
        return Ok(());
    };
    if w.sim.owner()[row] != who as i8 {
        st.illegal += 1;
        return Ok(());
    }
    if a.target_entity == 0 {
        st.incoherent += 1;
        return Ok(());
    }
    let Some(target_row) = w.obs_ents[who as usize]
        .get((a.target_entity - 1) as usize)
        .and_then(|handle| w.sim.row_of(*handle))
    else {
        st.stale += 1;
        return Ok(());
    };
    host.preflight(w)?;
    let target = host.farm_target(w, row, target_row)?;
    w.install_farm_gather(
        row,
        target_row,
        target,
        QueuePos::from_i64(a.queue_pos as i64),
    )?;
    st.applied += 1;
    Ok(())
}

pub fn apply_player(w: &mut EnvWorld, who: u8, a: PlayerAction, st: &mut ApplyStats) {
    if a.verb == 0 {
        st.noop += 1;
        return;
    }
    let vi = (a.verb - 1) as usize;
    if vi >= g::N_PLAYER_VERBS {
        st.illegal += 1;
        return;
    }
    let me = who as usize;
    let them = (a.target_player as usize).min(g::NUM_PLAYERS - 1);
    match vi {
        g::pv::TREATY | g::pv::DECLARE => {
            if them == me {
                // Only reachable when every other player is dead, which is the one case
                // the TargetPlayer mask cannot exclude without becoming all-zero.
                st.incoherent += 1;
                return;
            }
            let d = a.treaty.min(2) as u8;
            w.players[me].diplos[them] = d;
            // `process_declare` is unilateral, `process_treaty` needs an Accept; both are
            // applied unilaterally here and that difference is not modelled.
            if vi == g::pv::DECLARE && d == 0 {
                w.players[them].diplos[me] = 0;
            }
            st.applied += 1;
        }
        g::pv::TRIBUTE => {
            let k = (a.good as usize).min(g::NUM_COMMON - 1);
            let amt = AMOUNT_BUCKETS[(a.amount as usize).min(AMOUNT_BUCKETS.len() - 1)];
            if them == me || w.players[me].econ[k] < amt {
                // Good x Amount: each head is masked against `max(econ)`, so a
                // (good, amount) pair can be individually legal and jointly unaffordable.
                st.incoherent += 1;
                return;
            }
            w.players[me].econ[k] -= amt;
            w.players[them].econ[k] += amt;
            st.applied += 1;
        }
        g::pv::RESIGN => {
            w.players[me].alive = false;
            w.players[me].defeat_type = 2; // resigned
            st.applied += 1;
        }
        _ => {
            w.unimplemented.player[vi] += 1;
            st.accepted_no_effect += 1;
        }
    }
}

#[inline]
fn bit(b: &[u8], i: usize) -> bool {
    i / 8 < b.len() && b[i / 8] & (1 << (i % 8)) != 0
}
