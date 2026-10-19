// Refused: an iterator counted to its end only to compare the count with a
// constant, one function per comparison shape.
#![allow(dead_code, unused_variables)]

const LIMIT: usize = 3;

// Refused: `chars().count()` against a literal.
fn long_label(label: &str) -> bool
{
    label.chars().count() > 24
}

// Refused: the constant on the left, against a filtered count.
fn none_large(values: &[u32]) -> bool
{
    0 == values.iter().filter(|value| **value > 2).count()
}

// Refused: `lines().count()` against a literal by equality.
fn single_line(text: &str) -> bool
{
    text.lines().count() == 1
}

// Refused: a count against a named constant.
fn shallow(path: &str) -> bool
{
    path.split('/').count() <= LIMIT
}

// Refused: a count against zero by inequality.
fn has_separator(text: &str) -> bool
{
    text.matches(',').count() != 0
}

fn main()
{
}
