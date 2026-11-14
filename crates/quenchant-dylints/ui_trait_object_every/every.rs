//! The class `*` takes every trait object, auto traits alone too.

use std::fmt::Display;

// Each trait object: refused.

fn shown(value: &dyn Display) -> String
{
    value.to_string()
}

fn sent(value: Box<dyn Send>) -> usize
{
    drop(value);
    0
}

// A site that keeps its trait object and says why: accepted.

#[expect(
    trait_object,
    reason = "the boundary that the fixture does not compile"
)]
fn kept(value: &dyn Display) -> String
{
    value.to_string()
}

fn main()
{
    drop(shown(&1));
    let _count = sent(Box::new(1));
    drop(kept(&1));
}
