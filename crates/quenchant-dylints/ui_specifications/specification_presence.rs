// The presence rule reads the author's own name through a real expansion.
#![allow(dead_code)]

use quenchant::spec;

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, PartialOrd)]
struct Count(u32);

impl Count {
    const ZERO: Self = Self(0);
}

/// A specified function carrying the rustdoc block as well.
///
/// # Specification
/// - requires: `count` is positive.
/// - ensures: returns `count` unchanged.
/// - panics: enforcing builds panic on a non-positive output.
#[spec(requires: count > Count::ZERO, ensures: |output| output > Count::ZERO)]
fn specified_with_a_block(count: Count) -> Count {
    count
}

/// A specified function whose predicates are not the rustdoc block.
#[spec(requires: count > Count::ZERO)]
fn specified_without_a_block(count: Count) -> Count {
    count
}

/// A qualified attribute expands the same way, and hides no absence either.
#[quenchant::spec(maintains: count > Count::ZERO)]
fn qualified_without_a_block(count: Count) -> Count {
    count
}

/// The fixture's entry point.
///
/// # Specification
/// trivial.
fn main() {}
