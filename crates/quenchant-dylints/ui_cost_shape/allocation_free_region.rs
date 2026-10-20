// Refused: functions marked allocation-free in steady state whose calls reach
// an allocation, one function per path.
#![allow(dead_code, unused_variables)]

use std::collections::HashMap;

// Refused: a push into an owned vector reaches the allocator when it grows.
/// # Cost
/// - allocation: none in steady state.
fn pushes(
    entries: &mut Vec<u32>,
    entry: u32,
)
{
    entries.push(entry);
}

/// Grows its argument; unmarked, so only its callers answer for it.
fn grow(entries: &mut Vec<u32>)
{
    entries.push(0);
}

// Refused: an allocation two calls deep, reported at the call in the marked
// body.
/// # Cost
/// - allocation: none in steady state
fn through_a_helper(entries: &mut Vec<u32>)
{
    grow(entries);
}

// Refused: a closure called from the marked body formats a string.
/// # Cost
/// - allocation: none in steady state.
fn through_a_closure(value: u32) -> usize
{
    let render = |held: u32| format!("{held}").len();
    render(value)
}

// Refused: a call through a trait object can reach any implementation.
/// # Cost
/// - allocation: none in steady state.
fn through_dyn(
    step: &dyn Fn(u32) -> u32,
    value: u32,
) -> u32
{
    step(value)
}

/// A step the region calls on a type parameter, without the clause.
trait Step
{
    /// Advance one value.
    fn advance(
        &self,
        value: u32,
    ) -> u32;
}

// Refused: an unmarked trait method of a type parameter can allocate.
/// # Cost
/// - allocation: none in steady state.
fn through_a_parameter<S: Step>(
    step: &S,
    value: u32,
) -> u32
{
    step.advance(value)
}

/// A step held to the clause by its declaration.
trait Bounded
{
    /// Advance one value without allocating.
    ///
    /// # Cost
    /// - allocation: none in steady state.
    fn advance(
        &self,
        value: u32,
    ) -> u32;
}

/// A step whose implementation breaks its declaration's clause.
struct Recording(Vec<u32>);

impl Bounded for Recording
{
    // Refused: the implementation of a marked declaration answers for it.
    fn advance(
        &self,
        value: u32,
    ) -> u32
    {
        let copy = self.0.clone();
        copy.len() as u32 + value
    }
}

// Refused: inserting into a map reaches the allocator.
/// # Cost
/// - allocation: none in steady state.
fn inserts(
    table: &mut HashMap<u32, u32>,
    key: u32,
)
{
    table.insert(key, key);
}

fn main()
{
}
