//! Preserve the layout needed to interpret authored evidence sections.
//!
//! Indentation distinguishes a wrapped bullet value from a following paragraph
//! or link-reference definition. Trimming every line would merge those
//! meanings, so the adequacy and judgment readers retain the original
//! indentation.
//!
//! Any heading ends a section, including headings injected by a macro after the
//! author's text. The termination reader shares this boundary while applying
//! its own fixed prose grammar. No reader may complete an incomplete authored
//! statement by borrowing the following section's bullets.

use quenchant_shape::shape::Maybe;
use rustc_hir::Attribute;
use rustc_hir::def_id::LocalDefId;
use rustc_lint::LateContext;

use crate::semantic::IndentedContinuation;
use crate::semantic::RustdocLine;
use crate::semantic::SectionHeading;
use crate::termination::opens_heading;

quenchant_shape::reason_enum! {
    /// A requested section can be missing even when other documentation exists.
    pub mod section_lookup {
        /// Absence at an exact-heading lookup.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Missing {
            /// No line names the requested heading.
            HeadingAbsent,
        }
    }
}

/// Attribute text retains the indentation needed to distinguish continuation
/// from new prose.
///
/// # Specification
/// - requires: `def_id` identifies a crate-local item.
/// - ensures: returns every line of every doc attribute on the item, in source
///   order, with the indentation each line carries in the fragment.
/// - provides: the input both section readers below take.
/// - panics: none.
pub fn indented_rustdoc_lines(
    cx: &LateContext<'_>,
    def_id: LocalDefId,
) -> Vec<String>
{
    let hir_id = cx.tcx.local_def_id_to_hir_id(def_id);
    cx.tcx
        .hir_attrs(hir_id)
        .iter()
        .filter_map(Attribute::doc_str)
        .flat_map(|doc| {
            doc.as_str()
                .lines()
                .map(str::to_owned)
                .collect::<Vec<String>>()
        })
        .collect()
}

/// Bullet folding preserves section ownership and continuation boundaries.
///
/// # Specification
/// - requires: `lines` are the item's rustdoc lines with their indentation, and
///   `heading` is the section's heading exactly as it is written.
/// - ensures: returns `Absent` when no such heading is present; otherwise the
///   section runs from the heading to the next heading of any level, an
///   indented line is appended to the bullet above it, and an unindented
///   non-bullet line — a paragraph, or a link-reference definition trailing the
///   block — belongs to neither bullet and is dropped.
/// - provides: the bullet list every fixed-grammar gate in this crate
///   validates.
/// - provides: `section_lookup::Missing::HeadingAbsent` distinguishes an absent
///   section from a present section containing no bullets.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the tests separate a section terminated by a `#` heading,
///   one terminated by a `##` heading, a wrapped bullet value, and a
///   link-reference definition written after the last bullet.
/// - witness: `adequacy::tests::the_section_ends_at_the_next_heading_of_any_level`
/// - witness: `adequacy::tests::a_wrapped_bullet_value_stays_one_bullet`
/// - witness: `adequacy::tests::a_trailing_link_definition_joins_no_bullet`
/// - witness: `judgement::tests::a_wrapped_direction_value_stays_one_bullet`
pub fn section_lines(
    lines: &[String],
    heading: SectionHeading<'_>,
) -> Maybe<Vec<String>, section_lookup::Missing>
{
    let body = match section_body(lines, heading) {
        | Maybe::Present(body) => body,
        | Maybe::Absent(reason) => return Maybe::Absent(reason),
    };
    let mut bullets: Vec<String> = Vec::new();
    for line in &body {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if is_indented_continuation(RustdocLine::from(line.as_str())).0 {
            let Some(current) = bullets.last_mut()
            else {
                continue;
            };
            current.push(' ');
            current.push_str(trimmed);
            continue;
        }
        if trimmed.starts_with("- ") {
            bullets.push(trimmed.to_owned());
        }
    }
    Maybe::Present(bullets)
}

/// Section extent is preserved before any bullet-specific interpretation.
///
/// # Specification
/// - requires: `lines` are the item's rustdoc lines with their indentation, and
///   `heading` is the section's heading exactly as it is written.
/// - ensures: returns `Absent` when no such heading is present; otherwise every
///   line from below the heading to the next heading of any level, blank lines
///   included and nothing folded.
/// - provides: the section extent [`section_lines`] folds, and the whole body a
///   gate reads when its grammar is not a bullet list — the `trivial` marker of
///   [`crate::specification::SPECIFICATION_PRESENT`] is one bare word.
/// - provides: `section_lookup::Missing::HeadingAbsent` means the exact heading
///   was not found; an empty body remains present.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the tests separate a body terminated by a `#` heading,
///   one terminated by a `##` heading, and a bare-word body no bullet reader
///   would return.
/// - witness: `specification::tests::the_bare_word_is_the_whole_body`
/// - witness: `specification::tests::a_later_block_adds_no_clause_to_the_marker`
pub fn section_body(
    lines: &[String],
    heading: SectionHeading<'_>,
) -> Maybe<Vec<String>, section_lookup::Missing>
{
    let Some(start) = lines.iter().position(|line| line.trim() == heading.0)
    else {
        return Maybe::Absent(section_lookup::Missing::HeadingAbsent);
    };
    let mut body: Vec<String> = Vec::new();
    for line in lines
        .get(start.saturating_add(1_usize) ..)
        .unwrap_or_default()
    {
        if opens_heading(RustdocLine::from(line.trim())).0 {
            break;
        }
        body.push(line.clone());
    }
    Maybe::Present(body)
}

/// Indentation beyond the doc-comment prefix distinguishes a continuation.
///
/// # Specification
/// - requires: `line` is one rustdoc line with its indentation, as it arrives
///   from the doc fragment — a `///` comment contributes one leading space of
///   its own.
/// - ensures: answers affirmatively exactly when the line carries indentation
///   beyond that single leading space.
/// - provides: the rule separating a wrapped bullet value from a new paragraph.
/// - panics: none.
fn is_indented_continuation(line: RustdocLine<'_>) -> IndentedContinuation
{
    let content = line.0.strip_prefix(' ').unwrap_or(line.0);
    IndentedContinuation(content.chars().next().is_some_and(char::is_whitespace))
}
