// Refused: throwaways and repeated reads in a function that a loop, a role or
// a repeated caller repeats, one function per way the machinery reaches it.
#![allow(dead_code, unused_variables)]

// Refused: an `Iterator::next` runs once for each element, and each run
// formats a label that it drops.
struct Labels
{
    count: u32,
    total: usize,
}

impl Iterator for Labels
{
    type Item = usize;

    fn next(&mut self) -> Option<usize>
    {
        if self.count == 0 {
            return None;
        }
        self.count -= 1;
        let label = format!("v{}", self.count);
        self.total += label.len();
        Some(self.total)
    }
}

// Refused: an `Iterator::next` searches the iterator's own list on each run.
struct Fresh
{
    seen: Vec<u32>,
    next: u32,
}

impl Iterator for Fresh
{
    type Item = u32;

    fn next(&mut self) -> Option<u32>
    {
        let candidate = self.next;
        self.next += 1;
        if self.seen.contains(&candidate) {
            None
        }
        else {
            Some(candidate)
        }
    }
}

// Refused: a loop calls `measure`, which calls `describe`; each call of
// `describe` formats a string that it drops.
fn describe(value: u32) -> usize
{
    let text = format!("v{value}");
    text.len()
}

fn measure(value: u32) -> usize
{
    describe(value) + 1
}

fn sum(values: &[u32]) -> usize
{
    let mut total = 0;
    for value in values {
        total += measure(*value);
    }
    total
}

// Refused: a loop inside a closure calls `render`, which counts against the
// function that holds the closure.
fn render(value: u32) -> usize
{
    let text = value.to_string();
    text.len()
}

fn render_group(group: &[u32]) -> usize
{
    let count = |values: &[u32]| {
        let mut total = 0;
        for value in values {
            total += render(*value);
        }
        total
    };
    count(group)
}

// Refused: a loop calls a method that returns a copy of a field, and each pass
// only reads the copy.
struct Index
{
    names: Vec<String>,
}

impl Index
{
    fn names(&self) -> Vec<String>
    {
        self.names.clone()
    }
}

fn hits(
    index: &Index,
    queries: &[String],
) -> usize
{
    let mut hits = 0;
    for query in queries {
        let names = index.names();
        if names.len() > query.len() {
            hits += 1;
        }
    }
    hits
}

// Refused: a loop calls `label`, which grows the string that it returns
// through `push_str`, and each pass drops that string.
fn label(value: u32) -> String
{
    let mut text = String::new();
    text.push_str(if value > 9 { "large" } else { "small" });
    text
}

fn labelled(values: &[u32]) -> usize
{
    let mut total = 0;
    for value in values {
        total += label(*value).len();
    }
    total
}

// Refused: `relabel` returns the string that `label` grows, so each call of
// `relabel` returns a string that it allocates.
fn relabel(value: u32) -> String
{
    label(value)
}

fn relabelled(values: &[u32]) -> usize
{
    let mut total = 0;
    for value in values {
        total += relabel(*value).len();
    }
    total
}

fn main()
{
}
