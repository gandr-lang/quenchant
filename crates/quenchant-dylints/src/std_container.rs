//! A listed crate keeps its containers in the consumer's own types: a `Vec`, a
//! `String` or an `Arc` of `alloc` stands only where its site states why.
//!
//! A wave moves a crate's containers onto the consumer's own types, and each
//! site of those names its allocator. The rule keeps the crate so. It refuses
//! each type that names one of the three, and each call that builds one with
//! no written type: a `vec!`, a `format!`, a `to_string`, a `collect` into an
//! inferred type. A consumer turns the rule on crate by crate.
//!
//! # Boundaries
//!
//! A site that keeps its container states why with
//! `#[expect(std_container, reason = "…")]` at the site or on a node between
//! it and its enclosing item. An expectation on an enclosing module,
//! implementation or crate, one without its reason and an `allow` written in
//! the source are refused. The rule reads the container and not its
//! allocator.
//!
//! The rule does not read a test, as `allocation_free_region` does not: a
//! container in a test is the test's scratch, and no check runs it.
//! `trait_object` reads a test, since a trait object is a shape that a test
//! writes into the code it tests.
//!
//! A type that an external macro writes is not read. A call that an external
//! macro makes is refused at the macro's call, once. A call of a local function
//! is not refused, since the function's signature is. A call is not refused
//! when a refused type stands inside it: the `Vec` of `Vec::new()`, or the
//! turbofish of a `collect`.

use std::collections::HashSet;

use anodized::spec;
use clippy_utils::is_in_test;
use quenchant_shape::shape::Maybe;
use rustc_hir::AmbigArg;
use rustc_hir::Expr;
use rustc_hir::ExprKind;
use rustc_hir::HirId;
use rustc_hir::ItemKind;
use rustc_hir::Node;
use rustc_hir::Ty;
use rustc_hir::TyKind;
use rustc_hir::def::Res;
use rustc_hir::def_id::DefId;
use rustc_hir::def_id::LOCAL_CRATE;
use rustc_lint::LateContext;
use rustc_lint::LateLintPass;
use rustc_lint::Lint;
use rustc_lint::declare_lint;
use rustc_lint::impl_lint_pass;
use rustc_middle::lint::LintLevelSource;
use rustc_middle::ty;
use rustc_middle::ty::TyCtxt;
use rustc_span::Span;
use rustc_span::sym;

use crate::answer_totality::ExceptionRefusals;
use crate::answer_totality::report_defects;
use crate::answer_totality::report_refusing;
use crate::crate_list;
use crate::crate_list::ListKey;
use crate::semantic::CrateName;
use crate::semantic::DiagnosticText;

declare_lint! {
    /// ### What it does
    ///
    /// Refuses, in a crate that `std_container_crates` lists, each type that
    /// names `Vec`, `String` or `Arc` of `alloc`, and each call into another
    /// crate that builds one where no refused type stands.
    ///
    /// ### Why is this bad?
    ///
    /// A wave moves the crate's containers onto the consumer's own types,
    /// whose allocator each site names. A container of `alloc` that comes back
    /// draws from the global allocator with no site that says so.
    ///
    /// ### Configuration
    ///
    /// `std_container_crates` under `[quenchant-dylints]`: each crate whole,
    /// or the start of its name followed by `*`. Without the key the rule reads
    /// nothing.
    ///
    /// ### The exception
    ///
    /// A site that keeps its container states why:
    /// `#[expect(std_container, reason = "...")]` at the site or on a node
    /// between it and its enclosing item. A test is not read.
    ///
    /// ### Example
    ///
    /// ```rust,ignore
    /// fn names(count: usize) -> Vec<Name>;
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// fn names(count: usize) -> SmallVec<Name, 0, Global>;
    /// ```
    pub STD_CONTAINER,
    Deny,
    "a listed crate holds a `Vec`, `String` or `Arc` of `alloc` only under a reasoned expectation at its site"
}

impl_lint_pass!(StdContainer => [STD_CONTAINER]);

quenchant_shape::reason_enum! {
    /// A definition can be none of the containers the rule refuses.
    mod container_reading {
        /// Why the definition is not refused.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Unrefused {
            /// The crate is not listed, or the definition is none of `Vec`,
            /// `String` and `Arc`.
            OtherDefinition,
        }
    }
}

/// The configuration key of the listed crates.
const CRATES_KEY: ListKey = ListKey("std_container_crates");

/// The help every finding carries.
const HELP: &str = concat!(
    "take the crate's own container, with its allocator named at the site; a site that keeps ",
    "this one states why with `#[expect(std_container, reason = \"...\")]`",
);

/// The refusals of an exception the site does not state properly.
const REFUSALS: ExceptionRefusals = ExceptionRefusals {
    inherited: DiagnosticText(concat!(
        "a container exception must be stated where the container stands: an ",
        "`#[expect(std_container, ..)]` on an enclosing module, implementation or crate excepts ",
        "nothing inside it",
    )),
    unreasoned: DiagnosticText(concat!(
        "a container exception must state why the site keeps its container: write ",
        "`reason = \"...\"` on its `#[expect(std_container, ..)]`",
    )),
    allowed: DiagnosticText(concat!(
        "a container of `alloc` in a listed crate is admitted only by ",
        "`#[expect(std_container, reason = \"...\")]`; an `allow` does not say why the site keeps ",
        "it, nor fail when it goes",
    )),
};

/// The containers one crate refuses, and the sites it has refused.
#[derive(Default)]
pub struct StdContainer
{
    /// The definitions of `Vec`, `String` and `Arc` when the crate is listed
    /// and no test; empty otherwise, so the rule reads nothing.
    containers: Vec<DefId>,
    /// The spans of each refused type and call, so a call refuses nothing
    /// that a type in it or the same macro's call already refused.
    refused: HashSet<Span>,
    /// Entries of the configuration that name no crate.
    defects: Vec<String>,
}

/// The name a finding gives a container: its item's name.
#[repr(transparent)]
struct ContainerName(String);

impl StdContainer
{
    /// Read the listed crates as the crate `tcx` compiles reads them.
    ///
    /// # Specification
    /// - requires: `dylint_linting::init_config` has already run, which
    ///   [`crate::register_lints`] guarantees by calling it first.
    /// - ensures: the containers are the diagnostic items `Vec` and `Arc` and
    ///   the lang item `String` exactly when a pattern of
    ///   `std_container_crates` takes this crate and the crate is no test; each
    ///   entry that names no crate is a defect.
    /// - fails: an absent, unreadable or malformed configuration lists no
    ///   crate, which reads nothing.
    /// - panics: none.
    /// - executable: none — the configuration read outcome is not retained;
    ///   reading it again is a distinct fallible operation.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the fixtures list one crate and leave another out;
    ///   the refused configuration carries a number, an empty entry and `*`,
    ///   each reported; a listed test target refuses nothing.
    /// - witness: `tests::ui_std_container`
    /// - witness: `tests::ui_std_container_refused`
    /// - witness: `tests::ui_std_container_test_target`
    pub fn configured(tcx: TyCtxt<'_>) -> Self
    {
        let mut rule = Self::default();
        let crates = crate_list::read(CRATES_KEY, &mut rule.defects);
        let name = tcx.crate_name(LOCAL_CRATE);
        let listed = crates
            .iter()
            .any(|pattern| pattern.matches(CrateName(name.as_str())).0);
        if listed && !tcx.sess.is_test_crate() {
            rule.containers = [sym::Vec, sym::Arc]
                .into_iter()
                .filter_map(|item| tcx.get_diagnostic_item(item))
                .chain(tcx.lang_items().string())
                .collect();
        }
        rule
    }

    /// The name of `definition` when the rule refuses it.
    ///
    /// # Specification
    /// - ensures: `Present` with the item's name exactly when `definition` is
    ///   one of the containers; `Absent` otherwise.
    /// - panics: none.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the fixtures write each of the three containers and a
    ///   `BTreeMap`, which the rule leaves; a wrong set changes a verdict.
    /// - witness: `tests::ui_std_container`
    #[spec(ensures: |output| matches!(output, Maybe::Present(_)) == self.containers.contains(&definition))]
    fn named(
        &self,
        tcx: TyCtxt<'_>,
        definition: DefId,
    ) -> Maybe<ContainerName, container_reading::Unrefused>
    {
        if self.containers.contains(&definition) {
            Maybe::Present(ContainerName(tcx.item_name(definition).to_string()))
        }
        else {
            Maybe::Absent(container_reading::Unrefused::OtherDefinition)
        }
    }
}

impl<'tcx> LateLintPass<'tcx> for StdContainer
{
    /// Report the configuration's defects once per crate.
    ///
    /// # Specification
    /// - ensures: reports each defect [`StdContainer::configured`] recorded at
    ///   the crate root.
    /// - panics: none.
    /// - executable: none — rustc owns diagnostic emission; no readback exists.
    ///
    /// # Adequacy
    /// - hypothesis: L2 — the refused configuration's defects are reported in
    ///   the fixture crate.
    /// - witness: `tests::ui_std_container_refused`
    fn check_crate(
        &mut self,
        cx: &LateContext<'tcx>,
    )
    {
        report_defects(cx, STD_CONTAINER, &self.defects);
    }

    /// Refuse a written type that names a container.
    ///
    /// # Specification
    /// - ensures: reports `ty` exactly when its path resolves to a container,
    ///   it sits in no external macro's expansion and in no test; records its
    ///   span; an exception the site does not state properly is refused at the
    ///   crate root ([`report_refusing`]).
    /// - panics: none.
    /// - executable: none — rustc owns resolution and diagnostic emission.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the fixtures refuse a field, a parameter, a return
    ///   type, a `let` annotation and a turbofish of each container, accept a
    ///   reasoned expectation and a test module, and refuse an unreasoned
    ///   expectation, an inherited one and an `allow`.
    /// - witness: `tests::ui_std_container`
    fn check_ty(
        &mut self,
        cx: &LateContext<'tcx>,
        ty: &'tcx Ty<'tcx, AmbigArg>,
    )
    {
        if self.containers.is_empty() {
            return;
        }
        let ty = ty.as_unambig_ty();
        let TyKind::Path(ref path) = ty.kind
        else {
            return;
        };
        let Res::Def(_, definition) = cx.qpath_res(path, ty.hir_id)
        else {
            return;
        };
        let Maybe::Present(name) = self.named(cx.tcx, definition)
        else {
            return;
        };
        if ty.span.in_external_macro(cx.tcx.sess.source_map()) || is_in_test(cx.tcx, ty.hir_id) {
            return;
        }
        let _fresh = self.refused.insert(ty.span);
        report_refusing(
            cx,
            STD_CONTAINER,
            exception_site(cx, STD_CONTAINER, ty.hir_id),
            ty.span,
            format!(
                "`{}` of `alloc` in a crate whose containers are its own",
                name.0
            ),
            DiagnosticText(HELP),
            &REFUSALS,
        );
    }

    /// Refuse a call into another crate that builds a container where no
    /// refused type stands, once its operands' types are read.
    ///
    /// # Specification
    /// - ensures: reports a call or a method call exactly when its callee is no
    ///   function of this crate, its type is a container, it sits in no test,
    ///   and no span that the rule refused lies inside its span; a call that an
    ///   external macro makes is reported at the macro's call; records the span
    ///   it reports.
    /// - panics: none.
    /// - executable: none — rustc owns typing and diagnostic emission.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the fixtures refuse a `vec!` of a list and of a
    ///   repeat, a `format!`, a `to_string` and a `to_owned` once each; an
    ///   `Arc::new` and a turbofish `collect` once, at their written types; a
    ///   `collect` into an annotated `let` at the call and at the annotation; a
    ///   local helper's call and a `BTreeMap`'s are accepted.
    /// - witness: `tests::ui_std_container`
    fn check_expr_post(
        &mut self,
        cx: &LateContext<'tcx>,
        expr: &'tcx Expr<'tcx>,
    )
    {
        if self.containers.is_empty() {
            return;
        }
        let callee = if let ExprKind::Call(function, _) = expr.kind {
            if let ExprKind::Path(ref path) = function.kind {
                cx.qpath_res(path, function.hir_id).opt_def_id()
            }
            else {
                None
            }
        }
        else if let ExprKind::MethodCall(..) = expr.kind {
            cx.typeck_results().type_dependent_def_id(expr.hir_id)
        }
        else {
            return;
        };
        if callee.is_some_and(DefId::is_local) {
            return;
        }
        let ty::Adt(built, _) = *cx.typeck_results().expr_ty(expr).kind()
        else {
            return;
        };
        let Maybe::Present(name) = self.named(cx.tcx, built.did())
        else {
            return;
        };
        if is_in_test(cx.tcx, expr.hir_id) {
            return;
        }
        let span = if expr.span.in_external_macro(cx.tcx.sess.source_map()) {
            expr.span.source_callsite()
        }
        else {
            expr.span
        };
        if self.refused.iter().any(|seen| span.contains(*seen)) {
            return;
        }
        let _fresh = self.refused.insert(span);
        report_refusing(
            cx,
            STD_CONTAINER,
            exception_site(cx, STD_CONTAINER, expr.hir_id),
            span,
            format!(
                "this call builds a `{}` of `alloc` in a crate whose containers are its own",
                name.0
            ),
            DiagnosticText(HELP),
            &REFUSALS,
        );
    }
}
