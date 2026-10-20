// Accepted: copies that are cheap by type, leave their pass, or do not repeat.
#![allow(dead_code, unused_variables)]

use std::collections::BTreeMap;
use std::rc::Rc;

#[derive(Clone, Copy)]
struct Point
{
    x: u32,
    y: u32,
}

#[derive(Clone)]
struct Plain
{
    point: Point,
    label: &'static str,
}

// Accepted: a `Copy` value.
fn copied(points: &[Point]) -> u32
{
    let mut total = 0;
    for point in points {
        let copy = point.clone();
        total += copy.x;
    }
    total
}

// Accepted: a type of plain fields copies its bytes.
fn plain(
    held: &Plain,
    rounds: u32,
) -> u32
{
    let mut total = 0;
    for _ in 0 .. rounds {
        let copy = held.clone();
        total += copy.point.y;
    }
    total
}

// Accepted: an `Rc` clone bumps a count.
fn counted(
    shared: &Rc<Vec<u32>>,
    rounds: u32,
) -> usize
{
    let mut total = 0;
    for _ in 0 .. rounds {
        let handle = Rc::clone(shared);
        total += handle.len();
    }
    total
}

// Accepted: a type parameter's copy is unknown at the site.
fn generic<T: Clone>(
    held: &T,
    rounds: u32,
) -> u32
{
    let mut total = 0;
    for _ in 0 .. rounds {
        let copy = held.clone();
        total += 1;
    }
    total
}

// Accepted: the copy is pushed into a vector made outside the loop.
fn pushed(names: &[String]) -> Vec<String>
{
    let mut out = Vec::new();
    for name in names {
        out.push(name.clone());
    }
    out
}

// Accepted: one copy outside any loop.
fn once(names: &Vec<String>) -> usize
{
    let copy = names.clone();
    copy.len()
}

// Accepted: the copy a closure returns is stored in the map that called it.
fn stored_by_entry(
    labels: &[(u32, String)],
    fallback: &String,
) -> BTreeMap<u32, String>
{
    let mut named = BTreeMap::new();
    for (key, label) in labels {
        named
            .entry(*key)
            .or_insert_with(|| fallback.clone())
            .push_str(label);
    }
    named
}

fn main()
{
}
