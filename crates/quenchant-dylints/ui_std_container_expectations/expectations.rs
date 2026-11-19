#![allow(dead_code, primitive_signature, specification_present)]

struct Fields
{
    #[expect(
        std_container,
        reason = "the fixture keeps one field to test its local expectation"
    )]
    names: Vec<u8>,
}

#[expect(
    std_container,
    reason = "the fixture keeps one alias to test its local expectation"
)]
type Names = Vec<u8>;

struct Holder;

#[expect(
    std_container,
    reason = "the fixture keeps one implementation to test its local expectation"
)]
impl Holder
{
    fn names() -> Vec<u8>
    {
        Vec::new()
    }
}

#[expect(
    std_container,
    reason = "the fixture keeps one function to test its local expectation"
)]
fn function() -> Vec<u8>
{
    Vec::new()
}

#[expect(
    std_container,
    reason = "the fixture has no container at this expectation"
)]
type Unmet = u8;

fn main()
{
}
