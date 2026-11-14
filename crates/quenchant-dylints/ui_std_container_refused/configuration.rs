//! A configuration whose crates carry a number, an empty entry and the bare
//! `*`: each is a defect at the crate root, and no crate is listed.

fn names() -> Vec<u8>
{
    vec![1_u8, 2]
}

fn main()
{
    drop(names());
}
