//! A configuration of the followed crates with a number, an empty entry and
//! the bare `*` among them: each is refused at the crate root, and no entry
//! follows the calls of this crate.
#![allow(dead_code)]

/// A fresh vector for each value.
fn widened(value: u32) -> Vec<u32>
{
    vec![value; 4]
}

fn main()
{
    let mut total = 0;
    for value in 0 .. 3 {
        total += widened(value).len();
    }
}
