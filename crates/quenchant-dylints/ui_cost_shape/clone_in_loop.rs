// Refused: a heap-owning copy made in a repeated body and dropped before the
// next pass, one function per copying method and copied type.
#![allow(dead_code, unused_variables)]

use std::collections::BTreeMap;

struct Index
{
    names: Vec<String>,
    table: BTreeMap<u32, u32>,
}

// Refused: a `Vec<String>` cloned only to be searched, in a `for` body.
fn cloned_vector(
    index: &Index,
    queries: &[String],
) -> usize
{
    let mut hits = 0;
    for query in queries {
        let names = index.names.clone();
        if names.contains(query) {
            hits += 1;
        }
    }
    hits
}

// Refused: a `String` copied by `to_owned()` and borrowed, in a `while` body.
fn owned_text(
    text: &str,
    mut count: u32,
) -> usize
{
    let mut total = 0;
    while count > 0 {
        let copy = text.to_owned();
        total += copy.len();
        count -= 1;
    }
    total
}

// Refused: a slice copied by `to_vec()` and sorted in place, in a closure
// `for_each` calls once per element.
fn sorted_copies(
    values: &[u32],
    rounds: &[u32],
) -> u32
{
    let mut total = 0;
    rounds.iter().for_each(|round| {
        let mut copy = values.to_vec();
        copy.sort_unstable();
        total += copy.first().copied().unwrap_or(*round);
    });
    total
}

// Refused: a struct holding a map, cloned as a temporary.
#[derive(Clone)]
struct Snapshot
{
    table: BTreeMap<u32, u32>,
}

fn snapshot_reads(
    snapshot: &Snapshot,
    keys: &[u32],
) -> usize
{
    let mut found = 0;
    for key in keys {
        if snapshot.clone().table.contains_key(key) {
            found += 1;
        }
    }
    found
}

// Refused: `Option::cloned` of a boxed value, read and dropped.
fn cloned_option(
    held: &[Option<Box<u32>>],
    rounds: u32,
) -> u32
{
    let mut total = 0;
    for _ in 0 .. rounds {
        let first = held.first().cloned();
        if let Some(Some(value)) = &first {
            total += **value;
        }
    }
    total
}

fn main()
{
}
