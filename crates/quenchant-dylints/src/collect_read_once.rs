//! A vector collected from an iterator only to be read once and dropped.
//!
//! `collect()` into a `Vec` or `VecDeque` allocates the whole sequence and
//! writes every element; a single read afterwards — iterating it once, taking
//! a copy of its `first()`, `last()` or `get(i)` — could have read the
//! iterator itself. The collection costs an allocation of `n` elements and a
//! pass over them that the one read repeats; iterating, `next()`, `last()` and
//! `nth(i)` on the source cost the read alone, and `next()` and `nth(i)` stop
//! early.
//!
//! Clippy's `needless_collect` refuses a collection read by `len()`,
//! `is_empty()`, `contains()`, an index, or, through a `let`, `into_iter()`,
//! and one passed to a parameter that takes `IntoIterator`; this lint takes the
//! reads it leaves: a `for` loop over the collection, or over a borrow of it
//! with a copying pattern, a direct `into_iter()`, and `iter()`, `first()`,
//! `last()` or `get(i)` copied or cloned at once.
//!
//! # Boundaries
//!
//! The collection's type decides: only a `Vec` or a `VecDeque` is a plain
//! buffer whose single read the source answers; a set or a map collected and
//! read once deduplicates or orders, and a `String` joins. The read decides
//! too: a collection read twice, kept, returned, stored, or passed on is
//! accepted, and so is a read that borrows the elements rather than moving or
//! copying them out, because a borrow of a collected element can be stored
//! where a borrow of an element the source yields cannot.
//!
//! Collecting is also how effects are ordered and borrows end, and each such
//! collection is accepted: an iterator that holds a mutable or
//! interior-mutable borrow, or whose closures call something with a value
//! they capture or call an `unsafe` function (a thread spawned per element
//! and joined after, blocks allocated before any is freed), runs those effects
//! at the collection; an iterator that borrows a temporary — a lock guard, a
//! `RefCell` borrow, an owned value computed in place — or a local that the
//! code before the read uses or the read mutates, needs the collection for
//! the read to be sound; and a reversed read needs a source that is
//! double-ended and exact-size.

use core::ops::ControlFlow;

use anodized::spec;
use clippy_utils::diagnostics::span_lint_and_then;
use clippy_utils::higher::ForLoop;
use clippy_utils::res::MaybeResPath as _;
use clippy_utils::ty::has_non_owning_mutable_access;
use clippy_utils::ty::implements_trait;
use clippy_utils::usage::mutated_variables;
use clippy_utils::visitors::for_each_expr;
use quenchant_shape::shape::Maybe;
use rustc_hir::BindingMode;
use rustc_hir::Block;
use rustc_hir::Expr;
use rustc_hir::ExprKind;
use rustc_hir::HirId;
use rustc_hir::HirIdSet;
use rustc_hir::MatchSource;
use rustc_hir::Node;
use rustc_hir::PatKind;
use rustc_hir::StmtKind;
use rustc_lint::LateContext;
use rustc_lint::LateLintPass;
use rustc_middle::ty::adjustment::Adjust;
use rustc_middle::ty::adjustment::DerefAdjustKind;
use rustc_session::declare_lint;
use rustc_session::impl_lint_pass;
use rustc_span::Span;
use rustc_span::Symbol;

use crate::clause_cost::ClauseRegions;
use crate::cost_shape::Inspection;
use crate::cost_shape::Method;
use crate::cost_shape::Owner;
use crate::cost_shape::Placement;
use crate::cost_shape::enclosing_statement;
use crate::cost_shape::inspection;
use crate::cost_shape::std_method;

declare_lint! {
    /// ### What it does
    ///
    /// Refuses `collect()` into a `Vec` or `VecDeque` whose result is read exactly once by a moving or copying read — a `for` loop over it, or over a borrow of it with a copying pattern, a direct `into_iter()`, or `iter()`, `first()`, `last()` or `get(i)` followed by `copied()` or `cloned()` — either directly or through a `let` binding read once in the same block.
    ///
    /// ### Why is this bad?
    ///
    /// The collection allocates `n` elements and writes each, and the single read repeats the pass. Reading the iterator itself costs the read alone: iterate it, or call `next()`, `last()` or `nth(i)`, of which `next()` and `nth(i)` stop early.
    ///
    /// ### Accepted forms
    ///
    /// The collection's type decides: sets, maps and strings are collected for what the collection does. A collection read twice, inside a loop or closure that does not hold its binding, kept, returned, stored or passed on is accepted, and so is a read that borrows the elements. So is a collection that orders effects or ends a borrow: one whose iterator holds a mutable or interior-mutable borrow, whose closures call something with a captured value or call an `unsafe` function, that borrows a temporary, or that borrows a local which the code before the read uses or the read mutates. A reversed read of a source that is not both `DoubleEndedIterator` and `ExactSizeIterator` is accepted.
    ///
    /// ### Clippy
    ///
    /// `needless_collect` refuses a collection read by `len()`, `is_empty()`, `contains()`, an index, or through a `let` by `into_iter()`, and one passed to an `IntoIterator` parameter. This lint takes the reads it leaves.
    ///
    /// ### Example
    ///
    /// ```rust,ignore
    /// let names: Vec<_> = entries.iter().map(|entry| entry.id()).collect();
    /// for &name in &names {
    ///     emit(name);
    /// }
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// for name in entries.iter().map(|entry| entry.id()) {
    ///     emit(name);
    /// }
    /// ```
    pub COLLECT_READ_ONCE,
    Deny,
    "a vector is collected from an iterator only to be read once and dropped"
}

quenchant_shape::reason_enum! {
    /// A collection can be needed for more than one read of its source.
    pub mod collection_reading {
        /// Why the collection is not refused.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Needed {
            /// The call is not `collect()` into a `Vec` or `VecDeque`.
            NotACollect,
            /// The collection is kept, returned, stored, passed on or read by
            /// a method other than the single reads this lint prices.
            Kept,
            /// The binding is read more than once, or inside a loop or closure
            /// that does not hold the binding.
            ReadAgain,
            /// Clippy's `needless_collect` refuses this read.
            ClippyCovers,
            /// The iterator holds a mutable or interior-mutable borrow, so its
            /// effects run at the collection.
            OrderObserved,
            /// A closure in the iterator calls something on a value it
            /// captures, or calls an `unsafe` function: collecting runs those
            /// effects together before the read, as spawning every thread
            /// before joining any does.
            Effects,
            /// The read borrows the elements rather than moving or copying
            /// them out, and a borrow of a collected element can outlive the
            /// read where a borrow of a source element could not.
            BorrowingRead,
            /// The iterator borrows a temporary that the collection outlives.
            BorrowsTemporary,
            /// The iterator reads a local that the code between the
            /// collection and the read uses, or that the read mutates.
            BorrowEnded,
            /// The read reverses the collection and the iterator is not both
            /// `DoubleEndedIterator` and `ExactSizeIterator`.
            Irreversible,
        }
    }
}

/// How a collection read once could have read its source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Read
{
    /// Iterated once: iterate the source itself.
    Iterated,
    /// Its first element: `next()` on the source.
    First,
    /// Its last element: `last()`, or `next_back()` when double-ended.
    Last,
    /// One indexed element: `nth(i)` on the source.
    Indexed,
}

/// A refused collection: its read and where the read happens.
#[derive(Clone, Copy, Debug)]
pub struct ReadOnce
{
    /// How the collection is read.
    pub read: Read,
    /// The read's own span.
    pub at: Span,
}

/// Single-read-collection policy; specification clauses are left to
/// `spec_clause_traversal`.
#[repr(transparent)]
pub struct CollectReadOnce
{
    /// The authored `spec` attribute ranges, whose clauses are not inspected.
    regions: ClauseRegions,
}

impl CollectReadOnce
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

impl_lint_pass!(CollectReadOnce => [COLLECT_READ_ONCE]);

impl<'tcx> LateLintPass<'tcx> for CollectReadOnce
{
    /// Report each collection read once, at its `collect()`.
    ///
    /// # Specification
    /// - ensures: reports every expression [`read_once`] answers `Present` for,
    ///   with a note at the read and the source-side rewrite as help.
    /// - ensures: expressions from macro expansions and inside `spec`
    ///   attributes are not inspected ([`inspection`]).
    /// - panics: none.
    /// - executable: none — rustc emits diagnostics without a queryable
    ///   per-call diagnostic result.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the fixtures refuse each single read, directly and
    ///   through a `let`, and accept each exception beside it.
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
        let Maybe::Present(found) = read_once(cx, expr)
        else {
            return;
        };
        let help = match found.read {
            Read::Iterated => "iterate the source iterator itself",
            Read::First => "call `next()` on the source iterator",
            Read::Last => {
                "call `last()` on the source iterator, or `next_back()` when it is double-ended"
            },
            Read::Indexed => "call `nth(i)` on the source iterator",
        };
        let ExprKind::MethodCall(segment, ..) = expr.kind
        else {
            return;
        };
        span_lint_and_then(
            cx,
            COLLECT_READ_ONCE,
            segment.ident.span.with_hi(expr.span.hi()),
            "this collects a vector that is read once and dropped",
            |diagnostic| {
                diagnostic.span_note(found.at, "its only read");
                diagnostic.help(help);
            },
        );
    }
}

/// Decide whether a `collect()` builds a vector read once.
///
/// # Specification
/// - ensures: `Present` when `expr` is `Iterator::collect` into a `Vec` or
///   `VecDeque` whose iterator holds no mutable or interior-mutable borrow
///   ([`has_non_owning_mutable_access`]), runs no effect a closure could
///   reorder ([`effects`]) and borrows no temporary ([`temporary_borrow`]), and
///   whose result is read exactly once by a moving or copying read
///   ([`single_read`]), directly or through a plain `let` binding of the same
///   block, where no statement between the collection and the read uses a local
///   the iterator reads, the read mutates none ([`source_touched`]), and a
///   reversed read has a source that reverses ([`reversed`]).
/// - ensures: `Absent` with the first reason that holds otherwise, in the order
///   the body tests them.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures refuse a `for` loop over the collection and
///   over a borrow with a copying pattern, `iter().copied()`, a direct
///   `into_iter()`, and `first()`, `last()` and `get(i)` followed by
///   `copied()`, directly and through a `let`; they accept a set, a second
///   read, a returned vector, a borrowing read whose borrows are stored, a lock
///   guard's iterator, spawned threads joined after the collection, a source
///   dropped between collection and read, a loop that mutates the source, a
///   `map` that pushes into a captured vector, a reversed read of a
///   single-ended source and `needless_collect`'s own reads.
/// - witness: `tests::ui_cost_shape`
#[spec(ensures: |output| match output {
    Maybe::Present(_) => matches!(expr.kind, ExprKind::MethodCall(_, _, [], _)),
    Maybe::Absent(_) => true,
})]
pub fn read_once<'tcx>(
    cx: &LateContext<'tcx>,
    expr: &'tcx Expr<'tcx>,
) -> Maybe<ReadOnce, collection_reading::Needed>
{
    let ExprKind::MethodCall(_, iterator, &[], _) = expr.kind
    else {
        return Maybe::Absent(collection_reading::Needed::NotACollect);
    };
    let Maybe::Present(Method {
        owner: Owner::Iterator,
        name,
    }) = std_method(cx, expr)
    else {
        return Maybe::Absent(collection_reading::Needed::NotACollect);
    };
    let is_buffer = cx
        .typeck_results()
        .expr_ty(expr)
        .ty_adt_def()
        .and_then(|definition| cx.tcx.get_diagnostic_name(definition.did()))
        .is_some_and(|collected| matches!(collected.as_str(), "Vec" | "VecDeque"));
    if name.as_str() != "collect" || !is_buffer {
        return Maybe::Absent(collection_reading::Needed::NotACollect);
    }
    if has_non_owning_mutable_access(cx, cx.typeck_results().expr_ty(iterator)) {
        return Maybe::Absent(collection_reading::Needed::OrderObserved);
    }
    if matches!(effects(cx, iterator), Effect::Effectful) {
        return Maybe::Absent(collection_reading::Needed::Effects);
    }
    if matches!(temporary_borrow(cx, iterator), Borrowed::Temporary) {
        return Maybe::Absent(collection_reading::Needed::BorrowsTemporary);
    }
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "`Node` is rustc's, its variants moving with the pinned nightly; only a `let` or \
                  an expression reads the collection"
    )]
    let (read, reader, region) = match cx.tcx.parent_hir_node(expr.hir_id) {
        Node::LetStmt(binding) => {
            let PatKind::Binding(BindingMode::NONE | BindingMode::MUT, local, _, None) =
                binding.pat.kind
            else {
                return Maybe::Absent(collection_reading::Needed::Kept);
            };
            let Maybe::Present(placement) = enclosing_statement(cx, binding.hir_id)
            else {
                return Maybe::Absent(collection_reading::Needed::Kept);
            };
            let Maybe::Present(found) = only_use(cx, placement, local)
            else {
                return Maybe::Absent(collection_reading::Needed::ReadAgain);
            };
            let (read, reader) = match single_read(cx, found.expr, Binding::Let) {
                Maybe::Present(read) => read,
                Maybe::Absent(reason) => return Maybe::Absent(reason),
            };
            let region = Region {
                block: placement.block,
                first: placement.index.saturating_add(1),
                last: found.index,
            };
            (read, reader, region)
        },
        Node::Expr(_) => {
            let (read, reader) = match single_read(cx, expr, Binding::Direct) {
                Maybe::Present(read) => read,
                Maybe::Absent(reason) => return Maybe::Absent(reason),
            };
            let Maybe::Present(placement) = enclosing_statement(cx, expr.hir_id)
            else {
                return Maybe::Absent(collection_reading::Needed::Kept);
            };
            let region = Region {
                block: placement.block,
                first: placement.index,
                last: placement.index,
            };
            (read, reader, region)
        },
        _ => return Maybe::Absent(collection_reading::Needed::Kept),
    };
    if matches!(read, Read::Iterated)
        && matches!(reversed(cx, reader, iterator), Reversal::Impossible)
    {
        return Maybe::Absent(collection_reading::Needed::Irreversible);
    }
    if matches!(source_touched(cx, iterator, region), Touch::Touched) {
        return Maybe::Absent(collection_reading::Needed::BorrowEnded);
    }
    Maybe::Present(ReadOnce {
        read,
        at: reader.span,
    })
}

/// Where the collection's value reaches its read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Binding
{
    /// The `collect()` itself is read.
    Direct,
    /// A `let` binding of it is read.
    Let,
}

/// The statements from a collection to the end of its read: indices into a
/// block, the block's tail expression at `stmts.len()`.
#[derive(Clone, Copy, Debug)]
struct Region<'tcx>
{
    /// The block that holds both.
    block: &'tcx Block<'tcx>,
    /// The first statement's index.
    first: usize,
    /// The last statement's index, the read's own.
    last: usize,
}

/// A binding's single use and the index of the statement of the binding's
/// block that holds it.
#[derive(Clone, Copy, Debug)]
struct Use<'tcx>
{
    /// The path expression that reads the binding.
    expr: &'tcx Expr<'tcx>,
    /// The holding statement's index; the tail is at `stmts.len()`.
    index: usize,
}

quenchant_shape::reason_enum! {
    /// A binding can be read other than exactly once in its block.
    mod use_reading {
        /// Why the binding has no single use.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum NotOnce {
            /// The binding is read zero or several times, or inside a loop or
            /// closure that does not hold it.
            Repeated,
        }
    }
}

/// The single use of a `let` binding in the rest of its block.
///
/// # Specification
/// - ensures: `Present` with the path expression and the index of the block
///   statement, or tail, that holds it, when the statements after `placement`
///   and the block's tail mention `local` exactly once and no loop or closure
///   lies between that mention and the block; `Absent` otherwise.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures accept a binding read twice and one read
///   inside a `for` body that does not hold it.
/// - witness: `tests::ui_cost_shape`
#[spec(ensures: |output| match output {
    Maybe::Present(found) => found.expr.res_local_id() == Some(local) && found.index > placement.index,
    Maybe::Absent(_) => true,
})]
fn only_use<'tcx>(
    cx: &LateContext<'tcx>,
    placement: Placement<'tcx>,
    local: HirId,
) -> Maybe<Use<'tcx>, use_reading::NotOnce>
{
    let mut uses = Vec::new();
    let rest = placement
        .block
        .stmts
        .get(placement.index.saturating_add(1) ..)
        .unwrap_or_default();
    for statement in rest {
        for_each_expr(cx.tcx, statement, |expr| {
            if expr.res_local_id() == Some(local) {
                uses.push(expr);
            }
            ControlFlow::<()>::Continue(())
        });
    }
    if let Some(tail) = placement.block.expr {
        for_each_expr(cx.tcx, tail, |expr| {
            if expr.res_local_id() == Some(local) {
                uses.push(expr);
            }
            ControlFlow::<()>::Continue(())
        });
    }
    let &[found] = uses.as_slice()
    else {
        return Maybe::Absent(use_reading::NotOnce::Repeated);
    };
    let mut child = found.hir_id;
    for (parent_id, node) in cx.tcx.hir_parent_iter(found.hir_id) {
        if parent_id == placement.block.hir_id {
            let index = placement
                .block
                .stmts
                .iter()
                .position(|statement| statement.hir_id == child)
                .unwrap_or(placement.block.stmts.len());
            return Maybe::Present(Use { expr: found, index });
        }
        if let Node::Expr(parent) = node
            && matches!(parent.kind, ExprKind::Loop(..) | ExprKind::Closure(_))
        {
            return Maybe::Absent(use_reading::NotOnce::Repeated);
        }
        child = parent_id;
    }
    Maybe::Absent(use_reading::NotOnce::Repeated)
}

/// How a `for` loop reads an expression it iterates.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LoopRead
{
    /// The loop iterates the expression itself, moving each element out.
    Moving,
    /// The loop iterates a borrow of it and its pattern copies each element
    /// out (`for &x in &v`).
    Copying,
    /// The loop iterates a borrow of it and binds each element by reference.
    Borrowing,
    /// The expression is not a `for` loop's iterated argument.
    NotALoop,
}

/// Classify how a `for` loop reads an expression it iterates.
///
/// # Specification
/// - ensures: `Moving` when `expr` itself, `Copying` or `Borrowing` when a
///   borrow of it, is the argument of the `IntoIterator::into_iter` call a
///   `for` loop desugars its argument into, `Copying` exactly when the loop's
///   pattern is a reference pattern; `NotALoop` otherwise.
/// - panics: none.
/// - executable: none — the predicate would repeat the parent walk the body
///   makes.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures refuse `for x in collected` and `for &x in
///   &collected`, and accept `for x in &collected` whose borrows are stored.
/// - witness: `tests::ui_cost_shape`
fn loop_read(
    cx: &LateContext<'_>,
    expr: &Expr<'_>,
) -> LoopRead
{
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "`Node` is rustc's, its variants moving with the pinned nightly; only a borrow \
                  stands between a `for` loop's call and its collection"
    )]
    let (argument, borrowed) = match cx.tcx.parent_hir_node(expr.hir_id) {
        Node::Expr(borrow) if matches!(borrow.kind, ExprKind::AddrOf(..)) => (borrow.hir_id, true),
        _ => (expr.hir_id, false),
    };
    let Node::Expr(call) = cx.tcx.parent_hir_node(argument)
    else {
        return LoopRead::NotALoop;
    };
    let ExprKind::Call(_, &[ref iterated]) = call.kind
    else {
        return LoopRead::NotALoop;
    };
    let Node::Expr(desugared) = cx.tcx.parent_hir_node(call.hir_id)
    else {
        return LoopRead::NotALoop;
    };
    let ExprKind::Match(_, _, MatchSource::ForLoopDesugar) = desugared.kind
    else {
        return LoopRead::NotALoop;
    };
    let Node::Expr(outer) = cx.tcx.parent_hir_node(desugared.hir_id)
    else {
        return LoopRead::NotALoop;
    };
    let Some(for_loop) = ForLoop::hir(outer)
    else {
        return LoopRead::NotALoop;
    };
    if iterated.hir_id != argument {
        LoopRead::NotALoop
    }
    else if !borrowed {
        LoopRead::Moving
    }
    else if matches!(for_loop.pat.kind, PatKind::Ref(..)) {
        LoopRead::Copying
    }
    else {
        LoopRead::Borrowing
    }
}

/// Whether a borrowing read's result is copied or cloned at once.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Copied
{
    /// `copied()` or `cloned()` is called on the read's result.
    Yes,
    /// The read's result is used otherwise.
    No,
}

/// Classify what is done with a borrowing read's result.
///
/// # Specification
/// - ensures: `Yes` exactly when `reader` is the receiver of a `copied()` or
///   `cloned()` call.
/// - panics: none.
/// - executable: none — the predicate would repeat the parent lookup the body
///   makes.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures refuse `iter().copied()` and
///   `first().copied()` and accept `iter()` whose borrows are stored.
/// - witness: `tests::ui_cost_shape`
fn copied_at_once(
    cx: &LateContext<'_>,
    reader: &Expr<'_>,
) -> Copied
{
    if let Node::Expr(next) = cx.tcx.parent_hir_node(reader.hir_id)
        && let ExprKind::MethodCall(segment, receiver, &[], _) = next.kind
        && receiver.hir_id == reader.hir_id
        && matches!(segment.ident.name.as_str(), "copied" | "cloned")
    {
        Copied::Yes
    }
    else {
        Copied::No
    }
}

/// Classify the read of a collection's value.
///
/// # Specification
/// - ensures: `Present` with the read and its expression when `value` is the
///   receiver of a direct `into_iter()` or of `iter()`, `first()`, `last()` or
///   `get(i)` with a `usize` index whose result is copied or cloned at once
///   ([`copied_at_once`]), or the iterated argument of a `for` loop that moves
///   or copies its elements out ([`loop_read`]).
/// - ensures: `Absent(BorrowingRead)` for `iter_mut()`, for those reads when
///   their result is not copied at once and for a `for` loop that binds by
///   reference; `Absent(ClippyCovers)` for `len()`, `is_empty()`,
///   `contains(_)`, an index and `into_iter()` through a `let`; `Absent(Kept)`
///   for every other use.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures refuse each moving or copying read and
///   accept a borrowing read whose borrows are stored, `get` with a range,
///   `needless_collect`'s reads, `sort()` and a return.
/// - witness: `tests::ui_cost_shape`
#[spec(ensures: |output| match output {
    Maybe::Present((_, reader)) => reader.hir_id != value.hir_id || matches!(loop_read(cx, value), LoopRead::Moving | LoopRead::Copying),
    Maybe::Absent(_) => true,
})]
fn single_read<'tcx>(
    cx: &LateContext<'tcx>,
    value: &'tcx Expr<'tcx>,
    binding: Binding,
) -> Maybe<(Read, &'tcx Expr<'tcx>), collection_reading::Needed>
{
    match loop_read(cx, value) {
        LoopRead::Moving | LoopRead::Copying => return Maybe::Present((Read::Iterated, value)),
        LoopRead::Borrowing => return Maybe::Absent(collection_reading::Needed::BorrowingRead),
        LoopRead::NotALoop => {},
    }
    let Node::Expr(reader) = cx.tcx.parent_hir_node(value.hir_id)
    else {
        return Maybe::Absent(collection_reading::Needed::Kept);
    };
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "`ExprKind` is rustc's, its variants moving with the pinned nightly; only a \
                  method call or an index reads the collection in place"
    )]
    match reader.kind {
        ExprKind::MethodCall(segment, receiver, arguments, _)
            if receiver.hir_id == value.hir_id =>
        {
            let read = match (segment.ident.name.as_str(), binding, arguments) {
                ("into_iter", Binding::Direct, &[]) => {
                    return Maybe::Present((Read::Iterated, reader));
                },
                ("iter", _, &[]) => Read::Iterated,
                ("first", _, &[]) => Read::First,
                ("last", _, &[]) => Read::Last,
                ("get", _, &[ref index]) if cx.typeck_results().expr_ty(index).is_usize() => {
                    Read::Indexed
                },
                ("iter_mut", _, &[]) => {
                    return Maybe::Absent(collection_reading::Needed::BorrowingRead);
                },
                ("len" | "is_empty" | "contains", ..) | ("into_iter", Binding::Let, &[]) => {
                    return Maybe::Absent(collection_reading::Needed::ClippyCovers);
                },
                _ => return Maybe::Absent(collection_reading::Needed::Kept),
            };
            match copied_at_once(cx, reader) {
                Copied::Yes => Maybe::Present((read, reader)),
                Copied::No => Maybe::Absent(collection_reading::Needed::BorrowingRead),
            }
        },
        ExprKind::Index(collection, ..) if collection.hir_id == value.hir_id => {
            Maybe::Absent(collection_reading::Needed::ClippyCovers)
        },
        _ => Maybe::Absent(collection_reading::Needed::Kept),
    }
}

/// Whether an iterated read can reverse the source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Reversal
{
    /// The read does not reverse, or the source reverses as the collection
    /// does.
    Possible,
    /// The read's chain calls `rev()` and the source is not both
    /// double-ended and exact-size.
    Impossible,
}

/// Classify the reversal an iterated read asks of its source.
///
/// # Specification
/// - ensures: `Impossible` exactly when `rev()` is called somewhere along the
///   method chain whose innermost receiver is `reader`, and `iterator` does not
///   implement both `DoubleEndedIterator` and `ExactSizeIterator`: an adapter
///   between the read and `rev()` (`enumerate`, `zip`, `skip`) can need the
///   exact length the collection had.
/// - panics: none.
/// - executable: none — the predicate would repeat the trait queries the body
///   makes.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures accept `collect().into_iter().rev()` of a
///   `successors` chain and refuse it of a mapped slice iterator.
/// - witness: `tests::ui_cost_shape`
fn reversed(
    cx: &LateContext<'_>,
    reader: &Expr<'_>,
    iterator: &Expr<'_>,
) -> Reversal
{
    let mut current = reader.hir_id;
    let mut reverses = false;
    while let Node::Expr(next) = cx.tcx.parent_hir_node(current)
        && let ExprKind::MethodCall(segment, receiver, ..) = next.kind
        && receiver.hir_id == current
    {
        reverses |= segment.ident.name.as_str() == "rev";
        current = next.hir_id;
    }
    if !reverses {
        return Reversal::Possible;
    }
    let source = cx.typeck_results().expr_ty(iterator);
    let reverses_as_collected = ["DoubleEndedIterator", "ExactSizeIterator"]
        .iter()
        .all(|name| {
            cx.tcx
                .get_diagnostic_item(Symbol::intern(name))
                .is_some_and(|trait_id| implements_trait(cx, source, trait_id, &[]))
        });
    if reverses_as_collected {
        Reversal::Possible
    }
    else {
        Reversal::Impossible
    }
}

/// Whether an iterator's closures can run effects a collection orders.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Effect
{
    /// A closure calls something with a value it captures, or calls an
    /// `unsafe` function.
    Effectful,
    /// No closure does.
    Pure,
}

/// Classify the effects an iterator's closures can run.
///
/// # Specification
/// - ensures: `Effectful` exactly when some closure in `iterator`, nested ones
///   included, holds a call or method call whose callee, receiver or an
///   argument mentions a variable the closure captures, or a call to an
///   `unsafe` function or method; `Pure` otherwise. Collecting runs every such
///   call before the read; iterating the source interleaves them with it, which
///   for a thread spawned per element joins each before the next starts.
/// - panics: none.
/// - executable: none — the predicate would repeat the closure walk the body
///   makes.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures accept handles of threads spawned in a scope
///   and joined after the collection, and blocks allocated before any is freed,
///   and refuse a `map` over the elements alone.
/// - witness: `tests::ui_cost_shape`
fn effects<'tcx>(
    cx: &LateContext<'tcx>,
    iterator: &'tcx Expr<'tcx>,
) -> Effect
{
    let typeck = cx.typeck_results();
    let unsafe_callee = |expr: &Expr<'_>| {
        #[expect(
            clippy::wildcard_enum_match_arm,
            reason = "`ExprKind` is rustc's, its variants moving with the pinned nightly; only a \
                      call or a method call has a callee"
        )]
        match expr.kind {
            ExprKind::Call(callee, _) => {
                let callee = typeck.expr_ty(callee);
                callee.is_fn() && callee.fn_sig(cx.tcx).safety().is_unsafe()
            },
            ExprKind::MethodCall(..) => typeck
                .type_dependent_def_id(expr.hir_id)
                .is_some_and(|method| cx.tcx.fn_sig(method).skip_binder().safety().is_unsafe()),
            _ => false,
        }
    };
    let effectful = for_each_expr(cx.tcx, iterator, |expr| {
        let ExprKind::Closure(closure) = expr.kind
        else {
            return ControlFlow::Continue(());
        };
        let captured = cx.tcx.upvars_mentioned(closure.def_id);
        let mentions_captured = |operand: &'tcx Expr<'tcx>| {
            captured.is_some_and(|upvars| {
                for_each_expr(cx.tcx, operand, |inner| {
                    if inner
                        .res_local_id()
                        .is_some_and(|local| upvars.contains_key(&local))
                    {
                        ControlFlow::Break(())
                    }
                    else {
                        ControlFlow::Continue(())
                    }
                })
                .is_some()
            })
        };
        let body = cx.tcx.hir_body(closure.body);
        let calls = for_each_expr(cx.tcx, body.value, |inner| {
            #[expect(
                clippy::wildcard_enum_match_arm,
                reason = "`ExprKind` is rustc's, its variants moving with the pinned nightly; only a \
                          call or a method call runs an effect"
            )]
            let with_captured = match inner.kind {
                ExprKind::Call(callee, arguments) => {
                    mentions_captured(callee) || arguments.iter().any(&mentions_captured)
                },
                ExprKind::MethodCall(_, receiver, arguments, _) => {
                    mentions_captured(receiver) || arguments.iter().any(&mentions_captured)
                },
                _ => false,
            };
            if with_captured || unsafe_callee(inner) {
                ControlFlow::Break(())
            }
            else {
                ControlFlow::Continue(())
            }
        });
        if calls.is_some() {
            ControlFlow::Break(())
        }
        else {
            ControlFlow::Continue(())
        }
    });
    if effectful.is_some() {
        Effect::Effectful
    }
    else {
        Effect::Pure
    }
}

/// Whether an iterator borrows a temporary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Borrowed
{
    /// Some receiver in the chain is an owned value computed in place and
    /// borrowed, by reference or through an overloaded dereference.
    Temporary,
    /// Every borrowed receiver is a place or a reference value.
    Places,
}

/// Classify the receivers an iterator chain borrows.
///
/// # Specification
/// - ensures: `Temporary` exactly when a receiver along `iterator`'s method
///   chain, field projections and borrows is not a place, is not of reference
///   type, and is borrowed by an autoref, an overloaded dereference or an
///   explicit `&`.
/// - panics: none.
/// - executable: none — the predicate would repeat the chain walk the body
///   makes.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures accept a collection of a lock guard's
///   iterator and refuse one of a local vector's.
/// - witness: `tests::ui_cost_shape`
fn temporary_borrow(
    cx: &LateContext<'_>,
    iterator: &Expr<'_>,
) -> Borrowed
{
    let typeck = cx.typeck_results();
    let owned_temporary = |receiver: &Expr<'_>| {
        !receiver.is_place_expr(|_| false) && !typeck.expr_ty(receiver).is_ref()
    };
    let mut current = iterator;
    loop {
        #[expect(
            clippy::wildcard_enum_match_arm,
            reason = "`ExprKind` is rustc's, its variants moving with the pinned nightly; only a \
                      method call, a field or a borrow continues the chain"
        )]
        match current.kind {
            ExprKind::MethodCall(_, receiver, ..) => {
                let borrowed = typeck.expr_adjustments(receiver).iter().any(|adjustment| {
                    matches!(
                        adjustment.kind,
                        Adjust::Borrow(_) | Adjust::Deref(DerefAdjustKind::Overloaded(_))
                    )
                });
                if borrowed && owned_temporary(receiver) {
                    return Borrowed::Temporary;
                }
                current = receiver;
            },
            ExprKind::Field(base, _) | ExprKind::Index(base, ..) => current = base,
            ExprKind::AddrOf(_, _, inner) => {
                if owned_temporary(inner) {
                    return Borrowed::Temporary;
                }
                current = inner;
            },
            _ => return Borrowed::Places,
        }
    }
}

/// Whether a region touches a local an iterator reads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Touch
{
    /// A statement between the collection and the read uses such a local, or
    /// the read's statement mutates one.
    Touched,
    /// Neither does.
    Untouched,
}

/// Classify a region's uses of the locals an iterator reads.
///
/// # Specification
/// - ensures: `Touched` exactly when some local `iterator` mentions, or a local
///   its binding was initialized from ([`binding_roots`]), is mentioned by a
///   statement of the region before the read's own, or is mutably borrowed or
///   assigned in the read's statement or tail, as `mutated_variables` reports,
///   or when that report is unavailable. Iterating the source in place of the
///   collection moves its reads past every such statement and holds its borrows
///   through the read.
/// - panics: none.
/// - executable: none — the predicate would repeat the use and mutation
///   analyses the body runs.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures accept a collection whose source is dropped
///   before the read, one whose loop removes from the source, and one whose
///   source borrows from `self` through two bindings while the loop mutates
///   `self`, and refuse one whose loop only reads.
/// - witness: `tests::ui_cost_shape`
fn source_touched<'tcx>(
    cx: &LateContext<'tcx>,
    iterator: &'tcx Expr<'tcx>,
    region: Region<'tcx>,
) -> Touch
{
    let read = binding_roots(cx, iterator);
    if read.is_empty() {
        return Touch::Untouched;
    }
    let statements = region.block.stmts;
    let intervening = statements
        .get(region.first .. region.last.min(statements.len()))
        .unwrap_or_default();
    for statement in intervening {
        let uses = for_each_expr(cx.tcx, statement, |expr| {
            if expr
                .res_local_id()
                .is_some_and(|local| read.contains(&local))
            {
                ControlFlow::Break(())
            }
            else {
                ControlFlow::Continue(())
            }
        });
        if uses.is_some() {
            return Touch::Touched;
        }
    }
    let reading =
        statements
            .get(region.last)
            .map_or(region.block.expr, |statement| match statement.kind {
                StmtKind::Let(binding) => binding.init,
                StmtKind::Expr(expr) | StmtKind::Semi(expr) => Some(expr),
                StmtKind::Item(_) => None,
            });
    let Some(reading) = reading
    else {
        return Touch::Untouched;
    };
    match mutated_variables(reading, cx) {
        Some(mutated) if !mutated.iter().any(|local| read.contains(local)) => Touch::Untouched,
        Some(_) | None => Touch::Touched,
    }
}

/// The locals an expression mentions and the locals their bindings were
/// initialized from, transitively.
///
/// # Specification
/// - ensures: the result holds every local `expr` mentions and, for each local
///   in the result bound by a pattern of a `let` statement, a `let` expression
///   or a `match` arm, every local its initializer or scrutinee mentions.
/// - panics: none.
/// - executable: none — the predicate would repeat the fixpoint the body
///   computes.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures accept a collection of components borrowed
///   from `self` through two bindings whose loop mutates `self`.
/// - witness: `tests::ui_cost_shape`
fn binding_roots<'tcx>(
    cx: &LateContext<'tcx>,
    expr: &'tcx Expr<'tcx>,
) -> HirIdSet
{
    let mut roots = HirIdSet::default();
    let mut pending = Vec::new();
    let mention = |source: &'tcx Expr<'tcx>, roots: &mut HirIdSet, pending: &mut Vec<HirId>| {
        for_each_expr(cx.tcx, source, |inner| {
            if let Some(local) = inner.res_local_id()
                && roots.insert(local)
            {
                pending.push(local);
            }
            ControlFlow::<()>::Continue(())
        });
    };
    mention(expr, &mut roots, &mut pending);
    while let Some(local) = pending.pop() {
        let mut source = None;
        let mut in_arm = false;
        for (_, node) in cx.tcx.hir_parent_iter(local) {
            #[expect(
                clippy::wildcard_enum_match_arm,
                reason = "`Node` is rustc's, its variants moving with the pinned nightly; only a \
                          pattern, a binding site or an arm's match leads to an initializer"
            )]
            match node {
                Node::Pat(_) | Node::PatField(_) => {},
                Node::Arm(_) => in_arm = true,
                Node::LetStmt(binding) => {
                    source = binding.init;
                    break;
                },
                Node::Expr(&Expr {
                    kind: ExprKind::Let(binding),
                    ..
                }) => {
                    source = Some(binding.init);
                    break;
                },
                Node::Expr(&Expr {
                    kind: ExprKind::Match(scrutinee, ..),
                    ..
                }) if in_arm => {
                    source = Some(scrutinee);
                    break;
                },
                _ => break,
            }
        }
        if let Some(source) = source {
            mention(source, &mut roots, &mut pending);
        }
    }
    roots
}
