//! Structured evidence required beside an approved recursive exception.
//!
//! The section requires nonempty `reason`, `measure`, `boundedness`, and
//! `input recursion` bullets in that order. Shape makes a claim inspectable; it
//! does not prove the measure or supply approval inherited from another item.
//!
//! The local call graph can refute `input recursion: none.` when a recursive
//! edge carries parameter-derived data. A non-refuted statement remains scoped
//! to the analysis's known edges and provenance information.
//!
//! Every heading level terminates the section. Otherwise a short authored block
//! could be completed accidentally by unrelated or macro-injected prose that
//! follows it. Counting a fixed number of lines has the same defect.

use std::collections::HashMap;

use quenchant_shape::shape::Maybe;

quenchant_shape::reason_enum! {
    /// Passing this analysis is narrower than proving termination.
    pub mod termination_check {
        /// Why the approved exception has no reportable defect.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Accepted {
            /// The no-input claim is not refuted by the visible local call graph.
            NoInputNotRefuted,
            /// Input recursion is described rather than denied by a none claim.
            DescribedInput,
        }
    }
}

quenchant_shape::reason_enum! {
    /// Termination prose has its own trimmed-line section lookup.
    mod termination_section {
        /// Why no termination bullets can be read.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Missing {
            /// The item's documentation has no exact Termination heading.
            HeadingAbsent,
        }
    }
}

quenchant_shape::reason_enum! {
    /// An input-recursion value is available only after the whole grammar matches.
    mod input_grammar {
        /// Why no admitted input-recursion claim is available.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Mismatch {
            /// The section does not contain the required four bullets.
            BulletCount,
            /// A bullet has the wrong prefix, order, or an empty value.
            MalformedBullet,
        }
    }
}

use rustc_hir::Attribute;
use rustc_hir::def_id::LocalDefId;
use rustc_lint::LateContext;

use crate::callgraph::CallEdge;
use crate::callgraph::FunctionNode;
use crate::callgraph::scc_has_input_derived_recursive_call;
use crate::semantic::BulletHasValue;
use crate::semantic::BulletPrefix;
use crate::semantic::ClaimsNoInputRecursion;
use crate::semantic::DiagnosticText;
use crate::semantic::OpensHeading;
use crate::semantic::RustdocLine;
use crate::semantic::RustdocText;
use crate::semantic::TerminationLine;

/// The heading that opens the termination specification.
const HEADING: &str = "# Termination";

/// Ordered field prefixes define the admitted termination-section grammar.
const REQUIRED_BULLETS: [&str; 4] = [
    "- reason:",
    "- measure:",
    "- boundedness:",
    "- input recursion:",
];

/// Defects preventing an approved recursive exception from meeting its stated
/// obligations.
pub enum TerminationDefect
{
    /// Exception syntax appears without the required termination section.
    SectionMissing,
    /// The authored section fails the ordered nonempty-bullet grammar.
    GrammarViolated,
    /// Visible argument provenance refutes the authored no-input claim.
    InputRecursionRefuted,
}

impl TerminationDefect
{
    /// Each defect selects a diagnostic identifying the unmet obligation.
    ///
    /// # Specification
    /// trivial.
    pub fn message(&self) -> DiagnosticText<'static>
    {
        DiagnosticText(match *self {
            | Self::SectionMissing => {
                "recursion exception is missing its `# Termination` rustdoc section"
            },
            | Self::GrammarViolated => {
                "`# Termination` section must contain exactly the bullets `- reason:`, \
                 `- measure:`, `- boundedness:`, `- input recursion:`, in that order, each with a \
                 value"
            },
            | Self::InputRecursionRefuted => {
                "`- input recursion: none.` is refuted: a call inside this recursive cycle passes \
                 data derived from the function's own parameters"
            },
        })
    }
}

/// Return the defect in `def_id`'s termination specification, if any.
///
/// # Specification
/// - requires: `def_id` is a member of the recursive component `scc`, and the
///   item carries an approved recursion expectation.
/// - ensures: returns `Absent` exactly when a `# Termination` section is
///   present, matches the fixed grammar, and — when it claims `none` — is not
///   refuted by an input-derived call inside `scc`.
/// - provides: the failure reason [`crate::RECURSION_FORBIDDEN`] reports when
///   an exception is claimed but not justified.
/// - provides: `termination_check::Accepted::NoInputNotRefuted` means visible
///   provenance did not refute a none claim; `DescribedInput` means the author
///   described input recursion instead. Neither reason proves termination.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the UI matrix separates a missing section, a truncated
///   section, a section whose bullets are out of order, a section followed by a
///   further heading at `#` and at `##`, and a `none` claim refuted under both
///   plain and punctuated spellings.
/// - witness: `tests::ui`
pub fn termination_defect(
    cx: &LateContext<'_>,
    def_id: LocalDefId,
    scc: &[LocalDefId],
    functions: &HashMap<LocalDefId, FunctionNode>,
    edges: &HashMap<LocalDefId, Vec<CallEdge>>,
) -> Maybe<TerminationDefect, termination_check::Accepted>
{
    let lines = rustdoc_lines(cx, def_id);
    let Maybe::Present(bullets) = section_bullets(&lines)
    else {
        return Maybe::Present(TerminationDefect::SectionMissing);
    };
    let Maybe::Present(input_recursion) = grammar_input_recursion(&bullets)
    else {
        return Maybe::Present(TerminationDefect::GrammarViolated);
    };
    if claims_no_input_recursion(TerminationLine::from(&input_recursion)).0 {
        if scc_has_input_derived_recursive_call(cx, scc, functions, edges).0 {
            return Maybe::Present(TerminationDefect::InputRecursionRefuted);
        }
        return Maybe::Absent(termination_check::Accepted::NoInputNotRefuted);
    }
    Maybe::Absent(termination_check::Accepted::DescribedInput)
}

/// Termination interpretation deliberately discards surrounding line
/// whitespace.
///
/// # Specification
/// - requires: `def_id` identifies a crate-local item.
/// - ensures: returns every doc line of the item, in source order, each trimmed
///   of its surrounding whitespace.
/// - provides: the input of this module's section reader, whose grammar needs
///   no indentation.
/// - panics: none.
fn rustdoc_lines(
    cx: &LateContext<'_>,
    def_id: LocalDefId,
) -> Vec<String>
{
    let hir_id = cx.tcx.local_def_id_to_hir_id(def_id);
    cx.tcx
        .hir_attrs(hir_id)
        .iter()
        .filter_map(Attribute::doc_str)
        .flat_map(|doc| split_doc_lines(RustdocText::from(doc.as_str())))
        .collect()
}

/// Attribute fragments enter the termination grammar as individually trimmed
/// lines.
///
/// # Specification
/// trivial.
fn split_doc_lines(text: RustdocText<'_>) -> Vec<String>
{
    text.0.lines().map(|line| line.trim().to_owned()).collect()
}

/// Continuations fold within the termination section's own heading boundary.
///
/// # Specification
/// - requires: `lines` are the item's rustdoc lines, already trimmed.
/// - ensures: returns `Absent` when no `# Termination` heading is present;
///   otherwise the section runs from the heading to the next heading of any
///   level, and every non-bullet, non-empty line is appended to the bullet
///   above it.
/// - provides: the bullet list [`grammar_input_recursion`] validates.
/// - provides: `termination_section::Missing::HeadingAbsent` distinguishes a
///   missing section from a present but incomplete section.
/// - panics: none.
fn section_bullets(lines: &[String]) -> Maybe<Vec<String>, termination_section::Missing>
{
    let Some(start) = lines.iter().position(|line| line == HEADING)
    else {
        return Maybe::Absent(termination_section::Missing::HeadingAbsent);
    };
    let mut bullets: Vec<String> = Vec::new();
    for line in lines
        .get(start.saturating_add(1_usize) ..)
        .unwrap_or_default()
    {
        if opens_heading(RustdocLine::from(line)).0 {
            break;
        }
        if line.is_empty() {
            continue;
        }
        if line.starts_with("- ") {
            bullets.push(line.clone());
            continue;
        }
        let Some(current) = bullets.last_mut()
        else {
            continue;
        };
        current.push(' ');
        current.push_str(line);
    }
    Maybe::Present(bullets)
}

/// Every heading level can close the section being interpreted.
///
/// # Specification
/// - requires: `line` is one trimmed rustdoc line.
/// - ensures: answers affirmatively for a leading run of one or more `#`
///   followed by a space, and negatively for a bare `#` run or for `#` inside
///   the line.
/// - provides: the terminator of every rustdoc section this crate reads.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the tests separate `# `, `## `, a bare `#`, and a `#`
///   that appears after other text.
/// - witness: `termination::tests::any_heading_level_terminates_the_section`
pub fn opens_heading(line: RustdocLine<'_>) -> OpensHeading
{
    let rest = line.0.trim_start_matches('#');
    OpensHeading(rest.len() < line.0.len() && rest.starts_with(' '))
}

/// The input-recursion value becomes available only after the whole ordered
/// grammar matches.
///
/// # Specification
/// - requires: `bullets` is the folded bullet list of a `# Termination`
///   section.
/// - ensures: returns `Present` exactly when the bullets are the four required
///   ones, in order, with no extras and every value non-empty; the returned
///   string is the trimmed value of the last bullet.
/// - provides: the claim [`claims_no_input_recursion`] classifies.
/// - provides: `input_grammar::Mismatch::BulletCount` means the section's
///   length is wrong; `MalformedBullet` means a required nonempty bullet does
///   not match in its prescribed position.
/// - panics: none.
fn grammar_input_recursion(bullets: &[String]) -> Maybe<String, input_grammar::Mismatch>
{
    let [_, _, _, ref input] = *bullets
    else {
        return Maybe::Absent(input_grammar::Mismatch::BulletCount);
    };
    for (bullet, prefix) in bullets.iter().zip(REQUIRED_BULLETS) {
        if !required_bullet_has_value(TerminationLine::from(bullet), BulletPrefix::from(prefix)).0 {
            return Maybe::Absent(input_grammar::Mismatch::MalformedBullet);
        }
    }
    let [_, _, _, prefix] = REQUIRED_BULLETS;
    let Some(value) = input.get(prefix.len() ..)
    else {
        return Maybe::Absent(input_grammar::Mismatch::MalformedBullet);
    };
    Maybe::Present(value.trim().to_owned())
}

/// A required prefix alone does not supply the field's promised explanation.
///
/// # Specification
/// - requires: `line` is one trimmed rustdoc line and `prefix` is a required
///   bullet's literal prefix.
/// - ensures: answers affirmatively exactly when the line opens with that
///   prefix and the text after it is not whitespace alone.
/// - provides: the value requirement of every required bullet.
/// - panics: none.
fn required_bullet_has_value(
    line: TerminationLine<'_>,
    prefix: BulletPrefix<'_>,
) -> BulletHasValue
{
    BulletHasValue(
        line.0
            .strip_prefix(prefix.0)
            .is_some_and(|value| !value.trim().is_empty()),
    )
}

/// A no-input claim activates provenance-based refutation rather than accepting
/// descriptive recursion.
///
/// # Specification
/// - requires: `value` is the folded value of the `- input recursion:` bullet,
///   so it may carry continuation lines appended after the claim itself.
/// - ensures: answers affirmatively exactly when the value's first word is
///   `none` and nothing but punctuation or end-of-value follows it. Prose that
///   merely begins with the word — `none of the arguments descend` — is not the
///   claim, and no choice of punctuation after the word escapes it.
/// - provides: the trigger for the call-graph refutation.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the tests separate the bare claim, the claim under every
///   terminal punctuation mark, the claim followed by a sentence of prose, and
///   a described recursion that merely opens with the word.
/// - witness: `termination::tests::none_claim_survives_trailing_prose`
/// - witness: `termination::tests::none_claim_is_recognized_with_or_without_a_period`
/// - witness: `termination::tests::none_claim_survives_any_punctuation`
fn claims_no_input_recursion(value: TerminationLine<'_>) -> ClaimsNoInputRecursion
{
    // Punctuation cannot disable refutation: `none?` and `none!` carry the same
    // claim as `none.`. Recognize the word before classifying its continuation.
    let trimmed = value
        .0
        .trim()
        .trim_start_matches(|c: char| !c.is_alphanumeric());
    let word_end = trimmed
        .find(|c: char| !c.is_alphanumeric())
        .unwrap_or(trimmed.len());
    let Some(word) = trimmed.get(.. word_end)
    else {
        return ClaimsNoInputRecursion(false);
    };
    if !word.eq_ignore_ascii_case("none") {
        return ClaimsNoInputRecursion(false);
    }
    // Folded prose may follow a claim across punctuation. An uninterrupted
    // following word instead makes `none` part of a descriptive sentence.
    let rest = trimmed.get(word_end ..).unwrap_or_default().trim_start();
    ClaimsNoInputRecursion(rest.is_empty() || !rest.starts_with(|c: char| c.is_alphanumeric()))
}

#[cfg(test)]
mod tests
{
    use quenchant_shape::shape::Maybe;

    use super::claims_no_input_recursion;
    use super::grammar_input_recursion;
    use super::section_bullets;
    use super::split_doc_lines;
    use crate::semantic::RustdocText;

    /// Fixture lines use the production reader's whitespace interpretation.
    ///
    /// # Specification
    /// trivial.
    fn lines(text: RustdocText<'_>) -> Vec<String>
    {
        split_doc_lines(text)
    }

    #[test]
    fn section_stops_at_the_next_heading()
    {
        let doc = lines(
            r#"
            Summary.

            # Termination
            - reason: r.
            - measure: m.

            # Specifications
            - requires: injected by an attribute macro.
            - ensures: injected by an attribute macro.
            "#
            .into(),
        );
        let bullets = match section_bullets(&doc) {
            | Maybe::Present(lines) => lines,
            | Maybe::Absent(reason) => panic!("expected a section: {reason:?}"),
        };
        assert_eq!(bullets.len(), 2_usize, "the injected block is not absorbed");
        assert!(
            matches!(
                grammar_input_recursion(&bullets),
                Maybe::Absent(super::input_grammar::Mismatch::BulletCount)
            ),
            "a truncated section is rejected rather than completed by injected lines"
        );
    }

    #[test]
    fn wrapped_bullet_values_fold_into_one_bullet()
    {
        let doc = lines(
            r#"
            # Termination
            - reason: the reason continues
              onto a second line.
            - measure: m.
            - boundedness: b.
            - input recursion: none.
            "#
            .into(),
        );
        let bullets = match section_bullets(&doc) {
            | Maybe::Present(lines) => lines,
            | Maybe::Absent(reason) => panic!("expected a section: {reason:?}"),
        };
        assert_eq!(bullets.len(), 4_usize, "continuation lines fold upward");
        assert_eq!(
            grammar_input_recursion(&bullets),
            Maybe::Present("none.".to_owned()),
            "the grammar accepts a wrapped value and returns the last bullet's value"
        );
    }

    #[test]
    fn out_of_order_bullets_are_rejected()
    {
        let doc = lines(
            r#"
            # Termination
            - measure: m.
            - reason: r.
            - boundedness: b.
            - input recursion: none.
            "#
            .into(),
        );
        let bullets = match section_bullets(&doc) {
            | Maybe::Present(lines) => lines,
            | Maybe::Absent(reason) => panic!("expected a section: {reason:?}"),
        };
        assert!(
            matches!(
                grammar_input_recursion(&bullets),
                Maybe::Absent(super::input_grammar::Mismatch::MalformedBullet)
            ),
            "the bullet order is part of the grammar"
        );
    }

    #[test]
    fn empty_bullet_values_are_rejected()
    {
        let doc = lines(
            r#"
            # Termination
            - reason:
            - measure: m.
            - boundedness: b.
            - input recursion: none.
            "#
            .into(),
        );
        let bullets = match section_bullets(&doc) {
            | Maybe::Present(lines) => lines,
            | Maybe::Absent(reason) => panic!("expected a section: {reason:?}"),
        };
        assert!(
            matches!(
                grammar_input_recursion(&bullets),
                Maybe::Absent(super::input_grammar::Mismatch::MalformedBullet)
            ),
            "an empty value is not an explanation"
        );
    }

    #[test]
    fn none_claim_survives_trailing_prose()
    {
        assert!(
            claims_no_input_recursion(
                "none. The value is passed straight back, so the claim is false.".into()
            )
            .0,
            "prose folded in after the claim does not disarm the refutation"
        );
        assert!(
            !claims_no_input_recursion("none of the arguments descend on input.".into()).0,
            "a first sentence that merely starts with the word is not the claim"
        );
    }

    #[test]
    fn none_claim_survives_any_punctuation()
    {
        for value in [
            "none.", "none?", "none!", "none;", "none,", "none —", "`none`.",
        ] {
            assert!(
                claims_no_input_recursion(value.into()).0,
                "punctuation must not turn the claim into a declaration: {value}"
            );
        }
    }

    #[test]
    fn any_heading_level_terminates_the_section()
    {
        let doc = lines(
            r#"
            # Termination
            - reason: the section ends at the subheading below.

            ## Specifications
            - measure: injected below a subheading.
            - boundedness: injected below a subheading.
            - input recursion: structural descent over the input.
            "#
            .into(),
        );
        let bullets = match section_bullets(&doc) {
            | Maybe::Present(lines) => lines,
            | Maybe::Absent(reason) => panic!("expected a section: {reason:?}"),
        };
        assert_eq!(
            bullets.len(),
            1_usize,
            "a `##` heading terminates the section just as `#` does"
        );
        assert!(
            matches!(
                grammar_input_recursion(&bullets),
                Maybe::Absent(super::input_grammar::Mismatch::BulletCount)
            ),
            "an injected subheading block cannot complete a truncated section"
        );
    }

    #[test]
    fn none_claim_is_recognized_with_or_without_a_period()
    {
        assert!(
            claims_no_input_recursion("none.".into()).0,
            "trailing period"
        );
        assert!(claims_no_input_recursion("none".into()).0, "bare word");
        assert!(
            !claims_no_input_recursion("structural descent over the term".into()).0,
            "a described recursion is not a none claim"
        );
    }
}
