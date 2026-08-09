use don_sim::systems::ammo::{
    ammo_do_damage_splash_execute, ammo_do_damage_splash_scan, Ammo, AmmoWalk, DamageCall, ObjView,
    ShooterRules, SplashDamageEnv, SplashEnv, SplashObject, DOMAIN_LAND, FLAG_ALIVE, FLAG_FLYING,
};

struct SplashWorld {
    heads: Vec<(i32, i32, i32, i32)>,
    objects: Vec<(i32, i32, SplashObject)>,
    enemies: Vec<(i32, i32)>,
    applied: Vec<DamageCall>,
    mutate_after_first: Option<((i32, i32), (i32, i32))>,
}

impl SplashEnv for SplashWorld {
    fn world_wcells(&self) -> (i32, i32) {
        (10, 10)
    }

    fn splash_head(&self, wx: i32, wy: i32) -> Option<(i32, i32)> {
        self.heads
            .iter()
            .find(|&&(x, y, _, _)| x == wx && y == wy)
            .map(|&(_, _, who, o)| (who, o))
    }

    fn splash_object(&self, who: i32, o: i32) -> Option<SplashObject> {
        self.objects
            .iter()
            .find(|&&(w, i, _)| w == who && i == o)
            .map(|&(_, _, object)| object)
    }

    fn splash_is_enemy(&self, shooter_who: i32, candidate_who: i32) -> bool {
        self.enemies.contains(&(shooter_who, candidate_who))
    }
}

impl SplashDamageEnv for SplashWorld {
    fn splash_do_damage(&mut self, call: DamageCall) {
        self.applied.push(call);
        if self.applied.len() != 1 {
            return;
        }
        let Some((current, next)) = self.mutate_after_first else {
            return;
        };
        // Retail fetched `current.down` before this callback. Severing the live link must not
        // stop this projectile from reaching `next` after Object::do_damage returns.
        self.object_mut(current).down = None;
        // But the next object is looked up after this callback, so its new dead state must be
        // observed by the unit admission gate.
        self.object_mut(next).live_unit = false;
    }
}

impl SplashWorld {
    fn object_mut(&mut self, identity: (i32, i32)) -> &mut SplashObject {
        &mut self
            .objects
            .iter_mut()
            .find(|(who, o, _)| (*who, *o) == identity)
            .expect("fixture identity")
            .2
    }
}

fn unit(x: i32, y: i32, down: Option<(i32, i32)>) -> SplashObject {
    SplashObject {
        view: ObjView {
            alive: true,
            is_unit: true,
            x,
            y,
            rules: ShooterRules {
                domain: DOMAIN_LAND,
                ..Default::default()
            },
            ..Default::default()
        },
        live_build: false,
        live_unit: true,
        on_map: true,
        block_radius: 0,
        down,
    }
}

fn fixture() -> (SplashWorld, AmmoWalk) {
    let centre = 4 * 768;
    (
        SplashWorld {
            heads: vec![(4, 4, 1, 10)],
            objects: vec![
                (0, 7, unit(0, 0, None)),
                (1, 10, unit(centre, centre, Some((2, 20)))),
                (2, 20, unit(centre, centre, Some((3, 30)))),
                (3, 30, unit(centre, centre, None)),
            ],
            enemies: vec![(0, 1), (0, 2), (0, 3)],
            applied: Vec::new(),
            mutate_after_first: Some(((1, 10), (2, 20))),
        },
        AmmoWalk {
            flags: FLAG_ALIVE | FLAG_FLYING,
            sx: centre - 100,
            sy: centre,
            ex: centre,
            ey: centre,
            splash_area: 1,
            who: 0,
            o: 7,
            whom: 1,
            ox: 10,
            num_guys: 3,
            index: 12,
            ..Default::default()
        },
    )
}

#[test]
fn live_splash_interleaves_mutation_and_closes_after_the_captured_chain() {
    let (mut world, mut snapshot_ammo) = fixture();
    assert_eq!(
        ammo_do_damage_splash_scan(&mut snapshot_ammo, &world, DOMAIN_LAND)
            .iter()
            .map(|call| (call.victim_who, call.victim_o))
            .collect::<Vec<_>>(),
        [(1, 10), (2, 20), (3, 30)],
        "a deferred snapshot admits all three initially-live nodes"
    );

    let (_, live_walk) = fixture();
    let mut ammo = Ammo {
        w: live_walk,
        has_spline: true,
    };
    let impact = ammo_do_damage_splash_execute(&mut ammo, &mut world, DOMAIN_LAND)
        .expect("fixture is a splash projectile");

    assert_eq!(
        impact
            .calls
            .iter()
            .map(|call| (call.victim_who, call.victim_o))
            .collect::<Vec<_>>(),
        [(1, 10), (3, 30)],
        "the first hit makes the middle node ineligible before its turn"
    );
    assert_eq!(
        world.applied, impact.calls,
        "the return value is an audit log"
    );
    assert_eq!(
        (ammo.w.whom, ammo.w.ox),
        (3, 30),
        "the captured link reaches the dead middle node and then its tail"
    );
    assert!(impact.closed);
    assert_eq!(
        ammo.w.flags, 0,
        "Ammo::close is an assignment after the walk"
    );
    assert!(!ammo.has_spline, "close recycles the projectile spline");
}

#[test]
fn non_splash_input_fails_closed_without_mutating_ammo_or_world() {
    let (mut world, mut walk) = fixture();
    walk.splash_area = 0;
    let mut ammo = Ammo {
        w: walk,
        has_spline: true,
    };
    let before = ammo;

    assert_eq!(
        ammo_do_damage_splash_execute(&mut ammo, &mut world, DOMAIN_LAND),
        None
    );
    assert_eq!(ammo, before);
    assert!(world.applied.is_empty());
}
