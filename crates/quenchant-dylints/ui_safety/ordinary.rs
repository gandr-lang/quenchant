// compile-flags: --test
#![allow(dead_code)]

/// # Safety
/// - unsafe invariants: callers must uphold the function's input contract.
unsafe fn documented()
{
}

/// A missing section is not an invariant.
unsafe fn missing_section()
{
}

/// # Safety
/// The section alone does not state an invariant.
unsafe fn missing_clause()
{
}

/// # Safety
/// - unsafe invariants:
unsafe fn empty_clause()
{
}

/// # Safety
/// An earlier section cannot borrow a later section's bullet.
/// # Specification
/// - unsafe invariants: this belongs to the specification.
unsafe fn misplaced_clause()
{
}

/// # Safety
/// - unsafe invariants: implementations preserve this trait's promise.
unsafe trait DocumentedTrait
{
    /// # Safety
    /// - unsafe invariants: callers must satisfy the method's precondition.
    unsafe fn documented_method();

    unsafe fn missing_method();
}

struct Value;

/// # Safety
/// - unsafe invariants: this implementation preserves the trait's promise.
unsafe impl DocumentedTrait for Value
{
    /// # Safety
    /// - unsafe invariants: this method preserves the trait's promise.
    unsafe fn documented_method()
    {
    }

    /// # Safety
    /// - unsafe invariants: the caller ensures this method's precondition.
    unsafe fn missing_method()
    {
    }
}

unsafe trait MissingTrait
{
}

unsafe impl MissingTrait for Value
{
}

/// # Safety
/// - unsafe invariants: the foreign symbol uses the declared C ABI.
unsafe extern "C" {
    fn documented_foreign();
}

unsafe extern "C" {
    fn undocumented_foreign();
}
