//! The arena's command set — and the reason a learned policy is a drop-in.
//!
//! # The contract
//!
//! Every actor in the arena, scripted or learned, changes the world through
//! [`World::submit`](super::world::World::submit) and nothing else. That much
//! [`crate::orders::Order`] already established for the economic game. What is new here
//! is that each variant carries **the `don-env` unit verb it is**, so the mapping to the
//! RL action space is not a promise in a doc — it is [`Cmd::verb`], and a test asserts
//! every variant names a verb that exists in `don_env::generated::UNIT_VERBS`.
//!
//! `don-env` hands a policy a `MultiDiscrete` of ten heads per entity
//! (`Verb, TargetX, TargetY, TargetEntity, Type, QueuePos, Stance, Form, OrderMods,
//! Count`). [`Cmd::to_heads`] emits exactly that vector and [`Cmd::from_heads`] reads it
//! back, so:
//!
//! * a heuristic bot's decision can be *logged as an RL action* — behaviour cloning data
//!   with no translation layer, and
//! * a trained policy's output can be *executed by the arena* with no translation layer.
//!
//! Entity references cross that boundary as **observation slots**, not entity ids,
//! because that is what `don-env`'s `TargetEntity` head is: an index into the entity list
//! the observation was written from, `0` meaning "none". The two conversion functions
//! therefore take the same slot map the observation used.
//!
//! # What is deliberately not expressible
//!
//! Six verbs, out of `don-env`'s 33. The arena has no transports, no garrisons, no
//! formations, no spells and no air, so a `Cmd` for those would be a head the world
//! cannot honour — the mistake `don-env`'s own doc comment warns about ("a head whose
//! mask is not derivable from state is a head that should not exist yet").

use don_env::generated as g;

/// A nonzero handle to an entity. Zero is "none", matching the engine's convention that
/// a valid object id is never 0 — the shipped scripts test ids with bare truthiness.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct EntId(pub u32);

impl EntId {
    pub const NONE: EntId = EntId(0);
    pub fn is_none(self) -> bool {
        self.0 == 0
    }
    pub fn index(self) -> Option<usize> {
        if self.0 == 0 {
            None
        } else {
            Some(self.0 as usize - 1)
        }
    }
    pub fn from_index(i: usize) -> EntId {
        EntId(i as u32 + 1)
    }
}

/// One command. Six variants, each one `don-env` unit verb.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cmd {
    /// `QUEUE_UP` — put a unit **or a technology** on a producer's queue. Both go through
    /// one verb because in the engine both are queued at a building: a `TechType`'s
    /// `WHERE` is the Library exactly as a `Hoplites`' `WHERE` is the Barracks.
    Queue {
        producer: EntId,
        type_id: i32,
        count: u16,
    },
    /// `BUILD` — a citizen founds a building (or a city) at a tile and starts working it.
    Build {
        worker: EntId,
        type_id: i32,
        tx: i32,
        ty: i32,
    },
    /// `MOVE_TO`.
    Move { unit: EntId, tx: i32, ty: i32 },
    /// `ATTACK`.
    Attack { unit: EntId, target: EntId },
    /// `GATHER` — seat a citizen in a gatherer building's worker slot.
    Gather { unit: EntId, target: EntId },
    /// `REPAIR` — put a citizen to work on a building site (construction or repair).
    Work { unit: EntId, target: EntId },
    /// `HALT`.
    Halt { unit: EntId },
}

impl Cmd {
    /// The `don-env` unit-verb index this command is.
    pub fn verb(&self) -> usize {
        match self {
            Cmd::Queue { .. } => g::uv::QUEUE_UP,
            Cmd::Build { .. } => g::uv::BUILD,
            Cmd::Move { .. } => g::uv::MOVE_TO,
            Cmd::Attack { .. } => g::uv::ATTACK,
            Cmd::Gather { .. } => g::uv::GATHER,
            Cmd::Work { .. } => g::uv::REPAIR,
            Cmd::Halt { .. } => g::uv::HALT,
        }
    }

    /// The verb's name in the engine's own opcode table.
    pub fn verb_name(&self) -> &'static str {
        g::UNIT_VERBS[self.verb()].name
    }

    /// The entity that acts. `don-env` addresses actions *per entity*, so every command
    /// has one.
    pub fn actor(&self) -> EntId {
        match *self {
            Cmd::Queue { producer, .. } => producer,
            Cmd::Build { worker, .. } => worker,
            Cmd::Move { unit, .. }
            | Cmd::Attack { unit, .. }
            | Cmd::Gather { unit, .. }
            | Cmd::Work { unit, .. }
            | Cmd::Halt { unit } => unit,
        }
    }

    /// The ten head values `don-env` would carry this action in.
    ///
    /// `slot` maps an entity to its index in the observation the policy saw, `+1`
    /// (`don-env`'s `TargetEntity` convention, where 0 is "no target").
    pub fn to_heads(&self, slot: impl Fn(EntId) -> u16) -> [i32; g::N_UNIT_HEADS] {
        let mut h = [0i32; g::N_UNIT_HEADS];
        h[g::UnitHead::Verb as usize] = self.verb() as i32 + 1; // 0 is NOOP
        match *self {
            Cmd::Queue { type_id, count, .. } => {
                h[g::UnitHead::Type as usize] = type_id;
                h[g::UnitHead::Count as usize] = count as i32;
            }
            Cmd::Build {
                type_id, tx, ty, ..
            } => {
                h[g::UnitHead::Type as usize] = type_id;
                h[g::UnitHead::TargetX as usize] = tx;
                h[g::UnitHead::TargetY as usize] = ty;
            }
            Cmd::Move { tx, ty, .. } => {
                h[g::UnitHead::TargetX as usize] = tx;
                h[g::UnitHead::TargetY as usize] = ty;
            }
            Cmd::Attack { target, .. } | Cmd::Gather { target, .. } | Cmd::Work { target, .. } => {
                h[g::UnitHead::TargetEntity as usize] = slot(target) as i32;
            }
            Cmd::Halt { .. } => {}
        }
        h
    }

    /// The inverse of [`Cmd::to_heads`]. `unslot` is the observation's slot -> entity map.
    ///
    /// Returns `None` for a verb the arena has no dynamics for, which is the honest
    /// answer: `don-env` accepts 33 verbs and this world implements 7.
    pub fn from_heads(actor: EntId, h: &[i32], unslot: impl Fn(u16) -> EntId) -> Option<Cmd> {
        let v = *h.first()?;
        if v <= 0 {
            return None;
        }
        let vi = (v - 1) as usize;
        let get = |i: g::UnitHead| h.get(i as usize).copied().unwrap_or(0);
        let tgt = || unslot(get(g::UnitHead::TargetEntity).max(0) as u16);
        Some(match vi {
            g::uv::QUEUE_UP => Cmd::Queue {
                producer: actor,
                type_id: get(g::UnitHead::Type),
                count: get(g::UnitHead::Count).clamp(0, u16::MAX as i32) as u16,
            },
            g::uv::BUILD => Cmd::Build {
                worker: actor,
                type_id: get(g::UnitHead::Type),
                tx: get(g::UnitHead::TargetX),
                ty: get(g::UnitHead::TargetY),
            },
            g::uv::MOVE_TO => Cmd::Move {
                unit: actor,
                tx: get(g::UnitHead::TargetX),
                ty: get(g::UnitHead::TargetY),
            },
            g::uv::ATTACK => Cmd::Attack {
                unit: actor,
                target: tgt(),
            },
            g::uv::GATHER => Cmd::Gather {
                unit: actor,
                target: tgt(),
            },
            g::uv::REPAIR => Cmd::Work {
                unit: actor,
                target: tgt(),
            },
            g::uv::HALT => Cmd::Halt { unit: actor },
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all() -> Vec<Cmd> {
        vec![
            Cmd::Queue {
                producer: EntId(1),
                type_id: 170,
                count: 3,
            },
            Cmd::Build {
                worker: EntId(2),
                type_id: 417,
                tx: 10,
                ty: 11,
            },
            Cmd::Move {
                unit: EntId(3),
                tx: 4,
                ty: 5,
            },
            Cmd::Attack {
                unit: EntId(3),
                target: EntId(9),
            },
            Cmd::Gather {
                unit: EntId(4),
                target: EntId(8),
            },
            Cmd::Work {
                unit: EntId(4),
                target: EntId(8),
            },
            Cmd::Halt { unit: EntId(5) },
        ]
    }

    #[test]
    fn every_command_is_a_real_engine_verb() {
        for c in all() {
            let v = &g::UNIT_VERBS[c.verb()];
            assert!(v.opcode > 0, "{:?} maps to opcode 0", c);
            // The name is the engine's own; assert the pairing rather than trusting it.
            let expect = match c {
                Cmd::Queue { .. } => "QUEUE_UP",
                Cmd::Build { .. } => "BUILD",
                Cmd::Move { .. } => "MOVE_TO",
                Cmd::Attack { .. } => "ATTACK",
                Cmd::Gather { .. } => "GATHER",
                Cmd::Work { .. } => "REPAIR",
                Cmd::Halt { .. } => "HALT",
            };
            assert_eq!(c.verb_name(), expect);
        }
    }

    /// The round trip is what makes "a trained model is a drop-in" checkable.
    #[test]
    fn heads_round_trip_through_the_rl_action_layout() {
        let slot = |e: EntId| e.0 as u16;
        let unslot = |s: u16| EntId(s as u32);
        for c in all() {
            let h = c.to_heads(slot);
            assert_eq!(h.len(), g::N_UNIT_HEADS);
            let back = Cmd::from_heads(c.actor(), &h, unslot).expect("verb is implemented");
            assert_eq!(back, c, "round trip failed for {c:?}");
        }
    }

    #[test]
    fn an_unimplemented_verb_decodes_to_none_rather_than_something_wrong() {
        let h = {
            let mut h = [0i32; g::N_UNIT_HEADS];
            h[0] = g::uv::SPELL as i32 + 1;
            h
        };
        assert!(Cmd::from_heads(EntId(1), &h, |_| EntId::NONE).is_none());
    }
}
