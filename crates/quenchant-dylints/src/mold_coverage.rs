//! The rules a printer's productions name equal the rules its grammar defines.
//!
//! A printer promises to write only what the grammar parses, and the grammar
//! promises every form it molds a way to be written. A production names the
//! rule it spells in code — a marker the walk calls or constructs with the
//! rule's name as a literal argument — so the name is a value the runtime can
//! read and a lint can collect. The grammar's rules are the literal names its
//! builders give their rule constructor. The rule holds the two sets equal:
//!
//! - a production naming a rule no builder defines is reported at the
//!   production;
//! - a rule no production names is reported with its builder's file and line,
//!   unless the consumer's `unproduced` table states why no presenter writes it
//!   — a declaration form a type printer never meets, say.
//!
//! ```toml
//! [quenchant-dylints.productions]
//! marker = "printer::walk::spells"
//! argument = 0
//! rule_constructor = "RuleName"
//! rule_sources = ["/workspace/grammar/src/rules.rs"]
//! unproduced = [
//!   { rule = "shebang", reason = "a printer writes terms; a shebang is no term" },
//! ]
//! ```
//!
//! The check runs in the crate that defines the marker. The printer and the
//! grammar are usually separate crates neither of which depends on the other,
//! so the builders are read as source rather than compiled: each configured
//! file is scanned token by token for the constructor's name applied to one
//! string literal, `RuleName("sum_type")`, outside every item under
//! `#[cfg(test)]`. The files join the crate's dependency information, so an
//! edit to a builder lints the printer again. A path is read as written:
//! absolute, or relative to the directory the compiler runs in.
//!
//! - Alternatives: running the check in a crate that depends on both and
//!   reading the builders' MIR from metadata, which loses the productions'
//!   arms; a comment marker before each arm, which the runtime cannot read; the
//!   grammar's built mold table, which exists only at run time.
//! - Reversal: a grammar crate that exports its rule names as data a dependency
//!   can read, or a printer that depends on the grammar.

use alloc::collections::BTreeMap;
use alloc::collections::BTreeSet;
use std::path::PathBuf;

use anodized::spec;
use clippy_utils::diagnostics::span_lint_hir_and_then;
use clippy_utils::is_in_test;
use quenchant_shape::shape::Maybe;
use rustc_ast::LitKind;
use rustc_hir::CRATE_HIR_ID;
use rustc_hir::Expr;
use rustc_hir::ExprKind;
use rustc_hir::HirId;
use rustc_hir::def::CtorOf;
use rustc_hir::def::DefKind;
use rustc_hir::def::Res;
use rustc_hir::def_id::DefId;
use rustc_lexer::LiteralKind;
use rustc_lexer::TokenKind;
use rustc_lint::LateContext;
use rustc_lint::LateLintPass;
use rustc_middle::ty::TyCtxt;
use rustc_session::declare_lint;
use rustc_session::impl_lint_pass;
use rustc_span::Span;
use rustc_span::Symbol;

use crate::answer_totality::CONFIGURATION_TABLE;
use crate::answer_totality::crate_head;
use crate::answer_totality::names_local_crate;
use crate::answer_totality::report;
use crate::answer_totality::report_defects;
use crate::answer_totality::resolve;
use crate::semantic::DiagnosticText;
use crate::semantic::RuleName;
use crate::semantic::SourceText;
use crate::semantic::TypePath;

quenchant_shape::reason_enum! {
    /// The configuration can leave the rule off for this crate.
    mod production_setting {
        /// Why this crate checks no production set.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Unchecked {
            /// No `productions` table is configured, or its marker names
            /// another crate.
            NotThisCrate,
        }
    }
}

declare_lint! {
    /// ### What it does
    ///
    /// Compares the rule names a printer's productions pass to a configured
    /// marker with the rule names the grammar's builders define, and reports
    /// every difference: a production naming no rule at the production, and
    /// a rule with no production, unless the configuration states why no
    /// presenter writes it.
    ///
    /// ### Why is this bad?
    ///
    /// A production naming no rule writes text the grammar may not read back;
    /// a rule without a production is a form the printer can never write, so
    /// a node of it prints as something else or as the placeholder.
    ///
    /// ### Configuration
    ///
    /// `[quenchant-dylints.productions]`: `marker` (a function or tuple struct
    /// path), `argument` (the index of the rule's name among its arguments),
    /// `rule_constructor` (the name the builders apply to each rule's name),
    /// `rule_sources` (the builders' files) and `unproduced`
    /// (`{ rule, reason }`).
    ///
    /// ### Example
    ///
    /// ```rust,ignore
    /// Former::Product(_) => self.join(Join::Infix(" * ")),
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// Former::Product(_) => self.join(spells("product_type", Join::Infix(" * "))),
    /// ```
    pub MOLD_COVERAGE,
    Deny,
    "the rules a printer's productions name equal the rules its grammar defines"
}

impl_lint_pass!(MoldCoverage => [MOLD_COVERAGE]);

/// Help attached to a production that names no rule.
const UNKNOWN_HELP: &str = concat!(
    "name the rule this production spells exactly as its builder names it; a production of a form ",
    "the grammar lacks is a grammar change, not a printer one",
);

/// Help attached to a rule that no production names.
const UNPRODUCED_HELP: &str = concat!(
    "write the production that spells this rule and name the rule through the marker, or list it ",
    "under `unproduced` with the reason no presenter writes it",
);

/// Help attached to a marker whose rule is not a literal.
const LITERAL_HELP: &str = "pass the rule's name as a string literal, so the set can be read";

/// One rule a builder defines, where it defines it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DefinedRule
{
    /// The rule's name, as the constructor's literal spells it.
    pub name: String,
    /// The byte offset of the constructor's name in the source.
    pub offset: usize,
}

/// One rule a builder defines, with the file and line that define it.
struct Located
{
    /// The builder's file.
    file: PathBuf,
    /// The one-based line of the constructor.
    line: usize,
}

/// One production: the rule it names, where it names it.
struct Production
{
    /// The rule's name.
    rule: String,
    /// The marker's node, at which an exception sits.
    hir_id: HirId,
    /// The rule's literal.
    span: Span,
}

/// The configured production table, checked in the crate defining its marker.
struct Table
{
    /// The marker: a function, or a tuple struct whose constructor is called.
    marker: DefId,
    /// The index of the rule's name among the marker's arguments.
    argument: usize,
    /// The rules the builders define, by name; the first definition kept.
    rules: BTreeMap<String, Located>,
    /// The rules no presenter writes, each with its stated reason.
    unproduced: BTreeSet<String>,
}

/// The production set as one crate checks it.
pub struct MoldCoverage
{
    /// The resolved table, when this crate defines the marker.
    table: Maybe<Table, production_setting::Unchecked>,
    /// Every production the crate's code names.
    productions: Vec<Production>,
    /// Entries stated incompletely, unreadable sources and stale exclusions.
    defects: Vec<String>,
}

impl MoldCoverage
{
    /// Read the production table as the crate `tcx` compiles reads it.
    ///
    /// # Specification
    /// - requires: `dylint_linting::init_config` has already run, which
    ///   [`crate::register_lints`] guarantees by calling it first.
    /// - ensures: when the `productions` table's marker names this crate, the
    ///   marker is resolved, every rule source is read, scanned with
    ///   [`rule_names`] and listed among the crate's file dependencies, and
    ///   every `unproduced` entry with a non-empty `reason` is kept; a missing
    ///   key, an unresolved marker, an unreadable source and an `unproduced`
    ///   entry without its reason become defects.
    /// - fails: an absent, unreadable or malformed configuration, or a marker
    ///   naming another crate, checks nothing here.
    /// - panics: none.
    /// - executable: none — the configuration and the sources are read from
    ///   outside the compilation; reading them again is a distinct fallible
    ///   operation.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the fixtures read a marker function and a marker
    ///   struct, a builder file with a test module, and an `unproduced` table;
    ///   the refused configuration carries an entry without its reason and a
    ///   source that does not exist, each reported.
    /// - witness: `tests::ui_answer_totality`
    /// - witness: `tests::ui_answer_totality_refused`
    pub fn configured(tcx: TyCtxt<'_>) -> Self
    {
        let mut defects = Vec::new();
        let table = read_table(tcx, &mut defects);
        Self {
            table,
            productions: Vec::new(),
            defects,
        }
    }
}

/// Read and resolve the `productions` table, recording its defects.
///
/// # Specification
/// - requires: `dylint_linting::init_config` has already run.
/// - ensures: as [`MoldCoverage::configured`], the table returned exactly when
///   its marker names this crate and resolves to a function or a struct of it.
/// - provides: `production_setting::Unchecked::NotThisCrate` when no table is
///   configured or its marker names another crate or nothing.
/// - panics: none.
/// - executable: none — the configuration and the sources are read from outside
///   the compilation.
///
/// # Adequacy
/// - hypothesis: L3 — as [`MoldCoverage::configured`].
/// - witness: `tests::ui_answer_totality`
/// - witness: `tests::ui_answer_totality_refused`
fn read_table(
    tcx: TyCtxt<'_>,
    defects: &mut Vec<String>,
) -> Maybe<Table, production_setting::Unchecked>
{
    let unchecked = Maybe::Absent(production_setting::Unchecked::NotThisCrate);
    let Ok(Some(config)) = dylint_linting::config_toml(CONFIGURATION_TABLE)
    else {
        return unchecked;
    };
    let Some(productions) = config.get("productions")
    else {
        return unchecked;
    };
    let Some(marker_path) = productions.get("marker").and_then(|value| value.as_str())
    else {
        defects.push("the `productions` table names its `marker`".to_owned());
        return unchecked;
    };
    if !names_local_crate(tcx, TypePath(marker_path)).0 {
        return unchecked;
    }
    let Some(marker) = resolve(tcx, TypePath(marker_path)).into_iter().find(|&id| {
        id.is_local()
            && matches!(
                tcx.def_kind(id),
                DefKind::Fn | DefKind::AssocFn | DefKind::Struct
            )
    })
    else {
        defects.push(format!(
            "`productions` marker `{marker_path}` names no function or struct of this crate"
        ));
        return unchecked;
    };
    let argument = productions
        .get("argument")
        .and_then(|value| value.as_integer().map(usize::try_from))
        .and_then(Result::ok)
        .unwrap_or_default();
    let Some(constructor) = productions
        .get("rule_constructor")
        .and_then(|value| value.as_str())
    else {
        defects.push("the `productions` table names its `rule_constructor`".to_owned());
        return unchecked;
    };
    let mut rules = BTreeMap::new();
    for source in productions
        .get("rule_sources")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str())
    {
        tcx.sess.file_depinfo.lock().insert(Symbol::intern(source));
        let Ok(text) = std::fs::read_to_string(source)
        else {
            defects.push(format!(
                "`productions` rule source `{source}` cannot be read"
            ));
            continue;
        };
        for rule in rule_names(SourceText(&text), RuleName(constructor)) {
            let line = text
                .get(.. rule.offset)
                .map_or(0_usize, |before| before.matches('\n').count())
                .saturating_add(1);
            let _first = rules.entry(rule.name).or_insert_with(|| Located {
                file: PathBuf::from(source),
                line,
            });
        }
    }
    let mut unproduced = BTreeSet::new();
    for entry in productions
        .get("unproduced")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
    {
        let rule = entry.get("rule").and_then(|value| value.as_str());
        let reason = entry.get("reason").and_then(|value| value.as_str());
        match (rule, reason) {
            (Some(rule), Some(reason)) if !reason.trim().is_empty() => {
                let _fresh = unproduced.insert(rule.to_owned());
            },
            (rule, _) => defects.push(format!(
                "each `unproduced` entry states its `rule` and a non-empty `reason` (entry: `{}`)",
                rule.unwrap_or("<no rule>")
            )),
        }
    }
    Maybe::Present(Table {
        marker,
        argument,
        rules,
        unproduced,
    })
}

impl<'tcx> LateLintPass<'tcx> for MoldCoverage
{
    /// Record the rule a production names through the marker.
    ///
    /// # Specification
    /// - ensures: when this crate checks the table, a call of the marker
    ///   function, or of the marker struct's constructor, outside test code
    ///   records the string literal at the configured argument as a production;
    ///   an argument that is no string literal is reported at its span.
    /// - panics: none.
    /// - executable: none — the record is pass state read only at crate end;
    ///   the UI fixtures observe it through the findings.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — productions through a marker function and a marker
    ///   struct count; a marker called with a variable is reported.
    /// - witness: `tests::ui_answer_totality`
    /// - witness: `tests::ui_answer_totality_refused`
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "`Res` and `ExprKind` are rustc's, whose variants move with the pinned nightly; \
                  only a function or a struct constructor is a marker, and only a literal names \
                  a rule"
    )]
    fn check_expr(
        &mut self,
        cx: &LateContext<'tcx>,
        expr: &'tcx Expr<'tcx>,
    )
    {
        let Maybe::Present(ref table) = self.table
        else {
            return;
        };
        let ExprKind::Call(callee, arguments) = expr.kind
        else {
            return;
        };
        let ExprKind::Path(ref qpath) = callee.kind
        else {
            return;
        };
        let called = match cx.qpath_res(qpath, callee.hir_id) {
            Res::Def(DefKind::Fn | DefKind::AssocFn, function) => function,
            Res::Def(DefKind::Ctor(CtorOf::Struct, _), ctor) => cx.tcx.parent(ctor),
            _ => return,
        };
        if called != table.marker || is_in_test(cx.tcx, expr.hir_id) {
            return;
        }
        let Some(argument) = arguments.get(table.argument)
        else {
            return;
        };
        match argument.kind {
            ExprKind::Lit(literal) if let LitKind::Str(symbol, _) = literal.node => {
                self.productions.push(Production {
                    rule: symbol.as_str().to_owned(),
                    hir_id: expr.hir_id,
                    span: argument.span,
                });
            },
            _ => span_lint_hir_and_then(
                cx,
                MOLD_COVERAGE,
                argument.hir_id,
                argument.span,
                "a production names its rule through a literal",
                |diag| {
                    diag.help(LITERAL_HELP);
                },
            ),
        }
    }

    /// Compare the production set with the rule set.
    ///
    /// # Specification
    /// - requires: every expression of the crate has been visited.
    /// - ensures: reports each defect at the crate root; each production naming
    ///   no defined rule through [`report`]; each defined rule that no
    ///   production names and the `unproduced` table omits, in name order, at
    ///   the crate root with its builder's file and line; and each `unproduced`
    ///   entry naming no defined rule, or a rule a production names, as stale.
    /// - panics: none.
    /// - executable: none — rustc owns diagnostic emission; no readback exists.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the refused fixture names a rule no builder defines,
    ///   omits a defined rule, lists a produced rule and an undefined one under
    ///   `unproduced`, and defines a rule only inside a test module, which is
    ///   not read; each mismatch is reported by name. The accepted fixture's
    ///   sets are equal once its `unproduced` rule is set aside.
    /// - witness: `tests::ui_answer_totality`
    /// - witness: `tests::ui_answer_totality_refused`
    fn check_crate_post(
        &mut self,
        cx: &LateContext<'tcx>,
    )
    {
        let Maybe::Present(ref table) = self.table
        else {
            report_defects(cx, MOLD_COVERAGE, &self.defects);
            return;
        };
        let produced: BTreeSet<&str> = self
            .productions
            .iter()
            .map(|production| production.rule.as_str())
            .collect();
        for rule in &table.unproduced {
            if !table.rules.contains_key(rule) {
                self.defects.push(format!(
                    "`unproduced` lists `{rule}`, which no builder defines; remove the entry"
                ));
            }
            else if produced.contains(rule.as_str()) {
                self.defects.push(format!(
                    "`unproduced` lists `{rule}`, which a production names; remove the entry"
                ));
            }
        }
        report_defects(cx, MOLD_COVERAGE, &self.defects);
        for production in &self.productions {
            if table.rules.contains_key(&production.rule) {
                continue;
            }
            report(
                cx,
                MOLD_COVERAGE,
                production.hir_id,
                production.span,
                format!(
                    "this production names the rule `{}`, which no builder defines",
                    production.rule
                ),
                DiagnosticText(UNKNOWN_HELP),
            );
        }
        let crate_span = crate_head(cx);
        for (rule, located) in &table.rules {
            if produced.contains(rule.as_str()) || table.unproduced.contains(rule) {
                continue;
            }
            span_lint_hir_and_then(
                cx,
                MOLD_COVERAGE,
                CRATE_HIR_ID,
                crate_span,
                format!("the rule `{rule}` has no production"),
                |diag| {
                    diag.note(format!(
                        "`{rule}` is defined at {}:{}",
                        located.file.display(),
                        located.line
                    ));
                    diag.help(UNPRODUCED_HELP);
                },
            );
        }
    }
}

/// The rules a builder's source defines through its rule constructor.
///
/// # Specification
/// - ensures: in source order, one entry per occurrence of the identifier
///   `constructor` followed by `(`, one string literal and `)`, outside the
///   item following each `#[cfg(test)]` attribute; the entry carries the
///   literal's contents and the byte offset of the identifier. Comments and
///   string contents never match, because the scan reads tokens.
/// - provides: the rule set [`MOLD_COVERAGE`] compares the productions with.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the tests separate a rule defined in a builder, the same
///   spelling in a comment, in a string, inside a `#[cfg(test)]` module with
///   nested braces and in a `#[cfg(test)]` function, another identifier applied
///   to a literal, and the constructor applied to a non-literal.
/// - witness: `mold_coverage::tests::the_scan_reads_rule_literals_outside_test_items`
/// - witness: `mold_coverage::tests::the_scan_reports_offsets_in_source_order`
#[spec(ensures: |output| output.windows(2).all(|pair| pair.first().map(|rule| rule.offset) < pair.get(1).map(|rule| rule.offset))
    && output.iter().all(|rule| text.0.get(rule.offset ..).is_some_and(|rest| rest.starts_with(constructor.0))))]
#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "`TokenKind` is rustc's lexer's, whose variants move with the pinned nightly; only \
              brackets and `;` bound an item"
)]
pub fn rule_names(
    text: SourceText<'_>,
    constructor: RuleName<'_>,
) -> Vec<DefinedRule>
{
    let mut tokens = Vec::new();
    let mut offset = 0_usize;
    for token in rustc_lexer::tokenize(text.0, rustc_lexer::FrontmatterAllowed::No) {
        let length = usize::try_from(token.len).unwrap_or_default();
        let end = offset.saturating_add(length);
        if !matches!(
            token.kind,
            TokenKind::Whitespace | TokenKind::LineComment { .. } | TokenKind::BlockComment { .. }
        ) {
            tokens.push((
                token.kind,
                text.0.get(offset .. end).unwrap_or_default(),
                offset,
            ));
        }
        offset = end;
    }
    let mut rules = Vec::new();
    let mut index = 0_usize;
    while let Some(&(kind, spelling, start)) = tokens.get(index) {
        let ahead = |step: usize| tokens.get(index.saturating_add(step));
        let test_attribute = [
            TokenKind::Pound,
            TokenKind::OpenBracket,
            TokenKind::Ident,
            TokenKind::OpenParen,
            TokenKind::Ident,
            TokenKind::CloseParen,
            TokenKind::CloseBracket,
        ]
        .iter()
        .enumerate()
        .all(|(step, expected)| ahead(step).is_some_and(|&(found, ..)| found == *expected))
            && ahead(2).is_some_and(|&(_, word, _)| word == "cfg")
            && ahead(4).is_some_and(|&(_, word, _)| word == "test");
        if test_attribute {
            // Skip the item the attribute governs: through the first `;` at
            // depth zero, or through the brace closing a body opened there.
            let mut depth = 0_usize;
            let mut braced = false;
            index = index.saturating_add(7);
            while let Some(&(kind, ..)) = tokens.get(index) {
                index = index.saturating_add(1);
                match kind {
                    TokenKind::OpenBrace if depth == 0 => {
                        braced = true;
                        depth = 1;
                    },
                    TokenKind::OpenParen | TokenKind::OpenBracket | TokenKind::OpenBrace => {
                        depth = depth.saturating_add(1);
                    },
                    TokenKind::CloseParen | TokenKind::CloseBracket | TokenKind::CloseBrace => {
                        depth = depth.saturating_sub(1);
                        if braced && depth == 0 {
                            break;
                        }
                    },
                    TokenKind::Semi if depth == 0 => break,
                    _ => {},
                }
            }
            continue;
        }
        if kind == TokenKind::Ident
            && spelling == constructor.0
            && ahead(1).is_some_and(|&(found, ..)| found == TokenKind::OpenParen)
            && ahead(3).is_some_and(|&(found, ..)| found == TokenKind::CloseParen)
            && let Some(&(
                TokenKind::Literal {
                    kind: LiteralKind::Str { terminated: true },
                    ..
                },
                literal,
                _,
            )) = ahead(2)
            && let Some(name) = literal
                .strip_prefix('"')
                .and_then(|rest| rest.strip_suffix('"'))
        {
            rules.push(DefinedRule {
                name: name.to_owned(),
                offset: start,
            });
        }
        index = index.saturating_add(1);
    }
    rules
}

#[cfg(test)]
mod tests
{
    use super::rule_names;
    use crate::semantic::RuleName;
    use crate::semantic::SourceText;

    /// A builder with a rule, spellings that resemble one and are none, and
    /// test items.
    const BUILDER: &str = r#"
pub fn rules() -> Vec<Rule> {
    // RuleName("in_a_comment")
    let text = "RuleName(\"in_a_string\")";
    let mut rules = vec![rule(RuleName("sum_type"), Regex::seq([tile("+")]))];
    rules.push(rule(Other("not_a_rule"), Regex::empty()));
    rules.push(rule(RuleName(computed), Regex::empty()));
    rules
}

#[cfg(test)]
mod tests {
    fn helper() { if true { RuleName("sentinel"); } }
}

#[cfg(test)]
fn synthetic() -> Rule { rule(RuleName("synthetic"), Regex::empty()) }

fn more() { rule(RuleName("product_type"), Regex::empty()); }
"#;

    #[test]
    fn the_scan_reads_rule_literals_outside_test_items()
    {
        let names: Vec<String> = rule_names(SourceText(BUILDER), RuleName("RuleName"))
            .into_iter()
            .map(|rule| rule.name)
            .collect();
        assert_eq!(names, ["sum_type", "product_type"]);
    }

    #[test]
    fn the_scan_reports_offsets_in_source_order()
    {
        let rules = rule_names(SourceText(BUILDER), RuleName("RuleName"));
        let offsets: Vec<usize> = rules.iter().map(|rule| rule.offset).collect();
        let expected: Vec<usize> = ["RuleName(\"sum_type\")", "RuleName(\"product_type\")"]
            .iter()
            .map(|needle| BUILDER.find(needle).expect("the builder spells the rule"))
            .collect();
        assert_eq!(offsets, expected);
    }
}
