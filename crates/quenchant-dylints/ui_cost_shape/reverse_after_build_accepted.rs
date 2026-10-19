// Accepted: reversals with no source to read backward, sources that cannot be
// read backward, and receivers whose length their type fixes.
#![allow(dead_code, unused_variables)]

use std::iter::successors;

struct Node
{
    parent: Option<usize>,
}

// Accepted: an array's reversal costs a constant; a byte-order flip is one.
fn array(value: u64) -> [u8; 8]
{
    let mut bytes = value.to_le_bytes();
    bytes.reverse();
    bytes
}

// Accepted: a vector built by a loop of pushes has no source to reverse.
fn pushed_chain(
    nodes: &[Node],
    mut current: Option<usize>,
) -> Vec<usize>
{
    let mut chain = Vec::new();
    while let Some(index) = current {
        chain.push(index);
        current = nodes.get(index).and_then(|node| node.parent);
    }
    chain.reverse();
    chain
}

// Accepted: a use between the build and the reversal.
fn used_between(values: &[u32]) -> Vec<u32>
{
    let mut built: Vec<u32> = values.iter().copied().collect();
    built.push(0);
    built.reverse();
    built
}

// Accepted: a parameter was built elsewhere.
fn parameter(values: &mut Vec<u32>)
{
    values.reverse();
}

// Accepted: a `successors` chain is not double-ended.
fn single_ended(start: u32) -> Vec<u32>
{
    let mut chain: Vec<u32> = successors(Some(start), |value| value.checked_sub(1)).collect();
    chain.reverse();
    chain
}

// Accepted: a source whose closure pushes into a captured vector observes its
// own order.
fn order_observed(
    values: &[u32],
    log: &mut Vec<u32>,
) -> Vec<u32>
{
    let mut seen: Vec<u32> = values
        .iter()
        .map(|value| {
            log.push(*value);
            *value
        })
        .collect();
    seen.reverse();
    seen
}

fn fill(out: &mut Vec<u32>)
{
    out.push(1);
}

// Accepted: a suffix appended by a callback has no source to reverse.
fn suffix_after_callback(out: &mut Vec<u32>)
{
    let start = out.len();
    fill(out);
    out[start ..].reverse();
}

// Accepted: a suffix appended from a single-ended iterator.
fn suffix_single_ended(
    out: &mut Vec<u32>,
    start_value: u32,
)
{
    let start = out.len();
    out.extend(successors(Some(start_value), |value| value.checked_sub(1)));
    out[start ..].reverse();
}

fn main()
{
}
