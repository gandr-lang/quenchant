//! The specification dependency graph: which specification clause names which
//! item.
//!
//! Two sources feed the graph, and both name items by definition path:
//!
//! - The policy library's records, written when its Dylint configuration names
//!   an `evidence_directory`: an `item` record for every function, constant,
//!   static, type, trait, variant and field (path, file, start position, line
//!   extent, documentation lines, revision) and a `clause` record for every
//!   executable clause, listing the items the compiler resolved inside it.
//! - Rustdoc's JSON output, whose `links` table maps each written intra-doc
//!   link of an item to the item rustdoc resolved. The gate parses the item's
//!   documentation itself, with the Markdown parser rustdoc runs, to learn
//!   which clause each link sits in.
//!
//! A rustdoc item is joined to its record by file, line and column, the start
//! position both tools report for the same definition. A link target in the
//! same crate is joined the same way; a target in another crate is joined by
//! the path rustdoc's `paths` table gives, or, when that table has no entry
//! (an associated item or a variant of a foreign type), by the written link
//! matched against the end of every indexed path. Where the join is ambiguous
//! every candidate is kept: a spurious obligation costs a re-read, a missed
//! one costs the evidence.
//!
//! # Boundaries
//!
//! Only clauses carry obligations. A link counts when it sits in a bullet of
//! the `# Specification` section, keyed by the bullet's lead word
//! (`requires`, `ensures`, …), or anywhere in an `# Errors`, `# Panics`,
//! `# Safety` or `# Termination` section, keyed by the section. The summary,
//! the `# Adequacy` section and examples name things without stating a claim
//! about them.

use alloc::collections::BTreeMap;
use alloc::collections::BTreeSet;
use std::path::Path;
use std::path::PathBuf;

use anodized::spec;
use pulldown_cmark::BrokenLink;
use pulldown_cmark::CowStr;
use pulldown_cmark::Event;
use pulldown_cmark::Options;
use pulldown_cmark::Parser;
use pulldown_cmark::Tag;
use pulldown_cmark::TagEnd;
use quenchant_shape::shape::Maybe;

use crate::GateError;
use crate::semantic::ColumnNumber;
use crate::semantic::Contains;
use crate::semantic::DocText;
use crate::semantic::ErrorMessage;
use crate::semantic::FileExtension;
use crate::semantic::LineNumber;
use crate::semantic::SourceText;

quenchant_shape::reason_enum! {
    /// An item can carry no documentation.
    pub mod documentation {
        /// Why an item has no documentation lines.
        #[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
        pub enum Undocumented {
            /// The item carries no doc attribute.
            NoDocumentation,
        }
    }
}

quenchant_shape::reason_enum! {
    /// A position can be missing or name no indexed item.
    pub mod position {
        /// Why no item is found at a position.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Unindexed {
            /// The rustdoc entry carries no source span.
            NoSpan,
            /// No item record starts at the position.
            NoItem,
        }
    }
}

quenchant_shape::reason_enum! {
    /// A documentation reader can be outside every heading and clause.
    mod reading {
        /// Why no heading text or clause is being read.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Outside {
            /// The reader is between headings or clauses.
            Outside,
        }
    }
}

/// An inclusive range of source lines.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct LineSpan
{
    /// The first line, counted from one.
    pub first: LineNumber,
    /// The last line, at or after `first`.
    pub last: LineNumber,
}

impl LineSpan
{
    /// Whether `line` lies in the span.
    ///
    /// # Specification
    /// trivial.
    #[inline]
    #[must_use]
    pub fn contains(
        self,
        line: LineNumber,
    ) -> Contains
    {
        Contains(self.first <= line && line <= self.last)
    }
}

/// Where a definition starts.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Position
{
    /// The source file, relative to the workspace root when inside it.
    pub file: String,
    /// The line, counted from one.
    pub line: LineNumber,
    /// The column, counted from one.
    pub column: ColumnNumber,
}

/// One item the policy library indexed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Item
{
    /// The definition path, starting with the crate's name.
    pub path: String,
    /// Where the definition starts.
    pub position: Position,
    /// The lines the item spans with its attributes and documentation.
    pub extent: LineSpan,
    /// The lines the item's documentation spans.
    pub docs: Maybe<LineSpan, documentation::Undocumented>,
    /// The fingerprint of the item's source text.
    pub revision: String,
}

/// How a specification names an item.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Naming
{
    /// An executable `#[spec]` clause resolves to the item.
    Executable,
    /// A documentation clause links the item.
    Prose,
}

/// One named dependency of one specification clause.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Edge
{
    /// The specified item's path.
    pub specification: String,
    /// The clause key: `requires`, `ensures`, `errors`, …
    pub clause: String,
    /// Whether the clause is executable or prose.
    pub naming: Naming,
    /// The named item's path.
    pub target: String,
}

/// The items of a workspace and the clauses that name them.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Graph
{
    /// Every indexed item, by path.
    pub items: BTreeMap<String, Item>,
    /// Every clause's named dependencies.
    pub edges: BTreeSet<Edge>,
    /// The source lines of each prose clause that carries a link, by
    /// specification path and clause key.
    pub prose_clauses: BTreeMap<(String, String), LineSpan>,
}

/// One link written in a documentation clause.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ClauseLink
{
    /// The clause key.
    pub clause: String,
    /// The destination as rustdoc's `links` table keys it.
    pub destination: String,
    /// The source lines of the whole clause.
    pub lines: LineSpan,
}

/// One documentation line with its source line.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocumentationLine
{
    /// The source line.
    pub line: LineNumber,
    /// The text after the doc-comment marker and one space.
    pub text: String,
}

impl Graph
{
    /// The item whose definition starts at a source position.
    ///
    /// # Specification
    /// - ensures: `Present` with the path of the item recorded at `position`;
    ///   `Absent(Unindexed::NoItem)` when none is.
    /// - panics: none.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the fixture's rustdoc items join to their records
    ///   only through their position; the expected prose edges appear.
    /// - witness: `gates::obligations::the_fixture_names_its_clause_items_and_nothing_else`
    #[inline]
    #[spec(ensures: |ref output| match *output {
        Maybe::Present(ref path) => self.items.get(path).is_some_and(|item| item.position == *position),
        Maybe::Absent(_) => true,
    })]
    pub fn item_at(
        &self,
        position: &Position,
    ) -> Maybe<String, position::Unindexed>
    {
        self.items
            .values()
            .find(|item| item.position == *position)
            .map_or(Maybe::Absent(position::Unindexed::NoItem), |item| {
                Maybe::Present(item.path.clone())
            })
    }
}

/// Read the policy library's records into a graph of items and executable
/// edges.
///
/// # Specification
/// - requires: `directory` holds the JSON-lines files the policy library wrote
///   with its `evidence_directory` set, and `root` is the workspace root they
///   describe.
/// - ensures: every `item` record of every `.jsonl` file becomes an item, keyed
///   by path, the first record of a path winning; its file is made relative to
///   `root` when it lies inside it. Every `clause` record yields one executable
///   edge per name it lists.
/// - fails: [`GateError::Io`] when the directory or a file cannot be read,
///   [`GateError::Parse`] when a line is not a record.
/// - panics: none.
///
/// # Errors
/// [`GateError::Io`] for an unreadable directory or file;
/// [`GateError::Parse`] for a malformed record.
///
/// # Adequacy
/// - hypothesis: L3 — the fixture's pinned records produce exactly its
///   executable edges; a malformed record is refused, not skipped.
/// - witness: `gates::obligations::the_fixture_names_its_clause_items_and_nothing_else`
/// - witness: `gates::obligations::a_malformed_record_is_an_operational_error`
#[inline]
#[spec(ensures: |ref output| output.as_ref().is_ok_and(|graph| graph.edges.iter().all(|edge| edge.naming == Naming::Executable)) || matches!(output, &Err(GateError::Io { .. } | GateError::Parse { .. })))]
pub fn read_records(
    directory: &Path,
    root: &Path,
) -> Result<Graph, GateError>
{
    let mut graph = Graph::default();
    for path in files_with_extension(directory, FileExtension("jsonl"))? {
        let text = std::fs::read_to_string(&path)
            .map_err(|error| GateError::io(&path, ErrorMessage::from(&error.to_string())))?;
        for line in text.lines().filter(|line| !line.trim().is_empty()) {
            let record: serde_json::Value = serde_json::from_str(line)
                .map_err(|error| GateError::parse(&path, ErrorMessage::from(&error.to_string())))?;
            match record.get("record").and_then(serde_json::Value::as_str) {
                Some("item") => {
                    let item = item_from_record(&record, root)
                        .map_err(|message| GateError::parse(&path, ErrorMessage::from(&message)))?;
                    graph.items.entry(item.path.clone()).or_insert(item);
                },
                Some("clause") => {
                    let (Some(specification), Some(clause), Some(names)) = (
                        record.get("item").and_then(serde_json::Value::as_str),
                        record.get("clause").and_then(serde_json::Value::as_str),
                        record.get("names").and_then(serde_json::Value::as_array),
                    )
                    else {
                        return Err(GateError::parse(
                            &path,
                            "a clause record needs `item`, `clause` and `names`".into(),
                        ));
                    };
                    for name in names.iter().filter_map(serde_json::Value::as_str) {
                        graph.edges.insert(Edge {
                            specification: specification.to_owned(),
                            clause: clause.to_owned(),
                            naming: Naming::Executable,
                            target: name.to_owned(),
                        });
                    }
                },
                _ => {
                    return Err(GateError::parse(
                        &path,
                        "a record is neither an `item` nor a `clause` record".into(),
                    ));
                },
            }
        }
    }
    Ok(graph)
}

/// Build one item from its record.
///
/// # Specification
/// - ensures: `Ok` exactly when the record carries a string `path`, `file` and
///   `revision`, positive `line`, `column`, `start` and `end` with `start` at
///   most `end`, and `docs` that is `null` or a pair of lines; the file is made
///   relative to `root` when it lies inside it.
/// - fails: a message naming the first malformed field.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — a record missing its revision is refused with the field's
///   name.
/// - witness: `gates::obligations::a_malformed_record_is_an_operational_error`
#[spec(ensures: |ref output| output.as_ref().is_ok_and(|item| item.extent.first <= item.extent.last) || output.is_err())]
fn item_from_record(
    record: &serde_json::Value,
    root: &Path,
) -> Result<Item, String>
{
    let text = |field: &str| {
        record
            .get(field)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| format!("an item record needs a string `{field}`"))
    };
    let number = |field: &str| {
        record
            .get(field)
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .filter(|&value| value > 0_usize)
            .map(LineNumber)
            .ok_or_else(|| format!("an item record needs a positive `{field}`"))
    };
    let first = number("start")?;
    let last = number("end")?;
    if last < first {
        return Err("an item record ends before it starts".to_owned());
    }
    let docs = match *record.get("docs").unwrap_or(&serde_json::Value::Null) {
        serde_json::Value::Null => Maybe::Absent(documentation::Undocumented::NoDocumentation),
        serde_json::Value::Array(ref pair) => {
            let lines: Vec<usize> = pair
                .iter()
                .filter_map(serde_json::Value::as_u64)
                .filter_map(|value| usize::try_from(value).ok())
                .collect();
            let &[docs_first, docs_last] = lines.as_slice()
            else {
                return Err("an item record's `docs` is a pair of lines".to_owned());
            };
            Maybe::Present(LineSpan {
                first: LineNumber(docs_first),
                last: LineNumber(docs_last),
            })
        },
        serde_json::Value::Bool(_)
        | serde_json::Value::Number(_)
        | serde_json::Value::String(_)
        | serde_json::Value::Object(_) => {
            return Err("an item record's `docs` is a pair of lines or null".to_owned());
        },
    };
    Ok(Item {
        path: text("path")?,
        position: Position {
            file: relative(SourceText::from(&text("file")?), root),
            line: number("line")?,
            column: ColumnNumber(number("column")?.0),
        },
        extent: LineSpan { first, last },
        docs,
        revision: text("revision")?,
    })
}

/// Add the prose edges rustdoc's JSON output resolves.
///
/// # Specification
/// - requires: `graph` holds the records of the same workspace revision the
///   rustdoc output describes, and `root` is that workspace's root.
/// - ensures: for every local rustdoc item joined to a record by position,
///   every link [`clause_links`] finds in a clause of the record's
///   documentation, and that rustdoc resolved, adds one prose edge per item
///   [`link_targets`] names, and records the clause's lines.
/// - provides: links rustdoc did not resolve, links outside clauses and links
///   to items without a record add nothing.
/// - fails: [`GateError::Io`] when a file cannot be read and
///   [`GateError::Parse`] when one is not rustdoc JSON.
/// - panics: none.
/// - executable: none — the outcome is the mutation of `graph`, which the
///   predicate cannot compare with its prior state without a copy.
///
/// # Errors
/// [`GateError::Io`] for an unreadable file; [`GateError::Parse`] for one
/// that is not rustdoc JSON.
///
/// # Adequacy
/// - hypothesis: L3 — the fixture's prose clause links one item, its summary
///   and adequacy sections link others; only the clause's link becomes an edge.
/// - witness: `gates::obligations::the_fixture_names_its_clause_items_and_nothing_else`
#[inline]
pub fn read_prose(
    graph: &mut Graph,
    directory: &Path,
    root: &Path,
) -> Result<(), GateError>
{
    let mut sources: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let recorded: BTreeSet<String> = graph
        .items
        .keys()
        .filter_map(|path| path.trim_start_matches('<').split("::").next())
        .map(str::to_owned)
        .collect();
    for path in files_with_extension(directory, FileExtension("json"))? {
        let text = std::fs::read_to_string(&path)
            .map_err(|error| GateError::io(&path, ErrorMessage::from(&error.to_string())))?;
        let document: serde_json::Value = serde_json::from_str(&text)
            .map_err(|error| GateError::parse(&path, ErrorMessage::from(&error.to_string())))?;
        let (Some(index), Some(paths)) = (
            document.get("index").and_then(serde_json::Value::as_object),
            document.get("paths").and_then(serde_json::Value::as_object),
        )
        else {
            return Err(GateError::parse(
                &path,
                "rustdoc JSON needs an `index` and a `paths` table".into(),
            ));
        };
        let outside = document
            .get("external_crates")
            .and_then(serde_json::Value::as_object)
            .map(|crates| {
                crates
                    .iter()
                    .filter(|&(_, krate)| {
                        krate
                            .get("name")
                            .and_then(serde_json::Value::as_str)
                            .is_none_or(|name| !recorded.contains(name))
                    })
                    .map(|(id, _)| id.clone())
                    .collect()
            })
            .unwrap_or_default();
        let tables = RustdocTables {
            index,
            paths,
            outside,
        };
        for entry in index.values() {
            let Some(links) = entry
                .get("links")
                .and_then(serde_json::Value::as_object)
                .filter(|links| !links.is_empty())
            else {
                continue;
            };
            let Maybe::Present(found) = rustdoc_position(entry, root)
            else {
                continue;
            };
            let Maybe::Present(specification) = graph.item_at(&found)
            else {
                continue;
            };
            let Some(item) = graph.items.get(&specification).cloned()
            else {
                continue;
            };
            let Maybe::Present(docs) = item.docs
            else {
                continue;
            };
            if !sources.contains_key(&item.position.file) {
                let file = root.join(&item.position.file);
                let source = std::fs::read_to_string(&file).map_err(|error| {
                    GateError::io(&file, ErrorMessage::from(&error.to_string()))
                })?;
                sources.insert(
                    item.position.file.clone(),
                    source.lines().map(str::to_owned).collect(),
                );
            }
            let lines = sources
                .get(&item.position.file)
                .map(|source| documentation_lines(source, docs))
                .unwrap_or_default();
            for link in clause_links(&lines) {
                let Some(target) = links.get(&link.destination)
                else {
                    continue;
                };
                let targets = link_targets(
                    graph,
                    &tables,
                    target,
                    DocText::from(&link.destination),
                    root,
                );
                for resolved in targets {
                    graph.edges.insert(Edge {
                        specification: specification.clone(),
                        clause: link.clause.clone(),
                        naming: Naming::Prose,
                        target: resolved,
                    });
                    graph
                        .prose_clauses
                        .insert((specification.clone(), link.clause.clone()), link.lines);
                }
            }
        }
    }
    Ok(())
}

/// The tables of one crate's rustdoc JSON that resolve a link.
struct RustdocTables<'json>
{
    /// Local items by id.
    index: &'json serde_json::Map<String, serde_json::Value>,
    /// Item paths by id, foreign items included.
    paths: &'json serde_json::Map<String, serde_json::Value>,
    /// The ids of the external crates no record belongs to.
    outside: BTreeSet<String>,
}

/// The workspace items a resolved rustdoc link names.
///
/// # Specification
/// - ensures: a target in the crate's index with a span names the item recorded
///   at that position; a target in the `paths` table of a crate some record
///   belongs to names the recorded item with that path, or, failing that, every
///   recorded item of the same crate with the same last segment; any other
///   target, written with at least two path segments, names every recorded item
///   whose path ends, segment by segment, with the written link stripped of
///   backticks, a disambiguator and a call or macro suffix.
/// - provides: an empty set for a target in the `paths` table of a crate no
///   record belongs to, whatever path it is listed under (rustdoc lists a
///   standard-library variant under a local alias of its enum), and for a
///   target in neither table written as one bare segment: rustdoc's JSON
///   describes neither a prelude item (`Ok`, `Err`, `Some`, `None`) nor a
///   dependency's variant, field or associated item, so a bare name cannot tell
///   them apart, and matching it by its last segment would name every workspace
///   item spelled alike.
/// - panics: none.
/// - executable: none — the outcome joins three tables; a predicate would
///   repeat the join.
///
/// # Adequacy
/// - hypothesis: L3 — the fixture links a local constant; the replay fixture
///   links a table from functions in the same crate; each resolves to exactly
///   its item. The foreign fixture links the prelude's `Ok` and `Err` beside
///   its own `<Daily as FromStr>::Err`, and resolves to nothing.
/// - witness: `gates::obligations::the_fixture_names_its_clause_items_and_nothing_else`
/// - witness: `gates::obligations::the_tag_table_replay_lists_both_preconditions`
/// - witness: `gates::obligations::a_link_to_a_foreign_item_names_no_workspace_item`
fn link_targets(
    graph: &Graph,
    tables: &RustdocTables<'_>,
    target: &serde_json::Value,
    written: DocText<'_>,
    root: &Path,
) -> BTreeSet<String>
{
    let id = target
        .as_str()
        .map_or_else(|| target.to_string(), str::to_owned);
    if let Some(local) = tables.index.get(&id)
        && let Maybe::Present(found) = rustdoc_position(local, root)
    {
        return match graph.item_at(&found) {
            Maybe::Present(path) => BTreeSet::from([path]),
            Maybe::Absent(_) => BTreeSet::new(),
        };
    }
    if let Some(summary) = tables.paths.get(&id)
        && let Some(segments) = summary.get("path").and_then(serde_json::Value::as_array)
    {
        if summary
            .get("crate_id")
            .is_some_and(|krate| tables.outside.contains(&krate.to_string()))
        {
            return BTreeSet::new();
        }
        let segments: Vec<&str> = segments
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect();
        let path = segments.join("::");
        if graph.items.contains_key(&path) {
            return BTreeSet::from([path]);
        }
        let (Some(krate), Some(last)) = (segments.first(), segments.last())
        else {
            return BTreeSet::new();
        };
        let prefix = format!("{krate}::");
        let suffix = format!("::{last}");
        return graph
            .items
            .keys()
            .filter(|candidate| candidate.starts_with(&prefix) && candidate.ends_with(&suffix))
            .cloned()
            .collect();
    }
    let unquoted = written.0.trim_matches('`');
    let spelled = unquoted
        .split_once('@')
        .map_or(unquoted, |(_, name)| name)
        .trim_end_matches("()")
        .trim_end_matches('!');
    if !spelled.contains("::") {
        return BTreeSet::new();
    }
    let suffix = format!("::{spelled}");
    graph
        .items
        .keys()
        .filter(|candidate| candidate.ends_with(&suffix))
        .cloned()
        .collect()
}

/// Where rustdoc places an item.
///
/// # Specification
/// - ensures: `Present` exactly when the entry carries a `span` with a string
///   `filename` and a `begin` pair of numbers; the file is made relative to
///   `root` when it lies inside it.
/// - provides: `position::Unindexed::NoSpan` otherwise.
/// - panics: none.
/// - executable: none — the outcome is a projection of an untyped JSON value.
///
/// # Adequacy
/// - hypothesis: L3 — every fixture prose edge is found through this position;
///   a wrong projection loses the edge.
/// - witness: `gates::obligations::the_fixture_names_its_clause_items_and_nothing_else`
fn rustdoc_position(
    entry: &serde_json::Value,
    root: &Path,
) -> Maybe<Position, position::Unindexed>
{
    let span = entry.get("span");
    let file = span
        .and_then(|span| span.get("filename"))
        .and_then(serde_json::Value::as_str);
    let begin: Vec<usize> = span
        .and_then(|span| span.get("begin"))
        .and_then(serde_json::Value::as_array)
        .map(|begin| {
            begin
                .iter()
                .filter_map(serde_json::Value::as_u64)
                .filter_map(|value| usize::try_from(value).ok())
                .collect()
        })
        .unwrap_or_default();
    match (file, begin.as_slice()) {
        (Some(file), &[line, column]) => Maybe::Present(Position {
            file: relative(SourceText::from(file), root),
            line: LineNumber(line),
            column: ColumnNumber(column),
        }),
        _ => Maybe::Absent(position::Unindexed::NoSpan),
    }
}

/// Write a source path relative to the workspace root when it lies inside it.
///
/// # Specification
/// - ensures: an absolute path under `root` loses that prefix; any other path
///   is returned as written.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the fixture's records spell the file absolutely and its
///   rustdoc output relatively, and the two still join.
/// - witness: `gates::obligations::the_fixture_names_its_clause_items_and_nothing_else`
#[spec(ensures: |ref output| Path::new(file.0).strip_prefix(root).is_ok() || output == file.0)]
fn relative(
    file: SourceText<'_>,
    root: &Path,
) -> String
{
    Path::new(file.0).strip_prefix(root).map_or_else(
        |_| file.0.to_owned(),
        |inside| inside.to_string_lossy().into_owned(),
    )
}

/// Every file of a directory with an extension, sorted.
///
/// # Specification
/// - ensures: returns the directory's regular entries whose extension is
///   `extension`, without descending, in sorted order.
/// - fails: [`GateError::Io`] when the directory cannot be listed.
/// - panics: none.
///
/// # Errors
/// [`GateError::Io`] when the directory cannot be listed.
///
/// # Adequacy
/// - hypothesis: L3 — the fixture's records and rustdoc output are found in
///   directories beside files of other kinds.
/// - witness: `gates::obligations::the_fixture_names_its_clause_items_and_nothing_else`
#[spec(ensures: |ref output| output.as_ref().is_ok_and(|files| files.is_sorted() && files.iter().all(|file| file.extension().is_some_and(|found| found == extension.0))) || matches!(output, &Err(GateError::Io { .. })))]
fn files_with_extension(
    directory: &Path,
    extension: FileExtension<'_>,
) -> Result<Vec<PathBuf>, GateError>
{
    let entries = std::fs::read_dir(directory)
        .map_err(|error| GateError::io(directory, ErrorMessage::from(&error.to_string())))?;
    let mut files = Vec::new();
    for entry in entries {
        let entry = entry
            .map_err(|error| GateError::io(directory, ErrorMessage::from(&error.to_string())))?;
        let path = entry.path();
        if path.is_file() && path.extension().is_some_and(|found| found == extension.0) {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

/// The documentation lines of an item, read from its source.
///
/// # Specification
/// - requires: `source` holds the item's file, one entry per line.
/// - ensures: returns, for each line of `docs` that is a `///` or `//!`
///   comment, the line and its text after the marker and one space, in source
///   order; attribute lines between doc lines are skipped.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the fixture's clause link is found on its source line.
/// - witness: `gates::obligations::the_fixture_names_its_clause_items_and_nothing_else`
#[inline]
#[must_use]
#[spec(ensures: |ref output| output.iter().all(|line| docs.contains(line.line).0))]
pub fn documentation_lines(
    source: &[String],
    docs: LineSpan,
) -> Vec<DocumentationLine>
{
    let mut lines = Vec::new();
    for (offset, text) in source
        .iter()
        .enumerate()
        .skip(docs.first.0.saturating_sub(1_usize))
    {
        let line = LineNumber(offset.saturating_add(1_usize));
        if line > docs.last {
            break;
        }
        let trimmed = text.trim_start();
        let Some(rest) = trimmed
            .strip_prefix("///")
            .or_else(|| trimmed.strip_prefix("//!"))
        else {
            continue;
        };
        lines.push(DocumentationLine {
            line,
            text: rest.strip_prefix(' ').unwrap_or(rest).to_owned(),
        });
    }
    lines
}

/// Which clause each link of a documentation block sits in.
///
/// # Specification
/// - ensures: returns one entry per link inside a top-level bullet of a `#
///   Specification` section, keyed by the text before the bullet's first colon,
///   and per link anywhere in an `# Errors`, `# Panics`, `# Safety` or `#
///   Termination` section, keyed by the section's name in lower case; each
///   entry carries the destination as rustdoc keys it (the reference text,
///   backticks included, for a shortcut or collapsed reference without a
///   definition) and the source lines of its whole clause.
/// - provides: links in the summary, in other sections and in code are not
///   returned.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the fixture links one item from a `requires` bullet and
///   others from its summary and adequacy section; only the first is returned.
/// - witness: `gates::obligations::the_fixture_names_its_clause_items_and_nothing_else`
#[inline]
#[must_use]
#[spec(ensures: |ref output| output.iter().all(|link| link.lines.first <= link.lines.last && !link.clause.is_empty()))]
pub fn clause_links(lines: &[DocumentationLine]) -> Vec<ClauseLink>
{
    let text = lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let mut starts = vec![0_usize];
    for (offset, character) in text.char_indices() {
        if character == '\n' {
            starts.push(offset.saturating_add(1_usize));
        }
    }
    let line_of = |offset: usize| {
        let index = starts
            .partition_point(|&start| start <= offset)
            .saturating_sub(1_usize);
        lines
            .get(index)
            .or_else(|| lines.last())
            .map_or(LineNumber(0_usize), |line| line.line)
    };
    // A reference without a definition resolves to its own text, backticks
    // included, as rustdoc keys it before resolving the text as a path; no
    // answer would leave a shortcut link as plain text and lose it.
    let reference_destination: fn(BrokenLink<'_>) -> Option<(CowStr<'_>, CowStr<'_>)> =
        |link| Some((link.reference, CowStr::Borrowed("")));
    let events: Vec<_> =
        Parser::new_with_broken_link_callback(&text, Options::empty(), Some(reference_destination))
            .into_offset_iter()
            .collect();
    let headings: Vec<usize> = events
        .iter()
        .filter(|pair| matches!(pair.0, Event::Start(Tag::Heading { .. })))
        .map(|pair| pair.1.start)
        .collect();
    let section_last = |from: usize| {
        headings.iter().find(|&&start| start > from).map_or_else(
            || line_of(text.len().saturating_sub(1_usize)),
            |&start| line_of(start.saturating_sub(1_usize)),
        )
    };
    let mut found = Vec::new();
    let mut section = Section::Other;
    let mut heading: Maybe<(String, usize), reading::Outside> =
        Maybe::Absent(reading::Outside::Outside);
    let mut depth = 0_usize;
    let mut clause: Maybe<(String, LineSpan), reading::Outside> =
        Maybe::Absent(reading::Outside::Outside);
    let mut key_pending = false;
    for (event, range) in events {
        #[expect(
            clippy::wildcard_enum_match_arm,
            reason = "`Event` is pulldown-cmark's event stream; only headings, lists, items, \
                      text and links move the clause being read"
        )]
        match event {
            Event::Start(Tag::Heading { .. }) => {
                heading = Maybe::Present((String::new(), range.start));
                clause = Maybe::Absent(reading::Outside::Outside);
                section = Section::Other;
            },
            Event::End(TagEnd::Heading(_)) => {
                let Maybe::Present((title, start)) =
                    core::mem::replace(&mut heading, Maybe::Absent(reading::Outside::Outside))
                else {
                    continue;
                };
                let title = title.trim();
                section = match title {
                    "Specification" => Section::Specification,
                    "Errors" | "Panics" | "Safety" | "Termination" => {
                        clause = Maybe::Present((title.to_lowercase(), LineSpan {
                            first: line_of(start),
                            last: section_last(start),
                        }));
                        Section::Whole
                    },
                    _ => Section::Other,
                };
            },
            Event::Start(Tag::List(_)) => depth = depth.saturating_add(1_usize),
            Event::End(TagEnd::List(_)) => depth = depth.saturating_sub(1_usize),
            Event::Start(Tag::Item) if depth == 1_usize && section == Section::Specification => {
                clause = Maybe::Present((String::new(), LineSpan {
                    first: line_of(range.start),
                    last: line_of(range.end.saturating_sub(1_usize)),
                }));
                key_pending = true;
            },
            Event::End(TagEnd::Item) if depth == 1_usize && section == Section::Specification => {
                clause = Maybe::Absent(reading::Outside::Outside);
                key_pending = false;
            },
            Event::Text(ref fragment) | Event::Code(ref fragment) => {
                if let Maybe::Present((ref mut title, _)) = heading {
                    title.push_str(fragment);
                }
                if key_pending && let Maybe::Present((ref mut key, _)) = clause {
                    key_pending = false;
                    fragment
                        .split(':')
                        .next()
                        .unwrap_or_default()
                        .trim()
                        .clone_into(key);
                }
            },
            Event::Start(Tag::Link { ref dest_url, .. }) => {
                if let Maybe::Present((ref key, lines)) = clause
                    && !key.is_empty()
                {
                    found.push(ClauseLink {
                        clause: key.clone(),
                        destination: dest_url.to_string(),
                        lines,
                    });
                }
            },
            _ => {},
        }
    }
    found
}

/// The documentation section a reader is in.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Section
{
    /// `# Specification`: clauses are its top-level bullets.
    Specification,
    /// A section that is one clause as a whole.
    Whole,
    /// The summary or a section that states no clause.
    Other,
}

/// The witnesses an item's `# Adequacy` section names.
///
/// # Specification
/// - requires: `source` is the complete text of `file`.
/// - ensures: returns the witness paths of the claims
///   [`crate::witnesses::witness_claims`] reads from `source` whose bullet lies
///   in `docs`, in source order.
/// - fails: [`GateError::Parse`] when `source` is not parseable Rust.
/// - panics: none.
///
/// # Errors
/// [`GateError::Parse`] when `source` does not parse.
///
/// # Adequacy
/// - hypothesis: L3 — the fixture's obligated specification lists its own
///   witness and not a neighbour's.
/// - witness: `gates::obligations::changing_a_named_item_obligates_the_specification`
#[inline]
#[spec(ensures: |ref output| output.is_ok() || matches!(output, &Err(GateError::Parse { .. })))]
pub fn witnesses_in(
    file: &Path,
    source: SourceText<'_>,
    docs: LineSpan,
) -> Result<Vec<String>, GateError>
{
    Ok(crate::witnesses::witness_claims(file, source)?
        .into_iter()
        .filter(|claim| docs.contains(claim.line).0)
        .map(|claim| claim.witness)
        .collect())
}
