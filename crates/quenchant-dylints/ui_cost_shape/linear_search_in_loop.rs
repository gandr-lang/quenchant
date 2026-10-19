// Refused: a linear search of a collection bound outside the loop that repeats
// it, one function per form and repetition.
#![allow(dead_code, unused_variables)]

use std::collections::VecDeque;

struct Entry
{
    name: String,
}

struct Registry
{
    items: Vec<u32>,
}

// Refused: `Vec::contains` in a `for` body.
fn contains_in_for(names: &[String])
{
    let mut seen: Vec<String> = Vec::new();
    for name in names {
        if !seen.contains(name) {
            seen.push(name.clone());
        }
    }
}

// Refused: slice `contains` in a `while let` body, the slice a parameter.
fn contains_in_while(
    known: &[u32],
    mut queue: Vec<u32>,
)
{
    while let Some(next) = queue.pop() {
        if known.contains(&next) {
            continue;
        }
    }
}

// Refused: `contains` in a closure `filter` calls once per element.
fn contains_in_filter(items: &[u32]) -> usize
{
    let excluded = vec![1, 2, 3];
    items.iter().filter(|item| !excluded.contains(item)).count()
}

// Refused: `VecDeque::contains` in a `loop` body.
fn deque_contains_in_loop(
    window: &VecDeque<u32>,
    mut next: u32,
)
{
    loop {
        if !window.contains(&next) {
            break;
        }
        next = next.wrapping_add(1);
    }
}

// Refused: a field of a receiver bound outside the loop.
fn field_contains(
    registry: &Registry,
    candidates: &[u32],
)
{
    for candidate in candidates {
        if registry.items.contains(candidate) {
            return;
        }
    }
}

// Refused: a keyed `position` in a `for` body.
fn keyed_position(
    entries: &[Entry],
    names: &[String],
)
{
    for name in names {
        let found = entries.iter().position(|entry| entry.name == *name);
    }
}

// Refused: a keyed `any` in a closure `retain` calls once per element.
fn keyed_any_in_retain(
    kept: &mut Vec<u32>,
    removed: &[u32],
)
{
    kept.retain(|value| !removed.iter().any(|other| other == value));
}

// Refused: a keyed `find` and a keyed `rposition` in a `for` body, the key on
// the left.
fn keyed_find_and_rposition(
    entries: &[Entry],
    names: &[String],
)
{
    for name in names {
        let first = entries.iter().find(|entry| *name == entry.name);
        let last = entries.iter().rposition(|entry| entry.name == *name);
    }
}

// Refused: a `return` only some passes reach leaves the search repeated by
// the others.
fn conditionally_returned(
    rows: &[u32],
    seen: &[u32],
) -> bool
{
    for row in rows {
        if seen.contains(row) && *row > 9 {
            return true;
        }
    }
    false
}

// Refused: a `continue` skips the `return` after the search, so every pass
// that continues searches again.
fn continued_before_return(
    rows: &[u32],
    seen: &[u32],
) -> Option<u32>
{
    for &row in rows {
        if !seen.contains(&row) {
            continue;
        }
        return Some(row);
    }
    None
}

fn main()
{
}
