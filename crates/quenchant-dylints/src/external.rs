//! The third body of a `# Specification` block, `external`: an authority
//! outside the tree states the item's conditions, and the block names it.
//!
//! A block writes `external: <authority>, …; owed by #<n>` as its first
//! paragraph, alone or above its clauses. The clauses state what the tree
//! knows, and the authorities state the rest. A crate admits the body only
//! through its table in the consumer's configuration, which names the
//! patterns its authorities match and the one issue that owes the clauses:
//!
//! ```toml
//! [[quenchant-dylints.external_specification]]
//! crate = "gandr_runtime_uring"
//! authorities = ["io_uring_*(2)", "io_uring_*(3)", "include/uapi/linux/io_uring.h"]
//! owed_by = 7751
//! ```
//!
//! The kinds of authority are the table's patterns, not a rule of this
//! reader: an authority that matches no pattern of its crate is refused,
//! whatever it names.

use quenchant_shape::shape::Maybe;
use rustc_hir::def_id::LOCAL_CRATE;
use rustc_middle::ty::TyCtxt;

use crate::answer_totality::CONFIGURATION_TABLE;
use crate::crate_list::CratePattern;
use crate::semantic::Authority;
use crate::semantic::CrateName;
use crate::semantic::IssueNumber;
use crate::semantic::MatchesPattern;
use crate::semantic::RustdocLine;

/// The configuration key of the crates' tables.
const TABLE_KEY: &str = "external_specification";

/// The word that opens the body's line.
const OPENING: &str = "external:";

/// The text that separates the authorities from the owing issue.
const OWED_BY: &str = "; owed by #";

/// An authority's pattern: its text, where `*` stands for any run of
/// characters.
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthorityPattern(pub String);

impl AuthorityPattern
{
    /// Whether the pattern takes the authority `authority`.
    ///
    /// # Specification
    /// - ensures: answers affirmatively exactly when `authority` is the
    ///   pattern's text with each `*` replaced by some run of characters,
    ///   possibly empty; a pattern with no `*` takes its own text alone.
    /// - panics: none.
    /// - executable: none — the match is the definition; a predicate restating
    ///   it would be the match again.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the tests separate a whole name, a pattern with one
    ///   `*` in the middle, a pattern with a `*` at each end, and an authority
    ///   that matches the start of a pattern but not its end.
    /// - witness: `external::tests::a_pattern_takes_its_authorities`
    #[must_use]
    pub fn matches(
        &self,
        authority: Authority<'_>,
    ) -> MatchesPattern
    {
        let mut parts = self.0.split('*');
        let first = parts.next().unwrap_or_default();
        let Some(mut rest) = authority.0.strip_prefix(first)
        else {
            return MatchesPattern(false);
        };
        let middle: Vec<&str> = parts.collect();
        let Some((last, between)) = middle.split_last()
        else {
            return MatchesPattern(rest.is_empty());
        };
        for part in between {
            let Some(at) = rest.find(part)
            else {
                return MatchesPattern(false);
            };
            rest = rest
                .get(at.saturating_add(part.len()) ..)
                .unwrap_or_default();
        }
        MatchesPattern(rest.len() >= last.len() && rest.ends_with(last))
    }
}

/// What one crate's table admits.
#[derive(Clone, Debug)]
pub struct Admitted
{
    /// The patterns the crate's authorities match.
    authorities: Vec<AuthorityPattern>,
    /// The issue that owes the clauses of each such block.
    owed: IssueNumber,
}

/// Why a crate admits no `external` body.
pub mod admission
{
    /// The crate's absence from the configuration's tables.
    #[derive(Clone, Copy, Debug)]
    pub enum Unlisted
    {
        /// No table of the configuration takes the crate.
        NoTable,
    }
}

/// The crate's admission of `external`, and the configuration's defects.
#[derive(Debug)]
pub struct Permit
{
    /// The crate's table, when one takes it.
    admitted: Maybe<Admitted, admission::Unlisted>,
    /// Entries of the configuration that the reader refuses.
    pub defects: Vec<String>,
}

impl Permit
{
    /// Read the tables as the crate `tcx` compiles reads them.
    ///
    /// # Specification
    /// - requires: `dylint_linting::init_config` has already run, which
    ///   [`crate::register_lints`] guarantees by calling it first.
    /// - ensures: admits the patterns and the issue of the first table whose
    ///   `crate` takes this crate; each table that names no crate, no list of
    ///   authorities or no positive issue is a defect, and so is a second table
    ///   that takes this crate.
    /// - fails: an absent, unreadable or malformed configuration admits
    ///   nothing.
    /// - panics: none.
    /// - executable: none — the configuration read outcome is not retained;
    ///   reading it again is a distinct fallible operation.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the fixtures admit one crate by its table and leave
    ///   another with none; the refused configuration carries a table with no
    ///   crate, one with an empty pattern and one with no issue, each reported.
    /// - witness: `tests::ui_external`
    /// - witness: `tests::ui_external_refused`
    ///
    /// # Cost
    /// - allocation: the crate's patterns and the defects' text, setup
    #[must_use]
    pub fn configured(tcx: TyCtxt<'_>) -> Self
    {
        let mut permit = Self {
            admitted: Maybe::Absent(admission::Unlisted::NoTable),
            defects: Vec::new(),
        };
        let Ok(Some(config)) = dylint_linting::config_toml(CONFIGURATION_TABLE)
        else {
            return permit;
        };
        let Some(value) = config.get(TABLE_KEY)
        else {
            return permit;
        };
        let Some(tables) = value.as_array()
        else {
            permit
                .defects
                .push(format!("`{TABLE_KEY}` is no list of tables"));
            return permit;
        };
        let name = tcx.crate_name(LOCAL_CRATE);
        for table in tables {
            let Maybe::Present((pattern, admitted)) = read_table(table, &mut permit.defects)
            else {
                continue;
            };
            if !pattern.matches(CrateName(name.as_str())).0 {
                continue;
            }
            if matches!(permit.admitted, Maybe::Present(_)) {
                permit.defects.push(format!(
                    "a second `{TABLE_KEY}` table takes the crate `{name}`; one table names its \
                     authorities"
                ));
                continue;
            }
            permit.admitted = Maybe::Present(admitted);
        }
        permit
    }
}

/// Why a configuration table yields no admission.
pub mod table_reading
{
    /// The table's defect, which the reader records.
    #[derive(Clone, Copy, Debug)]
    pub enum Refused
    {
        /// The table lacks a field, or a field holds the wrong kind of value.
        Malformed,
    }
}

/// One table of the configuration, read.
///
/// # Specification
/// - ensures: answers the crate's pattern and its admission when `crate` is a
///   crate name or the start of one followed by `*`, `authorities` is a list of
///   nonempty strings, and `owed_by` is a positive integer; otherwise records
///   one defect naming the field and answers `Absent`.
/// - panics: none.
/// - executable: none — the reading is the table's fields in turn.
///
/// # Adequacy
/// - hypothesis: L3 — the refused configuration separates a table with no
///   crate, one with an empty pattern and one with no issue.
/// - witness: `tests::ui_external_refused`
///
/// # Cost
/// - allocation: the patterns and the defect's text, setup
fn read_table(
    table: &toml::Value,
    defects: &mut Vec<String>,
) -> Maybe<(CratePattern, Admitted), table_reading::Refused>
{
    let refused = Maybe::Absent(table_reading::Refused::Malformed);
    let Some(name) = table
        .get("crate")
        .and_then(toml::Value::as_str)
        .filter(|name| !name.is_empty() && *name != "*")
    else {
        defects.push(format!(
            "an `{TABLE_KEY}` table names no `crate`, nor the start of one followed by `*`"
        ));
        return refused;
    };
    let pattern = name.strip_suffix('*').map_or_else(
        || CratePattern::Exact(name.to_owned()),
        |start| CratePattern::Prefix(start.to_owned()),
    );
    let authorities = table
        .get("authorities")
        .and_then(toml::Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .map(|entry| entry.as_str().filter(|text| !text.is_empty()))
                .collect::<Option<Vec<&str>>>()
        });
    let Some(Some(authorities)) =
        authorities.filter(|list| list.as_ref().is_some_and(|list| !list.is_empty()))
    else {
        defects.push(format!(
            "the `{TABLE_KEY}` table of `{name}` names no list of authority patterns, each a \
             nonempty string"
        ));
        return refused;
    };
    let Some(owed) = table
        .get("owed_by")
        .and_then(toml::Value::as_integer)
        .and_then(|number| u64::try_from(number).ok())
        .filter(|number| *number > 0)
    else {
        defects.push(format!(
            "the `{TABLE_KEY}` table of `{name}` names no `owed_by`, the positive number of the \
             issue that owes the clauses"
        ));
        return refused;
    };
    Maybe::Present((pattern, Admitted {
        authorities: authorities
            .into_iter()
            .map(|text| AuthorityPattern(text.to_owned()))
            .collect(),
        owed: IssueNumber(owed),
    }))
}

/// The parts of a `# Specification` body: its `external` paragraph, and the
/// lines that follow it.
#[derive(Debug)]
pub struct Parts
{
    /// The `external:` paragraph, its lines joined, when the body opens with
    /// one.
    pub external: Maybe<String, paragraph::Unopened>,
    /// Every line of the body after that paragraph.
    pub rest: Vec<String>,
}

/// Why a body has no `external` paragraph.
pub mod paragraph
{
    /// The body's first line.
    #[derive(Clone, Copy, Debug)]
    pub enum Unopened
    {
        /// The body's first nonblank line does not open with `external:`.
        OtherLine,
    }
}

/// Split `body` into its `external` paragraph and the lines after it.
///
/// # Specification
/// - requires: `body` is the unfolded body of a `# Specification` section, as
///   [`crate::rustdoc::section_body`] returns it.
/// - ensures: when the first nonblank line opens with `external:`, the
///   paragraph is that line and each following nonblank line up to a blank line
///   or a bullet, joined by single spaces, and `rest` is every line after them;
///   otherwise the paragraph is `Absent` and `rest` is the whole body.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the tests separate the line alone, the line wrapped onto
///   a second line, the line above a clause, and a body that opens with a
///   clause.
/// - witness: `external::tests::the_paragraph_ends_at_a_blank_line_or_a_bullet`
///
/// # Cost
/// - allocation: the joined paragraph and the remaining lines, diagnostic
#[must_use]
pub fn split(body: &[String]) -> Parts
{
    let start = body.iter().position(|line| !line.trim().is_empty());
    let Some(start) = start.filter(|&at| {
        body.get(at)
            .is_some_and(|line| line.trim().starts_with(OPENING))
    })
    else {
        return Parts {
            external: Maybe::Absent(paragraph::Unopened::OtherLine),
            rest: body.to_vec(),
        };
    };
    let mut joined = String::new();
    let mut end = start;
    for line in body.get(start ..).unwrap_or_default() {
        let trimmed = line.trim();
        if end > start && (trimmed.is_empty() || trimmed.starts_with("- ")) {
            break;
        }
        if !joined.is_empty() {
            joined.push(' ');
        }
        joined.push_str(trimmed);
        end = end.saturating_add(1);
    }
    Parts {
        external: Maybe::Present(joined),
        rest: body.get(end ..).unwrap_or_default().to_vec(),
    }
}

/// Whether a line writes the body's word anywhere but as the opening
/// paragraph: alone or behind a bullet.
///
/// # Specification
/// - ensures: answers affirmatively exactly when the trimmed line opens with
///   `external:`, or with a `-` bullet whose value opens with it.
/// - panics: none.
/// - executable: none — the answer is the line's opening.
///
/// # Adequacy
/// - hypothesis: L3 — the fixture writes the word as a bullet and below a
///   clause, each refused.
/// - witness: `tests::ui_external`
#[must_use]
pub fn writes_the_word(line: RustdocLine<'_>) -> MatchesPattern
{
    let text = line.0.trim();
    let value = text.strip_prefix('-').map_or(text, str::trim_start);
    MatchesPattern(value.starts_with(OPENING))
}

/// What an `external` paragraph owes and lacks.
#[derive(Debug, Eq, PartialEq)]
pub enum ExternalDefect
{
    /// No table of the configuration takes the crate.
    NotAdmitted,
    /// The paragraph names no authority, or no owing issue after `; owed by
    /// #`.
    Malformed,
    /// The paragraph names an issue other than its crate's.
    OtherIssue
    {
        /// The issue the crate's table names.
        owed: IssueNumber,
    },
    /// An authority matches no pattern of its crate's table.
    Unlisted
    {
        /// The authority, as the paragraph writes it.
        authority: String,
    },
}

/// The defects of the `external` paragraph `paragraph` under `permit`.
///
/// # Specification
/// - requires: `paragraph` is the joined paragraph [`split`] returns.
/// - ensures: answers `NotAdmitted` alone when no table takes the crate;
///   otherwise `Malformed` alone when the paragraph names no authority or no
///   positive issue after `; owed by #`; otherwise `OtherIssue` when its issue
///   is not the table's, and one `Unlisted` for each authority that matches no
///   pattern of the table.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the tests separate a paragraph the table admits, one with
///   no owing issue, one that names another issue, one with an authority no
///   pattern takes, and one under no table.
/// - witness: `external::tests::a_paragraph_owes_its_crate_s_issue_and_patterns`
///
/// # Cost
/// - allocation: the defects, diagnostic
#[must_use]
pub fn defects(
    paragraph: RustdocLine<'_>,
    permit: &Permit,
) -> Vec<ExternalDefect>
{
    let Maybe::Present(ref admitted) = permit.admitted
    else {
        return vec![ExternalDefect::NotAdmitted];
    };
    let text = paragraph.0.trim();
    let text = text.strip_prefix(OPENING).unwrap_or(text);
    let Some((named, issue)) = text.rsplit_once(OWED_BY)
    else {
        return vec![ExternalDefect::Malformed];
    };
    let issue = issue.trim().trim_end_matches('.');
    let authorities: Vec<&str> = named.split(',').map(str::trim).collect();
    let Some(number) = issue.parse::<u64>().ok().filter(|number| *number > 0)
    else {
        return vec![ExternalDefect::Malformed];
    };
    if authorities.iter().any(|authority| authority.is_empty()) {
        return vec![ExternalDefect::Malformed];
    }
    let mut found = Vec::new();
    if IssueNumber(number) != admitted.owed {
        found.push(ExternalDefect::OtherIssue {
            owed: admitted.owed,
        });
    }
    for authority in authorities {
        if !admitted
            .authorities
            .iter()
            .any(|pattern| pattern.matches(Authority(authority)).0)
        {
            found.push(ExternalDefect::Unlisted {
                authority: authority.to_owned(),
            });
        }
    }
    found
}

#[cfg(test)]
mod tests
{
    use quenchant_shape::shape::Maybe;

    use super::Admitted;
    use super::AuthorityPattern;
    use super::ExternalDefect;
    use super::Permit;
    use super::admission;
    use super::defects;
    use super::split;
    use crate::semantic::Authority;
    use crate::semantic::IssueNumber;
    use crate::semantic::RustdocLine;

    /// A permit of the ring's pages and its header, owed by #7751.
    fn ring() -> Permit
    {
        Permit {
            admitted: Maybe::Present(Admitted {
                authorities: vec![
                    AuthorityPattern("io_uring_*(2)".to_owned()),
                    AuthorityPattern("io_uring_*(3)".to_owned()),
                    AuthorityPattern("include/uapi/linux/io_uring.h".to_owned()),
                ],
                owed: IssueNumber(7751),
            }),
            defects: Vec::new(),
        }
    }

    #[test]
    fn a_pattern_takes_its_authorities()
    {
        let page = AuthorityPattern("io_uring_*(3)".to_owned());
        assert!(page.matches(Authority("io_uring_prep_accept(3)")).0);
        assert!(!page.matches(Authority("io_uring_prep_accept(2)")).0);
        assert!(!page.matches(Authority("liburing/src/queue.c")).0);
        let whole = AuthorityPattern("include/uapi/linux/io_uring.h".to_owned());
        assert!(whole.matches(Authority("include/uapi/linux/io_uring.h")).0);
        assert!(
            !whole
                .matches(Authority("include/uapi/linux/io_uring.h.orig"))
                .0
        );
        let ends = AuthorityPattern("*_enter*".to_owned());
        assert!(ends.matches(Authority("io_uring_enter(2)")).0);
        assert!(!ends.matches(Authority("io_uring_setup(2)")).0);
        let short = AuthorityPattern("io_*_enter".to_owned());
        assert!(!short.matches(Authority("io_enter")).0);
    }

    #[test]
    fn the_paragraph_ends_at_a_blank_line_or_a_bullet()
    {
        let lines = |text: &[&str]| {
            text.iter()
                .map(|line| (*line).to_owned())
                .collect::<Vec<_>>()
        };
        let alone = split(&lines(&[" external: io_uring_enter(2); owed by #7751"]));
        assert!(
            matches!(alone.external, Maybe::Present(ref text) if text == "external: io_uring_enter(2); owed by #7751")
        );
        assert!(alone.rest.is_empty());
        let wrapped = split(&lines(&[
            " external: io_uring_enter(2),",
            " io_uring_prep_accept(3); owed by #7751",
            " - panics: none.",
        ]));
        assert!(matches!(wrapped.external, Maybe::Present(ref text)
            if text == "external: io_uring_enter(2), io_uring_prep_accept(3); owed by #7751"));
        assert_eq!(wrapped.rest, lines(&[" - panics: none."]));
        let clause = split(&lines(&[
            " - panics: none.",
            " external: io_uring_enter(2); owed by #7751",
        ]));
        assert!(matches!(clause.external, Maybe::Absent(_)));
        assert_eq!(clause.rest.len(), 2);
    }

    #[test]
    fn a_paragraph_owes_its_crate_s_issue_and_patterns()
    {
        let ring = ring();
        let read = |text: &str| defects(RustdocLine(text), &ring);
        assert!(
            read("external: io_uring_enter(2), io_uring_prep_accept(3); owed by #7751").is_empty()
        );
        assert_eq!(read("external: io_uring_enter(2)"), vec![
            ExternalDefect::Malformed
        ]);
        assert_eq!(read("external: io_uring_enter(2); owed by #7746"), vec![
            ExternalDefect::OtherIssue {
                owed: IssueNumber(7751)
            }
        ]);
        assert_eq!(read("external: liburing/src/queue.c; owed by #7751"), vec![
            ExternalDefect::Unlisted {
                authority: "liburing/src/queue.c".to_owned()
            }
        ]);
        let none = Permit {
            admitted: Maybe::Absent(admission::Unlisted::NoTable),
            defects: Vec::new(),
        };
        assert_eq!(
            defects(
                RustdocLine("external: io_uring_enter(2); owed by #7751"),
                &none
            ),
            vec![ExternalDefect::NotAdmitted]
        );
    }
}
