//! Compiler-coupled pins are read and changed as one named boundary.

use alloc::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use anodized::spec;
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
    /// Full rust-clippy commit, for a nightly that no release tag names.
    ClippyRev,
    /// Version that `clippy_utils` publishes at the selected rust-clippy
    /// source.
    ClippyVersion,
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
            /// `clippy_utils` names both a release tag and a commit.
            ClippySource,
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
#[spec(ensures: |ref output| {
    let keys: &[&str] = match name {
        Pin::Channel => &["toolchain", "channel"],
        Pin::ClippyTag => &["workspace", "dependencies", "clippy_utils", "tag"],
        Pin::ClippyRev => &["workspace", "dependencies", "clippy_utils", "rev"],
        Pin::ClippyVersion => &["package", "version"],
        Pin::Linting => &["workspace", "dependencies", "dylint_linting", "version"],
        Pin::Testing => &["workspace", "dependencies", "dylint_testing", "version"],
        Pin::Driver => &["tools", "cargo:cargo-dylint"],
        Pin::Linker => &["tools", "cargo:dylint-link"],
        Pin::ConsumerBinary => &["env", "QUENCHANT_REV"],
    };
    let expected = keys.iter().try_fold(document.as_item(), |item, key| item.get(*key)).and_then(|item| item.as_str().or_else(|| item.get("version").and_then(toml_edit::Item::as_str))).filter(|value| !value.is_empty());
    match *output {
        Maybe::Present(value) => expected == Some(value.0),
        Maybe::Absent(refusal::Refused::Missing(missing)) => expected.is_none() && missing == name,
        Maybe::Absent(_) => false,
    }
})]
fn pin(
    document: &toml_edit::DocumentMut,
    name: Pin,
) -> Maybe<SourceText<'_>, refusal::Refused>
{
    let keys: &[&str] = match name {
        Pin::Channel => &["toolchain", "channel"],
        Pin::ClippyTag => &["workspace", "dependencies", "clippy_utils", "tag"],
        Pin::ClippyRev => &["workspace", "dependencies", "clippy_utils", "rev"],
        Pin::ClippyVersion => &["package", "version"],
        Pin::Linting => &["workspace", "dependencies", "dylint_linting", "version"],
        Pin::Testing => &["workspace", "dependencies", "dylint_testing", "version"],
        Pin::Driver => &["tools", "cargo:cargo-dylint"],
        Pin::Linker => &["tools", "cargo:dylint-link"],
        Pin::ConsumerBinary => &["env", "QUENCHANT_REV"],
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
        Some(value) if !value.is_empty() => Maybe::Present(SourceText(value)),
        _ => Maybe::Absent(refusal::Refused::Missing(name)),
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
#[spec(ensures: |ref output| matches!(*output, Maybe::Present(_)) == tag.0.strip_prefix("rust-").is_some_and(|release| release.split('.').count() == 3 && release.split('.').all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))))]
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

/// Validate a full rust-clippy commit before it can enter a remote request or
/// edit.
///
/// # Specification
/// - ensures: accepts exactly 40 bytes, each an ASCII digit or a lowercase
///   letter from `a` to `f`.
/// - provides: `refusal::Refused::Invalid` retains an invalid commit.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 short, uppercase, and nonhexadecimal commits distinguish
///   permissive length and alphabet guards.
/// - witness: `repository::pins::tests::consumer_revision_boundaries`
/// - witness: `repository::pins::tests::clippy_reference_boundaries`
#[spec(ensures: |ref output| matches!(*output, Maybe::Present(_)) == (revision.0.len() == 40 && revision.0.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))))]
fn full_revision(revision: SourceText<'_>) -> Maybe<Passed, refusal::Refused>
{
    if revision.0.len() == 40
        && revision
            .0
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a' ..= b'f').contains(&byte))
    {
        Maybe::Present(Passed)
    }
    else {
        Maybe::Absent(refusal::Refused::Invalid(revision.0.into()))
    }
}

/// The rust-clippy source that `clippy_utils` and the compiler channel follow.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Reference<'text>
{
    /// Stable release tag, `rust-<version>`.
    Tag(SourceText<'text>),
    /// Full commit, for a nightly that no release tag names.
    Rev(SourceText<'text>),
}

impl<'text> Reference<'text>
{
    /// The tag or the commit as remote reference text.
    ///
    /// # Specification
    /// trivial.
    const fn text(self) -> SourceText<'text>
    {
        match self {
            Self::Tag(text) | Self::Rev(text) => text,
        }
    }

    /// Refuse a malformed tag or commit before it can enter a remote request or
    /// edit.
    ///
    /// # Specification
    /// - ensures: a tag passes exactly when `stable_tag` passes it, and a
    ///   commit exactly when `full_revision` passes it.
    /// - panics: none.
    /// - executable: none — the outcome is the selected validator's, which a
    ///   predicate could only call again.
    ///
    /// # Adequacy
    /// - hypothesis: L3 a valid tag and a valid commit pass, and a malformed
    ///   one of each is refused.
    /// - witness: `repository::pins::tests::clippy_reference_boundaries`
    fn valid(self) -> Maybe<Passed, refusal::Refused>
    {
        match self {
            Self::Tag(tag) => stable_tag(tag),
            Self::Rev(revision) => full_revision(revision),
        }
    }
}

/// Read the one rust-clippy source that `clippy_utils` names.
///
/// # Specification
/// - ensures: returns the tag when only a tag is pinned, or the commit when
///   only a commit is pinned, once its spelling passes.
/// - provides: `Missing(ClippyTag)` when neither is pinned, `ClippySource` when
///   both are, and `Invalid` for a malformed tag or commit.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 tag-only, commit-only, both, neither, and malformed sources
///   distinguish a reader that takes the wrong key or skips the spelling check.
/// - witness: `repository::pins::tests::clippy_reference_boundaries`
#[spec(ensures: |ref output| match (pin(manifest, Pin::ClippyTag), pin(manifest, Pin::ClippyRev)) {
    (Maybe::Present(_), Maybe::Present(_)) => *output == Maybe::Absent(refusal::Refused::ClippySource),
    (Maybe::Absent(_), Maybe::Absent(_)) => *output == Maybe::Absent(refusal::Refused::Missing(Pin::ClippyTag)),
    (Maybe::Present(tag), Maybe::Absent(_)) => matches!(*output, Maybe::Present(Reference::Tag(found)) if found == tag) == matches!(stable_tag(tag), Maybe::Present(_)),
    (Maybe::Absent(_), Maybe::Present(revision)) => matches!(*output, Maybe::Present(Reference::Rev(found)) if found == revision) == matches!(full_revision(revision), Maybe::Present(_)),
})]
fn clippy_reference(manifest: &toml_edit::DocumentMut) -> Maybe<Reference<'_>, refusal::Refused>
{
    let reference = match (pin(manifest, Pin::ClippyTag), pin(manifest, Pin::ClippyRev)) {
        (Maybe::Present(_), Maybe::Present(_)) => {
            return Maybe::Absent(refusal::Refused::ClippySource);
        },
        (Maybe::Present(tag), Maybe::Absent(_)) => Reference::Tag(tag),
        (Maybe::Absent(_), Maybe::Present(revision)) => Reference::Rev(revision),
        (Maybe::Absent(_), Maybe::Absent(_)) => {
            return Maybe::Absent(refusal::Refused::Missing(Pin::ClippyTag));
        },
    };
    match reference.valid() {
        Maybe::Present(Passed) => Maybe::Present(reference),
        Maybe::Absent(reason) => Maybe::Absent(reason),
    }
}

/// Fetch one file of the selected rust-clippy source.
///
/// # Specification
/// - requires: the caller has validated the tag or the commit.
/// - ensures: returns the remote TOML document of `file` at that exact
///   reference.
/// - fails: transport, authentication, and TOML failures remain operational
///   errors.
/// - panics: none.
///
/// # Errors
/// `GateError::Tool` and `GateError::Parse` retain failed upstream evidence.
///
/// # Adequacy
/// - hypothesis: the remote instrument remains outside the offline L3
///   comparison witnesses; the live pin gate exercises the selected reference's
///   query.
/// - witness: `repository::pins::tests::pin_presence_and_drift`
#[spec(requires: matches!(reference.valid(), Maybe::Present(_)))]
fn upstream(
    reference: Reference<'_>,
    file: SourceText<'_>,
) -> Result<toml_edit::DocumentMut, GateError>
{
    let endpoint = format!(
        "repos/rust-lang/rust-clippy/contents/{}?ref={}",
        file.0,
        reference.text().0
    );
    let text = super::output(Command::new("gh").args([
        "api",
        &endpoint,
        "--header",
        "Accept: application/vnd.github.raw+json",
    ]))?;
    let label = format!("rust-clippy {}", file.0);
    text.parse().map_err(|error: toml_edit::TomlError| {
        GateError::parse(Path::new(&label), ErrorMessage(&error.to_string()))
    })
}

/// Compare the local toolchain and Dylint versions with the upstream compiler.
///
/// # Specification
/// - ensures: all required pins exist, the rust-clippy source is one stable tag
///   or one full commit, the channel matches upstream, and all four Dylint
///   versions agree.
/// - provides: sealed missing, invalid, and drift reasons identify the failing
///   pin.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 independently drifted pins and missing values distinguish
///   weakened comparisons and vacuous success.
/// - witness: `repository::pins::tests::pin_presence_and_drift`
#[spec(ensures: |ref output| matches!(*output, Maybe::Present(_)) == match (clippy_reference(manifest), pin(toolchain, Pin::Channel), pin(remote, Pin::Channel), pin(manifest, Pin::Linting)) {
    (Maybe::Present(_), Maybe::Present(channel), Maybe::Present(expected), Maybe::Present(linting)) => channel == expected && [(manifest, Pin::Testing), (tools, Pin::Driver), (tools, Pin::Linker)].iter().all(|&(document, name)| pin(document, name) == Maybe::Present(linting)),
    _ => false,
})]
fn compare(
    manifest: &toml_edit::DocumentMut,
    tools: &toml_edit::DocumentMut,
    toolchain: &toml_edit::DocumentMut,
    remote: &toml_edit::DocumentMut,
) -> Maybe<Passed, refusal::Refused>
{
    if let Maybe::Absent(reason) = clippy_reference(manifest) {
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
#[spec(ensures: |ref output| matches!(*output, Maybe::Present(_)) == {
    let libraries = manifest.get("workspace").and_then(|item| item.get("metadata")).and_then(|item| item.get("dylint")).and_then(|item| item.get("libraries")).and_then(toml_edit::Item::as_array_of_tables);
    libraries.is_some_and(|libraries| {
        let mut revisions = libraries.iter().filter(|table| table.get("git").and_then(toml_edit::Item::as_str).is_some_and(|git| git.trim_end_matches(".git").trim_end_matches('/') == "https://github.com/gandr-lang/quenchant") && table.get("pattern").and_then(toml_edit::Item::as_str) == Some("crates/quenchant-dylints")).filter_map(|table| table.get("rev").and_then(toml_edit::Item::as_str));
        revisions.next().is_some_and(|revision| revision.len() == 40 && revision.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)) && pin(tools, Pin::ConsumerBinary) == Maybe::Present(SourceText(revision))) && revisions.next().is_none()
    })
})]
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
                        == "https://github.com/gandr-lang/quenchant"
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
        if let Maybe::Absent(reason) = full_revision(SourceText(revision)) {
            return Maybe::Absent(reason);
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
/// - ensures: local pins agree with the exact upstream tag or commit and
///   optional consumer inputs agree with each other.
/// - provides: sealed pin refusals; `--upstream-toolchain` supplies an explicit
///   offline evidence document.
/// - fails: input access, parsing, and upstream process failures return
///   `GateError`.
/// - ensures: a drift refusal retains unequal expected and actual values.
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
#[spec(ensures: |ref output| !matches!(output, &Ok(Maybe::Absent(refusal::Refused::Drift { ref expected, ref actual, .. })) if expected == actual))]
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
    let reference = match clippy_reference(&manifest) {
        Maybe::Present(reference) => reference,
        Maybe::Absent(reason) => return Ok(Maybe::Absent(reason)),
    };
    let remote = match options.get("--upstream-toolchain") {
        Some(path) => super::document(&root.join(path))?,
        None => upstream(reference, SourceText("rust-toolchain.toml"))?,
    };
    if let Maybe::Absent(reason) = compare(&manifest, &tools, &toolchain, &remote) {
        return Ok(Maybe::Absent(reason));
    }
    match (
        options.get("--consumer-manifest"),
        options.get("--consumer-config"),
    ) {
        (None, None) => Ok(Maybe::Present(Passed)),
        (Some(manifest), Some(tools)) => {
            let manifest = super::document(&root.join(manifest))?;
            let tools = super::document(&root.join(tools))?;
            Ok(consumer(&manifest, &tools))
        },
        _ => Ok(Maybe::Absent(refusal::Refused::ConsumerPair)),
    }
}

/// Replace only the selected compiler channel and the rust-clippy source, with
/// the exact `clippy_utils` version requirement that source publishes.
///
/// # Specification
/// - ensures: the replacements preserve unrelated TOML fields and comments.
/// - ensures: the source's key, `tag` or `rev`, holds the reference, and the
///   other key is gone.
/// - ensures: a declared `clippy_utils` version requirement becomes
///   `=<version>`, the version the source publishes.
/// - ensures: refusal leaves both input documents unchanged.
/// - provides: a missing pin, an invalid reference or version, or a non-nightly
///   upstream channel refuses the edit.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 unrelated tag fields and comments survive an exact pin
///   update; a tag moves to a commit, and the version requirement follows the
///   source; absent keys refuse before mutation.
/// - witness: `repository::pins::tests::bump_preserves_unrelated_configuration`
#[spec(
    captures: [before_manifest = manifest.to_string(), before_toolchain = toolchain.to_string()],
    ensures: |ref output| match *output {
        Maybe::Present(_) => clippy_reference(manifest) == Maybe::Present(reference) && pin(toolchain, Pin::Channel) == Maybe::Present(channel),
        Maybe::Absent(_) => manifest.to_string() == before_manifest && toolchain.to_string() == before_toolchain,
    },
)]
fn replace(
    manifest: &mut toml_edit::DocumentMut,
    toolchain: &mut toml_edit::DocumentMut,
    reference: Reference<'_>,
    channel: SourceText<'_>,
    version: SourceText<'_>,
) -> Maybe<Passed, refusal::Refused>
{
    if let Maybe::Absent(reason) = reference.valid() {
        return Maybe::Absent(reason);
    }
    if !channel.0.starts_with("nightly-") {
        return Maybe::Absent(refusal::Refused::Invalid(channel.0.into()));
    }
    if version.0.is_empty()
        || !version
            .0
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'.')
    {
        return Maybe::Absent(refusal::Refused::Invalid(version.0.into()));
    }
    let (key, other) = match reference {
        Reference::Tag(_) => ("tag", "rev"),
        Reference::Rev(_) => ("rev", "tag"),
    };
    let source = manifest
        .get_mut("workspace")
        .and_then(|item| item.get_mut("dependencies"))
        .and_then(|item| item.get_mut("clippy_utils"))
        .and_then(toml_edit::Item::as_table_like_mut);
    let Some(source) = source
    else {
        return Maybe::Absent(refusal::Refused::Missing(Pin::ClippyTag));
    };
    // The new pin keeps the decoration of the tag or commit it replaces.
    let Some(decor) = source
        .get(key)
        .or_else(|| source.get(other))
        .and_then(toml_edit::Item::as_value)
        .map(|value| value.decor().clone())
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
    let mut replacement = toml_edit::Value::from(channel.0);
    *replacement.decor_mut() = channel_item.decor().clone();
    *channel_item = replacement;
    source.remove(other);
    let mut replacement = toml_edit::Value::from(reference.text().0);
    *replacement.decor_mut() = decor;
    source.insert(key, toml_edit::Item::Value(replacement));
    if let Some(version_item) = source
        .get_mut("version")
        .and_then(toml_edit::Item::as_value_mut)
    {
        let mut replacement = toml_edit::Value::from(format!("={}", version.0));
        *replacement.decor_mut() = version_item.decor().clone();
        *version_item = replacement;
    }
    Maybe::Present(Passed)
}

/// Update the compiler-coupled pair after obtaining and validating upstream
/// evidence.
///
/// # Specification
/// - ensures: successful completion writes only the compiler channel, the
///   rust-clippy source and the `clippy_utils` version requirement.
/// - provides: a sealed pin refusal before writing when the reference, the
///   upstream evidence or the selected fields are invalid.
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
#[spec(ensures: |ref output| match reference.valid() {
    Maybe::Present(_) => true,
    Maybe::Absent(ref reason) => matches!(output, &Ok(Maybe::Absent(ref actual)) if actual == reason),
})]
pub fn bump(
    root: &Path,
    reference: Reference<'_>,
) -> Result<Maybe<Passed, refusal::Refused>, GateError>
{
    if let Maybe::Absent(reason) = reference.valid() {
        return Ok(Maybe::Absent(reason));
    }
    let remote = upstream(reference, SourceText("rust-toolchain.toml"))?;
    let Maybe::Present(channel) = pin(&remote, Pin::Channel)
    else {
        return Ok(Maybe::Absent(refusal::Refused::Missing(Pin::Channel)));
    };
    let utilities = upstream(reference, SourceText("clippy_utils/Cargo.toml"))?;
    let Maybe::Present(version) = pin(&utilities, Pin::ClippyVersion)
    else {
        return Ok(Maybe::Absent(refusal::Refused::Missing(Pin::ClippyVersion)));
    };
    let manifest_path = root.join("Cargo.toml");
    let toolchain_path = root.join("rust-toolchain.toml");
    let original = super::read_text(&manifest_path)?;
    let mut manifest = super::document(&manifest_path)?;
    let mut toolchain = super::document(&toolchain_path)?;
    if let Maybe::Absent(reason) =
        replace(&mut manifest, &mut toolchain, reference, channel, version)
    {
        return Ok(Maybe::Absent(reason));
    }
    std::fs::write(&manifest_path, manifest.to_string())
        .map_err(|error| GateError::io(&manifest_path, ErrorMessage(&error.to_string())))?;
    if let Err(error) = std::fs::write(&toolchain_path, toolchain.to_string()) {
        let message = match std::fs::write(&manifest_path, original) {
            Ok(()) => error.to_string(),
            Err(restore) => format!("{error}; restoring Cargo.toml also failed: {restore}"),
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
    fn clippy_reference_boundaries()
    {
        let revision = "a".repeat(40);
        let tools: toml_edit::DocumentMut = TOOLS.parse().unwrap();
        let toolchain: toml_edit::DocumentMut = TOOLCHAIN.parse().unwrap();
        let tagged: toml_edit::DocumentMut = MANIFEST.parse().unwrap();
        assert_eq!(
            clippy_reference(&tagged),
            Maybe::Present(Reference::Tag(SourceText("rust-1.98.0")))
        );
        let committed: toml_edit::DocumentMut = MANIFEST
            .replace("tag = \"rust-1.98.0\"", &format!("rev = \"{revision}\""))
            .parse()
            .unwrap();
        assert_eq!(
            clippy_reference(&committed),
            Maybe::Present(Reference::Rev(SourceText(&revision)))
        );
        assert_eq!(
            compare(&committed, &tools, &toolchain, &toolchain),
            Maybe::Present(Passed)
        );
        let both: toml_edit::DocumentMut = MANIFEST
            .replace(
                "tag = \"rust-1.98.0\"",
                &format!("tag = \"rust-1.98.0\", rev = \"{revision}\""),
            )
            .parse()
            .unwrap();
        assert_eq!(
            clippy_reference(&both),
            Maybe::Absent(refusal::Refused::ClippySource)
        );
        let neither: toml_edit::DocumentMut = MANIFEST
            .replace(
                "tag = \"rust-1.98.0\"",
                "git = \"https://github.com/rust-lang/rust-clippy\"",
            )
            .parse()
            .unwrap();
        assert_eq!(
            clippy_reference(&neither),
            Maybe::Absent(refusal::Refused::Missing(Pin::ClippyTag))
        );
        for malformed in ["a".repeat(39), "A".repeat(40), "g".repeat(40)] {
            let invalid: toml_edit::DocumentMut = MANIFEST
                .replace("tag = \"rust-1.98.0\"", &format!("rev = \"{malformed}\""))
                .parse()
                .unwrap();
            assert_eq!(
                clippy_reference(&invalid),
                Maybe::Absent(refusal::Refused::Invalid(malformed))
            );
        }
        let untagged: toml_edit::DocumentMut =
            MANIFEST.replace("rust-1.98.0", "1.98.0").parse().unwrap();
        assert_eq!(
            clippy_reference(&untagged),
            Maybe::Absent(refusal::Refused::Invalid("1.98.0".into()))
        );
    }

    #[test]
    fn consumer_revision_boundaries()
    {
        let revision = "a".repeat(40);
        let manifest = format!(
            r#"
[[workspace.metadata.dylint.libraries]]
git = "https://github.com/gandr-lang/quenchant"
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
            manifest.replace("gandr-lang/quenchant", "example/unrelated"),
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
                Reference::Tag(SourceText("rust-1.99.0")),
                SourceText("nightly-2026-08-01"),
                SourceText("0.1.99")
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
                Reference::Tag(SourceText("rust-1.99.0")),
                SourceText("nightly-2026-08-01"),
                SourceText("0.1.99")
            ),
            Maybe::Absent(refusal::Refused::Missing(Pin::Channel))
        );
        assert_eq!(manifest.to_string(), before);
        let mut versioned: toml_edit::DocumentMut = r#"
[workspace.dependencies.clippy_utils]
git = "https://github.com/rust-lang/rust-clippy"
tag = "rust-1.98.0"
version = "=0.1.98" # exact
"#
        .parse()
        .unwrap();
        assert_eq!(
            replace(
                &mut versioned,
                &mut TOOLCHAIN.parse().unwrap(),
                Reference::Tag(SourceText("rust-1.99.0")),
                SourceText("nightly-2026-08-01"),
                SourceText("0.1.99")
            ),
            Maybe::Present(Passed)
        );
        let clippy_utils = versioned
            .get("workspace")
            .and_then(|item| item.get("dependencies"))
            .and_then(|item| item.get("clippy_utils"))
            .unwrap();
        assert_eq!(
            clippy_utils.get("version").unwrap().as_str(),
            Some("=0.1.99")
        );
        assert!(
            versioned.to_string().contains("# exact"),
            "the requirement's comment must remain"
        );
        let revision = "b".repeat(40);
        assert_eq!(
            replace(
                &mut versioned,
                &mut TOOLCHAIN.parse().unwrap(),
                Reference::Rev(SourceText(&revision)),
                SourceText("nightly-2026-10-01"),
                SourceText("0.1.101")
            ),
            Maybe::Present(Passed)
        );
        assert_eq!(
            pin(&versioned, Pin::ClippyRev),
            Maybe::Present(SourceText(&revision))
        );
        assert_eq!(
            pin(&versioned, Pin::ClippyTag),
            Maybe::Absent(refusal::Refused::Missing(Pin::ClippyTag))
        );
        let clippy_utils = versioned
            .get("workspace")
            .and_then(|item| item.get("dependencies"))
            .and_then(|item| item.get("clippy_utils"))
            .unwrap();
        assert_eq!(
            clippy_utils.get("version").unwrap().as_str(),
            Some("=0.1.101")
        );
        assert!(
            versioned.to_string().contains("# exact"),
            "the requirement's comment must remain past a move to a commit"
        );
    }
}
