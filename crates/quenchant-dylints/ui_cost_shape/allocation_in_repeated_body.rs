// Refused: an allocation made in a repeated body and dropped before the next
// pass, one function per allocating form.
#![allow(dead_code, unused_variables)]

use std::collections::BTreeSet;

// Refused: a buffer made by `Vec::new` and grown, dropped each pass.
fn grown_buffer(lines: &[&str]) -> usize
{
    let mut total = 0;
    for line in lines {
        let mut words = Vec::new();
        words.extend(line.split(' '));
        total += words.len();
    }
    total
}

// Refused: a `with_capacity` buffer in a `while` body.
fn reserved_buffer(mut count: u32) -> usize
{
    let mut total = 0;
    while count > 0 {
        let mut digits: Vec<u32> = Vec::with_capacity(8);
        digits.push(count % 10);
        total += digits.len();
        count /= 10;
    }
    total
}

// Refused: `vec![..]` with elements, read and dropped.
fn listed(values: &[u32]) -> u32
{
    let mut total = 0;
    for value in values {
        let pair = vec![*value, value + 1];
        total += pair.iter().sum::<u32>();
    }
    total
}

// Refused: `format!` and `to_string()` borrowed once each pass.
fn formatted(values: &[u32]) -> usize
{
    let mut total = 0;
    for value in values {
        let label = format!("v{value}");
        total += label.len();
        total += value.to_string().len();
    }
    total
}

// Refused: `Box::new` of a sized value inside a `loop`.
fn boxed(limit: u32) -> u32
{
    let mut seen = 0;
    loop {
        let held = Box::new(seen);
        if *held > limit {
            break;
        }
        seen += 1;
    }
    seen
}

// Refused: a `collect()` into a set, searched and dropped in a per-element
// closure.
fn collected(groups: &[Vec<u32>]) -> usize
{
    groups
        .iter()
        .filter(|group| {
            let unique: BTreeSet<u32> = group.iter().copied().collect();
            unique.contains(&0)
        })
        .count()
}

// Refused by `collect_read_once` alone: this lint leaves it that lint's.
fn read_once(groups: &[Vec<u32>]) -> u32
{
    let mut total = 0;
    for group in groups {
        let doubled: Vec<u32> = group.iter().map(|value| value * 2).collect();
        for value in doubled {
            total += value;
        }
    }
    total
}

fn main()
{
}
