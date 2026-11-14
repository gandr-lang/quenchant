// rustc-env:CARGO_MANIFEST_DIR=/checkout/base
//! Outside the default, a function whose body hash differs from the base's
//! record, or that the base's record lacks, is held; one whose hash is the
//! base's is not. This crate's text is the same as the base fixture's, so
//! its records are the base of that fixture. The two build under two manifest
//! directories, and `located` reads its own, so its hash holds no path.

fn kept(values: &mut Vec<u32>)
{
    values.push(1);
}

fn changed(values: &mut Vec<u32>)
{
    values.push(2);
}

fn fresh(values: &mut Vec<u32>)
{
    values.push(3);
}

fn main()
{
    let mut values = Vec::new();
    kept(&mut values);
    changed(&mut values);
    fresh(&mut values);
    let mut directories = Vec::new();
    located(&mut directories);
}

fn located(directories: &mut Vec<&'static str>)
{
    directories.push(env!("CARGO_MANIFEST_DIR"));
}
