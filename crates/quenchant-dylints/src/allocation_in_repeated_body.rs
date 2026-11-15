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
//! `to_string()`; and `collect()` into an owned collection. In a crate that
//! `repeated_call_crates` names, a call allocates too when its callee answers
//! for its own allocations and returns a heap value that its body allocates
//! ([`Returns::fresh`]): a function of this crate, or of a crate that
//! `allocation_default_crates` names. So does each item of a `for` loop whose
//! iterator's `next` is such a function. The caller cannot change how a
//! different crate makes its result, so a call into one counts only by its
//! standard forms above. A `clone()` of an owned value is not one of them:
//! [`clone_in_loop`] prices it.
//!
//! # Boundaries
//!
//! The repetition is [`crate::cost_shape`]'s: a loop body or a per-element
//! closure whose trip count is open, and that the allocation does not leave
//! at once, else the body of a function that a loop, a role or a repeated
//! caller repeats ([`crate::repetition`]). An allocation whose value leaves
//! the pass — pushed into an outer collection, returned, stored, handed on by
//! value or captured by a moving closure — is the structure's growth, not a
//! throwaway, and is accepted. So is an allocation on a path that diverges,
//! such as a panic's message, and a `collect()` that [`collect_read_once`]
//! already refuses.
//!
//! The lint follows a call only through the machinery: the repetition table
//! and the summaries of [`crate::allocation_free_region`]. It reads no callee
//! of its own. The machinery follows calls only in a crate that
//! `repeated_call_crates` names, and the lint reports each entry of that key
//! that names no crate at the crate's root.
//!
//! Clippy's `format_collect` and `format_push_string` take a `format!`
//! collected or pushed onto a `String`; no Clippy lint prices an allocation
//! per pass.
//!
//! [`collect_read_once`]: crate::collect_read_once::COLLECT_READ_ONCE
//! [`clone_in_loop`]: crate::clone_in_loop::CLONE_IN_LOOP

use anodized::spec;
use clippy_utils::diagnostics::span_lint_and_then;
use clippy_utils::higher::ForLoop;
use clippy_utils::macros::root_macro_call_first_node;
use quenchant_shape::shape::Maybe;
use rustc_hir::Expr;
use rustc_hir::ExprKind;
use rustc_hir::MatchSource;
use rustc_hir::Node;
use rustc_hir::PatKind;
use rustc_hir::StmtKind;
use rustc_hir::def::Res;
use rustc_hir::def_id::DefId;
use rustc_lint::LateContext;
use rustc_lint::LateLintPass;
use rustc_lint::declare_lint;
use rustc_lint::impl_lint_pass;
use rustc_middle::ty;
use rustc_middle::ty::GenericArgsRef;
use rustc_middle::ty::TyCtxt;
use rustc_middle::ty::TypingEnv;
use rustc_middle::ty::layout::LayoutOf as _;
use rustc_span::Span;
use rustc_span::sym;

use crate::allocation_free_region::Fresh;
use crate::allocation_free_region::Returns;
use crate::allocation_free_region::default_crates;
use crate::allocation_free_region::fresh_reading;
use crate::answer_totality::report_defects;
use crate::clause_cost::Enclosure;
use crate::collect_read_once::read_once;
use crate::cost_shape::Divergence;
use crate::cost_shape::Escape;
use crate::cost_shape::Machinery;
use crate::cost_shape::Method;
use crate::cost_shape::Mutation;
use crate::cost_shape::Owner;
use crate::cost_shape::Ownership;
use crate::cost_shape::Repetition;
use crate::cost_shape::binding_escape;
use crate::cost_shape::divergence;
use crate::cost_shape::heap_ownership;
use crate::cost_shape::pass_escape;
use crate::cost_shape::pass_mutation;
use crate::cost_shape::repetition;
use crate::cost_shape::std_method;
use crate::crate_list::CratePattern;
use crate::semantic::FollowsCalls;
use crate::walk_summary::standard_name;

declare_lint! {
    /// ### What it does
    ///
    /// Refuses an allocation inside a `loop`, `while` or `for` body, a closure an iterator or a standard container calls once per element, or a function that a loop, an `Iterator::next` role or a repeated caller repeats, whose value is dropped before the next pass: a `Vec`, `VecDeque` or `String` made by `new` or an empty `vec![]` and then grown, or made by `with_capacity` or `vec![..]` with elements; `Box::new` of a value with a size; `format!`; `to_string()`; `collect()` into an owned collection; a call of a function that returns a heap value its body allocates; the item of a `for` loop that such an iterator's `next` returns. A function that its caller repeats, a call and a loop's item count only in a crate that `repeated_call_crates` names.
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
            /// The call makes no fresh heap value; the reason is the
            /// summaries'.
            Unmade(crate::allocation_free_region::fresh_reading::Unmade),
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

/// Where a refused allocation stands.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Site
{
    /// An expression of the pass makes it.
    Expression,
    /// The iterator of a `for` loop makes each item.
    Item,
}

/// What makes a refused allocation.
#[derive(Clone, Copy, Debug)]
pub enum Maker
{
    /// A standard form that this lint prices by name.
    Priced,
    /// A callee that allocates the value it returns.
    Called(Fresh),
}

/// A refused allocation: where it is and what repeats it.
#[derive(Clone, Copy, Debug)]
pub struct Allocation<'tcx>
{
    /// The allocating expression, its macro call, or a `for` loop's pattern.
    pub at: Span,
    /// Whether an expression or a loop's iterator makes it.
    pub site: Site,
    /// What makes it again on every pass.
    pub repetition: Repetition<'tcx>,
    /// What makes the value.
    pub maker: Maker,
}

/// The summaries a pass follows a call by, with the crates whose functions
/// answer for their own allocations.
pub struct Following<'tcx>
{
    /// The reach and return-place readings of each body read, by instance.
    returns: Returns<'tcx>,
    /// The patterns of `allocation_default_crates`.
    crates: Vec<CratePattern>,
    /// Whether `repeated_call_crates` names the crate under the lint.
    follows: FollowsCalls,
}

impl<'tcx> Following<'tcx>
{
    /// The allocation a call of `callee` makes for the value it returns.
    ///
    /// # Specification
    /// - ensures: `Absent(Unfollowed)` in a crate that `repeated_call_crates`
    ///   does not name; [`Returns::fresh`] under the crates that answer
    ///   otherwise.
    /// - panics: none.
    /// - executable: none — the outcome is the readings, which a predicate
    ///   could only compute again.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — as [`Returns::fresh`]; the unfollowed fixture's crate
    ///   follows no call.
    /// - witness: `tests::ui_repeated_call`
    fn fresh(
        &mut self,
        tcx: TyCtxt<'tcx>,
        typing_env: TypingEnv<'tcx>,
        callee: DefId,
        args: GenericArgsRef<'tcx>,
    ) -> Maybe<Fresh, fresh_reading::Unmade>
    {
        if !self.follows.0 {
            return Maybe::Absent(fresh_reading::Unmade::Unfollowed);
        }
        self.returns
            .fresh(tcx, typing_env, &self.crates, callee, args)
    }
}

/// Repeated-allocation policy; specification clauses are left to
/// `spec_clause_traversal`.
pub struct AllocationInRepeatedBody<'tcx>
{
    /// The machinery every cost-shape pass takes.
    machinery: Machinery,
    /// What the pass follows a call by.
    following: Following<'tcx>,
}

impl AllocationInRepeatedBody<'_>
{
    /// The pass over the shared machinery, with summaries of its own and the
    /// crates of the default's configuration.
    ///
    /// # Specification
    /// - requires: as [`default_crates`].
    /// - ensures: empty summaries and the configured crates; no call is
    ///   followed before [`LateLintPass::check_crate`] reads the crate's name.
    /// - panics: none.
    /// - executable: none — the configuration read outcome is not retained.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the cost-shape fixtures run the pass so built.
    /// - witness: `tests::ui_cost_shape`
    #[must_use]
    pub fn configured(machinery: Machinery) -> Self
    {
        Self {
            machinery,
            following: Following {
                returns: Returns::default(),
                crates: default_crates(),
                follows: FollowsCalls(false),
            },
        }
    }
}

impl_lint_pass!(AllocationInRepeatedBody<'_> => [ALLOCATION_IN_REPEATED_BODY]);

impl<'tcx> LateLintPass<'tcx> for AllocationInRepeatedBody<'tcx>
{
    /// Decide whether the pass follows the crate's calls, and report each
    /// defect of `repeated_call_crates`.
    ///
    /// # Specification
    /// - ensures: the pass follows calls exactly when the machinery does
    ///   ([`Machinery::follows`]); each defect is reported at the crate root.
    /// - panics: none.
    /// - executable: none — rustc owns diagnostic emission; no readback exists.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the call fixture names its crates, the unfollowed
    ///   fixture follows no call, and the refused configuration reports each
    ///   defect.
    /// - witness: `tests::ui_repeated_call`
    /// - witness: `tests::ui_repeated_call_refused`
    fn check_crate(
        &mut self,
        cx: &LateContext<'tcx>,
    )
    {
        self.following.follows = self.machinery.follows(cx.tcx);
        report_defects(
            cx,
            ALLOCATION_IN_REPEATED_BODY,
            self.machinery.repetitions.followed().defects(),
        );
    }

    /// Report each allocation a repeated body drops before its next pass.
    ///
    /// # Specification
    /// - ensures: reports every expression that [`allocation`] or
    ///   [`item_allocation`] answers `Present` for, at the expression, its
    ///   macro call or the loop's pattern, with the note of what repeats it
    ///   and, for a call, a note at the callee's call that allocates.
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
        let found = match allocation(cx, &self.machinery, &mut self.following, expr) {
            Maybe::Present(found) => found,
            Maybe::Absent(_) => match item_allocation(cx, &mut self.following, expr) {
                Maybe::Present(found) => found,
                Maybe::Absent(_) => return,
            },
        };
        if self.machinery.regions.encloses(found.at) == Enclosure::Inside {
            return;
        }
        let message = match found.site {
            Site::Expression => {
                "this allocates on every pass of a loop, and the value is dropped before the next \
                 pass"
            },
            Site::Item => {
                "each pass of this loop takes an item that its iterator allocates, and the pass \
                 drops it before the next pass"
            },
        };
        span_lint_and_then(
            cx,
            ALLOCATION_IN_REPEATED_BODY,
            found.at,
            message,
            |diagnostic| {
                found.repetition.note(cx.tcx, diagnostic);
                if let Maker::Called(fresh) = found.maker {
                    diagnostic.span_note(
                        fresh.at,
                        format!(
                            "`{}` allocates the value it returns here",
                            cx.tcx.def_path_str(fresh.item)
                        ),
                    );
                }
                diagnostic.help(
                "make the buffer once before the loop and `clear()` it each pass, write into one \
                 reused buffer, or borrow what the pass reads",
            );
            },
        );
    }
}

/// What an expression may allocate, before the repetition is read.
#[derive(Clone, Copy, Debug)]
enum Candidate
{
    /// A standard form this lint prices by name, and where to report it.
    Priced(Making, Span),
    /// A call whose result owns a heap allocation, which the summaries read.
    Call,
}

/// Decide whether an expression is an allocation a repeated body drops.
///
/// # Specification
/// - ensures: `Present` when `expr` allocates ([`making`]), or is a call that
///   returns a heap value its callee allocates ([`fresh_call`]), inside a
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
///   buffer, `vec![..]`, `Box::new`, `format!`, `to_string()`, a `collect` into
///   a set and a call of a function that returns a `vec!`, each dropped in its
///   pass, and accept the same allocations pushed outward, returned from a
///   per-element closure, stored, made by `new` and never grown, outside any
///   loop, in a loop over an array, in a panic's message and read once by
///   `collect_read_once`.
/// - witness: `tests::ui_cost_shape`
pub fn allocation<'tcx>(
    cx: &LateContext<'tcx>,
    machinery: &Machinery,
    following: &mut Following<'tcx>,
    expr: &'tcx Expr<'tcx>,
) -> Maybe<Allocation<'tcx>, allocation_reading::Unrefused>
{
    let candidate = match making(cx, expr) {
        Maybe::Present((made, at)) => Candidate::Priced(made, at),
        Maybe::Absent(reason) => match owning_call(cx, expr) {
            Ownership::Owning => Candidate::Call,
            Ownership::Unknown | Ownership::Refcounted | Ownership::Plain => {
                return Maybe::Absent(reason);
            },
        },
    };
    let found = match repetition(cx, machinery, expr) {
        Maybe::Present(found) => found,
        Maybe::Absent(reason) => {
            return Maybe::Absent(allocation_reading::Unrefused::Unrepeated(reason));
        },
    };
    let scope = found.scope();
    if divergence(cx, expr, scope) == Divergence::Diverges {
        return Maybe::Absent(allocation_reading::Unrefused::Diverges);
    }
    if pass_escape(cx, expr, scope) == Escape::Escapes {
        return Maybe::Absent(allocation_reading::Unrefused::Escapes);
    }
    let (made, at, maker) = match candidate {
        Candidate::Priced(made, at) => (made, at, Maker::Priced),
        Candidate::Call => match fresh_call(cx, following, expr) {
            Maybe::Present(fresh) => (Making::Allocates, expr.span, Maker::Called(fresh)),
            Maybe::Absent(reason) => {
                return Maybe::Absent(allocation_reading::Unrefused::Unmade(reason));
            },
        },
    };
    if made == Making::Empty && grown(cx, expr, scope) == Mutation::Untouched {
        return Maybe::Absent(allocation_reading::Unrefused::NeverGrown);
    }
    if matches!(read_once(cx, expr), Maybe::Present(_)) {
        return Maybe::Absent(allocation_reading::Unrefused::ReadOnce);
    }
    Maybe::Present(Allocation {
        at,
        site: Site::Expression,
        repetition: found,
        maker,
    })
}

/// Whether an expression is a call, outside a macro, whose result owns a heap
/// allocation and that `clone_in_loop` does not price.
///
/// # Specification
/// - ensures: [`heap_ownership`] of `expr`'s type when `expr` is a call or a
///   method call outside a macro expansion that resolves to neither
///   `Clone::clone` nor `ToOwned::to_owned`; `Plain` otherwise.
/// - panics: none.
/// - executable: none — the outcome is the compiler's types, which a predicate
///   could only read again.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures call a function that returns a vector and
///   one that returns a count, and clone a local type.
/// - witness: `tests::ui_cost_shape`
fn owning_call<'tcx>(
    cx: &LateContext<'tcx>,
    expr: &'tcx Expr<'tcx>,
) -> Ownership
{
    if expr.span.from_expansion()
        || !matches!(expr.kind, ExprKind::Call(..) | ExprKind::MethodCall(..))
    {
        return Ownership::Plain;
    }
    let copies = cx
        .typeck_results()
        .type_dependent_def_id(expr.hir_id)
        .and_then(|method| cx.tcx.trait_of_assoc(method))
        .is_some_and(|owner| {
            cx.tcx.is_diagnostic_item(sym::Clone, owner)
                || cx
                    .tcx
                    .get_diagnostic_name(owner)
                    .is_some_and(|name| name.as_str() == "ToOwned")
        });
    if copies {
        return Ownership::Plain;
    }
    heap_ownership(cx.tcx, cx.typing_env(), cx.typeck_results().expr_ty(expr))
}

/// The callee's allocation of the heap value a call returns.
///
/// # Specification
/// - ensures: [`Returns::fresh`] of the function `expr` calls, under the
///   arguments type checking records for it and the typing of the body that
///   holds `expr`; `Absent(Unresolved)` when `expr` names no function.
/// - panics: none.
/// - executable: none — the outcome is the summaries, which a predicate could
///   only compute again.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures call a function and a method that return a
///   vector they allocate, and one that returns an empty vector.
/// - witness: `tests::ui_cost_shape`
fn fresh_call<'tcx>(
    cx: &LateContext<'tcx>,
    following: &mut Following<'tcx>,
    expr: &'tcx Expr<'tcx>,
) -> Maybe<Fresh, fresh_reading::Unmade>
{
    let typeck = cx.typeck_results();
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "`ExprKind` is rustc's, its variants moving with the pinned nightly; only a call \
                  or a method call names a function"
    )]
    let (callee, args) = match expr.kind {
        ExprKind::Call(function, _) => match *typeck.expr_ty(function).kind() {
            ty::FnDef(callee, args) => match args.no_bound_vars() {
                Some(args) => (callee, args),
                None => return Maybe::Absent(fresh_reading::Unmade::Unresolved),
            },
            _ => return Maybe::Absent(fresh_reading::Unmade::Unresolved),
        },
        ExprKind::MethodCall(..) => match typeck.type_dependent_def_id(expr.hir_id) {
            Some(method) => (method, typeck.node_args(expr.hir_id)),
            None => return Maybe::Absent(fresh_reading::Unmade::Unresolved),
        },
        _ => return Maybe::Absent(fresh_reading::Unmade::Unresolved),
    };
    let typing_env = TypingEnv::post_analysis(cx.tcx, expr.hir_id.owner.def_id);
    following.fresh(cx.tcx, typing_env, callee, args)
}

/// Decide whether a `for` loop takes, on each pass, an item that its iterator
/// allocates and the pass drops.
///
/// # Specification
/// - ensures: `Present` at the loop's pattern when `expr` is a `for` loop, its
///   desugared `Iterator::next` call returns a heap value its callee allocates
///   ([`Returns::fresh`]), the pattern is a plain binding, and the loop's pass
///   does not move that binding out ([`binding_escape`]).
/// - ensures: `Absent` with the first reason that holds otherwise.
/// - panics: none.
/// - executable: none — the outcome is the compiler's resolution and use walk,
///   which a predicate could only repeat.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures refuse a `for` loop over a local iterator
///   whose `next` returns a clone of its digits, only borrowed in the pass, and
///   accept the same loop that pushes each item outward.
/// - witness: `tests::ui_cost_shape`
pub fn item_allocation<'tcx>(
    cx: &LateContext<'tcx>,
    following: &mut Following<'tcx>,
    expr: &'tcx Expr<'tcx>,
) -> Maybe<Allocation<'tcx>, allocation_reading::Unrefused>
{
    let none = Maybe::Absent(allocation_reading::Unrefused::NotAnAllocation);
    let Some(for_loop) = ForLoop::hir(expr)
    else {
        return none;
    };
    let Node::Expr(looped) = cx.tcx.hir_node(for_loop.loop_id)
    else {
        return none;
    };
    let ExprKind::Loop(block, ..) = looped.kind
    else {
        return none;
    };
    let matched = match (block.stmts, block.expr) {
        (&[ref statement], None) => match statement.kind {
            StmtKind::Expr(matched) | StmtKind::Semi(matched) => matched,
            StmtKind::Let(_) | StmtKind::Item(_) => return none,
        },
        (&[], Some(matched)) => matched,
        _ => return none,
    };
    let ExprKind::Match(next, _, MatchSource::ForLoopDesugar) = matched.kind
    else {
        return none;
    };
    let ExprKind::Call(function, _) = next.kind
    else {
        return none;
    };
    let ty::FnDef(callee, args) = *cx.typeck_results().expr_ty(function).kind()
    else {
        return none;
    };
    let Some(args) = args.no_bound_vars()
    else {
        return none;
    };
    let typing_env = TypingEnv::post_analysis(cx.tcx, expr.hir_id.owner.def_id);
    let fresh = match following.fresh(cx.tcx, typing_env, callee, args) {
        Maybe::Present(fresh) => fresh,
        Maybe::Absent(reason) => {
            return Maybe::Absent(allocation_reading::Unrefused::Unmade(reason));
        },
    };
    let PatKind::Binding(_, local, _, None) = for_loop.pat.kind
    else {
        return Maybe::Absent(allocation_reading::Unrefused::Escapes);
    };
    if binding_escape(cx, local, looped) == Escape::Escapes {
        return Maybe::Absent(allocation_reading::Unrefused::Escapes);
    }
    Maybe::Present(Allocation {
        at: for_loop.pat.span,
        site: Site::Item,
        repetition: Repetition::Local(looped),
        maker: Maker::Called(fresh),
    })
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
