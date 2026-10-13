//! The items executable and prose clauses name, and one reached only through a
//! call.
#![allow(
    dead_code,
    primitive_signature,
    single_field_struct_needs_transparent_repr
)]

use anodized::spec;

/// The bound a reading stays under: an executable dependency of [`read`].
pub const LIMIT: u32 = 8;

/// The margin [`read`]'s precondition links: its prose dependency.
pub const MARGIN: u32 = 3;

/// Whether a value is within the limit: an executable dependency of [`read`].
///
/// # Specification
/// trivial.
#[inline]
#[must_use]
pub fn within(value: u32) -> bool
{
    value <= LIMIT && nonzero(value)
}

/// Whether a value is nonzero: reached from [`read`] only through
/// [`within`]'s body.
///
/// # Specification
/// trivial.
pub fn nonzero(value: u32) -> bool
{
    value != 0
}

/// Read a value; the summary mentions [`nonzero`] without a claim.
///
/// # Specification
/// - requires: `value` is within [`MARGIN`] of the limit.
/// - ensures: returns `value`.
///
/// # Adequacy
/// - hypothesis: L3 — a reading at [`LIMIT`] is returned unchanged.
/// - witness: `tests::reads`
#[spec(requires: within(value) && value <= LIMIT, ensures: |output| output == value)]
pub fn read(value: u32) -> u32
{
    if nonzero(value) { value } else { 0 }
}

#[repr(transparent)]
/// A sample.
pub struct Reading
{
    /// The sampled value.
    pub value: u32,
}

#[non_exhaustive]
/// What a reading classifies as.
pub enum Class
{
    /// At or under the limit.
    Low,
    /// Over the limit, with the value.
    High(u32),
}

/// A scale applied through a trait.
pub trait Scaled
{
    /// The scaled value.
    ///
    /// # Specification
    /// trivial.
    fn scaled(&self) -> u32;
}

impl Scaled for Reading
{
    /// Twice the value.
    ///
    /// # Specification
    /// trivial.
    fn scaled(&self) -> u32
    {
        self.value.saturating_mul(2)
    }
}

impl Reading
{
    /// The class of this reading.
    ///
    /// # Specification
    /// trivial.
    pub fn class(&self) -> Class
    {
        if within(self.value) {
            Class::Low
        }
        else {
            Class::High(self.value)
        }
    }
}

/// Classify a reading.
///
/// # Specification
/// - requires: scaling does not shrink the reading.
/// - ensures: the class is low or high.
///
/// # Adequacy
/// - hypothesis: L3 — a low and a high reading classify apart.
/// - witness: `tests::classifies`
#[spec(
    requires: reading.scaled() >= reading.value && reading.value <= reading.scaled(),
    ensures: |output| matches!(output, Class::Low | Class::High(_)),
)]
pub fn classify(reading: &Reading) -> Class
{
    reading.class()
}

/// Whether a reading is low.
///
/// # Specification
/// - ensures: agrees with the reading's class.
///
/// # Adequacy
/// - hypothesis: L3 — a low reading is low.
/// - witness: `tests::is_low`
#[cfg_attr(all(), spec(ensures: |output| output == matches!(reading.class(), Class::Low)))]
pub fn is_low(reading: &Reading) -> bool
{
    nonzero(reading.value) && matches!(reading.class(), Class::Low)
}

/// The fixture compiles as a program.
///
/// # Specification
/// trivial.
fn main()
{
}
