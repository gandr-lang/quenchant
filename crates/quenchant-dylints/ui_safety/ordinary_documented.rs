// compile-flags: --test
#![allow(dead_code)]

/// # Safety
/// - unsafe invariants: callers supply a valid pointer to one initialized byte.
unsafe fn read_byte(pointer: *const u8) -> u8
{
    // SAFETY: the caller supplies an initialized byte at a valid address.
    unsafe { *pointer }
}
