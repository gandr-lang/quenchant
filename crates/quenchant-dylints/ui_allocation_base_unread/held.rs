//! Outside the default, a unit whose base holds no record of it, or a record
//! whose hashes carry another scheme, takes no base. No function is held.

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
}
