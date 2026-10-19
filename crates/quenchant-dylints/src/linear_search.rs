//! A linear search repeated by a loop over a collection that outlives it.
//!
//! `contains` on a slice, a `Vec` or a `VecDeque`, and a keyed `any`,
//! `position`, `rposition` or `find` over a slice iterator, compare against
//! every element until one matches. Once, that is the cost of reading the
//! collection. Repeated by a loop or a per-element closure over a collection
//! bound outside it, the work is the product of the two lengths: `m` searches
//! of an `n`-element collection cost `O(n·m)`, where a `HashSet` or
//! `BTreeSet` built once costs `O(n)` or `O(n log n)` to build and `O(1)` or
//! `O(log n)` per search, and a sorted slice answers `binary_search` in
//! `O(log n)`.
//!
//! Clippy's `manual_contains` turns `iter().any(|x| *x == y)` into `contains`
//! and says nothing of repetition; nothing in Clippy prices a search by the
//! loop around it.
//!
//! # Boundaries
//!
//! The receiver's type decides. A set or map's own `contains`, `contains_key`
//! and `get` are not linear and are never inspected; neither is `str`'s
//! substring search or a range's `contains`. An array, or a reference to one,
//! has a length its type fixes, so a search of it costs a constant. The
//! repetition decides too ([`crate::cost_shape`]): a search outside any loop,
//! inside a loop over an array, on a collection bound inside the loop, on a
//! constant or static item, or on a value computed in place is accepted.
//!
//! The iterator forms are refused only when their closure is a key
//! comparison: an `==` whose one side reads the element and whose other side
//! does not. Any other predicate has no index that answers it in general.

use anodized::spec;
use clippy_utils::diagnostics::span_lint_and_then;
use clippy_utils::visitors::is_local_used;
use quenchant_shape::shape::Maybe;
use rustc_hir::BinOpKind;
use rustc_hir::Expr;
use rustc_hir::ExprKind;
use rustc_lint::LateContext;
use rustc_lint::LateLintPass;
use rustc_session::declare_lint;
use rustc_session::impl_lint_pass;
use rustc_span::sym;

use crate::clause_cost::ClauseRegions;
use crate::cost_shape::Inspection;
use crate::cost_shape::Method;
use crate::cost_shape::Owner;
use crate::cost_shape::Repeated;
use crate::cost_shape::Trip;
use crate::cost_shape::inspection;
use crate::cost_shape::length_trip;
use crate::cost_shape::repeated_read;
use crate::cost_shape::std_method;

declare_lint! {
    /// ### What it does
    ///
    /// Refuses a linear search repeated by a loop over a collection that outlives it: `contains` on a slice, a `Vec` or a `VecDeque`, or `any`, `position`, `rposition` or `find` over a slice's `iter()` whose closure compares the element with a key by `==`, inside a `loop`, `while` or `for` body or a closure an iterator or a standard container calls once per element, when the searched collection is bound outside that loop.
    ///
    /// ### Why is this bad?
    ///
    /// Each search compares against every element. `m` searches of an `n`-element collection cost `O(n·m)`. A `HashSet` or `BTreeSet` built once before the loop costs `O(n)` or `O(n log n)` and answers each search in `O(1)` or `O(log n)`; a slice sorted once answers `binary_search` in `O(log n)`.
    ///
    /// ### Accepted forms
    ///
    /// The receiver's type decides: sets and maps, `str`'s substring search and ranges are never inspected, and an array or a reference to one has a length its type fixes. A search outside any loop, inside a loop over an array or a range with constant bounds, on a collection bound inside the loop, on a constant or static item, or on a value computed in place is accepted. An iterator search whose closure is not a key comparison has no index that answers it in general and is accepted.
    ///
    /// ### Clippy
    ///
    /// `manual_contains` rewrites `iter().any(|x| *x == y)` as `contains`; no Clippy lint prices a search by the loop that repeats it.
    ///
    /// ### Example
    ///
    /// ```rust,ignore
    /// let mut seen = Vec::new();
    /// for name in names {
    ///     if !seen.contains(&name) {
    ///         seen.push(name);
    ///     }
    /// }
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// let mut seen = BTreeSet::new();
    /// for name in names {
    ///     seen.insert(name);
    /// }
    /// ```
    pub LINEAR_SEARCH_IN_LOOP,
    Deny,
    "a linear search is repeated by a loop over a collection that outlives it"
}

quenchant_shape::reason_enum! {
    /// A search can cost no more than reading its collection once per loop.
    pub mod search_reading {
        /// Why the search is not refused.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Bounded {
            /// The call is not a linear search of a slice, `Vec` or
            /// `VecDeque`.
            NotASearch,
            /// The searched collection is an array, its length fixed by its
            /// type.
            FixedLength,
            /// The iterator search's closure is not a key comparison.
            NotKeyed,
            /// The search does not repeat over one collection; the reason is
            /// the repetition's.
            Unrepeated(crate::cost_shape::repetition_reading::Unrepeated),
        }
    }
}

/// A refused search: the searched collection and the loop that repeats it.
#[derive(Clone, Copy, Debug)]
pub struct Search<'tcx>
{
    /// The collection the search walks.
    pub collection: &'tcx Expr<'tcx>,
    /// The repetition that searches it again on every pass.
    pub repeated: Repeated<'tcx>,
}

/// Repeated-search policy; specification clauses are left to
/// `spec_clause_traversal`.
#[repr(transparent)]
pub struct LinearSearchInLoop
{
    /// The authored `spec` attribute ranges, whose clauses are not inspected.
    regions: ClauseRegions,
}

impl LinearSearchInLoop
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

impl_lint_pass!(LinearSearchInLoop => [LINEAR_SEARCH_IN_LOOP]);

impl<'tcx> LateLintPass<'tcx> for LinearSearchInLoop
{
    /// Report each repeated linear search once, at the search.
    ///
    /// # Specification
    /// - ensures: reports every expression [`search`] answers `Present` for,
    ///   labelling the collection, with a note at the loop that repeats it.
    /// - ensures: expressions from macro expansions and inside `spec`
    ///   attributes are not inspected ([`inspection`]).
    /// - panics: none.
    /// - executable: none — rustc emits diagnostics without a queryable
    ///   per-call diagnostic result.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the fixtures refuse each search form in each
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
        let Maybe::Present(found) = search(cx, expr)
        else {
            return;
        };
        span_lint_and_then(
            cx,
            LINEAR_SEARCH_IN_LOOP,
            expr.span,
            "this linear search runs once per pass of a loop over a collection that outlives it",
            |diagnostic| {
                diagnostic.span_label(found.collection.span, "searched again on every pass");
                diagnostic.span_note(found.repeated.scope.span, "the loop that repeats it");
                diagnostic.help(
                    "build a `HashSet` or `BTreeSet` of the collection once before the loop, or \
                     sort it once and use `binary_search`",
                );
            },
        );
    }
}

/// Decide whether an expression is a repeated linear search.
///
/// # Specification
/// - ensures: `Present` when `expr` is `contains` on a slice, `Vec` or
///   `VecDeque`, or a keyed `any`, `position`, `rposition` or `find` over a
///   slice iterator, whose collection is not an array and is read again by an
///   enclosing repetition ([`repeated_read`]).
/// - ensures: `Absent(NotASearch)` for every other expression;
///   `Absent(FixedLength)` for an array; `Absent(NotKeyed)` for an iterator
///   search whose closure is not a key comparison; `Absent(Unrepeated)` with
///   the repetition's reason otherwise.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures refuse `contains` on a vector, a slice and a
///   deque and each keyed iterator form in a `for`, a `while` and a `filter`
///   closure, and accept arrays, unkeyed closures, sets, `str` and every
///   repetition exception.
/// - witness: `tests::ui_cost_shape`
#[spec(ensures: |output| match output {
    Maybe::Present(found) => matches!(length_trip(cx.typeck_results().expr_ty(found.collection)), Trip::Open),
    Maybe::Absent(_) => true,
})]
pub fn search<'tcx>(
    cx: &LateContext<'tcx>,
    expr: &'tcx Expr<'tcx>,
) -> Maybe<Search<'tcx>, search_reading::Bounded>
{
    let ExprKind::MethodCall(_, receiver, arguments, _) = expr.kind
    else {
        return Maybe::Absent(search_reading::Bounded::NotASearch);
    };
    let Maybe::Present(Method { owner, name }) = std_method(cx, expr)
    else {
        return Maybe::Absent(search_reading::Bounded::NotASearch);
    };
    let collection = match (owner, name.as_str()) {
        (Owner::Slice | Owner::VecDeque, "contains") => receiver,
        (Owner::Iterator, "any" | "position" | "rposition" | "find") => {
            let Maybe::Present(collection) = slice_iteration(cx, receiver)
            else {
                return Maybe::Absent(search_reading::Bounded::NotASearch);
            };
            let &[ref predicate] = arguments
            else {
                return Maybe::Absent(search_reading::Bounded::NotASearch);
            };
            if matches!(keyed(cx, predicate), Keyed::Other) {
                return Maybe::Absent(search_reading::Bounded::NotKeyed);
            }
            collection
        },
        _ => return Maybe::Absent(search_reading::Bounded::NotASearch),
    };
    if matches!(
        length_trip(cx.typeck_results().expr_ty(collection)),
        Trip::Fixed
    ) {
        return Maybe::Absent(search_reading::Bounded::FixedLength);
    }
    match repeated_read(cx, expr, collection) {
        Maybe::Present(repeated) => Maybe::Present(Search {
            collection,
            repeated,
        }),
        Maybe::Absent(reason) => Maybe::Absent(search_reading::Bounded::Unrepeated(reason)),
    }
}

quenchant_shape::reason_enum! {
    /// An iterator can be something other than a slice's own iteration.
    mod iteration_reading {
        /// Why the iterator walks no slice directly.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum NotASlice {
            /// The iterator is not `slice::Iter` made by an `iter()` call.
            OtherIterator,
        }
    }
}

/// The collection a slice iterator was made from by `iter()`.
///
/// # Specification
/// - ensures: `Present` with the receiver of `iter()` when `iterator` is that
///   call and its type is the standard slice iterator; `Absent` otherwise.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures refuse `v.iter().position(..)` on a vector
///   and accept a search over a set's iterator.
/// - witness: `tests::ui_cost_shape`
#[spec(ensures: |output| match output {
    Maybe::Present(collection) => matches!(iterator.kind, ExprKind::MethodCall(_, receiver, [], _) if receiver.hir_id == collection.hir_id),
    Maybe::Absent(_) => true,
})]
fn slice_iteration<'tcx>(
    cx: &LateContext<'tcx>,
    iterator: &'tcx Expr<'tcx>,
) -> Maybe<&'tcx Expr<'tcx>, iteration_reading::NotASlice>
{
    let ExprKind::MethodCall(segment, collection, &[], _) = iterator.kind
    else {
        return Maybe::Absent(iteration_reading::NotASlice::OtherIterator);
    };
    let is_slice_iterator = cx
        .typeck_results()
        .expr_ty(iterator)
        .ty_adt_def()
        .is_some_and(|definition| {
            cx.tcx
                .get_diagnostic_name(definition.did())
                .is_some_and(|name| name.as_str() == "SliceIter")
        });
    if segment.ident.name == sym::iter && is_slice_iterator {
        Maybe::Present(collection)
    }
    else {
        Maybe::Absent(iteration_reading::NotASlice::OtherIterator)
    }
}

/// Whether a search closure compares the element with a key.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Keyed
{
    /// The closure's body is an `==` with the element on one side only.
    Key,
    /// The closure is something else.
    Other,
}

/// Classify a search predicate as a key comparison.
///
/// # Specification
/// - ensures: `Key` exactly when `predicate` is a closure whose body, after
///   blocks, is an `==` comparison one side of which reads a binding of the
///   closure's parameter and the other side of which reads none.
/// - panics: none.
/// - executable: none — the predicate would repeat the closure-body walk the
///   body makes.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures refuse `position(|entry| entry.name ==
///   name)` and accept `any(|span| span.contains(offset))` and a comparison of
///   two element fields.
/// - witness: `tests::ui_cost_shape`
fn keyed<'tcx>(
    cx: &LateContext<'tcx>,
    predicate: &Expr<'tcx>,
) -> Keyed
{
    let ExprKind::Closure(closure) = predicate.kind
    else {
        return Keyed::Other;
    };
    let body = cx.tcx.hir_body(closure.body);
    let ExprKind::Binary(operator, left, right) = body.value.peel_blocks().kind
    else {
        return Keyed::Other;
    };
    if operator.node != BinOpKind::Eq {
        return Keyed::Other;
    }
    let mut bindings = Vec::new();
    for parameter in body.params {
        parameter
            .pat
            .each_binding(|_, binding, _, _| bindings.push(binding));
    }
    let reads = |side: &'tcx Expr<'tcx>| {
        bindings
            .iter()
            .any(|&binding| is_local_used(cx, side, binding))
    };
    if reads(left) == reads(right) {
        Keyed::Other
    }
    else {
        Keyed::Key
    }
}
