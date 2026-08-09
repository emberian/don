//! How content and mods load — retail's rules, reproduced, plus the layer retail lacks.
//!
//! Descent of Nations wants to be an *improved and improvable* edition of Rise of Nations.
//! Improvable means someone other than us can change it, and that means inheriting a content
//! pipeline. RoN already has a good one: the whole ruleset is XML, mods are plain directory
//! trees, and there is a Workshop ecosystem built on top. This crate is the part of that we
//! can derive from the binary and hold on to.
//!
//! Four modules, in the order you should read them:
//!
//! * [`vfs`] — retail's discovery, classification and precedence, reproduced from
//!   `ModManager` / `ModPackage`. This is the compatibility surface.
//! * [`status`] — `mod-status.txt`, the on-disk enable/priority state, in retail's own
//!   fixed-width format.
//! * [`overlay`] — our addition: named, validated, field-level rule patches with an explicit
//!   layer order and a fidelity-mode lock.
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
pub mod overlay;
pub mod scan;
pub mod status;
pub mod vfs;

pub use compat::{report as compat_report, CompatReport, Support};
pub use extend::{BalanceOverlay, HookPoint, TypeId, TypeSpace};
pub use overlay::{Layer, Mode, OverlayError, Patch, RuleStack};
pub use vfs::{
    classify, is_map_forbidden, ContentStack, ModCategory, ModPackage, Resolved, StorageLocation,
    WorkshopTag,
};
