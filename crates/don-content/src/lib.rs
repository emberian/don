//! How content and mods load — retail's rules, reproduced, plus the layer retail lacks.
//!
//! Descent of Nations wants to be an *improved and improvable* edition of Rise of Nations.
//! Improvable means someone other than us can change it, and that means inheriting a content
//! pipeline. RoN already has a good one: the whole ruleset is XML, mods are plain directory
//! trees, and there is a Workshop ecosystem built on top. This crate is the part of that we
//! can derive from the binary and hold on to.
//!
//! The main modules, in the order you should read them:
//!
//! * [`vfs`] — retail's path classification and precedence, reproduced from `ModManager` /
//!   `ModPackage`. Directory discovery is separate because host enumeration order and the
//!   engine's `SkipForbiddenFiles` filter are not certified cross-platform.
//! * [`status`] — `mod-status.txt`, the on-disk enable/priority state, in retail's own
//!   fixed-width format.
//! * [`info`] — dropdown `info.xml` structural preflight, using the keys and gates recovered
//!   from `GameMod::init`.
//! * [`manifest`] — `GameMod::generate_file_list`, the seed-zero four-byte checksum path,
//!   and a stable independent-edition serialisation of the retail entry set.
//! * [`workflow`] — local plus explicitly named Workshop directories, activation, order
//!   provenance, collision tracing, and a refusal to bless uncertified host enumeration.
//! * [`overlay`] — our addition: named, validated, field-level rule patches with an explicit
//!   layer order and a fidelity-mode lock.
//! * [`overlay_file`] — the checked `don-overlay.xml` artifact for independent-edition mods.
//! * [`runtime`] — strict whole-file rule loading and an immutable, generation-checked
//!   prepare/commit boundary for new simulation worlds.
//! * [`extend`] — the surface beyond retail: extension type ids above the closed
//!   `enum TypeIndex` space, a sparse balance overlay over the captured 493x493 matrix, and
//!   the enumerated hook points.
//! * [`compat`] — "would this retail mod work here", answered per file.
//!
//! Everything marked `[measured]` in the module docs was read from
//! `ron-bin/riseofnations.exe` at a VA resolved through `ron-bin/sbl/rise.pdb`, or captured
//! by `gen/gen_tables.py` into [`generated`]. Nothing here comes from community
//! documentation, and nothing here is *verified* in the proof-assistant sense.
//!
//! # The three-sentence version
//!
//! Retail resolves content by path, not by merge: `String::prepend_content_dir`
//! `0x00A1D690` funnels every file open through `ModManager::calcFilePath` `0x00A22910`,
//! which returns the first enabled mod, in ascending `priority`, that *declares* the
//! requested filename in the requested category — otherwise the shipped path. A mod is
//! therefore a directory tree with the same shape as the install, and the only file it is
//! ever forbidden to replace is one of the 21 built-in map styles. Because that rule is
//! small and closed, we reproduce it exactly; because it can only replace whole files, we
//! add [`overlay`] on top for the things it cannot express.

pub mod compat;
pub mod extend;
pub mod generated;
pub mod info;
pub mod manifest;
pub mod overlay;
pub mod overlay_file;
pub mod runtime;
pub mod scan;
pub mod status;
pub mod vfs;
pub mod workflow;

pub use compat::{report as compat_report, CompatReport, Support};
pub use extend::{BalanceOverlay, HookPoint, TypeId, TypeSpace};
pub use info::{read_info, DropdownInfo, InfoError, RetailInfoGate};
pub use manifest::{generate as generate_manifest, ManifestError, RetailManifest};
pub use overlay::{Layer, Mode, OverlayError, Patch, RuleStack};
pub use overlay_file::{read_overlay, OverlayFile, OverlayFileError};
pub use runtime::{PreparedReload, RuleRegistry, RuntimeSnapshot};
pub use vfs::{
    classify, is_map_forbidden, ContentStack, ModCategory, ModPackage, Resolved, StorageLocation,
    WorkshopTag,
};
pub use workflow::{ActivationPlan, OrderAuthority, ResolutionOutcome, WorkshopSpec};
