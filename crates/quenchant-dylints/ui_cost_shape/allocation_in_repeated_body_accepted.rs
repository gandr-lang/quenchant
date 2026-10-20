// Accepted: allocations that leave their pass, do not repeat, never allocate,
// diverge, or belong to another lint.
#![allow(dead_code, unused_variables)]

struct Rows
{
    names: Vec<String>,
}

// Accepted: each pass's string is pushed into a vector made outside the loop.
fn pushed(values: &[u32]) -> Vec<String>
{
    let mut names = Vec::new();
    for value in values {
        let name = format!("v{value}");
        names.push(name);
    }
    names
}

// Accepted: a per-element closure returns its allocation as the element.
fn mapped(values: &[u32]) -> Vec<String>
{
    values.iter().map(|value| value.to_string()).collect()
}

// Accepted: each pass's buffer is stored in a field.
fn stored(
    rows: &mut Rows,
    values: &[u32],
)
{
    for value in values {
        let mut name = String::with_capacity(4);
        name.push('v');
        rows.names.push(name);
    }
}

// Accepted: the buffer is returned from the pass that made it.
fn returned(values: &[u32]) -> Option<Vec<u32>>
{
    for value in values {
        if *value > 3 {
            let found = vec![*value, 0];
            return Some(found);
        }
    }
    None
}

// Accepted: a buffer made by `Vec::new` and never grown never allocates.
fn never_grown(values: &[u32]) -> usize
{
    let mut total = 0;
    for value in values {
        let empty: Vec<u32> = Vec::new();
        total += empty.len();
    }
    total
}

// Accepted: one allocation outside any loop.
fn once(value: u32) -> usize
{
    format!("v{value}").len()
}

// Accepted: a loop over an array runs a fixed number of passes.
fn fixed() -> usize
{
    let mut total = 0;
    for value in [1, 2, 3] {
        total += value.to_string().len();
    }
    total
}

// Accepted: the allocation is a panic's message.
fn diverging(values: &[u32])
{
    for value in values {
        if *value == 0 {
            panic!("{}", value.to_string());
        }
    }
}

// Accepted: a `Box` of a zero-sized value allocates nothing.
fn zero_sized(count: u32)
{
    for _ in 0 .. count {
        let unit = Box::new(());
        drop(unit);
    }
}

// Accepted: a local type's `new` is not a standard allocation.
struct Counter(u32);

impl Counter
{
    fn new() -> Self
    {
        Self(0)
    }
}

fn local_new(values: &[u32]) -> u32
{
    let mut total = 0;
    for value in values {
        let counter = Counter::new();
        total += counter.0 + value;
    }
    total
}

/// A failure's message, copied from a borrowed string.
struct Message(usize);

impl Message
{
    fn from(text: &str) -> Self
    {
        Self(text.len())
    }
}

// Accepted: the string is made only for the error a `?` returns at once.
fn on_the_error_path(values: &[&str]) -> Result<u32, Message>
{
    let mut total = 0;
    for value in values {
        total += value
            .parse::<u32>()
            .map_err(|error| Message::from(&error.to_string()))?;
    }
    Ok(total)
}

/// A failure that owns its cause.
struct Owned(Box<u32>);

// Accepted: a box made inside a closure called once is moved into the
// closure's result, which leaves the pass.
fn wrapped(values: &[u32]) -> Vec<Result<u32, Owned>>
{
    let mut out = Vec::new();
    for value in values {
        out.push(value.checked_sub(1).ok_or_else(|| Owned(Box::new(*value))));
    }
    out
}

// Accepted: each pass's word buffer is handed on by `mem::take`.
fn words(lines: &[&str]) -> Vec<String>
{
    let mut out = Vec::new();
    for line in lines {
        let mut word = String::new();
        for character in line.chars() {
            if character == ' ' {
                out.push(core::mem::take(&mut word));
            }
            else {
                word.push(character);
            }
        }
    }
    out
}

fn main()
{
}
