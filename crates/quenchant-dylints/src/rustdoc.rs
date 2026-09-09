//! The indentation-preserving reader shared by this crate's rustdoc gates.
//!
//! Two fixed-grammar sections are read off an item's own documentation —
//! `# Adequacy` and `# Judgement` — and both need the same two things: the
//! item's doc lines with their indentation intact, and one section's bullets
//! folded with their continuation lines.
//!
//! The `# Termination` reader trims every line instead, which is enough for a
//! grammar whose bullets are all required and whose values are prose. These two
//! grammars are neither: a section may be followed by an ordinary paragraph or
//! by a markdown link-reference definition, and only indentation separates a
//! wrapped bullet value from a new paragraph, so it is kept.
//!
//! # The section terminates at the next heading
//!
//! Rustdoc-injecting attribute macros append their own heading after author
//! prose. A reader that ran to the end of the doc block would absorb the
//! injected bullets and complete a truncated section with them, so every
//! section here ends at the next heading of any level — the rule
//! [`opens_heading`] states, and the same reader the `# Termination` gate uses
//! for it.

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

/// Return rustdoc lines attached to `def_id`, indentation preserved.
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

/// Return one section's bullets, each folded with its continuation lines.
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

/// Return one section's own lines, indentation preserved and unfolded.
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

/// Return whether a rustdoc line is an indented continuation of the line above.
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
