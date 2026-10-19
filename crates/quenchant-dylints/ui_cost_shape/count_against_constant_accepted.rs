// Accepted: counts that are needed whole, and counts of iterators that know
// their length.
#![allow(dead_code, unused_variables)]

use anodized::spec;

// Accepted: an exact-size iterator knows its length.
fn exact_size(values: &[u32]) -> bool
{
    values.iter().map(|value| value + 1).count() == 3
}

// Accepted: a `take` with a constant limit already bounds the walk.
fn already_bounded(
    values: &[u32],
    key: u32,
) -> bool
{
    values.iter().filter(|value| **value == key).take(2).count() == 1
}

// Accepted: a count against a runtime value needs the whole count.
fn runtime_bound(
    text: &str,
    other: &[u32],
) -> bool
{
    text.chars().count() == other.len()
}

// Accepted: a count used as a number.
fn as_number(text: &str) -> usize
{
    let width = text.chars().count();
    text.chars().count() + width
}

// Accepted: a specification clause states its relation; its cost is
// `spec_clause_traversal`'s to police.
#[spec(ensures: |output| output == (values.iter().filter(|value| **value == 0).count() == 1))]
fn single_zero(values: &[u32]) -> bool
{
    values.iter().filter(|value| **value == 0).take(2).count() == 1
}

fn main()
{
}
