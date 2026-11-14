//! A consumer's list of crates under one configuration key. Each entry is a
//! crate's whole name, or the start of one with a `*` after it. The rules that
//! a consumer turns on crate by crate read their crates through this list:
//! `allocation_default_crates`, `repeated_call_crates` and
//! `std_container_crates`.

use anodized::spec;

use crate::answer_totality::CONFIGURATION_TABLE;
use crate::semantic::CrateName;
use crate::semantic::ListedCrate;

/// The configuration key under which a rule reads its crates.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct ListKey(pub &'static str);

/// One entry of a list of crates.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CratePattern
{
    /// A crate's whole name.
    Exact(String),
    /// The start of a crate's name, written with a `*` after it.
    Prefix(String),
}

impl CratePattern
{
    /// Whether the pattern takes a crate of name `name`.
    ///
    /// # Specification
    /// - ensures: an exact pattern takes its own name alone; a prefix takes
    ///   each name that starts with it.
    /// - panics: none.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the unit tests take a name whole and by prefix, and
    ///   refuse a longer name and another prefix.
    /// - witness: `crate_list::tests::a_pattern_takes_its_crates`
    #[spec(ensures: |output| !output.0 || !name.0.is_empty() || matches!(self, Self::Exact(whole) if whole.is_empty()))]
    pub fn matches(
        &self,
        name: CrateName<'_>,
    ) -> ListedCrate
    {
        ListedCrate(match *self {
            Self::Exact(ref whole) => name.0 == whole,
            Self::Prefix(ref start) => name.0.starts_with(start.as_str()),
        })
    }
}

/// Read the list of crates under `key`, and push a defect for each entry that
/// names no crate.
///
/// # Specification
/// - requires: `dylint_linting::init_config` has already run, which
///   [`crate::register_lints`] guarantees by calling it first.
/// - ensures: each string entry other than an empty one and `*` becomes a
///   pattern, a prefix when it ends in `*`. A value that is no list, and each
///   other entry, becomes a defect.
/// - fails: an absent, unreadable or malformed configuration, or an absent key,
///   yields no pattern.
/// - panics: none.
/// - executable: none — the configuration read outcome is not retained; reading
///   it again is a distinct fallible operation.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures name their crates whole and by prefix; the
///   refused configurations carry a number, an empty entry and `*`, each
///   reported.
/// - witness: `tests::ui_allocation_default`
/// - witness: `tests::ui_allocation_default_refused`
/// - witness: `tests::ui_repeated_call`
/// - witness: `tests::ui_repeated_call_refused`
/// - witness: `tests::ui_std_container`
/// - witness: `tests::ui_std_container_refused`
pub fn read(
    key: ListKey,
    defects: &mut Vec<String>,
) -> Vec<CratePattern>
{
    let mut crates = Vec::new();
    let Ok(Some(config)) = dylint_linting::config_toml(CONFIGURATION_TABLE)
    else {
        return crates;
    };
    let ListKey(key) = key;
    let Some(value) = config.get(key)
    else {
        return crates;
    };
    let Some(entries) = value.as_array()
    else {
        defects.push(format!("`{key}` is no list of crate names"));
        return crates;
    };
    for entry in entries {
        match entry.as_str() {
            Some(name) if !name.is_empty() && name != "*" => {
                crates.push(name.strip_suffix('*').map_or_else(
                    || CratePattern::Exact(name.to_owned()),
                    |start| CratePattern::Prefix(start.to_owned()),
                ));
            },
            Some(_) | None => defects.push(format!(
                "`{key}` entry `{entry}` is no crate name, nor the start of one followed by `*`"
            )),
        }
    }
    crates
}

#[cfg(test)]
mod tests
{
    use super::CratePattern;
    use crate::semantic::CrateName;

    #[test]
    fn a_pattern_takes_its_crates()
    {
        let whole = CratePattern::Exact("gandr_kernel_core".to_owned());
        let start = CratePattern::Prefix("gandr_kernel".to_owned());
        assert!(whole.matches(CrateName("gandr_kernel_core")).0);
        assert!(!whole.matches(CrateName("gandr_kernel_core_base")).0);
        assert!(start.matches(CrateName("gandr_kernel_core_base")).0);
        assert!(start.matches(CrateName("gandr_kernel")).0);
        assert!(!start.matches(CrateName("gandr_core_term")).0);
    }
}
