//! Work forks on the consumer's one pool, never on a thread or a pool of its
//! own.
//!
//! A workspace that holds one work pool sizes it once, for the host it runs
//! on. A bare thread or a private pool beside it oversubscribes the host and
//! escapes the pool's width, so a verdict measured at one width runs at
//! another. The rule refuses a call that starts a thread or a pool: the
//! standard library's `thread::spawn`, `thread::scope` and `Builder`'s spawns,
//! and rayon's `ThreadPoolBuilder::new`, `scope`, `in_place_scope`, `spawn`
//! and `join` with their FIFO and broadcast forms. The fix is the pool's fork.
//!
//! The rule is `allow` by default: only a consumer that holds one work pool
//! selects it, with `-D pool_only_threads` in its Dylint flags.
//!
//! # Boundaries
//!
//! A site that owns its thread — a world, an I/O reader, a listener, the pool
//! itself — states why with `#[expect(pool_only_threads, reason = "…")]` at
//! the site or on a node between it and its enclosing item. An expectation on
//! an enclosing module, implementation or crate, one without its reason and an
//! `allow` written in the source are refused. An item under `#[cfg(test)]` is
//! not read: a test owns the threads it races. A test target reads only the
//! sites that state an expectation, so the expectation a library states holds
//! in the library's test build too, and the target's own threads stay free.

use anodized::spec;
use clippy_utils::is_in_test;
use quenchant_shape::shape::Maybe;
use rustc_hir::Expr;
use rustc_hir::ExprKind;
use rustc_hir::def::DefKind;
use rustc_hir::def::Res;
use rustc_hir::def_id::DefId;
use rustc_lint::LateContext;
use rustc_lint::LateLintPass;
use rustc_lint::Level;
use rustc_session::declare_lint;
use rustc_session::declare_lint_pass;

use crate::answer_totality::ExceptionRefusals;
use crate::answer_totality::report_refusing;
use crate::semantic::DiagnosticText;

declare_lint! {
    /// ### What it does
    ///
    /// Refuses a call that starts a thread or a pool outside the consumer's
    /// work pool: `std::thread::spawn`, `std::thread::scope`,
    /// `std::thread::Builder::spawn`, `spawn_scoped` and `spawn_unchecked`,
    /// `rayon::ThreadPoolBuilder::new`, and rayon's `scope`, `in_place_scope`,
    /// `spawn`, `join`, their FIFO forms and its broadcasts.
    ///
    /// ### Why is this bad?
    ///
    /// The work pool is sized once for the host. A thread or a pool beside it
    /// oversubscribes the host and runs the work at a width no one chose.
    /// A consumer that holds one work pool selects the rule; it is `allow`
    /// by default.
    ///
    /// ### The exception
    ///
    /// A site that owns its thread states why:
    /// `#[expect(pool_only_threads, reason = "...")]` at the site or on a node
    /// between it and its enclosing item.
    ///
    /// ### Example
    ///
    /// ```rust,ignore
    /// std::thread::scope(|scope| for run in runs { scope.spawn(|| mold(run)); });
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// pool.each(runs, |run| mold(run));
    /// ```
    pub POOL_ONLY_THREADS,
    Allow,
    "work forks on the consumer's pool, never on a thread or a pool of its own"
}

declare_lint_pass!(PoolOnlyThreads => [POOL_ONLY_THREADS]);

quenchant_shape::reason_enum! {
    /// A call can start no thread and no pool.
    mod start_reading {
        /// Why the call is not refused.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Unstarted {
            /// The expression calls no function the compiler resolves.
            NotACall,
            /// The function starts no thread and no pool.
            OtherFunction,
        }
    }
}

/// The help every finding carries.
const HELP: &str = concat!(
    "fork the work on the consumer's pool (`Fork::each`, `Fork::join`); a site that owns its ",
    "thread states why with `#[expect(pool_only_threads, reason = \"...\")]`",
);

/// The refusals of an exception the site does not state properly.
const REFUSALS: ExceptionRefusals = ExceptionRefusals {
    inherited: DiagnosticText(concat!(
        "a pool-only-threads exception must be stated where the thread starts: an ",
        "`#[expect(pool_only_threads, ..)]` on an enclosing module, implementation or crate ",
        "excepts nothing inside it",
    )),
    unreasoned: DiagnosticText(concat!(
        "a pool-only-threads exception must state why the site owns its thread: write ",
        "`reason = \"...\"` on its `#[expect(pool_only_threads, ..)]`",
    )),
    allowed: DiagnosticText(concat!(
        "a thread outside the pool is admitted only by ",
        "`#[expect(pool_only_threads, reason = \"...\")]`; an `allow` does not say why the site ",
        "owns its thread, nor fail when the thread goes",
    )),
};

/// The standard library's free functions that start a thread, under
/// `std::thread`.
const STD_FUNCTIONS: [&str; 2] = ["spawn", "scope"];

/// The methods of `std::thread::Builder` that start a thread.
const STD_BUILDER_METHODS: [&str; 3] = ["spawn", "spawn_scoped", "spawn_unchecked"];

/// rayon's free functions that fork outside a pool the consumer holds.
const RAYON_FUNCTIONS: [&str; 9] = [
    "scope",
    "scope_fifo",
    "in_place_scope",
    "in_place_scope_fifo",
    "spawn",
    "spawn_fifo",
    "join",
    "broadcast",
    "spawn_broadcast",
];

impl<'tcx> LateLintPass<'tcx> for PoolOnlyThreads
{
    /// Refuse a call that starts a thread or a pool, outside the tests.
    ///
    /// # Specification
    /// - ensures: reports `expr` exactly when [`started`] names its callee,
    ///   `expr` sits in no test, and either the crate is no test target or the
    ///   level at `expr` is an expectation; an exception the site does not
    ///   state properly is refused at the crate root ([`report_refusing`]).
    /// - panics: none.
    /// - executable: none — rustc owns resolution and diagnostic emission.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the fixtures refuse each starting call, accept a
    ///   reasoned expectation and a test, and refuse an unreasoned expectation,
    ///   an inherited one and an `allow`; in a test target, a reasoned
    ///   expectation is fulfilled and an unstated thread is not reported.
    /// - witness: `tests::ui_pool_threads`
    /// - witness: `tests::ui_pool_threads_test_target`
    fn check_expr(
        &mut self,
        cx: &LateContext<'tcx>,
        expr: &'tcx Expr<'tcx>,
    )
    {
        let Maybe::Present(call) = started(cx, expr)
        else {
            return;
        };
        if is_in_test(cx.tcx, expr.hir_id) {
            return;
        }
        if cx.tcx.sess.is_test_crate()
            && cx
                .tcx
                .lint_level_spec_at_node(POOL_ONLY_THREADS, expr.hir_id)
                .level()
                != Level::Expect
        {
            return;
        }
        report_refusing(
            cx,
            POOL_ONLY_THREADS,
            expr.hir_id,
            expr.span,
            format!("`{}` starts work outside the pool", call.0),
            DiagnosticText(HELP),
            &REFUSALS,
        );
    }
}

/// The path a finding names a starting call by: a free function's path, or
/// a method's owner and name.
#[repr(transparent)]
struct StartingCall(String);

/// The call `expr` makes when it starts a thread or a pool.
///
/// # Specification
/// - ensures: `Present` with the callee's path when `expr` calls a function of
///   the crate `std` under `std::thread` whose name is in [`STD_FUNCTIONS`], a
///   method of an inherent impl of `std::thread`'s `Builder` named in
///   [`STD_BUILDER_METHODS`], `rayon_core`'s `ThreadPoolBuilder::new`, or a
///   free function of `rayon_core` named in [`RAYON_FUNCTIONS`]; `Absent`
///   otherwise. A method's path is its owner's path and its name.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures call each refused function and method, and
///   `Scope::spawn`, `thread::current` and `rayon::current_num_threads`, which
///   start nothing; a wrong crate, owner or name changes a verdict.
/// - witness: `tests::ui_pool_threads`
#[spec(ensures: |output| match output {
    Maybe::Present(_) => matches!(expr.kind, ExprKind::Call(..) | ExprKind::MethodCall(..)),
    Maybe::Absent(_) => true,
})]
fn started(
    cx: &LateContext<'_>,
    expr: &Expr<'_>,
) -> Maybe<StartingCall, start_reading::Unstarted>
{
    let callee = if let ExprKind::Call(function, _) = expr.kind
        && let ExprKind::Path(ref path) = function.kind
        && let Res::Def(DefKind::Fn | DefKind::AssocFn, callee) =
            cx.qpath_res(path, function.hir_id)
    {
        Some(callee)
    }
    else if let ExprKind::MethodCall(..) = expr.kind {
        cx.typeck_results().type_dependent_def_id(expr.hir_id)
    }
    else {
        None
    };
    let Some(callee) = callee
    else {
        return Maybe::Absent(start_reading::Unstarted::NotACall);
    };
    let tcx = cx.tcx;
    let name = tcx.item_name(callee);
    let owner = tcx
        .inherent_impl_of_assoc(callee)
        .and_then(|implementation| {
            tcx.type_of(implementation)
                .instantiate_identity()
                .skip_norm_wip()
                .ty_adt_def()
                .map(rustc_middle::ty::AdtDef::did)
        });
    let threaded = |definition: DefId| tcx.def_path_str(definition).starts_with("std::thread::");
    let refused = match (tcx.crate_name(callee.krate).as_str(), owner) {
        ("std", None) => STD_FUNCTIONS.contains(&name.as_str()) && threaded(callee),
        ("std", Some(owner)) => {
            tcx.item_name(owner).as_str() == "Builder"
                && threaded(owner)
                && STD_BUILDER_METHODS.contains(&name.as_str())
        },
        ("rayon_core", None) => RAYON_FUNCTIONS.contains(&name.as_str()),
        ("rayon_core", Some(owner)) => {
            tcx.item_name(owner).as_str() == "ThreadPoolBuilder" && name.as_str() == "new"
        },
        _ => false,
    };
    if !refused {
        return Maybe::Absent(start_reading::Unstarted::OtherFunction);
    }
    Maybe::Present(StartingCall(owner.map_or_else(
        || tcx.def_path_str(callee),
        |owner| format!("{}::{name}", tcx.def_path_str(owner)),
    )))
}
