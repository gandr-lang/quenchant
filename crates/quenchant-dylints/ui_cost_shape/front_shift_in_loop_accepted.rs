// Accepted: front operations whose cost does not grow with a loop, and
// operations that shift nothing or belong to a type whose front is cheap.
#![allow(dead_code, unused_variables)]

use std::collections::VecDeque;

// Accepted: a single shift outside any loop.
fn single_shift(mut values: Vec<u32>) -> u32
{
    values.remove(0)
}

// Accepted: an index other than the front.
fn interior_index(mut values: Vec<u32>)
{
    while values.len() > 1 {
        values.remove(1);
    }
}

// Accepted: a full drain or an interior range shifts nothing it keeps.
fn full_and_interior_drain(buffer: &mut Vec<u32>)
{
    loop {
        let all: Vec<u32> = buffer.drain(..).collect();
        let middle: Vec<u32> = buffer.drain(1 .. 3).collect();
        if all.is_empty() && middle.is_empty() {
            break;
        }
    }
}

// Accepted: a deque's front is constant-time.
fn deque_front(mut queue: VecDeque<u32>)
{
    while !queue.is_empty() {
        let next = queue.remove(0);
        queue.push_front(1);
    }
}

// Accepted: the buffer is bound inside the loop.
fn bound_inside(values: &[u32])
{
    for value in values {
        let mut local = vec![*value];
        local.insert(0, 0);
    }
}

// Accepted: the loop walks an array, so it runs a fixed number of times.
fn fixed_trip(mut values: Vec<u32>)
{
    for value in [1_u32, 2] {
        values.insert(0, value);
    }
}

fn main()
{
}
