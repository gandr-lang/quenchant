// Refused: a vector collected from an iterator only to be read once, one
// function per read, directly and through a `let`.
#![allow(dead_code, unused_variables)]

use std::collections::VecDeque;

fn emit(_: u32)
{
}

// Refused: a `for` loop over the collection itself, directly.
fn for_direct(values: &[u32])
{
    for value in values.iter().map(|value| value + 1).collect::<Vec<u32>>() {
        emit(value);
    }
}

// Refused: a `for` loop over a borrow of a `let` binding with a copying
// pattern.
fn for_borrowed(values: &[u32])
{
    let doubled: Vec<u32> = values.iter().map(|value| value * 2).collect();
    for &value in &doubled {
        emit(value);
    }
}

// Refused: `iter().copied()` on a `let` binding.
fn iter_binding(values: &[u32]) -> u32
{
    let shifted: Vec<u32> = values.iter().map(|value| value << 1).collect();
    shifted.iter().copied().sum()
}

// Refused: `first()` on a `let` binding.
fn first_binding(values: &[u32]) -> Option<u32>
{
    let large: Vec<u32> = values.iter().copied().filter(|value| *value > 9).collect();
    large.first().copied()
}

// Refused: `last()` on a `let` binding.
fn last_binding(values: &[u32]) -> Option<u32>
{
    let small: Vec<u32> = values.iter().copied().filter(|value| *value < 9).collect();
    small.last().copied()
}

// Refused: `get(i)` directly.
fn get_direct(values: &[u32]) -> Option<u32>
{
    values
        .iter()
        .copied()
        .filter(|value| *value > 1)
        .collect::<Vec<u32>>()
        .get(2)
        .copied()
}

// Refused: a direct `into_iter()`.
fn into_iter_direct(values: &[u32]) -> u32
{
    values
        .iter()
        .map(|value| value + 3)
        .collect::<Vec<u32>>()
        .into_iter()
        .sum()
}

// Refused: a `for` loop over a deque binding.
fn deque_binding(values: &[u32])
{
    let queue: VecDeque<u32> = values.iter().copied().collect();
    for value in queue {
        emit(value);
    }
}

fn main()
{
}
