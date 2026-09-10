// The presence rule reads the author's own declarations through a specified
// trait definition, and not the sibling that expansion synthesizes beneath the
// author's name.
#![allow(dead_code)]

use quenchant::spec;

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, PartialOrd)]
struct Count(u32);

impl Count {
    const ZERO: Self = Self(0);
}

/// A specified trait whose declarations carry their own rustdoc.
#[spec]
trait Counter {
    /// An unspecified declaration, documented like any other.
    ///
    /// # Specification
    /// - provides: the count the specified declarations are read against.
    /// - panics: none.
    fn count(&self) -> Count;

    /// A specified declaration carrying the rustdoc block as well.
    ///
    /// # Specification
    /// - requires: `count` is positive.
    /// - ensures: returns a positive count.
    /// - panics: enforcing builds panic on a non-positive output.
    #[spec(requires: count > Count::ZERO, ensures: |output| output > Count::ZERO)]
    fn advanced_by(&self, count: Count) -> Count;

    /// A specified declaration whose predicates are not the rustdoc block.
    #[spec(requires: count > Count::ZERO)]
    fn retreated_by(&self, count: Count) -> Count;
}

/// The fixture's entry point.
///
/// # Specification
/// trivial.
fn main() {}
