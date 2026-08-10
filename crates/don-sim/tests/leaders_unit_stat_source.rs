use don_sim::systems::leaders::{
    UnitTypeStatSource, UnitTypeStatSourceError, UnitTypeStatSourceProvenance,
    SUPPORTED_UNIT_TYPE_STAT_TSV_SHA256, UNIT_TYPE_STAT_END, UNIT_TYPE_STAT_FIRST,
};
use don_sim::systems::save_load::{save_sim, SaveError};
use don_sim::systems::unit_inctime::SUPPORTED_RETAIL_EXE_SHA256;
use don_sim::tick::{Sim, UnitTypeStatSourceInstallError};

fn synthetic_live_tsv() -> String {
    let mut tsv = String::from(
        "type_id\tfrom\twhere\tobj_masks\tarmor\tdomain\tgraft\tunit_flags\tunit_flags2\tmoves\n",
    );
    for type_id in UNIT_TYPE_STAT_FIRST..UNIT_TYPE_STAT_END {
        tsv.push_str(&format!(
            "{type_id}\t-1\t414\t2425094146\t0\t0\t-1\t0\t0\t25\n"
        ));
    }
    tsv
}

fn provenance() -> UnitTypeStatSourceProvenance {
    UnitTypeStatSourceProvenance {
        executable_sha256: SUPPORTED_RETAIL_EXE_SHA256,
        table_sha256: SUPPORTED_UNIT_TYPE_STAT_TSV_SHA256,
    }
}

#[test]
fn runtime_source_accepts_unsigned_masks_and_rejects_a_wrong_generation() {
    let tsv = synthetic_live_tsv();
    let source = UnitTypeStatSource::from_live_tsv(&tsv, provenance()).unwrap();
    assert_eq!(source.provenance(), provenance());

    let mut wrong = provenance();
    wrong.executable_sha256[0] ^= 1;
    assert_eq!(
        UnitTypeStatSource::from_live_tsv(&tsv, wrong),
        Err(UnitTypeStatSourceError::UnsupportedExecutable)
    );
}

#[test]
fn installed_external_source_is_refused_by_the_current_save_format() {
    let source = UnitTypeStatSource::from_live_tsv(&synthetic_live_tsv(), provenance()).unwrap();
    let mut sim = Sim::new(0x51a7, 8);
    sim.install_unit_type_stat_source(source).unwrap();

    assert_eq!(
        save_sim(&sim),
        Err(SaveError::Unsupported("step-8 leader state/hosts"))
    );
}

#[test]
fn installation_is_one_time_and_pre_frame_pre_population() {
    let first = UnitTypeStatSource::from_live_tsv(&synthetic_live_tsv(), provenance()).unwrap();
    let offered = first.clone();
    let mut installed = Sim::new(1, 8);
    installed
        .install_unit_type_stat_source(first)
        .expect("constructor-pristine install");
    assert_eq!(
        installed.install_unit_type_stat_source(offered),
        Err(UnitTypeStatSourceInstallError::AlreadyInstalled {
            installed: provenance(),
            offered: provenance(),
        })
    );
    assert_eq!(
        installed.unit_type_stat_source_provenance(),
        Some(provenance())
    );

    let source = UnitTypeStatSource::from_live_tsv(&synthetic_live_tsv(), provenance()).unwrap();
    let mut started = Sim::new(2, 8);
    started.world.frame = 1;
    assert_eq!(
        started.install_unit_type_stat_source(source),
        Err(UnitTypeStatSourceInstallError::SimulationStarted {
            frame: 1,
            completed_ticks: 0,
        })
    );

    let source = UnitTypeStatSource::from_live_tsv(&synthetic_live_tsv(), provenance()).unwrap();
    let mut populated = Sim::new(3, 8);
    populated.spawn_unit(0, 50, 192, 192, 2).unwrap();
    assert!(matches!(
        populated.install_unit_type_stat_source(source),
        Err(UnitTypeStatSourceInstallError::SimulationPopulated {
            live_units: 1,
            registered_objects: 1,
            ..
        })
    ));

    let source = UnitTypeStatSource::from_live_tsv(&synthetic_live_tsv(), provenance()).unwrap();
    let mut activated = Sim::new(4, 8);
    activated.activate(0);
    assert_eq!(
        activated.install_unit_type_stat_source(source),
        Err(UnitTypeStatSourceInstallError::LeadersActivated)
    );
}
