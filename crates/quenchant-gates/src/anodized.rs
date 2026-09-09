//! Report the compiler cfg evidence for one named consumer invocation.
//!
//! The published backend chooses its mode when its host artifact is compiled.
//! Discard wins over other cfgs; panic rejects violations; print alone reports
//! them; neither enforcing flag leaves only predicate compilation.
//!
//! Cargo resolves the consumer's configuration and environment when queried
//! with `rustc -Z unstable-options --print cfg`. The installed gate's own cfgs
//! do not describe that invocation. A failed or unreadable query is an
//! operational failure, not evidence of a benign default mode.
//!
//! Backend discard is rejected by this policy, and `--require-enforcing` also
//! requires panic mode. The facade's consumer-side feature is a different
//! selection boundary: this query neither counts emitted checks nor certifies
//! that a cached host artifact used target-only flags. Enforcing lanes also run
//! a deliberately violated specification under their actual dependency graph.
//!
//! Omitting executable instrumentation changes neither an authored obligation
//! nor its authority, and supplies no evidence for that obligation.

use std::path::Path;
use std::process::Command;

/// Compiler-query output, distinct from flag arguments or authored source.
#[repr(transparent)]
#[derive(Clone, Copy, Debug)]
pub struct CfgText<'text>(&'text str);

impl<'text> From<&'text str> for CfgText<'text>
{
    /// Borrowed text enters the compiler-cfg interpretation boundary.
    ///
    /// # Specification
    /// trivial.
    #[inline]
    fn from(value: &'text str) -> Self
    {
        Self(value)
    }
}

/// Checking mode selected by the resolved invocation cfgs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EnforcementState
{
    /// Discard cfg takes precedence over every requested checking mode.
    Discarded,
    /// Panic cfg is selected without discard; printing may also be selected.
    Enforcing,
    /// Print cfg is selected without discard or panic.
    PrintOnly,
    /// No discard, panic, or print cfg is selected.
    NonEnforcing,
}

impl core::fmt::Display for EnforcementState
{
    /// Stable mode labels identify the measured cfg classification.
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

/// Policy applied to one measured invocation state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Requirement
{
    /// Observation permits every mode except explicit discard.
    Observe,
    /// Acceptance requires the panic-enabled mode.
    Enforcing,
}

/// Acceptance of measured cfg state under the selected policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Verdict
{
    /// The observation meets the requested mode requirement.
    Pass,
    /// The observation does not meet the requested mode requirement.
    Fail,
}

impl EnforcementState
{
    /// Observation and enforcing policy have different acceptance sets.
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

/// Operational failures that prevent an invocation-state measurement.
#[derive(Debug)]
pub enum GateError
{
    /// The consumer manifest location cannot be resolved.
    Manifest(std::io::Error),
    /// Starting or collecting the Cargo process failed.
    CargoNotRun(std::io::Error),
    /// The query exited unsuccessfully with diagnostic text.
    CargoRefused(String),
    /// Query bytes cannot be interpreted as UTF-8 cfg text.
    InvalidText(alloc::string::FromUtf8Error),
    /// The output supplies no complete target-evidence trio.
    MissingCompilerCfgs,
}

impl core::fmt::Display for GateError
{
    /// Error rendering keeps the failed operation and its underlying evidence
    /// together.
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

/// Consumer-local Cargo configuration determines the cfg observation.
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

/// Exact cfg spelling and precedence determine the published backend's
/// requested mode.
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

    /// Target evidence remains fixed while checking-mode flags vary.
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
