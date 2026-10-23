//! Consumer-scoped verification and repository-policy refusals.
//!
//! ```text
//! quenchant-gates anodized --manifest-path <Cargo.toml> [--require-enforcing]
//! quenchant-gates witnesses --manifest-path <Cargo.toml> [--inventory <nextest-list.json>]...
//! quenchant-gates obligations --manifest-path <Cargo.toml> --base <revision> --records <directory> --rustdoc <directory> [--previous <comment>] [--body <file>] [--comment <file>] [--selection <file>] [--labels <label,...>] [--hotpath <report>] [--require-acknowledged]
//! quenchant-gates <public-boundary | pins | action-pins | publish-allowlist | conflict-markers> [--root <repository>]
//! quenchant-gates toolchain-bump --version <stable> [--root <repository>]
//! ```
//!
//! An explicit consumer manifest selects both gates' scope, including for an
//! installed binary launched elsewhere. `anodized` classifies invocation cfgs
//! and can require panic-enabled checking; `witnesses` resolves the consumer's
//! runnable inventory, listed here or read from the `cargo nextest list
//! --message-format json` listings `--inventory` names, as a sharded test run
//! writes them where its tests were built; `obligations` reports the
//! re-verification a change owes and writes the pull-request comment and the
//! nextest selection that discharges it.
//!
//! Policy violations and operational failures both produce unsuccessful exits
//! with distinct diagnostics. Failure to obtain an inventory remains an
//! operational failure rather than evidence that its obligations passed.

extern crate alloc;

mod repository;

use std::path::Path;
use std::path::PathBuf;
use std::process::ExitCode;

use anodized::spec;
use quenchant_gates::Finding;
use quenchant_gates::GateError;
use quenchant_gates::anodized::Requirement;
use quenchant_gates::anodized::Verdict;
use quenchant_gates::anodized::invocation_state;
use quenchant_gates::hotpath;
use quenchant_gates::inventory;
use quenchant_gates::obligations;
use quenchant_gates::semantic::DocText;
use quenchant_gates::semantic::GitRevision;
use quenchant_gates::semantic::IssueLabel;
use quenchant_shape::shape::Maybe;

/// Invalid argument shapes receive the complete supported command syntax.
const USAGE: &str = "usage: quenchant-gates anodized --manifest-path <Cargo.toml> [--require-enforcing]\n       quenchant-gates witnesses --manifest-path <Cargo.toml> [--inventory <nextest-list.json>]...\n       quenchant-gates obligations --manifest-path <Cargo.toml> --base <revision> --records <directory> --rustdoc <directory> [--previous <comment>] [--body <file>] [--comment <file>] [--selection <file>] [--labels <label,...>] [--hotpath <report>] [--require-acknowledged]\n       quenchant-gates <public-boundary | pins | action-pins | publish-allowlist | conflict-markers> [--root <repository>]\n       quenchant-gates toolchain-bump --version <stable> [--root <repository>]";

/// Argument interpretation selects one consumer or repository operation.
///
/// # Specification
/// - requires: arguments name one operation and its documented scope.
/// - ensures: consumer gates use their mandatory manifest; repository commands
///   use the selected root and exit successfully only on acceptance.
/// - fails: prints the usage text and exits unsuccessfully on an unrecognized
///   invocation, and prints the gate's own error on an operational failure.
/// - panics: none.
/// - executable: none — the exit code does not retain the selected operation’s
///   evidence or emitted diagnostics; replay can perform repository edits.
///
/// # Adequacy
/// - hypothesis: L3 — invalid options fail before running a gate and actual
///   consumer outcomes remain distinct.
/// - witness: `gates::repository::subcommands_refuse_broken_fixtures`
/// - witness: `gates::repository::witness_inventory_uses_consumer_selection_over_shadowed_plugins`
#[expect(
    clippy::print_stderr,
    reason = "the gate driver is a command-line program; its usage diagnostics are its output, \
              and routing them anywhere else would leave a misinvoked gate silent"
)]
fn main() -> ExitCode
{
    let mut arguments = std::env::args().skip(1_usize);
    let Some(subcommand) = arguments.next()
    else {
        eprintln!("missing gate\n{USAGE}");
        return ExitCode::FAILURE;
    };
    if matches!(
        subcommand.as_str(),
        "public-boundary"
            | "pins"
            | "action-pins"
            | "publish-allowlist"
            | "conflict-markers"
            | "toolchain-bump"
    ) {
        return repository::dispatch(
            quenchant_gates::semantic::SourceText(&subcommand),
            arguments,
        );
    }
    if subcommand == "obligations" {
        return run_obligations(arguments);
    }
    let Some(flag) = arguments.next()
    else {
        eprintln!("missing `--manifest-path`\n{USAGE}");
        return ExitCode::FAILURE;
    };
    if flag != "--manifest-path" {
        eprintln!("unknown option `{flag}`\n{USAGE}");
        return ExitCode::FAILURE;
    }
    let Some(manifest_path) = arguments.next()
    else {
        eprintln!("`--manifest-path` takes a path\n{USAGE}");
        return ExitCode::FAILURE;
    };
    let mut requirement = Requirement::Observe;
    let mut listings = Vec::new();
    while let Some(extra) = arguments.next() {
        match (subcommand.as_str(), extra.as_str()) {
            ("anodized", "--require-enforcing") if requirement == Requirement::Observe => {
                requirement = Requirement::Enforcing;
            },
            ("witnesses", "--inventory") => {
                let Some(listing) = arguments.next()
                else {
                    eprintln!("`--inventory` takes a path\n{USAGE}");
                    return ExitCode::FAILURE;
                };
                listings.push(PathBuf::from(listing));
            },
            _ => {
                eprintln!("unexpected argument `{extra}`\n{USAGE}");
                return ExitCode::FAILURE;
            },
        }
    }
    let manifest_path = PathBuf::from(manifest_path);
    match subcommand.as_str() {
        "anodized" => anodized_gate(&manifest_path, requirement),
        "witnesses" => run_witnesses(&manifest_path, &listings),
        _ => {
            eprintln!("unknown gate `{subcommand}`\n{USAGE}");
            ExitCode::FAILURE
        },
    }
}

/// The requested policy determines acceptance of the reported invocation state.
///
/// # Specification
/// - requires: `manifest_path` names the consumer manifest.
/// - ensures: prints the measured invocation state; discarded specs always
///   fail, and an enforcing requirement accepts only panic checks.
/// - fails: reports operational query errors separately from policy failure.
/// - panics: none.
/// - executable: none — the measured state is consumed into an exit code and
///   write-only output; an independent observation would rerun Cargo.
///
/// # Adequacy
/// - hypothesis: L3 — consumer flags change the measured state and enforcing
///   policy refuses non-enforcing modes.
/// - witness: `gates::anodized::consumer_configuration_and_encoded_flags_select_the_state`
#[expect(
    clippy::print_stderr,
    clippy::print_stdout,
    reason = "the CLI report is the gate's observable verdict"
)]
fn anodized_gate(
    manifest_path: &Path,
    requirement: Requirement,
) -> ExitCode
{
    let state = match invocation_state(manifest_path) {
        Ok(state) => state,
        Err(error) => {
            eprintln!("quenchant-gates: anodized state query could not run: {error}");
            return ExitCode::FAILURE;
        },
    };
    println!("quenchant-gates: anodized invocation state: {state}");
    match state.verdict(requirement) {
        Verdict::Pass => ExitCode::SUCCESS,
        Verdict::Fail => {
            eprintln!(
                "quenchant-gates: anodized policy FAILED: discard and incompatible cfgs are forbidden; enforcing lanes require anodized_panic"
            );
            ExitCode::FAILURE
        },
    }
}

/// Witness findings determine the consumer run's process outcome.
///
/// # Specification
/// - requires: `manifest_path` names the manifest of the workspace to gate;
///   `listings`, when any, are nextest listings of its built tests.
/// - ensures: prints one line per unresolved or ambiguous witness and fails
///   when there is at least one, and prints the passing line and succeeds when
///   every witness resolves.
/// - provides: the exit status the gate CI job reads.
/// - fails: reports a failing status and a distinct "could not run" message
///   when the workspace cannot be discovered or its tests cannot be listed or
///   read from the listings, so an unmeasured run is never reported as a pass.
/// - panics: none.
/// - executable: none — the findings are consumed into write-only process
///   output; checking them here would rerun the external inventory.
///
/// # Adequacy
/// - hypothesis: L3 — a present witness passes, an absent witness fails, and a
///   broken consumer or listing cannot report a pass.
/// - witness: `gates::repository::witness_inventory_uses_consumer_selection_over_shadowed_plugins`
/// - witness: `gates::repository::saved_listings_resolve_the_witnesses_without_listing_again`
#[expect(
    clippy::print_stderr,
    clippy::print_stdout,
    reason = "G0's verdict is its console report; the CI job reads the printed findings, each \
              addressed to the source line that must change"
)]
fn run_witnesses(
    manifest_path: &Path,
    listings: &[PathBuf],
) -> ExitCode
{
    let findings = match inventory_gate(manifest_path, listings) {
        Ok(findings) => findings,
        Err(error) => {
            eprintln!("adequacy witness resolution (G0) could not run: {error}");
            return ExitCode::FAILURE;
        },
    };
    for finding in &findings {
        println!("{finding}");
    }
    if findings.is_empty() {
        println!("adequacy witness resolution (G0): every witness resolves");
        return ExitCode::SUCCESS;
    }
    let count = findings.len();
    eprintln!("adequacy witness resolution (G0): {count} unresolved obligations");
    ExitCode::FAILURE
}

/// Workspace discovery and runnable inventory precede witness resolution.
///
/// # Specification
/// - requires: `manifest_path` names the manifest of the workspace to gate;
///   `listings`, when any, are nextest listings of its built tests, which then
///   stand for the listing this gate would run.
/// - ensures: every `- witness:` bullet of every member is resolved against
///   that member's own test targets, and the findings come back in a
///   deterministic order.
/// - provides: the unreported half of [`run_witnesses`], separated so an
///   operational failure stays a `Result` rather than becoming an empty
///   verdict.
/// - fails: propagates the [`GateError`] raised by discovery, listing, or
///   reading a member's sources or a listing.
/// - panics: none.
///
/// # Errors
/// [`GateError::Tool`] when `cargo metadata` or a test listing fails, or a
/// saved listing is not nextest's or lists nothing; [`GateError::Io`] when a
/// member's source or a listing cannot be read; and [`GateError::Parse`] when
/// one of those sources is not parseable Rust.
///
/// # Adequacy
/// - hypothesis: L3 — the real consumer’s witness is resolved only against its
///   own selected inventory, listed here or read from saved listings.
/// - witness: `gates::repository::witness_inventory_uses_consumer_selection_over_shadowed_plugins`
/// - witness: `gates::repository::saved_listings_resolve_the_witnesses_without_listing_again`
#[spec(ensures: |ref output| output.as_ref().is_ok_and(|findings| findings.is_sorted()) || output.is_err())]
fn inventory_gate(
    manifest_path: &Path,
    listings: &[PathBuf],
) -> Result<Vec<Finding>, GateError>
{
    let workspace = inventory::discover(manifest_path)?;
    let catalog = if listings.is_empty() {
        inventory::catalog(&workspace)?
    }
    else {
        inventory::catalog_from(listings)?
    };
    inventory::run(&workspace, &catalog)
}

/// The options of the `obligations` subcommand that take a value.
const OBLIGATION_OPTIONS: [&str; 10] = [
    "--manifest-path",
    "--base",
    "--records",
    "--rustdoc",
    "--previous",
    "--body",
    "--comment",
    "--selection",
    "--labels",
    "--hotpath",
];

/// The `obligations` flag that fails the run while a prose obligation is open.
const REQUIRE_ACKNOWLEDGED: &str = "--require-acknowledged";

/// Report a change's re-verification obligations and write their discharge
/// inputs.
///
/// # Specification
/// - requires: arguments are `--option value` pairs among
///   [`OBLIGATION_OPTIONS`], each at most once, with `--manifest-path`,
///   `--base`, `--records` and `--rustdoc` present, and at most one
///   [`REQUIRE_ACKNOWLEDGED`] among them.
/// - ensures: reads the acknowledgements from `--previous`, or from `--body`
///   when no `--previous` is given, and the lint assessment owed under the
///   comma-separated `--labels` from `--body`, or from `--previous` when no
///   `--body` is given, and the hotpath verdict owed under the `--hotpath`
///   report from the same text; prints [`obligations::render_text`] of the
///   report; writes [`obligations::render_comment`] to `--comment`, the lines
///   of [`obligations::selection`] to `--selection`, and `--body` back with its
///   section replaced, as [`obligations::render_body`] sizes it, when given.
///   Once all of that is done it succeeds, unless [`REQUIRE_ACKNOWLEDGED`] is
///   given and a prose obligation is open or an owed lint assessment or hotpath
///   verdict unsettled: then it lists each with its key or its standing and
///   exits with status 2.
/// - fails: prints the usage on a malformed invocation, the error on an
///   operational failure, and the character count of a body whose section
///   passes [`obligations::BODY_LIMIT`] even without its table, and exits with
///   status 1 in each case; an oversized body writes none of the outputs.
/// - panics: none.
/// - executable: none — the outcome is printed and written output, consumed
///   into an exit code.
///
/// # Adequacy
/// - hypothesis: L3 — the fixture repository's invocations print the
///   obligations, write the comment, the selection and the body the tests read
///   back, and refuse an open obligation only when asked to; a labelled run
///   refuses an unticked or malformed assessment read from the body, and a red
///   hotpath report an unticked, malformed or unmoved verdict.
/// - witness: `gates::obligations::changing_a_named_item_obligates_the_specification`
/// - witness: `gates::obligations::the_tag_table_replay_lists_both_preconditions`
/// - witness: `gates::obligations::an_open_obligation_fails_only_when_acknowledgement_is_required`
/// - witness: `gates::obligations::an_unsettled_lint_assessment_fails_the_required_run`
/// - witness: `gates::obligations::an_unsettled_hotpath_verdict_fails_the_required_run`
#[expect(
    clippy::print_stderr,
    clippy::print_stdout,
    reason = "the report is the subcommand's console output; CI reads it in the job log"
)]
fn run_obligations(mut arguments: core::iter::Skip<std::env::Args>) -> ExitCode
{
    let mut options = alloc::collections::BTreeMap::new();
    let mut required = false;
    while let Some(flag) = arguments.next() {
        if flag == REQUIRE_ACKNOWLEDGED {
            if required {
                eprintln!("obligations: repeated option `{flag}`\n{USAGE}");
                return ExitCode::FAILURE;
            }
            required = true;
            continue;
        }
        let Some(value) = arguments.next()
        else {
            eprintln!("obligations: `{flag}` takes a value\n{USAGE}");
            return ExitCode::FAILURE;
        };
        if !OBLIGATION_OPTIONS.contains(&flag.as_str())
            || options.insert(flag.clone(), value).is_some()
        {
            eprintln!("obligations: unsupported or repeated option `{flag}`\n{USAGE}");
            return ExitCode::FAILURE;
        }
    }
    let (Some(manifest_path), Some(base), Some(records), Some(rustdoc)) = (
        options.get("--manifest-path"),
        options.get("--base"),
        options.get("--records"),
        options.get("--rustdoc"),
    )
    else {
        eprintln!(
            "obligations: `--manifest-path`, `--base`, `--records` and `--rustdoc` are required\n{USAGE}"
        );
        return ExitCode::FAILURE;
    };
    let mut texts = alloc::collections::BTreeMap::new();
    for option in ["--previous", "--body"] {
        if let Some(path) = options.get(option) {
            match std::fs::read_to_string(path) {
                Ok(text) => {
                    let _first = texts.insert(option, text);
                },
                Err(error) => {
                    eprintln!("obligations could not read {path}: {error}");
                    return ExitCode::FAILURE;
                },
            }
        }
    }
    let previous = texts
        .get("--previous")
        .or_else(|| texts.get("--body"))
        .map_or("", String::as_str);
    let body = texts
        .get("--body")
        .or_else(|| texts.get("--previous"))
        .map_or("", String::as_str);
    let labels: Vec<IssueLabel<'_>> = options
        .get("--labels")
        .map_or("", String::as_str)
        .split(',')
        .map(str::trim)
        .filter(|label| !label.is_empty())
        .map(IssueLabel)
        .collect();
    let hotpath = match options.get("--hotpath") {
        None => Maybe::Absent(hotpath::Unreported),
        Some(path) => {
            let read = std::fs::read_to_string(path)
                .map_err(|error| error.to_string())
                .and_then(|text| {
                    hotpath::read_report(DocText(&text), Path::new(path))
                        .map_err(|error| error.to_string())
                });
            match read {
                Ok(deltas) => Maybe::Present(deltas),
                Err(error) => {
                    eprintln!("obligations could not read {path}: {error}");
                    return ExitCode::FAILURE;
                },
            }
        },
    };
    let inputs = obligations::Inputs {
        manifest_path: Path::new(manifest_path),
        base: GitRevision::from(base),
        records: Path::new(records),
        rustdoc: Path::new(rustdoc),
        previous: DocText(previous),
        body: DocText(body),
        labels: &labels,
        hotpath: &hotpath,
    };
    let report = match obligations::run(&inputs) {
        Ok(report) => report,
        Err(error) => {
            eprintln!("obligations could not run: {error}");
            return ExitCode::FAILURE;
        },
    };
    print!("{}", obligations::render_text(&report));
    let mut outputs = Vec::new();
    if let Some(path) = options.get("--comment") {
        outputs.push((path, obligations::render_comment(&report)));
    }
    if let Some(path) = options.get("--selection") {
        let mut lines = obligations::selection(&report).join("\n");
        if !lines.is_empty() {
            lines.push('\n');
        }
        outputs.push((path, lines));
    }
    if let (Some(path), Some(body)) = (options.get("--body"), texts.get("--body")) {
        match obligations::render_body(DocText(body), &report) {
            Ok(text) => outputs.push((path, text)),
            Err(oversized) => {
                eprintln!(
                    "obligations: {path} would hold {} characters, past the {} a pull request \
                     accepts, even with the table of clauses left to the comment",
                    oversized.characters,
                    obligations::BODY_LIMIT
                );
                return ExitCode::FAILURE;
            },
        }
    }
    for (path, contents) in outputs {
        if let Err(error) = std::fs::write(path, contents) {
            eprintln!("obligations could not write {path}: {error}");
            return ExitCode::FAILURE;
        }
    }
    let open = usize::from(obligations::open_count(&report));
    if required && open > 0 {
        eprintln!("obligations: {open} obligations are not acknowledged:");
        for obligation in report
            .obligations
            .iter()
            .filter(|obligation| obligation.discharge == obligations::Discharge::Open)
        {
            eprintln!(
                "  {} {} ({})",
                obligation.specification, obligation.clause, obligation.key
            );
        }
        if let obligations::Assessment::Owed {
            ref written,
            standing,
        } = report.assessment
        {
            match standing {
                obligations::Standing::Unticked => {
                    eprintln!("  lint assessment: the box is unticked");
                },
                obligations::Standing::Malformed => eprintln!(
                    "  lint assessment: `{written}` is neither `could: yes — <lint>, <reference>` nor `could: no — <one sentence>`"
                ),
                obligations::Standing::Settled => {},
            }
        }
        if let hotpath::Verdict::Owed {
            ref written,
            standing,
        } = report.hotpath
        {
            match standing {
                hotpath::Standing::Unticked => {
                    eprintln!("  hotpath verdict: the box is unticked");
                },
                hotpath::Standing::Malformed => eprintln!(
                    "  hotpath verdict: `{written}` is neither `hotpath: fixed — <benchmark> <before> → <after>` nor `hotpath: accepted — <benchmark> exercises <feature>; <n> allocations per <use>; <one sentence>` over a benchmark of the report"
                ),
                hotpath::Standing::Unmoved => eprintln!(
                    "  hotpath verdict: `{written}` accepts a benchmark the report does not move"
                ),
                hotpath::Standing::Settled => {},
            }
        }
        return ExitCode::from(2_u8);
    }
    ExitCode::SUCCESS
}
