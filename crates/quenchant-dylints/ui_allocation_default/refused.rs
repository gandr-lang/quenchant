//! The default holds each function of this crate. A call that reaches an
//! allocation, in a function whose `# Cost` declares none, is refused at its
//! origin.

// Each allocation of a function that declares none: refused.

fn pushed(
    values: &mut Vec<u32>,
    value: u32,
)
{
    values.push(value);
}

fn formatted(value: u32) -> String
{
    format!("{value}")
}

fn called(callback: &dyn Fn(u32) -> u32) -> u32
{
    callback(1)
}

fn through_a_closure() -> usize
{
    let make = || vec![1_u8];
    make().len()
}

#[derive(Clone)]
struct Held
{
    values: Vec<u32>,
}

fn copied(held: &Held) -> usize
{
    held.clone().values.len()
}

// An allocation line in neither form: refused at its function, and its body
// is not read.

/// # Cost
/// - allocation: the values, because it is simple
fn unreasoned_line() -> Vec<u32>
{
    vec![1]
}

// An exception the site does not state: refused at the crate root.

#[expect(allocation_free_region)]
fn unexplained() -> Vec<u32>
{
    vec![1]
}

#[expect(
    allocation_free_region,
    reason = "an expectation on a module states no site's exception"
)]
mod inherited
{
    pub fn boxed() -> Box<u32>
    {
        Box::new(1)
    }
}

#[allow(allocation_free_region)]
fn allowed() -> Vec<u32>
{
    vec![1]
}

fn main()
{
    let mut values = Vec::new();
    pushed(&mut values, 1);
    drop(formatted(1));
    let _value = called(&|value| value);
    let _length = through_a_closure();
    let _copied = copied(&Held { values: Vec::new() });
    drop(unreasoned_line());
    drop(unexplained());
    drop(inherited::boxed());
    drop(allowed());
}
