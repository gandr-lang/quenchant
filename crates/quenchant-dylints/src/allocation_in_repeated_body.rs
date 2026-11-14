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
//! In a crate that `repeated_call_crates` names, the lint also follows each
//! call of a repeated body into its callee, over MIR, with the summaries of
//! [`crate::allocation_free_region`]. A call is refused when some path of its
//! callee reaches the allocator or an allocating signature, as a marked region
//! reads it, and the pass drops what it allocates. A callee that the build
//! cannot name, as a trait object or one that names a parameter of its caller,
//! is not refused. Nor is a call that borrows an argument mutably and returns
//! nothing that owns heap memory: that is the growth of the borrowed place, as
//! with `push`. The refusal is `clone_in_loop`'s when the call path passes
//! through `Clone::clone` or `ToOwned::to_owned`. The other crates follow no
//! call, and pay nothing for it.
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
use rustc_hir::def_id::DefId;
use rustc_hir::def_id::LOCAL_CRATE;
use rustc_lint::LateContext;
use rustc_lint::LateLintPass;
use rustc_lint::declare_lint;
use rustc_lint::impl_lint_pass;
use rustc_middle::ty;
use rustc_middle::ty::Ty;
use rustc_middle::ty::TypeVisitableExt as _;
use rustc_middle::ty::TypingEnv;
use rustc_middle::ty::layout::LayoutOf as _;
use rustc_span::DesugaringKind;
use rustc_span::Span;
use rustc_span::sym;

use crate::allocation_free_region::Cause;
use crate::allocation_free_region::CratePattern;
use crate::allocation_free_region::Reach;
use crate::allocation_free_region::Summaries;
use crate::allocation_free_region::Witness;
use crate::allocation_free_region::allocation_path;
use crate::allocation_free_region::callee_reach;
use crate::allocation_free_region::crate_pattern;
use crate::allocation_free_region::pattern_reading;
use crate::answer_totality::CONFIGURATION_TABLE;
use crate::answer_totality::report_defects;
use crate::clause_cost::ClauseRegions;
use crate::clause_cost::Enclosure;
use crate::clone_in_loop::CLONE_IN_LOOP;
use crate::clone_in_loop::clone_reading;
use crate::clone_in_loop::copy;
use crate::clone_in_loop::copying_method;
use crate::collect_read_once::read_once;
use crate::cost_shape::Divergence;
use crate::cost_shape::Escape;
use crate::cost_shape::Method;
use crate::cost_shape::Mutation;
use crate::cost_shape::Owner;
use crate::cost_shape::Ownership;
use crate::cost_shape::divergence;
use crate::cost_shape::heap_ownership;
use crate::cost_shape::pass_escape;
use crate::cost_shape::pass_mutation;
use crate::cost_shape::repetition;
use crate::cost_shape::std_method;
use crate::semantic::CrateName;
use crate::semantic::FollowsCalls;
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

/// The key of the crates whose loops the lint follows into their callees.
const CALLS_KEY: &str = "repeated_call_crates";

/// Repeated-allocation policy; specification clauses are left to
/// `spec_clause_traversal`.
pub struct AllocationInRepeatedBody<'tcx>
{
    /// The authored `spec` attribute ranges, whose clauses are not inspected.
    regions: ClauseRegions,
    /// The crates whose loops the lint follows into their callees.
    crates: Vec<CratePattern>,
    /// Each entry of the configuration the lint cannot read.
    defects: Vec<String>,
    /// Whether the crate under lint is one of `crates`.
    follows: FollowsCalls,
    /// The reach of each callee read so far, over MIR, by instance.
    summaries: Summaries<'tcx>,
}

impl AllocationInRepeatedBody<'_>
{
    /// The pass reads the ranges the pre-expansion clause collector records,
    /// and the crates whose loops it follows into their callees.
    ///
    /// # Specification
    /// - requires: `dylint_linting::init_config` has already run, which
    ///   [`crate::register_lints`] guarantees by calling it first.
    /// - ensures: each entry of `repeated_call_crates` that names a crate, or
    ///   the start of one followed by `*`, becomes a pattern
    ///   ([`crate_pattern`]); a value that is no list, and each other entry,
    ///   becomes a defect. With no configuration or no key, no crate is
    ///   followed.
    /// - panics: none.
    /// - executable: none — the outcome is the configuration as read, which a
    ///   predicate could only read again.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the call fixture names its crate; the refused
    ///   configuration names a number, an empty entry and `*`, each reported.
    /// - witness: `tests::ui_repeated_call`
    /// - witness: `tests::ui_repeated_call_refused`
    #[must_use]
    pub fn new(regions: ClauseRegions) -> Self
    {
        let mut crates = Vec::new();
        let mut defects = Vec::new();
        if let Ok(Some(config)) = dylint_linting::config_toml(CONFIGURATION_TABLE)
            && let Some(value) = config.get(CALLS_KEY)
        {
            if let Some(entries) = value.as_array() {
                for entry in entries {
                    let pattern = entry
                        .as_str()
                        .map_or(Maybe::Absent(pattern_reading::Unnamed::Empty), |name| {
                            crate_pattern(CrateName(name))
                        });
                    match pattern {
                        Maybe::Present(pattern) => crates.push(pattern),
                        Maybe::Absent(_) => defects.push(format!(
                            "`{CALLS_KEY}` entry `{entry}` is no crate name, nor the start of one \
                             followed by `*`"
                        )),
                    }
                }
            }
            else {
                defects.push(format!("`{CALLS_KEY}` is no list of crate names"));
            }
        }
        Self {
            regions,
            crates,
            defects,
            follows: FollowsCalls(false),
            summaries: Summaries::default(),
        }
    }
}

impl_lint_pass!(AllocationInRepeatedBody<'_> => [ALLOCATION_IN_REPEATED_BODY]);

impl<'tcx> LateLintPass<'tcx> for AllocationInRepeatedBody<'tcx>
{
    /// Decide whether the crate's loops are followed into their callees, and
    /// report each defect of the configuration.
    ///
    /// # Specification
    /// - ensures: the crate's loops are followed exactly when one of the
    ///   configured patterns takes its name; each defect is reported at the
    ///   crate root.
    /// - panics: none.
    /// - executable: none — rustc owns diagnostic emission; no readback exists.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the call fixture names its crate, and the plain
    ///   fixture names none and follows no call.
    /// - witness: `tests::ui_repeated_call`
    fn check_crate(
        &mut self,
        cx: &LateContext<'tcx>,
    )
    {
        let name = cx.tcx.crate_name(LOCAL_CRATE);
        self.follows = FollowsCalls(
            self.crates
                .iter()
                .any(|pattern| pattern.matches(CrateName(name.as_str())).0),
        );
        report_defects(cx, ALLOCATION_IN_REPEATED_BODY, &self.defects);
    }

    /// Report each allocation a repeated body drops before its next pass, made
    /// in the body or, in a crate whose loops are followed, in a callee of it.
    ///
    /// # Specification
    /// - ensures: reports every expression [`allocation`] answers `Present`
    ///   for, at the expression or its macro call, with a note at the loop that
    ///   repeats it; and, in a crate whose loops are followed, every call that
    ///   [`allocation`] does not price and [`allocating_call`] answers
    ///   `Present` for, with notes at the loop and at the allocation that the
    ///   callee reaches.
    /// - ensures: an expression whose span, or whose macro call's span, lies
    ///   inside a `spec` attribute is not inspected.
    /// - panics: none.
    /// - executable: none — rustc emits diagnostics without a queryable
    ///   per-call diagnostic result.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the fixtures refuse each allocating form in each
    ///   repetition and each call whose callee allocates a value the pass
    ///   drops, and accept each exception beside them.
    /// - witness: `tests::ui_cost_shape`
    /// - witness: `tests::ui_repeated_call`
    fn check_expr(
        &mut self,
        cx: &LateContext<'tcx>,
        expr: &'tcx Expr<'tcx>,
    )
    {
        let found = match allocation(cx, expr) {
            Maybe::Present(found) => found,
            Maybe::Absent(allocation_reading::Unrefused::NotAnAllocation) => {
                if self.follows.0
                    && let Maybe::Present(called) = allocating_call(cx, expr, &mut self.summaries)
                    && self.regions.encloses(expr.span) != Enclosure::Inside
                {
                    report_call(cx, expr, &called);
                }
                return;
            },
            Maybe::Absent(_) => return,
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

quenchant_shape::reason_enum! {
    /// A call in a repeated body can allocate nothing that the pass drops.
    pub mod call_reading {
        /// Why the call is not refused.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Unrefused {
            /// The expression is no call, or a macro's expansion made it.
            NotACall,
            /// `clone_in_loop` prices the call.
            Copy,
            /// The call does not repeat; the reason is the repetition's.
            Unrepeated(crate::cost_shape::repetition_reading::Unrepeated),
            /// The callee reaches no allocation.
            Free,
            /// The callee can reach an implementation that the build cannot
            /// name: a trait object, a function pointer or a type parameter.
            Unknown,
            /// The call sits on a path that diverges.
            Diverges,
            /// The call's value owns heap memory and leaves the pass.
            Escapes,
            /// The call borrows a place mutably and hands back no heap memory,
            /// so its allocation is the growth of that place.
            Grows,
        }
    }
}

/// A refused call: the loop that repeats it, the allocation its callee
/// reaches, and the functions the allocation passes through.
#[derive(Clone, Debug)]
pub struct Called<'tcx>
{
    /// The repetition that makes the call again on every pass.
    pub scope: &'tcx Expr<'tcx>,
    /// The first allocation that the callee reaches.
    pub witness: Witness,
    /// The functions from the callee down to the body that allocates
    /// ([`allocation_path`]).
    pub path: Vec<DefId>,
}

/// Decide whether a call in a repeated body allocates, in its callee, a value
/// that the pass drops.
///
/// # Specification
/// - ensures: `Present` when `expr` is a call or a method call that no macro
///   expands and that `clone_in_loop` does not price, inside an open repetition
///   ([`repetition`]), whose callee reaches an allocation over MIR
///   ([`callee_reach`]) through calls the build can name, on a path that does
///   not diverge ([`divergence`]); and either its value owns heap memory and
///   does not leave the pass ([`pass_escape`]), or it borrows no argument
///   mutably.
/// - ensures: `Absent` with the first reason that holds otherwise, in the order
///   the body tests them.
/// - panics: none.
/// - executable: none — the outcome is the compiler's resolution, the callee's
///   MIR and the use walk of the enclosing body, which a predicate could only
///   repeat.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures refuse a call whose callee returns a fresh
///   vector the pass drops, one whose callee makes and drops a buffer, and one
///   two calls deep; and accept a push into an outer vector, a callee whose
///   value is pushed outward, a free callee and a call through `dyn`.
/// - witness: `tests::ui_repeated_call`
pub fn allocating_call<'tcx>(
    cx: &LateContext<'tcx>,
    expr: &'tcx Expr<'tcx>,
    summaries: &mut Summaries<'tcx>,
) -> Maybe<Called<'tcx>, call_reading::Unrefused>
{
    if expr.span.from_expansion() && expr.span.desugaring_kind() != Some(DesugaringKind::ForLoop) {
        return Maybe::Absent(call_reading::Unrefused::NotACall);
    }
    let typeck = cx.typeck_results();
    let (callee, receiver, arguments) = if let ExprKind::Call(function, arguments) = expr.kind {
        (typeck.expr_ty(function), None, arguments)
    }
    else if let ExprKind::MethodCall(_, receiver, arguments, _) = expr.kind
        && let Some(method) = typeck.type_dependent_def_id(expr.hir_id)
    {
        (
            Ty::new_fn_def(
                cx.tcx,
                method,
                ty::Binder::dummy(typeck.node_args(expr.hir_id)),
            ),
            Some(receiver),
            arguments,
        )
    }
    else {
        return Maybe::Absent(call_reading::Unrefused::NotACall);
    };
    if !matches!(
        copy(cx, expr),
        Maybe::Absent(clone_reading::Unrefused::NotACopy)
    ) {
        return Maybe::Absent(call_reading::Unrefused::Copy);
    }
    // A callee instantiated with the caller's own parameters names no body
    // that the build can read, as a type parameter's method names none.
    if callee.has_param() {
        return Maybe::Absent(call_reading::Unrefused::Unknown);
    }
    let scope = match repetition(cx, expr) {
        Maybe::Present(scope) => scope,
        Maybe::Absent(reason) => {
            return Maybe::Absent(call_reading::Unrefused::Unrepeated(reason));
        },
    };
    // The callee names no parameter of the caller, so its body reads with no
    // caller bound, after analysis: a specialization resolves and an opaque
    // type shows its hidden type.
    let owner = cx.tcx.hir_enclosing_body_owner(expr.hir_id);
    let Reach::Allocates(witness) = callee_reach(
        cx.tcx,
        TypingEnv::fully_monomorphized(),
        summaries,
        callee,
        expr.span,
        owner.to_def_id(),
    )
    else {
        return Maybe::Absent(call_reading::Unrefused::Free);
    };
    if matches!(witness.cause, Cause::Dynamic | Cause::Parameter) {
        return Maybe::Absent(call_reading::Unrefused::Unknown);
    }
    if divergence(cx, expr, scope) == Divergence::Diverges {
        return Maybe::Absent(call_reading::Unrefused::Diverges);
    }
    if heap_ownership(cx.tcx, cx.typing_env(), typeck.expr_ty(expr)) == Ownership::Owning {
        if pass_escape(cx, expr, scope) == Escape::Escapes {
            return Maybe::Absent(call_reading::Unrefused::Escapes);
        }
    }
    else if receiver.into_iter().chain(arguments).any(|argument| {
        matches!(
            typeck.expr_ty_adjusted(argument).kind(),
            ty::Ref(_, _, ty::Mutability::Mut)
        )
    }) {
        return Maybe::Absent(call_reading::Unrefused::Grows);
    }
    let path = allocation_path(cx.tcx, TypingEnv::fully_monomorphized(), summaries, callee);
    Maybe::Present(Called {
        scope,
        witness,
        path,
    })
}

/// Report a call whose callee allocates a value that the pass drops.
///
/// # Specification
/// - ensures: one diagnostic at `expr`, under `clone_in_loop` when a function
///   of the call path is `Clone::clone` or `ToOwned::to_owned`
///   ([`copying_method`]) and under `allocation_in_repeated_body` otherwise. It
///   carries a note at the loop that repeats the call; a note with the call
///   path, up to its first function outside the crate under lint; and a note
///   that names the allocation the callee reaches: at its span when the crate
///   under lint holds it, by its function's path otherwise, as a span or a
///   function in another crate moves with each toolchain.
/// - panics: none.
/// - executable: none — rustc emits diagnostics without a queryable result.
///
/// # Adequacy
/// - hypothesis: L3 — the call fixture refuses a clone one call down under
///   `clone_in_loop`, and a vector one and two calls down under
///   `allocation_in_repeated_body`, each with its call path.
/// - witness: `tests::ui_repeated_call`
fn report_call<'tcx>(
    cx: &LateContext<'tcx>,
    expr: &'tcx Expr<'tcx>,
    called: &Called<'tcx>,
)
{
    let reached = match called.witness.cause {
        Cause::Allocator => "a call into the allocator",
        Cause::Signature => {
            "a call whose body is unavailable and whose signature hands back heap memory"
        },
        Cause::Dynamic | Cause::Parameter => "a call that the build cannot name",
    };
    let (lint, message, help) = if called
        .path
        .iter()
        .any(|function| copying_method(cx.tcx, *function).0)
    {
        (
            CLONE_IN_LOOP,
            "this call copies a value that owns heap memory on every pass of a loop, and the \
             pass drops the copy",
            "let the callee lend what it copies, or turn one value in place over the passes",
        )
    }
    else {
        (
            ALLOCATION_IN_REPEATED_BODY,
            "this call allocates on every pass of a loop, and the pass drops what it allocates",
            "make the buffer once before the loop and lend it to the callee, or let the callee \
             write into one reused buffer",
        )
    };
    let mut shown = Vec::new();
    for function in &called.path {
        shown.push(format!("`{}`", cx.tcx.def_path_str(*function)));
        if !function.is_local() {
            break;
        }
    }
    let rest = if shown.len() < called.path.len() {
        " -> ..."
    }
    else {
        ""
    };
    span_lint_and_then(cx, lint, expr.span, message, |diagnostic| {
        diagnostic.span_note(called.scope.span, "the loop that repeats it");
        if !shown.is_empty() {
            diagnostic.note(format!("the call path: {}{rest}", shown.join(" -> ")));
        }
        let owner = cx.tcx.def_path_str(called.witness.owner);
        if called.witness.owner.is_local() {
            diagnostic.span_note(
                called.witness.span,
                format!("the callee reaches {reached} here, in `{owner}`"),
            );
        }
        else {
            diagnostic.note(format!("the callee reaches {reached}, in `{owner}`"));
        }
        diagnostic.help(help);
    });
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
