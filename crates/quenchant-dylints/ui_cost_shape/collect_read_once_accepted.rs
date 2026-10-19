// Accepted: collections read more than once, kept, read by borrow, or
// needed to end a borrow or to order an iterator's effects, and reads
// Clippy's `needless_collect` refuses itself.
#![allow(dead_code, unused_variables)]

use std::alloc::Layout;
use std::alloc::alloc;
use std::alloc::dealloc;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::iter::successors;
use std::sync::Mutex;
use std::thread::scope;

fn emit(_: u32)
{
}

// Accepted: a set deduplicates and orders.
fn set_collection(values: &[u32])
{
    let unique: BTreeSet<u32> = values.iter().copied().collect();
    for value in &unique {
        emit(*value);
    }
}

// Accepted: a binding read twice.
fn read_twice(values: &[u32]) -> usize
{
    let kept: Vec<u32> = values.iter().copied().collect();
    for value in &kept {
        emit(*value);
    }
    kept.len()
}

// Accepted: a returned collection is kept.
fn returned(values: &[u32]) -> Vec<u32>
{
    values.iter().map(|value| value + 1).collect()
}

// Accepted: a binding read inside a loop that does not hold it.
fn read_in_loop(
    values: &[u32],
    rounds: u32,
)
{
    let kept: Vec<u32> = values.iter().copied().collect();
    for _ in 0 .. rounds {
        let first = kept.first();
    }
}

// Accepted: the iterator borrows a lock guard the collection outlives.
fn lock_guard(shared: &Mutex<Vec<u32>>)
{
    let snapshot: Vec<u32> = shared.lock().unwrap().iter().copied().collect();
    for value in snapshot {
        emit(value);
    }
}

// Accepted: the loop that reads the collection mutates the iterator's source.
fn source_mutated(mut table: BTreeMap<u32, u32>)
{
    let keys: Vec<u32> = table.keys().copied().collect();
    for key in keys {
        table.remove(&key);
    }
}

// Accepted: a closure that pushes into a captured vector runs at the
// collection.
fn order_observed(
    values: &[u32],
    log: &mut Vec<u32>,
)
{
    let seen: Vec<u32> = values
        .iter()
        .map(|value| {
            log.push(*value);
            *value
        })
        .collect();
    for &value in &seen {
        emit(value);
    }
}

// Accepted: Clippy's `needless_collect` refuses these reads itself.
fn clippy_reads(values: &[u32]) -> usize
{
    let counted: Vec<u32> = values.iter().copied().collect();
    let direct = values.iter().copied().collect::<Vec<u32>>().len();
    let moved: Vec<u32> = values.iter().copied().collect();
    let total: u32 = moved.into_iter().sum();
    counted.len() + direct
}

// Accepted: a reversed read of a source that is not double-ended.
fn single_ended_reversed(start: u32) -> u32
{
    successors(Some(start), |value| value.checked_sub(1))
        .collect::<Vec<u32>>()
        .into_iter()
        .rev()
        .sum()
}

// Accepted: a borrowing read whose borrows outlive it; a borrow of an
// element the source yields could not be stored.
fn borrows_stored(names: &[&str]) -> usize
{
    let upper: Vec<String> = names.iter().map(|name| name.to_uppercase()).collect();
    let mut borrowed: Vec<&str> = Vec::new();
    for name in &upper {
        borrowed.push(name.as_str());
    }
    borrowed.len()
}

// Accepted: `get` with a range takes a slice, which the source cannot give.
fn range_get(values: &[u32]) -> Option<usize>
{
    let large: Vec<u32> = values.iter().copied().filter(|value| *value > 9).collect();
    large.get(1 ..).map(<[u32]>::len)
}

// Accepted: every thread is spawned before any is joined; iterating the
// source would join each before spawning the next.
fn spawned_together(values: &[u32]) -> u32
{
    scope(|threads| {
        let handles: Vec<_> = values
            .chunks(2)
            .map(|chunk| threads.spawn(move || chunk.iter().sum::<u32>()))
            .collect();
        let mut total = 0;
        for handle in handles {
            total += handle.join().unwrap_or(0);
        }
        total
    })
}

// Accepted: every block is allocated before any is freed.
fn allocated_together()
{
    let blocks: Vec<*mut u8> = (0 .. 4_u32)
        .map(|_| unsafe { alloc(Layout::new::<u64>()) })
        .collect();
    for block in blocks {
        unsafe { dealloc(block, Layout::new::<u64>()) };
    }
}

// Accepted: the source is dropped between the collection and the read.
fn source_dropped(table: BTreeMap<u32, u32>)
{
    let keys: Vec<u32> = table.keys().copied().collect();
    drop(table);
    for key in keys {
        emit(key);
    }
}

// Accepted: `enumerate().rev()` needs an exact-size source, and a filtered
// `chars()` is double-ended without being exact-size.
fn enumerated_reversed(text: &str)
{
    let letters: Vec<char> = text
        .chars()
        .filter(|letter| letter.is_alphabetic())
        .collect();
    for (index, letter) in letters.iter().copied().enumerate().rev() {
        emit(u32::from(letter));
    }
}

// Accepted: the source borrows from `self` through two bindings and the loop
// mutates `self`; the collection ends the borrow.
struct Table
{
    rows: Vec<Vec<u32>>,
    log: Vec<u32>,
}

impl Table
{
    fn replay(
        &mut self,
        at: usize,
    )
    {
        let Some(row) = self.rows.get(at)
        else {
            return;
        };
        let picked = row.as_slice();
        let kept: Vec<u32> = picked.iter().copied().filter(|value| *value > 1).collect();
        for value in kept {
            self.record(value);
        }
    }

    fn record(
        &mut self,
        value: u32,
    )
    {
        self.log.push(value);
    }
}

fn main()
{
}
