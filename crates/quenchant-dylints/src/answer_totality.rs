//! The static half of a presenter's answer-totality guarantee.
//!
//! A presenter — a printer, a hover — promises an answer at every node it is
//! handed and a spelling its grammar reads back. A runtime sweep checks that
//! promise over a corpus. Three rules check what the compiler can see of it in
//! the presenter's own source, each over a table the consumer writes in its
//! Dylint configuration under `[quenchant-dylints]`:
//!
//! - [`crate::dishonest_answer`]: a dishonest answer — a configured
//!   constructor, or a configured spelling such as `"?"` — is built only where
//!   an item-local expectation states why the honest answer is not available;
//! - [`crate::presenter_coverage`]: every variant of a configured node enum is
//!   named by an arm of its presenter functions, never reached through a
//!   wildcard;
//! - [`crate::mold_coverage`]: the rules a production names through a
//!   configured marker equal the rules the grammar's builders define, less a
//!   justified table of rules no presenter writes.
//!
//! The three share the `answer_totality` lint group. Each is also selectable
//! and expectable by its own name.
//!
//! # Boundaries
//!
//! A reported site is excepted by `#[expect(answer_totality, reason = "…")]`,
//! or the rule's own name, written at the site or on a node between it and its
//! enclosing item — a statement, an arm, the function. An expectation on an
//! enclosing module, implementation or crate is inherited, not stated where the
//! answer is built, and is refused; so is an expectation without its reason and
//! an `allow` written in the source. A level set on the command line is the
//! consumer's selection of the rule and is not an exception.

use anodized::spec;
use clippy_utils::diagnostics::span_lint_hir;
use clippy_utils::diagnostics::span_lint_hir_and_then;
use clippy_utils::paths::PathNS;
use clippy_utils::paths::lookup_path_str;
use rustc_data_structures::fx::FxIndexSet;
use rustc_hir::CRATE_HIR_ID;
use rustc_hir::HirId;
use rustc_hir::def_id::DefId;
use rustc_hir::def_id::LOCAL_CRATE;
use rustc_lint::LateContext;
use rustc_lint::Level;
use rustc_middle::lint::LintLevelSource;
use rustc_middle::ty::TyCtxt;
use rustc_session::lint::Lint;
use rustc_span::Span;

use crate::semantic::DiagnosticText;
use crate::semantic::ExpectationOnItem;
use crate::semantic::LocalTableEntry;
use crate::semantic::TypePath;

/// The lint group the three rules share; an exception names it or one rule.
pub const GROUP: &str = "answer_totality";

/// The configuration table the rules read their keys from.
pub const CONFIGURATION_TABLE: &str = env!("CARGO_PKG_NAME");

/// An exception inherited from an enclosing scope is refused.
const INHERITED_MESSAGE: &str = concat!(
    "an answer-totality exception must be stated where the answer is built: an ",
    "`#[expect(answer_totality, ..)]` on an enclosing module, implementation or crate excepts ",
    "nothing inside it",
);

/// An exception without its reason is refused.
const UNREASONED_MESSAGE: &str = concat!(
    "an answer-totality exception must state why the residue exists: write ",
    "`reason = \"...\"` on its `#[expect(answer_totality, ..)]`",
);

/// An `allow` admits a residue without stating it, and is refused.
const ALLOWED_MESSAGE: &str = concat!(
    "an answer-totality residue is admitted only by `#[expect(answer_totality, reason = \"...\")]`; ",
    "an `allow` does not say why the residue exists, nor fail when it goes",
);

/// How the lint level at a reported site stands toward the exception rule.
pub enum SiteException
{
    /// The level is the consumer's selection: no attribute in the source sets
    /// it to `expect` or `allow`.
    Absent,
    /// An `expect` with a reason sits at the site or between it and its
    /// enclosing item.
    Approved,
    /// The site's `expect` carries no reason.
    Unreasoned,
    /// An `expect` on an enclosing module, implementation or crate supplies the
    /// level.
    Inherited,
    /// An `allow` written in the source supplies the level.
    Allowed,
}

/// Report one finding of `lint` at a site, then refuse an exception the site
/// does not state properly.
///
/// # Specification
/// - requires: `hir_id` is the node the finding belongs to and `span` lies
///   within it.
/// - ensures: emits `message` with `help` at `hir_id`, which fulfils an
///   expectation in force there; then, when [`site_exception`] reads the level
///   as inherited, unreasoned or allowed, emits that refusal at the crate root
///   so the site's own level cannot hide it.
/// - panics: none.
/// - executable: none — rustc owns diagnostic emission and suppression; no
///   emitted-diagnostic readback is available here.
///
/// # Adequacy
/// - hypothesis: L3 — the refused fixtures carry an unexpected site, an
///   expectation without its reason, an expectation on an enclosing module and
///   an `allow`, each reported; the accepted fixtures carry reasoned
///   expectations on an item, a statement and an arm, none reported.
/// - witness: `tests::ui_answer_totality`
/// - witness: `tests::ui_answer_totality_refused`
pub fn report(
    cx: &LateContext<'_>,
    lint: &'static Lint,
    hir_id: HirId,
    span: Span,
    message: String,
    help: DiagnosticText<'_>,
)
{
    span_lint_hir_and_then(cx, lint, hir_id, span, message, |diag| {
        diag.help(help.0.to_owned());
    });
    let refusal = match site_exception(cx, lint, hir_id) {
        SiteException::Absent | SiteException::Approved => return,
        SiteException::Inherited => INHERITED_MESSAGE,
        SiteException::Unreasoned => UNREASONED_MESSAGE,
        SiteException::Allowed => ALLOWED_MESSAGE,
    };
    span_lint_hir(cx, lint, CRATE_HIR_ID, span, refusal);
}

/// Report each configuration defect of one rule at the crate root.
///
/// # Specification
/// - ensures: emits each defect, in order, as a finding of `lint` at the
///   crate's own span.
/// - panics: none.
/// - executable: none — rustc owns diagnostic emission; no readback exists.
///
/// # Adequacy
/// - hypothesis: L2 — the refused configuration carries entries missing their
///   required keys and an entry naming no definition, each reported.
/// - witness: `tests::ui_answer_totality_refused`
pub fn report_defects(
    cx: &LateContext<'_>,
    lint: &'static Lint,
    defects: &[String],
)
{
    let span = crate_head(cx);
    for defect in defects {
        span_lint_hir(cx, lint, CRATE_HIR_ID, span, defect.clone());
    }
}

/// The position whole-crate findings are reported at: the crate root's
/// first byte.
///
/// # Specification
/// - ensures: the empty span at the start of the crate root's span.
/// - provides: a location that names the crate's root file without quoting the
///   whole file beneath every finding.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L2 — the refused fixtures' configuration defects and
///   unproduced rules print at line one, column one of the fixture.
/// - witness: `tests::ui_answer_totality_refused`
#[spec(ensures: |output| output.is_empty() && output.lo() == cx.tcx.def_span(LOCAL_CRATE.as_def_id()).lo())]
pub fn crate_head(cx: &LateContext<'_>) -> Span
{
    cx.tcx.def_span(LOCAL_CRATE.as_def_id()).shrink_to_lo()
}

/// How the level of `lint` at `hir_id` stands toward the exception rule.
///
/// # Specification
/// - requires: `hir_id` is a node of the crate being checked.
/// - ensures: [`SiteException::Approved`] exactly when the level is `expect`,
///   set by an attribute with a reason that sits on `hir_id` or on a node
///   between it and its enclosing item, the item included;
///   [`SiteException::Unreasoned`] for such an attribute without a reason;
///   [`SiteException::Inherited`] for an `expect` set anywhere else;
///   [`SiteException::Allowed`] for an `allow` set by an attribute;
///   [`SiteException::Absent`] for every other level.
/// - provides: the gate [`report`] refuses an exception by.
/// - panics: none.
/// - executable: none — the level is the compiler's lint-level map, which a
///   predicate could only read again.
///
/// # Adequacy
/// - hypothesis: L3 — the fixture matrix separates a reasoned expectation on a
///   function, a statement and an arm (approved), one without its reason, one
///   on the enclosing module, and an `allow` (each refused).
/// - witness: `tests::ui_answer_totality`
/// - witness: `tests::ui_answer_totality_refused`
pub fn site_exception(
    cx: &LateContext<'_>,
    lint: &'static Lint,
    hir_id: HirId,
) -> SiteException
{
    let level = cx.tcx.lint_level_spec_at_node(lint, hir_id);
    match (level.level(), level.src) {
        (Level::Expect, LintLevelSource::Node { reason, span, .. }) => {
            if !attribute_at_site(cx, hir_id, span).0 {
                SiteException::Inherited
            }
            else if reason.is_none() {
                SiteException::Unreasoned
            }
            else {
                SiteException::Approved
            }
        },
        (Level::Expect, _) => SiteException::Inherited,
        (Level::Allow, LintLevelSource::Node { .. }) => SiteException::Allowed,
        _ => SiteException::Absent,
    }
}

/// Whether the attribute at `span` sits at the reported site.
///
/// # Specification
/// - requires: `span` is the source of a lint level resolved at `hir_id`.
/// - ensures: answers affirmatively exactly when an attribute of `hir_id`, or
///   of a node between it and its enclosing item, the item included, encloses
///   `span`.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — a reasoned expectation on the function, on a statement
///   and on an arm is at the site; one on the enclosing module is not.
/// - witness: `tests::ui_answer_totality`
/// - witness: `tests::ui_answer_totality_refused`
#[spec(ensures: |output| output.0 == core::iter::once(hir_id)
    .chain(cx.tcx.hir_parent_id_iter(hir_id).take_while(|&parent| parent != CRATE_HIR_ID))
    .take_while(|&node| node.owner == hir_id.owner)
    .any(|node| cx.tcx.hir_attrs(node).iter().any(|attribute| attribute.span().contains(span))))]
fn attribute_at_site(
    cx: &LateContext<'_>,
    hir_id: HirId,
    span: Span,
) -> ExpectationOnItem
{
    // A node and the parents within its owner end at the owner itself: the
    // enclosing function, constant or static, or a nested item's own owner.
    let mut within_owner = core::iter::once(hir_id)
        .chain(
            cx.tcx
                .hir_parent_id_iter(hir_id)
                .take_while(|&parent| parent != CRATE_HIR_ID),
        )
        .take_while(|&node| node.owner == hir_id.owner);
    ExpectationOnItem(within_owner.any(|node| {
        cx.tcx
            .hir_attrs(node)
            .iter()
            .any(|attribute| attribute.span().contains(span))
    }))
}

/// Every definition a configured path names, in either namespace.
///
/// # Specification
/// - ensures: the definitions [`lookup_path_str`] resolves `path` to in the
///   type namespace, then those in the value namespace, each once.
/// - provides: the configured tables' resolution, through private modules of
///   the local crate and the public paths of a dependency.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L2 — the fixtures resolve a variant, an enum, a struct, a
///   private inherent method and a marker function through private modules; a
///   misspelt entry of the local crate resolves to nothing and is reported.
/// - witness: `tests::ui_answer_totality`
/// - witness: `tests::ui_answer_totality_refused`
#[spec(ensures: |output| output.iter().all(|id| lookup_path_str(tcx, PathNS::Type, path.0).contains(id)
    || lookup_path_str(tcx, PathNS::Value, path.0).contains(id)))]
pub fn resolve(
    tcx: TyCtxt<'_>,
    path: TypePath<'_>,
) -> Vec<DefId>
{
    let mut found: FxIndexSet<DefId> = lookup_path_str(tcx, PathNS::Type, path.0)
        .into_iter()
        .collect();
    found.extend(lookup_path_str(tcx, PathNS::Value, path.0));
    found.into_iter().collect()
}

/// Whether a configured path's first segment names the crate being checked.
///
/// # Specification
/// - ensures: answers affirmatively exactly when the text before the path's
///   first `::` is the local crate's name.
/// - provides: the crate that owns a table entry, and so reports it when it
///   resolves to nothing.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L2 — a misspelt entry naming the fixture's own crate is
///   reported there, while entries naming the other fixtures' crates are not.
/// - witness: `tests::ui_answer_totality_refused`
#[spec(ensures: |output| output.0 == (path.0.split("::").next() == Some(tcx.crate_name(LOCAL_CRATE).as_str())))]
pub fn names_local_crate(
    tcx: TyCtxt<'_>,
    path: TypePath<'_>,
) -> LocalTableEntry
{
    LocalTableEntry(path.0.split("::").next() == Some(tcx.crate_name(LOCAL_CRATE).as_str()))
}
