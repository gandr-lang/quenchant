//! One declared package boundary governs publication eligibility.

use std::path::Path;
use std::process::Command;

use quenchant_gates::GateError;
use quenchant_gates::semantic::CommandLine;
use quenchant_gates::semantic::ErrorMessage;
use quenchant_gates::semantic::SourceText;
use quenchant_shape::shape::Maybe;

use super::Passed;

/// Registry eligibility, independently of whether an upload has occurred.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Eligibility
{
    /// Only the public Cargo registry is eligible.
    CratesIo,
    /// Git-distributed tooling and internal fixtures cannot be published.
    Disabled,
}

/// The sole package eligibility authority; actual publication remains manual.
const BOUNDARY: &[(&str, Eligibility)] = &[
    ("quenchant", Eligibility::CratesIo),
    ("quenchant-anodized", Eligibility::CratesIo),
    ("quenchant-arith", Eligibility::CratesIo),
    ("quenchant-dylints", Eligibility::Disabled),
    ("quenchant-fixture-macros", Eligibility::Disabled),
    ("quenchant-gates", Eligibility::CratesIo),
    ("quenchant-shape", Eligibility::CratesIo),
    ("quenchant-spec-macros", Eligibility::CratesIo),
];

quenchant_shape::reason_enum! {
    /// Publication can be refused without losing the classified package.
    pub mod refusal {
        /// Exact reasons the declared package boundary is not satisfied.
        #[derive(Clone, Debug, Eq, PartialEq)]
        pub enum Refused {
            /// An unknown member is outside the declared family.
            Unclassified(String),
            /// A declared package is absent from Cargo's member inventory.
            Missing(String),
            /// The member's registry eligibility differs from its declaration.
            Registry {
                /// The package requiring correction.
                package: String,
                /// Cargo's actual registry eligibility.
                actual: serde_json::Value,
            },
        }
    }
}

/// Compare Cargo's explicit package inventory to the one publication authority.
///
/// # Specification
/// - ensures: the eight declared packages are present, six restricted to
///   crates.io and the Dylint plugin and fixture macros disabled.
/// - provides: sealed `refusal::Refused` evidence for unknown, missing, or
///   incorrectly eligible packages.
/// - fails: malformed Cargo JSON returns `GateError::Tool` rather than a policy
///   verdict.
/// - panics: none.
///
/// # Errors
/// `GateError::Tool` identifies missing, duplicated, or malformed metadata
/// fields.
///
/// # Adequacy
/// - hypothesis: L2 an independent eight-package golden plus L3 unknown,
///   missing, unrestricted, extra-registry, and disabled boundaries distinguish
///   drift in either direction.
/// - witness: `repository::publish_allowlist::tests::exact_publication_boundary`
/// - witness: `repository::publish_allowlist::tests::malformed_metadata_is_operational`
fn inspect(text: SourceText<'_>) -> Result<Maybe<Passed, refusal::Refused>, GateError>
{
    let malformed = |message: &str| {
        GateError::tool(
            CommandLine("cargo metadata --no-deps"),
            ErrorMessage(message),
        )
    };
    let metadata: serde_json::Value =
        serde_json::from_str(text.0).map_err(|error| malformed(&error.to_string()))?;
    let packages = metadata
        .get("packages")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| malformed("missing packages array"))?;
    let mut actual = alloc::collections::BTreeMap::new();
    for package in packages {
        let name = package
            .get("name")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| malformed("missing package name"))?;
        let publish = package
            .get("publish")
            .ok_or_else(|| malformed("missing publish field"))?;
        if !publish.is_null()
            && !publish
                .as_array()
                .is_some_and(|registries| registries.iter().all(serde_json::Value::is_string))
        {
            return Err(malformed("publish must be null or a registry array"));
        }
        if actual.insert(name, publish).is_some() {
            return Err(malformed("duplicate package name"));
        }
    }
    for (name, publish) in &actual {
        let Some(&(_, expected)) = BOUNDARY.iter().find(|&&(known, _)| known == *name)
        else {
            return Ok(Maybe::Absent(refusal::Refused::Unclassified(
                (*name).into(),
            )));
        };
        let agrees = match expected {
            | Eligibility::CratesIo => publish.as_array().is_some_and(|registries| {
                registries.len() == 1
                    && registries.first().and_then(serde_json::Value::as_str) == Some("crates-io")
            }),
            | Eligibility::Disabled => publish.as_array().is_some_and(Vec::is_empty),
        };
        if !agrees {
            return Ok(Maybe::Absent(refusal::Refused::Registry {
                package: (*name).into(),
                actual: (*publish).clone(),
            }));
        }
    }
    for &(name, _) in BOUNDARY {
        if !actual.contains_key(name) {
            return Ok(Maybe::Absent(refusal::Refused::Missing(name.into())));
        }
    }
    Ok(Maybe::Present(Passed))
}

/// Measure member eligibility at the named manifest without resolving
/// dependencies.
///
/// # Specification
/// - ensures: the selected manifest's package inventory satisfies the declared
///   publication boundary before acceptance.
/// - provides: the same sealed reasons as the inventory comparison; no second
///   allowlist is maintained by the driver.
/// - fails: Cargo and metadata decoding failures return `GateError::Tool`.
/// - panics: none.
///
/// # Errors
/// An unavailable or malformed Cargo inventory supplies no publication
/// evidence.
///
/// # Adequacy
/// - hypothesis: L3 a real workspace member outside the allowlist is rejected
///   through the command path.
/// - witness: `gates::repository::subcommands_refuse_broken_fixtures`
pub fn check(manifest: &Path) -> Result<Maybe<Passed, refusal::Refused>, GateError>
{
    let text = super::output(
        Command::new("cargo")
            .args([
                "metadata",
                "--format-version",
                "1",
                "--no-deps",
                "--manifest-path",
            ])
            .arg(manifest),
    )?;
    inspect(SourceText(&text))
}

#[cfg(test)]
mod tests
{
    use super::*;

    /// Independent package golden represents the accepted release surface.
    ///
    /// # Specification
    /// trivial.
    fn golden() -> serde_json::Value
    {
        serde_json::json!({"packages": [
            {"name": "quenchant", "publish": ["crates-io"]},
            {"name": "quenchant-anodized", "publish": ["crates-io"]},
            {"name": "quenchant-arith", "publish": ["crates-io"]},
            {"name": "quenchant-dylints", "publish": []},
            {"name": "quenchant-fixture-macros", "publish": []},
            {"name": "quenchant-gates", "publish": ["crates-io"]},
            {"name": "quenchant-shape", "publish": ["crates-io"]},
            {"name": "quenchant-spec-macros", "publish": ["crates-io"]}
        ]})
    }

    #[test]
    fn exact_publication_boundary()
    {
        assert_eq!(
            inspect(SourceText(&golden().to_string())).unwrap(),
            Maybe::Present(Passed)
        );
        let mut unknown = golden();
        unknown["packages"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"name": "outside", "publish": []}));
        assert_eq!(
            inspect(SourceText(&unknown.to_string())).unwrap(),
            Maybe::Absent(refusal::Refused::Unclassified("outside".into()))
        );
        let mut missing = golden();
        missing["packages"].as_array_mut().unwrap().remove(0);
        assert_eq!(
            inspect(SourceText(&missing.to_string())).unwrap(),
            Maybe::Absent(refusal::Refused::Missing("quenchant".into()))
        );
        for (package, actual) in [
            ("quenchant", serde_json::json!(null)),
            ("quenchant", serde_json::json!([])),
            (
                "quenchant",
                serde_json::json!(["crates-io", "private-registry"]),
            ),
            ("quenchant-fixture-macros", serde_json::json!(["crates-io"])),
            ("quenchant-dylints", serde_json::json!(["crates-io"])),
        ] {
            let mut metadata = golden();
            for entry in metadata["packages"].as_array_mut().unwrap() {
                if entry["name"] == package {
                    entry["publish"] = actual.clone();
                }
            }
            assert_eq!(
                inspect(SourceText(&metadata.to_string())).unwrap(),
                Maybe::Absent(refusal::Refused::Registry {
                    package: package.into(),
                    actual
                })
            );
        }
    }

    #[test]
    fn malformed_metadata_is_operational()
    {
        for text in [
            "{",
            "{}",
            r#"{"packages":[{"name":"quenchant"}]}"#,
            r#"{"packages":[{"name":"quenchant","publish":false}]}"#,
        ] {
            assert!(
                matches!(inspect(SourceText(text)), Err(GateError::Tool { .. })),
                "malformed metadata cannot produce a verdict"
            );
        }
    }
}
