//! The default holds each function of this crate. Each function declares its
//! allocation, reaches none, or keeps it at a site that states why.

// Each of the five whys.

/// # Cost
/// - allocation: the values, setup
fn built() -> Vec<u32>
{
    vec![1, 2]
}

/// # Cost
/// - allocation: the values, growth within the length of the input.
fn grown(
    values: &mut Vec<u32>,
    value: u32,
)
{
    values.push(value);
}

/// # Cost
/// - allocation: the message, error path
fn failed(value: u32) -> Result<u32, String>
{
    value
        .checked_sub(1)
        .ok_or_else(|| format!("{value} is zero"))
}

/// # Cost
/// - allocation: the text, diagnostic
fn shown(value: u32) -> String
{
    value.to_string()
}

/// # Cost
/// - allocation: the verdict, durable by its content key
fn kept(value: u32) -> Box<u32>
{
    Box::new(value)
}

// A call of a function that declares its allocation: that function answers
// for itself.

fn caller() -> usize
{
    built().len()
}

// A function that reaches no allocation.

fn summed(values: &[u32]) -> u32
{
    values.iter().copied().fold(0, u32::wrapping_add)
}

// A generic root: a trait method of its type parameter is read where a
// caller's instance resolves it.

fn rendered<T: ToString>(value: &T) -> usize
{
    value.to_string().len()
}

// A site that keeps its allocation and states why.

fn statement() -> usize
{
    #[expect(allocation_free_region, reason = "the fixture's table, built once")]
    let table = vec![1_u8, 2];
    table.len()
}

#[expect(
    allocation_free_region,
    reason = "the function builds the fixture's table"
)]
fn item() -> Vec<u8>
{
    vec![1]
}

fn main()
{
    let mut values = Vec::new();
    grown(&mut values, 1);
    let _failed = failed(0);
    drop(shown(1));
    drop(kept(1));
    let _length = caller();
    let _sum = summed(&values);
    let _rendered = rendered(&1_u32);
    let _table = statement();
    drop(item());
}
