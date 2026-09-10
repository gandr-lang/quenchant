//! Repository policy checks keep refusals distinct from unavailable evidence.

pub mod action_pins;
pub mod pins;
pub mod public_boundary;
pub mod publish_allowlist;

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::ExitCode;

use quenchant_gates::GateError;
use quenchant_gates::semantic::CommandLine;
use quenchant_gates::semantic::ErrorMessage;
use quenchant_gates::semantic::SourceText;
use quenchant_shape::shape::Maybe;

/// The selected check reached its accepting verdict.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Passed;

/// Read text without turning an unavailable file into an empty observation.
///
/// # Specification
/// - ensures: returns the file's UTF-8 contents unchanged.
/// - fails: returns `GateError::Io` with the path on access or decoding
///   failure.
/// - panics: none.
///
/// # Errors
/// `GateError::Io` preserves access and decoding failures.
///
/// # Adequacy
/// - hypothesis: L3 an absent input produces an addressed operational error
///   rather than acceptance.
/// - witness: `repository::tests::unavailable_input_is_not_a_verdict`
pub fn read_text(path: &Path) -> Result<String, GateError>
{
    std::fs::read_to_string(path)
        .map_err(|error| GateError::io(path, ErrorMessage(&error.to_string())))
}

/// Run one instrument directly, without a shell or an implicit success
/// fallback.
///
/// # Specification
/// - ensures: returns UTF-8 stdout only after a successful process exit.
/// - fails: returns `GateError::Tool` for spawn, exit, or decoding failures.
/// - panics: none.
///
/// # Errors
/// `GateError::Tool` retains the invocation and the instrument's error.
///
/// # Adequacy
/// - hypothesis: L3 a failing instrument remains an operational failure even
///   when it emits stdout.
/// - witness: `repository::tests::failed_process_is_not_a_verdict`
pub fn output(command: &mut Command) -> Result<String, GateError>
{
    let identity = format!("{command:?}");
    let output = command.output().map_err(|error| {
        GateError::tool(CommandLine(&identity), ErrorMessage(&error.to_string()))
    })?;
    if !output.status.success() {
        return Err(GateError::tool(
            CommandLine(&identity),
            ErrorMessage(&String::from_utf8_lossy(&output.stderr)),
        ));
    }
    String::from_utf8(output.stdout)
        .map_err(|error| GateError::tool(CommandLine(&identity), ErrorMessage(&error.to_string())))
}

/// Parse a pin-bearing document with its original formatting retained.
///
/// # Specification
/// - ensures: returns the parsed document without changing its file.
/// - fails: returns `GateError::Io` on access failure or `GateError::Parse` on
///   invalid TOML.
/// - panics: none.
///
/// # Errors
/// Access and syntax failures retain their input path.
///
/// # Adequacy
/// - hypothesis: L3 malformed TOML cannot masquerade as absent pins.
/// - witness: `repository::tests::invalid_toml_is_not_a_verdict`
pub fn document(path: &Path) -> Result<toml_edit::DocumentMut, GateError>
{
    let text = read_text(path)?;
    text.parse().map_err(|error: toml_edit::TomlError| {
        GateError::parse(path, ErrorMessage(&error.to_string()))
    })
}

/// List tracked paths with NUL framing, including whitespace-bearing names.
///
/// # Specification
/// - ensures: each returned path is relative to the selected repository root.
/// - fails: returns `GateError::Tool` when Git cannot inventory the tree.
/// - panics: none.
///
/// # Errors
/// Git and path-decoding failures remain operational errors.
///
/// # Adequacy
/// - hypothesis: L3 a tracked path with spaces remains one address.
/// - witness: `repository::public_boundary::tests::tracked_tree_and_history_are_checked`
pub fn tracked_paths(root: &Path) -> Result<Vec<PathBuf>, GateError>
{
    let text = output(
        Command::new("git")
            .current_dir(root)
            .args(["ls-files", "-z"]),
    )?;
    Ok(text
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .collect())
}

/// Print one accepting, refusing, or operational result at the CLI boundary.
///
/// # Specification
/// - ensures: only a present acceptance witness exits successfully.
/// - provides: a refusal's exact reason or an operational diagnostic, never
///   both.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 CLI fixtures distinguish acceptance, policy refusal, and
///   unavailable evidence.
/// - witness: `gates::repository::subcommands_refuse_broken_fixtures`
#[expect(
    clippy::print_stderr,
    clippy::print_stdout,
    clippy::use_debug,
    reason = "the CLI emits the closed reason variant as structured refusal evidence"
)]
fn report<R>(
    name: SourceText<'_>,
    result: Result<Maybe<Passed, R>, GateError>,
) -> ExitCode
where
    R: core::fmt::Debug,
{
    let name = name.0;
    match result {
        | Ok(Maybe::Present(Passed)) => {
            println!("{name} OK");
            ExitCode::SUCCESS
        },
        | Ok(Maybe::Absent(reason)) => {
            eprintln!("{name} FAILED: {reason:?}");
            ExitCode::FAILURE
        },
        | Err(error) => {
            eprintln!("{name} could not run: {error}");
            ExitCode::FAILURE
        },
    }
}

/// Route repository commands while rejecting missing, repeated, and unrelated
/// options.
///
/// # Specification
/// - ensures: paths and version values select exactly the requested operation.
/// - provides: `--root` defaults to the working directory; pin inputs retain
///   their documented environment overrides.
/// - fails: invalid arguments exit unsuccessfully before any gate or edit runs.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 malformed CLI options cannot weaken the selected check or
///   silently change scope.
/// - witness: `gates::repository::subcommands_refuse_broken_fixtures`
#[expect(
    clippy::print_stderr,
    reason = "invalid CLI options require a diagnostic"
)]
pub fn dispatch(
    name: SourceText<'_>,
    mut arguments: core::iter::Skip<std::env::Args>,
) -> ExitCode
{
    let name = name.0;
    let mut options = alloc::collections::BTreeMap::new();
    while let Some(flag) = arguments.next() {
        let allowed = flag == "--root"
            || (name == "pins"
                && matches!(
                    flag.as_str(),
                    "--upstream-toolchain" | "--consumer-manifest" | "--consumer-config"
                ))
            || (name == "toolchain-bump" && flag == "--version");
        let Some(value) = arguments.next()
        else {
            eprintln!("{name}: {flag} requires a value");
            return ExitCode::FAILURE;
        };
        if !allowed || value.starts_with("--") || options.insert(flag.clone(), value).is_some() {
            eprintln!("{name}: unsupported or repeated option {flag}");
            return ExitCode::FAILURE;
        }
    }
    let root = options
        .remove("--root")
        .map_or_else(|| PathBuf::from("."), PathBuf::from);
    match name {
        | "public-boundary" => report(SourceText(name), public_boundary::check(&root)),
        | "conflict-markers" => report(SourceText(name), public_boundary::conflicts(&root)),
        | "action-pins" => report(SourceText(name), action_pins::check(&root)),
        | "publish-allowlist" => report(
            SourceText(name),
            publish_allowlist::check(&root.join("Cargo.toml")),
        ),
        | "pins" => report(SourceText(name), pins::check(&root, &options)),
        | "toolchain-bump" => match options.remove("--version") {
            | Some(version) => report(SourceText(name), pins::bump(&root, SourceText(&version))),
            | None => {
                eprintln!("toolchain-bump: --version is required");
                ExitCode::FAILURE
            },
        },
        | _ => {
            eprintln!("unknown repository command {name}");
            ExitCode::FAILURE
        },
    }
}

#[cfg(test)]
mod tests
{
    use super::*;

    /// Isolated fixture storage used only by this test process.
    #[repr(transparent)]
    pub struct Fixture(pub PathBuf);

    impl Fixture
    {
        /// Allocate a unique fixture root.
        ///
        /// # Specification
        /// trivial.
        pub fn new() -> Self
        {
            static SERIAL: core::sync::atomic::AtomicUsize =
                core::sync::atomic::AtomicUsize::new(0);
            let serial = SERIAL.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "quenchant-repository-{}-{serial}",
                std::process::id()
            ));
            std::fs::create_dir_all(&root).unwrap();
            Self(root)
        }
    }

    impl Drop for Fixture
    {
        /// Remove this fixture's owned directory.
        ///
        /// # Specification
        /// trivial.
        fn drop(&mut self)
        {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn unavailable_input_is_not_a_verdict()
    {
        let fixture = Fixture::new();
        let path = fixture.0.join("absent.toml");
        assert!(
            matches!(read_text(&path), Err(GateError::Io { path: actual, .. }) if actual == path),
            "missing input must retain its address"
        );
    }

    #[test]
    fn failed_process_is_not_a_verdict()
    {
        let result = output(Command::new("git").arg("not-a-real-command"));
        assert!(
            matches!(result, Err(GateError::Tool { .. })),
            "a failed instrument is not an observation"
        );
    }

    #[test]
    fn invalid_toml_is_not_a_verdict()
    {
        let fixture = Fixture::new();
        let path = fixture.0.join("broken.toml");
        std::fs::write(&path, "[").unwrap();
        assert!(
            matches!(document(&path), Err(GateError::Parse { path: actual, .. }) if actual == path),
            "invalid input must retain its syntax failure"
        );
    }
}
