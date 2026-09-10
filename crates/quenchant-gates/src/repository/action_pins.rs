//! GitHub Action eligibility is independent of YAML's presentation syntax.

use std::path::Path;

use quenchant_gates::GateError;
use quenchant_gates::semantic::ErrorMessage;
use quenchant_gates::semantic::SourceText;
use quenchant_shape::shape::Maybe;

use super::Passed;

/// External action names admitted by the repository policy.
const ALLOWED: &[&str] = &[
    "Swatinem/rust-cache",
    "actions/cache/restore",
    "actions/cache/save",
    "actions/checkout",
    "actions/upload-artifact",
    "jdx/mise-action",
    "taiki-e/install-action",
];

quenchant_shape::reason_enum! {
    /// Closed evidence that an action reference cannot be admitted.
    pub mod refusal {
        /// Action-policy rejection retains the offending file and reference.
        #[derive(Clone, Debug, Eq, PartialEq)]
        pub enum Refused {
            /// A uses value is not a string.
            NonScalar(std::path::PathBuf),
            /// The action has no allowlist entry.
            Unlisted {
                /// YAML source containing the reference.
                path: std::path::PathBuf,
                /// The decoded action reference.
                reference: String,
            },
            /// An admitted action lacks a full lowercase 40-hex revision.
            Unpinned {
                /// YAML source containing the reference.
                path: std::path::PathBuf,
                /// The decoded action reference.
                reference: String,
            },
        }
    }
}

/// Validate decoded uses entries without mistaking literal block text for YAML
/// keys.
///
/// # Specification
/// - ensures: local references pass; external references require an allowlisted
///   name and exactly 40 lowercase hex characters.
/// - provides: sealed `refusal::Refused` evidence for non-scalar, unlisted, and
///   unpinned references.
/// - fails: invalid YAML returns `GateError::Parse` rather than an empty action
///   set.
/// - panics: none.
///
/// # Errors
/// `GateError::Parse` preserves syntax evidence and its input address.
///
/// # Adequacy
/// - hypothesis: L3 block, quoted, flow, and alias forms share policy;
///   pin-length and allowlist boundaries distinguish weakened conjunctions.
/// - witness: `repository::action_pins::tests::yaml_forms_and_pin_boundaries`
/// - witness: `repository::action_pins::tests::invalid_yaml_and_non_scalar_uses`
fn inspect(
    path: &Path,
    text: SourceText<'_>,
) -> Result<Maybe<Passed, refusal::Refused>, GateError>
{
    let documents = yaml_rust2::YamlLoader::load_from_str(text.0)
        .map_err(|error| GateError::parse(path, ErrorMessage(&error.to_string())))?;
    let mut pending: Vec<_> = documents.iter().collect();
    while let Some(node) = pending.pop() {
        match *node {
            | yaml_rust2::Yaml::Hash(ref mapping) => {
                for (key, value) in mapping {
                    if key.as_str() == Some("uses") {
                        let Some(reference) = value.as_str()
                        else {
                            return Ok(Maybe::Absent(refusal::Refused::NonScalar(
                                path.to_path_buf(),
                            )));
                        };
                        if reference.starts_with("./") {
                            continue;
                        }
                        let (action, revision) =
                            reference.split_once('@').unwrap_or((reference, ""));
                        if !ALLOWED.contains(&action) {
                            return Ok(Maybe::Absent(refusal::Refused::Unlisted {
                                path: path.to_path_buf(),
                                reference: reference.into(),
                            }));
                        }
                        if revision.len() != 40
                            || !revision.bytes().all(|byte| {
                                byte.is_ascii_digit() || (b'a' ..= b'f').contains(&byte)
                            })
                        {
                            return Ok(Maybe::Absent(refusal::Refused::Unpinned {
                                path: path.to_path_buf(),
                                reference: reference.into(),
                            }));
                        }
                    }
                    else {
                        pending.push(value);
                    }
                }
            },
            | yaml_rust2::Yaml::Array(ref sequence) => pending.extend(sequence),
            | _ => {},
        }
    }
    Ok(Maybe::Present(Passed))
}

/// Inspect every YAML file below the repository's GitHub directory.
///
/// # Specification
/// - ensures: workflow and composite-action references obey the same allowlist
///   and pin rule.
/// - provides: the first sealed action refusal, with its file and decoded
///   reference.
/// - fails: missing directories, unreadable files, and invalid YAML remain
///   operational errors.
/// - panics: none.
///
/// # Errors
/// `GateError::Io` and `GateError::Parse` prevent an unobserved pass.
///
/// # Adequacy
/// - hypothesis: L3 a nested composite action with an unlisted reference cannot
///   escape workflow-only traversal.
/// - witness: `repository::action_pins::tests::nested_action_is_checked`
pub fn check(root: &Path) -> Result<Maybe<Passed, refusal::Refused>, GateError>
{
    let mut pending = vec![root.join(".github")];
    while let Some(directory) = pending.pop() {
        let entries = std::fs::read_dir(&directory)
            .map_err(|error| GateError::io(&directory, ErrorMessage(&error.to_string())))?;
        for entry in entries {
            let entry = entry
                .map_err(|error| GateError::io(&directory, ErrorMessage(&error.to_string())))?;
            let path = entry.path();
            let kind = entry
                .file_type()
                .map_err(|error| GateError::io(&path, ErrorMessage(&error.to_string())))?;
            if kind.is_dir() {
                pending.push(path);
            }
            else if path
                .extension()
                .is_some_and(|extension| extension == "yml" || extension == "yaml")
            {
                let text = super::read_text(&path)?;
                let relative = path.strip_prefix(root).unwrap_or(&path);
                if let Maybe::Absent(reason) = inspect(relative, SourceText(&text))? {
                    return Ok(Maybe::Absent(reason));
                }
            }
        }
    }
    Ok(Maybe::Present(Passed))
}

#[cfg(test)]
mod tests
{
    use super::*;
    use crate::repository::tests::Fixture;

    #[test]
    fn yaml_forms_and_pin_boundaries()
    {
        let path = Path::new("workflow.yml");
        for revision in [
            "a".repeat(39),
            "A".repeat(40),
            "g".repeat(40),
            "a".repeat(40),
            "a".repeat(41),
            "v7".into(),
            String::new(),
        ] {
            let reference = format!("actions/checkout@{revision}");
            for text in [
                format!("- uses: {reference}"),
                format!("- 'uses': '{reference}'"),
                format!(r#"[{{uses: "{reference}"}}]"#),
                format!("first: &action '{reference}'\nsteps: [{{uses: *action}}]"),
            ] {
                let expected = if revision == "a".repeat(40) {
                    Maybe::Present(Passed)
                }
                else {
                    Maybe::Absent(refusal::Refused::Unpinned {
                        path: path.into(),
                        reference: reference.clone(),
                    })
                };
                assert_eq!(inspect(path, SourceText(&text)).unwrap(), expected);
            }
        }
        let reference = format!("unknown/action@{}", "a".repeat(40));
        assert_eq!(
            inspect(path, SourceText(&format!("uses: {reference}"))).unwrap(),
            Maybe::Absent(refusal::Refused::Unlisted {
                path: path.into(),
                reference
            })
        );
        assert_eq!(
            inspect(
                path,
                SourceText(
                    r#"
steps:
  - uses: ./local-action
  - run: |
      uses: unknown/text
"#
                )
            )
            .unwrap(),
            Maybe::Present(Passed)
        );
    }

    #[test]
    fn invalid_yaml_and_non_scalar_uses()
    {
        let path = Path::new("workflow.yml");
        assert!(
            matches!(
                inspect(path, SourceText("steps: [")),
                Err(GateError::Parse { .. })
            ),
            "invalid YAML must not pass"
        );
        assert_eq!(
            inspect(path, SourceText("uses: [actions/checkout]")).unwrap(),
            Maybe::Absent(refusal::Refused::NonScalar(path.into()))
        );
    }

    #[test]
    fn nested_action_is_checked()
    {
        let fixture = Fixture::new();
        let relative = Path::new(".github/actions/nested/action.yaml");
        let path = fixture.0.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "runs: {steps: [{uses: unknown/action@v1}]}").unwrap();
        assert_eq!(
            check(&fixture.0).unwrap(),
            Maybe::Absent(refusal::Refused::Unlisted {
                path: relative.into(),
                reference: "unknown/action@v1".into()
            })
        );
    }
}
