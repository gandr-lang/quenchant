// compile-flags: --test
#![allow(dead_code)]

/// A block stating the item's clauses.
///
/// # Specification
/// - requires: nothing of the caller.
/// - ensures: nothing observable.
/// - panics: none.
fn clause_block()
{
}

/// An item with nothing to specify, in the marker's canonical form.
///
/// # Specification
/// trivial.
fn marker_block()
{
}

/// The period is the prose wall's, not the marker's, so the bare word is the
/// marker too.
///
/// # Specification
/// trivial
fn unpunctuated_marker_block()
{
}

/// An item documented without a specification.
fn documented_without_a_block()
{
}

fn undocumented()
{
}

/// An item claiming there is nothing to specify, beside a clause.
///
/// # Specification
/// trivial.
/// - ensures: nothing observable.
fn marker_beside_a_clause()
{
}

/// An item whose marker is written after the clause it contradicts.
///
/// # Specification
/// - ensures: nothing observable.
/// trivial.
fn clause_before_the_marker()
{
}

/// The marker written as a bullet, which the canon excludes: the word behind a
/// bullet reads as one oddly named clause rather than as a claim about the
/// whole block.
///
/// # Specification
/// - trivial
fn marker_written_as_a_bullet()
{
}

/// A `const fn` carrying its block.
///
/// # Specification
/// trivial.
const fn constant_with_a_block()
{
}

/// A `const fn` is authored code like any other.
const fn constant_without_a_block()
{
}

/// The closure this body builds is covered by this block.
///
/// # Specification
/// trivial.
fn holds_a_closure()
{
    let inner = || {};
    inner();
}

/// A wrapper whose derived implementations are nobody's prose to write.
#[repr(transparent)]
#[derive(Clone, Copy)]
struct Wrapped(u8);

impl Wrapped
{
    /// An inherent method carrying its block.
    ///
    /// # Specification
    /// trivial.
    fn inherent_with_a_block(&self)
    {
    }

    /// An inherent method without one.
    fn inherent_without_a_block(&self)
    {
    }
}

/// A trait whose declarations are the author's own prose.
trait Seam
{
    /// A required method carrying its block.
    ///
    /// # Specification
    /// trivial.
    fn required_with_a_block(&self);

    /// A required method without one.
    fn required_without_a_block(&self);

    /// A provided method carrying its block.
    ///
    /// # Specification
    /// trivial.
    fn provided_with_a_block(&self)
    {
    }

    /// A provided method without one.
    fn provided_without_a_block(&self)
    {
    }
}

impl Seam for Wrapped
{
    /// A trait impl method carrying its block.
    ///
    /// # Specification
    /// trivial.
    fn required_with_a_block(&self)
    {
    }

    /// A trait impl method without one.
    fn required_without_a_block(&self)
    {
    }
}

/// A trait an expansion is allowed to implement on this author's behalf.
trait Marked
{
    /// The one method of the marker trait.
    ///
    /// # Specification
    /// trivial.
    fn marked(&self);
}

#[automatically_derived]
impl Marked for Wrapped
{
    fn marked(&self)
    {
    }
}

unsafe extern "C" {
    /// A foreign declaration carrying its block.
    ///
    /// # Specification
    /// trivial.
    fn foreign_with_a_block();

    /// A foreign declaration without one.
    fn foreign_without_a_block();
}

/// Expand a function whose block this macro's author wrote once.
macro_rules! documented_expansion {
    ($name:ident) => {
        /// A function a crate-local macro documents.
        ///
        /// # Specification
        /// trivial.
        fn $name()
        {
        }
    };
}

/// Expand a function whose block this macro's author never wrote.
macro_rules! undocumented_expansion {
    ($name:ident) => {
        fn $name()
        {
        }
    };
}

documented_expansion!(from_a_documented_local_macro);
undocumented_expansion!(from_a_local_macro);

#[cfg(test)]
mod tests
{
    /// A `#[test]` function states its subject in its name and its specification in
    /// its assertions.
    #[test]
    fn a_test_carries_no_block()
    {
        assert!(
            super::Wrapped(0).0 == 0,
            "the wrapper holds what it was given"
        );
    }

    /// A helper beside the tests, carrying its block.
    ///
    /// # Specification
    /// trivial.
    fn helper_with_a_block()
    {
    }

    /// A helper beside the tests is authored code under the rule.
    fn helper_without_a_block()
    {
    }
}

/// A raw identifier the author wrote, with no block.
fn r#type()
{
}
