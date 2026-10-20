//! An allocation made and dropped within one pass of a repeated body.
//!
//! A buffer created inside a loop body and dropped at the pass's end, or a
//! string formatted only to be read in that pass, allocates and frees on
//! every pass: `k` passes cost `k` allocations and `k` frees where one buffer
//! hoisted before the loop and cleared each pass costs one allocation, grown
//! to the largest pass and reused, and a borrowed read costs none. The rule
//! the lint checks is that a hot path allocates nothing in steady state.
//!
//! An allocation is a growable buffer made by `Vec::new`, `VecDeque::new`,
//! `String::new` or an empty `vec![]` and then grown; `with_capacity` and
//! `vec![..]` with elements; `Box::new` of a value with a size; `format!` and
//! `to_string()`; and `collect()` into an owned collection. A `clone()` of an
//! owned value is not one of them.
//!
//! # Boundaries
//!
//! The repetition is [`crate::cost_shape`]'s: a loop body or a per-element
//! closure whose trip count is open, and that the allocation does not leave
//! at once. An allocation whose value leaves the pass — pushed into an outer
//! collection, returned, stored, handed on by value or captured by a moving
//! closure — is the structure's growth, not a throwaway, and is accepted. So
//! is an allocation on a path that diverges, such as a panic's message, and a
//! `collect()` that [`collect_read_once`] already refuses.
//!
//! Clippy's `format_collect` and `format_push_string` take a `format!`
//! collected or pushed onto a `String`; no Clippy lint prices an allocation
//! per pass.
//!
//! [`collect_read_once`]: crate::collect_read_once::COLLECT_READ_ONCE

use anodized::spec;
use clippy_utils::diagnostics::span_lint_and_then;
use clippy_utils::macros::root_macro_call_first_node;
use quenchant_shape::shape::Maybe;
use rustc_hir::Expr;
use rustc_hir::ExprKind;
use rustc_hir::Node;
use rustc_hir::PatKind;
use rustc_hir::def::Res;
use rustc_lint::LateContext;
use rustc_lint::LateLintPass;
use rustc_middle::ty::layout::LayoutOf as _;
use rustc_session::declare_lint;
use rustc_session::impl_lint_pass;
use rustc_span::Span;
use rustc_span::sym;

use crate::clause_cost::ClauseRegions;
use crate::clause_cost::Enclosure;
use crate::collect_read_once::read_once;
use crate::cost_shape::Divergence;
use crate::cost_shape::Escape;
use crate::cost_shape::Method;
use crate::cost_shape::Mutation;
use crate::cost_shape::Owner;
use crate::cost_shape::divergence;
use crate::cost_shape::pass_escape;
use crate::cost_shape::pass_mutation;
use crate::cost_shape::repetition;
use crate::cost_shape::std_method;
use crate::walk_summary::standard_name;

declare_lint! {
    /// ### What it does
    ///
    /// Refuses an allocation inside a `loop`, `while` or `for` body, or a closure an iterator or a standard container calls once per element, whose value is dropped before the next pass: a `Vec`, `VecDeque` or `String` made by `new` or an empty `vec![]` and then grown, or made by `with_capacity` or `vec![..]` with elements; `Box::new` of a value with a size; `format!`; `to_string()`; `collect()` into an owned collection.
    ///
    /// ### Why is this bad?
    ///
    /// Each pass allocates and frees. `k` passes cost `k` allocations where one buffer made before the loop and cleared each pass costs one, grown to the largest pass and reused; a value made only to be read in the pass can usually be borrowed or written into a reused buffer instead.
    ///
    /// ### Accepted forms
    ///
    /// An allocation whose value leaves the pass — pushed into a collection made outside it, returned, stored in a field, handed on by value, captured by a moving closure — is accepted: that is the structure's growth. So is an allocation outside any loop, in a loop over an array or a range with constant bounds, in a pass the allocation leaves at once, on a path that diverges, a buffer made by `new` and never grown, and a `collect()` that `collect_read_once` refuses.
    ///
    /// ### Clippy
    ///
    /// `format_collect` and `format_push_string` take a `format!` collected or pushed onto a `String`; no Clippy lint prices an allocation per pass.
    ///
    /// ### Example
    ///
    /// ```rust,ignore
    /// for line in lines {
    ///     let mut words = Vec::new();
    ///     words.extend(line.split(' '));
    ///     emit(&words);
    /// }
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// let mut words = Vec::new();
    /// for line in lines {
    ///     words.clear();
    ///     words.extend(line.split(' '));
    ///     emit(&words);
    /// }
    /// ```
    pub ALLOCATION_IN_REPEATED_BODY,
    Deny,
    "a repeated body allocates a value it drops before the next pass"
}

quenchant_shape::reason_enum! {
    /// An allocation can be paid once, or be needed by what follows the pass.
    pub mod allocation_reading {
        /// Why the expression is not refused.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Unrefused {
            /// The expression allocates nothing this lint prices.
            NotAnAllocation,
            /// A buffer made by `new` is never grown, so it never allocates.
            NeverGrown,
            /// The allocation does not repeat; the reason is the repetition's.
            Unrepeated(crate::cost_shape::repetition_reading::Unrepeated),
            /// The allocation sits on a path that diverges.
            Diverges,
            /// The value leaves the pass.
            Escapes,
            /// `collect_read_once` refuses the collection.
            ReadOnce,
        }
    }
}

/// How an expression allocates.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Making
{
    /// An empty growable buffer, which allocates once grown.
    Empty,
    /// An allocation made by the expression itself.
    Allocates,
}

/// A refused allocation: where it is and the loop that repeats it.
#[derive(Clone, Copy, Debug)]
pub struct Allocation<'tcx>
{
    /// The allocating expression, or its macro call.
    pub at: Span,
    /// The repetition that makes it again on every pass.
    pub scope: &'tcx Expr<'tcx>,
}

/// Repeated-allocation policy; specification clauses are left to
/// `spec_clause_traversal`.
#[repr(transparent)]
pub struct AllocationInRepeatedBody
{
    /// The authored `spec` attribute ranges, whose clauses are not inspected.
    regions: ClauseRegions,
}

impl AllocationInRepeatedBody
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

impl_lint_pass!(AllocationInRepeatedBody => [ALLOCATION_IN_REPEATED_BODY]);

impl<'tcx> LateLintPass<'tcx> for AllocationInRepeatedBody
{
    /// Report each allocation a repeated body drops before its next pass.
    ///
    /// # Specification
    /// - ensures: reports every expression [`allocation`] answers `Present`
    ///   for, at the expression or its macro call, with a note at the loop that
    ///   repeats it.
    /// - ensures: an expression whose span, or whose macro call's span, lies
    ///   inside a `spec` attribute is not inspected.
    /// - panics: none.
    /// - executable: none — rustc emits diagnostics without a queryable
    ///   per-call diagnostic result.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the fixtures refuse each allocating form in each
    ///   repetition and accept each exception beside it.
    /// - witness: `tests::ui_cost_shape`
    fn check_expr(
        &mut self,
        cx: &LateContext<'tcx>,
        expr: &'tcx Expr<'tcx>,
    )
    {
        let Maybe::Present(found) = allocation(cx, expr)
        else {
            return;
        };
        if self.regions.encloses(found.at) == Enclosure::Inside {
            return;
        }
        span_lint_and_then(
            cx,
            ALLOCATION_IN_REPEATED_BODY,
            found.at,
            "this allocates on every pass of a loop, and the value is dropped before the next pass",
            |diagnostic| {
                diagnostic.span_note(found.scope.span, "the loop that repeats it");
                diagnostic.help(
                    "make the buffer once before the loop and `clear()` it each pass, write into \
                     one reused buffer, or borrow what the pass reads",
                );
            },
        );
    }
}

/// Decide whether an expression is an allocation a repeated body drops.
///
/// # Specification
/// - ensures: `Present` when `expr` allocates ([`making`]) inside an open
///   repetition ([`repetition`]) on a path that does not diverge
///   ([`divergence`]), its value does not leave the pass ([`pass_escape`]), an
///   empty buffer is bound to a local the repetition mutates, and a `collect`
///   is not one [`read_once`] refuses.
/// - ensures: `Absent` with the first reason that holds otherwise, in the order
///   the body tests them.
/// - panics: none.
/// - executable: none — the outcome is the compiler's resolution and use walk
///   of the enclosing body, which a predicate could only repeat.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures refuse a grown buffer, a `with_capacity`
///   buffer, `vec![..]`, `Box::new`, `format!`, `to_string()` and a `collect`
///   into a set, each dropped in its pass, and accept the same allocations
///   pushed outward, returned from a per-element closure, stored, made by `new`
///   and never grown, outside any loop, in a loop over an array, in a panic's
///   message and read once by `collect_read_once`.
/// - witness: `tests::ui_cost_shape`
pub fn allocation<'tcx>(
    cx: &LateContext<'tcx>,
    expr: &'tcx Expr<'tcx>,
) -> Maybe<Allocation<'tcx>, allocation_reading::Unrefused>
{
    let (made, at) = match making(cx, expr) {
        Maybe::Present(found) => found,
        Maybe::Absent(reason) => return Maybe::Absent(reason),
    };
    let scope = match repetition(cx, expr) {
        Maybe::Present(scope) => scope,
        Maybe::Absent(reason) => {
            return Maybe::Absent(allocation_reading::Unrefused::Unrepeated(reason));
        },
    };
    if divergence(cx, expr, scope) == Divergence::Diverges {
        return Maybe::Absent(allocation_reading::Unrefused::Diverges);
    }
    if pass_escape(cx, expr, scope) == Escape::Escapes {
        return Maybe::Absent(allocation_reading::Unrefused::Escapes);
    }
    if made == Making::Empty && grown(cx, expr, scope) == Mutation::Untouched {
        return Maybe::Absent(allocation_reading::Unrefused::NeverGrown);
    }
    if matches!(read_once(cx, expr), Maybe::Present(_)) {
        return Maybe::Absent(allocation_reading::Unrefused::ReadOnce);
    }
    Maybe::Present(Allocation { at, scope })
}

/// Decide whether an empty buffer is grown.
///
/// # Specification
/// - ensures: `Mutated` exactly when `value` initializes a plain `let` binding
///   whose local the repetition `scope` mutably borrows or assigns
///   ([`pass_mutation`]).
/// - panics: none.
/// - executable: none — the outcome is the compiler's use walk, which a
///   predicate could only repeat.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures refuse a `Vec::new()` pushed to and accept
///   one only read.
/// - witness: `tests::ui_cost_shape`
fn grown<'tcx>(
    cx: &LateContext<'tcx>,
    value: &'tcx Expr<'tcx>,
    scope: &'tcx Expr<'tcx>,
) -> Mutation
{
    let Node::LetStmt(binding) = cx.tcx.parent_hir_node(value.hir_id)
    else {
        return Mutation::Untouched;
    };
    let PatKind::Binding(_, local, _, None) = binding.pat.kind
    else {
        return Mutation::Untouched;
    };
    pass_mutation(cx, local, scope)
}

/// Classify how an expression allocates, and where to report it.
///
/// # Specification
/// - ensures: `Present(Empty)` for a call of `new` on `Vec`, `VecDeque` or
///   `String`, and for an empty `vec![]`; `Present(Allocates)` for
///   `with_capacity` on those three, a `vec![..]` with elements, `Box::new` of
///   a value with a nonzero size, a `format!`, a call resolving to
///   `ToString::to_string`, and `Iterator::collect` into a `Vec`, `VecDeque`,
///   `String`, standard map, set, heap or list, or a `Box`; the span is the
///   macro call's for a macro and the expression's otherwise.
/// - ensures: `Absent(NotAnAllocation)` for every other expression, including
///   expressions inside a macro expansion other than its first node.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures refuse each allocating form and accept a
///   `Box::new(())`, a `Vec::new()` never grown and a same-named method of a
///   local type.
/// - witness: `tests::ui_cost_shape`
#[spec(ensures: |output| match output {
    Maybe::Present((_, at)) => !at.is_dummy(),
    Maybe::Absent(_) => true,
})]
#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "`ExprKind` is rustc's, its variants moving with the pinned nightly; only a call or a \
              method call allocates outside a macro"
)]
fn making<'tcx>(
    cx: &LateContext<'tcx>,
    expr: &'tcx Expr<'tcx>,
) -> Maybe<(Making, Span), allocation_reading::Unrefused>
{
    let none = Maybe::Absent(allocation_reading::Unrefused::NotAnAllocation);
    if expr.span.from_expansion() {
        let Some(call) = root_macro_call_first_node(cx, expr)
        else {
            return none;
        };
        if cx.tcx.is_diagnostic_item(sym::format_macro, call.def_id) {
            return Maybe::Present((Making::Allocates, call.span));
        }
        if cx
            .tcx
            .get_diagnostic_name(call.def_id)
            .is_none_or(|name| name.as_str() != "vec_macro")
        {
            return none;
        }
        return match constructor(cx, expr) {
            Maybe::Present((Constructor::New, _)) => Maybe::Present((Making::Empty, call.span)),
            Maybe::Present(_) | Maybe::Absent(_) => Maybe::Present((Making::Allocates, call.span)),
        };
    }
    match expr.kind {
        ExprKind::Call(_, arguments) => match constructor(cx, expr) {
            Maybe::Present((Constructor::New, Standard::Buffer)) => {
                Maybe::Present((Making::Empty, expr.span))
            },
            Maybe::Present((Constructor::WithCapacity, Standard::Buffer)) => {
                Maybe::Present((Making::Allocates, expr.span))
            },
            Maybe::Present((Constructor::New, Standard::Box)) => {
                let sized = arguments.first().is_some_and(|boxed| {
                    let held = cx.typeck_results().expr_ty(boxed);
                    cx.layout_of(held).is_ok_and(|layout| !layout.is_zst())
                });
                if sized {
                    Maybe::Present((Making::Allocates, expr.span))
                }
                else {
                    none
                }
            },
            Maybe::Present((Constructor::WithCapacity, Standard::Box)) | Maybe::Absent(_) => none,
        },
        ExprKind::MethodCall(..) => {
            let typeck = cx.typeck_results();
            let to_string = typeck
                .type_dependent_def_id(expr.hir_id)
                .and_then(|method| cx.tcx.trait_of_assoc(method))
                .and_then(|owner| cx.tcx.get_diagnostic_name(owner))
                .is_some_and(|owner| owner.as_str() == "ToString");
            if to_string {
                return Maybe::Present((Making::Allocates, expr.span));
            }
            let Maybe::Present(Method {
                owner: Owner::Iterator,
                name,
            }) = std_method(cx, expr)
            else {
                return none;
            };
            let collected = typeck.expr_ty(expr);
            let owned = collected.is_box()
                || collected.ty_adt_def().is_some_and(|definition| {
                    matches!(
                        standard_name(cx.tcx, definition.did()),
                        Maybe::Present(name) if matches!(
                            name.as_str(),
                            "Vec" | "VecDeque" | "String" | "HashMap" | "HashSet" | "BTreeMap"
                                | "BTreeSet" | "BinaryHeap" | "LinkedList"
                        )
                    )
                });
            if name.as_str() == "collect" && owned {
                Maybe::Present((Making::Allocates, expr.span))
            }
            else {
                none
            }
        },
        _ => none,
    }
}

quenchant_shape::reason_enum! {
    /// A call can name something other than a standard constructor.
    mod constructor_reading {
        /// Why the call names no standard constructor.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum NotAConstructor {
            /// The callee is not a path to an inherent associated function of
            /// a standard type.
            Other,
        }
    }
}

/// A standard constructor this lint prices.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Constructor
{
    /// `new`: an empty value.
    New,
    /// `with_capacity`: a buffer reserved up front.
    WithCapacity,
}

/// The standard type a priced constructor makes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Standard
{
    /// A `Vec`, `VecDeque` or `String`.
    Buffer,
    /// The owned box.
    Box,
}

/// The standard constructor a call resolves to, and the type it makes.
///
/// # Specification
/// - ensures: `Present` when `call` calls a path resolving to the inherent
///   `new` or `with_capacity` of `Vec`, `VecDeque` or `String`
///   ([`standard_name`]), or of the owned box; `Absent` otherwise.
/// - panics: none.
/// - executable: none — the outcome is the compiler's resolution, which a
///   predicate could only repeat.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures make buffers with `Vec::new`,
///   `Vec::with_capacity` and `Box::new`, beside a local type's `new`.
/// - witness: `tests::ui_cost_shape`
fn constructor<'tcx>(
    cx: &LateContext<'tcx>,
    call: &'tcx Expr<'tcx>,
) -> Maybe<(Constructor, Standard), constructor_reading::NotAConstructor>
{
    let other = Maybe::Absent(constructor_reading::NotAConstructor::Other);
    let ExprKind::Call(callee, _) = call.kind
    else {
        return other;
    };
    let ExprKind::Path(ref path) = callee.kind
    else {
        return other;
    };
    let Res::Def(_, function) = cx.qpath_res(path, callee.hir_id)
    else {
        return other;
    };
    let Some(implementation) = cx.tcx.inherent_impl_of_assoc(function)
    else {
        return other;
    };
    let owner = cx
        .tcx
        .type_of(implementation)
        .instantiate_identity()
        .skip_norm_wip();
    let Some(definition) = owner.ty_adt_def()
    else {
        return other;
    };
    let standard = if definition.is_box() {
        Standard::Box
    }
    else {
        match standard_name(cx.tcx, definition.did()) {
            Maybe::Present(name) if matches!(name.as_str(), "Vec" | "VecDeque" | "String") => {
                Standard::Buffer
            },
            Maybe::Present(_) | Maybe::Absent(_) => return other,
        }
    };
    let made = match cx.tcx.item_name(function).as_str() {
        "new" => Constructor::New,
        "with_capacity" => Constructor::WithCapacity,
        _ => return other,
    };
    Maybe::Present((made, standard))
}
