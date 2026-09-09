//! Discovering the workspace, listing its tests, and running G0 over it.
//!
//! # Why the listing is not one command
//!
//! A workspace member may pin its own toolchain; a `rust-toolchain` file
//! governs the directory a command runs in, not the package a command names,
//! so such a member is listed from its own directory and excluded from the
//! workspace-wide listing. No member does so today — the gate library builds
//! under the workspace's single pinned toolchain — so the listing is the
//! single workspace-wide run. The alternative is to drop a pinning member
//! from the gate, which is the same as declaring its witnesses exempt.

use std::ffi::OsStr;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use crate::Finding;
use crate::GateError;
use crate::catalog::TestCatalog;
use crate::catalog::alias_for;
use crate::catalog::is_integration;
use crate::semantic::CommandLine;
use crate::semantic::ErrorMessage;
use crate::semantic::HoldsPackageSource;
use crate::semantic::NextestAvailable;
use crate::semantic::PackageId;
use crate::semantic::PackageName;
use crate::semantic::PinsToolchain;
use crate::semantic::SourceText;
use crate::semantic::TargetKind;
use crate::semantic::TargetLabel;
use crate::semantic::TargetName;
use crate::semantic::TestAlias;

/// One workspace member the gate reads.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Member
{
    /// The Cargo package name.
    pub name: String,
    /// The directory holding the member's manifest.
    pub directory: PathBuf,
    /// The directories holding the member's declared target sources.
    ///
    /// Derived from the targets Cargo declares rather than from the package
    /// directory, so a fixture tree that is compiled by a test harness rather
    /// than by the package — the dylint UI corpus — is data, not source. A
    /// build script contributes none: its source sits in the member's own
    /// directory, and expanding it would sweep those data trees back in.
    pub source_roots: Vec<PathBuf>,
    /// Whether the member pins its own toolchain.
    pub pins_toolchain: PinsToolchain,
}

/// The workspace the gate runs over.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Workspace
{
    /// The workspace root directory.
    pub root: PathBuf,
    /// Every workspace member, sorted by name.
    pub members: Vec<Member>,
}

/// Read the workspace layout from `cargo metadata`.
///
/// # Specification
/// - requires: `manifest_path` names the workspace's root `Cargo.toml`.
/// - ensures: returns every workspace member with its directory, sorted by
///   name, each flagged with whether it pins its own toolchain.
/// - provides: the package ownership map every finding is attributed through.
/// - fails: [`GateError::Tool`] when `cargo metadata` fails or its output is
///   unreadable.
/// - panics: none.
///
/// # Errors
/// [`GateError::Tool`] when `cargo metadata` fails or cannot be read.
#[inline]
pub fn discover(manifest_path: &Path) -> Result<Workspace, GateError>
{
    let mut command = Command::new(cargo());
    command.args(["metadata", "--no-deps", "--format-version", "1"]);
    command.arg("--manifest-path");
    command.arg(manifest_path);
    let stdout = capture(&mut command, "cargo metadata".into())?;
    let document: serde_json::Value = serde_json::from_str(&stdout).map_err(|error| {
        GateError::tool(
            "cargo metadata".into(),
            ErrorMessage::from(&error.to_string()),
        )
    })?;
    let Some(root) = document
        .get("workspace_root")
        .and_then(serde_json::Value::as_str)
    else {
        return Err(GateError::tool(
            "cargo metadata".into(),
            "output carries no `workspace_root`".into(),
        ));
    };
    let Some(packages) = document
        .get("packages")
        .and_then(serde_json::Value::as_array)
    else {
        return Err(GateError::tool(
            "cargo metadata".into(),
            "output carries no `packages` array".into(),
        ));
    };
    let mut members = Vec::new();
    for package in packages {
        let Some(name) = package.get("name").and_then(serde_json::Value::as_str)
        else {
            continue;
        };
        let Some(manifest) = package
            .get("manifest_path")
            .and_then(serde_json::Value::as_str)
        else {
            continue;
        };
        let directory = Path::new(manifest)
            .parent()
            .map_or_else(|| PathBuf::from(manifest), Path::to_path_buf);
        let pins_toolchain = PinsToolchain(
            directory.join("rust-toolchain").exists()
                || directory.join("rust-toolchain.toml").exists(),
        );
        let mut source_roots: Vec<PathBuf> = package
            .get("targets")
            .and_then(serde_json::Value::as_array)
            .map(|targets| {
                targets
                    .iter()
                    .filter(|target| target_holds_package_source(target).0)
                    .filter_map(|target| target.get("src_path").and_then(serde_json::Value::as_str))
                    .filter_map(|source| Path::new(source).parent().map(Path::to_path_buf))
                    .collect()
            })
            .unwrap_or_default();
        source_roots.sort();
        source_roots.dedup();
        members.push(Member {
            name: name.to_owned(),
            directory,
            source_roots,
            pins_toolchain,
        });
    }
    members.sort();
    Ok(Workspace {
        root: PathBuf::from(root),
        members,
    })
}

/// Whether one target of the `cargo metadata` array holds the package's own
/// documented source.
///
/// # Specification
/// - requires: `target` is one entry of a package's `targets` array.
/// - ensures: every declared kind is admitted unless it is the one kind whose
///   source sits in the member's own directory, the build script; a target
///   declaring no kind at all is admitted.
/// - provides: the answer [`discover`] filters on, through the
///   [`HoldsPackageSource`] wrapper, before expanding a target into a source
///   root.
/// - panics: none.
fn target_holds_package_source(target: &serde_json::Value) -> HoldsPackageSource
{
    HoldsPackageSource(
        target
            .get("kind")
            .and_then(serde_json::Value::as_array)
            .is_none_or(|kinds| {
                kinds
                    .iter()
                    .all(|kind| kind.as_str() != Some("custom-build"))
            }),
    )
}

/// List every runnable test in the workspace.
///
/// # Specification
/// - requires: `workspace` was produced by [`discover`], and the workspace
///   builds under the toolchains its members select.
/// - ensures: every member's tests are listed, members pinning their own
///   toolchain from their own directory and the rest in one workspace-wide run;
///   an inventory that found no test at all is refused rather than returned,
///   since it would pass every witness.
/// - provides: the resolution table for the whole workspace.
/// - fails: [`GateError::Tool`] when a listing command fails or is unreadable.
/// - panics: none.
///
/// # Errors
/// [`GateError::Tool`] when a listing command fails, or lists nothing.
#[inline]
pub fn catalog(workspace: &Workspace) -> Result<TestCatalog, GateError>
{
    let nextest = nextest_available();
    let mut catalog = TestCatalog::new();
    let pinned: Vec<&Member> = workspace
        .members
        .iter()
        .filter(|member| member.pins_toolchain.0)
        .collect();

    let mut command = list_command(nextest);
    command.current_dir(&workspace.root);
    command.arg("--workspace");
    for member in &pinned {
        command.arg("--exclude");
        command.arg(&member.name);
    }
    merge_listing(&mut catalog, &mut command, &workspace.root, nextest)?;

    for member in pinned {
        let mut command = list_command(nextest);
        command.current_dir(&member.directory);
        merge_listing(&mut catalog, &mut command, &member.directory, nextest)?;
    }

    if usize::from(catalog.alias_count()) == 0_usize {
        return Err(GateError::tool(
            "cargo test listing".into(),
            "the workspace listed no tests at all; a gate that measured nothing must not report a \
             pass"
                .into(),
        ));
    }
    Ok(catalog)
}

/// Run G0 over every source file of every workspace member.
///
/// # Specification
/// - requires: `catalog` lists the same workspace `workspace` describes.
/// - ensures: returns every unresolved and ambiguous witness in the workspace,
///   in a deterministic order, each addressed to the file and line of the
///   bullet that must change.
/// - provides: the G0 verdict.
/// - fails: [`GateError::Io`] on an unreadable file, [`GateError::Parse`] on a
///   file that is not parseable Rust.
/// - panics: none.
///
/// # Errors
/// [`GateError::Io`] or [`GateError::Parse`] when a member's sources cannot be
/// read.
#[inline]
pub fn run(
    workspace: &Workspace,
    catalog: &TestCatalog,
) -> Result<Vec<Finding>, GateError>
{
    let mut findings = Vec::new();
    for member in &workspace.members {
        let package = PackageName::from(&member.name);
        for path in member_sources(member)? {
            let source = std::fs::read_to_string(&path)
                .map_err(|error| GateError::io(&path, ErrorMessage::from(&error.to_string())))?;
            let claims = crate::witnesses::witness_claims(&path, SourceText::from(&source))?;
            findings.extend(crate::witnesses::resolve(package, &claims, catalog));
        }
    }
    findings.sort();
    Ok(findings)
}

/// Every source file of one member, deduplicated across its target roots.
///
/// # Specification
/// - requires: `member` was produced by [`discover`].
/// - ensures: returns every `.rs` file beneath the member's declared target
///   source directories, once each, in sorted order.
/// - provides: the file set G0 reads for one crate.
/// - fails: [`GateError::Io`] when a directory cannot be listed.
/// - panics: none.
///
/// # Errors
/// [`GateError::Io`] when a directory cannot be listed.
fn member_sources(member: &Member) -> Result<Vec<PathBuf>, GateError>
{
    let mut files = Vec::new();
    for root in &member.source_roots {
        if !root.is_dir() {
            continue;
        }
        files.extend(source_files(root)?);
    }
    files.sort();
    files.dedup();
    Ok(files)
}

/// Every `.rs` file under a directory, excluding build output.
///
/// # Specification
/// - requires: `directory` exists.
/// - ensures: returns every `.rs` file beneath it in sorted order, skipping
///   `target` directories and anything hidden.
/// - provides: the file set G0 reads.
/// - fails: [`GateError::Io`] when a directory cannot be listed.
/// - panics: none.
///
/// # Errors
/// [`GateError::Io`] when a directory cannot be listed.
///
/// # Termination
/// - reason: no recursion; the walk is a loop over an explicit worklist.
/// - measure: the number of directories still pending.
/// - boundedness: every pop pushes only strict subdirectories of the popped
///   directory, and a directory tree is finite.
/// - input recursion: none.
#[inline]
pub fn source_files(directory: &Path) -> Result<Vec<PathBuf>, GateError>
{
    let mut files = Vec::new();
    let mut pending = vec![directory.to_path_buf()];
    while let Some(current) = pending.pop() {
        let entries = std::fs::read_dir(&current)
            .map_err(|error| GateError::io(&current, ErrorMessage::from(&error.to_string())))?;
        for entry in entries {
            let entry = entry
                .map_err(|error| GateError::io(&current, ErrorMessage::from(&error.to_string())))?;
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') || name == "target" {
                continue;
            }
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            if path.extension() == Some(OsStr::new("rs")) {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}

/// Whether `cargo nextest` is on the path.
///
/// # Specification
/// - ensures: answers affirmatively exactly when `cargo nextest --version` runs
///   and exits successfully; a Cargo that cannot be started answers negatively
///   rather than failing.
/// - panics: none.
fn nextest_available() -> NextestAvailable
{
    NextestAvailable(
        Command::new(cargo())
            .args(["nextest", "--version"])
            .output()
            .is_ok_and(|output| output.status.success()),
    )
}

/// Build the listing command for the selected instrument.
///
/// # Specification
/// - ensures: returns `cargo nextest list` in JSON when nextest is available,
///   and the `cargo test --no-run` listing otherwise.
/// - panics: none.
fn list_command(nextest: NextestAvailable) -> Command
{
    let mut command = Command::new(cargo());
    if nextest.0 {
        command.args(["nextest", "list", "--message-format", "json"]);
    }
    else {
        command.args(["test", "--no-run", "--message-format", "json"]);
    }
    command
}

/// Run one listing command and merge its result into `catalog`.
///
/// # Specification
/// - requires: `command` was built by [`list_command`] for the same instrument
///   `nextest` names, and `scope` is the directory it runs in.
/// - ensures: every test the command lists is recorded under its own package
///   and target, merged into whatever `catalog` already held.
/// - provides: the one step [`catalog`] repeats per toolchain scope.
/// - fails: [`GateError::Tool`] when the command cannot be run, exits
///   unsuccessfully, or produces a listing the reader does not support.
/// - panics: none.
///
/// # Errors
/// [`GateError::Tool`], carrying the scope in its command line so a failure
/// says which of the per-toolchain listings it came from.
fn merge_listing(
    catalog: &mut TestCatalog,
    command: &mut Command,
    scope: &Path,
    nextest: NextestAvailable,
) -> Result<(), GateError>
{
    let label = format!(
        "{} (in {})",
        if nextest.0 {
            "cargo nextest list"
        }
        else {
            "cargo test --no-run"
        },
        scope.display()
    );
    let stdout = capture(command, CommandLine::from(&label))?;
    if nextest.0 {
        let listed = TestCatalog::from_nextest_json(SourceText::from(&stdout))?;
        catalog.absorb(&listed);
        return Ok(());
    }
    merge_cargo_test_listing(
        catalog,
        SourceText::from(&stdout),
        CommandLine::from(&label),
    )
}

/// Merge the fallback listing: built test binaries, each asked for its tests.
///
/// # Specification
/// - requires: `stdout` is the JSON-lines output of `cargo test --no-run
///   --message-format json`.
/// - ensures: every built test binary is run with `--list --format terse` and
///   its tests recorded under the binary's own package and target.
/// - provides: the inventory on a machine without cargo-nextest.
/// - fails: [`GateError::Tool`] when a binary cannot be listed.
/// - panics: none.
///
/// # Errors
/// [`GateError::Tool`] when a built binary cannot be listed.
fn merge_cargo_test_listing(
    catalog: &mut TestCatalog,
    stdout: SourceText<'_>,
    label: CommandLine<'_>,
) -> Result<(), GateError>
{
    for line in stdout.0.lines() {
        let Ok(record) = serde_json::from_str::<serde_json::Value>(line)
        else {
            continue;
        };
        if record.get("reason").and_then(serde_json::Value::as_str) != Some("compiler-artifact") {
            continue;
        }
        if record
            .get("profile")
            .and_then(|profile| profile.get("test"))
            != Some(&serde_json::Value::Bool(true))
        {
            continue;
        }
        let Some(executable) = record.get("executable").and_then(serde_json::Value::as_str)
        else {
            continue;
        };
        let Some(target) = record.get("target")
        else {
            continue;
        };
        let Some(name) = target.get("name").and_then(serde_json::Value::as_str)
        else {
            continue;
        };
        let kind = target
            .get("kind")
            .and_then(serde_json::Value::as_array)
            .and_then(|kinds| kinds.first())
            .and_then(serde_json::Value::as_str)
            .unwrap_or("lib");
        let Some(package) = record
            .get("package_id")
            .and_then(serde_json::Value::as_str)
            .map(|id| package_name_of(PackageId::from(id)))
        else {
            continue;
        };
        let mut command = Command::new(executable);
        command.args(["--list", "--format", "terse"]);
        let listing = capture(&mut command, label)?;
        let label_text = format!("{package}::{name}");
        for entry in listing.lines() {
            let Some(test) = entry.strip_suffix(": test")
            else {
                continue;
            };
            let alias = alias_for(
                is_integration(TargetKind::from(kind)),
                TargetName::from(name),
                TestAlias::from(test),
            );
            catalog.insert(
                PackageName::from(&package),
                TestAlias::from(&alias),
                TargetLabel::from(&label_text),
            );
        }
    }
    Ok(())
}

/// The package name inside a Cargo package identifier.
///
/// # Specification
/// - requires: `id` is a package identifier as `cargo` spells it in machine
///   output, in either the modern `<source>#<name>@<version>` form or the
///   legacy `<name> <version> (<source>)` one.
/// - ensures: returns the bare package name for both forms; the modern form is
///   read first, and its result is refused when the fragment still carries a
///   path separator, which is how a `<source>#<version>` identifier — one that
///   omits the name because it repeats the source's last segment — falls
///   through to the legacy reader instead of yielding a directory.
/// - provides: the package a fallback-listed test binary is attributed to.
/// - panics: none.
fn package_name_of(id: PackageId<'_>) -> String
{
    let id = id.0;
    if let Some((_, rest)) = id.rsplit_once('#') {
        let name = rest.split_once('@').map_or(rest, |(name, _)| name);
        if !name.contains('/') {
            return name.to_owned();
        }
    }
    id.split_whitespace()
        .next()
        .unwrap_or(id)
        .rsplit('/')
        .next()
        .unwrap_or(id)
        .to_owned()
}

/// Run a command and return its standard output, or the reason it failed.
///
/// # Specification
/// - requires: `label` spells the command line as a reader should see it, since
///   the command itself is not recoverable from a failure.
/// - ensures: returns standard output, lossily decoded, exactly when the
///   process started and exited successfully.
/// - provides: the single place every subprocess in this module is run, so an
///   unsuccessful exit can never be read as empty output.
/// - fails: [`GateError::Tool`] when the process cannot start, carrying the
///   spawn error, and when it exits unsuccessfully, carrying its standard
///   error.
/// - panics: none.
///
/// # Errors
/// [`GateError::Tool`] on a failed spawn or an unsuccessful exit.
fn capture(
    command: &mut Command,
    label: CommandLine<'_>,
) -> Result<String, GateError>
{
    let output = command
        .output()
        .map_err(|error| GateError::tool(label, ErrorMessage::from(&error.to_string())))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        return Err(GateError::tool(label, ErrorMessage::from(&stderr)));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// The cargo executable to invoke.
///
/// # Specification
/// - ensures: returns the `CARGO` the current invocation was started under, and
///   `cargo` when the variable is unset or not text.
/// - panics: none.
fn cargo() -> String
{
    std::env::var("CARGO").unwrap_or_else(|_| String::from("cargo"))
}
