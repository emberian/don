//! Live bridge between `Unit::move_step` and retail unit collision.
//!
//! `movement::move_step_profile_with_collision` already reproduces the call order in
//! `Unit::move_step` `0x005FAF30`, while [`collision::detect_unit_collision`] and
//! [`collision::resolve_unit_collision`] expose the recovered detector and deterministic
//! detour/wait/repath body.  The missing piece was the caller transaction between them: a
//! move-step detect must persist the blocker and order destination before resolution, the
//! waypoint re-test must remain read-only, and resolution must consume the same actor row, path,
//! RNG stream, and repath host.
//!
//! This module owns that transaction.  It deliberately does not guess how a simulation stores
//! object rows or relocates collision stamps.  [`ActorCommit`] is a mandatory host boundary:
//! after every mutating detector/resolver call the host receives both row images and must make
//! `after` authoritative, including spatial-link and footprint maintenance when coordinates
//! changed.  Likewise [`CollisionPathHost`] makes the two external virtual bodies used by
//! `resolve_unit_collision` explicit.
//!
//! Provenance is the sole-caller edge visible in the shipped binary:
//! `Unit::move_step` `0x005FAF30` -> `Unit::detect_unit_collision` `0x00617060` ->
//! `Unit::resolve_unit_collision` `0x005F9D30`.  The detector call shapes are pinned by
//! [`collision::DetectArgs::MOVE_STEP`] and [`collision::DetectArgs::DETOUR_PROBE`].

use std::cell::RefCell;

use crate::rng::Random;
use crate::systems::collision::{self, CollCheck, CollUnits, Detect, DetectArgs, Resolve, UnitRow};
use crate::systems::map_terrain::World;
use crate::systems::movement::{
    Body, MoveCollisionEvent, MoveCollisionProbe, MoveCollisionReply, PathStack,
};

/// Stable retail object address used for one `move_step` collision transaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActorRef {
    pub who: i32,
    pub o: i32,
}

/// Why an authoritative actor-row write is required.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActorCommitKind {
    /// The side-effecting proposed-step detector ran.  This includes its clear/reset epilogue.
    Detect(Detect),
    /// The local detour/wait/repath resolver ran.
    Resolve(Resolve),
}

/// Complete mutation handed to the object-store adapter.
///
/// `after.x/y != before.x/y` is retail `Unit::set_new_location` territory.  A live adapter must
/// update the intrusive world link and per-guy collision stamps, not merely overwrite two
/// coordinates.  Keeping both snapshots makes that obligation impossible to miss.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActorCommit {
    pub actor: ActorRef,
    pub before: UnitRow,
    pub after: UnitRow,
    pub kind: ActorCommitKind,
}

/// External operations reached by the deterministic resolver body.
///
/// Retail calls `UnitData::invalid_loc` for at most two local-detour candidates and calls
/// `PathFinder::find_upath` from the repath arm.  `find_upath` returns true here for both retail
/// non-zero results (`Found` and suspended `-1`); only the raw zero/failure result is false.
pub trait CollisionPathHost {
    fn invalid_loc(&self, tile_x: i32, tile_y: i32) -> bool;
    fn find_upath(&mut self, path: &mut PathStack, quick: bool) -> bool;
}

/// A malformed/missing host fact.  Every rejection is fail-closed at the movement boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriverError {
    MissingActor(ActorRef),
    /// The read-only waypoint probe is only legal after the proposed step hit a body.
    WaypointProbeWithoutHit,
    /// Retail's resolver is only called after the side-effecting detector returned one.
    ResolveWithoutHit,
    /// A resolver may only consume the proposed coordinate persisted by the immediately
    /// preceding side-effecting detector; never reuse a stale blocker/order destination.
    ResolveTargetMismatch {
        detected: (i32, i32),
        resolve: (i32, i32),
    },
    /// The object-store row and the integrator body must describe the same live object.
    ActorPositionMismatch {
        row: (i32, i32),
        body: (i32, i32),
    },
}

/// The exact child decision, or a fail-closed protocol rejection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriverDecision {
    Detect(Detect),
    Resolve(Resolve),
    Rejected(DriverError),
}

/// One event's host-facing result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DriverEvent {
    pub reply: MoveCollisionReply,
    pub decision: DriverDecision,
    /// Authoritative actor image after a successful mutating event.  Waypoint probes are
    /// intentionally read-only and therefore leave this empty.
    pub actor_after: Option<UnitRow>,
}

/// Mutation-pinnable event counts for one or more collision transactions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DriverTrace {
    pub move_detects: u64,
    pub waypoint_probes: u64,
    pub resolves: u64,
    pub rejected: u64,
}

/// Protocol state for one actor's `move_step` collision callbacks.
///
/// A new proposed-step detect always starts a transaction.  This also handles retail's
/// close-waypoint escape: that path performs a hit and a clear read-only waypoint probe but no
/// resolver, so the next frame's proposed-step detect simply replaces the pending state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CollisionSession {
    actor: ActorRef,
    pending_target: Option<(i32, i32)>,
    trace: DriverTrace,
}

impl CollisionSession {
    pub fn new(actor: ActorRef) -> Self {
        Self {
            actor,
            pending_target: None,
            trace: DriverTrace::default(),
        }
    }

    pub fn actor(&self) -> ActorRef {
        self.actor
    }

    pub fn trace(&self) -> DriverTrace {
        self.trace
    }

    pub fn pending_hit(&self) -> bool {
        self.pending_target.is_some()
    }

    /// Execute one callback emitted by `move_step_profile_with_collision`.
    ///
    /// `commit` is mandatory rather than a default `CollUnits::write`: a live store may need
    /// to relocate an anchor and collision stamps when the resolver snaps the actor to its
    /// UCoord centre.  It must make `commit.after` visible to the next `CollUnits::row` call.
    #[allow(clippy::too_many_arguments)]
    pub fn handle<U, P, C>(
        &mut self,
        world: &mut World,
        check: &mut CollCheck,
        units: &mut U,
        rng: &mut Random,
        path_host: &mut P,
        commit: &mut C,
        event: MoveCollisionEvent<'_>,
    ) -> DriverEvent
    where
        U: CollUnits,
        P: CollisionPathHost,
        C: FnMut(&mut World, &mut U, ActorCommit),
    {
        match event {
            MoveCollisionEvent::Detect { x, y, probe } => {
                self.detect(world, check, units, commit, x, y, probe)
            }
            MoveCollisionEvent::Resolve { x, y, body, path } => self.resolve(
                world, check, units, rng, path_host, commit, x, y, body, path,
            ),
        }
    }

    fn rejected(&mut self, error: DriverError, reply: MoveCollisionReply) -> DriverEvent {
        self.trace.rejected += 1;
        DriverEvent {
            reply,
            decision: DriverDecision::Rejected(error),
            actor_after: None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn detect<U, C>(
        &mut self,
        world: &mut World,
        check: &mut CollCheck,
        units: &mut U,
        commit: &mut C,
        x: i32,
        y: i32,
        probe: MoveCollisionProbe,
    ) -> DriverEvent
    where
        U: CollUnits,
        C: FnMut(&mut World, &mut U, ActorCommit),
    {
        match probe {
            MoveCollisionProbe::MoveStep => self.trace.move_detects += 1,
            MoveCollisionProbe::Waypoint => {
                self.trace.waypoint_probes += 1;
                if self.pending_target.is_none() {
                    // A hit prevents the escape and therefore keeps movement stopped.
                    return self.rejected(
                        DriverError::WaypointProbeWithoutHit,
                        MoveCollisionReply::Hit,
                    );
                }
            }
        }

        let Some(before) = units.row(self.actor.who, self.actor.o) else {
            if probe == MoveCollisionProbe::MoveStep {
                self.pending_target = Some((x, y));
            }
            // Missing collision state may never be interpreted as open ground.
            return self.rejected(
                DriverError::MissingActor(self.actor),
                MoveCollisionReply::Hit,
            );
        };
        let args = match probe {
            MoveCollisionProbe::MoveStep => DetectArgs::MOVE_STEP,
            MoveCollisionProbe::Waypoint => DetectArgs::DETOUR_PROBE,
        };
        let detected = collision::detect_unit_collision(world, check, units, &before, x, y, args);
        let reply = if detected.blocked() {
            MoveCollisionReply::Hit
        } else {
            MoveCollisionReply::Clear
        };

        if probe == MoveCollisionProbe::Waypoint {
            // DETOUR_PROBE can only return Clear/HitProbe and is definitionally read-only.
            return DriverEvent {
                reply,
                decision: DriverDecision::Detect(detected),
                actor_after: None,
            };
        }

        let mut after = before;
        let frame = units.frame();
        detected.apply(units, &mut after, frame);
        commit(
            world,
            units,
            ActorCommit {
                actor: self.actor,
                before,
                after,
                kind: ActorCommitKind::Detect(detected),
            },
        );
        self.pending_target = detected.blocked().then_some((x, y));
        DriverEvent {
            reply,
            decision: DriverDecision::Detect(detected),
            actor_after: Some(after),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn resolve<U, P, C>(
        &mut self,
        world: &mut World,
        check: &mut CollCheck,
        units: &mut U,
        rng: &mut Random,
        path_host: &mut P,
        commit: &mut C,
        x: i32,
        y: i32,
        body: &mut Body,
        path: &mut PathStack,
    ) -> DriverEvent
    where
        U: CollUnits,
        P: CollisionPathHost,
        C: FnMut(&mut World, &mut U, ActorCommit),
    {
        self.trace.resolves += 1;
        let Some(detected_target) = self.pending_target.take() else {
            return self.rejected(
                DriverError::ResolveWithoutHit,
                MoveCollisionReply::Unhandled,
            );
        };
        if detected_target != (x, y) {
            return self.rejected(
                DriverError::ResolveTargetMismatch {
                    detected: detected_target,
                    resolve: (x, y),
                },
                MoveCollisionReply::Unhandled,
            );
        }

        let Some(before) = units.row(self.actor.who, self.actor.o) else {
            return self.rejected(
                DriverError::MissingActor(self.actor),
                MoveCollisionReply::Unhandled,
            );
        };
        if (before.x, before.y) != (body.x, body.y) {
            return self.rejected(
                DriverError::ActorPositionMismatch {
                    row: (before.x, before.y),
                    body: (body.x, body.y),
                },
                MoveCollisionReply::Unhandled,
            );
        }

        let mut after = before;
        // The resolver's invalid-location probes and repath callback are sequential, never
        // nested. RefCell lets one typed host provide the immutable and mutable callbacks
        // without an unsafe alias or a duplicated terrain view.
        let path_host = RefCell::new(path_host);
        let resolved = collision::resolve_unit_collision(
            world,
            check,
            units,
            &mut after,
            path,
            rng,
            &|tx, ty| path_host.borrow().invalid_loc(tx, ty),
            |stack, quick| path_host.borrow_mut().find_upath(stack, quick),
        );
        commit(
            world,
            units,
            ActorCommit {
                actor: self.actor,
                before,
                after,
                kind: ActorCommitKind::Resolve(resolved),
            },
        );
        // Repath can snap the actor to its UCoord centre before searching.  The integrator's
        // local Body copy must observe that write before it is returned to UnitWork.
        body.x = after.x;
        body.y = after.y;

        DriverEvent {
            reply: MoveCollisionReply::Handled,
            decision: DriverDecision::Resolve(resolved),
            actor_after: Some(after),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systems::collision::{place, CollGuy, UnitTable, DOMAIN_LAND, UCELL};
    use crate::systems::map_terrain::World;
    use crate::systems::movement::{
        self, ucell_centre, MoveStep, MoveTurnProfile, PathData, UnitWorld,
        UNIT_FLAG_MOVE_WHILE_TURNING,
    };
    use crate::trig::find_angle;

    fn world() -> World {
        World::init(3, 3, 44, 4, 4)
    }

    fn row(who: i32, o: i32, ux: i32, uy: i32) -> UnitRow {
        UnitRow {
            who,
            o,
            x: ucell_centre(ux),
            y: ucell_centre(uy),
            down: -1,
            down_who: -1,
            domain: DOMAIN_LAND,
            block_radius: 1,
            group: -1,
            collide_o: -1,
            collide_who: -1,
            on_map: true,
            active: true,
            moving: true,
            has_orders: true,
            order: 1,
            ..UnitRow::default()
        }
    }

    fn guy(ux: i32, uy: i32) -> CollGuy {
        CollGuy {
            x: ucell_centre(ux),
            y: ucell_centre(uy),
            block_radius: 1,
        }
    }

    #[derive(Default)]
    struct PathHost {
        invalid: Vec<(i32, i32)>,
        repaths: Vec<bool>,
        found: bool,
    }

    impl CollisionPathHost for PathHost {
        fn invalid_loc(&self, tx: i32, ty: i32) -> bool {
            self.invalid.contains(&(tx, ty))
        }

        fn find_upath(&mut self, _path: &mut PathStack, quick: bool) -> bool {
            self.repaths.push(quick);
            self.found
        }
    }

    fn write_commit(_world: &mut World, units: &mut UnitTable, commit: ActorCommit) {
        units.write(commit.actor.who, commit.actor.o, &commit.after);
    }

    #[test]
    fn missing_actor_and_out_of_order_events_fail_closed() {
        let actor = ActorRef { who: 0, o: 7 };
        let mut session = CollisionSession::new(actor);
        let mut w = world();
        let mut check = CollCheck::new();
        let mut units = UnitTable::default();
        let mut rng = Random::new(1);
        let mut paths = PathHost::default();
        let mut commit = write_commit;

        let detect = session.handle(
            &mut w,
            &mut check,
            &mut units,
            &mut rng,
            &mut paths,
            &mut commit,
            MoveCollisionEvent::Detect {
                x: 100,
                y: 100,
                probe: MoveCollisionProbe::MoveStep,
            },
        );
        assert_eq!(detect.reply, MoveCollisionReply::Hit);
        assert_eq!(
            detect.decision,
            DriverDecision::Rejected(DriverError::MissingActor(actor))
        );

        let mut body = Body::default();
        let mut path = PathStack::new();
        let resolve = session.handle(
            &mut w,
            &mut check,
            &mut units,
            &mut rng,
            &mut paths,
            &mut commit,
            MoveCollisionEvent::Resolve {
                x: 101,
                y: 100,
                body: &mut body,
                path: &mut path,
            },
        );
        assert_eq!(resolve.reply, MoveCollisionReply::Unhandled);
        assert_eq!(
            resolve.decision,
            DriverDecision::Rejected(DriverError::ResolveTargetMismatch {
                detected: (100, 100),
                resolve: (101, 100),
            })
        );
        assert_eq!(session.trace().rejected, 2);

        let waypoint = session.handle(
            &mut w,
            &mut check,
            &mut units,
            &mut rng,
            &mut paths,
            &mut commit,
            MoveCollisionEvent::Detect {
                x: 100,
                y: 100,
                probe: MoveCollisionProbe::Waypoint,
            },
        );
        assert_eq!(waypoint.reply, MoveCollisionReply::Hit);
        assert_eq!(
            waypoint.decision,
            DriverDecision::Rejected(DriverError::WaypointProbeWithoutHit)
        );
    }

    #[test]
    fn waypoint_probe_after_hit_is_read_only_and_preserves_pending_target() {
        let mut w = world();
        let mut units = UnitTable::default();
        place(&mut w, &mut units, row(0, 1, 7, 10), [guy(7, 10)]);
        place(&mut w, &mut units, row(0, 2, 12, 10), [guy(12, 10)]);

        let mut session = CollisionSession::new(ActorRef { who: 0, o: 1 });
        let mut check = CollCheck::new();
        let mut rng = Random::new(3);
        let mut paths = PathHost::default();
        let mut commits = Vec::new();
        let mut commit = |_world: &mut World, units: &mut UnitTable, c: ActorCommit| {
            commits.push(c);
            units.write(c.actor.who, c.actor.o, &c.after);
        };
        let hit = session.handle(
            &mut w,
            &mut check,
            &mut units,
            &mut rng,
            &mut paths,
            &mut commit,
            MoveCollisionEvent::Detect {
                x: ucell_centre(10),
                y: ucell_centre(10),
                probe: MoveCollisionProbe::MoveStep,
            },
        );
        assert_eq!(hit.reply, MoveCollisionReply::Hit);
        let after_hit = units.row(0, 1).unwrap();

        let probe = session.handle(
            &mut w,
            &mut check,
            &mut units,
            &mut rng,
            &mut paths,
            &mut commit,
            MoveCollisionEvent::Detect {
                x: ucell_centre(20),
                y: ucell_centre(20),
                probe: MoveCollisionProbe::Waypoint,
            },
        );
        assert_eq!(probe.reply, MoveCollisionReply::Clear);
        assert_eq!(probe.actor_after, None);
        assert_eq!(units.row(0, 1), Some(after_hit));
        assert_eq!(commits.len(), 1, "waypoint probe performs no row write");
        assert!(
            session.pending_hit(),
            "resolve still consumes the move-step hit"
        );
        assert_eq!(session.trace().waypoint_probes, 1);
    }

    #[test]
    fn clear_detector_epilogue_is_committed_before_return() {
        let mut w = world();
        let mut units = UnitTable::default();
        let mut actor = row(0, 1, 7, 10);
        actor.unit_masks = collision::umask::WAIT;
        actor.collide = 9;
        actor.collide_o = 2;
        actor.collide_who = 0;
        actor.collide_frame = 1;
        place(&mut w, &mut units, actor, [guy(7, 10)]);
        units.frame = 10;

        let mut session = CollisionSession::new(ActorRef { who: 0, o: 1 });
        let mut check = CollCheck::new();
        let mut rng = Random::new(2);
        let mut paths = PathHost::default();
        let mut commits = Vec::new();
        let mut commit = |_world: &mut World, units: &mut UnitTable, c: ActorCommit| {
            commits.push(c);
            units.write(c.actor.who, c.actor.o, &c.after);
        };
        let out = session.handle(
            &mut w,
            &mut check,
            &mut units,
            &mut rng,
            &mut paths,
            &mut commit,
            MoveCollisionEvent::Detect {
                x: ucell_centre(10),
                y: ucell_centre(10),
                probe: MoveCollisionProbe::MoveStep,
            },
        );

        assert_eq!(out.reply, MoveCollisionReply::Clear);
        assert_eq!(
            out.decision,
            DriverDecision::Detect(Detect::ClearAndReset { yielded: false })
        );
        let after = units.row(0, 1).unwrap();
        assert_eq!(
            (after.collide_who, after.collide_o, after.collide),
            (-1, -1, 0)
        );
        assert_eq!(after.unit_masks & collision::umask::WAIT, 0);
        assert_eq!(commits.len(), 1);
        assert!(!session.pending_hit());
    }

    struct OpenMovementWorld;

    impl UnitWorld for OpenMovementWorld {
        fn tiles_w(&self) -> i32 {
            12
        }
        fn tiles_h(&self) -> i32 {
            12
        }
        fn wcells_w(&self) -> i32 {
            3
        }
        fn invalid_loc(&self, _tile_x: i32, _tile_y: i32) -> bool {
            false
        }
        fn unit_collides(&self, _x: i32, _y: i32) -> bool {
            panic!("typed collision callback must own body detection")
        }
        fn needs_transport(&self, _fx: i32, _fy: i32, _tx: i32, _ty: i32) -> i32 {
            0
        }
        fn tregion(&self, _tile_x: i32, _tile_y: i32) -> i32 {
            0
        }
    }

    #[test]
    fn move_step_detects_persists_and_resolves_against_one_actor_image() {
        let mut coll_world = world();
        let mut units = UnitTable::default();
        place(&mut coll_world, &mut units, row(0, 1, 7, 10), [guy(7, 10)]);
        // Same owner makes the deterministic resolver choose its ordinary wait arm.
        place(
            &mut coll_world,
            &mut units,
            row(0, 2, 12, 10),
            [guy(12, 10)],
        );

        let actor = ActorRef { who: 0, o: 1 };
        let mut session = CollisionSession::new(actor);
        let mut check = CollCheck::new();
        let mut rng = Random::new(0x1234);
        let rng_before = rng.state();
        let mut paths = PathHost::default();
        let mut commits = Vec::new();
        let mut commit = |_world: &mut World, units: &mut UnitTable, c: ActorCommit| {
            commits.push(c);
            units.write(c.actor.who, c.actor.o, &c.after);
        };

        let start = (ucell_centre(7), ucell_centre(10));
        let target = (ucell_centre(10), ucell_centre(10));
        let mut body = Body {
            x: start.0,
            y: start.1,
            angle: find_angle(target.0 - start.0, target.1 - start.1),
            stuck_budget: 0,
        };
        let mut path = PathStack::new();
        path.push(PathData {
            to_x: target.0,
            to_y: target.1,
            tolerance: 0,
            flags: PathData::FLAG_MORE,
        });
        let mut profile = MoveTurnProfile {
            unit_flags: UNIT_FLAG_MOVE_WHILE_TURNING,
            ..MoveTurnProfile::default()
        };
        let mut movement_world = OpenMovementWorld;
        let step = movement::move_step_profile_with_collision(
            &mut movement_world,
            &mut body,
            &mut path,
            target,
            3 * UCELL,
            0x2000_0000,
            &mut profile,
            |_movement_world, event| {
                session
                    .handle(
                        &mut coll_world,
                        &mut check,
                        &mut units,
                        &mut rng,
                        &mut paths,
                        &mut commit,
                        event,
                    )
                    .reply
            },
        );

        assert_eq!(step, MoveStep::Blocked);
        assert_eq!(session.trace().move_detects, 1);
        assert_eq!(session.trace().resolves, 1);
        assert_eq!(session.trace().rejected, 0);
        assert_eq!(commits.len(), 2, "detect then resolve are distinct writes");
        assert!(matches!(
            commits[0].kind,
            ActorCommitKind::Detect(Detect::Hit {
                blocker_who: 0,
                blocker_o: 2,
                dest: Some(_)
            })
        ));
        assert_eq!(commits[1].kind, ActorCommitKind::Resolve(Resolve::Wait));
        let after = units.row(actor.who, actor.o).unwrap();
        assert_eq!((after.collide_who, after.collide_o), (0, 2));
        assert_eq!(after.collide, 1);
        assert_ne!(after.unit_masks & collision::umask::WAIT, 0);
        assert_eq!((body.x, body.y), start);
        assert_eq!(rng.state(), rng_before, "wait consumes no game RNG");
        assert!(paths.repaths.is_empty());
    }

    #[test]
    fn repath_snap_is_committed_and_copied_back_to_integrator_body() {
        let mut w = world();
        let mut units = UnitTable::default();
        let mut actor = row(0, 1, 7, 10);
        actor.order = 4; // FLEE_TO skips the wait arm.
        actor.x += 7;
        actor.y += 11;
        let start = (actor.x, actor.y);
        place(&mut w, &mut units, actor, [guy(7, 10)]);
        place(&mut w, &mut units, row(0, 2, 12, 10), [guy(12, 10)]);

        let mut session = CollisionSession::new(ActorRef { who: 0, o: 1 });
        let mut check = CollCheck::new();
        let mut rng = Random::new(99);
        let mut paths = PathHost {
            found: true,
            ..PathHost::default()
        };
        let mut commit = write_commit;
        let hit = session.handle(
            &mut w,
            &mut check,
            &mut units,
            &mut rng,
            &mut paths,
            &mut commit,
            MoveCollisionEvent::Detect {
                x: ucell_centre(10),
                y: ucell_centre(10),
                probe: MoveCollisionProbe::MoveStep,
            },
        );
        assert_eq!(hit.reply, MoveCollisionReply::Hit);

        let mut body = Body {
            x: start.0,
            y: start.1,
            ..Body::default()
        };
        let mut path = PathStack::new();
        path.push(PathData {
            to_x: ucell_centre(10),
            to_y: ucell_centre(10),
            tolerance: 0x60,
            // Suppress the local detour so this fixture reaches the repath arm directly.
            flags: PathData::FLAG_WAYPOINT,
        });
        let resolved = session.handle(
            &mut w,
            &mut check,
            &mut units,
            &mut rng,
            &mut paths,
            &mut commit,
            MoveCollisionEvent::Resolve {
                x: ucell_centre(10),
                y: ucell_centre(10),
                body: &mut body,
                path: &mut path,
            },
        );

        assert_eq!(
            resolved.decision,
            DriverDecision::Resolve(Resolve::Repath {
                wait_drawn: None,
                found: true
            })
        );
        let snapped = (ucell_centre(7), ucell_centre(10));
        assert_eq!((body.x, body.y), snapped);
        assert_eq!(
            (units.row(0, 1).unwrap().x, units.row(0, 1).unwrap().y),
            snapped
        );
        assert_eq!(paths.repaths, [false]);
    }
}
