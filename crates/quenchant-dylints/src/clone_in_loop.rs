//! A copy of a heap-owning value made and dropped within one pass of a
//! repeated body.
//!
//! `clone()`, `Option::cloned()`, `to_vec()` and `to_owned()` of a value that
//! owns a heap allocation allocate a copy of everything it owns. Inside a
//! loop body whose pass drops the copy before the next, `k` passes cost `k`
//! deep copies where a borrow of the original costs none: the copy was made
//! only to be read.
//!
//! The copied type decides. A type owns a heap allocation when it, or a type
//! reached through its fields, variants, tuple and array elements and its
//! resolved generic arguments, is a `Box`, `Vec`, `VecDeque`, `String` or a
//! standard map, set, heap or list ([`heap_ownership`]). A `Copy` type, a type
//! whose components are all plain data, and an `Rc` or `Arc` — whose copy
//! bumps a count — are accepted by type. A type whose ownership cannot be read
//! at the site, a type parameter above all, is accepted and counted as
//! unknown.
//!
//! # Boundaries
//!
//! The repetition and the escape are [`crate::cost_shape`]'s: a copy that
//! leaves the pass — the iteration's own value stored, pushed or handed on by
//! value — is accepted, and so is a copy on a path that diverges.
//! `Iterator::cloned` copies each element into whatever consumes the
//! iterator, which is that consumer's to price.
//!
//! Clippy's `redundant_clone`, `unnecessary_to_owned` and `implicit_clone`
//! refuse copies a move or a borrow serves within one statement; this lint
//! takes the copy repeated per pass.
//!
//! [`heap_ownership`]: crate::cost_shape::heap_ownership

use clippy_utils::diagnostics::span_lint_and_then;
use clippy_utils::ty::is_copy;
use quenchant_shape::shape::Maybe;
use rustc_hir::Expr;
use rustc_hir::ExprKind;
use rustc_lint::LateContext;
use rustc_lint::LateLintPass;
use rustc_session::declare_lint;
use rustc_session::impl_lint_pass;
use rustc_span::sym;

use crate::clause_cost::ClauseRegions;
use crate::cost_shape::Divergence;
use crate::cost_shape::Escape;
use crate::cost_shape::Inspection;
use crate::cost_shape::Method;
use crate::cost_shape::Owner;
use crate::cost_shape::Ownership;
use crate::cost_shape::divergence;
use crate::cost_shape::heap_ownership;
use crate::cost_shape::inspection;
use crate::cost_shape::pass_escape;
use crate::cost_shape::repetition;
use crate::cost_shape::std_method;
use crate::walk_summary::standard_name;

declare_lint! {
    /// ### What it does
    ///
    /// Refuses `clone()`, `Option::cloned()`, `Result::cloned()`, `to_vec()` or `to_owned()` inside a `loop`, `while` or `for` body, or a closure an iterator or a standard container calls once per element, when the copy's type owns a heap allocation by its structure and the copy is dropped before the next pass.
    ///
    /// ### Why is this bad?
    ///
    /// The copy allocates everything the original owns, and the pass drops it unread past the pass: `k` passes cost `k` deep copies where a borrow of the original costs none.
    ///
    /// ### Accepted forms
    ///
    /// The copy's type decides: a `Copy` type, a type whose components are all plain data, and an `Rc` or `Arc` are accepted, and so is a type whose ownership the site cannot read, such as a type parameter. A copy that leaves the pass — stored, pushed, returned or handed on by value — is accepted, and so is a copy outside any loop, in a loop over an array or a range with constant bounds, in a pass it leaves at once and on a path that diverges.
    ///
    /// ### Clippy
    ///
    /// `redundant_clone`, `unnecessary_to_owned` and `implicit_clone` refuse a copy a move or a borrow serves within one statement; this lint takes the copy repeated per pass.
    ///
    /// ### Example
    ///
    /// ```rust,ignore
    /// for query in queries {
    ///     let names = index.names.clone();
    ///     if names.contains(&query) { hits += 1; }
    /// }
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// for query in queries {
    ///     if index.names.contains(&query) { hits += 1; }
    /// }
    /// ```
    pub CLONE_IN_LOOP,
    Deny,
    "a repeated body copies a heap-owning value it drops before the next pass"
}

quenchant_shape::reason_enum! {
    /// A copy can be cheap, or be needed past its pass.
    pub mod clone_reading {
        /// Why the copy is not refused.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Unrefused {
            /// The call is not a copy this lint prices.
            NotACopy,
            /// The copy's type is `Copy`.
            Copy,
            /// No component of the copy's type owns a heap allocation.
            PlainData,
            /// The copy bumps a reference count.
            Refcounted,
            /// The copy's ownership cannot be read at the site: a type
            /// parameter, an opaque type, a foreign type with a destructor.
            Unknown,
            /// The copy does not repeat; the reason is the repetition's.
            Unrepeated(crate::cost_shape::repetition_reading::Unrepeated),
            /// The copy sits on a path that diverges.
            Diverges,
            /// The copy leaves the pass.
            Escapes,
        }
    }
}

/// Repeated-copy policy; specification clauses are left to
/// `spec_clause_traversal`.
#[repr(transparent)]
pub struct CloneInLoop
{
    /// The authored `spec` attribute ranges, whose clauses are not inspected.
    regions: ClauseRegions,
}

impl CloneInLoop
{
    /// The pass reads the ranges the pre-expansion clause collector records.
    ///
    /// # Specification
    /// trivial.
    #[must_use]
    pub const fn new(regions: ClauseRegions) -> Self
    {
        Self { regions }
    }
}

impl_lint_pass!(CloneInLoop => [CLONE_IN_LOOP]);

impl<'tcx> LateLintPass<'tcx> for CloneInLoop
{
    /// Report each heap-owning copy a repeated body drops.
    ///
    /// # Specification
    /// - ensures: reports every expression [`copy`] answers `Present` for, from
    ///   the method's name to the call's end, with a note at the loop that
    ///   repeats it.
    /// - ensures: expressions from macro expansions and inside `spec`
    ///   attributes are not inspected ([`inspection`]).
    /// - panics: none.
    /// - executable: none — rustc emits diagnostics without a queryable
    ///   per-call diagnostic result.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the fixtures refuse each copying method in each
    ///   repetition and accept each exception beside it.
    /// - witness: `tests::ui_cost_shape`
    fn check_expr(
        &mut self,
        cx: &LateContext<'tcx>,
        expr: &'tcx Expr<'tcx>,
    )
    {
        if inspection(&self.regions, expr) == Inspection::Skipped {
            return;
        }
        let Maybe::Present(scope) = copy(cx, expr)
        else {
            return;
        };
        let ExprKind::MethodCall(segment, ..) = expr.kind
        else {
            return;
        };
        span_lint_and_then(
            cx,
            CLONE_IN_LOOP,
            segment.ident.span.with_hi(expr.span.hi()),
            "this copies a heap-owning value on every pass of a loop, and the copy is dropped \
             before the next pass",
            |diagnostic| {
                diagnostic.span_note(scope.span, "the loop that repeats it");
                diagnostic.help(
                    "borrow the original, or make the copy once before the loop and reuse it",
                );
            },
        );
    }
}

/// Decide whether an expression is a heap-owning copy a repeated body drops.
///
/// # Specification
/// - ensures: `Present` with the repetition when `expr` calls `Clone::clone`,
///   `ToOwned::to_owned`, a slice's `to_vec` or `cloned` on an `Option` or
///   `Result`, its result's type is not `Copy` and owns a heap allocation
///   ([`heap_ownership`]), and it sits in an open repetition ([`repetition`])
///   on a path that does not diverge ([`divergence`]) and does not leave the
///   pass ([`pass_escape`]).
/// - ensures: `Absent` with the first reason that holds otherwise, in the order
///   the body tests them; a type parameter's copy is `Absent(Unknown)`.
/// - panics: none.
/// - executable: none — the outcome is the compiler's resolution, type layout
///   and use walk of the enclosing body, which a predicate could only repeat.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures refuse a copied `Vec`, `String`, a struct
///   holding a map and an `Option` of a boxed value, each dropped in its pass,
///   and accept a `Copy` struct, a struct of plain fields, an `Rc`, a type
///   parameter, a copy pushed outward and a copy outside any loop.
/// - witness: `tests::ui_cost_shape`
pub fn copy<'tcx>(
    cx: &LateContext<'tcx>,
    expr: &'tcx Expr<'tcx>,
) -> Maybe<&'tcx Expr<'tcx>, clone_reading::Unrefused>
{
    let not_a_copy = Maybe::Absent(clone_reading::Unrefused::NotACopy);
    if !matches!(expr.kind, ExprKind::MethodCall(..)) {
        return not_a_copy;
    }
    let Some(method) = cx.typeck_results().type_dependent_def_id(expr.hir_id)
    else {
        return not_a_copy;
    };
    let name = cx.tcx.item_name(method);
    let by_trait = cx.tcx.trait_of_assoc(method).is_some_and(|owner| {
        (cx.tcx.is_diagnostic_item(sym::Clone, owner) && name.as_str() == "clone")
            || (cx
                .tcx
                .get_diagnostic_name(owner)
                .is_some_and(|owner| owner.as_str() == "ToOwned")
                && name.as_str() == "to_owned")
    });
    let by_slice = matches!(
        std_method(cx, expr),
        Maybe::Present(Method { owner: Owner::Slice, name }) if name.as_str() == "to_vec"
    );
    let by_option = name.as_str() == "cloned"
        && cx
            .tcx
            .inherent_impl_of_assoc(method)
            .and_then(|implementation| {
                cx.tcx
                    .type_of(implementation)
                    .instantiate_identity()
                    .skip_norm_wip()
                    .ty_adt_def()
            })
            .is_some_and(|definition| {
                matches!(
                    standard_name(cx.tcx, definition.did()),
                    Maybe::Present(owner) if matches!(owner.as_str(), "Option" | "Result")
                )
            });
    if !by_trait && !by_slice && !by_option {
        return not_a_copy;
    }
    let copied = cx.typeck_results().expr_ty(expr);
    if is_copy(cx, copied) {
        return Maybe::Absent(clone_reading::Unrefused::Copy);
    }
    match heap_ownership(cx.tcx, cx.typing_env(), copied) {
        Ownership::Owning => {},
        Ownership::Unknown => return Maybe::Absent(clone_reading::Unrefused::Unknown),
        Ownership::Refcounted => return Maybe::Absent(clone_reading::Unrefused::Refcounted),
        Ownership::Plain => return Maybe::Absent(clone_reading::Unrefused::PlainData),
    }
    let scope = match repetition(cx, expr) {
        Maybe::Present(scope) => scope,
        Maybe::Absent(reason) => {
            return Maybe::Absent(clone_reading::Unrefused::Unrepeated(reason));
        },
    };
    if divergence(cx, expr, scope) == Divergence::Diverges {
        return Maybe::Absent(clone_reading::Unrefused::Diverges);
    }
    if pass_escape(cx, expr, scope) == Escape::Escapes {
        return Maybe::Absent(clone_reading::Unrefused::Escapes);
    }
    Maybe::Present(scope)
}
