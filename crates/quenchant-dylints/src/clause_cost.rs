//! Specification clauses priced against the function they check.
//!
//! An enforcing build evaluates every executable clause on every call. A clause
//! that walks state reached through the receiver or a `&mut` parameter costs in
//! proportion to that state, which grows across calls or dwarfs the function
//! itself, and the checked code turns quadratic. This gate finds such walks in
//! a clause's own expression: a method rooted at the receiver or at a mutable
//! parameter that starts an iterator, searches or order-checks a sequence,
//! copies a collection, or compares collections.
//!
//! Clauses are located by source. A pre-expansion pass records the span of
//! every authored `spec` attribute; the late pass reads the typed expressions
//! those spans enclose, which is where the attribute's predicates land after
//! expansion. Calls into other functions are not followed: a helper that checks
//! only what changed since a captured point is how a clause states a claim
//! about accumulated state within its cost.

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;

use anodized::spec;
use clippy_utils::diagnostics::span_lint_and_help;
use quenchant_shape::shape::Maybe;
use rustc_ast::Attribute;
use rustc_hir::BinOpKind;
use rustc_hir::Expr;
use rustc_hir::ExprKind;
use rustc_hir::LangItem;
use rustc_hir::Mutability;
use rustc_hir::QPath;
use rustc_hir::UnOp;
use rustc_hir::def::Res;
use rustc_lint::EarlyContext;
use rustc_lint::EarlyLintPass;
use rustc_lint::LateContext;
use rustc_lint::LateLintPass;
use rustc_middle::ty;
use rustc_middle::ty::Ty;
use rustc_session::declare_lint;
use rustc_session::impl_lint_pass;
use rustc_session::lint::Level;
use rustc_span::BytePos;
use rustc_span::Span;
use rustc_span::Symbol;
use rustc_span::sym;
use rustc_span::symbol::kw;

use crate::executable::spec_arguments;

quenchant_shape::reason_enum! {
    /// An expression can leave the state behind a clause untouched.
    mod walk_reading {
        /// Why the expression is not a walk over accumulated state.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum NotAWalk {
            /// The expression is not rooted at the receiver or a mutable
            /// parameter.
            Unrooted,
            /// The expression reads a bounded amount of what it reaches.
            Bounded,
        }
    }
}

quenchant_shape::reason_enum! {
    /// An expression can reach something other than accumulated state.
    mod root_reading {
        /// Why the expression has no state root.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Unrooted {
            /// The innermost place is a local that is neither the receiver nor
            /// a mutable reference, or no place at all.
            OtherBase,
        }
    }
}

quenchant_shape::reason_enum! {
    /// A type can hold a bounded number of elements.
    mod container_reading {
        /// Why the type is not a growable container.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Bounded {
            /// The type is not one of the standard sequences or collections.
            NotAContainer,
        }
    }
}

quenchant_shape::reason_enum! {
    /// A type can be something other than an iterator.
    mod iterator_reading {
        /// Why the type is not an iterator.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum NotAnIterator {
            /// The type does not implement `Iterator`, or the trait is not
            /// resolvable.
            NoImplementation,
        }
    }
}

declare_lint! {
    /// ### What it does
    ///
    /// Refuses a `#[spec]` clause that walks state reached through `self` or through a `&mut` parameter: a method rooted there that starts an iterator, a linear `contains` or `is_sorted*` over a sequence, a copy of a collection, or a comparison of collections. Fixed-size arrays are bounded and pass; so do `len`, `get`, `first`, `last`, `binary_search` and keyed map lookups.
    ///
    /// ### Why is this bad?
    ///
    /// An enforcing build evaluates the clause on every call. Walking state that grows across calls makes the checked code quadratic, and walking a large table from a constant-time query makes the check dwarf the function. A specification check should cost no more than the function it checks: walk the inputs and the output, check a structural invariant once where the state is built, and check accumulated state only through a helper that reads what changed since a captured point.
    ///
    /// ### What it does not decide
    ///
    /// Only the clause's own expression is read; a helper the clause calls is not followed, and is where a since-a-mark check belongs. Whether a walk is cheap because the state is small in practice is the exception's reason to state.
    ///
    /// ### Activation
    ///
    /// The lint is `allow` by default. A crate opts in at its root with `#![cfg_attr(dylint_lib = "quenchant_dylints", deny(spec_clause_traversal))]`; an exception is `#[expect(spec_clause_traversal, reason = "...")]` on the function.
    ///
    /// ### Example
    ///
    /// ```rust,ignore
    /// #[spec(ensures: |ret| ret == self.stack.iter().rposition(|cell| cell.open))]
    /// fn nearest_open(&self) -> Option<usize> { self.frontiers.last().copied() }
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// #[spec(ensures: |ret| ret.is_none_or(|index| self.stack.get(index).is_some_and(|cell| cell.open)))]
    /// fn nearest_open(&self) -> Option<usize> { self.frontiers.last().copied() }
    /// ```
    pub SPEC_CLAUSE_TRAVERSAL,
    Allow,
    "a specification clause walks state reached through the receiver or a mutable parameter"
}

/// Denial help naming the admitted ways to state the claim within its cost.
const TRAVERSAL_HELP: &str = concat!(
    "check the claim once where the state is built, check only what the call returns, ",
    "or call a helper that reads what changed since a captured point; ",
    "an exception is `#[expect(spec_clause_traversal, reason = \"...\")]`"
);

/// Denial for a clause that starts an iterator over accumulated state.
const ITERATES_MESSAGE: &str = "this specification clause iterates over state reached through the receiver or a mutable parameter";

/// Denial for a clause that searches or order-checks a sequence.
const SEARCHES_MESSAGE: &str =
    "this specification clause searches state reached through the receiver or a mutable parameter";

/// Denial for a clause that copies a container.
const COPIES_MESSAGE: &str =
    "this specification clause copies state reached through the receiver or a mutable parameter";

/// Denial for a clause that compares containers element by element.
const COMPARES_MESSAGE: &str =
    "this specification clause compares state reached through the receiver or a mutable parameter";

/// Whether a span lies inside an authored `spec` attribute.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Enclosure
{
    /// The span is inside a recorded attribute.
    Inside,
    /// The span is outside every recorded attribute.
    Outside,
}

/// The source ranges of the authored `spec` attributes, by start position.
#[derive(Clone, Default)]
#[repr(transparent)]
pub struct ClauseRegions(Arc<Mutex<BTreeMap<BytePos, BytePos>>>);

impl ClauseRegions
{
    /// Record one attribute's source range.
    ///
    /// # Specification
    /// - ensures: [`encloses`](Self::encloses) answers [`Enclosure::Inside`]
    ///   for `span` afterwards.
    /// - panics: none; a poisoned lock is recovered, since every write leaves
    ///   the map consistent.
    /// - executable: none — the outcome is the shared map, read through
    ///   [`encloses`](Self::encloses) by the late pass of the same compilation.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the UI fixture's refused clauses are reported only
    ///   because their attributes were recorded, and its bodies, which walk the
    ///   same state outside any attribute, are not.
    /// - witness: `tests::ui_spec_cost`
    fn record(
        &self,
        span: Span,
    )
    {
        let hi = span.hi();
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(span.lo())
            .and_modify(|end| *end = (*end).max(hi))
            .or_insert(hi);
    }

    /// Whether `span` lies inside a recorded attribute.
    ///
    /// # Specification
    /// - ensures: [`Enclosure::Inside`] exactly when some recorded range
    ///   starting at or before `span` ends at or after it.
    /// - panics: none; a poisoned lock is recovered.
    /// - executable: none — the map sits behind a lock the predicate would take
    ///   a second time.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the UI fixture walks the same state in a clause, in a
    ///   body and in a helper called from a clause; only the clause is
    ///   reported.
    /// - witness: `tests::ui_spec_cost`
    fn encloses(
        &self,
        span: Span,
    ) -> Enclosure
    {
        let regions = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let inside = regions
            .range(..= span.lo())
            .next_back()
            .is_some_and(|(_, &end)| span.hi() <= end);
        if inside {
            Enclosure::Inside
        }
        else {
            Enclosure::Outside
        }
    }
}

/// Pre-expansion recorder of the authored `spec` attributes.
#[repr(transparent)]
pub struct ClauseCollector
{
    /// Where the ranges are shared with the late pass.
    regions: ClauseRegions,
}

impl ClauseCollector
{
    /// The collector writes into the regions its late reader holds.
    ///
    /// # Specification
    /// trivial.
    #[must_use]
    pub const fn new(regions: ClauseRegions) -> Self
    {
        Self { regions }
    }
}

impl_lint_pass!(ClauseCollector => []);

impl EarlyLintPass for ClauseCollector
{
    /// Record an attribute that writes a `spec` attribute.
    ///
    /// # Specification
    /// - ensures: records the attribute's span when [`spec_arguments`] finds a
    ///   `spec` attribute in it, directly or under `cfg_attr`; ignores every
    ///   other attribute.
    /// - panics: none.
    /// - executable: none — the outcome is the shared map, observed through the
    ///   late pass's reports.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the UI fixture states clauses directly, qualified and
    ///   under `cfg_attr`, on free functions and methods; a missed form loses
    ///   its report.
    /// - witness: `tests::ui_spec_cost`
    fn check_attribute(
        &mut self,
        _cx: &EarlyContext<'_>,
        attr: &Attribute,
    )
    {
        if !spec_arguments(attr).is_empty() {
            self.regions.record(attr.span);
        }
    }
}

/// Late reader of the expressions inside authored `spec` attributes.
#[repr(transparent)]
pub struct SpecClauseTraversal
{
    /// The ranges the pre-expansion pass recorded.
    regions: ClauseRegions,
}

impl SpecClauseTraversal
{
    /// The late pass reads the regions its collector writes.
    ///
    /// # Specification
    /// trivial.
    #[must_use]
    pub const fn new(regions: ClauseRegions) -> Self
    {
        Self { regions }
    }
}

impl_lint_pass!(SpecClauseTraversal => [SPEC_CLAUSE_TRAVERSAL]);

impl<'tcx> LateLintPass<'tcx> for SpecClauseTraversal
{
    /// Report a walk over accumulated state inside a specification clause.
    ///
    /// # Specification
    /// - ensures: where the lint is not allowed at `expr`, reports `expr` at
    ///   its source call site when that site lies inside an authored `spec`
    ///   attribute and [`walk`] finds a walk; reports nothing otherwise.
    /// - panics: none.
    /// - executable: none — rustc emits diagnostics without a queryable
    ///   per-call diagnostic result.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the UI fixture pairs each refused walk with a bounded
    ///   read of the same state, the same walk over an input, in a body and in
    ///   a helper, and an exception with its reason.
    /// - witness: `tests::ui_spec_cost`
    fn check_expr(
        &mut self,
        cx: &LateContext<'tcx>,
        expr: &'tcx Expr<'tcx>,
    )
    {
        if cx
            .tcx
            .lint_level_spec_at_node(SPEC_CLAUSE_TRAVERSAL, expr.hir_id)
            .level()
            == Level::Allow
        {
            return;
        }
        let site = expr.span.source_callsite();
        if self.regions.encloses(site) == Enclosure::Outside {
            return;
        }
        if let Maybe::Present(found) = walk(cx, expr) {
            let message = match found {
                | Walk::Iterates => ITERATES_MESSAGE,
                | Walk::Searches => SEARCHES_MESSAGE,
                | Walk::Copies => COPIES_MESSAGE,
                | Walk::Compares => COMPARES_MESSAGE,
            };
            span_lint_and_help(
                cx,
                SPEC_CLAUSE_TRAVERSAL,
                site,
                message,
                None,
                TRAVERSAL_HELP,
            );
        }
    }
}

/// How a clause walks the state it reaches.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Walk
{
    /// A method starts an iterator over the state.
    Iterates,
    /// A sequence is searched or order-checked element by element.
    Searches,
    /// A collection is copied.
    Copies,
    /// Two collections are compared element by element.
    Compares,
}

/// Find the walk an expression makes over accumulated state.
///
/// # Specification
/// - ensures: a method call rooted, by [`state_root`], at the receiver or a
///   mutable parameter is [`Walk::Iterates`] when it turns a receiver that is
///   not an iterator into one; [`Walk::Searches`] for `contains` and the
///   `is_sorted` family on a sequence; [`Walk::Copies`] for `clone`, `to_owned`
///   and `to_vec` of a container; [`Walk::Compares`] for `eq` and `ne` on a
///   container. A comparison operator is [`Walk::Compares`] when either operand
///   is so rooted and the left is a container. A call of
///   `IntoIterator::into_iter`, which a `for` loop desugars to, is
///   [`Walk::Iterates`] when its argument is so rooted and not already an
///   iterator. A fixed-size array receiver is bounded; every other expression
///   is `Absent`.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the UI fixture states each walk once on the receiver and
///   once on a mutable parameter, beside the same method on an input, on a
///   keyed map, on an array and on the clause's own output, which pass.
/// - witness: `tests::ui_spec_cost`
#[spec(ensures: |output| match output {
    Maybe::Present(_) => matches!(expr.kind, ExprKind::MethodCall(..) | ExprKind::Binary(..) | ExprKind::Call(..)),
    Maybe::Absent(_) => true,
})]
fn walk<'tcx>(
    cx: &LateContext<'tcx>,
    expr: &'tcx Expr<'tcx>,
) -> Maybe<Walk, walk_reading::NotAWalk>
{
    let typeck = cx.typeck_results();
    match expr.kind {
        | ExprKind::MethodCall(segment, receiver, ..) => {
            if !matches!(state_root(cx, receiver), Maybe::Present(())) {
                return Maybe::Absent(walk_reading::NotAWalk::Unrooted);
            }
            // Autoref may coerce an array receiver to a slice; the array is
            // what the state holds.
            let held = typeck.expr_ty_adjusted(receiver).peel_refs();
            let fixed = typeck.expr_ty(receiver).peel_refs().is_array();
            if fixed || matches!(iterator(cx, held), Maybe::Present(())) {
                return Maybe::Absent(walk_reading::NotAWalk::Bounded);
            }
            if matches!(iterator(cx, typeck.expr_ty(expr)), Maybe::Present(())) {
                return Maybe::Present(Walk::Iterates);
            }
            let Maybe::Present(kind) = container(cx, held)
            else {
                return Maybe::Absent(walk_reading::NotAWalk::Bounded);
            };
            match (kind, segment.ident.name.as_str()) {
                | (
                    Container::Sequence,
                    "contains" | "is_sorted" | "is_sorted_by" | "is_sorted_by_key",
                ) => Maybe::Present(Walk::Searches),
                | (_, "clone" | "to_owned" | "to_vec") => Maybe::Present(Walk::Copies),
                | (_, "eq" | "ne") => Maybe::Present(Walk::Compares),
                | _ => Maybe::Absent(walk_reading::NotAWalk::Bounded),
            }
        },
        | ExprKind::Binary(op, left, right) => {
            let comparison = matches!(
                op.node,
                BinOpKind::Eq
                    | BinOpKind::Ne
                    | BinOpKind::Lt
                    | BinOpKind::Le
                    | BinOpKind::Gt
                    | BinOpKind::Ge
            );
            let rooted = matches!(state_root(cx, left), Maybe::Present(()))
                || matches!(state_root(cx, right), Maybe::Present(()));
            let held = typeck.expr_ty(left).peel_refs();
            if comparison
                && rooted
                && !held.is_array()
                && matches!(container(cx, held), Maybe::Present(_))
            {
                Maybe::Present(Walk::Compares)
            }
            else {
                Maybe::Absent(walk_reading::NotAWalk::Bounded)
            }
        },
        | ExprKind::Call(callee, &[ref argument]) => {
            let into_iter = matches!(callee.kind, ExprKind::Path(ref path)
                if matches!(typeck.qpath_res(path, callee.hir_id), Res::Def(_, id) if cx.tcx.is_lang_item(id, LangItem::IntoIterIntoIter)));
            if !into_iter || !matches!(state_root(cx, argument), Maybe::Present(())) {
                return Maybe::Absent(walk_reading::NotAWalk::Unrooted);
            }
            let held = typeck.expr_ty(argument).peel_refs();
            if held.is_array() || matches!(iterator(cx, held), Maybe::Present(())) {
                Maybe::Absent(walk_reading::NotAWalk::Bounded)
            }
            else {
                Maybe::Present(Walk::Iterates)
            }
        },
        | _ => Maybe::Absent(walk_reading::NotAWalk::Unrooted),
    }
}

/// Whether an expression's innermost place is the receiver or a mutable
/// parameter.
///
/// # Specification
/// - ensures: follows field projections, dereferences, borrows, indexing and
///   method receivers inward to the innermost expression; `Present` when that
///   is a path to the local `self`, or to a local whose type is a mutable
///   reference; `Absent` for any other innermost expression.
/// - panics: none.
/// - executable: none — the outcome is decided by walking the HIR place inward;
///   a predicate could only repeat that walk.
///
/// # Adequacy
/// - hypothesis: L3 — the UI fixture reaches state through fields, method
///   results and borrows of the receiver and of a mutable parameter, beside the
///   same chains on a shared parameter and on the clause's output.
/// - witness: `tests::ui_spec_cost`
fn state_root(
    cx: &LateContext<'_>,
    expr: &Expr<'_>,
) -> Maybe<(), root_reading::Unrooted>
{
    let mut current = expr;
    loop {
        current = match current.kind {
            | ExprKind::Field(base, _)
            | ExprKind::Unary(UnOp::Deref, base)
            | ExprKind::AddrOf(_, _, base)
            | ExprKind::Index(base, ..)
            | ExprKind::MethodCall(_, base, ..) => base,
            | ExprKind::Path(QPath::Resolved(None, path)) => {
                let Res::Local(_) = path.res
                else {
                    return Maybe::Absent(root_reading::Unrooted::OtherBase);
                };
                let receiver = path
                    .segments
                    .last()
                    .is_some_and(|segment| segment.ident.name == kw::SelfLower);
                let mutable = matches!(
                    *cx.typeck_results().expr_ty(current).kind(),
                    ty::Ref(_, _, Mutability::Mut)
                );
                return if receiver || mutable {
                    Maybe::Present(())
                }
                else {
                    Maybe::Absent(root_reading::Unrooted::OtherBase)
                };
            },
            | _ => return Maybe::Absent(root_reading::Unrooted::OtherBase),
        };
    }
}

/// Whether a type implements `Iterator`.
///
/// # Specification
/// - ensures: `Present` exactly when the standard `Iterator` trait resolves and
///   the type implements it.
/// - panics: none.
/// - executable: none — the predicate would repeat the one trait query the body
///   makes.
///
/// # Adequacy
/// - hypothesis: L3 — the UI fixture chains adapters after a refused `iter()`,
///   which report once, at the call that starts the iterator.
/// - witness: `tests::ui_spec_cost`
fn iterator<'tcx>(
    cx: &LateContext<'tcx>,
    held: Ty<'tcx>,
) -> Maybe<(), iterator_reading::NotAnIterator>
{
    let implements = cx
        .tcx
        .get_diagnostic_item(sym::Iterator)
        .is_some_and(|trait_id| clippy_utils::ty::implements_trait(cx, held, trait_id, &[]));
    if implements {
        Maybe::Present(())
    }
    else {
        Maybe::Absent(iterator_reading::NotAnIterator::NoImplementation)
    }
}

/// Which kind of growable container a type is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Container
{
    /// A slice, `str`, `Vec`, `VecDeque` or `String`: searched element by
    /// element.
    Sequence,
    /// A standard map, set, heap or list: searched by key, if at all.
    Collection,
}

/// Classify a type as a growable container.
///
/// # Specification
/// - ensures: [`Container::Sequence`] for slices, `str`, `Vec`, `VecDeque` and
///   `String`; [`Container::Collection`] for `BTreeMap`, `BTreeSet`, `HashMap`,
///   `HashSet`, `BinaryHeap` and `LinkedList`; `Absent` otherwise, arrays
///   included.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the UI fixture searches a vector and a set, copies a
///   vector and a map, and compares vectors and arrays; only the growable forms
///   are reported.
/// - witness: `tests::ui_spec_cost`
#[spec(ensures: |output| match output {
    Maybe::Present(_) => !held.is_array(),
    Maybe::Absent(_) => true,
})]
fn container<'tcx>(
    cx: &LateContext<'tcx>,
    held: Ty<'tcx>,
) -> Maybe<Container, container_reading::Bounded>
{
    if held.is_slice() || held.is_str() {
        return Maybe::Present(Container::Sequence);
    }
    let ty::Adt(definition, _) = *held.kind()
    else {
        return Maybe::Absent(container_reading::Bounded::NotAContainer);
    };
    let name = cx.tcx.get_diagnostic_name(definition.did());
    match name.as_ref().map(Symbol::as_str) {
        | Some("Vec" | "VecDeque" | "String") => Maybe::Present(Container::Sequence),
        | Some("BTreeMap" | "BTreeSet" | "HashMap" | "HashSet" | "BinaryHeap" | "LinkedList") => {
            Maybe::Present(Container::Collection)
        },
        | _ => Maybe::Absent(container_reading::Bounded::NotAContainer),
    }
}
