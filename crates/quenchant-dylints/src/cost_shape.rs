//! The shared reading of the cost-shape lints: where an operation repeats,
//! what its receiver is read from, and which standard method a call resolves
//! to.
//!
//! A standard-library operation's cost depends on its receiver's type and on
//! how often it runs against the same receiver. A linear search is cheap once
//! and quadratic when a loop repeats it over a collection that outlives the
//! loop; shifting a vector's front is one move of its tail once and a
//! quadratic queue when a loop drains it.
//!
//! # Items
//!
//! Six lints refuse a shape whose cost is wrong for its receiver:
//!
//! - [`linear_search_in_loop`]: a slice, `Vec` or `VecDeque` searched linearly
//!   once per pass of a loop over a collection that outlives it;
//! - [`front_shift_in_loop`]: a `Vec`'s or `String`'s front removed or inserted
//!   once per pass of such a loop;
//! - [`reverse_after_build`]: a vector reversed right after it is built from a
//!   source that reads backward;
//! - [`count_against_constant`]: an iterator counted to its end only to compare
//!   the count with a constant;
//! - [`collect_read_once`]: a vector collected from an iterator only to be read
//!   once;
//! - [`bucket_per_key`]: a map that allocates a growable bucket for each key
//!   and only ever searches a bucket for one entry.
//!
//! Each lint is type-directed: the receiver's type decides whether the shape
//! is a hazard, and the cheap receivers — arrays, sets and maps, exact-size
//! iterators, `VecDeque`'s front — are excepted by type, never by name. Each
//! takes only what Clippy misses and names the Clippy lint beside it. Each is
//! `deny` by default: a consumer whose crate has sites `expect`s the lint on
//! that crate's root with the count, and the sites are rewritten as their own
//! work.
//!
//! The set is the census's: a candidate is linted where the workspaces that
//! load this library have sites a cheaper shape would serve. `nth` repeated in
//! a loop has none — every site indexes by a constant, reads a random-access
//! iterator or walks one node's children by arity — and gains a lint when such
//! a site appears.
//!
//! # Boundaries
//!
//! Specification clauses are not inspected. A clause states a relation and
//! runs only under enforcement; its cost is
//! [`spec_clause_traversal`](crate::clause_cost::SPEC_CLAUSE_TRAVERSAL)'s to
//! police, and a clause that searches a list per element is the relation's
//! plainest statement.
//!
//! A repetition is a `loop`, `while` or `for` body, or a closure handed to a
//! method of an iterator or a standard container, which calls it once per
//! element. A closure handed anywhere else, such as `Option::map`, runs once
//! and is read through. A `for` loop or iterator chain over an array runs a
//! number of times its type fixes, so it multiplies the cost by a constant and
//! is read through as well.
//!
//! A loop the operation leaves is read through too: an operation inside a
//! `return` or a `break`, or followed in its own block by an unconditional
//! `return` or a `break` out of the loop with no `continue` between, runs at
//! most once each time the loop is entered.
//!
//! The receiver is followed through fields, indexing, dereferences, borrows and
//! accessors that return a reference to the place it is rooted at. A local
//! bound outside the repetition is read again on every pass; one bound inside
//! is a fresh value each pass. A constant or static item has a length fixed
//! when the program is built, and a computed value is rebuilt each pass, so
//! neither makes the repeated read grow with the loop.
//!
//! [`linear_search_in_loop`]: crate::linear_search::LINEAR_SEARCH_IN_LOOP
//! [`front_shift_in_loop`]: crate::front_shift::FRONT_SHIFT_IN_LOOP
//! [`reverse_after_build`]: crate::reverse_after_build::REVERSE_AFTER_BUILD
//! [`count_against_constant`]: crate::count_comparison::COUNT_AGAINST_CONSTANT
//! [`collect_read_once`]: crate::collect_read_once::COLLECT_READ_ONCE
//! [`bucket_per_key`]: crate::bucket_per_key::BUCKET_PER_KEY

use core::ops::ControlFlow;

use anodized::spec;
use clippy_utils::consts::ConstEvalCtxt;
use clippy_utils::higher;
use clippy_utils::visitors::for_each_expr_without_closures;
use quenchant_shape::shape::Maybe;
use rustc_hir::Block;
use rustc_hir::Expr;
use rustc_hir::ExprKind;
use rustc_hir::HirId;
use rustc_hir::LoopSource;
use rustc_hir::MatchSource;
use rustc_hir::Node;
use rustc_hir::StmtKind;
use rustc_hir::UnOp;
use rustc_hir::def::DefKind;
use rustc_hir::def::Res;
use rustc_lint::LateContext;
use rustc_middle::ty;
use rustc_middle::ty::Ty;
use rustc_span::Symbol;
use rustc_span::sym;

use crate::clause_cost::ClauseRegions;
use crate::clause_cost::Enclosure;
use crate::walk_summary::container;
use crate::walk_summary::iterator;
use crate::walk_summary::standard_name;

/// Whether a cost-shape pass inspects an expression.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Inspection
{
    /// The expression is authored code outside every specification clause.
    Inspected,
    /// The expression comes from a macro expansion or sits inside an
    /// authored `spec` attribute.
    Skipped,
}

/// Decide whether a cost-shape pass inspects an expression.
///
/// # Specification
/// - ensures: `Skipped` exactly when `expr`'s span comes from a macro expansion
///   or lies inside a `spec` attribute `regions` recorded.
/// - panics: none.
/// - executable: none — the region map sits behind a lock the predicate would
///   take a second time.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures state a refused form inside a `spec` clause
///   beside the same form in a body; only the body is reported.
/// - witness: `tests::ui_cost_shape`
pub fn inspection(
    regions: &ClauseRegions,
    expr: &Expr<'_>,
) -> Inspection
{
    if expr.span.from_expansion() || regions.encloses(expr.span) == Enclosure::Inside {
        Inspection::Skipped
    }
    else {
        Inspection::Inspected
    }
}

quenchant_shape::reason_enum! {
    /// An operation can run against its receiver a number of times that does
    /// not grow with a loop.
    pub mod repetition_reading {
        /// Why the operation is not repeated against one receiver.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Unrepeated {
            /// No loop or per-element closure encloses the operation in its
            /// body.
            Straight,
            /// Every enclosing repetition walks an array or a range with
            /// constant bounds, so its trip count is fixed when the program is
            /// built.
            FixedTrip,
            /// The receiver is bound inside the innermost repetition: each pass
            /// reads a value of its own.
            BoundInside,
            /// The receiver is a constant or static item, its length fixed when
            /// the program is built.
            Constant,
            /// The receiver is computed where it is used rather than read from
            /// a place, so each pass rebuilds it.
            Temporary,
            /// An unconditional `return`, or a `break` out of the loop, follows
            /// the operation in its own block or encloses it: each entry into
            /// the loop runs the operation at most once.
            Exits,
        }
    }
}

quenchant_shape::reason_enum! {
    /// A receiver can be read from no local binding.
    pub mod root_reading {
        /// Why the receiver has no local root.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Unrooted {
            /// The place is a constant or static item.
            Constant,
            /// The receiver is a computed value, not a place.
            Temporary,
        }
    }
}

quenchant_shape::reason_enum! {
    /// A closure can be called by its callee a number of times no element
    /// count decides.
    mod closure_reading {
        /// Why the closure is not called once per element.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum NotPerElement {
            /// The closure is not an argument of a method of an iterator or a
            /// standard container: `Option::map` calls it once.
            CalledOnce,
        }
    }
}

quenchant_shape::reason_enum! {
    /// A call can resolve to something other than a standard-library method
    /// the cost-shape lints price.
    pub mod method_reading {
        /// Why the call names no priced method.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Unpriced {
            /// The call resolves to no method definition.
            Unresolved,
            /// The method belongs to a type or trait the lints do not price.
            OtherOwner,
        }
    }
}

/// The standard-library owner of a method the cost-shape lints price.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Owner
{
    /// An inherent method of `[T]`, reached by `Vec`'s and arrays' deref too.
    Slice,
    /// An inherent method of `Vec<T>`.
    Vec,
    /// An inherent method of `VecDeque<T>`.
    VecDeque,
    /// An inherent method of `String`.
    String,
    /// A method of the `Iterator` trait.
    Iterator,
}

/// A priced method: its owner and its name.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Method
{
    /// The type or trait the method is defined on.
    pub owner: Owner,
    /// The method's name.
    pub name: Symbol,
}

/// The priced standard-library method a method call resolves to.
///
/// # Specification
/// - ensures: `Present` with [`Owner::Iterator`] when the call resolves to a
///   method of the standard `Iterator` trait, and with the matching inherent
///   owner when it resolves to an inherent method of a slice, `Vec`, `VecDeque`
///   or `String`; the name is the resolved method's own.
/// - ensures: `Absent(Unresolved)` when the call has no resolved definition;
///   `Absent(OtherOwner)` for every other method, including a crate's own
///   method of the same name.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — each lint's accepted fixture calls a same-named method of
///   a local type and of a standard type the lint does not price, beside the
///   refused standard form.
/// - witness: `tests::ui_cost_shape`
#[spec(ensures: |output| match output {
    Maybe::Present(method) => cx.typeck_results().type_dependent_def_id(call.hir_id).is_some_and(|def_id| cx.tcx.item_name(def_id) == method.name),
    Maybe::Absent(_) => true,
})]
pub fn std_method(
    cx: &LateContext<'_>,
    call: &Expr<'_>,
) -> Maybe<Method, method_reading::Unpriced>
{
    let Some(def_id) = cx.typeck_results().type_dependent_def_id(call.hir_id)
    else {
        return Maybe::Absent(method_reading::Unpriced::Unresolved);
    };
    let name = cx.tcx.item_name(def_id);
    if cx
        .tcx
        .trait_of_assoc(def_id)
        .is_some_and(|owner| cx.tcx.is_diagnostic_item(sym::Iterator, owner))
    {
        return Maybe::Present(Method {
            owner: Owner::Iterator,
            name,
        });
    }
    let Some(implementation) = cx.tcx.inherent_impl_of_assoc(def_id)
    else {
        return Maybe::Absent(method_reading::Unpriced::OtherOwner);
    };
    let owner = cx
        .tcx
        .type_of(implementation)
        .instantiate_identity()
        .skip_norm_wip();
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "`TyKind` is rustc's type kind, whose variants move with the pinned nightly; \
                  only a slice or a standard ADT owns a priced method"
    )]
    let owner = match *owner.kind() {
        ty::Slice(_) => Owner::Slice,
        ty::Adt(definition, _) => match standard_name(cx.tcx, definition.did()) {
            Maybe::Present(owner) => match owner.as_str() {
                "Vec" => Owner::Vec,
                "VecDeque" => Owner::VecDeque,
                "String" => Owner::String,
                _ => return Maybe::Absent(method_reading::Unpriced::OtherOwner),
            },
            Maybe::Absent(_) => return Maybe::Absent(method_reading::Unpriced::OtherOwner),
        },
        _ => return Maybe::Absent(method_reading::Unpriced::OtherOwner),
    };
    Maybe::Present(Method { owner, name })
}

/// A repetition that reads one receiver again on every pass.
#[derive(Clone, Copy, Debug)]
#[repr(transparent)]
pub struct Repeated<'tcx>
{
    /// The loop, or the closure called once per element, that repeats the
    /// operation.
    pub scope: &'tcx Expr<'tcx>,
}

/// Whether a repetition's trip count is fixed by its iterated expression.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Trip
{
    /// The repetition walks an array or a range with constant bounds.
    Fixed,
    /// The repetition walks something whose length is known only when it runs.
    Open,
}

/// Where a local binding sits relative to a repetition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Binding
{
    /// The binding is introduced inside the repetition.
    Inside,
    /// The binding outlives the repetition.
    Outside,
}

/// The repetition that reads `receiver` again on every pass of `operation`.
///
/// # Specification
/// - ensures: `Present` with the innermost enclosing loop or per-element
///   closure of `operation`, within its body, whose trip count is open, when
///   `receiver` is rooted at a local bound outside that repetition.
/// - ensures: `Absent(Straight)` when no repetition encloses `operation`;
///   `Absent(FixedTrip)` when only fixed-trip repetitions do; `Absent(Exits)`
///   when the operation leaves every loop that encloses it; otherwise
///   `Absent(BoundInside)`, `Absent(Constant)` or `Absent(Temporary)` from the
///   receiver's root.
/// - provides: the repeated-read decision of [`linear_search_in_loop`] and
///   [`front_shift_in_loop`].
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — each repeated-read lint's fixtures refuse a `for`, a
///   `while` and a per-element closure over an outliving receiver, and accept
///   the same operation outside any loop, inside a loop over an array, on a
///   receiver bound inside the loop, on a constant and on a computed value.
/// - witness: `tests::ui_cost_shape`
///
/// [`linear_search_in_loop`]: crate::linear_search::LINEAR_SEARCH_IN_LOOP
/// [`front_shift_in_loop`]: crate::front_shift::FRONT_SHIFT_IN_LOOP
#[spec(ensures: |output| match output {
    Maybe::Present(found) => matches!(found.scope.kind, ExprKind::Loop(..) | ExprKind::Closure(..)),
    Maybe::Absent(_) => true,
})]
pub fn repeated_read<'tcx>(
    cx: &LateContext<'tcx>,
    operation: &Expr<'tcx>,
    receiver: &'tcx Expr<'tcx>,
) -> Maybe<Repeated<'tcx>, repetition_reading::Unrepeated>
{
    let scope = match repetition(cx, operation) {
        Maybe::Present(scope) => scope,
        Maybe::Absent(reason) => return Maybe::Absent(reason),
    };
    match place_root(cx, receiver) {
        Maybe::Present(local) => match binding(cx, local, scope) {
            Binding::Outside => Maybe::Present(Repeated { scope }),
            Binding::Inside => Maybe::Absent(repetition_reading::Unrepeated::BoundInside),
        },
        Maybe::Absent(root_reading::Unrooted::Constant) => {
            Maybe::Absent(repetition_reading::Unrepeated::Constant)
        },
        Maybe::Absent(root_reading::Unrooted::Temporary) => {
            Maybe::Absent(repetition_reading::Unrepeated::Temporary)
        },
    }
}

/// The innermost open-trip repetition enclosing `operation` within its body.
///
/// # Specification
/// - ensures: `Present` with the innermost `loop`, `while` or `for` expression,
///   or per-element closure, enclosing `operation` before the item that owns
///   its body, skipping those whose trip count is fixed and those the operation
///   leaves ([`exit_after`]): a `return` leaves every loop up to the closure or
///   item that owns it, a `break` every loop up to its target.
/// - ensures: `Absent(Exits)` when a loop was skipped because the operation
///   leaves it and no open repetition remains; `Absent(FixedTrip)` when every
///   enclosing repetition has a fixed trip count; `Absent(Straight)` when none
///   encloses `operation`.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures place operations in each loop form, in a
///   closure handed to an iterator adapter, in a closure handed to
///   `Option::map`, inside a loop over an array nested in an open loop, in a
///   `for` loop's iterated expression, inside a `return` in a loop, and in a
///   block a `return` or a `break` closes.
/// - witness: `tests::ui_cost_shape`
#[spec(ensures: |output| match output {
    Maybe::Present(scope) => matches!(scope.kind, ExprKind::Loop(..) | ExprKind::Closure(..)),
    Maybe::Absent(reason) => matches!(reason, repetition_reading::Unrepeated::Straight | repetition_reading::Unrepeated::FixedTrip | repetition_reading::Unrepeated::Exits),
})]
pub fn repetition<'tcx>(
    cx: &LateContext<'tcx>,
    operation: &Expr<'tcx>,
) -> Maybe<&'tcx Expr<'tcx>, repetition_reading::Unrepeated>
{
    let mut fixed = Trip::Open;
    let mut leaving = Leaving::Stays;
    let mut left = false;
    let mut child = operation.hir_id;
    for (parent_id, node) in cx.tcx.hir_parent_iter(operation.hir_id) {
        #[expect(
            clippy::wildcard_enum_match_arm,
            reason = "`Node` and `ExprKind` are rustc's, their variants moving with the pinned \
                      nightly; only a loop or a closure repeats, only a block, a `return` or a \
                      `break` leaves, and only an item ends the body"
        )]
        match node {
            Node::Block(block) => {
                if leaving == Leaving::Breaks(parent_id) {
                    leaving = Leaving::Stays;
                }
                if leaving == Leaving::Stays {
                    leaving = exit_after(block, child);
                }
            },
            Node::Expr(parent) => {
                let trip = match parent.kind {
                    ExprKind::Ret(_) => {
                        leaving = Leaving::Returns;
                        child = parent_id;
                        continue;
                    },
                    ExprKind::Break(destination, _) => {
                        if let (Leaving::Stays, Ok(target)) = (leaving, destination.target_id) {
                            leaving = Leaving::Breaks(target);
                        }
                        child = parent_id;
                        continue;
                    },
                    ExprKind::Loop(..) if leaving != Leaving::Stays => {
                        left = true;
                        if leaving == Leaving::Breaks(parent_id) {
                            leaving = Leaving::Stays;
                        }
                        child = parent_id;
                        continue;
                    },
                    ExprKind::Loop(_, _, LoopSource::ForLoop, _) => for_loop_trip(cx, parent),
                    ExprKind::Loop(..) => Trip::Open,
                    ExprKind::Closure(_) => {
                        leaving = Leaving::Stays;
                        match per_element_trip(cx, parent) {
                            Maybe::Present(trip) => trip,
                            Maybe::Absent(closure_reading::NotPerElement::CalledOnce) => {
                                child = parent_id;
                                continue;
                            },
                        }
                    },
                    _ => {
                        if leaving == Leaving::Breaks(parent_id) {
                            leaving = Leaving::Stays;
                        }
                        child = parent_id;
                        continue;
                    },
                };
                match trip {
                    Trip::Open => return Maybe::Present(parent),
                    Trip::Fixed => fixed = Trip::Fixed,
                }
            },
            Node::Item(_) | Node::ImplItem(_) | Node::TraitItem(_) | Node::ForeignItem(_) => break,
            _ => {},
        }
        child = parent_id;
    }
    Maybe::Absent(match (left, fixed) {
        (true, _) => repetition_reading::Unrepeated::Exits,
        (false, Trip::Fixed) => repetition_reading::Unrepeated::FixedTrip,
        (false, Trip::Open) => repetition_reading::Unrepeated::Straight,
    })
}

/// What an operation's enclosing code does after it, as far as loops go.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Leaving
{
    /// Nothing unconditional leaves a loop after the operation.
    Stays,
    /// A `return` follows or encloses it: every loop of the body is left.
    Returns,
    /// A `break` follows or encloses it: every loop up to the one with this id
    /// is left.
    Breaks(HirId),
}

/// Classify the statements of a block that follow one of its children.
///
/// # Specification
/// - ensures: `Returns` when a statement after the one with id `child`, or the
///   block's tail when `child` is a statement, is a `return`; `Breaks` with the
///   target when it is a `break` with a resolved target; `Stays` otherwise, a
///   nested or conditional exit included, and whenever the child statement or a
///   statement between it and the exit holds a `continue`, which repeats the
///   pass without reaching the exit.
/// - panics: none.
/// - executable: none — the predicate would repeat the statement scan the body
///   makes.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures accept a search followed by a `return` in
///   its block, and refuse one followed by a conditional `return` and one whose
///   `continue` skips the `return` after it.
/// - witness: `tests::ui_cost_shape`
fn exit_after(
    block: &Block<'_>,
    child: HirId,
) -> Leaving
{
    let Some(position) = block
        .stmts
        .iter()
        .position(|statement| statement.hir_id == child)
    else {
        return Leaving::Stays;
    };
    let continues = |expr: &Expr<'_>| {
        for_each_expr_without_closures(expr, |inner| {
            if matches!(inner.kind, ExprKind::Continue(_)) {
                ControlFlow::Break(())
            }
            else {
                ControlFlow::Continue(())
            }
        })
        .is_some()
    };
    let from_child = block
        .stmts
        .get(position ..)
        .unwrap_or_default()
        .iter()
        .filter_map(|statement| match statement.kind {
            StmtKind::Expr(expr) | StmtKind::Semi(expr) => Some(expr),
            StmtKind::Let(binding) => binding.init,
            StmtKind::Item(_) => None,
        })
        .chain(block.expr);
    for expr in from_child {
        #[expect(
            clippy::wildcard_enum_match_arm,
            reason = "`ExprKind` is rustc's, its variants moving with the pinned nightly; only a \
                      `return` or a `break` leaves unconditionally"
        )]
        match expr.kind {
            ExprKind::Ret(_) => return Leaving::Returns,
            ExprKind::Break(destination, _) => {
                return destination
                    .target_id
                    .map_or(Leaving::Stays, Leaving::Breaks);
            },
            _ => {
                if continues(expr) {
                    return Leaving::Stays;
                }
            },
        }
    }
    Leaving::Stays
}

/// The trip count of a desugared `for` loop.
///
/// # Specification
/// - requires: `body` is the `loop` expression of a desugared `for`.
/// - ensures: `Fixed` when the iterated expression walks an array or a range
///   with constant bounds ([`iteration_trip`]); `Open` otherwise, including
///   when the desugaring is not recognized.
/// - panics: none.
/// - executable: none — the predicate would repeat the parent lookup and the
///   trip query the body makes.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures accept a search inside a `for` over an array
///   and refuse it inside a `for` over a vector.
/// - witness: `tests::ui_cost_shape`
fn for_loop_trip<'tcx>(
    cx: &LateContext<'tcx>,
    body: &Expr<'tcx>,
) -> Trip
{
    let mut parents = cx.tcx.hir_parent_iter(body.hir_id);
    let _arm = parents.next();
    if let Some((_, Node::Expr(desugared))) = parents.next()
        && let ExprKind::Match(into_iter, _, MatchSource::ForLoopDesugar) = desugared.kind
        && let ExprKind::Call(_, &[ref iterated]) = into_iter.kind
    {
        return iteration_trip(cx, iterated);
    }
    Trip::Open
}

/// The trip count of a closure, when a method calls it once per element.
///
/// # Specification
/// - ensures: `Present` when `closure` is an argument of a method call whose
///   receiver, after references, is an iterator or a standard container; the
///   trip is `Fixed` when that receiver chain walks an array or a range with
///   constant bounds, `Open` otherwise.
/// - ensures: `Absent(CalledOnce)` when the closure is handed to anything else,
///   which may call it once.
/// - panics: none.
/// - executable: none — the predicate would repeat the parent lookup and the
///   trait query the body makes.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures refuse a search inside a closure handed to
///   `Iterator::filter` and to `Vec::retain`, and accept one inside a closure
///   handed to `Option::map`.
/// - witness: `tests::ui_cost_shape`
fn per_element_trip<'tcx>(
    cx: &LateContext<'tcx>,
    closure: &Expr<'tcx>,
) -> Maybe<Trip, closure_reading::NotPerElement>
{
    let Node::Expr(call) = cx.tcx.parent_hir_node(closure.hir_id)
    else {
        return Maybe::Absent(closure_reading::NotPerElement::CalledOnce);
    };
    let ExprKind::MethodCall(_, receiver, arguments, _) = call.kind
    else {
        return Maybe::Absent(closure_reading::NotPerElement::CalledOnce);
    };
    if !arguments
        .iter()
        .any(|argument| argument.hir_id == closure.hir_id)
    {
        return Maybe::Absent(closure_reading::NotPerElement::CalledOnce);
    }
    let held = cx.typeck_results().expr_ty_adjusted(receiver).peel_refs();
    if matches!(iterator(cx.tcx, cx.typing_env(), held), Maybe::Present(()))
        || matches!(container(cx.tcx, held), Maybe::Present(_))
    {
        Maybe::Present(iteration_trip(cx, receiver))
    }
    else {
        Maybe::Absent(closure_reading::NotPerElement::CalledOnce)
    }
}

/// Whether an iterated expression walks a number of elements fixed when the
/// program is built.
///
/// # Specification
/// - ensures: `Fixed` when `iterated`, or the base of the chain of iterator
///   adapters and `iter` calls it is built from, is an array, a reference to
///   one, or a range whose bounds are constants; `Open` otherwise.
/// - panics: none.
/// - executable: none — the predicate would repeat the walk down the receiver
///   chain the body makes.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures accept searches inside a `for` over an
///   array, a `for` over a range with literal bounds and an iterator chain
///   started from an array.
/// - witness: `tests::ui_cost_shape`
pub fn iteration_trip<'tcx>(
    cx: &LateContext<'tcx>,
    iterated: &Expr<'tcx>,
) -> Trip
{
    let mut current = iterated;
    loop {
        let held = cx.typeck_results().expr_ty(current).peel_refs();
        if held.is_array() {
            return Trip::Fixed;
        }
        if let Some(range) = higher::Range::hir(cx, current) {
            let constant = ConstEvalCtxt::new(cx);
            return match (range.start, range.end) {
                (Some(start), Some(end))
                    if constant.eval(start).is_some() && constant.eval(end).is_some() =>
                {
                    Trip::Fixed
                },
                _ => Trip::Open,
            };
        }
        #[expect(
            clippy::wildcard_enum_match_arm,
            reason = "`ExprKind` is rustc's, its variants moving with the pinned nightly; only an \
                      adapter or a borrow stands between a loop and the iterator it counts"
        )]
        match current.kind {
            ExprKind::MethodCall(_, receiver, ..)
                if matches!(iterator(cx.tcx, cx.typing_env(), held), Maybe::Present(())) =>
            {
                current = receiver;
            },
            ExprKind::AddrOf(_, _, inner) => current = inner,
            _ => return Trip::Open,
        }
    }
}

/// The local binding a place is rooted at.
///
/// # Specification
/// - ensures: follows fields, indexing, dereferences, borrows and argument-free
///   method calls that return a reference; `Present` with the local at the
///   root, `Absent(Constant)` for a constant or static item, and
///   `Absent(Temporary)` for any other root.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures read receivers through a field of `self`,
///   through an accessor returning a slice, through a borrow, from a constant
///   and from a function's result.
/// - witness: `tests::ui_cost_shape`
#[spec(ensures: |output| !matches!(place.kind, ExprKind::Lit(_) | ExprKind::Call(..)) || matches!(output, Maybe::Absent(root_reading::Unrooted::Temporary)))]
pub fn place_root<'tcx>(
    cx: &LateContext<'tcx>,
    place: &'tcx Expr<'tcx>,
) -> Maybe<HirId, root_reading::Unrooted>
{
    let mut current = place;
    loop {
        #[expect(
            clippy::wildcard_enum_match_arm,
            reason = "`ExprKind` and `Res` are rustc's, their variants moving with the pinned \
                      nightly; only a projection, a borrow, a reference accessor or a path leads \
                      to a root"
        )]
        match current.kind {
            ExprKind::Field(base, _)
            | ExprKind::Index(base, ..)
            | ExprKind::Unary(UnOp::Deref, base)
            | ExprKind::AddrOf(_, _, base) => current = base,
            ExprKind::MethodCall(_, receiver, &[], _)
                if cx.typeck_results().expr_ty(current).is_ref() =>
            {
                current = receiver;
            },
            ExprKind::Path(ref path) => {
                return match cx.qpath_res(path, current.hir_id) {
                    Res::Local(local) => Maybe::Present(local),
                    Res::Def(
                        DefKind::Const { .. } | DefKind::AssocConst { .. } | DefKind::Static { .. },
                        _,
                    ) => Maybe::Absent(root_reading::Unrooted::Constant),
                    _ => Maybe::Absent(root_reading::Unrooted::Temporary),
                };
            },
            _ => return Maybe::Absent(root_reading::Unrooted::Temporary),
        }
    }
}

/// Whether a local binding is introduced inside a repetition.
///
/// # Specification
/// - ensures: `Inside` exactly when `scope` is an ancestor of the binding's
///   pattern, which holds for a closure's own parameters; `Outside` otherwise.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures accept a search on a vector bound in the
///   loop body and on a closure parameter, and refuse one on a vector bound
///   before the loop.
/// - witness: `tests::ui_cost_shape`
#[spec(ensures: |output| (output == Binding::Inside) == cx.tcx.hir_parent_id_iter(local).any(|parent| parent == scope.hir_id))]
pub fn binding(
    cx: &LateContext<'_>,
    local: HirId,
    scope: &Expr<'_>,
) -> Binding
{
    if cx
        .tcx
        .hir_parent_id_iter(local)
        .any(|parent| parent == scope.hir_id)
    {
        Binding::Inside
    }
    else {
        Binding::Outside
    }
}

/// Whether a type, after references, has a length fixed by the type itself.
///
/// # Specification
/// - ensures: `Fixed` exactly when `held`, after references, is an array.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures accept each lint's operation on an array
///   beside the refused vector form.
/// - witness: `tests::ui_cost_shape`
#[spec(ensures: |output| (output == Trip::Fixed) == held.peel_refs().is_array())]
pub fn length_trip(held: Ty<'_>) -> Trip
{
    if held.peel_refs().is_array() {
        Trip::Fixed
    }
    else {
        Trip::Open
    }
}

quenchant_shape::reason_enum! {
    /// A statement can be other than one of a block's statements.
    pub mod statement_reading {
        /// Why the expression has no statement position.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum NotAStatement {
            /// No block statement or block tail holds the expression.
            Unplaced,
        }
    }
}

/// A statement's place in its block.
#[derive(Clone, Copy, Debug)]
pub struct Placement<'tcx>
{
    /// The block that holds the statement.
    pub block: &'tcx Block<'tcx>,
    /// The statement's index; the block's tail expression is at
    /// `stmts.len()`.
    pub index: usize,
}

/// The block and statement index that hold an expression.
///
/// # Specification
/// - ensures: `Present` with the innermost block and the index of its
///   statement, or of its tail, that contains `hir_id`; `Absent` when none does
///   before an item.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures reverse as a statement, inside an `if let`
///   statement and after a `let` of the suffix, and read a collection once
///   after its `let`.
/// - witness: `tests::ui_cost_shape`
#[spec(ensures: |output| match output {
    Maybe::Present(placement) => placement.index <= placement.block.stmts.len(),
    Maybe::Absent(_) => true,
})]
pub fn enclosing_statement<'tcx>(
    cx: &LateContext<'tcx>,
    hir_id: HirId,
) -> Maybe<Placement<'tcx>, statement_reading::NotAStatement>
{
    let mut child = hir_id;
    for (parent_id, node) in cx.tcx.hir_parent_iter(hir_id) {
        #[expect(
            clippy::wildcard_enum_match_arm,
            reason = "`Node` is rustc's, its variants moving with the pinned nightly; only a block \
                      places a statement, and only an item ends the search"
        )]
        match node {
            Node::Block(block) => {
                if let Some(index) = block
                    .stmts
                    .iter()
                    .position(|statement| statement.hir_id == child)
                {
                    return Maybe::Present(Placement { block, index });
                }
                if block.expr.is_some_and(|tail| tail.hir_id == child) {
                    return Maybe::Present(Placement {
                        block,
                        index: block.stmts.len(),
                    });
                }
            },
            Node::Item(_) | Node::ImplItem(_) | Node::TraitItem(_) => break,
            _ => {},
        }
        child = parent_id;
    }
    Maybe::Absent(statement_reading::NotAStatement::Unplaced)
}
