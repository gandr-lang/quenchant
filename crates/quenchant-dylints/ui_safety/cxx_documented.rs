// compile-flags: --test
#![allow(dead_code)]

/// # Safety
/// - unsafe invariants: the C++ function honors the declared ABI and does not
///   unwind.
#[cxx::bridge]
mod ffi
{
    // SAFETY: the module's safety section states the C++ boundary invariants.
    unsafe extern "C++" {
        fn cpp_call();
    }
}
