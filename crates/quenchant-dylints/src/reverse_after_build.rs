//! A vector reversed right after it is built from a source that reads
//! backward at no cost.
//!
//! `<[T]>::reverse` swaps every element once: a second pass over what was just
//! written. When the elements came from a double-ended iterator — collected,
//! or appended by `extend` — or were copied out of a slice, or split off a
//! vector, the source can be read in the other direction as it is consumed:
//! `.rev()` before `collect` or inside `extend`, `iter().rev().cloned()` for a
//! copy, `drain(at..).rev()` for a split. That builds the reversed sequence in
//! the one pass the build already makes.
//!
//! Clippy has no lint for a reversal after a build.
//!
//! # Boundaries
//!
//! Two shapes are recognized, both read from the statements around the
//! reversal: a `let` whose initializer builds the vector, followed by its
//! `reverse()` with no use of the vector between them; and an `extend`
//! followed by the reversal of the suffix it appended, `v[start..]` or
//! `v.get_mut(start..)` where `start` was bound to `v.len()` before it.
//!
//! The receiver's type decides: an array has a length its type fixes, so its
//! reversal is a constant cost — a byte-order flip of `to_le_bytes()` is one —
//! and a `VecDeque` has no `reverse`. The source's type decides too: an
//! iterator that is not `DoubleEndedIterator` cannot be read backward, and one
//! that holds a mutable or interior-mutable borrow observes its order, so
//! reversing it would reorder its effects. A vector reversed that was built by
//! a loop of pushes — an application spine, a predecessor chain, a stack's
//! pops — has no source to reverse and is accepted; reading it backward with
//! `iter().rev()` where it is consumed is the cheaper shape there, and that
//! needs the consumer, which this lint does not follow.

use anodized::spec;
use clippy_utils::diagnostics::span_lint_and_help;
use clippy_utils::eq_expr_value;
use clippy_utils::higher;
use clippy_utils::ty::has_non_owning_mutable_access;
use clippy_utils::ty::implements_trait;
use clippy_utils::visitors::is_local_used;
use quenchant_shape::shape::Maybe;
use rustc_hir::Expr;
use rustc_hir::ExprKind;
use rustc_hir::HirId;
use rustc_hir::MatchSource;
use rustc_hir::Node;
use rustc_hir::QPath;
use rustc_hir::StmtKind;
use rustc_hir::def::Res;
use rustc_lint::LateContext;
use rustc_lint::LateLintPass;
use rustc_session::declare_lint;
use rustc_session::impl_lint_pass;
use rustc_span::Symbol;
use rustc_span::sym;

use crate::clause_cost::ClauseRegions;
use crate::cost_shape::Inspection;
use crate::cost_shape::Method;
use crate::cost_shape::Owner;
use crate::cost_shape::Trip;
use crate::cost_shape::enclosing_statement;
use crate::cost_shape::inspection;
use crate::cost_shape::length_trip;
use crate::cost_shape::std_method;

declare_lint! {
    /// ### What it does
    ///
    /// Refuses `reverse()` on a vector, or on the suffix of one, right after the statement that built it from a source that reads backward at no cost: a `let` initialized by `collect()` of a double-ended iterator, by `to_vec()`, `to_owned()` or `clone()` of a slice or vector, or by `split_off(at)`, with no use of the vector between the two; or an `extend` of a double-ended iterator or a collection followed by the reversal of the suffix it appended.
    ///
    /// ### Why is this bad?
    ///
    /// The reversal is a second pass over what the build just wrote: `n` swaps after `n` writes. The source can be read in the other direction as it is consumed, which builds the reversed sequence in the build's one pass: `.rev()` before `collect` or inside `extend`, `iter().rev().cloned().collect()` for a copy, `drain(at..).rev().collect()` for a split.
    ///
    /// ### Accepted forms
    ///
    /// The receiver's type decides: an array's length is fixed by its type, so reversing it costs a constant. The source's type decides too: an iterator that is not `DoubleEndedIterator` cannot be read backward, and one holding a mutable or interior-mutable borrow observes its own order. A vector built by a loop of pushes has no source to reverse and is accepted; read it with `iter().rev()` where it is consumed instead, which this lint does not follow.
    ///
    /// ### Clippy
    ///
    /// No Clippy lint prices a reversal after a build.
    ///
    /// ### Example
    ///
    /// ```rust,ignore
    /// let mut pending: Vec<_> = tree.children(root).collect();
    /// pending.reverse();
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// let mut pending: Vec<_> = tree.children(root).rev().collect();
    /// ```
    pub REVERSE_AFTER_BUILD,
    Deny,
    "a vector is reversed right after it is built from a source that reads backward"
}

quenchant_shape::reason_enum! {
    /// A reversal can be the only way to reach the order a vector needs.
    pub mod reversal_reading {
        /// Why the reversal is not refused.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Necessary {
            /// The call is not `reverse()` on a slice.
            NotAReversal,
            /// The reversed receiver is an array, its length fixed by its type.
            FixedLength,
            /// The reversed elements were not built by the statement before
            /// the reversal: a loop of pushes, a parameter, a field.
            NotJustBuilt,
            /// The source is an iterator that is not `DoubleEndedIterator`.
            Irreversible,
            /// The source holds a mutable or interior-mutable borrow, so
            /// reading it backward would reorder what it does.
            OrderObserved,
        }
    }
}

/// How a refused reversal's source could have been read backward.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Source
{
    /// A double-ended iterator was collected: collect its `.rev()`.
    Collected,
    /// A double-ended iterator or a collection was appended: extend with its
    /// `.rev()`.
    Extended,
    /// A slice or vector was copied: collect `iter().rev().cloned()`.
    Copied,
    /// A vector's tail was split off: collect `drain(at..).rev()`.
    SplitOff,
}

/// Reversal-after-build policy; specification clauses are left to
/// `spec_clause_traversal`.
#[repr(transparent)]
pub struct ReverseAfterBuild
{
    /// The authored `spec` attribute ranges, whose clauses are not inspected.
    regions: ClauseRegions,
}

impl ReverseAfterBuild
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

impl_lint_pass!(ReverseAfterBuild => [REVERSE_AFTER_BUILD]);

impl<'tcx> LateLintPass<'tcx> for ReverseAfterBuild
{
    /// Report each reversal of a vector just built from a reversible source.
    ///
    /// # Specification
    /// - ensures: reports every expression [`reversal`] answers `Present` for,
    ///   with the rewrite its source admits as help.
    /// - ensures: expressions from macro expansions and inside `spec`
    ///   attributes are not inspected ([`inspection`]).
    /// - panics: none.
    /// - executable: none — rustc emits diagnostics without a queryable
    ///   per-call diagnostic result.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the fixtures refuse each source form and accept each
    ///   exception beside it.
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
        let Maybe::Present(source) = reversal(cx, expr)
        else {
            return;
        };
        let help = match source {
            Source::Collected => "collect the iterator's `.rev()` instead",
            Source::Extended => "extend with the source's `.rev()` instead",
            Source::Copied => "collect `iter().rev().cloned()` from the source instead",
            Source::SplitOff => "collect `drain(at..).rev()` from the source instead",
        };
        span_lint_and_help(
            cx,
            REVERSE_AFTER_BUILD,
            expr.span,
            "this reverses what the statement before it just built from a source that reads \
             backward",
            None,
            help,
        );
    }
}

/// Decide whether an expression reverses a vector just built from a
/// reversible source.
///
/// # Specification
/// - ensures: `Present` with the source's kind when `expr` is `reverse()` on a
///   slice whose receiver is not an array, and either the receiver is a local
///   vector whose `let` initializer builds it ([`built`]) with no use between
///   the `let` and the reversal's statement, or the receiver is a suffix
///   `base[start..]` or `base.get_mut(start..)` of a vector extended by the
///   statement just before, `start` being bound to `base.len()`.
/// - ensures: `Absent(NotAReversal)` for every other expression;
///   `Absent(FixedLength)` for an array; `Absent(NotJustBuilt)` when no such
///   building statement precedes it; `Absent(Irreversible)` or
///   `Absent(OrderObserved)` from the source otherwise.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures refuse a collected, a copied, a split and an
///   extended build, each reversed, and accept an array, a pushed spine, a
///   parameter, a use between build and reversal, a single-ended iterator and
///   an iterator that mutates what it captures.
/// - witness: `tests::ui_cost_shape`
#[spec(ensures: |output| match output {
    Maybe::Present(_) => matches!(expr.kind, ExprKind::MethodCall(_, receiver, [], _) if matches!(length_trip(cx.typeck_results().expr_ty(receiver)), Trip::Open)),
    Maybe::Absent(_) => true,
})]
pub fn reversal<'tcx>(
    cx: &LateContext<'tcx>,
    expr: &'tcx Expr<'tcx>,
) -> Maybe<Source, reversal_reading::Necessary>
{
    let ExprKind::MethodCall(_, receiver, &[], _) = expr.kind
    else {
        return Maybe::Absent(reversal_reading::Necessary::NotAReversal);
    };
    let Maybe::Present(Method {
        owner: Owner::Slice,
        name,
    }) = std_method(cx, expr)
    else {
        return Maybe::Absent(reversal_reading::Necessary::NotAReversal);
    };
    if name.as_str() != "reverse" {
        return Maybe::Absent(reversal_reading::Necessary::NotAReversal);
    }
    if matches!(
        length_trip(cx.typeck_results().expr_ty(receiver)),
        Trip::Fixed
    ) {
        return Maybe::Absent(reversal_reading::Necessary::FixedLength);
    }
    if let Maybe::Present(local) = local_vector(cx, receiver) {
        return whole(cx, expr, local);
    }
    suffix(cx, expr, receiver)
}

quenchant_shape::reason_enum! {
    /// A receiver can be something other than a local vector.
    mod receiver_reading {
        /// Why the receiver names no local vector.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum NotLocal {
            /// The receiver, after borrows, is not a path to a local of type
            /// `Vec`.
            OtherPlace,
        }
    }
}

/// The local vector a receiver names, through borrows.
///
/// # Specification
/// - ensures: `Present` with the local's binding when `receiver`, after
///   borrows, is a path to a local whose type is `Vec`; `Absent` otherwise.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures reverse a local vector, a borrowed one and a
///   parameter slice.
/// - witness: `tests::ui_cost_shape`
#[spec(ensures: |output| match output {
    Maybe::Present(local) => cx.tcx.hir_node(local).as_owner().is_none(),
    Maybe::Absent(_) => true,
})]
fn local_vector(
    cx: &LateContext<'_>,
    receiver: &Expr<'_>,
) -> Maybe<HirId, receiver_reading::NotLocal>
{
    let mut place = receiver;
    while let ExprKind::AddrOf(_, _, inner) = place.kind {
        place = inner;
    }
    let is_vector = cx
        .typeck_results()
        .expr_ty(place)
        .ty_adt_def()
        .is_some_and(|definition| cx.tcx.is_diagnostic_item(sym::Vec, definition.did()));
    if let ExprKind::Path(QPath::Resolved(None, path)) = place.kind
        && let Res::Local(local) = path.res
        && is_vector
    {
        Maybe::Present(local)
    }
    else {
        Maybe::Absent(receiver_reading::NotLocal::OtherPlace)
    }
}

/// Decide a whole-vector reversal from the `let` that built the vector.
///
/// # Specification
/// - requires: `expr` is `reverse()` on the local vector `local`.
/// - ensures: `Present` with the source kind when `expr` is a statement of the
///   block that holds `local`'s `let`, the `let` has an initializer [`built`]
///   reads as a reversible source, and no statement between the two uses
///   `local`.
/// - ensures: `Absent(NotJustBuilt)` when the reversal is not such a statement,
///   the `let` sits elsewhere or has no initializer, a statement between uses
///   `local`, or the initializer builds from no recognized source; the source's
///   own reason otherwise.
/// - panics: none.
/// - executable: none — the predicate would repeat the block walk the body
///   makes.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures refuse each initializer form reversed by the
///   next statement, and accept a push between them and a vector built by a
///   loop.
/// - witness: `tests::ui_cost_shape`
fn whole<'tcx>(
    cx: &LateContext<'tcx>,
    expr: &'tcx Expr<'tcx>,
    local: HirId,
) -> Maybe<Source, reversal_reading::Necessary>
{
    let not_built = Maybe::Absent(reversal_reading::Necessary::NotJustBuilt);
    let Maybe::Present(reversal_at) = enclosing_statement(cx, expr.hir_id)
    else {
        return not_built;
    };
    let Node::LetStmt(binding) = cx.tcx.parent_hir_node(local)
    else {
        return not_built;
    };
    let Some(initializer) = binding.init
    else {
        return not_built;
    };
    let Maybe::Present(binding_at) = enclosing_statement(cx, binding.hir_id)
    else {
        return not_built;
    };
    if binding_at.block.hir_id != reversal_at.block.hir_id || binding_at.index >= reversal_at.index
    {
        return not_built;
    }
    let between = reversal_at
        .block
        .stmts
        .get(binding_at.index.saturating_add(1) .. reversal_at.index)
        .unwrap_or_default();
    if between
        .iter()
        .any(|statement| is_local_used(cx, statement, local))
    {
        return not_built;
    }
    built(cx, initializer)
}

/// Decide a suffix reversal from the `extend` that appended the suffix.
///
/// # Specification
/// - requires: `expr` is `reverse()` on `receiver`.
/// - ensures: `Present(Extended)` when `receiver` is a suffix of a vector
///   `base` from a `start` bound to `base.len()`, read through `base[start..]`
///   or `base.get_mut(start..)` directly or through a binding, and the
///   statement before the one holding the suffix is `base.extend(source)` with
///   a reversible `source`.
/// - ensures: `Absent(NotJustBuilt)` when any of those is missing; the source's
///   own reason otherwise.
/// - panics: none.
/// - executable: none — the predicate would repeat the statement walk the body
///   makes.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures refuse an indexed and a `get_mut` suffix
///   after an `extend` of a mapped slice iterator, and accept a suffix after a
///   callback that pushes and after an `extend` of a single-ended iterator.
/// - witness: `tests::ui_cost_shape`
fn suffix<'tcx>(
    cx: &LateContext<'tcx>,
    expr: &'tcx Expr<'tcx>,
    receiver: &'tcx Expr<'tcx>,
) -> Maybe<Source, reversal_reading::Necessary>
{
    let not_built = Maybe::Absent(reversal_reading::Necessary::NotJustBuilt);
    let (selection, anchor) = match bound_selection(cx, receiver) {
        Maybe::Present(found) => found,
        Maybe::Absent(_) => (receiver, expr.hir_id),
    };
    let Maybe::Present((base, start)) = range_suffix(cx, selection)
    else {
        return not_built;
    };
    let Maybe::Present(anchor_at) = enclosing_statement(cx, anchor)
    else {
        return not_built;
    };
    let Some(previous) = anchor_at
        .index
        .checked_sub(1)
        .and_then(|index| anchor_at.block.stmts.get(index))
    else {
        return not_built;
    };
    let (StmtKind::Semi(extension) | StmtKind::Expr(extension)) = previous.kind
    else {
        return not_built;
    };
    let ExprKind::MethodCall(segment, extended, &[ref appended], _) = extension.kind
    else {
        return not_built;
    };
    let context = expr.span.ctxt();
    if segment.ident.name.as_str() != "extend"
        || !eq_expr_value(cx, context, extended, base)
        || !matches!(length_of(cx, start, base), LengthOf::Base)
    {
        return not_built;
    }
    match reversible(cx, appended) {
        Maybe::Present(()) => Maybe::Present(Source::Extended),
        Maybe::Absent(reason) => Maybe::Absent(reason),
    }
}

quenchant_shape::reason_enum! {
    /// A receiver can be other than a binding of a selected suffix.
    mod selection_reading {
        /// Why the receiver names no bound selection.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Unbound {
            /// The receiver is not a local bound by a `let` or an `if let`.
            NotBound,
        }
    }
}

/// The selection a local receiver was bound to, and the expression whose
/// statement binds it.
///
/// # Specification
/// - ensures: `Present` with the initializer and the binding statement or `if
///   let` when `receiver` is a path to a local bound, possibly inside a
///   pattern, by a `let` statement or an `if let`; `Absent` otherwise.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures reverse a suffix bound by `if let Some(..)`
///   and by a `let` with `?`.
/// - witness: `tests::ui_cost_shape`
#[spec(ensures: |output| match output {
    Maybe::Present((selection, _)) => !matches!(selection.kind, ExprKind::Path(_)) || selection.hir_id != receiver.hir_id,
    Maybe::Absent(_) => true,
})]
fn bound_selection<'tcx>(
    cx: &LateContext<'tcx>,
    receiver: &Expr<'_>,
) -> Maybe<(&'tcx Expr<'tcx>, HirId), selection_reading::Unbound>
{
    let ExprKind::Path(QPath::Resolved(None, path)) = receiver.kind
    else {
        return Maybe::Absent(selection_reading::Unbound::NotBound);
    };
    let Res::Local(local) = path.res
    else {
        return Maybe::Absent(selection_reading::Unbound::NotBound);
    };
    for (parent_id, node) in cx.tcx.hir_parent_iter(local) {
        #[expect(
            clippy::wildcard_enum_match_arm,
            reason = "`Node` and `ExprKind` are rustc's, their variants moving with the pinned \
                      nightly; only a pattern leads to the `let` or `if let` that binds it"
        )]
        match node {
            Node::Pat(_) | Node::PatField(_) => {},
            Node::LetStmt(binding) => {
                return binding.init.map_or(
                    Maybe::Absent(selection_reading::Unbound::NotBound),
                    |init| Maybe::Present((init, binding.hir_id)),
                );
            },
            Node::Expr(&Expr {
                kind: ExprKind::Let(binding),
                ..
            }) => {
                let anchor = cx
                    .tcx
                    .hir_parent_iter(parent_id)
                    .find_map(|(id, node)| match node {
                        Node::Expr(&Expr {
                            kind: ExprKind::If(..),
                            ..
                        }) => Some(id),
                        _ => None,
                    })
                    .unwrap_or(parent_id);
                return Maybe::Present((binding.init, anchor));
            },
            _ => return Maybe::Absent(selection_reading::Unbound::NotBound),
        }
    }
    Maybe::Absent(selection_reading::Unbound::NotBound)
}

quenchant_shape::reason_enum! {
    /// An expression can select something other than a vector's suffix.
    mod suffix_reading {
        /// Why the expression selects no suffix.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum NotASuffix {
            /// The expression is not `base[start..]` or `base.get_mut(start..)`
            /// under borrows, `?`, `ok_or`, `ok_or_else`, `expect` or
            /// `unwrap`.
            OtherSelection,
        }
    }
}

/// The vector and start of a suffix selection.
///
/// # Specification
/// - ensures: `Present` with `base` and `start` when `selection`, through
///   borrows, `?`, `ok_or`, `ok_or_else`, `expect` and `unwrap`, is
///   `base[start..]` or `base.get_mut(start..)`; `Absent` otherwise.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures select suffixes by indexing, by `get_mut`
///   under `if let` and by `get_mut` under `ok_or` and `?`.
/// - witness: `tests::ui_cost_shape`
#[spec(ensures: |output| match output {
    Maybe::Present((base, _)) => base.hir_id != selection.hir_id,
    Maybe::Absent(_) => true,
})]
fn range_suffix<'tcx>(
    cx: &LateContext<'tcx>,
    selection: &'tcx Expr<'tcx>,
) -> Maybe<(&'tcx Expr<'tcx>, &'tcx Expr<'tcx>), suffix_reading::NotASuffix>
{
    let from = |range: &'tcx Expr<'tcx>| {
        let range = higher::Range::hir(cx, range)?;
        match (range.start, range.end) {
            (Some(start), None) => Some(start),
            _ => None,
        }
    };
    let mut current = selection;
    loop {
        #[expect(
            clippy::wildcard_enum_match_arm,
            reason = "`ExprKind` is rustc's, its variants moving with the pinned nightly; only an \
                      index, a borrow, a method call or a `?` leads to a suffix"
        )]
        match current.kind {
            ExprKind::Index(base, range, _) => {
                return from(range).map_or(
                    Maybe::Absent(suffix_reading::NotASuffix::OtherSelection),
                    |start| Maybe::Present((base, start)),
                );
            },
            ExprKind::AddrOf(_, _, inner) => current = inner,
            ExprKind::MethodCall(segment, base, &[ref range], _)
                if segment.ident.name.as_str() == "get_mut" =>
            {
                return from(range).map_or(
                    Maybe::Absent(suffix_reading::NotASuffix::OtherSelection),
                    |start| Maybe::Present((base, start)),
                );
            },
            ExprKind::MethodCall(segment, inner, ..)
                if matches!(
                    segment.ident.name.as_str(),
                    "ok_or" | "ok_or_else" | "expect" | "unwrap"
                ) =>
            {
                current = inner;
            },
            ExprKind::Match(scrutinee, _, MatchSource::TryDesugar(_)) => {
                let ExprKind::Call(_, &[ref inner]) = scrutinee.kind
                else {
                    return Maybe::Absent(suffix_reading::NotASuffix::OtherSelection);
                };
                current = inner;
            },
            _ => return Maybe::Absent(suffix_reading::NotASuffix::OtherSelection),
        }
    }
}

/// Whether a start index is bound to its vector's length.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LengthOf
{
    /// The start is a local bound by a `let` to `base.len()`.
    Base,
    /// The start is something else.
    Other,
}

/// Classify a suffix's start index.
///
/// # Specification
/// - ensures: `Base` exactly when `start` is a path to a local whose `let`
///   initializer is `len()` called on an expression equal to `base`.
/// - panics: none.
/// - executable: none — the predicate would repeat the binding lookup the body
///   makes.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures refuse a suffix from `base.len()` and accept
///   one from a different index.
/// - witness: `tests::ui_cost_shape`
fn length_of(
    cx: &LateContext<'_>,
    start: &Expr<'_>,
    base: &Expr<'_>,
) -> LengthOf
{
    if let ExprKind::Path(QPath::Resolved(None, path)) = start.kind
        && let Res::Local(local) = path.res
        && let Node::LetStmt(binding) = cx.tcx.parent_hir_node(local)
        && let Some(initializer) = binding.init
        && let ExprKind::MethodCall(segment, measured, &[], _) = initializer.kind
        && segment.ident.name.as_str() == "len"
        && eq_expr_value(cx, start.span.ctxt(), measured, base)
    {
        LengthOf::Base
    }
    else {
        LengthOf::Other
    }
}

/// Classify the initializer that built a vector.
///
/// # Specification
/// - ensures: `Present(Collected)` for `collect()` of a reversible iterator;
///   `Present(Copied)` for `to_vec()`, `to_owned()` or `clone()` of a slice or
///   vector; `Present(SplitOff)` for `split_off` on a vector.
/// - ensures: the iterator's own reason for a `collect()` of an irreversible
///   one; `Absent(NotJustBuilt)` for every other initializer.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures refuse each initializer form and accept a
///   vector from `Vec::new()` filled by pushes.
/// - witness: `tests::ui_cost_shape`
#[spec(ensures: |output| !matches!(output, Maybe::Present(Source::Extended)))]
fn built<'tcx>(
    cx: &LateContext<'tcx>,
    initializer: &'tcx Expr<'tcx>,
) -> Maybe<Source, reversal_reading::Necessary>
{
    let ExprKind::MethodCall(segment, source, ..) = initializer.kind
    else {
        return Maybe::Absent(reversal_reading::Necessary::NotJustBuilt);
    };
    let name = segment.ident.name;
    let source_held = cx.typeck_results().expr_ty_adjusted(source).peel_refs();
    let source_is_sequence = source_held.is_slice()
        || source_held
            .ty_adt_def()
            .is_some_and(|definition| cx.tcx.is_diagnostic_item(sym::Vec, definition.did()));
    match (std_method(cx, initializer), name.as_str()) {
        (
            Maybe::Present(Method {
                owner: Owner::Iterator,
                ..
            }),
            "collect",
        ) => match reversible(cx, source) {
            Maybe::Present(()) => Maybe::Present(Source::Collected),
            Maybe::Absent(reason) => Maybe::Absent(reason),
        },
        (
            Maybe::Present(Method {
                owner: Owner::Vec, ..
            }),
            "split_off",
        ) => Maybe::Present(Source::SplitOff),
        (_, "to_vec" | "to_owned" | "clone") if source_is_sequence => {
            Maybe::Present(Source::Copied)
        },
        _ => Maybe::Absent(reversal_reading::Necessary::NotJustBuilt),
    }
}

/// Whether a source can be read backward without reordering what it does.
///
/// # Specification
/// - ensures: for an iterator, `Present` exactly when it implements
///   `DoubleEndedIterator` and holds no mutable or interior-mutable borrow;
///   `Absent(Irreversible)` or `Absent(OrderObserved)` otherwise.
/// - ensures: for a slice, an array or a vector, by value or reference,
///   `Present`; `Absent(Irreversible)` for anything else.
/// - panics: none.
/// - executable: none — the predicate would repeat the two trait queries the
///   body makes.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures refuse a mapped slice iterator and a
///   borrowed vector, and accept a `successors` chain and a `map` whose closure
///   pushes into a captured vector.
/// - witness: `tests::ui_cost_shape`
fn reversible(
    cx: &LateContext<'_>,
    source: &Expr<'_>,
) -> Maybe<(), reversal_reading::Necessary>
{
    let held = cx.typeck_results().expr_ty(source);
    let iterator_trait = cx.tcx.get_diagnostic_item(sym::Iterator);
    if iterator_trait.is_some_and(|trait_id| implements_trait(cx, held, trait_id, &[])) {
        let double_ended = cx
            .tcx
            .get_diagnostic_item(Symbol::intern("DoubleEndedIterator"))
            .is_some_and(|trait_id| implements_trait(cx, held, trait_id, &[]));
        if !double_ended {
            return Maybe::Absent(reversal_reading::Necessary::Irreversible);
        }
        if has_non_owning_mutable_access(cx, held) {
            return Maybe::Absent(reversal_reading::Necessary::OrderObserved);
        }
        return Maybe::Present(());
    }
    let peeled = held.peel_refs();
    let is_sequence = peeled.is_slice()
        || peeled.is_array()
        || peeled
            .ty_adt_def()
            .is_some_and(|definition| cx.tcx.is_diagnostic_item(sym::Vec, definition.did()));
    if is_sequence {
        Maybe::Present(())
    }
    else {
        Maybe::Absent(reversal_reading::Necessary::Irreversible)
    }
}
