//! `don-content` — scan, check, and explain an independent-edition activation plan.
//!
//! ```text
//! don-content scan    <mods-dir> [activation options]
//! don-content check   <mods-dir> [activation options]
//! don-content explain <mods-dir> [activation options] <content-path>...
//! don-content overlay <don-overlay.xml>
//! don-content rules
//! ```
//!
//! Activation options are explicit where the retail environment would otherwise supply
//! facts we do not have: `--workshop NAME=PATH`, `--dropdown NAME`, `--status PATH`, and a
//! complete independent-edition `--order NAME,NAME,...`. The conventional
//! `<mods-dir>/../mod-status.txt` is auto-detected. Host enumeration is never silently
//! promoted to Workshop or retail precedence.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use don_content::compat::{report, Support};
use don_content::generated::{CATEGORY_INFO, FORBIDDEN_MAPSTYLES, TAG_LINKS};
use don_content::info::RetailInfoGate;
use don_content::overlay_file::read_overlay;
use don_content::scan::populated_categories;
use don_content::vfs::ALL_CATEGORIES;
use don_content::workflow::{
    build_plan, ActivationPlan, ActivationRequest, Artifact, PackageInspection, ResolutionOutcome,
    WorkshopSpec,
};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(|s| s.as_str()) {
        Some("scan") => with_plan(&args[1..], false, cmd_scan),
        Some("check") => with_plan(&args[1..], false, cmd_check),
        Some("explain") | Some("probe") => with_plan(&args[1..], true, cmd_explain),
        Some("overlay") if args.len() == 2 => cmd_overlay(Path::new(&args[1])),
        Some("rules") if args.len() == 1 => cmd_rules(),
        _ => {
            eprintln!("{}", usage());
            ExitCode::from(2)
        }
    }
}

fn usage() -> &'static str {
    "usage:\n  \
     don-content scan    <mods-dir> [--status PATH] [--workshop NAME=PATH]... [--dropdown NAME] [--order NAME,NAME,...]\n  \
     don-content check   <mods-dir> [same activation options]\n  \
     don-content explain <mods-dir> [same activation options] <content-path>...\n  \
     don-content overlay <don-overlay.xml>\n  \
     don-content rules\n\n  \
     Workshop directories and order are explicit because no installed retail corpus proves them.\n  \
     `probe` remains an alias for `explain`."
}

fn with_plan(
    args: &[String],
    allow_paths: bool,
    command: fn(&ActivationPlan, &[String]) -> ExitCode,
) -> ExitCode {
    let (request, paths) = match parse_activation(args, allow_paths) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{e}\n\n{}", usage());
            return ExitCode::from(2);
        }
    };
    if allow_paths && paths.is_empty() {
        eprintln!("explain requires at least one content path");
        return ExitCode::from(2);
    }
    let plan = match build_plan(&request) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("cannot build activation plan: {e}");
            return ExitCode::FAILURE;
        }
    };
    command(&plan, &paths)
}

fn parse_activation(
    args: &[String],
    allow_paths: bool,
) -> Result<(ActivationRequest, Vec<String>), String> {
    let Some(root) = args.first() else {
        return Err("missing <mods-dir>".into());
    };
    let mut request = ActivationRequest::new(root);
    let mut paths = Vec::new();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--status" => {
                i += 1;
                request.status_path =
                    Some(PathBuf::from(args.get(i).ok_or("--status requires PATH")?));
            }
            "--workshop" => {
                i += 1;
                let spec = args.get(i).ok_or("--workshop requires NAME=PATH")?;
                let (name, path) = spec
                    .split_once('=')
                    .ok_or("--workshop requires NAME=PATH")?;
                if name.is_empty() || path.is_empty() {
                    return Err("--workshop requires non-empty NAME=PATH".into());
                }
                request.workshops.push(WorkshopSpec::new(name, path));
            }
            "--dropdown" => {
                i += 1;
                request.active_dropdown =
                    Some(args.get(i).ok_or("--dropdown requires NAME")?.to_string());
            }
            "--order" => {
                i += 1;
                let order = args.get(i).ok_or("--order requires NAME,NAME,...")?;
                request.explicit_order = order
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect();
                if request.explicit_order.is_empty() {
                    return Err("--order contains no names".into());
                }
            }
            "--" if allow_paths => {
                paths.extend_from_slice(&args[i + 1..]);
                break;
            }
            a if a.starts_with("--") => return Err(format!("unknown option {a}")),
            _ if allow_paths => paths.push(args[i].clone()),
            _ => return Err(format!("unexpected argument {:?}", args[i])),
        }
        i += 1;
    }
    Ok((request, paths))
}

fn cmd_scan(plan: &ActivationPlan, _: &[String]) -> ExitCode {
    if plan.stack.mods().is_empty() {
        println!("no packages found");
        println!("an empty directory is dropped by ModPackage::buildPackage 0x00A358B0");
        return ExitCode::SUCCESS;
    }
    println!("{} package(s)", plan.stack.mods().len());
    println!("load order: {}", plan.order.label());
    if let Some(status) = &plan.status {
        println!(
            "status: {} — {}/{} row(s) matched{}",
            status.path.display(),
            status.matched_rows,
            status.parse.rows.len(),
            if status.explicit {
                " (explicit)"
            } else {
                " (auto-detected)"
            }
        );
        for (line, source) in &status.parse.ignored {
            println!("  ignored status line {line}: {source}");
        }
        if !status.unmatched_names.is_empty() {
            println!(
                "  status rows without installed packages: {}",
                status.unmatched_names.join(", ")
            );
        }
    } else {
        println!("status: none");
    }
    for note in &plan.notes {
        println!("note: {note}");
    }
    println!();

    for (i, m) in plan.stack.mods().iter().enumerate() {
        let inspect = &plan.packages[i];
        let active = m.enabled && (!m.is_dropdown_mod() || m.dropdown_active);
        let kind = match (m.is_dropdown_mod(), m.is_data_mod()) {
            (true, _) => "dropdown",
            (false, true) => "data",
            (false, false) => "content-only",
        };
        println!(
            "[{}] {} — {kind}, {}, {}",
            m.priority,
            m.name,
            inspect.origin.label(),
            if active {
                "ACTIVE"
            } else if !m.enabled {
                "disabled"
            } else {
                "dropdown inactive"
            }
        );
        println!("    root: {}", inspect.root.display());
        let tags: Vec<&str> = m.steam_tags().iter().map(|t| t.name()).collect();
        println!("    recovered Workshop tags: {}", tags.join(", "));
        for cat in populated_categories(m) {
            println!(
                "    {:<10} {:>4} file(s)  {}",
                cat.name(),
                m.files[cat.index()].len(),
                cat.relative_dir()
            );
        }
        print_info(inspect);
        print_overlay(inspect);
        let r = report(m);
        let (consumed, total) = r.consumed_fraction();
        println!(
            "    runtime support: {consumed}/{total} consumed, {} parsed, {} resolved-only, {} out-of-scope",
            r.count(Support::Parsed),
            r.count(Support::ResolvedOnly),
            r.count(Support::OutOfScope)
        );
        if !r.vetoed_by_retail.is_empty() {
            println!("    retail map veto: {}", r.vetoed_by_retail.join(", "));
        }
        println!();
    }
    if !plan.collisions.is_empty() {
        println!("active file collisions:");
        for ((cat, file), owners) in &plan.collisions {
            println!("  {}{} — {}", cat.relative_dir(), file, owners.join(" / "));
        }
    }
    let blockers = plan.activation_blockers();
    println!(
        "activation mechanics preflight: {}",
        if blockers.is_empty() {
            "PASS"
        } else {
            "BLOCKED"
        }
    );
    for b in blockers {
        println!("  {b}");
    }
    ExitCode::SUCCESS
}

fn print_info(inspect: &PackageInspection) {
    match &inspect.info {
        Artifact::Absent => {}
        Artifact::Invalid(e) => println!("    info.xml: INVALID — {e}"),
        Artifact::Valid(info) => {
            println!(
                "    info.xml: {} {:?} v{:?}; {} manifest entr{}",
                info.gate.label(),
                info.name,
                info.version,
                info.manifest.len(),
                if info.manifest.len() == 1 { "y" } else { "ies" }
            );
            for warning in &info.warnings {
                println!("      warning: {warning}");
            }
        }
    }
}

fn print_overlay(inspect: &PackageInspection) {
    match &inspect.overlay {
        Artifact::Absent => {}
        Artifact::Invalid(e) => println!("    don-overlay.xml: INVALID — {e}"),
        Artifact::Valid(o) => println!(
            "    don-overlay.xml: {} checked patch(es), {} value change(s)",
            o.patches.len(),
            o.attributions.len()
        ),
    }
}

fn cmd_check(plan: &ActivationPlan, _: &[String]) -> ExitCode {
    if plan.stack.mods().is_empty() {
        eprintln!("REJECT: no mod packages found");
        return ExitCode::FAILURE;
    }
    let mut failures = plan.activation_blockers();
    for (i, m) in plan.stack.mods().iter().enumerate() {
        let inspect = &plan.packages[i];
        if let Artifact::Invalid(e) = &inspect.info {
            failures.push(format!("{} info.xml: {e}", m.name));
        }
        if let Artifact::Valid(info) = &inspect.info {
            match info.gate {
                RetailInfoGate::Accepts => {}
                RetailInfoGate::WouldGenerateManifest => failures.push(format!(
                    "{} info.xml relies on unported retail manifest/checksum generation",
                    m.name
                )),
                RetailInfoGate::RejectsIncompleteManifest => {
                    failures.push(format!("{} info.xml has FILES complete=0", m.name))
                }
            }
        }
        if let Artifact::Invalid(e) = &inspect.overlay {
            failures.push(format!("{} don-overlay.xml: {e}", m.name));
        }
        let r = report(m);
        if !r.fully_consumed() {
            for f in r.files.iter().filter(|f| f.support != Support::Consumed) {
                failures.push(format!(
                    "{}: {}{} is {} — {}",
                    m.name,
                    f.category.relative_dir(),
                    f.filename,
                    f.support.label(),
                    f.reason
                ));
            }
            for f in &r.vetoed_by_retail {
                failures.push(format!("{}: mapstyles/{f} is vetoed by retail", m.name));
            }
        }
    }
    failures.sort();
    failures.dedup();
    if failures.is_empty() {
        println!("ACCEPT: activation order, metadata, overlays, and every file consumer are ready");
        ExitCode::SUCCESS
    } else {
        println!("REJECT:");
        for f in &failures {
            println!("  {f}");
        }
        eprintln!(
            "{} finding(s); static preflight is not live-retail or Workshop certification",
            failures.len()
        );
        ExitCode::FAILURE
    }
}

fn cmd_explain(plan: &ActivationPlan, paths: &[String]) -> ExitCode {
    println!("load order: {}", plan.order.label());
    let mut unresolved = false;
    for path in paths {
        let e = plan.explain(path);
        println!(
            "\n{}\n  category {}  key {:?}",
            path,
            e.category.name(),
            e.filename
        );
        for c in &e.candidates {
            println!(
                "  [{:>2}] {:<28} {}",
                c.priority,
                c.package,
                c.decision.label()
            );
        }
        match e.outcome {
            ResolutionOutcome::Shipped { path } => println!("  result: shipped -> {path}"),
            ResolutionOutcome::Mod { package, path } => {
                println!("  result: {package} -> {path}")
            }
            ResolutionOutcome::Unresolved { eligible } => {
                unresolved = true;
                println!(
                    "  result: UNRESOLVED among {}; supply complete --order or mod-status.txt",
                    eligible.join(", ")
                );
            }
        }
    }
    if unresolved {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn cmd_overlay(path: &Path) -> ExitCode {
    let overlay = match read_overlay(path) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("REJECT {}: {e}", path.display());
            return ExitCode::FAILURE;
        }
    };
    println!(
        "overlay v{}: {} patch(es), {} changed shipped value(s)",
        overlay.version,
        overlay.patches.len(),
        overlay.attributions.len()
    );
    for p in &overlay.patches {
        println!(
            "  {}[{}] = {}{}",
            p.field,
            p.index,
            p.value,
            if p.note.is_empty() {
                String::new()
            } else {
                format!(" — {}", p.note)
            }
        );
    }
    for a in &overlay.attributions {
        println!(
            "    {}[{}]: {} -> {} ({})",
            a.field,
            a.index,
            a.from,
            a.to,
            a.layer.name()
        );
    }
    ExitCode::SUCCESS
}

fn cmd_rules() -> ExitCode {
    println!("categories  (s_ModCategoryInfo 0x00C07AD0)");
    for (i, c) in CATEGORY_INFO.iter().enumerate() {
        println!(
            "  {i:>2} {:<10} {:<14} recursive={}",
            c.name, c.relative_dir, c.recursive
        );
    }
    println!("\npublish/tag patterns  (s_SteamWorkshopTagLinks 0x00C068D0; not a load whitelist)");
    for cat in ALL_CATEGORIES {
        let pats: Vec<&str> = TAG_LINKS
            .iter()
            .filter(|l| l.category == cat)
            .map(|l| l.pattern)
            .collect();
        if !pats.is_empty() {
            println!("  {:<10} {}", cat.name(), pats.join(" "));
        }
    }
    println!(
        "\nnot replaceable  (ModManager::isMapForbidden 0x00A21140, {} names, CAT_MAPSTYLES only)",
        FORBIDDEN_MAPSTYLES.len()
    );
    for chunk in FORBIDDEN_MAPSTYLES.chunks(4) {
        println!("  {}", chunk.join("  "));
    }
    println!(
        "\nSkipForbiddenFiles is a filesystem-enumeration flag whose internal attribute filter remains untraced."
    );
    ExitCode::SUCCESS
}
