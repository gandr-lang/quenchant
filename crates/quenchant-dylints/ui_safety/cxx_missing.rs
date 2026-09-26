// compile-flags: --test
#![allow(dead_code)]

#[cxx::bridge]
mod ffi
{
    unsafe extern "C++" {
        fn cpp_call();
    }
}
