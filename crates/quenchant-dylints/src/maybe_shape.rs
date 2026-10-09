//! A crate takes `Maybe` from `quenchant-shape` rather than defining its own.
//!
//! `Maybe<T, R>` is the canonical non-failure absence: `Present(T)` or
//! `Absent(R)` with a sealed per-site reason. A crate-defined enum of the same
//! shape, two variants each carrying one value of its own type parameter,
//! reimplements it under another name and loses the identity the absence rules
//! key on; the same shape is also `Result`'s, and the fix for a failure is
//! `Result`.
//!
//! The shape is read once, at the definition, rather than at every signature
//! that names it. A unit variant beside a payload is not this shape: the
//! discipline models a closed state set as one enum, and the unit variant's
//! name is its reason. A per-site enum that fixes the reason type is a closed
//! state enum by the same reading. Both stay with review.

use clippy_utils::diagnostics::span_lint_and_help;
use quenchant_shape::shape::Maybe;
use rustc_hir::Item;
use rustc_hir::ItemKind;
use rustc_hir::Variant;
use rustc_lint::LateContext;
use rustc_lint::LateLintPass;
use rustc_middle::ty;
use rustc_session::declare_lint;
use rustc_session::impl_lint_pass;
use rustc_span::Symbol;

use crate::specification::name_authored;

quenchant_shape::reason_enum! {
    /// A variant can carry something other than one parameter-typed value.
    mod payload_reading {
        /// Why the variant carries no single type-parameter payload.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum NotAParameter {
            /// The variant carries no field, or more than one.
            FieldCount,
            /// The one field's type is not a bare type parameter of the enum.
            Concrete,
        }
    }
}

declare_lint! {
    /// Reject a crate-defined enum with the shape of `quenchant_shape::Maybe`.
    ///
    /// ### What it does
    ///
    /// Reports, at its name, a crate-authored enum with exactly two variants
    /// that each carry exactly one value, typed by two distinct type
    /// parameters of the enum. The canonical `Maybe`, identified by its
    /// `quenchant_maybe` diagnostic item, is not reported.
    ///
    /// ### Why is this bad?
    ///
    /// A non-failure absence is `Maybe<T, R>` with a sealed per-site reason,
    /// and a failure is `Result<T, E>`. An enum of their shape under another
    /// name gives up the canonical identity the absence-signature rules
    /// recognise, and every reader learns a second spelling of the same type.
    ///
    /// ### What it does not decide
    ///
    /// A generic two-way choice that is neither absence nor failure has the
    /// same shape; it is reported, and the item allows the lint with a reason.
    /// A unit variant beside a payload, or a per-site enum that fixes the
    /// reason type, is a closed state enum whose variant names carry its
    /// meaning, and stays with review.
    ///
    /// ### Example
    ///
    /// ```rust,ignore
    /// enum Lookup<Value, Reason> {
    ///     Found(Value),
    ///     Missing(Reason),
    /// }
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// use quenchant_shape::shape::Maybe;
    ///
    /// fn lookup(key: Key) -> Maybe<Value, lookup::Missing>;
    /// ```
    pub MAYBE_SHAPE,
    Allow,
    "a crate takes `Maybe` from `quenchant-shape` rather than defining an enum of its shape"
}

/// Definition-site shape policy; stateless.
#[derive(Default)]
pub struct MaybeShape;

impl_lint_pass!(MaybeShape => [MAYBE_SHAPE]);

impl<'tcx> LateLintPass<'tcx> for MaybeShape
{
    /// Inspect every crate-authored enum once, at its definition.
    ///
    /// # Specification
    /// - ensures: reports once, at the enum's name, each crate-authored enum
    ///   other than the canonical `Maybe` whose exactly two variants each carry
    ///   one value typed by a type parameter of the enum, the two parameters
    ///   distinct; every other item, and every enum the lint is allowed for, is
    ///   not inspected.
    /// - panics: none.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the shape matrix separates tuple and named payloads
    ///   over two parameters (reported) from a unit variant beside a payload, a
    ///   concrete reason, one parameter twice, a wrapped parameter, a third
    ///   variant, and a locally declared canonical identity (all accepted).
    /// - witness: `tests::ui_shapes`
    fn check_item(
        &mut self,
        cx: &LateContext<'tcx>,
        item: &'tcx Item<'tcx>,
    )
    {
        let ItemKind::Enum(ident, _, ref definition) = item.kind
        else {
            return;
        };
        let def_id = item.owner_id.def_id;
        if cx
            .tcx
            .lint_level_spec_at_node(MAYBE_SHAPE, item.hir_id())
            .level()
            == rustc_session::lint::Level::Allow
            || !name_authored(cx, def_id).0
            || cx
                .tcx
                .is_diagnostic_item(Symbol::intern("quenchant_maybe"), def_id.to_def_id())
        {
            return;
        }
        let [ref first, ref second] = *definition.variants
        else {
            return;
        };
        if let (Maybe::Present(present), Maybe::Present(absent)) =
            (parameter_payload(cx, first), parameter_payload(cx, second))
            && present != absent
        {
            span_lint_and_help(
                cx,
                MAYBE_SHAPE,
                ident.span,
                "this enum has the shape of `quenchant_shape::Maybe`: two variants, each carrying one \
                 value of its own type parameter",
                None,
                "a non-failure absence is `quenchant_shape::shape::Maybe<T, R>` with a sealed \
                 per-site reason, and a failure is `Result<T, E>`; a two-way choice that is neither \
                 allows this lint at the item with its reason",
            );
        }
    }
}

/// A variant's payload is read as a bare type parameter or not at all.
///
/// # Specification
/// - ensures: yields the type parameter exactly when the variant, tuple or
///   named, has one field whose declared type is a bare type parameter of the
///   enclosing enum; a wrapped parameter, a concrete type, a unit variant and a
///   variant with several fields yield the reason.
/// - panics: none.
fn parameter_payload(
    cx: &LateContext<'_>,
    variant: &Variant<'_>,
) -> Maybe<ty::ParamTy, payload_reading::NotAParameter>
{
    let [ref field] = *variant.data.fields()
    else {
        return Maybe::Absent(payload_reading::NotAParameter::FieldCount);
    };
    match *cx
        .tcx
        .type_of(field.def_id)
        .instantiate_identity()
        .skip_norm_wip()
        .kind()
    {
        | ty::Param(parameter) => Maybe::Present(parameter),
        | _ => Maybe::Absent(payload_reading::NotAParameter::Concrete),
    }
}
