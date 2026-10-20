//! Every variant of a presented enum is named by an arm of its presenters.
//!
//! A presenter dispatches on a node enum — a printer's formers, an editor's
//! node kinds — and writes each variant its own way. Rust's exhaustiveness
//! check is satisfied by a wildcard or a bare binding, which hands a variant
//! added later to whatever the fallback writes, often the placeholder. This
//! rule asks more: every variant is named by some arm pattern of the functions
//! the consumer lists as the enum's presenters, so a new variant is a lint
//! failure naming it until an arm presents it.
//!
//! # Examples
//!
//! ```toml
//! [quenchant-dylints]
//! presenters = [
//!   { enum = "printer::former::Former", functions = [
//!     "printer::walk::Walker::visit",
//!     "printer::walk::Walker::form",
//!   ] },
//! ]
//! ```
//!
//! # Boundaries
//!
//! An entry is checked in the crate its functions name, which defines them;
//! the enum may be foreign. A variant counts as named when a pattern anywhere
//! in the listed functions' bodies, closures included, names it: a tuple, a
//! struct or a unit variant pattern, alone or inside an or-pattern or a tuple.
//! A wildcard and a bare binding name nothing.
//!
//! The table is a Rust enum against Rust functions. A presenter keyed by data —
//! an editor's node kinds read as strings from a grammar table, a hover
//! answering by typed spans rather than by kind — has no such table, and its
//! coverage stays with the runtime sweep that hovers every corpus position.
//!
//! A variant no presenter can write is excepted at the variant, when the enum
//! is local, or at the first listed function otherwise, by
//! `#[expect(answer_totality, reason = "…")]`.

use std::collections::HashMap;
use std::collections::HashSet;

use anodized::spec;
use quenchant_shape::shape::Maybe;
use rustc_hir::Pat;
use rustc_hir::PatExpr;
use rustc_hir::PatExprKind;
use rustc_hir::PatKind;
use rustc_hir::def::DefKind;
use rustc_hir::def_id::DefId;
use rustc_lint::LateContext;
use rustc_lint::LateLintPass;
use rustc_middle::ty::TyCtxt;
use rustc_session::declare_lint;
use rustc_session::impl_lint_pass;

use crate::answer_totality::CONFIGURATION_TABLE;
use crate::answer_totality::names_local_crate;
use crate::answer_totality::report;
use crate::answer_totality::report_defects;
use crate::answer_totality::resolve;
use crate::dishonest_answer::constructed;
use crate::semantic::DiagnosticText;
use crate::semantic::TypePath;

declare_lint! {
    /// ### What it does
    ///
    /// Reports each variant of a configured enum that no arm pattern of its
    /// configured presenter functions names.
    ///
    /// ### Why is this bad?
    ///
    /// A variant reached only through a wildcard or a bare binding is written
    /// by whatever the fallback writes. The presenter compiles, and the new
    /// node answers with the placeholder or with another node's spelling.
    ///
    /// ### Configuration
    ///
    /// `presenters = [{ enum, functions }]` under `[quenchant-dylints]`:
    /// definition paths, crate name first. Without the key the lint reads
    /// nothing.
    ///
    /// ### Example
    ///
    /// ```rust,ignore
    /// match node {
    ///     Node::Unit => "unit",
    ///     _ => "?",
    /// }
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// match node {
    ///     Node::Unit => "unit",
    ///     Node::Pair(..) => "(_, _)",
    /// }
    /// ```
    pub PRESENTER_COVERAGE,
    Deny,
    "every variant of a presented enum is named by an arm of its presenters"
}

impl_lint_pass!(PresenterCoverage => [PRESENTER_COVERAGE]);

/// Help attached to every finding.
const HELP: &str = concat!(
    "write the arm that presents this variant, naming it in its pattern; a variant no presenter ",
    "can answer states why with `#[expect(answer_totality, reason = \"...\")]`",
);

/// One configured presenter entry this crate checks.
struct Presented
{
    /// The enum the functions present.
    enumeration: DefId,
    /// The listed functions this crate defines, in configuration order.
    functions: Vec<DefId>,
}

/// The presenter table as one crate reads it, and the variants its functions
/// name.
#[derive(Default)]
pub struct PresenterCoverage
{
    /// The entries whose functions this crate defines.
    entries: Vec<Presented>,
    /// Every variant a pattern names, by the function whose body holds it.
    named: HashMap<DefId, HashSet<DefId>>,
    /// Entries stated incompletely, or naming a function or an enum this crate
    /// cannot find.
    defects: Vec<String>,
}

impl PresenterCoverage
{
    /// Read the presenter table as the crate `tcx` compiles reads it.
    ///
    /// # Specification
    /// - requires: `dylint_linting::init_config` has already run, which
    ///   [`crate::register_lints`] guarantees by calling it first.
    /// - ensures: an entry with an `enum` and a non-empty `functions` list, one
    ///   of which names this crate, is checked here when its enum resolves to
    ///   an enum and each function to a function of this crate; otherwise that
    ///   entry becomes a defect. An entry naming none of this crate's functions
    ///   is another crate's and is skipped.
    /// - fails: an absent, unreadable or malformed configuration yields the
    ///   empty table.
    /// - panics: none.
    /// - executable: none — the configuration read outcome is not retained;
    ///   reading it again is a distinct fallible operation.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the fixtures check an entry over a local enum with
    ///   two functions, one a method; the refused configuration carries an
    ///   entry whose function does not exist, reported.
    /// - witness: `tests::ui_answer_totality`
    /// - witness: `tests::ui_answer_totality_refused`
    pub fn configured(tcx: TyCtxt<'_>) -> Self
    {
        let mut table = Self::default();
        let Ok(Some(config)) = dylint_linting::config_toml(CONFIGURATION_TABLE)
        else {
            return table;
        };
        for entry in config
            .get("presenters")
            .and_then(|value| value.as_array())
            .into_iter()
            .flatten()
        {
            let enumeration = entry.get("enum").and_then(|value| value.as_str());
            let functions: Vec<TypePath<'_>> = entry
                .get("functions")
                .and_then(|value| value.as_array())
                .map(|functions| {
                    functions
                        .iter()
                        .filter_map(|path| path.as_str().map(TypePath))
                        .collect()
                })
                .unwrap_or_default();
            let Some(enumeration) = enumeration.filter(|_| !functions.is_empty())
            else {
                table.defects.push(
                    "each `presenters` entry states its `enum` and a non-empty `functions` list"
                        .to_owned(),
                );
                continue;
            };
            if !functions.iter().any(|&path| names_local_crate(tcx, path).0) {
                continue;
            }
            match presented(tcx, TypePath(enumeration), &functions) {
                Ok(presented) => table.entries.push(presented),
                Err(defect) => table.defects.push(defect),
            }
        }
        table
    }
}

/// Resolve one entry checked in this crate.
///
/// # Specification
/// - ensures: the entry with its enum and its functions when `enumeration`
///   resolves to an enum and every path of `functions` to a function or method
///   of this crate.
/// - fails: the first path that resolves otherwise, named in the defect.
/// - panics: none.
///
/// # Errors
/// - a defect naming an enum path that resolves to no enum, or a function path
///   that resolves to no function of this crate.
///
/// # Adequacy
/// - hypothesis: L2 — a resolving entry is checked, and an entry naming a
///   missing function is reported with that function's path.
/// - witness: `tests::ui_answer_totality`
/// - witness: `tests::ui_answer_totality_refused`
#[spec(ensures: |output| output.as_ref().is_ok_and(|presented| presented.functions.len() == functions.len()) || output.is_err())]
fn presented(
    tcx: TyCtxt<'_>,
    enumeration: TypePath<'_>,
    functions: &[TypePath<'_>],
) -> Result<Presented, String>
{
    let Some(enum_id) = resolve(tcx, enumeration)
        .into_iter()
        .find(|&id| tcx.def_kind(id) == DefKind::Enum)
    else {
        return Err(format!(
            "`presenters` entry names `{}`, which is no enum",
            enumeration.0
        ));
    };
    let mut resolved = Vec::with_capacity(functions.len());
    for &path in functions {
        let Some(function) = resolve(tcx, path).into_iter().find(|&id| {
            id.is_local() && matches!(tcx.def_kind(id), DefKind::Fn | DefKind::AssocFn)
        })
        else {
            return Err(format!(
                "`presenters` entry for `{}` names `{}`, which is no function of this crate",
                enumeration.0, path.0
            ));
        };
        resolved.push(function);
    }
    Ok(Presented {
        enumeration: enum_id,
        functions: resolved,
    })
}

impl<'tcx> LateLintPass<'tcx> for PresenterCoverage
{
    /// Record the variant a pattern of a presenter function names.
    ///
    /// # Specification
    /// - ensures: when the pattern sits in the body of a listed function, or of
    ///   a closure within one, and is a tuple, struct or path pattern resolving
    ///   to a variant, that variant is recorded against the function; every
    ///   other pattern records nothing.
    /// - panics: none.
    /// - executable: none — the record is pass state read only at crate end;
    ///   the UI fixtures observe it through the findings.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — variants named by tuple, struct and unit patterns, in
    ///   an or-pattern, inside a tuple scrutinee and in a closure count; one
    ///   reached through a wildcard does not.
    /// - witness: `tests::ui_answer_totality`
    /// - witness: `tests::ui_answer_totality_refused`
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "`PatKind` is rustc's, whose variants move with the pinned nightly; only tuple, \
                  struct and path patterns name a variant"
    )]
    fn check_pat(
        &mut self,
        cx: &LateContext<'tcx>,
        pat: &'tcx Pat<'tcx>,
    )
    {
        if self.entries.is_empty() {
            return;
        }
        let owner = cx
            .tcx
            .typeck_root_def_id(cx.tcx.hir_enclosing_body_owner(pat.hir_id).to_def_id());
        if !self
            .entries
            .iter()
            .any(|entry| entry.functions.contains(&owner))
        {
            return;
        }
        let res = match pat.kind {
            PatKind::TupleStruct(ref qpath, ..) | PatKind::Struct(ref qpath, ..) => {
                cx.qpath_res(qpath, pat.hir_id)
            },
            PatKind::Expr(&PatExpr {
                kind: PatExprKind::Path(ref qpath),
                hir_id,
                ..
            }) => cx.qpath_res(qpath, hir_id),
            _ => return,
        };
        if let Maybe::Present(variant) = constructed(cx, res)
            && cx.tcx.def_kind(variant) == DefKind::Variant
        {
            let _fresh = self.named.entry(owner).or_default().insert(variant);
        }
    }

    /// Report every variant no presenter names, and the table's defects.
    ///
    /// # Specification
    /// - requires: every pattern of the crate has been visited.
    /// - ensures: reports each defect at the crate root; then, per entry and in
    ///   declaration order, each variant of its enum that no pattern of its
    ///   functions names, through [`report`], at the variant when the enum is
    ///   local and at the first function otherwise.
    /// - panics: none.
    /// - executable: none — rustc owns diagnostic emission; no readback exists.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the refused fixture leaves one variant to a wildcard
    ///   and another to a bare binding, each reported by name; the accepted
    ///   fixture names every variant across two functions, one variant only in
    ///   the second, and excepts a variant at its definition.
    /// - witness: `tests::ui_answer_totality`
    /// - witness: `tests::ui_answer_totality_refused`
    fn check_crate_post(
        &mut self,
        cx: &LateContext<'tcx>,
    )
    {
        report_defects(cx, PRESENTER_COVERAGE, &self.defects);
        // One entry's named variants, and the rendered list of its presenters
        // once a variant goes unnamed, each in one buffer every entry reuses.
        let mut named: HashSet<DefId> = HashSet::new();
        let mut presenters = String::new();
        for entry in &self.entries {
            named.clear();
            named.extend(
                entry
                    .functions
                    .iter()
                    .filter_map(|function| self.named.get(function))
                    .flatten()
                    .copied(),
            );
            presenters.clear();
            for variant in cx.tcx.adt_def(entry.enumeration).variants() {
                if named.contains(&variant.def_id) {
                    continue;
                }
                if presenters.is_empty() {
                    for (index, &function) in entry.functions.iter().enumerate() {
                        if index != 0 {
                            presenters.push_str(", ");
                        }
                        presenters.push('`');
                        presenters.push_str(&cx.tcx.def_path_str(function));
                        presenters.push('`');
                    }
                }
                let site = variant
                    .def_id
                    .as_local()
                    .or_else(|| entry.functions.first().copied().and_then(DefId::as_local));
                let Some(site) = site
                else {
                    continue;
                };
                report(
                    cx,
                    PRESENTER_COVERAGE,
                    cx.tcx.local_def_id_to_hir_id(site),
                    cx.tcx.def_span(site),
                    format!(
                        "no arm of {presenters} names `{}`",
                        cx.tcx.def_path_str(variant.def_id)
                    ),
                    DiagnosticText(HELP),
                );
            }
        }
    }
}
