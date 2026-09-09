//! Witness extraction and resolution: what counts as an obligation, and what
//! each way of failing to resolve one is called.

use std::path::Path;

use quenchant_gates::GateError;
use quenchant_gates::catalog::TestCatalog;
use quenchant_gates::semantic::PackageName;
use quenchant_gates::semantic::SourceText;
use quenchant_gates::witnesses::opens_heading;
use quenchant_gates::witnesses::resolve;
use quenchant_gates::witnesses::witness_claims;

/// An inventory holding one library test and two integration targets of one
/// crate, plus a sibling crate exposing a test of its own.
///
/// # Specification
/// trivial.
fn catalog() -> TestCatalog
{
    TestCatalog::from_nextest_json(
        r#"{
          "rust-suites": {
            "a": {
              "package-name": "owner",
              "binary-id": "owner",
              "binary-name": "owner",
              "kind": "lib",
              "testcases": { "check::tests::a_universe_forms": {} }
            },
            "b": {
              "package-name": "owner",
              "binary-id": "owner::acceptance",
              "binary-name": "acceptance",
              "kind": "test",
              "testcases": { "memoized_agrees_with_memoless": {} }
            },
            "c": {
              "package-name": "sibling",
              "binary-id": "sibling::differential",
              "binary-name": "differential",
              "kind": "test",
              "testcases": { "the_two_walks_agree": {} }
            }
          }
        }"#
        .into(),
    )
    .expect("the fixture listing is supported")
}

/// Extract the claims of one source string, failing the test on a parse error.
///
/// # Specification
/// trivial.
fn claims(source: SourceText<'_>) -> Vec<String>
{
    witness_claims(Path::new("fixture.rs"), source)
        .expect("the fixture parses")
        .into_iter()
        .map(|claim| claim.witness)
        .collect()
}

/// Resolve one source string's claims and report the finding kinds, in order.
///
/// # Specification
/// trivial.
fn kinds(
    package: PackageName<'_>,
    source: SourceText<'_>,
) -> Vec<String>
{
    let claims = witness_claims(Path::new("fixture.rs"), source).expect("the fixture parses");
    resolve(package, &claims, &catalog())
        .into_iter()
        .map(|finding| finding.kind)
        .collect()
}

/// Resolve one source string's claims and report the finding details, in order.
///
/// # Specification
/// trivial.
fn details(
    package: PackageName<'_>,
    source: SourceText<'_>,
) -> Vec<String>
{
    let claims = witness_claims(Path::new("fixture.rs"), source).expect("the fixture parses");
    resolve(package, &claims, &catalog())
        .into_iter()
        .map(|finding| finding.detail)
        .collect()
}

#[test]
fn an_exact_in_crate_witness_resolves()
{
    let source = r"
/// Summary.
///
/// # Adequacy
/// - hypothesis: L3 — the boundary cases are enumerated.
/// - witness: `check::tests::a_universe_forms`
/// - witness: `acceptance::memoized_agrees_with_memoless`
pub fn documented() {}
";
    assert_eq!(
        2_usize,
        claims(source.into()).len(),
        "both bullets are obligations"
    );
    assert!(
        kinds("owner".into(), source.into()).is_empty(),
        "and both resolve in the owning crate's own targets: {:?}",
        details("owner".into(), source.into())
    );
}

#[test]
fn an_absent_witness_is_unresolved()
{
    let source = r"
/// Summary.
///
/// # Adequacy
/// - hypothesis: L3 — the boundary cases are enumerated.
/// - witness: `check::tests::a_test_that_was_renamed`
pub fn documented() {}
";
    assert_eq!(
        vec![String::from("unresolved-witness")],
        kinds("owner".into(), source.into()),
        "a path naming no runnable test fails, which is the whole point of the gate"
    );
}

#[test]
fn a_witness_owned_by_a_sibling_crate_names_the_sibling()
{
    let source = r"
/// Summary.
///
/// # Adequacy
/// - hypothesis: L2 — the differential pins the two walks against each other.
/// - witness: `differential::the_two_walks_agree`
pub fn documented() {}
";
    assert_eq!(
        vec![String::from("unresolved-witness")],
        kinds("owner".into(), source.into()),
        "a witness must name a test in the item's own crate"
    );
    let detail = details("owner".into(), source.into()).join("");
    assert!(
        detail.contains("sibling"),
        "and the diagnostic names the crate that really runs it: {detail}"
    );
}

#[test]
fn a_wrong_target_witness_suggests_the_owning_target()
{
    let source = r"
/// Summary.
///
/// # Adequacy
/// - hypothesis: L2 — the differential pins memoized against memoless.
/// - witness: `differential::memoized_agrees_with_memoless`
pub fn documented() {}
";
    assert_eq!(
        vec![String::from("unresolved-witness")],
        kinds("owner".into(), source.into()),
        "the test exists, but not under the target the bullet names"
    );
    let detail = details("owner".into(), source.into()).join("");
    assert!(
        detail.contains("acceptance::memoized_agrees_with_memoless"),
        "and the diagnostic names the target that really exposes it: {detail}"
    );
}

#[test]
fn a_witness_exposed_by_two_targets_is_ambiguous()
{
    let mut catalog = TestCatalog::new();
    catalog.insert("owner".into(), "round_trips".into(), "owner::first".into());
    catalog.insert("owner".into(), "round_trips".into(), "owner::second".into());
    let source = r"
/// Summary.
///
/// # Adequacy
/// - hypothesis: L3 — the boundary cases are enumerated.
/// - witness: `round_trips`
pub fn documented() {}
";
    let claims =
        witness_claims(Path::new("fixture.rs"), source.into()).expect("the fixture parses");
    let findings = resolve("owner".into(), &claims, &catalog);
    assert_eq!(
        1_usize,
        findings.len(),
        "one bullet, one finding: {findings:?}"
    );
    assert_eq!(
        "ambiguous-witness",
        findings.first().map_or("", |finding| finding.kind.as_str()),
        "a path a reviewer cannot follow to one test is not a witness"
    );
}

#[test]
fn a_bullet_outside_the_adequacy_section_is_not_an_obligation()
{
    let source = r#"
/// Summary.
///
/// # Adequacy
/// - hypothesis: L3 — the boundary cases are enumerated.
/// - witness: `check::tests::a_universe_forms`
///
/// # Specifications
/// - witness: `injected::by::an::attribute::macro`
///
/// ```rust
/// /// # Adequacy
/// /// - witness: `inside::a::doc::fence`
/// ```
pub fn documented() {}

// - witness: `in_an_ordinary_comment`
const NOT_A_WITNESS: &str = "- witness: `in_a_string_literal`";
"#;
    assert_eq!(
        vec![String::from("check::tests::a_universe_forms")],
        claims(source.into()),
        "the section ends at the next heading, and only parsed rustdoc is read at all"
    );
}

#[test]
fn any_heading_level_terminates_the_section()
{
    for heading in [
        "# Specifications",
        "## Specifications",
        "###### Specifications",
    ] {
        assert!(
            bool::from(opens_heading(heading.into())),
            "a heading of any level terminates the section: {heading}"
        );
    }
    for text in ["#no-space", "#", "###", "a # b", "- witness: `x`"] {
        assert!(
            !bool::from(opens_heading(text.into())),
            "and nothing else does: {text}"
        );
    }
}

#[test]
fn a_declaration_only_block_carries_no_obligation()
{
    let source = r"
/// Summary.
///
/// # Adequacy
/// - hypothesis: L0 — the declaration has no behaviour of its own.
/// - declaration-only: every implementation is witnessed at its own impl.
pub fn documented() {}
";
    assert!(
        claims(source.into()).is_empty(),
        "an exemption claims there is nothing to resolve; the dylint gate decides whether the \
         exemption was allowed"
    );
}

#[test]
fn unparseable_source_is_an_operational_error()
{
    let error = witness_claims(Path::new("fixture.rs"), "fn (".into())
        .expect_err("a file that does not parse has not been checked");
    assert!(
        matches!(error, GateError::Parse { .. }),
        "an unreadable file is an operational failure, never a clean crate: {error:?}"
    );
}

#[test]
fn a_finding_names_the_line_the_bullet_sits_on()
{
    let source = r"/// Summary.
///
/// # Adequacy
/// - hypothesis: L3 — enumerated.
/// - witness: `absent::test`
pub fn documented() {}
";
    let claims =
        witness_claims(Path::new("fixture.rs"), source.into()).expect("the fixture parses");
    let findings = resolve("owner".into(), &claims, &catalog());
    assert_eq!(
        5_usize,
        findings
            .first()
            .map_or(0_usize, |finding| usize::from(finding.line)),
        "the finding is addressed to the bullet, not to the item: {findings:?}"
    );
}
