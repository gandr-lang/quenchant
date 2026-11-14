//! A configuration of the default with a number, an empty entry and the bare
//! `*` among its crates, a base directory that does not exist, and a count
//! directory that is a number: each is refused at the crate root, and no
//! entry turns the default round for this crate.

fn pushed(
    values: &mut Vec<u32>,
    value: u32,
)
{
    values.push(value);
}

fn main()
{
    let mut values = Vec::new();
    pushed(&mut values, 1);
}
