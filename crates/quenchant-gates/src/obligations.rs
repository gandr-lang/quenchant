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
//! # Witnesses
//!
//! **Discharge.** An executable obligation is discharged by rerunning the
//! specification's witnesses with specifications enforced; the report lists
//! them and the nextest selection that runs them. A prose obligation is
//! discharged in the pull request: the comment the CI posts carries a
//! "still holds" and an "updated" box per obligation, and a ticked box is
//! read back from the previous comment on the next run. An acknowledgement
//! holds for the revisions it was given against: the obligation's key
//! carries the specification's revision and each changed dependency's, so a
//! later change to either reopens it. A clause whose own lines the change
//! edits is recorded as updated. A judge with a rubric replaces the boxes
//! later; the key is what its verdict will be recorded against.

use alloc::collections::BTreeMap;
use alloc::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use anodized::spec;

use crate::GateError;
use crate::evidence::Graph;
use crate::evidence::LineSpan;
use crate::evidence::Naming;
use crate::evidence::read_prose;
use crate::evidence::read_records;
use crate::inventory::Workspace;
use crate::semantic::CommandLine;
use crate::semantic::Contains;
use crate::semantic::Count;
use crate::semantic::DocText;
use crate::semantic::ErrorMessage;
use crate::semantic::GitRevision;
use crate::semantic::IntegrationTarget;
use crate::semantic::LineNumber;
use crate::semantic::SourceText;

/// The marker that opens the pull-request comment, so CI finds its own.
pub const COMMENT_MARKER: &str = "<!-- quenchant-obligations -->";

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
///   carries; its reruns are [`reruns`] of those.
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
///   `acknowledged` records its key, else `Open`.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — in a chain where the third specification names the second
///   and the second names the first, changing the first obligates the second
///   only; changing the first and the second's specification obligates both.
/// - witness: `gates::obligations::an_unchanged_reverified_specification_stops_the_cascade`
/// - witness: `gates::obligations::an_acknowledgement_holds_only_at_its_revisions`
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
    grouped
        .into_iter()
        .map(|((specification, clause, naming), dependencies)| {
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
                            changes.0.get(&item.position.file).is_some_and(|file| {
                                file.iter().any(|&change| touches(change, lines).0)
                            })
                        });
                    if edited {
                        Discharge::Updated
                    }
                    else {
                        acknowledged.get(&key).copied().unwrap_or(Discharge::Open)
                    }
                },
            };
            Obligation {
                specification,
                clause,
                naming,
                changed: dependencies,
                key,
                discharge,
            }
        })
        .collect()
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
            let integration =
                IntegrationTarget(witness.split_once("::").is_some_and(|(target, _)| {
                    member.integration_targets.iter().any(|name| name == target)
                }));
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
        format!(
            "{naming}: {} `{}` names {} [{discharge}]",
            obligation.specification,
            obligation.clause,
            obligation
                .changed
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join(", "),
        )
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

/// The report as the pull-request comment.
///
/// # Specification
/// - ensures: opens with [`COMMENT_MARKER`]; states the base and counts; lists
///   each prose obligation with a "still holds" and an "updated" task box,
///   ticked as the obligation stands, each carrying its `ack` marker and key so
///   [`acknowledgements`] reads it back; then the executable obligations, and
///   the witnesses rerun up to [`LISTED_WITNESSES`] with the count of the rest.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — a comment rendered from a report and ticked reads back as
///   the acknowledgement it records.
/// - witness: `gates::obligations::an_acknowledgement_holds_only_at_its_revisions`
#[inline]
#[must_use]
#[spec(ensures: |ref output| output.starts_with(COMMENT_MARKER))]
pub fn render_comment(report: &Report) -> String
{
    let prose: Vec<&Obligation> = report
        .obligations
        .iter()
        .filter(|obligation| obligation.naming == Naming::Prose)
        .collect();
    let executable: Vec<&Obligation> = report
        .obligations
        .iter()
        .filter(|obligation| obligation.naming == Naming::Executable)
        .collect();
    let settled = prose
        .iter()
        .filter(|obligation| obligation.discharge != Discharge::Open)
        .count();
    let mut lines = vec![
        COMMENT_MARKER.to_owned(),
        "## Re-verification obligations".to_owned(),
        String::new(),
        format!(
            "Against `{}`: {} changed items; {} executable and {} prose obligations; {} \
             witnesses rerun with specifications enforced.",
            report.base,
            report.changed.len(),
            executable.len(),
            prose.len(),
            report.reruns.len()
        ),
        String::new(),
        format!(
            "### Prose clauses to re-check ({settled} of {} settled)",
            prose.len()
        ),
        String::new(),
    ];
    if prose.is_empty() {
        lines.push("None.".to_owned());
    }
    else {
        lines.push(
            "Re-read each clause against the changed items it names, then tick one box. A tick \
             holds until the clause's item or a named item changes again."
                .to_owned(),
        );
        lines.push(String::new());
    }
    for obligation in prose {
        lines.push(format!(
            "- `{}` `{}` names {}",
            obligation.specification,
            obligation.clause,
            code_list(&obligation.changed)
        ));
        for (label, kind, ticked) in [
            (
                "still holds",
                "still-holds",
                obligation.discharge == Discharge::StillHolds,
            ),
            (
                "updated",
                "updated",
                obligation.discharge == Discharge::Updated,
            ),
        ] {
            lines.push(format!(
                "  - [{}] {label} <!-- ack {kind} {} -->",
                if ticked { 'x' } else { ' ' },
                obligation.key
            ));
        }
    }
    lines.push(String::new());
    lines.push(format!("### Executable obligations ({})", executable.len()));
    lines.push(String::new());
    if executable.is_empty() {
        lines.push("None.".to_owned());
    }
    lines.extend(executable.iter().map(|obligation| {
        format!(
            "- `{}` `{}` names {}",
            obligation.specification,
            obligation.clause,
            code_list(&obligation.changed)
        )
    }));
    lines.push(String::new());
    lines.push(format!("### Witnesses rerun ({})", report.reruns.len()));
    lines.push(String::new());
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

/// The number of prose obligations nobody has settled.
///
/// # Specification
/// trivial.
#[inline]
#[must_use]
pub fn open_count(report: &Report) -> Count
{
    Count(
        report
            .obligations
            .iter()
            .filter(|obligation| obligation.discharge == Discharge::Open)
            .count(),
    )
}
