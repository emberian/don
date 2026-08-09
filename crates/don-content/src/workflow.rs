//! A fail-closed activation plan over the recovered loader primitives.
//!
//! [`crate::scan`] intentionally returns host enumeration order, because retail keeps the
//! order returned by `FindAllMatchingFiles`. That is evidence on Windows and merely an
//! accident on another host. This module prevents the CLI from turning that accident into a
//! gameplay precedence claim: order becomes authoritative only when one active package is
//! present, active packages are disjoint, every active package has a unique `mod-status.txt`
//! priority, or the user supplies a complete independent-edition order.
//!
//! Workshop discovery is likewise explicit. A `WorkshopSpec` names one already-installed
//! directory; it does not enumerate Steam's UGC tree, infer a display name from a numeric
//! directory, or assign it a precedence. Retail receives those facts from Steam callbacks,
//! and no real subscribed package is installed in the current evidence environment.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

use crate::info::{read_info, DropdownInfo, RetailInfoGate};
use crate::overlay_file::{read_overlay, OverlayFile};
use crate::scan::{find_windows_file, scan_mod_dir, scan_mods_root};
use crate::status::{self, ParseReport};
use crate::vfs::{
    classify, is_map_forbidden, ContentStack, ModCategory, ModPackage, StorageLocation,
    ALL_CATEGORIES,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkshopSpec {
    /// Display name supplied by Steam to `ModManager::addModPackage`, not guessed from the
    /// numeric Workshop directory.
    pub name: String,
    /// The already-installed UGC directory.
    pub root: PathBuf,
    pub published_file_id: u64,
    pub author_id: u64,
}

impl WorkshopSpec {
    pub fn new(name: impl Into<String>, root: impl Into<PathBuf>) -> Self {
        Self {
            name: name.into(),
            root: root.into(),
            published_file_id: 0,
            author_id: 0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ActivationRequest {
    pub local_mods_root: PathBuf,
    /// `None` auto-detects `<mods-dir>/../mod-status.txt` when it exists.
    pub status_path: Option<PathBuf>,
    pub workshops: Vec<WorkshopSpec>,
    /// A complete, low-number-wins order over every discovered package. This is a DoN user
    /// decision, never described as retail or Workshop discovery order.
    pub explicit_order: Vec<String>,
    /// The one dropdown package installed into the single GameSettings slot. Other dropdown
    /// packages remain inactive even when enabled.
    pub active_dropdown: Option<String>,
}

impl ActivationRequest {
    pub fn new(local_mods_root: impl Into<PathBuf>) -> Self {
        Self {
            local_mods_root: local_mods_root.into(),
            status_path: None,
            workshops: Vec::new(),
            explicit_order: Vec::new(),
            active_dropdown: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PackageOrigin {
    Local,
    WorkshopExplicit,
}

impl PackageOrigin {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Local => "local MyMods",
            Self::WorkshopExplicit => "explicit installed Workshop directory",
        }
    }
}

#[derive(Clone, Debug)]
pub enum Artifact<T> {
    Absent,
    Valid(T),
    Invalid(String),
}

#[derive(Clone, Debug)]
pub struct PackageInspection {
    pub root: PathBuf,
    pub origin: PackageOrigin,
    pub info: Artifact<DropdownInfo>,
    pub overlay: Artifact<OverlayFile>,
}

#[derive(Clone, Debug)]
pub struct StatusInspection {
    pub path: PathBuf,
    pub explicit: bool,
    pub parse: ParseReport,
    pub matched_rows: usize,
    pub unmatched_names: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OrderAuthority {
    Trivial,
    /// All active packages were assigned unique priorities by installed-package rows.
    RetailStatus(PathBuf),
    /// The user supplied every discovered name exactly once.
    ExplicitEditionOrder,
    /// Multiple packages are active, but no two declare the same loadable `(category,file)`.
    Disjoint,
    /// At least one collision exists and its winner depends on uncertified host order.
    Unresolved(String),
}

impl OrderAuthority {
    pub fn is_authoritative(&self) -> bool {
        !matches!(self, Self::Unresolved(_))
    }

    pub fn label(&self) -> String {
        match self {
            Self::Trivial => "trivial (zero or one active package)".to_string(),
            Self::RetailStatus(p) => format!("retail mod-status.txt ({})", p.display()),
            Self::ExplicitEditionOrder => "explicit independent-edition order".to_string(),
            Self::Disjoint => "order-independent (active file sets are disjoint)".to_string(),
            Self::Unresolved(s) => format!("UNRESOLVED: {s}"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ActivationPlan {
    pub stack: ContentStack,
    /// Same ordering as `stack.mods()`.
    pub packages: Vec<PackageInspection>,
    pub status: Option<StatusInspection>,
    pub order: OrderAuthority,
    /// Duplicate active content keys that actually require precedence.
    pub collisions: BTreeMap<(ModCategory, String), Vec<String>>,
    pub notes: Vec<String>,
}

#[derive(Debug)]
pub enum WorkflowError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    EmptyWorkshop {
        name: String,
        path: PathBuf,
    },
    DuplicateIdentity(String),
    InvalidDropdown(String),
    InvalidExplicitOrder(String),
}

impl fmt::Display for WorkflowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::EmptyWorkshop { name, path } => write!(
                f,
                "Workshop package {name:?} at {} has no files in the 12 retail categories",
                path.display()
            ),
            Self::DuplicateIdentity(n) => write!(
                f,
                "more than one package has status identity {n:?}; activation would be ambiguous"
            ),
            Self::InvalidDropdown(s) => write!(f, "invalid dropdown activation: {s}"),
            Self::InvalidExplicitOrder(s) => write!(f, "invalid explicit order: {s}"),
        }
    }
}

impl std::error::Error for WorkflowError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CandidateDecision {
    Disabled,
    DropdownInactive,
    DoesNotDeclare,
    RetailMapVeto,
    Eligible,
}

impl CandidateDecision {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Disabled => "skipped: disabled",
            Self::DropdownInactive => "skipped: dropdown not selected",
            Self::DoesNotDeclare => "skipped: file not declared in scan snapshot",
            Self::RetailMapVeto => "skipped: ModManager::isMapForbidden veto",
            Self::Eligible => "eligible",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolutionCandidate {
    pub package: String,
    pub priority: i32,
    pub decision: CandidateDecision,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResolutionOutcome {
    Shipped { path: String },
    Mod { package: String, path: String },
    Unresolved { eligible: Vec<String> },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolutionExplanation {
    pub requested: String,
    pub category: ModCategory,
    pub filename: String,
    pub candidates: Vec<ResolutionCandidate>,
    pub outcome: ResolutionOutcome,
}

pub fn build_plan(request: &ActivationRequest) -> Result<ActivationPlan, WorkflowError> {
    let local = scan_mods_root(&request.local_mods_root).map_err(|source| WorkflowError::Io {
        path: request.local_mods_root.clone(),
        source,
    })?;
    let mut pending: Vec<(ModPackage, PathBuf, PackageOrigin)> = local
        .into_iter()
        .map(|m| {
            let root = request.local_mods_root.join(&m.install_dir);
            (m, root, PackageOrigin::Local)
        })
        .collect();
    for ws in &request.workshops {
        let install = ws.root.to_string_lossy().replace('\\', "/");
        let mut m = scan_mod_dir(&ws.root, &ws.name, &install, StorageLocation::None).map_err(
            |source| WorkflowError::Io {
                path: ws.root.clone(),
                source,
            },
        )?;
        if m.file_count() == 0 {
            return Err(WorkflowError::EmptyWorkshop {
                name: ws.name.clone(),
                path: ws.root.clone(),
            });
        }
        m.published_file_id = ws.published_file_id;
        m.author_id = ws.author_id;
        pending.push((m, ws.root.clone(), PackageOrigin::WorkshopExplicit));
    }
    reject_duplicate_identities(&pending)?;

    let roots: BTreeMap<(String, StorageLocation), (PathBuf, PackageOrigin)> = pending
        .iter()
        .map(|(m, root, origin)| {
            (
                (
                    m.name.to_ascii_lowercase(),
                    m.location.unwrap_or(StorageLocation::None),
                ),
                (root.clone(), origin.clone()),
            )
        })
        .collect();
    let mut stack = ContentStack::new();
    for (m, _, _) in pending {
        stack.push(m);
    }

    let explicit_status = request.status_path.is_some();
    let candidate_status = request.status_path.clone().or_else(|| {
        request
            .local_mods_root
            .parent()
            .map(|p| p.join("mod-status.txt"))
            .filter(|p| p.is_file())
    });
    let mut status_inspection = None;
    let mut status_rows = Vec::new();
    if let Some(path) = candidate_status {
        let text = std::fs::read_to_string(&path).map_err(|source| WorkflowError::Io {
            path: path.clone(),
            source,
        })?;
        let parsed = status::parse_report(&text);
        status_rows = parsed.rows.clone();
        let matched_rows = status::apply(&mut stack, &parsed.rows);
        let unmatched_names = parsed
            .rows
            .iter()
            .filter(|r| !status_row_matches_any(r, stack.mods()))
            .map(|r| r.name.clone())
            .collect();
        status_inspection = Some(StatusInspection {
            path,
            explicit: explicit_status,
            parse: parsed,
            matched_rows,
            unmatched_names,
        });
    }

    set_dropdown(&mut stack, request.active_dropdown.as_deref())?;
    if !request.explicit_order.is_empty() {
        apply_explicit_order(&mut stack, &request.explicit_order)?;
    }

    let collisions = active_collisions(&stack);
    let order = determine_order(
        &stack,
        &status_rows,
        status_inspection.as_ref().map(|s| &s.path),
        !request.explicit_order.is_empty(),
        &collisions,
    );
    let mut notes = vec![
        "host scanning cannot certify TFileSystem::SkipForbiddenFiles attribute filtering; the measured map-style veto is still applied at resolution".to_string(),
    ];
    if !request.workshops.is_empty() {
        notes.push(
            "Workshop directories were supplied explicitly; no Steam discovery or Workshop precedence was inferred"
                .to_string(),
        );
    }
    if request.active_dropdown.is_none()
        && stack
            .mods()
            .iter()
            .any(|m| m.enabled && m.is_dropdown_mod())
    {
        notes.push(
            "enabled dropdown packages remain inactive until exactly one is selected with --dropdown"
                .to_string(),
        );
    }
    if let Some(s) = &status_inspection {
        if !s.parse.ignored.is_empty() {
            notes.push(format!(
                "retail would silently skip {} malformed mod-status line(s); they are reported here",
                s.parse.ignored.len()
            ));
        }
    }

    let mut packages = Vec::new();
    for m in stack.mods() {
        let key = (
            m.name.to_ascii_lowercase(),
            m.location.unwrap_or(StorageLocation::None),
        );
        let (root, origin) = roots.get(&key).expect("identity checked").clone();
        let info = inspect_info(m, &root)?;
        let overlay = inspect_overlay(m, &root)?;
        packages.push(PackageInspection {
            root,
            origin,
            info,
            overlay,
        });
    }
    Ok(ActivationPlan {
        stack,
        packages,
        status: status_inspection,
        order,
        collisions,
        notes,
    })
}

impl ActivationPlan {
    pub fn explain(&self, path: &str) -> ResolutionExplanation {
        let (category, filename) = classify(path);
        let key = filename.to_ascii_lowercase();
        let mut candidates = Vec::new();
        let mut eligible = Vec::new();
        for m in self.stack.mods() {
            let decision = if !m.enabled {
                CandidateDecision::Disabled
            } else if m.is_dropdown_mod() && !m.dropdown_active {
                CandidateDecision::DropdownInactive
            } else if !m.has_asset(category, &key) {
                CandidateDecision::DoesNotDeclare
            } else if category == ModCategory::MapStyles && is_map_forbidden(&key) {
                CandidateDecision::RetailMapVeto
            } else {
                CandidateDecision::Eligible
            };
            if decision == CandidateDecision::Eligible {
                eligible.push(m);
            }
            candidates.push(ResolutionCandidate {
                package: m.name.clone(),
                priority: m.priority,
                decision,
            });
        }
        let shipped = format!("{}{}", category.relative_dir(), filename);
        let outcome = match eligible.len() {
            0 => ResolutionOutcome::Shipped { path: shipped },
            1 => ResolutionOutcome::Mod {
                package: eligible[0].name.clone(),
                path: mod_path(eligible[0], category, &filename),
            },
            _ if self.order.is_authoritative() => ResolutionOutcome::Mod {
                package: eligible[0].name.clone(),
                path: mod_path(eligible[0], category, &filename),
            },
            _ => {
                let mut names: Vec<String> = eligible.iter().map(|m| m.name.clone()).collect();
                names.sort_by_key(|n| n.to_ascii_lowercase());
                ResolutionOutcome::Unresolved { eligible: names }
            }
        };
        ResolutionExplanation {
            requested: path.to_string(),
            category,
            filename,
            candidates,
            outcome,
        }
    }

    /// Failures owned by activation itself. Per-file runtime support remains the separate
    /// [`crate::compat`] gate.
    pub fn activation_blockers(&self) -> Vec<String> {
        let mut out = Vec::new();
        if let OrderAuthority::Unresolved(reason) = &self.order {
            out.push(format!("load order unresolved: {reason}"));
        }
        for (i, m) in self.stack.mods().iter().enumerate() {
            if !m.enabled || (m.is_dropdown_mod() && !m.dropdown_active) {
                continue;
            }
            match &self.packages[i].info {
                Artifact::Invalid(e) => out.push(format!("{} info.xml: {e}", m.name)),
                Artifact::Valid(info) if m.is_dropdown_mod() => match info.gate {
                    RetailInfoGate::Accepts => {}
                    RetailInfoGate::WouldGenerateManifest => out.push(format!(
                        "{} info.xml needs retail FILES generation; exact checksum generation is unported",
                        m.name
                    )),
                    RetailInfoGate::RejectsIncompleteManifest => out.push(format!(
                        "{} info.xml is incomplete (FILES complete=0)",
                        m.name
                    )),
                },
                _ => {}
            }
            if let Artifact::Invalid(e) = &self.packages[i].overlay {
                out.push(format!("{} don-overlay.xml: {e}", m.name));
            }
        }
        out
    }
}

fn inspect_info(m: &ModPackage, root: &Path) -> Result<Artifact<DropdownInfo>, WorkflowError> {
    if !m.is_dropdown_mod() {
        return Ok(Artifact::Absent);
    }
    let path = find_windows_file(root, "info.xml").map_err(|source| WorkflowError::Io {
        path: root.to_path_buf(),
        source,
    })?;
    Ok(match path {
        Some(path) => match read_info(&path) {
            Ok(info) => Artifact::Valid(info),
            Err(e) => Artifact::Invalid(e.to_string()),
        },
        None => Artifact::Invalid("declared by scan but no case-insensitive file exists".into()),
    })
}

fn inspect_overlay(m: &ModPackage, root: &Path) -> Result<Artifact<OverlayFile>, WorkflowError> {
    if !m.has_asset(ModCategory::Root, "don-overlay.xml") {
        return Ok(Artifact::Absent);
    }
    let path = find_windows_file(root, "don-overlay.xml").map_err(|source| WorkflowError::Io {
        path: root.to_path_buf(),
        source,
    })?;
    Ok(match path {
        Some(path) => match read_overlay(&path) {
            Ok(overlay) => Artifact::Valid(overlay),
            Err(e) => Artifact::Invalid(e.to_string()),
        },
        None => Artifact::Invalid("declared by scan but no case-insensitive file exists".into()),
    })
}

fn reject_duplicate_identities(
    pending: &[(ModPackage, PathBuf, PackageOrigin)],
) -> Result<(), WorkflowError> {
    let mut seen = BTreeSet::new();
    for (m, _, _) in pending {
        let key = (
            m.name.to_ascii_lowercase(),
            m.location.unwrap_or(StorageLocation::None),
        );
        if !seen.insert(key) {
            return Err(WorkflowError::DuplicateIdentity(m.name.clone()));
        }
    }
    Ok(())
}

fn status_row_matches_any(row: &status::StatusRow, mods: &[ModPackage]) -> bool {
    let location = if row.local {
        StorageLocation::MyMods
    } else {
        StorageLocation::None
    };
    mods.iter()
        .any(|m| m.name.eq_ignore_ascii_case(&row.name) && m.location == Some(location))
}

fn set_dropdown(stack: &mut ContentStack, chosen: Option<&str>) -> Result<(), WorkflowError> {
    for m in stack.mods_mut() {
        m.dropdown_active = false;
    }
    let Some(chosen) = chosen else {
        return Ok(());
    };
    let hits: Vec<usize> = stack
        .mods()
        .iter()
        .enumerate()
        .filter(|(_, m)| m.name.eq_ignore_ascii_case(chosen))
        .map(|(i, _)| i)
        .collect();
    if hits.len() != 1 {
        return Err(WorkflowError::InvalidDropdown(format!(
            "{chosen:?} matches {} installed packages",
            hits.len()
        )));
    }
    let i = hits[0];
    if !stack.mods()[i].is_dropdown_mod() {
        return Err(WorkflowError::InvalidDropdown(format!(
            "{chosen:?} has no root info.xml"
        )));
    }
    stack.mods_mut()[i].dropdown_active = true;
    Ok(())
}

fn apply_explicit_order(stack: &mut ContentStack, order: &[String]) -> Result<(), WorkflowError> {
    if order.len() != stack.mods().len() {
        return Err(WorkflowError::InvalidExplicitOrder(format!(
            "expected {} names, got {}",
            stack.mods().len(),
            order.len()
        )));
    }
    let mut seen = BTreeSet::new();
    for (i, name) in order.iter().enumerate() {
        let key = name.to_ascii_lowercase();
        if !seen.insert(key) {
            return Err(WorkflowError::InvalidExplicitOrder(format!(
                "duplicate name {name:?}"
            )));
        }
        let hits: Vec<usize> = stack
            .mods()
            .iter()
            .enumerate()
            .filter(|(_, m)| m.name.eq_ignore_ascii_case(name))
            .map(|(j, _)| j)
            .collect();
        if hits.len() != 1 {
            return Err(WorkflowError::InvalidExplicitOrder(format!(
                "{name:?} matches {} installed packages",
                hits.len()
            )));
        }
        stack.mods_mut()[hits[0]].priority = i as i32 + 1;
    }
    stack.sort_via_priority();
    Ok(())
}

fn active(m: &ModPackage) -> bool {
    m.enabled && (!m.is_dropdown_mod() || m.dropdown_active)
}

fn active_collisions(stack: &ContentStack) -> BTreeMap<(ModCategory, String), Vec<String>> {
    let mut owners: BTreeMap<(ModCategory, String), Vec<String>> = BTreeMap::new();
    for m in stack.mods().iter().filter(|m| active(m)) {
        for cat in ALL_CATEGORIES {
            for name in &m.files[cat.index()] {
                if cat == ModCategory::MapStyles && is_map_forbidden(name) {
                    continue;
                }
                owners
                    .entry((cat, name.clone()))
                    .or_default()
                    .push(m.name.clone());
            }
        }
    }
    owners.retain(|_, v| v.len() > 1);
    owners
}

fn determine_order(
    stack: &ContentStack,
    rows: &[status::StatusRow],
    status_path: Option<&PathBuf>,
    explicit: bool,
    collisions: &BTreeMap<(ModCategory, String), Vec<String>>,
) -> OrderAuthority {
    let active_mods: Vec<&ModPackage> = stack.mods().iter().filter(|m| active(m)).collect();
    if explicit {
        return OrderAuthority::ExplicitEditionOrder;
    }
    if active_mods.len() <= 1 {
        return OrderAuthority::Trivial;
    }
    if let Some(path) = status_path {
        let mut priorities = BTreeSet::new();
        let fully_ordered = active_mods.iter().all(|m| {
            let local = m.location == Some(StorageLocation::MyMods);
            let priority = rows
                .iter()
                .rfind(|r| r.local == local && r.name.eq_ignore_ascii_case(&m.name))
                .map(|r| r.priority);
            priority.is_some_and(|p| priorities.insert(p))
        });
        if fully_ordered {
            return OrderAuthority::RetailStatus(path.clone());
        }
    }
    if collisions.is_empty() {
        return OrderAuthority::Disjoint;
    }
    OrderAuthority::Unresolved(format!(
        "{} active file collision(s) need a complete unique status order or --order",
        collisions.len()
    ))
}

fn mod_path(m: &ModPackage, cat: ModCategory, filename: &str) -> String {
    let prefix = if m.location == Some(StorageLocation::MyMods) {
        format!("mods/{}", m.install_dir)
    } else {
        m.install_dir.clone()
    };
    format!("{prefix}/{}{filename}", cat.relative_dir())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmpdir(tag: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("don-content-workflow-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn conflicting_host_order_is_unresolved_until_explicit() {
        let root = tmpdir("order");
        for n in ["Alpha", "Bravo"] {
            fs::create_dir_all(root.join(n).join("data")).unwrap();
            fs::write(root.join(n).join("data/rules.xml"), n).unwrap();
        }
        let req = ActivationRequest::new(&root);
        let plan = build_plan(&req).unwrap();
        assert!(matches!(plan.order, OrderAuthority::Unresolved(_)));
        assert!(matches!(
            plan.explain("data/rules.xml").outcome,
            ResolutionOutcome::Unresolved { .. }
        ));

        let mut explicit = ActivationRequest::new(&root);
        explicit.explicit_order = vec!["Bravo".into(), "Alpha".into()];
        let plan = build_plan(&explicit).unwrap();
        assert_eq!(plan.order, OrderAuthority::ExplicitEditionOrder);
        assert!(matches!(
            plan.explain("data/rules.xml").outcome,
            ResolutionOutcome::Mod { ref package, .. } if package == "Bravo"
        ));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn unique_complete_status_order_is_authoritative() {
        let root = tmpdir("status");
        for n in ["Alpha", "Bravo"] {
            fs::create_dir_all(root.join("mods").join(n).join("data")).unwrap();
            fs::write(root.join("mods").join(n).join("data/rules.xml"), n).unwrap();
        }
        let mut stack = ContentStack::new();
        for (i, n) in ["Bravo", "Alpha"].iter().enumerate() {
            let mut m = ModPackage::new(*n, *n);
            m.priority = i as i32 + 1;
            stack.push(m);
        }
        fs::write(root.join("mod-status.txt"), status::render(&stack)).unwrap();
        let plan = build_plan(&ActivationRequest::new(root.join("mods"))).unwrap();
        assert!(matches!(plan.order, OrderAuthority::RetailStatus(_)));
        assert!(matches!(
            plan.explain("data/rules.xml").outcome,
            ResolutionOutcome::Mod { ref package, .. } if package == "Bravo"
        ));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn dropdown_activation_is_explicit_and_preflighted() {
        let root = tmpdir("dropdown");
        fs::create_dir_all(root.join("Choice").join("data")).unwrap();
        fs::write(root.join("Choice/data/rules.xml"), b"x").unwrap();
        fs::write(
            root.join("Choice/info.xml"),
            br#"<INFO><FILE name="Choice" version="1" description="x" size="1" checksum="2"/><FILES complete="1" checksum="2"/></INFO>"#,
        )
        .unwrap();
        let plan = build_plan(&ActivationRequest::new(&root)).unwrap();
        assert!(matches!(
            plan.explain("data/rules.xml").outcome,
            ResolutionOutcome::Shipped { .. }
        ));
        let mut req = ActivationRequest::new(&root);
        req.active_dropdown = Some("Choice".into());
        let plan = build_plan(&req).unwrap();
        assert!(plan.activation_blockers().is_empty());
        assert!(matches!(plan.packages[0].info, Artifact::Valid(_)));
        fs::remove_dir_all(&root).unwrap();
    }
}
