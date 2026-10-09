// The adequacy presence gate, opted into at the crate root.
#![cfg_attr(dylint_lib = "quenchant_dylints", deny(adequacy_present))]
#![allow(dead_code)]

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
struct Count(u32);

/// Clauses with the section naming their evidence.
///
/// # Specification
/// - requires: `count` is positive.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the smallest positive count is returned exactly.
/// - witness: `tests::the_smallest_count_is_returned`
fn with_section(count: Count) -> Count
{
    count
}

/// Clauses with no section.
///
/// # Specification
/// - requires: `count` is positive.
/// - panics: none.
fn without_section(count: Count) -> Count
{
    count
}

/// Nothing to state, and no section owed.
///
/// # Specification
/// trivial.
fn trivial_block(count: Count) -> Count
{
    count
}

/// A trait whose declarations state clauses.
trait Counter
{
    /// A required method states its declaration-only boundary.
    ///
    /// # Specification
    /// - provides: the current count.
    /// - panics: none.
    ///
    /// # Adequacy
    /// - hypothesis: L0 — the declaration has no body.
    /// - declaration-only: each implementation is witnessed at its own impl.
    fn current(&self) -> Count;

    /// A required method with no section.
    ///
    /// # Specification
    /// - provides: the next count.
    /// - panics: none.
    fn next(&self) -> Count;
}

/// The fixture's entry point.
///
/// # Specification
/// trivial.
fn main()
{
}
