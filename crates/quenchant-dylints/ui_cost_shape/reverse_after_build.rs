// Refused: a vector reversed right after the statement that built it from a
// source that reads backward, one function per source form.
#![allow(dead_code, unused_variables)]

// Refused: a mapped slice iterator collected, then reversed.
fn collected(values: &[u32]) -> Vec<u32>
{
    let mut doubled: Vec<u32> = values.iter().map(|value| value * 2).collect();
    doubled.reverse();
    doubled
}

// Refused: a slice copied, then reversed.
fn copied(values: &[u32]) -> Vec<u32>
{
    let mut copy = values.to_vec();
    copy.reverse();
    copy
}

// Refused: a vector cloned, then reversed.
fn cloned(values: &Vec<u32>) -> Vec<u32>
{
    let mut copy = values.clone();
    copy.reverse();
    copy
}

// Refused: a tail split off, then reversed.
fn split(
    values: &mut Vec<u32>,
    at: usize,
) -> Vec<u32>
{
    let mut tail = values.split_off(at);
    tail.reverse();
    tail
}

// Refused: an `extend` followed by the reversal of the suffix it appended,
// selected by indexing.
fn extended_index(
    out: &mut Vec<u32>,
    values: &[u32],
)
{
    let start = out.len();
    out.extend(values.iter().map(|value| value + 1));
    out[start ..].reverse();
}

// Refused: the same suffix selected by `get_mut` under `if let`.
fn extended_if_let(
    out: &mut Vec<u32>,
    values: &[u32],
)
{
    let start = out.len();
    out.extend(values.iter().copied());
    if let Some(suffix) = out.get_mut(start ..) {
        suffix.reverse();
    }
}

// Refused: the same suffix selected by `get_mut` under `ok_or` and `?`.
fn extended_try(
    out: &mut Vec<u32>,
    values: &[u32],
) -> Result<(), ()>
{
    let start = out.len();
    out.extend(values);
    out.get_mut(start ..).ok_or(())?.reverse();
    Ok(())
}

fn main()
{
}
