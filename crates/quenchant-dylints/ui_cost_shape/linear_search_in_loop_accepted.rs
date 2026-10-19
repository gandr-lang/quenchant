// Accepted: searches whose cost does not grow with a loop, and receivers whose
// type makes the search something other than a linear scan.
#![allow(dead_code, unused_variables)]

use std::collections::BTreeSet;

use anodized::spec;

static RESERVED: &[&str] = &["fn", "let"];

struct Span
{
    start: u32,
    end: u32,
}

impl Span
{
    fn contains(
        &self,
        offset: u32,
    ) -> bool
    {
        self.start <= offset && offset < self.end
    }
}

fn make() -> Vec<u32>
{
    vec![1, 2, 3]
}

// Accepted: no loop repeats the search.
fn straight(
    values: &[u32],
    needle: u32,
) -> bool
{
    values.contains(&needle)
}

// Accepted: an array's length is fixed by its type.
fn array_receiver(candidates: &[u32])
{
    let small = [1_u32, 2, 3];
    for candidate in candidates {
        if small.contains(candidate) {
            return;
        }
    }
}

// Accepted: the loop walks an array, so it runs a fixed number of times.
fn fixed_trip(values: &[u32])
{
    for needle in [1_u32, 2, 3] {
        if values.contains(&needle) {
            return;
        }
    }
}

// Accepted: the collection is bound inside the loop.
fn bound_inside(candidates: &[u32])
{
    for candidate in candidates {
        let fresh = make();
        if fresh.contains(candidate) {
            return;
        }
    }
}

// Accepted: a static item's length is fixed when the program is built.
fn static_receiver(words: &[&str])
{
    for word in words {
        if RESERVED.contains(word) {
            return;
        }
    }
}

// Accepted: a value computed in place is rebuilt on every pass.
fn temporary_receiver(candidates: &[u32])
{
    for candidate in candidates {
        if make().contains(candidate) {
            return;
        }
    }
}

// Accepted: a predicate that is not a key comparison has no index.
fn unkeyed_predicate(
    spans: &[Span],
    offsets: &[u32],
)
{
    for offset in offsets {
        let hit = spans.iter().any(|span| span.contains(*offset));
    }
}

// Accepted: both sides of the comparison read the element.
fn both_sides_element(
    pairs: &[(u32, u32)],
    rounds: u32,
)
{
    for _ in 0 .. rounds {
        let same = pairs.iter().any(|pair| pair.0 == pair.1);
    }
}

// Accepted: a set's search is not linear.
fn set_receiver(
    known: &BTreeSet<u32>,
    candidates: &[u32],
)
{
    for candidate in candidates {
        if known.contains(candidate) {
            return;
        }
    }
}

// Accepted: `str`'s substring search is a different operation.
fn substring(
    text: &str,
    words: &[&str],
)
{
    for word in words {
        if text.contains(word) {
            return;
        }
    }
}

// Accepted: `Option::map` calls its closure once.
fn option_closure(
    values: Option<&Vec<u32>>,
    needle: u32,
) -> Option<bool>
{
    values.map(|values| values.contains(&needle))
}

// Accepted: a specification clause states its relation; its cost is
// `spec_clause_traversal`'s to police.
#[spec(ensures: |output| output.iter().all(|value| values.contains(value)))]
fn clause_relation(values: &[u32]) -> Vec<u32>
{
    values.to_vec()
}

// Accepted: the search is the value the loop returns, so it runs once.
fn returned_search(
    rows: &[u32],
    seen: &[u32],
) -> bool
{
    for row in rows {
        if *row == 0 {
            continue;
        }
        return seen.contains(row);
    }
    false
}

// Accepted: a `return` closes the block that searches.
fn searched_then_returned(
    pairs: &[(u32, u32)],
    keys: &[u32],
) -> Option<usize>
{
    for &(key, _) in pairs {
        if key > 9 {
            let position = keys.iter().position(|known| *known == key);
            return position;
        }
    }
    None
}

// Accepted: a `break` out of the loop closes the block that searches.
fn searched_then_broken(
    pairs: &[(u32, u32)],
    keys: &[u32],
) -> bool
{
    let mut found = false;
    for &(key, _) in pairs {
        if key > 9 {
            found = keys.contains(&key);
            break;
        }
    }
    found
}

fn main()
{
}
