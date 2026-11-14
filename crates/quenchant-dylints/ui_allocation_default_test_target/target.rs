//! A test target reads only the allocations whose sites state an
//! expectation.

// A library site that states its exception: the expectation holds in the
// test build too.

fn table() -> usize
{
    #[expect(allocation_free_region, reason = "the fixture's table, built once")]
    let table = vec![1_u8, 2];
    table.len()
}

// A site that states no expectation: not read in a test target.

fn pushed(
    values: &mut Vec<u32>,
    value: u32,
)
{
    values.push(value);
}

// A test: not read.

#[test]
fn reads()
{
    let mut values = vec![1];
    pushed(&mut values, 2);
    assert_eq!(table(), 2);
}
