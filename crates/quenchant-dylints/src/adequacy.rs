//! The `# Adequacy` rustdoc grammar, machine-checked wherever the block exists.
//!
//! The block is the item's test plan. It states which rung of the adequacy
//! ladder carries the item's decision surfaces, and it names the tests a
//! reviewer can apply a mutant against and watch fail. A block that states no
//! rung has named no plan, and a block that names no test has named no
//! evidence — in both cases the prose reads like an obligation and discharges
//! nothing.
//!
//! # What this gate does and does not decide
//!
//! This is a **shape** gate. It checks the grammar of the block wherever an
//! item carries one; whether an item is *required* to carry one stays with
//! review, because "nontrivial and new or substantially refactored" is not a
//! property of the HIR.
//!
//! Whether a named witness resolves to exactly one runnable test needs the
//! workspace's test inventory, which no lint pass can see. That is the G0 gate
//! in `quenchant-gates`, and it reads the same bullets under the same
//! section rule.
//!
//! # The section terminates at the next heading
//!
//! Rustdoc-injecting attribute macros append their own heading after author
//! prose. A reader that ran to the end of the doc block would absorb the
//! injected bullets and complete a truncated section with them, so the section
//! ends at the next heading of any level — the rule [`section_lines`] applies
//! to every fixed-grammar section this crate reads.

use quenchant_shape::shape::Maybe;

quenchant_shape::reason_enum! {
    /// A grammar check may finish without a reportable defect.
    mod grammar_check {
        /// Evidence that no grammar diagnostic is needed.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Accepted {
            /// The hypothesis and witness or declaration-only grammar is satisfied.
            WellFormed,
        }
    }
}

quenchant_shape::reason_enum! {
    /// Witness resolution requires a single quoted value, not surrounding prose.
    mod witness_syntax {
        /// Why a bullet supplies no exact path.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Missing {
            /// The witness prefix and opening backtick are not present.
            PrefixAbsent,
            /// The value lacks a terminal closing backtick.
            ClosingBacktickAbsent,
            /// The quoted value contains no path.
            EmptyPath,
            /// An interior backtick prevents interpreting the value as one path.
            InteriorBacktick,
        }
    }
}

use rustc_hir::ForeignItem;
use rustc_hir::ImplItem;
use rustc_hir::Item;
use rustc_hir::TraitFn;
use rustc_hir::TraitItem;
use rustc_hir::TraitItemKind;
use rustc_hir::def_id::LocalDefId;
use rustc_lint::LateContext;
use rustc_lint::LateLintPass;
use rustc_session::declare_lint;
use rustc_session::impl_lint_pass;
use rustc_span::Span;

use crate::rustdoc::indented_rustdoc_lines;
use crate::rustdoc::section_lines;
use crate::semantic::DiagnosticText;
use crate::semantic::HypothesisValue;
use crate::semantic::NamesLadderRung;
use crate::semantic::RustdocLine;
use crate::semantic::SectionHeading;
use crate::semantic::TraitRequiredMethod;

declare_lint! {
    /// ### What it does
    ///
    /// Checks the grammar of an item's `# Adequacy` rustdoc section wherever
    /// one is present: exactly one `- hypothesis:` bullet naming an adequacy
    /// ladder rung, at least one `- witness:` bullet naming an exact test path,
    /// and `- declaration-only:` — with a reason — only on a required trait
    /// method.
    ///
    /// ### Why is this bad?
    ///
    /// The block is the item's test plan and the reviewer's index into its
    /// evidence. A hypothesis that names no rung has stated no plan; a block
    /// that names no witness has named no evidence; and a `declaration-only`
    /// exemption on an item with a body excuses the very code that needed
    /// witnessing. Each defect reads as a discharged obligation and discharges
    /// nothing, which is worse than an absent block: the absence is visible.
    ///
    /// ### What this gate does not decide
    ///
    /// Whether an item must carry a block at all stays with review. Whether a
    /// named witness resolves to exactly one runnable test in the item's own
    /// crate needs the workspace test inventory, and is the `workflow-gates`
    /// G0 gate.
    ///
    /// ### Example
    ///
    /// ```rust
    /// /// # Adequacy
    /// /// - hypothesis: the boundary cases are covered.
    /// fn parse() {}
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust
    /// /// # Adequacy
    /// /// - hypothesis: L3 — the boundary inputs are enumerated exhaustively
    /// ///   and each is paired with an exact-variant assertion.
    /// /// - witness: `parse::tests::an_empty_input_is_refused`
    /// fn parse() {}
    /// ```
    pub ADEQUACY_BLOCK_GRAMMAR,
    Deny,
    "# Adequacy sections must state one ladder hypothesis and name their witnesses"
}

impl_lint_pass!(WorkflowAdequacy => [ADEQUACY_BLOCK_GRAMMAR]);

/// Late lint pass checking the `# Adequacy` block grammar.
pub struct WorkflowAdequacy;

impl<'tcx> LateLintPass<'tcx> for WorkflowAdequacy
{
    /// Check any item's block, at its own name.
    ///
    /// # Specification
    /// - ensures: reads the block wherever the item carries one; an item is
    ///   never a required trait method here.
    /// - panics: none.
    fn check_item(
        &mut self,
        cx: &LateContext<'tcx>,
        item: &'tcx Item<'tcx>,
    )
    {
        let span = item.kind.ident().map_or(item.span, |ident| ident.span);
        check_block(cx, item.owner_id.def_id, span, TraitRequiredMethod(false));
    }

    /// Check an associated item's block, at its own name.
    ///
    /// # Specification
    /// - ensures: reads the block wherever the item carries one; an implemented
    ///   item always has a body, so the declaration-only exemption is refused.
    /// - panics: none.
    fn check_impl_item(
        &mut self,
        cx: &LateContext<'tcx>,
        impl_item: &'tcx ImplItem<'tcx>,
    )
    {
        check_block(
            cx,
            impl_item.owner_id.def_id,
            impl_item.ident.span,
            TraitRequiredMethod(false),
        );
    }

    /// Check a trait item's block, at its own name.
    ///
    /// # Specification
    /// - ensures: reads the block wherever the item carries one, admitting the
    ///   declaration-only exemption exactly on a method declared without a
    ///   body.
    /// - panics: none.
    fn check_trait_item(
        &mut self,
        cx: &LateContext<'tcx>,
        trait_item: &'tcx TraitItem<'tcx>,
    )
    {
        let required = TraitRequiredMethod(matches!(
            trait_item.kind,
            TraitItemKind::Fn(_, TraitFn::Required(_))
        ));
        check_block(
            cx,
            trait_item.owner_id.def_id,
            trait_item.ident.span,
            required,
        );
    }

    /// Check a foreign item's block, at its own name.
    ///
    /// # Specification
    /// - ensures: reads the block wherever the item carries one; a foreign
    ///   declaration is not a trait method, so the exemption is refused.
    /// - panics: none.
    fn check_foreign_item(
        &mut self,
        cx: &LateContext<'tcx>,
        item: &'tcx ForeignItem<'tcx>,
    )
    {
        check_block(
            cx,
            item.owner_id.def_id,
            item.ident.span,
            TraitRequiredMethod(false),
        );
    }
}

/// Why an `# Adequacy` block fails the fixed grammar.
enum AdequacyDefect
{
    /// The block states no hypothesis at all.
    HypothesisMissing,
    /// The block states more than one hypothesis.
    HypothesisDuplicated,
    /// The hypothesis names no rung of the adequacy ladder.
    HypothesisNamesNoRung,
    /// A witness or exemption bullet precedes the hypothesis.
    HypothesisNotFirst,
    /// The block names no witness and claims no exemption.
    WitnessMissing,
    /// A `- witness:` bullet names no single backticked path.
    WitnessMalformed,
    /// The exemption sits on an item that is not a required trait method.
    ExemptionNotDeclaration,
    /// The exemption states no reason.
    ExemptionUnreasoned,
    /// The exemption is combined with witnesses.
    ExemptionWithWitness,
    /// The exemption appears more than once.
    ExemptionDuplicated,
}

impl AdequacyDefect
{
    /// Return the diagnostic text for this defect.
    ///
    /// # Specification
    /// trivial.
    fn message(&self) -> DiagnosticText<'static>
    {
        DiagnosticText(match *self {
            | Self::HypothesisMissing => {
                "`# Adequacy` section must open with exactly one `- hypothesis:` bullet naming an \
                 adequacy ladder rung (`L0` types, `L1` evidence, `L2` agreement, `L3` pointwise)"
            },
            | Self::HypothesisDuplicated => {
                "`# Adequacy` section must state exactly one `- hypothesis:`; one item has one test \
                 plan"
            },
            | Self::HypothesisNamesNoRung => {
                "`- hypothesis:` must name at least one adequacy ladder rung — `L0` types, `L1` \
                 evidence, `L2` agreement, `L3` pointwise — because the rung is what states where \
                 the decision surface is carried"
            },
            | Self::HypothesisNotFirst => {
                "`- hypothesis:` must precede every `- witness:` and `- declaration-only:` bullet: \
                 the plan is stated before the evidence that discharges it"
            },
            | Self::WitnessMissing => {
                "`# Adequacy` section must name at least one `- witness: `path`` bullet, or — on a \
                 required trait method only — carry `- declaration-only:` with a reason"
            },
            | Self::WitnessMalformed => {
                "`- witness:` must name exactly one test path in backticks, as `- witness: \
                 `module::tests::name``; the witness-resolution gate resolves the path verbatim"
            },
            | Self::ExemptionNotDeclaration => {
                "`- declaration-only:` is available only to a required trait method, which has no \
                 body to witness; every other item names the tests that kill its mutants"
            },
            | Self::ExemptionUnreasoned => {
                "`- declaration-only:` must state why the declaration carries no witness of its own"
            },
            | Self::ExemptionWithWitness => {
                "`- declaration-only:` claims there is nothing to witness, so it cannot be combined \
                 with `- witness:` bullets"
            },
            | Self::ExemptionDuplicated => "`- declaration-only:` may appear at most once",
        })
    }
}

/// Check the `# Adequacy` block on one item, when it carries one.
///
/// # Specification
/// - requires: `def_id` identifies a crate-local item, `span` is its own name
///   span, and `required` states whether it is a required trait method.
/// - ensures: reports the section's first grammar defect exactly when the item
///   carries an `# Adequacy` heading and that section fails the grammar; an
///   item with no such heading is reported nothing.
/// - provides: the reporting half of [`ADEQUACY_BLOCK_GRAMMAR`].
/// - panics: none.
fn check_block(
    cx: &LateContext<'_>,
    def_id: LocalDefId,
    span: Span,
    required: TraitRequiredMethod,
)
{
    let lines = indented_rustdoc_lines(cx, def_id);
    let Maybe::Present(section) = section_lines(&lines, SectionHeading::from(HEADING))
    else {
        return;
    };
    let Maybe::Present(defect) = grammar_defect(&section, required)
    else {
        return;
    };
    clippy_utils::diagnostics::span_lint(cx, ADEQUACY_BLOCK_GRAMMAR, span, defect.message().0);
}

/// The heading that opens the adequacy block.
const HEADING: &str = "# Adequacy";

/// Return the grammar defect in one `# Adequacy` section, if any.
///
/// # Specification
/// - requires: `bullets` is the folded bullet list of an `# Adequacy` section,
///   and `required` states whether the documented item is a required method of
///   a trait declaration.
/// - ensures: returns `Absent` exactly when the section states one rung-naming
///   hypothesis first and then either at least one exactly-spelled witness or,
///   on a required trait method alone, one reasoned `- declaration-only:`
///   bullet and no witness.
/// - provides: the denial [`ADEQUACY_BLOCK_GRAMMAR`] reports.
/// - provides: `grammar_check::Accepted::WellFormed` means the complete section
///   satisfies this grammar, not that its adequacy claim is proved.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the UI matrix separates each way of breaking the grammar
///   one at a time: no hypothesis, two hypotheses, a hypothesis naming no rung,
///   a hypothesis after the evidence, no witness, a witness that is not an
///   exact path, and the exemption misused four ways.
/// - witness: `tests::ui`
fn grammar_defect(
    bullets: &[String],
    required: TraitRequiredMethod,
) -> Maybe<AdequacyDefect, grammar_check::Accepted>
{
    let mut hypotheses = 0_usize;
    let mut witnesses = 0_usize;
    let mut exemptions = 0_usize;
    // Defer ordering classification until absence of a hypothesis can be
    // distinguished from a hypothesis that occurs too late.
    let mut evidence_before_hypothesis = false;
    for bullet in bullets {
        if let Some(value) = bullet.strip_prefix("- hypothesis:") {
            hypotheses = hypotheses.saturating_add(1_usize);
            if hypotheses > 1_usize {
                return Maybe::Present(AdequacyDefect::HypothesisDuplicated);
            }
            if !names_ladder_rung(HypothesisValue::from(value)).0 {
                return Maybe::Present(AdequacyDefect::HypothesisNamesNoRung);
            }
            continue;
        }
        if bullet.starts_with("- witness:") {
            if matches!(exact_witness(RustdocLine::from(bullet)), Maybe::Absent(_)) {
                return Maybe::Present(AdequacyDefect::WitnessMalformed);
            }
            if exemptions > 0_usize {
                return Maybe::Present(AdequacyDefect::ExemptionWithWitness);
            }
            if hypotheses == 0_usize {
                evidence_before_hypothesis = true;
            }
            witnesses = witnesses.saturating_add(1_usize);
            continue;
        }
        if let Some(reason) = bullet.strip_prefix("- declaration-only:") {
            exemptions = exemptions.saturating_add(1_usize);
            if exemptions > 1_usize {
                return Maybe::Present(AdequacyDefect::ExemptionDuplicated);
            }
            if !required.0 {
                return Maybe::Present(AdequacyDefect::ExemptionNotDeclaration);
            }
            if witnesses > 0_usize {
                return Maybe::Present(AdequacyDefect::ExemptionWithWitness);
            }
            if reason.trim().is_empty() {
                return Maybe::Present(AdequacyDefect::ExemptionUnreasoned);
            }
            if hypotheses == 0_usize {
                evidence_before_hypothesis = true;
            }
        }
    }
    if hypotheses == 0_usize {
        return Maybe::Present(AdequacyDefect::HypothesisMissing);
    }
    if evidence_before_hypothesis {
        return Maybe::Present(AdequacyDefect::HypothesisNotFirst);
    }
    if witnesses == 0_usize && exemptions == 0_usize {
        return Maybe::Present(AdequacyDefect::WitnessMissing);
    }
    Maybe::Absent(grammar_check::Accepted::WellFormed)
}

/// Return whether a hypothesis value names a rung of the adequacy ladder.
///
/// # Specification
/// - requires: `value` is the folded value of the `- hypothesis:` bullet.
/// - ensures: answers affirmatively exactly when some maximal alphanumeric run
///   in the value is `L0`, `L1`, `L2` or `L3`. Taking the token on its own
///   boundary keeps `L2` out of `HL21` and finds the rung inside `` `L3` `` or
///   `(L1)` alike.
/// - provides: the rung requirement of the hypothesis bullet.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the tests separate a bare rung, a rung inside backticks
///   and parentheses, every rung of the ladder, and an alphanumeric run that
///   merely contains a rung's spelling.
/// - witness: `adequacy::tests::a_rung_is_found_on_its_own_token_boundary`
fn names_ladder_rung(value: HypothesisValue<'_>) -> NamesLadderRung
{
    NamesLadderRung(
        value
            .0
            .split(|character: char| !character.is_ascii_alphanumeric())
            .any(|token| matches!(token, "L0" | "L1" | "L2" | "L3")),
    )
}

/// Return the exact path a `- witness:` bullet names.
///
/// # Specification
/// - requires: `bullet` is a folded `- witness:` bullet.
/// - ensures: returns the path exactly when the value is one nonempty
///   backtick-delimited run holding no further backtick; prose, a bare path and
///   two paths on one bullet all yield nothing.
/// - provides: the exactness requirement the witness-resolution gate depends
///   on, since it resolves the path verbatim against the test inventory.
/// - provides: `witness_syntax::Missing` separates `PrefixAbsent`,
///   `ClosingBacktickAbsent`, `EmptyPath`, and `InteriorBacktick`, retaining
///   which part of the exact-value grammar did not match.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the tests separate an exact path, an unquoted path, an
///   empty pair of backticks, a bullet carrying prose after the path, and two
///   paths on one bullet.
/// - witness: `adequacy::tests::only_one_backticked_path_is_an_exact_witness`
fn exact_witness(bullet: RustdocLine<'_>) -> Maybe<RustdocLine<'_>, witness_syntax::Missing>
{
    let Some(rest) = bullet.0.trim().strip_prefix("- witness: `")
    else {
        return Maybe::Absent(witness_syntax::Missing::PrefixAbsent);
    };
    let Some(witness) = rest.strip_suffix('`')
    else {
        return Maybe::Absent(witness_syntax::Missing::ClosingBacktickAbsent);
    };
    if witness.is_empty() {
        return Maybe::Absent(witness_syntax::Missing::EmptyPath);
    }
    if witness.contains('`') {
        return Maybe::Absent(witness_syntax::Missing::InteriorBacktick);
    }
    Maybe::Present(RustdocLine::from(witness))
}

#[cfg(test)]
mod tests
{
    use quenchant_shape::shape::Maybe;

    use super::HEADING;
    use super::exact_witness;
    use super::grammar_defect;
    use super::names_ladder_rung;
    use crate::rustdoc::section_lines;
    use crate::semantic::RustdocLine;
    use crate::semantic::SectionHeading;
    use crate::semantic::TraitRequiredMethod;

    /// Build a doc block the way a `///` comment reaches the pass: one leading
    /// space, and whatever further indentation the author wrote.
    ///
    /// # Specification
    /// trivial.
    fn block(lines: &[RustdocLine<'_>]) -> Vec<String>
    {
        lines.iter().map(|line| format!(" {}", line.0)).collect()
    }

    /// Read the `# Adequacy` section of a doc block, as [`super::check_block`]
    /// reads a real one.
    ///
    /// # Specification
    /// trivial.
    fn adequacy_section(lines: &[String]) -> Vec<String>
    {
        match section_lines(lines, SectionHeading::from(HEADING)) {
            | Maybe::Present(lines) => lines,
            | Maybe::Absent(reason) => panic!("expected a section: {reason:?}"),
        }
    }

    #[test]
    fn the_section_ends_at_the_next_heading_of_any_level()
    {
        let hash = block(&[
            RustdocLine("# Adequacy"),
            RustdocLine("- hypothesis: L3 — enumerated."),
            RustdocLine(""),
            RustdocLine("# Specifications"),
            RustdocLine("- witness: `injected::by::a::macro`"),
        ]);
        let bullets = adequacy_section(&hash);
        assert_eq!(bullets.len(), 1_usize, "a `#` heading terminates the block");
        assert!(
            matches!(
                grammar_defect(&bullets, TraitRequiredMethod(false)),
                Maybe::Present(_)
            ),
            "so an injected witness cannot complete a section that named none"
        );

        let subheading = block(&[
            RustdocLine("# Adequacy"),
            RustdocLine("- hypothesis: L3 — enumerated."),
            RustdocLine(""),
            RustdocLine("## Specifications"),
            RustdocLine("- witness: `injected::below::a::subheading`"),
        ]);
        let bullets = adequacy_section(&subheading);
        assert_eq!(
            bullets.len(),
            1_usize,
            "and a `##` heading terminates it just as `#` does"
        );
    }

    #[test]
    fn a_wrapped_bullet_value_stays_one_bullet()
    {
        let doc = block(&[
            RustdocLine("# Adequacy"),
            RustdocLine("- hypothesis: the plan continues"),
            RustdocLine("  onto a second line, where the rung L2 is finally named."),
            RustdocLine("- witness: `oracle::agrees_with_the_reference`"),
        ]);
        let bullets = adequacy_section(&doc);
        assert_eq!(bullets.len(), 2_usize, "continuation lines fold upward");
        assert!(
            matches!(
                grammar_defect(&bullets, TraitRequiredMethod(false)),
                Maybe::Absent(_)
            ),
            "and a rung named on the continuation line still counts"
        );
    }

    #[test]
    fn a_trailing_link_definition_joins_no_bullet()
    {
        let doc = block(&[
            RustdocLine("# Adequacy"),
            RustdocLine("- hypothesis: L2 — the oracle agrees on generated terms."),
            RustdocLine("- witness: `level_oracle::prop_eval_agrees_with_reference`"),
            RustdocLine(""),
            RustdocLine("[`validate_refutation`]: crate::validate_refutation"),
        ]);
        let bullets = adequacy_section(&doc);
        assert_eq!(
            bullets.len(),
            2_usize,
            "an unindented paragraph after the last bullet is not part of it"
        );
        assert!(
            matches!(
                grammar_defect(&bullets, TraitRequiredMethod(false)),
                Maybe::Absent(_)
            ),
            "so a link-reference definition does not corrupt the witness it follows"
        );
    }

    #[test]
    fn a_rung_is_found_on_its_own_token_boundary()
    {
        for value in [
            " L0 — types.",
            " `L1` evidence.",
            " (L2) agreement.",
            " L3.",
        ] {
            assert!(
                names_ladder_rung(value.into()).0,
                "the rung is found whatever punctuation surrounds it: {value}"
            );
        }
        for value in [" HL21 is not a rung.", " level 3.", " the ladder."] {
            assert!(
                !names_ladder_rung(value.into()).0,
                "and a run that merely contains the spelling is not the rung: {value}"
            );
        }
    }

    #[test]
    fn only_one_backticked_path_is_an_exact_witness()
    {
        assert!(
            matches!(
                exact_witness("- witness: `module::tests::name`".into()),
                Maybe::Present(_)
            ),
            "one backticked path is the witness"
        );
        for bullet in [
            "- witness: module::tests::name",
            "- witness: ``",
            "- witness: `module::tests::name` and also the other one",
            "- witness: `first` `second`",
        ] {
            assert!(
                matches!(exact_witness(bullet.into()), Maybe::Absent(_)),
                "and nothing else is, since the resolver reads the path verbatim: {bullet}"
            );
        }
    }

    #[test]
    fn an_exemption_needs_a_reason_and_a_declaration()
    {
        let doc = block(&[
            RustdocLine("# Adequacy"),
            RustdocLine("- hypothesis: L0 — the declaration has no body."),
            RustdocLine("- declaration-only: every implementation is witnessed at its own impl."),
        ]);
        let bullets = adequacy_section(&doc);
        assert!(
            matches!(
                grammar_defect(&bullets, TraitRequiredMethod(true)),
                Maybe::Absent(_)
            ),
            "a reasoned exemption on a required trait method is the sanctioned form"
        );
        assert!(
            matches!(
                grammar_defect(&bullets, TraitRequiredMethod(false)),
                Maybe::Present(_)
            ),
            "and the same block on an item with a body is not"
        );
    }
}
