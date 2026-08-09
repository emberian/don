//! The gate on the fidelity claim.
//!
//! `tools/replay-validate.sh` and `tools/oracle-regress.sh` produce numbers that only mean
//! something if the simulation they measured was reproducing retail. This test file is the
//! standing proof that our default does that, and it is deliberately separate from the
//! unit tests inside `deviations.rs`: those check the module, this checks the *promise*.
//!
//! It fails if any registry entry is active in fidelity mode — by any route: the default
//! constructor, `Default`, the environment, or a caller trying to force one on.

use don_sim::deviations::{
    behaviour, Deviation, ImplementationStatus, Kind, Mode, ModeConfig, ModeError,
    ReadinessBlocker, Surface, REGISTRY,
};

/// **The hard requirement.** No entry may be active in fidelity mode.
#[test]
fn no_registry_entry_is_active_in_fidelity_mode() {
    let configs = [
        ("ModeConfig::default()", ModeConfig::default()),
        ("ModeConfig::fidelity()", ModeConfig::fidelity()),
        (
            "from_env(unset)",
            ModeConfig::from_env_parts(None, None).unwrap(),
        ),
        (
            "from_env(DON_MODE=fidelity)",
            ModeConfig::from_env_parts(Some("fidelity"), None).unwrap(),
        ),
        (
            "from_env(DON_MODE=retail, DON_DEVIATIONS=-everything)",
            ModeConfig::from_env_parts(Some("retail"), Some("-ai-gather-handicap")).unwrap(),
        ),
    ];

    for (name, cfg) in configs {
        assert!(cfg.is_fidelity(), "{name} is not fidelity mode");
        assert_eq!(cfg.mode(), Mode::Fidelity, "{name}");
        assert_eq!(
            cfg.active_count(),
            0,
            "{name} reports {} active deviations",
            cfg.active_count()
        );
        for d in Deviation::ALL {
            assert!(
                !cfg.is_active(d),
                "{name}: `{d}` is active in fidelity mode — every fidelity number \
                 measured in this state is void"
            );
        }
        assert_eq!(
            cfg.assert_fidelity(),
            Ok(()),
            "{name}: assert_fidelity did not agree with is_active"
        );
    }
}

/// Fidelity is not merely the default value — it cannot be talked out of it.
#[test]
fn fidelity_cannot_be_switched_at_runtime() {
    let mut cfg = ModeConfig::default();
    for d in Deviation::ALL {
        assert_eq!(cfg.enable(d), Err(ModeError::FidelityIsImmutable(d)));
    }
    assert_eq!(cfg.active_count(), 0);
    assert_eq!(cfg.assert_fidelity(), Ok(()));
}

/// The environment is the realistic contamination route: a harness inherits a shell where
/// somebody was playing with improved mode. It must refuse, not comply quietly.
#[test]
fn the_environment_cannot_smuggle_a_fix_into_a_fidelity_run() {
    for slug in Deviation::ALL
        .into_iter()
        .filter(|d| d.kind() == Kind::Fix)
        .map(|d| d.slug())
    {
        for form in [format!("+{slug}"), slug.to_string()] {
            let r = ModeConfig::from_env_parts(Some("fidelity"), Some(&form));
            assert!(
                r.is_err(),
                "DON_DEVIATIONS={form} was accepted in fidelity mode"
            );
        }
    }
    // An improved-mode environment must be caught by the gate, not silently measured.
    let cfg = ModeConfig::from_env_parts(Some("improved"), None).unwrap();
    assert!(matches!(
        cfg.assert_fidelity(),
        Err(ModeError::ActiveInFidelity(_) | ModeError::NotFidelity(_))
    ));
}

/// A clean mode bit is necessary but not sufficient: known approximations must be scoped
/// to the entrypoints that can actually execute them and make those entrypoints fail closed.
#[test]
fn known_product_drift_blocks_only_the_surfaces_it_reaches() {
    let cfg = ModeConfig::fidelity();

    // Replay validation cannot execute either currently registered product drift.
    assert_eq!(cfg.assert_ready(Surface::ReplayValidation), Ok(()));

    assert_eq!(
        cfg.assert_ready(Surface::PlayableEdition),
        Err(ModeError::KnownDrift(
            Surface::PlayableEdition,
            Deviation::ArenaConstructionModel,
        ))
    );
    assert_eq!(
        cfg.assert_ready(Surface::RlEnvironment),
        Err(ModeError::KnownDrift(
            Surface::RlEnvironment,
            Deviation::EnvPatrolExecution,
        ))
    );

    let product: Vec<_> = cfg.readiness_blockers(Surface::ProductRelease).collect();
    assert_eq!(
        product,
        vec![
            ReadinessBlocker::KnownDrift(Deviation::EnvPatrolExecution),
            ReadinessBlocker::KnownDrift(Deviation::ArenaConstructionModel),
            ReadinessBlocker::KnownDrift(Deviation::ArenaGatherModel),
            ReadinessBlocker::KnownDrift(Deviation::ArenaTargetAcquisitionModel),
            ReadinessBlocker::KnownDrift(Deviation::ArenaFlankModel),
            ReadinessBlocker::KnownDrift(Deviation::ArenaWaterModel),
            ReadinessBlocker::KnownDrift(Deviation::ArenaNavalModel),
            ReadinessBlocker::KnownDrift(Deviation::ArenaAirModel),
            ReadinessBlocker::KnownDrift(Deviation::ArenaDiplomacyModel),
            ReadinessBlocker::KnownDrift(Deviation::ArenaAttritionModel),
            ReadinessBlocker::KnownDrift(Deviation::ArenaSupplyModel),
        ]
    );
    assert!(product
        .iter()
        .all(|b| b.deviation() != Deviation::AiModelSimplifications));
}

/// Improved-mode fixes may clear readiness only after their real call sites adopt the seam.
#[test]
fn wired_improvements_leave_only_known_product_drift() {
    let cfg = ModeConfig::improved();
    let playable: Vec<_> = cfg.readiness_blockers(Surface::PlayableEdition).collect();
    assert_eq!(
        playable,
        vec![
            ReadinessBlocker::KnownDrift(Deviation::ArenaConstructionModel),
            ReadinessBlocker::KnownDrift(Deviation::ArenaGatherModel),
            ReadinessBlocker::KnownDrift(Deviation::ArenaTargetAcquisitionModel),
            ReadinessBlocker::KnownDrift(Deviation::ArenaFlankModel),
            ReadinessBlocker::KnownDrift(Deviation::ArenaWaterModel),
            ReadinessBlocker::KnownDrift(Deviation::ArenaNavalModel),
            ReadinessBlocker::KnownDrift(Deviation::ArenaAirModel),
            ReadinessBlocker::KnownDrift(Deviation::ArenaDiplomacyModel),
            ReadinessBlocker::KnownDrift(Deviation::ArenaAttritionModel),
            ReadinessBlocker::KnownDrift(Deviation::ArenaSupplyModel),
        ]
    );
    for d in [
        Deviation::AiGatherHandicap,
        Deviation::GatherHandicapTruncation,
        Deviation::BhsPrereqResultTest,
        Deviation::BhsCitizensTypo,
        Deviation::TikalBorderRuleSlot,
        Deviation::RefundChargesPlayer,
        Deviation::RefundRepeatCompounding,
    ] {
        assert_eq!(d.entry().implementation, ImplementationStatus::Wired, "{d}");
    }
}

/// Research harnesses are allowed to be bounded models, but their gaps are never silently
/// promoted into a shipped-surface claim.
#[test]
fn research_only_models_do_not_create_false_product_completeness_requirements() {
    let e = Deviation::AiModelSimplifications.entry();
    assert_eq!(e.implementation, ImplementationStatus::ResearchOnly);
    assert!(e.surfaces.is_empty());
    for surface in Surface::ALL {
        assert!(!e.reaches(surface));
    }
}

/// Every seam must agree with the registry: in fidelity mode each one produces the retail
/// value that the derivation doc records, not merely "some value".
///
/// This is what stops the invariant from being true and useless — a mode system that is
/// correctly inert but that nothing consults.
#[test]
fn every_seam_produces_retail_behaviour_in_fidelity_mode() {
    let f = ModeConfig::fidelity();

    // ai-gather-handicap: LeaderData::get_gather_handicap 0x006D66A0
    assert_eq!(
        (0..6)
            .map(|d| behaviour::gather_handicap_pct(&f, d))
            .collect::<Vec<_>>(),
        vec![-35, -15, -7, 0, 25, 50]
    );
    // gather-handicap-truncation: Leader::do_gather 0x006CE450, truncating idiv
    assert_eq!(behaviour::apply_gather_handicap(&f, 7, 0), 4); // 4.55 -> 4
    assert_eq!(behaviour::apply_gather_handicap(&f, 7, 5), 10); // 10.5 -> 10

    // bhs-prereq-result-test: economic.bhs:638 tests `== 0`, so -1 is invisible
    assert!(!behaviour::bhs_order_failed(&f, behaviour::ORDER_INVALID));
    assert!(behaviour::bhs_order_failed(&f, behaviour::ORDER_REFUSED));

    // bhs-citizens-typo: the shipped name goes through unchanged and finds nothing
    assert_eq!(behaviour::bhs_unit_type_name(&f, "Citizens"), "Citizens");

    // tikal-border-rule-slot: 0x006B0DC9 reads +0x4A0 = TIKAL_TEMPLE_HP
    assert_eq!(behaviour::tikal_border_percent(&f, 90, 50), 50);

    // refund-charges-player / refund-repeat-compounding: Build::refund_cost 0x00620490
    let r = behaviour::refund_slot(&f, 100, 110);
    assert_eq!((r.credit, r.new_amt), (-10, 110));

    // caravan-heuristic-goal-y: 0x00685FD9 passes -goalY, so child.y never reaches h
    assert_eq!(behaviour::caravan_h_mode_a_dy(&f, 10, 400), -400);
    assert_eq!(behaviour::caravan_h_mode_a_dy(&f, 390, 400), -400);

    // refinery-bonus-dead: City::calc_gather 0x00737C60 stores a literal 0
    assert_eq!(behaviour::refinery_bonus_pct(&f, 33), 0);
}

/// The registry is the document. If an entry exists in code and not in the changelog, the
/// changelog is a lie by omission — which is the exact failure this lane exists to stop.
#[test]
fn the_changelog_covers_every_registry_entry() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/tracks/deviations.md"
    );
    let doc = match std::fs::read_to_string(path) {
        Ok(d) => d,
        Err(e) => panic!("cannot read {path}: {e}"),
    };
    for d in Deviation::ALL {
        assert!(
            doc.contains(d.slug()),
            "docs/tracks/deviations.md does not mention `{}`",
            d.slug()
        );
    }
    for entry in REGISTRY.iter() {
        for addr in entry.derived_from {
            assert!(
                !addr.trim().is_empty(),
                "{}: empty derivation line",
                entry.slug
            );
        }
    }
}
