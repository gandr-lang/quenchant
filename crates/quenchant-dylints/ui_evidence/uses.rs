//! The items each item's body and signature name, outside every clause.
#![allow(
    dead_code,
    primitive_signature,
    recursion_forbidden,
    single_field_struct_needs_transparent_repr
)]

/// The helpers the root imports.
mod inner
{
    /// A helper the root re-exports.
    ///
    /// # Specification
    /// trivial.
    pub fn helper() -> u32
    {
        1
    }

    /// A helper no public path reaches.
    ///
    /// # Specification
    /// trivial.
    pub(crate) fn hidden() -> u32
    {
        2
    }
}

pub use inner::helper;
use inner::hidden;

#[repr(transparent)]
/// A count.
pub struct Count
{
    /// The counted value.
    pub value: u32,
}

/// A size, empty or not.
pub enum Size
{
    /// Nothing counted.
    Empty,
    /// Something counted.
    Some,
}

/// A measure a generic function bounds its argument by.
pub trait Measured
{
    /// The measure.
    ///
    /// # Specification
    /// trivial.
    fn measure(&self) -> u32;
}

impl Measured for Count
{
    /// The counted value.
    ///
    /// # Specification
    /// trivial.
    fn measure(&self) -> u32
    {
        self.value
    }
}

/// The measure of a generic argument: its bound names the trait.
///
/// # Specification
/// trivial.
pub fn measure_of<T: Measured>(item: &T) -> u32
{
    item.measure()
}

/// Count down: the call to itself is no use, the call to the hidden helper
/// is.
///
/// # Specification
/// trivial.
pub fn countdown(n: u32) -> u32
{
    if n == 0 { hidden() } else { countdown(n - 1) }
}

/// Add the helper's value in a closure: the closure's names are this
/// function's.
///
/// # Specification
/// trivial.
pub fn total(count: &Count) -> u32
{
    let add = |extra: u32| helper() + extra;
    add(count.measure())
}

/// Whether a size is empty: the pattern names the variant.
///
/// # Specification
/// trivial.
pub fn is_empty(size: &Size) -> bool
{
    matches!(size, Size::Empty)
}

/// The fixture compiles as a program.
///
/// # Specification
/// trivial.
fn main()
{
}
