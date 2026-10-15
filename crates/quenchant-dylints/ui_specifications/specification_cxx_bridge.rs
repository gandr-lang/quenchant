// The presence rule asks every function a `cxx` bridge declares for its block,
// whatever its return type: `cxx` writes out the whole signature of a function
// returning `Result`, but the declaration is still the author's (#27).
#![allow(dead_code)]

/// The bridge whose two functions are both the author's.
///
/// # Safety
/// - unsafe invariants: the C++ side defines both functions with these
///   signatures.
#[cxx::bridge]
mod ffi
{
    /// A pair the bridge shares.
    struct Pair
    {
        first: u32,
        second: u32,
    }

    unsafe extern "C++" {
        fn pairs() -> Vec<Pair>;
        fn fallible() -> Result<Pair>;
    }
}

/// The fixture's entry point.
///
/// # Specification
/// trivial.
fn main() {}
