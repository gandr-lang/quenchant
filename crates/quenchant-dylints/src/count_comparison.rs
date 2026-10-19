//! An iterator counted to the end only to compare the count with a constant.
//!
//! `Iterator::count` consumes every element. Compared with a constant `k` —
//! `chars().count() > 24`, `lines().count() == 1`, `filter(..).count() == 0`
//! — the answer is decided by the first `k + 1` elements: `take(k + 1).count()`
//! reads at most that many, and `next().is_none()` or `nth(k).is_some()`
//! read the fewest. Over an `n`-element source the full count costs `O(n)`
//! where the bounded one costs `O(min(n, k))`.
//!
//! The receiver's type decides: an `ExactSizeIterator` knows its length, and
//! its `len()` is the cheaper shape — a different rewrite this lint leaves to
//! Clippy's `iter_count` where the iterator comes straight from a
//! collection. A count already bounded by a `take` with a constant limit
//! costs a constant. Only a comparison operator with an operand that
//! evaluates to an integer constant is a bound; a count compared with another
//! runtime value, or used as a number, is accepted.
//!
//! Clippy's `iter_count` rewrites `.iter().count()` on a collection as
//! `.len()` and `bytes_count_to_len` rewrites `.bytes().count()`; neither
//! bounds a count compared with a constant.

use anodized::spec;
use clippy_utils::consts::ConstEvalCtxt;
use clippy_utils::consts::Constant;
use clippy_utils::diagnostics::span_lint_and_help;
use clippy_utils::paths::PathNS;
use clippy_utils::paths::lookup_path_str;
use clippy_utils::ty::implements_trait;
use quenchant_shape::shape::Maybe;
use rustc_hir::BinOpKind;
use rustc_hir::Expr;
use rustc_hir::ExprKind;
use rustc_hir::Node;
use rustc_lint::LateContext;
use rustc_lint::LateLintPass;
use rustc_session::declare_lint;
use rustc_session::impl_lint_pass;

use crate::clause_cost::ClauseRegions;
use crate::cost_shape::Inspection;
use crate::cost_shape::Method;
use crate::cost_shape::Owner;
use crate::cost_shape::inspection;
use crate::cost_shape::std_method;

declare_lint! {
    /// ### What it does
    ///
    /// Refuses `Iterator::count()` compared by `==`, `!=`, `<`, `<=`, `>` or `>=` with an operand that evaluates to an integer constant, when the counted iterator is neither an `ExactSizeIterator` nor a `take` with a constant limit.
    ///
    /// ### Why is this bad?
    ///
    /// `count()` consumes every element, but a comparison with a constant `k` is decided by the first `k + 1`. Over an `n`-element source the full count costs `O(n)` where `take(k + 1).count()` costs `O(min(n, k))`; `next().is_none()` and `nth(k).is_some()` read the fewest.
    ///
    /// ### Accepted forms
    ///
    /// The receiver's type decides: an `ExactSizeIterator` knows its length, and its `len()` is the cheaper shape. A `take` with a constant limit already bounds the walk. A count compared with a runtime value, or used as a number, is accepted.
    ///
    /// ### Clippy
    ///
    /// `iter_count` rewrites `.iter().count()` on a collection as `.len()`, and `bytes_count_to_len` rewrites `.bytes().count()`; neither bounds a count compared with a constant.
    ///
    /// ### Example
    ///
    /// ```rust,ignore
    /// if label.chars().count() > 24 {
    ///     shorten(label);
    /// }
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// if label.chars().nth(24).is_some() {
    ///     shorten(label);
    /// }
    /// ```
    pub COUNT_AGAINST_CONSTANT,
    Deny,
    "an iterator is counted to its end only to compare the count with a constant"
}

quenchant_shape::reason_enum! {
    /// A count can be the cheapest way to answer its comparison.
    pub mod count_reading {
        /// Why the count is not refused.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Needed {
            /// The call is not `Iterator::count`.
            NotACount,
            /// The count is not compared with an integer constant.
            Unbounded,
            /// The iterator is an `ExactSizeIterator`.
            KnownLength,
            /// The iterator is a `take` with a constant limit, so its walk is
            /// already bounded.
            AlreadyBounded,
        }
    }
}

/// The integer constant a count is compared with.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct Bound(u128);

/// Bounded-count policy; specification clauses are left to
/// `spec_clause_traversal`.
#[repr(transparent)]
pub struct CountAgainstConstant
{
    /// The authored `spec` attribute ranges, whose clauses are not inspected.
    regions: ClauseRegions,
}

impl CountAgainstConstant
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

impl_lint_pass!(CountAgainstConstant => [COUNT_AGAINST_CONSTANT]);

impl<'tcx> LateLintPass<'tcx> for CountAgainstConstant
{
    /// Report each full count compared with a constant.
    ///
    /// # Specification
    /// - ensures: reports every expression [`bounded_count`] answers `Present`
    ///   for, with the bound the comparison needs as help.
    /// - ensures: expressions from macro expansions and inside `spec`
    ///   attributes are not inspected ([`inspection`]).
    /// - panics: none.
    /// - executable: none — rustc emits diagnostics without a queryable
    ///   per-call diagnostic result.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the fixtures refuse a count on each side of each
    ///   comparison operator and accept an exact-size count and a runtime
    ///   bound.
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
        let Maybe::Present(Bound(bound)) = bounded_count(cx, expr)
        else {
            return;
        };
        let help = bound.checked_add(1).map_or_else(
            || "test `nth(k)` for the constant `k` instead".to_owned(),
            |past| {
                format!(
                    "bound the walk: compare `take({past}).count()`, or test `next()` or `nth(k)`"
                )
            },
        );
        span_lint_and_help(
            cx,
            COUNT_AGAINST_CONSTANT,
            expr.span,
            "this counts every element to compare the count with a constant",
            None,
            help,
        );
    }
}

/// Decide whether an expression is a full count compared with a constant.
///
/// # Specification
/// - ensures: `Present` with the constant when `expr` is `Iterator::count()` on
///   an iterator that is neither an `ExactSizeIterator` nor a `take` with a
///   constant limit, and is an operand of a comparison whose other operand
///   evaluates to an integer constant.
/// - ensures: `Absent(NotACount)` for every other expression;
///   `Absent(Unbounded)` when the count is not so compared;
///   `Absent(KnownLength)` for an exact-size iterator; `Absent(AlreadyBounded)`
///   for a `take` with a constant limit.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures refuse `chars().count() > 24`, `0 ==
///   filter(..).count()` and a count against a named constant, and accept a
///   count against a runtime length, over a mapped slice iterator and over a
///   `take(2)`.
/// - witness: `tests::ui_cost_shape`
#[spec(ensures: |output| match output {
    Maybe::Present(_) => matches!(expr.kind, ExprKind::MethodCall(..)),
    Maybe::Absent(_) => true,
})]
pub fn bounded_count<'tcx>(
    cx: &LateContext<'tcx>,
    expr: &'tcx Expr<'tcx>,
) -> Maybe<Bound, count_reading::Needed>
{
    let ExprKind::MethodCall(_, iterator, &[], _) = expr.kind
    else {
        return Maybe::Absent(count_reading::Needed::NotACount);
    };
    let Maybe::Present(Method {
        owner: Owner::Iterator,
        name,
    }) = std_method(cx, expr)
    else {
        return Maybe::Absent(count_reading::Needed::NotACount);
    };
    if name.as_str() != "count" {
        return Maybe::Absent(count_reading::Needed::NotACount);
    }
    let Node::Expr(comparison) = cx.tcx.parent_hir_node(expr.hir_id)
    else {
        return Maybe::Absent(count_reading::Needed::Unbounded);
    };
    let ExprKind::Binary(operator, left, right) = comparison.kind
    else {
        return Maybe::Absent(count_reading::Needed::Unbounded);
    };
    if !matches!(
        operator.node,
        BinOpKind::Eq
            | BinOpKind::Ne
            | BinOpKind::Lt
            | BinOpKind::Le
            | BinOpKind::Gt
            | BinOpKind::Ge
    ) {
        return Maybe::Absent(count_reading::Needed::Unbounded);
    }
    let other = if left.hir_id == expr.hir_id {
        right
    }
    else {
        left
    };
    let Some(Constant::Int(bound)) = ConstEvalCtxt::new(cx).eval(other)
    else {
        return Maybe::Absent(count_reading::Needed::Unbounded);
    };
    let held = cx.typeck_results().expr_ty(iterator);
    let exact = lookup_path_str(cx.tcx, PathNS::Type, "core::iter::ExactSizeIterator")
        .into_iter()
        .any(|trait_id| implements_trait(cx, held, trait_id, &[]));
    if exact {
        return Maybe::Absent(count_reading::Needed::KnownLength);
    }
    if let ExprKind::MethodCall(_, _, &[ref limit], _) = iterator.kind
        && let Maybe::Present(Method {
            owner: Owner::Iterator,
            name: adapter,
        }) = std_method(cx, iterator)
        && adapter.as_str() == "take"
        && let Some(Constant::Int(_)) = ConstEvalCtxt::new(cx).eval(limit)
    {
        return Maybe::Absent(count_reading::Needed::AlreadyBounded);
    }
    Maybe::Present(Bound(bound))
}
