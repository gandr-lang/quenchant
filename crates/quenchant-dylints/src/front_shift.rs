//! A buffer's front removed or inserted once per pass of a loop.
//!
//! `Vec::remove(0)`, `Vec::insert(0, _)` and `Vec::drain` of a prefix move
//! every element after the front; so do `String::remove(0)`,
//! `String::insert(0, _)`, `String::insert_str(0, _)` and `String::drain` of
//! a prefix, over every byte. Once, that is one move of the buffer. Repeated
//! by a loop over a buffer bound outside it — a queue drained from the front,
//! a sequence built by prepending — `k` passes over an `n`-element buffer cost
//! `O(n·k)`, where a `VecDeque`'s `pop_front` and `push_front` cost `O(1)`,
//! and a prefix removed once with `drain(..k)` after the loop costs `O(n)`.
//!
//! The receiver's type decides: `VecDeque`'s own front operations are
//! constant-time and never inspected, and an index other than the literal
//! front is not a front shift. The repetition decides too
//! ([`crate::cost_shape`]): a shift outside any loop, inside a loop over an
//! array, on a buffer bound inside the loop or on a computed value is
//! accepted.
//!
//! Clippy has no lint for a front shift.

use anodized::spec;
use clippy_utils::diagnostics::span_lint_and_then;
use clippy_utils::higher;
use clippy_utils::is_integer_literal;
use quenchant_shape::shape::Maybe;
use rustc_hir::Expr;
use rustc_hir::ExprKind;
use rustc_lint::LateContext;
use rustc_lint::LateLintPass;
use rustc_session::declare_lint;
use rustc_session::impl_lint_pass;

use crate::clause_cost::ClauseRegions;
use crate::cost_shape::Inspection;
use crate::cost_shape::Method;
use crate::cost_shape::Owner;
use crate::cost_shape::Repeated;
use crate::cost_shape::inspection;
use crate::cost_shape::repeated_read;
use crate::cost_shape::std_method;

declare_lint! {
    /// ### What it does
    ///
    /// Refuses a shift of a buffer's front repeated by a loop over a buffer that outlives it: `Vec::remove(0)`, `Vec::insert(0, _)`, `Vec::drain` of a range that starts at the front and has an end, and the same three on `String` together with `String::insert_str(0, _)`, inside a `loop`, `while` or `for` body or a closure an iterator or a standard container calls once per element, when the buffer is bound outside that loop.
    ///
    /// ### Why is this bad?
    ///
    /// Each call moves every element after the front. `k` calls on an `n`-element buffer cost `O(n·k)`: draining a queue from the front, or building a sequence by prepending, is quadratic. A `VecDeque` pops and pushes its front in `O(1)`; a sequence built in reverse can pop and push its back; a prefix removed once with `drain(..k)` after the loop costs `O(n)`.
    ///
    /// ### Accepted forms
    ///
    /// The receiver's type decides: `VecDeque`'s front operations are constant-time and never inspected, and only the literal index `0` or a range from the front shifts the whole buffer. A shift outside any loop, inside a loop over an array or a range with constant bounds, on a buffer bound inside the loop, or on a value computed in place is accepted.
    ///
    /// ### Clippy
    ///
    /// No Clippy lint prices a front shift.
    ///
    /// ### Example
    ///
    /// ```rust,ignore
    /// while !queue.is_empty() {
    ///     let next = queue.remove(0);
    ///     visit(next, &mut queue);
    /// }
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// let mut queue = VecDeque::from(queue);
    /// while let Some(next) = queue.pop_front() {
    ///     visit(next, &mut queue);
    /// }
    /// ```
    pub FRONT_SHIFT_IN_LOOP,
    Deny,
    "a buffer's front is removed or inserted once per pass of a loop"
}

quenchant_shape::reason_enum! {
    /// A front operation can cost no more than one move of its buffer per
    /// loop.
    pub mod shift_reading {
        /// Why the call is not refused.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Bounded {
            /// The call is not a front removal or insertion on a `Vec` or a
            /// `String`.
            NotAShift,
            /// The shift does not repeat over one buffer; the reason is the
            /// repetition's.
            Unrepeated(crate::cost_shape::repetition_reading::Unrepeated),
        }
    }
}

/// A refused shift: the buffer and the loop that repeats it.
#[derive(Clone, Copy, Debug)]
pub struct Shift<'tcx>
{
    /// The buffer whose front moves.
    pub buffer: &'tcx Expr<'tcx>,
    /// The repetition that shifts it again on every pass.
    pub repeated: Repeated<'tcx>,
}

/// Repeated-front-shift policy; specification clauses are left to
/// `spec_clause_traversal`.
#[repr(transparent)]
pub struct FrontShiftInLoop
{
    /// The authored `spec` attribute ranges, whose clauses are not inspected.
    regions: ClauseRegions,
}

impl FrontShiftInLoop
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

impl_lint_pass!(FrontShiftInLoop => [FRONT_SHIFT_IN_LOOP]);

impl<'tcx> LateLintPass<'tcx> for FrontShiftInLoop
{
    /// Report each repeated front shift once, at the call.
    ///
    /// # Specification
    /// - ensures: reports every expression [`shift`] answers `Present` for,
    ///   labelling the buffer, with a note at the loop that repeats it.
    /// - ensures: expressions from macro expansions and inside `spec`
    ///   attributes are not inspected ([`inspection`]).
    /// - panics: none.
    /// - executable: none — rustc emits diagnostics without a queryable
    ///   per-call diagnostic result.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the fixtures refuse each front operation in each
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
        let Maybe::Present(found) = shift(cx, expr)
        else {
            return;
        };
        span_lint_and_then(
            cx,
            FRONT_SHIFT_IN_LOOP,
            expr.span,
            "this call moves every element after the front, once per pass of a loop over a \
             buffer that outlives it",
            |diagnostic| {
                diagnostic.span_label(found.buffer.span, "shifted again on every pass");
                diagnostic.span_note(found.repeated.scope.span, "the loop that repeats it");
                diagnostic.help(
                    "use a `VecDeque` and its `pop_front` or `push_front`, build the sequence in \
                     reverse and use its back, or remove the whole prefix once with `drain(..k)`",
                );
            },
        );
    }
}

/// Decide whether an expression is a repeated front shift.
///
/// # Specification
/// - ensures: `Present` when `expr` is `remove(0)`, `insert(0, _)` or a
///   front-anchored, end-bounded `drain` on a `Vec` or `String`, or
///   `insert_str(0, _)` on a `String`, whose buffer is read again by an
///   enclosing repetition ([`repeated_read`]).
/// - ensures: `Absent(NotAShift)` for every other expression;
///   `Absent(Unrepeated)` with the repetition's reason otherwise.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures refuse each front operation in a `while`, a
///   `for` and a `for_each` closure, and accept a nonzero index, a full drain,
///   a deque, a single shift and a buffer bound inside the loop.
/// - witness: `tests::ui_cost_shape`
#[spec(ensures: |output| match output {
    Maybe::Present(found) => matches!(expr.kind, ExprKind::MethodCall(_, receiver, ..) if receiver.hir_id == found.buffer.hir_id),
    Maybe::Absent(_) => true,
})]
pub fn shift<'tcx>(
    cx: &LateContext<'tcx>,
    expr: &'tcx Expr<'tcx>,
) -> Maybe<Shift<'tcx>, shift_reading::Bounded>
{
    let ExprKind::MethodCall(_, buffer, arguments, _) = expr.kind
    else {
        return Maybe::Absent(shift_reading::Bounded::NotAShift);
    };
    let Maybe::Present(Method { owner, name }) = std_method(cx, expr)
    else {
        return Maybe::Absent(shift_reading::Bounded::NotAShift);
    };
    let front = match (owner, name.as_str(), arguments) {
        (Owner::Vec | Owner::String, "remove" | "insert", &[ref index, ..])
        | (Owner::String, "insert_str", &[ref index, _]) => is_integer_literal(index, 0),
        (Owner::Vec | Owner::String, "drain", &[ref range]) => higher::Range::hir(cx, range)
            .is_some_and(|range| {
                range.end.is_some() && range.start.is_none_or(|start| is_integer_literal(start, 0))
            }),
        _ => false,
    };
    if !front {
        return Maybe::Absent(shift_reading::Bounded::NotAShift);
    }
    match repeated_read(cx, expr, buffer) {
        Maybe::Present(repeated) => Maybe::Present(Shift { buffer, repeated }),
        Maybe::Absent(reason) => Maybe::Absent(shift_reading::Bounded::Unrepeated(reason)),
    }
}
