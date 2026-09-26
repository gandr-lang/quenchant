// compile-flags: --test
#![allow(dead_code)]

/// # Safety
/// The section does not give an unsafe invariants clause.
#[cxx::bridge]
mod ffi
{
    unsafe extern "C++" {
        fn cpp_call();
    }
}
