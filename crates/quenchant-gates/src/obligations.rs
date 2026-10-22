//! Re-verification obligations: from the items a change touches to the
//! specifications that must be checked again.
//!
//! **Changed items.** An item changed when a line of its extent, with its
//! attributes and documentation, differs between the base revision and the
//! working tree: `git diff --no-renames -U0 <base>` supplies the changed lines
//! of the working tree, and a removal counts when it falls strictly inside an
//! extent. The extents are the working tree's, from the policy library's
//! records. A renamed item has no identity across the rename today: it reads as
//! a new item under its new path, and every specification that named it under
//! the old path had to change to compile, so it reads as changed too.
//!
//! **Obligations.** A specification owes re-verification for each clause
//! that names a changed item, executable or prose, and its witnesses rerun
//! when it changed itself. Only named dependencies count, never the call
//! graph: an item reached only through a body is not an obligation.
//!
//! # Boundaries
//!
//! **Early cutoff.** Obligations are one hop from the changed items. A
//! specification that is obligated but whose own source is unchanged stops
//! the cascade: what names it is not flagged. If re-verifying it leads to an
//! edit, the edit makes it a changed item and the next run flags what names
//! it, as an incremental build reruns a dependent only when its input's
//! output changed.
//!
//! **A body fits a pull request.** GitHub refuses a body over 65 536
//! characters. A section whose table would carry the body past that leaves
//! the table to the comment and keeps one line in its place; the boxes stay,
//! so every tick reads back alike. A body still too long without its table
//! is refused with its character count ([`render_body`]): its change raises
//! more distinct prose obligations than one body can list.
//!
//! # Witnesses
//!
//! **Discharge.** An executable obligation is discharged by rerunning the
//! specification's witnesses with specifications enforced; the report lists
//! them and the nextest selection that runs them. A prose obligation is
//! discharged in the pull request: the body's re-verification section carries
//! a "still holds" and an "updated" box per obligation, one pair for all the
//! clauses naming exactly the same changed items, and a ticked box is read
//! back from the body, or from the previous comment, on the next run. An
//! acknowledgement holds for the revisions it was given against: the
//! obligation's key carries the specification's revision and each changed
//! dependency's, so a later change to either reopens it. A clause whose own
//! lines the change edits is recorded as updated. A judge with a rubric
//! replaces the boxes later; the key is what its verdict will be recorded
//! against.
//!
//! **The lint assessment.** A pull request working an issue labelled
//! `kind:performance`, or labelled `fix:performance` itself, owes one more
//! prose obligation: whether a lint could have refused the pattern. Its task
//! line reads `could: yes — <lint>, <reference>` or `could: no — <one
//! sentence>`; it stays open while unticked or in neither shape, and is read
//! from the body, so a rewrite keeps it.
//!
//! **One shape in two places.** The body section and the comment open with
//! the same verdict heading: `Re-verification · ✅ 9 of 9 settled`,
//! `⚠️ 2 of 9 unticked` while a box is open, or `✅ nothing owed`. The base and
//! the counts follow on the next line, then a table with one row per prose
//! obligation: item, clause, names, verdict. The detail is collapsed under
//! `<details>`. In the body the detail is the task boxes, whose lines and
//! `ack` markers [`acknowledgements`] reads, so a body any earlier version
//! wrote still reads back. In the comment the detail is the witnesses rerun,
//! and the comment carries no task box. The shape follows the benchmark
//! report's comment: the verdict on one line, the detail one click away.

use alloc::collections::BTreeMap;
use alloc::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use anodized::spec;
use quenchant_shape::shape::Maybe;

use crate::GateError;
use crate::evidence::Graph;
use crate::evidence::LineSpan;
use crate::evidence::Naming;
use crate::evidence::read_prose;
use crate::evidence::read_records;
use crate::hotpath::Deltas;
use crate::hotpath::Unreported;
use crate::hotpath::Verdict;
use crate::hotpath::hotpath_verdict;
use crate::hotpath::verdict_lines;
use crate::inventory::Workspace;
use crate::semantic::CommandLine;
use crate::semantic::Contains;
use crate::semantic::Count;
use crate::semantic::CrateSegment;
use crate::semantic::DocText;
use crate::semantic::ErrorMessage;
use crate::semantic::GitRevision;
use crate::semantic::InShape;
use crate::semantic::IntegrationTarget;
use crate::semantic::IssueLabel;
use crate::semantic::LineNumber;
use crate::semantic::SourceText;
use crate::semantic::Unsettled;

/// The marker that opens the pull-request comment, so CI finds its own.
pub const COMMENT_MARKER: &str = "<!-- quenchant-obligations -->";

/// The marker that opens the re-verification section of a pull-request body.
pub const SECTION_START: &str = "<!-- quenchant-obligations:start -->";

/// The marker that closes the re-verification section of a pull-request body.
pub const SECTION_END: &str = "<!-- quenchant-obligations:end -->";

/// How many hexadecimal digits of each revision an obligation key keeps.
const REVISION_DIGITS: usize = 12;

/// How many witnesses the pull-request comment lists before counting the rest.
pub const LISTED_WITNESSES: usize = 60;

/// What one invocation reads.
#[derive(Clone, Copy, Debug)]
pub struct Inputs<'input>
{
    /// The consumer workspace's root manifest.
    pub manifest_path: &'input Path,
    /// The revision the working tree is compared with.
    pub base: GitRevision<'input>,
    /// The directory the policy library wrote its records into.
    pub records: &'input Path,
    /// The directory holding rustdoc's JSON output, one file per crate.
    pub rustdoc: &'input Path,
    /// The previous comment's text, empty when there is none.
    pub previous: DocText<'input>,
    /// The pull request body the lint assessment and the hotpath verdict are
    /// read from: the body being written, or the previous text when there is
    /// none.
    pub body: DocText<'input>,
    /// The labels of the issues the pull request closes or advances, and its
    /// own.
    pub labels: &'input [IssueLabel<'input>],
    /// The hotpath check's per-benchmark deltas, when a report is given.
    pub hotpath: &'input Maybe<Deltas, Unreported>,
}

/// Compute the obligations a working tree owes against a base revision.
///
/// # Specification
/// - requires: the records and rustdoc output describe the working tree of the
///   workspace `inputs.manifest_path` names, rustdoc run with
///   `--document-private-items`.
/// - ensures: the report's changed items are [`changed_items`] of the diff
///   against `inputs.base`; its obligations are [`obligations`] over the graph
///   of records and prose links with the acknowledgements the previous comment
///   carries; its reruns are [`reruns`] of those; its assessment is
///   [`lint_assessment`] of `inputs.labels` over `inputs.body`, and its hotpath
///   verdict [`hotpath_verdict`] of `inputs.hotpath` over it.
/// - fails: [`GateError::Tool`] when the workspace or the diff cannot be read,
///   [`GateError::Io`] or [`GateError::Parse`] when the records, the rustdoc
///   output or a source cannot.
/// - panics: none.
///
/// # Errors
/// Any [`GateError`] of the steps it runs.
///
/// # Adequacy
/// - hypothesis: L3 — the fixture repository's edits produce exactly the
///   obligations of the clauses naming the edited item.
/// - witness: `gates::obligations::changing_a_named_item_obligates_the_specification`
/// - witness: `gates::obligations::changing_an_item_reached_only_through_a_call_obligates_nothing`
/// - witness: `gates::obligations::the_tag_table_replay_lists_both_preconditions`
#[inline]
#[spec(ensures: |ref output| output.as_ref().is_ok_and(|report| report.base == inputs.base.0) || output.is_err())]
pub fn run(inputs: &Inputs<'_>) -> Result<Report, GateError>
{
    let workspace = crate::inventory::discover(inputs.manifest_path)?;
    let root = workspace.root.clone();
    let mut graph = read_records(inputs.records, &root)?;
    read_prose(&mut graph, inputs.rustdoc, &root)?;
    let changes = changed_lines(&root, inputs.base)?;
    let changed = changed_items(&graph, &changes);
    let acknowledged = acknowledgements(inputs.previous);
    let mut report = Report {
        base: inputs.base.0.to_owned(),
        obligations: obligations(&graph, &changed, &changes, &acknowledged),
        changed,
        reruns: BTreeSet::new(),
        assessment: lint_assessment(inputs.labels, inputs.body),
        hotpath: hotpath_verdict(inputs.hotpath, inputs.body),
    };
    report.reruns = reruns(&graph, &workspace, &root, &report)?;
    Ok(report)
}

/// One changed region of a file in the working tree.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Change
{
    /// These lines were added or rewritten.
    Lines(LineSpan),
    /// Lines were removed after this line; zero removes before the first.
    Removal(LineNumber),
}

/// The changed regions of a working tree, by workspace-relative file.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
#[repr(transparent)]
pub struct ChangedLines(pub BTreeMap<String, Vec<Change>>);

/// How an obligation stands.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Discharge
{
    /// The witnesses rerun with specifications enforced discharge it.
    Witnesses,
    /// A prose clause nobody has acknowledged at these revisions.
    Open,
    /// Acknowledged: the clause still holds.
    StillHolds,
    /// Acknowledged, or edited by the change itself: the clause was updated.
    Updated,
}

/// One clause of one specification that names changed items.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Obligation
{
    /// The specified item's path.
    pub specification: String,
    /// The clause key.
    pub clause: String,
    /// Whether the clause is executable or prose.
    pub naming: Naming,
    /// The named items that changed.
    pub changed: BTreeSet<String>,
    /// The identity an acknowledgement is recorded against: specification,
    /// clause, and the revisions of the specification and each changed
    /// dependency.
    pub key: String,
    /// How the obligation stands.
    pub discharge: Discharge,
    /// Which boxes acknowledge it.
    pub boxes: Boxes,
}

/// Which task boxes acknowledge a prose obligation.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Boxes
{
    /// Its own pair, recorded against its key.
    Own,
    /// One pair for every unedited prose obligation naming exactly the same
    /// changed items, recorded against this key.
    Shared(String),
}

/// One witness to rerun, with the package whose tests hold it.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Rerun
{
    /// The package owning the specification and its witness.
    pub package: String,
    /// The witness path as the adequacy section writes it.
    pub witness: String,
    /// The specification the witness checks.
    pub specification: String,
    /// Whether the witness's first segment names an integration test target
    /// of the package, which then holds the rest of the path.
    pub integration: IntegrationTarget,
}

/// Everything one run of the subcommand found.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Report
{
    /// The base revision as given.
    pub base: String,
    /// The items the change touches.
    pub changed: BTreeSet<String>,
    /// The obligations, executable and prose.
    pub obligations: Vec<Obligation>,
    /// The witnesses to rerun.
    pub reruns: BTreeSet<Rerun>,
    /// Whether the change owes a lint assessment, and how it stands.
    pub assessment: Assessment,
    /// Whether the change owes a hotpath verdict, and how it stands.
    pub hotpath: Verdict,
}

/// The labels whose pull requests owe a lint assessment: a pull request
/// closing or advancing an issue so labelled, or labelled so itself.
pub const ASSESSED_LABELS: [&str; 2] = ["kind:performance", "fix:performance"];

/// The text that opens the lint assessment's task line, before its verdict.
pub const ASSESSMENT_LABEL: &str = "lint assessment — ";

/// The marker that closes the lint assessment's task line.
pub const ASSESSMENT_MARKER: &str = "<!-- ack lint-assessment -->";

/// The verdict a fresh assessment line holds, for the author to replace.
pub const ASSESSMENT_TEMPLATE: &str = "`could: yes — <lint>, <reference>` or `could: no — <one \
                                       sentence>`";

/// Whether a pull request owes the lint assessment, and how it stands.
///
/// A performance fix states whether a lint could have refused the pattern it
/// fixes: `could: yes — <lint>, <reference>`, naming the lint and the quenchant
/// issue or pull request that adds it, or `could: no — <one sentence>` saying
/// why no syntactic shape exists.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum Assessment
{
    /// No label asks for it.
    #[default]
    NotOwed,
    /// A label asks for it.
    Owed
    {
        /// The verdict as written on the task line.
        written: String,
        /// How it stands.
        standing: Standing,
    },
}

/// How an owed lint assessment stands.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Standing
{
    /// The box is open.
    Unticked,
    /// The box is ticked and the verdict is in neither shape.
    Malformed,
    /// The box is ticked and the verdict is in one of the shapes.
    Settled,
}

/// The changed lines of the working tree against `base`.
///
/// # Specification
/// - requires: `root` is inside a Git working tree in which `base` names a
///   commit.
/// - ensures: returns [`parse_diff`] of the zero-context diff `git diff
///   --relative --no-renames -U0 <base>` run in `root`, with external diff
///   drivers and text conversion off, so paths are relative to `root`, as the
///   records' are.
/// - fails: [`GateError::Tool`] when Git cannot be run or refuses.
/// - panics: none.
///
/// # Errors
/// [`GateError::Tool`] when the diff cannot be produced.
///
/// # Adequacy
/// - hypothesis: L3 — the fixture repository's edits to one item each are found
///   as changes of exactly that item.
/// - witness: `gates::obligations::changing_a_named_item_obligates_the_specification`
#[inline]
#[spec(ensures: |ref output| output.is_ok() || matches!(output, &Err(GateError::Tool { .. })))]
pub fn changed_lines(
    root: &Path,
    base: GitRevision<'_>,
) -> Result<ChangedLines, GateError>
{
    let output = Command::new("git")
        .current_dir(root)
        .args([
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--no-color",
            "--no-renames",
            "--relative",
            "--unified=0",
            base.0,
            "--",
        ])
        .output()
        .map_err(|error| {
            GateError::tool("git diff".into(), ErrorMessage::from(&error.to_string()))
        })?;
    if !output.status.success() {
        return Err(GateError::tool(
            CommandLine::from(&format!("git diff {}", base.0)),
            ErrorMessage::from(String::from_utf8_lossy(&output.stderr).trim()),
        ));
    }
    Ok(parse_diff(SourceText::from(
        String::from_utf8_lossy(&output.stdout).as_ref(),
    )))
}

/// Read a zero-context unified diff into changed regions.
///
/// # Specification
/// - requires: `diff` is the output of `git diff --unified=0`.
/// - ensures: every hunk of a file whose new side exists becomes a change of
///   that file, keyed by its path without the `b/` prefix: a hunk adding lines
///   is the span of its new lines, a hunk adding none is a removal after its
///   new-side start line.
/// - provides: deleted files contribute nothing; the working tree has no item
///   in them.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the fixture's one-line edits and the replay's inserted
///   table row are read as the regions they are.
/// - witness: `gates::obligations::changing_a_named_item_obligates_the_specification`
/// - witness: `gates::obligations::the_tag_table_replay_lists_both_preconditions`
#[inline]
#[must_use]
#[spec(ensures: |ref output| output.0.values().all(|changes| !changes.is_empty()))]
pub fn parse_diff(diff: SourceText<'_>) -> ChangedLines
{
    let mut files: BTreeMap<String, Vec<Change>> = BTreeMap::new();
    let mut current: Option<String> = None;
    for line in diff.0.lines() {
        if let Some(path) = line.strip_prefix("+++ ") {
            current = path.strip_prefix("b/").map(str::to_owned);
            continue;
        }
        let (Some(file), Some(hunk)) = (current.as_ref(), line.strip_prefix("@@ "))
        else {
            continue;
        };
        let Some(added) = hunk
            .split_whitespace()
            .find_map(|field| field.strip_prefix('+'))
        else {
            continue;
        };
        let (start, count) = added.split_once(',').unwrap_or((added, "1"));
        let (Ok(start), Ok(count)) = (start.parse::<usize>(), count.parse::<usize>())
        else {
            continue;
        };
        let change =
            count
                .checked_sub(1_usize)
                .map_or(Change::Removal(LineNumber(start)), |extra| {
                    Change::Lines(LineSpan {
                        first: LineNumber(start),
                        last: LineNumber(start.saturating_add(extra)),
                    })
                });
        files.entry(file.clone()).or_default().push(change);
    }
    ChangedLines(files)
}

/// Whether a change touches an extent.
///
/// # Specification
/// - ensures: changed lines touch the extent when they overlap it; a removal
///   touches it when it falls strictly inside, after one of its lines and
///   before another.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — an edit inside one fixture item marks that item and not
///   its neighbours.
/// - witness: `gates::obligations::changing_a_named_item_obligates_the_specification`
#[spec(ensures: |output| output.0 == match change {
    Change::Lines(lines) => lines.first <= extent.last && extent.first <= lines.last,
    Change::Removal(after) => extent.first <= after && after < extent.last,
})]
fn touches(
    change: Change,
    extent: LineSpan,
) -> Contains
{
    Contains(match change {
        Change::Lines(lines) => lines.first <= extent.last && extent.first <= lines.last,
        Change::Removal(after) => extent.first <= after && after < extent.last,
    })
}

/// The items a change touches.
///
/// # Specification
/// - ensures: returns the path of every item whose extent a change of its file
///   touches.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — editing the unrelated fixture item touches only it.
/// - witness: `gates::obligations::changing_an_item_reached_only_through_a_call_obligates_nothing`
#[inline]
#[must_use]
#[spec(ensures: |ref output| output.iter().all(|path| graph.items.contains_key(path)))]
pub fn changed_items(
    graph: &Graph,
    changes: &ChangedLines,
) -> BTreeSet<String>
{
    graph
        .items
        .values()
        .filter(|item| {
            changes
                .0
                .get(&item.position.file)
                .is_some_and(|file| file.iter().any(|&change| touches(change, item.extent).0))
        })
        .map(|item| item.path.clone())
        .collect()
}

/// The obligations a set of changed items creates.
///
/// # Specification
/// - ensures: returns one obligation per specification, clause and naming whose
///   edges name a changed item, listing those items, in sorted order; nothing
///   else is returned, so an obligated but unchanged specification adds no
///   obligation for what names it.
/// - ensures: an executable obligation is discharged by its witnesses; a prose
///   obligation is `Updated` when a change touches its clause's lines, else as
///   `acknowledged` records its key, else as `acknowledged` records the key of
///   the boxes it shares, else `Open`.
/// - ensures: the unedited prose obligations that name exactly the same changed
///   items share one pair of boxes when there are two or more of them,
///   [`Boxes::Shared`] under one key: the names, their revisions, and each
///   member's specification revision and clause, so a change to any of them or
///   a member gained or lost reopens it. Every other obligation's boxes are
///   [`Boxes::Own`].
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — in a chain where the third specification names the second
///   and the second names the first, changing the first obligates the second
///   only; changing the first and the second's specification obligates both. A
///   hundred clauses naming one item share one key, which one tick settles and
///   a change to one member's specification reopens.
/// - witness: `gates::obligations::an_unchanged_reverified_specification_stops_the_cascade`
/// - witness: `gates::obligations::an_acknowledgement_holds_only_at_its_revisions`
/// - witness: `gates::obligations::clauses_naming_the_same_items_share_one_pair`
#[inline]
#[must_use]
#[spec(ensures: |ref output| output.is_sorted() && output.iter().all(|obligation| !obligation.changed.is_empty() && obligation.changed.is_subset(changed)))]
pub fn obligations(
    graph: &Graph,
    changed: &BTreeSet<String>,
    changes: &ChangedLines,
    acknowledged: &BTreeMap<String, Discharge>,
) -> Vec<Obligation>
{
    let mut grouped: BTreeMap<(String, String, Naming), BTreeSet<String>> = BTreeMap::new();
    for edge in graph
        .edges
        .iter()
        .filter(|edge| changed.contains(&edge.target))
    {
        grouped
            .entry((edge.specification.clone(), edge.clause.clone(), edge.naming))
            .or_default()
            .insert(edge.target.clone());
    }
    let revision = |path: &str| {
        graph.items.get(path).map_or_else(String::new, |item| {
            item.revision
                .get(.. REVISION_DIGITS)
                .unwrap_or(&item.revision)
                .to_owned()
        })
    };
    let mut owed: Vec<Obligation> = Vec::new();
    // The unedited prose obligations, by index into `owed`.
    let mut unedited: Vec<usize> = Vec::new();
    for ((specification, clause, naming), dependencies) in grouped {
        let mut key = format!("{specification}#{clause}@{}", revision(&specification));
        for dependency in &dependencies {
            key.push('+');
            key.push_str(&revision(dependency));
        }
        let discharge = match naming {
            Naming::Executable => Discharge::Witnesses,
            Naming::Prose => {
                let edited = graph
                    .prose_clauses
                    .get(&(specification.clone(), clause.clone()))
                    .zip(graph.items.get(&specification))
                    .is_some_and(|(&lines, item)| {
                        changes
                            .0
                            .get(&item.position.file)
                            .is_some_and(|file| file.iter().any(|&change| touches(change, lines).0))
                    });
                if edited {
                    Discharge::Updated
                }
                else {
                    unedited.push(owed.len());
                    acknowledged.get(&key).copied().unwrap_or(Discharge::Open)
                }
            },
        };
        owed.push(Obligation {
            specification,
            clause,
            naming,
            changed: dependencies,
            key,
            discharge,
            boxes: Boxes::Own,
        });
    }
    // Ordered by what each names, stably, so the clauses naming the same
    // changed items run together in report order.
    let names = |index: &usize| owed.get(*index).map(|obligation| &obligation.changed);
    unedited.sort_by(|left, right| names(left).cmp(&names(right)));
    // Each sharing group's key and the acknowledgement recorded against it,
    // and each member's group.
    let mut groups: Vec<(String, Option<Discharge>)> = Vec::new();
    let mut assigned: Vec<(usize, usize)> = Vec::new();
    for members in unedited.chunk_by(|left, right| names(left) == names(right)) {
        let (Some(first), true) = (members.first().and_then(names), members.len() > 1_usize)
        else {
            continue;
        };
        let mut key = String::new();
        for (index, name) in first.iter().enumerate() {
            key.push_str(if index == 0 { "" } else { "," });
            key.push_str(name);
        }
        for (index, name) in first.iter().enumerate() {
            key.push_str(if index == 0 { "@" } else { "+" });
            key.push_str(&revision(name));
        }
        for (index, obligation) in members
            .iter()
            .filter_map(|&member| owed.get(member))
            .enumerate()
        {
            key.push_str(if index == 0 { "#" } else { "+" });
            key.push_str(&revision(&obligation.specification));
            key.push('.');
            key.push_str(&obligation.clause);
        }
        for &member in members {
            assigned.push((member, groups.len()));
        }
        let shared = acknowledged.get(&key).copied();
        groups.push((key, shared));
    }
    for (member, group) in assigned {
        if let (Some(obligation), Some(&(ref key, shared))) =
            (owed.get_mut(member), groups.get(group))
        {
            if let (Discharge::Open, Some(discharge)) = (obligation.discharge, shared) {
                obligation.discharge = discharge;
            }
            obligation.boxes = Boxes::Shared(key.clone());
        }
    }
    owed
}

/// The acknowledgements a previous comment carries.
///
/// # Specification
/// - ensures: returns, for every ticked task line (`- [x]` or `- [X]`) that
///   carries an `<!-- ack still-holds KEY -->` or `<!-- ack updated KEY -->`
///   marker, the key and the acknowledgement; a later line for the same key
///   wins.
/// - provides: unticked boxes and other text acknowledge nothing.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — a ticked box carries forward while the revisions match
///   and is ignored once the dependency changes again.
/// - witness: `gates::obligations::an_acknowledgement_holds_only_at_its_revisions`
#[inline]
#[must_use]
#[spec(ensures: |ref output| output.values().all(|discharge| matches!(*discharge, Discharge::StillHolds | Discharge::Updated)))]
pub fn acknowledgements(previous: DocText<'_>) -> BTreeMap<String, Discharge>
{
    let mut found = BTreeMap::new();
    for line in previous.0.lines() {
        let line = line.trim();
        if !(line.starts_with("- [x]") || line.starts_with("- [X]")) {
            continue;
        }
        let Some((_, marker)) = line.split_once("<!-- ack ")
        else {
            continue;
        };
        let Some(marker) = marker.strip_suffix("-->")
        else {
            continue;
        };
        let Some((kind, key)) = marker.trim().split_once(' ')
        else {
            continue;
        };
        let discharge = match kind {
            "still-holds" => Discharge::StillHolds,
            "updated" => Discharge::Updated,
            _ => continue,
        };
        found.insert(key.trim().to_owned(), discharge);
    }
    found
}

/// The witnesses whose specifications the report obligates or changes.
///
/// # Specification
/// - requires: `workspace` describes the workspace whose root is `root`.
/// - ensures: returns every witness of every obligated specification and of
///   every changed item, each with the package whose directory holds the
///   specification's file; an item outside every member, or without
///   documentation, contributes none.
/// - fails: [`GateError::Io`] when a specification's file cannot be read,
///   [`GateError::Parse`] when it is not Rust.
/// - panics: none.
///
/// # Errors
/// [`GateError::Io`] or [`GateError::Parse`] for an unreadable source.
///
/// # Adequacy
/// - hypothesis: L3 — the fixture's obligated specification contributes its own
///   witness, and the unrelated change contributes none.
/// - witness: `gates::obligations::changing_a_named_item_obligates_the_specification`
/// - witness: `gates::obligations::changing_an_item_reached_only_through_a_call_obligates_nothing`
#[inline]
#[spec(ensures: |ref output| output.is_ok() || matches!(output, &Err(GateError::Io { .. } | GateError::Parse { .. })))]
pub fn reruns(
    graph: &Graph,
    workspace: &Workspace,
    root: &Path,
    report: &Report,
) -> Result<BTreeSet<Rerun>, GateError>
{
    let specifications: BTreeSet<&String> = report
        .obligations
        .iter()
        .map(|obligation| &obligation.specification)
        .chain(report.changed.iter())
        .collect();
    let mut found = BTreeSet::new();
    let mut sources: BTreeMap<&str, String> = BTreeMap::new();
    for specification in specifications {
        let Some(item) = graph.items.get(specification)
        else {
            continue;
        };
        let quenchant_shape::shape::Maybe::Present(docs) = item.docs
        else {
            continue;
        };
        let file = root.join(&item.position.file);
        let Some(member) = workspace
            .members
            .iter()
            .filter(|member| file.starts_with(&member.directory))
            .max_by_key(|member| member.directory.components().count())
        else {
            continue;
        };
        if !sources.contains_key(item.position.file.as_str()) {
            let text = std::fs::read_to_string(&file)
                .map_err(|error| GateError::io(&file, ErrorMessage::from(&error.to_string())))?;
            sources.insert(&item.position.file, text);
        }
        let source = sources
            .get(item.position.file.as_str())
            .map_or("", String::as_str);
        for witness in crate::evidence::witnesses_in(&file, SourceText::from(source), docs)? {
            let integration = IntegrationTarget(
                witness
                    .split_once("::")
                    .is_some_and(|(target, _)| member.integration_targets.contains(target)),
            );
            found.insert(Rerun {
                package: member.name.clone(),
                witness,
                specification: specification.clone(),
                integration,
            });
        }
    }
    Ok(found)
}

/// The `cargo nextest run` arguments that run exactly the report's witnesses.
///
/// # Specification
/// - ensures: empty when there is no witness to rerun; otherwise `-p` with each
///   package once, then `-E` with a filterset matching, per witness, the binary
///   its first segment names and the test the rest names when that segment is
///   an integration target, and a test of its package named as written
///   otherwise.
///
/// # Adequacy
/// - hypothesis: L3 — the fixture's obligated witness yields one package and
///   one filter alternative, naming the binary only once the fixture gains an
///   integration target of the witness's first segment.
/// - witness: `gates::obligations::changing_a_named_item_obligates_the_specification`
/// - witness: `gates::obligations::an_integration_witness_selects_its_target_binary`
#[inline]
#[must_use]
#[spec(ensures: |ref output| output.is_empty() == report.reruns.is_empty())]
pub fn selection(report: &Report) -> Vec<String>
{
    if report.reruns.is_empty() {
        return Vec::new();
    }
    let packages: BTreeSet<&String> = report.reruns.iter().map(|rerun| &rerun.package).collect();
    let mut arguments = Vec::new();
    for package in packages {
        arguments.push("-p".to_owned());
        arguments.push(package.clone());
    }
    let alternatives: BTreeSet<String> = report
        .reruns
        .iter()
        .map(|rerun| match rerun.witness.split_once("::") {
            Some((target, rest)) if rerun.integration.0 => format!(
                "(package(={}) & binary(={target}) & test(={rest}))",
                rerun.package
            ),
            _ => format!("(package(={}) & test(={}))", rerun.package, rerun.witness),
        })
        .collect();
    arguments.push("-E".to_owned());
    arguments.push(alternatives.into_iter().collect::<Vec<_>>().join(" | "));
    arguments
}

/// The report as console lines.
///
/// # Specification
/// - ensures: a summary line with the base, the changed-item count and the
///   obligation count, then one line per changed item, per obligation with its
///   standing, and per witness to rerun.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the replay's printed report names both preconditions as
///   open prose obligations on the table.
/// - witness: `gates::obligations::the_tag_table_replay_lists_both_preconditions`
#[inline]
#[must_use]
#[spec(ensures: |ref output| output.lines().count() == report.changed.len().saturating_add(report.obligations.len()).saturating_add(report.reruns.len()).saturating_add(1_usize))]
pub fn render_text(report: &Report) -> String
{
    let mut lines = vec![format!(
        "obligations against {}: {} changed items, {} obligations, {} witnesses to rerun",
        report.base,
        report.changed.len(),
        report.obligations.len(),
        report.reruns.len()
    )];
    lines.extend(report.changed.iter().map(|item| format!("changed: {item}")));
    lines.extend(report.obligations.iter().map(|obligation| {
        let naming = match obligation.naming {
            Naming::Executable => "executable",
            Naming::Prose => "prose",
        };
        let discharge = match obligation.discharge {
            Discharge::Witnesses => "witnesses rerun",
            Discharge::Open => "open",
            Discharge::StillHolds => "still holds",
            Discharge::Updated => "updated",
        };
        let mut line = format!(
            "{naming}: {} `{}` names ",
            obligation.specification, obligation.clause,
        );
        for (index, name) in obligation.changed.iter().enumerate() {
            if index != 0 {
                line.push_str(", ");
            }
            line.push_str(name);
        }
        line.push_str(" [");
        line.push_str(discharge);
        line.push(']');
        line
    }));
    lines.extend(report.reruns.iter().map(|rerun| {
        format!(
            "witness: {} {} (for {})",
            rerun.package, rerun.witness, rerun.specification
        )
    }));
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

/// The report's prose obligations as the pull-request body's re-verification
/// section.
///
/// # Specification
/// - ensures: opens with [`SECTION_START`] and closes with [`SECTION_END`].
///   Between them come [`verdict`]'s heading at level two, the base and the
///   counts, and [`table`]'s rows. Then, collapsed under one `<details>` block,
///   one pair of task boxes, "still holds" and "updated", per [`Boxes::Own`]
///   obligation, ticked as it stands, and one pair per [`Boxes::Shared`] key
///   where its first member stands, counting its members and ticked as all of
///   them stand alike. Each box carries its `ack` marker and key, so
///   [`acknowledgements`] reads it back. A report without prose obligations
///   says so, and has neither table nor boxes. An owed lint assessment follows,
///   uncollapsed: its instruction and its task line with the verdict as
///   written, ticked unless it stands unticked, so [`lint_assessment`] reads it
///   back; an owed hotpath verdict follows it the same way, read back by
///   [`hotpath_verdict`].
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — a section rendered from a report and ticked reads back as
///   the acknowledgement it records, its heading and rows state the verdict the
///   boxes record, and one ticked shared pair settles every clause sharing it.
/// - witness: `gates::obligations::an_acknowledgement_holds_only_at_its_revisions`
/// - witness: `gates::obligations::the_heading_and_the_table_carry_the_verdict`
/// - witness: `gates::obligations::clauses_naming_the_same_items_share_one_pair`
#[inline]
#[must_use]
#[spec(ensures: |ref output| output.starts_with(SECTION_START) && output.trim_end().ends_with(SECTION_END))]
pub fn render_section(report: &Report) -> String
{
    section(report, Rows::Table)
}

/// The most characters GitHub accepts in a pull-request body, 65 536.
pub const BODY_LIMIT: usize = 0x1_0000;

/// The line that stands for the table in a section too long to carry it.
pub const TABLE_IN_COMMENT: &str = "The table of clauses is in the obligations comment: with it, \
                                    this body would pass the 65 536 characters a pull request \
                                    accepts.";

/// A body whose re-verification section, the table of clauses left to the
/// comment, still passes [`BODY_LIMIT`].
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Oversized
{
    /// The characters the body would hold.
    pub characters: usize,
}

/// `body` with its re-verification section replaced, sized to fit a pull
/// request.
///
/// # Specification
/// - ensures: `Ok` holds [`splice_section`] of `body` and [`render_section`]'s
///   section when that body is at most [`BODY_LIMIT`] characters; otherwise of
///   the same section with [`TABLE_IN_COMMENT`] for its table, when that one
///   is. The boxes, their keys and the verdict are the same in both, so every
///   tick reads back alike.
/// - fails: [`Oversized`] with the shorter body's character count when it
///   passes [`BODY_LIMIT`] too.
/// - panics: none.
///
/// # Errors
/// [`Oversized`] when the section passes the limit without its table.
///
/// # Adequacy
/// - hypothesis: L3 — a short report keeps its table; a report whose table
///   alone passes the limit leaves it to the comment, fits, and a tick of one
///   of its boxes reads back; a report whose boxes alone pass it is refused.
/// - witness: `gates::obligations::a_section_too_long_for_a_body_leaves_its_table_to_the_comment`
#[inline]
#[spec(ensures: |ref output| output.as_ref().is_ok_and(|text| text.chars().nth(BODY_LIMIT).is_none()) || output.as_ref().is_err_and(|oversized| oversized.characters > BODY_LIMIT))]
pub fn render_body(
    body: DocText<'_>,
    report: &Report,
) -> Result<String, Oversized>
{
    let full = splice_section(body, DocText(&render_section(report)));
    if full.chars().nth(BODY_LIMIT).is_none() {
        return Ok(full);
    }
    let compact = splice_section(body, DocText(&section(report, Rows::InComment)));
    if compact.chars().nth(BODY_LIMIT).is_none() {
        Ok(compact)
    }
    else {
        Err(Oversized {
            characters: compact.chars().count(),
        })
    }
}

/// Where a section's clauses are tabled.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Rows
{
    /// In the section, one row per clause.
    Table,
    /// In the comment only, with [`TABLE_IN_COMMENT`] in the section.
    InComment,
}

/// The section [`render_section`] states, its clauses tabled as `rows`
/// says.
///
/// # Specification
/// trivial.
fn section(
    report: &Report,
    rows: Rows,
) -> String
{
    let prose = prose(report);
    let mut lines = vec![
        SECTION_START.to_owned(),
        format!("## {}", verdict(&prose, report)),
        String::new(),
        facts(report, &prose),
        String::new(),
    ];
    if prose.is_empty() {
        lines.push("No prose clause names a changed item.".to_owned());
    }
    else {
        match rows {
            Rows::Table => lines.extend(table(&prose)),
            Rows::InComment => lines.push(TABLE_IN_COMMENT.to_owned()),
        }
        lines.extend([
            String::new(),
            "<details><summary>Acknowledgements</summary>".to_owned(),
            String::new(),
            "Re-read each clause against the changed items it names, then tick one box. The \
             clauses naming exactly the same changed items share one pair. A tick holds until \
             the clause's item or a named item changes again."
                .to_owned(),
            String::new(),
        ]);
        // Each shared key's member count, and the standing every member
        // shares, `Open` when they differ.
        let mut shared: BTreeMap<&str, (usize, Discharge)> = BTreeMap::new();
        for obligation in &prose {
            if let Boxes::Shared(ref key) = obligation.boxes {
                let entry = shared
                    .entry(key.as_str())
                    .or_insert((0_usize, obligation.discharge));
                entry.0 = entry.0.saturating_add(1_usize);
                if entry.1 != obligation.discharge {
                    entry.1 = Discharge::Open;
                }
            }
        }
        for obligation in &prose {
            let (key, standing) = match obligation.boxes {
                Boxes::Own => {
                    lines.push(format!(
                        "- `{}` `{}` names {}",
                        obligation.specification,
                        obligation.clause,
                        code_list(&obligation.changed)
                    ));
                    (obligation.key.as_str(), obligation.discharge)
                },
                Boxes::Shared(ref key) => {
                    let Some((members, standing)) = shared.remove(key.as_str())
                    else {
                        continue;
                    };
                    lines.push(format!(
                        "- the {members} clauses naming {}",
                        code_list(&obligation.changed)
                    ));
                    (key.as_str(), standing)
                },
            };
            for (label, kind, ticked) in [
                (
                    "still holds",
                    "still-holds",
                    standing == Discharge::StillHolds,
                ),
                ("updated", "updated", standing == Discharge::Updated),
            ] {
                lines.push(format!(
                    "  - [{}] {label} <!-- ack {kind} {key} -->",
                    if ticked { 'x' } else { ' ' },
                ));
            }
        }
        lines.extend([String::new(), "</details>".to_owned()]);
    }
    lines.extend(assessment_lines(&report.assessment));
    lines.extend(verdict_lines(&report.hotpath));
    lines.push(SECTION_END.to_owned());
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

/// The report's prose obligations, in report order.
///
/// # Specification
/// trivial.
fn prose(report: &Report) -> Vec<&Obligation>
{
    report
        .obligations
        .iter()
        .filter(|obligation| obligation.naming == Naming::Prose)
        .collect()
}

/// The heading text that carries the verdict on `prose`, the lint
/// assessment and the hotpath verdict.
///
/// # Specification
/// - ensures: `Re-verification · ✅ nothing owed` when `prose` is empty and
///   neither the assessment nor the hotpath verdict is owed; `Re-verification ·
///   ⚠️ n of m unticked` while `n > 0` of the `m` obligations, each owed one of
///   those two counted as one, stand unsettled; `Re-verification · ✅ m of m
///   settled` otherwise.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the three states each produce their own heading, and an
///   owed assessment counts.
/// - witness: `gates::obligations::the_heading_and_the_table_carry_the_verdict`
/// - witness: `gates::obligations::a_performance_fix_owes_a_lint_assessment`
/// - witness: `gates::obligations::a_red_hotpath_check_owes_a_verdict`
#[spec(ensures: |ref output| output.starts_with("Re-verification · "))]
fn verdict(
    prose: &[&Obligation],
    report: &Report,
) -> String
{
    let assessed = usize::from(matches!(report.assessment, Assessment::Owed { .. }))
        .saturating_add(usize::from(matches!(report.hotpath, Verdict::Owed { .. })));
    let open = prose
        .iter()
        .filter(|obligation| obligation.discharge == Discharge::Open)
        .count()
        .saturating_add(usize::from(report.assessment.unsettled().0))
        .saturating_add(usize::from(report.hotpath.unsettled().0));
    let owed = prose.len().saturating_add(assessed);
    if owed == 0 {
        "Re-verification · ✅ nothing owed".to_owned()
    }
    else if open > 0 {
        format!("Re-verification · ⚠️ {open} of {owed} unticked")
    }
    else {
        format!("Re-verification · ✅ {owed} of {owed} settled")
    }
}

/// The line under the heading: the base, the changed items, the executable
/// obligations, and the crate every prose row shares.
///
/// # Specification
/// trivial.
fn facts(
    report: &Report,
    prose: &[&Obligation],
) -> String
{
    let executable = report
        .obligations
        .iter()
        .filter(|obligation| obligation.naming == Naming::Executable)
        .count();
    let shared = shared_crate(prose).map_or_else(String::new, |segment| {
        format!(" · items in `{}`", segment.0)
    });
    format!(
        "Against `{}` · {} changed items · {executable} executable obligations, discharged by the \
         witnesses CI reruns{shared}.",
        report.base,
        report.changed.len(),
    )
}

/// One markdown table row per prose obligation: the item, the clause, what it
/// names, and its verdict.
///
/// # Specification
/// - ensures: a header row, a delimiter row, then one row per obligation in
///   order. Each path drops the crate every row shares, which [`facts`] names
///   once. The verdict reads `✅ still holds`, `🔁 updated` or `☐ unticked`.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — an open and a ticked obligation render the row shape with
///   their verdicts.
/// - witness: `gates::obligations::the_heading_and_the_table_carry_the_verdict`
#[spec(ensures: |ref output| output.len() == prose.len().saturating_add(2_usize))]
fn table(prose: &[&Obligation]) -> Vec<String>
{
    let shared = shared_crate(prose);
    // Write `path` as inline code, without the shared crate segment, a pipe
    // escaped so it cannot end the cell.
    let code = |row: &mut String, path: &str| {
        let path = shared
            .and_then(|segment| path.strip_prefix(segment.0))
            .and_then(|rest| rest.strip_prefix("::"))
            .unwrap_or(path);
        row.push('`');
        for character in path.chars() {
            if character == '|' {
                row.push('\\');
            }
            row.push(character);
        }
        row.push('`');
    };
    let mut rows = vec![
        "| item | clause | names | verdict |".to_owned(),
        "| --- | --- | --- | --- |".to_owned(),
    ];
    rows.extend(prose.iter().map(|obligation| {
        let mut row = String::from("| ");
        code(&mut row, &obligation.specification);
        row.push_str(" | ");
        code(&mut row, &obligation.clause);
        row.push_str(" | ");
        for (index, name) in obligation.changed.iter().enumerate() {
            if index != 0 {
                row.push_str(", ");
            }
            code(&mut row, name);
        }
        row.push_str(match obligation.discharge {
            Discharge::StillHolds => " | ✅ still holds |",
            Discharge::Updated => " | 🔁 updated |",
            Discharge::Open => " | ☐ unticked |",
            Discharge::Witnesses => " | ✅ witnesses rerun |",
        });
        row
    }));
    rows
}

/// The crate every obligation's item lies in, when they share one.
///
/// # Specification
/// trivial.
fn shared_crate<'report>(prose: &[&'report Obligation]) -> Option<CrateSegment<'report>>
{
    let mut crates = prose.iter().map(|obligation| {
        obligation
            .specification
            .split_once("::")
            .map(|(name, _)| name)
    });
    let first = crates.next()??;
    crates
        .all(|name| name == Some(first))
        .then_some(CrateSegment(first))
}

/// A pull-request body with its re-verification section replaced by
/// `section`.
///
/// # Specification
/// - requires: `section` is a [`render_section`] output.
/// - ensures: when `body` holds [`SECTION_START`] followed by [`SECTION_END`],
///   the text from the first start through the end after it is replaced by
///   `section`, and the text around it is kept; otherwise `section` follows the
///   body after one blank line. Either way the result holds `section`.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — splicing a fresh section into a body that holds a ticked
///   one keeps the prose around it, and a body without one gains it at its end.
/// - witness: `gates::obligations::the_section_replaces_its_own_markers_only`
#[inline]
#[must_use]
#[spec(ensures: |ref output| output.contains(section.0))]
pub fn splice_section(
    body: DocText<'_>,
    section: DocText<'_>,
) -> String
{
    let spliced = body.0.split_once(SECTION_START).and_then(|(before, rest)| {
        rest.split_once(SECTION_END).map(|(_, after)| {
            let after = after.strip_prefix('\n').unwrap_or(after);
            format!("{before}{}{after}", section.0)
        })
    });
    spliced.unwrap_or_else(|| {
        let kept = body.0.trim_end();
        if kept.is_empty() {
            section.0.to_owned()
        }
        else {
            format!("{kept}\n\n{}", section.0)
        }
    })
}

/// The report as the pull-request comment: the body section's verdict and
/// table, then the witnesses collapsed.
///
/// # Specification
/// - ensures: opens with [`COMMENT_MARKER`]. Then come [`verdict`]'s heading at
///   level three, the base and the counts, and [`table`]'s rows when prose is
///   owed. Then one `<details>` block whose summary states the witnesses rerun
///   with specifications enforced, listing them up to [`LISTED_WITNESSES`] with
///   the count of the rest. The task boxes are the body's, never the comment's.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the comment carries the body's verdict and rows, lists
///   the witnesses collapsed, and carries no task box.
/// - witness: `gates::obligations::the_comment_lists_only_the_witnesses`
/// - witness: `gates::obligations::the_heading_and_the_table_carry_the_verdict`
#[inline]
#[must_use]
#[spec(ensures: |ref output| output.starts_with(COMMENT_MARKER) && !output.contains("- [ ]"))]
pub fn render_comment(report: &Report) -> String
{
    let prose = prose(report);
    let mut lines = vec![
        COMMENT_MARKER.to_owned(),
        format!("### {}", verdict(&prose, report)),
        String::new(),
        facts(report, &prose),
        String::new(),
    ];
    if !prose.is_empty() {
        lines.extend(table(&prose));
        lines.push(String::new());
    }
    lines.extend([
        "<details>".to_owned(),
        format!(
            "<summary>Witnesses rerun with specifications enforced: {}</summary>",
            report.reruns.len()
        ),
        String::new(),
    ]);
    if report.reruns.is_empty() {
        lines.push("None.".to_owned());
    }
    lines.extend(report.reruns.iter().take(LISTED_WITNESSES).map(|rerun| {
        format!(
            "- `{}` `{}` for `{}`",
            rerun.package, rerun.witness, rerun.specification
        )
    }));
    if let Some(rest) = report.reruns.len().checked_sub(LISTED_WITNESSES)
        && rest > 0_usize
    {
        lines.push(format!("- and {rest} more"));
    }
    lines.extend([String::new(), "</details>".to_owned()]);
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

/// Paths as an inline-code list.
///
/// # Specification
/// trivial.
fn code_list(paths: &BTreeSet<String>) -> String
{
    paths
        .iter()
        .map(|path| format!("`{path}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The number of obligations nobody has settled: the open prose obligations,
/// and the lint assessment and the hotpath verdict while owed and unsettled.
///
/// # Specification
/// trivial.
#[inline]
#[must_use]
pub fn open_count(report: &Report) -> Count
{
    let prose = report
        .obligations
        .iter()
        .filter(|obligation| obligation.discharge == Discharge::Open)
        .count();
    Count(
        prose
            .saturating_add(usize::from(report.assessment.unsettled().0))
            .saturating_add(usize::from(report.hotpath.unsettled().0)),
    )
}

impl Assessment
{
    /// Whether the assessment is owed and not settled.
    ///
    /// # Specification
    /// trivial.
    #[inline]
    #[must_use]
    pub const fn unsettled(&self) -> Unsettled
    {
        Unsettled(matches!(*self, Self::Owed {
            standing: Standing::Unticked | Standing::Malformed,
            ..
        }))
    }
}

/// Whether the labels make the lint assessment owed, and how it stands in
/// `previous`.
///
/// # Specification
/// - ensures: [`Assessment::NotOwed`] when no label is one of
///   [`ASSESSED_LABELS`]. Otherwise owed, with the verdict written on the task
///   line of `previous` that carries [`ASSESSMENT_MARKER`], the last such line
///   winning, or [`ASSESSMENT_TEMPLATE`] when there is none. It stands unticked
///   while the box is open, settled when the box is ticked and the verdict is
///   in one of [`verdict_shape`]'s shapes, malformed otherwise.
/// - provides: the verdict is read from the body, never from a comment, and it
///   carries over every rewrite of the section.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — an unlabelled report owes nothing; a labelled one is
///   unticked as rendered, settled once ticked with `could: no — <sentence>`,
///   and malformed when ticked with `could: yes` and no lint.
/// - witness: `gates::obligations::a_performance_fix_owes_a_lint_assessment`
#[inline]
#[must_use]
#[spec(ensures: |ref output| matches!(*output, Assessment::NotOwed) != labels.iter().any(|label| ASSESSED_LABELS.contains(&label.0)))]
pub fn lint_assessment(
    labels: &[IssueLabel<'_>],
    previous: DocText<'_>,
) -> Assessment
{
    if !labels
        .iter()
        .any(|label| ASSESSED_LABELS.contains(&label.0))
    {
        return Assessment::NotOwed;
    }
    let mut found = (ASSESSMENT_TEMPLATE.to_owned(), Standing::Unticked);
    for line in previous.0.lines() {
        let line = line.trim();
        let ticked = line.starts_with("- [x]") || line.starts_with("- [X]");
        if !(ticked || line.starts_with("- [ ]")) {
            continue;
        }
        let Some((before, _)) = line.split_once(ASSESSMENT_MARKER)
        else {
            continue;
        };
        let Some((_, written)) = before.split_once(ASSESSMENT_LABEL)
        else {
            continue;
        };
        let written = written.trim().to_owned();
        let standing = if !ticked {
            Standing::Unticked
        }
        else if verdict_shape(DocText(&written)).0 {
            Standing::Settled
        }
        else {
            Standing::Malformed
        };
        found = (written, standing);
    }
    Assessment::Owed {
        written: found.0,
        standing: found.1,
    }
}

/// Whether `written` is a lint assessment's verdict.
///
/// # Specification
/// - ensures: true exactly for `could: yes — <lint>, <reference>`, the lint a
///   non-empty name of lowercase letters, digits and underscores and the
///   reference non-empty, and for `could: no — <sentence>`, the sentence
///   non-empty; neither part may be a `<…>` placeholder. Backticks around the
///   whole are ignored.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — both shapes pass; a `could: yes` without a lint, the
///   template itself and free text fail.
/// - witness: `gates::obligations::a_performance_fix_owes_a_lint_assessment`
#[inline]
#[must_use]
#[spec(ensures: |output| !output.0 || written.0.contains("could: "))]
pub fn verdict_shape(written: DocText<'_>) -> InShape
{
    let written = written.0.trim().trim_matches('`').trim();
    let filled = |part: &str| !part.is_empty() && !part.starts_with('<');
    if let Some(rest) = written.strip_prefix("could: yes — ") {
        return InShape(rest.split_once(", ").is_some_and(|(lint, reference)| {
            let lint = lint.trim();
            filled(lint)
                && lint
                    .chars()
                    .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
                && filled(reference.trim())
        }));
    }
    InShape(
        written
            .strip_prefix("could: no — ")
            .is_some_and(|sentence| filled(sentence.trim())),
    )
}

/// The lint assessment's task line and its instruction, for the body.
///
/// # Specification
/// trivial.
fn assessment_lines(assessment: &Assessment) -> Vec<String>
{
    let Assessment::Owed {
        ref written,
        standing,
    } = *assessment
    else {
        return Vec::new();
    };
    let mut lines = vec![
        String::new(),
        "### Lint assessment".to_owned(),
        String::new(),
        "This pull request works a performance issue, so it states whether a lint could have \
         refused the pattern. Replace the text after the dash with `could: yes — <lint>, \
         <reference>` or `could: no — <one sentence>`, then tick the box."
            .to_owned(),
        String::new(),
        format!(
            "- [{}] {ASSESSMENT_LABEL}{written} {ASSESSMENT_MARKER}",
            if standing == Standing::Unticked {
                ' '
            }
            else {
                'x'
            }
        ),
    ];
    if standing == Standing::Malformed {
        lines.extend([
            String::new(),
            "⚠️ The ticked verdict is in neither shape.".to_owned(),
        ]);
    }
    lines
}
