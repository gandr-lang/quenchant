//! The specification-enforcement state of a Cargo invocation.
//!
//! `anodized-macros` chooses instrumentation when its host artifact is built.
//! `anodized_discard_specs` removes checks and takes precedence over every
//! other mode; `anodized_panic` rejects violations; `anodized_print` alone only
//! reports them. With neither flag, specifications are type-checked but not
//! enforced.
//!
//! # Specification
//! The gate asks `cargo rustc -Z unstable-options --print cfg` at the consumer
//! manifest's directory. Cargo resolves configuration, `RUSTFLAGS`, and
//! `CARGO_ENCODED_RUSTFLAGS` for this invocation; the installed gate's own cfgs
//! are irrelevant. Discarded specifications fail every lane. An enforcing lane
//! additionally requires the panic state, requested with `--require-enforcing`.
//! Missing compiler evidence or a failed query is an operational error.
//!
//! This is invocation-state evidence, not a census of compiled dependencies.
//! In particular, explicit targets can separate host proc-macro flags from
//! target flags. Consumer enforcing lanes must also execute a specification
//! sentinel built with their graph-wide flags; cached artifacts and target-only
//! flags cannot be certified by a target cfg report.

use std::path::Path;
use std::process::Command;

/// The resolved cfg listing printed by Cargo's compiler query.
#[repr(transparent)]
#[derive(Clone, Copy, Debug)]
pub struct CfgText<'text>(&'text str);

impl<'text> From<&'text str> for CfgText<'text>
{
    /// Wrap the borrowed cfg listing in the semantic type.
    ///
    /// # Specification
    /// trivial.
    #[inline]
    fn from(value: &'text str) -> Self
    {
        Self(value)
    }
}

/// Which checks this invocation requests.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EnforcementState
{
    /// Instrumentation is removed, even if another checking flag is present.
    Discarded,
    /// Violations panic; printing may also be enabled.
    Enforcing,
    /// Violations print without rejecting execution.
    PrintOnly,
    /// Predicates are type-checked without runtime enforcement.
    NonEnforcing,
}

impl core::fmt::Display for EnforcementState
{
    /// Write the state's name.
    ///
    /// # Specification
    /// - ensures: writes the lower-case name of the state, hyphenated where it
    ///   is two words.
    /// - fails: propagates the formatter's own write failure.
    /// - panics: none.
    #[inline]
    fn fmt(
        &self,
        f: &mut core::fmt::Formatter<'_>,
    ) -> core::fmt::Result
    {
        f.write_str(match *self {
            | Self::Discarded => "discarded",
            | Self::Enforcing => "enforcing",
            | Self::PrintOnly => "print-only",
            | Self::NonEnforcing => "non-enforcing",
        })
    }
}

/// The policy the current lane requires.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Requirement
{
    /// Report the state, rejecting only discarded specifications.
    Observe,
    /// Require checks that panic on violations.
    Enforcing,
}

/// Whether measured state satisfies lane policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Verdict
{
    /// The measured invocation satisfies the requested policy.
    Pass,
    /// The measured invocation violates the requested policy.
    Fail,
}

impl EnforcementState
{
    /// Apply lane policy without conflating observation with enforcement.
    ///
    /// # Specification
    /// - ensures: discarded specifications always fail; other states pass
    ///   observation; only `Enforcing` passes the enforcing requirement.
    /// - panics: none.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — every state is checked under both lane requirements.
    /// - witness: `anodized::tests::every_state_obeys_lane_policy`
    #[inline]
    #[must_use]
    pub fn verdict(
        self,
        requirement: Requirement,
    ) -> Verdict
    {
        match (self, requirement) {
            | (Self::Discarded, _)
            | (Self::PrintOnly | Self::NonEnforcing, Requirement::Enforcing) => Verdict::Fail,
            | (Self::Enforcing, _)
            | (Self::PrintOnly | Self::NonEnforcing, Requirement::Observe) => Verdict::Pass,
        }
    }
}

/// Why the invocation could not be measured.
#[derive(Debug)]
pub enum GateError
{
    /// The requested manifest cannot be resolved to a file.
    Manifest(std::io::Error),
    /// Cargo could not be started or waited for.
    CargoNotRun(std::io::Error),
    /// Cargo rejected the query; its diagnostics are retained.
    CargoRefused(String),
    /// Compiler output was not UTF-8 text.
    InvalidText(alloc::string::FromUtf8Error),
    /// Output lacks the compiler's target cfg evidence.
    MissingCompilerCfgs,
}

impl core::fmt::Display for GateError
{
    /// Write the error's cause, retaining the diagnostic it carries.
    ///
    /// # Specification
    /// - ensures: writes one sentence naming the failed step and the retained
    ///   diagnostic.
    /// - fails: propagates the formatter's own write failure.
    /// - panics: none.
    #[inline]
    fn fmt(
        &self,
        f: &mut core::fmt::Formatter<'_>,
    ) -> core::fmt::Result
    {
        match *self {
            | Self::Manifest(ref error) => write!(f, "cannot read consumer manifest: {error}"),
            | Self::CargoNotRun(ref error) => {
                write!(f, "could not run Cargo cfg query: {error}")
            },
            | Self::CargoRefused(ref error) => {
                write!(f, "Cargo cfg query refused: {}", error.trim_end())
            },
            | Self::InvalidText(ref error) => {
                write!(f, "compiler cfg output is not UTF-8: {error}")
            },
            | Self::MissingCompilerCfgs => {
                f.write_str("cfg query returned no compiler target evidence")
            },
        }
    }
}

impl core::error::Error for GateError
{
}

/// Resolve the consumer invocation's cfgs and classify its enforcement state.
///
/// # Specification
/// - requires: Cargo supports the nightly `--print cfg` query.
/// - ensures: resolves flags at the manifest's directory rather than the
///   installed binary's source checkout; returns the measured enforcement
///   state.
/// - fails: returns a manifest, process, query, or output error rather than an
///   unmeasured passing verdict.
/// - panics: none.
///
/// # Errors
/// Returns the corresponding [`GateError`] for each failed boundary operation.
///
/// # Adequacy
/// - hypothesis: L3 — a separate consumer manifest selects Cargo configuration;
///   encoded flags override both ordinary flags and that configuration.
/// - witness: `gates::anodized::consumer_configuration_and_encoded_flags_select_the_state`
#[inline]
pub fn invocation_state(manifest: &Path) -> Result<EnforcementState, GateError>
{
    let manifest = manifest.canonicalize().map_err(GateError::Manifest)?;
    let Some(directory) = manifest.parent()
    else {
        return Err(GateError::Manifest(std::io::Error::other(
            "manifest has no parent directory",
        )));
    };
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| std::ffi::OsString::from("cargo"));
    let output = Command::new(cargo)
        .args([
            "rustc",
            "-Z",
            "unstable-options",
            "--print",
            "cfg",
            "--manifest-path",
        ])
        .arg(&manifest)
        .current_dir(directory)
        .output()
        .map_err(GateError::CargoNotRun)?;
    if !output.status.success() {
        return Err(GateError::CargoRefused(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ));
    }
    let output = String::from_utf8(output.stdout).map_err(GateError::InvalidText)?;
    enforcement_state(CfgText::from(output.as_str()))
}

/// Classify exact cfg names with the macro's discard-before-panic precedence.
///
/// # Specification
/// - requires: `cfgs` is the compiler cfg listing, not Rust flags or source
///   text.
/// - ensures: discard wins over panic, panic over print, otherwise
///   non-enforcing; valued or similarly named cfgs do not enable a bare flag.
/// - fails: missing `target_arch`, `target_os`, or `target_pointer_width`
///   evidence returns [`GateError::MissingCompilerCfgs`].
/// - panics: none.
///
/// # Errors
/// Returns [`GateError::MissingCompilerCfgs`] for an unmeasured listing.
///
/// # Adequacy
/// - hypothesis: L3 — all eight mode combinations distinguish precedence;
///   exact-name near misses and missing evidence distinguish false passes.
/// - witness: `anodized::tests::every_mode_combination_matches_macro_precedence`
/// - witness: `anodized::tests::similar_or_valued_cfgs_do_not_enable_modes`
/// - witness: `anodized::tests::missing_compiler_evidence_is_an_error`
#[inline]
pub fn enforcement_state(cfgs: CfgText<'_>) -> Result<EnforcementState, GateError>
{
    let mut discard = false;
    let mut panic = false;
    let mut print = false;
    let mut arch = false;
    let mut os = false;
    let mut width = false;
    for line in cfgs.0.lines().map(str::trim) {
        match line {
            | "anodized_discard_specs" => discard = true,
            | "anodized_panic" => panic = true,
            | "anodized_print" => print = true,
            | _ => {
                arch |= line.starts_with("target_arch=\"");
                os |= line.starts_with("target_os=\"");
                width |= line.starts_with("target_pointer_width=\"");
            },
        }
    }
    if !(arch && os && width) {
        return Err(GateError::MissingCompilerCfgs);
    }
    Ok(if discard {
        EnforcementState::Discarded
    }
    else if panic {
        EnforcementState::Enforcing
    }
    else if print {
        EnforcementState::PrintOnly
    }
    else {
        EnforcementState::NonEnforcing
    })
}

#[cfg(test)]
mod tests
{
    use super::CfgText;
    use super::EnforcementState;
    use super::GateError;
    use super::Requirement;
    use super::Verdict;
    use super::enforcement_state;

    /// Independent compiler evidence shared by the mode cases.
    const TARGET: &str = r#"target_arch="aarch64"
target_os="linux"
target_pointer_width="64"
"#;

    #[test]
    fn every_mode_combination_matches_macro_precedence()
    {
        for (flags, expected) in [
            ("", EnforcementState::NonEnforcing),
            ("anodized_print", EnforcementState::PrintOnly),
            ("anodized_panic", EnforcementState::Enforcing),
            (
                r#"anodized_panic
anodized_print"#,
                EnforcementState::Enforcing,
            ),
            ("anodized_discard_specs", EnforcementState::Discarded),
            (
                r#"anodized_discard_specs
anodized_print"#,
                EnforcementState::Discarded,
            ),
            (
                r#"anodized_discard_specs
anodized_panic"#,
                EnforcementState::Discarded,
            ),
            (
                r#"anodized_discard_specs
anodized_panic
anodized_print"#,
                EnforcementState::Discarded,
            ),
        ] {
            let cfgs = format!("{TARGET}{flags}");
            assert_eq!(
                enforcement_state(CfgText::from(cfgs.as_str())).unwrap(),
                expected,
                "cfgs: {flags}"
            );
        }
    }

    #[test]
    fn similar_or_valued_cfgs_do_not_enable_modes()
    {
        let cfgs = format!(
            r#"{TARGET}anodized_panic="true"
anodized_discard_specs_extra
feature="anodized_print"
"#
        );
        assert_eq!(
            enforcement_state(CfgText::from(cfgs.as_str())).unwrap(),
            EnforcementState::NonEnforcing
        );
    }

    #[test]
    fn missing_compiler_evidence_is_an_error()
    {
        for cfgs in ["", "anodized_panic", r#"target_arch="aarch64""#] {
            assert!(
                matches!(
                    enforcement_state(CfgText::from(cfgs)),
                    Err(GateError::MissingCompilerCfgs)
                ),
                "unmeasured listing: {cfgs}"
            );
        }
    }

    #[test]
    fn every_state_obeys_lane_policy()
    {
        for (state, observed, enforced) in [
            (EnforcementState::Discarded, Verdict::Fail, Verdict::Fail),
            (EnforcementState::Enforcing, Verdict::Pass, Verdict::Pass),
            (EnforcementState::PrintOnly, Verdict::Pass, Verdict::Fail),
            (EnforcementState::NonEnforcing, Verdict::Pass, Verdict::Fail),
        ] {
            assert_eq!(state.verdict(Requirement::Observe), observed);
            assert_eq!(state.verdict(Requirement::Enforcing), enforced);
        }
    }
}
