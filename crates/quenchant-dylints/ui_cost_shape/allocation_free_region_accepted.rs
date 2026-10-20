// Accepted: marked functions that reach no allocation, unmarked functions
// that do, and a site excused by `expect`.
#![allow(dead_code, unused_variables)]

// Accepted: slice reads and arithmetic live in `core`.
/// # Cost
/// - allocation: none in steady state.
fn sums(values: &[u32]) -> u32
{
    values
        .iter()
        .fold(0, |total, value| total.wrapping_add(*value))
}

// Accepted: the panic path ends the steady state.
/// # Cost
/// - allocation: none in steady state.
fn checked(
    values: &[u32],
    at: usize,
) -> u32
{
    if at >= values.len() {
        panic!("index {at} past {}", values.len());
    }
    values[at]
}

/// A helper that allocates nothing, called from the marked body.
fn halve(value: u32) -> u32
{
    value / 2
}

// Accepted: a local helper whose body reaches no allocation.
/// # Cost
/// - allocation: none in steady state.
fn through_a_helper(value: u32) -> u32
{
    halve(value)
}

// Accepted: one helper reached along two paths is read once.
/// # Cost
/// - allocation: none in steady state.
fn twice(value: u32) -> u32
{
    halve(value) + through_a_helper(value)
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

// Accepted: a marked trait method of a type parameter is trusted, its
// implementations checked where they are written.
/// # Cost
/// - allocation: none in steady state.
fn through_a_parameter<B: Bounded>(
    step: &B,
    value: u32,
) -> u32
{
    step.advance(value)
}

/// A step that keeps its declaration's clause.
struct Doubling;

impl Bounded for Doubling
{
    // Accepted: the implementation reaches no allocation.
    fn advance(
        &self,
        value: u32,
    ) -> u32
    {
        value.wrapping_mul(2)
    }
}

// Accepted: a `# Cost` section that states another clause marks nothing.
/// # Cost
/// - time: linear in the entries.
fn unmarked(entries: &mut Vec<u32>)
{
    entries.push(0);
}

// Accepted: the one allocating site is excused where it is written.
/// # Cost
/// - allocation: none in steady state.
fn excused(
    entries: &mut Vec<u32>,
    entry: u32,
)
{
    #[expect(
        allocation_free_region,
        reason = "the caller reserves capacity before the region"
    )]
    entries.push(entry);
}

fn main()
{
}
