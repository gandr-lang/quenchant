//! Evidence boundaries that require a consumer invocation or workspace
//! inventory.
//!
//! A compiler lint sees a crate's HIR, not the whole build graph or the
//! runnable test inventory. These gates obtain those views explicitly and
//! preserve the consumer manifest, package, target, and operational failure
//! that qualify a result.
//!
//! Invocation-state reporting does not certify a host macro artifact. Witness
//! resolution establishes a runnable name in its owning scope, not execution,
//! distinguishing power, or completeness of the specification it accompanies.
//! Nextest and native-harness inventory routes preserve the same package/target
//! identity so similarly named tests cannot silently satisfy each other's
//! obligations.

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

/// A policy finding whose source address and owning scope remain available for
/// repair.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Finding
{
    /// Machine-readable classification, independent of the explanatory wording.
    pub kind: String,
    /// Owning Cargo package; a same-named test elsewhere is not a substitute.
    pub package: String,
    /// Source address supplied by the catalog, not the tool's installation
    /// path.
    pub path: PathBuf,
    /// One-based source position, not a byte offset or doc-block ordinal.
    pub line: LineNumber,
    /// Authored spelling retained for repair rather than rewritten to a near
    /// match.
    pub witness: String,
    /// Reader-directed detail; the stable class and source address remain
    /// separate.
    pub detail: String,
}

impl Finding
{
    /// Finding construction retains the authored obligation's complete repair
    /// address.
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
    /// A diagnostic line preserves package ownership and source location.
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

/// An operational failure that prevented the selected gate from reaching a
/// verdict.
///
/// An unreadable workspace or failed inventory query has supplied no successful
/// observation. This channel remains distinct from a reached policy finding,
/// even though the command driver treats either as an unsuccessful run.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum GateError
{
    /// Source access failed before the file or directory could be inspected.
    Io
    {
        /// Source address whose contents were unavailable.
        path: PathBuf,
        /// Diagnostic supplied by the failed access operation.
        message: String,
    },
    /// Source text could not be interpreted as a Rust file.
    Parse
    {
        /// Source address of the rejected Rust text.
        path: PathBuf,
        /// Parser evidence explaining the rejection.
        message: String,
    },
    /// An external instrument failed to supply usable output.
    Tool
    {
        /// Invocation identity, including the scope selected by its arguments.
        command: String,
        /// Process or output-format evidence explaining the failure.
        message: String,
    },
}

impl GateError
{
    /// I/O failure retains the source address needed for repair.
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

    /// Parse failure retains the rejected file's identity.
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

    /// Tool failure retains the invocation that could not supply evidence.
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
    /// Operational diagnostics preserve the failing address or invocation.
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
