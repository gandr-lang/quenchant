//! The workspace test inventory, indexed by owning package and target.
//!
//! A witness path is resolved against the crate that wrote it, never against
//! the workspace at large. Indexing by package is what turns "some test
//! somewhere is called this" — which every workspace satisfies by accident —
//! into "this crate's own suite runs it".
//!
//! # The alias a target contributes
//!
//! - A library or binary target contributes the test path as the harness
//!   reports it, which is the module path from the crate root:
//!   `memo::tests::an_ordered_memo_serves_what_it_was_told`.
//! - An integration target contributes the target name in front of it:
//!   `acceptance::memoized_checking_agrees_with_memoless_checking` for
//!   `tests/acceptance.rs`, and `tests::store::a_written_root_opens` for a
//!   consolidated suite whose target is named `tests`.
//!
//! The target prefix is not decoration. Two integration targets in one crate
//! may both declare `fn round_trips()`, and a witness that names neither target
//! has not said which test a reviewer should watch fail.

use alloc::collections::BTreeMap;
use alloc::collections::BTreeSet;

use quenchant_shape::shape::Maybe;

use crate::GateError;
use crate::semantic::AliasCount;
use crate::semantic::ErrorMessage;
use crate::semantic::IntegrationTarget;
use crate::semantic::PackageName;
use crate::semantic::SourceText;
use crate::semantic::TargetKind;
use crate::semantic::TargetLabel;
use crate::semantic::TargetName;
use crate::semantic::TestAlias;
use crate::semantic::WitnessPath;

quenchant_shape::reason_enum! {
    /// Missing ownership and missing aliases require distinct lookup evidence.
    pub mod target_lookup {
        /// Why an exact package-scoped witness has no target set.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Missing {
            /// The catalog contains no aliases for this package.
            PackageUnlisted,
            /// The package is listed but does not expose this alias.
            AliasUnlisted,
        }
    }
}

/// Every runnable test in the workspace, by owning package and exact alias.
#[repr(transparent)]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TestCatalog
{
    /// Package name to alias to the labels of the targets exposing it.
    aliases: BTreeMap<String, BTreeMap<String, BTreeSet<String>>>,
}

impl TestCatalog
{
    /// An inventory holding no tests.
    ///
    /// # Specification
    /// trivial.
    #[inline]
    #[must_use]
    pub fn new() -> Self
    {
        Self {
            aliases: BTreeMap::new(),
        }
    }

    /// How many distinct package-and-alias pairs the inventory holds.
    ///
    /// # Specification
    /// - requires: nothing.
    /// - ensures: counts each alias once per package, however many targets
    ///   expose it.
    /// - provides: the emptiness check a caller uses to refuse a listing that
    ///   found nothing, which would otherwise pass every witness silently.
    /// - panics: none.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the count is asserted exactly on a fixture whose
    ///   aliases are known, so a miscount is a wrong number rather than a
    ///   missing assertion.
    /// - witness: `gates::catalog::a_nextest_listing_indexes_by_package_and_target`
    #[inline]
    #[must_use]
    pub fn alias_count(&self) -> AliasCount
    {
        AliasCount(
            self.aliases
                .values()
                .map(BTreeMap::len)
                .fold(0_usize, usize::saturating_add),
        )
    }

    /// Merge another inventory into this one.
    ///
    /// # Specification
    /// - requires: `other` lists packages disjoint from, or consistent with,
    ///   the ones already held.
    /// - ensures: every package, alias and target of `other` is present
    ///   afterwards, and nothing already held is dropped.
    /// - provides: the union of the per-toolchain listings a workspace with a
    ///   toolchain-pinning member needs.
    /// - panics: none.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — two listings that both carry a package are merged and
    ///   the resulting alias set is asserted to hold both sides.
    /// - witness: `gates::catalog::two_listings_merge_into_one_inventory`
    #[inline]
    pub fn absorb(
        &mut self,
        other: &Self,
    )
    {
        for (package, table) in &other.aliases {
            let own = self.aliases.entry(package.clone()).or_default();
            for (alias, targets) in table {
                own.entry(alias.clone())
                    .or_default()
                    .extend(targets.iter().cloned());
            }
        }
    }

    /// Record one alias exposed by one target of one package.
    ///
    /// # Specification
    /// - ensures: the target joins the set recorded for that package and alias,
    ///   creating either level that is absent; recording the same target twice
    ///   changes nothing.
    /// - panics: none.
    #[inline]
    pub fn insert(
        &mut self,
        package: PackageName<'_>,
        alias: TestAlias<'_>,
        target: TargetLabel<'_>,
    )
    {
        self.aliases
            .entry(package.0.to_owned())
            .or_default()
            .entry(alias.0.to_owned())
            .or_default()
            .insert(target.0.to_owned());
    }

    /// The targets of `package` exposing `witness`, if any.
    ///
    /// # Specification
    /// - ensures: returns the recorded target set exactly when `package` holds
    ///   an alias spelled as `witness`, and nothing otherwise.
    /// - provides: `target_lookup::Missing::PackageUnlisted` means the package
    ///   has no catalog entry; `AliasUnlisted` means its entry lacks the alias.
    /// - panics: none.
    #[inline]
    pub fn targets(
        &self,
        package: PackageName<'_>,
        witness: WitnessPath<'_>,
    ) -> Maybe<&BTreeSet<String>, target_lookup::Missing>
    {
        let Some(aliases) = self.aliases.get(package.0)
        else {
            return Maybe::Absent(target_lookup::Missing::PackageUnlisted);
        };
        match aliases.get(witness.0) {
            | Some(targets) => Maybe::Present(targets),
            | None => Maybe::Absent(target_lookup::Missing::AliasUnlisted),
        }
    }

    /// The packages other than `package` whose own targets expose `witness`.
    ///
    /// # Specification
    /// - requires: nothing.
    /// - ensures: names every other package holding the exact alias, in a
    ///   deterministic order, and never names `package` itself.
    /// - provides: the diagnostic that separates "this test does not exist"
    ///   from "this test belongs to a different crate".
    /// - panics: none.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — a witness naming a test that exists only in a sibling
    ///   crate is asserted to report the sibling by name, on a fixture where
    ///   both crates are present.
    /// - witness: `gates::witnesses::a_witness_owned_by_a_sibling_crate_names_the_sibling`
    #[inline]
    #[must_use]
    pub fn foreign_packages(
        &self,
        package: PackageName<'_>,
        witness: WitnessPath<'_>,
    ) -> Vec<String>
    {
        self.aliases
            .iter()
            .filter(|&(name, _)| name.as_str() != package.0)
            .filter(|&(_, table)| table.contains_key(witness.0))
            .map(|(name, _)| name.clone())
            .collect()
    }

    /// Aliases of `package` whose final path segment matches `witness`'s.
    ///
    /// # Specification
    /// - requires: nothing.
    /// - ensures: returns the package's own aliases sharing the witness's final
    ///   `::` segment, excluding an exact match, in a deterministic order.
    /// - provides: the repair suggestion for the commonest defect — a witness
    ///   that names the right test under the wrong target.
    /// - panics: none.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — a witness naming the correct test under a nonexistent
    ///   target is asserted to suggest the target that really holds it.
    /// - witness: `gates::witnesses::a_wrong_target_witness_suggests_the_owning_target`
    #[inline]
    #[must_use]
    pub fn near_misses(
        &self,
        package: PackageName<'_>,
        witness: WitnessPath<'_>,
    ) -> Vec<String>
    {
        let Some(table) = self.aliases.get(package.0)
        else {
            return Vec::new();
        };
        let leaf = leaf_segment(TestAlias::from(witness.0));
        table
            .keys()
            .filter(|alias| alias.as_str() != witness.0)
            .filter(|alias| leaf_segment(TestAlias::from(*alias)) == leaf)
            .cloned()
            .collect()
    }

    /// Build an inventory from `cargo nextest list --message-format json`.
    ///
    /// # Specification
    /// - requires: `source` is one nextest aggregate JSON document.
    /// - ensures: every listed test contributes exactly one alias under its own
    ///   package, prefixed by its target name when the target is an integration
    ///   test; a document without a `rust-suites` table is refused rather than
    ///   read as an empty inventory.
    /// - provides: the resolution table for every witness in the workspace.
    /// - fails: [`GateError::Tool`] when the document is not a supported
    ///   nextest listing.
    /// - panics: none.
    ///
    /// # Errors
    /// [`GateError::Tool`] on unsupported or malformed nextest output.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the fixture separates a library target, a named
    ///   integration target and a consolidated integration target, asserting
    ///   the exact alias each contributes; an unsupported document is asserted
    ///   to be an operational error rather than an empty inventory.
    /// - witness: `gates::catalog::a_nextest_listing_indexes_by_package_and_target`
    /// - witness: `gates::catalog::an_unsupported_listing_is_an_operational_error`
    #[inline]
    pub fn from_nextest_json(source: SourceText<'_>) -> Result<Self, GateError>
    {
        let document: serde_json::Value = serde_json::from_str(source.0).map_err(|error| {
            GateError::tool(
                "cargo nextest list".into(),
                ErrorMessage::from(&error.to_string()),
            )
        })?;
        let Some(suites) = document
            .get("rust-suites")
            .and_then(serde_json::Value::as_object)
        else {
            return Err(GateError::tool(
                "cargo nextest list".into(),
                "output has no `rust-suites` table; the nextest JSON schema has moved".into(),
            ));
        };
        let mut catalog = Self::new();
        for suite in suites.values() {
            let Some(package) = suite
                .get("package-name")
                .and_then(serde_json::Value::as_str)
            else {
                return Err(GateError::tool(
                    "cargo nextest list".into(),
                    "a suite record carries no `package-name`".into(),
                ));
            };
            let Some(kind) = suite.get("kind").and_then(serde_json::Value::as_str)
            else {
                return Err(GateError::tool(
                    "cargo nextest list".into(),
                    "a suite record carries no `kind`".into(),
                ));
            };
            let target = suite
                .get("binary-name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(package);
            let label = suite
                .get("binary-id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(target);
            let Some(testcases) = suite
                .get("testcases")
                .and_then(serde_json::Value::as_object)
            else {
                continue;
            };
            for name in testcases.keys() {
                let alias = alias_for(
                    is_integration(TargetKind::from(kind)),
                    TargetName::from(target),
                    TestAlias::from(name),
                );
                catalog.insert(
                    PackageName::from(package),
                    TestAlias::from(&alias),
                    TargetLabel::from(label),
                );
            }
        }
        Ok(catalog)
    }
}

/// Whether a target kind names an integration-test target.
///
/// # Specification
/// - requires: `kind` is a Cargo target kind as a listing reports it.
/// - ensures: answers affirmatively for `test` alone. A library, a binary and a
///   benchmark all report their tests by module path from their own root, so
///   only the integration kind takes a target prefix.
/// - provides: the alias-shape decision for every listed test.
/// - panics: none.
#[inline]
#[must_use]
pub fn is_integration(kind: TargetKind<'_>) -> IntegrationTarget
{
    IntegrationTarget(kind.0 == "test")
}

/// Build the alias one listed test contributes.
///
/// # Specification
/// - requires: `name` is the test path the harness reports.
/// - ensures: prefixes the target name for an integration target and returns
///   the reported path unchanged otherwise.
/// - provides: the exact string a `- witness:` bullet must spell.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the fixture separates the library case from both the
///   named and the consolidated integration cases and asserts the exact alias.
/// - witness: `gates::catalog::a_nextest_listing_indexes_by_package_and_target`
#[inline]
#[must_use]
pub fn alias_for(
    integration: IntegrationTarget,
    target: TargetName<'_>,
    name: TestAlias<'_>,
) -> String
{
    if integration.0 {
        return format!("{}::{}", target.0, name.0);
    }
    name.0.to_owned()
}

/// The final `::`-separated segment of a path.
///
/// # Specification
/// - ensures: returns the text after the last `::`, and the whole path when it
///   holds none.
/// - panics: none.
fn leaf_segment(path: TestAlias<'_>) -> TestAlias<'_>
{
    TestAlias::from(path.0.rsplit("::").next().unwrap_or(path.0))
}
