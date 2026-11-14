//! The count holds each function of this crate, outside the default, and
//! refuses none: two reach the allocator, one a signature that owns heap
//! memory and one a call through `dyn`; two reach no allocation, and one
//! declares its allocation.

fn pushed(
    values: &mut Vec<u32>,
    value: u32,
)
{
    values.push(value);
}

fn boxed(value: u32) -> Box<u32>
{
    Box::new(value)
}

fn variable() -> bool
{
    std::env::var("QUENCHANT_FIXTURE").is_ok()
}

fn called(callback: &dyn Fn(u32) -> u32) -> u32
{
    callback(1)
}

fn summed(values: &[u32]) -> u32
{
    values.iter().copied().fold(0, u32::wrapping_add)
}

/// # Cost
/// - allocation: the values, setup
fn built() -> Vec<u32>
{
    vec![1, 2]
}

fn main()
{
    let mut values = built();
    pushed(&mut values, 1);
    drop(boxed(1));
    let _set = variable();
    let _value = called(&|value| value);
    let _sum = summed(&values);
}
