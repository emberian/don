//! End-to-end workflow tests over committed synthetic trees. These exist separately from the
//! small unit tests so discovery, status, metadata, overlays, and resolution cross module
//! boundaries exactly as the CLI does.

use std::path::{Path, PathBuf};

use don_content::workflow::{
    build_plan, ActivationRequest, Artifact, OrderAuthority, ResolutionOutcome, WorkshopSpec,
};
use don_content::{read_info, RetailInfoGate};

fn fixture(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(path)
}

#[test]
fn status_metadata_overlay_and_resolution_form_one_plan() {
    let plan = build_plan(&ActivationRequest::new(fixture("activation/mods"))).unwrap();
    assert!(matches!(plan.order, OrderAuthority::RetailStatus(_)));
    assert_eq!(plan.stack.mods()[0].name, "Bravo");
    assert!(matches!(
        plan.explain("data/rules.xml").outcome,
        ResolutionOutcome::Mod { ref package, .. } if package == "Bravo"
    ));
    let alpha = plan
        .stack
        .mods()
        .iter()
        .position(|m| m.name == "Alpha")
        .unwrap();
    assert!(matches!(plan.packages[alpha].overlay, Artifact::Valid(_)));
    let choice = plan
        .stack
        .mods()
        .iter()
        .position(|m| m.name == "Choice")
        .unwrap();
    assert!(matches!(
        plan.packages[choice].info,
        Artifact::Valid(ref i) if i.gate == RetailInfoGate::Accepts
    ));
    assert!(!plan.stack.mods()[choice].dropdown_active);
}

#[test]
fn an_explicit_workshop_directory_never_acquires_an_invented_order() {
    let mut request = ActivationRequest::new(fixture("activation/mods"));
    request.workshops.push(WorkshopSpec::new(
        "Workshop Choice",
        fixture("workshop/WorkshopChoice"),
    ));
    request.active_dropdown = Some("Workshop Choice".into());
    let plan = build_plan(&request).unwrap();
    assert!(matches!(plan.order, OrderAuthority::Unresolved(_)));
    assert!(matches!(
        plan.explain("data/rules.xml").outcome,
        ResolutionOutcome::Unresolved { .. }
    ));

    request.explicit_order = vec![
        "Workshop Choice".into(),
        "Bravo".into(),
        "Alpha".into(),
        "Choice".into(),
    ];
    let plan = build_plan(&request).unwrap();
    assert_eq!(plan.order, OrderAuthority::ExplicitEditionOrder);
    assert!(matches!(
        plan.explain("data/rules.xml").outcome,
        ResolutionOutcome::Mod { ref package, .. } if package == "Workshop Choice"
    ));
}

#[test]
fn the_incomplete_info_fixture_hits_the_measured_retail_gate() {
    let info = read_info(&fixture("invalid-info/info.xml")).unwrap();
    assert_eq!(info.gate, RetailInfoGate::RejectsIncompleteManifest);
}
