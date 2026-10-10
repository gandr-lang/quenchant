// Without the opt-in, both clause-bearing gates stay at their default `allow`,
// so a crate not yet brought up reports nothing.
#![allow(dead_code)]

use anodized::spec;

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
struct Count(u32);

/// Clauses with neither an attribute, an exemption, nor an adequacy section.
///
/// # Specification
/// - requires: `count` is positive.
/// - panics: none.
fn unchecked(count: Count) -> Count
{
    count
}

/// A literal truth, which an opted-in crate refuses.
///
/// # Specification
/// - requires: `count` is positive.
/// - panics: none.
#[spec(requires: true)]
fn literal_truth(count: Count) -> Count
{
    count
}

/// Qualified spelling is also opt-in.
///
/// # Specification
/// trivial.
#[anodized::spec]
fn qualified_default()
{
}

/// The fixture's entry point.
///
/// # Specification
/// trivial.
fn main()
{
}
