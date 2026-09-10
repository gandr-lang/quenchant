//! Build a runnable inventory under the toolchain selected by each source
//! owner.
//!
//! A member-local toolchain file affects commands executed in that directory,
//! not commands that merely name its package. Such members are inventoried
//! separately and excluded from the shared listing; omitting them would hide
//! their witness obligations.
//!
//! Source roots come from declared Cargo targets, not a recursive sweep of
//! every file under a package. UI inputs and other test data are therefore not
//! mistaken for authored package declarations. Build-script roots do not pull
//! those data trees back into the scan.
//!
//! Tool availability, successful inventory output, and a finding-free
//! resolution are distinct states. Failed or malformed tool output cannot
//! become an empty successful catalog.

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
use crate::semantic::PackageId;
use crate::semantic::PackageName;
use crate::semantic::PinsToolchain;
use crate::semantic::SourceText;
use crate::semantic::TargetKind;
use crate::semantic::TargetLabel;
use crate::semantic::TargetName;
use crate::semantic::TestAlias;

/// Package ownership and source scope for one discovered workspace member.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Member
{
    /// Package identity attached to findings and test listings.
    pub name: String,
    /// Manifest-relative operations use this member directory.
    pub directory: PathBuf,
    /// Cargo-declared target directories bound the authored-source scan.
    ///
    /// Target source directories exclude separately compiled fixture corpora
    /// such as Dylint UI inputs. Build scripts contribute no root: their
    /// package-level directory would admit those fixture trees again.
    pub source_roots: Vec<PathBuf>,
    /// A member-local compiler pin requires its own listing invocation.
    pub pins_toolchain: PinsToolchain,
}

/// Discovered workspace scope, including deterministic member order.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Workspace
{
    /// Root used when a listing can share the workspace invocation.
    pub root: PathBuf,
    /// Name ordering makes member traversal deterministic.
    pub members: Vec<Member>,
}

/// Cargo metadata establishes package ownership before source or witness
/// inspection.
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
    let manifest_path = std::fs::canonicalize(manifest_path).map_err(|error| {
        GateError::tool(
            "cargo metadata".into(),
            ErrorMessage::from(&error.to_string()),
        )
    })?;
    let Some(scope) = manifest_path.parent()
    else {
        return Err(GateError::tool(
            "cargo metadata".into(),
            "manifest has no parent directory".into(),
        ));
    };
    let runner = ListingRunner::CargoTest {
        toolchain: active_toolchain(scope)?,
    };
    let mut command = runner_command(&runner);
    command.current_dir(scope);
    command.args(["metadata", "--no-deps", "--format-version", "1"]);
    command.arg("--manifest-path");
    command.arg(&manifest_path);
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

/// Target kind determines whether its directory belongs to the authored-source
/// scan.
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

/// Runnable inventory combines shared-workspace and member-pinned compiler
/// scopes.
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
    let mut catalog = TestCatalog::new();
    let pinned: Vec<&Member> = workspace
        .members
        .iter()
        .filter(|member| member.pins_toolchain.0)
        .collect();

    if pinned.len() != workspace.members.len() {
        let runner = listing_runner(&workspace.root)?;
        let mut command = list_command(&runner);
        command.current_dir(&workspace.root);
        command.arg("--workspace");
        for member in &pinned {
            command.arg("--exclude");
            command.arg(&member.name);
        }
        merge_listing(&mut catalog, &mut command, &workspace.root, &runner)?;
    }

    for member in pinned {
        let runner = listing_runner(&member.directory)?;
        let mut command = list_command(&runner);
        command.current_dir(&member.directory);
        merge_listing(&mut catalog, &mut command, &member.directory, &runner)?;
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

/// Validate each member's authored witness references against the runnable
/// catalog.
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

/// Overlapping target roots contribute each member source file only once.
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

/// Rust source traversal excludes build-output subtrees.
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

/// The selected test-listing instrument and its source-owned compiler context.
enum ListingRunner
{
    /// Mise resolves the executable; Rustup owns its compiler environment.
    Nextest
    {
        /// Absolute executable retained from the consumer's mise selection.
        executable: PathBuf,
        /// Source-resolved Rustup toolchain, independent of the launch
        /// directory.
        toolchain: String,
    },
    /// Without mise, the ordinary Cargo inventory remains the supported route.
    CargoTest
    {
        /// The fallback retains the same source-owned compiler selection.
        toolchain: String,
    },
}

/// Select the consumer's runner once per compiler scope, never through Cargo's
/// plugin search.
///
/// # Specification
/// - ensures: a successful mise selection is one absolute executable path,
///   probed through Rustup under the scope's own active compiler selection.
/// - ensures: missing mise selects the ordinary Cargo inventory; failed mise
///   selection or selected-runner execution never invokes a global nextest.
/// - fails: [`GateError::Tool`] on malformed selection, unavailable selected
///   compiler or runner, or an unsuccessful selection/probe.
/// - panics: none.
///
/// # Errors
/// Returns the failed selection or probe as an addressed operational error.
///
/// # Adequacy
/// - hypothesis: L3 — an actual consumer inventory resolves its own selected
///   runner despite an unusable launch selection and a poisoned `CARGO_HOME`
///   plugin; invalid source remains an operational error from that selected
///   runner.
/// - witness: `gates::repository::witness_inventory_uses_consumer_selection_over_shadowed_plugins`
fn listing_runner(scope: &Path) -> Result<ListingRunner, GateError>
{
    let toolchain = active_toolchain(scope)?;
    let selection = Command::new("mise")
        .args(["which", "cargo-nextest"])
        .current_dir(scope)
        .env_remove("RUSTUP_TOOLCHAIN")
        .output();
    let selection = match selection {
        | Ok(selection) => selection,
        #[expect(
            clippy::std_instead_of_core,
            reason = "core::io is unstable on the supported compiler; process spawn errors use std"
        )]
        | Err(error) => {
            if error.kind() == std::io::ErrorKind::NotFound {
                return Ok(ListingRunner::CargoTest { toolchain });
            }
            return Err(GateError::tool(
                "mise which cargo-nextest".into(),
                ErrorMessage::from(&error.to_string()),
            ));
        },
    };
    if !selection.status.success() {
        return Err(GateError::tool(
            "mise which cargo-nextest".into(),
            ErrorMessage::from(String::from_utf8_lossy(&selection.stderr).as_ref()),
        ));
    }
    let selected = String::from_utf8(selection.stdout).map_err(|error| {
        GateError::tool(
            "mise which cargo-nextest".into(),
            ErrorMessage::from(&error.to_string()),
        )
    })?;
    let selected = selected.trim();
    let executable = PathBuf::from(selected);
    if !executable.is_absolute() || selected.lines().count() != 1_usize {
        return Err(GateError::tool(
            "mise which cargo-nextest".into(),
            "selection must be exactly one absolute executable path".into(),
        ));
    }
    let runner = ListingRunner::Nextest {
        executable,
        toolchain,
    };
    let mut probe = runner_command(&runner);
    probe.current_dir(scope).args(["nextest", "--version"]);
    let label = format!("{probe:?}");
    capture(&mut probe, CommandLine::from(&label))?;
    Ok(runner)
}

/// Resolve the compiler from the supplied source scope instead of inherited
/// producer state.
///
/// # Specification
/// - ensures: Rustup's active source selection supplies the compiler identity;
///   an inherited `RUSTUP_TOOLCHAIN` cannot override the source's pin.
/// - fails: [`GateError::Tool`] when Rustup cannot resolve a nonempty
///   selection.
/// - panics: none.
///
/// # Errors
/// Returns a failed or empty Rustup selection as an operational error.
///
/// # Adequacy
/// - hypothesis: L3 — the consumer fixture has a supported source pin while its
///   caller supplies an unavailable compiler and Cargo path; inventory
///   succeeds.
/// - witness: `gates::repository::witness_inventory_uses_consumer_selection_over_shadowed_plugins`
fn active_toolchain(scope: &Path) -> Result<String, GateError>
{
    let active = capture(
        Command::new("rustup")
            .args(["show", "active-toolchain"])
            .env_remove("RUSTUP_TOOLCHAIN")
            .current_dir(scope),
        "rustup show active-toolchain".into(),
    )?;
    let Some(toolchain) = active.split_whitespace().next()
    else {
        return Err(GateError::tool(
            "rustup show active-toolchain".into(),
            "no active compiler was selected".into(),
        ));
    };
    Ok(toolchain.to_owned())
}

/// Reuse the selected executable under the complete Rustup proxy environment.
///
/// # Specification
/// - ensures: nextest uses the retained executable and compiler; an inherited
///   producer `CARGO` path cannot redirect the consumer's compiler invocation.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the actual selected-runner fixture compiles and
///   inventories under the consumer compiler despite an unrelated launch
///   compiler.
/// - witness: `gates::repository::witness_inventory_uses_consumer_selection_over_shadowed_plugins`
fn runner_command(runner: &ListingRunner) -> Command
{
    match *runner {
        | ListingRunner::Nextest { ref executable, .. } => {
            context_command(runner, executable.as_os_str())
        },
        | ListingRunner::CargoTest { .. } => context_command(runner, OsStr::new("cargo")),
    }
}

/// Apply the retained Rustup context to a selected tool or a built native test
/// binary.
///
/// # Specification
/// - ensures: every subprocess uses the retained compiler context, including
///   dynamic-library search paths; producer `CARGO` does not redirect it.
/// - panics: none.
fn context_command(
    runner: &ListingRunner,
    executable: &OsStr,
) -> Command
{
    let (ListingRunner::Nextest { ref toolchain, .. } | ListingRunner::CargoTest { ref toolchain }) =
        *runner;
    let mut command = Command::new("rustup");
    command
        .args(["run", toolchain])
        .arg(executable)
        .env_remove("CARGO");
    command
}

/// Listing arguments preserve the selected instrument's output format.
///
/// # Specification
/// - ensures: the selected nextest supplies aggregate JSON; the explicit
///   ordinary Cargo route supplies executable artifacts for native listing.
/// - panics: none.
fn list_command(runner: &ListingRunner) -> Command
{
    let mut command = runner_command(runner);
    match *runner {
        | ListingRunner::Nextest { .. } => {
            command.args(["nextest", "list", "--message-format", "json"]);
        },
        | ListingRunner::CargoTest { .. } => {
            command.args(["test", "--no-run", "--message-format", "json"]);
        },
    }
    command
}

/// A scoped listing contributes aliases only after its command and format
/// succeed.
///
/// # Specification
/// - requires: `command` was built by [`list_command`] for the same instrument
///   `runner` names, and `scope` is the directory it runs in.
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
    runner: &ListingRunner,
) -> Result<(), GateError>
{
    let label = format!("{command:?} (in {})", scope.display());
    let stdout = capture(command, CommandLine::from(&label))?;
    if matches!(runner, ListingRunner::Nextest { .. }) {
        let listed = TestCatalog::from_nextest_json(SourceText::from(&stdout))?;
        catalog.absorb(&listed);
        return Ok(());
    }
    merge_cargo_test_listing(
        catalog,
        SourceText::from(&stdout),
        CommandLine::from(&label),
        scope,
        runner,
    )
}

/// Native harness listings retain the package and target identity of built test
/// executables.
///
/// # Specification
/// - requires: `stdout` is the JSON-lines output of `cargo test --no-run
///   --message-format json`.
/// - ensures: every built test binary is run with `--list --format terse` and
///   its tests recorded under the binary's own package and target. Native
///   listings retain the source compiler's Rustup execution context.
/// - provides: the ordinary Cargo inventory when mise is unavailable.
/// - fails: [`GateError::Tool`] when a binary cannot be listed.
/// - panics: none.
///
/// # Errors
/// [`GateError::Tool`] when a built binary cannot be listed.
fn merge_cargo_test_listing(
    catalog: &mut TestCatalog,
    stdout: SourceText<'_>,
    label: CommandLine<'_>,
    scope: &Path,
    runner: &ListingRunner,
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
        let mut command = context_command(runner, OsStr::new(executable));
        command.current_dir(scope);
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

/// Cargo package identifiers carry the ownership name used by the witness
/// catalog.
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

/// Process output is available only after successful execution of the
/// identified command.
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
