//! Re-verification obligations from a change: the fixtures whose records the
//! policy library's UI suite pins, their rustdoc links, and one edit at a
//! time.

use alloc::collections::BTreeMap;
use alloc::collections::BTreeSet;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use quenchant_gates::GateError;
use quenchant_gates::evidence::Edge;
use quenchant_gates::evidence::Graph;
use quenchant_gates::evidence::Item;
use quenchant_gates::evidence::LineSpan;
use quenchant_gates::evidence::Naming;
use quenchant_gates::evidence::Position;
use quenchant_gates::evidence::documentation;
use quenchant_gates::evidence::read_prose;
use quenchant_gates::evidence::read_records;
use quenchant_gates::obligations::COMMENT_MARKER;
use quenchant_gates::obligations::Change;
use quenchant_gates::obligations::ChangedLines;
use quenchant_gates::obligations::Discharge;
use quenchant_gates::obligations::Obligation;
use quenchant_gates::obligations::Report;
use quenchant_gates::obligations::acknowledgements;
use quenchant_gates::obligations::changed_items;
use quenchant_gates::obligations::obligations;
use quenchant_gates::obligations::render_comment;
use quenchant_gates::obligations::render_section;
use quenchant_gates::obligations::splice_section;
use quenchant_gates::semantic::ColumnNumber;
use quenchant_gates::semantic::DocText;
use quenchant_gates::semantic::LineNumber;

/// One fixture crate in its own Git repository, beside the policy library's
/// pinned records for it.
struct Fixture
{
    /// The repository: the crate, its toolchain pin, one commit.
    repository: PathBuf,
    /// The records directory.
    records: PathBuf,
    /// The crate's name, which is the fixture file's stem.
    name: String,
    /// Everything the fixture wrote.
    scratch: PathBuf,
}

impl Fixture
{
    /// Build the fixture crate from a policy-library UI fixture and commit it.
    ///
    /// # Specification
    /// trivial.
    fn new(
        name: DocText<'_>,
        test: DocText<'_>,
    ) -> Self
    {
        let scratch = std::env::temp_dir().join(format!(
            "quenchant-obligations-{}-{}",
            test.0,
            std::process::id()
        ));
        if scratch.exists() {
            std::fs::remove_dir_all(&scratch).expect("remove a stale fixture");
        }
        let repository = scratch.join("repository");
        let records = scratch.join("records");
        std::fs::create_dir_all(repository.join("src")).expect("create the fixture crate");
        std::fs::create_dir_all(&records).expect("create the records directory");
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let fixtures = workspace.join("crates/quenchant-dylints/ui_evidence");
        std::fs::copy(
            fixtures.join(format!("{}.rs", name.0)),
            repository.join(format!("src/{}.rs", name.0)),
        )
        .expect("copy the fixture source");
        let pinned = std::fs::read_to_string(fixtures.join(format!("{}.jsonl", name.0)))
            .expect("read the pinned records");
        std::fs::write(
            records.join(format!("{}.jsonl", name.0)),
            pinned.replace("\"file\":\"ui_evidence/", "\"file\":\"src/"),
        )
        .expect("write the records");
        std::fs::copy(
            workspace.join("rust-toolchain.toml"),
            repository.join("rust-toolchain.toml"),
        )
        .expect("pin the fixture's compiler");
        let facade = workspace
            .join("crates/quenchant-anodized")
            .canonicalize()
            .expect("locate the facade");
        std::fs::write(
            repository.join("Cargo.toml"),
            format!(
                "[package]\nname = \"{name}\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = \
                 false\n\n[lib]\npath = \"src/{name}.rs\"\n\n[dependencies]\nquenchant-anodized = \
                 {{ path = {facade:?} }}\n\n[workspace]\n",
                name = name.0,
            ),
        )
        .expect("write the fixture manifest");
        std::fs::write(repository.join(".gitignore"), "/target\n/Cargo.lock\n")
            .expect("ignore build output");
        let fixture = Self {
            repository,
            records,
            name: name.0.to_owned(),
            scratch,
        };
        fixture.git(DocText("init --quiet"));
        fixture.git(DocText("add --all"));
        fixture.git(DocText("commit --quiet --message base"));
        fixture
    }

    /// Run Git in the repository with a fixed identity and no signing; the
    /// command line splits at whitespace.
    ///
    /// # Specification
    /// trivial.
    fn git(
        &self,
        command: DocText<'_>,
    )
    {
        let output = Command::new("git")
            .current_dir(&self.repository)
            .args([
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.hooksPath=/dev/null",
            ])
            .args(command.0.split_whitespace())
            .env("GIT_AUTHOR_NAME", "Fixture")
            .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
            .env("GIT_COMMITTER_NAME", "Fixture")
            .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git {}: {}",
            command.0,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// Build rustdoc's JSON output for the crate as it stands.
    ///
    /// # Specification
    /// trivial.
    fn rustdoc(&self) -> PathBuf
    {
        let target = self.scratch.join("target");
        let output = Command::new(env!("CARGO"))
            .current_dir(&self.repository)
            .args(["doc", "--no-deps", "--document-private-items", "--offline"])
            .env("CARGO_TARGET_DIR", &target)
            .env("RUSTDOCFLAGS", "-Z unstable-options --output-format json")
            .env_remove("CARGO_ENCODED_RUSTDOCFLAGS")
            .env_remove("CARGO_ENCODED_RUSTFLAGS")
            .env_remove("RUSTFLAGS")
            .output()
            .expect("run rustdoc");
        assert!(
            output.status.success(),
            "rustdoc: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        target.join("doc")
    }

    /// Replace one occurrence of `from` in the fixture source.
    ///
    /// # Specification
    /// trivial.
    fn edit(
        &self,
        from: DocText<'_>,
        to: DocText<'_>,
    )
    {
        let path = self.repository.join(format!("src/{}.rs", self.name));
        let source = std::fs::read_to_string(&path).expect("read the fixture source");
        assert_eq!(
            source.matches(from.0).count(),
            1,
            "`{}` occurs once",
            from.0
        );
        std::fs::write(&path, source.replacen(from.0, to.0, 1)).expect("edit the fixture");
    }

    /// The subcommand against the committed base, writing the comment, the
    /// selection and the body beside the fixture.
    ///
    /// # Specification
    /// trivial.
    fn command(
        &self,
        rustdoc: &Path,
    ) -> Command
    {
        let mut command = Command::new(env!("CARGO_BIN_EXE_quenchant-gates"));
        command
            .arg("obligations")
            .arg("--manifest-path")
            .arg(self.repository.join("Cargo.toml"))
            .args(["--base", "HEAD", "--records"])
            .arg(&self.records)
            .arg("--rustdoc")
            .arg(rustdoc)
            .arg("--comment")
            .arg(self.scratch.join("comment.md"))
            .arg("--selection")
            .arg(self.scratch.join("selection.args"))
            .arg("--body")
            .arg(self.scratch.join("body.md"));
        command
    }

    /// The pull-request body the subcommand reads and rewrites.
    ///
    /// # Specification
    /// trivial.
    fn body(&self) -> String
    {
        std::fs::read_to_string(self.scratch.join("body.md")).expect("the body was written")
    }

    /// Run the subcommand against the committed base over a fresh body and
    /// return its console report, comment and selection.
    ///
    /// # Specification
    /// trivial.
    fn obligations(
        &self,
        rustdoc: &Path,
    ) -> (String, String, String)
    {
        std::fs::write(self.scratch.join("body.md"), "## What\n\nA change.\n")
            .expect("write the body");
        let output = self.command(rustdoc).output().expect("run the subcommand");
        assert!(
            output.status.success(),
            "obligations: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        (
            String::from_utf8(output.stdout).expect("the report is text"),
            std::fs::read_to_string(self.scratch.join("comment.md"))
                .expect("the comment was written"),
            std::fs::read_to_string(self.scratch.join("selection.args"))
                .expect("the selection was written"),
        )
    }
}

impl Drop for Fixture
{
    /// Remove everything the fixture wrote.
    ///
    /// # Specification
    /// trivial.
    fn drop(&mut self)
    {
        if self.scratch.exists() {
            std::fs::remove_dir_all(&self.scratch).expect("remove the fixture");
        }
    }
}

/// The obligation lines of a console report, without the summary or the
/// changed items.
///
/// # Specification
/// trivial.
fn obligation_lines(report: DocText<'_>) -> Vec<String>
{
    report
        .0
        .lines()
        .filter(|line| line.starts_with("executable: ") || line.starts_with("prose: "))
        .map(str::to_owned)
        .collect()
}

/// An item of a hand-built graph: one file, a line range, documentation on
/// its first lines.
///
/// # Specification
/// trivial.
fn item(
    path: DocText<'_>,
    extent: LineSpan,
    revision: DocText<'_>,
) -> Item
{
    Item {
        path: path.0.to_owned(),
        position: Position {
            file: "lib.rs".to_owned(),
            line: extent.first,
            column: ColumnNumber(1),
        },
        extent,
        docs: quenchant_shape::shape::Maybe::Absent(documentation::Undocumented::NoDocumentation),
        revision: revision.0.to_owned(),
    }
}

/// A line span.
///
/// # Specification
/// trivial.
const fn lines(
    first: LineNumber,
    last: LineNumber,
) -> LineSpan
{
    LineSpan { first, last }
}

/// A chain: `c` names `b` in an executable clause, `b` names `a` in a prose
/// clause on its lines 11 and 12.
///
/// # Specification
/// trivial.
fn chain() -> Graph
{
    let mut graph = Graph::default();
    for (path, extent) in [
        ("a", lines(LineNumber(1), LineNumber(5))),
        ("b", lines(LineNumber(10), LineNumber(20))),
        ("c", lines(LineNumber(30), LineNumber(40))),
    ] {
        graph.items.insert(
            path.to_owned(),
            item(DocText(path), extent, DocText("0123456789abcdef")),
        );
    }
    graph.edges.insert(Edge {
        specification: "b".to_owned(),
        clause: "requires".to_owned(),
        naming: Naming::Prose,
        target: "a".to_owned(),
    });
    graph.edges.insert(Edge {
        specification: "c".to_owned(),
        clause: "requires".to_owned(),
        naming: Naming::Executable,
        target: "b".to_owned(),
    });
    graph.prose_clauses.insert(
        ("b".to_owned(), "requires".to_owned()),
        lines(LineNumber(11), LineNumber(12)),
    );
    graph
}

/// Changed lines of the chain's one file.
///
/// # Specification
/// trivial.
fn changes(spans: &[LineSpan]) -> ChangedLines
{
    ChangedLines(BTreeMap::from([(
        "lib.rs".to_owned(),
        spans.iter().copied().map(Change::Lines).collect(),
    )]))
}

#[test]
fn the_fixture_names_its_clause_items_and_nothing_else()
{
    let fixture = Fixture::new(DocText("edges"), DocText("names"));
    let rustdoc = fixture.rustdoc();
    let mut graph =
        read_records(&fixture.records, &fixture.repository).expect("the records are readable");
    read_prose(&mut graph, &rustdoc, &fixture.repository).expect("the rustdoc output is readable");
    let named: BTreeSet<(String, String, Naming, String)> = graph
        .edges
        .iter()
        .map(|edge| {
            (
                edge.specification.clone(),
                edge.clause.clone(),
                edge.naming,
                edge.target.clone(),
            )
        })
        .collect();
    let expected: BTreeSet<(String, String, Naming, String)> = [
        (
            "edges::read",
            "requires",
            Naming::Executable,
            "edges::LIMIT",
        ),
        (
            "edges::read",
            "requires",
            Naming::Executable,
            "edges::within",
        ),
        ("edges::read", "requires", Naming::Prose, "edges::MARGIN"),
        (
            "edges::classify",
            "requires",
            Naming::Executable,
            "<edges::Reading as edges::Scaled>::scaled",
        ),
        (
            "edges::classify",
            "requires",
            Naming::Executable,
            "edges::Reading::value",
        ),
        (
            "edges::classify",
            "ensures",
            Naming::Executable,
            "edges::Class::High",
        ),
        (
            "edges::classify",
            "ensures",
            Naming::Executable,
            "edges::Class::Low",
        ),
        (
            "edges::is_low",
            "ensures",
            Naming::Executable,
            "edges::Class::Low",
        ),
        (
            "edges::is_low",
            "ensures",
            Naming::Executable,
            "edges::Reading::class",
        ),
    ]
    .into_iter()
    .map(|(specification, clause, naming, target)| {
        (
            specification.to_owned(),
            clause.to_owned(),
            naming,
            target.to_owned(),
        )
    })
    .collect();
    assert_eq!(
        named, expected,
        "two executable names and one prose link for `read`, the other clauses' names, and nothing \
         reached through a body, a summary or an adequacy section"
    );
}

#[test]
fn changing_a_named_item_obligates_the_specification()
{
    let fixture = Fixture::new(DocText("edges"), DocText("named"));
    let rustdoc = fixture.rustdoc();
    for (from, to, expected) in [
        (
            "LIMIT: u32 = 8;",
            "LIMIT: u32 = 9;",
            "executable: edges::read `requires` names edges::LIMIT [witnesses rerun]",
        ),
        (
            "value <= LIMIT && nonzero(value)",
            "value < LIMIT && nonzero(value)",
            "executable: edges::read `requires` names edges::within [witnesses rerun]",
        ),
        (
            "MARGIN: u32 = 3;",
            "MARGIN: u32 = 4;",
            "prose: edges::read `requires` names edges::MARGIN [open]",
        ),
    ] {
        fixture.edit(DocText(from), DocText(to));
        let (report, comment, selection) = fixture.obligations(&rustdoc);
        assert_eq!(
            obligation_lines(DocText(&report)),
            vec![expected.to_owned()],
            "editing `{from}` obligates `read`'s precondition alone: {report}"
        );
        assert!(
            report.contains("witness: edges tests::reads (for edges::read)"),
            "the obligated specification's own witness reruns: {report}"
        );
        assert_eq!(
            selection, "-p\nedges\n-E\n(package(=edges) & test(=tests::reads))\n",
            "the selection runs that witness and nothing else"
        );
        let prose = expected.starts_with("prose: ");
        let body = fixture.body();
        assert_eq!(
            body.contains("  - [ ] still holds <!-- ack still-holds edges::read#requires@"),
            prose,
            "the body asks for an acknowledgement exactly for the prose obligation: {body}"
        );
        assert!(
            body.starts_with("## What\n\nA change.\n\n<!-- quenchant-obligations:start -->"),
            "the section follows the body's own prose: {body}"
        );
        assert!(
            !comment.contains("- [ ]"),
            "the comment carries no task box: {comment}"
        );
        fixture.git(DocText("checkout --quiet -- ."));
    }
}

#[test]
fn an_open_obligation_fails_only_when_acknowledgement_is_required()
{
    let fixture = Fixture::new(DocText("edges"), DocText("required"));
    let rustdoc = fixture.rustdoc();
    fixture.edit(DocText("MARGIN: u32 = 3;"), DocText("MARGIN: u32 = 4;"));
    let _optional = fixture.obligations(&rustdoc);
    let refused = fixture
        .command(&rustdoc)
        .arg("--require-acknowledged")
        .output()
        .expect("run the subcommand");
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert_eq!(
        refused.status.code(),
        Some(2_i32),
        "an open prose obligation refuses the run: {stderr}"
    );
    assert!(
        stderr.contains("edges::read requires (edges::read#requires@"),
        "the refusal names the obligation and its key: {stderr}"
    );
    let ticked = fixture
        .body()
        .replacen("- [ ] still holds", "- [x] still holds", 1);
    std::fs::write(fixture.scratch.join("body.md"), &ticked).expect("tick the box");
    let accepted = fixture
        .command(&rustdoc)
        .arg("--require-acknowledged")
        .output()
        .expect("run the subcommand");
    assert!(
        accepted.status.success(),
        "the ticked body settles it: {}",
        String::from_utf8_lossy(&accepted.stderr)
    );
    assert_eq!(
        fixture.body(),
        ticked
            .replace("⚠️ 1 of 1 unticked", "✅ 1 of 1 settled")
            .replace("☐ unticked", "✅ still holds"),
        "rewriting the section keeps the tick and the prose around it, and counts it settled"
    );
}

#[test]
fn an_integration_witness_selects_its_target_binary()
{
    let fixture = Fixture::new(DocText("edges"), DocText("integration"));
    let rustdoc = fixture.rustdoc();
    std::fs::create_dir_all(fixture.repository.join("tests")).expect("create the target directory");
    std::fs::write(
        fixture.repository.join("tests/tests.rs"),
        "//! The fixture's integration target.\n\n#[test]\nfn reads() {}\n",
    )
    .expect("write the integration target");
    fixture.edit(DocText("LIMIT: u32 = 8;"), DocText("LIMIT: u32 = 9;"));
    let (report, _, selection) = fixture.obligations(&rustdoc);
    assert!(
        report.contains("witness: edges tests::reads (for edges::read)"),
        "the witness reruns: {report}"
    );
    assert_eq!(
        selection, "-p\nedges\n-E\n(package(=edges) & binary(=tests) & test(=reads))\n",
        "a witness whose first segment is an integration target selects that binary's test"
    );
}

#[test]
fn changing_an_item_reached_only_through_a_call_obligates_nothing()
{
    let fixture = Fixture::new(DocText("edges"), DocText("unrelated"));
    let rustdoc = fixture.rustdoc();
    fixture.edit(DocText("value != 0"), DocText("value > 0"));
    let (report, _, selection) = fixture.obligations(&rustdoc);
    assert!(
        report.starts_with(
            "obligations against HEAD: 1 changed items, 0 obligations, 0 witnesses to rerun\n\
             changed: edges::nonzero\n"
        ),
        "`nonzero` changed, and the specifications that reach it through `within`'s body owe \
         nothing: {report}"
    );
    assert!(selection.is_empty(), "nothing is selected: {selection}");
}

#[test]
fn the_tag_table_replay_lists_both_preconditions()
{
    let fixture = Fixture::new(DocText("tags"), DocText("replay"));
    fixture.edit(DocText("[Row; 3]"), DocText("[Row; 4]"));
    fixture.edit(
        DocText("        bounded: false,\n    },\n];"),
        DocText(
            "        bounded: false,\n    },\n    Row {\n        tag: 0x63,\n        bounded: \
             false,\n    },\n];",
        ),
    );
    let rustdoc = fixture.rustdoc();
    let (report, ..) = fixture.obligations(&rustdoc);
    assert_eq!(
        obligation_lines(DocText(&report)),
        vec![
            "prose: tags::row `requires` names tags::NODE_TAG_TABLE [open]".to_owned(),
            "prose: tags::unbounded `requires` names tags::NODE_TAG_TABLE [open]".to_owned(),
        ],
        "adding a row to the table obligates both preconditions that link it, and nothing \
         else: {report}"
    );
    let body = fixture.body();
    assert_eq!(
        body.matches("<!-- ack still-holds tags::").count(),
        2,
        "the body asks for both acknowledgements: {body}"
    );
}

#[test]
fn an_unchanged_reverified_specification_stops_the_cascade()
{
    let graph = chain();
    let body_only = changes(&[lines(LineNumber(3), LineNumber(3))]);
    let changed = changed_items(&graph, &body_only);
    assert_eq!(
        changed,
        BTreeSet::from(["a".to_owned()]),
        "only `a` changed"
    );
    let owed: Vec<(String, Discharge)> =
        obligations(&graph, &changed, &body_only, &BTreeMap::new())
            .into_iter()
            .map(|obligation| (obligation.specification, obligation.discharge))
            .collect();
    assert_eq!(
        owed,
        vec![("b".to_owned(), Discharge::Open)],
        "`b` owes re-verification; `c`, which names the unchanged `b`, does not"
    );

    let updated = changes(&[
        lines(LineNumber(3), LineNumber(3)),
        lines(LineNumber(12), LineNumber(12)),
    ]);
    let changed = changed_items(&graph, &updated);
    let owed: Vec<(String, Discharge)> = obligations(&graph, &changed, &updated, &BTreeMap::new())
        .into_iter()
        .map(|obligation| (obligation.specification, obligation.discharge))
        .collect();
    assert_eq!(
        owed,
        vec![
            ("b".to_owned(), Discharge::Updated),
            ("c".to_owned(), Discharge::Witnesses),
        ],
        "once `b`'s clause is updated, `b` is a changed item and `c` owes re-verification"
    );
}

#[test]
fn an_acknowledgement_holds_only_at_its_revisions()
{
    let mut graph = chain();
    let edit = changes(&[lines(LineNumber(3), LineNumber(3))]);
    let changed = changed_items(&graph, &edit);
    let report = Report {
        base: "base".to_owned(),
        changed: changed.clone(),
        obligations: obligations(&graph, &changed, &edit, &BTreeMap::new()),
        reruns: BTreeSet::new(),
    };
    let ticked = render_section(&report).replace("- [ ] still holds", "- [x] still holds");
    let acknowledged = acknowledgements(DocText(&ticked));
    let standing: Vec<Discharge> = obligations(&graph, &changed, &edit, &acknowledged)
        .into_iter()
        .map(|obligation| obligation.discharge)
        .collect();
    assert_eq!(
        standing,
        vec![Discharge::StillHolds],
        "the ticked box settles the obligation at the same revisions"
    );
    if let Some(dependency) = graph.items.get_mut("a") {
        "fedcba9876543210".clone_into(&mut dependency.revision);
    }
    let standing: Vec<Discharge> = obligations(&graph, &changed, &edit, &acknowledged)
        .into_iter()
        .map(|obligation| obligation.discharge)
        .collect();
    assert_eq!(
        standing,
        vec![Discharge::Open],
        "a later change to the named item reopens it"
    );
}

#[test]
fn the_comment_lists_only_the_witnesses()
{
    let graph = chain();
    let edit = changes(&[lines(LineNumber(3), LineNumber(3))]);
    let changed = changed_items(&graph, &edit);
    let report = Report {
        base: "base".to_owned(),
        changed: changed.clone(),
        obligations: obligations(&graph, &changed, &edit, &BTreeMap::new()),
        reruns: BTreeSet::new(),
    };
    let comment = render_comment(&report);
    assert_eq!(
        comment.lines().take(2).collect::<Vec<_>>(),
        [COMMENT_MARKER, "### Re-verification · ⚠️ 1 of 1 unticked"],
        "the comment opens with its marker and the body's verdict"
    );
    assert!(
        comment.contains(
            "<details>\n<summary>Witnesses rerun with specifications enforced: 0</summary>\n"
        ),
        "the witnesses are collapsed under one summary line: {comment}"
    );
    assert!(
        !comment.contains("<!-- ack ") && !comment.contains("- [ ]"),
        "the task boxes are the body's, never the comment's: {comment}"
    );
    assert_eq!(
        comment.matches("<details>").count(),
        comment.matches("</details>").count(),
        "every block closes"
    );
}

#[test]
fn the_heading_and_the_table_carry_the_verdict()
{
    let graph = chain();
    let edit = changes(&[lines(LineNumber(3), LineNumber(3))]);
    let changed = changed_items(&graph, &edit);
    let open = Report {
        base: "base".to_owned(),
        changed: changed.clone(),
        obligations: obligations(&graph, &changed, &edit, &BTreeMap::new()),
        reruns: BTreeSet::new(),
    };
    let row = |verdict: &str| format!("| `b` | `requires` | `a` | {verdict} |");
    let section = render_section(&open);
    let comment = render_comment(&open);
    for (place, text, heading) in [
        ("body", &section, "## Re-verification · ⚠️ 1 of 1 unticked"),
        (
            "comment",
            &comment,
            "### Re-verification · ⚠️ 1 of 1 unticked",
        ),
    ] {
        assert!(
            text.lines().any(|line| line == heading),
            "the {place} heading counts the open box: {text}"
        );
        assert!(
            text.contains(&format!(
                "| item | clause | names | verdict |\n| --- | --- | --- | --- |\n{}\n",
                row("☐ unticked")
            )),
            "the {place} table has one row per prose obligation: {text}"
        );
    }
    assert!(
        section.contains("<details><summary>Acknowledgements</summary>\n\n")
            && section.contains("<!-- ack updated b#requires@")
            && section.contains("\n\n</details>\n"),
        "the boxes are collapsed, a blank line inside each edge of the block: {section}"
    );

    for (ticked, verdict) in [("still holds", "✅ still holds"), ("updated", "🔁 updated")] {
        let acknowledged = acknowledgements(DocText(
            &section.replace(&format!("- [ ] {ticked}"), &format!("- [x] {ticked}")),
        ));
        let settled = Report {
            obligations: obligations(&graph, &changed, &edit, &acknowledged),
            ..open.clone()
        };
        let section = render_section(&settled);
        assert!(
            section.contains("## Re-verification · ✅ 1 of 1 settled\n")
                && section.contains(&row(verdict)),
            "a box ticked {ticked} settles the heading and the row: {section}"
        );
    }

    let owed_nothing = Report {
        base: "base".to_owned(),
        ..Report::default()
    };
    let section = render_section(&owed_nothing);
    assert!(
        section.contains("## Re-verification · ✅ nothing owed\n")
            && !section.contains("| item |")
            && !section.contains("<details>"),
        "a change that owes no prose has the verdict and neither table nor boxes: {section}"
    );

    let one_crate = Report {
        base: "base".to_owned(),
        changed: BTreeSet::from(["edges::LIMIT".to_owned()]),
        obligations: vec![Obligation {
            specification: "edges::read".to_owned(),
            clause: "requires".to_owned(),
            naming: Naming::Prose,
            changed: BTreeSet::from(["edges::LIMIT".to_owned()]),
            key: "edges::read#requires@0".to_owned(),
            discharge: Discharge::Open,
        }],
        reruns: BTreeSet::new(),
    };
    let section = render_section(&one_crate);
    assert!(
        section.contains("· items in `edges`.\n")
            && section.contains("| `read` | `requires` | `LIMIT` | ☐ unticked |"),
        "the crate every row shares is named once and dropped from the rows: {section}"
    );
}

#[test]
fn the_section_replaces_its_own_markers_only()
{
    let graph = chain();
    let edit = changes(&[lines(LineNumber(3), LineNumber(3))]);
    let changed = changed_items(&graph, &edit);
    let report = Report {
        base: "base".to_owned(),
        changed: changed.clone(),
        obligations: obligations(&graph, &changed, &edit, &BTreeMap::new()),
        reruns: BTreeSet::new(),
    };
    let section = render_section(&report);
    let gained = splice_section(DocText("## What\n\nA change.\n"), DocText(&section));
    assert_eq!(
        gained,
        format!("## What\n\nA change.\n\n{section}"),
        "a body without a section gains it at its end"
    );
    let written = format!(
        "{}\nCloses #1\n",
        gained.replace("- [ ] still holds", "- [x] still holds")
    );
    let acknowledged = acknowledgements(DocText(&written));
    let reread = Report {
        obligations: obligations(&graph, &changed, &edit, &acknowledged),
        ..report
    };
    let fresh = render_section(&reread);
    assert_eq!(
        splice_section(DocText(&written), DocText(&fresh)),
        written
            .replace("⚠️ 1 of 1 unticked", "✅ 1 of 1 settled")
            .replace("☐ unticked", "✅ still holds"),
        "a fresh section over a ticked one keeps the tick and the prose on either side, and \
         counts it settled"
    );
}

#[test]
fn a_malformed_record_is_an_operational_error()
{
    let directory = std::env::temp_dir().join(format!(
        "quenchant-obligations-malformed-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).expect("create the records directory");
    std::fs::write(
        directory.join("unit.jsonl"),
        "{\"record\":\"item\",\"path\":\"a\",\"file\":\"lib.rs\",\"line\":1,\"column\":1,\"start\":\
         1,\"end\":2,\"docs\":null}\n",
    )
    .expect("write the record");
    let outcome = read_records(&directory, Path::new("/"));
    std::fs::remove_dir_all(&directory).expect("remove the records directory");
    assert!(
        matches!(outcome, Err(GateError::Parse { ref message, .. }) if message.contains("`revision`")),
        "a record without its revision is refused, naming the field: {outcome:?}"
    );
}
