//! Specification clauses priced against the function they check.
//!
//! An enforcing build evaluates every executable clause on every call. A clause
//! that walks state reached through the receiver or a `&mut` parameter costs in
//! proportion to that state, which grows across calls or dwarfs the function
//! itself, and the checked code turns quadratic. This gate finds such walks in
//! a clause: a standard-library method rooted at the receiver or at a mutable
//! parameter that starts an iterator, searches or order-checks a sequence,
//! copies a collection, or compares collections; and a call that hands such
//! state to a function whose body, followed through its own calls, does one of
//! those (see [`crate::walk_summary`]).
//!
//! Clauses are located by source. A pre-expansion pass records the span of
//! every authored `spec` attribute; the late pass reads the typed expressions
//! those spans enclose, which is where the attribute's predicates land after
//! expansion. Following stops at a function whose rustdoc states its bound in a
//! `# Clause cost` section: that is how a combinator that checks only what
//! changed since a captured point declares itself.

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;

use anodized::spec;
use clippy_utils::diagnostics::span_lint_and_help;
use clippy_utils::diagnostics::span_lint_and_then;
use quenchant_shape::shape::Maybe;
use rustc_ast::Attribute;
use rustc_hir::BinOpKind;
use rustc_hir::Body;
use rustc_hir::Expr;
use rustc_hir::ExprKind;
use rustc_hir::FnDecl;
use rustc_hir::LangItem;
use rustc_hir::Mutability;
use rustc_hir::QPath;
use rustc_hir::TraitFn;
use rustc_hir::TraitItem;
use rustc_hir::TraitItemKind;
use rustc_hir::UnOp;
use rustc_hir::def::DefKind;
use rustc_hir::def::Res;
use rustc_hir::def_id::DefId;
use rustc_hir::def_id::LocalDefId;
use rustc_hir::intravisit::FnKind;
use rustc_lint::EarlyContext;
use rustc_lint::EarlyLintPass;
use rustc_lint::LateContext;
use rustc_lint::LateLintPass;
use rustc_middle::ty;
use rustc_middle::ty::GenericArgsRef;
use rustc_session::declare_lint;
use rustc_session::impl_lint_pass;
use rustc_session::lint::Level;
use rustc_span::BytePos;
use rustc_span::Span;
use rustc_span::sym;
use rustc_span::symbol::kw;

use crate::executable::spec_arguments;
use crate::walk_summary::Callee;
use crate::walk_summary::Section;
use crate::walk_summary::Summaries;
use crate::walk_summary::Walk;
use crate::walk_summary::classify;
use crate::walk_summary::container;
use crate::walk_summary::iterator;
use crate::walk_summary::library_walk;
use crate::walk_summary::section;

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
    /// An expression can be something other than a call of a function.
    mod call_reading {
        /// Why the expression has no callee to follow.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum NotACall {
            /// The expression is not a method call, a call of a function path,
            /// or an equality comparison.
            OtherExpression,
            /// The callee is a constructor, a closure or a function pointer.
            NotAFunction,
        }
    }
}

declare_lint! {
    /// ### What it does
    ///
    /// Refuses a `#[spec]` clause that walks state reached through `self` or through a `&mut` parameter: a standard-library method rooted there that starts an iterator, a linear `contains` or `is_sorted*` over a sequence, a copy of a collection, or a comparison of collections. A call that hands such state to a function is followed into that function's body, and through every call it makes, across crates. Fixed-size arrays are bounded and pass; so do `len`, `get`, `first`, `last`, `binary_search` and keyed map lookups.
    ///
    /// ### Why is this bad?
    ///
    /// An enforcing build evaluates the clause on every call. Walking state that grows across calls makes the checked code quadratic, and walking a large table from a constant-time query makes the check dwarf the function. A specification check should cost no more than the function it checks: walk the inputs and the output, check a structural invariant once where the state is built, and check accumulated state only through a combinator that reads what changed since a captured point.
    ///
    /// ### Declaring a bound
    ///
    /// Following stops at a function whose rustdoc has a `# Clause cost` section with a `- bound:` clause stating why a clause may call it on accumulated state. A heading without that clause is refused at the function. A trait method that resolves to no single body, a `dyn` call for instance, is refused unless its declaration states the bound. Bodies from other crates are read from their metadata, so the whole graph must be built with `-Zalways-encode-mir` and `-Zinline-mir=no`; a body the metadata does not carry is refused with that remedy.
    ///
    /// ### What it does not decide
    ///
    /// Whether a walk is cheap because the state is small in practice is the exception's reason to state.
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
    "or call a combinator whose `# Clause cost` section states its bound; ",
    "an exception is `#[expect(spec_clause_traversal, reason = \"...\")]`"
);

/// Denial help for state handed to a call with no single body.
const UNRESOLVED_HELP: &str = concat!(
    "state the bound every implementation keeps in a `# Clause cost` section with a `- bound:` ",
    "clause on the trait method, or call a combinator that states it"
);

/// Denial help for a body the dependency metadata does not carry.
const UNREADABLE_HELP: &str = concat!(
    "build the whole dependency graph with `-Zalways-encode-mir -Zinline-mir=no`, ",
    "as the policy task does, so that every body can be followed"
);

/// Denial help for a `# Clause cost` heading without its bound.
const SECTION_HELP: &str = "state the bound below the heading as `- bound: <why a clause may call this on accumulated state>`";

/// Denial for a `# Clause cost` heading without its bound.
const SECTION_MESSAGE: &str = "this `# Clause cost` section states no `- bound:`";

/// The note that points at the walk inside a followed body.
const WITNESS_NOTE: &str = "the walk is here";

/// What a clause or a followed body does to the state it walks.
const ITERATES_TAIL: &str =
    "iterates over state reached through the receiver or a mutable parameter";

/// What a clause or a followed body does when it searches a sequence.
const SEARCHES_TAIL: &str = "searches state reached through the receiver or a mutable parameter";

/// What a clause or a followed body does when it copies a container.
const COPIES_TAIL: &str = "copies state reached through the receiver or a mutable parameter";

/// What a clause or a followed body does when it compares containers.
const COMPARES_TAIL: &str = "compares state reached through the receiver or a mutable parameter";

/// What a followed body does when it hands the state to an unresolved call.
const UNRESOLVED_TAIL: &str = "hands state reached through the receiver or a mutable parameter to a call that resolves to no single body";

/// What a followed body does when it hands the state to an unreadable body.
const UNREADABLE_TAIL: &str = "hands state reached through the receiver or a mutable parameter to a body this build cannot read";

/// What a clause does when it hands rooted state to a call it cannot follow.
const HANDED_PREFIX: &str = "hands state reached through the receiver or a mutable parameter to";

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
    /// - hypothesis: L3 — the UI fixture walks the same state in a clause and
    ///   in a body; only the clause is reported.
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
pub struct SpecClauseTraversal
{
    /// The ranges the pre-expansion pass recorded.
    regions: ClauseRegions,
    /// The bodies summarized so far in this compilation.
    summaries: Summaries,
}

impl SpecClauseTraversal
{
    /// The late pass reads the regions its collector writes.
    ///
    /// # Specification
    /// trivial.
    #[must_use]
    pub fn new(regions: ClauseRegions) -> Self
    {
        Self {
            regions,
            summaries: Summaries::default(),
        }
    }
}

impl_lint_pass!(SpecClauseTraversal => [SPEC_CLAUSE_TRAVERSAL]);

impl<'tcx> LateLintPass<'tcx> for SpecClauseTraversal
{
    /// Report a walk over accumulated state inside a specification clause.
    ///
    /// # Specification
    /// - ensures: where the lint is not allowed at `expr` and `expr`'s source
    ///   call site lies inside an authored `spec` attribute, reports a
    ///   standard-library walk that [`walk`] finds, a call whose followed body
    ///   walks a position given rooted state, and rooted state handed to a call
    ///   with no single or no readable body; reports nothing otherwise.
    /// - panics: none.
    /// - executable: none — rustc emits diagnostics without a queryable
    ///   per-call diagnostic result.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the UI fixture pairs each refused walk with a bounded
    ///   read of the same state, the same walk over an input, in a body and in
    ///   a marked combinator, and an exception with its reason; helpers are
    ///   followed locally, through closures and across crates.
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
        let Maybe::Present(call) = call_site(cx, expr)
        else {
            report_direct(cx, expr, site);
            return;
        };
        let rooted = |position: crate::walk_summary::Position| {
            matches!(
                position.select(&call.arguments),
                Maybe::Present(argument) if matches!(state_root(cx, argument), Maybe::Present(()))
            )
        };
        match classify(cx.tcx, cx.typing_env(), call.def_id, call.args) {
            Callee::Library(_) => report_direct(cx, expr, site),
            Callee::Body(target) => {
                let summary = self.summaries.of(cx.tcx, target);
                let Some((_, witness)) = summary.walks().find(|&(position, _)| rooted(position))
                else {
                    return;
                };
                let tail = match witness.walk {
                    Walk::Iterates => ITERATES_TAIL,
                    Walk::Searches => SEARCHES_TAIL,
                    Walk::Copies => COPIES_TAIL,
                    Walk::Compares => COMPARES_TAIL,
                    Walk::Unresolved => UNRESOLVED_TAIL,
                    Walk::Unreadable => UNREADABLE_TAIL,
                };
                let help = match witness.walk {
                    Walk::Unresolved => UNRESOLVED_HELP,
                    Walk::Unreadable => UNREADABLE_HELP,
                    Walk::Iterates | Walk::Searches | Walk::Copies | Walk::Compares => {
                        TRAVERSAL_HELP
                    },
                };
                let path = cx.tcx.def_path_str(target);
                span_lint_and_then(
                    cx,
                    SPEC_CLAUSE_TRAVERSAL,
                    site,
                    format!("this specification clause calls `{path}`, which {tail}"),
                    |diagnostic| {
                        diagnostic.span_note(witness.span, WITNESS_NOTE);
                        diagnostic.help(help);
                    },
                );
            },
            Callee::Unresolved(target) => {
                if call
                    .arguments
                    .iter()
                    .any(|argument| matches!(state_root(cx, argument), Maybe::Present(())))
                {
                    let path = cx.tcx.def_path_str(target);
                    span_lint_and_help(
                        cx,
                        SPEC_CLAUSE_TRAVERSAL,
                        site,
                        format!(
                            "this specification clause {HANDED_PREFIX} `{path}`, which resolves to no single body here"
                        ),
                        None,
                        UNRESOLVED_HELP,
                    );
                }
            },
            Callee::Unreadable(target) => {
                if call
                    .arguments
                    .iter()
                    .any(|argument| matches!(state_root(cx, argument), Maybe::Present(())))
                {
                    let path = cx.tcx.def_path_str(target);
                    span_lint_and_help(
                        cx,
                        SPEC_CLAUSE_TRAVERSAL,
                        site,
                        format!(
                            "this specification clause {HANDED_PREFIX} `{path}`, whose body this build cannot read"
                        ),
                        None,
                        UNREADABLE_HELP,
                    );
                }
            },
            Callee::Instrumentation | Callee::Bounded | Callee::Opaque => {},
        }
    }

    /// Refuse a `# Clause cost` heading on a function without its bound.
    ///
    /// # Specification
    /// - ensures: reports the function's definition when [`section`] reads
    ///   [`Section::Malformed`]; reports nothing otherwise.
    /// - panics: none.
    /// - executable: none — rustc emits diagnostics without a queryable
    ///   per-call diagnostic result.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the UI fixture leaves one heading without its bound
    ///   beside well-formed sections on a function and on a trait method.
    /// - witness: `tests::ui_spec_cost`
    fn check_fn(
        &mut self,
        cx: &LateContext<'tcx>,
        kind: FnKind<'tcx>,
        _decl: &'tcx FnDecl<'tcx>,
        _body: &'tcx Body<'tcx>,
        _span: Span,
        def_id: LocalDefId,
    )
    {
        if matches!(kind, FnKind::Closure) {
            return;
        }
        report_malformed(cx, def_id);
    }

    /// Refuse a `# Clause cost` heading on a required trait method without its
    /// bound.
    ///
    /// # Specification
    /// - ensures: reports a required trait method's declaration when
    ///   [`section`] reads [`Section::Malformed`]; methods with a default body
    ///   are checked by [`LateLintPass::check_fn`].
    /// - panics: none.
    /// - executable: none — rustc emits diagnostics without a queryable
    ///   per-call diagnostic result.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the UI fixture marks a required trait method with a
    ///   well-formed section, which an unresolved call relies on.
    /// - witness: `tests::ui_spec_cost`
    fn check_trait_item(
        &mut self,
        cx: &LateContext<'tcx>,
        item: &'tcx TraitItem<'tcx>,
    )
    {
        if matches!(item.kind, TraitItemKind::Fn(_, TraitFn::Required(_))) {
            report_malformed(cx, item.owner_id.def_id);
        }
    }
}

/// Report a malformed `# Clause cost` section at its function.
///
/// # Specification
/// - ensures: reports the definition of `def_id` when [`section`] reads
///   [`Section::Malformed`]; reports nothing otherwise.
/// - panics: none.
/// - executable: none — rustc emits diagnostics without a queryable per-call
///   diagnostic result.
///
/// # Adequacy
/// - hypothesis: L3 — the UI fixture leaves one heading without its bound.
/// - witness: `tests::ui_spec_cost`
fn report_malformed(
    cx: &LateContext<'_>,
    def_id: LocalDefId,
)
{
    if section(cx.tcx, def_id.to_def_id()) == Section::Malformed {
        span_lint_and_help(
            cx,
            SPEC_CLAUSE_TRAVERSAL,
            cx.tcx.def_span(def_id),
            SECTION_MESSAGE,
            None,
            SECTION_HELP,
        );
    }
}

/// Report a standard-library walk the clause makes itself.
///
/// # Specification
/// - ensures: reports `site` with the walk's message when [`walk`] finds a walk
///   in `expr`; reports nothing otherwise.
/// - panics: none.
/// - executable: none — rustc emits diagnostics without a queryable per-call
///   diagnostic result.
///
/// # Adequacy
/// - hypothesis: L3 — the UI fixture iterates, searches, copies and compares
///   rooted state in clauses.
/// - witness: `tests::ui_spec_cost`
fn report_direct<'tcx>(
    cx: &LateContext<'tcx>,
    expr: &'tcx Expr<'tcx>,
    site: Span,
)
{
    let Maybe::Present(found) = walk(cx, expr)
    else {
        return;
    };
    let tail = match found {
        Walk::Iterates => ITERATES_TAIL,
        Walk::Searches => SEARCHES_TAIL,
        Walk::Copies => COPIES_TAIL,
        Walk::Compares | Walk::Unresolved | Walk::Unreadable => COMPARES_TAIL,
    };
    span_lint_and_help(
        cx,
        SPEC_CLAUSE_TRAVERSAL,
        site,
        format!("this specification clause {tail}"),
        None,
        TRAVERSAL_HELP,
    );
}

/// A call inside a clause: its callee and the expressions in parameter order.
struct CallSite<'tcx>
{
    /// The function or method the call names.
    def_id: DefId,
    /// The generic arguments the call instantiates it with.
    args: GenericArgsRef<'tcx>,
    /// The receiver, if any, then the arguments.
    arguments: Vec<&'tcx Expr<'tcx>>,
}

/// Read a call's callee and arguments.
///
/// # Specification
/// - ensures: a method call gives its resolved method with the receiver first;
///   a call of a path to a function or associated function gives that function
///   with its arguments; an equality comparison gives `PartialEq::eq` with both
///   operands; every other expression is `Absent`.
/// - panics: none.
/// - executable: none — the outcome is read from the typeck results, which a
///   predicate could only read again.
///
/// # Adequacy
/// - hypothesis: L3 — the UI fixture follows helpers called as methods, as
///   paths and through `==` on a type whose equality walks.
/// - witness: `tests::ui_spec_cost`
fn call_site<'tcx>(
    cx: &LateContext<'tcx>,
    expr: &'tcx Expr<'tcx>,
) -> Maybe<CallSite<'tcx>, call_reading::NotACall>
{
    let typeck = cx.typeck_results();
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "`ExprKind` is rustc's HIR expression kind, whose variants move with the pinned \
                  nightly; only a method call or an equality comparison is a call"
    )]
    let (def_id, args, arguments) = match expr.kind {
        ExprKind::MethodCall(_, receiver, arguments, _) => {
            let Some(def_id) = typeck.type_dependent_def_id(expr.hir_id)
            else {
                return Maybe::Absent(call_reading::NotACall::NotAFunction);
            };
            let mut all = vec![receiver];
            all.extend(arguments.iter());
            (def_id, typeck.node_args(expr.hir_id), all)
        },
        ExprKind::Call(callee, arguments) => {
            let ExprKind::Path(ref path) = callee.kind
            else {
                return Maybe::Absent(call_reading::NotACall::NotAFunction);
            };
            let Res::Def(DefKind::Fn | DefKind::AssocFn, def_id) =
                typeck.qpath_res(path, callee.hir_id)
            else {
                return Maybe::Absent(call_reading::NotACall::NotAFunction);
            };
            (
                def_id,
                typeck.node_args(callee.hir_id),
                arguments.iter().collect(),
            )
        },
        ExprKind::Binary(op, left, right) if matches!(op.node, BinOpKind::Eq | BinOpKind::Ne) => {
            let Some(eq) = cx.tcx.lang_items().eq_trait().and_then(|trait_id| {
                cx.tcx
                    .associated_items(trait_id)
                    .filter_by_name_unhygienic(sym::eq)
                    .next()
                    .map(|item| item.def_id)
            })
            else {
                return Maybe::Absent(call_reading::NotACall::NotAFunction);
            };
            let args = cx
                .tcx
                .mk_args(&[typeck.expr_ty(left).into(), typeck.expr_ty(right).into()]);
            (eq, args, vec![left, right])
        },
        _ => return Maybe::Absent(call_reading::NotACall::OtherExpression),
    };
    // A desugared call through a lang-item path records no arguments; the
    // standard-library table reads it instead.
    if cx.tcx.generics_of(def_id).count() != args.len() {
        return Maybe::Absent(call_reading::NotACall::NotAFunction);
    }
    Maybe::Present(CallSite {
        def_id,
        args,
        arguments,
    })
}

/// Find the standard-library walk an expression makes over accumulated state.
///
/// # Specification
/// - ensures: a method call rooted, by [`state_root`], at the receiver or a
///   mutable parameter is classified by [`library_walk`] from its name, its
///   receiver and its result. A comparison operator is [`Walk::Compares`] when
///   either operand is so rooted and the left is a container. A call of
///   `IntoIterator::into_iter`, which a `for` loop desugars to, is
///   [`Walk::Iterates`] when its argument is so rooted and not already an
///   iterator. A fixed-size array is bounded; every other expression is
///   `Absent`.
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
#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "`ExprKind` is rustc's HIR expression kind, whose variants move with the pinned \
              nightly; only a method call or an `into_iter` call walks"
)]
fn walk<'tcx>(
    cx: &LateContext<'tcx>,
    expr: &'tcx Expr<'tcx>,
) -> Maybe<Walk, walk_reading::NotAWalk>
{
    let typeck = cx.typeck_results();
    match expr.kind {
        ExprKind::MethodCall(segment, receiver, ..) => {
            if !matches!(state_root(cx, receiver), Maybe::Present(())) {
                return Maybe::Absent(walk_reading::NotAWalk::Unrooted);
            }
            // Autoref may coerce an array receiver to a slice; the array is
            // what the state holds.
            if typeck.expr_ty(receiver).peel_refs().is_array() {
                return Maybe::Absent(walk_reading::NotAWalk::Bounded);
            }
            match library_walk(
                cx.tcx,
                cx.typing_env(),
                segment.ident.name,
                typeck.expr_ty_adjusted(receiver),
                typeck.expr_ty(expr),
            ) {
                Maybe::Present(found) => Maybe::Present(found),
                Maybe::Absent(_) => Maybe::Absent(walk_reading::NotAWalk::Bounded),
            }
        },
        ExprKind::Binary(op, left, right) => {
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
                && matches!(container(cx.tcx, held), Maybe::Present(_))
            {
                Maybe::Present(Walk::Compares)
            }
            else {
                Maybe::Absent(walk_reading::NotAWalk::Bounded)
            }
        },
        ExprKind::Call(callee, arguments) => {
            let [ref argument] = *arguments
            else {
                return Maybe::Absent(walk_reading::NotAWalk::Unrooted);
            };
            let into_iter = matches!(callee.kind, ExprKind::Path(ref path)
                if matches!(typeck.qpath_res(path, callee.hir_id), Res::Def(_, id) if cx.tcx.is_lang_item(id, LangItem::IntoIterIntoIter)));
            if !into_iter || !matches!(state_root(cx, argument), Maybe::Present(())) {
                return Maybe::Absent(walk_reading::NotAWalk::Unrooted);
            }
            let held = typeck.expr_ty(argument).peel_refs();
            if held.is_array()
                || matches!(iterator(cx.tcx, cx.typing_env(), held), Maybe::Present(()))
            {
                Maybe::Absent(walk_reading::NotAWalk::Bounded)
            }
            else {
                Maybe::Present(Walk::Iterates)
            }
        },
        _ => Maybe::Absent(walk_reading::NotAWalk::Unrooted),
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
#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "`ExprKind` is rustc's HIR expression kind, whose variants move with the pinned \
              nightly; only a projection, a borrow or a local path leads to the root"
)]
fn state_root(
    cx: &LateContext<'_>,
    expr: &Expr<'_>,
) -> Maybe<(), root_reading::Unrooted>
{
    let mut current = expr;
    loop {
        current = match current.kind {
            ExprKind::Field(base, _)
            | ExprKind::Unary(UnOp::Deref, base)
            | ExprKind::AddrOf(_, _, base)
            | ExprKind::Index(base, ..)
            | ExprKind::MethodCall(_, base, ..) => base,
            ExprKind::Path(QPath::Resolved(None, path)) => {
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
            _ => return Maybe::Absent(root_reading::Unrooted::OtherBase),
        };
    }
}
