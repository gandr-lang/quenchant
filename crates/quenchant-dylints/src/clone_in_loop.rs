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
//! The repetition and the escape are [`crate::cost_shape`]'s: a loop body or
//! a per-element closure, else the body of a function that a loop, a role or
//! a repeated caller repeats ([`crate::repetition`]). A copy that leaves the
//! pass — the iteration's own value stored, pushed or handed on by value — is
//! accepted, and so is a copy on a path that diverges. `Iterator::cloned`
//! copies each element into whatever consumes the iterator, which is that
//! consumer's to price.
//!
//! A copy of a value that the repetition reads again and does not change is
//! refused even when it leaves the pass: each pass keeps an equal copy of
//! one value, where one shared value, an index or a borrow serves them all.
//! The repetition reads the value again when it is a local bound outside a
//! loop, or the receiver `self` of a function that a role repeats
//! ([`reread`]). A copy that a closure returns to a call stays accepted: the
//! call can run the closure once for each new key, and the pass can change
//! each copy after.
//!
//! Clippy's `redundant_clone`, `unnecessary_to_owned` and `implicit_clone`
//! refuse copies a move or a borrow serves within one statement; this lint
//! takes the copy repeated per pass.
//!
//! [`heap_ownership`]: crate::cost_shape::heap_ownership
//! [`reread`]: crate::cost_shape::reread

use clippy_utils::diagnostics::span_lint_and_then;
use clippy_utils::ty::is_copy;
use quenchant_shape::shape::Maybe;
use rustc_hir::Expr;
use rustc_hir::ExprKind;
use rustc_hir::def_id::DefId;
use rustc_lint::LateContext;
use rustc_lint::LateLintPass;
use rustc_lint::declare_lint;
use rustc_lint::impl_lint_pass;
use rustc_middle::ty::TyCtxt;
use rustc_span::sym;

use crate::cost_shape::Divergence;
use crate::cost_shape::Escape;
use crate::cost_shape::Inspection;
use crate::cost_shape::Machinery;
use crate::cost_shape::Method;
use crate::cost_shape::Mutation;
use crate::cost_shape::Owner;
use crate::cost_shape::Ownership;
use crate::cost_shape::Repetition;
use crate::cost_shape::divergence;
use crate::cost_shape::heap_ownership;
use crate::cost_shape::inspection;
use crate::cost_shape::pass_escape;
use crate::cost_shape::pass_mutation;
use crate::cost_shape::repetition;
use crate::cost_shape::reread;
use crate::cost_shape::returned_by;
use crate::cost_shape::std_method;
use crate::semantic::CopyingMethod;
use crate::walk_summary::standard_name;

declare_lint! {
    /// ### What it does
    ///
    /// Refuses `clone()`, `Option::cloned()`, `Result::cloned()`, `to_vec()` or `to_owned()` inside a `loop`, `while` or `for` body, a closure an iterator or a standard container calls once per element, or a function that a loop, an `Iterator::next` role or a repeated caller repeats, when the copy's type owns a heap allocation by its structure and the copy is dropped before the next pass, or copies a value the repetition reads again and does not change.
    ///
    /// ### Why is this bad?
    ///
    /// The copy allocates everything the original owns, and the pass drops it unread past the pass: `k` passes cost `k` deep copies where a borrow of the original costs none.
    ///
    /// ### Accepted forms
    ///
    /// The copy's type decides: a `Copy` type, a type whose components are all plain data, and an `Rc` or `Arc` are accepted, and so is a type whose ownership the site cannot read, such as a type parameter. A copy that leaves the pass — stored, pushed, returned or handed on by value — is accepted unless it copies a value the repetition reads again and does not change; a copy that a closure returns to a call is accepted. So is a copy outside any loop, in a loop over an array or a range with constant bounds, in a pass it leaves at once and on a path that diverges.
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

/// Why a refused copy is refused.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Waste
{
    /// The pass drops the copy before the next pass.
    Dropped,
    /// The copy repeats a value that the repetition does not change.
    Equal,
}

/// A refused copy: what repeats it, and why it is waste.
#[derive(Clone, Copy, Debug)]
pub struct Copied<'tcx>
{
    /// What makes the copy again on every pass.
    pub repetition: Repetition<'tcx>,
    /// Why the copy is waste.
    pub waste: Waste,
}

/// Repeated-copy policy; specification clauses are left to
/// `spec_clause_traversal`.
#[repr(transparent)]
pub struct CloneInLoop
{
    /// The machinery every cost-shape pass takes.
    machinery: Machinery,
}

impl CloneInLoop
{
    /// The pass over the shared machinery.
    ///
    /// # Specification
    /// trivial.
    #[must_use]
    pub const fn new(machinery: Machinery) -> Self
    {
        Self { machinery }
    }
}

impl_lint_pass!(CloneInLoop => [CLONE_IN_LOOP]);

impl<'tcx> LateLintPass<'tcx> for CloneInLoop
{
    /// Report each heap-owning copy a repeated body drops, or that repeats a
    /// value its repetition does not change.
    ///
    /// # Specification
    /// - ensures: reports every expression [`copy`] answers `Present` for, from
    ///   the method's name to the call's end, with the note of what repeats it.
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
        if inspection(&self.machinery.regions, expr) == Inspection::Skipped {
            return;
        }
        let Maybe::Present(found) = copy(cx, &self.machinery, expr)
        else {
            return;
        };
        let ExprKind::MethodCall(segment, ..) = expr.kind
        else {
            return;
        };
        let (message, help) = match found.waste {
            Waste::Dropped => (
                "this copies a heap-owning value on every pass of a loop, and the copy is \
                 dropped before the next pass",
                "borrow the original, or make the copy once before the loop and reuse it",
            ),
            Waste::Equal => (
                "this copies, on every pass of a loop, a heap-owning value that the loop does \
                 not change, so each pass keeps an equal copy",
                "share one value: keep an index or a borrow of the original, or an `Rc` or `Arc` \
                 of it, in place of each copy",
            ),
        };
        span_lint_and_then(
            cx,
            CLONE_IN_LOOP,
            segment.ident.span.with_hi(expr.span.hi()),
            message,
            |diagnostic| {
                found.repetition.note(cx.tcx, diagnostic);
                diagnostic.help(help);
            },
        );
    }
}

/// Whether a function is `Clone::clone` or `ToOwned::to_owned`, as its trait
/// declares it or as an implementation of it.
///
/// # Specification
/// - ensures: true exactly when `method`, or the trait item that it implements,
///   is the `clone` method of `Clone` or the `to_owned` method of `ToOwned`; a
///   function with no name, as a closure, is neither.
/// - panics: none.
/// - executable: none — the outcome is the compiler's definition tables, which
///   a predicate could only read again.
///
/// # Adequacy
/// - hypothesis: L3 — the clone fixtures refuse `clone()` and `to_owned()` in a
///   loop, and accept a type parameter's copy.
/// - witness: `tests::ui_cost_shape`
pub fn copying_method(
    tcx: TyCtxt<'_>,
    method: DefId,
) -> CopyingMethod
{
    let declared = tcx
        .opt_associated_item(method)
        .and_then(|item| item.trait_item_def_id())
        .unwrap_or(method);
    let Some(name) = tcx.opt_item_name(declared)
    else {
        return CopyingMethod(false);
    };
    CopyingMethod(tcx.trait_of_assoc(declared).is_some_and(|owner| {
        (tcx.is_diagnostic_item(sym::Clone, owner) && name.as_str() == "clone")
            || (tcx
                .get_diagnostic_name(owner)
                .is_some_and(|owner| owner.as_str() == "ToOwned")
                && name.as_str() == "to_owned")
    }))
}

/// Decide whether an expression is a heap-owning copy a repeated body drops,
/// or one that repeats a value its repetition does not change.
///
/// # Specification
/// - ensures: `Present` with the repetition when `expr` calls `Clone::clone`,
///   `ToOwned::to_owned`, a slice's `to_vec` or `cloned` on an `Option` or
///   `Result`, its result's type is not `Copy` and owns a heap allocation
///   ([`heap_ownership`]), and it sits in a repetition ([`repetition`]) on a
///   path that does not diverge ([`divergence`]); then `Dropped` when the copy
///   does not leave the pass ([`pass_escape`]), else `Equal` when the receiver
///   is a local the repetition reads again ([`reread`]) and does not change
///   ([`pass_mutation`]) and no closure returns the copy ([`returned_by`]).
/// - ensures: `Absent` with the first reason that holds otherwise, in the order
///   the body tests them; a type parameter's copy is `Absent(Unknown)`.
/// - panics: none.
/// - executable: none — the outcome is the compiler's resolution, type layout
///   and use walk of the enclosing body, which a predicate could only repeat.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures refuse a copied `Vec`, `String`, a struct
///   holding a map and an `Option` of a boxed value, each dropped in its pass,
///   and a copy of an unchanged outer value stored in each pass; and accept a
///   `Copy` struct, a struct of plain fields, an `Rc`, a type parameter, a copy
///   of each element pushed outward, a copy a closure returns to a map's entry
///   and a copy outside any loop.
/// - witness: `tests::ui_cost_shape`
pub fn copy<'tcx>(
    cx: &LateContext<'tcx>,
    machinery: &Machinery,
    expr: &'tcx Expr<'tcx>,
) -> Maybe<Copied<'tcx>, clone_reading::Unrefused>
{
    let not_a_copy = Maybe::Absent(clone_reading::Unrefused::NotACopy);
    let ExprKind::MethodCall(_, receiver, ..) = expr.kind
    else {
        return not_a_copy;
    };
    let Some(method) = cx.typeck_results().type_dependent_def_id(expr.hir_id)
    else {
        return not_a_copy;
    };
    let name = cx.tcx.item_name(method);
    let by_trait = copying_method(cx.tcx, method).0;
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
    let found = match repetition(cx, machinery, expr) {
        Maybe::Present(found) => found,
        Maybe::Absent(reason) => {
            return Maybe::Absent(clone_reading::Unrefused::Unrepeated(reason));
        },
    };
    let scope = found.scope();
    if divergence(cx, expr, scope) == Divergence::Diverges {
        return Maybe::Absent(clone_reading::Unrefused::Diverges);
    }
    if pass_escape(cx, expr, scope) == Escape::Stays {
        return Maybe::Present(Copied {
            repetition: found,
            waste: Waste::Dropped,
        });
    }
    let equal = match reread(cx, found, receiver) {
        Maybe::Present(local) => {
            pass_mutation(cx, local, scope) == Mutation::Untouched
                && matches!(returned_by(cx, expr), Maybe::Absent(_))
        },
        Maybe::Absent(_) => false,
    };
    if !equal {
        return Maybe::Absent(clone_reading::Unrefused::Escapes);
    }
    Maybe::Present(Copied {
        repetition: found,
        waste: Waste::Equal,
    })
}
