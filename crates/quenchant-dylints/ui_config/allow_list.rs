#![allow(dead_code, specification_present)]

use std::ptr::NonNull;

// Accepted: `NonNull` carries a justified allow-list entry, so its argument
// forms no owning edge and the cycle through it is not a cycle.

#[repr(transparent)]
struct RawLinked(Option<NonNull<RawLinked>>);

// Denied: the allow-list entry naming `Vec` states no justification, so it is
// refused and `Vec` stays conservatively owning. A malformed allow-list denies
// rather than admits.

#[repr(transparent)]
struct VecSelf
{
    children: Vec<VecSelf>,
}

fn main()
{
}
