// compile-flags: --test
#![allow(dead_code)]

/// # Safety
/// - unsafe invariants: the C++ function honors the declared ABI.
#[cxx::bridge]
mod ffi
{
    unsafe extern "C++" {
        fn cpp_call();
    }
}

unsafe extern "C" {
    fn unrelated_foreign();
}
