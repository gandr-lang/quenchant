//! Absence held in a field names its reason, unless the field is wire form.
//!
//! A struct or enum-variant field typed `Option` records that a value may be
//! missing, not why; a reader of the structure learns nothing from `None`.
//! The one place a bare `Option` field is the right shape is a parse or
//! serialization target: a serialized `Option` is a value the writer has not
//! supplied, and a parsed `Option` is an argument the user did not pass, and
//! in both the protocol is the reason. An item is wire form when it
//! implements serde's `Serialize` or `Deserialize` or clap's `FromArgMatches`,
//! whether derived or written by hand; the compiler's implementation index
//! answers, so a hand-written impl counts and a derive on another type does
//! not.
//!
//! Fields are walked with the same traversal as [`crate::option_signature`],
//! so aliases, containers, references, tuples, and opaque outputs resolve the
//! same way in both positions.

use core::cell::OnceCell;

use rustc_hir::Item;
use rustc_hir::ItemKind;
use rustc_hir::def_id::DefId;
use rustc_hir::def_id::LocalDefId;
use rustc_lint::LateContext;
use rustc_lint::LateLintPass;
use rustc_middle::ty::fast_reject::SimplifiedType;
use rustc_session::declare_lint;
use rustc_session::impl_lint_pass;
use rustc_span::Span;

use crate::option_signature::Exposure;
use crate::option_signature::Required;
use crate::option_signature::exposure;
use crate::semantic::WireFormItem;

declare_lint! {
    /// Reject standard Option in the fields of a crate-defined struct or enum
    /// that is not a serialization or argument-parsing target.
    pub OPTION_FIELD,
    Allow,
    "crate-defined fields must preserve the reason for absence unless they are wire form"
}

/// Wire-form traits, by path. Serde re-exports its traits from `serde_core`,
/// and clap re-exports `FromArgMatches`, which every clap derive implements,
/// from `clap_builder`; each spelling resolves to the same trait, so a crate
/// that links either one is covered.
const WIRE_FORM_TRAITS: [&str; 6] = [
    "serde::ser::Serialize",
    "serde::de::Deserialize",
    "serde_core::ser::Serialize",
    "serde_core::de::Deserialize",
    "clap::FromArgMatches",
    "clap_builder::FromArgMatches",
];

/// Field-level absence policy; the wire-form traits are resolved once per
/// crate.
#[derive(Default)]
#[repr(transparent)]
pub struct OptionField
{
    /// The definitions of every wire-form trait among this crate's
    /// dependencies, resolved on the first item with fields.
    traits: OnceCell<Vec<DefId>>,
}

impl_lint_pass!(OptionField => [OPTION_FIELD]);

impl<'tcx> LateLintPass<'tcx> for OptionField
{
    /// Inspect the fields of every local struct and enum.
    ///
    /// # Specification
    /// - ensures: each `Option` reached by a field of a struct or enum variant
    ///   is reported once at the field's type, unless the item implements a
    ///   wire-form trait; other items are not inspected.
    /// - panics: none.
    ///
    /// # Adequacy
    /// - hypothesis: L3 UI contrasts plain structs and enum variants against
    ///   items with a derived serde trait, a hand-written serde impl, and a
    ///   clap parser derive, with nested and aliased absence in each.
    /// - witness: `tests::ui_options`
    fn check_item(
        &mut self,
        cx: &LateContext<'tcx>,
        item: &'tcx Item<'tcx>,
    )
    {
        let fields: Vec<&rustc_hir::FieldDef<'tcx>> = match item.kind {
            | ItemKind::Struct(_, _, ref data) => data.fields().iter().collect(),
            | ItemKind::Enum(_, _, ref definition) => definition
                .variants
                .iter()
                .flat_map(|variant| variant.data.fields())
                .collect(),
            | _ => return,
        };
        if fields.is_empty() {
            return;
        }
        let traits = self.traits.get_or_init(|| wire_form_traits(cx));
        if implements_wire_form(cx, traits, item.owner_id.def_id).0 {
            return;
        }
        for field in fields {
            let actual = cx
                .tcx
                .type_of(field.def_id)
                .instantiate_identity()
                .skip_norm_wip();
            if matches!(exposure(cx, actual, Required::Authored), Exposure::Option) {
                emit(cx, field.ty.span);
            }
        }
    }
}

/// Wire-form trait identities are resolved once per crate.
///
/// # Specification
/// - ensures: returns the definition of every serde `Serialize` and
///   `Deserialize` trait and every clap `FromArgMatches` trait among this
///   crate's dependencies, each once; a crate that links neither yields none.
/// - panics: none.
fn wire_form_traits(cx: &LateContext<'_>) -> Vec<DefId>
{
    let mut traits = Vec::new();
    for path in WIRE_FORM_TRAITS {
        for def_id in
            clippy_utils::paths::lookup_path_str(cx.tcx, clippy_utils::paths::PathNS::Type, path)
        {
            if !traits.contains(&def_id) {
                traits.push(def_id);
            }
        }
    }
    traits
}

/// The compiler's implementation index decides whether an item is wire form.
///
/// # Specification
/// - requires: `traits` is [`wire_form_traits`] for this crate.
/// - ensures: affirmative exactly when some trait in `traits` has a non-blanket
///   implementation whose self type is the ADT `adt`, whether a derive or the
///   author wrote it.
/// - panics: none.
fn implements_wire_form(
    cx: &LateContext<'_>,
    traits: &[DefId],
    adt: LocalDefId,
) -> WireFormItem
{
    let key = SimplifiedType::Adt(adt.to_def_id());
    WireFormItem(traits.iter().any(|&trait_def_id| {
        cx.tcx
            .trait_impls_of(trait_def_id)
            .non_blanket_impls()
            .contains_key(&key)
    }))
}

/// Anchor the field absence diagnostic to the field's written type.
///
/// # Specification
/// trivial.
fn emit(
    cx: &LateContext<'_>,
    span: Span,
)
{
    clippy_utils::diagnostics::span_lint(
        cx,
        OPTION_FIELD,
        span,
        "crate-defined fields must preserve the reason for absence instead of holding Option; \
         a serde or clap target may keep it as wire form",
    );
}
