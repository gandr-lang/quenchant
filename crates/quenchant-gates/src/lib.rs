//! Non-lint gates for Rust workspaces.
//!
//! A dylint pass sees one crate's HIR. Some invariants are not visible there at
//! all: they live in the build graph, in the test inventory, or in the shape of
//! the workspace. Those gates live here, and they run as ordinary binaries the
//! wall invokes.
//!
//! # The gates
//!
//! [`anodized`] reports the consumer invocation's specification-enforcement
//! state. G0 (the `witnesses` subcommand of the binary) closes the other half
//! of the `# Adequacy` grammar: every `-
//! witness:` path in the workspace must resolve to **exactly one runnable test
//! in the item's own crate's targets**. Absent, renamed, ambiguous and
//! wrong-target paths fail.
//!
//! A witness bullet that names nothing runnable is documentation wearing a
//! test's clothes: it passes review, survives refactors that rename the test it
//! meant, and reports adequacy that was never measured.
//!
//! The test inventory comes from `cargo nextest list --message-format json`
//! when nextest is on the path, and otherwise from `cargo test --no-run
//! --message-format json` followed by `--list` on each built test binary. Both
//! routes preserve the owning package and target of every test, which is what
//! makes a wrong-target path distinguishable from a correct one.

extern crate alloc;

pub mod anodized;

pub mod catalog;

pub mod inventory;

pub mod semantic;

pub mod witnesses;

use core::fmt;
use std::path::Path;
use std::path::PathBuf;

use crate::semantic::CommandLine;
use crate::semantic::ErrorMessage;
use crate::semantic::FindingDetail;
use crate::semantic::FindingKind;
use crate::semantic::LineNumber;
use crate::semantic::PackageName;
use crate::semantic::WitnessPath;

/// One gate violation, addressed to the source line that must change.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Finding
{
    /// The stable classification of the violation.
    pub kind: String,
    /// The Cargo package owning the file the violation was found in.
    pub package: String,
    /// The file the violation was found in.
    pub path: PathBuf,
    /// The one-based line of the offending rustdoc bullet.
    pub line: LineNumber,
    /// The witness path as written.
    pub witness: String,
    /// What is wrong, and what would resolve it.
    pub detail: String,
}

impl Finding
{
    /// Build one finding.
    ///
    /// # Specification
    /// trivial.
    #[inline]
    #[must_use]
    pub fn new(
        kind: FindingKind<'_>,
        package: PackageName<'_>,
        path: &Path,
        line: LineNumber,
        witness: WitnessPath<'_>,
        detail: FindingDetail<'_>,
    ) -> Self
    {
        Self {
            kind: kind.0.to_owned(),
            package: package.0.to_owned(),
            path: path.to_path_buf(),
            line,
            witness: witness.0.to_owned(),
            detail: detail.0.to_owned(),
        }
    }
}

impl fmt::Display for Finding
{
    /// Write the finding as one reader-addressable line.
    ///
    /// # Specification
    /// - ensures: writes path, line, kind, package, witness and detail in that
    ///   order, the path and line first so an editor can jump to it.
    /// - fails: propagates the formatter's own write failure.
    /// - panics: none.
    #[inline]
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result
    {
        write!(
            f,
            "{}:{}: {} [{}] `{}`: {}",
            self.path.display(),
            self.line.0,
            self.kind,
            self.package,
            self.witness,
            self.detail
        )
    }
}

/// Why a gate run could not reach a verdict.
///
/// An operational error is never a finding: a gate that cannot read the
/// workspace has measured nothing, and reporting that as "no violations" is the
/// failure mode this crate exists to prevent.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum GateError
{
    /// A file or directory could not be read.
    Io
    {
        /// The path that could not be read.
        path: PathBuf,
        /// The underlying diagnostic.
        message: String,
    },
    /// A Rust source file could not be parsed.
    Parse
    {
        /// The file that failed to parse.
        path: PathBuf,
        /// The underlying diagnostic.
        message: String,
    },
    /// A tool invocation failed, or produced output the gate cannot read.
    Tool
    {
        /// The command line the gate ran.
        command: String,
        /// The underlying diagnostic.
        message: String,
    },
}

impl GateError
{
    /// Build an I/O error naming the path that failed.
    ///
    /// # Specification
    /// trivial.
    #[inline]
    #[must_use]
    pub fn io(
        path: &Path,
        message: ErrorMessage<'_>,
    ) -> Self
    {
        Self::Io {
            path: path.to_path_buf(),
            message: message.0.to_owned(),
        }
    }

    /// Build a parse error naming the file that failed.
    ///
    /// # Specification
    /// trivial.
    #[inline]
    #[must_use]
    pub fn parse(
        path: &Path,
        message: ErrorMessage<'_>,
    ) -> Self
    {
        Self::Parse {
            path: path.to_path_buf(),
            message: message.0.to_owned(),
        }
    }

    /// Build a tool error naming the command that failed.
    ///
    /// # Specification
    /// trivial.
    #[inline]
    #[must_use]
    pub fn tool(
        command: CommandLine<'_>,
        message: ErrorMessage<'_>,
    ) -> Self
    {
        Self::Tool {
            command: command.0.to_owned(),
            message: message.0.to_owned(),
        }
    }
}

impl fmt::Display for GateError
{
    /// Write the error's cause, naming the path or command that failed.
    ///
    /// # Specification
    /// - ensures: writes one sentence naming the failed read, parse or
    ///   invocation and the diagnostic retained with it.
    /// - fails: propagates the formatter's own write failure.
    /// - panics: none.
    #[inline]
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result
    {
        match *self {
            | Self::Io {
                ref path,
                ref message,
            } => write!(f, "cannot read {}: {message}", path.display()),
            | Self::Parse {
                ref path,
                ref message,
            } => write!(f, "cannot parse {}: {message}", path.display()),
            | Self::Tool {
                ref command,
                ref message,
            } => write!(f, "`{command}` failed: {message}"),
        }
    }
}

impl core::error::Error for GateError
{
}
