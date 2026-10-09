// The crate opts in at its root; a module not yet brought up allows both gates
// with the same `cfg_attr` form, and only the module is excused.
#![cfg_attr(
    dylint_lib = "quenchant_dylints",
    deny(spec_attribute_present, adequacy_present)
)]
#![allow(dead_code)]

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
struct Count(u32);

/// Clauses with neither an attribute nor a section, under the denial.
///
/// # Specification
/// - requires: `count` is positive.
/// - panics: none.
fn denied(count: Count) -> Count
{
    count
}

/// A module whose items are not yet brought up.
mod legacy
{
    #![cfg_attr(
        dylint_lib = "quenchant_dylints",
        allow(spec_attribute_present, adequacy_present)
    )]

    use super::Count;

    /// Clauses with neither an attribute nor a section, under the allowance.
    ///
    /// # Specification
    /// - requires: `count` is positive.
    /// - panics: none.
    pub fn allowed(count: Count) -> Count
    {
        count
    }
}

/// The fixture's entry point.
///
/// # Specification
/// trivial.
fn main()
{
}
