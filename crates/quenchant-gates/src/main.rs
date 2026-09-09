//! Consumer-scoped invocation and witness verification.
//!
//! ```text
//! quenchant-gates <anodized | witnesses> --manifest-path <Cargo.toml>
//! ```
//!
//! An explicit consumer manifest selects both gates' scope, including for an
//! installed binary launched elsewhere. `anodized` classifies invocation cfgs
//! and can require panic-enabled checking; `witnesses` resolves the consumer's
//! runnable inventory.
//!
//! Policy violations and operational failures both produce unsuccessful exits
//! with distinct diagnostics. Failure to obtain an inventory remains an
//! operational failure rather than evidence that its obligations passed.

use std::path::Path;
use std::path::PathBuf;
use std::process::ExitCode;

use quenchant_gates::Finding;
use quenchant_gates::GateError;
use quenchant_gates::anodized::Requirement;
use quenchant_gates::anodized::Verdict;
use quenchant_gates::anodized::invocation_state;
use quenchant_gates::inventory;

/// Invalid argument shapes receive the complete supported command syntax.
const USAGE: &str = "usage: quenchant-gates <anodized | witnesses> --manifest-path <Cargo.toml> [--require-enforcing]";

/// Argument interpretation selects one consumer-scoped gate.
///
/// # Specification
/// - requires: the arguments name one gate, `--manifest-path`, and the
///   workspace manifest to read.
/// - ensures: runs the named gate against that manifest and exits successfully
///   exactly when the gate reaches a verdict with no findings.
/// - fails: prints the usage text and exits unsuccessfully on an unrecognized
///   invocation, and prints the gate's own error on an operational failure.
/// - panics: none.
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
    let requirement = match arguments.next() {
        | None => Requirement::Observe,
        | Some(extra) if subcommand == "anodized" && extra == "--require-enforcing" => {
            Requirement::Enforcing
        },
        | Some(extra) => {
            eprintln!("unexpected argument `{extra}`\n{USAGE}");
            return ExitCode::FAILURE;
        },
    };
    if let Some(extra) = arguments.next() {
        eprintln!("unexpected argument `{extra}`\n{USAGE}");
        return ExitCode::FAILURE;
    }
    let manifest_path = PathBuf::from(manifest_path);
    match subcommand.as_str() {
        | "anodized" => anodized_gate(&manifest_path, requirement),
        | "witnesses" => run_witnesses(&manifest_path),
        | _ => {
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
        | Ok(state) => state,
        | Err(error) => {
            eprintln!("quenchant-gates: anodized state query could not run: {error}");
            return ExitCode::FAILURE;
        },
    };
    println!("quenchant-gates: anodized invocation state: {state}");
    match state.verdict(requirement) {
        | Verdict::Pass => ExitCode::SUCCESS,
        | Verdict::Fail => {
            eprintln!(
                "quenchant-gates: anodized policy FAILED: discard is forbidden; enforcing lanes require anodized_panic"
            );
            ExitCode::FAILURE
        },
    }
}

/// Witness findings determine the consumer run's process outcome.
///
/// # Specification
/// - requires: `manifest_path` names the manifest of the workspace to gate.
/// - ensures: prints one line per unresolved or ambiguous witness and fails
///   when there is at least one, and prints the passing line and succeeds when
///   every witness resolves.
/// - provides: the exit status the gate CI job reads.
/// - fails: reports a failing status and a distinct "could not run" message
///   when the workspace cannot be discovered or its tests cannot be listed, so
///   an unmeasured run is never reported as a pass.
/// - panics: none.
#[expect(
    clippy::print_stderr,
    clippy::print_stdout,
    reason = "G0's verdict is its console report; the CI job reads the printed findings, each \
              addressed to the source line that must change"
)]
fn run_witnesses(manifest_path: &Path) -> ExitCode
{
    let findings = match inventory_gate(manifest_path) {
        | Ok(findings) => findings,
        | Err(error) => {
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
/// - requires: `manifest_path` names the manifest of the workspace to gate.
/// - ensures: every `- witness:` bullet of every member is resolved against
///   that member's own test targets, and the findings come back in a
///   deterministic order.
/// - provides: the unreported half of [`run_witnesses`], separated so an
///   operational failure stays a `Result` rather than becoming an empty
///   verdict.
/// - fails: propagates the [`GateError`] raised by discovery, listing, or
///   reading a member's sources.
/// - panics: none.
///
/// # Errors
/// [`GateError::Tool`] when `cargo metadata` or a test listing fails,
/// [`GateError::Io`] when a member's source cannot be read, and
/// [`GateError::Parse`] when one of those sources is not parseable Rust.
fn inventory_gate(manifest_path: &Path) -> Result<Vec<Finding>, GateError>
{
    let workspace = inventory::discover(manifest_path)?;
    let catalog = inventory::catalog(&workspace)?;
    inventory::run(&workspace, &catalog)
}
