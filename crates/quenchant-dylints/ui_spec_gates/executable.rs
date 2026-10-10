// The executable-predicate gate, opted into at the crate root.
#![cfg_attr(dylint_lib = "quenchant_dylints", deny(spec_attribute_present))]
#![allow(dead_code, unreachable_patterns)]

use anodized::spec;

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
struct Count(u32);

impl Count
{
    const ZERO: Self = Self(0);
}

/// Body-level refusal.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Refused;

/// A precondition stated as a predicate.
///
/// # Specification
/// - requires: `count` is positive.
/// - panics: none.
#[spec(requires: count > Count::ZERO)]
fn precondition(count: Count) -> Count
{
    count
}

/// A postcondition stated through the output closure.
///
/// # Specification
/// - ensures: returns a positive count.
/// - panics: none.
#[spec(ensures: |output| output > Count::ZERO)]
fn postcondition(count: Count) -> Count
{
    count
}

/// An invariant under the qualified path.
///
/// # Specification
/// - requires: `count` is positive.
/// - ensures: it stays positive.
/// - panics: none.
#[anodized::spec(maintains: count > Count::ZERO)]
fn invariant(count: Count) -> Count
{
    count
}

/// A predicate applied through `cfg_attr`.
///
/// # Specification
/// - requires: `count` is positive.
/// - panics: none.
#[cfg_attr(debug_assertions, spec(requires: count > Count::ZERO))]
fn conditional(count: Count) -> Count
{
    count
}

/// Two distinct predicates under one clause, written as a list.
///
/// # Specification
/// - requires: `count` is positive and below ten.
/// - panics: none.
#[spec(requires: [count > Count::ZERO, count < Count(10)])]
fn listed(count: Count) -> Count
{
    count
}

/// One discriminant query is a substantive predicate.
///
/// # Specification
/// - ensures: succeeds.
/// - panics: none.
#[spec(ensures: |output| output.is_ok())]
fn single_query(count: Count) -> Result<Count, Refused>
{
    Ok(count)
}

/// A refutable pattern is a substantive predicate.
///
/// # Specification
/// - ensures: succeeds.
/// - panics: none.
#[spec(ensures: |output| matches!(output, Ok(_)))]
fn refutable_pattern(count: Count) -> Result<Count, Refused>
{
    Ok(count)
}

/// Nothing to state, and no attribute owed.
///
/// # Specification
/// trivial.
fn trivial_block(count: Count) -> Count
{
    count
}

/// An obligation no runtime predicate expresses, exempted in the block.
///
/// # Specification
/// - ensures: returns `count` unchanged.
/// - panics: none.
/// - executable: none — the output is the input, so a predicate would restate
///   the body.
fn exempted(count: Count) -> Count
{
    count
}

/// The exemption may be followed by the intension alone.
///
/// # Specification
/// - ensures: returns `count` unchanged.
/// - panics: none.
/// - executable: none — the promise is about cost, which no predicate observes.
/// - intension: performs no allocation.
fn exempted_before_intension(count: Count) -> Count
{
    count
}

/// A specified trait whose declarations carry nested markers.
#[spec]
trait Counter
{
    /// A nested marker states the declaration's predicate.
    ///
    /// # Specification
    /// - requires: `count` is positive.
    /// - panics: none.
    #[spec(requires: count > Count::ZERO)]
    fn advanced_by(
        &self,
        count: Count,
    ) -> Count;

    /// A declaration whose clauses carry no marker.
    ///
    /// # Specification
    /// - provides: the current count.
    /// - panics: none.
    fn current(&self) -> Count;
}

/// Clauses with neither an attribute nor an exemption.
///
/// # Specification
/// - requires: `count` is positive.
/// - panics: none.
fn unchecked(count: Count) -> Count
{
    count
}

/// An attribute stating no predicate at all.
///
/// # Specification
/// - requires: `count` is positive.
/// - panics: none.
#[spec]
fn bare_attribute(count: Count) -> Count
{
    count
}

/// A literal truth.
///
/// # Specification
/// - requires: `count` is positive.
/// - panics: none.
#[spec(requires: true)]
fn literal_truth(count: Count) -> Count
{
    count
}

/// A literal truth behind the output closure.
///
/// # Specification
/// - ensures: returns a positive count.
/// - panics: none.
#[spec(ensures: |_output| (true))]
fn closure_truth(count: Count) -> Count
{
    count
}

/// An expression compared with itself.
///
/// # Specification
/// - requires: `count` is positive.
/// - panics: none.
#[spec(requires: count == count)]
fn reflexive(count: Count) -> Count
{
    count
}

/// A pattern every value matches.
///
/// # Specification
/// - ensures: returns a positive count.
/// - panics: none.
#[spec(ensures: |output| matches!(output, _))]
fn wildcard(count: Count) -> Count
{
    count
}

/// Both arms of the output's type.
///
/// # Specification
/// - ensures: succeeds.
/// - panics: none.
#[spec(ensures: |output| matches!(output, Ok(_) | Err(_)))]
fn both_arms(count: Count) -> Result<Count, Refused>
{
    Ok(count)
}

/// Both discriminant queries of the output.
///
/// # Specification
/// - ensures: succeeds.
/// - panics: none.
#[spec(ensures: |output| output.is_ok() || output.is_err())]
fn both_queries(count: Count) -> Result<Count, Refused>
{
    Ok(count)
}

/// One predicate written twice.
///
/// # Specification
/// - requires: `count` is positive.
/// - panics: none.
#[spec(requires: [count > Count::ZERO, count > Count::ZERO])]
fn repeated(count: Count) -> Count
{
    count
}

/// An exemption with no reason.
///
/// # Specification
/// - ensures: returns `count` unchanged.
/// - panics: none.
/// - executable: none
fn unreasoned(count: Count) -> Count
{
    count
}

/// An exemption a clause follows.
///
/// # Specification
/// - ensures: returns `count` unchanged.
/// - executable: none — the output is the input.
/// - panics: none.
fn misplaced(count: Count) -> Count
{
    count
}

/// Two exemptions.
///
/// # Specification
/// - ensures: returns `count` unchanged.
/// - panics: none.
/// - executable: none — the output is the input.
/// - executable: none — and it says so twice.
fn twice_exempted(count: Count) -> Count
{
    count
}

/// An exemption a predicate contradicts.
///
/// # Specification
/// - requires: `count` is positive.
/// - panics: none.
/// - executable: none — no predicate expresses this, it says.
#[spec(requires: count > Count::ZERO)]
fn contradicted(count: Count) -> Count
{
    count
}

/// The fixture's entry point.
///
/// # Specification
/// trivial.
fn main()
{
}
