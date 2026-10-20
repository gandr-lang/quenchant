// Refused: a map whose growable buckets are filled one element per key and
// only ever searched for one entry, one function per spelling.
#![allow(dead_code, unused_variables)]

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::VecDeque;

// The memo shape: a field bucketed by digest, searched through `?` and through
// `if let`, filled through `or_default()`.
struct Memo
{
    buckets: BTreeMap<u64, Vec<(u32, u32)>>,
}

impl Memo
{
    fn recall(
        &self,
        digest: u64,
        key: u32,
    ) -> Option<u32>
    {
        let bucket = self.buckets.get(&digest)?;
        bucket
            .iter()
            .find(|entry| entry.0 == key)
            .map(|entry| entry.1)
    }

    fn remember(
        &mut self,
        digest: u64,
        key: u32,
        value: u32,
    )
    {
        if let Some(bucket) = self.buckets.get_mut(&digest) {
            if let Some(entry) = bucket.iter_mut().find(|entry| entry.0 == key) {
                entry.1 = value;
                return;
            }
        }
        if self.buckets.is_empty() || self.buckets.contains_key(&digest) {
            self.buckets.clear();
        }
        self.buckets.entry(digest).or_default().push((key, value));
    }
}

// A local map of strings filled through `or_insert_with(String::new)` and
// searched through a closure of `is_some_and`.
fn local_strings(words: &[(u64, char)]) -> usize
{
    let mut seen: HashMap<u64, String> = HashMap::new();
    for &(digest, letter) in words {
        seen.entry(digest).or_insert_with(String::new).push(letter);
    }
    words
        .iter()
        .filter(|(digest, letter)| seen.get(digest).is_some_and(|text| text.contains(*letter)))
        .count()
}

// A local deque filled through `or_insert(VecDeque::new())` and a closure
// returning `Vec::new()`, read only by `is_some`.
fn local_deques(pairs: &[(u64, u32)]) -> usize
{
    let mut queues: HashMap<u64, VecDeque<u32>> = HashMap::new();
    let mut lists: HashMap<u64, Vec<u32>> = HashMap::new();
    for &(digest, value) in pairs {
        queues
            .entry(digest)
            .or_insert(VecDeque::new())
            .push_back(value);
        lists
            .entry(digest)
            .or_insert_with(|| Vec::new())
            .push(value);
    }
    pairs
        .iter()
        .filter(|(digest, _)| queues.get(digest).is_some() && lists.get(digest).is_none())
        .count()
}

fn main()
{
}
