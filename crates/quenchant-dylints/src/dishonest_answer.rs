//! A dishonest answer is built only where an item-local expectation states why.
//!
//! A presenter answers a node it cannot spell with a placeholder: the printer's
//! `?`, a hover's unknown, a reason enum naming why nothing better was written.
//! Each is honest about itself and dishonest about the node. The rule makes
//! every place that builds one an enumerated residue: the site carries
//! `#[expect(answer_totality, reason = "…")]` with the reason the honest answer
//! is unavailable, and a new source of the placeholder fails the build.
//!
//! The consumer names the placeholders in its Dylint configuration:
//!
//! ```toml
//! [quenchant-dylints]
//! dishonest_constructors = [
//!   "printer::former::Former::Unreadable",
//!   "printer::former::Unspelled",
//! ]
//! dishonest_spellings = [{ text = "?", crates = ["printer", "hover"] }]
//! ```
//!
//! A constructor entry names an enum variant, an enum (every variant) or a
//! struct by its definition path, crate name first; it is resolved in every
//! crate the definition is visible to, and the crate it names reports it when
//! it resolves to nothing. A spelling entry names a string literal and the
//! crates it is read in: a `?` is a placeholder in a printer and an operator in
//! a parser.
//!
//! What counts as building one: a constructor reached as a value — a unit
//! variant, a tuple constructor called or passed as a function, a struct
//! expression — and the configured literal written as an expression. A pattern
//! builds nothing. An operand of `==` or `!=` asks whether a value is the
//! placeholder rather than answering with one, and is not read. Test code —
//! a `#[test]` function or a `#[cfg(test)]` module — asserts what the
//! presenter answers and is not read.

use std::collections::HashSet;

use anodized::spec;
use clippy_utils::is_in_test;
use quenchant_shape::shape::Maybe;
use rustc_ast::LitKind;
use rustc_hir::BinOpKind;
use rustc_hir::Expr;
use rustc_hir::ExprKind;
use rustc_hir::Node;
use rustc_hir::def::CtorOf;
use rustc_hir::def::DefKind;
use rustc_hir::def::Res;
use rustc_hir::def_id::DefId;
use rustc_hir::def_id::LOCAL_CRATE;
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
use crate::semantic::ComparedOperand;
use crate::semantic::ConfiguredPlaceholder;
use crate::semantic::DiagnosticText;
use crate::semantic::TypePath;

quenchant_shape::reason_enum! {
    /// A resolution can name something other than a constructed definition.
    pub mod construction {
        /// Why the resolution builds no variant or struct.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum NotBuilt {
            /// The path resolves to a local, a function, a constant or any
            /// other definition that constructs nothing.
            NotAConstructor,
        }
    }
}

declare_lint! {
    /// ### What it does
    ///
    /// Reports each place that builds a configured placeholder answer — a
    /// listed constructor reached as a value, or a listed spelling written as a
    /// string literal in a listed crate — unless the site carries
    /// `#[expect(answer_totality, reason = "...")]` (or this lint's name), at
    /// the site or on a node between it and its enclosing item.
    ///
    /// ### Why is this bad?
    ///
    /// A placeholder answers a node the presenter could not spell. Unlisted,
    /// it multiplies silently; enumerated, each residue says why it exists and
    /// fails the build the day it stops being built.
    ///
    /// ### Configuration
    ///
    /// `dishonest_constructors` (definition paths) and `dishonest_spellings`
    /// (`{ text, crates }`) under `[quenchant-dylints]`. Without either key the
    /// lint reads nothing.
    ///
    /// ### Example
    ///
    /// ```rust,ignore
    /// fn spell(node: Node) -> Glyphs { Glyphs("?") }
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// #[expect(answer_totality, reason = "a lift has no surface spelling (#123)")]
    /// fn spell(node: Node) -> Glyphs { Glyphs("?") }
    /// ```
    pub DISHONEST_ANSWER,
    Deny,
    "a placeholder answer is built only under an item-local expectation with its reason"
}

impl_lint_pass!(DishonestAnswer => [DISHONEST_ANSWER]);

/// Help attached to every finding.
const HELP: &str = concat!(
    "write the answer the node deserves, or state why it cannot be had: ",
    "`#[expect(answer_totality, reason = \"...\")]` on the item, statement or arm that builds it",
);

/// The configured placeholder table, as one crate reads it.
#[derive(Default)]
pub struct DishonestAnswer
{
    /// The configured constructors' definitions visible to this crate:
    /// variants, enums and structs.
    resolved: HashSet<DefId>,
    /// The spellings this crate reads.
    spellings: Vec<String>,
    /// Entries the configuration states incompletely or that resolve to
    /// nothing in the crate they name.
    defects: Vec<String>,
}

impl DishonestAnswer
{
    /// Read the placeholder table as the crate `tcx` compiles reads it.
    ///
    /// # Specification
    /// - requires: `dylint_linting::init_config` has already run, which
    ///   [`crate::register_lints`] guarantees by calling it first.
    /// - ensures: every definition a `dishonest_constructors` path resolves to
    ///   is read; a path naming this crate that resolves to nothing, a
    ///   non-string entry, and a `dishonest_spellings` entry without a
    ///   non-empty `text` or a non-empty `crates` list become defects, the last
    ///   in the crates it lists or, listing none, in every crate; the text of
    ///   every spelling whose `crates` name this crate is read.
    /// - fails: an absent, unreadable or malformed configuration yields the
    ///   empty table, which reads nothing.
    /// - panics: none.
    /// - executable: none — the configuration read outcome is not retained;
    ///   reading it again is a distinct fallible operation.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the fixtures read a configured variant, enum, struct
    ///   and spelling; the refused configuration carries a spelling without its
    ///   crates and a misspelt constructor of the fixture crate, each reported,
    ///   while the spelling is not read in a crate its entry omits.
    /// - witness: `tests::ui_answer_totality`
    /// - witness: `tests::ui_answer_totality_refused`
    pub fn configured(tcx: TyCtxt<'_>) -> Self
    {
        let mut table = Self::default();
        let Ok(Some(config)) = dylint_linting::config_toml(CONFIGURATION_TABLE)
        else {
            return table;
        };
        let krate = tcx.crate_name(LOCAL_CRATE);
        for entry in config
            .get("dishonest_constructors")
            .and_then(|value| value.as_array())
            .into_iter()
            .flatten()
        {
            let Some(path) = entry.as_str()
            else {
                table
                    .defects
                    .push("each `dishonest_constructors` entry is a definition path".to_owned());
                continue;
            };
            let found = resolve(tcx, TypePath(path));
            if found.is_empty() && names_local_crate(tcx, TypePath(path)).0 {
                table.defects.push(format!(
                    "`dishonest_constructors` entry `{path}` names no definition of this crate"
                ));
            }
            table.resolved.extend(found);
        }
        for entry in config
            .get("dishonest_spellings")
            .and_then(|value| value.as_array())
            .into_iter()
            .flatten()
        {
            let text = entry.get("text").and_then(|value| value.as_str());
            let crates: Vec<&str> = entry
                .get("crates")
                .and_then(|value| value.as_array())
                .map(|crates| crates.iter().filter_map(|name| name.as_str()).collect())
                .unwrap_or_default();
            let read_here = crates.is_empty() || crates.contains(&krate.as_str());
            match text {
                Some(text) if !text.is_empty() && !crates.is_empty() => {
                    if read_here {
                        table.spellings.push(text.to_owned());
                    }
                },
                _ if read_here => table.defects.push(
                    "each `dishonest_spellings` entry states a non-empty `text` and the `crates` \
                     it is read in"
                        .to_owned(),
                ),
                _ => {},
            }
        }
        table
    }

    /// Whether a built definition is a configured placeholder.
    ///
    /// # Specification
    /// - ensures: answers affirmatively exactly when the definition, or the
    ///   enum a variant belongs to, is in the resolved set.
    /// - panics: none.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — a configured variant is read while its siblings are
    ///   not; every variant of a configured enum is read.
    /// - witness: `tests::ui_answer_totality_refused`
    #[spec(ensures: |output| output.0 == (self.resolved.contains(&definition)
        || (cx.tcx.def_kind(definition) == DefKind::Variant && self.resolved.contains(&cx.tcx.parent(definition)))))]
    fn placeholder(
        &self,
        cx: &LateContext<'_>,
        definition: DefId,
    ) -> ConfiguredPlaceholder
    {
        ConfiguredPlaceholder(
            self.resolved.contains(&definition)
                || (cx.tcx.def_kind(definition) == DefKind::Variant
                    && self.resolved.contains(&cx.tcx.parent(definition))),
        )
    }
}

impl<'tcx> LateLintPass<'tcx> for DishonestAnswer
{
    /// Report the table's defects once per crate.
    ///
    /// # Specification
    /// - ensures: reports each defect [`DishonestAnswer::configured`] recorded
    ///   at the crate root.
    /// - panics: none.
    /// - executable: none — rustc owns diagnostic emission; no readback exists.
    ///
    /// # Adequacy
    /// - hypothesis: L2 — the refused configuration's defects are reported in
    ///   the fixture crate.
    /// - witness: `tests::ui_answer_totality_refused`
    fn check_crate(
        &mut self,
        cx: &LateContext<'tcx>,
    )
    {
        report_defects(cx, DISHONEST_ANSWER, &self.defects);
    }

    /// Report an expression that builds a configured placeholder.
    ///
    /// # Specification
    /// - ensures: reports, through [`report`], a path or struct expression
    ///   resolving to a configured variant, a variant of a configured enum or a
    ///   configured struct, and a string literal equal to a spelling this crate
    ///   reads; an operand of `==` or `!=`, an expression of test code, and an
    ///   expression an external macro expands are not read.
    /// - panics: none.
    /// - executable: none — rustc owns diagnostic emission; no readback exists.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the refused fixture builds a unit variant, a called
    ///   and a passed tuple constructor, a struct-variant expression, a struct
    ///   and the spelling, each reported; the accepted fixture builds them
    ///   under reasoned expectations, matches them in patterns, compares
    ///   against them and asserts them in a test, none reported.
    /// - witness: `tests::ui_answer_totality`
    /// - witness: `tests::ui_answer_totality_refused`
    #[expect(
        clippy::match_same_arms,
        reason = "the path and the struct expression hold their path at different reference \
                  depths, so one pattern cannot bind both"
    )]
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "`ExprKind` and `LitKind` are rustc's, whose variants move with the pinned \
                  nightly; only a path, a struct expression and a string literal build an answer"
    )]
    fn check_expr(
        &mut self,
        cx: &LateContext<'tcx>,
        expr: &'tcx Expr<'tcx>,
    )
    {
        if self.resolved.is_empty() && self.spellings.is_empty() {
            return;
        }
        let res = match expr.kind {
            ExprKind::Path(ref qpath) => cx.qpath_res(qpath, expr.hir_id),
            ExprKind::Struct(qpath, ..) => cx.qpath_res(qpath, expr.hir_id),
            _ => Res::Err,
        };
        let built = match expr.kind {
            ExprKind::Path(_) | ExprKind::Struct(..) => match constructed(cx, res) {
                Maybe::Present(definition) if self.placeholder(cx, definition).0 => {
                    format!("`{}`", cx.tcx.def_path_str(definition))
                },
                Maybe::Present(_) | Maybe::Absent(_) => return,
            },
            ExprKind::Lit(literal) => match literal.node {
                LitKind::Str(symbol, _)
                    if self.spellings.iter().any(|text| text == symbol.as_str()) =>
                {
                    format!("{:?}", symbol.as_str())
                },
                _ => return,
            },
            _ => return,
        };
        if expr.span.in_external_macro(cx.tcx.sess.source_map())
            || compared(cx, expr).0
            || is_in_test(cx.tcx, expr.hir_id)
        {
            return;
        }
        report(
            cx,
            DISHONEST_ANSWER,
            expr.hir_id,
            expr.span,
            format!("builds the placeholder answer {built}"),
            DiagnosticText(HELP),
        );
    }
}

/// The variant or struct a resolved path constructs.
///
/// # Specification
/// - ensures: a variant or struct constructor yields its variant or struct; a
///   variant or struct named directly, as a struct expression names it, yields
///   itself; every other resolution yields
///   `construction::NotBuilt::NotAConstructor`.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — unit, tuple and struct variants and a unit struct are
///   each read through their own resolution shape.
/// - witness: `tests::ui_answer_totality_refused`
#[spec(ensures: |output| match output {
    Maybe::Present(definition) => matches!(cx.tcx.def_kind(definition), DefKind::Variant | DefKind::Struct),
    Maybe::Absent(_) => !matches!(res, Res::Def(DefKind::Ctor(..) | DefKind::Variant | DefKind::Struct, _)),
})]
#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "`Res` is rustc's, whose variants move with the pinned nightly; only a constructor, \
              a variant and a struct build a value"
)]
pub fn constructed(
    cx: &LateContext<'_>,
    res: Res,
) -> Maybe<DefId, construction::NotBuilt>
{
    match res {
        Res::Def(DefKind::Ctor(CtorOf::Variant | CtorOf::Struct, _), ctor) => {
            Maybe::Present(cx.tcx.parent(ctor))
        },
        Res::Def(DefKind::Variant | DefKind::Struct, definition) => Maybe::Present(definition),
        _ => Maybe::Absent(construction::NotBuilt::NotAConstructor),
    }
}

/// Whether an expression is an operand of an equality test.
///
/// # Specification
/// - ensures: answers affirmatively exactly when the expression, through any
///   constructor calls it is the callee or an argument of and any borrows of
///   it, is an operand of `==` or `!=`.
/// - provides: the one place a placeholder is read without being answered.
/// - panics: none.
/// - executable: none — the walk is its own specification over the HIR parents;
///   the UI fixtures are the oracle.
///
/// # Adequacy
/// - hypothesis: L3 — a comparison against a unit variant, a called constructor
///   and the spelling is not reported, while the same values bound or returned
///   are.
/// - witness: `tests::ui_answer_totality`
/// - witness: `tests::ui_answer_totality_refused`
#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "`ExprKind` is rustc's, whose variants move with the pinned nightly; only a call, a \
              borrow and a binary operator can stand between a value and its comparison"
)]
fn compared(
    cx: &LateContext<'_>,
    expr: &Expr<'_>,
) -> ComparedOperand
{
    let mut current = expr.hir_id;
    for (parent_id, parent) in cx.tcx.hir_parent_iter(expr.hir_id) {
        let Node::Expr(parent_expr) = parent
        else {
            return ComparedOperand(false);
        };
        match parent_expr.kind {
            ExprKind::Call(callee, _)
                if callee.hir_id == current
                    || matches!(callee.kind, ExprKind::Path(ref qpath)
                        if matches!(cx.qpath_res(qpath, callee.hir_id), Res::Def(DefKind::Ctor(..), _))) =>
            {
                current = parent_id;
            },
            ExprKind::AddrOf(..) => current = parent_id,
            ExprKind::Binary(op, ..) => {
                return ComparedOperand(matches!(op.node, BinOpKind::Eq | BinOpKind::Ne));
            },
            _ => return ComparedOperand(false),
        }
    }
    ComparedOperand(false)
}
