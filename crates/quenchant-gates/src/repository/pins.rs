//! Compiler-coupled pins are read and changed as one named boundary.

use alloc::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use quenchant_gates::GateError;
use quenchant_gates::semantic::ErrorMessage;
use quenchant_gates::semantic::SourceText;
use quenchant_shape::shape::Maybe;

use super::Passed;

/// The meaning of each required pin, independent of TOML table presentation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Pin
{
    /// Rustup's selected compiler channel.
    Channel,
    /// Stable rust-clippy release tag.
    ClippyTag,
    /// Dylint library runtime version.
    Linting,
    /// Dylint fixture harness version.
    Testing,
    /// Installed Dylint driver version.
    Driver,
    /// Installed Dylint linker version.
    Linker,
    /// Consumer's installed gate revision.
    ConsumerBinary,
}

quenchant_shape::reason_enum! {
    /// Evidence that the compiler-coupled pin set is not coherent.
    pub mod refusal {
        /// Closed pin refusal vocabulary.
        #[derive(Clone, Debug, Eq, PartialEq)]
        pub enum Refused {
            /// A required nonempty string pin is absent or has the wrong type.
            Missing(super::Pin),
            /// A stable release or full revision has an invalid spelling.
            Invalid(String),
            /// Two pins that must agree differ.
            Drift {
                /// The boundary that disagrees.
                pin: super::Pin,
                /// The authoritative value.
                expected: String,
                /// The measured value.
                actual: String,
            },
            /// Consumer inputs are paired rather than independently optional.
            ConsumerPair,
            /// No unique matching compiler-plugin source was declared.
            ConsumerLibrary,
        }
    }
}

/// Read one required pin without erasing why its value is unavailable.
///
/// # Specification
/// - ensures: returns the selected nonempty string; inline and expanded tables
///   agree.
/// - provides: `refusal::Refused::Missing` names an absent, empty, or
///   non-string pin.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 pin presence, type, and TOML-layout boundaries distinguish
///   empty or wrong-table lookups.
/// - witness: `repository::pins::tests::pin_presence_and_drift`
fn pin(
    document: &toml_edit::DocumentMut,
    name: Pin,
) -> Maybe<SourceText<'_>, refusal::Refused>
{
    let keys: &[&str] = match name {
        | Pin::Channel => &["toolchain", "channel"],
        | Pin::ClippyTag => &["workspace", "dependencies", "clippy_utils", "tag"],
        | Pin::Linting => &["workspace", "dependencies", "dylint_linting", "version"],
        | Pin::Testing => &["workspace", "dependencies", "dylint_testing", "version"],
        | Pin::Driver => &["tools", "cargo:cargo-dylint"],
        | Pin::Linker => &["tools", "cargo:dylint-link"],
        | Pin::ConsumerBinary => &["env", "QUENCHANT_REV"],
    };
    let mut item = document.as_item();
    for key in keys {
        let Some(value) = item.get(*key)
        else {
            return Maybe::Absent(refusal::Refused::Missing(name));
        };
        item = value;
    }
    let value = item
        .as_str()
        .or_else(|| item.get("version").and_then(toml_edit::Item::as_str));
    match value {
        | Some(value) if !value.is_empty() => Maybe::Present(SourceText(value)),
        | _ => Maybe::Absent(refusal::Refused::Missing(name)),
    }
}

/// Validate a stable rust-clippy tag before it can enter a remote request or
/// edit.
///
/// # Specification
/// - ensures: accepts exactly rust- followed by three nonempty ASCII decimal
///   components.
/// - provides: `refusal::Refused::Invalid` retains an invalid tag.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 missing prefix, empty/extra component, and nondecimal cases
///   distinguish permissive tag guards.
/// - witness: `repository::pins::tests::stable_tag_boundaries`
fn stable_tag(tag: SourceText<'_>) -> Maybe<Passed, refusal::Refused>
{
    let valid = tag.0.strip_prefix("rust-").is_some_and(|release| {
        let mut count = 0_usize;
        let valid = release.split('.').all(|part| {
            count = count.saturating_add(1);
            !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit())
        });
        valid && count == 3
    });
    if valid {
        Maybe::Present(Passed)
    }
    else {
        Maybe::Absent(refusal::Refused::Invalid(tag.0.into()))
    }
}

/// Fetch the selected rust-clippy release's own compiler declaration.
///
/// # Specification
/// - requires: the caller has validated the stable release tag.
/// - ensures: returns the remote toolchain document for that exact tag.
/// - fails: transport, authentication, and TOML failures remain operational
///   errors.
/// - panics: none.
///
/// # Errors
/// `GateError::Tool` and `GateError::Parse` retain failed upstream evidence.
///
/// # Adequacy
/// - hypothesis: the remote instrument remains outside the offline L3
///   comparison witnesses; the live pin gate exercises the selected release
///   query.
/// - witness: `repository::pins::tests::pin_presence_and_drift`
fn upstream(tag: SourceText<'_>) -> Result<toml_edit::DocumentMut, GateError>
{
    let endpoint = format!(
        "repos/rust-lang/rust-clippy/contents/rust-toolchain.toml?ref={}",
        tag.0
    );
    let text = super::output(Command::new("gh").args([
        "api",
        &endpoint,
        "--header",
        "Accept: application/vnd.github.raw+json",
    ]))?;
    text.parse().map_err(|error: toml_edit::TomlError| {
        GateError::parse(
            Path::new("rust-clippy toolchain"),
            ErrorMessage(&error.to_string()),
        )
    })
}

/// Compare the local toolchain and Dylint versions with the upstream compiler.
///
/// # Specification
/// - ensures: all required pins exist, the tag is stable, the channel matches
///   upstream, and all four Dylint versions agree.
/// - provides: sealed missing, invalid, and drift reasons identify the failing
///   pin.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 independently drifted pins and missing values distinguish
///   weakened comparisons and vacuous success.
/// - witness: `repository::pins::tests::pin_presence_and_drift`
fn compare(
    manifest: &toml_edit::DocumentMut,
    tools: &toml_edit::DocumentMut,
    toolchain: &toml_edit::DocumentMut,
    remote: &toml_edit::DocumentMut,
) -> Maybe<Passed, refusal::Refused>
{
    let Maybe::Present(tag) = pin(manifest, Pin::ClippyTag)
    else {
        return Maybe::Absent(refusal::Refused::Missing(Pin::ClippyTag));
    };
    if let Maybe::Absent(reason) = stable_tag(tag) {
        return Maybe::Absent(reason);
    }
    let Maybe::Present(channel) = pin(toolchain, Pin::Channel)
    else {
        return Maybe::Absent(refusal::Refused::Missing(Pin::Channel));
    };
    let Maybe::Present(expected) = pin(remote, Pin::Channel)
    else {
        return Maybe::Absent(refusal::Refused::Missing(Pin::Channel));
    };
    if channel != expected {
        return Maybe::Absent(refusal::Refused::Drift {
            pin: Pin::Channel,
            expected: expected.0.into(),
            actual: channel.0.into(),
        });
    }
    let Maybe::Present(expected) = pin(manifest, Pin::Linting)
    else {
        return Maybe::Absent(refusal::Refused::Missing(Pin::Linting));
    };
    for (document, name) in [
        (manifest, Pin::Testing),
        (tools, Pin::Driver),
        (tools, Pin::Linker),
    ] {
        let Maybe::Present(actual) = pin(document, name)
        else {
            return Maybe::Absent(refusal::Refused::Missing(name));
        };
        if expected != actual {
            return Maybe::Absent(refusal::Refused::Drift {
                pin: name,
                expected: expected.0.into(),
                actual: actual.0.into(),
            });
        }
    }
    Maybe::Present(Passed)
}

/// Require the consumer's library and installed binary to select one full
/// revision.
///
/// # Specification
/// - ensures: one matching plugin declaration and its binary both name the same
///   lowercase 40-hex commit.
/// - provides: `ConsumerLibrary`, `Missing`, `Invalid`, or `Drift` retain the
///   rejected boundary.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 absent, ambiguous, short, uppercase, and unequal revision
///   cases distinguish selection and equality defects.
/// - witness: `repository::pins::tests::consumer_revision_boundaries`
fn consumer(
    manifest: &toml_edit::DocumentMut,
    tools: &toml_edit::DocumentMut,
) -> Maybe<Passed, refusal::Refused>
{
    let libraries = manifest
        .get("workspace")
        .and_then(|item| item.get("metadata"))
        .and_then(|item| item.get("dylint"))
        .and_then(|item| item.get("libraries"))
        .and_then(toml_edit::Item::as_array_of_tables);
    let Some(libraries) = libraries
    else {
        return Maybe::Absent(refusal::Refused::ConsumerLibrary);
    };
    let mut revisions = libraries
        .iter()
        .filter(|table| {
            table
                .get("git")
                .and_then(toml_edit::Item::as_str)
                .is_some_and(|git| {
                    git.trim_end_matches(".git").trim_end_matches('/')
                        == "https://github.com/silvanshade/quenchant"
                })
                && table.get("pattern").and_then(toml_edit::Item::as_str)
                    == Some("crates/quenchant-dylints")
        })
        .filter_map(|table| table.get("rev").and_then(toml_edit::Item::as_str));
    let Some(library) = revisions.next()
    else {
        return Maybe::Absent(refusal::Refused::ConsumerLibrary);
    };
    if revisions.next().is_some() {
        return Maybe::Absent(refusal::Refused::ConsumerLibrary);
    }
    let Maybe::Present(binary) = pin(tools, Pin::ConsumerBinary)
    else {
        return Maybe::Absent(refusal::Refused::Missing(Pin::ConsumerBinary));
    };
    for revision in [library, binary.0] {
        if revision.len() != 40
            || !revision
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a' ..= b'f').contains(&byte))
        {
            return Maybe::Absent(refusal::Refused::Invalid(revision.into()));
        }
    }
    if library != binary.0 {
        return Maybe::Absent(refusal::Refused::Drift {
            pin: Pin::ConsumerBinary,
            expected: library.into(),
            actual: binary.0.into(),
        });
    }
    Maybe::Present(Passed)
}

/// Resolve one legacy pin-input override relative to the selected repository.
///
/// # Specification
/// trivial.
fn input(
    root: &Path,
    variable: SourceText<'_>,
    default: &Path,
) -> PathBuf
{
    root.join(std::env::var_os(variable.0).map_or_else(|| default.to_path_buf(), PathBuf::from))
}

/// Check compiler pins and the optional paired consumer revision boundary.
///
/// # Specification
/// - ensures: local pins agree with the exact upstream tag and optional
///   consumer inputs agree with each other.
/// - provides: sealed pin refusals; `--upstream-toolchain` supplies an explicit
///   offline evidence document.
/// - fails: input access, parsing, and upstream process failures return
///   `GateError`.
/// - panics: none.
///
/// # Errors
/// `GateError::Io`, `GateError::Parse`, and `GateError::Tool` preserve
/// unavailable evidence.
///
/// # Adequacy
/// - hypothesis: L3 deliberately drifted fixture files reach the same
///   comparison used by the live command.
/// - witness: `gates::repository::subcommands_refuse_broken_fixtures`
pub fn check(
    root: &Path,
    options: &BTreeMap<String, String>,
) -> Result<Maybe<Passed, refusal::Refused>, GateError>
{
    let manifest = super::document(&input(
        root,
        SourceText("WORKSPACE_MANIFEST"),
        Path::new("Cargo.toml"),
    ))?;
    let tools = super::document(&input(
        root,
        SourceText("TOOL_CONFIG"),
        Path::new("mise.toml"),
    ))?;
    let toolchain = super::document(&input(
        root,
        SourceText("TOOLCHAIN_FILE"),
        Path::new("rust-toolchain.toml"),
    ))?;
    let Maybe::Present(tag) = pin(&manifest, Pin::ClippyTag)
    else {
        return Ok(Maybe::Absent(refusal::Refused::Missing(Pin::ClippyTag)));
    };
    if let Maybe::Absent(reason) = stable_tag(tag) {
        return Ok(Maybe::Absent(reason));
    }
    let remote = match options.get("--upstream-toolchain") {
        | Some(path) => super::document(&root.join(path))?,
        | None => upstream(tag)?,
    };
    if let Maybe::Absent(reason) = compare(&manifest, &tools, &toolchain, &remote) {
        return Ok(Maybe::Absent(reason));
    }
    match (
        options.get("--consumer-manifest"),
        options.get("--consumer-config"),
    ) {
        | (None, None) => Ok(Maybe::Present(Passed)),
        | (Some(manifest), Some(tools)) => {
            let manifest = super::document(&root.join(manifest))?;
            let tools = super::document(&root.join(tools))?;
            Ok(consumer(&manifest, &tools))
        },
        | _ => Ok(Maybe::Absent(refusal::Refused::ConsumerPair)),
    }
}

/// Replace only the selected compiler channel and compiler-utilities tag.
///
/// # Specification
/// - ensures: both replacements preserve unrelated TOML fields and comments.
/// - provides: a missing pin or non-nightly upstream channel refuses the edit.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 unrelated tag fields and comments survive an exact pin
///   update; absent keys refuse before mutation.
/// - witness: `repository::pins::tests::bump_preserves_unrelated_configuration`
fn replace(
    manifest: &mut toml_edit::DocumentMut,
    toolchain: &mut toml_edit::DocumentMut,
    tag: SourceText<'_>,
    channel: SourceText<'_>,
) -> Maybe<Passed, refusal::Refused>
{
    if let Maybe::Absent(reason) = stable_tag(tag) {
        return Maybe::Absent(reason);
    }
    if !channel.0.starts_with("nightly-") {
        return Maybe::Absent(refusal::Refused::Invalid(channel.0.into()));
    }
    let tag_item = manifest
        .get_mut("workspace")
        .and_then(|item| item.get_mut("dependencies"))
        .and_then(|item| item.get_mut("clippy_utils"))
        .and_then(|item| item.get_mut("tag"))
        .and_then(toml_edit::Item::as_value_mut);
    let Some(tag_item) = tag_item
    else {
        return Maybe::Absent(refusal::Refused::Missing(Pin::ClippyTag));
    };
    let channel_item = toolchain
        .get_mut("toolchain")
        .and_then(|item| item.get_mut("channel"))
        .and_then(toml_edit::Item::as_value_mut);
    let Some(channel_item) = channel_item
    else {
        return Maybe::Absent(refusal::Refused::Missing(Pin::Channel));
    };
    let mut replacement = toml_edit::Value::from(tag.0);
    *replacement.decor_mut() = tag_item.decor().clone();
    *tag_item = replacement;
    let mut replacement = toml_edit::Value::from(channel.0);
    *replacement.decor_mut() = channel_item.decor().clone();
    *channel_item = replacement;
    Maybe::Present(Passed)
}

/// Update the compiler-coupled pair after obtaining and validating upstream
/// evidence.
///
/// # Specification
/// - ensures: successful completion writes only the compiler channel and
///   `clippy_utils` tag values.
/// - provides: a sealed pin refusal before writing when the release or selected
///   fields are invalid.
/// - fails: upstream, parsing, or write failures return `GateError`; a failed
///   second write restores the first file or reports both failures.
/// - panics: none.
///
/// # Errors
/// Instrument, parse, and addressed I/O errors prevent a false completion
/// claim.
///
/// # Adequacy
/// - hypothesis: L3 exact editor witnesses distinguish unrelated tag
///   replacement and missing-key mutation; filesystem write failures remain an
///   operational boundary.
/// - witness: `repository::pins::tests::bump_preserves_unrelated_configuration`
pub fn bump(
    root: &Path,
    version: SourceText<'_>,
) -> Result<Maybe<Passed, refusal::Refused>, GateError>
{
    let tag = format!("rust-{}", version.0);
    if let Maybe::Absent(reason) = stable_tag(SourceText(&tag)) {
        return Ok(Maybe::Absent(reason));
    }
    let remote = upstream(SourceText(&tag))?;
    let Maybe::Present(channel) = pin(&remote, Pin::Channel)
    else {
        return Ok(Maybe::Absent(refusal::Refused::Missing(Pin::Channel)));
    };
    let manifest_path = root.join("Cargo.toml");
    let toolchain_path = root.join("rust-toolchain.toml");
    let original = super::read_text(&manifest_path)?;
    let mut manifest = super::document(&manifest_path)?;
    let mut toolchain = super::document(&toolchain_path)?;
    if let Maybe::Absent(reason) = replace(&mut manifest, &mut toolchain, SourceText(&tag), channel)
    {
        return Ok(Maybe::Absent(reason));
    }
    std::fs::write(&manifest_path, manifest.to_string())
        .map_err(|error| GateError::io(&manifest_path, ErrorMessage(&error.to_string())))?;
    if let Err(error) = std::fs::write(&toolchain_path, toolchain.to_string()) {
        let message = match std::fs::write(&manifest_path, original) {
            | Ok(()) => error.to_string(),
            | Err(restore) => format!("{error}; restoring Cargo.toml also failed: {restore}"),
        };
        return Err(GateError::io(&toolchain_path, ErrorMessage(&message)));
    }
    Ok(Maybe::Present(Passed))
}

#[cfg(test)]
mod tests
{
    use super::*;

    /// Independent compiler-coupled fixture values.
    const MANIFEST: &str = r#"
[workspace.dependencies]
clippy_utils = { tag = "rust-1.98.0" }
dylint_linting = { version = "6.0.4" }
dylint_testing = { version = "6.0.4" }
"#;
    /// Both supported mise tool-table forms participate in one equality check.
    const TOOLS: &str = r#"
[tools]
"cargo:dylint-link" = "6.0.4"
[tools."cargo:cargo-dylint"]
version = "6.0.4"
"#;
    /// Independent matching upstream and local channel fixture.
    const TOOLCHAIN: &str = r#"
[toolchain]
channel = "nightly-2026-07-30"
"#;

    #[test]
    fn pin_presence_and_drift()
    {
        let manifest: toml_edit::DocumentMut = MANIFEST.parse().unwrap();
        let tools: toml_edit::DocumentMut = TOOLS.parse().unwrap();
        let toolchain: toml_edit::DocumentMut = TOOLCHAIN.parse().unwrap();
        assert_eq!(
            compare(&manifest, &tools, &toolchain, &toolchain),
            Maybe::Present(Passed)
        );
        for (text, name) in [
            (
                MANIFEST.replace("tag = \"rust-1.98.0\"", "tag = 1"),
                Pin::ClippyTag,
            ),
            (
                MANIFEST.replace("dylint_linting = { version = \"6.0.4\" }", ""),
                Pin::Linting,
            ),
            (
                MANIFEST.replace("dylint_testing = { version = \"6.0.4\" }", ""),
                Pin::Testing,
            ),
        ] {
            assert_eq!(
                compare(&text.parse().unwrap(), &tools, &toolchain, &toolchain),
                Maybe::Absent(refusal::Refused::Missing(name))
            );
        }
        for (name, source, key) in [
            (
                Pin::Testing,
                MANIFEST,
                "dylint_testing = { version = \"6.0.4\" }",
            ),
            (Pin::Driver, TOOLS, "version = \"6.0.4\""),
            (Pin::Linker, TOOLS, "\"cargo:dylint-link\" = \"6.0.4\""),
        ] {
            let drifted: toml_edit::DocumentMut = source
                .replace(key, &key.replace("6.0.4", "6.0.3"))
                .parse()
                .unwrap();
            let result = if name == Pin::Testing {
                compare(&drifted, &tools, &toolchain, &toolchain)
            }
            else {
                compare(&manifest, &drifted, &toolchain, &toolchain)
            };
            assert_eq!(
                result,
                Maybe::Absent(refusal::Refused::Drift {
                    pin: name,
                    expected: "6.0.4".into(),
                    actual: "6.0.3".into()
                })
            );
        }
        let drifted = TOOLCHAIN
            .replace("2026-07-30", "2026-07-29")
            .parse()
            .unwrap();
        assert_eq!(
            compare(&manifest, &tools, &drifted, &toolchain),
            Maybe::Absent(refusal::Refused::Drift {
                pin: Pin::Channel,
                expected: "nightly-2026-07-30".into(),
                actual: "nightly-2026-07-29".into()
            })
        );
    }

    #[test]
    fn stable_tag_boundaries()
    {
        for tag in [
            "1.98.0",
            "rust-1.98",
            "rust-1.98.0.1",
            "rust-1..0",
            "rust-1.98.x",
            "rust-1.98.0/path",
        ] {
            assert_eq!(
                stable_tag(SourceText(tag)),
                Maybe::Absent(refusal::Refused::Invalid(tag.into()))
            );
        }
        assert_eq!(
            stable_tag(SourceText("rust-1.98.0")),
            Maybe::Present(Passed)
        );
    }

    #[test]
    fn consumer_revision_boundaries()
    {
        let revision = "a".repeat(40);
        let manifest = format!(
            r#"
[[workspace.metadata.dylint.libraries]]
git = "https://github.com/silvanshade/quenchant"
pattern = "crates/quenchant-dylints"
rev = "{revision}"
"#
        );
        let tools = format!(
            r#"[env]
QUENCHANT_REV = "{revision}"
"#
        );
        assert_eq!(
            consumer(&manifest.parse().unwrap(), &tools.parse().unwrap()),
            Maybe::Present(Passed)
        );
        for unrelated in [
            manifest.replace("silvanshade/quenchant", "example/unrelated"),
            manifest.replace("crates/quenchant-dylints", "crates/unrelated"),
        ] {
            assert_eq!(
                consumer(&unrelated.parse().unwrap(), &tools.parse().unwrap()),
                Maybe::Absent(refusal::Refused::ConsumerLibrary)
            );
        }
        for revision in ["a".repeat(39), "A".repeat(40)] {
            let invalid = tools.replace(&"a".repeat(40), &revision);
            assert_eq!(
                consumer(&manifest.parse().unwrap(), &invalid.parse().unwrap()),
                Maybe::Absent(refusal::Refused::Invalid(revision))
            );
        }
        let drifted = tools.replace(&revision, &"b".repeat(40));
        assert_eq!(
            consumer(&manifest.parse().unwrap(), &drifted.parse().unwrap()),
            Maybe::Absent(refusal::Refused::Drift {
                pin: Pin::ConsumerBinary,
                expected: revision,
                actual: "b".repeat(40)
            })
        );
        assert_eq!(
            consumer(&toml_edit::DocumentMut::new(), &tools.parse().unwrap()),
            Maybe::Absent(refusal::Refused::ConsumerLibrary)
        );
        assert_eq!(
            consumer(
                &manifest.repeat(2).parse().unwrap(),
                &tools.parse().unwrap()
            ),
            Maybe::Absent(refusal::Refused::ConsumerLibrary)
        );
    }

    #[test]
    fn bump_preserves_unrelated_configuration()
    {
        let mut manifest: toml_edit::DocumentMut = format!(
            r#"{MANIFEST}
[other]
tag = "keep" # unrelated
"#
        )
        .parse()
        .unwrap();
        let mut toolchain: toml_edit::DocumentMut =
            format!("{TOOLCHAIN}# keep components\ncomponents = [\"miri\"]\n")
                .parse()
                .unwrap();
        assert_eq!(
            replace(
                &mut manifest,
                &mut toolchain,
                SourceText("rust-1.99.0"),
                SourceText("nightly-2026-08-01")
            ),
            Maybe::Present(Passed)
        );
        assert_eq!(
            pin(&manifest, Pin::ClippyTag),
            Maybe::Present(SourceText("rust-1.99.0"))
        );
        assert_eq!(
            pin(&toolchain, Pin::Channel),
            Maybe::Present(SourceText("nightly-2026-08-01"))
        );
        assert_eq!(
            manifest.get("other").unwrap().get("tag").unwrap().as_str(),
            Some("keep")
        );
        assert!(
            manifest.to_string().contains("# unrelated"),
            "unrelated comments must remain"
        );
        assert!(
            toolchain.to_string().contains("# keep components"),
            "component comments must remain"
        );
        let before = manifest.to_string();
        assert_eq!(
            replace(
                &mut manifest,
                &mut toml_edit::DocumentMut::new(),
                SourceText("rust-1.99.0"),
                SourceText("nightly-2026-08-01")
            ),
            Maybe::Absent(refusal::Refused::Missing(Pin::Channel))
        );
        assert_eq!(manifest.to_string(), before);
    }
}
