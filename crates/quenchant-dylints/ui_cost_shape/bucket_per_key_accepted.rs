// Accepted: maps whose buckets are groups, are filled in bulk, hold an entry
// inline, or are read where this crate does not look. Each map would be
// refused but for the one use its function names.
#![crate_type = "lib"]
#![allow(dead_code, unused_variables)]

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;

// Accepted: the bucket is iterated whole, a grouping.
fn grouped(edges: &[(u32, u32)]) -> u32
{
    let mut referrers: HashMap<u32, Vec<u32>> = HashMap::new();
    for &(from, to) in edges {
        referrers.entry(to).or_default().push(from);
    }
    let mut total = 0;
    if let Some(group) = referrers.get(&0) {
        for from in group {
            total += from;
        }
    }
    total
}

// Accepted: a bucket's length is read.
fn counted(edges: &[(u32, u32)]) -> usize
{
    let mut referrers: HashMap<u32, Vec<u32>> = HashMap::new();
    for &(from, to) in edges {
        referrers.entry(to).or_default().push(from);
    }
    referrers.get(&0).map_or(0, |group| group.len())
}

// Accepted: the map is iterated.
fn iterated(edges: &[(u32, u32)]) -> usize
{
    let mut referrers: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for &(from, to) in edges {
        referrers.entry(to).or_default().push(from);
    }
    referrers
        .values()
        .filter(|group| group.contains(&0))
        .count()
}

// Accepted: a bucket is handed to a function.
fn handed_on(edges: &[(u32, u32)]) -> u32
{
    let mut referrers: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for &(from, to) in edges {
        referrers.entry(to).or_default().push(from);
    }
    let bucket = referrers.get(&0);
    summed(bucket)
}

fn summed(bucket: Option<&Vec<u32>>) -> u32
{
    bucket.map_or(0, |group| group.iter().sum())
}

// Accepted: one key's bucket is extended in bulk.
fn extended(
    edges: &[(u32, u32)],
    extra: &[u32],
) -> Option<usize>
{
    let mut referrers: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for &(from, to) in edges {
        referrers.entry(to).or_default().push(from);
    }
    referrers.entry(0).or_default().extend(extra);
    referrers.get(&0)?.iter().position(|&from| from == 1)
}

// Accepted: a whole bucket is inserted.
fn inserted(edges: &[(u32, u32)]) -> Option<usize>
{
    let mut referrers: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for &(from, to) in edges {
        referrers.entry(to).or_default().push(from);
    }
    referrers.insert(1, vec![2, 3]);
    referrers.get(&0)?.iter().position(|&from| from == 1)
}

// Accepted: an entry seeded with its first element is not an empty default.
fn seeded(edges: &[(u32, u32)]) -> Option<usize>
{
    let mut referrers: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for &(from, to) in edges {
        referrers.entry(to).or_insert(vec![from]).push(from);
    }
    referrers.get(&0)?.iter().position(|&from| from == 1)
}

// Accepted: the entry is bound to a local and filled through it.
fn bound_entry(edges: &[(u32, u32)]) -> Option<usize>
{
    let mut referrers: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for &(from, to) in edges {
        let bucket = referrers.entry(to).or_default();
        bucket.push(from);
    }
    referrers.get(&0)?.iter().position(|&from| from == 1)
}

// Accepted: the bucket is a set, not a growable sequence.
fn set_bucket(edges: &[(u32, u32)]) -> Option<bool>
{
    let mut referrers: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
    for &(from, to) in edges {
        referrers.entry(to).or_default().insert(from);
    }
    Some(referrers.get(&0)?.iter().any(|&from| from == 1))
}

// Accepted: the map arrives by reference; its other reads are the caller's.
fn borrowed(
    referrers: &mut BTreeMap<u32, Vec<u32>>,
    edges: &[(u32, u32)],
)
{
    for &(from, to) in edges {
        referrers.entry(to).or_default().push(from);
    }
}

// Accepted: the field is exported, so other crates may read its buckets.
pub struct Shared
{
    pub buckets: BTreeMap<u64, Vec<u32>>,
}

impl Shared
{
    pub fn remember(
        &mut self,
        digest: u64,
        value: u32,
    )
    {
        self.buckets.entry(digest).or_default().push(value);
    }
}

// Accepted: a struct pattern binds the field, whose reads go out of sight.
struct Destructured
{
    buckets: BTreeMap<u64, Vec<u32>>,
}

impl Destructured
{
    fn remember(
        &mut self,
        digest: u64,
        value: u32,
    )
    {
        self.buckets.entry(digest).or_default().push(value);
    }

    fn taken(self) -> BTreeMap<u64, Vec<u32>>
    {
        let Self { buckets } = self;
        buckets
    }
}
