//! A crate that the rule does not list: no container is refused.

fn names() -> Vec<u8>
{
    vec![1_u8, 2]
}

fn text() -> String
{
    format!("{}", 1_u8)
}

fn main()
{
    drop(names());
    drop(text());
}
