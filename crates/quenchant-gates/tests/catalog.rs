//! The inventory: which alias each kind of target contributes, and what an
//! unreadable listing does.

use quenchant_gates::GateError;
use quenchant_gates::catalog::TestCatalog;
use quenchant_gates::catalog::target_lookup;
use quenchant_shape::shape::Maybe;

/// A nextest listing covering a library target, a named integration target and
/// a consolidated one, in two packages.
const LISTING: &str = r#"{
  "rust-suites": {
    "a": {
      "package-name": "consumer-kernel-core",
      "binary-id": "consumer-kernel-core",
      "binary-name": "consumer_kernel_core",
      "kind": "lib",
      "testcases": { "check::tests::a_universe_forms_one_level_up": {} }
    },
    "b": {
      "package-name": "consumer-kernel-core",
      "binary-id": "consumer-kernel-core::acceptance",
      "binary-name": "acceptance",
      "kind": "test",
      "testcases": { "memoized_checking_agrees_with_memoless_checking": {} }
    },
    "c": {
      "package-name": "consumer-storage-records",
      "binary-id": "consumer-storage-records::tests",
      "binary-name": "tests",
      "kind": "test",
      "testcases": { "store::a_written_root_opens": {} }
    }
  }
}"#;

/// Build the fixture inventory.
///
/// # Specification
/// trivial.
fn listing() -> TestCatalog
{
    TestCatalog::from_nextest_json(LISTING.into()).expect("the fixture listing is supported")
}

#[test]
fn a_nextest_listing_indexes_by_package_and_target()
{
    let catalog = listing();
    assert_eq!(
        3_usize,
        usize::from(catalog.alias_count()),
        "each listed test contributes exactly one alias"
    );
    assert!(matches!(
        catalog.targets(
            "consumer-kernel-core".into(),
            "check::tests::a_universe_forms_one_level_up".into()
        ),
        Maybe::Present(_)
    ));
    assert!(matches!(
        catalog.targets(
            "consumer-kernel-core".into(),
            "acceptance::memoized_checking_agrees_with_memoless_checking".into()
        ),
        Maybe::Present(_)
    ));
    assert_eq!(
        catalog.targets(
            "consumer-kernel-core".into(),
            "memoized_checking_agrees_with_memoless_checking".into()
        ),
        Maybe::Absent(target_lookup::Missing::AliasUnlisted),
        "an existing package does not expose an unprefixed integration witness"
    );
    assert_eq!(
        catalog.targets(
            "unlisted-package".into(),
            "check::tests::a_universe_forms_one_level_up".into()
        ),
        Maybe::Absent(target_lookup::Missing::PackageUnlisted),
        "an absent package is distinct from a missing alias in a listed package"
    );
    assert!(matches!(
        catalog.targets(
            "consumer-storage-records".into(),
            "tests::store::a_written_root_opens".into()
        ),
        Maybe::Present(_)
    ));
}

#[test]
fn an_unsupported_listing_is_an_operational_error()
{
    let error = TestCatalog::from_nextest_json(r#"{"suites": {}}"#.into())
        .expect_err("a document without `rust-suites` is not an empty inventory");
    assert!(
        matches!(error, GateError::Tool { .. }),
        "an unreadable listing is an operational failure, never a clean inventory: {error:?}"
    );
    let error = TestCatalog::from_nextest_json("not json".into())
        .expect_err("unparseable output is not an empty inventory");
    assert!(
        matches!(error, GateError::Tool { .. }),
        "and neither is unparseable output: {error:?}"
    );
}

#[test]
fn two_listings_merge_into_one_inventory()
{
    let mut catalog = listing();
    let second = TestCatalog::from_nextest_json(
        r#"{
          "rust-suites": {
            "d": {
              "package-name": "consumer-kernel-core",
              "binary-id": "consumer-kernel-core::adversarial_depth",
              "binary-name": "adversarial_depth",
              "kind": "test",
              "testcases": { "the_two_machines_are_total_on_a_chain_deep_term": {} }
            }
          }
        }"#
        .into(),
    )
    .expect("the second fixture listing is supported");
    catalog.absorb(&second);
    assert_eq!(
        4_usize,
        usize::from(catalog.alias_count()),
        "the merge is a union, so the first listing's aliases survive it"
    );
    assert!(matches!(
        catalog.targets(
            "consumer-kernel-core".into(),
            "adversarial_depth::the_two_machines_are_total_on_a_chain_deep_term".into()
        ),
        Maybe::Present(_)
    ));
}
