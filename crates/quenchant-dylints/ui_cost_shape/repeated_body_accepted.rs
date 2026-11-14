// Accepted: functions that nothing repeats, reads a repeated call cannot tie
// to one value, and calls whose values leave their pass or allocate nothing.
#![allow(dead_code, unused_variables)]

// Accepted: straight code alone calls `once`.
fn once(value: u32) -> usize
{
    let text = format!("v{value}");
    text.len()
}

fn caller(value: u32) -> usize
{
    once(value) + 1
}

// Accepted: a loop calls `holds`, but each call can hand it a different list.
fn holds(
    list: &[u32],
    value: u32,
) -> bool
{
    list.contains(&value)
}

fn count(
    lists: &[Vec<u32>],
    value: u32,
) -> usize
{
    let mut found = 0;
    for list in lists {
        if holds(list, value) {
            found += 1;
        }
    }
    found
}

// Accepted: an inherent method of an iterator type plays no role.
struct Labels
{
    count: u32,
}

impl Labels
{
    fn label(&self) -> usize
    {
        let text = format!("v{}", self.count);
        text.len()
    }
}

// Accepted: each pass pushes the vector that `make` allocates.
fn make(value: u32) -> Vec<u32>
{
    vec![value, value + 1]
}

fn made(values: &[u32]) -> Vec<Vec<u32>>
{
    let mut out = Vec::new();
    for value in values {
        out.push(make(*value));
    }
    out
}

// Accepted: `empty` returns a vector that it never allocates.
fn empty() -> Vec<u32>
{
    Vec::new()
}

fn empties(values: &[u32]) -> usize
{
    let mut total = 0;
    for value in values {
        let held = empty();
        total += held.len();
    }
    total
}

// Accepted: each pass pushes the item that the iterator allocates.
struct Counter
{
    left: u32,
}

impl Iterator for Counter
{
    type Item = Vec<u32>;

    fn next(&mut self) -> Option<Vec<u32>>
    {
        if self.left == 0 {
            return None;
        }
        self.left -= 1;
        Some(vec![self.left])
    }
}

fn kept(left: u32) -> Vec<Vec<u32>>
{
    let mut out = Vec::new();
    for item in (Counter { left }) {
        out.push(item);
    }
    out
}

// Accepted: the call follows the loop, so no loop repeats `tail`.
fn tail(value: u32) -> usize
{
    let text = format!("v{value}");
    text.len()
}

fn after(values: &[u32]) -> usize
{
    let mut total = 0;
    for value in values {
        total += *value as usize;
    }
    total + tail(0)
}

// Accepted: a loop calls `parse`, but `parse` makes its string only for the
// error that a `?` returns at once.
fn parse(text: &str) -> Result<u32, String>
{
    let value = text
        .parse::<u32>()
        .map_err(|error| format!("{text}: {error}"))?;
    Ok(value + 1)
}

fn parsed(texts: &[&str]) -> Result<u32, String>
{
    let mut total = 0;
    for text in texts {
        total += parse(text)?;
    }
    Ok(total)
}

// Accepted: `checked` makes a string only for the error that it returns, so a
// call returns no value that it allocates.
fn checked(value: u32) -> Result<u32, String>
{
    if value > 9 {
        return Err(format!("{value} is too large"));
    }
    Ok(value)
}

fn total(values: &[u32]) -> u32
{
    let mut sum = 0;
    for value in values {
        if let Ok(found) = checked(*value) {
            sum += found;
        }
    }
    sum
}

// Accepted: `opened` returns the result of `map_err`, whose closure makes a
// string only for the error, and a loop calls `opened`.
fn opened(text: &str) -> Result<u32, String>
{
    text.parse::<u32>()
        .map_err(|error| format!("{text}: {error}"))
}

fn opens(texts: &[&str]) -> usize
{
    let mut count = 0;
    for text in texts {
        if opened(text).is_ok() {
            count += 1;
        }
    }
    count
}

// Accepted: `verified` returns what `checked` returns, and `checked` makes a
// string only for its error.
fn verified(value: u32) -> Result<u32, String>
{
    checked(value)
}

fn verifies(values: &[u32]) -> usize
{
    let mut count = 0;
    for value in values {
        if verified(*value).is_ok() {
            count += 1;
        }
    }
    count
}

fn main()
{
}
