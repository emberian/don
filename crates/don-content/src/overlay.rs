//! The layer retail does not have: field-level rule overlays, with validation.
//!
//! # Why this exists
//!
//! Retail's whole mod story is file replacement ([`crate::vfs`]). To change one constant you
//! must ship the entire `data/rules.xml`, which means (a) your mod silently reverts every
//! later balance patch, and (b) two mods that each want one constant are mutually exclusive.
//! That is the single biggest practical limit on the Workshop library, and it is a limit of
//! the *loader*, not of the data.
//!
//! An overlay is an ordered list of named field writes applied on top of a resolved base.
//! Because `don-rules` already knows every constant's name, byte offset, array arity and
//! parse mode, an overlay can be checked before it is applied instead of failing silently the
//! way an unknown XML element does in retail.
//!
//! # The layer stack, lowest to highest
//!
//! | # | layer | source | fidelity mode |
//! |--:|---|---|---|
//! | 0 | `Shipped` | `don_rules::SHIPPED`, i.e. `ron-data/rules.xml` as the engine stores it | required |
//! | 1 | `Edition` | Descent of Nations' own corrections (the improved-mode lane owns the contents) | **must be empty** |
//! | 2 | `Content` | retail-compatible file replacement, resolved by [`crate::vfs::ContentStack`] | allowed, but see below |
//! | 3 | `Overlay` | this module: named field writes | **must be empty** |
//! | 4 | `Session` | per-match overrides from a scenario or BHS script | **must be empty** |
//!
//! Higher layer wins. Within layer 2, retail's own rule applies: lowest `priority` wins and
//! the scan stops at the first mod that declares the file.
//! `Content` is supplied as the whole parsed base via [`RuleStack::from_content`]; it is never
//! legal to add field [`Patch`] values to `Shipped` or `Content`, because that would disguise
//! an overlay as a retail file replacement.
//!
//! # What fidelity mode means here, precisely
//!
//! `Fidelity` asserts that the rule block we simulate with is byte-identical to the one a
//! retail process would hold given the same content stack. It therefore forbids layers 1, 3
//! and 4 outright. It does **not** forbid layer 2 — a retail client running the same mods
//! also has those bytes — but it does mean that a replay captured without a mod cannot be
//! validated against a stack that has one, because the `rules` checksum channel
//! (`Game::walk_rules_data` `0x00589550`) walks the loaded values.
//!
//! [`RuleStack::content_digest`] exists for exactly that: retail solves the same problem with
//! `GameMod::compute_checksum` `0x005A94A0`, which sums a per-file checksum over
//! `GameMod::generate_file_list` `0x005A9030` and ships it through
//! `NetMsg_GameModSyncRequest` / `GameMod::sync` `0x005A9650` so a joiner without the mod is
//! made to subscribe (`ModSteamWorkshop::SubscribeModNow` `0x005672C0`) rather than desync.

use std::collections::BTreeMap;

use don_rules::rules::{Parser, RuleField, FIELDS, RULES_DWORDS};
use don_rules::Rules;

/// Which layer a write came from. Ordering is precedence: higher wins.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Layer {
    Shipped = 0,
    Edition = 1,
    Content = 2,
    Overlay = 3,
    Session = 4,
}

impl Layer {
    pub fn name(self) -> &'static str {
        match self {
            Layer::Shipped => "shipped",
            Layer::Edition => "edition",
            Layer::Content => "content",
            Layer::Overlay => "overlay",
            Layer::Session => "session",
        }
    }
}

/// Fidelity contract for a stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Reproduce retail. Layers `Edition`, `Overlay` and `Session` must be empty.
    Fidelity,
    /// Free to deviate through edition, overlay, and session patches. Shipped/content bases
    /// still enter through constructors because a content file is not a field overlay.
    Improved,
}

/// One named write.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Patch {
    /// Lowercase constant name, exactly as `Constants::init` binds it.
    pub field: String,
    /// Array index; `0` for scalars.
    pub index: u16,
    /// The value **as the engine would store it**, i.e. already through the field's parser.
    /// Use [`Patch::from_text`] to go through the real tokenizer instead of guessing.
    pub value: i32,
    /// Free text: why. Carried into the audit so a deviation is never anonymous.
    pub note: String,
}

impl Patch {
    pub fn new(field: &str, value: i32) -> Patch {
        Patch {
            field: field.to_ascii_lowercase(),
            index: 0,
            value,
            note: String::new(),
        }
    }

    pub fn at(field: &str, index: u16, value: i32) -> Patch {
        Patch {
            field: field.to_ascii_lowercase(),
            index,
            value,
            note: String::new(),
        }
    }

    pub fn why(mut self, note: &str) -> Patch {
        self.note = note.to_string();
        self
    }

    /// Build a patch from the *text* a modder would have written in XML, running it through
    /// the field's own parser (`_wtoi` or `String::fraction` at the field's compile-time
    /// scale). This is the "capture, do not calculate" path: the number is produced by the
    /// tokenizer we derived, not by hand arithmetic.
    pub fn from_text(field: &str, index: u16, text: &str) -> Result<Patch, OverlayError> {
        let f = lookup(field).ok_or_else(|| OverlayError::UnknownField(field.to_string()))?;
        let value = match f.parser {
            Parser::Wtoi => don_rules::wtoi(text),
            Parser::Scaled(scale) => don_rules::as_scaled(text, scale),
            Parser::Unclassified => {
                return Err(OverlayError::UnclassifiedParser(field.to_string()))
            }
        };
        Ok(Patch {
            field: f.name.to_string(),
            index,
            value,
            note: String::new(),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OverlayError {
    /// Retail ignores an unknown `<CONSTANTS>` element in silence, so a typo becomes a mod
    /// that "works" and does nothing. We refuse instead.
    UnknownField(String),
    /// `index >= field.count`.
    IndexOutOfRange {
        field: String,
        index: u16,
        count: u16,
    },
    /// Two of `don-rules`' 717 fields have a loader site the extractor could not classify
    /// (`unit_block_radius`, `americans_marine_entrench`). Writing them by text would require
    /// guessing the scale, which is folklore.
    UnclassifiedParser(String),
    /// `Shipped` and `Content` are whole-file bases, not patch lists. Accepting patches here
    /// would let a caller disguise an improved-mode deviation as retail content.
    BaseLayerMustUseConstructor { layer: Layer, count: usize },
    /// A non-empty deviating layer under [`Mode::Fidelity`].
    FidelityViolation { layer: Layer, count: usize },
}

impl std::fmt::Display for OverlayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OverlayError::UnknownField(n) => write!(f, "no rule constant named `{n}`"),
            OverlayError::IndexOutOfRange {
                field,
                index,
                count,
            } => {
                write!(
                    f,
                    "`{field}` has {count} slot(s); index {index} is out of range"
                )
            }
            OverlayError::UnclassifiedParser(n) => {
                write!(
                    f,
                    "`{n}` has an unclassified loader site; set its value explicitly"
                )
            }
            OverlayError::BaseLayerMustUseConstructor { layer, count } => write!(
                f,
                "the `{}` layer is a whole-file base, not a patch list ({count} patch(es)); use RuleStack::shipped/from_content",
                layer.name()
            ),
            OverlayError::FidelityViolation { layer, count } => {
                write!(
                    f,
                    "fidelity mode forbids the `{}` layer ({count} patch(es))",
                    layer.name()
                )
            }
        }
    }
}

impl std::error::Error for OverlayError {}

pub fn lookup(name: &str) -> Option<&'static RuleField> {
    let n = name.to_ascii_lowercase();
    FIELDS.iter().find(|f| f.name == n)
}

/// Where a given dword ended up, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attribution {
    pub layer: Layer,
    pub field: &'static str,
    pub index: u16,
    pub from: i32,
    pub to: i32,
    pub note: String,
}

/// The composed rule block plus a full record of who wrote what.
#[derive(Clone, Debug)]
pub struct RuleStack {
    mode: Mode,
    base: Rules,
    layers: BTreeMap<Layer, Vec<Patch>>,
    content_digest: u64,
}

impl RuleStack {
    /// Start from `don_rules::SHIPPED` — the values `ron-data/rules.xml` produces.
    pub fn shipped(mode: Mode) -> RuleStack {
        RuleStack {
            mode,
            base: Rules::shipped(),
            layers: BTreeMap::new(),
            content_digest: 0,
        }
    }

    /// Start from a base a content mod already replaced wholesale — the layer-2 result. The
    /// digest is the caller's summary of which files produced it, and it is what a lockstep
    /// handshake should compare (retail's analogue is `GameMod::compute_checksum`).
    pub fn from_content(mode: Mode, base: Rules, content_digest: u64) -> RuleStack {
        RuleStack {
            mode,
            base,
            layers: BTreeMap::new(),
            content_digest,
        }
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn content_digest(&self) -> u64 {
        self.content_digest
    }

    pub fn add(&mut self, layer: Layer, patch: Patch) -> &mut Self {
        self.layers.entry(layer).or_default().push(patch);
        self
    }

    pub fn extend(&mut self, layer: Layer, patches: impl IntoIterator<Item = Patch>) -> &mut Self {
        self.layers.entry(layer).or_default().extend(patches);
        self
    }

    pub fn patches(&self, layer: Layer) -> &[Patch] {
        self.layers.get(&layer).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Check without applying. Every error is reported, not just the first — a modder should
    /// see their whole file's problems in one pass.
    pub fn validate(&self) -> Vec<OverlayError> {
        let mut errs = Vec::new();
        for layer in [Layer::Shipped, Layer::Content] {
            let n = self.patches(layer).len();
            if n > 0 {
                errs.push(OverlayError::BaseLayerMustUseConstructor { layer, count: n });
            }
        }
        if self.mode == Mode::Fidelity {
            for layer in [Layer::Edition, Layer::Overlay, Layer::Session] {
                let n = self.patches(layer).len();
                if n > 0 {
                    errs.push(OverlayError::FidelityViolation { layer, count: n });
                }
            }
        }
        for patches in self.layers.values() {
            for p in patches {
                match lookup(&p.field) {
                    None => errs.push(OverlayError::UnknownField(p.field.clone())),
                    Some(f) if p.index >= f.count => errs.push(OverlayError::IndexOutOfRange {
                        field: p.field.clone(),
                        index: p.index,
                        count: f.count,
                    }),
                    Some(_) => {}
                }
            }
        }
        errs
    }

    /// Apply the layers in order and return the resulting block plus one [`Attribution`] per
    /// write that actually changed something.
    pub fn compose(&self) -> Result<(Rules, Vec<Attribution>), Vec<OverlayError>> {
        let errs = self.validate();
        if !errs.is_empty() {
            return Err(errs);
        }
        let mut out = self.base.clone();
        let mut audit = Vec::new();
        for (layer, patches) in self.layers.iter() {
            for p in patches {
                let f = lookup(&p.field).expect("validated");
                let slot = f.offset as usize / 4 + p.index as usize;
                debug_assert!(slot < RULES_DWORDS);
                let from = out.raw[slot];
                if from != p.value {
                    out.raw[slot] = p.value;
                    audit.push(Attribution {
                        layer: *layer,
                        field: f.name,
                        index: p.index,
                        from,
                        to: p.value,
                        note: p.note.clone(),
                    });
                }
            }
        }
        Ok((out, audit))
    }

    /// True when the composed block is byte-identical to the base — i.e. nothing deviates.
    /// The honest check a fidelity run should assert, rather than trusting the mode flag.
    pub fn is_byte_identical_to_base(&self) -> bool {
        match self.compose() {
            Ok((r, _)) => r.raw == self.base.raw,
            Err(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_patch_lands_on_the_right_dword() {
        let mut s = RuleStack::shipped(Mode::Improved);
        let before = Rules::shipped();
        let f = lookup("unit_move_speed").expect("field exists");
        s.add(
            Layer::Overlay,
            Patch::new("unit_move_speed", before.at(f.offset as usize) + 7),
        );
        let (after, audit) = s.compose().expect("composes");
        assert_eq!(audit.len(), 1);
        assert_eq!(audit[0].field, "unit_move_speed");
        assert_eq!(
            after.at(f.offset as usize),
            before.at(f.offset as usize) + 7
        );
        // and nothing else moved
        let diff = after
            .raw
            .iter()
            .zip(before.raw.iter())
            .filter(|(a, b)| a != b)
            .count();
        assert_eq!(diff, 1);
    }

    #[test]
    fn higher_layers_win() {
        let mut s = RuleStack::shipped(Mode::Improved);
        s.add(Layer::Edition, Patch::new("unit_move_speed", 111));
        s.add(Layer::Overlay, Patch::new("unit_move_speed", 222));
        s.add(Layer::Session, Patch::new("unit_move_speed", 333));
        let (r, _) = s.compose().unwrap();
        let f = lookup("unit_move_speed").unwrap();
        assert_eq!(r.at(f.offset as usize), 333);
    }

    #[test]
    fn an_unknown_field_is_an_error_not_a_shrug() {
        let mut s = RuleStack::shipped(Mode::Improved);
        s.add(Layer::Overlay, Patch::new("unit_move_speeed", 1));
        let errs = s.validate();
        assert_eq!(
            errs,
            vec![OverlayError::UnknownField("unit_move_speeed".into())]
        );
    }

    #[test]
    fn array_bounds_are_checked_against_the_engines_arity() {
        // Find a real array field rather than assuming one exists.
        let arr = FIELDS
            .iter()
            .find(|f| f.count > 1)
            .expect("some field is an array");
        let mut s = RuleStack::shipped(Mode::Improved);
        s.add(Layer::Overlay, Patch::at(arr.name, arr.count, 1));
        assert_eq!(
            s.validate(),
            vec![OverlayError::IndexOutOfRange {
                field: arr.name.to_string(),
                index: arr.count,
                count: arr.count
            }]
        );
    }

    #[test]
    fn fidelity_refuses_deviations_and_accepts_content_only_as_a_whole_base() {
        let mut s = RuleStack::shipped(Mode::Fidelity);
        s.add(Layer::Overlay, Patch::new("unit_move_speed", 1));
        assert!(matches!(
            s.validate().as_slice(),
            [OverlayError::FidelityViolation {
                layer: Layer::Overlay,
                count: 1
            }]
        ));

        let mut bad = RuleStack::shipped(Mode::Fidelity);
        bad.add(Layer::Content, Patch::new("unit_move_speed", 1));
        assert_eq!(
            bad.validate(),
            vec![OverlayError::BaseLayerMustUseConstructor {
                layer: Layer::Content,
                count: 1,
            }]
        );

        let mut content = Rules::shipped();
        content.raw[1] += 1;
        let c = RuleStack::from_content(Mode::Fidelity, content.clone(), 0x1234);
        assert!(c.validate().is_empty());
        assert_eq!(c.content_digest(), 0x1234);
        assert_eq!(c.compose().unwrap().0.raw, content.raw);
    }

    #[test]
    fn an_empty_stack_is_byte_identical_to_shipped() {
        let s = RuleStack::shipped(Mode::Fidelity);
        assert!(s.is_byte_identical_to_base());
        let (r, audit) = s.compose().unwrap();
        assert!(audit.is_empty());
        assert_eq!(r.raw, Rules::shipped().raw);
    }

    #[test]
    fn from_text_goes_through_the_derived_tokenizer() {
        // `unit_formation_spacing` is a Scaled(192) field [measured, Constants::init].
        let f = lookup("unit_formation_spacing").unwrap();
        assert!(matches!(f.parser, Parser::Scaled(192)));
        let p = Patch::from_text("unit_formation_spacing", 0, "3/4").unwrap();
        assert_eq!(p.value, don_rules::as_scaled("3/4", 192));
        // and a wtoi field is not silently scaled
        let w = FIELDS
            .iter()
            .find(|f| matches!(f.parser, Parser::Wtoi))
            .unwrap();
        let q = Patch::from_text(w.name, 0, "17").unwrap();
        assert_eq!(q.value, 17);
    }

    #[test]
    fn a_write_equal_to_the_existing_value_produces_no_attribution() {
        let base = Rules::shipped();
        let f = lookup("unit_move_speed").unwrap();
        let mut s = RuleStack::shipped(Mode::Improved);
        s.add(
            Layer::Overlay,
            Patch::new("unit_move_speed", base.at(f.offset as usize)),
        );
        let (_, audit) = s.compose().unwrap();
        assert!(audit.is_empty());
    }
}
