// compile-flags: --test
#![allow(dead_code)]

#[repr(transparent)]
struct Count(u32);

#[cxx::bridge]
mod ffi
{
    struct Pair
    {
        first: u32,
        second: u32,
    }

    unsafe extern "C++" {
        type Opaque;

        fn pairs() -> Vec<Pair>;
        fn shared() -> SharedPtr<Opaque>;
        fn count(value: i32) -> u64;
        fn fallible(value: i32) -> Result<u64>;
    }
}

fn authored(value: i32) -> Count
{
    Count(0)
}
